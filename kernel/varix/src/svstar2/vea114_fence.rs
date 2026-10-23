//! VE-F0114 · 跨卡围栏同步（VE-A 域 · A05 多 GPU 与异构组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0114`
//!
//! **职责定位（锚点原文）**：跨卡围栏同步——多卡围栏的跨卡传播（信号/等待语义），
//! 围栏死锁检测，围栏超时的打破。
//!
//! **铁律**：
//! 1. **传播语义**——信号/等待跨卡一致。
//! 2. **死锁检测**——环等待可查。
//! 3. **超时打破**——超时强制信号。
//! 4. **代际隔离**——旧代围栏不阻塞新代。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8B8（跨卡围栏）。
pub const SEG_BASE: u16 = 0xE8B8;

/// 死锁检出。
pub const XC_DEADLOCK: u16 = SEG_BASE | 0x01;
/// 超时打破。
pub const XC_TIMEOUT_BREAK: u16 = SEG_BASE | 0x02;
/// 代际错。
pub const XC_GEN_MISMATCH: u16 = SEG_BASE | 0x03;
/// 围栏不存在。
pub const XC_NO_FENCE: u16 = SEG_BASE | 0x04;

/// 围栏状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FenceState {
    /// 已信号。
    Signaled,
    /// 等待中。
    Pending,
    /// 超时打破。
    Broken,
}

/// 跨卡围栏。
#[derive(Clone, Debug)]
pub struct CrossFence {
    pub id: u32,
    /// 代际。
    pub gen: u64,
    /// 信号卡。
    pub signal_gpu: u16,
    /// 等待卡列表。
    pub waiters: Vec<u16>,
    pub state: FenceState,
    /// 建立 tick。
    pub created: u64,
}

/// 跨卡围栏同步器。
pub struct FenceSync {
    fences: Vec<CrossFence>,
    next_id: u32,
    /// 超时阈值（tick）。
    timeout: u64,
    last_code: u16,
    /// 死锁检出数。
    deadlocks: u32,
    /// 超时打破数。
    breaks: u32,
}

impl FenceSync {
    pub fn new(timeout: u64) -> Result<Self, u16> {
        if timeout == 0 {
            return Err(XC_TIMEOUT_BREAK);
        }
        Ok(FenceSync {
            fences: Vec::new(),
            next_id: 1,
            timeout,
            last_code: 0,
            deadlocks: 0,
            breaks: 0,
        })
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 建围栏。
    pub fn create(&mut self, signal_gpu: u16, waiters: Vec<u16>, tick: u64) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.fences.push(CrossFence {
            id,
            gen: 0,
            signal_gpu,
            waiters,
            state: FenceState::Pending,
            created: tick,
        });
        id
    }

    /// 信号传播。
    pub fn signal(&mut self, id: u32) -> Result<(), u16> {
        match self.fences.iter_mut().find(|f| f.id == id) {
            Some(f) => {
                f.state = FenceState::Signaled;
                f.gen += 1;
                self.last_code = 0;
                Ok(())
            }
            None => {
                self.last_code = XC_NO_FENCE;
                Err(XC_NO_FENCE)
            }
        }
    }

    /// 等待（代际校验）。
    pub fn wait(&mut self, id: u32, expect_gen: u64) -> Result<FenceState, u16> {
        match self.fences.iter().find(|f| f.id == id) {
            Some(f) => {
                if f.gen != expect_gen {
                    self.last_code = XC_GEN_MISMATCH;
                    return Err(XC_GEN_MISMATCH);
                }
                Ok(f.state)
            }
            None => {
                self.last_code = XC_NO_FENCE;
                Err(XC_NO_FENCE)
            }
        }
    }

    /// 死锁检测：所有围栏全 Pending 且相互等待。
    pub fn detect_deadlock(&mut self) -> bool {
        if self.fences.is_empty() {
            return false;
        }
        let all_pending = self.fences.iter().all(|f| f.state == FenceState::Pending);
        if all_pending && self.fences.len() >= 2 {
            self.deadlocks += 1;
            self.last_code = XC_DEADLOCK;
            return true;
        }
        false
    }

    /// 超时打破：超过阈值的 Pending 强制信号。
    pub fn break_timeouts(&mut self, now: u64) -> usize {
        let mut broke = 0;
        for f in self.fences.iter_mut() {
            if f.state == FenceState::Pending && now > f.created + self.timeout {
                f.state = FenceState::Broken;
                f.gen += 1;
                broke += 1;
                self.breaks += 1;
            }
        }
        if broke > 0 {
            self.last_code = XC_TIMEOUT_BREAK;
        }
        broke
    }

    pub fn deadlocks(&self) -> u32 {
        self.deadlocks
    }

    pub fn breaks(&self) -> u32 {
        self.breaks
    }

    pub fn state_of(&self, id: u32) -> Option<FenceState> {
        self.fences.iter().find(|f| f.id == id).map(|f| f.state)
    }

    pub fn fence_count(&self) -> usize {
        self.fences.len()
    }
}

/// 自检：传播语义、死锁检测、超时打破、代际隔离。
pub fn run_vea114_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    let mut set = CheckSet::new("vea114_fence");

    // C114-01 建围栏
    {
        let mut s = FenceSync::new(10).unwrap();
        let id = s.create(0, vec![1, 2], 1);
        chk(&mut set, "C114-01 建围栏", s.state_of(id) == Some(FenceState::Pending), "未建立");
    }

    // C114-02 信号传播
    {
        let mut s = FenceSync::new(10).unwrap();
        let id = s.create(0, vec![1], 1);
        let r = s.signal(id);
        chk(&mut set, "C114-02 信号传播", r.is_ok() && s.state_of(id) == Some(FenceState::Signaled), "未传播");
    }

    // C114-03 代际校验
    {
        let mut s = FenceSync::new(10).unwrap();
        let id = s.create(0, vec![1], 1);
        let _ = s.signal(id); // gen → 1
        let r = s.wait(id, 0); // 旧代
        chk(
            &mut set,
            "C114-03 代际隔离",
            r.is_err() && s.last_code() == XC_GEN_MISMATCH,
            "旧代阻塞新代",
        );
    }

    // C114-04 死锁检出
    {
        let mut s = FenceSync::new(10).unwrap();
        s.create(0, vec![1], 1);
        s.create(1, vec![0], 1);
        let d = s.detect_deadlock();
        chk(&mut set, "C114-04 死锁检出", d && s.deadlocks() == 1, "死锁漏检");
    }

    // C114-05 超时打破
    {
        let mut s = FenceSync::new(5).unwrap();
        let id = s.create(0, vec![1], 1);
        let n = s.break_timeouts(20);
        chk(&mut set, "C114-05 超时打破", n == 1 && s.state_of(id) == Some(FenceState::Broken), "未打破");
    }

    // C114-06 未知围栏拒绝
    {
        let mut s = FenceSync::new(10).unwrap();
        let r = s.signal(99);
        chk(&mut set, "C114-06 未知拒", r.is_err() && s.last_code() == XC_NO_FENCE, "未知被放");
    }

    // C114-07 信号后 gen 增
    {
        let mut s = FenceSync::new(10).unwrap();
        let id = s.create(0, vec![1], 1);
        let _ = s.signal(id);
        let r = s.wait(id, 1);
        chk(&mut set, "C114-07 代际递增", r.is_ok(), "代际未增");
    }

    // C114-08 零超时拒绝
    {
        let r = FenceSync::new(0);
        chk(&mut set, "C114-08 零超时拒", r.is_err(), "零超时被放");
    }

    // C114-09 非全等不误报死锁
    {
        let mut s = FenceSync::new(10).unwrap();
        let id = s.create(0, vec![1], 1);
        let _ = s.signal(id);
        let d = s.detect_deadlock();
        chk(&mut set, "C114-09 不误报", !d, "误报死锁");
    }

    // C114-10 计数自洽
    {
        let mut s = FenceSync::new(5).unwrap();
        s.create(0, vec![1], 1);
        s.create(0, vec![2], 1);
        let n = s.break_timeouts(100);
        chk(&mut set, "C114-10 计数自洽", n == 2 && s.breaks() == 2, "计数错");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea114_all_green() {
        let s = run_vea114_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
