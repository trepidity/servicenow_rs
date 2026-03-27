use thiserror::Error;

/// Primary error type for all servicenow_rs operations.
#[derive(Error, Debug)]
pub enum Error {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("authentication failed: {message}")]
    Auth {
        message: String,
        status: Option<u16>,
    },

    #[error("API error ({status}): {message}")]
    Api {
        status: u16,
        message: String,
        detail: Option<String>,
    },

    #[error("rate limited, retry after {retry_after:?}s")]
    RateLimited { retry_after: Option<u64> },

    #[error("schema error: {0}")]
    Schema(String),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("query error: {0}")]
    Query(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("URL parse error: {0}")]
    UrlParse(#[from] url::ParseError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("partial result: {succeeded} succeeded, {failed} failed")]
    PartialResult {
        succeeded: usize,
        failed: usize,
        errors: Vec<Error>,
    },
}

pub type Result<T> = std::result::Result<T, Error>;
