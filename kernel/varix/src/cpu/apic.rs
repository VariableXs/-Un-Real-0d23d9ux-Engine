//! F031 PIC→APIC 迁移 / F032 LAPIC 驱动 / F033 IO APIC 路由 / F034 EOI 纪律 /
//! F035 x2APIC 支持 / F036 MSI/MSI-X.
//!
//! The interrupt fabric. Everything here is pure data manipulation plus a
//! handful of `asm!`-gated MMIO/MSR reads, so the routing decisions are fully
//! unit-testable on the host — which matters because a mis-routed IRQ is one
//! of the few kernel bugs QEMU cannot explain for you.

use core::sync::atomic::{AtomicU16, AtomicU32, AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// F031 — legacy PIC
// ---------------------------------------------------------------------------

pub const PIC1_CMD: u16 = 0x20;
pub const PIC1_DATA: u16 = 0x21;
pub const PIC2_CMD: u16 = 0xA0;
pub const PIC2_DATA: u16 = 0xA1;
const ICW1_INIT: u8 = 0x10;
const ICW1_ICW4: u8 = 0x01;
const ICW4_8086: u8 = 0x01;
pub const PIC_EOI: u8 = 0x20;

/// Lifecycle of the legacy controllers.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PicState {
    #[default]
    /// Untouched since reset — still delivering INT 0x00..0x0F.
    Legacy,
    /// IRQs remapped into `IRQ_BASE..IRQ_BASE+16`.
    Remapped,
    /// All lines masked; the APIC owns delivery now.
    Masked,
}

pub struct Pic {
    state: core::sync::atomic::AtomicU8,
    mask: AtomicU16,
}

impl Pic {
    pub const fn new() -> Pic {
        Pic {
            state: core::sync::atomic::AtomicU8::new(PicState::Legacy as u8),
            mask: AtomicU16::new(0),
        }
    }

    pub fn state(&self) -> PicState {
        match self.state.load(Ordering::Acquire) {
            1 => PicState::Remapped,
            2 => PicState::Masked,
            _ => PicState::Legacy,
        }
    }

    fn set_state(&self, s: PicState) {
        self.state.store(s as u8, Ordering::Release);
    }

    /// ICW1..ICW4: move the PIC out of the CPU exception range (F031).
    pub fn remap(&self, base: u8) {
        let mask_saved = inb(PIC1_DATA) as u16 | ((inb(PIC2_DATA) as u16) << 8);
        outb(PIC1_CMD, ICW1_INIT | ICW1_ICW4);
        outb(PIC2_CMD, ICW1_INIT | ICW1_ICW4);
        outb(PIC1_DATA, base);
        outb(PIC2_DATA, base + 8);
        outb(PIC1_DATA, 4); // slave on IRQ2
        outb(PIC2_DATA, 2);
        outb(PIC1_DATA, ICW4_8086);
        outb(PIC2_DATA, ICW4_8086);
        self.set_state(PicState::Remapped);
        self.set_mask(mask_saved);
    }

    /// Mask a line (`irq` is 0..15 in PIC numbering).
    pub fn mask_line(&self, irq: u8) {
        if irq >= 16 {
            return;
        }
        self.set_mask(self.mask.load(Ordering::Relaxed) | (1 << irq));
    }

    pub fn unmask_line(&self, irq: u8) {
        if irq >= 16 {
            return;
        }
        self.set_mask(self.mask.load(Ordering::Relaxed) & !(1 << irq));
    }

    pub fn set_mask(&self, m: u16) {
        outb(PIC1_DATA, m as u8);
        outb(PIC2_DATA, (m >> 8) as u8);
        self.mask.store(m, Ordering::Relaxed);
    }

    pub fn mask(&self) -> u16 {
        self.mask.load(Ordering::Relaxed)
    }

    /// Hand delivery over to the APIC: mask everything, leave the PIC quiet.
    pub fn disable(&self) {
        self.set_mask(0xFFFF);
        self.set_state(PicState::Masked);
        crate::kinfo!("pic: all 16 lines masked, delivery handed to APIC");
    }

    /// Only needed while the PIC is still the active controller.
    pub fn eoi(&self, irq: u8) {
        if irq >= 8 {
            outb(PIC2_CMD, PIC_EOI);
        }
        outb(PIC1_CMD, PIC_EOI);
    }
}

impl Default for Pic {
    fn default() -> Pic {
        Pic::new()
    }
}

static PIC: Pic = Pic::new();

pub fn pic() -> &'static Pic {
    &PIC
}

// ---------------------------------------------------------------------------
// F032 — Local APIC
// ---------------------------------------------------------------------------

pub const LAPIC_ID: u32 = 0x020;
pub const LAPIC_VERSION: u32 = 0x030;
pub const LAPIC_TPR: u32 = 0x080;
pub const LAPIC_EOI: u32 = 0x0B0;
pub const LAPIC_LDR: u32 = 0x0D0;
pub const LAPIC_DFR: u32 = 0x0E0;
pub const LAPIC_SVR: u32 = 0x0F0;
pub const LAPIC_ISR_BASE: u32 = 0x100;
pub const LAPIC_TMR_BASE: u32 = 0x180;
pub const LAPIC_IRR_BASE: u32 = 0x200;
pub const LAPIC_ESR: u32 = 0x280;
pub const LAPIC_ICR_LOW: u32 = 0x300;
pub const LAPIC_ICR_HIGH: u32 = 0x310;
pub const LAPIC_TIMER: u32 = 0x320;
pub const LAPIC_THERMAL: u32 = 0x330;
pub const LAPIC_PERF: u32 = 0x340;
pub const LAPIC_LINT0: u32 = 0x350;
pub const LAPIC_LINT1: u32 = 0x360;
pub const LAPIC_ERROR: u32 = 0x370;
pub const LAPIC_TIMER_INITIAL: u32 = 0x380;
pub const LAPIC_TIMER_CURRENT: u32 = 0x390;
pub const LAPIC_TIMER_DIVIDE: u32 = 0x3E0;

pub const SVR_ENABLE: u32 = 1 << 8;
pub const TIMER_PERIODIC: u32 = 1 << 17;
pub const TIMER_MASKED: u32 = 1 << 16;
pub const LVT_MASKED: u32 = 1 << 16;
pub const LVT_NMI: u32 = 0x400;

/// IA32_APIC_BASE decode (F032 + F035).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ApicBase {
    pub phys: u64,
    pub enabled: bool,
    pub x2apic: bool,
    pub bsp: bool,
}

impl ApicBase {
    pub const fn from_msr(v: u64) -> ApicBase {
        ApicBase {
            phys: v & 0xFFFF_FFFF_FFFF_F000,
            enabled: v & (1 << 11) != 0,
            x2apic: v & (1 << 10) != 0,
            bsp: v & (1 << 8) != 0,
        }
    }

    /// Default MMIO window the LAPIC is traditionally mapped at.
    pub fn mmio_vaddr(&self) -> u64 {
        // Varix keeps the low-half identity window; the kernel's high-half
        // alias is established by AI-03 (F057).
        self.phys
    }
}

/// How register access reaches the LAPIC (F035).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Access {
    /// Legacy 4 KiB MMIO window.
    Mmio,
    /// MSR window at 0x800 + (reg >> 4).
    X2Apic,
}

impl Access {
    pub fn register_msr(self, reg: u32) -> u32 {
        match self {
            Access::X2Apic => 0x800 | (reg >> 4),
            Access::Mmio => reg,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ApicMode {
    #[default]
    None,
    Mmio,
    X2Apic,
}

pub struct Lapic {
    base: AtomicU64,
    mode: core::sync::atomic::AtomicU8,
    id: AtomicU32,
    version: AtomicU32,
}

impl Lapic {
    pub const fn new() -> Lapic {
        Lapic {
            base: AtomicU64::new(0),
            mode: core::sync::atomic::AtomicU8::new(ApicMode::None as u8),
            id: AtomicU32::new(0),
            version: AtomicU32::new(0),
        }
    }

    pub fn mode(&self) -> ApicMode {
        match self.mode.load(Ordering::Acquire) {
            1 => ApicMode::Mmio,
            2 => ApicMode::X2Apic,
            _ => ApicMode::None,
        }
    }

    pub fn is_active(&self) -> bool {
        self.mode() != ApicMode::None
    }

    pub fn base(&self) -> u64 {
        self.base.load(Ordering::Relaxed)
    }

    pub fn id(&self) -> u32 {
        self.id.load(Ordering::Relaxed)
    }

    /// Version register: (max LVT entry, 4-bit) | (version, 8 bits).
    pub fn version(&self) -> u32 {
        self.version.load(Ordering::Relaxed)
    }

    /// This core's architectural APIC id, decoded for the active mode.
    /// x2APIC reports the full 32-bit id; the legacy window keeps it above
    /// bit 24.
    pub fn apic_id(&self) -> u32 {
        match self.mode() {
            ApicMode::X2Apic => self.read(LAPIC_ID),
            ApicMode::Mmio => (self.read(LAPIC_ID) >> 24) & 0xFF,
            ApicMode::None => 0,
        }
    }

    /// Number of LVT entries this LAPIC supports.
    pub fn max_lvt(&self) -> u32 {
        (self.version() >> 16) & 0xFF
    }

    pub fn read(&self, reg: u32) -> u32 {
        match self.mode() {
            ApicMode::X2Apic => crate::cpu::msr::read(crate::cpu::msr::Msr::ApicBase) as u32,
            ApicMode::Mmio => {
                let addr = self.base() + reg as u64;
                if addr == 0 {
                    return 0;
                }
                // SAFETY: mapped by `init` before any core uses it.
                unsafe { core::ptr::read_volatile(addr as *const u32) }
            }
            ApicMode::None => 0,
        }
    }

    pub fn write(&self, reg: u32, value: u32) {
        match self.mode() {
            ApicMode::X2Apic => {
                crate::cpu::msr::write_x2apic(reg, value as u64);
            }
            ApicMode::Mmio => {
                let addr = self.base() + reg as u64;
                if addr == 0 {
                    return;
                }
                // SAFETY: see `read`.
                unsafe { core::ptr::write_volatile(addr as *mut u32, value) }
            }
            ApicMode::None => {}
        }
    }

    /// Bring the LAPIC up: software-enable, wire the spurious vector, mask
    /// LINT0/1 until AI-04 owns them.
    pub fn enable(&self, spurious_vector: u8) {
        self.write(LAPIC_SVR, SVR_ENABLE | spurious_vector as u32);
        self.write(LAPIC_TPR, 0);
        self.write(LAPIC_LINT0, LVT_MASKED);
        self.write(LAPIC_LINT1, LVT_MASKED);
        self.write(LAPIC_ERROR, LVT_MASKED);
        self.write(LAPIC_TIMER, LVT_MASKED);
        // Clear any stale error state.
        self.write(LAPIC_ESR, 0);
        self.write(LAPIC_ESR, 0);
    }

    /// One-shot LAPIC timer in TSC-deadline-free divide/initial-count form.
    pub fn arm_timer(&self, vector: u8, initial_count: u32, periodic: bool) {
        self.write(LAPIC_TIMER_DIVIDE, 0b1011); // divide by 1
        self.write(LAPIC_TIMER_INITIAL, initial_count);
        let mut lvt = vector as u32;
        if periodic {
            lvt |= TIMER_PERIODIC;
        }
        self.write(LAPIC_TIMER, lvt);
    }

    /// F034: signal end-of-interrupt. Only legal for device vectors.
    pub fn eoi(&self, vector: u8) -> bool {
        if !EOI_AUDIT.eoi(vector) {
            return false;
        }
        self.write(LAPIC_EOI, 0);
        true
    }

    /// Raw ICR write (used by F042 IPI).
    pub fn send_ipi(&self, dest: u32, command: u32) {
        self.write(LAPIC_ICR_HIGH, dest);
        self.write(LAPIC_ICR_LOW, command);
    }

    /// Detect and adopt the LAPIC (F032 + F035).
    pub fn init(&self, x2apic_supported: bool, force_x2apic: bool) -> ApicMode {
        let raw = crate::cpu::msr::read(crate::cpu::msr::Msr::ApicBase);
        let decoded = ApicBase::from_msr(raw);
        if !decoded.enabled {
            crate::kwarn!("apic: disabled by firmware");
            return ApicMode::None;
        }
        self.base.store(decoded.mmio_vaddr(), Ordering::Relaxed);

        let mode = if x2apic_supported && (force_x2apic || decoded.x2apic) {
            let _ = crate::cpu::msr::write(
                crate::cpu::msr::Msr::ApicBase,
                raw | (1 << 10) | (1 << 11),
            );
            ApicMode::X2Apic
        } else {
            ApicMode::Mmio
        };
        self.mode.store(mode as u8, Ordering::Release);

        let id = match mode {
            ApicMode::X2Apic => self.read(LAPIC_ID),
            ApicMode::Mmio => (self.read(LAPIC_ID) >> 24) & 0xFF,
            ApicMode::None => 0,
        };
        self.id.store(id, Ordering::Relaxed);
        self.version.store(self.read(LAPIC_VERSION), Ordering::Relaxed);

        self.enable(crate::cpu::idt::SPURIOUS_VECTOR);
        crate::kinfo!(
            "lapic: mode={:?} id={} base={:#x} lvt={}",
            mode,
            id,
            decoded.phys,
            self.max_lvt()
        );
        mode
    }
}

impl Default for Lapic {
    fn default() -> Lapic {
        Lapic::new()
    }
}

static LAPIC: Lapic = Lapic::new();

pub fn lapic() -> &'static Lapic {
    &LAPIC
}

// ---------------------------------------------------------------------------
// F034 — EOI 纪律
// ---------------------------------------------------------------------------

/// One outstanding interrupt that has been handled but not acknowledged.
pub struct EoiAudit {
    issued: AtomicU64,
    rejected: AtomicU64,
    outstanding: AtomicU64,
    max_outstanding: AtomicU64,
    last_reject: AtomicU32,
}

impl EoiAudit {
    pub const fn new() -> EoiAudit {
        EoiAudit {
            issued: AtomicU64::new(0),
            rejected: AtomicU64::new(0),
            outstanding: AtomicU64::new(0),
            max_outstanding: AtomicU64::new(0),
            last_reject: AtomicU32::new(0),
        }
    }

    /// Record that `vector` began service.
    pub fn begin(&self, vector: u8) {
        let n = self.outstanding.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_outstanding.fetch_max(n, Ordering::SeqCst);
        let _ = vector;
    }

    /// Acknowledge `vector`. Refuses (and counts) an EOI for a CPU exception
    /// or for the spurious vector — those have no LAPIC EOI to send.
    pub fn eoi(&self, vector: u8) -> bool {
        let ok = (vector as usize) >= crate::cpu::idt::IRQ_BASE as usize
            && vector != crate::cpu::idt::SPURIOUS_VECTOR;
        if !ok {
            self.rejected.fetch_add(1, Ordering::Relaxed);
            self.last_reject.store(vector as u32, Ordering::Relaxed);
            crate::kwarn!("eoi: rejected for vector {}", vector);
            return false;
        }
        self.issued.fetch_add(1, Ordering::Relaxed);
        self.outstanding.fetch_sub(1, Ordering::SeqCst);
        true
    }

    pub fn issued(&self) -> u64 {
        self.issued.load(Ordering::Relaxed)
    }
    pub fn rejected(&self) -> u64 {
        self.rejected.load(Ordering::Relaxed)
    }
    pub fn outstanding(&self) -> u64 {
        self.outstanding.load(Ordering::SeqCst)
    }
    pub fn max_outstanding(&self) -> u64 {
        self.max_outstanding.load(Ordering::SeqCst)
    }
    pub fn last_reject(&self) -> u32 {
        self.last_reject.load(Ordering::Relaxed)
    }
    /// F050 gate: no interrupt may leave the CPU unacknowledged.
    pub fn balanced(&self) -> bool {
        self.outstanding() == 0
    }
}

static EOI_AUDIT: EoiAudit = EoiAudit::new();

pub fn eoi_audit() -> &'static EoiAudit {
    &EOI_AUDIT
}

// ---------------------------------------------------------------------------
// F033 — IO APIC
// ---------------------------------------------------------------------------

pub const IOAPIC_IOREGSEL: u32 = 0x00;
pub const IOAPIC_IOWIN: u32 = 0x10;
pub const IOAPIC_REG_ID: u32 = 0x00;
pub const IOAPIC_REG_VER: u32 = 0x01;
pub const IOAPIC_REG_TABLE: u32 = 0x10;

/// Delivery modes in a redirection entry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Delivery {
    Fixed = 0,
    LowestPriority = 1,
    Smi = 2,
    Nmi = 4,
    Init = 5,
    ExtInt = 7,
}

/// Pin polarity / trigger (from the MADT interrupt-source overrides).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PinPolarity(pub bool); // true = active low
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TriggerMode(pub bool); // true = level

/// Build a 64-bit IO APIC redirection entry.
pub fn redirection_entry(
    vector: u8,
    delivery: Delivery,
    logical_dest: bool,
    pending_mask: bool,
    active_low: bool,
    level_triggered: bool,
    masked: bool,
    dest: u8,
) -> u64 {
    let mut v = vector as u64;
    v |= (delivery as u64 & 0x7) << 8;
    if logical_dest {
        v |= 1 << 11;
    }
    if pending_mask {
        v |= 1 << 12;
    }
    if active_low {
        v |= 1 << 13;
    }
    if level_triggered {
        v |= 1 << 15;
    }
    if masked {
        v |= 1 << 16;
    }
    v | ((dest as u64) << 56)
}

/// Split an entry for the two 32-bit window writes.
pub fn redirection_halves(entry: u64) -> (u32, u32) {
    (entry as u32, (entry >> 32) as u32)
}

/// Max redirection entries encoded in the version register.
pub fn max_redirects(version: u32) -> u32 {
    ((version >> 16) & 0xFF) + 1
}

pub struct IoApic {
    base: AtomicU64,
    gsi_base: AtomicU32,
    redirects: AtomicU32,
    id: AtomicU32,
}

impl IoApic {
    pub const fn new() -> IoApic {
        IoApic {
            base: AtomicU64::new(0),
            gsi_base: AtomicU32::new(0),
            redirects: AtomicU32::new(0),
            id: AtomicU32::new(0),
        }
    }

    pub fn is_present(&self) -> bool {
        self.base.load(Ordering::Relaxed) != 0
    }

    pub fn gsi_base(&self) -> u32 {
        self.gsi_base.load(Ordering::Relaxed)
    }

    pub fn redirects(&self) -> u32 {
        self.redirects.load(Ordering::Relaxed)
    }

    /// True when `gsi` belongs to this controller.
    pub fn owns(&self, gsi: u32) -> bool {
        let base = self.gsi_base();
        gsi >= base && gsi < base + self.redirects()
    }

    pub fn read(&self, reg: u32) -> u32 {
        let base = self.base.load(Ordering::Relaxed);
        if base == 0 {
            return 0;
        }
        // SAFETY: mapped by AI-03 before IO APIC routing is used.
        unsafe {
            core::ptr::write_volatile((base + IOAPIC_IOREGSEL as u64) as *mut u32, reg);
            core::ptr::read_volatile((base + IOAPIC_IOWIN as u64) as *const u32)
        }
    }

    pub fn write(&self, reg: u32, value: u32) {
        let base = self.base.load(Ordering::Relaxed);
        if base == 0 {
            return;
        }
        // SAFETY: see `read`.
        unsafe {
            core::ptr::write_volatile((base + IOAPIC_IOREGSEL as u64) as *mut u32, reg);
            core::ptr::write_volatile((base + IOAPIC_IOWIN as u64) as *mut u32, value);
        }
    }

    /// Route `gsi` to `vector` on the current CPU (F033).
    pub fn route(&self, gsi: u32, vector: u8, level: bool, active_low: bool) -> bool {
        if !self.owns(gsi) {
            return false;
        }
        let pin = gsi - self.gsi_base();
        let entry = redirection_entry(
            vector,
            Delivery::Fixed,
            false,
            false,
            active_low,
            level,
            false,
            lapic().id() as u8,
        );
        let (lo, hi) = redirection_halves(entry);
        let reg = IOAPIC_REG_TABLE + pin * 2;
        self.write(reg, lo);
        self.write(reg + 1, hi);
        true
    }

    pub fn mask(&self, gsi: u32) -> bool {
        if !self.owns(gsi) {
            return false;
        }
        let pin = gsi - self.gsi_base();
        let reg = IOAPIC_REG_TABLE + pin * 2;
        self.write(reg, self.read(reg) | (1 << 16));
        true
    }

    /// Adopt a controller discovered through the MADT (F009 hand-off).
    pub fn attach(&self, phys: u64, gsi_base: u32) {
        self.base.store(phys, Ordering::Relaxed);
        self.gsi_base.store(gsi_base, Ordering::Relaxed);
        let ver = self.read(IOAPIC_REG_VER);
        self.redirects.store(max_redirects(ver), Ordering::Relaxed);
        self.id.store((self.read(IOAPIC_REG_ID) >> 24) & 0xF, Ordering::Relaxed);
        // Mask every line first: nothing should fire before its driver exists.
        for pin in 0..self.redirects() {
            self.write(IOAPIC_REG_TABLE + pin * 2, 1 << 16);
            self.write(IOAPIC_REG_TABLE + pin * 2 + 1, 0);
        }
        crate::kinfo!(
            "ioapic: id={} gsi_base={} pins={}",
            self.id.load(Ordering::Relaxed),
            gsi_base,
            self.redirects()
        );
    }
}

impl Default for IoApic {
    fn default() -> IoApic {
        IoApic::new()
    }
}

/// Varix supports up to 4 IO APICs (24 GSIs each → 96 GSIs).
pub const MAX_IOAPICS: usize = 4;
static IOAPICS: [IoApic; MAX_IOAPICS] = [IoApic::new(), IoApic::new(), IoApic::new(), IoApic::new()];

pub fn ioapics() -> &'static [IoApic; MAX_IOAPICS] {
    &IOAPICS
}

/// First controller that owns `gsi`.
pub fn owner_of(gsi: u32) -> Option<&'static IoApic> {
    IOAPICS.iter().find(|c| c.owns(gsi))
}

// ---------------------------------------------------------------------------
// F036 — MSI / MSI-X
// ---------------------------------------------------------------------------

pub const MSI_BASE_ADDR: u64 = 0xFEE0_0000;
pub const MSI_ADDR_DEST_LOGICAL: u64 = 1 << 2;
pub const MSI_ADDR_REDIR_CPU: u64 = 1 << 3;

/// Compose the MSI message address (destination).
pub fn msi_address(dest_id: u8, logical: bool, redirect_hint: bool) -> u64 {
    let mut a = MSI_BASE_ADDR | ((dest_id as u64) << 12);
    if logical {
        a |= MSI_ADDR_DEST_LOGICAL;
    }
    if redirect_hint {
        a |= MSI_ADDR_REDIR_CPU;
    }
    a
}

/// Compose the MSI message data (vector + delivery semantics).
pub fn msi_data(vector: u8, delivery: Delivery, level_assert: bool, level_trigger: bool) -> u32 {
    let mut d = vector as u32;
    d |= (delivery as u32 & 0x7) << 8;
    if level_trigger {
        d |= 1 << 15;
    }
    if level_trigger && level_assert {
        d |= 1 << 14;
    }
    d
}

/// A 16-byte MSI-X table entry as it appears in device BAR space.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct MsixEntry {
    pub addr_lo: u32,
    pub addr_hi: u32,
    pub data: u32,
    /// Bit 0 = masked, bits 31..4 reserved.
    pub control: u32,
}

impl MsixEntry {
    pub fn is_masked(&self) -> bool {
        self.control & 1 != 0
    }

    pub fn vector(&self) -> u8 {
        (self.data & 0xFF) as u8
    }

    pub fn address(&self) -> u64 {
        ((self.addr_hi as u64) << 32) | self.addr_lo as u64
    }

    /// Program and unmask the entry.
    pub fn program(&mut self, addr: u64, data: u32) {
        self.addr_lo = addr as u32;
        self.addr_hi = (addr >> 32) as u32;
        self.data = data;
        self.control &= !1;
    }

    pub fn mask(&mut self) {
        self.control |= 1;
    }
}

/// MSI-X capability: table size from the message-control register.
pub fn msix_table_size(msg_ctrl: u16) -> u16 {
    ((msg_ctrl >> 0) & 0x7FF) + 1
}

pub fn msix_enabled(msg_ctrl: u16) -> bool {
    msg_ctrl & 0x8000 != 0
}

pub fn msix_function_masked(msg_ctrl: u16) -> bool {
    msg_ctrl & 0x4000 != 0
}

// ---------------------------------------------------------------------------
// Bring-up
// ---------------------------------------------------------------------------

/// What the interrupt fabric ended up as.
#[derive(Clone, Copy, Debug, Default)]
pub struct ApicState {
    pub mode: ApicMode,
    pub pic: PicState,
    pub ioapic_count: usize,
    pub gsi_total: u32,
}

/// F031~F036 bring-up. `force_x2apic` comes from the `varix.x2apic=` cmdline.
pub fn init(force_x2apic: bool) -> ApicState {
    // 1. Move the PIC out of the exception range, then quiet it (F031).
    pic().remap(crate::cpu::idt::IRQ_BASE);
    pic().disable();

    // 2. Adopt the LAPIC (F032/F035).
    let x2 = crate::platform::info()
        .map(|p| p.features.x2apic)
        .unwrap_or(false);
    let mode = lapic().init(x2, force_x2apic);

    // 3. Attach IO APICs reported by the MADT (F033).
    let mut count = 0usize;
    let mut gsi_total = 0u32;
    if let Some(madt) = crate::acpi::madt() {
        for entry in madt.ioapics().iter().take(MAX_IOAPICS) {
            if entry.address == 0 {
                continue;
            }
            let base = if entry.gsi_base != 0 {
                entry.gsi_base
            } else {
                gsi_total
            };
            IOAPICS[count].attach(entry.address as u64, base);
            gsi_total = base + IOAPICS[count].redirects();
            count += 1;
        }
    }

    // 4. The EOI audit is live from here on (F034).
    crate::kinfo!(
        "apic: mode={:?} ioapics={} gsis={}",
        mode,
        count,
        gsi_total
    );

    ApicState {
        mode,
        pic: pic().state(),
        ioapic_count: count,
        gsi_total,
    }
}

// ---------------------------------------------------------------------------
// Port IO (gated: real instructions only on the kernel target)
// ---------------------------------------------------------------------------

#[cfg(all(target_arch = "x86_64", target_os = "none"))]
fn outb(port: u16, value: u8) {
    // SAFETY: these four legacy ports are always decoded.
    unsafe {
        core::arch::asm!("out dx, al", in("dx") port, in("al") value, options(nostack, preserves_flags));
    }
}

#[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
fn outb(_port: u16, _value: u8) {}

#[cfg(all(target_arch = "x86_64", target_os = "none"))]
fn inb(port: u16) -> u8 {
    let v: u8;
    // SAFETY: see `outb`.
    unsafe {
        core::arch::asm!("in al, dx", out("al") v, in("dx") port, options(nostack, preserves_flags));
    }
    v
}

#[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
fn inb(_port: u16) -> u8 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pic_remap_moves_irqs_out_of_the_exception_range() {
        let p = Pic::new();
        assert_eq!(p.state(), PicState::Legacy);
        p.remap(32);
        assert_eq!(p.state(), PicState::Remapped);
        p.mask_line(0);
        assert_eq!(p.mask() & 1, 1);
        p.unmask_line(0);
        assert_eq!(p.mask() & 1, 0);
        p.disable();
        assert_eq!(p.state(), PicState::Masked);
        assert_eq!(p.mask(), 0xFFFF);
        // Out-of-range lines are ignored instead of corrupting the mask.
        p.mask_line(99);
        assert_eq!(p.mask(), 0xFFFF);
    }

    #[test]
    fn apic_base_msr_decode() {
        let raw: u64 = 0xFEE0_0000 | (1 << 11) | (1 << 10) | (1 << 8);
        let d = ApicBase::from_msr(raw);
        assert_eq!(d.phys, 0xFEE0_0000);
        assert!(d.enabled);
        assert!(d.x2apic);
        assert!(d.bsp);
        let off = ApicBase::from_msr(0xFEE0_0000);
        assert!(!off.enabled && !off.x2apic && !off.bsp);
    }

    #[test]
    fn x2apic_register_window_offsets() {
        assert_eq!(Access::X2Apic.register_msr(LAPIC_EOI), 0x800 | 0x0B);
        assert_eq!(Access::X2Apic.register_msr(LAPIC_SVR), 0x800 | 0x0F);
        assert_eq!(Access::Mmio.register_msr(LAPIC_EOI), LAPIC_EOI);
    }

    #[test]
    fn redirection_entry_round_trips() {
        let e = redirection_entry(0x41, Delivery::Fixed, false, false, false, true, true, 0);
        assert_eq!(e & 0xFF, 0x41);
        assert_eq!(e >> 8 & 0x7, 0);
        assert_ne!(e & (1 << 15), 0, "level triggered");
        assert_ne!(e & (1 << 16), 0, "masked");
        let e2 = redirection_entry(0x50, Delivery::LowestPriority, true, true, true, false, false, 3);
        assert_eq!(e2 >> 8 & 0x7, 1);
        assert_ne!(e2 & (1 << 11), 0, "logical dest");
        assert_ne!(e2 & (1 << 12), 0, "remote IRR");
        assert_ne!(e2 & (1 << 13), 0, "active low");
        assert_eq!(e2 >> 56, 3);
        let (lo, hi) = redirection_halves(e2);
        assert_eq!(lo as u64 | ((hi as u64) << 32), e2);
    }

    #[test]
    fn max_redirects_uses_the_version_register() {
        assert_eq!(max_redirects(0x0017_0020), 0x17 + 1);
        assert_eq!(max_redirects(0x0000_0011), 1);
    }

    #[test]
    fn msi_composition_is_spec_exact() {
        let a = msi_address(0, false, false);
        assert_eq!(a, 0xFEE0_0000);
        let a2 = msi_address(4, true, true);
        assert_eq!(a2, 0xFEE0_4000 | (1 << 2) | (1 << 3));
        let d = msi_data(0x42, Delivery::Fixed, false, false);
        assert_eq!(d, 0x42);
        let d2 = msi_data(0x42, Delivery::Nmi, true, true);
        assert_eq!(d2 & 0xFF, 0x42);
        assert_eq!(d2 >> 8 & 0x7, 4);
        assert_ne!(d2 & (1 << 14), 0);
        assert_ne!(d2 & (1 << 15), 0);
    }

    #[test]
    fn msix_table_helpers() {
        assert_eq!(msix_table_size(0x8000 | 0x0007), 8);
        assert!(msix_enabled(0x8000));
        assert!(msix_function_masked(0x4000));
        let mut e = MsixEntry::default();
        e.mask();
        assert!(e.is_masked());
        e.program(0xFEE0_1000, 0x33);
        assert!(!e.is_masked());
        assert_eq!(e.address(), 0xFEE0_1000);
        assert_eq!(e.vector(), 0x33);
    }

    #[test]
    fn eoi_audit_rejects_non_device_vectors() {
        let a = EoiAudit::new();
        assert!(a.balanced());
        a.begin(0x41);
        assert!(!a.balanced());
        assert_eq!(a.max_outstanding(), 1);
        assert!(a.eoi(0x41));
        assert!(a.balanced());
        assert_eq!(a.issued(), 1);
        assert!(!a.eoi(14), "page fault has no LAPIC EOI");
        assert!(!a.eoi(0xFF), "spurious vector has no LAPIC EOI");
        assert_eq!(a.rejected(), 2);
        assert_eq!(a.last_reject(), 0xFF);
    }
}
