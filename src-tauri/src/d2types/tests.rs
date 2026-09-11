use super::ItemData;
use crate::offsets::item_data;
use std::mem::offset_of;

#[test]
fn item_data_offsets_match_known_memory_layout() {
    assert_eq!(offset_of!(ItemData, flags), item_data::FLAGS);
    assert_eq!(item_data::FILE_INDEX, 0x28);
    assert_eq!(offset_of!(ItemData, file_index), item_data::FILE_INDEX);
    assert_eq!(offset_of!(ItemData, ear_level), 0x48);
}
