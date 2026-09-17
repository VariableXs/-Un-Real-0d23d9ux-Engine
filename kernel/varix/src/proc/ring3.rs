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

use super::uspace::{ProcState, ProcTable};
// elf 解析与进入计划只被 run_demo（内核目标）与宿主测试使用——宿主
// lib 编译（非 test cfg）下两者都不在场，不门控会挂 unused 告警。
#[cfg(any(all(target_arch = "x86_64", target_os = "none"), test))]
use super::elf;
#[cfg(any(all(target_arch = "x86_64", target_os = "none"), test))]
use super::uspace::{plan_entry, EntryPath};
use crate::cpu::sync::SpinProtected;
use crate::entry;
use crate::proc::syscall::{SyscallError, SYS_EXIT, SYS_WAIT, SYS_WRITE};

/// 进程表（任务14/15 演示期单一实例；任务16 PCB 全局化时收编）。
static PROCS: SpinProtected<ProcTable> = SpinProtected::new(ProcTable::new());

/// 演示进程的 pid 登记：spawn 后写入，exit/wait 读取（曾经硬编码
/// 1，而 hello 实际是 2——exit/reap 全程空转）。
static DEMO_PID: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// 监护生命周期轮数：两轮完整 spawn→exit→wait 后进入压力探针。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
const LIFECYCLES: u32 = 2;
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
static EXITS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// 任务15 · 逐页账本（pid → 装载页）：退出回收的凭据。装在监护者手里，
/// reap 时摘叶归还帧——地址空间随进程消亡，一页不留。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
static CHILD_PAGES: SpinProtected<alloc::vec::Vec<(u32, alloc::vec::Vec<(u64, u64)>)>> =
    SpinProtected::new(alloc::vec::Vec::new());

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

/// 任务39（AI-B）· 静态 PE64 样例（tools/make-pe.py 生成）：
/// write(1, "hello from PE"(NL), 14) 后 exit(0)，与 hello.elf 同一 syscall ABI。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
static HELLO_PE: &[u8] = include_bytes!("hello.pe");

/// PE 实例只跑一轮（监护链状态位）。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
static PE_DONE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

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
        SYS_WAIT => sys_wait_user(),
        // 其余稳定号（read/open/…）按任务15 口径如实 ENOSYS——
        // 号表形态定义在 proc::syscall（F102），处理器随任务16 落地。
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

/// exit：置 Zombie 后把控制权交还监护者——回收是 wait（父）的职责，
/// 这是父子分工的真实语义（任务14 曾在 exit 里就地 reap+停机，回收
/// 断言是自导自演）。pid 取自 spawn 时登记的静态槽（曾经硬编码
/// pid=1，而 hello 实际 pid=2——exit/reap 全程空转，断言假通过）。
fn sys_exit(code: i32) -> ! {
    let pid = DEMO_PID.load(core::sync::atomic::Ordering::Relaxed);
    let zombie = {
        let mut t = PROCS.lock();
        t.exit(pid, code, 0);
        t.find(pid)
            .and_then(|i| t.get(i))
            .map(|p| p.state == ProcState::Zombie)
            .unwrap_or(false)
    };
    crate::kinfo!(
        "ring3: exit({}) pid={} → zombie={} — 控制权交还监护者，wait 收割",
        code,
        pid,
        zombie
    );
    if !zombie {
        crate::kwarn!("ring3: exit did not reach Zombie — lifecycle broken");
        halt_demo();
    }
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    resume_supervisor();
    #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
    halt_demo();
}

/// wait 语义的纯函数（宿主可测）：收割调用者的一个僵尸孩子。
/// 返回 (child_pid << 32 | exit_code)；没有可收割的孩子 → -ECHILD。
fn wait_user_in(t: &mut ProcTable, me: u32) -> i64 {
    match t.wait(me) {
        Some((child, code)) => ((child as i64) << 32) | (code as i64 & 0xFFFF_FFFF),
        None => -(entry::ErrNo::Echild.to_i32() as i64),
    }
}

/// 用户态 wait 臂。演示期调用者身份 = 唯一用户进程（DEMO_PID）；
/// 多进程调用方身份随任务16 PCB 全局化接入 stub。
fn sys_wait_user() -> i64 {
    let me = DEMO_PID.load(core::sync::atomic::Ordering::Relaxed);
    let mut t = PROCS.lock();
    wait_user_in(&mut t, me)
}

// ---------------------------------------------------------------------------
// 目标态：监护生命周期（任务15）——spawn → exit → wait → 复用 + 压力探针
// ---------------------------------------------------------------------------

/// 从 exit 现场（syscall stub 的 kstack 帧）切回环零栈顶，跳进监护续体。
/// stub 残帧被有意放弃——exit 之后 sysretq 永不执行；rsp 重置保证监护
/// 循环每轮从同一栈顶开始，无栈深度累积。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
fn resume_supervisor() -> ! {
    let top = target_only::kstack_top();
    // SAFETY: 跳转目标 after_exit 不返回；kstack 此刻无其他使用者
    // （演示期单核、用户现场已放弃）。
    unsafe {
        core::arch::asm!(
            "mov rsp, {top}",
            "xor ebp, ebp",
            "jmp {after}",
            top = in(reg) top,
            after = sym after_exit,
            options(noreturn)
        )
    }
}

/// 监护续体：收割刚退场的实例（wait=父子语义的真实回收点）+ 页账本
/// 摘叶归还帧 → 决定下一实例或压力探针。每轮一个实例，先跑满
/// [`LIFECYCLES`] 轮完整生命周期。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
fn after_exit() -> ! {
    use core::sync::atomic::Ordering;
    let exits = EXITS.fetch_add(1, Ordering::Relaxed) + 1;
    match PROCS.lock().wait(0) {
        Some((pid, code)) => {
            let ledger = CHILD_PAGES
                .lock()
                .iter()
                .find(|e| e.0 == pid)
                .map(|e| e.1.clone())
                .unwrap_or_default();
            let mut mp = super::loader::KernelMapper::new(crate::mem::pfh::target_ops());
            let freed = super::loader::release_pages(&ledger, &mut mp);
            CHILD_PAGES.lock().retain(|e| e.0 != pid);
            crate::kinfo!(
                "ring3: supervisor wait → pid={} code={} pages_released={}/{} — 空间随进程消亡，一页不留",
                pid,
                code,
                freed,
                ledger.len()
            );
        }
        None => crate::kwarn!("ring3: supervisor wait found no zombie — lifecycle broken"),
    }
    if exits < LIFECYCLES {
        spawn_hello(exits + 1);
    }
    // 任务39（AI-B）：hello 轮次满后，装载并运行一轮静态 PE64 实例
    // （PE_DONE 只放行一次；PE 自己 exit 后回到本续体进压力探针）。
    if !PE_DONE.swap(true, Ordering::Relaxed) {
        spawn_pe();
    }
    pressure_probe_and_finish()
}

/// 装载 hello 并进入 ring3（任务15 版）：PCB 入表 → 共用装载引擎
/// （页账本登记进 CHILD_PAGES）→ 进入计划 → iretq。不返回（exit 经
/// resume_supervisor 交还控制权；失败路径如实 kwarn 后停机）。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
fn spawn_hello(instance: u32) -> ! {
    let img = match elf::parse(HELLO_ELF) {
        Ok(i) => i,
        Err(e) => {
            crate::kwarn!("ring3: hello.elf parse failed: {} — halting", e.as_str());
            halt_demo()
        }
    };
    let pid = match PROCS.lock().alloc(0, b"hello") {
        Ok(p) => p,
        Err(e) => {
            crate::kwarn!("ring3: proc alloc failed: {:?} — halting", e);
            halt_demo()
        }
    };
    DEMO_PID.store(pid, core::sync::atomic::Ordering::Relaxed);
    let mut mp = super::loader::KernelMapper::new(crate::mem::pfh::target_ops());
    let src = super::loader::ElfSource {
        img: &img,
        blob: HELLO_ELF,
    };
    let loaded = match super::loader::load_into(
        &src,
        &mut mp,
        entry::DEFAULT_STACK_PAGES as u64,
        entry::USER_STACK_TOP,
    ) {
        Ok(l) => l,
        Err(e) => {
            PROCS.lock().exit(pid, -1, 0);
            crate::kwarn!("ring3: load failed ({:?}) — halting", e);
            halt_demo()
        }
    };
    let pages_n = loaded.pages.len();
    let (entry_ip, stack_top) = (loaded.entry, loaded.stack_top);
    CHILD_PAGES.lock().push((pid, loaded.pages));
    crate::kinfo!(
        "ring3: instance#{} spawned pid={} entry={:#x} pages={} — PCB + 逐页账本登记",
        instance,
        pid,
        entry_ip,
        pages_n
    );
    // 进入计划（既有校验：用户半区 + 对齐 + rflags 硬性要求）。
    let mut plan = super::uspace::EntryPlan::default();
    if let Err(e) = plan_entry(EntryPath::Iret, entry_ip, stack_top, &mut plan) {
        crate::kwarn!("ring3: entry plan rejected: {} — halting", e);
        halt_demo()
    }
    crate::kinfo!(
        "ring3: iretq → user (rip={:#x} rsp={:#x} rflags={:#x})",
        plan.rip,
        plan.rsp,
        plan.rflags
    );
    // SAFETY: 依赖 loader 已映射的段页与栈页、已写入的 MSR 与 TSS.RSP0。
    unsafe { enter_user(plan.rip, plan.rsp, plan.rflags) }
}

/// 任务39（AI-B）· 装载静态 PE64 并进入 ring3：与 spawn_hello 走同一条
/// 监护链（PCB 入表 → 共用装载引擎 → 页账本登记 → iretq）。样例的
/// .text 是 write(1, "hello from PE"(NL), 14) + exit(0)——PE 运行证据与
/// hello.elf 同源（syscall 指令 + 双入口决策层），装载差异只在格式
/// 适配器（PeSource 按首选基址映射，无重定位——静态映像）。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
fn spawn_pe() -> ! {
    let img = match super::pe::parse(HELLO_PE) {
        Ok(i) => i,
        Err(e) => {
            crate::kwarn!("ring3: hello.pe parse failed: {} — halting", e.as_str());
            halt_demo()
        }
    };
    if img.has_imports() {
        // 任务39 边界：不做导入解析（任务40）。样例无导入；带导入的
        // 映像在此如实拒绝，绝不带着未解析 thunk 进 ring3。
        crate::kwarn!("ring3: PE has imports — import resolution is task 40 — halting");
        halt_demo()
    }
    let pid = match PROCS.lock().alloc(0, b"hellope") {
        Ok(p) => p,
        Err(e) => {
            crate::kwarn!("ring3: PE proc alloc failed: {:?} — halting", e);
            halt_demo()
        }
    };
    DEMO_PID.store(pid, core::sync::atomic::Ordering::Relaxed);
    let mut mp = super::loader::KernelMapper::new(crate::mem::pfh::target_ops());
    let src = super::pe::PeSource {
        img: &img,
        blob: HELLO_PE,
    };
    let loaded = match super::loader::load_into(
        &src,
        &mut mp,
        entry::DEFAULT_STACK_PAGES as u64,
        entry::USER_STACK_TOP,
    ) {
        Ok(l) => l,
        Err(e) => {
            PROCS.lock().exit(pid, -1, 0);
            crate::kwarn!("ring3: PE load failed ({:?}) — halting", e);
            halt_demo()
        }
    };
    let pages_n = loaded.pages.len();
    let (entry_ip, stack_top) = (loaded.entry, loaded.stack_top);
    CHILD_PAGES.lock().push((pid, loaded.pages));
    crate::kinfo!(
        "ring3: PE spawned pid={} entry={:#x} pages={} imports={} — 任务39 静态 PE 装载",
        pid,
        entry_ip,
        pages_n,
        img.has_imports()
    );
    let mut plan = super::uspace::EntryPlan::default();
    if let Err(e) = plan_entry(EntryPath::Iret, entry_ip, stack_top, &mut plan) {
        crate::kwarn!("ring3: PE entry plan rejected: {} — halting", e);
        halt_demo()
    }
    crate::kinfo!(
        "ring3: iretq → PE user (rip={:#x} rsp={:#x} rflags={:#x})",
        plan.rip,
        plan.rsp,
        plan.rflags
    );
    // SAFETY: 同 spawn_hello——loader 已映射段页/栈页，MSR/TSS 已就绪。
    unsafe { enter_user(plan.rip, plan.rsp, plan.rflags) }
}

/// 压力探针（总案任务15 验收：spawn 压力 64 槽打满/耗尽优雅拒绝）：
/// 唯一命名填满剩余槽 → 下一次 alloc 必须被优雅拒绝（exhausted 计数）→
/// 全量 exit+wait 退场回收 → 表清空（无泄漏）→ 证据汇总停机。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
fn pressure_probe_and_finish() -> ! {
    fn probe_name(i: u32) -> ([u8; 16], usize) {
        let mut name = *b"probe0000000000\0";
        let digits = [
            b'0' + ((i / 100) % 10) as u8,
            b'0' + ((i / 10) % 10) as u8,
            b'0' + (i % 10) as u8,
        ];
        name[5..8].copy_from_slice(&digits);
        (name, 8)
    }
    let mut filled: u32 = 0;
    let mut victims: alloc::vec::Vec<u32> = alloc::vec::Vec::new();
    loop {
        let (name, n) = probe_name(filled);
        // 守卫先释放再分支：match 临时守卫会活到整个 match 结束，
        // Err 臂里再 PROCS.lock() 就是自旋死锁（实机曾卡死在此）。
        let res = PROCS.lock().alloc(0, &name[..n]);
        match res {
            Ok(pid) => {
                victims.push(pid);
                filled += 1;
            }
            Err(e) => {
                let exhausted = PROCS.lock().exhausted;
                crate::kinfo!(
                    "ring3: pressure fill: +{} procs → next alloc rejected {:?} (exhausted={}) — 64 槽打满，优雅拒绝",
                    filled,
                    e,
                    exhausted
                );
                break;
            }
        }
        if filled > 4096 {
            crate::kwarn!("ring3: pressure fill runaway — aborting probe");
            break;
        }
    }
    for &pid in &victims {
        PROCS.lock().exit(pid, 0, 0);
    }
    let mut reaped = 0u32;
    while PROCS.lock().wait(0).is_some() {
        reaped += 1;
    }
    let live = PROCS.lock().live();
    crate::kinfo!(
        "ring3: pressure drain: exited={} reaped={} live={} — 表清空，无泄漏",
        victims.len(),
        reaped,
        live
    );
    crate::kinfo!(
        "ring3: task15 lifecycle complete — {} 轮 spawn/exit/wait + 压力探针全过 — halting",
        LIFECYCLES
    );
    halt_demo()
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

/// 监护生命周期入口（任务15）：spawn → 用户态执行 → exit 交还控制权 →
/// after_exit 收割并驱动下一实例/压力探针 → 停机。不返回。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn run_demo() -> ! {
    crate::kinfo!(
        "ring3: supervised lifecycle demo — {} 轮 spawn/exit/wait，随后压力探针",
        LIFECYCLES
    );
    spawn_hello(1)
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
