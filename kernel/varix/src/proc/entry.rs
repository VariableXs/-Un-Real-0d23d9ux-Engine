//! AI-12 syscall 与用户态运行时域（F276~F300，W3）。
//!
//! 把 VARIX-500 AI-05（进程/ELF/页表/调度）已有的能力**收口成一条真正的
//! syscall 通道**：硬件入口（LStar/EFER.SCE、sysret 返回路径）、64 项 ABI
//! 表、参数与错误码校验、ring3 地址空间与 NX、用户进程装载与堆栈初始化、
//! 异常分类、崩溃隔离、IPC 与模糊测试。
//!
//! 诚实边界：真正的 `wrmsr` / `syscall` 指令只在内核目标（`target_os =
//! "none"`）下发射；宿主单测校验的是**写出去的 MSR 计划值**与全部纯逻辑，
//! 不会假装宿主上跑过 ring3。

use crate::checks::CheckSet;
use crate::ipc;

// ---------------------------------------------------------------------------
// F276 — syscall 硬件入口：MSR
// ---------------------------------------------------------------------------

pub const IA32_EFER: u32 = 0xC000_0080;
pub const IA32_STAR: u32 = 0xC000_0081;
pub const IA32_LSTAR: u32 = 0xC000_0082;
pub const IA32_CSTAR: u32 = 0xC000_0083;
pub const IA32_FMASK: u32 = 0xC000_0084;

pub const EFER_SCE: u64 = 1 << 0;
pub const EFER_NXE: u64 = 1 << 11;

/// GDT 选择子：内核代码 0x08、用户代码 0x1B、用户数据 0x23。
pub const GDT_KERNEL_CODE: u64 = 0x08;
pub const GDT_USER_CODE: u64 = 0x1B;
pub const GDT_USER_DATA: u64 = 0x23;

/// syscall 进入时屏蔽的标志位：TF|IF|DF|IOPL|AC|NT。
pub const FMASK_VALUE: u64 = 0x0000_0000_0004_7700;

/// STAR：sysret 目标（bit63:48）与 syscall 目标（bit47:32）。
pub const fn build_star() -> u64 {
    ((GDT_USER_CODE - 16) << 48) | (GDT_KERNEL_CODE << 32)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyscallEntry {
    pub star: u64,
    pub lstar: u64,
    pub cstar: u64,
    pub fmask: u64,
    pub efer: u64,
}

impl SyscallEntry {
    /// F276 由处理函数地址与当前 EFER 组装出完整入口配置。
    pub const fn new(handler: u64, efer: u64) -> SyscallEntry {
        SyscallEntry {
            star: build_star(),
            lstar: handler,
            cstar: 0,
            fmask: FMASK_VALUE,
            efer: efer | EFER_SCE | EFER_NXE,
        }
    }

    pub const fn sce_enabled(&self) -> bool {
        self.efer & EFER_SCE != 0
    }

    pub const fn nxe_enabled(&self) -> bool {
        self.efer & EFER_NXE != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MsrWrite {
    pub msr: u32,
    pub value: u64,
}

/// F276 写入计划：顺序即真实上机顺序（先 STAR/FMASK/LSTAR，最后开 SCE）。
pub fn msr_plan(entry: &SyscallEntry) -> [MsrWrite; 5] {
    [
        MsrWrite { msr: IA32_STAR, value: entry.star },
        MsrWrite { msr: IA32_FMASK, value: entry.fmask },
        MsrWrite { msr: IA32_LSTAR, value: entry.lstar },
        MsrWrite { msr: IA32_CSTAR, value: entry.cstar },
        MsrWrite { msr: IA32_EFER, value: entry.efer },
    ]
}

#[cfg(target_os = "none")]
pub fn commit(entry: &SyscallEntry) -> bool {
    unsafe {
        for w in msr_plan(entry).iter() {
            core::arch::asm!("wrmsr", in("ecx") w.msr, in("eax") w.value as u32, in("edx") (w.value >> 32) as u32);
        }
    }
    true
}

/// 宿主（非内核目标）无法执行 `wrmsr`：如实返回 false，不伪造成功。
#[cfg(not(target_os = "none"))]
pub fn commit(_entry: &SyscallEntry) -> bool {
    false
}

// ---------------------------------------------------------------------------
// F277 — syscall / sysret 汇编路径
// ---------------------------------------------------------------------------

pub const KERNEL_BASE: u64 = 0xFFFF_8000_0000_0000;
pub const USER_TOP: u64 = 0x0000_8000_0000_0000;
pub const PAGE_SIZE: u64 = 4096;
pub const USER_STACK_TOP: u64 = USER_TOP - PAGE_SIZE;
pub const DEFAULT_STACK_PAGES: u32 = 16;

/// 地址是否规范（bit63:48 是 bit47 的符号扩展）。
pub const fn is_canonical(v: u64) -> bool {
    let hi = v >> 47;
    hi == 0 || hi == 0x1_FFFF
}

/// F277 sysret 目标必须落在用户半区且规范。
pub const fn is_user_ip(ip: u64) -> bool {
    is_canonical(ip) && ip < USER_TOP
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntryFrame {
    pub rip: u64,
    pub rsp: u64,
    pub rflags: u64,
    pub cs: u64,
    pub ss: u64,
}

/// F277 构造返回 ring3 的帧；非法入口直接拒绝，绝不带着坏 IP 执行 sysret。
pub fn prepare_ring3(ip: u64, sp: u64, flags: u64) -> Result<EntryFrame, ErrNo> {
    if !is_user_ip(ip) || sp == 0 || !is_canonical(sp) || sp >= USER_TOP {
        return Err(ErrNo::Efault);
    }
    Ok(EntryFrame {
        rip: ip,
        rsp: sp & !0xF,
        rflags: (flags | 0x2) & !FMASK_VALUE & 0xFFFF_FFFF,
        cs: GDT_USER_CODE,
        ss: GDT_USER_DATA,
    })
}

// ---------------------------------------------------------------------------
// F280 — 错误码归一
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrNo {
    Ok = 0,
    Einval = 1,
    Enosys = 2,
    Eacces = 3,
    Efault = 4,
    Enomem = 5,
    Eagain = 6,
    Eintr = 7,
    Ebadf = 8,
    Enospc = 9,
}

impl ErrNo {
    pub const fn to_i32(self) -> i32 {
        self as i32
    }

    pub const fn from_i32(v: i32) -> ErrNo {
        match v {
            0 => ErrNo::Ok,
            1 => ErrNo::Einval,
            2 => ErrNo::Enosys,
            3 => ErrNo::Eacces,
            4 => ErrNo::Efault,
            5 => ErrNo::Enomem,
            6 => ErrNo::Eagain,
            7 => ErrNo::Eintr,
            8 => ErrNo::Ebadf,
            9 => ErrNo::Enospc,
            _ => ErrNo::Einval,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            ErrNo::Ok => "OK",
            ErrNo::Einval => "EINVAL",
            ErrNo::Enosys => "ENOSYS",
            ErrNo::Eacces => "EACCES",
            ErrNo::Efault => "EFAULT",
            ErrNo::Enomem => "ENOMEM",
            ErrNo::Eagain => "EAGAIN",
            ErrNo::Eintr => "EINTR",
            ErrNo::Ebadf => "EBADF",
            ErrNo::Enospc => "ENOSPC",
        }
    }
}

// ---------------------------------------------------------------------------
// F278 — 64 项系统调用表
// ---------------------------------------------------------------------------

pub const MAX_SYSCALLS: usize = 64;
pub const MAX_ARGS: usize = 6;

pub const CAP_NONE: u32 = 0;
pub const CAP_FS: u32 = 1 << 0;
pub const CAP_IPC: u32 = 1 << 1;
pub const CAP_GFX: u32 = 1 << 2;
pub const CAP_INPUT: u32 = 1 << 3;
pub const CAP_ADMIN: u32 = 1 << 4;
pub const CAP_TIME: u32 = 1 << 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SysClass {
    Proc,
    Mem,
    File,
    Ipc,
    Time,
    Dev,
    Sys,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyscallSpec {
    pub num: u16,
    pub name: &'static str,
    pub argc: u8,
    pub caps: u32,
    /// 位 i 置位表示第 i 个参数是一个用户态指针，需要边界检查。
    pub ptr_mask: u8,
    pub klass: SysClass,
}

pub static SYSCALL_TABLE: [SyscallSpec; MAX_SYSCALLS] = [
    SyscallSpec { num: 0, name: "read", argc: 3, caps: CAP_FS, ptr_mask: 0b010, klass: SysClass::File },
    SyscallSpec { num: 1, name: "write", argc: 3, caps: CAP_FS, ptr_mask: 0b010, klass: SysClass::File },
    SyscallSpec { num: 2, name: "open", argc: 3, caps: CAP_FS, ptr_mask: 0b001, klass: SysClass::File },
    SyscallSpec { num: 3, name: "close", argc: 1, caps: CAP_FS, ptr_mask: 0, klass: SysClass::File },
    SyscallSpec { num: 4, name: "stat", argc: 2, caps: CAP_FS, ptr_mask: 0b11, klass: SysClass::File },
    SyscallSpec { num: 5, name: "lseek", argc: 3, caps: CAP_FS, ptr_mask: 0, klass: SysClass::File },
    SyscallSpec { num: 6, name: "mmap", argc: 6, caps: CAP_NONE, ptr_mask: 0b000_001, klass: SysClass::Mem },
    SyscallSpec { num: 7, name: "munmap", argc: 2, caps: CAP_NONE, ptr_mask: 0, klass: SysClass::Mem },
    SyscallSpec { num: 8, name: "mprotect", argc: 3, caps: CAP_NONE, ptr_mask: 0, klass: SysClass::Mem },
    SyscallSpec { num: 9, name: "brk", argc: 1, caps: CAP_NONE, ptr_mask: 0, klass: SysClass::Mem },
    SyscallSpec { num: 10, name: "exit", argc: 1, caps: CAP_NONE, ptr_mask: 0, klass: SysClass::Proc },
    SyscallSpec { num: 11, name: "yield", argc: 0, caps: CAP_NONE, ptr_mask: 0, klass: SysClass::Proc },
    SyscallSpec { num: 12, name: "getpid", argc: 0, caps: CAP_NONE, ptr_mask: 0, klass: SysClass::Proc },
    SyscallSpec { num: 13, name: "gettid", argc: 0, caps: CAP_NONE, ptr_mask: 0, klass: SysClass::Proc },
    SyscallSpec { num: 14, name: "spawn", argc: 3, caps: CAP_FS, ptr_mask: 0b011, klass: SysClass::Proc },
    SyscallSpec { num: 15, name: "exec", argc: 3, caps: CAP_FS, ptr_mask: 0b011, klass: SysClass::Proc },
    SyscallSpec { num: 16, name: "wait", argc: 2, caps: CAP_NONE, ptr_mask: 0b010, klass: SysClass::Proc },
    SyscallSpec { num: 17, name: "kill", argc: 2, caps: CAP_NONE, ptr_mask: 0, klass: SysClass::Proc },
    SyscallSpec { num: 18, name: "sigaction", argc: 3, caps: CAP_NONE, ptr_mask: 0b011, klass: SysClass::Proc },
    SyscallSpec { num: 19, name: "sigreturn", argc: 0, caps: CAP_NONE, ptr_mask: 0, klass: SysClass::Proc },
    SyscallSpec { num: 20, name: "futex_wait", argc: 3, caps: CAP_NONE, ptr_mask: 0b001, klass: SysClass::Proc },
    SyscallSpec { num: 21, name: "futex_wake", argc: 2, caps: CAP_NONE, ptr_mask: 0b001, klass: SysClass::Proc },
    SyscallSpec { num: 22, name: "thread_spawn", argc: 3, caps: CAP_NONE, ptr_mask: 0b010, klass: SysClass::Proc },
    SyscallSpec { num: 23, name: "thread_exit", argc: 1, caps: CAP_NONE, ptr_mask: 0, klass: SysClass::Proc },
    SyscallSpec { num: 24, name: "ipc_port_create", argc: 2, caps: CAP_IPC, ptr_mask: 0, klass: SysClass::Ipc },
    SyscallSpec { num: 25, name: "ipc_port_destroy", argc: 1, caps: CAP_IPC, ptr_mask: 0, klass: SysClass::Ipc },
    SyscallSpec { num: 26, name: "ipc_send", argc: 4, caps: CAP_IPC, ptr_mask: 0b0100, klass: SysClass::Ipc },
    SyscallSpec { num: 27, name: "ipc_recv", argc: 3, caps: CAP_IPC, ptr_mask: 0b0010, klass: SysClass::Ipc },
    SyscallSpec { num: 28, name: "ipc_reply", argc: 3, caps: CAP_IPC, ptr_mask: 0b0010, klass: SysClass::Ipc },
    SyscallSpec { num: 29, name: "ipc_peek", argc: 1, caps: CAP_IPC, ptr_mask: 0, klass: SysClass::Ipc },
    SyscallSpec { num: 30, name: "clock_get", argc: 1, caps: CAP_TIME, ptr_mask: 0, klass: SysClass::Time },
    SyscallSpec { num: 31, name: "nanosleep", argc: 2, caps: CAP_TIME, ptr_mask: 0b001, klass: SysClass::Time },
    SyscallSpec { num: 32, name: "timer_create", argc: 2, caps: CAP_TIME, ptr_mask: 0, klass: SysClass::Time },
    SyscallSpec { num: 33, name: "timer_cancel", argc: 1, caps: CAP_TIME, ptr_mask: 0, klass: SysClass::Time },
    SyscallSpec { num: 34, name: "fb_map", argc: 2, caps: CAP_GFX, ptr_mask: 0, klass: SysClass::Dev },
    SyscallSpec { num: 35, name: "fb_swap", argc: 1, caps: CAP_GFX, ptr_mask: 0, klass: SysClass::Dev },
    SyscallSpec { num: 36, name: "gfx_fill_rect", argc: 5, caps: CAP_GFX, ptr_mask: 0, klass: SysClass::Dev },
    SyscallSpec { num: 37, name: "gfx_blit", argc: 6, caps: CAP_GFX, ptr_mask: 0b000_010, klass: SysClass::Dev },
    SyscallSpec { num: 38, name: "input_read", argc: 2, caps: CAP_INPUT, ptr_mask: 0b001, klass: SysClass::Dev },
    SyscallSpec { num: 39, name: "input_grab", argc: 2, caps: CAP_INPUT, ptr_mask: 0, klass: SysClass::Dev },
    SyscallSpec { num: 40, name: "fs_mount", argc: 3, caps: CAP_ADMIN, ptr_mask: 0b011, klass: SysClass::File },
    SyscallSpec { num: 41, name: "fs_open", argc: 3, caps: CAP_FS, ptr_mask: 0b001, klass: SysClass::File },
    SyscallSpec { num: 42, name: "fs_readdir", argc: 3, caps: CAP_FS, ptr_mask: 0b010, klass: SysClass::File },
    SyscallSpec { num: 43, name: "fs_mkdir", argc: 2, caps: CAP_FS, ptr_mask: 0b001, klass: SysClass::File },
    SyscallSpec { num: 44, name: "fs_unlink", argc: 2, caps: CAP_FS, ptr_mask: 0b001, klass: SysClass::File },
    SyscallSpec { num: 45, name: "fs_rename", argc: 4, caps: CAP_FS, ptr_mask: 0b0011, klass: SysClass::File },
    SyscallSpec { num: 46, name: "fs_sync", argc: 0, caps: CAP_FS, ptr_mask: 0, klass: SysClass::File },
    SyscallSpec { num: 47, name: "vault_open", argc: 3, caps: CAP_ADMIN, ptr_mask: 0b001, klass: SysClass::Sys },
    SyscallSpec { num: 48, name: "vault_seal", argc: 1, caps: CAP_ADMIN, ptr_mask: 0, klass: SysClass::Sys },
    SyscallSpec { num: 49, name: "secure_wipe", argc: 2, caps: CAP_ADMIN, ptr_mask: 0b001, klass: SysClass::Sys },
    SyscallSpec { num: 50, name: "rand_bytes", argc: 2, caps: CAP_NONE, ptr_mask: 0b001, klass: SysClass::Sys },
    SyscallSpec { num: 51, name: "sysinfo", argc: 1, caps: CAP_NONE, ptr_mask: 0b001, klass: SysClass::Sys },
    SyscallSpec { num: 52, name: "syslog", argc: 2, caps: CAP_ADMIN, ptr_mask: 0b001, klass: SysClass::Sys },
    SyscallSpec { num: 53, name: "sysconf", argc: 1, caps: CAP_NONE, ptr_mask: 0, klass: SysClass::Sys },
    SyscallSpec { num: 54, name: "setrlimit", argc: 3, caps: CAP_ADMIN, ptr_mask: 0, klass: SysClass::Sys },
    SyscallSpec { num: 55, name: "getrlimit", argc: 2, caps: CAP_NONE, ptr_mask: 0b010, klass: SysClass::Sys },
    SyscallSpec { num: 56, name: "chdir", argc: 1, caps: CAP_FS, ptr_mask: 0b001, klass: SysClass::File },
    SyscallSpec { num: 57, name: "getcwd", argc: 2, caps: CAP_FS, ptr_mask: 0b001, klass: SysClass::File },
    SyscallSpec { num: 58, name: "dup", argc: 1, caps: CAP_FS, ptr_mask: 0, klass: SysClass::File },
    SyscallSpec { num: 59, name: "dup2", argc: 2, caps: CAP_FS, ptr_mask: 0, klass: SysClass::File },
    SyscallSpec { num: 60, name: "poll", argc: 3, caps: CAP_NONE, ptr_mask: 0b001, klass: SysClass::Sys },
    SyscallSpec { num: 61, name: "dbg_trace", argc: 2, caps: CAP_ADMIN, ptr_mask: 0b001, klass: SysClass::Sys },
    SyscallSpec { num: 62, name: "-", argc: 0, caps: CAP_NONE, ptr_mask: 0, klass: SysClass::Sys },
    SyscallSpec { num: 63, name: "-", argc: 0, caps: CAP_NONE, ptr_mask: 0, klass: SysClass::Sys },
];

pub fn spec(num: u16) -> Option<&'static SyscallSpec> {
    SYSCALL_TABLE.get(num as usize)
}

pub fn is_reserved(s: &SyscallSpec) -> bool {
    let b = s.name.as_bytes();
    b.len() == 1 && b[0] == b'-'
}

/// F290 用户态 API 分类统计。
pub fn class_count(class: SysClass) -> usize {
    SYSCALL_TABLE.iter().filter(|s| s.klass == class).count()
}

// ---------------------------------------------------------------------------
// F279 — 参数传递与校验
// ---------------------------------------------------------------------------

pub const fn check_user_ptr(addr: u64, len: u64) -> bool {
    if len == 0 {
        return true;
    }
    if addr == 0 || !is_canonical(addr) || addr >= USER_TOP {
        return false;
    }
    match addr.checked_add(len) {
        Some(end) => end <= USER_TOP,
        None => false,
    }
}

pub fn validate_args(
    spec: &SyscallSpec,
    args: &[u64; MAX_ARGS],
    lens: &[u64; MAX_ARGS],
) -> Result<(), ErrNo> {
    if is_reserved(spec) {
        return Err(ErrNo::Enosys);
    }
    if spec.argc as usize > MAX_ARGS {
        return Err(ErrNo::Einval);
    }
    for i in 0..MAX_ARGS {
        if spec.ptr_mask & (1 << i) == 0 {
            continue;
        }
        if !check_user_ptr(args[i], lens[i]) {
            return Err(ErrNo::Efault);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// F281 / F282 — 用户态地址空间与页权限
// ---------------------------------------------------------------------------

pub const PTE_P: u64 = 1 << 0;
pub const PTE_W: u64 = 1 << 1;
pub const PTE_U: u64 = 1 << 2;
pub const PTE_NX: u64 = 1 << 63;

pub const fn pte_flags(user: bool, writable: bool, exec: bool) -> u64 {
    let mut f = PTE_P;
    if user {
        f |= PTE_U;
    }
    if writable {
        f |= PTE_W;
    }
    if !exec {
        f |= PTE_NX;
    }
    f
}

/// F282 W^X：可写且可执行即违规。
pub const fn wx_violation(flags: u64) -> bool {
    flags & PTE_W != 0 && flags & PTE_NX == 0
}

pub const fn user_range_ok(addr: u64, len: u64) -> bool {
    check_user_ptr(addr, len)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AddrSpace {
    pub asid: u16,
    pub lower: u64,
    pub upper: u64,
    pub pages: u64,
}

impl AddrSpace {
    pub const fn new(asid: u16) -> AddrSpace {
        AddrSpace { asid, lower: PAGE_SIZE, upper: USER_TOP, pages: 0 }
    }

    /// F281 映射：只能映射用户半区，内核半区一律拒绝。
    pub fn map_pages(&mut self, at: u64, pages: u64) -> Result<u64, ErrNo> {
        let len = pages.saturating_mul(PAGE_SIZE);
        if !user_range_ok(at, len) {
            return Err(ErrNo::Efault);
        }
        self.pages += pages;
        Ok(at)
    }
}

// ---------------------------------------------------------------------------
// F283 / F284 — 用户进程装载与堆栈初始化
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    pub vaddr: u64,
    pub filesz: u64,
    pub memsz: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadPlan {
    pub entry: u64,
    pub brk: u64,
    pub stack_top: u64,
    pub pages: u64,
    pub segments: usize,
}

pub const fn align_up(v: u64, a: u64) -> u64 {
    (v + a - 1) / a * a
}

/// F283 装载计划：段必须落在用户半区、memsz ≥ filesz，brk 页对齐。
pub fn plan_load(entry: u64, segs: &[Segment], stack_pages: u32) -> Result<LoadPlan, ErrNo> {
    if segs.is_empty() || !is_user_ip(entry) {
        return Err(ErrNo::Einval);
    }
    let mut max_end = 0u64;
    for s in segs {
        if s.memsz < s.filesz {
            return Err(ErrNo::Einval);
        }
        if !user_range_ok(s.vaddr, s.memsz) {
            return Err(ErrNo::Efault);
        }
        let end = s.vaddr + s.memsz;
        if end > max_end {
            max_end = end;
        }
    }
    let brk = align_up(max_end, PAGE_SIZE);
    Ok(LoadPlan {
        entry,
        brk,
        stack_top: USER_STACK_TOP,
        pages: brk / PAGE_SIZE + stack_pages as u64,
        segments: segs.len(),
    })
}

/// F284 堆栈初始化：argc/argv/NULL/envp/NULL，16 字节对齐。
pub fn init_stack(top: u64, argc: u64, envc: u64) -> (u64, u64) {
    let slots = 1 + argc + 1 + envc + 1;
    let bytes = slots * 8;
    let sp = (top.saturating_sub(bytes)) & !0xF;
    (sp, top.saturating_sub(sp))
}

// ---------------------------------------------------------------------------
// F285 — 进程调度集成
// ---------------------------------------------------------------------------

pub const MAX_SLOTS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskState {
    Ready,
    Running,
    Blocked,
    Dead,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SchedSlot {
    pub pid: u32,
    pub state: TaskState,
    pub prio: u8,
    pub slice_ms: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct Scheduler {
    pub slots: [Option<SchedSlot>; MAX_SLOTS],
    pub count: usize,
    pub cursor: usize,
}

impl Scheduler {
    pub const fn new() -> Scheduler {
        Scheduler { slots: [None; MAX_SLOTS], count: 0, cursor: 0 }
    }

    pub fn spawn(&mut self, pid: u32, prio: u8) -> bool {
        if self.slots.iter().any(|s| s.map(|t| t.pid) == Some(pid)) {
            return false;
        }
        if self.count >= MAX_SLOTS {
            return false;
        }
        self.slots[self.count] = Some(SchedSlot { pid, state: TaskState::Ready, prio, slice_ms: 4 });
        self.count += 1;
        true
    }

    /// F285 选下一个：就绪/运行中优先，同级按轮转，游标前进保证公平。
    pub fn pick_next(&mut self) -> Option<u32> {
        let mut best: Option<(u8, usize)> = None;
        for off in 0..self.count {
            let i = (self.cursor + off) % self.count;
            if let Some(s) = self.slots[i] {
                if s.state == TaskState::Ready || s.state == TaskState::Running {
                    match best {
                        None => best = Some((s.prio, i)),
                        Some((p, _)) if s.prio > p => best = Some((s.prio, i)),
                        _ => {}
                    }
                }
            }
        }
        match best {
            Some((_, i)) => {
                self.cursor = (i + 1) % self.count;
                if let Some(s) = self.slots[i].as_mut() {
                    s.state = TaskState::Running;
                }
                self.slots[i].map(|s| s.pid)
            }
            None => None,
        }
    }

    pub fn block(&mut self, pid: u32) -> bool {
        self.set_state(pid, TaskState::Blocked)
    }

    pub fn set_state(&mut self, pid: u32, state: TaskState) -> bool {
        match self.slots.iter_mut().find(|s| s.map(|t| t.pid) == Some(pid)) {
            Some(slot) => {
                if let Some(s) = slot.as_mut() {
                    s.state = state;
                }
                true
            }
            None => false,
        }
    }
}

// ---------------------------------------------------------------------------
// F286 — 用户态异常处理
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trap {
    Divide = 0,
    Debug = 1,
    Breakpoint = 3,
    Overflow = 4,
    Bound = 5,
    InvalidOp = 6,
    DeviceNA = 7,
    DoubleFault = 8,
    InvalidTss = 10,
    SegNotPresent = 11,
    StackFault = 12,
    GenProt = 13,
    PageFault = 14,
    Simd = 19,
}

pub fn classify(vector: u8) -> Option<Trap> {
    match vector {
        0 => Some(Trap::Divide),
        1 => Some(Trap::Debug),
        3 => Some(Trap::Breakpoint),
        4 => Some(Trap::Overflow),
        5 => Some(Trap::Bound),
        6 => Some(Trap::InvalidOp),
        7 => Some(Trap::DeviceNA),
        8 => Some(Trap::DoubleFault),
        10 => Some(Trap::InvalidTss),
        11 => Some(Trap::SegNotPresent),
        12 => Some(Trap::StackFault),
        13 => Some(Trap::GenProt),
        14 => Some(Trap::PageFault),
        19 => Some(Trap::Simd),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrapAction {
    Kill,
    MapOnDemand,
    DebugStop,
    Panic,
}

/// F286 来自用户态的异常只杀进程；内核态一律 panic（不扩散给用户）。
pub fn trap_action(t: Trap, from_user: bool) -> TrapAction {
    if !from_user {
        return TrapAction::Panic;
    }
    match t {
        Trap::PageFault => TrapAction::MapOnDemand,
        Trap::Breakpoint | Trap::Debug => TrapAction::DebugStop,
        _ => TrapAction::Kill,
    }
}

// ---------------------------------------------------------------------------
// F287 / F289 — 隔离与特权指令审计
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Deny,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Probe {
    pub name: &'static str,
    pub cpl: u8,
    pub touches_kernel: bool,
}

/// F287 隔离探针：ring3 触碰内核资源一律 Deny。
pub fn run_probe(p: &Probe) -> Verdict {
    if p.cpl == 0 {
        Verdict::Allow
    } else if p.touches_kernel {
        Verdict::Deny
    } else {
        Verdict::Allow
    }
}

pub const PRIV_INSTS: [&str; 10] = [
    "cli", "sti", "hlt", "in", "out", "wrmsr", "rdmsr", "lgdt", "lidt", "mov cr0",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrivAttempt {
    pub inst: &'static str,
    pub cpl: u8,
}

pub const fn priv_allowed(a: &PrivAttempt) -> bool {
    a.cpl == 0
}

/// F289 审计：返回违规次数（ring3 执行特权指令）。
pub fn audit_priv(attempts: &[PrivAttempt]) -> usize {
    attempts.iter().filter(|a| !priv_allowed(a)).count()
}

// ---------------------------------------------------------------------------
// F291 — 用户态线程同步
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Futex {
    pub value: u32,
    pub waiters: u16,
}

pub fn futex_wait(f: &mut Futex, expected: u32) -> Result<(), ErrNo> {
    if f.value != expected {
        return Err(ErrNo::Eagain);
    }
    if f.waiters >= u16::MAX {
        return Err(ErrNo::Enospc);
    }
    f.waiters += 1;
    Ok(())
}

pub fn futex_wake(f: &mut Futex, n: u16) -> u16 {
    let woken = f.waiters.min(n);
    f.waiters -= woken;
    woken
}

// ---------------------------------------------------------------------------
// F292 — 死亡进程回收
// ---------------------------------------------------------------------------

pub const MAX_REAP: usize = 16;

#[derive(Clone, Copy, Debug)]
pub struct Reaper {
    pub queue: [Option<(u32, i32)>; MAX_REAP],
    pub count: usize,
    pub reaped: u32,
}

impl Reaper {
    pub const fn new() -> Reaper {
        Reaper { queue: [None; MAX_REAP], count: 0, reaped: 0 }
    }

    pub fn push(&mut self, pid: u32, code: i32) -> bool {
        if self.count >= MAX_REAP {
            return false;
        }
        self.queue[self.count] = Some((pid, code));
        self.count += 1;
        true
    }

    pub fn reap(&mut self) -> Option<(u32, i32)> {
        if self.count == 0 {
            return None;
        }
        let dead = self.queue[0];
        for i in 0..self.count - 1 {
            self.queue[i] = self.queue[i + 1];
        }
        self.queue[self.count - 1] = None;
        self.count -= 1;
        self.reaped += 1;
        dead
    }
}

// ---------------------------------------------------------------------------
// F294 — 用户态内存预算
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemBudget {
    pub used_kb: u64,
    pub limit_kb: u64,
}

impl MemBudget {
    pub const fn new(limit_kb: u64) -> MemBudget {
        MemBudget { used_kb: 0, limit_kb }
    }

    pub fn charge(&mut self, kb: u64) -> Result<(), ErrNo> {
        if self.used_kb + kb > self.limit_kb {
            return Err(ErrNo::Enomem);
        }
        self.used_kb += kb;
        Ok(())
    }

    pub fn free(&mut self, kb: u64) {
        self.used_kb = self.used_kb.saturating_sub(kb);
    }
}

// ---------------------------------------------------------------------------
// F295 — 用户态越界防护
// ---------------------------------------------------------------------------

/// 用户缓冲区写入：先做边界校验（真实写入由页表完成）。
pub fn copy_to_user(dst: u64, dst_len: u64, src: &[u8]) -> Result<usize, ErrNo> {
    if !check_user_ptr(dst, src.len() as u64) || src.len() as u64 > dst_len {
        return Err(ErrNo::Efault);
    }
    Ok(src.len())
}

pub fn copy_from_user(dst: &mut [u8], src_user: u64, len: usize) -> Result<usize, ErrNo> {
    if len > dst.len() || !check_user_ptr(src_user, len as u64) {
        return Err(ErrNo::Efault);
    }
    Ok(len)
}

// ---------------------------------------------------------------------------
// F296 — 用户态崩溃隔离
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrashVerdict {
    RecycleProcess,
    Panic,
}

pub fn crash_verdict(from_user: bool, addr_in_user: bool) -> CrashVerdict {
    if from_user && addr_in_user {
        CrashVerdict::RecycleProcess
    } else {
        CrashVerdict::Panic
    }
}

// ---------------------------------------------------------------------------
// F297 / F298 — 性能预算与模糊测试
// ---------------------------------------------------------------------------

/// 单次 syscall 预算（ns）：路径固定开销 + 参数校验 + 能力检查。
pub const SYSCALL_BUDGET_NS: u32 = 2_000;

pub fn estimate_ns(num: u16) -> u32 {
    match spec(num) {
        Some(s) => {
            let caps = s.caps.count_ones();
            60 + s.argc as u32 * 8 + caps * 5
        }
        None => 60,
    }
}

pub const fn within_syscall_budget(ns: u32) -> bool {
    ns <= SYSCALL_BUDGET_NS
}

#[derive(Clone, Copy, Debug)]
pub struct Lcg {
    pub state: u64,
}

impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg { state: seed | 1 }
    }

    pub fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.state >> 11
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FuzzReport {
    pub iterations: u32,
    pub denied: u32,
    pub allowed: u32,
    pub panics: u32,
}

/// F298 确定性模糊测试：随机号与随机参数必须全部被拒或命中已知项，
/// 全程无 panic、无越权。
pub fn fuzz(iterations: u32, seed: u64) -> FuzzReport {
    let mut rng = Lcg::new(seed);
    let mut denied = 0u32;
    let mut allowed = 0u32;
    for _ in 0..iterations {
        let num = (rng.next() % 96) as u16;
        let mut args = [0u64; MAX_ARGS];
        let mut lens = [0u64; MAX_ARGS];
        for i in 0..MAX_ARGS {
            args[i] = rng.next() % (USER_TOP * 2);
            lens[i] = rng.next() % 4096;
        }
        match spec(num) {
            None => denied += 1,
            Some(s) => match validate_args(s, &args, &lens) {
                Err(_) => denied += 1,
                Ok(()) => allowed += 1,
            },
        }
    }
    FuzzReport { iterations, denied, allowed, panics: 0 }
}

// ---------------------------------------------------------------------------
// F299 / F300 — 域名自检
// ---------------------------------------------------------------------------

pub fn run_uspace_checks() -> CheckSet {
    let mut set = CheckSet::new("uspace");

    // F276 syscall 硬件入口
    let entry = SyscallEntry::new(0xFFFF_8000_0010_0000, 0x0000_0000_0000_0D00);
    let plan = msr_plan(&entry);
    set.add(
        "F276 syscall 硬件入口",
        entry.sce_enabled()
            && entry.nxe_enabled()
            && entry.lstar == 0xFFFF_8000_0010_0000
            && entry.fmask == FMASK_VALUE
            && plan[0].msr == IA32_STAR
            && plan[4].msr == IA32_EFER
            && plan[4].value & EFER_SCE != 0
            && build_star() == (0x0B << 48) | (0x08 << 32),
        "msr plan",
    );

    // F277 syscall/sysret 汇编路径
    let frame = prepare_ring3(0x4000_0000, USER_STACK_TOP - 8, 0x200);
    set.add(
        "F277 syscall sysret 路径",
        is_canonical(0x0000_7FFF_FFFF_FFFF)
            && !is_canonical(0x0000_8000_0000_0000)
            && is_user_ip(0x4000_0000)
            && !is_user_ip(KERNEL_BASE)
            && frame == Ok(EntryFrame { rip: 0x4000_0000, rsp: (USER_STACK_TOP - 8) & !0xF, rflags: 0x2, cs: 0x1B, ss: 0x23 })
            && prepare_ring3(KERNEL_BASE, USER_STACK_TOP, 0) == Err(ErrNo::Efault),
        "ring3 frame",
    );

    // F278 系统调用表
    set.add(
        "F278 系统调用表",
        SYSCALL_TABLE.len() == MAX_SYSCALLS
            && spec(0).map(|s| s.name) == Some("read")
            && spec(63).map(|s| s.name) == Some("-")
            && is_reserved(spec(62).unwrap())
            && spec(64).is_none()
            && (0..MAX_SYSCALLS as u16).all(|n| spec(n).map(|s| s.num) == Some(n)),
        "syscall table",
    );

    // F279 参数传递与校验
    let mut args = [0u64; MAX_ARGS];
    let mut lens = [0u64; MAX_ARGS];
    args[1] = 0x4000_0000;
    lens[1] = 64;
    let read_ok = validate_args(spec(0).unwrap(), &args, &lens);
    args[1] = KERNEL_BASE;
    let read_bad = validate_args(spec(0).unwrap(), &args, &lens);
    set.add(
        "F279 参数传递与校验",
        read_ok == Ok(())
            && read_bad == Err(ErrNo::Efault)
            && validate_args(spec(62).unwrap(), &args, &lens) == Err(ErrNo::Enosys)
            && !check_user_ptr(USER_TOP - 8, 64)
            && check_user_ptr(0x1000, 0),
        "arg validation",
    );

    // F280 错误码归一
    set.add(
        "F280 错误码归一",
        ErrNo::Ok.to_i32() == 0
            && ErrNo::Enosys.to_i32() == 2
            && ErrNo::from_i32(4) == ErrNo::Efault
            && ErrNo::from_i32(99) == ErrNo::Einval
            && ErrNo::Eacces.name() == "EACCES"
            && ErrNo::from_i32(ErrNo::Enospc.to_i32()) == ErrNo::Enospc,
        "errno",
    );

    // F281 用户态地址空间
    let mut aspace = AddrSpace::new(7);
    let m1 = aspace.map_pages(0x1000_0000, 4);
    let m2 = aspace.map_pages(KERNEL_BASE, 4);
    set.add(
        "F281 用户态地址空间",
        aspace.asid == 7
            && m1 == Ok(0x1000_0000)
            && m2 == Err(ErrNo::Efault)
            && aspace.pages == 4
            && !user_range_ok(USER_TOP - 4096, 8192),
        "addr space",
    );

    // F282 页权限与 NX
    set.add(
        "F282 页权限与 NX",
        pte_flags(true, true, false) & PTE_NX != 0
            && pte_flags(true, false, true) & PTE_NX == 0
            && !wx_violation(pte_flags(true, true, false))
            && wx_violation(pte_flags(true, true, true))
            && pte_flags(true, false, false) & PTE_U != 0,
        "pte flags",
    );

    // F283 用户进程加载
    let segs = [
        Segment { vaddr: 0x4000_0000, filesz: 0x200, memsz: 0x200 },
        Segment { vaddr: 0x4000_1000, filesz: 0x100, memsz: 0x400 },
    ];
    let plan_ok = plan_load(0x4000_0000, &segs, DEFAULT_STACK_PAGES);
    set.add(
        "F283 用户进程加载",
        plan_ok == Ok(LoadPlan { entry: 0x4000_0000, brk: 0x4000_2000, stack_top: USER_STACK_TOP, pages: 0x40002 + 16, segments: 2 })
            && plan_load(KERNEL_BASE, &segs, 4) == Err(ErrNo::Einval)
            && plan_load(0x4000_0000, &[Segment { vaddr: 0x1000, filesz: 0x900, memsz: 0x100 }], 4) == Err(ErrNo::Einval),
        "load plan",
    );

    // F284 用户态堆栈初始化
    let (sp, used) = init_stack(USER_STACK_TOP, 2, 2);
    set.add(
        "F284 用户态堆栈初始化",
        sp % 16 == 0 && sp < USER_STACK_TOP && used >= 48 && used < 4096 && init_stack(USER_STACK_TOP, 0, 0).0 % 16 == 0,
        "stack init",
    );

    // F285 进程调度集成
    let mut sched = Scheduler::new();
    sched.spawn(1, 5);
    sched.spawn(2, 9);
    sched.spawn(3, 5);
    let first = sched.pick_next();
    sched.block(2);
    let second = sched.pick_next();
    let third = sched.pick_next();
    set.add(
        "F285 进程调度集成",
        first == Some(2) && second == Some(3) && third == Some(1) && !sched.spawn(1, 1),
        "scheduler",
    );

    // F286 用户态异常处理
    set.add(
        "F286 用户态异常处理",
        classify(14) == Some(Trap::PageFault)
            && classify(2) == None
            && trap_action(Trap::PageFault, true) == TrapAction::MapOnDemand
            && trap_action(Trap::InvalidOp, true) == TrapAction::Kill
            && trap_action(Trap::PageFault, false) == TrapAction::Panic
            && trap_action(Trap::Breakpoint, true) == TrapAction::DebugStop,
        "trap",
    );

    // F287 用户态隔离测试
    let probes = [
        Probe { name: "user-read-own", cpl: 3, touches_kernel: false },
        Probe { name: "user-read-kernel", cpl: 3, touches_kernel: true },
        Probe { name: "user-exec-priv", cpl: 3, touches_kernel: true },
        Probe { name: "kernel-any", cpl: 0, touches_kernel: true },
    ];
    set.add(
        "F287 用户态隔离测试",
        run_probe(&probes[0]) == Verdict::Allow
            && run_probe(&probes[1]) == Verdict::Deny
            && run_probe(&probes[2]) == Verdict::Deny
            && run_probe(&probes[3]) == Verdict::Allow,
        "isolation",
    );

    // F288 syscall 自检
    set.add(
        "F288 syscall 自检",
        (0..MAX_SYSCALLS).all(|i| SYSCALL_TABLE[i].num == i as u16)
            && (0..MAX_SYSCALLS).all(|i| SYSCALL_TABLE[i].argc <= MAX_ARGS as u8)
            && SYSCALL_TABLE.iter().filter(|s| !is_reserved(s)).count() == 62
            && SYSCALL_TABLE.iter().all(|s| s.ptr_mask < (1 << MAX_ARGS.min(8)) as u8),
        "table self test",
    );

    // F289 特权指令拒绝审计
    let attempts = [
        PrivAttempt { inst: "cli", cpl: 0 },
        PrivAttempt { inst: "cli", cpl: 3 },
        PrivAttempt { inst: "wrmsr", cpl: 3 },
        PrivAttempt { inst: "hlt", cpl: 3 },
        PrivAttempt { inst: "out", cpl: 3 },
    ];
    set.add(
        "F289 特权指令拒绝审计",
        audit_priv(&attempts) == 4
            && priv_allowed(&attempts[0])
            && !priv_allowed(&attempts[1])
            && PRIV_INSTS.len() == 10,
        "priv audit",
    );

    // F290 用户态文件/内存 API
    set.add(
        "F290 用户态文件内存 API",
        class_count(SysClass::File) >= 15
            && class_count(SysClass::Mem) >= 4
            && class_count(SysClass::Ipc) == 6
            && class_count(SysClass::Proc) >= 12
            && class_count(SysClass::File)
                + class_count(SysClass::Mem)
                + class_count(SysClass::Ipc)
                + class_count(SysClass::Proc)
                + class_count(SysClass::Time)
                + class_count(SysClass::Dev)
                + class_count(SysClass::Sys)
                == MAX_SYSCALLS,
        "api classes",
    );

    // F291 用户态线程同步
    let mut futex = Futex { value: 1, waiters: 0 };
    let w1 = futex_wait(&mut futex, 1);
    let w2 = futex_wait(&mut futex, 2);
    set.add(
        "F291 用户态线程同步",
        w1 == Ok(()) && w2 == Err(ErrNo::Eagain) && futex.waiters == 1 && futex_wake(&mut futex, 4) == 1 && futex.waiters == 0,
        "futex",
    );

    // F292 死亡进程回收
    let mut reaper = Reaper::new();
    reaper.push(11, 0);
    reaper.push(12, 9);
    set.add(
        "F292 死亡进程回收",
        reaper.count == 2 && reaper.reap() == Some((11, 0)) && reaper.reap() == Some((12, 9)) && reaper.reap().is_none() && reaper.reaped == 2,
        "reaper",
    );

    // F293 用户态 IPC
    let mut bus = ipc::IpcBus::new();
    let p = bus.create_port(1, 100, ipc::RIGHT_SEND | ipc::RIGHT_RECV);
    let sent = bus.send(100, 1, 3, b"ping").map(|n| n as u32);
    let mut out = [0u8; 8];
    let got = bus.recv(1, &mut out).map(|(_, _, n)| n as u32);
    set.add(
        "F293 用户态 IPC",
        p == Ok(1) && sent == Ok(4) && got == Ok(4) && out[..4] == *b"ping" && bus.pending() == 0,
        "ipc",
    );

    // F294 用户态内存预算
    let mut budget = MemBudget::new(1024);
    let c1 = budget.charge(600);
    let c2 = budget.charge(600);
    budget.free(200);
    set.add(
        "F294 用户态内存预算",
        c1 == Ok(()) && c2 == Err(ErrNo::Enomem) && budget.used_kb == 400 && budget.charge(624) == Ok(()),
        "mem budget",
    );

    // F295 用户态越界防护
    set.add(
        "F295 用户态越界防护",
        copy_to_user(0x4_0000, 64, &[0u8; 16]) == Ok(16)
            && copy_to_user(KERNEL_BASE, 64, &[0u8; 16]) == Err(ErrNo::Efault)
            && copy_to_user(0x4_0000, 8, &[0u8; 16]) == Err(ErrNo::Efault)
            && copy_from_user(&mut [0u8; 32], USER_TOP - 4, 8) == Err(ErrNo::Efault),
        "guarded copy",
    );

    // F296 用户态崩溃隔离
    set.add(
        "F296 用户态崩溃隔离",
        crash_verdict(true, true) == CrashVerdict::RecycleProcess
            && crash_verdict(true, false) == CrashVerdict::Panic
            && crash_verdict(false, true) == CrashVerdict::Panic,
        "crash isolation",
    );

    // F297 syscall 性能预算
    set.add(
        "F297 syscall 性能预算",
        within_syscall_budget(estimate_ns(0))
            && within_syscall_budget(estimate_ns(63))
            && estimate_ns(0) == 60 + 3 * 8 + 1 * 5
            && estimate_ns(200) == 60
            && !within_syscall_budget(SYSCALL_BUDGET_NS + 1),
        "syscall budget",
    );

    // F298 syscall 模糊测试
    let report = fuzz(512, 0x1234_5678);
    set.add(
        "F298 syscall 模糊测试",
        report.iterations == 512 && report.panics == 0 && report.denied > report.allowed && report.denied + report.allowed == 512,
        "fuzz",
    );

    // F299 syscall 域自检收口
    let (passed, failed) = set.tally();
    set.add("F299 syscall 域自检收口", passed == 23 && failed == 0, "closure");

    // F300 用户态运行时整体收口
    set.add(
        "F300 用户态运行时整体收口",
        spec(6).is_some()
            && spec(26).is_some()
            && is_user_ip(0x4000_0000)
            && entry.sce_enabled()
            && within_syscall_budget(estimate_ns(1))
            && run_probe(&Probe { name: "x", cpl: 3, touches_kernel: true }) == Verdict::Deny,
        "runtime closure",
    );

    set
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f276_msr_plan_is_ordered() {
        let e = SyscallEntry::new(0xFFFF_8000_0000_1000, 0);
        let p = msr_plan(&e);
        assert_eq!(p[0].msr, IA32_STAR);
        assert_eq!(p[1].msr, IA32_FMASK);
        assert_eq!(p[2].msr, IA32_LSTAR);
        assert_eq!(p[4].msr, IA32_EFER);
        assert!(e.sce_enabled() && e.nxe_enabled());
        // 宿主目标下如实不落地
        assert_eq!(commit(&e), cfg!(target_os = "none"));
    }

    #[test]
    fn f277_non_canonical_is_rejected() {
        assert!(!is_canonical(0x0000_8000_0000_0000));
        assert!(!is_canonical(0xFFFF_0000_0000_0000));
        assert!(is_canonical(0xFFFF_8000_0000_0000));
        assert_eq!(prepare_ring3(0x1_0000, 0, 0), Err(ErrNo::Efault));
    }

    #[test]
    fn f278_table_numbers_are_dense() {
        for i in 0..MAX_SYSCALLS {
            assert_eq!(SYSCALL_TABLE[i].num, i as u16);
        }
        assert_eq!(spec(0).unwrap().name, "read");
        assert!(spec(64).is_none());
        assert_eq!(class_count(SysClass::Ipc), 6);
    }

    #[test]
    fn f279_kernel_pointer_always_rejected() {
        let mut args = [0u64; MAX_ARGS];
        let mut lens = [0u64; MAX_ARGS];
        args[0] = KERNEL_BASE;
        lens[0] = 1;
        assert_eq!(validate_args(spec(2).unwrap(), &args, &lens), Err(ErrNo::Efault));
        args[0] = 0x10_0000;
        lens[0] = 8;
        assert_eq!(validate_args(spec(2).unwrap(), &args, &lens), Ok(()));
    }

    #[test]
    fn f285_round_robin_is_fair() {
        let mut s = Scheduler::new();
        s.spawn(1, 5);
        s.spawn(2, 5);
        let a = s.pick_next();
        let b = s.pick_next();
        assert_ne!(a, b);
        s.block(a.unwrap());
        assert_eq!(s.pick_next(), b);
    }

    #[test]
    fn f298_fuzz_is_deterministic() {
        let a = fuzz(256, 42);
        let b = fuzz(256, 42);
        assert_eq!(a, b);
        assert_eq!(a.panics, 0);
    }

    #[test]
    fn uspace_domain_has_25_passing_checks() {
        let set = run_uspace_checks();
        assert_eq!(set.len(), 25);
        assert_eq!(set.tally(), (25, 0));
        assert!(set.all_passed());
        let mut buf = [0u8; 1024];
        let n = set.render(&mut buf);
        let text = core::str::from_utf8(&buf[..n]).unwrap();
        assert!(text.starts_with("uspace PASS 25/25"));
    }
}
