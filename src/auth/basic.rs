use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use reqwest::header::{HeaderValue, AUTHORIZATION};
use reqwest::RequestBuilder;
use zeroize::Zeroizing;

use crate::error::{Error, Result};

use super::Authenticator;

/// HTTP Basic Authentication.
///
/// Builds an `Authorization: Basic` header from username/password input, then
/// retains only the username and encoded header value. The raw password is not
/// stored after construction; the encoded header is credential-equivalent and
/// stored in zeroizing memory. Supports optional session cookie reuse through
/// reqwest's cookie store.
#[derive(Clone)]
pub struct BasicAuth {
    username: String,
    encoded: Zeroizing<String>,
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
        let password = Zeroizing::new(password.into());
        let credentials = Zeroizing::new(format!("{username}:{}", password.as_str()));
        let encoded = Zeroizing::new(STANDARD.encode(credentials.as_bytes()));
        Self {
            username,
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

    fn authorization_header(&self) -> Result<HeaderValue> {
        let header = Zeroizing::new(format!("Basic {}", self.encoded.as_str()));
        let mut value = HeaderValue::from_str(header.as_str())
            .map_err(|err| Error::Config(format!("invalid basic auth header value: {err}")))?;
        value.set_sensitive(true);
        Ok(value)
    }
}

#[async_trait]
impl Authenticator for BasicAuth {
    async fn authenticate(&self, request: RequestBuilder) -> Result<RequestBuilder> {
        Ok(request.header(AUTHORIZATION, self.authorization_header()?))
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
        assert_eq!(auth.encoded.as_str(), expected);
    }

    #[test]
    fn test_debug_redacts_secret_material() {
        let auth = BasicAuth::new("admin", "password123");
        let debug = format!("{auth:?}");
        assert!(debug.contains("admin"));
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("password123"));
        assert!(!debug.contains(&STANDARD.encode("admin:password123")));
    }

    #[tokio::test]
    async fn test_authorization_header_is_sensitive() {
        let auth = BasicAuth::new("admin", "password123");
        let request = auth
            .authenticate(reqwest::Client::new().get("https://example.com"))
            .await
            .expect("authenticate")
            .build()
            .expect("request");
        let header = request
            .headers()
            .get(AUTHORIZATION)
            .expect("authorization header");
        assert!(header.is_sensitive());
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
