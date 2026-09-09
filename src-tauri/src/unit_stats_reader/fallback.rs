use std::collections::HashMap;

use super::{StatReadResult, StatReaderError, UnitStatsReader};
use crate::injection::D2Injector;
use crate::process::D2Context;
use crate::stat_telemetry::{ConsumerCounters, DirectKind, StatConsumer, ValueCounts};

pub(crate) struct StatReadContext<'a> {
    ctx: &'a D2Context,
    injector: &'a D2Injector,
    consumer: StatConsumer,
}

impl<'a> StatReadContext<'a> {
    pub const fn new(ctx: &'a D2Context, injector: &'a D2Injector, consumer: StatConsumer) -> Self {
        Self {
            ctx,
            injector,
            consumer,
        }
    }

    pub fn read_stat(&self, unit: u32, id: u32) -> Result<i32, ()> {
        self.single_with(
            id,
            || UnitStatsReader::new(&self.ctx.process, self.ctx.d2_common, unit).read_stat(id, 0),
            || {
                self.injector
                    .get_unit_stat(&self.ctx.process, unit, id)
                    .map_err(|_| ())
            },
        )
    }

    pub fn read_bulk(&self, unit: u32, ids: &[u32]) -> Result<HashMap<u32, i32>, ()> {
        let metrics = self.metrics();
        metrics.direct_attempt(DirectKind::Bulk);
        let values = match UnitStatsReader::new(&self.ctx.process, self.ctx.d2_common, unit)
            .read_bulk(ids, 0)
        {
            Ok(values) => values,
            Err(_) => {
                metrics.reader_error(DirectKind::Bulk);
                return Err(());
            }
        };
        metrics.direct_completed(DirectKind::Bulk);
        let mut counts = ValueCounts {
            found: 0,
            missing: 0,
        };
        for id in ids {
            match values.get(id) {
                Some(_) => counts.found += 1,
                None => counts.missing += 1,
            }
        }
        metrics.direct_values(DirectKind::Bulk, counts);
        if ids
            .iter()
            .any(|&id| self.accept(id, values.get(&id).copied()).is_none())
        {
            metrics.semantic_reject(DirectKind::Bulk);
            return Err(());
        }
        Ok(values)
    }

    pub fn legacy_stat(&self, unit: u32, id: u32) -> Result<i32, ()> {
        self.legacy_with(|| {
            self.injector
                .get_unit_stat(&self.ctx.process, unit, id)
                .map_err(|_| ())
        })
    }

    pub(super) fn single_with(
        &self,
        id: u32,
        direct: impl FnOnce() -> Result<StatReadResult, StatReaderError>,
        legacy: impl FnOnce() -> Result<u32, ()>,
    ) -> Result<i32, ()> {
        let metrics = self.metrics();
        metrics.direct_attempt(DirectKind::Single);
        match direct() {
            Ok(result) => {
                metrics.direct_completed(DirectKind::Single);
                let (value, counts) = match result {
                    StatReadResult::Found(value) => (
                        Some(value),
                        ValueCounts {
                            found: 1,
                            missing: 0,
                        },
                    ),
                    StatReadResult::Missing => (
                        None,
                        ValueCounts {
                            found: 0,
                            missing: 1,
                        },
                    ),
                };
                metrics.direct_values(DirectKind::Single, counts);
                match self.accept(id, value) {
                    Some(value) => return Ok(value),
                    None => metrics.semantic_reject(DirectKind::Single),
                }
            }
            Err(_) => metrics.reader_error(DirectKind::Single),
        }
        self.legacy_with(legacy)
    }

    fn legacy_with(&self, legacy: impl FnOnce() -> Result<u32, ()>) -> Result<i32, ()> {
        let metrics = self.metrics();
        metrics.fallback_attempt();
        match legacy() {
            Ok(raw) => {
                metrics.fallback_ok();
                Ok(i32::from_ne_bytes(raw.to_ne_bytes()))
            }
            Err(()) => {
                metrics.fallback_error();
                Err(())
            }
        }
    }

    fn accept(&self, id: u32, value: Option<i32>) -> Option<i32> {
        match self.consumer {
            StatConsumer::Stats if id == 12 => value.filter(|&value| value > 0),
            StatConsumer::Stats | StatConsumer::Breakpoints | StatConsumer::Damage => {
                Some(value.unwrap_or(0))
            }
            StatConsumer::Level => value.filter(|&value| value > 0),
            StatConsumer::Sockets => value.filter(|value| (1..=6).contains(value)),
        }
    }

    fn metrics(&self) -> &ConsumerCounters {
        self.injector.telemetry.consumer(self.consumer)
    }
}
