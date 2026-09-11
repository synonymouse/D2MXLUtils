use super::*;
use std::ffi::c_void;
use std::ptr::NonNull;
use windows::Win32::System::Memory::{
    VirtualAlloc, VirtualFree, VirtualProtect, MEM_COMMIT, MEM_DECOMMIT, MEM_RELEASE, MEM_RESERVE,
    PAGE_NOACCESS, PAGE_READWRITE,
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_VM_READ};

struct FixturePage(NonNull<c_void>);

impl FixturePage {
    fn new() -> Self {
        for address in (0x10000000usize..0x70000000).step_by(0x10000) {
            // SAFETY: FFI boundary; a hint is not dereferenced, and allocation failure is checked.
            let page = unsafe {
                VirtualAlloc(
                    Some(std::ptr::without_provenance(address)),
                    4096,
                    MEM_RESERVE | MEM_COMMIT,
                    PAGE_READWRITE,
                )
            };
            if let Some(pointer) = NonNull::new(page) {
                return Self(pointer);
            }
        }
        panic!("no low-address fixture page available");
    }

    fn unit(&self) -> u32 {
        u32::try_from(self.0.as_ptr().addr()).unwrap()
    }

    fn initialize(&mut self) {
        let unit = self.unit();
        let mut bytes = [0u8; 4096];
        bytes[0x5c..0x60].copy_from_slice(&(unit + 0x100).to_le_bytes());
        bytes[0x124..0x128].copy_from_slice(&(unit + 0x200).to_le_bytes());
        bytes[0x128..0x12a].copy_from_slice(&2i16.to_le_bytes());
        bytes[0x202..0x204].copy_from_slice(&12u16.to_le_bytes());
        bytes[0x204..0x208].copy_from_slice(&(-256i32).to_le_bytes());
        bytes[0x20a..0x20c].copy_from_slice(&93u16.to_le_bytes());
        // SAFETY: bounds/aliasing; this uniquely owned writable page has 4096 bytes and does not overlap the stack array.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), self.0.as_ptr().cast::<u8>(), bytes.len())
        };
    }

    fn make_inaccessible(&mut self) {
        let mut previous = PAGE_READWRITE;
        // SAFETY: FFI boundary; this page is live and owned, and the output pointer is valid.
        unsafe { VirtualProtect(self.0.as_ptr(), 4096, PAGE_NOACCESS, &mut previous) }.unwrap();
    }

    fn decommit(&mut self) {
        // SAFETY: ownership; decommit frees backing memory but retains our reservation until Drop.
        unsafe { VirtualFree(self.0.as_ptr(), 4096, MEM_DECOMMIT) }.unwrap();
    }
}

impl Drop for FixturePage {
    fn drop(&mut self) {
        // SAFETY: ownership; exactly this allocation base is released once, including decommitted pages.
        let result = unsafe { VirtualFree(self.0.as_ptr(), 0, MEM_RELEASE) };
        assert!(result.is_ok(), "fixture release failed: {result:?}");
    }
}

fn own_process() -> ProcessHandle {
    let pid = std::process::id();
    // SAFETY: FFI boundary; opens the current process with read-only rights and checks failure.
    let handle = unsafe { OpenProcess(PROCESS_VM_READ, false, pid) }.unwrap();
    ProcessHandle { handle, pid }
}

#[test]
fn own_process_bulk_reads_owned_low_page() {
    let mut page = FixturePage::new();
    page.initialize();
    let process = own_process();
    let result = UnitStatsReader::new(&process, 0, page.unit()).read_bulk(&[12, 93, 94], 0);
    assert_eq!(result, Ok(HashMap::from([(12, -256), (93, 0)])));
}

#[test]
fn own_process_read_returns_error_when_page_inaccessible() {
    let mut page = FixturePage::new();
    page.initialize();
    page.make_inaccessible();
    let process = own_process();
    let result = UnitStatsReader::new(&process, 0, page.unit()).read_stat(12, 0);
    assert!(matches!(result, Err(StatReaderError::MemoryReadFailed(_))));
}

#[test]
fn own_process_read_returns_error_when_backing_memory_freed() {
    let mut page = FixturePage::new();
    page.initialize();
    page.decommit();
    let process = own_process();
    let result = UnitStatsReader::new(&process, 0, page.unit()).read_bulk(&[12], 0);
    assert!(matches!(result, Err(StatReaderError::MemoryReadFailed(_))));
}
