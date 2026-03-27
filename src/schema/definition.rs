use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// A complete schema definition for a ServiceNow release.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaDefinition {
    /// The ServiceNow release name (e.g., "xanadu", "washington").
    pub release: String,
    /// Table definitions keyed by table name.
    pub tables: HashMap<String, TableDef>,
}

/// A custom overlay that extends or overrides a base schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaOverlay {
    /// The base release this overlay extends.
    pub extends_release: String,
    /// Table definitions to merge (additive + override).
    #[serde(default)]
    pub tables: HashMap<String, TableOverlay>,
}

/// Definition of a single ServiceNow table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableDef {
    /// Human-readable label for the table.
    pub label: String,
    /// Parent table this extends (e.g., "task").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extends: Option<String>,
    /// Field definitions keyed by field name.
    #[serde(default)]
    pub fields: HashMap<String, FieldDef>,
    /// Relationship definitions keyed by relationship name.
    #[serde(default)]
    pub relationships: HashMap<String, RelationshipDef>,
}

/// Overlay for a table — same structure but all fields optional for merging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableOverlay {
    /// Override the label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Override the parent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extends: Option<String>,
    /// Additional or overridden fields.
    #[serde(default)]
    pub fields: HashMap<String, FieldDef>,
    /// Additional or overridden relationships.
    #[serde(default)]
    pub relationships: HashMap<String, RelationshipDef>,
}

/// Definition of a single field on a table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDef {
    /// The data type of this field.
    #[serde(rename = "type")]
    pub field_type: FieldType,
    /// Maximum length for string fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_length: Option<u32>,
    /// Whether this field is read-only.
    #[serde(default)]
    pub read_only: bool,
    /// Whether this field is mandatory.
    #[serde(default)]
    pub mandatory: bool,
    /// Choice values for choice fields: value -> display label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub choices: Option<HashMap<String, String>>,
    /// For reference fields, the table being referenced.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference_table: Option<String>,
    /// Human-readable label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Definition of a relationship between tables.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipDef {
    /// The related table name.
    pub table: String,
    /// The foreign key field on the related table that points back.
    pub foreign_key: String,
    /// The type of relationship.
    #[serde(rename = "type")]
    pub relationship_type: RelationshipType,
    /// Additional filter to apply when querying related records.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
}

/// Supported field data types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    String,
    Integer,
    Boolean,
    Decimal,
    Float,
    DateTime,
    Date,
    Time,
    Reference,
    Journal,
    JournalInput,
    GlideList,
    Url,
    Email,
    Phone,
    Currency,
    Price,
    Html,
    Script,
    Conditions,
    DocumentId,
    SysClassName,
    DomainId,
    Other,
}

/// Relationship cardinality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipType {
    OneToMany,
    ManyToOne,
    ManyToMany,
}
