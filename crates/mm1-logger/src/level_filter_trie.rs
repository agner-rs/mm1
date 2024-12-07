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
}
