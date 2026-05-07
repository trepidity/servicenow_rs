# Task SLA — `servicenow_rs` Crate Handoff

> **Audience:** Maintainer / worker operating in `~/Projects/servicenow_rs`.
> **Scope:** This document covers the crate only. The companion document
> `2026-05-07-task-sla-agent-handoff.md` lives in the **consumer** repository
> (`servicenow_agent`) and is not present in this tree. Anything outside the
> crate is owned by that document.
> **Status:** Ready to dispatch.

**Goal:** Ship a stable, generically tested Task SLA API in the `servicenow_rs`
crate that downstream consumers (including `servicenow_agent`) can pin to.

---

## Background

- Branch: `codex/task-sla-spec`. Latest published commit: `b8df579 Implement
  Task SLA read helpers`. That commit introduced:
  - `TaskSla`, `TaskSlaStage`, `TaskSlaSummary` in `src/model/sla.rs`.
  - `ServiceNowClient::task_slas`, `task_slas_for_number`,
    `task_slas_for_tasks` in `src/client.rs`, plus the
    `TASK_SLA_BULK_CHUNK_SIZE` and `TASK_SLA_BULK_MAX_CONCURRENT_CHUNKS`
    public constants.
  - Internal chunking + capped concurrency for
    `fetch_related_by_foreign_key`.
  - `task_sla` and a minimal `contract_sla` in the bundled
    `washington`, `xanadu`, and `yokohama` schemas, plus a task-level
    `task_sla` relationship.
  - Wiremock coverage for builder shape, paginated number lookup,
    chunk size, URL-length-under-4 KB, concurrency cap, and incident-side
    schema inheritance.
  - An opt-in live read-only test
    (`tests/live_readonly_test.rs::test_live_task_sla_projection_readable_or_acl_empty`).

- **Working-tree additions not yet committed:** two unit tests inside
  `src/model/sla.rs` —
  `stage_parser_accepts_common_aliases_and_preserves_custom_values` and
  `summary_handles_empty_and_excludes_rows_without_actionable_breach_time`.
  These were authored after `b8df579` and currently live as unstaged changes.
  They must be committed as a **new** commit (not amended onto `b8df579`) before
  tagging — see Tasks.

- A grep audit across `src/` and `tests/` for organization-specific strings on
  `b8df579` returned no hits; the audit must be re-run on the post-commit HEAD
  before tagging (concrete patterns in Tasks).

- Focused validation passed on the working tree:
  `cargo test model::sla` and `cargo test task_sla`.

This handoff finalizes the crate work for downstream consumption. It does
**not** ship a consumer feature — interpretation of empty rows, CLI shape,
daemon RPC, and TUI rendering are owned by the agent-side handoff in the
consumer repository.

---

## Non-Negotiable Constraints (crate scope)

- No BSWH-specific data anywhere in `src/` or `tests/`: no real instance URLs,
  no real incident numbers, no real SLA names, no real assignment groups, no
  production-like alert text. Generic identifiers like `INC0010001`,
  `task_sys_id`, `Resolution SLA`, etc. are fine.
- Live tests remain opt-in (`SERVICENOW_LIVE_TEST=1`) and read-only. Unit and
  wiremock tests must not hit a real ServiceNow instance.
- The existing live test
  `tests/live_readonly_test.rs::test_live_task_sla_projection_readable_or_acl_empty`
  is **in scope** — do not remove or weaken it. Live execution belongs to
  the agent handoff (its `WS7`); this test exists in the crate so a maintainer
  can validate ACL/field access locally with credentials present.
- Public API additions must carry rustdoc on every type, method, and
  re-exported field. Rustdoc wording for empty-response semantics is
  prescribed below; align verbatim.
- Empty Task SLA responses are returned as empty `Vec`/`TaskSlaSummary` — the
  crate does **not** decide whether empty means "ACL-restricted" vs "no SLAs
  attached." That ambiguity is the consumer's concern (C2 below).

---

## Crate Decisions (owned here)

| ID | Decision | Resolution |
|---|---|---|
| `C1` | Semver impact and version number. | Bump `Cargo.toml` from `0.3.0` to **`0.4.0`**. Under Cargo's 0.x rules, an additive public surface change is a minor-version bump that breaks the lockfile and forces consumers to opt in. Validate the surface against the §"Public API Surface" list in this document by running `cargo doc --no-deps --document-private-items=false` and reading the generated public items; a manual diff is sufficient because `cargo-public-api` is not installed in this environment and is not declared as a dev dep. |
| `C2` | Where does "readable vs ACL-empty" ambiguity live? | **Not here.** The crate returns what the API returned. Consumers interpret. This keeps the crate free of product-layer enums. Documented in rustdoc using the verbatim wording in §"Rustdoc Wording (verbatim)" below. |
| `C3` | Should `task_slas_for_tasks` chunk size and concurrency be configurable? | **No** for first ship. Hardcoded constants (`TASK_SLA_BULK_CHUNK_SIZE = 100`, `TASK_SLA_BULK_MAX_CONCURRENT_CHUNKS = 4`) are exposed as `pub const` for documentation / introspection only. Their numeric values are not part of the semver contract — see the note under §"Public API Surface." Make configurable later only if a consumer asks with a concrete need. |
| `C4` | Release channel and pin mechanism. | Tag the resulting commit as **`v0.4.0`** and push to `origin` (`github.com/trepidity/servicenow_rs`). Consumers pin by **git tag**, not commit sha and not crates.io: `servicenow_rs = { git = "https://github.com/trepidity/servicenow_rs", tag = "v0.4.0" }`. Crates.io publication is explicitly **out of scope** for this handoff and will be a separate decision. Release notes are written on the GitHub release for the tag (this repo has no `CHANGELOG.md` and the precedent is commit-message-as-changelog plus GitHub release notes). |

---

## Public API Surface (frozen by this handoff)

```rust
// src/model/sla.rs
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

impl TaskSla {
    pub fn from_record(record: Record) -> Self;
    pub fn is_active(&self) -> bool;
    pub fn is_unbreached(&self) -> bool;
    pub fn is_terminal_stage(&self) -> bool;
    pub fn is_next_breach_candidate(&self) -> bool;
}

impl From<Record> for TaskSla;

pub enum TaskSlaStage {
    InProgress,
    Paused,
    Completed,
    Cancelled,
    Other(String),
}

impl TaskSlaStage {
    pub fn from_service_now(value: &str) -> Option<Self>;
    pub fn is_terminal(&self) -> bool;
}

pub struct TaskSlaSummary {
    pub total: usize,
    pub active: usize,
    pub breached: usize,
    pub next_breach: Option<TaskSla>,
    pub highest_business_elapsed_percentage: Option<f64>,
}

impl TaskSlaSummary {
    pub fn from_task_slas(slas: &[TaskSla]) -> Self;
}

impl From<&[TaskSla]> for TaskSlaSummary;

// src/client.rs (impl ServiceNowClient)
pub fn task_slas(&self, task_sys_id: &str) -> TableApi;
pub async fn task_slas_for_number(&self, number: &str) -> Result<Vec<TaskSla>>;
pub async fn task_slas_for_tasks(
    &self,
    task_sys_ids: &[&str],
) -> Result<HashMap<String, Vec<TaskSla>>>;

// src/client.rs (module-level)
pub const TASK_SLA_TABLE: &str = "task_sla";
pub const TASK_SLA_DEFAULT_FIELDS: &[&str] = &[
    "sys_id",
    "task",
    "sla",
    "stage",
    "active",
    "has_breached",
    "start_time",
    "end_time",
    "planned_end_time",
    "original_breach_time",
    "percentage",
    "time_left",
    "business_percentage",
    "business_time_left",
    "business_duration",
    "duration",
    "schedule",
];
pub const TASK_SLA_BULK_CHUNK_SIZE: usize = 100;
pub const TASK_SLA_BULK_MAX_CONCURRENT_CHUNKS: usize = 4;
```

**Notes on shape:**

- `task_slas` returns a `TableApi` builder, not `Result<Vec<TaskSla>>`. This is
  deliberate (R1 in `docs/task-sla-spec.md`): it lets callers add filters,
  ordering, limits, or extra fields before terminal execution. Consumers that
  want a typed `Vec<TaskSla>` for one task should call `task_slas_for_number`
  or pass a one-element slice to `task_slas_for_tasks`. Do not "fix" this to
  return a `Vec` — that is the bulk helper's job.
- `TASK_SLA_BULK_CHUNK_SIZE` and `TASK_SLA_BULK_MAX_CONCURRENT_CHUNKS` are
  `pub` for rustdoc intra-doc links and for consumer introspection only. Their
  **numeric values** may change in a later 0.x release without being treated
  as a breaking change; consumers must not assert on specific numbers. Removal
  or rename, however, is breaking.
- `TASK_SLA_TABLE` and `TASK_SLA_DEFAULT_FIELDS` are public helper metadata for
  consumers that need to inspect the lower-level query shape. The typed helper
  contract remains the client methods and `TaskSla*` types above.

Any field rename, type change, method-signature change, or removal of the
items above is a breaking change after this handoff is tagged and requires
explicit coordination with the agent handoff.

---

## Rustdoc Wording (verbatim)

The empty-response sentence on `task_slas`, `task_slas_for_number`,
`task_slas_for_tasks`, and `TaskSlaSummary` must read exactly:

> An empty result may indicate no attached SLAs or an ACL-restricted
> `task_sla` table; the crate does not distinguish.

Existing rustdoc in `src/client.rs` and `src/model/sla.rs` is close but not
identical; align all four sites verbatim before tagging.

---

## Tasks

Ordered for execution. Each item is independently verifiable.

### Bring HEAD into agreement with the freeze

- [ ] Stage and commit the working-tree additions to `src/model/sla.rs`
      (`stage_parser_accepts_common_aliases_and_preserves_custom_values` and
      `summary_handles_empty_and_excludes_rows_without_actionable_breach_time`)
      as a **new** commit on `codex/task-sla-spec`. Do not amend `b8df579`.
- [ ] Rebase `codex/task-sla-spec` onto current `main` if needed. The rebase
      must not alter the public API surface in §"Public API Surface."

### Add the tests this handoff demands but `b8df579` is missing

- [ ] **R5 multi-chunk short-page test for `task_slas_for_tasks`.** Wiremock
      ≥ 2 chunks where one chunk's last page returns fewer records than
      `sysparm_limit`. Assert the merged `HashMap<String, Vec<TaskSla>>`
      contains every record from every chunk. Place near
      `tests/integration_test.rs:1648`.
- [ ] **Dedup test for `task_slas_for_tasks`.** Pass the same task sys_id
      twice in the input slice and assert exactly one chunk is issued (count
      requests against `/api/now/table/task_sla`). The dedup logic at
      `client.rs:311` is currently untested.
- [ ] **Schema inheritance for non-incident task subclasses.** Extend or add
      a wiremock test asserting `task_sla` is inherited via the `task` parent
      for both `change_request` and `problem`, not only `incident`
      (`tests/integration_test.rs:1811` covers `incident` only).

### Rustdoc and naming

- [ ] Apply the verbatim wording from §"Rustdoc Wording" to `task_slas`,
      `task_slas_for_number`, `task_slas_for_tasks` (in `src/client.rs`) and
      to `TaskSlaSummary` (in `src/model/sla.rs`). Update existing wording in
      `src/client.rs:251–254` and `src/model/sla.rs:167–172` to match.
- [ ] Verify rustdoc renders with `cargo doc --no-deps`; no broken intra-doc
      links to `TASK_SLA_BULK_CHUNK_SIZE` /
      `TASK_SLA_BULK_MAX_CONCURRENT_CHUNKS` /
      `Paginator::collect_all`.

### Audits

- [ ] Re-run the organization-string grep on the post-commit HEAD with these
      explicit patterns (all case-insensitive, scoped to `src/` and `tests/`):

    ```bash
    rg -i 'bswh|baylor|scott\s*&\s*white|helptraining|bswhealth' src/ tests/
    rg -E '\b[A-Z]+[0-9]{6,}\b' src/ tests/   # IDs that look like real records
    rg -E 'https?://[^/[:space:]]+\.service-now\.com' src/ tests/
    ```

    Record the result in the PR description verbatim. The organization-specific
    grep must be empty. The broad ID and `service-now.com` patterns may return
    existing generic examples or placeholders such as `INC0010001` and
    `instance.service-now.com`; review those hits and sanitize only real
    organization-specific or production-like values before tagging.

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo test` (full suite, no `SERVICENOW_LIVE_TEST` set)

### Release

- [ ] Bump `Cargo.toml` `version` from `0.3.0` to `0.4.0`. Update
      `Cargo.lock` with `cargo update -p servicenow_rs`.
- [ ] Manual public-surface diff: run `cargo doc --no-deps` and confirm the
      generated public items exactly match §"Public API Surface" — no extras,
      no omissions.
- [ ] Open a PR titled `feat(sla): Task SLA read helpers (v0.4.0)` against
      `main`. Include the grep-audit output and the public-surface diff
      summary in the PR description.
- [ ] After merge, tag the merge commit on `main` as **`v0.4.0`** and push:
      `git tag -a v0.4.0 -m "Task SLA read helpers" && git push origin v0.4.0`.
- [ ] Create a GitHub release for `v0.4.0` with notes covering: new
      `TaskSla*` types and methods, chunking + concurrency defaults, empty-
      response semantics, and the consumer pin stanza from C4. Use the draft
      in §"GitHub Release Notes Draft" as the starting text.

## GitHub Release Notes Draft

Title: `Task SLA read helpers (v0.4.0)`

````markdown
## Added

- Added read-only Task SLA support backed by ServiceNow's `task_sla` table.
- Added typed SLA models:
  - `TaskSla`, preserving the raw `Record` plus typed fields for task, SLA,
    stage, active/breached state, breach timing, elapsed percentage, duration,
    and schedule values.
  - `TaskSlaStage`, with built-in variants for in-progress, paused,
    completed, and cancelled rows, plus `Other(String)` for custom stage
    values.
  - `TaskSlaSummary`, a pure Rust summary over caller-provided rows with total,
    active, breached, next breach, and highest business elapsed percentage.
- Added `ServiceNowClient::task_slas(task_sys_id)` for callers that want a
  customizable `TableApi` over one task's Task SLA rows.
- Added `ServiceNowClient::task_slas_for_number(number)` to resolve a
  task-like record number and return typed Task SLA rows.
- Added `ServiceNowClient::task_slas_for_tasks(task_sys_ids)` for bulk
  workflows that already have raw task sys_ids.
- Added public bulk defaults:
  - `TASK_SLA_BULK_CHUNK_SIZE = 100`
  - `TASK_SLA_BULK_MAX_CONCURRENT_CHUNKS = 4`

## Behavior

- Bulk Task SLA reads deduplicate task ids, prepopulate requested ids with
  empty vectors, query `task_sla` with chunked `task IN (...)` requests, cap
  concurrent chunks at 4, drain paginated chunk responses, and group rows by
  raw task sys_id.
- Task SLA helpers request `DisplayValue::Both`: raw values drive parsing and
  grouping, while display values such as the SLA name are presentation data.
- An empty result may indicate no attached SLAs or an ACL-restricted
  `task_sla` table; the crate does not distinguish.

## Consumer Pin

Consumers should pin the v0.4.0 git tag:

```toml
[dependencies]
servicenow_rs = { git = "https://github.com/trepidity/servicenow_rs", tag = "v0.4.0" }
```
````

### Publication of the tag identifier to the agent handoff

- [ ] Once `v0.4.0` is published, record the **tag name** (`v0.4.0`), the
      **merge commit sha** on `main`, and the **release URL** in the PR
      description. The dispatcher who routed this work is responsible for
      relaying these to the consumer-side handoff
      (`servicenow_agent/docs/superpowers/plans/2026-05-07-task-sla-agent-handoff.md`)
      so the agent can bump its dependency in one step. The crate worker
      does not write to the consumer repo.

---

## Test Plan

All tests are generic; no live calls without `SERVICENOW_LIVE_TEST=1`.

### Already present on `b8df579`

- [x] `task_slas` builds the expected `task_sla` query with the documented
      filter, projection, display mode, **and no `sysparm_orderby`**
      (`integration_test.rs:1509`).
- [x] `task_slas_for_number` resolves a number, drains multiple Task SLA
      pages, parses typed rows (`integration_test.rs:1538`).
- [x] `task_slas_for_tasks` chunks parent ids at 100 per chunk, groups by
      raw `task`, prepopulates the result map for missing ids, asserts
      encoded-URL length under 4 KB (`integration_test.rs:1648`).
- [x] `task_slas_for_tasks` concurrency assertion: ≤ 4 chunks in flight
      (`integration_test.rs:1775`).
- [x] `include_related(&["task_sla"])` chunks parent ids and inherits
      through `task` for `incident` (`integration_test.rs:1811`).
- [x] Typed parsing handles `DisplayValue::Both`, raw-only values,
      missing fields, empty strings, numeric strings, unknown `stage`
      choices (`src/model/sla.rs` unit tests).

### Already present on the working tree (must be committed; see Tasks)

- [ ] Stage parser accepts common aliases and preserves custom values.
- [ ] Summary handles the empty case and excludes rows without actionable
      breach time.

### To be added in this PR (see Tasks)

- [ ] R5 multi-chunk short-page test for `task_slas_for_tasks`.
- [ ] Dedup test for `task_slas_for_tasks`.
- [ ] Schema inheritance assertions for `change_request` and `problem`.

### Validation gates before tag

- [ ] `cargo fmt --all -- --check` clean.
- [ ] `cargo clippy --all-targets -- -D warnings` clean.
- [ ] `cargo test` full suite green.
- [ ] Grep audit clean against the three patterns in §"Tasks → Audits."
- [ ] Manual public-surface diff matches §"Public API Surface" exactly.

Live validation against a real ServiceNow instance is **out of scope** for
this crate handoff. The opt-in
`test_live_task_sla_projection_readable_or_acl_empty` exists only as a
local-with-credentials sanity check and is not gating.

---

## Exit Criteria

- `v0.4.0` tag exists on `origin` (`github.com/trepidity/servicenow_rs`),
  pointing to a commit on `main` whose public surface matches §"Public API
  Surface" exactly, with the verbatim rustdoc wording from §"Rustdoc
  Wording" applied.
- All generic tests green; clippy clean; format clean; grep audit clean and
  recorded in the PR.
- A GitHub release for `v0.4.0` is published with notes covering new types,
  methods, defaults, empty-response semantics, and the consumer pin stanza.
- The PR description records the tag name, merge sha, and release URL so
  the dispatcher can relay them to the agent handoff in the consumer
  repository.

---

## Out of Scope (handled by the agent handoff)

- `TaskSlaReadability` enum and the empty-vs-ACL interpretation.
- `TaskSlaStatus`, `TaskSlaView`, `TaskSlaSummaryView` product-layer types.
- CLI command shape, TUI rendering, daemon RPC, MCP exposure.
- Training-instance smoke validation against BSWH.
- Persistence decisions.
- Crates.io publication.

If a question lands here that touches any of the above, **do not solve it
in this crate**. Punt back to the dispatcher; the consumer-side companion
document `2026-05-07-task-sla-agent-handoff.md` (located in the consumer
repository, not in this tree) carries those decisions.
