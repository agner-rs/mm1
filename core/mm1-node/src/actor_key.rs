use std::fmt;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ActorKey(Arc<Node>);

impl ActorKey {
    pub(crate) fn root() -> Self {
        Self(Arc::new(Node::Root))
    }

    pub(crate) fn child(&self, func: &'static str) -> Self {
        let func = func.trim_matches('(');
        let func = func.find('<').map(|at| &func[..at]).unwrap_or_else(|| func);
        let node = Node::Child {
            parent: self.0.clone(),
            func,
        };
        Self(Arc::new(node))
    }

    pub(crate) fn path(&self) -> impl Iterator<Item = &str> + '_ {
        let mut acc = vec![];
        let mut node = self.0.as_ref();
        loop {
            match node {
                Node::Root => break acc.into_iter().rev(),
                Node::Child { parent, func } => {
                    acc.push(func);
                    node = parent.as_ref();
                },
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Node {
    Root,
    Child {
        parent: Arc<Self>,
        func:   &'static str,
    },
}

impl fmt::Display for ActorKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.0.as_ref(), f)
    }
}

impl fmt::Display for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Root => write!(f, ""),
            Self::Child { parent, func } => {
                fmt::Display::fmt(parent.as_ref(), f)?;
                write!(f, "/")?;
                write!(f, "{func}")?;

                Ok(())
            },
        }
    }
}
