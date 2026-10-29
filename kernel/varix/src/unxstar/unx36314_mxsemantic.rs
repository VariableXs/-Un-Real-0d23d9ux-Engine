//! UNX-F36314-J1 矩阵文档核账（矩阵语义书与格定义一致）。
//! 判据：① 语义书 240 格定义与实现逐格对照零漂移；② 漂移格回
//! 写标记；③ 纯功能代码 ≥300 行。
//!
//! 设计纪律（对标 Windows 文档与实现的一致性审计）：语义书是矩
//! 阵的**文档侧工件**——八张主体面定义行（每行 30 格定义值）+
//! 面语义注记（面名/规则标签）。文档核账的判据是**两个独立工件
//! 的逐格对照**：书侧定义行（本模块的常量转录——按书排版誊抄，
//! 不从引擎生成）与实现侧语义书（unx36302 冻结 SPEC）逐格比对，
//! 任何一格不等即「漂移格」，落**回写标记**（格号 + 书值 + 实现
//! 值的漂移账行——回写就是漂移账行本身，修书或修实现后重跑必须
//！ 清空）。文档核账同时核**书的排版完整性**：每行恰 30 格、八
//! 行覆盖 8 面 × 6 客体 × 5 操作全空间、行内格序与格号双射一致
//! （书排版错位也算漂移——文档错和实现错一样致命）。规则标签
//! 闭集（许/DACL/IL/特权/掩码/过滤）六态与归因码一一对应，标签
//! 出闭集 = 书排版破。

use crate::checks::CheckSet;
use super::unx36302_mxloband as mx;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xB240;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 0; // 版本锚不符
pub const E_LAYOUT: u16 = SEG_BASE + 1; // 书排版破（行长/覆盖/格序错）
pub const E_DRIFT: u16 = SEG_BASE + 2; // 语义漂移（书 vs 实现）
pub const E_TAG: u16 = SEG_BASE + 3; // 规则标签出闭集
pub const E_WRITEBACK: u16 = SEG_BASE + 4; // 回写标记未清（漂移账未平）
pub const E_RANGE: u16 = SEG_BASE + 5; // 面号越界

/// 模块版本锚。
pub const VER: u32 = 1;

// ---- 书侧定义行（文档工件：按语义书排版逐行誊抄，不从引擎生成） -----------------
pub const BOOK_S0: [u8; 30] = [0, 2, 0, 1, 1, 0, 2, 4, 1, 1, 0, 1, 2, 1, 1, 0, 0, 1, 1, 1, 2, 1, 1, 1, 1, 0, 2, 4, 4, 3];
pub const BOOK_S1: [u8; 30] = [0, 2, 0, 1, 1, 0, 0, 4, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 1, 1, 2, 1, 1, 1, 1, 0, 2, 4, 4, 3];
pub const BOOK_S2: [u8; 30] = [0, 5, 0, 5, 5, 0, 5, 4, 5, 5, 0, 5, 0, 5, 5, 0, 5, 1, 5, 5, 2, 5, 1, 5, 5, 0, 5, 4, 4, 5];
pub const BOOK_S3: [u8; 30] = [0, 0, 0, 1, 1, 0, 0, 4, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 1, 1, 2, 1, 1, 1, 1, 0, 0, 4, 4, 3];
pub const BOOK_S4: [u8; 30] = [0, 0, 0, 1, 1, 0, 0, 4, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 1, 1, 2, 1, 1, 1, 1, 0, 0, 4, 4, 3];
pub const BOOK_S5: [u8; 30] = [0, 5, 0, 5, 5, 0, 5, 4, 5, 5, 0, 5, 0, 5, 5, 0, 5, 1, 5, 5, 2, 5, 1, 5, 5, 0, 5, 4, 4, 5];
pub const BOOK_S6: [u8; 30] = [0, 0, 0, 1, 5, 0, 0, 4, 1, 5, 0, 1, 0, 1, 5, 0, 0, 1, 1, 5, 2, 1, 1, 1, 5, 0, 0, 4, 4, 5];
pub const BOOK_S7: [u8; 30] = [0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 0];

/// 书侧行册（行序 = S0..S7，与主体面表对齐）。
pub const BOOK_ROWS: [&[u8; 30]; 8] = [
    &BOOK_S0, &BOOK_S1, &BOOK_S2, &BOOK_S3, &BOOK_S4, &BOOK_S5, &BOOK_S6, &BOOK_S7,
];

/// 面语义注记（书侧的行标题——规则标签的呈现面）。
pub fn book_row_title(s: usize) -> Result<&'static str, u16> {
    Ok(match s {
        0 => "S0 匿名不可信（格 1–30）",
        1 => "S1 基础低权（格 31–60）",
        2 => "S2 AppContainer 沙箱（格 61–90）",
        3 => "S3 标准用户（格 91–120）",
        4 => "S4 服务低IL请求者（格 121–150）",
        5 => "S5 沙箱消费预设（格 151–180）",
        6 => "S6 UAC过滤令牌（格 181–210）",
        7 => "S7 提权/系统（格 211–240）",
        _ => return Err(E_RANGE),
    })
}

/// 规则标签闭集（六态 ↔ 归因码一一对应）。
pub fn rule_tag(code: u8) -> Result<&'static str, u16> {
    Ok(match code {
        mx::A_ALLOW => "许",
        mx::A_DACL => "DACL",
        mx::A_IL => "IL",
        mx::A_PRIV => "特权",
        mx::A_MASK => "掩码",
        mx::A_DENY => "过滤",
        _ => return Err(E_TAG),
    })
}

// ---- 书排版完整性核（判据的前置：书没排好谈不上对照） -----------------------------
// 每行恰 30 格、行内格序与格号双射一致（行[o*5+op] 对应格
// cell_id(s,o,op)）、八行覆盖 240 格全空间。
pub fn book_layout_check() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut covered = 0u32;
    for s in 0..mx::N_SUBJ {
        let row = BOOK_ROWS[s];
        if row.len() != 30 {
            return Err(E_LAYOUT);
        }
        for o in 0..mx::N_OBJ {
            for op in 0..mx::N_OP {
                let cell = mx::cell_id(s, o, op)?;
                let want = row[o * mx::N_OP + op];
                if mx::spec_at(cell)? != want {
                    // 排版错位 vs 实现漂移都在此显影——账行级再分。
                    return Err(E_LAYOUT);
                }
                rule_tag(want)?;
                covered += 1;
            }
        }
    }
    if covered != mx::N_CELLS as u32 {
        return Err(E_LAYOUT);
    }
    Ok(covered)
}

// ---- 漂移扫描与回写标记（判据 ①②） ---------------------------------------------
// 漂移账行：格号 + 书值 + 实现值 + 标签对照。全绿 = 账行空。
pub struct DriftMark {
    pub cell: u32,
    pub book: u8,
    pub engine: u8,
}

/// 漂移扫描（书侧 vs 实现侧逐格对照——以书行排版序走）。
pub fn drift_scan() -> Result<Vec<DriftMark>, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut marks = Vec::new();
    for s in 0..mx::N_SUBJ {
        let row = BOOK_ROWS[s];
        for o in 0..mx::N_OBJ {
            for op in 0..mx::N_OP {
                let cell = mx::cell_id(s, o, op)?;
                let book_v = row[o * mx::N_OP + op];
                let engine_v = mx::spec_at(cell)?;
                if book_v != engine_v {
                    marks.push(DriftMark { cell, book: book_v, engine: engine_v });
                }
            }
        }
    }
    Ok(marks)
}

/// 回写标记账（判据 ②：漂移格必须全部落标记；无漂移 = 账空）。
pub fn writeback_ledger() -> Result<(u32, Vec<String>), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    // 书排版先过（排版破 = 核账无效）。
    book_layout_check()?;
    let marks = drift_scan()?;
    let mut rows = Vec::new();
    for m in marks.iter() {
        let mut text = String::from("漂移格");
        text.push_str(&m.cell.to_string());
        text.push_str(" 书=");
        text.push_str(&m.book.to_string());
        text.push_str(" 实现=");
        text.push_str(&m.engine.to_string());
        text.push_str(" 书标签=");
        text.push_str(rule_tag(m.book)?);
        text.push_str(" 实现标签=");
        text.push_str(rule_tag(m.engine)?);
        rows.push(text);
    }
    Ok((marks.len() as u32, rows))
}

/// 面账行（呈现面：行标题 + 行内许格数——书行的可读摘要）。
pub fn book_row_summary(s: usize) -> Result<String, u16> {
    if s >= mx::N_SUBJ {
        return Err(E_RANGE);
    }
    let row = BOOK_ROWS[s];
    let allow = row.iter().filter(|v| **v == mx::A_ALLOW).count();
    let mut text = String::from(book_row_title(s)?);
    text.push_str(" 许=");
    text.push_str(&allow.to_string());
    Ok(text)
}

// ---- 确定性演练 -------------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let covered = book_layout_check()?;
    let (n_marks, rows) = writeback_ledger()?;
    // 回写账必须空（有漂移 = 有账行 = 判据破）。
    if n_marks != 0 || !rows.is_empty() {
        return Err(E_WRITEBACK);
    }
    // 呈现面抽核：八行摘要齐备、首行带格段标注。
    let mut summaries = 0u32;
    for s in 0..mx::N_SUBJ {
        let text = book_row_summary(s)?;
        if text.is_empty() || !text.starts_with("S") {
            return Err(E_LAYOUT);
        }
        summaries += 1;
    }
    if summaries != 8 || covered != 240 {
        return Err(E_LAYOUT);
    }
    Ok(covered.wrapping_mul(1000).wrapping_add(summaries))
}

// ---- 书行标签账增补 -----------------------------------------------------------
/// 每书行六标签计数（与归因账按面冻结分布同值——书侧与账侧的
/// 分布级对照：书排版错位或账侧漂移都会打破同值）。
pub fn book_row_tag_tally() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    // 冻结行分布（DACL/IL/特权/掩码/过滤/许——行序 S0..S7）。
    let frozen: [[u32; 6]; 8] = [
        [14, 5, 1, 3, 0, 7],
        [14, 3, 1, 3, 0, 9],
        [2, 1, 0, 3, 17, 7],
        [14, 1, 1, 3, 0, 11],
        [14, 1, 1, 3, 0, 11],
        [2, 1, 0, 3, 17, 7],
        [9, 1, 0, 3, 6, 11],
        [0, 0, 0, 3, 0, 27],
    ];
    let mut n = 0u32;
    for s in 0..mx::N_SUBJ {
        let mut tally = [0u32; 6];
        for v in BOOK_ROWS[s].iter() {
            let slot = match *v {
                mx::A_DACL => 0,
                mx::A_IL => 1,
                mx::A_PRIV => 2,
                mx::A_MASK => 3,
                mx::A_DENY => 4,
                mx::A_ALLOW => 5,
                _ => return Err(E_TAG),
            };
            tally[slot] += 1;
        }
        if tally != frozen[s] {
            return Err(E_DRIFT);
        }
        n += 1;
    }
    if n != mx::N_SUBJ as u32 {
        return Err(E_LAYOUT);
    }
    Ok(n)
}

/// 书行许格目录（每面许格计数汇总行——书的目录面，和 90 与语义
/// 书分布守恒）。
pub fn book_allow_catalog() -> Result<String, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut text = String::from("许格目录:");
    let mut n = 0u32;
    for s in 0..mx::N_SUBJ {
        for v in BOOK_ROWS[s].iter() {
            if *v == mx::A_ALLOW {
                n += 1;
            }
        }
    }
    text.push_str(&n.to_string());
    text.push_str(" 格");
    if n != 90 {
        return Err(E_DRIFT);
    }
    Ok(text)
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_LAYOUT => "书排版破（行长/覆盖/格序/标签出闭集）",
        E_DRIFT => "语义漂移（书 vs 实现）",
        E_TAG => "规则标签出闭集",
        E_WRITEBACK => "回写标记未清（存在漂移格）",
        E_RANGE => "面号越界",
        _ => "未知诊断码",
    }
}

pub fn run_unx36314_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36314_mxsemantic");
    cs.add("SEG 段位独占", SEG_BASE == 0xB240, "0xB240~0xB24F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "书 240 格零漂移·排版 8 行全格覆盖·回写账空"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("排版锚", book_layout_check() == Ok(240), "书排版 8×30 全格覆盖");
    cs.add("标签锚", rule_tag(mx::A_DENY) == Ok("过滤"), "六态标签闭集在位");
    cs.add("书行标签账", book_row_tag_tally() == Ok(8) && book_allow_catalog().is_ok(), "八行六标签分布与归因账同值·许格目录 90");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36314_checks();
        assert!(cs.len() > 0);
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36314_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 4);
    }

    #[test]
    fn test_drift_marker_on_doctored_book() {
        // 注入差值的书行必须产出漂移标记（回写能力自证）。
        let mut doctored: Vec<u8> = BOOK_S5.to_vec();
        doctored[1] = mx::A_ALLOW; // 与实现值 5 漂移
        let mut marks = 0u32;
        for o in 0..mx::N_OBJ {
            for op in 0..mx::N_OP {
                let cell = mx::cell_id(mx::S_LOWSANDBOX, o, op).unwrap_or(0);
                let book_v = doctored[o * mx::N_OP + op];
                let engine_v = mx::spec_at(cell).unwrap_or(255);
                if book_v != engine_v {
                    marks += 1;
                }
            }
        }
        assert_eq!(marks, 1);
    }
}
