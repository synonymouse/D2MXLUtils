//! Existing ordered shellcode construction and writes into game memory.

use super::D2Injector;
use crate::offsets::{d2client, d2common, d2lang};
use crate::process::ProcessHandle;

/// Helper to swap endianness for injection code (little-endian)
fn swap_endian(value: u32) -> [u8; 4] {
    value.to_le_bytes()
}

impl D2Injector {
    /// Inject all helper functions into game memory. Identical on every OS
    /// — it only calls `ProcessHandle::write_buffer`, which is already
    /// OS-abstracted; the shellcode bytes themselves are plain x86 machine
    /// code with no Windows/Linux distinction.
    pub(super) fn inject_functions(
        &self,
        process: &ProcessHandle,
        d2_client: usize,
        d2_common: usize,
        d2_lang: usize,
    ) -> Result<(), String> {
        let inject_base = d2_client + d2client::INJECT_BASE;
        let string_addr = self.string_buffer.address as u32;
        let _params_addr = self.params_buffer.address as u32;

        // GetString injection (D2Lang_GetStringById)
        // From D2Stats.au3:2800-2806 — calls D2Lang.dll+0x9450 by absolute address.
        //   D2Client.dll+CDE10 - 8B CB                 - mov ecx,ebx        ; iNameID
        //   D2Client.dll+CDE12 - 31 C0                 - xor eax,eax
        //   D2Client.dll+CDE14 - BB *                  - mov ebx,D2Lang+0x9450
        //   D2Client.dll+CDE19 - FF D3                 - call ebx
        //   D2Client.dll+CDE1B - C3                    - ret
        let get_string_target = (d2_lang + d2lang::GET_STRING_BY_ID) as u32;
        let mut get_string_code: Vec<u8> = vec![0x8B, 0xCB, 0x31, 0xC0, 0xBB];
        get_string_code.extend_from_slice(&swap_endian(get_string_target));
        get_string_code.extend_from_slice(&[0xFF, 0xD3, 0xC3]);
        process.write_buffer(self.inject_get_string, &get_string_code)?;

        // GetItemName injection
        // push 0x100 (max length)
        // push string_addr
        // push ebx (pUnit)
        // call D2Client+0x914F0
        // ret
        let get_name_offset = (d2_client + d2client::func::GET_ITEM_NAME) as i32
            - (inject_base + d2client::inject::GET_ITEM_NAME + 0x10) as i32;
        let mut get_name_code: Vec<u8> = vec![0x68, 0x00, 0x01, 0x00, 0x00, 0x68];
        get_name_code.extend_from_slice(&swap_endian(string_addr));
        get_name_code.push(0x53); // push ebx
        get_name_code.push(0xE8);
        get_name_code.extend_from_slice(&(get_name_offset as u32).to_le_bytes());
        get_name_code.push(0xC3);
        process.write_buffer(self.inject_get_item_name, &get_name_code)?;

        // GetItemStat injection
        // D2Client.dll+CDE40 - 57                    - push edi
        // D2Client.dll+CDE41 - BF *                  - mov edi,D2Client.dll+CDEF0 (string addr)
        // D2Client.dll+CDE43 - 6A 00                 - push 00
        // D2Client.dll+CDE45 - 6A 01                 - push 01
        // D2Client.dll+CDE47 - 53                    - push ebx (pUnit)
        // D2Client.dll+CDE4B - E8 *                  - call D2Client.dll+560B0 (GetItemStat)
        // D2Client.dll+CDE50 - 5F                    - pop edi
        // D2Client.dll+CDE51 - C3                    - ret
        //
        // NOTE: The relative offset must match the original AutoIt injection:
        //   iIDWNTT = (D2Client+0x560B0) - (D2Client+0xCDE4E)
        // So we subtract (inject_base + GET_ITEM_STAT + 0x10), *not* +0x0E.
        let get_stat_offset = (d2_client + d2client::func::GET_ITEM_STAT) as i32
            - (inject_base + d2client::inject::GET_ITEM_STAT + 0x10) as i32;
        let mut get_stat_code: Vec<u8> = vec![0x57, 0xBF];
        get_stat_code.extend_from_slice(&swap_endian(string_addr));
        // push 0, push 1, push ebx (pUnit), call GetItemStatЫФ
        get_stat_code.extend_from_slice(&[0x6A, 0x00, 0x6A, 0x01, 0x53, 0xE8]);
        get_stat_code.extend_from_slice(&(get_stat_offset as u32).to_le_bytes());
        get_stat_code.extend_from_slice(&[0x5F, 0xC3]);
        process.write_buffer(self.inject_get_item_stat, &get_stat_code)?;

        // GetUnitStat injection
        // push 0; push [ebx]; push [ebx+4]; call rel32; mov [str], eax; ret
        // rel32 base is byte after the call's imm32 (offset 0x0C).
        let get_unit_stat_offset = (d2_common + d2common::GET_UNIT_STAT) as i32
            - (inject_base + d2common::INJECT_GET_UNIT_STAT + 0x0C) as i32;
        let mut get_unit_stat_code: Vec<u8> = vec![0x6A, 0x00, 0xFF, 0x33, 0xFF, 0x73, 0x04, 0xE8];
        get_unit_stat_code.extend_from_slice(&(get_unit_stat_offset as u32).to_le_bytes());
        get_unit_stat_code.push(0xA3);
        get_unit_stat_code.extend_from_slice(&swap_endian(string_addr));
        get_unit_stat_code.push(0xC3);
        process.write_buffer(self.inject_get_unit_stat, &get_unit_stat_code)?;

        // NewAutomapCell: `E8 [rel32]; C3` — call + ret. `__fastcall` with
        // no args, so no setup needed. EAX on return = AutomapCell*.
        let new_cell_offset = (d2_client + d2client::func::NEW_AUTOMAP_CELL) as i32
            - (inject_base + d2client::inject::NEW_AUTOMAP_CELL + 5) as i32;
        let mut new_cell_code: Vec<u8> = vec![0xE8];
        new_cell_code.extend_from_slice(&(new_cell_offset as u32).to_le_bytes());
        new_cell_code.push(0xC3);
        process.write_buffer(self.inject_new_automap_cell, &new_cell_code)?;

        Ok(())
    }
}
