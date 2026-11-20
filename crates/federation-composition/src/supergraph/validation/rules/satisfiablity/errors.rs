//! Port of `src/supergraph/validation/rules/satisfiablity/errors.ts`.

use std::fmt;

use smallvec::SmallVec;

use super::edge::EdgeId;
use super::graph::Graph;
use super::move_validator::is_external;
use super::moves::Move;
use super::node::{FieldName, NodeId, NodeTypeState};
use crate::utils::js::JsResult;

/// `SatisfiabilityErrorKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SatisfiabilityErrorKind {
    /// cannot move to subgraph "X" using @key(fields: "a b c") of "User", the key field(s)
    /// cannot be resolved from subgraph "Y".
    Key,
    /// cannot satisfy @require conditions on field "User.name".
    Require,
    /// field "User.name" is not resolvable because marked @external
    External,
    /// cannot find field "User.name".
    MissingField,
    /// cannot move to subgraph "X", which has field "User.name", because type "User" has no
    /// @key defined in subgraph "Y".
    NoKey,
    /// no subgraph can be reached to resolve the implementation type of @interfaceObject type "X".
    NoImplementation,
    /// could not find a match for required context for field "User.name".
    RequiredContext,
}

/// `SatisfiabilityError` (an `Error` subclass in TS; `toString()` is the message).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SatisfiabilityError {
    pub kind: SatisfiabilityErrorKind,
    pub source_graph_name: String,
    pub type_name: String,
    pub field_name: Option<String>,
    pub message: String,
}

impl SatisfiabilityError {
    pub fn for_key(source_graph_name: &str, target_graph_name: &str, type_name: &str, key_fields: &str) -> Self {
        Self::new(
            SatisfiabilityErrorKind::Key,
            source_graph_name,
            type_name,
            None,
            format!(
                "cannot move to subgraph \"{target_graph_name}\" using @key(fields: \"{key_fields}\") of \"{type_name}\", the key field(s) cannot be resolved from subgraph \"{source_graph_name}\"."
            ),
        )
    }

    pub fn for_require(source_graph_name: &str, type_name: &str, field_name: &str) -> Self {
        Self::new(
            SatisfiabilityErrorKind::Require,
            source_graph_name,
            type_name,
            Some(field_name),
            format!("cannot satisfy @require conditions on field \"{type_name}.{field_name}\"."),
        )
    }

    pub fn for_external(source_graph_name: &str, type_name: &str, field_name: &str) -> Self {
        Self::new(
            SatisfiabilityErrorKind::External,
            source_graph_name,
            type_name,
            Some(field_name),
            format!("field \"{type_name}.{field_name}\" is not resolvable because marked @external."),
        )
    }

    pub fn for_missing_field(source_graph_name: &str, type_name: &str, field_name: &str) -> Self {
        Self::new(
            SatisfiabilityErrorKind::MissingField,
            source_graph_name,
            type_name,
            Some(field_name),
            format!("cannot find field \"{type_name}.{field_name}\"."),
        )
    }

    pub fn for_no_key(source_graph_name: &str, target_graph_name: &str, type_name: &str, field_name: &str) -> Self {
        Self::new(
            SatisfiabilityErrorKind::NoKey,
            source_graph_name,
            type_name,
            Some(field_name),
            format!(
                "cannot move to subgraph \"{target_graph_name}\", which has field \"{type_name}.{field_name}\", because type \"{type_name}\" has no @key defined in subgraph \"{target_graph_name}\"."
            ),
        )
    }

    pub fn for_required_context(source_graph_name: &str, type_name: &str, field_name: &str) -> Self {
        Self::new(
            SatisfiabilityErrorKind::RequiredContext,
            source_graph_name,
            type_name,
            Some(field_name),
            format!("could not find a match for required context for field \"{type_name}.{field_name}\"."),
        )
    }

    pub fn for_no_implementation(source_graph_name: &str, type_name: &str) -> Self {
        Self::new(
            SatisfiabilityErrorKind::NoImplementation,
            source_graph_name,
            type_name,
            None,
            format!(
                "no subgraph can be reached to resolve the implementation type of @interfaceObject type \"{type_name}\"."
            ),
        )
    }

    fn new(
        kind: SatisfiabilityErrorKind,
        source_graph_name: &str,
        type_name: &str,
        field_name: Option<&str>,
        message: String,
    ) -> Self {
        SatisfiabilityError {
            kind,
            source_graph_name: source_graph_name.to_string(),
            type_name: type_name.to_string(),
            field_name: field_name.map(str::to_string),
            message,
        }
    }

    // TS: only commented-out code in `move-validator.ts` calls it.
    #[allow(dead_code)]
    pub fn is_matching_field(&self, type_name: &str, field_name: &str) -> bool {
        if self.type_name != type_name {
            return false;
        }
        match &self.field_name {
            Some(f) if !f.is_empty() => f == field_name,
            _ => true,
        }
    }
}

impl fmt::Display for SatisfiabilityError {
    /// `toString()`: the message.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// A satisfiability error that is only built if it is reported (`lazy(() => ...)` in TS).
///
/// NOTE(port): the TS keeps a memoized closure per error. Everything those closures read is in
/// the merged graph: an edge error's names are its head's and tail's graph and type names and
/// its move's names. So an error is the id of what it is about, built when the errors are
/// listed; most are never listed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LazyError<'s> {
    /// `forKey(head graph, tail graph, head type, key fields)` of an entity or abstract edge.
    Key(EdgeId),
    /// `forRequire(head graph, type, field)` of a field edge.
    Require(EdgeId),
    /// `forExternal(head graph, type, field)` of a field edge.
    External(EdgeId),
    /// `forMissingField(tail graph, type, field)` of a field edge whose override is off.
    OverriddenField(EdgeId),
    /// `forRequiredContext(head graph, type, field)` of a field edge.
    RequiredContext(EdgeId),
    /// `forNoImplementation(graph, type)` of an `@interfaceObject` node.
    NoImplementation(NodeId),
    /// The two lazy errors `findDirectPaths` adds when no edge has the field: the missing field
    /// from `tail`, then the subgraphs that have the field but no resolvable `@key`. The second
    /// throws if the type has no node.
    MissingFieldOrNoKey {
        tail: NodeId,
        type_name: &'s str,
        field_name: FieldName<'s>,
    },
}

impl<'s> LazyError<'s> {
    /// Builds the error(s), appending them to `out`. `graph` is the merged graph the error was
    /// found in.
    fn build(self, graph: &Graph<'s>, out: &mut Vec<SatisfiabilityError>) -> JsResult<()> {
        let field_error = |edge: EdgeId, of_tail: bool| {
            let e = graph.edge(edge);
            let m = e.r#move.as_field().expect("a field edge's error");
            let graph_name = graph.node(if of_tail { e.tail } else { e.head }).graph_name;
            (graph_name, m.type_name, m.field_name.as_str())
        };
        let error = match self {
            LazyError::Key(edge) => {
                let e = graph.edge(edge);
                let (head, tail) = (graph.node(e.head), graph.node(e.tail));
                let key_fields = match &e.r#move {
                    Move::Entity(m) => m.key_fields.to_string(),
                    Move::Abstract(m) => m.key_fields.as_ref().map(|k| k.to_string()).unwrap_or_default(),
                    Move::Field(_) => unreachable!("a key error is about an entity or abstract edge"),
                };
                SatisfiabilityError::for_key(head.graph_name, tail.graph_name, head.type_name, &key_fields)
            }
            LazyError::Require(edge) => {
                let (g, t, f) = field_error(edge, false);
                SatisfiabilityError::for_require(g, t, f)
            }
            LazyError::External(edge) => {
                let (g, t, f) = field_error(edge, false);
                SatisfiabilityError::for_external(g, t, f)
            }
            LazyError::OverriddenField(edge) => {
                let (g, t, f) = field_error(edge, true);
                SatisfiabilityError::for_missing_field(g, t, f)
            }
            LazyError::RequiredContext(edge) => {
                let (g, t, f) = field_error(edge, false);
                SatisfiabilityError::for_required_context(g, t, f)
            }
            LazyError::NoImplementation(node) => {
                let node = graph.node(node);
                SatisfiabilityError::for_no_implementation(node.graph_name, node.type_name)
            }
            LazyError::MissingFieldOrNoKey {
                tail,
                type_name,
                field_name,
            } => return build_missing_field_or_no_key(graph, tail, type_name, field_name, out),
        };
        out.push(error);
        Ok(())
    }
}

/// [`LazyError::MissingFieldOrNoKey`].
fn build_missing_field_or_no_key<'s>(
    graph: &Graph<'s>,
    tail: NodeId,
    type_name: &'s str,
    field_name: FieldName<'s>,
    out: &mut Vec<SatisfiabilityError>,
) -> JsResult<()> {
    let tail_graph_name = graph.node(tail).graph_name;
    // In case of no errors, we know that there were no edges matching the field name.
    out.push(SatisfiabilityError::for_missing_field(
        tail_graph_name,
        type_name,
        field_name.as_str(),
    ));
    // find graphs with the same type and field name, but no @key defined
    for type_node in graph.nodes_of(type_name, true)? {
        for edge in graph.field_edges_of_head(type_node, field_name) {
            let e = graph.edge(edge);
            let Move::Field(field_move) = &e.r#move else {
                continue;
            };
            if
            // edge.move.typeName === typeName &&
            field_move.field_name != field_name || is_external(graph, edge) {
                continue;
            }

            let head = graph.node(e.head);
            let resolvable_keys = match head.type_state {
                Some(NodeTypeState::Object(type_state)) => type_state
                    .by_graph
                    .get(head.graph_id)
                    .map_or(0, |g| g.keys.iter().filter(|key| key.resolvable).count()),
                _ => 0,
            };

            if resolvable_keys == 0 {
                out.push(SatisfiabilityError::for_no_key(
                    tail_graph_name,
                    graph.node(e.tail).graph_name,
                    type_name,
                    field_name.as_str(),
                ));
            }
        }
    }
    Ok(())
}

/// `LazyErrors<T>`: errors collected lazily and evaluated only when reported. Most results carry
/// at most one, so one is stored inline.
#[derive(Clone, Debug, Default)]
pub struct LazyErrors<'s> {
    lazy_error: SmallVec<[LazyError<'s>; 1]>,
}

impl<'s> LazyErrors<'s> {
    pub fn new() -> Self {
        Self::default()
    }

    /// `add(lazyError)`.
    pub fn add(&mut self, lazy_error: LazyError<'s>) -> &mut Self {
        self.lazy_error.push(lazy_error);
        self
    }

    /// `add(otherLazyErrors)`: appends the other collection's lazy errors.
    pub fn add_all(&mut self, other: LazyErrors<'s>) -> &mut Self {
        self.lazy_error.extend(other.lazy_error);
        self
    }

    /// `toArray()`: builds every error, in order. Throws if building one throws.
    pub fn to_array(&self, merged_graph: &Graph<'s>) -> JsResult<Vec<SatisfiabilityError>> {
        let mut out = Vec::new();
        for e in &self.lazy_error {
            e.build(merged_graph, &mut out)?;
        }
        Ok(out)
    }

    pub fn is_empty(&self) -> bool {
        self.lazy_error.is_empty()
    }
}
