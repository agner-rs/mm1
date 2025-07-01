use std::time::Duration;

#[derive(Debug)]
pub struct ChildSpec<F, T = ChildType> {
    pub launcher:     F,
    pub child_type:   T,
    pub init_type:    InitType,
    pub stop_timeout: Duration,
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

impl<F, T> ChildSpec<F, T> {
    pub fn map_launcher<F1, M>(self, map: M) -> ChildSpec<F1, T>
    where
        M: FnOnce(F) -> F1,
    {
        let ChildSpec {
            launcher: factory,
            child_type,
            init_type,
            stop_timeout,
        } = self;
        ChildSpec {
            launcher: map(factory),
            child_type,
            init_type,
            stop_timeout,
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
            launcher:     self.launcher.clone(),
            child_type:   self.child_type.clone(),
            init_type:    self.init_type,
            stop_timeout: self.stop_timeout,
        }
    }
}
