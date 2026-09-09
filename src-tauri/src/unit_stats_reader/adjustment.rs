use super::snapshot::{Address, Descriptor, Record};
use super::{ReadMemory, StatReaderError};
use crate::offsets::{d2common, data_tables, item_stat_cost, stat_list};

pub(super) struct Adjustment<'a> {
    pub(super) common: usize,
    pub(super) descriptor: &'a Descriptor,
}

impl Adjustment<'_> {
    pub(super) fn apply(
        &self,
        read: &mut ReadMemory<'_>,
        record: &Record,
    ) -> Result<i32, StatReaderError> {
        if self.descriptor.flags & stat_list::SL_FLAG_EX == 0 {
            return Ok(record.value);
        }
        if self.common == 0 {
            return Err(StatReaderError::MetadataUnavailable);
        }
        let common =
            Address::new(u32::try_from(self.common).map_err(|_| StatReaderError::AddressOverflow)?);
        let tables = common.offset(d2common::SGPT_DATA_TABLES)?.pointer(read)?;
        let count = u32::from_le_bytes(
            tables
                .offset(data_tables::ITEM_STAT_COST_TXT_COUNT)?
                .scalar(read)?,
        );
        let id = record.key >> 16;
        if id >= count {
            return Err(StatReaderError::MetadataUnavailable);
        }
        let table = tables
            .offset(data_tables::ITEM_STAT_COST_TXT_PTR)?
            .pointer(read)?;
        let offset = usize::try_from(id)
            .map_err(|_| StatReaderError::AddressOverflow)?
            .checked_mul(item_stat_cost::RECORD_SIZE)
            .ok_or(StatReaderError::AddressOverflow)?;
        let metadata = table.offset(offset)?;
        let [op_flag] = metadata
            .offset(item_stat_cost::FIELD_OP_FLAG)?
            .scalar(read)?;
        if op_flag == 0 {
            return Ok(record.value);
        }
        let global = common
            .offset(d2common::GLOBAL_STAT_FLAGS_PTR)?
            .pointer(read)?;
        let [mask] = global.offset(8)?.scalar(read)?;
        if mask & op_flag == 0 {
            return Ok(record.value);
        }
        let owner = self
            .descriptor
            .stats
            .offset(stat_list::SL_OWNER_UNIT)?
            .pointer(read)?;
        let owner_type = u32::from_le_bytes(owner.scalar(read)?);
        if owner_type > 1 {
            return Ok(record.value);
        }
        let floor = i32::from_le_bytes(
            metadata
                .offset(item_stat_cost::FIELD_OP_BASE)?
                .scalar(read)?,
        );
        if record.value >= floor {
            return Ok(record.value);
        }
        let [shift] = metadata
            .offset(item_stat_cost::FIELD_OP_PARAM)?
            .scalar(read)?;
        floor
            .checked_shl(u32::from(shift))
            .ok_or(StatReaderError::InvalidShift(shift))
    }
}
