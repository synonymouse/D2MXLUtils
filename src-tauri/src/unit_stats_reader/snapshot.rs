use super::{ReadMemory, StatReaderError};
use crate::offsets::stat_list;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Address(u32);

impl Address {
    pub(super) const fn new(value: u32) -> Self {
        Self(value)
    }

    pub(super) fn offset(self, offset: usize) -> Result<Self, StatReaderError> {
        let offset = u32::try_from(offset).map_err(|_| StatReaderError::AddressOverflow)?;
        self.0
            .checked_add(offset)
            .map(Self)
            .ok_or(StatReaderError::AddressOverflow)
    }

    pub(super) fn bytes(
        self,
        read: &mut ReadMemory<'_>,
        size: usize,
    ) -> Result<Vec<u8>, StatReaderError> {
        self.offset(size.saturating_sub(1))?;
        let address = usize::try_from(self.0).map_err(|_| StatReaderError::AddressOverflow)?;
        let bytes = read(address, size).map_err(StatReaderError::MemoryReadFailed)?;
        if bytes.len() != size {
            return Err(StatReaderError::ReadLength {
                expected: size,
                actual: bytes.len(),
            });
        }
        Ok(bytes)
    }

    pub(super) fn scalar<const SIZE: usize>(
        self,
        read: &mut ReadMemory<'_>,
    ) -> Result<[u8; SIZE], StatReaderError> {
        let bytes = self.bytes(read, SIZE)?;
        bytes
            .try_into()
            .map_err(|bytes: Vec<u8>| StatReaderError::ReadLength {
                expected: SIZE,
                actual: bytes.len(),
            })
    }

    pub(super) fn pointer(self, read: &mut ReadMemory<'_>) -> Result<Self, StatReaderError> {
        let value = u32::from_le_bytes(self.scalar(read)?);
        if value == 0 {
            return Err(StatReaderError::MetadataUnavailable);
        }
        Ok(Self(value))
    }
}

pub(super) struct Record {
    pub(super) key: u32,
    pub(super) value: i32,
}

pub(super) struct Descriptor {
    pub(super) stats: Address,
    pub(super) flags: u32,
    array: Address,
    count: i16,
    array_field: Address,
    count_field: Address,
}

impl Descriptor {
    pub(super) fn load(read: &mut ReadMemory<'_>, unit: u32) -> Result<Self, StatReaderError> {
        if unit == 0 {
            return Err(StatReaderError::InvalidUnitPointer);
        }
        let stats = u32::from_le_bytes(
            Address(unit)
                .offset(stat_list::UNIT_TO_STATS_LIST)?
                .scalar(read)?,
        );
        if stats == 0 {
            return Err(StatReaderError::NullStatList);
        }
        let stats = Address(stats);
        let flags = u32::from_le_bytes(stats.offset(stat_list::SL_FLAGS)?.scalar(read)?);
        let (array_offset, count_offset) = if flags & stat_list::SL_FLAG_EX == 0 {
            (stat_list::SL_PSTAT, stat_list::SL_STAT_COUNT)
        } else {
            (stat_list::SL_FULL_PSTAT, stat_list::SL_FULL_STAT_COUNT)
        };
        let array_field = stats.offset(array_offset)?;
        let count_field = stats.offset(count_offset)?;
        let array = Address(u32::from_le_bytes(array_field.scalar(read)?));
        let count = i16::from_le_bytes(count_field.scalar(read)?);
        if !(0..=2048).contains(&count) || (count > 0 && array.0 == 0) {
            return Err(StatReaderError::MalformedDescriptor {
                count,
                array_ptr: array.0,
            });
        }
        Ok(Self {
            stats,
            flags,
            array,
            count,
            array_field,
            count_field,
        })
    }

    pub(super) fn revalidate(
        &self,
        read: &mut ReadMemory<'_>,
        unit: u32,
    ) -> Result<(), StatReaderError> {
        let stats = u32::from_le_bytes(
            Address(unit)
                .offset(stat_list::UNIT_TO_STATS_LIST)?
                .scalar(read)?,
        );
        let flags = u32::from_le_bytes(self.stats.offset(stat_list::SL_FLAGS)?.scalar(read)?);
        let array = u32::from_le_bytes(self.array_field.scalar(read)?);
        let count = i16::from_le_bytes(self.count_field.scalar(read)?);
        if (stats, flags, array, count) != (self.stats.0, self.flags, self.array.0, self.count) {
            return Err(StatReaderError::UnstableSnapshot);
        }
        Ok(())
    }

    pub(super) fn records(
        &self,
        read: &mut ReadMemory<'_>,
    ) -> Result<Vec<Record>, StatReaderError> {
        let count =
            usize::try_from(self.count).map_err(|_| StatReaderError::MalformedDescriptor {
                count: self.count,
                array_ptr: self.array.0,
            })?;
        if count == 0 {
            return Ok(Vec::new());
        }
        let size = count
            .checked_mul(stat_list::STAT_RECORD_SIZE)
            .ok_or(StatReaderError::AddressOverflow)?;
        let bytes = self.array.bytes(read, size)?;
        let mut records: Vec<Record> = Vec::with_capacity(count);
        for chunk in bytes.chunks_exact(stat_list::STAT_RECORD_SIZE) {
            let key = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            let value = i32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
            if records.last().is_some_and(|previous| previous.key >= key) {
                return Err(StatReaderError::UnsortedOrDuplicateKeys);
            }
            records.push(Record { key, value });
        }
        Ok(records)
    }
}
