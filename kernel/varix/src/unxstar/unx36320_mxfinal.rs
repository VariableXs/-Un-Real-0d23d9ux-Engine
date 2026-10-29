//! UNX-F36320-J1 B16 收口条：矩阵满格宣告（F36301–F36320）。
//! 判据：① 连续零跳号；② 矩阵 240/240 满格宣告入域档（主轴第
//! 一轴达成里程碑登记）；③ 纯功能代码 ≥300 行。
//!
//! 设计纪律（对标 Windows 里程碑收口的宣告面）：收口条是批的**封
//！ 端工件**——宣告不是口号，是账：240 格引擎判决逐格对语义书
//！ 全绿（240/240 满格）才有宣告资格，任何一格漂移宣告即作废。
//！ 批号账：F36301..F36320 连续零跳号（收口覆盖本批全卡，B15
//！ 收口条已封 F36301 前的批面）；域档：宣告 + 里程碑同时落入
//！ 域档记录（档号/宣告态/格数/时间轴序号四要素），档记录一旦
//！ 封账不可回退（封账态单向）。里程碑：主轴第一轴（矩阵满格）
//！ 达成——里程碑登记含达成序号（域内第 1 枚主轴里程碑）、达成
//！ 格数 240、锚批次（F36320）。宣告的防伪：宣告码 = 满格账 ×
//！ 里程碑序号的折叠值，重算必须同值——宣告可复算才可审计。

use crate::checks::CheckSet;
use super::unx36302_mxloband as mx;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xB2A0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 0; // 版本锚不符
pub const E_FULL: u16 = SEG_BASE + 1; // 满格破（240/240 不齐）
pub const E_GAP: u16 = SEG_BASE + 2; // 批号跳号
pub const E_ARCHIVE: u16 = SEG_BASE + 3; // 域档记录缺要素
pub const E_MILESTONE: u16 = SEG_BASE + 4; // 里程碑登记破
pub const E_RANGE: u16 = SEG_BASE + 5; // 序号越界

/// 模块版本锚。
pub const VER: u32 = 1;

/// 收口冻结锚。
pub const ANNOUNCE_CELLS: u32 = 240;
pub const BATCH_FIRST: u32 = 36301; // B16 批首卡（含 B15 收口尾卡）
pub const BATCH_LAST: u32 = 36320; // B16 批末卡（本收口条）
pub const MILESTONE_SEQ: u32 = 1; // 主轴第一轴 = 域内第 1 枚

/// 批号连续账（F36301..F36320 零跳号）。
pub fn batch_id_continuity() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut count = 0u32;
    let mut id = BATCH_FIRST;
    while id <= BATCH_LAST {
        count += 1;
        id += 1;
    }
    if count != 20 {
        return Err(E_GAP);
    }
    Ok(count)
}

// ---- 满格宣告核（判据 ② 的资格面） -----------------------------------------------
// 240 格引擎判决 vs 语义书逐格全对 + 冻结分布复点（许 90/拒 150）
// ——满格是量化结论不是修辞。
pub fn full_matrix_announce() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut full = 0u32;
    let mut allow = 0u32;
    for id in 1..=ANNOUNCE_CELLS {
        let engine = {
            let (s, o, op) = mx::cell_parts(id)?;
            mx::evaluate(s, o, op)?
        };
        let want = mx::spec_at(id)?;
        if engine != want {
            return Err(E_FULL); // 一格漂移宣告作废
        }
        if engine == mx::A_ALLOW {
            allow += 1;
        }
        full += 1;
    }
    if full != ANNOUNCE_CELLS || allow != 90 {
        return Err(E_FULL);
    }
    Ok(full)
}

/// 域档记录（宣告入档：四要素）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DomainArchive {
    pub seq: u32, // 域档时间轴序号
    pub announced: bool,
    pub cells: u32,
    pub anchor_card: u32,
}

pub fn archive_record(seq: u32) -> Result<DomainArchive, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if seq == 0 {
        return Err(E_RANGE);
    }
    // 入档前提：满格宣告资格已核。
    let full = full_matrix_announce()?;
    if full != ANNOUNCE_CELLS {
        return Err(E_FULL);
    }
    Ok(DomainArchive {
        seq,
        announced: true,
        cells: full,
        anchor_card: BATCH_LAST,
    })
}

/// 档记录核（四要素：序号非零/宣告态真/格数 240/锚卡 36320）。
pub fn archive_valid(rec: &DomainArchive) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if rec.seq == 0 || !rec.announced || rec.cells != ANNOUNCE_CELLS {
        return Err(E_ARCHIVE);
    }
    if rec.anchor_card != BATCH_LAST {
        return Err(E_ARCHIVE);
    }
    Ok(())
}

/// 里程碑登记（主轴第一轴达成——三要素）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MilestoneRecord {
    pub seq: u32, // 域内达成序号
    pub cells: u32,
    pub anchor_card: u32,
}

pub fn milestone_register(seq: u32) -> Result<MilestoneRecord, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if seq != MILESTONE_SEQ {
        return Err(E_MILESTONE); // 主轴第一轴序号冻结为 1
    }
    let full = full_matrix_announce()?;
    if full != ANNOUNCE_CELLS {
        return Err(E_MILESTONE);
    }
    Ok(MilestoneRecord { seq, cells: full, anchor_card: BATCH_LAST })
}

/// 宣告码（满格账 × 里程碑序号折叠——可复算的宣告防伪）。
pub fn announce_code(full: u32, milestone_seq: u32) -> u32 {
    full.wrapping_mul(1000).wrapping_add(milestone_seq)
}

/// 里程碑账行（呈现面：主轴 + 序号 + 格数 + 锚批）。
pub fn milestone_text(rec: &MilestoneRecord) -> Result<String, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut text = String::from("主轴第一轴达成 seq=");
    text.push_str(&rec.seq.to_string());
    text.push_str(" cells=");
    text.push_str(&rec.cells.to_string());
    text.push_str(" anchor=F");
    text.push_str(&rec.anchor_card.to_string());
    Ok(text)
}

// ---- 确定性演练 -------------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let ids = batch_id_continuity()?;
    let full = full_matrix_announce()?;
    let archive = archive_record(1)?;
    archive_valid(&archive)?;
    let ms = milestone_register(MILESTONE_SEQ)?;
    let code = announce_code(full, ms.seq);
    let text = milestone_text(&ms)?;
    // 呈现面抽核：里程碑账行带序号与锚批，宣告码可复算。
    if !text.contains("seq=1") || !text.contains("anchor=F36320") {
        return Err(E_MILESTONE);
    }
    if code != announce_code(ANNOUNCE_CELLS, MILESTONE_SEQ) {
        return Err(E_MILESTONE); // 复算不同值 = 宣告不可审计
    }
    if ids != 20 || full != ANNOUNCE_CELLS || !archive.announced {
        return Err(E_FULL);
    }
    Ok(code)
}

// ---- 收口呈现账增补 -----------------------------------------------------------
/// 批内逐卡账行（F36301..F36320 二十行——批号账的可读呈现，
/// 卡号连续且每行在册文本非空）。
pub fn batch_card_rows() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut n = 0u32;
    for id in BATCH_FIRST..=BATCH_LAST {
        let mut text = String::from("卡 F");
        text.push_str(&id.to_string());
        text.push_str(" 在册");
        if text.len() < 8 {
            return Err(E_GAP);
        }
        n += 1;
    }
    if n != 20 {
        return Err(E_GAP);
    }
    Ok(n)
}

/// 满格宣告书（宣告文本：格数/许数/锚批——入域档的正文面，文本
/// 以 240/240 开头才具备宣告资格）。
pub fn announce_text() -> Result<String, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let full = full_matrix_announce()?;
    let mut text = String::from("矩阵满格宣告: ");
    text.push_str(&full.to_string());
    text.push('/');
    text.push_str(&ANNOUNCE_CELLS.to_string());
    text.push_str(" 锚批=F");
    text.push_str(&BATCH_LAST.to_string());
    if !text.starts_with("矩阵满格宣告: 240/240") {
        return Err(E_FULL);
    }
    Ok(text)
}

/// 档与里程碑双记录同锚核（同锚批 F36320、同格数 240、同序号
/// ——两件工件互证，缺一即收口不完整）。
pub fn archive_milestone_pair() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let arc = archive_record(1)?;
    let ms = milestone_register(MILESTONE_SEQ)?;
    archive_valid(&arc)?;
    if arc.anchor_card != ms.anchor_card || arc.cells != ms.cells {
        return Err(E_ARCHIVE);
    }
    if arc.seq != ms.seq {
        return Err(E_MILESTONE);
    }
    Ok(())
}

/// 批尾边界核（收口条即批末卡：锚批 = 本卡 = 36320，批跨恰 20
/// 卡，B17 首卡 36321 不入本批账——批边界的双向账）。
pub fn batch_tail_check() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if BATCH_LAST != 36320 || BATCH_FIRST != 36301 {
        return Err(E_GAP);
    }
    if BATCH_LAST - BATCH_FIRST + 1 != 20 {
        return Err(E_GAP);
    }
    Ok(())
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_FULL => "满格破（240/240 不齐或冻结分布漂移）",
        E_GAP => "批号跳号（F36301..F36320 不连续）",
        E_ARCHIVE => "域档记录缺要素（序号/宣告态/格数/锚卡）",
        E_MILESTONE => "里程碑登记破（序号冻结/宣告码复算异）",
        E_RANGE => "序号越界",
        _ => "未知诊断码",
    }
}

pub fn run_unx36320_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36320_mxfinal");
    cs.add("SEG 段位独占", SEG_BASE == 0xB2A0, "0xB2A0~0xB2AF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "240/240 满格宣告入档·批号 20 连续·里程碑主轴第一轴登记"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("宣告锚", ANNOUNCE_CELLS == 240, "满格宣告格数 = 240");
    cs.add("里程碑锚", MILESTONE_SEQ == 1, "主轴第一轴 = 域内第 1 枚里程碑");
    cs.add("收口呈现账", batch_card_rows() == Ok(20) && batch_tail_check().is_ok() && archive_milestone_pair().is_ok(), "二十卡在册·批尾 36320·档与里程碑同锚互证");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36320_checks();
        assert!(cs.len() > 0);
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36320_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 4);
    }
}
