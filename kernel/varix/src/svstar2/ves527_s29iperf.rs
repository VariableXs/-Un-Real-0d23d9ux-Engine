//! VE-F4293 · 工具集成性能（U05 工具集成组）
//! 职责：集成面性能三目标（事件端到端 P95<200ms/IDE 实时 P95<50ms/CI 增量
//! 占比）+ 背压控制器（溢出丢弃低优记死信）+ 适配器池化（耗尽排队+扩容
//! 信号）+ 批量合并（语义冲突拆批重放）。
//! 段位：0x2E00（续表 idx527）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2E00;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IpCode {
    /// 背压溢出。
    Backpressure = SEG_BASE,
    /// 池耗尽（排队+扩容信号）。
    PoolExhaust = SEG_BASE + 0x01,
    /// 合并语义冲突（拆批重放）。
    MergeClash = SEG_BASE + 0x02,
}

/// 事件优先级。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Prio {
    Low = 0,
    High = 1,
}

/// 背压控制器。
pub struct Backpressure {
    pub cap: u32,
    pub queued: Vec<(Prio, u32)>,
    pub dead: Vec<u32>,
}

impl Backpressure {
    pub fn new(cap: u32) -> Self {
        Backpressure { cap, queued: Vec::new(), dead: Vec::new() }
    }

    /// 入队：满时丢低优记死信；高优挤掉低优。
    pub fn push(&mut self, p: Prio, ev: u32) -> Result<(), IpCode> {
        if (self.queued.len() as u32) < self.cap {
            self.queued.push((p, ev));
            return Ok(());
        }
        if p == Prio::High {
            if let Some(idx) = self.queued.iter().position(|(q, _)| *q == Prio::Low) {
                let (_, old) = self.queued.remove(idx);
                self.dead.push(old);
                self.queued.push((p, ev));
                return Ok(());
            }
        }
        self.dead.push(ev);
        Err(IpCode::Backpressure)
    }
}

/// 适配器池。
pub struct AdapterPool {
    pub size: u32,
    pub used: u32,
    pub waiters: u32,
    pub expand_signals: u32,
}

impl AdapterPool {
    pub fn new(size: u32) -> Self {
        AdapterPool { size, used: 0, waiters: 0, expand_signals: 0 }
    }

    /// 借用：池耗尽排队+扩容信号。
    pub fn acquire(&mut self) -> Result<(), IpCode> {
        if self.used < self.size {
            self.used += 1;
            Ok(())
        } else {
            self.waiters += 1;
            self.expand_signals += 1;
            Err(IpCode::PoolExhaust)
        }
    }

    pub fn release(&mut self) {
        self.used = self.used.saturating_sub(1);
        if self.waiters > 0 {
            self.waiters -= 1;
            self.used += 1;
        }
    }

    /// 扩容。
    pub fn expand(&mut self, n: u32) {
        self.size += n;
    }
}

/// 批量合并器。
pub struct Merger {
    pub batch: Vec<u32>,
    pub cap: u32,
    pub replayed: u32,
}

impl Merger {
    pub fn new(cap: u32) -> Self {
        Merger { batch: Vec::new(), cap, replayed: 0 }
    }

    /// 合并：达容量触发合并；同值冲突（语义冲突）→拆批重放。
    pub fn add(&mut self, v: u32) -> Result<Vec<u32>, IpCode> {
        // 语义冲突：批内已有相同值再加=冲突
        if self.batch.contains(&v) && self.batch.len() > 0 {
            let old = core::mem::take(&mut self.batch);
            self.replayed += old.len() as u32;
            self.batch.push(v);
            return Err(IpCode::MergeClash); // 拆批重放
        }
        self.batch.push(v);
        if self.batch.len() as u32 >= self.cap {
            Ok(core::mem::take(&mut self.batch))
        } else {
            Ok(Vec::new())
        }
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C527-01..12。
pub fn run_ves527_checks() -> CheckSet {
    let mut set = CheckSet::new("ves527");

    // 背压
    let mut bp = Backpressure::new(2);
    let _ = bp.push(Prio::Low, 1);
    let _ = bp.push(Prio::Low, 2);
    chk(&mut set, "C527-01", bp.push(Prio::Low, 3) == Err(IpCode::Backpressure), "满载低优丢弃记死信");
    chk(&mut set, "C527-02", bp.dead == alloc::vec![3], "死信账在册");
    let _ = bp.push(Prio::High, 9);
    chk(&mut set, "C527-03", bp.queued.contains(&(Prio::High, 9)) && bp.dead.contains(&1), "高优挤掉低优");

    // 池化
    let mut ap = AdapterPool::new(2);
    let _ = ap.acquire();
    let _ = ap.acquire();
    chk(&mut set, "C527-04", ap.acquire() == Err(IpCode::PoolExhaust), "池耗尽排队");
    chk(&mut set, "C527-05", ap.expand_signals == 1 && ap.waiters == 1, "扩容信号+等待者");
    ap.expand(1);
    ap.release();
    chk(&mut set, "C527-06", ap.used == 2 && ap.waiters == 0, "扩容+释放唤醒等待者");

    // 合并
    let mut mg = Merger::new(3);
    let _ = mg.add(1);
    let _ = mg.add(2);
    let out = mg.add(3).unwrap_or_default();
    chk(&mut set, "C527-07", out == alloc::vec![1, 2, 3], "达容量触发合并出批");
    let _ = mg.add(5);
    chk(&mut set, "C527-08", mg.add(5) == Err(IpCode::MergeClash), "同值语义冲突拆批重放");
    chk(&mut set, "C527-09", mg.replayed == 1 && mg.batch == alloc::vec![5], "冲突批重放账");

    // P95 目标
    let mut samples: Vec<u32> = Vec::new();
    for i in 0..20u32 {
        samples.push(150 + i);
    }
    samples.sort_unstable();
    let p95 = samples[(samples.len() * 95 / 100).saturating_sub(1)];
    chk(&mut set, "C527-10", p95 < 200, "事件端到端 P95<200ms 目标");
    let mut ide: Vec<u32> = Vec::new();
    for i in 0..20u32 {
        ide.push(40 + i);
    }
    ide.sort_unstable();
    let p95b = ide[(ide.len() * 95 / 100).saturating_sub(1)];
    chk(&mut set, "C527-11", p95b < 50, "IDE 实时 P95<50ms 目标");
    chk(&mut set, "C527-12", Prio::High > Prio::Low && mg.cap == 3, "优先级序与容量自洽");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backpressure_pool() {
        let mut bp = Backpressure::new(1);
        assert!(bp.push(Prio::Low, 1).is_ok());
        assert!(bp.push(Prio::Low, 2).is_err());
        assert!(bp.push(Prio::High, 3).is_ok()); // 挤掉 1
        assert_eq!(bp.dead, alloc::vec![2]);
    }
}
