//! Encode stored macros into the on-wire macro buffer uploaded via `04 15`.
//!
//! Layout (confirmed against a vendor USB capture, hardware-verified):
//!   * a fixed [`macros::EVENT_REGION_START`]-byte **index table** at the head,
//!     one [`macros::INDEX_SLOT_BYTES`]-byte slot per macro holding the 16-bit
//!     base-relative offset of that macro's data structure,
//!   * a **macro data region** from [`macros::EVENT_REGION_START`] on; each
//!     macro is `[count:1][7 reserved:0][event records]`, where `count` is the
//!     number of 4-byte records (auto-spacers included) and the firmware plays
//!     exactly that many. Each event is a fixed 4-byte record `[b0, b1, b2,
//!     opcode]`. (Omitting `count` was the "plays nothing" bug.)
//!   * the `0x55AA` trailer as the final two bytes,
//!   * padded to the vendor's chunk count (the device reads it from `04 15`).
//!
//! Validation rejects more than [`macros::MAX_MACROS`] macros and any buffer
//! past [`macros::MAX_BUFFER`] (too many/too large events).

use crate::error::{Error, Result};
use crate::model::{Macro, MacroEvent};
use crate::protocol::{macros, TABLE_TRAILER};

/// Encode one event into its 4-byte record `[b0, b1, b2, opcode]`. Opcode is in
/// `b3`; a delay's ms value is a 16-bit LE in `b0:b1`, while key/mouse operands
/// go in `b2`. Verified against vendor profile exports.
fn encode_event(ev: MacroEvent) -> [u8; 4] {
    match ev {
        MacroEvent::Delay(ms) => [(ms & 0xff) as u8, (ms >> 8) as u8, 0, 0x50],
        MacroEvent::KeyDown(hid) => [0, 0, hid, 0xB0],
        MacroEvent::KeyUp(hid) => [0, 0, hid, 0x30],
        MacroEvent::MouseDown(b) => [0, 0, b.bit(), 0x90],
        MacroEvent::MouseUp(b) => [0, 0, b.bit(), 0x10],
        MacroEvent::Raw(bytes) => bytes,
    }
}

/// Append a macro's events to `out`, inserting a `default_delay`-ms delay between
/// any two consecutive non-delay events (the vendor's implicit inter-action gap;
/// it hardwires 10 ms, we honour the macro's field).
fn encode_macro_events(m: &Macro, out: &mut Vec<u8>) {
    let mut prev_was_delay = false;
    let mut emitted = 0usize;
    for &ev in &m.events {
        let is_delay = matches!(ev, MacroEvent::Delay(_));
        if !is_delay && !prev_was_delay && emitted > 0 {
            out.extend_from_slice(&encode_event(MacroEvent::Delay(m.default_delay)));
            emitted += 1;
        }
        out.extend_from_slice(&encode_event(ev));
        prev_was_delay = is_delay;
        emitted += 1;
    }
}

/// The number of 64-byte chunks a `buf` of this length will upload as.
pub fn chunk_count(buf: &[u8]) -> u8 {
    (buf.len() / 64) as u8
}

/// Encode all `macros` into the upload buffer, or `None` if there are none
/// (nothing to upload). The returned length is a multiple of 64.
pub fn encode_macros(macros_in: &[Macro]) -> Result<Option<Vec<u8>>> {
    if macros_in.is_empty() {
        return Ok(None);
    }
    if macros_in.len() > macros::MAX_MACROS {
        return Err(Error::Encode(format!(
            "{} macros defined, but at most {} can be stored",
            macros_in.len(),
            macros::MAX_MACROS
        )));
    }

    // Build the macro data region (placed at EVENT_REGION_START). Each macro is
    // `[count:1][7 reserved:0][event records]`; index[P] is the base-relative
    // offset of that structure. `count` = number of 4-byte records (spacers
    // included) — the firmware plays exactly this many.
    let mut region: Vec<u8> = Vec::new();
    let mut offsets: Vec<usize> = Vec::with_capacity(macros_in.len());
    for m in macros_in {
        offsets.push(macros::EVENT_REGION_START + region.len());
        let mut events = Vec::new();
        encode_macro_events(m, &mut events);
        let record_count = (events.len() / macros::EVENT_BYTES) as u8;
        region.push(record_count);
        region.resize(region.len() + (macros::EVENT_HEADER_BYTES - 1), 0); // 7 reserved
        region.extend_from_slice(&events);
    }

    // Chunk count uses the vendor's exact formula (FUN_004215b0): with
    // `data_end` = EVENT_REGION_START + region length,
    //   N = data_end/64 + (data_end % 64 == 0 ? 1 : 2)
    // i.e. always at least one whole chunk of padding past the data, with the
    // `0x55AA` trailer at the very end of the last chunk. The firmware reads N
    // from the `04 15` header and expects exactly that many chunks.
    let data_end = macros::EVENT_REGION_START + region.len();
    let n_chunks = data_end / 64 + if data_end.is_multiple_of(64) { 1 } else { 2 };
    let total = n_chunks * 64;
    if total > macros::MAX_BUFFER {
        let events: usize = macros_in.iter().map(|m| m.events.len()).sum();
        return Err(Error::Encode(format!(
            "macro data is {total} bytes (> {} limit): {events} events across {} macros is too many",
            macros::MAX_BUFFER,
            macros_in.len()
        )));
    }

    let mut buf = vec![0u8; total];
    // Index table: 16-bit offset of each macro's structure in the low half of
    // each slot.
    for (i, &off) in offsets.iter().enumerate() {
        let slot = i * macros::INDEX_SLOT_BYTES;
        buf[slot..slot + 2].copy_from_slice(&(off as u16).to_le_bytes());
    }
    // Macro data region.
    buf[macros::EVENT_REGION_START..macros::EVENT_REGION_START + region.len()]
        .copy_from_slice(&region);
    // Trailer as the final two bytes.
    let end = total - TABLE_TRAILER_LEN;
    buf[end..total].copy_from_slice(&TABLE_TRAILER.to_le_bytes());

    Ok(Some(buf))
}

const TABLE_TRAILER_LEN: usize = 2;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MouseButton;

    fn mac(events: Vec<MacroEvent>) -> Macro {
        Macro {
            events,
            ..Default::default()
        }
    }

    #[test]
    fn empty_macros_encode_to_nothing() {
        assert!(encode_macros(&[]).unwrap().is_none());
    }

    #[test]
    fn single_macro_layout_and_trailer() {
        // key down, explicit delay, key up (the delay avoids the auto-spacer).
        let m = mac(vec![
            MacroEvent::KeyDown(0x07), // 'd'
            MacroEvent::Delay(20),
            MacroEvent::KeyUp(0x07),
        ]);
        let buf = encode_macros(std::slice::from_ref(&m)).unwrap().unwrap();
        assert_eq!(buf.len() % 64, 0);
        // index[0] = 400 (0x0190, LE): the macro's [count][7 reserved][events]
        // structure starts there.
        assert_eq!(&buf[0..4], &[0x90, 0x01, 0x00, 0x00]);
        assert_eq!(buf[400], 3); // event count (3 records: down, delay, up)
                                 // events follow the 8-byte header at offset 408.
        assert_eq!(&buf[408..412], &[0, 0, 0x07, 0xB0]); // key down
        assert_eq!(&buf[412..416], &[20, 0, 0, 0x50]); // delay 20 ms
        assert_eq!(&buf[416..420], &[0, 0, 0x07, 0x30]); // key up
        assert_eq!(&buf[buf.len() - 2..], &[0xAA, 0x55]); // trailer
    }

    #[test]
    fn mouse_and_delay_event_encodings() {
        let buf = encode_macros(&[mac(vec![
            MacroEvent::MouseDown(MouseButton::Left),
            MacroEvent::Delay(50),
            MacroEvent::MouseUp(MouseButton::Right),
        ])])
        .unwrap()
        .unwrap();
        assert_eq!(&buf[408..412], &[0, 0, 0x01, 0x90]); // left down
        assert_eq!(&buf[412..416], &[50, 0, 0, 0x50]); // delay 50 ms
        assert_eq!(&buf[416..420], &[0, 0, 0x02, 0x10]); // right up
    }

    #[test]
    fn auto_spacer_between_consecutive_actions() {
        // Two back-to-back non-delay events get a 10 ms spacer between them,
        // but none before the first.
        let buf = encode_macros(&[mac(vec![
            MacroEvent::KeyDown(0x04),
            MacroEvent::KeyUp(0x04),
        ])])
        .unwrap()
        .unwrap();
        assert_eq!(&buf[408..412], &[0, 0, 0x04, 0xB0]); // key down, no leading spacer
        assert_eq!(&buf[412..416], &[10, 0, 0, 0x50]); // auto 10 ms spacer
        assert_eq!(&buf[416..420], &[0, 0, 0x04, 0x30]); // key up
    }

    #[test]
    fn default_delay_sets_the_spacer() {
        let m = Macro {
            events: vec![MacroEvent::KeyDown(0x04), MacroEvent::KeyUp(0x04)],
            default_delay: 50,
            ..Default::default()
        };
        let buf = encode_macros(&[m]).unwrap().unwrap();
        assert_eq!(&buf[412..416], &[50, 0, 0, 0x50]); // spacer uses default_delay
    }

    #[test]
    fn rejects_too_many_macros() {
        let many: Vec<Macro> = (0..=macros::MAX_MACROS).map(|_| mac(vec![])).collect();
        let err = encode_macros(&many).unwrap_err().to_string();
        assert!(err.contains("at most"), "got: {err}");
    }

    #[test]
    fn rejects_too_many_events() {
        // One giant macro whose events overflow the buffer ceiling.
        let huge = mac(vec![MacroEvent::Delay(1); macros::MAX_BUFFER]);
        let err = encode_macros(&[huge]).unwrap_err().to_string();
        assert!(err.contains("too many"), "got: {err}");
    }

    #[test]
    fn packs_two_macros_with_correct_offsets() {
        let buf = encode_macros(&[
            mac(vec![MacroEvent::KeyDown(0x04), MacroEvent::KeyUp(0x04)]),
            mac(vec![MacroEvent::KeyDown(0x05)]),
        ])
        .unwrap()
        .unwrap();
        // macro 0 = down + auto-spacer + up = 3 records; its structure is
        // [count][7 reserved][3*4 events] = 20 bytes at offset 400, so macro 1
        // starts at 420. Macro 1's events follow its 8-byte header at 428.
        assert_eq!(&buf[0..2], &400u16.to_le_bytes());
        assert_eq!(&buf[4..6], &420u16.to_le_bytes());
        assert_eq!(buf[400], 3); // macro 0 record count
        assert_eq!(buf[420], 1); // macro 1 record count
        assert_eq!(&buf[428..432], &[0, 0, 0x05, 0xB0]);
    }
}
