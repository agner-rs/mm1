use std::collections::HashSet;
use std::fmt;

use either::Either;
use mm1_address::address::Address;
use mm1_common::log;

use crate::common::restart_intensity::{RestartIntensity, RestartStats};
use crate::mixed::decider::{Action, Decider};
use crate::mixed::strategy::{AllForOne, DeciderError, RestartStrategy};

pub struct AllForOneDecider<K> {
    restart_intensity: RestartIntensity,
    restart_stats:     RestartStats,
    status:            SupStatus,
    states:            Vec<(K, State)>,
    orphans:           HashSet<Address>,
}

impl<K> AllForOne<K> {
    pub fn new(restart_intensity: RestartIntensity) -> Self {
        Self {
            restart_intensity,
            _pd: Default::default(),
        }
    }
}

impl<K> RestartStrategy<K> for AllForOne<K>
where
    AllForOneDecider<K>: Decider<Key = K>,
{
    type Decider = AllForOneDecider<K>;

    fn decider(&self) -> Self::Decider {
        let restart_intensity = self.restart_intensity;
        let restart_stats = restart_intensity.new_stats();
        AllForOneDecider {
            restart_intensity,
            restart_stats,
            status: SupStatus::Running,
            states: Default::default(),
            orphans: Default::default(),
        }
    }
}

impl<K> Clone for AllForOne<K> {
    fn clone(&self) -> Self {
        Self {
            restart_intensity: self.restart_intensity,
            _pd:               Default::default(),
        }
    }
}

enum SupStatus {
    Running,
    Restarting,
    Stopping { normal_exit: bool },
    Stopped,
}

struct State {
    target: Target,
    status: Status,
}

#[derive(Debug, Clone, Copy)]
enum Target {
    Running,
    Stopped,
}

#[derive(Debug, Clone, Copy)]
enum Status {
    Starting,
    Running { address: Address },
    Terminating { address: Address },
    Stopped,
}

impl<K> Decider for AllForOneDecider<K>
where
    K: fmt::Display + Eq,
{
    type Error = DeciderError;
    type Key = K;

    fn add(&mut self, key: Self::Key) -> Result<(), Self::Error> {
        if self.states.iter().any(|(k, _)| *k == key) {
            return Err(DeciderError::DuplicateKey)
        }
        self.states.push((
            key,
            State {
                target: Target::Running,
                status: Status::Stopped,
            },
        ));
        Ok(())
    }

    fn rm(&mut self, _key: &Self::Key) -> Result<(), Self::Error> {
        Err(DeciderError::Unsupported)
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
                "reported start, key not found [key: {}; addr: {}]",
                key,
                reported_address
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
            log::warn!(
                "reporrted exit, address not found [addr: {}]",
                reported_addr
            );
            return;
        };

        match (state.status, normal_exit) {
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
                    log::info!("restart intensity exceeded; giving up. Reason: {}", reason);
                    self.status = SupStatus::Stopping { normal_exit: false };
                } else {
                    self.status = SupStatus::Restarting;
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

    fn next_action(
        &mut self,
        _at: tokio::time::Instant,
    ) -> Result<Option<Action<'_, Self::Key>>, Self::Error> {
        if let Some(address) = self.orphans.iter().next().copied() {
            self.orphans.remove(&address);
            return Ok(Some(Action::Stop {
                address,
                child_id: None,
            }))
        }

        let states_iter_mut = match self.status {
            SupStatus::Running => Either::Left(Either::Left(self.states.iter_mut())),
            SupStatus::Stopping { .. } | SupStatus::Restarting => {
                Either::Left(Either::Right(self.states.iter_mut().rev()))
            },
            SupStatus::Stopped => Either::Right(std::iter::empty()),
        };

        let mut all_children_stopped = true;
        for (key, state) in states_iter_mut {
            let status = state.status;
            let target = match self.status {
                SupStatus::Running => state.target,
                SupStatus::Stopping { .. } | SupStatus::Restarting => Target::Stopped,
                SupStatus::Stopped => unreachable!("wouldn't iterate over children"),
            };

            all_children_stopped = all_children_stopped && matches!(status, Status::Stopped);

            log::debug!(
                "considering {}; status: {:?}, target: {:?}",
                key,
                status,
                target
            );

            match (status, target) {
                // Nothing we can do
                (Status::Starting, Target::Running | Target::Stopped) => continue,
                (Status::Terminating { .. }, Target::Running | Target::Stopped) => continue,

                // Nothing we should do
                (Status::Running { .. }, Target::Running) => continue,
                (Status::Stopped, Target::Stopped) => continue,

                // Do something!
                (Status::Stopped, Target::Running) => {
                    state.status = Status::Starting;
                    return Ok(Some(Action::Start { child_id: key }))
                },
                (Status::Running { address }, Target::Stopped) => {
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
            SupStatus::Restarting => {
                log::info!(
                    "restarting [all-children-stopped: {}]",
                    all_children_stopped
                );
                if all_children_stopped {
                    self.status = SupStatus::Running;
                    Ok(Some(Action::Noop))
                } else {
                    Ok(None)
                }
            },
            SupStatus::Stopping { normal_exit } => {
                log::info!(
                    "stopping [normal: {}; all-children-stopped: {}]",
                    normal_exit,
                    all_children_stopped
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

#[cfg(test)]
mod tests;
