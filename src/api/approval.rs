use serde_json::json;
use tracing::debug;

use crate::error::{Error, Result};
use crate::model::record::Record;
use crate::model::value::DisplayValue;
use crate::transport::TransportHandle;

/// The target state for an approval action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalAction {
    /// Set the approval state to "approved".
    Approve,
    /// Set the approval state to "rejected".
    Reject,
}

impl ApprovalAction {
    fn as_state(&self) -> &str {
        match self {
            ApprovalAction::Approve => "approved",
            ApprovalAction::Reject => "rejected",
        }
    }
}

/// Builder for approving or rejecting a record in ServiceNow.
///
/// Created via [`ServiceNowClient::approve`] or [`ServiceNowClient::reject`].
///
/// This queries the `sysapproval_approver` table for a pending approval
/// matching the given record and approver, then updates it.
///
/// # Security
///
/// The `approver_sys_id` must match the approval record's `approver` field.
/// If no matching pending approval is found, an error is returned — this
/// prevents accidentally approving on behalf of another user.
///
/// # Examples
///
/// ```no_run
/// # async fn example() -> servicenow_rs::error::Result<()> {
/// # let client: servicenow_rs::client::ServiceNowClient = todo!();
/// // Approve a change request.
/// let approval = client
///     .approve("change_request", "chg_sys_id", "your_user_sys_id")
///     .comment("Looks good, approved via API")
///     .execute()
///     .await?;
///
/// println!("Approval state: {}", approval.get_str("state").unwrap_or("?"));
///
/// // Reject with a reason.
/// let rejection = client
///     .reject("change_request", "chg_sys_id", "your_user_sys_id")
///     .comment("Missing test plan — please revise")
///     .execute()
///     .await?;
/// # Ok(())
/// # }
/// ```
pub struct ApprovalBuilder {
    transport: TransportHandle,
    source_table: String,
    record_sys_id: String,
    approver_sys_id: String,
    action: ApprovalAction,
    comment: Option<String>,
}

impl ApprovalBuilder {
    pub(crate) fn new(
        transport: TransportHandle,
        source_table: &str,
        record_sys_id: &str,
        approver_sys_id: &str,
        action: ApprovalAction,
    ) -> Self {
        Self {
            transport,
            source_table: source_table.to_string(),
            record_sys_id: record_sys_id.to_string(),
            approver_sys_id: approver_sys_id.to_string(),
            action,
            comment: None,
        }
    }

    /// Add an optional comment to the approval or rejection.
    pub fn comment(mut self, comment: &str) -> Self {
        self.comment = Some(comment.to_string());
        self
    }

    /// Execute the approval or rejection.
    ///
    /// 1. Queries `sysapproval_approver` for a pending approval matching the
    ///    record and approver.
    /// 2. PATCHes the approval record with the new state (and optional comment).
    /// 3. Returns the updated approval record.
    pub async fn execute(self) -> Result<Record> {
        let action_str = self.action.as_state();

        debug!(
            source_table = self.source_table,
            record_sys_id = self.record_sys_id,
            approver_sys_id = self.approver_sys_id,
            action = action_str,
            "looking up pending approval"
        );

        let path = format!("{}/sysapproval_approver", crate::api::table::TABLE_API_PATH);
        // Step 1: Find the pending approval record. Prefer document_id because
        // that matches how sysapproval_approver is queried elsewhere in the CLI,
        // but fall back to sysapproval for older instances.
        let lookup_queries = [
            format!(
                "document_id={}^approver={}^state=requested",
                self.record_sys_id, self.approver_sys_id
            ),
            format!(
                "sysapproval={}^approver={}^state=requested",
                self.record_sys_id, self.approver_sys_id
            ),
        ];
        let approval_sys_id = {
            let mut match_id = None;
            for query in lookup_queries {
                let params = vec![
                    ("sysparm_query".to_string(), query.clone()),
                    (
                        "sysparm_fields".to_string(),
                        "sys_id,document_id,sysapproval,approver,state,source_table,comments"
                            .to_string(),
                    ),
                    ("sysparm_display_value".to_string(), "false".to_string()),
                    ("sysparm_limit".to_string(), "1".to_string()),
                ];

                debug!(query = query, "querying pending approval");

                let response = self.transport.get(&path, &params).await?;
                match_id = response
                    .result
                    .as_array()
                    .and_then(|arr| arr.first())
                    .and_then(|r| r.get("sys_id"))
                    .and_then(|v| v.as_str())
                    .map(String::from);

                if match_id.is_some() {
                    break;
                }
            }
            match_id
        }
        .ok_or_else(|| Error::Api {
            status: 404,
            message: format!(
                "no pending approval found for {} {} with approver {}",
                self.source_table, self.record_sys_id, self.approver_sys_id
            ),
            detail: Some(
                "verify the record has a pending approval assigned to this user".to_string(),
            ),
        })?;

        debug!(
            approval_sys_id = approval_sys_id,
            action = action_str,
            "found approval record, updating"
        );

        // Step 2: PATCH the approval record.
        let update_path = format!(
            "{}/sysapproval_approver/{}",
            crate::api::table::TABLE_API_PATH,
            approval_sys_id
        );

        let mut body = json!({ "state": action_str });
        if let Some(ref comment) = self.comment {
            body["comments"] = json!(comment);
        }

        let update_params = vec![
            ("sysparm_display_value".to_string(), "true".to_string()),
            (
                "sysparm_fields".to_string(),
                "sys_id,sysapproval,approver,state,source_table,comments,sys_updated_on"
                    .to_string(),
            ),
        ];

        let update_response = self
            .transport
            .patch_with_params(&update_path, &update_params, body)
            .await?;

        Record::from_json(
            "sysapproval_approver",
            &update_response.result,
            DisplayValue::Display,
        )
        .ok_or_else(|| Error::Api {
            status: 200,
            message: "failed to parse updated approval record from response".to_string(),
            detail: None,
        })
    }
}
