//! 深化层三 · F141 无障碍开放标准（2026-09-26 深化批次三）。
//!
//! 补深工具化判定面（主册 G-D-16）：对比度安全前景色推荐（候选集内
//! 选最优）、焦点顺序遍历验证器（行主序 vs 声明序逐项对拍）、替代
//! 文本审计（空/超长/占位词三检）、批量对比度审计（失败清单输出）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// WCAG 2.1 相对亮度（f64 直算——与 a11yopen 同口径）
// ---------------------------------------------------------------------------

fn channel_lin(c: u8) -> f64 {
    let c = c as f64 / 255.0;
    if c <= 0.03928 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub fn relative_luminance(rgb: [u8; 3]) -> f64 {
    0.2126 * channel_lin(rgb[0]) + 0.7152 * channel_lin(rgb[1]) + 0.0722 * channel_lin(rgb[2])
}

pub fn contrast_ratio(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

pub fn contrast_ge_45(a: [u8; 3], b: [u8; 3]) -> bool {
    contrast_ratio(a, b) >= 4.5
}

// ---------------------------------------------------------------------------
// 安全前景色推荐：候选集内选对比度最高者（全不达标也要给最优+警示）
// ---------------------------------------------------------------------------

/// 返回 (选中的候选序号, 是否达标)。空候选集拒绝。
pub fn best_foreground(bg: [u8; 3], candidates: &[[u8; 3]]) -> Result<(usize, bool), &'static str> {
    if candidates.is_empty() {
        return Err("候选前景色集为空");
    }
    let mut best = 0usize;
    let mut best_r = 0.0f64;
    for (i, c) in candidates.iter().enumerate() {
        let r = contrast_ratio(bg, *c);
        if r > best_r {
            best_r = r;
            best = i;
        }
    }
    Ok((best, best_r >= 4.5))
}

// ---------------------------------------------------------------------------
// 焦点顺序验证：行主序（y 主排序、x 次排序）vs 声明序逐项对拍
// ---------------------------------------------------------------------------

pub struct FocusWidget {
    pub name: &'static str,
    pub x: u32,
    pub y: u32,
}

/// 期望 Tab 序：行主序。返回声明序错位名单（视觉顺序与焦点序一致
/// 是键盘可达的硬判据）。
pub fn focus_order_violations(declared: &[FocusWidget]) -> alloc::vec::Vec<&'static str> {
    let n = declared.len();
    let mut order: alloc::vec::Vec<usize> = (0..n).collect();
    for i in 1..order.len() {
        let k = order[i];
        let mut j = i;
        while j > 0 {
            let a = &declared[order[j - 1]];
            let b = &declared[k];
            if (a.y, a.x) > (b.y, b.x) {
                order[j] = order[j - 1];
                j -= 1;
            } else {
                break;
            }
        }
        order[j] = k;
    }
    order
        .iter()
        .enumerate()
        .filter(|(pos, idx)| *pos != **idx)
        .map(|(_, idx)| declared[*idx].name)
        .collect()
}

// ---------------------------------------------------------------------------
// 替代文本审计：非空 / ≤80 字节 / 不以占位词开头
// ---------------------------------------------------------------------------

pub const ALT_PLACEHOLDERS: [&str; 3] = ["图片", "image", "pic"];

pub fn alt_text_issue(alt: &str) -> Option<&'static str> {
    if alt.trim().is_empty() {
        return Some("替代文本为空：读屏将跳过图像");
    }
    if alt.len() > 80 {
        return Some("替代文本超 80 字节：读屏体验劣化");
    }
    for p in ALT_PLACEHOLDERS.iter() {
        if alt.starts_with(p) {
            return Some("替代文本以占位词开头：未描述图像内容");
        }
    }
    None
}

// ---------------------------------------------------------------------------
// 批量对比度审计：文本/背景对序列 → 失败清单
// ---------------------------------------------------------------------------

pub fn batch_contrast_failures(pairs: &[(&'static str, [u8; 3], [u8; 3])]) -> alloc::vec::Vec<&'static str> {
    pairs
        .iter()
        .filter(|(_, fg, bg)| !contrast_ge_45(*fg, *bg))
        .map(|(name, _, _)| *name)
        .collect()
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F141F_TAG: &str = "stareco-F141-deep3";

pub fn run_f141_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new(F141F_TAG);

    // 亮度与对比度（基础口径复证）
    let white = [255u8, 255, 255];
    let black = [0u8, 0, 0];
    set.add("f141f max contrast", contrast_ratio(white, black) > 20.0, "黑白 21:1 上界");
    set.add("f141f same color", contrast_ratio(white, white) < 1.01, "同色 1:1 下界");

    // 安全前景推荐
    let cands = [
        [0x88u8, 0x88, 0x88], // 白底上 4.48 不达标
        [0x33u8, 0x33, 0x33],
        [0x00u8, 0x00, 0x00],
    ];
    let (pick, ok) = best_foreground(white, &cands).expect("non-empty");
    set.add("f141f best pick", pick == 2 && ok, "白底选黑且达标");
    let bad_cands = [[0x77u8, 0x77, 0x77], [0x99u8, 0x99, 0x99]];
    let (_, ok2) = best_foreground(white, &bad_cands).expect("non-empty");
    set.add("f141f best warn", !ok2, "全不达标仍给最优+警示");
    set.add("f141f empty cands", best_foreground(white, &[]).is_err(), "空候选拒绝");

    // 焦点顺序
    let good = [
        FocusWidget { name: "a", x: 0, y: 0 },
        FocusWidget { name: "b", x: 100, y: 0 },
        FocusWidget { name: "c", x: 0, y: 50 },
    ];
    set.add("f141f focus clean", focus_order_violations(&good).is_empty(), "行主序声明零错位");
    let bad_order = [
        FocusWidget { name: "b", x: 100, y: 0 },
        FocusWidget { name: "a", x: 0, y: 0 },
    ];
    set.add(
        "f141f focus violation",
        focus_order_violations(&bad_order) == alloc::vec!["a", "b"],
        "错位名单输出",
    );

    // 替代文本
    set.add("f141f alt empty", alt_text_issue("  ").is_some(), "空白替代拒");
    set.add("f141f alt placeholder", alt_text_issue("图片").is_some(), "占位词拒");
    set.add("f141f alt long", alt_text_issue(&"x".repeat(81)).is_some(), "超长拒");
    set.add("f141f alt ok", alt_text_issue("折线图：三季度营收上行").is_none(), "合格替代通过");
    set.add("f141f alt ok en", alt_text_issue("chart of revenue").is_none(), "英文合格通过");

    // 批量审计
    let pairs = [
        ("btn", [0x33u8, 0x33, 0x33], white),
        ("muted", [0x77u8, 0x77, 0x77], white),
    ];
    set.add(
        "f141f batch fail",
        batch_contrast_failures(&pairs) == alloc::vec!["muted"],
        "失败清单只含不达标者",
    );
    set.add(
        "f141f batch pass",
        batch_contrast_failures(&[("ok", black, white)]).is_empty(),
        "全达标零失败",
    );

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn contrast_known_values() {
        // #777 on white ≈ 4.48（不达标）——主册判据的复证锚点。
        let g = [0x77u8, 0x77, 0x77];
        let r = contrast_ratio([255, 255, 255], g);
        assert!(r > 4.4 && r < 4.5);
        assert!(!contrast_ge_45([255, 255, 255], g));
        // #333 on white > 12:1。
        assert!(contrast_ratio([255, 255, 255], [0x33, 0x33, 0x33]) > 12.0);
    }

    #[test]
    fn focus_row_major_tie() {
        // 同行同 y 时按 x 升序。
        let w = [
            FocusWidget { name: "r", x: 50, y: 0 },
            FocusWidget { name: "l", x: 10, y: 0 },
        ];
        assert_eq!(focus_order_violations(&w), alloc::vec!["l", "r"]);
        // 空表零违例。
        assert!(focus_order_violations(&[]).is_empty());
    }
}
