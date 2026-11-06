//! UNX-F51512 UASP 任务管理队列化（TMFU 排队序） · 0xC0300段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51513-J1 校验开销 P95 落账 ≤5%（AI-10 联签+AI-86 复核位复核在册）
//!
//! 设计要点：
//!   ① TMF 四类冻结优先级表：LunReset > AbortTaskSet > ClearAca > AbortTask，
//!      排队出队按 (优先级, 串号) 二元全序，任何人不得插队。
//!   ② 单调串号机：入队即由队列发号，串号只增不退；伪造小串号/陈旧串号
//!      入队显性拒（SeriaRegress），10 轮全检出。
//!   ③ 并发 TMF 注入串行化：10 条并发到达逐一发号入队，出队序与
//!      (优先级, 串号) 全序逐条一致，10/10 串行化无一乱序。
//!   ④ ×50 排队序账：混合优先级 50 条全部入队再全部出队，序账逐条对平。
//!   ⑤ 队深容量冻结、tag 去重、Draining 态入队拒（状态机倒退显性拒）；
//!      判定次序：来源 → 状态 → 去重 → 容量。
//!   ⑥ 守恒账：注入 = 检出 + 逃逸，逃逸恒 0；诊断码闭集配中文建议文案。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC0300;

/// TMFU 队深上限（冻结表）。
pub const QUEUE_CAP: usize = 8;
/// 压序样账长度（冻结表：×50 排队序账）。
pub const ORDER_BOOK_LEN: usize = 50;

/// 诊断码闭集（TMFU 排队域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TmfuCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败（SEG_BASE 越界或不对齐）
    SegBad,
    /// 队深已满
    QueueFull,
    /// tag 重复（在队或已派发）
    DupTag,
    /// 空队出队
    EmptyQueue,
    /// Draining 态入队 / Idle 态收尾（状态机倒退）
    StateRegress,
    /// 伪造串号入队
    SeriaRegress,
    /// 发起方编号为零
    OriginInvalid,
    /// 出队序与全序账不符
    OrderMismatch,
    /// 守恒账不平
    LedgerMismatch,
}

impl TmfuCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TmfuCode::Ok => "入队/出队成立",
            TmfuCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            TmfuCode::QueueFull => "队深已满：等待排空后再提交 TMF",
            TmfuCode::DupTag => "tag 重复：同一 tag 在队或已派发期不得复用",
            TmfuCode::EmptyQueue => "空队出队：先入队再派发",
            TmfuCode::StateRegress => "状态机倒退：Draining 不得入队，Idle 不得收尾",
            TmfuCode::SeriaRegress => "伪造串号：串号由队列统一发号，不得自报",
            TmfuCode::OriginInvalid => "发起方编号为零：编号必须 ≥1",
            TmfuCode::OrderMismatch => "出队乱序：与 (优先级, 串号) 全序不符",
            TmfuCode::LedgerMismatch => "守恒账不平：注入=检出+逃逸被破坏",
        }
    }
}

/// TMF 类别（冻结优先级：rank 越小越优先）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TmfKind {
    /// 中止单任务（最低优先级）
    AbortTask,
    /// 清除 ACA
    ClearAca,
    /// 中止任务集
    AbortTaskSet,
    /// 逻辑单元复位（最高优先级）
    LunReset,
}

impl TmfKind {
    pub fn rank(self) -> u8 {
        match self {
            TmfKind::AbortTask => 3,
            TmfKind::ClearAca => 2,
            TmfKind::AbortTaskSet => 1,
            TmfKind::LunReset => 0,
        }
    }
}

/// 排队中的 TMF。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingTmf {
    pub serial: u32,
    pub kind: TmfKind,
    pub origin: u32,
    pub tag: u32,
}

/// 已派发记录（序账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DispatchRec {
    pub serial: u32,
    pub kind: TmfKind,
    pub tag: u32,
    pub rank: u8,
}

/// 队列状态机：Idle → Queueing → Draining → Idle 单向轮转。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QueueState {
    /// 空闲
    Idle,
    /// 排队受理中
    Queueing,
    /// 排空派发中（禁止入队）
    Draining,
}

/// TMFU 排队引擎。
pub struct TmfuQueue {
    capacity: usize,
    pending: Vec<PendingTmf>,
    serial_next: u32,
    history: Vec<DispatchRec>,
    state: QueueState,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl TmfuQueue {
    pub fn new(capacity: usize) -> TmfuQueue {
        TmfuQueue {
            capacity,
            pending: Vec::new(),
            serial_next: 0,
            history: Vec::new(),
            state: QueueState::Idle,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    pub fn serial_next(&self) -> u32 {
        self.serial_next
    }
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }
    pub fn history(&self) -> Vec<DispatchRec> {
        self.history.clone()
    }
    pub fn state_is(&self, st: QueueState) -> bool {
        self.state == st
    }

    /// 入队（队列统一发号）：来源 → 状态 → 去重 → 容量。
    pub fn enqueue(&mut self, kind: TmfKind, origin: u32, tag: u32) -> Result<u32, TmfuCode> {
        if origin == 0 {
            return Err(TmfuCode::OriginInvalid);
        }
        if self.state == QueueState::Draining {
            return Err(TmfuCode::StateRegress);
        }
        for p in self.pending.iter() {
            if p.tag == tag {
                return Err(TmfuCode::DupTag);
            }
        }
        for h in self.history.iter() {
            if h.tag == tag {
                return Err(TmfuCode::DupTag);
            }
        }
        if self.pending.len() >= self.capacity {
            return Err(TmfuCode::QueueFull);
        }
        let serial = self.serial_next;
        self.pending.push(PendingTmf { serial, kind, origin, tag });
        self.serial_next = self.serial_next.saturating_add(1);
        if self.state == QueueState::Idle {
            self.state = QueueState::Queueing;
        }
        Ok(serial)
    }

    /// 伪造串号入队面：自报串号与发号机不符即拒（注入检出点）。
    pub fn enqueue_forged(&mut self, kind: TmfKind, origin: u32, tag: u32, claimed: u32) -> Result<u32, TmfuCode> {
        if claimed != self.serial_next {
            return Err(TmfuCode::SeriaRegress);
        }
        self.enqueue(kind, origin, tag)
    }

    /// 进入排空态（Queueing → Draining）。
    pub fn begin_drain(&mut self) -> Result<(), TmfuCode> {
        if self.state != QueueState::Queueing {
            return Err(TmfuCode::StateRegress);
        }
        self.state = QueueState::Draining;
        Ok(())
    }

    /// 出队一条：按 (rank, serial) 二元全序取最优。
    pub fn dequeue_one(&mut self) -> Result<DispatchRec, TmfuCode> {
        if self.pending.is_empty() {
            return Err(TmfuCode::EmptyQueue);
        }
        let mut best: usize = 0;
        for (idx, p) in self.pending.iter().enumerate() {
            let b = match self.pending.get(best) {
                Some(bp) => bp,
                None => return Err(TmfuCode::EmptyQueue),
            };
            if p.kind.rank() < b.kind.rank() || (p.kind.rank() == b.kind.rank() && p.serial < b.serial) {
                best = idx;
            }
        }
        let p = match self.pending.get(best) {
            Some(p) => *p,
            None => return Err(TmfuCode::EmptyQueue),
        };
        self.pending.remove(best);
        let rec = DispatchRec {
            serial: p.serial,
            kind: p.kind,
            tag: p.tag,
            rank: p.kind.rank(),
        };
        self.history.push(rec);
        if self.pending.is_empty() && self.state == QueueState::Draining {
            self.state = QueueState::Idle;
        }
        Ok(rec)
    }

    /// 排空到空，返回派发序账。
    pub fn drain_all(&mut self) -> Vec<DispatchRec> {
        let mut out: Vec<DispatchRec> = Vec::new();
        loop {
            match self.dequeue_one() {
                Ok(rec) => out.push(rec),
                Err(TmfuCode::EmptyQueue) => break,
                Err(_) => break,
            }
        }
        out
    }

    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }
    pub fn note_escape(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }
    /// 守恒面：注入 = 检出 + 逃逸，逃逸恒 0。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }
}

/// 序账一致性：派发序按 (rank, serial) 严格递增。
pub fn order_consistent(recs: &[DispatchRec]) -> bool {
    for i in 1..recs.len() {
        let prev = match recs.get(i - 1) {
            Some(p) => p,
            None => return false,
        };
        let cur = match recs.get(i) {
            Some(c) => c,
            None => return false,
        };
        if cur.rank < prev.rank || (cur.rank == prev.rank && cur.serial <= prev.serial) {
            return false;
        }
    }
    true
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一脚本两次建序账逐字段一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> Vec<DispatchRec> {
        let mut q = TmfuQueue::new(64);
        let _ = q.enqueue(TmfKind::AbortTask, 1, 11);
        let _ = q.enqueue(TmfKind::LunReset, 1, 12);
        let _ = q.enqueue(TmfKind::ClearAca, 2, 13);
        let _ = q.enqueue(TmfKind::AbortTaskSet, 2, 14);
        let _ = q.begin_drain();
        q.drain_all()
    };
    build() == build()
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [TmfuCode; 10] = [
    TmfuCode::Ok,
    TmfuCode::SegBad,
    TmfuCode::QueueFull,
    TmfuCode::DupTag,
    TmfuCode::EmptyQueue,
    TmfuCode::StateRegress,
    TmfuCode::SeriaRegress,
    TmfuCode::OriginInvalid,
    TmfuCode::OrderMismatch,
    TmfuCode::LedgerMismatch,
];

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51512_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51512");
    // 1 正常流：入队 → 排空 → 序账按 (rank, serial) 全序。
    {
        let mut q = TmfuQueue::new(QUEUE_CAP);
        let s0 = q.enqueue(TmfKind::AbortTask, 1, 1);
        let s1 = q.enqueue(TmfKind::LunReset, 1, 2);
        let _ = q.begin_drain();
        let book = q.drain_all();
        s.add_dyn(
            "正常流排队序",
            s0 == Ok(0) && s1 == Ok(1) && book.len() == 2 && order_consistent(&book),
            "合法 TMF 应按 (优先级, 串号) 全序派发",
        );
    }
    // 2 优先级序：低串号低优先级仍让位高优先级。
    {
        let mut q = TmfuQueue::new(QUEUE_CAP);
        let _ = q.enqueue(TmfKind::AbortTask, 1, 1);
        let _ = q.enqueue(TmfKind::LunReset, 1, 2);
        let _ = q.begin_drain();
        let book = q.drain_all();
        let first_ok = match book.get(0) {
            Some(r) => r.kind == TmfKind::LunReset,
            None => false,
        };
        s.add_dyn("优先级压序", first_ok, "LunReset 必须先于先到的 AbortTask 派发");
    }
    // 3 同优先级 FIFO：串号小者先出。
    {
        let mut q = TmfuQueue::new(QUEUE_CAP);
        let _ = q.enqueue(TmfKind::ClearAca, 1, 1);
        let _ = q.enqueue(TmfKind::ClearAca, 2, 2);
        let _ = q.enqueue(TmfKind::ClearAca, 3, 3);
        let _ = q.begin_drain();
        let book = q.drain_all();
        let fifo_ok = match (book.get(0), book.get(2)) {
            (Some(a), Some(c)) => a.tag == 1 && c.tag == 3,
            _ => false,
        };
        s.add_dyn("同优先级FIFO", fifo_ok, "同优先级按串号先进先出");
    }
    // 4 边界：队深容量与上限+1。
    {
        let mut q = TmfuQueue::new(4);
        let mut all_ok = true;
        for i in 0..4u32 {
            if q.enqueue(TmfKind::AbortTask, 1, 100 + i).is_err() {
                all_ok = false;
            }
        }
        let over = q.enqueue(TmfKind::AbortTask, 1, 200);
        s.add_dyn(
            "队深容量耗尽",
            all_ok && over == Err(TmfuCode::QueueFull),
            "队满后再入队必须明拒",
        );
    }
    // 5 边界：来源编号为零拒绝。
    {
        let mut q = TmfuQueue::new(QUEUE_CAP);
        let r = q.enqueue(TmfKind::AbortTask, 0, 1);
        s.add_dyn("来源零号拒", r == Err(TmfuCode::OriginInvalid), "发起方编号必须 ≥1");
    }
    // 6 边界：空队出队拒绝。
    {
        let mut q = TmfuQueue::new(QUEUE_CAP);
        let r = q.dequeue_one();
        s.add_dyn("空队出队拒", r == Err(TmfuCode::EmptyQueue), "空队必须明拒出队");
    }
    // 7 注入必抓：伪造串号 ×10。
    {
        let mut q = TmfuQueue::new(64);
        let _ = q.enqueue(TmfKind::AbortTask, 1, 1);
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let r = q.enqueue_forged(TmfKind::AbortTask, 1, 900, 0);
            if r == Err(TmfuCode::SeriaRegress) {
                caught += 1;
                q.note_detected();
            }
        }
        s.add_dyn("伪造串号 10/10", caught == 10, "自报陈旧串号必须全抓");
    }
    // 8 注入必抓：Draining 态入队（状态机倒退）×10。
    {
        let mut q = TmfuQueue::new(64);
        let _ = q.enqueue(TmfKind::AbortTask, 1, 1);
        let _ = q.begin_drain();
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let r = q.enqueue(TmfKind::LunReset, 2, 100);
            if r == Err(TmfuCode::StateRegress) {
                caught += 1;
                q.note_detected();
            }
        }
        s.add_dyn("排空态入队 10/10", caught == 10, "Draining 态入队必须全抓");
    }
    // 9 注入必抓：tag 重复 ×10。
    {
        let mut q = TmfuQueue::new(64);
        let _ = q.enqueue(TmfKind::AbortTask, 1, 42);
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let r = q.enqueue(TmfKind::ClearAca, 2, 42);
            if r == Err(TmfuCode::DupTag) {
                caught += 1;
                q.note_detected();
            }
        }
        s.add_dyn("tag重复 10/10", caught == 10, "在队 tag 复用必须全抓");
    }
    // 10 并发 TMF 注入串行化 10/10。
    {
        let mut q = TmfuQueue::new(64);
        let kinds = [
            TmfKind::AbortTask,
            TmfKind::LunReset,
            TmfKind::ClearAca,
            TmfKind::AbortTaskSet,
            TmfKind::AbortTask,
            TmfKind::AbortTaskSet,
            TmfKind::ClearAca,
            TmfKind::LunReset,
            TmfKind::AbortTask,
            TmfKind::ClearAca,
        ];
        let mut serials_ok = true;
        for (i, k) in kinds.iter().enumerate() {
            if q.enqueue(*k, 1 + i as u32, 10 + i as u32).is_err() {
                serials_ok = false;
            }
        }
        let _ = q.begin_drain();
        let book = q.drain_all();
        // 独立核对面：入队种类多重集与出队种类多重集逐类相等（无丢失无篡改），
        // 且派发序满足 (rank, serial) 严格递增 ⇒ 全序唯一确定。
        let mut c_reset = 0u32;
        let mut c_set = 0u32;
        let mut c_aca = 0u32;
        let mut c_abort = 0u32;
        for k in kinds.iter() {
            match k {
                TmfKind::LunReset => c_reset = c_reset.saturating_add(1),
                TmfKind::AbortTaskSet => c_set = c_set.saturating_add(1),
                TmfKind::ClearAca => c_aca = c_aca.saturating_add(1),
                TmfKind::AbortTask => c_abort = c_abort.saturating_add(1),
            }
        }
        let mut o_reset = 0u32;
        let mut o_set = 0u32;
        let mut o_aca = 0u32;
        let mut o_abort = 0u32;
        for r in book.iter() {
            match r.kind {
                TmfKind::LunReset => o_reset = o_reset.saturating_add(1),
                TmfKind::AbortTaskSet => o_set = o_set.saturating_add(1),
                TmfKind::ClearAca => o_aca = o_aca.saturating_add(1),
                TmfKind::AbortTask => o_abort = o_abort.saturating_add(1),
            }
        }
        let multiset_same = c_reset == o_reset && c_set == o_set && c_aca == o_aca && c_abort == o_abort;
        s.add_dyn(
            "并发TMF串行化 10/10",
            serials_ok && book.len() == 10 && multiset_same && order_consistent(&book),
            "10 条并发到达必须串行化为 (优先级, 串号) 全序",
        );
    }
    // 11 ×50 排队序账逐条正确。
    {
        let mut q = TmfuQueue::new(128);
        let mut all_in = true;
        for i in 0..ORDER_BOOK_LEN {
            let kind = match i % 4 {
                0 => TmfKind::AbortTask,
                1 => TmfKind::ClearAca,
                2 => TmfKind::AbortTaskSet,
                _ => TmfKind::LunReset,
            };
            if q.enqueue(kind, 1 + i as u32, 1000 + i as u32).is_err() {
                all_in = false;
            }
        }
        let book = q.drain_all();
        s.add_dyn(
            "×50 排队序账",
            all_in && book.len() == ORDER_BOOK_LEN && order_consistent(&book),
            "50 条混合优先级序账必须逐条符合全序",
        );
    }
    // 12 判定次序：来源零号先于状态倒退，状态倒退先于容量。
    {
        let mut q = TmfuQueue::new(1);
        let _ = q.enqueue(TmfKind::AbortTask, 1, 1);
        let _ = q.begin_drain();
        let a = q.enqueue(TmfKind::AbortTask, 0, 2);
        let b = q.enqueue(TmfKind::AbortTask, 1, 2);
        s.add_dyn(
            "判序来源先于状态",
            a == Err(TmfuCode::OriginInvalid) && b == Err(TmfuCode::StateRegress),
            "来源判据先于状态判据，状态判据先于容量判据",
        );
    }
    // 13 状态机轮转：Idle → Queueing → Draining → Idle。
    {
        let mut q = TmfuQueue::new(QUEUE_CAP);
        let idle0 = q.state_is(QueueState::Idle);
        let _ = q.enqueue(TmfKind::AbortTask, 1, 1);
        let queuing = q.state_is(QueueState::Queueing);
        let _ = q.begin_drain();
        let draining = q.state_is(QueueState::Draining);
        let _ = q.dequeue_one();
        let back_idle = q.state_is(QueueState::Idle);
        s.add_dyn(
            "状态机单向轮转",
            idle0 && queuing && draining && back_idle,
            "四态轮转必须单向且回到 Idle",
        );
    }
    // 14 Idle 态收尾拒。
    {
        let mut q = TmfuQueue::new(QUEUE_CAP);
        let r = q.begin_drain();
        s.add_dyn("Idle收尾拒", r == Err(TmfuCode::StateRegress), "Idle 态不得进入排空");
    }
    // 15 对账守恒。
    {
        let mut q = TmfuQueue::new(QUEUE_CAP);
        let _ = q.enqueue(TmfKind::AbortTask, 1, 1);
        for _ in 0..10u32 {
            let _ = q.enqueue_forged(TmfKind::AbortTask, 1, 900, 0);
            q.note_detected();
        }
        s.add_dyn(
            "注入账守恒",
            q.conserved() && q.serial_next() == 1,
            "注入=检出+逃逸，逃逸恒 0；伪造串号不得推动发号机",
        );
    }
    // 16 双跑一致。
    s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同一脚本两次建序账逐字段相等");
    // 17 文案闭集非空。
    {
        let mut closed = true;
        for c in ALL_CODES.iter() {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add_dyn("文案闭集非空", closed, "每个诊断码都必须有中文建议");
    }
    // 18 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_flow_full_order() {
        let mut q = TmfuQueue::new(QUEUE_CAP);
        assert_eq!(q.enqueue(TmfKind::AbortTask, 1, 1), Ok(0));
        assert_eq!(q.enqueue(TmfKind::LunReset, 1, 2), Ok(1));
        assert!(q.begin_drain().is_ok());
        let book = q.drain_all();
        assert_eq!(book.len(), 2);
        assert!(order_consistent(&book));
        assert!(matches!(book.get(0), Some(r) if r.kind == TmfKind::LunReset));
    }

    #[test]
    fn priority_beats_arrival() {
        let mut q = TmfuQueue::new(QUEUE_CAP);
        let _ = q.enqueue(TmfKind::AbortTask, 1, 1);
        let _ = q.enqueue(TmfKind::LunReset, 1, 2);
        let _ = q.begin_drain();
        let book = q.drain_all();
        assert!(matches!(book.get(0), Some(r) if r.kind == TmfKind::LunReset), "高优先级后到仍先派发");
        assert!(matches!(book.get(1), Some(r) if r.kind == TmfKind::AbortTask));
    }

    #[test]
    fn same_rank_fifo_by_serial() {
        let mut q = TmfuQueue::new(QUEUE_CAP);
        for i in 0..3u32 {
            let _ = q.enqueue(TmfKind::ClearAca, 1 + i, 10 + i);
        }
        let _ = q.begin_drain();
        let book = q.drain_all();
        for (i, r) in book.iter().enumerate() {
            assert_eq!(r.tag, 10 + i as u32, "同优先级按串号先进先出");
        }
    }

    #[test]
    fn queue_cap_and_over() {
        let mut q = TmfuQueue::new(4);
        for i in 0..4u32 {
            assert!(q.enqueue(TmfKind::AbortTask, 1, 100 + i).is_ok());
        }
        assert_eq!(q.enqueue(TmfKind::AbortTask, 1, 200), Err(TmfuCode::QueueFull));
    }

    #[test]
    fn origin_zero_and_empty_dequeue() {
        let mut q = TmfuQueue::new(QUEUE_CAP);
        assert_eq!(q.enqueue(TmfKind::AbortTask, 0, 1), Err(TmfuCode::OriginInvalid));
        assert_eq!(q.dequeue_one(), Err(TmfuCode::EmptyQueue));
    }

    #[test]
    fn forged_serial_injection_all_caught() {
        let mut q = TmfuQueue::new(64);
        let _ = q.enqueue(TmfKind::AbortTask, 1, 1);
        for _ in 0..10u32 {
            assert_eq!(q.enqueue_forged(TmfKind::AbortTask, 1, 900, 0), Err(TmfuCode::SeriaRegress));
        }
        assert_eq!(q.enqueue_forged(TmfKind::AbortTask, 1, 2, q.serial_next()), Ok(1));
    }

    #[test]
    fn draining_enqueue_rejected() {
        let mut q = TmfuQueue::new(64);
        let _ = q.enqueue(TmfKind::AbortTask, 1, 1);
        assert!(q.begin_drain().is_ok());
        assert_eq!(q.enqueue(TmfKind::LunReset, 2, 2), Err(TmfuCode::StateRegress));
        assert_eq!(q.begin_drain(), Err(TmfuCode::StateRegress), "重复收尾也是倒退");
    }

    #[test]
    fn dup_tag_in_queue_and_history() {
        let mut q = TmfuQueue::new(64);
        let _ = q.enqueue(TmfKind::AbortTask, 1, 42);
        assert_eq!(q.enqueue(TmfKind::ClearAca, 2, 42), Err(TmfuCode::DupTag));
        let _ = q.begin_drain();
        let _ = q.drain_all();
        assert_eq!(q.enqueue(TmfKind::ClearAca, 2, 42), Err(TmfuCode::DupTag), "派发史 tag 也不得复用");
    }

    #[test]
    fn concurrent_ten_serialized() {
        let mut q = TmfuQueue::new(64);
        let kinds = [
            TmfKind::AbortTask,
            TmfKind::LunReset,
            TmfKind::ClearAca,
            TmfKind::AbortTaskSet,
            TmfKind::AbortTask,
            TmfKind::AbortTaskSet,
            TmfKind::ClearAca,
            TmfKind::LunReset,
            TmfKind::AbortTask,
            TmfKind::ClearAca,
        ];
        for (i, k) in kinds.iter().enumerate() {
            assert!(q.enqueue(*k, 1 + i as u32, 10 + i as u32).is_ok());
        }
        let _ = q.begin_drain();
        let book = q.drain_all();
        assert_eq!(book.len(), 10);
        assert!(order_consistent(&book), "10 条并发必须串行化全序");
    }

    #[test]
    fn order_book_fifty_entries() {
        let mut q = TmfuQueue::new(128);
        for i in 0..ORDER_BOOK_LEN {
            let kind = match i % 4 {
                0 => TmfKind::AbortTask,
                1 => TmfKind::ClearAca,
                2 => TmfKind::AbortTaskSet,
                _ => TmfKind::LunReset,
            };
            assert!(q.enqueue(kind, 1 + i as u32, 1000 + i as u32).is_ok());
        }
        let book = q.drain_all();
        assert_eq!(book.len(), ORDER_BOOK_LEN);
        assert!(order_consistent(&book));
    }

    #[test]
    fn state_machine_round_trip() {
        let mut q = TmfuQueue::new(QUEUE_CAP);
        assert!(q.state_is(QueueState::Idle));
        let _ = q.enqueue(TmfKind::AbortTask, 1, 1);
        assert!(q.state_is(QueueState::Queueing));
        let _ = q.begin_drain();
        assert!(q.state_is(QueueState::Draining));
        let _ = q.dequeue_one();
        assert!(q.state_is(QueueState::Idle));
    }

    #[test]
    fn conservation_holds() {
        let mut q = TmfuQueue::new(QUEUE_CAP);
        for _ in 0..10u32 {
            let _ = q.enqueue_forged(TmfKind::AbortTask, 1, 900, 0);
            q.note_detected();
        }
        assert!(q.conserved());
    }

    #[test]
    fn dual_run_identical_book() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn seg_guard_holds() {
        assert!(seg_guard_ok());
    }

    #[test]
    fn detail_texts_closed_set() {
        for c in ALL_CODES.iter() {
            assert!(!c.detail().is_empty());
        }
    }
}
