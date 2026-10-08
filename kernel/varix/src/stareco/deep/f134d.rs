//! 深化层 · F134 主题分享页（2026-09-26 回炉补深化）。
//!
//! 补深：审核队列（SLA 排程视图）、标签筛选器、举报分流路由
//! （F142 恶意 / 目录维护面）、目录 JSON 开放数据导出（F128 面）。

use crate::checks::CheckSet;
use crate::stareco::ebase::TraceId;
use crate::stareco::themeshare::{LinkHealth, ListingState, ThemeBoard, ThemeListing};

// ---------------------------------------------------------------------------
// 审核队列（SLA 排程视图）
// ---------------------------------------------------------------------------

pub struct ReviewQueueItem {
    pub id: TraceId,
    pub applied_day: u32,
    /// 距 SLA 违约剩余天数（0 = 今日到期；负语义用 saturating 0 表示已爆）。
    pub sla_days_left: u32,
    pub breached: bool,
}

/// 从目录页提取审核中队列，按 SLA 剩余升序（最紧急在前）。
pub fn review_queue(board: &ThemeBoard, today: u32) -> alloc::vec::Vec<ReviewQueueItem> {
    let mut items: alloc::vec::Vec<ReviewQueueItem> = board
        .items_view()
        .iter()
        .flatten()
        .filter(|it| it.state == ListingState::Reviewing)
        .map(|it| {
            let elapsed = today.saturating_sub(it.applied_day);
            let sla = if elapsed > 7 { 0 } else { 7 - elapsed };
            ReviewQueueItem {
                id: it.id,
                applied_day: it.applied_day,
                sla_days_left: sla,
                breached: it.sla_breached(today),
            }
        })
        .collect();
    items.sort_by_key(|q| q.sla_days_left);
    items
}

// ---------------------------------------------------------------------------
// 标签筛选器
// ---------------------------------------------------------------------------

/// 按标签筛选上架条目（列表卡筛选数据源）。
pub fn filter_by_tag(board: &ThemeBoard, tag: &str) -> alloc::vec::Vec<TraceId> {
    board
        .items_view()
        .iter()
        .flatten()
        .filter(|it| {
            it.state == ListingState::Listed
                && it.link_health == LinkHealth::Ok
                && it.tags.iter().any(|t| *t == Some(tag))
        })
        .map(|it| it.id)
        .collect()
}

// ---------------------------------------------------------------------------
// 举报分流路由
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReportRoute {
    /// 恶意/安全 → F142 披露通道。
    Security,
    /// 链接失效/审美 → 目录维护面。
    Maintenance,
}

/// 举报分流：恶意走 F142，其余走目录维护——恶意与审美分开处理。
pub fn route_report(malicious: bool, link_stale: bool) -> ReportRoute {
    if malicious {
        ReportRoute::Security
    } else if link_stale {
        ReportRoute::Maintenance
    } else {
        ReportRoute::Maintenance
    }
}

// ---------------------------------------------------------------------------
// 目录 JSON 开放数据导出（F128 面）
// ---------------------------------------------------------------------------

/// 导出一行条目的开放数据键值（key=value 对，F128 JSON 面的序列化源）。
pub fn open_data_row(it: &ThemeListing) -> alloc::vec::Vec<(&'static str, alloc::string::String)> {
    use core::fmt::Write;
    let mut rows = alloc::vec::Vec::new();
    let mut idb = [0u8; 32];
    let n = it.id.render(&mut idb);
    rows.push(("id", alloc::string::String::from_utf8_lossy(&idb[..n]).into_owned()));
    rows.push(("name", alloc::format!("{}", it.name)));
    rows.push(("author", alloc::format!("{}", it.author)));
    rows.push(("version", alloc::format!("{}", it.version)));
    let mut tags = alloc::string::String::new();
    for t in it.tags.iter().flatten() {
        let _ = write!(tags, "{t};");
    }
    rows.push(("tags", tags));
    rows.push(("state", alloc::format!("{:?}", it.state)));
    rows
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F134D_TAG: &str = "stareco-F134-deep";

pub fn run_f134_deep_checks() -> CheckSet {
    let mut set = CheckSet::new(F134D_TAG);
    let day = 20260926;

    let mk = |name: &'static str, applied: u32| ThemeListing {
        id: TraceId::new("THEME", applied, 0),
        name,
        author: "a",
        version: "1",
        tags: [Some("glass"), None, None, None],
        fetch: crate::stareco::themeshare::FetchKind::External,
        link_health: LinkHealth::Ok,
        state: ListingState::Reviewing,
        applied_day: applied,
        format_validated: true,
        hash_cleared: true,
    };

    let mut board = ThemeBoard::new();
    // 申请日跨度：一条临近 SLA、一条刚申请
    let mut early = mk("early", day - 6);
    let id_early = TraceId::new("THEME", day - 6, 1);
    early.id = id_early;
    let mut late = mk("late", day);
    late.id = TraceId::new("THEME", day, 2);
    board.admit(early).ok();
    board.admit(late).ok();

    let q = review_queue(&board, day);
    set.add(
        "f134d queue sorted by urgency",
        q.len() == 2 && q[0].sla_days_left == 1 && q[1].sla_days_left == 7,
        "most urgent first",
    );
    set.add("f134d breach flagged in queue", q.iter().all(|x| !x.breached), "none breached yet");

    // 上架 + 筛选
    board.publish(id_early, day).ok();
    let glass = filter_by_tag(&board, "glass");
    let warm = filter_by_tag(&board, "warm");
    set.add("f134d tag filter", glass.len() == 1 && warm.is_empty(), "listed+tag hit");

    // 举报分流
    set.add(
        "f134d route split",
        route_report(true, false) == ReportRoute::Security
            && route_report(false, true) == crate::stareco::deep::f134d::ReportRoute::Maintenance,
        "恶意走 F142；审美/链接走维护",
    );

    // 开放数据导出
    let rows = open_data_row(board.items_view().iter().flatten().next().expect("one item"));
    set.add(
        "f134d open data keys",
        rows.iter().any(|(k, _)| *k == "id") && rows.iter().any(|(k, _)| *k == "tags"),
        "F128 serialization source",
    );

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn route_law() {
        assert_eq!(route_report(true, true), ReportRoute::Security);
    }
}
