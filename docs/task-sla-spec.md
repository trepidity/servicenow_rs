# Task SLA Read Helpers Specification

Status: Draft for architect and designer review

Issue: https://github.com/trepidity/servicenow_rs/issues/4

## Goal

Add a read-only Task SLA surface that answers "what is the SLA status for this
task or incident?" by reading ServiceNow's SLA Engine output from
`task_sla`. The library must not recompute SLA schedules, pause windows, or
breach calculations locally.

The design must work for a single incident lookup and for large systems that
need to inspect SLA status across many tasks without issuing one child-query
request per parent record.

## Source Of Truth

The implementation is anchored on ServiceNow's documented Task SLA model:

- `task_sla` is the runtime Task SLA table for SLAs attached to tasks.
- `contract_sla` is the SLA definition table referenced by `task_sla.sla`.
- Legacy task fields such as `sla_due`, `made_sla`, and `escalation` are not
  the primary source because one task can have multiple Task SLA rows.
- Table API field parameters require internal column names, not UI labels.

Before hardcoding any typed field mapping, implementation must capture
dictionary evidence from a real Yokohama instance:

```text
GET /api/now/table/sys_dictionary?sysparm_query=name=task_sla&sysparm_fields=element,column_label,internal_type,reference
```

Commit the evidence as `docs/verification/task_sla_dictionary_yokohama.json`.
If the dictionary cannot be captured, implementation must stop at the schema
and low-level builder helper rather than shipping guessed typed defaults.

## Non-Goals

- No writes to `task_sla` or `contract_sla`.
- No local SLA schedule, pause, timezone, or business-duration calculation.
- No reliance on legacy task SLA fields for summary status.
- No requirement that schema loading be enabled for the basic single-task
  helper.
- No full authoritative `contract_sla` model in this issue.

## Public API

### Single Task Builder

Add a builder-style helper matching existing `journal()` conventions:

```rust
impl ServiceNowClient {
    pub fn task_slas(&self, task_sys_id: &str) -> TableApi;
}
```

Default query behavior:

- Table: `task_sla`
- Filter: `task=<task_sys_id>`
- Display mode: `DisplayValue::Both`
- Reference links: excluded by default
- Field projection: only fields needed by `TaskSla`, `TaskSlaSummary`, and
  basic UI rendering
- Default order: active rows first, then unbreached rows, then planned end
  ascending where the query builder can express this safely

The helper must return `TableApi` so callers can add filters, ordering, limits,
or fields before a terminal operation. Do not set irreversible builder options
such as `no_count()` in this low-level helper unless the query builder first
gets an explicit way to undo them.

### Number Lookup

Add a typed convenience method:

```rust
impl ServiceNowClient {
    pub async fn task_slas_for_number(&self, number: &str) -> Result<Vec<TaskSla>>;
}
```

Behavior:

1. Resolve the parent table and record using existing `get_by_number()`.
2. If the record is missing, return an empty `Vec`.
3. Query Task SLA rows by the resolved parent `sys_id`.
4. Drain pagination with `execute_all(None)` or `Paginator::collect_all()`;
   do not return only the first ServiceNow page.
5. Parse every returned record into `TaskSla`, preserving the underlying
   `Record`.

The two-request shape is accepted: one lookup by number, one paginated Task SLA
query. A single public method that hides the lookup is more important than
shaving one round trip for the common single-incident workflow.

### Bulk Task Lookup

Large systems need a non-N+1 path. Add a bulk helper, or document an equivalent
supported pattern if the API is deferred:

```rust
impl ServiceNowClient {
    pub async fn task_slas_for_tasks(
        &self,
        task_sys_ids: &[&str],
    ) -> Result<HashMap<String, Vec<TaskSla>>>;
}
```

Required behavior:

- Deduplicate task ids before querying.
- Prepopulate the result map with every requested task id and an empty vector.
- Query `task_sla` with `task IN (...)` chunks instead of one request per task.
- Use a conservative chunk size to avoid URL and encoded-query limits. Start
  with 100 task ids per chunk unless test evidence supports a larger default.
- Use field projection and `DisplayValue::Both`.
- Use `no_count()` for the bulk helper because it drains result pages by link
  and short-page behavior and does not need ServiceNow's total count.
- Drain pagination for each chunk.
- Group records by the raw `task` sys_id, not by display value.
- Preserve partial progress if one chunk fails only if the crate gains a
  structured partial-result type for this helper; otherwise fail the whole
  call clearly.

`task_slas_for_number()` may internally call `task_slas_for_tasks(&[sys_id])`
after resolving the parent record.

### Relationship Traversal

Update bundled schemas so relationship traversal works from any task subclass:

```json
"task_sla": {
  "table": "task_sla",
  "foreign_key": "task",
  "type": "one_to_many"
}
```

The relationship belongs on `task`, not separately on `incident`,
`change_request`, `problem`, or service catalog task tables. The registry
already walks `extends`, so a single task-level relationship is inherited.

Large-system guidance:

- `include_related(&["task_sla"])` is acceptable for page-sized parent queries.
- For full-table scans or reports, prefer `task_slas_for_tasks()` over repeated
  `task_slas_for_number()` calls.
- Documentation must warn users to page parent task queries and cap report
  sizes intentionally.

## Typed Model

Add `src/model/sla.rs` and re-export it from `src/model/mod.rs` and the prelude.

```rust
pub struct TaskSla {
    pub record: Record,
    pub sys_id: String,
    pub task_sys_id: Option<String>,
    pub sla_sys_id: Option<String>,
    pub sla_name: Option<String>,
    pub stage: Option<TaskSlaStage>,
    pub active: Option<bool>,
    pub has_breached: Option<bool>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub planned_end_time: Option<String>,
    pub original_breach_time: Option<String>,
    pub actual_elapsed_percentage: Option<f64>,
    pub actual_time_left: Option<String>,
    pub business_elapsed_percentage: Option<f64>,
    pub business_time_left: Option<String>,
    pub business_duration: Option<String>,
    pub duration: Option<String>,
    pub schedule_sys_id: Option<String>,
}
```

`TaskSla` field names are Rust-facing names. The ServiceNow source column for
each field must come from the committed dictionary evidence.

Expected mappings to verify:

| Rust field | Candidate internal column |
|---|---|
| `task_sys_id` | `task` |
| `sla_sys_id` | `sla` |
| `sla_name` | display value of `sla` |
| `stage` | `stage` |
| `active` | `active` |
| `has_breached` | `has_breached` |
| `start_time` | `start_time` |
| `end_time` | `end_time` |
| `planned_end_time` | `planned_end_time` |
| `original_breach_time` | `original_breach_time` |
| `actual_elapsed_percentage` | `percentage` |
| `actual_time_left` | `time_left` |
| `business_elapsed_percentage` | `business_percentage` |
| `business_time_left` | `business_time_left` |
| `business_duration` | `business_duration` |
| `duration` | `duration` |
| `schedule_sys_id` | `schedule` |

If a field is absent or has an unexpected shape, parsing must return `None` for
that typed property and keep the raw `Record` available.

### Stage Values

`stage` is not a closed enum. Use a permissive enum:

```rust
pub enum TaskSlaStage {
    InProgress,
    Paused,
    Completed,
    Cancelled,
    Other(String),
}
```

Parsing must preserve unknown customer-defined values in `Other(String)`.

## Summary Helper

Add a pure Rust summary:

```rust
pub struct TaskSlaSummary {
    pub total: usize,
    pub active: usize,
    pub breached: usize,
    pub next_breach: Option<TaskSla>,
    pub highest_business_elapsed_percentage: Option<f64>,
}
```

Semantics:

- `total`: number of Task SLA rows passed to the summary.
- `active`: rows where `active == Some(true)`.
- `breached`: rows where `has_breached == Some(true)`.
- `next_breach`: the active, unbreached row with the earliest non-empty
  `planned_end_time`. Completed, cancelled, inactive, and already breached rows
  must not mask a real upcoming breach.
- `highest_business_elapsed_percentage`: maximum parsed business percentage
  across rows with a value.

The summary must not infer missing SLA rows. If a user has no ACL access and
ServiceNow returns zero rows, the summary is empty. Documentation must explain
that "empty" can mean "no Task SLAs" or "no readable Task SLAs."

## Schema Scope

Add `task_sla` to bundled `washington`, `xanadu`, and `yokohama` definitions
with the verified field set used by `TaskSla`.

For `contract_sla`, use only a minimal schema stub in this issue:

- `sys_id`
- `name`
- `schedule`

The `task_sla.sla -> contract_sla` reference enables dot-walking and display
values without pretending the crate has a complete SLA-definition model.

Do not add a typed `ContractSla` model in this issue.

## Query Efficiency Requirements

The implementation must avoid expensive default queries:

- Always project fields for Task SLA helpers; do not fetch full rows by default.
- Use `DisplayValue::Both` only because summary parsing needs raw values and
  documentation/UI examples need display values.
- Exclude reference links by default.
- Drain pagination for typed helpers that return `Vec<TaskSla>`.
- Use chunked `IN` queries for bulk task ids.
- Do not call `get_by_number()` in a loop for report-style workflows.
- Do not call `include_related()` on unbounded parent table queries.
- Do not request total counts for bulk Task SLA reads unless a public API needs
  that number.

Suggested default field list after verification:

```text
sys_id,task,sla,stage,active,has_breached,start_time,end_time,
planned_end_time,original_breach_time,percentage,time_left,
business_percentage,business_time_left,business_duration,duration,schedule
```

## ACL And Failure Modes

`task_sla` reads can be ACL-restricted. A standard integration user may receive
zero rows without an API error. Public docs and rustdoc must mirror the warning
style already used by `journal()`:

- Empty results can mean no records or insufficient `task_sla` read access.
- Live tests should report ACL and field-access failures clearly.
- The library should not turn an empty successful response into an error.

## Documentation Requirements

Update:

- `README.md`: Quick Start example for incident SLA status.
- `src/lib.rs`: doctest showing `include_related(&["task_sla"])`.
- `docs/developer-guide.md`: implementation notes, query plan, field evidence,
  ACL caveats, and large-system bulk guidance.
- Rustdoc for `ServiceNowClient::task_slas`,
  `ServiceNowClient::task_slas_for_number`, `TaskSla`, and `TaskSlaSummary`.

Designer review should focus on whether the examples make these distinctions
clear:

- "No readable Task SLAs" is not necessarily "no SLA obligations."
- "Next breach" means next active and unbreached planned end time.
- Display values are for people; raw values drive parsing and grouping.
- Bulk/reporting examples should be visibly different from one-incident
  examples.

## Test Plan

Wiremock tests:

- `task_slas()` builds the expected `task_sla` query, field projection,
  display mode, and ordering.
- `task_slas_for_number()` resolves a number, drains multiple Task SLA pages,
  and parses typed rows.
- `task_slas_for_tasks()` chunks parent ids, drains pages for each chunk, and
  groups by raw `task`.
- Typed parsing handles `DisplayValue::Both`, raw-only values, missing fields,
  empty strings, numeric strings, and unknown `stage` choices.
- `TaskSlaSummary::next_breach` ignores inactive, breached, completed, and
  cancelled rows.
- Schema lookup proves `task_sla` on `task` is inherited by `incident`,
  `change_request`, and `problem`.

Live read-only tests:

- If credentials are present, resolve an incident sys_id and query `task_sla`.
- Report empty results with an ACL note instead of failing with an ambiguous
  assertion.
- Verify that all default fields in the typed model are accepted by the Table
  API for the configured release.

Static checks:

- `cargo fmt`
- `cargo test`
- `cargo clippy -- -D warnings`

## Acceptance Criteria

- A committed dictionary evidence file supports every default typed field.
- `task_slas(task_sys_id)` returns a customizable `TableApi` over `task_sla`.
- `task_slas_for_number(number)` returns all Task SLA rows for the resolved
  task, not just one page.
- Large-system users have a documented and tested non-N+1 path.
- Bundled schemas include `task_sla`, a minimal `contract_sla`, and a task-level
  `task_sla` relationship.
- The typed model preserves `Record` and tolerates missing or custom fields.
- `TaskSlaStage` preserves unknown values.
- `TaskSlaSummary::next_breach` is active and unbreached by definition.
- README, rustdoc, and developer guide document ACL caveats and query-scaling
  guidance.

## Open Review Questions

1. Should `task_slas_for_tasks()` ship in the same implementation as Issue 4,
   or should Issue 4 require only documentation that points advanced users to
   `fetch_related_by_foreign_key()`?
2. Should the low-level `task_slas()` builder set default ordering, or should
   ordering live only in typed helpers to avoid surprising caller-added order
   clauses?
3. Should `TaskSlaSummary` own cloned `TaskSla` values, borrow rows, or store
   only identifiers and computed fields?
4. Is a 100-id bulk chunk size conservative enough for customer instances with
   long domain-separated sysparm queries?
