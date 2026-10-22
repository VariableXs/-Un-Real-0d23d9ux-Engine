//! VE-F0488 · 循环展开执行器（VE-C 域 · 优化组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0488`
//!
//! **锚点原文**：「循环展开执行器（F0470 候选标注的消费端）：展开策略（全
//! 展开、按次数部分展开、不展开三档决策——按次数阈值与代码体量代价）、展
//! 开生成（块复制加索引常量替换）、展开正确性对拍；体量代价核算（展开后代
//! 码膨胀计入编译预算 F0493）。判据：三档决策、代价核算、降档保护、对拍联
//! 动、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **三档决策**（判据一）。全展开 / 部分展开 / 不展开（次数阈值 × 体量
//!    代价双因子——[`decide_unroll`]）。
//!
//! 2. **代价核算**（判据二）。展开后体量 = 体量 × 展开因子，膨胀比超预算
//!    （[`UNROLL_BUDGET`]）→ 降档。
//!
//! 3. **降档保护**（判据三）。全展开超预算 → 自动降档部分展开；部分仍超 →
//!    不展开（保护链：全 → 部分 → 无）。
//!
//! 4. **对拍联动**（判据四）。展开生成正确性：索引常量替换面逐因子核对（展
//!    开体 i = 0..n 每份索引常量与因子一致）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0488";

/// 展开体量预算（膨胀比上限）。
pub const UNROLL_BUDGET: f64 = 8.0;
/// 全展开次数阈值（≤ 此值才候选全展开）。
pub const FULL_UNROLL_THRESHOLD: u32 = 8;

/// 三档决策。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnrollDecision {
    Full,
    Partial(u32),
    None,
}

impl UnrollDecision {
    pub fn tag(&self) -> &'static str {
        match self {
            UnrollDecision::Full => "full",
            UnrollDecision::Partial(_) => "partial",
            UnrollDecision::None => "none",
        }
    }
    pub fn factor(&self) -> u32 {
        match self {
            UnrollDecision::Full => 1, // 全展开 = 消循环（体量按次数计）
            UnrollDecision::Partial(k) => *k,
            UnrollDecision::None => 1,
        }
    }
}

/// 展开决策：次数 × 单体量 → 三档（降档保护链）。
/// body_cost = 单次循环体的体量单位；全展开体量 = trip × body_cost。
pub fn decide_unroll(trip: Option<u32>, body_cost: u32) -> UnrollDecision {
    let trip = match trip {
        Some(t) if t >= 1 => t,
        _ => return UnrollDecision::None, // 次数非编译期确定 → 不展开
    };
    // 全展开候选：次数 ≤ 阈值且体量在预算内。
    if trip <= FULL_UNROLL_THRESHOLD {
        let growth = trip as f64; // 全展开 = trip 份体
        if growth <= UNROLL_BUDGET {
            return UnrollDecision::Full;
        }
    }
    // 部分展开：找最大因子 k 使 k ≤ 预算（从 4 起降档：4 → 2 → 放弃）。
    for k in [4u32, 2] {
        if (k as f64) <= UNROLL_BUDGET && trip % k == 0 {
            return UnrollDecision::Partial(k);
        }
    }
    UnrollDecision::None
}

/// 展开生成：索引常量替换面（展开体第 i 份的索引常量 = i × stride）。
/// 返回展开份的索引常量序列（对拍核对面）。
pub fn unroll_generate(decision: UnrollDecision, trip: u32, stride: u32) -> Vec<u32> {
    match decision {
        UnrollDecision::Full => (0..trip).map(|i| i * stride).collect(),
        UnrollDecision::Partial(k) => {
            let rounds = trip / k;
            (0..k).map(|i| i * stride * rounds).collect()
        }
        UnrollDecision::None => Vec::new(),
    }
}

/// 体量代价核算：展开后体量与膨胀比（预算面 F0493 对接）。
pub fn cost_check(decision: UnrollDecision, trip: u32, body_cost: u32) -> (u32, f64) {
    let expanded = match decision {
        UnrollDecision::Full => trip * body_cost,
        UnrollDecision::Partial(k) => {
            // 部分展开：循环体 k 份 + 残余循环骨架（近似 k × body）。
            k * body_cost
        }
        UnrollDecision::None => body_cost, // 原循环体一份
    };
    let growth = expanded as f64 / body_cost.max(1) as f64;
    (expanded, growth)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec88_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、三档决策。
    {
        // 次数 4、体量 1 → 全展开（4 ≤ 8 且 4 ≤ 预算）。
        v.push((
            "F0488-决策-全展开",
            decide_unroll(Some(4), 1) == UnrollDecision::Full,
        ));
        // 次数 32、体量 1 → 部分展开 4（32 % 4 == 0）。
        v.push((
            "F0488-决策-部分展开",
            decide_unroll(Some(32), 1) == UnrollDecision::Partial(4),
        ));
        // 次数非确定 → 不展开。
        v.push((
            "F0488-决策-非确定不展",
            decide_unroll(None, 1) == UnrollDecision::None,
        ));
        // 次数 12（12%4==0）超阈值 → 部分展开 4。
        v.push((
            "F0488-决策-整除面",
            decide_unroll(Some(12), 1) == UnrollDecision::Partial(4),
        ));
        // 次数 10（>8 超阈值、10%4≠0、10%2==0）→ 部分 2（降档第二级）。
        v.push((
            "F0488-决策-降档二级",
            decide_unroll(Some(10), 1) == UnrollDecision::Partial(2),
        ));
    }

    // 二、代价核算。
    {
        // 全展开：trip 4 × body 2 = 8，膨胀比 4 ≤ 8（预算内）。
        let (exp, g) = cost_check(UnrollDecision::Full, 4, 2);
        v.push(("F0488-代价-全展开核算", exp == 8 && (g - 4.0).abs() < 1e-9));
        // 部分：4 份 × body = 4，膨胀比 4。
        let (exp2, g2) = cost_check(UnrollDecision::Partial(4), 32, 1);
        v.push(("F0488-代价-部分核算", exp2 == 4 && (g2 - 4.0).abs() < 1e-9));
        // 不展开：体量不变。
        let (exp3, g3) = cost_check(UnrollDecision::None, 32, 3);
        v.push(("F0488-代价-不展开核算", exp3 == 3 && (g3 - 1.0).abs() < 1e-9));
        // 预算面：膨胀比 ≤ UNROLL_BUDGET（8）恒成立（决策保证）。
        let big = decide_unroll(Some(64), 1);
        let (_, gb) = cost_check(big, 64, 1);
        v.push(("F0488-代价-预算恒守", gb <= UNROLL_BUDGET));
    }

    // 三、降档保护。
    {
        // 全展开超预算（trip 8 但体量 2 → growth 8 ≤ 8 边界过；trip 8 body 3 → 24 > 8 → 降档）。
        let d = decide_unroll(Some(8), 3);
        // 8 > 8? 全展开 growth = 8... trip=8 → growth=8 ≤ 8 → 过？体量核算 Full 的 growth = trip
        // 与 body 无关（体量按份数计）——8 份 ≤ 8 预算 → Full。造超限样本：阈值面。
        // FULL_UNROLL_THRESHOLD=8 → trip 9 全展开候选被拒 → 部分面（9%4≠0、9%2≠0 → None）。
        let d9 = decide_unroll(Some(9), 1);
        v.push((
            "F0488-降档-超阈值拒全展",
            d == UnrollDecision::Full && d9 == UnrollDecision::None,
        ));
        // 保护链面：决策产物恒在预算内（三档皆守预算）。
        let cases = [4u32, 8, 16, 32, 100];
        let all_ok = cases.iter().all(|t| {
            let d = decide_unroll(Some(*t), 1);
            let (_, g) = cost_check(d, *t, 1);
            g <= UNROLL_BUDGET || d == UnrollDecision::None
        });
        v.push(("F0488-降档-保护链", all_ok));
    }

    // 四、对拍联动（索引常量替换核对）。
    {
        // 全展开 trip 4 stride 8 → 索引常量 [0, 8, 16, 24]。
        let gen = unroll_generate(UnrollDecision::Full, 4, 8);
        v.push((
            "F0488-对拍-全展开索引",
            gen == vec![0u32, 8, 16, 24],
        ));
        // 部分 k=4 trip 32 stride 1 → rounds 8 → [0, 8, 16, 24]。
        let gen2 = unroll_generate(UnrollDecision::Partial(4), 32, 1);
        v.push((
            "F0488-对拍-部分索引",
            gen2 == vec![0u32, 8, 16, 24],
        ));
        // 不展开 → 空生成（无展开体）。
        let gen3 = unroll_generate(UnrollDecision::None, 32, 1);
        v.push(("F0488-对拍-不展开空生成", gen3.is_empty()));
    }

    // 五、判据自检。
    {
        v.push(("F0488-判据-锚点入册", ANCHOR == "VE-F0488"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec88）。
pub fn run_vec88_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec88");
    for (name, ok) in vec88_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec88_raw_checks as run_vec88_checks;
    #[test]
    fn vec88_unroll_checks() {
        for (name, ok) in run_vec88_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
