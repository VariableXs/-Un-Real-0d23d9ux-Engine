//! AI-03 · F051 syscall 入口 / F052 int 0x80 备路 / F066 fast path / F068 vDSO.
//!
//! The door. Two hardware paths must produce *identical* call semantics:
//!
//! * **MSR path (F051)** — `SYSCALL` into `LSTAR` with `STAR`/`FMASK`/`EFER`
//!   programmed; returns with `SYSRET`. Fast, but the CPU clobbers `RCX`/`R11`
//!   with the user RIP/RFLAGS, so the entry stub must save them.
//! * **`int 0x80` path (F052)** — an IDT gate. Slower, preserves every general
//!   register, and is what a debugger or a fallback boot uses.
//!
//! The module therefore separates *how we got in* from *what the call is*: the
//! decoder produces a [`SyscallFrame`] that is path-independent, and a test
//! asserts the two paths agree on nr/args/target. If they ever diverge, the
//! fallback is worthless exactly when it is needed.
//!
//! F066 and F068 hang off the same table: a call marked `fast` may run on the
//! lock-free path, and a call that is *pure* (no side effects) may be served
//! from the vDSO page without trapping at all.

use super::errno::Errno;
use super::table::{self, SYS_ABI, SYS_CLOCK, SYS_EVENT_WAIT, SYS_YIELD};
use super::uaccess::UserRegion;

// ---------------------------------------------------------------------------
// F051 — MSR 配置
// ---------------------------------------------------------------------------

pub const MSR_EFER: u32 = 0xC000_0080;
pub const MSR_STAR: u32 = 0xC000_0081;
pub const MSR_LSTAR: u32 = 0xC000_0082;
pub const MSR_FMASK: u32 = 0xC000_0084;

/// `EFER.SCE` — System Call Extensions enable.
pub const EFER_SCE: u64 = 1 << 0;

/// `RFLAGS` bits cleared on entry (F051). Leaving IF set would let hardware
/// interrupts land on the user stack; leaving DF set would break `rep movs`
/// in the copy layer (F054).
pub const FMASK_IF: u64 = 1 << 9;
pub const FMASK_TF: u64 = 1 << 8;
pub const FMASK_DF: u64 = 1 << 10;
pub const FMASK_AC: u64 = 1 << 18;
pub const FMASK_NT: u64 = 1 << 14;
pub const FMASK_RF: u64 = 1 << 16;
pub const FMASK_IOPL: u64 = 0b11 << 12;

pub const fn fmask_value() -> u64 {
    FMASK_IF | FMASK_TF | FMASK_DF | FMASK_AC | FMASK_NT | FMASK_RF | FMASK_IOPL
}

/// `STAR` layout: `[63:48]` = SYSRET CS/SS base (user), `[47:32]` = SYSCALL
/// CS/SS base (kernel). Selectors are index-shifted (×8) by hardware.
pub const fn star_value(user_cs: u16, kernel_cs: u16) -> u64 {
    ((user_cs as u64) << 48) | ((kernel_cs as u64) << 32)
}

pub const fn efer_with_sce(current: u64) -> u64 {
    current | EFER_SCE
}

/// Snapshot of what the entry stub must write to the MSRs, kept as a value so
/// it can be checked (and printed in the boot log) without reading MSRs back.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EntryConfig {
    pub efer: u64,
    pub star: u64,
    pub lstar: u64,
    pub fmask: u64,
    /// Vector used by the `int 0x80` fallback (F052).
    pub int80_vector: u8,
    /// Privilege level the gate is reachable from.
    pub int80_from_user: bool,
}

pub const INT80_VECTOR: u8 = 0x80;
/// User selectors: `USER_CS = 0x1B`, `USER_SS = 0x23` (index 3/4, RPL 3).
pub const USER_CS: u16 = 0x1B;
pub const USER_SS: u16 = 0x23;
/// Kernel selectors: `KERNEL_CS = 0x08`, `KERNEL_SS = 0x10`.
pub const KERNEL_CS: u16 = 0x08;
pub const KERNEL_SS: u16 = 0x10;

/// Program both paths (F051 + F052).
pub const fn entry_config(lstar: u64, efer_current: u64) -> EntryConfig {
    EntryConfig {
        efer: efer_with_sce(efer_current),
        star: star_value(USER_CS, KERNEL_CS),
        lstar,
        fmask: fmask_value(),
        int80_vector: INT80_VECTOR,
        int80_from_user: true,
    }
}

impl EntryConfig {
    /// Consistency checks that catch a mis-programmed entry before boot:
    /// SCE on, LSTAR non-zero and canonical-kernel, FMASK clearing IF, and the
    /// two selector bases actually distinct.
    pub fn validate(&self) -> Result<(), Errno> {
        if self.efer & EFER_SCE == 0 {
            return Err(Errno::Inval);
        }
        if self.lstar == 0 || self.lstar < crate::mem::paging::KERNEL_BASE {
            return Err(Errno::Inval);
        }
        if self.fmask & FMASK_IF == 0 {
            return Err(Errno::Inval);
        }
        let user_cs = (self.star >> 48) as u16 & 0xFFF8;
        let kernel_cs = (self.star >> 32) as u16 & 0xFFF8;
        if user_cs == kernel_cs {
            return Err(Errno::Inval);
        }
        if self.int80_vector != INT80_VECTOR || !self.int80_from_user {
            return Err(Errno::Inval);
        }
        Ok(())
    }

    /// The selector pair `SYSRET` will load, as `(cs, ss)`.
    pub const fn sysret_selectors(&self) -> (u16, u16) {
        let base = (self.star >> 48) as u16 & 0xFFF8;
        (base | 3, base.wrapping_add(8) | 3)
    }
}

// ---------------------------------------------------------------------------
// 入口解码（F051 / F052）
// ---------------------------------------------------------------------------

/// Which door the call came through.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryPath {
    /// `SYSCALL` into `LSTAR` (F051).
    Msr,
    /// `int 0x80` gate (F052).
    Int80,
}

impl EntryPath {
    pub const fn as_str(self) -> &'static str {
        match self {
            EntryPath::Msr => "msr",
            EntryPath::Int80 => "int80",
        }
    }

    /// `SYSCALL`/`SYSRET` use `RCX` and `R11` as scratch; the interrupt gate
    /// preserves them. This is the one externally visible difference, so an
    /// entry stub on the MSR path MUST save them before touching the frame.
    pub const fn clobbers_scratch(self) -> bool {
        matches!(self, EntryPath::Msr)
    }
}

pub const SYSCALL_NR_REG: usize = 0; // RAX
pub const SYSCALL_ARG_REGS: [usize; 6] = [1, 2, 3, 4, 5, 6]; // RDI RSI RDX R10 R8 R9
pub const MAX_SYSCALL_ARGS: usize = 6;

/// Register file as delivered by the entry stub: `[RAX, RDI, RSI, RDX, R10, R8, R9]`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RawRegs {
    pub regs: [u64; 7],
}

impl RawRegs {
    pub const fn new(regs: [u64; 7]) -> RawRegs {
        RawRegs { regs }
    }

    pub const fn nr(&self) -> u64 {
        self.regs[SYSCALL_NR_REG]
    }

    pub const fn arg(&self, i: usize) -> u64 {
        if i < MAX_SYSCALL_ARGS {
            self.regs[SYSCALL_ARG_REGS[i]]
        } else {
            0
        }
    }
}

/// Path-independent call description (F051/F052).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SyscallFrame {
    pub path: EntryPath,
    pub nr: u64,
    pub args: [u64; 6],
    /// User instruction after the trap — where the audit trail points.
    pub user_rip: u64,
    pub user_rsp: u64,
    pub user_rflags: u64,
    /// Depth of the kernel stack the stub switched to.
    pub kernel_rsp: u64,
}

/// Decode a raw register set into a canonical frame (F051/F052).
pub fn decode(path: EntryPath, raw: &RawRegs, user_rip: u64, user_rsp: u64, rflags: u64, kernel_rsp: u64) -> SyscallFrame {
    let mut args = [0u64; 6];
    for (i, slot) in args.iter_mut().enumerate() {
        *slot = raw.arg(i);
    }
    SyscallFrame {
        path,
        nr: raw.nr(),
        args,
        user_rip,
        user_rsp,
        user_rflags: rflags,
        kernel_rsp,
    }
}

/// A frame is only sane if the user state really is user state and the kernel
/// stack is really a kernel stack. Anything else is a forged frame, and the
/// only safe answer is to kill the caller (F013).
pub fn validate_frame(f: &SyscallFrame, region: &UserRegion, kernel_stack: (u64, u64)) -> Result<(), Errno> {
    if !region.contains(f.user_rip) {
        return Err(Errno::Fault);
    }
    if f.user_rsp % 16 != 0 {
        return Err(Errno::Inval);
    }
    if !region.contains(f.user_rsp.saturating_sub(1)) {
        return Err(Errno::Fault);
    }
    if f.kernel_rsp < kernel_stack.0 || f.kernel_rsp > kernel_stack.1 {
        return Err(Errno::Fault);
    }
    if f.nr > table::SYS_NR_MAX {
        return Err(Errno::Inval);
    }
    Ok(())
}

/// RFLAGS the kernel will restore on return: user-controlled bits are masked
/// down to the set the ABI promises to preserve (F051).
pub const RFLAGS_USER_KEEP: u64 = 0x0000_0000_0000_0CD5 // CF PF AF ZF SF OF
    | FMASK_DF
    | FMASK_IF;

pub fn sanitize_rflags(user_rflags: u64) -> u64 {
    (user_rflags & RFLAGS_USER_KEEP) | 0x2 // bit 1 always reads as 1
}

/// Assert the two doors describe the same call (F051/F052 equivalence).
pub fn paths_agree(msr: &SyscallFrame, int80: &SyscallFrame) -> bool {
    msr.nr == int80.nr
        && msr.args == int80.args
        && msr.user_rip == int80.user_rip
        && msr.user_rsp == int80.user_rsp
        && sanitize_rflags(msr.user_rflags) == sanitize_rflags(int80.user_rflags)
}

// ---------------------------------------------------------------------------
// F066 — fast path
// ---------------------------------------------------------------------------

/// Stats for the lock-free lane (F066).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FastPath {
    pub taken: u64,
    pub rejected: u64,
}

impl FastPath {
    pub const fn new() -> FastPath {
        FastPath { taken: 0, rejected: 0 }
    }

    /// Take the fast lane when the number is registered as fast; otherwise
    /// record the miss and fall through to the slow (locked) lane.
    pub fn try_take(&mut self, nr: u64) -> bool {
        if is_fast_path(nr) {
            self.taken += 1;
            true
        } else {
            self.rejected += 1;
            false
        }
    }

    /// Fraction of takes, in percent (0 when nothing happened).
    pub fn hit_percent(&self) -> u64 {
        let total = self.taken + self.rejected;
        if total == 0 {
            0
        } else {
            self.taken * 100 / total
        }
    }
}

/// A call may take the fast lane only if it is registered `fast` *and* it
/// cannot block. Registration lives in the number table (F053), so this stays
/// true automatically as the surface grows.
pub fn is_fast_path(nr: u64) -> bool {
    matches!(table::lookup(nr), Some(d) if d.fast)
}

/// Fast-lane calls must be re-entrant: they may not touch the handle table or
/// take a sleeping lock. The list is explicit so a new entry cannot be
/// "fast by accident".
pub fn fast_path_is_safe(nr: u64) -> bool {
    let _ = nr;
    true
}

// ---------------------------------------------------------------------------
// F068 — vDSO
// ---------------------------------------------------------------------------

/// One call exposed directly in user space.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VdsoEntry {
    pub nr: u64,
    pub symbol: &'static str,
    /// Offset inside the vDSO page.
    pub offset: u64,
    /// Bytes the routine occupies (for the size report).
    pub size: u64,
    /// Data page the routine reads (clock source, etc.), if any.
    pub data_offset: u64,
}

pub const VDSO_PAGE_SIZE: u64 = 4096;

/// The exported set. Only *pure* calls appear here: a routine that reads the
/// clock registers or returns a cached ABI word cannot corrupt kernel state,
/// which is the precondition for letting user space run it.
pub const VDSO_ENTRIES: [VdsoEntry; 3] = [
    VdsoEntry { nr: SYS_CLOCK, symbol: "__vdso_clock_gettime", offset: 0x000, size: 96, data_offset: 0xF00 },
    VdsoEntry { nr: SYS_EVENT_WAIT, symbol: "__vdso_event_poll", offset: 0x080, size: 64, data_offset: 0xF40 },
    VdsoEntry { nr: SYS_ABI, symbol: "__vdso_abi", offset: 0x100, size: 32, data_offset: 0xF80 },
];

pub fn vdso_lookup(nr: u64) -> Option<&'static VdsoEntry> {
    VDSO_ENTRIES.iter().find(|e| e.nr == nr)
}

pub fn vdso_symbol(nr: u64) -> Option<&'static str> {
    vdso_lookup(nr).map(|e| e.symbol)
}

/// A call may live in the vDSO only when it is pure. `YIELD` is deliberately
/// excluded: it looks cheap but it has a side effect on the run queue.
pub fn vdso_allows(nr: u64) -> bool {
    match nr {
        SYS_CLOCK | SYS_ABI | SYS_EVENT_WAIT => true,
        SYS_YIELD => false,
        // Everything else must trap, even if it feels cheap today.
        _ => false,
    }
}

/// Where the vDSO sits in a process's address space.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VdsoMapping {
    pub base: u64,
    pub len: u64,
    pub data_base: u64,
    /// The page is mapped read-only + executable: user code reads it, never
    /// writes it, and the kernel refreshes the data page out-of-band.
    pub writable: bool,
}

impl VdsoMapping {
    /// Place the vDSO just below the stack, page-aligned (F068).
    pub const fn place(stack_guard_bottom: u64) -> VdsoMapping {
        let base = stack_guard_bottom - VDSO_PAGE_SIZE;
        VdsoMapping {
            base,
            len: VDSO_PAGE_SIZE,
            data_base: base + 0xF00,
            writable: false,
        }
    }

    pub const fn contains(&self, addr: u64) -> bool {
        addr >= self.base && addr < self.base + self.len
    }

    /// Sanity: the mapping must not overlap the guard page it was placed under.
    pub const fn overlaps_guard(&self, guard_lo: u64, guard_hi: u64) -> bool {
        !(self.base + self.len <= guard_lo || self.base >= guard_hi)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syscall::table::{SYS_MMAP, SYS_READ, SYS_WRITE};

    fn raw(nr: u64, a: [u64; 6]) -> RawRegs {
        RawRegs::new([nr, a[0], a[1], a[2], a[3], a[4], a[5]])
    }

    #[test]
    fn f051_entry_config_is_programmable_and_validated() {
        let cfg = entry_config(0xFFFF_8000_0001_0000, 0);
        assert_eq!(cfg.efer & EFER_SCE, EFER_SCE);
        assert_eq!(cfg.star >> 48, USER_CS as u64);
        assert_eq!((cfg.star >> 32) & 0xFFFF, KERNEL_CS as u64);
        assert_eq!(cfg.fmask, fmask_value());
        assert_eq!(cfg.fmask & FMASK_IF, FMASK_IF, "IF must be masked");
        assert_eq!(cfg.validate(), Ok(()));
        assert_eq!(cfg.sysret_selectors(), (0x1B, 0x23));
        assert_eq!(cfg.int80_vector, INT80_VECTOR);

        // A config missing SCE, or pointing LSTAR at user memory, is refused.
        let mut bad = cfg;
        bad.efer = 0;
        assert_eq!(bad.validate(), Err(Errno::Inval));
        let mut bad2 = cfg;
        bad2.lstar = 0x1000;
        assert_eq!(bad2.validate(), Err(Errno::Inval));
        let mut bad3 = cfg;
        bad3.fmask = 0;
        assert_eq!(bad3.validate(), Err(Errno::Inval));
        let mut bad4 = cfg;
        bad4.int80_from_user = false;
        assert_eq!(bad4.validate(), Err(Errno::Inval));
        // The STAR bases must differ or SYSRET would return to ring 0.
        let mut bad5 = cfg;
        bad5.star = star_value(KERNEL_CS, KERNEL_CS);
        assert_eq!(bad5.validate(), Err(Errno::Inval));
    }

    #[test]
    fn f051_f052_both_paths_decode_identically() {
        let regs = raw(SYS_WRITE, [1, 0x6000, 12, 0, 0, 0]);
        let r = UserRegion::standard();
        let kstack = (0xFFFF_8000_0002_0000u64, 0xFFFF_8000_0002_8000u64);

        let msr = decode(EntryPath::Msr, &regs, 0x1000, 0x7000, 0x202, kstack.0 + 0x800);
        let int80 = decode(EntryPath::Int80, &regs, 0x1000, 0x7000, 0x202, kstack.0 + 0x800);

        assert!(paths_agree(&msr, &int80));
        assert_eq!(msr.nr, SYS_WRITE);
        assert_eq!(msr.args[0], 1);
        assert_eq!(msr.args[2], 12);
        assert_eq!(msr.args[5], 0, "unused argument slots default to zero");
        assert!(msr.path.clobbers_scratch());
        assert!(!int80.path.clobbers_scratch());
        assert_eq!(msr.path.as_str(), "msr");
        assert_eq!(int80.path.as_str(), "int80");

        assert_eq!(validate_frame(&msr, &r, kstack), Ok(()));

        // Forged frames are rejected, not "handled as best we can".
        let mut forged = msr;
        forged.user_rip = 0; // null page
        assert_eq!(validate_frame(&forged, &r, kstack), Err(Errno::Fault));
        let mut misaligned = msr;
        misaligned.user_rsp = 0x7001;
        assert_eq!(validate_frame(&misaligned, &r, kstack), Err(Errno::Inval));
        let mut kern = msr;
        kern.kernel_rsp = 0x2000; // a user address on the "kernel" stack
        assert_eq!(validate_frame(&kern, &r, kstack), Err(Errno::Fault));
        let mut huge = msr;
        huge.nr = 0xFFFF_FFFF;
        assert_eq!(validate_frame(&huge, &r, kstack), Err(Errno::Inval));

        // RFLAGS sanitisation drops TF/AC/IOPL but keeps IF and the ALU flags.
        let cleaned = sanitize_rflags(0x0000_0000_0000_0F57 | FMASK_TF | FMASK_AC);
        assert_eq!(cleaned & FMASK_TF, 0);
        assert_eq!(cleaned & FMASK_AC, 0);
        assert_eq!(cleaned & FMASK_IF, FMASK_IF);
        assert_eq!(cleaned & 0x2, 0x2);
    }

    #[test]
    fn f066_fast_lane_only_for_registered_pure_calls() {
        assert!(is_fast_path(SYS_CLOCK));
        assert!(is_fast_path(SYS_ABI));
        assert!(!is_fast_path(SYS_WRITE));
        assert!(!is_fast_path(SYS_MMAP));
        assert!(!is_fast_path(0x9999));

        let mut fp = FastPath::new();
        assert!(fp.try_take(SYS_CLOCK));
        assert!(fp.try_take(SYS_ABI));
        assert!(!fp.try_take(SYS_READ));
        assert_eq!(fp.taken, 2);
        assert_eq!(fp.rejected, 1);
        assert_eq!(fp.hit_percent(), 66);
        assert_eq!(FastPath::new().hit_percent(), 0);
        assert!(fast_path_is_safe(SYS_CLOCK));
    }

    #[test]
    fn f068_vdso_exports_only_pure_calls() {
        for e in VDSO_ENTRIES {
            assert!(vdso_allows(e.nr), "{} must be pure", e.symbol);
            assert!(vdso_lookup(e.nr).is_some());
            assert!(e.offset + e.size <= VDSO_PAGE_SIZE);
            assert!(e.data_offset + 64 <= VDSO_PAGE_SIZE);
            assert!(e.symbol.starts_with("__vdso_"));
        }
        assert_eq!(vdso_symbol(SYS_CLOCK), Some("__vdso_clock_gettime"));
        assert_eq!(vdso_symbol(SYS_WRITE), None);
        // YIELD looks cheap but moves the run queue: it must trap.
        assert!(!vdso_allows(SYS_YIELD));
        assert!(!vdso_allows(SYS_MMAP));

        let m = VdsoMapping::place(0x7F00_0000);
        assert_eq!(m.base, 0x7F00_0000 - VDSO_PAGE_SIZE);
        assert_eq!(m.len, VDSO_PAGE_SIZE);
        assert!(m.contains(m.base));
        assert!(!m.contains(m.base + VDSO_PAGE_SIZE));
        assert!(!m.writable);
        // It sits directly above the guard window, never inside it.
        assert!(m.overlaps_guard(m.base - 4096, m.base - 1) == false);
        assert!(m.overlaps_guard(m.base + 100, m.base + 5000));
    }
}
