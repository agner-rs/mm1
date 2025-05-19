use std::collections::HashMap;
use std::time::Duration;

use tokio::runtime::{Builder, Runtime};

#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) struct RtConfigs {
    default: Option<RtConfig>,

    #[cfg_attr(feature = "serde", serde(flatten))]
    named: HashMap<String, RtConfig>,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct RtConfig {
    #[cfg_attr(feature = "serde", serde(default = "defaults::worker_threads"))]
    worker_threads:             usize,
    #[cfg_attr(feature = "serde", serde(default = "defaults::blocking_threads_max"))]
    blocking_threads_max:       usize,
    #[cfg_attr(
        feature = "serde",
        serde(default = "defaults::blocking_thread_keep_alive")
    )]
    blocking_thread_keep_alive: Duration,
    #[cfg_attr(feature = "serde", serde(default = "defaults::thread_stack_size"))]
    thread_stack_size:          usize,
    #[cfg_attr(feature = "serde", serde(default = "defaults::enable_io"))]
    enable_io:                  bool,
    #[cfg_attr(feature = "serde", serde(default = "defaults::enable_time"))]
    enable_time:                bool,
}

impl RtConfigs {
    pub(crate) fn build_runtimes(&self) -> std::io::Result<(Runtime, HashMap<String, Runtime>)> {
        let default = if let Some(c) = self.default.as_ref() {
            c.build_runtime("default")?
        } else {
            Builder::new_multi_thread()
                .thread_name("default")
                .enable_all()
                .build()?
        };

        let mut named = HashMap::new();

        for (n, c) in &self.named {
            named.insert(n.to_owned(), c.build_runtime(n)?);
        }

        Ok((default, named))
    }

    pub(crate) fn runtime_keys(&self) -> impl Iterator<Item = &str> {
        self.named.keys().map(String::as_str)
    }
}

impl RtConfig {
    fn build_runtime(&self, thread_name: &str) -> std::io::Result<Runtime> {
        let mut b = Builder::new_multi_thread();
        b.thread_name(thread_name)
            .worker_threads(self.worker_threads)
            .max_blocking_threads(self.blocking_threads_max)
            .thread_keep_alive(self.blocking_thread_keep_alive)
            .thread_stack_size(self.thread_stack_size);

        if self.enable_io {
            b.enable_io();
        }
        if self.enable_time {
            b.enable_time();
        }

        let runtime = b.build()?;
        Ok(runtime)
    }
}

mod defaults {
    use std::time::Duration;

    use super::RtConfig;

    pub(super) fn worker_threads() -> usize {
        num_cpus::get()
    }
    pub(super) fn blocking_threads_max() -> usize {
        // taken from tokio's docs
        512
    }
    pub(super) fn blocking_thread_keep_alive() -> Duration {
        Duration::from_secs(10)
    }
    pub(super) fn thread_stack_size() -> usize {
        2 * 1024 * 1024
    }
    pub(super) fn enable_io() -> bool {
        true
    }
    pub(super) fn enable_time() -> bool {
        true
    }

    impl Default for RtConfig {
        fn default() -> Self {
            Self {
                worker_threads:             worker_threads(),
                blocking_threads_max:       blocking_threads_max(),
                blocking_thread_keep_alive: blocking_thread_keep_alive(),
                thread_stack_size:          thread_stack_size(),
                enable_io:                  enable_io(),
                enable_time:                enable_time(),
            }
        }
    }
}
