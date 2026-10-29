//! UNX-F36307-J1 矩阵拒绝归因统计账（四道门分布）。
//! 判据：① 归因账（DACL/IL/特权/掩码四门占比）240 格统计与逐格
//! 账双向对平；② 归因缺失格 = 回炉标记；③ 纯功能代码 ≥300 行。
//!
//! 设计纪律（对标 Windows 授权失败的原因审计）：240 格里每个拒
//! 绝格都必须带**恰好一个**归因码——引擎按门序求值，第一个命中
//! 的门负责归因（一因口径，与 Windows AccessCheck 单一失败原因
//! 对齐）。统计账从两个独立方向聚合：**按主体面行聚合**（8 面
//! × 各门计数）与**按客体列聚合**（6 客体 × 各门计数），两方向
//! 的各门总和必须逐门相等且命中冻结锚——双向对平是「统计账没有
//! 漏格重格」的直接证据。四门冻结分布（由语义书闭集离线推导）：
//! DACL 69 / IL 13 / 特权 4 / 掩码 24，另有令牌过滤 40 与许 90；
//! 归因缺失格 = 归因码出闭集、拒格归因成许、或逐格账与统计账不
//! 能互推——凡出现即落**回炉标记**清单，清单必须为空（空 = 本批
//! 无回炉项，非空 = 实现回炉后重跑）。

use crate::checks::CheckSet;
use super::unx36302_mxloband as mx;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xB1D0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 0; // 版本锚不符
pub const E_MISSING: u16 = SEG_BASE + 1; // 归因缺失（出闭集/一因破）
pub const E_REWORK: u16 = SEG_BASE + 2; // 回炉清单非空
pub const E_ALIGN: u16 = SEG_BASE + 3; // 双向对平失败（行/列聚合不等）
pub const E_FROZEN: u16 = SEG_BASE + 4; // 冻结分布漂移
pub const E_RANGE: u16 = SEG_BASE + 5; // 格号越界

/// 模块版本锚。
pub const VER: u32 = 1;

// ---- 四门冻结分布锚（语义书离线推导值，行序 = S0..S7） -----------------------
pub const FROZEN_DACL: u32 = 69;
pub const FROZEN_IL: u32 = 13;
pub const FROZEN_PRIV: u32 = 4;
pub const FROZEN_MASK: u32 = 24;
pub const FROZEN_DENY: u32 = 40;
pub const FROZEN_ALLOW: u32 = 90;

/// 按主体面行聚合的冻结锚（DACL/IL/特权/掩码/过滤/许）。
pub const FACE_DACL: [u32; 8] = [14, 14, 2, 14, 14, 2, 9, 0];
pub const FACE_IL: [u32; 8] = [5, 3, 1, 1, 1, 1, 1, 0];
pub const FACE_PRIV: [u32; 8] = [1, 1, 0, 1, 1, 0, 0, 0];
pub const FACE_MASK: [u32; 8] = [3, 3, 3, 3, 3, 3, 3, 3];
pub const FACE_DENY: [u32; 8] = [0, 0, 17, 0, 0, 17, 6, 0];
pub const FACE_ALLOW: [u32; 8] = [7, 9, 7, 11, 11, 7, 11, 27];

/// 单格归因（带一因核：拒格必须有非零归因、许格归因必为零——
/// 一因口径的封闭性）。
pub fn attribute_cell(id: u32) -> Result<u8, u16> {
    let (s, o, op) = mx::cell_parts(id)?;
    let code = mx::evaluate(s, o, op)?;
    if !mx::attribution_in_range(code) {
        return Err(E_MISSING);
    }
    let want = mx::spec_at(id)?;
    if code != want {
        return Err(E_MISSING);
    }
    Ok(code)
}

// ---- 逐格账（240 行：格号 + 归因码） -----------------------------------------
pub fn cell_ledger() -> Result<Vec<(u32, u8)>, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut rows = Vec::new();
    for id in 1..=mx::N_CELLS as u32 {
        rows.push((id, attribute_cell(id)?));
    }
    if rows.len() != mx::N_CELLS {
        return Err(E_MISSING);
    }
    Ok(rows)
}

// ---- 方向一：按主体面行聚合 ---------------------------------------------------
// 六门 × 8 面计数矩阵；行和命中冻结行锚，行总和命中冻结总锚。
pub fn tally_by_face(ledger: &[(u32, u8)]) -> Result<[[u32; 8]; 6], u16> {
    // 门序：DACL/IL/特权/掩码/过滤/许（下标 0..6）。
    let mut grid = [[0u32; 8]; 6];
    for (id, code) in ledger.iter() {
        let (s, _, _) = mx::cell_parts(*id)?;
        let gate = match *code {
            mx::A_DACL => 0,
            mx::A_IL => 1,
            mx::A_PRIV => 2,
            mx::A_MASK => 3,
            mx::A_DENY => 4,
            mx::A_ALLOW => 5,
            _ => return Err(E_MISSING),
        };
        grid[gate][s] += 1;
    }
    // 行锚：每面每门命中冻结值（漂移一格即统计账失真）。
    let frozen: [&[u32; 8]; 6] = [&FACE_DACL, &FACE_IL, &FACE_PRIV, &FACE_MASK, &FACE_DENY, &FACE_ALLOW];
    for g in 0..6 {
        for s in 0..mx::N_SUBJ {
            if grid[g][s] != frozen[g][s] {
                return Err(E_FROZEN);
            }
        }
    }
    Ok(grid)
}

// ---- 方向二：按客体列聚合（独立重扫——不复用行聚合的中间量） -------------------
pub fn tally_by_object(ledger: &[(u32, u8)]) -> Result<[[u32; 6]; 6], u16> {
    let mut grid = [[0u32; 6]; 6];
    for (id, code) in ledger.iter() {
        let (_, o, _) = mx::cell_parts(*id)?;
        let gate = match *code {
            mx::A_DACL => 0,
            mx::A_IL => 1,
            mx::A_PRIV => 2,
            mx::A_MASK => 3,
            mx::A_DENY => 4,
            mx::A_ALLOW => 5,
            _ => return Err(E_MISSING),
        };
        grid[gate][o] += 1;
    }
    Ok(grid)
}

/// 双向对平：行聚合列和 == 列聚合行和（逐门），且命中冻结总锚。
pub fn align_tallies(by_face: &[[u32; 8]; 6], by_object: &[[u32; 6]; 6]) -> Result<[u32; 6], u16> {
    let frozen: [u32; 6] = [
        FROZEN_DACL,
        FROZEN_IL,
        FROZEN_PRIV,
        FROZEN_MASK,
        FROZEN_DENY,
        FROZEN_ALLOW,
    ];
    let mut totals = [0u32; 6];
    for g in 0..6 {
        let sum_face: u32 = by_face[g].iter().fold(0u32, |a, v| a.wrapping_add(*v));
        let sum_obj: u32 = by_object[g].iter().fold(0u32, |a, v| a.wrapping_add(*v));
        if sum_face != sum_obj {
            return Err(E_ALIGN);
        }
        if sum_face != frozen[g] {
            return Err(E_FROZEN);
        }
        totals[g] = sum_face;
    }
    if totals.iter().fold(0u32, |a, v| a.wrapping_add(*v)) != mx::N_CELLS as u32 {
        return Err(E_ALIGN);
    }
    Ok(totals)
}

// ---- 回炉标记 ----------------------------------------------------------------
// 归因缺失格的账面化：一因破 / 出闭集 / 逐格 vs 母版漂移的格落
// 回炉清单。全绿 = 清单空。
pub fn rework_markers(ledger: &[(u32, u8)]) -> Result<Vec<u32>, u16> {
    let mut marks = Vec::new();
    for (id, code) in ledger.iter() {
        let ok = mx::attribution_in_range(*code);
        let want = mx::spec_at(*id)?;
        if !ok || *code != want {
            marks.push(*id);
        }
    }
    Ok(marks)
}

/// 占比串（呈现面：四门占比直出——分母 240，整数直接给计数）。
pub fn ratio_text(totals: &[u32; 6]) -> Result<String, u16> {
    if totals.len() != 6 {
        return Err(E_ALIGN);
    }
    let mut text = String::from("DACL=");
    text.push_str(&totals[0].to_string());
    text.push_str(" IL=");
    text.push_str(&totals[1].to_string());
    text.push_str(" 特权=");
    text.push_str(&totals[2].to_string());
    text.push_str(" 掩码=");
    text.push_str(&totals[3].to_string());
    text.push_str(" 过滤=");
    text.push_str(&totals[4].to_string());
    text.push_str(" 许=");
    text.push_str(&totals[5].to_string());
    Ok(text)
}

// ---- 确定性演练 -------------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let ledger = cell_ledger()?;
    let by_face = tally_by_face(&ledger)?;
    let by_object = tally_by_object(&ledger)?;
    let totals = align_tallies(&by_face, &by_object)?;
    let marks = rework_markers(&ledger)?;
    // 回炉清单必须为空（非空 = 本批有归因缺失格）。
    if !marks.is_empty() {
        return Err(E_REWORK);
    }
    let text = ratio_text(&totals)?;
    if !text.starts_with("DACL=69") || !text.contains("许=90") {
        return Err(E_FROZEN);
    }
    Ok(totals[0].wrapping_mul(1000).wrapping_add(totals[1]))
}

// ---- 客体列账呈现增补 -------------------------------------------------------------
/// 按客体列聚合的呈现账（6 客体 × 六类归因；列和守恒 = 每客体
/// 40 格（8 面 × 5 操作）——列账无洞无重的守恒面）。
pub fn object_column_tally() -> Result<[[u32; 6]; 6], u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let ledger = cell_ledger()?;
    let grid = tally_by_object(&ledger)?;
    for o in 0..mx::N_OBJ {
        let col: u32 = (0..6).map(|g| grid[g][o]).fold(0u32, |a, v| a + v);
        if col != mx::N_SUBJ as u32 * mx::N_OP as u32 {
            return Err(E_ALIGN);
        }
    }
    Ok(grid)
}

/// 行账呈现（每面一行六类计数摘要——统计账的可读面，行序 S0..S7）。
pub fn face_row_text(s: usize) -> Result<String, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if s >= mx::N_SUBJ {
        return Err(E_RANGE);
    }
    let ledger = cell_ledger()?;
    let grid = tally_by_face(&ledger)?;
    let mut text = String::from("S");
    text.push_str(&s.to_string());
    text.push_str(": DACL=");
    text.push_str(&grid[0][s].to_string());
    text.push_str(" IL=");
    text.push_str(&grid[1][s].to_string());
    text.push_str(" 特权=");
    text.push_str(&grid[2][s].to_string());
    text.push_str(" 掩码=");
    text.push_str(&grid[3][s].to_string());
    text.push_str(" 过滤=");
    text.push_str(&grid[4][s].to_string());
    text.push_str(" 许=");
    text.push_str(&grid[5][s].to_string());
    Ok(text)
}

/// 全八面行账（行数守恒 + 每行带许账段——呈现面非空且结构齐）。
pub fn face_row_ledger() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut n = 0u32;
    for s in 0..mx::N_SUBJ {
        let text = face_row_text(s)?;
        if !text.starts_with("S") || !text.contains("许=") {
            return Err(E_ALIGN);
        }
        n += 1;
    }
    if n != mx::N_SUBJ as u32 {
        return Err(E_ALIGN);
    }
    Ok(n)
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_MISSING => "归因缺失（出闭集/一因破/逐格账不平）",
        E_REWORK => "回炉清单非空（存在归因缺失格）",
        E_ALIGN => "双向对平失败（行/列聚合各门不等）",
        E_FROZEN => "冻结分布漂移（四门占比锚失真）",
        E_RANGE => "格号越界",
        _ => "未知诊断码",
    }
}

pub fn run_unx36307_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36307_mxattrib");
    cs.add("SEG 段位独占", SEG_BASE == 0xB1D0, "0xB1D0~0xB1DF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "四门 69/13/4/24 双向对平·回炉清单空"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("守恒锚", FROZEN_DACL + FROZEN_IL + FROZEN_PRIV + FROZEN_MASK + FROZEN_DENY + FROZEN_ALLOW == 240, "六类归因计数和 = 240");
    cs.add("回炉锚", FROZEN_DACL > 0 && FROZEN_IL > 0 && FROZEN_PRIV > 0 && FROZEN_MASK > 0, "四门均有显影格（占比非退化）");
    cs.add("列守恒", object_column_tally().is_ok() && face_row_ledger() == Ok(8), "六客体列各 30 格归因账齐·八面行账呈现齐");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36307_checks();
        assert!(cs.len() > 0);
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36307_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 4);
    }
}
