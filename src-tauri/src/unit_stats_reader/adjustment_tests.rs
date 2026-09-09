use super::fixtures::{Memory, COMMON, RECORD, UNIT};
use super::*;

fn read(memory: &mut Memory) -> Result<StatReadResult, StatReaderError> {
    read_single(
        &mut |address, size| memory.read(address, size),
        Source {
            common: COMMON,
            unit: UNIT,
        },
        StatKey { id: 12, layer: 0 },
    )
}

#[test]
fn applies_floor_when_extended_player_or_monster_below_threshold() {
    for owner in [0u32, 1] {
        let mut memory = Memory::extended(50);
        memory.put(0x5000, owner.to_le_bytes());
        let result = read(&mut memory);
        assert_eq!(result, Ok(StatReadResult::Found(400)));
    }
}

#[test]
fn preserves_signed_bits_when_adjustment_shift_overflows_value() {
    for (floor, shift, raw, expected) in [
        (100i32, 0u8, -5i32, 100i32),
        (-100, 2, -101, -400),
        (1, 31, 0, i32::MIN),
        (0x40000001, 2, 0, 4),
    ] {
        let mut memory = Memory::extended(raw);
        memory.put(RECORD + 0x2c, floor.to_le_bytes());
        memory.put(RECORD + 0x18, [shift]);
        let result = read(&mut memory);
        assert_eq!(result, Ok(StatReadResult::Found(expected)));
    }
}

#[test]
fn returns_raw_when_proven_no_adjustment() {
    for (address, bytes) in [
        (RECORD + 5, vec![0]),
        (0x7008, vec![2]),
        (0x5000, 4u32.to_le_bytes().to_vec()),
        (RECORD + 0x2c, 50i32.to_le_bytes().to_vec()),
    ] {
        let mut memory = Memory::extended(50);
        memory.put(address, bytes);
        let result = read(&mut memory);
        assert_eq!(result, Ok(StatReadResult::Found(50)));
    }
}

#[test]
fn errors_when_any_required_metadata_read_unavailable() {
    for address in [
        COMMON + 0x99e1c,
        0x6bd4,
        0x6bcc,
        RECORD + 5,
        COMMON + 0x890b0,
        0x7008,
        0x2044,
        0x5000,
        RECORD + 0x2c,
        RECORD + 0x18,
    ] {
        let mut memory = Memory::extended(50);
        memory.data.remove(&address);
        let result = read(&mut memory);
        assert!(result.is_err(), "metadata address {address:x}");
    }
}

#[test]
fn errors_when_required_metadata_scalars_short() {
    for address in [
        COMMON + 0x99e1c,
        0x6bd4,
        0x6bcc,
        RECORD + 5,
        COMMON + 0x890b0,
        0x7008,
        0x2044,
        0x5000,
        RECORD + 0x2c,
        RECORD + 0x18,
    ] {
        let mut memory = Memory::extended(50);
        memory.put(address, Vec::new());
        let result = read(&mut memory);
        assert!(matches!(result, Err(StatReaderError::ReadLength { .. })));
    }
}

#[test]
fn errors_when_required_metadata_pointer_null() {
    for address in [COMMON + 0x99e1c, 0x6bcc, COMMON + 0x890b0, 0x2044] {
        let mut memory = Memory::extended(50);
        memory.put(address, 0u32.to_le_bytes());
        let result = read(&mut memory);
        assert!(matches!(result, Err(StatReaderError::MetadataUnavailable)));
    }
}

#[test]
fn errors_when_index_outside_table_or_shift_invalid() {
    let mut memory = Memory::extended(50);
    memory.put(0x6bd4, 12u32.to_le_bytes());
    let result = read(&mut memory);
    assert_eq!(result, Err(StatReaderError::MetadataUnavailable));
    for shift in [32, 255] {
        let mut memory = Memory::extended(50);
        memory.put(RECORD + 0x18, [shift]);
        let result = read(&mut memory);
        assert_eq!(result, Err(StatReaderError::InvalidShift(shift)));
    }
}

#[test]
fn errors_when_common_or_record_address_invalid() {
    for common in [0, usize::MAX] {
        let mut memory = Memory::extended(50);
        let result = read_single(
            &mut |address, size| memory.read(address, size),
            Source { common, unit: UNIT },
            StatKey { id: 12, layer: 0 },
        );
        assert!(result.is_err());
    }
    let mut memory = Memory::extended(50);
    memory.put(0x6bcc, (u32::MAX - 10).to_le_bytes());
    let result = read(&mut memory);
    assert_eq!(result, Err(StatReaderError::AddressOverflow));
}

#[test]
fn revalidates_after_adjustment_before_returning() {
    let mut memory = Memory::extended(50);
    memory.sequence(
        0x204c,
        [1i16, 1, 2, 1, 1, 2]
            .map(|value| value.to_le_bytes().to_vec())
            .to_vec(),
    );
    let result = read(&mut memory);
    assert_eq!(result, Err(StatReaderError::UnstableSnapshot));
    assert_eq!(memory.reads.get(&(RECORD + 0x18)), Some(&2));
}

#[test]
fn bulk_adjustment_uses_same_validated_path() {
    let mut memory = Memory::extended(50);
    let result = acquire(
        &mut |address, size| memory.read(address, size),
        Source {
            common: COMMON,
            unit: UNIT,
        },
        Request {
            ids: &[12, 93],
            layer: 0,
        },
    );
    assert_eq!(result, Ok(HashMap::from([(12, 400)])));
}
