use super::*;

#[test]
fn e9_prologue_is_existing_hook_target_not_mismatch() {
    let ord10887_addr = 0x6FD8_A740;
    let trampoline_addr = 0x275D_0000;
    let rel = (trampoline_addr as i64 - (ord10887_addr + 5) as i64) as i32;
    let mut prologue = [0u8; 5];
    prologue[0] = 0xE9;
    prologue[1..5].copy_from_slice(&rel.to_le_bytes());

    assert_eq!(
        classify_prologue(ord10887_addr, prologue),
        PrologueState::ExistingHook { trampoline_addr }
    );
}
