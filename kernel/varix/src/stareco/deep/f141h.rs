//! 深化层五 · F141 无障碍开放标准（2026-09-27 深化批次五 · h 层装配）。
//!
//! 装配与跨域接口件：无障碍设置页装配（各开关 → 行 + 联动引擎状态
//! 查询）、对照检查结果 → 系统通知的人话行。

use super::f141f::{alt_text_issue, contrast_ge_45};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 设置页装配：开关位 → 设置行（开/关 + 联动说明）
// ---------------------------------------------------------------------------

pub struct A11ySwitch {
    pub key: &'static str,
    pub on: bool,
}

pub struct A11yRow {
    pub key: &'static str,
    pub state_label: &'static str,
    pub links_engine: &'static str,
}

/// 联动说明：每个开关接到哪个引擎（跨域可见性——用户知道改了影响啥）。
pub fn settings_rows(sw: &[A11ySwitch]) -> alloc::vec::Vec<A11yRow> {
    sw.iter()
        .map(|s| A11yRow {
            key: s.key,
            state_label: if s.on { "已开启" } else { "已关闭" },
            links_engine: match s.key {
                "reduce_motion" => "动效降级引擎（F245）",
                "high_contrast" => "对比度判定引擎（F113/F141）",
                "large_text" => "字号缩放引擎（F246）",
                "screen_reader" => "语义树播报（F385）",
                _ => "未知开关",
            },
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 对照检查 → 通知行：批量对比度/替代文本结果的人话化
// ---------------------------------------------------------------------------

pub fn contrast_notice(_name: &str, fg: [u8; 3], bg: [u8; 3]) -> (bool, &'static str) {
    if contrast_ge_45(fg, bg) {
        (true, "对比度达标")
    } else {
        (false, "对比度不足：请换用推荐前景色（设置页有安全色建议）")
    }
}

pub fn alt_notice(alt: &str) -> (bool, &'static str) {
    match alt_text_issue(alt) {
        None => (true, "替代文本合格"),
        Some(_) => (false, "替代文本不合格：需描述图像内容而非占位词"),
    }
}

// ---------------------------------------------------------------------------
// 无障碍健康分：五项检查各 20 分（整数口径，确定性）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct A11yAudit {
    pub focus_visible: bool,
    pub contrast_ok: bool,
    pub alt_ok: bool,
    pub motion_alt_ok: bool,
    pub touch_ok: bool,
}

pub fn health_score(a: &A11yAudit) -> u32 {
    let mut s = 0;
    if a.focus_visible {
        s += 20;
    }
    if a.contrast_ok {
        s += 20;
    }
    if a.alt_ok {
        s += 20;
    }
    if a.motion_alt_ok {
        s += 20;
    }
    if a.touch_ok {
        s += 20;
    }
    s
}

pub fn health_label(score: u32) -> &'static str {
    match score {
        100 => "全绿",
        80..=99 => "良好",
        40..=79 => "需改进",
        _ => "不合格",
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F141H_TAG: &str = "stareco-F141-deep5";

pub fn run_f141_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new(F141H_TAG);

    // 设置行
    let sw = [
        A11ySwitch { key: "reduce_motion", on: true },
        A11ySwitch { key: "high_contrast", on: false },
        A11ySwitch { key: "mystery", on: true },
    ];
    let rows = settings_rows(&sw);
    set.add(
        "f141h rows",
        rows[0].state_label == "已开启" && rows[1].state_label == "已关闭",
        "状态标签",
    );
    set.add(
        "f141h engine links",
        rows[0].links_engine.contains("F245") && rows[1].links_engine.contains("F113"),
        "联动引擎指名",
    );
    set.add(
        "f141h unknown switch",
        rows[2].links_engine == "未知开关",
        "未知开关不冒领引擎",
    );

    // 对照通知
    let (ok1, m1) = contrast_notice("btn", [0x33, 0x33, 0x33], [255, 255, 255]);
    let (ok2, m2) = contrast_notice("btn", [0x77, 0x77, 0x77], [255, 255, 255]);
    set.add(
        "f141h contrast notice",
        ok1 && m1.contains("达标") && !ok2 && m2.contains("安全色建议"),
        "三要素建议",
    );
    let (ok3, _m3) = alt_notice("折线图：营收上行");
    let (ok4, m4) = alt_notice("图片");
    set.add(
        "f141h alt notice",
        ok3 && !ok4 && m4.contains("占位词"),
        "替代文本通知",
    );

    // 健康分
    let full = A11yAudit {
        focus_visible: true,
        contrast_ok: true,
        alt_ok: true,
        motion_alt_ok: true,
        touch_ok: true,
    };
    let partial = A11yAudit { focus_visible: false, ..full };
    set.add(
        "f141h score",
        health_score(&full) == 100 && health_score(&partial) == 80,
        "五项各 20 分",
    );
    set.add(
        "f141h labels",
        health_label(100) == "全绿" && health_label(80) == "良好" && health_label(20) == "不合格",
        "分档标签",
    );

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn score_zero_and_bounds() {
        let zero = A11yAudit {
            focus_visible: false,
            contrast_ok: false,
            alt_ok: false,
            motion_alt_ok: false,
            touch_ok: false,
        };
        assert_eq!(health_score(&zero), 0);
        assert_eq!(health_label(40), "需改进");
    }
}
