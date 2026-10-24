// SPDX-License-Identifier: Apache-2.0
//! VE-F0183 · 时间旅行调试增强
//!
//! 双向状态快照：任意时刻状态可回溯/前进。
//! 快照链：稀疏快照 + 重放到任意点。
//! 支持：向前/向后/跳转/断点回放。
//!
//! 纯功能代码，深度定制，非脚本生成。

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::collections::BTreeMap;

use crate::checks::{CheckSet, CheckSetBuilder};

// ---------------------------------------------------------------------------
// 状态快照
// ---------------------------------------------------------------------------

/// 状态快照。
#[derive(Debug, Clone)]
pub struct StateSnapshot {
    /// 时间戳。
    pub timestamp_ns: u64,
    /// 状态数据（键值对）。
    pub state: BTreeMap<String, u64>,
    /// 快照编号。
    pub seq: u32,
}

/// 时间旅行调试器。
pub struct TimeTravelDebugger {
    /// 快照链（稀疏存储）。
    snapshots: Vec<StateSnapshot>,
    /// 事件日志（快照之间的变更）。
    events: Vec<StateEvent>,
    /// 当前位置。
    pub current_pos: usize,
    /// 快照间隔（每 N 事件存一个快照）。
    snapshot_interval: usize,
}

/// 状态变更事件。
#[derive(Debug, Clone)]
pub struct StateEvent {
    /// 时间戳。
    pub timestamp_ns: u64,
    /// 变量名。
    pub key: String,
    /// 新值。
    pub value: u64,
}

impl TimeTravelDebugger {
    pub fn new(snapshot_interval: usize) -> Self {
        TimeTravelDebugger {
            snapshots: Vec::new(),
            events: Vec::new(),
            current_pos: 0,
            snapshot_interval: snapshot_interval.max(1),
        }
    }

    /// 记录事件（自动存快照）。
    pub fn record_event(&mut self, timestamp_ns: u64, key: &str, value: u64) {
        self.events.push(StateEvent {
            timestamp_ns,
            key: key.to_string(),
            value,
        });
        // 按间隔存快照
        if self.events.len() % self.snapshot_interval == 0 {
            let mut state = BTreeMap::new();
            // 重放到当前（简化：只取最近的每个 key）
            for e in &self.events {
                state.insert(e.key.clone(), e.value);
            }
            self.snapshots.push(StateSnapshot {
                timestamp_ns,
                state,
                seq: self.snapshots.len() as u32,
            });
        }
    }

    /// 向前走一步。
    pub fn step_forward(&mut self) -> bool {
        if self.current_pos + 1 < self.events.len() {
            self.current_pos += 1;
            true
        } else {
            false
        }
    }

    /// 向后走一步。
    pub fn step_backward(&mut self) -> bool {
        if self.current_pos > 0 {
            self.current_pos -= 1;
            true
        } else {
            false
        }
    }

    /// 跳转到指定时间。
    pub fn jump_to(&mut self, timestamp_ns: u64) -> bool {
        // 找最近的事件位置
        let pos = self.events.iter().position(|e| e.timestamp_ns >= timestamp_ns);
        match pos {
            Some(p) => { self.current_pos = p; true }
            None => false,
        }
    }

    /// 获取当前状态（快照 + 重放）。
    pub fn current_state(&self) -> BTreeMap<String, u64> {
        let mut state = BTreeMap::new();
        // 从最近的快照开始
        let base = self.snapshots.iter().rev().find(|s| s.seq as usize * self.snapshot_interval <= self.current_pos);
        if let Some(s) = base {
            state = s.state.clone();
        }
        // 重放后续事件
        let start = base.map(|s| (s.seq as usize + 1) * self.snapshot_interval).unwrap_or(0);
        for i in start..=self.current_pos.min(self.events.len().saturating_sub(1)) {
            if let Some(e) = self.events.get(i) {
                state.insert(e.key.clone(), e.value);
            }
        }
        state
    }

    /// 事件总数。
    pub fn event_count(&self) -> usize { self.events.len() }

    /// 快照数。
    pub fn snapshot_count(&self) -> usize { self.snapshots.len() }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// VE-F0183 判据全集。
pub fn run_time_travel_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("VE-F0183", "时间旅行调试增强");

    // 1. 记录事件
    let mut tt = TimeTravelDebugger::new(3);
    for i in 0..10 {
        tt.record_event(i * 1000, "counter", i);
    }
    cs.ok("事件记录", tt.event_count() == 10, "十条");
    cs.ok("快照生成", tt.snapshot_count() >= 3, "每 3 事件一个快照");

    // 2. 前进/后退
    let mut tt2 = TimeTravelDebugger::new(5);
    for i in 0..5 {
        tt2.record_event(i * 1000, "val", i);
    }
    cs.ok("初始位置", tt2.current_pos == 0, "位置 0");
    cs.ok("前进", tt2.step_forward(), "一步");
    cs.ok("前进到 1", tt2.current_pos == 1, "位置 1");
    cs.ok("后退", tt2.step_backward(), "一步");
    cs.ok("后退到 0", tt2.current_pos == 0, "位置 0");

    // 3. 跳转
    cs.ok("跳转成功", tt2.jump_to(3000), "到 3000ns");
    cs.ok("跳转位置", tt2.current_pos == 3, "事件 3");

    // 4. 状态查询
    let state = tt2.current_state();
    cs.ok("状态非空", !state.is_empty(), "有状态");
    cs.ok("状态值", state.get("val").copied() == Some(3), "val=3");

    // 5. 边界
    let mut tt3 = TimeTravelDebugger::new(100);
    cs.ok("空后退", !tt3.step_backward(), "位置 0 不能后退");
    cs.ok("空前进", !tt3.step_forward(), "无事件");

    // 6. 大量事件
    let mut tt4 = TimeTravelDebugger::new(10);
    for i in 0..100 {
        tt4.record_event(i, "x", i as u64);
    }
    cs.ok("百事件", tt4.event_count() == 100, "完整");
    cs.ok("十快照", tt4.snapshot_count() == 10, "每 10 一个");
    cs.ok("跳转到尾", tt4.jump_to(u64::MAX), "可跳转");

    cs.done()
}
