use std::path::Path;

use crate::error::{Error, Result};

use super::definition::{SchemaDefinition, SchemaOverlay, TableDef};

/// Load a schema definition from a JSON file.
pub fn load_definition(path: &Path) -> Result<SchemaDefinition> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        Error::Schema(format!("failed to read schema file {}: {}", path.display(), e))
    })?;
    serde_json::from_str(&content).map_err(|e| {
        Error::Schema(format!(
            "failed to parse schema file {}: {}",
            path.display(),
            e
        ))
    })
}

/// Load a schema definition from a JSON string.
pub fn load_definition_from_str(json: &str) -> Result<SchemaDefinition> {
    serde_json::from_str(json)
        .map_err(|e| Error::Schema(format!("failed to parse schema JSON: {}", e)))
}

/// Load a schema overlay from a JSON file.
pub fn load_overlay(path: &Path) -> Result<SchemaOverlay> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        Error::Schema(format!(
            "failed to read overlay file {}: {}",
            path.display(),
            e
        ))
    })?;
    serde_json::from_str(&content).map_err(|e| {
        Error::Schema(format!(
            "failed to parse overlay file {}: {}",
            path.display(),
            e
        ))
    })
}

/// Load a schema overlay from a JSON string.
pub fn load_overlay_from_str(json: &str) -> Result<SchemaOverlay> {
    serde_json::from_str(json)
        .map_err(|e| Error::Schema(format!("failed to parse overlay JSON: {}", e)))
}

/// Merge an overlay into a base schema definition.
///
/// - New tables from the overlay are added.
/// - Existing tables get their fields and relationships merged (overlay wins on conflict).
/// - If the overlay specifies a new label or extends, those override the base.
pub fn merge_overlay(base: &mut SchemaDefinition, overlay: &SchemaOverlay) {
    for (table_name, table_overlay) in &overlay.tables {
        if let Some(existing) = base.tables.get_mut(table_name) {
            // Merge fields (overlay wins).
            for (field_name, field_def) in &table_overlay.fields {
                existing.fields.insert(field_name.clone(), field_def.clone());
            }
            // Merge relationships (overlay wins).
            for (rel_name, rel_def) in &table_overlay.relationships {
                existing
                    .relationships
                    .insert(rel_name.clone(), rel_def.clone());
            }
            // Override label if provided.
            if let Some(ref label) = table_overlay.label {
                existing.label = label.clone();
            }
            // Override extends if provided.
            if let Some(ref extends) = table_overlay.extends {
                existing.extends = Some(extends.clone());
            }
        } else {
            // New table from overlay — convert TableOverlay to TableDef.
            let table_def = TableDef {
                label: table_overlay
                    .label
                    .clone()
                    .unwrap_or_else(|| table_name.clone()),
                extends: table_overlay.extends.clone(),
                fields: table_overlay.fields.clone(),
                relationships: table_overlay.relationships.clone(),
            };
            base.tables.insert(table_name.clone(), table_def);
        }
    }
}

/// Load a base schema definition from the embedded definitions directory.
///
/// Looks for `definitions/base/{release}.json` relative to a given base path,
/// or falls back to the compiled-in definitions.
pub fn load_bundled_definition(release: &str) -> Result<SchemaDefinition> {
    // Try to load from the bundled JSON embedded at compile time.
    let json = match release {
        "xanadu" => include_str!("../../definitions/base/xanadu.json"),
        "yokohama" => include_str!("../../definitions/base/yokohama.json"),
        "washington" => include_str!("../../definitions/base/washington.json"),
        _ => {
            return Err(Error::Schema(format!(
                "unknown release '{}'. Available: xanadu, yokohama, washington",
                release
            )));
        }
    };
    load_definition_from_str(json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::definition::*;
    use std::collections::HashMap;

    fn make_base() -> SchemaDefinition {
        let mut fields = HashMap::new();
        fields.insert(
            "number".to_string(),
            FieldDef {
                field_type: FieldType::String,
                max_length: Some(40),
                read_only: true,
                mandatory: false,
                choices: None,
                reference_table: None,
                label: Some("Number".to_string()),
            },
        );

        let mut tables = HashMap::new();
        tables.insert(
            "change_request".to_string(),
            TableDef {
                label: "Change Request".to_string(),
                extends: Some("task".to_string()),
                fields,
                relationships: HashMap::new(),
            },
        );

        SchemaDefinition {
            release: "test".to_string(),
            tables,
        }
    }

    #[test]
    fn test_merge_adds_new_field() {
        let mut base = make_base();
        let overlay = SchemaOverlay {
            extends_release: "test".to_string(),
            tables: {
                let mut t = HashMap::new();
                let mut fields = HashMap::new();
                fields.insert(
                    "u_custom".to_string(),
                    FieldDef {
                        field_type: FieldType::String,
                        max_length: Some(255),
                        read_only: false,
                        mandatory: false,
                        choices: None,
                        reference_table: None,
                        label: Some("Custom Field".to_string()),
                    },
                );
                t.insert(
                    "change_request".to_string(),
                    super::super::definition::TableOverlay {
                        label: None,
                        extends: None,
                        fields,
                        relationships: HashMap::new(),
                    },
                );
                t
            },
        };

        merge_overlay(&mut base, &overlay);
        let cr = &base.tables["change_request"];
        assert!(cr.fields.contains_key("number"));
        assert!(cr.fields.contains_key("u_custom"));
    }

    #[test]
    fn test_merge_adds_new_table() {
        let mut base = make_base();
        let overlay = SchemaOverlay {
            extends_release: "test".to_string(),
            tables: {
                let mut t = HashMap::new();
                t.insert(
                    "u_custom_table".to_string(),
                    super::super::definition::TableOverlay {
                        label: Some("Custom Table".to_string()),
                        extends: None,
                        fields: HashMap::new(),
                        relationships: HashMap::new(),
                    },
                );
                t
            },
        };

        merge_overlay(&mut base, &overlay);
        assert!(base.tables.contains_key("u_custom_table"));
        assert_eq!(base.tables["u_custom_table"].label, "Custom Table");
    }
}
