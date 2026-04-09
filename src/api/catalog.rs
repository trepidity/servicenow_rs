//! Catalog variable fetching for Requested Items (RITMs).
//!
//! ServiceNow stores catalog variables (the form fields users fill out when
//! submitting a catalog request) in a separate table chain:
//!
//! 1. `sc_item_option_mtom` — join table linking RITM sys_id → variable sys_id
//! 2. `sc_item_option` — the actual variable name + value
//! 3. `item_option_new` — the variable definition (type, reference table, etc.)
//!
//! This module provides [`CatalogVariable`] and a helper method on
//! [`ServiceNowClient`](crate::client::ServiceNowClient) to fetch them.

use std::collections::HashMap;
use std::sync::Arc;

use crate::error::Result;
use crate::model::DisplayValue;
use crate::query::builder::TableApi;
use crate::transport::TransportHandle;

/// A single catalog variable from a requested item.
#[derive(Debug, Clone)]
pub struct CatalogVariable {
    /// Human-readable variable label (e.g. `"Business justification"`).
    pub name: String,
    /// The user-provided value (text, date, or sys_id for reference fields).
    pub value: String,
    /// Form display order.
    pub order: u32,
    /// For reference and list collector variables, the table the value points to.
    /// When set, the `value` contains one or more sys_ids (comma-separated for
    /// list collectors) that can be resolved via
    /// [`resolve_catalog_variables`](crate::client::ServiceNowClient::resolve_catalog_variables).
    pub reference_table: Option<String>,
}

/// Fetch catalog variables for a requested item (RITM).
///
/// Performs three queries:
/// 1. `sc_item_option_mtom` to get the variable sys_ids linked to the RITM
/// 2. `sc_item_option` to get the variable names and values
/// 3. `item_option_new` to get variable type metadata (reference table)
///
/// Results are sorted by form order. Variables with no label are excluded.
pub(crate) async fn fetch_catalog_variables(
    transport: TransportHandle,
    ritm_sys_id: &str,
) -> Result<Vec<CatalogVariable>> {
    // Step 1: get the many-to-many links
    let mtom_api = TableApi::new(Arc::clone(&transport), None, "sc_item_option_mtom")
        .equals("request_item", ritm_sys_id);
    let mtom = mtom_api.execute().await?;

    let opt_ids: Vec<&str> = mtom.records.iter()
        .filter_map(|r| r.get_str("sc_item_option"))
        .collect();
    if opt_ids.is_empty() {
        return Ok(Vec::new());
    }

    // Step 2: fetch the actual option records (name + value).
    // Use Both mode so we can read the raw item_option_new sys_id (via get_raw)
    // while still getting the display label (via get_str).
    let opts_api = TableApi::new(Arc::clone(&transport), None, "sc_item_option")
        .in_list("sys_id", &opt_ids)
        .fields(&["sys_id", "item_option_new", "value", "order"])
        .display_value(DisplayValue::Both);
    let opts = opts_api.execute().await?;

    // Collect unique variable-definition sys_ids for the metadata lookup
    let mut var_def_ids: Vec<&str> = opts.records.iter()
        .filter_map(|r| r.get_raw("item_option_new"))
        .filter(|s| !s.is_empty())
        .collect();
    var_def_ids.sort_unstable();
    var_def_ids.dedup();

    // Step 3: fetch variable definitions to discover which ones are reference /
    // list-collector types.  Use Raw mode so the table name columns return the
    // internal name (e.g. "sys_user") rather than the display label.
    //
    // Reference-type variables store the target table in `reference`, while
    // List Collector variables use `list_table`.
    let ref_map: HashMap<String, String> = if !var_def_ids.is_empty() {
        let defs = TableApi::new(Arc::clone(&transport), None, "item_option_new")
            .in_list("sys_id", &var_def_ids)
            .fields(&["sys_id", "reference", "list_table"])
            .display_value(DisplayValue::Raw)
            .execute()
            .await?;

        defs.records.iter()
            .filter_map(|r| {
                let ref_table = r.get_str("reference")
                    .filter(|s| !s.is_empty())
                    .or_else(|| r.get_str("list_table").filter(|s| !s.is_empty()))?;
                Some((r.sys_id.clone(), ref_table.to_string()))
            })
            .collect()
    } else {
        HashMap::new()
    };

    // Build variables, attaching reference_table when the definition has one
    let mut variables: Vec<CatalogVariable> = opts.records.iter()
        .filter_map(|r| {
            let name = r.get_str("item_option_new")?.to_string();
            if name.is_empty() {
                return None;
            }
            let value = r.get_str("value").unwrap_or_default().to_string();
            let order: u32 = r.get_str("order").and_then(|o| o.parse().ok()).unwrap_or(999);
            let reference_table = r.get_raw("item_option_new")
                .and_then(|id| ref_map.get(id))
                .cloned();
            Some(CatalogVariable { name, value, order, reference_table })
        })
        .collect();
    variables.sort_by_key(|v| v.order);

    Ok(variables)
}
