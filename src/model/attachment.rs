use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// Metadata returned by the ServiceNow Attachment API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AttachmentMetadata {
    pub sys_id: String,
    pub file_name: String,
    pub table_name: String,
    pub table_sys_id: String,
    pub content_type: String,
    #[serde(default, deserialize_with = "optional_u64_from_string")]
    pub size_bytes: Option<u64>,
    #[serde(default, deserialize_with = "optional_u64_from_string")]
    pub size_compressed: Option<u64>,
    #[serde(default)]
    pub compressed: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub download_link: Option<String>,
    #[serde(default)]
    pub sys_created_on: Option<String>,
    #[serde(default)]
    pub sys_created_by: Option<String>,
    #[serde(default)]
    pub sys_updated_on: Option<String>,
    #[serde(default)]
    pub sys_updated_by: Option<String>,
}

fn optional_u64_from_string<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    match Option::<Value>::deserialize(deserializer)? {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number
            .as_u64()
            .map(Some)
            .ok_or_else(|| serde::de::Error::custom("expected unsigned integer")),
        Some(Value::String(value)) => {
            let value = value.trim();
            if value.is_empty() {
                Ok(None)
            } else {
                value
                    .parse::<u64>()
                    .map(Some)
                    .map_err(serde::de::Error::custom)
            }
        }
        Some(other) => Err(serde::de::Error::custom(format!(
            "expected integer string, got {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_attachment_metadata_number_strings() {
        let metadata: AttachmentMetadata = serde_json::from_value(serde_json::json!({
            "sys_id": "att-sys",
            "file_name": "evidence.jpeg",
            "table_name": "change_request",
            "table_sys_id": "chg-sys",
            "content_type": "image/jpeg",
            "size_bytes": "83338",
            "size_compressed": "69391",
            "download_link": "https://example.service-now.com/api/now/attachment/att-sys/file"
        }))
        .expect("metadata");

        assert_eq!(metadata.size_bytes, Some(83338));
        assert_eq!(metadata.size_compressed, Some(69391));
        assert_eq!(metadata.file_name, "evidence.jpeg");
    }
}
