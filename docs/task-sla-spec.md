# Task SLA Read Helpers Specification

Status: Revised after architect review

Issue: https://github.com/trepidity/servicenow_rs/issues/4

Prior revision: 9df1b3f Draft Task SLA helper spec

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
- Field projection: only fields needed by `TaskSla`, `TaskSlaSummary`, and
  basic UI rendering (see `Default Projection` below)
- **No default ordering.** See `Resolved Decisions / R1`.

The helper must return `TableApi` so callers can add filters, ordering, limits,
or fields before a terminal operation. The helper must not set irreversible
builder options such as `no_count()` because the builder has no `with_count`
escape hatch as of this writing.

Reference link inclusion is not set explicitly; the `TableApi` builder default
(`exclude_reference_link = true`, `query/builder.rs:100`) already matches
the desired behavior.

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
3. Internally call `task_slas_for_tasks(&[sys_id])` and unwrap the single
   entry, preserving order produced by that helper.

This avoids duplicating pagination, chunking, and ordering logic between the
single-task and bulk paths.

### Bulk Task Lookup

Ships in the same change as the single-task helper. Resolved per `R6`.

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
- Prepopulate the result map with every requested task id and an empty vector,
  so callers can index by id without missing-key handling.
- Query `task_sla` with `task IN (...)` chunks instead of one request per task.
- **Chunk size: 100 task ids per chunk.** See `R8`.
- **Concurrency: at most 4 chunks in flight at once.** See `R4`.
- Use field projection (`Default Projection` below) and `DisplayValue::Both`.
- Use `no_count()` because this helper drains result pages by short-page
  detection and does not need ServiceNow's total count.
- Drain pagination for each chunk via `Paginator::collect_all()`
  (`query/paginator.rs:176`).
- Group records by the **raw** `task` sys_id, not by display value.
- **Ordering:** within each task's `Vec<TaskSla>`, sort active rows first,
  then unbreached rows, then ascending non-empty `planned_end_time`. The
  ordering is applied in Rust after collection, not via `sysparm_orderby`,
  so it does not interact with caller-added ordering on the low-level helper.
- On any chunk failure, fail the whole call. Partial-result reporting is
  deferred until the crate gains a structured partial-result type.

`task_slas_for_tasks` is the supported non-N+1 path. `task_slas_for_number`
defers to it; callers writing report-style workflows should call it directly.

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
already walks `extends`, so a single task-level relationship is inherited
(`schema/registry.rs` `relationship()`).

#### Internal chunking for `fetch_related_by_foreign_key`

`include_related(&["task_sla"])` resolves through
`fetch_related_by_foreign_key` at `client.rs:211`, which today builds a single
`task IN (...)` query without chunking. With more than ~100-200 parent records
this can exceed customer URL/encoded-query limits.

Resolved per `R2`: this issue updates `fetch_related_by_foreign_key` to chunk
internally using the same defaults as the bulk helper (100-id chunks, max 4
concurrent chunks). Callers no longer have to cap parent-set size manually,
and `include_related(&["task_sla"])` becomes safe at any parent count the
parent query itself can return.

Large-system guidance after this change:

- `include_related(&["task_sla"])` is correct for any parent query.
- For full-table scans where the parent is also enumerated by sys_id,
  `task_slas_for_tasks()` is one fewer hop.
- Documentation must still warn users to page parent task queries and cap
  report sizes intentionally — chunking solves URL length, not response size.

## Typed Model

Add `src/model/sla.rs` and re-export it from `src/model/mod.rs` and the
prelude.

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

`TaskSla` field names are Rust-facing. The ServiceNow source for each field
must come from the committed dictionary evidence.

### Column-Mapped Fields

These fields map to a single `sys_dictionary` row and must appear in the
projection (`sysparm_fields`):

| Rust field | Candidate internal column |
|---|---|
| `task_sys_id` | `task` |
| `sla_sys_id` | `sla` |
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

If a field is absent or has an unexpected shape, parsing must return `None`
for that typed property and keep the raw `Record` available.

### Derived Fields

These are not `sys_dictionary` rows and are not requested in `sysparm_fields`.
They are derived from the response payload when `DisplayValue::Both` is set.

| Rust field | Derivation |
|---|---|
| `sla_name` | `Record` display value of the `sla` reference field |

Implementation must use the existing `Record` accessor for display values
(introduced for `JournalEntry`); do not re-implement reference resolution.

### Default Projection

The default `sysparm_fields` value used by `task_slas()`,
`task_slas_for_number()`, and `task_slas_for_tasks()`:

```text
sys_id,task,sla,stage,active,has_breached,start_time,end_time,
planned_end_time,original_breach_time,percentage,time_left,
business_percentage,business_time_left,business_duration,duration,schedule
```

`sla_name` is intentionally absent from this list — it is derived from the
display value of `sla`, which is included in the response automatically when
`DisplayValue::Both` is set.

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
  `planned_end_time`. Completed, cancelled, inactive, and already breached
  rows must not mask a real upcoming breach.
- `highest_business_elapsed_percentage`: maximum parsed business percentage
  across rows with a value.

Per `R7`, `TaskSlaSummary` owns a cloned `TaskSla` in `next_breach` rather
than borrowing. Borrowed alternatives leak lifetime parameters into a public
type and are painful in async code; the clone cost is bounded and the summary
is computed at most once per task.

The summary must not infer missing SLA rows. If a user has no ACL access and
ServiceNow returns zero rows, the summary is empty. Documentation must explain
that "empty" can mean "no Task SLAs" or "no readable Task SLAs."

## Schema Scope

Add `task_sla` to bundled `washington`, `xanadu`, and `yokohama` definitions
with the verified field set used by `TaskSla`. All three release files are
present in `definitions/base/`.

For `contract_sla`, use only a minimal schema stub in this issue:

- `sys_id`
- `name`
- `schedule`

The `task_sla.sla -> contract_sla` reference enables dot-walking and display
values without pretending the crate has a complete SLA-definition model.

Do not add a typed `ContractSla` model in this issue.

## Query Efficiency Requirements

The implementation must avoid expensive default queries:

- Always project fields for Task SLA helpers; do not fetch full rows by
  default.
- Use `DisplayValue::Both` only because summary parsing needs raw values and
  documentation/UI examples need display values.
- Reference links remain excluded by builder default.
- Drain pagination for typed helpers that return `Vec<TaskSla>` or
  `HashMap<_, Vec<TaskSla>>`.
- Use chunked `IN` queries for bulk task ids — both in the public bulk helper
  and in `fetch_related_by_foreign_key`.
- Do not call `get_by_number()` in a loop for report-style workflows.
- `include_related(&["task_sla"])` is safe after the chunking change in `R2`,
  but the parent query itself must still be paged or capped.
- Do not request total counts (`no_count()`) for bulk reads.

## ACL And Failure Modes

`task_sla` reads can be ACL-restricted. A standard integration user may
receive zero rows without an API error. Public docs and rustdoc must mirror
the warning style already used by `journal()`:

- Empty results can mean no records or insufficient `task_sla` read access.
- Live tests should report ACL and field-access failures clearly.
- The library should not turn an empty successful response into an error.

## Documentation Requirements

Update:

- `README.md`: Quick Start example for incident SLA status, plus a separate
  bulk/report example using `task_slas_for_tasks()`.
- `src/lib.rs`: doctest showing `include_related(&["task_sla"])`.
- `docs/developer-guide.md`: implementation notes, query plan, field evidence,
  ACL caveats, large-system bulk guidance, and the chunking/concurrency
  defaults for both `task_slas_for_tasks()` and
  `fetch_related_by_foreign_key()`.
- Rustdoc for `ServiceNowClient::task_slas`,
  `ServiceNowClient::task_slas_for_number`,
  `ServiceNowClient::task_slas_for_tasks`,
  `TaskSla`, `TaskSlaStage`, and `TaskSlaSummary`.

Designer review should focus on whether the examples make these distinctions
clear:

- "No readable Task SLAs" is not necessarily "no SLA obligations."
- "Next breach" means next active and unbreached planned end time.
- Display values are for people; raw values drive parsing and grouping.
- Bulk/reporting examples should be visibly different from one-incident
  examples.

## Test Plan

### Wiremock tests

- `task_slas()` builds the expected `task_sla` query with the documented
  filter, projection, and display mode, **and asserts no `sysparm_orderby`
  is set** (R1).
- `task_slas_for_number()` resolves a number, drains multiple Task SLA pages,
  and parses typed rows.
- `task_slas_for_tasks()` chunks parent ids at 100 per chunk (R8), groups by
  raw `task`, and prepopulates the result map for missing ids.
- `task_slas_for_tasks()` URL-length assertion: at the default chunk size,
  the encoded request URL stays under 4 KB. (R8 falsifiability gate.)
- `task_slas_for_tasks()` concurrency assertion: no more than 4 chunks
  in flight simultaneously (R4).
- `task_slas_for_tasks()` short-page across chunks: when one chunk's last
  page returns fewer records than `sysparm_limit`, the typed result still
  contains every record from every chunk. (R5.)
- `fetch_related_by_foreign_key` chunking: identical chunking and concurrency
  guarantees as `task_slas_for_tasks()` (R2).
- Typed parsing handles `DisplayValue::Both`, raw-only values, missing fields,
  empty strings, numeric strings, and unknown `stage` choices.
- Typed parsing populates `sla_name` from the display value of the `sla`
  reference field, not from a `sla_name` column in the response.
- `TaskSlaSummary::next_breach` ignores inactive, breached, completed, and
  cancelled rows.
- Schema lookup proves `task_sla` on `task` is inherited by `incident`,
  `change_request`, and `problem`.

### Live read-only tests

- If credentials are present, resolve an incident sys_id and query `task_sla`.
- Report empty results with an ACL note instead of failing with an ambiguous
  assertion.
- Verify that all default-projection fields in the typed model are accepted
  by the Table API for the configured release.

### Static checks

- `cargo fmt`
- `cargo test`
- `cargo clippy -- -D warnings`

## Acceptance Criteria

- A committed dictionary evidence file under
  `docs/verification/task_sla_dictionary_yokohama.json` supports every
  default column-mapped field.
- `task_slas(task_sys_id)` returns a customizable `TableApi` over `task_sla`
  with **no preset ordering** and without setting `no_count()` (R1).
- `task_slas_for_number(number)` returns all Task SLA rows for the resolved
  task, not just one page, and ordering matches `task_slas_for_tasks()`.
- `task_slas_for_tasks(ids)` ships in the same change, chunks at 100 ids per
  request, runs at most 4 chunks concurrently, drains short pages correctly,
  and is the documented large-system path (R4, R5, R6, R8).
- `fetch_related_by_foreign_key` uses the same chunking and concurrency
  defaults; `include_related(&["task_sla"])` is safe at arbitrary parent
  count (R2).
- Bundled schemas include `task_sla`, a minimal `contract_sla`, and a
  task-level `task_sla` relationship across all three release files.
- The typed model preserves `Record` and tolerates missing or custom fields.
- `TaskSla` derived fields (`sla_name`) are populated from the response
  display value, not from `sysparm_fields` (R3).
- `TaskSlaStage` preserves unknown values.
- `TaskSlaSummary::next_breach` is active and unbreached by definition and
  owns a cloned `TaskSla` (R7).
- README, rustdoc, and developer guide document ACL caveats, ordering
  semantics, the chunk/concurrency defaults, and explicit single-record vs
  bulk usage examples.

## Findings From Architect Review And Resolutions

The findings below are recorded in this spec so that subsequent reviewers do
not need to re-litigate the prior pass.

### R1 — No default ordering on the low-level builder

**Finding.** `TableApi::order_by` (`query/builder.rs:250`) is append-style,
not replace-style. If `task_slas()` set default ordering, any caller that
added their own `.order_by(...)` would silently get the helper's defaults as
the primary sort key and their clause as a tiebreaker — almost never the
intent.

**Resolution.** `task_slas()` returns a `TableApi` with no preset ordering.
Ordering policy lives in the typed helpers (`task_slas_for_number`,
`task_slas_for_tasks`), which sort the resulting `Vec<TaskSla>` in Rust after
collection. This keeps the low-level helper unopinionated and avoids the
surprise.

### R2 — `fetch_related_by_foreign_key` chunks internally

**Finding.** The current implementation at `client.rs:211` builds a single
`IN (parent_ids)` query without chunking. `include_related(&["task_sla"])`
on a parent query of more than ~100-200 records produces an encoded query
that exceeds many customer URL caps.

**Resolution.** `fetch_related_by_foreign_key` chunks internally using the
same defaults as `task_slas_for_tasks()`: 100 ids per chunk, 4 concurrent
chunks. This change is in scope for this issue. After it lands,
`include_related(&["task_sla"])` is safe at any parent count the parent
query itself can return.

### R3 — Derived fields are documented separately from column-mapped fields

**Finding.** The prior spec listed `sla_name` in the column mapping table
with the entry "display value of `sla`," which is not a `sys_dictionary`
column.

**Resolution.** The typed model section now has a separate "Derived Fields"
subsection. `sla_name` is not in `sysparm_fields`; it is read from the
response payload's display-value position for the `sla` reference field
when `DisplayValue::Both` is set. This prevents the implementer from
searching the dictionary for a non-existent column or padding the projection
list.

### R4 — Bulk helper concurrency is bounded

**Finding.** Without an explicit cap, chunked bulk reads with thousands of
parent ids would either run serially (latency × N) or fan out without limit
(tripping rate limits and burning the transport-layer retry budget).

**Resolution.** Bulk helper and `fetch_related_by_foreign_key` both run with
at most 4 chunks in flight, relying on the existing transport-layer rate
limiter for finer pacing. The cap is asserted in tests (see Test Plan).

### R5 — Multi-chunk short-page test is required

**Finding.** The bulk helper's correctness depends on the recent short-page
fix (commit `0ccabc3`). A regression in pagination termination would cause
silent data loss across chunks — harder to detect than single-table
truncation.

**Resolution.** Test plan now includes a multi-chunk short-page wiremock
test where one chunk's last page returns fewer records than `sysparm_limit`,
and the assertion is that the merged typed result contains every record from
every chunk.

### R6 — `task_slas_for_tasks` ships with this issue

**Finding.** The motivating use case is large-system SLA inspection.
Deferring the bulk helper would push every advanced consumer to write the
N+1 anti-pattern against `task_slas_for_number`, then force a migration
when the supported helper arrives later.

**Resolution.** `task_slas_for_tasks` ships in the same PR as the single-task
helpers. `task_slas_for_number` is implemented as a thin wrapper around
`task_slas_for_tasks(&[sys_id])`.

### R7 — `TaskSlaSummary` owns cloned values

**Finding.** A borrowed `next_breach` would leak lifetime parameters into
a public type, which is painful in async code and forces awkward call
sites. The "ids only" alternative throws away the typed accessors that
motivated the model.

**Resolution.** `next_breach: Option<TaskSla>` owns a cloned `TaskSla`.
Summary computation is one-shot, the clone is bounded (a few KB), and this
is not a hot path.

### R8 — 100-id chunk default with a falsifiable URL-length test

**Finding.** 100 sys_ids × ~33 chars/id (encoded) ≈ 3.3 KB of `IN` clause.
ServiceNow Tomcat default URL cap is 8 KB; reverse proxies and domain-
separated instances are sometimes tighter (4 KB observed). 100 leaves
headroom but the assumption needs to be testable so a future query-string
addition cannot quietly push past customer caps.

**Resolution.** Default chunk size is 100. Test plan asserts the encoded
URL at the default chunk size stays under 4 KB. Future changes that push
near the cap will fail this test before reaching a customer.

### R9 — Acceptance criteria reflect the ordering decision

**Finding.** The original acceptance criteria did not record the ordering
decision, so a future implementer could re-add default ordering to
`task_slas()` without violating any criterion.

**Resolution.** Acceptance criteria now explicitly require that
`task_slas()` returns a `TableApi` with **no preset ordering** and without
setting `no_count()`.

### R10 — Spec wording cleanup

**Finding.** Prior spec stated "Reference links: excluded by default" under
the single-task helper, which is true at the builder layer
(`query/builder.rs:100`) and so the helper does not call it.

**Resolution.** Removed the redundant directive. Documented once under
"Single Task Builder" that builder default already matches desired
behavior.
