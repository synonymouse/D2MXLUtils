// --- Linux ptrace: PTRACE_POKEDATA write fallback + remote-call primitive ---
// (remote-call/register plumbing for `injection/` lives here so it can
// share the 32-bit register-layout handling with the write fallback.)
use nix::sys::ptrace;
use nix::sys::signal::Signal;
use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::Pid;
use std::mem;

fn attach_hint(e: impl std::fmt::Display) -> String {
    format!("PTRACE_ATTACH failed: {e} (try: sudo sysctl kernel.yama.ptrace_scope=0)")
}

fn attach_and_wait(tid: Pid) -> Result<(), String> {
    ptrace::attach(tid).map_err(attach_hint)?;
    match waitpid(tid, None) {
        Ok(WaitStatus::Stopped(_, _)) => Ok(()),
        other => {
            let _ = ptrace::detach(tid, None);
            Err(format!("unexpected wait status after attach: {:?}", other))
        }
    }
}

/// Every thread id currently in the process, via `/proc/<pid>/task/`.
/// Falls back to `[pid]` if the listing can't be read (e.g. the process
/// just exited) — `call_remote` treats a failed attach on any candidate
/// as "try the next one", so an empty/stale list just degrades to the
/// single-thread behavior rather than erroring outright.
fn list_thread_ids(pid: u32) -> Vec<i32> {
    let tids: Vec<i32> = std::fs::read_dir(format!("/proc/{}/task", pid))
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().to_str()?.parse::<i32>().ok())
        .collect();
    if tids.is_empty() {
        vec![pid as i32]
    } else {
        tids
    }
}

/// `process_vm_writev` — not wrapped by `nix`, so a raw syscall.
pub fn process_vm_writev(pid: u32, address: usize, buffer: &[u8]) -> Result<(), String> {
    #[repr(C)]
    struct IoVec {
        iov_base: *const u8,
        iov_len: usize,
    }
    let local = IoVec {
        iov_base: buffer.as_ptr(),
        iov_len: buffer.len(),
    };
    let remote = IoVec {
        iov_base: address as *const u8,
        iov_len: buffer.len(),
    };
    let n = unsafe {
        libc::syscall(
            libc::SYS_process_vm_writev,
            pid as libc::pid_t,
            &local as *const IoVec,
            1usize,
            &remote as *const IoVec,
            1usize,
            0usize,
        )
    };
    if n == buffer.len() as i64 {
        Ok(())
    } else {
        Err(format!(
            "process_vm_writev failed (ret={}, errno={})",
            n,
            std::io::Error::last_os_error()
        ))
    }
}

/// Word-at-a-time `PTRACE_POKEDATA` write, assuming `tid` is already
/// ptrace-attached and stopped. Bypasses page protection the same way
/// debuggers plant `0xCC` breakpoints into `.text` — needed for writing
/// the shellcode stubs into `D2Client.dll`'s mapped image.
fn poke_write_attached(tid: Pid, address: usize, buffer: &[u8]) -> Result<(), String> {
    let word_size = mem::size_of::<usize>();
    let mut offset = 0usize;
    while offset < buffer.len() {
        let word_addr = address + offset;
        // Read-modify-write so a write shorter than a full word doesn't
        // clobber trailing bytes we didn't intend to touch.
        let existing = ptrace::read(tid, word_addr as *mut _)
            .map_err(|e| format!("PTRACE_PEEKDATA failed: {}", e))?;
        let mut word_bytes = existing.to_ne_bytes();
        let remaining = buffer.len() - offset;
        let take = remaining.min(word_size);
        word_bytes[..take].copy_from_slice(&buffer[offset..offset + take]);
        let new_word = i64::from_ne_bytes(word_bytes);
        unsafe {
            ptrace::write(tid, word_addr as *mut _, new_word)
                .map_err(|e| format!("PTRACE_POKEDATA failed: {}", e))?;
        }
        offset += take;
    }
    Ok(())
}

pub fn poke_write(pid: u32, address: usize, buffer: &[u8]) -> Result<(), String> {
    let tid = Pid::from_raw(pid as i32);
    attach_and_wait(tid)?;
    let result = poke_write_attached(tid, address, buffer);
    let _ = ptrace::detach(tid, None);
    result
}

/// The game process is a genuine **64-bit** ELF (confirmed empirically:
/// raw `PTRACE_GETREGS` returns the full 216-byte/27-field native
/// `libc::user_regs_struct`, not a 32-bit-compat 68-byte one) — Wine
/// runs the 32-bit Windows code by switching the CPU into legacy
/// compatibility mode (`cs = 0x23`, `ss/ds/es = 0x2b`: the classic Linux
/// `__USER32_CS`/`__USER32_DS` selectors) within that same 64-bit task,
/// rather than via the kernel's ia32-compat *task* mode. So `nix`'s
/// typed `getregs`/`setregs` (host-native x86_64 layout) are exactly
/// right here — the earlier hand-rolled 32-bit struct was wrong for
/// this specific Wine build. `rip`/`rsp`/`rbx`/`rax` only need their low
/// 32 bits touched (zero-extended); `cs`/`ss`/`ds`/`es` must be left
/// completely untouched so the CPU stays in 32-bit compat mode for our
/// injected call.
fn getregs(tid: Pid) -> Result<libc::user_regs_struct, String> {
    ptrace::getregs(tid).map_err(|e| format!("PTRACE_GETREGS failed: {}", e))
}

fn setregs(tid: Pid, regs: &libc::user_regs_struct) -> Result<(), String> {
    ptrace::setregs(tid, *regs).map_err(|e| format!("PTRACE_SETREGS failed: {}", e))
}

/// Retries this many times to catch the target thread actually
/// executing userspace code rather than blocked mid-syscall (see
/// `call_remote`'s doc comment) before giving up.
const ATTACH_RETRY_LIMIT: u32 = 100;

/// Linux analog of `CreateRemoteThread(func_addr, param)` +
/// `WaitForSingleObject(INFINITE)` + `GetExitCodeThread`. `ptrace` has
/// no "spawn a thread" primitive, so this hijacks the target's own
/// thread execution context instead of creating a new one: freeze it,
/// redirect `EIP`=`func_addr`/`EBX`=`param` (matches the existing
/// shellcode's calling convention) with `ESP` backed onto the thread's
/// *own already-mapped* stack (offset down from its live value — no
/// separate scratch allocation needed), and push a fake return address
/// pointing at an unmapped page. Every existing shellcode stub already
/// ends in a bare `ret`, so it pops that address and immediately
/// SIGSEGVs — our signal that the call finished, with `EAX` already
/// holding the return value the callee set before `ret`. Restore the
/// original registers exactly and detach, even on an error path.
///
/// Empirically (see `process::live_probe::minimal_int3_hijack`),
/// `PTRACE_ATTACH` can catch a thread mid-syscall (deep in libc/vdso,
/// e.g. blocked in `futex`/`poll`) — forcing a new `rip` in that state
/// does not behave like a clean userspace jump (the thread faults a
/// couple of bytes off from wherever the kernel's syscall-return path
/// was really headed, not at our intended address at all). So after
/// attaching we require `rip` to already be within shouting distance of
/// `func_addr` (i.e. actually executing D2Client.dll code, not kernel/
/// libc internals) before touching any registers, retrying the
/// attach/detach cycle otherwise.
pub fn call_remote(pid: u32, func_addr: usize, param: usize) -> Result<u32, String> {
    // Cycle through every thread in the process each round instead of
    // sleep-retrying a single fixed tid: the game (~25 threads in
    // practice) almost always has *some* thread actively running
    // userspace code at any instant, even when any one particular
    // thread is off blocked in a syscall. Trying them all before
    // sleeping converges far faster than waiting for one specific
    // thread's turn to come back around.
    let tids = list_thread_ids(pid);

    let (tid, orig_regs) = {
        let mut found = None;
        'rounds: for _ in 0..ATTACH_RETRY_LIMIT {
            for &raw_tid in &tids {
                let candidate = Pid::from_raw(raw_tid);
                if attach_and_wait(candidate).is_err() {
                    continue; // thread may have exited; try the next one
                }
                let r = match getregs(candidate) {
                    Ok(r) => r,
                    Err(_) => {
                        let _ = ptrace::detach(candidate, None);
                        continue;
                    }
                };
                if (r.rip as usize).abs_diff(func_addr) < 0x0200_0000 {
                    found = Some((candidate, r));
                    break 'rounds;
                }
                let _ = ptrace::detach(candidate, None);
            }
            // A full pass over every thread came up empty — give them a
            // moment to make progress before trying again.
            std::thread::sleep(std::time::Duration::from_micros(200));
        }
        found.ok_or_else(|| "gave up waiting for target thread to leave a syscall".to_string())?
    };

    let result = (|| -> Result<u32, String> {
        let mut call_regs = orig_regs;
        call_regs.rip = func_addr as u64;
        call_regs.rbx = param as u64;

        // Fake return address: page 0 is never mapped, so landing here
        // faults immediately rather than executing garbage.
        const FAKE_RETURN: u32 = 0x1;
        let new_esp = (orig_regs.rsp as u32).wrapping_sub(0x400) & !0xF;
        call_regs.rsp = new_esp as u64;

        poke_write_attached(tid, new_esp as usize, &FAKE_RETURN.to_le_bytes())?;
        setregs(tid, &call_regs)?;
        ptrace::cont(tid, None).map_err(|e| format!("PTRACE_CONT failed: {}", e))?;

        match waitpid(tid, None) {
            Ok(WaitStatus::Stopped(_, sig)) if sig == Signal::SIGSEGV || sig == Signal::SIGTRAP => {
                let result_regs = getregs(tid)?;
                Ok(result_regs.rax as u32)
            }
            other => Err(format!(
                "unexpected wait status after remote call: {:?}",
                other
            )),
        }
    })();

    // Restore exactly, unconditionally — even if the call above failed
    // partway through (e.g. timed out mid-flight).
    let _ = setregs(tid, &orig_regs);
    let _ = ptrace::detach(tid, None);

    result
}
