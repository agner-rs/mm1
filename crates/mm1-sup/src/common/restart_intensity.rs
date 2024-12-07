use crate::common::time;

#[derive(Debug, thiserror::Error)]
#[error("max restart intensity reached")]
pub struct MaxRestartIntensityReached;

#[derive(Debug, Clone, Copy)]
pub struct RestartIntensity<D> {
    pub max_restarts: usize,
    pub within:       D,
}

#[derive(Debug)]
pub struct RestartStats<T>(Vec<T>);

impl<D> RestartIntensity<D>
where
    D: time::D,
{
    pub fn new_stats(&self) -> RestartStats<D::I> {
        RestartStats(Vec::with_capacity(self.max_restarts + 1))
    }

    pub fn report_exit(
        &self,
        stats: &mut RestartStats<D::I>,
        now: D::I,
    ) -> Result<(), MaxRestartIntensityReached> {
        stats.0.retain(|i| (now - *i) < self.within);
        stats.0.push(now);

        if stats.0.len() > self.max_restarts {
            Err(MaxRestartIntensityReached)
        } else {
            Ok(())
        }
    }
}
