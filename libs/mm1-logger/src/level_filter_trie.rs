use std::collections::HashMap;

#[derive(Debug, Default)]
pub(crate) struct FilterTrie {
    level:    Option<tracing::Level>,
    children: HashMap<String, Self>,
}
impl FilterTrie {
    pub(crate) fn level_for_target<'a>(
        &self,
        path: impl IntoIterator<Item = &'a str>,
    ) -> Option<tracing::Level> {
        let mut path = path.into_iter();
        if let Some(next) = path.next() {
            self.children
                .get(next)
                .and_then(move |node| node.level_for_target(path))
                .or_else(|| {
                    self.children
                        .get("*")
                        .and_then(|node| node.level_for_target([]))
                })
        } else {
            self.level
        }
    }

    pub(crate) fn from_statements(statements: &[crate::config::LogTargetConfig]) -> Self {
        let mut root = Self::default();

        for statement in statements {
            let mut n = &mut root;
            for mod_name in statement.path.iter() {
                n = n.children.entry(mod_name.to_owned()).or_default();
            }
            n.level = Some(statement.level);
        }
        root
    }

    /// Whether an event at `event_level` on `target` passes the per-target
    /// filter. This is independent of the global minimum level.
    pub(crate) fn allows(&self, target: &str, event_level: tracing::Level) -> bool {
        self.level_for_target(target.split("::"))
            .map(|level| level >= event_level)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use tracing::Level;

    use super::FilterTrie;

    fn trie(statements: &[&str]) -> FilterTrie {
        let parsed = statements
            .iter()
            .map(|s| s.parse().expect("valid statement"))
            .collect::<Vec<_>>();
        FilterTrie::from_statements(&parsed)
    }

    // Regression test for #138: an empty filter must not reject everything; it
    // should defer to the global minimum level (i.e. allow, here).
    #[test]
    fn empty_filter_allows_all_targets() {
        let filter = trie(&[]);
        assert!(filter.allows("anything", Level::ERROR));
        assert!(filter.allows("mm1_node::runtime", Level::INFO));
    }

    // Regression test for #138: `a=debug` must also apply to `a::b`.
    #[test]
    fn statement_applies_to_child_targets() {
        let filter = trie(&["a=debug"]);
        assert!(filter.allows("a::b", Level::DEBUG));
        assert!(filter.allows("a::b::c", Level::INFO));
        // debug does not include the more-verbose trace level
        assert!(!filter.allows("a::b", Level::TRACE));
        // a more specific statement still wins
        let filter = trie(&["a=debug", "a::b=info"]);
        assert!(!filter.allows("a::b", Level::DEBUG));
        assert!(filter.allows("a::b", Level::INFO));
    }
}
