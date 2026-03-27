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
}
