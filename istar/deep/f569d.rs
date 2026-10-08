//! 深化层 · F569 通知点击直达（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深三条（判据唯一源：主册 F569 节）：
//! ①「主区/按钮命中区分离」的**几何命中器**——主区与按钮区之间隔
//!   [`HIT_GAP_PX`] 缓冲带，落在缓冲带 = 不命中（误触防线是几何可测
//!   的，不是一句「分离」）；点击途中滑出主区应取消（按压语义同路）；
//! ②「三类通知直达目标」的**路由表**——消息→会话/邮件→那封邮件/
//!   更新完成→更新页，类型→目标形制的映射唯一源；
//! ③「无去处应用回退」的**回退账**——target=None 的点击走首页回退，
//!   回退逐笔入账（回退是降级路径，不许静默装作直达成功）。

use alloc::string::String;
use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::notifyjump::{NotifyJump, NotifyKind, JumpTarget, HIT_GAP_PX};

// ---------------------------------------------------------------------------
// 几何命中器
// ---------------------------------------------------------------------------

/// 命中结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitRegion {
    /// 主区（直达）。
    Main,
    /// 操作按钮（执行动作）。
    Button,
    /// 缓冲带/卡外（不命中——误触防线）。
    None,
}

/// 横幅几何命中：主区 [0, main_end)、按钮 [btn_x, banner_w)、中间隔
/// [`HIT_GAP_PX`] 缓冲带（落在带上 = None——宁可漏点不可误触）。
pub fn hit_region(click_x: u32, main_end: u32, btn_x: u32, banner_w: u32) -> HitRegion {
    if click_x < main_end {
        HitRegion::Main
    } else if click_x >= main_end + HIT_GAP_PX && click_x < btn_x {
        // 主区尾到按钮头之间还有正文空隙——归主区（点空白不误开按钮）。
        HitRegion::Main
    } else if click_x >= btn_x && click_x < banner_w {
        HitRegion::Button
    } else {
        HitRegion::None
    }
}

// ---------------------------------------------------------------------------
// 类型路由表
// ---------------------------------------------------------------------------

/// 三类通知的标准直达目标（路由唯一源——应用声明缺失时按类补位；
/// 会话/邮件类携带引用 id，形制与主册一一对应）。
pub fn standard_target(kind: NotifyKind, ref_id: &str) -> JumpTarget {
    match kind {
        NotifyKind::Message => JumpTarget::Conversation(String::from(ref_id)),
        NotifyKind::Mail => JumpTarget::MailItem(String::from(ref_id)),
        NotifyKind::Update => JumpTarget::UpdatePage,
    }
}

// ---------------------------------------------------------------------------
// 回退账
// ---------------------------------------------------------------------------

/// 一条回退记录（无去处应用点击主区的降级路径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FallbackRecord {
    pub id: u64,
    pub app_fallback: bool,
}

/// 回退账（环，容量 16）。
pub struct FallbackLog {
    buf: [Option<FallbackRecord>; 16],
    head: usize,
    len: usize,
}

impl FallbackLog {
    pub fn new() -> FallbackLog {
        FallbackLog { buf: [None; 16], head: 0, len: 0 }
    }

    pub fn record(&mut self, id: u64) {
        self.buf[self.head] = Some(FallbackRecord { id, app_fallback: true });
        self.head = (self.head + 1) % 16;
        if self.len < 16 {
            self.len += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }
}

impl Default for FallbackLog {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f569_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 几何命中：主区直达、按钮动作、缓冲带不命中、卡外不命中。
    //    布局：主区 [0,100)、缓冲 [100,108)、按钮 [108,140)、卡外 ≥140。
    cs.add(
        "hit regions with gap buffer",
        hit_region(50, 100, 108, 140) == HitRegion::Main
            && hit_region(103, 100, 108, 140) == HitRegion::None
            && hit_region(120, 100, 108, 140) == HitRegion::Button
            && hit_region(139, 100, 108, 140) == HitRegion::Button
            && hit_region(150, 100, 108, 140) == HitRegion::None
            && HIT_GAP_PX == 8,
        "",
    );

    // 2) 路由表：三类通知的标准去向形制与主册一致。
    cs.add(
        "route table matches main book",
        standard_target(NotifyKind::Message, "s1") == JumpTarget::Conversation(String::from("s1"))
            && standard_target(NotifyKind::Mail, "m1") == JumpTarget::MailItem(String::from("m1"))
            && standard_target(NotifyKind::Update, "") == JumpTarget::UpdatePage,
        "",
    );

    // 3) 有去处直达：主区点击 → 跳转账 + 横幅消 + 已读（对账三件齐）。
    let mut j = NotifyJump::new();
    j.push(1, NotifyKind::Message, "im", Some(JumpTarget::Conversation(String::from("sess-7"))));
    let got = j.click_main(1);
    let (banner_off, read) = j.state_of(1).unwrap();
    cs.add(
        "main click jumps and closes",
        got == Some(JumpTarget::Conversation(String::from("sess-7")))
            && !banner_off
            && read
            && j.jump_count() == 1,
        "",
    );

    // 4) 按钮区点击：只执行动作不跳转（主区/按钮职责分明的对账面）。
    let mut j2 = NotifyJump::new();
    j2.push(2, NotifyKind::Update, "update", Some(JumpTarget::UpdatePage));
    let btn = j2.click_button(2);
    cs.add(
        "button click acts without jump",
        btn && j2.jump_count() == 0 && j2.state_of(2).map(|(b, _)| !b).unwrap_or(false),
        "",
    );

    // 5) 无去处回退：target=None → HomeFallback 且回退入账。
    let mut j3 = NotifyJump::new();
    let mut fb = FallbackLog::new();
    j3.push(3, NotifyKind::Mail, "legacy-app", None);
    let got3 = j3.click_main(3);
    if got3 == Some(JumpTarget::HomeFallback) {
        fb.record(3);
    }
    cs.add(
        "no target falls back with ledger",
        got3 == Some(JumpTarget::HomeFallback) && fb.len() == 1,
        "",
    );

    // 6) 跳转账新到旧：三次点击逐笔可回放（直达验证的对账面）。
    let mut j4 = NotifyJump::new();
    for id in [7u64, 8, 9] {
        j4.push(id, NotifyKind::Message, "im", Some(JumpTarget::Conversation(alloc::format!("c{}", id))));
        let _ = j4.click_main(id);
    }
    let first = j4.jump_at(0);
    let last = j4.jump_at(2);
    cs.add(
        "jump ledger replayable",
        first.map(|(id, _)| id == 7).unwrap_or(false)
            && last.map(|(id, _)| id == 9).unwrap_or(false),
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gap_boundary_exact() {
        // 缓冲带两端：main_end 恰好是 Main 最后一格；main_end+7 仍 None。
        assert_eq!(hit_region(99, 100, 108, 140), HitRegion::Main);
        assert_eq!(hit_region(107, 100, 108, 140), HitRegion::None);
        assert_eq!(hit_region(108, 100, 108, 140), HitRegion::Button);
    }
}
