//! HID usage codes and human-friendly name parsing.
//!
//! `KEYS` maps names to USB HID keyboard usage ids; `CONSUMER` maps names to the
//! single-byte consumer-control codes used by entry type `0x03` (validated
//! against the vendor tool, see PROTOCOL.md §5).

/// Standard keyboard usages. First name for a given code is the canonical one.
pub const KEYS: &[(&str, u8)] = &[
    ("a", 0x04),
    ("b", 0x05),
    ("c", 0x06),
    ("d", 0x07),
    ("e", 0x08),
    ("f", 0x09),
    ("g", 0x0a),
    ("h", 0x0b),
    ("i", 0x0c),
    ("j", 0x0d),
    ("k", 0x0e),
    ("l", 0x0f),
    ("m", 0x10),
    ("n", 0x11),
    ("o", 0x12),
    ("p", 0x13),
    ("q", 0x14),
    ("r", 0x15),
    ("s", 0x16),
    ("t", 0x17),
    ("u", 0x18),
    ("v", 0x19),
    ("w", 0x1a),
    ("x", 0x1b),
    ("y", 0x1c),
    ("z", 0x1d),
    ("1", 0x1e),
    ("2", 0x1f),
    ("3", 0x20),
    ("4", 0x21),
    ("5", 0x22),
    ("6", 0x23),
    ("7", 0x24),
    ("8", 0x25),
    ("9", 0x26),
    ("0", 0x27),
    ("enter", 0x28),
    ("esc", 0x29),
    ("backspace", 0x2a),
    ("tab", 0x2b),
    ("space", 0x2c),
    ("minus", 0x2d),
    ("equal", 0x2e),
    ("lbracket", 0x2f),
    ("rbracket", 0x30),
    ("backslash", 0x31),
    ("semicolon", 0x33),
    ("quote", 0x34),
    ("grave", 0x35),
    ("comma", 0x36),
    ("period", 0x37),
    ("slash", 0x38),
    ("capslock", 0x39),
    ("caps", 0x39),
    ("f1", 0x3a),
    ("f2", 0x3b),
    ("f3", 0x3c),
    ("f4", 0x3d),
    ("f5", 0x3e),
    ("f6", 0x3f),
    ("f7", 0x40),
    ("f8", 0x41),
    ("f9", 0x42),
    ("f10", 0x43),
    ("f11", 0x44),
    ("f12", 0x45),
    ("printscreen", 0x46),
    ("print", 0x46),
    ("scrolllock", 0x47),
    ("pause", 0x48),
    ("insert", 0x49),
    ("home", 0x4a),
    ("pageup", 0x4b),
    ("delete", 0x4c),
    ("end", 0x4d),
    ("pagedown", 0x4e),
    ("right", 0x4f),
    ("left", 0x50),
    ("down", 0x51),
    ("up", 0x52),
    // Keypad / numpad cluster.
    ("numlock", 0x53),
    ("kp_numlock", 0x53),
    ("kp_divide", 0x54),
    ("kp_multiply", 0x55),
    ("kp_minus", 0x56),
    ("kp_subtract", 0x56),
    ("kp_plus", 0x57),
    ("kp_add", 0x57),
    ("kp_enter", 0x58),
    ("kp1", 0x59),
    ("kp2", 0x5a),
    ("kp3", 0x5b),
    ("kp4", 0x5c),
    ("kp5", 0x5d),
    ("kp6", 0x5e),
    ("kp7", 0x5f),
    ("kp8", 0x60),
    ("kp9", 0x61),
    ("kp0", 0x62),
    ("kp_dot", 0x63),
    ("kp_decimal", 0x63),
    ("kp_equal", 0x67),
    ("menu", 0x65),
    ("app", 0x65),
    // Non-US (ISO) keys and keyboard power.
    ("nonushash", 0x32),
    ("nonusbackslash", 0x64),
    ("power", 0x66),
    // Extended function keys F13–F24.
    ("f13", 0x68),
    ("f14", 0x69),
    ("f15", 0x6a),
    ("f16", 0x6b),
    ("f17", 0x6c),
    ("f18", 0x6d),
    ("f19", 0x6e),
    ("f20", 0x6f),
    ("f21", 0x70),
    ("f22", 0x71),
    ("f23", 0x72),
    ("f24", 0x73),
    // Editing / application keys. (`kb_stop` avoids clashing with `media:stop`.)
    ("execute", 0x74),
    ("help", 0x75),
    ("kb_menu", 0x76),
    ("select", 0x77),
    ("kb_stop", 0x78),
    ("again", 0x79),
    ("undo", 0x7a),
    ("cut", 0x7b),
    ("copy", 0x7c),
    ("paste", 0x7d),
    ("find", 0x7e),
    // Locking lock keys (distinct from the momentary lock keys above).
    ("locking_capslock", 0x82),
    ("locking_numlock", 0x83),
    ("locking_scrolllock", 0x84),
    // Extra keypad keys.
    ("kp_comma", 0x85),
    ("kp_equalsign", 0x86),
    // International (JIS/ABNT) keys.
    ("intl1", 0x87),
    ("ro", 0x87),
    ("intl2", 0x88),
    ("intl3", 0x89),
    ("yen", 0x89),
    ("intl4", 0x8a),
    ("henkan", 0x8a),
    ("intl5", 0x8b),
    ("muhenkan", 0x8b),
    ("intl6", 0x8c),
    ("intl7", 0x8d),
    ("intl8", 0x8e),
    ("intl9", 0x8f),
    // Language keys (Korean / Japanese).
    ("lang1", 0x90),
    ("hangul", 0x90),
    ("lang2", 0x91),
    ("hanja", 0x91),
    ("lang3", 0x92),
    ("katakana", 0x92),
    ("lang4", 0x93),
    ("hiragana", 0x93),
    ("lang5", 0x94),
    ("lang6", 0x95),
    ("lang7", 0x96),
    ("lang8", 0x97),
    ("lang9", 0x98),
    // Obscure system / editing usages.
    ("alterase", 0x99),
    ("sysreq", 0x9a),
    ("cancel", 0x9b),
    ("clear", 0x9c),
    ("prior", 0x9d),
    ("kb_return", 0x9e),
    ("separator", 0x9f),
    ("out", 0xa0),
    ("oper", 0xa1),
    ("clear_again", 0xa2),
    ("crsel", 0xa3),
    ("exsel", 0xa4),
    ("lctrl", 0xe0),
    ("ctrl", 0xe0),
    ("lshift", 0xe1),
    ("shift", 0xe1),
    ("lalt", 0xe2),
    ("alt", 0xe2),
    ("lgui", 0xe3),
    ("win", 0xe3),
    ("rctrl", 0xe4),
    ("rshift", 0xe5),
    ("ralt", 0xe6),
    ("rgui", 0xe7),
    ("fn", 0xaf),
];

/// Consumer-control usages (media keys), single-byte codes for entry type 0x03.
pub const CONSUMER: &[(&str, u8)] = &[
    ("playpause", 0xcd),
    ("play", 0xcd),
    ("stop", 0xb7),
    ("prev", 0xb6),
    ("previous", 0xb6),
    ("next", 0xb5),
    ("mute", 0xe2),
    ("volup", 0xe9),
    ("volumeup", 0xe9),
    ("voldown", 0xea),
    ("volumedown", 0xea),
];

/// Resolve a keyboard key name to its HID usage id (case-insensitive).
pub fn hid_from_name(name: &str) -> Option<u8> {
    let n = name.to_ascii_lowercase();
    KEYS.iter().find(|(k, _)| *k == n).map(|(_, v)| *v)
}

/// Resolve a HID usage id back to its canonical name.
pub fn name_from_hid(hid: u8) -> Option<&'static str> {
    KEYS.iter().find(|(_, v)| *v == hid).map(|(k, _)| *k)
}

/// Resolve a media/consumer name to its code (case-insensitive).
pub fn consumer_from_name(name: &str) -> Option<u8> {
    let n = name.to_ascii_lowercase();
    CONSUMER.iter().find(|(k, _)| *k == n).map(|(_, v)| *v)
}

/// Whether `hid` is a defined keyboard/keypad usage a keymap may write. Reserved
/// codes are excluded; the keyboard-page media keys (0x7f–0x81) are defined but
/// belong on the consumer page, so they are excluded here (use `media:`). The
/// vendor Fn code (0xAF) is allowed even though the HID spec reserves it.
pub fn is_writable_key_hid(hid: u8) -> bool {
    matches!(hid, 0x04..=0x7e | 0x82..=0xa4 | 0xaf | 0xb0..=0xdd | 0xe0..=0xe7)
}

/// Whether `hid` is a keyboard-page media key (mute / vol±) that duplicates a
/// consumer control; these belong on `media:` instead.
pub fn is_media_key_hid(hid: u8) -> bool {
    matches!(hid, 0x7f..=0x81)
}

/// Convert a Windows Virtual-Key code to its USB HID keyboard usage, as the
/// vendor macro recorder does (its `FUN_0045ea80`). Macro exports store VK
/// codes, so importing a macro key event goes through this. `None` = no mapping.
pub fn hid_from_vk(vk: u16) -> Option<u8> {
    let hid = match vk {
        0x41..=0x5a => vk - 0x3d, // A-Z -> 0x04-0x1d
        0x31..=0x39 => vk - 0x13, // 1-9 -> 0x1e-0x26
        0x30 => 0x27,             // 0
        0x0d => 0x28,             // Enter
        0x1b => 0x29,             // Esc
        0x08 => 0x2a,             // Backspace
        0x09 => 0x2b,             // Tab
        0x20 => 0x2c,             // Space
        0xbd => 0x2d,             // - _
        0xbb => 0x2e,             // = +
        0xdb => 0x2f,             // [ {
        0xdd => 0x30,             // ] }
        0xdc => 0x31,             // \ |
        0x29a => 0x32,            // non-US #
        0xba => 0x33,             // ; :
        0xde => 0x34,             // ' "
        0xc0 => 0x35,             // ` ~
        0xbc => 0x36,             // , <
        0xbe => 0x37,             // . >
        0xbf => 0x38,             // / ?
        0x14 => 0x39,             // Caps Lock
        0x70..=0x7b => vk - 0x36, // F1-F12 -> 0x3a-0x45
        0x7c..=0x87 => vk - 0x14, // F13-F24 -> 0x68-0x73
        0x2c => 0x46,             // Print Screen
        0x91 => 0x47,             // Scroll Lock
        0x13 => 0x48,             // Pause
        0x2d => 0x49,             // Insert
        0x24 => 0x4a,             // Home
        0x21 => 0x4b,             // Page Up
        0x2e => 0x4c,             // Delete
        0x23 => 0x4d,             // End
        0x22 => 0x4e,             // Page Down
        0x27 => 0x4f,             // Right
        0x25 => 0x50,             // Left
        0x28 => 0x51,             // Down
        0x26 => 0x52,             // Up
        0x90 => 0x53,             // Num Lock
        0x6f => 0x54,             // KP /
        0x6a => 0x55,             // KP *
        0x6d => 0x56,             // KP -
        0x6b => 0x57,             // KP +
        0x6c => 0x58,             // KP Enter
        0x61..=0x69 => vk - 0x08, // KP 1-9 -> 0x59-0x61
        0x60 => 0x62,             // KP 0
        0x6e => 0x63,             // KP .
        0xe2 => 0x64,             // non-US \
        0x5d => 0x65,             // Application / Menu
        0xa2 => 0xe0,             // Left Ctrl
        0xa0 => 0xe1,             // Left Shift
        0xa4 => 0xe2,             // Left Alt
        0x5b => 0xe3,             // Left GUI
        0xa3 => 0xe4,             // Right Ctrl
        0xa1 => 0xe5,             // Right Shift
        0xa5 => 0xe6,             // Right Alt
        0x5c => 0xe7,             // Right GUI
        0xfe => 0xaf,             // Fn
        0x12d => 0xec,
        0x12e => 0xeb,
        _ => return None,
    };
    Some(hid as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vk_to_hid_covers_common_keys() {
        assert_eq!(hid_from_vk(0x44), Some(0x07)); // VK 'D' -> HID d (macro export case)
        assert_eq!(hid_from_vk(0x41), Some(0x04)); // 'A' -> a
        assert_eq!(hid_from_vk(0x31), Some(0x1e)); // '1'
        assert_eq!(hid_from_vk(0x0d), Some(0x28)); // Enter
        assert_eq!(hid_from_vk(0x20), Some(0x2c)); // Space
        assert_eq!(hid_from_vk(0xa2), Some(0xe0)); // Left Ctrl
        assert_eq!(hid_from_vk(0x0000), None); // unmapped
    }
}
