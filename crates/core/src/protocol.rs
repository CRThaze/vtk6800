//! Wire protocol constants and report framing (see `docs/PROTOCOL.md`).
//!
//! Every exchange is a 65-byte HID feature report: `[report_id=0x00][64-byte
//! payload]`. Command payloads start with the prefix byte `0x04` followed by a
//! command byte; bulk keymap data is sent as raw 64-byte chunks bracketed by
//! command reports.

/// Full feature-report length: 1 report-id byte + 64 payload bytes.
pub const REPORT_LEN: usize = 65;
/// Payload length (report minus the report-id byte).
pub const PAYLOAD_LEN: usize = 64;
/// Report id is always 0 on this device.
pub const REPORT_ID: u8 = 0x00;
/// First payload byte of every command.
pub const CMD_PREFIX: u8 = 0x04;
/// Status byte read back after a command: `payload[3] == 0xFF` means the
/// firmware is still busy and the command must be resent (per the vendor's
/// `FUN_0045dbf0` handshake), not that it succeeded.
pub const ACK_PAYLOAD_INDEX: usize = 3;

/// Command bytes (`payload[1]`, wire form is `[0x04, cmd, ..]`).
pub mod cmd {
    pub const ENTER: u8 = 0x18; // enter programming (prefixes a write batch)
    pub const LAYER_BASE: u8 = 0x11; // program base keymap table
    pub const LAYER_FN: u8 = 0x27; // program Fn keymap table
    pub const LAYER_ALT: u8 = 0x23; // program alt/raw keymap table
    pub const COMMIT: u8 = 0x02; // commit/apply
    pub const SAVE: u8 = 0xF0; // save & exit
    pub const DUMP: u8 = 0xF5; // read live per-key RGB framebuffer
    pub const MACRO: u8 = 0x15; // macro upload header (carries chunk count)
    pub const MACRO_PREP: u8 = 0x19; // prepare macro programming (sent first)
    pub const LIGHT_APPLY: u8 = 0x20; // enter custom-light programming
    pub const FN_MODE: u8 = 0x17; // set Fn-key mode (momentary/latch)
}

/// Macro storage layout (see `macro_table`).
pub mod macros {
    /// Fixed index table at the head of the macro buffer: one 4-byte slot per
    /// macro, holding the 16-bit start offset of that macro's event stream.
    pub const INDEX_SLOT_BYTES: usize = 4;
    /// Each stored macro is `[count:1][7 reserved][event records]`; `index[P]`
    /// points at the count byte, and the events follow this 8-byte header. The
    /// count is the number of 4-byte event records (spacers included).
    /// CONFIRMED via a vendor USB capture — omitting the count made the firmware
    /// read 0 events and play nothing.
    pub const EVENT_HEADER_BYTES: usize = 8;
    /// Byte offset where the macro data region (first macro's header) begins;
    /// also the index-table size, giving `EVENT_REGION_START / INDEX_SLOT_BYTES`
    /// = 100 macro slots.
    pub const EVENT_REGION_START: usize = 400;
    /// Maximum number of stored macros (index-table capacity).
    pub const MAX_MACROS: usize = EVENT_REGION_START / INDEX_SLOT_BYTES;
    /// Each event is a fixed 4-byte record `[b0, b1, b2, opcode]`.
    pub const EVENT_BYTES: usize = 4;
    /// Hard ceiling on the whole macro buffer (the vendor's size guard).
    pub const MAX_BUFFER: usize = 0xA8C;
}

/// Build the reports for the `04 17` Fn-mode setting: the command report plus
/// its single data report. Reverse-engineered from `FUN_00427910` (see the
/// `vendor-settings-and-command-map` notes): the command carries `0x01` at
/// `payload[8]`, and the data report holds the mode byte at `payload[5]` with
/// the usual `0x55AA` table trailer.
///
/// Direction: the vendor sets `payload[5] = (combobox_data == 0)`. The dialog
/// defaults to data `1` = momentary (the factory Fn behaviour) and data `0` =
/// latch, so on the wire **momentary -> 0, latch -> 1**.
pub fn fn_mode_reports(momentary: bool) -> [[u8; REPORT_LEN]; 2] {
    let mut cmd_rep = command_report(cmd::FN_MODE, &[]);
    cmd_rep[9] = 0x01; // payload[8]

    let mut data = [0u8; REPORT_LEN];
    data[6] = u8::from(!momentary); // payload[5]: momentary -> 0, latch -> 1
    let [lo, hi] = TABLE_TRAILER.to_le_bytes();
    data[63] = lo; // payload[62]  (trailer 0x55AA spans payload[62..64], LE)
    data[64] = hi; // payload[63]

    [cmd_rep, data]
}

/// Magic trailer written near the end of every keymap table.
pub const TABLE_TRAILER: u16 = 0x55AA;
/// Byte offset of the trailer within a keymap table (from the base-layer
/// decompile; fits inside all layer table sizes).
pub const TABLE_TRAILER_OFFSET: usize = 574;

/// Build a command feature report: `[0x00, 0x04, cmd, params..]`.
pub fn command_report(cmd: u8, params: &[u8]) -> [u8; REPORT_LEN] {
    let mut r = [0u8; REPORT_LEN];
    r[1] = CMD_PREFIX;
    r[2] = cmd;
    r[3..3 + params.len()].copy_from_slice(params);
    r
}

/// Build the "enter layer programming" report for a layer command.
///
/// The layer command carries a `0x09` mode byte at `payload[8]`, confirmed both
/// in the decompiled vendor tool and on hardware (see PROTOCOL.md §8).
pub fn layer_command_report(cmd: u8) -> [u8; REPORT_LEN] {
    let mut r = command_report(cmd, &[]);
    r[9] = 0x09; // payload[8]
    r
}

/// Build a raw bulk-data chunk report: `[0x00, <=64 payload bytes]`.
pub fn chunk_report(chunk: &[u8]) -> [u8; REPORT_LEN] {
    debug_assert!(chunk.len() <= PAYLOAD_LEN);
    let mut r = [0u8; REPORT_LEN];
    r[1..1 + chunk.len()].copy_from_slice(chunk);
    r
}

/// True if a dump chunk is the `04 F5` terminator echo.
pub fn is_dump_terminator(payload: &[u8]) -> bool {
    payload.len() >= 2 && payload[0] == CMD_PREFIX && payload[1] == cmd::DUMP
}
