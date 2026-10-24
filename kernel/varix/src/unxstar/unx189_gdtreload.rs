//! unx189_gdtreload — UNX-F0189 GDT 重载与段语义固化（0x6C30 段）。
//!
//! 锚点（CoRun Unxreal 施工书 UNX-F0189）：GDT 重载与段语义固化。
//! 判据原文（UNX-F0189-J1）：重载后段描述符布局与设计一致，异常段访问
//! 触发 #GP 断言。
//!
//! 设计三线：①描述符布局声明表（NULL/内核代码/内核数据/用户代码/用户
//! 数据/TSS 六槽位设计真源，每槽位有 access/limit/granularity 预期）；
//! ②重载后逐槽对拍（实际描述符解码值 vs 设计声明逐位一致——布局漂移
//! 即红）；③异常段访问闸（越权访问请求按段类型语义判定：向低特权代码
//! 段 Ring0 直跳、写只读数据段 → 必须 #GP——语义固化兑现）。

use alloc::vec;
use alloc::vec::Vec;

/// 模块版本。
pub const GDT_VERSION: &str = "189-gdtreload-v1";

/// 段基（0x6C30）。
pub const SEG_BASE: u16 = 0x6C30;

/// 槽位序（设计真源布局）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Null = 0,
    KCode = 1,
    KData = 2,
    UCode = 3,
    UData = 4,
    Tss = 5,
}

/// 槽位选择子（字节偏移 ×8）。
pub fn selector(s: Slot) -> u16 {
    (s as u16) * 8
}

/// 特权级。
pub type Ring = u8;

/// 段类型语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegKind {
    Code { ring: Ring, conforming: bool },
    Data { ring: Ring, writable: bool },
    Tss,
    Null,
}

/// 段描述符（解码后的判据视角字段）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegDesc {
    pub kind: SegKind,
    /// present 位。
    pub present: bool,
    /// 4KiB 粒度。
    pub gran4k: bool,
    /// 64 位代码段（L 位）。
    pub long: bool,
}

impl SegDesc {
    /// 原始 8 字节 → 解码（判据对拍输入形态）。
    pub fn decode(raw: u64) -> SegDesc {
        let access = ((raw >> 40) & 0xFF) as u8;
        let flags = ((raw >> 52) & 0xF) as u8;
        let present = access & 0x80 != 0;
        let ring = (access >> 5) & 0x3;
        let exec = access & 0x8 != 0;
        let dc = access & 0x4 != 0;
        let rw = access & 0x2 != 0;
        let kind = if access & 0x1F == 0 {
            SegKind::Null
        } else if (access & 0x1F) == 0x9 || (access & 0x1F) == 0xB {
            SegKind::Tss
        } else if exec {
            SegKind::Code {
                ring,
                conforming: dc,
            }
        } else {
            SegKind::Data { ring, writable: rw }
        };
        SegDesc {
            kind,
            present,
            gran4k: flags & 0x8 != 0,
            long: flags & 0x2 != 0,
        }
    }
}

// ---------------------------------------------------------------------------
// 设计布局表
// ---------------------------------------------------------------------------

/// 设计真源：六槽位预期描述符。
pub fn design_layout() -> [(Slot, SegDesc); 6] {
    [
        (
            Slot::Null,
            SegDesc {
                kind: SegKind::Null,
                present: false,
                gran4k: false,
                long: false,
            },
        ),
        (
            Slot::KCode,
            SegDesc {
                kind: SegKind::Code {
                    ring: 0,
                    conforming: false,
                },
                present: true,
                gran4k: true,
                long: true,
            },
        ),
        (
            Slot::KData,
            SegDesc {
                kind: SegKind::Data {
                    ring: 0,
                    writable: true,
                },
                present: true,
                gran4k: true,
                long: false,
            },
        ),
        (
            Slot::UCode,
            SegDesc {
                kind: SegKind::Code {
                    ring: 3,
                    conforming: false,
                },
                present: true,
                gran4k: true,
                long: true,
            },
        ),
        (
            Slot::UData,
            SegDesc {
                kind: SegKind::Data {
                    ring: 3,
                    writable: true,
                },
                present: true,
                gran4k: true,
                long: false,
            },
        ),
        (
            Slot::Tss,
            SegDesc {
                kind: SegKind::Tss,
                present: true,
                gran4k: false,
                long: false,
            },
        ),
    ]
}

/// 重载后逐槽对拍（判据主体）：解码实值 vs 设计声明逐位一致。
pub fn reload_compare(actual: &[u64]) -> Result<(), u16> {
    let design = design_layout();
    if actual.len() != design.len() {
        return Err(E_LAYOUT_LEN);
    }
    for (i, (_, expect)) in design.iter().enumerate() {
        let got = SegDesc::decode(actual[i]);
        if got != *expect {
            return Err(E_LAYOUT_DRIFT);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 异常段访问闸
// ---------------------------------------------------------------------------

/// 段访问请求。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegAccess {
    /// 访问目标槽位。
    pub target: Slot,
    /// 访问方 Ring。
    pub from_ring: Ring,
    /// 是否写访问。
    pub write: bool,
    /// 是否执行访问。
    pub exec: bool,
}

/// 访问裁决：违例返回 #GP（语义固化兑现处）。
pub fn access_verdict(desc: &SegDesc, a: &SegAccess) -> Result<(), u16> {
    if !desc.present {
        return Err(E_GP_NOT_PRESENT);
    }
    match desc.kind {
        SegKind::Null => Err(E_GP_NULL),
        SegKind::Tss => Err(E_GP_TSS_EXEC),
        SegKind::Code { ring, conforming } => {
            if !a.exec {
                return Err(E_GP_CODE_READ);
            }
            // 非一致代码段：CPL 数值上高于（特权低于）DPL 即越权。
            if !conforming && a.from_ring > ring {
                return Err(E_GP_PRIV);
            }
            Ok(())
        }
        SegKind::Data { ring, writable } => {
            if a.exec {
                return Err(E_GP_DATA_EXEC);
            }
            // 数据段：CPL 高于 DPL 即越权（Ring0 可读 Ring3 数据）。
            if a.from_ring > ring {
                return Err(E_GP_PRIV);
            }
            if a.write && !writable {
                return Err(E_GP_READONLY);
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// 错误码（0x6C30 段，显式判别式）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum GdtErr {
    /// 布局长度漂移。
    LayoutLen = SEG_BASE + 1,
    /// 槽位描述符漂移。
    LayoutDrift = SEG_BASE + 2,
    /// #GP：访问不 present 段。
    GpNotPresent = SEG_BASE + 3,
    /// #GP：访问 NULL 槽。
    GpNull = SEG_BASE + 4,
    /// #GP：TSS 当执行段。
    GpTssExec = SEG_BASE + 5,
    /// #GP：代码段读访问。
    GpCodeRead = SEG_BASE + 6,
    /// #GP：特权越权。
    GpPriv = SEG_BASE + 7,
    /// #GP：数据段执行。
    GpDataExec = SEG_BASE + 8,
    /// #GP：写只读段。
    GpReadonly = SEG_BASE + 9,
}

impl GdtErr {
    pub const fn code(self) -> u16 {
        self as u16
    }
    pub fn detail(self) -> &'static str {
        match self {
            GdtErr::LayoutLen => "重载后槽位数与设计不符",
            GdtErr::LayoutDrift => "槽位描述符与设计漂移",
            GdtErr::GpNotPresent => "#GP：段不 present",
            GdtErr::GpNull => "#GP：NULL 槽访问",
            GdtErr::GpTssExec => "#GP：TSS 不可当执行段",
            GdtErr::GpCodeRead => "#GP：代码段不可读访问",
            GdtErr::GpPriv => "#GP：特权级越权",
            GdtErr::GpDataExec => "#GP：数据段不可执行",
            GdtErr::GpReadonly => "#GP：只读数据段写访问",
        }
    }
}

const E_LAYOUT_LEN: u16 = GdtErr::LayoutLen.code();
const E_LAYOUT_DRIFT: u16 = GdtErr::LayoutDrift.code();
const E_GP_NOT_PRESENT: u16 = GdtErr::GpNotPresent.code();
const E_GP_NULL: u16 = GdtErr::GpNull.code();
const E_GP_TSS_EXEC: u16 = GdtErr::GpTssExec.code();
const E_GP_CODE_READ: u16 = GdtErr::GpCodeRead.code();
const E_GP_PRIV: u16 = GdtErr::GpPriv.code();
const E_GP_DATA_EXEC: u16 = GdtErr::GpDataExec.code();
const E_GP_READONLY: u16 = GdtErr::GpReadonly.code();

// ---------------------------------------------------------------------------
// 判据（自证）
// ---------------------------------------------------------------------------

/// 判据夹具：按设计布局编码六槽原始描述符。
fn fixture_gdt() -> Vec<u64> {
    let mut v = Vec::new();
    // NULL。
    v.push(0);
    // 内核代码：present、ring0、code、long、gran4k。
    v.push(0x00AF_9A00_0000_FFFF);
    // 内核数据：present、ring0、data、writable、gran4k。
    v.push(0x00CF_9200_0000_FFFF);
    // 用户代码：present、ring3、code、long、gran4k。
    v.push(0x00AF_FA00_0000_FFFF);
    // 用户数据：present、ring3、data、writable、gran4k。
    v.push(0x00CF_F200_0000_FFFF);
    // TSS：present、ring0、type 0x9（available 64-bit TSS）。
    v.push(0x0000_8900_0000_0067);
    v
}

/// 判据集（UNX-F0189-J1 + 结构红线）。自证六段。
pub fn self_audit() -> Result<(), u16> {
    // 一、J1 主体：重载后逐槽对拍与设计一致。
    let gdt = fixture_gdt();
    reload_compare(&gdt)?;
    // 二、布局漂移检出：内核数据槽写成 ring3 → 漂移即红。
    let mut drifted = gdt.clone();
    drifted[2] = 0x00CF_F200_0000_FFFF; // KData 槽被写成 UData。
    if reload_compare(&drifted).is_ok() {
        return Err(E_LAYOUT_DRIFT);
    }
    // 三、长度漂移：少一槽即拒。
    let short: Vec<u64> = gdt[..5].to_vec();
    if reload_compare(&short).is_ok() {
        return Err(E_LAYOUT_LEN);
    }
    // 四、异常段访问闸四向：低权直跳内核代码 #GP / 写只读 #GP /
    // 数据段执行 #GP / 合法内核数据读写放行。
    let kcode = SegDesc::decode(gdt[1]);
    let kdata = SegDesc::decode(gdt[2]);
    // 只读数据段（access 0x90：present、ring0、data、非 writable）。
    let ro_data = SegDesc::decode(0x00CF_9000_0000_FFFF);
    let a = SegAccess {
        target: Slot::KCode,
        from_ring: 3,
        write: false,
        exec: true,
    };
    if access_verdict(&kcode, &a).is_ok() {
        return Err(E_GP_PRIV);
    }
    let a = SegAccess {
        target: Slot::KData,
        from_ring: 0,
        write: true,
        exec: false,
    };
    if access_verdict(&ro_data, &a) != Err(E_GP_READONLY) {
        return Err(E_GP_READONLY);
    }
    let a = SegAccess {
        target: Slot::KData,
        from_ring: 0,
        write: false,
        exec: true,
    };
    if access_verdict(&kdata, &a).is_ok() {
        return Err(E_GP_DATA_EXEC);
    }
    let a = SegAccess {
        target: Slot::KCode,
        from_ring: 0,
        write: false,
        exec: true,
    };
    if access_verdict(&kcode, &a).is_err() {
        return Err(E_GP_PRIV);
    }
    // 特权方向锚：Ring0 访问 Ring3 数据合法（DPL 高不构成越权）。
    let udata = SegDesc::decode(gdt[4]);
    let a = SegAccess {
        target: Slot::UData,
        from_ring: 0,
        write: true,
        exec: false,
    };
    if access_verdict(&udata, &a).is_err() {
        return Err(E_GP_PRIV);
    }
    // 五、NULL/TSS 闸：NULL 槽访问 #GP、TSS 执行 #GP（语义固化双锚）。
    let null_desc = SegDesc::decode(gdt[0]);
    let tss = SegDesc::decode(gdt[5]);
    let a = SegAccess {
        target: Slot::Null,
        from_ring: 0,
        write: false,
        exec: false,
    };
    if access_verdict(&null_desc, &a) != Err(E_GP_NULL) {
        return Err(E_GP_NULL);
    }
    let a = SegAccess {
        target: Slot::Tss,
        from_ring: 0,
        write: false,
        exec: true,
    };
    if access_verdict(&tss, &a) != Err(E_GP_TSS_EXEC) {
        return Err(E_GP_TSS_EXEC);
    }
    // 六、选择子序与判别式段位对拍。
    if selector(Slot::KCode) != 8 || selector(Slot::Tss) != 40 {
        return Err(E_LAYOUT_DRIFT);
    }
    if E_GP_READONLY != SEG_BASE + 9 || E_LAYOUT_LEN != SEG_BASE + 1 {
        return Err(E_GP_READONLY);
    }
    Ok(())
}
