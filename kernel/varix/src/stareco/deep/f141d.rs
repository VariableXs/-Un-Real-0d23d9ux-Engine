//! 深化层 · F141 无障碍开放标准（2026-09-26 回炉补深化）。
//!
//! 补深：Tab 顺序校验（视觉顺序一致）、大字定义（18pt/14pt bold）、
//! 一分钟自测片段模型、错例集数据、门禁判定书输出（逐判据红绿）。

use crate::checks::CheckSet;
use crate::stareco::a11yopen::{a11y_check, A11yInput, MotionPref, SemanticState};

// ---------------------------------------------------------------------------
// Tab 顺序校验（视觉顺序一致）
// ---------------------------------------------------------------------------

/// 声明的 Tab 序（用户按 Tab 走的控件序列）与视觉序（屏幕上从左到右
/// 从上到下）一致性：Tab 序必须与视觉序**完全同路**（同序列）——
/// 键盘用户与视觉用户走同一条路。
pub fn tab_order_ok(visual: &[u32], tab: &[u32]) -> bool {
    if visual.len() != tab.len() || visual.is_empty() {
        return false;
    }
    let mut v = visual.to_vec();
    v.sort_unstable();
    if v.windows(2).any(|w| w[0] == w[1]) {
        return false; // 重复索引
    }
    tab == visual
}

// ---------------------------------------------------------------------------
// 大字定义（WCAG：18pt / 14pt bold）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FontSpec {
    pub pt: u32,
    pub bold: bool,
}

/// 大字判定：≥18pt 或（≥14pt 且 bold）。
pub fn is_large_text(f: FontSpec) -> bool {
    f.pt >= 18 || (f.bold && f.pt >= 14)
}

// ---------------------------------------------------------------------------
// 一分钟自测片段（每节末代码片段——指南同源）
// ---------------------------------------------------------------------------

/// 生成某判据的一分钟自测片段（返回固定文案——与指南同源常量）。
pub fn one_minute_snippet(criterion: &str) -> Option<&'static str> {
    match criterion {
        "focus-ring" => Some("self_test: 控件注册带 ring 令牌；Tab 一圈焦点环可见"),
        "restore-focus" => Some("self_test: 开对话框→Esc→焦点回到触发钮"),
        "contrast" => Some("self_test: 正文色/底色代入对比度公式 ≥4.5"),
        "shape-redundancy" => Some("self_test: 错误态除红外有叉图标或文字"),
        "reduced-motion" => Some("self_test: 减弱档动画时长为完整档 60%"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// 错例集（图解对错对比的数据面）
// ---------------------------------------------------------------------------

/// 错例：对/错描述对（指南错例集章节的固定数据）。
pub const ANTI_PATTERNS: [(&str, &str, &str); 4] = [
    ("焦点环", "为好看删焦点环", "焦点环换高对比令牌，不删除"),
    ("还焦点", "对话框关闭焦点丢失", "记录触发元素，关闭时还原"),
    ("对比度", "浅灰字配白底", "正文对比度 ≥4.5:1"),
    ("色弱", "只用红色标错误", "红 + 叉图标双通道"),
];

// ---------------------------------------------------------------------------
// 门禁判定书（逐判据红绿 + 总裁决）
// ---------------------------------------------------------------------------

/// 输出判定书：五判据逐条结论 + 总裁决（任一红 = 不发徽标）。
pub fn verdict_report(i: &A11yInput) -> ([bool; 5], bool, &'static str) {
    let r = a11y_check(i);
    let all = r.iter().all(|&b| b);
    let word = if all { "PASS" } else { "FAIL" };
    (r, all, word)
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F141D_TAG: &str = "stareco-F141-deep";

pub fn run_f141_deep_checks() -> CheckSet {
    let mut set = CheckSet::new(F141D_TAG);

    // Tab 顺序
    set.add(
        "f141d tab follows visual",
        tab_order_ok(&[1, 2, 3], &[1, 2, 3]) && !tab_order_ok(&[3, 1, 2], &[1, 2, 3]),
        "同路才合规",
    );
    set.add(
        "f141d duplicate index refused",
        !tab_order_ok(&[1, 1, 2], &[1, 1, 2]),
        "重复索引",
    );
    set.add("f141d empty refused", !tab_order_ok(&[], &[]), "空序列");

    // 大字定义
    set.add(
        "f141d large text law",
        is_large_text(FontSpec { pt: 18, bold: false })
            && is_large_text(FontSpec { pt: 14, bold: true })
            && !is_large_text(FontSpec { pt: 14, bold: false })
            && !is_large_text(FontSpec { pt: 16, bold: false }),
        "18pt / 14pt bold",
    );

    // 一分钟自测片段
    set.add(
        "f141d snippets five criteria",
        ["focus-ring", "restore-focus", "contrast", "shape-redundancy", "reduced-motion"]
            .iter()
            .all(|c| one_minute_snippet(c).is_some())
            && one_minute_snippet("nope").is_none(),
        "五节全配",
    );

    // 错例集
    set.add(
        "f141d anti-patterns fixed set",
        ANTI_PATTERNS.len() == 4 && ANTI_PATTERNS.iter().all(|(t, bad, fix)| !t.is_empty() && !bad.is_empty() && !fix.is_empty()),
        "对/错/改法三元",
    );

    // 判定书
    let good = A11yInput {
        ring_token: true,
        ring_width: 3,
        high_contrast: false,
        restore_ok: true,
        fg: (20, 20, 20),
        bg: (250, 250, 250),
        large_text: false,
        state: SemanticState::Success,
        has_shape: true,
        has_text: false,
        motion: MotionPref::Reduced,
        anim_ms: 180,
    };
    let (_, all, word) = verdict_report(&good);
    set.add("f141d verdict pass word", all && word == "PASS", "五绿 PASS");
    let bad = A11yInput { ring_token: false, ..good };
    let (r5, all_bad, word_bad) = verdict_report(&bad);
    set.add(
        "f141d verdict fail word",
        !all_bad && word_bad == "FAIL" && r5[0] == false,
        "首判据红",
    );

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn tab_same_path() {
        assert!(!tab_order_ok(&[2, 1], &[1, 2]));
        assert!(tab_order_ok(&[1, 2], &[1, 2]));
    }
}
