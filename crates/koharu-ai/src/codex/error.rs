use std::time::Duration;

use reqwest::StatusCode;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, CodexError>;

#[derive(Debug, Error)]
pub enum CodexError {
    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("middleware request failed: {0}")]
    Middleware(#[from] reqwest_middleware::Error),

    #[error("failed to build runtime http client: {0}")]
    RuntimeHttpClient(#[source] anyhow::Error),

    #[error("failed to serialize or parse json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("secret storage failed: {0}")]
    SecretStore(#[source] anyhow::Error),

    #[error("missing stored Codex OAuth token")]
    MissingToken,

    #[error("stored Codex OAuth token is invalid: {0}")]
    InvalidStoredToken(String),

    #[error("stored Codex OAuth token does not include a refresh token")]
    MissingRefreshToken,

    #[error("device code authorization timed out after {0:?}")]
    DeviceCodeTimeout(Duration),

    #[error("{endpoint} returned {status}: {body}")]
    HttpStatus {
        endpoint: String,
        status: StatusCode,
        body: String,
    },
}
