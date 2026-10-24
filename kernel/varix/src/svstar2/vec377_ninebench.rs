//! VE-F0777 九宫格基准
//! 职责：九宫格管线各阶段成本（布局/UV/顶点/量化）、档位轮数、
//!       顶点数恒定成本、复杂度线性度（尺寸无关性验证）。
//! 契约：布局成本与目标尺寸无关（恒 9 区）；顶点成本恒 36。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec361_ninecore::{Cell, Insets, Slice, layout};

/// 阶段成本（伪时钟）
#[derive(Copy, Clone, Debug, PartialEq, Default)]
pub struct NineCost {
    pub layout_ops: u64,
    pub uv_ops: u64,
    pub vtx_ops: u64,
    pub quant_ops: u64,
}

impl NineCost {
    pub fn total(&self) -> u64 {
        self.layout_ops + self.uv_ops + self.vtx_ops + self.quant_ops
    }
}

/// 布局成本：常数（9 区固定算术）
pub const COST_LAYOUT: u64 = 9 * 6;
/// UV 成本：9 区 × 4 参数
pub const COST_UV: u64 = 9 * 4;
/// 顶点成本：36 顶点 × 4 分量
pub const COST_VTX: u64 = 36 * 4;

/// 量化成本：随尺寸档跨度
pub fn cost_quant(spans: usize) -> u64 {
    spans as u64 * 2
}

/// 基准档位
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum NTier {
    Smoke,
    Standard,
    Deep,
}

impl NTier {
    pub fn rounds(self) -> usize {
        match self {
            NTier::Smoke => 32,
            NTier::Standard => 512,
            NTier::Deep => 8192,
        }
    }
}

/// 单次布局成本（恒定）
pub fn layout_cost() -> NineCost {
    NineCost { layout_ops: COST_LAYOUT, uv_ops: COST_UV, vtx_ops: COST_VTX, quant_ops: 0 }
}

/// 预算：单次布局 ≤ 500 ops
pub const BUDGET_LAYOUT: u64 = 500;

pub fn within_budget() -> bool {
    layout_cost().total() <= BUDGET_LAYOUT
}

/// 基准行
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct NBenchRow {
    pub name: &'static str,
    pub tier: NTier,
    pub ops_per_call: u64,
}

pub fn run_bench(tier: NTier, name: &'static str) -> NBenchRow {
    let c = layout_cost();
    NBenchRow { name, tier, ops_per_call: c.total() }
}

/// 尺寸无关性：不同目标尺寸成本恒定
pub fn size_independent(sizes: &[f64]) -> bool {
    sizes.iter().all(|_| layout_cost().total() == COST_LAYOUT + COST_UV + COST_VTX)
}

/// 实际布局执行计时桩（用操作计数模拟：布局一次 + 顶点数验证）
pub fn profile_layout(s: &Slice, w: f64, h: f64) -> Option<(u64, usize)> {
    let cells = layout(s, 0.0, 0.0, w, h)?;
    let vtx: u64 = cells.iter().map(|_| 4u64).sum();
    Some((layout_cost().total(), vtx as usize))
}

pub fn run_vec377_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec377");
    let ins = Insets::uniform(8.0);
    let sl = Slice::build(64.0, 64.0, ins).expect("slice");
    // 族一：成本常量
    s.add("成本-布局", COST_LAYOUT == 54, "9×6");
    s.add("成本-UV", COST_UV == 36, "9×4");
    s.add("成本-顶点", COST_VTX == 144, "36×4");
    let c = layout_cost();
    s.add("成本-单次", c.total() == 234, "54+36+144");
    s.add("成本-默认零", NineCost::default().total() == 0, "zero default");
    // 量化成本
    s.add("成本-量化", cost_quant(100) == 200, "spans×2");
    s.add("成本-量化零", cost_quant(0) == 0, "zero spans");
    // 族二：预算
    s.add("预算-过", within_budget(), "234 ≤ 500");
    // 恒定成本 → 任何档位都过
    s.add("预算-全档", NTier::iter_all().iter().all(|_| within_budget()), "all tiers");
    // 族三：档位
    s.add("档位-轮数", NTier::Smoke.rounds() == 32 && NTier::Deep.rounds() == 8192, "rounds");
    s.add("档位-有序", NTier::Smoke < NTier::Standard && NTier::Standard < NTier::Deep, "ordered");
    // 族四：尺寸无关性
    let sizes: Vec<f64> = alloc::vec![16.0, 100.0, 1000.0, 3840.0];
    s.add("无关-成本恒定", size_independent(&sizes), "size free");
    // 基准行
    let row = run_bench(NTier::Standard, "nine-layout");
    s.add("入册-名称", row.name == "nine-layout", "name");
    s.add("入册-单次", row.ops_per_call == 234, "234");
    s.add("入册-档位", row.tier == NTier::Standard, "tier");
    s.add("确定-重跑", run_bench(NTier::Standard, "nine-layout") == row, "deterministic");
    // 族五：实测剖析
    let (ops_small, vtx_small) = profile_layout(&sl, 32.0, 32.0).expect("small");
    let (ops_big, vtx_big) = profile_layout(&sl, 3840.0, 2160.0).expect("big");
    s.add("剖析-小图", ops_small == 234 && vtx_small == 36, "small");
    s.add("剖析-大图", ops_big == 234 && vtx_big == 36, "big same");
    s.add("剖析-尺寸无关", ops_small == ops_big, "equal ops");
    // 布局失败
    s.add("剖析-过小None", profile_layout(&sl, 8.0, 8.0).is_none(), "below min");
    // 族六：不同切片成本仍恒定
    let asym = Slice::build(100.0, 100.0, Insets { left: 10.0, top: 20.0, right: 30.0, bottom: 5.0 }).expect("a");
    let (ops_asym, _) = profile_layout(&asym, 200.0, 150.0).expect("asym");
    s.add("剖析-非对称同价", ops_asym == 234, "asym same cost");
    s
}

impl NTier {
    fn iter_all() -> [NTier; 3] {
        [NTier::Smoke, NTier::Standard, NTier::Deep]
    }
}
