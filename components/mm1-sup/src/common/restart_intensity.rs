use std::fmt;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
#[error("max restart intensity reached")]
pub struct MaxRestartIntensityReached;

#[derive(Debug, Clone, Copy)]
pub struct RestartIntensity {
    pub max_restarts: usize,
    pub within:       Duration,
}

#[derive(Debug)]
pub struct RestartStats(Vec<tokio::time::Instant>);

impl RestartIntensity {
    pub fn new_stats(&self) -> RestartStats {
        RestartStats(Vec::with_capacity(self.max_restarts + 1))
    }

    pub fn report_exit(
        &self,
        stats: &mut RestartStats,
        now: tokio::time::Instant,
    ) -> Result<(), MaxRestartIntensityReached> {
        let t_cutoff = now.checked_sub(self.within).unwrap_or(now);
        stats.0.retain(|t| *t >= t_cutoff);
        stats.0.push(now);

        if stats.0.len() > self.max_restarts {
            Err(MaxRestartIntensityReached)
        } else {
            Ok(())
        }
    }
}

impl Default for RestartIntensity {
    fn default() -> Self {
        Self {
            max_restarts: 0,
            within:       Duration::MAX,
        }
    }
}

impl fmt::Display for RestartIntensity
where
    Self: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}
