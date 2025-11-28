use std::time::Duration;

#[derive(Debug)]
pub struct ChildSpec<F, T = ChildType> {
    pub launcher:        F,
    pub child_type:      T,
    pub init_type:       InitType,
    pub stop_timeout:    Duration,
    pub announce_parent: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum ChildType {
    Permanent,
    Temporary,
}

#[derive(Debug, Clone, Copy)]
pub enum InitType {
    NoAck,
    WithAck { start_timeout: Duration },
}

impl<F> ChildSpec<F, ()> {
    pub fn new(launcher: F) -> Self {
        Self {
            launcher,
            child_type: (),
            init_type: InitType::NoAck,
            stop_timeout: Duration::from_secs(1),
            announce_parent: false,
        }
    }
}

impl<F, T> ChildSpec<F, T> {
    pub fn with_child_type<T1>(self, child_type: T1) -> ChildSpec<F, T1> {
        let Self {
            launcher,
            child_type: _,
            init_type,
            stop_timeout,
            announce_parent,
        } = self;
        ChildSpec {
            launcher,
            child_type,
            init_type,
            stop_timeout,
            announce_parent,
        }
    }

    pub fn with_init_type(self, init_type: InitType) -> Self {
        Self { init_type, ..self }
    }

    pub fn with_stop_timeout(self, stop_timeout: Duration) -> Self {
        Self {
            stop_timeout,
            ..self
        }
    }

    pub fn with_announce_parent(self, announce_parent: bool) -> Self {
        Self {
            announce_parent,
            ..self
        }
    }

    pub fn announce_parent(self) -> Self {
        self.with_announce_parent(true)
    }

    pub fn map_launcher<F1, M>(self, map: M) -> ChildSpec<F1, T>
    where
        M: FnOnce(F) -> F1,
    {
        let Self {
            launcher: factory,
            child_type,
            init_type,
            stop_timeout,
            announce_parent,
        } = self;
        ChildSpec {
            launcher: map(factory),
            child_type,
            init_type,
            stop_timeout,
            announce_parent,
        }
    }
}

impl<F, T> Clone for ChildSpec<F, T>
where
    F: Clone,
    T: Clone,
{
    fn clone(&self) -> Self {
        Self {
            launcher:        self.launcher.clone(),
            child_type:      self.child_type.clone(),
            init_type:       self.init_type,
            stop_timeout:    self.stop_timeout,
            announce_parent: self.announce_parent,
        }
    }
}
