use std::collections::BTreeMap;

use crate::offsets::stat_list;
use crate::process::D2Context;

/// Reads raw stat values directly from the unit's own (unmerged) StatList —
/// i.e. without equipped-item or skill-granted bonuses. Mirrors
/// `D2Stats.au3::UpdateStatValueMem(0)`, which reads the same
/// `unit -> StatList -> Stats[]` array our DPS-meter trampoline already
/// trusts (`offsets::stat_list`). The coordinating readout applies scaling.
pub(super) fn read_base_stats(ctx: &D2Context, p_unit: u32, ids: &[u32]) -> BTreeMap<u32, i32> {
    let mut out = BTreeMap::new();
    for &id in ids {
        out.insert(id, 0);
    }

    let p_stat_list = match ctx
        .process
        .read_memory::<u32>(p_unit as usize + stat_list::UNIT_TO_STATS_LIST)
    {
        Ok(p) if p != 0 => p as usize,
        _ => return out,
    };
    let p_stat = match ctx
        .process
        .read_memory::<u32>(p_stat_list + stat_list::SL_PSTAT)
    {
        Ok(p) if p != 0 => p as usize,
        _ => return out,
    };
    // `count` is read straight from the target process; if we ever land on a
    // stale/garbage pointer (e.g. mid-attach, or a foreign process layout),
    // it could be any u16. Cap it well above any real StatList's size and
    // use `checked_add`/`saturating_mul` for the record address so garbage
    // input can only yield short-lived bad reads (already tolerated via
    // `unwrap_or`), never an arithmetic-overflow panic.
    const MAX_PLAUSIBLE_STATS: usize = 512;
    let count = (ctx
        .process
        .read_memory::<u16>(p_stat_list + stat_list::SL_STAT_COUNT)
        .unwrap_or(0) as usize)
        .min(MAX_PLAUSIBLE_STATS);

    for i in 0..count {
        let record = match p_stat.checked_add(i.saturating_mul(stat_list::STAT_RECORD_SIZE)) {
            Some(addr) => addr,
            None => break,
        };
        let nstat = ctx
            .process
            .read_memory::<u16>(record + stat_list::STAT_NSTAT)
            .unwrap_or(u16::MAX) as u32;
        if let Some(slot) = out.get_mut(&nstat) {
            let value = ctx
                .process
                .read_memory::<i32>(record + stat_list::STAT_VALUE)
                .unwrap_or(0);
            *slot += value;
        }
    }

    out
}
