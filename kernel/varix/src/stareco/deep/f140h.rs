//! 深化层五 · F140 本地化开放（2026-09-27 深化批次五 · h 层装配）。
//!
//! 装配与跨域接口件：语言热切（F398）可用语言清单装配、缺失词条
//! 降级策略行（回退链人话化）、词条覆盖徽标数据。

use super::f140g::CoverageCell;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 热切可用语言清单：覆盖地图 → 可切语言行（≥800‰ 全域才可热切）
// ---------------------------------------------------------------------------

pub struct LangOption {
    pub lang: &'static str,
    pub native_label: &'static str,
    pub ready: bool,
}

const NATIVE: [(&str, &str); 4] = [
    ("zh", "简体中文"),
    ("en", "English"),
    ("ru", "Русский"),
    ("ko", "한국어"),
];

pub fn hot_switch_options(cells: &[CoverageCell]) -> alloc::vec::Vec<LangOption> {
    NATIVE
        .iter()
        .map(|(lang, native)| {
            let doms: alloc::vec::Vec<u32> =
                cells.iter().filter(|c| c.lang == *lang).map(|c| c.per_mille).collect();
            let ready = !doms.is_empty() && doms.iter().all(|p| *p >= 800);
            LangOption { lang, native_label: native, ready }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 缺失词条降级行：回退链的人话化（用户看得见的降级要说明白）
// ---------------------------------------------------------------------------

pub struct FallbackRow {
    pub key: &'static str,
    pub action: &'static str,
}

/// 策略：目标语言缺 → 英文兜底；英文也缺 → 键名直出（开发者可见）。
/// 永远不给空串——空串是静默。
pub fn fallback_rows(missing_keys: &[&'static str], en_missing: &[&'static str]) -> alloc::vec::Vec<FallbackRow> {
    missing_keys
        .iter()
        .map(|k| {
            if en_missing.contains(k) {
                FallbackRow { key: k, action: "英文缺失：显示键名（开发者可见）" }
            } else {
                FallbackRow { key: k, action: "回退英文显示" }
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 覆盖徽标数据：<90% 测试版徽标（f140 口径延续）
// ---------------------------------------------------------------------------

pub fn badge_for(per_mille: u32) -> Option<&'static str> {
    if per_mille >= 1000 {
        None
    } else if per_mille >= 900 {
        Some("正式版")
    } else {
        Some("测试版")
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F140H_TAG: &str = "stareco-F140-deep5";

pub fn run_f140_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new(F140H_TAG);

    // 热切清单
    let cells = [
        CoverageCell { lang: "zh", domain: 1, per_mille: 1000 },
        CoverageCell { lang: "zh", domain: 2, per_mille: 950 },
        CoverageCell { lang: "ko", domain: 1, per_mille: 600 },
    ];
    let opts = hot_switch_options(&cells);
    set.add(
        "f140h options count",
        opts.len() == 4,
        "登记语言全列（含未就绪）",
    );
    let zh = opts.iter().find(|o| o.lang == "zh").expect("zh");
    let ko = opts.iter().find(|o| o.lang == "ko").expect("ko");
    set.add(
        "f140h ready flags",
        zh.ready && !ko.ready && ko.native_label == "한국어",
        "就绪位+母语标签",
    );
    // en 无登记 → 不就绪（零域覆盖）。
    let en = opts.iter().find(|o| o.lang == "en").expect("en");
    set.add("f140h zero domain", !en.ready, "零覆盖语言不就绪");

    // 降级行
    let rows = fallback_rows(&["ui.open", "ui.deep"], &["ui.deep"]);
    set.add(
        "f140h fallback",
        rows[0].action.contains("英文") && rows[1].action.contains("键名"),
        "两级降级人话",
    );
    set.add(
        "f140h no empty",
        rows.iter().all(|r| !r.action.is_empty()),
        "零空串（不静默）",
    );

    // 覆盖徽标
    set.add(
        "f140h badges",
        badge_for(1000).is_none()
            && badge_for(950) == Some("正式版")
            && badge_for(500) == Some("测试版"),
        "三档徽标",
    );

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn native_labels_cover_all() {
        // NATIVE 表必须覆盖热切登记的四个语言（漏一个 = 界面出英文键）。
        for lang in ["zh", "en", "ru", "ko"] {
            assert!(NATIVE.iter().any(|(l, _)| *l == lang), "{} 缺母语标签", lang);
        }
    }

    #[test]
    fn fallback_dedup_not_required() {
        // 重复缺失键如实重复上报（调用方去重）。
        let rows = fallback_rows(&["k", "k"], &[]);
        assert_eq!(rows.len(), 2);
    }
}
