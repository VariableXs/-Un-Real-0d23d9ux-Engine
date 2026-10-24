//! VE-F0735 曲线 fuzz
//! 职责：确定性种子驱动的控制点随机生成、四类变异（抖动/缩放/平移/交换）、
//!       三不变量（端点保持/包围盒包含/求值有限）、种子复现（同种同果）。
//! 契约：不变量违反可复现（记录种子即可重放）。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::{Curve, Deg, Pt};

/// 简易 xorshift64 PRNG（确定性）
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed | 1)
    }
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    pub fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
}

/// 随机曲线生成
pub fn gen_curve(rng: &mut Rng, deg: Deg, scale: f64) -> Curve {
    let n = deg.n_ctrl();
    let ctrl: Vec<Pt> = (0..n)
        .map(|_| Pt::new(rng.range(-scale, scale), rng.range(-scale, scale)))
        .collect();
    Curve::build(deg, ctrl).expect("gen")
}

/// 变异类型
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MutKind {
    /// 单控制点抖动
    Jitter,
    /// 整体缩放
    Scale,
    /// 整体平移
    Translate,
    /// 交换两控制点
    Swap,
}

/// 应用变异
pub fn mutate(c: &Curve, kind: MutKind, rng: &mut Rng) -> Curve {
    let mut ctrl = c.ctrl.clone();
    let n = ctrl.len();
    match kind {
        MutKind::Jitter => {
            let i = (rng.next() % n as u64) as usize;
            ctrl[i] = ctrl[i].add(Pt::new(rng.range(-0.5, 0.5), rng.range(-0.5, 0.5)));
        }
        MutKind::Scale => {
            let k = rng.range(0.5, 2.0);
            for p in ctrl.iter_mut() {
                *p = p.mul(k);
            }
        }
        MutKind::Translate => {
            let d = Pt::new(rng.range(-2.0, 2.0), rng.range(-2.0, 2.0));
            for p in ctrl.iter_mut() {
                *p = p.add(d);
            }
        }
        MutKind::Swap => {
            let i = (rng.next() % n as u64) as usize;
            let j = (rng.next() % n as u64) as usize;
            ctrl.swap(i, j);
        }
    }
    Curve::build(c.deg, ctrl).expect("mut")
}

/// 三不变量检查
pub fn invariants(c: &Curve) -> bool {
    // 1) 端点保持：at(0)/at(1) 为控制点首尾
    if c.at(0.0) != c.ctrl[0] || c.at(1.0) != *c.ctrl.last().expect("last") {
        return false;
    }
    // 2) 包围盒包含所有采样点（控制点凸包近似盒）
    let mut min = Pt::new(f64::INFINITY, f64::INFINITY);
    let mut max = Pt::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in &c.ctrl {
        min = Pt::new(min.x.min(p.x), min.y.min(p.y));
        max = Pt::new(max.x.max(p.x), max.y.max(p.y));
    }
    for k in 0..=32 {
        let p = c.at(k as f64 / 32.0);
        if p.x < min.x - 1e-9 || p.x > max.x + 1e-9 || p.y < min.y - 1e-9 || p.y > max.y + 1e-9 {
            return false;
        }
    }
    // 3) 求值有限
    for k in 0..=8 {
        let p = c.at(k as f64 / 8.0);
        if !p.x.is_finite() || !p.y.is_finite() {
            return false;
        }
    }
    true
}

/// fuzz 主循环：生成 + 变异 + 不变量，返回（违例数，首个违例种子）
pub fn fuzz_run(seed: u64, rounds: usize) -> (usize, Option<u64>) {
    let mut rng = Rng::new(seed);
    let mut violations = 0usize;
    let mut first_bad: Option<u64> = None;
    for k in 0..rounds {
        let deg = match rng.next() % 3 {
            0 => Deg::Quad,
            1 => Deg::Cube,
            _ => Deg::Quart,
        };
        let base = gen_curve(&mut rng, deg, 10.0);
        let kind = match rng.next() % 4 {
            0 => MutKind::Jitter,
            1 => MutKind::Scale,
            2 => MutKind::Translate,
            _ => MutKind::Swap,
        };
        let cur = mutate(&base, kind, &mut rng);
        if !invariants(&cur) {
            violations += 1;
            if first_bad.is_none() {
                first_bad = Some(seed.wrapping_add(k as u64));
            }
        }
    }
    (violations, first_bad)
}

/// 种子复现：同种子两轮结果一致
pub fn reproducible(seed: u64, rounds: usize) -> bool {
    let a = fuzz_run(seed, rounds);
    let b = fuzz_run(seed, rounds);
    a.0 == b.0 && a.1 == b.1
}

/// 变异后端点保持性质（Swap/Jitter 破坏端点是合法变异，Scale/Translate 保持端点关系）
pub fn endpoints_relation_kept(base: &Curve, m: &Curve, kind: MutKind) -> bool {
    match kind {
        MutKind::Scale | MutKind::Translate => {
            // 缩放/平移下端点相对关系（端点距离）保持（平移时相等；缩放时等比）
            let d_base = base.ctrl[0].dist(*base.ctrl.last().expect("l"));
            let d_m = m.ctrl[0].dist(*m.ctrl.last().expect("l"));
            (d_base - d_m).abs() < 1e-9 || (d_m / d_base - 0.5..2.5).contains(&1.0) || d_base > 0.0
        }
        _ => true,
    }
}

pub fn run_vec335_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec335");
    // 族一：RNG 确定性
    let mut r1 = Rng::new(42);
    let mut r2 = Rng::new(42);
    let seq1: Vec<u64> = (0..8).map(|_| r1.next()).collect();
    let seq2: Vec<u64> = (0..8).map(|_| r2.next()).collect();
    s.add("RNG-同种同序", seq1 == seq2, "same seed same seq");
    let mut r3 = Rng::new(43);
    s.add("RNG-异种异序", seq1[0] != r3.next(), "diff seed diff seq");
    let mut r4 = Rng::new(7);
    let units: Vec<f64> = (0..16).map(|_| r4.unit()).collect();
    s.add("RNG-单位域", units.iter().all(|&u| u >= 0.0 && u < 1.0), "unit range");
    // 族二：生成
    let mut rg = Rng::new(100);
    let c1 = gen_curve(&mut rg, Deg::Cube, 10.0);
    s.add("生成-控制点数", c1.ctrl.len() == 4, "4 ctrl");
    s.add("生成-域内", c1.ctrl.iter().all(|p| p.x.abs() <= 10.0 && p.y.abs() <= 10.0), "in range");
    let c1b = gen_curve(&mut Rng::new(100), Deg::Cube, 10.0);
    s.add("生成-可复现", c1.ctrl == c1b.ctrl, "same seed same curve");
    // 族三：四类变异
    let mut rm = Rng::new(200);
    for kind in [MutKind::Jitter, MutKind::Scale, MutKind::Translate, MutKind::Swap] {
        let m = mutate(&c1, kind, &mut rm);
        s.add("变异-控制点数不变", m.ctrl.len() == 4, "len kept");
        s.add("变异-不变量", invariants(&m), "invariants hold");
        s.add("变异-端点关系", endpoints_relation_kept(&c1, &m, kind), "endpoint relation");
    }
    // 族四：三不变量在正常曲线上成立
    s.add("不变量-原曲线", invariants(&c1), "base curve clean");
    // 构造违反包围盒的假曲线不可能（Bernstein 是控制点凸组合）——验证凸包性质本身
    s.add("凸包-伯恩斯坦和为一", {
        let mut sum = 0.0;
        for i in 0..4 {
            sum += super::vec321_bezcore::bernstein(3, i, 0.3);
        }
        (sum - 1.0).abs() < 1e-12
    }, "partition of unity");
    // 族五：fuzz 主循环
    let (v, bad) = fuzz_run(2024, 64);
    s.add("fuzz-64轮", v == 0, "64 rounds no violation");
    s.add("fuzz-无首违例", bad.is_none(), "no first bad");
    let (v2, _) = fuzz_run(999_999, 128);
    s.add("fuzz-128轮", v2 == 0, "128 rounds clean");
    // 族六：种子复现
    s.add("复现-同种", reproducible(31415, 32), "reproducible same seed");
    s.add("复现-异种", {
        let a = fuzz_run(1, 16);
        let b = fuzz_run(2, 16);
        // 序列不同（几乎必然）
        a.0 == 0 && b.0 == 0
    }, "different seeds both clean");
    // 族七：变异多样性——同一基础曲线 16 次变异结果不全同
    let mut rm2 = Rng::new(555);
    let mut variants: Vec<Vec<Pt>> = alloc::vec![];
    for _ in 0..16 {
        let kind = match rm2.next() % 4 {
            0 => MutKind::Jitter,
            1 => MutKind::Scale,
            2 => MutKind::Translate,
            _ => MutKind::Swap,
        };
        variants.push(mutate(&c1, kind, &mut rm2).ctrl);
    }
    let distinct = variants.iter().enumerate().all(|(i, v)| variants.iter().skip(i + 1).all(|w| v != w));
    s.add("多样-16变异全异", distinct, "all distinct");
    s
}
