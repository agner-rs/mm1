use std::fmt;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActorKey(Arc<Node>);

#[derive(Default, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Args(Box<[Box<str>]>);

impl ActorKey {
    pub fn root() -> Self {
        Self(Arc::new(Node::Root))
    }

    pub fn child(&self, func: &'static str, args: Args) -> Self {
        let func = func.trim_matches('(');
        let func = func.find('<').map(|at| &func[..at]).unwrap_or_else(|| func);
        let node = Node::Child {
            parent: self.0.clone(),
            func,
            args,
        };
        Self(Arc::new(node))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Node {
    Root,
    Child {
        parent: Arc<Self>,
        func:   &'static str,
        args:   Args,
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
            Self::Root => write!(f, "/"),
            Self::Child { parent, func, args } => {
                fmt::Display::fmt(parent.as_ref(), f)?;
                write!(f, "{}", func)?;
                if !args.0.is_empty() {
                    for arg in &args.0[..] {
                        write!(f, "{},", &arg)?;
                    }
                }
                write!(f, "/")?;

                Ok(())
            },
        }
    }
}
