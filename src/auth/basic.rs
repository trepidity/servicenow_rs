use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use reqwest::RequestBuilder;

use crate::error::Result;

use super::Authenticator;

/// HTTP Basic Authentication.
///
/// Encodes username:password as Base64 and sets the Authorization header.
/// Supports session cookie reuse to avoid re-authenticating every request.
#[derive(Clone)]
pub struct BasicAuth {
    username: String,
    #[allow(dead_code)]
    password: String,
    encoded: String,
    use_session: bool,
}

impl std::fmt::Debug for BasicAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BasicAuth")
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .field("encoded", &"[REDACTED]")
            .field("use_session", &self.use_session)
            .finish()
    }
}

impl BasicAuth {
    /// Create a new BasicAuth with username and password.
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        let username = username.into();
        let password = password.into();
        let encoded = STANDARD.encode(format!("{}:{}", username, password));
        Self {
            username,
            password,
            encoded,
            use_session: true,
        }
    }

    /// Create from environment variables `SERVICENOW_USERNAME` and `SERVICENOW_PASSWORD`.
    pub fn from_env() -> Result<Self> {
        let username = std::env::var("SERVICENOW_USERNAME").map_err(|_| {
            crate::error::Error::Config("SERVICENOW_USERNAME environment variable not set".into())
        })?;
        let password = std::env::var("SERVICENOW_PASSWORD").map_err(|_| {
            crate::error::Error::Config("SERVICENOW_PASSWORD environment variable not set".into())
        })?;
        Ok(Self::new(username, password))
    }

    /// Disable session cookie reuse.
    pub fn without_session(mut self) -> Self {
        self.use_session = false;
        self
    }

    /// Get the username.
    pub fn username(&self) -> &str {
        &self.username
    }
}

#[async_trait]
impl Authenticator for BasicAuth {
    async fn authenticate(&self, request: RequestBuilder) -> Result<RequestBuilder> {
        Ok(request.header("Authorization", format!("Basic {}", self.encoded)))
    }

    fn supports_session(&self) -> bool {
        self.use_session
    }

    fn method_name(&self) -> &str {
        "basic"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_auth_encoding() {
        let auth = BasicAuth::new("admin", "password123");
        let expected = STANDARD.encode("admin:password123");
        assert_eq!(auth.encoded, expected);
    }

    #[test]
    fn test_supports_session_default() {
        let auth = BasicAuth::new("admin", "pass");
        assert!(auth.supports_session());
    }

    #[test]
    fn test_without_session() {
        let auth = BasicAuth::new("admin", "pass").without_session();
        assert!(!auth.supports_session());
    }

    #[test]
    fn test_method_name() {
        let auth = BasicAuth::new("admin", "pass");
        assert_eq!(auth.method_name(), "basic");
    }
}
