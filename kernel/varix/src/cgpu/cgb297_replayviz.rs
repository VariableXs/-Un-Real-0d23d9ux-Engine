// CGPU-F0297 持久化重放可视化 · cgb297_replayviz.rs
// AI-MIMO-1022 · CGPU-B 可视化续章
// 判据：重放进度可视化（基线+增量链）；每步 diff 可高亮；损坏块定位展示；
//       重放时间轴可拖动；确定性步进（同输入同帧序）。
#![allow(dead_code)]

use alloc::format;
use alloc::vec;
extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;

use super::super::checks::CheckSet;

/// 重放阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReplayPhase {
    Idle,
    LoadingBaseline,
    ApplyingDeltas,
    Verifying,
    Complete,
    Failed,
    Partial,
}

/// 单个增量步。
#[derive(Clone, Debug)]
pub struct DeltaStep {
    pub index: u32,
    pub frame: u64,
    pub kind: DeltaKind,
    pub bytes: u32,
    pub crc_ok: bool,
    pub entities_touched: Vec<u32>,
    pub duration_us: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeltaKind {
    NodeAdd,
    NodeRemove,
    NodeUpdate,
    EdgeAdd,
    EdgeRemove,
    EdgeUpdate,
    AccountUpdate,
    EventAppend,
    SnapshotMerge,
}

/// 损坏块定位。
#[derive(Clone, Debug)]
pub struct CorruptBlock {
    pub block_index: u32,
    pub byte_offset: u64,
    pub byte_len: u32,
    pub expected_crc: u32,
    pub actual_crc: u32,
    pub severity: CorruptSeverity,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CorruptSeverity {
    Header,
    Payload,
    Truncated,
    Unknown,
}

/// 时间轴刻度。
#[derive(Clone, Copy, Debug)]
pub struct TimelineTick {
    pub frame: u64,
    pub x: u16,
    pub label_id: u32,
}

/// 重放帧视图（单帧的可视化状态）。
#[derive(Clone, Debug)]
pub struct ReplayFrameView {
    pub frame: u64,
    pub phase: ReplayPhase,
    pub deltas_applied: u32,
    pub deltas_total: u32,
    pub highlighted_entities: Vec<u32>,
    pub progress_q8: u16,
    pub corrupt_here: Option<u32>,
}

/// 重放可视化引擎。
pub struct ReplayViz {
    pub baseline_frame: u64,
    pub steps: Vec<DeltaStep>,
    pub corrupt_blocks: Vec<CorruptBlock>,
    pub cursor: usize,
    pub phase: ReplayPhase,
    pub ticks: Vec<TimelineTick>,
    pub frame_views: Vec<ReplayFrameView>,
    pub deterministic_seed: u64,
}

impl ReplayViz {
    pub fn new(baseline_frame: u64) -> Self {
        Self {
            baseline_frame,
            steps: Vec::new(),
            corrupt_blocks: Vec::new(),
            cursor: 0,
            phase: ReplayPhase::Idle,
            ticks: Vec::new(),
            frame_views: Vec::new(),
            deterministic_seed: 0x5EED_1022,
        }
    }

    pub fn load_steps(&mut self, steps: Vec<DeltaStep>) {
        self.steps = steps;
        self.steps.sort_by_key(|s| s.index);
        self.cursor = 0;
        self.phase = ReplayPhase::LoadingBaseline;
    }

    pub fn load_corrupt(&mut self, blocks: Vec<CorruptBlock>) {
        self.corrupt_blocks = blocks;
        self.corrupt_blocks.sort_by_key(|b| b.block_index);
    }

    /// 确定性步进：推进到下一增量步。
    pub fn step_forward(&mut self) -> Option<&ReplayFrameView> {
        if self.cursor >= self.steps.len() {
            self.phase = if self.corrupt_blocks.is_empty() {
                ReplayPhase::Complete
            } else {
                ReplayPhase::Partial
            };
            return None;
        }
        let step = &self.steps[self.cursor];
        self.cursor += 1;

        let corrupt_here = self
            .corrupt_blocks
            .iter()
            .find(|b| b.block_index == self.cursor as u32 - 1)
            .map(|b| b.block_index);

        let applied = self.cursor as u32;
        let total = self.steps.len() as u32;
        let progress = if total == 0 {
            256
        } else {
            ((applied as u64 * 256) / total as u64) as u16
        };

        let phase = if corrupt_here.is_some() {
            ReplayPhase::Failed
        } else if self.cursor >= self.steps.len() {
            if self.corrupt_blocks.is_empty() {
                ReplayPhase::Complete
            } else {
                ReplayPhase::Partial
            }
        } else {
            ReplayPhase::ApplyingDeltas
        };
        self.phase = phase;

        let view = ReplayFrameView {
            frame: step.frame,
            phase,
            deltas_applied: applied,
            deltas_total: total,
            highlighted_entities: step.entities_touched.clone(),
            progress_q8: progress,
            corrupt_here,
        };
        self.frame_views.push(view);
        self.frame_views.last()
    }

    /// 反向步进（时间旅行回看）。
    pub fn step_backward(&mut self) -> Option<&ReplayFrameView> {
        if self.cursor == 0 {
            return None;
        }
        self.cursor -= 1;
        self.frame_views.pop();
        self.frame_views.last()
    }

    /// 拖动到任意步（跳步预览）。
    pub fn seek(&mut self, index: usize) -> bool {
        if index > self.steps.len() {
            return false;
        }
        self.frame_views.clear();
        self.cursor = 0;
        while self.cursor < index {
            if self.step_forward().is_none() {
                break;
            }
        }
        true
    }

    /// 构建时间轴刻度（确定性：每 N 步一刻）。
    pub fn build_timeline(&mut self, max_ticks: usize) {
        self.ticks.clear();
        if self.steps.is_empty() || max_ticks == 0 {
            return;
        }
        let n = self.steps.len().min(max_ticks);
        let stride = self.steps.len() / n.max(1);
        for i in 0..n {
            let idx = i * stride.max(1);
            if idx >= self.steps.len() {
                break;
            }
            self.ticks.push(TimelineTick {
                frame: self.steps[idx].frame,
                x: ((i as u32 * 1000) / n as u32) as u16,
                label_id: i as u32,
            });
        }
    }

    /// 损坏定位摘要（人话）。
    pub fn corrupt_summary(&self) -> String {
        if self.corrupt_blocks.is_empty() {
            return String::from("无损坏块");
        }
        let mut s = String::from("损坏块 ");
        s.push_str(&alloc::format!("{} 处", self.corrupt_blocks.len()));
        for b in &self.corrupt_blocks {
            s.push_str(&alloc::format!(
                " [块{} off={} exp_crc={:08X} act_crc={:08X}]",
                b.block_index, b.byte_offset, b.expected_crc, b.actual_crc
            ));
        }
        s
    }

    /// 重放一致性校验：步序单调、CRC 状态与损坏表一致。
    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0297 持久化重放可视化");
        cs.add(
            "增量步 index 单调递增",
            self.steps.windows(2).all(|w| w[0].index < w[1].index),
            "sorted",
        );
        cs.add(
            "帧号单调不减",
            self.steps.windows(2).all(|w| w[0].frame <= w[1].frame),
            "frame mono",
        );
        cs.add(
            "损坏块 index 有序",
            self.corrupt_blocks.windows(2).all(|w| w[0].block_index < w[1].block_index),
            "sorted",
        );
        cs.add(
            "CRC 失败步与损坏表对齐",
            {
                let bad: Vec<u32> = self.steps.iter().filter(|s| !s.crc_ok).map(|s| s.index).collect();
                let corrupt_idx: Vec<u32> = self.corrupt_blocks.iter().map(|b| b.block_index).collect();
                bad.iter().all(|i| corrupt_idx.contains(i))
            },
            "crc_fail ⊆ corrupt",
        );
        cs.add(
            "游标在合法范围",
            self.cursor <= self.steps.len(),
            "cursor ≤ len",
        );
        cs.add(
            "阶段与进度自洽",
            match self.phase {
                ReplayPhase::Idle => self.cursor == 0,
                ReplayPhase::Complete => self.cursor == self.steps.len(),
                ReplayPhase::Failed => !self.corrupt_blocks.is_empty(),
                _ => true,
            },
            "phase consistency",
        );
        cs.add(
            "时间轴刻度 x 单调",
            self.ticks.windows(2).all(|w| w[0].x <= w[1].x),
            "tick x mono",
        );
        cs.add(
            "帧视图数与游标一致",
            self.frame_views.len() == self.cursor,
            "views == cursor",
        );
        cs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk() -> ReplayViz {
        let mut v = ReplayViz::new(0);
        let steps = vec![
            DeltaStep { index: 0, frame: 1, kind: DeltaKind::NodeAdd, bytes: 64, crc_ok: true, entities_touched: vec![1], duration_us: 10 },
            DeltaStep { index: 1, frame: 2, kind: DeltaKind::EdgeAdd, bytes: 32, crc_ok: true, entities_touched: vec![1, 2], duration_us: 8 },
            DeltaStep { index: 2, frame: 3, kind: DeltaKind::AccountUpdate, bytes: 16, crc_ok: false, entities_touched: vec![2], duration_us: 5 },
        ];
        v.load_steps(steps);
        v.load_corrupt(vec![CorruptBlock {
            block_index: 2,
            byte_offset: 112,
            byte_len: 16,
            expected_crc: 0xDEAD,
            actual_crc: 0xBEEF,
            severity: CorruptSeverity::Payload,
        }]);
        v
    }

    #[test]
    fn step_through() {
        let mut v = mk();
        assert!(v.step_forward().is_some());
        assert!(v.step_forward().is_some());
        let last = v.step_forward().unwrap();
        assert_eq!(last.phase, ReplayPhase::Failed);
        assert!(v.step_forward().is_none());
    }

    #[test]
    fn seek_and_back() {
        let mut v = mk();
        assert!(v.seek(2));
        assert_eq!(v.cursor, 2);
        assert!(v.step_backward().is_some());
        assert_eq!(v.cursor, 1);
    }

    #[test]
    fn timeline_monotonic() {
        let mut v = mk();
        v.build_timeline(2);
        assert!(v.ticks.windows(2).all(|w| w[0].x <= w[1].x));
    }
}
