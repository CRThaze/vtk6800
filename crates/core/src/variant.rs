//! Board variants and their factory layouts. Layouts are bundled as TOML data
//! generated from the vendor tool; `default_keymap` turns one into a [`Keymap`].

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::model::{Entry, FnMode, KeyMapping, Keymap, Layer, LayerId};

/// The four SKUs: {ANSI, ISO} × {68-key, 66-key}. Shared across the Vortex
/// PC66/PC68 and KBt RE families; both use the same hfd.cn "M77" controller and
/// the same host protocol, so a KBt RE is just an alias of the matching SKU.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Variant {
    Ansi68,
    Iso68,
    Ansi66,
    Iso66,
}

const ANSI68_BASE: &str = include_str!("../data/ansi68_base_default.toml");
const ISO68_BASE: &str = include_str!("../data/iso68_base_default.toml");
const ANSI66_BASE: &str = include_str!("../data/ansi66_base_default.toml");
const ISO66_BASE: &str = include_str!("../data/iso66_base_default.toml");
// The Fn layer is position-identical across all four SKUs (media/F-keys/nav/
// firmware combos sit on shared slots), so they all share one file (named for
// the layer, not a variant). The 66-key SKUs lack the physical slot 92 (Win)
// key, so its firmware combo simply never renders; harmless, and the shared
// data stays a single source.
const SHARED_FN: &str = include_str!("../data/fn_default.toml");

#[derive(Deserialize)]
struct BaseFile {
    key: Vec<BaseKey>,
}

#[derive(Deserialize)]
struct BaseKey {
    slot: u8,
    name: String,
    hid: u8,
}

#[derive(Deserialize)]
struct FnFile {
    key: Vec<FnKey>,
}

#[derive(Deserialize)]
struct FnKey {
    slot: u8,
    class: String,
    code: Option<u8>,
}

impl Variant {
    pub const ALL: [Variant; 4] = [
        Variant::Ansi68,
        Variant::Iso68,
        Variant::Ansi66,
        Variant::Iso66,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Variant::Ansi68 => "ansi68",
            Variant::Iso68 => "iso68",
            Variant::Ansi66 => "ansi66",
            Variant::Iso66 => "iso66",
        }
    }

    pub fn parse(s: &str) -> Option<Variant> {
        // `-`/`_` are stripped, so `pc-68-ansi`, `kbt_re_68_iso`, etc. normalize.
        // The `pc…`/`re…`/`kbtre…` forms are the PC66/PC68 and KBt RE families,
        // which share the SKU.
        match s.to_ascii_lowercase().replace(['-', '_'], "").as_str() {
            "ansi68" | "pc68ansi" | "re68ansi" | "kbtre68ansi" => Some(Variant::Ansi68),
            "iso68" | "pc68iso" | "re68iso" | "kbtre68iso" => Some(Variant::Iso68),
            "ansi66" | "pc66ansi" | "re66ansi" | "kbtre66ansi" => Some(Variant::Ansi66),
            "iso66" | "pc66iso" | "re66iso" | "kbtre66iso" => Some(Variant::Iso66),
            _ => None,
        }
    }

    fn base_file(self) -> Result<&'static str> {
        Ok(match self {
            Variant::Ansi68 => ANSI68_BASE,
            Variant::Iso68 => ISO68_BASE,
            Variant::Ansi66 => ANSI66_BASE,
            Variant::Iso66 => ISO66_BASE,
        })
    }

    fn fn_file(self) -> Result<&'static str> {
        // All SKUs share the Fn layer.
        Ok(SHARED_FN)
    }

    /// The `(slot, name, hid)` triples of this variant's base layout, used for
    /// resolving key names and rendering.
    pub fn key_slots(self) -> Result<Vec<(u8, String, u8)>> {
        let base: BaseFile =
            toml::from_str(self.base_file()?).map_err(|e| Error::Config(e.to_string()))?;
        Ok(base
            .key
            .into_iter()
            .map(|k| (k.slot, k.name, k.hid))
            .collect())
    }

    /// Resolve a key to its hardware slot, by the layout's display name
    /// (e.g. "Caps", "esc") or by a HID keycode name (e.g. "capslock").
    pub fn slot_for_name(self, name: &str) -> Result<Option<u8>> {
        let n = name.to_ascii_lowercase();
        let slots = self.key_slots()?;
        if let Some((slot, _, _)) = slots.iter().find(|(_, kn, _)| kn.to_ascii_lowercase() == n) {
            return Ok(Some(*slot));
        }
        if let Some(hid) = crate::keycode::hid_from_name(&n) {
            if let Some((slot, _, _)) = slots.iter().find(|(_, _, h)| *h == hid) {
                return Ok(Some(*slot));
            }
        }
        Ok(None)
    }

    /// Build the factory-default keymap for this variant. Works for any variant
    /// with bundled base + Fn data; others report "not bundled yet".
    pub fn default_keymap(self) -> Result<Keymap> {
        build_default(self)
    }
}

fn build_default(variant: Variant) -> Result<Keymap> {
    let base: BaseFile =
        toml::from_str(variant.base_file()?).map_err(|e| Error::Config(e.to_string()))?;
    let fnf: FnFile =
        toml::from_str(variant.fn_file()?).map_err(|e| Error::Config(e.to_string()))?;

    let mut base_keys: Vec<KeyMapping> = base
        .key
        .into_iter()
        // Every base key is a normal key entry, including the Fn key (hid 0xAF):
        // the controller recognizes 0xAF as the layer-hold, and the vendor tool
        // encodes it identically (as key value 0xAF), so it is fully remappable.
        .map(|k| KeyMapping {
            slot: k.slot,
            entry: Entry::Key {
                modifier: 0,
                hid: k.hid,
            },
        })
        .collect();

    let mut fn_keys = Vec::new();
    for k in fnf.key {
        let entry = match (k.class.as_str(), k.code) {
            ("key", Some(c)) => Some(Entry::Key {
                modifier: 0,
                hid: c,
            }),
            ("consumer", Some(c)) => Some(Entry::Consumer { code: c }),
            // "firmware" combos are hardwired and non-remappable. They are not
            // stored in the (configurable) keymap; the layout view gets them as
            // a read-only overlay via `firmware_keys`.
            _ => None,
        };
        if let Some(entry) = entry {
            fn_keys.push(KeyMapping {
                slot: k.slot,
                entry,
            });
        }
    }

    // Keep every layer in ascending slot order (as `Layer::set` and the YAML
    // document format do), so a built default matches its serialized form.
    base_keys.sort_by_key(|k| k.slot);
    fn_keys.sort_by_key(|k| k.slot);

    Ok(Keymap {
        variant,
        fn_mode: FnMode::default(),
        macros: Vec::new(),
        layers: vec![
            Layer {
                id: LayerId::Base,
                keys: base_keys,
            },
            Layer {
                id: LayerId::Fn,
                keys: fn_keys,
            },
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi68_default_keymap_loads() {
        let km = Variant::Ansi68.default_keymap().unwrap();
        assert_eq!(km.variant, Variant::Ansi68);
        let base = km.layer(LayerId::Base).unwrap();
        assert_eq!(base.keys.len(), 68);
        // esc at slot 1 -> hid 0x29
        assert_eq!(
            base.get(1),
            Some(Entry::Key {
                modifier: 0,
                hid: 0x29
            })
        );
        // The Fn key is a normal, remappable key (hid 0xAF), not an overlay.
        assert_eq!(
            base.get(96),
            Some(Entry::Key {
                modifier: 0,
                hid: 0xAF
            })
        );
        // Fn layer has media keys
        let fnl = km.layer(LayerId::Fn).unwrap();
        assert!(fnl
            .keys
            .iter()
            .any(|k| matches!(k.entry, Entry::Consumer { .. })));
    }

    #[test]
    fn iso68_default_keymap_loads() {
        let km = Variant::Iso68.default_keymap().unwrap();
        assert_eq!(km.variant, Variant::Iso68);
        let base = km.layer(LayerId::Base).unwrap();
        assert_eq!(base.keys.len(), 69);
        // ISO drops slot 67 and adds slots 110 + 111. The controller indexes
        // these two reversed vs their left-to-right order, so slot 110 must be
        // the "<>|" key (hid 0x64) and slot 111 the "#~" key (hid 0x32).
        assert!(base.get(67).is_none());
        assert_eq!(
            base.get(110),
            Some(Entry::Key {
                modifier: 0,
                hid: 0x64
            })
        );
        assert_eq!(
            base.get(111),
            Some(Entry::Key {
                modifier: 0,
                hid: 0x32
            })
        );
        // The formerly-"firmware" Fn combos are now ordinary keys: Fn+Z = Menu.
        let fnl = km.layer(LayerId::Fn).unwrap();
        assert_eq!(
            fnl.get(74),
            Some(Entry::Key {
                modifier: 0,
                hid: 0x65
            })
        );
    }

    #[test]
    fn parses_variant_aliases() {
        // Canonical forms plus the PC66/PC68 and KBt RE family aliases.
        assert_eq!(Variant::parse("ansi68"), Some(Variant::Ansi68));
        assert_eq!(Variant::parse("pc68ansi"), Some(Variant::Ansi68));
        assert_eq!(Variant::parse("re68ansi"), Some(Variant::Ansi68));
        assert_eq!(Variant::parse("KBt-RE-68-ISO"), Some(Variant::Iso68));
        assert_eq!(Variant::parse("iso68"), Some(Variant::Iso68));
        assert_eq!(Variant::parse("pc66iso"), Some(Variant::Iso66));
        assert_eq!(Variant::parse("re66ansi"), Some(Variant::Ansi66));
        // Bare family names (no key count / no layout) are not accepted.
        assert_eq!(Variant::parse("kbtre"), None);
        assert_eq!(Variant::parse("pc68"), None);
        assert_eq!(Variant::parse("iso"), None);
    }

    #[test]
    fn slot_lookup_and_roundtrip_toml() {
        let slot = Variant::Ansi68.slot_for_name("capslock").unwrap();
        assert_eq!(slot, Some(55));
        let km = Variant::Ansi68.default_keymap().unwrap();
        let s = toml::to_string(&km).unwrap();
        let back: Keymap = toml::from_str(&s).unwrap();
        assert_eq!(km, back);
    }
}
