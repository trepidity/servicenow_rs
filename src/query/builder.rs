use std::sync::Arc;

use serde_json::Value;
use tracing::debug;

use crate::error::{Error, Result};
use crate::model::record::Record;
use crate::model::result::QueryResult;
use crate::model::value::DisplayValue;
use crate::schema::registry::SchemaRegistry;
use crate::transport::http::HttpTransport;

use super::batch;
use super::filter::{Condition, Filter, Joiner, Operator, Order, encode_query};
use super::strategy::FetchStrategy;

/// Builder for constructing and executing ServiceNow Table API queries.
///
/// Created via `ServiceNowClient::table("table_name")`.
///
/// # Examples
///
/// ```no_run
/// # async fn example() -> servicenow_rs::error::Result<()> {
/// # let client: servicenow_rs::client::ServiceNowClient = todo!();
/// use servicenow_rs::query::filter::Operator;
/// use servicenow_rs::model::value::DisplayValue;
///
/// // Query with filters
/// let results = client.table("change_request")
///     .filter("state", Operator::Equals, "1")
///     .fields(&["number", "short_description"])
///     .display_value(DisplayValue::Both)
///     .limit(10)
///     .execute()
///     .await?;
///
/// // Get a single record
/// let record = client.table("change_request")
///     .get("some_sys_id")
///     .await?;
/// # Ok(())
/// # }
/// ```
pub struct TableApi {
    transport: Arc<HttpTransport>,
    schema: Option<Arc<SchemaRegistry>>,
    table: String,
    conditions: Vec<Condition>,
    fields: Option<Vec<String>>,
    related: Vec<String>,
    display_value: DisplayValue,
    limit: Option<u32>,
    offset: Option<u32>,
    order_by: Vec<(String, Order)>,
    strategy: FetchStrategy,
    exclude_reference_link: bool,
    no_count: bool,
}

impl TableApi {
    /// Create a new TableApi. Typically called via `ServiceNowClient::table()`.
    pub(crate) fn new(
        transport: Arc<HttpTransport>,
        schema: Option<Arc<SchemaRegistry>>,
        table: impl Into<String>,
    ) -> Self {
        Self {
            transport,
            schema,
            table: table.into(),
            conditions: Vec::new(),
            fields: None,
            related: Vec::new(),
            display_value: DisplayValue::default(),
            limit: None,
            offset: None,
            order_by: Vec::new(),
            strategy: FetchStrategy::default(),
            exclude_reference_link: true,
            no_count: false,
        }
    }

    // ── Filter methods ──────────────────────────────────────────────

    /// Add a filter condition (AND).
    pub fn filter(mut self, field: &str, op: Operator, value: &str) -> Self {
        self.conditions.push(Condition {
            joiner: Joiner::And,
            filter: Filter {
                field: field.to_string(),
                operator: op,
                value: value.to_string(),
            },
        });
        self
    }

    /// Add an OR filter condition.
    pub fn or_filter(mut self, field: &str, op: Operator, value: &str) -> Self {
        self.conditions.push(Condition {
            joiner: Joiner::Or,
            filter: Filter {
                field: field.to_string(),
                operator: op,
                value: value.to_string(),
            },
        });
        self
    }

    /// Shorthand: field equals value.
    pub fn equals(self, field: &str, value: &str) -> Self {
        self.filter(field, Operator::Equals, value)
    }

    /// Shorthand: field not equals value.
    pub fn not_equals(self, field: &str, value: &str) -> Self {
        self.filter(field, Operator::NotEquals, value)
    }

    /// Shorthand: field contains value (fuzzy/LIKE).
    pub fn contains(self, field: &str, value: &str) -> Self {
        self.filter(field, Operator::Contains, value)
    }

    /// Shorthand: field starts with value.
    pub fn starts_with(self, field: &str, value: &str) -> Self {
        self.filter(field, Operator::StartsWith, value)
    }

    /// Shorthand: field ends with value.
    pub fn ends_with(self, field: &str, value: &str) -> Self {
        self.filter(field, Operator::EndsWith, value)
    }

    /// Shorthand: field is empty.
    pub fn is_empty_field(self, field: &str) -> Self {
        self.filter(field, Operator::IsEmpty, "")
    }

    /// Shorthand: field is not empty.
    pub fn is_not_empty(self, field: &str) -> Self {
        self.filter(field, Operator::IsNotEmpty, "")
    }

    /// Shorthand: field value is in the given list.
    pub fn in_list(self, field: &str, values: &[&str]) -> Self {
        self.filter(field, Operator::In, &values.join(","))
    }

    /// Shorthand: field greater than value.
    pub fn greater_than(self, field: &str, value: &str) -> Self {
        self.filter(field, Operator::GreaterThan, value)
    }

    /// Shorthand: field less than value.
    pub fn less_than(self, field: &str, value: &str) -> Self {
        self.filter(field, Operator::LessThan, value)
    }

    // ── Configuration methods ───────────────────────────────────────

    /// Specify which fields to return. If not called, all fields are returned.
    pub fn fields(mut self, fields: &[&str]) -> Self {
        self.fields = Some(fields.iter().map(|s| s.to_string()).collect());
        self
    }

    /// Include related records by relationship name.
    ///
    /// Requires a schema to be loaded so the library knows how to
    /// traverse the relationship.
    pub fn include_related(mut self, relations: &[&str]) -> Self {
        self.related
            .extend(relations.iter().map(|s| s.to_string()));
        self
    }

    /// Set the display value mode.
    pub fn display_value(mut self, mode: DisplayValue) -> Self {
        self.display_value = mode;
        self
    }

    /// Set the maximum number of records to return.
    pub fn limit(mut self, n: u32) -> Self {
        self.limit = Some(n);
        self
    }

    /// Set the pagination offset.
    pub fn offset(mut self, n: u32) -> Self {
        self.offset = Some(n);
        self
    }

    /// Add an order-by clause.
    pub fn order_by(mut self, field: &str, order: Order) -> Self {
        self.order_by.push((field.to_string(), order));
        self
    }

    /// Override the fetch strategy for related records.
    pub fn strategy(mut self, strategy: FetchStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Whether to exclude reference links from the response.
    pub fn exclude_reference_link(mut self, exclude: bool) -> Self {
        self.exclude_reference_link = exclude;
        self
    }

    /// Skip total count for better performance on large tables.
    pub fn no_count(mut self) -> Self {
        self.no_count = true;
        self
    }

    // ── Terminal operations: Read ───────────────────────────────────

    /// Execute the query and return all matching records.
    pub async fn execute(self) -> Result<QueryResult> {
        let path = format!("/api/now/table/{}", self.table);
        let params = self.build_params();

        debug!(
            table = self.table,
            params = ?params,
            "executing table query"
        );

        let response = self.transport.get(&path, &params).await?;

        // Parse records from the result array.
        let mut records: Vec<Record> = match response.result {
            Value::Array(arr) => arr
                .iter()
                .filter_map(|v| Record::from_json(&self.table, v, self.display_value))
                .collect(),
            _ => Vec::new(),
        };

        // Fetch related records if requested.
        let errors = self.fetch_related(&mut records).await;

        Ok(QueryResult {
            records,
            total_count: response.total_count,
            errors,
        })
    }

    /// Execute the query and return only the first matching record.
    pub async fn first(self) -> Result<Option<Record>> {
        let result = self.limit(1).execute().await?;
        Ok(result.records.into_iter().next())
    }

    /// Get the count of matching records without fetching them.
    pub async fn count(self) -> Result<u64> {
        let path = format!("/api/now/stats/{}", self.table);
        let mut params = Vec::new();

        // Build the query string from conditions.
        let query = encode_query(&self.conditions, &self.order_by);
        if !query.is_empty() {
            params.push(("sysparm_query".to_string(), query));
        }
        params.push(("sysparm_count".to_string(), "true".to_string()));

        let response = self.transport.get(&path, &params).await?;

        // The stats endpoint returns {"result": {"stats": {"count": "123"}}}
        let count = response
            .result
            .get("stats")
            .and_then(|s| s.get("count"))
            .and_then(|c| c.as_str())
            .and_then(|c| c.parse::<u64>().ok())
            .unwrap_or(0);

        Ok(count)
    }

    /// Get a single record by sys_id.
    pub async fn get(self, sys_id: &str) -> Result<Record> {
        let path = format!("/api/now/table/{}/{}", self.table, sys_id);
        let mut params = Vec::new();

        if let Some(ref fields) = self.fields {
            params.push(("sysparm_fields".to_string(), fields.join(",")));
        }
        params.push((
            "sysparm_display_value".to_string(),
            self.display_value.as_param().to_string(),
        ));
        if self.exclude_reference_link {
            params.push((
                "sysparm_exclude_reference_link".to_string(),
                "true".to_string(),
            ));
        }

        let response = self.transport.get(&path, &params).await?;

        let mut record = Record::from_json(&self.table, &response.result, self.display_value)
            .ok_or_else(|| Error::Api {
                status: 200,
                message: "failed to parse record from response".to_string(),
                detail: None,
            })?;

        // Fetch related records for the single record.
        let _errors = self.fetch_related(std::slice::from_mut(&mut record)).await;

        Ok(record)
    }

    // ── Terminal operations: Write ──────────────────────────────────

    /// Create a new record.
    pub async fn create(self, data: Value) -> Result<Record> {
        let path = format!("/api/now/table/{}", self.table);
        let mut params = Vec::new();
        params.push((
            "sysparm_display_value".to_string(),
            self.display_value.as_param().to_string(),
        ));
        if let Some(ref fields) = self.fields {
            params.push(("sysparm_fields".to_string(), fields.join(",")));
        }
        if self.exclude_reference_link {
            params.push((
                "sysparm_exclude_reference_link".to_string(),
                "true".to_string(),
            ));
        }

        // POST doesn't use query params for filtering, but does for display_value.
        // We need to add them as query params on the URL.
        let path_with_params = if params.is_empty() {
            path
        } else {
            let qs: Vec<String> = params.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
            format!("{}?{}", path, qs.join("&"))
        };

        let response = self.transport.post(&path_with_params, data).await?;

        Record::from_json(&self.table, &response.result, self.display_value).ok_or_else(|| {
            Error::Api {
                status: 201,
                message: "failed to parse created record from response".to_string(),
                detail: None,
            }
        })
    }

    /// Update an existing record by sys_id.
    pub async fn update(self, sys_id: &str, data: Value) -> Result<Record> {
        let path = format!("/api/now/table/{}/{}", self.table, sys_id);
        let response = self.transport.patch(&path, data).await?;

        Record::from_json(&self.table, &response.result, self.display_value).ok_or_else(|| {
            Error::Api {
                status: 200,
                message: "failed to parse updated record from response".to_string(),
                detail: None,
            }
        })
    }

    /// Delete a record by sys_id.
    pub async fn delete(self, sys_id: &str) -> Result<()> {
        let path = format!("/api/now/table/{}/{}", self.table, sys_id);
        self.transport.delete(&path).await?;
        Ok(())
    }

    // ── Internal helpers ────────────────────────────────────────────

    /// Build the query parameters for a GET request.
    fn build_params(&self) -> Vec<(String, String)> {
        let mut params = Vec::new();

        // Encoded query.
        let query = encode_query(&self.conditions, &self.order_by);
        if !query.is_empty() {
            params.push(("sysparm_query".to_string(), query));
        }

        // Field selection.
        if let Some(ref fields) = self.fields {
            params.push(("sysparm_fields".to_string(), fields.join(",")));
        }

        // Display value mode.
        params.push((
            "sysparm_display_value".to_string(),
            self.display_value.as_param().to_string(),
        ));

        // Pagination.
        if let Some(limit) = self.limit {
            params.push(("sysparm_limit".to_string(), limit.to_string()));
        }
        if let Some(offset) = self.offset {
            params.push(("sysparm_offset".to_string(), offset.to_string()));
        }

        // Reference links.
        if self.exclude_reference_link {
            params.push((
                "sysparm_exclude_reference_link".to_string(),
                "true".to_string(),
            ));
        }

        // Count suppression.
        if self.no_count {
            params.push(("sysparm_no_count".to_string(), "true".to_string()));
        }

        params
    }

    /// Fetch related records based on the `include_related` configuration.
    async fn fetch_related(&self, records: &mut [Record]) -> Vec<crate::error::Error> {
        if self.related.is_empty() || records.is_empty() {
            return Vec::new();
        }

        let schema = match &self.schema {
            Some(s) => s,
            None => {
                debug!("no schema loaded, skipping related record fetch");
                return vec![Error::Schema(
                    "cannot fetch related records without a schema. \
                     Load a schema definition to enable relationship traversal."
                        .to_string(),
                )];
            }
        };

        // Resolve requested relationship names to definitions.
        let mut rel_defs = Vec::new();
        let mut missing_errors = Vec::new();

        for rel_name in &self.related {
            match schema.relationship(&self.table, rel_name) {
                Some(rel_def) => {
                    rel_defs.push((rel_name.as_str(), rel_def));
                }
                None => {
                    missing_errors.push(Error::Schema(format!(
                        "relationship '{}' not found on table '{}'",
                        rel_name, self.table
                    )));
                }
            }
        }

        if rel_defs.is_empty() {
            return missing_errors;
        }

        // Use the configured strategy.
        let mut errors = match self.strategy {
            FetchStrategy::Concurrent | FetchStrategy::Auto => {
                batch::fetch_related_concurrent(
                    &self.transport,
                    &self.table,
                    records,
                    &rel_defs,
                    self.display_value,
                )
                .await
            }
            FetchStrategy::DotWalk => {
                // Dot-walking is handled at query time by adding dotted fields.
                // For now, fall back to concurrent.
                debug!("dot-walk strategy not yet implemented, falling back to concurrent");
                batch::fetch_related_concurrent(
                    &self.transport,
                    &self.table,
                    records,
                    &rel_defs,
                    self.display_value,
                )
                .await
            }
        };

        errors.extend(missing_errors);
        errors
    }
}

