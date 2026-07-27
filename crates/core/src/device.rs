//! High-level device operations built on a [`FeatureTransport`]: enumeration,
//! the read-only lighting dump, and the keymap flash sequence.

use std::thread;
use std::time::Duration;

use crate::encode::encode_layer;
use crate::error::{Error, Result};
use crate::model::{Keymap, Layer};
use crate::protocol::{self, chunk_report, cmd, command_report, layer_command_report};
use crate::transport::{FeatureTransport, HidTransport};

fn sleep(ms: u64) {
    thread::sleep(Duration::from_millis(ms));
}

/// True for the vendor-defined HID collection (usage page `0xffXX`), which is
/// the collection this tool exchanges feature reports over. Single source of
/// truth for the preference used by both [`list`]/[`preferred_index`] and
/// [`Device::open`].
fn is_vendor_usage_page(usage_page: u16) -> bool {
    usage_page & 0xff00 == 0xff00
}

/// Summary of a matching HID interface (for the `devices` command).
#[derive(Clone, Debug)]
pub struct DeviceSummary {
    pub path: String,
    pub interface: i32,
    pub usage_page: u16,
    pub usage: u16,
    pub product: Option<String>,
    pub manufacturer: Option<String>,
}

impl DeviceSummary {
    /// Whether this interface is the vendor-defined collection this tool talks
    /// over. This is the interface `Device::open(None)` prefers.
    pub fn is_config_interface(&self) -> bool {
        is_vendor_usage_page(self.usage_page)
    }
}

/// Index of the interface [`Device::open`] would select given these summaries:
/// the last vendor-defined collection, or the first interface if none qualify.
/// Mirrors the selection loop in `open` so the `devices` listing can flag the
/// exact interface that will be used.
pub fn preferred_index(summaries: &[DeviceSummary]) -> Option<usize> {
    let mut chosen = None;
    for (i, s) in summaries.iter().enumerate() {
        if chosen.is_none() || s.is_config_interface() {
            chosen = Some(i);
        }
    }
    chosen
}

/// Enumerate all HID interfaces matching the keyboard's VID/PID.
pub fn list() -> Result<Vec<DeviceSummary>> {
    let api = hidapi::HidApi::new().map_err(|e| Error::Hid(e.to_string()))?;
    let mut out = Vec::new();
    for info in api.device_list() {
        if info.vendor_id() == crate::VID && info.product_id() == crate::PID {
            out.push(DeviceSummary {
                path: info.path().to_string_lossy().into_owned(),
                interface: info.interface_number(),
                usage_page: info.usage_page(),
                usage: info.usage(),
                product: info.product_string().map(str::to_owned),
                manufacturer: info.manufacturer_string().map(str::to_owned),
            });
        }
    }
    Ok(out)
}

/// A connected keyboard.
pub struct Device<T: FeatureTransport> {
    transport: T,
    /// Delay between reports, mirroring the vendor tool's pacing.
    pub op_delay_ms: u64,
}

impl<T: FeatureTransport> Device<T> {
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            op_delay_ms: 10,
        }
    }

    /// Consume the device, returning the underlying transport (useful for the
    /// dry-run transport's captured log).
    pub fn into_transport(self) -> T {
        self.transport
    }

    /// Read the live per-key RGB framebuffer via `04 F5`. Returns `[slot, R, G,
    /// B]` entries. This is the one safe read the firmware exposes and is used
    /// for connectivity validation.
    pub fn dump_lights(&mut self) -> Result<Vec<[u8; 4]>> {
        let mut out = Vec::new();
        let mut prev: Option<[u8; protocol::PAYLOAD_LEN]> = None;
        for i in 0..32u32 {
            let first = u8::from(i == 0);
            self.transport
                .set_feature(&command_report(cmd::DUMP, &[first]))?;
            sleep(3);
            let mut buf = [0u8; protocol::REPORT_LEN];
            self.transport.get_feature(&mut buf)?;
            let mut payload = [0u8; protocol::PAYLOAD_LEN];
            payload.copy_from_slice(&buf[1..]);

            if i > 0 && protocol::is_dump_terminator(&payload) {
                break;
            }
            if Some(payload) == prev || payload.iter().all(|&b| b == 0) {
                break;
            }
            for e in payload.chunks_exact(4) {
                out.push([e[0], e[1], e[2], e[3]]);
            }
            prev = Some(payload);
            sleep(3);
        }
        Ok(out)
    }

    /// Send a command report using the vendor's read-back handshake: a settle
    /// delay, the SET_FEATURE, another settle, then a GET_FEATURE status read.
    /// If the status' `payload[3] == 0xFF` the firmware reports busy, so the
    /// command is resent once and re-read (mirrors the vendor's `FUN_0045dbf0`
    /// path). Read errors are tolerated so a flaky status read can't abort a
    /// flash mid-sequence. Bulk table chunks deliberately skip this.
    fn send_command(&mut self, report: &[u8; protocol::REPORT_LEN]) -> Result<()> {
        sleep(self.op_delay_ms);
        self.transport.set_feature(report)?;
        sleep(self.op_delay_ms);
        let mut status = [0u8; protocol::REPORT_LEN];
        if self.transport.get_feature(&mut status).is_ok()
            && status[1 + protocol::ACK_PAYLOAD_INDEX] == 0xFF
        {
            self.transport.set_feature(report)?;
            sleep(self.op_delay_ms);
            let _ = self.transport.get_feature(&mut status);
        }
        Ok(())
    }

    /// Flash a single layer: enter → layer-select → 64-byte chunks → commit →
    /// save. Matches the vendor tool byte-for-byte (see the reverse-engineered
    /// protocol in PROTOCOL.md §8): commands use the read-back handshake, and
    /// exactly `(table_size >> 6) - 1` full 64-byte chunks are sent (576 bytes
    /// for every layer), stopping just past the `0x55AA` trailer at offset 574.
    /// Sending the whole padded buffer (extra chunks past the trailer) hangs the
    /// firmware. WARNING: still pending a USB-capture cross-check on hardware.
    pub fn flash_layer(&mut self, layer: &Layer) -> Result<()> {
        let table = encode_layer(layer)?;
        let n_chunks = (table.len() >> 6).saturating_sub(1);

        self.send_command(&command_report(cmd::ENTER, &[]))?;
        self.send_command(&layer_command_report(layer.id.command()))?;
        for chunk in table.chunks(protocol::PAYLOAD_LEN).take(n_chunks) {
            self.transport.set_feature(&chunk_report(chunk))?;
            sleep(self.op_delay_ms);
        }
        self.send_command(&command_report(cmd::COMMIT, &[]))?;
        self.send_command(&command_report(cmd::SAVE, &[]))?;
        Ok(())
    }

    /// Flash every layer in the keymap.
    pub fn flash(&mut self, keymap: &Keymap) -> Result<()> {
        for layer in &keymap.layers {
            self.flash_layer(layer)?;
        }
        Ok(())
    }

    /// Upload the stored macro bodies via the `04 19` / `04 15` sequence:
    /// prepare (`04 19`), header `04 15` carrying the 64-byte chunk count at
    /// `payload[8]`, the data chunks, then commit (`04 02`). No enter/save
    /// brackets. Does nothing when there are no macros. Hardware-verified (the
    /// buffer format in [`crate::macro_table`] was confirmed against a vendor
    /// USB capture); apply uploads macros last, after the keymap.
    pub fn upload_macros(&mut self, macros: &[crate::model::Macro]) -> Result<()> {
        let Some(buf) = crate::macro_table::encode_macros(macros)? else {
            return Ok(());
        };
        let n = crate::macro_table::chunk_count(&buf);

        self.send_command(&command_report(cmd::MACRO_PREP, &[]))?;
        let mut header = command_report(cmd::MACRO, &[]);
        header[9] = n; // payload[8] = chunk count
        self.send_command(&header)?;
        // The vendor waits 30 ms (Sleep(0x1e)) around the bulk macro data; the
        // macro area is a slower persistent write, so use 3x the base delay.
        sleep(self.op_delay_ms * 3);
        for chunk in buf.chunks(protocol::PAYLOAD_LEN) {
            self.transport.set_feature(&chunk_report(chunk))?;
            sleep(self.op_delay_ms);
        }
        sleep(self.op_delay_ms * 3);
        self.send_command(&command_report(cmd::COMMIT, &[]))?;
        Ok(())
    }

    /// Set the Fn-key mode (momentary vs latch) via the `04 17` command:
    /// enter → `04 17` → data report → commit (no `04 F0` save, matching the
    /// vendor). See [`protocol::fn_mode_reports`]; the mode-byte encoding is
    /// decompile-derived and awaiting a hardware confirm.
    pub fn set_fn_mode(&mut self, momentary: bool) -> Result<()> {
        let [cmd_rep, data] = protocol::fn_mode_reports(momentary);
        self.send_command(&command_report(cmd::ENTER, &[]))?;
        self.send_command(&cmd_rep)?;
        self.send_command(&data)?;
        self.send_command(&command_report(cmd::COMMIT, &[]))?;
        Ok(())
    }
}

impl Device<HidTransport> {
    /// Open the keyboard. `interface`, if given, selects a specific HID
    /// interface number; otherwise the vendor-defined collection is preferred.
    pub fn open(interface: Option<i32>) -> Result<Self> {
        let api = hidapi::HidApi::new().map_err(|e| Error::Hid(e.to_string()))?;
        let mut chosen: Option<&hidapi::DeviceInfo> = None;
        for info in api.device_list() {
            if info.vendor_id() != crate::VID || info.product_id() != crate::PID {
                continue;
            }
            match interface {
                Some(iface) if info.interface_number() == iface => {
                    chosen = Some(info);
                    break;
                }
                Some(_) => {}
                None => {
                    if chosen.is_none() || is_vendor_usage_page(info.usage_page()) {
                        chosen = Some(info);
                    }
                }
            }
        }
        let info = chosen.ok_or(Error::DeviceNotFound {
            vid: crate::VID,
            pid: crate::PID,
        })?;
        let device = info
            .open_device(&api)
            .map_err(|e| Error::Hid(e.to_string()))?;
        Ok(Device::new(HidTransport::new(device)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{chunk_report, command_report};
    use crate::transport::MockTransport;

    #[test]
    fn dump_lights_decodes_chunks_until_terminator() {
        // one data chunk of 16 [slot,r,g,b] entries, then a terminator echo.
        let mut data = [0u8; protocol::PAYLOAD_LEN];
        for slot in 0..16u8 {
            let o = slot as usize * 4;
            data[o] = slot;
            data[o + 1] = 0x11;
            data[o + 2] = 0x22;
            data[o + 3] = 0x33;
        }
        let chunk = chunk_report(&data);
        let terminator = command_report(cmd::DUMP, &[]); // payload starts 04 F5
        let mut dev = Device::new(MockTransport::with_responses([chunk, terminator]));
        let lights = dev.dump_lights().unwrap();
        assert_eq!(lights.len(), 16);
        assert_eq!(lights[0], [0, 0x11, 0x22, 0x33]);
        assert_eq!(lights[15][0], 15);
    }

    #[test]
    fn flash_layer_emits_bracketed_sequence() {
        use crate::model::{Entry, KeyMapping, Layer, LayerId};
        let layer = Layer {
            id: LayerId::Base,
            keys: vec![KeyMapping {
                slot: 55,
                entry: Entry::Key {
                    modifier: 0,
                    hid: 0xe0,
                },
            }],
        };
        let mut dev = Device::new(MockTransport::default());
        dev.op_delay_ms = 0;
        dev.flash_layer(&layer).unwrap();
        let sent = dev.into_transport().sent;
        // enter, layer-select, then (table_size >> 6) - 1 = 9 full 64-byte
        // chunks (the vendor stops just past the trailer), commit, save.
        assert_eq!(
            sent.first().unwrap()[1..3],
            [protocol::CMD_PREFIX, cmd::ENTER]
        );
        assert_eq!(sent[1][1..3], [protocol::CMD_PREFIX, cmd::LAYER_BASE]);
        let chunks = (0x2b6usize >> 6) - 1;
        assert_eq!(chunks, 9);
        assert_eq!(sent.len(), 2 + chunks + 2);
        // the 0x55AA trailer (table offset 574) lands in the last emitted chunk
        // (chunk 8 covers bytes 512..576): report byte 1 + (574 - 512) = 63.
        let last_chunk = &sent[2 + chunks - 1];
        assert_eq!(&last_chunk[63..65], &[0xAA, 0x55]);
        assert_eq!(
            sent[sent.len() - 2][1..3],
            [protocol::CMD_PREFIX, cmd::COMMIT]
        );
        assert_eq!(
            sent[sent.len() - 1][1..3],
            [protocol::CMD_PREFIX, cmd::SAVE]
        );
    }

    #[test]
    fn set_fn_mode_emits_enter_cmd_data_commit() {
        let mut dev = Device::new(MockTransport::default());
        dev.op_delay_ms = 0;
        dev.set_fn_mode(false).unwrap(); // latch
        let sent = dev.into_transport().sent;
        assert_eq!(sent.len(), 4);
        assert_eq!(sent[0][1..3], [protocol::CMD_PREFIX, cmd::ENTER]);
        assert_eq!(sent[1][1..3], [protocol::CMD_PREFIX, cmd::FN_MODE]);
        assert_eq!(sent[1][9], 0x01); // payload[8]
                                      // data report: latch -> mode byte 1 at payload[5], 0x55AA trailer.
        assert_eq!(sent[2][6], 0x01);
        assert_eq!(&sent[2][63..65], &[0xAA, 0x55]);
        assert_eq!(sent[3][1..3], [protocol::CMD_PREFIX, cmd::COMMIT]);

        // momentary -> mode byte 0.
        let mut dev = Device::new(MockTransport::default());
        dev.op_delay_ms = 0;
        dev.set_fn_mode(true).unwrap();
        assert_eq!(dev.into_transport().sent[2][6], 0x00);
    }

    #[test]
    fn upload_macros_emits_prep_header_chunks_commit() {
        use crate::model::{Macro, MacroEvent};
        let m = Macro {
            events: vec![MacroEvent::KeyDown(0x04), MacroEvent::KeyUp(0x04)],
            ..Default::default()
        };
        let mut dev = Device::new(MockTransport::default());
        dev.op_delay_ms = 0;
        dev.upload_macros(std::slice::from_ref(&m)).unwrap();
        let sent = dev.into_transport().sent;
        // prep (04 19), header (04 15) with chunk count at payload[8], N data
        // chunks, then commit (04 02).
        let buf = crate::macro_table::encode_macros(std::slice::from_ref(&m))
            .unwrap()
            .unwrap();
        let n = (buf.len() / 64) as usize;
        assert_eq!(sent[0][1..3], [protocol::CMD_PREFIX, cmd::MACRO_PREP]);
        assert_eq!(sent[1][1..3], [protocol::CMD_PREFIX, cmd::MACRO]);
        assert_eq!(sent[1][9] as usize, n); // payload[8] = chunk count
        assert_eq!(sent.len(), 2 + n + 1);
        assert_eq!(
            sent[sent.len() - 1][1..3],
            [protocol::CMD_PREFIX, cmd::COMMIT]
        );
    }

    #[test]
    fn upload_macros_noop_when_empty() {
        let mut dev = Device::new(MockTransport::default());
        dev.op_delay_ms = 0;
        dev.upload_macros(&[]).unwrap();
        assert!(dev.into_transport().sent.is_empty());
    }
}
