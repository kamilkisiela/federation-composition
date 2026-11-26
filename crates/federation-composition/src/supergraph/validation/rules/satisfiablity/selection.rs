//! Port of `src/supergraph/validation/rules/satisfiablity/selection.ts`.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::fmt;
use std::rc::Rc;

use graphql_core::{Field as FieldNode, InlineFragment, Selection as SelectionAstNode, SelectionSet};

use super::node::{FieldName, FieldNames};
use crate::subgraph::helpers::parse_fields;
use crate::supergraph::state::SupergraphState;
use crate::utils::collections::IndexSet;
use crate::utils::js::{locale_compare, JsError, JsResult};
use crate::utils::state::strip_type_modifiers;

/// JS `\s` (`WhiteSpace` and `LineTerminator` code points).
fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{0009}'
            | '\u{000A}'
            | '\u{000B}'
            | '\u{000C}'
            | '\u{000D}'
            | '\u{0020}'
            | '\u{00A0}'
            | '\u{1680}'
            | '\u{2000}'
            ..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}'
    )
}

/// `str.replace(/\s+/g, " ")`.
fn collapse_whitespace(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut in_whitespace = false;
    for c in source.chars() {
        if is_js_whitespace(c) {
            if !in_whitespace {
                out.push(' ');
                in_whitespace = true;
            }
        } else {
            out.push(c);
            in_whitespace = false;
        }
    }
    out
}

/// `Field` (`kind: "field"`). The names are the supergraph state's.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Field<'s> {
    pub type_name: &'s str,
    pub field_name: FieldName<'s>,
    pub selection_set: Option<Vec<SelectionNode<'s>>>,
}

/// `Fragment` (`kind: "fragment"`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Fragment<'s> {
    pub type_name: &'s str,
    pub selection_set: Vec<SelectionNode<'s>>,
}

/// `SelectionNode = Field | Fragment`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SelectionNode<'s> {
    Field(Field<'s>),
    Fragment(Fragment<'s>),
}

impl<'s> SelectionNode<'s> {
    pub fn type_name(&self) -> &'s str {
        match self {
            SelectionNode::Field(f) => f.type_name,
            SelectionNode::Fragment(f) => f.type_name,
        }
    }

    /// `selectionSet` (`null` for a leaf field).
    pub fn selection_set(&self) -> Option<&[SelectionNode<'s>]> {
        match self {
            SelectionNode::Field(f) => f.selection_set.as_deref(),
            SelectionNode::Fragment(f) => Some(&f.selection_set),
        }
    }
}

/// Which selections are equal (`Selection.equals`): the same type and the same resolved fields.
/// Each distinct pair gets an id, so comparing two selections compares their ids.
#[derive(Debug, Default)]
pub struct SelectionIds<'s> {
    ids: RefCell<IndexSet<(&'s str, Vec<SelectionNode<'s>>)>>,
}

impl<'s> SelectionIds<'s> {
    pub fn selection(
        &self,
        type_name: &'s str,
        source: &'s str,
        selection_set: Vec<SelectionNode<'s>>,
    ) -> Selection<'s> {
        let mut ids = self.ids.borrow_mut();
        let key = (type_name, selection_set);
        let id = match ids.get_index_of(&key) {
            Some(id) => id,
            None => ids.insert_full(key.clone()).0,
        };
        Selection {
            type_name,
            source,
            selection_set: key.1,
            id: id as u32,
        }
    }
}

/// For tests: a selection with an id from one [`SelectionIds`] per thread.
#[cfg(test)]
pub fn test_selection(
    type_name: &'static str,
    source: &'static str,
    selection_set: Vec<SelectionNode<'static>>,
) -> Selection<'static> {
    thread_local!(static IDS: &'static SelectionIds<'static> = Box::leak(Box::default()));
    IDS.with(|ids| ids.selection(type_name, source, selection_set))
}

/// `Selection`: resolved key/requires/provides fields, shared (`Rc<Selection>`) by the edges
/// created for one field or key. Created by [`SelectionIds::selection`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection<'s> {
    type_name: &'s str,
    source: &'s str,
    pub selection_set: Vec<SelectionNode<'s>>,
    /// Equal for equal selections, see [`SelectionIds`].
    id: u32,
}

impl<'s> Selection<'s> {
    // TS: defined here and never called in `src/` either.
    #[allow(dead_code)]
    pub fn contains(&self, type_name: &str, field_name: &str) -> bool {
        self.contains_in(type_name, field_name, &self.selection_set)
    }

    /// `equals(other)`: the same type and the same fields. The TS compares the type names,
    /// then the sources, then the sorted selection sets; equal sources of one type resolve to
    /// equal selection sets, so equal ids are the same answer.
    pub fn equals(&self, other: &Selection<'_>) -> bool {
        self.id == other.id
    }

    /// Equal for equal selections (see [`Selection::equals`]).
    pub fn id(&self) -> u32 {
        self.id
    }

    /// `_contains(typeName, fieldName, selectionSet)`.
    // TS: only `contains` calls it.
    #[allow(dead_code)]
    fn contains_in(&self, type_name: &str, field_name: &str, selection_set: &[SelectionNode<'_>]) -> bool {
        selection_set.iter().any(|s| {
            (matches!(s, SelectionNode::Field(f) if f.type_name == type_name && f.field_name.as_str() == field_name))
                || match s.selection_set() {
                    Some(inner) => self.contains_in(type_name, field_name, inner),
                    None => false,
                }
        })
    }
}

impl fmt::Display for Selection<'_> {
    /// `toString()`: the source with whitespace runs collapsed (JS `\s+`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&collapse_whitespace(self.source))
    }
}

/// `SelectionResolver`: parses and resolves field sets.
/// NOTE(port): the TS caches the result per `typeName/keyFields`. Nothing relies on resolved
/// selections being shared (`equals` compares them), and resolving is cheap next to the walk,
/// so each call resolves.
#[derive(Debug)]
pub struct SelectionResolver<'s> {
    supergraph_state: &'s SupergraphState<'s>,
    field_names: Rc<FieldNames<'s>>,
    selection_ids: SelectionIds<'s>,
}

impl<'s> SelectionResolver<'s> {
    pub fn new(supergraph_state: &'s SupergraphState, field_names: Rc<FieldNames<'s>>) -> Self {
        SelectionResolver {
            supergraph_state,
            field_names,
            selection_ids: SelectionIds::default(),
        }
    }

    /// `resolve(typeName, keyFields)`. Throws for an unknown type or unparsable fields.
    pub fn resolve(&self, type_name: &'s str, key_fields: &'s str) -> JsResult<Rc<Selection<'s>>> {
        let type_state_exists = self.supergraph_state.object_types.contains_key(type_name)
            || self.supergraph_state.interface_types.contains_key(type_name)
            || self.supergraph_state.union_types.contains_key(type_name);

        if !type_state_exists {
            return Err(JsError::error(format!(
                "Expected an object/interface/union type when resolving keyFields of {type_name}"
            )));
        }

        let Some(selection_set_node) = parse_fields(key_fields)? else {
            return Err(JsError::error(format!(
                "Expected a selection set when resolving keyFields of {type_name}"
            )));
        };

        Ok(Rc::new(self.selection_ids.selection(
            type_name,
            key_fields,
            self.resolve_selection_set_node(type_name, &selection_set_node)?,
        )))
    }

    /// `objectTypes.get(typeName) ?? interfaceTypes.get(typeName)`, as `Some(fields)` where
    /// `fields` gives the output type of a field.
    /// The state's names are returned with the type: `(type name, field name, field type)`.
    fn object_or_interface_field_type(
        &self,
        type_name: &str,
        field_name: &str,
    ) -> Option<Option<(&'s str, &'s str, &'s str)>> {
        let state = self.supergraph_state;
        if let Some(object_state) = state.object_types.get(type_name) {
            return Some(
                object_state
                    .fields
                    .get_key_value(field_name)
                    .map(|(name, f)| (object_state.name.as_str(), *name, f.r#type)),
            );
        }
        if let Some(interface_state) = state.interface_types.get(type_name) {
            return Some(
                interface_state
                    .fields
                    .get_key_value(field_name)
                    .map(|(name, f)| (interface_state.name.as_str(), name.as_str(), f.r#type.as_str())),
            );
        }
        None
    }

    fn resolve_field_node(
        &self,
        type_name: &str,
        field_node: &FieldNode,
        selection_set: &mut Vec<SelectionNode<'s>>,
    ) -> JsResult<()> {
        if field_node.name.value == "__typename" {
            return Ok(());
        }

        let Some(field_type) = self.object_or_interface_field_type(type_name, &field_node.name.value) else {
            return Err(JsError::error(format!("Type \"{type_name}\" is not defined.")));
        };

        let Some((type_name, field_name, field_type)) = field_type else {
            return Err(JsError::error(format!(
                "Type \"{type_name}\" does not have field \"{}\".",
                field_node.name.value
            )));
        };

        if let Some(inner_selection_set) = &field_node.selection_set {
            let output_type = strip_type_modifiers(field_type);

            let resolved = self.resolve_selection_set_node(&output_type, inner_selection_set)?;
            selection_set.push(SelectionNode::Field(Field {
                type_name,
                field_name: self.field_names.intern(field_name),
                selection_set: Some(resolved),
            }));
        } else {
            // it's a leaf
            selection_set.push(SelectionNode::Field(Field {
                type_name,
                field_name: self.field_names.intern(field_name),
                selection_set: None,
            }));
        }

        Ok(())
    }

    /// The TS takes no `typeName`: the fragment's type condition is used.
    fn resolve_inline_fragment_node(
        &self,
        _type_name: &str,
        fragment_node: &InlineFragment,
        selection_set: &mut Vec<SelectionNode<'s>>,
    ) -> JsResult<()> {
        let type_name = match &fragment_node.type_condition {
            Some(type_condition) if !type_condition.name.value.is_empty() => type_condition.name.value.as_str(),
            _ => {
                return Err(JsError::error(
                    "Inline fragment without type condition is not supported.",
                ));
            }
        };

        let state = self.supergraph_state;
        let state_type_name = state
            .object_types
            .get(type_name)
            .map(|t| t.name.as_str())
            .or_else(|| state.interface_types.get(type_name).map(|t| t.name.as_str()));
        let Some(type_name) = state_type_name else {
            return Err(JsError::error(format!("Type \"{type_name}\" is not defined.")));
        };

        let resolved = self.resolve_selection_set_node(type_name, &fragment_node.selection_set)?;
        selection_set.push(SelectionNode::Fragment(Fragment {
            type_name,
            selection_set: resolved,
        }));

        Ok(())
    }

    fn resolve_selection_set_node(
        &self,
        type_name: &str,
        selection_set_node: &SelectionSet,
    ) -> JsResult<Vec<SelectionNode<'s>>> {
        let mut selection_set = Vec::new();

        for selection in &selection_set_node.selections {
            match selection {
                SelectionAstNode::Field(field) => self.resolve_field_node(type_name, field, &mut selection_set)?,
                SelectionAstNode::InlineFragment(fragment) => {
                    self.resolve_inline_fragment_node(type_name, fragment, &mut selection_set)?
                }
                SelectionAstNode::FragmentSpread(_) => {
                    return Err(JsError::error("Fragment spread is not supported."));
                }
            }
        }

        Ok(self.sort(selection_set))
    }

    /// `sort(selectionSet)`: the comparator of the TS (by kind, type name, field name).
    fn sort(&self, mut selection_set: Vec<SelectionNode<'s>>) -> Vec<SelectionNode<'s>> {
        selection_set.sort_by(compare_selection_nodes);
        selection_set
    }
}

/// The comparator of `SelectionResolver#sort`.
fn compare_selection_nodes(a: &SelectionNode<'_>, b: &SelectionNode<'_>) -> Ordering {
    match (a, b) {
        // sort fields by typeName.fieldName
        (SelectionNode::Field(a), SelectionNode::Field(b)) => locale_compare(
            &format!("{}.{}", a.type_name, a.field_name),
            &format!("{}.{}", b.type_name, b.field_name),
        ),
        // sort fragments by typeName
        (SelectionNode::Fragment(a), SelectionNode::Fragment(b)) => locale_compare(a.type_name, b.type_name),
        // field -> fragment
        (SelectionNode::Field(_), SelectionNode::Fragment(_)) => Ordering::Less,
        (SelectionNode::Fragment(_), SelectionNode::Field(_)) => Ordering::Greater,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supergraph::validation::rules::satisfiablity::node::test_field_name;

    fn leaf(type_name: &'static str, field_name: &'static str) -> SelectionNode<'static> {
        SelectionNode::Field(Field {
            type_name,
            field_name: test_field_name(field_name),
            selection_set: None,
        })
    }

    fn fragment(type_name: &'static str, selection_set: Vec<SelectionNode<'static>>) -> SelectionNode<'static> {
        SelectionNode::Fragment(Fragment {
            type_name,
            selection_set,
        })
    }

    #[test]
    fn to_string_collapses_js_whitespace() {
        let s = test_selection("User", "  id\n\t name \u{a0}\u{feff}x\u{85}y ", vec![]);
        // U+0085 is not JS whitespace; U+00A0 and U+FEFF are.
        assert_eq!(s.to_string(), " id name x\u{85}y ");
    }

    #[test]
    fn sort_puts_fields_first_and_uses_locale_compare() {
        let resolver_sorted = {
            let mut v = vec![
                fragment("B", vec![]),
                leaf("User", "name"),
                fragment("a", vec![]),
                leaf("User", "Id"),
                leaf("User", "id"),
                leaf("User", "_x"),
            ];
            v.sort_by(compare_selection_nodes);
            v
        };
        let names: Vec<String> = resolver_sorted
            .iter()
            .map(|s| match s {
                SelectionNode::Field(f) => f.field_name.to_string(),
                SelectionNode::Fragment(f) => format!("... on {}", f.type_name),
            })
            .collect();
        // localeCompare: "_" before letters, lowercase before uppercase.
        assert_eq!(names, vec!["_x", "id", "Id", "name", "... on a", "... on B"]);
    }

    #[test]
    fn equals_and_contains() {
        let a = test_selection(
            "User",
            "id profile { name }",
            vec![
                leaf("User", "id"),
                SelectionNode::Field(Field {
                    type_name: "User",
                    field_name: test_field_name("profile"),
                    selection_set: Some(vec![leaf("Profile", "name")]),
                }),
            ],
        );
        let b = test_selection("User", "id  profile { name }", a.selection_set.clone());
        assert!(a.equals(&b));
        let c = test_selection("User", "id profile", vec![leaf("User", "id"), leaf("User", "profile")]);
        assert!(!a.equals(&c));
        let d = test_selection("Other", "id profile { name }", a.selection_set.clone());
        assert!(!a.equals(&d));
        assert!(a.contains("Profile", "name"));
        assert!(a.contains("User", "id"));
        assert!(!a.contains("User", "name"));
    }
}
