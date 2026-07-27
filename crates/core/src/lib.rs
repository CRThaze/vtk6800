//! `vtk6800-core`: hardware-independent library for configuring the Vortex
//! PC66/PC68 (VTK-6800) keyboard: HID transport, the reverse-engineered feature
//! report protocol, the keymap model, and per-variant layouts.
//!
//! See `docs/PROTOCOL.md` for the wire protocol this implements.

pub mod device;
pub mod document;
pub mod encode;
pub mod error;
pub mod keycode;
pub mod macro_table;
pub mod model;
pub mod protocol;
pub mod reserved;
pub mod transport;
pub mod variant;

pub use device::Device;
pub use error::{Error, Result};
pub use model::{
    Entry, FnMode, KeyMapping, Keymap, Layer, LayerId, Macro, MacroEvent, MouseAction, MouseButton,
};
pub use transport::{DryRunTransport, FeatureTransport, HidTransport, MockTransport};
pub use variant::Variant;

/// USB vendor id reported by the keyboard (a cloned Apple id).
pub const VID: u16 = 0x05ac;
/// USB product id reported by the keyboard.
pub const PID: u16 = 0x0256;
