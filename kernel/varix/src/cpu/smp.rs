//! F041 SMP trampoline / F042 核间 IPI / F047 per-CPU 数据区.
//!
//! Bringing up application processors is the one place where "it works on my
//! machine" is a real hazard: the handshake has to survive a core that never
//! answers. Everything here is therefore explicit about timeouts and leaves a
//! machine-readable reason behind.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

/// Maximum cores Varix drives. 64 keeps the per-CPU table at a fixed size and
/// matches the x2APIC logical-destination reality of current hardware.
pub const MAX_CPUS: usize = 64;

/// Page number the AP trampoline is copied to (real-mode segment = 0x08xx).
pub const TRAMPOLINE_PAGE: u8 = 0x08;
/// APs start in real mode: `TRAMPOLINE_PAGE << 8` is the reset vector.
pub const TRAMPOLINE_VECTOR: u8 = TRAMPOLINE_PAGE;
/// How long to wait for an AP to report ready, in 10 ms units.
pub const AP_READY_TIMEOUT_10MS: u32 = 200; // 2 s

// ---------------------------------------------------------------------------
// F047 — per-CPU data
// ---------------------------------------------------------------------------

/// The per-core control block. Only atomics live here: the area is reachable
/// from any core at any time, and a torn read would be a race the kernel cannot
/// afford to reason about.
pub struct PerCpu {
    pub cpu_id: AtomicU32,
    pub lapic_id: AtomicU32,
    pub online: AtomicBool,
    /// Scheduler heartbeat count for this core.
    pub ticks: AtomicU64,
    /// IRQs received since boot.
    pub irqs: AtomicU64,
    pub ipi_sent: AtomicU64,
    pub ipi_received: AtomicU64,
    /// Current interrupt nesting depth (F045).
    pub irq_depth: AtomicU32,
    /// Set by the scheduler when this core must re-evaluate its run queue.
    pub reschedule: AtomicBool,
    /// Thread currently running here (`u32::MAX` = idle).
    pub current_tid: AtomicU32,
    /// Scratch words for the IRQ entry path (kept out of the stack).
    pub scratch: [AtomicU64; 4],
}

impl PerCpu {
    const fn new() -> PerCpu {
        PerCpu {
            cpu_id: AtomicU32::new(u32::MAX),
            lapic_id: AtomicU32::new(0),
            online: AtomicBool::new(false),
            ticks: AtomicU64::new(0),
            irqs: AtomicU64::new(0),
            ipi_sent: AtomicU64::new(0),
            ipi_received: AtomicU64::new(0),
            irq_depth: AtomicU32::new(0),
            reschedule: AtomicBool::new(false),
            current_tid: AtomicU32::new(u32::MAX),
            scratch: [
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
            ],
        }
    }

    pub fn id(&self) -> u32 {
        self.cpu_id.load(Ordering::Relaxed)
    }

    pub fn is_online(&self) -> bool {
        self.online.load(Ordering::Acquire)
    }

    pub fn note_tick(&self) {
        self.ticks.fetch_add(1, Ordering::Relaxed);
    }

    pub fn note_irq(&self) {
        self.irqs.fetch_add(1, Ordering::Relaxed);
    }

    /// F045: enter an interrupt handler. Returns the new depth.
    pub fn irq_enter(&self) -> u32 {
        self.irq_depth.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn irq_leave(&self) -> u32 {
        let d = self.irq_depth.load(Ordering::SeqCst);
        if d == 0 {
            return 0;
        }
        self.irq_depth.fetch_sub(1, Ordering::SeqCst);
        d - 1
    }

    pub fn irq_depth(&self) -> u32 {
        self.irq_depth.load(Ordering::SeqCst)
    }
}

static CPUS: [PerCpu; MAX_CPUS] = [const { PerCpu::new() }; MAX_CPUS];
static CPU_COUNT: AtomicU32 = AtomicU32::new(0);

pub fn percpu(cpu_id: u32) -> Option<&'static PerCpu> {
    CPUS.get(cpu_id as usize).filter(|c| c.id() == cpu_id)
}

pub fn cpus() -> &'static [PerCpu; MAX_CPUS] {
    &CPUS
}

pub fn cpu_count() -> u32 {
    CPU_COUNT.load(Ordering::Acquire)
}

/// Register a core. The BSP is core 0 and is registered before anything else.
pub fn register(cpu_id: u32, lapic_id: u32) -> Option<&'static PerCpu> {
    let slot = CPUS.get(cpu_id as usize)?;
    slot.cpu_id.store(cpu_id, Ordering::Relaxed);
    slot.lapic_id.store(lapic_id, Ordering::Relaxed);
    slot.online.store(true, Ordering::Release);
    CPU_COUNT.fetch_max(cpu_id + 1, Ordering::AcqRel);
    Some(slot)
}

/// This core's control block. On the kernel target the per-CPU area is found
/// through `GS_BASE`; on the host (tests) there is only the BSP.
pub fn current() -> &'static PerCpu {
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    {
        let base = crate::cpu::msr::read(crate::cpu::msr::Msr::GsBase);
        if base != 0 {
            // SAFETY: `GS_BASE` always points at a `PerCpu` registered above.
            return unsafe { &*(base as *const PerCpu) };
        }
    }
    &CPUS[0]
}

pub fn current_id() -> u32 {
    current().id()
}

// ---------------------------------------------------------------------------
// F042 — inter-processor interrupts
// ---------------------------------------------------------------------------

pub const IPI_RESCHEDULE: u8 = 0xF0;
pub const IPI_TLB_SHOOTDOWN: u8 = 0xF1;
pub const IPI_PANIC: u8 = 0xF2;
pub const IPI_HALT: u8 = 0xF3;
pub const IPI_CALL: u8 = 0xF4;
pub const IPI_WAKE: u8 = 0xF5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IpiKind {
    Reschedule,
    TlbShootdown,
    Panic,
    Halt,
    Call,
    Wake,
}

impl IpiKind {
    pub const fn vector(self) -> u8 {
        match self {
            IpiKind::Reschedule => IPI_RESCHEDULE,
            IpiKind::TlbShootdown => IPI_TLB_SHOOTDOWN,
            IpiKind::Panic => IPI_PANIC,
            IpiKind::Halt => IPI_HALT,
            IpiKind::Call => IPI_CALL,
            IpiKind::Wake => IPI_WAKE,
        }
    }

    pub fn from_vector(v: u8) -> Option<IpiKind> {
        match v {
            IPI_RESCHEDULE => Some(IpiKind::Reschedule),
            IPI_TLB_SHOOTDOWN => Some(IpiKind::TlbShootdown),
            IPI_PANIC => Some(IpiKind::Panic),
            IPI_HALT => Some(IpiKind::Halt),
            IPI_CALL => Some(IpiKind::Call),
            IPI_WAKE => Some(IpiKind::Wake),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            IpiKind::Reschedule => "reschedule",
            IpiKind::TlbShootdown => "tlb-shootdown",
            IpiKind::Panic => "panic",
            IpiKind::Halt => "halt",
            IpiKind::Call => "call",
            IpiKind::Wake => "wake",
        }
    }
}

/// Destination shorthand encoded in ICR bits 18..19.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IpiTarget {
    /// Explicit APIC id in the destination field.
    ApicId(u32),
    /// This core.
    Self_,
    /// Every core including the sender.
    All,
    /// Every core except the sender.
    Others,
}

impl IpiTarget {
    pub const fn shorthand(self) -> u32 {
        match self {
            IpiTarget::ApicId(_) => 0b00,
            IpiTarget::Self_ => 0b01,
            IpiTarget::All => 0b10,
            IpiTarget::Others => 0b11,
        }
    }

    pub const fn dest(self) -> u32 {
        match self {
            IpiTarget::ApicId(id) => id,
            _ => 0,
        }
    }
}

/// ICR delivery modes used by Varix.
pub const DM_FIXED: u32 = 0b000;
pub const DM_NMI: u32 = 0b100;
pub const DM_INIT: u32 = 0b101;
pub const DM_STARTUP: u32 = 0b110;

/// Compose the low half of the interrupt command register.
pub const fn icr_low(vector: u8, delivery: u32, level_assert: bool, shorthand: u32) -> u32 {
    let mut v = vector as u32;
    v |= (delivery & 0x7) << 8;
    v |= (shorthand & 0x3) << 18;
    if level_assert {
        v |= 1 << 14;
    }
    v
}

/// Per-kind counters: a stuck core is usually visible here first.
pub struct IpiStats {
    sent: [AtomicU64; 6],
    received: [AtomicU64; 6],
}

impl IpiStats {
    const fn new() -> IpiStats {
        IpiStats {
            sent: [const { AtomicU64::new(0) }; 6],
            received: [const { AtomicU64::new(0) }; 6],
        }
    }
    fn idx(k: IpiKind) -> usize {
        match k {
            IpiKind::Reschedule => 0,
            IpiKind::TlbShootdown => 1,
            IpiKind::Panic => 2,
            IpiKind::Halt => 3,
            IpiKind::Call => 4,
            IpiKind::Wake => 5,
        }
    }
    pub fn sent(&self, k: IpiKind) -> u64 {
        self.sent[Self::idx(k)].load(Ordering::Relaxed)
    }
    pub fn received(&self, k: IpiKind) -> u64 {
        self.received[Self::idx(k)].load(Ordering::Relaxed)
    }
    fn note_sent(&self, k: IpiKind) {
        self.sent[Self::idx(k)].fetch_add(1, Ordering::Relaxed);
    }
    fn note_received(&self, k: IpiKind) {
        self.received[Self::idx(k)].fetch_add(1, Ordering::Relaxed);
    }
}

static IPI_STATS: IpiStats = IpiStats::new();

pub fn ipi_stats() -> &'static IpiStats {
    &IPI_STATS
}

/// Send an IPI. `Panic`/`Halt` are NMI-delivered so they land even with IRQs
/// masked on the target.
pub fn send(kind: IpiKind, target: IpiTarget) {
    let (vector, delivery) = match kind {
        IpiKind::Panic | IpiKind::Halt => (0, DM_NMI),
        other => (other.vector(), DM_FIXED),
    };
    let low = icr_low(vector, delivery, true, target.shorthand());
    let high = target.dest() << 24;
    crate::cpu::apic::lapic().send_ipi(high, low);
    IPI_STATS.note_sent(kind);
    if let IpiTarget::ApicId(id) = target {
        if let Some(c) = cpus().iter().find(|c| c.lapic_id.load(Ordering::Relaxed) == id) {
            c.ipi_received.fetch_add(1, Ordering::Relaxed);
        }
    }
    current().ipi_sent.fetch_add(1, Ordering::Relaxed);
}

/// Called from the interrupt path when an IPI vector arrives.
pub fn handle_ipi(vector: u8) -> Option<IpiKind> {
    let kind = IpiKind::from_vector(vector)?;
    IPI_STATS.note_received(kind);
    let me = current();
    me.ipi_received.fetch_add(1, Ordering::Relaxed);
    if kind == IpiKind::Reschedule {
        me.reschedule.store(true, Ordering::Release);
    }
    Some(kind)
}

// ---------------------------------------------------------------------------
// F041 — AP trampoline
// ---------------------------------------------------------------------------

/// Why an AP failed to start.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApError {
    /// cpu_id outside `MAX_CPUS`.
    BadCpuId,
    /// The trampoline was already handed to another core.
    Busy,
    /// Core never set its ready flag.
    Timeout,
}

/// The real-mode → long-mode hand-off block, shared with the assembly stub.
#[repr(C)]
pub struct ApParams {
    /// Guards `stack_top`/`cr3`/`entry` being visible to the AP.
    pub ready_flag: AtomicU32,
    /// Stack the AP should switch to (top, 16-byte aligned).
    pub stack_top: AtomicU64,
    /// Page table root in physical address form.
    pub cr3: AtomicU64,
    /// Long-mode entry point.
    pub entry: AtomicU64,
    /// GDT pseudo-descriptor copied by the stub.
    pub gdt_limit: AtomicU32,
    pub gdt_base: AtomicU64,
    /// Core index handed to the AP.
    pub cpu_id: AtomicU32,
}

impl ApParams {
    pub const fn new() -> ApParams {
        ApParams {
            ready_flag: AtomicU32::new(0),
            stack_top: AtomicU64::new(0),
            cr3: AtomicU64::new(0),
            entry: AtomicU64::new(0),
            gdt_limit: AtomicU32::new(0),
            gdt_base: AtomicU64::new(0),
            cpu_id: AtomicU32::new(u32::MAX),
        }
    }

    pub fn is_claimed(&self) -> bool {
        self.ready_flag.load(Ordering::Acquire) != 0
    }
}

static AP_PARAMS: ApParams = ApParams::new();

pub fn ap_params() -> &'static ApParams {
    &AP_PARAMS
}

/// INIT–SIPI–SIPI, expressed as data so the sequence is testable.
pub const SIPI_COUNT: usize = 2;

pub struct Trampoline {
    busy: AtomicBool,
}

impl Trampoline {
    pub const fn new() -> Trampoline {
        Trampoline {
            busy: AtomicBool::new(false),
        }
    }

    /// Fill the hand-off block. The store to `ready_flag` is the release that
    /// publishes every other field to the AP.
    pub fn prepare(&self, cpu_id: u32, stack_top: u64, cr3: u64, entry: u64) -> Result<(), ApError> {
        if cpu_id as usize >= MAX_CPUS {
            return Err(ApError::BadCpuId);
        }
        if self.busy.swap(true, Ordering::AcqRel) {
            return Err(ApError::Busy);
        }
        AP_PARAMS.stack_top.store(stack_top, Ordering::Relaxed);
        AP_PARAMS.cr3.store(cr3, Ordering::Relaxed);
        AP_PARAMS.entry.store(entry, Ordering::Relaxed);
        AP_PARAMS.cpu_id.store(cpu_id, Ordering::Relaxed);
        let (limit, base) = crate::cpu::gdt::tables().descriptor();
        AP_PARAMS.gdt_limit.store(limit as u32, Ordering::Relaxed);
        AP_PARAMS.gdt_base.store(base, Ordering::Relaxed);
        AP_PARAMS.ready_flag.store(1, Ordering::Release);
        Ok(())
    }

    /// The three (ICR high, ICR low) writes that start the core.
    pub fn startup_commands(&self, apic_id: u32) -> [(u32, u32); 3] {
        let high = apic_id << 24;
        let init_deassert = icr_low(0, DM_INIT, false, IpiTarget::ApicId(apic_id).shorthand());
        let init_assert = icr_low(0, DM_INIT, true, IpiTarget::ApicId(apic_id).shorthand());
        let sipi = icr_low(
            TRAMPOLINE_VECTOR,
            DM_STARTUP,
            true,
            IpiTarget::ApicId(apic_id).shorthand(),
        );
        [(high, init_deassert), (high, init_assert), (high, sipi)]
    }

    /// Poll for the ready flag; returns `Err(Timeout)` instead of hanging.
    pub fn wait_ready(&self, timeout_10ms: u32) -> Result<(), ApError> {
        for _ in 0..timeout_10ms.max(1) {
            if crate::cpu::smp::percpu(AP_PARAMS.cpu_id.load(Ordering::Acquire))
                .map(|c| c.is_online())
                .unwrap_or(false)
            {
                self.busy.store(false, Ordering::Release);
                return Ok(());
            }
            delay_10ms();
        }
        self.busy.store(false, Ordering::Release);
        Err(ApError::Timeout)
    }

    /// Release the trampoline after a failed start.
    pub fn abort(&self) {
        AP_PARAMS.ready_flag.store(0, Ordering::Release);
        self.busy.store(false, Ordering::Release);
    }
}

static TRAMPOLINE: Trampoline = Trampoline::new();

pub fn trampoline() -> &'static Trampoline {
    &TRAMPOLINE
}

fn delay_10ms() {
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    {
        // Busy-wait on the TSC: 10 ms is short and this runs before timers.
        let cal = crate::cpu::clock::calibration();
        let target = crate::cpu::clock::read_tsc() + cal.ns_to_tsc(10_000_000);
        while crate::cpu::clock::read_tsc() < target {}
    }
    #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
    {
        let mut spin = 0u64;
        for i in 0..50_000u64 {
            spin = spin.wrapping_add(i);
        }
        core::hint::black_box(spin);
    }
}

/// F041 + F047 bring-up: register the BSP, then start every AP the MADT lists.
pub fn init() -> u32 {
    let bsp_id = crate::cpu::apic::lapic().id();
    register(0, bsp_id);
    let mut started = 1u32;

    if let Some(madt) = crate::acpi::madt() {
        for core in madt.cpus().iter() {
            if !core.enabled || core.acpi_id == 0 {
                continue;
            }
            // The BSP is whichever core is already running.
            if core.apic_id == bsp_id {
                continue;
            }
            if started as usize >= MAX_CPUS {
                break;
            }
            let cpu_id = started;
            // Stack: 64 KiB for this core, from the reserved AP stack region.
            let stack_top = ap_stack_top(cpu_id);
            let cr3 = current_cr3();
            let long_mode_entry = ap_entry();
            match trampoline().prepare(cpu_id, stack_top, cr3, long_mode_entry) {
                Ok(()) => {
                    for (high, low) in trampoline().startup_commands(core.apic_id) {
                        crate::cpu::apic::lapic().send_ipi(high, low);
                    }
                    match trampoline().wait_ready(AP_READY_TIMEOUT_10MS) {
                        Ok(()) => {
                            register(cpu_id, core.apic_id);
                            started += 1;
                        }
                        Err(e) => {
                            crate::kwarn!("smp: apic {} failed: {:?}", core.apic_id, e);
                            trampoline().abort();
                        }
                    }
                }
                Err(e) => {
                    crate::kwarn!("smp: cannot start apic {}: {:?}", core.apic_id, e);
                    trampoline().abort();
                }
            }
        }
    }
    crate::kinfo!("smp: {} core(s) online (bsp apic {})", started, bsp_id);
    started
}

/// 64 KiB stack per AP, allocated from a boot-reserved region.
pub const AP_STACK_SIZE: usize = 64 * 1024;
pub const AP_STACKS: usize = MAX_CPUS - 1;

static AP_STACK_AREA: [u8; AP_STACK_SIZE * AP_STACKS] = [0u8; AP_STACK_SIZE * AP_STACKS];

fn ap_stack_top(cpu_id: u32) -> u64 {
    let idx = (cpu_id as usize).saturating_sub(1).min(AP_STACKS - 1);
    let base = AP_STACK_AREA.as_ptr() as u64;
    // Stacks grow down: hand out the *end* of this core's slice.
    base + ((idx + 1) * AP_STACK_SIZE) as u64
}

fn current_cr3() -> u64 {
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    {
        let v: u64;
        // SAFETY: reading CR3 has no side effects.
        unsafe {
            core::arch::asm!("mov {}, cr3", out(reg) v, options(nomem, nostack, preserves_flags));
        }
        v
    }
    #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
    {
        0
    }
}

/// The long-mode entry point an AP jumps to once the real-mode stub has set up
/// paging and CR0. It runs on the AP's own 64 KiB stack (handed over through
/// `ApParams`), claims its per-CPU block and parks until the scheduler (AI-04)
/// has a thread to run there.
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
#[no_mangle]
pub extern "C" fn varix_ap_entry() -> ! {
    let params = ap_params();
    // Acquire pairs with the Release store in `Trampoline::prepare`, so every
    // field below is guaranteed visible.
    let _ = params.ready_flag.load(Ordering::Acquire);
    let cpu_id = params.cpu_id.load(Ordering::Relaxed);
    let lapic_id = crate::cpu::apic::lapic().apic_id();
    match register(cpu_id, lapic_id) {
        Some(me) => {
            // F047: every core addresses its control block through GS_BASE.
            let base = me as *const PerCpu as u64;
            let _ = crate::cpu::msr::write(crate::cpu::msr::Msr::GsBase, base);
            crate::kinfo!("smp: core {} online (apic {})", cpu_id, lapic_id);
        }
        None => crate::kerror!("smp: core {} has no control block", cpu_id),
    }
    // Park with interrupts off; the scheduler owns `sti` on this core too.
    loop {
        // SAFETY: halting until the next interrupt is always correct here.
        unsafe { core::arch::asm!("hlt", options(nomem, nostack, preserves_flags)) };
    }
}

/// Long-mode entry for APs (`extern` symbol in the trampoline stub).
fn ap_entry() -> u64 {
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    {
        extern "C" {
            fn varix_ap_entry() -> !;
        }
        // The symbol is provided by the AP trampoline stub; taking a function
        // item's address is safe.
        varix_ap_entry as *const () as u64
    }
    #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
    {
        0xFFFF_8000_0001_0000
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_cpu_register_and_lookup() {
        let c = register(3, 12).unwrap();
        assert_eq!(c.id(), 3);
        assert_eq!(c.lapic_id.load(Ordering::Relaxed), 12);
        assert!(c.is_online());
        assert_eq!(percpu(3).map(|x| x.id()), Some(3));
        assert!(percpu(200).is_none());
        assert!(cpu_count() >= 4);
    }

    #[test]
    fn irq_nesting_depth_is_balanced() {
        let c = PerCpu::new();
        assert_eq!(c.irq_depth(), 0);
        assert_eq!(c.irq_enter(), 1);
        assert_eq!(c.irq_enter(), 2);
        assert_eq!(c.irq_leave(), 1);
        assert_eq!(c.irq_leave(), 0);
        // Underflow is clamped: a stray `irq_leave` must not wrap.
        assert_eq!(c.irq_leave(), 0);
    }

    #[test]
    fn ipi_vectors_round_trip() {
        for k in [
            IpiKind::Reschedule,
            IpiKind::TlbShootdown,
            IpiKind::Panic,
            IpiKind::Halt,
            IpiKind::Call,
            IpiKind::Wake,
        ] {
            assert_eq!(IpiKind::from_vector(k.vector()), Some(k));
            assert!(!k.name().is_empty());
        }
        assert_eq!(IpiKind::from_vector(0x41), None);
        // All IPI vectors live above the device range and below spurious.
        assert!(IPI_RESCHEDULE as usize >= crate::cpu::idt::IRQ_BASE as usize);
        assert_ne!(IPI_PANIC, crate::cpu::idt::SPURIOUS_VECTOR);
    }

    #[test]
    fn icr_encoding_matches_the_arch() {
        // Fixed delivery to an explicit id.
        let low = icr_low(0xF0, DM_FIXED, true, IpiTarget::ApicId(7).shorthand());
        assert_eq!(low & 0xFF, 0xF0);
        assert_eq!(low >> 8 & 0x7, 0);
        assert_ne!(low & (1 << 14), 0);
        assert_eq!(low >> 18 & 0x3, 0);
        // Startup (SIPI) with shorthand "all but self".
        let low2 = icr_low(TRAMPOLINE_VECTOR, DM_STARTUP, true, IpiTarget::Others.shorthand());
        assert_eq!(low2 >> 8 & 0x7, 0b110);
        assert_eq!(low2 >> 18 & 0x3, 0b11);
        assert_eq!(IpiTarget::All.shorthand(), 0b10);
        assert_eq!(IpiTarget::Self_.shorthand(), 0b01);
        assert_eq!(IpiTarget::ApicId(9).dest(), 9);
    }

    #[test]
    fn trampoline_prepare_and_startup_sequence() {
        let t = Trampoline::new();
        assert!(t.prepare(1, 0x9000, 0x1000, 0xFFFF_8000_0000_0000).is_ok());
        assert!(AP_PARAMS.is_claimed());
        assert_eq!(AP_PARAMS.stack_top.load(Ordering::Relaxed), 0x9000);
        assert_eq!(AP_PARAMS.cr3.load(Ordering::Relaxed), 0x1000);
        assert_eq!(AP_PARAMS.cpu_id.load(Ordering::Relaxed), 1);
        let cmds = t.startup_commands(4);
        assert_eq!(cmds.len(), 3);
        assert_eq!(cmds[0].0, 4 << 24); // INIT de-assert
        assert_eq!(cmds[1].0, 4 << 24); // INIT
        assert_eq!(cmds[2].0, 4 << 24); // SIPI
        assert_eq!(cmds[2].1 & 0xFF, TRAMPOLINE_VECTOR as u32);
        // Busy trampoline refuses a second core.
        assert_eq!(t.prepare(2, 0, 0, 0), Err(ApError::Busy));
        assert_eq!(t.prepare(999, 0, 0, 0), Err(ApError::BadCpuId));
        t.abort();
        assert!(!AP_PARAMS.is_claimed());
    }

    #[test]
    fn ap_stack_slices_do_not_overlap() {
        let a = ap_stack_top(1);
        let b = ap_stack_top(2);
        assert_eq!(b - a, AP_STACK_SIZE as u64);
        assert!(a >= AP_STACK_AREA.as_ptr() as u64);
    }
}
