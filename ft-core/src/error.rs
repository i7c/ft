use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("vault not found; searched:\n{}", .tried.join("\n"))]
    VaultNotFound { tried: Vec<String> },

    #[error("config error in {path}: {source}")]
    Config {
        path: String,
        source: Box<figment::Error>,
    },

    #[error("I/O error at {}: {source}", .path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

pub type Result<T> = std::result::Result<T, Error>;
