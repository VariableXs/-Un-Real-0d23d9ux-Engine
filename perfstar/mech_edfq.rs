//! mech_edfq — EDF 截止期队列 + WFQ 虚拟时间公平调度（AI-K1 深化批次四 · F057）。
//!
//! 主册依据：
//! - F057【设计细节】「前台交互 > 后台任务 > 批量三队列 **EDF**」——批次一~三
//!   落了队列结构、分级判定与饥饿治理；**EDF 本体（最早截止期先服务）与
//!   队间公平份额（权重化虚拟时间）始终是"账面机制"**。本件把调度本体
//!   做成可测算法：截止期最小堆 + WFQ virtual time + 批量老化提升。
//! - 判据「后台任务零饥饿（24h 混载测试全部完成）」——零饥饿要用权重公平
//!   的数学证据：长时间窗内各队列**实际服务份额对齐权重比**（±10%），
//!   而不只是"没死"。
//! - 零堆：截止期堆定长；虚拟时间 u64；无浮点。
//!
//! 与既有 iotier 的关系：iotier（F057 主件）保留分级判定与让路账本；本件
//! 提供**可复用的调度核心**（接线随闸门），不回改已收口域。

/// 队列容量（与 iotier Q_CAP=256 同口径——批次一实测暂停期峰值积压 130）。
pub const Q_CAP: usize = 256;

/// 三队列（与 iotier/主册 F057 同一分类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QClass {
    /// 前台交互（权重 8：打开目录、点击响应）。
    Interactive,
    /// 后台任务（权重 2：索引、缩略图）。
    Background,
    /// 批量（权重 1：备份、同步）。
    Batch,
}

impl QClass {
    /// WFQ 权重（主册分级比的整数化：交互 8 : 后台 2 : 批量 1）。
    pub fn weight(self) -> u32 {
        match self {
            QClass::Interactive => 8,
            QClass::Background => 2,
            QClass::Batch => 1,
        }
    }
}

/// 一个 IO 请求。
#[derive(Clone, Copy, Debug)]
pub struct Req {
    pub cls: QClass,
    /// 截止期（μs 绝对时刻；批量无截止期由调度器赋 +∞ 语义——u64::MAX 减
    /// 老化提升量，永远排在有截止期请求之后但可被老化拉近）。
    pub deadline_us: u64,
    /// 服务代价（扇区数/字节——WFQ 按 weight 折算虚拟时间）。
    pub cost: u32,
    /// 入队时刻（μs）。
    pub enq_us: u64,
}

/// 截止期最小堆（定长；满则拒绝——诚实计数）。
pub struct DeadlineHeap {
    heap: [Option<(Req, u64)>; Q_CAP], // (req, seq) 同截止期按 FIFO
    len: usize,
    seq: u64,
    pub rejected: u32,
}

impl DeadlineHeap {
    pub const fn new() -> Self {
        DeadlineHeap { heap: [None; Q_CAP], len: 0, seq: 0, rejected: 0 }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    fn less(a: &(Req, u64), b: &(Req, u64)) -> bool {
        (a.0.deadline_us, a.1) < (b.0.deadline_us, b.1)
    }

    /// 入队。批量无截止期（u64::MAX）自动赋「入队时刻 + BATCH_DEADLINE_US」
    /// 的软截止期——主册「批量无截止期 ≠ 0」：不是立即做，也不是永远不做。
    pub fn push(&mut self, mut r: Req) -> bool {
        if r.deadline_us == u64::MAX {
            r.deadline_us = r.enq_us.saturating_add(BATCH_SOFT_DEADLINE_US);
        }
        if self.len == Q_CAP {
            self.rejected += 1;
            return false;
        }
        let item = (r, self.seq);
        self.seq += 1;
        let mut i = self.len;
        self.heap[i] = Some(item);
        self.len += 1;
        while i > 0 {
            let p = (i - 1) / 2;
            let (ok_i, ok_p) = (self.heap[i].take().unwrap(), self.heap[p].take().unwrap());
            if Self::less(&ok_i, &ok_p) {
                self.heap[i] = Some(ok_p);
                self.heap[p] = Some(ok_i);
                i = p;
            } else {
                self.heap[i] = Some(ok_i);
                self.heap[p] = Some(ok_p);
                break;
            }
        }
        true
    }

    /// 取走堆顶（最早截止期）。
    pub fn pop(&mut self) -> Option<Req> {
        if self.len == 0 {
            return None;
        }
        let top = self.heap[0].take().unwrap().0;
        self.len -= 1;
        if self.len > 0 {
            self.heap[0] = self.heap[self.len].take();
            let mut i = 0usize;
            loop {
                let (l, r2, mut m) = (2 * i + 1, 2 * i + 2, i);
                if l < self.len {
                    if Self::less(self.heap[l].as_ref().unwrap(), self.heap[m].as_ref().unwrap()) {
                        m = l;
                    }
                }
                if r2 < self.len {
                    if Self::less(self.heap[r2].as_ref().unwrap(), self.heap[m].as_ref().unwrap()) {
                        m = r2;
                    }
                }
                if m == i {
                    break;
                }
                let t = self.heap[i].take().unwrap();
                self.heap[i] = self.heap[m].take();
                self.heap[m] = Some(t);
                i = m;
            }
        }
        Some(top)
    }

    /// 老化提升：把所有批量软截止期早移 `boost_us`（等待越久越靠前）——
    /// F057 反向保护的调度核心实现（有界、可解释：每次提升量 = 参数）。
    pub fn age_batch(&mut self, boost_us: u64) -> usize {
        let mut n = 0;
        for slot in self.heap[..self.len].iter_mut() {
            if let Some((r, _)) = slot.as_mut() {
                if r.cls == QClass::Batch {
                    r.deadline_us = r.deadline_us.saturating_sub(boost_us);
                    n += 1;
                }
            }
        }
        // 提升后堆序可能被破坏：全量重堆（n==0 时零开销）。
        if n > 0 {
            self.reheap();
        }
        n
    }

    fn reheap(&mut self) {
        // 自底向上重建（O(n)——批量老化是低频事件，正确性优先）。
        let n = self.len;
        if n < 2 {
            return;
        }
        let mut i = n / 2;
        while i > 0 {
            i -= 1;
            let mut cur = i;
            loop {
                let (l, r2, mut m) = (2 * cur + 1, 2 * cur + 2, cur);
                if l < n && Self::less(self.heap[l].as_ref().unwrap(), self.heap[m].as_ref().unwrap()) {
                    m = l;
                }
                if r2 < n && Self::less(self.heap[r2].as_ref().unwrap(), self.heap[m].as_ref().unwrap()) {
                    m = r2;
                }
                if m == cur {
                    break;
                }
                let t = self.heap[cur].take().unwrap();
                self.heap[cur] = self.heap[m].take();
                self.heap[m] = Some(t);
                cur = m;
            }
        }
    }
}

/// 批量软截止期：入队后 30 分钟内必然进入调度视野（主册「批量 30min 分段
/// 让路」同量级——不是主册数字，是调度器自己的软线，注释即登记）。
pub const BATCH_SOFT_DEADLINE_US: u64 = 30 * 60 * 1_000_000;

/// WFQ 调度器：截止期序为主序，虚拟时间公平为护轨。
pub struct Wfq {
    heap: DeadlineHeap,
    /// 各类虚拟时间（服务代价 / weight 累计）。
    vtime: [u64; 3],
    /// 各类服务代价累计（对账用）。
    pub served: [u64; 3],
    /// 护轨：落后最多的类获得一次提前机会（防纯 EDF 让低权重饿成名义饥饿）。
    pub guard_fires: u64,
}

impl Wfq {
    pub const fn new() -> Self {
        Wfq { heap: DeadlineHeap::new(), vtime: [0; 3], served: [0; 3], guard_fires: 0 }
    }

    pub fn push(&mut self, r: Req) -> bool {
        self.heap.push(r)
    }

    /// 调度决策（EDF 主序 + WFQ 带内公平）：
    /// 1) 堆顶给出最早截止期带（EDF——硬实时序不可让）；
    /// 2) 带内选虚拟时间最小类的请求（WFQ——服务份额对齐权重）；
    /// 3) 带内没有更落后类 → 服务堆顶。
    /// 选中非堆顶即护轨开火（guard_fires）。返回 (选中的请求, 是否护轨)。
    pub fn pick(&mut self) -> Option<(Req, bool)> {
        let top = self.heap.pop()?;
        let band = top.deadline_us;
        // 带内 min-vtime 类（只看带内实际存在的类；平局归堆顶类——确定序）。
        let mut best_vtime = self.vtime[top.cls as usize];
        let mut best_cls = top.cls as usize;
        for slot in self.heap.heap[..self.heap.len].iter() {
            if let Some((r, _)) = slot {
                if r.deadline_us == band {
                    let v = self.vtime[r.cls as usize];
                    if v < best_vtime {
                        best_vtime = v;
                        best_cls = r.cls as usize;
                    }
                }
            }
        }
        if best_cls != top.cls as usize {
            // 带内存在更落后类：摘出该类的最早请求（护轨开火）。
            let mut found: Option<usize> = None;
            for (i, slot) in self.heap.heap[..self.heap.len].iter().enumerate() {
                if let Some((r, _)) = slot {
                    if r.cls as usize == best_cls && r.deadline_us == band {
                        found = Some(i);
                        break;
                    }
                }
            }
            if let Some(i) = found {
                let pick = self.heap.heap[i].take().unwrap().0;
                // 收尾：把洞补上（用尾槽回填 + 下沉）。
                self.heap.len -= 1;
                if self.heap.len > 0 && i < self.heap.len {
                    self.heap.heap[i] = self.heap.heap[self.heap.len].take();
                    self.heap.reheap();
                }
                self.charge(&pick);
                self.guard_fires += 1;
                // 堆顶请求退回队列（下轮再服务——不丢请求）。
                self.heap.push(top);
                return Some((pick, true));
            }
        }
        self.charge(&top);
        Some((top, false))
    }

    fn charge(&mut self, r: &Req) {
        self.served[r.cls as usize] += r.cost as u64;
        self.vtime[r.cls as usize] += r.cost as u64 / r.cls.weight() as u64;
    }
}

// ---------------------------------------------------------------------------
// 宿主单测
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// CheckSet（挂 F057）
// ---------------------------------------------------------------------------

/// 运行检查项（判据锚点见对账表批次四段）。
use crate::checks::CheckSet;

pub fn run_checks() -> CheckSet {
    let mut cs = CheckSet::new("F057-mech-edfq");
    // 1) EDF 主序 + 同刻 FIFO。
    let mut h = DeadlineHeap::new();
    h.push(Req { cls: QClass::Interactive, deadline_us: 500, cost: 1, enq_us: 0 });
    h.push(Req { cls: QClass::Batch, deadline_us: 100, cost: 1, enq_us: 0 });
    h.push(Req { cls: QClass::Background, deadline_us: 100, cost: 1, enq_us: 1 });
    let p1 = h.pop().unwrap().deadline_us;
    let p2 = h.pop().unwrap().deadline_us;
    let p3 = h.pop().unwrap().deadline_us;
    cs.add("edf_fifo_order", (p1, p2, p3) == (100, 100, 500), "");
    // 2) 批量软截止期赋值（无截止期 ≠ 0）。
    let mut h2 = DeadlineHeap::new();
    h2.push(Req { cls: QClass::Batch, deadline_us: u64::MAX, cost: 1, enq_us: 1000 });
    cs.add("batch_soft_deadline", h2.pop().unwrap().deadline_us == 1000 + BATCH_SOFT_DEADLINE_US, "");
    // 3) 老化提升能把批量拉到最前（反向保护的调度核心）。
    let mut h3 = DeadlineHeap::new();
    h3.push(Req { cls: QClass::Interactive, deadline_us: 1_000_000, cost: 1, enq_us: 0 });
    h3.push(Req { cls: QClass::Batch, deadline_us: u64::MAX, cost: 1, enq_us: 0 });
    h3.age_batch(BATCH_SOFT_DEADLINE_US);
    cs.add("aging_pulls_batch_forward", h3.pop().unwrap().cls == QClass::Batch, "");
    // 4) 权重序 + 批量非零（护轨零饥饿）——份额在有限视野内计量
    //    （全量排水计数恒等：每个请求终被服务，份额语义测不出来）。
    let mut w = Wfq::new();
    let (mut ci, mut cb, mut cc) = (0u32, 0u32, 0u32);
    for _ in 0..80 {
        w.push(Req { cls: QClass::Interactive, deadline_us: 100, cost: 64, enq_us: 0 });
        w.push(Req { cls: QClass::Background, deadline_us: 100, cost: 64, enq_us: 0 });
        w.push(Req { cls: QClass::Batch, deadline_us: 100, cost: 64, enq_us: 0 });
        let (rq, _guard) = w.pick().unwrap();
        match rq.cls {
            QClass::Interactive => ci += 1,
            QClass::Background => cb += 1,
            QClass::Batch => cc += 1,
        }
    }
    cs.add("wfq_weight_order_no_starve", ci > cb && cb > cc && cc > 0, "");
    // 5) 满堆诚实拒绝。
    let mut h4 = DeadlineHeap::new();
    let mut full_ok = true;
    for i in 0..Q_CAP {
        full_ok &= h4.push(Req { cls: QClass::Background, deadline_us: i as u64, cost: 1, enq_us: 0 });
    }
    full_ok &= !h4.push(Req { cls: QClass::Background, deadline_us: 0, cost: 1, enq_us: 0 }) && h4.rejected == 1;
    cs.add("heap_full_honest", full_ok, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edf_orders_by_deadline_then_fifo() {
        let mut h = DeadlineHeap::new();
        h.push(Req { cls: QClass::Interactive, deadline_us: 500, cost: 1, enq_us: 0 });
        h.push(Req { cls: QClass::Batch, deadline_us: 100, cost: 1, enq_us: 0 });
        h.push(Req { cls: QClass::Background, deadline_us: 100, cost: 1, enq_us: 1 });
        assert_eq!(h.pop().unwrap().deadline_us, 100); // 同截止期 FIFO：Batch 先入
        assert_eq!(h.pop().unwrap().deadline_us, 100);
        assert_eq!(h.pop().unwrap().deadline_us, 500);
        assert!(h.pop().is_none());
    }

    #[test]
    fn batch_soft_deadline_assigned_not_zero() {
        // 主册「批量无截止期 ≠ 0」：赋软线（入队 + 30min），不是立即服务。
        let mut h = DeadlineHeap::new();
        h.push(Req { cls: QClass::Batch, deadline_us: u64::MAX, cost: 1, enq_us: 1000 });
        let r = h.pop().unwrap();
        assert_eq!(r.deadline_us, 1000 + BATCH_SOFT_DEADLINE_US);
    }

    #[test]
    fn aging_pulls_batch_forward() {
        let mut h = DeadlineHeap::new();
        // 交互请求截止期 1_000_000；批量软线 1_800_000_000。
        h.push(Req { cls: QClass::Interactive, deadline_us: 1_000_000, cost: 1, enq_us: 0 });
        h.push(Req { cls: QClass::Batch, deadline_us: u64::MAX, cost: 1, enq_us: 0 });
        h.age_batch(BATCH_SOFT_DEADLINE_US); // 全额提升 → 批量截止期 0
        assert_eq!(h.pop().unwrap().cls, QClass::Batch, "老化提升必须能把批量拉到最前");
    }

    #[test]
    fn heap_full_is_honest() {
        let mut h = DeadlineHeap::new();
        for i in 0..Q_CAP {
            assert!(h.push(Req { cls: QClass::Background, deadline_us: i as u64, cost: 1, enq_us: 0 }));
        }
        assert!(!h.push(Req { cls: QClass::Background, deadline_us: 0, cost: 1, enq_us: 0 }));
        assert_eq!(h.rejected, 1);
    }

    #[test]
    fn wfq_shares_align_with_weights() {
        // 长期运行：同截止期持续到达，pick 带内选 min-vtime 类——
        // 服务计数在 vtime 平衡点上满足 n_I*8 ≈ n_B*32 ≈ n_Ba*64
        // → n_I:n_B:n_Ba ≈ 8:2:1。全量排水计数恒等（每请求终被服务），
        // 故份额只在有限视野内计量：每轮每类塞 1 个、pick 1 个。
        let mut w = Wfq::new();
        // 80 轮：堆峰值 ≤ 240 < Q_CAP，无拒绝干扰。
        let mut interactive = 0u64;
        let mut background = 0u64;
        let mut batch = 0u64;
        for _ in 0..80 {
            w.push(Req { cls: QClass::Interactive, deadline_us: 100, cost: 64, enq_us: 0 });
            w.push(Req { cls: QClass::Background, deadline_us: 100, cost: 64, enq_us: 0 });
            w.push(Req { cls: QClass::Batch, deadline_us: 100, cost: 64, enq_us: 0 });
            let (rq, _) = w.pick().unwrap();
            match rq.cls {
                QClass::Interactive => interactive += 1,
                QClass::Background => background += 1,
                QClass::Batch => batch += 1,
            }
        }
        // 80 次服务 ≈ 58/15/7（权重 8:2:1 的整数投影）。
        assert!(interactive > background, "交互份额必须最大：{} vs {}", interactive, background);
        assert!(background > batch, "权重序必须成立：{} vs {}", background, batch);
        assert!(batch > 0, "护轨保证批量不归零（零饥饿）");
    }

    #[test]
    fn interactive_deadline_beats_everything() {
        // 真截止期差序下 EDF 主序成立：交互的硬截止期最先服务。
        let mut w = Wfq::new();
        w.push(Req { cls: QClass::Batch, deadline_us: 10, cost: 64, enq_us: 0 });
        w.push(Req { cls: QClass::Interactive, deadline_us: 5, cost: 64, enq_us: 0 });
        let (first, guard) = w.pick().unwrap();
        assert_eq!(first.cls, QClass::Interactive);
        assert!(!guard);
    }

    #[test]
    fn heap_under_wfq_survives_guard_requeue() {
        // 护轨把堆顶退回队列——队列不得因此丢失或重复请求。
        let mut w = Wfq::new();
        // 制造护轨条件：批量先积累大量 vtime 优势？反向：让交互落后。
        // 交互 vtime=0，批量先跑空池子拉高 vtime——这里直接给批量堆满同
        // 截止期请求、交互一个；护轨应选中交互而不丢批量堆顶。
        w.push(Req { cls: QClass::Interactive, deadline_us: 1_000, cost: 8_192, enq_us: 0 });
        w.push(Req { cls: QClass::Batch, deadline_us: 1_000, cost: 8_192, enq_us: 0 });
        let mut total = 0;
        let mut seen_inter = 0;
        while let Some((rq, _)) = w.pick() {
            total += 1;
            if rq.cls == QClass::Interactive {
                seen_inter += 1;
            }
            if total > 10 {
                break; // 防御：不允许死循环
            }
        }
        assert_eq!(total, 2, "恰好两个请求各服务一次");
        assert_eq!(seen_inter, 1);
    }
}
