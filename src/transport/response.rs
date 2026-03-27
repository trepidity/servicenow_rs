use serde_json::Value;

use crate::error::{Error, Result};

/// Parsed response from the ServiceNow API.
#[derive(Debug)]
pub struct ServiceNowResponse {
    /// HTTP status code.
    pub status: u16,
    /// The "result" field from the response body.
    pub result: Value,
    /// Total count from X-Total-Count header, if present.
    pub total_count: Option<u64>,
}

/// Parse a reqwest Response into a ServiceNowResponse.
///
/// Handles error detection, extracting the "result" payload, and
/// reading pagination headers.
pub async fn parse_response(response: reqwest::Response) -> Result<ServiceNowResponse> {
    let status = response.status().as_u16();

    // Extract total count from header before consuming the body.
    let total_count = response
        .headers()
        .get("X-Total-Count")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());

    // Check for auth failures.
    if status == 401 || status == 403 {
        let body = response.text().await.unwrap_or_default();
        return Err(Error::Auth {
            message: extract_error_message(&body).unwrap_or_else(|| {
                if status == 401 {
                    "unauthorized".to_string()
                } else {
                    "forbidden".to_string()
                }
            }),
            status: Some(status),
        });
    }

    // Check for rate limiting.
    if status == 429 {
        let retry_after = response
            .headers()
            .get("Retry-After")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());
        return Err(Error::RateLimited { retry_after });
    }

    // Read body.
    let body = response.text().await.map_err(Error::Http)?;

    // For DELETE with 204 No Content.
    if status == 204 || body.is_empty() {
        return Ok(ServiceNowResponse {
            status,
            result: Value::Null,
            total_count,
        });
    }

    // Parse JSON.
    let json: Value = serde_json::from_str(&body).map_err(|e| Error::Api {
        status,
        message: format!("failed to parse response JSON: {}", e),
        detail: Some(body.chars().take(500).collect()),
    })?;

    // Check for error response.
    if let Some(error_obj) = json.get("error") {
        let message = error_obj
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error")
            .to_string();
        let detail = error_obj
            .get("detail")
            .and_then(|v| v.as_str())
            .map(String::from);
        return Err(Error::Api {
            status,
            message,
            detail,
        });
    }

    // Non-2xx without an error object.
    if status >= 400 {
        return Err(Error::Api {
            status,
            message: format!("HTTP {}", status),
            detail: Some(body.chars().take(500).collect()),
        });
    }

    // Extract "result" field.
    let result = json
        .get("result")
        .cloned()
        .unwrap_or(Value::Null);

    Ok(ServiceNowResponse {
        status,
        result,
        total_count,
    })
}

/// Try to extract an error message from a response body string.
fn extract_error_message(body: &str) -> Option<String> {
    let json: Value = serde_json::from_str(body).ok()?;
    json.get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
        .map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_error_message() {
        let body = r#"{"error":{"message":"Record not found","detail":"Could not find record"}}"#;
        assert_eq!(
            extract_error_message(body),
            Some("Record not found".to_string())
        );
    }

    #[test]
    fn test_extract_error_message_invalid_json() {
        assert_eq!(extract_error_message("not json"), None);
    }

    #[test]
    fn test_extract_error_message_no_error_field() {
        assert_eq!(extract_error_message(r#"{"result": []}"#), None);
    }
}
