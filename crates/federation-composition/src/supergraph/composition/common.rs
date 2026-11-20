//! Port of `src/supergraph/composition/common.ts`.

use crate::utils::collections::IndexMap;
use graphql_core::{ConstDirective, Definition, Directive as DirectiveNode};
use std::rc::Rc;

use crate::supergraph::state::{SubgraphStates, SupergraphState};
use crate::utils::js::JsResult;

/// `MapByGraph<T> = Map<string, T>` keyed by graph id, in insertion order.
///
/// NOTE(port): a type, field or argument is defined in a handful of subgraphs, most often one,
/// so this is a list searched linearly with the first entry stored inline, instead of a hash
/// map that allocates a table and spare entries for every type, field and argument. (Not a
/// `SmallVec`: that would make the supergraph state invariant in the lifetime of the subgraph
/// states it borrows from.)
#[derive(Clone, Debug, PartialEq)]
pub struct MapByGraph<T> {
    first: Option<(Rc<str>, T)>,
    rest: Vec<(Rc<str>, T)>,
}

impl<T> Default for MapByGraph<T> {
    fn default() -> Self {
        MapByGraph {
            first: None,
            rest: Vec::new(),
        }
    }
}

impl<T> MapByGraph<T> {
    fn entries(&self) -> impl ExactSizeIterator<Item = &(Rc<str>, T)> + DoubleEndedIterator + Clone {
        // `Option::iter` is exact-size; chaining two exact-size iterators is not, so the length
        // is restored with `ExactLen`.
        ExactLen {
            inner: self.first.iter().chain(self.rest.iter()),
            len: self.len(),
        }
    }

    pub fn len(&self) -> usize {
        usize::from(self.first.is_some()) + self.rest.len()
    }

    pub fn is_empty(&self) -> bool {
        self.first.is_none()
    }

    pub fn get_index_of(&self, graph_id: &str) -> Option<usize> {
        self.entries().position(|(id, _)| **id == *graph_id)
    }

    pub fn get_index(&self, i: usize) -> Option<(&str, &T)> {
        match i {
            0 => self.first.as_ref(),
            _ => self.rest.get(i - 1),
        }
        .map(|(id, v)| (&**id, v))
    }

    fn entry_mut(&mut self, i: usize) -> &mut (Rc<str>, T) {
        match i {
            0 => self.first.as_mut().expect("index in range"),
            _ => &mut self.rest[i - 1],
        }
    }

    pub fn get(&self, graph_id: &str) -> Option<&T> {
        self.entries().find(|(id, _)| **id == *graph_id).map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, graph_id: &str) -> Option<&mut T> {
        let i = self.get_index_of(graph_id)?;
        Some(&mut self.entry_mut(i).1)
    }

    pub fn contains_key(&self, graph_id: &str) -> bool {
        self.get(graph_id).is_some()
    }

    /// `map.set(graphId, value)`: replaces the value in place, or appends it.
    pub fn insert(&mut self, graph_id: Rc<str>, value: T) -> Option<T> {
        if let Some(i) = self.get_index_of(&graph_id) {
            return Some(std::mem::replace(&mut self.entry_mut(i).1, value));
        }
        if self.first.is_none() {
            self.first = Some((graph_id, value));
        } else {
            self.rest.push((graph_id, value));
        }
        None
    }

    pub fn first(&self) -> Option<(&str, &T)> {
        self.get_index(0)
    }

    pub fn keys(&self) -> impl ExactSizeIterator<Item = &str> + DoubleEndedIterator + Clone {
        self.entries().map(|(id, _)| &**id)
    }

    pub fn values(&self) -> impl ExactSizeIterator<Item = &T> + DoubleEndedIterator + Clone {
        self.entries().map(|(_, v)| v)
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, &T)> + DoubleEndedIterator + Clone {
        self.entries().map(|(id, v)| (&**id, v))
    }
}

/// An iterator of known length (see [`MapByGraph::entries`]).
#[derive(Clone)]
struct ExactLen<I> {
    inner: I,
    len: usize,
}

impl<I: Iterator> Iterator for ExactLen<I> {
    type Item = I::Item;

    fn next(&mut self) -> Option<I::Item> {
        let item = self.inner.next()?;
        self.len -= 1;
        Some(item)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len, Some(self.len))
    }
}

impl<I: DoubleEndedIterator> DoubleEndedIterator for ExactLen<I> {
    fn next_back(&mut self) -> Option<I::Item> {
        let item = self.inner.next_back()?;
        self.len -= 1;
        Some(item)
    }
}

impl<I: Iterator> ExactSizeIterator for ExactLen<I> {}

impl<T> std::ops::Index<usize> for MapByGraph<T> {
    type Output = T;

    fn index(&self, i: usize) -> &T {
        self.get_index(i).expect("index in range").1
    }
}

impl<T> std::ops::IndexMut<usize> for MapByGraph<T> {
    fn index_mut(&mut self, i: usize) -> &mut T {
        &mut self.entry_mut(i).1
    }
}

impl<K: Into<Rc<str>>, T> FromIterator<(K, T)> for MapByGraph<T> {
    /// Like `new Map(entries)`: a repeated graph id keeps its first position and last value.
    fn from_iter<I: IntoIterator<Item = (K, T)>>(entries: I) -> Self {
        let mut map = MapByGraph::default();
        for (graph_id, value) in entries {
            map.insert(graph_id.into(), value);
        }
        map
    }
}

/// `(graphId, value)` of an entry.
fn entry_ref<T>(entry: &(Rc<str>, T)) -> (&str, &T) {
    (&entry.0, &entry.1)
}

impl<'a, T> IntoIterator for &'a MapByGraph<T> {
    type Item = (&'a str, &'a T);
    type IntoIter = std::iter::Map<
        std::iter::Chain<std::option::Iter<'a, (Rc<str>, T)>, std::slice::Iter<'a, (Rc<str>, T)>>,
        fn(&'a (Rc<str>, T)) -> (&'a str, &'a T),
    >;

    fn into_iter(self) -> Self::IntoIter {
        self.first.iter().chain(self.rest.iter()).map(entry_ref)
    }
}

/// `Key`: same shape as the subgraph state's key.
pub use crate::subgraph::state::Key;

/// `Graph`: `{ name, id, version, url? }`, the subgraph state's `graph`.
pub type Graph = crate::subgraph::state::SubgraphGraph;

/// The `helpers` argument of `composeSupergraphState`/`composeSupergraphNode`:
/// `{ graphNameToId(graphName), supergraphState }`.
#[derive(Clone, Copy)]
pub struct ComposeHelpers<'s> {
    pub supergraph_state: &'s SupergraphState<'s>,
    /// `graphNameToIdMap` of the supergraph state builder.
    pub graph_name_to_id_map: &'s IndexMap<String, String>,
}

impl ComposeHelpers<'_> {
    /// `graphNameToId(graphName)`: `graphNameToIdMap[graphName] ?? null`.
    /// NOTE(port): a plain object in TS; graph names are user input and could be integer-like,
    /// but it is only looked up, never iterated.
    pub fn graph_name_to_id(&self, graph_name: &str) -> Option<String> {
        self.graph_name_to_id_map.get(graph_name).cloned()
    }
}

/// `TypeBuilder<T, S>`: a builder merging subgraph types `T` into supergraph type states `S`.
/// `composeSupergraphState?` is an inherent method of the two builders that have it
/// (objects and interfaces), with the parts of the supergraph state it reads passed
/// explicitly, because the type being composed lives inside the supergraph state.
/// `'a` is the lifetime of the subgraph states, which the supergraph state may borrow from.
pub trait TypeBuilder<'a, T, S> {
    fn visit_subgraph_state(
        &mut self,
        graph: &'a Graph,
        state: &mut IndexMap<String, S>,
        type_name: &'a str,
        ty: &'a T,
    );

    /// Returns a `TypeDefinitionNode | DirectiveDefinitionNode`. Fallible because the TS
    /// bodies call `ensureValue` and can throw.
    fn compose_supergraph_node(
        &mut self,
        ty: &S,
        graphs: &SubgraphStates,
        helpers: ComposeHelpers<'_>,
    ) -> JsResult<Definition>;
}

/// The `getOrCreate*` helpers' `map.get(key) ?? map.set(key, create()).get(key)`.
/// PERF: looks the key up before allocating it; `entry(key.to_string())` allocated a `String`
/// on every visit, and the key is already present for every subgraph after the first. A new
/// entry is appended, as with `entry`.
pub fn get_or_insert_with<'m, V>(map: &'m mut IndexMap<String, V>, key: &str, create: impl FnOnce() -> V) -> &'m mut V {
    let index = match map.get_index_of(key) {
        Some(index) => index,
        None => map.insert_full(key.to_string(), create()).0,
    };
    &mut map[index]
}

/// [`get_or_insert_with`] for a map keyed by names borrowed from the subgraph states.
pub fn get_or_insert_borrowed<'m, 'a, V>(
    map: &'m mut IndexMap<&'a str, V>,
    key: &'a str,
    create: impl FnOnce() -> V,
) -> &'m mut V {
    let index = match map.get_index_of(key) {
        Some(index) => index,
        None => map.insert_full(key, create()).0,
    };
    &mut map[index]
}

/// `convertToConst(nodes)`: a type cast in TS; directives are already const here.
pub fn convert_to_const(nodes: &[DirectiveNode]) -> Vec<ConstDirective> {
    nodes.to_vec()
}
