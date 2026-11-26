//! penkaku core: documents, layout, device settings, fonts, backend API and settings.
//! Protocol notes: `docs/protocol.md`.

pub mod api;
pub mod calibration;
pub mod chixie3_devices;
pub mod config;
pub mod credentials;
pub mod deformation;
pub mod diagram;
pub mod font;
pub mod font_store;
pub mod handwriting;
pub mod layout;
pub mod path;
pub mod pickle;
pub mod profile;
pub mod session;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("serial: {0}")]
    Serial(#[from] serialport::Error),
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("grbl error on line {line}: {msg}")]
    Grbl { line: usize, msg: String },
    #[error("grbl alarm: {0}")]
    Alarm(String),
    #[error("font: {0}")]
    Font(String),
    #[error("api: {0}")]
    Api(String),
    #[error("credential store: {0}")]
    Credential(String),
    #[error("config: {0}")]
    Config(String),
    #[error("timeout waiting for device")]
    Timeout,
    #[error("cancelled")]
    Cancelled,
}

pub type Result<T> = std::result::Result<T, Error>;

/// Environment variable `PENKAKU_<key>`.
pub fn env_var(key: &str) -> Option<String> {
    std::env::var(format!("PENKAKU_{key}")).ok()
}
