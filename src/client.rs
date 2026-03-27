use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use url::Url;

use crate::auth::basic::BasicAuth;
use crate::auth::Authenticator;
use crate::config::{self, Config};
use crate::error::{Error, Result};
use crate::query::builder::TableApi;
use crate::schema::registry::SchemaRegistry;
use crate::transport::http::HttpTransport;
use crate::transport::retry::{RateLimiter, RetryConfig};

/// The primary client for interacting with a ServiceNow instance.
///
/// Create via `ServiceNowClient::builder()`, `from_env()`, or `from_config()`.
///
/// # Examples
///
/// ```no_run
/// use servicenow_rs::prelude::*;
///
/// # async fn example() -> servicenow_rs::error::Result<()> {
/// let client = ServiceNowClient::builder()
///     .instance("mycompany")
///     .auth(BasicAuth::new("admin", "password"))
///     .build()
///     .await?;
///
/// let changes = client.table("change_request")
///     .equals("state", "1")
///     .limit(10)
///     .execute()
///     .await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct ServiceNowClient {
    transport: Arc<HttpTransport>,
    schema: Option<Arc<SchemaRegistry>>,
}

impl ServiceNowClient {
    /// Create a new client builder.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// Create a client from environment variables.
    ///
    /// Reads `SERVICENOW_INSTANCE`, `SERVICENOW_USERNAME`, `SERVICENOW_PASSWORD`, etc.
    pub async fn from_env() -> Result<Self> {
        Self::builder().from_env().build().await
    }

    /// Create a client from `servicenow.toml` in the current directory.
    ///
    /// Falls back to environment variables if the file doesn't exist.
    pub async fn from_config() -> Result<Self> {
        Self::builder().from_default_config().from_env().build().await
    }

    /// Create a client from a specific config file path.
    pub async fn from_config_file(path: impl AsRef<Path>) -> Result<Self> {
        Self::builder()
            .from_config_file(path)
            .from_env()
            .build()
            .await
    }

    /// Start building a query or operation on a table.
    pub fn table(&self, name: &str) -> TableApi {
        TableApi::new(
            Arc::clone(&self.transport),
            self.schema.as_ref().map(Arc::clone),
            name,
        )
    }

    /// Get a reference to the schema registry, if loaded.
    pub fn schema(&self) -> Option<&SchemaRegistry> {
        self.schema.as_deref()
    }
}

/// Builder for constructing a `ServiceNowClient` with layered configuration.
///
/// Configuration precedence (highest wins):
/// 1. Builder methods (explicit code)
/// 2. Environment variables
/// 3. Config file (servicenow.toml)
/// 4. Defaults
#[derive(Default)]
pub struct ClientBuilder {
    config: Config,
    instance_override: Option<String>,
    auth_override: Option<Box<dyn Authenticator>>,
    schema_release: Option<String>,
    schema_overlay_path: Option<PathBuf>,
    max_retries: Option<u32>,
    timeout: Option<Duration>,
    rate_limit: Option<u32>,
}

impl ClientBuilder {
    /// Set the instance URL or name.
    pub fn instance(mut self, instance: impl Into<String>) -> Self {
        self.instance_override = Some(instance.into());
        self
    }

    /// Set the authentication method.
    pub fn auth(mut self, auth: impl Authenticator + 'static) -> Self {
        self.auth_override = Some(Box::new(auth));
        self
    }

    /// Set the schema release to load (e.g., "xanadu").
    pub fn schema_release(mut self, release: impl Into<String>) -> Self {
        self.schema_release = Some(release.into());
        self
    }

    /// Set the path to a custom schema overlay file.
    pub fn schema_overlay(mut self, path: impl Into<PathBuf>) -> Self {
        self.schema_overlay_path = Some(path.into());
        self
    }

    /// Set the maximum number of retry attempts.
    pub fn max_retries(mut self, n: u32) -> Self {
        self.max_retries = Some(n);
        self
    }

    /// Set the request timeout.
    pub fn timeout(mut self, duration: Duration) -> Self {
        self.timeout = Some(duration);
        self
    }

    /// Set the rate limit (requests per second).
    pub fn rate_limit(mut self, rps: u32) -> Self {
        self.rate_limit = Some(rps);
        self
    }

    /// Load configuration from a TOML file.
    pub fn from_config_file(mut self, path: impl AsRef<Path>) -> Self {
        match Config::from_file(path.as_ref()) {
            Ok(config) => self.config = config,
            Err(e) => {
                tracing::warn!(error = %e, "failed to load config file, using defaults");
            }
        }
        self
    }

    /// Load configuration from the default `servicenow.toml` file.
    pub fn from_default_config(mut self) -> Self {
        match Config::from_default_file() {
            Ok(Some(config)) => self.config = config,
            Ok(None) => {} // No file, that's fine.
            Err(e) => {
                tracing::warn!(error = %e, "failed to load default config, using defaults");
            }
        }
        self
    }

    /// Apply environment variable overrides.
    pub fn from_env(mut self) -> Self {
        self.config.apply_env();
        self
    }

    /// Build the client.
    pub async fn build(self) -> Result<ServiceNowClient> {
        // Destructure to avoid partial-move issues.
        let ClientBuilder {
            config,
            instance_override,
            auth_override,
            schema_release,
            schema_overlay_path,
            max_retries,
            timeout,
            rate_limit,
        } = self;

        // Resolve instance URL: builder override > config.
        let instance_raw = instance_override
            .or(config.instance.url.clone())
            .ok_or_else(|| {
                Error::Config(
                    "instance URL not configured. Set it via .instance(), \
                     SERVICENOW_INSTANCE env var, or servicenow.toml"
                        .into(),
                )
            })?;

        let base_url_str = config::normalize_instance_url(&instance_raw)?;
        let base_url = Url::parse(&base_url_str)?;

        // Resolve authenticator.
        let authenticator: Arc<dyn Authenticator> = if let Some(auth) = auth_override {
            Arc::from(auth)
        } else {
            resolve_auth_from_config(&config)?
        };

        // Resolve transport config.
        let timeout = timeout
            .or(config.transport.timeout_secs.map(Duration::from_secs))
            .unwrap_or(Duration::from_secs(30));

        let retry_config = RetryConfig {
            max_retries: max_retries
                .or(config.transport.max_retries)
                .unwrap_or(3),
            ..RetryConfig::default()
        };

        let rate_limiter = rate_limit
            .or(config.transport.rate_limit)
            .map(RateLimiter::new);

        let transport = Arc::new(HttpTransport::new(
            base_url,
            authenticator,
            timeout,
            retry_config,
            rate_limiter,
        )?);

        // Resolve schema.
        let schema = resolve_schema(
            schema_release.as_deref(),
            schema_overlay_path.as_deref(),
            &config,
        )?;

        Ok(ServiceNowClient {
            transport,
            schema: schema.map(Arc::new),
        })
    }

}

/// Resolve authentication from config when no explicit auth override is set.
fn resolve_auth_from_config(config: &Config) -> Result<Arc<dyn Authenticator>> {
    let method = config.auth.method.as_deref().unwrap_or("basic");

    match method {
        "basic" => {
            let username = config.auth.username.as_deref().ok_or_else(|| {
                Error::Config(
                    "basic auth requires username. Set SERVICENOW_USERNAME or \
                     configure auth.username in servicenow.toml"
                        .into(),
                )
            })?;
            let password = config.auth.password.as_deref().ok_or_else(|| {
                Error::Config(
                    "basic auth requires password. Set SERVICENOW_PASSWORD or \
                     configure auth.password in servicenow.toml"
                        .into(),
                )
            })?;
            Ok(Arc::new(BasicAuth::new(username, password)))
        }
        other => Err(Error::Config(format!(
            "unsupported auth method '{}'. Available: basic",
            other
        ))),
    }
}

/// Resolve schema from configuration.
fn resolve_schema(
    schema_release: Option<&str>,
    schema_overlay_path: Option<&Path>,
    config: &Config,
) -> Result<Option<SchemaRegistry>> {
    let release = schema_release.or(config.schema.release.as_deref());

    let overlay_path = schema_overlay_path
        .or(config.schema.overlay.as_deref().map(Path::new));

    match (release, overlay_path) {
        (Some(release), Some(overlay)) => Ok(Some(
            SchemaRegistry::from_release_with_overlay(release, overlay)?,
        )),
        (Some(release), None) => Ok(Some(SchemaRegistry::from_release(release)?)),
        (None, _) => Ok(None),
    }
}

// Implement Debug manually because Box<dyn Authenticator> is in the builder.
impl std::fmt::Debug for ClientBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientBuilder")
            .field("instance_override", &self.instance_override)
            .field("has_auth_override", &self.auth_override.is_some())
            .field("schema_release", &self.schema_release)
            .field("schema_overlay_path", &self.schema_overlay_path)
            .field("max_retries", &self.max_retries)
            .field("timeout", &self.timeout)
            .field("rate_limit", &self.rate_limit)
            .finish()
    }
}
