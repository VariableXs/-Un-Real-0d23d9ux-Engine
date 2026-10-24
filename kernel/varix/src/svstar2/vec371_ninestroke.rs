//! VE-F0771 双色描边九宫格
//! 职责：九宫格的双色描边（内圈+外圈独立色/宽）、描边宽度约束
//!       （不越出角区）、双色渲染序列（先外后内）、描边开关。
//! 契约：总描边宽 ≤ min(角区)/2；关闭描边零绘制；双色独立可控。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec361_ninecore::Insets;

/// 单圈描边
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Stroke {
    pub width: f64,
    pub color: u8,
    pub on: bool,
}

/// 双色描边配置
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DualStroke {
    pub outer: Stroke,
    pub inner: Stroke,
}

impl DualStroke {
    pub fn none() -> DualStroke {
        DualStroke {
            outer: Stroke { width: 0.0, color: 0, on: false },
            inner: Stroke { width: 0.0, color: 0, on: false },
        }
    }
    /// 总描边宽
    pub fn total_w(&self) -> f64 {
        let w = |st: &Stroke| if st.on { st.width } else { 0.0 };
        w(&self.outer) + w(&self.inner)
    }
    /// 约束校验：总宽 ≤ min(角区)/2
    pub fn fits(&self, inset: Insets) -> bool {
        let min_corner = inset.left.min(inset.top).min(inset.right).min(inset.bottom);
        self.total_w() <= min_corner * 0.5 + 1e-12
    }
    /// 渲染序列（先外后内；关闭层跳过）
    pub fn render_order(&self) -> Vec<u8> {
        let mut out: Vec<u8> = alloc::vec![];
        if self.outer.on && self.outer.width > 0.0 {
            out.push(self.outer.color);
        }
        if self.inner.on && self.inner.width > 0.0 {
            out.push(self.inner.color);
        }
        out
    }
    /// 外圈矩形内缩（相对内容区）
    pub fn outer_ring(&self, x: f64, y: f64, w: f64, h: f64) -> Option<(f64, f64, f64, f64)> {
        if !self.outer.on || self.outer.width == 0.0 {
            return None;
        }
        Some((x, y, w, h))
    }
    /// 内圈矩形内缩
    pub fn inner_ring(&self, x: f64, y: f64, w: f64, h: f64) -> Option<(f64, f64, f64, f64)> {
        if !self.inner.on || self.inner.width == 0.0 {
            return None;
        }
        let ow = if self.outer.on { self.outer.width } else { 0.0 };
        Some((x + ow, y + ow, w - 2.0 * ow, h - 2.0 * ow))
    }
}

pub fn run_vec371_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec371");
    let ins = Insets::uniform(16.0);
    // 族一：构造与宽度
    let dual = DualStroke {
        outer: Stroke { width: 3.0, color: 10, on: true },
        inner: Stroke { width: 2.0, color: 20, on: true },
    };
    s.add("双色-总宽", dual.total_w() == 5.0, "3+2");
    s.add("双色-适配", dual.fits(ins), "5 ≤ 8");
    // 关闭外圈
    let inner_only = DualStroke { outer: Stroke { width: 3.0, color: 10, on: false }, inner: dual.inner };
    s.add("双色-关外宽", inner_only.total_w() == 2.0, "only inner");
    s.add("双色-关外适配", inner_only.fits(ins), "fits");
    // 超界
    let fat = DualStroke {
        outer: Stroke { width: 6.0, color: 1, on: true },
        inner: Stroke { width: 6.0, color: 2, on: true },
    };
    s.add("双色-超界", !fat.fits(ins), "12 > 8 rejected");
    // 恰好
    let exact = DualStroke {
        outer: Stroke { width: 4.0, color: 1, on: true },
        inner: Stroke { width: 4.0, color: 2, on: true },
    };
    s.add("双色-恰好", exact.fits(ins), "8 == 8 ok");
    // 零描边
    s.add("双色-零宽", DualStroke::none().total_w() == 0.0, "none");
    s.add("双色-零适配", DualStroke::none().fits(ins), "always fits");
    // 族二：渲染序列
    s.add("序列-先外后内", dual.render_order() == alloc::vec![10, 20], "outer then inner");
    s.add("序列-仅内", inner_only.render_order() == alloc::vec![20], "inner only");
    s.add("序列-零描边空", DualStroke::none().render_order().is_empty(), "none empty");
    // 宽度为 0 但开启 → 跳过
    let zero_w = DualStroke { outer: Stroke { width: 0.0, color: 5, on: true }, inner: Stroke { width: 0.0, color: 6, on: true } };
    s.add("序列-零宽跳过", zero_w.render_order().is_empty(), "zero width skipped");
    // 族三：环矩形
    let ring_o = dual.outer_ring(0.0, 0.0, 100.0, 80.0).expect("o");
    s.add("环-外圈原点", ring_o == (0.0, 0.0, 100.0, 80.0), "outer at border");
    let ring_i = dual.inner_ring(0.0, 0.0, 100.0, 80.0).expect("i");
    s.add("环-内圈缩进", ring_i == (3.0, 3.0, 94.0, 74.0), "inner inset by outer");
    // 关闭外圈时内圈不缩进
    let ring_i2 = inner_only.inner_ring(0.0, 0.0, 100.0, 80.0).expect("i2");
    s.add("环-无外不缩", ring_i2 == (0.0, 0.0, 100.0, 80.0), "no outer offset");
    // 关闭时返回 None
    s.add("环-关外无", DualStroke::none().outer_ring(0.0, 0.0, 10.0, 10.0).is_none(), "none outer");
    s.add("环-关内无", DualStroke::none().inner_ring(0.0, 0.0, 10.0, 10.0).is_none(), "none inner");
    // 内圈不越出（宽缩非负）
    s.add("环-内圈非负", ring_i.2 >= 0.0 && ring_i.3 >= 0.0, "inner non-negative");
    // 族四：颜色独立
    let c1 = DualStroke { outer: Stroke { width: 2.0, color: 100, on: true }, inner: Stroke { width: 2.0, color: 200, on: true } };
    s.add("颜色-异色", c1.outer.color != c1.inner.color, "distinct colors");
    let c2 = DualStroke { outer: Stroke { width: 2.0, color: 100, on: true }, inner: Stroke { width: 2.0, color: 100, on: true } };
    s.add("颜色-同色合法", c2.render_order() == alloc::vec![100, 100], "same color ok");
    // 族五：与九宫格联动（fits 传入真实切片角区）
    let slice_ins = Insets::uniform(8.0);
    s.add("联动-切片适配", dual.fits(slice_ins), "dual fits real slice");
    s
}
