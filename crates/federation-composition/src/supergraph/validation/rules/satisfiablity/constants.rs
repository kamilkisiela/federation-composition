//! Port of `src/supergraph/validation/rules/satisfiablity/constants.ts`.
//!
//! The TS symbols `SUPERGRAPH_ID` and `MERGEDGRAPH_ID` identify the two non-subgraph graphs.
//! They are variants of [`GraphId`].

/// The `id: string | Symbol` of a satisfiability `Graph`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GraphId<'s> {
    /// A subgraph's id (`Symbol.for(id)` in TS).
    Subgraph(&'s str),
    /// `SUPERGRAPH_ID = Symbol("__supergraph__")`.
    Supergraph,
    /// `MERGEDGRAPH_ID = Symbol("__mergedgraph__")`.
    MergedGraph,
}

impl<'s> GraphId<'s> {
    /// `this.id`: the subgraph id, or `Symbol(...).toString()` for the two symbols.
    pub fn as_str(self) -> &'s str {
        match self {
            GraphId::Subgraph(id) => id,
            GraphId::Supergraph => "Symbol(__supergraph__)",
            GraphId::MergedGraph => "Symbol(__mergedgraph__)",
        }
    }
}
