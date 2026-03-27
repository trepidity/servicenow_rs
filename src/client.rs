use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use url::Url;

use crate::api::aggregate::AggregateApi;
use crate::auth::basic::BasicAuth;
use crate::auth::token::TokenAuth;
use crate::auth::Authenticator;
use crate::config::{self, Config};
use crate::error::{Error, Result};
use crate::model::record::Record;
use crate::model::value::DisplayValue;
use crate::prefix::PrefixRegistry;
use crate::query::builder::TableApi;
use crate::query::filter::Order;
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
    prefix_registry: PrefixRegistry,
    base_url: String,
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

    /// Start building an aggregate/stats query on a table.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example() -> servicenow_rs::error::Result<()> {
    /// # let client: servicenow_rs::client::ServiceNowClient = todo!();
    /// let stats = client.aggregate("incident")
    ///     .count()
    ///     .group_by("state")
    ///     .execute()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn aggregate(&self, table: &str) -> AggregateApi {
        AggregateApi::new(Arc::clone(&self.transport), table)
    }

    /// Get a reference to the schema registry, if loaded.
    pub fn schema(&self) -> Option<&SchemaRegistry> {
        self.schema.as_deref()
    }

    /// Get a reference to the prefix registry.
    pub fn prefix_registry(&self) -> &PrefixRegistry {
        &self.prefix_registry
    }

    /// Get the base instance URL.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    // ── Record Number Resolution ────────────────────────────────────

    /// Resolve a record number prefix to a table name.
    ///
    /// ```
    /// # fn example(client: &servicenow_rs::client::ServiceNowClient) {
    /// assert_eq!(client.table_for_prefix("INC"), Some("incident"));
    /// assert_eq!(client.table_for_prefix("CHG"), Some("change_request"));
    /// # }
    /// ```
    pub fn table_for_prefix(&self, prefix: &str) -> Option<&str> {
        self.prefix_registry.table_for_prefix(prefix)
    }

    /// Extract the prefix from a record number and resolve the table name.
    ///
    /// ```
    /// # fn example(client: &servicenow_rs::client::ServiceNowClient) {
    /// assert_eq!(client.table_for_number("INC0012345"), Some("incident"));
    /// assert_eq!(client.table_for_number("CHG0307336"), Some("change_request"));
    /// # }
    /// ```
    pub fn table_for_number(&self, number: &str) -> Option<&str> {
        self.prefix_registry.table_for_number(number)
    }

    /// Fetch a record by its number (e.g., "INC0012345").
    ///
    /// Resolves the table from the prefix and queries by number.
    ///
    /// ```no_run
    /// # async fn example() -> servicenow_rs::error::Result<()> {
    /// # let client: servicenow_rs::client::ServiceNowClient = todo!();
    /// let record = client.get_by_number("INC0012345").await?;
    /// if let Some(record) = record {
    ///     println!("{}: {}", record.get_str("number").unwrap_or("?"),
    ///         record.get_str("short_description").unwrap_or("?"));
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_by_number(&self, number: &str) -> Result<Option<Record>> {
        let table = self
            .prefix_registry
            .table_for_number(number)
            .ok_or_else(|| {
                Error::Query(format!(
                    "cannot resolve table for number '{}' — unknown prefix. \
                     Register it with .register_prefix() on the builder.",
                    number
                ))
            })?;

        self.table(table).equals("number", number).first().await
    }

    // ── Journal Entry Reading ───────────────────────────────────────

    /// Read journal entries (work_notes, comments) for a record.
    ///
    /// Returns a `TableApi` pre-configured to query `sys_journal_field`
    /// filtered by the record's sys_id and the specified field name.
    /// Chain `.order_by()`, `.limit()`, `.display_value()`, etc.
    ///
    /// # Permissions
    ///
    /// This method queries the `sys_journal_field` table, which is
    /// ACL-restricted on many ServiceNow instances. Non-admin users may
    /// get **empty results with no error** even when journal entries exist.
    /// If you encounter this, use [`journal_inline`](Self::journal_inline)
    /// instead, which reads journal fields directly from the record table
    /// and works regardless of `sys_journal_field` ACL configuration.
    ///
    /// ```no_run
    /// # async fn example() -> servicenow_rs::error::Result<()> {
    /// # let client: servicenow_rs::client::ServiceNowClient = todo!();
    /// use servicenow_rs::query::Order;
    ///
    /// // Read private work notes.
    /// let notes = client.journal("incident", "abc123sys_id", "work_notes")
    ///     .order_by("sys_created_on", Order::Desc)
    ///     .limit(50)
    ///     .execute()
    ///     .await?;
    ///
    /// for entry in &notes {
    ///     println!("{} by {}: {}",
    ///         entry.get_str("sys_created_on").unwrap_or("?"),
    ///         entry.get_str("sys_created_by").unwrap_or("?"),
    ///         entry.get_str("value").unwrap_or(""),
    ///     );
    /// }
    ///
    /// // Read public comments.
    /// let comments = client.journal("incident", "abc123sys_id", "comments")
    ///     .limit(20)
    ///     .execute()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn journal(&self, table: &str, sys_id: &str, field: &str) -> TableApi {
        self.table("sys_journal_field")
            .equals("element_id", sys_id)
            .equals("element", field)
            .equals("name", table)
            .fields(&[
                "sys_created_on",
                "sys_created_by",
                "value",
                "element",
                "element_id",
            ])
            .order_by("sys_created_on", Order::Desc)
    }

    /// Read all journal entries (both work_notes and comments) for a record.
    ///
    /// Returns entries from both public comments and private work notes,
    /// sorted by creation time (newest first). Use the `element` field
    /// on each entry to distinguish: `"work_notes"` = private, `"comments"` = public.
    ///
    /// # Permissions
    ///
    /// This method queries the `sys_journal_field` table, which is
    /// ACL-restricted on many ServiceNow instances. Non-admin users may
    /// get **empty results with no error** even when journal entries exist.
    /// If you encounter this, use [`journal_inline`](Self::journal_inline)
    /// instead, which reads journal fields directly from the record table.
    pub fn journal_all(&self, table: &str, sys_id: &str) -> TableApi {
        self.table("sys_journal_field")
            .equals("element_id", sys_id)
            .equals("name", table)
            .fields(&[
                "sys_created_on",
                "sys_created_by",
                "value",
                "element",
                "element_id",
            ])
            .order_by("sys_created_on", Order::Desc)
    }

    /// Read journal content directly from the record's own table.
    ///
    /// Unlike [`journal`](Self::journal) and [`journal_all`](Self::journal_all),
    /// this method does **not** query `sys_journal_field`. Instead, it fetches the
    /// specified journal fields (e.g. `work_notes`, `comments`) as columns on the
    /// record itself, using `display_value=true` so ServiceNow returns the full
    /// formatted journal text with timestamps and author names.
    ///
    /// This approach works regardless of `sys_journal_field` ACL restrictions and
    /// is the recommended method when the per-entry breakdown is not required.
    ///
    /// # Trade-offs
    ///
    /// - Returns the **full concatenated journal text** per field, not individual
    ///   entries. You get one string per field rather than separate records per entry.
    /// - Always uses [`DisplayValue::Display`] — raw journal values are opaque
    ///   internal formats and not useful for reading.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example() -> servicenow_rs::error::Result<()> {
    /// # let client: servicenow_rs::client::ServiceNowClient = todo!();
    /// // Read work_notes and comments for a single incident.
    /// let record = client
    ///     .journal_inline("incident", "abc123sys_id", &["work_notes", "comments"])
    ///     .first()
    ///     .await?
    ///     .expect("record not found");
    ///
    /// if let Some(notes) = record.get_str("work_notes") {
    ///     println!("Work notes:\n{notes}");
    /// }
    /// if let Some(comments) = record.get_str("comments") {
    ///     println!("Comments:\n{comments}");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn journal_inline(&self, table: &str, sys_id: &str, fields: &[&str]) -> TableApi {
        self.table(table)
            .equals("sys_id", sys_id)
            .fields(fields)
            .display_value(DisplayValue::Display)
    }

    // ── Browser URL Construction ────────────────────────────────────

    /// Generate a URL that opens a record in the ServiceNow browser UI by number.
    ///
    /// ```
    /// # fn example(client: &servicenow_rs::client::ServiceNowClient) {
    /// let url = client.browser_url("incident", "INC0012345");
    /// // "https://instance.service-now.com/nav_to.do?uri=incident.do?sysparm_query=number=INC0012345"
    /// # }
    /// ```
    pub fn browser_url(&self, table: &str, number: &str) -> String {
        format!(
            "{}/nav_to.do?uri={}.do?sysparm_query=number={}",
            self.base_url, table, number
        )
    }

    /// Generate a URL that opens a record in the ServiceNow browser UI by sys_id.
    ///
    /// ```
    /// # fn example(client: &servicenow_rs::client::ServiceNowClient) {
    /// let url = client.browser_url_by_id("incident", "abc123def456");
    /// // "https://instance.service-now.com/nav_to.do?uri=incident.do?sys_id=abc123def456"
    /// # }
    /// ```
    pub fn browser_url_by_id(&self, table: &str, sys_id: &str) -> String {
        format!(
            "{}/nav_to.do?uri={}.do?sys_id={}",
            self.base_url, table, sys_id
        )
    }

    /// Generate a browser URL from a record number, resolving the table from the prefix.
    ///
    /// Returns `None` if the prefix cannot be resolved.
    ///
    /// ```
    /// # fn example(client: &servicenow_rs::client::ServiceNowClient) {
    /// let url = client.browser_url_for_number("INC0012345");
    /// // Some("https://instance.service-now.com/nav_to.do?uri=incident.do?sysparm_query=number=INC0012345")
    /// # }
    /// ```
    pub fn browser_url_for_number(&self, number: &str) -> Option<String> {
        let table = self.prefix_registry.table_for_number(number)?;
        Some(self.browser_url(table, number))
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
    allow_http: bool,
    prefix_registry: Option<PrefixRegistry>,
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

    /// Allow HTTP (non-TLS) connections. **For testing only.**
    ///
    /// By default, the library enforces HTTPS. Call this to allow
    /// `http://` URLs, e.g., for wiremock or local test servers.
    pub fn allow_http(mut self) -> Self {
        self.allow_http = true;
        self
    }

    /// Register a custom prefix -> table name mapping for record number resolution.
    ///
    /// The default mappings (INC -> incident, CHG -> change_request, etc.) are
    /// always included. This adds or overrides additional mappings.
    pub fn register_prefix(mut self, prefix: &str, table: &str) -> Self {
        let reg = self
            .prefix_registry
            .get_or_insert_with(PrefixRegistry::default);
        reg.register(prefix, table);
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
            allow_http,
            prefix_registry,
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

        // Enforce HTTPS unless explicitly allowed for testing.
        if base_url.scheme() == "http" && !allow_http {
            return Err(Error::Config(
                "HTTP URLs are not allowed. Use HTTPS for all ServiceNow connections. \
                 Call .allow_http() on the builder for local testing."
                    .into(),
            ));
        }

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
            prefix_registry: prefix_registry.unwrap_or_default(),
            base_url: base_url_str,
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
        "token" | "bearer" => {
            let token = config.auth.token.as_deref().ok_or_else(|| {
                Error::Config(
                    "token auth requires a token. Set SERVICENOW_API_TOKEN or \
                     configure auth.token in servicenow.toml"
                        .into(),
                )
            })?;
            Ok(Arc::new(TokenAuth::bearer(token)))
        }
        other => Err(Error::Config(format!(
            "unsupported auth method '{}'. Available: basic, token",
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
