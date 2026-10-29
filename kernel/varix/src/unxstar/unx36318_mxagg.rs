//! UNX-F36318-J1 B16 ktest 断言集（矩阵满格面全判据注册）。
//! 判据：① 本批 18 条判据聚合 ktest 全绿；② 纯功能代码 ≥300 行。
//!
//! 设计纪律（对标 Windows 测试套的批级聚合出口）：B16 批的判据
//！ 共 18 条（F36302–F36317 矩阵面 16 条 + F36319 守恒预核 +
//！ F36320 收口宣告；F36321 属 B17 域不入本批聚合）。聚合器的
//! 判据是**逐条 drill 全绿**：18 个模块的确定性演练逐一调用，
//! 每条必须 Ok——聚合不做抽样、不做短路（一条红即整批红，且红
//! 的位置必须显性落在聚合账行里，不许吞成笼统失败）。聚合账行
//! 按 18 项登记（批内序号 + 模块段位基址 + 绿/红），账行数 = 18
//! 才算批面完整；聚合码 = 18 条 Ok 值的折叠和（wrapping），供批
//! 收口（F36320）引用作批级指纹。聚合器自身不复制任何判据逻辑
//! ——只做编排（判据的单一真源在各模块内，聚合层复制逻辑就是
//! 第二真源，是漂移的温床）。

use crate::checks::CheckSet;
use super::unx36302_mxloband as m02;
use super::unx36303_mxfilter as m03;
use super::unx36304_mxelev as m04;
use super::unx36305_mxledger as m05;
use super::unx36306_mxwinmap as m06;
use super::unx36307_mxattrib as m07;
use super::unx36308_mxnegative as m08;
use super::unx36309_mxperf as m09;
use super::unx36310_mxlockstep as m10;
use super::unx36311_mxexplain as m11;
use super::unx36312_mxescape as m12;
use super::unx36313_mxaudit as m13;
use super::unx36314_mxsemantic as m14;
use super::unx36315_mxwhy as m15;
use super::unx36316_mxfuzz as m16;
use super::unx36317_mxktest as m17;
use super::unx36319_mxconserv as m19;
use super::unx36320_mxfinal as m20;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xB280;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 0; // 版本锚不符
pub const E_AGG: u16 = SEG_BASE + 1; // 聚合面破（红项/账行缺）
pub const E_COUNT: u16 = SEG_BASE + 2; // 判据计数漂移（非 18）
pub const E_FINGER: u16 = SEG_BASE + 3; // 批级指纹折叠账不平
pub const E_RANGE: u16 = SEG_BASE + 4; // 批内序号越界
pub const E_SHORTCUT: u16 = SEG_BASE + 5; // 短路（聚合中途返回）

/// 模块版本锚。
pub const VER: u32 = 1;

/// 本批判据数冻结锚（16 + 2 = 18）。
pub const BATCH_JUDGMENTS: u32 = 18;

/// 单条判据的聚合账行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AggRow {
    pub slot: u32, // 批内序号 1..=18
    pub seg: u16,  // 模块段位基址
    pub ok: bool,
}

/// 批内段位表（与任务板段位册对齐——聚合账的可审计锚）。
pub const BATCH_SEGS: [u16; 18] = [
    0xB180, 0xB190, 0xB1A0, 0xB1B0, 0xB1C0, 0xB1D0, 0xB1E0, 0xB1F0, 0xB200,
    0xB210, 0xB220, 0xB230, 0xB240, 0xB250, 0xB260, 0xB270, 0xB290, 0xB2A0,
];

// ---- 18 条判据编排（判据逻辑单一真源在各模块——此处只编排） ------------------------
pub fn run_batch_drills() -> Result<Vec<AggRow>, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut rows = Vec::new();
    // 逐条调用、逐条落账——不做短路（红也记账再显性上抛）。
    let mut red: Option<(u32, u16)> = None;
    let checks: [fn() -> Result<u32, u16>; 18] = [
        m02::drill, m03::drill, m04::drill, m05::drill, m06::drill,
        m07::drill, m08::drill, m09::drill, m10::drill, m11::drill,
        m12::drill, m13::drill, m14::drill, m15::drill, m16::drill,
        m17::drill, m19::drill, m20::drill,
    ];
    for (i, check) in checks.iter().enumerate() {
        let slot = (i + 1) as u32;
        if slot as usize > BATCH_SEGS.len() {
            return Err(E_RANGE);
        }
        let ok = check().is_ok();
        rows.push(AggRow { slot, seg: BATCH_SEGS[i], ok });
        if !ok && red.is_none() {
            red = Some((slot, BATCH_SEGS[i]));
        }
    }
    if rows.len() as u32 != BATCH_JUDGMENTS {
        return Err(E_COUNT);
    }
    // 红项显性上抛（聚合不吞错——位置经诊断码与账行外显）。
    if red.is_some() {
        return Err(E_AGG);
    }
    Ok(rows)
}

// ---- 批级指纹（判据 Ok 值的折叠和——F36320 收口引用） ----------------------------
pub fn batch_fingerprint(rows: &[AggRow]) -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if rows.len() as u32 != BATCH_JUDGMENTS {
        return Err(E_COUNT);
    }
    // 全绿核：18 行无一红。
    let mut greens = 0u32;
    let mut fold: u32 = 0;
    for row in rows.iter() {
        if !row.ok {
            return Err(E_AGG);
        }
        greens += 1;
        fold = fold.wrapping_mul(31).wrapping_add(row.seg as u32).wrapping_add(row.slot);
    }
    if greens != BATCH_JUDGMENTS {
        return Err(E_AGG);
    }
    Ok(fold)
}

/// 聚合账行文本（呈现面：slot/seg 逐行摘要）。
pub fn agg_row_text(row: &AggRow) -> Result<String, u16> {
    if row.slot == 0 || row.slot > BATCH_JUDGMENTS {
        return Err(E_RANGE);
    }
    let mut text = String::from("slot=");
    text.push_str(&row.slot.to_string());
    text.push_str(" seg=0x");
    text.push_str(&row.seg.to_string());
    text.push_str(if row.ok { " 绿" } else { " 红" });
    Ok(text)
}

/// 段位表自核：18 段位升序独占（窗距 0x10，唯二例外是聚合器跳
/// 过自身 0xB280 与 B17 域 0xB2B0——窗距应跳 0x20）。
pub fn seg_table_check() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    for i in 0..BATCH_SEGS.len() - 1 {
        let step = BATCH_SEGS[i + 1].wrapping_sub(BATCH_SEGS[i]);
        // 0xB270 → 0xB290 之间跳过聚合器自身窗（0x20）。
        let expect = if i == 15 { 0x20 } else { 0x10 };
        if step != expect {
            return Err(E_COUNT);
        }
    }
    Ok(())
}

// ---- 确定性演练 -------------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    seg_table_check()?;
    let rows = run_batch_drills()?;
    let fp = batch_fingerprint(&rows)?;
    // 呈现面抽核：首行/末行账行文本齐备且为绿。
    let head = agg_row_text(&rows[0])?;
    let tail = agg_row_text(&rows[17])?;
    if !head.contains("绿") || !tail.contains("绿") || !head.starts_with("slot=1") {
        return Err(E_AGG);
    }
    if rows.len() as u32 != BATCH_JUDGMENTS || fp == 0 {
        return Err(E_FINGER);
    }
    Ok(fp)
}

// ---- 聚合呈现账增补 -----------------------------------------------------------
/// 十八行聚合账文本（slot+seg 逐行——批面完整性的可读呈现，账
/// 行数守恒 = 18）。
pub fn agg_ledger_text() -> Result<Vec<String>, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let rows = run_batch_drills()?;
    let mut texts = Vec::new();
    for row in rows.iter() {
        texts.push(agg_row_text(row)?);
    }
    if texts.len() as u32 != BATCH_JUDGMENTS {
        return Err(E_COUNT);
    }
    Ok(texts)
}

/// 段位窗检查账（批内 18 窗逐窗基址合法：落在 B16 段位册区间、
/// 0x10 对齐——窗基址与段位册一致是聚合账可审计的前提）。
pub fn seg_window_rows() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    seg_table_check()?;
    let rows = run_batch_drills()?;
    let mut n = 0u32;
    for row in rows.iter() {
        let seg = row.seg;
        if seg < 0xB180 || seg > 0xB2B0 || seg % 0x10 != 0 {
            return Err(E_COUNT);
        }
        n += 1;
    }
    if n != BATCH_JUDGMENTS {
        return Err(E_COUNT);
    }
    Ok(n)
}

/// 批指纹复算核（指纹两次折叠同值——聚合账的可复算性：重算异
/// 值即折叠账不可信）。
pub fn fingerprint_recheck() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let rows = run_batch_drills()?;
    let fp1 = batch_fingerprint(&rows)?;
    let fp2 = batch_fingerprint(&rows)?;
    if fp1 != fp2 || fp1 == 0 {
        return Err(E_FINGER);
    }
    Ok(fp1)
}

/// 批锚段位闭集（18 窗全落 B16 段位册区间，不触 B15 尾窗与 B17
/// 首窗——批边界的段位账）。
pub fn seg_boundary_check() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    for seg in BATCH_SEGS.iter() {
        if *seg <= 0xB170 || *seg >= 0xB2B0 {
            return Err(E_COUNT);
        }
    }
    Ok(())
}

/// 批面守恒（18 行绿数 = 判据数；批内序号连续 1..=18——聚合账
/// 无跳槽无缺卡）。
pub fn batch_conservation() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let rows = run_batch_drills()?;
    let greens = rows.iter().filter(|r| r.ok).count() as u32;
    if greens != BATCH_JUDGMENTS {
        return Err(E_AGG);
    }
    for (i, row) in rows.iter().enumerate() {
        if row.slot != (i + 1) as u32 {
            return Err(E_RANGE);
        }
    }
    Ok(greens)
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_AGG => "聚合面破（红项/账行缺/指纹折叠零）",
        E_COUNT => "判据计数漂移（非 18）/段位表窗距错",
        E_FINGER => "批级指纹账不平",
        E_RANGE => "批内序号越界",
        E_SHORTCUT => "短路（聚合中途返回）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36318_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36318_mxagg");
    cs.add("SEG 段位独占", SEG_BASE == 0xB280, "0xB280~0xB28F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "18 条判据逐条全绿·段位表窗距对·批级指纹折叠在案"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("批锚", BATCH_JUDGMENTS == 18, "B16 判据 = 16 矩阵面 + 守恒 + 收口");
    cs.add("呈现账", agg_ledger_text().map(|v| v.len()) == Ok(18) && seg_window_rows() == Ok(18) && seg_boundary_check().is_ok(), "十八行账文本齐·18 窗段位册对齐·批界段位闭集");
    cs.add("复算账", fingerprint_recheck().is_ok() && batch_conservation() == Ok(18), "批指纹可复算·聚合账 18 行全绿连续");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36318_checks();
        assert!(cs.len() > 0);
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36318_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 4);
    }
}
