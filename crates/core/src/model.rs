//! Keymap data model. A [`Keymap`] is a set of [`Layer`]s; each layer maps a
//! hardware slot to an [`Entry`]. Entries encode to the 4-byte on-wire form
//! `[type, b1, b2, b3]` documented in PROTOCOL.md §5.

use serde::{Deserialize, Serialize};

use crate::protocol::cmd;
use crate::variant::Variant;

/// Serde helper: omit a `u8` field from output when it is the default `0`.
fn is_zero_u8(v: &u8) -> bool {
    *v == 0
}

/// A single key assignment. Serialized with an internal `type` tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Entry {
    /// No assignment (all-zero entry).
    Unassigned,
    /// Standard keyboard key (entry type 0x02).
    Key {
        #[serde(default, skip_serializing_if = "is_zero_u8")]
        modifier: u8,
        hid: u8,
    },
    /// Consumer control / media key (entry type 0x03).
    Consumer { code: u8 },
    /// System/media 16-bit code (entry type 0x01).
    Media { code: u16 },
    /// Macro reference (entry type 0x06).
    Macro {
        index: u8,
        #[serde(default)]
        repeat: u8,
        #[serde(default)]
        count: u8,
    },
    /// Mouse action (entry type 0x01, a 16-bit mouse code).
    Mouse { action: MouseAction },
    /// Raw three payload bytes (escape hatch).
    Raw { bytes: [u8; 3] },
}

impl Entry {
    /// Encode to the 4-byte on-wire entry `[type, b1, b2, b3]`.
    pub fn encode(&self) -> [u8; 4] {
        match *self {
            Entry::Unassigned => [0, 0, 0, 0],
            Entry::Key { modifier, hid } => [0x02, modifier, hid, 0],
            Entry::Consumer { code } => [0x03, code, 0, 0],
            Entry::Media { code } => [0x01, (code & 0xff) as u8, (code >> 8) as u8, 0],
            Entry::Macro {
                index,
                repeat,
                count,
            } => [0x06, index, repeat, count],
            Entry::Mouse { action } => {
                let c = action.code();
                [0x01, (c & 0xff) as u8, (c >> 8) as u8, 0]
            }
            Entry::Raw { bytes } => [bytes[0], bytes[1], bytes[2], 0],
        }
    }

    /// Short human description for CLI/TUI display.
    pub fn describe(&self) -> String {
        match *self {
            // Display-only marker for an empty slot. An em-dash (not a hyphen)
            // so it can't be mistaken for the `-`/minus key; the CLI still only
            // accepts `none` to *set* a slot unassigned.
            Entry::Unassigned => "—".into(),
            Entry::Key { modifier, hid } => match crate::keycode::name_from_hid(hid) {
                Some(n) if modifier == 0 => n.to_string(),
                Some(n) => format!("{n}+mod{modifier:#x}"),
                None => format!("hid:{hid:#04x}"),
            },
            Entry::Consumer { code } => format!("media:{code:#04x}"),
            Entry::Media { code } => format!("sys:{code:#06x}"),
            Entry::Macro { index, .. } => format!("macro#{index}"),
            Entry::Mouse { action } => format!("mouse:{}", action.as_str()),
            Entry::Raw { bytes } => format!("raw:{bytes:02x?}"),
        }
    }
}

/// One key's slot + assignment within a layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyMapping {
    pub slot: u8,
    #[serde(flatten)]
    pub entry: Entry,
}

/// Which physical layer a table targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerId {
    Base,
    Fn,
    Alt,
}

impl LayerId {
    /// The command byte that programs this layer.
    pub fn command(self) -> u8 {
        match self {
            LayerId::Base => cmd::LAYER_BASE,
            LayerId::Fn => cmd::LAYER_FN,
            LayerId::Alt => cmd::LAYER_ALT,
        }
    }

    /// The exact table byte-length this layer is sent as (from the decompile).
    pub fn table_size(self) -> usize {
        match self {
            LayerId::Base => 0x2b6, // 694
            LayerId::Fn => 0x2ac,   // 684
            LayerId::Alt => 0x280,  // 640
        }
    }

    /// Parse from a CLI name.
    pub fn parse(s: &str) -> Option<LayerId> {
        match s.to_ascii_lowercase().as_str() {
            "base" => Some(LayerId::Base),
            "fn" => Some(LayerId::Fn),
            "alt" => Some(LayerId::Alt),
            _ => None,
        }
    }
}

/// A programmable layer: slot → entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Layer {
    pub id: LayerId,
    pub keys: Vec<KeyMapping>,
}

impl Layer {
    pub fn get(&self, slot: u8) -> Option<Entry> {
        self.keys.iter().find(|k| k.slot == slot).map(|k| k.entry)
    }

    /// Insert or replace the entry for a slot.
    pub fn set(&mut self, slot: u8, entry: Entry) {
        match self.keys.iter_mut().find(|k| k.slot == slot) {
            Some(k) => k.entry = entry,
            None => self.keys.push(KeyMapping { slot, entry }),
        }
        self.keys.sort_by_key(|k| k.slot);
    }
}

/// How the physical Fn key behaves: hold-to-access (default) or tap-to-latch.
/// This is a keyboard *setting* (vendor `fn_switchfunction`), written by the
/// `04 17` command, separate from the keymap tables.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FnMode {
    /// Hold Fn to reach the Fn layer, release to leave (factory default).
    #[default]
    Momentary,
    /// Tap Fn to latch the Fn layer on, tap again to release.
    Latch,
}

impl FnMode {
    pub fn as_str(self) -> &'static str {
        match self {
            FnMode::Momentary => "momentary",
            FnMode::Latch => "latch",
        }
    }

    pub fn parse(s: &str) -> Option<FnMode> {
        match s.to_ascii_lowercase().as_str() {
            "momentary" | "hold" => Some(FnMode::Momentary),
            "latch" | "toggle" | "lock" => Some(FnMode::Latch),
            _ => None,
        }
    }

    pub fn is_momentary(self) -> bool {
        matches!(self, FnMode::Momentary)
    }
}

/// A mouse button, stored in the wire format as its HID button bitmask.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

impl MouseButton {
    /// HID mouse button bit (left = 1, right = 2, middle = 4).
    pub fn bit(self) -> u8 {
        match self {
            MouseButton::Left => 0x01,
            MouseButton::Right => 0x02,
            MouseButton::Middle => 0x04,
        }
    }
}

/// A mouse action bindable to a key (vendor "Mouse Function"). Encodes as a
/// type-`0x01` entry carrying a 16-bit code (byte1 = 0x01 button / 0x03 wheel,
/// byte2 = button mask or wheel delta). Verified against vendor exports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseAction {
    LeftClick,
    MiddleClick,
    RightClick,
    DoubleClick,
    ScrollUp,
    ScrollDown,
    Button4,
    Button5,
}

impl MouseAction {
    /// The 16-bit wire code.
    pub fn code(self) -> u16 {
        match self {
            MouseAction::LeftClick => 0x0101,
            MouseAction::MiddleClick => 0x0401,
            MouseAction::RightClick => 0x0201,
            MouseAction::DoubleClick => 0x0301,
            MouseAction::ScrollUp => 0x0103,
            MouseAction::ScrollDown => 0xff03,
            MouseAction::Button4 => 0x0801,
            MouseAction::Button5 => 0x1001,
        }
    }

    /// The vendor's 1-based action index (its `macro_value` for Mouse Function).
    pub fn from_vendor_index(v: u8) -> Option<MouseAction> {
        Some(match v {
            1 => MouseAction::LeftClick,
            2 => MouseAction::MiddleClick,
            3 => MouseAction::RightClick,
            4 => MouseAction::DoubleClick,
            5 => MouseAction::ScrollUp,
            6 => MouseAction::ScrollDown,
            7 => MouseAction::Button4,
            8 => MouseAction::Button5,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            MouseAction::LeftClick => "left_click",
            MouseAction::MiddleClick => "middle_click",
            MouseAction::RightClick => "right_click",
            MouseAction::DoubleClick => "double_click",
            MouseAction::ScrollUp => "scroll_up",
            MouseAction::ScrollDown => "scroll_down",
            MouseAction::Button4 => "button4",
            MouseAction::Button5 => "button5",
        }
    }

    pub fn parse(s: &str) -> Option<MouseAction> {
        Some(match s.to_ascii_lowercase().replace('-', "_").as_str() {
            "left_click" | "left" | "lclick" => MouseAction::LeftClick,
            "middle_click" | "middle" | "mclick" => MouseAction::MiddleClick,
            "right_click" | "right" | "rclick" => MouseAction::RightClick,
            "double_click" | "double" | "dclick" => MouseAction::DoubleClick,
            "scroll_up" | "wheel_up" | "scrollup" => MouseAction::ScrollUp,
            "scroll_down" | "wheel_down" | "scrolldown" => MouseAction::ScrollDown,
            "button4" | "back" | "btn4" => MouseAction::Button4,
            "button5" | "forward" | "btn5" => MouseAction::Button5,
            _ => return None,
        })
    }

    /// All actions, for `--valid-actions` listing.
    pub const ALL: [MouseAction; 8] = [
        MouseAction::LeftClick,
        MouseAction::MiddleClick,
        MouseAction::RightClick,
        MouseAction::DoubleClick,
        MouseAction::ScrollUp,
        MouseAction::ScrollDown,
        MouseAction::Button4,
        MouseAction::Button5,
    ];
}

/// One step in a macro's recorded sequence. The wire encoding (see
/// `macro_table`) is derived from the vendor tool and cross-checked against real
/// profile exports; `Raw` is the escape hatch for a literal 4-byte event record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MacroEvent {
    /// Press a key (HID keycode).
    KeyDown(u8),
    /// Release a key (HID keycode).
    KeyUp(u8),
    /// Press a mouse button.
    MouseDown(MouseButton),
    /// Release a mouse button.
    MouseUp(MouseButton),
    /// Wait, in milliseconds (stored 16-bit on the wire).
    Delay(u16),
    /// A literal 4-byte event record `[b0, b1, b2, opcode]`.
    Raw([u8; 4]),
}

fn default_times() -> u8 {
    1
}
fn is_default_times(t: &u8) -> bool {
    *t == 1
}
fn default_macro_delay() -> u16 {
    10
}
fn is_default_macro_delay(d: &u16) -> bool {
    *d == 10
}

/// A single stored macro: an ordered list of [`MacroEvent`]s. Its position in
/// [`Keymap::macros`] is the index referenced by an [`Entry::Macro`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Macro {
    /// Play count when triggered (vendor `times`). Drives the binding's
    /// count/repeat bytes (see [`Keymap::resolve_macros`]).
    #[serde(default = "default_times", skip_serializing_if = "is_default_times")]
    pub times: u8,
    /// Default gap (ms) inserted between two consecutive non-delay events
    /// (vendor `delaytime`); the vendor hardwires 10, but this honours the field.
    #[serde(
        default = "default_macro_delay",
        skip_serializing_if = "is_default_macro_delay"
    )]
    pub default_delay: u16,
    pub events: Vec<MacroEvent>,
}

impl Default for Macro {
    fn default() -> Self {
        Macro {
            times: 1,
            default_delay: 10,
            events: Vec::new(),
        }
    }
}

/// A full keymap for a specific board variant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Keymap {
    pub variant: Variant,
    /// The Fn-key behaviour setting (not part of the keymap tables).
    #[serde(default)]
    pub fn_mode: FnMode,
    /// Stored macro bodies, uploaded separately from the keymap tables. The
    /// index into this vec is what an [`Entry::Macro`] references.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub macros: Vec<Macro>,
    pub layers: Vec<Layer>,
}

impl Keymap {
    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    pub fn layer_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.id == id)
    }

    /// Sync each macro binding's wire `count`/`repeat` bytes from the referenced
    /// macro's play count (`times`), the single source of truth. Bindings whose
    /// index has no macro are left unchanged.
    pub fn resolve_macros(&mut self) {
        let times: Vec<u8> = self.macros.iter().map(|m| m.times).collect();
        for layer in &mut self.layers {
            for km in &mut layer.keys {
                if let Entry::Macro {
                    index,
                    repeat,
                    count,
                } = &mut km.entry
                {
                    if let Some(&t) = times.get(*index as usize) {
                        *count = t;
                        *repeat = u8::from(t > 1);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_encodings_match_spec() {
        assert_eq!(
            Entry::Key {
                modifier: 0,
                hid: 0xe0
            }
            .encode(),
            [0x02, 0x00, 0xe0, 0x00]
        );
        assert_eq!(
            Entry::Consumer { code: 0xcd }.encode(),
            [0x03, 0xcd, 0x00, 0x00]
        );
        assert_eq!(
            Entry::Macro {
                index: 3,
                repeat: 1,
                count: 5
            }
            .encode(),
            [0x06, 3, 1, 5]
        );
        // Mouse actions are type 0x01 with the 16-bit code (LE). Right-Click =
        // 0x0201, Scroll Up = 0x0103 (verified against a vendor export).
        assert_eq!(
            Entry::Mouse {
                action: MouseAction::RightClick
            }
            .encode(),
            [0x01, 0x01, 0x02, 0x00]
        );
        assert_eq!(
            Entry::Mouse {
                action: MouseAction::ScrollUp
            }
            .encode(),
            [0x01, 0x03, 0x01, 0x00]
        );
        assert_eq!(Entry::Unassigned.encode(), [0, 0, 0, 0]);
    }

    #[test]
    fn mouse_action_vendor_index_roundtrip() {
        // The 1-based indices seen in exports (Right=3, Scroll Up=5).
        assert_eq!(
            MouseAction::from_vendor_index(3),
            Some(MouseAction::RightClick)
        );
        assert_eq!(
            MouseAction::from_vendor_index(5),
            Some(MouseAction::ScrollUp)
        );
        assert_eq!(
            MouseAction::from_vendor_index(8),
            Some(MouseAction::Button5)
        );
        assert_eq!(MouseAction::from_vendor_index(0), None);
        assert_eq!(MouseAction::from_vendor_index(9), None);
    }
}
