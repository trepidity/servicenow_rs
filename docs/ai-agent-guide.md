# servicenow_rs -- AI Agent Integration Guide

This document is written for AI agents (Claude, GPT, Copilot, and similar) that need
to use, extend, or reason about the `servicenow_rs` Rust library. It contains exact
type names, method signatures, and working code. Treat it as the authoritative quick
reference for the library's public API surface.

---

## 1. Library Overview

`servicenow_rs` is an async Rust library for the ServiceNow REST API. It runs on Tokio
and uses `reqwest` for HTTP.

**What it provides:**

- Typed client with builder-pattern configuration
- Table API: query, get, create, update, delete records
- Aggregate/Stats API: count, avg, sum, min, max with group-by
- Relationship traversal: fetch related records (e.g., Change -> Change Tasks)
- Dot-walking: inline related fields in a single request
- Pagination: manual page-by-page or auto-collect
- Schema system: bundled definitions per ServiceNow release with overlay support
- Multiple auth methods: Basic and Token/Bearer (extensible via trait)
- Retry with exponential backoff, rate limiting, session cookie reuse

**Key entry points:**

| What you want | Start here |
|---|---|
| Build a client | `ServiceNowClient::builder()`, `::from_env()`, `::from_config()` |
| Query records | `client.table("table_name")` returns `TableApi` |
| Aggregate stats | `client.aggregate("table_name")` returns `AggregateApi` |
| Access schema | `client.schema()` returns `Option<&SchemaRegistry>` |

**Prelude import:**

```rust
use servicenow_rs::prelude::*;
```

This re-exports: `ServiceNowClient`, `ClientBuilder`, `BasicAuth`, `TokenAuth`,
`Error`, `Result`, `Record`, `FieldValue`, `QueryResult`, `DisplayValue`, `Operator`,
`Order`, `FetchStrategy`, `Paginator`, `AggregateApi`, `AggregateResult`,
`SchemaRegistry`.

---

## 2. Quick Reference

### 2.1 Create a Client

**Via builder (explicit credentials):**

```rust
use servicenow_rs::prelude::*;

let client = ServiceNowClient::builder()
    .instance("mycompany")                    // or full URL
    .auth(BasicAuth::new("admin", "password"))
    .schema_release("xanadu")                 // optional
    .max_retries(3)                           // optional, default 3
    .timeout(std::time::Duration::from_secs(30)) // optional
    .rate_limit(20)                           // optional, requests/sec
    .build()
    .await?;
```

**Via environment variables:**

```rust
// Reads SERVICENOW_INSTANCE, SERVICENOW_USERNAME, SERVICENOW_PASSWORD
let client = ServiceNowClient::from_env().await?;
```

**Via config file + env fallback:**

```rust
// Reads servicenow.toml in cwd, then applies env var overrides
let client = ServiceNowClient::from_config().await?;

// Or from a specific file:
let client = ServiceNowClient::from_config_file("path/to/config.toml").await?;
```

**Token auth:**

```rust
let client = ServiceNowClient::builder()
    .instance("mycompany")
    .auth(TokenAuth::bearer("my-api-token"))
    .build()
    .await?;

// Custom header (e.g., X-sn-api-token):
let client = ServiceNowClient::builder()
    .instance("mycompany")
    .auth(TokenAuth::custom_header("X-sn-api-token", "my-token"))
    .build()
    .await?;
```

### 2.2 Query Records with Filters

```rust
let results = client.table("incident")
    .equals("state", "1")
    .not_equals("priority", "5")
    .contains("short_description", "network")
    .fields(&["number", "short_description", "state", "priority"])
    .order_by("sys_created_on", Order::Desc)
    .limit(50)
    .execute()
    .await?;

for record in &results {
    println!("{}: {}",
        record.get_str("number").unwrap_or("?"),
        record.get_str("short_description").unwrap_or("?"));
}
```

**Available filter shorthand methods on TableApi:**

| Method | Operator |
|---|---|
| `.equals(field, value)` | `=` |
| `.not_equals(field, value)` | `!=` |
| `.contains(field, value)` | `LIKE` |
| `.starts_with(field, value)` | `STARTSWITH` |
| `.ends_with(field, value)` | `ENDSWITH` |
| `.greater_than(field, value)` | `>` |
| `.less_than(field, value)` | `<` |
| `.in_list(field, &["a","b"])` | `IN` |
| `.is_empty_field(field)` | `ISEMPTY` |
| `.is_not_empty(field)` | `ISNOTEMPTY` |

**OR conditions and raw operators:**

```rust
let results = client.table("incident")
    .filter("state", Operator::Equals, "1")
    .or_filter("state", Operator::Equals, "2")
    .execute()
    .await?;
// Produces: state=1^ORstate=2
```

### 2.3 Get a Single Record

```rust
let record = client.table("change_request")
    .fields(&["number", "short_description", "state"])
    .display_value(DisplayValue::Both)
    .get("abc123_sys_id")
    .await?;

println!("Number: {}", record.get_str("number").unwrap_or("?"));
println!("State raw: {}", record.get_raw("state").unwrap_or("?"));
println!("State display: {}", record.get_display("state").unwrap_or("?"));
```

### 2.4 Create a Record

```rust
let new_record = client.table("incident")
    .create(serde_json::json!({
        "short_description": "Server is down",
        "urgency": "1",
        "impact": "1",
        "assignment_group": "some_group_sys_id",
        "caller_id": "some_user_sys_id"
    }))
    .await?;

println!("Created: {}", new_record.sys_id);
```

### 2.5 Update a Record

```rust
let updated = client.table("incident")
    .update("existing_sys_id", serde_json::json!({
        "state": "2",
        "work_notes": "Investigating the issue"
    }))
    .await?;
```

### 2.6 Delete a Record

```rust
client.table("incident")
    .delete("sys_id_to_delete")
    .await?;
```

### 2.7 Fetch with Relationships

Requires a schema to be loaded (`.schema_release("xanadu")` on the builder).

```rust
let changes = client.table("change_request")
    .equals("state", "1")
    .include_related(&["change_task", "approvals"])
    .display_value(DisplayValue::Both)
    .limit(10)
    .execute()
    .await?;

for record in &changes {
    println!("Change: {}", record.get_str("number").unwrap_or("?"));
    for task in record.related("change_task") {
        println!("  Task: {}", task.get_str("number").unwrap_or("?"));
    }
}
```

### 2.8 Paginate Through Results

**Manual pagination:**

```rust
let mut paginator = client.table("incident")
    .equals("active", "true")
    .limit(100)   // this becomes the page size
    .paginate();

while let Some(page) = paginator.next_page().await? {
    println!("Page: {} records (total: {:?})", page.len(), paginator.total_count());
    for record in &page {
        println!("  {}", record.get_str("number").unwrap_or("?"));
    }
}
```

**Auto-collect all pages:**

```rust
// Collect all, with a safety cap:
let all = client.table("incident")
    .equals("active", "true")
    .limit(100)
    .execute_all(Some(5000))   // max 5000 records total
    .await?;

// Or via paginator:
let mut paginator = client.table("incident")
    .equals("active", "true")
    .limit(100)
    .paginate();
let all = paginator.collect_all().await?;
```

### 2.9 Aggregate Queries

```rust
// Simple count
let stats = client.aggregate("incident")
    .count()
    .equals("active", "true")
    .execute()
    .await?;
println!("Active incidents: {}", stats.count());

// Grouped count
let stats = client.aggregate("incident")
    .count()
    .group_by("state")
    .equals("active", "true")
    .execute()
    .await?;
for group in stats.groups() {
    println!("State {}: {} incidents", group.field_value("state"), group.count());
}

// Multiple aggregates
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
```

### 2.10 Dot-Walk Fields

Dot-walking fetches fields from referenced records in a single HTTP request.
More efficient than `include_related` when you only need a few fields.

```rust
let results = client.table("incident")
    .dot_walk(&[
        "assigned_to.name",
        "assigned_to.email",
        "caller_id.manager.name",
    ])
    .fields(&["number", "short_description"])
    .limit(10)
    .execute()
    .await?;

for record in &results {
    println!("{}: assigned to {}, manager {}",
        record.get_str("number").unwrap_or("?"),
        record.get_str("assigned_to.name").unwrap_or("unassigned"),
        record.get_str("caller_id.manager.name").unwrap_or("?"));
}

// You can also get all dot-walked fields for a prefix:
let assigned_fields = results.first().unwrap().dot_walked_fields("assigned_to");
for (field_name, value) in &assigned_fields {
    println!("  {}: {:?}", field_name, value.as_str());
}
```

### 2.11 Use Display Values

```rust
// Raw values only (default) -- state returns "1", "2", etc.
let results = client.table("incident")
    .display_value(DisplayValue::Raw)
    .execute().await?;

// Display values only -- state returns "New", "In Progress", etc.
let results = client.table("incident")
    .display_value(DisplayValue::Display)
    .execute().await?;

// Both -- each field has .value and .display_value
let results = client.table("incident")
    .display_value(DisplayValue::Both)
    .execute().await?;
let record = results.first().unwrap();
let state = record.get("state").unwrap();
println!("Raw: {:?}", state.raw_str());       // Some("1")
println!("Display: {:?}", state.display_str()); // Some("New")
println!("Prefer display: {:?}", state.as_str()); // Some("New")
```

### 2.12 Read Journal Entries

Journal fields (`work_notes`, `comments`) are write-only on the parent record.
Query `sys_journal_field` to read structured entries.

```rust
let entries = client.table("sys_journal_field")
    .equals("element_id", "target_record_sys_id")  // sys_id of the incident/change/etc.
    .equals("name", "incident")                     // table name of the parent record
    .equals("element", "work_notes")                // "work_notes" or "comments"
    .fields(&["element", "value", "sys_created_on", "sys_created_by"])
    .order_by("sys_created_on", Order::Desc)
    .limit(50)
    .execute()
    .await?;

for entry in &entries {
    println!("{} by {}: {}",
        entry.get_str("sys_created_on").unwrap_or("?"),
        entry.get_str("sys_created_by").unwrap_or("?"),
        entry.get_str("value").unwrap_or(""));
}
```

---

## 3. Type Reference

### 3.1 ServiceNowClient

```
src/client.rs
```

The primary client. Holds an `Arc<HttpTransport>` and an optional `Arc<SchemaRegistry>`.

| Method | Returns | Description |
|---|---|---|
| `::builder()` | `ClientBuilder` | Start building a client |
| `::from_env()` | `Result<Self>` | Create from env vars |
| `::from_config()` | `Result<Self>` | Create from servicenow.toml + env |
| `::from_config_file(path)` | `Result<Self>` | Create from specific TOML file + env |
| `.table(name)` | `TableApi` | Start a Table API operation |
| `.aggregate(table)` | `AggregateApi` | Start an aggregate query |
| `.schema()` | `Option<&SchemaRegistry>` | Get loaded schema, if any |

### 3.2 ClientBuilder

```
src/client.rs
```

Builder with layered configuration. All setters return `Self`.

| Method | Description |
|---|---|
| `.instance(name_or_url)` | Set instance (e.g., "mycompany" or full URL) |
| `.auth(impl Authenticator)` | Set auth method |
| `.schema_release(name)` | Load bundled schema (e.g., "xanadu") |
| `.schema_overlay(path)` | Path to overlay JSON file |
| `.max_retries(n)` | Max retry attempts (default 3) |
| `.timeout(duration)` | Request timeout (default 30s) |
| `.rate_limit(rps)` | Requests per second |
| `.from_config_file(path)` | Load TOML config |
| `.from_default_config()` | Load servicenow.toml from cwd |
| `.from_env()` | Apply env var overrides |
| `.build()` | `Result<ServiceNowClient>` (async) |

### 3.3 TableApi

```
src/query/builder.rs
```

Builder for Table API operations. Created via `client.table("name")`.
All builder methods return `Self`. Terminal operations consume `self`.

**Builder methods (return Self):**

| Method | Description |
|---|---|
| `.filter(field, Operator, value)` | AND filter |
| `.or_filter(field, Operator, value)` | OR filter |
| `.equals(field, value)` | Shorthand for Operator::Equals |
| `.not_equals(field, value)` | Shorthand for Operator::NotEquals |
| `.contains(field, value)` | Shorthand for Operator::Contains |
| `.starts_with(field, value)` | Shorthand for Operator::StartsWith |
| `.ends_with(field, value)` | Shorthand for Operator::EndsWith |
| `.greater_than(field, value)` | Shorthand for Operator::GreaterThan |
| `.less_than(field, value)` | Shorthand for Operator::LessThan |
| `.in_list(field, &[values])` | Shorthand for Operator::In |
| `.is_empty_field(field)` | Shorthand for Operator::IsEmpty |
| `.is_not_empty(field)` | Shorthand for Operator::IsNotEmpty |
| `.fields(&[names])` | Select specific fields |
| `.dot_walk(&[dotted_names])` | Add dot-walked fields |
| `.include_related(&[rel_names])` | Fetch related records (needs schema) |
| `.display_value(DisplayValue)` | Set display value mode |
| `.limit(n)` | Max records / page size |
| `.offset(n)` | Pagination offset |
| `.order_by(field, Order)` | Sort results |
| `.strategy(FetchStrategy)` | Override related-record fetch strategy |
| `.exclude_reference_link(bool)` | Exclude reference link URLs (default true) |
| `.no_count()` | Skip X-Total-Count for performance |

**Terminal operations (consume self):**

| Method | Returns | Description |
|---|---|---|
| `.execute()` | `Result<QueryResult>` | Run query, return records |
| `.first()` | `Result<Option<Record>>` | Get first matching record |
| `.execute_all(max)` | `Result<QueryResult>` | Auto-paginate and collect |
| `.paginate()` | `Paginator` | Create manual paginator |
| `.count()` | `Result<u64>` | Count matching records |
| `.get(sys_id)` | `Result<Record>` | Get single record by sys_id |
| `.create(json)` | `Result<Record>` | POST a new record |
| `.update(sys_id, json)` | `Result<Record>` | PATCH an existing record |
| `.delete(sys_id)` | `Result<()>` | DELETE a record |

### 3.4 AggregateApi and AggregateResult

```
src/api/aggregate.rs
```

**AggregateApi builder methods (return Self):**

| Method | Description |
|---|---|
| `.count()` | Include count |
| `.avg(field)` | Average of field |
| `.sum(field)` | Sum of field |
| `.min(field)` | Min of field |
| `.max(field)` | Max of field |
| `.group_by(field)` | Group results by field |
| `.having_count(condition)` | HAVING clause (e.g., ">10") |
| `.display_value(bool)` | Use display values in group-by |
| `.filter(field, Operator, value)` | AND filter |
| `.equals(field, value)` | Shorthand equals filter |
| `.order_by(field, Order)` | Sort groups |
| `.execute()` | Terminal: `Result<AggregateResult>` |

**AggregateResult methods:**

| Method | Returns | Description |
|---|---|---|
| `.count()` | `u64` | Total count (non-grouped) |
| `.stat_value(name)` | `Option<&str>` | Raw stat by name |
| `.avg(field)` | `Option<f64>` | Average value |
| `.sum(field)` | `Option<f64>` | Sum value |
| `.min_val(field)` | `Option<&str>` | Min value |
| `.max_val(field)` | `Option<&str>` | Max value |
| `.is_grouped()` | `bool` | Whether result has groups |
| `.groups()` | `&[AggregateGroup]` | All groups |
| `.group_count()` | `usize` | Number of groups |

**AggregateGroup methods:**

| Method | Returns | Description |
|---|---|---|
| `.count()` | `u64` | Count for this group |
| `.stat_value(name)` | `Option<&str>` | Stat by name |
| `.field_value(field)` | `&str` | Group-by field value (empty string if missing) |
| `.field_values()` | `&HashMap<String, String>` | All group-by values |

### 3.5 Record

```
src/model/record.rs
```

A single ServiceNow record.

| Field/Method | Type/Returns | Description |
|---|---|---|
| `.table` | `String` | Table name |
| `.sys_id` | `String` | Record sys_id |
| `.get(field)` | `Option<&FieldValue>` | Get field value object |
| `.get_str(field)` | `Option<&str>` | Prefer display, fall back to raw |
| `.get_raw(field)` | `Option<&str>` | Raw database value |
| `.get_display(field)` | `Option<&str>` | Display/rendered value |
| `.dot_walked_fields(prefix)` | `Vec<(&str, &FieldValue)>` | All fields starting with prefix. |
| `.has_field(field)` | `bool` | Check field existence |
| `.field_names()` | `impl Iterator<Item = &str>` | All field names |
| `.fields()` | `&HashMap<String, FieldValue>` | All fields |
| `.set(field, FieldValue)` | `()` | Set a field |
| `.related(name)` | `&[Record]` | Related records by name |
| `.relationship_names()` | `impl Iterator<Item = &str>` | Names with data |
| `.set_related(name, Vec<Record>)` | `()` | Attach related records |
| `.has_related()` | `bool` | Any related records? |

**Static constructors:**

| Method | Description |
|---|---|
| `Record::new(table, sys_id)` | Empty record |
| `Record::from_json(table, &Value, DisplayValue)` | Parse from JSON |

### 3.6 FieldValue

```
src/model/value.rs
```

| Field | Type | Description |
|---|---|---|
| `.value` | `Option<serde_json::Value>` | Raw database value |
| `.display_value` | `Option<String>` | Human-readable display |
| `.link` | `Option<String>` | Reference link URL |

| Method | Returns | Description |
|---|---|---|
| `::from_raw(Value)` | `FieldValue` | From raw value |
| `::from_display(String)` | `FieldValue` | From display value |
| `.as_str()` | `Option<&str>` | Prefer display, fall back to raw |
| `.raw_str()` | `Option<&str>` | Raw as str |
| `.display_str()` | `Option<&str>` | Display as str |

### 3.7 QueryResult

```
src/model/result.rs
```

| Field/Method | Type/Returns | Description |
|---|---|---|
| `.records` | `Vec<Record>` | The records |
| `.total_count` | `Option<u64>` | Total matching (from X-Total-Count) |
| `.errors` | `Vec<Error>` | Partial failure errors |
| `.has_errors()` | `bool` | Any partial errors? |
| `.is_ok()` | `bool` | No errors at all? |
| `.len()` | `usize` | Record count |
| `.is_empty()` | `bool` | Zero records? |
| `.first()` | `Option<&Record>` | First record |
| `.iter()` | `impl Iterator<Item = &Record>` | Iterate records |

Implements `IntoIterator` for both `QueryResult` and `&QueryResult`.

### 3.8 Operator

```
src/query/filter.rs
```

```rust
pub enum Operator {
    Equals,           // =
    NotEquals,        // !=
    Contains,         // LIKE
    NotContains,      // NOT LIKE
    StartsWith,       // STARTSWITH
    EndsWith,         // ENDSWITH
    GreaterThan,      // >
    GreaterThanOrEqual, // >=
    LessThan,         // <
    LessThanOrEqual,  // <=
    In,               // IN
    NotIn,            // NOT IN
    IsEmpty,          // ISEMPTY
    IsNotEmpty,       // ISNOTEMPTY
    Between,          // BETWEEN
    Like,             // LIKE
    NotLike,          // NOT LIKE
    InstanceOf,       // INSTANCEOF
}
```

### 3.9 Order

```rust
pub enum Order {
    Asc,
    Desc,
}
```

### 3.10 DisplayValue

```rust
pub enum DisplayValue {
    Raw,     // sysparm_display_value=false (default)
    Display, // sysparm_display_value=true
    Both,    // sysparm_display_value=all
}
```

### 3.11 FetchStrategy

```rust
pub enum FetchStrategy {
    Auto,       // Library picks (default)
    DotWalk,    // Inline via dot-walking
    Concurrent, // Parallel HTTP requests per relationship
}
```

### 3.12 Paginator

```
src/query/paginator.rs
```

Created via `TableApi::paginate()`.

| Method | Returns | Description |
|---|---|---|
| `.next_page()` | `Result<Option<QueryResult>>` | Next page, None when done |
| `.total_count()` | `Option<u64>` | Total (after first page) |
| `.current_offset()` | `u32` | Records fetched so far |
| `.is_done()` | `bool` | All pages fetched? |
| `.collect_all()` | `Result<QueryResult>` | Collect remaining pages |

### 3.13 Error

```
src/error.rs
```

```rust
pub enum Error {
    Http(reqwest::Error),
    Auth { message: String, status: Option<u16> },
    Api { status: u16, message: String, detail: Option<String> },
    RateLimited { retry_after: Option<u64> },
    Schema(String),
    Config(String),
    Query(String),
    Json(serde_json::Error),
    UrlParse(url::ParseError),
    Io(std::io::Error),
    Toml(toml::de::Error),
    PartialResult { succeeded: usize, failed: usize, errors: Vec<Error> },
}

pub type Result<T> = std::result::Result<T, Error>;
```

### 3.14 Auth Types

**BasicAuth** (`src/auth/basic.rs`):

| Method | Description |
|---|---|
| `::new(username, password)` | Create from strings |
| `::from_env()` | From SERVICENOW_USERNAME and SERVICENOW_PASSWORD |
| `.without_session()` | Disable cookie reuse |
| `.username()` | Get username |

**TokenAuth** (`src/auth/token.rs`):

| Method | Description |
|---|---|
| `::bearer(token)` | Authorization: Bearer <token> |
| `::custom_header(header, token)` | Custom header name |
| `::from_env()` | From SERVICENOW_API_TOKEN |

**Authenticator trait** (`src/auth/mod.rs`):

```rust
#[async_trait]
pub trait Authenticator: Send + Sync + std::fmt::Debug {
    async fn authenticate(&self, request: RequestBuilder) -> Result<RequestBuilder>;
    async fn refresh(&self) -> Result<()> { Ok(()) }
    fn supports_session(&self) -> bool { false }
    fn method_name(&self) -> &str;
}
```

### 3.15 SchemaRegistry

```
src/schema/registry.rs
```

| Method | Returns | Description |
|---|---|---|
| `::new(SchemaDefinition)` | `Self` | From pre-built definition |
| `::from_release(name)` | `Result<Self>` | Load bundled (xanadu, yokohama, washington) |
| `::from_release_with_overlay(release, path)` | `Result<Self>` | Bundled + overlay file |
| `::from_release_with_overlay_str(release, json)` | `Result<Self>` | Bundled + overlay string |
| `.apply_overlay(&mut self, &SchemaOverlay)` | `()` | Merge additional overlay |
| `.release()` | `&str` | Release name |
| `.table(name)` | `Option<&TableDef>` | Table definition |
| `.field(table, field)` | `Option<&FieldDef>` | Field def (walks inheritance) |
| `.relationship(table, name)` | `Option<&RelationshipDef>` | Relationship def (walks inheritance) |
| `.relationships(table)` | `Vec<(&str, &RelationshipDef)>` | All relationships including inherited |
| `.table_names()` | `Vec<&str>` | All known tables |
| `.has_table(name)` | `bool` | Table exists? |
| `.has_field(table, field)` | `bool` | Field exists? |
| `.parent_table(table)` | `Option<&str>` | Parent via "extends" |
| `.all_fields(table)` | `Vec<(&str, &FieldDef)>` | All fields including inherited (walks extends chain) |
| `.writable_fields(table)` | `Vec<(&str, &FieldDef)>` | Fields where `is_writable()` is true (not read-only) |
| `.read_only_fields(table)` | `Vec<(&str, &FieldDef)>` | Fields where `read_only` is true |
| `.mandatory_fields(table)` | `Vec<(&str, &FieldDef)>` | Fields where `mandatory` is true |
| `.journal_fields(table)` | `Vec<(&str, &FieldDef)>` | Fields where `is_journal()` is true (Journal/JournalInput types) |
| `.schema()` | `&SchemaDefinition` | Full definition |

### 3.16 Schema Definition Types

```
src/schema/definition.rs
```

**SchemaDefinition**: `{ release: String, tables: HashMap<String, TableDef> }`

**TableDef**: `{ label: String, extends: Option<String>, fields: HashMap<String, FieldDef>, relationships: HashMap<String, RelationshipDef> }`

**FieldDef**: `{ field_type: FieldType, max_length: Option<u32>, read_only: bool, mandatory: bool, write_only: bool, choices: Option<HashMap<String, String>>, reference_table: Option<String>, label: Option<String> }`

- `read_only` -- system-generated fields (e.g., `sys_id`, `number`). Cannot be set via API.
- `mandatory` -- required fields. Must be provided on create; should be provided on update.
- `write_only` -- journal fields (`work_notes`, `comments`). Can be set via POST/PATCH but always return empty strings on GET. To read actual journal entries, query `sys_journal_field` directly (see Common Patterns).

**FieldDef helper methods:**

| Method | Returns | Description |
|---|---|---|
| `.is_writable()` | `bool` | `true` if the field is not read-only (i.e., can be set via API) |
| `.is_journal()` | `bool` | `true` if `field_type` is `Journal` or `JournalInput` |
| `.is_reference()` | `bool` | `true` if `field_type` is `Reference` and `reference_table` is set |

**RelationshipDef**: `{ table: String, foreign_key: String, relationship_type: RelationshipType, filter: Option<String> }`

**FieldType enum**: `String, Integer, Boolean, Decimal, Float, DateTime, Date, Time, Reference, Journal, JournalInput, GlideList, Url, Email, Phone, Currency, Price, Html, Script, Conditions, DocumentId, SysClassName, DomainId, Duration, Json, Long, Choice, Other`

Additional FieldType variants:

| Variant | Description |
|---|---|
| `Duration` | Duration/timer fields (`glide_duration`, `timer`). Value is an epoch-offset datetime like `"1970-01-05 11:00:11"` representing a time span. |
| `Json` | JSON-typed fields storing structured data as a JSON string. |
| `Long` | 64-bit integer fields (`longint`, `auto_increment`). |
| `Choice` | Choice fields with predefined values. Functionally a string with a `choices` map, but semantically distinct. |

**RelationshipType enum**: `OneToMany, ManyToOne, ManyToMany`

**SchemaOverlay**: `{ extends_release: String, tables: HashMap<String, TableOverlay> }`

**TableOverlay**: `{ label: Option<String>, extends: Option<String>, fields: HashMap<String, FieldDef>, relationships: HashMap<String, RelationshipDef> }`

---

## 4. Extending the Library

### 4.1 Add a New Auth Method

Implement the `Authenticator` trait in a new file under `src/auth/`.

```rust
// src/auth/oauth.rs
use async_trait::async_trait;
use reqwest::RequestBuilder;
use crate::error::Result;
use super::Authenticator;

#[derive(Debug)]
pub struct OAuthAuth {
    access_token: tokio::sync::RwLock<String>,
    client_id: String,
    client_secret: String,
    token_url: String,
}

#[async_trait]
impl Authenticator for OAuthAuth {
    async fn authenticate(&self, request: RequestBuilder) -> Result<RequestBuilder> {
        let token = self.access_token.read().await;
        Ok(request.header("Authorization", format!("Bearer {}", *token)))
    }

    async fn refresh(&self) -> Result<()> {
        // Implement token refresh logic here.
        // Update self.access_token via the RwLock.
        Ok(())
    }

    fn supports_session(&self) -> bool {
        false
    }

    fn method_name(&self) -> &str {
        "oauth"
    }
}
```

Then register it in `src/auth/mod.rs`:

```rust
pub mod oauth;
pub use oauth::OAuthAuth;
```

And add a match arm in `resolve_auth_from_config()` in `src/client.rs` for
config-driven auth:

```rust
"oauth" => {
    // Read oauth config fields and construct OAuthAuth
}
```

### 4.2 Add a New API Endpoint

Create a new file under `src/api/` (e.g., `src/api/cmdb.rs`).

1. Define a builder struct similar to `AggregateApi`.
2. Hold an `Arc<HttpTransport>` for HTTP calls.
3. Add a constructor method on `ServiceNowClient`.

```rust
// src/api/cmdb.rs
use std::sync::Arc;
use crate::transport::http::HttpTransport;
use crate::error::Result;

pub struct CmdbApi {
    transport: Arc<HttpTransport>,
    ci_class: String,
}

impl CmdbApi {
    pub(crate) fn new(transport: Arc<HttpTransport>, ci_class: impl Into<String>) -> Self {
        Self { transport, ci_class: ci_class.into() }
    }

    pub async fn get_ci(&self, sys_id: &str) -> Result<serde_json::Value> {
        let path = format!("/api/now/cmdb/instance/{}/{}", self.ci_class, sys_id);
        let response = self.transport.get(&path, &[]).await?;
        Ok(response.result)
    }
}
```

Then add to `ServiceNowClient`:

```rust
pub fn cmdb(&self, ci_class: &str) -> CmdbApi {
    CmdbApi::new(Arc::clone(&self.transport), ci_class)
}
```

Register in `src/api/mod.rs`:

```rust
pub mod cmdb;
```

### 4.3 Create a Schema Overlay for Custom Tables

Write a JSON file following the `SchemaOverlay` structure:

```json
{
    "extends_release": "xanadu",
    "tables": {
        "u_custom_app_request": {
            "label": "Custom App Request",
            "extends": "task",
            "fields": {
                "u_requested_app": {
                    "type": "string",
                    "max_length": 255,
                    "mandatory": true,
                    "label": "Requested Application"
                },
                "u_license_count": {
                    "type": "integer",
                    "label": "License Count"
                },
                "u_approver": {
                    "type": "reference",
                    "reference_table": "sys_user",
                    "label": "Approver"
                }
            },
            "relationships": {
                "u_app_tasks": {
                    "table": "u_app_task",
                    "foreign_key": "parent",
                    "type": "one_to_many"
                }
            }
        },
        "change_request": {
            "fields": {
                "u_change_risk_score": {
                    "type": "integer",
                    "label": "Change Risk Score"
                }
            }
        }
    }
}
```

Use it:

```rust
let client = ServiceNowClient::builder()
    .instance("mycompany")
    .auth(BasicAuth::new("admin", "password"))
    .schema_release("xanadu")
    .schema_overlay("path/to/overlay.json")
    .build()
    .await?;
```

Or apply at runtime:

```rust
use servicenow_rs::schema::{SchemaOverlay, SchemaRegistry};

let mut registry = SchemaRegistry::from_release("xanadu")?;
let overlay: SchemaOverlay = serde_json::from_str(overlay_json)?;
registry.apply_overlay(&overlay);
```

**Key rules for overlays:**

- New tables are added.
- Existing tables get fields and relationships merged (overlay wins on conflict).
- If the overlay provides `label` or `extends`, they override the base.
- Base fields not mentioned in the overlay are preserved.

### 4.4 Add a New Release Definition

1. Create `definitions/base/{release_name}.json` following the `SchemaDefinition`
   structure (see `definitions/base/xanadu.json` for the canonical example).

2. Add a match arm in `src/schema/loader.rs` function `load_bundled_definition()`:

```rust
"new_release" => include_str!("../../definitions/base/new_release.json"),
```

3. Update the error message listing available releases.

The JSON structure is:

```json
{
    "release": "new_release",
    "tables": {
        "task": {
            "label": "Task",
            "fields": {
                "sys_id": { "type": "string", "max_length": 32, "read_only": true },
                "number": { "type": "string", "max_length": 40, "read_only": true }
            },
            "relationships": {}
        },
        "incident": {
            "label": "Incident",
            "extends": "task",
            "fields": {
                "category": { "type": "string", "label": "Category" }
            },
            "relationships": {}
        }
    }
}
```

Tables that `extends` another table inherit fields and relationships from the parent
through the `SchemaRegistry` lookup methods (`field()` and `relationship()` walk the
inheritance chain).

---

## 5. Common Patterns

### 5.1 Find All Open Incidents Assigned to a Group

```rust
let incidents = client.table("incident")
    .equals("state", "1")                          // New
    .equals("assignment_group", "group_sys_id")
    .fields(&["number", "short_description", "priority", "assigned_to"])
    .order_by("priority", Order::Asc)
    .limit(100)
    .execute()
    .await?;

println!("Found {} open incidents", incidents.len());
for record in &incidents {
    println!("  {} [P{}]: {}",
        record.get_str("number").unwrap_or("?"),
        record.get_str("priority").unwrap_or("?"),
        record.get_str("short_description").unwrap_or("?"));
}
```

### 5.2 Get a Change Request with All Its Tasks and Approvals

```rust
let change = client.table("change_request")
    .include_related(&["change_task", "approvals"])
    .display_value(DisplayValue::Both)
    .get("change_sys_id")
    .await?;

println!("Change: {} - {}",
    change.get_str("number").unwrap_or("?"),
    change.get_display("state").unwrap_or("?"));

println!("Tasks:");
for task in change.related("change_task") {
    println!("  {} [{}]: {}",
        task.get_str("number").unwrap_or("?"),
        task.get_display("state").unwrap_or("?"),
        task.get_str("short_description").unwrap_or("?"));
}

println!("Approvals:");
for approval in change.related("approvals") {
    println!("  {}: {}",
        approval.get_str("approver").unwrap_or("?"),
        approval.get_display("state").unwrap_or("?"));
}
```

### 5.3 Count Incidents by State

```rust
let stats = client.aggregate("incident")
    .count()
    .group_by("state")
    .display_value(true)   // get state labels instead of numbers
    .execute()
    .await?;

for group in stats.groups() {
    println!("{}: {} incidents", group.field_value("state"), group.count());
}
```

### 5.4 Update an Incident's State

```rust
let updated = client.table("incident")
    .update("incident_sys_id", serde_json::json!({
        "state": "2",
        "work_notes": "Moving to In Progress"
    }))
    .await?;

println!("Updated {} to state {}",
    updated.get_str("number").unwrap_or("?"),
    updated.get_str("state").unwrap_or("?"));
```

### 5.5 Paginate Through All Records Matching a Filter

```rust
let mut paginator = client.table("incident")
    .equals("active", "true")
    .contains("short_description", "network")
    .fields(&["number", "short_description", "state"])
    .order_by("sys_created_on", Order::Desc)
    .limit(200)   // 200 per page
    .paginate();

let mut total_processed = 0;
while let Some(page) = paginator.next_page().await? {
    for record in &page {
        // Process each record
        total_processed += 1;
    }
    println!("Processed {} / {:?} records",
        total_processed,
        paginator.total_count());
}
```

### 5.6 Read Notes and Comments from a Record

Journal fields (`work_notes`, `comments`) are write-only: you can POST/PATCH values
into them, but GET always returns an empty string. There are two approaches to read
the actual entries.

**Approach 1: display_value=all on the parent record (reliable, concatenated string)**

This returns the full journal history as a single concatenated string in the display
value. It always works and does not require extra permissions, but the result is not
structured (entries are separated by newlines with timestamps).

```rust
let record = client.table("incident")
    .fields(&["number", "work_notes", "comments"])
    .display_value(DisplayValue::Both)
    .get("incident_sys_id")
    .await?;

// raw_str() will be empty; display_str() has the concatenated history
let notes = record.get_display("work_notes").unwrap_or("(none)");
let comments = record.get_display("comments").unwrap_or("(none)");
println!("Work notes:\n{}", notes);
println!("Customer comments:\n{}", comments);
```

**Approach 2: query sys_journal_field directly (structured, may be ACL-blocked)**

The `sys_journal_field` table stores each journal entry as a separate record. This
gives you structured data (author, timestamp, body) but may be blocked by ACLs on
some instances.

```rust
// All journal entries for a specific incident
let entries = client.table("sys_journal_field")
    .equals("element_id", "incident_sys_id")   // sys_id of the parent record
    .equals("name", "incident")                 // parent table name
    .fields(&["element", "value", "sys_created_on", "sys_created_by"])
    .order_by("sys_created_on", Order::Desc)
    .limit(100)
    .execute()
    .await?;

for entry in &entries {
    let field = entry.get_str("element").unwrap_or("?");       // "comments" or "work_notes"
    let author = entry.get_str("sys_created_by").unwrap_or("?");
    let when = entry.get_str("sys_created_on").unwrap_or("?");
    let body = entry.get_str("value").unwrap_or("");
    println!("[{}] {} by {}: {}", when, field, author, body);
}
```

**Separating public comments from private work notes:**

```rust
// Public comments only (visible to the caller/customer)
let public = client.table("sys_journal_field")
    .equals("element_id", "incident_sys_id")
    .equals("name", "incident")
    .equals("element", "comments")
    .order_by("sys_created_on", Order::Desc)
    .execute()
    .await?;

// Private work notes only (internal to the team)
let private = client.table("sys_journal_field")
    .equals("element_id", "incident_sys_id")
    .equals("name", "incident")
    .equals("element", "work_notes")
    .order_by("sys_created_on", Order::Desc)
    .execute()
    .await?;
```

### 5.7 Check Which Fields Are Writable Before Creating a Record

Use `writable_fields()` and `mandatory_fields()` from the schema registry to
discover which fields you can set and which ones you must provide.

```rust
let schema = client.schema().expect("schema must be loaded");

// All fields that can be set via POST/PATCH
let writable = schema.writable_fields("incident");
println!("Writable fields on incident:");
for (name, field_def) in &writable {
    println!("  {} ({:?})", name, field_def.field_type);
}

// Fields that must be provided on create
let required = schema.mandatory_fields("incident");
println!("\nMandatory fields on incident:");
for (name, field_def) in &required {
    println!("  {} ({:?})", name, field_def.field_type);
}

// Journal fields (write-only -- can set but will read back empty)
let journals = schema.journal_fields("incident");
println!("\nJournal (write-only) fields:");
for (name, _) in &journals {
    println!("  {}", name);
}

// Example: build a create payload ensuring all mandatory fields are present
let mandatory_names: Vec<&str> = required.iter().map(|(n, _)| *n).collect();
println!("\nBefore creating, ensure you provide: {:?}", mandatory_names);

let record = client.table("incident")
    .create(serde_json::json!({
        "short_description": "New issue",       // mandatory
        "caller_id": "some_user_sys_id",        // mandatory
        "urgency": "2",
        "work_notes": "Created by automation"   // write-only journal field
    }))
    .await?;
```

---

## 6. Error Handling

### 6.1 Error Variants and When They Occur

| Variant | When | What to do |
|---|---|---|
| `Error::Http(e)` | Network failure, DNS, TLS | Check connectivity. Retried automatically. |
| `Error::Auth { message, status }` | 401/403 response | Check credentials. `refresh()` is tried first. |
| `Error::Api { status, message, detail }` | 4xx/5xx with error body | Read message. 404 = record not found. |
| `Error::RateLimited { retry_after }` | 429 response | Retried automatically with backoff. Reduce rate_limit. |
| `Error::Schema(msg)` | Bad schema JSON, missing release | Check schema file or release name. |
| `Error::Config(msg)` | Missing required config | Set the missing value via builder, env, or TOML. |
| `Error::Query(msg)` | Invalid query construction | Fix the query parameters. |
| `Error::Json(e)` | Response body not valid JSON | Likely a ServiceNow error page (HTML). |
| `Error::UrlParse(e)` | Malformed instance URL | Check the instance name/URL. |
| `Error::Io(e)` | File read failure | Check file path and permissions. |
| `Error::Toml(e)` | Config file parse failure | Fix TOML syntax. |
| `Error::PartialResult { succeeded, failed, errors }` | Some sub-requests failed | Records are still returned. Check `.errors`. |

### 6.2 Handling Partial Failures

`QueryResult.errors` may be non-empty even when records are returned. This happens
when the main query succeeds but a related-record fetch fails.

```rust
let result = client.table("change_request")
    .include_related(&["change_task", "approvals"])
    .execute()
    .await?;

if result.has_errors() {
    for error in &result.errors {
        eprintln!("Partial failure: {}", error);
    }
}

// Records from the main query are still available:
for record in &result {
    // ...
}
```

### 6.3 Retry Behavior

The transport layer retries automatically for:

- HTTP status codes: 429, 500, 502, 503, 504
- Network errors (reqwest::Error)
- Auth failures (401) -- triggers `refresh()` first

Retry uses exponential backoff: 500ms, 1s, 2s, ... capped at 30s.
The 429 handler respects the `Retry-After` header if present.

Default: 3 retries. Override with `.max_retries(n)` on the builder.

---

## 7. Configuration Precedence

Configuration is layered. Higher layers override lower layers for the same setting.

```
Priority (highest to lowest):
  1. Builder methods   -- .instance(), .auth(), .max_retries(), etc.
  2. Environment vars  -- SERVICENOW_INSTANCE, SERVICENOW_USERNAME, etc.
  3. Config file       -- servicenow.toml
  4. Defaults          -- timeout=30s, max_retries=3, display_value=Raw, etc.
```

### 7.1 Environment Variables

| Variable | Description |
|---|---|
| `SERVICENOW_INSTANCE` | Instance URL or short name |
| `SERVICENOW_USERNAME` | Basic auth username |
| `SERVICENOW_PASSWORD` | Basic auth password |
| `SERVICENOW_API_TOKEN` | Bearer/token auth |
| `SERVICENOW_OAUTH_CLIENT_ID` | OAuth client ID |
| `SERVICENOW_OAUTH_CLIENT_SECRET` | OAuth client secret |
| `SERVICENOW_SCHEMA_PATH` | Path to schema overlay file |

### 7.2 TOML Config File

Default location: `servicenow.toml` in the working directory.

```toml
[instance]
url = "mycompany"

[auth]
method = "basic"     # "basic", "token", or "bearer"
username = "admin"
password = "secret"
# token = "my-api-token"   # for token auth

[schema]
release = "xanadu"
# overlay = "path/to/overlay.json"

[transport]
timeout_secs = 30
max_retries = 3
rate_limit = 20
```

### 7.3 Instance URL Normalization

The library normalizes instance identifiers automatically:

| Input | Resolved URL |
|---|---|
| `"mycompany"` | `https://mycompany.service-now.com` |
| `"servicenow.mycompany.com"` | `https://servicenow.mycompany.com` |
| `"https://mycompany.service-now.com/"` | `https://mycompany.service-now.com` |

---

## 8. Schema System

### 8.1 How Definitions Work

Schema definitions describe ServiceNow tables, their fields, and their relationships.
They are JSON files compiled into the binary at build time.

Available bundled releases: **xanadu**, **yokohama**, **washington**.

The schema is optional. Without it:
- Table queries, CRUD, aggregates, and pagination all work normally.
- `include_related()` will not work (returns `Error::Schema`).
- Dot-walking still works (it is a ServiceNow API feature, not schema-dependent).

### 8.2 Inheritance

Tables can extend other tables via the `extends` field. For example:

```
task (base)
  +-- incident (extends task)
  +-- change_request (extends task)
  +-- change_task (extends task)
```

When you call `registry.field("incident", "number")`, the registry first checks
`incident`'s own fields, then walks up to `task` and checks there. The same
inheritance applies to `registry.relationship()`.

### 8.3 Overlays

Overlays let you extend a base release definition with custom tables and fields
without modifying the bundled definitions.

**Merge rules:**

- Fields in the overlay are added to or replace fields in the base table.
- Relationships in the overlay are added to or replace relationships in the base.
- New tables in the overlay are added to the schema.
- `label` and `extends` in a `TableOverlay` override the base only if present
  (they are `Option`).
- Fields in the base that are not mentioned in the overlay are untouched.

**Overlay JSON structure:**

```json
{
    "extends_release": "xanadu",
    "tables": {
        "existing_table": {
            "fields": {
                "u_new_field": { "type": "string", "label": "New Field" }
            }
        },
        "u_entirely_new_table": {
            "label": "My Custom Table",
            "extends": "task",
            "fields": { ... },
            "relationships": { ... }
        }
    }
}
```

### 8.4 Loading Schemas

```rust
// At client build time (most common):
let client = ServiceNowClient::builder()
    .schema_release("xanadu")
    .schema_overlay("my_overlay.json")
    .build().await?;

// Standalone registry:
let registry = SchemaRegistry::from_release("xanadu")?;

// With overlay from string:
let registry = SchemaRegistry::from_release_with_overlay_str("xanadu", json_str)?;

// Querying the registry:
let table = registry.table("incident").unwrap();
println!("Label: {}", table.label);
println!("Extends: {:?}", table.extends);

let field = registry.field("incident", "priority").unwrap();
println!("Type: {:?}", field.field_type);
println!("Choices: {:?}", field.choices);

let rels = registry.relationships("change_request");
for (name, def) in &rels {
    println!("Relationship '{}': {} via {}", name, def.table, def.foreign_key);
}
```

---

## Appendix: Project File Map

```
src/
  lib.rs              -- Module declarations, prelude re-exports
  client.rs           -- ServiceNowClient, ClientBuilder, resolve_auth_from_config
  config.rs           -- Config (TOML), env vars, normalize_instance_url
  error.rs            -- Error enum, Result type alias
  auth/
    mod.rs            -- Authenticator trait
    basic.rs          -- BasicAuth
    token.rs          -- TokenAuth
  transport/
    mod.rs            -- Re-exports
    http.rs           -- HttpTransport (reqwest wrapper with retry)
    retry.rs          -- RetryConfig, RateLimiter
    response.rs       -- ServiceNowResponse, PaginationLinks, parse_response
  schema/
    mod.rs            -- Re-exports
    definition.rs     -- SchemaDefinition, TableDef, FieldDef, RelationshipDef, etc.
    registry.rs       -- SchemaRegistry (lookup with inheritance)
    loader.rs         -- load_bundled_definition, load_overlay, merge_overlay
  query/
    mod.rs            -- Re-exports
    builder.rs        -- TableApi (query builder + CRUD terminal ops)
    filter.rs         -- Operator, Order, Filter, Condition, encode_query
    paginator.rs      -- Paginator, PaginationConfig
    strategy.rs       -- FetchStrategy enum
    batch.rs          -- fetch_related_concurrent
  model/
    mod.rs            -- Re-exports
    record.rs         -- Record
    value.rs          -- FieldValue, DisplayValue, parse_field_value
  api/
    mod.rs            -- Re-exports
    table.rs          -- TABLE_API_PATH, DEFAULT_LIMIT, MAX_LIMIT constants
    aggregate.rs      -- AggregateApi, AggregateResult, AggregateGroup
definitions/
  base/
    xanadu.json       -- Xanadu release schema
    yokohama.json     -- Yokohama release schema
    washington.json   -- Washington release schema
```
