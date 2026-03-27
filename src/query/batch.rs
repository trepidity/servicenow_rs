use std::collections::HashMap;
use std::sync::Arc;

use futures::future::join_all;
use tracing::debug;

use crate::error::{Error, Result};
use crate::model::record::Record;
use crate::model::value::DisplayValue;
use crate::schema::definition::RelationshipDef;
use crate::transport::http::HttpTransport;

/// Fetch related records for a set of parent records using concurrent requests.
///
/// For each relationship, fires a single query that fetches all related records
/// for all parent sys_ids at once (using IN operator), then distributes the results
/// back to the correct parent records.
pub async fn fetch_related_concurrent(
    transport: &Arc<HttpTransport>,
    parent_table: &str,
    parents: &mut [Record],
    relationships: &[(&str, &RelationshipDef)],
    display_value: DisplayValue,
) -> Vec<Error> {
    if parents.is_empty() || relationships.is_empty() {
        return Vec::new();
    }

    // Collect all parent sys_ids.
    let sys_ids: Vec<&str> = parents.iter().map(|r| r.sys_id.as_str()).collect();
    let sys_id_list = sys_ids.join(",");

    debug!(
        parent_table = parent_table,
        parent_count = sys_ids.len(),
        relationships = relationships.len(),
        "fetching related records concurrently"
    );

    // Fire one request per relationship, all in parallel.
    let futures: Vec<_> = relationships
        .iter()
        .map(|(rel_name, rel_def)| {
            let transport = Arc::clone(transport);
            let sys_id_list = sys_id_list.clone();
            let rel_name = rel_name.to_string();
            let rel_def = (*rel_def).clone();

            async move {
                let result = fetch_relationship(
                    &transport,
                    &sys_id_list,
                    &rel_def,
                    display_value,
                )
                .await;
                (rel_name, rel_def, result)
            }
        })
        .collect();

    let results = join_all(futures).await;

    let mut errors = Vec::new();

    // Distribute results to parent records.
    for (rel_name, rel_def, result) in results {
        match result {
            Ok(related_records) => {
                // Build a map: foreign_key_value -> Vec<Record>
                let mut by_parent: HashMap<String, Vec<Record>> = HashMap::new();
                for record in related_records {
                    // The foreign key field on the related record points to the parent.
                    let parent_id = record
                        .get_raw(&rel_def.foreign_key)
                        .or_else(|| record.get_str(&rel_def.foreign_key))
                        .unwrap_or_default()
                        .to_string();
                    by_parent.entry(parent_id).or_default().push(record);
                }

                // Attach to each parent.
                for parent in parents.iter_mut() {
                    if let Some(children) = by_parent.remove(&parent.sys_id) {
                        parent.set_related(&rel_name, children);
                    }
                }
            }
            Err(e) => {
                debug!(
                    relationship = rel_name,
                    error = %e,
                    "failed to fetch related records"
                );
                errors.push(e);
            }
        }
    }

    errors
}

/// Fetch all records from a relationship table matching the given parent sys_ids.
async fn fetch_relationship(
    transport: &HttpTransport,
    sys_id_list: &str,
    rel_def: &RelationshipDef,
    display_value: DisplayValue,
) -> Result<Vec<Record>> {
    // Build encoded query: foreign_key IN sys_id1,sys_id2,...
    let mut query = format!("{}IN{}", rel_def.foreign_key, sys_id_list);

    // Append additional filter if specified in the relationship definition.
    if let Some(ref filter) = rel_def.filter {
        query.push('^');
        query.push_str(filter);
    }

    let path = format!("/api/now/table/{}", rel_def.table);
    let params = vec![
        ("sysparm_query".to_string(), query),
        (
            "sysparm_display_value".to_string(),
            display_value.as_param().to_string(),
        ),
        (
            "sysparm_exclude_reference_link".to_string(),
            "true".to_string(),
        ),
    ];

    let response = transport.get(&path, &params).await?;

    // Parse the result array.
    let records = match response.result {
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|v| Record::from_json(&rel_def.table, v, display_value))
            .collect(),
        _ => Vec::new(),
    };

    debug!(
        table = rel_def.table,
        count = records.len(),
        "fetched related records"
    );

    Ok(records)
}
