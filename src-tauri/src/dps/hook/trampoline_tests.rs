use super::*;

fn sample_params() -> BuildParams {
    BuildParams {
        ord10887_resume: 0x6FD8A745, // hypothetical D2Common.dll + 0x3A745
        blob_base: 0x10000000,
        ring_capacity: 1024,
        difficulty_addr: 0x6FBC0000 + 0x11C390,
        get_tick_count_addr: 0x77000000,
    }
}

#[test]
fn build_produces_substantial_bytecode() {
    let blob = build(&sample_params());
    // Trampoline + helper alone should be > 256 bytes; ring adds 16 KB.
    assert!(
        blob.bytes.len() > 0x100,
        "blob too small: {} bytes",
        blob.bytes.len()
    );
    assert!(blob.helper_offset > blob.trampoline_offset);
    assert!(blob.ring_offset > blob.helper_offset);
    // Ring layout: header(0x10) + 1024*16 = 16400 bytes.
    let ring_bytes = blob.bytes.len() - blob.ring_offset;
    assert_eq!(ring_bytes, 0x10 + 1024 * 16);
}

#[test]
fn ring_header_has_correct_capacity() {
    let p = sample_params();
    let blob = build(&p);
    let cap = u32::from_le_bytes(
        blob.bytes[blob.ring_offset + 8..blob.ring_offset + 12]
            .try_into()
            .unwrap(),
    );
    assert_eq!(cap, p.ring_capacity);
}

#[test]
fn last_jmp_is_e9_to_resume() {
    let p = sample_params();
    let blob = build(&p);
    // Find the `E9 rel32` that targets ord10887_resume.
    // The trampoline's resume jmp is the last E9 inside the
    // trampoline body (before the helper).
    let body = &blob.bytes[..blob.helper_offset];
    let pos = body
        .windows(5)
        .rposition(|w| w[0] == 0xE9)
        .expect("expected E9 jmp in trampoline");
    let rel = i32::from_le_bytes(blob.bytes[pos + 1..pos + 5].try_into().unwrap());
    // EIP after the rel32 dword is at blob_base + pos + 5.
    let from = p.blob_base + pos + 5;
    let abs = (from as i64 + rel as i64) as usize;
    assert_eq!(abs, p.ord10887_resume);
}

#[test]
fn helper_call_resolves_to_helper_offset() {
    let p = sample_params();
    let blob = build(&p);
    // First `E8 rel32` in the trampoline calls the helper.
    let body = &blob.bytes[..blob.helper_offset];
    let pos = body
        .windows(5)
        .position(|w| w[0] == 0xE8)
        .expect("expected E8 call in trampoline");
    let rel = i32::from_le_bytes(blob.bytes[pos + 1..pos + 5].try_into().unwrap());
    let from = p.blob_base + pos + 5;
    let abs = (from as i64 + rel as i64) as usize;
    assert_eq!(abs, p.blob_base + blob.helper_offset);
}

#[test]
fn ring_addr_immediate_matches_blob_base_plus_ring_offset() {
    let p = sample_params();
    let blob = build(&p);
    // The helper has a single `B9 imm32` (mov ecx, ring_addr).
    let helper = &blob.bytes[blob.helper_offset..blob.ring_offset];
    let pos_in_helper = helper.windows(5).position(|w| w[0] == 0xB9).unwrap();
    let imm = u32::from_le_bytes(
        helper[pos_in_helper + 1..pos_in_helper + 5]
            .try_into()
            .unwrap(),
    ) as usize;
    assert_eq!(imm, p.blob_base + blob.ring_offset);
}

#[test]
#[should_panic(expected = "ring_capacity must be a power of 2")]
fn rejects_non_power_of_two_capacity() {
    let mut p = sample_params();
    p.ring_capacity = 1000;
    build(&p);
}

/// Regression guard: the trampoline must drop monsters in `MONMODE_DEATH (0)`
/// or `MONMODE_DEAD (12)` so corpse-side stat-6 writes (visibility refresh,
/// town-portal return) don't register as fresh kills. The compiled bytecode
/// must contain `mov ecx, [eax+0x10]` followed by both
/// `test ecx, ecx` and `cmp ecx, 12`.
#[test]
fn trampoline_filters_corpse_modes() {
    let blob = build(&sample_params());
    let body = &blob.bytes[..blob.helper_offset];

    // mov ecx, [eax+0x10]  →  8B 48 10
    let mov_ecx_mode = body
        .windows(3)
        .position(|w| w == [0x8B, 0x48, 0x10])
        .expect("missing `mov ecx, [eax+0x10]` (dwMode read) in trampoline");

    let after_mov = &body[mov_ecx_mode + 3..];

    // Search forward for both `test ecx, ecx` (85 C9) and
    // `cmp ecx, 12` (83 F9 0C). They must come before any
    // unconditional jump out of the filter chain (the rel8 jmp to
    // `skip_bridge` is `EB ??`, so we cap our window at the first EB).
    let stop_at = after_mov
        .iter()
        .position(|&b| b == 0xEB)
        .unwrap_or(after_mov.len());
    let window = &after_mov[..stop_at];

    assert!(
        window.windows(2).any(|w| w == [0x85, 0xC9]),
        "expected `test ecx, ecx` (MONMODE_DEATH=0 check) after dwMode read"
    );
    assert!(
        window.windows(3).any(|w| w == [0x83, 0xF9, 0x0C]),
        "expected `cmp ecx, 12` (MONMODE_DEAD check) after dwMode read"
    );
}
