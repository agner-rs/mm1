use std::time::Duration;

#[derive(Debug)]
pub struct ChildSpec<F, D = Duration> {
    pub factory:    F,
    pub child_type: ChildType,
    pub init_type:  InitType,
    pub timeouts:   ChildTimeouts<D>,
}

#[derive(Debug, Clone, Copy)]
pub enum ChildType {
    Permanent,
    Temporary,
}

#[derive(Debug, Clone, Copy)]
pub enum InitType {
    NoAck,
    WithAck,
}

#[derive(Debug, Clone, Copy)]
pub struct ChildTimeouts<D> {
    pub start_timeout: D,
    pub stop_timeout:  D,
}

impl Default for ChildTimeouts<Duration> {
    fn default() -> Self {
        Self {
            start_timeout: Duration::from_secs(1),
            stop_timeout:  Duration::from_secs(1),
        }
    }
}
