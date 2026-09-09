use super::lifecycle::TelemetryEvent;
use super::memory::ProcessMemorySample;
use super::{
    rate_per_second, InjectorCall, StatConsumer, TelemetrySnapshot, DIRECT_LABELS, FALLBACK_LABELS,
};
use serde_json::{json, Map, Value};
use std::time::Duration;

fn metrics<const N: usize>(delta: &[u64; N], cumulative: &[u64; N], elapsed: Duration) -> Value {
    let rates = delta.map(|count| rate_per_second(count, elapsed));
    json!({"delta": delta.as_slice(), "cumulative": cumulative.as_slice(), "per_second": rates.as_slice()})
}

impl TelemetryEvent {
    pub(super) fn render(&self, memory: Option<ProcessMemorySample>) -> Value {
        let zero = TelemetrySnapshot::default();
        let (event, elapsed, delta, cumulative) = match self {
            Self::Start(baseline) => ("start", Duration::ZERO, &zero, baseline),
            Self::Periodic(summary) => (
                "periodic",
                summary.elapsed,
                &summary.delta,
                &summary.cumulative,
            ),
            Self::Final(summary) => (
                "final",
                summary.elapsed,
                &summary.delta,
                &summary.cumulative,
            ),
        };
        let consumers: Map<String, Value> = StatConsumer::ALL
            .into_iter()
            .map(|consumer| {
                let index = consumer.index();
                let delta = &delta.consumers[index];
                let cumulative = &cumulative.consumers[index];
                (
                    consumer.label().to_owned(),
                    json!({
                        "single": metrics(&delta.single, &cumulative.single, elapsed),
                        "bulk": metrics(&delta.bulk, &cumulative.bulk, elapsed),
                        "fallback": metrics(&delta.fallback, &cumulative.fallback, elapsed),
                    }),
                )
            })
            .collect();
        let memory = match memory {
            Some(sample) => {
                json!({"working_set_bytes": sample.working_set_bytes.0, "private_commit_bytes": sample.private_commit_bytes.0})
            }
            None => json!("unavailable"),
        };
        json!({
            "diagnostic": "stat_telemetry", "schema": 1,
            "package_version": env!("CARGO_PKG_VERSION"), "observational": true,
            "event": event, "elapsed_seconds": elapsed.as_secs_f64(),
            "columns": {"injector_attempts": InjectorCall::ALL.map(InjectorCall::label), "direct": DIRECT_LABELS, "fallback": FALLBACK_LABELS},
            "injector_attempts": metrics(&delta.injector_attempts, &cumulative.injector_attempts, elapsed),
            "consumers": consumers, "memory": memory,
        })
    }

    pub fn log(self, memory: impl FnOnce() -> Option<ProcessMemorySample>) {
        let sample = match self {
            Self::Start(_) => None,
            Self::Periodic(_) | Self::Final(_) => memory(),
        };
        crate::logger::info(&self.render(sample).to_string());
    }
}
