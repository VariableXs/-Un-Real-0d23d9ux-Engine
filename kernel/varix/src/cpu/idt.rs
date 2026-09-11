//! F028 IDT 全量向量 / F029 异常现场转储 / F030 panic 人话解释.
//!
//! Every one of the 256 vectors gets a real slot (no "reserved for later"),
//! every exception dumps a full register frame, and every dump ends with a
//! plain-language sentence — a kernel panic should read like a diagnosis, not
//! a hex dump.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub const IDT_VECTORS: usize = 256;
/// CPU exceptions occupy 0..=31; everything above is device/software.
pub const EXCEPTION_COUNT: usize = 32;
/// Where the remapped PIC (or the IO APIC's first GSIs) start (F031).
pub const IRQ_BASE: u8 = 32;
/// Spurious interrupt vector (F032 LAPIC spurious-vector register).
pub const SPURIOUS_VECTOR: u8 = 0xFF;

/// Gate types used by Varix.
pub const GATE_INTERRUPT: u8 = 0xE;
pub const GATE_TRAP: u8 = 0xF;

/// One IDT slot, kept in decoded form so it can assert on its own fields.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IdtEntry {
    pub offset: u64,
    pub selector: u16,
    pub ist: u8,
    pub gate: u8,
    pub dpl: u8,
    pub present: bool,
}

impl IdtEntry {
    /// A vector with nothing wired to it yet.
    pub const fn missing() -> IdtEntry {
        IdtEntry {
            offset: 0,
            selector: 0,
            ist: 0,
            gate: GATE_INTERRUPT,
            dpl: 0,
            present: false,
        }
    }

    pub const fn attrs(&self) -> u8 {
        let mut a = self.gate & 0xF;
        a |= (self.dpl & 0x3) << 5;
        if self.present {
            a |= 0x80;
        }
        a
    }

    /// Encode to the 16-byte wire format as two `u64`s.
    pub const fn encode(&self) -> [u64; 2] {
        let mut low = self.offset & 0xFFFF;
        low |= (self.selector as u64) << 16;
        low |= ((self.ist as u64) & 0x7) << 32;
        low |= (self.attrs() as u64) << 40;
        low |= ((self.offset >> 16) & 0xFFFF) << 48;
        let high = self.offset >> 32;
        [low, high]
    }

    /// Decode back from the wire format (used by the self-test to prove the
    /// encoder is lossless).
    pub fn decode(low: u64, high: u64) -> IdtEntry {
        let offset = (low & 0xFFFF) | ((low >> 48) & 0xFFFF) << 16 | (high << 32);
        let attrs = (low >> 40) as u8;
        IdtEntry {
            offset,
            selector: (low >> 16) as u16,
            ist: ((low >> 32) as u8) & 0x7,
            gate: attrs & 0xF,
            dpl: (attrs >> 5) & 0x3,
            present: attrs & 0x80 != 0,
        }
    }
}

// ---------------------------------------------------------------------------
// F028 — the table itself
// ---------------------------------------------------------------------------

/// SAFETY: populated on the single-threaded boot path, read-only afterwards.
unsafe impl Sync for Idt {}

pub struct Idt {
    entries: UnsafeCell<[IdtEntry; IDT_VECTORS]>,
    /// Wire-format gates the CPU actually reads through the IDTR. `IdtEntry`
    /// is a plain Rust struct — its memory layout is NOT the x86 gate layout —
    /// so the CPU must never be pointed at `entries` directly (the first
    /// hardware interrupt triple-faulted exactly that way, QEMU 2026-09-12).
    wire: UnsafeCell<[[u64; 2]; IDT_VECTORS]>,
    installed: AtomicBool,
}

impl Idt {
    pub const fn new() -> Idt {
        Idt {
            entries: UnsafeCell::new([IdtEntry::missing(); IDT_VECTORS]),
            wire: UnsafeCell::new([IdtEntry::missing().encode(); IDT_VECTORS]),
            installed: AtomicBool::new(false),
        }
    }

    pub fn installed(&self) -> bool {
        self.installed.load(Ordering::Acquire)
    }

    /// # Safety
    /// Single-threaded bring-up only.
    unsafe fn slot(&self, vec: usize) -> Option<&mut IdtEntry> {
        if vec >= IDT_VECTORS {
            return None;
        }
        Some(&mut (*self.entries.get())[vec])
    }

    /// Wire `vec` to `handler`. `ist = 0` means "use the current stack";
    /// 1..=7 selects an IST slot (F027).
    pub fn set(&self, vec: usize, handler: u64, ist: u8, dpl: u8) -> bool {
        if vec >= IDT_VECTORS || ist > 7 || dpl > 3 {
            return false;
        }
        // SAFETY: see `slot`.
        unsafe {
            if let Some(e) = self.slot(vec) {
                *e = IdtEntry {
                    offset: handler,
                    selector: crate::cpu::gdt::SEL_KERNEL_CODE,
                    ist,
                    gate: GATE_INTERRUPT,
                    dpl,
                    present: true,
                };
                // Mirror into the wire-format gate the CPU dispatches through.
                (*self.wire.get())[vec] = e.encode();
                true
            } else {
                false
            }
        }
    }

    pub fn entry(&self, vec: usize) -> Option<IdtEntry> {
        if vec >= IDT_VECTORS {
            return None;
        }
        // SAFETY: read-only view of a boot-time table.
        Some(unsafe { (*self.entries.get())[vec] })
    }

    /// How many of the 256 vectors currently have a handler.
    pub fn populated(&self) -> usize {
        (0..IDT_VECTORS)
            .filter(|v| self.entry(*v).map(|e| e.present).unwrap_or(false))
            .count()
    }

    /// LIDT pseudo-descriptor — points at the wire-format gate table.
    pub fn descriptor(&self) -> (u16, u64) {
        let base = self.wire.get() as u64;
        ((IDT_VECTORS * 16 - 1) as u16, base)
    }

    /// SAFETY: `entries` must stay alive for as long as the CPU uses it.
    pub unsafe fn install(&self) {
        let (limit, base) = self.descriptor();
        load_idt(limit, base);
        self.installed.store(true, Ordering::Release);
    }
}

impl Default for Idt {
    fn default() -> Idt {
        Idt::new()
    }
}

static IDT: Idt = Idt::new();

pub fn table() -> &'static Idt {
    &IDT
}

// ---------------------------------------------------------------------------
// F029 — exception frame + dump
// ---------------------------------------------------------------------------

/// Push order produced by the assembly stubs (matches the `TrapFrame` layout).
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct TrapFrame {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
    pub vector: u64,
    pub error_code: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// Append `0x` + 16 hex digits of `v`.
fn write_hex(out: &mut [u8], n: &mut usize, v: u64) {
    let mut put = |b: u8| {
        if *n < out.len() {
            out[*n] = b;
            *n += 1;
        }
    };
    put(b'0');
    put(b'x');
    let mut shift = 60i32;
    while shift >= 0 {
        put(HEX[((v >> shift) as usize) & 0xF]);
        shift -= 4;
    }
}

fn write_str(out: &mut [u8], n: &mut usize, s: &str) {
    for &b in s.as_bytes() {
        if *n < out.len() {
            out[*n] = b;
            *n += 1;
        }
    }
}

fn write_reg(out: &mut [u8], n: &mut usize, name: &str, v: u64) {
    write_str(out, n, name);
    write_str(out, n, "=");
    write_hex(out, n, v);
    write_str(out, n, "\n");
}

/// Render the whole frame, followed by the human sentence from F030.
/// Returns the number of bytes written (never overflows `out`).
pub fn dump_frame(f: &TrapFrame, out: &mut [u8]) -> usize {
    let mut n = 0usize;
    write_str(out, &mut n, "EXCEPTION ");
    write_hex(out, &mut n, f.vector);
    write_str(out, &mut n, " ");
    write_str(out, &mut n, exception_name(f.vector));
    write_str(out, &mut n, "\n");
    write_reg(out, &mut n, "RIP", f.rip);
    write_reg(out, &mut n, "RSP", f.rsp);
    write_reg(out, &mut n, "RFLAGS", f.rflags);
    write_reg(out, &mut n, "RAX", f.rax);
    write_reg(out, &mut n, "RBX", f.rbx);
    write_reg(out, &mut n, "RCX", f.rcx);
    write_reg(out, &mut n, "RDX", f.rdx);
    write_reg(out, &mut n, "RSI", f.rsi);
    write_reg(out, &mut n, "RDI", f.rdi);
    write_reg(out, &mut n, "RBP", f.rbp);
    write_str(out, &mut n, "ERR=");
    write_hex(out, &mut n, f.error_code);
    write_str(out, &mut n, " CS=");
    write_hex(out, &mut n, f.cs);
    write_str(out, &mut n, "\n");
    n += humanize_into(f.vector, f.error_code, &mut out[n..]);
    n
}

// ---------------------------------------------------------------------------
// F030 — panic 人话解释
// ---------------------------------------------------------------------------

/// Short architectural name.
pub fn exception_name(vector: u64) -> &'static str {
    match vector {
        0 => "DE  #Divide-Error",
        1 => "DB  #Debug",
        2 => "    NMI",
        3 => "BP  #Breakpoint",
        4 => "OF  #Overflow",
        5 => "BR  #BOUND-Range",
        6 => "UD  #Invalid-Opcode",
        7 => "NM  #Device-Not-Available",
        8 => "DF  #Double-Fault",
        9 => "    Coprocessor-Segment-Overrun",
        10 => "TS  #Invalid-TSS",
        11 => "NP  #Segment-Not-Present",
        12 => "SS  #Stack-Segment-Fault",
        13 => "GP  #General-Protection",
        14 => "PF  #Page-Fault",
        15 => "    Reserved",
        16 => "MF  #x87-FPU-Error",
        17 => "AC  #Alignment-Check",
        18 => "MC  #Machine-Check",
        19 => "XM  #SIMD-FP-Exception",
        20 => "VE  #Virtualization-Exception",
        21 => "CP  #Control-Protection",
        28 => "HV  #Hypervisor-Injection",
        29 => "VC  #VMM-Communication",
        30 => "SX  #Security-Exception",
        _ => "    External/Unknown",
    }
}

/// Plain-language cause, in the operator's language.
pub fn explain(vector: u64, error_code: u64) -> &'static str {
    match vector {
        0 => "除以零：代码用 0 做了除数，或除法结果放不下。",
        1 => "调试断点：调试寄存器条件命中。",
        2 => "不可屏蔽中断：硬件报错，优先看温度与内存。",
        3 => "断点指令 int3：调试器或断言触发。",
        4 => "溢出：into 指令检测到 OF 置位。",
        5 => "越界：BOUND 指令检查失败。",
        6 => "非法指令：CPU 不认识这条指令，常见于内核/用户态特性不匹配。",
        7 => "协处理器不可用：任务切换后未恢复 FPU/SSE 状态。",
        8 => "双重错误：处理异常时又发生异常，栈或页表多半已损坏。",
        10 => "TSS 非法：任务状态段的段选择子或类型不对。",
        11 => "段不存在：引用了一个未加载的段。",
        12 => "栈段错误：栈越界或栈段权限不足。",
        13 => "通用保护：权限违规、空段、或写只读段。",
        14 => page_fault_plain(error_code),
        16 => "浮点错误：x87 运算异常未屏蔽。",
        17 => "对齐检查：未对齐的内存访问。",
        18 => "机器检查：硬件报告不可纠正错误。",
        19 => "SIMD 浮点异常：SSE/AVX 运算异常未屏蔽。",
        21 => "控制流保护失败：返回地址或影子栈不匹配（可能是攻击）。",
        _ => "未知异常：需要结合现场寄存器分析。",
    }
}

/// F030 core: decode the page-fault error code into words.
pub fn page_fault_plain(error_code: u64) -> &'static str {
    let present = error_code & 0x1 != 0; // P
    let write = error_code & 0x2 != 0; // W/R
    let user = error_code & 0x4 != 0; // U/S
    let exec = error_code & 0x10 != 0; // I/D (needs NX/EFER)
    match (present, write, user, exec) {
        (false, _, _, true) => "取指缺页：跳到了一段没有执行权限或根本没映射的内存。",
        (false, true, _, false) => "写缺页：地址没映射，通常是空指针或已释放的对象。",
        (false, false, _, false) => "读缺页：地址没映射，通常是空指针或越界下标。",
        (true, true, true, _) => "权限缺页：用户态写了内核页（越权）。",
        (true, true, false, _) => "权限缺页：内核写了只读页（可能是 CoW 页未复制）。",
        (true, false, true, _) => "权限缺页：用户态读了内核页（越权）。",
        (true, false, false, _) => "权限缺页：内核读了不可执行/保留位异常页。",
    }
}

/// Page-fault error code field names, for the HUD.
pub fn page_fault_flags(error_code: u64) -> &'static str {
    const P: u64 = 0x1;
    const W: u64 = 0x2;
    const U: u64 = 0x4;
    const RSVD: u64 = 0x8;
    const I: u64 = 0x10;
    const PK: u64 = 0x20;
    const SS: u64 = 0x40;
    const SGX: u64 = 0x8000;
    match (
        error_code & P,
        error_code & W,
        error_code & U,
        error_code & RSVD,
        error_code & I,
        error_code & PK,
        error_code & SS,
        error_code & SGX,
    ) {
        (_, _, _, 0, 0, 0, 0, 0) => "P/W/U",
        (_, _, _, 0, I, 0, 0, 0) => "P/W/U/EXEC",
        (_, _, _, 0, 0, PK, 0, 0) => "P/W/U/PKEY",
        (_, _, _, 0, 0, 0, SS, 0) => "P/W/U/SS",
        (_, _, _, 0, 0, 0, 0, SGX) => "P/W/U/SGX",
        _ => "P/W/U/RSVD",
    }
}

/// Append the human sentence (`崩溃原因：...`) into `out`; returns bytes used.
pub fn humanize_into(vector: u64, error_code: u64, out: &mut [u8]) -> usize {
    let mut n = 0usize;
    write_str(out, &mut n, "崩溃原因：");
    write_str(out, &mut n, explain(vector, error_code));
    write_str(out, &mut n, "\n");
    n
}

/// Owned-buffer convenience wrapper around [`humanize_into`].
pub fn humanize(vector: u64, error_code: u64) -> [u8; 128] {
    let mut buf = [0u8; 128];
    let _ = humanize_into(vector, error_code, &mut buf);
    buf
}

// ---------------------------------------------------------------------------
// Bring-up (F028)
// ---------------------------------------------------------------------------

/// Fill all 256 vectors: 32 exceptions (with IST where it matters), then the
/// device range, then the spurious slot. Handlers come from the stub table in
/// `cpu::entry`, but the table is valid even when no handler exists yet.
pub fn init() -> usize {
    let idt = table();
    // Every vector gets real executable code, not a placeholder.
    let base = build_stubs(common_entry_addr());
    crate::kinfo!("idt-stubs: 256 x {}B at {:#x}", STUB_SIZE, base);
    for v in 0..EXCEPTION_COUNT {
        let ist = match v as u64 {
            2 => crate::cpu::gdt::IST_NMI as u8,
            8 => crate::cpu::gdt::IST_DOUBLE_FAULT as u8,
            18 => crate::cpu::gdt::IST_MACHINE_CHECK as u8,
            _ => 0,
        };
        // Vectors 3 (int3) and 0x80 stay reachable from ring 3.
        let dpl = if v == 3 { 3 } else { 0 };
        let handler = stub_for(v as u8);
        idt.set(v, handler, ist, dpl);
    }
    for v in EXCEPTION_COUNT..IDT_VECTORS {
        idt.set(v, stub_for(v as u8), 0, 0);
    }
    idt.set(SPURIOUS_VECTOR as usize, stub_for(SPURIOUS_VECTOR), 0, 0);
    // SAFETY: `IDT` is a `static` and never moves.
    unsafe { idt.install() };
    let n = idt.populated();
    crate::kinfo!("idt: {}/{} vectors live", n, IDT_VECTORS);
    n
}

// ---------------------------------------------------------------------------
// Per-vector entry stubs
// ---------------------------------------------------------------------------

/// Bytes of one entry stub.
pub const STUB_SIZE: usize = 16;

/// Encode one entry stub:
/// ```text
/// 6A xx          push imm8          (vector number, sign-extended to 64 bits)
/// E9 rel32       jmp  rel32         (to the common Rust dispatcher)
/// CC …           int3 padding       (so a runaway PC traps instead of flowing)
/// ```
pub fn encode_stub(vector: u8, rel32: i32) -> [u8; STUB_SIZE] {
    let mut b = [0xCCu8; STUB_SIZE];
    b[0] = 0x6A;
    b[1] = vector;
    b[2] = 0xE9;
    let r = rel32.to_le_bytes();
    b[3..7].copy_from_slice(&r);
    b
}

/// `rel32` from a stub slot to `target`: relative to the *next* instruction
/// (slot + 7), which is where the CPU resumes after `jmp`.
pub fn stub_rel32(slot_addr: u64, target: u64) -> i32 {
    let delta = target as i64 - (slot_addr as i64 + 7);
    if delta > i32::MAX as i64 {
        i32::MAX
    } else if delta < i32::MIN as i64 {
        i32::MIN
    } else {
        delta as i32
    }
}

/// x86_64 exceptions that push an error code; it shifts RIP in the frame.
fn has_error_code(vector: u8) -> bool {
    matches!(vector, 8 | 10 | 11 | 12 | 13 | 14 | 17 | 30)
}

/// CR2 — the address that caused the last page fault.
fn fault_addr() -> u64 {
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    {
        let cr2: u64;
        // SAFETY: reading a control register has no memory effect.
        unsafe {
            core::arch::asm!("mov {}, cr2", out(reg) cr2, options(nomem, nostack, preserves_flags))
        };
        cr2
    }
    #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
    {
        0
    }
}

/// The single Rust entry every stub reaches. `vector` is the number the stub
/// pushed; `rsp` points at it (the CPU's own frame sits just above).
///
/// CPU exceptions are fatal and end in a diagnosis. Device interrupts go
/// through the CPU domain's entry/exit pair, get acknowledged, and return —
/// the `iretq` is done by the naked wrapper below.
extern "C" fn isr_dispatch(vector: u64, rsp: u64) {
    if vector < EXCEPTION_COUNT as u64 {
        // `rsp` is the frame base: [rsp+8] is the vector the stub pushed, then
        // the CPU's own frame (error code if the exception has one, then RIP).
        let (code, rip) = unsafe {
            if has_error_code(vector as u8) {
                (
                    core::ptr::read((rsp + 16) as *const u64),
                    core::ptr::read((rsp + 24) as *const u64),
                )
            } else {
                (0u64, core::ptr::read((rsp + 16) as *const u64))
            }
        };
        let cr2: u64 = fault_addr();
        crate::kerror!(
            "fatal exception {} err={:#x} rip={:#x} cr2={:#x} — halting",
            vector,
            code,
            rip,
            cr2
        );
        let frame = TrapFrame {
            vector,
            rsp,
            ..TrapFrame::default()
        };
        let mut buf = [0u8; 768];
        let n = dump_frame(&frame, &mut buf);
        if let Some(c) = crate::console::installed_ref() {
            for &b in &buf[..n] {
                c.put_byte(b);
            }
        }
        crate::kerror!("fatal exception {} — halting", vector);
        halt_forever();
    }

    // Device / IPI / spurious path.
    let _ = crate::cpu::on_irq_entry(vector as u8);
    if vector >= IRQ_BASE as u64 && vector != SPURIOUS_VECTOR as u64 {
        crate::cpu::apic::eoi_audit().begin(vector as u8);
        if !crate::cpu::apic::lapic().eoi(vector as u8) {
            crate::kwarn!("isr: vector {} has no EOI discipline", vector);
        }
    }
    let _ = crate::cpu::on_irq_exit();
}

#[cfg(all(target_arch = "x86_64", target_os = "none"))]
fn halt_forever() -> ! {
    // SAFETY: park the core for good.
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack));
        loop {
            core::arch::asm!("hlt", options(nomem, nostack, preserves_flags));
        }
    }
}

#[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
fn halt_forever() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

/// The common dispatcher the stubs jump to. Naked because the entry state is
/// exactly the stub's stack (`[rsp]` = vector) and nothing else is known yet.
///
/// `rbp` is used as the scratch base: it is callee-saved, so the Rust callee
/// cannot clobber it, which lets us restore the exact interrupt stack before
/// `iretq`. The `and rsp, -16` realigns for the ABI without losing that base.
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
#[unsafe(naked)]
unsafe extern "C" fn common_entry() {
    core::arch::naked_asm!(
        "push rbp",
        "mov rbp, rsp",
        // The Rust dispatcher follows the C calling convention, so every
        // caller-saved register it touches would otherwise be handed back to
        // the interrupted code already overwritten. A timer tick landing
        // inside `core::fmt::write` used to come back with a dead writer
        // pointer and fault immediately, so save the whole volatile set.
        "push rax",
        "push rcx",
        "push rdx",
        "push rsi",
        "push rdi",
        "push r8",
        "push r9",
        "push r10",
        "push r11",
        "mov rdi, [rbp + 8]",      // vector pushed by the stub
        "mov rsi, rbp",            // frame pointer
        "and rsp, -16",            // ABI stack alignment for the call
        "call {entry}",
        "mov rsp, rbp",
        "sub rsp, 72",             // rewind to the last saved register (9 × 8)
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rdi",
        "pop rsi",
        "pop rdx",
        "pop rcx",
        "pop rax",
        "pop rbp",
        "add rsp, 8",              // drop the vector the stub pushed
        "iretq",
        entry = sym isr_dispatch,
    )
}

/// The stub table: 256 × 16 bytes of real machine code, built at boot.
struct StubTable {
    bytes: UnsafeCell<[u8; STUB_SIZE * IDT_VECTORS]>,
    base: AtomicU64,
}

// SAFETY: filled once during boot before the IDT is installed; read-only after.
unsafe impl Sync for StubTable {}

#[cfg_attr(target_os = "none", link_section = ".stubs")]
static STUBS: StubTable = StubTable {
    bytes: UnsafeCell::new([0xCCu8; STUB_SIZE * IDT_VECTORS]),
    base: AtomicU64::new(0),
};

/// Outcome of hardening the stub page (see [`harden_stub_mapping`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StubMapping {
    /// NX and the write bit both cleared: the table is now read-execute.
    Tightened,
    /// NX cleared, but the leaf is a huge page shared with writable kernel
    /// data, so the write bit had to stay (the table is still executable).
    Executable,
    /// Nothing could be walked — host build, or the page is not mapped.
    Skipped,
}

/// The entry stubs are *generated* at boot, so they have to live in a writable
/// page, and Limine marks every writable page NX. Once the bytes are in place
/// there is no reason to keep the page writable, so this walks the live page
/// tables (CR3 + HHDM) and turns the range into read-execute before the IDT
/// can deliver anything. Without it the first interrupt faults with
/// `e=0x11` (instruction fetch from a non-executable page).
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
unsafe fn harden_stub_mapping(base: u64, len: usize) -> StubMapping {
    let hhdm = match crate::limine::hhdm_offset() {
        Some(h) => h,
        None => return StubMapping::Skipped,
    };
    let cr3: u64;
    core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags));
    let root = (cr3 & crate::mem::paging::P_ADDR_MASK) + hhdm;

    let mut page = base & !0xFFFu64;
    let end = base + len as u64;
    let mut pages = 0usize;
    let mut tightened = true;
    while page < end {
        let (ptr, huge) = match leaf_entry(root, page, hhdm) {
            Some(v) => v,
            None => return StubMapping::Skipped,
        };
        let entry = core::ptr::read(ptr);
        let mut next = entry & !crate::mem::paging::P_NX;
        if huge {
            // Shared with ordinary writable kernel data: only drop NX.
            tightened = false;
        } else {
            next &= !crate::mem::paging::P_WRITE;
        }
        core::ptr::write(ptr, next);
        core::arch::asm!("invlpg [{}]", in(reg) page, options(nostack, preserves_flags));
        pages += 1;
        page += 0x1000;
    }
    if pages == 0 {
        StubMapping::Skipped
    } else if tightened {
        StubMapping::Tightened
    } else {
        StubMapping::Executable
    }
}

#[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
unsafe fn harden_stub_mapping(_base: u64, _len: usize) -> StubMapping {
    StubMapping::Skipped
}

/// Walk PML4→PDPT→PD→PT for `virt`; returns the leaf entry address and whether
/// the walk stopped early on a huge page.
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
unsafe fn leaf_entry(root: u64, virt: u64, hhdm: u64) -> Option<(*mut u64, bool)> {
    const P_PRESENT: u64 = 1 << 0;
    let idx = |shift: u32| ((virt >> shift) & 0x1FF) as usize;
    let mut table = root as *mut u64;
    for shift in [39u32, 30, 21] {
        let slot = table.add(idx(shift));
        let entry = core::ptr::read(slot);
        if entry & P_PRESENT == 0 {
            return None;
        }
        if entry & crate::mem::paging::P_HUGE != 0 {
            return Some((slot, true));
        }
        table = ((entry & crate::mem::paging::P_ADDR_MASK) + hhdm) as *mut u64;
    }
    Some((table.add(idx(12)), false))
}

/// Build the stub table and return its base address.
pub fn build_stubs(common_entry_addr: u64) -> u64 {
    let bytes = STUBS.bytes.get();
    // SAFETY: `bytes` is a live `UnsafeCell` inside a `static`; the table is
    // never moved and this is the only writer.
    let base = unsafe { (*bytes).as_ptr() as u64 };
    for v in 0..IDT_VECTORS {
        let slot = base + (v * STUB_SIZE) as u64;
        let code = encode_stub(v as u8, stub_rel32(slot, common_entry_addr));
        // SAFETY: each slot is written exactly once, before the IDT exists.
        unsafe {
            let dst = (*bytes).as_mut_ptr().add(v * STUB_SIZE);
            core::ptr::copy_nonoverlapping(code.as_ptr(), dst, STUB_SIZE);
        }
    }
    // Publish before the mapping loses its write permission.
    STUBS.base.store(base, Ordering::Release);
    // SAFETY: generation is finished; nothing writes to the table again, so it
    // is safe to turn it into read-execute code.
    let mapping = unsafe { harden_stub_mapping(base, STUB_SIZE * IDT_VECTORS) };
    if mapping != StubMapping::Skipped {
        crate::kinfo!("idt-stubs: mapping {:?}", mapping);
    }
    base
}

/// Address of the entry stub for `vec`. `None` before `build_stubs` ran.
pub fn stub_addr(vec: u8) -> Option<u64> {
    let base = STUBS.base.load(Ordering::Acquire);
    if base == 0 {
        None
    } else {
        Some(base + vec as u64 * STUB_SIZE as u64)
    }
}

/// Address of the common dispatcher (kernel target) or a host placeholder.
pub fn common_entry_addr() -> u64 {
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    {
        common_entry as *const () as u64
    }
    #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
    {
        // On the host the naked wrapper is not compiled, but the dispatcher
        // itself is real code: point the stubs at it so the encoder, the table
        // and the dispatcher all stay live and testable.
        isr_dispatch as *const () as u64
    }
}

/// Address the IDT should point at for `vec`.
pub fn stub_for(vec: u8) -> u64 {
    if let Some(a) = stub_addr(vec) {
        return a;
    }
    // Host/test path: the table is data, so expose a deterministic placeholder.
    stub_addr_without_build(vec)
}

fn stub_addr_without_build(vec: u8) -> u64 {
    STUBS.bytes.get() as u64 + vec as u64 * STUB_SIZE as u64
}

#[cfg(all(target_arch = "x86_64", target_os = "none"))]
unsafe fn load_idt(limit: u16, base: u64) {
    let ptr: [u16; 5] = [
        limit,
        base as u16,
        (base >> 16) as u16,
        (base >> 32) as u16,
        (base >> 48) as u16,
    ];
    core::arch::asm!("lidt [{0}]", in(reg) ptr.as_ptr(), options(readonly, nostack, preserves_flags));
}

#[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
unsafe fn load_idt(_limit: u16, _base: u64) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_is_lossless() {
        let e = IdtEntry {
            offset: 0xFFFF_8020_ABCD_1234,
            selector: 0x08,
            ist: 3,
            gate: GATE_INTERRUPT,
            dpl: 0,
            present: true,
        };
        let [lo, hi] = e.encode();
        let back = IdtEntry::decode(lo, hi);
        assert_eq!(back, e);
        assert_eq!(back.attrs(), 0x8E);
    }

    #[test]
    fn dpl_and_present_bits_encode() {
        let mut e = IdtEntry::missing();
        e.dpl = 3;
        e.present = true;
        e.gate = GATE_TRAP;
        assert_eq!(e.attrs(), 0xEF);
    }

    #[test]
    fn every_exception_has_a_name_and_a_plain_explanation() {
        for v in 0..EXCEPTION_COUNT as u64 {
            let name = exception_name(v);
            assert!(!name.is_empty());
            let plain = explain(v, 0);
            assert!(!plain.is_empty());
            // Reserved vector 15 has no mnemonic but still explains itself.
            if v != 15 {
                assert!(!name.starts_with("    Reserved"), "v={v}");
            }
        }
    }

    #[test]
    fn page_fault_explanations_are_distinct() {
        // not-present read / write / exec
        assert!(explain(14, 0x0).contains("读缺页"));
        assert!(explain(14, 0x2).contains("写缺页"));
        assert!(explain(14, 0x10).contains("取指缺页"));
        // protection violations
        assert!(explain(14, 0x7).contains("越权"));
        assert!(explain(14, 0x3).contains("只读页"));
        assert_eq!(page_fault_flags(0x10), "P/W/U/EXEC");
        assert_eq!(page_fault_flags(0x8), "P/W/U/RSVD");
    }

    #[test]
    fn humanize_fits_and_starts_with_the_cause() {
        let buf = humanize(14, 0x2);
        let s = core::str::from_utf8(&buf).unwrap();
        let s = s.trim_end_matches('\0');
        assert!(s.starts_with("崩溃原因："), "got {s}");
        assert!(s.ends_with('\n'));
    }

    #[test]
    fn table_populates_and_rejects_bad_vectors() {
        let idt = Idt::new();
        assert_eq!(idt.populated(), 0);
        assert!(idt.set(0, 0x1234, 0, 0));
        assert!(idt.set(255, 0x1234, 0, 0));
        assert!(!idt.set(256, 0x1234, 0, 0), "vector out of range");
        assert!(!idt.set(1, 0x1234, 8, 0), "IST out of range");
        assert!(!idt.set(1, 0x1234, 0, 4), "DPL out of range");
        assert_eq!(idt.populated(), 2);
        assert!(idt.entry(0).unwrap().present);
        assert!(idt.entry(3).is_some());
        assert!(idt.entry(256).is_none());
    }

    #[test]
    fn dump_frame_writes_registers_and_cause() {
        let f = TrapFrame {
            rip: 0xFFFF_8000_0000_1234,
            rsp: 0xFFFF_8000_0000_2000,
            vector: 14,
            error_code: 0x2,
            ..TrapFrame::default()
        };
        let mut out = [0u8; 1024];
        let n = dump_frame(&f, &mut out);
        let s = core::str::from_utf8(&out[..n]).unwrap();
        assert!(s.contains("EXCEPTION 0x000000000000000E"), "{s}");
        assert!(s.contains("RIP=0xFFFF800000001234"), "{s}");
        assert!(s.contains("PF  #Page-Fault"), "{s}");
        assert!(s.contains("写缺页"), "{s}");
    }

    #[test]
    fn stub_encoder_emits_push_then_jump() {
        let code = encode_stub(0x42, -8);
        assert_eq!(code[0], 0x6A, "push imm8");
        assert_eq!(code[1], 0x42, "vector number");
        assert_eq!(code[2], 0xE9, "jmp rel32");
        assert_eq!(
            i32::from_le_bytes([code[3], code[4], code[5], code[6]]),
            -8
        );
        assert_eq!(&code[7..], &[0xCCu8; 9], "padding traps");
    }

    #[test]
    fn stub_rel32_is_relative_to_the_next_instruction() {
        let slot = 0x1000u64;
        assert_eq!(stub_rel32(slot, slot + 7 + 16), 16);
        assert_eq!(stub_rel32(slot, slot), -7);
        // Saturates instead of wrapping on an impossible distance.
        assert_eq!(stub_rel32(0, i32::MAX as u64 + 4096), i32::MAX);
        assert_eq!(stub_rel32(i32::MAX as u64 + 8, 0), i32::MIN);
    }

    #[test]
    fn stub_table_covers_every_vector() {
        let base = build_stubs(0xFFFF_8000_1234_0000);
        assert_ne!(base, 0);
        for v in [0u8, 1, 13, 32, 0xF0, 255] {
            let addr = stub_addr(v).unwrap();
            assert_eq!(addr, base + v as u64 * STUB_SIZE as u64);
            assert_eq!(stub_for(v), addr);
        }
    }

    #[test]
    fn error_code_shift_only_applies_to_the_documented_traps() {
        // #PF/#GP/… push a code, so RIP sits 8 bytes higher in the frame.
        for v in [8u8, 10, 11, 12, 13, 14, 17, 30] {
            assert!(has_error_code(v), "vector {v} must report an error code");
        }
        for v in [0u8, 1, 3, 6, 7, 16, 18, 19, 32, 255] {
            assert!(!has_error_code(v), "vector {v} must not report one");
        }
    }

    #[test]
    fn stub_mapping_is_a_no_op_off_target() {
        // There are no Limine page tables to walk on the host.
        // SAFETY: no real mapping is touched; the host build returns early.
        let state = unsafe { harden_stub_mapping(0x1000, STUB_SIZE * IDT_VECTORS) };
        #[cfg(all(target_arch = "x86_64", target_os = "none"))]
        assert_ne!(state, StubMapping::Tightened);
        #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
        assert_eq!(state, StubMapping::Skipped);
    }

    #[test]
    fn dump_respects_a_tiny_buffer() {
        let f = TrapFrame::default();
        let mut out = [0u8; 16];
        let n = dump_frame(&f, &mut out);
        assert_eq!(n, 16);
    }
}
