//! 任务14 · ring3 切换 + 最小 syscall + 用户态 hello（双域总案·阶段2 步骤4/5）。
//!
//! 前置积木（全部既有）：GDT 用户段（gdt，RPL3）、TSS（RSP0 环零栈）、
//! `entry.rs` 的 MSR 计划与 `commit()`（wrmsr 五连）、`uspace.rs` 的
//! `plan_entry`/`ProcTable`、`elf.rs` 的 `parse`。本模块补上执行链：
//!
//! 1. **安装**：TSS.RSP0 → int 栈（int 0x80 的 ring3→ring0 换栈）；
//!    `SyscallEntry`（STAR/FMASK/LSTAR + EFER.SCE）经 `commit()` 真写
//!    MSR；LSTAR 指向 `syscall_entry`。
//! 2. **双入口**：
//!    - `syscall` 指令 → `syscall_entry`（naked stub：切内核栈 →
//!      `syscall_common` → sysretq）；
//!    - `int 0x80` → idt common_entry（dpl3 gate）→ `isr_dispatch` 分流
//!      `int80_from_frame`（wrapper 保存区取用户 rax/rdi/rsi/rdx，返回
//!      值写回保存区 rax，iretq 回用户）。
//!    两路共用 [`syscall_common`]——同一决策函数是"双入口结果一致"的
//!    结构性保证，宿主 ×1000 用例再证一次。
//! 3. **进入 ring3**：ELF 段逐页装载（`pfh` 新增 `map_user_frame`，
//!    P_USER/W/NX 按 `LoadSegment::page_flags`）+ 用户栈零页映射 →
//!    naked `enter_user`（iretq，通用寄存器清零，不泄露内核值）。
//! 4. **退场**：`exit(0)` → 进程槽 Zombie → `reap` 回收 Vacant（64 槽
//!    复用语义）→ 串口打印证据 → 停机（演示终点，不返回内核执行流）。
//!
//! 诚实边界：内核目标之外汇编路径不编译；宿主单测覆盖决策函数、缓冲
//! 校验、装载计划校验、退出回收复用与 blob 可解析性。

use super::uspace::ProcTable;
// elf 解析与进入计划只被 run_demo（内核目标）与宿主测试使用——宿主
// lib 编译（非 test cfg）下两者都不在场，不门控会挂 unused 告警。
#[cfg(any(all(target_arch = "x86_64", target_os = "none"), test))]
use super::elf;
#[cfg(any(all(target_arch = "x86_64", target_os = "none"), test))]
use super::uspace::{plan_entry, EntryPath};
use crate::cpu::sync::SpinProtected;
use crate::entry;
use crate::proc::syscall::{SyscallError, SYS_EXIT, SYS_WRITE};

/// 进程表（任务14 演示期单一实例；任务16 PCB 全局化时收编）。
static PROCS: SpinProtected<ProcTable> = SpinProtected::new(ProcTable::new());

/// 演示进程的 pid 登记：spawn 后写入，exit 读取（曾硬编码 1 而 hello
/// 实际是 2，exit/reap 全程空转）。
static DEMO_PID: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// 双入口共享的调用统计（实机/测试读数用；完整 SyscallTable 仍由
/// `proc::syscall` 持有形态定义，演示决策层不重复挂表）。
static CALLS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

pub fn calls() -> u64 {
    CALLS.load(core::sync::atomic::Ordering::Relaxed)
}

/// write 缓冲上限：用户态一次可写的最大字节数（防内核越权长拷）。
pub const MAX_WRITE: usize = 512;

/// syscall 指令入口的环零栈（stub 手工换栈）；int 0x80 走 TSS.RSP0。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
const KSTACK_PAGES: u64 = 16;
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
const INT_KSTACK_PAGES: u64 = 8;

/// hello.elf（构建产物随源入库；重建流程见 user/hello/README 说明）。
/// 使用者：run_demo（内核目标）与宿主测试——同上按 cfg 门控。
#[cfg(any(all(target_arch = "x86_64", target_os = "none"), test))]
static HELLO_ELF: &[u8] = include_bytes!("hello.elf");

// ---------------------------------------------------------------------------
// 决策层：双入口共用
// ---------------------------------------------------------------------------

/// 双入口（syscall 指令 / int 0x80）共同的语义层。返回 i64 ABI：
/// ≥0 = 成功值（write = 字节数），<0 = -errno（沿用 `SyscallError::errno`
/// 的约定值；本模块自产错误用 entry::ErrNo 反号）。
pub fn syscall_common(nr: u32, a1: u64, a2: u64, a3: u64) -> i64 {
    CALLS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    match nr {
        SYS_WRITE => sys_write(a1, a2, a3),
        SYS_EXIT => sys_exit(a1 as i32),
        // 其余稳定号（read/open/…）按任务14 口径如实 ENOSYS——
        // 号表形态定义在 proc::syscall（F102），处理器随任务15/16 落地。
        _ => SyscallError::NotImplemented.errno(),
    }
}

/// write(fd=1) 最小实现：缓冲校验（用户半区 + 长度上限）→ 逐字节串口。
/// fd/指针/长度三重校验后才触碰用户内存——内核绝不替坏参数越权。
fn sys_write(fd: u64, buf: u64, len: u64) -> i64 {
    if fd != 1 {
        return -ebadf();
    }
    if len > MAX_WRITE as u64 {
        return -einval();
    }
    if len == 0 {
        return 0;
    }
    let Some(end) = buf.checked_add(len) else {
        return -efault();
    };
    // 用户半区整段校验（防内核指针伪装成用户缓冲）。
    if end > entry::USER_TOP || !entry::is_user_ip(buf) {
        return -efault();
    }
    // 输出双通道：serial（验收证据）+ console（VGA，用户可见）。
    // put_byte 只写 VGA——QEMU -display none 下串口才是可观测出口。
    let mut line = [0u8; MAX_WRITE];
    // SAFETY: buf..end 已校验在用户半区；单核演示地址空间独占。
    for (i, slot) in line[..len as usize].iter_mut().enumerate() {
        *slot = unsafe { core::ptr::read_volatile((buf + i as u64) as *const u8) };
    }
    // 诊断（验收后移除，仅内核目标）：CR3 + sys_write 实读首 8 字节 +
    // 0x401000 页翻译与物理帧直读。曾经写成 0x401_0000——翻译了一个
    // 从未映射的地址，tr=None 全是红鲱鱼。宿主无真页表（target_ops
    // 不编译）且 buf=0 不可读，整块 cfg 门控。
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    {
        let cr3: u64;
        // SAFETY: 读控制寄存器无内存副作用。
        unsafe {
            core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem));
        }
        let peek = unsafe { core::ptr::read_volatile(buf as *const u64) };
        use crate::mem::pfh::PageTableOps as _;
        let mut ops = crate::mem::pfh::target_ops();
        let tr = ops.translate(0x401_000);
        let hhdm = crate::limine::hhdm_offset().unwrap_or(0);
        let frame_read = match tr {
            Some(p) => unsafe {
                core::ptr::read_volatile(((p & 0x000f_ffff_ffff_f000) + 0x70 + hhdm) as *const u64)
            },
            None => 0xDEAD_BEEF,
        };
        crate::kinfo!(
            "ring3: write cr3={:#x} peek={:#x} pte={:#x?} frame={:#x}",
            cr3,
            peek,
            tr,
            frame_read
        );
    }
    crate::serial::write_bytes(&line[..len as usize]);
    if let Some(c) = crate::console::installed_ref() {
        for &b in &line[..len as usize] {
            c.put_byte(b);
        }
    }
    len as i64
}

// entry::ErrNo 正值（调用点统一取负，负 errno ABI）。
const fn einval() -> i64 {
    entry::ErrNo::Einval.to_i32() as i64
}
const fn efault() -> i64 {
    entry::ErrNo::Efault.to_i32() as i64
}
const fn ebadf() -> i64 {
    entry::ErrNo::Ebadf.to_i32() as i64
}

/// exit：置 Zombie → reap 回 Vacant（64 槽复用）→ 打印证据 → 停机。
/// 内核侧不返回——syscall stub 的 sysret 永远不会在 exit 之后执行。
/// pid 取自 spawn 时登记的静态槽（演示期唯一用户进程；曾经硬编码
/// pid=1，而 hello 实际 pid=2——exit/reap 空转，回收断言假通过）。
fn sys_exit(code: i32) -> ! {
    let pid = DEMO_PID.load(core::sync::atomic::Ordering::Relaxed);
    let (reaped, slot_free_again) = {
        let mut t = PROCS.lock();
        t.exit(pid, code, 0);
        let r = t.reap(pid);
        (r.is_some(), t.find(pid).is_none())
    };
    crate::kinfo!(
        "ring3: exit({}) pid={} zombie_reaped={} slot_free_again={} — 64-slot reuse ok",
        code,
        pid,
        reaped,
        slot_free_again
    );
    if !reaped {
        crate::kwarn!("ring3: exit reap failed — zombie path broken");
        halt_demo();
    }
    halt_demo();
}

/// 演示终点停机（行为等价于 boot complete 的 halting，只是先跑完 ring3）。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
fn halt_demo() -> ! {
    crate::kinfo!("ring3: user program finished — halting");
    loop {
        unsafe {
            core::arch::asm!("cli", options(nomem, nostack));
            core::arch::asm!("hlt", options(nomem, nostack, preserves_flags));
        }
    }
}
#[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
fn halt_demo() -> ! {
    unreachable!("ring3 exit is target-only")
}

// ---------------------------------------------------------------------------
// 目标态：环零栈 / 汇编 stub / 安装 / 装载 / 进入
// ---------------------------------------------------------------------------

#[cfg(all(target_arch = "x86_64", target_os = "none"))]
mod target_only {
    use core::cell::UnsafeCell;
    use core::sync::atomic::{AtomicU64, Ordering};

    /// 环零栈（.bss 静态页，boot 期单线程写者）。**必须**用 `UnsafeCell`
    /// 包裹：这两个 static 只被取地址（写入全在裸汇编里，编译器不可见），
    /// 无内部可变性时 LLVM 会把「无写访问的全零私有 static」常量降级进
    /// `.rodata` —— 实机 iretq 前 push 写只读页 → #PF → #DF（cr2=栈顶，
    /// 铁证）。UnsafeCell 强制存储落可写段。字段地址即栈存储，内容不经
    /// Rust 读取——dead_code 属预期，显式豁免。
    #[repr(align(16))]
    pub struct KStack(#[allow(dead_code)] UnsafeCell<[u8; super::KSTACK_PAGES as usize * 4096]>);
    #[repr(align(16))]
    pub struct IntStack(#[allow(dead_code)] UnsafeCell<[u8; super::INT_KSTACK_PAGES as usize * 4096]>);
    // SAFETY: 栈存储只在 boot 期单线程窗口被触碰（enter_user 换栈、
    // TSS.RSP0 指向的中断入口），无并发访问者。
    unsafe impl Sync for KStack {}
    unsafe impl Sync for IntStack {}

    pub static KSTACK: KStack = KStack(UnsafeCell::new(
        [0; super::KSTACK_PAGES as usize * 4096],
    ));
    pub static INT_KSTACK: IntStack = IntStack(UnsafeCell::new(
        [0; super::INT_KSTACK_PAGES as usize * 4096],
    ));

    pub fn kstack_top() -> u64 {
        (&raw const KSTACK).addr() as u64 + super::KSTACK_PAGES * 4096
    }
    pub fn int_kstack_top() -> u64 {
        (&raw const INT_KSTACK).addr() as u64 + super::INT_KSTACK_PAGES * 4096
    }

    /// stub 与 Rust 之间的静态槽。原子类型使 Rust 侧写入免 UB；x86-64
    /// 上 Relaxed 原生就是普通 mov，与 stub 的裸读写同一内存序。stub 经
    /// `sym target_only::…` 直接寻址这里的符号——**唯一事实源**，不存在
    /// 第二份同名槽（曾经两处同名导致 install 写 A、stub 读 B 的真 bug）。
    pub static USER_RSP_SLOT: AtomicU64 = AtomicU64::new(0);
    pub static KSTACK_TOP_SLOT: AtomicU64 = AtomicU64::new(0);

    /// 把环零栈顶写进 stub 可读的槽（install 之前调用一次）。
    pub fn prime_stack_slots() {
        KSTACK_TOP_SLOT.store(kstack_top(), Ordering::Relaxed);
    }
}

#[cfg(all(target_arch = "x86_64", target_os = "none"))]
use target_only::{int_kstack_top, prime_stack_slots};

/// LSTAR 入口。CPU 已置 rcx=用户 rip、r11=用户 rflags；rsp 仍是用户的。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
#[unsafe(naked)]
unsafe extern "C" fn syscall_entry() -> ! {
    core::arch::naked_asm!(
        // 换环零栈；用户 rsp 存静态槽（syscall 指令不自动保存它）。
        "mov [rip + {user_rsp}], rsp",
        "mov rsp, [rip + {kstack_top}]",
        // rcx/r11 必须活到 sysret（rip/rflags），连同用户 rdi/rsi/rdx 入栈。
        "push rcx",
        "push r11",
        "push rdi",
        "push rsi",
        "push rdx",
        // 用户 ABI：rax=nr, rdi/rsi/rdx=a1/a2/a3（与 int 0x80 通道一致）。
        // SysV dispatch：rdi=nr, rsi=a1, rdx=a2, rcx=a3——逐参搬运，
        // 曾经直接拿用户 rsi 当 a1（错位一位，exit 收到 buf 地址当 code）。
        "mov rdi, rax",
        "mov rsi, [rsp + 16]", // a1 = 用户 rdi
        "mov rdx, [rsp + 8]",  // a2 = 用户 rsi
        "mov rcx, [rsp]",      // a3 = 用户 rdx
        "call {dispatch}",
        // 栈（高→低）= rcx,r11,rdi,rsi,rdx 槽；call 返回后 add rsp,24
        // 越过 rdx/rsi/rdi 槽（syscall ABI 允许破坏这三个 caller-saved），
        // 此时 rsp 恰在 r11 槽——依次恢复 rflags 与用户 rip，最后从
        // 静态槽恢复用户 rsp。**曾经 pop rdx,rsi,rdi 再 pop r11,rcx——
        // 多弹三层，rcx 吃进栈外垃圾，sysretq 直接跳飞（RIP=1）**。
        "add rsp, 24",
        "pop r11",
        "pop rcx",
        "mov rsp, [rip + {user_rsp}]",
        "sysretq",
        user_rsp = sym target_only::USER_RSP_SLOT,
        kstack_top = sym target_only::KSTACK_TOP_SLOT,
        dispatch = sym syscall_common,
    )
}

/// iretq 进入 ring3：通用寄存器清零（不泄露内核值），只带
/// rip/rsi(=用户rsp)/rdx(=rflags)。不返回。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
#[unsafe(naked)]
unsafe extern "C" fn enter_user(rip: u64, rsp: u64, rflags: u64) -> ! {
    core::arch::naked_asm!(
        "mov rsp, [rip + {kstack_top}]",
        "push {ss_user}", // SS
        "push rsi",       // 用户 RSP
        "push rdx",       // RFLAGS
        "push {cs_user}", // CS（RPL3）
        "push rdi",       // RIP
        "xor rax, rax",
        "xor rbx, rbx",
        "xor rcx, rcx",
        "xor rdx, rdx",
        "xor rsi, rsi",
        "xor rdi, rdi",
        "xor rbp, rbp",
        "xor r8, r8",
        "xor r9, r9",
        "xor r10, r10",
        "xor r11, r11",
        "xor r12, r12",
        "xor r13, r13",
        "xor r14, r14",
        "xor r15, r15",
        "        iretq",
        kstack_top = sym target_only::KSTACK_TOP_SLOT,
        ss_user = const entry::GDT_USER_DATA as u64,
        cs_user = const entry::GDT_USER_CODE as u64,
    )
}

/// 安装 syscall 硬件入口。返回是否全部生效（宿主 false，不伪造）。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn install() -> bool {
    prime_stack_slots();
    // int 0x80 的环零栈：TSS.RSP0 必须指真栈，否则 ring3 触发即 #SS。
    // SAFETY: TSS 单线程 boot 期配置。
    unsafe {
        crate::cpu::gdt::set_rsp0(int_kstack_top());
    }
    let cur_efer = crate::cpu::msr::read(crate::cpu::msr::Msr::Efer);
    let entry_cfg = entry::SyscallEntry::new(syscall_entry as *const () as u64, cur_efer);
    let ok = entry::commit(&entry_cfg);
    // 诊断（验收后移除）：两个环零栈的地址——GP 现场的 rsp 归属一目了然。
    crate::kinfo!(
        "ring3: kstack={:#x}..{:#x} int_kstack={:#x}..{:#x}",
        target_only::kstack_top() - KSTACK_PAGES * 4096,
        target_only::kstack_top(),
        int_kstack_top() - INT_KSTACK_PAGES * 4096,
        int_kstack_top()
    );
    ok
}

#[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
pub fn install() -> bool {
    false
}

/// FMASK 生效性：进入内核时 IF/IOPL/NT/TF/AC 必须被硬件清掉。
pub fn fmask_covers_if_iopl_nt_tf_ac() -> bool {
    const F: u64 = entry::FMASK_VALUE;
    F & 0x100 != 0 && F & 0x200 != 0 && F & 0x3000 != 0 && F & 0x4000 != 0 && F & 0x40000 != 0
}

/// int 0x80 分流（idt isr_dispatch 调用）：wrapper 保存区取用户
/// rax/rdi/rsi/rdx → 共享决策层 → 返回值写回保存区 rax（iretq 后即
/// 用户的返回值）。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn int80_from_frame(rsp: u64) {
    let read = |off: i64| -> u64 {
        // SAFETY: wrapper 帧内偏移，帧存活至 iretq。
        unsafe { core::ptr::read((rsp as i64 + off) as *const u64) }
    };
    let nr = read(-8) as u32; // rax
    let a1 = read(-40); // rdi
    let a2 = read(-32); // rsi
    let a3 = read(-24); // rdx
    let ret = syscall_common(nr, a1, a2, a3);
    // SAFETY: 只改保存区 rax 槽。
    unsafe {
        core::ptr::write((rsp as i64 - 8) as *mut u64, ret as u64);
    }
    // 诊断：写回后读回 CPU 帧（+8 vector, +16 RIP, +24 CS, +32 RFLAGS,
    // +40 RSP, +48 SS）——iretq 就从 +16 弹起，若这里已经坏，坏在内核侧。
    let rd = |off: i64| -> u64 { unsafe { core::ptr::read((rsp as i64 + off) as *const u64) } };
    crate::kinfo!(
        "ring3: int80 vec={:#x} rip={:#x} cs={:#x} fl={:#x} sp={:#x} ss={:#x} raxw={:#x}",
        rd(8),
        rd(16),
        rd(24),
        rd(32),
        rd(40),
        rd(48),
        rd(-8)
    );
}

/// 宿主占位：idt 的 isr_dispatch 在宿主测试目标同样编译，vector 0x80
/// 的分流调用需要符号存在。宿主没有 CPU 中断帧，此路径物理不可达。
#[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
pub fn int80_from_frame(_rsp: u64) {}

/// 装载 hello 并进入 ring3。不返回（exit 停机；任何一步失败也如实
/// kwarn 后停机——绝不带着半成品地址空间进用户态）。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn run_demo() -> ! {
    let blob = HELLO_ELF;
    crate::kinfo!(
        "ring3: loading hello.elf ({} bytes) into user half",
        blob.len()
    );
    let img = match elf::parse(blob) {
        Ok(i) => i,
        Err(e) => {
            crate::kwarn!("ring3: elf parse failed: {} — demo aborted", e.as_str());
            halt_demo();
        }
    };
    if img.needs_interpreter() {
        crate::kwarn!("ring3: dynamically linked ELF refused — demo aborted");
        halt_demo();
    }

    // 进程槽先行（exit 要回收它）。
    let pid = {
        let mut t = PROCS.lock();
        match t.alloc(0, b"hello") {
            Ok(p) => p,
            Err(e) => {
                crate::kwarn!("ring3: proc alloc failed: {:?} — demo aborted", e);
                halt_demo()
            }
        }
    };
    DEMO_PID.store(pid, core::sync::atomic::Ordering::Relaxed);
    crate::kinfo!("ring3: hello spawned as pid={} (slot occupied)", pid);

    // 段装载：每 4KiB **页**一帧（不是每段每页一帧）。hello.elf 的
    // .rodata@0x401000 与 .data@0x401088 同页——曾经各自分配新帧并整体
    // 覆写 PTE：后者（W 帧）顶掉前者（R 帧），MSG 经页表读出全零。
    // 同页多段必须：共享一帧、按页内真实偏移拷贝、flags 取并集。
    use crate::mem::pfh::PageTableOps;
    let mut pt = crate::mem::pfh::target_ops();
    let hhdm = crate::limine::hhdm_offset().unwrap_or(0);
    let mut mapped = 0u64;
    // 金丝雀（验收后移除）：MSG 页装载后记录物理帧，此后每次新帧分配/
    // 映射都回读校验——装载期破坏当场报出肇事 va/phys，不再靠终态猜。
    const CANARY: u64 = 0x7266_206F_6C6C_6568; // "hello fr" 小端 u64
    let mut seg0_phys = 0u64;
    // 页账本：(页基址, 帧, writable, nx)；演示 19 页封顶，32 足够。
    let mut ledger: [(u64, u64, bool, bool); 32] = [(0, 0, false, false); 32];
    let mut ledger_n = 0usize;
    for seg in img.segments() {
        let flags = seg.page_flags();
        let seg_w = flags & crate::mem::paging::P_WRITE != 0;
        let seg_nx = flags & crate::mem::paging::P_NX != 0;
        let pages = seg.memsz.div_ceil(4096).max(1);
        for k in 0..pages {
            let va = seg.vaddr + k * 4096;
            let page_va = va & !0xFFF;
            let in_page = va & 0xFFF;
            // 该页是否已有帧（同页多段共享，不得重复分配）。
            let (phys, w, nx) = match (0..ledger_n).find(|&i| ledger[i].0 == page_va) {
                Some(i) => {
                    // flags 并集：任一段要 W 即 W，任一段要 NX 即 NX。
                    let merged_w = ledger[i].2 || seg_w;
                    let merged_nx = ledger[i].3 || seg_nx;
                    ledger[i].2 = merged_w;
                    ledger[i].3 = merged_nx;
                    (ledger[i].1, merged_w, merged_nx)
                }
                None => {
                    let Some(p) = crate::mem::pmm::alloc_page() else {
                        crate::kwarn!("ring3: pmm exhausted — demo aborted");
                        halt_demo()
                    };
                    let page = (p + hhdm) as *mut u8;
                    // SAFETY: 新帧来自 PMM，HHDM 全覆盖；页内布局本函数独占。
                    unsafe {
                        core::ptr::write_bytes(page, 0, 4096);
                    }
                    ledger[ledger_n] = (page_va, p, seg_w, seg_nx);
                    ledger_n += 1;
                    (p, seg_w, seg_nx)
                }
            };
            // 文件内容按页内真实偏移拷贝（段可以从页中间开始）。
            if k * 4096 < seg.filesz {
                let file_off = (seg.offset + k * 4096) as usize;
                let n = core::cmp::min(4096 - in_page as usize, (seg.filesz - k * 4096) as usize);
                if file_off + n <= blob.len() {
                    // SAFETY: blob 只读、帧由本函数独占，区间不重叠。
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            blob.as_ptr().add(file_off),
                            (phys + hhdm + in_page) as *mut u8,
                            n,
                        );
                    }
                }
            }
            if !pt.map_user_frame(page_va, phys, w, nx) {
                crate::kwarn!("ring3: map user page failed — demo aborted");
                halt_demo()
            }
            pt.flush(page_va);
            mapped += 1;
            // 诊断（验收后移除）：MSG 页（0x401000）首次映射时记录帧。
            if page_va == 0x401000 && seg0_phys == 0 {
                seg0_phys = phys;
                let via_hhdm = unsafe { core::ptr::read_volatile((phys + hhdm + 0x70) as *const u64) };
                let via_va = unsafe { core::ptr::read_volatile((page_va + 0x70) as *const u64) };
                crate::kinfo!(
                    "ring3: msg page phys={:#x} via_hhdm={:#x} via_va={:#x}",
                    phys,
                    via_hhdm,
                    via_va
                );
            }
            // 金丝雀校验：其它帧的分配/映射后回读 MSG 页帧。
            if seg0_phys != 0 && phys != seg0_phys {
                let cur = unsafe { core::ptr::read_volatile((seg0_phys + hhdm + 0x70) as *const u64) };
                if cur != CANARY {
                    crate::kerror!(
                        "ring3: msg frame {:#x} clobbered by map va={:#x} phys={:#x} read={:#x} — halting",
                        seg0_phys,
                        page_va,
                        phys,
                        cur
                    );
                    halt_demo();
                }
            }
        }
    }
    // 用户栈：DEFAULT_STACK_PAGES 零页（P_USER|W|NX），从栈顶向下。
    for k in 0..entry::DEFAULT_STACK_PAGES as u64 {
        let va = entry::USER_STACK_TOP - (k + 1) * 4096;
        let Some(phys) = crate::mem::pmm::alloc_page() else {
            crate::kwarn!("ring3: pmm exhausted for stack — demo aborted");
            halt_demo()
        };
        let page = (phys + hhdm) as *mut u8;
        // SAFETY: 新帧来自 PMM，独占写。
        unsafe {
            core::ptr::write_bytes(page, 0, 4096);
        }
        if !pt.map_user_frame(va, phys, true, true) {
            crate::kwarn!("ring3: map stack page failed — demo aborted");
            halt_demo()
        }
        pt.flush(va);
        mapped += 1;
        // 金丝雀校验（栈页阶段同样盯 seg0 帧）。
        if seg0_phys != 0 {
            let cur = unsafe { core::ptr::read_volatile((seg0_phys + hhdm + 0x70) as *const u64) };
            if cur != CANARY {
                crate::kerror!(
                    "ring3: seg0 frame {:#x} clobbered by stack alloc va={:#x} phys={:#x} read={:#x} — halting",
                    seg0_phys,
                    va,
                    phys,
                    cur
                );
                halt_demo();
            }
        }
    }
    crate::kinfo!(
        "ring3: {} user pages mapped (segments+stack), entry {:#x}",
        mapped,
        img.entry
    );

    // 快照（验收后移除）：0x401000 走的四级链原始项 + MSG 帧内容 + CR3
    // + STAR/FMASK 读回——大叶伪链、装载窗口外的破坏、STAR 高半段错误
    //（SYSRET 的 SS=CS+8 契约）在此一锤定音。
    {
        let rd = |phys: u64| -> u64 { unsafe { core::ptr::read_volatile((phys + hhdm) as *const u64) } };
        let cr3_snap: u64;
        // SAFETY: 读控制寄存器无内存副作用。
        unsafe {
            core::arch::asm!("mov {}, cr3", out(reg) cr3_snap, options(nomem));
        }
        let mask = crate::mem::paging::P_ADDR_MASK;
        let pml4e0 = rd(cr3_snap & mask);
        let pdpte0 = rd(pml4e0 & mask);
        let pde2 = rd((pdpte0 & mask) + 2 * 8);
        let huge = pde2 & crate::mem::paging::P_HUGE != 0;
        let pte1 = if huge { 0 } else { rd((pde2 & mask) + 1 * 8) };
        let seg0 = if seg0_phys != 0 {
            unsafe { core::ptr::read_volatile((seg0_phys + hhdm + 0x70) as *const u64) }
        } else {
            0
        };
        let star = crate::cpu::msr::read(crate::cpu::msr::Msr::Star);
        let fmask = crate::cpu::msr::read(crate::cpu::msr::Msr::SFmask);
        let lstar = crate::cpu::msr::read(crate::cpu::msr::Msr::LStar);
        crate::kinfo!(
            "ring3: pre-iretq cr3={:#x} pml4e0={:#x} pdpte0={:#x} pde2={:#x} huge={} pte1={:#x} seg0={:#x} star={:#x} fmask={:#x} lstar={:#x}",
            cr3_snap,
            pml4e0,
            pdpte0,
            pde2,
            huge as u8,
            pte1,
            seg0,
            star,
            fmask,
            lstar
        );
    }

    // 进入计划（既有校验：用户半区 + 对齐 + rflags 硬性要求）。
    let mut plan = super::uspace::EntryPlan::default();
    if let Err(e) = plan_entry(EntryPath::Iret, img.entry, entry::USER_STACK_TOP, &mut plan) {
        crate::kwarn!("ring3: entry plan rejected: {} — demo aborted", e);
        halt_demo()
    }
    crate::kinfo!(
        "ring3: iretq → user (rip={:#x} rflags={:#x}) — hello should print twice via int80 & syscall",
        plan.rip,
        plan.rflags
    );
    // SAFETY: 依赖上面已映射的页、已写入的 MSR 与 TSS.RSP0。
    unsafe { enter_user(plan.rip, plan.rsp, plan.rflags) }
}

// ---------------------------------------------------------------------------
// 宿主测试：双入口一致性 ×1000、缓冲校验、退出回收复用、计划拒绝
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proc::uspace::user_rflags;

    #[test]
    fn dual_entry_results_agree_x1000() {
        // 结构上两路共用 syscall_common；这里 ×1000 验证决策纯函数稳定
        // （len=0 → 0，不触碰串口），实机再以两行相同输出复核。
        for i in 0..1000u64 {
            let via_int80 = simulate_int80(SYS_WRITE, 1, 0, 0);
            let via_syscall = syscall_common(SYS_WRITE, 1, 0, 0);
            assert_eq!(via_int80, via_syscall, "round {i}: 双入口必须一致");
            assert_eq!(via_syscall, 0);
        }
        assert!(
            calls() >= 2000,
            "双入口统计必须如实增长（1000 轮 × 2 路）"
        );
    }

    /// int 0x80 的保存区偏移读取在宿主的等价复刻（偏移必须与目标态
    /// int80_from_frame 逐字一致：rax@-8 rdi@-40 rsi@-32 rdx@-24）。
    fn simulate_int80(nr: u32, a1: u64, a2: u64, a3: u64) -> i64 {
        let mut mem = [0u64; 16];
        let base = mem.as_mut_ptr() as u64 + 8 * 8;
        unsafe {
            core::ptr::write((base as i64 - 8) as *mut u64, nr as u64);
            core::ptr::write((base as i64 - 40) as *mut u64, a1);
            core::ptr::write((base as i64 - 32) as *mut u64, a2);
            core::ptr::write((base as i64 - 24) as *mut u64, a3);
        }
        let rd = |off: i64| unsafe { core::ptr::read((base as i64 + off) as *const u64) };
        syscall_common(rd(-8) as u32, rd(-40), rd(-32), rd(-24))
    }

    #[test]
    fn write_buffer_validation_rejects_bad_args() {
        // fd!=1 → EBADF(-8)。
        assert_eq!(syscall_common(SYS_WRITE, 2, 0, 0), -8);
        // len 超上限 → EINVAL(-1)。
        let big = (MAX_WRITE + 1) as u64;
        assert_eq!(syscall_common(SYS_WRITE, 1, 0, big), -1);
        // 内核地址当用户缓冲 → EFAULT(-4)。
        assert_eq!(syscall_common(SYS_WRITE, 1, 0xFFFF_8000_0000_0000, 4), -4);
        // 指针环绕 → EFAULT。
        assert_eq!(syscall_common(SYS_WRITE, 1, u64::MAX, 4), -4);
        // 越过 USER_TOP → EFAULT。
        assert_eq!(syscall_common(SYS_WRITE, 1, entry::USER_TOP - 2, 4), -4);
        // 未注册号 → 表口径 ENOSYS(-38)。
        assert_eq!(syscall_common(9999, 0, 0, 0), -38);
    }

    #[test]
    fn exit_reclaims_slot_for_reuse() {
        let mut t = ProcTable::new();
        let pid = t.alloc(0, b"hello").unwrap();
        assert!(t.find(pid).is_some());
        t.exit(pid, 0, 0);
        assert!(t.reap(pid).is_some(), "Zombie 必须可 reap");
        assert!(t.find(pid).is_none(), "回收后槽 Vacant");
        let pid2 = t.alloc(0, b"hello2").unwrap();
        assert_eq!(pid2, pid, "复用必须拿到同一最小槽位（64 槽复用语义）");
    }

    #[test]
    fn entry_plan_rejects_kernel_addresses() {
        let mut plan = crate::proc::uspace::EntryPlan::default();
        // 内核 rip → 拒（把内核地址交给 sysret/iret 是典型提权）。
        assert!(plan_entry(
            EntryPath::Iret,
            0xFFFF_8000_0010_0000,
            entry::USER_STACK_TOP,
            &mut plan
        )
        .is_err());
        // 内核栈 → 拒。
        assert!(plan_entry(EntryPath::Iret, 0x400030, 0xFFFF_8000_0080_0000, &mut plan).is_err());
        // 合法组合 → 过，且 rflags 带用户态硬性要求（IF 开、IOPL=3）。
        assert!(plan_entry(EntryPath::Iret, 0x400030, entry::USER_STACK_TOP, &mut plan).is_ok());
        assert_eq!(plan.rflags, user_rflags());
    }

    #[test]
    fn fmask_masks_if_iopl_nt_tf_ac() {
        assert!(fmask_covers_if_iopl_nt_tf_ac());
        const F: u64 = entry::FMASK_VALUE;
        assert_eq!(F & 0x100, 0x100, "TF");
        assert_eq!(F & 0x200, 0x200, "IF");
        assert_eq!(F & 0x400, 0x400, "DF");
        assert_eq!(F & 0x3000, 0x3000, "IOPL");
        assert_eq!(F & 0x4000, 0x4000, "NT");
        assert_eq!(F & 0x40000, 0x40000, "AC");
    }

    #[test]
    fn hello_blob_parses_and_lands_in_user_half() {
        let img = elf::parse(HELLO_ELF).expect("blob 必须可解析");
        assert!(!img.needs_interpreter(), "演示只跑静态链接");
        assert!(img.entry < entry::USER_TOP, "入口必须用户半区");
        for seg in img.segments() {
            assert!(seg.end() <= entry::USER_TOP);
            assert_eq!(
                seg.page_flags() & crate::mem::paging::P_USER,
                crate::mem::paging::P_USER,
                "每段都必须带用户位"
            );
        }
    }

    #[test]
    fn star_layout_matches_gdt_user_segments() {
        // STAR[63:48] = user code 基值（0x1b 清 RPL = 0x18；sysret 时 CPU
        // 强制 RPL3，SS = CS+8 恰落 user data 0x20）；
        // STAR[47:32] = KERNEL_CS（syscall 进内核码段，SS = +8 = 内核数据）。
        let s = entry::build_star();
        assert_eq!((s >> 48) as u16, entry::GDT_USER_CODE as u16 & !0x3);
        assert_eq!(((s >> 48) as u16) + 8, entry::GDT_USER_DATA as u16 & !0x3);
        assert_eq!((s >> 32) as u16, entry::GDT_KERNEL_CODE as u16);
        assert_eq!(((s >> 32) as u16) + 8, crate::cpu::gdt::SEL_KERNEL_DATA as u16);
    }
}
