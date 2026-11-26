//! Port of `src/supergraph/validation/rules/satisfiablity/edge.ts`.
//!
//! Edges live in their `Graph`'s arena and are referred to by [`EdgeId`]; identity checks
//! (`edge === other`, `previousEdges.includes(edge)`) compare ids. `head`/`tail` are node ids
//! of the same graph. The resolvability records are added while walking, so they are in a
//! `RefCell`. They are not a cache: a record made for some visited graphs answers later checks
//! that visited more, so dropping them would change which moves the walk accepts.
//!
//! NOTE(port): in TS several edges can share one `Move` object (`new Edge(head, edge.move, tail)`
//! in `duplicateNode`, `connectUnionOrInterface` and the `@provides` rewiring), and
//! `updateOverride` mutates the shared `FieldMove`. Here edges share a field move through `Rc`
//! too, but `updateOverride` copies it on write, so each edge's update is its own. This is not
//! observable: the edges sharing a move always have the same head type, head graph, field
//! name and `provided` flag, so `addOverriddenFields` reaches and updates all of them with the same
//! value in the same iteration (no edge is ignored before that loop ends).

use std::cell::RefCell;
use std::rc::Rc;

use smallvec::SmallVec;

use super::errors::LazyError;
use super::graph::Graph;
use super::helpers::OverrideLabels;
use super::moves::{FieldMoveOverride, Move};
use super::node::{GraphNameId, GraphNameSet, NodeId};
use crate::utils::js::{JsError, JsResult};

/// Index of an edge in its graph's edge arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EdgeId(u32);

impl EdgeId {
    pub fn new(index: usize) -> Self {
        EdgeId(index as u32)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// `EdgeResolvabilityResult`.
#[derive(Clone, Debug)]
pub enum EdgeResolvabilityResult<'s> {
    Success,
    Failure { error: LazyError<'s> },
}

impl EdgeResolvabilityResult<'_> {
    pub fn success(&self) -> bool {
        matches!(self, EdgeResolvabilityResult::Success)
    }
}

/// A check of an edge: the graphs and labels it was made for, and its outcome.
/// A check is made for a few graphs, so they are a list in id order rather than a bit set over
/// every graph of the supergraph.
type ResolvabilityRecord<'s> = (
    SmallVec<[GraphNameId; 4]>,
    OverrideLabels<'s>,
    EdgeResolvabilityResult<'s>,
);

/// `Edge<T = Move>`.
#[derive(Debug)]
pub struct Edge<'s> {
    pub head: NodeId,
    pub r#move: Move<'s>,
    pub tail: NodeId,
    /// `resolvable`: the outcome of each check, with the graphs and labels it was made for.
    /// Boxed: most edges are never checked, and a pointer is a third of an empty `Vec`.
    #[allow(clippy::box_collection)]
    resolvable: RefCell<Option<Box<Vec<ResolvabilityRecord<'s>>>>>,
    ignored: bool,
}

/// `isEntityEdge(edge)`.
pub fn is_entity_edge(edge: &Edge<'_>) -> bool {
    matches!(edge.r#move, Move::Entity(_))
}

/// `assertEntityEdge(edge)`. Takes the graph to print the edge (`${edge}`) in the error.
// TS: defined here and never called in `src/` either.
#[allow(dead_code)]
pub fn assert_entity_edge(graph: &Graph<'_>, edge: EdgeId) -> JsResult<()> {
    if !is_entity_edge(graph.edge(edge)) {
        return Err(JsError::error(format!(
            "Expected edge to be Edge<EntityMove>, but got {}",
            graph.edge_to_string(edge)
        )));
    }
    Ok(())
}

/// `isAbstractEdge(edge)`.
pub fn is_abstract_edge(edge: &Edge<'_>) -> bool {
    matches!(edge.r#move, Move::Abstract(_))
}

/// `assertAbstractEdge(edge)`. Takes the graph to print the edge in the error.
pub fn assert_abstract_edge(graph: &Graph<'_>, edge: EdgeId) -> JsResult<()> {
    if !is_abstract_edge(graph.edge(edge)) {
        return Err(JsError::error(format!(
            "Expected edge to be Edge<AbstractMove>, but got {}",
            graph.edge_to_string(edge)
        )));
    }
    Ok(())
}

/// `isFieldEdge(edge)`.
pub fn is_field_edge(edge: &Edge<'_>) -> bool {
    matches!(edge.r#move, Move::Field(_))
}

/// `assertFieldEdge(edge)`. Takes the graph to print the edge in the error.
pub fn assert_field_edge(graph: &Graph<'_>, edge: EdgeId) -> JsResult<()> {
    if !is_field_edge(graph.edge(edge)) {
        return Err(JsError::error(format!(
            "Expected edge to be Edge<FieldMove>, but got {}",
            graph.edge_to_string(edge)
        )));
    }
    Ok(())
}

impl<'s> Edge<'s> {
    pub fn new(head: NodeId, r#move: Move<'s>, tail: NodeId) -> Self {
        Edge {
            head,
            r#move,
            tail,
            resolvable: RefCell::new(None),
            ignored: false,
        }
    }

    pub fn set_ignored(&mut self, ignored: bool) {
        self.ignored = ignored;
    }

    pub fn is_ignored(&self) -> bool {
        self.ignored
    }

    /// `updateOverride(override)`. Throws when the move is not a field move.
    pub fn update_override(&mut self, r#override: FieldMoveOverride<'s>) -> JsResult<()> {
        match &mut self.r#move {
            // Copy-on-write: the other edges sharing the move are updated on their own.
            Move::Field(m) => {
                Rc::make_mut(m).r#override = Some(r#override);
                Ok(())
            }
            other => Err(JsError::error(format!(
                "Expected move to be FieldMove, but got {}",
                other
            ))),
        }
    }

    /// `getResolvability(graphNames, labelValues)`. The caller's `graphNames` is
    /// `concatIfNotExistsString(visitedGraphs, graphName)`, passed as its two parts.
    pub fn get_resolvability(
        &self,
        visited_graphs: &GraphNameSet,
        graph_name: GraphNameId,
        label_values: &OverrideLabels<'s>,
    ) -> Option<EdgeResolvabilityResult<'s>> {
        self.resolvable
            .borrow()
            .iter()
            .flat_map(|records| records.iter())
            .find(|(checked_graph_names, checked_label_values, _)| {
                checked_graph_names
                    .iter()
                    .all(|&g| g == graph_name || visited_graphs.contains(g))
                    && checked_label_values.matches(label_values)
            })
            .map(|(_, _, result)| result.clone())
    }

    /// `setResolvable(success, graphNames, labelValues, error?)`: `error` is `Some` exactly when
    /// `success` is false.
    pub fn set_resolvable(
        &self,
        success: bool,
        graph_names: GraphNameSet,
        label_values: OverrideLabels<'s>,
        error: Option<LazyError<'s>>,
    ) -> EdgeResolvabilityResult<'s> {
        let result = if success {
            EdgeResolvabilityResult::Success
        } else {
            EdgeResolvabilityResult::Failure {
                error: error.expect("setResolvable(false, ...) is always called with an error"),
            }
        };
        let mut resolvable = self.resolvable.borrow_mut();
        let records = resolvable.get_or_insert_default();
        // Most edges keep a few records: grow by half instead of starting at four.
        if records.len() == records.capacity() {
            records.reserve_exact(records.len() / 2 + 1);
        }
        records.push((graph_names.iter().collect(), label_values, result.clone()));
        result
    }
}
