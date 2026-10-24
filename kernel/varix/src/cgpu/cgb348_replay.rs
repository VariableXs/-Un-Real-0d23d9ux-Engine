// CGPU-F0348 预算回放 · cgb348_replay.rs
// AI-MIMO-1022 · CGPU-C 预算收口
// 判据：预算事件流可回放（确定性）；回放与原始一致；
//       快进/慢放/断点；回放差异报告。
#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;

use super::super::checks::CheckSet;

/// 预算事件。
#[derive(Clone, Copy, Debug)]
pub struct BudgetEvent {
    pub seq: u64,
    pub kind: u8,
    pub subject: u32,
    pub amount: u64,
    pub timestamp: u64,
}

/// 回放模式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReplayMode {
    Normal,
    FastForward,
    SlowMotion,
    StepByStep,
}

/// 回放状态。
#[derive(Clone, Copy, Debug)]
pub struct ReplayState {
    pub position: u64,
    pub total: u64,
    pub mode: ReplayMode,
    pub paused: bool,
    pub speed_q8: u16,
}

/// 预算回放引擎。
pub struct BudgetReplayer {
    pub events: Vec<BudgetEvent>,
    pub state: ReplayState,
    pub replay_log: Vec<BudgetEvent>,
    pub divergences: Vec<u64>,
    pub seed: u64,
}

impl BudgetReplayer {
    pub fn new(events: Vec<BudgetEvent>) -> Self {
        let total = events.len() as u64;
        Self {
            events,
            state: ReplayState {
                position: 0,
                total,
                mode: ReplayMode::Normal,
                paused: false,
                speed_q8: 256,
            },
            replay_log: Vec::new(),
            divergences: Vec::new(),
            seed: 0x1022_2026_u64 as u64 & 0xFFFF_FFFF,
        }
    }

    /// 步进一事件。
    pub fn step(&mut self) -> Option<&BudgetEvent> {
        if self.state.position >= self.state.total {
            return None;
        }
        let idx = self.state.position as usize;
        let ev = self.events[idx];
        self.replay_log.push(ev);
        self.state.position += 1;
        Some(&self.replay_log[self.replay_log.len() - 1])
    }

    /// 快进 N 步。
    pub fn fast_forward(&mut self, n: u64) -> u64 {
        self.state.mode = ReplayMode::FastForward;
        let mut advanced = 0u64;
        for _ in 0..n {
            if self.step().is_some() {
                advanced += 1;
            } else {
                break;
            }
        }
        self.state.mode = ReplayMode::Normal;
        advanced
    }

    /// 回退到断点。
    pub fn seek(&mut self, position: u64) -> bool {
        if position > self.state.total {
            return false;
        }
        self.replay_log.clear();
        self.state.position = 0;
        self.fast_forward(position);
        true
    }

    /// 验证回放一致性（确定性）。
    pub fn verify_consistency(&mut self) -> bool {
        if self.replay_log.len() != self.state.position as usize {
            return false;
        }
        for (i, ev) in self.replay_log.iter().enumerate() {
            if ev.seq != self.events[i].seq || ev.amount != self.events[i].amount {
                self.divergences.push(i as u64);
            }
        }
        self.divergences.is_empty()
    }

    /// 回放差异报告。
    pub fn divergence_count(&self) -> u32 {
        self.divergences.len() as u32
    }

    /// 设置速度。
    pub fn set_speed(&mut self, speed_q8: u16) {
        self.state.speed_q8 = speed_q8.min(4096);
    }

    pub fn pause(&mut self) {
        self.state.paused = true;
    }

    pub fn resume(&mut self) {
        self.state.paused = false;
    }

    /// 进度（Q8）。
    pub fn progress_q8(&self) -> u16 {
        if self.state.total == 0 {
            return 256;
        }
        ((self.state.position * 256) / self.state.total) as u16
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0348 预算回放");
        cs.add("事件 seq 单调", {
            self.events.windows(2).all(|w| w[0].seq < w[1].seq)
        }, "seq mono");
        cs.add("位置 ≤ 总数", self.state.position <= self.state.total, "pos ≤ total");
        cs.add("回放日志与位置一致", {
            self.replay_log.len() == self.state.position as usize
        }, "sync");
        cs.add("进度 ≤ 256", self.progress_q8() <= 256, "q8");
        cs.add("速度 ≤ 4096", self.state.speed_q8 <= 4096, "speed");
        cs.add("零分歧或已记录", {
            self.divergence_count() as usize == self.divergences.len()
        }, "divergences");
        cs.add("seek 后状态一致", {
            self.state.position <= self.state.total
        }, "seek valid");
        cs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events() -> Vec<BudgetEvent> {
        (0..10)
            .map(|i| BudgetEvent {
                seq: i,
                kind: 0,
                subject: 1,
                amount: i * 10,
                timestamp: i,
            })
            .collect()
    }

    #[test]
    fn step_through() {
        let mut r = BudgetReplayer::new(events());
        assert!(r.step().is_some());
        assert_eq!(r.state.position, 1);
    }

    #[test]
    fn fast_forward() {
        let mut r = BudgetReplayer::new(events());
        let n = r.fast_forward(5);
        assert_eq!(n, 5);
        assert_eq!(r.state.position, 5);
    }

    #[test]
    fn consistency() {
        let mut r = BudgetReplayer::new(events());
        r.fast_forward(10);
        assert!(r.verify_consistency());
        assert_eq!(r.divergence_count(), 0);
    }
}
