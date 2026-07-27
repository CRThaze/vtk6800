use thiserror::Error;

/// Errors produced by `vtk6800-core`.
#[derive(Debug, Error)]
pub enum Error {
    #[error("HID error: {0}")]
    Hid(String),

    #[error("keyboard not found (VID {vid:04x} PID {pid:04x}); is it connected and do you have hidraw access?")]
    DeviceNotFound { vid: u16, pid: u16 },

    #[error("config error: {0}")]
    Config(String),

    #[error("encode error: {0}")]
    Encode(String),

    #[error("protocol error: {0}")]
    Protocol(String),

    #[error("device did not acknowledge ({0})")]
    NoAck(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
