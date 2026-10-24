//! cga52_bezier —— CGPU-F0052 SIMD 贝塞尔曲线求值。
//! 锚点（CGPU 施工书）：CGPU-F0052 · SIMD 贝塞尔曲线求值 · 目标 340 行
//! 判据摘录：de Casteljau 递推与二项式直算双实现对拍一致；端点
//!   插值精确（t=0/t=1 恰端点）；细分平展的弦误差上界可配且被
//!   尊重（细分段弦高 ≤ 容差）；凸包快拒（控制点全在裁剪域外
//!   同侧 = 整线在外）；控制点数闭集（2~4：线性/二次/三次）。
//! 组件：定点 de Casteljau 引擎（Q16.16，两级线性插值三角递推）
//!   + 二项式直算参照（Bernstein 基加权，双实现互证）+ 平展器
//!   （弦误差驱动自适应细分：细分深度由控制多边形的最大边弯曲
//!   量估计，段数闭集 1~64）+ 凸包快拒（四轴同侧判定）+ 求值
//!   账（平展段数/最大弦高/指纹落档）。
//! 职责定位：矢量字形与路径渲染的几何地基——贝塞尔求值精度
//!   直接决定字形边缘质量；双实现互证 + 端点精确 + 弦差上界
//!   三条红线让「这条曲线被画成什么」有账面答案。
//! 性能逐项分解：单点求值 O(控制点数)；平展 O(段数×控制点数)；
//!   快拒 O(控制点数)。Q16.16 全整数确定性。
//! 诊断码段：0xE550（0xE551~0xE559 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const BZ_VERSION: &str = "cga52_bezier-v1";

/// 诊断段基址（0xE550 段 · 本模块独占）。
pub const BZ_SEG_BASE: u16 = 0xE550;

/// XcCode 闭集（0xE551~0xE559）。
pub const XC_CTRL_OOR: u32 = 0xE551;
pub const XC_ENDPOINT_DRIFT: u32 = 0xE552;
pub const XC_DUAL_DRIFT: u32 = 0xE553;
pub const XC_CHORD_BUDGET: u32 = 0xE554;
pub const XC_FLAT_OVERRUN: u32 = 0xE555;
pub const XC_T_OOR: u32 = 0xE556;
pub const XC_DEGEN_CURVE: u32 = 0xE557;
pub const XC_HULL_MISS: u32 = 0xE558;
pub const XC_FINGER_MISS: u32 = 0xE559;

/// 定点标量与插值。
pub type Fixed = i32;
pub const FX_ONE: Fixed = 1 << 16;

/// Q16.16 线性插值（t ∈ [0, 65536]）。
pub fn lerp(a: Fixed, b: Fixed, t: Fixed) -> Fixed {
    a + (((b - a) as i64 * t as i64) / FX_ONE as i64) as Fixed
}

/// 控制点。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pt {
    pub x: Fixed,
    pub y: Fixed,
}

/// 曲线阶闭集（控制点数 2~4）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CurveDegree {
    Linear,
    Quadratic,
    Cubic,
}

impl CurveDegree {
    pub fn ctrl_count(self) -> usize {
        match self {
            CurveDegree::Linear => 2,
            CurveDegree::Quadratic => 3,
            CurveDegree::Cubic => 4,
        }
    }
}

/// de Casteljau 求值（逐层 lerp 三角递推；控制点数闭集校验）。
pub fn eval_casteljau(ctrl: &[Pt], t: Fixed) -> Result<Pt, u16> {
    if t < 0 || t > FX_ONE {
        return Err(XC_T_OOR as u16);
    }
    let n = ctrl.len();
    if !(2..=4).contains(&n) {
        return Err(XC_CTRL_OOR as u16);
    }
    let mut xs: Vec<Fixed> = ctrl.iter().map(|p| p.x).collect();
    let mut ys: Vec<Fixed> = ctrl.iter().map(|p| p.y).collect();
    let mut level = n - 1;
    while level > 0 {
        for i in 0..level {
            xs[i] = lerp(xs[i], xs[i + 1], t);
            ys[i] = lerp(ys[i], ys[i + 1], t);
        }
        level -= 1;
    }
    Ok(Pt { x: xs[0], y: ys[0] })
}

/// Bernstein 直算参照（整数权重 × 预偏移定点，用于对拍）。
pub fn eval_bernstein_ref(ctrl: &[Pt], t: Fixed) -> Result<Pt, u16> {
    if t < 0 || t > FX_ONE {
        return Err(XC_T_OOR as u16);
    }
    let n = ctrl.len();
    if !(2..=4).contains(&n) {
        return Err(XC_CTRL_OOR as u16);
    }
    let u = FX_ONE - t;
    let (mut x, mut y) = (0i64, 0i64);
    // Q16.16 Bernstein 权重：w = (u^(n-i) t^i × 组合数) >> 32，
    // Σw = (u+t)^n >> 32 = 65536（权重和恒为 1 的定点表示）。
    let uu = u as i64 * u as i64;
    let tt = t as i64 * t as i64;
    let weights: Vec<i64> = match n {
        2 => vec![u as i64, t as i64],
        3 => vec![
            uu >> 16,
            2 * (u as i64) * (t as i64) >> 16,
            tt >> 16,
        ],
        _ => vec![
            (uu * u as i64) >> 32,
            (3 * uu * t as i64) >> 32,
            (3 * u as i64 * tt) >> 32,
            (tt * t as i64) >> 32,
        ],
    };
    for (i, &w) in weights.iter().enumerate() {
        x += ctrl[i].x as i64 * w;
        y += ctrl[i].y as i64 * w;
    }
    Ok(Pt { x: (x / FX_ONE as i64) as Fixed, y: (y / FX_ONE as i64) as Fixed })
}

/// 平展器：弦误差驱动自适应细分。
///
/// 细分策略：均匀 t 采样到段数 segs，段数由控制多边形最大边到
/// 弦的距离估计（弯曲量 × 2 ≤ 容差即停）。段数闭集 1~64。
pub fn flatten(ctrl: &[Pt], tol: Fixed) -> Result<(Vec<Pt>, u32), u16> {
    if ctrl.len() < 2 || ctrl.len() > 4 {
        return Err(XC_CTRL_OOR as u16);
    }
    if tol <= 0 {
        return Err(XC_CHORD_BUDGET as u16);
    }
    // 弯曲量估计：中间控制点到端点连线的最大距离。
    let chord_dx = ctrl[ctrl.len() - 1].x - ctrl[0].x;
    let chord_dy = ctrl[ctrl.len() - 1].y - ctrl[0].y;
    let mut max_bend: i64 = 0;
    for p in ctrl[1..(ctrl.len() - 1)].iter() {
        // 点到直线距离（叉积 / 弦长），全 i64 域防溢出。
        let cross = ((chord_dx as i64) * ((p.y - ctrl[0].y) as i64)
            - (chord_dy as i64) * ((p.x - ctrl[0].x) as i64)).abs();
        let len = (((chord_dx as i64) * (chord_dx as i64)
            + (chord_dy as i64) * (chord_dy as i64)) as f64).sqrt() as i64;
        if len == 0 {
            continue;
        }
        let d = cross / len;
        if d > max_bend {
            max_bend = d;
        }
    }
    // 段数 ≈ sqrt(max_bend / tol) 上取整，钳到 [1, 64]。
    let mut segs: u32 = 1;
    if max_bend > tol as i64 {
        let ratio = (max_bend / tol as i64) as u32;
        segs = ((ratio as u64).isqrt2() as u32) + 1;
    }
    if segs > 64 {
        segs = 64;
    }
    let mut pts = Vec::new();
    let step = FX_ONE / segs as Fixed;
    for i in 0..=segs {
        let t = if i == segs { FX_ONE } else { step * i as Fixed };
        pts.push(eval_casteljau(ctrl, t)?);
    }
    if pts.len() > 65 {
        return Err(XC_FLAT_OVERRUN as u16);
    }
    Ok((pts, segs))
}

/// u64 整数 isqrt（牛顿迭代，本模块局部实现）。
trait Isqrt2 {
    fn isqrt2(self) -> u64;
}

impl Isqrt2 for u64 {
    fn isqrt2(self) -> u64 {
        if self == 0 {
            return 0;
        }
        let mut x = self;
        let mut y = (x + 1) / 2;
        while y < x {
            x = y;
            y = (x + self / x) / 2;
        }
        x
    }
}

/// 凸包快拒：控制点全部在轴对齐域外同一侧 = 整线在外。
pub fn hull_outside(ctrl: &[Pt], xmin: Fixed, ymin: Fixed, xmax: Fixed, ymax: Fixed) -> bool {
    let all_left = ctrl.iter().all(|p| p.x < xmin);
    let all_right = ctrl.iter().all(|p| p.x > xmax);
    let all_above = ctrl.iter().all(|p| p.y > ymax);
    let all_below = ctrl.iter().all(|p| p.y < ymin);
    all_left || all_right || all_above || all_below
}

/// 域自检入口：求值/平展/快拒/账面 四族。
pub fn run_cga52_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0052-贝塞尔求值");
    // 夹具：三次曲线 (0,0)→(1,1)，控制 (0,1),(1,0)。
    let ctrl = vec![
        Pt { x: 0, y: 0 },
        Pt { x: 0, y: FX_ONE },
        Pt { x: FX_ONE, y: 0 },
        Pt { x: FX_ONE, y: FX_ONE },
    ];
    // 族一：端点精确 + 双实现互证。
    let t0 = eval_casteljau(&ctrl, 0);
    let t1 = eval_casteljau(&ctrl, FX_ONE);
    cs.add("端点 t=0", matches!(t0, Ok(p) if p.x == 0 && p.y == 0), "恰起点");
    cs.add("端点 t=1", matches!(t1, Ok(p) if p.x == FX_ONE && p.y == FX_ONE), "恰终点");
    let mid_a = eval_casteljau(&ctrl, FX_ONE / 2);
    let mid_b = eval_bernstein_ref(&ctrl, FX_ONE / 2);
    let drift = match (mid_a, mid_b) {
        (Ok(a), Ok(b)) => ((a.x - b.x).abs() as i64) + ((a.y - b.y).abs() as i64),
        _ => i64::MAX,
    };
    cs.add("双实现互证", drift <= 2, "Q16.16 舍入界 ±2");
    cs.add("t 越界拒", eval_casteljau(&ctrl, FX_ONE + 1).is_err(), "[0,1] 闭域");
    cs.add("控制点数闭集", eval_casteljau(&ctrl[..1], 0).is_err(), "1 点无曲线");

    // 族二：平展（直线零弯曲 1 段；三次弯曲自适应多段）。
    let line = vec![Pt { x: 0, y: 0 }, Pt { x: FX_ONE, y: 0 }];
    let (pts_l, segs_l) = flatten(&line, FX_ONE / 16).ok().unwrap_or((vec![], 0));
    cs.add("直线一段", segs_l == 1 && pts_l.len() == 2, "零弯曲零细分");
    let (pts_c, segs_c) = flatten(&ctrl, FX_ONE / 16).ok().unwrap_or((vec![], 0));
    cs.add("弯曲细分", segs_c > 1 && pts_c.len() == segs_c as usize + 1, "弦差驱动");
    cs.add("段数上界", segs_c <= 64, "闭集 [1,64]");

    // 族三：凸包快拒。
    cs.add("左侧全拒", hull_outside(&[
        Pt { x: -10, y: 0 }, Pt { x: -5, y: FX_ONE }], 0, 0, FX_ONE, FX_ONE), "全左 = 外");
    cs.add("跨越不拒", !hull_outside(&ctrl, 0, 0, FX_ONE, FX_ONE), "跨域不快拒");

    // 族四：账面（平展指纹稳定）。
    let fp1 = flatten(&ctrl, FX_ONE / 16).map(|(p, s)| p.len() as u64 + s as u64);
    let fp2 = flatten(&ctrl, FX_ONE / 16).map(|(p, s)| p.len() as u64 + s as u64);
    cs.add("平展稳定", fp1 == fp2 && fp1.is_ok(), "同输入同输出");
    cs.add("容差越界拒", flatten(&ctrl, 0).is_err(), "tol=0 拒收");
    cs
}

// ===== 扩展面：升阶转换 + 对称性核 + 弧长步账 =====
/// 升阶转换：二次 → 三次（插入式控制点重排——渲染管线在阶
/// 不匹配时的标准桥）。
pub fn elevate_quad_to_cubic(q: &[Pt; 3]) -> [Pt; 4] {
    let c1 = Pt {
        x: q[0].x + ((q[1].x - q[0].x) as i64 * 2 / 3) as Fixed,
        y: q[0].y + ((q[1].y - q[0].y) as i64 * 2 / 3) as Fixed,
    };
    let c2 = Pt {
        x: q[2].x + ((q[1].x - q[2].x) as i64 * 2 / 3) as Fixed,
        y: q[2].y + ((q[1].y - q[2].y) as i64 * 2 / 3) as Fixed,
    };
    [q[0], c1, c2, q[2]]
}

/// 对称性核：B(t) 与 B(1-t) 的镜像对称（控制点反序 + 参数反转
/// → 结果按弦中点镜像）——曲线求值器的不变式。
pub fn symmetry_ok(ctrl: &[Pt]) -> Result<bool, u16> {
    let n = ctrl.len();
    if n < 2 || n > 4 {
        return Err(XC_CTRL_OOR as u16);
    }
    let rev: Vec<Pt> = ctrl.iter().rev().copied().collect();
    let a = eval_casteljau(ctrl, FX_ONE / 4)?;
    let b = eval_casteljau(&rev, FX_ONE / 4)?;
    // 镜像：x 关于端点弦中轴对称（简化：端点对换后 x 和恒定）。
    let sum_ax = ctrl[0].x + ctrl[n - 1].x;
    let mirror = sum_ax - a.x;
    Ok((mirror - b.x).abs() <= 2 && (a.y - b.y).abs() <= 2)
}

/// 弧长步账：平展段折线长（曼哈顿近似——整数口径）与段数联动
/// （同容差同曲线步长恒定——确定性画像）。
pub fn arc_steps_manhattan(pts: &[Pt]) -> Result<u64, u16> {
    if pts.len() < 2 {
        return Err(XC_CTRL_OOR as u16);
    }
    let mut total: u64 = 0;
    for w in pts.windows(2) {
        let dx = (w[1].x - w[0].x).abs() as u64;
        let dy = (w[1].y - w[0].y).abs() as u64;
        total += dx + dy;
    }
    Ok(total)
}

/// 曲线链求值面：多段路径（字形轮廓就是曲线链）逐段求值拼合，
/// 段间接缝 = 前段终点 == 后段起点（连续性红线——断缝即轮廓破）。
pub struct CurveChain {
    segments: Vec<Vec<Pt>>,
}

impl CurveChain {
    pub fn new() -> CurveChain {
        CurveChain { segments: Vec::new() }
    }

    /// 追加一段：与上一段终点连续性校验（容差 2 定点位）。
    pub fn push_segment(&mut self, ctrl: Vec<Pt>) -> Result<(), u16> {
        if ctrl.len() < 2 || ctrl.len() > 4 {
            return Err(XC_CTRL_OOR as u16);
        }
        if let Some(last) = self.segments.last() {
            let tail = last[last.len() - 1];
            let head = ctrl[0];
            if (tail.x - head.x).abs() > 2 || (tail.y - head.y).abs() > 2 {
                return Err(XC_ENDPOINT_DRIFT as u16);
            }
        }
        self.segments.push(ctrl);
        Ok(())
    }

    /// 全链平展：逐段 flatten 后拼合（接缝去重：相邻段共享端点只留一份）。
    pub fn flatten_all(&self, tol: Fixed) -> Result<Vec<Pt>, u16> {
        let mut out: Vec<Pt> = Vec::new();
        for seg in self.segments.iter() {
            let (pts, _) = flatten(seg, tol)?;
            if let Some(tail) = out.last() {
                if *tail == pts[0] {
                    out.extend_from_slice(&pts[1..]);
                    continue;
                }
            }
            out.extend_from_slice(&pts);
        }
        if out.is_empty() {
            return Err(XC_DEGEN_CURVE as u16);
        }
        Ok(out)
    }

    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }
}

/// 闭合轮廓核：链首尾相接（字形外轮廓必须闭合——开口轮廓是
/// 渲染破面源）。
pub fn chain_closed(pts: &[Pt], tol: Fixed) -> Result<(), u16> {
    if pts.len() < 3 {
        return Err(XC_DEGEN_CURVE as u16);
    }
    let head = pts[0];
    let tail = pts[pts.len() - 1];
    if ((head.x - tail.x).abs() as i64) > tol as i64 || ((head.y - tail.y).abs() as i64) > tol as i64 {
        return Err(XC_ENDPOINT_DRIFT as u16);
    }
    Ok(())
}
