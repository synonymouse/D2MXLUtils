use super::*;

#[test]
fn extracts_last_non_empty_line() {
    let raw = "Crystal Sword\n\nAzurewrath";
    assert_eq!(
        display_name_from_raw_item_name(raw),
        Some("Azurewrath".to_string())
    );
}

#[test]
fn strips_diablo_color_codes() {
    let raw = "\u{00ff}c4Sacred Set\n\u{00ff}c2Witchhunter's Ire";
    assert_eq!(
        display_name_from_raw_item_name(raw),
        Some("Witchhunter's Ire".to_string())
    );
}

#[test]
fn blank_name_returns_none() {
    assert_eq!(display_name_from_raw_item_name("\n  \n"), None);
}

#[test]
fn freshness_accepts_recent_ticks() {
    assert!(is_fresh(1_750, 1_000, 750));
}

#[test]
fn freshness_rejects_stale_ticks() {
    assert!(!is_fresh(1_751, 1_000, 750));
}

#[test]
fn freshness_handles_get_tick_count_wrap() {
    assert!(is_fresh(20, u32::MAX - 10, 40));
}

#[cfg(target_os = "windows")]
#[test]
fn stable_snapshot_accepts_matching_even_sequence() {
    let snapshot = HoveredItemSnapshot {
        p_unit: 0x1234_5678,
        last_seen_ms: 42,
        sequence: 2,
        stack_args: [0x1234_5678, 0x2000_0000, 0x3000_0000, 0x4000_0000, 0, 0],
        saved_regs: [0x11, 0x22, 0x33, 0x44, 0x55, 0x66],
    };

    assert_eq!(
        stable_snapshot_from_reads(snapshot, snapshot),
        Some(snapshot)
    );
}

#[cfg(target_os = "windows")]
#[test]
fn stable_snapshot_rejects_changed_or_odd_sequence() {
    let first = HoveredItemSnapshot {
        p_unit: 0x1234_5678,
        last_seen_ms: 42,
        sequence: 2,
        stack_args: [0x1234_5678, 0, 0, 0, 0, 0],
        saved_regs: [0, 0, 0, 0, 0, 0],
    };
    let changed = HoveredItemSnapshot {
        sequence: 4,
        ..first
    };
    let odd = HoveredItemSnapshot {
        sequence: 3,
        ..first
    };

    assert_eq!(stable_snapshot_from_reads(first, changed), None);
    assert_eq!(stable_snapshot_from_reads(odd, odd), None);
}

#[cfg(target_os = "windows")]
#[test]
fn stable_snapshot_retries_until_matching_even_sequence() {
    let first = HoveredItemSnapshot {
        p_unit: 0x1234_5678,
        last_seen_ms: 42,
        sequence: 2,
        stack_args: [0x1234_5678, 0, 0, 0, 0, 0],
        saved_regs: [0, 0, 0, 0, 0, 0],
    };
    let stable = HoveredItemSnapshot {
        p_unit: 0x8765_4321,
        last_seen_ms: 43,
        sequence: 4,
        stack_args: [0x8765_4321, 0, 0, 0, 0, 0],
        saved_regs: [0, 0, 0, 0, 0, 0],
    };

    assert_eq!(
        stable_snapshot_from_read_sequence(&[first, stable, stable]),
        Some(stable)
    );
}

#[cfg(target_os = "windows")]
#[test]
fn protected_write_restores_remote_page_protection() {
    use windows::Win32::System::Memory::{PAGE_NOACCESS, PAGE_READWRITE};
    use windows::Win32::System::Threading::GetCurrentProcess;

    let process = unsafe { GetCurrentProcess() };
    let ptr = unsafe {
        VirtualAllocEx(
            process,
            None,
            0x1000,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        )
    };
    assert!(!ptr.is_null());

    let addr = ptr as usize;
    crate::remote_io::write_remote(process, addr, &[0x11, 0x22, 0x33, 0x44, 0x55]).unwrap();
    let mut old_protect = PAGE_PROTECTION_FLAGS(0);
    unsafe {
        VirtualProtectEx(process, ptr, 0x1000, PAGE_NOACCESS, &mut old_protect).unwrap();
    }

    write_remote_with_page_protection(process, addr, &[0xAA, 0xBB, 0xCC, 0xDD, 0xEE]).unwrap();

    let mut restored_protect = PAGE_PROTECTION_FLAGS(0);
    unsafe {
        VirtualProtectEx(process, ptr, 0x1000, PAGE_READWRITE, &mut restored_protect).unwrap();
    }
    assert_eq!(restored_protect, PAGE_NOACCESS);

    let mut bytes = [0u8; 5];
    crate::remote_io::read_remote(process, addr, &mut bytes).unwrap();
    assert_eq!(bytes, [0xAA, 0xBB, 0xCC, 0xDD, 0xEE]);

    unsafe {
        let _ = VirtualFreeEx(process, ptr, 0, MEM_RELEASE);
    }
}

#[test]
fn hovered_item_location_accepts_only_player_inventory_scopes() {
    let player_inventory = 0x2000;
    assert!(valid_hovered_item_location(
        player_inventory,
        player_inventory,
        3,
        0
    ));
    assert!(valid_hovered_item_location(
        player_inventory,
        player_inventory,
        6,
        0
    ));
    assert!(valid_hovered_item_location(
        player_inventory,
        player_inventory,
        7,
        0
    ));
    assert!(valid_hovered_item_location(
        player_inventory,
        player_inventory,
        0,
        4
    ));

    assert!(!valid_hovered_item_location(0x3000, player_inventory, 3, 0));
    assert!(!valid_hovered_item_location(
        player_inventory,
        player_inventory,
        0,
        0
    ));
    assert!(!valid_hovered_item_location(
        player_inventory,
        player_inventory,
        8,
        0
    ));
}

#[cfg(target_os = "windows")]
#[test]
fn tooltip_trampoline_reads_item_arg_after_saved_registers() {
    let blob = build_tooltip_hook_blob(0x1000_0000, 0x2000_0000, 0x3000_0000);
    let body = &blob.bytes[..blob.fields_offset];
    let expected = [
        0x8B,
        0x44,
        0x24,
        crate::offsets::d2sigma::TOOLTIP_ITEM_ARG_AFTER_PUSHFD_PUSHAD as u8,
    ];

    assert!(body.windows(expected.len()).any(|w| w == expected));
}

#[cfg(target_os = "windows")]
#[test]
fn tooltip_trampoline_writes_shared_fields() {
    let base = 0x1000_0000;
    let blob = build_tooltip_hook_blob(base, 0x2000_0000, 0x3000_0000);
    let fields = blob.fields_addr(base);
    let body = &blob.bytes[..blob.fields_offset];

    assert!(body.windows(5).any(|w| {
        w[0] == 0xA3 && u32::from_le_bytes(w[1..5].try_into().unwrap()) as usize == fields
    }));
    assert!(body.windows(6).any(|w| {
        w[0] == 0xFF
            && w[1] == 0x05
            && u32::from_le_bytes(w[2..6].try_into().unwrap()) as usize == fields + 8
    }));
}

#[cfg(target_os = "windows")]
#[test]
fn tooltip_trampoline_writes_diagnostic_stack_args() {
    let base = 0x1000_0000;
    let blob = build_tooltip_hook_blob(base, 0x2000_0000, 0x3000_0000);
    let fields = blob.fields_addr(base);
    let body = &blob.bytes[..blob.fields_offset];
    let expected_load_arg1 = [
        0x8B,
        0x44,
        0x24,
        (crate::offsets::d2sigma::TOOLTIP_ITEM_ARG_AFTER_PUSHFD_PUSHAD + 4) as u8,
    ];
    let expected_store_arg1 = (fields + 12).to_le_bytes();

    let load_pos = body
        .windows(expected_load_arg1.len())
        .position(|w| w == expected_load_arg1)
        .expect("expected load of second stack arg");
    let store = &body[load_pos + expected_load_arg1.len()..load_pos + expected_load_arg1.len() + 5];

    assert_eq!(store[0], 0xA3);
    assert_eq!(&store[1..5], &expected_store_arg1[..4]);
}

#[cfg(target_os = "windows")]
#[test]
fn tooltip_trampoline_writes_saved_register_diagnostics() {
    let base = 0x1000_0000;
    let blob = build_tooltip_hook_blob(base, 0x2000_0000, 0x3000_0000);
    let fields = blob.fields_addr(base);
    let body = &blob.bytes[..blob.fields_offset];
    let expected_load_ecx = [0x8B, 0x44, 0x24, 0x18];
    let expected_store_ecx = (fields + 36).to_le_bytes();

    let load_pos = body
        .windows(expected_load_ecx.len())
        .position(|w| w == expected_load_ecx)
        .expect("expected load of saved ECX");
    let store = &body[load_pos + expected_load_ecx.len()..load_pos + expected_load_ecx.len() + 5];

    assert_eq!(store[0], 0xA3);
    assert_eq!(&store[1..5], &expected_store_ecx[..4]);
}

#[cfg(target_os = "windows")]
#[test]
fn hovered_item_candidates_try_second_stack_arg_after_first_arg() {
    let snapshot = HoveredItemSnapshot {
        p_unit: 0x0000_01BE,
        last_seen_ms: 7,
        sequence: 8,
        stack_args: [0x0000_01BE, 0x28FA_CB00, 0x28E0_A200, 0, 0, 0],
        saved_regs: [0, 0, 0, 0, 0x0000_01BE, 0],
    };
    let candidates = hovered_item_candidates(snapshot);

    assert_eq!(candidates[0], ("arg0", 0x0000_01BE));
    assert_eq!(candidates[1], ("arg1", 0x28FA_CB00));
    assert!(candidates.contains(&("arg2", 0x28E0_A200)));
}

#[cfg(target_os = "windows")]
#[test]
fn hovered_item_candidates_drop_zero_and_duplicate_values() {
    let snapshot = HoveredItemSnapshot {
        p_unit: 0x28E0_0200,
        last_seen_ms: 7,
        sequence: 8,
        stack_args: [0x28E0_0200, 0, 0x28E0_0200, 0, 0, 0],
        saved_regs: [0, 0x28E0_0200, 0x28E0_A200, 0, 0, 0],
    };
    let candidates = hovered_item_candidates(snapshot);

    assert_eq!(
        candidates,
        vec![("arg0", 0x28E0_0200), ("edx", 0x28E0_A200)]
    );
}

#[cfg(target_os = "windows")]
#[test]
fn tooltip_trampoline_marks_sequence_odd_before_writing_snapshot() {
    let base = 0x1000_0000;
    let blob = build_tooltip_hook_blob(base, 0x2000_0000, 0x3000_0000);
    let fields = blob.fields_addr(base);
    let body = &blob.bytes[..blob.fields_offset];
    let first_seq_inc = body
        .windows(6)
        .position(|w| {
            w[0] == 0xFF
                && w[1] == 0x05
                && u32::from_le_bytes(w[2..6].try_into().unwrap()) as usize == fields + 8
        })
        .expect("expected sequence increment");
    let p_unit_write = body
        .windows(5)
        .position(|w| {
            w[0] == 0xA3 && u32::from_le_bytes(w[1..5].try_into().unwrap()) as usize == fields
        })
        .expect("expected pUnit write");

    assert!(first_seq_inc < p_unit_write);
}

#[cfg(target_os = "windows")]
#[test]
fn tooltip_trampoline_replays_prologue_and_jumps_to_resume() {
    let base = 0x1000_0000;
    let resume = 0x2000_0000;
    let blob = build_tooltip_hook_blob(base, resume, 0x3000_0000);
    let body = &blob.bytes[..blob.fields_offset];
    let prologue = crate::offsets::d2sigma::TOOLTIP_ITEM_HOOK_PROLOGUE;
    let pos = body
        .windows(prologue.len())
        .position(|w| w == prologue)
        .expect("expected replayed tooltip hook prologue");
    let jmp = &body[pos + prologue.len()..pos + prologue.len() + 5];
    let rel = i32::from_le_bytes(jmp[1..5].try_into().unwrap());
    let from = base + pos + prologue.len() + 5;

    assert_eq!(jmp[0], 0xE9);
    assert_eq!((from as i64 + rel as i64) as usize, resume);
}

#[test]
fn hovered_item_unit_shape_accepts_valid_inventory_item() {
    assert!(valid_hovered_item_unit(
        crate::offsets::unit_type::ITEM,
        0x1234,
        4,
        0x2500_0000,
    ));
}

#[test]
fn hovered_item_unit_shape_rejects_ground_or_invalid_units() {
    assert!(!valid_hovered_item_unit(1, 0x1234, 4, 0x2500_0000));
    assert!(!valid_hovered_item_unit(
        crate::offsets::unit_type::ITEM,
        0x1_0000,
        4,
        0x2500_0000,
    ));
    assert!(!valid_hovered_item_unit(
        crate::offsets::unit_type::ITEM,
        0x1234,
        3,
        0x2500_0000,
    ));
    assert!(!valid_hovered_item_unit(
        crate::offsets::unit_type::ITEM,
        0x1234,
        4,
        0,
    ));
}
