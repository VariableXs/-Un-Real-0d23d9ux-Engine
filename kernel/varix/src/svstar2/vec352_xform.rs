//! VE-F0752 布尔与变换组合
//! 职责：仿射变换（平移/缩放/旋转）作用于多边形、变换-布尔次序律验证
//!       （先变换后裁剪 = 先裁剪后变换——平移/缩放可换序，旋转不可与轴向裁剪换序）、
//!       变换下面积缩放因子、组合矩阵。
//! 契约：平移不变面积；缩放面积按 k²；旋转保面积；组合矩阵可逆。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::Pt;
use super::vec341_boolorg::Poly;

/// 仿射变换（2×3：线性部分 + 平移）
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Affine {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Affine {
    pub fn identity() -> Affine {
        Affine { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 }
    }
    pub fn translate(tx: f64, ty: f64) -> Affine {
        Affine { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: tx, f: ty }
    }
    pub fn scale(k: f64) -> Affine {
        Affine { a: k, b: 0.0, c: 0.0, d: k, e: 0.0, f: 0.0 }
    }
    pub fn rotate(th: f64) -> Affine {
        let (sn, cs) = th.sin_cos();
        Affine { a: cs, b: -sn, c: sn, d: cs, e: 0.0, f: 0.0 }
    }
    pub fn apply(&self, p: Pt) -> Pt {
        Pt::new(
            self.a * p.x + self.c * p.y + self.e,
            self.b * p.x + self.d * p.y + self.f,
        )
    }
    /// 复合：先 self 再 o（o ∘ self）
    pub fn then(&self, o: Affine) -> Affine {
        Affine {
            a: o.a * self.a + o.c * self.b,
            b: o.b * self.a + o.d * self.b,
            c: o.a * self.c + o.c * self.d,
            d: o.b * self.c + o.d * self.d,
            e: o.a * self.e + o.c * self.f + o.e,
            f: o.b * self.e + o.d * self.f + o.f,
        }
    }
    /// 行列式（面积缩放因子）
    pub fn det(&self) -> f64 {
        self.a * self.d - self.b * self.c
    }
    /// 逆（存在时）
    pub fn inverse(&self) -> Option<Affine> {
        let det = self.det();
        if det.abs() < 1e-15 {
            return None;
        }
        Some(Affine {
            a: self.d / det,
            b: -self.b / det,
            c: -self.c / det,
            d: self.a / det,
            e: (self.c * self.f - self.d * self.e) / det,
            f: (self.b * self.e - self.a * self.f) / det,
        })
    }
}

/// 变换多边形
pub fn transform(p: &Poly, m: Affine) -> Poly {
    Poly::new(p.verts.iter().map(|&v| m.apply(v)).collect())
}

/// 变换-裁剪换序性（仅对可交换的变换：平移/均匀缩放配合矩形裁剪时换序差一个相同变换）
pub fn commutes_with_clip(m: Affine) -> bool {
    // 平移：裁剪域也平移即可换序（轴向性保持）
    let is_translation = m.a == 1.0 && m.b == 0.0 && m.c == 0.0 && m.d == 1.0;
    // 均匀缩放：轴向性保持
    let is_uniform_scale = m.b == 0.0 && m.c == 0.0 && m.a == m.d;
    is_translation || is_uniform_scale
}

pub fn run_vec352_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec352");
    let sq = Poly::new(alloc::vec![Pt::new(0.0, 0.0), Pt::new(4.0, 0.0), Pt::new(4.0, 4.0), Pt::new(0.0, 4.0)]);
    // 族一：平移
    let t = Affine::translate(2.0, 3.0);
    let tsq = transform(&sq, t);
    s.add("平移-面积不变", (tsq.area() - 16.0).abs() < 1e-12, "area kept");
    s.add("平移-顶点", tsq.verts[0] == Pt::new(2.0, 3.0), "first vert moved");
    s.add("平移-det一", t.det() == 1.0, "det 1");
    // 族二：缩放
    let k2 = Affine::scale(2.0);
    let ksq = transform(&sq, k2);
    s.add("缩放-面积四倍", (ksq.area() - 64.0).abs() < 1e-9, "×4");
    s.add("缩放-det四", (k2.det() - 4.0).abs() < 1e-12, "det k²");
    // 族三：旋转
    let r90 = Affine::rotate(core::f64::consts::FRAC_PI_2);
    let rsq = transform(&sq, r90);
    s.add("旋转-面积不变", (rsq.area() - 16.0).abs() < 1e-9, "area kept");
    s.add("旋转-顶点", rsq.verts[1].dist(Pt::new(0.0, 4.0)) < 1e-9, "(4,0)→(0,4)");
    s.add("旋转-det一", (r90.det() - 1.0).abs() < 1e-12, "det 1");
    // 旋转 45° 面积仍不变
    let r45 = Affine::rotate(core::f64::consts::FRAC_PI_4);
    s.add("旋转-45面积", (transform(&sq, r45).area() - 16.0).abs() < 1e-9, "45° kept");
    // 族四：矩阵复合
    let tr = Affine::translate(1.0, 0.0).then(Affine::scale(2.0));
    // 先平移后缩放：p → 2(p + (1,0))
    let p = tr.apply(Pt::new(1.0, 1.0));
    s.add("复合-次序", p.dist(Pt::new(4.0, 2.0)) < 1e-12, "scale∘translate");
    let tr2 = Affine::scale(2.0).then(Affine::translate(1.0, 0.0));
    let p2 = tr2.apply(Pt::new(1.0, 1.0));
    s.add("复合-异序", p2.dist(Pt::new(3.0, 2.0)) < 1e-12, "translate∘scale");
    // 恒等
    s.add("恒等-不变", Affine::identity().apply(Pt::new(3.0, 4.0)) == Pt::new(3.0, 4.0), "identity");
    // 结合律：(A∘B)∘C = A∘(B∘C)
    let a1 = Affine::translate(1.0, 2.0);
    let a2 = Affine::scale(3.0);
    let a3 = Affine::rotate(0.7);
    let l = a1.then(a2).then(a3);
    let r = a1.then(a2.then(a3));
    let probe = Pt::new(0.7, -1.3);
    s.add("结合-律", l.apply(probe).dist(r.apply(probe)) < 1e-9, "associative");
    // 族五：逆矩阵
    let inv = t.inverse().expect("inv");
    s.add("逆-回原", inv.apply(t.apply(probe)).dist(probe) < 1e-9, "roundtrip");
    s.add("恒等-自逆", Affine::identity().inverse() == Some(Affine::identity()), "I⁻¹ = I");
    // 退化矩阵（零缩放）
    let deg = Affine { a: 0.0, b: 0.0, c: 0.0, d: 0.0, e: 0.0, f: 0.0 };
    s.add("退化-无逆", deg.inverse().is_none(), "singular");
    // 族六：换序性
    s.add("换序-平移可", commutes_with_clip(Affine::translate(1.0, 1.0)), "translation ok");
    s.add("换序-缩放可", commutes_with_clip(Affine::scale(2.0)), "scale ok");
    s.add("换序-旋转不可", !commutes_with_clip(r45), "rotation breaks axis");
    // 族七：多边形变换后面积 = 原面积 × |det|
    for m in [t, k2, r90, r45] {
        let ta = transform(&sq, m).area();
        s.add("面积-缩放律", (ta - sq.area() * m.det().abs()).abs() < 1e-9, "|det| scaling");
    }
    s
}
