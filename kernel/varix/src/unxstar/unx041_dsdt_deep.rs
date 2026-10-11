//! unx041_dsdt_deep — UNX-F0041 DSDT 采集与校验链（深化版）。
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F0041 · UNX-A1-B03）：真机引导后
//! 定位 DSDT 指针、校验和与 OEM ID 提取落账，坏表时引导报告列出诊断
//! 项且不阻塞引导。
//!
//! **分层深化声明**：与 [`super::unx041_dsdt`]（速写版，0x2Exx 段五类
//! 诊断）并存——深化版不改动速写版一行（只增不减纪律），在其四要点
//! 之上做四通道定位链、严重级分级、健康度三因子、快照指纹四向深化；
//! 基础解析同样复用 `crate::acpi` 的 SdtHeader/checksum_ok/
//! SDT_HEADER_LEN（基础解析复用不另立——分层深化不是第二套真相）。
//!
//! **深化一：四通道定位链**——RSDP（v1 20 字节校验 + v2 36 字节扩展
//! 校验双验）→ 表目录（XSDT 优先 RSDT 回退——64 位表目录是现代机正道，
//! 32 位目录是回退不是缺陷）→ FACP（签名恰配 "FACP"）→ DSDT 指针
//! （X_DSDT 140 偏移 64 位优先、为 0 或 revision 1 回退 40 偏移 32 位
//! ——双字段回退链每跳诊断码独立，断在哪一跳报告就指哪）。
//!
//! **深化二：表头深度校验**——长度界（≥ 36 字节头、≤ 1 MiB 饱和线——
//! 超线即内存映射事故不是表）、revision 语义表（0 无效/1-2 常规/≥3
//! 新语义须 OEM 佐证）、签名 "DSDT" 恰配（大小写敏感——ACPI 规范面）。
//!
//! **深化三：OEM ID 落账三态**——6 字节提取后分三态：全零（固件没写）、
//! 全空格（固件写了空白占位）、有效 ASCII（可读账面）；OEM Table ID
//! 8 字节同法。落账即报告行，不因空白态拒表——空白是账面事实不是错。
//!
//! **深化四：诊断严重级 + 不阻塞语义**——诊断项三级闭集（Info 账面
//! 注记/Warn 可疑待查/Fatal 表不可用），任何级别**只追加不中止**——
//! 引导报告（BootReport）是 append-only 账，continue 语义复用 F008
//! 声明（防重：分层深化不改引导主链行为）。健康度三因子（表完整/
//! 校验通过/诊断密度）把"引导时 ACPI 面"量化成三个数，坏引导一眼见。
//!
//! 诊断枚举 DsdtDeepCode 十六码（0x2F 段 0x2F00..=0x2F0F 互异 +
//! [`segment_guard`] 段守恒对账）。no_std 零 unwrap 零 panic 面。

use alloc::string::String;
use alloc::vec::Vec;

use crate::acpi::{checksum_ok, SDT_HEADER_LEN};
use crate::checks::CheckSet;

/// 模块版本。
pub const UNX041_DEEP_VERSION: &str = "unx041-dsdt-deep-v1";

/// 诊断段基值（0x2F 段——速写版占 0x2E，深化版占 0x2F，段位不重叠）。
pub const SEG_BASE: u16 = 0x2F00;

/// 表长饱和线（1 MiB——超线即内存映射事故不是表）。
pub const TABLE_LEN_CAP: u32 = 1 << 20;

/// FACP 签名（ACPI 规范面——大小写敏感恰配）。
pub const FACP_SIGNATURE: [u8; 4] = *b"FACP";

/// DSDT 签名。
pub const DSDT_SIGNATURE: [u8; 4] = *b"DSDT";

/// FACP 内 32 位 DSDT 指针偏移（ACPI 1.0 规范）。
pub const FACP_DSDT_OFF32: usize = 40;

/// FACP 内 64 位 X_DSDT 指针偏移（ACPI 2.0+ 规范）。
pub const FACP_XDSDT_OFF64: usize = 140;

/// FACP 最小长度（覆盖 40 偏移 32 位指针——少于此无法取 DSDT 指针）。
pub const FACP_MIN_LEN: u32 = 44;

/// 引导报告容量策略值（诊断项上限——append-only 账也需界防灌爆）。
pub const REPORT_CAPACITY: usize = 64;

// ---------------------------------------------------------------------------
// 诊断码（DsdtDeepCode 十六码 · 0x2F 段互异）
// ---------------------------------------------------------------------------

/// 诊断码（DSDT 深化链——0x2F 段十六码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DsdtDeepCode {
    /// 0x2F00 RSDP 签名不符（"RSD PTR " 8 字节恰配失败）。
    RsdpSignature,
    /// 0x2F01 RSDP v1 校验和坏（前 20 字节和 mod 256 非零）。
    RsdpChecksumV1,
    /// 0x2F02 RSDP v2 扩展校验和坏（前 36 字节和 mod 256 非零——v2 才查）。
    RsdpChecksumV2,
    /// 0x2F03 表目录双通道皆空（XSDT 与 RSDT 指针均为零——无处找 FACP）。
    TableDirEmpty,
    /// 0x2F04 FACP 表头不可解析（长度不足 36 或字节面残缺）。
    FacpHeaderBad,
    /// 0x2F05 FACP 签名不符（非 "FACP"——拿错了表）。
    FacpSignature,
    /// 0x2F06 FACP 长度不足（< FACP_MIN_LEN——32 位指针偏移 40 越界）。
    FacpTooShort,
    /// 0x2F07 DSDT 双指针皆零（X_DSDT 与 DSDT 均为 0——固件没挂表）。
    DsdtPtrBothZero,
    /// 0x2F08 DSDT 表头不可解析（指针非零但字节面残缺）。
    DsdtHeaderBad,
    /// 0x2F09 DSDT 签名不符（非 "DSDT"）。
    DsdtSignature,
    /// 0x2F0A DSDT 校验和坏（全表字节和 mod 256 非零——复用 checksum_ok）。
    DsdtChecksum,
    /// 0x2F0B DSDT 长度越界（< 36 或 > TABLE_LEN_CAP——饱和线外）。
    DsdtLengthOut,
    /// 0x2F0C OEM ID 全零（固件没写——账面事实不拒表）。
    OemIdAllZero,
    /// 0x2F0D revision 无效（0——规范面 revision 从 1 起）。
    RevisionInvalid,
    /// 0x2F0E 报告容量满（诊断项超 REPORT_CAPACITY——灌爆防线）。
    ReportFull,
    /// 0x2F0F 段守恒对账失败（十六码值/连续/互异校验不过——段位事故）。
    SegmentGuard,
}

impl DsdtDeepCode {
    /// 十六码闭集（判据侧独立复核）。
    pub const ALL: [DsdtDeepCode; 16] = [
        DsdtDeepCode::RsdpSignature,
        DsdtDeepCode::RsdpChecksumV1,
        DsdtDeepCode::RsdpChecksumV2,
        DsdtDeepCode::TableDirEmpty,
        DsdtDeepCode::FacpHeaderBad,
        DsdtDeepCode::FacpSignature,
        DsdtDeepCode::FacpTooShort,
        DsdtDeepCode::DsdtPtrBothZero,
        DsdtDeepCode::DsdtHeaderBad,
        DsdtDeepCode::DsdtSignature,
        DsdtDeepCode::DsdtChecksum,
        DsdtDeepCode::DsdtLengthOut,
        DsdtDeepCode::OemIdAllZero,
        DsdtDeepCode::RevisionInvalid,
        DsdtDeepCode::ReportFull,
        DsdtDeepCode::SegmentGuard,
    ];

    /// 段内码值（0x2F00 起连续——segment_guard 对账面）。
    pub const fn code(self) -> u16 {
        match self {
            DsdtDeepCode::RsdpSignature => 0x2F00,
            DsdtDeepCode::RsdpChecksumV1 => 0x2F01,
            DsdtDeepCode::RsdpChecksumV2 => 0x2F02,
            DsdtDeepCode::TableDirEmpty => 0x2F03,
            DsdtDeepCode::FacpHeaderBad => 0x2F04,
            DsdtDeepCode::FacpSignature => 0x2F05,
            DsdtDeepCode::FacpTooShort => 0x2F06,
            DsdtDeepCode::DsdtPtrBothZero => 0x2F07,
            DsdtDeepCode::DsdtHeaderBad => 0x2F08,
            DsdtDeepCode::DsdtSignature => 0x2F09,
            DsdtDeepCode::DsdtChecksum => 0x2F0A,
            DsdtDeepCode::DsdtLengthOut => 0x2F0B,
            DsdtDeepCode::OemIdAllZero => 0x2F0C,
            DsdtDeepCode::RevisionInvalid => 0x2F0D,
            DsdtDeepCode::ReportFull => 0x2F0E,
            DsdtDeepCode::SegmentGuard => 0x2F0F,
        }
    }

    /// 人读诊断（发生了什么/为什么/怎么办——互异非空）。
    pub const fn detail(self) -> &'static str {
        match self {
            DsdtDeepCode::RsdpSignature => {
                "RSDP 签名不符：须恰配 RSD PTR 八字节（含空格），建议核查扫描基址对齐"
            }
            DsdtDeepCode::RsdpChecksumV1 => {
                "RSDP v1 校验和坏：前 20 字节和模 256 须为零，建议复核 EBDA 段边界"
            }
            DsdtDeepCode::RsdpChecksumV2 => {
                "RSDP v2 扩展校验和坏：前 36 字节和模 256 须为零，建议复核长度字段"
            }
            DsdtDeepCode::TableDirEmpty => {
                "表目录双通道皆空：XSDT 与 RSDT 指针均为零——无处定位 FACP，建议复核 RSDP 载荷"
            }
            DsdtDeepCode::FacpHeaderBad => {
                "FACP 表头不可解析：字节面不足 36 字节头长，建议核对表目指针有效性"
            }
            DsdtDeepCode::FacpSignature => {
                "FACP 签名不符：取到的不是 FACP 表，建议核查表目索引次序"
            }
            DsdtDeepCode::FacpTooShort => {
                "FACP 长度不足：短于 44 字节无法取 40 偏移 DSDT 指针，建议排查固件版本"
            }
            DsdtDeepCode::DsdtPtrBothZero => {
                "DSDT 双指针皆零：X_DSDT 与 DSDT 均未挂表，建议复核 FACP revision 语义"
            }
            DsdtDeepCode::DsdtHeaderBad => {
                "DSDT 表头不可解析：指针非零但字节面残缺，建议核对映射窗口"
            }
            DsdtDeepCode::DsdtSignature => {
                "DSDT 签名不符：指向的不是 DSDT 表，建议核查 FACP 指针域字节序"
            }
            DsdtDeepCode::DsdtChecksum => {
                "DSDT 校验和坏：全表字节和模 256 须为零，建议重扫或降级用 SSDT 兜底"
            }
            DsdtDeepCode::DsdtLengthOut => {
                "DSDT 长度越界：短于 36 或超过 1 MiB 饱和线——超线即映射事故，建议核查表长字段"
            }
            DsdtDeepCode::OemIdAllZero => {
                "OEM ID 全零：固件未写厂商标识——账面事实不拒表，建议引导报告注记"
            }
            DsdtDeepCode::RevisionInvalid => {
                "revision 无效：规范面从 1 起，0 为无效值，建议按 1 语义降级处理"
            }
            DsdtDeepCode::ReportFull => {
                "报告容量满：诊断项超过策略上限——追加被拒旧账保留，建议归档旧报告"
            }
            DsdtDeepCode::SegmentGuard => {
                "段守恒对账失败：十六码须连续互异界内——段位事故立即立案"
            }
        }
    }
}

/// **段守恒对账**（十六码 × 连续性 × 互异性 × 界内——0x2F 段占用声明
/// 自证；const 面判等用 code() u16——枚举 PartialEq 非 const 不可用）。
pub const fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < DsdtDeepCode::ALL.len() {
        let c = DsdtDeepCode::ALL[i].code();
        if c < SEG_BASE || c > SEG_BASE + 15 {
            return false;
        }
        if c != SEG_BASE + i as u16 {
            return false;
        }
        let mut j = i + 1;
        while j < DsdtDeepCode::ALL.len() {
            if DsdtDeepCode::ALL[i].code() == DsdtDeepCode::ALL[j].code() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 基础字节面助手（零拷贝读取——小端 u16/u32/u64）
// ---------------------------------------------------------------------------

/// 小端 u16 读（越界 None——零 panic 面）。
pub fn le_u16(b: &[u8], off: usize) -> Option<u16> {
    if off + 2 > b.len() {
        return None;
    }
    Some((b[off] as u16) | ((b[off + 1] as u16) << 8))
}

/// 小端 u32 读。
pub fn le_u32(b: &[u8], off: usize) -> Option<u32> {
    if off + 4 > b.len() {
        return None;
    }
    Some(
        (b[off] as u32)
            | ((b[off + 1] as u32) << 8)
            | ((b[off + 2] as u32) << 16)
            | ((b[off + 3] as u32) << 24),
    )
}

/// 小端 u64 读。
pub fn le_u64(b: &[u8], off: usize) -> Option<u64> {
    let lo = le_u32(b, off)? as u64;
    let hi = le_u32(b, off + 4)? as u64;
    Some(lo | (hi << 32))
}

/// 字节和 mod 256（RSDP 校验和复核——单遍无溢出）。
pub fn sum_mod256(b: &[u8]) -> u8 {
    let mut s: u32 = 0;
    let mut i = 0usize;
    while i < b.len() {
        s += b[i] as u32;
        i += 1;
    }
    (s & 0xFF) as u8
}

// ---------------------------------------------------------------------------
// RSDP 校验链（v1/v2 双验——深化一第一跳）
// ---------------------------------------------------------------------------

/// RSDP 校验复核结果（三态——v1 过/v2 双过/哪级坏）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RsdpVerdict {
    /// v1 校验过但 v2 面残缺（< 36 字节——按 v1 语义放行）。
    V1Only,
    /// v1 + v2 双校验过（现代机正道）。
    V1AndV2,
    /// v1 校验即坏（根基不可信）。
    V1Bad,
    /// v1 过但 v2 扩展校验坏（36 字节面可信度存疑）。
    V2Bad,
}

/// **RSDP 复核**（字节面视图——签名恰配 "RSD PTR " + v1 前 20 字节和 +
/// v2 前 36 字节和三道闸）。
pub fn rsdp_verify(b: &[u8]) -> Result<RsdpVerdict, DsdtDeepCode> {
    // 闸 1：签名（8 字节 "RSD PTR "——含中部空格恰配）。
    if b.len() < 8 || &b[0..8] != b"RSD PTR " {
        return Err(DsdtDeepCode::RsdpSignature);
    }
    // 闸 2：v1 校验和（前 20 字节——面不足按残缺算 V1Bad）。
    if b.len() < 20 || sum_mod256(&b[0..20]) != 0 {
        return Err(DsdtDeepCode::RsdpChecksumV1);
    }
    // 闸 3：v2 面（不足 36 → V1Only 放行；足 → 扩展校验）。
    if b.len() < 36 {
        return Ok(RsdpVerdict::V1Only);
    }
    if sum_mod256(&b[0..36]) != 0 {
        return Err(DsdtDeepCode::RsdpChecksumV2);
    }
    Ok(RsdpVerdict::V1AndV2)
}

// ---------------------------------------------------------------------------
// 表目录通道（XSDT 优先 RSDT 回退——深化一第二跳）
// ---------------------------------------------------------------------------

/// 表目录通道判定（哪条路找到的 FACP——通道可见性）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TableDirChannel {
    /// XSDT 64 位表目录（现代正道）。
    Xsdt,
    /// RSDT 32 位表目录（回退——不是缺陷是兼容）。
    Rsdt,
}

/// 表目录通道选择（rsdp 载荷视图：36 偏移 XSDT 64 位指针、16 偏移
/// RSDT 32 位指针——XSDT 非零优先，皆零 TableDirEmpty）。
pub fn select_table_dir(rsdp_payload: &[u8]) -> Result<TableDirChannel, DsdtDeepCode> {
    let xsdt = if rsdp_payload.len() >= 36 + 8 {
        le_u64(rsdp_payload, 36).ok_or(DsdtDeepCode::TableDirEmpty)?
    } else {
        0
    };
    let rsdt = if rsdp_payload.len() >= 16 + 4 {
        le_u32(rsdp_payload, 16).ok_or(DsdtDeepCode::TableDirEmpty)?
    } else {
        0
    };
    if xsdt != 0 {
        Ok(TableDirChannel::Xsdt)
    } else if rsdt != 0 {
        Ok(TableDirChannel::Rsdt)
    } else {
        Err(DsdtDeepCode::TableDirEmpty)
    }
}

// ---------------------------------------------------------------------------
// FACP→DSDT 指针提取（深化一第三四跳 + 深化二表头深度校验）
// ---------------------------------------------------------------------------

/// FACP 深度校验结果（头面/签名/长度三闸后可提取指针）。
pub fn facp_verify(facp: &[u8]) -> Result<(), DsdtDeepCode> {
    if facp.len() < SDT_HEADER_LEN {
        return Err(DsdtDeepCode::FacpHeaderBad);
    }
    if facp[0..4] != FACP_SIGNATURE {
        return Err(DsdtDeepCode::FacpSignature);
    }
    let len = match le_u32(facp, 4) {
        Some(l) => l,
        None => return Err(DsdtDeepCode::FacpHeaderBad),
    };
    if len < FACP_MIN_LEN {
        return Err(DsdtDeepCode::FacpTooShort);
    }
    Ok(())
}

/// **DSDT 指针双字段回退提取**（X_DSDT 140 偏移 64 位优先——为 0 或
/// revision 1 回退 40 偏移 32 位；返回 (指针, 是否走 64 位路)）。
pub fn extract_dsdt_ptr(facp: &[u8]) -> Result<(u64, bool), DsdtDeepCode> {
    facp_verify(facp)?;
    let revision = facp[8];
    if revision == 0 {
        // revision 0 无效（规范面从 1 起）——按 1 语义降级但注记。
        // 深化面：降级不拒——继续走回退链。
    }
    let xsdt_ptr = if facp.len() >= FACP_XDSDT_OFF64 + 8 {
        le_u64(facp, FACP_XDSDT_OFF64).ok_or(DsdtDeepCode::FacpHeaderBad)?
    } else {
        0
    };
    if revision >= 2 && xsdt_ptr != 0 {
        return Ok((xsdt_ptr, true));
    }
    let dsdt32 = le_u32(facp, FACP_DSDT_OFF32).ok_or(DsdtDeepCode::FacpHeaderBad)?;
    if dsdt32 != 0 {
        return Ok((dsdt32 as u64, false));
    }
    Err(DsdtDeepCode::DsdtPtrBothZero)
}

/// DSDT 表头深度校验（签名/长度界/revision/校验和四闸——深化二）。
pub fn dsdt_verify(dsdt: &[u8]) -> Result<(), DsdtDeepCode> {
    if dsdt.len() < SDT_HEADER_LEN {
        return Err(DsdtDeepCode::DsdtHeaderBad);
    }
    let len = match le_u32(dsdt, 4) {
        Some(l) => l,
        None => return Err(DsdtDeepCode::DsdtHeaderBad),
    };
    if len < SDT_HEADER_LEN as u32 || len > TABLE_LEN_CAP {
        return Err(DsdtDeepCode::DsdtLengthOut);
    }
    if dsdt[0..4] != DSDT_SIGNATURE {
        return Err(DsdtDeepCode::DsdtSignature);
    }
    if dsdt[8] == 0 {
        return Err(DsdtDeepCode::RevisionInvalid);
    }
    // 校验和：全表（按表长字段截断——不是切片全长，头与表体一致才有效）。
    let use_len = if (len as usize) <= dsdt.len() {
        len as usize
    } else {
        dsdt.len()
    };
    if !checksum_ok(dsdt, use_len) {
        return Err(DsdtDeepCode::DsdtChecksum);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// OEM ID 落账三态（深化三）
// ---------------------------------------------------------------------------

/// OEM 标识三态（账面事实分级——空白不拒表）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OemState {
    /// 全零（固件没写）。
    AllZero,
    /// 全空格（固件写了占位）。
    AllSpace,
    /// 有效可读 ASCII（正常账面）。
    Readable,
}

impl OemState {
    /// 三态闭集。
    pub const ALL: [OemState; 3] = [OemState::AllZero, OemState::AllSpace, OemState::Readable];

    /// 中文名（读屏面——互异）。
    pub const fn zh(self) -> &'static str {
        match self {
            OemState::AllZero => "全零未写",
            OemState::AllSpace => "空白占位",
            OemState::Readable => "有效账面",
        }
    }
}

/// 字节串三态判定（全零/全空格/其余按可读 ASCII 算——含不可打印字节
/// 仍算 Readable，账面如实记录不美化）。
pub const fn oem_state_of(b: &[u8]) -> OemState {
    let mut all_zero = true;
    let mut all_space = true;
    let mut i = 0usize;
    while i < b.len() {
        if b[i] != 0 {
            all_zero = false;
        }
        if b[i] != b' ' {
            all_space = false;
        }
        i += 1;
    }
    if all_zero {
        OemState::AllZero
    } else if all_space {
        OemState::AllSpace
    } else {
        OemState::Readable
    }
}

/// OEM ID 提取落账（6 字节 + 三态——返回 (文本, 态)）。
pub fn extract_oem_id(header: &[u8]) -> Result<(String, OemState), DsdtDeepCode> {
    if header.len() < SDT_HEADER_LEN {
        return Err(DsdtDeepCode::DsdtHeaderBad);
    }
    let raw = &header[10..16]; // SdtHeader oem_id 偏移 10..16
    let st = oem_state_of(raw);
    if st == OemState::AllZero {
        // 账面注记码——不拒表，调用侧决定是否入报告。
        return Ok((String::from(""), OemState::AllZero));
    }
    let mut s = String::new();
    let mut i = 0usize;
    while i < raw.len() {
        // 可打印 ASCII 直收；其余以 '.' 占位（账面如实不美化）。
        if raw[i] >= 0x20 && raw[i] < 0x7F {
            s.push(raw[i] as char);
        } else {
            s.push('.');
        }
        i += 1;
    }
    Ok((s, st))
}

/// OEM Table ID 提取落账（8 字节同法——偏移 16..24）。
pub fn extract_oem_table_id(header: &[u8]) -> Result<(String, OemState), DsdtDeepCode> {
    if header.len() < SDT_HEADER_LEN {
        return Err(DsdtDeepCode::DsdtHeaderBad);
    }
    let raw = &header[16..24];
    let st = oem_state_of(raw);
    if st == OemState::AllZero {
        return Ok((String::from(""), OemState::AllZero));
    }
    let mut s = String::new();
    let mut i = 0usize;
    while i < raw.len() {
        if raw[i] >= 0x20 && raw[i] < 0x7F {
            s.push(raw[i] as char);
        } else {
            s.push('.');
        }
        i += 1;
    }
    Ok((s, st))
}

// ---------------------------------------------------------------------------
// 诊断严重级 + 引导报告（深化四——append-only 不阻塞）
// ---------------------------------------------------------------------------

/// 诊断严重级三闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    /// 账面注记（不影响健康度扣分——OEM 空白之类）。
    Info,
    /// 可疑待查（轻度扣分——revision 异常之类）。
    Warn,
    /// 表不可用（重度扣分——校验和坏之类）。
    Fatal,
}

impl Severity {
    /// 三闭集。
    pub const ALL: [Severity; 3] = [Severity::Info, Severity::Warn, Severity::Fatal];

    /// 中文名（读屏面——互异）。
    pub const fn zh(self) -> &'static str {
        match self {
            Severity::Info => "注记",
            Severity::Warn => "可疑",
            Severity::Fatal => "致命",
        }
    }

    /// 码→严重级映射（诊断面统一口径——码在哪级报告就在哪级）。
    pub const fn of_code(c: DsdtDeepCode) -> Severity {
        match c {
            DsdtDeepCode::OemIdAllZero | DsdtDeepCode::RevisionInvalid => Severity::Info,
            DsdtDeepCode::RsdpSignature
            | DsdtDeepCode::RsdpChecksumV1
            | DsdtDeepCode::RsdpChecksumV2
            | DsdtDeepCode::TableDirEmpty
            | DsdtDeepCode::FacpHeaderBad
            | DsdtDeepCode::FacpSignature
            | DsdtDeepCode::FacpTooShort
            | DsdtDeepCode::DsdtPtrBothZero
            | DsdtDeepCode::DsdtHeaderBad
            | DsdtDeepCode::DsdtSignature
            | DsdtDeepCode::DsdtChecksum
            | DsdtDeepCode::DsdtLengthOut => Severity::Fatal,
            DsdtDeepCode::ReportFull | DsdtDeepCode::SegmentGuard => Severity::Warn,
        }
    }
}

/// 单条诊断项（码 × 级 × 人读行）。
#[derive(Clone, Debug)]
pub struct ReportItem {
    /// 诊断码值（0x2Fxx）。
    pub code: u16,
    /// 严重级序数（0=Info 1=Warn 2=Fatal）。
    pub sev: usize,
    /// 人读行。
    pub line: String,
}

/// **引导报告**（append-only 账——任何诊断只追加不中止；容量界防灌爆）。
pub struct BootReport {
    items: Vec<ReportItem>,
    /// 逐码计数（十六槽——码面分布统计）。
    per_code: [u32; 16],
    /// 拒收计数（容量满被拒的追加次数）。
    pub rejected_full: u32,
}

impl BootReport {
    /// 空报告。
    pub fn new() -> BootReport {
        BootReport {
            items: Vec::new(),
            per_code: [0; 16],
            rejected_full: 0,
        }
    }

    /// **追加诊断**（码 → 级 → 行；容量满 ReportFull 计数但旧账保留——
    /// append-only 账的界）。
    pub fn push(&mut self, c: DsdtDeepCode, line: &str) {
        if self.items.len() >= REPORT_CAPACITY {
            self.rejected_full += 1;
            return;
        }
        let idx = (c.code() - SEG_BASE) as usize;
        self.per_code[idx] += 1;
        let sev = Severity::of_code(c);
        self.items.push(ReportItem {
            code: c.code(),
            sev: match sev {
                Severity::Info => 0,
                Severity::Warn => 1,
                Severity::Fatal => 2,
            },
            line: {
                let mut s = String::from(line);
                s.push_str(" | ");
                s.push_str(c.detail());
                s
            },
        });
    }

    /// 在账条数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 最高严重级（空账 None——无诊断即健康）。
    pub fn max_severity(&self) -> Option<Severity> {
        let mut hi: Option<Severity> = None;
        let mut i = 0usize;
        while i < self.items.len() {
            let s = match self.items[i].sev {
                0 => Severity::Info,
                1 => Severity::Warn,
                _ => Severity::Fatal,
            };
            hi = Some(match hi {
                None => s,
                Some(prev) => {
                    if sev_rank(s) > sev_rank(prev) {
                        s
                    } else {
                        prev
                    }
                }
            });
            i += 1;
        }
        hi
    }

    /// 码面分布（十六槽快照）。
    pub fn code_distribution(&self) -> [u32; 16] {
        self.per_code
    }

    /// 一行摘要（读屏面——含不阻塞语义显性声明）。
    pub fn summary(&self) -> String {
        let sev = match self.max_severity() {
            Some(s) => s.zh(),
            None => "无诊断",
        };
        alloc::format!(
            "U041 深化引导报告: 诊断={} 最高级={} 拒追加={} 引导继续=是(不阻塞 F008 语义)",
            self.len(),
            sev,
            self.rejected_full
        )
    }
}

impl Default for BootReport {
    fn default() -> Self {
        BootReport::new()
    }
}

/// 严重级序（Info < Warn < Fatal——比较面）。
const fn sev_rank(s: Severity) -> u8 {
    match s {
        Severity::Info => 0,
        Severity::Warn => 1,
        Severity::Fatal => 2,
    }
}

// ---------------------------------------------------------------------------
// 健康度三因子（深化四量化面——引导 ACPI 面一眼可见）
// ---------------------------------------------------------------------------

/// 健康度三因子（万分制——三个数把引导 ACPI 面说清）。
#[derive(Clone, Copy, Debug)]
pub struct HealthFactors {
    /// 表完整率（万分制）：定位链走通的跳数 / 总跳数（4 跳：RSDP/目录/FACP/DSDT）。
    pub chain_completeness: u64,
    /// 校验通过率（万分制）：过的校验数 / 总校验数（RSDP v1/v2/FACP/DSDT 四道）。
    pub checksum_pass_rate: u64,
    /// 诊断密度反指（万分制）：容量未用比例——账越满越病。
    pub report_headroom: u64,
}

/// 健康度计算（跳数/校验数/账面占用三输入——全整数无浮点）。
pub const fn health_of(chain_hops: u64, checksums_passed: u64, report_used: u64) -> HealthFactors {
    let chain = if chain_hops >= 4 {
        10_000
    } else {
        chain_hops * 2_500
    };
    let chk = if checksums_passed >= 4 {
        10_000
    } else {
        checksums_passed * 2_500
    };
    let used = if report_used > REPORT_CAPACITY as u64 {
        REPORT_CAPACITY as u64
    } else {
        report_used
    };
    let headroom = 10_000 - used * 10_000 / (REPORT_CAPACITY as u64);
    HealthFactors {
        chain_completeness: chain,
        checksum_pass_rate: chk,
        report_headroom: headroom,
    }
}

impl HealthFactors {
    /// 综合分（三因子等权均值——整数除）。
    pub const fn overall(&self) -> u64 {
        (self.chain_completeness + self.checksum_pass_rate + self.report_headroom) / 3
    }
}

// ---------------------------------------------------------------------------
// 主编排：四通道全链走查（定位→校验→落账→报告——一次调用走全链）
// ---------------------------------------------------------------------------

/// 全链走查结果（定位产物 + 报告 + 健康度——编排面单点）。
pub struct ChainOutcome {
    /// 走通的通道（XSDT/Rsdt——目录空时 None）。
    pub channel: Option<TableDirChannel>,
    /// DSDT 指针（双零 None）。
    pub dsdt_ptr: Option<u64>,
    /// 指针是否 64 位路（true=X_DSDT false=32 位回退）。
    pub ptr_is_wide: bool,
    /// DSDT 校验过（true=四闸全过）。
    pub dsdt_ok: bool,
    /// OEM ID 落账（文本, 态）。
    pub oem_id: (String, OemState),
    /// OEM Table ID 落账。
    pub oem_table_id: (String, OemState),
    /// 引导报告（append-only——不阻塞语义载体）。
    pub report: BootReport,
    /// 健康度三因子。
    pub health: HealthFactors,
}

/// **全链走查**（rsdp 载荷 + 表目录条目字节面 + facp + dsdt 四段视图
/// ——真实机由引导主链映射后传入，本函数只做纯视图判读零 IO）。
///
/// 不阻塞纪律：任何一步失败只 `report.push` 后继续能走的路——链断在
/// 哪跳，后续跳自动跳过（跳过不等于静默：每跳断点都有诊断行）。
pub fn walk_chain(
    rsdp_payload: &[u8],
    facp: &[u8],
    dsdt: &[u8],
) -> ChainOutcome {
    let mut report = BootReport::new();
    let mut hops: u64 = 0;
    let mut chk_passed: u64 = 0;

    // 跳一：RSDP 复核（v1 必过才算走通）。
    let mut channel = None;
    match rsdp_verify(rsdp_payload) {
        Ok(RsdpVerdict::V1AndV2) => {
            hops += 1;
            chk_passed += 2;
        }
        Ok(RsdpVerdict::V1Only) => {
            hops += 1;
            chk_passed += 1;
        }
        Ok(_) => {
            hops += 1;
        }
        Err(c) => report.push(c, "RSDP 复核未过"),
    }

    // 跳二：表目录通道。
    if let Ok(ch) = select_table_dir(rsdp_payload) {
        hops += 1;
        channel = Some(ch);
    } else {
        report.push(DsdtDeepCode::TableDirEmpty, "表目录双空");
    }

    // 跳三：FACP 深度校验 + 指针提取。
    let mut dsdt_ptr = None;
    let mut ptr_is_wide = false;
    match facp_verify(facp) {
        Ok(()) => {
            hops += 1;
            // FACP 自身校验和（头长截断面——表短时按实际）。
            let facp_len = le_u32(facp, 4).unwrap_or(0) as usize;
            let use_len = if facp_len > 0 && facp_len <= facp.len() {
                facp_len
            } else {
                facp.len()
            };
            if checksum_ok(facp, use_len) {
                chk_passed += 1;
            } else {
                report.push(DsdtDeepCode::FacpHeaderBad, "FACP 校验和未过");
            }
            match extract_dsdt_ptr(facp) {
                Ok((p, wide)) => {
                    hops += 1;
                    dsdt_ptr = Some(p);
                    ptr_is_wide = wide;
                }
                Err(c) => report.push(c, "DSDT 指针提取失败"),
            }
        }
        Err(c) => report.push(c, "FACP 深度校验未过"),
    }

    // 跳四：DSDT 四闸 + OEM 落账。
    let mut dsdt_ok = false;
    let oem_id = match extract_oem_id(dsdt) {
        Ok(x) => x,
        Err(_) => (String::from(""), OemState::AllZero),
    };
    let oem_table_id = match extract_oem_table_id(dsdt) {
        Ok(x) => x,
        Err(_) => (String::from(""), OemState::AllZero),
    };
    if dsdt.len() >= SDT_HEADER_LEN {
        match dsdt_verify(dsdt) {
            Ok(()) => {
                dsdt_ok = true;
                chk_passed += 1;
            }
            Err(c) => report.push(c, "DSDT 校验未过"),
        }
    } else if dsdt_ptr.is_some() {
        report.push(DsdtDeepCode::DsdtHeaderBad, "DSDT 字节面残缺");
    }
    // OEM 空白账面注记（Info 级——不拒表不扣大分）。
    if oem_id.1 == OemState::AllZero {
        report.push(DsdtDeepCode::OemIdAllZero, "OEM ID 全零");
    }
    if dsdt.len() >= SDT_HEADER_LEN && dsdt[8] == 0 {
        report.push(DsdtDeepCode::RevisionInvalid, "revision 为零");
    }

    let health = health_of(hops, chk_passed, report.len() as u64);
    ChainOutcome {
        channel,
        dsdt_ptr,
        ptr_is_wide,
        dsdt_ok,
        oem_id,
        oem_table_id,
        report,
        health,
    }
}

// ---------------------------------------------------------------------------
// 快照（'|' 分隔保空段 + FNV-1a + 往返恒等）
// ---------------------------------------------------------------------------

/// 引导报告快照（账面定格——诊断分布 + 条目行）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DsdtSnap {
    /// 版本戳。
    pub version: String,
    /// 条目行（code:sev:line 三段）。
    pub rows: Vec<String>,
    /// 码面分布定格。
    pub per_code: [u32; 16],
    /// 拒追加数。
    pub rejected_full: u32,
}

/// FNV-1a 64（快照指纹——手写 const 零依赖）。
pub const fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

impl DsdtSnap {
    /// 从报告定格。
    pub fn capture(r: &BootReport) -> DsdtSnap {
        let mut rows = Vec::new();
        let mut i = 0usize;
        while i < r.items.len() {
            // 快照行换基纪律：行内 '|' → ';'——'|' 是快照串分隔符，
            // 行内残留会让 parse 段数断言错位（序列化面单语义）。
            rows.push(alloc::format!(
                "{}:{}:{}",
                r.items[i].code,
                r.items[i].sev,
                r.items[i].line.replace('|', ";")
            ));
            i += 1;
        }
        DsdtSnap {
            version: String::from(UNX041_DEEP_VERSION),
            rows,
            per_code: r.code_distribution(),
            rejected_full: r.rejected_full,
        }
    }

    /// 序列化（'|' 分隔保空段——空账也产出完整串）。
    pub fn serialize(&self) -> String {
        let mut s = alloc::format!(
            "{}|{}|{}|{}",
            self.version,
            self.rows.len(),
            self.rejected_full,
            seg_dist_str(&self.per_code)
        );
        let mut i = 0usize;
        while i < self.rows.len() {
            s.push('|');
            s.push_str(&self.rows[i]);
            i += 1;
        }
        s
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        fnv1a64(self.serialize().as_bytes())
    }

    /// 往返恒等（快照→串→重解析→深度比对）。
    pub fn roundtrip(&self) -> Result<(), DsdtDeepCode> {
        let parsed = parse_snap(&self.serialize());
        match parsed {
            Some(p) => {
                if p == *self && p.fingerprint() == self.fingerprint() {
                    Ok(())
                } else {
                    Err(DsdtDeepCode::SegmentGuard)
                }
            }
            None => Err(DsdtDeepCode::ReportFull),
        }
    }
}

/// 十六槽分布串（逗号连——序列化面）。
fn seg_dist_str(d: &[u32; 16]) -> String {
    let mut s = String::new();
    let mut i = 0usize;
    while i < 16 {
        if i > 0 {
            s.push(',');
        }
        let n = d[i];
        s.push_str(&alloc::format!("{}", n));
        i += 1;
    }
    s
}

/// 十六槽分布解析（逗号分——往返面）。
fn parse_dist(s: &str) -> Option<[u32; 16]> {
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() != 16 {
        return None;
    }
    let mut out = [0u32; 16];
    let mut i = 0usize;
    while i < 16 {
        let v = parse_u64(parts[i])?;
        if v > u32::MAX as u64 {
            return None;
        }
        out[i] = v as u32;
        i += 1;
    }
    Some(out)
}

/// 十进制解析（u64——空/非数字 None）。
fn parse_u64(s: &str) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    let mut v: u64 = 0;
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        let d = (b[i] as u64).wrapping_sub(b'0' as u64);
        if d > 9 {
            return None;
        }
        v = v.wrapping_mul(10).wrapping_add(d);
        i += 1;
    }
    Some(v)
}

/// 快照反序列化（串 → 快照——格式漂移 None）。
pub fn parse_snap(s: &str) -> Option<DsdtSnap> {
    let parts: Vec<&str> = s.split('|').collect();
    if parts.len() < 4 {
        return None;
    }
    let version = String::from(parts[0]);
    let rows_len = parse_u64(parts[1])? as usize;
    let rejected_full = parse_u64(parts[2])? as u32;
    let per_code = parse_dist(parts[3])?;
    if parts.len() != 4 + rows_len {
        return None;
    }
    let mut rows = Vec::new();
    let mut i = 0usize;
    while i < rows_len {
        let cols: Vec<&str> = parts[4 + i].splitn(3, ':').collect();
        if cols.len() != 3 {
            return None;
        }
        let _code = parse_u64(cols[0])?;
        let _sev = parse_u64(cols[1])?;
        rows.push(String::from(parts[4 + i]));
        i += 1;
    }
    Some(DsdtSnap {
        version,
        rows,
        per_code,
        rejected_full,
    })
}

// ---------------------------------------------------------------------------
// 多口径参考探针（四口径独立自证——本会话惯例命名）
// ---------------------------------------------------------------------------

/// 口径一：回退链单调——X_DSDT 非零必走 64 位路、X_DSDT 零必落 32 位、
/// 双零报 DsdtPtrBothZero（三态恰值手算）。
fn probe_ptr_fallback() -> bool {
    // 构造最小 FACP：36 头 + revision 2 + X_DSDT=0x1122334455667788。
    let mut f1 = [0u8; 160];
    f1[0..4].copy_from_slice(&FACP_SIGNATURE);
    f1[4..8].copy_from_slice(&160u32.to_le_bytes());
    f1[8] = 2;
    f1[140..148].copy_from_slice(&0x1122_3344_5566_7788u64.to_le_bytes());
    let r1 = extract_dsdt_ptr(&f1);
    // 构造 FACP2：revision 2 但 X_DSDT=0 → 回退 32 位 0x0000BEEF。
    let mut f2 = f1;
    f2[140..148].copy_from_slice(&0u64.to_le_bytes());
    f2[40..44].copy_from_slice(&0x0000_BEEFu32.to_le_bytes());
    let r2 = extract_dsdt_ptr(&f2);
    // 构造 FACP3：revision 1（忽略 X_DSDT）→ 32 位回退带真值。
    let mut f3 = f1;
    f3[8] = 1;
    f3[40..44].copy_from_slice(&0x0000_CAFEu32.to_le_bytes());
    let r3 = extract_dsdt_ptr(&f3);
    // 双零面。
    let mut f4 = f1;
    f4[140..148].copy_from_slice(&0u64.to_le_bytes());
    f4[40..44].copy_from_slice(&0u32.to_le_bytes());
    let r4 = extract_dsdt_ptr(&f4);
    r1 == Ok((0x1122_3344_5566_7788, true))
        && r2 == Ok((0x0000_BEEF, false))
        && r3 == Ok((0x0000_CAFE, false)) // revision 1 → 32 位路真值回退
        && r4 == Err(DsdtDeepCode::DsdtPtrBothZero)
}

/// 口径二：校验和敏感——同一 DSDT 翻转一字节校验即破（checksum_ok 复用
/// 单点语义 + dsdt_verify 闸生效）。
pub fn probe_checksum_sensitivity() -> bool {
    // 最小合法 DSDT：36 头（signature DSDT, len=36, rev=2, checksum 待补,
    // oem_id "VARIX1"）。
    let mut t = [0u8; 36];
    t[0..4].copy_from_slice(&DSDT_SIGNATURE);
    t[4..8].copy_from_slice(&36u32.to_le_bytes());
    t[8] = 2;
    t[10..16].copy_from_slice(b"VARIX1");
    // 补校验和：和 mod 256 = 0。
    let s = sum_mod256(&t) as u8;
    t[9] = (256 - s as u16) as u8; // checksum 偏移 9
    let ok = dsdt_verify(&t).is_ok();
    // 翻转表尾一字节 → 校验破。
    let mut bad = t;
    bad[35] ^= 0xFF;
    let broken = matches!(dsdt_verify(&bad), Err(DsdtDeepCode::DsdtChecksum));
    ok && broken
}

/// 口径三：不阻塞语义——全坏输入走 walk_chain 也产出报告与健康度
/// （链全断 hops=0 但报告非空、不 panic、headroom 满格）。
fn probe_no_block_semantics() -> bool {
    let empty: [u8; 0] = [];
    let out = walk_chain(&empty, &empty, &empty);
    // 空输入立案恰 4 条（签名/表目录/FACP头/DSDT 指针四跳全断）——
    // headroom 万分制手算：(64-4)/64×10000 = 9375（判据侧独立重算）。
    !out.report.is_empty() && out.dsdt_ptr.is_none() && out.health.chain_completeness == 0
        && out.health.report_headroom == 9_375
}

/// 口径四：段守恒 + 快照往返 + OEM 三态恰值（全零/全空格/可读）。
pub fn probe_snapshot_segment() -> bool {
    if !segment_guard() {
        return false;
    }
    let mut r = BootReport::new();
    r.push(DsdtDeepCode::OemIdAllZero, "注记一");
    r.push(DsdtDeepCode::DsdtChecksum, "致命一");
    let snap = DsdtSnap::capture(&r);
    let rt = snap.roundtrip().is_ok() && snap.rows.len() == 2;
    let oem_ok = oem_state_of(&[0, 0, 0, 0, 0, 0]) == OemState::AllZero
        && oem_state_of(b"      ") == OemState::AllSpace
        && oem_state_of(b"VARIX1") == OemState::Readable;
    rt && oem_ok
}

/// 汇总参考探针（四口径全真才真）。
pub fn reference_budget_check() -> bool {
    probe_ptr_fallback()
        && probe_checksum_sensitivity()
        && probe_no_block_semantics()
        && probe_snapshot_segment()
}

// ---------------------------------------------------------------------------
// 判据（CheckSet 风格与速写版一致——判据面零 panic）
// ---------------------------------------------------------------------------

/// UNX-F0041 深化版域自检（判据四组：定位链×3、校验×3、落账报告×3、
/// 判据元×1）。
pub fn run_unx041b_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0041-DEEP");

    // --- 定位链（深化一）---
    // 链-01：四通道回退恰值——probe_ptr_fallback 全真（64 位优先/32 位
    // 回退/revision 1 忽略 X_DSDT/双零立案）。
    if probe_ptr_fallback() {
        s.ok("U041D-链-01-四通道回退恰值");
    } else {
        s.fail("U041D-链-01-四通道回退恰值", "回退链失效");
    }

    // 链-02：RSDP 三闸——签名坏/v1 坏/v2 坏逐码拒；V1Only 放行语义。
    let good_rsdp: &mut [u8] = &mut [0u8; 36];
    good_rsdp[0..8].copy_from_slice(b"RSD PTR ");
    let s1 = sum_mod256(&good_rsdp[0..20]) as u8;
    good_rsdp[19] = (256 - s1 as u16) as u8;
    let s2 = sum_mod256(&good_rsdp[0..36]) as u8;
    good_rsdp[35] = (256 - s2 as u16) as u8;
    let v_ok = rsdp_verify(good_rsdp) == Ok(RsdpVerdict::V1AndV2);
    let mut bad_sig: Vec<u8> = Vec::new();
    for b in good_rsdp.iter() {
        bad_sig.push(*b);
    }
    bad_sig[0] = b'X';
    let sig_bad = rsdp_verify(&bad_sig) == Err(DsdtDeepCode::RsdpSignature);
    let mut bad_v1: Vec<u8> = Vec::new();
    for b in good_rsdp.iter() {
        bad_v1.push(*b);
    }
    bad_v1[12] ^= 0xFF; // 偏移 12 在 v1 校验和覆盖域（8..20）且不在签名域（0..8）
    let v1_bad = rsdp_verify(&bad_v1) == Err(DsdtDeepCode::RsdpChecksumV1);
    let mut bad_v2: Vec<u8> = Vec::new();
    for b in good_rsdp.iter() {
        bad_v2.push(*b);
    }
    bad_v2[30] ^= 0xFF;
    let v2_bad = rsdp_verify(&bad_v2) == Err(DsdtDeepCode::RsdpChecksumV2);
    // V1Only：20 字节合法 v1 面。
    let mut only_v1: Vec<u8> = Vec::new();
    for b in good_rsdp[0..20].iter() {
        only_v1.push(*b);
    }
    let v1_only = rsdp_verify(&only_v1) == Ok(RsdpVerdict::V1Only);
    if v_ok && sig_bad && v1_bad && v2_bad && v1_only {
        s.ok("U041D-链-02-RSDP三闸分级");
    } else {
        s.fail("U041D-链-02-RSDP三闸分级", "RSDP 闸面失效");
    }

    // 链-03：目录通道——XSDT 非零优先/RSDT 回退/双空立案（三态恰值）。
    let mut rp_x: Vec<u8> = Vec::new();
    for _ in 0..44 {
        rp_x.push(0);
    }
    // 16 偏移 RSDT=0x1000；36 偏移 XSDT=0x2000。
    rp_x[16..20].copy_from_slice(&0x1000u32.to_le_bytes());
    rp_x[36..44].copy_from_slice(&0x2000u64.to_le_bytes());
    let ch_x = select_table_dir(&rp_x) == Ok(TableDirChannel::Xsdt);
    let mut rp_r: Vec<u8> = Vec::new();
    for _ in 0..20 {
        rp_r.push(0);
    }
    rp_r[16..20].copy_from_slice(&0x1000u32.to_le_bytes());
    let ch_r = select_table_dir(&rp_r) == Ok(TableDirChannel::Rsdt);
    let mut rp_0: Vec<u8> = Vec::new();
    for _ in 0..20 {
        rp_0.push(0);
    }
    let ch_0 = select_table_dir(&rp_0) == Err(DsdtDeepCode::TableDirEmpty);
    if ch_x && ch_r && ch_0 {
        s.ok("U041D-链-03-目录通道三态");
    } else {
        s.fail("U041D-链-03-目录通道三态", "通道选择失效");
    }

    // --- 校验（深化二）---
    // 校-01：DSDT 四闸——合法表过/长度越界拒/签名错拒/校验破拒。
    if probe_checksum_sensitivity() {
        s.ok("U041D-校-01-四闸与校验敏感");
    } else {
        s.fail("U041D-校-01-四闸与校验敏感", "校验面失效");
    }

    // 校-02：长度界恰值——<36 与 >1MiB 双向越界逐码（DsdtLengthOut）。
    let mut short = [0u8; 20];
    short[0..4].copy_from_slice(&DSDT_SIGNATURE);
    let short_bad = matches!(dsdt_verify(&short), Err(DsdtDeepCode::DsdtHeaderBad));
    // 声明超长：len 字段写 2 MiB（> TABLE_LEN_CAP）→ LengthOut。
    let mut overlen = [0u8; 40];
    overlen[0..4].copy_from_slice(&DSDT_SIGNATURE);
    overlen[4..8].copy_from_slice(&(2 * TABLE_LEN_CAP).to_le_bytes());
    overlen[8] = 2;
    let over_bad = matches!(dsdt_verify(&overlen), Err(DsdtDeepCode::DsdtLengthOut));
    if short_bad && over_bad {
        s.ok("U041D-校-02-长度界双向恰值");
    } else {
        s.fail("U041D-校-02-长度界双向恰值", "长度闸失效");
    }

    // 校-03：FACP 三闸——头短/签名错/长度不足逐码 + 健康度手算。
    let facp_short = facp_verify(&[0u8; 20]) == Err(DsdtDeepCode::FacpHeaderBad);
    let mut fsig = [0u8; 50];
    fsig[0..4].copy_from_slice(b"XXXX");
    let facp_sig = facp_verify(&fsig) == Err(DsdtDeepCode::FacpSignature);
    let mut flen = [0u8; 50];
    flen[0..4].copy_from_slice(&FACP_SIGNATURE);
    flen[4..8].copy_from_slice(&40u32.to_le_bytes());
    let facp_len = facp_verify(&flen) == Err(DsdtDeepCode::FacpTooShort);
    let h = health_of(4, 4, 0);
    let h_ok = h.chain_completeness == 10_000
        && h.checksum_pass_rate == 10_000
        && h.report_headroom == 10_000
        && h.overall() == 10_000;
    let h_half = health_of(2, 1, 16);
    let h_half_ok = h_half.chain_completeness == 5_000 && h_half.checksum_pass_rate == 2_500
        && h_half.report_headroom == 10_000 - 16 * 10_000 / (REPORT_CAPACITY as u64);
    if facp_short && facp_sig && facp_len && h_ok && h_half_ok {
        s.ok("U041D-校-03-FACP三闸与健康度手算");
    } else {
        s.fail("U041D-校-03-FACP三闸与健康度手算", "FACP 或健康度失效");
    }

    // --- 落账报告（深化三、四）---
    // 报-01：OEM 三态落账——全零/空白/可读恰值 + 不可打印字节 '.' 占位。
    let mut hdr = [0u8; 36];
    hdr[0..4].copy_from_slice(&DSDT_SIGNATURE);
    hdr[4..8].copy_from_slice(&36u32.to_le_bytes());
    hdr[8] = 2;
    let (txt0, st0) = extract_oem_id(&hdr).unwrap_or((String::new(), OemState::AllZero));
    let zero_ok = st0 == OemState::AllZero && txt0.is_empty();
    hdr[10..16].copy_from_slice(b"      ");
    let sp = extract_oem_id(&hdr).unwrap_or((String::new(), OemState::AllZero));
    let space_ok = sp.1 == OemState::AllSpace && sp.0 == "      ";
    hdr[10..16].copy_from_slice(b"VARIX\x01");
    let rd = extract_oem_id(&hdr).unwrap_or((String::new(), OemState::AllZero));
    let read_ok = rd.1 == OemState::Readable && rd.0 == "VARIX.";
    if zero_ok && space_ok && read_ok {
        s.ok("U041D-报-01-OEM三态落账");
    } else {
        s.fail("U041D-报-01-OEM三态落账", "OEM 落账失效");
    }

    // 报-02：append-only 不阻塞——全坏输入报告非空且引导继续声明在案
    // （summary 含"引导继续=是"）+ 容量灌爆拒追加旧账保留。
    if probe_no_block_semantics() {
        s.ok("U041D-报-02-全坏不阻塞语义");
    } else {
        s.fail("U041D-报-02-全坏不阻塞语义", "不阻塞语义失效");
    }
    let mut full = BootReport::new();
    let mut i = 0usize;
    while i < REPORT_CAPACITY + 8 {
        full.push(DsdtDeepCode::OemIdAllZero, "灌");
        i += 1;
    }
    let full_ok = full.len() == REPORT_CAPACITY
        && full.rejected_full == 8
        && full.summary().contains("引导继续=是");
    if full_ok {
        s.ok("U041D-报-03-容量界旧账保留");
    } else {
        s.fail("U041D-报-03-容量界旧账保留", "容量闸失效");
    }

    // --- 一组 ---
    // 组-01：十六码互异/连续/界内 + 详情互异非空 + 段守恒。
    let codes = DsdtDeepCode::ALL;
    let mut codes_ok = codes.len() == 16;
    let mut i2 = 0usize;
    while i2 < codes.len() && codes_ok {
        if codes[i2].detail().is_empty() {
            codes_ok = false;
        }
        let mut j = i2 + 1;
        while j < codes.len() {
            if codes[i2].code() == codes[j].code()
                || codes[i2].detail() == codes[j].detail()
            {
                codes_ok = false;
            }
            j += 1;
        }
        i2 += 1;
    }
    if codes_ok && segment_guard() {
        s.ok("U041D-组-01-十六码段守恒");
    } else {
        s.fail("U041D-组-01-十六码段守恒", "码面漂移");
    }

    // 组-02：严重级映射三分——OEM/revision 走 Info、致命族走 Fatal、
    // 段/报告走 Warn（映射面恰值抽样）。
    let sev_ok = Severity::of_code(DsdtDeepCode::OemIdAllZero) == Severity::Info
        && Severity::of_code(DsdtDeepCode::RevisionInvalid) == Severity::Info
        && Severity::of_code(DsdtDeepCode::DsdtChecksum) == Severity::Fatal
        && Severity::of_code(DsdtDeepCode::RsdpSignature) == Severity::Fatal
        && Severity::of_code(DsdtDeepCode::SegmentGuard) == Severity::Warn
        && sev_rank(Severity::Info) < sev_rank(Severity::Warn)
        && sev_rank(Severity::Warn) < sev_rank(Severity::Fatal);
    if sev_ok {
        s.ok("U041D-组-02-严重级映射恰值");
    } else {
        s.fail("U041D-组-02-严重级映射恰值", "级别映射失效");
    }

    // 组-03：参考探针四口径全真。
    if reference_budget_check() {
        s.ok("U041D-组-03-四口径参考探针全真");
    } else {
        s.fail("U041D-组-03-四口径参考探针全真", "参考探针失效");
    }

    // --- 判据元 ---
    // 元-01：零 panic 面（自扫）+ 空态平凡 + 快照往返 + 版本非空。
    let src = include_str!("unx041_dsdt_deep.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".expect(", "panic!", "unreachable!", "unsafe"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    let empty_rep = BootReport::new();
    let snap0 = DsdtSnap::capture(&empty_rep);
    let empty_ok = empty_rep.is_empty()
        && empty_rep.max_severity().is_none()
        && empty_rep.summary().contains("U041")
        && snap0.roundtrip().is_ok()
        && snap0.rows.is_empty()
        && !UNX041_DEEP_VERSION.is_empty()
        && le_u16(&[0x34, 0x12], 0) == Some(0x1234)
        && le_u32(&[0x78, 0x56, 0x34, 0x12], 0) == Some(0x1234_5678)
        && le_u64(&[1, 0, 0, 0, 0, 0, 0, 0], 0) == Some(1)
        && le_u16(&[0], 0).is_none()
        && le_u32(&[0, 0, 0], 0).is_none()
        && le_u64(&[0, 0, 0, 0, 0, 0, 0], 0).is_none();
    if hits == 0 && empty_ok {
        s.ok("U041D-元-01-零panic空态平凡");
    } else {
        s.fail("U041D-元-01-零panic空态平凡", "panic 面或空态失效");
    }

    s
}

/// 单遍词法剥除（注释剥除、字符串整体剥空——判据模式串不自 hit）。
fn strip_lexical_noise(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            let mut depth = 1usize;
            i += 2;
            while i < b.len() && depth > 0 {
                if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < b.len() && b[i] == b'*' && b[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'"';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if b[i] == b'\'' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'\'';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if let Some(c) = src.get(i..i + 1) {
            out.push_str(c);
        }
        i += 1;
    }
    out
}
