use super::record::Record;
use crate::error::Error;

/// The result of a query execution, potentially containing multiple records
/// and partial failure information.
#[derive(Debug)]
pub struct QueryResult {
    /// Successfully retrieved records.
    pub records: Vec<Record>,
    /// Total count of matching records (from X-Total-Count header, if available).
    pub total_count: Option<u64>,
    /// Errors from partial failures (e.g., a related-record fetch failed
    /// but the main query succeeded).
    pub errors: Vec<Error>,
}

impl QueryResult {
    /// Create a new QueryResult with records only.
    pub fn new(records: Vec<Record>) -> Self {
        Self {
            records,
            total_count: None,
            errors: Vec::new(),
        }
    }

    /// Create an empty result.
    pub fn empty() -> Self {
        Self {
            records: Vec::new(),
            total_count: Some(0),
            errors: Vec::new(),
        }
    }

    /// Whether the result has any partial errors.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Whether the result is completely successful (no partial errors).
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }

    /// Number of records returned.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether zero records were returned.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Get the first record, if any.
    pub fn first(&self) -> Option<&Record> {
        self.records.first()
    }

    /// Iterate over records.
    pub fn iter(&self) -> impl Iterator<Item = &Record> {
        self.records.iter()
    }
}

impl IntoIterator for QueryResult {
    type Item = Record;
    type IntoIter = std::vec::IntoIter<Record>;

    fn into_iter(self) -> Self::IntoIter {
        self.records.into_iter()
    }
}

impl<'a> IntoIterator for &'a QueryResult {
    type Item = &'a Record;
    type IntoIter = std::slice::Iter<'a, Record>;

    fn into_iter(self) -> Self::IntoIter {
        self.records.iter()
    }
}
