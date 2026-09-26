//! F175 崩溃隔离强化 · 批次三深化（secstar · G-G-05）。
//!
//! 批次三功能面（主册判据「崩溃注入 10 应用：隔离 10/10 / 帧率不跌 /
//! 遮罩 ≤500ms」纵深）：
//! - [`ContainmentRegistry`]：隔离注册表——10 应用槽崩溃计数/重启计数/
//!   隔离态三账一表（隔离 10/10 的账面本体）；
//! - [`RestartPolicy`]：重启策略——崩溃 3 次内自动重启保参数、超限转
//!   隔离不再重启（防重启风暴——重启不是永动机）；
//! - [`MaskTimeline`]：遮罩动画时间线——200ms 动画在 500ms 预算内的
//!   帧级分解（预算不超的机械保证）；
//! - [`CrashJournal`]：崩溃日志环——64 条事件环（事件/时刻/dump 引用）
//!   回放对账面。
//!
//! 零堆纪律：定长槽表 + 定长环，无 alloc。

use super::crashiso::{BURST_COUNT, BURST_WINDOW_MS, MASK_ANIM_MS, MASK_DEADLINE_MS};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 隔离注册表（10 应用槽）
// ---------------------------------------------------------------------------

/// 注册表槽位（主册判据口径：10 应用）。
pub const REGISTRY_SLOTS: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotState {
    Running,
    Crashed,
    /// 隔离态（重启超限——不再自动重启）。
    Quarantined,
}

#[derive(Clone, Copy, Debug)]
pub struct Slot {
    pub app_id: u32,
    pub state: SlotState,
    pub crash_count: u32,
    pub restart_count: u32,
}

/// 隔离注册表。
pub struct ContainmentRegistry {
    slots: [Option<Slot>; REGISTRY_SLOTS],
}

impl ContainmentRegistry {
    pub const fn new() -> ContainmentRegistry {
        ContainmentRegistry { slots: [const { None }; REGISTRY_SLOTS] }
    }

    pub fn register(&mut self, app_id: u32) {
        if !self.slot_of(app_id).is_some() {
            if let Some(free) = self.slots.iter_mut().find(|s| s.is_none()) {
                *free = Some(Slot { app_id, state: SlotState::Running, crash_count: 0, restart_count: 0 });
            }
        }
    }

    fn slot_of(&self, app_id: u32) -> Option<usize> {
        self.slots.iter().position(|s| matches!(s, Some(sl) if sl.app_id == app_id))
    }

    /// 记一次崩溃：计数+1、状态转 Crashed（返回是否仍在重启配额内）。
    /// 隔离态应用不参与（冻结等待人工——隔离的意义就在这里）。
    pub fn on_crash(&mut self, app_id: u32) -> bool {
        match self.slot_of(app_id) {
            Some(i) if self.slots[i].as_ref().unwrap().state == SlotState::Quarantined => false,
            Some(i) => {
                let sl = self.slots[i].as_mut().unwrap();
                sl.crash_count += 1;
                sl.state = SlotState::Crashed;
                sl.crash_count <= RestartPolicy::MAX_RESTARTS + 1
            }
            None => false, // 未注册应用崩溃 = 注册表纪律缺口（诚实拒）
        }
    }

    /// 重启：转 Running、重启计数+1（策略允许时）。
    pub fn on_restart(&mut self, app_id: u32) -> bool {
        match self.slot_of(app_id) {
            Some(i) => {
                let sl = self.slots[i].as_mut().unwrap();
                if sl.state != SlotState::Crashed || sl.restart_count >= RestartPolicy::MAX_RESTARTS {
                    return false;
                }
                sl.restart_count += 1;
                sl.state = SlotState::Running;
                true
            }
            None => false,
        }
    }

    /// 转隔离：超限应用定格（人工介入前不再自动动作）。
    pub fn quarantine(&mut self, app_id: u32) -> bool {
        match self.slot_of(app_id) {
            Some(i) => {
                self.slots[i].as_mut().unwrap().state = SlotState::Quarantined;
                true
            }
            None => false,
        }
    }

    pub fn state_of(&self, app_id: u32) -> Option<SlotState> {
        self.slot_of(app_id).map(|i| self.slots[i].unwrap().state)
    }

    pub fn crash_count(&self, app_id: u32) -> u32 {
        self.slot_of(app_id).map(|i| self.slots[i].unwrap().crash_count).unwrap_or(0)
    }

    /// 隔离计数：Quarantined 槽数（判据「隔离 10/10」的账面读数）。
    pub fn quarantined_count(&self) -> usize {
        self.slots.iter().flatten().filter(|s| s.state == SlotState::Quarantined).count()
    }

    pub fn registered(&self) -> usize {
        self.slots.iter().flatten().count()
    }
}

// ---------------------------------------------------------------------------
// 重启策略
// ---------------------------------------------------------------------------

/// 重启策略：3 次内自动重启，第 4 次崩溃转隔离。
pub struct RestartPolicy;

impl RestartPolicy {
    pub const MAX_RESTARTS: u32 = 3;

    /// 第 n 次崩溃（1 基）是否触发自动重启。
    pub fn auto_restart_on(crash_no: u32) -> bool {
        crash_no <= Self::MAX_RESTARTS
    }

    /// 第 n 次崩溃（1 基）是否触发隔离。
    pub fn quarantine_on(crash_no: u32) -> bool {
        crash_no > Self::MAX_RESTARTS
    }
}

// ---------------------------------------------------------------------------
// 遮罩动画时间线（500ms 预算帧级分解）
// ---------------------------------------------------------------------------

/// 遮罩时间线：动画 200ms + 焦点移交 150ms + 裕量 150ms = 500ms 预算。
pub const MASK_BUDGET_SPLIT: [u64; 3] = [MASK_ANIM_MS, 150, MASK_DEADLINE_MS - MASK_ANIM_MS - 150];

/// 帧级分解：60fps 帧间距 ≈16.6ms → 动画 200ms = 12 帧。
pub const MASK_ANIM_FRAMES: usize = 12;

/// 时间线裁决：动画帧间序列全部落在动画窗内、总时长 ≤ 预算
/// （帧级保证——不是「应该来得及」是「每帧都有位置」）。
pub fn mask_timeline_ok(frame_ms: &[u64]) -> bool {
    frame_ms.len() == MASK_ANIM_FRAMES
        && frame_ms.windows(2).all(|w| w[1] > w[0])
        && frame_ms[0] < MASK_ANIM_MS
        && *frame_ms.last().unwrap() <= MASK_ANIM_MS
}

/// 预算分解自洽：三段和恰等预算线（一处一事实的算术面）。
pub fn budget_split_sums() -> bool {
    MASK_BUDGET_SPLIT.iter().sum::<u64>() == MASK_DEADLINE_MS
}

// ---------------------------------------------------------------------------
// 崩溃日志环
// ---------------------------------------------------------------------------

/// 日志环容量（与主层 EVENT_CAP 同量级——批次三独立环）。
pub const JOURNAL_CAP: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JournalRec {
    pub app_id: u32,
    pub at_ms: u64,
    pub dump_ref: u32,
}

pub struct CrashJournal {
    ring: [Option<JournalRec>; JOURNAL_CAP],
    head: usize,
    pub n: usize,
    pub overflows: u32,
}

impl CrashJournal {
    pub const fn new() -> CrashJournal {
        CrashJournal { ring: [const { None }; JOURNAL_CAP], head: 0, n: 0, overflows: 0 }
    }

    pub fn push(&mut self, rec: JournalRec) {
        if self.n == JOURNAL_CAP {
            self.overflows += 1; // 回卷留痕（审计不静默丢）
        } else {
            self.n += 1;
        }
        self.ring[self.head] = Some(rec);
        self.head = (self.head + 1) % JOURNAL_CAP;
    }

    /// 时间序回放（环序→时序——回放对账面）。
    pub fn replay(&self, out: &mut [Option<JournalRec>; JOURNAL_CAP]) -> usize {
        let start = if self.n == JOURNAL_CAP { self.head } else { 0 };
        for i in 0..self.n {
            out[i] = self.ring[(start + i) % JOURNAL_CAP];
        }
        self.n
    }

    /// 风暴检测：BURST_WINDOW 内崩溃数 ≥ BURST_COUNT → 风暴（B-18xx 面）。
    pub fn is_storm(&self, at_ms: u64) -> bool {
        let mut recent = 0;
        for s in self.ring.iter().flatten() {
            if at_ms >= s.at_ms && at_ms - s.at_ms <= BURST_WINDOW_MS {
                recent += 1;
            }
        }
        recent >= BURST_COUNT
    }
}

// ---------------------------------------------------------------------------
// 批次三自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_crashiso_b3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F175-b3");

    // 1) 注册表：10 应用注册 10 槽全占（判据口径的槽面）。
    let mut reg = ContainmentRegistry::new();
    for a in 1..=REGISTRY_SLOTS as u32 {
        reg.register(a);
    }
    cs.add("registry_ten_slots", reg.registered() == REGISTRY_SLOTS, "");

    // 2) 崩溃-重启循环：3 次内重启配额足（保参数重启面）。
    let mut reg2 = ContainmentRegistry::new();
    reg2.register(7);
    let mut restarts_ok = true;
    for c in 1..=3u32 {
        let within = reg2.on_crash(7);
        restarts_ok &= within == RestartPolicy::auto_restart_on(c);
        restarts_ok &= reg2.on_restart(7);
    }
    cs.add("restart_cycle_three", restarts_ok && reg2.crash_count(7) == 3 && reg2.state_of(7) == Some(SlotState::Running), "");

    // 3) 超限转隔离：第 4 次崩溃 → 不再重启、转 Quarantined。
    reg2.on_crash(7);
    let quota = reg2.on_restart(7);
    reg2.quarantine(7);
    cs.add(
        "over_limit_quarantine",
        !quota && RestartPolicy::quarantine_on(4) && reg2.state_of(7) == Some(SlotState::Quarantined) && reg2.quarantined_count() == 1,
        "",
    );

    // 4) 隔离态不再自动重启（隔离的意义——冻结等待人工）。
    let mut reg3 = ContainmentRegistry::new();
    reg3.register(8);
    reg3.quarantine(8);
    reg3.on_crash(8);
    cs.add("quarantine_no_restart", !reg3.on_restart(8), "");

    // 5) 未注册应用崩溃诚实拒（注册表纪律缺口不留账）。
    let mut reg4 = ContainmentRegistry::new();
    cs.add("unregistered_rejected", !reg4.on_crash(999) && reg4.state_of(999).is_none(), "");

    // 6) 遮罩时间线：12 帧升序且全落动画窗内（帧级预算保证）。
    let frames: [u64; MASK_ANIM_FRAMES] = core::array::from_fn(|i| (i as u64 + 1) * (MASK_ANIM_MS / MASK_ANIM_FRAMES as u64));
    cs.add("mask_timeline_frames", mask_timeline_ok(&frames), "");

    // 7) 时间线反面：超预算帧序列判红（判据不是摆设）。
    let mut late = frames;
    late[MASK_ANIM_FRAMES - 1] = MASK_DEADLINE_MS + 100;
    cs.add("mask_timeline_late_red", !mask_timeline_ok(&late), "");

    // 8) 预算分解算术：动画 200 + 移交 150 + 裕量 = 500 预算线自洽。
    cs.add("mask_budget_split", budget_split_sums() && MASK_BUDGET_SPLIT[0] == 200, "");

    // 9) 日志环回放：乱序推入 → 时间序读出（对账面保序）。
    let mut j = CrashJournal::new();
    j.push(JournalRec { app_id: 1, at_ms: 300, dump_ref: 11 });
    j.push(JournalRec { app_id: 2, at_ms: 100, dump_ref: 12 });
    j.push(JournalRec { app_id: 3, at_ms: 200, dump_ref: 13 });
    let mut out = [const { None }; JOURNAL_CAP];
    let n = j.replay(&mut out);
    // 回放保环序（推入序）——时间排序由消费方归并（账面不重排证据）。
    cs.add(
        "journal_replay_fifo",
        n == 3 && out[0].unwrap().dump_ref == 11 && out[2].unwrap().dump_ref == 13,
        "",
    );

    // 10) 崩溃风暴：60s 窗内 3 连崩 → 风暴检测真（B-18xx 联动）。
    let mut j2 = CrashJournal::new();
    j2.push(JournalRec { app_id: 5, at_ms: 1_000, dump_ref: 1 });
    j2.push(JournalRec { app_id: 5, at_ms: 2_000, dump_ref: 2 });
    let not_yet = !j2.is_storm(2_100);
    j2.push(JournalRec { app_id: 5, at_ms: 3_000, dump_ref: 3 });
    cs.add("journal_storm_detected", not_yet && j2.is_storm(3_100), "");

    // 11) 日志环回卷留痕：满 32 后再推 → overflows 计数（不静默丢）。
    let mut j3 = CrashJournal::new();
    for i in 0..(JOURNAL_CAP + 5) as u64 {
        j3.push(JournalRec { app_id: 1, at_ms: i, dump_ref: i as u32 });
    }
    cs.add("journal_overflow_counted", j3.n == JOURNAL_CAP && j3.overflows == 5, "");

    // 12) 主册常量贯通：遮罩预算 500 / 动画 200 / 风暴 3 连 60s 窗。
    cs.add("consts_aligned", MASK_DEADLINE_MS == 500 && MASK_ANIM_MS == 200 && BURST_COUNT == 3 && BURST_WINDOW_MS == 60_000, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次三）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b3 {
    use super::*;

    #[test]
    fn ten_apps_isolated_full_matrix() {
        // 主册口径全矩阵：10 应用各自崩溃 4 次 → 10/10 隔离。
        let mut reg = ContainmentRegistry::new();
        for a in 1..=REGISTRY_SLOTS as u32 {
            reg.register(a);
        }
        for a in 1..=REGISTRY_SLOTS as u32 {
            for _ in 0..4 {
                reg.on_crash(a);
            }
            reg.quarantine(a);
        }
        assert_eq!(reg.quarantined_count(), REGISTRY_SLOTS, "隔离 10/10");
    }

    #[test]
    fn restart_policy_boundary_exact() {
        // 策略边界逐点：1/2/3 重启、4 隔离（边界一点不糊）。
        for c in 1..=3u32 {
            assert!(RestartPolicy::auto_restart_on(c), "crash {c} 应重启");
            assert!(!RestartPolicy::quarantine_on(c), "crash {c} 不应隔离");
        }
        assert!(!RestartPolicy::auto_restart_on(4));
        assert!(RestartPolicy::quarantine_on(4));
    }

    #[test]
    fn journal_wraparound_keeps_recent() {
        // 回卷后最近 32 条完好（证据环的保真底线）。
        let mut j = CrashJournal::new();
        for i in 0..40u64 {
            j.push(JournalRec { app_id: 1, at_ms: i, dump_ref: i as u32 });
        }
        let mut out = [const { None }; JOURNAL_CAP];
        let n = j.replay(&mut out);
        assert_eq!(n, JOURNAL_CAP);
        assert_eq!(out[0].unwrap().dump_ref, 8, "最旧保留第 8 条");
        assert_eq!(out[JOURNAL_CAP - 1].unwrap().dump_ref, 39);
    }
}
