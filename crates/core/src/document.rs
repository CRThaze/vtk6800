//! The on-disk keymap file format (YAML) and its validation.
//!
//! Assignments are grouped as `layers.{base,fn}`, each a map keyed by `slot:N`
//! (the same `slot:N` syntax the CLI accepts), which is friendlier to hand-edit
//! and discover than the flat internal model. This module converts to/from
//! [`Keymap`] and validates on load:
//!   * slot keys are well-formed (`slot:N`) and not duplicated within a layer,
//!   * every slot is a real key on the target variant,
//!   * only `base`/`fn` layers and known top-level keys appear,
//!   * each entry carries the right fields for its `type` (via serde).

use std::collections::HashSet;
use std::fmt;

use serde::de::{self, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{Error, Result};
use crate::model::{Entry, FnMode, KeyMapping, Keymap, Layer, LayerId, Macro, MouseAction};
use crate::variant::Variant;

/// The on-disk form of a key [`Entry`]. Mirrors it, except a `key` may be given
/// by `name` (resolved to a hid) as well as, or instead of, a raw `hid`.
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum EntryDoc {
    Unassigned,
    Key {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hid: Option<u8>,
        #[serde(default, skip_serializing_if = "is_zero_u8")]
        modifier: u8,
    },
    Consumer {
        code: u8,
    },
    Media {
        code: u16,
    },
    // A macro binding is just the index; the wire count/repeat are derived from
    // the referenced macro's `times` on load (see `Keymap::resolve_macros`).
    Macro {
        index: u8,
    },
    Mouse {
        action: MouseAction,
    },
    Raw {
        bytes: [u8; 3],
    },
}

fn is_zero_u8(v: &u8) -> bool {
    *v == 0
}

/// Resolve a `key` entry's `name`/`hid` to a single validated hid: the name (if
/// any) must be known and must agree with an explicit hid, and the result must
/// be a writable, non-media keyboard usage.
fn resolve_key_hid(name: Option<&str>, hid: Option<u8>) -> std::result::Result<u8, String> {
    let name_hid = match name {
        Some(n) => Some(
            crate::keycode::hid_from_name(n).ok_or_else(|| format!("unknown key name {n:?}"))?,
        ),
        None => None,
    };
    let hid = match (name_hid, hid) {
        (Some(nh), Some(h)) if nh != h => {
            return Err(format!(
                "name {:?} is hid {nh:#04x}, which does not match the given hid {h:#04x}",
                name.unwrap()
            ));
        }
        (Some(nh), _) => nh,
        (None, Some(h)) => h,
        (None, None) => return Err("a `type: key` entry needs a `name` or a `hid`".into()),
    };
    if crate::keycode::is_media_key_hid(hid) {
        return Err(format!(
            "hid {hid:#04x} is a media key; use a `type: consumer` entry instead"
        ));
    }
    if !crate::keycode::is_writable_key_hid(hid) {
        return Err(format!("hid {hid:#04x} is reserved / not a keyboard usage"));
    }
    Ok(hid)
}

impl EntryDoc {
    fn into_entry(self) -> std::result::Result<Entry, String> {
        Ok(match self {
            EntryDoc::Unassigned => Entry::Unassigned,
            EntryDoc::Key {
                name,
                hid,
                modifier,
            } => Entry::Key {
                modifier,
                hid: resolve_key_hid(name.as_deref(), hid)?,
            },
            EntryDoc::Consumer { code } => Entry::Consumer { code },
            EntryDoc::Media { code } => Entry::Media { code },
            // count/repeat are filled by resolve_macros() after the whole
            // keymap (and its macro list) is built.
            EntryDoc::Macro { index } => Entry::Macro {
                index,
                repeat: 0,
                count: 0,
            },
            EntryDoc::Mouse { action } => Entry::Mouse { action },
            EntryDoc::Raw { bytes } => Entry::Raw { bytes },
        })
    }

    fn from_entry(e: Entry) -> EntryDoc {
        match e {
            Entry::Unassigned => EntryDoc::Unassigned,
            // Prefer a name; fall back to a raw hid for the few unnamed usages.
            Entry::Key { modifier, hid } => match crate::keycode::name_from_hid(hid) {
                Some(name) => EntryDoc::Key {
                    name: Some(name.to_string()),
                    hid: None,
                    modifier,
                },
                None => EntryDoc::Key {
                    name: None,
                    hid: Some(hid),
                    modifier,
                },
            },
            Entry::Consumer { code } => EntryDoc::Consumer { code },
            Entry::Media { code } => EntryDoc::Media { code },
            Entry::Macro { index, .. } => EntryDoc::Macro { index },
            Entry::Mouse { action } => EntryDoc::Mouse { action },
            Entry::Raw { bytes } => EntryDoc::Raw { bytes },
        }
    }
}

/// A layer body: `slot:N -> entry`. Deserialization rejects malformed slot keys
/// and duplicate slots; serialization emits `slot:N` keys in ascending order.
#[derive(Default)]
struct SlotEntries(Vec<(u8, EntryDoc)>);

fn parse_slot_key(key: &str) -> std::result::Result<u8, String> {
    key.strip_prefix("slot:")
        .and_then(|n| n.parse::<u8>().ok())
        .ok_or_else(|| format!("invalid key {key:?} (expected `slot:N`, N = 0..=255)"))
}

impl<'de> Deserialize<'de> for SlotEntries {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = SlotEntries;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a map of `slot:N` keys to key entries")
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<SlotEntries, A::Error> {
                // A custom visitor sees every pair from the YAML stream, so
                // duplicate `slot:N` keys are caught here (a map type would
                // silently keep only the last).
                let mut out = Vec::new();
                let mut seen = HashSet::new();
                while let Some(key) = map.next_key::<String>()? {
                    let slot = parse_slot_key(&key).map_err(de::Error::custom)?;
                    if !seen.insert(slot) {
                        return Err(de::Error::custom(format!(
                            "slot {slot} is defined more than once in the same layer"
                        )));
                    }
                    out.push((slot, map.next_value()?));
                }
                Ok(SlotEntries(out))
            }
        }
        d.deserialize_map(V)
    }
}

impl Serialize for SlotEntries {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let mut items: Vec<&(u8, EntryDoc)> = self.0.iter().collect();
        items.sort_by_key(|(slot, _)| *slot);
        let mut map = s.serialize_map(Some(items.len()))?;
        for (slot, entry) in items {
            map.serialize_entry(&format!("slot:{slot}"), entry)?;
        }
        map.end()
    }
}

fn slot_entries_is_empty(s: &SlotEntries) -> bool {
    s.0.is_empty()
}

#[derive(Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Layers {
    #[serde(default, skip_serializing_if = "slot_entries_is_empty")]
    base: SlotEntries,
    #[serde(default, rename = "fn", skip_serializing_if = "slot_entries_is_empty")]
    fn_layer: SlotEntries,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct KeymapDoc {
    variant: Variant,
    // The Fn-key behaviour setting. Always written (even the default
    // `momentary`) so the file documents the mode explicitly; still defaulted on
    // load so older files without the field parse.
    #[serde(default)]
    fn_mode: FnMode,
    // Stored macro bodies (index = the `macro:N` a key entry references).
    // Omitted when empty. `singleton_map_recursive` renders each event as a
    // friendly `key_down: 4` map instead of a YAML `!key_down` tag.
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        with = "serde_yaml::with::singleton_map_recursive"
    )]
    macros: Vec<Macro>,
    #[serde(default)]
    layers: Layers,
}

/// Parse and validate a YAML keymap document into a [`Keymap`].
pub fn from_yaml(yaml: &str) -> Result<Keymap> {
    let doc: KeymapDoc = serde_yaml::from_str(yaml).map_err(|e| Error::Config(e.to_string()))?;
    let valid: HashSet<u8> = doc
        .variant
        .key_slots()?
        .into_iter()
        .map(|(slot, _, _)| slot)
        .collect();

    let build = |entries: SlotEntries, layer: &str| -> Result<Vec<KeyMapping>> {
        let mut keys = Vec::with_capacity(entries.0.len());
        for (slot, doc_entry) in entries.0 {
            if !valid.contains(&slot) {
                return Err(Error::Config(format!(
                    "layer `{layer}`: slot {slot} is not a key on variant {}",
                    doc.variant.as_str()
                )));
            }
            // Resolve name/hid and validate the entry (media/reserved rejected).
            let entry = doc_entry
                .into_entry()
                .map_err(|m| Error::Config(format!("layer `{layer}`: slot {slot}: {m}")))?;
            keys.push(KeyMapping { slot, entry });
        }
        Ok(keys)
    };

    let base = build(doc.layers.base, "base")?;
    let fn_keys = build(doc.layers.fn_layer, "fn")?;

    // Validate the macro set eagerly (count / size limits, event encoding) so
    // an over-budget definition is caught on load, not at flash time.
    crate::macro_table::encode_macros(&doc.macros).map_err(|e| Error::Config(e.to_string()))?;
    // Any `macro:N` binding must reference a defined macro.
    for layer in [&base, &fn_keys] {
        for km in layer.iter() {
            if let Entry::Macro { index, .. } = km.entry {
                if index as usize >= doc.macros.len() {
                    return Err(Error::Config(format!(
                        "slot {} references macro {index}, but only {} macro(s) are defined",
                        km.slot,
                        doc.macros.len()
                    )));
                }
            }
        }
    }

    let mut km = Keymap {
        variant: doc.variant,
        fn_mode: doc.fn_mode,
        macros: doc.macros,
        layers: vec![
            Layer {
                id: LayerId::Base,
                keys: base,
            },
            Layer {
                id: LayerId::Fn,
                keys: fn_keys,
            },
        ],
    };
    // Fill each macro binding's wire count/repeat from the referenced macro.
    km.resolve_macros();
    Ok(km)
}

/// Serialize a [`Keymap`] to the YAML document format.
pub fn to_yaml(km: &Keymap) -> Result<String> {
    let mut layers = Layers::default();
    for layer in &km.layers {
        let entries = SlotEntries(
            layer
                .keys
                .iter()
                .map(|k| (k.slot, EntryDoc::from_entry(k.entry)))
                .collect(),
        );
        match layer.id {
            LayerId::Base => layers.base = entries,
            LayerId::Fn => layers.fn_layer = entries,
            LayerId::Alt => {} // the alt table isn't written yet
        }
    }
    let doc = KeymapDoc {
        variant: km.variant,
        fn_mode: km.fn_mode,
        macros: km.macros.clone(),
        layers,
    };
    serde_yaml::to_string(&doc).map_err(|e| Error::Config(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macro_times_drives_binding_count_and_round_trips() {
        let yaml = "\
variant: ansi68
fn_mode: momentary
macros:
  - times: 3
    events:
      - key_down: 4
      - key_up: 4
layers:
  base:
    slot:1: { type: macro, index: 0 }
";
        let km = from_yaml(yaml).unwrap();
        assert_eq!(km.macros[0].times, 3);
        // resolve_macros fills the binding's wire count/repeat from times=3.
        assert_eq!(
            km.layer(LayerId::Base).unwrap().get(1),
            Some(Entry::Macro {
                index: 0,
                repeat: 1,
                count: 3
            })
        );
        // count/repeat are derived (not serialized), so the file round-trips.
        assert!(!to_yaml(&km).unwrap().contains("count:"));
        assert_eq!(from_yaml(&to_yaml(&km).unwrap()).unwrap(), km);
    }

    #[test]
    fn round_trips_a_default_keymap() {
        let km = Variant::Ansi68.default_keymap().unwrap();
        let yaml = to_yaml(&km).unwrap();
        assert!(yaml.contains("variant: ansi68"));
        assert!(yaml.contains("slot:1:")); // esc
                                           // The seeded default is written with names, not raw hids.
        assert!(yaml.contains("name: esc"));
        assert!(!yaml.contains("hid:"));
        let back = from_yaml(&yaml).unwrap();
        assert_eq!(km, back);
    }

    #[test]
    fn fn_mode_round_trips_and_is_always_written() {
        // The default (momentary) is written explicitly and loads back as itself.
        let km = Variant::Ansi68.default_keymap().unwrap();
        let yaml = to_yaml(&km).unwrap();
        assert!(yaml.contains("fn_mode: momentary"));
        assert_eq!(from_yaml(&yaml).unwrap().fn_mode, FnMode::Momentary);

        // A file without the field still parses (defaults to momentary).
        let no_field = "variant: ansi68\nlayers:\n  base:\n    slot:1: { type: key, name: esc }\n";
        assert_eq!(from_yaml(no_field).unwrap().fn_mode, FnMode::Momentary);

        // A non-default mode is written and round-trips.
        let mut km2 = km.clone();
        km2.fn_mode = FnMode::Latch;
        let yaml2 = to_yaml(&km2).unwrap();
        assert!(yaml2.contains("fn_mode: latch"));
        assert_eq!(from_yaml(&yaml2).unwrap().fn_mode, FnMode::Latch);
    }

    #[test]
    fn key_by_name_resolves_and_cross_checks_hid() {
        let by_name = "variant: ansi68\nlayers:\n  base:\n    slot:1: { type: key, name: esc }\n";
        assert_eq!(
            from_yaml(by_name)
                .unwrap()
                .layer(LayerId::Base)
                .unwrap()
                .get(1),
            Some(Entry::Key {
                modifier: 0,
                hid: 0x29
            })
        );
        // name + matching hid is fine.
        let both_ok =
            "variant: ansi68\nlayers:\n  base:\n    slot:1: { type: key, name: esc, hid: 0x29 }\n";
        assert!(from_yaml(both_ok).is_ok());
        // name + conflicting hid is an error.
        let mismatch =
            "variant: ansi68\nlayers:\n  base:\n    slot:1: { type: key, name: esc, hid: 0x04 }\n";
        assert!(from_yaml(mismatch)
            .unwrap_err()
            .to_string()
            .contains("does not match"));
        // unknown name is an error.
        let unknown = "variant: ansi68\nlayers:\n  base:\n    slot:1: { type: key, name: florp }\n";
        assert!(from_yaml(unknown)
            .unwrap_err()
            .to_string()
            .contains("unknown key name"));
        // neither name nor hid is an error.
        let neither = "variant: ansi68\nlayers:\n  base:\n    slot:1: { type: key }\n";
        assert!(from_yaml(neither)
            .unwrap_err()
            .to_string()
            .contains("needs a `name` or a `hid`"));
    }

    #[test]
    fn parses_the_nested_shape_with_entry_types() {
        let yaml = "\
variant: ansi68
layers:
  base:
    slot:1:
      type: key
      hid: 0x29
  fn:
    slot:37:
      type: consumer
      code: 205
";
        let km = from_yaml(yaml).unwrap();
        assert_eq!(
            km.layer(LayerId::Base).unwrap().get(1),
            Some(Entry::Key {
                modifier: 0,
                hid: 0x29
            })
        );
        assert_eq!(
            km.layer(LayerId::Fn).unwrap().get(37),
            Some(Entry::Consumer { code: 205 })
        );
    }

    #[test]
    fn rejects_duplicate_slot() {
        let yaml = "\
variant: ansi68
layers:
  base:
    slot:1: { type: key, hid: 0x29 }
    slot:1: { type: key, hid: 0x04 }
";
        let err = from_yaml(yaml).unwrap_err().to_string();
        assert!(err.contains("more than once"), "got: {err}");
    }

    #[test]
    fn rejects_slot_not_on_variant() {
        // slot 200 is not a physical key on ansi68.
        let yaml = "\
variant: ansi68
layers:
  base:
    slot:200: { type: key, hid: 0x04 }
";
        let err = from_yaml(yaml).unwrap_err().to_string();
        assert!(err.contains("not a key on variant"), "got: {err}");
    }

    #[test]
    fn rejects_unknown_layer_and_bad_slot_key() {
        let bad_layer = "variant: ansi68\nlayers:\n  meta:\n    slot:1: { type: key, hid: 4 }\n";
        assert!(from_yaml(bad_layer).is_err());
        let bad_key = "variant: ansi68\nlayers:\n  base:\n    esc: { type: key, hid: 4 }\n";
        assert!(from_yaml(bad_key)
            .unwrap_err()
            .to_string()
            .contains("slot:N"));
    }

    #[test]
    fn rejects_wrong_fields_for_type() {
        // `type: key` requires `hid`; giving `code` instead is a missing field.
        let yaml = "variant: ansi68\nlayers:\n  base:\n    slot:1: { type: key, code: 5 }\n";
        assert!(from_yaml(yaml).is_err());
    }

    #[test]
    fn rejects_media_and_reserved_key_hids() {
        let media = "variant: ansi68\nlayers:\n  base:\n    slot:1: { type: key, hid: 0x80 }\n";
        assert!(from_yaml(media).unwrap_err().to_string().contains("media"));
        let reserved = "variant: ansi68\nlayers:\n  base:\n    slot:1: { type: key, hid: 0x01 }\n";
        assert!(from_yaml(reserved)
            .unwrap_err()
            .to_string()
            .contains("reserved"));
        // A formerly-unnamed but valid usage (F13 = 0x68) is accepted.
        let ok = "variant: ansi68\nlayers:\n  base:\n    slot:1: { type: key, hid: 0x68 }\n";
        assert!(from_yaml(ok).is_ok());
    }
}
