//! Core errors folded into the closed set of [`ErrorKind`]s a frontend
//! can act on.

use televisor_core::catalog::CatalogError;
use televisor_core::config::ConfigError;
use televisor_core::xtream::XtreamError;
use televisor_remux::RemuxError;

use crate::protocol::{ErrorBody, ErrorKind};

#[derive(Debug)]
pub struct BridgeError {
    pub kind: ErrorKind,
    pub message: String,
}

impl BridgeError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn into_body(self) -> ErrorBody {
        ErrorBody {
            kind: self.kind,
            message: self.message,
        }
    }
}

pub type Result<T> = std::result::Result<T, BridgeError>;

impl From<XtreamError> for BridgeError {
    fn from(err: XtreamError) -> Self {
        let kind = match &err {
            XtreamError::Http(_) => ErrorKind::Network,
            XtreamError::Status { .. } | XtreamError::Decode { .. } => ErrorKind::Server,
            XtreamError::PaymentRequired(_) => ErrorKind::PaymentRequired,
            XtreamError::AuthFailed { .. } => ErrorKind::AuthFailed,
            XtreamError::InvalidHost { .. } => ErrorKind::InvalidHost,
            XtreamError::M3uOnly { .. } => ErrorKind::LiveOnly,
        };
        // Display strips credentials: `XtreamError::Http` drops its URL.
        Self::new(kind, err.to_string())
    }
}

impl From<CatalogError> for BridgeError {
    fn from(err: CatalogError) -> Self {
        match err {
            CatalogError::Xtream(inner) => inner.into(),
            CatalogError::CategoryNotFound(_) => Self::new(ErrorKind::NotFound, err.to_string()),
            CatalogError::NoCacheDir | CatalogError::Io { .. } => {
                Self::new(ErrorKind::Storage, err.to_string())
            }
        }
    }
}

impl From<ConfigError> for BridgeError {
    fn from(err: ConfigError) -> Self {
        let kind = match &err {
            ConfigError::InvalidHost { .. } => ErrorKind::InvalidHost,
            ConfigError::MissingUsername | ConfigError::InvalidAccountName(_) => {
                ErrorKind::InvalidRequest
            }
            ConfigError::NoConfigDir | ConfigError::Io { .. } | ConfigError::Parse(_) => {
                ErrorKind::Storage
            }
            // Account-file management is the CLI's job; the bridge never
            // reads the config file, so these cannot arise from a request.
            ConfigError::NoAccounts
            | ConfigError::AccountNotFound { .. }
            | ConfigError::NoDefaultAccount { .. }
            | ConfigError::AccountExists(_)
            | ConfigError::Interactive(_) => ErrorKind::InvalidRequest,
        };
        Self::new(kind, err.to_string())
    }
}

impl From<RemuxError> for BridgeError {
    fn from(err: RemuxError) -> Self {
        let kind = match &err {
            RemuxError::Upstream(_) => ErrorKind::Network,
            // A file the remuxer can't read may still play in VLC.
            RemuxError::Parse(_) | RemuxError::Unsupported(_) => ErrorKind::Unsupported,
            RemuxError::Io(_) => ErrorKind::Server,
        };
        // Upstream errors are built without the URL, which holds credentials.
        Self::new(kind, err.to_string())
    }
}
