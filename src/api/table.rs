// The Table API functionality is implemented directly in `query::builder::TableApi`.
//
// This module exists to hold Table API-specific constants and helpers
// that don't belong on the builder itself.

/// Base path for the ServiceNow Table API.
pub const TABLE_API_PATH: &str = "/api/now/table";

/// Default record limit if none is specified.
pub const DEFAULT_LIMIT: u32 = 100;

/// Maximum records ServiceNow will return in a single request.
pub const MAX_LIMIT: u32 = 10000;
