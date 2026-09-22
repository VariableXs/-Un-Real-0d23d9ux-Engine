//! UEFI BootNext 一键切 Windows（双域总案·阶段0 任务2）。
//!
//! 职责（刻意最小、每一步都可实测）：
//! - UEFI 引导时：向全局变量区写 `BootNext`（GUID 8BE4DF61-93CA-11D2-AA0D-
//!   00E098032B8C，属性 NV|BS|RT = 0x7），**读回校验**，然后
//!   `ResetSystem(EfiResetCold)` 重启——固件重启后按 BootNext 引导一次目标项。
//! - BIOS 引导时：没有 UEFI Runtime Services，如实返回 `NoRuntimeServices`，
//!   绝不假装切换成功（调用方继续引导 varix 并记录）。
//!
//! 事实依据：EFI 系统表布局（x64）：Hdr 24B + FirmwareVendor + FirmwareRevision
//! (对齐 pad) → RuntimeServices @ +88；RuntimeServices 内偏移：
//! GetVariable @ +72、SetVariable @ +88、ResetSystem @ +104。
//! 运行期调用约定为 Microsoft x64（Rust `extern "win64"`）；Limine 不调用
//! ExitBootServices、以 HHDM 提供运行期区域访问，物理地址可直接调用。

use crate::limine;

/// EFI_STATUS 成功码。
const EFI_SUCCESS: usize = 0;
/// BootNext 变量属性：NonVolatile | BootserviceAccess | RuntimeAccess。
const ATTR_NV_BS_RT: u32 = 0x7;
/// EfiResetCold。
const EFI_RESET_COLD: u32 = 1;
/// EfiResetShutdown（SYS_POWEROFF 主路径：固件级断电）。
const EFI_RESET_SHUTDOWN: u32 = 2;

// ---------------------------------------------------------------------------
// UEFI Runtime Services 区域的恒等映射（任务2 前置事实）
// ---------------------------------------------------------------------------
// 固件的 Runtime Services 代码在 SetVirtualAddressMap 之前以**绝对物理地址**
// 自引用；内核页表只有 HHDM 映射（phys+0xffff8000…），没有低区恒等映射，
// 直接调用会在 RS 内部 #PF。因此调用前须把 EfiRuntimeServicesCode/Data
// 区域按 phys→phys 恒等映射进**当前生效的** CR3 页表（Limine 交付的表）。

#[cfg(target_os = "none")]
const EFI_MEMORY_RUNTIME_CODE: u32 = 5;
#[cfg(target_os = "none")]
const EFI_MEMORY_RUNTIME_DATA: u32 = 6;

#[cfg(target_os = "none")]
mod ident {
    use crate::mem::paging::{P_ADDR_MASK, P_PRESENT, P_WRITE};

    const PAGE: u64 = 4096;
    const P_HUGE: u64 = 1 << 7;

    pub fn hhdm() -> Option<u64> {
        crate::limine::hhdm_offset()
    }

    /// 活跃 CR3 → HHDM 化的 PML4 虚拟地址。
    unsafe fn root_table() -> Option<u64> {
        let cr3: u64;
        unsafe {
            core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags));
        }
        let off = hhdm()?;
        Some((cr3 & P_ADDR_MASK) + off)
    }

    /// 从 PMM 拿一个零化页帧，返回物理地址。
    unsafe fn zalloc_frame() -> Option<u64> {
        let phys = crate::mem::pmm::alloc_page()?;
        let virt = phys + hhdm()?;
        unsafe {
            core::ptr::write_bytes(virt as *mut u8, 0, PAGE as usize);
        }
        Some(phys)
    }

    /// 确保从 root 出发、va 的 PML4/PDPT/PD 三级表存在（缺则分配零页）。
    /// 返回 PD 表项的 HHDM 虚拟地址（供写 2MiB/4KiB 叶子）。
    unsafe fn ensure_tables(root: u64, va: u64) -> Option<*mut u64> {
        let mut table = root;
        // 级别索引：PML4(39) → PDPT(30) → PD(21)
        for shift in [39u64, 30u64] {
            let idx = ((va >> shift) & 0x1FF) as usize;
            let e_ptr = (table + (idx as u64) * 8) as *mut u64;
            let e = unsafe { core::ptr::read_volatile(e_ptr) };
            if e & P_PRESENT == 0 {
                let phys = unsafe { zalloc_frame() }?;
                unsafe {
                    core::ptr::write_volatile(e_ptr, phys | P_PRESENT | P_WRITE);
                }
            } else if e & P_HUGE != 0 {
                // 上层已是 1GiB/2MiB 大叶：恒等映射已在位，本函数无需求细分。
                return None;
            }
            table = (e & P_ADDR_MASK) + hhdm()?;
        }
        let idx = ((va >> 21) & 0x1FF) as usize;
        Some((table + (idx as u64) * 8) as *mut u64)
    }

    /// 把 [start, end) 物理区域恒等映射（2MiB 粒度，RWX）。返回映射的 2MiB 块数。
    pub fn identity_map_runtime(start: u64, end: u64) -> usize {
        let Some(root) = (unsafe { root_table() }) else {
            return 0;
        };
        let Some(off) = hhdm() else {
            return 0;
        };
        let mut mapped = 0usize;
        let s = start & !(2 * 1024 * 1024 - 1);
        let e = (end + 2 * 1024 * 1024 - 1) & !(2 * 1024 * 1024 - 1);
        let mut va = s;
        while va < e {
            unsafe {
                if let Some(pd_e) = ensure_tables(root, va) {
                    let e = core::ptr::read_volatile(pd_e);
                    if e & P_PRESENT == 0 || e & P_HUGE != 0 {
                        // 写 2MiB 恒等叶（可写可执行，无 NX）
                        core::ptr::write_volatile(pd_e, va | P_PRESENT | P_WRITE | P_HUGE);
                        mapped += 1;
                    } else {
                        // 已有 4KiB 页表：逐项确保 present+write+可执行
                        let pt = (e & P_ADDR_MASK) + off;
                        for k in 0..512u64 {
                            let p4 = (pt + k * 8) as *mut u64;
                            let v = core::ptr::read_volatile(p4);
                            if v & P_PRESENT != 0 {
                                let nv = (v & !crate::mem::paging::P_NX) | P_WRITE;
                                if nv != v {
                                    core::ptr::write_volatile(p4, nv);
                                }
                            }
                        }
                        mapped += 1;
                    }
                }
            }
            va += 2 * 1024 * 1024;
        }
        mapped
    }
}

/// 解析 Limine 的 EFI 内存映射，对所有 RuntimeServices 区域建恒等映射。
/// 返回映射的 2MiB 块数（BIOS 引导恒 0）。
#[cfg(target_os = "none")]
pub fn prepare_runtime_identity_map() -> usize {
    let (memmap, memmap_size, desc_size, _ver) = match limine::efi_memmap() {
        Some(v) => v,
        None => return 0,
    };
    let base = memmap as usize;
    let count = (memmap_size as usize) / (desc_size as usize);
    let mut mapped = 0usize;
    for i in 0..count {
        let d = unsafe { &*((base + i * desc_size as usize) as *const EfiMemoryDesc) };
        if d.kind == EFI_MEMORY_RUNTIME_CODE || d.kind == EFI_MEMORY_RUNTIME_DATA {
            mapped += ident::identity_map_runtime(d.phys_start, d.phys_start + d.pages * 4096);
        }
    }
    mapped
}

#[cfg(not(target_os = "none"))]
pub fn prepare_runtime_identity_map() -> usize {
    0
}

/// 低 4GiB 全区恒等映射（2MiB 粒度；含 NV 变量 flash 等内存映射区域）。
///
/// 为什么不止映射 RuntimeServices 区域：Limine 不调用 ExitBootServices，固件的
/// 引导期数据结构（如变量驱动全局）仍在 EfiBootServicesData 里被 RS 代码以
/// 绝对物理地址访问；低 1GiB 只需 1×PDPT + 1×PD 共 512 个 2MiB 叶子，一次到位。
#[cfg(target_os = "none")]
pub fn identity_map_low_4gib() -> usize {
    // BIOS 引导没有 Runtime Services，无需（也不应）动页表。
    if limine::efi_system_table().is_none() {
        return 0;
    }
    ident::identity_map_runtime(0, 0x1_0000_0000)
}

#[cfg(not(target_os = "none"))]
pub fn identity_map_low_4gib() -> usize {
    0
}

#[cfg(target_os = "none")]
#[repr(C)]
struct EfiMemoryDesc {
    kind: u32,
    _pad: u32,
    phys_start: u64,
    virt_start: u64,
    pages: u64,
    attrs: u64,
}

/// 全局变量 vendor GUID（EFI_GLOBAL_VARIABLE）小端字节序。
pub const GLOBAL_VARIABLE_GUID: [u8; 16] = [
    0x61, 0xDF, 0xE4, 0x8B, // Data1 (LE)
    0xCA, 0x93, // Data2 (LE)
    0xD2, 0x11, // Data3 (LE)
    0xAA, 0x0D, 0x00, 0xE0, 0x98, 0x03, 0x2B, 0x8C, // Data4
];

/// BootNext 的 CHAR16（UTF-16LE + NUL）名字。
pub const BOOTNEXT_NAME: [u16; 9] = [
    b'B' as u16, b'o' as u16, b'o' as u16, b't' as u16, b'N' as u16, b'e' as u16, b'x' as u16,
    b't' as u16, 0,
];

/// EFI_SYSTEM_TABLE：RuntimeServices 指针位于偏移 88。
const ST_RUNTIME_SERVICES: usize = 88;
/// EFI_RUNTIME_SERVICES 内的函数偏移（每项一个 8B 指针）。
const RS_GET_VARIABLE: usize = 72;
const RS_SET_VARIABLE: usize = 88;
const RS_RESET_SYSTEM: usize = 104;

/// BootNext 写入结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootNextOutcome {
    /// 写入成功且读回一致。
    Written { entry: u16 },
    /// 非 UEFI 引导（无 Runtime Services）。
    NoRuntimeServices,
    /// SetVariable 失败（status 为 EFI_STATUS）。
    SetFailed(usize),
    /// 写入后读回不一致。
    ReadbackMismatch { wrote: u16, read: u16 },
}

type EfiGetVariable = unsafe extern "win64" fn(
    name: *const u16,
    guid: *const u8,
    attrs: *mut u32,
    size: *mut usize,
    data: *mut u16,
) -> usize;
type EfiSetVariable = unsafe extern "win64" fn(
    name: *const u16,
    guid: *const u8,
    attrs: u32,
    size: usize,
    data: *const u16,
) -> usize;
type EfiResetSystem =
    unsafe extern "win64" fn(reset_type: u32, status: usize, data_size: usize, data: *const u8);

/// Limine 交付的 EFI 系统表指针**已是 HHDM 虚拟地址**（实测 st=0xffff8000…）；
/// 而 ST 内的 RuntimeServices 指针仍是物理地址（固件原样），访问/调用前须
/// 加 HHDM 偏移。
fn hhdm() -> Option<usize> {
    limine::hhdm_offset().map(|o| o as usize)
}

/// ST 内的物理指针 → HHDM 虚拟地址。
fn hhdm_ptr(p: u64) -> Option<usize> {
    let off = hhdm()?;
    if p == 0 {
        return None;
    }
    Some(p as usize + off)
}

fn runtime_services() -> Option<usize> {
    // 系统表指针直接用（Limine 已 HHDM 化）；调试日志见 git 历史。
    let st = limine::efi_system_table()? as usize;
    let rs = unsafe { core::ptr::read_volatile((st + ST_RUNTIME_SERVICES) as *const u64) };
    hhdm_ptr(rs)
}

fn fn_ptr<T>(rs: usize, offset: usize) -> Option<T> {
    let p = unsafe { core::ptr::read_volatile((rs + offset) as *const u64) };
    if p == 0 {
        return None;
    }
    Some(unsafe { core::mem::transmute_copy::<u64, T>(&(p as u64 + hhdm()? as u64)) })
}

/// 写 BootNext 并读回校验。只在 UEFI 引导下有效。
pub fn write_bootnext(entry: u16) -> BootNextOutcome {
    let Some(rs) = runtime_services() else {
        return BootNextOutcome::NoRuntimeServices;
    };
    let Some(set_var) = fn_ptr::<EfiSetVariable>(rs, RS_SET_VARIABLE) else {
        return BootNextOutcome::NoRuntimeServices;
    };
    let status = unsafe {
        set_var(
            BOOTNEXT_NAME.as_ptr(),
            GLOBAL_VARIABLE_GUID.as_ptr(),
            ATTR_NV_BS_RT,
            core::mem::size_of::<u16>(),
            &entry,
        )
    };
    if status != EFI_SUCCESS {
        return BootNextOutcome::SetFailed(status);
    }
    // 读回校验：GetVariable 写到同一变量。
    let Some(get_var) = fn_ptr::<EfiGetVariable>(rs, RS_GET_VARIABLE) else {
        return BootNextOutcome::NoRuntimeServices;
    };
    let mut buf: u16 = 0;
    let mut size = core::mem::size_of::<u16>();
    let mut attrs: u32 = 0;
    let st = unsafe {
        get_var(
            BOOTNEXT_NAME.as_ptr(),
            GLOBAL_VARIABLE_GUID.as_ptr(),
            &mut attrs,
            &mut size,
            &mut buf,
        )
    };
    if st != EFI_SUCCESS || buf != entry {
        return BootNextOutcome::ReadbackMismatch {
            wrote: entry,
            read: buf,
        };
    }
    BootNextOutcome::Written { entry }
}

/// ResetSystem(EfiResetCold)。仅在 UEFI 引导下有效；BIOS 引导返回 false。
pub fn reset_cold() -> bool {
    let Some(rs) = runtime_services() else {
        return false;
    };
    let Some(reset) = fn_ptr::<EfiResetSystem>(rs, RS_RESET_SYSTEM) else {
        return false;
    };
    unsafe { reset(EFI_RESET_COLD, EFI_SUCCESS, 0, core::ptr::null()) };
    // ResetSystem 理论上不返回；若返回，调用方继续（如实）。
    true
}

/// ResetSystem(EfiResetShutdown)：固件级断电（SYS_POWEROFF ①）。
/// 仅在 UEFI 引导下有效；BIOS 引导返回 false（调用方落 ACPI S5）。
pub fn reset_shutdown() -> bool {
    let Some(rs) = runtime_services() else {
        return false;
    };
    let Some(reset) = fn_ptr::<EfiResetSystem>(rs, RS_RESET_SYSTEM) else {
        return false;
    };
    unsafe { reset(EFI_RESET_SHUTDOWN, EFI_SUCCESS, 0, core::ptr::null()) };
    // ResetSystem 理论上不返回；若返回，调用方继续（如实）。
    true
}

/// ACPI S5 关机（SYS_POWEROFF ②）：FACP → PM1a_CNT 写
/// `(SLP_TYPa<<10)|SLP_EN`。
///
/// 链路：`acpi::facp_addr()`（init 表遍历登记）→ HHDM 映射 → 表长校验 →
/// DSDT 地址（FACP +40，ACPI 1.0 布局）→ DSDT 表长校验 →
/// `power::find_s5_slp_typ`（`\_S5` 包解码）→ `power::slp_typ_value` 编码
/// → `ps2::port::outw`（PM1a_CNT 是 16 位寄存器）。任何一环缺失 → false
/// 如实返回（调用方报错回桌面），绝不乱写端口。写入即断电，正常不返回。
pub fn poweroff_s5() -> bool {
    let Some(facp_phys) = crate::acpi::facp_addr() else {
        crate::kwarn!("poweroff: no FACP — ACPI S5 unavailable");
        return false;
    };
    let Some(off) = limine::hhdm_offset() else {
        crate::kwarn!("poweroff: no HHDM — ACPI S5 unavailable");
        return false;
    };
    let off = off as u64;
    let facp = facp_phys + off;
    // FACP 表长 @ +4；最小 276（ACPI 1.0 FADT 全长），此处宽松下限 92
    //（与 power::parse_fadt 一致）。
    let flen = unsafe { core::ptr::read_volatile((facp + 4) as *const u32) } as usize;
    if flen < 92 || flen > (1 << 20) {
        crate::kwarn!("poweroff: FACP length implausible ({})", flen);
        return false;
    }
    let facp_bytes =
        unsafe { core::slice::from_raw_parts(facp as *const u8, flen) };
    let Some(fadt) = crate::power::parse_fadt(facp_bytes) else {
        crate::kwarn!("poweroff: FACP parse failed");
        return false;
    };
    if fadt.pm1a_cnt_blk == 0 {
        crate::kwarn!("poweroff: PM1a_CNT_BLK == 0 — ACPI S5 unavailable");
        return false;
    }
    // DSDT 物理地址 @ +40（ACPI 1.0 FADT；X_DSDT 可选扩展，q35/实机均
    // 填 32 位域）。
    let dsdt_phys =
        unsafe { core::ptr::read_volatile((facp + 40) as *const u32) } as u64;
    if dsdt_phys == 0 {
        crate::kwarn!("poweroff: no DSDT — ACPI S5 unavailable");
        return false;
    }
    let dsdt = dsdt_phys + off;
    let dlen = unsafe { core::ptr::read_volatile((dsdt + 4) as *const u32) } as usize;
    if dlen < 36 || dlen > (1 << 20) {
        crate::kwarn!("poweroff: DSDT length implausible ({})", dlen);
        return false;
    }
    let dsdt_bytes =
        unsafe { core::slice::from_raw_parts(dsdt as *const u8, dlen) };
    let Some((typ_a, typ_b)) = crate::power::find_s5_slp_typ(dsdt_bytes) else {
        crate::kwarn!("poweroff: \\_S5 package not found in DSDT");
        return false;
    };
    let val = crate::power::slp_typ_value(typ_a);
    crate::kinfo!(
        "poweroff: S5 PM1a_CNT {:#x} <- {:#06x} (slp_typa={} b={})",
        fadt.pm1a_cnt_blk,
        val,
        typ_a,
        typ_b
    );
    // SAFETY: PM1a_CNT 是 ACPI 定义的 16 位电源管理寄存器；地址与值均已
    // 经 FACP/DSDT 解码校验。ps2::port 仅在内核目标下编译（宿主测试
    // 走不到这里——acpi::facp_addr() 恒 None 提前返回 false）。
    #[cfg(target_os = "none")]
    unsafe { crate::ps2::port::outw(fadt.pm1a_cnt_blk as u16, val) };
    #[cfg(not(target_os = "none"))]
    let _ = (fadt.pm1a_cnt_blk, val);
    true
}

/// 从内核命令行取 BootNext 目标项号：`boot_next=<dec|0xhex>`。
/// **显式写出才有值**——缺省 None，由调用方决定兜底策略（不再静默猜 1）。
pub fn cmdline_entry(cmdline: &str) -> Option<u16> {
    for token in cmdline.split_whitespace() {
        if let Some(v) = token.strip_prefix("boot_next=") {
            let parsed = if let Some(h) = v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")) {
                u16::from_str_radix(h, 16).ok()
            } else {
                v.parse::<u16>().ok()
            };
            if let Some(e) = parsed {
                return Some(e);
            }
        }
    }
    None
}

/// 命令行优先，缺省 0x0001（保留旧调用点语义；新代码请用
/// `resolve_windows_entry`，它能区分「已确认」与「盲猜」）。
pub fn entry_from_cmdline(cmdline: &str) -> u16 {
    cmdline_entry(cmdline).unwrap_or(1)
}

// ---------------------------------------------------------------------------
// EFI 变量读取 + Boot#### 智能枚举
// ---------------------------------------------------------------------------
//
// 为什么必须枚举而不写死 Boot0001：Boot#### 的编号由固件分配，随「装过几个
// 系统、有没有插过别的盘、BIOS 有没有重建过引导项」漂移。写死一个号在别人
// 机器上是蒙的，在本机 BIOS 更新后也可能是蒙的。正确做法是读固件的
// `BootOrder` 拿到优先序，再逐个读 `Boot####` 解析 EFI_LOAD_OPTION，按
// 「描述含 Windows」或「设备路径指向 \EFI\Microsoft\Boot\bootmgfw.efi」判定。

/// BootOrder 变量名（CHAR16 + NUL）。
pub const BOOTORDER_NAME: [u16; 10] = [
    b'B' as u16, b'o' as u16, b'o' as u16, b't' as u16, b'O' as u16, b'r' as u16, b'd' as u16,
    b'e' as u16, b'r' as u16, 0,
];

/// OsIndications 变量名（CHAR16 + NUL）——UEFI 2.4+ 进固件设置的标准通道。
pub const OSINDICATIONS_NAME: [u16; 14] = [
    b'O' as u16, b's' as u16, b'I' as u16, b'n' as u16, b'd' as u16, b'i' as u16, b'c' as u16,
    b'a' as u16, b't' as u16, b'i' as u16, b'o' as u16, b'n' as u16, b's' as u16, 0,
];

/// `EFI_OS_INDICATIONS_BOOT_TO_FW_UI`：置位后冷重启，固件进设置界面。
const OS_IND_BOOT_TO_FW_UI: u64 = 0x0000_0000_0000_0001;

/// 引导项扫描缓冲（.bss；4KiB —— 单个 EFI_LOAD_OPTION 的实际上限量级）。
///
/// 放 .bss 而非栈：内核栈容量不保证，EFI 变量读取在引导早期执行。
static mut BOOT_OPT_BUF: [u8; 4096] = [0u8; 4096];

/// 取扫描缓冲（走裸指针，避开 `static_mut_refs` lint）。
fn opt_buf() -> &'static mut [u8; 4096] {
    unsafe { &mut *core::ptr::addr_of_mut!(BOOT_OPT_BUF) }
}

/// 通用 GetVariable：把全局变量内容读进 `buf`，返回实际字节数。
/// 无 Runtime Services / 变量不存在 / 读取失败 → None（如实，不伪造）。
pub fn get_variable(name: &[u16], buf: &mut [u8]) -> Option<usize> {
    if name.last() != Some(&0) {
        return None; // 变量名必须 NUL 结尾（UEFI 规范），防御性拒绝。
    }
    let rs = runtime_services()?;
    let get_var = fn_ptr::<EfiGetVariable>(rs, RS_GET_VARIABLE)?;
    let mut size = buf.len();
    let mut attrs: u32 = 0;
    let status = unsafe {
        get_var(
            name.as_ptr(),
            GLOBAL_VARIABLE_GUID.as_ptr(),
            &mut attrs,
            &mut size,
            buf.as_mut_ptr().cast::<u16>(),
        )
    };
    if status != EFI_SUCCESS {
        return None;
    }
    Some(size.min(buf.len()))
}

/// 通用 SetVariable：把 `data` 写进全局变量。成功返回 true。
pub fn set_variable(name: &[u16], data: &[u8]) -> bool {
    if name.last() != Some(&0) || data.is_empty() {
        return false;
    }
    let Some(rs) = runtime_services() else {
        return false;
    };
    let Some(set_var) = fn_ptr::<EfiSetVariable>(rs, RS_SET_VARIABLE) else {
        return false;
    };
    let status = unsafe {
        set_var(
            name.as_ptr(),
            GLOBAL_VARIABLE_GUID.as_ptr(),
            ATTR_NV_BS_RT,
            data.len(),
            data.as_ptr().cast::<u16>(),
        )
    };
    status == EFI_SUCCESS
}

/// `Boot####` 变量名（UEFI 规范：4 位**大写**十六进制）。
pub fn boot_var_name(num: u16) -> [u16; 9] {
    const HEX: [u16; 16] = [
        b'0' as u16, b'1' as u16, b'2' as u16, b'3' as u16, b'4' as u16, b'5' as u16, b'6' as u16,
        b'7' as u16, b'8' as u16, b'9' as u16, b'A' as u16, b'B' as u16, b'C' as u16, b'D' as u16,
        b'E' as u16, b'F' as u16,
    ];
    let mut n = [0u16; 9];
    n[0] = b'B' as u16;
    n[1] = b'o' as u16;
    n[2] = b'o' as u16;
    n[3] = b't' as u16;
    for i in 0..4 {
        let shift = 12 - i * 4;
        n[4 + i] = HEX[((num >> shift) & 0xF) as usize];
    }
    n[8] = 0;
    n
}

/// `Boot####` 变量名反解：非 `Boot` 前缀或含非法十六进制字符 → None。
/// 用于扫描阶段区分「这是一条引导项」而不是别的 Boot* 变量。
pub fn boot_var_number(name: &[u16]) -> Option<u16> {
    if name.len() < 9 || name[0..4] != [b'B' as u16, b'o' as u16, b'o' as u16, b't' as u16] {
        return None;
    }
    if name[8] != 0 {
        return None;
    }
    let mut v: u16 = 0;
    for &c in &name[4..8] {
        let d = match c {
            c if (b'0' as u16..=b'9' as u16).contains(&c) => c - b'0' as u16,
            c if (b'A' as u16..=b'F' as u16).contains(&c) => c - b'A' as u16 + 10,
            _ => return None,
        };
        v = v * 16 + d;
    }
    Some(v)
}

/// 读 `BootOrder`（固件引导优先序，u16 数组）。返回项数；不可读返回 0。
pub fn boot_order(out: &mut [u16]) -> usize {
    let buf = opt_buf();
    let Some(n) = get_variable(&BOOTORDER_NAME, &mut buf[..]) else {
        return 0;
    };
    let count = (n / 2).min(out.len());
    for (i, slot) in out.iter_mut().take(count).enumerate() {
        *slot = u16::from_le_bytes([buf[i * 2], buf[i * 2 + 1]]);
    }
    count
}

/// 在 UTF-16LE 字节流中查找 ASCII 子串（忽略大小写）。
///
/// 只在偶数偏移比对：UTF-16LE 的 ASCII 字符是 `(c, 0)` 两字节，从奇数偏移
/// 起比会跨字符错位，永不命中。
fn utf16_contains_ascii_ignore_case(bytes: &[u8], needle: &str) -> bool {
    let nb = needle.len();
    if nb == 0 || nb > 32 || bytes.len() < nb * 2 {
        return false;
    }
    let mut pat = [0u8; 64];
    for (i, &c) in needle.as_bytes().iter().enumerate() {
        pat[i * 2] = c.to_ascii_lowercase();
    }
    let plen = nb * 2;
    let mut i = 0usize;
    while i + plen <= bytes.len() {
        let mut ok = true;
        for k in 0..nb {
            // 高位必须为 0（否则不是 ASCII 字符，是别的 Unicode）。
            if bytes[i + k * 2 + 1] != 0
                || bytes[i + k * 2].to_ascii_lowercase() != pat[k * 2]
            {
                ok = false;
                break;
            }
        }
        if ok {
            return true;
        }
        i += 2;
    }
    false
}

/// 判定一条 `EFI_LOAD_OPTION` 是否指向 Windows。
///
/// 布局：`u32 Attributes | u16 FilePathListLength | CHAR16 Description[NUL]
/// | FilePathList[FilePathListLength] | OptionalData`。
/// 两级判据——描述命中优先（"Windows Boot Manager"），描述被改写时退到
/// 设备路径特征（Microsoft + bootmgfw）。
pub fn option_looks_like_windows(bytes: &[u8]) -> bool {
    if utf16_contains_ascii_ignore_case(bytes, "windows") {
        return true;
    }
    utf16_contains_ascii_ignore_case(bytes, "microsoft")
        && utf16_contains_ascii_ignore_case(bytes, "bootmgfw")
}

/// 解析 GUID 文本（`{xxxxxxxx-…}` 或裸 8-4-4-4-12，大小写不敏感）为
/// **EFI 字节序** 16 字节——首三字段小端摆放，与固件设备路径里的
/// EFI_GUID 布局一致（S1.3 登记的 `usb_windows_esp_guid` 语义）。
pub fn parse_guid_text(s: &str) -> Option<[u8; 16]> {
    let mut hex = [0u8; 32];
    let mut n = 0usize;
    for c in s.chars() {
        match c {
            '{' | '}' | '-' => {}
            _ => {
                if n >= 32 {
                    return None;
                }
                hex[n] = c.to_digit(16)? as u8;
                n += 1;
            }
        }
    }
    if n != 32 {
        return None;
    }
    let mut raw = [0u8; 16];
    for i in 0..16 {
        raw[i] = (hex[i * 2] << 4) | hex[i * 2 + 1];
    }
    let mut out = [0u8; 16];
    out[0] = raw[3];
    out[1] = raw[2];
    out[2] = raw[1];
    out[3] = raw[0];
    out[4] = raw[5];
    out[5] = raw[4];
    out[6] = raw[7];
    out[7] = raw[6];
    out[8..16].copy_from_slice(&raw[8..16]);
    Some(out)
}

/// 16 字节 GUID 子序列匹配（EFI_LOAD_OPTION 内容里设备路径的
/// 分区 GUID 节点原样出现）。
pub fn bytes_contains_guid(bytes: &[u8], guid: &[u8; 16]) -> bool {
    if bytes.len() < 16 {
        return false;
    }
    for i in 0..=(bytes.len() - 16) {
        if bytes[i..i + 16] == *guid {
            return true;
        }
    }
    false
}

/// 目标匹配：Windows 项 + （给定 GUID 时）内容含该 U 盘 ESP GUID。
/// GUID 是分区级精确判据——指向同一 ESP 的项不可能认错盘。
pub fn option_matches_target(bytes: &[u8], usb_guid: Option<&[u8; 16]>) -> bool {
    if !option_looks_like_windows(bytes) {
        return false;
    }
    match usb_guid {
        Some(g) => bytes_contains_guid(bytes, g),
        None => true,
    }
}

/// Windows 引导项解析结果。**是否「已确认」是硬信息**，不许混为一谈。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsEntry {
    /// 从固件的 BootOrder/Boot#### 里枚举出来的，或命令行显式钉死的。
    Resolved(u16),
    /// 枚举不到（无 Runtime Services / 变量读不动 / 没有匹配项）：
    /// 退回命令行或内置默认值，**未经验证**——调用方必须如实标注。
    Unverified(u16),
}

impl WindowsEntry {
    pub fn number(self) -> u16 {
        match self {
            WindowsEntry::Resolved(n) | WindowsEntry::Unverified(n) => n,
        }
    }
    pub fn verified(self) -> bool {
        matches!(self, WindowsEntry::Resolved(_))
    }
}

/// 解析「切 Windows 要写哪个 BootNext」。优先级：
/// 1. 命令行 `boot_next=`（部署线可按实机钉死）→ Resolved；
/// 2. 按 `BootOrder` 优先序逐个读 `Boot####` 匹配 → Resolved；
/// 3. `BootOrder` 读不到时全扫 `Boot0000..Boot00FF` 兜底 → Resolved；
/// 4. 全都失败 → Unverified(命令行值或 1)，调用方须如实告知用户。
pub fn resolve_windows_entry(cmdline: &str) -> WindowsEntry {
    resolve_windows_entry_for(cmdline, None)
}

/// 目标化解析：`usb_guid=Some` 时只认「Windows 项且设备路径含该 U 盘
/// ESP GUID」——`handoff_target=usb` 语义（S1.5 接线）。优先级不变：
/// 1. cmdline `boot_next=`（视为已验证）；2. BootOrder 顺序匹配；
/// 3. BootOrder 读不到时全扫兜底；4. 全失败 → Unverified。
///
/// usb 目标的匹配分两个梯队（2026-09-23 实机实证）：
/// 第一梯队 = GUID 精确匹配（设备路径含 U 盘 ESP GUID 字节）；
/// 第二梯队 = 描述含 `VARIX` 字样且形似 Windows 项——**Lenovo 真机固件对
/// 可移动 U 盘的引导项天生不带分区 HD 节点**（可移动媒体无需分区定位），
/// GUID 字节结构性缺席；而部署线供应的固件项描述固定为
/// `VARIX Windows (USB)`，内置项（Windows Boot Manager）与厂商原生项
/// （EFI USB Device / PXE / DVD）都不含 `varix` 字样，零误伤。
pub fn resolve_windows_entry_for(cmdline: &str, usb_guid: Option<&[u8; 16]>) -> WindowsEntry {
    if let Some(v) = cmdline_entry(cmdline) {
        return WindowsEntry::Resolved(v);
    }
    let mut order = [0u16; 32];
    let n = boot_order(&mut order);
    if n > 0 {
        for &num in &order[..n] {
            let buf = opt_buf();
            let name = boot_var_name(num);
            let Some(sz) = get_variable(&name, &mut buf[..]) else {
                continue;
            };
            if option_matches_target(&buf[..sz], usb_guid) {
                return WindowsEntry::Resolved(num);
            }
            if option_matches_varix(&buf[..sz], usb_guid) {
                return WindowsEntry::Resolved(num);
            }
        }
    } else {
        // BootOrder 不可读（部分固件隐藏该变量）：全量扫描兜底。
        let mut all = [0u16; 0x100];
        for (i, v) in all.iter_mut().enumerate() {
            *v = i as u16;
        }
        for &num in all.iter() {
            let buf = opt_buf();
            let name = boot_var_name(num);
            let Some(sz) = get_variable(&name, &mut buf[..]) else {
                continue;
            };
            if option_matches_target(&buf[..sz], usb_guid) {
                return WindowsEntry::Resolved(num);
            }
            if option_matches_varix(&buf[..sz], usb_guid) {
                return WindowsEntry::Resolved(num);
            }
        }
    }
    WindowsEntry::Unverified(entry_from_cmdline(cmdline))
}

/// 第二梯队匹配（S1.5 补充）：形似 Windows 项 + 描述含 `VARIX`。
/// 仅在 usb 目标下生效——internal 目标保持原语义（任意 Windows 项），
/// 不给内置盘引导项引入新的匹配面。
///
/// **设备路径有效性闸门（2026-09-23 实机循环根因加固）**：Lenovo 真机
/// Boot2001 实锤 `FilePathListLength=4`（路径 = 纯 END 节点 `7fff0400`）——
/// 描述命中但固件无处加载，兑现失败后回落本次引导设备（F12 手选的 U 盘）
/// → 再进菜单 → 再交接 → **无限复位循环**。凡 Load Option 路径长度 ≤4
/// （END-only）的项一律不命中：宁可不交接（落 ushell 防自锁语义），
/// 也不写一个必然兑现失败的 BootNext。
pub fn option_matches_varix(bytes: &[u8], usb_guid: Option<&[u8; 16]>) -> bool {
    if usb_guid.is_none() {
        return false;
    }
    match load_option_filepath_len(bytes) {
        Some(n) if n > 4 => {}
        _ => return false,
    }
    option_looks_like_windows(bytes) && utf16_contains_ascii_ignore_case(bytes, "varix")
}

/// 解析 EFI Load Option 的 `FilePathListLength`（偏移 4..6，u16 LE）。
/// 少于 6 字节的缓冲无此字段，返回 None。
pub fn load_option_filepath_len(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < 6 {
        return None;
    }
    Some(u16::from_le_bytes([bytes[4], bytes[5]]) as usize)
}

/// 进固件设置（UEFI 2.4+ `OsIndications` 标准通道）。
///
/// 流程：读 `OsIndications` → 或上 `BOOT_TO_FW_UI` → 写回 → 冷重启。
/// 读-改-写而非覆盖：保留固件已有的其他 indication 位（有些厂商用私有位）。
/// 任一环失败如实 false（调用方退回普通冷重启并告知）。
pub fn boot_to_firmware_ui() -> bool {
    let buf = opt_buf();
    // ① 读现值（读不到视为 0——变量不存在时固件按全零处理）。
    let mut cur: u64 = 0;
    if let Some(n) = get_variable(&OSINDICATIONS_NAME, &mut buf[..]) {
        if n >= 8 {
            let mut b = [0u8; 8];
            b.copy_from_slice(&buf[..8]);
            cur = u64::from_le_bytes(b);
        }
    } else if runtime_services().is_none() {
        return false; // 非 UEFI：压根没法进固件设置，别假装。
    }
    // ② 或上目标位并写回。
    let next = cur | OS_IND_BOOT_TO_FW_UI;
    if !set_variable(&OSINDICATIONS_NAME, &next.to_le_bytes()) {
        return false;
    }
    // ③ 冷重启交给固件兑现。
    reset_cold()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_variable_guid_bytes() {
        // EFI_GLOBAL_VARIABLE = {8BE4DF61-93CA-11D2-AA0D-00E098032B8C}
        assert_eq!(
            GLOBAL_VARIABLE_GUID,
            [0x61, 0xDF, 0xE4, 0x8B, 0xCA, 0x93, 0xD2, 0x11, 0xAA, 0x0D, 0x00, 0xE0, 0x98, 0x03,
             0x2B, 0x8C]
        );
    }

    #[test]
    fn bootnext_name_is_utf16_nul_terminated() {
        let name: String = BOOTNEXT_NAME
            .iter()
            .take(8)
            .map(|&c| char::from_u32(c as u32).unwrap())
            .collect();
        assert_eq!(name, "BootNext");
        assert_eq!(BOOTNEXT_NAME[8], 0);
    }

    #[test]
    fn outcome_vocabulary() {
        // 词表足够打印/匹配即可（诚实分支四态）。
        assert_ne!(BootNextOutcome::Written { entry: 2 }, BootNextOutcome::NoRuntimeServices);
        assert_eq!(
            BootNextOutcome::SetFailed(0x8000000000000003), // EFI_INVALID_PARAMETER
            BootNextOutcome::SetFailed(0x8000000000000003)
        );
        assert_eq!(
            BootNextOutcome::ReadbackMismatch { wrote: 2, read: 0 },
            BootNextOutcome::ReadbackMismatch { wrote: 2, read: 0 }
        );
    }

    #[test]
    fn runtime_services_absent_on_host() {
        // 宿主无 EFI 系统表请求响应 → 恒 None/NoRuntimeServices。
        assert_eq!(write_bootnext(2), BootNextOutcome::NoRuntimeServices);
        assert!(!reset_cold());
        assert!(!reset_shutdown());
        // 宿主无 FACP 登记 → S5 路径如实 false。
        assert!(!poweroff_s5());
    }

    // ---- Boot#### 智能枚举（不再盲猜项号） ----

    /// 把 ASCII 串编成 UTF-16LE 字节（含结尾 NUL），用于构造假 LOAD_OPTION。
    fn utf16(s: &str) -> std::vec::Vec<u8> {
        let mut v = std::vec::Vec::new();
        for &c in s.as_bytes() {
            v.push(c);
            v.push(0);
        }
        v.push(0);
        v.push(0);
        v
    }

    /// 构造一条 EFI_LOAD_OPTION：Attributes + FilePathListLength + 描述 + 路径。
    /// 构造**真实形态**的 EFI Load Option：
    /// attr=1 | FilePathListLength | desc(UTF-16 NUL) | path(UTF-16 NUL) + END 节点。
    /// 2026-09-23 起第二梯队校验 FilePathListLength>4——测试数据必须带真头部，
    /// 全零假头部（fplen=0）会让所有合法项被路径闸门拒收。
    fn load_option(desc: &str, path: &str) -> std::vec::Vec<u8> {
        let mut p = utf16(path);
        p.extend_from_slice(&[0x7F, 0xFF, 0x04, 0x00]); // END node (Type 7F, Sub FF, Len 4)
        let mut v = 1u32.to_le_bytes().to_vec();
        v.extend_from_slice(&(p.len() as u16).to_le_bytes());
        v.extend_from_slice(&utf16(desc));
        v.extend_from_slice(&p);
        v
    }

    #[test]
    fn boot_var_name_is_upper_hex() {
        assert_eq!(boot_var_name(0), ['B' as u16, 'o' as u16, 'o' as u16, 't' as u16,
            '0' as u16, '0' as u16, '0' as u16, '0' as u16, 0]);
        assert_eq!(boot_var_name(1), ['B' as u16, 'o' as u16, 'o' as u16, 't' as u16,
            '0' as u16, '0' as u16, '0' as u16, '1' as u16, 0]);
        // 大写十六进制（UEFI 规范），小写会被部分固件当成另一个变量。
        assert_eq!(boot_var_name(0x00AB), ['B' as u16, 'o' as u16, 'o' as u16, 't' as u16,
            '0' as u16, '0' as u16, 'A' as u16, 'B' as u16, 0]);
        assert_eq!(boot_var_name(0xFFFF), ['B' as u16, 'o' as u16, 'o' as u16, 't' as u16,
            'F' as u16, 'F' as u16, 'F' as u16, 'F' as u16, 0]);
    }

    #[test]
    fn boot_var_number_roundtrip() {
        for n in [0u16, 1, 0x0A, 0xAB, 0x1234, 0xFFFF] {
            assert_eq!(boot_var_number(&boot_var_name(n)), Some(n));
        }
        // 非引导项变量（BootOrder / BootNext / BootCurrent）必须拒收，
        // 否则全量扫描会把它们误当成 Boot####。
        assert_eq!(boot_var_number(&BOOTORDER_NAME), None);
        assert_eq!(boot_var_number(&BOOTNEXT_NAME), None);
    }

    #[test]
    fn utf16_search_matches_ascii_ignore_case() {
        let hay = utf16("Windows Boot Manager");
        assert!(utf16_contains_ascii_ignore_case(&hay, "windows"));
        assert!(utf16_contains_ascii_ignore_case(&hay, "WINDOWS"));
        assert!(utf16_contains_ascii_ignore_case(&hay, "Boot Manager"));
        assert!(!utf16_contains_ascii_ignore_case(&hay, "linux"));
        // 空串恒不匹配（避免 `contains("")` 恒真的语义陷阱）。
        assert!(!utf16_contains_ascii_ignore_case(&hay, ""));
    }

    #[test]
    fn utf16_search_ignores_odd_offsets() {
        // 奇数偏移起比会跨字符错位：这里只能从 0 命中，不能从 1 命中。
        // 构造：一个非 ASCII 字符（高位非 0）在前，ASCII 在后。
        let mut v = std::vec![0x41u8, 0x04, 0x00, 0x00]; // U+0441 西里尔字母
        v.extend_from_slice(&utf16("abc"));
        assert!(utf16_contains_ascii_ignore_case(&v, "abc"));
        assert!(!utf16_contains_ascii_ignore_case(&v, "\u{0441}a"));
    }

    #[test]
    fn guid_text_parses_to_efi_byte_order() {
        // .NET Guid.ToByteArray 同布局：首三字段小端
        let g = parse_guid_text("{636786cb-e967-49f6-b0df-7608909d1f11}").unwrap();
        assert_eq!(g[0], 0xcb);
        assert_eq!(g[1], 0x86);
        assert_eq!(g[2], 0x67);
        assert_eq!(g[3], 0x63);
        assert_eq!(g[4], 0x67);
        assert_eq!(g[5], 0xe9);
        assert_eq!(&g[8..], &[0xb0, 0xdf, 0x76, 0x08, 0x90, 0x9d, 0x1f, 0x11]);
        // 无花括号 / 大写 等价
        assert_eq!(parse_guid_text("636786CB-E967-49F6-B0DF-7608909D1F11").unwrap(), g);
        // 非法：长度不足 / 非 hex
        assert!(parse_guid_text("636786cb").is_none());
        assert!(parse_guid_text("zzzzzzcb-e967-49f6-b0df-7608909d1f11").is_none());
        assert!(parse_guid_text("").is_none());
    }

    #[test]
    fn guid_bytes_subsequence_match() {
        let g = parse_guid_text("636786cb-e967-49f6-b0df-7608909d1f11").unwrap();
        let mut content = load_option("Windows Boot Manager", r"\EFI\Microsoft\Bootootmgfw.efi");
        assert!(!bytes_contains_guid(&content, &g), "GUID 未植入时不得命中");
        content.extend_from_slice(&g);
        assert!(bytes_contains_guid(&content, &g), "植入后必须命中");
        let mut other = g;
        other[0] ^= 0xFF;
        assert!(!bytes_contains_guid(&content, &other));
        assert!(!bytes_contains_guid(&g[..8], &g), "短缓冲不得命中");
    }

    #[test]
    fn target_match_requires_windows_and_guid() {
        let g = parse_guid_text("636786cb-e967-49f6-b0df-7608909d1f11").unwrap();
        let mut win_usb = load_option("Windows Boot Manager", r"\EFI\Microsoft\Bootootmgfw.efi");
        win_usb.extend_from_slice(&g);
        // Windows 项 + GUID 在 → 命中
        assert!(option_matches_target(&win_usb, Some(&g)));
        // GUID=None（内置盘目标）→ 任意 Windows 项命中
        assert!(option_matches_target(&win_usb, None));
        // Windows 项但 GUID 不在 → 不命中（防止误指内置盘）
        let win_plain = load_option("Windows Boot Manager", r"\EFI\Microsoft\Bootootmgfw.efi");
        assert!(!option_matches_target(&win_plain, Some(&g)));
        // 非 Windows 项（如 Limine/UEFI 壳）即使 GUID 在也不命中
        let mut limine = load_option("UEFI: VARIX", r"\EFI\BOOT\BOOTX64.EFI");
        limine.extend_from_slice(&g);
        assert!(!option_matches_target(&limine, Some(&g)));
    }

    #[test]
    fn varix_second_tier_matches_guidless_usb_entry() {
        let g = parse_guid_text("636786cb-e967-49f6-b0df-7608909d1f11").unwrap();
        // 真机 Lenovo 实证（vx-enum-bootvars，Boot2001）：描述 `VARIX Windows (USB)`，
        // 整项仅 52 字节、无分区 HD 节点 → GUID 字节结构性缺席。
        // 第一梯队（GUID）必然失手，第二梯队必须接住。
        let fw_entry = load_option("VARIX Windows (USB)", r"\EFI\Microsoft\Boot\bootmgfw.efi");
        assert!(!bytes_contains_guid(&fw_entry, &g), "本项模拟无 GUID 尾巴的真机形态");
        assert!(option_matches_varix(&fw_entry, Some(&g)));
        // 描述大小写不敏感（固件可能给小写 varix）。
        assert!(option_matches_varix(
            &load_option("varix windows (usb)", r"\EFI\Microsoft\Boot\bootmgfw.efi"),
            Some(&g)
        ));
    }

    #[test]
    fn varix_second_tier_zero_false_positives() {
        let g = parse_guid_text("636786cb-e967-49f6-b0df-7608909d1f11").unwrap();
        // ① 内置 Windows Boot Manager：无 varix 字样 → 不命中（内置目标走原语义）。
        assert!(!option_matches_varix(
            &load_option("Windows Boot Manager", r"\EFI\Microsoft\Boot\bootmgfw.efi"),
            Some(&g)
        ));
        // ② Limine / UEFI 壳项即使描述带 VARIX：非 Windows 形态 → 不命中
        //    （防误指我们自己的 Limine 项，BootNext 只该指 Windows）。
        assert!(!option_matches_varix(
            &load_option("VARIX", r"\EFI\limine\limine_x64.efi"),
            Some(&g)
        ));
        // ③ internal 目标（usb_guid=None）→ 恒不命中，不给内置盘引入新匹配面。
        assert!(!option_matches_varix(
            &load_option("VARIX Windows (USB)", r"\EFI\Microsoft\Boot\bootmgfw.efi"),
            None
        ));
        // ④ 形似 Windows 但既无 GUID 又无 varix 字样的他项 → 不命中
        //    （此时宁可 Unverified 回退，也不可错指）。
        assert!(!option_matches_varix(
            &load_option("Windows Boot Manager", r"\EFI\Microsoft\Boot\bootmgfw.efi"),
            Some(&g)
        ));
    }

    #[test]
    fn varix_second_tier_rejects_empty_filepath() {
        // 2026-09-23 实机循环根因复刻：Lenovo Boot2001 描述对、路径 END-only
        // （FilePathListLength=4，hex 7fff0400）——bcdedit set device 清空所致。
        // 这种项描述完美命中、固件却无处加载 → 兑现失败回落 U 盘 → 无限复位循环。
        // 路径闸门必须拒收它，退化为防自锁语义（落 ushell，宁缺毋循环）。
        let g = parse_guid_text("636786cb-e967-49f6-b0df-7608909d1f11").unwrap();
        let mut broken = load_option("VARIX Windows (USB)", r"\EFI\Microsoft\Boot\bootmgfw.efi");
        assert!(option_matches_varix(&broken, Some(&g)), "合法形态必须先通过（对照）");
        broken[4] = 4; // FilePathListLength = 4 = 纯 END 节点
        broken[5] = 0;
        assert!(!option_matches_varix(&broken, Some(&g)), "END-only 路径不得命中");
        // 路径字段解析器本身的边界
        assert_eq!(load_option_filepath_len(&broken), Some(4));
        assert_eq!(load_option_filepath_len(&broken[..5]), None, "不足 6 字节无此字段");
        assert!(option_looks_like_windows(&broken), "路径闸门不影响 looks 判定本身");
    }

    #[test]
    fn windows_option_detected_by_description() {
        let w = load_option("Windows Boot Manager", r"\EFI\Microsoft\Boot\bootmgfw.efi");
        assert!(option_looks_like_windows(&w));
        // 描述被本地化改写时，靠设备路径特征兜底。
        let zh = load_option("某个操作系统", r"\EFI\Microsoft\Boot\bootmgfw.efi");
        assert!(option_looks_like_windows(&zh));
    }

    #[test]
    fn non_windows_option_rejected() {
        assert!(!option_looks_like_windows(&load_option(
            "Linux Boot Manager",
            r"\EFI\systemd\systemd-bootx64.efi"
        )));
        assert!(!option_looks_like_windows(&load_option(
            "Limine",
            r"\EFI\limine\limine_x64.efi"
        )));
        // 只提到 Microsoft 但不是 bootmgfw（如第三方引导器借路径）不算。
        assert!(!option_looks_like_windows(&load_option(
            "Other",
            r"\EFI\Microsoft\Boot\something.efi"
        )));
        assert!(!option_looks_like_windows(&[]));
    }

    #[test]
    fn variable_access_absent_on_host() {
        let mut buf = [0u8; 16];
        // 宿主无 Runtime Services → 全部如实失败，不伪造读到的值。
        assert_eq!(get_variable(&BOOTORDER_NAME, &mut buf), None);
        assert_eq!(boot_order(&mut [0u16; 8]), 0);
        assert!(!set_variable(&OSINDICATIONS_NAME, &1u64.to_le_bytes()));
        // 变量名未 NUL 结尾 → 防御性拒绝（UEFI 会把越界名当垃圾）。
        let bad = ['B' as u16, 'o' as u16, 'o' as u16, 't' as u16];
        assert_eq!(get_variable(&bad, &mut buf), None);
        assert!(!set_variable(&bad, &[1, 0]));
        // 空数据不允许写（SetVariable 语义上也不接受 0 长度）。
        assert!(!set_variable(&OSINDICATIONS_NAME, &[]));
    }

    #[test]
    fn resolve_prefers_cmdline_then_falls_back_unverified() {
        // 命令行显式钉死 → 已确认（部署线按实机项号固定时的正路）。
        assert_eq!(
            resolve_windows_entry("varix.smp boot_next=0x0003"),
            WindowsEntry::Resolved(3)
        );
        assert_eq!(
            resolve_windows_entry("boot_next=7"),
            WindowsEntry::Resolved(7)
        );
        // 宿主枚举不到任何项 → 未确认，且调用方可见（不许当成已确认）。
        let e = resolve_windows_entry("");
        assert_eq!(e, WindowsEntry::Unverified(1));
        assert!(!e.verified());
        assert_eq!(e.number(), 1);
    }

    #[test]
    fn windows_entry_vocabulary_distinguishes_confidence() {
        assert!(WindowsEntry::Resolved(2).verified());
        assert!(!WindowsEntry::Unverified(2).verified());
        assert_ne!(WindowsEntry::Resolved(2), WindowsEntry::Unverified(2));
    }

    #[test]
    fn firmware_ui_unavailable_on_host() {
        // 宿主无 Runtime Services → 进固件设置如实失败（不假装成功重启）。
        assert!(!boot_to_firmware_ui());
    }

    #[test]
    fn cmdline_entry_distinguishes_explicit_from_default() {
        assert_eq!(cmdline_entry("boot_next=0x0002"), Some(2));
        assert_eq!(cmdline_entry("boot_next=5"), Some(5));
        assert_eq!(cmdline_entry(""), None);
        assert_eq!(cmdline_entry("varix.smp"), None);
        // 非法值不静默当 0，而是视为「没指定」交给枚举。
        assert_eq!(cmdline_entry("boot_next=99999"), None);
        assert_eq!(cmdline_entry("boot_next=zzz"), None);
        // 旧 API 语义保持（缺省 1），仅供存量调用点。
        assert_eq!(entry_from_cmdline(""), 1);
    }
}
