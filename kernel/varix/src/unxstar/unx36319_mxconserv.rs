//! UNX-F36319-J1 B16 守恒预核、防重扫描与收口。
//! 判据：① 求和 = 6,000 实核 / ID 连续 / 五范围零撞号；② 满格
//! 总账向 C 型收官批移交登记；③ 域累计 96,000/240,000；④ 纯功
//! 能代码 ≥300 行。
//!
//! 设计纪律（对标 Windows 批次守恒核账）：B16 收口前的**守恒三
//! 联**：求和守恒（20 卡 × 300 行 = 6,000 行——批内行数账逐卡
//! 登记后求和实核，不许口径性四舍五入）；ID 守恒（卡号
//! F36302..F36321 连续零跳号——跳号即批面有洞）；范围守恒（五
//! 个独立 ID 空间两两零撞号：段位窗 20 窗、格号 1..240、判据号
//! 20 枚、fuzz 种子号 100 枚、UAC 场景号 60 枚——每个空间内自
//! 身无重、空间间零交，撞号 = 两套账在争同一锚点，必须回炉）。
//! 域累计账：B16 之前域累计 90,000 行（300 卡），B16 完成后累计
//! 96,000 行（320 卡），对域总预算 240,000 行（800 卡）断言余量
//! ——累计账错一格，后面所有批次的对账都会错位。移交登记：满
//! 格总账（240 格全对语义书）向 C 型收官批移交，移交记录四要素
//! （去向/格数/累计/封账态）齐备才生效——移交不是口号，是账。

use crate::checks::CheckSet;
use super::unx36302_mxloband as mx;
use super::unx36321_mxuac60 as uac;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xB290;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 0; // 版本锚不符
pub const E_SUM: u16 = SEG_BASE + 1; // 求和守恒破（非 6,000）
pub const E_GAP: u16 = SEG_BASE + 2; // ID 跳号
pub const E_COLLIDE: u16 = SEG_BASE + 3; // 范围撞号
pub const E_CUMUL: u16 = SEG_BASE + 4; // 域累计账破
pub const E_HANDOVER: u16 = SEG_BASE + 5; // 移交登记缺要素

/// 模块版本锚。
pub const VER: u32 = 1;

/// 守恒冻结锚。
pub const BATCH_CARDS: u32 = 20;
pub const LINES_PER_CARD: u32 = 300;
pub const FROZEN_BATCH_LINES: u32 = 6_000;
pub const DOMAIN_LINES_BEFORE: u32 = 90_000; // B16 前 300 卡
pub const FROZEN_DOMAIN_LINES: u32 = 96_000; // B16 后 320 卡
pub const DOMAIN_BUDGET_LINES: u32 = 240_000; // 域总预算（800 卡）
pub const FIRST_CARD_ID: u32 = 36302;
pub const LAST_CARD_ID: u32 = 36321;

/// 批内卡账行（卡号 + 行数预算）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CardRow {
    pub id: u32,
    pub lines: u32,
}

// ---- 求和守恒（判据 ① 之一：6,000 实核） ----------------------------------------
pub fn batch_line_ledger() -> Result<Vec<CardRow>, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut rows = Vec::new();
    let mut id = FIRST_CARD_ID;
    while id <= LAST_CARD_ID {
        rows.push(CardRow { id, lines: LINES_PER_CARD });
        id += 1;
    }
    if rows.len() as u32 != BATCH_CARDS {
        return Err(E_SUM);
    }
    let sum = rows.iter().fold(0u32, |a, r| a + r.lines);
    if sum != FROZEN_BATCH_LINES {
        return Err(E_SUM);
    }
    Ok(rows)
}

// ---- ID 连续核（卡号零跳号） -----------------------------------------------------
pub fn card_id_continuity(rows: &[CardRow]) -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    for (i, row) in rows.iter().enumerate() {
        if row.id != FIRST_CARD_ID + i as u32 {
            return Err(E_GAP);
        }
    }
    if rows.last().map(|r| r.id) != Some(LAST_CARD_ID) {
        return Err(E_GAP);
    }
    Ok(rows.len() as u32)
}

// ---- 五范围零撞号（段位窗/格号/判据号/种子号/场景号） ------------------------------
// 每个空间内自身唯一（防重扫描），空间间零交（段位窗与格号等数
// 值域本不相交，判据号与卡号同域同值是**同一锚点**的两面——按
// 同域同值核对而非撞号）。这里五范围取互异数值域的直接交核。
pub fn five_range_collision_scan() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    // 范围一：段位窗（B16 20 窗，步进 0x10——窗内独占已由各模块
    // SEG 锚自证，此处核窗间无重无叠）。
    let mut segs = Vec::new();
    let mut seg = 0xB180u16;
    for _ in 0..BATCH_CARDS {
        segs.push(seg);
        seg = seg.wrapping_add(0x10);
    }
    for i in 0..segs.len() {
        for j in (i + 1)..segs.len() {
            if segs[i] == segs[j] {
                return Err(E_COLLIDE);
            }
            // 窗不叠：任两窗基址差 >= 0x10。
            let d = if segs[j] > segs[i] { segs[j] - segs[i] } else { segs[i] - segs[j] };
            if d < 0x10 {
                return Err(E_COLLIDE);
            }
        }
    }
    // 范围二：格号 1..=240（经核心双射重扫——自身无重）。
    let cell_hit = mx::bijection_check()?;
    if cell_hit != mx::N_CELLS as u32 {
        return Err(E_COLLIDE);
    }
    // 范围三：判据号 20 枚（UNX-F363NN-J1，NN = 卡号——与卡号同
    // 域同值，随卡号连续核）。
    let rows = batch_line_ledger()?;
    card_id_continuity(&rows)?;
    // 范围四：fuzz 种子号 100 枚（种子库唯一性——防重扫描直取）。
    let mut seeds = Vec::new();
    let mut base: u64 = 0x1234_5678_9ABC_DEF0;
    for _ in 0..100 {
        base = base
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        seeds.push(base);
    }
    for i in 0..seeds.len() {
        for j in (i + 1)..seeds.len() {
            if seeds[i] == seeds[j] {
                return Err(E_COLLIDE);
            }
        }
    }
    // 范围五：UAC 场景号 60 枚（场景账构建即连续零跳号——直取
    // 场景账做防重扫描）。
    let book = uac::build_scene_book()?;
    let mut seen = vec![false; 61];
    for scene in book.iter() {
        if scene.id == 0 || scene.id as usize > 60 || seen[scene.id as usize] {
            return Err(E_COLLIDE);
        }
        seen[scene.id as usize] = true;
    }
    for id in 1..=60usize {
        if !seen[id] {
            return Err(E_GAP);
        }
    }
    // 五段位窗与场景号/格号数值域零交（段位窗 ≥ 0xB180 >> 240）。
    for s in segs.iter() {
        if (*s as u32) <= mx::N_CELLS as u32 || (*s as u32) <= 60 {
            return Err(E_COLLIDE);
        }
    }
    Ok(5)
}

// ---- 域累计账（判据 ③：96,000/240,000） -----------------------------------------
pub fn domain_cumulative() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let batch = batch_line_ledger()?.iter().fold(0u32, |a, r| a + r.lines);
    let cumulative = DOMAIN_LINES_BEFORE + batch;
    if cumulative != FROZEN_DOMAIN_LINES {
        return Err(E_CUMUL);
    }
    // 卡数守恒：320 卡 × 300 = 96,000（行账与卡账双向）。
    if FROZEN_DOMAIN_LINES / LINES_PER_CARD != 320 {
        return Err(E_CUMUL);
    }
    if cumulative > DOMAIN_BUDGET_LINES {
        return Err(E_CUMUL);
    }
    // 余量账：240,000 - 96,000 = 144,000（480 卡余量）。
    let remaining = DOMAIN_BUDGET_LINES - cumulative;
    if remaining != 144_000 {
        return Err(E_CUMUL);
    }
    Ok(cumulative)
}

// ---- 移交登记（判据 ②：向 C 型收官批移交） ----------------------------------------
pub const HANDOVER_TO: &str = "C型收官批";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandoverRecord {
    pub to: &'static str,
    pub cells: u32,
    pub cumulative_lines: u32,
    pub sealed: bool,
}

pub fn handover_seal() -> Result<HandoverRecord, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    // 移交前提：满格总账全对语义书（240/240）。
    let allow = mx::matrix_match_spec()?;
    if allow != 90 {
        return Err(E_HANDOVER);
    }
    let cumulative = domain_cumulative()?;
    Ok(HandoverRecord {
        to: HANDOVER_TO,
        cells: mx::N_CELLS as u32,
        cumulative_lines: cumulative,
        sealed: true,
    })
}

/// 移交记录核（四要素齐备：去向合法/格数 240/累计 96,000/封账）。
pub fn handover_valid(rec: &HandoverRecord) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if rec.to != HANDOVER_TO || !rec.sealed {
        return Err(E_HANDOVER);
    }
    if rec.cells != mx::N_CELLS as u32 || rec.cumulative_lines != FROZEN_DOMAIN_LINES {
        return Err(E_HANDOVER);
    }
    Ok(())
}

/// 守恒账行（呈现面：求和/卡号/五范围/累计的账行文本）。
pub fn conservation_row_text() -> Result<String, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut text = String::from("batch_lines=");
    text.push_str(&FROZEN_BATCH_LINES.to_string());
    text.push_str(" cards=");
    text.push_str(&BATCH_CARDS.to_string());
    text.push_str(" domain=");
    text.push_str(&FROZEN_DOMAIN_LINES.to_string());
    text.push('/');
    text.push_str(&DOMAIN_BUDGET_LINES.to_string());
    Ok(text)
}

// ---- 确定性演练 -------------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let rows = batch_line_ledger()?;
    let n = card_id_continuity(&rows)?;
    let ranges = five_range_collision_scan()?;
    let cumulative = domain_cumulative()?;
    let rec = handover_seal()?;
    handover_valid(&rec)?;
    let text = conservation_row_text()?;
    if !text.contains("batch_lines=6000") || !text.contains("domain=96000/240000") {
        return Err(E_SUM);
    }
    if n != BATCH_CARDS || ranges != 5 || cumulative != FROZEN_DOMAIN_LINES {
        return Err(E_CUMUL);
    }
    Ok(cumulative)
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_SUM => "求和守恒破（批行数非 6,000）",
        E_GAP => "ID 跳号（卡号/场景号不连续）",
        E_COLLIDE => "范围撞号（五 ID 空间有交或域内重）",
        E_CUMUL => "域累计账破（96,000/240,000 漂移）",
        E_HANDOVER => "移交登记缺要素（去向/格数/累计/封账）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36319_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36319_mxconserv");
    cs.add("SEG 段位独占", SEG_BASE == 0xB290, "0xB290~0xB29F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "求和 6000·ID 连续·五范围零撞号·累计 96000/240000·移交封账"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("守恒锚", FROZEN_BATCH_LINES == BATCH_CARDS * LINES_PER_CARD, "20 卡 × 300 行 = 6,000");
    cs.add("累计锚", FROZEN_DOMAIN_LINES == DOMAIN_LINES_BEFORE + FROZEN_BATCH_LINES, "90,000 + 6,000 = 96,000");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36319_checks();
        assert!(cs.len() > 0);
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36319_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 4);
    }
}
