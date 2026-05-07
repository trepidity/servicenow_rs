use std::cmp::Ordering;

use serde_json::Value;

use super::record::Record;
use super::value::parse_servicenow_timestamp;

/// A typed view of one row from ServiceNow's `task_sla` table.
///
/// The raw [`Record`] is preserved in [`TaskSla::record`] so callers can still
/// inspect custom fields or fields not included in this typed model. Missing
/// fields, empty strings, and fields returned in an unexpected shape parse as
/// `None` for the corresponding typed property.
#[derive(Debug, Clone)]
pub struct TaskSla {
    /// The raw ServiceNow record used to build this typed view.
    pub record: Record,
    /// The `task_sla` row sys_id.
    pub sys_id: String,
    /// Raw sys_id of the task referenced by the `task` field.
    pub task_sys_id: Option<String>,
    /// Raw sys_id of the SLA definition referenced by the `sla` field.
    pub sla_sys_id: Option<String>,
    /// Display value of the `sla` reference field, when returned by the API.
    pub sla_name: Option<String>,
    /// Current Task SLA stage.
    pub stage: Option<TaskSlaStage>,
    /// Whether this Task SLA is active.
    pub active: Option<bool>,
    /// Whether this Task SLA has breached.
    pub has_breached: Option<bool>,
    /// Raw ServiceNow `start_time` value.
    pub start_time: Option<String>,
    /// Raw ServiceNow `end_time` value.
    pub end_time: Option<String>,
    /// Raw ServiceNow `planned_end_time` value.
    pub planned_end_time: Option<String>,
    /// Raw ServiceNow `original_breach_time` value.
    pub original_breach_time: Option<String>,
    /// Parsed actual elapsed percentage from the `percentage` field.
    pub actual_elapsed_percentage: Option<f64>,
    /// Raw ServiceNow `time_left` value.
    pub actual_time_left: Option<String>,
    /// Parsed business elapsed percentage from the `business_percentage` field.
    pub business_elapsed_percentage: Option<f64>,
    /// Raw ServiceNow `business_time_left` value.
    pub business_time_left: Option<String>,
    /// Raw ServiceNow `business_duration` value.
    pub business_duration: Option<String>,
    /// Raw ServiceNow `duration` value.
    pub duration: Option<String>,
    /// Raw sys_id of the referenced schedule.
    pub schedule_sys_id: Option<String>,
}

impl TaskSla {
    /// Build a typed Task SLA view from a raw ServiceNow record.
    pub fn from_record(record: Record) -> Self {
        let sys_id = record.sys_id.clone();
        Self {
            task_sys_id: raw_string_field(&record, "task"),
            sla_sys_id: raw_string_field(&record, "sla"),
            sla_name: display_string_field(&record, "sla"),
            stage: stage_field(&record, "stage"),
            active: bool_field(&record, "active"),
            has_breached: bool_field(&record, "has_breached"),
            start_time: string_field(&record, "start_time"),
            end_time: string_field(&record, "end_time"),
            planned_end_time: string_field(&record, "planned_end_time"),
            original_breach_time: string_field(&record, "original_breach_time"),
            actual_elapsed_percentage: f64_field(&record, "percentage"),
            actual_time_left: string_field(&record, "time_left"),
            business_elapsed_percentage: f64_field(&record, "business_percentage"),
            business_time_left: string_field(&record, "business_time_left"),
            business_duration: string_field(&record, "business_duration"),
            duration: string_field(&record, "duration"),
            schedule_sys_id: raw_string_field(&record, "schedule"),
            record,
            sys_id,
        }
    }

    /// Returns `true` when the Task SLA is known to be active.
    pub fn is_active(&self) -> bool {
        self.active == Some(true)
    }

    /// Returns `true` when the Task SLA is known not to have breached.
    pub fn is_unbreached(&self) -> bool {
        self.has_breached == Some(false)
    }

    /// Returns `true` for terminal stages that should not be considered for
    /// an upcoming breach.
    pub fn is_terminal_stage(&self) -> bool {
        matches!(
            self.stage,
            Some(TaskSlaStage::Completed | TaskSlaStage::Cancelled)
        )
    }

    /// Returns `true` when this row is eligible for
    /// [`TaskSlaSummary::next_breach`].
    pub fn is_next_breach_candidate(&self) -> bool {
        self.is_active()
            && self.is_unbreached()
            && !self.is_terminal_stage()
            && non_empty_str(self.planned_end_time.as_deref()).is_some()
    }
}

impl From<Record> for TaskSla {
    fn from(record: Record) -> Self {
        Self::from_record(record)
    }
}

/// Permissive Task SLA stage value.
///
/// ServiceNow instances can define custom stage values. Unknown non-empty
/// values are preserved exactly, after trimming, in [`TaskSlaStage::Other`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskSlaStage {
    /// The SLA clock is running.
    InProgress,
    /// The SLA clock is paused.
    Paused,
    /// The SLA has completed.
    Completed,
    /// The SLA has been cancelled.
    Cancelled,
    /// A customer-defined or otherwise unrecognized stage value.
    Other(String),
}

impl TaskSlaStage {
    /// Parse a raw or display stage value from ServiceNow.
    pub fn from_service_now(value: &str) -> Option<Self> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return None;
        }

        let normalized: String = trimmed
            .chars()
            .map(|c| match c {
                ' ' | '-' => '_',
                other => other.to_ascii_lowercase(),
            })
            .collect();

        Some(match normalized.as_str() {
            "in_progress" | "inprogress" => Self::InProgress,
            "paused" | "pause" => Self::Paused,
            "completed" | "complete" => Self::Completed,
            "cancelled" | "canceled" => Self::Cancelled,
            _ => Self::Other(trimmed.to_string()),
        })
    }

    /// Returns `true` for completed or cancelled stages.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled)
    }
}

/// A pure Rust summary of a set of Task SLA rows.
///
/// The summary only describes rows the caller passed in. An empty summary can
/// mean the task has no Task SLAs, or that the integration user has no readable
/// `task_sla` rows due to ServiceNow ACLs.
#[derive(Debug, Clone)]
pub struct TaskSlaSummary {
    /// Number of Task SLA rows included in the summary.
    pub total: usize,
    /// Number of rows where `active == Some(true)`.
    pub active: usize,
    /// Number of rows where `has_breached == Some(true)`.
    pub breached: usize,
    /// The active, unbreached, non-terminal row with the earliest planned end
    /// time, cloned from the input slice.
    pub next_breach: Option<TaskSla>,
    /// Highest parsed business elapsed percentage among rows that had one.
    pub highest_business_elapsed_percentage: Option<f64>,
}

impl TaskSlaSummary {
    /// Build a summary from typed Task SLA rows.
    pub fn from_task_slas(slas: &[TaskSla]) -> Self {
        let total = slas.len();
        let active = slas.iter().filter(|sla| sla.active == Some(true)).count();
        let breached = slas
            .iter()
            .filter(|sla| sla.has_breached == Some(true))
            .count();
        let highest_business_elapsed_percentage = slas
            .iter()
            .filter_map(|sla| sla.business_elapsed_percentage)
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
        let next_breach = slas
            .iter()
            .filter(|sla| sla.is_next_breach_candidate())
            .min_by(|a, b| compare_planned_end_time(a, b))
            .cloned();

        Self {
            total,
            active,
            breached,
            next_breach,
            highest_business_elapsed_percentage,
        }
    }
}

impl From<&[TaskSla]> for TaskSlaSummary {
    fn from(slas: &[TaskSla]) -> Self {
        Self::from_task_slas(slas)
    }
}

pub(crate) fn compare_planned_end_time(a: &TaskSla, b: &TaskSla) -> Ordering {
    let a_value = non_empty_str(a.planned_end_time.as_deref());
    let b_value = non_empty_str(b.planned_end_time.as_deref());

    match (a_value, b_value) {
        (Some(a_value), Some(b_value)) => {
            let a_time = parse_servicenow_timestamp(Some(a_value));
            let b_time = parse_servicenow_timestamp(Some(b_value));
            match (a_time, b_time) {
                (Some(a_time), Some(b_time)) => a_time.cmp(&b_time),
                _ => a_value.cmp(b_value),
            }
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn raw_string_field(record: &Record, field: &str) -> Option<String> {
    record.get_raw(field).and_then(non_empty_string)
}

fn display_string_field(record: &Record, field: &str) -> Option<String> {
    record.get_display(field).and_then(non_empty_string)
}

fn string_field(record: &Record, field: &str) -> Option<String> {
    record
        .get_raw(field)
        .or_else(|| record.get_display(field))
        .and_then(non_empty_string)
}

fn stage_field(record: &Record, field: &str) -> Option<TaskSlaStage> {
    record
        .get_raw(field)
        .or_else(|| record.get_display(field))
        .and_then(TaskSlaStage::from_service_now)
}

fn bool_field(record: &Record, field: &str) -> Option<bool> {
    let field_value = record.get(field)?;
    field_value
        .value
        .as_ref()
        .and_then(parse_json_bool)
        .or_else(|| {
            field_value
                .display_value
                .as_deref()
                .and_then(parse_bool_str)
        })
}

fn f64_field(record: &Record, field: &str) -> Option<f64> {
    let field_value = record.get(field)?;
    field_value
        .value
        .as_ref()
        .and_then(parse_json_f64)
        .or_else(|| field_value.display_value.as_deref().and_then(parse_f64_str))
}

fn parse_json_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(value) => Some(*value),
        Value::Number(value) => match value.as_i64() {
            Some(0) => Some(false),
            Some(1) => Some(true),
            _ => None,
        },
        Value::String(value) => parse_bool_str(value),
        _ => None,
    }
}

fn parse_bool_str(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "y" => Some(true),
        "false" | "0" | "no" | "n" => Some(false),
        _ => None,
    }
}

fn parse_json_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(value) => value.as_f64().filter(|value| value.is_finite()),
        Value::String(value) => parse_f64_str(value),
        _ => None,
    }
}

fn parse_f64_str(value: &str) -> Option<f64> {
    let cleaned = value.trim().trim_end_matches('%').trim().replace(',', "");
    if cleaned.is_empty() {
        return None;
    }
    cleaned
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
}

fn non_empty_string(value: &str) -> Option<String> {
    non_empty_str(Some(value)).map(ToString::to_string)
}

fn non_empty_str(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::value::DisplayValue;

    #[test]
    fn parses_task_sla_from_display_value_both_record() {
        let record = Record::from_json(
            "task_sla",
            &serde_json::json!({
                "sys_id": { "display_value": "sla-row", "value": "sla-row" },
                "task": { "display_value": "INC0010001", "value": "task-sys-id" },
                "sla": { "display_value": "Priority 1 resolution", "value": "sla-sys-id" },
                "sla_name": "wrong column",
                "stage": { "display_value": "In Progress", "value": "in_progress" },
                "active": { "display_value": "true", "value": "true" },
                "has_breached": { "display_value": "false", "value": false },
                "start_time": "2026-05-06 10:00:00",
                "planned_end_time": { "display_value": "2026-05-06 11:00:00", "value": "2026-05-06 11:00:00" },
                "percentage": { "display_value": "50%", "value": "50" },
                "time_left": "01:00:00",
                "business_percentage": { "display_value": "42.5", "value": "42.5" },
                "business_time_left": "00:45:00",
                "business_duration": "01:00:00",
                "duration": "01:00:00",
                "schedule": { "display_value": "Weekdays", "value": "schedule-sys-id" }
            }),
            DisplayValue::Both,
        )
        .unwrap();

        let sla = TaskSla::from_record(record);
        assert_eq!(sla.sys_id, "sla-row");
        assert_eq!(sla.task_sys_id.as_deref(), Some("task-sys-id"));
        assert_eq!(sla.sla_sys_id.as_deref(), Some("sla-sys-id"));
        assert_eq!(sla.sla_name.as_deref(), Some("Priority 1 resolution"));
        assert_eq!(sla.stage, Some(TaskSlaStage::InProgress));
        assert_eq!(sla.active, Some(true));
        assert_eq!(sla.has_breached, Some(false));
        assert_eq!(sla.actual_elapsed_percentage, Some(50.0));
        assert_eq!(sla.business_elapsed_percentage, Some(42.5));
        assert_eq!(sla.schedule_sys_id.as_deref(), Some("schedule-sys-id"));
        assert_eq!(sla.record.get_display("sla"), Some("Priority 1 resolution"));
    }

    #[test]
    fn missing_fields_return_none_and_unknown_stage_is_preserved() {
        let record = Record::from_json(
            "task_sla",
            &serde_json::json!({
                "sys_id": "sla-row",
                "stage": { "display_value": "Customer Hold", "value": "customer_hold" },
                "active": "not a bool",
                "business_percentage": ""
            }),
            DisplayValue::Both,
        )
        .unwrap();

        let sla = TaskSla::from_record(record);
        assert_eq!(
            sla.stage,
            Some(TaskSlaStage::Other("customer_hold".to_string()))
        );
        assert_eq!(sla.task_sys_id, None);
        assert_eq!(sla.sla_sys_id, None);
        assert_eq!(sla.sla_name, None);
        assert_eq!(sla.active, None);
        assert_eq!(sla.business_elapsed_percentage, None);
    }

    #[test]
    fn summary_counts_and_selects_next_breach() {
        let slas = vec![
            task_sla(
                "inactive",
                false,
                false,
                "in_progress",
                "2026-05-06 08:00:00",
                10.0,
            ),
            task_sla(
                "breached",
                true,
                true,
                "in_progress",
                "2026-05-06 09:00:00",
                99.0,
            ),
            task_sla(
                "completed",
                true,
                false,
                "completed",
                "2026-05-06 07:00:00",
                80.0,
            ),
            task_sla(
                "later",
                true,
                false,
                "in_progress",
                "2026-05-06 11:00:00",
                20.0,
            ),
            task_sla("next", true, false, "paused", "2026-05-06 10:00:00", 60.0),
        ];

        let summary = TaskSlaSummary::from_task_slas(&slas);
        assert_eq!(summary.total, 5);
        assert_eq!(summary.active, 4);
        assert_eq!(summary.breached, 1);
        assert_eq!(
            summary.next_breach.as_ref().map(|sla| sla.sys_id.as_str()),
            Some("next")
        );
        assert_eq!(summary.highest_business_elapsed_percentage, Some(99.0));
    }

    fn task_sla(
        sys_id: &str,
        active: bool,
        breached: bool,
        stage: &str,
        planned_end_time: &str,
        business_percentage: f64,
    ) -> TaskSla {
        let record = Record::from_json(
            "task_sla",
            &serde_json::json!({
                "sys_id": sys_id,
                "task": { "display_value": "INC0010001", "value": "task-sys-id" },
                "stage": stage,
                "active": active,
                "has_breached": breached,
                "planned_end_time": planned_end_time,
                "business_percentage": business_percentage.to_string()
            }),
            DisplayValue::Both,
        )
        .unwrap();
        TaskSla::from_record(record)
    }
}
