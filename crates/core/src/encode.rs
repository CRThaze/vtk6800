//! Encode a [`Layer`] into the on-wire keymap table: an array of 4-byte entries
//! indexed by hardware slot, with the `0x55AA` trailer, sized exactly as the
//! vendor tool sends it.
//!
//! NOTE: the slot→offset mapping and the exact table tail are still pending
//! hardware verification (PROTOCOL.md §8); callers gate real writes behind an
//! explicit opt-in.

use crate::error::{Error, Result};
use crate::model::Layer;
use crate::protocol::{TABLE_TRAILER, TABLE_TRAILER_OFFSET};

/// Encode `layer` into its fixed-size table buffer.
pub fn encode_layer(layer: &Layer) -> Result<Vec<u8>> {
    let size = layer.id.table_size();
    let mut buf = vec![0u8; size];

    for km in &layer.keys {
        let off = km.slot as usize * 4;
        if off + 4 > size {
            return Err(Error::Encode(format!(
                "slot {} (offset {off}) exceeds {:?} table size {size}",
                km.slot, layer.id
            )));
        }
        if off < TABLE_TRAILER_OFFSET + 2 && off + 4 > TABLE_TRAILER_OFFSET {
            return Err(Error::Encode(format!(
                "slot {} collides with table trailer at {TABLE_TRAILER_OFFSET}",
                km.slot
            )));
        }
        buf[off..off + 4].copy_from_slice(&km.entry.encode());
    }

    buf[TABLE_TRAILER_OFFSET..TABLE_TRAILER_OFFSET + 2]
        .copy_from_slice(&TABLE_TRAILER.to_le_bytes());
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Entry, KeyMapping, Layer, LayerId};

    #[test]
    fn encodes_entries_and_trailer() {
        let layer = Layer {
            id: LayerId::Base,
            keys: vec![
                KeyMapping {
                    slot: 1,
                    entry: Entry::Key {
                        modifier: 0,
                        hid: 0x29,
                    },
                },
                KeyMapping {
                    slot: 55,
                    entry: Entry::Key {
                        modifier: 0,
                        hid: 0xe0,
                    },
                },
            ],
        };
        let buf = encode_layer(&layer).unwrap();
        assert_eq!(buf.len(), 0x2b6);
        // slot 1 -> offset 4
        assert_eq!(&buf[4..8], &[0x02, 0x00, 0x29, 0x00]);
        // slot 55 -> offset 220
        assert_eq!(&buf[220..224], &[0x02, 0x00, 0xe0, 0x00]);
        // trailer
        assert_eq!(&buf[574..576], &[0xAA, 0x55]);
    }
}
