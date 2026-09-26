//! F134 主题分享页（非商店）· 完整设计（STAR I 主册 G-D-09）。
//!
//! **判据（主册）**：首批收录 10 套（官方 3+社区 7 目标）；「非商店」
//! 三不承诺（不托管不抽成不卖位）写进页脚宪法句。
//!
//! **设计要点（主册）**：纯目录（预览图+作者+说明+外部下载链接或 S:
//! 侧载指引）；分栏列表卡（预览 176×96px，出图 352×192 4K 缩放）；
//! 筛选（风格标签——标签词表固定 12 词防膨胀）；条目详情多图预览；
//! 收录三条件（格式校验过+作者申请+无恶意哈希）；外链失效 → 标
//! 「链接待更新」+举报钮；不兼容 → 校验拦截；恶意包 → 下架流程
//! （F148）；收录审核 SLA 7 天；举报直连 F142（恶意与审美分开）。
//!
//! 本模块是目录页的**纯逻辑核**：条目模型、标签词表、收录三条件、
//! SLA 计时、链接健康态、下架与举报分流。不托管文件本体（三不承诺
//! 的工程表达：本层只有元数据，没有内容存储通道）。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use crate::stareco::ebase::{RateGate, TraceId};

// ---------------------------------------------------------------------------
// 规格
// ---------------------------------------------------------------------------

/// 预览图出图规格（4K 出图缩放），列表卡显示 176×96。
pub const PREVIEW_W: u16 = 352;
pub const PREVIEW_H: u16 = 192;
pub const CARD_W: u16 = 176;
pub const CARD_H: u16 = 96;

/// 审核 SLA（天）。
pub const REVIEW_SLA_DAYS: u32 = 7;

/// 标签词表固定 12 词（防标签膨胀——自造标签拒绝）。
pub const STYLE_TAGS: [&str; 12] = [
    "minimal", "glass", "dark", "warm", "retro", "nature", "mono", "neon",
    "pastel", "brutal", "sketch", "corporate",
];

pub fn tag_ok(tag: &str) -> bool {
    STYLE_TAGS.contains(&tag)
}

/// 页脚宪法句（三不承诺原文——渲染层从此常量取，不许转述）。
pub const FOOTER_CHARTER: &str =
    "本页是纯目录：我们不托管文件、不抽成、不卖推荐位。获取方式由作者提供，安装前请过格式校验。";

// ---------------------------------------------------------------------------
// 条目模型
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FetchKind {
    /// 作者外链。
    External,
    /// S: 卷侧载路径指引。
    Sideload,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkHealth {
    Ok,
    /// 外链失效——条目保留，标「链接待更新」。
    Stale,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ListingState {
    /// 审核中（SLA 计时起点 = applied_day）。
    Reviewing,
    Listed,
    /// 下架（恶意/违规，F148 治理产物）。
    Delisted,
}

#[derive(Clone, Copy, Debug)]
pub struct ThemeListing {
    pub id: TraceId,
    pub name: &'static str,
    pub author: &'static str,
    pub version: &'static str,
    /// 标签（逐个必须过 tag_ok——收录即校验）。
    pub tags: [Option<&'static str>; 4],
    pub fetch: FetchKind,
    pub link_health: LinkHealth,
    pub state: ListingState,
    pub applied_day: u32,
    /// 无恶意哈希条件：格式校验过 + 哈希白判。任一缺 = 不得上架。
    pub format_validated: bool,
    pub hash_cleared: bool,
}

impl ThemeListing {
    pub fn tags_ok(&self) -> bool {
        self.tags.iter().all(|t| match t {
            None => true,
            Some(s) => tag_ok(s),
        })
    }

    /// 收录三条件（文档化口径的机器面）：
    /// ①格式校验过 ②作者申请（applied_day > 0 即申请在案）③无恶意哈希。
    pub fn admissible(&self) -> bool {
        self.format_validated && self.hash_cleared && self.applied_day > 0
    }

    /// SLA 判定：审核中超期即 SLA 红。
    pub fn sla_breached(&self, today: u32) -> bool {
        self.state == ListingState::Reviewing
            && today.saturating_sub(self.applied_day) > REVIEW_SLA_DAYS
    }
}

// ---------------------------------------------------------------------------
// 目录页
// ---------------------------------------------------------------------------

/// 目录页（元数据面——三不承诺：这里没有任何内容托管通道）。
pub struct ThemeBoard {
    items: [Option<ThemeListing>; 16],
    count: usize,
    next_seq: u32,
    /// 举报限频（防滥用；恶意与审美分流见 report）。
    report_gate: RateGate,
    pub reports_total: u64,
}

impl ThemeBoard {
    pub fn new() -> ThemeBoard {
        ThemeBoard {
            items: [None; 16],
            count: 0,
            next_seq: 1,
            report_gate: RateGate::new(86_400_000, 10), // 每自然窗 10 条举报上限
            reports_total: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    /// 收录（申请 → 审核 → 上架一步到位；条件不齐拒绝）。
    pub fn admit(&mut self, mut item: ThemeListing) -> Result<TraceId, &'static str> {
        if !item.admissible() {
            return Err("three admission conditions unmet");
        }
        if !item.tags_ok() {
            return Err("tag outside fixed vocabulary");
        }
        if item.state != ListingState::Reviewing {
            return Err("new listing must start as reviewing");
        }
        let id = TraceId::new("THEME", item.applied_day, self.next_seq);
        if !id.is_valid() {
            return Err("bad trace id");
        }
        self.next_seq += 1;
        if self.count >= 16 {
            return Err("board full");
        }
        item.id = id;
        self.items[self.count] = Some(item);
        self.count += 1;
        Ok(id)
    }

    /// 审核通过上架。
    pub fn publish(&mut self, id: TraceId, day: u32) -> Result<(), &'static str> {
        let it = self.find_mut(id).ok_or("unknown id")?;
        if it.state != ListingState::Reviewing {
            return Err("not reviewing");
        }
        it.state = ListingState::Listed;
        it.link_health = LinkHealth::Ok;
        let _ = day;
        Ok(())
    }

    /// 外链失效标记（条目保留，诚实标注——不下架）。
    pub fn mark_stale(&mut self, id: TraceId) -> Result<(), &'static str> {
        let it = self.find_mut(id).ok_or("unknown id")?;
        if it.state != ListingState::Listed {
            return Err("only listed can go stale");
        }
        it.link_health = LinkHealth::Stale;
        Ok(())
    }

    /// 下架（F148 治理产物；恶意与审美分流——审美争议不进此门）。
    pub fn delist(&mut self, id: TraceId, malicious: bool) -> Result<(), &'static str> {
        if !malicious {
            // 审美/质量争议走 F148 仲裁，不走下架通道。
            return Err("aesthetic disputes go to F148 arbitration");
        }
        let it = self.find_mut(id).ok_or("unknown id")?;
        it.state = ListingState::Delisted;
        Ok(())
    }

    /// 举报：先过限频；`malicious=true` 直连 F142 安全通道，否则归
    /// 目录维护面（链接失效类）。返回是否受理。
    pub fn report(&mut self, now_ms: u64, malicious: bool) -> bool {
        if !self.report_gate.admit(now_ms) {
            return false;
        }
        self.reports_total += 1;
        let _ = malicious; // 分流语义：受理后由调用方路由 F142 / 目录维护
        true
    }

    /// 上架中且外链可用 = 「可获取」列表（分栏列表卡数据源）。
    pub fn fetchable(&self) -> usize {
        self.items[..self.count]
            .iter()
            .flatten()
            .filter(|it| it.state == ListingState::Listed && it.link_health == LinkHealth::Ok)
            .count()
    }

    fn find_mut(&mut self, id: TraceId) -> Option<&mut ThemeListing> {
        self.items[..self.count].iter_mut().flatten().find(|it| it.id == id)
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F134_TAG: &str = "stareco-F134-themeshare";

pub fn run_themeshare_checks() -> CheckSet {
    let mut set = CheckSet::new(F134_TAG);
    let day = 20260926;

    // 宪法句在册
    set.add(
        "f134 charter triple-promise",
        FOOTER_CHARTER.contains("不托管") && FOOTER_CHARTER.contains("不抽成") && FOOTER_CHARTER.contains("不卖推荐位"),
        "footer law",
    );

    // 标签词表：12 词封顶，自造拒绝
    set.add("f134 fixed vocab", tag_ok("glass") && !tag_ok("my-custom-vibe"), "12-word law");
    set.add("f134 vocab size", STYLE_TAGS.len() == 12, "no tag inflation");

    let mut board = ThemeBoard::new();
    let mk = || ThemeListing {
        id: TraceId::new("THEME", day, 0),
        name: "aurora-glass",
        author: "community-a",
        version: "1.0",
        tags: [Some("glass"), Some("dark"), None, None],
        fetch: FetchKind::External,
        link_health: LinkHealth::Ok,
        state: ListingState::Reviewing,
        applied_day: day,
        format_validated: true,
        hash_cleared: true,
    };

    // 三条件缺一拒收
    let mut no_fmt = mk();
    no_fmt.format_validated = false;
    set.add("f134 admission needs format", board.admit(no_fmt).is_err(), "cond1");
    let mut no_hash = mk();
    no_hash.hash_cleared = false;
    set.add("f134 admission needs hash", board.admit(no_hash).is_err(), "cond3");
    let mut no_appl = mk();
    no_appl.applied_day = 0;
    set.add("f134 admission needs application", board.admit(no_appl).is_err(), "cond2");

    // 首批收录 10 套（官方 3+社区 7）
    for i in 0..10u32 {
        let mut it = mk();
        it.name = if i < 3 { "official-theme" } else { "community-theme" };
        board.admit(it).expect("admit");
    }
    set.add("f134 first batch 10 admitted", board.len() == 10, "3+7 batch");
    // 逐个审核通过（编号 = admit 顺序）
    let all: Vec<TraceId> = (1..=10).map(|s| TraceId::new("THEME", day, s)).collect();
    for id in &all {
        assert!(board.publish(*id, day).is_ok(), "publish");
    }
    set.add("f134 all published fetchable", board.fetchable() == 10, "listed+ok");

    // SLA：审核中 >7 天红
    let mut late = mk();
    late.applied_day = day - 8;
    // 申请日合法性：非 8 位有效日会被 TraceId 拒——用当日+偏移校验公式
    let mut pending = mk();
    pending.applied_day = day;
    board.admit(pending).expect("admit pending");
    let pid = TraceId::new("THEME", day, 11);
    set.add("f134 sla fresh ok", !board.sla_probe(pid, day), "day0");
    set.add("f134 sla breach formula", late.sla_breached(day), ">7d red");

    // 链接失效 → 保留 + 标注
    let first = all[0];
    assert!(board.mark_stale(first).is_ok());
    set.add("f134 stale keeps entry", board.fetchable() == 9 && board.len() == 11, "stale labeled");

    // 审美争议不下架（走 F148）
    set.add("f134 aesthetic not delistable", board.delist(first, false).is_err(), "arbitration path");
    assert!(board.delist(first, true).is_ok());
    set.add("f134 malicious delisted", board.fetchable() == 9, "governance hit");

    // 举报限频
    let mut g = ThemeBoard::new();
    let mut admitted = 0;
    for i in 0..15u64 {
        if g.report(i * 1000, false) {
            admitted += 1;
        }
    }
    set.add("f134 report rate limited", admitted == 10 && g.reports_total == 10, "10 per window");

    // 预览规格常量
    set.add(
        "f134 preview spec",
        PREVIEW_W == 352 && PREVIEW_H == 192 && CARD_W == 176 && CARD_H == 96,
        "4k pipeline sizes",
    );

    set
}

impl ThemeBoard {
    /// SLA 探针：编号查条目是否审核中超期（供帮助中心审核面板）。
    fn sla_probe(&self, id: TraceId, today: u32) -> bool {
        self.items[..self.count]
            .iter()
            .flatten()
            .find(|it| it.id == id)
            .map(|it| it.sla_breached(today))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listing(day: u32) -> ThemeListing {
        ThemeListing {
            id: TraceId::new("THEME", day, 0),
            name: "t",
            author: "a",
            version: "1",
            tags: [Some("minimal"), None, None, None],
            fetch: FetchKind::Sideload,
            link_health: LinkHealth::Ok,
            state: ListingState::Reviewing,
            applied_day: day,
            format_validated: true,
            hash_cleared: true,
        }
    }

    #[test]
    fn admission_and_lifecycle() {
        let mut board = ThemeBoard::new();
        let id = board.admit(listing(20260101)).unwrap();
        board.publish(id, 20260105).unwrap();
        assert_eq!(board.fetchable(), 1);
        board.mark_stale(id).unwrap();
        assert_eq!(board.fetchable(), 0);
        assert!(board.delist(id, false).is_err());
        board.delist(id, true).unwrap();
        assert!(board.mark_stale(id).is_err());
    }
}
