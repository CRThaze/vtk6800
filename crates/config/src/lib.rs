//! Shared frontend support for the vtk6800 CLI, TUI, and GUI: local config-file
//! storage (keymaps, presets, settings, the applied snapshot) and Linux host
//! integration (udev rule management).
//!
//! This crate sits above [`vtk6800_core`] (which stays hardware-independent and
//! never touches the filesystem) and below the UI binaries, so all three
//! frontends read and write the same files through one implementation.

pub mod config;
#[cfg(target_os = "linux")]
pub mod udev;
