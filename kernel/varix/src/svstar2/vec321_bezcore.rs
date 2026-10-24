//! VE-F0721 贝塞尔数学库核心
//! 职责：二/三/四次贝塞尔曲线的统一定义、求值、导数、极值，
//!       全部标量运算 f64 精度，供栅格化与路径子系统共用。
//! 契约：B(t) 多项式形式与 de Casteljau 两种求值路径必须逐点一致；
//!       导数用控制点差分构造；极值端点必含。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 点（二维，f64）
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
}

impl Pt {
    pub const ZERO: Pt = Pt { x: 0.0, y: 0.0 };
    pub fn new(x: f64, y: f64) -> Pt {
        Pt { x, y }
    }
    pub fn add(self, o: Pt) -> Pt {
        Pt::new(self.x + o.x, self.y + o.y)
    }
    pub fn sub(self, o: Pt) -> Pt {
        Pt::new(self.x - o.x, self.y - o.y)
    }
    pub fn mul(self, k: f64) -> Pt {
        Pt::new(self.x * k, self.y * k)
    }
    pub fn dot(self, o: Pt) -> f64 {
        self.x * o.x + self.y * o.y
    }
    pub fn cross(self, o: Pt) -> f64 {
        self.x * o.y - self.y * o.x
    }
    pub fn len2(self) -> f64 {
        self.dot(self)
    }
    pub fn len(self) -> f64 {
        self.len2().sqrt()
    }
    pub fn dist(self, o: Pt) -> f64 {
        self.sub(o).len()
    }
    pub fn lerp(self, o: Pt, t: f64) -> Pt {
        self.add(o.sub(self).mul(t))
    }
}

/// 阶数
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Deg {
    Line,
    Quad,
    Cube,
    Quart,
}

impl Deg {
    pub fn n(self) -> usize {
        match self {
            Deg::Line => 1,
            Deg::Quad => 2,
            Deg::Cube => 3,
            Deg::Quart => 4,
        }
    }
    pub fn n_ctrl(self) -> usize {
        self.n() + 1
    }
}

/// 贝塞尔曲线：控制点数 = 阶数 + 1，构造时校验
#[derive(Clone, Debug)]
pub struct Curve {
    pub deg: Deg,
    pub ctrl: Vec<Pt>,
}

impl Curve {
    /// 构造：控制点数量必须与阶数吻合，否则 None
    pub fn build(deg: Deg, ctrl: Vec<Pt>) -> Option<Curve> {
        if ctrl.len() != deg.n_ctrl() {
            return None;
        }
        Some(Curve { deg, ctrl })
    }

    /// Bernstein 多项式求值（显式形式）
    pub fn at(&self, t: f64) -> Pt {
        let n = self.deg.n();
        let mut acc = Pt::ZERO;
        for i in 0..=n {
            let b = bernstein(n, i, t);
            acc = acc.add(self.ctrl[i].mul(b));
        }
        acc
    }

    /// de Casteljau 递推求值（数值更稳）
    pub fn at_casteljau(&self, t: f64) -> Pt {
        let mut tmp: Vec<Pt> = self.ctrl.clone();
        let n = tmp.len();
        for r in 1..n {
            for i in 0..(n - r) {
                tmp[i] = tmp[i].lerp(tmp[i + 1], t);
            }
        }
        tmp[0]
    }

    /// 一阶导曲线（阶数减一：Cube→Quad→Line）
    pub fn derivative(&self) -> Option<Curve> {
        let n = self.deg.n();
        let d: Vec<Pt> = (0..n)
            .map(|i| self.ctrl[i + 1].sub(self.ctrl[i]).mul(n as f64))
            .collect();
        // 导数阶数 n-1：n=4→Cube，n=3→Quad，n=2→Line
        let deg = match n {
            4 => Deg::Cube,
            3 => Deg::Quad,
            2 => Deg::Line,
            _ => return None,
        };
        Curve::build(deg, d)
    }

    /// 二阶导
    pub fn second_derivative(&self) -> Option<Curve> {
        self.derivative().and_then(|c| c.derivative())
    }

    /// t 参数处的速度向量
    pub fn velocity(&self, t: f64) -> Option<Pt> {
        self.derivative().map(|d| d.at(t))
    }

    /// t 参数处的速率（标量）
    pub fn speed(&self, t: f64) -> Option<f64> {
        self.velocity(t).map(|v| v.len())
    }

    /// 分轴极值参数：解 (n-1) 次导数分量 = 0，用二分扫描
    pub fn axis_extrema(&self, axis: Axis) -> Vec<f64> {
        let d = match self.derivative() {
            Some(d) => d,
            None => return alloc::vec![0.0, 1.0],
        };
        let mut out = alloc::vec![0.0, 1.0];
        let steps = 64;
        let mut prev = axis.get(d.at(0.0));
        for k in 1..=steps {
            let t = k as f64 / steps as f64;
            let cur = axis.get(d.at(t));
            if (prev < 0.0 && cur > 0.0) || (prev > 0.0 && cur < 0.0) {
                // 二分精化
                let (mut lo, mut hi) = ((k - 1) as f64 / steps as f64, t);
                for _ in 0..40 {
                    let mid = (lo + hi) * 0.5;
                    let mv = axis.get(d.at(mid));
                    if (mv < 0.0) == (prev < 0.0) {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                out.push((lo + hi) * 0.5);
            }
            prev = cur;
        }
        out.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
        out.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        out
    }

    /// 全局包围盒（含端点与极值点）
    pub fn bbox(&self) -> (Pt, Pt) {
        let xs = self.axis_extrema(Axis::X);
        let ys = self.axis_extrema(Axis::Y);
        let mut min = Pt::new(f64::INFINITY, f64::INFINITY);
        let mut max = Pt::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
        for ts in [xs, ys] {
            for t in ts {
                let p = self.at(t);
                min.x = min.x.min(p.x);
                min.y = min.y.min(p.y);
                max.x = max.x.max(p.x);
                max.y = max.y.max(p.y);
            }
        }
        (min, max)
    }
}

/// 轴选择
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}

impl Axis {
    pub fn get(self, p: Pt) -> f64 {
        match self {
            Axis::X => p.x,
            Axis::Y => p.y,
        }
    }
}

/// Bernstein 基 B(n,i;t) = C(n,i) t^i (1-t)^(n-i)
pub fn bernstein(n: usize, i: usize, t: f64) -> f64 {
    let one = 1.0 - t;
    let mut b = binom(n, i) as f64;
    for _ in 0..i {
        b *= t;
    }
    for _ in 0..(n - i) {
        b *= one;
    }
    b
}

/// 二项式系数（小 n 直接乘除，避免溢出）
pub fn binom(n: usize, k: usize) -> usize {
    if k > n {
        return 0;
    }
    let k = k.min(n - k);
    let mut r = 1usize;
    for i in 0..k {
        r = r * (n - i) / (i + 1);
    }
    r
}

pub fn run_vec321_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec321");
    // 族一：点代数
    let a = Pt::new(3.0, 4.0);
    s.add("点-模长", (a.len() - 5.0).abs() < 1e-12, "3-4-5");
    s.add("点-距离", a.dist(Pt::ZERO) == a.len(), "dist==len@origin");
    let b = Pt::new(1.0, 0.0);
    s.add("点-叉积", a.cross(b) == 4.0, "cross");
    s.add("点-点积", a.dot(b) == 3.0, "dot");
    s.add("点-插值", a.lerp(b, 0.5).dist(Pt::new(2.0, 2.0)) < 1e-12, "lerp half");
    // 族二：阶数
    s.add("阶-控制点数", Deg::Cube.n_ctrl() == 4 && Deg::Quad.n_ctrl() == 3, "n+1");
    s.add("曲线-构造校验", Curve::build(Deg::Quad, alloc::vec![Pt::ZERO; 4]).is_none(), "len mismatch reject");
    // 族三：求值双路径一致
    let c = Curve::build(
        Deg::Cube,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(1.0, 2.0), Pt::new(3.0, 2.0), Pt::new(4.0, 0.0)],
    )
    .expect("cube");
    let mut max_diff = 0.0f64;
    let mut k = 0;
    while k <= 20 {
        let t = k as f64 / 20.0;
        let d = c.at(t).dist(c.at_casteljau(t));
        if d > max_diff {
            max_diff = d;
        }
        k += 1;
    }
    s.add("求值-双路径一致", max_diff < 1e-9, "bernstein vs casteljau");
    s.add("求值-端点", c.at(0.0) == c.ctrl[0] && c.at(1.0) == c.ctrl[3], "endpoints");
    s.add("求值-中点对称", c.at_casteljau(0.5).y == 1.5, "symmetric mid y");
    // 族四：导数
    let d1 = c.derivative().expect("d1");
    s.add("导数-阶数减一", d1.ctrl.len() == 3, "3 ctrl for cubic deriv");
    let expect0 = c.ctrl[1].sub(c.ctrl[0]).mul(3.0);
    s.add("导数-t0", d1.at(0.0).dist(expect0) < 1e-9, "deriv at 0");
    let v = c.velocity(0.5).expect("vel");
    s.add("速度-一致", v.dist(d1.at(0.5)) < 1e-12, "velocity==deriv.at");
    let sp = c.speed(0.5).expect("spd");
    s.add("速率-正", sp > 0.0 && (sp - v.len()).abs() < 1e-12, "speed");
    // 族五：极值与包围盒
    let ex = c.axis_extrema(Axis::Y);
    s.add("极值-含端点", ex[0] == 0.0 && *ex.last().expect("last") == 1.0, "0..1");
    let mut ymax = f64::NEG_INFINITY;
    for t in &ex {
        ymax = ymax.max(c.at(*t).y);
    }
    s.add("极值-覆盖峰", (ymax - 2.25).abs() < 1e-6, "y peak 2.25");
    let (mn, mx) = c.bbox();
    s.add("包围盒-谷", mn.y.abs() < 1e-9, "min y 0");
    s.add("包围盒-峰", (mx.y - 2.25).abs() < 1e-6, "max y 2.25");
    // 族六：Bernstein 与二项式
    s.add("基-端点", bernstein(3, 0, 0.0) == 1.0 && bernstein(3, 3, 1.0) == 1.0, "B(0)=B(1)");
    s.add("基-归一", {
        let mut sum = 0.0;
        for i in 0..=4 {
            sum += bernstein(4, i, 0.37);
        }
        (sum - 1.0).abs() < 1e-12
    }, "partition of unity");
    s.add("二项式-杨辉", binom(5, 2) == 10 && binom(4, 0) == 1 && binom(3, 4) == 0, "C(n,k)");
    // 族七：二次曲线专项
    let q = Curve::build(Deg::Quad, alloc::vec![Pt::new(0.0, 0.0), Pt::new(1.0, 2.0), Pt::new(2.0, 0.0)]).expect("quad");
    s.add("二次-峰", (q.at(0.5).y - 1.0).abs() < 1e-12, "quad mid y=1");
    s.add("二次-导线性", q.derivative().expect("qd").ctrl.len() == 2, "quad deriv 2 ctrl");
    s
}
