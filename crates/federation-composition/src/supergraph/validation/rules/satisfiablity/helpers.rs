//! Port of `src/supergraph/validation/rules/satisfiablity/helpers.ts`.

/// `occurrences(str, subString)`.
/// NOTE(port): lengths are UTF-16 in JS; key fields are ASCII.
pub fn occurrences(s: &str, sub_string: &str) -> usize {
    if sub_string.is_empty() {
        return s.len() + 1;
    }
    s.matches(sub_string).count()
}

/// JS truthiness of a `string | null | undefined` (`""` is falsy).
pub fn is_truthy_str<S: AsRef<str>>(value: &Option<S>) -> bool {
    value.as_ref().is_some_and(|s| !s.as_ref().is_empty())
}

/// `scoreKeyFields(keyFields)`.
pub fn score_key_fields(key_fields: &str) -> usize {
    let fields = occurrences(key_fields, " ") + 1;
    let inner_selection_sets = occurrences(key_fields, "{") * 3;
    fields + inner_selection_sets
}

/// `OverrideLabels`: label name to on/off, in the order the labels were set. The labels are the
/// supergraph state's strings. Most supergraphs have none, and an empty `Vec` does not allocate
/// when cloned.
/// NOTE(port): a plain object in TS; only lookups and `matches` read it, so order is unobservable.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OverrideLabels<'s> {
    state: Vec<(&'s str, bool)>,
}

impl<'s> OverrideLabels<'s> {
    pub fn set(mut self, key: &'s str, value: bool) -> Self {
        match self.state.iter_mut().find(|(k, _)| *k == key) {
            Some((_, v)) => *v = value,
            None => self.state.push((key, value)),
        }
        self
    }

    pub fn get(&self, key: &str) -> Option<bool> {
        self.state.iter().find(|(k, _)| *k == key).map(|&(_, v)| v)
    }

    /// Every label set here has the same value in `other` (`undefined !== true/false`).
    pub fn matches(&self, other: &OverrideLabels<'_>) -> bool {
        self.state.iter().all(|&(k, v)| other.get(k) == Some(v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn occurrences_counts_non_overlapping() {
        assert_eq!(occurrences("aaaa", "aa"), 2);
        assert_eq!(occurrences("a b c", " "), 2);
        assert_eq!(occurrences("abc", ""), 4);
        assert_eq!(occurrences("", "x"), 0);
    }

    #[test]
    fn score_key_fields_counts_fields_and_nested_sets() {
        assert_eq!(score_key_fields("id"), 1);
        assert_eq!(score_key_fields("id name"), 2);
        // 4 spaces -> 5 "fields", plus 3 for the nested selection set.
        assert_eq!(score_key_fields("id organization { id }"), 5 + 3);
    }

    #[test]
    fn truthy_strings() {
        assert!(!is_truthy_str::<&str>(&None));
        assert!(!is_truthy_str(&Some(String::new())));
        assert!(is_truthy_str(&Some("a")));
    }

    #[test]
    fn override_labels_match_only_set_keys() {
        let empty = OverrideLabels::default();
        let on = OverrideLabels::default().set("feature", true);
        let off = OverrideLabels::default().set("feature", false);
        assert!(empty.matches(&on));
        assert!(on.matches(&on.clone().set("other", false)));
        assert!(!on.matches(&off));
        assert!(!on.matches(&empty));
        assert_eq!(on.get("feature"), Some(true));
        assert_eq!(on.get("missing"), None);
    }
}
