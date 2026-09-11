// --- Live probe (manual verification only) ---
//
// `#[ignore]`d so `cargo test` never runs these in CI — they need an
// actually-running game and touch its process memory. Run explicitly with:
//   cargo test --target x86_64-unknown-linux-gnu -- --ignored --nocapture live_probe
use super::*;

/// Scan every readable region of the live process for `needle`, both
/// as raw ASCII and as UTF-16LE (D2's UI text fields can be either
/// depending on the control), printing each hit with surrounding
/// context bytes. One-off RE tool: run with
/// `SCAN_NEEDLE=<marker> cargo test --target x86_64-unknown-linux-gnu \
///   scan_memory_for_string -- --ignored --nocapture`
/// after typing `<marker>` into a live UI text field, to locate its
/// backing buffer address without guessing at struct layouts.
#[test]
#[ignore]
fn scan_memory_for_string() {
    let needle_str =
        std::env::var("SCAN_NEEDLE").expect("set SCAN_NEEDLE=<marker text> before running");
    let ascii_needle = needle_str.as_bytes().to_vec();
    let utf16_needle: Vec<u8> = needle_str
        .encode_utf16()
        .flat_map(|c| c.to_le_bytes())
        .collect();

    let ctx = D2Context::new().expect("attach failed");
    let pid = ctx.process.pid;

    let maps = std::fs::read_to_string(format!("/proc/{}/maps", pid))
        .expect("reading /proc/<pid>/maps failed");

    let mut total_hits = 0usize;
    for line in maps.lines() {
        let mut parts = line.split_whitespace();
        let Some(range) = parts.next() else { continue };
        let Some(perms) = parts.next() else { continue };
        if !perms.starts_with('r') {
            continue;
        }
        let Some((start_s, end_s)) = range.split_once('-') else {
            continue;
        };
        let (Ok(start), Ok(end)) = (
            usize::from_str_radix(start_s, 16),
            usize::from_str_radix(end_s, 16),
        ) else {
            continue;
        };
        let size = end.saturating_sub(start);
        // Skip absurdly large regions (e.g. reserved-but-unbacked
        // ranges) to keep this a quick manual tool, not a full dump.
        if size == 0 || size > 256 * 1024 * 1024 {
            continue;
        }

        let Ok(buf) = ctx.process.read_buffer(start, size) else {
            continue;
        };

        for (needle, kind) in [(&ascii_needle, "ascii"), (&utf16_needle, "utf16le")] {
            if needle.is_empty() {
                continue;
            }
            let mut offset = 0usize;
            while let Some(pos) = buf[offset..]
                .windows(needle.len())
                .position(|w| w == needle.as_slice())
            {
                let addr = start + offset + pos;
                total_hits += 1;
                let ctx_start = (offset + pos).saturating_sub(32);
                let ctx_end = (offset + pos + needle.len() + 32).min(buf.len());
                println!(
                    "[{}] hit @ {:#x} (region {}-{} perms={})",
                    kind, addr, range, perms, perms
                );
                println!("  bytes: {:02x?}", &buf[ctx_start..ctx_end]);
                println!(
                    "  ascii: {:?}",
                    String::from_utf8_lossy(&buf[ctx_start..ctx_end])
                );
                offset += pos + 1;
                if offset >= buf.len() {
                    break;
                }
            }
        }
    }
    println!("total hits: {}", total_hits);
}

/// Dump a window of memory around a known-good hit address (found via
/// `scan_memory_for_string`) to visually inspect the surrounding
/// struct layout — e.g. sibling UI control fields for Password /
/// Description next to a confirmed Game Name buffer. Run with
/// `SCAN_ADDR=0x... [SCAN_BEFORE=0x400] [SCAN_AFTER=0x400] cargo test \
///   --target x86_64-unknown-linux-gnu dump_region -- --ignored --nocapture`
#[test]
#[ignore]
fn dump_region() {
    let addr = usize::from_str_radix(
        std::env::var("SCAN_ADDR")
            .expect("set SCAN_ADDR=0x... before running")
            .trim_start_matches("0x"),
        16,
    )
    .expect("SCAN_ADDR must be hex");
    let before: usize = std::env::var("SCAN_BEFORE")
        .ok()
        .and_then(|s| usize::from_str_radix(s.trim_start_matches("0x"), 16).ok())
        .unwrap_or(0x400);
    let after: usize = std::env::var("SCAN_AFTER")
        .ok()
        .and_then(|s| usize::from_str_radix(s.trim_start_matches("0x"), 16).ok())
        .unwrap_or(0x400);

    let ctx = D2Context::new().expect("attach failed");
    let start = addr.saturating_sub(before);
    let size = before + after;
    let buf = ctx
        .process
        .read_buffer(start, size)
        .expect("read_buffer failed");

    for (i, chunk) in buf.chunks(16).enumerate() {
        let line_addr = start + i * 16;
        let hex: Vec<String> = chunk.iter().map(|b| format!("{:02x}", b)).collect();
        let ascii: String = chunk
            .iter()
            .map(|&b| {
                if (0x20..0x7f).contains(&b) {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();
        let marker = if line_addr <= addr && addr < line_addr + 16 {
            " <=="
        } else {
            ""
        };
        println!(
            "{:#010x}: {:<48} {}{}",
            line_addr,
            hex.join(" "),
            ascii,
            marker
        );
    }
}

/// Search every readable region for 4-byte little-endian pointer
/// values equal to `SCAN_PTR` (a target address found via
/// `scan_memory_for_string`) — i.e. "what points at this address".
/// Used to trace a transient heap buffer back to whatever stable
/// parent structure holds a pointer to it.
#[test]
#[ignore]
fn scan_memory_for_pointer() {
    let target = usize::from_str_radix(
        std::env::var("SCAN_PTR")
            .expect("set SCAN_PTR=0x... before running")
            .trim_start_matches("0x"),
        16,
    )
    .expect("SCAN_PTR must be hex") as u32;
    let needle = target.to_le_bytes();

    let ctx = D2Context::new().expect("attach failed");
    let pid = ctx.process.pid;
    let maps = std::fs::read_to_string(format!("/proc/{}/maps", pid))
        .expect("reading /proc/<pid>/maps failed");

    let mut total_hits = 0usize;
    for line in maps.lines() {
        let mut parts = line.split_whitespace();
        let Some(range) = parts.next() else { continue };
        let Some(perms) = parts.next() else { continue };
        if !perms.starts_with('r') {
            continue;
        }
        let Some((start_s, end_s)) = range.split_once('-') else {
            continue;
        };
        let (Ok(start), Ok(end)) = (
            usize::from_str_radix(start_s, 16),
            usize::from_str_radix(end_s, 16),
        ) else {
            continue;
        };
        let size = end.saturating_sub(start);
        if size == 0 || size > 256 * 1024 * 1024 {
            continue;
        }
        let Ok(buf) = ctx.process.read_buffer(start, size) else {
            continue;
        };

        let mut offset = 0usize;
        while offset + 4 <= buf.len() {
            if buf[offset..offset + 4] == needle {
                let addr = start + offset;
                total_hits += 1;
                println!("hit @ {:#x} (region {} perms={})", addr, range, perms);
            }
            offset += 4;
        }
    }
    println!("total hits: {}", total_hits);
}

#[test]
#[ignore]
fn attach_and_resolve_modules() {
    let ctx = D2Context::new().expect("attach failed");
    println!("pid = {}", ctx.process.pid);
    println!("d2_client = {:#x}", ctx.d2_client);
    println!("d2_common = {:#x}", ctx.d2_common);
    println!("d2_win = {:#x}", ctx.d2_win);
    println!("d2_lang = {:#x}", ctx.d2_lang);
    println!(
        "d2_sigma = {:#x} (size {:#x})",
        ctx.d2_sigma, ctx.d2_sigma_size
    );
    assert_ne!(ctx.d2_client, 0);
    assert_ne!(ctx.d2_common, 0);
    assert_ne!(ctx.d2_win, 0);
    assert_ne!(ctx.d2_lang, 0);
}

#[test]
#[ignore]
fn read_player_unit_pointer() {
    use crate::offsets::d2client;

    let ctx = D2Context::new().expect("attach failed");
    let player_unit_ptr = ctx
        .process
        .read_memory::<u32>(ctx.d2_client + d2client::PLAYER_UNIT)
        .expect("read failed");
    println!("PLAYER_UNIT ptr = {:#x}", player_unit_ptr);
    // Nonzero only while actually in-game; this just proves the read
    // pipeline (process_vm_readv against the real host PID) works.
}

#[test]
#[ignore]
fn find_free_padding_runs() {
    use crate::offsets::d2client;

    let ctx = D2Context::new().expect("attach failed");
    let inject_base = ctx.d2_client + d2client::INJECT_BASE;
    // Scan a generous window past the existing stubs (which top out
    // around +0x76) looking for long all-zero runs — candidates for
    // `string_buffer` (need ~0x1000 bytes) and `params_buffer` (~0x100).
    let dump = ctx
        .process
        .read_buffer(inject_base, 0x4000)
        .expect("read failed");

    let mut runs: Vec<(usize, usize)> = Vec::new(); // (start, len)
    let mut run_start: Option<usize> = None;
    for (i, &b) in dump.iter().enumerate() {
        if b == 0 {
            if run_start.is_none() {
                run_start = Some(i);
            }
        } else if let Some(start) = run_start.take() {
            runs.push((start, i - start));
        }
    }
    if let Some(start) = run_start {
        runs.push((start, dump.len() - start));
    }

    runs.sort_by(|a, b| b.1.cmp(&a.1));
    println!("longest zero runs within INJECT_BASE+0x0..0x4000:");
    for (start, len) in runs.iter().take(10) {
        println!(
            "  +{:#x}, len {:#x} (ends at +{:#x})",
            start,
            len,
            start + len
        );
    }
}

/// End-to-end: mmap allocation + shellcode install + a ptrace-hijacked
/// call, via `D2Injector::get_string` (pure read-only string-table
/// lookup — no game-state mutation, the safest first real call target).
#[test]
#[ignore]
fn injector_get_string_roundtrip() {
    let ctx = D2Context::new().expect("attach failed");
    let injector =
        crate::injection::D2Injector::new(&ctx.process, ctx.d2_client, ctx.d2_common, ctx.d2_lang)
            .expect("D2Injector::new failed");
    println!("string_buffer @ {:#x}", injector.string_buffer.address);
    println!("params_buffer @ {:#x}", injector.params_buffer.address);

    for id in [1u16, 2, 100, 1000] {
        match injector.get_string(&ctx.process, id, 64) {
            Ok(s) => println!("get_string({id}) = {:?}", s),
            Err(e) => println!("get_string({id}) failed: {e}"),
        }
    }
}
