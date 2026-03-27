use std::path::Path;

use crate::error::Result;

use super::definition::{
    FieldDef, RelationshipDef, SchemaDefinition, SchemaOverlay, TableDef,
};
use super::loader;

/// Runtime registry for looking up table and relationship definitions.
///
/// The registry holds a merged schema (base + overlays) and provides
/// fast lookups by table name.
#[derive(Debug, Clone)]
pub struct SchemaRegistry {
    schema: SchemaDefinition,
}

impl SchemaRegistry {
    /// Create a registry from a pre-built schema definition.
    pub fn new(schema: SchemaDefinition) -> Self {
        Self { schema }
    }

    /// Create a registry from a bundled release name (e.g., "xanadu").
    pub fn from_release(release: &str) -> Result<Self> {
        let schema = loader::load_bundled_definition(release)?;
        Ok(Self { schema })
    }

    /// Create a registry from a bundled release with an overlay file applied.
    pub fn from_release_with_overlay(release: &str, overlay_path: &Path) -> Result<Self> {
        let mut schema = loader::load_bundled_definition(release)?;
        let overlay = loader::load_overlay(overlay_path)?;
        loader::merge_overlay(&mut schema, &overlay);
        Ok(Self { schema })
    }

    /// Create a registry from a bundled release with an overlay string applied.
    pub fn from_release_with_overlay_str(release: &str, overlay_json: &str) -> Result<Self> {
        let mut schema = loader::load_bundled_definition(release)?;
        let overlay = loader::load_overlay_from_str(overlay_json)?;
        loader::merge_overlay(&mut schema, &overlay);
        Ok(Self { schema })
    }

    /// Apply an additional overlay to this registry.
    pub fn apply_overlay(&mut self, overlay: &SchemaOverlay) {
        loader::merge_overlay(&mut self.schema, overlay);
    }

    /// Get the release name.
    pub fn release(&self) -> &str {
        &self.schema.release
    }

    /// Get a table definition by name.
    pub fn table(&self, name: &str) -> Option<&TableDef> {
        self.schema.tables.get(name)
    }

    /// Get a field definition for a specific table and field.
    ///
    /// Walks the inheritance chain (via `extends`) to find inherited fields.
    pub fn field(&self, table: &str, field: &str) -> Option<&FieldDef> {
        let mut current = Some(table);
        while let Some(tbl_name) = current {
            if let Some(tbl) = self.schema.tables.get(tbl_name) {
                if let Some(f) = tbl.fields.get(field) {
                    return Some(f);
                }
                current = tbl.extends.as_deref();
            } else {
                break;
            }
        }
        None
    }

    /// Get a relationship definition for a specific table and relationship name.
    ///
    /// Walks the inheritance chain (via `extends`) to find inherited relationships.
    pub fn relationship(&self, table: &str, rel_name: &str) -> Option<&RelationshipDef> {
        let mut current = Some(table);
        while let Some(tbl_name) = current {
            if let Some(tbl) = self.schema.tables.get(tbl_name) {
                if let Some(r) = tbl.relationships.get(rel_name) {
                    return Some(r);
                }
                current = tbl.extends.as_deref();
            } else {
                break;
            }
        }
        None
    }

    /// Get all relationship definitions for a table, including inherited ones.
    pub fn relationships(&self, table: &str) -> Vec<(&str, &RelationshipDef)> {
        let mut result = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut current = Some(table);

        while let Some(tbl_name) = current {
            if let Some(tbl) = self.schema.tables.get(tbl_name) {
                for (k, v) in &tbl.relationships {
                    if seen.insert(k.as_str()) {
                        result.push((k.as_str(), v));
                    }
                }
                current = tbl.extends.as_deref();
            } else {
                break;
            }
        }
        result
    }

    /// List all known table names.
    pub fn table_names(&self) -> Vec<&str> {
        self.schema.tables.keys().map(|s| s.as_str()).collect()
    }

    /// Check if a table exists in the schema.
    pub fn has_table(&self, name: &str) -> bool {
        self.schema.tables.contains_key(name)
    }

    /// Check if a field exists on a table.
    pub fn has_field(&self, table: &str, field: &str) -> bool {
        self.field(table, field).is_some()
    }

    /// Get the parent table (via "extends") for a given table, if any.
    pub fn parent_table(&self, table: &str) -> Option<&str> {
        self.table(table)
            .and_then(|t| t.extends.as_deref())
    }

    /// Get all writable fields for a table (including inherited).
    pub fn writable_fields(&self, table: &str) -> Vec<(&str, &FieldDef)> {
        self.all_fields(table)
            .into_iter()
            .filter(|(_, f)| f.is_writable())
            .collect()
    }

    /// Get all read-only fields for a table (including inherited).
    pub fn read_only_fields(&self, table: &str) -> Vec<(&str, &FieldDef)> {
        self.all_fields(table)
            .into_iter()
            .filter(|(_, f)| f.read_only)
            .collect()
    }

    /// Get all mandatory fields for a table (including inherited).
    pub fn mandatory_fields(&self, table: &str) -> Vec<(&str, &FieldDef)> {
        self.all_fields(table)
            .into_iter()
            .filter(|(_, f)| f.mandatory)
            .collect()
    }

    /// Get all journal fields for a table (including inherited).
    pub fn journal_fields(&self, table: &str) -> Vec<(&str, &FieldDef)> {
        self.all_fields(table)
            .into_iter()
            .filter(|(_, f)| f.is_journal())
            .collect()
    }

    /// Get all fields for a table, walking the inheritance chain.
    pub fn all_fields(&self, table: &str) -> Vec<(&str, &FieldDef)> {
        let mut result = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut current = Some(table);

        while let Some(tbl_name) = current {
            if let Some(tbl) = self.schema.tables.get(tbl_name) {
                for (k, v) in &tbl.fields {
                    if seen.insert(k.as_str()) {
                        result.push((k.as_str(), v));
                    }
                }
                current = tbl.extends.as_deref();
            } else {
                break;
            }
        }
        result
    }

    /// Get the full schema definition.
    pub fn schema(&self) -> &SchemaDefinition {
        &self.schema
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_xanadu() {
        let registry = SchemaRegistry::from_release("xanadu").unwrap();
        assert_eq!(registry.release(), "xanadu");
        assert!(registry.has_table("change_request"));
        assert!(registry.has_table("incident"));
    }

    #[test]
    fn test_unknown_release() {
        let result = SchemaRegistry::from_release("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_table_lookup() {
        let registry = SchemaRegistry::from_release("xanadu").unwrap();
        let table = registry.table("change_request").unwrap();
        assert_eq!(table.label, "Change Request");
        // "number" is inherited from the "task" table via extends.
        assert!(registry.has_field("change_request", "number"));
        // Direct field on change_request.
        assert!(registry.has_field("change_request", "risk"));
    }

    #[test]
    fn test_relationship_lookup() {
        let registry = SchemaRegistry::from_release("xanadu").unwrap();
        let rel = registry.relationship("change_request", "change_task");
        assert!(rel.is_some());
        let rel = rel.unwrap();
        assert_eq!(rel.table, "change_task");
    }

    #[test]
    fn test_overlay_application() {
        let overlay_json = r#"{
            "extends_release": "xanadu",
            "tables": {
                "change_request": {
                    "fields": {
                        "u_custom_field": {
                            "type": "string",
                            "max_length": 255
                        }
                    }
                }
            }
        }"#;
        let registry =
            SchemaRegistry::from_release_with_overlay_str("xanadu", overlay_json).unwrap();
        assert!(registry.has_field("change_request", "u_custom_field"));
        // Original fields still present.
        assert!(registry.has_field("change_request", "number"));
    }

    #[test]
    fn test_field_attributes() {
        let registry = SchemaRegistry::from_release("xanadu").unwrap();

        // sys_id is read-only (inherited from task).
        let sys_id = registry.field("incident", "sys_id").unwrap();
        assert!(sys_id.read_only);
        assert!(!sys_id.is_journal());
        assert!(sys_id.is_writable() == false);

        // work_notes is journal + write-only (inherited from task).
        let wn = registry.field("incident", "work_notes").unwrap();
        assert!(wn.is_journal());
        assert!(wn.write_only);

        // comments is journal + write-only.
        let c = registry.field("incident", "comments").unwrap();
        assert!(c.is_journal());
        assert!(c.write_only);

        // assigned_to is a reference field.
        let at = registry.field("incident", "assigned_to").unwrap();
        assert!(at.is_reference());
        assert_eq!(at.reference_table.as_deref(), Some("sys_user"));
        assert!(at.is_writable());
    }

    #[test]
    fn test_writable_fields() {
        let registry = SchemaRegistry::from_release("xanadu").unwrap();
        let writable = registry.writable_fields("incident");
        let read_only = registry.read_only_fields("incident");

        // Should have writable fields.
        assert!(!writable.is_empty());
        // Should have read-only fields (sys_id, number, etc.).
        assert!(!read_only.is_empty());

        // Writable should NOT contain sys_id.
        assert!(!writable.iter().any(|(name, _)| *name == "sys_id"));
        // Read-only SHOULD contain sys_id.
        assert!(read_only.iter().any(|(name, _)| *name == "sys_id"));
    }

    #[test]
    fn test_journal_fields() {
        let registry = SchemaRegistry::from_release("xanadu").unwrap();
        let journals = registry.journal_fields("incident");

        let names: Vec<&str> = journals.iter().map(|(n, _)| *n).collect();
        assert!(names.contains(&"work_notes"));
        assert!(names.contains(&"comments"));
        assert!(names.contains(&"comments_and_work_notes"));
        assert!(names.contains(&"approval_history"));
    }

    #[test]
    fn test_all_fields_with_inheritance() {
        let registry = SchemaRegistry::from_release("xanadu").unwrap();
        let all = registry.all_fields("incident");

        let names: Vec<&str> = all.iter().map(|(n, _)| *n).collect();
        // Should include incident-specific fields.
        assert!(names.contains(&"caller_id"));
        assert!(names.contains(&"incident_state"));
        // Should include inherited task fields.
        assert!(names.contains(&"number"));
        assert!(names.contains(&"work_notes"));
        assert!(names.contains(&"assigned_to"));
    }

    #[test]
    fn test_yokohama_and_washington_releases() {
        let yoko = SchemaRegistry::from_release("yokohama").unwrap();
        assert_eq!(yoko.release(), "yokohama");
        assert!(yoko.has_table("incident"));

        let wash = SchemaRegistry::from_release("washington").unwrap();
        assert_eq!(wash.release(), "washington");
        assert!(wash.has_table("change_request"));
    }
}
