use serde::Serialize;
use thiserror::Error;

pub type CoreResult<T> = Result<T, CoreError>;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("{0}")]
    Validation(String),
    #[error("{0}")]
    Authentication(String),
    #[error("{0}")]
    Network(String),
    #[error("{0}")]
    Protocol(String),
    #[error("{0}")]
    Storage(String),
    #[error("{0}")]
    SecretStore(String),
    #[error("{0}")]
    Target(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Drift(String),
}

impl CoreError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Validation(_) => "VALIDATION_ERROR",
            Self::Authentication(_) => "AUTHENTICATION_ERROR",
            Self::Network(_) => "NETWORK_ERROR",
            Self::Protocol(_) => "PROTOCOL_ERROR",
            Self::Storage(_) => "STORAGE_ERROR",
            Self::SecretStore(_) => "SECRET_STORE_ERROR",
            Self::Target(_) => "TARGET_ERROR",
            Self::Conflict(_) => "CONFLICT_ERROR",
            Self::Drift(_) => "DRIFT_ERROR",
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

impl From<CoreError> for CommandError {
    fn from(error: CoreError) -> Self {
        Self {
            code: error.code().to_string(),
            message: error.to_string(),
        }
    }
}

impl From<rusqlite::Error> for CoreError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(error.to_string())
    }
}

impl From<std::io::Error> for CoreError {
    fn from(error: std::io::Error) -> Self {
        Self::Storage(error.to_string())
    }
}
