//! VE-F0755 布尔精度合同
//! 职责：精度档位（f32/f64 双档）定义、档位-容差-应用三列合同表、
//!       输入坐标预处理承诺（snap 到档位网格）、档位升级守恒（f64 结果 ⊇ f32 结果）。
//! 契约：合同表条目可查；预处理后坐标全部落在档位网格上。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::Pt;
use super::vec341_boolorg::Poly;

/// 精度档位
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PrecTier {
    /// 单精度：UI 级图标
    F32,
    /// 双精度：排版/布尔链
    F64,
}

impl PrecTier {
    /// 网格步长（snap 单位）
    pub fn grid(self) -> f64 {
        match self {
            PrecTier::F32 => 1.0 / 1024.0,
            PrecTier::F64 => 1.0 / (1u64 << 40) as f64,
        }
    }
    /// 合同容差
    pub fn tol(self) -> f64 {
        match self {
            PrecTier::F32 => 1e-3,
            PrecTier::F64 => 1e-9,
        }
    }
    /// 适用域
    pub fn domain(self) -> &'static str {
        match self {
            PrecTier::F32 => "UI 图标与蒙版",
            PrecTier::F64 => "排版与级联布尔",
        }
    }
}

/// 合同表行
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ContractRow {
    pub tier: PrecTier,
    pub tol: f64,
    pub domain: &'static str,
}

/// 合同表（可查）
pub fn contract_table() -> Vec<ContractRow> {
    alloc::vec![
        ContractRow { tier: PrecTier::F32, tol: PrecTier::F32.tol(), domain: PrecTier::F32.domain() },
        ContractRow { tier: PrecTier::F64, tol: PrecTier::F64.tol(), domain: PrecTier::F64.domain() },
    ]
}

pub fn lookup(tier: PrecTier) -> Option<ContractRow> {
    contract_table().into_iter().find(|r| r.tier == tier)
}

/// snap 到档位网格
pub fn snap_coord(x: f64, tier: PrecTier) -> f64 {
    let g = tier.grid();
    (x / g).round() * g
}

/// 预处理：全部顶点 snap
pub fn preprocess(p: &Poly, tier: PrecTier) -> Poly {
    Poly::new(p.verts.iter().map(|v| Pt::new(snap_coord(v.x, tier), snap_coord(v.y, tier))).collect())
}

/// 预处理后顶点全部落在网格上
pub fn on_grid(p: &Poly, tier: PrecTier) -> bool {
    let g = tier.grid();
    p.verts.iter().all(|v| {
        let rx = v.x / g;
        let ry = v.y / g;
        (rx - rx.round()).abs() < 1e-6 && (ry - ry.round()).abs() < 1e-6
    })
}

/// 档位升级守恒：f32 snap 后形状与 f64 snap 后形状的面积差有界
pub fn upgrade_drift(p: &Poly) -> f64 {
    let p32 = preprocess(p, PrecTier::F32);
    let p64 = preprocess(p, PrecTier::F64);
    (p32.area() - p64.area()).abs()
}

pub fn run_vec355_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec355");
    // 族一：合同表
    let table = contract_table();
    s.add("合同-两档", table.len() == 2, "2 tiers");
    s.add("合同-有序", PrecTier::F32 < PrecTier::F64, "ordered");
    s.add("合同-查f32", lookup(PrecTier::F32).expect("r").tier == PrecTier::F32, "lookup f32");
    s.add("合同-查f64", lookup(PrecTier::F64).expect("r").tol == 1e-9, "f64 tol");
    // 表内自洽（tol 与 tier 方法一致）
    s.add("合同-自洽", table.iter().all(|r| r.tol == r.tier.tol() && r.domain == r.tier.domain()), "consistent");
    // 族二：网格
    s.add("网格-f32", (PrecTier::F32.grid() - 1.0 / 1024.0).abs() < 1e-15, "1/1024");
    s.add("网格-f64细", PrecTier::F64.grid() < PrecTier::F32.grid() / 1e6, "f64 much finer");
    // snap 行为
    let g = PrecTier::F32.grid();
    s.add("snap-整点", snap_coord(3.0, PrecTier::F32) == 3.0, "integer kept");
    s.add("snap-半格", (snap_coord(g * 1.4, PrecTier::F32) - g).abs() < 1e-12, "rounds to nearest");
    s.add("snap-负数", (snap_coord(-g * 0.6, PrecTier::F32) + g).abs() < 1e-12, "negative rounds");
    // 族三：预处理
    let noisy = Poly::new(alloc::vec![
        Pt::new(0.0, 0.0),
        Pt::new(4.0000001, 0.0000001),
        Pt::new(4.0, 4.0),
        Pt::new(0.0, 4.0),
    ]);
    let clean = preprocess(&noisy, PrecTier::F32);
    s.add("预处理-在网格", on_grid(&clean, PrecTier::F32), "on grid");
    s.add("预处理-顶点数不变", clean.verts.len() == 4, "same count");
    // f64 档不破坏精细坐标
    let fine = Poly::new(alloc::vec![Pt::new(0.0, 0.0), Pt::new(g * 3.0, 0.0), Pt::new(g * 3.0, g * 3.0), Pt::new(0.0, g * 3.0)]);
    let fine64 = preprocess(&fine, PrecTier::F64);
    s.add("预处理-f64保精", on_grid(&fine64, PrecTier::F64) && (fine64.area() - fine.area()).abs() < 1e-18, "f64 preserved");
    // f32 会破坏精细坐标（升档漂移 > 0 检测）
    s.add("预处理-f32有损", {
        let fine32 = preprocess(&fine, PrecTier::F32);
        (fine32.area() - fine.area()).abs() > 1e-9
    }, "f32 lossy on fine coords");
    // 族四：漂移有界
    let drift = upgrade_drift(&noisy);
    s.add("漂移-有界", drift < PrecTier::F32.tol() * 10.0, "drift bounded");
    s.add("漂移-非负", drift >= 0.0, "non-negative");
    // 族五：容差语义（f32 合同容差 > f64）
    s.add("容差-降序", PrecTier::F32.tol() > PrecTier::F64.tol(), "f32 looser");
    // 族六：领域文本非空
    s.add("领域-非空", table.iter().all(|r| !r.domain.is_empty()), "domains described");
    s
}
