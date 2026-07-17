use thiserror::Error;

use crate::catalog::CatalogError;
use crate::config::ConfigError;
use crate::payment::{PaymentChallenge, PaymentChallengeExt};
use crate::player::PlayerError;
use crate::xtream::XtreamError;

#[derive(Debug, Error)]
pub enum TelevisorError {
    #[error("Player error: {0}")]
    Player(#[from] PlayerError),

    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("Xtream API error: {0}")]
    Xtream(#[from] XtreamError),

    #[error("Catalog error: {0}")]
    Catalog(#[from] CatalogError),
}

impl PaymentChallengeExt for TelevisorError {
    fn payment_challenge(&self) -> Option<&PaymentChallenge> {
        match self {
            TelevisorError::Xtream(e) => e.payment_challenge(),
            TelevisorError::Catalog(e) => e.payment_challenge(),
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, TelevisorError>;
