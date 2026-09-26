//! 深化层五 · F138 版本发布节奏公开（2026-09-27 深化批次五 · h 层装配）。
//!
//! 装配与跨域接口件：发布日历 → 关于本机页数据行、更新体验面（F122）
//! 的预约可用判定、渠道公告装配（各渠道取各自内容子集）。

use super::f138g::{channel_allows, render_notes, NoteKind};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 关于本机页数据行：当前版本 + 下窗 → 展示行（缺数据显性未登记）
// ---------------------------------------------------------------------------

pub struct AboutFeed {
    pub current_ver: u32,
    pub next_window_day: Option<u32>,
    pub last_release_day: Option<u32>,
}

pub fn about_rows(f: &AboutFeed) -> alloc::vec::Vec<(&'static str, &'static str)> {
    let fmt_day = |d: u32| -> alloc::string::String { alloc::format!("第 {} 天", d) };
    alloc::vec![
        ("当前版本", alloc::format!("v{}", f.current_ver).leak() as &'static str),
        (
            "下个发布窗",
            f.next_window_day.map(|d| fmt_day(d).leak() as &'static str).unwrap_or("未排程"),
        ),
        (
            "上次发布",
            f.last_release_day.map(|d| fmt_day(d).leak() as &'static str).unwrap_or("未登记"),
        ),
    ]
}

// ---------------------------------------------------------------------------
// 更新预约判定：下窗已排程 + 当前版本落后 → 可预约；否则各给原因
// ---------------------------------------------------------------------------

pub enum UpdateOffer {
    Available,
    UpToDate,
    NoWindow,
}

pub fn update_offer(f: &AboutFeed, latest_published: u32) -> (UpdateOffer, &'static str) {
    if f.next_window_day.is_none() {
        return (UpdateOffer::NoWindow, "下个发布窗未排程");
    }
    if f.current_ver >= latest_published {
        return (UpdateOffer::UpToDate, "已是最新版本");
    }
    (UpdateOffer::Available, "可预约在发布窗内安装")
}

// ---------------------------------------------------------------------------
// 渠道公告装配：一次发布 → 各渠道取各自内容子集（矩阵裁决过滤）
// ---------------------------------------------------------------------------

/// 每条公告带内容类型位；渠道只收自己允许的类型（f138g 矩阵复用）。
pub fn channel_digest(
    channel_grants: u8,
    items: &[(u8, &'static str)],
) -> alloc::vec::Vec<&'static str> {
    items
        .iter()
        .filter(|(content, _)| channel_allows(channel_grants, *content))
        .map(|(_, text)| *text)
        .collect()
}

/// 发布说明完整形制校验（复用 g 层渲染器——一处一事实）。
pub fn notes_valid(entries: &[(NoteKind, &'static str)]) -> bool {
    render_notes(entries).is_ok()
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F138H_TAG: &str = "stareco-F138-deep5";

pub fn run_f138_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new(F138H_TAG);

    // 关于页行
    let f = AboutFeed { current_ver: 4, next_window_day: Some(120), last_release_day: Some(30) };
    let rows = about_rows(&f);
    set.add(
        "f138h about rows",
        rows.len() == 3 && rows[0].1 == "v4" && rows[1].1.contains("120"),
        "三行装配",
    );
    let empty = AboutFeed { current_ver: 4, next_window_day: None, last_release_day: None };
    let rows2 = about_rows(&empty);
    set.add(
        "f138h about missing",
        rows2[1].1 == "未排程" && rows2[2].1 == "未登记",
        "缺失显性化",
    );

    // 更新预约
    let (o1, m1) = update_offer(&f, 5);
    let (o2, _) = update_offer(&f, 4);
    let (o3, m3) = update_offer(&empty, 5);
    set.add("f138h offer available", matches!(o1, UpdateOffer::Available) && m1.contains("预约"), "可预约");
    set.add("f138h offer uptodate", matches!(o2, UpdateOffer::UpToDate), "已最新");
    set.add("f138h offer nowindow", matches!(o3, UpdateOffer::NoWindow) && m3.contains("未排程"), "无窗如实");

    // 渠道装配
    const GR_SECURITY: u8 = 2;
    const GR_NOTES: u8 = 1;
    let items = [
        (GR_SECURITY, "TLS 库升级"),
        (GR_NOTES, "跳转清单上线"),
        (4, "博客文"),
    ];
    let sec = channel_digest(GR_SECURITY, &items);
    let notes = channel_digest(GR_NOTES, &items);
    set.add("f138h sec digest", sec == alloc::vec!["TLS 库升级"], "安全渠道子集");
    set.add(
        "f138h notes digest",
        notes == alloc::vec!["跳转清单上线"],
        "说明渠道子集（位含判定）",
    );

    // 发布说明复用
    set.add(
        "f138h notes valid",
        notes_valid(&[(NoteKind::Added, "x")]) && !notes_valid(&[]),
        "g 层渲染器复用",
    );

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn offer_boundary_same_version() {
        let f = AboutFeed { current_ver: 7, next_window_day: Some(9), last_release_day: None };
        let (o, _) = update_offer(&f, 7);
        assert!(matches!(o, UpdateOffer::UpToDate)); // 等版本不算落后
    }
}
