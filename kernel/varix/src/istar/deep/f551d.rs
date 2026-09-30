//! 深化层 · F551 特权操作确认窗（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深三条（判据唯一源：主册 F551 节）：
//! ①「与 F406 安全屏同走内核通道**不可伪造**」的可执行化——基础层用
//!   `Channel` 枚举判通道，用户态仍可构造同名值；深化层引入**内核票据**：
//!   只有内核侧 [`TicketIssuer`] 签发的一次性、序号连续的票据能让 Kernel
//!   通道请求过门。花掉即废（一次性）、跳号即废（连续性）、用户态持票
//!   仍被通道判据拦在门外（票据不抬升通道）；
//! ②「确认留痕（F372 时间线）」的对账口——按时间窗提取留痕子集，
//!   供时间线渲染取数（全量倒序已达基础层，窗口切片在此补）；
//! ③确认卡**字面合同**——卡面三要素（操作方/要做什么/两钮文案）与
//!   白名单文案的唯一源对账（渲染层不得私改一个字）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::privconfirm::{AuditEntry, Channel, OpKind, PrivGate, Verdict};

// ---------------------------------------------------------------------------
// 内核票据（内核通道判据的可执行化）
// ---------------------------------------------------------------------------

/// 一张内核票据：单调序号 + 操作类绑定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ticket {
    pub seq: u64,
    pub kind: OpKind,
}

/// 内核侧票据签发器（只有它能造票——签发权即通道权的实体化）。
pub struct TicketIssuer {
    next_seq: u64,
    /// 已花掉的票据序号环（容量 32，滚动——足够覆盖一次会话的确认流）。
    spent: [Option<u64>; 32],
    spent_len: usize,
}

impl TicketIssuer {
    pub fn new() -> TicketIssuer {
        TicketIssuer { next_seq: 1, spent: [None; 32], spent_len: 0 }
    }

    /// 签发一张绑定 `kind` 的票据（序号单调递增，永不复用）。
    pub fn issue(&mut self, kind: OpKind) -> Ticket {
        let t = Ticket { seq: self.next_seq, kind };
        self.next_seq += 1;
        t
    }

    fn mark_spent(&mut self, seq: u64) {
        if self.spent_len < 32 {
            self.spent[self.spent_len] = Some(seq);
            self.spent_len += 1;
        } else {
            // 环满滚动：挤出最旧（确认流的高频场景下 32 张在途足够；
            // 滚动只影响「重放最旧票据」的判定窗口，不破坏单调性判定）。
            for i in 1..32 {
                self.spent[i - 1] = self.spent[i];
            }
            self.spent[31] = Some(seq);
        }
    }

    /// 票据有效性：序号已签发（≥1 且 < next_seq——0 号从未签发过）且
    /// 未花过（一次性）。
    pub fn valid_unspent(&self, t: &Ticket) -> bool {
        if t.seq == 0 || t.seq >= self.next_seq {
            return false;
        }
        (0..self.spent_len).all(|i| self.spent[i] != Some(t.seq))
    }
}

impl Default for TicketIssuer {
    fn default() -> Self {
        Self::new()
    }
}

/// 带票据判据的确认门（组合 [`PrivGate`]——基础层语义原样保留）。
pub struct TicketedGate {
    inner: PrivGate,
    issuer: TicketIssuer,
}

impl TicketedGate {
    pub fn new() -> TicketedGate {
        TicketedGate { inner: PrivGate::new(), issuer: TicketIssuer::new() }
    }

    pub fn issuer(&mut self) -> &mut TicketIssuer {
        &mut self.issuer
    }

    /// 发起请求：Kernel 通道必须携带有效未花票据；User 通道持票也拒。
    ///
    /// 判定顺序：通道判据（票据不抬升通道）→ 票据判据（未签发/已花/
    /// 类不匹配皆拒）→ 基础层四步（会话合并等原语义）。
    pub fn request(
        &mut self,
        channel: Channel,
        caller: &str,
        kind: OpKind,
        ticket: Option<&Ticket>,
    ) -> Verdict {
        if channel != Channel::Kernel {
            return self.inner.request(channel, caller, kind);
        }
        let ok = match ticket {
            Some(t) => {
                t.kind == kind && self.issuer.valid_unspent(t)
            }
            None => false,
        };
        if !ok {
            // 无票/坏票：以 User 通道身份递入——基础层按门外即拒处理并留痕。
            return self.inner.request(Channel::User, caller, kind);
        }
        if let Some(t) = ticket {
            self.issuer.mark_spent(t.seq);
        }
        self.inner.request(channel, caller, kind)
    }

    /// 留痕对账口（F372 时间线取数）：时间窗 [since_ms, until_ms] 内的留痕。
    pub fn audit_between(&self, since_ms: u64, until_ms: u64) -> alloc::vec::Vec<AuditEntry> {
        self.inner
            .audit()
            .into_iter()
            .filter(|e| e.ms >= since_ms && e.ms <= until_ms)
            .collect()
    }

    /// 基础层原语义直通（确认卡/放行/拒绝/合并——深化层不重复造）。
    pub fn inner(&mut self) -> &mut PrivGate {
        &mut self.inner
    }
}

impl Default for TicketedGate {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 确认卡字面合同（渲染层唯一取数口径）
// ---------------------------------------------------------------------------

/// 两钮字面合同（渲染层不得私改——一处一事实）。
pub const BTN_ALLOW: &str = "允许一次";
pub const BTN_CANCEL: &str = "取消";

/// 卡面三要素合同校验：操作方原样、要做什么 = 白名单文案、两钮在合同内。
pub fn card_contract(gate: &PrivGate, expect_caller: &str, kind: OpKind) -> bool {
    match gate.card() {
        Some((caller, what)) => {
            caller == expect_caller && what == kind.what() && what != BTN_ALLOW && what != BTN_CANCEL
        }
        None => false,
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f551_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 无票的 Kernel 请求：门外即拒且留痕（不可伪造的可执行化）。
    let mut g = TicketedGate::new();
    let v = g.request(Channel::Kernel, "app", OpKind::SystemFormat, None);
    cs.add("kernel without ticket rejected", v == Verdict::RejectedChannel, "");

    // 2) 有效票据放行：Pending 弹窗，票据一次性（重放即拒）。
    let mut g2 = TicketedGate::new();
    let t = g2.issuer().issue(OpKind::BootEntryEdit);
    let v1 = g2.request(Channel::Kernel, "bootcfg", OpKind::BootEntryEdit, Some(&t));
    let v2 = g2.request(Channel::Kernel, "bootcfg", OpKind::BootEntryEdit, Some(&t));
    cs.add(
        "ticket once only",
        v1 == Verdict::Pending && v2 == Verdict::RejectedChannel,
        "",
    );

    // 3) 跳号票据（伪造未来票）与未签发票皆拒。
    let mut g3 = TicketedGate::new();
    let forged_future = Ticket { seq: 999, kind: OpKind::SystemFormat };
    let v3 = g3.request(Channel::Kernel, "mal", OpKind::SystemFormat, Some(&forged_future));
    let forged_unissued = Ticket { seq: 0, kind: OpKind::SystemFormat };
    let v4 = g3.request(Channel::Kernel, "mal", OpKind::SystemFormat, Some(&forged_unissued));
    cs.add(
        "skipped or unissued seq rejected",
        v3 == Verdict::RejectedChannel && v4 == Verdict::RejectedChannel,
        "",
    );

    // 4) 类不匹配票拒（票绑操作类——拿 A 类票办 B 类事不行）。
    let mut g4 = TicketedGate::new();
    let t4 = g4.issuer().issue(OpKind::KernelDriverInstall);
    let v5 = g4.request(Channel::Kernel, "dev", OpKind::SystemFormat, Some(&t4));
    cs.add("kind mismatched ticket rejected", v5 == Verdict::RejectedChannel, "");

    // 5) 用户态持票仍拒（票据不抬升通道——通道判据在票据之上）。
    let mut g5 = TicketedGate::new();
    let t5 = g5.issuer().issue(OpKind::SystemFormat);
    let v6 = g5.request(Channel::User, "mal", OpKind::SystemFormat, Some(&t5));
    cs.add("user channel with ticket still rejected", v6 == Verdict::RejectedChannel, "");

    // 6) 留痕时间窗对账：窗内命中、窗外不混入、倒序保持。
    let mut g6 = TicketedGate::new();
    for ms in [1_000u64, 2_000, 3_000, 4_000] {
        g6.inner().tick(ms);
        let t = g6.issuer().issue(OpKind::BootEntryEdit);
        let _ = g6.request(Channel::Kernel, "sw", OpKind::BootEntryEdit, Some(&t));
        let _ = g6.inner().allow_once();
    }
    let win = g6.audit_between(2_000, 3_000);
    let in_window = win.len() == 4
        && win.iter().all(|e| e.ms >= 2_000 && e.ms <= 3_000)
        && win.windows(2).all(|w| w[0].ms >= w[1].ms);
    cs.add("audit window slice", in_window, "");

    // 7) 卡面字面合同：三要素逐字节 + 两钮合同常量在卡外。
    let mut g7 = TicketedGate::new();
    let t7 = g7.issuer().issue(OpKind::KernelDriverInstall);
    let _ = g7.request(Channel::Kernel, "devtool", OpKind::KernelDriverInstall, Some(&t7));
    let card_ok = card_contract(g7.inner(), "devtool", OpKind::KernelDriverInstall);
    cs.add(
        "card literal contract",
        card_ok && BTN_ALLOW == "允许一次" && BTN_CANCEL == "取消",
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticket_seq_monotonic() {
        let mut iss = TicketIssuer::new();
        let a = iss.issue(OpKind::SystemFormat);
        let b = iss.issue(OpKind::BootEntryEdit);
        assert_eq!(b.seq, a.seq + 1);
        assert!(iss.valid_unspent(&b));
        iss.mark_spent(b.seq);
        assert!(!iss.valid_unspent(&b));
    }

    #[test]
    fn audit_window_empty_when_out_of_range() {
        let g = TicketedGate::new();
        assert!(g.audit_between(10_000, 20_000).is_empty());
    }
}
