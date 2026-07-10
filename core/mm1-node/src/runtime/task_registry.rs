use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use mm1_address::address::Address;
use tokio::runtime::Handle;
use tokio::sync::{Notify, oneshot};
use tokio::task::AbortHandle;
use tokio::time::{Instant, timeout_at};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub(crate) struct ContainerTaskRegistry {
    inner: Arc<Inner>,
}

pub(crate) struct SpawnPermit {
    registry: ContainerTaskRegistry,
    pending:  bool,
}

#[derive(Debug)]
pub(crate) struct SpawnRejected;

struct Inner {
    closing:  AtomicBool,
    shutdown: CancellationToken,
    changed:  Notify,
    state:    Mutex<State>,
}

#[derive(Default)]
struct State {
    next_task_id:   u64,
    pending_spawns: usize,
    tasks:          HashMap<u64, TrackedTask>,
}

struct CompletionGuard {
    task_id: u64,
    inner:   Weak<Inner>,
}

struct TrackedTask {
    address:      Address,
    abort_handle: AbortHandle,
}

#[derive(Clone, Copy)]
pub(crate) struct ContainerTaskCounts {
    pub(crate) pending_spawns: usize,
    pub(crate) active_tasks:   usize,
}

impl ContainerTaskRegistry {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                closing:  AtomicBool::new(false),
                shutdown: CancellationToken::new(),
                changed:  Notify::new(),
                state:    Mutex::new(State::default()),
            }),
        }
    }

    pub(crate) fn reserve_spawn(&self) -> Option<SpawnPermit> {
        let mut state = self.inner.state.lock().expect("task registry poisoned");
        if self.inner.closing.load(Ordering::Acquire) {
            return None
        }
        state.pending_spawns += 1;
        Some(SpawnPermit {
            registry: self.clone(),
            pending:  true,
        })
    }

    pub(crate) fn shutdown_token(&self) -> CancellationToken {
        self.inner.shutdown.clone()
    }

    pub(crate) fn close(&self) {
        let _state = self.inner.state.lock().expect("task registry poisoned");
        self.inner.closing.store(true, Ordering::Release);
        self.inner.shutdown.cancel();
        self.inner.changed.notify_one();
    }

    pub(crate) fn addresses(&self) -> Vec<Address> {
        self.inner
            .state
            .lock()
            .expect("task registry poisoned")
            .tasks
            .values()
            .map(|task| task.address)
            .collect()
    }

    pub(crate) fn counts(&self) -> ContainerTaskCounts {
        let state = self.inner.state.lock().expect("task registry poisoned");
        ContainerTaskCounts {
            pending_spawns: state.pending_spawns,
            active_tasks:   state.tasks.len(),
        }
    }

    pub(crate) fn abort_all(&self) {
        let abort_handles = self
            .inner
            .state
            .lock()
            .expect("task registry poisoned")
            .tasks
            .values()
            .map(|task| task.abort_handle.clone())
            .collect::<Vec<_>>();
        for abort_handle in abort_handles {
            abort_handle.abort();
        }
    }

    pub(crate) async fn wait_for_empty(&self, within: Duration) -> bool {
        let deadline = Instant::now() + within;
        loop {
            let changed = self.inner.changed.notified();
            if self.is_empty() {
                return true
            }
            if timeout_at(deadline, changed).await.is_err() {
                return self.is_empty()
            }
        }
    }

    fn is_empty(&self) -> bool {
        let state = self.inner.state.lock().expect("task registry poisoned");
        state.pending_spawns == 0 && state.tasks.is_empty()
    }
}

impl SpawnPermit {
    pub(crate) fn spawn_on<F>(
        mut self,
        address: Address,
        executor: &Handle,
        task: F,
    ) -> Result<(), SpawnRejected>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let mut state = self
            .registry
            .inner
            .state
            .lock()
            .expect("task registry poisoned");
        if self.registry.inner.closing.load(Ordering::Acquire) {
            state.pending_spawns -= 1;
            self.pending = false;
            drop(state);
            self.registry.inner.changed.notify_one();
            return Err(SpawnRejected)
        }

        let task_id = state.next_task_id;
        state.next_task_id = state.next_task_id.wrapping_add(1);
        let (start_tx, start_rx) = oneshot::channel();
        let inner = Arc::downgrade(&self.registry.inner);
        let completion_guard = CompletionGuard { task_id, inner };
        let join_handle = executor.spawn(async move {
            let _completion_guard = completion_guard;
            if start_rx.await.is_ok() {
                task.await;
            }
        });

        let previous = state.tasks.insert(
            task_id,
            TrackedTask {
                address,
                abort_handle: join_handle.abort_handle(),
            },
        );
        assert!(previous.is_none(), "duplicate tracked task: {task_id}");
        state.pending_spawns -= 1;
        self.pending = false;
        let started = start_tx.send(()).is_ok();
        drop(state);
        self.registry.inner.changed.notify_one();
        started.then_some(()).ok_or(SpawnRejected)
    }
}

impl Drop for SpawnPermit {
    fn drop(&mut self) {
        if !self.pending {
            return
        }
        let mut state = self
            .registry
            .inner
            .state
            .lock()
            .expect("task registry poisoned");
        state.pending_spawns -= 1;
        self.registry.inner.changed.notify_one();
    }
}

impl Drop for CompletionGuard {
    fn drop(&mut self) {
        let Some(inner) = self.inner.upgrade() else {
            return
        };
        let removed = inner
            .state
            .lock()
            .expect("task registry poisoned")
            .tasks
            .remove(&self.task_id);
        if removed.is_some() {
            inner.changed.notify_one();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    #[test]
    fn abort_before_first_poll_removes_the_tracked_task() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build runtime");
        let registry = ContainerTaskRegistry::new();
        let permit = registry.reserve_spawn().expect("reserve spawn");

        permit
            .spawn_on(
                Address::from_u64(1),
                runtime.handle(),
                std::future::pending(),
            )
            .expect("spawn tracked task");
        registry.close();
        registry.abort_all();

        assert!(runtime.block_on(registry.wait_for_empty(Duration::from_secs(1))));
    }

    #[test]
    fn permit_reserved_before_close_cannot_spawn_after_close() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build runtime");
        let registry = ContainerTaskRegistry::new();
        let permit = registry.reserve_spawn().expect("reserve spawn");
        let ran = Arc::new(AtomicBool::new(false));

        registry.close();
        registry.abort_all();
        let ran_in_task = ran.clone();
        assert!(
            permit
                .spawn_on(Address::from_u64(1), runtime.handle(), async move {
                    ran_in_task.store(true, Ordering::Release);
                })
                .is_err()
        );

        assert!(runtime.block_on(registry.wait_for_empty(Duration::from_secs(1))));
        assert!(!ran.load(Ordering::Acquire));
        let counts = registry.counts();
        assert_eq!(counts.pending_spawns, 0);
        assert_eq!(counts.active_tasks, 0);
    }
}
