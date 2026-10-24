// SPDX-License-Identifier: Apache-2.0
// Varix Kernel — SVSTAR2 · ves100_r05close
// VE-F3700 · R05 组收口双签（R05 组 · 生态运营组）
//
// 锚点：VE-F3700
// 判据原文：R05 收口——20 卡全销、双签闭环、判据全绿、链接证据、交接。
//
// 难点分析：
//   1. 收口双签：实现方+验收方两道签字缺一不可，单签不算收口。
//   2. 链接证据：每张卡的「规格→实现→判据」链路必须可追溯，断链即收口不通过。
//   3. 判据全绿：R05 全组判据（ves81-99）累计通过率 100% 是收口前提。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::CheckSet;

// ── 诊断码（0x9400 起）────────────────────────────────────────────

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x9400;

/// 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C94Code(pub u16);

impl C94Code {
    pub const SIGN_MISSING: Self = Self(SEG_BASE);     // 双签缺失
    pub const LINK_BROKEN: Self = Self(SEG_BASE + 1);  // 链接证据断链
    pub const CARD_OPEN: Self = Self(SEG_BASE + 2);    // 卡未销
    pub const CHECKS_NOT_GREEN: Self = Self(SEG_BASE + 3); // 判据未全绿
    pub const CLOSE_OK: Self = Self(SEG_BASE + 4);     // 收口通过
    pub const HANDOFF_READY: Self = Self(SEG_BASE + 5); // 交接就绪
    pub const SIGN_COMPLETE: Self = Self(SEG_BASE + 6); // 双签完成

    pub fn code(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            Self::SIGN_MISSING => "收口双签缺失——单签不算收口",
            Self::LINK_BROKEN => "链接证据断链——规格→实现→判据不可追溯",
            Self::CARD_OPEN => "存在未销卡——收口前提未满足",
            Self::CHECKS_NOT_GREEN => "判据未全绿——收口前提未满足",
            Self::CLOSE_OK => "R05 收口通过",
            Self::HANDOFF_READY => "交接包就绪",
            Self::SIGN_COMPLETE => "双签完成",
            _ => "未知诊断",
        }
    }
}

// ── 双签 ──────────────────────────────────────────────────────────

/// 签字方。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Signer {
    /// 实现方。
    Implementer,
    /// 验收方。
    Accepter,
}

impl Signer {
    pub fn name(self) -> &'static str {
        match self {
            Self::Implementer => "实现方",
            Self::Accepter => "验收方",
        }
    }
}

/// 双签器。
pub struct DualSign {
    impl_signed: bool,
    accept_signed: bool,
}

impl DualSign {
    pub fn new() -> Self {
        Self { impl_signed: false, accept_signed: false }
    }

    /// 签字。
    pub fn sign(&mut self, who: Signer) {
        match who {
            Signer::Implementer => self.impl_signed = true,
            Signer::Accepter => self.accept_signed = true,
        }
    }

    /// 双签是否闭环。
    pub fn is_complete(&self) -> bool {
        self.impl_signed && self.accept_signed
    }

    /// 检查：缺任一签即拒。
    pub fn check(&self) -> Result<(), C94Code> {
        if self.is_complete() {
            Ok(())
        } else {
            Err(C94Code::SIGN_MISSING)
        }
    }

    /// 已签数。
    pub fn sign_count(&self) -> u32 {
        (self.impl_signed as u32) + (self.accept_signed as u32)
    }
}

impl Default for DualSign {
    fn default() -> Self {
        Self::new()
    }
}

// ── 链接证据 ──────────────────────────────────────────────────────

/// 链路节点。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkNode {
    /// 规格。
    Spec,
    /// 实现。
    Impl,
    /// 判据。
    Check,
}

impl LinkNode {
    pub fn name(self) -> &'static str {
        match self {
            Self::Spec => "规格",
            Self::Impl => "实现",
            Self::Check => "判据",
        }
    }

    pub fn all() -> [LinkNode; 3] {
        [Self::Spec, Self::Impl, Self::Check]
    }
}

/// 单卡链接证据。
pub struct LinkEvidence {
    pub card_id: u64,
    /// 每节点是否就位。
    present: [bool; 3],
}

impl LinkEvidence {
    pub fn new(card_id: u64) -> Self {
        Self { card_id, present: [false; 3] }
    }

    fn idx(n: LinkNode) -> usize {
        match n {
            LinkNode::Spec => 0,
            LinkNode::Impl => 1,
            LinkNode::Check => 2,
        }
    }

    /// 标记节点就位。
    pub fn mark(&mut self, n: LinkNode) {
        self.present[Self::idx(n)] = true;
    }

    /// 三节点是否全就位（规格→实现→判据链完整）。
    pub fn is_complete(&self) -> bool {
        self.present.iter().all(|&b| b)
    }

    /// 缺失节点数。
    pub fn missing(&self) -> usize {
        self.present.iter().filter(|&&b| !b).count()
    }
}

// ── 卡片台账 ──────────────────────────────────────────────────────

/// 卡状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CardState {
    /// 已销。
    Done,
    /// 未销。
    Open,
}

impl CardState {
    pub fn name(self) -> &'static str {
        match self {
            Self::Done => "已销",
            Self::Open => "未销",
        }
    }
}

/// 收口台账。
pub struct CloseLedger {
    /// (card_id, state)。
    cards: Vec<(u64, CardState)>,
    /// 判据是否全绿。
    checks_green: bool,
}

impl CloseLedger {
    pub fn new() -> Self {
        Self { cards: Vec::new(), checks_green: false }
    }

    /// 登记卡。
    pub fn add(&mut self, card_id: u64, state: CardState) {
        self.cards.push((card_id, state));
    }

    /// 销卡。
    pub fn close_card(&mut self, card_id: u64) {
        for entry in self.cards.iter_mut() {
            if entry.0 == card_id {
                entry.1 = CardState::Done;
                return;
            }
        }
    }

    /// 设置判据状态。
    pub fn set_checks(&mut self, green: bool) {
        self.checks_green = green;
    }

    /// 全部已销？
    pub fn all_done(&self) -> bool {
        self.cards.iter().all(|(_, s)| *s == CardState::Done)
    }

    /// 未销数。
    pub fn open_count(&self) -> usize {
        self.cards.iter().filter(|(_, s)| *s == CardState::Open).count()
    }

    pub fn total(&self) -> usize {
        self.cards.len()
    }

    pub fn checks_green(&self) -> bool {
        self.checks_green
    }
}

impl Default for CloseLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 收口器 ────────────────────────────────────────────────────────

/// R05 收口器。
pub struct R05Closer {
    pub sign: DualSign,
    pub ledger: CloseLedger,
    pub evidence: Vec<LinkEvidence>,
}

impl R05Closer {
    pub fn new() -> Self {
        Self {
            sign: DualSign::new(),
            ledger: CloseLedger::new(),
            evidence: Vec::new(),
        }
    }

    /// 添加链接证据。
    pub fn add_evidence(&mut self, e: LinkEvidence) {
        self.evidence.push(e);
    }

    /// 链接证据是否全完整。
    pub fn evidence_ok(&self) -> bool {
        !self.evidence.is_empty() && self.evidence.iter().all(|e| e.is_complete())
    }

    /// 断链数。
    pub fn broken_links(&self) -> usize {
        self.evidence.iter().filter(|e| !e.is_complete()).count()
    }

    /// 尝试收口。
    pub fn close(&mut self) -> Result<(), C94Code> {
        self.sign.check()?;
        if self.ledger.open_count() > 0 {
            return Err(C94Code::CARD_OPEN);
        }
        if !self.ledger.checks_green() {
            return Err(C94Code::CHECKS_NOT_GREEN);
        }
        if !self.evidence_ok() {
            return Err(C94Code::LINK_BROKEN);
        }
        Ok(())
    }
}

impl Default for R05Closer {
    fn default() -> Self {
        Self::new()
    }
}

// ── 交接包 ────────────────────────────────────────────────────────

/// 交接包——收口后移交下游。
pub struct HandoffPackage {
    /// 收口是否通过。
    closed: bool,
    /// 待交接卡片清单。
    cards: Vec<u64>,
}

impl HandoffPackage {
    pub fn new() -> Self {
        Self { closed: false, cards: Vec::new() }
    }

    /// 标记收口通过。
    pub fn mark_closed(&mut self) {
        self.closed = true;
    }

    /// 加入交接清单。
    pub fn add_card(&mut self, id: u64) {
        self.cards.push(id);
    }

    /// 是否就绪（收口通过+清单非空）。
    pub fn ready(&self) -> bool {
        self.closed && !self.cards.is_empty()
    }

    pub fn card_count(&self) -> usize {
        self.cards.len()
    }
}

impl Default for HandoffPackage {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ──────────────────────────────────────────────────────────

pub fn run_ves100_checks() -> CheckSet {
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves100_r05close");
    {
        // 01 单签不算收口
        let mut ds = DualSign::new();
        ds.sign(Signer::Implementer);
        let r = ds.check();
        chk(
            &mut set,
            "C394-01",
            r.err() == Some(C94Code::SIGN_MISSING) && ds.sign_count() == 1,
            "只实现方签字不算收口",
        );
    }
    {
        // 02 双签闭环
        let mut ds = DualSign::new();
        ds.sign(Signer::Implementer);
        ds.sign(Signer::Accepter);
        chk(&mut set, "C394-02", ds.check().is_ok() && ds.is_complete(), "实现+验收双签闭环");
    }
    {
        // 03 链接证据三节点
        let mut e = LinkEvidence::new(1);
        e.mark(LinkNode::Spec);
        e.mark(LinkNode::Impl);
        e.mark(LinkNode::Check);
        chk(&mut set, "C394-03", e.is_complete() && e.missing() == 0, "规格→实现→判据三节点全就位");
    }
    {
        // 04 断链检出
        let mut e = LinkEvidence::new(2);
        e.mark(LinkNode::Spec);
        e.mark(LinkNode::Impl); // 缺判据
        chk(&mut set, "C394-04", !e.is_complete() && e.missing() == 1, "缺判据节点即断链");
    }
    {
        // 05 未销卡阻收口
        let mut c = R05Closer::new();
        c.sign.sign(Signer::Implementer);
        c.sign.sign(Signer::Accepter);
        c.ledger.add(1, CardState::Done);
        c.ledger.add(2, CardState::Open);
        c.ledger.set_checks(true);
        let mut e = LinkEvidence::new(1);
        e.mark(LinkNode::Spec);
        e.mark(LinkNode::Impl);
        e.mark(LinkNode::Check);
        c.add_evidence(e);
        let r = c.close();
        chk(
            &mut set,
            "C394-05",
            r.err() == Some(C94Code::CARD_OPEN) && c.ledger.open_count() == 1,
            "存在未销卡→收口不通过",
        );
    }
    {
        // 06 判据未绿阻收口
        let mut c = R05Closer::new();
        c.sign.sign(Signer::Implementer);
        c.sign.sign(Signer::Accepter);
        c.ledger.add(1, CardState::Done);
        c.ledger.set_checks(false);
        let mut e = LinkEvidence::new(1);
        e.mark(LinkNode::Spec);
        e.mark(LinkNode::Impl);
        e.mark(LinkNode::Check);
        c.add_evidence(e);
        let r = c.close();
        chk(
            &mut set,
            "C394-06",
            r.err() == Some(C94Code::CHECKS_NOT_GREEN),
            "判据未全绿→收口不通过",
        );
    }
    {
        // 07 断链阻收口
        let mut c = R05Closer::new();
        c.sign.sign(Signer::Implementer);
        c.sign.sign(Signer::Accepter);
        c.ledger.add(1, CardState::Done);
        c.ledger.set_checks(true);
        let mut e = LinkEvidence::new(1);
        e.mark(LinkNode::Spec); // 缺 Impl/Check
        c.add_evidence(e);
        let r = c.close();
        chk(
            &mut set,
            "C394-07",
            r.err() == Some(C94Code::LINK_BROKEN) && c.broken_links() == 1,
            "链接证据断链→收口不通过",
        );
    }
    {
        // 08 全绿收口通过
        let mut c = R05Closer::new();
        c.sign.sign(Signer::Implementer);
        c.sign.sign(Signer::Accepter);
        c.ledger.add(1, CardState::Done);
        c.ledger.add(2, CardState::Done);
        c.ledger.set_checks(true);
        let mut e = LinkEvidence::new(1);
        e.mark(LinkNode::Spec);
        e.mark(LinkNode::Impl);
        e.mark(LinkNode::Check);
        c.add_evidence(e);
        let r = c.close();
        chk(&mut set, "C394-08", r.is_ok(), "双签+全销+全绿+证据完整→收口通过");
    }
    {
        // 09 销卡动作
        let mut led = CloseLedger::new();
        led.add(1, CardState::Open);
        led.close_card(1);
        chk(&mut set, "C394-09", led.all_done() && led.open_count() == 0, "销卡后全销");
    }
    {
        // 10 空台账不满足收口（无卡即无意义）
        let mut c = R05Closer::new();
        c.sign.sign(Signer::Implementer);
        c.sign.sign(Signer::Accepter);
        c.ledger.set_checks(true);
        let r = c.close();
        chk(
            &mut set,
            "C394-10",
            r.err() == Some(C94Code::LINK_BROKEN),
            "无证据记录收口不通过",
        );
    }
    {
        // 11 交接包就绪
        let mut hp = HandoffPackage::new();
        hp.mark_closed();
        hp.add_card(1);
        hp.add_card(2);
        chk(&mut set, "C394-11", hp.ready() && hp.card_count() == 2, "收口通过+清单非空→交接就绪");
    }
    {
        // 12 交接未就绪
        let mut hp = HandoffPackage::new();
        hp.add_card(1); // 未 mark_closed
        chk(&mut set, "C394-12", !hp.ready(), "收口未通过交接不就绪");
    }
    set
}
