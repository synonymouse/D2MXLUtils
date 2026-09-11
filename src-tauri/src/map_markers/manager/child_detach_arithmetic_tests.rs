use super::*;

#[test]
fn checked_slot_when_pointer_or_word_end_overflows() {
    for (base, offset, expected) in [
        (u32::MAX - 3, 0, Some(u32::MAX - 3)),
        (u32::MAX - 2, 0, None),
        (u32::MAX - 4, automap_cell::P_MORE, None),
        (u32::MAX, automap_layer::P_OBJECTS, None),
    ] {
        let actual = slot(base, offset);

        assert_eq!(actual.ok(), expected);
    }
}
