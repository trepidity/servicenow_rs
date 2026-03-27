use std::collections::HashMap;

use serde_json::Value;

use super::value::{parse_field_value, DisplayValue, FieldValue};

/// A single record from a ServiceNow table.
#[derive(Debug, Clone)]
pub struct Record {
    /// The table this record belongs to.
    pub table: String,
    /// The sys_id of this record.
    pub sys_id: String,
    /// Field name -> value mapping.
    fields: HashMap<String, FieldValue>,
    /// Related records fetched via relationship traversal.
    /// Key is the relationship name (e.g., "change_task"), value is the list of related records.
    related: HashMap<String, Vec<Record>>,
}

impl Record {
    /// Create a new empty record.
    pub fn new(table: impl Into<String>, sys_id: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            sys_id: sys_id.into(),
            fields: HashMap::new(),
            related: HashMap::new(),
        }
    }

    /// Parse a record from a JSON object returned by the ServiceNow API.
    pub fn from_json(table: &str, json: &Value, display_mode: DisplayValue) -> Option<Self> {
        let obj = json.as_object()?;

        let sys_id = obj
            .get("sys_id")
            .and_then(|v| match v {
                Value::String(s) => Some(s.clone()),
                Value::Object(o) => o
                    .get("value")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                _ => None,
            })
            .unwrap_or_default();

        let mut fields = HashMap::new();
        for (key, value) in obj {
            fields.insert(key.clone(), parse_field_value(value.clone(), display_mode));
        }

        Some(Self {
            table: table.to_string(),
            sys_id,
            fields,
            related: HashMap::new(),
        })
    }

    /// Get a field value by name.
    pub fn get(&self, field: &str) -> Option<&FieldValue> {
        self.fields.get(field)
    }

    /// Get a field's string representation, preferring display value.
    pub fn get_str(&self, field: &str) -> Option<&str> {
        self.fields.get(field).and_then(|fv| fv.as_str())
    }

    /// Get a field's raw string value.
    pub fn get_raw(&self, field: &str) -> Option<&str> {
        self.fields.get(field).and_then(|fv| fv.raw_str())
    }

    /// Get a field's display string value.
    pub fn get_display(&self, field: &str) -> Option<&str> {
        self.fields.get(field).and_then(|fv| fv.display_str())
    }

    /// Get all field names.
    pub fn field_names(&self) -> impl Iterator<Item = &str> {
        self.fields.keys().map(|s| s.as_str())
    }

    /// Get all fields.
    pub fn fields(&self) -> &HashMap<String, FieldValue> {
        &self.fields
    }

    /// Set a field value.
    pub fn set(&mut self, field: impl Into<String>, value: FieldValue) {
        self.fields.insert(field.into(), value);
    }

    /// Get related records by relationship name.
    pub fn related(&self, relationship: &str) -> &[Record] {
        self.related
            .get(relationship)
            .map(|v| v.as_slice())
            .unwrap_or_default()
    }

    /// Get all relationship names that have data.
    pub fn relationship_names(&self) -> impl Iterator<Item = &str> {
        self.related.keys().map(|s| s.as_str())
    }

    /// Attach related records under a relationship name.
    pub fn set_related(&mut self, relationship: impl Into<String>, records: Vec<Record>) {
        self.related.insert(relationship.into(), records);
    }

    /// Check if this record has any related records.
    pub fn has_related(&self) -> bool {
        !self.related.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_json_simple() {
        let json = serde_json::json!({
            "sys_id": "abc123",
            "number": "CHG0012345",
            "state": "1"
        });
        let record = Record::from_json("change_request", &json, DisplayValue::Raw).unwrap();
        assert_eq!(record.sys_id, "abc123");
        assert_eq!(record.table, "change_request");
        assert_eq!(record.get_str("number"), Some("CHG0012345"));
    }

    #[test]
    fn test_from_json_display_value_all() {
        let json = serde_json::json!({
            "sys_id": { "display_value": "abc123", "value": "abc123" },
            "state": { "display_value": "New", "value": "1" }
        });
        let record = Record::from_json("change_request", &json, DisplayValue::Both).unwrap();
        assert_eq!(record.sys_id, "abc123");
        assert_eq!(record.get_display("state"), Some("New"));
        assert_eq!(record.get_raw("state"), Some("1"));
    }

    #[test]
    fn test_related_records() {
        let mut parent = Record::new("change_request", "parent_id");
        let child = Record::new("change_task", "child_id");
        parent.set_related("change_task", vec![child]);

        assert_eq!(parent.related("change_task").len(), 1);
        assert_eq!(parent.related("nonexistent").len(), 0);
        assert!(parent.has_related());
    }
}
