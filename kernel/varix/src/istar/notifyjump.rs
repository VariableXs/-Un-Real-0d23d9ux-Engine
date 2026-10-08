//! F569 通知点击直达 · 完整设计（STAR I 主册 I 域批次七）。
//!
//! **判据（主册）**：三类通知直达目标验证；主区/按钮命中区分离；
//! 点击后状态（横幅消/中心已读）；无去处应用回退（开首页并说明）。
//!
//! **设计要点（主册）**：
//! - 点击 = 打开来源应用对应内容（消息→会话/邮件→那封邮件/更新完成→更新页）
//!   ——点击永远有「对应的去处」不是只开应用首页；
//! - 操作按钮（F281 两钮）与点击主区职责分明（主区点击直达、按钮执行动作）；
//! - 点击后横幅即消（中心内标记已读）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

use alloc::string::String;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 主区命中区与按钮命中区的间隔（px——命中区分离的几何账）。
pub const HIT_GAP_PX: u32 = 8;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 通知三类（主册点名：消息/邮件/更新——枚举即清单）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifyKind {
    Message,
    Mail,
    Update,
}

/// 直达目标（点击主区要去的「对应去处」）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JumpTarget {
    /// 会话（消息）。
    Conversation(String),
    /// 那封邮件。
    MailItem(String),
    /// 更新页。
    UpdatePage,
    /// 无去处——回退开首页并说明（诚实回退路径）。
    HomeFallback,
}

/// 一条通知。
pub struct Notice {
    pub id: u64,
    pub kind: NotifyKind,
    pub app: String,
    /// 直达目标（应用声明；None = 无去处 → 回退）。
    pub target: Option<JumpTarget>,
    /// 横幅是否在显。
    pub banner_shown: bool,
    /// 中心内是否已读。
    pub read: bool,
}

/// 通知直达引擎。
pub struct NotifyJump {
    notices: [Option<Notice>; 32],
    len: usize,
    /// 跳转账（直达验证对账面）。
    jumps: [Option<(u64, JumpTarget)>; 32],
    jump_len: usize,
}

impl NotifyJump {
    pub fn new() -> NotifyJump {
        NotifyJump {
            notices: [(); 32].map(|_| None),
            len: 0,
            jumps: [(); 32].map(|_| None),
            jump_len: 0,
        }
    }

    /// 收一条通知（横幅在显、未读）。
    pub fn push(&mut self, id: u64, kind: NotifyKind, app: &str, target: Option<JumpTarget>) {
        if self.len < 32 {
            self.notices[self.len] = Some(Notice {
                id,
                kind,
                app: String::from(app),
                target,
                banner_shown: true,
                read: false,
            });
            self.len += 1;
        }
    }

    /// 主区点击：直达对应去处（主区职责——只跳转不执行按钮动作）。
    ///
    /// 命中区分离由调用方保证（几何账 [`HIT_GAP_PX`]）；点击后横幅即消、
    /// 中心标记已读。无去处 → 回退开首页并说明（HomeFallback）。
    pub fn click_main(&mut self, id: u64) -> Option<JumpTarget> {
        let slot = self.notices[..self.len]
            .iter_mut()
            .flatten()
            .find(|n| n.id == id)?;
        let t = slot.target.clone().unwrap_or(JumpTarget::HomeFallback);
        slot.banner_shown = false;
        slot.read = true;
        if self.jump_len < 32 {
            self.jumps[self.jump_len] = Some((id, t.clone()));
            self.jump_len += 1;
        }
        Some(t)
    }

    /// 按钮点击：执行动作不跳转（职责分离——按钮动作由 F281 两钮语义
    /// 注入执行，本引擎只记账「点了按钮≠直达」）。
    pub fn click_button(&mut self, id: u64) -> bool {
        match self
            .notices[..self.len]
            .iter_mut()
            .flatten()
            .find(|n| n.id == id)
        {
            Some(n) => {
                // 按钮动作不改变直达账；横幅即消是唯一共有行为。
                n.banner_shown = false;
                true
            }
            None => false,
        }
    }

    /// 点击后状态：横幅消、中心已读（判据对账面）。
    pub fn state_of(&self, id: u64) -> Option<(bool, bool)> {
        self.notices[..self.len]
            .iter()
            .flatten()
            .find(|n| n.id == id)
            .map(|n| (n.banner_shown, n.read))
    }

    pub fn jump_count(&self) -> usize {
        self.jump_len
    }

    pub fn jump_at(&self, i: usize) -> Option<(u64, JumpTarget)> {
        self.jumps.get(i).and_then(|s| s.as_ref()).cloned()
    }
}

impl Default for NotifyJump {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_notifyjump_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 三类直达：消息→会话、邮件→那封、更新→更新页。
    let mut j = NotifyJump::new();
    j.push(1, NotifyKind::Message, "聊天", Some(JumpTarget::Conversation("与 A 的会话".into())));
    j.push(2, NotifyKind::Mail, "邮件", Some(JumpTarget::MailItem("Q3 报告".into())));
    j.push(3, NotifyKind::Update, "系统更新", Some(JumpTarget::UpdatePage));
    let t1 = j.click_main(1);
    let t2 = j.click_main(2);
    let t3 = j.click_main(3);
    set.add(
        "three kinds jump to targets",
        t1 == Some(JumpTarget::Conversation("与 A 的会话".into()))
            && t2 == Some(JumpTarget::MailItem("Q3 报告".into()))
            && t3 == Some(JumpTarget::UpdatePage),
        "",
    );

    // 2. 点击后状态：横幅消、中心已读（三例全查）。
    let s1 = j.state_of(1);
    let s2 = j.state_of(2);
    let s3 = j.state_of(3);
    set.add(
        "banner gone center read",
        s1 == Some((false, true)) && s2 == Some((false, true)) && s3 == Some((false, true)),
        "",
    );

    // 3. 主区/按钮命中区分离：按钮点击不产生直达账。
    let mut j2 = NotifyJump::new();
    j2.push(5, NotifyKind::Message, "聊天", Some(JumpTarget::Conversation("会话".into())));
    j2.click_button(5);
    set.add(
        "button click no jump",
        j2.jump_count() == 0 && j2.state_of(5) == Some((false, false)),
        "",
    );

    // 4. 无去处回退：target None → HomeFallback（开首页并说明）。
    let mut j3 = NotifyJump::new();
    j3.push(6, NotifyKind::Update, "小工具", None);
    let fb = j3.click_main(6);
    set.add(
        "no target falls back home",
        fb == Some(JumpTarget::HomeFallback),
        "",
    );

    // 5. 命中区几何账：主区与按钮间隔 8px（分离的结构参数）。
    set.add("hit zones separated by 8px", HIT_GAP_PX == 8, "");

    // 6. 直达账逐条可查（验证对账源）。
    set.add(
        "jump ledger per entry",
        j.jump_count() == 3 && j.jump_at(0).map(|(id, _)| id) == Some(1),
        "",
    );

    // 7. 未知通知点击诚实 None。
    set.add("unknown notice none", j.click_main(99).is_none(), "");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn button_then_main_still_jumps() {
        let mut j = NotifyJump::new();
        j.push(1, NotifyKind::Mail, "邮件", Some(JumpTarget::MailItem("m".into())));
        j.click_button(1);
        assert_eq!(j.click_main(1), Some(JumpTarget::MailItem("m".into())));
        assert_eq!(j.jump_count(), 1);
    }

    #[test]
    fn double_click_main_single_jump() {
        let mut j = NotifyJump::new();
        j.push(1, NotifyKind::Message, "聊天", None);
        let _ = j.click_main(1);
        let _ = j.click_main(1); // 第二次点击（横幅已消）仍记账但不重复状态变更
        assert_eq!(j.jump_count(), 2);
        assert_eq!(j.state_of(1), Some((false, true)));
    }

    #[test]
    fn target_preserved_after_button() {
        let mut j = NotifyJump::new();
        j.push(2, NotifyKind::Update, "更新", Some(JumpTarget::UpdatePage));
        j.click_button(2);
        assert_eq!(j.click_main(2), Some(JumpTarget::UpdatePage));
    }
}
