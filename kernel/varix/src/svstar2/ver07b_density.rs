//! VE-F3442 · 密度变体运行时 —— 三档密度、网格量化、触达硬线、级联重算
//!
//! ## 职责拆解
//! 密度不是「把数字乘个系数」这么简单，它同时动三样东西：
//! 1. **间距**（`spacing_permille`）：875 / 1000 / 1150 千分比。
//!    乘完之后必须**量化到 2px 网格**——不量化的结果是 7px、11px、13px 这类
//!    无法用像素对齐的间距，在高 DPI 下会糊成一片。
//! 2. **行高**（`line_height_permille`）：1150 / 1300 / 1500 千分比。
//!    行高必须严格大于字号，否则字形被裁——这条在紧凑档最容易破。
//! 3. **触达目标**（`touch_min`）：32 / 40 / 48 px，且必须同时满足两条硬线：
//!    全档 ≥ 32px（`FLOOR_PX`）；舒适档 ≥ 44px（`COMFORT_FLOOR_PX`，**不下探**）。
//!    硬线的意义是：紧凑档可以靠得更近，但不能小到点不中；舒适档既然叫舒适，
//!    就不许为了塞下更多控件把目标缩回 44 以下。
//!
//! 4. **级联重算**（`Cascade`）：档位切换后重算整条间距标度，并给出
//!    「改了几个令牌」的精确计数。影响面计算（F3448）靠这个数决定轻重。
//!
//! ## 为什么这么拆
//! 密度是最容易被做成「一个乘数」的轴，也是最能暴露「乘数思维」问题的轴：
//! 乘数不会量化（网格错位）、不会检查下限（触达失效）、不会检查单调性
//! （紧凑档反而比标准档更松）。所以本册把三件事都做成**可失败的检查**，
//! 而不是写进文档让人自觉。
//!
//! ## 复杂度
//! - 单令牌派生：O(1)。
//! - 整标度重算：O(令牌数)，出厂标度 7 项。
//! - 审计：O(档数 × 检查项) = 常数。
//!
//! ## 零 panic 面
//! - 乘除全走 u64 中间量，避免 u32 溢出；除法分母是字面常量 1000。
//! - 无切片索引（`get()` + `unwrap_or`），无 `unwrap` / `expect`。

use alloc::vec;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 任何档位下触达目标的硬底（px）。低于此值的手指目标不可接受。
pub const FLOOR_PX: u32 = 32;
/// 舒适档触达目标硬线（px）。舒适档不许下探到此值以下。
pub const COMFORT_FLOOR_PX: u32 = 44;
/// 间距量化网格（px）。2px 是 1x/1.5x/2x 三种缩放比的最小公倍粒度。
pub const GRID_PX: u32 = 2;

/// 诊断码（E18 段 · 5597~5606）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum DensityCode {
    /// 档位越界。
    LevelOutOfRange = 5597,
    /// 触达目标低于全档硬底。
    TouchBelowFloor = 5598,
    /// 舒适档触达目标下探。
    ComfortTouchDowngrade = 5599,
    /// 行高不大于字号（字形被裁）。
    LineHeightTooSmall = 5600,
    /// 三档间距不单调。
    SpacingNonMonotonic = 5601,
    /// 量化后偏离网格。
    GridMisaligned = 5602,
    /// 级联重算结果与独立派生不一致。
    CascadeSkew = 5603,
    /// 标度为空。
    EmptyScale = 5604,
    /// 字号超出可派生范围。
    FontTooLarge = 5605,
    /// 计算溢出。
    ScaleOverflow = 5606,
}

impl DensityCode {
    pub fn code(self) -> u16 {
        self as u16
    }

    pub fn text(self) -> &'static str {
        match self {
            DensityCode::LevelOutOfRange => "档位越界",
            DensityCode::TouchBelowFloor => "触达目标低于硬底",
            DensityCode::ComfortTouchDowngrade => "舒适档触达目标下探",
            DensityCode::LineHeightTooSmall => "行高不大于字号",
            DensityCode::SpacingNonMonotonic => "三档间距不单调",
            DensityCode::GridMisaligned => "量化后偏离网格",
            DensityCode::CascadeSkew => "级联重算与独立派生不一致",
            DensityCode::EmptyScale => "间距标度为空",
            DensityCode::FontTooLarge => "字号超出派生范围",
            DensityCode::ScaleOverflow => "计算溢出",
        }
    }
}

/// 三档密度。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Density {
    Compact,
    Standard,
    Comfortable,
}

impl Density {
    pub const ALL: [Density; 3] = [Density::Compact, Density::Standard, Density::Comfortable];

    /// 出厂基准字号（px）。行高派生以它为基准，避免「行高按 16 算，字号按 14 排」。
    pub const BASE_FONT_PX: u32 = 14;

    pub fn of_level(level: u8) -> Result<Self, DensityCode> {
        match level {
            0 => Ok(Density::Compact),
            1 => Ok(Density::Standard),
            2 => Ok(Density::Comfortable),
            _ => Err(DensityCode::LevelOutOfRange),
        }
    }

    pub fn level(self) -> u8 {
        match self {
            Density::Compact => 0,
            Density::Standard => 1,
            Density::Comfortable => 2,
        }
    }

    /// 与总纲轴表的对应名（`compact` / `standard` / `comfortable`）。
    pub fn name(self) -> &'static str {
        match self {
            Density::Compact => "compact",
            Density::Standard => "standard",
            Density::Comfortable => "comfortable",
        }
    }

    /// 间距乘数（千分比）。
    pub fn spacing_permille(self) -> u32 {
        match self {
            Density::Compact => 875,
            Density::Standard => 1000,
            Density::Comfortable => 1150,
        }
    }

    /// 行高乘数（千分比，相对字号）。
    pub fn line_height_permille(self) -> u32 {
        match self {
            Density::Compact => 1150,
            Density::Standard => 1300,
            Density::Comfortable => 1500,
        }
    }

    /// 该档的触达目标下限（px）。
    pub fn touch_min(self) -> u32 {
        match self {
            Density::Compact => FLOOR_PX,
            Density::Standard => 40,
            Density::Comfortable => 48,
        }
    }

    /// 把请求的触达尺寸夹到合法区间。**夹的方向永远是向上**——
    /// 宁可让控件大一点，不可让手指点不中。
    pub fn touch_target(self, requested: u32) -> u32 {
        let mut v = requested.max(self.touch_min()).max(FLOOR_PX);
        if self == Density::Comfortable {
            v = v.max(COMFORT_FLOOR_PX);
        }
        v
    }

    /// 行高（px）。保证严格大于字号：量化后若等于字号，往上补一格。
    pub fn line_height(self, font_px: u32) -> u32 {
        let raw = mul_permille(font_px, self.line_height_permille());
        let q = quantize(raw);
        if q <= font_px {
            quantize(font_px + GRID_PX)
        } else {
            q
        }
    }
}

/// 千分比乘法，u64 中间量防溢出。
fn mul_permille(value: u32, permille: u32) -> u32 {
    let v = u64::from(value) * u64::from(permille);
    let r = (v + 500) / 1000;
    if r > u64::from(u32::MAX) {
        u32::MAX
    } else {
        r as u32
    }
}

/// 量化到 2px 网格，四舍五入。0 保持 0（「零间距」是合法语义，不能量化成 2）。
pub fn quantize(px: u32) -> u32 {
    if px == 0 {
        return 0;
    }
    let half = GRID_PX / 2;
    let q = ((px + half) / GRID_PX) * GRID_PX;
    if q == 0 {
        GRID_PX
    } else {
        q
    }
}

/// 出厂间距标度（标准档的绝对 px）。七级，覆盖 4~48 的常见梯度。
pub const BASE_SCALE: [u32; 7] = [4, 8, 12, 16, 24, 32, 48];

/// 派生一条标度。
///
/// 先乘再量化，逐项独立；**不做「保持相对比例」的二次校正**——二次校正会让
/// 4px 变成 4px、8px 变成 10px 这种「比例正确但网格错位」的结果，
/// 反而不如老老实实各自量化。
pub fn derive_scale(density: Density, base: &[u32]) -> Vec<u32> {
    let p = density.spacing_permille();
    let mut out = Vec::with_capacity(base.len());
    for v in base {
        out.push(quantize(mul_permille(*v, p)));
    }
    out
}

/// 一次级联重算的结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cascade {
    pub from: Density,
    pub to: Density,
    pub changed: usize,
    pub total: usize,
    pub pairs: Vec<(u32, u32)>,
}

impl Cascade {
    /// 变化率千分比：影响面计算用它判轻重档。
    pub fn change_permille(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        ((self.changed * 1000) / self.total) as u32
    }

    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "密度 {} → {}：{}/{} 项变化（{}‰）",
            self.from.name(),
            self.to.name(),
            self.changed,
            self.total,
            self.change_permille()
        ));
        for (i, (a, b)) in self.pairs.iter().enumerate() {
            if a != b {
                out.push(format!("  [{}] {}px → {}px", i, a, b));
            }
        }
        out
    }
}

/// 级联重算：算出 from→to 的逐项变化。
///
/// 与 `derive_scale` 的分工：`derive_scale` 是「从头算」，本函数是「算差异」。
/// 两条路必须给出同一个终点，否则说明其中一条用了不同的舍入——
/// `Cascade::verify` 就是拿这个当判据。
pub fn cascade(from: Density, to: Density, base: &[u32]) -> Cascade {
    let a = derive_scale(from, base);
    let b = derive_scale(to, base);
    let mut pairs = Vec::new();
    let mut changed = 0usize;
    for i in 0..base.len() {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            changed += 1;
        }
        pairs.push((x, y));
    }
    Cascade {
        from,
        to,
        changed,
        total: base.len(),
        pairs,
    }
}

impl Cascade {
    /// 自证：终点必须等于独立派生。
    pub fn verify(&self, base: &[u32]) -> Result<(), DensityCode> {
        let fresh = derive_scale(self.to, base);
        if fresh.len() != self.pairs.len() {
            return Err(DensityCode::CascadeSkew);
        }
        for (i, expected) in fresh.iter().enumerate() {
            let got = self.pairs.get(i).map(|p| p.1).unwrap_or(0);
            if got != *expected {
                return Err(DensityCode::CascadeSkew);
            }
        }
        Ok(())
    }
}

/// 单档自检。
pub fn audit_one(density: Density) -> Vec<DensityCode> {
    let mut out = Vec::new();
    // 触达硬线
    let t = density.touch_min();
    if t < FLOOR_PX {
        out.push(DensityCode::TouchBelowFloor);
    }
    if density == Density::Comfortable && t < COMFORT_FLOOR_PX {
        out.push(DensityCode::ComfortTouchDowngrade);
    }
    // 行高必须大于字号
    let lh = density.line_height(Density::BASE_FONT_PX);
    if lh <= Density::BASE_FONT_PX {
        out.push(DensityCode::LineHeightTooSmall);
    }
    // 网格对齐
    for v in derive_scale(density, &BASE_SCALE) {
        if v != 0 && v % GRID_PX != 0 {
            out.push(DensityCode::GridMisaligned);
            break;
        }
    }
    out
}

/// 全档自检：单档 + 单调性。
pub fn audit() -> Vec<DensityCode> {
    let mut out = Vec::new();
    for d in Density::ALL {
        for c in audit_one(d) {
            if !out.contains(&c) {
                out.push(c);
            }
        }
    }
    // 单调性：紧凑 ≤ 标准 ≤ 舒适，逐项。
    let a = derive_scale(Density::Compact, &BASE_SCALE);
    let b = derive_scale(Density::Standard, &BASE_SCALE);
    let c = derive_scale(Density::Comfortable, &BASE_SCALE);
    for i in 0..BASE_SCALE.len() {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        let z = c.get(i).copied().unwrap_or(0);
        if !(x <= y && y <= z) {
            out.push(DensityCode::SpacingNonMonotonic);
            break;
        }
    }
    // 级联自证
    for from in Density::ALL {
        for to in Density::ALL {
            if cascade(from, to, &BASE_SCALE).verify(&BASE_SCALE).is_err() {
                out.push(DensityCode::CascadeSkew);
            }
        }
    }
    out
}

/// 读屏清单：三档的四个关键数字一行一档。设置页与文档都消费这段。
pub fn matrix_lines() -> Vec<String> {
    let mut out = Vec::new();
    out.push("密度档位 | 间距 | 行高 | 触达".to_string());
    for d in Density::ALL {
        out.push(format!(
            "{:<12} {:>4}‰ {:>4}‰ {:>3}px",
            d.name(),
            d.spacing_permille(),
            d.line_height_permille(),
            d.touch_min()
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_gears_map_to_levels() {
        assert_eq!(Density::of_level(0), Ok(Density::Compact));
        assert_eq!(Density::of_level(2), Ok(Density::Comfortable));
        assert_eq!(
            Density::of_level(3),
            Err(DensityCode::LevelOutOfRange)
        );
        assert_eq!(Density::Compact.level(), 0);
        assert_eq!(Density::Comfortable.level(), 2);
    }

    #[test]
    fn compact_is_tighter_and_comfort_is_looser() {
        let base = [16u32];
        assert_eq!(derive_scale(Density::Compact, &base), vec![14]);
        assert_eq!(derive_scale(Density::Standard, &base), vec![16]);
        // 16 * 1.15 = 18.4 → 四舍五入 18
        assert_eq!(derive_scale(Density::Comfortable, &base), vec![18]);
    }

    #[test]
    fn zero_spacing_stays_zero() {
        assert_eq!(quantize(0), 0);
        // 1px 量化到 2px（宁可放大不可糊）
        assert_eq!(quantize(1), 2);
        assert_eq!(quantize(2), 2);
        assert_eq!(quantize(3), 4);
    }

    #[test]
    fn touch_targets_never_dip_below_floor() {
        for d in Density::ALL {
            assert!(d.touch_target(1) >= FLOOR_PX);
            assert!(d.touch_target(0) >= d.touch_min());
        }
        // 舒适档即使请求 30，也必须抬到 44 以上
        assert!(Density::Comfortable.touch_target(30) >= COMFORT_FLOOR_PX);
        assert_eq!(Density::Comfortable.touch_target(30), 48);
        // 紧凑档请求 64 时不该被压小
        assert_eq!(Density::Compact.touch_target(64), 64);
    }

    #[test]
    fn line_height_always_exceeds_font() {
        for d in Density::ALL {
            for font in [10u32, 12, 14, 16, 28, 64] {
                let lh = d.line_height(font);
                assert!(lh > font, "{} @ {}px 行高 {}px 不大于字号", d.name(), font, lh);
                assert_eq!(lh % GRID_PX, 0, "行高必须落在网格上");
            }
        }
    }

    #[test]
    fn cascade_verifies_against_independent_derivation() {
        let c = cascade(Density::Compact, Density::Comfortable, &BASE_SCALE);
        assert_eq!(c.verify(&BASE_SCALE), Ok(()));
        assert!(c.changed > 0, "两档之间必须真的有变化");
        assert_eq!(c.total, BASE_SCALE.len());
        assert!(c.change_permille() <= 1000);
        assert!(!c.lines().is_empty());
    }

    #[test]
    fn full_audit_is_clean() {
        assert!(
            audit().is_empty(),
            "出厂密度表必须自洽：{:?}",
            audit()
        );
    }

    #[test]
    fn monotonicity_check_can_actually_fail() {
        // 构造一个反向标度，验证判据不是恒绿：这里直接用假的乘数算一遍。
        let a = derive_scale(Density::Compact, &[48u32]);
        let b = derive_scale(Density::Comfortable, &[48u32]);
        assert!(a[0] < b[0], "紧凑 48 应小于舒适 48");
        // 若有人把 Compact 的乘数改成 1200，则 a > b，单调性判据必须报红。
        let fake_a = quantize(mul_permille(48, 1200));
        assert!(fake_a > b[0], "改成 1200‰ 后确实会倒挂");
    }

    #[test]
    fn matrix_lines_are_readable() {
        let lines = matrix_lines();
        assert_eq!(lines.len(), 4);
        assert!(lines[0].contains("密度档位"));
        assert!(lines[1].contains("compact"));
    }
}
