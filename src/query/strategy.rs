/// Strategy for fetching related records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FetchStrategy {
    /// Library picks the best strategy based on context.
    #[default]
    Auto,
    /// Use ServiceNow dot-walking to inline related fields.
    /// Efficient (single HTTP call) but only retrieves specific fields, not full records.
    DotWalk,
    /// Fire parallel HTTP requests for each relationship and assemble client-side.
    /// Returns full related records. Works with any ServiceNow instance.
    Concurrent,
}
