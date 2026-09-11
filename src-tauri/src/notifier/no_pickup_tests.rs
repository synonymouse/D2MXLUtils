use super::*;

#[test]
fn no_pickup_write_targets_d2client_flag_byte() {
    let (addr, bytes) = no_pickup_flag_write(0x1000_0000, true);

    assert_eq!(addr, 0x1000_0000 + d2client::NO_PICKUP_FLAG);
    assert_eq!(bytes, [1]);
}

#[test]
fn no_pickup_write_can_disable_flag() {
    let (_, bytes) = no_pickup_flag_write(0x1000_0000, false);

    assert_eq!(bytes, [0]);
}
