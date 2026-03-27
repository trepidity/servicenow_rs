use serde_json::json;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use servicenow_rs::prelude::*;

/// Helper to build a client pointing at a wiremock server.
async fn test_client(server: &MockServer) -> ServiceNowClient {
    ServiceNowClient::builder()
        .instance(server.uri())
        .auth(BasicAuth::new("test_user", "test_pass"))
        .schema_release("xanadu")
        .build()
        .await
        .expect("failed to build test client")
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

#[tokio::test]
async fn test_client_from_env() {
    // Set env vars for the test.
    std::env::set_var("SERVICENOW_INSTANCE", "testinstance");
    std::env::set_var("SERVICENOW_USERNAME", "env_user");
    std::env::set_var("SERVICENOW_PASSWORD", "env_pass");

    let client = ServiceNowClient::from_env().await;
    assert!(client.is_ok());

    // Clean up.
    std::env::remove_var("SERVICENOW_INSTANCE");
    std::env::remove_var("SERVICENOW_USERNAME");
    std::env::remove_var("SERVICENOW_PASSWORD");
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
