//! Game-call wrappers sharing the existing injector scratch buffers.

use super::{remote_thread, D2Injector};
use crate::process::ProcessHandle;
use crate::stat_telemetry::InjectorCall;

/// Public call wrappers — identical on every OS, since they only go through
/// the already OS-abstracted `remote_thread`/`ProcessHandle::read_buffer`/
/// `write_buffer`. Shared by both the Windows and Linux `D2Injector`, which
/// have the same field shape (`string_buffer`/`params_buffer: RemoteAlloc`,
/// `inject_*: usize`) even though allocation/construction differ per OS.
impl D2Injector {
    /// Get item name by calling the injected function
    pub fn get_item_name(&self, process: &ProcessHandle, p_unit: u32) -> Result<String, String> {
        self.telemetry.injector_attempt(InjectorCall::GetItemName);
        // Clear buffer before use
        // Original D2Stats reads wchar[256] → 512 bytes
        let zeros = vec![0u8; 512];
        process.write_buffer(self.string_buffer.address, &zeros)?;

        // Call GetItemName with pUnit in EBX
        remote_thread(process, self.inject_get_item_name, p_unit as usize)?;

        // Read the result string (wide char)
        let buffer = process.read_buffer(self.string_buffer.address, 512)?;

        // Convert from UTF-16LE to String
        let wide: Vec<u16> = buffer
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .take_while(|&c| c != 0)
            .collect();

        Ok(String::from_utf16_lossy(&wide))
    }

    /// Get item stats by calling the injected function
    pub fn get_item_stats(&self, process: &ProcessHandle, p_unit: u32) -> Result<String, String> {
        self.telemetry.injector_attempt(InjectorCall::GetItemStats);
        // Clear buffer before use
        // Original D2Stats reads wchar[2048] → 4096 bytes
        let zeros = vec![0u8; 4096];
        process.write_buffer(self.string_buffer.address, &zeros)?;

        // Call GetItemStats with pUnit in EBX
        remote_thread(process, self.inject_get_item_stat, p_unit as usize)?;

        // Read the result string
        let buffer = process.read_buffer(self.string_buffer.address, 4096)?;

        // Convert from UTF-16LE to String
        let wide: Vec<u16> = buffer
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .take_while(|&c| c != 0)
            .collect();

        Ok(String::from_utf16_lossy(&wide))
    }

    /// Resolve a string-table ID to a wide string by calling D2Lang_GetStringById.
    /// Used to read base item names from items.txt during tier-cache construction.
    ///
    /// Returns at most `max_chars` wide characters (stops at the first NUL).
    pub fn get_string(
        &self,
        process: &ProcessHandle,
        name_id: u16,
        max_chars: usize,
    ) -> Result<String, String> {
        self.telemetry.injector_attempt(InjectorCall::GetString);
        // remote_thread returns the thread's exit code which is EAX from our
        // shellcode — for GetStringById this is a pointer into the game's
        // string table (UTF-16).
        let str_ptr = remote_thread(process, self.inject_get_string, name_id as usize)? as usize;
        if str_ptr == 0 {
            return Ok(String::new());
        }

        let byte_len = max_chars.saturating_mul(2);
        let buffer = process.read_buffer(str_ptr, byte_len)?;
        let wide: Vec<u16> = buffer
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .take_while(|&c| c != 0)
            .collect();

        Ok(String::from_utf16_lossy(&wide))
    }

    /// Allocate a fresh `AutomapCell` from the game's pool. Caller fills
    /// the fields; the engine reclaims the cell on area change.
    pub fn new_automap_cell(&self, process: &ProcessHandle) -> Result<u32, String> {
        self.telemetry
            .injector_attempt(InjectorCall::NewAutomapCell);
        #[cfg(all(test, target_os = "windows"))]
        if let Some(allocator) = &self.marker_allocator {
            return allocator.lock().unwrap().allocate();
        }
        let cell = remote_thread(process, self.inject_new_automap_cell, 0)?;
        Ok(cell)
    }

    /// Get a unit stat value
    pub fn get_unit_stat(
        &self,
        process: &ProcessHandle,
        p_unit: u32,
        stat_id: u32,
    ) -> Result<u32, String> {
        self.telemetry.injector_attempt(InjectorCall::GetUnitStat);
        // Write params: [stat_id, p_unit]
        process.write_buffer(self.params_buffer.address, &stat_id.to_le_bytes())?;
        process.write_buffer(self.params_buffer.address + 4, &p_unit.to_le_bytes())?;

        // Call GetUnitStat with params pointer in EBX
        remote_thread(
            process,
            self.inject_get_unit_stat,
            self.params_buffer.address,
        )?;

        // Read result from string buffer
        process.read_memory::<u32>(self.string_buffer.address)
    }
}
