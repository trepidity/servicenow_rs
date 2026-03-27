pub mod basic;
pub mod token;

use async_trait::async_trait;
use reqwest::RequestBuilder;

use crate::error::Result;

pub use basic::BasicAuth;
pub use token::TokenAuth;

/// Trait for authentication strategies.
///
/// Implementations decorate outgoing HTTP requests with credentials
/// and handle credential lifecycle (e.g., token refresh for OAuth).
#[async_trait]
pub trait Authenticator: Send + Sync + std::fmt::Debug {
    /// Apply credentials to an outgoing request.
    async fn authenticate(&self, request: RequestBuilder) -> Result<RequestBuilder>;

    /// Refresh credentials if expired. No-op for static credentials like basic auth.
    async fn refresh(&self) -> Result<()> {
        Ok(())
    }

    /// Whether this authenticator supports session cookie reuse.
    fn supports_session(&self) -> bool {
        false
    }

    /// A human-readable name for this auth method (for logging/debugging).
    fn method_name(&self) -> &str;
}
