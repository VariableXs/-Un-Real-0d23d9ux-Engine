//! AI-03 · F053 号表治理 / F069 ABI 版本查询 / F070 向后兼容层.
//!
//! The syscall number space is *registered*, not scattered. One static table
//! carries number, name, arity, required capability bits and whether the call
//! has a lock-free fast path. Everything else in this domain — the compiler,
//! the capability gate, the audit filter, the fuzzer, the ABI probe — reads
//! this table instead of hard-coding numbers.
//!
//! Numbers live in bands so evolution never collides:
//!   `0x0000..0x3FFF` reserved (never issued)   `0x4000..0x40FF` Varix native
//!   `0x4100..0x41FF` Linux compat slots        `0x4200..0x7FFF` vendor/experimental
//!
//! Not-implemented numbers are answered with `ENOSYS` at the table level, before
//! any argument is touched — the caller never sees a partially-handled call.

use super::errno::Errno;
use super::guard::{CAP_ADMIN, CAP_CONSOLE, CAP_DEBUG, CAP_EVENT, CAP_FS, CAP_MEM, CAP_NONE, CAP_PROC, CAP_TIME};

// ---------------------------------------------------------------------------
// 号段划分（F053）
// ---------------------------------------------------------------------------

pub const SYS_NR_BASE: u64 = 0x4000;
/// Highest registerable number (vendor band included).
pub const SYS_NR_MAX: u64 = 0x7FFF;
pub const BAND_RESERVED_HI: u64 = 0x3FFF;
pub const BAND_NATIVE_LO: u64 = 0x4000;
pub const BAND_NATIVE_HI: u64 = 0x40FF;
pub const BAND_COMPAT_LO: u64 = 0x4100;
pub const BAND_COMPAT_HI: u64 = 0x41FF;
pub const BAND_VENDOR_LO: u64 = 0x4200;

/// Which band a number falls into (F053).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NrBand {
    Reserved,
    Native,
    Compat,
    Vendor,
    Out,
}

impl NrBand {
    pub const fn as_str(self) -> &'static str {
        match self {
            NrBand::Reserved => "reserved",
            NrBand::Native => "native",
            NrBand::Compat => "compat",
            NrBand::Vendor => "vendor",
            NrBand::Out => "out-of-range",
        }
    }
}

pub const fn band_of(nr: u64) -> NrBand {
    if nr <= BAND_RESERVED_HI {
        NrBand::Reserved
    } else if nr <= BAND_NATIVE_HI {
        NrBand::Native
    } else if nr <= BAND_COMPAT_HI {
        NrBand::Compat
    } else if nr <= SYS_NR_MAX {
        NrBand::Vendor
    } else {
        NrBand::Out
    }
}

// ---------------------------------------------------------------------------
// 调用号常量（F053）—— 原生号 = SYS_NR_BASE + 偏移
// ---------------------------------------------------------------------------

pub const SYS_READ: u64 = SYS_NR_BASE + 0;
pub const SYS_WRITE: u64 = SYS_NR_BASE + 1;
pub const SYS_OPEN: u64 = SYS_NR_BASE + 2;
pub const SYS_CLOSE: u64 = SYS_NR_BASE + 3;
pub const SYS_EXIT: u64 = SYS_NR_BASE + 4;
pub const SYS_WAIT: u64 = SYS_NR_BASE + 5;
pub const SYS_CLOCK: u64 = SYS_NR_BASE + 6;
pub const SYS_MMAP: u64 = SYS_NR_BASE + 7;
pub const SYS_MUNMAP: u64 = SYS_NR_BASE + 8;
pub const SYS_MPROTECT: u64 = SYS_NR_BASE + 9;
pub const SYS_EVENT_CTL: u64 = SYS_NR_BASE + 10;
pub const SYS_EVENT_WAIT: u64 = SYS_NR_BASE + 11;
pub const SYS_PIPE: u64 = SYS_NR_BASE + 12;
pub const SYS_DUP: u64 = SYS_NR_BASE + 13;
pub const SYS_SEND_HANDLE: u64 = SYS_NR_BASE + 14;
pub const SYS_ABI: u64 = SYS_NR_BASE + 15;
pub const SYS_YIELD: u64 = SYS_NR_BASE + 16;
pub const SYS_LOG: u64 = SYS_NR_BASE + 17;

/// One registered call.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SyscallDesc {
    pub nr: u64,
    pub name: &'static str,
    /// Number of meaningful arguments (0..=6).
    pub argc: u8,
    /// Capability bits required before the body runs (F063).
    pub caps: u64,
    /// Has a lock-free fast path (F066).
    pub fast: bool,
    /// Landed in the sensitive-audit set (F064).
    pub audited: bool,
}

/// The table, in ascending number order. `lookup` binary-searches it, so the
/// order is part of the contract (asserted by the self-test).
pub const SYSCALLS: [SyscallDesc; 18] = [
    SyscallDesc { nr: SYS_READ, name: "read", argc: 3, caps: CAP_FS, fast: false, audited: false },
    SyscallDesc { nr: SYS_WRITE, name: "write", argc: 3, caps: CAP_CONSOLE, fast: false, audited: true },
    SyscallDesc { nr: SYS_OPEN, name: "open", argc: 3, caps: CAP_FS, fast: false, audited: true },
    SyscallDesc { nr: SYS_CLOSE, name: "close", argc: 1, caps: CAP_NONE, fast: false, audited: false },
    SyscallDesc { nr: SYS_EXIT, name: "exit", argc: 1, caps: CAP_NONE, fast: false, audited: true },
    SyscallDesc { nr: SYS_WAIT, name: "wait", argc: 2, caps: CAP_PROC, fast: false, audited: true },
    SyscallDesc { nr: SYS_CLOCK, name: "clock_gettime", argc: 2, caps: CAP_TIME, fast: true, audited: false },
    SyscallDesc { nr: SYS_MMAP, name: "mmap", argc: 6, caps: CAP_MEM, fast: false, audited: true },
    SyscallDesc { nr: SYS_MUNMAP, name: "munmap", argc: 2, caps: CAP_MEM, fast: false, audited: false },
    SyscallDesc { nr: SYS_MPROTECT, name: "mprotect", argc: 3, caps: CAP_MEM, fast: false, audited: true },
    SyscallDesc { nr: SYS_EVENT_CTL, name: "event_ctl", argc: 3, caps: CAP_EVENT, fast: false, audited: false },
    SyscallDesc { nr: SYS_EVENT_WAIT, name: "event_wait", argc: 3, caps: CAP_EVENT, fast: true, audited: false },
    SyscallDesc { nr: SYS_PIPE, name: "pipe", argc: 2, caps: CAP_PROC, fast: false, audited: false },
    SyscallDesc { nr: SYS_DUP, name: "dup", argc: 2, caps: CAP_NONE, fast: false, audited: false },
    SyscallDesc { nr: SYS_SEND_HANDLE, name: "send_handle", argc: 3, caps: CAP_ADMIN, fast: false, audited: true },
    SyscallDesc { nr: SYS_ABI, name: "abi_query", argc: 1, caps: CAP_NONE, fast: true, audited: false },
    SyscallDesc { nr: SYS_YIELD, name: "yield", argc: 0, caps: CAP_NONE, fast: true, audited: false },
    SyscallDesc { nr: SYS_LOG, name: "log", argc: 2, caps: CAP_DEBUG, fast: false, audited: true },
];

pub const SYSCALL_COUNT: usize = SYSCALLS.len();

/// Binary search — the table is sorted by `nr`.
pub fn lookup(nr: u64) -> Option<&'static SyscallDesc> {
    let mut lo = 0usize;
    let mut hi = SYSCALLS.len();
    while lo < hi {
        let mid = (lo + hi) / 2;
        let n = SYSCALLS[mid].nr;
        if n == nr {
            return Some(&SYSCALLS[mid]);
        } else if n < nr {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    None
}

pub fn name(nr: u64) -> &'static str {
    match lookup(nr) {
        Some(d) => d.name,
        None => "unknown",
    }
}

pub fn name_of_band(nr: u64) -> &'static str {
    band_of(nr).as_str()
}

pub fn arity(nr: u64) -> Option<u8> {
    lookup(nr).map(|d| d.argc)
}

pub fn required_caps(nr: u64) -> u64 {
    match lookup(nr) {
        Some(d) => d.caps,
        None => CAP_ADMIN, // 未知号一律要求最高权限，绝不放行
    }
}

/// The F053 verdict for a number: implemented / unknown-but-in-range / out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NrVerdict {
    Implemented(&'static SyscallDesc),
    /// Inside a declared band but with no entry yet — `ENOSYS` today.
    NotYet(u64),
    /// Outside every band — a caller bug, still answered with `ENOSYS`.
    Unknown,
}

/// Resolve a number to its verdict (F053). Never returns a body for an
/// unregistered number, and never consults arguments first.
pub fn resolve(nr: u64) -> NrVerdict {
    if let Some(d) = lookup(nr) {
        return NrVerdict::Implemented(d);
    }
    match band_of(nr) {
        NrBand::Out | NrBand::Reserved => NrVerdict::Unknown,
        _ => NrVerdict::NotYet(nr),
    }
}

/// Every unregistered number is answered with exactly this error.
pub const fn unimplemented_errno() -> Errno {
    Errno::NoSys
}

pub fn is_implemented(nr: u64) -> bool {
    matches!(resolve(nr), NrVerdict::Implemented(_))
}

// ---------------------------------------------------------------------------
// F069 — ABI 版本查询
// ---------------------------------------------------------------------------

/// Bumped whenever the call surface changes incompatibly.
pub const ABI_VERSION: u32 = 1;
/// Oldest ABI version this kernel still serves (see F070).
pub const ABI_VERSION_MIN: u32 = 1;

pub const FEAT_CAPS: u64 = 1 << 0;
pub const FEAT_AUDIT: u64 = 1 << 1;
pub const FEAT_QUOTA: u64 = 1 << 2;
pub const FEAT_SECCOMP: u64 = 1 << 3;
pub const FEAT_VDSO: u64 = 1 << 4;
pub const FEAT_COMPAT: u64 = 1 << 5;
pub const FEAT_PIPES: u64 = 1 << 6;
pub const FEAT_EVENTPORT: u64 = 1 << 7;
pub const FEAT_HANDLE_PASSING: u64 = 1 << 8;
pub const FEAT_FASTPATH: u64 = 1 << 9;
pub const FEAT_METER: u64 = 1 << 10;

/// Everything this kernel advertises.
pub const ABI_FEATURES: u64 = FEAT_CAPS
    | FEAT_AUDIT
    | FEAT_QUOTA
    | FEAT_SECCOMP
    | FEAT_VDSO
    | FEAT_COMPAT
    | FEAT_PIPES
    | FEAT_EVENTPORT
    | FEAT_HANDLE_PASSING
    | FEAT_FASTPATH
    | FEAT_METER;

/// What `SYS_ABI` hands back. Fixed layout, no pointers — the probe must work
/// before the caller trusts any user memory.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AbiInfo {
    pub version: u32,
    pub version_min: u32,
    pub nr_base: u64,
    pub nr_max: u64,
    pub nr_count: u32,
    pub features: u64,
    pub page_size: u64,
    /// Word size of the calling process (must match before any pointer moves).
    pub ptr_size: u8,
    pub endian: u8,
}

pub const PTR_SIZE_64: u8 = 8;
pub const ENDIAN_LITTLE: u8 = 0;

pub const fn abi_info(page_size: u64) -> AbiInfo {
    AbiInfo {
        version: ABI_VERSION,
        version_min: ABI_VERSION_MIN,
        nr_base: SYS_NR_BASE,
        nr_max: SYS_NR_MAX,
        nr_count: SYSCALL_COUNT as u32,
        features: ABI_FEATURES,
        page_size,
        ptr_size: PTR_SIZE_64,
        endian: ENDIAN_LITTLE,
    }
}

/// `SYS_ABI` handler body (F069): the caller states the version it was built
/// against, we answer with either the matching info or `ENOTSUP`.
pub fn abi_negotiate(requested: u32, ptr_size: u8, page_size: u64) -> Result<AbiInfo, Errno> {
    if ptr_size != PTR_SIZE_64 {
        return Err(Errno::NotSup);
    }
    if requested < ABI_VERSION_MIN {
        return Err(Errno::NotSup);
    }
    // A caller from the future is refused: it would guess at fields we do not
    // have yet, which is exactly how silent ABI drift starts.
    if requested > ABI_VERSION {
        return Err(Errno::Inval);
    }
    Ok(abi_info(page_size))
}

/// Feature probe without a full negotiation (cheap, used by vDSO setup).
pub const fn has_feature(bits: u64) -> bool {
    bits & ABI_FEATURES == bits
}

// ---------------------------------------------------------------------------
// F070 — 向后兼容层
// ---------------------------------------------------------------------------

/// Linux/x86-64 number → Varix number. Deliberately partial: only the calls
/// whose *semantics* are a subset get translated. Everything else is ENOSYS
/// rather than a guess.
pub const COMPAT_TABLE: [(u64, u64); 15] = [
    (0, SYS_READ),
    (1, SYS_WRITE),
    (2, SYS_OPEN),
    (3, SYS_CLOSE),
    (9, SYS_MMAP),
    (10, SYS_MPROTECT),
    (11, SYS_MUNMAP),
    (22, SYS_PIPE),
    (24, SYS_YIELD),
    (32, SYS_DUP),
    (33, SYS_DUP),       // dup2 → dup (旧号转接)
    (60, SYS_EXIT),
    (61, SYS_WAIT),      // wait4 → wait
    (228, SYS_CLOCK),    // clock_gettime
    (231, SYS_EXIT),     // exit_group → exit
];

/// How a translated call differs from its native twin, so the shim can fix up
/// the argument list instead of pretending the ABIs are identical.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RouteNote {
    /// Same signature.
    Exact,
    /// Target takes fewer arguments; extras are ignored.
    ExtraArgsIgnored,
    /// Argument order differs and the shim must permute.
    Permuted,
    /// Return layout differs.
    ReturnDiffers,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CompatRoute {
    pub linux_nr: u64,
    pub varix_nr: u64,
    pub note: RouteNote,
    pub deprecated: bool,
}

/// Translate a legacy number (F070).
pub fn compat_translate(linux_nr: u64) -> Option<CompatRoute> {
    for &(l, v) in COMPAT_TABLE.iter() {
        if l == linux_nr {
            let note = match l {
                // `open` in the compat ABI takes (path, flags, mode) while the
                // native one takes (path, flags, mode, attr) — close enough to
                // treat as exact for now, but `dup2` really is dup-with-target.
                33 => RouteNote::Permuted,
                61 => RouteNote::ExtraArgsIgnored, // wait4(status, options, rusage)
                60 | 231 => RouteNote::Exact,
                _ => RouteNote::Exact,
            };
            return Some(CompatRoute {
                linux_nr: l,
                varix_nr: v,
                note,
                deprecated: l == 33,
            });
        }
    }
    None
}

/// Normative path for any incoming number: native bands pass straight through,
/// the compat band is translated, and anything else is refused with ENOSYS.
pub fn route_incoming(nr: u64) -> Result<CompatRoute, Errno> {
    match band_of(nr) {
        NrBand::Native | NrBand::Vendor => {
            if is_implemented(nr) {
                Ok(CompatRoute {
                    linux_nr: nr,
                    varix_nr: nr,
                    note: RouteNote::Exact,
                    deprecated: false,
                })
            } else {
                // In a declared band with no entry: still ENOSYS, but the audit
                // log records it as a *band miss* rather than a garbage number.
                Err(Errno::NoSys)
            }
        }
        NrBand::Compat => {
            let legacy = nr - BAND_COMPAT_LO;
            match compat_translate(legacy) {
                Some(mut r) => {
                    r.linux_nr = nr; // remember what actually arrived
                    Ok(r)
                }
                None => Err(Errno::NoSys),
            }
        }
        NrBand::Reserved | NrBand::Out => Err(Errno::NoSys),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f053_table_is_sorted_and_complete() {
        assert!(SYSCALL_COUNT >= 15);
        for w in SYSCALLS.windows(2) {
            assert!(w[0].nr < w[1].nr, "table must ascend: {} {}", w[0].nr, w[1].nr);
        }
        for d in SYSCALLS {
            assert_eq!(band_of(d.nr), NrBand::Native, "{}", d.name);
            assert_eq!(lookup(d.nr).map(|x| x.name), Some(d.name));
            assert!(d.argc <= 6);
        }
        assert_eq!(name(SYS_WRITE), "write");
        assert_eq!(name(0x7FFF), "unknown");
        assert_eq!(arity(SYS_MMAP), Some(6));
        assert_eq!(arity(0x7777), None);
    }

    #[test]
    fn f053_unknown_numbers_are_enosys_not_handled() {
        assert_eq!(unimplemented_errno(), Errno::NoSys);
        // Reserved band, in-range-but-unregistered, and wildly out of range all
        // land on the same refusal.
        assert_eq!(resolve(0x100), NrVerdict::Unknown);
        assert_eq!(resolve(SYS_NR_BASE + 200), NrVerdict::NotYet(SYS_NR_BASE + 200));
        assert_eq!(resolve(0xFFFF_FFFF), NrVerdict::Unknown);
        assert!(!is_implemented(0x100));
        assert!(matches!(resolve(SYS_READ), NrVerdict::Implemented(_)));
        // Unknown numbers demand admin capability: fail closed.
        assert_eq!(required_caps(0x9999), CAP_ADMIN);
        assert_eq!(band_of(0x4100), NrBand::Compat);
        assert_eq!(band_of(0x5000), NrBand::Vendor);
    }

    #[test]
    fn f069_abi_negotiation_refuses_mismatches() {
        let info = abi_negotiate(ABI_VERSION, PTR_SIZE_64, 4096).unwrap();
        assert_eq!(info.version, ABI_VERSION);
        assert_eq!(info.nr_base, SYS_NR_BASE);
        assert_eq!(info.nr_count as usize, SYSCALL_COUNT);
        assert!(has_feature(FEAT_VDSO | FEAT_SECCOMP | FEAT_COMPAT));
        assert!(!has_feature(1 << 62));
        // 32-bit or future-version callers do not get to guess.
        assert_eq!(abi_negotiate(ABI_VERSION, 4, 4096), Err(Errno::NotSup));
        assert_eq!(abi_negotiate(ABI_VERSION + 1, 8, 4096), Err(Errno::Inval));
        assert_eq!(abi_negotiate(0, 8, 4096), Err(Errno::NotSup));
    }

    #[test]
    fn f070_compat_layer_translates_and_refuses() {
        assert_eq!(compat_translate(1).unwrap().varix_nr, SYS_WRITE);
        assert_eq!(compat_translate(231).unwrap().varix_nr, SYS_EXIT);
        assert!(compat_translate(33).unwrap().deprecated);
        assert_eq!(compat_translate(33).unwrap().note, RouteNote::Permuted);
        assert_eq!(compat_translate(61).unwrap().note, RouteNote::ExtraArgsIgnored);
        // Not in the legacy set → refused, never guessed at.
        assert!(compat_translate(157).is_none());
        assert_eq!(route_incoming(0x4100 + 1).unwrap().varix_nr, SYS_WRITE);
        assert_eq!(route_incoming(0x4100 + 157), Err(Errno::NoSys));
        assert_eq!(route_incoming(SYS_PIPE).unwrap().varix_nr, SYS_PIPE);
        assert_eq!(route_incoming(0x55), Err(Errno::NoSys));
        assert_eq!(route_incoming(0x40FF), Err(Errno::NoSys));
    }
}
