# PR12 correction: implementation and evidence

## Current publication status

This is a partial PR #12 follow-up based on master `020e81f`, after the PR #15
revert, not a restoration of PR #13's architecture. Eight added inline regression
tests live in the existing marker modules, with `cfg(test)` process/injector
fixtures; there is no separate `src-tauri/tests` tree. The user explicitly
dropped the expanded/rare injector restart and lifecycle work from this scope.
Native pool-generation and renderer-concurrency QA remain pending: this patch
has not been validated in a live game and does not close ERROR #8 or prove a
full-lifetime allocation bound. Quarantine can stop marker updates or leave old
markers visible until native cleanup.

After updating to `020e81f`, independently rerun locked Cargo checks passed:
**259 backend tests**, **29 i686 Windows marker-filter tests**, and a normal
backend build (**exit 0**). Existing compiler warnings remain. These results
supplement, rather than replace, the historical totals below. The preserved
[investigation](map-marker-memory-investigation-plan.md) records earlier live
observations and broader proposals, not live validation of this correction.
The sections below retain their original chronology, commands, and scope;
pre-consolidation file lists and line counts describe historical revisions.

The harness/driver commands below are historical evidence. The current inline
suite and its reduced coverage are documented in [Inline consolidation](#inline-consolidation).

Base: master `607499e`. Before edits, `git diff e296ab4 --
src-tauri/src/map_marker.rs` was empty: current production exactly matched the
reviewed PR12 marker implementation. No branch switch, formatting, commit, or
push was performed. The pre-existing `.gitignore` edits and
`docs/map-marker-memory-investigation-plan.md` were not modified. Original
review worktree sources, failing probes, and evidence remain untouched.

## Production decisions

- Layer-switch detach errors propagate before any cells become spares. Read
  failures keep bookkeeping for retry. A failed write, unexpected link/parent,
  unknown allocator outcome, or failed post-allocation preparation quarantines
  the existing manager's addresses. No parallel ownership manager was added.
- Quarantine retains the placed/spare vectors and prevents further writes and
  allocator calls until lifecycle invalidation. Logical map-off does not clear
  quarantine. An ambiguous write may have completed, partially completed, or
  had no effect; the manager does not infer which from the next memory read.
- Logical map-off clears logical item state before any fallible memory operation,
  successfully detaches even after a layer switch, and retains safe detached
  cells. Failed clears retain physical ownership accounting, not logical markers,
  and are retried by the scanner gate rather than recorded as completed.
- Game-entry reset drops old addresses and logical state without writing old
  memory. A successfully read NULL player invalidates addresses before the
  scanner's early return. A player read error is not a NULL observation.
- Evicted identities retain first-seen stamps only while in the current BFS
  set (or the capped persistent set). Complete over-cap snapshots stay stable;
  genuine newcomers still displace oldest entries. No historical ID archive.
  TTL, distance, cached visibility/filter decisions, and the 100-cell cap remain.
- Leaf traversal propagates read errors. Publication rechecks the current layer
  and empty attach slot after cell writes and traversal. These are TOCTOU guards,
  not synchronization with the native renderer or allocator.

### Allocator distinction

`D2Injector::new_automap_cell` currently returns `Result<u32, String>` from a
remote thread. `Err` does not establish whether allocation ran. It now freezes
further operations rather than retrying indefinitely. Successfully allocated
addresses remain accounted for. A confirmed NULL return keeps earlier allocated
cells available for retry; the permanent public-module test exercises this.
The PR's original pure helper partial-failure retention test is unchanged and
passes. The old review fake's "Err means definitely no effect" retry probe is
preserved in its original worktree, not presented as a native guarantee. Restoring
automatic retry for particular native errors requires a proven typed no-effect
contract from the injection layer; string matching would not supply that proof.

## Executed red/green checks

Commands below run from the repository root. Test I/O is simulated, production
`map_marker.rs`, `marker_scanner.rs`, and `offsets.rs` are imported directly.
The scanner fixture exercises the real BFS and public scanner tick/clear paths;
its shared state/filter containers and allocator are doubles, not the full app.

```text
rustc --edition=2021 --test src-tauri/tests/marker_memory.rs -o src-tauri/target/marker-memory-tests.exe
src-tauri\target\marker-memory-tests.exe --test-threads=1 --nocapture
```

Initial permanent marker probes before production edits: **22 passed, 10 failed**.
After marker changes: **32 passed, 0 failed**.
Scanner-specific failing-first run after adding scanner fixtures but before
scanner changes: **0 passed, 3 failed** (`scanner_support` filter).
Initial expanded suite: **47 passed, 0 failed**, including all 18 original marker
tests and both original scanner tests.

An additional substitution check compiled that revision's public probes against the
unchanged reviewed production files using a temporary `target/marker-baseline.rs`
import harness: **27 passed, 19 failed**. The one new private bounded-stamp test
was not part of that old-source harness (46 total). Commands:

```text
rustc --edition=2021 --test src-tauri/target/marker-baseline.rs -o src-tauri/target/marker-baseline.exe
src-tauri\target\marker-baseline.exe --test-threads=1
```

The temporary harness was removed after execution. Its imports used the original
review worktree map/scanner/offsets and the permanent `marker_support` doubles and
probes. The output remains in ignored `src-tauri/target/marker-baseline-results.txt`.

```text
cargo test --manifest-path src-tauri/Cargo.toml --locked --quiet
Final logical-clear correction: 252 backend unit tests passed; 49 integration-harness tests passed.

rustc --edition=2021 --target i686-pc-windows-msvc --test src-tauri/tests/marker_memory.rs -o src-tauri/target/marker-memory-tests-x86.exe
src-tauri\target\marker-memory-tests-x86.exe --test-threads=1
Final logical-clear correction: 49 passed, 0 failed.

cargo build --manifest-path src-tauri/Cargo.toml --locked --quiet
Exit 0; existing compiler warnings remain. Not a packaged Tauri release build.

cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings
Fails on existing repository warnings/style findings.

cargo clippy --manifest-path src-tauri/Cargo.toml --locked --test marker_memory -- -D warnings
Pre-logical-clear-correction scoped run fails only on existing reconcile_persistent argument count
and marker_scanner explicit auto-deref. No new test-support lint findings.
```

The single-threaded scanner fake has a narrowly justified Clippy expectation
for its required production `Arc` API over `RefCell` byte memory. It does not
claim thread safety. LSP error diagnostics: none on all changed Rust files.
`git diff --check`: exit 0 (line-ending notices only).

### Logical clear intent correction

The follow-up review found that a failed map-off detach retained the logical
cache. A subsequent NULL-player invalidation and map re-enable could resurrect
an item no longer in BFS. `clear` now clears `persistent`, `last_seen`,
`first_seen`, and `last_player_sub` before reading the layer. It does not reset
`last_hash`, `last_layer`, parent, placed cells, spares, or trust as part of that
logical change. Existing physical teardown and ambiguous-write quarantine remain.

Failing-first and green commands:

```text
rustc --edition=2021 --test src-tauri/tests/marker_memory.rs -o src-tauri/target/marker-memory-tests.exe
src-tauri\target\marker-memory-tests.exe scanner_support::tests::failed_clear --nocapture --test-threads=1
```

Red: **0 passed, 2 failed**. The effect-then-error detach, NULL-player interval,
item removal, and re-enable scenario produced **allocations=2, additional
writes=2, root=0x10080**. The transient layer-read failure scenario left the old
root at **0x10040** after re-enable with the item removed.

Green: **2 passed, 0 failed**. The loading scenario produced **allocations=1,
additional writes=0, root=0x0**. The read-error scenario detached the old marker
without additional allocation. Both scenarios use unknown player position, so
distance-based pickup pruning cannot hide the regression.

After all Rust inputs were final, Cargo tests passed **252 + 49**, Cargo build
exited **0**, and the freshly compiled x86 harness passed **49/0**. LSP error
diagnostics were clean on both modified Rust files. Existing compiler warning
output is retained under `src-tauri/target/marker-clear-{test,build}-warnings.txt`.

## Public-module driver QA

```text
rustc --edition=2021 src-tauri/tests/marker_support/driver.rs -o src-tauri/target/marker-memory-driver.exe
src-tauri\target\marker-memory-driver.exe
```

Observed before the logical-clear follow-up, with assertions, exit 0 (the lead
will run the driver against final source):

- 100 full 101-item scans: 100 allocations, zero additional writes.
- Ten map-off/on cycles: high-water stays at 100 allocated cells.
- Ambiguous detach followed by 100 ticks: zero additional calls or writes.
- Explicit session reset: old reassigned address untouched, fresh cell allocated.

The driver's executable and test binaries are normal ignored target outputs.
No debugger attachment, native hooks, or live-game modifications were made.

## Remaining native limits and product impact

This is not proof of pool lifetime, race freedom, an ERROR #8 fix, or a total
allocation bound across native lifecycle transitions. The old conditional probe
`reset_without_loading_observation_should_not_overwrite_reassigned_spare` remains
unresolved: a missed native pool reset can leave a stale spare address looking
valid. The fix wires known loading/session signals; it cannot prove those signals
cover every cleanup. Byte/link/content checks do not establish generation.

The existing area-jump heuristic still invalidates cells and clears logical
markers. It no longer attempts to detach through potentially freed addresses.
If such a jump or a NULL-player interval does not actually destroy the pool,
discarding addresses can still abandon live cells/old markers. Repeated false
lifecycle signals and scanner stop/restart can therefore defeat a lifetime-wide
allocation bound. Conversely, a missed signal can permit stale reuse. Resolving
that conflict requires verified native pool-generation evidence, not changing
the filter/TTL policy or adding speculative hooks.

Quarantine is deliberately fail-closed: markers may stop updating, and a failed
map-off detach can leave already published markers visible until native cleanup.
There is no honest way to promise immediate hide and safe reuse after an unknown
write. Clear retry handles transient read failures; ambiguous writes stay blocked.
Remote-thread completion after a timeout and renderer-held pointers after detach
are not serialized by this patch. The final publication checks narrow races but
cannot eliminate them. Shutdown errors are logged, but no continuing scanner
exists to retry after the owning scanner has been dropped.

Required native validation before any broader safety claim: observed pool
destruction/generation transitions, renderer/allocator concurrency, sustained
movement/portals/acts/loading, repeated filter toggles and scanner restarts,
and Game.exe memory/rendering measurements. No live game was exercised here.

## Architectural review

The existing marker manager remains the sole ownership coordinator. Added files
have one role each: bounded-cache regression, import harness, byte I/O, allocator,
marker probes, extra failure probes, scanner fixture, or driver. No unsafe code,
dependencies, production casts, policy variants, or broad refactors were added.
Production errors retain the existing String boundary contract and logger.

Approximate pure LOC (`rg -c "^\s*[^/\s#]"`): existing map_marker 790 and
marker_scanner 276; new files 8-162 each. The existing oversized modules were
deliberately not split under the task's explicit focused-scope instruction.

Changed files owned by this task:

- `src-tauri/src/map_marker.rs`
- `src-tauri/src/marker_scanner.rs`
- `src-tauri/src/map_marker_regression.rs`
- `src-tauri/tests/marker_memory.rs`
- `src-tauri/tests/marker_support/process.rs`
- `src-tauri/tests/marker_support/injection.rs`
- `src-tauri/tests/marker_support/probes.rs`
- `src-tauri/tests/marker_support/extra.rs`
- `src-tauri/tests/marker_support/scanner.rs`
- `src-tauri/tests/marker_support/driver.rs`
- `docs/pr12-correction-evidence.md`

## Inline consolidation

The separate `src-tauri/tests` tree (including its driver) and
`src-tauri/src/map_marker_regression.rs` were removed. All 18 original marker
tests and both original scanner tests remain unchanged. The 29 added regression
tests were consolidated into eight test families inside the owning components.
Counting physical source lines, the removed suite plus its four-line module
declaration occupied 683 lines; replacement tests/support occupy 402 lines:
**281 fewer lines**, with no archived or hidden replacement harness.

### Current locations and retained behavior

| Inline test | Retained behavior / replaced probes |
| --- | --- |
| `map_marker::tests::full_snapshots_keep_stable_markers_and_bounded_identity_stamps` | Platform-independent complete 101-item snapshots, stable selected identities, 100 successive batches, bounded first-seen storage. Replaces the separate bounded-stamp regression. |
| `map_marker::tests::native::unchanged_full_bfs_snapshot_performs_no_rebuild` | Real BFS and public manager tick for 100 -> 101 items, then 20 unchanged full snapshots; identical published chain, zero additional write attempts, 100 allocator calls. |
| `map_marker::tests::native::unconfirmed_detach_never_authorizes_reuse` | Layer-switch detach read, write-before-effect, and effect-then-error failures; physical accounting retained; read retry succeeds; ambiguous writes prevent subsequent allocation/rewrite. |
| `map_marker::tests::native::preparation_and_publication_errors_quarantine_cells` | Field, link, and publication effect-then-error writes prevent later allocator calls and writes. Combines separate preparation/publication failure probes. |
| `map_marker::tests::native::partial_allocation_distinguishes_null_from_unknown_outcome` | Earlier allocated cells remain accounted for; confirmed NULL permits retry; unknown allocation outcome blocks retry. |
| `marker_scanner::tests::native::map_off_on_cycles_reuse_the_high_water_pool` | Real shared state/filter and scanner tick, 101 BFS candidates, ten map-off/on cycles, transient clear-read failure and retry on every cycle; allocation high-water stays 100. |
| `marker_scanner::tests::native::failed_clear_does_not_resurrect_removed_items` | Layer/root read failures and effect-then-error detach, item removal, re-enable; loading interval for the ambiguous-write case. Removed items remain absent with no new allocation. Player is far from items, so pickup-distance pruning cannot mask the regression. |
| `marker_scanner::tests::native::loading_and_session_reset_never_write_old_cells` | Player NULL and explicit session reset invalidate before further use; no teardown writes, stale/reassigned cell contents untouched, fresh cell used after re-entry. |

The seven `native` tests are Windows-only. They reserve zeroed pages below 2 GiB
using `VirtualAllocEx` in the **test process**, use actual `D2Context`,
`ProcessHandle`, and `SharedScannerState`, and execute real WinAPI reads/writes.
Fixture pages are released with `VirtualFreeEx` in Drop; independently opened
own-process handles use the existing `ProcessHandle` Drop. No Game.exe access.

`process.rs::marker_test_io` is a small `cfg(test)` thread-local fault/counting
scope restricted to the fixture address range. It restores the previous state
on Drop and cannot move across threads. `injection.rs` has a per-instance,
test-only allocator returning addresses in the fixture's owned pages; real
injectors default to no override. Its dummy buffers allocate no memory. There
are no global cross-test allocator variables or replacement crate-root modules.
All support is excluded from normal builds. The only new boundary statements
are cfg-gated test interception; existing PR12 production behavior is unchanged.

### Removed coverage limits

The old byte-memory fake's self-test and duplicate standalone driver are gone.
Individual public-flow probes for foreign-child tampering, leaf-read failure,
foreign insertion immediately before publication, layer changes during cell
writes, shutdown, failed player reads, and explicit quarantine-release/spare
reassignment sequences are not retained as separate scenarios. Original pure
chain-integrity, allocation-helper, eviction/newcomer, TTL, distance, hash, and
loading tests remain, but are not claimed as equivalent public-flow coverage.
The final publication guards remain in production; the smaller suite does not
simulate their interleavings. No native allocator execution, renderer race,
pool-generation/lifetime guarantee, or live-game validation is claimed.

### Verification and mutation evidence

Baseline from immediately before consolidation: standalone harness **49/0**,
full Cargo run **252 backend + 49 harness / 0 failures**, normal build exit 0.
The baseline was not rerun before these edits. Current commands from the root:

```text
cargo test --manifest-path src-tauri/Cargo.toml --locked --quiet marker
cargo test --manifest-path src-tauri/Cargo.toml --locked --quiet
cargo build --manifest-path src-tauri/Cargo.toml --locked --quiet
cargo test --manifest-path src-tauri/Cargo.toml --target i686-pc-windows-msvc --locked --quiet marker
```

Results: **29/0** for the `marker` filter (28 component tests plus one other
matching test), **259/0** full backend, normal build exit **0**, and **29/0**
for the full Tauri test binary's x86 marker-filter run. The old duplicated
integration target no longer exists. Existing compiler warnings remain.

Temporary mutations were applied only in this worktree with exact inverse
patches, then restored before final verification:

- Swallow the layer-switch detach error: public detach regression fails because
  tick incorrectly succeeds after the failed read.
- Remove first-seen stamps at cap eviction: both the pure and native full-BFS
  stability tests fail because the selected marker identities change.
- Delay logical-cache clearing until after fallible detach: scanner clear-intent
  test fails because the removed item remains published.
- Discard spares after successful clear: scanner high-water test fails on the
  first re-enable with **200 allocator calls instead of 100**.

The first three mutations together yielded **25 passed / 4 failed**; the
separate high-water mutation yielded **0 passed / 1 failed**. All are restored.
LSP error diagnostics are clean on all four changed Rust files. WinAPI fixture
calls were exercised on x64 and x86; Miri/sanitizer proof is not claimed (the
Miri availability command initiated nightly setup and timed out, and this
native FFI fixture is not a Miri-compatible execution surface).
