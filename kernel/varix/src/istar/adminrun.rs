//! F552 以管理员身份运行 · 完整设计（STAR I 主册 I 域批次七）。
//!
//! **判据（主册）**：三入口链路；确认窗联动；防护色边条；降权路径；理由字段。
//!
//! **设计要点（主册）**：
//! - 右键「以管理员身份运行」三入口：应用 / 终端 / 脚本；
//! - 触发 F551 特权确认窗（应用名 + 图标 + 请求特权说明）——本模块经
//!   注入的 [`crate::istar::privconfirm::PrivGate`] 发起请求，不自建门；
//! - 特权会话标记：特权窗口标题栏加防护色边条（一眼知道这个窗权限大）；
//! - 降权按钮：特权窗内「回到普通模式」随时可退；
//! - 滥用防护：理由字段，缺省显示「未说明用途」。
//!
//! 钟注入式（ms 实参），无外部依赖。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::privconfirm::{Channel, OpKind, PrivGate, Verdict};

use alloc::string::String;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 防护色令牌名（F151 令牌表锚点——边条颜色唯一源，不持 RGB）。
pub const GUARD_TOKEN: &str = "guard-privilege";

/// 缺省理由文案（滥用防护：未声明用途时的显式提示）。
pub const REASON_UNSTATED: &str = "未说明用途";

/// 边条高度（px，标题栏内一眼可辨的最小可见线）。
pub const GUARD_BAR_H_PX: u32 = 3;

// ---------------------------------------------------------------------------
// 入口与会话
// ---------------------------------------------------------------------------

/// 三入口链路（主册点名：应用/终端/脚本——枚举即清单）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    App,
    Terminal,
    Script,
}

impl EntryKind {
    /// 确认卡上的入口说明文案。
    pub fn label(self) -> &'static str {
        match self {
            EntryKind::App => "以管理员身份运行应用",
            EntryKind::Terminal => "以管理员身份打开终端",
            EntryKind::Script => "以管理员身份运行脚本",
        }
    }
}

/// 特权会话（一次授权 = 一个会话；降权即会话终结）。
pub struct PrivSession {
    pub target: String,
    pub kind: EntryKind,
    /// 理由字段（缺省「未说明用途」——不给空串过审）。
    pub reason: String,
    /// 会话开始时刻（ms，审计账取数）。
    pub since_ms: u64,
}

/// 管理员运行面板（三入口 × F551 确认窗联动 × 会话标记与降权）。
pub struct AdminRun {
    gate: PrivGate,
    /// 在途提权请求（elevate 存入；confirm 消费——单一请求生命周期）。
    pending_req: Option<(String, EntryKind, String)>,
    /// 当前特权会话（None = 无特权窗口在跑）。
    session: Option<PrivSession>,
    now_ms: u64,
}

impl AdminRun {
    pub fn new(gate: PrivGate) -> AdminRun {
        AdminRun {
            gate,
            pending_req: None,
            session: None,
            now_ms: 0,
        }
    }

    pub fn tick(&mut self, ms: u64) {
        if ms > self.now_ms {
            self.now_ms = ms;
        }
    }

    /// 发起提权：三入口统一走此口，内部触发 F551 确认窗。
    ///
    /// 返回确认窗裁决；`Pending` 时窗上应有目标名 + 入口说明 + 理由字段
    /// （请求在案，确认时落会话）。
    pub fn elevate(&mut self, target: &str, kind: EntryKind, reason: Option<&str>) -> Verdict {
        self.tick(self.now_ms);
        let v = self
            .gate
            .request(Channel::Kernel, target, OpKind::KernelDriverInstall);
        if v == Verdict::Pending {
            self.pending_req = Some((
                String::from(target),
                kind,
                String::from(reason.unwrap_or(REASON_UNSTATED)),
            ));
        }
        v
    }

    /// 「允许一次」落会话：消费在途请求，建特权会话。
    pub fn confirm(&mut self) -> Option<PrivSession> {
        let (target, kind, reason) = self.pending_req.take()?;
        self.gate.allow_once()?;
        self.session = Some(PrivSession {
            target: target.clone(),
            kind,
            reason: reason.clone(),
            since_ms: self.now_ms,
        });
        Some(PrivSession {
            target,
            kind,
            reason,
            since_ms: self.now_ms,
        })
    }

    /// 特权窗口边条参数（防护色 + 高度）——渲染层唯一取数口。
    pub fn guard_bar(&self) -> Option<(&'static str, u32)> {
        self.session.as_ref().map(|_| (GUARD_TOKEN, GUARD_BAR_H_PX))
    }

    /// 降权路径：「回到普通模式」随时可退，返回被终结的会话。
    pub fn demote(&mut self) -> Option<PrivSession> {
        self.session.take()
    }

    /// 当前是否处于特权会话（边条显示的唯一判据）。
    pub fn privileged(&self) -> bool {
        self.session.is_some()
    }

    /// 当前会话的理由字段（审计与确认卡复显用）。
    pub fn current_reason(&self) -> Option<&str> {
        self.session.as_ref().map(|s| s.reason.as_str())
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_adminrun_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 三入口链路文案齐备（应用/终端/脚本枚举即清单）。
    set.add(
        "three entries labeled",
        !EntryKind::App.label().is_empty()
            && !EntryKind::Terminal.label().is_empty()
            && !EntryKind::Script.label().is_empty(),
        "",
    );

    // 2. 确认窗联动：elevate 走 F551 门（Kernel 通道）→ Pending。
    let mut ar = AdminRun::new(PrivGate::new());
    ar.tick(1_000);
    let v = ar.elevate("devtool", EntryKind::App, Some("编译驱动需要写内核日志"));
    set.add("elevate hits f551 gate", v == Verdict::Pending, "");

    // 3. 会话建立：确认后特权会话在案、防护色边条参数齐。
    ar.confirm();
    let bar = ar.guard_bar();
    set.add(
        "guard bar on privileged",
        ar.privileged() && bar == Some((GUARD_TOKEN, GUARD_BAR_H_PX)),
        "",
    );

    // 4. 理由字段：声明即用；缺省「未说明用途」（不给空串过审）。
    let mut ar2 = AdminRun::new(PrivGate::new());
    ar2.tick(2_000);
    ar2.elevate("patcher", EntryKind::Script, None);
    ar2.confirm();
    let stated = ar2.current_reason() == Some(REASON_UNSTATED);
    set.add("reason default unstated", stated, "");

    // 5. 降权路径：随时可退，退后边条消失、会话清空。
    let demoted = ar.demote();
    set.add(
        "demote revokes session",
        demoted.is_some()
            && demoted.as_ref().map(|s| s.target.as_str()) == Some("devtool")
            && !ar.privileged()
            && ar.guard_bar().is_none(),
        "",
    );

    // 6. 用户态伪造入口：elevate 的门在 F551，User 通道请求被门外即拒
    //    （防护不因三入口绕过）。
    set.add(
        "gate rejects forged channel",
        {
            let mut g = PrivGate::new();
            g.request(Channel::User, "rogue", OpKind::KernelDriverInstall)
                == Verdict::RejectedChannel
        },
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elevate_confirm_demote_flow() {
        let mut ar = AdminRun::new(PrivGate::new());
        ar.tick(100);
        assert_eq!(ar.elevate("t", EntryKind::Terminal, None), Verdict::Pending);
        assert!(!ar.privileged());
        ar.confirm();
        assert!(ar.privileged());
        assert_eq!(ar.current_reason(), Some(REASON_UNSTATED));
        let s = ar.demote().unwrap();
        assert_eq!(s.target, "t");
        assert!(!ar.privileged());
    }

    #[test]
    fn confirm_without_pending_is_none() {
        let mut ar = AdminRun::new(PrivGate::new());
        assert!(ar.confirm().is_none());
    }

    #[test]
    fn bar_height_visible_line() {
        assert_eq!(GUARD_BAR_H_PX, 3);
    }
}
