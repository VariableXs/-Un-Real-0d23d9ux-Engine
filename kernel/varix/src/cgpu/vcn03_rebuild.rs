//! CGPU-F2083 · 效果重建原则（CGPU-N 域 · 效果重建域 · N01 组 · 目标 320 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2083`
//!
//! **判据（锚点原文）**：三原则、分级声明、两组、判据。
//!
//! **职责定位（锚点原文）**：重建原则：原则（重建三原则（等价优先/性能
//! 保底/降级诚实——原则声明；等价（像素级等价 vs 感知等价分级——分级
//! 声明（分级验收；测试（原则/分级两组）。判据：三原则、分级声明、
//! 两组、判据。
//!
//! ## 一、重建三原则（判据一：字面量冻结 + 可机检）
//!
//! 三原则（[`PRINCIPLES`]）：等价优先（重建效果先求与原生等价）/性能
//! 保底（等价不得击穿性能底线——80 帧合同仍然成立）/降级诚实（给不了
//! 等价时明说给了什么）。原则不是口号：每条绑定检查键
//! （[`PRINCIPLE_KEYS`]）与裁决函数位（[`apply_principles`] 中逐条生效）。
//!
//! ## 二、等价分级声明（判据二：分级是验收口径）
//!
//! 三级等价（[`EquivalenceLevel`]）：像素级（逐像素与原生一致——最高
//! 验收）/感知级（人眼不可辨——SSIM 类感知阈值验收）/结构级（结构
//! 相同、细节降档——验收带降档声明）。分级声明（[`LEVEL_DECLARE`]）
//! 把每级的验收方式写死——「分级验收」的落法是每级有自己的验收口径，
//! 不许拿感知级冒充像素级。
//!
//! ## 三、原则应用引擎（判据三的函数面：原则逐条裁决）
//!
//! [`apply_principles`]：给定重建预算（可用的等价档位）与性能余量 →
//! 裁决实际可承诺的等价级 + 诚实声明（性能余量不足时按性能保底降档，
//! 降档必须带 [`HonestyLabel`]——降级诚实原则的函数面）。裁决确定性：
//! 同输入同结论（对拍前提）。
//!
//! ## 四、验收锚定
//!
//! 分级验收规则（[`accepts`]）：承诺级 ≤ 达成级才过——承诺像素级而
//! 实际感知级 = 不达标（宁降承诺不给虚账，诚实原则的反向应用）。
//!
//! **对接**：F2084（重建管线——本原则是其约束面）；F2086（质量基准
//! SSIM/deltaE——感知级阈值的计量来源）；O 域（降级诚实与 O 域
//! 「降质不降帧」呼应）。零 panic 面、零 IO、零墙钟、no_std。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、三原则（判据一）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const REBUILD_PRINCIPLE_VERSION: &str = "N01-rebuildprinciple-v1";

/// 重建三原则（字面量冻结）。
pub const PRINCIPLES: [&str; 3] = [
    "等价优先：重建效果先求与原生等价",
    "性能保底：等价不得击穿性能底线（80帧合同仍成立）",
    "降级诚实：给不了等价时明说给了什么",
];

/// 原则检查键（可机检 grep 键面）。
pub const PRINCIPLE_KEYS: [&str; 3] =
    ["n01.principle.equivalence-first", "n01.principle.performance-floor", "n01.principle.honest-degrade"];

// ---------------------------------------------------------------------------
// 二、等价分级（判据二）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x67 段独占：N01 重建原则面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 承诺级高于达成级（虚账）。
    Overpromise,
    /// 原则键面缺项。
    PrincipleMissing,
    /// 预算与余量输入非法（负余量等）。
    BadBudget,
}

impl XcCode {
    /// 人读诊断。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::Overpromise => "承诺级高于达成级：分级验收不过；建议降承诺或提升实现档位，不给虚账",
            XcCode::PrincipleMissing => "原则键面缺项：三原则三键须齐备，缺一即未就位",
            XcCode::BadBudget => "预算输入非法：性能余量与可用档位须非负且在闭集内",
        }
    }
}

/// 等价三级闭集（级序：像素级 > 感知级 > 结构级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EquivalenceLevel {
    /// 结构级：结构相同、细节降档（最低承诺档）。
    Structural,
    /// 感知级：人眼不可辨（SSIM 类阈值验收）。
    Perceptual,
    /// 像素级：逐像素与原生一致（最高档）。
    PixelExact,
}

impl EquivalenceLevel {
    /// 级序（0-2；越高承诺越强）。
    pub const fn rank(self) -> u8 {
        match self {
            EquivalenceLevel::Structural => 0,
            EquivalenceLevel::Perceptual => 1,
            EquivalenceLevel::PixelExact => 2,
        }
    }

    /// 级名。
    pub const fn zh(self) -> &'static str {
        match self {
            EquivalenceLevel::Structural => "结构级",
            EquivalenceLevel::Perceptual => "感知级",
            EquivalenceLevel::PixelExact => "像素级",
        }
    }

    /// 该级的验收方式（分级声明：每级有自己的验收口径）。
    pub const fn acceptance(self) -> &'static str {
        match self {
            EquivalenceLevel::Structural => "结构相同+细节降档声明（降档项逐条列明）",
            EquivalenceLevel::Perceptual => "SSIM/deltaE 感知阈值达标的计量验收（F2086 双指标）",
            EquivalenceLevel::PixelExact => "逐像素全同比对（位精确）",
        }
    }
}

/// 分级声明表（判据二：三级 × 验收口径逐行在册）。
pub const LEVEL_DECLARE: [(&'static str, &'static str); 3] = [
    ("像素级", "逐像素全同比对（位精确）"),
    ("感知级", "SSIM/deltaE 感知阈值达标的计量验收（F2086 双指标）"),
    ("结构级", "结构相同+细节降档声明（降档项逐条列明）"),
];

// ---------------------------------------------------------------------------
// 三、原则应用引擎（判据三的函数面）
// ---------------------------------------------------------------------------

/// 诚实标签（降级诚实原则的输出物：给了什么必须写明）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HonestyLabel {
    /// 无降档：按承诺级足额达成。
    AsPromised,
    /// 性能保底降档：因性能余量不足降档（带原因）。
    PerfFloorDegrade { reason: String },
    /// 能力不足降档：实现档位够不到承诺级（带原因）。
    CapabilityDegrade { reason: String },
}

impl HonestyLabel {
    /// 人读行（降级诚实：给不了等价时明说给了什么）。
    pub fn line(&self) -> String {
        match self {
            HonestyLabel::AsPromised => "足额达成：按承诺级交付".to_string(),
            HonestyLabel::PerfFloorDegrade { reason } => {
                alloc::format!("性能保底降档：{}", reason)
            }
            HonestyLabel::CapabilityDegrade { reason } => {
                alloc::format!("能力不足降档：{}", reason)
            }
        }
    }
}

/// 重建预算输入（性能余量 + 可用实现档位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RebuildBudget {
    /// 性能余量（占帧预算百分比；0-100）。
    pub perf_headroom_pct: u32,
    /// 可用实现档位（实现能承诺的最高等价级）。
    pub capable: EquivalenceLevel,
}

/// 原则应用裁决（三原则逐条生效的函数面；确定性：同输入同结论）。
///
/// 裁决序：①等价优先——先看能不能按承诺级给；②性能保底——余量
/// 不足时降到余量允许的最高档（性能底线 80 帧合同优先于等价）；③
/// 降级诚实——降档必带标签与原因。返回（承诺级, 诚实标签）。
pub fn apply_principles(
    promised: EquivalenceLevel,
    budget: &RebuildBudget,
) -> Result<(EquivalenceLevel, HonestyLabel), XcCode> {
    if budget.perf_headroom_pct > 100 {
        return Err(XcCode::BadBudget);
    }
    // ①②：性能保底阈值（余量 <15% 时不承诺像素级——像素级全同比对的
    // 比对面开销最大；<5% 时最高只承诺感知级）。阈值来源：本域规划
    // （性能底线 80 帧合同的余量保护带）；当前策略值，可调。
    let floor = if budget.perf_headroom_pct < 5 {
        EquivalenceLevel::Structural
    } else if budget.perf_headroom_pct < 15 {
        EquivalenceLevel::Perceptual
    } else {
        EquivalenceLevel::PixelExact
    };
    // 可用档位 = min(承诺级, 实现能力级, 性能底线级)。
    let granted = if budget.capable.rank() <= promised.rank() && budget.capable.rank() <= floor.rank()
    {
        budget.capable
    } else if promised.rank() <= floor.rank() {
        promised
    } else {
        floor
    };
    // ③：诚实标签（降档必带原因）。
    let label = if granted.rank() == promised.rank() {
        HonestyLabel::AsPromised
    } else if granted.rank() < budget.capable.rank() {
        HonestyLabel::PerfFloorDegrade {
            reason: alloc::format!(
                "性能余量 {}% 不足，按性能保底降到 {}（底线保护 80 帧合同）",
                budget.perf_headroom_pct,
                granted.zh()
            ),
        }
    } else {
        HonestyLabel::CapabilityDegrade {
            reason: alloc::format!(
                "实现档位仅到 {}，承诺 {} 超出能力",
                budget.capable.zh(),
                promised.zh()
            ),
        }
    };
    Ok((granted, label))
}

// ---------------------------------------------------------------------------
// 四、分级验收（判据四的函数面）
// ---------------------------------------------------------------------------

/// 分级验收（承诺级 ≤ 达成级才过——虚账显性拒）。
pub fn accepts(promised: EquivalenceLevel, achieved: EquivalenceLevel) -> Result<(), XcCode> {
    if promised.rank() > achieved.rank() {
        return Err(XcCode::Overpromise);
    }
    Ok(())
}

/// 原则键面核验（三原则三键齐备）。
pub fn principles_ready() -> Result<(), XcCode> {
    if PRINCIPLES.len() != 3 || PRINCIPLE_KEYS.len() != 3 {
        return Err(XcCode::PrincipleMissing);
    }
    let k0 = PRINCIPLE_KEYS[0];
    let k1 = PRINCIPLE_KEYS[1];
    let k2 = PRINCIPLE_KEYS[2];
    if k0 == k1 || k1 == k2 || k0 == k2 {
        return Err(XcCode::PrincipleMissing);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 五、重建裁决账（审计面：逐效果裁决留痕，验收可回溯）
// ---------------------------------------------------------------------------

/// 单效果裁决行（一次裁决的全部事实：承诺/达成/标签/依据）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RebuildVerdict {
    /// 效果名（重建对象；如「Mica 材质」「Aero 毛玻璃」）。
    pub effect: String,
    /// 承诺级（需求面给的等价承诺）。
    pub promised: EquivalenceLevel,
    /// 达成级（裁决后实际可承诺级）。
    pub granted: EquivalenceLevel,
    /// 诚实标签（降级诚实原则输出物）。
    pub label_kind: u8,
    /// 标签原因（人读；AsPromised 时为「足额达成」）。
    pub label_reason: String,
}

impl RebuildVerdict {
    /// 验收判定（分级验收的账面应用：承诺 ≤ 达成才算过）。
    pub fn accepted(&self) -> bool {
        accepts(self.promised, self.granted).is_ok()
    }
}

/// 重建裁决账（封顶滚动；同效果重裁决覆盖旧行——账上永远是最新结论）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RebuildLedger {
    rows: Vec<RebuildVerdict>,
    overwritten: u64,
}

/// 裁决账容量（封顶滚动配额；当前策略值，可调）。
pub const LEDGER_CAPACITY: usize = 128;

impl RebuildLedger {
    /// 记一次裁决（判据三审计面：裁决即留痕；同效果覆盖旧行并计数）。
    pub fn record(&mut self, v: RebuildVerdict) {
        // 同效果覆盖（最新结论语义；覆盖显性计数不静默）。
        let mut idx: Option<usize> = None;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.effect == v.effect {
                    idx = Some(i);
                    break;
                }
            }
            i += 1;
        }
        match idx {
            Some(i) => {
                self.rows.remove(i);
                self.overwritten = self.overwritten.saturating_add(1);
            }
            None => {}
        }
        self.rows.push(v);
        if self.rows.len() > LEDGER_CAPACITY {
            self.rows.remove(0);
            self.overwritten = self.overwritten.saturating_add(1);
        }
    }

    /// 按效果回查（裁决可回溯；无记录给 None 不臆造）。
    pub fn lookup(&self, effect: &str) -> Option<&RebuildVerdict> {
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.effect == effect {
                    return Some(r);
                }
            }
            i += 1;
        }
        None
    }

    /// 未达标行清单（验收面：虚账/降档未申报的行一眼可见）。
    pub fn rejected_rows(&self) -> Vec<&RebuildVerdict> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if !r.accepted() {
                    out.push(r);
                }
            }
            i += 1;
        }
        out
    }

    /// 在账行数。
    pub const fn len_of(&self) -> usize {
        self.rows.len()
    }

    /// 覆盖/退场计数（账完整性声明）。
    pub const fn overwritten(&self) -> u64 {
        self.overwritten
    }
}
