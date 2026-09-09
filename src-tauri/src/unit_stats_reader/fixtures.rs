use std::collections::{HashMap, VecDeque};

pub(super) const UNIT: u32 = 0x1000;
pub(super) const COMMON: usize = 0x100000;
pub(super) const RECORD: usize = 0x8000 + 12 * 0x144;

pub(super) struct Memory {
    pub data: HashMap<usize, Vec<u8>>,
    pub sequences: HashMap<usize, VecDeque<Result<Vec<u8>, String>>>,
    pub reads: HashMap<usize, usize>,
}

impl Memory {
    pub fn base(records: &[(u16, u16, i32)]) -> Self {
        let mut memory = Self {
            data: HashMap::new(),
            sequences: HashMap::new(),
            reads: HashMap::new(),
        };
        memory.put(0x105c, 0x2000u32.to_le_bytes());
        memory.put(0x2010, 0u32.to_le_bytes());
        memory.put(0x2024, 0x3000u32.to_le_bytes());
        memory.put(0x2028, i16::try_from(records.len()).unwrap().to_le_bytes());
        let mut bytes = Vec::new();
        for &(id, layer, value) in records {
            bytes.extend(layer.to_le_bytes());
            bytes.extend(id.to_le_bytes());
            bytes.extend(value.to_le_bytes());
        }
        memory.data.insert(0x3000, bytes);
        memory
    }

    pub fn extended(raw: i32) -> Self {
        let mut memory = Self::base(&[(12, 0, raw)]);
        memory.put(0x2024, 0x4000u32.to_le_bytes());
        memory.put(0x4000, [0, 0, 12, 0, 231, 3, 0, 0]);
        memory.put(0x2010, 0x8000_0000u32.to_le_bytes());
        memory.put(0x2048, 0x3000u32.to_le_bytes());
        memory.put(0x204c, 1i16.to_le_bytes());
        memory.put(COMMON + 0x99e1c, 0x6000u32.to_le_bytes());
        memory.put(0x6bd4, 100u32.to_le_bytes());
        memory.put(0x6bcc, 0x8000u32.to_le_bytes());
        memory.put(RECORD + 5, [1]);
        memory.put(COMMON + 0x890b0, 0x7000u32.to_le_bytes());
        memory.put(0x7008, [1]);
        memory.put(0x2044, 0x5000u32.to_le_bytes());
        memory.put(0x5000, 0u32.to_le_bytes());
        memory.put(RECORD + 0x2c, 100i32.to_le_bytes());
        memory.put(RECORD + 0x18, [2]);
        memory
    }

    pub fn put(&mut self, address: usize, bytes: impl Into<Vec<u8>>) {
        self.data.insert(address, bytes.into());
    }

    pub fn sequence(&mut self, address: usize, values: Vec<Vec<u8>>) {
        self.sequences
            .insert(address, values.into_iter().map(Ok).collect());
    }

    pub fn read(&mut self, address: usize, size: usize) -> Result<Vec<u8>, String> {
        *self.reads.entry(address).or_default() += 1;
        if let Some(result) = self
            .sequences
            .get_mut(&address)
            .and_then(VecDeque::pop_front)
        {
            return result;
        }
        self.data
            .get(&address)
            .map(|bytes| bytes[..bytes.len().min(size)].to_vec())
            .ok_or_else(|| "fixture unavailable".to_owned())
    }
}
