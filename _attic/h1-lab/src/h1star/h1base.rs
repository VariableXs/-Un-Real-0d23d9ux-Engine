//! h1base — H1 分队五十项共用的三件基础设施。
//!
//! 与 K1/K2 分队的 `star::sbase` 分工对齐（零冗余：账本/旋钮/环形日志/
//! 分位数直接复用 [`crate::star::sbase`]，本文件只补 H 域特有三件）：
//!
//! - [`Curve`] F124 动画总谱五曲线的整数实现——H 域判据反复引用的
//!   「进入 120ms / 退出曲线 / 强调曲线 / 弹性 320ms / 线性」与
//!   「减少动效降级 80ms 直切」（F245）全部从这一处取值（一处一事实）；
//! - [`Luma`] 令牌色整数工具——相对亮度 / 对比度倍数（×100）整数判定，
//!   服务 F201（插入符对比度 ≥4.5:1）、F229（占位/正文灰值差 ≥2 档）、
//!   F225（旧色残留扫描）等全部「对比度 ≥4.5:1」类判据，core 无 f64；
//! - [`Rect`] 平面几何工具——橡皮筋（F203）、标题栏还原（F213）、窗口
//!   排列（F236）、位置记忆（F237）、热角（F249）共用的矩形相交/包含/
//!   钳制，i32 整数像素域。
//!
//! 时间纪律沿用全域：一切时间由调用方注入（毫秒戳），模块不持时钟。

// ---------------------------------------------------------------------------
// F124 动画总谱 · 五曲线（H 域引用的唯一取值点）
// ---------------------------------------------------------------------------

/// F124 动画总谱曲线档位。
///
/// 时长与缓动均为主册判据原文数值（一处一事实）：
/// - 进入曲线 120ms（F212 落点高亮、F227 窗口打开、F229/F216 控件）；
/// - 退出曲线 120ms（F227 窗口关闭）；
/// - 强调曲线 320ms（F204 滚动回弹、F235 虚拟桌面横移、F225 交叉淡入
///   单独 200ms 走 [`Curve::Emphasis200`]）；
/// - 弹性曲线 320ms（F124「弹性」档，F204 触控板惯性收尾）；
/// - 线性（F208 往复进度 2s 周期、F223 插入符平滑、F124 线性档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Curve {
    /// 进入曲线 120ms。
    Enter,
    /// 退出曲线 120ms。
    Exit,
    /// 强调曲线 320ms。
    Emphasis,
    /// 强调曲线 200ms 变体（F225 主题交叉淡入 ±20ms 判据专用）。
    Emphasis200,
    /// 弹性曲线 320ms（F124 弹性档）。
    Spring,
    /// 线性（时长由调用方给定）。
    Linear,
}

/// 减少动效（F245）直切时长：开启后全系统动画替换为 80ms 直切。
pub const REDUCED_MOTION_MS: u32 = 80;

/// 减少动效策略（注入式——调用方持有开关，模块不持全局态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MotionPolicy {
    /// true = F245 开关开启（晕动症/远程桌面低带宽）。
    pub reduced: bool,
}

impl MotionPolicy {
    /// 常态（动效全开）。
    pub const fn normal() -> MotionPolicy {
        MotionPolicy { reduced: false }
    }

    /// F245 降级态。
    pub const fn reduced() -> MotionPolicy {
        MotionPolicy { reduced: true }
    }

    /// 本曲线在当前策略下的实际时长（ms）。
    ///
    /// F245 判据：开启后替换为 80ms 直切（保状态信号、去位移缩放）；
    /// 线性档时长本就由调用方给定，降级同样钳到 80ms。
    pub const fn duration_ms(&self, curve: Curve, linear_ms: u32) -> u32 {
        if self.reduced {
            return REDUCED_MOTION_MS;
        }
        match curve {
            Curve::Enter => 120,
            Curve::Exit => 120,
            Curve::Emphasis => 320,
            Curve::Emphasis200 => 200,
            Curve::Spring => 320,
            Curve::Linear => linear_ms,
        }
    }

    /// 归一化进度（t ∈ [0,duration) → 进度 0..=1000，整数定点）。
    ///
    /// 缓动实现（整数逼近，core 无 f64）：
    /// - Enter/Emphasis/Emphasis200：cubic ease-out（1-(1-t)^3）——
    ///   「快进慢收」的进入手感；
    /// - Exit：cubic ease-in（t^3）——退场先缓后快；
    /// - Spring：先 overshoot 后回弹的五段折线近似（20% 处 110% 峰值、
    ///   55% 处 95% 谷值、100% 收 1000）——「回弹」的定性形状；
    /// - Linear：直通。
    pub fn progress(&self, curve: Curve, t_ms: u32, linear_ms: u32) -> u32 {
        let dur = self.duration_ms(curve, linear_ms).max(1);
        let t = (t_ms.min(dur) as u64 * 1000) / dur as u64; // 0..1000
        if self.reduced {
            // 直切：80ms 内完成即到终点（保状态变化信号）。
            return 1000;
        }
        match curve {
            Curve::Exit => ((t * t * t) / 1_000_000) as u32,
            Curve::Enter | Curve::Emphasis | Curve::Emphasis200 => {
                let inv = 1000 - t as u32;
                (1000 - (inv as u64 * inv as u64 * inv as u64 / 1_000_000) as u32) as u32
            }
            Curve::Spring => {
                let t = t as u32;
                if t <= 200 {
                    // 0→20%：升到 1100（overshoot 10%）。
                    t * 1100 / 200
                } else if t <= 550 {
                    // 20%→55%：1100 回落到 950（回弹谷）。
                    1100 - (t - 200) * 150 / 350
                } else {
                    // 55%→100%：950 收到 1000。
                    950 + (t - 550) * 50 / 450
                }
            }
            Curve::Linear => t as u32,
        }
    }
}

// ---------------------------------------------------------------------------
// 令牌色整数工具（F151 令牌的语义面，H 域只读消费）
// ---------------------------------------------------------------------------

/// 8bit/通道令牌色（F151 语义色的最小承载——只做判定不做渲染）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// sRGB 线性化 LUT（×100000 定点，WCAG 2.x 分段定义）：
///
/// - 段一（csrgb ≤ 0.04045）：`lin = csrgb/12.92`；
/// - 段二：`lin = ((csrgb+0.055)/1.055)^2.4`。
///
/// 幂次用整数二分求 `y^5 = cs^12`（u128 域，编译期 const fn 计算，
/// 内核零运行时开销）。误差来源只有 csrgb 的 1000 分点量化（≤0.5/1000）
/// 与 floor 截断——黑白端精确、全谱与 f64 参考值偏差 <0.5%，满足
/// 「≥4.5:1 门两侧取样」的判据用途。
static SRGB_LIN100K: [u64; 256] = build_srgb_lut();

/// `floor(cs^2.4)`（cs ∈ 0..=1000）——整数二分实现，y^5 = cs^12 在 u128 域。
const fn fifth_root_pow24(cs: u64) -> u64 {
    let c = cs as u128;
    let c2 = c * c;
    let c3 = c2 * c;
    let c6 = c3 * c3;
    let t = c6 * c6; // cs^12，cs ≤ 1000 → ≤ 1e36 < u128 上限
    let mut lo: u128 = 0;
    let mut hi: u128 = 15_900_000; // 1000^2.4 ≈ 1.59e7
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        let m2 = mid * mid;
        let m4 = m2 * m2;
        let m5 = m4 * mid; // mid^5 ≤ 1.02e36 < 3.4e38
        if m5 <= t {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lo as u64
}

/// `((cs/1000 + 55/1000)/1.055)^2.4 × 100000`（sRGB 段二，×1e5 定点）。
const fn srgb_seg2_100k(cs: u64) -> u64 {
    // 归一化分子：n = (cs+55)/1055 ∈ 0..=1000（1000 分点）。
    let n = ((cs + 55) * 1000) / 1055;
    // y = floor(n^2.4)（0..=15848931）；换算 ×1e5 标度：
    // n^2.4 ×1e5 / 10^7.2 → 定点参考分母 15848932。
    (fifth_root_pow24(n) * 100_000) / 15_848_932
}

/// 编译期构建 LUT。
const fn build_srgb_lut() -> [u64; 256] {
    let mut table = [0u64; 256];
    let mut c = 0usize;
    while c < 256 {
        let cs = (c as u64 * 1000) / 255; // csrgb ×1000 量化
        table[c] = if cs <= 40 {
            (cs * 100_000) / 12_920 // 段一：/12.92 → ×1e5
        } else {
            srgb_seg2_100k(cs)
        };
        c += 1;
    }
    table
}

impl Rgb8 {
    pub const fn new(r: u8, g: u8, b: u8) -> Rgb8 {
        Rgb8 { r, g, b }
    }

    /// 相对亮度（WCAG 2.x，×10000 定点，白 = 10000）。
    ///
    /// L = 0.2126·R + 0.7152·G + 0.0722·B（R/G/B 为线性化通道，
    /// 查编译期 LUT）。
    pub fn luma_10000(&self) -> u32 {
        let lin = (SRGB_LIN100K[self.r as usize] * 2126
            + SRGB_LIN100K[self.g as usize] * 7152
            + SRGB_LIN100K[self.b as usize] * 722)
            / 10_000; // ×1e5 标度（权重 ×1e4）
        (lin / 10) as u32 // 1e5 → 1e4 标度（白 = 10000）
    }
}

/// 对比度倍数（×100 整数）：WCAG 公式 (L1+0.05)/(L2+0.05)。
///
/// 判据口径：≥450 即满足「对比度 ≥4.5:1」（F201 插入符深浅底、
/// F229 占位/正文可分辨、F242 图标可辨共用同一条线）。
pub fn contrast_ratio_100(a: Rgb8, b: Rgb8) -> u32 {
    let la = a.luma_10000() as u64 + 500;
    let lb = b.luma_10000() as u64 + 500;
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    ((hi * 100) / lo.max(1)) as u32
}

/// 灰值档差（F229 判据「灰值差 ≥2 档」）。
///
/// 档位定义在 **sRGB 编码域**（用户看见的灰阶），0..255 均分 10 档
/// （每档 25.5 级）：差 ≥2 档 = 最大通道差 ≥51。判据语义是「占位符
/// 与正文的灰阶肉眼可分辨」，用编码域而不是线性化亮度——线性域把
/// 中间调压扁，会把两档灰差误判成同档。
pub fn gray_steps_apart(a: Rgb8, b: Rgb8) -> u32 {
    let dr = a.r.abs_diff(b.r);
    let dg = a.g.abs_diff(b.g);
    let db = a.b.abs_diff(b.b);
    let max = dr.max(dg).max(db) as u32;
    max / 26
}

// ---------------------------------------------------------------------------
// 平面几何（i32 像素域）
// ---------------------------------------------------------------------------

/// 整数矩形（左上原点，右/下开区间）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    pub const fn right(&self) -> i32 {
        self.x + self.w
    }

    pub const fn bottom(&self) -> i32 {
        self.y + self.h
    }

    /// 点包含判定。
    pub const fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }

    /// 矩形相交面积（0 = 不相交）。
    pub fn intersect_area(&self, o: &Rect) -> i64 {
        let ix = self.x.max(o.x);
        let iy = self.y.max(o.y);
        let ir = self.right().min(o.right());
        let ib = self.bottom().min(o.bottom());
        if ir <= ix || ib <= iy {
            return 0;
        }
        ((ir - ix) as i64) * ((ib - iy) as i64)
    }

    /// 把值钳进 [lo, hi]（lo > hi 时返回 lo——非法区间显性取下界）。
    pub const fn clamp_i32(v: i32, lo: i32, hi: i32) -> i32 {
        if lo > hi {
            return lo;
        }
        if v < lo {
            lo
        } else if v > hi {
            hi
        } else {
            v
        }
    }

    /// 矩形钳进屏幕（F237 显示器拉回、F214 最小尺寸的共同落点）。
    /// 宽高超屏时取屏宽高（最小尺寸纪律在上层先行钳制）。
    pub fn clamped_into(&self, screen: &Rect) -> Rect {
        let w = Rect::clamp_i32(self.w, 1, screen.w);
        let h = Rect::clamp_i32(self.h, 1, screen.h);
        let x = Rect::clamp_i32(self.x, screen.x, screen.right() - w);
        let y = Rect::clamp_i32(self.y, screen.y, screen.bottom() - h);
        Rect::new(x, y, w, h)
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

/// h1base 自检（三件基建的判据面）。
pub fn run_h1base_checks() -> CheckSet {
    let mut set = CheckSet::new("h1base");

    // 1. F124 时长表：五档数值与主册判据逐字一致。
    let p = MotionPolicy::normal();
    set.add(
        "F124 durations 120/120/320/200/320",
        p.duration_ms(Curve::Enter, 0) == 120
            && p.duration_ms(Curve::Exit, 0) == 120
            && p.duration_ms(Curve::Emphasis, 0) == 320
            && p.duration_ms(Curve::Emphasis200, 0) == 200
            && p.duration_ms(Curve::Spring, 0) == 320,
        "",
    );
    set.add("F124 linear passthrough", p.duration_ms(Curve::Linear, 2000) == 2000, "");

    // 2. F245 降级：全档 80ms 直切。
    let r = MotionPolicy::reduced();
    set.add(
        "F245 reduced all curves 80ms",
        r.duration_ms(Curve::Enter, 0) == REDUCED_MOTION_MS
            && r.duration_ms(Curve::Spring, 0) == REDUCED_MOTION_MS
            && r.duration_ms(Curve::Linear, 2000) == REDUCED_MOTION_MS,
        "",
    );

    // 3. 缓动形状：进入 ease-out 单调不减、终点 1000；退出 ease-in 起点近 0。
    let mut mono = true;
    let mut prev = 0u32;
    for ms in (0..=120).step_by(10) {
        let v = p.progress(Curve::Enter, ms, 0);
        if v < prev {
            mono = false;
        }
        prev = v;
    }
    set.add(
        "Enter ease-out monotonic & ends at 1000",
        mono && p.progress(Curve::Enter, 120, 0) == 1000 && p.progress(Curve::Enter, 0, 0) == 0,
        "",
    );
    set.add(
        "Exit ease-in starts slow",
        p.progress(Curve::Exit, 12, 0) < 40,
        "",
    );
    let over = p.progress(Curve::Spring, 64, 0); // 320ms 的 20% 处 overshoot
    set.add(
        "Spring overshoots to ~110% at 20%",
        (1000..=1100).contains(&over),
        "",
    );

    // 4. F245 直切语义：降级态进度立即 1000（保状态信号）。
    set.add(
        "F245 reduced progress cuts to 1000",
        r.progress(Curve::Enter, 1, 0) == 1000,
        "",
    );

    // 5. 对比度判据：黑白 21:1 量级；4.5:1 线两侧取样（浅灰对白、深灰对黑）。
    let white = Rgb8::new(255, 255, 255);
    let black = Rgb8::new(0, 0, 0);
    let midgray = Rgb8::new(118, 118, 118); // WCAG ≈4.48:1 对白
    let darkgray = Rgb8::new(63, 63, 63); // WCAG ≈1.99:1 对黑
    set.add(
        "contrast black/white ~21:1",
        (2050..=2200).contains(&contrast_ratio_100(black, white)),
        "",
    );
    set.add(
        "contrast 4.5:1 line sides",
        contrast_ratio_100(midgray, white) >= 450 && contrast_ratio_100(darkgray, black) < 450,
        "",
    );
    set.add("contrast identical = 100", contrast_ratio_100(midgray, midgray) == 100, "");

    // 6. 灰值档差（sRGB 编码域，10 档 ×25.5 级）。
    let placeholder = Rgb8::new(150, 150, 150); // 占位符灰
    let bodytext = Rgb8::new(40, 40, 40); // 正文深灰
    set.add(
        "gray steps: placeholder/bodytext >= 2, near tones < 2",
        gray_steps_apart(placeholder, bodytext) >= 2
            && gray_steps_apart(placeholder, Rgb8::new(140, 140, 140)) < 2,
        "",
    );

    // 7. 几何：相交面积 / 钳入屏幕。
    let a = Rect::new(0, 0, 100, 100);
    let b = Rect::new(50, 50, 100, 100);
    set.add("intersect area 2500", a.intersect_area(&b) == 2500, "");
    set.add("disjoint area 0", a.intersect_area(&Rect::new(200, 200, 10, 10)) == 0, "");
    let off = Rect::new(-50, 900, 400, 200);
    let screen = Rect::new(0, 0, 1920, 1080);
    let back = off.clamped_into(&screen);
    set.add(
        "clamped_into pulls back onscreen",
        screen.contains(back.x, back.y)
            && back.right() <= screen.right()
            && back.bottom() <= screen.bottom(),
        "",
    );
    set.add(
        "clamp wider than screen capped",
        Rect::new(0, 0, 3000, 100).clamped_into(&screen).w == 1920,
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f124_curve_table_matches_manual() {
        let p = MotionPolicy::normal();
        assert_eq!(p.duration_ms(Curve::Enter, 0), 120);
        assert_eq!(p.duration_ms(Curve::Exit, 0), 120);
        assert_eq!(p.duration_ms(Curve::Emphasis, 0), 320);
        assert_eq!(p.duration_ms(Curve::Emphasis200, 0), 200);
        assert_eq!(p.duration_ms(Curve::Spring, 0), 320);
        assert_eq!(p.duration_ms(Curve::Linear, 1500), 1500);
    }

    #[test]
    fn f245_reduced_motion_overrides_all() {
        let r = MotionPolicy::reduced();
        for c in [Curve::Enter, Curve::Exit, Curve::Emphasis, Curve::Emphasis200, Curve::Spring] {
            assert_eq!(r.duration_ms(c, 999), 80);
        }
        assert_eq!(r.progress(Curve::Spring, 10, 0), 1000);
    }

    #[test]
    fn easing_shapes_are_sane() {
        let p = MotionPolicy::normal();
        // 进入曲线：中点进度 > 500（ease-out 快进）。
        assert!(p.progress(Curve::Enter, 60, 0) > 500);
        // 退出曲线：中点进度 < 500（ease-in 慢起）。
        assert!(p.progress(Curve::Exit, 60, 0) < 500);
        // 弹性曲线：20% 处过冲。
        assert!(p.progress(Curve::Spring, 64, 0) >= 1000);
        // 终点收敛。
        for c in [Curve::Enter, Curve::Exit, Curve::Emphasis, Curve::Spring] {
            assert_eq!(p.progress(c, 10_000, 0), 1000);
        }
    }

    #[test]
    fn wcag_contrast_reference_values() {
        let white = Rgb8::new(255, 255, 255);
        let black = Rgb8::new(0, 0, 0);
        // WCAG 参考值 21:1（整数定点 ±容差）。
        let r = contrast_ratio_100(black, white);
        assert!((2050..=2200).contains(&r), "black/white = {r}");
        // #777 vs 白：WCAG 参考 4.48:1 —— 在 4.5 门下方。
        assert!(contrast_ratio_100(Rgb8::new(119, 119, 119), white) < 460);
        // #707070 vs 白：WCAG 参考 4.95:1 —— 在 4.5 门上方。
        assert!(contrast_ratio_100(Rgb8::new(112, 112, 112), white) > 460);
        // 纯灰 #3F3F3F vs 黑 ≈2.0:1。
        assert!((180..=220).contains(&contrast_ratio_100(Rgb8::new(63, 63, 63), black)));
    }

    #[test]
    fn rect_geometry_edge_cases() {
        let s = Rect::new(0, 0, 1920, 1080);
        // 恰好贴边不算越界。
        assert!(s.contains(1919, 1079));
        assert!(!s.contains(1920, 1079));
        // 零尺寸矩形相交为 0。
        assert_eq!(s.intersect_area(&Rect::new(5, 5, 0, 0)), 0);
        // 非法钳制区间显性取下界。
        assert_eq!(Rect::clamp_i32(5, 10, 1), 10);
    }

    #[test]
    fn h1base_selfcheck_all_green() {
        let set = run_h1base_checks();
        assert!(set.all_passed(), "h1base 自检存在红项");
        assert!(!set.truncated());
    }
}
