//! Programmatically-assembled x86 trampoline + ring-push helper.
//! Lives in injected memory inside the D2 process.
//!
//! See `docs/superpowers/specs/2026-05-07-dps-meter-design.md` Section 3
//! and `docs/dps-meter-reverse-engineering.md` for the full pseudocode
//! this builder emits.

#![cfg(any(target_os = "windows", target_os = "linux"))]

use std::collections::HashMap;

use super::ring::{HEADER_SIZE, SLOT_SIZE};

#[allow(dead_code)]
pub struct TrampolineBlob {
    pub bytes: Vec<u8>,
    /// Offset into `bytes` where the trampoline entry starts.
    pub trampoline_offset: usize,
    /// Offset into `bytes` where the ring-push helper starts.
    pub helper_offset: usize,
    /// Offset into `bytes` where the ring buffer (header + slots) starts.
    pub ring_offset: usize,
}

#[allow(dead_code)]
pub struct BuildParams {
    /// Absolute address of `Ord10887 + 5` (resume point after our hook).
    pub ord10887_resume: usize,
    /// Absolute address where the blob will live in D2's address space.
    pub blob_base: usize,
    /// Capacity of the ring buffer (must be a power of 2; recommend 1024).
    pub ring_capacity: u32,
    /// Absolute address of `D2Client.dll + DIFFICULTY` (u32: 0/1/2).
    pub difficulty_addr: usize,
    /// Absolute address of `KERNEL32.GetTickCount`. Resolved via
    /// `GetProcAddress` at install time.
    pub get_tick_count_addr: usize,
}

#[derive(Clone, Copy)]
enum Rel32Target {
    /// Resume point: `Ord10887 + 5`.
    Resume,
    /// Helper subroutine inside this same blob.
    Helper,
}

/// Emit the trampoline + helper + ring buffer bytecode.
///
/// The trampoline expects to be entered via a 5-byte `E9 rel32` patch at
/// `Ord10887`. Stack on entry:
///   `[esp+0]   = ret_addr_to_caller`
///   `[esp+4]   = pUnit`
///   `[esp+8]   = statId`
///   `[esp+0xC] = value`
///   `[esp+0x10] = layer`
///
/// After `pushfd; pushad`, the args are at `+0x28..+0x34`.
///
/// Slot layout (16 bytes per ring entry):
///   +0x00  u32  ts_ms
///   +0x04  u32  unit_id
///   +0x08  u32  delta_raw  (bit 31 = is_kill, bits 0-30 = delta)
///   +0x0C  u16  max_hp     (template wMaxHP[diff])
///   +0x0E  u16  mLvl       (runtime monster level, stat 12; 0 = not found)
pub fn build(p: &BuildParams) -> TrampolineBlob {
    assert!(
        p.ring_capacity.is_power_of_two(),
        "ring_capacity must be a power of 2"
    );

    let mut buf: Vec<u8> = Vec::with_capacity(0x800);
    let mut labels: HashMap<&'static str, usize> = HashMap::new();
    // Forward short fixups: (offset_of_rel8, target_label).
    let mut short_fixups: Vec<(usize, &'static str)> = Vec::new();
    // Rel32 fixups against external/internal targets.
    let mut rel32_fixups: Vec<(usize, Rel32Target)> = Vec::new();
    // Internal rel32 fixups against named labels — used by long-range
    // intra-blob jumps (e.g. the restore-bridge below) that don't fit
    // in a rel8.
    let mut internal_rel32_fixups: Vec<(usize, &'static str)> = Vec::new();

    // ──────────────────────────────────────────────────────────────────
    //  TRAMPOLINE ENTRY
    // ──────────────────────────────────────────────────────────────────
    let trampoline_offset = buf.len();

    // pushfd; pushad — save all flags + GP regs (4 + 32 = 0x24 bytes)
    buf.extend_from_slice(&[0x9C, 0x60]);

    // Read args (offsets shifted by 0x24 due to pushfd+pushad).
    // mov eax, [esp+0x28]   ; pUnit
    buf.extend_from_slice(&[0x8B, 0x44, 0x24, 0x28]);
    // mov ecx, [esp+0x2C]   ; statId
    buf.extend_from_slice(&[0x8B, 0x4C, 0x24, 0x2C]);
    // mov edx, [esp+0x30]   ; new value
    buf.extend_from_slice(&[0x8B, 0x54, 0x24, 0x30]);

    // Filter: statId == STAT_HITPOINTS (6)
    // cmp ecx, 6
    buf.extend_from_slice(&[0x83, 0xF9, 0x06]);
    emit_short_jcc(&mut buf, &mut short_fixups, 0x75, "restore"); // jne

    // pUnit must be non-null
    buf.extend_from_slice(&[0x85, 0xC0]); // test eax, eax
    emit_short_jcc(&mut buf, &mut short_fixups, 0x74, "restore"); // jz

    // dwType (UnitAny+0x00) == UNIT_MONSTER (1)
    buf.extend_from_slice(&[0x83, 0x38, 0x01]); // cmp dword [eax], 1
    emit_short_jcc(&mut buf, &mut short_fixups, 0x75, "restore"); // jne

    // Drop dying (mode 0) / dead (mode 12) monsters: the engine re-fires
    // HP=0 writes when a corpse comes back into view, which would
    // otherwise register as fresh kills.
    buf.extend_from_slice(&[0x8B, 0x48, 0x10]); // mov ecx, [eax+0x10]   ; dwMode
    buf.extend_from_slice(&[0x85, 0xC9]); // test ecx, ecx          ; MONMODE_DEATH (0)
    emit_short_jcc(&mut buf, &mut short_fixups, 0x74, "restore"); // je
    buf.extend_from_slice(&[0x83, 0xF9, 0x0C]); // cmp ecx, 12           ; MONMODE_DEAD (12)
    emit_short_jcc(&mut buf, &mut short_fixups, 0x74, "restore"); // je

    // pUnitData (UnitAny+0x14)
    buf.extend_from_slice(&[0x8B, 0x70, 0x14]); // mov esi, [eax+0x14]
    buf.extend_from_slice(&[0x85, 0xF6]); // test esi, esi
    emit_short_jcc(&mut buf, &mut short_fixups, 0x74, "restore"); // jz

    // pMonStatsRecord = *pUnitData (deref once)
    buf.extend_from_slice(&[0x8B, 0x36]); // mov esi, [esi]
    buf.extend_from_slice(&[0x85, 0xF6]); // test esi, esi
    emit_short_jcc(&mut buf, &mut short_fixups, 0x74, "restore"); // jz

    // Filter out summons: MonStats.is_spawn (+0x4C) must be 0
    buf.extend_from_slice(&[0x0F, 0xB6, 0x5E, 0x4C]); // movzx ebx, byte [esi+0x4C]
    buf.extend_from_slice(&[0x85, 0xDB]); // test ebx, ebx
    emit_short_jcc(&mut buf, &mut short_fixups, 0x75, "restore"); // jnz

    // Walk pUnit.pStatListEx (+0x5C) → pStat (+0x24), wStatCount1 (+0x28)
    buf.extend_from_slice(&[0x8B, 0x58, 0x5C]); // mov ebx, [eax+0x5C]
    buf.extend_from_slice(&[0x85, 0xDB]); // test ebx, ebx
    emit_short_jcc(&mut buf, &mut short_fixups, 0x74, "restore"); // jz

    buf.extend_from_slice(&[0x8B, 0x7B, 0x24]); // mov edi, [ebx+0x24]   ; pStat
    buf.extend_from_slice(&[0x85, 0xFF]); // test edi, edi
    emit_short_jcc(&mut buf, &mut short_fixups, 0x74, "restore"); // jz

    buf.extend_from_slice(&[0x0F, 0xB7, 0x4B, 0x28]); // movzx ecx, word [ebx+0x28]
    buf.extend_from_slice(&[0x85, 0xC9]); // test ecx, ecx
    emit_short_jcc(&mut buf, &mut short_fixups, 0x74, "restore"); // jz

    // Bail on absurd stat counts — real monsters carry ~30-50, anything
    // near 127 means a corrupted/torn stat list. Without this guard the
    // search loop walks off the array into a guard page → AV → D2 crash.
    buf.extend_from_slice(&[0x83, 0xF9, 0x7F]); // cmp ecx, 0x7F
    emit_short_jcc(&mut buf, &mut short_fixups, 0x77, "restore"); // ja

    // Search loop: find first stat with nStat == 6 (STAT_HITPOINTS).
    buf.extend_from_slice(&[0x33, 0xDB]); // xor ebx, ebx   ; index = 0
    labels.insert("find_hp", buf.len());
    // cmp word [edi + ebx*8 + 2], 6
    buf.extend_from_slice(&[0x66, 0x83, 0x7C, 0xDF, 0x02, 0x06]);
    emit_short_jcc(&mut buf, &mut short_fixups, 0x74, "got_hp"); // je
    buf.push(0x43); // inc ebx
    buf.extend_from_slice(&[0x3B, 0xD9]); // cmp ebx, ecx
    emit_short_jcc(&mut buf, &mut short_fixups, 0x72, "find_hp"); // jb (back-edge)
    emit_short_jmp(&mut buf, &mut short_fixups, "restore"); // not found: bail

    labels.insert("got_hp", buf.len());
    buf.extend_from_slice(&[0x8B, 0x5C, 0xDF, 0x04]); // mov ebx, [edi + ebx*8 + 4]   ; old_hp

    // Damage gate: new < old
    buf.extend_from_slice(&[0x3B, 0xD3]); // cmp edx, ebx
    emit_short_jcc(&mut buf, &mut short_fixups, 0x73, "restore"); // jae
    buf.extend_from_slice(&[0x2B, 0xDA]); // sub ebx, edx   ; ebx = old - new = delta

    // Encode kill flag in bit 31 if new value (edx) was 0.
    buf.extend_from_slice(&[0x85, 0xD2]); // test edx, edx
    emit_short_jcc(&mut buf, &mut short_fixups, 0x75, "not_kill"); // jnz
    buf.extend_from_slice(&[0x81, 0xCB, 0x00, 0x00, 0x00, 0x80]); // or ebx, 0x80000000
    labels.insert("not_kill", buf.len());

    // Restore-bridge: the real epilogue is >127 B away, out of rel8 reach.
    // Filter-bail jcc's target this `restore` label and rel32-jmp through.
    emit_short_jmp(&mut buf, &mut short_fixups, "skip_bridge");
    labels.insert("restore", buf.len());
    buf.push(0xE9); // jmp rel32 → real_restore
    let bridge_jmp_at = buf.len();
    buf.extend_from_slice(&[0, 0, 0, 0]);
    internal_rel32_fixups.push((bridge_jmp_at, "real_restore"));
    labels.insert("skip_bridge", buf.len());

    // Second scan: find stat 12 (LEVEL) → ebp. Used Rust-side as a
    // linear scaling factor; ebp=0 falls back to ×1 on that side.
    // eax scans, pUnit is saved/restored on the stack.
    buf.extend_from_slice(&[0x33, 0xED]); // xor ebp, ebp        ; mLvl = 0 (default)
    buf.push(0x50); // push eax          ; preserve pUnit across scan
    buf.extend_from_slice(&[0x33, 0xC0]); // xor eax, eax        ; scan index
    labels.insert("find_lvl", buf.len());
    // cmp word [edi + eax*8 + 2], 12
    buf.extend_from_slice(&[0x66, 0x83, 0x7C, 0xC7, 0x02, 0x0C]);
    emit_short_jcc(&mut buf, &mut short_fixups, 0x75, "next_lvl"); // jne
    buf.extend_from_slice(&[0x8B, 0x6C, 0xC7, 0x04]); // mov ebp, [edi + eax*8 + 4]
    emit_short_jmp(&mut buf, &mut short_fixups, "lvl_done");
    labels.insert("next_lvl", buf.len());
    buf.push(0x40); // inc eax
    buf.extend_from_slice(&[0x3B, 0xC1]); // cmp eax, ecx
    emit_short_jcc(&mut buf, &mut short_fixups, 0x72, "find_lvl"); // jb
    labels.insert("lvl_done", buf.len());
    buf.push(0x58); // pop eax           ; restore pUnit

    // Push event via helper(pUnit, pMonStats, delta, mLvl).
    // Args pushed right-to-left → on stack:
    //   [esp]=pUnit, [esp+4]=pMonStats, [esp+8]=delta, [esp+0xC]=mLvl.
    buf.push(0x55); // push ebp          ; mLvl
    buf.push(0x53); // push ebx          ; delta_raw_with_kill_flag
    buf.push(0x56); // push esi          ; pMonStatsRecord
    buf.push(0x50); // push eax          ; pUnit
    buf.push(0xE8); // call rel32
    let helper_call_at = buf.len();
    buf.extend_from_slice(&[0, 0, 0, 0]);
    rel32_fixups.push((helper_call_at, Rel32Target::Helper));
    buf.extend_from_slice(&[0x83, 0xC4, 0x10]); // add esp, 0x10  (4 args × 4 bytes, cdecl)

    // .real_restore: pop, run the saved 5 bytes, jmp back to Ord10887+5.
    // Reachable via the bridge (filter bails) or fall-through (normal path).
    labels.insert("real_restore", buf.len());
    buf.push(0x61); // popad
    buf.push(0x9D); // popfd
                    // Saved 5 bytes from Ord10887: 8B 44 24 0C 53
    buf.extend_from_slice(&[0x8B, 0x44, 0x24, 0x0C, 0x53]);
    buf.push(0xE9); // jmp rel32 → Ord10887+5
    let resume_jmp_at = buf.len();
    buf.extend_from_slice(&[0, 0, 0, 0]);
    rel32_fixups.push((resume_jmp_at, Rel32Target::Resume));

    // Pad to 16-byte boundary so the helper starts aligned.
    while buf.len() % 16 != 0 {
        buf.push(0x90); // nop
    }

    // ──────────────────────────────────────────────────────────────────
    //  HELPER: ring_push(pUnit, pMonStatsRecord, delta_raw, mLvl)
    //  __cdecl, caller cleans 16 bytes of args. Trashes eax/ecx/edx;
    //  preserves ebx/esi/edi/ebp via prologue.
    //  mLvl is packed into the high 16 bits of the slot's max_hp dword
    //  (low 16 bits stay max_hp). Rust ring reader unpacks both.
    // ──────────────────────────────────────────────────────────────────
    let helper_offset = buf.len();

    // Prologue
    buf.push(0x55); // push ebp
    buf.extend_from_slice(&[0x8B, 0xEC]); // mov ebp, esp
    buf.push(0x53); // push ebx
    buf.push(0x56); // push esi
    buf.push(0x57); // push edi
    buf.extend_from_slice(&[0x83, 0xEC, 0x10]); // sub esp, 0x10   ; locals

    // Read difficulty (u32 → 0/1/2), compute offset 0xB0 + diff*2 in ecx.
    buf.extend_from_slice(&[0x8B, 0x0D]); // mov ecx, [imm32]
    buf.extend_from_slice(&(p.difficulty_addr as u32).to_le_bytes());
    buf.extend_from_slice(&[0xD1, 0xE1]); // shl ecx, 1
                                          // add ecx, 0xB0   (use imm32 form because 0xB0 sign-extends to negative as imm8)
    buf.extend_from_slice(&[0x81, 0xC1, 0xB0, 0x00, 0x00, 0x00]);

    // pMonStatsRecord → esi; max_hp = u16 [esi + ecx], stashed at [ebp-0x10].
    // NB: [ebp-4] holds the saved ebx — using it would corrupt `pop ebx`
    // in the epilogue.
    buf.extend_from_slice(&[0x8B, 0x75, 0x0C]); // mov esi, [ebp+0xC]
    buf.extend_from_slice(&[0x0F, 0xB7, 0x04, 0x0E]); // movzx eax, word [esi + ecx]
    buf.extend_from_slice(&[0x89, 0x45, 0xF0]); // mov [ebp-0x10], eax   ; save max_hp

    // pUnit -> esi; unit_id = [esi + 0x0C]
    buf.extend_from_slice(&[0x8B, 0x75, 0x08]); // mov esi, [ebp+8]
    buf.extend_from_slice(&[0x8B, 0x7E, 0x0C]); // mov edi, [esi+0x0C]   ; unit_id

    // ts_ms = GetTickCount(). __stdcall 0 args → trashes eax/ecx/edx.
    buf.push(0xB8); // mov eax, imm32
    buf.extend_from_slice(&(p.get_tick_count_addr as u32).to_le_bytes());
    buf.extend_from_slice(&[0xFF, 0xD0]); // call eax

    // Atomic ring push.
    // Layout: ring_addr → header (head, tail, capacity, pad), then slots[capacity].
    let ring_addr_imm_at = buf.len() + 1; // patch site for `mov ecx, ring_addr`
    buf.push(0xB9); // mov ecx, imm32
    buf.extend_from_slice(&[0, 0, 0, 0]); // patched after layout known

    buf.extend_from_slice(&[0x8B, 0x59, 0x08]); // mov ebx, [ecx+0x08]   ; capacity
    buf.extend_from_slice(&[0x83, 0xEB, 0x01]); // sub ebx, 1            ; mask

    // edx = 1; lock xadd [ecx], edx → edx = old head
    buf.extend_from_slice(&[0xBA, 0x01, 0x00, 0x00, 0x00]); // mov edx, 1
    buf.extend_from_slice(&[0xF0, 0x0F, 0xC1, 0x11]); // lock xadd [ecx], edx

    buf.extend_from_slice(&[0x23, 0xD3]); // and edx, ebx   ; slot index
    buf.extend_from_slice(&[0xC1, 0xE2, 0x04]); // shl edx, 4     ; * 16
    buf.extend_from_slice(&[0x03, 0xD1]); // add edx, ecx
    buf.extend_from_slice(&[0x83, 0xC2, 0x10]); // add edx, 0x10  ; skip header

    // Write slot fields
    buf.extend_from_slice(&[0x89, 0x02]); // mov [edx+0], eax   ; ts_ms
    buf.extend_from_slice(&[0x89, 0x7A, 0x04]); // mov [edx+4], edi   ; unit_id
    buf.extend_from_slice(&[0x8B, 0x75, 0x10]); // mov esi, [ebp+0x10] ; delta_raw
    buf.extend_from_slice(&[0x89, 0x72, 0x08]); // mov [edx+8], esi   ; delta_raw
                                                // Pack max_hp (u16 low) | (mLvl u16 high) into a single dword write.
    buf.extend_from_slice(&[0x8B, 0x75, 0xF0]); // mov esi, [ebp-0x10] ; max_hp (low 16)
    buf.extend_from_slice(&[0x8B, 0x7D, 0x14]); // mov edi, [ebp+0x14] ; mLvl (4th arg)
    buf.extend_from_slice(&[0xC1, 0xE7, 0x10]); // shl edi, 0x10       ; mLvl << 16
    buf.extend_from_slice(&[0x0B, 0xF7]); // or  esi, edi        ; combined
    buf.extend_from_slice(&[0x89, 0x72, 0x0C]); // mov [edx+0xC], esi  ; max_hp | (mLvl<<16)

    // Epilogue
    buf.extend_from_slice(&[0x83, 0xC4, 0x10]); // add esp, 0x10
    buf.push(0x5F); // pop edi
    buf.push(0x5E); // pop esi
    buf.push(0x5B); // pop ebx
    buf.push(0x5D); // pop ebp
    buf.push(0xC3); // ret

    // Pad to 16-byte boundary before ring buffer.
    while buf.len() % 16 != 0 {
        buf.push(0x90);
    }

    // ──────────────────────────────────────────────────────────────────
    //  RING BUFFER
    // ──────────────────────────────────────────────────────────────────
    let ring_offset = buf.len();
    let ring_addr = p.blob_base + ring_offset;

    // Header: head=0, tail=0, capacity, pad
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&p.ring_capacity.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    debug_assert_eq!(buf.len() - ring_offset, HEADER_SIZE);

    // Slots — capacity * 16 bytes, all zero.
    let slots_bytes = (p.ring_capacity as usize) * SLOT_SIZE;
    buf.resize(buf.len() + slots_bytes, 0);

    // ──────────────────────────────────────────────────────────────────
    //  Patch the helper's ring-address load.
    // ──────────────────────────────────────────────────────────────────
    buf[ring_addr_imm_at..ring_addr_imm_at + 4].copy_from_slice(&(ring_addr as u32).to_le_bytes());

    // ──────────────────────────────────────────────────────────────────
    //  Resolve fixups
    // ──────────────────────────────────────────────────────────────────
    for (at, label) in &short_fixups {
        let target = labels[label];
        let from = at + 1; // EIP after the rel8 byte
        let rel = (target as isize) - (from as isize);
        assert!(
            (-128..=127).contains(&rel),
            "short jump out of range to '{}' (rel={})",
            label,
            rel
        );
        buf[*at] = (rel as i8) as u8;
    }

    for (at, target) in &rel32_fixups {
        let abs_target = match target {
            Rel32Target::Resume => p.ord10887_resume,
            Rel32Target::Helper => p.blob_base + helper_offset,
        };
        let from = p.blob_base + at + 4; // EIP after the rel32 dword
        let rel: i32 = (abs_target as i64 - from as i64) as i32;
        buf[*at..*at + 4].copy_from_slice(&rel.to_le_bytes());
    }

    for (at, label) in &internal_rel32_fixups {
        let target = labels[label];
        let from = at + 4; // EIP after the rel32 dword (intra-blob)
        let rel: i32 = (target as isize - from as isize) as i32;
        buf[*at..*at + 4].copy_from_slice(&rel.to_le_bytes());
    }

    TrampolineBlob {
        bytes: buf,
        trampoline_offset,
        helper_offset,
        ring_offset,
    }
}

fn emit_short_jcc(
    buf: &mut Vec<u8>,
    fixups: &mut Vec<(usize, &'static str)>,
    opcode: u8,
    label: &'static str,
) {
    buf.push(opcode);
    buf.push(0); // placeholder
    fixups.push((buf.len() - 1, label));
}

fn emit_short_jmp(buf: &mut Vec<u8>, fixups: &mut Vec<(usize, &'static str)>, label: &'static str) {
    emit_short_jcc(buf, fixups, 0xEB, label);
}

#[cfg(test)]
mod tests {
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
}
