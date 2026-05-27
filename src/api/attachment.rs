use std::path::Path;

use serde_json::Value;

use crate::error::{Error, Result};
use crate::model::AttachmentMetadata;
use crate::query::builder::validate_identifier;
use crate::transport::TransportHandle;

const ATTACHMENT_API_PATH: &str = "/api/now/attachment";
const ATTACHMENT_FILE_API_PATH: &str = "/api/now/attachment/file";
const ATTACHMENT_FIELDS: &str = "sys_id,file_name,content_type,size_bytes,size_compressed,compressed,state,table_name,table_sys_id,download_link,sys_created_on,sys_created_by,sys_updated_on,sys_updated_by";

/// ServiceNow Attachment API operations.
#[derive(Debug, Clone)]
pub struct AttachmentApi {
    transport: TransportHandle,
}

impl AttachmentApi {
    pub(crate) fn new(transport: TransportHandle) -> Self {
        Self { transport }
    }

    /// List attachments associated with one ServiceNow record.
    pub async fn list_for_record(
        &self,
        table_name: &str,
        table_sys_id: &str,
    ) -> Result<Vec<AttachmentMetadata>> {
        validate_record_target(table_name, table_sys_id)?;

        let params = vec![
            (
                "sysparm_query".to_string(),
                format!("table_name={table_name}^table_sys_id={table_sys_id}"),
            ),
            ("sysparm_fields".to_string(), ATTACHMENT_FIELDS.to_string()),
        ];
        let response = self.transport.get(ATTACHMENT_API_PATH, &params).await?;
        parse_attachment_list(response.result)
    }

    /// Upload bytes as an attachment to one ServiceNow record.
    pub async fn upload_bytes(
        &self,
        table_name: &str,
        table_sys_id: &str,
        file_name: &str,
        content_type: &str,
        body: Vec<u8>,
    ) -> Result<AttachmentMetadata> {
        validate_record_target(table_name, table_sys_id)?;
        validate_upload(file_name, content_type, body.len())?;

        let params = vec![
            ("table_name".to_string(), table_name.to_string()),
            ("table_sys_id".to_string(), table_sys_id.to_string()),
            ("file_name".to_string(), file_name.to_string()),
        ];
        let response = self
            .transport
            .post_bytes(ATTACHMENT_FILE_API_PATH, &params, content_type, body)
            .await?;
        parse_attachment(response.result)
    }

    /// Upload a local file as an attachment to one ServiceNow record.
    pub async fn upload_file(
        &self,
        table_name: &str,
        table_sys_id: &str,
        path: impl AsRef<Path>,
        file_name: Option<&str>,
        content_type: Option<&str>,
    ) -> Result<AttachmentMetadata> {
        let path = path.as_ref();
        let inferred_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                Error::Query(format!(
                    "attachment path '{}' does not have a valid file name",
                    path.display()
                ))
            })?;
        let file_name = file_name.unwrap_or(inferred_name);
        let content_type = content_type.unwrap_or("application/octet-stream");
        let body = tokio::fs::read(path).await?;

        self.upload_bytes(table_name, table_sys_id, file_name, content_type, body)
            .await
    }
}

fn validate_record_target(table_name: &str, table_sys_id: &str) -> Result<()> {
    validate_identifier(table_name, "table name")?;
    validate_non_empty(table_sys_id, "table_sys_id")
}

fn validate_upload(file_name: &str, content_type: &str, size: usize) -> Result<()> {
    validate_non_empty(file_name, "file_name")?;
    if file_name.contains('/') || file_name.contains('\\') {
        return Err(Error::Query(
            "attachment file_name must not contain path separators".to_string(),
        ));
    }
    validate_non_empty(content_type, "content_type")?;
    if size == 0 {
        return Err(Error::Query(
            "attachment upload body must not be empty".to_string(),
        ));
    }
    Ok(())
}

fn validate_non_empty(value: &str, name: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::Query(format!("{name} cannot be empty")));
    }
    Ok(())
}

fn parse_attachment_list(value: Value) -> Result<Vec<AttachmentMetadata>> {
    serde_json::from_value(value).map_err(Error::Json)
}

fn parse_attachment(value: Value) -> Result<AttachmentMetadata> {
    serde_json::from_value(value).map_err(Error::Json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_rejects_path_separator_in_file_name() {
        let err = validate_upload("../evidence.txt", "text/plain", 1).expect_err("invalid name");
        assert!(matches!(err, Error::Query(message) if message.contains("path separators")));
    }

    #[test]
    fn upload_rejects_empty_body() {
        let err = validate_upload("evidence.txt", "text/plain", 0).expect_err("empty body");
        assert!(matches!(err, Error::Query(message) if message.contains("must not be empty")));
    }
}
