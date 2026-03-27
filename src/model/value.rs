use serde::{Deserialize, Serialize};

/// Controls whether the API returns raw values, display values, or both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisplayValue {
    /// Return raw database values (default).
    #[default]
    Raw,
    /// Return display/rendered values.
    Display,
    /// Return both raw and display values.
    Both,
}

impl DisplayValue {
    /// Convert to the ServiceNow `sysparm_display_value` parameter value.
    pub fn as_param(&self) -> &str {
        match self {
            DisplayValue::Raw => "false",
            DisplayValue::Display => "true",
            DisplayValue::Both => "all",
        }
    }
}

/// A single field value from a ServiceNow record.
///
/// When `sysparm_display_value=all`, both `value` and `display_value` are populated.
/// Otherwise, only one is populated depending on the mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldValue {
    /// The raw database value.
    pub value: Option<serde_json::Value>,
    /// The human-readable display value.
    pub display_value: Option<String>,
    /// Reference link URL (for reference fields).
    pub link: Option<String>,
}

impl FieldValue {
    /// Create a FieldValue from a raw value only.
    pub fn from_raw(value: serde_json::Value) -> Self {
        Self {
            value: Some(value),
            display_value: None,
            link: None,
        }
    }

    /// Create a FieldValue from a display value only.
    pub fn from_display(display: String) -> Self {
        Self {
            value: None,
            display_value: Some(display),
            link: None,
        }
    }

    /// Get the value as a string, preferring display_value if available.
    pub fn as_str(&self) -> Option<&str> {
        self.display_value
            .as_deref()
            .or_else(|| self.value.as_ref().and_then(|v| v.as_str()))
    }

    /// Get the raw value as a string.
    pub fn raw_str(&self) -> Option<&str> {
        self.value.as_ref().and_then(|v| v.as_str())
    }

    /// Get the display value.
    pub fn display_str(&self) -> Option<&str> {
        self.display_value.as_deref()
    }
}

/// Parse a JSON value into a FieldValue, handling both simple and expanded formats.
///
/// When `sysparm_display_value=all`, ServiceNow returns:
/// ```json
/// { "display_value": "Display", "value": "raw", "link": "..." }
/// ```
///
/// Otherwise, it returns a simple value (string, number, null, etc.).
pub fn parse_field_value(json: serde_json::Value, mode: DisplayValue) -> FieldValue {
    match &json {
        serde_json::Value::Object(obj)
            if obj.contains_key("value") || obj.contains_key("display_value") =>
        {
            FieldValue {
                value: obj.get("value").cloned(),
                display_value: obj
                    .get("display_value")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                link: obj
                    .get("link")
                    .and_then(|v| v.as_str())
                    .map(String::from),
            }
        }
        _ => match mode {
            DisplayValue::Display => FieldValue {
                value: None,
                display_value: Some(json.as_str().unwrap_or_default().to_string()),
                link: None,
            },
            _ => FieldValue::from_raw(json),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_value() {
        let json = serde_json::json!("CHG0012345");
        let fv = parse_field_value(json, DisplayValue::Raw);
        assert_eq!(fv.raw_str(), Some("CHG0012345"));
        assert!(fv.display_value.is_none());
    }

    #[test]
    fn test_parse_expanded_value() {
        let json = serde_json::json!({
            "display_value": "New",
            "value": "1",
            "link": "https://instance.service-now.com/api/now/table/sys_choice/123"
        });
        let fv = parse_field_value(json, DisplayValue::Both);
        assert_eq!(fv.raw_str(), Some("1"));
        assert_eq!(fv.display_str(), Some("New"));
        assert!(fv.link.is_some());
    }

    #[test]
    fn test_as_str_prefers_display() {
        let fv = FieldValue {
            value: Some(serde_json::json!("1")),
            display_value: Some("New".to_string()),
            link: None,
        };
        assert_eq!(fv.as_str(), Some("New"));
    }

    #[test]
    fn test_display_value_param() {
        assert_eq!(DisplayValue::Raw.as_param(), "false");
        assert_eq!(DisplayValue::Display.as_param(), "true");
        assert_eq!(DisplayValue::Both.as_param(), "all");
    }
}
