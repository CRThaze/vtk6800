//! Render a keymap as an ASCII/Unicode drawing of the physical keyboard.
//!
//! The physical geometry (row grouping, per-key relative widths) is transcribed
//! from the vendor `KeyboardLayout.xml` (`rect_top`/`rect_left`/`rect_width`) for
//! ANSI/ISO, plus the KBt RE 66-key product photo for the 66-key bottom row. All
//! four SKUs have a template ([`rows_for`]). Each cell shows the *binding* at that
//! slot (decoded from the layer's [`Entry`]), using Unicode glyphs for
//! non-printable keys; the ISO tall Enter spans two rows via a vertical merge.

use vtk6800_core::{keycode, model::Entry, reserved::reserved_fn, Keymap, Layer, LayerId, Variant};

/// Chars of interior per half-unit, so a 1u key (2 half-units) has interior 4,
/// wide enough for 4-character mnemonics.
const CELL: usize = 2;
/// Most keys in any row. Rows with fewer keys pad their widest key so every row
/// is the same width; `GRID_WIDTH` = 65 (the base) + the row's key count.
const MAX_CELLS: usize = 15;
/// Total drawing width in columns (every row is padded to this).
const GRID_WIDTH: usize = 65 + MAX_CELLS;
/// Sentinel "slot" for dead space in a row (e.g. the badged bottom-left corner
/// of the 66-key boards): it reserves width but renders as blank, with no box.
/// Real slots start at 1, so 0 is safe.
const GAP: u8 = 0;

/// SGR reset.
const SGR_OFF: &str = "\x1b[0m";
/// Reverse-video in amber (256-colour 214): marks keys changed since last apply.
const AMBER_ON: &str = "\x1b[7;38;5;214m";
/// Reverse-video in grey (256-colour 244): marks Fn-layer slots the firmware
/// reserves for its own combos (Bluetooth / RGB / Win-lock), which cannot be
/// remapped. See [`vtk6800_core::reserved`].
const LOCK_ON: &str = "\x1b[7;38;5;244m";

/// One rendered key.
struct Cell {
    slot: u8,
    label: String,
    /// Binding differs from the baseline (last applied, else factory default).
    changed: bool,
    /// Firmware-reserved on this layer (Fn only): the hardware owns the combo,
    /// so whatever is flashed here is inert. Rendered greyed, with the firmware
    /// function as its label.
    reserved: bool,
}

// Physical rows as `(slot, half_units)`, left to right, quantised from the XML
// pixel rects so every row sums to 32 half-units (the drawing is a rectangle and
// the right nav column lines up). Rows are shared where the SKUs agree.

/// Number row + backspace + Home; identical across all four SKUs.
const ROW_NUM: &[(u8, u8)] = &[
    (1, 2),
    (20, 2),
    (21, 2),
    (22, 2),
    (23, 2),
    (24, 2),
    (25, 2),
    (26, 2),
    (27, 2),
    (28, 2),
    (29, 2),
    (30, 2),
    (31, 2),
    (103, 4),
    (117, 2),
];

// ANSI upper three rows (backslash on the QWERTY row, one-piece Enter).
const ANSI_R1: &[(u8, u8)] = &[
    (37, 3),
    (38, 2),
    (39, 2),
    (40, 2),
    (41, 2),
    (42, 2),
    (43, 2),
    (44, 2),
    (45, 2),
    (46, 2),
    (47, 2),
    (48, 2),
    (49, 2),
    (67, 3),
    (118, 2),
];
const ANSI_R2: &[(u8, u8)] = &[
    (55, 3),
    (56, 2),
    (57, 2),
    (58, 2),
    (59, 2),
    (60, 2),
    (61, 2),
    (62, 2),
    (63, 2),
    (64, 2),
    (65, 2),
    (66, 2),
    (85, 5),
    (121, 2),
];
const ANSI_R3: &[(u8, u8)] = &[
    (73, 5),
    (74, 2),
    (75, 2),
    (76, 2),
    (77, 2),
    (78, 2),
    (79, 2),
    (80, 2),
    (81, 2),
    (82, 2),
    (83, 2),
    (84, 3),
    (101, 2),
    (120, 2),
];

// ISO upper three rows: no slot 67, the "#~" key (slot 111) left of the tall
// Enter, and the non-US "<>|" key (slot 110) beside a narrower left Shift.
// (Slot numbers equal the controller's table index, which is reversed from the
// keys' left-to-right order for these two; see iso*_base_default.toml.) Enter (85)
// appears on
// both R1 and R2 at the same columns; the renderer merges them into one tall
// key (a grid-aligned stand-in for the L-shaped ISO Enter).
const ISO_R1: &[(u8, u8)] = &[
    (37, 3),
    (38, 2),
    (39, 2),
    (40, 2),
    (41, 2),
    (42, 2),
    (43, 2),
    (44, 2),
    (45, 2),
    (46, 2),
    (47, 2),
    (48, 2),
    (49, 2),
    (85, 3),
    (118, 2),
];
const ISO_R2: &[(u8, u8)] = &[
    (55, 3),
    (56, 2),
    (57, 2),
    (58, 2),
    (59, 2),
    (60, 2),
    (61, 2),
    (62, 2),
    (63, 2),
    (64, 2),
    (65, 2),
    (66, 2),
    (111, 2),
    (85, 3),
    (121, 2),
];
const ISO_R3: &[(u8, u8)] = &[
    (73, 3),
    (110, 2),
    (74, 2),
    (75, 2),
    (76, 2),
    (77, 2),
    (78, 2),
    (79, 2),
    (80, 2),
    (81, 2),
    (82, 2),
    (83, 2),
    (84, 3),
    (101, 2),
    (120, 2),
];

/// Bottom row for 68-key SKUs (Ctrl Win Alt Space Alt Fn Ctrl ← ↓ →).
const ROW_BOTTOM_68: &[(u8, u8)] = &[
    (91, 2),
    (92, 2),
    (93, 2),
    (94, 14),
    (95, 2),
    (96, 2),
    (98, 2),
    (99, 2),
    (100, 2),
    (102, 2),
];
/// Bottom row for 66-key SKUs. Unlike the 68 it has no Win (92) or right Ctrl
/// (98), a shorter spacebar, and the whole cluster is inset from the left: the
/// bottom-left corner holds a badge, not a key, so Ctrl sits under the Z key.
/// The leading `GAP` reserves that corner as blank space; the spacebar (the
/// widest cell) absorbs the row's width deficit as usual.
const ROW_BOTTOM_66: &[(u8, u8)] = &[
    (GAP, 5),
    (91, 2),
    (93, 2),
    (94, 13),
    (95, 2),
    (96, 2),
    (99, 2),
    (100, 2),
    (102, 2),
];

const ANSI68_ROWS: &[&[(u8, u8)]] = &[ROW_NUM, ANSI_R1, ANSI_R2, ANSI_R3, ROW_BOTTOM_68];
const ANSI66_ROWS: &[&[(u8, u8)]] = &[ROW_NUM, ANSI_R1, ANSI_R2, ANSI_R3, ROW_BOTTOM_66];
const ISO68_ROWS: &[&[(u8, u8)]] = &[ROW_NUM, ISO_R1, ISO_R2, ISO_R3, ROW_BOTTOM_68];
const ISO66_ROWS: &[&[(u8, u8)]] = &[ROW_NUM, ISO_R1, ISO_R2, ISO_R3, ROW_BOTTOM_66];

fn rows_for(variant: Variant) -> &'static [&'static [(u8, u8)]] {
    match variant {
        Variant::Ansi68 => ANSI68_ROWS,
        Variant::Ansi66 => ANSI66_ROWS,
        Variant::Iso68 => ISO68_ROWS,
        Variant::Iso66 => ISO66_ROWS,
    }
}

/// How key bindings are labelled in the drawing.
#[derive(Clone, Copy)]
pub enum Labels {
    /// Unicode glyphs for non-printable keys (⎋, ⇥, ⇧, arrows…).
    Glyph,
    /// Short ASCII mnemonics, up to 4 chars (Caps, LCtl, PgUp, Prev…).
    Mnemonic,
}

/// Render every layer of `km` as a keyboard drawing. Firmware-reserved Fn-layer
/// slots (Bluetooth / RGB / Win-lock combos the hardware owns; see
/// [`vtk6800_core::reserved`]) are labelled with their firmware function, and
/// when `color` is set are greyed to flag that a keymap entry there is inert.
/// Keys that differ from `baseline` (the last-applied snapshot, or the factory
/// default) are shown in amber reverse-video. Without `color` the drawing is
/// plain (safe to pipe/redirect); the reserved labels and legend still appear.
pub fn render(km: &Keymap, baseline: &Keymap, labels: Labels, color: bool, slots: bool) -> String {
    let rows = rows_for(km.variant);
    let mut out = format!("variant: {}\n", km.variant.as_str());
    let mut any_changed = false;
    let mut any_reserved = false;
    for layer in &km.layers {
        let name = layer_name(layer.id);
        let base_layer = baseline.layer(layer.id);

        // Bindings drawing.
        out.push_str(&format!("\n{name} layer\n"));
        let grid = render_grid(rows, layer, base_layer, labels, color, false);
        any_changed |= grid.changed;
        any_reserved |= grid.reserved;
        out.push_str(&grid.text);

        // Optional slot-number table, right after the same layer's bindings.
        if slots {
            let slot_grid = render_grid(rows, layer, base_layer, labels, color, true);
            out.push_str(&slot_grid.text);
        }
    }
    if color && any_changed {
        out.push_str("\nAmber: changed since last applied (run `apply` to write).\n");
    }
    if any_reserved {
        let lead = if color { "Grey" } else { "Reserved" };
        out.push_str(&format!(
            "\n{lead}: firmware-reserved on Fn (hardware owns the combo, not remappable). \
             Pair/BT1-3 = Bluetooth pair & device switch; Bri/Spd = RGB brightness & speed; \
             RGBc/RGBm = RGB colour & mode; WLck = Win-lock.\n"
        ));
    }
    out
}

fn layer_name(id: LayerId) -> &'static str {
    match id {
        LayerId::Base => "base",
        LayerId::Fn => "fn",
        LayerId::Alt => "alt",
    }
}

/// One rendered layer grid plus whether it contains any changed / reserved keys
/// (so the caller can print the matching legends).
struct Grid {
    text: String,
    changed: bool,
    reserved: bool,
}

fn render_grid(
    rows: &[&[(u8, u8)]],
    layer: &Layer,
    base_layer: Option<&Layer>,
    labels: Labels,
    color: bool,
    slots: bool,
) -> Grid {
    // Reserved status is Fn-layer-only; on base/alt these same keys are normal.
    let is_fn = layer.id == LayerId::Fn;
    let mut rows_cells: Vec<Vec<Cell>> = Vec::new();
    let mut rows_borders: Vec<Vec<usize>> = Vec::new();
    let mut any_changed = false;
    let mut any_reserved = false;
    for row in rows {
        let mut cells = Vec::new();
        let mut borders = vec![0usize];
        let mut acc = 0usize;
        // Each key gets `hu*CELL` interior columns; the widest key in the row
        // absorbs the deficit so the row reaches the full width.
        let deficit = MAX_CELLS.saturating_sub(row.len());
        let widest = row
            .iter()
            .enumerate()
            .max_by_key(|(_, &(_, hu))| hu)
            .map_or(0, |(i, _)| i);
        // A leading gap (the inset 66-key bottom row) takes one column of the
        // deficit. A cell's natural width `hu*CELL + 1` is odd, but the Z/Y key
        // it must line up under sits on an even column (its own row's left shift
        // is deficit-widened), so without this the inset cluster lands one column
        // to the left.
        let lead_gap = matches!(row.first(), Some(&(GAP, _))) && deficit > 0 && widest != 0;
        for (idx, &(slot, hu)) in row.iter().enumerate() {
            let extra = match idx {
                0 if lead_gap => 1,
                i if i == widest => deficit - usize::from(lead_gap),
                _ => 0,
            };
            acc += hu as usize * CELL + 1 + extra;
            borders.push(acc);
            let reserved = is_fn.then(|| reserved_fn(slot)).flatten();
            any_reserved |= reserved.is_some();
            // In `--slots` mode, label with the hardware slot number. Otherwise a
            // reserved Fn slot shows its firmware function (the keymap entry there
            // is inert); every other slot shows its decoded keymap binding.
            let label = if slot == GAP {
                String::new()
            } else if slots {
                slot.to_string()
            } else if let Some(r) = reserved {
                r.short.to_string()
            } else {
                layer
                    .get(slot)
                    .map(|e| entry_label(e, labels))
                    .unwrap_or_default()
            };
            // Changed = binding differs from the baseline. Reserved slots are
            // inert, so a difference there is not a meaningful "change".
            let changed =
                reserved.is_none() && layer.get(slot) != base_layer.and_then(|b| b.get(slot));
            any_changed |= changed;
            cells.push(Cell {
                slot,
                label,
                changed,
                reserved: reserved.is_some(),
            });
        }
        rows_cells.push(cells);
        rows_borders.push(borders);
    }

    // A key that spans two rows (the ISO Enter) is listed in both; label it only
    // in its lowest row so the upper cell stays blank and merges cleanly.
    let last_row: std::collections::HashMap<u8, usize> = rows_cells
        .iter()
        .enumerate()
        .flat_map(|(ri, cells)| cells.iter().map(move |c| (c.slot, ri)))
        .collect();
    for (ri, cells) in rows_cells.iter_mut().enumerate() {
        for c in cells.iter_mut() {
            if last_row.get(&c.slot) != Some(&ri) {
                c.label.clear();
            }
        }
    }

    // Per-row `(slot, l, r)` spans, used to merge a key that spans two rows
    // (the ISO Enter).
    let rows_spans: Vec<Vec<(u8, usize, usize)>> = rows_cells
        .iter()
        .zip(&rows_borders)
        .map(|(cells, borders)| {
            cells
                .iter()
                .enumerate()
                .map(|(i, c)| (c.slot, borders[i], borders[i + 1]))
                .collect()
        })
        .collect();

    let mut s = String::new();
    // Top edge: nothing above, row 0 below.
    s.push_str(&rule(None, Some(&rows_spans[0])));
    s.push('\n');
    for i in 0..rows_cells.len() {
        s.push_str(&content_line(&rows_borders[i], &rows_cells[i], color));
        s.push('\n');
        let below = rows_spans.get(i + 1).map(Vec::as_slice);
        s.push_str(&rule(Some(&rows_spans[i]), below));
        s.push('\n');
    }
    Grid {
        text: s,
        changed: any_changed,
        reserved: any_reserved,
    }
}

/// A horizontal rule between an `upper` and/or `lower` row (each given as its
/// cells' `(slot, left, right)` spans, or `None` at the drawing's outer edge).
/// Each column's box-drawing glyph is chosen from the four line segments that
/// meet there, considering only real (non-[`GAP`]) cells, so dead space renders
/// blank. A key that spans this rule (an identical span above and below, e.g. the
/// ISO Enter) is carved out: vertical walls, no horizontal line.
fn rule(upper: Option<&[(u8, usize, usize)]>, lower: Option<&[(u8, usize, usize)]>) -> String {
    // Real (box-drawn) cells of a row: gaps reserve width but draw nothing.
    let real = |spans: Option<&[(u8, usize, usize)]>| -> Vec<(u8, usize, usize)> {
        spans
            .into_iter()
            .flatten()
            .filter(|(slot, _, _)| *slot != GAP)
            .copied()
            .collect()
    };
    let us = real(upper);
    let ls = real(lower);
    // Keys continuing through this rule: the SAME slot at the same bounds above
    // and below (the ISO Enter). Matching on slot too keeps stacked but distinct
    // keys (e.g. Home over PgUp in the nav column) from merging.
    let merged: Vec<(usize, usize)> = us
        .iter()
        .filter(|s| ls.contains(s))
        .map(|&(_, l, r)| (l, r))
        .collect();
    let ur: Vec<(usize, usize)> = us.iter().map(|&(_, l, r)| (l, r)).collect();
    let lr: Vec<(usize, usize)> = ls.iter().map(|&(_, l, r)| (l, r)).collect();

    // A vertical wall at `col` (a real cell's left/right edge); a horizontal
    // segment just right of `col` (a real cell's interior or edge span).
    let wall = |bs: &[(usize, usize)], col: usize| bs.iter().any(|&(l, r)| l == col || r == col);
    let bar = |bs: &[(usize, usize)], col: usize| bs.iter().any(|&(l, r)| l <= col && col < r);

    (0..GRID_WIDTH)
        .map(|col| {
            if let Some(&(l, r)) = merged.iter().find(|&&(l, r)| col >= l && col <= r) {
                return if col == l || col == r { '│' } else { ' ' };
            }
            let up = wall(&ur, col);
            let down = wall(&lr, col);
            let left = col > 0 && (bar(&ur, col - 1) || bar(&lr, col - 1));
            let right = bar(&ur, col) || bar(&lr, col);
            box_char(up, down, left, right)
        })
        .collect()
}

/// The box-drawing glyph whose strokes match the four present segments.
fn box_char(up: bool, down: bool, left: bool, right: bool) -> char {
    match (up, down, left, right) {
        (false, false, false, false) => ' ',
        (false, false, false, true) | (false, false, true, false) | (false, false, true, true) => {
            '─'
        }
        (true, false, false, false) | (false, true, false, false) | (true, true, false, false) => {
            '│'
        }
        (false, true, false, true) => '┌',
        (false, true, true, false) => '┐',
        (true, false, false, true) => '└',
        (true, false, true, false) => '┘',
        (false, true, true, true) => '┬',
        (true, false, true, true) => '┴',
        (true, true, false, true) => '├',
        (true, true, true, false) => '┤',
        (true, true, true, true) => '┼',
    }
}

/// Terminal display width of a char. Pictographic emoji (the speaker glyphs)
/// render two cells wide; variation selectors render zero. The base media
/// symbols (`U+23xx`, text presentation) and everything else are single-cell.
fn char_width(c: char) -> usize {
    let u = c as u32;
    if (0x1F300..=0x1FAFF).contains(&u) {
        2 // pictographic emoji (speakers): always emoji, double-width
    } else if (0xFE00..=0xFE0F).contains(&u) {
        0 // variation selectors
    } else {
        1
    }
}

fn display_width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

/// Truncate `s` to at most `max` display columns (keeps whole chars).
fn take_width(s: &str, max: usize) -> String {
    let mut w = 0;
    let mut out = String::new();
    for c in s.chars() {
        let cw = char_width(c);
        if w + cw > max {
            break;
        }
        w += cw;
        out.push(c);
    }
    out
}

fn content_line(borders: &[usize], cells: &[Cell], color: bool) -> String {
    // Built cell-by-cell in *display columns* so double-width (emoji) labels
    // still land inside their cell and the grid stays aligned.
    let mut out = String::new();
    for (i, cell) in cells.iter().enumerate() {
        let interior = borders[i + 1] - borders[i] - 1;
        // A wall precedes this cell if it or its left neighbour is a real key; a
        // gap between two blanks (or at the drawing edge) has no wall.
        let this_real = cell.slot != GAP;
        let prev_real = i > 0 && cells[i - 1].slot != GAP;
        out.push(if this_real || prev_real { '│' } else { ' ' });
        if !this_real {
            out.push_str(&" ".repeat(interior));
            continue;
        }

        // Label centered in the interior, measured in display columns.
        let label = take_width(&cell.label, interior);
        let lw = display_width(&label);
        let padl = " ".repeat((interior - lw) / 2);
        let padr = " ".repeat(interior - lw - (interior - lw) / 2);
        // Reserved (grey) wins over changed (amber): a firmware-owned slot is
        // inert, so flagging it as "changed" would mislead.
        let sgr = if !color {
            None
        } else if cell.reserved {
            Some(LOCK_ON)
        } else if cell.changed {
            Some(AMBER_ON)
        } else {
            None
        };
        if let Some(on) = sgr {
            out.push_str(on);
            out.push_str(&padl);
            out.push_str(&label);
            out.push_str(&padr);
            out.push_str(SGR_OFF);
        } else {
            out.push_str(&padl);
            out.push_str(&label);
            out.push_str(&padr);
        }
    }
    // Closing wall, unless the row ends in dead space.
    out.push(if cells.last().is_some_and(|c| c.slot != GAP) {
        '│'
    } else {
        ' '
    });
    out
}

/// Compact label for a binding: Unicode glyphs for non-printable keys, the key
/// name otherwise. An unassigned slot renders blank.
fn entry_label(e: Entry, labels: Labels) -> String {
    match e {
        Entry::Unassigned => String::new(),
        Entry::Key { hid, .. } => key_label(hid, labels),
        Entry::Consumer { code } => match labels {
            Labels::Glyph => consumer_glyph(code),
            Labels::Mnemonic => consumer_mnemonic(code),
        },
        Entry::Media { .. } => match labels {
            Labels::Glyph => "▸".into(),
            Labels::Mnemonic => "Sys".into(),
        },
        Entry::Macro { index, .. } => format!("M{index}"),
        Entry::Mouse { .. } => "Ms".into(),
        Entry::Raw { .. } => "?".into(),
    }
}

fn key_label(hid: u8, labels: Labels) -> String {
    // Non-printable keys carry a Unicode glyph and an up-to-4-char mnemonic.
    let special = match hid {
        0x29 => Some(("⎋", "Esc")),
        0x2b => Some(("⇥", "Tab")),
        0x39 => Some(("⇪", "Caps")),
        0x28 => Some(("⏎", "Entr")),
        0x2a => Some(("⌫", "Bksp")),
        0x2c => Some(("␣", "Spc")),
        // Modifiers distinguish left/right. Glyph mode leaves the left key
        // canonical and marks the right with `R`; mnemonic mode is symmetric.
        0xe0 => Some(("Ctrl", "LCtl")), // left ctrl
        0xe4 => Some(("RCtl", "RCtl")), // right ctrl
        0xe1 => Some(("⇧", "LSft")),    // left shift
        0xe5 => Some(("R⇧", "RSft")),   // right shift
        0xe2 => Some(("Alt", "LAlt")),  // left alt
        0xe6 => Some(("RAlt", "RAlt")), // right alt
        0xe3 => Some(("Win", "LWin")),  // left gui
        0xe7 => Some(("RWin", "RWin")), // right gui
        0xaf => Some(("Fn", "Fn")),
        0x4f => Some(("→", "Rght")),
        0x50 => Some(("←", "Left")),
        0x51 => Some(("↓", "Down")),
        0x52 => Some(("↑", "Up")),
        0x4a => Some(("⇱", "Home")),
        0x4d => Some(("⇲", "End")),
        0x4b => Some(("⇞", "PgUp")),
        0x4e => Some(("⇟", "PgDn")),
        0x4c => Some(("⌦", "Del")),
        0x49 => Some(("⎀", "Ins")),
        0x46 => Some(("⎙", "PrtS")),
        0x47 => Some(("↕", "ScrL")),
        0x48 => Some(("Paus", "Paus")),
        0x65 => Some(("▤", "Menu")), // application / menu key
        // Keypad / numpad cluster: `kp` + the key's own symbol/digit. NumLock has
        // no symbol so it keeps a mnemonic; keypad Enter uses the ⏎ glyph.
        0x53 => Some(("NmLk", "NmLk")),
        0x54 => Some(("kp/", "kp/")),
        0x55 => Some(("kp*", "kp*")),
        0x56 => Some(("kp-", "kp-")),
        0x57 => Some(("kp+", "kp+")),
        0x58 => Some(("kp⏎", "kp⏎")),
        0x59 => Some(("kp1", "kp1")),
        0x5a => Some(("kp2", "kp2")),
        0x5b => Some(("kp3", "kp3")),
        0x5c => Some(("kp4", "kp4")),
        0x5d => Some(("kp5", "kp5")),
        0x5e => Some(("kp6", "kp6")),
        0x5f => Some(("kp7", "kp7")),
        0x60 => Some(("kp8", "kp8")),
        0x61 => Some(("kp9", "kp9")),
        0x62 => Some(("kp0", "kp0")),
        0x63 => Some(("kp.", "kp.")),
        0x67 => Some(("kp=", "kp=")),
        0x85 => Some(("kp,", "kp,")),
        0x86 => Some(("kp=", "kp=")),
        _ => None,
    };
    if let Some((glyph, mnemonic)) = special {
        return match labels {
            Labels::Glyph => glyph,
            Labels::Mnemonic => mnemonic,
        }
        .to_string();
    }
    // Printable punctuation renders as its symbol in either style.
    let sym = match hid {
        0x2d => "-",
        0x2e => "=",
        0x2f => "[",
        0x30 => "]",
        0x31 => "\\",
        0x32 => "#",
        0x64 => "\\",
        0x33 => ";",
        0x34 => "'",
        0x35 => "`",
        0x36 => ",",
        0x37 => ".",
        0x38 => "/",
        _ => "",
    };
    if !sym.is_empty() {
        return sym.to_string();
    }
    keycode::name_from_hid(hid)
        .map(str::to_uppercase)
        .unwrap_or_else(|| format!("{hid:02x}"))
}

/// Media-control glyph per consumer/media code. Transport symbols use their base
/// (text-presentation, single-cell) form, with no `U+FE0F`, so they render as plain
/// glyphs rather than colour emoji. The speaker symbols have no text form, so
/// they stay as the (two-cell) base emoji; the renderer measures display width.
fn consumer_glyph(code: u8) -> String {
    match code {
        0xb6 => "⏮",  // previous track
        0xcd => "⏯",  // play / pause
        0xb5 => "⏭",  // next track
        0xb7 => "⏹",  // stop
        0xe2 => "🔇", // mute
        0xe9 => "🔊", // volume up
        0xea => "🔉", // volume down
        _ => "⏯",
    }
    .to_string()
}

/// Three-letter mnemonic for a consumer/media code.
fn consumer_mnemonic(code: u8) -> String {
    match code {
        0xcd => "Play",
        0xb5 => "Next",
        0xb6 => "Prev",
        0xb7 => "Stop",
        0xe2 => "Mute",
        0xe9 => "Vol+",
        0xea => "Vol-",
        _ => return format!("{code:02x}"),
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// The drawing must place exactly the slots each variant actually defines:
    /// no key missing from the picture, and no cell drawn for a slot the board
    /// does not have. This is what keeps the 66- vs 68-key and ANSI vs ISO
    /// renders faithful to the hardware.
    #[test]
    fn every_variant_layout_covers_exactly_its_slots() {
        for v in Variant::ALL {
            let placed: BTreeSet<u8> = rows_for(v)
                .iter()
                .flat_map(|row| row.iter().map(|&(slot, _)| slot))
                .filter(|&slot| slot != GAP)
                .collect();
            let defined: BTreeSet<u8> = v
                .key_slots()
                .unwrap()
                .into_iter()
                .map(|(slot, _, _)| slot)
                .collect();
            let missing: Vec<_> = defined.difference(&placed).collect();
            let extra: Vec<_> = placed.difference(&defined).collect();
            assert!(
                missing.is_empty() && extra.is_empty(),
                "{v:?}: layout is missing slots {missing:?} and has stray slots {extra:?}"
            );
        }
    }

    /// Every physical row must span the same 32 half-units, or keys stop lining
    /// up vertically across rows.
    #[test]
    fn every_row_spans_full_width() {
        for v in Variant::ALL {
            for row in rows_for(v) {
                let half_units: u32 = row.iter().map(|&(_, hu)| hu as u32).sum();
                assert_eq!(
                    half_units, 32,
                    "{v:?}: a row spans {half_units} half-units, not 32"
                );
            }
        }
    }
}
