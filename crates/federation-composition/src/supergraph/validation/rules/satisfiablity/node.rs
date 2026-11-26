//! Port of `src/supergraph/validation/rules/satisfiablity/node.ts`.
//!
//! Nodes live in their `Graph`'s arena; [`NodeId`] is the TS `node.index` (the position in
//! `nodesByTypeIndex`, reassigned when a subgraph graph is merged with `addSubgraph`).
//! Fields mutated while walking (after the graph is built) use interior mutability.

use std::cell::RefCell;
use std::fmt;

use smallvec::SmallVec;

use crate::utils::collections::IndexMap;

use super::contexts::ContextMask;
use super::helpers::OverrideLabels;
use crate::supergraph::composition::enum_type::EnumTypeState;
use crate::supergraph::composition::interface_type::InterfaceTypeState;
use crate::supergraph::composition::object_type::ObjectTypeState;
use crate::supergraph::composition::scalar_type::ScalarTypeState;
use crate::supergraph::composition::union_type::UnionTypeState;

/// Index of a node in its graph (`node.index`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(u32);

impl NodeId {
    pub fn new(index: usize) -> Self {
        NodeId(index as u32)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// An interned graph name. The TS compares graph names as strings (`visitedGraphs`,
/// `resolvedGraphs`, the resolvability records); comparing interned ids is equivalent because
/// [`GraphNames`] gives equal names the same id and different names different ids.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GraphNameId(u32);

impl GraphNameId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// A set of graph names (`visitedGraphs`, `resolvedGraphs`), as a bit set over the interned
/// ids. The first 128 subgraphs fit inline, so copying a set does not allocate.
/// NOTE(port): the TS keeps arrays; only `includes` and `push`-if-missing are used on them, and
/// their order only shows in log messages, which list the graphs in id order here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GraphNameSet {
    bits: SmallVec<[u64; 2]>,
}

impl GraphNameSet {
    /// The bits of the graphs `64 * i..64 * (i + 1)`.
    pub fn word(&self, i: usize) -> u64 {
        self.bits.get(i).copied().unwrap_or(0)
    }

    pub fn contains(&self, id: GraphNameId) -> bool {
        let i = id.0 as usize;
        self.bits.get(i / 64).is_some_and(|word| word & (1 << (i % 64)) != 0)
    }

    pub fn insert(&mut self, id: GraphNameId) {
        let i = id.0 as usize;
        if self.bits.len() <= i / 64 {
            self.bits.resize(i / 64 + 1, 0);
        }
        self.bits[i / 64] |= 1 << (i % 64);
    }

    /// `concatIfNotExistsString(set, id)`.
    pub fn with(&self, id: GraphNameId) -> Self {
        let mut set = self.clone();
        set.insert(id);
        set
    }

    pub fn iter(&self) -> impl Iterator<Item = GraphNameId> + '_ {
        self.bits.iter().enumerate().flat_map(|(i, &word)| {
            (0..64)
                .filter(move |bit| word & (1 << bit) != 0)
                .map(move |bit| GraphNameId((i * 64 + bit) as u32))
        })
    }
}

/// A field name with its interned id. Equal names have equal ids, so comparing and looking up
/// field names while walking never touches the string.
#[derive(Clone, Copy, Debug)]
pub struct FieldName<'s> {
    name: &'s str,
    id: u32,
}

impl<'s> FieldName<'s> {
    pub fn as_str(self) -> &'s str {
        self.name
    }
}

impl PartialEq for FieldName<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for FieldName<'_> {}

impl std::hash::Hash for FieldName<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl fmt::Display for FieldName<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name)
    }
}

/// For tests: `name` interned in one interner per thread.
#[cfg(test)]
pub fn test_field_name(name: &'static str) -> FieldName<'static> {
    thread_local!(static NAMES: &'static FieldNames<'static> = Box::leak(Box::default()));
    NAMES.with(|names| names.intern(name))
}

/// The field names of one `Supergraph`'s graphs and selections, interned when the graphs are
/// built.
#[derive(Debug, Default)]
pub struct FieldNames<'s> {
    ids: RefCell<IndexMap<&'s str, u32>>,
}

impl<'s> FieldNames<'s> {
    pub fn intern(&self, name: &'s str) -> FieldName<'s> {
        let mut ids = self.ids.borrow_mut();
        let next = ids.len() as u32;
        let id = *ids.entry(name).or_insert(next);
        FieldName { name, id }
    }
}

/// The graph name interner shared by the graphs of one `Supergraph`. The names live in the
/// supergraph state, so the table holds borrowed names and an id is an index into it.
#[derive(Debug, Default)]
pub struct GraphNames<'s> {
    names: RefCell<Vec<&'s str>>,
}

impl<'s> GraphNames<'s> {
    pub fn intern(&self, name: &'s str) -> GraphNameId {
        let mut names = self.names.borrow_mut();
        let index = match names.iter().position(|n| *n == name) {
            Some(index) => index,
            None => {
                names.push(name);
                names.len() - 1
            }
        };
        GraphNameId(index as u32)
    }

    pub fn len(&self) -> usize {
        self.names.borrow().len()
    }

    pub fn name(&self, id: GraphNameId) -> &'s str {
        self.names.borrow()[id.0 as usize]
    }

    /// `graphNames.join(separator)` (logging only).
    pub fn join(&self, ids: &GraphNameSet, separator: &str) -> String {
        ids.iter().map(|id| self.name(id)).collect::<Vec<_>>().join(separator)
    }
}

/// The key `nextStep` builds for each path (`${graphName}#provides${contextsKey}` or
/// `${graphName}${contextsKey}`), as the parts it is made of.
/// NOTE(port): the TS compares the strings; comparing the parts is the same unless a subgraph
/// name itself ends with `#provides` or `#contexts:N`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphCombo {
    pub graph: GraphNameId,
    pub has_provides: bool,
    pub contexts: ContextMask,
}

/// `typeState` of a node (non-null): a reference into the supergraph state.
#[derive(Clone, Copy, Debug)]
// TS: the type state each node holds; nothing reads it beyond its kind.
#[allow(dead_code)]
pub enum NodeTypeState<'s> {
    Object(&'s ObjectTypeState<'s>),
    Interface(&'s InterfaceTypeState),
    Enum(&'s EnumTypeState),
    Scalar(&'s ScalarTypeState),
    Union(&'s UnionTypeState),
}

impl NodeTypeState<'_> {
    /// `typeState.kind`.
    pub fn kind(&self) -> &'static str {
        match self {
            NodeTypeState::Object(_) => "object",
            NodeTypeState::Interface(_) => "interface",
            NodeTypeState::Enum(_) => "enum",
            NodeTypeState::Scalar(_) => "scalar",
            NodeTypeState::Union(_) => "union",
        }
    }
}

/// `Node`. Its names borrow from the supergraph state.
#[derive(Debug)]
pub struct Node<'s> {
    pub index: NodeId,
    pub type_name: &'s str,
    /// `null` for specified scalars.
    pub type_state: Option<NodeTypeState<'s>>,
    pub graph_id: &'s str,
    pub graph_name: &'s str,
    /// `graph_name` interned, for the comparisons made while walking.
    pub graph_name_id: GraphNameId,
    /// Contexts (`@context(name:)`) set by this type, including the ones inherited from
    /// implemented interfaces and from unions this type is a member of.
    pub contexts: ContextMask,
    pub debug_post_fix: String,
    pub is_leaf: bool,
    /// `childrenIndex` of `abstract__X` and `cross-graph__X`: positions in the head's edge list.
    /// NOTE(port): the TS keys them by type name, but always the node's own one, so each is a
    /// plain list. `entity__X` is never read.
    pub abstract_edge_indexes: EdgeIndexes,
    pub cross_graph_edge_indexes: EdgeIndexes,
    /// `field__X` of `childrenIndex`, sorted by field name id.
    field_edge_indexes: Vec<(u32, EdgeIndexes)>,
    /// When a Node is visited, the names of the graphs that lead to it with @provides.
    visited_graph_combos: RefCell<Vec<Vec<GraphCombo>>>,
}

/// Positions in a head's edge list, in insertion order and without duplicates.
pub type EdgeIndexes = SmallVec<[u32; 2]>;

/// Appends `edge_at` unless it is listed already.
pub fn push_edge_index(indexes: &mut EdgeIndexes, edge_at: usize) {
    let edge_at = edge_at as u32;
    if !indexes.contains(&edge_at) {
        indexes.push(edge_at);
    }
}

impl<'s> Node<'s> {
    /// `new Node(index, typeName, typeState, graphId, graphName, contexts)`.
    pub fn new(
        index: NodeId,
        type_name: &'s str,
        type_state: Option<NodeTypeState<'s>>,
        graph_id: &'s str,
        graph_name: &'s str,
        graph_name_id: GraphNameId,
        contexts: ContextMask,
    ) -> Self {
        // NOTE(port): the TS throws when `typeState` is `undefined`; `Option` cannot express that
        // state (callers pass a state or `null`).
        let is_leaf = matches!(
            type_state,
            None | Some(NodeTypeState::Scalar(_)) | Some(NodeTypeState::Enum(_))
        );

        Node {
            index,
            type_name,
            type_state,
            graph_id,
            graph_name,
            graph_name_id,
            contexts,
            debug_post_fix: String::new(),
            is_leaf,
            abstract_edge_indexes: EdgeIndexes::new(),
            cross_graph_edge_indexes: EdgeIndexes::new(),
            field_edge_indexes: Vec::new(),
            visited_graph_combos: RefCell::new(Vec::new()),
        }
    }

    /// `withoutState()`: clears the children index and visited combos (used by `addSubgraph`).
    pub fn without_state(mut self) -> Self {
        self.abstract_edge_indexes.clear();
        self.cross_graph_edge_indexes.clear();
        self.field_edge_indexes.clear();
        self.visited_graph_combos = RefCell::new(Vec::new());
        self
    }

    /// The positions of the field's edges in this node's edge list.
    pub fn field_edge_indexes(&self, field: FieldName<'_>) -> &[u32] {
        match self.field_edge_indexes.binary_search_by_key(&field.id, |(id, _)| *id) {
            Ok(i) => &self.field_edge_indexes[i].1,
            Err(_) => &[],
        }
    }

    pub fn add_field_edge(&mut self, field: FieldName<'_>, edge_at: usize) {
        let i = match self.field_edge_indexes.binary_search_by_key(&field.id, |(id, _)| *id) {
            Ok(i) => i,
            Err(i) => {
                self.field_edge_indexes.insert(i, (field.id, EdgeIndexes::new()));
                i
            }
        };
        push_edge_index(&mut self.field_edge_indexes[i].1, edge_at);
    }

    /// Whether a combination of graph names and @provides that lead to the Node was already
    /// checked. (`labelValues` is unused, as in the TS.)
    pub fn is_graph_combo_visited(
        &self,
        graph_name_provides_combos: &[GraphCombo],
        _label_values: &OverrideLabels<'s>,
    ) -> bool {
        self.visited_graph_combos
            .borrow()
            .iter()
            .any(|visited_graphs| visited_graphs.iter().all(|g| graph_name_provides_combos.contains(g)))
    }

    pub fn set_graph_combo_as_visited(&self, graph_names: Vec<GraphCombo>) {
        self.visited_graph_combos.borrow_mut().push(graph_names);
    }
}

impl fmt::Display for Node<'_> {
    /// `toString()`: `${typeName}/${graphName}${debugPostFix}` (lazy in TS; logging only).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}{}", self.type_name, self.graph_name, self.debug_post_fix)
    }
}
