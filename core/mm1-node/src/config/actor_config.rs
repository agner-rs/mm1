use std::collections::{HashMap, HashSet};

use mm1_address::subnet::NetMask;

use crate::actor_key::ActorKey;

pub(crate) trait EffectiveActorConfig {
    fn netmask(&self) -> NetMask;
    fn inbox_size(&self) -> usize;
    fn runtime_key(&self) -> Option<&str>;
    fn message_tap_key(&self) -> Option<&str>;
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct ActorConfigNode {
    #[serde(flatten, default)]
    config: ActorConfig<String>,

    #[serde(rename = "/", default)]
    sub: HashMap<String, Self>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct ActorConfig<S> {
    runtime:     Option<S>,
    message_tap: Option<S>,
    netmask:     Option<NetMask>,
    inbox_size:  Option<usize>,
}

impl ActorConfigNode {
    pub(crate) fn select(&self, actor_key: &ActorKey) -> impl EffectiveActorConfig + '_ {
        let mut out = ActorConfig {
            runtime:     self.config.runtime.as_deref(),
            message_tap: self.config.message_tap.as_deref(),
            netmask:     self.config.netmask,
            inbox_size:  self.config.inbox_size,
        };

        let mut node = self;
        for p in actor_key.path() {
            let Some(n) = node.sub.get(p).or_else(|| node.sub.get("_")) else {
                break
            };
            out.runtime = n.config.runtime.as_deref().or(out.runtime);
            out.netmask = n.config.netmask.or(out.netmask);
            out.inbox_size = n.config.inbox_size.or(out.inbox_size);
            node = n;
        }

        out
    }

    pub(crate) fn ensure_runtime_keys_are_valid(
        &self,
        available_keys: &HashSet<&str>,
    ) -> Result<(), String> {
        let mut used_keys = HashSet::<&str>::new();

        let mut q = vec![self];
        while let Some(n) = q.pop() {
            if let Some(k) = n.config.runtime.as_ref() {
                used_keys.insert(k);
            }
            q.extend(n.sub.values());
        }

        let invalid_keys = used_keys.difference(available_keys).collect::<Vec<_>>();
        if invalid_keys.is_empty() {
            Ok(())
        } else {
            Err(format!("invalid rt-keys: {invalid_keys:?}"))
        }
    }
}

impl EffectiveActorConfig for ActorConfig<&'_ str> {
    fn netmask(&self) -> NetMask {
        self.netmask.unwrap_or(defaults::DEFAULT_ACTOR_NETMASK)
    }

    fn inbox_size(&self) -> usize {
        self.inbox_size
            .unwrap_or(defaults::DEFAULT_ACTOR_INBOX_SIZE)
    }

    fn runtime_key(&self) -> Option<&str> {
        self.runtime
    }

    fn message_tap_key(&self) -> Option<&str> {
        self.message_tap
    }
}

mod defaults {
    use super::*;

    pub(super) const DEFAULT_ACTOR_NETMASK: NetMask = NetMask::M_56;
    pub(super) const DEFAULT_ACTOR_INBOX_SIZE: usize = 1024;
}
