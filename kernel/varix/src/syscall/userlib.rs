//! AI-03 · F071 varix-std 雏形（内核侧视图）.
//!
//! `kernel/userspace/varix-std/` is the libc-free Rust user library: it owns the
//! inline-asm entry stubs and the typed wrappers a user program actually calls.
//! The kernel cannot depend on it (different target, opposite direction), so
//! this module carries the *contract*: the exact symbol set, the number each
//! wrapper binds to and the signature it presents.
//!
//! Keeping the table here is what makes F071 testable: the self-check asserts
//! that every implemented native call has a wrapper (or is on the documented
//! no-wrapper list, like the admin-only handle transfer), and that no wrapper
//! binds to a number the kernel does not implement. Drift between the library
//! and the kernel therefore fails the kernel's own self-test.

use super::table;

/// One user-side wrapper.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Wrapper {
    /// Exported Rust symbol in `varix_std`.
    pub symbol: &'static str,
    /// Kernel number it invokes.
    pub nr: u64,
    /// Signature as the user sees it.
    pub signature: &'static str,
    /// The `varix_std::Error` variant it returns, if any.
    pub error: &'static str,
}

pub const WRAPPERS: [Wrapper; 16] = [
    Wrapper { symbol: "read", nr: table::SYS_READ, signature: "fn read(fd: u32, buf: &mut [u8]) -> Result<usize>", error: "varix_std::Error" },
    Wrapper { symbol: "write", nr: table::SYS_WRITE, signature: "fn write(fd: u32, buf: &[u8]) -> Result<usize>", error: "varix_std::Error" },
    Wrapper { symbol: "open", nr: table::SYS_OPEN, signature: "fn open(path: &str, flags: u32) -> Result<u32>", error: "varix_std::Error" },
    Wrapper { symbol: "close", nr: table::SYS_CLOSE, signature: "fn close(fd: u32) -> Result<()>", error: "varix_std::Error" },
    Wrapper { symbol: "exit", nr: table::SYS_EXIT, signature: "fn exit(code: u8) -> !", error: "never" },
    Wrapper { symbol: "wait", nr: table::SYS_WAIT, signature: "fn wait(pid: u32) -> Result<ExitStatus>", error: "varix_std::Error" },
    Wrapper { symbol: "clock_gettime", nr: table::SYS_CLOCK, signature: "fn clock_gettime(id: ClockId) -> Result<(u64, u64)>", error: "varix_std::Error" },
    Wrapper { symbol: "mmap", nr: table::SYS_MMAP, signature: "fn mmap(len: usize, prot: Prot) -> Result<*mut u8>", error: "varix_std::Error" },
    Wrapper { symbol: "munmap", nr: table::SYS_MUNMAP, signature: "fn munmap(addr: *mut u8, len: usize) -> Result<()>", error: "varix_std::Error" },
    Wrapper { symbol: "mprotect", nr: table::SYS_MPROTECT, signature: "fn mprotect(addr: *mut u8, len: usize, prot: Prot) -> Result<()>", error: "varix_std::Error" },
    Wrapper { symbol: "event_ctl", nr: table::SYS_EVENT_CTL, signature: "fn event_ctl(port: u32, mask: u64) -> Result<()>", error: "varix_std::Error" },
    Wrapper { symbol: "event_wait", nr: table::SYS_EVENT_WAIT, signature: "fn event_wait(port: u32, out: &mut Event) -> Result<()>", error: "varix_std::Error" },
    Wrapper { symbol: "pipe", nr: table::SYS_PIPE, signature: "fn pipe() -> Result<(u32, u32)>", error: "varix_std::Error" },
    Wrapper { symbol: "dup", nr: table::SYS_DUP, signature: "fn dup(fd: u32, target: u32) -> Result<u32>", error: "varix_std::Error" },
    Wrapper { symbol: "abi", nr: table::SYS_ABI, signature: "fn abi() -> AbiInfo", error: "never" },
    Wrapper { symbol: "yield_now", nr: table::SYS_YIELD, signature: "fn yield_now()", error: "never" },
];

/// Native calls that deliberately have **no** user wrapper.
pub const NO_WRAPPER: [u64; 2] = [
    // Moves authority between processes: only a supervisor should reach it,
    // and through a supervisor library, not the general-purpose one.
    table::SYS_SEND_HANDLE,
    // Kernel log: debug builds only, exposed through `varix_std::debug`, not
    // part of the stable surface the kernel promises.
    table::SYS_LOG,
];

pub fn wrapper_for(nr: u64) -> Option<&'static Wrapper> {
    WRAPPERS.iter().find(|w| w.nr == nr)
}

pub fn wrapper_symbol(nr: u64) -> Option<&'static str> {
    wrapper_for(nr).map(|w| w.symbol)
}

pub fn has_wrapper(nr: u64) -> bool {
    wrapper_for(nr).is_some()
}

pub fn is_no_wrapper(nr: u64) -> bool {
    NO_WRAPPER.contains(&nr)
}

/// The stub body every wrapper compiles down to: place the number in RAX, the
/// arguments in the ABI registers and trap. Kept as text so the kernel-side
/// spec and the user-side source cannot drift.
pub const STUB_TEMPLATE: &str = "\
    core::arch::asm!(
        \"syscall\",
        inlateout(\"rax\") nr => ret,
        in(\"rdi\") a0, in(\"rsi\") a1, in(\"rdx\") a2,
        in(\"r10\") a3, in(\"r8\") a4, in(\"r9\") a5,
        lateout(\"rcx\") _, lateout(\"r11\") _,
    )";

/// Which calls the *strict* surface promises to keep stable forever.
pub const STABLE: [u64; 12] = [
    table::SYS_READ,
    table::SYS_WRITE,
    table::SYS_OPEN,
    table::SYS_CLOSE,
    table::SYS_EXIT,
    table::SYS_WAIT,
    table::SYS_CLOCK,
    table::SYS_MMAP,
    table::SYS_MUNMAP,
    table::SYS_MPROTECT,
    table::SYS_PIPE,
    table::SYS_ABI,
];

/// Which surface level a call belongs to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stability {
    Stable,
    Provisional,
    Internal,
}

pub fn stability_of(nr: u64) -> Stability {
    if STABLE.contains(&nr) {
        Stability::Stable
    } else if is_no_wrapper(nr) {
        Stability::Internal
    } else {
        Stability::Provisional
    }
}

pub fn stability_str(s: Stability) -> &'static str {
    match s {
        Stability::Stable => "stable",
        Stability::Provisional => "provisional",
        Stability::Internal => "internal",
    }
}

/// Every wrapper must bind to an implemented number, and the descriptor's
/// argument count must not exceed what the stub can carry.
pub fn wrapper_is_consistent(w: &Wrapper) -> bool {
    match table::lookup(w.nr) {
        Some(d) => d.argc as usize <= 6 && w.signature.starts_with("fn ") && !w.symbol.is_empty(),
        None => false,
    }
}

/// The calls that still need a wrapper, as a fixed list.
pub fn missing_wrappers(out: &mut [u64; 32]) -> usize {
    let mut n = 0;
    for d in table::SYSCALLS.iter() {
        if !has_wrapper(d.nr) && !is_no_wrapper(d.nr) && n < out.len() {
            out[n] = d.nr;
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f071_wrappers_match_the_kernel_surface() {
        assert!(WRAPPERS.len() >= 15);
        for w in WRAPPERS {
            assert!(wrapper_is_consistent(&w), "{} is inconsistent", w.symbol);
            assert!(
                table::is_implemented(w.nr),
                "{} binds to unimplemented {}",
                w.symbol,
                w.nr
            );
        }
        // No duplicate numbers, no duplicate symbols.
        for i in 0..WRAPPERS.len() {
            for j in (i + 1)..WRAPPERS.len() {
                assert_ne!(WRAPPERS[i].nr, WRAPPERS[j].nr);
                assert_ne!(WRAPPERS[i].symbol, WRAPPERS[j].symbol);
            }
        }
        // Every implemented native call is either wrapped or explicitly not.
        let mut missing = [0u64; 32];
        let n = missing_wrappers(&mut missing);
        assert_eq!(n, 0, "uncovered calls: {:?}", &missing[..n]);
        // The two exceptions are real and intentional.
        assert!(is_no_wrapper(table::SYS_SEND_HANDLE));
        assert!(!has_wrapper(table::SYS_SEND_HANDLE));
        assert!(is_no_wrapper(table::SYS_LOG));
        assert_eq!(wrapper_symbol(table::SYS_WRITE), Some("write"));
        assert_eq!(wrapper_for(0x9999), None);
        assert!(STUB_TEMPLATE.contains("syscall"));
        assert!(STUB_TEMPLATE.contains("rcx"));
    }

    #[test]
    fn f071_stability_levels_are_assigned() {
        assert_eq!(stability_of(table::SYS_READ), Stability::Stable);
        assert_eq!(stability_of(table::SYS_SEND_HANDLE), Stability::Internal);
        assert_eq!(stability_of(table::SYS_EVENT_CTL), Stability::Provisional);
        assert_eq!(stability_str(Stability::Stable), "stable");
        for nr in STABLE {
            assert!(table::is_implemented(nr), "stable {nr} must exist");
            assert!(has_wrapper(nr), "stable {nr} must be wrapped");
        }
    }
}
