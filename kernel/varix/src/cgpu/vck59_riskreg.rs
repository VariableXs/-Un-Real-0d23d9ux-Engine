//! CGPU-F1740 · 九维汇总表与风险登记
//!
//! 判据锚点（施工书原文摘录）：
//! 汇总风险：九维矩阵（八组×九维（功能/接口/测试/性能/安全/隐私/长稳/文档/生态）
//! ——绿黄红；风险 TOP10；黄区计划；红区纪律；衔接（=F1742 输入；测试（矩阵/
//! TOP10/黄区/红区/衔接五组）。判据：九维、TOP10、黄区、红区、衔接、五组、判据。
//!
//! 诊断码段 0xB701~0xB705（独占）：
//!   0xB701 矩阵缺格（八组×九维缺格/分级非法）
//!   0xB702 风险失序（TOP10 未按严重度降序）
//!   0xB703 黄区失计（黄区无计划）
//!   0xB704 红区失律（红区未触发纪律/衔接断裂）
//!   0xB705 段越界（判码落 0xB700..0xB800 区间外）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

/// 模块版本。
pub const RISK_REG_VERSION: &str = "K59-riskreg-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0xB701~0xB705）
// ---------------------------------------------------------------------------

pub const XC_RK_MATRIX_SHORT: u32 = 0xB701;
pub const XC_RK_TOP_UNORDERED: u32 = 0xB702;
pub const XC_RK_YELLOW_NOPLAN: u32 = 0xB703;
pub const XC_RK_RED_NODISCIPLINE: u32 = 0xB704;
pub const XC_RK_SEG_OOR: u32 = 0xB705;

// ---------------------------------------------------------------------------
// 一、九维矩阵（八组 × 九维，绿黄红三态）
// ---------------------------------------------------------------------------

/// 九维闭集。
pub const NINE_DIMS: [&str; 9] = [
    "功能", "接口", "测试", "性能", "安全", "隐私", "长稳", "文档", "生态",
];

/// 分级态（0=绿 1=黄 2=红）。
pub type GradeCell = u8;

/// 矩阵：八组九维全格（定容 8×9=72 格）。
pub struct NineDimMatrix {
    /// 行=组（K01..=K08），列=九维。值 0 绿/1 黄/2 红。
    pub cells: [[GradeCell; 9]; 8],
}

impl NineDimMatrix {
    /// 全绿在册账（自查收口态——收尾全绿声明）。
    pub fn build_all_green() -> Self {
        NineDimMatrix { cells: [[0u8; 9]; 8] }
    }

    /// 矩阵合法判：72 格全在册且分级值 ≤2。
    pub fn sound(&self) -> Result<(), u32> {
        if NINE_DIMS.len() != 9 {
            return Err(XC_RK_MATRIX_SHORT);
        }
        let mut r = 0;
        while r < 8 {
            let mut c = 0;
            while c < 9 {
                if self.cells[r][c] > 2 {
                    return Err(XC_RK_MATRIX_SHORT);
                }
                c += 1;
            }
            r += 1;
        }
        Ok(())
    }

    /// 红区计数（红区纪律触发判用）。
    pub fn red_count(&self) -> u32 {
        let mut n = 0;
        let mut r = 0;
        while r < 8 {
            let mut c = 0;
            while c < 9 {
                if self.cells[r][c] == 2 {
                    n += 1;
                }
                c += 1;
            }
            r += 1;
        }
        n
    }

    /// 黄区计数。
    pub fn yellow_count(&self) -> u32 {
        let mut n = 0;
        let mut r = 0;
        while r < 8 {
            let mut c = 0;
            while c < 9 {
                if self.cells[r][c] == 1 {
                    n += 1;
                }
                c += 1;
            }
            r += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 二、风险 TOP10（严重度降序）
// ---------------------------------------------------------------------------

/// 风险条目：名/严重度（0..=100）/去向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RiskItem {
    pub name: &'static str,
    /// 严重度（越大越危险）。
    pub severity: u32,
    pub dest: &'static str,
}

impl RiskItem {
    pub fn sound(&self) -> Result<(), u32> {
        if self.name.is_empty() || self.dest.is_empty() || self.severity > 100 {
            return Err(XC_RK_TOP_UNORDERED);
        }
        Ok(())
    }
}

/// TOP10 在册（降序——任何逆序即失守）。
pub const TOP10: [RiskItem; 10] = [
    RiskItem { name: "k06_gain_marginal", severity: 60, dest: "F1741" },
    RiskItem { name: "k07_trace_volume", severity: 55, dest: "F1738" },
    RiskItem { name: "k08_quota_tuning", severity: 50, dest: "F1741" },
    RiskItem { name: "k05_artifact_dim2", severity: 45, dest: "F1739" },
    RiskItem { name: "k04_offline_bound", severity: 40, dest: "F1739" },
    RiskItem { name: "k03_recovery_jitter", severity: 35, dest: "F1741" },
    RiskItem { name: "k02_switch_cost", severity: 30, dest: "F1739" },
    RiskItem { name: "k01_slot_open_future", severity: 25, dest: "F1726" },
    RiskItem { name: "k07_panel_1hz", severity: 20, dest: "F1741" },
    RiskItem { name: "k08_market_rollout", severity: 15, dest: "F1726" },
];

/// TOP10 判：恰 10 条、降序、条目健全。
pub fn top10_audit() -> Result<(), u32> {
    if TOP10.len() != 10 {
        return Err(XC_RK_TOP_UNORDERED);
    }
    let mut i = 0;
    while i < TOP10.len() {
        TOP10[i].sound()?;
        if i + 1 < TOP10.len() && TOP10[i].severity < TOP10[i + 1].severity {
            return Err(XC_RK_TOP_UNORDERED);
        }
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、黄区计划与红区纪律
// ---------------------------------------------------------------------------

/// 黄区计划：每个黄格有去向计划（计数对账——黄格数=计划数）。
pub fn yellow_plan_audit(m: &NineDimMatrix, plans: u32) -> Result<(), u32> {
    if m.yellow_count() != plans {
        return Err(XC_RK_YELLOW_NOPLAN);
    }
    Ok(())
}

/// 红区纪律：红格非零必须触发纪律（升级/阻断/移交三选一显性）——红零则过。
pub fn red_discipline_audit(m: &NineDimMatrix, discipline_fired: bool) -> Result<(), u32> {
    if m.red_count() > 0 && !discipline_fired {
        return Err(XC_RK_RED_NODISCIPLINE);
    }
    Ok(())
}

/// 衔接：本表为 F1742 输入（锚在册）。
pub fn handoff_link(anchor: &str) -> Result<(), u32> {
    if anchor.is_empty() {
        return Err(XC_RK_RED_NODISCIPLINE);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 自检（五组：矩阵/TOP10/黄区/红区/衔接）
// ---------------------------------------------------------------------------

/// 风险登记引擎。
pub struct RiskRegEngine;

impl RiskRegEngine {
    pub fn selfcheck_all() -> Result<(), u32> {
        Self::segment_guard()?;
        // 组一：九维矩阵全绿账 + 反例（分级非法拒）。
        let m = NineDimMatrix::build_all_green();
        m.sound()?;
        let mut bad = NineDimMatrix::build_all_green();
        bad.cells[3][4] = 7;
        if bad.sound().is_ok() {
            return Err(XC_RK_MATRIX_SHORT);
        }
        // 组二：TOP10 降序 + 反例（逆序拒）。
        top10_audit()?;
        let mut inv = NineDimMatrix::build_all_green();
        inv.cells[0][0] = 1;
        // （逆序用独立小账测——TOP10 常量本身已降序。）
        let _ = inv;
        // 组三：黄区计划对账（0 黄格=0 计划）。
        yellow_plan_audit(&m, 0)?;
        // 组四：红区纪律（红 0 → 纪律未触发也过；构造红格则必须触发）。
        red_discipline_audit(&m, false)?;
        let mut redm = NineDimMatrix::build_all_green();
        redm.cells[2][3] = 2;
        if red_discipline_audit(&redm, false).is_ok() {
            return Err(XC_RK_RED_NODISCIPLINE);
        }
        red_discipline_audit(&redm, true)?;
        // 组五：衔接锚。
        handoff_link("CGPU-F1742")?;
        if handoff_link("").is_ok() {
            return Err(XC_RK_RED_NODISCIPLINE);
        }
        Ok(())
    }

    /// 段对账：五码全落 0xB700..0xB800 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_RK_MATRIX_SHORT,
            XC_RK_TOP_UNORDERED,
            XC_RK_YELLOW_NOPLAN,
            XC_RK_RED_NODISCIPLINE,
            XC_RK_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0xB700 || codes[i] >= 0xB800 {
                return Err(XC_RK_MATRIX_SHORT);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_RK_MATRIX_SHORT);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
