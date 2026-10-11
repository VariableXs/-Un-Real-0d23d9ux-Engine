//! UNX-F0041 · DSDT 采集与校验链（CoRun·Unxreal · UNX-A1-B03 · ACPI 深化之二）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md#UNX-F0041`
//!
//! **判据 J1**：真机引导后定位 DSDT 指针、校验和与 OEM ID 提取落账，
//! 坏表时引导报告列出诊断项且不阻塞引导。
//!
//! ## 要点一：DSDT 指针定位——X_DSDT 优先回退 32 位
//!
//! FACP（FADT）revision >= 2 用 140 偏移的 64 位 X_DSDT 指针；X_DSDT
//! 为零或 revision 1 回退 40 偏移的 32 位 DSDT 指针——双字段回退链显性。
//!
//! ## 要点二：校验和与 OEM ID 提取落账
//!
//! 表头复用 `crate::acpi::SdtHeader`（签名/长度/revision/checksum/
//! OEM ID/OEM Table ID）；校验和规则复用 `crate::acpi::checksum_ok`
//! （全表字节和 mod 256 为零）——基础解析复用不另立。
//!
//! ## 要点三：坏表诊断不阻塞引导
//!
//! 诊断项闭集五类（指针为零/签名不符/校验和坏/长度越界/OEM ID 空）；
//! 任何坏表只向引导报告追加诊断项（DsdtReport），引导继续——诊断项
//! 是报告不是中止（防重：分层深化不改 F008 的 continue 语义）。
//!
//! ## 要点四：零 panic 面 + 诊断码独占 0x2Exx 段
//!
//! 与 CGPU 册（0x50..0x5D）/cga02（0x39）/VE 册（0x43/0x44）等互不
//! 重叠；本模块零 panic 面——所有输入不可信（固件表可被改坏）。

use crate::acpi::{checksum_ok, SdtHeader, SDT_HEADER_LEN};
use alloc::string::String;

// ---------------------------------------------------------------------------
// 一、诊断项闭集（独占 0x2Exx 段）
// ---------------------------------------------------------------------------

/// DSDT 采集诊断码。独占 `0x2Exx` 段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DsCode(pub u16);

impl DsCode {
    /// FACP 内 DSDT 指针为零。
    pub const PTR_ZERO: DsCode = DsCode(0x2E01);
    /// 表签名不符（非 "DSDT"）。
    pub const SIGNATURE_MISMATCH: DsCode = DsCode(0x2E02);
    /// 全表校验和不过。
    pub const CHECKSUM_BAD: DsCode = DsCode(0x2E03);
    /// 表长度越界（0 或超读面）。
    pub const LENGTH_OUT_OF_BOUNDS: DsCode = DsCode(0x2E04);
    /// OEM ID 空（全零/全空格——固件偷懒可检）。
    pub const OEM_ID_EMPTY: DsCode = DsCode(0x2E05);

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn reason(self) -> String {
        match self {
            DsCode::PTR_ZERO => "DSDT 指针为零：FACP 双字段（X_DSDT/DSDT）皆空".into(),
            DsCode::SIGNATURE_MISMATCH => "表签名不符：目标地址表头不是 DSDT".into(),
            DsCode::CHECKSUM_BAD => "全表校验和不过：字节和 mod 256 非零".into(),
            DsCode::LENGTH_OUT_OF_BOUNDS => "表长度越界：零长度或超读面".into(),
            DsCode::OEM_ID_EMPTY => "OEM ID 空：全零或全空格，固件偷懒可检".into(),
            DsCode(_) => "未知 UNX DSDT 采集域诊断码".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 二、DSDT 指针定位——X_DSDT 优先回退 32 位
// ---------------------------------------------------------------------------

/// FACP revision 2+ 的 X_DSDT 64 位指针偏移。
pub const FACP_X_DSDT_OFFSET: usize = 140;
/// FACP revision 1 的 DSDT 32 位指针偏移。
pub const FACP_DSDT_OFFSET: usize = 40;

/// 从 FACP 表体定位 DSDT 物理指针（双字段回退链）。
///
/// 规则：① 表体不足以覆盖目标偏移→None（读面越界防护）；② revision
/// >= 2 且 X_DSDT 非零→用 64 位指针；③ 否则回退 32 位 DSDT 字段；
/// ④ 两字段皆零→诊断 PTR_ZERO。
pub fn locate_dsdt_ptr(facp: &[u8]) -> Result<u64, DsCode> {
    let revision = if facp.len() >= SDT_HEADER_LEN { facp[8] } else { 0 };
    let x_dsdt = if facp.len() >= FACP_X_DSDT_OFFSET + 8 {
        let b = &facp[FACP_X_DSDT_OFFSET..FACP_X_DSDT_OFFSET + 8];
        u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
    } else {
        0
    };
    let dsdt32 = if facp.len() >= FACP_DSDT_OFFSET + 4 {
        let b = &facp[FACP_DSDT_OFFSET..FACP_DSDT_OFFSET + 4];
        u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as u64
    } else {
        0
    };
    if revision >= 2 && x_dsdt != 0 {
        return Ok(x_dsdt);
    }
    if dsdt32 != 0 {
        return Ok(dsdt32);
    }
    Err(DsCode::PTR_ZERO)
}

// ---------------------------------------------------------------------------
// 三、DSDT 表采集落账——校验和与 OEM ID 提取
// ---------------------------------------------------------------------------

/// DSDT 采集账（真机引导后落账——OEM ID/校验和/长度可审计）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DsdtInfo {
    /// DSDT 物理指针。
    pub dsdt_addr: u64,
    /// 表长度（表头 length 字段）。
    pub length: u32,
    /// 表 revision。
    pub revision: u8,
    /// OEM ID（6 字节 ASCII——逐字节提取留痕）。
    pub oem_id: [u8; 6],
    /// OEM Table ID（8 字节 ASCII）。
    pub oem_table_id: [u8; 8],
    /// 全表校验和是否通过。
    pub checksum_ok: bool,
}

/// OEM ID 有效性：非全零且非全空格（固件偷懒可检——诊断 OEM_ID_EMPTY）。
pub fn oem_id_valid(oem_id: &[u8; 6]) -> bool {
    let all_zero = oem_id.iter().all(|&b| b == 0);
    let all_space = oem_id.iter().all(|&b| b == b' ');
    !(all_zero || all_space)
}

/// 由 DSDT 表字节采集信息账：签名/长度/校验和/OEM ID 逐项校验。
///
/// 规则：① 表头签名必须为 "DSDT"；② 长度非零且不超读面；③ 全表校验
/// 和复用 checksum_ok；④ OEM ID 有效性复用 oem_id_valid；⑤ 通过→
/// DsdtInfo 落账，任一不过→对应诊断码（坏表不 panic——诊断入报告）。
pub fn collect(dsdt_addr: u64, table: &[u8]) -> Result<DsdtInfo, DsCode> {
    if table.len() < SDT_HEADER_LEN {
        return Err(DsCode::LENGTH_OUT_OF_BOUNDS);
    }
    let header = match SdtHeader::parse(table) {
        Some(h) => h,
        None => return Err(DsCode::LENGTH_OUT_OF_BOUNDS),
    };
    if &header.signature != b"DSDT" {
        return Err(DsCode::SIGNATURE_MISMATCH);
    }
    if header.length == 0 || (header.length as usize) > table.len() {
        return Err(DsCode::LENGTH_OUT_OF_BOUNDS);
    }
    let ck = checksum_ok(table, header.length as usize);
    if !ck {
        return Err(DsCode::CHECKSUM_BAD);
    }
    if !oem_id_valid(&header.oem_id) {
        return Err(DsCode::OEM_ID_EMPTY);
    }
    Ok(DsdtInfo {
        dsdt_addr,
        length: header.length,
        revision: header.revision,
        oem_id: header.oem_id,
        oem_table_id: header.oem_table_id,
        checksum_ok: ck,
    })
}

// ---------------------------------------------------------------------------
// 四、引导报告——坏表诊断不阻塞引导
// ---------------------------------------------------------------------------

/// 引导报告可容纳的诊断项上限（定长容量纪律——不触发堆分配超限）。
pub const MAX_DIAGS: usize = 8;

/// DSDT 引导报告（诊断项列表+健康位——坏表只报告不中止）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DsdtReport {
    /// 采集账（全部诊断过闸才有值）。
    pub info: Option<DsdtInfo>,
    /// 诊断项（按发生序累积，容量 MAX_DIAGS 截断如实标注）。
    pub diags: [Option<DsCode>; MAX_DIAGS],
    /// 诊断项实数（<= MAX_DIAGS，超出截断不静默）。
    pub diag_count: usize,
    /// 是否发生截断。
    pub truncated: bool,
    /// 健康位——零诊断项且落账成功。
    pub healthy: bool,
}

impl DsdtReport {
    /// 空报告（引导起步形态）。
    pub fn new() -> DsdtReport {
        DsdtReport {
            info: None,
            diags: [None; MAX_DIAGS],
            diag_count: 0,
            truncated: false,
            healthy: false,
        }
    }

    /// 追加诊断项（超容量如实标注截断——不静默吞）。
    pub fn push(&mut self, code: DsCode) {
        if self.diag_count < MAX_DIAGS {
            self.diags[self.diag_count] = Some(code);
        } else {
            self.truncated = true;
        }
        self.diag_count += 1;
        self.healthy = false;
    }
}

/// 不阻塞引导语义声明（判据逐字对拍）。
pub const NON_BLOCKING_NOTE: &str =
    "坏表时引导报告列出诊断项且不阻塞引导——诊断项是报告不是中止，引导继续";

/// 采集合成入口：FACP 表体 + DSDT 表体（可选——指针坏时读不到）→ 报告。
///
/// 规则：① FACP 定位指针失败→PTR_ZERO 入报告；② DSDT 表体缺失→
/// 同样入报告（真机 HHDM 映射失败时调用方传 None——采集层不做映射）；
/// ③ collect 逐项校验，任何诊断码累积入报告；④ 报告无论好坏都返回
/// ——调用方（引导）继续，不阻塞。
pub fn boot_collect(facp: &[u8], dsdt: Option<(u64, &[u8])>) -> DsdtReport {
    let mut r = DsdtReport::new();
    let addr = match locate_dsdt_ptr(facp) {
        Ok(a) => a,
        Err(code) => {
            r.push(code);
            return r;
        }
    };
    let table = match dsdt {
        Some((a, t)) => {
            if a != addr {
                r.push(DsCode::SIGNATURE_MISMATCH);
                return r;
            }
            t
        }
        None => {
            r.push(DsCode::PTR_ZERO);
            return r;
        }
    };
    match collect(addr, table) {
        Ok(info) => {
            r.info = Some(info);
            r.healthy = true;
        }
        Err(code) => r.push(code),
    }
    r
}

// ---------------------------------------------------------------------------
// 五、真机引导落账（单次提交——引导后可查不可改）
// ---------------------------------------------------------------------------

use core::sync::atomic::{AtomicBool, Ordering};

static REPORT: ReportSlot = ReportSlot::new();
static COMMITTED: AtomicBool = AtomicBool::new(false);

/// 报告落账槽（零堆分配——DsdtReport 为定长值类型）。
struct ReportSlot {
    slot: core::cell::UnsafeCell<Option<DsdtReport>>,
}

unsafe impl Sync for ReportSlot {}

impl ReportSlot {
    const fn new() -> ReportSlot {
        ReportSlot { slot: core::cell::UnsafeCell::new(None) }
    }
}

/// 引导期提交报告（恰一次——重复提交拒，真机引导只走一遍）。
pub fn boot_commit(report: DsdtReport) -> Result<(), DsCode> {
    if COMMITTED.swap(true, Ordering::AcqRel) {
        return Err(DsCode::PTR_ZERO);
    }
    unsafe {
        (*REPORT.slot.get()).get_or_insert(report);
    }
    Ok(())
}

/// 引导后读取报告（未提交返回 None——只读不改）。
pub fn committed_report() -> Option<DsdtReport> {
    if !COMMITTED.load(Ordering::Acquire) {
        return None;
    }
    unsafe { (*REPORT.slot.get()).clone() }
}

// ---------------------------------------------------------------------------
// 六、测试支撑（判据 J1 全链：定位/提取/坏表不阻塞）
// ---------------------------------------------------------------------------

#[cfg(all(test, not(feature = "kernel-image")))]
mod tests {
    use super::*;

    /// 构造一个最小 FACP（revision 2、X_DSDT 指向 0x12345000）。
    fn facp_v2(x_dsdt: u64) -> alloc::vec::Vec<u8> {
        let mut v = alloc::vec![0u8; 148];
        v[0..4].copy_from_slice(b"FACP");
        v[8] = 2;
        v[140..148].copy_from_slice(&x_dsdt.to_le_bytes());
        v
    }

    /// 构造一张校验和合法的 DSDT 表。
    fn dsdt_good(oem: &[u8; 6]) -> alloc::vec::Vec<u8> {
        let mut v = alloc::vec![0u8; 64];
        v[0..4].copy_from_slice(b"DSDT");
        let len = v.len() as u32;
        v[4..8].copy_from_slice(&len.to_le_bytes());
        v[8] = 2;
        v[10..16].copy_from_slice(oem);
        let sum = v.iter().fold(0u8, |a, &b| a.wrapping_add(b));
        v[9] = sum.wrapping_neg();
        v
    }

    #[test]
    fn 定位提取与坏表诊断() {
        // X_DSDT 优先
        let f = facp_v2(0x1234_5000);
        assert_eq!(locate_dsdt_ptr(&f), Ok(0x1234_5000));
        // 双零指针 → PTR_ZERO
        let f0 = facp_v2(0);
        assert_eq!(locate_dsdt_ptr(&f0), Err(DsCode::PTR_ZERO));
        // 好表采集：OEM ID 逐字节提取落账
        let t = dsdt_good(b"VARIX1");
        let info = collect(0x1234_5000, &t).unwrap();
        assert_eq!(&info.oem_id, b"VARIX1");
        assert!(info.checksum_ok);
        // 坏表（校验和翻转一字节）→ CHECKSUM_BAD，不 panic
        let mut bad = dsdt_good(b"VARIX1");
        bad[30] ^= 0xFF;
        assert_eq!(collect(0x1234_5000, &bad), Err(DsCode::CHECKSUM_BAD));
    }

    #[test]
    fn 报告不阻塞与提交恰一次() {
        // 好链：报告 healthy 且 info 落账
        let f = facp_v2(0x1234_5000);
        let t = dsdt_good(b"VARIX1");
        let r = boot_collect(&f, Some((0x1234_5000, &t)));
        assert!(r.healthy && r.info.is_some());
        // 坏链：坏表诊断入报告但不 panic（不阻塞引导语义）
        let mut bad = dsdt_good(b"VARIX1");
        bad[30] ^= 0xFF;
        let r2 = boot_collect(&f, Some((0x1234_5000, &bad)));
        assert!(!r2.healthy && r2.diag_count == 1);
        // OEM ID 空 → OEM_ID_EMPTY
        let blank = dsdt_good(b"      ");
        assert_eq!(collect(0x1234_5000, &blank), Err(DsCode::OEM_ID_EMPTY));
        // 提交恰一次（模块级槽——单测内第二次提交拒）
        assert!(boot_commit(r).is_ok());
        assert!(boot_commit(r2).is_err());
        assert!(committed_report().is_some());
    }
}
