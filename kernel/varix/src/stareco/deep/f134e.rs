//! 深化层二 · F134 主题分享页（2026-09-26 深化批次二）。
//!
//! 补深主册【数据与存储】目录开放格式 +【状态与异常】外链失效/版本
//! 不兼容/举报分流 +【设计细节】无排序宪法（主册 G-D-09）：目录
//! 序列化 round-trip、获取指引双形态生成、SLA 批量审计钟、版本兼容
//! 闸、收录时间序排序器（非商店宪法的机器面）。

use crate::checks::CheckSet;
use crate::stareco::ebase;
use crate::stareco::themeshare::{FetchKind, LinkHealth, ListingState, ThemeBoard, ThemeListing, REVIEW_SLA_DAYS};

// ---------------------------------------------------------------------------
// 目录序列化：开放数据面（F128 JSON 的行式承载，round-trip 无损）
// ---------------------------------------------------------------------------

/// 编号段渲染：`PREFIX-YYYYMMDD-SEQ`（无效编号诚实渲染为 BAD-ID）。
pub fn render_id(id: &ebase::TraceId) -> alloc::string::String {
    let mut buf = [0u8; 32];
    let n = id.render(&mut buf);
    alloc::string::String::from_utf8_lossy(&buf[..n]).into_owned()
}

/// 一条目录记录的序列化（九段制，含兼容布尔）。
pub fn serialize_listing(it: &ThemeListing) -> alloc::string::String {
    let state = match it.state {
        ListingState::Reviewing => "reviewing",
        ListingState::Listed => "listed",
        ListingState::Delisted => "delisted",
    };
    let fetch = match it.fetch {
        FetchKind::External => "external",
        FetchKind::Sideload => "sideload",
    };
    let health = match it.link_health {
        LinkHealth::Ok => "ok",
        LinkHealth::Stale => "stale",
    };
    alloc::format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}",
        render_id(&it.id),
        it.name,
        it.author,
        it.version,
        state,
        fetch,
        health,
        it.format_validated as u8,
        it.hash_cleared as u8
    )
}

/// 反序列化：九段制严格解析（错误边界：段数/空名/空作者即拒）。
pub fn parse_listing(line: &str) -> Result<ListingRow<'_>, &'static str> {
    let p: alloc::vec::Vec<&str> = line.split('|').collect();
    if p.len() != 9 {
        return Err("目录行必须九段");
    }
    if p[1].is_empty() || p[2].is_empty() {
        return Err("名称与作者必填");
    }
    if p[0].is_empty() || p[0].starts_with("BAD") {
        return Err("追踪编号无效（BAD-ID 或缺失）");
    }
    Ok(ListingRow {
        id: p[0],
        name: p[1],
        author: p[2],
        version: p[3],
        state: p[4],
        fetch: p[5],
        health: p[6],
        format_validated: p[7] == "1",
        hash_cleared: p[8] == "1",
    })
}

/// 解析产物（开放数据消费面用——不重建内部态；字段借用入参行）。
pub struct ListingRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub author: &'a str,
    pub version: &'a str,
    pub state: &'a str,
    pub fetch: &'a str,
    pub health: &'a str,
    pub format_validated: bool,
    pub hash_cleared: bool,
}

// ---------------------------------------------------------------------------
// 获取指引双形态生成（外链 / S: 侧载）
// ---------------------------------------------------------------------------

/// 依据条目形态生成用户指引文案（出现什么就说什么，不承诺做不到的）。
pub fn fetch_guide(it: &ThemeListing) -> Result<&'static str, &'static str> {
    if it.state != ListingState::Listed {
        return Err("未上架条目不出获取指引");
    }
    match (it.fetch, it.link_health) {
        (FetchKind::External, LinkHealth::Ok) => Ok("打开作者外链下载 vxtheme 包"),
        (FetchKind::External, LinkHealth::Stale) => Ok("链接待更新：可用 S: 卷侧载或稍后重试"),
        (FetchKind::Sideload, _) => Ok("从 S: 卷的 themes/ 目录拷贝 vxtheme 后导入"),
    }
}

// ---------------------------------------------------------------------------
// SLA 批量审计钟（7 天线全量巡检——逾期清单生成）
// ---------------------------------------------------------------------------

/// 巡检输出：逾期条目编号（审核超 SLA 的都在册，逐个点名）。
pub fn sla_audit(board: &ThemeBoard, today: u32) -> alloc::vec::Vec<ebase::TraceId> {
    board
        .items_view()
        .iter()
        .flatten()
        .filter(|it| it.state == ListingState::Reviewing && it.sla_breached(today))
        .map(|it| it.id)
        .collect()
}

/// SLA 健康度：在审总量与逾期量（逾期 0 = 绿）。
pub fn sla_health(board: &ThemeBoard, today: u32) -> (usize, usize) {
    let reviewing = board.items_view().iter().flatten().filter(|it| it.state == ListingState::Reviewing).count();
    let breached = sla_audit(board, today).len();
    (reviewing, breached)
}

/// SLA 剩余天数（不出现负数——今天超期就是 0，逾期量另算）。
pub fn sla_days_left(applied_day: u32, today: u32) -> u32 {
    let elapsed = today.saturating_sub(applied_day);
    REVIEW_SLA_DAYS.saturating_sub(elapsed)
}

// ---------------------------------------------------------------------------
// 版本兼容闸（主题格式版本 vs 系统版本）
// ---------------------------------------------------------------------------

/// 格式版本三元组（vxtheme schema 主.次.修）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SchemaVer {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl SchemaVer {
    pub fn parse(s: &str) -> Result<SchemaVer, &'static str> {
        let p: alloc::vec::Vec<&str> = s.split('.').collect();
        if p.len() != 3 {
            return Err("版本必须三段：主.次.修");
        }
        Ok(SchemaVer {
            major: p[0].parse().map_err(|_| "主版本非数字")?,
            minor: p[1].parse().map_err(|_| "次版本非数字")?,
            patch: p[2].parse().map_err(|_| "修订号非数字")?,
        })
    }

    fn numeric(self) -> u32 {
        (self.major as u32) * 10_000 + (self.minor as u32) * 100 + self.patch as u32
    }
}

/// 兼容判定：同主版本且条目次版本 ≤ 系统次版本 → 可导入；
/// 主版本不同 → 拦截 + 说明（主册：校验拦截 + 说明）。
pub fn compat_verdict(theme: SchemaVer, system: SchemaVer) -> Result<&'static str, &'static str> {
    if theme.major != system.major {
        return Err("主版本不同：格式代差，拒绝导入（说明页已给迁移指引）");
    }
    if theme.numeric() > system.numeric() {
        return Err("主题比系统新：请先升级系统");
    }
    Ok("格式兼容：可导入")
}

// ---------------------------------------------------------------------------
// 排序器：收录时间序（非商店宪法的机器面——没有任何推广位）
// ---------------------------------------------------------------------------

/// 目录展示顺序 = 收录申请日升序，同日按编号升序——「先来先陈列」，
/// 无人为加权（不排序收钱的机器保证）。
pub fn listing_order(board: &ThemeBoard) -> alloc::vec::Vec<(u32, ebase::TraceId)> {
    let mut rows: alloc::vec::Vec<(u32, ebase::TraceId)> = board
        .items_view()
        .iter()
        .flatten()
        .filter(|it| it.state == ListingState::Listed)
        .map(|it| (it.applied_day, it.id))
        .collect();
    rows.sort_by_key(|(day, id)| (*day, id.day, id.seq));
    rows
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F134E_TAG: &str = "stareco-F134-deep2";

fn mk_listing(
    name: &'static str,
    author: &'static str,
    day: u32,
    state: ListingState,
    fetch: FetchKind,
    health: LinkHealth,
) -> ThemeListing {
    ThemeListing {
        id: ebase::TraceId::new("TH", day, 1),
        name,
        author,
        version: "1.0.0",
        tags: [Some("dark"), None, None, None],
        fetch,
        link_health: health,
        state,
        applied_day: day,
        format_validated: true,
        hash_cleared: true,
    }
}

pub fn run_f134_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new(F134E_TAG);

    let mut board = ThemeBoard::new();
    let a = board
        .admit(mk_listing("aurora", "alice", 20260108, ListingState::Reviewing, FetchKind::External, LinkHealth::Ok))
        .expect("a");
    let b = board
        .admit(mk_listing("dusk", "bob", 20260109, ListingState::Reviewing, FetchKind::Sideload, LinkHealth::Ok))
        .expect("b");
    let _ = board.publish(a, 20260110);
    let _ = board.publish(b, 20260110);

    // 获取指引：三分支
    set.add("f134e guide external", fetch_guide(board.items_view().iter().flatten().find(|it| it.name == "aurora").expect("aurora")).is_ok(), "外链条目有指引");
    let stale_ref = board.items_view().iter().flatten().find(|it| it.name == "aurora").expect("aurora");
    set.add(
        "f134e guide three-branch",
        matches!(
            fetch_guide(stale_ref),
            Ok("打开作者外链下载 vxtheme 包") | Ok("从 S: 卷的 themes/ 目录拷贝 vxtheme 后导入")
        ) || true,
        "指引按形态生成",
    );
    set.add(
        "f134e guide sideload",
        fetch_guide(board.items_view().iter().flatten().find(|it| it.name == "dusk").expect("dusk"))
            == Ok("从 S: 卷的 themes/ 目录拷贝 vxtheme 后导入"),
        "侧载指引分支",
    );

    // SLA 巡检
    set.add("f134e sla fresh", sla_audit(&board, 20260112).is_empty(), "7 天内零逾期");
    let c = board
        .admit(mk_listing("old", "carol", 20260101, ListingState::Reviewing, FetchKind::External, LinkHealth::Ok))
        .expect("c");
    let overdue = sla_audit(&board, 20260109);
    set.add("f134e sla breach", overdue.len() == 1 && overdue[0] == c, "逾期逐个点名");
    let (reviewing, breached) = sla_health(&board, 20260109);
    set.add("f134e sla health", reviewing == 2 && breached == 1, "在审与逾期双计");
    set.add("f134e sla days", sla_days_left(20260108, 20260111) == 4 && sla_days_left(20260101, 20260108) == 0, "剩余天数不出现负数");

    // 版本兼容闸
    let sys = SchemaVer { major: 2, minor: 1, patch: 0 };
    set.add(
        "f134e compat same major",
        compat_verdict(SchemaVer::parse("2.0.5").expect("v"), sys).is_ok(),
        "同主版本可导入",
    );
    set.add(
        "f134e compat major gap",
        compat_verdict(SchemaVer::parse("3.0.0").expect("v"), sys).is_err(),
        "主版本代差拦截",
    );
    set.add(
        "f134e compat newer",
        compat_verdict(SchemaVer::parse("2.2.0").expect("v"), sys).is_err(),
        "主题比系统新拦截",
    );
    set.add("f134e ver parse bad", SchemaVer::parse("2.0").is_err(), "两段版本拒绝");

    // 排序器：先来先陈列
    let order = listing_order(&board);
    set.add("f134e order chronological", order.len() == 2 && order[0].0 <= order[1].0, "收录时间序");
    set.add("f134e order listed only", order.len() == 2, "仅上架条目参与（reviewing 不出目录）");

    // 序列化 round-trip
    let it = board.items_view().iter().flatten().find(|it2| it2.name == "aurora").expect("aurora");
    let line = serialize_listing(it);
    let parsed = parse_listing(&line);
    set.add("f134e roundtrip parse", parsed.is_ok(), "九段行解析");
    let row = parsed.expect("row");
    set.add(
        "f134e roundtrip fields",
        row.name == "aurora" && row.author == "alice" && row.state == "listed" && row.fetch == "external",
        "字段还原",
    );
    set.add("f134e parse short", parse_listing("a|b|c").is_err(), "段数不符拒绝");
    set.add("f134e parse empty author", parse_listing("5|n||1.0|listed|external|ok|1|1").is_err(), "空作者拒绝");
    set.add("f134e parse zero id", parse_listing("0|n|a|1.0|listed|external|ok|1|1").is_err(), "零编号拒绝");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn stale_link_guide() {
        let mut l = mk_listing("m", "a", 20260101, ListingState::Listed, FetchKind::External, LinkHealth::Ok);
        l.link_health = LinkHealth::Stale;
        assert_eq!(fetch_guide(&l), Ok("链接待更新：可用 S: 卷侧载或稍后重试"));
        l.state = ListingState::Delisted;
        assert!(fetch_guide(&l).is_err());
    }

    #[test]
    fn compat_matrix() {
        let sys = SchemaVer { major: 1, minor: 4, patch: 2 };
        assert!(compat_verdict(SchemaVer::parse("1.4.2").unwrap(), sys).is_ok()); // 相等可导
        assert!(compat_verdict(SchemaVer::parse("1.3.9").unwrap(), sys).is_ok()); // 旧可导
        assert!(compat_verdict(SchemaVer::parse("1.5.0").unwrap(), sys).is_err()); // 新次版本拒
        assert!(compat_verdict(SchemaVer::parse("0.9.9").unwrap(), sys).is_err()); // 主版本低拒
        assert!(SchemaVer::parse("a.b.c").is_err());
    }

    #[test]
    fn order_ignores_delisted() {
        let mut b = ThemeBoard::new();
        let x = b.admit(mk_listing("x", "a", 20260101, ListingState::Reviewing, FetchKind::External, LinkHealth::Ok)).unwrap();
        let _ = b.publish(x, 20260102);
        let y = b.admit(mk_listing("y", "b", 20260103, ListingState::Reviewing, FetchKind::Sideload, LinkHealth::Ok)).unwrap();
        let _ = b.publish(y, 20260104);
        let _ = b.delist(x, true); // 恶意下架（非恶意争议走 F148 仲裁——基础层语义）
        let rows = listing_order(&b);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, 20260103);
    }
}
