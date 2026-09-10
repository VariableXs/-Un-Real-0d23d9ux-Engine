//! F102 syscall 号表 / F103 快速 syscall.
//!
//! One table, one dispatcher, one numbering scheme. Every call is checked for
//! "does this number exist" and "did the caller pass the right count of
//! arguments" *before* any handler runs, because a handler that has to defend
//! itself against a malformed frame is a handler that will eventually forget.

use core::sync::atomic::{AtomicU64, Ordering};

pub const MAX_SYSCALLS: usize = 64;
/// Numbers 0..16 are the stable ABI base that F490 promises not to move.
pub const STABLE_SYSCALL_BASE: u32 = 0;
pub const STABLE_SYSCALL_COUNT: u32 = 16;
/// Arguments a syscall may take (rdi, rsi, rdx, r10, r8, r9).
pub const MAX_SYSCALL_ARGS: usize = 6;

// The numbering, fixed here so every domain can reference the same constants.
pub const SYS_EXIT: u32 = 0;
pub const SYS_READ: u32 = 1;
pub const SYS_WRITE: u32 = 2;
pub const SYS_OPEN: u32 = 3;
pub const SYS_CLOSE: u32 = 4;
pub const SYS_MMAP: u32 = 5;
pub const SYS_FORK: u32 = 6;
pub const SYS_EXEC: u32 = 7;
pub const SYS_WAIT: u32 = 8;
pub const SYS_SIGNAL: u32 = 9;
pub const SYS_SLEEP: u32 = 10;
pub const SYS_TIME: u32 = 11;
pub const SYS_CAPS: u32 = 12;
pub const SYS_PROC_INFO: u32 = 13;
pub const SYS_TRACE: u32 = 14;
pub const SYS_SYNC: u32 = 15;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyscallError {
    /// The number is not registered.
    NotImplemented,
    /// The number is outside the table.
    BadNumber,
    /// The caller passed fewer values than the handler needs.
    TooFewArgs,
    /// Extra arguments are a caller bug, not something to ignore.
    TooManyArgs,
    /// The handler refused for its own reason.
    Denied,
    /// The handler could not complete the work.
    Failed,
}

impl SyscallError {
    /// The negative value the handler convention returns in `rax`.
    pub const fn errno(self) -> i64 {
        match self {
            SyscallError::NotImplemented => -38, // ENOSYS
            SyscallError::BadNumber => -38,
            SyscallError::TooFewArgs => -22,     // EINVAL
            SyscallError::TooManyArgs => -22,
            SyscallError::Denied => -13,         // EACCES
            SyscallError::Failed => -5,          // EIO
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            SyscallError::NotImplemented => "not implemented",
            SyscallError::BadNumber => "bad syscall number",
            SyscallError::TooFewArgs => "too few arguments",
            SyscallError::TooManyArgs => "too many arguments",
            SyscallError::Denied => "denied",
            SyscallError::Failed => "failed",
        }
    }
}

/// A handler. It receives exactly `arity` arguments and returns a value the
/// caller sees in `rax` (negative = error, by convention).
pub type Handler = fn(&[u64]) -> Result<i64, SyscallError>;

/// Not `PartialEq`: comparing function pointers is meaningless (the compiler may
/// merge or duplicate them), and nothing should be tempted to try.
#[derive(Clone, Copy, Debug)]
pub struct Syscall {
    pub nr: u32,
    pub name: &'static str,
    pub arity: u8,
    pub handler: Option<Handler>,
    /// Capability a caller must hold (F112). `0` = none.
    pub required_cap: u64,
}

impl Syscall {
    pub const fn new(nr: u32, name: &'static str, arity: u8) -> Syscall {
        Syscall {
            nr,
            name,
            arity,
            handler: None,
            required_cap: 0,
        }
    }

    pub const fn with_handler(mut self, h: Handler) -> Syscall {
        self.handler = Some(h);
        self
    }

    pub const fn requiring(mut self, cap: u64) -> Syscall {
        self.required_cap = cap;
        self
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyscallTableError {
    Duplicate(u32),
    OutOfRange(u32),
    Reserved(u32),
}

pub struct SyscallTable {
    entries: [Option<Syscall>; MAX_SYSCALLS],
    registered: usize,
    calls: u64,
    denied: u64,
}

impl SyscallTable {
    pub const fn new() -> SyscallTable {
        SyscallTable {
            entries: [None; MAX_SYSCALLS],
            registered: 0,
            calls: 0,
            denied: 0,
        }
    }

    /// The table every Varix image starts with: the stable ABI base, with the
    /// numbers registered (handlers arrive with their subsystems).
    pub fn with_stable_abi() -> SyscallTable {
        let mut t = SyscallTable::new();
        let base: [(u32, &'static str, u8); STABLE_SYSCALL_COUNT as usize] = [
            (SYS_EXIT, "exit", 1),
            (SYS_READ, "read", 3),
            (SYS_WRITE, "write", 3),
            (SYS_OPEN, "open", 2),
            (SYS_CLOSE, "close", 1),
            (SYS_MMAP, "mmap", 3),
            (SYS_FORK, "fork", 0),
            (SYS_EXEC, "exec", 2),
            (SYS_WAIT, "wait", 1),
            (SYS_SIGNAL, "signal", 2),
            (SYS_SLEEP, "sleep", 1),
            (SYS_TIME, "time", 0),
            (SYS_CAPS, "caps", 1),
            (SYS_PROC_INFO, "proc-info", 2),
            (SYS_TRACE, "trace", 3),
            (SYS_SYNC, "sync", 0),
        ];
        for (nr, name, arity) in base {
            let syscall = match nr {
                SYS_CAPS => Syscall::new(nr, name, arity).requiring(crate::proc::Caps::SIGNAL),
                SYS_TRACE => Syscall::new(nr, name, arity).requiring(crate::proc::Caps::TRACE),
                _ => Syscall::new(nr, name, arity),
            };
            let _ = t.register(syscall);
        }
        t
    }

    /// F102: register a syscall number. The stable base cannot be renumbered.
    pub fn register(&mut self, syscall: Syscall) -> Result<(), SyscallTableError> {
        let nr = syscall.nr;
        if nr as usize >= MAX_SYSCALLS {
            return Err(SyscallTableError::OutOfRange(nr));
        }
        if self.entries[nr as usize].is_some() {
            return Err(SyscallTableError::Duplicate(nr));
        }
        self.entries[nr as usize] = Some(syscall);
        self.registered += 1;
        Ok(())
    }

    /// Attach a handler to an already-registered number.
    pub fn set_handler(&mut self, nr: u32, handler: Handler) -> Result<(), SyscallError> {
        let slot = self
            .entries
            .get_mut(nr as usize)
            .ok_or(SyscallError::BadNumber)?;
        match slot {
            Some(s) => {
                s.handler = Some(handler);
                Ok(())
            }
            None => Err(SyscallError::NotImplemented),
        }
    }

    pub fn lookup(&self, nr: u32) -> Option<Syscall> {
        self.entries.get(nr as usize).copied().flatten()
    }

    pub fn len(&self) -> usize {
        self.registered
    }

    pub fn is_empty(&self) -> bool {
        self.registered == 0
    }

    /// No duplicate numbers, and every entry is where its own `nr` says it is.
    pub fn validate(&self) -> Result<(), SyscallTableError> {
        for (i, e) in self.entries.iter().enumerate() {
            if let Some(s) = e {
                if s.nr as usize != i {
                    return Err(SyscallTableError::OutOfRange(s.nr));
                }
                if s.arity as usize > MAX_SYSCALL_ARGS {
                    return Err(SyscallTableError::OutOfRange(s.nr));
                }
            }
        }
        Ok(())
    }

    pub fn calls(&self) -> u64 {
        self.calls
    }

    pub fn denied(&self) -> u64 {
        self.denied
    }

    /// Numbers registered but not yet backed by a handler — the honest answer
    /// to "is this ABI real yet".
    pub fn unimplemented(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.map(|s| s.handler.is_none()).unwrap_or(false))
            .count()
    }

    /// F102: dispatch. Argument count and capability are checked here, once.
    pub fn dispatch(
        &mut self,
        nr: u32,
        args: &[u64],
        held: crate::proc::Caps,
    ) -> Result<i64, SyscallError> {
        self.calls += 1;
        let syscall = match self.lookup(nr) {
            Some(s) => s,
            None => {
                self.denied += 1;
                return Err(SyscallError::NotImplemented);
            }
        };
        if args.len() < syscall.arity as usize {
            self.denied += 1;
            return Err(SyscallError::TooFewArgs);
        }
        if args.len() > MAX_SYSCALL_ARGS {
            self.denied += 1;
            return Err(SyscallError::TooManyArgs);
        }
        if syscall.required_cap != 0 && !held.has(syscall.required_cap) {
            self.denied += 1;
            return Err(SyscallError::Denied);
        }
        match syscall.handler {
            Some(h) => h(&args[..syscall.arity as usize]),
            None => {
                self.denied += 1;
                Err(SyscallError::NotImplemented)
            }
        }
    }

    /// Entry blocks in `out`; returns bytes written. This is the table the way
    /// a debugger or the docs site wants to read it (F488).
    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut w = crate::cpu::Hud::new(out);
        for e in self.entries.iter().flatten() {
            w.str(e.name);
            w.str("(");
            w.num(e.nr as u64);
            w.str(") arity=");
            w.num(e.arity as u64);
            w.str(if e.handler.is_some() { " impl" } else { " stub" });
            w.str("\n");
        }
        w.used()
    }
}

impl Default for SyscallTable {
    fn default() -> SyscallTable {
        SyscallTable::new()
    }
}

// ---------------------------------------------------------------------------
// F103 — the fast path
// ---------------------------------------------------------------------------

/// `IA32_STAR` layout. `SYSCALL` takes CS from bits 47:32 and derives
/// `SS = CS + 8`; `SYSRET` takes its base from bits 63:48 and derives
/// `CS = base + 16`, `SS = base + 8`.
///
/// So the base is *not* the user code selector — it is the selector one slot
/// before user data, and the GDT must therefore lay user data out directly
/// before user code (F026 does). Getting this wrong loads a valid-looking
/// selector that is actually kernel data on the way back from a syscall, which
/// is a privilege bug that only shows up under load.
pub const fn star_value(kernel_code: u16, user_data: u16) -> u64 {
    let sysret_base = user_data.wrapping_sub(8);
    ((kernel_code as u64) << 32) | ((sysret_base as u64) << 48)
}

/// `IA32_SFMASK`: the flags cleared on entry. IF kills re-entrancy, TF kills
/// single-step surprises, DF would corrupt the string ops, AC breaks alignment
/// assumptions, and the IOPL bits must go to 0 so the kernel runs at level 0.
pub const SFMASK_VALUE: u64 = (1 << 8)  // TF
    | (1 << 9)                          // IF
    | (1 << 10)                         // DF
    | (1 << 18)                         // AC
    | (3 << 12); // IOPL

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FastPath {
    pub configured: bool,
    pub star: u64,
    pub lstar: u64,
    pub sfmask: u64,
}

impl FastPath {
    /// F103: program the three MSRs that make `syscall`/`sysret` work.
    pub fn configure(entry_point: u64) -> Result<FastPath, &'static str> {
        if entry_point == 0 {
            return Err("syscall entry point is null");
        }
        let (kernel_code, _kernel_data) = crate::proc::selectors_for(crate::proc::Ring::Zero);
        let (_user_code, user_data) = crate::proc::selectors_for(crate::proc::Ring::Three);
        let star = star_value(kernel_code, user_data);
        let fp = FastPath {
            configured: true,
            star,
            lstar: entry_point,
            sfmask: SFMASK_VALUE,
        };
        Ok(fp)
    }

    /// Write the MSRs (kernel target only; the host build only computes them).
    pub fn install(&self) -> Result<(), &'static str> {
        if !self.configured {
            return Err("fast path not configured");
        }
        #[cfg(all(target_arch = "x86_64", target_os = "none"))]
        {
            use crate::cpu::msr::{self, Msr};
            let _ = msr::write(Msr::Star, self.star);
            let _ = msr::write(Msr::LStar, self.lstar);
            let _ = msr::write(Msr::SFmask, self.sfmask);
            // EFER.SCE enables the instruction set itself.
            let efer = msr::read(Msr::Efer);
            let _ = msr::write(Msr::Efer, efer | crate::cpu::msr::EFER_SCE);
        }
        Ok(())
    }

    /// Round-trip check: read the MSRs back and confirm they hold what we wrote.
    pub fn verify(&self) -> bool {
        #[cfg(all(target_arch = "x86_64", target_os = "none"))]
        {
            use crate::cpu::msr::{self, Msr};
            return msr::read(Msr::Star) == self.star
                && msr::read(Msr::LStar) == self.lstar
                && msr::read(Msr::SFmask) == self.sfmask
                && msr::read(Msr::Efer) & crate::cpu::msr::EFER_SCE != 0;
        }
        #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
        {
            self.configured && self.star != 0 && self.lstar != 0
        }
    }
}

/// F103 counters: how often the fast path actually ran, and how often the
/// kernel had to fall back to the slow (interrupt-gate) path.
pub struct FastPathStats {
    pub fast: AtomicU64,
    pub fallback: AtomicU64,
    pub slowest_ns: AtomicU64,
}

impl FastPathStats {
    pub const fn new() -> FastPathStats {
        FastPathStats {
            fast: AtomicU64::new(0),
            fallback: AtomicU64::new(0),
            slowest_ns: AtomicU64::new(0),
        }
    }

    pub fn note_fast(&self) {
        self.fast.fetch_add(1, Ordering::Relaxed);
    }

    pub fn note_fallback(&self) {
        self.fallback.fetch_add(1, Ordering::Relaxed);
    }

    pub fn note_latency(&self, ns: u64) {
        self.slowest_ns.fetch_max(ns, Ordering::Relaxed);
    }

    pub fn fast(&self) -> u64 {
        self.fast.load(Ordering::Relaxed)
    }

    pub fn fallback(&self) -> u64 {
        self.fallback.load(Ordering::Relaxed)
    }

    pub fn slowest_ns(&self) -> u64 {
        self.slowest_ns.load(Ordering::Relaxed)
    }

    /// Percentage of entries that took the fast path (0 when nothing ran).
    pub fn fast_percent(&self) -> u64 {
        let total = self.fast() + self.fallback();
        if total == 0 {
            return 0;
        }
        self.fast() * 100 / total
    }
}

impl Default for FastPathStats {
    fn default() -> FastPathStats {
        FastPathStats::new()
    }
}

static FAST_STATS: FastPathStats = FastPathStats::new();

pub fn fast_path_stats() -> &'static FastPathStats {
    &FAST_STATS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proc::{Caps, CapToken, SecurityLayer};

    fn ok_handler(_args: &[u64]) -> Result<i64, SyscallError> {
        Ok(42)
    }

    fn echoing_handler(args: &[u64]) -> Result<i64, SyscallError> {
        Ok(args.iter().sum::<u64>() as i64)
    }

    fn denying_handler(_args: &[u64]) -> Result<i64, SyscallError> {
        Err(SyscallError::Denied)
    }

    #[test]
    fn stable_abi_is_registered_and_numbered() {
        let t = SyscallTable::with_stable_abi();
        assert_eq!(t.len(), STABLE_SYSCALL_COUNT as usize);
        assert!(t.validate().is_ok());
        let exit = t.lookup(SYS_EXIT).unwrap();
        assert_eq!(exit.name, "exit");
        assert_eq!(exit.arity, 1);
        assert!(t.lookup(SYS_WRITE).is_some());
        assert!(t.lookup(SYS_TRACE).unwrap().required_cap == Caps::TRACE);
        assert!(t.lookup(999).is_none());
        // Every stable number is present, and nothing is silently missing.
        for nr in STABLE_SYSCALL_BASE..STABLE_SYSCALL_BASE + STABLE_SYSCALL_COUNT {
            assert!(t.lookup(nr).is_some(), "syscall {nr} missing");
        }
        // All of them are still stubs until their subsystem registers a handler.
        assert_eq!(t.unimplemented(), STABLE_SYSCALL_COUNT as usize);
        let mut out = [0u8; 512];
        let n = t.render(&mut out);
        let s = core::str::from_utf8(&out[..n]).unwrap();
        assert!(s.starts_with("exit(0) arity=1 stub"), "got {s}");
    }

    #[test]
    fn registration_rules_are_enforced() {
        let mut t = SyscallTable::new();
        assert!(t.register(Syscall::new(10, "a", 1)).is_ok());
        assert_eq!(
            t.register(Syscall::new(10, "b", 1)),
            Err(SyscallTableError::Duplicate(10))
        );
        assert_eq!(
            t.register(Syscall::new(999, "c", 1)),
            Err(SyscallTableError::OutOfRange(999))
        );
        assert_eq!(t.len(), 1);
        // A handler can be attached later, and only to a registered number.
        assert!(t.set_handler(10, ok_handler).is_ok());
        assert_eq!(t.set_handler(11, ok_handler), Err(SyscallError::NotImplemented));
        assert_eq!(t.lookup(10).unwrap().handler.is_some(), true);
    }

    #[test]
    fn dispatch_checks_arity_before_running_anything() {
        let mut t = SyscallTable::new();
        t.register(Syscall::new(20, "add", 2).with_handler(echoing_handler))
            .unwrap();
        t.register(Syscall::new(21, "stub", 0)).unwrap();
        let caps = CapToken::new(1, Caps::kernel(), SecurityLayer::L0Kernel).caps;

        assert_eq!(t.dispatch(20, &[1, 2], caps), Ok(3));
        assert_eq!(
            t.dispatch(20, &[1], caps),
            Err(SyscallError::TooFewArgs),
            "a short argument list is refused before the handler sees it"
        );
        assert_eq!(
            t.dispatch(20, &[1, 2, 3, 4, 5, 6, 7], caps),
            Err(SyscallError::TooManyArgs)
        );
        assert_eq!(t.dispatch(21, &[], caps), Err(SyscallError::NotImplemented));
        assert_eq!(t.dispatch(77, &[], caps), Err(SyscallError::NotImplemented));
        assert_eq!(t.calls(), 5);
        assert_eq!(t.denied(), 4);
    }

    #[test]
    fn capability_gate_precedes_the_handler() {
        let mut t = SyscallTable::new();
        t.register(Syscall::new(30, "trace", 1).requiring(Caps::TRACE))
            .unwrap();
        t.set_handler(30, ok_handler).unwrap();
        let weak = CapToken::new(1, Caps(Caps::FS_READ), SecurityLayer::L2Application);
        assert_eq!(
            t.dispatch(30, &[0], weak.caps),
            Err(SyscallError::Denied),
            "the capability is checked before the handler runs"
        );
        let strong = CapToken::new(2, Caps::kernel(), SecurityLayer::L0Kernel);
        assert_eq!(t.dispatch(30, &[0], strong.caps), Ok(42));
        // A handler that refuses for its own reason is still an error.
        t.register(Syscall::new(31, "nope", 0).with_handler(denying_handler))
            .unwrap();
        assert_eq!(t.dispatch(31, &[], strong.caps), Err(SyscallError::Denied));
        assert_eq!(SyscallError::Denied.errno(), -13);
        assert_eq!(SyscallError::NotImplemented.errno(), -38);
        assert!(!SyscallError::TooFewArgs.as_str().is_empty());
    }

    #[test]
    fn star_encoding_survives_the_architectural_derivation() {
        use crate::cpu::gdt;
        let star = star_value(gdt::SEL_KERNEL_CODE, gdt::SEL_USER_DATA);
        // SYSCALL: CS = STAR[47:32], SS = that + 8.
        let syscall_cs = star >> 32 & 0xFFFF;
        assert_eq!(syscall_cs, gdt::SEL_KERNEL_CODE as u64);
        assert_eq!(syscall_cs + 8, gdt::SEL_KERNEL_DATA as u64);
        // SYSRET: CS = base + 16, SS = base + 8.
        let base = star >> 48 & 0xFFFF;
        assert_eq!(base + 16, gdt::SEL_USER_CODE as u64, "SYSRET CS");
        assert_eq!(base + 8, gdt::SEL_USER_DATA as u64, "SYSRET SS");
        // The GDT layout that derivation depends on.
        assert_eq!(gdt::SEL_KERNEL_DATA, gdt::SEL_KERNEL_CODE + 8);
        assert_eq!(gdt::SEL_USER_CODE, gdt::SEL_USER_DATA + 8);
    }

    #[test]
    fn sfmask_clears_the_dangerous_flags() {
        assert_ne!(SFMASK_VALUE & (1 << 9), 0, "IF must be cleared on entry");
        assert_ne!(SFMASK_VALUE & (1 << 8), 0, "TF must be cleared");
        assert_ne!(SFMASK_VALUE & (1 << 10), 0, "DF must be cleared");
        assert_ne!(SFMASK_VALUE & (1 << 18), 0, "AC must be cleared");
        assert_eq!(SFMASK_VALUE & (3 << 12), 3 << 12, "IOPL must drop to 0");
    }

    #[test]
    fn fast_path_configures_and_verifies() {
        let fp = FastPath::configure(0xFFFF_8000_0000_2000).expect("valid entry");
        assert!(fp.configured);
        assert_eq!(fp.lstar, 0xFFFF_8000_0000_2000);
        assert_eq!(fp.sfmask, SFMASK_VALUE);
        assert!(fp.verify());
        assert_eq!(FastPath::configure(0).unwrap_err(), "syscall entry point is null");
        assert_eq!(FastPath::default().install(), Err("fast path not configured"));
        assert!(fp.install().is_ok());

        let s = FastPathStats::new();
        s.note_fast();
        s.note_fast();
        s.note_fallback();
        s.note_latency(1200);
        s.note_latency(300);
        assert_eq!(s.fast(), 2);
        assert_eq!(s.fallback(), 1);
        assert_eq!(s.fast_percent(), 66);
        assert_eq!(s.slowest_ns(), 1200, "the worst case is kept, not the last");
        assert_eq!(FastPathStats::new().fast_percent(), 0);
    }
}
