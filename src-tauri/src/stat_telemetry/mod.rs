#[cfg(test)]
mod lifecycle_tests;
#[cfg(all(test, target_os = "windows"))]
mod log_tests;
#[cfg(test)]
mod tests;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

mod lifecycle;
pub(crate) mod memory;
mod rendering;
pub(crate) use lifecycle::TelemetrySession;
pub(crate) use rendering::rate_per_second;

#[derive(Clone, Copy, Debug)]
pub(crate) enum StatConsumer {
    Stats,
    Breakpoints,
    Damage,
    Level,
    Sockets,
}
impl StatConsumer {
    pub const ALL: [Self; 5] = [
        Self::Stats,
        Self::Breakpoints,
        Self::Damage,
        Self::Level,
        Self::Sockets,
    ];
    pub const fn index(self) -> usize {
        match self {
            Self::Stats => 0,
            Self::Breakpoints => 1,
            Self::Damage => 2,
            Self::Level => 3,
            Self::Sockets => 4,
        }
    }
    pub const fn label(self) -> &'static str {
        match self {
            Self::Stats => "stats",
            Self::Breakpoints => "breakpoints",
            Self::Damage => "damage",
            Self::Level => "level",
            Self::Sockets => "sockets",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum InjectorCall {
    GetString,
    GetItemName,
    GetItemStats,
    GetUnitStat,
    NewAutomapCell,
}
impl InjectorCall {
    pub const ALL: [Self; 5] = [
        Self::GetString,
        Self::GetItemName,
        Self::GetItemStats,
        Self::GetUnitStat,
        Self::NewAutomapCell,
    ];
    pub const fn index(self) -> usize {
        match self {
            Self::GetString => 0,
            Self::GetItemName => 1,
            Self::GetItemStats => 2,
            Self::GetUnitStat => 3,
            Self::NewAutomapCell => 4,
        }
    }
    pub const fn label(self) -> &'static str {
        match self {
            Self::GetString => "get_string",
            Self::GetItemName => "get_item_name",
            Self::GetItemStats => "get_item_stats",
            Self::GetUnitStat => "get_unit_stat",
            Self::NewAutomapCell => "new_automap_cell",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum DirectKind {
    Single,
    Bulk,
}

pub(crate) const DIRECT_LABELS: [&str; 6] = [
    "attempts",
    "completed",
    "reader_errors",
    "semantic_rejects",
    "found",
    "missing",
];
pub(crate) const FALLBACK_LABELS: [&str; 3] = ["attempted", "ok", "error"];

#[derive(Clone, Copy, Debug)]
pub(crate) struct ValueCounts {
    pub found: u64,
    pub missing: u64,
}

#[derive(Default)]
pub(crate) struct ConsumerCounters {
    single: [AtomicU64; 6],
    bulk: [AtomicU64; 6],
    fallback: [AtomicU64; 3],
}
impl ConsumerCounters {
    fn direct(&self, kind: DirectKind) -> &[AtomicU64; 6] {
        match kind {
            DirectKind::Single => &self.single,
            DirectKind::Bulk => &self.bulk,
        }
    }
    pub fn direct_attempt(&self, kind: DirectKind) {
        self.direct(kind)[0].fetch_add(1, Ordering::Relaxed);
    }
    pub fn direct_completed(&self, kind: DirectKind) {
        self.direct(kind)[1].fetch_add(1, Ordering::Relaxed);
    }
    pub fn reader_error(&self, kind: DirectKind) {
        self.direct(kind)[2].fetch_add(1, Ordering::Relaxed);
    }
    pub fn semantic_reject(&self, kind: DirectKind) {
        self.direct(kind)[3].fetch_add(1, Ordering::Relaxed);
    }
    /// Record only found/missing values from a validated reader result.
    pub fn direct_values(&self, kind: DirectKind, values: ValueCounts) {
        self.direct(kind)[4].fetch_add(values.found, Ordering::Relaxed);
        self.direct(kind)[5].fetch_add(values.missing, Ordering::Relaxed);
    }
    pub fn fallback_attempt(&self) {
        self.fallback[0].fetch_add(1, Ordering::Relaxed);
    }
    pub fn fallback_ok(&self) {
        self.fallback[1].fetch_add(1, Ordering::Relaxed);
    }
    pub fn fallback_error(&self) {
        self.fallback[2].fetch_add(1, Ordering::Relaxed);
    }
    fn snapshot(&self) -> ConsumerSnapshot {
        ConsumerSnapshot {
            single: std::array::from_fn(|index| self.single[index].load(Ordering::Relaxed)),
            bulk: std::array::from_fn(|index| self.bulk[index].load(Ordering::Relaxed)),
            fallback: std::array::from_fn(|index| self.fallback[index].load(Ordering::Relaxed)),
        }
    }
}

#[derive(Default)]
pub(crate) struct StatTelemetryCounters {
    injector_attempts: [AtomicU64; 5],
    consumers: [ConsumerCounters; 5],
}
impl StatTelemetryCounters {
    pub fn injector_attempt(&self, call: InjectorCall) {
        self.injector_attempts[call.index()].fetch_add(1, Ordering::Relaxed);
    }
    pub fn consumer(&self, consumer: StatConsumer) -> &ConsumerCounters {
        &self.consumers[consumer.index()]
    }
    /// Observational relaxed loads, not a transactional snapshot or created-thread counts.
    pub fn snapshot(&self) -> TelemetrySnapshot {
        TelemetrySnapshot {
            injector_attempts: std::array::from_fn(|index| {
                self.injector_attempts[index].load(Ordering::Relaxed)
            }),
            consumers: std::array::from_fn(|index| self.consumers[index].snapshot()),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ConsumerSnapshot {
    pub single: [u64; 6],
    pub bulk: [u64; 6],
    pub fallback: [u64; 3],
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TelemetrySnapshot {
    pub injector_attempts: [u64; 5],
    pub consumers: [ConsumerSnapshot; 5],
}
fn delta_array<const N: usize>(current: &[u64; N], previous: &[u64; N]) -> [u64; N] {
    std::array::from_fn(|index| current[index].saturating_sub(previous[index]))
}
impl TelemetrySnapshot {
    pub fn delta(&self, previous: &Self) -> Self {
        Self {
            injector_attempts: delta_array(&self.injector_attempts, &previous.injector_attempts),
            consumers: std::array::from_fn(|index| ConsumerSnapshot {
                single: delta_array(
                    &self.consumers[index].single,
                    &previous.consumers[index].single,
                ),
                bulk: delta_array(&self.consumers[index].bulk, &previous.consumers[index].bulk),
                fallback: delta_array(
                    &self.consumers[index].fallback,
                    &previous.consumers[index].fallback,
                ),
            }),
        }
    }
    pub fn has_activity(&self) -> bool {
        self != &Self::default()
    }
}

pub(crate) fn try_snapshot<T>(
    source: &Mutex<T>,
    snapshot: impl FnOnce(&T) -> TelemetrySnapshot,
) -> Option<TelemetrySnapshot> {
    let guard = source.try_lock().ok()?;
    Some(snapshot(&guard))
}

pub(crate) const SAMPLE_INTERVAL: Duration = Duration::from_secs(30);
pub(crate) struct TelemetrySampler {
    previous_time: Instant,
    previous: TelemetrySnapshot,
    next_due: Instant,
}
pub(crate) struct TelemetrySummary {
    pub elapsed: Duration,
    pub delta: TelemetrySnapshot,
    pub cumulative: TelemetrySnapshot,
}
impl TelemetrySampler {
    pub fn new(now: Instant, baseline: TelemetrySnapshot) -> Self {
        Self {
            previous_time: now,
            previous: baseline,
            next_due: now + SAMPLE_INTERVAL,
        }
    }
    pub fn is_due(&self, now: Instant) -> bool {
        now >= self.next_due
    }
    /// Caller only supplies successful snapshots; skipped locks leave this baseline untouched.
    pub fn sample(&mut self, now: Instant, current: TelemetrySnapshot) -> Option<TelemetrySummary> {
        let elapsed = now.saturating_duration_since(self.previous_time);
        if elapsed.is_zero() {
            return None;
        }
        let summary = TelemetrySummary {
            elapsed,
            delta: current.delta(&self.previous),
            cumulative: current,
        };
        self.previous = current;
        self.previous_time = now;
        self.next_due = now + SAMPLE_INTERVAL;
        Some(summary)
    }
}
