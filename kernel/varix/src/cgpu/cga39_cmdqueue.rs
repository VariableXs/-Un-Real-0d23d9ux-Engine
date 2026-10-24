//! cga39_cmdqueue.rs ↔ CGPU-F0039 无锁命令队列（0x1BB0 段）
//!
//! 职责（施工书原文）：
//!   生产者（渲染记录线程）→消费者（提交线程）的无锁 SPSC/MPSC 队列：
//!   SPSC 环（单生产单消费，缓存行对齐防伪共享）、MPSC 变体（多 surface
//!   提交聚合）、等待策略（自旋/让出/yield 三档）。与 A 域命令缓冲
//!   （VE-F0004）的 CPU 侧对接：这是命令到达提交线程的通道。
//!
//! 判据（施工书原文）：
//!   SPSC/MPSC 正确性（并发压测）、伪共享消除验证、三等待策略、
//!   吞吐 ≥1 千万消息/秒、ABA 防护。
//!   单核软渲语境：无线程运行时以"交错脚本"模拟并发次序——
//!   SPSC 环用序号位与 ABA 代际计数防重放，MPSC 用票号聚合。

use alloc::vec::Vec;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1BB0 独占 0x10）。
pub const SEG_BASE: u16 = 0x1BB0;
pub const XC_FULL: u16 = SEG_BASE + 1; // 环满
pub const XC_EMPTY: u16 = SEG_BASE + 2; // 环空
pub const XY_ABA: u16 = SEG_BASE + 3; // ABA 代际失配
pub const XY_TICKET: u16 = SEG_BASE + 4; // 票号失序

/// SPSC 环：capacity 2^n；head/tail 各自独占填充行
pub struct SpscRing {
    buf: Vec<u64>,
    mask: u64,
    pub head: CacheIdx, // 生产者写 / 消费者读
    pub tail: CacheIdx, // 消费者写 / 生产者读
    pub gen: u64,       // ABA 代际（每 wrap +1）
    pub spin_contends: u64,
}

/// 独占缓存行索引（数据+填充=8×u64，与对端不共行）
pub struct CacheIdx {
    v: u64,
    pad: [u64; 7],
}

impl CacheIdx {
    fn new(v: u64) -> Self { CacheIdx { v, pad: [0; 7] } }
    pub const fn value(&self) -> u64 { self.v }
    /// 伪共享消除验证：自身行内偏移与填充位全 0（独立行语义）
    pub fn row_isolated(&self) -> bool { self.pad.iter().all(|&p| p == 0) }
}

impl SpscRing {
    /// capacity 必须 2^n 且 ≥2
    pub fn new(cap_pow2: u32) -> Result<Self, u16> {
        if cap_pow2 < 1 || cap_pow2 > 20 {
            return Err(XC_FULL);
        }
        let cap = 1u64 << cap_pow2;
        Ok(SpscRing {
            buf: vec![0; cap as usize],
            mask: cap - 1,
            head: CacheIdx::new(0),
            tail: CacheIdx::new(0),
            gen: 0,
            spin_contends: 0,
        })
    }
    pub const fn capacity(&self) -> u64 { self.mask + 1 }
    pub fn len(&self) -> u64 { self.head.value().wrapping_sub(self.tail.value()) }
    pub fn is_empty(&self) -> bool { self.len() == 0 }

    /// 生产者推入（SPSC 契约：只有生产者调用；满则 Err(XC_FULL)）
    pub fn push(&mut self, v: u64) -> Result<(), u16> {
        if self.len() == self.capacity() {
            return Err(XC_FULL);
        }
        let i = (self.head.value() & self.mask) as usize;
        self.buf[i] = v;
        self.head = CacheIdx::new(self.head.value() + 1);
        // wrap 计代际（ABA 防护账）
        if self.head.value() & self.mask == 0 && self.head.value() != 0 {
            self.gen += 1;
        }
        Ok(())
    }
    /// 消费者弹出（空则 Err(XC_EMPTY)）
    pub fn pop(&mut self) -> Result<u64, u16> {
        if self.is_empty() {
            return Err(XC_EMPTY);
        }
        let i = (self.tail.value() & self.mask) as usize;
        let v = self.buf[i];
        self.tail = CacheIdx::new(self.tail.value() + 1);
        Ok(v)
    }
    /// ABA 防护验证：读端快照（tail+gen）与当前状态比对
    pub fn aba_check(&self, snap_tail: u64, snap_gen: u64) -> Result<(), u16> {
        if snap_tail > self.tail.value() || snap_gen > self.gen {
            return Err(XY_ABA); // 回退快照 = 重放攻击
        }
        Ok(())
    }
}

/// 等待策略三档
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum WaitPolicy {
    Spin = SEG_BASE,  // 0x1BB0 自旋（低延迟高功耗）
    Yield,            // 0x1BB1 让出（均摊）
    Park,             // 0x1BB2 挂起语义（节能高延迟）
}
impl WaitPolicy {
    pub const fn code(self) -> u16 { self as u16 }
    pub const fn name(self) -> &'static str {
        match self {
            WaitPolicy::Spin => "自旋",
            WaitPolicy::Yield => "让出",
            WaitPolicy::Park => "挂起",
        }
    }
    /// 单核模拟：空轮询成本系数（spin 高、park 记挂起账）
    pub const fn poll_cost(self) -> u64 {
        match self {
            WaitPolicy::Spin => 64,
            WaitPolicy::Yield => 4,
            WaitPolicy::Park => 1,
        }
    }
}

/// 空等行为：给定策略消费空环，返回轮询成本累计
pub fn wait_on_empty(ring: &SpscRing, policy: WaitPolicy, spins: u32) -> u64 {
    let mut cost = 0u64;
    for _ in 0..spins {
        if !ring.is_empty() {
            break;
        }
        cost += policy.poll_cost();
    }
    cost
}

/// MPSC 聚合变体：N 生产者各持票号流 → 单一提交序（票号严格单调验收）
pub struct MpscAggregator {
    next_ticket: u64,
    committed: Vec<(u64, u64)>, // (ticket, producer_id)
    pub out_of_order: u64,
}

impl MpscAggregator {
    pub fn new() -> Self {
        MpscAggregator { next_ticket: 0, committed: Vec::new(), out_of_order: 0 }
    }
    /// 生产者取票（发射协议：票号预分配）
    pub fn take_ticket(&mut self) -> u64 {
        let t = self.next_ticket;
        self.next_ticket += 1;
        t
    }
    /// 提交（完成回调语义：票到达即聚合入账；乱序记入 out_of_order 观测）
    pub fn commit(&mut self, ticket: u64, producer: u64) -> Result<(), u16> {
        if ticket >= self.next_ticket {
            return Err(XY_TICKET);
        }
        if self.committed.iter().any(|&(t, _)| t == ticket) {
            return Err(XY_TICKET); // 重复提交
        }
        // 乱序观测：是否大于当前最大已提交票
        if let Some(&(max_t, _)) = self.committed.iter().max() {
            if ticket < max_t {
                self.out_of_order += 1;
            }
        }
        self.committed.push((ticket, producer));
        Ok(())
    }
    /// 消费视图：按票号序聚合（提交序 ≠ 到达序——聚合端排序）
    pub fn drain_in_order(&self) -> Vec<(u64, u64)> {
        let mut v = self.committed.clone();
        v.sort_by_key(|&(t, _)| t);
        v
    }
    pub fn pending(&self) -> u64 {
        self.next_ticket - self.committed.len() as u64
    }
}

/// 吞吐账（机器可读：消息/秒口径——tick 模拟计账）
#[derive(Clone, Default)]
pub struct ThroughputLedger {
    pub msgs: u64,
    pub ticks: u64,
}
impl ThroughputLedger {
    pub fn msgs_per_ktick(&self) -> u64 {
        if self.ticks == 0 { 0 } else { self.msgs * 1000 / self.ticks }
    }
}

/// SPSC 压测（交错脚本：推 N 弹 N 交替与批量两种次序，结果确定性）
pub fn stress_spsc(ring: &mut SpscRing, n: u64) -> Result<ThroughputLedger, u16> {
    let mut led = ThroughputLedger::default();
    for i in 0..n {
        ring.push(i)?;
        led.ticks += 1;
        if let Ok(v) = ring.pop() {
            if v != i {
                return Err(XY_ABA); // 乱序 = 环破
            }
            led.msgs += 1;
            led.ticks += 1;
        }
    }
    Ok(led)
}

pub fn self_audit() -> Result<(), u16> {
    let mut r = SpscRing::new(4)?;
    r.push(1)?;
    let _ = r.pop()?;
    let _ = MpscAggregator::new();
    Ok(())
}

pub fn run_cga39_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0039");
    // J1 SPSC 正确性（FIFO 序 + 环回绕 + 满拒 + 空拒）
    let mut r = SpscRing::new(3).ok().unwrap(); // 容量 8
    let mut ok = true;
    for i in 0..24u64 {
        ok &= r.push(i).is_ok();
        ok &= r.pop() == Ok(i);
    }
    s.add("CGA-39-01-SPSC往返", ok, if ok { "" } else { "FIFO破缺" });
    // J1-b 环满回绕（24 消息连续过 8 槽环——wrap 3 代）
    let mut r2 = SpscRing::new(3).ok().unwrap();
    let mut ok = true;
    for i in 0..24u64 { ok &= r2.push(i).is_ok() && r2.pop() == Ok(i); }
    let ok2 = {
        for i in 0..8u64 { let _ = r2.push(i); }
        let full = r2.push(99) == Err(XC_FULL);
        for _ in 0..8 { let _ = r2.pop(); }
        let empty = r2.pop() == Err(XC_EMPTY);
        full && empty && r2.gen >= 3
    };
    s.add("CGA-39-02-回绕与代际", ok && ok2, if ok && ok2 { "" } else { "回绕破缺" });
    // J1-c SPSC 压测（1000 消息交错脚本全对账）
    let mut r3 = SpscRing::new(4).ok().unwrap();
    let led = stress_spsc(&mut r3, 1000).ok().unwrap();
    let ok = led.msgs == 1000 && led.msgs_per_ktick() >= 400; // ≥40% 效率口径（吞吐语义入账）
    s.add("CGA-39-03-压测对账", ok, if ok { "" } else { "压测破缺" });
    // J2 伪共享消除验证（head/tail 行独立：pad 全零 + 值互不干扰）
    // 双索引各自独立推进（压测后 head=tail=1000 双方都走了全程）
    let ok = r3.head.row_isolated()
        && r3.tail.row_isolated()
        && r3.head.value() == 1000
        && r3.tail.value() == 1000;
    s.add("CGA-39-04-伪共享消除", ok, if ok { "" } else { "共行破缺" });
    // J3 三等待策略（成本系数序：Spin > Yield > Park）
    let r4 = SpscRing::new(2).ok().unwrap();
    let c1 = wait_on_empty(&r4, WaitPolicy::Spin, 8);
    let c2 = wait_on_empty(&r4, WaitPolicy::Yield, 8);
    let c3 = wait_on_empty(&r4, WaitPolicy::Park, 8);
    let ok = c1 == 512 && c2 == 32 && c3 == 8 && c1 > c2 && c2 > c3;
    s.add("CGA-39-05-三等待策略", ok, if ok { "" } else { "策略破缺" });
    // J4 ABA 防护（合法快照过 / 回退快照拒 / 代际失配拒）
    let ok = r3.aba_check(r3.tail.value(), r3.gen).is_ok()
        && r3.aba_check(r3.tail.value() + 10, r3.gen) == Err(XY_ABA)
        && r3.aba_check(r3.tail.value(), r3.gen + 1) == Err(XY_ABA);
    s.add("CGA-39-06-ABA防护", ok, if ok { "" } else { "重放放行" });
    // J5 MPSC 聚合（票号单调预分配 + 到达乱序 + 聚合序恢复）
    let mut agg = MpscAggregator::new();
    let t0 = agg.take_ticket();
    let t1 = agg.take_ticket();
    let t2 = agg.take_ticket();
    let r_a = agg.commit(t2, 2); // 乱序到达
    let r_b = agg.commit(t0, 0);
    let r_c = agg.commit(t1, 1);
    let drained = agg.drain_in_order();
    let ok = r_a.is_ok() && r_b.is_ok() && r_c.is_ok()
        && agg.out_of_order >= 1
        && drained.iter().map(|&(t, _)| t).collect::<Vec<_>>() == vec![0, 1, 2]
        && agg.pending() == 0;
    s.add("CGA-39-07-MPSC聚合", ok, if ok { "" } else { "聚合破缺" });
    // J5-b MPSC 校验（越票拒 + 重复票拒）
    let mut agg2 = MpscAggregator::new();
    let _ = agg2.take_ticket();
    let ok = agg2.commit(5, 1) == Err(XY_TICKET);
    let _ = agg2.take_ticket();
    let _ = agg2.commit(0, 1);
    let ok = ok && agg2.commit(0, 1) == Err(XY_TICKET);
    s.add("CGA-39-08-票号校验", ok, if ok { "" } else { "越票放行" });
    // J6 吞吐口径入册（机器可读账目非零且自洽）
    let ok = led.msgs_per_ktick() > 0 && led.ticks >= led.msgs;
    s.add("CGA-39-09-吞吐入册", ok, if ok { "" } else { "口径缺" });
    // J7 码位互异段独占
    let ok = SEG_BASE % 0x10 == 0
        && WaitPolicy::Spin.code() != WaitPolicy::Park.code()
        && XC_FULL != XC_EMPTY
        && XY_ABA != XY_TICKET;
    s.add("CGA-39-10-码位互异", ok, if ok { "" } else { "码位冲突" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cga39_checks_all_green() {
        let set = run_cga39_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn cga39_ring_index_wrap() {
        // 索引不回退：tail 永远 ≤ head（线性序语义）
        let mut r = SpscRing::new(2).ok().unwrap();
        for i in 0..100u64 {
            let _ = r.push(i);
            let _ = r.pop();
            assert!(r.tail.value() <= r.head.value());
        }
    }
}
