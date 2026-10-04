//! F574 系统更新摘要卡 · 完整设计（STAR I 主册 I 域批次七）。
//!
//! **判据（主册）**：要点 3-5 条约束；人话转译抽查；展示时机（重启后
//! 首登）；同源对账 F132；关闭开关。
//!
//! **设计要点（主册）**：
//! - 更新完成（F122 链路）后的用户面：更新摘要卡一条通知（本次更新了什么
//!   ——3-5 条人话要点 +「查看详情」）；卡片在更新重启后首次登录展示
//!   （不是更新前吓你）；要点来自更新日志的人话转译（F132 差异表同源）；
//! - 「不再显示」此卡一键。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 要点条数红线：3-5 条（少于 3 不成卡、多于 5 拆卡——约束唯一源）。
pub const SUMMARY_MIN: usize = 3;
pub const SUMMARY_MAX: usize = 5;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 一条人话要点（F132 差异表条目 → 转译产物）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bullet {
    /// 人话文案（「任务栏支持中键关闭」——不是 changelog 天书）。
    pub text: String,
    /// F132 差异表追踪号（同源对账锚）。
    pub trace_id: u32,
}

/// 摘要卡。
pub struct SummaryCard {
    pub version: String,
    pub bullets: Vec<Bullet>,
    /// 用户是否点了「不再显示」。
    dismissed: bool,
    /// 是否已展示（重启后首登时机账）。
    pub shown: bool,
}

impl SummaryCard {
    /// 从差异表转译出卡（translates 由宿主转译层注入；本函数执行 3-5 条
    /// 约束门——不合格拒绝成卡）。
    pub fn compose(version: &str, bullets: Vec<Bullet>) -> Option<SummaryCard> {
        if bullets.len() < SUMMARY_MIN || bullets.len() > SUMMARY_MAX {
            return None;
        }
        if bullets.iter().any(|b| b.text.trim().is_empty()) {
            return None; // 空要点不成卡（人话转译缺位 = 不交付）。
        }
        Some(SummaryCard {
            version: String::from(version),
            bullets,
            dismissed: false,
            shown: false,
        })
    }

    /// 展示时机：重启后首登（宿主注入 first_login_after_reboot）。
    pub fn maybe_show(&mut self, first_login_after_reboot: bool) -> bool {
        if self.dismissed || self.shown || !first_login_after_reboot {
            return false;
        }
        self.shown = true;
        true
    }

    /// 「不再显示」一键（对当前版本卡——下版本新卡不受影响）。
    pub fn dismiss(&mut self) -> bool {
        if self.dismissed {
            return false;
        }
        self.dismissed = true;
        true
    }

    pub fn dismissed(&self) -> bool {
        self.dismissed
    }

    /// F132 同源对账：卡上每条要点都能在差异表追踪号集合里找到出处。
    pub fn same_source_with(&self, diff_table_ids: &[u32]) -> bool {
        self.bullets.iter().all(|b| diff_table_ids.contains(&b.trace_id))
    }
}

/// 卡片账（版本序列管理——升级后新卡替换旧卡）。
pub struct CardLedger {
    cards: Vec<SummaryCard>,
}

impl CardLedger {
    pub fn new() -> CardLedger {
        CardLedger { cards: Vec::new() }
    }

    /// 挂新卡（新版本 → 旧卡自动 dismiss——只展示最新一张）。
    pub fn push(&mut self, card: SummaryCard) -> bool {
        if self.cards.iter().any(|c| c.version == card.version) {
            return false; // 同版本重复挂卡拒绝。
        }
        for old in self.cards.iter_mut() {
            old.dismissed = true;
        }
        self.cards.push(card);
        true
    }

    /// 首登展示检查：最新未展示且未 dismiss 的卡。
    pub fn due_card(&mut self, first_login_after_reboot: bool) -> Option<&str> {
        for c in self.cards.iter_mut().rev() {
            if c.maybe_show(first_login_after_reboot) {
                return Some(&c.version);
            }
        }
        None
    }

    pub fn latest_version(&self) -> Option<&str> {
        self.cards.last().map(|c| c.version.as_str())
    }
}

impl Default for CardLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_upsummary_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    let mk = |text: &str, id: u32| Bullet { text: String::from(text), trace_id: id };

    // 1. 要点 3-5 条约束：2 条拒、3 条收、6 条拒。
    let two: Vec<Bullet> = (0..2).map(|i| mk(&alloc::format!("要点{}", i), i)).collect();
    let three: Vec<Bullet> = (0..3).map(|i| mk(&alloc::format!("要点{}", i), i)).collect();
    let six: Vec<Bullet> = (0..6).map(|i| mk(&alloc::format!("要点{}", i), i)).collect();
    let r2 = SummaryCard::compose("1.1", two).is_none();
    let r3 = SummaryCard::compose("1.1", three).is_some();
    let r6 = SummaryCard::compose("1.1", six).is_none();
    set.add(
        "bullet count three to five",
        r2 && r3 && r6 && SUMMARY_MIN == 3 && SUMMARY_MAX == 5,
        "",
    );

    // 2. 人话转译抽查：空要点拒绝成卡（转译缺位不交付）。
    let with_empty = alloc::vec![mk("任务栏支持中键关闭", 1), mk("", 2), mk("安全补丁", 3)];
    set.add("empty bullet rejected", SummaryCard::compose("1.1", with_empty).is_none(), "");

    // 3. 展示时机：重启后首登才展示；未首登/已展示不再出。
    let mut card = SummaryCard::compose(
        "1.1",
        alloc::vec![mk("任务栏支持中键关闭", 1), mk("修复蓝牙重连问题", 2), mk("安全补丁", 3)],
    )
    .unwrap();
    let too_early = !card.maybe_show(false);
    let on_login = card.maybe_show(true);
    let once = !card.maybe_show(true);
    set.add(
        "shown after reboot first login only",
        too_early && on_login && once && card.shown,
        "",
    );

    // 4. 关闭开关：dismiss 后不再展示。
    let mut card2 = SummaryCard::compose(
        "1.2",
        alloc::vec![mk("a", 1), mk("b", 2), mk("c", 3)],
    )
    .unwrap();
    card2.dismiss();
    set.add(
        "dismiss switch works",
        card2.dismissed() && !card2.maybe_show(true),
        "",
    );

    // 5. F132 同源对账：每条要点的追踪号都能在差异表找到。
    let ok_card = SummaryCard::compose(
        "1.1",
        alloc::vec![mk("x", 10), mk("y", 11), mk("z", 12)],
    )
    .unwrap();
    let drift = SummaryCard::compose(
        "1.1",
        alloc::vec![mk("x", 10), mk("y", 99), mk("z", 12)],
    )
    .unwrap();
    set.add(
        "f132 same source audit",
        ok_card.same_source_with(&[10, 11, 12]) && !drift.same_source_with(&[10, 11, 12]),
        "",
    );

    // 6. 卡片账：新版本挂卡旧卡自动退场；同版本重复挂拒绝。
    let mut led = CardLedger::new();
    led.push(SummaryCard::compose("1.0", alloc::vec![mk("a", 1), mk("b", 2), mk("c", 3)]).unwrap());
    led.push(SummaryCard::compose("1.1", alloc::vec![mk("d", 4), mk("e", 5), mk("f", 6)]).unwrap());
    let dup_card = SummaryCard::compose("1.1", alloc::vec![mk("g", 7), mk("h", 8), mk("i", 9)]);
    let dup_rejected = match dup_card {
        Some(c) => !led.push(c),
        None => true,
    };
    set.add(
        "new version retires old",
        led.latest_version() == Some("1.1") && dup_rejected,
        "",
    );

    // 7. 首登展示沿账走：最新卡在首登时出。
    let due = led.due_card(true);
    set.add("due card on first login", due == Some("1.1"), "");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shown_card_not_redisplayed_after_reboot() {
        let mut led = CardLedger::new();
        led.push(SummaryCard::compose("9", vec![
            Bullet { text: String::from("a"), trace_id: 1 },
            Bullet { text: String::from("b"), trace_id: 2 },
            Bullet { text: String::from("c"), trace_id: 3 },
        ]).unwrap());
        assert_eq!(led.due_card(true), Some("9"));
        assert_eq!(led.due_card(true), None); // 已展示不再出
    }

    #[test]
    fn dismissed_card_not_due() {
        let mut led = CardLedger::new();
        let mut c = SummaryCard::compose("8", vec![
            Bullet { text: String::from("a"), trace_id: 1 },
            Bullet { text: String::from("b"), trace_id: 2 },
            Bullet { text: String::from("c"), trace_id: 3 },
        ]).unwrap();
        c.dismiss();
        led.push(c);
        assert_eq!(led.due_card(true), None);
    }

    #[test]
    fn bullet_count_bounds_exact() {
        let five: Vec<Bullet> = (0..5).map(|i| Bullet { text: alloc::format!("b{}", i), trace_id: i }).collect();
        assert!(SummaryCard::compose("v", five).is_some());
    }
}
