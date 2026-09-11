use super::{automap_cell, read_layer, D2Context, MapMarkerManager};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Stage {
    TickParent,
    DetachParent,
    DetachLinks,
    DetachWrite,
    ChildEligibility,
    ChildTopology,
    ChildRecheck,
    ChildWrite,
    ChildPostverify,
    Allocation,
    AllocationLayer,
    PrepareCell,
    LinkCell,
    FindSlot,
    PublicationLayer,
    PublicationSlot,
    Publish,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub(super) enum Reason {
    ParentMismatch {
        expected_parent: u32,
        actual_parent: u32,
    },
    InternalLinks {
        index: usize,
        cell: u32,
        expected_less: u32,
        actual_less: u32,
        expected_more: u32,
        actual_more: u32,
    },
    TailChild {
        index: usize,
        cell: u32,
        expected_less: u32,
        actual_less: u32,
        expected_more: u32,
        actual_more: u32,
    },
    AllocationUnknown {
        index: usize,
    },
    ReadFailed {
        slot: Option<u32>,
    },
    LeafSearchFailed,
    PreparationWriteAmbiguous {
        index: usize,
        cell: u32,
        expected_less: u32,
        expected_more: u32,
    },
    WriteAmbiguous {
        index: Option<usize>,
        cell: Option<u32>,
        slot: u32,
        expected_value: Option<u32>,
    },
    LayerMismatch {
        expected_layer: u32,
        actual_layer: u32,
    },
    AttachSlotChanged {
        slot: u32,
        expected_parent: u32,
        actual_parent: u32,
    },
    ChildTopology {
        issue: TopologyIssue,
        node: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum TopologyIssue {
    AddressOverflow,
    InvalidOwnedSet,
    RepeatedNode,
    BoundExceeded,
    SpareReachable,
    OwnedReachable,
    HeadUnreachable,
    HeadParentMismatch,
    ChildUnreachable,
    ChildParentMismatch,
    PublicationUnconfirmed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(super) struct Failure {
    pub stage: Stage,
    #[serde(flatten)]
    pub reason: Reason,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct Snapshot {
    pub failure: Failure,
    pub tracked_layer: u32,
    pub published_layer: Option<u32>,
    pub observed_layer: Option<u32>,
    pub parent_slot: u32,
    pub head: Option<u32>,
    pub tail: Option<u32>,
    pub placed_count: usize,
    pub spare_count: usize,
    pub post_error_value: Option<u32>,
    pub post_error_less: Option<u32>,
    pub post_error_more: Option<u32>,
}

#[derive(Default)]
pub(super) struct Diagnostics {
    pub first: Option<Snapshot>,
    pub published_layer: Option<u32>,
    emitted: bool,
}

impl MapMarkerManager {
    pub(super) fn record_quarantine(&mut self, ctx: &D2Context, failure: Failure) {
        if self.diagnostics.first.is_some() {
            return;
        }
        let observed_layer = match failure.reason {
            Reason::LayerMismatch { actual_layer, .. } => Some(actual_layer),
            Reason::ReadFailed { .. }
                if matches!(
                    failure.stage,
                    Stage::AllocationLayer | Stage::PublicationLayer
                ) =>
            {
                None
            }
            Reason::ParentMismatch { .. }
            | Reason::InternalLinks { .. }
            | Reason::TailChild { .. }
            | Reason::AllocationUnknown { .. }
            | Reason::ReadFailed { .. }
            | Reason::WriteAmbiguous { .. }
            | Reason::PreparationWriteAmbiguous { .. }
            | Reason::LeafSearchFailed
            | Reason::AttachSlotChanged { .. }
            | Reason::ChildTopology { .. } => read_layer(ctx).ok(),
        };
        let (post_error_value, post_error_less, post_error_more) = match failure.reason {
            Reason::WriteAmbiguous { slot, .. } => (
                ctx.process.read_memory::<u32>(slot as usize).ok(),
                None,
                None,
            ),
            Reason::PreparationWriteAmbiguous { cell, .. } => (
                None,
                ctx.process
                    .read_memory::<u32>(cell as usize + automap_cell::P_LESS)
                    .ok(),
                ctx.process
                    .read_memory::<u32>(cell as usize + automap_cell::P_MORE)
                    .ok(),
            ),
            Reason::ParentMismatch { .. }
            | Reason::InternalLinks { .. }
            | Reason::TailChild { .. }
            | Reason::AllocationUnknown { .. }
            | Reason::ReadFailed { .. }
            | Reason::LeafSearchFailed
            | Reason::LayerMismatch { .. }
            | Reason::AttachSlotChanged { .. }
            | Reason::ChildTopology { .. } => (None, None, None),
        };
        self.diagnostics.first = Some(Snapshot {
            failure,
            tracked_layer: self.last_layer,
            published_layer: self.diagnostics.published_layer,
            observed_layer,
            parent_slot: self.chain_parent_slot,
            head: self.placed.first().copied(),
            tail: self.placed.last().copied(),
            placed_count: self.placed.len(),
            spare_count: self.spare_cells.len(),
            post_error_value,
            post_error_less,
            post_error_more,
        });
    }

    pub(crate) fn emit_quarantine_with(&mut self, emit: impl FnOnce(&str)) {
        if self.diagnostics.emitted {
            return;
        }
        if let Some(snapshot) = &self.diagnostics.first {
            self.diagnostics.emitted = true;
            emit(&format!(
                "map_marker quarantine: {}",
                serde_json::json!(snapshot)
            ));
        }
    }
}
