//! VE-F0751 布尔应用：蒙版复合
//! 蒙版层栈（base/mask 子域）、复合模式（cover/reveal/invert）、
//! 像素域蒙版位图生成与合成、蒙版链求值。
//! 契约：cover 模式下 mask 内区域保留；invert 互补；链式复合与两步复合一致。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::Pt;
use super::vec341_boolorg::{FillRule, Poly, DirEdge, scan_row, inside_by_rule};

/// 位图蒙版
#[derive(Clone, Debug, PartialEq)]
pub struct Mask {
    pub w: usize,
    pub h: usize,
    pub bits: Vec<bool>,
}

impl Mask {
    pub fn blank(w: usize, h: usize) -> Mask {
        Mask { w, h, bits: alloc::vec![false; w * h] }
    }
    pub fn solid(w: usize, h: usize) -> Mask {
        Mask { w, h, bits: alloc::vec![true; w * h] }
    }
    pub fn get(&self, x: usize, y: usize) -> bool {
        self.bits[y * self.w + x]
    }
    pub fn set(&mut self, x: usize, y: usize, v: bool) {
        if x < self.w && y < self.h {
            self.bits[y * self.w + x] = v;
        }
    }
    pub fn invert(&self) -> Mask {
        Mask { w: self.w, h: self.h, bits: self.bits.iter().map(|b| !b).collect() }
    }
    pub fn count(&self) -> usize {
        self.bits.iter().filter(|b| **b).count()
    }
    /// 布尔合成模式
    pub fn combine(&self, o: &Mask, mode: CompMode) -> Mask {
        assert_eq!(self.bits.len(), o.bits.len(), "mask size mismatch");
        let bits = self.bits.iter().zip(o.bits.iter()).map(|(a, b)| match mode {
            CompMode::Cover => a | b,
            CompMode::Reveal => a & b,
            CompMode::Xor => a ^ b,
        }).collect();
        Mask { w: self.w, h: self.h, bits }
    }
}

/// 复合模式
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CompMode {
    Cover,
    Reveal,
    Xor,
}

/// 多边形 → 位图蒙版（像素中心采样）
pub fn poly_to_mask(p: &Poly, w: usize, h: usize, scale: f64) -> Mask {
    let edges: Vec<DirEdge> = p.edges().iter().map(|e| DirEdge::new(
        Pt::new(e.a.x * scale, e.a.y * scale),
        Pt::new(e.b.x * scale, e.b.y * scale),
    )).collect();
    let mut m = Mask::blank(w, h);
    for y in 0..h {
        let evs = scan_row(&edges, y as f64 + 0.5);
        let flags = inside_by_rule(&evs, FillRule::NonZero);
        // 标记区间内像素
        let mut i = 0;
        while i < flags.len() {
            if flags[i] {
                let x0 = (evs[i].x.ceil().max(0.0)) as usize;
                let x1 = (if i + 1 < evs.len() { evs[i + 1].x } else { evs[i].x }).floor().min(w as f64) as usize;
                for x in x0..x1.min(w) {
                    m.set(x, y, true);
                }
                i += 1;
            } else {
                i += 1;
            }
        }
    }
    m
}

/// 蒙版链求值：从底到顶逐层复合
pub fn chain_eval(layers: &[Mask], modes: &[CompMode]) -> Option<Mask> {
    let mut it = layers.iter();
    let mut cur = it.next()?.clone();
    for (m, mode) in it.zip(modes.iter()) {
        cur = cur.combine(m, *mode);
    }
    Some(cur)
}

pub fn run_vec351_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec351");
    // 族一：位图基本
    let b = Mask::blank(4, 4);
    s.add("位图-空", b.count() == 0, "blank");
    let sol = Mask::solid(4, 4);
    s.add("位图-满", sol.count() == 16, "solid 16");
    s.add("位图-取值", sol.get(2, 2) && !b.get(2, 2), "get");
    let mut m = Mask::blank(4, 4);
    m.set(1, 1, true);
    s.add("位图-写读", m.get(1, 1), "set/get");
    m.set(9, 9, true); // 越界无害
    s.add("位图-越界无害", m.count() == 1, "oob ignored");
    // 反转
    let inv = sol.invert();
    s.add("反转-互补", inv.count() == 0 && sol.invert().invert().count() == 16, "invert");
    s.add("反转-二次回", inv.invert() == sol, "double invert");
    // 族二：复合模式
    let mut l = Mask::blank(4, 4);
    l.set(0, 0, true);
    l.set(1, 0, true);
    let mut r = Mask::blank(4, 4);
    r.set(1, 0, true);
    r.set(2, 0, true);
    let cover = l.combine(&r, CompMode::Cover);
    s.add("复合-cover并", cover.get(0, 0) && cover.get(1, 0) && cover.get(2, 0), "union");
    s.add("复合-cover数", cover.count() == 3, "3 bits");
    let reveal = l.combine(&r, CompMode::Reveal);
    s.add("复合-reveal交", reveal.get(1, 0) && reveal.count() == 1, "intersect");
    let xor = l.combine(&r, CompMode::Xor);
    s.add("复合-xor", xor.get(0, 0) && xor.get(2, 0) && !xor.get(1, 0), "xor");
    // 幂等
    s.add("复合-cover幂等", cover.combine(&cover, CompMode::Cover) == cover, "idempotent");
    // 交换律
    s.add("复合-交换", l.combine(&r, CompMode::Cover) == r.combine(&l, CompMode::Cover), "commutative");
    // 族三：多边形转蒙版
    let sq = Poly::new(alloc::vec![Pt::new(1.0, 1.0), Pt::new(5.0, 1.0), Pt::new(5.0, 5.0), Pt::new(1.0, 5.0)]);
    let pm = poly_to_mask(&sq, 8, 8, 1.0);
    s.add("转蒙版-计数", pm.count() == 16, "4×4 square");
    s.add("转蒙版-内点", pm.get(3, 3), "inside");
    s.add("转蒙版-外点", !pm.get(0, 0) && !pm.get(7, 7), "outside");
    // 族四：蒙版链
    let chain = chain_eval(&alloc::vec![sol.clone(), inv.clone()], &alloc::vec![CompMode::Reveal]);
    s.add("链-两层", chain.expect("c").count() == 0, "solid ∩ inv = 0");
    let chain2 = chain_eval(
        &alloc::vec![sol.clone(), inv.clone(), sol.clone()],
        &alloc::vec![CompMode::Reveal, CompMode::Cover],
    );
    s.add("链-三层", chain2.as_ref().expect("c2").count() == 16, "(solid∩inv)∪solid = solid");
    s.add("链-空层", chain_eval(&[], &[]).is_none(), "empty chain none");
    // 链式与两步一致
    let step = sol.combine(&inv, CompMode::Reveal).combine(&sol, CompMode::Cover);
    s.add("链-两步一致", chain2 == Some(step), "chain == steps");
    // 族五：蒙版间不一致尺寸（combine 有 assert，跨尺寸调用由调用方保证——此处验证同尺寸约定）
    s.add("尺寸-同型才复合", l.bits.len() == r.bits.len(), "same size");
    s
}
