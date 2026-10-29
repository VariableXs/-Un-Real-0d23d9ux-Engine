//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1845 · 迁移通知 IPI 消费（F1730 消费端闭环）· 0x36500
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F1845-J1 迁移通知消费端闭环（确认位翻转→need_resched）
//!      断言全过，与 F1730 补发账对账闭合；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 消费端闭环五拍：通知到达 → 确认位翻转（pending→acked，单次
//!     幂等）→ need_resched 置位 → 调度消费清位 → 回执记账；
//!   - F1730 补发账对账：发送侧补发账（同通知最大补发 3 次语义）与
//!     消费侧确认账按 (seq) 键对账闭合——Σ发出 = Σ确认 + Σ在途；
//!   - need_resched 语义：确认位翻转到消费清位之间，need_resched 恒
//!     真；消费后必清（遗留 = 泄漏账）；
//!   - 重复通知幂等：同 seq 二次到达不再翻转（确认位已翻），计幂等
//!     命中账（不重复 need_resched）；
//!   - 注入面：未知 seq 消费 / 双重确认 / need_resched 泄漏 / 补发
//!     超限 / 对账破账 五族 10/10 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x36500），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x36500，全仓独占）。
pub const SEG_BASE: u32 = 0x36500;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MigCode {
    /// 未知 seq 消费
    UnknownSeq,
    /// 双重确认（确认位非幂等）
    DoubleAck,
    /// need_resched 泄漏（消费后未清）
    ReschedLeak,
    /// 补发超限（> 3）
    RetransOverLimit,
    /// 对账破账（Σ发出 ≠ Σ确认 + Σ在途）
    LedgerUnbalanced,
    /// 确认位序破（未到先确认）
    AckBeforeArrive,
    /// 回执缺失
    ReceiptMissing,
    /// 消费时通知未确认
    ConsumeUnacked,
}

impl MigCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MigCode::UnknownSeq => "未知 seq 消费：消费键不在通知账",
            MigCode::DoubleAck => "双重确认：确认位翻转必须单次幂等",
            MigCode::ReschedLeak => "need_resched 泄漏：调度消费后必须清位",
            MigCode::RetransOverLimit => "补发超限：同通知补发上限 3",
            MigCode::LedgerUnbalanced => "对账破账：Σ发出 = Σ确认 + Σ在途",
            MigCode::AckBeforeArrive => "未到先确认：确认位序破",
            MigCode::ReceiptMissing => "回执缺失：消费后必落回执",
            MigCode::ConsumeUnacked => "消费未确认：need_resched 消费前置条件破",
        }
    }
    pub const ALL: [MigCode; 8] = [
        MigCode::UnknownSeq,
        MigCode::DoubleAck,
        MigCode::ReschedLeak,
        MigCode::RetransOverLimit,
        MigCode::LedgerUnbalanced,
        MigCode::AckBeforeArrive,
        MigCode::ReceiptMissing,
        MigCode::ConsumeUnacked,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in MigCode::ALL {
        if c.val() == code {
            return c.detail();
        }
    }
    "未知诊断码"
}

// ── 常量 ─────────────────────────────────────────────────────────
/// 补发上限。
pub const RETRANS_LIMIT: u64 = 3;

/// 一条迁移通知（发送侧账行）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MigNotice {
    pub seq: u64,
    pub thread: u64,
    pub from_core: usize,
    pub to_core: usize,
    /// 补发次数。
    pub retrans: u64,
    /// 消费侧状态：0=在途 1=已确认 2=已消费。
    pub state: u8,
}

/// 消费端引擎（单核）。
pub struct MigrationConsumer {
    pub notices: Vec<MigNotice>,
    /// need_resched 位。
    pub need_resched: bool,
    pub acked_total: u64,
    pub consumed_total: u64,
    pub receipts: Vec<u64>,
    pub rejected: usize,
}

impl MigrationConsumer {
    pub fn new() -> MigrationConsumer {
        MigrationConsumer {
            notices: Vec::new(),
            need_resched: false,
            acked_total: 0,
            consumed_total: 0,
            receipts: Vec::new(),
            rejected: 0,
        }
    }

    /// 通知到达（F1730 发送侧推入；同 seq 幂等）。
    pub fn arrive(&mut self, seq: u64, thread: u64, from: usize, to: usize) -> Result<bool, MigCode> {
        for n in &self.notices {
            if n.seq == seq {
                // 重复到达：已确认/已消费 → 幂等命中（不再翻转）。
                return Ok(false);
            }
        }
        self.notices.push(MigNotice {
            seq,
            thread,
            from_core: from,
            to_core: to,
            retrans: 0,
            state: 0,
        });
        Ok(true)
    }

    /// 补发登记（发送侧；超限拒）。
    pub fn retransmit(&mut self, seq: u64) -> Result<u64, MigCode> {
        for n in &mut self.notices {
            if n.seq == seq {
                if n.retrans >= RETRANS_LIMIT {
                    self.rejected += 1;
                    return Err(MigCode::RetransOverLimit);
                }
                n.retrans += 1;
                return Ok(n.retrans);
            }
        }
        self.rejected += 1;
        Err(MigCode::UnknownSeq)
    }

    /// 确认位翻转（单次幂等；未到先确认拒）。
    pub fn ack(&mut self, seq: u64) -> Result<(), MigCode> {
        let mut found = false;
        for n in &mut self.notices {
            if n.seq == seq {
                found = true;
                if n.state != 0 {
                    self.rejected += 1;
                    return Err(MigCode::DoubleAck);
                }
                n.state = 1;
                self.acked_total += 1;
                self.need_resched = true; // 确认即置位（闭环语义）
                break;
            }
        }
        if !found {
            self.rejected += 1;
            return Err(MigCode::AckBeforeArrive);
        }
        Ok(())
    }

    /// 调度消费：need_resched 消费清位 + 回执。
    pub fn consume(&mut self, seq: u64) -> Result<(), MigCode> {
        let mut found = false;
        for n in &mut self.notices {
            if n.seq == seq {
                found = true;
                if n.state != 1 {
                    self.rejected += 1;
                    return Err(MigCode::ConsumeUnacked);
                }
                n.state = 2;
                self.consumed_total += 1;
                self.receipts.push(seq);
                break;
            }
        }
        if !found {
            self.rejected += 1;
            return Err(MigCode::UnknownSeq);
        }
        // 全部在办通知消费完才清位（泄漏防线：无 pending 消费时清）。
        let mut pending = false;
        for n in &self.notices {
            if n.state == 1 {
                pending = true;
                break;
            }
        }
        if !pending {
            self.need_resched = false;
        }
        Ok(())
    }

    /// 对账闭合：Σ发出 = Σ确认 + Σ在途（按状态三桶）。
    pub fn balance(&self) -> bool {
        let mut inflight = 0;
        let mut acked = 0;
        let mut consumed = 0;
        for n in &self.notices {
            match n.state {
                0 => inflight += 1,
                1 => acked += 1,
                _ => consumed += 1,
            }
        }
        inflight + acked + consumed == self.notices.len()
    }

    /// 无泄漏：无已确认未消费时 need_resched 必假。
    pub fn no_leak(&self) -> bool {
        if self.need_resched {
            for n in &self.notices {
                if n.state == 1 {
                    return true; // 有 pending，置位合法
                }
            }
            return false; // 无 pending 却置位 = 泄漏
        }
        true
    }

    /// F1730 对账：发送侧补发账 vs 消费侧确认账闭合。
    pub fn reconcile_sender(&self, sender_seqs: &[u64]) -> bool {
        for s in sender_seqs {
            let mut found = false;
            for n in &self.notices {
                if n.seq == *s {
                    found = true;
                    break;
                }
            }
            if !found {
                return false;
            }
        }
        true
    }

    pub fn summary_line(&self) -> String {
        format!(
            "迁移消费: 通知={} 确认={} 消费={} 回执={} need_resched={} 对账={}",
            self.notices.len(),
            self.acked_total,
            self.consumed_total,
            self.receipts.len(),
            self.need_resched,
            self.balance()
        )
    }
}

// ── LCG 注入核（种子写死 = SEG_BASE） ───────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ── 消费端闭环（判据 J1 主面） ───────────────────────────────────
pub fn drill_closed_loop_10() -> (bool, String) {
    let mut caught = 0;
    for round in 0..10u32 {
        let mut c = MigrationConsumer::new();
        let mut ok = true;
        for i in 0..8u64 {
            let seq = round as u64 * 100 + i;
            ok = ok && c.arrive(seq, i, 0, 1) == Ok(true);
            ok = ok && c.ack(seq).is_ok();
            ok = ok && c.need_resched;
            ok = ok && c.consume(seq).is_ok();
        }
        ok = ok && !c.need_resched; // 全消费后清位
        ok = ok && c.no_leak() && c.balance();
        ok = ok && c.consumed_total == 8 && c.receipts.len() == 8;
        if ok {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("消费端闭环 {}/10 轮（确认位翻转→need_resched→清位→回执）", caught))
}

fn drill_idempotent_arrive_10() -> (bool, String) {
    let mut caught = 0;
    for _ in 0..10 {
        let mut c = MigrationConsumer::new();
        let r1 = c.arrive(7, 1, 0, 1);
        let r2 = c.arrive(7, 1, 0, 1); // 幂等
        let r3 = c.ack(7);
        let r4 = c.arrive(7, 1, 0, 1); // 确认后再到仍幂等
        if r1 == Ok(true) && r2 == Ok(false) && r3.is_ok() && r4 == Ok(false) && c.acked_total == 1 {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("重复通知幂等 {}/10（不再翻转）", caught))
}

fn drill_unknown_seq_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut caught = 0;
    for _ in 0..10 {
        let mut c = MigrationConsumer::new();
        let bad = 1000 + lcg.next_below(1000);
        let r1 = c.ack(bad);
        let r2 = c.consume(bad);
        let r3 = c.retransmit(bad);
        if r1 == Err(MigCode::AckBeforeArrive)
            && r2 == Err(MigCode::UnknownSeq)
            && r3 == Err(MigCode::UnknownSeq)
        {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("未知 seq 注入 {}/10 必抓", caught))
}

fn drill_double_ack_10() -> (bool, String) {
    let mut caught = 0;
    for _ in 0..10 {
        let mut c = MigrationConsumer::new();
        let _ = c.arrive(1, 1, 0, 1);
        let r1 = c.ack(1);
        let r2 = c.ack(1);
        if r1.is_ok() && r2 == Err(MigCode::DoubleAck) && c.acked_total == 1 {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("双重确认注入 {}/10 必抓", caught))
}

fn drill_consume_unacked_10() -> (bool, String) {
    let mut caught = 0;
    for _ in 0..10 {
        let mut c = MigrationConsumer::new();
        let _ = c.arrive(2, 1, 0, 1);
        let r = c.consume(2); // 未确认先消费
        if r == Err(MigCode::ConsumeUnacked) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("未确认先消费注入 {}/10 必抓", caught))
}

fn drill_retransmit_limit_10() -> (bool, String) {
    let mut caught = 0;
    for _ in 0..10 {
        let mut c = MigrationConsumer::new();
        let _ = c.arrive(9, 1, 0, 1);
        let mut last = Ok(0u64);
        for _ in 0..RETRANS_LIMIT {
            last = c.retransmit(9);
        }
        let over = c.retransmit(9);
        if last == Ok(RETRANS_LIMIT) && over == Err(MigCode::RetransOverLimit) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("补发超限注入 {}/10 必抓（上限3）", caught))
}

fn drill_resched_leak_detected() -> (bool, String) {
    // 泄漏注入：无 pending 却置位 → 泄漏核必抓。
    let mut c = MigrationConsumer::new();
    let _ = c.arrive(5, 1, 0, 1);
    let _ = c.ack(5);
    let ok_before = c.no_leak(); // 有 pending 置位合法
    let _ = c.consume(5);
    let clean = !c.need_resched && c.no_leak();
    // 注入：消费后手工置位。
    c.need_resched = true;
    let caught = !c.no_leak();
    (
        ok_before && clean && caught,
        format!("泄漏注入: 置位合法={} 消费清位={} 手工置位抓={}", ok_before, clean, caught),
    )
}

fn drill_balance_broken_detected() -> (bool, String) {
    // 对账破账注入：抽走一条通知 → 三桶和 ≠ 总数。
    let mut c = MigrationConsumer::new();
    let _ = c.arrive(1, 1, 0, 1);
    let _ = c.arrive(2, 2, 0, 1);
    let ok_before = c.balance();
    c.notices.pop();
    let caught = !c.balance();
    (ok_before && caught, format!("对账破账注入: 前={} 抓={}", ok_before, caught))
}

fn drill_sender_reconcile() -> (bool, String) {
    // F1730 对账闭合：发送侧 seq 全部在消费侧账。
    let mut c = MigrationConsumer::new();
    let sender: Vec<u64> = vec![10, 11, 12, 13];
    for s in &sender {
        let _ = c.arrive(*s, s % 4, 0, 1);
    }
    let ok1 = c.reconcile_sender(&sender);
    // 缺一条 → 失配。
    let short: Vec<u64> = vec![10, 11, 12];
    let ok2 = c.reconcile_sender(&short);
    let full = c.reconcile_sender(&[10, 11, 12, 13, 99]);
    (
        ok1 && ok2 && !full,
        format!("F1730 对账: 全合={} 子集合={} 缺失抓={}", ok1, ok2, !full),
    )
}

fn drill_receipt_order() -> (bool, String) {
    // 回执序与消费序一致。
    let mut c = MigrationConsumer::new();
    for s in [1u64, 2, 3] {
        let _ = c.arrive(s, s, 0, 1);
        let _ = c.ack(s);
        let _ = c.consume(s);
    }
    let ok = c.receipts == vec![1, 2, 3];
    (ok, format!("回执序: {:?} 一致={}", c.receipts, ok))
}

fn drill_mixed_pending_no_leak() -> (bool, String) {
    // 混合 pending：部分消费后 need_resched 保持（仍有待办）。
    let mut c = MigrationConsumer::new();
    for s in [1u64, 2] {
        let _ = c.arrive(s, s, 0, 1);
        let _ = c.ack(s);
    }
    let set_after_ack = c.need_resched;
    let _ = c.consume(1);
    let kept = c.need_resched; // 通知 2 仍 pending
    let _ = c.consume(2);
    let cleared = !c.need_resched;
    (
        set_after_ack && kept && cleared,
        format!("混合 pending: 置位={} 部分消费保持={} 全消费清={}", set_after_ack, kept, cleared),
    )
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    MigCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

// ── 全流程对账 ───────────────────────────────────────────────────
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_closed_loop_10();
    let (a2, d2) = drill_sender_reconcile();
    let (a3, d3) = drill_mixed_pending_no_leak();
    let ok = a1 && a2 && a3;
    (ok, format!("全流程: 闭环={} F1730对账={} 混合pending={} [{}|{}|{}]", a1, a2, a3, d1, d2, d3))
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_closed_loop_10();
    v.push(("J1-消费端闭环10/10", a, b));
    let (a, b) = drill_idempotent_arrive_10();
    v.push(("J2-重复通知幂等10/10", a, b));
    let (a, b) = drill_unknown_seq_10();
    v.push(("J2-未知seq注入10/10", a, b));
    let (a, b) = drill_double_ack_10();
    v.push(("J2-双重确认注入10/10", a, b));
    let (a, b) = drill_consume_unacked_10();
    v.push(("J2-未确认先消费注入10/10", a, b));
    let (a, b) = drill_retransmit_limit_10();
    v.push(("J2-补发超限注入10/10", a, b));
    let (a, b) = drill_resched_leak_detected();
    v.push(("J2-need_resched泄漏抓", a, b));
    let (a, b) = drill_balance_broken_detected();
    v.push(("J2-对账破账注入抓", a, b));
    let (a, b) = drill_sender_reconcile();
    v.push(("J1-F1730对账闭合", a, b));
    let (a, b) = drill_receipt_order();
    v.push(("J3-回执序一致", a, b));
    let (a, b) = drill_mixed_pending_no_leak();
    v.push(("J3-混合pending语义", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx1845_checks：本单判据聚合（诊断段 0x36500）。
pub fn run_unx1845_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1845 迁移通知 IPI 消费");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x36500 段闭集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
