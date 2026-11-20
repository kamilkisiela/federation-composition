//! Port of `src/supergraph/validation/rules/satisfiablity/moves.ts`.
//!
//! `Move` is an interface with three classes; here it is an enum. `instanceof FieldMove` is
//! `matches!(m, Move::Field(_))`.

use std::fmt;
use std::rc::Rc;

use super::contexts::ContextMask;
use super::node::FieldName;
use super::selection::Selection;

/// `Move`.
#[derive(Clone, Debug, PartialEq)]
pub enum Move<'s> {
    Field(FieldMove<'s>),
    Abstract(AbstractMove<'s>),
    Entity(EntityMove<'s>),
}

impl<'s> Move<'s> {
    pub fn as_field(&self) -> Option<&FieldMove<'s>> {
        match self {
            Move::Field(m) => Some(m),
            _ => None,
        }
    }
}

impl fmt::Display for Move<'_> {
    /// `toString()`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Move::Field(m) => m.fmt(f),
            Move::Abstract(m) => m.fmt(f),
            Move::Entity(m) => m.fmt(f),
        }
    }
}

/// `FieldMoveOverride` (non-null). The names borrow from the supergraph state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldMoveOverride<'s> {
    pub label: Option<&'s str>,
    pub from_graph_id: Option<&'s str>,
    pub value: bool,
}

/// `FieldMove`. The names borrow from the supergraph state.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldMove<'s> {
    pub type_name: &'s str,
    pub field_name: FieldName<'s>,
    pub requires: Option<Rc<Selection<'s>>>,
    pub provides: Option<Rc<Selection<'s>>>,
    /// `_override` (the `override` getter/setter).
    pub r#override: Option<FieldMoveOverride<'s>>,
    pub provided: bool,
    /// Mask of contexts that have to be set by an ancestor for the field's `@fromContext`
    /// arguments to be resolvable.
    pub required_contexts: ContextMask,
}

impl<'s> FieldMove<'s> {
    pub fn new(
        type_name: &'s str,
        field_name: FieldName<'s>,
        requires: Option<Rc<Selection<'s>>>,
        provides: Option<Rc<Selection<'s>>>,
        r#override: Option<FieldMoveOverride<'s>>,
        provided: bool,
        required_contexts: ContextMask,
    ) -> Self {
        FieldMove {
            type_name,
            field_name,
            requires,
            provides,
            r#override,
            provided,
            required_contexts,
        }
    }
}

impl fmt::Display for FieldMove<'_> {
    /// `toString()` (lazy in TS). Used for logging and for the `debugPostFix` of duplicated nodes.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.field_name.as_str())?;

        if let Some(requires) = &self.requires {
            write!(f, " @require({requires})")?;
        }

        if let Some(provides) = &self.provides {
            write!(f, " @provides({provides})")?;
        }

        if self.provided {
            f.write_str(" @provided")?;
        }

        if let Some(r#override) = &self.r#override {
            // `${null}` is "null".
            write!(f, " @override(label: {}", r#override.label.unwrap_or("null"))?;
            write!(f, ", on: {})", r#override.value)?;
        }

        if !self.required_contexts.is_zero() {
            write!(f, " @fromContext(mask: {})", self.required_contexts)?;
        }

        Ok(())
    }
}

/// `AbstractMove`.
#[derive(Clone, Debug, PartialEq)]
pub struct AbstractMove<'s> {
    pub key_fields: Option<Rc<Selection<'s>>>,
}

impl<'s> AbstractMove<'s> {
    pub fn new(key_fields: Option<Rc<Selection<'s>>>) -> Self {
        AbstractMove { key_fields }
    }
}

impl fmt::Display for AbstractMove<'_> {
    /// `toString()`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.key_fields {
            Some(k) => write!(f, "🔮 🔑 {k}"),
            None => f.write_str("🔮"),
        }
    }
}

/// `EntityMove`.
#[derive(Clone, Debug, PartialEq)]
pub struct EntityMove<'s> {
    pub key_fields: Rc<Selection<'s>>,
}

impl<'s> EntityMove<'s> {
    pub fn new(key_fields: Rc<Selection<'s>>) -> Self {
        EntityMove { key_fields }
    }
}

impl fmt::Display for EntityMove<'_> {
    /// `toString()`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "🔑 {}", self.key_fields)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supergraph::validation::rules::satisfiablity::node::test_field_name;

    fn selection(source: &'static str) -> Rc<Selection<'static>> {
        Rc::new(Selection::new("User", source, Vec::new()))
    }

    #[test]
    fn field_move_to_string() {
        let plain = FieldMove::new(
            "User",
            test_field_name("name"),
            None,
            None,
            None,
            false,
            ContextMask::zero(),
        );
        assert_eq!(plain.to_string(), "name");

        let full = FieldMove::new(
            "User",
            test_field_name("name"),
            Some(selection("id\n  email")),
            Some(selection("a { b }")),
            Some(FieldMoveOverride {
                label: None,
                from_graph_id: None,
                value: false,
            }),
            true,
            ContextMask::bit(0).or(&ContextMask::bit(2)),
        );
        assert_eq!(
            full.to_string(),
            "name @require(id email) @provides(a { b }) @provided @override(label: null, on: false) @fromContext(mask: 5)"
        );

        let labelled = FieldMove::new(
            "User",
            test_field_name("name"),
            None,
            None,
            Some(FieldMoveOverride {
                label: Some("percent(5)"),
                from_graph_id: Some("a"),
                value: true,
            }),
            false,
            ContextMask::zero(),
        );
        assert_eq!(labelled.to_string(), "name @override(label: percent(5), on: true)");
    }

    #[test]
    fn abstract_and_entity_move_to_string() {
        assert_eq!(AbstractMove::new(None).to_string(), "🔮");
        assert_eq!(AbstractMove::new(Some(selection("id"))).to_string(), "🔮 🔑 id");
        assert_eq!(EntityMove::new(selection("id  name")).to_string(), "🔑 id name");
    }
}
