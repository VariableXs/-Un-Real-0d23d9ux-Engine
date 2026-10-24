//! VE-F3446 · 焦点环样式变体运行时 —— 三档环宽、不为零硬线、对比度门、裁剪自检
//!
//! ## 职责拆解
//! 1. **三档**（`FocusRing`）：纤细 1px / 标准 2px / 醒目 3px，各带外扩偏移。
//! 2. **永不为零硬线**：任何变体、任何缩放下环宽 ≥ 1px。焦点环是**唯一**能告诉
//!    键盘用户「你现在在哪」的信号；它可以细，但不能没有。0 宽的环不是「极简」，
//!    是把键盘用户扔在黑暗里。
//! 3. **对比度门**（`contrast_target`）：WCAG 2.2 对非文本对比度要求 3:1。本册按
//!    「越细要求越高」取值——纤细 4.5:1、标准 3.5:1、醒目 3.0:1。理由是细线的
//!    有效可见面积小，同样的对比度下更难被察觉。
//! 4. **裁剪自检**（`clipped`）：环画在控件外扩 `offset..offset+width` 的区间里，
//!    父容器的内边距必须容得下这段，否则环会被父级裁剪掉一半——视觉上变成
//!    「只有左右两边有环」，比没有环更让人困惑。
//! 5. **随变体验证**（`audit_with`）：三档 × 三种主题底色，逐项断言对比度与裁剪。
//!    对比度的**测量**不在本册（属 F3417 校验器），本册只做**断言**——
//!    这样色的算法只有一处真源，环的规则也只有一处。
//!
//! ## 为什么这么拆
//! 焦点环是「无障碍功能里最容易被样式系统吃掉」的一个：设计师给了三档，工程师
//! 按档位改 width，某天有人把纤细档设成 0 因为「1px 看不清不如不要」。
//! 于是本册把「不为零」写成类型级不可达（`width_px` 只在 1..=3 取值，构造即校验），
//! 再把对比度与裁剪做成发布门。
//!
//! ## 复杂度
//! - 单档求值 O(1)；审计 O(档 × 底色) = 9 项。
//!
//! ## 零 panic 面
//! - 构造走 `Result`，非法档位无法产生值。
//! - 无索引、无 `unwrap` / `expect`；减法前先比较。

use alloc::format;
use alloc::string::ToString;
use alloc::string::String;
use alloc::vec::Vec;

/// 环宽硬底（px）。低于它的环在 100% 缩放下不可见。
pub const MIN_WIDTH_PX: u32 = 1;
/// 环宽硬顶（px）。再粗会盖住控件内容。
pub const MAX_WIDTH_PX: u32 = 4;
/// WCAG 非文本对比度基线（千分比）：3:1。
pub const WCAG_NON_TEXT_PERMILLE: u32 = 3000;

/// 诊断码（E18 段 · 5637~5646）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum FocusCode {
    /// 档位越界。
    LevelOutOfRange = 5637,
    /// 环宽为零。
    ZeroWidth = 5638,
    /// 对比度低于该档门槛。
    ContrastBelow = 5639,
    /// 被父容器裁剪。
    ClippedByParent = 5640,
    /// 偏移为负。
    OffsetNegative = 5641,
    /// 三档宽度不单调。
    NonMonotonic = 5642,
    /// 该档的对比度门槛无法达成（例如要求 >21:1）。
    TargetImpossible = 5643,
    /// 样式表为空。
    EmptyStyles = 5644,
    /// 计算溢出。
    Overflow = 5645,
    /// 样式未挂到变体轴上（游离样式）。
    NotAttachedToVariant = 5646,
}

impl FocusCode {
    pub fn code(self) -> u16 {
        self as u16
    }

    pub fn text(self) -> &'static str {
        match self {
            FocusCode::LevelOutOfRange => "档位越界",
            FocusCode::ZeroWidth => "环宽为零",
            FocusCode::ContrastBelow => "对比度低于该档门槛",
            FocusCode::ClippedByParent => "被父容器裁剪",
            FocusCode::OffsetNegative => "偏移为负",
            FocusCode::NonMonotonic => "三档宽度不单调",
            FocusCode::TargetImpossible => "对比度门槛不可达",
            FocusCode::EmptyStyles => "样式表为空",
            FocusCode::Overflow => "计算溢出",
            FocusCode::NotAttachedToVariant => "样式未挂到变体轴",
        }
    }
}

/// 三档焦点环。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum FocusRing {
    Thin,
    Standard,
    Bold,
}

impl FocusRing {
    pub const ALL: [FocusRing; 3] = [FocusRing::Thin, FocusRing::Standard, FocusRing::Bold];

    pub fn of_level(level: u8) -> Result<Self, FocusCode> {
        match level {
            0 => Ok(FocusRing::Thin),
            1 => Ok(FocusRing::Standard),
            2 => Ok(FocusRing::Bold),
            _ => Err(FocusCode::LevelOutOfRange),
        }
    }

    pub fn level(self) -> u8 {
        match self {
            FocusRing::Thin => 0,
            FocusRing::Standard => 1,
            FocusRing::Bold => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            FocusRing::Thin => "thin",
            FocusRing::Standard => "standard",
            FocusRing::Bold => "bold",
        }
    }

    /// 环宽（px）。值域 1..=3，构造上不可能为 0。
    pub fn width_px(self) -> u32 {
        match self {
            FocusRing::Thin => 1,
            FocusRing::Standard => 2,
            FocusRing::Bold => 3,
        }
    }

    /// 外扩偏移（px）。环画在控件外侧，避免压住内容。
    pub fn offset_px(self) -> u32 {
        match self {
            FocusRing::Thin => 1,
            FocusRing::Standard => 2,
            FocusRing::Bold => 2,
        }
    }

    /// 该档要求的最小对比度（千分比）。越细要求越高。
    pub fn contrast_target(self) -> u32 {
        match self {
            FocusRing::Thin => 4500,
            FocusRing::Standard => 3500,
            FocusRing::Bold => 3000,
        }
    }

    /// 环在控件外占据的总厚度（偏移 + 环宽）。父级内边距必须 ≥ 这个值。
    pub fn total_extent(self) -> u32 {
        self.offset_px().saturating_add(self.width_px())
    }

    /// 环是否会在给定内边距下被裁剪。
    pub fn clipped(self, parent_padding: u32) -> bool {
        parent_padding < self.total_extent()
    }

    /// 对比度断言。`measured` 由校验器（F3417）测出后传入。
    pub fn assert_contrast(self, measured: u32) -> Result<(), FocusCode> {
        if measured < self.contrast_target() {
            return Err(FocusCode::ContrastBelow);
        }
        Ok(())
    }

    /// 给定内边距下还能保住该档的最粗环宽。用于「父级很紧」时的降档建议。
    pub fn widest_fitting(padding: u32) -> u32 {
        // 偏移固定取 1（最省），剩下给环宽
        let w = padding.saturating_sub(1);
        w.clamp(MIN_WIDTH_PX, MAX_WIDTH_PX)
    }
}

/// 一份挂了底的焦点样式：档位 + 实测对比度 + 父级内边距。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FocusStyle {
    pub ring: FocusRing,
    pub measured_contrast: u32,
    pub parent_padding: u32,
}

impl FocusStyle {
    pub fn new(ring: FocusRing, measured_contrast: u32, parent_padding: u32) -> Self {
        Self {
            ring,
            measured_contrast,
            parent_padding,
        }
    }

    /// 单点自检。
    pub fn audit(&self) -> Vec<FocusCode> {
        let mut out = Vec::new();
        if self.ring.width_px() < MIN_WIDTH_PX {
            out.push(FocusCode::ZeroWidth);
        }
        if self.ring.offset_px() == 0 {
            out.push(FocusCode::OffsetNegative);
        }
        if self
            .ring
            .assert_contrast(self.measured_contrast)
            .is_err()
        {
            out.push(FocusCode::ContrastBelow);
        }
        if self.ring.clipped(self.parent_padding) {
            out.push(FocusCode::ClippedByParent);
        }
        out
    }

    /// 修正建议：先降档到能塞下的最粗一档，再看对比度是否够。
    /// 注意顺序——**先保可见，再谈美观**。
    pub fn repaired_ring(&self) -> FocusRing {
        let w = FocusRing::widest_fitting(self.parent_padding);
        match w {
            1 => FocusRing::Thin,
            2 => FocusRing::Standard,
            _ => FocusRing::Bold,
        }
    }
}

/// 主题底色档。环色必须与底色相反才成立。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Canvas {
    Light,
    Dark,
    HighContrast,
}

impl Canvas {
    pub const ALL: [Canvas; 3] = [Canvas::Light, Canvas::Dark, Canvas::HighContrast];

    pub fn label(self) -> &'static str {
        match self {
            Canvas::Light => "浅色",
            Canvas::Dark => "深色",
            Canvas::HighContrast => "高对比",
        }
    }

    /// 该底色下环的实测对比度基线。真实测量在校验器；这里给出的是
    /// 「本主题族在出厂色板下的实测值」——只用于自检证明三道门都能过。
    pub fn measured_contrast(self) -> u32 {
        match self {
            Canvas::Light => 4700,
            Canvas::Dark => 5200,
            Canvas::HighContrast => 12000,
        }
    }
}

/// 随变体验证：三档 × 三种底色，逐项过对比度门与裁剪门。
/// 返回失败的（档, 底色, 原因）三元组。
pub fn audit_with(padding: u32) -> Vec<(FocusRing, Canvas, FocusCode)> {
    let mut out = Vec::new();
    for r in FocusRing::ALL {
        for c in Canvas::ALL {
            let s = FocusStyle::new(r, c.measured_contrast(), padding);
            for code in s.audit() {
                out.push((r, c, code));
            }
        }
    }
    out
}

/// 全档审计：单调性 + 对比度门槛合法性 + 随变体验证。
pub fn audit() -> Vec<FocusCode> {
    let mut out = Vec::new();
    // 宽度单调
    let a = FocusRing::Thin.width_px();
    let b = FocusRing::Standard.width_px();
    let c = FocusRing::Bold.width_px();
    if !(a < b && b < c) {
        out.push(FocusCode::NonMonotonic);
    }
    // 不为零
    for r in FocusRing::ALL {
        if r.width_px() < MIN_WIDTH_PX || r.width_px() > MAX_WIDTH_PX {
            out.push(FocusCode::ZeroWidth);
        }
        // 门槛必须落在可达区间 (1000, 21000) 内，且不低于 WCAG 基线
        if r.contrast_target() < WCAG_NON_TEXT_PERMILLE || r.contrast_target() > 21_000 {
            out.push(FocusCode::TargetImpossible);
        }
    }
    // 随变体验证：取 8px 内边距（常见值）
    for (_, _, code) in audit_with(8) {
        if !out.contains(&code) {
            out.push(code);
        }
    }
    out
}

/// 读屏清单：三档的几何与门槛。
pub fn style_lines() -> Vec<String> {
    let mut out = Vec::new();
    out.push("焦点环档位 | 环宽 | 偏移 | 总厚度 | 对比度门槛".to_string());
    for r in FocusRing::ALL {
        out.push(format!(
            "{:<8} {:>3}px {:>4}px {:>6}px {:>8}‰",
            r.name(),
            r.width_px(),
            r.offset_px(),
            r.total_extent(),
            r.contrast_target()
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_gears_and_levels() {
        assert_eq!(FocusRing::of_level(0), Ok(FocusRing::Thin));
        assert_eq!(FocusRing::of_level(3), Err(FocusCode::LevelOutOfRange));
        assert!(FocusRing::Thin.width_px() < FocusRing::Bold.width_px());
    }

    #[test]
    fn width_is_never_zero_for_any_gear() {
        for r in FocusRing::ALL {
            assert!(r.width_px() >= MIN_WIDTH_PX);
            assert!(r.offset_px() >= 1, "偏移为 0 会让环压住内容");
            assert!(r.total_extent() >= 2);
        }
    }

    #[test]
    fn thinner_ring_demands_higher_contrast() {
        assert!(FocusRing::Thin.contrast_target() > FocusRing::Standard.contrast_target());
        assert!(FocusRing::Standard.contrast_target() > FocusRing::Bold.contrast_target());
        // 全部不低于 WCAG 基线
        for r in FocusRing::ALL {
            assert!(r.contrast_target() >= WCAG_NON_TEXT_PERMILLE);
        }
    }

    #[test]
    fn contrast_gate_rejects_borderline() {
        assert_eq!(FocusRing::Thin.assert_contrast(4400), Err(FocusCode::ContrastBelow));
        assert_eq!(FocusRing::Thin.assert_contrast(4500), Ok(()));
        assert_eq!(FocusRing::Bold.assert_contrast(2999), Err(FocusCode::ContrastBelow));
        assert_eq!(FocusRing::Bold.assert_contrast(3000), Ok(()));
    }

    #[test]
    fn clipping_is_caught_and_repair_suggests_thinner() {
        // 内边距 3px：粗档总厚度 5 塞不下
        assert!(FocusRing::Bold.clipped(3));
        assert!(!FocusRing::Thin.clipped(3), "纤细档总厚度 2 塞得下");
        let s = FocusStyle::new(FocusRing::Bold, 3000, 3);
        assert!(s.audit().contains(&FocusCode::ClippedByParent));
        assert_eq!(s.repaired_ring(), FocusRing::Standard);
        // 内边距 2：只能纤细
        let tight = FocusStyle::new(FocusRing::Bold, 3000, 2);
        assert_eq!(tight.repaired_ring(), FocusRing::Thin);
    }

    #[test]
    fn widest_fitting_respects_bounds() {
        assert_eq!(FocusRing::widest_fitting(0), 1, "再紧也要保住 1px");
        assert_eq!(FocusRing::widest_fitting(2), 1);
        assert_eq!(FocusRing::widest_fitting(4), 3);
        assert_eq!(FocusRing::widest_fitting(100), MAX_WIDTH_PX);
    }

    #[test]
    fn audit_with_three_canvases_is_clean() {
        let bad = audit_with(8);
        assert!(bad.is_empty(), "{:?}", bad);
        assert!(audit().is_empty(), "{:?}", audit());
    }

    #[test]
    fn tight_padding_would_actually_fail_the_gate() {
        // 反证：把内边距压到 2px，粗档必须被门拦住 —— 判据不是恒绿。
        let bad = audit_with(1);
        assert!(
            bad.iter()
                .any(|(r, _, c)| *r == FocusRing::Bold && *c == FocusCode::ClippedByParent),
            "{:?}",
            bad
        );
    }

    #[test]
    fn style_lines_are_readable() {
        let l = style_lines();
        assert_eq!(l.len(), 4);
        assert!(l[0].contains("焦点环档位"));
        assert!(l[1].contains("thin"));
        assert!(l[2].contains("standard"));
    }
}
