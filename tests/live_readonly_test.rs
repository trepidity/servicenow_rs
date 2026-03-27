//! Integration tests against a live ServiceNow instance.
//!
//! **READ ONLY** — these tests only perform GET queries.
//! They are ignored by default; run with:
//!   SERVICENOW_LIVE_TEST=1 cargo test --test live_readonly_test
//!
//! Required env vars:
//!   SERVICENOW_INSTANCE, SERVICENOW_USERNAME, SERVICENOW_PASSWORD

use servicenow_rs::prelude::*;

fn should_run() -> bool {
    std::env::var("SERVICENOW_LIVE_TEST").is_ok()
}

async fn live_client() -> ServiceNowClient {
    let instance = std::env::var("SERVICENOW_INSTANCE")
        .expect("SERVICENOW_INSTANCE env var required for live tests");
    let username = std::env::var("SERVICENOW_USERNAME")
        .expect("SERVICENOW_USERNAME env var required for live tests");
    let password = std::env::var("SERVICENOW_PASSWORD")
        .expect("SERVICENOW_PASSWORD env var required for live tests");

    ServiceNowClient::builder()
        .instance(instance)
        .auth(BasicAuth::new(username, password))
        .schema_release("xanadu")
        .build()
        .await
        .expect("failed to build live client")
}

#[tokio::test]
async fn test_live_simple_query() {
    if !should_run() {
        return;
    }
    let client = live_client().await;

    let result = client
        .table("incident")
        .fields(&["number", "short_description", "state"])
        .limit(3)
        .execute()
        .await
        .expect("query failed");

    assert!(result.len() <= 3);
    assert!(result.len() > 0, "expected at least 1 incident");
    for record in &result {
        assert!(record.get_str("number").is_some(), "missing number field");
    }
    println!(
        "Fetched {} incidents (total: {:?})",
        result.len(),
        result.total_count
    );
}

#[tokio::test]
async fn test_live_dot_walk() {
    if !should_run() {
        return;
    }
    let client = live_client().await;

    let result = client
        .table("change_request")
        .fields(&["number", "short_description"])
        .dot_walk(&["assigned_to.name", "assigned_to.email"])
        .limit(3)
        .execute()
        .await
        .expect("dot-walk query failed");

    assert!(!result.is_empty(), "expected at least 1 change request");
    for record in &result {
        let num = record.get_str("number").unwrap_or("?");
        let name = record.get_str("assigned_to.name").unwrap_or("(empty)");
        println!("{}: assigned_to.name = {}", num, name);
        // Verify the dot-walked field exists as a flat key.
        assert!(
            record.has_field("assigned_to.name"),
            "dot-walked field 'assigned_to.name' not found"
        );
    }
}

#[tokio::test]
async fn test_live_deep_dot_walk() {
    if !should_run() {
        return;
    }
    let client = live_client().await;

    let result = client
        .table("incident")
        .fields(&["number"])
        .dot_walk(&["caller_id.name", "caller_id.manager.name"])
        .display_value(DisplayValue::Both)
        .limit(2)
        .execute()
        .await
        .expect("deep dot-walk query failed");

    for record in &result {
        let num = record.get_display("number").unwrap_or("?");
        let caller = record.get_str("caller_id.name").unwrap_or("(empty)");
        let mgr = record.get_str("caller_id.manager.name").unwrap_or("(empty)");
        println!("{}: caller={}, manager={}", num, caller, mgr);
    }
}

#[tokio::test]
async fn test_live_display_value_both() {
    if !should_run() {
        return;
    }
    let client = live_client().await;

    let result = client
        .table("incident")
        .fields(&["number", "state", "assigned_to"])
        .display_value(DisplayValue::Both)
        .limit(2)
        .execute()
        .await
        .expect("display_value=both query failed");

    for record in &result {
        // With display_value=all, state should have both raw and display values.
        if let Some(fv) = record.get("state") {
            println!(
                "state: raw={:?}, display={:?}",
                fv.raw_str(),
                fv.display_str()
            );
        }
        // assigned_to should have a link.
        if let Some(fv) = record.get("assigned_to") {
            println!(
                "assigned_to: display={:?}, link={:?}",
                fv.display_str(),
                fv.link
            );
        }
    }
}

#[tokio::test]
async fn test_live_pagination() {
    if !should_run() {
        return;
    }
    let client = live_client().await;

    let mut paginator = client
        .table("incident")
        .fields(&["number"])
        .equals("state", "1")
        .limit(5) // page size of 5
        .paginate();

    let mut total_records = 0;
    let mut pages = 0;

    // Fetch up to 3 pages.
    while let Some(page) = paginator.next_page().await.expect("pagination failed") {
        pages += 1;
        total_records += page.len();
        println!(
            "Page {}: {} records (total_count: {:?})",
            pages,
            page.len(),
            paginator.total_count()
        );
        if pages >= 3 {
            break;
        }
    }

    assert!(pages > 0, "expected at least 1 page");
    println!(
        "Fetched {} records across {} pages",
        total_records, pages
    );
}

#[tokio::test]
async fn test_live_execute_all_with_limit() {
    if !should_run() {
        return;
    }
    let client = live_client().await;

    let result = client
        .table("change_request")
        .fields(&["number", "short_description"])
        .limit(10) // page size
        .execute_all(Some(25)) // max 25 records total
        .await
        .expect("execute_all failed");

    println!(
        "execute_all: {} records (total_count: {:?})",
        result.len(),
        result.total_count
    );
    assert!(result.len() <= 25, "should respect max_records limit");
}

#[tokio::test]
async fn test_live_related_records() {
    if !should_run() {
        return;
    }
    let client = live_client().await;

    // Find a change request that likely has tasks.
    let result = client
        .table("change_request")
        .fields(&["number", "short_description"])
        .include_related(&["change_task"])
        .limit(3)
        .execute()
        .await
        .expect("related records query failed");

    for record in &result {
        let num = record.get_str("number").unwrap_or("?");
        let tasks = record.related("change_task");
        println!("{}: {} change tasks", num, tasks.len());
        for task in tasks.iter().take(3) {
            println!(
                "  - {}",
                task.get_str("number").unwrap_or("?")
            );
        }
    }
}

#[tokio::test]
async fn test_live_complex_filter() {
    if !should_run() {
        return;
    }
    let client = live_client().await;

    let result = client
        .table("incident")
        .contains("short_description", "network")
        .fields(&["number", "short_description", "state"])
        .order_by("sys_created_on", Order::Desc)
        .limit(5)
        .execute()
        .await
        .expect("complex filter failed");

    println!("Found {} incidents matching 'network'", result.len());
    for record in &result {
        println!(
            "  {} - {}",
            record.get_str("number").unwrap_or("?"),
            record.get_str("short_description").unwrap_or("?")
        );
    }
}

#[tokio::test]
async fn test_live_aggregate_count() {
    if !should_run() {
        return;
    }
    let client = live_client().await;

    let stats = client
        .aggregate("incident")
        .count()
        .execute()
        .await
        .expect("aggregate count failed");

    println!("Total incidents: {}", stats.count());
    assert!(stats.count() > 0, "expected non-zero incident count");
}

#[tokio::test]
async fn test_live_aggregate_grouped() {
    if !should_run() {
        return;
    }
    let client = live_client().await;

    let stats = client
        .aggregate("incident")
        .count()
        .group_by("state")
        .execute()
        .await
        .expect("aggregate grouped failed");

    assert!(stats.is_grouped());
    println!("Incident counts by state:");
    for group in stats.groups() {
        println!("  state={}: {}", group.field_value("state"), group.count());
    }
}

#[tokio::test]
async fn test_live_aggregate_with_filter() {
    if !should_run() {
        return;
    }
    let client = live_client().await;

    let stats = client
        .aggregate("incident")
        .count()
        .equals("state", "1")
        .execute()
        .await
        .expect("aggregate with filter failed");

    println!("New incidents (state=1): {}", stats.count());
}

#[tokio::test]
async fn test_live_count_method() {
    if !should_run() {
        return;
    }
    let client = live_client().await;

    // Test the TableApi .count() convenience method.
    let count = client
        .table("incident")
        .equals("state", "1")
        .count()
        .await
        .expect("count failed");

    println!("Incident count via .count(): {}", count);
    assert!(count > 0);
}
