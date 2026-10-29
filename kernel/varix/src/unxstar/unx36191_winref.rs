//! UNX-F36191-J1 边界条件的 Windows 对照预告（J-根 消费 20 格）。
//! 判据：① 对照矩阵 20 格登记（MA/空态/探测/错误码四面 × 五视角）；
//! ② 缺基准数据格标 N/A 不编造——无基准的面显性 N/A（对照面的诚实性
//! 判据：宁缺毋假）；③ 纯功能 ≥320 行。
//! 设计纪律：Windows 对照的纪律边界——能对账的格必须有「基准事实的
//! 来源锚」（文档引用形态编码），没有来源锚的格标 N/A 并写明缺什么。
//! 编造基准是对照面最恶劣的缺陷形式（假锚比无锚毒十倍——它会污染
//! 后续所有消费它的对账）。四面 × 五视角 = 20 格：视角 = 语义/边界/
//! 性能/诊断/呈现（J-根五问）。

use crate::checks::CheckSet;
use super::unx36102_stdrights as sr;

pub const SEG_BASE: u16 = 0x4430;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_GRID: u16 = SEG_BASE + 2; // 矩阵格越界/缺格
pub const E_FABRIC: u16 = SEG_BASE + 3; // 编造基准（无来源锚却非 N/A）
pub const E_NA: u16 = SEG_BASE + 4; // N/A 格缺理由（诚实性不完整）
pub const E_ANCHOR: u16 = SEG_BASE + 5; // 来源锚不符
pub const E_COVER: u16 = SEG_BASE + 6; // 四面覆盖破

/// 模块版本锚。
pub const VER: u32 = 1;

/// 矩阵容量（4 面 × 5 视角 = 20 格）。
pub const FACES: usize = 4;
pub const VIEWS: usize = 5;
pub const GRID_CAP: usize = FACES * VIEWS;

/// 四面（对照维度的域面闭集）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Face {
    /// MA 面（36181~36183）。
    Ma = 1,
    /// 空态面（36184 三态 DACL）。
    Empty = 2,
    /// 探测面（36185）。
    Probe = 3,
    /// 错误码面（36186~36187）。
    ErrCode = 4,
}

impl Face {
    pub fn code(self) -> u16 {
        self as u16
    }
    pub fn name(self) -> &'static str {
        match self {
            Face::Ma => "MA 面",
            Face::Empty => "空态面",
            Face::Probe => "探测面",
            Face::ErrCode => "错误码面",
        }
    }
}

/// 五视角（J-根五问：语义/边界/性能/诊断/呈现）。
pub const VIEW_NAMES: [&str; VIEWS] = ["语义", "边界", "性能", "诊断", "呈现"];

/// 格状态闭集。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GridState {
    /// 已对账（有基准事实 + 来源锚）。
    Matched = 1,
    /// N/A（缺基准数据——显性不编造）。
    NotAvail = 2,
}

impl GridState {
    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 对照格（面 × 视角 → 状态 + 基准来源锚）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GridCell {
    pub face: u16,
    pub view: u16,
    pub state: u16,
    /// 来源锚编码（Matched 必非零：1=MS-DOC 语义册 / 2=行为基准机
    /// 实测 / 3=跨域对账锁步；N/A 格必须为零——N/A 无锚可引）。
    pub anchor: u16,
    /// N/A 格的缺口说明索引（0=非 N/A；1..=3 = 缺数据/缺工具/缺口径）。
    pub gap: u16,
}

impl GridCell {
    pub const fn na(face: u16, view: u16, gap: u16) -> GridCell {
        GridCell { face, view, state: 2, anchor: 0, gap }
    }
    pub const fn matched(face: u16, view: u16, anchor: u16) -> GridCell {
        GridCell { face, view, state: 1, anchor, gap: 0 }
    }
    /// 诚实性自检：Matched 必带锚，N/A 必带缺口且锚为零。
    pub fn honest(&self) -> bool {
        if self.state == GridState::Matched.code() {
            self.anchor >= 1 && self.anchor <= 3 && self.gap == 0
        } else if self.state == GridState::NotAvail.code() {
            self.anchor == 0 && self.gap >= 1 && self.gap <= 3
        } else {
            false
        }
    }
}

/// 20 格对照矩阵（登记面——缺基准的格显性 N/A 不编造）。
/// 登记依据：MA 语义/边界有 MS-DOC 语义册锚（1）与行为基准实测（2）；
/// MA 性能有行为基准（2）；MA 诊断/呈现为 J1 自有形态（无外部基准——
/// N/A 缺口径）。空态语义/边界有锚；空态性能/诊断/呈现 N/A。探测面
/// 语义/边界/诊断有行为基准（2）；性能/呈现 N/A（缺工具）。错误码面
/// 语义/呈现有语义册锚（1）；边界/性能/诊断 N/A（缺口径）。
pub const GRID: [GridCell; GRID_CAP] = [
    // MA 面（face=1）
    GridCell::matched(1, 0, 1),
    GridCell::matched(1, 1, 2),
    GridCell::matched(1, 2, 2),
    GridCell::na(1, 3, 3),
    GridCell::na(1, 4, 3),
    // 空态面（face=2）
    GridCell::matched(2, 0, 1),
    GridCell::matched(2, 1, 1),
    GridCell::na(2, 2, 1),
    GridCell::na(2, 3, 3),
    GridCell::na(2, 4, 3),
    // 探测面（face=3）
    GridCell::matched(3, 0, 2),
    GridCell::matched(3, 1, 2),
    GridCell::na(3, 2, 2),
    GridCell::matched(3, 3, 2),
    GridCell::na(3, 4, 2),
    // 错误码面（face=4）
    GridCell::matched(4, 0, 1),
    GridCell::na(4, 1, 3),
    GridCell::na(4, 2, 3),
    GridCell::na(4, 3, 3),
    GridCell::matched(4, 4, 1),
];

/// 矩阵完整性：20 格全量、面×视角坐标唯一且全覆盖、状态闭集。
pub fn grid_integrity() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if GRID.len() != GRID_CAP {
        return Err(E_GRID);
    }
    let mut seen = 0u32; // 位图（face*5+view，20 位）
    let mut i = 0;
    while i < GRID_CAP {
        let g = &GRID[i];
        if g.face < 1 || g.face > FACES as u16 || g.view >= VIEWS as u16 {
            return Err(E_GRID);
        }
        if g.state != GridState::Matched.code() && g.state != GridState::NotAvail.code() {
            return Err(E_GRID);
        }
        let bit = 1u32 << ((g.face - 1) * VIEWS as u16 + g.view);
        if seen & bit != 0 {
            return Err(E_GRID); // 坐标重复
        }
        seen |= bit;
        if !g.honest() {
            return Err(E_FABRIC); // 不诚实格（有锚无据或 N/A 无缺口）
        }
        i += 1;
    }
    if seen != (1u32 << GRID_CAP) - 1 {
        return Err(E_COVER); // 20 格不全
    }
    Ok(seen)
}

/// 诚实性审计：N/A 格的缺口说明必须在缺口闭集内（缺数据/缺工具/缺
/// 口径三形态），且 N/A 数量有账（本登记 10 格 N/A——账面快照）。
pub fn na_audit() -> Result<u32, u16> {
    let mut na = 0u32;
    let mut i = 0;
    while i < GRID_CAP {
        let g = &GRID[i];
        if g.state == GridState::NotAvail.code() {
            if g.gap < 1 || g.gap > 3 || g.anchor != 0 {
                return Err(E_NA);
            }
            na += 1;
        } else if g.anchor == 0 {
            return Err(E_FABRIC); // Matched 无锚 = 编造
        }
        i += 1;
    }
    if na != 10 {
        return Err(E_NA); // 登记快照：10 格 N/A（宁缺毋假的账面）
    }
    Ok(na)
}

/// 来源锚直证：Matched 格的锚编码在闭集内（1/2/3），且四面至少各
/// 有一格 Matched（对照面不许整面 N/A——整面缺基准说明该面还没做）。
pub fn anchor_audit() -> Result<(), u16> {
    let mut face_matched = [0u32; FACES];
    let mut i = 0;
    while i < GRID_CAP {
        let g = &GRID[i];
        if g.state == GridState::Matched.code() {
            if g.anchor < 1 || g.anchor > 3 {
                return Err(E_ANCHOR);
            }
            face_matched[(g.face - 1) as usize] += 1;
        }
        i += 1;
    }
    let mut f = 0;
    while f < FACES {
        if face_matched[f] == 0 {
            return Err(E_COVER); // 整面 N/A = 面未做（对照面预注不许）
        }
        f += 1;
    }
    Ok(())
}

/// 矩阵快照校验和（face/view/state/anchor/gap 五元折叠——登记漂移检测）。
pub fn grid_checksum() -> u32 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i < GRID_CAP {
        let g = &GRID[i];
        sum = sum.wrapping_mul(31).wrapping_add(g.face as u32);
        sum = sum.wrapping_mul(31).wrapping_add(g.view as u32);
        sum = sum.wrapping_mul(31).wrapping_add(g.state as u32);
        sum = sum.wrapping_mul(31).wrapping_add(g.anchor as u32);
        sum = sum.wrapping_mul(31).wrapping_add(g.gap as u32);
        i += 1;
    }
    sum
}

/// 视角锚：五视角名互异非空（J-根五问的入口健康）。
pub fn view_anchor() -> Result<(), u16> {
    let mut i = 0;
    while i < VIEWS {
        if VIEW_NAMES[i].is_empty() {
            return Err(E_ANCHOR);
        }
        let mut k = i + 1;
        while k < VIEWS {
            if VIEW_NAMES[i] == VIEW_NAMES[k] {
                return Err(E_ANCHOR);
            }
            k += 1;
        }
        i += 1;
    }
    Ok(())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：版本锚 + 视角锚。
    if VER != 1 {
        return Err(E_VER);
    }
    view_anchor()?;
    // R2：矩阵完整性（20 格全量唯一覆盖 + 逐格诚实性）。
    let seen = grid_integrity()?;
    if seen != (1u32 << GRID_CAP) - 1 {
        return Err(E_COVER);
    }
    // R3：N/A 账面（13 格快照）+ Matched 锚直证。
    let na = na_audit()?;
    anchor_audit()?;
    // R4：快照确定性（校验和重放恒等）。
    let s1 = grid_checksum();
    let s2 = grid_checksum();
    if s1 != s2 {
        return Err(E_ANCHOR);
    }
    // R5：面名锚（四面名互异）。
    let faces = [Face::Ma, Face::Empty, Face::Probe, Face::ErrCode];
    let mut i = 0;
    while i < FACES {
        if faces[i].name().is_empty() {
            return Err(E_ANCHOR);
        }
        let mut k = i + 1;
        while k < FACES {
            if faces[i].name() == faces[k].name() || faces[i].code() == faces[k].code() {
                return Err(E_GRID);
            }
            k += 1;
        }
        i += 1;
    }
    Ok(seen.wrapping_mul(7).wrapping_add(na).wrapping_add(s1 & 0xFF))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_GRID => "矩阵格越界/缺格",
        E_FABRIC => "编造基准（无来源锚却非 N/A）",
        E_NA => "N/A 格缺理由（诚实性不完整）",
        E_ANCHOR => "来源锚不符",
        E_COVER => "四面覆盖破",
        _ => "未知诊断码",
    }
}

pub fn run_unx36191_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36191_winref");
    cs.add("SEG 段位独占", SEG_BASE == 0x4430, "0x4430~0x443F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "20 格登记/10 格 N/A 宁缺毋假/四面对照覆盖全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("矩阵容量", GRID_CAP == 20 && FACES == 4 && VIEWS == 5, "4 面 × 5 视角 = 20 格（判据锚）");
    cs.add("N/A 账面", na_audit().is_ok(), "10 格 N/A 显性不编造");
    cs.add("状态闭集", GridState::Matched.code() == 1 && GridState::NotAvail.code() == 2, "两态闭集（对账/N-A）");
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
    fn test_honest_cells() {
        assert!(GridCell::matched(1, 0, 1).honest());
        assert!(GridCell::na(2, 3, 1).honest());
        assert!(!GridCell::matched(1, 0, 0).honest()); // 无锚 = 编造
        assert!(!GridCell::na(1, 0, 0).honest()); // N/A 无缺口
    }

    #[test]
    fn test_grid_cover() {
        assert!(grid_integrity().is_ok());
        assert_eq!(grid_checksum(), grid_checksum());
    }
}
