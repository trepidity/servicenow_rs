use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering as AtomicOrdering},
    Arc,
};
use std::time::Duration;
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

use servicenow_rs::prelude::*;

const TASK_SLA_DEFAULT_FIELDS: &str = "sys_id,task,sla,stage,active,has_breached,start_time,end_time,planned_end_time,original_breach_time,percentage,time_left,business_percentage,business_time_left,business_duration,duration,schedule";

/// Helper to build a client pointing at a wiremock server.
async fn test_client(server: &MockServer) -> ServiceNowClient {
    ServiceNowClient::builder()
        .instance(server.uri())
        .auth(BasicAuth::new("test_user", "test_pass"))
        .schema_release("xanadu")
        .allow_http() // wiremock uses http://
        .build()
        .await
        .expect("failed to build test client")
}

async fn graphql_client(server: &MockServer) -> ServiceNowClient {
    ServiceNowClient::builder()
        .instance(server.uri())
        .auth(BasicAuth::new("test_user", "test_pass"))
        .schema_release("xanadu")
        .allow_http()
        .transport_mode(servicenow_rs::transport::TransportMode::Graphql)
        .graphql_batch_threshold(1)
        .build()
        .await
        .expect("failed to build GraphQL client")
}

#[tokio::test]
async fn test_basic_auth_without_session_does_not_reuse_cookies() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(
            ResponseTemplate::new(200)
                .append_header("Set-Cookie", "JSESSIONID=abc; Path=/")
                .set_body_json(json!({ "result": [] })),
        )
        .mount(&server)
        .await;

    let client = ServiceNowClient::builder()
        .instance(server.uri())
        .auth(BasicAuth::new("test_user", "test_pass").without_session())
        .allow_http()
        .build()
        .await
        .expect("failed to build test client");

    client
        .table("incident")
        .limit(1)
        .execute()
        .await
        .expect("first query");
    client
        .table("incident")
        .limit(1)
        .execute()
        .await
        .expect("second query");

    let requests = requests_for_path(&server, "/api/now/table/incident").await;
    assert_eq!(requests.len(), 2);
    assert!(requests[1].headers.get("cookie").is_none());
}

fn fixture_sys_id(n: usize) -> String {
    format!("{:032x}", n + 1)
}

fn as_str_refs(values: &[String]) -> Vec<&str> {
    values.iter().map(String::as_str).collect()
}

fn request_query(request: &wiremock::Request) -> Option<String> {
    request
        .url
        .query_pairs()
        .find(|(key, _)| key == "sysparm_query")
        .map(|(_, value)| value.into_owned())
}

fn request_query_param(request: &wiremock::Request, name: &str) -> Option<String> {
    request
        .url
        .query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

fn in_list_values(query: &str, foreign_key: &str) -> Vec<String> {
    let prefix = format!("{}IN", foreign_key);
    query
        .split('^')
        .find_map(|part| part.strip_prefix(&prefix))
        .unwrap_or_default()
        .split(',')
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

async fn requests_for_path(server: &MockServer, api_path: &str) -> Vec<wiremock::Request> {
    server
        .received_requests()
        .await
        .expect("request recording enabled")
        .into_iter()
        .filter(|request| request.url.path() == api_path)
        .collect()
}

struct TaskSlaRowFixture<'a> {
    sys_id: &'a str,
    task_sys_id: &'a str,
    task_display: &'a str,
    sla_sys_id: &'a str,
    sla_display: &'a str,
    stage: &'a str,
    active: bool,
    has_breached: bool,
    planned_end_time: &'a str,
    business_percentage: &'a str,
}

macro_rules! task_sla_row {
    (
        $sys_id:expr,
        $task_sys_id:expr,
        $task_display:expr,
        $sla_sys_id:expr,
        $sla_display:expr,
        $stage:expr,
        $active:expr,
        $has_breached:expr,
        $planned_end_time:expr,
        $business_percentage:expr $(,)?
    ) => {
        task_sla_row(TaskSlaRowFixture {
            sys_id: $sys_id,
            task_sys_id: $task_sys_id,
            task_display: $task_display,
            sla_sys_id: $sla_sys_id,
            sla_display: $sla_display,
            stage: $stage,
            active: $active,
            has_breached: $has_breached,
            planned_end_time: $planned_end_time,
            business_percentage: $business_percentage,
        })
    };
}

fn task_sla_row(row: TaskSlaRowFixture<'_>) -> Value {
    json!({
        "sys_id": { "value": row.sys_id, "display_value": row.sys_id },
        "task": { "value": row.task_sys_id, "display_value": row.task_display },
        "sla": { "value": row.sla_sys_id, "display_value": row.sla_display },
        "stage": { "value": row.stage, "display_value": row.stage },
        "active": { "value": row.active.to_string(), "display_value": row.active.to_string() },
        "has_breached": { "value": row.has_breached.to_string(), "display_value": row.has_breached.to_string() },
        "start_time": { "value": "2026-05-06 12:00:00", "display_value": "2026-05-06 12:00:00" },
        "end_time": { "value": "", "display_value": "" },
        "planned_end_time": { "value": row.planned_end_time, "display_value": row.planned_end_time },
        "original_breach_time": { "value": row.planned_end_time, "display_value": row.planned_end_time },
        "percentage": { "value": "42.5", "display_value": "42.5" },
        "time_left": { "value": "1970-01-01 04:00:00", "display_value": "4 Hours" },
        "business_percentage": { "value": row.business_percentage, "display_value": row.business_percentage },
        "business_time_left": { "value": "1970-01-01 03:00:00", "display_value": "3 Hours" },
        "business_duration": { "value": "1970-01-01 01:00:00", "display_value": "1 Hour" },
        "duration": { "value": "1970-01-01 02:00:00", "display_value": "2 Hours" },
        "schedule": { "value": "schedule_sys_id", "display_value": "24 x 7" }
    })
}

#[tokio::test]
async fn test_simple_query() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_query", "state=1"))
        .and(query_param("sysparm_limit", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                {
                    "sys_id": "abc123",
                    "number": "INC0010001",
                    "state": "1",
                    "short_description": "Network outage"
                },
                {
                    "sys_id": "def456",
                    "number": "INC0010002",
                    "state": "1",
                    "short_description": "Server down"
                }
            ]
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let result = client
        .table("incident")
        .equals("state", "1")
        .limit(5)
        .execute()
        .await
        .expect("query failed");

    assert_eq!(result.len(), 2);
    assert_eq!(result.records[0].get_str("number"), Some("INC0010001"));
    assert_eq!(
        result.records[1].get_str("short_description"),
        Some("Server down")
    );
}

#[tokio::test]
async fn test_graphql_table_query_executes_when_preferred() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "table": {
                    "records": [
                        {
                            "sys_id": "abc123",
                            "number": { "value": "CTASK001", "display_value": "CTASK001" },
                            "short_description": { "value": "Patch cluster", "display_value": "Patch cluster" },
                            "state": { "value": "1", "display_value": "Open" }
                        }
                    ]
                }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = graphql_client(&server).await;
    let result = client
        .table("change_task")
        .equals("change_request", "chg-sys")
        .fields(&["sys_id", "number", "short_description", "state"])
        .display_value(DisplayValue::Both)
        .limit(10)
        .execute()
        .await
        .expect("graphql query failed");

    assert_eq!(result.records.len(), 1);
    assert_eq!(result.records[0].get_str("number"), Some("CTASK001"));
    assert_eq!(
        client.transport_selection().preferred,
        servicenow_rs::transport::TransportMode::Graphql
    );
}

#[tokio::test]
async fn test_graphql_falls_back_to_rest_on_error() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({
            "errors": [{ "message": "GraphQL unavailable" }]
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/change_task"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                {
                    "sys_id": "abc123",
                    "number": "CTASK001",
                    "short_description": "Patch cluster",
                    "state": "1"
                }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = graphql_client(&server).await;
    let result = client
        .table("change_task")
        .equals("change_request", "chg-sys")
        .fields(&["sys_id", "number", "short_description", "state"])
        .display_value(DisplayValue::Both)
        .limit(10)
        .execute()
        .await
        .expect("fallback query failed");

    assert_eq!(result.records[0].get_str("number"), Some("CTASK001"));
}

#[tokio::test]
async fn test_get_single_record() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/change_request/sys123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {
                "sys_id": "sys123",
                "number": "CHG0012345",
                "state": "1",
                "short_description": "Deploy update"
            }
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let record = client
        .table("change_request")
        .get("sys123")
        .await
        .expect("get failed");

    assert_eq!(record.sys_id, "sys123");
    assert_eq!(record.get_str("number"), Some("CHG0012345"));
}

#[tokio::test]
async fn test_create_record() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/now/table/incident"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "result": {
                "sys_id": "new123",
                "number": "INC0099999",
                "short_description": "Test incident",
                "urgency": "2"
            }
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let record = client
        .table("incident")
        .create(json!({
            "short_description": "Test incident",
            "urgency": "2"
        }))
        .await
        .expect("create failed");

    assert_eq!(record.sys_id, "new123");
    assert_eq!(record.get_str("number"), Some("INC0099999"));
}

#[tokio::test]
async fn test_update_record() {
    let server = MockServer::start().await;

    Mock::given(method("PATCH"))
        .and(path("/api/now/table/incident/abc123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {
                "sys_id": "abc123",
                "number": "INC0010001",
                "state": "2",
                "short_description": "Network outage"
            }
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let record = client
        .table("incident")
        .update("abc123", json!({ "state": "2" }))
        .await
        .expect("update failed");

    assert_eq!(record.get_str("state"), Some("2"));
}

#[tokio::test]
async fn test_update_work_notes() {
    let server = MockServer::start().await;

    // ServiceNow returns the updated record after a PATCH.
    // work_notes is write-only, so it comes back empty in the response.
    Mock::given(method("PATCH"))
        .and(path("/api/now/table/incident/abc123"))
        .and(query_param("sysparm_display_value", "true"))
        .and(query_param(
            "sysparm_fields",
            "sys_id,number,state,work_notes",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {
                "sys_id": "abc123",
                "number": "INC0010001",
                "state": "Pending",
                "work_notes": ""
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let record = client
        .table("incident")
        .fields(&["sys_id", "number", "state", "work_notes"])
        .display_value(DisplayValue::Display)
        .update(
            "abc123",
            json!({ "work_notes": "Test work note from Rust client" }),
        )
        .await
        .expect("work_notes update failed");

    // The record comes back successfully.
    assert_eq!(record.get_str("number"), Some("INC0010001"));
    assert_eq!(record.get_str("state"), Some("Pending"));
    // work_notes is write-only — returns empty on GET/PATCH response.
    assert_eq!(record.get_str("work_notes"), Some(""));
}

#[tokio::test]
async fn test_update_passes_query_params() {
    let server = MockServer::start().await;

    // Verify that update() passes display_value, fields, and exclude_reference_link.
    Mock::given(method("PATCH"))
        .and(path("/api/now/table/incident/sys123"))
        .and(query_param("sysparm_display_value", "all"))
        .and(query_param("sysparm_fields", "number,state"))
        .and(query_param("sysparm_exclude_reference_link", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {
                "sys_id": "sys123",
                "number": { "value": "INC0010002", "display_value": "INC0010002" },
                "state": { "value": "1", "display_value": "New" }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let record = client
        .table("incident")
        .fields(&["number", "state"])
        .display_value(DisplayValue::Both)
        .exclude_reference_link(true)
        .update("sys123", json!({ "state": "1" }))
        .await
        .expect("update with params failed");

    assert_eq!(record.get_str("number"), Some("INC0010002"));
    assert_eq!(record.get_display("state"), Some("New"));
}

#[tokio::test]
async fn test_update_with_body_json() {
    let server = MockServer::start().await;

    // Verify the request body is sent correctly using body_json matcher.
    Mock::given(method("PATCH"))
        .and(path("/api/now/table/incident/sys456"))
        .and(wiremock::matchers::body_json(json!({
            "work_notes": "Note added via API",
            "state": "2"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {
                "sys_id": "sys456",
                "number": "INC0010003",
                "state": "2",
                "work_notes": ""
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let record = client
        .table("incident")
        .update(
            "sys456",
            json!({
                "work_notes": "Note added via API",
                "state": "2"
            }),
        )
        .await
        .expect("update with body failed");

    assert_eq!(record.get_str("state"), Some("2"));
    // work_notes is write-only, comes back empty.
    assert_eq!(record.get_str("work_notes"), Some(""));
}

#[tokio::test]
async fn test_delete_record() {
    let server = MockServer::start().await;

    Mock::given(method("DELETE"))
        .and(path("/api/now/table/incident/abc123"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    client
        .table("incident")
        .delete("abc123")
        .await
        .expect("delete failed");
}

#[tokio::test]
async fn test_display_value_both() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/change_request"))
        .and(query_param("sysparm_display_value", "all"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                {
                    "sys_id": { "display_value": "abc123", "value": "abc123" },
                    "number": { "display_value": "CHG0012345", "value": "CHG0012345" },
                    "state": { "display_value": "New", "value": "1" },
                    "assigned_to": {
                        "display_value": "John Smith",
                        "value": "user_sys_id",
                        "link": "https://instance.service-now.com/api/now/table/sys_user/user_sys_id"
                    }
                }
            ]
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let result = client
        .table("change_request")
        .display_value(DisplayValue::Both)
        .execute()
        .await
        .expect("query failed");

    assert_eq!(result.len(), 1);
    let record = &result.records[0];
    assert_eq!(record.get_display("state"), Some("New"));
    assert_eq!(record.get_raw("state"), Some("1"));
    assert_eq!(record.get_display("assigned_to"), Some("John Smith"));
}

#[tokio::test]
async fn test_field_selection() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_fields", "number,short_description"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                {
                    "sys_id": "abc123",
                    "number": "INC0010001",
                    "short_description": "Test"
                }
            ]
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let result = client
        .table("incident")
        .fields(&["number", "short_description"])
        .execute()
        .await
        .expect("query failed");

    assert_eq!(result.len(), 1);
}

#[tokio::test]
async fn test_related_records_concurrent() {
    let server = MockServer::start().await;

    // Main query for change_request.
    Mock::given(method("GET"))
        .and(path("/api/now/table/change_request"))
        .and(query_param("sysparm_query", "number=CHG0012345"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                {
                    "sys_id": "chg_sys_id",
                    "number": "CHG0012345",
                    "state": "1"
                }
            ]
        })))
        .mount(&server)
        .await;

    // Related change_tasks.
    Mock::given(method("GET"))
        .and(path("/api/now/table/change_task"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                {
                    "sys_id": "ctask1",
                    "number": "CTASK0001",
                    "change_request": "chg_sys_id",
                    "short_description": "Pre-implementation"
                },
                {
                    "sys_id": "ctask2",
                    "number": "CTASK0002",
                    "change_request": "chg_sys_id",
                    "short_description": "Post-implementation"
                }
            ]
        })))
        .mount(&server)
        .await;

    // Related approvals.
    Mock::given(method("GET"))
        .and(path("/api/now/table/sysapproval_approver"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                {
                    "sys_id": "appr1",
                    "sysapproval": "chg_sys_id",
                    "approver": "user123",
                    "state": "approved"
                }
            ]
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let result = client
        .table("change_request")
        .equals("number", "CHG0012345")
        .include_related(&["change_task", "approvals"])
        .execute()
        .await
        .expect("query failed");

    assert_eq!(result.len(), 1);
    let record = &result.records[0];
    assert_eq!(record.get_str("number"), Some("CHG0012345"));

    // Check related change tasks were attached.
    let tasks = record.related("change_task");
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].get_str("number"), Some("CTASK0001"));
    assert_eq!(tasks[1].get_str("number"), Some("CTASK0002"));

    // Check related approvals were attached.
    let approvals = record.related("approvals");
    assert_eq!(approvals.len(), 1);
    assert_eq!(approvals[0].get_str("state"), Some("approved"));
}

#[tokio::test]
async fn test_api_error_handling() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/nonexistent"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "error": {
                "message": "Record not found",
                "detail": "Could not find table: nonexistent"
            }
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let result = client.table("nonexistent").execute().await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    let err_str = err.to_string();
    assert!(err_str.contains("Record not found"), "Got: {}", err_str);
}

#[tokio::test]
async fn test_auth_failure() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "error": {
                "message": "User Not Authenticated",
                "detail": "Required to provide Auth information"
            }
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let result = client.table("incident").execute().await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    match err {
        Error::Auth { message, status } => {
            assert!(message.contains("User Not Authenticated"));
            assert_eq!(status, Some(401));
        }
        other => panic!("Expected Auth error, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_complex_query_encoding() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param(
            "sysparm_query",
            "state=1^priorityIN1,2^short_descriptionLIKEnetwork^ORDERBYDESCsys_created_on",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": []
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let result = client
        .table("incident")
        .equals("state", "1")
        .in_list("priority", &["1", "2"])
        .contains("short_description", "network")
        .order_by("sys_created_on", Order::Desc)
        .execute()
        .await
        .expect("query failed");

    assert_eq!(result.len(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn test_client_from_env() {
    // SAFETY: current_thread runtime avoids data race on env vars.
    unsafe {
        std::env::set_var("SERVICENOW_INSTANCE", "testinstance");
        std::env::set_var("SERVICENOW_USERNAME", "env_user");
        std::env::set_var("SERVICENOW_PASSWORD", "env_pass");
    }

    let client = ServiceNowClient::from_env().await;
    assert!(client.is_ok());

    unsafe {
        std::env::remove_var("SERVICENOW_INSTANCE");
        std::env::remove_var("SERVICENOW_USERNAME");
        std::env::remove_var("SERVICENOW_PASSWORD");
    }
}

#[tokio::test]
async fn test_first() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_limit", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                {
                    "sys_id": "first_id",
                    "number": "INC0000001"
                }
            ]
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let record = client
        .table("incident")
        .first()
        .await
        .expect("first failed");

    assert!(record.is_some());
    assert_eq!(record.unwrap().get_str("number"), Some("INC0000001"));
}

#[tokio::test]
async fn test_no_schema_related_records() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/change_request"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                { "sys_id": "id1", "number": "CHG001" }
            ]
        })))
        .mount(&server)
        .await;

    // Build client WITHOUT schema.
    let client = ServiceNowClient::builder()
        .instance(server.uri())
        .auth(BasicAuth::new("user", "pass"))
        .allow_http()
        .build()
        .await
        .expect("build failed");

    let result = client
        .table("change_request")
        .include_related(&["change_task"])
        .execute()
        .await
        .expect("query failed");

    // Should succeed with records but have schema errors.
    assert_eq!(result.len(), 1);
    assert!(result.has_errors());
}

#[tokio::test]
async fn test_dot_walk_fields() {
    let server = MockServer::start().await;

    // ServiceNow returns dot-walked fields as flat keys.
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param(
            "sysparm_fields",
            "number,assigned_to.name,assigned_to.email",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                {
                    "sys_id": "abc123",
                    "number": "INC0010001",
                    "assigned_to.name": "John Smith",
                    "assigned_to.email": "john@example.com"
                }
            ]
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let result = client
        .table("incident")
        .fields(&["number"])
        .dot_walk(&["assigned_to.name", "assigned_to.email"])
        .execute()
        .await
        .expect("dot-walk query failed");

    assert_eq!(result.len(), 1);
    let record = &result.records[0];
    assert_eq!(record.get_str("assigned_to.name"), Some("John Smith"));
    assert_eq!(
        record.get_str("assigned_to.email"),
        Some("john@example.com")
    );
    assert!(record.has_field("assigned_to.name"));

    // Test dot_walked_fields helper.
    let dw = record.dot_walked_fields("assigned_to");
    assert_eq!(dw.len(), 2);
}

#[tokio::test]
async fn test_dot_walk_with_display_value_all() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_display_value", "all"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                {
                    "sys_id": { "display_value": "abc123", "value": "abc123" },
                    "number": { "display_value": "INC0010001", "value": "INC0010001" },
                    "caller_id.name": { "display_value": "Jane Doe", "value": "Jane Doe" },
                    "caller_id.manager.name": { "display_value": "Bob Boss", "value": "Bob Boss" }
                }
            ]
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let result = client
        .table("incident")
        .fields(&["number"])
        .dot_walk(&["caller_id.name", "caller_id.manager.name"])
        .display_value(DisplayValue::Both)
        .execute()
        .await
        .expect("dot-walk + display_value query failed");

    let record = &result.records[0];
    assert_eq!(record.get_display("caller_id.name"), Some("Jane Doe"));
    assert_eq!(
        record.get_display("caller_id.manager.name"),
        Some("Bob Boss")
    );
}

#[tokio::test]
async fn test_include_related_with_display_value_display() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/change_request"))
        .and(query_param("sysparm_display_value", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                {
                    "sys_id": "chg_sys_id",
                    "number": "CHG0012345"
                }
            ]
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/change_task"))
        .and(query_param("sysparm_display_value", "all"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                {
                    "sys_id": { "value": "ctask1", "display_value": "ctask1" },
                    "number": { "value": "CTASK0001", "display_value": "CTASK0001" },
                    "change_request": {
                        "value": "chg_sys_id",
                        "display_value": "CHG0012345"
                    }
                }
            ]
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let result = client
        .table("change_request")
        .include_related(&["change_task"])
        .display_value(DisplayValue::Display)
        .execute()
        .await
        .expect("query failed");

    assert_eq!(result.len(), 1);
    let tasks = result.records[0].related("change_task");
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].get_str("number"), Some("CTASK0001"));
}

#[tokio::test]
async fn test_pagination() {
    let server = MockServer::start().await;
    let counter = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));

    // Page 1.
    let counter_clone = counter.clone();
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_offset", "0"))
        .and(query_param("sysparm_limit", "2"))
        .respond_with(move |_: &wiremock::Request| {
            counter_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            ResponseTemplate::new(200)
                .append_header("X-Total-Count", "5")
                .set_body_json(json!({
                    "result": [
                        { "sys_id": "a1", "number": "INC001" },
                        { "sys_id": "a2", "number": "INC002" }
                    ]
                }))
        })
        .mount(&server)
        .await;

    // Page 2.
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_offset", "2"))
        .and(query_param("sysparm_limit", "2"))
        .respond_with(
            ResponseTemplate::new(200)
                .append_header("X-Total-Count", "5")
                .set_body_json(json!({
                    "result": [
                        { "sys_id": "a3", "number": "INC003" },
                        { "sys_id": "a4", "number": "INC004" }
                    ]
                })),
        )
        .mount(&server)
        .await;

    // Page 3 (last, partial).
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_offset", "4"))
        .and(query_param("sysparm_limit", "2"))
        .respond_with(
            ResponseTemplate::new(200)
                .append_header("X-Total-Count", "5")
                .set_body_json(json!({
                    "result": [
                        { "sys_id": "a5", "number": "INC005" }
                    ]
                })),
        )
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    // Test paginate() manual iteration.
    let mut paginator = client.table("incident").limit(2).paginate().unwrap();

    let page1 = paginator.next_page().await.unwrap().unwrap();
    assert_eq!(page1.len(), 2);
    assert_eq!(paginator.total_count(), Some(5));

    let page2 = paginator.next_page().await.unwrap().unwrap();
    assert_eq!(page2.len(), 2);

    let page3 = paginator.next_page().await.unwrap().unwrap();
    assert_eq!(page3.len(), 1);

    let page4 = paginator.next_page().await.unwrap();
    assert!(page4.is_none()); // no more pages
    assert!(paginator.is_done());
}

#[tokio::test]
async fn test_pagination_follows_link_header_for_short_pages() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_offset", "0"))
        .and(query_param("sysparm_limit", "2"))
        .respond_with(
            ResponseTemplate::new(200)
                .append_header("X-Total-Count", "3")
                .append_header(
                    "Link",
                    format!(
                        "<{}/api/now/table/incident?sysparm_limit=2&sysparm_offset=0>;rel=\"first\",<{}/api/now/table/incident?sysparm_limit=2&sysparm_offset=2>;rel=\"next\",<{}/api/now/table/incident?sysparm_limit=2&sysparm_offset=2>;rel=\"last\"",
                        server.uri(),
                        server.uri(),
                        server.uri()
                    ),
                )
                .set_body_json(json!({
                    "result": [
                        { "sys_id": "a1", "number": "INC001" }
                    ]
                })),
        )
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_offset", "2"))
        .and(query_param("sysparm_limit", "2"))
        .respond_with(
            ResponseTemplate::new(200)
                .append_header("X-Total-Count", "3")
                .set_body_json(json!({
                    "result": [
                        { "sys_id": "a2", "number": "INC002" }
                    ]
                })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let mut paginator = client.table("incident").limit(2).paginate().unwrap();

    let page1 = paginator.next_page().await.unwrap().unwrap();
    assert_eq!(page1.len(), 1);
    assert_eq!(paginator.current_offset(), 2);
    assert!(!paginator.is_done());

    let page2 = paginator.next_page().await.unwrap().unwrap();
    assert_eq!(page2.len(), 1);
    assert!(paginator.is_done());

    let page3 = paginator.next_page().await.unwrap();
    assert!(page3.is_none());
}

#[tokio::test]
async fn test_pagination_with_offset() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_offset", "2"))
        .and(query_param("sysparm_limit", "2"))
        .respond_with(
            ResponseTemplate::new(200)
                .append_header("X-Total-Count", "4")
                .set_body_json(json!({
                    "result": [
                        { "sys_id": "a3", "number": "INC003" },
                        { "sys_id": "a4", "number": "INC004" }
                    ]
                })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let mut paginator = client
        .table("incident")
        .offset(2)
        .limit(2)
        .paginate()
        .unwrap();
    let page = paginator.next_page().await.unwrap().unwrap();

    assert_eq!(page.len(), 2);
    assert_eq!(page.records[0].get_str("number"), Some("INC003"));
    assert_eq!(paginator.current_offset(), 4);
}

#[tokio::test]
async fn test_execute_all_with_related_records() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/change_request"))
        .and(query_param("sysparm_offset", "0"))
        .and(query_param("sysparm_limit", "1"))
        .respond_with(
            ResponseTemplate::new(200)
                .append_header("X-Total-Count", "2")
                .set_body_json(json!({
                    "result": [
                        { "sys_id": "chg1", "number": "CHG001" }
                    ]
                })),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/change_request"))
        .and(query_param("sysparm_offset", "1"))
        .and(query_param("sysparm_limit", "1"))
        .respond_with(
            ResponseTemplate::new(200)
                .append_header("X-Total-Count", "2")
                .set_body_json(json!({
                    "result": [
                        { "sys_id": "chg2", "number": "CHG002" }
                    ]
                })),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/change_task"))
        .and(query_param("sysparm_query", "change_requestINchg1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                {
                    "sys_id": "ctask1",
                    "number": "CTASK001",
                    "change_request": "chg1"
                }
            ]
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/change_task"))
        .and(query_param("sysparm_query", "change_requestINchg2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                {
                    "sys_id": "ctask2",
                    "number": "CTASK002",
                    "change_request": "chg2"
                }
            ]
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let result = client
        .table("change_request")
        .include_related(&["change_task"])
        .limit(1)
        .execute_all(None)
        .await
        .expect("execute_all failed");

    assert_eq!(result.len(), 2);
    assert_eq!(result.records[0].related("change_task").len(), 1);
    assert_eq!(result.records[1].related("change_task").len(), 1);
}

#[tokio::test]
async fn test_include_related_dotwalk_strategy_falls_back_to_concurrent() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/change_request"))
        .and(query_param("sysparm_fields", "number"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                { "sys_id": "chg1", "number": "CHG001" }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/change_task"))
        .and(query_param("sysparm_query", "change_requestINchg1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                {
                    "sys_id": "ctask1",
                    "number": "CTASK001",
                    "change_request": "chg1"
                }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let result = client
        .table("change_request")
        .fields(&["number"])
        .include_related(&["change_task"])
        .strategy(FetchStrategy::DotWalk)
        .execute()
        .await
        .expect("query failed");

    assert_eq!(result.len(), 1);
    assert_eq!(result.records[0].related("change_task").len(), 1);
}

#[tokio::test]
async fn test_execute_all() {
    let server = MockServer::start().await;

    // Page 1.
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_offset", "0"))
        .and(query_param("sysparm_limit", "3"))
        .respond_with(
            ResponseTemplate::new(200)
                .append_header("X-Total-Count", "5")
                .set_body_json(json!({
                    "result": [
                        { "sys_id": "a1", "number": "INC001" },
                        { "sys_id": "a2", "number": "INC002" },
                        { "sys_id": "a3", "number": "INC003" }
                    ]
                })),
        )
        .mount(&server)
        .await;

    // Page 2 (last).
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_offset", "3"))
        .and(query_param("sysparm_limit", "3"))
        .respond_with(
            ResponseTemplate::new(200)
                .append_header("X-Total-Count", "5")
                .set_body_json(json!({
                    "result": [
                        { "sys_id": "a4", "number": "INC004" },
                        { "sys_id": "a5", "number": "INC005" }
                    ]
                })),
        )
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let result = client
        .table("incident")
        .limit(3)
        .execute_all(None)
        .await
        .expect("execute_all failed");

    assert_eq!(result.len(), 5);
    assert_eq!(result.total_count, Some(5));
}

#[tokio::test]
async fn test_execute_all_with_max() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_offset", "0"))
        .and(query_param("sysparm_limit", "5"))
        .respond_with(
            ResponseTemplate::new(200)
                .append_header("X-Total-Count", "1000")
                .set_body_json(json!({
                    "result": [
                        { "sys_id": "a1", "number": "INC001" },
                        { "sys_id": "a2", "number": "INC002" },
                        { "sys_id": "a3", "number": "INC003" },
                        { "sys_id": "a4", "number": "INC004" },
                        { "sys_id": "a5", "number": "INC005" }
                    ]
                })),
        )
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    // Limit to 3 records even though there are 1000 total.
    let result = client
        .table("incident")
        .limit(5)
        .execute_all(Some(3))
        .await
        .expect("execute_all with max failed");

    assert_eq!(result.len(), 3);
}

#[tokio::test]
async fn test_reference_field_parsing() {
    let server = MockServer::start().await;

    // Reference fields with display_value=false return {link, value} objects.
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                {
                    "sys_id": "abc123",
                    "number": "INC001",
                    "assigned_to": {
                        "link": "https://instance.service-now.com/api/now/table/sys_user/user123",
                        "value": "user123"
                    }
                }
            ]
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let result = client
        .table("incident")
        .execute()
        .await
        .expect("query failed");

    let record = &result.records[0];
    let assigned = record.get("assigned_to").expect("missing assigned_to");
    assert_eq!(assigned.raw_str(), Some("user123"));
    assert!(assigned.link.is_some());
}

#[tokio::test]
async fn test_aggregate_count() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/stats/incident"))
        .and(query_param("sysparm_count", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {
                "stats": {
                    "count": "700793"
                }
            }
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let stats = client
        .aggregate("incident")
        .count()
        .execute()
        .await
        .unwrap();
    assert_eq!(stats.count(), 700793);
    assert!(!stats.is_grouped());
}

#[tokio::test]
async fn test_aggregate_grouped() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/stats/incident"))
        .and(query_param("sysparm_count", "true"))
        .and(query_param("sysparm_group_by", "state"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                {
                    "stats": {"count": "1772"},
                    "groupby_fields": [{"field": "state", "value": "-5"}]
                },
                {
                    "stats": {"count": "145"},
                    "groupby_fields": [{"field": "state", "value": "1"}]
                },
                {
                    "stats": {"count": "668"},
                    "groupby_fields": [{"field": "state", "value": "2"}]
                }
            ]
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let stats = client
        .aggregate("incident")
        .count()
        .group_by("state")
        .execute()
        .await
        .unwrap();

    assert!(stats.is_grouped());
    assert_eq!(stats.group_count(), 3);
    assert_eq!(stats.groups()[0].count(), 1772);
    assert_eq!(stats.groups()[0].field_value("state"), "-5");
    assert_eq!(stats.groups()[1].count(), 145);
    assert_eq!(stats.groups()[1].field_value("state"), "1");
}

#[tokio::test]
async fn test_aggregate_with_filter() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/stats/incident"))
        .and(query_param("sysparm_count", "true"))
        .and(query_param("sysparm_query", "active=true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {
                "stats": {"count": "2585"}
            }
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let stats = client
        .aggregate("incident")
        .count()
        .equals("active", "true")
        .execute()
        .await
        .unwrap();

    assert_eq!(stats.count(), 2585);
}

#[tokio::test]
async fn test_token_auth() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                { "sys_id": "abc", "number": "INC001" }
            ]
        })))
        .mount(&server)
        .await;

    // Build client with token auth.
    let client = ServiceNowClient::builder()
        .instance(server.uri())
        .auth(TokenAuth::bearer("my-secret-token"))
        .allow_http()
        .build()
        .await
        .expect("build with token auth failed");

    let result = client
        .table("incident")
        .limit(1)
        .execute()
        .await
        .expect("query with token auth failed");

    assert_eq!(result.len(), 1);
}

// ── Task SLA read helper tests ──────────────────────────────────

#[tokio::test]
async fn test_task_slas_builder_sets_projection_display_and_no_orderby() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/task_sla"))
        .and(query_param("sysparm_query", "task=task_sys_id"))
        .and(query_param("sysparm_fields", TASK_SLA_DEFAULT_FIELDS))
        .and(query_param("sysparm_display_value", "all"))
        .and(query_param("sysparm_exclude_reference_link", "true"))
        .and(query_param_is_missing("sysparm_orderby"))
        .and(query_param_is_missing("sysparm_no_count"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": []
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let result = client
        .task_slas("task_sys_id")
        .execute()
        .await
        .expect("task_slas query failed");

    assert!(result.is_empty());
}

#[tokio::test]
async fn test_task_slas_for_number_resolves_parent_drains_pages_and_parses_display_values() {
    let server = MockServer::start().await;
    let task_sys_id = fixture_sys_id(10);
    let sla_one = fixture_sys_id(20);
    let sla_two = fixture_sys_id(21);

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_query", "number=INC0010001"))
        .and(query_param("sysparm_limit", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                {
                    "sys_id": task_sys_id.clone(),
                    "number": "INC0010001"
                }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/task_sla"))
        .and(query_param("sysparm_query", format!("taskIN{}", task_sys_id)))
        .and(query_param("sysparm_fields", TASK_SLA_DEFAULT_FIELDS))
        .and(query_param("sysparm_display_value", "all"))
        .and(query_param("sysparm_no_count", "true"))
        .and(query_param("sysparm_limit", "100"))
        .and(query_param("sysparm_offset", "0"))
        .respond_with(
            ResponseTemplate::new(200)
                .append_header(
                    "Link",
                    format!(
                        "<{}/api/now/table/task_sla?sysparm_limit=100&sysparm_offset=100>;rel=\"next\"",
                        server.uri()
                    ),
                )
                .set_body_json(json!({
                    "result": [
                        task_sla_row!(
                            &fixture_sys_id(30),
                            &task_sys_id,
                            "INC0010001",
                            &sla_one,
                            "Resolution SLA",
                            "in_progress",
                            true,
                            false,
                            "2026-05-06 17:00:00",
                            "55.5"
                        )
                    ]
                })),
        )
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/task_sla"))
        .and(query_param(
            "sysparm_query",
            format!("taskIN{}", task_sys_id),
        ))
        .and(query_param("sysparm_fields", TASK_SLA_DEFAULT_FIELDS))
        .and(query_param("sysparm_display_value", "all"))
        .and(query_param("sysparm_no_count", "true"))
        .and(query_param("sysparm_limit", "100"))
        .and(query_param("sysparm_offset", "100"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                task_sla_row!(
                    &fixture_sys_id(31),
                    &task_sys_id,
                    "INC0010001",
                    &sla_two,
                    "Response SLA",
                    "in_progress",
                    true,
                    false,
                    "2026-05-06 16:00:00",
                    "65.25"
                )
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let slas = client
        .task_slas_for_number("INC0010001")
        .await
        .expect("task_slas_for_number failed");

    assert_eq!(slas.len(), 2);
    assert_eq!(slas[0].sys_id, fixture_sys_id(31));
    assert_eq!(slas[0].task_sys_id.as_deref(), Some(task_sys_id.as_str()));
    assert_eq!(slas[0].sla_sys_id.as_deref(), Some(sla_two.as_str()));
    assert_eq!(slas[0].sla_name.as_deref(), Some("Response SLA"));
    assert!(matches!(&slas[0].stage, Some(TaskSlaStage::InProgress)));
    assert_eq!(slas[0].active, Some(true));
    assert_eq!(slas[0].has_breached, Some(false));
    assert_eq!(slas[0].business_elapsed_percentage, Some(65.25));
    assert_eq!(slas[0].schedule_sys_id.as_deref(), Some("schedule_sys_id"));
}

#[tokio::test]
async fn test_task_slas_for_tasks_drains_multi_chunk_short_pages() {
    let server = MockServer::start().await;
    let base_url = server.uri();
    let task_ids: Vec<String> = (0..101).map(fixture_sys_id).collect();
    let task_refs = as_str_refs(&task_ids);
    let mock_task_ids = task_ids.clone();
    let mut expected_sla_ids: Vec<String> = (0..100).map(|idx| fixture_sys_id(300 + idx)).collect();
    expected_sla_ids.extend([
        fixture_sys_id(500),
        fixture_sys_id(501),
        fixture_sys_id(600),
    ]);

    Mock::given(method("GET"))
        .and(path("/api/now/table/task_sla"))
        .respond_with(move |request: &wiremock::Request| {
            let query = request_query(request).expect("task_sla query missing");
            let chunk_ids = in_list_values(&query, "task");
            let offset = request_query_param(request, "sysparm_offset")
                .expect("task_sla query missing offset");
            let mut rows = Vec::new();

            if chunk_ids.iter().any(|id| id == &mock_task_ids[0]) {
                match offset.as_str() {
                    "0" => {
                        for (idx, task_id) in mock_task_ids.iter().take(100).enumerate() {
                            rows.push(task_sla_row!(
                                &fixture_sys_id(300 + idx),
                                task_id,
                                &format!("TASK{:07}", idx + 1),
                                &fixture_sys_id(400 + idx),
                                "Resolution SLA",
                                "in_progress",
                                true,
                                false,
                                "2026-05-06 17:00:00",
                                "55.5",
                            ));
                        }

                        ResponseTemplate::new(200)
                            .append_header(
                                "Link",
                                format!(
                                    "<{base_url}/api/now/table/task_sla?sysparm_limit=100&sysparm_offset=100>;rel=\"next\""
                                ),
                            )
                            .set_body_json(json!({ "result": rows }))
                    }
                    "100" => {
                        rows.push(task_sla_row!(
                            &fixture_sys_id(500),
                            &mock_task_ids[0],
                            "TASK0000001",
                            &fixture_sys_id(700),
                            "Short Page SLA A",
                            "in_progress",
                            true,
                            false,
                            "2026-05-06 18:00:00",
                            "65.0",
                        ));
                        rows.push(task_sla_row!(
                            &fixture_sys_id(501),
                            &mock_task_ids[1],
                            "TASK0000002",
                            &fixture_sys_id(701),
                            "Short Page SLA B",
                            "paused",
                            true,
                            false,
                            "2026-05-06 19:00:00",
                            "35.0",
                        ));

                        ResponseTemplate::new(200).set_body_json(json!({ "result": rows }))
                    }
                    other => panic!("unexpected first chunk offset: {other}"),
                }
            } else if chunk_ids.iter().any(|id| id == &mock_task_ids[100]) {
                assert_eq!(offset, "0");
                rows.push(task_sla_row!(
                    &fixture_sys_id(600),
                    &mock_task_ids[100],
                    "TASK0000101",
                    &fixture_sys_id(800),
                    "Second Chunk SLA",
                    "in_progress",
                    true,
                    false,
                    "2026-05-06 20:00:00",
                    "25.0",
                ));

                ResponseTemplate::new(200).set_body_json(json!({ "result": rows }))
            } else {
                panic!("unexpected task_sla chunk ids: {chunk_ids:?}");
            }
        })
        .expect(3)
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let by_task = client
        .task_slas_for_tasks(&task_refs)
        .await
        .expect("task_slas_for_tasks failed");

    assert_eq!(by_task.len(), 101);

    let mut actual_sla_ids: Vec<String> = by_task
        .values()
        .flat_map(|rows| rows.iter().map(|sla| sla.sys_id.clone()))
        .collect();
    actual_sla_ids.sort();
    expected_sla_ids.sort();
    assert_eq!(actual_sla_ids, expected_sla_ids);

    assert_eq!(
        by_task.get(&task_ids[0]).expect("first task missing").len(),
        2
    );
    assert_eq!(
        by_task
            .get(&task_ids[100])
            .expect("second chunk task missing")
            .len(),
        1
    );

    let task_sla_requests = requests_for_path(&server, "/api/now/table/task_sla").await;
    assert_eq!(task_sla_requests.len(), 3);
    assert!(task_sla_requests.iter().any(|request| {
        request_query_param(request, "sysparm_offset").as_deref() == Some("100")
    }));
    for request in &task_sla_requests {
        assert_eq!(
            request_query_param(request, "sysparm_limit").as_deref(),
            Some("100")
        );
    }
}

#[tokio::test]
async fn test_task_slas_for_tasks_chunks_groups_prepopulates_and_sorts() {
    let server = MockServer::start().await;
    let task_ids: Vec<String> = (0..101).map(fixture_sys_id).collect();
    let task_refs = as_str_refs(&task_ids);
    let first_task = task_ids[0].clone();
    let missing_task = task_ids[1].clone();
    let last_task = task_ids[100].clone();
    let mock_task_ids = task_ids.clone();

    Mock::given(method("GET"))
        .and(path("/api/now/table/task_sla"))
        .respond_with(move |request: &wiremock::Request| {
            let query = request_query(request).expect("task_sla query missing");
            let chunk_ids = in_list_values(&query, "task");
            let mut rows = Vec::new();

            if chunk_ids.iter().any(|id| id == &mock_task_ids[0]) {
                rows.push(task_sla_row!(
                    &fixture_sys_id(200),
                    &mock_task_ids[0],
                    "INC0010001",
                    &fixture_sys_id(300),
                    "Inactive SLA",
                    "completed",
                    false,
                    false,
                    "2026-05-06 13:00:00",
                    "99.0",
                ));
                rows.push(task_sla_row!(
                    &fixture_sys_id(201),
                    &mock_task_ids[0],
                    "INC0010001",
                    &fixture_sys_id(301),
                    "Breached SLA",
                    "in_progress",
                    true,
                    true,
                    "2026-05-06 12:00:00",
                    "95.0",
                ));
                rows.push(task_sla_row!(
                    &fixture_sys_id(202),
                    &mock_task_ids[0],
                    "INC0010001",
                    &fixture_sys_id(302),
                    "Later Active SLA",
                    "in_progress",
                    true,
                    false,
                    "2026-05-06 17:00:00",
                    "40.0",
                ));
                rows.push(task_sla_row!(
                    &fixture_sys_id(203),
                    &mock_task_ids[0],
                    "INC0010001",
                    &fixture_sys_id(303),
                    "Next Breach SLA",
                    "in_progress",
                    true,
                    false,
                    "2026-05-06 15:00:00",
                    "50.0",
                ));
            }

            if chunk_ids.iter().any(|id| id == &mock_task_ids[100]) {
                rows.push(task_sla_row!(
                    &fixture_sys_id(204),
                    &mock_task_ids[100],
                    "INC0010101",
                    &fixture_sys_id(304),
                    "Last Chunk SLA",
                    "paused",
                    true,
                    false,
                    "2026-05-06 18:00:00",
                    "20.0",
                ));
            }

            ResponseTemplate::new(200).set_body_json(json!({ "result": rows }))
        })
        .expect(2)
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let by_task = client
        .task_slas_for_tasks(&task_refs)
        .await
        .expect("task_slas_for_tasks failed");

    assert_eq!(by_task.len(), 101);
    assert!(by_task
        .get(&missing_task)
        .expect("missing task should be prepopulated")
        .is_empty());
    assert_eq!(by_task.get(&last_task).expect("last task missing").len(), 1);

    let first_rows = by_task.get(&first_task).expect("first task missing");
    assert_eq!(first_rows.len(), 4);
    assert_eq!(first_rows[0].sla_name.as_deref(), Some("Next Breach SLA"));
    assert_eq!(first_rows[1].sla_name.as_deref(), Some("Later Active SLA"));
    assert_eq!(first_rows[2].sla_name.as_deref(), Some("Breached SLA"));
    assert_eq!(first_rows[3].sla_name.as_deref(), Some("Inactive SLA"));

    let task_sla_requests = requests_for_path(&server, "/api/now/table/task_sla").await;
    assert_eq!(task_sla_requests.len(), 2);
    for request in &task_sla_requests {
        let query = request_query(request).expect("task_sla query missing");
        let chunk_ids = in_list_values(&query, "task");
        assert!(
            chunk_ids.len() <= 100,
            "task_slas_for_tasks chunk exceeded 100 ids: {}",
            chunk_ids.len()
        );
        assert!(
            request.url.as_str().len() < 4000,
            "encoded task_sla chunk URL exceeded 4 KB: {}",
            request.url.as_str().len()
        );
    }
}

#[tokio::test]
async fn test_task_slas_for_tasks_deduplicates_task_ids_before_querying() {
    let server = MockServer::start().await;
    let task_id = fixture_sys_id(900);
    let task_refs = [task_id.as_str(), task_id.as_str()];

    Mock::given(method("GET"))
        .and(path("/api/now/table/task_sla"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": []
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let by_task = client
        .task_slas_for_tasks(&task_refs)
        .await
        .expect("task_slas_for_tasks failed");

    assert_eq!(by_task.len(), 1);
    assert!(by_task.get(&task_id).expect("task missing").is_empty());

    let task_sla_requests = requests_for_path(&server, "/api/now/table/task_sla").await;
    assert_eq!(task_sla_requests.len(), 1);
    let query = request_query(&task_sla_requests[0]).expect("task_sla query missing");
    assert_eq!(in_list_values(&query, "task"), vec![task_id]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_task_slas_for_tasks_limits_chunk_concurrency_to_four() {
    let server = MockServer::start().await;
    let task_ids: Vec<String> = (0..401).map(fixture_sys_id).collect();
    let task_refs = as_str_refs(&task_ids);
    let in_flight = Arc::new(AtomicUsize::new(0));
    let max_in_flight = Arc::new(AtomicUsize::new(0));
    let in_flight_for_mock = Arc::clone(&in_flight);
    let max_for_mock = Arc::clone(&max_in_flight);

    Mock::given(method("GET"))
        .and(path("/api/now/table/task_sla"))
        .respond_with(move |_: &wiremock::Request| {
            let current = in_flight_for_mock.fetch_add(1, AtomicOrdering::SeqCst) + 1;
            max_for_mock.fetch_max(current, AtomicOrdering::SeqCst);
            std::thread::sleep(Duration::from_millis(40));
            in_flight_for_mock.fetch_sub(1, AtomicOrdering::SeqCst);
            ResponseTemplate::new(200).set_body_json(json!({ "result": [] }))
        })
        .expect(5)
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let by_task = client
        .task_slas_for_tasks(&task_refs)
        .await
        .expect("task_slas_for_tasks failed");

    assert_eq!(by_task.len(), 401);
    assert!(
        max_in_flight.load(AtomicOrdering::SeqCst) <= 4,
        "task_slas_for_tasks exceeded 4 concurrent chunks"
    );
}

#[tokio::test]
async fn test_include_related_task_sla_chunks_parent_ids() {
    let server = MockServer::start().await;
    let task_ids: Vec<String> = (0..101).map(fixture_sys_id).collect();
    let incidents: Vec<Value> = task_ids
        .iter()
        .enumerate()
        .map(|(idx, sys_id)| {
            json!({
                "sys_id": sys_id,
                "number": format!("INC{:07}", idx + 1)
            })
        })
        .collect();

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": incidents
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/task_sla"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": []
        })))
        .expect(2)
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let result = client
        .table("incident")
        .fields(&["number"])
        .include_related(&["task_sla"])
        .limit(101)
        .execute()
        .await
        .expect("incident task_sla related query failed");

    assert_eq!(result.len(), 101);
    assert!(
        !result.has_errors(),
        "task_sla relationship should resolve through task inheritance"
    );

    let task_sla_requests = requests_for_path(&server, "/api/now/table/task_sla").await;
    assert_eq!(task_sla_requests.len(), 2);
    for request in &task_sla_requests {
        let query = request_query(request).expect("task_sla query missing");
        let chunk_ids = in_list_values(&query, "task");
        assert!(
            chunk_ids.len() <= 100,
            "related task_sla chunk exceeded 100 ids: {}",
            chunk_ids.len()
        );
    }
}

#[test]
fn test_task_sla_schema_is_defined_on_task_and_inherited_by_task_subclasses() {
    for release in ["washington", "xanadu", "yokohama"] {
        let registry = servicenow_rs::schema::SchemaRegistry::from_release(release).unwrap();

        assert!(registry.has_table("task_sla"), "{release} missing task_sla");
        assert!(
            registry.has_table("contract_sla"),
            "{release} missing contract_sla"
        );
        assert!(
            registry.has_field("task_sla", "business_percentage"),
            "{release} missing task_sla.business_percentage"
        );
        assert!(
            registry.has_field("contract_sla", "schedule"),
            "{release} missing contract_sla.schedule"
        );

        let task_rel = registry
            .relationship("task", "task_sla")
            .expect("task_sla relationship missing on task");
        assert_eq!(task_rel.table, "task_sla");
        assert_eq!(task_rel.foreign_key, "task");
        assert_eq!(
            task_rel.relationship_type,
            servicenow_rs::schema::RelationshipType::OneToMany
        );

        for table in ["incident", "change_request", "problem"] {
            let inherited = registry
                .relationship(table, "task_sla")
                .unwrap_or_else(|| panic!("{release} {table} missing inherited task_sla"));
            assert_eq!(inherited.table, "task_sla");
            assert_eq!(inherited.foreign_key, "task");
            assert_eq!(
                inherited.relationship_type,
                servicenow_rs::schema::RelationshipType::OneToMany
            );
            assert!(
                !registry
                    .table(table)
                    .expect("table exists")
                    .relationships
                    .contains_key("task_sla"),
                "{release} should not duplicate task_sla on {table}"
            );
        }
    }
}

// ── Journal field tests ─────────────────────────────────────────

#[tokio::test]
async fn test_journal_fields_return_empty_on_get() {
    let server = MockServer::start().await;

    // ServiceNow returns empty strings for journal fields on GET.
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                {
                    "sys_id": "inc123",
                    "number": "INC001",
                    "work_notes": "",
                    "comments": "",
                    "comments_and_work_notes": ""
                }
            ]
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let result = client
        .table("incident")
        .fields(&[
            "number",
            "work_notes",
            "comments",
            "comments_and_work_notes",
        ])
        .execute()
        .await
        .expect("journal field query failed");

    let record = &result.records[0];
    // Journal fields return empty on GET — this is expected ServiceNow behavior.
    assert_eq!(record.get_str("work_notes"), Some(""));
    assert_eq!(record.get_str("comments"), Some(""));
    assert_eq!(record.get_str("comments_and_work_notes"), Some(""));
}

#[tokio::test]
async fn test_journal_entries_via_sys_journal_field() {
    let server = MockServer::start().await;

    // Main incident query.
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                {
                    "sys_id": "inc123",
                    "number": "INC001",
                    "short_description": "Test incident"
                }
            ]
        })))
        .mount(&server)
        .await;

    // sys_journal_field entries for the incident (work_notes relationship).
    // The relationship is defined on change_request but we can also query directly.
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_journal_field"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                {
                    "sys_id": "jf001",
                    "element_id": "inc123",
                    "name": "incident",
                    "element": "work_notes",
                    "value": "Internal note: checking network switch",
                    "sys_created_on": "2026-03-25 10:00:00",
                    "sys_created_by": "admin"
                },
                {
                    "sys_id": "jf002",
                    "element_id": "inc123",
                    "name": "incident",
                    "element": "comments",
                    "value": "Hi, we are looking into this issue.",
                    "sys_created_on": "2026-03-25 10:05:00",
                    "sys_created_by": "admin"
                },
                {
                    "sys_id": "jf003",
                    "element_id": "inc123",
                    "name": "incident",
                    "element": "work_notes",
                    "value": "Escalated to network team",
                    "sys_created_on": "2026-03-25 11:00:00",
                    "sys_created_by": "admin"
                }
            ]
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    // Query the journal entries directly.
    let journal = client
        .table("sys_journal_field")
        .equals("element_id", "inc123")
        .fields(&["element", "value", "sys_created_on", "sys_created_by"])
        .execute()
        .await
        .expect("journal query failed");

    assert_eq!(journal.len(), 3);

    // Separate work_notes (private) from comments (public).
    let work_notes: Vec<_> = journal
        .iter()
        .filter(|r| r.get_str("element") == Some("work_notes"))
        .collect();
    let comments: Vec<_> = journal
        .iter()
        .filter(|r| r.get_str("element") == Some("comments"))
        .collect();

    assert_eq!(work_notes.len(), 2, "expected 2 work notes (private)");
    assert_eq!(comments.len(), 1, "expected 1 comment (public)");

    assert_eq!(
        work_notes[0].get_str("value"),
        Some("Internal note: checking network switch")
    );
    assert_eq!(
        comments[0].get_str("value"),
        Some("Hi, we are looking into this issue.")
    );
}

#[tokio::test]
async fn test_change_request_work_notes_relationship() {
    let server = MockServer::start().await;

    // Main change_request query.
    Mock::given(method("GET"))
        .and(path("/api/now/table/change_request"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                {
                    "sys_id": "chg123",
                    "number": "CHG001",
                    "short_description": "Deploy update"
                }
            ]
        })))
        .mount(&server)
        .await;

    // work_notes relationship: sys_journal_field with filter name=change_request.
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_journal_field"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                {
                    "sys_id": "jf001",
                    "element_id": "chg123",
                    "name": "change_request",
                    "element": "work_notes",
                    "value": "CAB approved, proceeding with implementation",
                    "sys_created_on": "2026-03-20 14:00:00",
                    "sys_created_by": "change_mgr"
                },
                {
                    "sys_id": "jf002",
                    "element_id": "chg123",
                    "name": "change_request",
                    "element": "comments",
                    "value": "Change scheduled for this weekend",
                    "sys_created_on": "2026-03-20 14:05:00",
                    "sys_created_by": "change_mgr"
                }
            ]
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    // Use include_related to fetch work_notes via the relationship.
    let result = client
        .table("change_request")
        .include_related(&["work_notes"])
        .execute()
        .await
        .expect("change_request with work_notes failed");

    assert_eq!(result.len(), 1);
    let chg = &result.records[0];
    let notes = chg.related("work_notes");
    assert_eq!(
        notes.len(),
        2,
        "expected 2 journal entries via relationship"
    );
    assert_eq!(notes[0].get_str("element"), Some("work_notes"));
}

#[tokio::test]
async fn test_schema_journal_field_metadata() {
    // Verify schema correctly identifies journal fields.
    let registry = servicenow_rs::schema::SchemaRegistry::from_release("xanadu").unwrap();

    // work_notes should be journal, write-only.
    let wn = registry.field("incident", "work_notes").unwrap();
    assert!(wn.is_journal());
    assert!(wn.write_only);

    // comments should be journal, write-only.
    let c = registry.field("incident", "comments").unwrap();
    assert!(c.is_journal());
    assert!(c.write_only);

    // approval_history should be journal, read-only.
    let ah = registry.field("incident", "approval_history").unwrap();
    assert!(ah.is_journal());
    assert!(ah.read_only);

    // short_description should NOT be journal.
    let sd = registry.field("incident", "short_description").unwrap();
    assert!(!sd.is_journal());
    assert!(!sd.read_only);
}

// ── Record Number Resolution tests ──────────────────────────────

#[tokio::test]
async fn test_prefix_resolution() {
    let server = MockServer::start().await;
    let client = test_client(&server).await;

    assert_eq!(client.table_for_prefix("INC"), Some("incident"));
    assert_eq!(client.table_for_prefix("CHG"), Some("change_request"));
    assert_eq!(client.table_for_prefix("CTASK"), Some("change_task"));
    assert_eq!(client.table_for_prefix("PRB"), Some("problem"));
    assert_eq!(client.table_for_prefix("RITM"), Some("sc_req_item"));
    assert_eq!(client.table_for_prefix("TASK"), Some("sc_task"));

    assert_eq!(client.table_for_number("INC0012345"), Some("incident"));
    assert_eq!(
        client.table_for_number("CHG0307336"),
        Some("change_request")
    );
    assert_eq!(client.table_for_number("TASK3462966"), Some("sc_task"));
    assert_eq!(client.table_for_number("UNKNOWN001"), None);
}

#[tokio::test]
async fn test_get_by_number() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_query", "number=INC0012345"))
        .and(query_param("sysparm_limit", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                {
                    "sys_id": "abc123",
                    "number": "INC0012345",
                    "short_description": "Network outage"
                }
            ]
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let record = client
        .get_by_number("INC0012345")
        .await
        .expect("get_by_number failed");

    assert!(record.is_some());
    let record = record.unwrap();
    assert_eq!(record.get_str("number"), Some("INC0012345"));
    assert_eq!(record.get_str("short_description"), Some("Network outage"));
}

#[tokio::test]
async fn test_get_by_number_sc_task_task_prefix() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/sc_task"))
        .and(query_param("sysparm_query", "number=TASK3462966"))
        .and(query_param("sysparm_limit", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                {
                    "sys_id": "sctask123",
                    "number": "TASK3462966",
                    "short_description": "Catalog task"
                }
            ]
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let record = client
        .get_by_number("TASK3462966")
        .await
        .expect("get_by_number failed");

    assert!(record.is_some());
    let record = record.unwrap();
    assert_eq!(record.get_str("number"), Some("TASK3462966"));
    assert_eq!(record.get_str("short_description"), Some("Catalog task"));
}

#[tokio::test]
async fn test_get_by_number_unknown_prefix() {
    let server = MockServer::start().await;
    let client = test_client(&server).await;

    let result = client.get_by_number("UNKNOWN001").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_custom_prefix_registration() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/u_custom_table"))
        .and(query_param("sysparm_query", "number=MYPREFIX0001"))
        .and(query_param("sysparm_limit", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                { "sys_id": "custom1", "number": "MYPREFIX0001" }
            ]
        })))
        .mount(&server)
        .await;

    let client = ServiceNowClient::builder()
        .instance(server.uri())
        .auth(BasicAuth::new("user", "pass"))
        .allow_http()
        .register_prefix("MYPREFIX", "u_custom_table")
        .build()
        .await
        .expect("build failed");

    assert_eq!(
        client.table_for_number("MYPREFIX0001"),
        Some("u_custom_table")
    );

    let record = client.get_by_number("MYPREFIX0001").await.unwrap();
    assert!(record.is_some());
}

// ── Journal Reader tests ────────────────────────────────────────

#[tokio::test]
async fn test_journal_convenience_method() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_journal_field"))
        .and(query_param(
            "sysparm_query",
            "element_id=inc_sys_id^element=work_notes^name=incident^ORDERBYDESCsys_created_on",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                {
                    "sys_id": "jf001",
                    "element_id": "inc_sys_id",
                    "element": "work_notes",
                    "value": "Escalated to network team",
                    "sys_created_on": "2026-03-25 11:00:00",
                    "sys_created_by": "admin"
                },
                {
                    "sys_id": "jf002",
                    "element_id": "inc_sys_id",
                    "element": "work_notes",
                    "value": "Checking network switch",
                    "sys_created_on": "2026-03-25 10:00:00",
                    "sys_created_by": "admin"
                }
            ]
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let notes = client
        .journal("incident", "inc_sys_id", "work_notes")
        .limit(10)
        .execute()
        .await
        .expect("journal query failed");

    assert_eq!(notes.len(), 2);
    assert_eq!(
        notes.records[0].get_str("value"),
        Some("Escalated to network team")
    );
    assert_eq!(notes.records[0].get_str("element"), Some("work_notes"));
}

#[tokio::test]
async fn test_journal_all_method() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_journal_field"))
        .and(query_param(
            "sysparm_query",
            "element_id=inc_sys_id^name=incident^ORDERBYDESCsys_created_on",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                {
                    "sys_id": "jf001",
                    "element": "work_notes",
                    "value": "Private note",
                    "sys_created_on": "2026-03-25 11:00:00",
                    "sys_created_by": "admin"
                },
                {
                    "sys_id": "jf002",
                    "element": "comments",
                    "value": "Public comment",
                    "sys_created_on": "2026-03-25 10:00:00",
                    "sys_created_by": "admin"
                }
            ]
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let all = client
        .journal_all("incident", "inc_sys_id")
        .limit(20)
        .execute()
        .await
        .expect("journal_all query failed");

    assert_eq!(all.len(), 2);
    // First is private, second is public.
    assert_eq!(all.records[0].get_str("element"), Some("work_notes"));
    assert_eq!(all.records[1].get_str("element"), Some("comments"));
}

#[tokio::test]
async fn test_journal_inline() {
    let server = MockServer::start().await;

    // Mock returns journal fields with display values (formatted text).
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_display_value", "true"))
        .and(query_param("sysparm_fields", "work_notes,comments"))
        .and(query_param(
            "sysparm_query",
            "sys_id=inc_sys_id",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                {
                    "sys_id": "inc_sys_id",
                    "work_notes": "2026-03-27 10:00:00 - admin (Work notes)\nEscalated to network team\n\n2026-03-27 09:00:00 - admin (Work notes)\nInitial triage complete",
                    "comments": "2026-03-27 11:00:00 - admin (Additional comments)\nWe are investigating this issue"
                }
            ]
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let record = client
        .journal_inline("incident", "inc_sys_id", &["work_notes", "comments"])
        .first()
        .await
        .expect("journal_inline query failed")
        .expect("record not found");

    // work_notes contains formatted journal text.
    let notes = record.get_str("work_notes").expect("work_notes missing");
    assert!(notes.contains("Escalated to network team"));
    assert!(notes.contains("Initial triage complete"));

    // comments contains formatted journal text.
    let comments = record.get_str("comments").expect("comments missing");
    assert!(comments.contains("We are investigating this issue"));
}

// ── Record Update Helper tests ──────────────────────────────────

#[tokio::test]
async fn test_add_work_note() {
    let server = MockServer::start().await;

    Mock::given(method("PATCH"))
        .and(path("/api/now/table/rm_scrum_task/stsk_sys_id"))
        .and(wiremock::matchers::body_json(json!({
            "work_notes": "Starting configuration work"
        })))
        .and(query_param("sysparm_display_value", "all"))
        .and(query_param("sysparm_fields", "sys_id,number,state"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {
                "sys_id": { "value": "stsk_sys_id", "display_value": "stsk_sys_id" },
                "number": { "value": "STSK0010001", "display_value": "STSK0010001" },
                "state": { "value": "-6", "display_value": "Draft" }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let record = client
        .add_work_note(
            "rm_scrum_task",
            "stsk_sys_id",
            "Starting configuration work",
        )
        .await
        .expect("add_work_note failed");

    assert_eq!(record.get_display("number"), Some("STSK0010001"));
    assert_eq!(record.get_display("state"), Some("Draft"));
}

#[tokio::test]
async fn test_set_state() {
    let server = MockServer::start().await;

    Mock::given(method("PATCH"))
        .and(path("/api/now/table/rm_scrum_task/stsk_sys_id"))
        .and(wiremock::matchers::body_json(json!({
            "state": "2",
            "work_notes": "Starting work on this task"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {
                "sys_id": { "value": "stsk_sys_id", "display_value": "stsk_sys_id" },
                "number": { "value": "STSK0010001", "display_value": "STSK0010001" },
                "state": { "value": "2", "display_value": "Work in progress" }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let record = client
        .set_state(
            "rm_scrum_task",
            "stsk_sys_id",
            "2",
            Some("Starting work on this task"),
        )
        .await
        .expect("set_state failed");

    assert_eq!(record.get_display("state"), Some("Work in progress"));
    assert_eq!(record.get_raw("state"), Some("2"));
}

#[tokio::test]
async fn test_set_state_without_note() {
    let server = MockServer::start().await;

    Mock::given(method("PATCH"))
        .and(path("/api/now/table/incident/inc_sys_id"))
        .and(wiremock::matchers::body_json(json!({ "state": "6" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {
                "sys_id": { "value": "inc_sys_id", "display_value": "inc_sys_id" },
                "number": { "value": "INC0010001", "display_value": "INC0010001" },
                "state": { "value": "6", "display_value": "Resolved" }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let record = client
        .set_state("incident", "inc_sys_id", "6", None)
        .await
        .expect("set_state without note failed");

    assert_eq!(record.get_display("state"), Some("Resolved"));
}

// ── Raw POST (Service Catalog, etc.) tests ──────────────────────

#[tokio::test]
async fn test_post_service_catalog_order() {
    let server = MockServer::start().await;

    // Mock the Service Catalog order_now endpoint.
    Mock::given(method("POST"))
        .and(path(
            "/api/sn_sc/servicecatalog/items/cat_item_sys_id/order_now",
        ))
        .and(wiremock::matchers::body_json(json!({
            "sysparm_quantity": "1",
            "variables": {
                "short_description": "Test request",
                "additional_comments": "Created via API",
                "requested_for": "user_sys_id"
            }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {
                "request_number": "REQ0010001",
                "request_id": "req_sys_id"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let result = client
        .post(
            "/api/sn_sc/servicecatalog/items/cat_item_sys_id/order_now",
            json!({
                "sysparm_quantity": "1",
                "variables": {
                    "short_description": "Test request",
                    "additional_comments": "Created via API",
                    "requested_for": "user_sys_id"
                }
            }),
        )
        .await
        .expect("catalog order failed");

    assert_eq!(
        result.get("request_number").and_then(|v| v.as_str()),
        Some("REQ0010001")
    );
    assert_eq!(
        result.get("request_id").and_then(|v| v.as_str()),
        Some("req_sys_id")
    );
}

#[tokio::test]
async fn test_post_returns_api_error() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/sn_sc/servicecatalog/items/bad_id/order_now"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": {
                "message": "Mandatory Variables are required",
                "detail": "missing required fields"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let result = client
        .post(
            "/api/sn_sc/servicecatalog/items/bad_id/order_now",
            json!({ "sysparm_quantity": "1", "variables": {} }),
        )
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Mandatory Variables"),
        "error should contain API message: {}",
        err
    );
}

// ── Requested Item (RITM) update tests ──────────────────────────

#[tokio::test]
async fn test_update_ritm_cmdb_ci() {
    let server = MockServer::start().await;

    // Mock PATCH to set cmdb_ci on a requested item.
    Mock::given(method("PATCH"))
        .and(path("/api/now/table/sc_req_item/ritm_sys_id"))
        .and(wiremock::matchers::body_json(json!({
            "cmdb_ci": "ci_sys_id",
            "work_notes": "Set CI to test application"
        })))
        .and(query_param("sysparm_display_value", "all"))
        .and(query_param(
            "sysparm_fields",
            "sys_id,number,cmdb_ci,assignment_group",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {
                "sys_id": { "value": "ritm_sys_id", "display_value": "ritm_sys_id" },
                "number": { "value": "RITM0010001", "display_value": "RITM0010001" },
                "cmdb_ci": { "value": "ci_sys_id", "display_value": "Test Application" },
                "assignment_group": { "value": "grp_sys_id", "display_value": "Engineering Team" }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let updated = client
        .table("sc_req_item")
        .fields(&["sys_id", "number", "cmdb_ci", "assignment_group"])
        .display_value(DisplayValue::Both)
        .update(
            "ritm_sys_id",
            json!({
                "cmdb_ci": "ci_sys_id",
                "work_notes": "Set CI to test application"
            }),
        )
        .await
        .expect("RITM cmdb_ci update failed");

    assert_eq!(updated.get_display("cmdb_ci"), Some("Test Application"));
    assert_eq!(updated.get_raw("cmdb_ci"), Some("ci_sys_id"));
    assert_eq!(
        updated.get_display("assignment_group"),
        Some("Engineering Team")
    );
}

#[tokio::test]
async fn test_update_ritm_assignment_group() {
    let server = MockServer::start().await;

    Mock::given(method("PATCH"))
        .and(path("/api/now/table/sc_req_item/ritm_sys_id"))
        .and(wiremock::matchers::body_json(json!({
            "assignment_group": "new_grp_sys_id",
            "work_notes": "Reassigned"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {
                "sys_id": { "value": "ritm_sys_id", "display_value": "ritm_sys_id" },
                "number": { "value": "RITM0010001", "display_value": "RITM0010001" },
                "assignment_group": { "value": "new_grp_sys_id", "display_value": "IAM Engineering" }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let updated = client
        .table("sc_req_item")
        .display_value(DisplayValue::Both)
        .update(
            "ritm_sys_id",
            json!({
                "assignment_group": "new_grp_sys_id",
                "work_notes": "Reassigned"
            }),
        )
        .await
        .expect("RITM assignment_group update failed");

    assert_eq!(
        updated.get_display("assignment_group"),
        Some("IAM Engineering")
    );
}

// ── Browser URL tests ───────────────────────────────────────────

#[tokio::test]
async fn test_browser_url() {
    let server = MockServer::start().await;
    let client = test_client(&server).await;
    let base = server.uri();

    let url = client.browser_url("incident", "INC0012345").unwrap();
    assert_eq!(
        url,
        format!(
            "{}/nav_to.do?uri=incident.do?sysparm_query=number=INC0012345",
            base
        )
    );
}

#[tokio::test]
async fn test_browser_url_by_id() {
    let server = MockServer::start().await;
    let client = test_client(&server).await;
    let base = server.uri();

    let url = client
        .browser_url_by_id("incident", "abc123def456")
        .unwrap();
    assert_eq!(
        url,
        format!("{}/nav_to.do?uri=incident.do?sys_id=abc123def456", base)
    );
}

#[tokio::test]
async fn test_browser_url_for_number() {
    let server = MockServer::start().await;
    let client = test_client(&server).await;
    let base = server.uri();

    let url = client.browser_url_for_number("INC0012345");
    assert!(url.is_some());
    assert_eq!(
        url.unwrap().unwrap(),
        format!(
            "{}/nav_to.do?uri=incident.do?sysparm_query=number=INC0012345",
            base
        )
    );

    // Unknown prefix returns None.
    assert!(client.browser_url_for_number("UNKNOWN001").is_none());
}

// ── Change Request + Change Task tests ──────────────────────────

#[tokio::test]
async fn test_create_change_request() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/now/table/change_request"))
        .and(wiremock::matchers::body_json(json!({
            "type": "normal",
            "short_description": "Test change request",
            "description": "Created via API",
            "assignment_group": "grp_sys_id",
            "cmdb_ci": "ci_sys_id",
            "change_plan": "Deploy and test",
            "backout_plan": "Revert if needed"
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "result": {
                "sys_id": "chg_sys_id",
                "number": "CHG0010001",
                "type": "normal",
                "state": "-5",
                "short_description": "Test change request",
                "assignment_group": "grp_sys_id",
                "cmdb_ci": "ci_sys_id"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let chg = client
        .table("change_request")
        .create(json!({
            "type": "normal",
            "short_description": "Test change request",
            "description": "Created via API",
            "assignment_group": "grp_sys_id",
            "cmdb_ci": "ci_sys_id",
            "change_plan": "Deploy and test",
            "backout_plan": "Revert if needed"
        }))
        .await
        .expect("create change_request failed");

    assert_eq!(chg.get_str("number"), Some("CHG0010001"));
    assert_eq!(chg.get_str("type"), Some("normal"));
    assert_eq!(chg.get_str("sys_id"), Some("chg_sys_id"));
}

#[tokio::test]
async fn test_create_change_task_with_parent() {
    let server = MockServer::start().await;

    // Create a change task linked via "parent" field (not "change_request"
    // which is read-only/computed on many instances).
    Mock::given(method("POST"))
        .and(path("/api/now/table/change_task"))
        .and(wiremock::matchers::body_json(json!({
            "parent": "chg_sys_id",
            "short_description": "Pre-Implementation Testing",
            "change_task_type": "planning",
            "assignment_group": "grp_sys_id"
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "result": {
                "sys_id": "ctask_sys_id",
                "number": "CTASK0010001",
                "short_description": "Pre-Implementation Testing",
                "change_task_type": "planning",
                "parent": "chg_sys_id",
                "state": "-5"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let task = client
        .table("change_task")
        .create(json!({
            "parent": "chg_sys_id",
            "short_description": "Pre-Implementation Testing",
            "change_task_type": "planning",
            "assignment_group": "grp_sys_id"
        }))
        .await
        .expect("create change_task failed");

    assert_eq!(task.get_str("number"), Some("CTASK0010001"));
    assert_eq!(task.get_str("parent"), Some("chg_sys_id"));
    assert_eq!(task.get_str("change_task_type"), Some("planning"));
}

#[tokio::test]
async fn test_query_change_tasks_via_parent() {
    let server = MockServer::start().await;

    // Change tasks should be queried via "parent" field to include
    // both auto-generated and manually created tasks.
    Mock::given(method("GET"))
        .and(path("/api/now/table/change_task"))
        .and(query_param("sysparm_query", "parent=chg_sys_id"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                {
                    "sys_id": "ctask1",
                    "number": "CTASK0010001",
                    "short_description": "Pre-Implementation Testing",
                    "change_task_type": "planning",
                    "parent": "chg_sys_id",
                    "change_request": "chg_sys_id"
                },
                {
                    "sys_id": "ctask2",
                    "number": "CTASK0010002",
                    "short_description": "Implementation",
                    "change_task_type": "implementation",
                    "parent": "chg_sys_id",
                    "change_request": "chg_sys_id"
                },
                {
                    "sys_id": "ctask3",
                    "number": "CTASK0010003",
                    "short_description": "Custom validation step",
                    "change_task_type": "planning",
                    "parent": "chg_sys_id",
                    "change_request": ""
                }
            ]
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let tasks = client
        .table("change_task")
        .equals("parent", "chg_sys_id")
        .execute()
        .await
        .expect("query change_tasks failed");

    // Should return all 3: 2 auto-generated + 1 manually created.
    assert_eq!(tasks.len(), 3);
    assert_eq!(tasks.records[0].get_str("number"), Some("CTASK0010001"));
    assert_eq!(tasks.records[2].get_str("number"), Some("CTASK0010003"));
    // The manually created one has empty change_request but valid parent.
    assert_eq!(tasks.records[2].get_str("change_request"), Some(""));
    assert_eq!(tasks.records[2].get_str("parent"), Some("chg_sys_id"));
}

#[tokio::test]
async fn test_create_and_link_change_task() {
    // On many ServiceNow instances, `change_request` is read-only on POST
    // but writable on PATCH. This test covers the two-step pattern:
    // 1. Create the task with `parent` field
    // 2. PATCH to set `change_request` so the UI shows it

    let server = MockServer::start().await;

    // Step 1: POST creates the task (change_request ignored on create).
    Mock::given(method("POST"))
        .and(path("/api/now/table/change_task"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "result": {
                "sys_id": "ctask_new",
                "number": "CTASK0010004",
                "short_description": "Implementation — deploy changes",
                "change_task_type": "implementation",
                "parent": "chg_sys_id",
                "change_request": "",
                "state": "-5"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    // Step 2: PATCH sets the change_request field.
    Mock::given(method("PATCH"))
        .and(path("/api/now/table/change_task/ctask_new"))
        .and(wiremock::matchers::body_json(json!({
            "change_request": "chg_sys_id"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {
                "sys_id": "ctask_new",
                "number": "CTASK0010004",
                "short_description": "Implementation — deploy changes",
                "change_task_type": "implementation",
                "parent": "chg_sys_id",
                "change_request": "chg_sys_id",
                "state": "-5"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    // Create.
    let task = client
        .table("change_task")
        .create(json!({
            "parent": "chg_sys_id",
            "change_request": "chg_sys_id",
            "short_description": "Implementation — deploy changes",
            "change_task_type": "implementation",
            "assignment_group": "grp_sys_id"
        }))
        .await
        .expect("create change_task failed");

    let task_sys_id = task.get_str("sys_id").unwrap_or("");
    // change_request is empty after create.
    assert_eq!(task.get_str("change_request"), Some(""));
    assert_eq!(task.get_str("parent"), Some("chg_sys_id"));

    // Link via PATCH.
    let linked = client
        .table("change_task")
        .update(task_sys_id, json!({ "change_request": "chg_sys_id" }))
        .await
        .expect("link change_task failed");

    // Now change_request is set.
    assert_eq!(linked.get_str("change_request"), Some("chg_sys_id"));
    assert_eq!(linked.get_str("parent"), Some("chg_sys_id"));
    assert_eq!(linked.get_str("number"), Some("CTASK0010004"));
}

// ── Approval tests ──────────────────────────────────────────────

#[tokio::test]
async fn test_approve_change_request() {
    let server = MockServer::start().await;

    // Step 1: Mock the lookup for the pending approval record (single OR query).
    Mock::given(method("GET"))
        .and(path("/api/now/table/sysapproval_approver"))
        .and(query_param(
            "sysparm_query",
            "(document_id=chg_sys_id^approver=user_sys_id^state=requested)^OR(sysapproval=chg_sys_id^approver=user_sys_id^state=requested)",
        ))
        .and(query_param("sysparm_limit", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                {
                    "sys_id": "appr_sys_id",
                    "document_id": "chg_sys_id",
                    "sysapproval": "chg_sys_id",
                    "approver": "user_sys_id",
                    "state": "requested",
                    "source_table": "change_request",
                    "comments": ""
                }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    // Step 2: Mock the PATCH to approve.
    Mock::given(method("PATCH"))
        .and(path("/api/now/table/sysapproval_approver/appr_sys_id"))
        .and(query_param("sysparm_display_value", "true"))
        .and(wiremock::matchers::body_json(json!({
            "state": "approved",
            "comments": "Looks good"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {
                "sys_id": "appr_sys_id",
                "sysapproval": "CHG0010001",
                "approver": "Test User",
                "state": "Approved",
                "source_table": "change_request",
                "comments": "Looks good",
                "sys_updated_on": "2026-03-27 17:30:00"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let approval = client
        .approve("change_request", "chg_sys_id", "user_sys_id")
        .comment("Looks good")
        .execute()
        .await
        .expect("approve failed");

    assert_eq!(approval.get_str("state"), Some("Approved"));
    assert_eq!(approval.get_str("comments"), Some("Looks good"));
    assert_eq!(approval.get_str("source_table"), Some("change_request"));
}

#[tokio::test]
async fn test_reject_change_request() {
    let server = MockServer::start().await;

    // Lookup mock (single OR query).
    Mock::given(method("GET"))
        .and(path("/api/now/table/sysapproval_approver"))
        .and(query_param(
            "sysparm_query",
            "(document_id=chg_sys_id^approver=user_sys_id^state=requested)^OR(sysapproval=chg_sys_id^approver=user_sys_id^state=requested)",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                {
                    "sys_id": "appr_sys_id",
                    "document_id": "chg_sys_id",
                    "sysapproval": "chg_sys_id",
                    "approver": "user_sys_id",
                    "state": "requested",
                    "source_table": "change_request",
                    "comments": ""
                }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    // PATCH mock for rejection.
    Mock::given(method("PATCH"))
        .and(path("/api/now/table/sysapproval_approver/appr_sys_id"))
        .and(wiremock::matchers::body_json(json!({
            "state": "rejected",
            "comments": "Missing test plan"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {
                "sys_id": "appr_sys_id",
                "sysapproval": "CHG0010001",
                "approver": "Test User",
                "state": "Rejected",
                "source_table": "change_request",
                "comments": "Missing test plan",
                "sys_updated_on": "2026-03-27 17:30:00"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let rejection = client
        .reject("change_request", "chg_sys_id", "user_sys_id")
        .comment("Missing test plan")
        .execute()
        .await
        .expect("reject failed");

    assert_eq!(rejection.get_str("state"), Some("Rejected"));
    assert_eq!(rejection.get_str("comments"), Some("Missing test plan"));
}

#[tokio::test]
async fn test_approve_no_pending_approval() {
    let server = MockServer::start().await;

    // Return empty results for the combined OR query.
    Mock::given(method("GET"))
        .and(path("/api/now/table/sysapproval_approver"))
        .and(query_param(
            "sysparm_query",
            "(document_id=chg_sys_id^approver=wrong_user_sys_id^state=requested)^OR(sysapproval=chg_sys_id^approver=wrong_user_sys_id^state=requested)",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": []
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let result = client
        .approve("change_request", "chg_sys_id", "wrong_user_sys_id")
        .execute()
        .await;

    assert!(
        result.is_err(),
        "should fail when no pending approval found"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("no pending approval found"),
        "error should explain the failure: {}",
        err
    );
}

#[tokio::test]
async fn test_approve_without_comment() {
    let server = MockServer::start().await;

    // Lookup mock (single OR query).
    Mock::given(method("GET"))
        .and(path("/api/now/table/sysapproval_approver"))
        .and(query_param(
            "sysparm_query",
            "(document_id=chg_sys_id^approver=user_sys_id^state=requested)^OR(sysapproval=chg_sys_id^approver=user_sys_id^state=requested)",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [{
                "sys_id": "appr_sys_id",
                "document_id": "chg_sys_id",
                "sysapproval": "chg_sys_id",
                "approver": "user_sys_id",
                "state": "requested",
                "source_table": "change_request",
                "comments": ""
            }]
        })))
        .mount(&server)
        .await;

    // PATCH mock — body should only contain state, no comments key.
    Mock::given(method("PATCH"))
        .and(path("/api/now/table/sysapproval_approver/appr_sys_id"))
        .and(wiremock::matchers::body_json(
            json!({ "state": "approved" }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {
                "sys_id": "appr_sys_id",
                "sysapproval": "CHG0010001",
                "approver": "Test User",
                "state": "Approved",
                "source_table": "change_request",
                "comments": "",
                "sys_updated_on": "2026-03-27 17:30:00"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let approval = client
        .approve("change_request", "chg_sys_id", "user_sys_id")
        .execute()
        .await
        .expect("approve without comment failed");

    assert_eq!(approval.get_str("state"), Some("Approved"));
}

/// Verifies that approval lookup issues exactly one GET request using a
/// combined `^OR` query instead of two sequential queries.
#[tokio::test]
async fn test_approve_uses_single_or_query() {
    let server = MockServer::start().await;

    // The single GET must use an OR query and be called exactly once.
    Mock::given(method("GET"))
        .and(path("/api/now/table/sysapproval_approver"))
        .and(query_param(
            "sysparm_query",
            "(document_id=chg_sys_id^approver=user_sys_id^state=requested)^OR(sysapproval=chg_sys_id^approver=user_sys_id^state=requested)",
        ))
        .and(query_param("sysparm_limit", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [{
                "sys_id": "appr_sys_id",
                "sysapproval": "chg_sys_id",
                "approver": "user_sys_id",
                "state": "requested",
                "source_table": "change_request",
                "comments": ""
            }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("PATCH"))
        .and(path("/api/now/table/sysapproval_approver/appr_sys_id"))
        .and(wiremock::matchers::body_json(
            json!({ "state": "approved" }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {
                "sys_id": "appr_sys_id",
                "sysapproval": "CHG0010001",
                "approver": "Test User",
                "state": "Approved",
                "source_table": "change_request",
                "comments": "",
                "sys_updated_on": "2026-03-27 17:30:00"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server).await;

    let approval = client
        .approve("change_request", "chg_sys_id", "user_sys_id")
        .execute()
        .await
        .expect("approve with OR query failed");

    assert_eq!(approval.get_str("state"), Some("Approved"));
}

#[tokio::test]
async fn test_attachment_list_for_record() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/attachment"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [{
                "sys_id": "att_sys_id",
                "file_name": "evidence.jpeg",
                "table_name": "change_request",
                "table_sys_id": "chg_sys_id",
                "content_type": "image/jpeg",
                "size_bytes": "83338",
                "download_link": "https://example.service-now.com/api/now/attachment/att_sys_id/file"
            }]
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let attachments = client
        .list_attachments("change_request", "chg_sys_id")
        .await
        .expect("attachments");

    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].sys_id, "att_sys_id");
    assert_eq!(attachments[0].file_name, "evidence.jpeg");
    assert_eq!(attachments[0].size_bytes, Some(83338));

    let requests = requests_for_path(&server, "/api/now/attachment").await;
    assert_eq!(requests.len(), 1);
    assert_eq!(
        request_query(&requests[0]).as_deref(),
        Some("table_name=change_request^table_sys_id=chg_sys_id")
    );
    assert!(request_query_param(&requests[0], "sysparm_fields")
        .expect("fields")
        .contains("download_link"));
}

#[tokio::test]
async fn test_attachment_upload_posts_binary_body() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/now/attachment/file"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "result": {
                "sys_id": "att_sys_id",
                "file_name": "evidence.jpeg",
                "table_name": "change_request",
                "table_sys_id": "chg_sys_id",
                "content_type": "image/jpeg",
                "size_bytes": "4"
            }
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let attachment = client
        .upload_attachment_bytes(
            "change_request",
            "chg_sys_id",
            "evidence.jpeg",
            "image/jpeg",
            b"jpeg".to_vec(),
        )
        .await
        .expect("attachment");

    assert_eq!(attachment.sys_id, "att_sys_id");
    assert_eq!(attachment.file_name, "evidence.jpeg");
    assert_eq!(attachment.size_bytes, Some(4));

    let requests = requests_for_path(&server, "/api/now/attachment/file").await;
    assert_eq!(requests.len(), 1);
    assert_eq!(
        request_query_param(&requests[0], "table_name").as_deref(),
        Some("change_request")
    );
    assert_eq!(
        request_query_param(&requests[0], "table_sys_id").as_deref(),
        Some("chg_sys_id")
    );
    assert_eq!(
        request_query_param(&requests[0], "file_name").as_deref(),
        Some("evidence.jpeg")
    );
    assert_eq!(requests[0].body, b"jpeg");
    assert_eq!(
        requests[0]
            .headers
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("image/jpeg")
    );
}
