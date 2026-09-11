//! Per-instance allocator override and dummy injector for existing Windows fixtures.

use windows::Win32::Foundation::HANDLE;

use super::{D2Injector, RemoteAlloc};
use crate::stat_telemetry::StatTelemetryCounters;

pub(crate) struct MarkerTestAllocator {
    pub cells: std::collections::VecDeque<Result<u32, String>>,
    pub calls: usize,
}

impl MarkerTestAllocator {
    pub(super) fn allocate(&mut self) -> Result<u32, String> {
        self.calls += 1;
        self.cells.pop_front().expect("marker fixture exhausted")
    }
}

impl D2Injector {
    pub(crate) fn for_marker_test(cells: impl Iterator<Item = u32>) -> Self {
        Self {
            telemetry: StatTelemetryCounters::default(),
            marker_allocator: Some(std::sync::Mutex::new(MarkerTestAllocator {
                cells: cells.map(Ok).collect(),
                calls: 0,
            })),
            string_buffer: RemoteAlloc {
                handle: HANDLE::default(),
                address: 0,
                size: 0,
            },
            params_buffer: RemoteAlloc {
                handle: HANDLE::default(),
                address: 0,
                size: 0,
            },
            inject_get_string: 0,
            inject_get_item_name: 0,
            inject_get_item_stat: 0,
            inject_get_unit_stat: 0,
            inject_new_automap_cell: 0,
        }
    }
}
