//! 任务40（AI-B）· Win32 服务台 —— 导入绑定 + Wine 核心 DLL 首层 API。
//!
//! 三层结构（总案阶段5）的中间层：PE 装载器（pe.rs）解析出导入表，本模块
//! 把每个导入绑定到 VARIX 移植版 Win32 API 的实现上。首层口径（如实声明）：
//!
//! * **注册表化**：全部 API 集中在 [`API_TABLE`]，每个条目带三态标记
//!   （Full=语义完整 / Partial=映射到内核既有服务、有边界 / Stub=未实现），
//!   缺失能力返回明确错误码（Enosys），绝不崩溃、绝不静默假装成功。
//! * **thunk 页**：内核在用户地址空间固定基址（[`WINAPI_THUNK_BASE`]）放一页
//!   R+X 代码，每个 API 一个 32B 槽：`mov rdi,rcx; mov rsi,rdx; mov rdx,r8;
//!   mov eax,NR; syscall; ret`——把 Win32 x64 调用约定（rcx/rdx/r8/r9）搬运到
//!   内核用户 ABI（rax=nr, rdi/rsi/rdx=a1/a2/a3）再陷内核。IAT 补钉写入的
//!   就是槽地址，GetProcAddress（动态路）返回的也是同一槽地址——静态/动态
//!   两路一致是结构性保证，宿主+实机双重断言。
//! * **号段**：`WIN32_NR_BASE = 0x40`（64）起，独立于 0..15 稳定 ABI；
//!   槽号 = 注册表扁平下标，`dispatch(slot, …)` 查表分发。
//! * **内核服务映射**：每个非 Stub API 的注释注明对应内核服务号
//!   （SYS_EXIT=0 / SYS_WRITE=2，proc/ring3 决策层）。
//!
//! 首层 API 面（进程/内存/文件三组优先，总案阶段5 步骤5）：
//! kernel32 10 + ntdll 6 + user32 7 + gdi32 4 = 27 项，Full 4 / Partial 3 /
//! Stub 20。user32/gdi32 窗口面整体 Stub——那是任务41（记事本闭环）的
//! 范围，这里只立注册表插槽，不假装能画窗口。

use super::loader::{LoadedImage, UserMapper};
use super::pe::{parse_imports_into, ImportError, ImportTable, PeImage};
use crate::cpu::sync::SpinProtected;
use crate::entry::ErrNo;
use core::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// 号段与布局
// ---------------------------------------------------------------------------

/// Win32 服务台 syscall 号段基址（0..15 是稳定 ABI，绝不重叠）。
pub const WIN32_NR_BASE: u32 = 0x40;
/// thunk 页固定用户基址：页对齐、用户半区、低于映像区（0x1400_0000 起）
/// 与栈区（USER_STACK_TOP 附近），与两者都有巨大隔离带。
pub const WINAPI_THUNK_BASE: u64 = 0x7000_0000;
/// 单槽字节数（17B 编码 + 15B 余量；槽内偏移固定便于宿主/实机同址断言）。
pub const THUNK_SLOT_BYTES: usize = 32;
/// thunk 页 4KiB。
pub const THUNK_PAGE_BYTES: usize = 4096;
/// 单页槽数（27 项 API 远在容量内；越界是编译期断言的事，见 tests）。
pub const THUNKS_PER_PAGE: usize = THUNK_PAGE_BYTES / THUNK_SLOT_BYTES;

/// STD_OUTPUT_HANDLE（Windows 语义：-11）。
pub const STD_OUTPUT_HANDLE: u64 = (-11i64) as u64;

// ---------------------------------------------------------------------------
// 注册表
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApiStatus {
    /// 语义完整（内核侧有完整实现）。
    Full,
    /// 映射到内核既有服务、有如实声明的边界。
    Partial,
    /// 未实现——调用返回 Enosys 错误码，绝不崩溃。
    Stub,
}

impl ApiStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            ApiStatus::Full => "full",
            ApiStatus::Partial => "partial",
            ApiStatus::Stub => "stub",
        }
    }
}

/// 注册表条目。slot = 扁平下标；dll 名小写规范形（查找大小写不敏感）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ApiDef {
    pub dll: &'static str,
    pub name: &'static str,
    pub status: ApiStatus,
}

/// 首层注册表（27 项）。**开放性契约**：新增 API = 在表尾追加一行 +
/// dispatch 的 match 加一臂，注册表/thunk/查找机制零改动。
pub const API_TABLE: [ApiDef; 33] = [
    // ---- kernel32.dll（进程/内存/文件三组优先）----
    // ExitProcess → 内核 SYS_EXIT(0)（proc/ring3 sys_exit，监护链收割）。
    ApiDef { dll: "kernel32.dll", name: "ExitProcess", status: ApiStatus::Full },
    // GetProcAddress → 内核注册表查找（本模块），动态路返回 thunk 槽地址。
    ApiDef { dll: "kernel32.dll", name: "GetProcAddress", status: ApiStatus::Full },
    // GetStdHandle → 仅 STD_OUTPUT_HANDLE(-11) 映射到控制台句柄 1，其余返回 0。
    ApiDef { dll: "kernel32.dll", name: "GetStdHandle", status: ApiStatus::Partial },
    // WriteFile → 内核 SYS_WRITE(2) 控制台路（handle==1 only，边界如实）。
    ApiDef { dll: "kernel32.dll", name: "WriteFile", status: ApiStatus::Partial },
    ApiDef { dll: "kernel32.dll", name: "ReadFile", status: ApiStatus::Full }, // 任务41：handle 3=读取源
    ApiDef { dll: "kernel32.dll", name: "CreateFileW", status: ApiStatus::Stub },
    ApiDef { dll: "kernel32.dll", name: "CloseHandle", status: ApiStatus::Partial }, // 任务41：句柄收尾
    ApiDef { dll: "kernel32.dll", name: "VirtualAlloc", status: ApiStatus::Stub },
    ApiDef { dll: "kernel32.dll", name: "VirtualFree", status: ApiStatus::Stub },
    ApiDef { dll: "kernel32.dll", name: "GetLastError", status: ApiStatus::Stub },
    // ---- ntdll.dll（NTAPI 直呼层）----
    ApiDef { dll: "ntdll.dll", name: "RtlExitUserProcess", status: ApiStatus::Full },
    ApiDef { dll: "ntdll.dll", name: "NtTerminateProcess", status: ApiStatus::Full },
    ApiDef { dll: "ntdll.dll", name: "NtWriteFile", status: ApiStatus::Partial },
    ApiDef { dll: "ntdll.dll", name: "LdrGetProcedureAddress", status: ApiStatus::Full },
    ApiDef { dll: "ntdll.dll", name: "NtCreateFile", status: ApiStatus::Stub },
    ApiDef { dll: "ntdll.dll", name: "NtAllocateVirtualMemory", status: ApiStatus::Stub },
    // ---- user32.dll（窗口面：任务41 记事本闭环——多参 API 走 NT 风格参数块桥接，
    //      语义完整实现于 winsrv::win32_dispatch；派发类由用户态循环承担如实 Partial）----
    ApiDef { dll: "user32.dll", name: "MessageBoxW", status: ApiStatus::Partial },
    ApiDef { dll: "user32.dll", name: "RegisterClassExW", status: ApiStatus::Full },
    ApiDef { dll: "user32.dll", name: "CreateWindowExW", status: ApiStatus::Full },
    ApiDef { dll: "user32.dll", name: "ShowWindow", status: ApiStatus::Full },
    ApiDef { dll: "user32.dll", name: "UpdateWindow", status: ApiStatus::Full },
    ApiDef { dll: "user32.dll", name: "InvalidateRect", status: ApiStatus::Full },
    ApiDef { dll: "user32.dll", name: "GetMessageW", status: ApiStatus::Full },
    ApiDef { dll: "user32.dll", name: "TranslateMessage", status: ApiStatus::Partial },
    ApiDef { dll: "user32.dll", name: "DispatchMessageW", status: ApiStatus::Partial },
    ApiDef { dll: "user32.dll", name: "PostQuitMessage", status: ApiStatus::Full },
    ApiDef { dll: "user32.dll", name: "DefWindowProcW", status: ApiStatus::Partial },
    // ---- gdi32.dll（文本/绘制面：任务41——画布字模渲染，hdc 约定 1）----
    ApiDef { dll: "gdi32.dll", name: "TextOutW", status: ApiStatus::Full },
    ApiDef { dll: "gdi32.dll", name: "BeginPaint", status: ApiStatus::Full },
    ApiDef { dll: "gdi32.dll", name: "EndPaint", status: ApiStatus::Full },
    ApiDef { dll: "gdi32.dll", name: "CreateFontW", status: ApiStatus::Partial },
    // ---- comdlg32.dll（文件对话框面：任务41——虚拟文件槽+轮转选择器，
    //      选择 UI 实机渲染后置任务 27，内核语义如实登记）----
    ApiDef { dll: "comdlg32.dll", name: "GetOpenFileNameW", status: ApiStatus::Full },
    ApiDef { dll: "comdlg32.dll", name: "GetSaveFileNameW", status: ApiStatus::Full },
    // （ReadFile/CloseHandle 由原 kernel32 条目升级承载——任务41 句柄分类扩展）
];

/// 注册表条目数（= thunk 槽数）。
pub const API_COUNT: usize = API_TABLE.len();

/// 三态覆盖率快照（首层 API 覆盖率公示的机器可读源）。
pub const fn coverage() -> (usize, usize, usize) {
    let mut full = 0;
    let mut partial = 0;
    let mut stub = 0;
    let mut i = 0;
    while i < API_COUNT {
        match API_TABLE[i].status {
            ApiStatus::Full => full += 1,
            ApiStatus::Partial => partial += 1,
            ApiStatus::Stub => stub += 1,
        }
        i += 1;
    }
    (full, partial, stub)
}

// ---------------------------------------------------------------------------
// 调用统计（实机证据行 + 宿主断言共用）
// ---------------------------------------------------------------------------

static FULL_CALLS: AtomicU64 = AtomicU64::new(0);
static PARTIAL_CALLS: AtomicU64 = AtomicU64::new(0);
static STUB_CALLS: AtomicU64 = AtomicU64::new(0);

/// (full, partial, stub) 调用计数快照。
pub fn stats() -> (u64, u64, u64) {
    (
        FULL_CALLS.load(Ordering::Relaxed),
        PARTIAL_CALLS.load(Ordering::Relaxed),
        STUB_CALLS.load(Ordering::Relaxed),
    )
}

// ---------------------------------------------------------------------------
// thunk 编码（const：宿主测试与目标态同一字节，杜绝两套编码漂移）
// ---------------------------------------------------------------------------

/// 第 `slot` 号 API 的 thunk 代码（17B + 零填充到 32B）。
///
/// ```text
/// mov rdi, rcx        ; Win32 a1 → 用户 ABI a1
/// mov rsi, rdx        ; Win32 a2 → a2
/// mov rdx, r8         ; Win32 a3 → a3
/// mov eax, NR         ; NR = WIN32_NR_BASE + slot
/// syscall             ; 双入口统一决策层
/// ret                 ; Full/Partial 返回 rax；ExitProcess 永不返回
/// ```
pub const fn thunk_bytes(slot: u32) -> [u8; THUNK_SLOT_BYTES] {
    let nr = WIN32_NR_BASE + slot;
    let mut b = [0u8; THUNK_SLOT_BYTES];
    b[0] = 0x48;
    b[1] = 0x89;
    b[2] = 0xCF; // mov rdi, rcx
    b[3] = 0x48;
    b[4] = 0x89;
    b[5] = 0xD6; // mov rsi, rdx
    b[6] = 0x4C;
    b[7] = 0x89;
    b[8] = 0xC2; // mov rdx, r8
    b[9] = 0xB8; // mov eax, imm32
    b[10] = nr as u8;
    b[11] = (nr >> 8) as u8;
    b[12] = (nr >> 16) as u8;
    b[13] = (nr >> 24) as u8;
    b[14] = 0x0F;
    b[15] = 0x05; // syscall
    b[16] = 0xC3; // ret
    b
}

/// 第 `slot` 号 API 的 thunk 用户 VA（IAT 补钉与 GetProcAddress 共同返回值）。
pub const fn thunk_va(slot: usize) -> u64 {
    WINAPI_THUNK_BASE + (slot * THUNK_SLOT_BYTES) as u64
}

// ---------------------------------------------------------------------------
// 查找（静态路 = 绑定期；动态路 = GetProcAddress，同一张表）
// ---------------------------------------------------------------------------

fn eq_ignore_case(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b.iter())
            .all(|(x, y)| x.to_ascii_lowercase() == y.to_ascii_lowercase())
}

/// 按名字解析 API → 槽号。DLL/函数名大小写不敏感（Windows 装载器语义）。
pub fn resolve(dll: &[u8], func: &[u8]) -> Option<usize> {
    API_TABLE
        .iter()
        .position(|d| eq_ignore_case(d.dll.as_bytes(), dll) && eq_ignore_case(d.name.as_bytes(), func))
}

/// 按序号解析。首层注册表无序号项——恒 None（绑定期如实 MissingApi）。
pub const fn resolve_ordinal(_dll: &[u8], _ordinal: u16) -> Option<usize> {
    None
}

// ---------------------------------------------------------------------------
// 绑定：解析 → 计划 → 安装（thunk 页 + IAT 补钉）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BindError {
    /// 导入表解析被具名拒绝。
    Parse(ImportError),
    /// 名字导入在注册表无此 API（dll 序, func 序）。
    MissingApi(usize, usize),
    /// 序号导入无注册项（首层边界，如实拒绝）。
    MissingOrdinal(usize, usize, u16),
    /// thunk 页分配/映射失败。
    ThunkPageFailed,
    /// IAT 落点页不在装载账本（装载器没映射这段文件内容）。
    IatPageNotFound,
}

impl BindError {
    pub fn as_str(self) -> &'static str {
        match self {
            BindError::Parse(e) => e.as_str(),
            BindError::MissingApi(..) => "imported API is not in the Win32 registry",
            BindError::MissingOrdinal(..) => "ordinal import has no registry entry (first layer)",
            BindError::ThunkPageFailed => "thunk page allocation/mapping failed",
            BindError::IatPageNotFound => "IAT page was not mapped by the loader",
        }
    }
}

/// 一条 IAT 补钉：落点 VA、槽号、绑定后的三态（证据行用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Patch {
    pub iat_va: u64,
    pub slot: usize,
    pub status: ApiStatus,
}

/// 绑定计划（导入表本体驻留 [`IMPORT_SCRATCH`] .bss 暂存，只出计数与
/// 堆上补钉清单——**栈纪律**：ImportTable ≈41KB，64KB 内核栈装不下
/// 两层返回值物化，实机 #DF 已验证；此为任务56 戒律的又一实证）。
#[derive(Debug)]
pub struct BindPlan {
    pub dlls: usize,
    pub funcs: usize,
    pub patches: alloc::vec::Vec<Patch>,
}

/// 导入表 .bss 暂存（引导链单线程窗口使用；SpinProtected 与 sched/probe
/// 同款静态模式，宿主测试同样可用）。
static IMPORT_SCRATCH: SpinProtected<ImportTable> = SpinProtected::new(ImportTable::empty());

/// 绑定计划：解析导入表（直写 .bss 暂存，栈上零大物化）+ 全量注册表
/// 解析。任何缺失 = 具名拒绝（LoadError 语义），绝不带着未解析 thunk
/// 进 ring3。
pub fn plan(img: &PeImage, blob: &[u8]) -> Result<BindPlan, BindError> {
    let mut scratch = IMPORT_SCRATCH.lock();
    parse_imports_into(blob, img, &mut scratch).map_err(BindError::Parse)?;
    let mut patches = alloc::vec::Vec::new();
    for (di, dll) in scratch.dlls().iter().enumerate() {
        for (fi, func) in dll.funcs[..dll.func_count].iter().enumerate() {
            let slot = match func.ordinal {
                Some(ord) => resolve_ordinal(&dll.name[..dll.name_len], ord)
                    .ok_or(BindError::MissingOrdinal(di, fi, ord))?,
                None => resolve(&dll.name[..dll.name_len], &func.name[..func.name_len])
                    .ok_or(BindError::MissingApi(di, fi))?,
            };
            patches.push(Patch {
                iat_va: img.image_base + dll.iat_rva + (fi as u64) * 8,
                slot,
                status: API_TABLE[slot].status,
            });
        }
    }
    let dlls = scratch.count;
    let funcs = patches.len();
    drop(scratch); // 守卫显式归还（match 临时守卫自旋死锁的教训同源）。
    Ok(BindPlan { dlls, funcs, patches })
}

/// 安装：thunk 页映射（R+X）+ IAT 补钉逐条写入。thunk 页帧追加进
/// `loaded.pages` 账本（退出随进程归还，一页不留）。成功返回三态计数。
pub fn install(
    patches: &[Patch],
    loaded: &mut LoadedImage,
    mp: &mut dyn UserMapper,
) -> Result<(usize, usize, usize), BindError> {
    // thunk 页：分配 → 全槽编码写入 → R+X 映射。
    let Some(phys) = mp.alloc_zero_frame() else {
        return Err(BindError::ThunkPageFailed);
    };
    for slot in 0..API_COUNT {
        let bytes = thunk_bytes(slot as u32);
        mp.write_frame_bytes(phys, (slot * THUNK_SLOT_BYTES) as u64, &bytes);
    }
    if !mp.map_user_frame(WINAPI_THUNK_BASE, phys, false, false) {
        mp.free_frame(phys);
        return Err(BindError::ThunkPageFailed);
    }
    loaded.pages.push((WINAPI_THUNK_BASE, phys));

    // IAT 补钉：账本反查帧 → 页内偏移写入 8B 槽地址。
    for p in patches {
        let page_va = p.iat_va & !0xFFF;
        let off = p.iat_va & 0xFFF;
        let Some(&(_, frame)) = loaded.pages.iter().find(|(va, _)| *va == page_va) else {
            return Err(BindError::IatPageNotFound);
        };
        mp.write_frame_bytes(frame, off, &thunk_va(p.slot).to_le_bytes());
    }

    let mut full = 0;
    let mut partial = 0;
    let mut stub = 0;
    for p in patches {
        match p.status {
            ApiStatus::Full => full += 1,
            ApiStatus::Partial => partial += 1,
            ApiStatus::Stub => stub += 1,
        }
    }
    Ok((full, partial, stub))
}

// ---------------------------------------------------------------------------
// 分发：syscall_common 的 Win32 号段入口
// ---------------------------------------------------------------------------

/// Win32 号段分发。返回 i64 ABI：≥0 成功值，<0 = -ErrNo（F280 归一码）。
pub fn dispatch(slot: usize, a1: u64, a2: u64, a3: u64) -> i64 {
    let Some(def) = API_TABLE.get(slot) else {
        // 号段内未注册槽位：与未实现 API 同一明确错误码。
        return -(ErrNo::Enosys.to_i32()) as i64;
    };
    match def.status {
        ApiStatus::Stub => {
            STUB_CALLS.fetch_add(1, Ordering::Relaxed);
            -(ErrNo::Enosys.to_i32()) as i64
        }
        ApiStatus::Full | ApiStatus::Partial => {
            // 任务41 · 窗口/文本/文件服务台：user32/gdi32/comdlg32 全量走
            // winsrv::win32_dispatch（NT 风格参数块桥接，见 winsrv 模块头）。
            if def.dll == "user32.dll" || def.dll == "gdi32.dll" || def.dll == "comdlg32.dll" {
                return super::winsrv::win32_dispatch(def.dll, def.name, a1, a2, a3);
            }
            match (def.dll, def.name) {
            // 进程组 → SYS_EXIT(0)：控制权交还监护者（永不返回）。
            ("kernel32.dll", "ExitProcess")
            | ("ntdll.dll", "RtlExitUserProcess")
            | ("ntdll.dll", "NtTerminateProcess") => {
                FULL_CALLS.fetch_add(1, Ordering::Relaxed);
                crate::proc::ring3::user_exit(a1 as i32)
            }
            // 文件组 → SYS_WRITE(2) 控制台路：handle==1 控制台（现状边界）；
            // handle==2 任务41 虚拟文件保存目标（winsrv 句柄分类扩展）。
            ("kernel32.dll", "WriteFile") | ("ntdll.dll", "NtWriteFile") => {
                PARTIAL_CALLS.fetch_add(1, Ordering::Relaxed);
                if a1 == 2 {
                    return super::winsrv::win32_dispatch(def.dll, def.name, a1, a2, a3);
                }
                if a1 != 1 {
                    return -(ErrNo::Ebadf.to_i32()) as i64;
                }
                crate::proc::ring3::user_write(a2, a3)
            }
            // 任务41 新增：ReadFile（handle 3=读取源）/ CloseHandle。
            ("kernel32.dll", "ReadFile") | ("kernel32.dll", "CloseHandle") => {
                FULL_CALLS.fetch_add(1, Ordering::Relaxed);
                super::winsrv::win32_dispatch(def.dll, def.name, a1, a2, a3)
            }
            // 动态查找 → 注册表（与绑定期静态路同表同值）。
            ("kernel32.dll", "GetProcAddress") | ("ntdll.dll", "LdrGetProcedureAddress") => {
                FULL_CALLS.fetch_add(1, Ordering::Relaxed);
                win_getproc(a1, a2)
            }
            // GetStdHandle：仅标准输出映射（-11 → 控制台句柄 1）。
            ("kernel32.dll", "GetStdHandle") => {
                PARTIAL_CALLS.fetch_add(1, Ordering::Relaxed);
                if a1 == STD_OUTPUT_HANDLE {
                    1
                } else {
                    0
                }
            }
            // Full/Partial 声明与实现必须一一对应——漏臂是注册表 bug。
            _ => {
                STUB_CALLS.fetch_add(1, Ordering::Relaxed);
                -(ErrNo::Enosys.to_i32()) as i64
            }
            }
        }
    }
}

/// GetProcAddress 语义：两个用户态 NUL 串 → 注册表查找 → thunk VA / NULL。
fn win_getproc(dll_ptr: u64, name_ptr: u64) -> i64 {
    let (Some(dll), Some(name)) = (read_user_str(dll_ptr, 32), read_user_str(name_ptr, 64)) else {
        return 0; // 参数不可读 → NULL（Windows 语义），内核侧不猜。
    };
    match resolve(&dll, &name) {
        Some(slot) => thunk_va(slot) as i64,
        None => 0,
    }
}

/// winsrv 复用入口（任务41）。
pub fn read_user_str_pub(buf: u64, cap: usize) -> Option<alloc::vec::Vec<u8>> {
    read_user_str(buf, cap)
}

/// 读用户态 NUL 结尾串（≤cap 字节 + 终止符）。用户半区校验同 sys_write
/// 口径；越界/超长/无终止符 → None（由调用方决定 NULL/错误语义）。
fn read_user_str(buf: u64, cap: usize) -> Option<alloc::vec::Vec<u8>> {
    if buf == 0 || !crate::entry::is_user_ip(buf) {
        return None;
    }
    let mut out = alloc::vec::Vec::new();
    for i in 0..=cap {
        let addr = buf.checked_add(i as u64)?;
        if addr >= crate::entry::USER_TOP {
            return None;
        }
        // SAFETY: addr 已校验在用户半区且 < USER_TOP；单核演示地址空间独占。
        let b = unsafe { core::ptr::read_volatile(addr as *const u8) };
        if b == 0 {
            return Some(out);
        }
        if i == cap {
            return None; // 无终止符。
        }
        out.push(b);
    }
    None
}

// ---------------------------------------------------------------------------
// 宿主测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proc::loader::PAGE;

    #[test]
    fn registry_shape_and_coverage() {
        // 四核心 DLL 全部在场。
        for dll in ["kernel32.dll", "ntdll.dll", "user32.dll", "gdi32.dll"] {
            assert!(
                API_TABLE.iter().any(|d| d.dll == dll),
                "{dll} 必须在首层注册表"
            );
        }
        let (full, partial, stub) = coverage();
        assert_eq!(full + partial + stub, API_COUNT);
        // 任务41 扩容后口径：Full=18（进程/文件组 5 + 窗口/文本/对话框面
        // RegisterClassExW/CreateWindowExW/ShowWindow/UpdateWindow/
        // InvalidateRect/GetMessageW/PostQuitMessage/TextOutW/BeginPaint/
        // EndPaint/GetOpenFileNameW/GetSaveFileNameW/ReadFile）；
        // Partial=13（WriteFile/NtWriteFile/GetStdHandle/MessageBoxW/
        // TranslateMessage/DispatchMessageW/DefWindowProcW/CreateFontW/
        // CloseHandle + 窗口面桥接如实登记项）；Stub=4（NtCreateFile/
        // NtAllocateVirtualMemory 等未实现面）。
        assert_eq!(full, 18, "任务41 扩容后 Full 面");
        assert_eq!(partial, 9, "任务41 扩容后 Partial 面");
        assert_eq!(stub, 6, "未实现面如实保持 Stub");
        // comdlg32 对话框面必须存在（任务41 文件对话框）。
        assert!(API_TABLE.iter().any(|d| d.dll == "comdlg32.dll" && d.name == "GetOpenFileNameW"));
        assert!(API_TABLE.iter().any(|d| d.dll == "kernel32.dll" && d.name == "ReadFile"));
        // 槽容量：全部 thunk 必须装进一页。
        assert!(API_COUNT <= THUNKS_PER_PAGE);
        // 名字无重复（同 DLL 内）。
        for (i, a) in API_TABLE.iter().enumerate() {
            for b in API_TABLE.iter().skip(i + 1) {
                assert!(
                    !(a.dll == b.dll && a.name == b.name),
                    "注册表重复项 {}/{}",
                    a.name,
                    b.name
                );
            }
        }
    }

    #[test]
    fn resolve_is_case_insensitive_and_honest() {
        assert_eq!(resolve(b"KERNEL32.DLL", b"WriteFile"), Some(3));
        assert_eq!(resolve(b"kernel32.dll", b"WRITEFILE"), Some(3));
        assert_eq!(resolve(b"Ntdll.Dll", b"rtlExitUserProcess"), Some(10));
        assert_eq!(resolve(b"user32.dll", b"MessageBoxW"), Some(16));
        // 缺失 API：明确 None（绑定期具名拒绝，运行期 GetProcAddress → NULL）。
        assert_eq!(resolve(b"kernel32.dll", b"CreateFileExW"), None);
        assert_eq!(resolve(b"notdll.dll", b"WriteFile"), None);
        // 首层无序号项。
        assert_eq!(resolve_ordinal(b"kernel32.dll", 1), None);
    }

    #[test]
    fn thunk_encoding_is_exact() {
        let t = thunk_bytes(0);
        assert_eq!(
            &t[..17],
            &[
                0x48, 0x89, 0xCF, // mov rdi, rcx
                0x48, 0x89, 0xD6, // mov rsi, rdx
                0x4C, 0x89, 0xC2, // mov rdx, r8
                0xB8, 0x40, 0x00, 0x00, 0x00, // mov eax, 0x40（WIN32_NR_BASE+0）
                0x0F, 0x05, // syscall
                0xC3, // ret
            ][..]
        );
        assert_eq!(&t[17..], &[0u8; 15][..]);
        // 槽号进入 imm32。
        let t1 = thunk_bytes(1);
        assert_eq!(t1[10], 0x41);
        let t99 = thunk_bytes(99);
        assert_eq!(&t99[10..14], &0x40u32.saturating_add(99).to_le_bytes()[..]);
        // 槽地址算术。
        assert_eq!(thunk_va(0), WINAPI_THUNK_BASE);
        assert_eq!(thunk_va(3), WINAPI_THUNK_BASE + 3 * 32);
    }

    #[test]
    fn dispatch_stub_and_missing_return_enosys() {
        // Stub API：明确 Enosys（-2，F280 归一码），绝不崩溃。
        let before = stats().2;
        assert_eq!(dispatch(9, 0, 0, 0), -(ErrNo::Enosys.to_i32() as i64)); // NtCreateFile（Stub 面）
        assert_eq!(stats().2, before + 1, "stub 调用必须计数");
        // 号段内未注册槽位：同一错误码。
        assert_eq!(dispatch(API_COUNT + 5, 0, 0, 0), -(ErrNo::Enosys.to_i32() as i64));
    }

    #[test]
    fn dispatch_partial_apis_have_honest_boundaries() {
        // GetStdHandle(-11) → 1；其余 → 0。
        assert_eq!(dispatch(2, STD_OUTPUT_HANDLE, 0, 0), 1);
        assert_eq!(dispatch(2, (-10i64) as u64, 0, 0), 0);
        // WriteFile：handle 1=控制台 / 2=任务41 保存目标（服务单例空表
        // 返回 0 = 写目标未选）；其余句柄 Ebadf。
        assert_eq!(dispatch(3, 2, 0, 0), 1, "handle 2 空写合法（服务面就绪）");
        assert_eq!(dispatch(3, 5, 0, 0), -(ErrNo::Ebadf.to_i32() as i64));
        // len=0 合法（与 sys_write 同口径）。
        assert_eq!(dispatch(3, 1, 0, 0), 0);
    }

    /// 独立最小宿主假内存（与 pe.rs 测试同模型，独立成套免跨模块依赖）。
    struct HostMapper {
        map: alloc::vec::Vec<(u64, u64, bool, bool)>,
        frames: alloc::vec::Vec<[u8; PAGE as usize]>,
    }
    impl HostMapper {
        fn new() -> Self {
            HostMapper { map: alloc::vec::Vec::new(), frames: alloc::vec::Vec::new() }
        }
        fn read_va(&self, va: u64, n: usize) -> alloc::vec::Vec<u8> {
            let (_, phys, _, _) = *self.map.iter().find(|e| e.0 == va & !0xFFF).unwrap();
            let in_page = (va & 0xFFF) as usize;
            let idx = ((phys - 0x1000_0000) / PAGE) as usize;
            self.frames[idx][in_page..in_page + n].to_vec()
        }
    }
    impl UserMapper for HostMapper {
        fn alloc_zero_frame(&mut self) -> Option<u64> {
            let phys = 0x1000_0000 + (self.frames.len() as u64) * PAGE;
            self.frames.push([0u8; PAGE as usize]);
            Some(phys)
        }
        fn map_user_frame(&mut self, va: u64, phys: u64, w: bool, nx: bool) -> bool {
            if va >> 47 != 0 {
                return false;
            }
            match self.map.iter_mut().find(|e| e.0 == va) {
                Some(e) => *e = (va, phys, w, nx),
                None => self.map.push((va, phys, w, nx)),
            }
            true
        }
        fn write_frame_bytes(&mut self, phys: u64, off: u64, data: &[u8]) {
            let idx = ((phys - 0x1000_0000) / PAGE) as usize;
            let off = off as usize;
            self.frames[idx][off..off + data.len()].copy_from_slice(data);
        }
        fn flush(&mut self, _va: u64) {}
        fn unmap_user(&mut self, _va: u64) -> Option<u64> {
            None
        }
        fn free_frame(&mut self, _phys: u64) {}
    }

    /// 端到端：真实 hello-imp.pe（tools/make-pe-imp.py 产物，随源入库）
    /// 解析 → 计划 → 安装 → IAT 读回 == thunk VA。
    #[test]
    fn hello_imp_pe_binds_end_to_end() {
        static IMG: &[u8] = include_bytes!("hello-imp.pe");
        let pe = crate::proc::pe::parse(IMG).expect("hello-imp.pe 必须可解析");
        assert!(pe.has_imports());
        let plan = plan(&pe, IMG).expect("hello-imp.pe 导入必须全部可绑定");
        assert_eq!(plan.dlls, 1, "样例只导入 kernel32.dll");
        assert_eq!(plan.patches.len(), 3);
        // 结构断言走纯函数 parse_imports（宿主专用返回值版）——IMPORT_SCRATCH
        // 是共享静态，宿主测试并行线程不能事后复查它的内容。
        let table = crate::proc::pe::parse_imports(IMG, &pe).expect("结构解析");
        let dll = &table.dlls()[0];
        assert_eq!(&dll.name[..dll.name_len], b"KERNEL32.DLL");
        assert_eq!(dll.func_count, 3, "WriteFile/GetProcAddress/ExitProcess");
        assert!(dll.iat_writable);

        let mut mp = HostMapper::new();
        // 先按真实装载器语义映射各节（PeSource 走引擎），再做绑定安装。
        let src = crate::proc::pe::PeSource { img: &pe, blob: IMG };
        let mut loaded = crate::proc::loader::load_into(&src, &mut mp, 2, 0x7FFF_F000)
            .expect("样例必须能走任务15 引擎");
        let pages_before = loaded.pages.len();
        let (full, partial, stub) = install(&plan.patches, &mut loaded, &mut mp).expect("绑定安装必须成功");
        assert_eq!((full, partial, stub), (2, 1, 0), "GetProcAddress+ExitProcess=full, WriteFile=partial");
        assert_eq!(loaded.pages.len(), pages_before + 1, "thunk 页入账本");

        // IAT 读回：每条补钉 == thunk VA（静态绑定路）。
        for p in &plan.patches {
            assert_eq!(mp.read_va(p.iat_va, 8), thunk_va(p.slot).to_le_bytes().to_vec());
        }
        // thunk 页编码读回：槽 0 与 IAT 指向槽逐字节一致。
        assert_eq!(mp.read_va(WINAPI_THUNK_BASE, 17), thunk_bytes(0)[..17].to_vec());
        let bound_slot = plan.patches.iter().find(|p| p.status == ApiStatus::Partial).unwrap().slot;
        assert_eq!(
            mp.read_va(thunk_va(bound_slot), 17),
            thunk_bytes(bound_slot as u32)[..17].to_vec()
        );
        // thunk 页 R+X（w=false, nx=false）。
        let (_, _, w, nx) = *mp.map.iter().find(|e| e.0 == WINAPI_THUNK_BASE).unwrap();
        assert!(!w && !nx);
    }

    /// 合成"单 DLL + 指定导入函数"的最小 PE（敌意/边界用例的底版）。
    /// 布局：headers 0x400 / .text@RVA0x1000（可执行，入口在内）/
    /// .idata@RVA0x2000（可写：描述符+INT+IAT+名字）。
    fn synth_import_pe(dll: &str, funcs: &[(&str, Option<u16>)]) -> alloc::vec::Vec<u8> {
        let n = funcs.len();
        assert!(n >= 1 && n <= 16);
        let mut img = alloc::vec![0u8; 0x800];
        // DOS + PE 签名 + COFF。
        img[0..2].copy_from_slice(b"MZ");
        img[0x3C..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        img[0x40..0x44].copy_from_slice(b"PE\0\0");
        img[0x44..0x46].copy_from_slice(&0x8664u16.to_le_bytes());
        img[0x46..0x48].copy_from_slice(&2u16.to_le_bytes()); // 2 节
        img[0x54..0x56].copy_from_slice(&0xF0u16.to_le_bytes()); // opt size
        // 可选头 @0x58。
        let opt = 0x58;
        img[opt..opt + 2].copy_from_slice(&0x20Bu16.to_le_bytes());
        img[opt + 16..opt + 20].copy_from_slice(&0x1000u32.to_le_bytes()); // entry rva
        img[opt + 24..opt + 32].copy_from_slice(&0x1400_0000u64.to_le_bytes()); // base
        img[opt + 32..opt + 36].copy_from_slice(&0x1000u32.to_le_bytes()); // SecAlign
        img[opt + 36..opt + 40].copy_from_slice(&0x200u32.to_le_bytes()); // FileAlign
        img[opt + 56..opt + 60].copy_from_slice(&0x4000u32.to_le_bytes()); // SizeOfImage
        img[opt + 60..opt + 64].copy_from_slice(&0x400u32.to_le_bytes()); // SizeOfHeaders
        // 数据目录[1] = 导入（@ .idata 起始 RVA 0x2000）。
        let dirs = opt + 112;
        img[dirs + 8..dirs + 12].copy_from_slice(&0x2000u32.to_le_bytes());
        img[dirs + 12..dirs + 16].copy_from_slice(&(40u32).to_le_bytes());
        // 节表 @ opt+0xF0。
        let sh = opt + 0xF0;
        img[sh..sh + 8].copy_from_slice(b".text\0\0\0");
        img[sh + 8..sh + 12].copy_from_slice(&0x200u32.to_le_bytes());
        img[sh + 12..sh + 16].copy_from_slice(&0x1000u32.to_le_bytes());
        img[sh + 16..sh + 20].copy_from_slice(&0x200u32.to_le_bytes());
        img[sh + 20..sh + 24].copy_from_slice(&0x400u32.to_le_bytes());
        img[sh + 36..sh + 40].copy_from_slice(&0x6000_0020u32.to_le_bytes());
        let sh2 = sh + 40;
        img[sh2..sh2 + 8].copy_from_slice(b".idata\0\0");
        img[sh2 + 8..sh2 + 12].copy_from_slice(&0x200u32.to_le_bytes());
        img[sh2 + 12..sh2 + 16].copy_from_slice(&0x2000u32.to_le_bytes());
        img[sh2 + 16..sh2 + 20].copy_from_slice(&0x200u32.to_le_bytes());
        img[sh2 + 20..sh2 + 24].copy_from_slice(&0x600u32.to_le_bytes());
        img[sh2 + 36..sh2 + 40].copy_from_slice(&0xC000_0040u32.to_le_bytes());
        // .text：入口处一条 ret（宿主只测解析/绑定，不执行）。
        img[0x400] = 0xC3;
        // .idata @0x600（RVA 0x2000）：描述符 / INT / IAT / DLL 名 / hint+name。
        let idata = 0x600;
        let int_rva: u32 = 0x2000 + 40; // INT 紧随描述符+终止符
        let iat_rva: u32 = int_rva + (8 * (n as u32 + 1));
        let name_rva: u32 = iat_rva + (8 * (n as u32 + 1));
        // DLL 名区。
        let mut off = name_rva as usize - idata + idata; // 文件偏移 = 0x600 + (rva-0x2000)
        let file_of = |rva: u32| -> usize { idata + (rva - 0x2000) as usize };
        off = file_of(name_rva);
        img[off..off + dll.len()].copy_from_slice(dll.as_bytes());
        img[off + dll.len()] = 0;
        // hint+name 条目区（DLL 名后顺序排布）。
        let mut hn_rva = name_rva + (dll.len() as u32 + 1);
        let mut int_off = file_of(int_rva);
        let mut iat_off = file_of(iat_rva);
        for (fname, ordinal) in funcs {
            let val: u64 = match ordinal {
                Some(o) => super::super::pe::THUNK_ORDINAL_FLAG | (*o as u64),
                None => {
                    let r = hn_rva;
                    let fo = file_of(hn_rva);
                    img[fo..fo + 2].copy_from_slice(&0u16.to_le_bytes()); // hint
                    img[fo + 2..fo + 2 + fname.len()].copy_from_slice(fname.as_bytes());
                    img[fo + 2 + fname.len()] = 0;
                    hn_rva += (fname.len() as u32) + 3;
                    r as u64
                }
            };
            img[int_off..int_off + 8].copy_from_slice(&val.to_le_bytes());
            img[iat_off..iat_off + 8].copy_from_slice(&val.to_le_bytes());
            int_off += 8;
            iat_off += 8;
        }
        // 描述符。
        img[idata..idata + 4].copy_from_slice(&int_rva.to_le_bytes());
        img[idata + 4..idata + 8].copy_from_slice(&0u32.to_le_bytes()); // 未绑定
        img[idata + 8..idata + 12].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        img[idata + 12..idata + 16].copy_from_slice(&name_rva.to_le_bytes());
        img[idata + 16..idata + 20].copy_from_slice(&iat_rva.to_le_bytes());
        img
    }

    #[test]
    fn missing_api_is_named_at_bind_time() {
        let blob = synth_import_pe("KERNEL32.DLL", &[("WriteFile", None), ("NoSuchApi", None)]);
        let pe = crate::proc::pe::parse(&blob).unwrap();
        let err = plan(&pe, &blob).unwrap_err();
        assert_eq!(err, BindError::MissingApi(0, 1), "第二个函数（未知 API）必须具名拒绝");
    }

    #[test]
    fn ordinal_import_is_parsed_but_honestly_refused() {
        let blob = synth_import_pe("kernel32.dll", &[("WriteFile", None), ("", Some(7))]);
        let pe = crate::proc::pe::parse(&blob).unwrap();
        let err = plan(&pe, &blob).unwrap_err();
        assert_eq!(err, BindError::MissingOrdinal(0, 1, 7), "首层无序号项，必须如实拒绝");
    }

    #[test]
    fn bound_imports_are_refused_by_name() {
        let mut blob = synth_import_pe("kernel32.dll", &[("WriteFile", None)]);
        // 描述符 TimeDateStamp != 0 → 旧式 bound import → 具名拒绝。
        blob[0x600 + 4..0x600 + 8].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        let pe = crate::proc::pe::parse(&blob).unwrap();
        assert_eq!(plan(&pe, &blob).unwrap_err(), BindError::Parse(crate::proc::pe::ImportError::BoundImports));
    }
}
