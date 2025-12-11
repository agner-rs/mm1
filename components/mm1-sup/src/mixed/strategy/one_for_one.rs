use std::collections::HashSet;
use std::fmt;

use either::Either;
use mm1_address::address::Address;
use mm1_common::errors::chain::StdErrorDisplayChainExt;
use mm1_common::log;

use crate::common::restart_intensity::{RestartIntensity, RestartStats};
use crate::mixed::ChildType;
use crate::mixed::decider::{Action, Decider};
use crate::mixed::strategy::{DeciderError, OneForOne, RestartStrategy};

pub struct OneForOneDecider<K> {
    restart_intensity: RestartIntensity,
    restart_stats:     RestartStats,
    states:            Vec<(K, State)>,
    orphans:           HashSet<Address>,
    status:            SupStatus,
}

#[derive(Debug)]
struct State {
    child_type: ChildType,
    status:     Status,
    target:     Target,
}

#[derive(Debug, Clone, Copy)]
enum Status {
    Starting,
    Running { address: Address },
    Terminating { address: Address },
    Stopped,
}

#[derive(Debug, Clone, Copy)]
enum Target {
    Running,
    Stopped,
    Removed,
}

enum SupStatus {
    Starting,
    Running,
    Stopping { normal_exit: bool },
    Stopped,
}

impl<K> OneForOne<K> {
    pub fn new(restart_intensity: RestartIntensity) -> Self {
        Self {
            restart_intensity,
            _pd: Default::default(),
        }
    }
}

impl<K> RestartStrategy<K> for OneForOne<K>
where
    Self: Clone,
    OneForOneDecider<K>: Decider<Key = K>,
{
    type Decider = OneForOneDecider<K>;

    fn decider(&self) -> Self::Decider {
        let restart_intensity = self.restart_intensity;
        let restart_stats = restart_intensity.new_stats();
        OneForOneDecider {
            restart_intensity,
            restart_stats,
            states: Default::default(),
            orphans: Default::default(),
            status: SupStatus::Starting,
        }
    }
}

impl<K> Decider for OneForOneDecider<K>
where
    K: fmt::Display + Eq,
{
    type Error = DeciderError;
    type Key = K;

    fn address(&self, key: &Self::Key) -> Result<Option<Address>, Self::Error> {
        self.states
            .iter()
            .find_map(|(k, s)| {
                (key == k).then_some(match s.status {
                    Status::Running { address } | Status::Terminating { address } => Some(address),
                    _ => None,
                })
            })
            .ok_or(DeciderError::KeyNotFound)
    }

    fn add(&mut self, key: Self::Key, child_type: ChildType) -> Result<(), Self::Error> {
        if self.states.iter().any(|(k, _)| *k == key) {
            return Err(DeciderError::DuplicateKey)
        }
        self.states.push((
            key,
            State {
                child_type,
                status: Status::Stopped,
                target: Target::Running,
            },
        ));
        Ok(())
    }

    fn rm(&mut self, key: &Self::Key) -> Result<(), Self::Error> {
        if let Some(state) = self
            .states
            .iter_mut()
            .find_map(|(k, s)| (k == key).then_some(s))
        {
            state.target = Target::Removed;
        }
        Ok(())
    }

    fn started(&mut self, key: &Self::Key, reported_address: Address, _at: tokio::time::Instant) {
        assert!(!self.states.iter().any(|(_, s)|
                matches!(s.status,
                    Status::Running { address } | Status::Terminating { address } if address == reported_address
                )
            )
        );

        let Some(state) = self
            .states
            .iter_mut()
            .find_map(|(k, s)| (k == key).then_some(s))
        else {
            log::warn!(
                key = %key, addr = %reported_address,
                "reported start, key not found"
            );
            return
        };
        match state.status {
            Status::Running { .. } | Status::Terminating { .. } | Status::Stopped => {
                self.orphans.insert(reported_address);
            },
            Status::Starting => {
                state.status = Status::Running {
                    address: reported_address,
                };
            },
        }
    }

    fn exited(&mut self, reported_addr: Address, normal_exit: bool, at: tokio::time::Instant) {
        let Some(state) = self.states.iter_mut().find_map(|(_, s)| {
            matches!(s.status,
                    Status::Running { address } |
                    Status::Terminating { address }
                    if address == reported_addr)
            .then_some(s)
        }) else {
            log::info!(
                by_addr = %reported_addr, normal_exit = %normal_exit,
                "termination requested"
            );
            self.status = SupStatus::Stopping { normal_exit: true };
            return;
        };

        let is_acceptable = match state.child_type {
            ChildType::Permanent => false,
            ChildType::Transient => normal_exit,
            ChildType::Temporary => true,
        };
        match (state.status, is_acceptable) {
            (Status::Terminating { address }, _) | (Status::Running { address }, true) => {
                assert_eq!(address, reported_addr);
                state.status = Status::Stopped;
            },
            (Status::Running { address }, false) => {
                assert_eq!(address, reported_addr);
                if let Err(reason) = self
                    .restart_intensity
                    .report_exit(&mut self.restart_stats, at)
                {
                    log::info!(reason = %reason.as_display_chain(), "restart intensity exceeded; giving up");
                    self.status = SupStatus::Stopping { normal_exit: false };
                }
                state.status = Status::Stopped;
            },
            _ => unreachable!("how could this state be selected?"),
        }
    }

    fn failed(&mut self, key: &Self::Key, _at: tokio::time::Instant) {
        if let Some(state) = self
            .states
            .iter_mut()
            .find_map(|(k, s)| (k == key).then_some(s))
        {
            match state.status {
                Status::Starting => {
                    state.status = Status::Stopped;
                },
                _ => {
                    state.target = Target::Stopped;
                },
            }
            self.status = SupStatus::Stopping { normal_exit: false }
        }
    }

    fn quit(&mut self, normal_exit: bool) {
        self.status = match self.status {
            SupStatus::Stopped => SupStatus::Stopped,
            SupStatus::Stopping {
                normal_exit: existing_normal_exit,
            } => {
                SupStatus::Stopping {
                    normal_exit: normal_exit && existing_normal_exit,
                }
            },
            SupStatus::Starting | SupStatus::Running => SupStatus::Stopping { normal_exit },
        };
    }

    fn next_action(
        &mut self,
        _at: tokio::time::Instant,
    ) -> Result<Option<Action<'_, Self::Key>>, Self::Error> {
        self.states.retain(|(_k, state)| {
            !matches!(
                (state.status, state.target),
                (Status::Stopped, Target::Removed)
            )
        });

        if let Some(address) = self.orphans.iter().next().copied() {
            self.orphans.remove(&address);
            return Ok(Some(Action::Stop {
                address,
                child_id: None,
            }))
        }

        let states_iter_mut = match self.status {
            SupStatus::Starting | SupStatus::Running => {
                Either::Left(Either::Left(self.states.iter_mut()))
            },
            SupStatus::Stopping { .. } => Either::Left(Either::Right(self.states.iter_mut().rev())),
            SupStatus::Stopped => Either::Right(std::iter::empty()),
        };
        let mut all_children_started = true;
        let mut all_children_stopped = true;
        for &mut (ref key, ref mut state) in states_iter_mut {
            let status = state.status;
            let target = match self.status {
                SupStatus::Starting | SupStatus::Running => state.target,
                SupStatus::Stopping { .. } => Target::Stopped,
                SupStatus::Stopped => unreachable!("wouldn't iterate over children"),
            };

            all_children_stopped = all_children_stopped && matches!(status, Status::Stopped);
            all_children_started = all_children_started && matches!(status, Status::Running { .. });

            log::debug!(
                key = %key, status = ?status, target = ?target,
                "considering"
            );

            match (status, target) {
                // Nothing we can do
                (Status::Starting, Target::Running | Target::Stopped | Target::Removed) => continue,
                (
                    Status::Terminating { .. },
                    Target::Running | Target::Stopped | Target::Removed,
                ) => continue,

                // Nothing we should do
                (Status::Running { .. }, Target::Running) => continue,
                (Status::Stopped, Target::Stopped) => continue,

                // Should not happen
                (Status::Stopped, Target::Removed) => unreachable!("filtered out above"),

                // Do something!
                (Status::Stopped, Target::Running) => {
                    state.status = Status::Starting;
                    return Ok(Some(Action::Start { child_id: key }))
                },
                (Status::Running { address }, Target::Stopped | Target::Removed) => {
                    state.status = Status::Terminating { address };
                    return Ok(Some(Action::Stop {
                        address,
                        child_id: Some(key),
                    }))
                },
            }
        }

        match self.status {
            SupStatus::Running | SupStatus::Stopped => Ok(None),

            SupStatus::Starting => {
                log::info!(all_children_started = %all_children_started, "starting");
                if all_children_started {
                    self.status = SupStatus::Running;
                    Ok(Some(Action::InitDone))
                } else {
                    Ok(None)
                }
            },

            SupStatus::Stopping { normal_exit } => {
                log::info!(
                    normal = %normal_exit, all_children_stopped = %all_children_stopped,
                    "stopping"
                );
                if all_children_stopped {
                    self.status = SupStatus::Stopped;
                    Ok(Some(Action::Quit { normal_exit }))
                } else {
                    Ok(None)
                }
            },
        }
    }
}

impl<K> Clone for OneForOne<K> {
    fn clone(&self) -> Self {
        Self {
            restart_intensity: self.restart_intensity,
            _pd:               Default::default(),
        }
    }
}

#[cfg(test)]
mod tests;
