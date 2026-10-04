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
    // RAW APIC id——send_ipi 按模式自行格式化（x2APIC 传预移位值会双重
    // 移位成 0x1000000 无人匹配，IPI 静默丢失，2026-09-19 实机根因）。
    let dest = target.dest();
    crate::cpu::apic::lapic().send_ipi(dest, low);
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

// 2026-09-19 实机戒律：此前「trampoline stub」从未存在——SIPI 把 AP 发到物理
// 0x8000 执行内存垃圾（QEMU 低内存是零页 → AP 静默死、单核 fail-open 假装全通；
// 真机 Lenovo UEFI 残留在低内存 → AP 执行固件垃圾乱写内存拖死整机）。
// 本 stub：16 位实模式 → 32 位保护模式 → 64 位长模式，全部取数在开分页前完成
// （内核页表无低区恒等映射——由 install_trampoline 注入「单页恒等映射」补上），
// 寄存器跨模式传递 stack/entry。
//
// 2026-09-19 跳板走查修复（stage=0 根因链，8 处全修）：
//  ① 16 位段 DS=CS=0x0800 已自带 0x8000 线性基址——数据偏移必须用 image 内
//     偏移（TR_DEBUG_RM），带 0x8000 前缀的 TR_*_OFF 只给 32/64 位平铺段用
//     （此前 stage=1 写到物理 0x10000，BSP 在 0x8000+off 永远读到 0）；
//  ② far jump 的 EIP 操作数是「平铺线性地址」：CS 描述符 base=0，必须
//     0x8000+off，裸 off 会取指到中断向量表垃圾；
//  ③ 32→64 far jump 选择子必须 0x18（code64），0x08 是 32 位代码段——
//     LME+PG 开启后跳进去是兼容模式，r15d 等全部乱套。
core::arch::global_asm!(
    ".globl ap_trampoline_start",
    ".globl ap_trampoline_end",
    ".globl tr_gdt_label",
    ".globl tr_gdt_base_slot",
    ".globl tr_cr3_slot",
    ".globl tr_stack_slot",
    ".globl tr_entry_slot",
    ".globl tr_debug_slot",
    ".globl tr_idt_desc",
    ".globl tr_idt_base_slot",
    "ap_trampoline_start:",
    ".set TR_CR3_OFF, 0x00008000 + (tr_cr3_slot - ap_trampoline_start)",
    ".set TR_STACK_OFF, 0x00008000 + (tr_stack_slot - ap_trampoline_start)",
    ".set TR_ENTRY_OFF, 0x00008000 + (tr_entry_slot - ap_trampoline_start)",
    ".set TR_DEBUG_OFF, 0x00008000 + (tr_debug_slot - ap_trampoline_start)",
    ".set TR_DEBUG_RM, tr_debug_slot - ap_trampoline_start",
    ".set TR_GDT_DESC_OFF, tr_gdt_desc - ap_trampoline_start",
    ".set TR_IDT_DESC_OFF, 0x00008000 + (tr_idt_desc - ap_trampoline_start)",
    ".set PM32_OFF, pm32_entry - ap_trampoline_start",
    ".set PM64_OFF, pm64_entry - ap_trampoline_start",
    ".code16",
    "    cli",
    "    cld",
    "    mov ax, cs",
    "    mov ds, ax",
    "    mov es, ax",
    "    mov ss, ax",
    "    mov sp, 0xFE00",
    "    mov word ptr ds:[TR_DEBUG_RM], 1",
    "    lgdt [TR_GDT_DESC_OFF]",
    "    mov eax, cr0",
    "    or eax, 1",
    "    mov cr0, eax",
    "    .byte 0xEA",
    "    .word 0x00008000 + PM32_OFF",
    "    .word 0x0008",
    ".code32",
    "pm32_entry:",
    "    mov ax, 0x10",
    "    mov ds, ax",
    "    mov es, ax",
    "    mov ss, ax",
    "    mov fs, ax",
    "    mov gs, ax",
    "    mov esp, 0x00017E00",
    "    mov dword ptr ds:[TR_DEBUG_OFF], 2",
    "    mov eax, dword ptr ds:[TR_CR3_OFF]",
    "    mov ebx, dword ptr ds:[TR_STACK_OFF]",
    "    mov ebp, dword ptr ds:[TR_STACK_OFF + 4]",
    "    mov esi, dword ptr ds:[TR_ENTRY_OFF]",
    "    mov edi, dword ptr ds:[TR_ENTRY_OFF + 4]",
    "    mov cr3, eax",
    "    mov eax, cr4",
    "    or eax, 0x20",
    "    mov cr4, eax",
    "    mov ecx, 0xC0000080",
    "    rdmsr",
    // LME|NXE 一起置位。戒律（2026-09-19 QEMU 取证）：Limine 建的内核页表
    // 给数据段（含 AP 栈 .bss）打 NX——实测 AP 栈页 PTE=0x8000000018f9f003
    // 带 bit63；BSP 由 Limine 启动 EFER=0xD00（NXE=1）所以没事，而 AP 复位后
    // EFER=0，这里若只置 LME，AP 进内核第一条 push rbp 写 NX 栈页即被当成
    // 保留位违例 → RSVD #PF（err=0xa）→ 嵌套 #DF → 三重故障死循环。
    "    or eax, 0x900",
    "    wrmsr",
    "    mov eax, cr0",
    "    or eax, 0x80000000",
    "    mov cr0, eax",
    "    .byte 0xEA",
    "    .long 0x00008000 + PM64_OFF",
    "    .word 0x0018",
    ".code64",
    "pm64_entry:",
    "    mov ax, 0x10",
    "    mov ds, ax",
    "    mov es, ax",
    "    mov ss, ax",
    "    mov fs, ax",
    "    mov gs, ax",
    // 调试痕迹必须用显式内存操作数（[disp32] 绝对寻址，链接期解析）。
    // 戒律：`mov r15d, TR_DEBUG_OFF` 这类「裸符号当寄存器源」会被 GAS
    // 汇编成内存加载 mov r15d,[0x8118]——r15 装进的是槽里的值而非地址，
    // 随后 [r15] 写直接打到 VA 2 页故障（QEMU 监视器取证实锤，
    // stage 永远停在 2）。
    "    mov dword ptr ds:[TR_DEBUG_OFF], 3",
    "    mov rsp, rbp",
    "    shl rsp, 32",
    "    or rsp, rbx",
    "    mov rax, rdi",
    "    shl rax, 32",
    "    or rax, rsi",
    // 内核 IDT 必须在 jmp rax 前就位：入口一旦异常，#PF 要落到真处理器
    // 而不是 IDT=0 的垃圾门死循环（那会顺带执行垃圾代码破坏内核静态——
    // 2026-09-19 QEMU 取证：BSP 的 TSC 校准静态被污染、延迟膨胀 1000 倍）。
    "    lidt [TR_IDT_DESC_OFF]",
    "    mov dword ptr ds:[TR_DEBUG_OFF], 4",
    "    jmp rax",
    "    .align 8",
    "tr_gdt_desc:",
    "    .word 8*4 - 1",
    "tr_gdt_base_slot:",
    "    .long 0",
    "    .align 8",
    "tr_gdt_label:",
    "    .quad 0",                       // null
    "    .quad 0x00CF9A000000FFFF",     // code32: base 0, limit 4G, 9A/CF
    "    .quad 0x00CF92000000FFFF",     // data:   base 0, limit 4G, 92/CF
    "    .quad 0x00209A0000000000",     // code64: L=1
    "tr_idt_desc:",
    "    .word 0xfff",
    "tr_idt_base_slot:",
    "    .quad 0",
    "tr_cr3_slot:",
    "    .long 0",
    "    .align 8",
    "tr_stack_slot:",
    "    .quad 0",
    "tr_entry_slot:",
    "    .quad 0",
    "tr_debug_slot:",
    "    .long 0",
    "ap_trampoline_end:",
);

extern "C" {
    static ap_trampoline_start: u8;
    static ap_trampoline_end: u8;
    static tr_gdt_label: u8;
    static tr_gdt_base_slot: u8;
    static tr_idt_base_slot: u8;
    static tr_cr3_slot: u8;
    static tr_stack_slot: u8;
    static tr_entry_slot: u8;
    static tr_debug_slot: u8;
}

/// 恒等映射用的页表结构物理地址（常规内存空闲区，0x9FC00 以下、跳板页之后；
/// 固定低地址省去内核 VA→PA 换算，QEMU 与真机均为可用 RAM）。
const ID_PDPT_PA: u64 = 0xA000;
const ID_PD_PA: u64 = 0xB000;
const ID_PT_PA: u64 = 0xC000;
const PTE_PRESENT_RW: u64 = 0x03;

/// 把跳板复制到低物理页 0x8000 并打上固定参数（GDT base / CR3 / 入口）。
/// 每个 AP 的栈不同，`patch_ap_stack` 在各自 send_ipi 前单独打。
fn install_trampoline(cr3: u64, entry: u64) -> Result<(), &'static str> {
    // CR3 低 12 位是 PCID/标志位，不是物理帧——不掩掉整个页表基址就是错的。
    let cr3 = cr3 & 0x0000_ffff_ffff_f000;
    if cr3 > 0x000F_FFFF_F000 {
        // 跳板 32 位阶段用 u32 装载 CR3；Limine 常规布局页表都在 4G 内。
        return Err("ap cr3 above 4G");
    }
    let hhdm = crate::limine::hhdm_offset().ok_or("no hhdm mapping")?;
    let start = (&raw const ap_trampoline_start) as *const u8 as u64;
    let end = (&raw const ap_trampoline_end) as *const u8 as u64;
    let len = (end - start) as usize;
    if len == 0 || len > 0x1000 {
        return Err("trampoline size out of page");
    }
    // 内核页表注入「单页恒等映射」VA 0x8000 → PA 0x8000（Linux 同范式）。
    // AP 开 PG 后取指/数据都走页表：没有它，far jump 到 0x8000+off 就是
    // #PF 三重故障；且 AP 全程用内核 CR3，规避「切 CR3 后下一条取指必死」
    // 的死结。只映射一页——空指针保护（VA 0 未映射）不受影响。
    // AP 进入长模式后访存同样依赖它（调试槽 [r15]=0x8000+off）。
    unsafe {
        let pml4 = (cr3 + hhdm) as *mut u64;
        if pml4.read_volatile() & 0x1 != 0 {
            return Err("pml4[0] already mapped");
        }
        for pa in [ID_PDPT_PA, ID_PD_PA, ID_PT_PA] {
            let tbl = (pa + hhdm) as *mut u64;
            for i in 0..512 {
                tbl.add(i).write_volatile(0);
            }
        }
        ((ID_PDPT_PA + hhdm) as *mut u64).write_volatile(ID_PD_PA | PTE_PRESENT_RW);
        ((ID_PD_PA + hhdm) as *mut u64).write_volatile(ID_PT_PA | PTE_PRESENT_RW);
        // PT[8]（VA 0x8000>>12=8）→ 物理页 0x8000，4KB 粒度
        ((ID_PT_PA + hhdm) as *mut u64)
            .add(8)
            .write_volatile(0x8000 | PTE_PRESENT_RW);
        // 链搭完最后挂 PML4[0]，对 AP 是一次性新页表（CR3 装载即见）
        pml4.write_volatile(ID_PDPT_PA | PTE_PRESENT_RW);
    }
    let dst = 0x8000u64 + hhdm;
    let off = |sym: u64| (sym - start) as u64;
    unsafe {
        core::ptr::copy_nonoverlapping(start as *const u8, dst as *mut u8, len);
        // GDT base = 线性 0x8000 + GDT 在 stub 内的偏移（伪描述符引用它）
        let gdt_base_slot = dst + off((&raw const tr_gdt_base_slot) as *const u8 as u64);
        (gdt_base_slot as *mut u32).write_volatile((0x8000u32).wrapping_add(off((&raw const tr_gdt_label) as *const u8 as u64) as u32));
        // CR3（物理，<4G）
        let cr3_slot = dst + off((&raw const tr_cr3_slot) as *const u8 as u64);
        (cr3_slot as *mut u32).write_volatile(cr3 as u32);
        // 入口（64 位内核虚拟地址）
        let entry_slot = dst + off((&raw const tr_entry_slot) as *const u8 as u64);
        (entry_slot as *mut u64).write_volatile(entry);
        // 内核 IDT（伪描述符：limit 已在镜像里，base 这里补）——AP 的
        // 任何异常都必须有归宿，跳板阶段就开始兜底。
        // 戒律：必须用 descriptor() 的 wire 表地址（CPU 真正派发的门表），
        // table() 的结构体首地址是 entries 逻辑表——差整整 4KB，拿它当
        // IDTR 的门全是 not-present，任何异常直接嵌套三重故障
        // （QEMU 取证：AP 的 IDTR=0x803b93a0，CR2=门表取指页）。
        let idt_base_slot = dst + off((&raw const tr_idt_base_slot) as *const u8 as u64);
        let (_, idt_wire) = crate::cpu::idt::table().descriptor();
        (idt_base_slot as *mut u64).write_volatile(idt_wire);
    }
    Ok(())
}

/// 每个 AP 专属：把该核的栈顶打进跳板。
fn patch_ap_stack(stack_top: u64) {
    let hhdm = crate::limine::hhdm_offset().expect("hhdm");
    let start = (&raw const ap_trampoline_start) as *const u8 as u64;
    let slot = (&raw const tr_stack_slot) as *const u8 as u64;
    let dst = 0x8000u64 + hhdm + (slot - start);
    unsafe { (dst as *mut u64).write_volatile(stack_top) };
}

/// 读 AP 跳板的阶段痕迹（0=未执行 1=16位 2=32位 3=64位 4=跳内核前）。
fn trampoline_stage() -> u32 {
    let hhdm = match crate::limine::hhdm_offset() {
        Some(h) => h,
        None => return u32::MAX,
    };
    let start = (&raw const ap_trampoline_start) as *const u8 as u64;
    let slot = (&raw const tr_debug_slot) as *const u8 as u64;
    let addr = 0x8000u64 + hhdm + (slot - start);
    unsafe { (addr as *const u32).read_volatile() }
}

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

    /// The three (dest, ICR low) writes that start the core. `dest` is the
    /// RAW architectural APIC id——`send_ipi` 按模式自行格式化（xAPIC 填
    /// ICR_HIGH[31:24]，x2APIC 放 ICR[63:32]）；在这里预移位就会双重移位。
    pub fn startup_commands(&self, apic_id: u32) -> [(u32, u32); 3] {
        let dest = apic_id;
        let init_deassert = icr_low(0, DM_INIT, false, IpiTarget::ApicId(apic_id).shorthand());
        let init_assert = icr_low(0, DM_INIT, true, IpiTarget::ApicId(apic_id).shorthand());
        let sipi = icr_low(
            TRAMPOLINE_VECTOR,
            DM_STARTUP,
            true,
            IpiTarget::ApicId(apic_id).shorthand(),
        );
        [(dest, init_deassert), (dest, init_assert), (dest, sipi)]
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

/// 2026-09-19：Intel MP 规范要求 INIT 与 SIPI 间隔 ≥10ms、两次 SIPI 间隔 ≥200µs；
/// 连发时 AP 还没从 INIT 复位完成，SIPI 被忽略（QEMU 宽容、真机必死）。
fn delay_us(us: u64) {
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    {
        let cal = crate::cpu::clock::calibration();
        let target = crate::cpu::clock::read_tsc() + cal.ns_to_tsc(us * 1_000);
        while crate::cpu::clock::read_tsc() < target {}
    }
    #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
    {
        let mut spin = 0u64;
        for i in 0..(us as u64 * 100) {
            spin = spin.wrapping_add(i);
        }
        core::hint::black_box(spin);
    }
}

/// 按 MP 规范时序把一颗 AP 拉起来：INIT → 10ms → SIPI → 200µs → SIPI。
fn send_startup_sequence(apic_id: u32) {
    let cmds = trampoline().startup_commands(apic_id);
    let lapic = crate::cpu::apic::lapic();
    let _ = cmds[0]; // INIT de-assert：老系统兼容位，现代硬件 INIT assert 即复位
    lapic.send_ipi(cmds[1].0, cmds[1].1); // INIT assert
    delay_10ms();
    lapic.send_ipi(cmds[2].0, cmds[2].1); // SIPI #1
    delay_us(200);
    lapic.send_ipi(cmds[2].0, cmds[2].1); // SIPI #2（首次 SIPI 时序竞争的兜底）
}

/// F041 + F047 bring-up: register the BSP, then start every AP the MADT lists.
pub fn init() -> u32 {
    let bsp_id = crate::cpu::apic::lapic().id();
    register(0, bsp_id);
    let mut started = 1u32;

    if let Some(madt) = crate::acpi::madt() {
        // 低 12 位是 PCID/标志位——跳板按物理帧处理，必须掩掉。
        let cr3 = current_cr3() & 0x0000_ffff_ffff_f000;
        let long_mode_entry = ap_entry();
        // 跳板先行：没有 0x8000 处的真实代码，SIPI 就是把 AP 发去执行垃圾。
        if let Err(e) = install_trampoline(cr3, long_mode_entry) {
            crate::kwarn!("smp: trampoline not installed ({}), APs skipped", e);
        } else {
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
                match trampoline().prepare(cpu_id, stack_top, cr3, long_mode_entry) {
                    Ok(()) => {
                        patch_ap_stack(stack_top);
                        send_startup_sequence(core.apic_id);
                        match trampoline().wait_ready(AP_READY_TIMEOUT_10MS) {
                            Ok(()) => {
                                register(cpu_id, core.apic_id);
                                started += 1;
                            }
                            Err(e) => {
                                let stage = trampoline_stage();
                                crate::kwarn!(
                                    "smp: apic {} failed: {:?} (trampoline stage={})",
                                    core.apic_id, e, stage
                                );
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
    }
    crate::kinfo!("smp: {} core(s) online (bsp apic {})", started, bsp_id);
    started
}

/// 64 KiB stack per AP, allocated from a boot-reserved region.
pub const AP_STACK_SIZE: usize = 64 * 1024;
pub const AP_STACKS: usize = MAX_CPUS - 1;

/// .bss 静态区（任务27 实测教训：immutable static 全零仍物化 .rodata，内核文件
/// >7.69MB 触发 Limine iso9660 "failed to read file data" 引导 PANIC——与
/// shmsrv::target::STORE 同范式落 .bss：static mut + &raw 访问，运行时零文件占用）。
static mut AP_STACK_AREA: [u8; AP_STACK_SIZE * AP_STACKS] = [0u8; AP_STACK_SIZE * AP_STACKS];

fn ap_stack_top(cpu_id: u32) -> u64 {
    let idx = (cpu_id as usize).saturating_sub(1).min(AP_STACKS - 1);
    let base = (&raw const AP_STACK_AREA) as *const u8 as u64;
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
    // 本核 IDT：全局表 BSP 已建好，这里只做本核 lidt。AP 任何异常都必须有
    // 归宿——三重故障静默死是最昂贵的调试方式。
    // SAFETY: the shared IDT table was fully populated by the BSP before the
    // first INIT was sent.
    unsafe { crate::cpu::idt::table().install() };
    // 本核 APIC 模式切换：INIT 复位后 AP 的 APIC 处于 disabled/xAPIC，而全局
    // lapic mode 是 BSP 探测的 X2Apic——不先切模式，read_x2apic 一律 #GP
    // （三重故障静默死，stage 停在 4）。SDM 禁止 disabled 直跳 x2APIC，
    // 必须两步：xAPIC → x2APIC。
    let mut base = crate::cpu::msr::read(crate::cpu::msr::Msr::ApicBase);
    base |= 1 << 11; // APIC global enable（xAPIC）
    base &= !(1 << 10);
    let _ = crate::cpu::msr::write(crate::cpu::msr::Msr::ApicBase, base);
    base |= 1 << 10; // x2APIC enable
    let _ = crate::cpu::msr::write(crate::cpu::msr::Msr::ApicBase, base);
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
    // ⚠ 调度器（AI-04）给本核派发第一个中断前必须：per-CPU GDT+TSS 安装并
    // load_tr——当前本核仍载着跳板的最小 GDT（无 TSS），任何走 IST 的向量
    // （NMI/DF/MC）或 ring3 切换都会三重故障。
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
        // 2026-09-19 实机根因回归锁：dest 必须是 RAW id。此前这里断言
        // `4 << 24`（xAPIC 格式），x2APIC 分支再 <<32 后目标变成 0x1000000，
        // INIT/SIPI 全部静默丢弃——AP 永不复位，实机卡在加载界面。
        assert_eq!(cmds[0].0, 4); // INIT de-assert（RAW）
        assert_eq!(cmds[1].0, 4); // INIT（RAW）
        assert_eq!(cmds[2].0, 4); // SIPI（RAW）
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
        assert!(a >= (&raw const AP_STACK_AREA) as *const u8 as u64);
    }
}
