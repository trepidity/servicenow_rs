// The Table API functionality is implemented directly in `query::builder::TableApi`.
// Constants for the Table API and Stats API paths.

/// Base path for the ServiceNow Table API.
pub const TABLE_API_PATH: &str = "/api/now/table";

/// Base path for the ServiceNow Stats/Aggregate API.
pub const STATS_API_PATH: &str = "/api/now/stats";

/// Default page size for pagination.
pub const DEFAULT_PAGE_SIZE: u32 = 100;

/// Default safety limit for execute_all().
pub const DEFAULT_MAX_RECORDS: u64 = 10_000;

/// Maximum records ServiceNow will return in a single request.
pub const MAX_LIMIT: u32 = 10_000;
