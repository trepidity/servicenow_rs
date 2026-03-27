# Developer Guide

This guide covers the internals of `servicenow_rs` for contributors and developers building on top of the library.

## Architecture Overview

### Module Structure

```
src/
  lib.rs              Crate root. Module declarations and prelude re-exports.
  client.rs           ServiceNowClient and ClientBuilder.
  config.rs           Config struct (TOML deserialization), env var application,
                      normalize_instance_url().
  error.rs            Error enum (thiserror) and Result type alias.

  auth/
    mod.rs            Authenticator trait definition.
    basic.rs          BasicAuth (username/password, Base64 encoding).
    token.rs          TokenAuth (Bearer token, custom header).

  transport/
    mod.rs            Re-exports.
    http.rs           HttpTransport -- reqwest wrapper with retry loop.
    retry.rs          RetryConfig (exponential backoff), RateLimiter (token bucket).
    response.rs       ServiceNowResponse parsing, PaginationLinks, Link header parser.

  schema/
    mod.rs            Re-exports.
    definition.rs     SchemaDefinition, TableDef, FieldDef, RelationshipDef,
                      SchemaOverlay, TableOverlay, FieldType, RelationshipType.
    registry.rs       SchemaRegistry -- runtime lookup with inheritance walking.
    loader.rs         JSON loading, overlay merging, bundled release loading
                      via include_str!.

  query/
    mod.rs            Re-exports.
    builder.rs        TableApi -- query builder with filter/config/terminal methods.
    filter.rs         Operator, Order, Filter, Condition, Joiner, encode_query().
    paginator.rs      Paginator -- page-by-page iteration state machine.
    strategy.rs       FetchStrategy enum (Auto, DotWalk, Concurrent).
    batch.rs          fetch_related_concurrent() -- parallel relationship fetching.

  model/
    mod.rs            Re-exports.
    record.rs         Record -- field storage, related records, dot-walk helpers.
    value.rs          FieldValue, DisplayValue, parse_field_value().

  api/
    mod.rs            Sub-module declarations.
    table.rs          Table API constants (paths, limits).
    aggregate.rs      AggregateApi builder, AggregateResult, AggregateGroup,
                      response parsing.

definitions/
  base/
    xanadu.json       Bundled schema for the Xanadu release.
    yokohama.json     Bundled schema for the Yokohama release.
    washington.json   Bundled schema for the Washington release.

tests/
  integration_test.rs   Wiremock-based integration tests.
  live_readonly_test.rs Read-only tests against a live instance (gated by env var).
```

### Data Flow

A typical query follows this path:

```
ServiceNowClient::table("incident")
  -> TableApi (builder, accumulates filters/config)
  -> .execute()
     -> build_params() assembles sysparm_query, sysparm_fields, etc.
     -> HttpTransport::get(path, params)
        -> RateLimiter::acquire() if configured
        -> Authenticator::authenticate(request) adds auth header
        -> reqwest sends the HTTP request
        -> On failure: retry loop with exponential backoff
        -> response::parse_response() extracts result, total_count, links
     -> Record::from_json() parses each element of the result array
     -> fetch_related() if include_related was called
        -> SchemaRegistry resolves relationship definitions
        -> batch::fetch_related_concurrent() fires parallel requests
        -> Results distributed back to parent records via sys_id matching
  -> QueryResult { records, total_count, errors }
```

For pagination, `TableApi::paginate()` returns a `Paginator` that manages its own offset state and calls `HttpTransport::get()` for each page.

## Adding New Authentication Methods

To add a new authentication strategy (e.g., OAuth2, mutual TLS, API key), implement the `Authenticator` trait defined in `src/auth/mod.rs`:

```rust
#[async_trait]
pub trait Authenticator: Send + Sync + std::fmt::Debug {
    /// Apply credentials to an outgoing request.
    async fn authenticate(&self, request: RequestBuilder) -> Result<RequestBuilder>;

    /// Refresh credentials if expired. Default is a no-op.
    async fn refresh(&self) -> Result<()> {
        Ok(())
    }

    /// Whether this authenticator supports session cookie reuse.
    fn supports_session(&self) -> bool {
        false
    }

    /// A human-readable name for this auth method (for logging).
    fn method_name(&self) -> &str;
}
```

### Steps

1. Create a new file `src/auth/myauth.rs`.

2. Define your struct and implement `Authenticator`:

```rust
use async_trait::async_trait;
use reqwest::RequestBuilder;
use crate::error::Result;
use super::Authenticator;

#[derive(Debug)]
pub struct OAuth2Auth {
    access_token: tokio::sync::RwLock<String>,
    client_id: String,
    client_secret: String,
    token_url: String,
}

#[async_trait]
impl Authenticator for OAuth2Auth {
    async fn authenticate(&self, request: RequestBuilder) -> Result<RequestBuilder> {
        let token = self.access_token.read().await;
        Ok(request.header("Authorization", format!("Bearer {}", *token)))
    }

    async fn refresh(&self) -> Result<()> {
        // Call the token endpoint, update self.access_token
        let mut token = self.access_token.write().await;
        *token = "new_token".to_string(); // replace with actual refresh logic
        Ok(())
    }

    fn method_name(&self) -> &str {
        "oauth2"
    }
}
```

3. Register it in `src/auth/mod.rs`:

```rust
pub mod myauth;
pub use myauth::OAuth2Auth;
```

4. Add resolution logic in `client.rs` inside `resolve_auth_from_config()` to support the new method from config/env.

5. Re-export from the prelude in `src/lib.rs` if it should be a first-class auth method.

Key considerations:

- The `authenticate` method is called on every request. The transport layer calls `refresh()` automatically on 401 responses before retrying.
- Use interior mutability (`RwLock`, `Mutex`) for mutable state since `Authenticator` requires `Send + Sync`.
- The `supports_session` return value controls whether reqwest enables its cookie store. BasicAuth returns `true`; token-based methods typically return `false`.
- The `method_name` string is included in the User-Agent header.

## Adding New API Endpoints

The Aggregate API (`src/api/aggregate.rs`) is the reference pattern for adding new ServiceNow API endpoints. Follow this structure.

### Pattern: New API Builder

1. Create `src/api/myapi.rs` with a builder struct:

```rust
use std::sync::Arc;
use crate::error::Result;
use crate::transport::http::HttpTransport;

pub struct MyApi {
    transport: Arc<HttpTransport>,
    table: String,
    // ... builder fields
}

impl MyApi {
    pub(crate) fn new(transport: Arc<HttpTransport>, table: impl Into<String>) -> Self {
        Self {
            transport,
            table: table.into(),
            // ... defaults
        }
    }

    // Builder methods return Self
    pub fn some_option(mut self, value: &str) -> Self {
        // set option
        self
    }

    // Terminal method consumes self
    pub async fn execute(self) -> Result<MyResult> {
        let path = format!("/api/now/my_endpoint/{}", self.table);
        let params = self.build_params();
        let response = self.transport.get(&path, &params).await?;
        // parse response into MyResult
        todo!()
    }

    fn build_params(&self) -> Vec<(String, String)> {
        let mut params = Vec::new();
        // build query parameters
        params
    }
}
```

2. Add an entry point on `ServiceNowClient` in `src/client.rs`:

```rust
pub fn my_api(&self, table: &str) -> MyApi {
    MyApi::new(Arc::clone(&self.transport), table)
}
```

3. Register the module in `src/api/mod.rs`:

```rust
pub mod myapi;
```

4. Re-export from the prelude if appropriate.

### How AggregateApi Works (Reference)

The `AggregateApi` in `src/api/aggregate.rs` demonstrates the pattern:

- **Builder fields** accumulate state: `do_count`, `avg_fields`, `sum_fields`, `group_by_fields`, `conditions`, etc.
- **Builder methods** return `Self` for chaining: `.count()`, `.avg("field")`, `.group_by("field")`, `.filter(...)`.
- **`build_params()`** converts accumulated state into `Vec<(String, String)>` query parameters matching the ServiceNow Stats API parameter names (`sysparm_count`, `sysparm_avg_fields`, `sysparm_group_by`, etc.).
- **`execute()`** calls `self.transport.get()` with the stats API path and parses the response into `AggregateResult`.
- **Result types** (`AggregateResult`, `AggregateGroup`) provide typed accessors like `.count()`, `.avg("field")`, `.groups()`.

## Schema Definition Format

Schema definitions are JSON files with this structure:

```json
{
  "release": "xanadu",
  "tables": {
    "task": {
      "label": "Task",
      "fields": {
        "sys_id": {
          "type": "string",
          "max_length": 32,
          "read_only": true,
          "label": "Sys ID"
        },
        "state": {
          "type": "integer",
          "label": "State",
          "choices": { "1": "New", "2": "In Progress", "7": "Closed" }
        },
        "assigned_to": {
          "type": "reference",
          "reference_table": "sys_user",
          "label": "Assigned to"
        }
      },
      "relationships": {}
    },
    "incident": {
      "label": "Incident",
      "extends": "task",
      "fields": {
        "caller_id": {
          "type": "reference",
          "reference_table": "sys_user",
          "label": "Caller"
        }
      },
      "relationships": {
        "child_incidents": {
          "table": "incident",
          "foreign_key": "parent_incident",
          "type": "one_to_many"
        }
      }
    }
  }
}
```

### Field Types

The `FieldType` enum in `src/schema/definition.rs` supports:

| JSON value | Rust variant | Notes |
|---|---|---|
| `"string"` | `FieldType::String` | |
| `"integer"` | `FieldType::Integer` | |
| `"boolean"` | `FieldType::Boolean` | |
| `"decimal"` | `FieldType::Decimal` | |
| `"float"` | `FieldType::Float` | |
| `"date_time"` | `FieldType::DateTime` | |
| `"date"` | `FieldType::Date` | |
| `"time"` | `FieldType::Time` | |
| `"reference"` | `FieldType::Reference` | Requires `reference_table` |
| `"journal"` | `FieldType::Journal` | Read-only audit log |
| `"journal_input"` | `FieldType::JournalInput` | Work notes, comments |
| `"glide_list"` | `FieldType::GlideList` | Comma-separated sys_ids |
| `"url"` | `FieldType::Url` | |
| `"email"` | `FieldType::Email` | |
| `"phone"` | `FieldType::Phone` | |
| `"currency"` | `FieldType::Currency` | |
| `"price"` | `FieldType::Price` | |
| `"html"` | `FieldType::Html` | |
| `"script"` | `FieldType::Script` | |
| `"conditions"` | `FieldType::Conditions` | Encoded query conditions |
| `"document_id"` | `FieldType::DocumentId` | |
| `"sys_class_name"` | `FieldType::SysClassName` | |
| `"domain_id"` | `FieldType::DomainId` | |
| `"other"` | `FieldType::Other` | Fallback |

### Relationship Types

The `RelationshipType` enum:

| JSON value | Rust variant | Meaning |
|---|---|---|
| `"one_to_many"` | `OneToMany` | Parent has many children (e.g., Change -> Change Tasks) |
| `"many_to_one"` | `ManyToOne` | Many records reference one parent |
| `"many_to_many"` | `ManyToMany` | M2M via an intermediate table |

### Relationship Definition Fields

```json
{
  "table": "change_task",
  "foreign_key": "change_request",
  "type": "one_to_many",
  "filter": "active=true"
}
```

- `table` -- the related table to query.
- `foreign_key` -- the field on the related table that holds the parent's sys_id.
- `type` -- the cardinality.
- `filter` (optional) -- additional encoded query appended when fetching related records (e.g., only active tasks).

### Table Inheritance

The `extends` field on `TableDef` declares that a table inherits fields and relationships from a parent. For example, `incident` extends `task`, so looking up `registry.field("incident", "number")` will walk from `incident` to `task` and find the `number` field defined on `task`.

The `SchemaRegistry` methods `field()`, `relationship()`, and `relationships()` all walk this inheritance chain automatically.

## Schema Field Attributes and Querying

### Field-Level Boolean Attributes

Each `FieldDef` in the schema carries three boolean attributes that control how a field can be used:

| Attribute | Default | Purpose |
|---|---|---|
| `read_only` | `false` | Field is system-generated and cannot be set via the API. Examples: `sys_id`, `sys_created_on`, `number`, `approval_history`. The API silently ignores values provided for these fields on POST/PATCH. |
| `mandatory` | `false` | Field is required when creating a record. A create request that omits a mandatory field will be rejected by ServiceNow with a validation error. Examples vary by table and instance configuration. |
| `write_only` | `false` | Field accepts input on POST/PATCH but always returns an empty string on GET. This applies to journal-type fields like `work_notes` and `comments`. The field value is not stored on the record itself; ServiceNow writes it to the `sys_journal_field` table as an individual journal entry. |

These attributes appear in the JSON schema definition files:

```json
{
  "sys_id": { "type": "string", "max_length": 32, "read_only": true, "label": "Sys ID" },
  "short_description": { "type": "string", "mandatory": true, "label": "Short description" },
  "work_notes": { "type": "journal_input", "write_only": true, "label": "Work notes (private)" }
}
```

### FieldDef Helper Methods

The `FieldDef` struct in `src/schema/definition.rs` provides convenience methods for checking field characteristics:

- **`is_writable()`** -- returns `true` if the field is not `read_only`. Note that write-only fields (journals) are considered writable because they accept POST/PATCH input.
- **`is_journal()`** -- returns `true` if the field type is `Journal` or `JournalInput`. Use this to identify fields that behave as audit logs or note streams rather than normal data fields.
- **`is_reference()`** -- returns `true` if the field type is `Reference` and a `reference_table` is set. Reference fields store a sys_id pointing to a record in another table.

```rust
let field = registry.field("incident", "work_notes").unwrap();
assert!(field.is_writable());   // true -- journals accept writes
assert!(field.is_journal());    // true -- journal_input type
assert!(field.write_only);      // true -- GET always returns ""

let sys_id = registry.field("incident", "sys_id").unwrap();
assert!(!sys_id.is_writable()); // false -- read_only
assert!(!sys_id.is_journal());  // false -- string type

let assigned = registry.field("incident", "assigned_to").unwrap();
assert!(assigned.is_reference());
assert_eq!(assigned.reference_table.as_deref(), Some("sys_user"));
```

### SchemaRegistry Field Query Methods

`SchemaRegistry` provides several methods that return filtered views of a table's fields. All of them walk the inheritance chain, so querying `incident` includes fields inherited from `task`.

**`all_fields(table)`** -- returns every field defined on the table and its ancestors as a `Vec<(&str, &FieldDef)>`.

**`writable_fields(table)`** -- returns only fields where `is_writable()` is true. Filters out system fields like `sys_id`, `sys_created_on`, and `number`. Useful for building create/update payloads.

**`read_only_fields(table)`** -- returns only fields where `read_only` is true.

**`mandatory_fields(table)`** -- returns only fields where `mandatory` is true. Use this to validate that all required fields are present before calling `create()`.

**`journal_fields(table)`** -- returns only fields where `is_journal()` is true. For `incident`, this includes `work_notes`, `comments`, `comments_and_work_notes`, and `approval_history`.

```rust
let registry = client.schema().expect("schema not loaded");

// List all writable fields for building an update form.
let writable = registry.writable_fields("incident");
println!("Writable fields on incident:");
for (name, def) in &writable {
    println!("  {} ({:?})", name, def.field_type);
}

// Validate mandatory fields before creating a record.
let mandatory = registry.mandatory_fields("incident");
let user_payload: HashMap<String, String> = get_user_input();
for (name, _) in &mandatory {
    if !user_payload.contains_key(*name) {
        eprintln!("Missing required field: {}", name);
    }
}

// Find all journal fields.
let journals = registry.journal_fields("incident");
let names: Vec<&str> = journals.iter().map(|(n, _)| *n).collect();
// ["work_notes", "comments", "comments_and_work_notes", "approval_history"]
```

### New FieldType Variants

Four field types were added beyond the original set:

| JSON value | Rust variant | ServiceNow types | Notes |
|---|---|---|---|
| `"duration"` | `FieldType::Duration` | `glide_duration`, `timer` | Values are epoch-offset datetimes like `"1970-01-05 11:00:11"` representing a time span, not an absolute timestamp. Used for fields like `business_duration`, `calendar_duration`, and `time_worked`. Typically read-only when system-calculated. |
| `"long"` | `FieldType::Long` | `longint`, `auto_increment` | 64-bit integer fields. Used for fields like `calendar_stc` and `business_stc` (duration in seconds). |
| `"json"` | `FieldType::Json` | `json` | Fields storing structured data as a JSON string. The value is a string containing serialized JSON, not a parsed object. |
| `"choice"` | `FieldType::Choice` | `choice` | Fields with predefined value sets. Functionally a string with a `choices` map, but semantically distinct from a plain string. The `choices` map on the `FieldDef` provides the value-to-label mapping. |

### Filtering Writable Fields for a Create/Update Payload

When building a create or update payload, filter the schema to exclude fields the API will reject or ignore:

```rust
let registry = client.schema().expect("schema not loaded");

// Get writable fields (excludes read_only system fields).
let writable = registry.writable_fields("change_request");

// For a create payload, separate journal fields from regular fields.
// Journal fields (work_notes, comments) can be included in a POST body,
// but they behave differently: the value is written to sys_journal_field
// rather than stored on the record.
let mut payload = serde_json::Map::new();
for (name, def) in &writable {
    if let Some(user_value) = user_input.get(*name) {
        payload.insert(name.to_string(), json!(user_value));
    }
}

// Validate mandatory fields are present.
for (name, _) in registry.mandatory_fields("change_request") {
    if !payload.contains_key(name) {
        return Err(format!("Missing mandatory field: {}", name));
    }
}
```

## Creating Custom Overlays

Overlays extend a base schema without modifying the bundled definition files. The overlay format uses `SchemaOverlay`:

```json
{
  "extends_release": "xanadu",
  "tables": {
    "change_request": {
      "label": "Change Request (Custom)",
      "fields": {
        "u_environment": {
          "type": "string",
          "max_length": 40,
          "label": "Environment"
        }
      },
      "relationships": {
        "u_deployment_records": {
          "table": "u_deployment",
          "foreign_key": "change_request",
          "type": "one_to_many"
        }
      }
    },
    "u_deployment": {
      "label": "Deployment Record",
      "fields": {
        "change_request": {
          "type": "reference",
          "reference_table": "change_request",
          "label": "Change Request"
        },
        "u_target_server": {
          "type": "string",
          "max_length": 100,
          "label": "Target Server"
        }
      }
    }
  }
}
```

### Merge Behavior

The `merge_overlay` function in `src/schema/loader.rs`:

1. For existing tables: fields and relationships from the overlay are merged in (overlay wins on key conflict). If the overlay provides a `label` or `extends`, those override the base.
2. For new tables: the entire table is added to the schema. If no `label` is provided, the table name is used as the label.

Overlays are additive. They never remove fields or tables from the base.

### Loading Overlays

Three approaches:

```rust
// 1. Via ClientBuilder (from file path)
let client = ServiceNowClient::builder()
    .schema_release("xanadu")
    .schema_overlay("./overlays/my_org.json")
    .build()
    .await?;

// 2. Directly on SchemaRegistry (from file path)
let registry = SchemaRegistry::from_release_with_overlay("xanadu", Path::new("./overlay.json"))?;

// 3. Directly on SchemaRegistry (from JSON string)
let registry = SchemaRegistry::from_release_with_overlay_str("xanadu", &json_string)?;

// 4. Apply multiple overlays incrementally
let mut registry = SchemaRegistry::from_release("xanadu")?;
let overlay: SchemaOverlay = serde_json::from_str(&json_string)?;
registry.apply_overlay(&overlay);
```

## Query Builder Internals

### How Encoded Queries Are Built

ServiceNow uses a custom "encoded query" format for filtering. The `encode_query` function in `src/query/filter.rs` converts a list of `Condition` structs into this format.

Each `Condition` has:
- A `Joiner` (`And`, `Or`, `NewQuery`) that determines the separator between conditions.
- A `Filter` with `field`, `operator` (`Operator` enum), and `value`.

The encoding rules:

```
First condition:  field{operator}value
AND:              ^field{operator}value
OR:               ^ORfield{operator}value
New query:        ^NQfield{operator}value
Order by:         ^ORDERBYfield  or  ^ORDERBYDESCfield
```

Examples:
- `state=1` -- single equals
- `state=1^priority<3` -- AND
- `state=1^ORstate=2` -- OR
- `active=true^ORDERBYDESCsys_created_on` -- with ordering
- `assigned_toISEMPTY` -- isEmpty (no value part)
- `priorityIN1,2,3` -- IN list

The `TableApi` shorthand methods (`.equals()`, `.contains()`, etc.) push `Condition` structs with `Joiner::And`. The `.or_filter()` method uses `Joiner::Or`.

### Parameter Assembly

`TableApi::build_params()` assembles the full set of ServiceNow query parameters:

| Parameter | Source |
|---|---|
| `sysparm_query` | `encode_query(conditions, order_by)` |
| `sysparm_fields` | `.fields()` + `.dot_walk()` merged |
| `sysparm_display_value` | `.display_value()` -- `"false"`, `"true"`, or `"all"` |
| `sysparm_limit` | `.limit()` |
| `sysparm_offset` | `.offset()` |
| `sysparm_exclude_reference_link` | `"true"` by default |
| `sysparm_no_count` | `.no_count()` if called |

When using `FetchStrategy::DotWalk` with `include_related`, the schema is used to automatically generate dot-walked field names from the related table's field definitions.

## Journal Field Internals

Journal fields (work notes, comments) are one of the most confusing parts of the ServiceNow API. They do not behave like normal record fields. This section explains what happens at the API level and how the library models it.

### How Journal Fields Work at the API Level

ServiceNow journal fields have split read/write behavior:

1. **Write-only on the record.** A POST or PATCH body can include `work_notes` or `comments` with a text value. ServiceNow accepts the value, creates a journal entry in the `sys_journal_field` table, and returns a response where the field value is an empty string. The record itself never stores the text.

2. **GET always returns empty.** A standard GET request for an incident (or any task-based record) returns `""` for `work_notes`, `comments`, and `comments_and_work_notes`, regardless of how many journal entries exist. This is not a bug; it is by design.

3. **`display_value=all` returns a concatenated string.** When the `sysparm_display_value=all` parameter is set, the `display_value` property of a journal field contains all journal entries concatenated together as a single formatted string. This is a quick way to retrieve notes but provides no structure (no per-entry timestamps or authors).

4. **`sys_journal_field` stores individual entries.** Each journal entry is a separate record in the `sys_journal_field` table with these key fields:

   | Field | Description |
   |---|---|
   | `element_id` | The sys_id of the parent record (e.g., the incident) |
   | `name` | The table name the entry belongs to (e.g., `"incident"`, `"change_request"`) |
   | `element` | Which journal field this entry is for: `"work_notes"` or `"comments"` |
   | `value` | The text content of the journal entry |
   | `sys_created_on` | When the entry was written |
   | `sys_created_by` | Who wrote the entry |

5. **ACL restrictions commonly block `sys_journal_field` reads.** Many ServiceNow instances restrict direct queries to `sys_journal_field` via ACL rules. If the service account does not have the `itil` role or a specific ACL grant, queries against this table return 403 or an empty result set. The `display_value=all` approach works as a fallback because it goes through the parent record's access controls instead.

### Schema Representation

The schema marks journal fields with two type variants and the `write_only` attribute:

- **`journal_input`** (Rust: `FieldType::JournalInput`) -- fields that accept new entries via POST/PATCH. Examples: `work_notes`, `comments`. These are marked `write_only: true` in the schema.
- **`journal`** (Rust: `FieldType::Journal`) -- read-only aggregated journal fields. Examples: `comments_and_work_notes`, `approval_history`. These are marked `read_only: true` in the schema.

```json
"work_notes": { "type": "journal_input", "write_only": true, "label": "Work notes (private)" },
"comments": { "type": "journal_input", "write_only": true, "label": "Additional comments (public)" },
"comments_and_work_notes": { "type": "journal", "read_only": true, "label": "Comments and work notes" },
"approval_history": { "type": "journal", "read_only": true, "label": "Approval history" }
```

Use `FieldDef::is_journal()` to test for both variants, and check `write_only` or `read_only` to distinguish input fields from aggregated views.

### The work_notes Relationship on change_request

The bundled schema definitions include a relationship on `change_request` for fetching journal entries via the related record mechanism:

```json
"work_notes": {
  "table": "sys_journal_field",
  "foreign_key": "element_id",
  "filter": "name=change_request",
  "type": "one_to_many"
}
```

The `filter` value `"name=change_request"` is critical. Without it, the query against `sys_journal_field` would match journal entries for any table that happens to share a sys_id with the parent record. The filter scopes the results to entries where the `name` column equals `"change_request"`, ensuring only journal entries belonging to change requests are returned.

When using `include_related(&["work_notes"])`, the library issues a query like:

```
GET /api/now/table/sys_journal_field?sysparm_query=element_idINsys_id1,sys_id2^name=change_request
```

### Distinguishing Public from Private Notes

Journal entries use the `element` field to indicate which journal stream they belong to:

- **`element = "comments"`** -- public notes visible to the caller/requester. Referred to as "Additional comments" in the ServiceNow UI.
- **`element = "work_notes"`** -- private/internal notes visible only to the fulfillment team. Referred to as "Work notes" in the ServiceNow UI.

To separate them after querying `sys_journal_field`:

```rust
let journal = client
    .table("sys_journal_field")
    .equals("element_id", &incident_sys_id)
    .fields(&["element", "value", "sys_created_on", "sys_created_by"])
    .execute()
    .await?;

let work_notes: Vec<_> = journal
    .iter()
    .filter(|r| r.get_str("element") == Some("work_notes"))
    .collect();

let comments: Vec<_> = journal
    .iter()
    .filter(|r| r.get_str("element") == Some("comments"))
    .collect();

println!("Private (work_notes): {}", work_notes.len());
println!("Public (comments): {}", comments.len());
```

Alternatively, filter at the query level to retrieve only one type:

```rust
let private_notes = client
    .table("sys_journal_field")
    .equals("element_id", &sys_id)
    .equals("element", "work_notes")
    .fields(&["value", "sys_created_on", "sys_created_by"])
    .order_by("sys_created_on", Order::Desc)
    .limit(10)
    .execute()
    .await?;
```

### Reading Notes: Two Approaches

There are two ways to read journal entries, each with trade-offs:

**Approach 1: `display_value=all` on the parent record.** Set `sysparm_display_value=all` and read the `display_value` property of the journal field. This returns all entries concatenated into a single string. It works even when `sys_journal_field` is ACL-blocked, but the result is unstructured text with no per-entry metadata.

```rust
let result = client
    .table("incident")
    .fields(&["number", "work_notes", "comments"])
    .display_value(DisplayValue::Both)
    .get(&sys_id)
    .await?;

// display_value has the concatenated text; raw value is still "".
let notes_text = result.get_display("work_notes").unwrap_or("");
```

**Approach 2: Query `sys_journal_field` directly.** This returns individual entries with timestamps and authors, but requires that the service account has read access to `sys_journal_field`. If ACLs block it, fall back to approach 1.

```rust
let entries = client
    .table("sys_journal_field")
    .equals("element_id", &sys_id)
    .equals("element", "work_notes")
    .fields(&["value", "sys_created_on", "sys_created_by"])
    .order_by("sys_created_on", Order::Desc)
    .execute()
    .await?;
```

### work_notes_list Is Not Journal Content

The `work_notes_list` field on task-based tables is a `glide_list` (comma-separated sys_ids of `sys_user` records). It represents the subscriber list for work note notifications, not the journal content itself. Do not confuse it with `work_notes`:

| Field | Type | Purpose |
|---|---|---|
| `work_notes` | `journal_input` | Accepts new private journal entries on POST/PATCH |
| `work_notes_list` | `glide_list` | Comma-separated sys_user sys_ids subscribed to work note notifications |

## Batch/Concurrent Fetching Strategy

When `include_related` is called, the library fetches related records after the main query returns. The `fetch_related_concurrent` function in `src/query/batch.rs` implements this:

1. **Collect parent sys_ids**: All parent records' `sys_id` values are joined into a comma-separated list.

2. **Fire parallel requests**: One HTTP request per relationship, all launched concurrently via `futures::future::join_all`. Each request queries the related table with `foreign_keyINsys_id1,sys_id2,...`.

3. **Distribute results**: For each relationship's results, a HashMap (`parent_sys_id -> Vec<Record>`) is built by reading the foreign key field value from each related record. Then each parent gets its children attached via `record.set_related()`.

4. **Error handling**: If any relationship fetch fails, the error is collected but other relationships and the main records are still returned. Errors appear in `QueryResult.errors`.

The `FetchStrategy` enum controls this behavior:

- `Auto` (default) -- currently maps to `Concurrent`.
- `Concurrent` -- fire parallel HTTP requests per relationship.
- `DotWalk` -- intended to use ServiceNow dot-walking to inline related fields in a single request (currently falls back to `Concurrent`).

### Relationship Filters

Each `RelationshipDef` can include an optional `filter` string. When present, it is appended to the query with a `^` separator:

```
change_requestINsys_id1,sys_id2^active=true
```

This allows relationships to be scoped (e.g., only fetch active change tasks).

## Pagination Internals

The `Paginator` struct in `src/query/paginator.rs` manages page-by-page fetching:

### State Machine

```
new() -> Paginator { done: false, current_offset: 0 }

next_page():
  if done -> return None
  build params with sysparm_limit=page_size, sysparm_offset=current_offset
  HTTP GET
  update total_count from X-Total-Count header
  parse records
  advance current_offset by record count
  if records < page_size -> done = true
  if current_offset >= total_count -> done = true
  if records == 0 -> done = true, return None
  return Some(QueryResult)
```

### Entry Points

- `TableApi::paginate()` -- returns a `Paginator`. The builder's `limit` becomes the page size (default 100).
- `TableApi::execute_all(max_records)` -- creates a paginator internally and collects all pages, respecting the optional max.
- `Paginator::collect_all()` -- collects remaining pages from an existing paginator.

### Total Count

The `X-Total-Count` response header provides the total number of matching records. It is captured on the first response and available via `paginator.total_count()`. Use `TableApi::no_count()` to suppress this header for better performance on large tables (ServiceNow must count all matches to produce it).

## Transport Layer

### HttpTransport

`HttpTransport` in `src/transport/http.rs` wraps `reqwest::Client` and provides:

- **Method dispatch**: `get()`, `post()`, `put()`, `patch()`, `delete()` all delegate to a common `request()` method.
- **Authentication**: Every request passes through `Authenticator::authenticate()` to add auth headers.
- **JSON handling**: POST/PUT/PATCH set `Content-Type: application/json` and serialize the body. All methods set `Accept: application/json`.
- **User-Agent**: Set to `servicenow_rs/{version} ({auth_method})`.
- **Cookie store**: Enabled when the authenticator returns `supports_session() == true`.

### Retry Logic

The `request()` method implements a retry loop:

```
for attempt in 0..=max_retries:
    rate_limit.acquire()
    build request
    authenticate request
    send request
    match result:
        Ok(response):
            if 401 and can retry -> authenticator.refresh(), continue
            parse response
            if RateLimited -> sleep(retry_after or backoff), continue
            if retryable status (429, 500, 502, 503, 504) and can retry -> sleep(backoff), continue
            return parsed result or error
        Err(network_error):
            if can retry -> sleep(backoff), continue
            return error
```

### Exponential Backoff

`RetryConfig` controls the backoff parameters:

```rust
RetryConfig {
    max_retries: 3,           // default
    initial_delay: 500ms,     // default
    max_delay: 30s,           // default
    backoff_factor: 2.0,      // default
}
```

Delay for attempt N = `initial_delay * backoff_factor^N`, capped at `max_delay`.

If a `Retry-After` header is present (from a 429 response), that value is used instead of the calculated backoff.

### Rate Limiting

`RateLimiter` implements a token-bucket algorithm:

- Configured with a maximum requests-per-second value.
- `acquire()` is called before each request. If no token is available, it sleeps for `1000 / max_rps` milliseconds and rechecks.
- Tokens refill continuously based on elapsed time since last check.

Configure via the builder:

```rust
ServiceNowClient::builder()
    .rate_limit(20) // 20 requests per second
    .build()
    .await?;
```

### Response Parsing

`parse_response` in `src/transport/response.rs` handles:

1. **Auth errors** (401, 403) -- returns `Error::Auth`.
2. **Rate limiting** (429) -- returns `Error::RateLimited` with the `Retry-After` header value.
3. **Empty responses** (204, empty body) -- returns `ServiceNowResponse` with `Value::Null`.
4. **JSON parsing** -- extracts the `"result"` field from the response body.
5. **Error detection** -- if the JSON contains an `"error"` object, returns `Error::Api`.
6. **Non-2xx without error object** -- returns `Error::Api` with the status and body preview.
7. **Pagination headers** -- extracts `X-Total-Count` and parses the RFC 5988 `Link` header into `PaginationLinks`.

## Testing Approach

### Unit Tests

Each module contains `#[cfg(test)] mod tests` blocks for unit testing. Run with:

```bash
cargo test
```

Key unit test areas:
- `config.rs` -- URL normalization, TOML parsing.
- `auth/basic.rs` -- Base64 encoding, session support toggling.
- `auth/token.rs` -- Bearer header formatting, custom header support.
- `model/value.rs` -- `parse_field_value` for simple, expanded, and reference field formats.
- `model/record.rs` -- `from_json` parsing, related records, display value handling.
- `query/filter.rs` -- `encode_query` for various operator combinations and ordering.
- `transport/retry.rs` -- Backoff calculation, retryable status codes.
- `transport/response.rs` -- Error message extraction, Link header parsing.
- `schema/loader.rs` -- Overlay merging (new fields, new tables).
- `schema/registry.rs` -- Schema loading, inheritance walking, overlay application.
- `api/aggregate.rs` -- Non-grouped, grouped, and multi-stat response parsing.

### Integration Tests (Wiremock)

`tests/integration_test.rs` uses [wiremock](https://crates.io/crates/wiremock) to mock the ServiceNow API. Each test:

1. Starts a `MockServer`.
2. Configures mock responses using `Mock::given(method(...)).and(path(...))`.
3. Builds a `ServiceNowClient` pointing at the mock server's URI.
4. Executes operations and asserts on the results.

This covers:
- Simple queries, get/create/update/delete.
- Display value modes (raw, display, both).
- Field selection.
- Related record fetching with concurrent strategy.
- Complex query encoding (multiple operators, ordering).
- Error handling (404, 401).
- Pagination (multi-page fetching, execute_all, max_records cap).
- Aggregate queries (count, grouped, filtered).
- Dot-walking (flat and with display values).
- Token auth.
- Client from env vars.

Helper pattern used across all integration tests:

```rust
async fn test_client(server: &MockServer) -> ServiceNowClient {
    ServiceNowClient::builder()
        .instance(server.uri())
        .auth(BasicAuth::new("test_user", "test_pass"))
        .schema_release("xanadu")
        .build()
        .await
        .expect("failed to build test client")
}
```

### Live Read-Only Tests

`tests/live_readonly_test.rs` contains tests that run against a real ServiceNow instance. They are gated behind an environment variable:

```bash
SERVICENOW_LIVE_TEST=1 \
SERVICENOW_INSTANCE=mycompany \
SERVICENOW_USERNAME=admin \
SERVICENOW_PASSWORD=secret \
cargo test --test live_readonly_test
```

These tests only perform GET operations and verify real API responses parse correctly. They cover:
- Simple queries with field selection.
- Dot-walking (single and multi-level).
- Display value modes.
- Pagination (manual and execute_all).
- Related record fetching.
- Complex filters with ordering.
- Aggregate queries (count, grouped, filtered).
- The `count()` convenience method.

## Common Patterns and Recipes

### Conditional Queries

Build up filters conditionally:

```rust
let mut query = client.table("incident")
    .fields(&["number", "short_description", "state"]);

if let Some(state) = user_state_filter {
    query = query.equals("state", &state);
}

if let Some(keyword) = search_term {
    query = query.contains("short_description", &keyword);
}

let result = query.limit(50).execute().await?;
```

### Fetching a Known Record with Specific Fields

```rust
let record = client.table("change_request")
    .fields(&["number", "short_description", "state", "risk"])
    .display_value(DisplayValue::Both)
    .get("sys_id_here")
    .await?;

println!("Risk: {} (raw: {})",
    record.get_display("risk").unwrap_or("?"),
    record.get_raw("risk").unwrap_or("?"),
);
```

### Bulk Fetching with Safety Cap

```rust
let result = client.table("change_request")
    .equals("state", "1")
    .fields(&["number", "short_description"])
    .limit(200)            // page size
    .execute_all(Some(1000))  // stop after 1000 records
    .await?;

println!("Fetched {} of {:?} total", result.len(), result.total_count);
```

### Counting Before Fetching

```rust
let count = client.table("incident")
    .equals("state", "1")
    .count()
    .await?;

if count > 10_000 {
    println!("Too many records ({}), narrowing filter", count);
} else {
    let result = client.table("incident")
        .equals("state", "1")
        .execute_all(None)
        .await?;
}
```

### Processing Pages as They Arrive

```rust
let mut paginator = client.table("incident")
    .fields(&["number", "sys_id"])
    .limit(500)
    .paginate();

let mut processed = 0;
while let Some(page) = paginator.next_page().await? {
    for record in &page {
        // Process each record immediately without buffering all in memory
        process_record(record);
        processed += 1;
    }
    println!("Processed {}/{:?}", processed, paginator.total_count());
}
```

### Getting Aggregate Stats for a Dashboard

```rust
// Count by state
let by_state = client.aggregate("incident")
    .count()
    .group_by("state")
    .equals("active", "true")
    .display_value(true)
    .execute()
    .await?;

for group in by_state.groups() {
    println!("{}: {}", group.field_value("state"), group.count());
}

// Average priority of active incidents
let stats = client.aggregate("incident")
    .avg("priority")
    .equals("active", "true")
    .execute()
    .await?;

if let Some(avg) = stats.avg("priority") {
    println!("Average priority: {:.1}", avg);
}
```

### Handling Partial Failures Gracefully

```rust
let result = client.table("change_request")
    .include_related(&["change_task", "approvals"])
    .limit(10)
    .execute()
    .await?;

// Log partial errors but still process the main data
if result.has_errors() {
    for err in &result.errors {
        tracing::warn!("Related record fetch issue: {}", err);
    }
}

for record in &result {
    let num = record.get_str("number").unwrap_or("?");
    let tasks = record.related("change_task");
    let approvals = record.related("approvals");
    println!("{}: {} tasks, {} approvals", num, tasks.len(), approvals.len());
}
```

### Using Schema for Field Validation

```rust
let registry = client.schema().expect("schema not loaded");

let table = "change_request";
let field = "u_custom_field";

if registry.has_field(table, field) {
    let field_def = registry.field(table, field).unwrap();
    println!("Field type: {:?}", field_def.field_type);
    if field_def.read_only {
        println!("Warning: {} is read-only", field);
    }
} else {
    println!("{} not found in schema for {}", field, table);
}
```

### Checking Available Relationships

```rust
let registry = client.schema().expect("schema not loaded");

let rels = registry.relationships("change_request");
println!("Available relationships for change_request:");
for (name, def) in &rels {
    println!("  {} -> {} (via {}, {:?})",
        name, def.table, def.foreign_key, def.relationship_type);
}
```

### Skipping Count for Performance

On large tables, ServiceNow must count all matching records to populate the `X-Total-Count` header. Skip it when you don't need the total:

```rust
let result = client.table("sys_audit")
    .fields(&["sys_id", "tablename"])
    .no_count()
    .limit(100)
    .execute()
    .await?;

// result.total_count will be None
```
