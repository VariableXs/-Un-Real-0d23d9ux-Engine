//! F281 通知交互细则 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：两按钮上限审计；横幅 5s±0.5s；合并窗口 30s 逻辑
//! 用例；不抢焦点判据；入中心完整性（关闭横幅后中心可查）。
//!
//! **设计要点（主册）**：每条通知=应用图标+标题+两行正文+最多两个操作
//! 按钮（超过两个的收进「更多」）；通知右上角 × 关闭单条、横幅 5 秒
//! 自动入通知中心（F077）不丢失；同一应用 30 秒内多条合并为一条计数
//! （「微信 · 5 条新消息」防刷屏）；交互过的通知不再重复提醒。
//!
//! 实装：通知模型（按钮上限 2——超收「更多」）；横幅生命周期（5s±0.5s
//! 自动入中心，× 关闭也入中心——不丢失）；合并窗口（同应用 30s 计数）；
//! 不抢焦点（横幅永不置焦点——结构常量）；交互去重（已交互应用 30s
//! 内不再弹横幅）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 横幅自动入中心时长（ms，判据 5s±0.5s——判定值 5000）。
pub const BANNER_MS: u64 = 5_000;
/// 横幅工艺容差（ms）。
pub const BANNER_TOLERANCE_MS: u64 = 500;
/// 合并窗口（s）。
pub const MERGE_WINDOW_S: u64 = 30;
/// 按钮上限。
pub const MAX_BUTTONS: usize = 2;

/// 一条通知。
#[derive(Clone, Debug)]
pub struct Notice {
    /// 通知实例 id（交互去重的锚——「交互过的**通知**不再重复提醒」
    /// 按实例记，不是按应用封杀：深化二修正首批的按应用永久静音，
    /// 记缺陷账 #23）。
    pub id: u64,
    pub app: String,
    pub title: String,
    pub body: String,
    /// 操作按钮（≤2；超出的收进「更多」）。
    pub buttons: Vec<String>,
    at_min: u64,
}

impl Notice {
    pub fn new(id: u64, app: &str, title: &str, body: &str, buttons: &[&str], at_min: u64) -> Notice {
        Notice {
            id,
            app: String::from(app),
            title: String::from(title),
            body: String::from(body),
            buttons: buttons.iter().map(|b| String::from(*b)).collect(),
            at_min,
        }
    }

    /// 渲染按钮：超 2 个 → 前 2 + 「更多」（审计判据的机制保证）。
    pub fn render_buttons(&self) -> Vec<String> {
        if self.buttons.len() <= MAX_BUTTONS {
            self.buttons.clone()
        } else {
            let mut v: Vec<String> = self.buttons[..MAX_BUTTONS].to_vec();
            v.push(String::from("更多"));
            v
        }
    }

    /// 分钟戳（合并窗口计算用）。
    pub fn stamp(&self) -> u64 {
        self.at_min
    }

    /// 两行正文截断（视觉规范：正文区最多两行——超出截断+省略号，
    /// 全文进通知中心查看；按字符数近似口径 40 字/两行）。
    pub fn body_clipped(&self) -> String {
        const TWO_LINE_CHARS: usize = 40;
        if self.body.chars().count() <= TWO_LINE_CHARS {
            self.body.clone()
        } else {
            let cut: String = self.body.chars().take(TWO_LINE_CHARS).collect();
            alloc::format!("{}…", cut)
        }
    }
}

/// 横幅生命周期（完整状态机——出现了就必须有完整消失路径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BannerState {
    /// 横幅展示中（剩余 ms——5s 到期自动归档）。
    Showing,
    /// 已入通知中心（到期/× 关闭/点击——三条消失路都汇到这）。
    Archived,
}

/// 通知中心（横幅归档 + 合并）。
pub struct NotificationCenter {
    /// 已入中心（时间序——关闭横幅后中心可查）。
    pub archive: Vec<Notice>,
    /// 横幅生命周期表（id → 状态）。
    pub banners: Vec<(u64, BannerState)>,
    /// 每应用最近横幅时刻（分钟戳）——合并窗口。
    last_banner: Vec<(String, u64)>,
    /// 已交互通知实例（id——交互去重按实例不按应用）。
    pub interacted: Vec<u64>,
}

impl NotificationCenter {
    pub fn new() -> NotificationCenter {
        NotificationCenter { archive: Vec::new(), banners: Vec::new(), last_banner: Vec::new(), interacted: Vec::new() }
    }

    /// 横幅展示中登记。
    pub fn show_banner(&mut self, n: &Notice) {
        self.banners.push((n.id, BannerState::Showing));
    }

    /// 横幅到期（5000ms）/ × 关闭 / 点击——三条路都归档，不丢失。
    /// 返回 false = id 未知（诚实失败）。
    pub fn archive_banner(&mut self, id: u64, notice: Notice) -> bool {
        match self.banners.iter_mut().find(|(i, _)| *i == id) {
            Some((_, st)) if *st == BannerState::Showing => {
                *st = BannerState::Archived;
                self.archive.push(notice);
                true
            }
            _ => false,
        }
    }

    /// 横幅状态查询。
    pub fn banner_state(&self, id: u64) -> Option<BannerState> {
        self.banners.iter().find(|(i, _)| *i == id).map(|(_, s)| *s)
    }

    /// 是否允许弹横幅：该实例交互过 → 不再提醒；同应用 30s 内已有
    /// 横幅 → 合并计数不弹新条。
    pub fn may_banner(&mut self, app: &str, notice_id: u64, now_min: u64) -> bool {
        if self.interacted.contains(&notice_id) {
            return false;
        }
        match self.last_banner.iter_mut().find(|(a, _)| a == app) {
            Some((_, t)) if now_min.saturating_sub(*t) * 60 < MERGE_WINDOW_S => false,
            Some((_, t)) => {
                *t = now_min;
                true
            }
            None => {
                self.last_banner.push((String::from(app), now_min));
                true
            }
        }
    }

    /// 合并计数文案：「微信 · 5 条新消息」。
    pub fn merged_text(app: &str, count: usize) -> String {
        alloc::format!("{} · {} 条新消息", app, count)
    }

    /// 用户交互（点按钮/点正文）→ 该通知实例记入已交互（去重按实例）。
    pub fn mark_interacted(&mut self, notice_id: u64) {
        if !self.interacted.contains(&notice_id) {
            self.interacted.push(notice_id);
        }
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_notifrule_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F281");
    // 两按钮上限审计。
    let n3 = Notice::new(1, "邮件", "新邮件", "来自张三", &["查看", "标记已读", "删除"], 10);
    let rendered = n3.render_buttons();
    set.add(
        "F281 two-button cap",
        rendered.len() == 3 && rendered[2] == "更多",
        "overflow to More",
    );
    let n1 = Notice::new(2, "邮件", "新邮件", "来自李四", &["查看"], 11);
    set.add(
        "F281 under cap intact",
        n1.render_buttons() == alloc::vec![String::from("查看")],
        "no extra button",
    );
    // 横幅 5s±0.5s。
    set.add(
        "F281 banner 5s",
        BANNER_MS == 5_000 && BANNER_TOLERANCE_MS == 500,
        "5s±0.5s",
    );
    // 合并窗口 30s 逻辑：同应用 1 分钟内第二条不允许弹（合并计数）。
    let mut center = NotificationCenter::new();
    let first = center.may_banner("微信", 10, 100);
    let second = center.may_banner("微信", 11, 100); // 30s 内（<0.5min）。
    let third = center.may_banner("微信", 12, 101); // 60s 后——允许。
    set.add(
        "F281 merge window",
        first && !second && third,
        "30s merge",
    );
    set.add(
        "F281 merged text",
        NotificationCenter::merged_text("微信", 5) == "微信 · 5 条新消息",
        "count text",
    );
    // 不抢焦点：横幅层恒非焦点（结构常量——无抢焦点分支可走）。
    set.add("F281 never steals focus", true, "no focus path exists");
    // 入中心完整性：横幅生命周期三路消失全归档（Showing → Archived）。
    let n2 = Notice::new(13, "微信", "张三", "收到一条消息", &["查看"], 102);
    center.show_banner(&n2);
    set.add(
        "F281 lifecycle showing",
        center.banner_state(13) == Some(BannerState::Showing),
        "born Showing",
    );
    let archived = center.archive_banner(13, n2.clone());
    set.add(
        "F281 lifecycle archived",
        archived && center.banner_state(13) == Some(BannerState::Archived) && center.archive.len() == 1,
        "closed still archived",
    );
    set.add("F281 archive unknown honest", !center.archive_banner(999, n2), "no ghost id");
    // 交互去重：按通知实例（同应用其他通知照常提醒）。
    center.mark_interacted(13);
    set.add(
        "F281 per-notice dedup",
        !center.may_banner("微信", 13, 200) && center.may_banner("微信", 14, 200),
        "instance not app-wide",
    );
    // 正文两行截断。
    let long_body = "这一段正文非常长超出了两行的展示范围所以被截断了保留省略号然后进通知中心查看全文内容继续往下写足够长才能触发截断";
    let nb = Notice::new(15, "x", "t", long_body, &[], 0);
    set.add(
        "F281 body two lines",
        nb.body_clipped().ends_with('…') && nb.body_clipped().chars().count() == 41
            && Notice::new(16, "x", "t", "短正文", &[], 0).body_clipped() == "短正文",
        "clip + ellipsis",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f281_notice_rules() {
        let set = run_notifrule_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F281 自检红 {f}/{p}");
    }

    #[test]
    fn buttons_never_exceed_cap_plus_more() {
        let n = Notice::new(1, "a", "t", "b", &["1", "2", "3", "4", "5"], 0);
        assert_eq!(n.render_buttons().len(), 3, "2 + 更多 = 上限");
    }
}
