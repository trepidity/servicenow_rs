# servicenow_rs

A Rust library for the ServiceNow REST API. Async-first, built on Tokio and reqwest.

Provides a typed, builder-based interface to the ServiceNow Table API and Aggregate/Stats API with support for schema-aware relationship traversal, display values, dot-walking, pagination, retry with exponential backoff, and rate limiting.

## Features

- **Builder-pattern queries** -- filter, field selection, ordering, and pagination via method chaining
- **Full CRUD** -- get, create, update, delete records on any table
- **Relationship traversal** -- fetch related records (e.g., Change Request -> Change Tasks, Approvals) using concurrent batched requests driven by schema definitions
- **Dot-walking** -- inline fields from referenced records in a single HTTP request
- **Aggregate/Stats API** -- count, avg, sum, min, max with group-by and having clauses
- **Display values** -- raw database values, human-readable display values, or both
- **Flexible schema system** -- bundled base definitions per ServiceNow release with custom overlay support for org-specific tables and fields
- **Layered configuration** -- builder methods, environment variables, TOML config file, with clear precedence
- **Multiple auth methods** -- Basic auth and Bearer token auth, with a trait for custom implementations
- **Transport resilience** -- automatic retry with exponential backoff, rate limiting, session cookie reuse
- **Feature flags** -- `table_api` (default) and `codegen`

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
servicenow_rs = "0.1"
tokio = { version = "1", features = ["full"] }
serde_json = "1"
```

## Quick Start

### Basic Query

```rust
use servicenow_rs::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    let client = ServiceNowClient::builder()
        .instance("mycompany")
        .auth(BasicAuth::new("admin", "password"))
        .build()
        .await?;

    let result = client.table("incident")
        .equals("state", "1")
        .fields(&["number", "short_description", "state"])
        .limit(10)
        .execute()
        .await?;

    for record in &result {
        println!("{}: {}",
            record.get_str("number").unwrap_or("?"),
            record.get_str("short_description").unwrap_or("?"),
        );
    }

    Ok(())
}
```

### Get a Single Record

```rust
let record = client.table("change_request")
    .get("some_sys_id")
    .await?;

println!("Number: {}", record.get_str("number").unwrap_or("?"));
```

### Get the First Match

```rust
let record = client.table("incident")
    .equals("number", "INC0010001")
    .first()
    .await?;

if let Some(r) = record {
    println!("Found: {}", r.get_str("short_description").unwrap_or("?"));
}
```

### Create a Record

```rust
use serde_json::json;

let record = client.table("incident")
    .create(json!({
        "short_description": "Server unreachable",
        "urgency": "2"
    }))
    .await?;

println!("Created: {}", record.get_str("number").unwrap_or("?"));
```

### Update a Record

```rust
use serde_json::json;

let record = client.table("incident")
    .update("abc123", json!({ "state": "2" }))
    .await?;

println!("Updated state: {}", record.get_str("state").unwrap_or("?"));
```

### Delete a Record

```rust
client.table("incident")
    .delete("abc123")
    .await?;
```

### Complex Filters

The query builder supports all ServiceNow encoded query operators:

```rust
use servicenow_rs::prelude::*;

let result = client.table("incident")
    .equals("state", "1")
    .in_list("priority", &["1", "2"])
    .contains("short_description", "network")
    .order_by("sys_created_on", Order::Desc)
    .limit(20)
    .execute()
    .await?;
```

Available filter shorthands on `TableApi`:

| Method | Encoded Query Operator |
|---|---|
| `equals(field, value)` | `field=value` |
| `not_equals(field, value)` | `field!=value` |
| `contains(field, value)` | `fieldLIKEvalue` |
| `starts_with(field, value)` | `fieldSTARTSWITHvalue` |
| `ends_with(field, value)` | `fieldENDSWITHvalue` |
| `greater_than(field, value)` | `field>value` |
| `less_than(field, value)` | `field<value` |
| `in_list(field, values)` | `fieldINval1,val2` |
| `is_empty_field(field)` | `fieldISEMPTY` |
| `is_not_empty(field)` | `fieldISNOTEMPTY` |

For operators not covered by a shorthand, use `filter` directly with an `Operator` variant:

```rust
use servicenow_rs::query::Operator;

let result = client.table("incident")
    .filter("priority", Operator::LessThanOrEqual, "2")
    .or_filter("state", Operator::Equals, "1")
    .execute()
    .await?;
```

The full set of `Operator` variants is: `Equals`, `NotEquals`, `Contains`, `NotContains`, `StartsWith`, `EndsWith`, `GreaterThan`, `GreaterThanOrEqual`, `LessThan`, `LessThanOrEqual`, `In`, `NotIn`, `IsEmpty`, `IsNotEmpty`, `Between`, `Like`, `NotLike`, `InstanceOf`.

### Relationship Traversal

Fetch related records alongside the main query. Requires a schema to be loaded so the library knows how to resolve the relationship (which table, which foreign key):

```rust
let client = ServiceNowClient::builder()
    .instance("mycompany")
    .auth(BasicAuth::new("admin", "password"))
    .schema_release("xanadu")
    .build()
    .await?;

let result = client.table("change_request")
    .equals("number", "CHG0012345")
    .include_related(&["change_task", "approvals"])
    .execute()
    .await?;

for record in &result {
    println!("Change: {}", record.get_str("number").unwrap_or("?"));
    for task in record.related("change_task") {
        println!("  Task: {}", task.get_str("number").unwrap_or("?"));
    }
    for approval in record.related("approvals") {
        println!("  Approval state: {}", approval.get_str("state").unwrap_or("?"));
    }
}
```

Without a schema loaded, calling `include_related` will still return the main records but will produce partial errors on the `QueryResult`:

```rust
assert!(result.has_errors());  // schema errors for missing relationship definitions
assert!(!result.is_empty());   // main records are still returned
```

### Dot-Walking

Dot-walking fetches fields from referenced records inline in a single HTTP request. This is more efficient than `include_related` when you only need a few fields from a reference:

```rust
let result = client.table("incident")
    .fields(&["number"])
    .dot_walk(&["assigned_to.name", "assigned_to.email", "caller_id.manager.name"])
    .limit(5)
    .execute()
    .await?;

for record in &result {
    println!("{}: assigned to {} ({})",
        record.get_str("number").unwrap_or("?"),
        record.get_str("assigned_to.name").unwrap_or("?"),
        record.get_str("assigned_to.email").unwrap_or("?"),
    );
    // Multi-level dot-walking works too
    println!("  Manager: {}", record.get_str("caller_id.manager.name").unwrap_or("?"));
}

// Helper to get all dot-walked fields for a prefix
let assigned_fields = result.records[0].dot_walked_fields("assigned_to");
for (key, value) in &assigned_fields {
    println!("{} = {:?}", key, value.as_str());
}
```

### Pagination

#### Manual Page-by-Page

```rust
let mut paginator = client.table("incident")
    .equals("state", "1")
    .limit(100) // page size
    .paginate();

while let Some(page) = paginator.next_page().await? {
    println!("Got {} records (total: {:?})", page.len(), paginator.total_count());
    for record in &page {
        println!("  {}", record.get_str("number").unwrap_or("?"));
    }
}
```

#### Auto-Paginate All Records

```rust
// Fetch all matching records, paginating automatically
let result = client.table("incident")
    .equals("state", "1")
    .limit(100) // page size
    .execute_all(Some(500)) // safety cap: stop after 500 records
    .await?;

println!("Fetched {} of {:?} total", result.len(), result.total_count);
```

Pass `None` to `execute_all` to fetch everything (use with caution on large tables).

#### Collect Remaining Pages

```rust
let mut paginator = client.table("incident")
    .limit(50)
    .paginate();

// Fetch first page manually
let first_page = paginator.next_page().await?;

// Then collect the rest
let remaining = paginator.collect_all().await?;
```

#### Get Count Without Fetching Records

```rust
let count = client.table("incident")
    .equals("state", "1")
    .count()
    .await?;

println!("There are {} new incidents", count);
```

### Aggregate Queries

The Aggregate/Stats API provides server-side calculations:

```rust
// Simple count
let stats = client.aggregate("incident")
    .count()
    .execute()
    .await?;
println!("Total incidents: {}", stats.count());

// Grouped count with filter
let stats = client.aggregate("incident")
    .count()
    .group_by("state")
    .equals("active", "true")
    .execute()
    .await?;

for group in stats.groups() {
    println!("state={}: {}", group.field_value("state"), group.count());
}

// Multiple aggregate operations
let stats = client.aggregate("incident")
    .count()
    .avg("priority")
    .sum("reassignment_count")
    .min("priority")
    .max("priority")
    .execute()
    .await?;

println!("Count: {}", stats.count());
println!("Avg priority: {:?}", stats.avg("priority"));
println!("Sum reassignments: {:?}", stats.sum("reassignment_count"));

// HAVING clause
let stats = client.aggregate("incident")
    .count()
    .group_by("assignment_group")
    .having_count(">10")
    .execute()
    .await?;
```

## Configuration

Configuration uses a layered precedence system (highest wins):

1. Builder methods (explicit code)
2. Environment variables
3. Config file (`servicenow.toml`)
4. Defaults

### Builder

```rust
use std::time::Duration;

let client = ServiceNowClient::builder()
    .instance("mycompany")
    .auth(BasicAuth::new("admin", "password"))
    .schema_release("xanadu")
    .schema_overlay("./my_overlay.json")
    .max_retries(5)
    .timeout(Duration::from_secs(60))
    .rate_limit(20)  // requests per second
    .build()
    .await?;
```

### Environment Variables

| Variable | Purpose |
|---|---|
| `SERVICENOW_INSTANCE` | Instance URL or short name |
| `SERVICENOW_USERNAME` | Basic auth username |
| `SERVICENOW_PASSWORD` | Basic auth password |
| `SERVICENOW_API_TOKEN` | Bearer token for token auth |
| `SERVICENOW_OAUTH_CLIENT_ID` | OAuth client ID |
| `SERVICENOW_OAUTH_CLIENT_SECRET` | OAuth client secret |
| `SERVICENOW_SCHEMA_PATH` | Path to a custom schema overlay |

```rust
// Reads all SERVICENOW_* env vars automatically
let client = ServiceNowClient::from_env().await?;
```

### TOML Config File

Create a `servicenow.toml` (gitignored by default):

```toml
[instance]
url = "mycompany"

[auth]
method = "basic"
username = "admin"
password = "secret"

[schema]
release = "xanadu"
overlay = "./custom_schema.json"

[transport]
timeout_secs = 30
max_retries = 3
rate_limit = 20
```

```rust
// Load from servicenow.toml in current directory, then apply env var overrides
let client = ServiceNowClient::from_config().await?;

// Load from a specific file path
let client = ServiceNowClient::from_config_file("./config/prod.toml").await?;
```

### URL Normalization

The `instance` value is flexible. All of these produce `https://mycompany.service-now.com`:

```rust
.instance("mycompany")                              // bare name
.instance("mycompany.service-now.com")               // domain without scheme
.instance("https://mycompany.service-now.com")       // full URL
.instance("https://mycompany.service-now.com/")      // trailing slash stripped
```

Custom domains are preserved as-is:

```rust
.instance("servicenow.mycompany.com")
// -> https://servicenow.mycompany.com
```

## Authentication

### BasicAuth

```rust
use servicenow_rs::auth::BasicAuth;

// Explicit credentials
let auth = BasicAuth::new("admin", "password");

// From environment variables (SERVICENOW_USERNAME, SERVICENOW_PASSWORD)
let auth = BasicAuth::from_env()?;

// Disable session cookie reuse
let auth = BasicAuth::new("admin", "password").without_session();
```

BasicAuth encodes credentials as Base64 at construction time and supports session cookie reuse by default (reqwest's cookie store).

### TokenAuth

```rust
use servicenow_rs::auth::TokenAuth;

// Bearer token (Authorization: Bearer <token>)
let auth = TokenAuth::bearer("my-api-token");

// Custom header (e.g., X-sn-api-token: <token>)
let auth = TokenAuth::custom_header("X-sn-api-token", "my-token");

// From environment variable (SERVICENOW_API_TOKEN)
let auth = TokenAuth::from_env()?;
```

### Authenticator Trait

Both `BasicAuth` and `TokenAuth` implement the `Authenticator` trait. You can implement it for custom auth methods:

```rust
use async_trait::async_trait;
use reqwest::RequestBuilder;
use servicenow_rs::auth::Authenticator;
use servicenow_rs::error::Result;

#[derive(Debug)]
struct MyCustomAuth { /* ... */ }

#[async_trait]
impl Authenticator for MyCustomAuth {
    async fn authenticate(&self, request: RequestBuilder) -> Result<RequestBuilder> {
        Ok(request.header("X-My-Auth", "custom-value"))
    }

    fn method_name(&self) -> &str {
        "custom"
    }

    // Optional: implement refresh() for token rotation
    // Optional: implement supports_session() to enable cookie reuse
}
```

Then pass it to the builder:

```rust
let client = ServiceNowClient::builder()
    .instance("mycompany")
    .auth(MyCustomAuth { /* ... */ })
    .build()
    .await?;
```

## Schema System

The schema system enables relationship traversal, field type awareness, and inheritance-aware lookups.

### Base Definitions

The library ships bundled schema definitions for ServiceNow releases:
- `xanadu`
- `yokohama`
- `washington`

These are JSON files compiled into the binary via `include_str!` from `definitions/base/`.

```rust
let client = ServiceNowClient::builder()
    .instance("mycompany")
    .auth(BasicAuth::new("admin", "password"))
    .schema_release("xanadu")
    .build()
    .await?;
```

### Custom Overlays

For org-specific tables, custom fields (`u_` prefix), or additional relationships, create a JSON overlay file:

```json
{
    "extends_release": "xanadu",
    "tables": {
        "change_request": {
            "fields": {
                "u_custom_field": {
                    "type": "string",
                    "max_length": 255,
                    "label": "My Custom Field"
                }
            },
            "relationships": {
                "u_related_items": {
                    "table": "u_related_item",
                    "foreign_key": "change_request",
                    "type": "one_to_many"
                }
            }
        },
        "u_custom_table": {
            "label": "Custom Table",
            "fields": {
                "u_name": { "type": "string", "max_length": 100 }
            }
        }
    }
}
```

Apply the overlay:

```rust
let client = ServiceNowClient::builder()
    .instance("mycompany")
    .auth(BasicAuth::new("admin", "password"))
    .schema_release("xanadu")
    .schema_overlay("./my_overlay.json")
    .build()
    .await?;
```

Or apply programmatically:

```rust
use servicenow_rs::schema::SchemaRegistry;

let registry = SchemaRegistry::from_release_with_overlay_str("xanadu", r#"{
    "extends_release": "xanadu",
    "tables": {
        "incident": {
            "fields": {
                "u_env": { "type": "string", "label": "Environment" }
            }
        }
    }
}"#)?;
```

### Schema Lookups

The `SchemaRegistry` provides lookup methods that walk the table inheritance chain (via `extends`):

```rust
let registry = SchemaRegistry::from_release("xanadu")?;

// Table lookup
let table = registry.table("change_request").unwrap();
println!("Label: {}", table.label);                // "Change Request"
println!("Parent: {:?}", table.extends);           // Some("task")

// Field lookup (walks inheritance: change_request -> task)
let field = registry.field("change_request", "number").unwrap();
println!("Type: {:?}", field.field_type);          // String

// Relationship lookup
let rel = registry.relationship("change_request", "change_task").unwrap();
println!("Related table: {}", rel.table);          // "change_task"
println!("Foreign key: {}", rel.foreign_key);      // "change_request"

// All relationships (including inherited)
let rels = registry.relationships("change_request");

// Utility checks
assert!(registry.has_table("incident"));
assert!(registry.has_field("change_request", "risk"));
```

## Display Values

ServiceNow fields can return raw database values, human-readable display values, or both. Control this with `DisplayValue`:

```rust
use servicenow_rs::model::DisplayValue;

// Raw values (default) -- state returns "1"
let result = client.table("incident")
    .display_value(DisplayValue::Raw)
    .execute().await?;
let raw = result.records[0].get_str("state"); // Some("1")

// Display values -- state returns "New"
let result = client.table("incident")
    .display_value(DisplayValue::Display)
    .execute().await?;
let display = result.records[0].get_str("state"); // Some("New")

// Both -- access raw and display separately
let result = client.table("incident")
    .display_value(DisplayValue::Both)
    .execute().await?;

let record = &result.records[0];
let raw = record.get_raw("state");       // Some("1")
let display = record.get_display("state"); // Some("New")
let prefer_display = record.get_str("state"); // Some("New") -- prefers display
```

Reference fields with `DisplayValue::Both` also include a link URL:

```rust
let fv = record.get("assigned_to").unwrap();
println!("Sys ID: {:?}", fv.raw_str());      // Some("user_sys_id")
println!("Name: {:?}", fv.display_str());     // Some("John Smith")
println!("Link: {:?}", fv.link);              // Some("https://...")
```

## Error Handling

All operations return `servicenow_rs::error::Result<T>`. The error type covers:

```rust
use servicenow_rs::error::Error;

match client.table("incident").execute().await {
    Ok(result) => { /* ... */ }
    Err(Error::Auth { message, status }) => {
        // 401/403 -- bad credentials or insufficient permissions
    }
    Err(Error::Api { status, message, detail }) => {
        // 4xx/5xx from ServiceNow (e.g., 404 table not found)
    }
    Err(Error::RateLimited { retry_after }) => {
        // 429 -- all retries exhausted
    }
    Err(Error::Config(msg)) => {
        // Missing or invalid configuration
    }
    Err(Error::Schema(msg)) => {
        // Schema loading or lookup failure
    }
    Err(Error::Query(msg)) => {
        // Query building error
    }
    Err(Error::Http(e)) => {
        // Network/transport error (reqwest)
    }
    Err(Error::Json(e)) => {
        // Response parsing failure
    }
    Err(Error::PartialResult { succeeded, failed, errors }) => {
        // Some sub-operations failed (e.g., related record fetch)
    }
    Err(e) => {
        // IO, TOML, URL parse errors
    }
}
```

`QueryResult` tracks partial errors (e.g., a related-record fetch fails but the main query succeeds):

```rust
let result = client.table("change_request")
    .include_related(&["change_task"])
    .execute()
    .await?;

if result.has_errors() {
    for err in &result.errors {
        eprintln!("Partial error: {}", err);
    }
}
// Main records are still available
for record in &result {
    println!("{}", record.get_str("number").unwrap_or("?"));
}
```

## Feature Flags

| Feature | Default | Description |
|---|---|---|
| `table_api` | Yes | Table API support (query, CRUD, pagination) |
| `codegen` | No | Code generation for typed table structs (future) |

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes with tests
4. Run checks:
   ```bash
   cargo check
   cargo test
   cargo clippy -- -D warnings
   ```
5. Open a pull request

All public API changes require tests. Integration tests use [wiremock](https://crates.io/crates/wiremock) to mock ServiceNow responses. See `tests/integration_test.rs` for examples.

## License

MIT
