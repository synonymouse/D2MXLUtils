use super::{TelemetrySampler, TelemetrySnapshot, TelemetrySummary};
use std::time::Instant;

#[derive(Default)]
pub(crate) struct TelemetrySession {
    sampler: Option<TelemetrySampler>,
}

pub(crate) enum TelemetryEvent {
    Start(TelemetrySnapshot),
    Periodic(TelemetrySummary),
    Final(TelemetrySummary),
}

impl TelemetrySession {
    pub fn poll(
        &mut self,
        now: Instant,
        snapshot: impl FnOnce() -> Option<TelemetrySnapshot>,
    ) -> Option<TelemetryEvent> {
        match &mut self.sampler {
            Some(sampler) => {
                if !sampler.is_due(now) {
                    return None;
                }
                sampler
                    .sample(now, snapshot()?)
                    .map(TelemetryEvent::Periodic)
            }
            None => {
                let baseline = snapshot()?;
                self.sampler = Some(TelemetrySampler::new(now, baseline));
                Some(TelemetryEvent::Start(baseline))
            }
        }
    }

    pub fn finish(
        self,
        now: Instant,
        snapshot: impl FnOnce() -> Option<TelemetrySnapshot>,
    ) -> Option<TelemetryEvent> {
        let summary = self.sampler?.sample(now, snapshot()?)?;
        summary
            .delta
            .has_activity()
            .then_some(TelemetryEvent::Final(summary))
    }
}
