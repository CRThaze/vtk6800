//! Firmware-reserved Fn-layer keys.
//!
//! On the wireless controller (hfd.cn HFD801) a fixed set of `Fn + <key>` chords
//! are hardwired to firmware functions (Bluetooth device switching, RGB control,
//! Win-lock) and are intercepted *before* the Fn keymap table is consulted. Any
//! entry flashed to one of these slots on the Fn layer is therefore inert: it
//! emits nothing to USB. This is confirmed on hardware (`evtest` shows no event
//! for these Fn combos while base output is fine) and against the vendor's own
//! Windows GUI (remapping them has no effect either). The canonical functions
//! are documented in the official manuals (the "Fn Key Combos", "Connection",
//! and "RGB Light Setting" panels); the RGB combos apply only to the RGB variant
//! in wired mode.
//!
//! The set is keyed by hardware slot and is the same across all four SKUs; slots
//! absent from a given SKU (e.g. Win / slot 92 on the 66-key boards) simply
//! never appear in that variant's layout. Reserved status applies to the **Fn
//! layer only**: the same physical keys are ordinary, remappable keys on base.

use crate::model::{Entry, Keymap, LayerId};

/// A firmware-reserved Fn-layer key.
#[derive(Clone, Copy, Debug)]
pub struct ReservedFn {
    /// Hardware slot (keymap-table index) of the physical key.
    pub slot: u8,
    /// Compact label for the keyboard drawing (<= 4 display columns).
    pub short: &'static str,
    /// Human-readable firmware function: `Fn + <this key>` does this.
    pub function: &'static str,
}

/// Every firmware-reserved Fn-layer slot, in physical reading order.
pub const RESERVED_FN: &[ReservedFn] = &[
    // Right column: Bluetooth / 2.4G device manager ("Connection" panel).
    ReservedFn {
        slot: 117,
        short: "Pair",
        function: "Bluetooth pairing mode",
    },
    ReservedFn {
        slot: 118,
        short: "BT1",
        function: "Bluetooth device 1",
    },
    ReservedFn {
        slot: 121,
        short: "BT2",
        function: "Bluetooth device 2",
    },
    ReservedFn {
        slot: 120,
        short: "BT3",
        function: "Bluetooth device 3",
    },
    // Arrow cluster: RGB brightness / effect speed ("RGB Light Setting" panel).
    ReservedFn {
        slot: 101,
        short: "Bri+",
        function: "RGB brightness up",
    },
    ReservedFn {
        slot: 100,
        short: "Bri-",
        function: "RGB brightness down",
    },
    ReservedFn {
        slot: 102,
        short: "Spd+",
        function: "RGB effect speed up",
    },
    ReservedFn {
        slot: 99,
        short: "Spd-",
        function: "RGB effect speed down",
    },
    // Bottom-left modifiers: Win-lock + RGB colour / mode.
    ReservedFn {
        slot: 92,
        short: "WLck",
        function: "Win-lock (disable Win key)",
    },
    ReservedFn {
        slot: 91,
        short: "RGBc",
        function: "switch RGB colour",
    },
    ReservedFn {
        slot: 93,
        short: "RGBm",
        function: "switch RGB lighting mode",
    },
];

/// The reserved-Fn entry for `slot`, or `None` if the slot is freely remappable
/// on the Fn layer.
pub fn reserved_fn(slot: u8) -> Option<&'static ReservedFn> {
    RESERVED_FN.iter().find(|r| r.slot == slot)
}

/// A user binding that lands on a firmware-reserved Fn slot: the hardware owns
/// the combo, so this binding is inert (emits nothing).
#[derive(Clone, Copy, Debug)]
pub struct ReservedConflict {
    /// The reserved slot and its firmware function.
    pub reserved: &'static ReservedFn,
    /// The (inert) binding the user placed there.
    pub entry: Entry,
}

/// Every reserved Fn slot in `km` that carries a real (non-`Unassigned`) binding,
/// in [`RESERVED_FN`] order. Empty when the Fn layer holds nothing the firmware
/// would ignore. Intended for a load-time validation warning: a freshly seeded
/// default keymap leaves these slots unassigned, so only user edits trigger it.
pub fn reserved_conflicts(km: &Keymap) -> Vec<ReservedConflict> {
    let Some(fn_layer) = km.layer(LayerId::Fn) else {
        return Vec::new();
    };
    RESERVED_FN
        .iter()
        .filter_map(|reserved| match fn_layer.get(reserved.slot) {
            Some(entry) if entry != Entry::Unassigned => Some(ReservedConflict { reserved, entry }),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_reserved_slots_resolve() {
        // Home = Bluetooth pairing; left arrow = RGB effect speed down.
        assert_eq!(reserved_fn(117).unwrap().short, "Pair");
        assert_eq!(reserved_fn(99).unwrap().function, "RGB effect speed down");
        // A normal key (Fn+P / slot 47) is not reserved.
        assert!(reserved_fn(47).is_none());
    }

    #[test]
    fn reserved_conflicts_flags_only_reserved_fn_bindings() {
        use crate::model::{Entry, LayerId};
        use crate::Variant;
        let mut km = Variant::Ansi68.default_keymap().unwrap();
        // A freshly seeded default leaves every reserved Fn slot unassigned.
        assert!(reserved_conflicts(&km).is_empty());
        // Map Home (117, reserved) and P (47, free) on the Fn layer.
        let fn_layer = km.layer_mut(LayerId::Fn).unwrap();
        fn_layer.set(
            117,
            Entry::Key {
                modifier: 0,
                hid: 0x46,
            },
        );
        fn_layer.set(
            47,
            Entry::Key {
                modifier: 0,
                hid: 0x13,
            },
        );
        let conflicts = reserved_conflicts(&km);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].reserved.slot, 117);
        // A binding on the base layer is never a conflict.
        km.layer_mut(LayerId::Base).unwrap().set(
            117,
            Entry::Key {
                modifier: 0,
                hid: 0x04,
            },
        );
        assert_eq!(reserved_conflicts(&km).len(), 1);
    }

    #[test]
    fn short_labels_fit_the_grid() {
        // The keyboard drawing gives a 1u key 4 interior columns; every short
        // label must fit (all are ASCII, so char count == display width).
        for r in RESERVED_FN {
            assert!(
                r.short.len() <= 4,
                "slot {} short {:?} too wide",
                r.slot,
                r.short
            );
        }
    }
}
