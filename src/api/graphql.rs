use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphqlRequest {
    pub query: String,
    #[serde(default)]
    pub variables: Value,
}

impl GraphqlRequest {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            variables: json!({}),
        }
    }

    pub fn variables(mut self, variables: Value) -> Self {
        self.variables = variables;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphqlOperation {
    TableList {
        table: String,
        query: Option<String>,
        fields: Vec<String>,
        limit: Option<u32>,
        offset: Option<u32>,
        display_value: String,
        exclude_reference_link: bool,
    },
    TableGet {
        table: String,
        sys_id: String,
        fields: Vec<String>,
        display_value: String,
        exclude_reference_link: bool,
    },
}

impl GraphqlOperation {
    pub fn from_table_get(path: &str, params: &[(String, String)]) -> Result<Option<Self>> {
        let table_path = path
            .strip_prefix("/api/now/table/")
            .ok_or_else(|| Error::Query(format!("unsupported GraphQL path '{path}'")))?;
        let mut segments = table_path.split('/');
        let Some(table) = segments.next().filter(|segment| !segment.is_empty()) else {
            return Ok(None);
        };
        let Some(sys_id) = segments.next() else {
            return Ok(None);
        };
        if segments.next().is_some() {
            return Ok(None);
        }

        let mut display_value = "false".to_string();
        let mut exclude_reference_link = false;
        let mut fields = Vec::new();

        for (key, value) in params {
            match key.as_str() {
                "sysparm_fields" => fields = split_csv(value),
                "sysparm_display_value" => display_value = value.clone(),
                "sysparm_exclude_reference_link" => exclude_reference_link = value == "true",
                unsupported
                    if unsupported == "sysparm_query"
                        || unsupported == "sysparm_limit"
                        || unsupported == "sysparm_offset"
                        || unsupported == "sysparm_no_count" =>
                {
                    return Ok(None);
                }
                _ => {}
            }
        }

        Ok(Some(Self::TableGet {
            table: table.to_string(),
            sys_id: sys_id.to_string(),
            fields,
            display_value,
            exclude_reference_link,
        }))
    }

    pub fn from_table_list(path: &str, params: &[(String, String)]) -> Result<Option<Self>> {
        let table = path
            .strip_prefix("/api/now/table/")
            .ok_or_else(|| Error::Query(format!("unsupported GraphQL path '{path}'")))?;
        if table.is_empty() || table.contains('/') {
            return Ok(None);
        }

        let mut query = None;
        let mut fields = Vec::new();
        let mut limit = None;
        let mut offset = None;
        let mut display_value = "false".to_string();
        let mut exclude_reference_link = false;

        for (key, value) in params {
            match key.as_str() {
                "sysparm_query" => query = Some(value.clone()),
                "sysparm_fields" => fields = split_csv(value),
                "sysparm_limit" => limit = value.parse::<u32>().ok(),
                "sysparm_offset" => offset = value.parse::<u32>().ok(),
                "sysparm_display_value" => display_value = value.clone(),
                "sysparm_exclude_reference_link" => exclude_reference_link = value == "true",
                "sysparm_no_count" => {}
                _ => return Ok(None),
            }
        }

        Ok(Some(Self::TableList {
            table: table.to_string(),
            query,
            fields,
            limit,
            offset,
            display_value,
            exclude_reference_link,
        }))
    }

    pub fn request(&self) -> GraphqlRequest {
        match self {
            Self::TableList {
                table,
                query,
                fields,
                limit,
                offset,
                display_value,
                exclude_reference_link,
            } => GraphqlRequest::new(
                "query TableList($table: String!, $query: String, $fields: [String!], $limit: Int, $offset: Int, $displayValue: String!, $excludeReferenceLink: Boolean!) { table(name: $table) { records(query: $query, fields: $fields, limit: $limit, offset: $offset, displayValue: $displayValue, excludeReferenceLink: $excludeReferenceLink) } }",
            )
            .variables(json!({
                "table": table,
                "query": query,
                "fields": fields,
                "limit": limit,
                "offset": offset,
                "displayValue": display_value,
                "excludeReferenceLink": exclude_reference_link,
            })),
            Self::TableGet {
                table,
                sys_id,
                fields,
                display_value,
                exclude_reference_link,
            } => GraphqlRequest::new(
                "query TableGet($table: String!, $sysId: String!, $fields: [String!], $displayValue: String!, $excludeReferenceLink: Boolean!) { table(name: $table) { record(sysId: $sysId, fields: $fields, displayValue: $displayValue, excludeReferenceLink: $excludeReferenceLink) } }",
            )
            .variables(json!({
                "table": table,
                "sysId": sys_id,
                "fields": fields,
                "displayValue": display_value,
                "excludeReferenceLink": exclude_reference_link,
            })),
        }
    }

    pub fn extract_result(&self, data: &Value) -> Result<Value> {
        let table = data.get("table").ok_or_else(|| Error::Api {
            status: 200,
            message: "missing `table` in GraphQL response".to_string(),
            detail: Some(data.to_string()),
        })?;

        match self {
            Self::TableList { .. } => Ok(table
                .get("records")
                .cloned()
                .unwrap_or(Value::Array(vec![]))),
            Self::TableGet { .. } => table.get("record").cloned().ok_or_else(|| Error::Api {
                status: 200,
                message: "missing `record` in GraphQL response".to_string(),
                detail: Some(data.to_string()),
            }),
        }
    }
}

fn split_csv(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_table_list_operation() {
        let operation = GraphqlOperation::from_table_list(
            "/api/now/table/change_task",
            &[
                (
                    "sysparm_query".to_string(),
                    "change_request=abc".to_string(),
                ),
                ("sysparm_fields".to_string(), "sys_id,number".to_string()),
                ("sysparm_limit".to_string(), "25".to_string()),
            ],
        )
        .expect("operation")
        .expect("supported");

        match operation {
            GraphqlOperation::TableList {
                table,
                limit,
                fields,
                ..
            } => {
                assert_eq!(table, "change_task");
                assert_eq!(limit, Some(25));
                assert_eq!(fields, vec!["sys_id".to_string(), "number".to_string()]);
            }
            other => panic!("unexpected operation: {other:?}"),
        }
    }

    #[test]
    fn extracts_table_get_result() {
        let operation = GraphqlOperation::TableGet {
            table: "incident".to_string(),
            sys_id: "abc".to_string(),
            fields: vec!["number".to_string()],
            display_value: "all".to_string(),
            exclude_reference_link: true,
        };
        let result = operation
            .extract_result(&json!({ "table": { "record": { "number": "INC001" } } }))
            .expect("result");
        assert_eq!(result["number"], "INC001");
    }
}
