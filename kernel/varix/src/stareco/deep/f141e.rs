//! 深化层二 · F141 无障碍开放标准（2026-09-26 深化批次二）。
//!
//! 补深主册【状态与异常】「指南与门禁漂移 → CI 交叉校验（指南引用
//! 的阈值即代码常量）」与【设计细节】一分钟自测片段（主册 G-D-16）：
//! 阈值同源对账（指南文档数值 ↔ 门禁常量）、常见错例集检测规则、
//! 自测片段生成器、大字/正文双阈值字号权重判定、焦点环令牌映射表、
//! 自测报告格式。

use crate::checks::CheckSet;
use crate::stareco::a11yopen::{
    contrast_gate, focus_ring_ok, motion_duration_ms, restore_focus_ok, shape_redundancy_ok,
    CONTRAST_BODY_X10, CONTRAST_LARGE_X10, MotionPref, RestoreScene, SemanticState,
};

// ---------------------------------------------------------------------------
// 阈值同源对账：指南页展示值必须与门禁常量逐字一致（漂移即 CI 红）
// ---------------------------------------------------------------------------

/// 指南页的阈值卡（作者按文档填——CI 拿它与常量对拍）。
pub struct GuideThresholdCard {
    pub body_x10: u32,
    pub large_x10: u32,
    pub motion_full_ms: u32,
    pub motion_reduced_ms: u32,
    pub motion_off_ms: u32,
}

/// 对账：卡片五个数值全部命中常量才算同源（任何一个漂移 = 文档说谎）。
pub fn guide_card_matches(c: &GuideThresholdCard) -> bool {
    c.body_x10 == CONTRAST_BODY_X10
        && c.large_x10 == CONTRAST_LARGE_X10
        && c.motion_full_ms == motion_duration_ms(MotionPref::Full, 100)
        && c.motion_reduced_ms == motion_duration_ms(MotionPref::Reduced, 100)
        && c.motion_off_ms == motion_duration_ms(MotionPref::Off, 100)
}

// ---------------------------------------------------------------------------
// 大字/正文双阈值：字号权重判定（3:1 只给大字）
// ---------------------------------------------------------------------------

/// 大字定义（WCAG 2.1 口径的 VARIX 落地）：≥18pt 常规或 ≥14pt 加粗。
pub fn is_large_text(point_size: u16, bold: bool) -> bool {
    point_size >= 18 || (bold && point_size >= 14)
}

/// 双阈值判定：字号/字重决定走哪条线（一处一事实：is_large_text 决定）。
pub fn contrast_for_text(fg: (u8, u8, u8), bg: (u8, u8, u8), point_size: u16, bold: bool) -> bool {
    contrast_gate(fg, bg, is_large_text(point_size, bold))
}

// ---------------------------------------------------------------------------
// 常见错例集：错例特征 → 检测规则 → 修法建议（图解对错对比的数据面）
// ---------------------------------------------------------------------------

pub struct AntiPattern {
    pub name: &'static str,
    pub detect: fn() -> bool,
    pub fix_hint: &'static str,
}

/// 错例一：焦点环被"为了好看"删掉——检测：有焦点环令牌但宽度为 0。
pub fn anti_pattern_invisible_ring(ring_token: bool, ring_width: u8) -> bool {
    ring_token && ring_width == 0
}

/// 错例二：弹窗关闭后焦点丢宇宙——检测：有还焦点场景但无目标。
pub fn anti_pattern_lost_focus(scene: RestoreScene) -> bool {
    !restore_focus_ok(scene, true) // 场景合法却无目标 = 丢失
}

/// 错例三：颜色单通道语义（红绿只靠色相区分）——检测：语义态无形状冗余。
pub fn anti_pattern_color_only(state: SemanticState, has_shape: bool, has_text: bool) -> bool {
    !shape_redundancy_ok(state, has_shape, has_text)
}

// ---------------------------------------------------------------------------
// 焦点环令牌映射表（E1 主题联动：高对比加粗的机器面）
// ---------------------------------------------------------------------------

/// 环样式：令牌在 + 高对比主题 → 宽度加粗（2px→4px）；无令牌 → 0（不合规）。
pub fn ring_width_for(ring_token: bool, high_contrast: bool) -> u8 {
    match (ring_token, high_contrast) {
        (false, _) => 0,
        (true, false) => 2,
        (true, true) => 4,
    }
}

/// 映射表自洽：宽度产出必须过基础层 focus_ring_ok（联动不断链）。
pub fn ring_mapping_consistent() -> bool {
    focus_ring_ok(true, ring_width_for(true, false), false)
        && focus_ring_ok(true, ring_width_for(true, true), true)
        && !focus_ring_ok(false, ring_width_for(false, false), false)
}

// ---------------------------------------------------------------------------
// 自测片段生成器：每判据一段最小检测代码（「一分钟自测」的数据源）
// ---------------------------------------------------------------------------

/// 五判据的自测片段（页面上可复制的最小代码——文本生成，非运行时代码）。
pub fn snippet_for(criterion: u8) -> Result<&'static str, &'static str> {
    match criterion {
        1 => Ok("assert!(focus_ring_ok(true, 2, false)); // 判据一：焦点环"),
        2 => Ok("assert!(restore_focus_ok(RestoreScene::DialogClosed, true)); // 判据二：还焦点"),
        3 => Ok("assert!(contrast_gate((32,32,32),(255,255,255), false)); // 判据三：对比度"),
        4 => Ok("assert!(shape_redundancy_ok(SemanticState::Error, true, true)); // 判据四：形状冗余"),
        5 => Ok("assert_eq!(motion_duration_ms(MotionPref::Reduced, 100), 60); // 判据五：减弱动效"),
        _ => Err("判据号越界（1..=5）"),
    }
}

// ---------------------------------------------------------------------------
// 自测报告格式：五判据逐项 + 总分 + 徽标预判（F144 四查的无障碍一查）
// ---------------------------------------------------------------------------

pub struct A11ySelfReport {
    /// 五项判定（对齐 a11y_check 的 [bool; 5]）。
    pub five: [bool; 5],
}

impl A11ySelfReport {
    pub fn passed_count(&self) -> usize {
        self.five.iter().filter(|b| **b).count()
    }

    pub fn all_green(&self) -> bool {
        self.five.iter().all(|b| *b)
    }

    /// 徽标预判：五判据全绿才建议申请（缺一 = 先修再申请）。
    pub fn badge_recommendation(&self) -> &'static str {
        if self.all_green() {
            "全绿：可申请 Crafted for VARIX 徽标审核"
        } else {
            "未全绿：先补齐红项再申请（审核必拒）"
        }
    }

    /// 渲染为逐行文本（报告页展示面——行序固定，判定明示）。
    pub fn render(&self, buf: &mut alloc::string::String) {
        const NAMES: [&str; 5] = ["焦点环", "还焦点", "对比度", "形状冗余", "减弱动效"];
        for (i, name) in NAMES.iter().enumerate() {
            let verdict = if self.five[i] { "绿" } else { "红" };
            buf.push_str(&alloc::format!("判据{} {}：{}\n", i + 1, name, verdict));
        }
        buf.push_str(&alloc::format!("通过 {}/5\n", self.passed_count()));
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F141E_TAG: &str = "stareco-F141-deep2";

pub fn run_f141_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new(F141E_TAG);

    // 阈值同源
    let good = GuideThresholdCard {
        body_x10: CONTRAST_BODY_X10,
        large_x10: CONTRAST_LARGE_X10,
        motion_full_ms: 100,
        motion_reduced_ms: 60,
        motion_off_ms: 0,
    };
    set.add("f141e guide aligned", guide_card_matches(&good), "指南卡与常量同源");
    let mut drifted = good;
    drifted.body_x10 = 40; // 文档写了 4.0:1（漂移）
    set.add("f141e guide drift", !guide_card_matches(&drifted), "阈值漂移检出");

    // 双阈值
    set.add("f141e large 18pt", is_large_text(18, false), "18pt 常规即大字");
    set.add("f141e large 14pt bold", is_large_text(14, true), "14pt 加粗即大字");
    set.add("f141e body 14pt", !is_large_text(14, false), "14pt 常规是正文");
    let dark = (32u8, 32u8, 32u8);
    let white = (255u8, 255u8, 255u8);
    let gray = (140u8, 140u8, 140u8);
    set.add(
        "f141e dual gate body",
        !contrast_for_text(gray, white, 12, false),
        "正文线走 4.5（灰 140 约 3.4:1——正文红）",
    );
    set.add(
        "f141e dual gate large",
        contrast_for_text(gray, white, 18, false),
        "大字线走 3.0（灰 140 过）",
    );
    set.add("f141e dual gate dark", contrast_for_text(dark, white, 12, false), "深字白底正文过");

    // 错例检测
    set.add("f141e ap ring", anti_pattern_invisible_ring(true, 0), "零宽焦点环检出");
    set.add("f141e ap ring ok", !anti_pattern_invisible_ring(true, 2), "正常宽度不误报");
    set.add("f141e ap focus", anti_pattern_lost_focus(RestoreScene::DialogClosed), "焦点丢失检出");
    set.add(
        "f141e ap color only",
        anti_pattern_color_only(SemanticState::Error, false, false),
        "单色相语义检出",
    );

    // 令牌映射
    set.add("f141e ring widths", ring_width_for(true, true) == 4 && ring_width_for(true, false) == 2, "高对比加粗");
    set.add("f141e ring consistent", ring_mapping_consistent(), "映射与门禁联动自洽");

    // 自测片段
    set.add("f141e snippets five", (1..=5).all(|i| snippet_for(i).is_ok()), "五判据片段齐");
    set.add("f141e snippet bounds", snippet_for(0).is_err() && snippet_for(6).is_err(), "越界拒绝");

    // 自测报告
    let report = A11ySelfReport { five: [true, true, true, true, true] };
    set.add("f141e report green", report.all_green() && report.passed_count() == 5, "全绿报告");
    set.add(
        "f141e badge rec",
        report.badge_recommendation().starts_with("全绿"),
        "全绿建议申请",
    );
    let partial = A11ySelfReport { five: [true, false, true, true, true] };
    set.add("f141e report partial", partial.passed_count() == 4 && !partial.all_green(), "四绿一红");
    let mut out = alloc::string::String::new();
    partial.render(&mut out);
    set.add(
        "f141e report render",
        out.contains("判据2 还焦点：红") && out.contains("通过 4/5"),
        "渲染逐行含判定",
    );

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn large_text_matrix() {
        assert!(!is_large_text(13, true)); // 13pt 加粗仍正文
        assert!(is_large_text(20, false));
        assert!(is_large_text(15, true));
        assert!(!is_large_text(17, false));
    }

    #[test]
    fn report_edge_cases() {
        let none = A11ySelfReport { five: [false; 5] };
        assert_eq!(none.passed_count(), 0);
        assert!(none.badge_recommendation().starts_with("未全绿"));
        let mut out = alloc::string::String::new();
        none.render(&mut out);
        assert_eq!(out.matches("：红").count(), 5);
    }

    #[test]
    fn snippet_texts_distinct() {
        let a = snippet_for(1).unwrap();
        let b = snippet_for(3).unwrap();
        assert_ne!(a, b);
    }
}
