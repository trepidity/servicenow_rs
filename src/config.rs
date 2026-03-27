use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{Error, Result};

/// Top-level configuration, typically loaded from `servicenow.toml`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub instance: InstanceConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub schema: SchemaFileConfig,
    #[serde(default)]
    pub transport: TransportConfig,
}

/// Instance connection settings.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct InstanceConfig {
    /// Instance URL or short name (e.g., "mycompany" or "https://mycompany.service-now.com").
    pub url: Option<String>,
}

/// Authentication settings.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct AuthConfig {
    /// Auth method: "basic", "oauth", "token".
    pub method: Option<String>,
    /// Username for basic auth.
    pub username: Option<String>,
    /// Password for basic auth.
    pub password: Option<String>,
    /// OAuth sub-config.
    pub oauth: Option<OAuthConfig>,
    /// API token for token auth.
    pub token: Option<String>,
}

/// OAuth-specific configuration.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct OAuthConfig {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
}

/// Schema configuration from file.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct SchemaFileConfig {
    /// ServiceNow release name (e.g., "xanadu").
    pub release: Option<String>,
    /// Path to custom overlay JSON file.
    pub overlay: Option<String>,
}

/// Transport/HTTP settings.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct TransportConfig {
    /// Request timeout in seconds.
    pub timeout_secs: Option<u64>,
    /// Maximum retry attempts.
    pub max_retries: Option<u32>,
    /// Rate limit (requests per second).
    pub rate_limit: Option<u32>,
}

impl Config {
    /// Load configuration from a TOML file.
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            Error::Config(format!(
                "failed to read config file {}: {}",
                path.display(),
                e
            ))
        })?;
        toml::from_str(&content).map_err(|e| {
            Error::Config(format!(
                "failed to parse config file {}: {}",
                path.display(),
                e
            ))
        })
    }

    /// Load configuration from `servicenow.toml` in the current directory.
    pub fn from_default_file() -> Result<Option<Self>> {
        let path = PathBuf::from("servicenow.toml");
        if path.exists() {
            Ok(Some(Self::from_file(&path)?))
        } else {
            Ok(None)
        }
    }

    /// Apply environment variable overrides on top of this config.
    ///
    /// Environment variables take precedence over file values.
    pub fn apply_env(&mut self) {
        if let Ok(url) = std::env::var("SERVICENOW_INSTANCE") {
            self.instance.url = Some(url);
        }
        if let Ok(username) = std::env::var("SERVICENOW_USERNAME") {
            self.auth.username = Some(username);
        }
        if let Ok(password) = std::env::var("SERVICENOW_PASSWORD") {
            self.auth.password = Some(password);
        }
        if let Ok(client_id) = std::env::var("SERVICENOW_OAUTH_CLIENT_ID") {
            let oauth = self.auth.oauth.get_or_insert_with(OAuthConfig::default);
            oauth.client_id = Some(client_id);
        }
        if let Ok(client_secret) = std::env::var("SERVICENOW_OAUTH_CLIENT_SECRET") {
            let oauth = self.auth.oauth.get_or_insert_with(OAuthConfig::default);
            oauth.client_secret = Some(client_secret);
        }
        if let Ok(token) = std::env::var("SERVICENOW_API_TOKEN") {
            self.auth.token = Some(token);
        }
        if let Ok(schema) = std::env::var("SERVICENOW_SCHEMA_PATH") {
            self.schema.overlay = Some(schema);
        }
    }
}

/// Normalize an instance URL/name into a full base URL.
///
/// - `"mycompany"` -> `"https://mycompany.service-now.com"`
/// - `"https://mycompany.service-now.com/"` -> `"https://mycompany.service-now.com"`
/// - `"https://servicenow.mycompany.com"` -> `"https://servicenow.mycompany.com"`
pub fn normalize_instance_url(input: &str) -> Result<String> {
    let input = input.trim();

    if input.is_empty() {
        return Err(Error::Config("instance URL/name cannot be empty".into()));
    }

    let with_scheme = if input.starts_with("http://") || input.starts_with("https://") {
        input.to_string()
    } else if !input.contains('.') {
        // Bare instance name — add scheme and domain.
        format!("https://{}.service-now.com", input)
    } else {
        // Has dots but no scheme — add https.
        format!("https://{}", input)
    };

    // Validate and normalize.
    let url = url::Url::parse(&with_scheme).map_err(|e| {
        Error::Config(format!("invalid instance URL '{}': {}", input, e))
    })?;

    // Strip trailing slash.
    let mut result = url.to_string();
    while result.ends_with('/') {
        result.pop();
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_bare_name() {
        assert_eq!(
            normalize_instance_url("mycompany").unwrap(),
            "https://mycompany.service-now.com"
        );
    }

    #[test]
    fn test_normalize_full_url() {
        assert_eq!(
            normalize_instance_url("https://mycompany.service-now.com").unwrap(),
            "https://mycompany.service-now.com"
        );
    }

    #[test]
    fn test_normalize_trailing_slash() {
        assert_eq!(
            normalize_instance_url("https://mycompany.service-now.com/").unwrap(),
            "https://mycompany.service-now.com"
        );
    }

    #[test]
    fn test_normalize_custom_domain() {
        assert_eq!(
            normalize_instance_url("servicenow.mycompany.com").unwrap(),
            "https://servicenow.mycompany.com"
        );
    }

    #[test]
    fn test_normalize_empty() {
        assert!(normalize_instance_url("").is_err());
    }

    #[test]
    fn test_toml_parsing() {
        let toml = r#"
[instance]
url = "mycompany"

[auth]
method = "basic"
username = "admin"
password = "secret"

[schema]
release = "xanadu"

[transport]
timeout_secs = 30
max_retries = 3
rate_limit = 20
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.instance.url.as_deref(), Some("mycompany"));
        assert_eq!(config.auth.method.as_deref(), Some("basic"));
        assert_eq!(config.auth.username.as_deref(), Some("admin"));
        assert_eq!(config.transport.timeout_secs, Some(30));
        assert_eq!(config.transport.rate_limit, Some(20));
    }
}
