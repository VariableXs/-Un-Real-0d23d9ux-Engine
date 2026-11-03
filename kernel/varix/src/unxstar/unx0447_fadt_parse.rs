//! UNX-F0447 FADT 解析（计时字段账） · 0x75180段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，AI-VX7-1104）：
//!   FADT 关键字段账齐（PMTimer 块/century/长度-版本自洽），
//!   A2 消费字段预备。
//!
//! 设计要点：
//!   ① FADT 布局按 ACPI 规范建 276B v4/v5/v6 三版差分表：PM_TMR_BLK
//!      （偏移 72，32 位 I/O 端口）/ PM_TMR_LEN（偏移 87，1B，固定 4）/
//!      CENTURY（偏移 106，RTC CMOS 索引，典型 0x32）/ FLAGS 偏移 112
//!      的 TMR_VAL_EXT（位 8，电平触发溢出 SCI）逐项落位；
//!   ② 长度-版本自洽门：v1=128（"128+" 扩展变体容忍至 132）/ v2~v4=244
//!      / v5=v6=276，双向门（短拒 + 版本与长度错配拒）；
//!   ③ 表头门先行：签名 "FACP" + 表头校验和（sum mod 256 ≡ 0），
//!      缓冲短于表长显性拒；
//!   ④ 计时字段账：PMTimer 频率 3.579545MHz 整数口径（3_579_545 Hz），
//!      32 位计数器回绕周期按整数除法复算（≥1199 秒 ≈ 20 分钟）；
//!   ⑤ A2 消费字段预备：7 字段（端口/长度/频率/回绕秒/CMOS 索引/
//!      世纪可用位/电平触发位）打包成 A2Consume，账面可复算；
//!   ⑥ 单调状态机 Raw→HeaderVetted→GatePassed→FieldsLedgered→A2Prepared，
//!      倒退显性拒。
//!
//! 诊断码段：0x75180（0x40 窗口闭集，全域独占）。
//! 纯功能线：no_std + alloc，无 std::/unsafe/unwrap/expect/panic，
//! 整数算术，确定性，双跑一致。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75180，AI-VX7-1104 F0447 独占区）。
pub const SEG_BASE: u32 = 0x75180;

// ── 冻结常量表（来源：ACPI Specification 6.x，FADT 章节 §5.2.9； ──
// ── PM Timer 频率口径见 §4.1 ACPI 硬件规范「ACPI PM Timer」节） ──
/// 表头定长（Signature..Checksum + OEM 区 + Creator 区）。
pub const FADT_HDR_LEN: usize = 36;
/// FADT 1.0（revision 1）标称长度。
pub const FADT_LEN_V1: usize = 128;
/// "128+" 扩展变体容忍上限（少数 1.0 固件把扩展字段塞进 129..132）。
pub const FADT_LEN_V1_EXT_MAX: usize = 132;
/// FADT 2.0/3.0/4.0 标称长度（244 字节平台）。
pub const FADT_LEN_V4: usize = 244;
/// FADT 5.0/5.1/6.x 标称长度（276 字节平台，v6 与 v5 同长）。
pub const FADT_LEN_V5: usize = 276;
/// 表头 Revision 字节偏移（标准 ACPI 表头：Length u32 @4 占 4..8）。
pub const OFF_REVISION: usize = 8;
/// 表头 Checksum 字节偏移。
pub const OFF_CHECKSUM: usize = 9;
/// PM_TMR_BLK 偏移（32 位 I/O 端口基址）。
pub const OFF_PM_TMR_BLK: usize = 72;
/// PM_TMR_LEN 偏移（1 字节，PM Timer 寄存器组长度，固定 4）。
pub const OFF_PM_TMR_LEN: usize = 87;
/// CENTURY 偏移（RTC CMOS 世纪寄存器索引，典型 0x32；IAPC_BOOT_ARCH@108 之前）。
pub const OFF_CENTURY: usize = 106;
/// FLAGS 偏移。
pub const OFF_FLAGS: usize = 112;
/// TMR_VAL_EXT 位号（FLAGS 位 8：溢出 SCI 电平触发口径）。
pub const FLAG_TMR_VAL_EXT: u32 = 8;
/// FIRMWARE_CTRL 偏移（F0449 域账，此处只在账本里原样透传原始读数，
/// 不做任何裁定——指针裁定与 AML 纪律归 F0449）。
pub const OFF_FIRMWARE_CTRL: usize = 36;
/// DSDT 32 位指针偏移（F0449 域账，此处只透传原始读数）。
pub const OFF_DSDT32: usize = 40;
/// SCI_INT 偏移（A2 电源管理预备；44 保留 1B + 45 Preferred_PM_Profile 后）。
pub const OFF_SCI_INT: usize = 46;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FadtCode {
    /// 表头签名非 "FACP"
    SigBad,
    /// 表头校验和坏
    SumBad,
    /// FADT revision 不在支持闭集 {1,2,3,4,5,6}
    RevUnknown,
    /// 长度-版本自洽门拒（短于版本下限或错配）
    LenGate,
    /// 声明表长超出实际缓冲
    LenOver,
    /// PM_TMR_BLK 为 0：无 PM 计时器立案
    TmrMissing,
    /// PM_TMR_LEN 非 4（PM Timer 固定 32 位/4 字节）
    TmrLenBad,
    /// 状态机倒退（禁越级、禁回退）
    StageRegress,
    /// 双跑不一致（复现红线）
    NotRepro,
}

impl FadtCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FadtCode::SigBad => "签名非 FACP：这不是 FADT，先查 RSDT/XSDT 指针链再回来",
            FadtCode::SumBad => "表头校验和坏：逐字节 sum mod 256 ≠ 0，疑似表被改写",
            FadtCode::RevUnknown => "FADT revision 越闭集：仅收 1/2/3/4/5/6，拒收未知代",
            FadtCode::LenGate => "长度-版本错配：v1=128/128+、v2~v4=244、v5/v6=276，按规范回对",
            FadtCode::LenOver => "声明表长 > 缓冲实长：截断表不可信，补齐映射再解析",
            FadtCode::TmrMissing => "PM_TMR_BLK=0：无 PM 计时器，A2 需换 TSC 口径",
            FadtCode::TmrLenBad => "PM_TMR_LEN≠4：PM Timer 固定 32 位/4 字节，按规范回填",
            FadtCode::StageRegress => "状态机倒退：解析只能单向推进，禁止回退重走",
            FadtCode::NotRepro => "双跑不一致：同输入两次解析结果不同，查非确定性来源",
        }
    }
    pub const ALL: [FadtCode; 9] = [
        FadtCode::SigBad,
        FadtCode::SumBad,
        FadtCode::RevUnknown,
        FadtCode::LenGate,
        FadtCode::LenOver,
        FadtCode::TmrMissing,
        FadtCode::TmrLenBad,
        FadtCode::StageRegress,
        FadtCode::NotRepro,
    ];
}

/// 诊断码查表（闭集返回；未知码给中性文案，不越段）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < FadtCode::ALL.len() {
        if let Some(c) = FadtCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75180 段闭集内"
}

// ── 字节面读数（无裸下标：全走 .get()） ─────────────────────────
fn le16(buf: &[u8], off: usize) -> Option<u16> {
    let b0 = *buf.get(off)?;
    let b1 = *buf.get(off.checked_add(1)?)?;
    Some(u16::from(b0) | (u16::from(b1) << 8))
}

fn le32(buf: &[u8], off: usize) -> Option<u32> {
    let b0 = *buf.get(off)?;
    let b1 = *buf.get(off.checked_add(1)?)?;
    let b2 = *buf.get(off.checked_add(2)?)?;
    let b3 = *buf.get(off.checked_add(3)?)?;
    Some(u32::from(b0) | (u32::from(b1) << 8) | (u32::from(b2) << 16) | (u32::from(b3) << 24))
}

/// 表头校验和（首 `declared` 字节 sum mod 256 ≡ 0）。
fn header_sum_ok(buf: &[u8], declared: usize) -> bool {
    let mut acc: u32 = 0;
    let mut i = 0;
    while i < declared {
        if let Some(b) = buf.get(i) {
            acc += u32::from(*b);
        } else {
            return false;
        }
        i += 1;
    }
    acc & 0xFF == 0
}

// ── 长度-版本自洽门 ─────────────────────────────────────────────
/// 门裁定结果（版本家族）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FadtFamily {
    /// revision 1：128 / 128+（≤132）
    V1,
    /// revision 2~4：244 字节家族
    V4,
    /// revision 5/6：276 字节家族
    V5,
}

impl FadtFamily {
    /// 版本家族下限（供差分表记账）。
    pub fn min_len(self) -> usize {
        match self {
            FadtFamily::V1 => FADT_LEN_V1,
            FadtFamily::V4 => FADT_LEN_V4,
            FadtFamily::V5 => FADT_LEN_V5,
        }
    }
}

/// 双向门：短拒 + 错配拒。
pub fn len_gate(rev: u8, declared: usize) -> Result<FadtFamily, u32> {
    match rev {
        1 => {
            if declared >= FADT_LEN_V1 && declared <= FADT_LEN_V1_EXT_MAX {
                Ok(FadtFamily::V1)
            } else {
                Err(FadtCode::LenGate.val())
            }
        }
        2 | 3 | 4 => {
            if declared >= FADT_LEN_V4 && declared < FADT_LEN_V5 {
                Ok(FadtFamily::V4)
            } else {
                Err(FadtCode::LenGate.val())
            }
        }
        5 | 6 => {
            if declared >= FADT_LEN_V5 {
                Ok(FadtFamily::V5)
            } else {
                Err(FadtCode::LenGate.val())
            }
        }
        _ => Err(FadtCode::RevUnknown.val()),
    }
}

// ── 计时字段账本 ────────────────────────────────────────────────
/// FADT 解析账本：判据三要素 + 留底原始读数。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FadtLedger {
    /// 表头 revision 原值。
    pub revision: u8,
    /// 表头声明的表长。
    pub declared_len: usize,
    /// 版本家族（长度门裁定）。
    pub family: FadtFamily,
    /// PM_TMR_BLK（32 位 I/O 端口基址；0 = 无计时器，解析层已拒）。
    pub pm_tmr_blk: u32,
    /// PM_TMR_LEN（固定 4）。
    pub pm_tmr_len: u8,
    /// CENTURY（RTC CMOS 索引；0 = 固件未提供世纪寄存器）。
    pub century: u8,
    /// 世纪寄存器可用位（非 0 才入账）。
    pub century_available: bool,
    /// TMR_VAL_EXT（FLAGS 位 8）：溢出 SCI 电平触发口径。
    pub tmr_val_ext: bool,
    /// FIRMWARE_CTRL 原始读数（F0449 域账，此处只透传）。
    pub firmware_ctrl: u32,
    /// DSDT 32 位指针原始读数（F0449 域账，此处只透传）。
    pub dsdt32: u32,
    /// SCI_INT 原始读数（偏移 42，A2 电源管理预备）。
    pub sci_int: u16,
}

/// A2 消费字段预备包（7 字段口径，账面可复算）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct A2Consume {
    /// PM_TMR_BLK I/O 端口基址。
    pub pm_tmr_blk: u32,
    /// PM_TMR_LEN（4）。
    pub pm_tmr_len: u8,
    /// PM Timer 频率整数口径：3_579_545 Hz（3.579545MHz）。
    pub pm_tmr_freq_hz: u32,
    /// 32 位计数器回绕周期（整数除法下取整，秒）。
    pub pm_tmr_wrap_sec: u32,
    /// CENTURY 原值（典型 0x32）。
    pub century_cmos_index: u8,
    /// 世纪寄存器可用位。
    pub century_available: bool,
    /// 电平触发位（TMR_VAL_EXT）。
    pub level_triggered: bool,
}

/// PM Timer 频率冻结口径（ACPI §4.1：3.579545 MHz）。
pub const PM_TMR_FREQ_HZ: u32 = 3_579_545;

/// 回绕周期复算：2^32 / 频率（整数除法，下取整）。
pub fn pm_tmr_wrap_sec() -> u32 {
    ((1u64 << 32) / u64::from(PM_TMR_FREQ_HZ)) as u32
}

/// 从账本打包 A2 消费字段。
pub fn a2_consume(l: &FadtLedger) -> A2Consume {
    A2Consume {
        pm_tmr_blk: l.pm_tmr_blk,
        pm_tmr_len: l.pm_tmr_len,
        pm_tmr_freq_hz: PM_TMR_FREQ_HZ,
        pm_tmr_wrap_sec: pm_tmr_wrap_sec(),
        century_cmos_index: l.century,
        century_available: l.century_available,
        level_triggered: l.tmr_val_ext,
    }
}

/// A2 预备齐账谓词：7 字段口径无缺项。
pub fn a2_ready_ok(a: &A2Consume) -> bool {
    a.pm_tmr_blk != 0
        && a.pm_tmr_len == 4
        && a.pm_tmr_freq_hz == PM_TMR_FREQ_HZ
        && a.pm_tmr_wrap_sec == pm_tmr_wrap_sec()
        && a.pm_tmr_wrap_sec >= 1199
        && (a.century_available == (a.century_cmos_index != 0))
    // level_triggered（TMR_VAL_EXT）是信息位：1.0 固件允许缺位，无独立
    // 不变量可校，故不参与齐账谓词（判据口径是「账齐」非「必真」）。
}

// ── 单调状态机 ──────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FadtStage {
    Raw,
    HeaderVetted,
    GatePassed,
    FieldsLedgered,
    A2Prepared,
}

impl FadtStage {
    fn rank(self) -> u8 {
        match self {
            FadtStage::Raw => 0,
            FadtStage::HeaderVetted => 1,
            FadtStage::GatePassed => 2,
            FadtStage::FieldsLedgered => 3,
            FadtStage::A2Prepared => 4,
        }
    }
}

/// 单调推进器：只许 +1，倒退/越级一律拒。
#[derive(Clone, Copy, Debug, Default)]
pub struct FadtMachine {
    stage: u8,
}

impl FadtMachine {
    pub fn new() -> FadtMachine {
        FadtMachine { stage: FadtStage::Raw.rank() }
    }
    pub fn stage(&self) -> FadtStage {
        match self.stage {
            0 => FadtStage::Raw,
            1 => FadtStage::HeaderVetted,
            2 => FadtStage::GatePassed,
            3 => FadtStage::FieldsLedgered,
            _ => FadtStage::A2Prepared,
        }
    }
    /// 推进到 `to`：仅当恰好 +1 才收，否则立案倒退。
    pub fn advance(&mut self, to: FadtStage) -> Result<(), u32> {
        let want = to.rank();
        if want == self.stage.wrapping_add(1) {
            self.stage = want;
            Ok(())
        } else {
            Err(FadtCode::StageRegress.val())
        }
    }
}

/// 注入后重封校验和（OFF_CHECKSUM 归零再回填），保证注入只破目标闸
/// 而不被表头校验和闸提前截杀。
fn reseal_fadt(buf: &mut Vec<u8>) {
    if let Some(slot) = buf.get_mut(OFF_CHECKSUM) {
        *slot = 0;
    }
    let mut acc: u32 = 0;
    let mut i = 0;
    while i < buf.len() {
        if let Some(b) = buf.get(i) {
            acc += u32::from(*b);
        }
        i += 1;
    }
    let cks = (0x100u32 - (acc & 0xFF)) & 0xFF;
    put8(buf, OFF_CHECKSUM, cks as u8);
}

// ── FADT 夹具构造（字节面软模型，供解析与注入用） ────────────────
fn put8(buf: &mut Vec<u8>, off: usize, v: u8) {
    if let Some(slot) = buf.get_mut(off) {
        *slot = v;
    }
}

fn put32(buf: &mut Vec<u8>, off: usize, v: u32) {
    put8(buf, off, (v & 0xFF) as u8);
    put8(buf, off.wrapping_add(1), ((v >> 8) & 0xFF) as u8);
    put8(buf, off.wrapping_add(2), ((v >> 16) & 0xFF) as u8);
    put8(buf, off.wrapping_add(3), ((v >> 24) & 0xFF) as u8);
}

/// FADT 夹具：按给定参数铺 0 并落字段，最后补齐表头校验和。
pub fn build_fadt(
    rev: u8,
    declared_len: usize,
    pm_tmr_blk: u32,
    pm_tmr_len: u8,
    century: u8,
    tmr_val_ext: bool,
) -> Vec<u8> {
    let mut b: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < declared_len {
        b.push(0u8);
        i += 1;
    }
    put8(&mut b, 0, b'F');
    put8(&mut b, 1, b'A');
    put8(&mut b, 2, b'C');
    put8(&mut b, 3, b'P');
    put32(&mut b, 4, declared_len as u32);
    put8(&mut b, OFF_REVISION, rev);
    // 校验和先置 0，铺完字段再回填。
    put8(&mut b, OFF_CHECKSUM, 0);
    put32(&mut b, OFF_FIRMWARE_CTRL, 0x0000_0000);
    put32(&mut b, OFF_DSDT32, 0xBF00_0000); // DSDT 32 位读数留底（F0449 域）
    put32(&mut b, OFF_SCI_INT, 0x0009); // SCI_INT 留底
    put32(&mut b, OFF_PM_TMR_BLK, pm_tmr_blk);
    put8(&mut b, OFF_PM_TMR_LEN, pm_tmr_len);
    put8(&mut b, OFF_CENTURY, century);
    let mut flags: u32 = 0;
    if tmr_val_ext {
        flags |= 1u32 << FLAG_TMR_VAL_EXT;
    }
    put32(&mut b, OFF_FLAGS, flags);
    // 校验和回填：sum mod 256 ≡ 0。
    let mut acc: u32 = 0;
    let mut k = 0;
    while k < declared_len {
        if let Some(byte) = b.get(k) {
            acc += u32::from(*byte);
        }
        k += 1;
    }
    let cks = (0x100u32 - (acc & 0xFF)) & 0xFF;
    put8(&mut b, OFF_CHECKSUM, cks as u8);
    b
}

/// 标准真机口径夹具（v5 / 276B / QEMU 型 PM_TMR 0x0608 / century 0x32）。
pub fn fadt_v5_machine() -> Vec<u8> {
    build_fadt(5, FADT_LEN_V5, 0x0608, 4, 0x32, true)
}

/// v1 极简夹具（128B，世纪缺位）。
pub fn fadt_v1_machine() -> Vec<u8> {
    build_fadt(1, FADT_LEN_V1, 0x0608, 4, 0, false)
}

/// v4 夹具（244B）。
pub fn fadt_v4_machine() -> Vec<u8> {
    build_fadt(4, FADT_LEN_V4, 0x0408, 4, 0x32, false)
}

// ── 主解析器 ────────────────────────────────────────────────────
/// FADT 字节面解析：表头门 → 长度-版本自洽门 → 计时字段账 → 拒绝路径全显性。
pub fn parse_fadt(buf: &[u8]) -> Result<FadtLedger, u32> {
    // 门 0：连表头 36B 都放不下。
    if buf.len() < FADT_HDR_LEN {
        return Err(FadtCode::LenGate.val());
    }
    // 门 1：签名。
    let sig_ok = buf.get(0) == Some(&b'F')
        && buf.get(1) == Some(&b'A')
        && buf.get(2) == Some(&b'C')
        && buf.get(3) == Some(&b'P');
    if !sig_ok {
        return Err(FadtCode::SigBad.val());
    }
    // 门 2：长度读数 + 缓冲上限。
    let declared = match le32(buf, 4) {
        Some(v) => v as usize,
        None => return Err(FadtCode::LenGate.val()),
    };
    if declared > buf.len() {
        return Err(FadtCode::LenOver.val());
    }
    let rev = match buf.get(OFF_REVISION) {
        Some(r) => *r,
        None => return Err(FadtCode::LenGate.val()),
    };
    // 门 3：长度-版本自洽（双向）。
    let family = len_gate(rev, declared)?;
    // 门 4：表头校验和（覆盖整个声明表长）。
    if !header_sum_ok(buf, declared) {
        return Err(FadtCode::SumBad.val());
    }
    // 计时字段账。
    let pm_tmr_blk = le32(buf, OFF_PM_TMR_BLK).ok_or_else(|| FadtCode::LenGate.val())?;
    let pm_tmr_len = *buf.get(OFF_PM_TMR_LEN).ok_or_else(|| FadtCode::LenGate.val())?;
    if pm_tmr_blk == 0 {
        return Err(FadtCode::TmrMissing.val());
    }
    if pm_tmr_len != 4 {
        return Err(FadtCode::TmrLenBad.val());
    }
    let century = *buf.get(OFF_CENTURY).ok_or_else(|| FadtCode::LenGate.val())?;
    let flags = le32(buf, OFF_FLAGS).unwrap_or(0);
    let firmware_ctrl = le32(buf, OFF_FIRMWARE_CTRL).unwrap_or(0);
    let dsdt32 = le32(buf, OFF_DSDT32).unwrap_or(0);
    let sci_int = le16(buf, OFF_SCI_INT).unwrap_or(0);
    Ok(FadtLedger {
        revision: rev,
        declared_len: declared,
        family,
        pm_tmr_blk,
        pm_tmr_len,
        century,
        century_available: century != 0,
        tmr_val_ext: (flags >> FLAG_TMR_VAL_EXT) & 1 == 1,
        firmware_ctrl,
        dsdt32,
        sci_int,
    })
}

/// 全链解析（状态机驱动）：表头 → 门 → 字段 → A2 预备。
pub fn parse_fadt_full(buf: &[u8]) -> Result<(FadtLedger, A2Consume), u32> {
    let mut m = FadtMachine::new();
    // 表头门（签名在此门内先行核验前 36B 可读）。
    if buf.len() < FADT_HDR_LEN {
        return Err(FadtCode::LenGate.val());
    }
    m.advance(FadtStage::HeaderVetted)?;
    // 长度-版本门（parse_fadt 内含签名/校验和/门三闸）。
    let ledger = parse_fadt(buf)?;
    m.advance(FadtStage::GatePassed)?;
    m.advance(FadtStage::FieldsLedgered)?;
    let a2 = a2_consume(&ledger);
    if !a2_ready_ok(&a2) {
        return Err(FadtCode::TmrLenBad.val());
    }
    m.advance(FadtStage::A2Prepared)?;
    Ok((ledger, a2))
}

// ── LCG（种子固定，零随机红线合规） ─────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

// ── 注入检出账（四路：长度门/签名/校验和/TMR_LEN） ───────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t += 1;
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c += 1;
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if let (Some(t), Some(c)) = (self.tries.get(i), self.caught.get(i)) {
                if *t == 0 || c != t {
                    return false;
                }
            } else {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 注入轮（LCG 驱动，固定种子，每轮四路必抓）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(u64::from(SEG_BASE));
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：长度门注入（v5 表长缩到 244 → 门拒）。
        let short = build_fadt(5, FADT_LEN_V4, 0x0608, 4, 0x32, true);
        led.record(0, parse_fadt(&short).is_err());
        // 路 2：签名注入。
        let mut sig = fadt_v5_machine();
        put8(&mut sig, 2, b'X');
        led.record(1, parse_fadt(&sig).is_err());
        // 路 3：校验和注入。
        let mut bad = fadt_v5_machine();
        let len = bad.len();
        if let Some(slot) = bad.get_mut(len - 1) {
            *slot = slot.wrapping_add(1);
        }
        led.record(2, parse_fadt(&bad).is_err());
        // 路 4：TMR_LEN 注入（重封校验和，保证破的是 LEN 闸本身）。
        let mut tl = fadt_v5_machine();
        put8(&mut tl, OFF_PM_TMR_LEN, 3);
        reseal_fadt(&mut tl);
        led.record(3, parse_fadt(&tl) == Err(FadtCode::TmrLenBad.val()));
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：v5 真机账 → v1/v4 门账 → A2 预备 → 四路注入必抓。
pub fn golden_run() -> Result<(u32, u32), u32> {
    // v5 真机：全账绿。
    let v5 = fadt_v5_machine();
    let (l5, a5) = parse_fadt_full(&v5)?;
    let ok_v5 = (l5.family == FadtFamily::V5
        && l5.pm_tmr_blk == 0x0608
        && l5.pm_tmr_len == 4
        && l5.century == 0x32
        && l5.century_available
        && l5.tmr_val_ext
        && a2_ready_ok(&a5)) as u32;
    // v1 极简：门收，世纪缺位记不可用。
    let v1 = fadt_v1_machine();
    let (l1, a1) = parse_fadt_full(&v1)?;
    let ok_v1 = (l1.family == FadtFamily::V1
        && l1.declared_len == FADT_LEN_V1
        && !a1.century_available
        && a2_ready_ok(&a1)) as u32;
    // v4：门收。
    let v4 = fadt_v4_machine();
    let l4 = parse_fadt(&v4)?;
    let ok_v4 = (l4.family == FadtFamily::V4 && l4.declared_len == FADT_LEN_V4) as u32;
    // 四路注入必抓。
    let inj = inject_rounds(4)?;
    let ok_inj = inj.all_caught() as u32;
    // 频率/回绕口径。
    let ok_freq = (PM_TMR_FREQ_HZ == 3_579_545 && pm_tmr_wrap_sec() >= 1199) as u32;
    Ok((ok_v5 + ok_v1 + ok_v4 + ok_freq, ok_inj))
}

/// 双跑一致（复现红线）。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(FadtCode::NotRepro.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < FadtCode::ALL.len() {
        if let Some(c) = FadtCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. v5 真机全账绿。
    let v5 = parse_fadt_full(&fadt_v5_machine());
    v.push((
        "F0447-v5全账",
        match &v5 {
            Ok((l, a)) => {
                l.family == FadtFamily::V5
                    && l.pm_tmr_blk == 0x0608
                    && l.pm_tmr_len == 4
                    && l.century == 0x32
                    && l.century_available
                    && l.tmr_val_ext
                    && a2_ready_ok(a)
            }
            Err(_) => false,
        },
        match &v5 {
            Ok((l, _)) => format!("rev={} blk=0x{:04X} cent=0x{:02X}", l.revision, l.pm_tmr_blk, l.century),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // 2. 三版门收：v1=128 / 128+(130) / v4=244。
    let g1 = parse_fadt(&build_fadt(1, FADT_LEN_V1, 0x0608, 4, 0, false));
    let g1x = parse_fadt(&build_fadt(1, 130, 0x0608, 4, 0x32, true));
    let g4 = parse_fadt(&fadt_v4_machine());
    v.push((
        "F0447-三版门收",
        g1.is_ok() && g1x.is_ok() && g4.is_ok(),
        format!(
            "v1={} v1x={} v4={}",
            g1.is_ok(),
            g1x.is_ok(),
            g4.is_ok()
        ),
    ));

    // 3. 长度门注入必抓（v5 缩 244 / v1 缩 120 / v4 撑 276 / v1 撑 200）。
    let s1 = parse_fadt(&build_fadt(5, FADT_LEN_V4, 0x0608, 4, 0x32, true));
    let s2 = parse_fadt(&build_fadt(1, 120, 0x0608, 4, 0, false));
    let s3 = parse_fadt(&build_fadt(4, FADT_LEN_V5, 0x0608, 4, 0x32, true));
    let s4 = parse_fadt(&build_fadt(1, 200, 0x0608, 4, 0, false));
    v.push((
        "F0447-长度门注入",
        s1.is_err() && s2.is_err() && s3.is_err() && s4.is_err(),
        String::from("v5缩/v1缩/v4撑/v1撑 四向全拒"),
    ));

    // 4. revision 闭集外拒（0 与 7）。
    let r0 = len_gate(0, FADT_LEN_V5);
    let r7 = len_gate(7, FADT_LEN_V5);
    v.push((
        "F0447-版本闭集",
        r0 == Err(FadtCode::RevUnknown.val()) && r7 == Err(FadtCode::RevUnknown.val()),
        String::from("rev 0/7 越闭集拒"),
    ));

    // 5. 声明表长超缓冲拒。
    let mut over = fadt_v5_machine();
    put32(&mut over, 4, 4096);
    let ov = parse_fadt(&over);
    v.push((
        "F0447-超缓冲拒",
        ov == Err(FadtCode::LenOver.val()),
        String::from("declared>buf 显性拒"),
    ));

    // 6. 签名注入必抓。
    let mut sig = fadt_v5_machine();
    put8(&mut sig, 3, b'X');
    let sg = parse_fadt(&sig);
    v.push((
        "F0447-签名注入",
        sg == Err(FadtCode::SigBad.val()),
        String::from("FACP→FACX 拒"),
    ));

    // 7. 校验和注入必抓。
    let mut bad = fadt_v5_machine();
    let bl = bad.len();
    if let Some(slot) = bad.get_mut(bl - 1) {
        *slot = slot.wrapping_add(0x40);
    }
    let bs = parse_fadt(&bad);
    v.push((
        "F0447-校验和注入",
        bs == Err(FadtCode::SumBad.val()),
        String::from("尾字节扰动必抓"),
    ));

    // 8. TMR_LEN 注入必抓 + TMR_BLK 缺失立案。
    let mut tl = fadt_v5_machine();
    put8(&mut tl, OFF_PM_TMR_LEN, 8);
    reseal_fadt(&mut tl);
    let t1 = parse_fadt(&tl);
    let no_tmr = build_fadt(5, FADT_LEN_V5, 0, 4, 0x32, true);
    let t2 = parse_fadt(&no_tmr);
    v.push((
        "F0447-计时注入",
        t1 == Err(FadtCode::TmrLenBad.val()) && t2 == Err(FadtCode::TmrMissing.val()),
        String::from("LEN≠4 拒 + BLK=0 立案"),
    ));

    // 9. century 两态账（0x32 入账 / 0 记不可用且不拒）。
    let c1 = parse_fadt(&build_fadt(5, FADT_LEN_V5, 0x0608, 4, 0x32, true));
    let c2 = parse_fadt(&fadt_v1_machine());
    let cent_ok = match (&c1, &c2) {
        (Ok(a), Ok(b)) => a.century == 0x32 && a.century_available && !b.century_available,
        _ => false,
    };
    v.push((
        "F0447-century两态",
        cent_ok,
        String::from("0x32 入账 / 0 缺位记账"),
    ));

    // 10. TMR_VAL_EXT 两态账。
    let e1 = parse_fadt(&build_fadt(5, FADT_LEN_V5, 0x0608, 4, 0x32, true));
    let e2 = parse_fadt(&build_fadt(5, FADT_LEN_V5, 0x0608, 4, 0x32, false));
    let ext_ok = match (&e1, &e2) {
        (Ok(a), Ok(b)) => a.tmr_val_ext && !b.tmr_val_ext,
        _ => false,
    };
    v.push((
        "F0447-电平触发位",
        ext_ok,
        String::from("FLAGS 位 8 两态入账"),
    ));

    // 11. 频率/回绕口径复算。
    let w = pm_tmr_wrap_sec();
    v.push((
        "F0447-频率口径",
        PM_TMR_FREQ_HZ == 3_579_545 && w >= 1199 && w < 1201,
        format!("freq=3.579545MHz wrap={}s", w),
    ));

    // 12. A2 消费字段预备（7 字段齐账）。
    match parse_fadt_full(&fadt_v5_machine()) {
        Ok((_, a)) => v.push((
            "F0447-A2预备",
            a2_ready_ok(&a) && a.pm_tmr_freq_hz == 3_579_545 && a.pm_tmr_len == 4,
            format!("blk=0x{:04X} wrap={}s", a.pm_tmr_blk, a.pm_tmr_wrap_sec),
        )),
        Err(e) => v.push(("F0447-A2预备", false, format!("err=0x{:x}", e))),
    }

    // 13. 状态机倒退拒。
    let mut m = FadtMachine::new();
    let a1 = m.advance(FadtStage::HeaderVetted).is_ok();
    let a2s = m.advance(FadtStage::HeaderVetted).is_err();
    let a3 = m.advance(FadtStage::GatePassed).is_ok();
    let a4 = m.advance(FadtStage::A2Prepared).is_err();
    v.push((
        "F0447-状态机倒退",
        a1 && a2s && a3 && a4,
        String::from("禁重走 + 禁越级"),
    ));

    // 14. 注入账 10/10。
    let inj = inject_rounds(10);
    v.push((
        "F0447-注入10/10",
        match &inj {
            Ok(l) => l.all_caught(),
            Err(_) => false,
        },
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));

    // 15. 双跑一致。
    v.push((
        "F0447-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));

    // 16. 段位守卫。
    v.push((
        "F0447-段位守卫",
        segment_guard(),
        String::from("0x75180 窗口"),
    ));

    v
}

pub fn run_unx0447_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0447 FADT 解析（计时字段账）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 单元测试（注入必抓 / 双跑一致 / 边界） ───────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_v5_machine_full_ledger() {
        let (l, a) = match parse_fadt_full(&fadt_v5_machine()) {
            Ok(v) => v,
            Err(_) => {
                assert!(false, "v5 机器夹具必须可解析");
                return;
            }
        };
        assert_eq!(l.revision, 5);
        assert_eq!(l.family, FadtFamily::V5);
        assert_eq!(l.pm_tmr_blk, 0x0608);
        assert_eq!(l.century, 0x32);
        assert!(a2_ready_ok(&a));
    }

    #[test]
    fn test_v1_and_v4_gate() {
        match parse_fadt(&fadt_v1_machine()) {
            Ok(l1) => {
                assert_eq!(l1.family, FadtFamily::V1);
                assert_eq!(l1.declared_len, 128);
            }
            Err(_) => assert!(false, "v1 机器夹具必须可解析"),
        }
        match parse_fadt(&fadt_v4_machine()) {
            Ok(l4) => assert_eq!(l4.family, FadtFamily::V4),
            Err(_) => assert!(false, "v4 机器夹具必须可解析"),
        }
    }

    #[test]
    fn test_len_gate_injections() {
        assert!(parse_fadt(&build_fadt(5, FADT_LEN_V4, 0x0608, 4, 0x32, true)).is_err());
        assert!(parse_fadt(&build_fadt(1, 120, 0x0608, 4, 0, false)).is_err());
        assert!(parse_fadt(&build_fadt(4, FADT_LEN_V5, 0x0608, 4, 0x32, true)).is_err());
        assert!(parse_fadt(&build_fadt(1, 200, 0x0608, 4, 0, false)).is_err());
    }

    #[test]
    fn test_sig_and_sum_injection() {
        let mut sig = fadt_v5_machine();
        put8(&mut sig, 0, b'X');
        assert_eq!(parse_fadt(&sig), Err(FadtCode::SigBad.val()));
        let mut bad = fadt_v5_machine();
        let n = bad.len();
        if let Some(slot) = bad.get_mut(n - 1) {
            *slot = slot.wrapping_add(1);
        }
        assert_eq!(parse_fadt(&bad), Err(FadtCode::SumBad.val()));
    }

    #[test]
    fn test_tmr_rules() {
        let mut tl = fadt_v5_machine();
        put8(&mut tl, OFF_PM_TMR_LEN, 2);
        reseal_fadt(&mut tl);
        assert_eq!(parse_fadt(&tl), Err(FadtCode::TmrLenBad.val()));
        let no_tmr = build_fadt(5, FADT_LEN_V5, 0, 4, 0x32, true);
        assert_eq!(parse_fadt(&no_tmr), Err(FadtCode::TmrMissing.val()));
    }

    #[test]
    fn test_freq_and_wrap() {
        assert_eq!(PM_TMR_FREQ_HZ, 3_579_545);
        let w = pm_tmr_wrap_sec();
        assert!(w >= 1199 && w < 1200);
    }

    #[test]
    fn test_machine_regress() {
        let mut m = FadtMachine::new();
        assert!(m.advance(FadtStage::HeaderVetted).is_ok());
        assert!(m.advance(FadtStage::HeaderVetted).is_err());
        assert!(m.advance(FadtStage::GatePassed).is_ok());
        assert!(m.advance(FadtStage::A2Prepared).is_err());
    }

    #[test]
    fn test_double_run_repro() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn test_inject_all_caught() {
        let led = match inject_rounds(10) {
            Ok(v) => v,
            Err(_) => {
                assert!(false, "注入轮不得报错");
                return;
            }
        };
        assert!(led.all_caught());
        assert_eq!(led.tries, [10, 10, 10, 10]);
        assert_eq!(led.caught, [10, 10, 10, 10]);
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in FadtCode::ALL.iter() {
            assert_ne!(detail(c.val()), "未知诊断码：不在 0x75180 段闭集内");
        }
        assert_eq!(detail(0x7999_9999), "未知诊断码：不在 0x75180 段闭集内");
    }
}
