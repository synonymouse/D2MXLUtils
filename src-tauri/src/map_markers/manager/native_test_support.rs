//! Owned low-address pages and marker-chain setup shared by native regressions.

use super::super::*;
use super::mk;
use crate::process::{marker_test_io::Scope, ProcessHandle};
use windows::Win32::System::{
    Memory::{VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE},
    Threading::{
        GetCurrentProcessId, OpenProcess, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
    },
};

pub(crate) struct Fixture {
    pub ctx: D2Context,
    region: std::ptr::NonNull<std::ffi::c_void>,
    pub io: Scope,
}
impl Fixture {
    pub fn new() -> Self {
        // SAFETY: GetCurrentProcessId takes no pointers and has no preconditions.
        let pid = unsafe { GetCurrentProcessId() };
        // SAFETY: the PID is current and these rights are used only on fixture-owned pages.
        let handle = unsafe {
            OpenProcess(
                PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_VM_OPERATION,
                false,
                pid,
            )
        }
        .unwrap();
        let process = ProcessHandle { handle, pid };
        let region = (0x10000000usize..0x70000000)
            .step_by(0x200000)
            .find_map(|base| {
                // SAFETY: reservation hints do not overwrite existing mappings; NULL means try another low address.
                std::ptr::NonNull::new(unsafe {
                    VirtualAllocEx(
                        handle,
                        Some(std::ptr::with_exposed_provenance(base)),
                        0x200000,
                        MEM_COMMIT | MEM_RESERVE,
                        PAGE_READWRITE,
                    )
                })
            })
            .expect("low-address fixture allocation");
        let base = region.as_ptr().expose_provenance();
        let fixture = Self {
            ctx: D2Context {
                process,
                d2_client: base,
                d2_common: 0,
                d2_win: 0,
                d2_lang: 0,
                d2_sigma: 0,
                d2_sigma_size: 0,
                always_show_items_ptr_rva: None,
            },
            region,
            io: Scope::new(base..base + 0x200000),
        };
        fixture.seed(d2client::AUTOMAP_LAYER, fixture.address(0x1000));
        fixture.seed(d2client::PLAYER_UNIT, fixture.address(0x2000));
        fixture.seed(0x2000 + paths::TO_PATHS_PTR[1], fixture.address(0x3000));
        fixture.seed(0x3000 + paths::TO_PATHS_PTR[2], fixture.address(0x4000));
        fixture
    }
    pub fn address(&self, offset: usize) -> u32 {
        u32::try_from(self.ctx.d2_client + offset).unwrap()
    }
    pub fn seed(&self, offset: usize, value: u32) {
        self.ctx
            .process
            .write_buffer(self.ctx.d2_client + offset, &value.to_le_bytes())
            .unwrap();
    }
    pub fn word(&self, offset: usize) -> u32 {
        self.ctx
            .process
            .read_memory(self.ctx.d2_client + offset)
            .unwrap()
    }
    pub fn items(&self, count: u32) {
        self.seed(
            0x4000 + room1::UNIT_FIRST,
            if count == 0 { 0 } else { self.address(0x5000) },
        );
        for index in 0..count {
            let offset = 0x5000 + usize::try_from(index).unwrap() * 0x100;
            self.seed(offset + unit::UNIT_TYPE, unit_type::ITEM);
            self.seed(offset + unit::UNIT_ID, index + 1);
            self.seed(offset + unit::PATH, self.address(offset + 0x80));
            self.seed(offset + 0x80 + item_path::SUB_X, 100 + index);
            self.seed(offset + 0x80 + item_path::SUB_Y, 100);
            self.seed(
                offset + unit::ROOM_NEXT,
                if index + 1 == count {
                    0
                } else {
                    self.address(offset + 0x100)
                },
            );
        }
    }
    pub fn injector(&self) -> D2Injector {
        D2Injector::for_marker_test((0..200).map(|index| self.address(0x20000 + index * 0x40)))
    }
    pub fn context(&self) -> D2Context {
        // SAFETY: duplicate ownership via a fresh handle to this process, not a borrowed HANDLE.
        let handle = unsafe {
            OpenProcess(
                PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_VM_OPERATION,
                false,
                self.ctx.process.pid,
            )
        }
        .unwrap();
        D2Context {
            process: ProcessHandle {
                handle,
                pid: self.ctx.process.pid,
            },
            d2_client: self.ctx.d2_client,
            d2_common: 0,
            d2_win: 0,
            d2_lang: 0,
            d2_sigma: 0,
            d2_sigma_size: 0,
            always_show_items_ptr_rva: None,
        }
    }
    pub fn tick(
        &self,
        manager: &mut MapMarkerManager,
        injector: &D2Injector,
    ) -> Result<(), String> {
        let matched: Vec<_> = bfs_item_positions(&self.ctx, 10)?
            .into_iter()
            .map(|(ptr, sx, sy)| {
                mk(
                    self.ctx
                        .process
                        .read_memory::<u32>(usize::try_from(ptr).unwrap() + unit::UNIT_ID)
                        .unwrap(),
                    sx,
                    sy,
                )
            })
            .collect();
        let bfs = matched.iter().map(|item| item.unit_id).collect();
        manager.tick(&self.ctx, injector, &matched, &HashSet::new(), &bfs, None)
    }
    pub fn cells(&self) -> Vec<(u32, u16)> {
        let mut node = self.word(0x1000 + automap_layer::P_OBJECTS);
        let mut cells = Vec::new();
        while node != 0 {
            assert!(
                cells.len() < MAX_MARKER_CELLS,
                "chain exceeds cap or cycles"
            );
            let address = usize::try_from(node).unwrap();
            cells.push((
                node,
                self.ctx
                    .process
                    .read_memory(address + automap_cell::X_PIXEL)
                    .unwrap(),
            ));
            node = self
                .ctx
                .process
                .read_memory(address + automap_cell::P_LESS)
                .unwrap();
        }
        cells
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // SAFETY: this is the original reservation, owned once, and the process handle is still live.
        let result = unsafe {
            VirtualFreeEx(
                self.ctx.process.handle,
                self.region.as_ptr(),
                0,
                MEM_RELEASE,
            )
        };
        assert!(result.is_ok(), "fixture pages must be released");
    }
}
pub(crate) fn calls(injector: &D2Injector) -> usize {
    injector
        .marker_allocator
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .calls
}

pub(in crate::map_markers::manager) fn seeded(
    count: u32,
) -> (Fixture, D2Injector, MapMarkerManager) {
    let fixture = Fixture::new();
    let injector = fixture.injector();
    let mut manager = MapMarkerManager::new();
    fixture.seed(0x1000 + automap_layer::P_OBJECTS, fixture.address(0x7000));
    fixture.items(count);
    fixture.tick(&mut manager, &injector).unwrap();
    let tail = usize::try_from(*manager.placed.last().unwrap()).unwrap();
    fixture
        .ctx
        .process
        .write_buffer(
            tail + automap_cell::P_LESS,
            &fixture.address(0x8000).to_le_bytes(),
        )
        .unwrap();
    (fixture, injector, manager)
}

pub(in crate::map_markers::manager) fn remains_quarantined(
    fixture: &Fixture,
    manager: &mut MapMarkerManager,
    injector: &D2Injector,
) {
    assert!(!manager.cells_trusted);
    let diagnosis = manager.diagnostics.first.clone().unwrap();
    let placed = manager.placed.clone();
    let spares = manager.spare_cells.clone();
    let allocated = calls(injector);
    let writes = fixture.io.writes();
    for _ in 0..3 {
        assert!(fixture.tick(manager, injector).is_err());
        assert!(manager.clear(&fixture.ctx).is_err());
    }
    assert_eq!(manager.placed, placed);
    assert_eq!(manager.spare_cells, spares);
    assert_eq!(manager.diagnostics.first.as_ref(), Some(&diagnosis));
    assert_eq!(fixture.io.writes(), writes);
    assert_eq!(calls(injector), allocated);
}
