//! AI-02 · CPU 与中断域（F026~F050）.
//!
//! Bring-up order matters more here than anywhere else in the kernel: the
//! descriptor tables must exist before an exception can be taken, the APIC
//! must be moving before a timer can be armed, and the clock must be
//! calibrated before the first latency sample means anything. `init` runs that
//! order and reports what actually happened, so the boot log answers "how many
//! vectors, which clock source, how many cores" without anyone guessing.

pub mod apic;
pub mod clock;
pub mod cpuinfo;
pub mod gdt;
pub mod idt;
pub mod msr;
pub mod smp;
pub mod sync;

/// Everything the rest of the kernel may want to know about this domain.
#[derive(Clone, Copy, Debug, Default)]
pub struct CpuDomainState {
    /// F026/F027 — GDT + TSS + TR live.
    pub gdt_ok: bool,
    /// F028 — number of IDT vectors with an entry.
    pub idt_vectors: usize,
    /// F031 — the legacy PIC is masked and quiet.
    pub pic_masked: bool,
    /// F032/F035 — which delivery mode the LAPIC ended up in.
    pub apic: apic::ApicMode,
    /// F033 — IO APICs adopted.
    pub ioapics: usize,
    /// F033 — total GSIs available across them.
    pub gsis: u32,
    /// F036 — MSI/MSI-X is architecturally available.
    pub msi_capable: bool,
    /// F037/F038/F039 — the clock's chosen source.
    pub clock: clock::ClockSource,
    /// F038 — calibrated TSC frequency.
    pub tsc_hz: u64,
    /// F039 — HPET adopted.
    pub hpet: bool,
    /// F041/F047 — cores online.
    pub cpus: u32,
    /// F048 — highest ISA level the kernel may execute.
    pub isa: &'static str,
    /// F050 — (passed, failed) from the interrupt-chain self-test.
    pub self_test: (usize, usize),
}

impl CpuDomainState {
    pub fn ok(&self) -> bool {
        self.gdt_ok && self.idt_vectors == idt::IDT_VECTORS && self.self_test.1 == 0
    }

    /// One-line HUD summary:
    /// `cpu cores=8 vectors=256 ioapic=1 gsi=24 clk=tsc tsc=3000MHz isa=avx2 [9/9]`
    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut w = Hud::new(out);
        w.str("cpu cores=");
        w.num(self.cpus as u64);
        w.str(" vectors=");
        w.num(self.idt_vectors as u64);
        w.str(" ioapic=");
        w.num(self.ioapics as u64);
        w.str(" gsi=");
        w.num(self.gsis as u64);
        w.str(" clk=");
        w.str(self.clock.as_str());
        w.str(" tsc=");
        w.num(self.tsc_hz / 1_000_000);
        w.str("MHz isa=");
        w.str(self.isa);
        w.str(" [");
        w.num(self.self_test.0 as u64);
        w.str("/");
        w.num((self.self_test.0 + self.self_test.1) as u64);
        w.str("]\n");
        w.used()
    }
}

/// Bounded ASCII writer for the HUD line. A struct (not a closure) because two
/// closures over the same `&mut [u8]` is exactly the kind of aliasing the
/// borrow checker exists to refuse.
pub struct Hud<'a> {
    out: &'a mut [u8],
    n: usize,
}

impl<'a> Hud<'a> {
    pub fn new(out: &'a mut [u8]) -> Hud<'a> {
        Hud { out, n: 0 }
    }

    pub fn str(&mut self, s: &str) {
        for &b in s.as_bytes() {
            if self.n < self.out.len() {
                self.out[self.n] = b;
                self.n += 1;
            }
        }
    }

    pub fn num(&mut self, v: u64) {
        let mut buf = [0u8; 20];
        let mut w = 0usize;
        let mut v = v;
        if v == 0 {
            buf[0] = b'0';
            w = 1;
        }
        while v > 0 && w < buf.len() {
            buf[w] = b'0' + (v % 10) as u8;
            v /= 10;
            w += 1;
        }
        while w > 0 {
            w -= 1;
            let b = buf[w];
            if self.n < self.out.len() {
                self.out[self.n] = b;
                self.n += 1;
            }
        }
    }

    pub fn used(&self) -> usize {
        self.n
    }
}

/// F026~F050 bring-up, in dependency order.
pub fn init() -> CpuDomainState {
    let mut st = CpuDomainState::default();

    // F026/F027 — descriptors first: without a GDT an exception cannot be
    // delivered at all, and without a TSS there is nowhere to land ring 0.
    st.gdt_ok = gdt::init();

    // F028/F029/F030 — 256 vectors, each with real entry code.
    st.idt_vectors = idt::init();

    // F040 — the MSR door, before anything reads model-specific state.
    let _ = msr::init();

    // F031~F036 — the interrupt fabric.
    let pic_done = {
        let x2apic_forced = crate::cmdline::flag("varix.x2apic");
        let a = apic::init(x2apic_forced);
        st.apic = a.mode;
        st.pic_masked = a.pic == apic::PicState::Masked;
        st.ioapics = a.ioapic_count;
        st.gsis = a.gsi_total;
        st.msi_capable = a.mode != apic::ApicMode::None;
        a.pic
    };
    let _ = pic_done;

    // F037/F038/F039 — time. HPET is adopted only when ACPI handed one over.
    st.clock = clock::init(0);
    st.tsc_hz = clock::calibration().hz;
    st.hpet = clock::hpet().is_present();

    // F041/F042/F047 — other cores.
    st.cpus = smp::init();

    // F043/F044/F045/F046 — locking, inheritance, nesting, audit.
    sync::init();

    // F048 — instruction-set readiness.
    st.isa = cpuinfo::init().level_str();

    // F050 — prove the chain before handing control to the next domain.
    st.self_test = cpuinfo::run_interrupt_checks();

    crate::kinfo!(
        "cpu: cores={} vectors={} apic={:?} ioapic={} gsi={} clk={} self-test {}/{}",
        st.cpus,
        st.idt_vectors,
        st.apic,
        st.ioapics,
        st.gsis,
        st.clock.as_str(),
        st.self_test.0,
        st.self_test.0 + st.self_test.1
    );
    st
}

/// Render the domain HUD onto the console (used by the boot sequence).
pub fn render_to_console(st: &CpuDomainState) {
    let mut buf = [0u8; 256];
    let n = st.render(&mut buf);
    if let Some(c) = crate::console::installed_ref() {
        for &b in &buf[..n] {
            c.put_byte(b);
        }
    }
    let mut detail = [0u8; 512];
    let m = cpuinfo::irq_selftest().render(&mut detail);
    if let Some(c) = crate::console::installed_ref() {
        for &b in &detail[..m] {
            c.put_byte(b);
        }
    }
}

/// F037/F049 — the single entry point every ISR runs through. Opening the
/// measurement here and closing it in [`on_irq_exit`] gives an honest
/// end-to-end interrupt service time (entry → acknowledgement).
pub fn on_irq_entry(vector: u8) -> u64 {
    let me = smp::current();
    me.note_irq();
    let depth = me.irq_enter();
    sync::nesting().note_depth(depth);
    let token = cpuinfo::latency().begin();
    me.scratch[0].store(token, core::sync::atomic::Ordering::Relaxed);
    clock::tick();
    if let Some(kind) = smp::handle_ipi(vector) {
        crate::ktrace!("ipi: {}", kind.name());
    }
    token
}

/// Close the measurement opened by [`on_irq_entry`] and drop the nesting depth.
/// Returns the service time in nanoseconds.
pub fn on_irq_exit() -> u64 {
    let me = smp::current();
    let token = me.scratch[0].load(core::sync::atomic::Ordering::Relaxed);
    let ns = cpuinfo::latency().end(token);
    let _ = me.irq_leave();
    ns
}

/// Interrupts stay disabled until the scheduler owns them (AI-04), so this is
/// exposed rather than called here.
pub fn enable_interrupts() {
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    {
        // SAFETY: the IDT is installed by `init` before this is reachable.
        unsafe { core::arch::asm!("sti", options(nomem, nostack, preserves_flags)) };
    }
}

pub fn interrupts_enabled() -> bool {
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    {
        let flags: u64;
        // SAFETY: `pushfq` only reads RFLAGS.
        unsafe {
            core::arch::asm!("pushfq", "pop {}", out(reg) flags, options(nomem, nostack));
        }
        flags & (1 << 9) != 0
    }
    #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
    {
        false
    }
}
