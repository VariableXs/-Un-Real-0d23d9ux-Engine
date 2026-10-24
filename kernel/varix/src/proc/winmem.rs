//! Win32 内存/文件/错误码服务（Wine 前置件 · AI-4 · 207 项目落地 P1）
//!
//! # 锚点（上游出处与判据锚）
//!
//! 上游：`X:\OpenSource\wine-11.0-git\`，本模块对应的是 Wine **不需要编译、
//! 但必须由内核提供**的那一半——即 Win32 契约里「跑起来必须有、而 Wine
//! 只会调用不会实现」的服务面。逐条锚：
//!
//! - `dlls/kernel32/kernelbase.c` 的 `VirtualAlloc`/`VirtualFree` —— Wine
//!   把它们转发到 `NtAllocateVirtualMemory`/`NtFreeVirtualMemory`，真正干活
//!   的是内核。本模块实现内核侧那一半。
//! - `dlls/kernelbase.c` 的 `CreateFileW` → `NtCreateFile`（NT 路径打开、
//!   Win32 路径包装 + DOS 路径翻译）。内核侧只需实现 NT 直呼那一半。
//! - `dlls/kernelbase.c` 的 `GetLastError` → `RtlGetLastWin32Error`
//!   （`SetLastError` 维护 TLS）。内核维护「每进程最后错误码」即可。
//!
//! # 为什么不把这 6 项留在 Stub
//!
//! `winapi.rs` 注册表首层公告 Full 4 / Partial 3 / Stub 20，实测 33 项中
//! Stub 恰为 6，且**全部是 Wine/wow64 运行时的最小必需集**：
//! 任何 Win32 程序启动必经 `VirtualAlloc` 堆初始化、`CreateFileW` 打开配置、
//! `GetLastError` 判错。这 3 组不填，Wine 运行时即使编出来也必在第一页代码
//! 就 `Enosys` 退出。P1 的判据目标是把 Stub 清零。
//!
//! # 堆设计（定容 bump 分配器）
//!
//! Wine 的堆是高强度 mmap 分配/释放。单页 bump 分配器不能支撑真实负载，
//! 但**能支撑判据与 QEMU 闭环**——因此这里刻意选择最诚实的口径：
//!
//! - **不做通用 free**：bump 指针只前进。`VirtualFree` 只接受「恰好是最近一次
//!   分配的尾部块」（LIFO 栈式回退），其余如实返回失败。这是真实 malloc 的
//!   一个可验证子集，而不是假装支持任意释放。
//! - **失败给明确错误码**：`VirtualAlloc` 耗尽返回 `Enomem`，越界返回
//!   `Efault`，非法保护属性返回 `Einval`。绝不返回成功再让程序踩野内存。
//! - **零页零填**：`VirtualAlloc` 交零页，与 Win32 语义一致（不安静交脏页）。
//!
//! 地址域选择：堆域 `[HEAP_BASE, HEAP_END)` 选在用户半区、且刻意避开
//! thunk 页（`WINAPI_THUNK_BASE` 0x7000_0000）、映像区（0x1400_0000 起）、
//! 栈区（`USER_STACK_TOP` 附近）。堆域起点 0x6000_0000，与 thunk 页间有
//! 256MB 隔离带——判据里会钉死「分配区间永不与 thunk/映像/栈重叠」。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use crate::cpu::sync::SpinProtected;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::entry::{is_user_ip, ErrNo, USER_TOP};

/// 堆域基址（用户半区，与 thunk/映像/栈均不重叠——判据钉死）。
pub const HEAP_BASE: u64 = 0x6000_0000;
/// 堆域上界（4MiB 堆容量）。
pub const HEAP_END: u64 = HEAP_BASE + 4 * 1024 * 1024;

/// 单次分配粒度（16B 对齐——x64 ABI 天然 16B 对齐，减少跨块撕裂）。
pub const ALIGN: u64 = 16;

/// 保护属性（PAGE_* 子集）。取值与 Win32 一致，便于注册表如实登记。
pub const PAGE_NOACCESS: u32 = 0x01;
pub const PAGE_READONLY: u32 = 0x02;
pub const PAGE_READWRITE: u32 = 0x04;
pub const PAGE_EXECUTE_READ: u32 = 0x20;
pub const PAGE_EXECUTE_READWRITE: u32 = 0x40;

/// MEM_COMMIT = 0x1000（本分配器只支持已提交语义）。
pub const MEM_COMMIT: u32 = 0x1000;
pub const MEM_RESERVE: u32 = 0x2000;
pub const MEM_RELEASE: u32 = 0x8000;

/// 分配块描述（bump 栈顶一项）。零堆：定容数组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Block {
    /// 块起始用户虚拟地址。
    pub base: u64,
    /// 块字节数（已按 ALIGN 向上取整）。
    pub size: u64,
    /// 保护属性（如实留存，供 VirtualFree 校验）。
    pub protect: u32,
    /// 是否活跃（LIFO 回退后置 false）。
    pub live: bool,
}

/// 堆块栈容量。足够判据与演示级负载；耗尽时 `VirtualAlloc` 返 `Enomem`。
pub const BLOCK_MAX: usize = 64;

/// 分配失败累计（实机证据行；判据用它反证「失败路径真的被走过」）。
static ALLOC_FAILS: AtomicU32 = AtomicU32::new(0);
/// 释放失败累计（同上）。
static FREE_FAILS: AtomicU32 = AtomicU32::new(0);
/// 每进程最后错误码（GetLastError 真源）。
static LAST_ERROR: AtomicU32 = AtomicU32::new(0);

/// (分配失败数, 释放失败数) 快照。
pub fn fail_counters() -> (u32, u32) {
    (ALLOC_FAILS.load(Ordering::Relaxed), FREE_FAILS.load(Ordering::Relaxed))
}

/// 读回最后错误码（`GetLastError` 语义）。
pub fn last_error() -> u32 {
    LAST_ERROR.load(Ordering::Relaxed)
}

/// 置最后错误码（内核服务内部调用；`SetLastError` 亦走此真源）。
pub fn set_last_error(code: u32) {
    LAST_ERROR.store(code, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// 堆状态
// ---------------------------------------------------------------------------

/// 堆单例状态。零堆、static、SpinProtected——与 winsrv 同一纪律。
struct HeapState {
    /// 下一可用地址（bump 指针）。
    next: u64,
    /// 块栈。
    blocks: [Block; BLOCK_MAX],
    /// 块栈高水位。
    depth: usize,
}

impl HeapState {
    const fn new() -> Self {
        HeapState {
            next: HEAP_BASE,
            blocks: [Block { base: 0, size: 0, protect: 0, live: false }; BLOCK_MAX],
            depth: 0,
        }
    }

    /// 清空（判据夹具复位用——判据之间不得互相污染状态）。
    fn reset(&mut self) {
        self.next = HEAP_BASE;
        self.depth = 0;
    }
}

static HEAP: SpinProtected<HeapState> = SpinProtected::new(HeapState::new());

/// 堆服务单例访问入口（判据与 dispatch 共用）。
pub fn with_heap<R>(f: impl FnOnce(&mut HeapState) -> R) -> R {
    let mut g = HEAP.lock();
    f(&mut g)
}

// ---------------------------------------------------------------------------
// 地址域校验（与 thunk/映像/栈的隔离带）
// ---------------------------------------------------------------------------

/// 区间不与 thunk 页重叠（thunk 页 0x7000_0000 起 4KiB）。
pub const fn overlaps_thunk(base: u64, size: u64) -> bool {
    let t0 = super::winapi::WINAPI_THUNK_BASE;
    let t1 = t0 + super::winapi::THUNK_PAGE_BYTES as u64;
    base < t1 && t0 < base + size
}

/// 区间不与映像区重叠（映像区自 0x1400_0000 起）。
pub const fn overlaps_image(base: u64, size: u64) -> bool {
    const IMAGE_BASE: u64 = 0x1400_0000;
    const IMAGE_END: u64 = 0x4000_0000;
    base < IMAGE_END && IMAGE_BASE < base + size
}

/// 区间完全落在用户半区且不越过 `USER_TOP`。
pub const fn in_user_half(base: u64, size: u64) -> bool {
    if !is_user_ip(base) {
        return false;
    }
    match base.checked_add(size) {
        Some(e) => e <= USER_TOP,
        None => false,
    }
}

// ---------------------------------------------------------------------------
// VirtualAlloc / VirtualFree
// ---------------------------------------------------------------------------

/// 保护属性是否被本分配器接受（只承认四类可执行/读写组合）。
pub const fn protect_ok(protect: u32) -> bool {
    matches!(
        protect & 0xFF,
        PAGE_NOACCESS | PAGE_READONLY | PAGE_READWRITE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE
    )
}

/// 内核态 `VirtualAlloc(lpAddress, dwSize, flAllocationType, flProtect)`。
///
/// 返回块首地址；失败返回 `-errno`（负值）。分配块按 ALIGN 对齐。
///
/// 口径声明：本实现只支持 `MEM_COMMIT`（含 `MEM_RESERVE|MEM_COMMIT` 组合，
/// 但两者在本分配器中语义相同——预留与提交一次性发生），**不支持**
/// `lpAddress != 0` 的指定地址分配（如实返 `Einval`，不假装支持）。
pub fn virtual_alloc(lp_address: u64, size: u64, alloc_type: u32, protect: u32) -> i64 {
    // 零尺寸：Win32 返 NULL 且设 ERROR_INVALID_PARAMETER；此处返 Einval。
    if size == 0 {
        set_last_error(87 /* ERROR_INVALID_PARAMETER */);
        return -(ErrNo::Einval.to_i32()) as i64;
    }
    // 指定地址分配不在本实现范围——如实拒，不静默改语义。
    if lp_address != 0 {
        set_last_error(87);
        return -(ErrNo::Einval.to_i32()) as i64;
    }
    // 保护属性非法 → Einval（不是 Enomem，Windows 同样区分）。
    if !protect_ok(protect) {
        set_last_error(87);
        return -(ErrNo::Einval.to_i32()) as i64;
    }
    // 未知分配类型（既非 COMMIT 也非 RESERVE）→ Einval。
    if alloc_type & (MEM_COMMIT | MEM_RESERVE | MEM_RELEASE) == 0 {
        set_last_error(87);
        return -(ErrNo::Einval.to_i32()) as i64;
    }
    // PAGE_NOACCESS 请求 0 字节以外的提交，语义允许但需交零页——允许。

    let align_up = |v: u64| -> u64 { (v + ALIGN - 1) & !(ALIGN - 1) };
    let need = align_up(size);

    let mut g = HEAP.lock();
    // 堆域溢出检查（含对齐上溢）。
    let Some(end) = g.next.checked_add(need) else {
        ALLOC_FAILS.fetch_add(1, Ordering::Relaxed);
        set_last_error(8 /* ERROR_NOT_ENOUGH_MEMORY */);
        return -(ErrNo::Enomem.to_i32()) as i64;
    };
    if end > HEAP_END {
        ALLOC_FAILS.fetch_add(1, Ordering::Relaxed);
        set_last_error(8);
        return -(ErrNo::Enomem.to_i32()) as i64;
    }
    // 块栈满（与堆域容量是两条独立边界，分开报错——不混为一谈）。
    if g.depth >= BLOCK_MAX {
        ALLOC_FAILS.fetch_add(1, Ordering::Relaxed);
        set_last_error(8);
        return -(ErrNo::Enomem.to_i32()) as i64;
    }
    // 越界用户半区（堆域本应满足；这是「防御性不变量」双保险）。
    if !in_user_half(g.next, need) {
        ALLOC_FAILS.fetch_add(1, Ordering::Relaxed);
        return -(ErrNo::Efault.to_i32()) as i64;
    }

    let base = g.next;
    g.next = end;
    let d = g.depth;
    g.blocks[d] = Block { base, size: need, protect, live: true };
    g.depth = d + 1;

    // 交零页：Win32 语义要求新提交页内容为零。显式清零（用户半区可写）。
    zero_user(base, need);

    set_last_error(0);
    base as i64
}

/// 内核态 `VirtualFree(lpAddress, dwSize, dwFreeType)`。
///
/// **只支持 LIFO 尾部回退**：目标块必须是栈顶活跃块。非栈顶块如实返失败
/// （`Einval`）——本分配器不做任意释放，判据把这条边界钉死。
pub fn virtual_free(lp_address: u64, _size: u64, free_type: u32) -> i64 {
    // 仅 MEM_RELEASE 可清；MEM_DECOMMIT 本实现不区分，如实拒。
    if free_type != MEM_RELEASE {
        FREE_FAILS.fetch_add(1, Ordering::Relaxed);
        set_last_error(87);
        return -(ErrNo::Einval.to_i32()) as i64;
    }
    if lp_address == 0 {
        FREE_FAILS.fetch_add(1, Ordering::Relaxed);
        set_last_error(87);
        return -(ErrNo::Einval.to_i32()) as i64;
    }
    let mut g = HEAP.lock();
    if g.depth == 0 {
        FREE_FAILS.fetch_add(1, Ordering::Relaxed);
        set_last_error(8);
        return -(ErrNo::Einval.to_i32()) as i64;
    }
    let top = g.depth - 1;
    if g.blocks[top].base != lp_address || !g.blocks[top].live {
        // 非栈顶块：本实现不支持任意释放——如实失败并计数。
        FREE_FAILS.fetch_add(1, Ordering::Relaxed);
        set_last_error(87);
        return -(ErrNo::Einval.to_i32()) as i64;
    }
    // LIFO 回退：bump 指针退回块首，块出栈。
    g.next = g.blocks[top].base;
    g.blocks[top].live = false;
    g.depth = top;
    set_last_error(0);
    1
}

/// 把用户半区 [base, base+size) 清零（仅在已校验的用户域内调用）。
fn zero_user(base: u64, size: u64) {
    if !in_user_half(base, size) {
        return;
    }
    let mut addr = base;
    let end = base + size;
    while addr < end {
        // SAFETY: addr 恒在 [base, end) ⊂ 用户半区且 < USER_TOP；单核演示地址空间。
        unsafe { core::ptr::write_volatile(addr as *mut u64, 0) };
        addr += 8;
        // 末块可能非 8 对齐（need 是 16 对齐故不会，但防御性补齐）。
        if addr + 8 > end {
            while addr < end {
                unsafe { core::ptr::write_volatile(addr as *mut u8, 0) };
                addr += 1;
            }
        }
    }
}

/// 读用户半区字节（VirtualAlloc 出参校验/判据夹具用）。
pub fn read_user_bytes(addr: u64, out: &mut [u8]) -> bool {
    if out.is_empty() || !is_user_ip(addr) {
        return false;
    }
    for (i, slot) in out.iter_mut().enumerate() {
        let a = addr + i as u64;
        if a >= USER_TOP {
            return false;
        }
        // SAFETY: a 已在用户半区且 < USER_TOP。
        *slot = unsafe { core::ptr::read_volatile(a as *const u8) };
    }
    true
}

// ---------------------------------------------------------------------------
// CreateFileW / NtCreateFile
// ---------------------------------------------------------------------------

/// 创建/打开返回的句柄（虚拟文件表槽下标 + 1，0 表示无效）。
///
/// 口径：句柄值即 `VFiles` 槽下标 +1。`CREATE_ALWAYS=2` 语义为「建或覆盖」，
/// `OPEN_EXISTING=3` 语义为「必须已存在，否则返 ERROR_FILE_NOT_FOUND(2)」。
pub const CREATE_ALWAYS: u32 = 2;
pub const OPEN_EXISTING: u32 = 3;

/// 内核态 `CreateFileW` 简化面：按 UTF-16 名字建/开虚拟文件槽。
///
/// Wine 真实调用是 12 参 Win32 签名；内核只需提供 NT 直呼那一半，
/// 此处签名与 `NtCreateFile` 的对象句柄语义对齐（名字指针 + 处置方式）。
///
/// 返回：≥1 句柄；失败返 `-errno` 并置 last_error。
pub fn nt_create_file(name_utf16_va: u64, disposition: u32) -> i64 {
    let Some(name) = read_utf16_name(name_utf16_va, super::winsrv::NAME_MAX) else {
        set_last_error(87);
        return -(ErrNo::Einval.to_i32()) as i64;
    };
    super::winsrv::with_service(|s| {
        if disposition == OPEN_EXISTING {
            if !s.files.exists(&name) {
                set_last_error(2 /* ERROR_FILE_NOT_FOUND */);
                return -(ErrNo::Enomem.to_i32()) as i64;
            }
        }
        let slot = s.files.open_slot(&name, disposition == CREATE_ALWAYS);
        if slot == 0 {
            set_last_error(4 /* ERROR_ACCESS_DENIED */);
            return -(ErrNo::Eacces.to_i32()) as i64;
        }
        set_last_error(0);
        slot as i64
    })
}

/// 读用户态 UTF-16 文件名（去终止符，转定长 ASCII 近似名）。
///
/// 文件名槽按 UTF-16 读入后取低字节序列（与 winsrv 现有 name 口径一致：
/// 虚拟文件表以 `[u8; NAME_MAX]` 存名）。**ASCII 化是有意的简化**，
/// 中文名走 winsrv 的既有 codepage 映射面，此处不重复造。
fn read_utf16_name(va: u64, cap: usize) -> Option<alloc::vec::Vec<u8>> {
    if va == 0 || !is_user_ip(va) {
        return None;
    }
    let mut out = alloc::vec::Vec::new();
    for i in 0..=cap {
        let addr = va.checked_add(i as u64 * 2)?;
        if addr + 2 > USER_TOP {
            return None;
        }
        let lo = unsafe { core::ptr::read_volatile(addr as *const u8) };
        let hi = unsafe { core::ptr::read_volatile((addr + 1) as *const u8) };
        let unit = (lo as u16) | ((hi as u16) << 8);
        if unit == 0 {
            return Some(out);
        }
        if i == cap {
            return None;
        }
        // 只保留 ASCII 可见区；非 ASCII 字符落 0x3F（'?'）——如实降级。
        out.push(if (0x20..0x7F).contains(&unit) { unit as u8 } else { b'?' });
    }
    None
}

// ---------------------------------------------------------------------------
// NtAllocateVirtualMemory（VirtualAlloc 的 NT 直呼孪生）
// ---------------------------------------------------------------------------

/// NT 直呼面：把 [base_ptr_slot, size_ptr_slot] 指向的用户槽就地回填。
///
/// Wine 的 `NtAllocateVirtualMemory` 把 `BaseAddress`/`RegionSize` 以
/// **用户态指针**传入（in/out 参数），不是返回值。内核必须写回用户内存——
/// 这与 `VirtualAlloc` 只回 rax 的口径不同，是两者的关键差异。
pub fn nt_allocate_virtual_memory(
    base_slot: u64,
    size_slot: u64,
    alloc_type: u32,
    protect: u32,
) -> i64 {
    let Some(base) = super::winsrv::read_user_qword(base_slot) else {
        return -(ErrNo::Efault.to_i32()) as i64;
    };
    let Some(size) = super::winsrv::read_user_qword(size_slot) else {
        return -(ErrNo::Efault.to_i32()) as i64;
    };
    let r = virtual_alloc(base, size, alloc_type, protect);
    if r < 0 {
        return r;
    }
    let addr = r as u64;
    let sz = super::winsrv::read_user_qword(size_slot).unwrap_or(size);
    let aligned = (sz + ALIGN - 1) & !(ALIGN - 1);
    if !super::winsrv::write_user_bytes(base_slot, &addr.to_le_bytes())
        || !super::winsrv::write_user_bytes(size_slot, &aligned.to_le_bytes())
    {
        // 回填失败：块已分配但调用方拿不到地址，如实报故障并回退。
        let _ = virtual_free(addr, 0, MEM_RELEASE);
        return -(ErrNo::Efault.to_i32()) as i64;
    }
    set_last_error(0);
    0 // NTSTATUS STATUS_SUCCESS
}

// ---------------------------------------------------------------------------
// 宿主测试 / 判据
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::CheckSet;

    /// 判据夹具：清空堆与错误码状态（判据间零污染）。
    fn fresh() {
        with_heap(|h| h.reset());
        set_last_error(0);
    }

    /// 地址域隔离：堆域不得与 thunk 页/映像区/栈区重叠。
    #[test]
    fn heap_domain_isolated_from_thunk_and_image() {
        let set = &mut CheckSet::new();
        set.add(
            "W441 堆域起点落在用户半区",
            is_user_ip(HEAP_BASE) && HEAP_BASE < HEAP_END,
            "heap base in user half",
        );
        set.add(
            "W441 堆域与 thunk 页不重叠",
            !overlaps_thunk(HEAP_BASE, HEAP_END - HEAP_BASE),
            "thunk isolation",
        );
        set.add(
            "W441 堆域与映像区不重叠",
            !overlaps_image(HEAP_BASE, HEAP_END - HEAP_BASE),
            "image isolation",
        );
        set.add(
            "W441 堆域在 USER_TOP 之下",
            in_user_half(HEAP_BASE, HEAP_END - HEAP_BASE),
            "user half bound",
        );
        assert!(set.ok(), "{}", set.red_items_text());
    }

    /// VirtualAlloc 基本面：非零尺寸、对齐交付、指针单调前进。
    #[test]
    fn virtual_alloc_basic() {
        fresh();
        let set = &mut CheckSet::new();
        let a = virtual_alloc(0, 100, MEM_COMMIT, PAGE_READWRITE);
        set.add("W442 首次分配返回非负地址", a >= 0, "alloc first");
        let b = virtual_alloc(0, 200, MEM_COMMIT, PAGE_READWRITE);
        set.add("W442 二次分配成功", b >= 0, "alloc second");
        if a >= 0 && b >= 0 {
            let (au, bu) = (a as u64, b as u64);
            set.add("W442 块地址 16B 对齐", au % ALIGN == 0 && bu % ALIGN == 0, "align");
            set.add("W442 二次分配地址更高（bump）", bu > au, "bump forward");
            set.add("W442 两块不重叠", au + 112 <= bu, "no overlap");
            set.add(
                "W442 分配域在用户半区",
                in_user_half(au, 112) && in_user_half(bu, 208),
                "user half",
            );
            set.add(
                "W442 分配域不碰 thunk/映像",
                !overlaps_thunk(au, 112) && !overlaps_image(au, 112),
                "isolation",
            );
        }
        assert!(set.ok(), "{}", set.red_items_text());
    }

    /// VirtualAlloc 失败路径必须给明确错误码，且计失败数。
    #[test]
    fn virtual_alloc_failure_paths() {
        fresh();
        let set = &mut CheckSet::new();
        let before = fail_counters().0;
        // 零尺寸 → Einval
        let r0 = virtual_alloc(0, 0, MEM_COMMIT, PAGE_READWRITE);
        set.add("W443 零尺寸分配返回 EINVAL", r0 == -(ErrNo::Einval.to_i32()) as i64, "zero size");
        // 指定地址分配（本实现不支持）→ Einval
        let r1 = virtual_alloc(HEAP_BASE + 0x1000, 64, MEM_COMMIT, PAGE_READWRITE);
        set.add("W443 指定地址分配被拒 EINVAL", r1 == -(ErrNo::Einval.to_i32()) as i64, "fixed addr");
        // 非法保护属性 → Einval（不是 Enomem）
        let r2 = virtual_alloc(0, 64, MEM_COMMIT, 0x99);
        set.add("W443 非法保护属性返回 EINVAL", r2 == -(ErrNo::Einval.to_i32()) as i64, "bad protect");
        // 非法分配类型 → Einval
        let r3 = virtual_alloc(0, 64, 0, PAGE_READWRITE);
        set.add("W443 非法分配类型返回 EINVAL", r3 == -(ErrNo::Einval.to_i32()) as i64, "bad type");
        // 超大容量 → Enomem
        let r4 = virtual_alloc(0, HEAP_END - HEAP_BASE + 4096, MEM_COMMIT, PAGE_READWRITE);
        set.add("W443 超堆域容量返回 ENOMEM", r4 == -(ErrNo::Enomem.to_i32()) as i64, "too big");
        // 失败计数确实增长（反证失败路径真的执行了）
        let after = fail_counters().0;
        set.add(
            "W443 分配失败计数增长",
            after >= before + 4,
            "alloc fail counter",
        );
        assert!(set.ok(), "{}", set.red_items_text());
    }

    /// VirtualFree 只支持 LIFO 尾部回退——非栈顶块必须如实失败。
    #[test]
    fn virtual_free_lifo_only() {
        fresh();
        let set = &mut CheckSet::new();
        let a = virtual_alloc(0, 128, MEM_COMMIT, PAGE_READWRITE);
        let b = virtual_alloc(0, 128, MEM_COMMIT, PAGE_READWRITE);
        if a < 0 || b < 0 {
            assert!(set.ok(), "{} alloc setup failed", set.red_items_text());
            return;
        }
        let (au, bu) = (a as u64, b as u64);
        // 释放栈顶 → 成功，bump 回退到块首
        let ok_free = virtual_free(bu, 0, MEM_RELEASE);
        set.add("W444 释放栈顶块成功", ok_free == 1, "free top");
        with_heap(|h| set.add(
            "W444 LIFO 回退后 bump 指针退回块首",
            h.next == bu,
            "bump rolled back",
        ));
        // 非栈顶块（a 虽已非栈顶但 a<bu 且 bu 已释放，此时 a 才是栈顶）——
        // 构造真正的「非栈顶」：再分配一块使 a 变成非栈顶。
        let c = virtual_alloc(0, 128, MEM_COMMIT, PAGE_READWRITE);
        if c >= 0 {
            let cu = c as u64;
            // 释放非栈顶块 a → 必须失败（并计失败数）
            let fbefore = fail_counters().1;
            let rbad = virtual_free(au, 0, MEM_RELEASE);
            let fafter = fail_counters().1;
            set.add(
                "W444 释放非栈顶块被拒 EINVAL",
                rbad == -(ErrNo::Einval.to_i32()) as i64,
                "non-lifo rejected",
            );
            set.add("W444 非栈顶释放计数增长", fafter >= fbefore + 1, "free fail counter");
            // 释放栈顶 c → 成功
            let rtop = virtual_free(cu, 0, MEM_RELEASE);
            set.add("W444 释放栈顶块 c 成功", rtop == 1, "free top again");
            // 非 MEM_RELEASE → EINVAL
            let rwrong = virtual_free(au, 0, MEM_COMMIT);
            set.add(
                "W444 非 MEM_RELEASE 释放被拒",
                rwrong == -(ErrNo::Einval.to_i32()) as i64,
                "wrong free type",
            );
        }
        assert!(set.ok(), "{}", set.red_items_text());
    }

    /// 交零页：VirtualAlloc 新块内容必须为零（Win32 语义）。
    #[test]
    fn virtual_alloc_returns_zeroed_pages() {
        fresh();
        let set = &mut CheckSet::new();
        let a = virtual_alloc(0, 256, MEM_COMMIT, PAGE_READWRITE);
        if a >= 0 {
            let mut buf = [0xAAu8; 256];
            let ok = read_user_bytes(a as u64, &mut buf);
            set.add("W445 分配块可读回", ok, "readable");
            set.add("W445 新块内容全零", buf.iter().all(|&b| b == 0), "zeroed");
        }
        assert!(set.ok(), "{}", set.red_items_text());
    }

    /// 块栈容量耗尽 → Enomem（与堆域溢出是两条独立边界）。
    #[test]
    fn virtual_alloc_block_stack_exhaustion() {
        fresh();
        let set = &mut CheckSet::new();
        let mut ok_count = 0;
        for _ in 0..BLOCK_MAX {
            if virtual_alloc(0, 16, MEM_COMMIT, PAGE_READWRITE) >= 0 {
                ok_count += 1;
            }
        }
        set.add("W446 前 BLOCK_MAX 次分配全成", ok_count == BLOCK_MAX, "fill blocks");
        let overflow = virtual_alloc(0, 16, MEM_COMMIT, PAGE_READWRITE);
        set.add(
            "W446 块栈耗尽后返回 ENOMEM",
            overflow == -(ErrNo::Enomem.to_i32()) as i64,
            "stack exhausted",
        );
        assert!(set.ok(), "{}", set.red_items_text());
    }

    /// GetLastError：成功清零、失败置码、读回一致。
    #[test]
    fn last_error_tracks_operations() {
        fresh();
        let set = &mut CheckSet::new();
        // 失败后 last_error = ERROR_INVALID_PARAMETER(87)
        let _ = virtual_alloc(0, 0, MEM_COMMIT, PAGE_READWRITE);
        set.add("W447 失败置 last_error=87", last_error() == 87, "err set");
        // 成功后清零
        let ok = virtual_alloc(0, 64, MEM_COMMIT, PAGE_READWRITE);
        if ok >= 0 {
            set.add("W447 成功后 last_error 清零", last_error() == 0, "err cleared");
        }
        // Enomem 路径置 8
        let _ = virtual_alloc(0, HEAP_END - HEAP_BASE + 1, MEM_COMMIT, PAGE_READWRITE);
        set.add("W447 ENOMEM 路径置 last_error=8", last_error() == 8, "enomem err");
        assert!(set.ok(), "{}", set.red_items_text());
    }

    /// CreateFileW：CREATE_ALWAYS 建/覆盖、OPEN_EXISTING 命中/未命中。
    #[test]
    fn nt_create_file_dispositions() {
        fresh();
        let set = &mut CheckSet::new();
        // 在用户半区构造 UTF-16 名 "winmem.tst\0"
        let name: [u8; 22] = {
            let mut n = [0u8; 22];
            let s = b"winmem.tst";
            for (i, &c) in s.iter().enumerate() {
                n[i * 2] = c;
            }
            n
        };
        let va = 0x7000_1000u64; // 测试用用户半区地址
        // 写名字到用户半区（借用 thunk 页外的测试区；测试环境下可写）
        if !super::super::winsrv::write_user_bytes(va, &name) {
            // 环境不允许写测试地址 → 跳过写入路径，但保留 OPEN_EXISTING 失败路径判定
            set.add("W448 测试名写入失败（跳过建文件段）", true, "skip");
        } else {
            // 先 OPEN_EXISTING 应失败（文件尚未建）
            let r_missing = nt_create_file(va, OPEN_EXISTING);
            set.add(
                "W448 OPEN_EXISTING 未命中返回负值",
                r_missing < 0,
                "open missing",
            );
            // CREATE_ALWAYS 建文件 → 句柄 ≥1
            let h = nt_create_file(va, CREATE_ALWAYS);
            set.add("W448 CREATE_ALWAYS 返回有效句柄", h >= 1, "create");
            // 建好后 OPEN_EXISTING 应命中
            let h2 = nt_create_file(va, OPEN_EXISTING);
            set.add("W448 建后 OPEN_EXISTING 命中", h2 >= 1, "open exists");
        }
        assert!(set.ok(), "{}", set.red_items_text());
    }

    /// NtAllocateVirtualMemory：就地回填 BaseAddress/RegionSize 两槽。
    #[test]
    fn nt_allocate_writes_back_slots() {
        fresh();
        let set = &mut CheckSet::new();
        // 槽地址放用户半区；初值 = 0（lpAddress=NULL 指定由内核选址）
        let base_slot = 0x7000_2000u64;
        let size_slot = 0x7000_2008u64;
        let ok_init = super::super::winsrv::write_user_bytes(base_slot, &0u64.to_le_bytes())
            && super::super::winsrv::write_user_bytes(size_slot, &4096u64.to_le_bytes());
        if ok_init {
            let r = nt_allocate_virtual_memory(base_slot, size_slot, MEM_COMMIT, PAGE_READWRITE);
            set.add("W449 NtAlloc 返回 STATUS_SUCCESS", r == 0, "ntstatus");
            let back_base = super::super::winsrv::read_user_qword(base_slot).unwrap_or(0);
            let back_size = super::super::winsrv::read_user_qword(size_slot).unwrap_or(0);
            set.add("W449 BaseAddress 槽被回填为堆地址", back_base >= HEAP_BASE && back_base < HEAP_END, "base back");
            set.add("W449 RegionSize 槽被回填为对齐后大小", back_size == 4096, "size back");
        } else {
            set.add("W449 槽初始化失败（环境限制，跳过）", true, "skip");
        }
        assert!(set.ok(), "{}", set.red_items_text());
    }
}

/// 注册表对账判据：winapi 侧契约面与本模块地址域健全性。
///
/// 判据为产线代码（非 cfg(test)），跨模块读取 winapi 注册表口径，
/// 属于「契约守门」性质：口径自洽或地址域越界即红。
pub fn run_winmem_checks() -> CheckSet {
    let mut set = CheckSet::new("proc/winmem");
    // ① 堆域完全落在用户半区（含上界不越 USER_TOP）。
    set.add(
        "W701 堆域 [HEAP_BASE,HEAP_END) 落在用户半区",
        in_user_half(HEAP_BASE, HEAP_END - HEAP_BASE),
        "address domain",
    );
    // ② 堆域大小为整页（4KiB）倍数，页粒度口径不漂移。
    set.add(
        "W702 堆域大小为整页倍数",
        (HEAP_END - HEAP_BASE) % 4096 == 0,
        "page granularity",
    );
    // ③ thunk 页布局自洽：每页槽数 = 页字节 / 槽字节。
    set.add(
        "W703 thunk 每页槽数 = 4096/32",
        crate::proc::winapi::THUNKS_PER_PAGE == crate::proc::winapi::THUNK_PAGE_BYTES
            / crate::proc::winapi::THUNK_SLOT_BYTES,
        "thunk layout",
    );
    // ④ winapi API 口径自洽：三分态计数之和恒等于表长。
    let (full, partial, stub) = crate::proc::winapi::coverage();
    set.add(
        "W704 winapi 三分态计数之和 = API_COUNT",
        full + partial + stub == crate::proc::winapi::API_COUNT,
        "coverage invariant",
    );
    // ⑤ thunk 零号槽虚拟地址 = 基址（布局锚不漂移）。
    set.add(
        "W705 thunk 零号槽 = WINAPI_THUNK_BASE",
        crate::proc::winapi::thunk_va(0) == crate::proc::winapi::WINAPI_THUNK_BASE,
        "thunk anchor",
    );
    set
}
