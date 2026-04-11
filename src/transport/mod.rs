use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

use crate::error::Result;

pub mod graphql;
pub mod http;
pub mod response;
pub mod retry;

pub use self::graphql::GraphqlTransport;
pub use self::http::HttpTransport;
pub use response::ServiceNowResponse;
pub use retry::RetryConfig;

/// How the client should choose a transport implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TransportMode {
    /// Pick the best transport internally. Today this means HTTP.
    #[default]
    Auto,
    /// Force REST transport.
    Rest,
    /// Placeholder for future GraphQL transport selection.
    Graphql,
}

/// Transport selection policy stored on the transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransportSelection {
    pub preferred: TransportMode,
    pub graphql_fallback: bool,
    pub graphql_batch_threshold: usize,
}

impl Default for TransportSelection {
    fn default() -> Self {
        Self {
            preferred: TransportMode::Auto,
            graphql_fallback: true,
            graphql_batch_threshold: 3,
        }
    }
}

impl TransportSelection {
    pub fn new(
        preferred: TransportMode,
        graphql_fallback: bool,
        graphql_batch_threshold: usize,
    ) -> Self {
        Self {
            preferred,
            graphql_fallback,
            graphql_batch_threshold,
        }
    }
}

/// Shared async transport interface used by the client and query builders.
#[async_trait]
pub trait Transport: Send + Sync + std::fmt::Debug {
    async fn get(&self, path: &str, params: &[(String, String)]) -> Result<ServiceNowResponse>;

    async fn post(&self, path: &str, body: Value) -> Result<ServiceNowResponse>;

    async fn post_with_params(
        &self,
        path: &str,
        params: &[(String, String)],
        body: Value,
    ) -> Result<ServiceNowResponse>;

    async fn put(&self, path: &str, body: Value) -> Result<ServiceNowResponse>;

    async fn patch(&self, path: &str, body: Value) -> Result<ServiceNowResponse>;

    async fn patch_with_params(
        &self,
        path: &str,
        params: &[(String, String)],
        body: Value,
    ) -> Result<ServiceNowResponse>;

    async fn delete(&self, path: &str) -> Result<ServiceNowResponse>;

    fn selection(&self) -> TransportSelection {
        TransportSelection::default()
    }
}

/// Shared transport handle used across query and API layers.
pub type TransportHandle = Arc<dyn Transport>;
