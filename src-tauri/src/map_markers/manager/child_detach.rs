use super::diagnostics::TopologyIssue;
use super::*;

const NODE_BOUND: usize = 4096;

const fn topology(issue: TopologyIssue, node: u32) -> Reason {
    Reason::ChildTopology { issue, node }
}

#[cfg(test)]
#[path = "child_detach_arithmetic_tests.rs"]
mod tests;

fn slot(base: u32, offset: usize) -> Result<u32, Reason> {
    let address = u32::try_from(offset)
        .ok()
        .and_then(|offset| base.checked_add(offset));
    address
        .filter(|address| address.checked_add(3).is_some())
        .ok_or_else(|| topology(TopologyIssue::AddressOverflow, base))
}

fn word(ctx: &D2Context, address: u32) -> Result<u32, Reason> {
    let address = slot(address, 0)?;
    let native =
        usize::try_from(address).map_err(|_| topology(TopologyIssue::AddressOverflow, address))?;
    ctx.process
        .read_memory(native)
        .map_err(|_| Reason::ReadFailed {
            slot: Some(address),
        })
}

fn layer(ctx: &D2Context) -> Result<u32, Reason> {
    let base =
        u32::try_from(ctx.d2_client).map_err(|_| topology(TopologyIssue::AddressOverflow, 0))?;
    word(ctx, slot(base, d2client::AUTOMAP_LAYER)?)
}

impl MapMarkerManager {
    pub(super) fn detach_child(
        &mut self,
        ctx: &D2Context,
        candidate: (u32, Reason),
    ) -> Result<(), String> {
        let (child, original) = candidate;
        self.cells_trusted = false;
        if let Err(failure) = self.splice_child(ctx, child) {
            let failure = match failure {
                Failure {
                    stage: Stage::ChildEligibility,
                    reason: Reason::LayerMismatch { .. },
                } => Failure {
                    stage: Stage::DetachLinks,
                    reason: original,
                },
                failure => failure,
            };
            self.record_quarantine(ctx, failure);
            return self.require_trusted_cells();
        }
        self.spare_cells.append(&mut self.placed);
        self.chain_parent_slot = 0;
        self.cells_trusted = true;
        Ok(())
    }

    fn splice_child(&self, ctx: &D2Context, child: u32) -> Result<(), Failure> {
        let mut stage = Stage::ChildEligibility;
        let result = (|| {
            let owned: HashSet<_> = self.placed.iter().copied().collect();
            if child == 0
                || owned.is_empty()
                || owned.len() != self.placed.len()
                || owned.contains(&0)
                || self.chain_parent_slot == 0
                || self.spare_cells.iter().any(|cell| owned.contains(cell))
            {
                return Err(topology(TopologyIssue::InvalidOwnedSet, child));
            }
            let current_layer = layer(ctx)?;
            if current_layer == 0 || current_layer != self.last_layer {
                return Err(Reason::LayerMismatch {
                    expected_layer: self.last_layer,
                    actual_layer: current_layer,
                });
            }
            if self.diagnostics.published_layer != Some(current_layer) {
                return Err(topology(
                    TopologyIssue::PublicationUnconfirmed,
                    current_layer,
                ));
            }
            stage = Stage::ChildTopology;
            self.check_child_root(ctx, Some(child))?;
            stage = Stage::ChildRecheck;
            self.check_child_boundary(ctx, self.placed[0])?;
            self.check_child_root(ctx, Some(child))?;
            self.check_child_boundary(ctx, self.placed[0])?;
            // Pointer equality is not generation proof. RPM/WPM checks reduce
            // risk but do not synchronize with the engine or establish ownership.
            stage = Stage::ChildWrite;
            let parent = usize::try_from(self.chain_parent_slot)
                .map_err(|_| topology(TopologyIssue::AddressOverflow, self.chain_parent_slot))?;
            ctx.process
                .write_buffer(parent, &child.to_le_bytes())
                .map_err(|_| Reason::WriteAmbiguous {
                    index: None,
                    cell: None,
                    slot: self.chain_parent_slot,
                    expected_value: Some(child),
                })?;
            stage = Stage::ChildPostverify;
            self.check_child_boundary(ctx, child)?;
            let incoming = self.check_child_root(ctx, None)?;
            match incoming.get(&child) {
                Some(parent) if *parent == self.chain_parent_slot => (),
                Some(_) => return Err(topology(TopologyIssue::ChildParentMismatch, child)),
                None => return Err(topology(TopologyIssue::ChildUnreachable, child)),
            }
            self.check_child_boundary(ctx, child)?;
            Ok(())
        })();
        result.map_err(|reason| Failure { stage, reason })
    }

    fn check_child_boundary(&self, ctx: &D2Context, expected_parent: u32) -> Result<(), Reason> {
        let current_layer = layer(ctx)?;
        if current_layer != self.last_layer {
            return Err(Reason::LayerMismatch {
                expected_layer: self.last_layer,
                actual_layer: current_layer,
            });
        }
        let actual_parent = word(ctx, self.chain_parent_slot)?;
        if actual_parent != expected_parent {
            return Err(Reason::ParentMismatch {
                expected_parent,
                actual_parent,
            });
        }
        Ok(())
    }

    fn check_child_links(
        &self,
        ctx: &D2Context,
        indexed_cell: (usize, u32),
        child: u32,
    ) -> Result<(u32, u32), Reason> {
        let (index, cell) = indexed_cell;
        let actual_less = word(ctx, slot(cell, automap_cell::P_LESS)?)?;
        let actual_more = word(ctx, slot(cell, automap_cell::P_MORE)?)?;
        let expected_less = self.placed.get(index + 1).copied().unwrap_or(child);
        if actual_less != expected_less || actual_more != 0 {
            return Err(Reason::InternalLinks {
                index,
                cell,
                expected_less,
                actual_less,
                expected_more: 0,
                actual_more,
            });
        }
        Ok((actual_less, actual_more))
    }

    fn check_child_root(
        &self,
        ctx: &D2Context,
        child: Option<u32>,
    ) -> Result<HashMap<u32, u32>, Reason> {
        let root = slot(self.last_layer, automap_layer::P_OBJECTS)?;
        let mut pending = vec![(root, word(ctx, root)?)];
        // Record incoming slots for every node, including all head ancestors.
        // One shared visited map rejects subtree cycles and intersections with
        // ancestors/owned nodes/other branches without a second subtree walk.
        let mut incoming = HashMap::new();
        while let Some((parent, node)) = pending.pop() {
            if node == 0 {
                continue;
            }
            if incoming.contains_key(&node) {
                return Err(topology(TopologyIssue::RepeatedNode, node));
            }
            if incoming.len() == NODE_BOUND {
                return Err(topology(TopologyIssue::BoundExceeded, node));
            }
            incoming.insert(node, parent);
            if self.spare_cells.contains(&node) {
                return Err(topology(TopologyIssue::SpareReachable, node));
            }
            let less_slot = slot(node, automap_cell::P_LESS)?;
            let more_slot = slot(node, automap_cell::P_MORE)?;
            let (less, more) =
                if let Some(index) = self.placed.iter().position(|cell| *cell == node) {
                    match child {
                        Some(child) => self.check_child_links(ctx, (index, node), child)?,
                        None => return Err(topology(TopologyIssue::OwnedReachable, node)),
                    }
                } else {
                    (word(ctx, less_slot)?, word(ctx, more_slot)?)
                };
            pending.push((more_slot, more));
            pending.push((less_slot, less));
        }
        if child.is_some() {
            match incoming.get(&self.placed[0]) {
                Some(parent) if *parent == self.chain_parent_slot => (),
                Some(_) => return Err(topology(TopologyIssue::HeadParentMismatch, self.placed[0])),
                None => return Err(topology(TopologyIssue::HeadUnreachable, self.placed[0])),
            }
            if self.placed.iter().any(|cell| !incoming.contains_key(cell)) {
                return Err(topology(TopologyIssue::InvalidOwnedSet, self.placed[0]));
            }
        }
        Ok(incoming)
    }
}
