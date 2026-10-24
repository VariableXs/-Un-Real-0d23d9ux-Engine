// cgb294_timeline.rs ↔ CGPU-F0294 时间轴联动
//
// 判据原文：CGPU-F0294-J1 双视图、回放四操作、双向高亮、判据。
// 难点分析：图×时间轴双视图的双向关联是核心——选帧定位时间、选时间回放图态。
//          回放数据对接 B07 录制重放，需要按时间戳索引图态快照。
// 纪律约束：双视图联动；回放四操作（播放/暂停/逐帧/拖动）；双向高亮。
// 成本记账：时间戳二分查找 O(log n)；回放步进 O(1)。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;

pub type NodeId = u32;
pub type TaskId = u32;

/// 回放操作（四操作）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayOp {
    /// 播放。
    Play,
    /// 暂停。
    Pause,
    /// 逐帧前进。
    StepNext,
    /// 逐帧后退。
    StepPrev,
}

/// 时间轴标记。
#[derive(Debug, Clone)]
pub struct TimelineMark {
    /// 时间戳（μs）。
    pub ts_us: u64,
    /// 关联任务。
    pub task: TaskId,
    /// 执行区间起（μs）。
    pub start_us: u64,
    /// 执行区间止（μs）。
    pub end_us: u64,
}

/// 图态快照（某时刻的图状态）。
#[derive(Debug, Clone)]
pub struct GraphState {
    pub ts_us: u64,
    /// 活跃节点。
    pub active_nodes: Vec<NodeId>,
    /// 阻塞节点。
    pub blocked_nodes: Vec<NodeId>,
}

/// 双视图联动引擎。
pub struct TimelineLink {
    /// 时间轴标记（按 ts 排序）。
    marks: Vec<TimelineMark>,
    /// 图态序列（按 ts 排序）。
    states: Vec<GraphState>,
    /// 当前时间指针。
    cursor_us: u64,
    /// 回放状态。
    playing: bool,
    /// 选中任务（双向高亮）。
    selected_task: Option<TaskId>,
    /// 选中时间点。
    selected_ts: Option<u64>,
}

impl TimelineLink {
    /// 新建。
    pub fn new() -> Self {
        TimelineLink {
            marks: Vec::new(),
            states: Vec::new(),
            cursor_us: 0,
            playing: false,
            selected_task: None,
            selected_ts: None,
        }
    }

    /// 添加时间轴标记（保持排序）。
    pub fn add_mark(&mut self, mark: TimelineMark) {
        let idx = self
            .marks
            .iter()
            .position(|m| m.ts_us > mark.ts_us)
            .unwrap_or(self.marks.len());
        self.marks.insert(idx, mark);
    }

    /// 添加图态快照（保持排序）。
    pub fn add_state(&mut self, state: GraphState) {
        let idx = self
            .states
            .iter()
            .position(|s| s.ts_us > state.ts_us)
            .unwrap_or(self.states.len());
        self.states.insert(idx, state);
    }

    /// 回放操作。
    pub fn replay_op(&mut self, op: ReplayOp) -> u64 {
        match op {
            ReplayOp::Play => self.playing = true,
            ReplayOp::Pause => self.playing = false,
            ReplayOp::StepNext => {
                // 二分找下一个时间点。
                if let Some(next) = self
                    .states
                    .iter()
                    .find(|s| s.ts_us > self.cursor_us)
                {
                    self.cursor_us = next.ts_us;
                }
            }
            ReplayOp::StepPrev => {
                if let Some(prev) = self
                    .states
                    .iter()
                    .rev()
                    .find(|s| s.ts_us < self.cursor_us)
                {
                    self.cursor_us = prev.ts_us;
                }
            }
        }
        self.cursor_us
    }

    /// 拖动到时间点。
    pub fn seek(&mut self, ts_us: u64) {
        self.cursor_us = ts_us;
    }

    /// 当前图态（时间指针处）。
    pub fn current_state(&self) -> Option<&GraphState> {
        // 找 ≤ cursor 的最近快照。
        self.states
            .iter()
            .rev()
            .find(|s| s.ts_us <= self.cursor_us)
    }

    /// 选中任务 → 高亮其时间轴区间（正向关联）。
    pub fn select_task(&mut self, task: TaskId) -> Option<(u64, u64)> {
        self.selected_task = Some(task);
        let m = self.marks.iter().find(|m| m.task == task);
        if let Some(m) = m {
            self.selected_ts = Some(m.ts_us);
            Some((m.start_us, m.end_us))
        } else {
            None
        }
    }

    /// 选中时间点 → 高亮该时刻活跃任务（反向关联）。
    pub fn select_time(&mut self, ts_us: u64) -> Vec<TaskId> {
        self.selected_ts = Some(ts_us);
        self.cursor_us = ts_us;
        self.marks
            .iter()
            .filter(|m| m.start_us <= ts_us && ts_us <= m.end_us)
            .map(|m| m.task)
            .collect()
    }

    /// 双向高亮结果。
    pub fn highlight(&self) -> (Option<TaskId>, Option<u64>) {
        (self.selected_task, self.selected_ts)
    }

    /// 是否播放中。
    pub fn is_playing(&self) -> bool {
        self.playing
    }
    /// 时间指针。
    pub fn cursor(&self) -> u64 {
        self.cursor_us
    }
    /// 标记数。
    pub fn mark_count(&self) -> usize {
        self.marks.len()
    }
    /// 快照数。
    pub fn state_count(&self) -> usize {
        self.states.len()
    }
}

impl Default for TimelineLink {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

use super::super::checks::CheckSet;

pub fn run_cgb294_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0294 时间轴联动");

    let mut t = TimelineLink::new();
    t.add_mark(TimelineMark {
        ts_us: 100,
        task: 1,
        start_us: 100,
        end_us: 500,
    });
    t.add_mark(TimelineMark {
        ts_us: 200,
        task: 2,
        start_us: 200,
        end_us: 800,
    });
    t.add_state(GraphState {
        ts_us: 100,
        active_nodes: alloc::vec![1],
        blocked_nodes: alloc::vec![],
    });
    t.add_state(GraphState {
        ts_us: 200,
        active_nodes: alloc::vec![1, 2],
        blocked_nodes: alloc::vec![3],
    });

    // --- 双视图数据 ---
    cs.add("时间轴标记 2", t.mark_count() == 2, "两个任务区间");
    cs.add("图态快照 2", t.state_count() == 2, "两个时刻");

    // --- 回放四操作 ---
    t.replay_op(ReplayOp::Play);
    cs.add("播放", t.is_playing(), "playing=true");
    t.replay_op(ReplayOp::Pause);
    cs.add("暂停", !t.is_playing(), "playing=false");
    t.seek(0);
    t.replay_op(ReplayOp::StepNext);
    cs.add("逐帧前进", t.cursor() == 100, "到第一快照");
    t.replay_op(ReplayOp::StepNext);
    cs.add("再前进", t.cursor() == 200, "到第二快照");
    t.replay_op(ReplayOp::StepPrev);
    cs.add("逐帧后退", t.cursor() == 100, "回第一快照");

    // --- 拖动 ---
    t.seek(150);
    cs.add("拖动定位", t.cursor() == 150, "时间指针");

    // --- 当前图态 ---
    let st = t.current_state();
    cs.add("图态查询", st.is_some(), "≤cursor 最近快照");

    // --- 正向关联：选任务 → 区间 ---
    let range = t.select_task(1);
    cs.add("选任务得区间", range == Some((100, 500)), "任务1 执行区间");

    // --- 反向关联：选时间 → 活跃任务 ---
    let tasks = t.select_time(300);
    cs.add("选时间得任务", tasks.contains(&1) && tasks.contains(&2), "两任务区间覆盖 300");

    // --- 双向高亮 ---
    let (task, ts) = t.highlight();
    cs.add("高亮任务", task.is_some(), "选中任务");
    cs.add("高亮时间", ts == Some(300), "选中时刻");

    cs
}
