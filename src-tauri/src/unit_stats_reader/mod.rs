use std::collections::HashMap;

use crate::process::ProcessHandle;

mod adjustment;
#[cfg(test)]
mod adjustment_tests;
pub(crate) mod fallback;
#[cfg(test)]
mod fixtures;
#[cfg(all(test, target_os = "windows"))]
mod own_process_tests;
mod snapshot;
#[cfg(all(test, target_os = "windows"))]
pub(crate) mod stat_acquisition_fixtures;
#[cfg(all(test, target_os = "windows"))]
mod stat_acquisition_policy_tests;
#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatReaderError {
    InvalidUnitPointer,
    NullStatList,
    InvalidStatId(u32),
    AddressOverflow,
    MalformedDescriptor { count: i16, array_ptr: u32 },
    ReadLength { expected: usize, actual: usize },
    MemoryReadFailed(String),
    UnsortedOrDuplicateKeys,
    UnstableSnapshot,
    MetadataUnavailable,
    InvalidShift(u8),
}

impl std::fmt::Display for StatReaderError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidUnitPointer => formatter.write_str("invalid unit pointer"),
            Self::NullStatList => formatter.write_str("null stat list"),
            Self::InvalidStatId(id) => write!(formatter, "stat id outside u16: {id}"),
            Self::AddressOverflow => formatter.write_str("address outside target u32 domain"),
            Self::MalformedDescriptor { count, .. } => {
                write!(formatter, "invalid stat descriptor count: {count}")
            }
            Self::ReadLength { expected, actual } => {
                write!(formatter, "read length {actual}, expected {expected}")
            }
            Self::MemoryReadFailed(message) => write!(formatter, "memory read failed: {message}"),
            Self::UnsortedOrDuplicateKeys => {
                formatter.write_str("stat keys not strictly increasing")
            }
            Self::UnstableSnapshot => formatter.write_str("stat descriptor changed during read"),
            Self::MetadataUnavailable => formatter.write_str("required stat metadata unavailable"),
            Self::InvalidShift(shift) => {
                write!(formatter, "invalid stat adjustment shift: {shift}")
            }
        }
    }
}

impl std::error::Error for StatReaderError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatReadResult {
    Found(i32),
    Missing,
}

#[derive(Clone, Copy)]
struct Source {
    common: usize,
    unit: u32,
}

struct StatKey {
    id: u32,
    layer: u16,
}

#[derive(Clone, Copy)]
struct Request<'a> {
    ids: &'a [u32],
    layer: u16,
}

type ReadMemory<'a> = dyn FnMut(usize, usize) -> Result<Vec<u8>, String> + 'a;

pub struct UnitStatsReader<'a> {
    process: &'a ProcessHandle,
    source: Source,
}

impl<'a> UnitStatsReader<'a> {
    pub const fn new(process: &'a ProcessHandle, d2_common: usize, unit: u32) -> Self {
        Self {
            process,
            source: Source {
                common: d2_common,
                unit,
            },
        }
    }

    pub fn read_stat(&self, stat_id: u32, layer: u16) -> Result<StatReadResult, StatReaderError> {
        read_single(
            &mut |address, size| self.process.read_buffer(address, size),
            self.source,
            StatKey { id: stat_id, layer },
        )
    }

    pub fn read_bulk(&self, ids: &[u32], layer: u16) -> Result<HashMap<u32, i32>, StatReaderError> {
        acquire(
            &mut |address, size| self.process.read_buffer(address, size),
            self.source,
            Request { ids, layer },
        )
    }
}

fn read_single(
    read: &mut ReadMemory<'_>,
    source: Source,
    key: StatKey,
) -> Result<StatReadResult, StatReaderError> {
    let values = acquire(
        read,
        source,
        Request {
            ids: &[key.id],
            layer: key.layer,
        },
    )?;
    Ok(match values.get(&key.id) {
        Some(&value) => StatReadResult::Found(value),
        None => StatReadResult::Missing,
    })
}

fn acquire(
    read: &mut ReadMemory<'_>,
    source: Source,
    request: Request<'_>,
) -> Result<HashMap<u32, i32>, StatReaderError> {
    if request.ids.is_empty() {
        return Ok(HashMap::new());
    }
    for &id in request.ids {
        u16::try_from(id).map_err(|_| StatReaderError::InvalidStatId(id))?;
    }
    match attempt(read, source, request) {
        Ok(values) => Ok(values),
        Err(_) => attempt(read, source, request),
    }
}

fn attempt(
    read: &mut ReadMemory<'_>,
    source: Source,
    request: Request<'_>,
) -> Result<HashMap<u32, i32>, StatReaderError> {
    let descriptor = snapshot::Descriptor::load(read, source.unit)?;
    let records = descriptor.records(read)?;
    descriptor.revalidate(read, source.unit)?;
    let mut values = HashMap::with_capacity(request.ids.len());
    let adjustment = adjustment::Adjustment {
        common: source.common,
        descriptor: &descriptor,
    };
    for &id in request.ids {
        let key = (id << 16) | u32::from(request.layer);
        if let Ok(index) = records.binary_search_by_key(&key, |record| record.key) {
            let value = adjustment.apply(read, &records[index])?;
            values.insert(id, value);
        }
    }
    descriptor.revalidate(read, source.unit)?;
    Ok(values)
}
