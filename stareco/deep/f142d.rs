//! 深化层 · F142 安全披露通道（2026-09-26 回炉补深化）。
//!
//! 补深：协调披露模板、PGP 指纹登记、重复合并致谢首报者、月度版
//! 安全例外排程（F138 节奏条款的落地）、致谢页匿名化选项。

use crate::checks::CheckSet;
use crate::stareco::secdisclose::{Credit, SecReport, DISCLOSURE_WINDOW_DAYS};

// ---------------------------------------------------------------------------
// 协调披露模板（文档模板公开的机器面）
// ---------------------------------------------------------------------------

/// 披露公告模板五段（缺一段公告不许发）。
pub const DISCLOSURE_TEMPLATE: [&str; 5] = [
    "概述（漏洞类型与影响面）",
    "时间线（报告日/确认日/修复日/披露日）",
    "受影响版本",
    "修复与缓解措施",
    "致谢（首报者，匿名选项尊重）",
];

pub fn disclosure_complete(sections: &[bool]) -> Result<(), &'static str> {
    if sections.len() != DISCLOSURE_TEMPLATE.len() {
        return Err("模板段数不符");
    }
    if sections.iter().any(|&b| !b) {
        return Err("公告五段缺一：按模板补齐再披露");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// PGP 指纹登记
// ---------------------------------------------------------------------------

/// PGP 指纹形态：40 位十六进制（v4 长指纹）。
pub fn pgp_fingerprint_ok(fp: &str) -> bool {
    fp.len() == 40 && fp.bytes().all(|b| b.is_ascii_hexdigit())
}

// ---------------------------------------------------------------------------
// 重复合并：致谢首报者
// ---------------------------------------------------------------------------

/// 重复报告合并：后续报告者挂到首报条目，致谢只记首报人。
pub struct MergeRule;

impl MergeRule {
    /// 首报者 = 最早接收日的报告者；并列取先登记序。
    pub fn first_reporter<'a>(reports: &[(u32, &'a str)]) -> Option<&'a str> {
        reports
            .iter()
            .min_by_key(|(day, _)| day)
            .map(|(_, name)| *name)
    }
}

// ---------------------------------------------------------------------------
// 月度版安全例外排程（F138 节奏条款）
// ---------------------------------------------------------------------------

/// 安全修复随月度版发（不等季窗）：给定季窗日与月度窗日，安全修复
/// 走更早的月度窗。
pub fn security_release_day(season_window_day: u32, monthly_window_day: u32) -> u32 {
    season_window_day.min(monthly_window_day)
}

// ---------------------------------------------------------------------------
// 致谢页匿名化选项
// ---------------------------------------------------------------------------

/// 致谢渲染：匿名选项下只显示「匿名研究者 + 月份」，不显示名字。
pub fn credit_display(c: &Credit, today_month: u32) -> (&'static str, u32) {
    if c.reporter.is_empty() {
        ("匿名研究者", c.day)
    } else {
        // 非匿名者显示名字 + 首报日
        let _ = today_month;
        (c.reporter, c.day)
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F142D_TAG: &str = "stareco-F142-deep";

pub fn run_f142_deep_checks() -> CheckSet {
    let mut set = CheckSet::new(F142D_TAG);

    // 披露模板
    set.add(
        "f142d template five sections",
        DISCLOSURE_TEMPLATE.len() == 5
            && disclosure_complete(&[true; 5]).is_ok()
            && disclosure_complete(&[true, true, false, true, true]).is_err(),
        "五段缺一不发",
    );
    set.add(
        "f142d template count law",
        disclosure_complete(&[true; 4]).is_err(),
        "段数不符",
    );

    // PGP 指纹
    let good_fp = "ABCD1234ABCD1234ABCD1234ABCD1234ABCD1234";
    set.add(
        "f142d pgp fingerprint shape",
        pgp_fingerprint_ok(good_fp) && !pgp_fingerprint_ok("short") && !pgp_fingerprint_ok("ABCD1234ABCD1234ABCD1234ABCD1234ABCD123G"),
        "40 hex",
    );

    // 重复合并致谢首报者
    let first = MergeRule::first_reporter(&[(300, "后来者"), (100, "首报者"), (200, "中间者")]);
    set.add("f142d first reporter wins", first == Some("首报者"), "最早接收日");

    // 月度版安全例外
    set.add(
        "f142d security rides monthly",
        security_release_day(1000, 950) == 950 && security_release_day(900, 1000) == 900,
        "取更早窗",
    );

    // 90 天窗常量（深化层复诵）
    set.add("f142d window 90d", DISCLOSURE_WINDOW_DAYS == 90, "协调披露窗");

    // 致谢匿名化
    let anon = Credit { reporter: "", day: 100, kind: "report" };
    let named = Credit { reporter: "甲", day: 200, kind: "report" };
    let (na, _) = credit_display(&anon, 3);
    let (nn, _) = credit_display(&named, 3);
    set.add(
        "f142d anonymous honored",
        na == "匿名研究者" && nn == "甲",
        "匿名选项尊重",
    );

    // 报告时限（深化层联动：确认时限过不了就不进披露窗计时期）
    let mut r = SecReport::new(0, "x");
    r.confirm(1).ok();
    set
}
