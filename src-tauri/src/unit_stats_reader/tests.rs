use super::fixtures::{Memory, COMMON, UNIT};
use super::*;

fn single(memory: &mut Memory, id: u32, layer: u16) -> Result<StatReadResult, StatReaderError> {
    read_single(
        &mut |address, size| memory.read(address, size),
        Source {
            common: COMMON,
            unit: UNIT,
        },
        StatKey { id, layer },
    )
}

#[test]
fn distinguishes_zero_missing_and_negative_when_base_list() {
    let mut memory = Memory::base(&[(12, 0, 0), (12, 1, -256)]);
    let results = [
        single(&mut memory, 12, 0),
        single(&mut memory, 13, 0),
        single(&mut memory, 12, 1),
    ];
    assert_eq!(
        results,
        [
            Ok(StatReadResult::Found(0)),
            Ok(StatReadResult::Missing),
            Ok(StatReadResult::Found(-256))
        ]
    );
}

#[test]
fn bulk_uses_hand_authored_values_when_layers_differ() {
    let mut memory = Memory::base(&[(12, 0, 99), (12, 1, -256), (93, 0, 0)]);
    let result = acquire(
        &mut |address, size| memory.read(address, size),
        Source {
            common: COMMON,
            unit: UNIT,
        },
        Request {
            ids: &[12, 93, 999],
            layer: 0,
        },
    );
    assert_eq!(result, Ok(HashMap::from([(12, 99), (93, 0)])));
}

#[test]
fn empty_request_skips_io_when_source_invalid() {
    let result = acquire(
        &mut |_, _| panic!("unexpected read"),
        Source {
            common: usize::MAX,
            unit: 0,
        },
        Request { ids: &[], layer: 0 },
    );
    assert_eq!(result, Ok(HashMap::new()));
}

#[test]
fn rejects_invalid_id_before_io() {
    let result = read_single(
        &mut |_, _| panic!("unexpected read"),
        Source {
            common: 0,
            unit: UNIT,
        },
        StatKey {
            id: 65536,
            layer: 0,
        },
    );
    assert_eq!(result, Err(StatReaderError::InvalidStatId(65536)));
}

#[test]
fn rejects_counts_when_negative_or_above_limit() {
    for count in [-1i16, 2049, i16::MIN] {
        let mut memory = Memory::base(&[]);
        memory.put(0x2028, count.to_le_bytes());
        let result = single(&mut memory, 12, 0);
        assert!(matches!(
            result,
            Err(StatReaderError::MalformedDescriptor { .. })
        ));
    }
}

#[test]
fn accepts_limit_when_2048_sorted_records() {
    let records: Vec<_> = (0u16..2048).map(|id| (id, 0, i32::from(id))).collect();
    let mut memory = Memory::base(&records);
    let result = single(&mut memory, 2047, 0);
    assert_eq!(result, Ok(StatReadResult::Found(2047)));
}

#[test]
fn rejects_null_array_when_count_positive() {
    let mut memory = Memory::base(&[(12, 0, 99)]);
    memory.put(0x2024, 0u32.to_le_bytes());
    let result = single(&mut memory, 12, 0);
    assert!(matches!(
        result,
        Err(StatReaderError::MalformedDescriptor { .. })
    ));
}

#[test]
fn rejects_null_unit_and_list() {
    let mut memory = Memory::base(&[]);
    memory.put(0x105c, 0u32.to_le_bytes());
    let result = single(&mut memory, 12, 0);
    assert_eq!(result, Err(StatReaderError::NullStatList));
    let result = read_single(
        &mut |_, _| panic!("unexpected read"),
        Source {
            common: COMMON,
            unit: 0,
        },
        StatKey { id: 12, layer: 0 },
    );
    assert_eq!(result, Err(StatReaderError::InvalidUnitPointer));
}

#[test]
fn rejects_short_or_long_scalar_reads_without_panicking() {
    for (address, size) in [(0x105c, 4), (0x2010, 4), (0x2024, 4), (0x2028, 2)] {
        for length in [0, size - 1, size + 1] {
            let mut memory = Memory::base(&[(12, 0, 99)]);
            memory.sequence(address, vec![vec![0; length]; 2]);
            let result = single(&mut memory, 12, 0);
            assert_eq!(
                result,
                Err(StatReaderError::ReadLength {
                    expected: size,
                    actual: length
                })
            );
        }
    }
}

#[test]
fn rejects_short_records_and_unreadable_memory() {
    let mut memory = Memory::base(&[(12, 0, 99)]);
    memory.put(0x3000, [0; 7]);
    let result = single(&mut memory, 12, 0);
    assert_eq!(
        result,
        Err(StatReaderError::ReadLength {
            expected: 8,
            actual: 7
        })
    );
    memory.data.remove(&0x3000);
    let result = single(&mut memory, 12, 0);
    assert!(matches!(result, Err(StatReaderError::MemoryReadFailed(_))));
}

#[test]
fn rejects_unsorted_and_duplicate_keys_instead_of_missing() {
    for records in [[(50, 0, 1), (12, 0, 2)], [(12, 0, 1), (12, 0, 2)]] {
        let mut memory = Memory::base(&records);
        let result = single(&mut memory, 999, 0);
        assert_eq!(result, Err(StatReaderError::UnsortedOrDuplicateKeys));
    }
}

#[test]
fn rejects_target_address_overflow_before_io() {
    let result = read_single(
        &mut |_, _| panic!("unexpected read"),
        Source {
            common: 0,
            unit: u32::MAX,
        },
        StatKey { id: 12, layer: 0 },
    );
    assert_eq!(result, Err(StatReaderError::AddressOverflow));
    let mut memory = Memory::base(&[(12, 0, 99)]);
    memory.put(0x2024, (u32::MAX - 3).to_le_bytes());
    let result = single(&mut memory, 12, 0);
    assert_eq!(result, Err(StatReaderError::AddressOverflow));
}

#[test]
fn revalidates_empty_snapshot_and_retries_once() {
    let mut memory = Memory::base(&[]);
    memory.sequence(
        0x2028,
        vec![0i16.to_le_bytes().to_vec(), 1i16.to_le_bytes().to_vec()],
    );
    let result = single(&mut memory, 12, 0);
    assert_eq!(result, Ok(StatReadResult::Missing));
    assert_eq!(memory.reads.get(&0x105c), Some(&5));
}

#[test]
fn accepts_null_array_when_empty_and_stable() {
    let mut memory = Memory::base(&[]);
    memory.put(0x2024, 0u32.to_le_bytes());
    let result = single(&mut memory, 12, 0);
    assert_eq!(result, Ok(StatReadResult::Missing));
    assert_eq!(memory.reads.get(&0x105c), Some(&3));
}

#[test]
fn retries_whole_snapshot_when_each_descriptor_field_changes() {
    for (address, changed) in [
        (0x105c, 0x2100u32.to_le_bytes().to_vec()),
        (0x2010, 1u32.to_le_bytes().to_vec()),
        (0x2024, 0x3100u32.to_le_bytes().to_vec()),
        (0x2028, 2i16.to_le_bytes().to_vec()),
    ] {
        let mut memory = Memory::base(&[(12, 0, 99)]);
        let original = memory.data[&address].clone();
        memory.sequence(address, vec![original, changed]);
        let result = single(&mut memory, 12, 0);
        assert_eq!(result, Ok(StatReadResult::Found(99)));
        assert_eq!(memory.reads.get(&0x3000), Some(&2));
    }
}

#[test]
fn stops_after_two_attempts_when_descriptor_keeps_changing() {
    let mut memory = Memory::base(&[(12, 0, 99)]);
    memory.sequence(
        0x2028,
        [1i16, 2, 1, 2]
            .map(|value| value.to_le_bytes().to_vec())
            .to_vec(),
    );
    let result = single(&mut memory, 12, 0);
    assert_eq!(result, Err(StatReaderError::UnstableSnapshot));
    assert_eq!(memory.reads.get(&0x3000), Some(&2));
}

#[test]
fn recovers_when_first_array_read_fails() {
    let mut memory = Memory::base(&[(12, 0, 99)]);
    memory
        .sequences
        .insert(0x3000, [Err("transient".to_owned())].into());
    let result = single(&mut memory, 12, 0);
    assert_eq!(result, Ok(StatReadResult::Found(99)));
    assert_eq!(memory.reads.get(&0x3000), Some(&2));
}
