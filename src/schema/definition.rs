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
    /// Whether this field is read-only (system-generated, cannot be set via API).
    #[serde(default)]
    pub read_only: bool,
    /// Whether this field is mandatory (required for create/update).
    #[serde(default)]
    pub mandatory: bool,
    /// Whether this field is write-only (journal fields: can be set but always return empty).
    /// Journal fields like work_notes and comments accept input on POST/PATCH
    /// but return empty strings on GET. Read actual entries via sys_journal_field.
    #[serde(default)]
    pub write_only: bool,
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

impl FieldDef {
    /// Whether this field can be set via API (not read-only and not a read-only journal).
    pub fn is_writable(&self) -> bool {
        !self.read_only
    }

    /// Whether this field is a journal type (work_notes, comments, etc.).
    pub fn is_journal(&self) -> bool {
        matches!(
            self.field_type,
            FieldType::Journal | FieldType::JournalInput
        )
    }

    /// Whether this is a reference field pointing to another table.
    pub fn is_reference(&self) -> bool {
        self.field_type == FieldType::Reference && self.reference_table.is_some()
    }
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
