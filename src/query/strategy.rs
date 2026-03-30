/// Strategy for fetching related records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FetchStrategy {
    /// Library picks the best strategy based on context.
    #[default]
    Auto,
    /// Preserve explicit `.dot_walk()` field requests while falling back to
    /// concurrent related-record fetches for `.include_related()`.
    ///
    /// ServiceNow dot-walking only works through actual reference fields, so
    /// schema relationship names are not expanded into `sysparm_fields`.
    DotWalk,
    /// Fire parallel HTTP requests for each relationship and assemble client-side.
    /// Returns full related records. Works with any ServiceNow instance.
    Concurrent,
}
