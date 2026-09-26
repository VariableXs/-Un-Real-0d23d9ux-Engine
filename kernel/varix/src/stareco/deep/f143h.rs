//! 深化层五 · F143 星徽与品牌资产包（2026-09-27 深化批次五 · h 层装配）。
//!
//! 装配与跨域接口件：资产包 → 关于本机页展示行（4K 线标注）、第三方
//! 申请页数据装配、色板 → 设置外观页 token 行（F151 联动格式）。

use super::f143f::perceived_luma;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 关于页资产行：资产清单 → 展示行（名称/类型/4K 标注）
// ---------------------------------------------------------------------------

pub struct AssetRow {
    pub name: &'static str,
    pub kind: &'static str,
    pub uhd_note: Option<&'static str>,
}

/// 类型按扩展名分派；4K 线：≥3840 宽的位图资产标「4K 原生」。
pub fn asset_rows(items: &[(&'static str, u32)]) -> alloc::vec::Vec<AssetRow> {
    items
        .iter()
        .map(|(name, width)| {
            let kind = if name.ends_with(".svg") {
                "矢量"
            } else if name.ends_with(".png") {
                "位图"
            } else if name.ends_with(".ico") {
                "图标"
            } else {
                "其他"
            };
            let uhd_note = if *width >= 3840 { Some("4K 原生") } else { None };
            AssetRow { name, kind, uhd_note }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 第三方申请页装配：审批流状态 → 页面行（含拒绝理由显性化）
// ---------------------------------------------------------------------------

pub struct ApplyView {
    pub applicant: &'static str,
    pub stage_label: &'static str,
    pub reason: Option<&'static str>,
}

/// 拒绝理由必须出现在页面上（拒绝不带理由是流程缺陷——H 红线复检）。
pub fn apply_page_rows(cases: &[(u32, &'static str, u8, Option<&'static str>)]) -> alloc::vec::Vec<ApplyView> {
    cases
        .iter()
        .map(|(id, applicant, stage, reason)| {
            let stage_label = match stage {
                0 => "已提交",
                1 => "评审中",
                2 => "已批准",
                3 => "已拒绝",
                _ => "状态未知",
            };
            let _ = id;
            ApplyView { applicant, stage_label, reason: *reason }
        })
        .collect()
}

/// 页面完整性门：拒绝态必须带理由（缺 = 流程账与页面矛盾）。
pub fn rejection_reason_complete(cases: &[(u32, &'static str, u8, Option<&'static str>)]) -> bool {
    cases.iter().all(|(_, _, stage, reason)| {
        *stage != 3 || reason.map(|r| !r.trim().is_empty()).unwrap_or(false)
    })
}

// ---------------------------------------------------------------------------
// 色板 → 外观页 token 行：与 F151 令牌表同构（名/值/亮度档）
// ---------------------------------------------------------------------------

pub fn palette_token_rows(colors: &[(&'static str, [u8; 3])]) -> alloc::vec::Vec<(&'static str, [u8; 3], &'static str)> {
    colors
        .iter()
        .map(|(n, rgb)| {
            let tier = match perceived_luma(*rgb) {
                0..=85 => "深",
                86..=170 => "中",
                _ => "浅",
            };
            (*n, *rgb, tier)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F143H_TAG: &str = "stareco-F143-deep5";

pub fn run_f143_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new(F143H_TAG);

    // 资产行
    let rows = asset_rows(&[
        ("logo-4k.png", 3840),
        ("logo.svg", 0),
        ("mark.ico", 48),
    ]);
    set.add(
        "f143h kinds",
        rows[0].kind == "位图" && rows[1].kind == "矢量" && rows[2].kind == "图标",
        "类型分派",
    );
    set.add(
        "f143h uhd note",
        rows[0].uhd_note == Some("4K 原生") && rows[1].uhd_note.is_none(),
        "4K 线标注",
    );

    // 申请页
    let cases = [
        (1u32, "acme", 2u8, None),
        (2, "evil", 3, Some("将徽标用于商品售卖")),
        (3, "pending", 1, None),
    ];
    let pages = apply_page_rows(&cases);
    set.add(
        "f143h apply labels",
        pages[0].stage_label == "已批准" && pages[2].stage_label == "评审中",
        "状态标签",
    );
    set.add(
        "f143h reject reason shown",
        pages[1].reason.is_some(),
        "拒绝理由上页面",
    );
    set.add("f143h reason gate", rejection_reason_complete(&cases), "理由完整性门");
    let broken = [(9u32, "x", 3u8, None)];
    set.add("f143h reason gate red", !rejection_reason_complete(&broken), "无理由拒绝判红");

    // 色板 token 行
    let tok = palette_token_rows(&[
        ("deep-bg", [10, 14, 18]),
        ("mid-tone", [128, 128, 128]),
        ("paper", [250, 250, 250]),
    ]);
    set.add(
        "f143h token tiers",
        tok[0].2 == "深" && tok[1].2 == "中" && tok[2].2 == "浅",
        "亮度三档",
    );

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn asset_unknown_ext() {
        let rows = asset_rows(&[("weird.xyz", 100)]);
        assert_eq!(rows[0].kind, "其他");
        assert!(rows[0].uhd_note.is_none());
    }

    #[test]
    fn tier_boundaries() {
        let t = palette_token_rows(&[("a", [85, 85, 85]), ("b", [86, 86, 86]), ("c", [170, 170, 170])]);
        assert_eq!(t[0].2, "深");
        assert_eq!(t[1].2, "中");
        assert_eq!(t[2].2, "中");
    }
}
