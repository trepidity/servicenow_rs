//! # servicenow_rs
//!
//! A Rust library for the ServiceNow REST API with support for:
//!
//! - **Flexible schema**: Ship base definitions per ServiceNow release, overlay custom schemas
//! - **Multiple auth methods**: Basic auth (phase 1), OAuth, token, certificate (future)
//! - **Rich queries**: Builder pattern with filtering, field selection, pagination, ordering
//! - **Relationship traversal**: Fetch related records (Change -> Change Tasks, Approvers, etc.)
//! - **Query batching**: Concurrent requests assembled into a single result
//! - **Display values**: Raw, display, or both value modes
//!
//! # Quick Start
//!
//! ```no_run
//! use servicenow_rs::prelude::*;
//!
//! # async fn example() -> servicenow_rs::error::Result<()> {
//! // Create client
//! let client = ServiceNowClient::builder()
//!     .instance("mycompany")
//!     .auth(BasicAuth::new("admin", "password"))
//!     .schema_release("xanadu")
//!     .build()
//!     .await?;
//!
//! // Query with relationships
//! let changes = client.table("change_request")
//!     .equals("state", "1")
//!     .include_related(&["change_task", "approvals"])
//!     .display_value(DisplayValue::Both)
//!     .limit(10)
//!     .execute()
//!     .await?;
//!
//! for record in &changes {
//!     println!("Change: {}", record.get_str("number").unwrap_or("?"));
//!     for task in record.related("change_task") {
//!         println!("  Task: {}", task.get_str("number").unwrap_or("?"));
//!     }
//! }
//! # Ok(())
//! # }
//! ```

pub mod api;
pub mod auth;
pub mod client;
pub mod config;
pub mod error;
pub mod model;
pub mod prefix;
pub mod query;
pub mod schema;
pub mod transport;

/// Convenience re-exports for common usage.
pub mod prelude {
    pub use crate::api::aggregate::{AggregateApi, AggregateResult};
    pub use crate::api::approval::{ApprovalAction, ApprovalBuilder};
    pub use crate::api::catalog::CatalogVariable;
    pub use crate::auth::{BasicAuth, TokenAuth};
    pub use crate::client::{ClientBuilder, ServiceNowClient};
    pub use crate::error::{Error, Result};
    pub use crate::model::{
        parse_servicenow_timestamp, DisplayValue, FieldValue, JournalEntry, QueryResult, Record,
    };
    pub use crate::prefix::PrefixRegistry;
    pub use crate::query::{FetchStrategy, Operator, Order, Paginator};
    pub use crate::schema::SchemaRegistry;
    pub use crate::schema::{
        child_relation_for_table, parent_reference_field, reference_default_table,
        reference_fields_for_table,
    };
}
