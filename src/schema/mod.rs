pub mod definition;
pub mod loader;
pub mod registry;
pub mod relationships;

pub use definition::{
    FieldDef, FieldType, RelationshipDef, RelationshipType, SchemaDefinition, SchemaOverlay,
    TableDef,
};
pub use registry::SchemaRegistry;
pub use relationships::{
    child_relation_for_table, parent_reference_field, reference_default_table,
    reference_fields_for_table,
};
