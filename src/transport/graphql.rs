use async_trait::async_trait;
use serde_json::Value;

use crate::api::graphql::GraphqlOperation;
use crate::error::Result;

use super::{HttpTransport, ServiceNowResponse, Transport, TransportSelection};

#[derive(Debug)]
pub struct GraphqlTransport {
    http: HttpTransport,
}

impl GraphqlTransport {
    pub fn new(http: HttpTransport) -> Self {
        Self { http }
    }

    pub fn http(&self) -> &HttpTransport {
        &self.http
    }
}

#[async_trait]
impl Transport for GraphqlTransport {
    async fn get(&self, path: &str, params: &[(String, String)]) -> Result<ServiceNowResponse> {
        if let Some(operation) = self.select_operation(path, params)? {
            match self.execute_operation(&operation).await {
                Ok(response) => return Ok(response),
                Err(err) if self.http.selection().graphql_fallback => {
                    tracing::debug!(error = %err, path = path, "graphql read failed, falling back to REST");
                }
                Err(err) => return Err(err),
            }
        }

        self.http.get(path, params).await
    }

    async fn post(&self, path: &str, body: Value) -> Result<ServiceNowResponse> {
        self.http.post(path, body).await
    }

    async fn post_with_params(
        &self,
        path: &str,
        params: &[(String, String)],
        body: Value,
    ) -> Result<ServiceNowResponse> {
        self.http.post_with_params(path, params, body).await
    }

    async fn put(&self, path: &str, body: Value) -> Result<ServiceNowResponse> {
        self.http.put(path, body).await
    }

    async fn patch(&self, path: &str, body: Value) -> Result<ServiceNowResponse> {
        self.http.patch(path, body).await
    }

    async fn patch_with_params(
        &self,
        path: &str,
        params: &[(String, String)],
        body: Value,
    ) -> Result<ServiceNowResponse> {
        self.http.patch_with_params(path, params, body).await
    }

    async fn delete(&self, path: &str) -> Result<ServiceNowResponse> {
        self.http.delete(path).await
    }

    fn selection(&self) -> TransportSelection {
        self.http.selection()
    }
}

impl GraphqlTransport {
    fn select_operation(
        &self,
        path: &str,
        params: &[(String, String)],
    ) -> Result<Option<GraphqlOperation>> {
        if !path.starts_with("/api/now/table/") {
            return Ok(None);
        }

        if let Some(operation) = GraphqlOperation::from_table_get(path, params)? {
            return Ok(Some(operation));
        }

        let Some(operation) = GraphqlOperation::from_table_list(path, params)? else {
            return Ok(None);
        };

        let threshold = self.http.selection().graphql_batch_threshold;
        let query_size = operation
            .request()
            .variables
            .get("fields")
            .and_then(Value::as_array)
            .map(|fields| fields.len())
            .unwrap_or(0);

        if query_size >= threshold
            || self.http.selection().preferred == super::TransportMode::Graphql
        {
            Ok(Some(operation))
        } else {
            Ok(None)
        }
    }

    async fn execute_operation(&self, operation: &GraphqlOperation) -> Result<ServiceNowResponse> {
        let request = operation.request();
        let response = self
            .http
            .post("/api/now/graphql", serde_json::to_value(request)?)
            .await?;
        let result = operation.extract_result(&response.result)?;

        Ok(ServiceNowResponse {
            status: response.status,
            result,
            total_count: response.total_count,
            links: response.links,
        })
    }
}
