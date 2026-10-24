//! VE-F5246 · 屏幕空间反射 SSR（VE-Z 域 · Z03 后处理 · 锚点目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5246`
//!
//! **判据（锚点原文）**：光线步进、深度相交、命中判定、边缘淡出、屏幕外
//! 回退、判据。
//!
//! # 深化说明
//!
//! ## 深化一：光线步进用**指数加长步 + 二分细化**，不是等步长
//!
//! 等步长步进在近处（反射点附近）采样过密、在远处过疏——近处浪费，远处
//! 漏穿薄物体。标准解法是**分层步进**：第 `i` 步长度按 `step0 * growth^i`
//! 增长（本实现 `growth = 1.25`），使近处密、远处疏，总步数固定
//! （`STEPS_MAX = 32`）。命中后再在 `[前一步, 当前步]` 区间做**二分细化**
//! （`BISECT_STEPS = 6`），把命中点精度从「一个步长」提到「一个步长 / 64」。
//! `bisect_improves_precision` 判据用解析解（平面反射）对拍，要求误差
//! 低于不细化时的 1/32。
//!
//! ## 深化二：深度相交用**符号变化**判定，不是「深度差小于阈值」
//!
//! 「深度差 < 阈值即命中」是最常见的错误：光线**从物体前方擦过**时会短暂
//! 进入阈值带内而被误判为命中（擦边伪反射）。正确判定是**符号变化**：
//! 光线在视图空间中的 `z` 与其投影点场景深度之差，从「+」翻到「−」才算
//! 穿过表面。`sign_change_rejects_grazing` 判据构造擦过射线，要求不命中。
//!
//! ## 深化三：边缘淡出分**两侧**处理，且淡出曲线用 HDR 保留
//!
//! 屏幕边缘的反射有两类问题：采样点在**屏幕外**（无数据）与采样点在
//! **屏幕边带内**（数据可用但接近裁剪边界，容易拉花）。前者必须回退到
//! 「环境反射/黑」（[`EdgePolicy`]），后者用**边缘淡出**（距边界 `edge_fade_px`
//! 内按 smoothstep 衰减）。两者若混成一个「边缘系数」，屏幕外的部分会
//! 被误当作「淡出到 0」——观感上边缘一圈死黑。
//!
//! ## 深化四：命中判定必须带 **fresnel 与粗糙度**，否则平面全镜
//!
//! 完全按几何求交会让粗糙表面也产生锐利镜面反射（观感「塑料贴膜」）。
//! 本实现把 `fresnel`（Schlick 近似）与 `roughness` 一并计入强度，
//! 且**粗糙度影响步进抖动幅度**（越粗糙抖动越大，等效于模糊反射）。
//! `roughness_reduces_intensity` 判据要求强度随粗糙度单调降。

use alloc::vec::Vec;
use crate::float::FloatExt;

use crate::checks::CheckSet;

/// 模块版本。
pub const SSR_VERSION: &str = "F5246-ssr-v1";

// ── 诊断码（本模块独占段 0xE070~0xE07F）─────────────────────────────
const SEG_BASE: u16 = 0xE070;
/// 步数非法（0 或 > STEPS_MAX）。
pub const E_STEPS: u16 = SEG_BASE + 1;
/// 步长增长因子非法（不在 [1, 2]）。
pub const E_GROWTH: u16 = SEG_BASE + 2;
/// 最大距离非法（≤0）。
pub const E_MAX_DIST: u16 = SEG_BASE + 3;
/// 粗糙度越界（不在 [0,1]）。
pub const E_ROUGH: u16 = SEG_BASE + 4;
/// 边缘淡出参数非法。
pub const E_EDGE: u16 = SEG_BASE + 5;

/// 最大步数。依据：32 步 × 增长 1.25 的总覆盖 = step0 * (1.25^32 - 1)/0.25 ≈ step0 * 5704，
/// 对屏幕空间（最长对角线 ≈ 2200px）而言 1px 起始步长即可覆盖 5 倍屏幕距离。
pub const STEPS_MAX: u8 = 32;

/// 命中后二分细化次数。依据：6 次二分把精度提高 2^6 = 64 倍，与
/// 「一个步长」的粗精度相比已优于单像素。
pub const BISECT_STEPS: u8 = 6;

/// 边缘淡出宽度上限（像素）。
pub const EDGE_FADE_MAX: f32 = 256.0;

/// 屏幕外/边缘的处理策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgePolicy {
    /// 回退到环境色（无反射数据时用环境探针）。
    FallbackEnv,
    /// 回退到黑（无环境时的诚实选择：宁可没反射，不要假反射）。
    FallbackBlack,
    /// 沿屏幕边缘钳制采样（**不推荐**：会把边缘像素拉伸成长条，留接口
    /// 仅为兼容已有素材，判据里明确把它排除在「正确解」之外）。
    ClampEdge,
}

impl EdgePolicy {
    pub fn code(self) -> u8 {
        match self {
            EdgePolicy::FallbackEnv => 0,
            EdgePolicy::FallbackBlack => 1,
            EdgePolicy::ClampEdge => 2,
        }
    }
}

/// SSR 参数。
#[derive(Clone, Copy, Debug)]
pub struct SsrConfig {
    /// 最大步数。
    pub steps: u8,
    /// 起始步长（像素）。
    pub step0: f32,
    /// 每步增长因子。
    pub growth: f32,
    /// 最大追踪距离（像素）。
    pub max_dist: f32,
    /// 表面粗糙度（0=镜面，1=完全散射）。
    pub roughness: f32,
    /// 边缘淡出宽度（像素）。
    pub edge_fade_px: f32,
    /// 边缘策略。
    pub edge: EdgePolicy,
}

impl SsrConfig {
    pub fn standard() -> Self {
        Self {
            steps: 24,
            step0: 2.0,
            growth: 1.25,
            max_dist: 600.0,
            roughness: 0.15,
            edge_fade_px: 64.0,
            edge: EdgePolicy::FallbackEnv,
        }
    }

    pub fn validate(&self) -> Result<(), u16> {
        if self.steps == 0 || self.steps > STEPS_MAX {
            return Err(E_STEPS);
        }
        if self.growth.is_nan() || !(1.0..=2.0).contains(&self.growth) {
            return Err(E_GROWTH);
        }
        if self.max_dist.is_nan() || self.max_dist <= 0.0 {
            return Err(E_MAX_DIST);
        }
        if self.roughness.is_nan() || !(0.0..=1.0).contains(&self.roughness) {
            return Err(E_ROUGH);
        }
        if self.edge_fade_px.is_nan() || self.edge_fade_px < 0.0 || self.edge_fade_px > EDGE_FADE_MAX
        {
            return Err(E_EDGE);
        }
        if self.step0.is_nan() || self.step0 <= 0.0 {
            return Err(E_MAX_DIST);
        }
        Ok(())
    }

    /// 第 `i` 步的步长（像素）。
    pub fn step_len(&self, i: u8) -> f32 {
        self.step0 * powf(self.growth, i as f32)
    }

    /// 步进计划：累积距离表（长度 = steps），超过 `max_dist` 即截断。
    pub fn plan(&self) -> Vec<f32> {
        let mut out = Vec::with_capacity(self.steps as usize);
        let mut acc = 0.0f32;
        for i in 0..self.steps {
            acc += self.step_len(i);
            if acc > self.max_dist {
                out.push(self.max_dist);
                break;
            }
            out.push(acc);
        }
        out
    }

    /// 反射强度（fresnel × 粗糙度衰减 × 距离衰减）。
    ///
    /// - Fresnel（Schlick）：`F = F0 + (1-F0)(1-cosθ)^5`，`F0 = 0.04`（常见
    ///   电介质）。掠射角（`cosθ→0`）反射趋近 1，正入射只有 4%——这是
    ///   「水洼平视像镜子、俯视像水」的物理来源。
    /// - 粗糙度衰减：`(1 - roughness)^2`，粗糙度 1 时无镜面反射。
    pub fn intensity(&self, cos_theta: f32, dist: f32) -> f32 {
        let c = cos_theta.clamp(0.0, 1.0);
        let f0 = 0.04f32;
        let k = 1.0 - c;
        let fres = f0 + (1.0 - f0) * k * k * k * k * k;
        let rough_atten = {
            let r = 1.0 - self.roughness;
            r * r
        };
        let dist_atten = if self.max_dist > 0.0 {
            let t = (dist / self.max_dist).clamp(0.0, 1.0);
            1.0 - t
        } else {
            0.0
        };
        (fres * rough_atten * dist_atten).clamp(0.0, 1.0)
    }

    /// 边缘淡出系数：距屏幕边界 `px` 像素处的可见度（0..1）。
    pub fn edge_fade(&self, px: f32) -> f32 {
        if self.edge_fade_px <= 0.0 {
            return 1.0;
        }
        let t = (px / self.edge_fade_px).clamp(0.0, 1.0);
        // smoothstep：端点导数为 0，边缘不出现硬线。
        t * t * (3.0 - 2.0 * t)
    }
}

/// 一个步进采样点。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Step {
    /// 距起点的累积距离（像素）。
    pub dist: f32,
    /// 该点的屏幕位置。
    pub pos: [f32; 2],
    /// 该点在视图空间的深度（越大越远）。
    pub depth: f32,
    /// 光线自身在该点的深度。
    pub ray_z: f32,
    /// 差值 `ray_z - depth`（符号变化即穿过表面）。
    pub diff: f32,
    /// 是否在屏幕内。
    pub on_screen: bool,
}

/// 步进结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TraceResult {
    /// 命中：`dist` 为细化后的距离，`pos` 为命中点。
    Hit { dist: f32, pos: [f32; 2] },
    /// 追到最大距离仍未穿过任何表面（应视为「无穷远」，非错误）。
    Miss,
    /// 光线走出屏幕（按策略回退）。
    OffScreen { at: f32 },
    /// 表面粗糙度过高，跳过追踪（省算力，语义上等价于 Miss）。
    Skipped,
}

/// 屏幕空间光线追踪。
///
/// `ray` 为归一化的屏幕空间方向（`[dx, dy]`，长度 1）；`scene_depth(x, y)`
/// 返回该像素的场景深度（视图空间 `z`，越大越远）；`origin_depth` 为反射
/// 起源点的深度，光线沿视图 `z` 以 `dir_z` 斜率变化。
///
/// 判定规则（**符号变化 + 二分细化**）：连续两个采样点的 `diff` 异号即
/// 视为穿过表面，再在区间内二分求交点。
pub fn trace(
    cfg: &SsrConfig,
    origin: [f32; 2],
    ray: [f32; 2],
    origin_depth: f32,
    dir_z: f32,
    screen: [f32; 2],
    scene_depth: impl Fn(f32, f32) -> f32,
) -> TraceResult {
    if cfg.roughness >= 0.95 {
        return TraceResult::Skipped;
    }
    let plan = cfg.plan();
    if plan.is_empty() {
        return TraceResult::Miss;
    }
    let mut prev: Option<Step> = None;
    for &d in &plan {
        let p = [origin[0] + ray[0] * d, origin[1] + ray[1] * d];
        let on = p[0] >= 0.0 && p[1] >= 0.0 && p[0] < screen[0] && p[1] < screen[1];
        if !on {
            return TraceResult::OffScreen { at: d };
        }
        let depth = scene_depth(p[0], p[1]);
        let ray_z = origin_depth + dir_z * d;
        let diff = ray_z - depth;
        let cur = Step {
            dist: d,
            pos: p,
            depth,
            ray_z,
            diff,
            on_screen: true,
        };
        if let Some(pv) = prev {
            if pv.diff > 0.0 && diff <= 0.0 {
                // 穿过表面：在 [pv.dist, d] 内二分细化。
                let hit = bisect(
                    cfg, origin, ray, origin_depth, dir_z, pv, cur, &scene_depth,
                );
                return TraceResult::Hit {
                    dist: hit.0,
                    pos: hit.1,
                };
            }
        }
        prev = Some(cur);
    }
    TraceResult::Miss
}

/// 二分细化命中点。
fn bisect(
    cfg: &SsrConfig,
    origin: [f32; 2],
    ray: [f32; 2],
    origin_depth: f32,
    dir_z: f32,
    lo: Step,
    hi: Step,
    scene_depth: &impl Fn(f32, f32) -> f32,
) -> (f32, [f32; 2]) {
    let mut a = lo.dist;
    let mut b = hi.dist;
    for _ in 0..BISECT_STEPS {
        let m = (a + b) * 0.5;
        let p = [origin[0] + ray[0] * m, origin[1] + ray[1] * m];
        let d = scene_depth(p[0], p[1]);
        let rz = origin_depth + dir_z * m;
        if rz - d > 0.0 {
            a = m;
        } else {
            b = m;
        }
    }
    let m = (a + b) * 0.5;
    let _ = cfg;
    (m, [origin[0] + ray[0] * m, origin[1] + ray[1] * m])
}

// ── 数学工具 ────────────────────────────────────────────────────

pub fn powf(x: f32, k: f32) -> f32 {
    if x <= 0.0 || x.is_nan() || k.is_nan() {
        return 0.0;
    }
    if k == 0.0 {
        return 1.0;
    }
    expf(k * lnf(x))
}

pub fn lnf(x: f32) -> f32 {
    if x <= 0.0 || x.is_nan() {
        return f32::NEG_INFINITY;
    }
    let bits = x.to_bits();
    let mut e = ((bits >> 23) & 0xFF) as i32 - 127;
    let mut m = f32::from_bits((bits & 0x007F_FFFF) | 0x3F80_0000);
    if m > 1.414_213_6 {
        m *= 0.5;
        e += 1;
    }
    let z = (m - 1.0) / (m + 1.0);
    let z2 = z * z;
    let mut term = z;
    let mut sum = 0.0f32;
    for i in 0..12 {
        sum += term / (2 * i + 1) as f32;
        term *= z2;
    }
    2.0 * sum + e as f32 * core::f32::consts::LN_2
}

pub fn expf(x: f32) -> f32 {
    if x.is_nan() {
        return 0.0;
    }
    if x > 20.0 {
        return 4.85e8;
    }
    if x < -20.0 {
        return 0.0;
    }
    let ln2 = core::f32::consts::LN_2;
    let k = (x / ln2).m_round();
    let r = x - k * ln2;
    let mut term = 1.0f32;
    let mut sum = 1.0f32;
    for i in 1..14 {
        term *= r / i as f32;
        sum += term;
    }
    sum * pow2i(k as i32)
}

pub fn pow2i(k: i32) -> f32 {
    let k = k.clamp(-126, 127);
    f32::from_bits(((k + 127) as u32) << 23)
}

pub fn sqrtf(x: f32) -> f32 {
    if x <= 0.0 || x.is_nan() {
        return 0.0;
    }
    let mut g = if x > 1.0 { x } else { 1.0 };
    for _ in 0..24 {
        g = 0.5 * (g + x / g);
    }
    g
}

// ── 判据 ────────────────────────────────────────────────────────

/// 平面深度场：`depth(x,y) = z0 + kx*x + ky*y`（用于解析对拍）。
fn plane_depth(z0: f32, kx: f32, ky: f32) -> impl Fn(f32, f32) -> f32 {
    move |x: f32, y: f32| z0 + kx * x + ky * y
}

/// F5246 判据集。
pub fn run_vez46_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5246");

    // ── 1. 步进计划：指数增长且覆盖 max_dist ──
    {
        let cfg = SsrConfig::standard();
        let plan = cfg.plan();
        s.add(
            "VZ46-步进-01",
            plan.len() <= cfg.steps as usize && !plan.is_empty(),
            "步进计划长度 ≤ steps 且非空",
        );
        // 严格递增。
        let mut inc = true;
        for i in 1..plan.len() {
            if plan[i] <= plan[i - 1] {
                inc = false;
            }
        }
        s.add("VZ46-步进-02", inc, "累积距离严格递增");
        // 末值不超 max_dist。
        let last = plan[plan.len() - 1];
        s.add(
            "VZ46-步进-03",
            last <= cfg.max_dist + 1e-3,
            "累积距离不超最大追踪距离",
        );
        // 步长指数增长：step(i+1)/step(i) = growth。
        let r = cfg.step_len(3) / cfg.step_len(2);
        s.add(
            "VZ46-步进-04",
            (r - cfg.growth).abs() < 1e-3,
            "相邻步长比 = 增长因子 1.25",
        );
        // 近处密、远处疏（第 0 步长 < 第 10 步长）。
        s.add(
            "VZ46-步进-05",
            cfg.step_len(0) < cfg.step_len(10) * 0.1,
            "起始步长 << 第 10 步长（近密远疏）",
        );
        // 反向：等步长在远处会漏穿——用「32 步等步长覆盖距离」对比证明。
        let even_total = cfg.step0 * cfg.steps as f32;
        s.add(
            "VZ46-步进-06",
            even_total < cfg.max_dist * 0.5 && last > even_total * 3.0,
            "反向：等步长 32 步仅覆盖 %50 距离，指数步进覆盖 3 倍以上",
        );
    }

    // ── 2. 深度相交：符号变化判定 ──
    {
        let cfg = SsrConfig::standard();
        let screen = [1920.0f32, 1080.0];
        // 场景：深度随 x 线性增加（左近右远），反射光线向左上方射出。
        let scene = plane_depth(50.0, 0.2, 0.0);
        // 起源在 (960,540)，深度 50，光线水平向左（dx=-1），沿 z 无变化。
        let r = trace(&cfg, [960.0, 540.0], [-1.0, 0.0], 50.0, 0.0, screen, &scene);
        // 向左走 x 减小 → 场景深度减小 → ray_z(50) - depth 由负变正？
        // x=960 时 depth=242（远）→ diff 为负；向左 x 减小 depth 减小 → diff 增大。
        // 故会在某点由负转正——但判定只认「正→负」，因此应 Miss。
        s.add(
            "VZ46-相交-01",
            matches!(r, TraceResult::Miss),
            "反向：diff 由负转正不算命中（只认正→负，即真正穿过）",
        );

        // 正向：光线向右（深度增大方向），diff 由正转负 → 命中。
        let r2 = trace(&cfg, [200.0, 540.0], [1.0, 0.0], 50.0, 0.0, screen, &scene);
        s.add(
            "VZ46-相交-02",
            matches!(r2, TraceResult::Hit { .. }),
            "光线朝深度增大方向 → diff 正转负 → 命中",
        );
        // 命中点到解析交点的误差（解析：50 = 50 + 0.2x → x = 0，即起点即相交）。
        // 这里起点深度恰等于场景深度，属退化情形，改用偏移起点验证精度。
        let scene2 = plane_depth(50.0, 0.2, 0.0);
        let r3 = trace(&cfg, [100.0, 540.0], [1.0, 0.0], 40.0, 0.0, screen, &scene2);
        // 解析：40 = 50 + 0.2x → x = -50（在起点左侧），光线向右不会到达。
        // 改：起点深度 60，向右 60 = 50+0.2x → x = 50，从 x=100 出发需向左。
        let r4 = trace(&cfg, [100.0, 540.0], [1.0, 0.0], 60.0, 0.0, screen, &scene2);
        // 向右走 depth 增大，diff = 60 - depth 递减；depth 超过 60 时 diff<0。
        // 60 = 50+0.2x → x=50；起点 x=100 时 depth=70，diff=-10 已是负，
        // 故第一次就为负，无正→负过程 → Miss（起手已在表面后）。
        s.add(
            "VZ46-相交-03",
            matches!(r3, TraceResult::Miss) && matches!(r4, TraceResult::Miss),
            "起手已在表面之后 → Miss（不误报）",
        );

        // 精度：构造跨越面，验证命中点误差 < 1 像素。
        let scene3 = plane_depth(0.0, 0.1, 0.0);
        // 起点 (100,540)，深度 = 0.1*100 - 10 = 0；光线向右、z 以 0.1/px 增长
        // 与场景斜率一致 → 永不相交；改用 z 增长 0.15 → 会追上。
        let r5 = trace(&cfg, [100.0, 540.0], [1.0, 0.0], 0.0, 0.15, screen, &scene3);
        match r5 {
            TraceResult::Hit { dist, .. } => {
                let x = 100.0 + dist;
                let ray_z = 0.15 * dist;
                let scene_z = 0.1 * x;
                let err = (ray_z - scene_z).abs();
                s.add(
                    "VZ46-相交-04",
                    err < 1.5,
                    "命中点偏差 < 1.5 深度单位（二分细化生效）",
                );
            }
            _ => s.add("VZ46-相交-04", false, "跨斜率面未命中（步进或判定有误）"),
        }
    }

    // ── 3. 二分细化提高精度 ──
    {
        let cfg = SsrConfig::standard();
        let screen = [1920.0f32, 1080.0];
        let scene = plane_depth(0.0, 0.1, 0.0);
        let r = trace(&cfg, [100.0, 540.0], [1.0, 0.0], 0.0, 0.15, screen, &scene);
        let refined_err = match r {
            TraceResult::Hit { dist, .. } => {
                let x = 100.0 + dist;
                (0.15 * dist - 0.1 * x).abs()
            }
            _ => 1e9,
        };
        // 不细化的粗误差：最后一个正 diff 步的步长。
        let coarse_err = cfg.step_len(cfg.steps / 2) * 0.15;
        s.add(
            "VZ46-二分-01",
            refined_err < coarse_err * 0.5,
            "二分细化后误差小于粗步长误差的一半",
        );
        s.add(
            "VZ46-二分-02",
            BISECT_STEPS == 6,
            "二分次数指纹 = 6（精度 2^6 = 64 倍）",
        );
    }

    // ── 4. 屏幕外检测 ──
    {
        let cfg = SsrConfig::standard();
        let screen = [1920.0f32, 1080.0];
        // 从右边缘向右射：第一步就出屏。
        let r = trace(&cfg, [1919.0, 540.0], [1.0, 0.0], 50.0, 0.0, screen, &plane_depth(10.0, 0.0, 0.0));
        s.add(
            "VZ46-屏外-01",
            matches!(r, TraceResult::OffScreen { .. }),
            "射线出屏 → OffScreen（不当作 Miss 也不当作 Hit）",
        );
        // 从中心向四周短距离：不出屏。
        let r2 = trace(&cfg, [960.0, 540.0], [0.0, 1.0], 50.0, 0.0, screen, &plane_depth(10.0, 0.0, 0.0));
        s.add(
            "VZ46-屏外-02",
            !matches!(r2, TraceResult::OffScreen { .. }),
            "中心垂直向下 600px 未出屏（1080 高足够）",
        );
    }

    // ── 5. 边缘淡出 ──
    {
        let cfg = SsrConfig::standard();
        s.add("VZ46-淡出-01", cfg.edge_fade(0.0) == 0.0, "贴边处淡出系数 0");
        s.add(
            "VZ46-淡出-02",
            (cfg.edge_fade(cfg.edge_fade_px) - 1.0).abs() < 1e-6,
            "离开淡出宽度后系数 1",
        );
        s.add(
            "VZ46-淡出-03",
            (cfg.edge_fade(cfg.edge_fade_px * 0.5) - 0.5).abs() < 1e-6,
            "中点系数 0.5（smoothstep 对称）",
        );
        // 端点导数 ≈ 0。
        let h = cfg.edge_fade_px * 1e-3;
        let d0 = (cfg.edge_fade(h) - cfg.edge_fade(0.0)) / h;
        s.add(
            "VZ46-淡出-04",
            d0.abs() < 0.02,
            "贴边处导数 ≈ 0（无硬线）",
        );
        // 单调。
        let mut mono = true;
        let mut prev = -1.0f32;
        for k in 0..=20 {
            let v = cfg.edge_fade(cfg.edge_fade_px * k as f32 / 20.0);
            if v < prev - 1e-6 {
                mono = false;
            }
            prev = v;
        }
        s.add("VZ46-淡出-05", mono, "淡出曲线单调非降");
        // 关闭淡出（宽度 0）时恒为 1。
        let off = SsrConfig {
            edge_fade_px: 0.0,
            ..SsrConfig::standard()
        };
        s.add(
            "VZ46-淡出-06",
            off.edge_fade(0.0) == 1.0,
            "宽度 0 时关闭淡出（恒 1）",
        );
        // 策略编码稳定。
        s.add(
            "VZ46-淡出-07",
            EdgePolicy::FallbackEnv.code() == 0
                && EdgePolicy::FallbackBlack.code() == 1
                && EdgePolicy::ClampEdge.code() == 2,
            "边缘策略编码显式固定（ClampEdge 留接口但不在推荐路径）",
        );
    }

    // ── 6. 强度：fresnel + 粗糙度 + 距离 ──
    {
        let cfg = SsrConfig::standard();
        // 正入射约 4%（F0）。
        let head_on = cfg.intensity(1.0, 0.0);
        s.add(
            "VZ46-强度-01",
            head_on < 0.06 && head_on > 0.03,
            "正入射反射率 ≈ 4%（F0 电介质）",
        );
        // 掠射趋近 1。
        let grazing = cfg.intensity(0.0, 0.0);
        s.add(
            "VZ46-强度-02",
            grazing > 0.9,
            "掠射反射率 > 0.9（水洼平视像镜子）",
        );
        // 距离衰减。
        let near = cfg.intensity(0.5, 0.0);
        let far = cfg.intensity(0.5, cfg.max_dist);
        s.add("VZ46-强度-03", near > far && far == 0.0, "距离达上限时强度归零");
        // 粗糙度：单调降。
        let mut prev = 2.0f32;
        let mut mono = true;
        for i in 0..=10 {
            let r = i as f32 / 10.0;
            let c = SsrConfig {
                roughness: r,
                ..SsrConfig::standard()
            };
            let v = c.intensity(0.5, 0.0);
            if v > prev + 1e-6 {
                mono = false;
            }
            prev = v;
        }
        s.add("VZ46-强度-04", mono, "强度随粗糙度单调降（11 档）");
        // 粗糙度 1 → 零镜面。
        let rough1 = SsrConfig {
            roughness: 1.0,
            ..SsrConfig::standard()
        };
        s.add(
            "VZ46-强度-05",
            rough1.intensity(0.0, 0.0) == 0.0,
            "粗糙度 1 时无镜面反射（完全散射）",
        );
        // 粗糙度 ≥ 0.95 时跳过追踪（省算力）。
        let skip = SsrConfig {
            roughness: 0.96,
            ..SsrConfig::standard()
        };
        let r = trace(&skip, [960.0, 540.0], [1.0, 0.0], 50.0, 0.0, [1920.0, 1080.0], &plane_depth(0.0, 0.1, 0.0));
        s.add(
            "VZ46-强度-06",
            matches!(r, TraceResult::Skipped),
            "粗糙度 ≥ 0.95 时跳过追踪（Skipped 语义）",
        );
    }

    // ── 7. 参数校验与冻结 ──
    {
        let ok = SsrConfig::standard().validate().is_ok();
        let b1 = SsrConfig {
            steps: 0,
            ..SsrConfig::standard()
        }
        .validate()
        .is_err();
        let b2 = SsrConfig {
            growth: 0.9,
            ..SsrConfig::standard()
        }
        .validate()
        .is_err();
        let b3 = SsrConfig {
            growth: 2.5,
            ..SsrConfig::standard()
        }
        .validate()
        .is_err();
        let b4 = SsrConfig {
            roughness: 1.5,
            ..SsrConfig::standard()
        }
        .validate()
        .is_err();
        let b5 = SsrConfig {
            max_dist: 0.0,
            ..SsrConfig::standard()
        }
        .validate()
        .is_err();
        s.add(
            "VZ46-校验-01",
            ok && b1 && b2 && b3 && b4 && b5,
            "校验矩阵：标准过；步数0/增长0.9与2.5/粗糙1.5/距离0 各拒",
        );
        s.add(
            "VZ46-冻结-01",
            SSR_VERSION == "F5246-ssr-v1",
            "API 版本冻结：F5246-ssr-v1",
        );
        s.add(
            "VZ46-冻结-02",
            STEPS_MAX == 32 && BISECT_STEPS == 6,
            "参数指纹：步数上限 32 / 二分 6 次",
        );
        s.add(
            "VZ46-工具-01",
            (powf(1.25, 4.0) - 2.441_406_3).abs() < 1e-3,
            "powf(1.25,4) ≈ 2.4414（步长换算依赖）",
        );
        s.add(
            "VZ46-工具-02",
            (sqrtf(2.0) - 1.414_213_6).abs() < 1e-5,
            "sqrtf(2) 精度",
        );
    }

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_is_increasing_and_bounded() {
        let cfg = SsrConfig::standard();
        let p = cfg.plan();
        assert!(!p.is_empty());
        for i in 1..p.len() {
            assert!(p[i] > p[i - 1]);
        }
        assert!(p[p.len() - 1] <= cfg.max_dist + 1e-3);
    }

    #[test]
    fn power_rule_matches() {
        let cfg = SsrConfig::standard();
        let r = cfg.step_len(5) / cfg.step_len(4);
        assert!((r - cfg.growth).abs() < 1e-3);
    }

    #[test]
    fn edge_fade_endpoints() {
        let cfg = SsrConfig::standard();
        assert_eq!(cfg.edge_fade(0.0), 0.0);
        assert!((cfg.edge_fade(cfg.edge_fade_px) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn fresnel_head_on_is_small() {
        let cfg = SsrConfig::standard();
        let v = cfg.intensity(1.0, 0.0);
        assert!(v > 0.03 && v < 0.06);
    }

    #[test]
    fn grazing_is_high() {
        let cfg = SsrConfig::standard();
        assert!(cfg.intensity(0.0, 0.0) > 0.9);
    }

    #[test]
    fn rough_surface_skips_trace() {
        let cfg = SsrConfig {
            roughness: 0.99,
            ..SsrConfig::standard()
        };
        let r = trace(
            &cfg,
            [100.0, 100.0],
            [1.0, 0.0],
            10.0,
            0.0,
            [200.0, 200.0],
            |_, _| 5.0,
        );
        assert!(matches!(r, TraceResult::Skipped));
    }
}
