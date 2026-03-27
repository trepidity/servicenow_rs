use std::collections::HashMap;

/// Registry mapping record number prefixes to ServiceNow table names.
///
/// ServiceNow record numbers follow the pattern `PREFIX0000123` where
/// the prefix identifies the table. This registry resolves prefixes
/// to table names, enabling lookup-by-number workflows.
///
/// Comes pre-loaded with standard ServiceNow prefix mappings and
/// supports registering additional prefixes for custom tables.
///
/// # Examples
///
/// ```
/// use servicenow_rs::prefix::PrefixRegistry;
///
/// let registry = PrefixRegistry::default();
/// assert_eq!(registry.table_for_prefix("INC"), Some("incident"));
/// assert_eq!(registry.table_for_number("CHG0012345"), Some("change_request"));
/// ```
#[derive(Debug, Clone)]
pub struct PrefixRegistry {
    /// Prefix -> table name mapping. Prefixes are stored uppercase.
    mappings: HashMap<String, String>,
}

impl PrefixRegistry {
    /// Create an empty registry with no mappings.
    pub fn empty() -> Self {
        Self {
            mappings: HashMap::new(),
        }
    }

    /// Register a prefix -> table mapping.
    ///
    /// Prefixes are normalized to uppercase. Overwrites existing mappings
    /// for the same prefix.
    pub fn register(&mut self, prefix: &str, table: &str) {
        self.mappings
            .insert(prefix.to_uppercase(), table.to_string());
    }

    /// Resolve a prefix to a table name.
    ///
    /// The prefix is matched case-insensitively.
    pub fn table_for_prefix(&self, prefix: &str) -> Option<&str> {
        self.mappings.get(&prefix.to_uppercase()).map(|s| s.as_str())
    }

    /// Extract the prefix from a record number and resolve the table name.
    ///
    /// Splits the number at the boundary between letters and digits.
    /// For example, `"INC0012345"` -> prefix `"INC"` -> `"incident"`.
    pub fn table_for_number(&self, number: &str) -> Option<&str> {
        let prefix = extract_prefix(number)?;
        self.table_for_prefix(&prefix)
    }

    /// Extract just the prefix portion from a record number.
    ///
    /// Returns `None` if the number has no alphabetic prefix.
    pub fn extract_prefix(number: &str) -> Option<String> {
        extract_prefix(number)
    }

    /// Get all registered prefix -> table mappings.
    pub fn mappings(&self) -> &HashMap<String, String> {
        &self.mappings
    }

    /// Number of registered prefixes.
    pub fn len(&self) -> usize {
        self.mappings.len()
    }

    /// Whether the registry has no mappings.
    pub fn is_empty(&self) -> bool {
        self.mappings.is_empty()
    }
}

impl Default for PrefixRegistry {
    /// Create a registry with standard ServiceNow prefix mappings.
    fn default() -> Self {
        let mut registry = Self::empty();
        // Standard ITSM tables.
        registry.register("INC", "incident");
        registry.register("CHG", "change_request");
        registry.register("CTASK", "change_task");
        registry.register("PRB", "problem");
        registry.register("RITM", "sc_req_item");
        registry.register("REQ", "sc_request");
        registry.register("SCTASK", "sc_task");
        registry.register("TASK", "sc_task");
        // Knowledge.
        registry.register("KB", "kb_knowledge");
        // Project / Agile.
        registry.register("STRY", "rm_story");
        registry.register("STSK", "rm_scrum_task");
        registry.register("PRJ", "pm_project");
        registry.register("DMND", "dmn_demand");
        registry
    }
}

/// Extract the alphabetic prefix from a record number.
///
/// Splits at the boundary between letters and digits.
/// `"INC0012345"` -> `Some("INC")`
/// `"CTASK0457943"` -> `Some("CTASK")`
/// `"0012345"` -> `None` (no letters)
fn extract_prefix(number: &str) -> Option<String> {
    let number = number.trim();
    if number.is_empty() {
        return None;
    }

    let prefix: String = number.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    if prefix.is_empty() {
        return None;
    }

    Some(prefix.to_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_mappings() {
        let reg = PrefixRegistry::default();
        assert_eq!(reg.table_for_prefix("INC"), Some("incident"));
        assert_eq!(reg.table_for_prefix("CHG"), Some("change_request"));
        assert_eq!(reg.table_for_prefix("CTASK"), Some("change_task"));
        assert_eq!(reg.table_for_prefix("PRB"), Some("problem"));
        assert_eq!(reg.table_for_prefix("RITM"), Some("sc_req_item"));
        assert_eq!(reg.table_for_prefix("REQ"), Some("sc_request"));
        assert_eq!(reg.table_for_prefix("SCTASK"), Some("sc_task"));
        assert_eq!(reg.table_for_prefix("KB"), Some("kb_knowledge"));
        assert_eq!(reg.table_for_prefix("STRY"), Some("rm_story"));
        assert_eq!(reg.table_for_prefix("PRJ"), Some("pm_project"));
        assert_eq!(reg.table_for_prefix("DMND"), Some("dmn_demand"));
        assert_eq!(reg.len(), 13);
    }

    #[test]
    fn test_case_insensitive() {
        let reg = PrefixRegistry::default();
        assert_eq!(reg.table_for_prefix("inc"), Some("incident"));
        assert_eq!(reg.table_for_prefix("Inc"), Some("incident"));
        assert_eq!(reg.table_for_prefix("INC"), Some("incident"));
    }

    #[test]
    fn test_table_for_number() {
        let reg = PrefixRegistry::default();
        assert_eq!(reg.table_for_number("INC0012345"), Some("incident"));
        assert_eq!(reg.table_for_number("CHG0307336"), Some("change_request"));
        assert_eq!(reg.table_for_number("CTASK0457943"), Some("change_task"));
        assert_eq!(reg.table_for_number("RITM2513403"), Some("sc_req_item"));
        assert_eq!(reg.table_for_number("REQ2540612"), Some("sc_request"));
        assert_eq!(reg.table_for_number("KB0010001"), Some("kb_knowledge"));
    }

    #[test]
    fn test_unknown_prefix() {
        let reg = PrefixRegistry::default();
        assert_eq!(reg.table_for_prefix("XYZ"), None);
        assert_eq!(reg.table_for_number("XYZ001"), None);
    }

    #[test]
    fn test_no_prefix() {
        let reg = PrefixRegistry::default();
        assert_eq!(reg.table_for_number("0012345"), None);
        assert_eq!(reg.table_for_number(""), None);
    }

    #[test]
    fn test_custom_registration() {
        let mut reg = PrefixRegistry::default();
        reg.register("MYPREFIX", "u_custom_table");
        assert_eq!(reg.table_for_prefix("MYPREFIX"), Some("u_custom_table"));
        assert_eq!(
            reg.table_for_number("MYPREFIX0001"),
            Some("u_custom_table")
        );
        // Original mappings still work.
        assert_eq!(reg.table_for_prefix("INC"), Some("incident"));
    }

    #[test]
    fn test_extract_prefix() {
        assert_eq!(extract_prefix("INC0012345"), Some("INC".to_string()));
        assert_eq!(extract_prefix("CTASK0457943"), Some("CTASK".to_string()));
        assert_eq!(extract_prefix("inc001"), Some("INC".to_string()));
        assert_eq!(extract_prefix("0012345"), None);
        assert_eq!(extract_prefix(""), None);
    }

    #[test]
    fn test_override_mapping() {
        let mut reg = PrefixRegistry::default();
        // Override INC to point to a custom table.
        reg.register("INC", "u_custom_incident");
        assert_eq!(reg.table_for_prefix("INC"), Some("u_custom_incident"));
    }
}
