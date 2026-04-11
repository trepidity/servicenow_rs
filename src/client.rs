use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use url::Url;

use crate::api::aggregate::AggregateApi;
use crate::api::approval::{ApprovalAction, ApprovalBuilder};
use crate::auth::basic::BasicAuth;
use crate::auth::token::TokenAuth;
use crate::auth::Authenticator;
use crate::config::{self, Config};
use crate::error::{Error, Result};
use crate::model::record::Record;
use crate::model::value::DisplayValue;
use crate::prefix::PrefixRegistry;
use crate::query::builder::{validate_identifier, TableApi};
use crate::query::filter::Order;
use crate::schema::registry::SchemaRegistry;
use crate::transport::retry::{RateLimiter, RetryConfig};
use crate::transport::{
    GraphqlTransport, HttpTransport, Transport, TransportMode, TransportSelection,
};

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
    transport: Arc<dyn Transport>,
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
        Self::builder()
            .from_default_config()
            .from_env()
            .build()
            .await
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

    /// Get the configured transport selection policy.
    pub fn transport_selection(&self) -> TransportSelection {
        self.transport.selection()
    }

    /// Send a raw POST request to any ServiceNow API endpoint.
    ///
    /// Use this for APIs not covered by the Table API (e.g. Service Catalog,
    /// Import Sets, Scripted REST APIs). The `path` should start with `/`
    /// (e.g. `"/api/sn_sc/servicecatalog/items/{id}/order_now"`).
    ///
    /// Returns the raw JSON `result` field from the response.
    pub async fn post(&self, path: &str, body: serde_json::Value) -> Result<serde_json::Value> {
        let response = self.transport.post(path, body).await?;
        Ok(response.result)
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

    /// Fetch related rows for multiple parent records in one query by foreign key.
    ///
    /// This is useful when the caller already knows the child table and link field,
    /// and wants to avoid one HTTP request per parent.
    pub async fn fetch_related_by_foreign_key(
        &self,
        table: &str,
        foreign_key: &str,
        parent_ids: &[&str],
        fields: &[&str],
        display_value: DisplayValue,
        order_by: Option<(&str, Order)>,
    ) -> Result<HashMap<String, Vec<Record>>> {
        if parent_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut query = self
            .table(table)
            .in_list(foreign_key, parent_ids)
            .fields(fields)
            .display_value(display_value)
            .exclude_reference_link(true);
        if let Some((field, direction)) = order_by {
            query = query.order_by(field, direction);
        }

        let response = query.execute().await?;
        let mut by_parent: HashMap<String, Vec<Record>> = parent_ids
            .iter()
            .map(|id| ((*id).to_string(), Vec::new()))
            .collect();

        for record in response.records {
            let Some(parent_id) = record
                .get_raw(foreign_key)
                .or_else(|| record.get_str(foreign_key))
                .map(ToString::to_string)
            else {
                continue;
            };
            by_parent.entry(parent_id).or_default().push(record);
        }

        Ok(by_parent)
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

    // ── Catalog Helpers ────────────────────────────────────────────

    /// Fetch catalog variables for a requested item (RITM).
    ///
    /// Catalog variables are the form fields users fill out when submitting a
    /// service catalog request. They are stored in `sc_item_option` and linked
    /// via `sc_item_option_mtom`.
    ///
    /// Returns variables sorted by form display order. Variables with no label
    /// are excluded.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example() -> servicenow_rs::error::Result<()> {
    /// # let client: servicenow_rs::client::ServiceNowClient = todo!();
    /// let vars = client.catalog_variables("ritm_sys_id_here").await?;
    /// for var in &vars {
    ///     println!("{}: {}", var.name, var.value);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn catalog_variables(
        &self,
        ritm_sys_id: &str,
    ) -> Result<Vec<crate::api::catalog::CatalogVariable>> {
        crate::api::catalog::fetch_catalog_variables(Arc::clone(&self.transport), ritm_sys_id).await
    }

    /// Resolve reference and list collector values in catalog variables to
    /// human-readable display names.
    ///
    /// Variables whose [`reference_table`](crate::api::catalog::CatalogVariable::reference_table)
    /// is set have values that are sys_ids (or comma-separated sys_ids for list
    /// collectors). This method batch-queries each referenced table and replaces
    /// the sys_ids with the record `name` field.
    ///
    /// If a table query fails (e.g. ACL restrictions), those variables keep
    /// their original sys_id values.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example() -> servicenow_rs::error::Result<()> {
    /// # let client: servicenow_rs::client::ServiceNowClient = todo!();
    /// let mut vars = client.catalog_variables("ritm_sys_id_here").await?;
    /// client.resolve_catalog_variables(&mut vars).await?;
    /// for var in &vars {
    ///     println!("{}: {}", var.name, var.value);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn resolve_catalog_variables(
        &self,
        variables: &mut [crate::api::catalog::CatalogVariable],
    ) -> Result<()> {
        use std::collections::HashMap;

        // Collect all sys_ids grouped by their reference table
        let mut table_ids: HashMap<String, Vec<String>> = HashMap::new();
        for var in variables.iter() {
            if let Some(ref table) = var.reference_table {
                for id in var.value.split(',') {
                    let id = id.trim();
                    if is_sys_id(id) {
                        table_ids
                            .entry(table.clone())
                            .or_default()
                            .push(id.to_string());
                    }
                }
            }
        }

        if table_ids.is_empty() {
            return Ok(());
        }

        // Batch-query each reference table and build a sys_id → name map
        let mut name_map: HashMap<String, String> = HashMap::new();

        for (table, ids) in &table_ids {
            let mut unique_ids: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
            unique_ids.sort_unstable();
            unique_ids.dedup();

            // If the query fails (ACL, table doesn't exist), skip silently
            if let Ok(result) = self
                .table(table)
                .in_list("sys_id", &unique_ids)
                .fields(&["sys_id", "name"])
                .display_value(DisplayValue::Display)
                .execute()
                .await
            {
                for record in &result.records {
                    if let Some(name) = record.get_str("name") {
                        if !name.is_empty() {
                            name_map.insert(record.sys_id.clone(), name.to_string());
                        }
                    }
                }
            }
        }

        if name_map.is_empty() {
            return Ok(());
        }

        // Replace sys_ids with resolved names in variable values
        for var in variables.iter_mut() {
            if var.reference_table.is_some() {
                let parts: Vec<&str> = var.value.split(',').collect();
                if parts.iter().any(|id| name_map.contains_key(id.trim())) {
                    let resolved: Vec<String> = parts
                        .iter()
                        .map(|id| {
                            let id = id.trim();
                            name_map.get(id).cloned().unwrap_or_else(|| id.to_string())
                        })
                        .collect();
                    var.value = resolved.join(", ");
                }
            }
        }

        Ok(())
    }

    // ── Record Update Helpers ──────────────────────────────────────

    /// Append a work note to a record.
    ///
    /// This is a convenience wrapper around [`TableApi::update`] that sends
    /// only the `work_notes` field. The note is appended as a new journal
    /// entry — it does not overwrite existing notes.
    ///
    /// Returns the updated record with the fields specified in `return_fields`.
    /// If `return_fields` is empty, ServiceNow returns all fields.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example() -> servicenow_rs::error::Result<()> {
    /// # let client: servicenow_rs::client::ServiceNowClient = todo!();
    /// let record = client
    ///     .add_work_note("incident", "sys_id_here", "Escalated to network team")
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn add_work_note(&self, table: &str, sys_id: &str, note: &str) -> Result<Record> {
        self.table(table)
            .fields(&["sys_id", "number", "state"])
            .display_value(DisplayValue::Both)
            .update(sys_id, serde_json::json!({ "work_notes": note }))
            .await
    }

    /// Change the state of a record.
    ///
    /// The `state` value should be the raw numeric string as ServiceNow
    /// expects it (e.g. `"1"` for New, `"2"` for Work In Progress).
    /// Use [`DisplayValue::Both`] on a query to discover the mapping
    /// between raw values and display labels for a given table.
    ///
    /// Optionally appends a work note explaining the state change.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example() -> servicenow_rs::error::Result<()> {
    /// # let client: servicenow_rs::client::ServiceNowClient = todo!();
    /// // Change to "Work In Progress" with a note.
    /// let record = client
    ///     .set_state("rm_scrum_task", "sys_id_here", "2", Some("Starting work"))
    ///     .await?;
    ///
    /// println!("New state: {}", record.get_display("state").unwrap_or("?"));
    /// # Ok(())
    /// # }
    /// ```
    pub async fn set_state(
        &self,
        table: &str,
        sys_id: &str,
        state: &str,
        work_note: Option<&str>,
    ) -> Result<Record> {
        let mut body = serde_json::json!({ "state": state });
        if let Some(note) = work_note {
            body["work_notes"] = serde_json::json!(note);
        }
        self.table(table)
            .fields(&["sys_id", "number", "state"])
            .display_value(DisplayValue::Both)
            .update(sys_id, body)
            .await
    }

    // ── Approval Operations ─────────────────────────────────────────

    /// Approve a pending approval for a record.
    ///
    /// Finds the approval in `sysapproval_approver` matching the record
    /// and approver, then sets its state to "approved".
    ///
    /// # Arguments
    ///
    /// * `source_table` — The table the approval is for (e.g. `"change_request"`)
    /// * `record_sys_id` — The sys_id of the record being approved
    /// * `approver_sys_id` — The sys_id of the approving user (must match the
    ///   assigned approver)
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example() -> servicenow_rs::error::Result<()> {
    /// # let client: servicenow_rs::client::ServiceNowClient = todo!();
    /// let approval = client
    ///     .approve("change_request", "chg_sys_id", "user_sys_id")
    ///     .comment("Approved via API")
    ///     .execute()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn approve(
        &self,
        source_table: &str,
        record_sys_id: &str,
        approver_sys_id: &str,
    ) -> ApprovalBuilder {
        ApprovalBuilder::new(
            Arc::clone(&self.transport),
            source_table,
            record_sys_id,
            approver_sys_id,
            ApprovalAction::Approve,
        )
    }

    /// Reject a pending approval for a record.
    ///
    /// Finds the approval in `sysapproval_approver` matching the record
    /// and approver, then sets its state to "rejected".
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example() -> servicenow_rs::error::Result<()> {
    /// # let client: servicenow_rs::client::ServiceNowClient = todo!();
    /// let rejection = client
    ///     .reject("change_request", "chg_sys_id", "user_sys_id")
    ///     .comment("Missing test plan")
    ///     .execute()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn reject(
        &self,
        source_table: &str,
        record_sys_id: &str,
        approver_sys_id: &str,
    ) -> ApprovalBuilder {
        ApprovalBuilder::new(
            Arc::clone(&self.transport),
            source_table,
            record_sys_id,
            approver_sys_id,
            ApprovalAction::Reject,
        )
    }

    // ── Browser URL Construction ────────────────────────────────────

    /// Generate a URL that opens a record in the ServiceNow browser UI by number.
    ///
    /// Returns an error if `table` or `number` contain invalid characters.
    ///
    /// ```
    /// # fn example(client: &servicenow_rs::client::ServiceNowClient) {
    /// let url = client.browser_url("incident", "INC0012345").unwrap();
    /// // "https://instance.service-now.com/nav_to.do?uri=incident.do?sysparm_query=number=INC0012345"
    /// # }
    /// ```
    pub fn browser_url(&self, table: &str, number: &str) -> Result<String> {
        validate_identifier(table, "table name")?;
        validate_identifier(number, "record number")?;
        Ok(format!(
            "{}/nav_to.do?uri={}.do?sysparm_query=number={}",
            self.base_url, table, number
        ))
    }

    /// Generate a URL that opens a record in the ServiceNow browser UI by sys_id.
    ///
    /// Returns an error if `table` or `sys_id` contain invalid characters.
    ///
    /// ```
    /// # fn example(client: &servicenow_rs::client::ServiceNowClient) {
    /// let url = client.browser_url_by_id("incident", "abc123def456").unwrap();
    /// // "https://instance.service-now.com/nav_to.do?uri=incident.do?sys_id=abc123def456"
    /// # }
    /// ```
    pub fn browser_url_by_id(&self, table: &str, sys_id: &str) -> Result<String> {
        validate_identifier(table, "table name")?;
        validate_identifier(sys_id, "sys_id")?;
        Ok(format!(
            "{}/nav_to.do?uri={}.do?sys_id={}",
            self.base_url, table, sys_id
        ))
    }

    /// Generate a browser URL from a record number, resolving the table from the prefix.
    ///
    /// Returns `None` if the prefix cannot be resolved, or an error if inputs
    /// contain invalid characters.
    ///
    /// ```
    /// # fn example(client: &servicenow_rs::client::ServiceNowClient) {
    /// let url = client.browser_url_for_number("INC0012345");
    /// // Some(Ok("https://instance.service-now.com/nav_to.do?uri=incident.do?sysparm_query=number=INC0012345"))
    /// # }
    /// ```
    pub fn browser_url_for_number(&self, number: &str) -> Option<Result<String>> {
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

    /// Override the preferred transport mode.
    pub fn transport_mode(mut self, mode: TransportMode) -> Self {
        self.config.transport.preferred = mode;
        self
    }

    /// Control whether GraphQL transport falls back to REST on unsupported or failed reads.
    pub fn graphql_fallback(mut self, fallback: bool) -> Self {
        self.config.transport.graphql_fallback = fallback;
        self
    }

    /// Set the minimum field-count threshold for GraphQL table-list routing.
    pub fn graphql_batch_threshold(mut self, threshold: usize) -> Self {
        self.config.transport.graphql_batch_threshold = threshold;
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
            max_retries: max_retries.or(config.transport.max_retries).unwrap_or(3),
            ..RetryConfig::default()
        };

        let rate_limiter = rate_limit
            .or(config.transport.rate_limit)
            .map(RateLimiter::new);

        let transport_selection = TransportSelection::new(
            config.transport.preferred,
            config.transport.graphql_fallback,
            config.transport.graphql_batch_threshold,
        );

        let http_transport = HttpTransport::new(
            base_url,
            authenticator,
            timeout,
            retry_config,
            rate_limiter,
            transport_selection,
        )?;

        let transport: Arc<dyn Transport> = match transport_selection.preferred {
            TransportMode::Graphql => Arc::new(GraphqlTransport::new(http_transport)),
            TransportMode::Auto | TransportMode::Rest => Arc::new(http_transport),
        };

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

    let overlay_path = schema_overlay_path.or(config.schema.overlay.as_deref().map(Path::new));

    match (release, overlay_path) {
        (Some(release), Some(overlay)) => Ok(Some(SchemaRegistry::from_release_with_overlay(
            release, overlay,
        )?)),
        (Some(release), None) => Ok(Some(SchemaRegistry::from_release(release)?)),
        (None, _) => Ok(None),
    }
}

/// Check if a string looks like a ServiceNow sys_id (32-character hex string).
fn is_sys_id(s: &str) -> bool {
    s.len() == 32 && s.chars().all(|c| c.is_ascii_hexdigit())
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
