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

/// 从内核命令行取 BootNext 目标项号：`boot_next=<dec|0xhex>`，缺省 0x0001。
/// 目标项号取决于 ESP 上 Windows 引导项的 Boot#### 编号（AI-P 部署线装配）。
pub fn entry_from_cmdline(cmdline: &str) -> u16 {
    for token in cmdline.split_whitespace() {
        if let Some(v) = token.strip_prefix("boot_next=") {
            let parsed = if let Some(h) = v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")) {
                u16::from_str_radix(h, 16).ok()
            } else {
                v.parse::<u16>().ok()
            };
            if let Some(e) = parsed {
                return e;
            }
        }
    }
    1
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
}
