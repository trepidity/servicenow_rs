use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;
use serde_json::Value;
use tracing::{debug, error};
use url::Url;

use crate::auth::Authenticator;
use crate::error::{Error, Result};

use super::response::{self, ServiceNowResponse};
use super::retry::{self, RateLimiter, RetryConfig};

/// HTTP transport layer wrapping reqwest with authentication, retry, and rate limiting.
#[derive(Debug)]
pub struct HttpTransport {
    client: Client,
    base_url: Url,
    authenticator: Arc<dyn Authenticator>,
    retry_config: RetryConfig,
    rate_limiter: Option<RateLimiter>,
}

impl HttpTransport {
    /// Create a new HttpTransport.
    pub fn new(
        base_url: Url,
        authenticator: Arc<dyn Authenticator>,
        timeout: Duration,
        retry_config: RetryConfig,
        rate_limiter: Option<RateLimiter>,
    ) -> Result<Self> {
        let mut builder = Client::builder()
            .timeout(timeout)
            .cookie_store(authenticator.supports_session());

        // Set a user agent.
        builder = builder.user_agent(format!(
            "servicenow_rs/{} ({})",
            env!("CARGO_PKG_VERSION"),
            authenticator.method_name()
        ));

        let client = builder.build().map_err(Error::Http)?;

        Ok(Self {
            client,
            base_url,
            authenticator,
            retry_config,
            rate_limiter,
        })
    }

    /// Build a full URL from a path (e.g., "/api/now/table/change_request").
    fn url(&self, path: &str) -> Result<Url> {
        self.base_url.join(path).map_err(Error::UrlParse)
    }

    /// Perform a GET request with query parameters.
    pub async fn get(
        &self,
        path: &str,
        params: &[(String, String)],
    ) -> Result<ServiceNowResponse> {
        self.request(Method::Get, path, params, None).await
    }

    /// Perform a POST request with a JSON body.
    pub async fn post(&self, path: &str, body: Value) -> Result<ServiceNowResponse> {
        self.request(Method::Post, path, &[], Some(body)).await
    }

    /// Perform a PUT request with a JSON body.
    pub async fn put(&self, path: &str, body: Value) -> Result<ServiceNowResponse> {
        self.request(Method::Put, path, &[], Some(body)).await
    }

    /// Perform a PATCH request with a JSON body.
    pub async fn patch(&self, path: &str, body: Value) -> Result<ServiceNowResponse> {
        self.request(Method::Patch, path, &[], Some(body)).await
    }

    /// Perform a DELETE request.
    pub async fn delete(&self, path: &str) -> Result<ServiceNowResponse> {
        self.request(Method::Delete, path, &[], None).await
    }

    /// Core request method with retry logic.
    async fn request(
        &self,
        method: Method,
        path: &str,
        params: &[(String, String)],
        body: Option<Value>,
    ) -> Result<ServiceNowResponse> {
        let url = self.url(path)?;
        let mut last_error: Option<Error> = None;

        for attempt in 0..=self.retry_config.max_retries {
            // Rate limiting.
            if let Some(ref limiter) = self.rate_limiter {
                limiter.acquire().await;
            }

            // Build the request.
            let mut req = match method {
                Method::Get => self.client.get(url.clone()),
                Method::Post => self.client.post(url.clone()),
                Method::Put => self.client.put(url.clone()),
                Method::Patch => self.client.patch(url.clone()),
                Method::Delete => self.client.delete(url.clone()),
            };

            // Add query parameters.
            if !params.is_empty() {
                req = req.query(params);
            }

            // Add JSON body.
            if let Some(ref body) = body {
                req = req
                    .header("Content-Type", "application/json")
                    .header("Accept", "application/json")
                    .json(body);
            } else {
                req = req.header("Accept", "application/json");
            }

            // Authenticate.
            req = self.authenticator.authenticate(req).await?;

            debug!(
                method = ?method,
                url = %url,
                attempt = attempt,
                "sending request"
            );

            // Execute.
            let result = req.send().await;

            match result {
                Ok(resp) => {
                    let status = resp.status().as_u16();

                    // If auth failed and we can refresh, try that.
                    if status == 401 && attempt < self.retry_config.max_retries {
                        if let Err(e) = self.authenticator.refresh().await {
                            debug!("auth refresh failed: {}", e);
                        } else {
                            debug!("auth refreshed, retrying");
                            continue;
                        }
                    }

                    match response::parse_response(resp).await {
                        Ok(parsed) => return Ok(parsed),
                        Err(Error::RateLimited { retry_after }) => {
                            if attempt < self.retry_config.max_retries {
                                retry::retry_delay(
                                    &self.retry_config,
                                    attempt,
                                    retry_after,
                                )
                                .await;
                                last_error = Some(Error::RateLimited { retry_after });
                                continue;
                            }
                            return Err(Error::RateLimited { retry_after });
                        }
                        Err(e) => {
                            if self.retry_config.should_retry_status(status)
                                && attempt < self.retry_config.max_retries
                            {
                                retry::retry_delay(&self.retry_config, attempt, None).await;
                                last_error = Some(e);
                                continue;
                            }
                            return Err(e);
                        }
                    }
                }
                Err(e) => {
                    error!(attempt = attempt, error = %e, "request failed");
                    if attempt < self.retry_config.max_retries {
                        retry::retry_delay(&self.retry_config, attempt, None).await;
                        last_error = Some(Error::Http(e));
                        continue;
                    }
                    return Err(Error::Http(e));
                }
            }
        }

        Err(last_error.unwrap_or_else(|| Error::Api {
            status: 0,
            message: "max retries exceeded".to_string(),
            detail: None,
        }))
    }
}

#[derive(Debug, Clone, Copy)]
enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}
