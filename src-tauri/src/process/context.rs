#[cfg(any(target_os = "windows", target_os = "linux"))]
use super::{open_process_by_window_class, ProcessHandle};

/// AOB anchoring on the lazy-init body of the always-show-items getter
/// `D2Sigma+0x57470`. The 4 bytes after the leading `A1` are the absolute
/// VA of the cached struct pointer.
///
/// ```text
/// A1 ?? ?? ?? ?? 85 C0 75 ?? 56 68 D0 00 00 00 E8 ?? ?? ?? ?? 8B F0
/// ```
#[cfg(any(target_os = "windows", target_os = "linux"))]
const ALWAYS_SHOW_ITEMS_GETTER_PATTERN: &[Option<u8>] = &[
    Some(0xA1),
    None,
    None,
    None,
    None,
    Some(0x85),
    Some(0xC0),
    Some(0x75),
    None,
    Some(0x56),
    Some(0x68),
    Some(0xD0),
    Some(0x00),
    Some(0x00),
    Some(0x00),
    Some(0xE8),
    None,
    None,
    None,
    None,
    Some(0x8B),
    Some(0xF0),
];

/// `None` on no match, ambiguous match (>1 hit = signature too loose to trust),
/// or out-of-module decoded address.
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn resolve_always_show_items_ptr_rva(
    process: &ProcessHandle,
    base: usize,
    size: usize,
) -> Option<usize> {
    let first =
        process.scan_pattern_wildcard(base, size, ALWAYS_SHOW_ITEMS_GETTER_PATTERN, base)?;
    if process
        .scan_pattern_wildcard(base, size, ALWAYS_SHOW_ITEMS_GETTER_PATTERN, first + 1)
        .is_some()
    {
        return None;
    }
    let abs_va = process.read_memory::<u32>(first + 1).ok()? as usize;
    if abs_va < base || abs_va >= base.saturating_add(size) {
        return None;
    }
    Some(abs_va - base)
}

#[cfg(target_os = "windows")]
pub struct D2Context {
    pub process: ProcessHandle,
    pub d2_client: usize,
    pub d2_common: usize,
    pub d2_win: usize,
    pub d2_lang: usize,
    pub d2_sigma: usize,
    /// `SizeOfImage` of `D2Sigma.dll`, or 0 if not loaded. Use as the upper
    /// bound for AOB scans over the module.
    pub d2_sigma_size: usize,
    /// `None` if the AOB signature didn't resolve — feature unavailable.
    pub always_show_items_ptr_rva: Option<usize>,
}

#[cfg(target_os = "windows")]
impl D2Context {
    pub fn new() -> Result<Self, String> {
        let process = open_process_by_window_class("Diablo II")?;
        let d2_client = process.get_module_base("D2Client.dll")?;
        let d2_common = process.get_module_base("D2Common.dll")?;
        let d2_win = process.get_module_base("D2Win.dll")?;
        let d2_lang = process.get_module_base("D2Lang.dll")?;
        let (d2_sigma, d2_sigma_size) = process.get_module_info("D2Sigma.dll").unwrap_or((0, 0));

        let always_show_items_ptr_rva = if d2_sigma != 0 && d2_sigma_size != 0 {
            let rva = resolve_always_show_items_ptr_rva(&process, d2_sigma, d2_sigma_size);
            match rva {
                Some(rva) => crate::logger::info(&format!(
                    "Resolved always-show-items static at D2Sigma+{:#x}",
                    rva
                )),
                None => crate::logger::error(
                    "always-show-items: AOB signature did not resolve in D2Sigma.dll",
                ),
            }
            rva
        } else {
            None
        };

        Ok(Self {
            process,
            d2_client,
            d2_common,
            d2_win,
            d2_lang,
            d2_sigma,
            d2_sigma_size,
            always_show_items_ptr_rva,
        })
    }
}

#[cfg(target_os = "linux")]
pub struct D2Context {
    pub process: ProcessHandle,
    pub d2_client: usize,
    pub d2_common: usize,
    pub d2_win: usize,
    pub d2_lang: usize,
    pub d2_sigma: usize,
    pub d2_sigma_size: usize,
    pub always_show_items_ptr_rva: Option<usize>,
}

#[cfg(target_os = "linux")]
impl D2Context {
    pub fn new() -> Result<Self, String> {
        let process = open_process_by_window_class(super::LINUX_WINDOW_TITLE)?;
        let d2_client = process.get_module_base("D2Client.dll")?;
        let d2_common = process.get_module_base("D2Common.dll")?;
        let d2_win = process.get_module_base("D2Win.dll")?;
        let d2_lang = process.get_module_base("D2Lang.dll")?;
        let (d2_sigma, d2_sigma_size) = process.get_module_info("D2Sigma.dll").unwrap_or((0, 0));

        let always_show_items_ptr_rva = if d2_sigma != 0 && d2_sigma_size != 0 {
            resolve_always_show_items_ptr_rva(&process, d2_sigma, d2_sigma_size)
        } else {
            None
        };

        Ok(Self {
            process,
            d2_client,
            d2_common,
            d2_win,
            d2_lang,
            d2_sigma,
            d2_sigma_size,
            always_show_items_ptr_rva,
        })
    }
}

// --- Stub for other OSes (compilation only) ---

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub struct D2Context {
    pub d2_client: usize,
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
impl D2Context {
    pub fn new() -> Result<Self, String> {
        Err("Not supported on this OS".to_string())
    }
}
