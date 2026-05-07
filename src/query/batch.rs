use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use futures::future::join_all;
use futures::stream::{self, StreamExt, TryStreamExt};
use tracing::debug;

use crate::error::{Error, Result};
use crate::model::record::Record;
use crate::model::value::DisplayValue;
use crate::schema::definition::RelationshipDef;
use crate::transport::TransportHandle;

pub(crate) const RELATED_QUERY_CHUNK_SIZE: usize = 100;
pub(crate) const RELATED_QUERY_MAX_CONCURRENCY: usize = 4;

/// Fetch related records for a set of parent records using concurrent requests.
///
/// For each relationship, fetches related records with chunked IN queries, then
/// distributes the results back to the correct parent records.
pub async fn fetch_related_concurrent(
    transport: TransportHandle,
    parent_table: &str,
    parents: &mut [Record],
    relationships: &[(&str, &RelationshipDef)],
    display_value: DisplayValue,
) -> Vec<Error> {
    if parents.is_empty() || relationships.is_empty() {
        return Vec::new();
    }

    // Collect all parent sys_ids.
    let sys_ids = unique_parent_sys_ids(parents);

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
            let transport = Arc::clone(&transport);
            let sys_ids = sys_ids.clone();
            let rel_name = rel_name.to_string();
            let rel_def = (*rel_def).clone();

            async move {
                let result = fetch_relationship_with_raw_refs(
                    transport.as_ref(),
                    &sys_ids,
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
pub(crate) async fn fetch_relationship_with_raw_refs(
    transport: &dyn crate::transport::Transport,
    parent_sys_ids: &[String],
    rel_def: &RelationshipDef,
    display_value: DisplayValue,
) -> Result<Vec<Record>> {
    if parent_sys_ids.is_empty() {
        return Ok(Vec::new());
    }

    let chunks = parent_id_chunks(parent_sys_ids);
    let chunk_count = chunks.len();

    let chunk_records: Vec<Vec<Record>> = stream::iter(chunks)
        .map(|sys_id_list| async move {
            fetch_relationship_chunk(transport, &sys_id_list, rel_def, display_value).await
        })
        .buffer_unordered(RELATED_QUERY_MAX_CONCURRENCY)
        .try_collect()
        .await?;

    let records = chunk_records.into_iter().flatten().collect::<Vec<_>>();

    debug!(
        table = rel_def.table,
        count = records.len(),
        chunks = chunk_count,
        "fetched related records"
    );

    Ok(records)
}

async fn fetch_relationship_chunk(
    transport: &dyn crate::transport::Transport,
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

    let path = format!("{}/{}", crate::api::table::TABLE_API_PATH, rel_def.table);
    let response_display_value = match display_value {
        // Related-record matching needs the raw foreign key to be present.
        // Request "all" here so reference fields still carry their sys_id
        // even when the caller wants display-oriented accessors.
        DisplayValue::Display => DisplayValue::Both,
        other => other,
    };
    let params = vec![
        ("sysparm_query".to_string(), query),
        (
            "sysparm_display_value".to_string(),
            response_display_value.as_param().to_string(),
        ),
        (
            "sysparm_exclude_reference_link".to_string(),
            "true".to_string(),
        ),
        // Cap related-record fetches to prevent silently truncated results.
        ("sysparm_limit".to_string(), "10000".to_string()),
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

    Ok(records)
}

fn unique_parent_sys_ids(parents: &[Record]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut sys_ids = Vec::new();

    for parent in parents {
        if seen.insert(parent.sys_id.as_str()) {
            sys_ids.push(parent.sys_id.clone());
        }
    }

    sys_ids
}

fn parent_id_chunks(parent_sys_ids: &[String]) -> Vec<String> {
    parent_sys_ids
        .chunks(RELATED_QUERY_CHUNK_SIZE)
        .map(|chunk| {
            chunk
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_parent_sys_ids_dedupes_in_request_order() {
        let parents = vec![
            Record::new("task", "one"),
            Record::new("task", "two"),
            Record::new("task", "one"),
            Record::new("task", "three"),
        ];

        assert_eq!(
            unique_parent_sys_ids(&parents),
            vec!["one".to_string(), "two".to_string(), "three".to_string()]
        );
    }

    #[test]
    fn parent_id_chunks_caps_each_in_query_at_100_ids() {
        let parent_sys_ids = (0..205).map(|i| format!("id{i:03}")).collect::<Vec<_>>();

        let chunks = parent_id_chunks(&parent_sys_ids);

        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].split(',').count(), RELATED_QUERY_CHUNK_SIZE);
        assert_eq!(chunks[1].split(',').count(), RELATED_QUERY_CHUNK_SIZE);
        assert_eq!(chunks[2].split(',').count(), 5);
        assert!(chunks[0].starts_with("id000,id001"));
        assert!(chunks[2].ends_with("id204"));
    }
}
