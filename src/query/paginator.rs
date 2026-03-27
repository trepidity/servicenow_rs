use std::sync::Arc;

use serde_json::Value;
use tracing::debug;

use crate::error::Result;
use crate::model::record::Record;
use crate::model::result::QueryResult;
use crate::model::value::DisplayValue;
use crate::transport::http::HttpTransport;

/// Pagination state for iterating through large result sets.
///
/// Created via `TableApi::paginate()`. Yields one page of results at a time.
///
/// # Examples
///
/// ```no_run
/// # async fn example() -> servicenow_rs::error::Result<()> {
/// # let client: servicenow_rs::client::ServiceNowClient = todo!();
/// let mut paginator = client.table("incident")
///     .equals("state", "1")
///     .limit(100)
///     .paginate();
///
/// while let Some(page) = paginator.next_page().await? {
///     println!("Got {} records (total: {:?})", page.len(), paginator.total_count());
///     for record in &page {
///         println!("  {}", record.get_str("number").unwrap_or("?"));
///     }
/// }
/// # Ok(())
/// # }
/// ```
pub struct Paginator {
    transport: Arc<HttpTransport>,
    path: String,
    base_params: Vec<(String, String)>,
    page_size: u32,
    current_offset: u32,
    total_count: Option<u64>,
    display_value: DisplayValue,
    table: String,
    done: bool,
}

impl Paginator {
    /// Create a new paginator. Called internally by `TableApi::paginate()`.
    pub(crate) fn new(
        transport: Arc<HttpTransport>,
        table: String,
        base_params: Vec<(String, String)>,
        page_size: u32,
        display_value: DisplayValue,
    ) -> Self {
        let path = format!("/api/now/table/{}", table);
        Self {
            transport,
            path,
            base_params,
            page_size,
            current_offset: 0,
            total_count: None,
            display_value,
            table,
            done: false,
        }
    }

    /// Fetch the next page of results. Returns `Ok(None)` when there are no more pages.
    pub async fn next_page(&mut self) -> Result<Option<QueryResult>> {
        if self.done {
            return Ok(None);
        }

        // Build params for this page.
        let mut params = self.base_params.clone();
        params.push(("sysparm_limit".to_string(), self.page_size.to_string()));
        params.push(("sysparm_offset".to_string(), self.current_offset.to_string()));

        debug!(
            table = self.table,
            offset = self.current_offset,
            limit = self.page_size,
            "fetching page"
        );

        let response = self.transport.get(&self.path, &params).await?;

        // Update total count from response.
        if let Some(tc) = response.total_count {
            self.total_count = Some(tc);
        }

        // Parse records.
        let records: Vec<Record> = match response.result {
            Value::Array(arr) => arr
                .iter()
                .filter_map(|v| Record::from_json(&self.table, v, self.display_value))
                .collect(),
            _ => Vec::new(),
        };

        let count = records.len() as u32;

        // Advance offset.
        self.current_offset += count;

        // Check if we've reached the end.
        if count < self.page_size {
            self.done = true;
        }
        if let Some(total) = self.total_count {
            if self.current_offset as u64 >= total {
                self.done = true;
            }
        }
        if count == 0 {
            self.done = true;
            return Ok(None);
        }

        Ok(Some(QueryResult {
            records,
            total_count: self.total_count,
            errors: Vec::new(),
        }))
    }

    /// Get the total count of matching records (available after the first page is fetched).
    pub fn total_count(&self) -> Option<u64> {
        self.total_count
    }

    /// Get the current offset (number of records already fetched).
    pub fn current_offset(&self) -> u32 {
        self.current_offset
    }

    /// Whether all pages have been fetched.
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Collect all remaining pages into a single QueryResult.
    ///
    /// Fetches pages sequentially until exhausted. If the paginator has
    /// already fetched some pages, this only collects the remaining ones.
    pub async fn collect_all(&mut self) -> Result<QueryResult> {
        let mut all_records = Vec::new();
        let mut all_errors = Vec::new();

        while let Some(page) = self.next_page().await? {
            all_errors.extend(page.errors);
            all_records.extend(page.records);
        }

        Ok(QueryResult {
            records: all_records,
            total_count: self.total_count,
            errors: all_errors,
        })
    }
}

/// Configuration for auto-pagination behavior.
#[derive(Debug, Clone)]
pub struct PaginationConfig {
    /// Page size for each request.
    pub page_size: u32,
    /// Maximum total records to fetch (safety limit). None = no limit.
    pub max_records: Option<u64>,
}

impl Default for PaginationConfig {
    fn default() -> Self {
        Self {
            page_size: 100,
            max_records: Some(10_000),
        }
    }
}
