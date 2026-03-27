use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;
use tracing::debug;

use crate::error::{Error, Result};
use crate::query::filter::{Condition, Filter, Joiner, Operator, Order, encode_query};
use crate::transport::http::HttpTransport;

/// Builder for constructing ServiceNow Aggregate/Stats API queries.
///
/// Created via `ServiceNowClient::aggregate("table_name")`.
///
/// # Examples
///
/// ```no_run
/// # async fn example() -> servicenow_rs::error::Result<()> {
/// # let client: servicenow_rs::client::ServiceNowClient = todo!();
/// // Simple count
/// let stats = client.aggregate("incident")
///     .count()
///     .execute()
///     .await?;
/// println!("Total incidents: {}", stats.count());
///
/// // Grouped count with filter
/// let stats = client.aggregate("incident")
///     .count()
///     .group_by("state")
///     .filter("active", servicenow_rs::query::Operator::Equals, "true")
///     .execute()
///     .await?;
/// for group in stats.groups() {
///     println!("state={}: {}", group.field_value("state"), group.count());
/// }
/// # Ok(())
/// # }
/// ```
pub struct AggregateApi {
    transport: Arc<HttpTransport>,
    table: String,
    conditions: Vec<Condition>,
    order_by: Vec<(String, Order)>,
    do_count: bool,
    avg_fields: Vec<String>,
    sum_fields: Vec<String>,
    min_fields: Vec<String>,
    max_fields: Vec<String>,
    group_by_fields: Vec<String>,
    having_count: Option<String>,
    display_value: bool,
}

impl AggregateApi {
    /// Create a new AggregateApi. Called via `ServiceNowClient::aggregate()`.
    pub(crate) fn new(transport: Arc<HttpTransport>, table: impl Into<String>) -> Self {
        Self {
            transport,
            table: table.into(),
            conditions: Vec::new(),
            order_by: Vec::new(),
            do_count: false,
            avg_fields: Vec::new(),
            sum_fields: Vec::new(),
            min_fields: Vec::new(),
            max_fields: Vec::new(),
            group_by_fields: Vec::new(),
            having_count: None,
            display_value: false,
        }
    }

    // ── Aggregate operations ────────────────────────────────────────

    /// Include a count in the result.
    pub fn count(mut self) -> Self {
        self.do_count = true;
        self
    }

    /// Calculate the average of a field.
    pub fn avg(mut self, field: &str) -> Self {
        self.avg_fields.push(field.to_string());
        self
    }

    /// Calculate the sum of a field.
    pub fn sum(mut self, field: &str) -> Self {
        self.sum_fields.push(field.to_string());
        self
    }

    /// Calculate the minimum value of a field.
    pub fn min(mut self, field: &str) -> Self {
        self.min_fields.push(field.to_string());
        self
    }

    /// Calculate the maximum value of a field.
    pub fn max(mut self, field: &str) -> Self {
        self.max_fields.push(field.to_string());
        self
    }

    /// Group results by a field.
    pub fn group_by(mut self, field: &str) -> Self {
        self.group_by_fields.push(field.to_string());
        self
    }

    /// Filter groups by count (HAVING clause).
    /// Example: `having_count(">10")` keeps only groups with count > 10.
    pub fn having_count(mut self, condition: &str) -> Self {
        self.having_count = Some(condition.to_string());
        self
    }

    /// Use display values in group-by fields.
    pub fn display_value(mut self, enabled: bool) -> Self {
        self.display_value = enabled;
        self
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

    /// Shorthand: field equals value.
    pub fn equals(self, field: &str, value: &str) -> Self {
        self.filter(field, Operator::Equals, value)
    }

    /// Add an order-by clause for group ordering.
    pub fn order_by(mut self, field: &str, order: Order) -> Self {
        self.order_by.push((field.to_string(), order));
        self
    }

    // ── Execute ─────────────────────────────────────────────────────

    /// Execute the aggregate query.
    pub async fn execute(self) -> Result<AggregateResult> {
        let path = format!("/api/now/stats/{}", self.table);
        let params = self.build_params();

        debug!(
            table = self.table,
            params = ?params,
            "executing aggregate query"
        );

        let response = self.transport.get(&path, &params).await?;

        parse_aggregate_result(response.result)
    }

    /// Build query parameters.
    fn build_params(&self) -> Vec<(String, String)> {
        let mut params = Vec::new();

        // Encoded query.
        let query = encode_query(&self.conditions, &self.order_by);
        if !query.is_empty() {
            params.push(("sysparm_query".to_string(), query));
        }

        // Count.
        if self.do_count {
            params.push(("sysparm_count".to_string(), "true".to_string()));
        }

        // Avg fields.
        if !self.avg_fields.is_empty() {
            params.push((
                "sysparm_avg_fields".to_string(),
                self.avg_fields.join(","),
            ));
        }

        // Sum fields.
        if !self.sum_fields.is_empty() {
            params.push((
                "sysparm_sum_fields".to_string(),
                self.sum_fields.join(","),
            ));
        }

        // Min fields.
        if !self.min_fields.is_empty() {
            params.push((
                "sysparm_min_fields".to_string(),
                self.min_fields.join(","),
            ));
        }

        // Max fields.
        if !self.max_fields.is_empty() {
            params.push((
                "sysparm_max_fields".to_string(),
                self.max_fields.join(","),
            ));
        }

        // Group by.
        if !self.group_by_fields.is_empty() {
            params.push((
                "sysparm_group_by".to_string(),
                self.group_by_fields.join(","),
            ));
        }

        // Having.
        if let Some(ref having) = self.having_count {
            params.push(("sysparm_having".to_string(), having.clone()));
        }

        // Display value for group-by fields.
        if self.display_value {
            params.push((
                "sysparm_display_value".to_string(),
                "true".to_string(),
            ));
        }

        params
    }
}

/// Result from an aggregate/stats query.
///
/// ServiceNow returns two shapes:
/// - Non-grouped: `{"result": {"stats": {"count": "123", ...}}}`
/// - Grouped: `{"result": [{"stats": {...}, "groupby_fields": [...]}]}`
#[derive(Debug, Clone)]
pub struct AggregateResult {
    /// For non-grouped queries, the single set of stats.
    /// For grouped queries, this holds the overall stats (if any).
    stats: HashMap<String, HashMap<String, String>>,
    /// For grouped queries, one entry per group.
    groups: Vec<AggregateGroup>,
}

/// A single group in a grouped aggregate result.
#[derive(Debug, Clone)]
pub struct AggregateGroup {
    /// Stats for this group (e.g., count, avg, sum, etc.).
    stats: HashMap<String, HashMap<String, String>>,
    /// Group-by field values.
    group_fields: HashMap<String, String>,
}

impl AggregateResult {
    /// Get the count (non-grouped).
    pub fn count(&self) -> u64 {
        self.stat_value("count")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    }

    /// Get a specific stat value by operation name (e.g., "count", "avg", "sum").
    pub fn stat_value(&self, stat: &str) -> Option<&str> {
        self.stats
            .get("stats")
            .and_then(|s| s.get(stat))
            .map(|s| s.as_str())
    }

    /// Get the average of a field (non-grouped).
    pub fn avg(&self, field: &str) -> Option<f64> {
        self.stat_value(&format!("avg.{}", field))
            .and_then(|s| s.parse().ok())
    }

    /// Get the sum of a field (non-grouped).
    pub fn sum(&self, field: &str) -> Option<f64> {
        self.stat_value(&format!("sum.{}", field))
            .and_then(|s| s.parse().ok())
    }

    /// Get the min of a field (non-grouped).
    pub fn min_val(&self, field: &str) -> Option<&str> {
        self.stat_value(&format!("min.{}", field))
    }

    /// Get the max of a field (non-grouped).
    pub fn max_val(&self, field: &str) -> Option<&str> {
        self.stat_value(&format!("max.{}", field))
    }

    /// Whether this is a grouped result.
    pub fn is_grouped(&self) -> bool {
        !self.groups.is_empty()
    }

    /// Get the groups (for grouped queries).
    pub fn groups(&self) -> &[AggregateGroup] {
        &self.groups
    }

    /// Number of groups.
    pub fn group_count(&self) -> usize {
        self.groups.len()
    }
}

impl AggregateGroup {
    /// Get the count for this group.
    pub fn count(&self) -> u64 {
        self.stat_value("count")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    }

    /// Get a stat value for this group.
    pub fn stat_value(&self, stat: &str) -> Option<&str> {
        self.stats
            .get("stats")
            .and_then(|s| s.get(stat))
            .map(|s| s.as_str())
    }

    /// Get a group-by field value.
    pub fn field_value(&self, field: &str) -> &str {
        self.group_fields
            .get(field)
            .map(|s| s.as_str())
            .unwrap_or("")
    }

    /// Get all group-by field values.
    pub fn field_values(&self) -> &HashMap<String, String> {
        &self.group_fields
    }
}

/// Parse the JSON response from the stats API into an AggregateResult.
fn parse_aggregate_result(result: Value) -> Result<AggregateResult> {
    match result {
        // Grouped: result is an array.
        Value::Array(arr) => {
            let mut groups = Vec::new();
            for item in arr {
                let stats = parse_stats_object(&item);
                let group_fields = parse_groupby_fields(&item);
                groups.push(AggregateGroup {
                    stats,
                    group_fields,
                });
            }
            Ok(AggregateResult {
                stats: HashMap::new(),
                groups,
            })
        }
        // Non-grouped: result is a single object.
        Value::Object(_) => {
            let stats = parse_stats_object(&result);
            Ok(AggregateResult {
                stats,
                groups: Vec::new(),
            })
        }
        _ => Err(Error::Api {
            status: 200,
            message: "unexpected aggregate response format".to_string(),
            detail: None,
        }),
    }
}

/// Parse the "stats" portion of an aggregate response item.
fn parse_stats_object(value: &Value) -> HashMap<String, HashMap<String, String>> {
    let mut result = HashMap::new();
    if let Some(stats) = value.get("stats").and_then(|v| v.as_object()) {
        let mut inner = HashMap::new();
        for (k, v) in stats {
            inner.insert(k.clone(), v.as_str().unwrap_or_default().to_string());
        }
        result.insert("stats".to_string(), inner);
    }
    result
}

/// Parse the "groupby_fields" array from a grouped aggregate response.
fn parse_groupby_fields(value: &Value) -> HashMap<String, String> {
    let mut fields = HashMap::new();
    if let Some(arr) = value.get("groupby_fields").and_then(|v| v.as_array()) {
        for item in arr {
            if let (Some(field), Some(val)) = (
                item.get("field").and_then(|v| v.as_str()),
                item.get("value").and_then(|v| v.as_str()),
            ) {
                fields.insert(field.to_string(), val.to_string());
            }
        }
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_non_grouped() {
        let json = serde_json::json!({"stats": {"count": "145"}});
        let result = parse_aggregate_result(json).unwrap();
        assert!(!result.is_grouped());
        assert_eq!(result.count(), 145);
    }

    #[test]
    fn test_parse_grouped() {
        let json = serde_json::json!([
            {
                "stats": {"count": "100"},
                "groupby_fields": [{"field": "state", "value": "1"}]
            },
            {
                "stats": {"count": "200"},
                "groupby_fields": [{"field": "state", "value": "2"}]
            }
        ]);
        let result = parse_aggregate_result(json).unwrap();
        assert!(result.is_grouped());
        assert_eq!(result.group_count(), 2);
        assert_eq!(result.groups()[0].count(), 100);
        assert_eq!(result.groups()[0].field_value("state"), "1");
        assert_eq!(result.groups()[1].count(), 200);
        assert_eq!(result.groups()[1].field_value("state"), "2");
    }

    #[test]
    fn test_parse_multi_stat() {
        let json = serde_json::json!({
            "stats": {
                "count": "50",
                "avg.priority": "2.5",
                "sum.reassignment_count": "120"
            }
        });
        let result = parse_aggregate_result(json).unwrap();
        assert_eq!(result.count(), 50);
        assert_eq!(result.stat_value("avg.priority"), Some("2.5"));
        assert_eq!(result.stat_value("sum.reassignment_count"), Some("120"));
    }
}
