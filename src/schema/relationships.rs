//! Schema relationship helpers for common ServiceNow tables.
//!
//! These functions encode the structural relationships between ServiceNow tables
//! and fields as static knowledge, allowing callers to navigate parent/child
//! relationships and resolve reference fields without querying the instance.

/// Reference fields to extract for a given ServiceNow table.
///
/// Returns the list of field names that are reference fields (foreign keys
/// to other tables) for common ServiceNow tables.
pub fn reference_fields_for_table(table_name: &str) -> &'static [&'static str] {
    match table_name {
        "kb_knowledge" => &["knowledge_base", "category", "author"],
        "sysapproval_approver" => &["approver", "sysapproval"],
        _ => &[
            "assigned_to",
            "assignment_group",
            "caller_id",
            "cmdb_ci",
            "requested_for",
            "request_item",
            "change_request",
            "story",
        ],
    }
}

/// Default target table for a reference field name.
///
/// Maps common reference field names to the ServiceNow table they
/// typically point to.
pub fn reference_default_table(field_name: &str) -> Option<&'static str> {
    match field_name {
        "assigned_to" | "caller_id" | "requested_for" => Some("sys_user"),
        "assignment_group" => Some("sys_user_group"),
        "cmdb_ci" => Some("cmdb_ci"),
        "request_item" => Some("sc_req_item"),
        "change_request" => Some("change_request"),
        "story" => Some("rm_story"),
        "knowledge_base" => Some("kb_knowledge_base"),
        "category" => Some("kb_category"),
        "author" => Some("sys_user"),
        "approver" => Some("sys_user"),
        "sysapproval" => Some("task"),
        _ => None,
    }
}

/// For child tables, returns `(parent_field_name, parent_table)`.
///
/// Maps child tables to the field name that references their parent and
/// the parent table name.
pub fn parent_reference_field(table_name: &str) -> Option<(&'static str, &'static str)> {
    match table_name {
        "change_task" => Some(("change_request", "change_request")),
        "sc_task" => Some(("request_item", "sc_req_item")),
        "rm_scrum_task" => Some(("story", "rm_story")),
        "sysapproval_approver" => Some(("sysapproval", "task")),
        _ => None,
    }
}

/// For parent tables, returns `(child_table, foreign_key_field)`.
///
/// Maps parent tables to their child table and the field name the child
/// uses to reference the parent.
pub fn child_relation_for_table(table_name: &str) -> Option<(&'static str, &'static str)> {
    match table_name {
        "change_request" => Some(("change_task", "change_request")),
        "sc_req_item" | "request_item" => Some(("sc_task", "request_item")),
        "rm_story" => Some(("rm_scrum_task", "story")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reference_fields_for_incident() {
        let fields = reference_fields_for_table("incident");
        assert!(fields.contains(&"assigned_to"));
        assert!(fields.contains(&"assignment_group"));
    }

    #[test]
    fn test_reference_fields_for_knowledge() {
        let fields = reference_fields_for_table("kb_knowledge");
        assert!(fields.contains(&"author"));
        assert!(fields.contains(&"knowledge_base"));
        assert!(!fields.contains(&"assigned_to"));
    }

    #[test]
    fn test_default_table_for_assigned_to() {
        assert_eq!(reference_default_table("assigned_to"), Some("sys_user"));
    }

    #[test]
    fn test_default_table_unknown() {
        assert_eq!(reference_default_table("unknown_field"), None);
    }

    #[test]
    fn test_parent_reference_field() {
        assert_eq!(
            parent_reference_field("change_task"),
            Some(("change_request", "change_request"))
        );
        assert!(parent_reference_field("incident").is_none());
    }

    #[test]
    fn test_child_relation() {
        assert_eq!(
            child_relation_for_table("change_request"),
            Some(("change_task", "change_request"))
        );
        assert!(child_relation_for_table("incident").is_none());
    }
}
