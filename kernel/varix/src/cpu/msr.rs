//! F040 MSR 封装 — a single, auditable door to `rdmsr`/`wrmsr`.
//!
//! Bare MSR access scattered across drivers is how kernels end up poking at
//! undocumented model-specific state. Varix keeps an allow-list: every MSR the
//! kernel has a reason to touch is named here, and anything else is refused
//! (and counted) instead of executed.

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// Named MSRs Varix is allowed to touch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Msr {
    /// Extended Feature Enable Register (LME/NXE/SCE...).
    Efer,
    /// `syscall` target CS/SS selector halves.
    Star,
    /// `syscall` entry RIP (F103).
    LStar,
    /// `syscall` entry for compatibility mode.
    CStar,
    /// RFLAGS mask applied on `syscall`.
    SFmask,
    /// FS segment base (user pointer for TLS).
    FsBase,
    /// GS segment base — the per-CPU area (F047).
    GsBase,
    /// Swap target for `swapgs`.
    KernelGsBase,
    /// TSC_AUX: cheap CPU id for `rdtscp`.
    TscAux,
    /// Local APIC base / enable / x2APIC (F032, F035).
    ApicBase,
    /// x2APIC register window base (0x800 + reg << 4).
    X2ApicBase,
    /// Page Attribute Table.
    Pat,
    /// Speculation control (Spectre v2 mitigations).
    SpecCtrl,
    /// Architectural capabilities (hardware mitigation enumeration).
    ArchCaps,
    /// L1D$ flush command.
    FlushCmd,
    /// Miscellaneous feature enable.
    MiscEnable,
    /// XSAVE-managed state component mask (F048).
    Xcr0,
    /// Performance / frequency feedback (F084).
    Mperf,
    Aperf,
    PerfStatus,
    /// Time Stamp Counter.
    Tsc,
}

impl Msr {
    pub const fn addr(self) -> u32 {
        match self {
            Msr::Efer => 0xC000_0080,
            Msr::Star => 0xC000_0081,
            Msr::LStar => 0xC000_0082,
            Msr::CStar => 0xC000_0083,
            Msr::SFmask => 0xC000_0084,
            Msr::FsBase => 0xC000_0100,
            Msr::GsBase => 0xC000_0101,
            Msr::KernelGsBase => 0xC000_0102,
            Msr::TscAux => 0xC000_0103,
            Msr::ApicBase => 0x0000_001B,
            Msr::X2ApicBase => 0x0000_0800,
            Msr::Pat => 0x0000_0277,
            Msr::SpecCtrl => 0x0000_0048,
            Msr::ArchCaps => 0x0000_010A,
            Msr::FlushCmd => 0x0000_010B,
            Msr::MiscEnable => 0x0000_01A0,
            Msr::Xcr0 => 0x0000_0E01,
            Msr::Mperf => 0x0000_00E7,
            Msr::Aperf => 0x0000_00E8,
            Msr::PerfStatus => 0x0000_0198,
            Msr::Tsc => 0x0000_0010,
        }
    }

    /// Writable by the kernel? Read-only MSRs stay readable but refuse writes.
    pub const fn writable(self) -> bool {
        !matches!(self, Msr::ArchCaps | Msr::Tsc)
    }

    /// Every named MSR is allow-listed by construction; a raw address has to
    /// be resolved to one of these before it can be used.
    pub fn from_addr(addr: u32) -> Option<Msr> {
        const ALL: [Msr; 20] = [
            Msr::Efer,
            Msr::Star,
            Msr::LStar,
            Msr::CStar,
            Msr::SFmask,
            Msr::FsBase,
            Msr::GsBase,
            Msr::KernelGsBase,
            Msr::TscAux,
            Msr::ApicBase,
            Msr::X2ApicBase,
            Msr::Pat,
            Msr::SpecCtrl,
            Msr::ArchCaps,
            Msr::FlushCmd,
            Msr::MiscEnable,
            Msr::Xcr0,
            Msr::Mperf,
            Msr::Aperf,
            Msr::PerfStatus,
        ];
        ALL.iter().copied().find(|m| m.addr() == addr)
    }
}

// EFER bits.
pub const EFER_SCE: u64 = 1 << 0; // syscall/sysret enable
pub const EFER_LME: u64 = 1 << 8; // long mode enable
pub const EFER_LMA: u64 = 1 << 10; // long mode active
pub const EFER_NXE: u64 = 1 << 11; // no-execute enable (F227)
pub const EFER_SVME: u64 = 1 << 12; // SVM enable
pub const EFER_FFXSR: u64 = 1 << 14;

/// Access counters — the audit surface of F040.
pub struct MsrStats {
    reads: AtomicU64,
    writes: AtomicU64,
    denied: AtomicU64,
    last_denied: AtomicU32,
}

impl MsrStats {
    pub const fn new() -> MsrStats {
        MsrStats {
            reads: AtomicU64::new(0),
            writes: AtomicU64::new(0),
            denied: AtomicU64::new(0),
            last_denied: AtomicU32::new(0),
        }
    }
    pub fn reads(&self) -> u64 {
        self.reads.load(Ordering::Relaxed)
    }
    pub fn writes(&self) -> u64 {
        self.writes.load(Ordering::Relaxed)
    }
    pub fn denied(&self) -> u64 {
        self.denied.load(Ordering::Relaxed)
    }
    pub fn last_denied(&self) -> u32 {
        self.last_denied.load(Ordering::Relaxed)
    }
}

static STATS: MsrStats = MsrStats::new();

pub fn stats() -> &'static MsrStats {
    &STATS
}

/// Why an MSR access was refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Denied {
    /// Address is not on the allow-list.
    NotAllowed,
    /// Named MSR exists but is read-only.
    ReadOnly,
}

/// Read a named MSR. Never fails for an allow-listed MSR.
pub fn read(msr: Msr) -> u64 {
    STATS.reads.fetch_add(1, Ordering::Relaxed);
    // SAFETY: `Msr` is only constructible for allow-listed addresses, and
    // every one of them exists on any x86_64 Varix boots on (guard for the
    // few optional ones happens in the callers).
    unsafe { rdmsr(msr.addr()) }
}

/// Write a named MSR; refuses read-only MSRs instead of trapping.
pub fn write(msr: Msr, value: u64) -> Result<(), Denied> {
    if !msr.writable() {
        deny(msr.addr());
        return Err(Denied::ReadOnly);
    }
    STATS.writes.fetch_add(1, Ordering::Relaxed);
    // SAFETY: see `read`.
    unsafe { wrmsr(msr.addr(), value) };
    Ok(())
}

/// The x2APIC register window (`0x800 | reg >> 4`, F035) is allow-listed as a
/// block rather than register by register: every offset inside it is a normal
/// LAPIC register, and enumerating 64 of them would only add noise.
pub fn read_x2apic(reg: u32) -> u64 {
    STATS.reads.fetch_add(1, Ordering::Relaxed);
    // SAFETY: the x2APIC window is architecturally defined whenever the mode
    // is enabled (which is the only way this is reached).
    unsafe { rdmsr(0x800 | (reg >> 4)) }
}

pub fn write_x2apic(reg: u32, value: u64) {
    STATS.writes.fetch_add(1, Ordering::Relaxed);
    // SAFETY: see `read_x2apic`.
    unsafe { wrmsr(0x800 | (reg >> 4), value) };
}

/// Try a raw address. This is the only path a driver can reach, and it is the
/// one that produces the "denied" metric.
pub fn try_raw_read(addr: u32) -> Result<u64, Denied> {
    match Msr::from_addr(addr) {
        Some(m) => Ok(read(m)),
        None => {
            deny(addr);
            Err(Denied::NotAllowed)
        }
    }
}

fn deny(addr: u32) {
    STATS.denied.fetch_add(1, Ordering::Relaxed);
    STATS.last_denied.store(addr, Ordering::Relaxed);
    crate::kwarn!("msr: refused access to {:#x} (not allow-listed)", addr);
}

/// SAFETY: caller guarantees the MSR exists.
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
unsafe fn rdmsr(msr: u32) -> u64 {
    let (hi, lo): (u32, u32);
    core::arch::asm!(
        "rdmsr",
        in("ecx") msr,
        out("eax") lo,
        out("edx") hi,
        options(nomem, nostack, preserves_flags)
    );
    ((hi as u64) << 32) | lo as u64
}

#[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
unsafe fn rdmsr(_msr: u32) -> u64 {
    0
}

/// SAFETY: caller guarantees the MSR exists and is writable.
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
unsafe fn wrmsr(msr: u32, value: u64) {
    core::arch::asm!(
        "wrmsr",
        in("ecx") msr,
        in("eax") value as u32,
        in("edx") (value >> 32) as u32,
        options(nostack, preserves_flags)
    );
}

#[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
unsafe fn wrmsr(_msr: u32, _value: u64) {}

/// F040 bring-up: prove the accessor works and that the deny path is armed.
pub fn init() -> u64 {
    // Touching an unknown MSR must be refused without executing anything.
    let probe = 0x0000_04FF;
    let refused = try_raw_read(probe).is_err();
    crate::kinfo!(
        "msr: allow-list armed (probe {:#x} refused={})",
        probe,
        refused
    );
    STATS.reads.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_unique_and_resolvable() {
        const ALL: [Msr; 20] = [
            Msr::Efer,
            Msr::Star,
            Msr::LStar,
            Msr::CStar,
            Msr::SFmask,
            Msr::FsBase,
            Msr::GsBase,
            Msr::KernelGsBase,
            Msr::TscAux,
            Msr::ApicBase,
            Msr::X2ApicBase,
            Msr::Pat,
            Msr::SpecCtrl,
            Msr::ArchCaps,
            Msr::FlushCmd,
            Msr::MiscEnable,
            Msr::Xcr0,
            Msr::Mperf,
            Msr::Aperf,
            Msr::PerfStatus,
        ];
        for m in ALL {
            assert_eq!(Msr::from_addr(m.addr()), Some(m), "{m:?}");
        }
        assert_eq!(Msr::from_addr(0x4FF), None);
    }

    #[test]
    fn well_known_addresses() {
        assert_eq!(Msr::Efer.addr(), 0xC000_0080);
        assert_eq!(Msr::LStar.addr(), 0xC000_0082);
        assert_eq!(Msr::ApicBase.addr(), 0x1B);
        assert_eq!(Msr::X2ApicBase.addr(), 0x800);
        assert_eq!(Msr::GsBase.addr(), 0xC000_0101);
    }

    #[test]
    fn read_only_msrs_refuse_writes() {
        assert!(!Msr::ArchCaps.writable());
        assert!(!Msr::Tsc.writable());
        let before = stats().denied();
        assert_eq!(write(Msr::ArchCaps, 0), Err(Denied::ReadOnly));
        assert_eq!(stats().denied(), before + 1);
        assert_eq!(stats().last_denied(), Msr::ArchCaps.addr());
    }

    #[test]
    fn raw_access_denies_unknown_addresses() {
        let before = stats().denied();
        assert_eq!(try_raw_read(0x1234), Err(Denied::NotAllowed));
        assert_eq!(stats().denied(), before + 1);
        assert_eq!(stats().last_denied(), 0x1234);
    }

    #[test]
    fn efer_bit_positions() {
        assert_eq!(EFER_SCE, 1);
        assert_eq!(EFER_LME, 1 << 8);
        assert_eq!(EFER_LMA, 1 << 10);
        assert_eq!(EFER_NXE, 1 << 11);
        // A long-mode kernel always has LME|LMA|NXE|SCE set.
        let v = EFER_SCE | EFER_LME | EFER_LMA | EFER_NXE;
        assert_ne!(v & EFER_LMA, 0);
        assert_ne!(v & EFER_NXE, 0);
    }
}
