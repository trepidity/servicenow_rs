use async_trait::async_trait;
use reqwest::header::HeaderValue;
use reqwest::RequestBuilder;
use zeroize::Zeroizing;

use crate::error::{Error, Result};

use super::Authenticator;

/// API Token / Bearer Token authentication.
///
/// Sends the token as a Bearer token in the Authorization header.
/// Some ServiceNow instances may also accept tokens via custom headers.
/// Retained token material is stored in zeroizing memory and redacted from
/// `Debug` output.
///
/// # Examples
///
/// ```no_run
/// use servicenow_rs::prelude::*;
/// use servicenow_rs::auth::TokenAuth;
///
/// # async fn example() -> servicenow_rs::error::Result<()> {
/// let client = ServiceNowClient::builder()
///     .instance("mycompany")
///     .auth(TokenAuth::bearer("my-api-token"))
///     .build()
///     .await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct TokenAuth {
    token: Zeroizing<String>,
    header_name: String,
    header_prefix: Option<String>,
}

impl std::fmt::Debug for TokenAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenAuth")
            .field("token", &"[REDACTED]")
            .field("header_name", &self.header_name)
            .field("header_prefix", &self.header_prefix)
            .finish()
    }
}

impl TokenAuth {
    /// Create a Bearer token auth (Authorization: Bearer <token>).
    pub fn bearer(token: impl Into<String>) -> Self {
        Self {
            token: Zeroizing::new(token.into()),
            header_name: "Authorization".to_string(),
            header_prefix: Some("Bearer".to_string()),
        }
    }

    /// Create a token auth with a custom header name and no prefix.
    ///
    /// Useful for ServiceNow instances that accept tokens via custom headers
    /// like `X-sn-api-token` or similar.
    pub fn custom_header(header_name: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            token: Zeroizing::new(token.into()),
            header_name: header_name.into(),
            header_prefix: None,
        }
    }

    /// Create from the `SERVICENOW_API_TOKEN` environment variable.
    pub fn from_env() -> Result<Self> {
        let token = std::env::var("SERVICENOW_API_TOKEN").map_err(|_| {
            crate::error::Error::Config("SERVICENOW_API_TOKEN environment variable not set".into())
        })?;
        Ok(Self::bearer(token))
    }

    fn authorization_value(&self) -> Result<HeaderValue> {
        let raw_value = if let Some(ref prefix) = self.header_prefix {
            Zeroizing::new(format!("{} {}", prefix, self.token.as_str()))
        } else {
            Zeroizing::new(self.token.as_str().to_string())
        };
        let mut value = HeaderValue::from_str(raw_value.as_str())
            .map_err(|err| Error::Config(format!("invalid token auth header value: {err}")))?;
        value.set_sensitive(true);
        Ok(value)
    }
}

#[async_trait]
impl Authenticator for TokenAuth {
    async fn authenticate(&self, request: RequestBuilder) -> Result<RequestBuilder> {
        Ok(request.header(&self.header_name, self.authorization_value()?))
    }

    fn supports_session(&self) -> bool {
        false
    }

    fn method_name(&self) -> &str {
        "token"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bearer_token() {
        let auth = TokenAuth::bearer("my-secret-token");
        assert_eq!(auth.header_name, "Authorization");
        assert_eq!(auth.header_prefix.as_deref(), Some("Bearer"));
        assert_eq!(auth.method_name(), "token");
    }

    #[test]
    fn test_custom_header() {
        let auth = TokenAuth::custom_header("X-sn-api-token", "my-token");
        assert_eq!(auth.header_name, "X-sn-api-token");
        assert!(auth.header_prefix.is_none());
    }

    #[test]
    fn test_no_session_support() {
        let auth = TokenAuth::bearer("token");
        assert!(!auth.supports_session());
    }

    #[test]
    fn test_debug_redacts_token() {
        let auth = TokenAuth::bearer("my-secret-token");
        let debug = format!("{auth:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("my-secret-token"));
    }

    #[tokio::test]
    async fn test_authorization_header_is_sensitive() {
        let auth = TokenAuth::bearer("my-secret-token");
        let request = auth
            .authenticate(reqwest::Client::new().get("https://example.com"))
            .await
            .expect("authenticate")
            .build()
            .expect("request");
        let header = request
            .headers()
            .get("Authorization")
            .expect("authorization header");
        assert!(header.is_sensitive());
    }
}
