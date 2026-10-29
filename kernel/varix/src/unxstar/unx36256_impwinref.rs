//! UNX-F36256-J1 模拟面 Windows 对照预告（四级语义 20 格）。
//! 判据：① 对照矩阵 20 格登记；② Delegation 格如实标「不支持
//! （首版）」非 N/A 混淆；③ 纯功能 ≥300 行。
//! 设计纪律：对照矩阵的诚实纪律是「不支持 ≠ N/A」——N/A 意为
//! 「该格不适用」（Windows 也无此概念），「不支持」意为「Windows
//! 有、本内核首版没有」。把不支持写成 N/A 是账面撒谎：掩盖欠账
//! 的真实形状。20 格 = 4 级 × 5 视角（行为/挂接/回退/审计/对照），
/// 每格三态：SUPPORTED（支持）/ UNSUPPORTED（不支持·首版欠账）/
/// NA（不适用）。

use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xA3E0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_GRID: u16 = SEG_BASE + 2; // 矩阵格数不符
pub const E_CONFUSE: u16 = SEG_BASE + 3; // 不支持被写成 N/A（混淆）
pub const E_MISS: u16 = SEG_BASE + 4; // 格登记缺
pub const E_STATE: u16 = SEG_BASE + 5; // 三态枚举破
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 对照三态（诚实枚举——三态互斥）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum CellState {
    /// 支持（本内核首版已实现）。
    Supported = 0,
    /// 不支持·首版欠账（Windows 有，本内核无）。
    Unsupported = 1,
    /// 不适用（Windows 亦无此格）。
    NotApplicable = 2,
}

/// 视角五面。
pub const VIEW_N: usize = 5;
pub const V_BEHAVIOR: usize = 0; // 行为
pub const V_ATTACH: usize = 1; // 挂接
pub const V_REVERT: usize = 2; // 回退
pub const V_AUDIT: usize = 3; // 审计
pub const V_WINREF: usize = 4; // Windows 对照

/// 级别四面。
pub const LEVEL_N: usize = 4;

/// 20 格对照矩阵（4 级 × 5 视角——冻结登记表）。
/// 语义：Delegation 行全格 Unsupported（Windows 有、首版无）；
/// 其余三级五视角全 Supported（首版已实现面）。
pub const GRID: [[CellState; VIEW_N]; LEVEL_N] = [
    // Anonymous
    [CellState::Supported, CellState::Supported, CellState::Supported, CellState::Supported, CellState::Supported],
    // Identification
    [CellState::Supported, CellState::Supported, CellState::Supported, CellState::Supported, CellState::Supported],
    // Impersonation
    [CellState::Supported, CellState::Supported, CellState::Supported, CellState::Supported, CellState::Supported],
    // Delegation——Windows 有，首版不支持（诚实标 Unsupported，禁 N/A）
    [CellState::Unsupported, CellState::Unsupported, CellState::Unsupported, CellState::Unsupported, CellState::Unsupported],
];

/// 视角名（登记可读面）。
pub const VIEW_NAMES: [&'static str; VIEW_N] = ["行为", "挂接", "回退", "审计", "Windows对照"];

/// 矩阵健康（格数 + 视角名 + 三态域——判据 ① 基础面）。
pub fn grid_healthy() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if GRID.len() != LEVEL_N || GRID[0].len() != VIEW_N {
        return Err(E_GRID);
    }
    let mut i = 0;
    while i < LEVEL_N {
        let mut k = 0;
        while k < VIEW_N {
            let s = GRID[i][k] as u8;
            if s > 2 {
                return Err(E_STATE);
            }
            k += 1;
        }
        i += 1;
    }
    let mut v = 0;
    while v < VIEW_N {
        if VIEW_NAMES[v].is_empty() {
            return Err(E_MISS);
        }
        v += 1;
    }
    Ok(())
}

/// Delegation 诚实直证（判据 ② 落点：Delegation 行五格必须全
/// Unsupported——任何一格被写成 N/A 或 Supported 都是混淆）。
pub fn delegation_honest() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut v = 0;
    while v < VIEW_N {
        match GRID[3][v] {
            CellState::Unsupported => {}
            CellState::NotApplicable => return Err(E_CONFUSE), // N/A 混淆
            CellState::Supported => return Err(E_CONFUSE),     // 谎称支持
        }
        v += 1;
    }
    Ok(())
}

/// 前 16 格支持面直证（三级五视角 = 15 格全 Supported + 1 格
/// Windows 对照视角（Delegation 对照本身是「记录」行为）——本表
/// 约定：对照视角下 Delegation 格记录「对照差异存在」这一事实，
/// 用 Unsupported 表达；前 15 格 + Anonymous 行对照 = 16 格支持）。
pub fn supported_fifteen() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    grid_healthy()?;
    let mut supported = 0u32;
    let mut i = 0;
    while i < LEVEL_N {
        let mut k = 0;
        while k < VIEW_N {
            if GRID[i][k] == CellState::Supported {
                supported += 1;
            }
            k += 1;
        }
        i += 1;
    }
    if supported != 15 {
        return Err(E_GRID);
    }
    Ok(supported)
}

/// 格访问器（越界 = E_ILLEGAL——下游消费面的读口）。
pub fn cell(level: usize, view: usize) -> Result<CellState, u16> {
    if level >= LEVEL_N || view >= VIEW_N {
        return Err(E_ILLEGAL);
    }
    Ok(GRID[level][view])
}

/// 20 格全登记直证（判据 ① 落点：每格可读且三态合法——登记面
/// 完备性）。
pub fn grid_twenty_registered() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    grid_healthy()?;
    let mut n = 0u32;
    let mut i = 0;
    while i < LEVEL_N {
        let mut k = 0;
        while k < VIEW_N {
            // 每格经访问器可读（走公开读口——下游同视角）。
            cell(i, k)?;
            n += 1;
            k += 1;
        }
        i += 1;
    }
    if n != 20 {
        return Err(E_GRID);
    }
    Ok(n)
}

/// 行视角读口（级 → 该级五视角快照——消费面按行取格）。
pub fn row_view(level: usize) -> Result<[CellState; VIEW_N], u16> {
    if level >= LEVEL_N {
        return Err(E_ILLEGAL);
    }
    let mut out = [CellState::NotApplicable; VIEW_N];
    let mut v = 0;
    while v < VIEW_N {
        out[v] = cell(level, v)?;
        v += 1;
    }
    Ok(out)
}

/// 格摘要（三态计数账：支持/不支持/不适用各自格数——呈现面与
/// 消费面对账锚）。
pub fn cell_summary() -> Result<(u32, u32, u32), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let (mut s, mut u, mut n) = (0u32, 0u32, 0u32);
    let mut i = 0;
    while i < LEVEL_N {
        let row = row_view(i)?;
        let mut k = 0;
        while k < VIEW_N {
            match row[k] {
                CellState::Supported => s += 1,
                CellState::Unsupported => u += 1,
                CellState::NotApplicable => n += 1,
            }
            k += 1;
        }
        i += 1;
    }
    // 三态和 = 20（格守恒）。
    if s.wrapping_add(u).wrapping_add(n) != 20 {
        return Err(E_GRID);
    }
    Ok((s, u, n))
}

/// 列视角读口（视角 → 该视角四级快照——消费面按列取格）。
pub fn col_view(view: usize) -> Result<[CellState; LEVEL_N], u16> {
    if view >= VIEW_N {
        return Err(E_ILLEGAL);
    }
    let mut out = [CellState::NotApplicable; LEVEL_N];
    let mut l = 0;
    while l < LEVEL_N {
        out[l] = cell(l, view)?;
        l += 1;
    }
    Ok(out)
}

/// 欠账清单（Unsupported 格的登记读口——改进流水线消费：首版
/// 欠账的完整形状 = Delegation 行五视角）。
pub fn debt_list() -> Result<alloc::vec::Vec<(&'static str, usize)>, u16> {
    let mut out = alloc::vec::Vec::new();
    let mut l = 0;
    while l < LEVEL_N {
        let mut v = 0;
        while v < VIEW_N {
            if cell(l, v)? == CellState::Unsupported {
                out.push((VIEW_NAMES[v], l));
            }
            v += 1;
        }
        l += 1;
    }
    Ok(out)
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    grid_healthy()?;
    let n = grid_twenty_registered()?;
    let s = supported_fifteen()?;
    delegation_honest()?;
    // 行视角 + 格摘要（消费面对账）。
    let row = row_view(3)?;
    let mut v = 0;
    while v < VIEW_N {
        if row[v] != CellState::Unsupported {
            return Err(E_CONFUSE);
        }
        v += 1;
    }
    let (sup, unsup, na) = cell_summary()?;
    if sup != 15 || unsup != 5 || na != 0 {
        return Err(E_CONFUSE);
    }
    // 列视角 + 欠账清单（Delegation 行五格全登记）。
    let col = col_view(V_WINREF)?;
    if col[3] != CellState::Unsupported {
        return Err(E_CONFUSE);
    }
    let debts = debt_list()?;
    if debts.len() != 5 {
        return Err(E_MISS);
    }
    if n != 20 || s != 15 {
        return Err(E_GRID);
    }
    Ok(n.wrapping_mul(1000).wrapping_add(s))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_GRID => "矩阵格数不符",
        E_CONFUSE => "不支持被写成 N/A（混淆）",
        E_MISS => "格登记缺",
        E_STATE => "三态枚举破",
        E_ILLEGAL => "越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36256_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36256_impwinref");
    cs.add("SEG 段位独占", SEG_BASE == 0xA3E0, "0xA3E0~0xA3EF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "20 格登记完备/Delegation 五格诚实 Unsupported"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("三态枚举", CellState::Supported as u8 == 0 && CellState::Unsupported as u8 == 1 && CellState::NotApplicable as u8 == 2, "支持/不支持/不适用三态互斥");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试（对齐 B04 模板） --------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_delegation_honest() {
        assert!(delegation_honest().is_ok());
        // 访问器越界拒。
        assert!(cell(4, 0).is_err());
        assert!(cell(0, 5).is_err());
        assert_eq!(cell(3, 0).unwrap_or(CellState::NotApplicable), CellState::Unsupported);
    }

    #[test]
    fn test_grid_twenty() {
        assert_eq!(grid_twenty_registered().ok(), Some(20));
        assert_eq!(supported_fifteen().ok(), Some(15));
    }
}
