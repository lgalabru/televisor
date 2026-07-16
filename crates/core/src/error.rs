use thiserror::Error;

use crate::catalog::CatalogError;
use crate::config::ConfigError;
use crate::vlc::VlcError;
use crate::xtream::XtreamError;

#[derive(Debug, Error)]
pub enum TelevisorError {
    #[error("VLC error: {0}")]
    Vlc(#[from] VlcError),

    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("Xtream API error: {0}")]
    Xtream(#[from] XtreamError),

    #[error("Catalog error: {0}")]
    Catalog(#[from] CatalogError),
}

pub type Result<T> = std::result::Result<T, TelevisorError>;
