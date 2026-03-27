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

// ── Journal / Notes tests ───────────────────────────────────────

#[tokio::test]
async fn test_live_journal_fields_empty_on_get() {
    if !should_run() {
        return;
    }
    let client = live_client().await;

    // work_notes and comments return empty on GET — this is by design.
    let result = client
        .table("incident")
        .fields(&["number", "work_notes", "comments"])
        .limit(3)
        .execute()
        .await
        .expect("journal field query failed");

    for record in &result {
        let num = record.get_str("number").unwrap_or("?");
        let wn = record.get_str("work_notes").unwrap_or("(null)");
        let cm = record.get_str("comments").unwrap_or("(null)");
        println!("{}: work_notes='{}', comments='{}'", num, wn, cm);
        // Journal fields always return empty on GET.
        assert!(
            wn.is_empty() || wn == "(null)",
            "work_notes should be empty on GET, got: '{}'",
            wn
        );
    }
}

#[tokio::test]
async fn test_live_journal_entries_via_sys_journal_field() {
    if !should_run() {
        return;
    }
    let client = live_client().await;

    // Get an incident that likely has journal entries.
    let incidents = client
        .table("incident")
        .fields(&["sys_id", "number"])
        .limit(1)
        .execute()
        .await
        .expect("incident query failed");

    let inc = &incidents.records[0];
    let sys_id = &inc.sys_id;
    let number = inc.get_str("number").unwrap_or("?");
    println!("Checking journal entries for {}", number);

    // Query sys_journal_field for this incident's entries.
    let journal = client
        .table("sys_journal_field")
        .equals("element_id", sys_id)
        .fields(&["element", "value", "sys_created_on", "sys_created_by"])
        .order_by("sys_created_on", Order::Desc)
        .limit(10)
        .execute()
        .await
        .expect("journal query failed");

    println!("Found {} journal entries for {}", journal.len(), number);

    for entry in &journal {
        let element = entry.get_str("element").unwrap_or("?");
        let by = entry.get_str("sys_created_by").unwrap_or("?");
        let at = entry.get_str("sys_created_on").unwrap_or("?");
        let val = entry
            .get_str("value")
            .unwrap_or("")
            .chars()
            .take(80)
            .collect::<String>();
        let note_type = match element {
            "work_notes" => "PRIVATE",
            "comments" => "PUBLIC",
            _ => "OTHER",
        };
        println!("  [{}] {} by {} at {}: {}", note_type, element, by, at, val);
    }
}

#[tokio::test]
async fn test_live_change_request_notes_relationship() {
    if !should_run() {
        return;
    }
    let client = live_client().await;

    // Fetch a change request with its work_notes relationship.
    let result = client
        .table("change_request")
        .fields(&["number", "short_description"])
        .include_related(&["work_notes"])
        .limit(1)
        .execute()
        .await
        .expect("change_request notes query failed");

    let chg = &result.records[0];
    let num = chg.get_str("number").unwrap_or("?");
    let notes = chg.related("work_notes");

    println!("{}: {} journal entries via relationship", num, notes.len());
    for entry in notes.iter().take(5) {
        let element = entry.get_str("element").unwrap_or("?");
        let by = entry.get_str("sys_created_by").unwrap_or("?");
        println!("  {} by {}", element, by);
    }
}

#[tokio::test]
async fn test_live_separate_public_private_notes() {
    if !should_run() {
        return;
    }
    let client = live_client().await;

    // Get an incident sys_id.
    let inc = client
        .table("incident")
        .fields(&["sys_id", "number"])
        .limit(1)
        .execute()
        .await
        .expect("query failed");

    let sys_id = &inc.records[0].sys_id;

    // Query private notes (work_notes).
    let private = client
        .table("sys_journal_field")
        .equals("element_id", sys_id)
        .equals("element", "work_notes")
        .fields(&["element", "value", "sys_created_by"])
        .limit(5)
        .execute()
        .await
        .expect("private notes query failed");

    // Query public notes (comments).
    let public = client
        .table("sys_journal_field")
        .equals("element_id", sys_id)
        .equals("element", "comments")
        .fields(&["element", "value", "sys_created_by"])
        .limit(5)
        .execute()
        .await
        .expect("public notes query failed");

    println!(
        "Private (work_notes): {}, Public (comments): {}",
        private.len(),
        public.len()
    );

    for r in &private {
        assert_eq!(r.get_str("element"), Some("work_notes"));
    }
    for r in &public {
        assert_eq!(r.get_str("element"), Some("comments"));
    }
}

// ── Record Number Resolution (live) ─────────────────────────────

#[tokio::test]
async fn test_live_get_by_number() {
    if !should_run() {
        return;
    }
    let client = live_client().await;

    // Resolve a change request by number.
    let record = client
        .get_by_number("CHG0307336")
        .await
        .expect("get_by_number failed");

    assert!(record.is_some(), "expected to find CHG0307336");
    let record = record.unwrap();
    assert_eq!(record.get_str("number"), Some("CHG0307336"));
    println!(
        "Resolved CHG0307336: {}",
        record.get_str("short_description").unwrap_or("?")
    );
}

#[tokio::test]
async fn test_live_prefix_resolution() {
    if !should_run() {
        return;
    }
    let client = live_client().await;

    assert_eq!(client.table_for_number("INC0012345"), Some("incident"));
    assert_eq!(
        client.table_for_number("RITM2513403"),
        Some("sc_req_item")
    );
    assert_eq!(client.table_for_number("UNKNOWN001"), None);
}

// ── Journal Reader Convenience (live) ───────────────────────────

#[tokio::test]
async fn test_live_journal_convenience() {
    if !should_run() {
        return;
    }
    let client = live_client().await;

    // Get an incident sys_id.
    let inc = client
        .table("incident")
        .fields(&["sys_id", "number"])
        .limit(1)
        .execute()
        .await
        .expect("query failed");
    let sys_id = &inc.records[0].sys_id;
    let number = inc.records[0].get_str("number").unwrap_or("?");

    // Use journal convenience method.
    let notes = client
        .journal("incident", sys_id, "work_notes")
        .limit(5)
        .execute()
        .await
        .expect("journal convenience failed");

    println!(
        "{}: {} work_notes via journal() method",
        number,
        notes.len()
    );

    // Use journal_all convenience method.
    let all = client
        .journal_all("incident", sys_id)
        .limit(5)
        .execute()
        .await
        .expect("journal_all convenience failed");

    println!(
        "{}: {} total journal entries via journal_all()",
        number,
        all.len()
    );
}

// ── Browser URL Construction (live) ─────────────────────────────

#[tokio::test]
async fn test_live_browser_url() {
    if !should_run() {
        return;
    }
    let client = live_client().await;

    let url = client.browser_url("incident", "INC0012345");
    assert!(url.contains("nav_to.do"));
    assert!(url.contains("incident.do"));
    assert!(url.contains("INC0012345"));
    println!("Browser URL: {}", url);

    let url_by_id = client.browser_url_by_id("incident", "abc123");
    assert!(url_by_id.contains("sys_id=abc123"));

    let url_for_number = client.browser_url_for_number("CHG0307336");
    assert!(url_for_number.is_some());
    let url_for_number = url_for_number.unwrap();
    assert!(url_for_number.contains("change_request.do"));
    println!("Browser URL for CHG: {}", url_for_number);
}
