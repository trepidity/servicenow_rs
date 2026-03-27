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
    /// Parsed Link header pagination URLs.
    pub links: PaginationLinks,
}

/// Parsed RFC 5988 Link header from ServiceNow pagination responses.
#[derive(Debug, Default, Clone)]
pub struct PaginationLinks {
    /// URL for the first page.
    pub first: Option<String>,
    /// URL for the previous page.
    pub prev: Option<String>,
    /// URL for the next page.
    pub next: Option<String>,
    /// URL for the last page.
    pub last: Option<String>,
}

impl PaginationLinks {
    /// Whether there is a next page available.
    pub fn has_next(&self) -> bool {
        self.next.is_some()
    }
}

/// Parse an RFC 5988 Link header value into PaginationLinks.
///
/// Format: `<URL>;rel="first", <URL>;rel="next", ...`
pub fn parse_link_header(header: &str) -> PaginationLinks {
    let mut links = PaginationLinks::default();

    for part in header.split(',') {
        let part = part.trim();
        // Extract URL between < and >.
        let url = part
            .find('<')
            .and_then(|start| part.find('>').map(|end| &part[start + 1..end]));
        // Extract rel value.
        let rel = part.find("rel=\"").map(|start| {
            let rest = &part[start + 5..];
            rest.split('"').next().unwrap_or("")
        });

        if let (Some(url), Some(rel)) = (url, rel) {
            match rel {
                "first" => links.first = Some(url.to_string()),
                "prev" => links.prev = Some(url.to_string()),
                "next" => links.next = Some(url.to_string()),
                "last" => links.last = Some(url.to_string()),
                _ => {}
            }
        }
    }

    links
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

    // Parse Link header for pagination.
    let links = response
        .headers()
        .get("Link")
        .and_then(|v| v.to_str().ok())
        .map(parse_link_header)
        .unwrap_or_default();

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
            links,
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
    let result = json.get("result").cloned().unwrap_or(Value::Null);

    Ok(ServiceNowResponse {
        status,
        result,
        total_count,
        links,
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

    #[test]
    fn test_parse_link_header() {
        let header = r#"<https://instance.service-now.com/api/now/table/incident?sysparm_limit=3&sysparm_offset=0>;rel="first",<https://instance.service-now.com/api/now/table/incident?sysparm_limit=3&sysparm_offset=3>;rel="next",<https://instance.service-now.com/api/now/table/incident?sysparm_limit=3&sysparm_offset=99>;rel="last""#;
        let links = parse_link_header(header);
        assert!(links.first.is_some());
        assert!(links.next.is_some());
        assert!(links.last.is_some());
        assert!(links.prev.is_none());
        assert!(links.has_next());
    }

    #[test]
    fn test_parse_link_header_with_prev() {
        let header = r#"<https://x.com?offset=0>;rel="first",<https://x.com?offset=0>;rel="prev",<https://x.com?offset=6>;rel="next",<https://x.com?offset=99>;rel="last""#;
        let links = parse_link_header(header);
        assert!(links.first.is_some());
        assert!(links.prev.is_some());
        assert!(links.next.is_some());
        assert!(links.last.is_some());
    }

    #[test]
    fn test_parse_link_header_empty() {
        let links = parse_link_header("");
        assert!(!links.has_next());
    }
}
