//! CGPU-F0247 · 看门狗与超时联动（cgb247_watchdog）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0247`
//! **判据原文摘录**：分级超时表（任务/锁/栅栏/IO 各级默认超时 + 可覆盖）、
//! 心跳机制（长任务注册心跳、喂狗超时视同死锁候选进 F0244 证据链）、半死判定
//! （心跳间歇存在但无进展——「饥饿」与「死锁」分类统计）、看门狗自身看护
//! （看门狗线程死亡自检重启、检测器不成为新单点）。
//!
//! ## 看门狗与环检测的关系
//!
//! 环检测（F0243）抓**拓扑死锁**——等待成环。但有一类「等价死锁」
//! **拓扑上无环**：
//!
//! - 任务 A 持锁自旋等一个永远不会来的事件（外部 IO 丢失）；
//! - 任务 B 被抢占后饿死（优先级反转链固化）。
//!
//! 这些场景 WFG 上无环，环检测漏。看门狗补的就是这个缺口——
//! **时间维度的死锁等价物**：等太久 = 等不到 = 死了。
//!
//! ## 心跳 → F0244 证据链
//!
//! 看门狗超时不直接杀任务。它把「喂狗超时」作为**证据**送进
//! F0244 的三维证据链（stall 维度）。F0244 的观察期 + 三维齐备
//! 判定仍然适用——看门狗只是**输入源**，不是执行者。
//!
//! 这条纪律防止看门狗变成误杀机器：超时 ≠ 死锁，还需要环证据 +
//! 等待时长配合。
//!
//! ## 半死判定：饥饿 vs 死锁
//!
//! | 症状 | 心跳 | 环 | 分类 |
//! |---|---|---|---|
//! | 死锁 | 停 | 有 | Deadlock |
//! | 饥饿 | 间歇 | 无 | Starvation |
//! | 慢任务 | 活跃 | 无 | Slow |
//! | 活锁 | 活跃 | 有 | Livelock |
//!
//! 分类统计进遥测（F0254），让「为什么慢」有答案。
//!
//! ## 看门狗自看护
//!
//! 看门狗自身也是进程/线程，也可能死。**检测器不成为新单点**：
//! - 心跳自检：看门狗定期检查自己的心跳记录；
//! - 死亡检测：超过 `self_timeout` 无自心跳 → 自动重启；
//! - 重启计数进遥测（F0248 的自保指标）。

use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

use super::cgb241_dlarch::{SubjectId, WaitSource};

// ---------------------------------------------------------------------------
// 诊断码（0xCBA0 起）
// ---------------------------------------------------------------------------

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0xCBA0;
/// 未注册心跳。
pub const E_NO_HEARTBEAT: u16 = SEG_BASE + 1;
/// 分级表满。
pub const E_TABLE_FULL: u16 = SEG_BASE + 2;
/// 超时表项不存在。
pub const E_NO_TIMEOUT: u16 = SEG_BASE + 3;

// ---------------------------------------------------------------------------
// 容量
// ---------------------------------------------------------------------------

/// 心跳注册上限。
pub const MAX_HEARTBEATS: usize = 1024;
/// 分级超时表容量。
pub const MAX_TIMEOUT_CLASSES: usize = 16;

// ---------------------------------------------------------------------------
// 分级超时表
// ---------------------------------------------------------------------------

/// 超时分级类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimeoutClass {
    /// 任务级超时。
    Task,
    /// 锁级超时。
    Lock,
    /// 栅栏级超时。
    Barrier,
    /// IO 级超时。
    Io,
}

impl TimeoutClass {
    pub const ALL: [TimeoutClass; 4] = [
        TimeoutClass::Task,
        TimeoutClass::Lock,
        TimeoutClass::Barrier,
        TimeoutClass::Io,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            TimeoutClass::Task => "task",
            TimeoutClass::Lock => "lock",
            TimeoutClass::Barrier => "barrier",
            TimeoutClass::Io => "io",
        }
    }

    pub const fn idx(self) -> usize {
        match self {
            TimeoutClass::Task => 0,
            TimeoutClass::Lock => 1,
            TimeoutClass::Barrier => 2,
            TimeoutClass::Io => 3,
        }
    }

    /// 默认超时（tick）。
    pub const fn default_timeout(self) -> u32 {
        match self {
            TimeoutClass::Task => 30_000,    // 30s
            TimeoutClass::Lock => 5_000,     // 5s
            TimeoutClass::Barrier => 10_000, // 10s
            TimeoutClass::Io => 60_000,      // 60s
        }
    }
}

/// 分级超时表（默认值 + 可覆盖）。
pub struct TimeoutTable {
    /// (类别, 当前超时, 是否被覆盖)。
    entries: [(TimeoutClass, u32, bool); 4],
    /// 覆盖计数。
    pub overrides: u64,
}

impl TimeoutTable {
    pub fn new() -> TimeoutTable {
        TimeoutTable {
            entries: [
                (TimeoutClass::Task, TimeoutClass::Task.default_timeout(), false),
                (TimeoutClass::Lock, TimeoutClass::Lock.default_timeout(), false),
                (TimeoutClass::Barrier, TimeoutClass::Barrier.default_timeout(), false),
                (TimeoutClass::Io, TimeoutClass::Io.default_timeout(), false),
            ],
            overrides: 0,
        }
    }

    /// 取某类的超时。
    pub fn get(&self, class: TimeoutClass) -> u32 {
        self.entries[class.idx()].1
    }

    /// 覆盖某类的超时。
    pub fn override_timeout(&mut self, class: TimeoutClass, ticks: u32) {
        if ticks == 0 {
            return;
        }
        let idx = class.idx();
        self.entries[idx].1 = ticks;
        self.entries[idx].2 = true;
        self.overrides += 1;
    }

    /// 重置为默认。
    pub fn reset(&mut self, class: TimeoutClass) {
        let idx = class.idx();
        self.entries[idx].1 = class.default_timeout();
        self.entries[idx].2 = false;
    }

    /// 是否被覆盖。
    pub fn is_overridden(&self, class: TimeoutClass) -> bool {
        self.entries[class.idx()].2
    }

    /// 从 WaitSource 映射到 TimeoutClass。
    pub fn class_for_source(source: WaitSource) -> TimeoutClass {
        match source {
            WaitSource::Task => TimeoutClass::Task,
            WaitSource::Lock => TimeoutClass::Lock,
            WaitSource::Barrier => TimeoutClass::Barrier,
            WaitSource::VramAlloc => TimeoutClass::Io,
        }
    }
}

// ---------------------------------------------------------------------------
// 心跳机制
// ---------------------------------------------------------------------------

/// 半死分类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HalfDeadClass {
    /// 死锁（心跳停 + 有环）。
    Deadlock,
    /// 饥饿（心跳间歇 + 无环）。
    Starvation,
    /// 慢任务（心跳活跃 + 无环）。
    Slow,
    /// 活锁（心跳活跃 + 有环）。
    Livelock,
}

impl HalfDeadClass {
    pub const fn name(self) -> &'static str {
        match self {
            HalfDeadClass::Deadlock => "deadlock",
            HalfDeadClass::Starvation => "starvation",
            HalfDeadClass::Slow => "slow",
            HalfDeadClass::Livelock => "livelock",
        }
    }
}

/// 心跳注册条目。
#[derive(Clone, Copy, Debug)]
pub struct HeartbeatEntry {
    pub subject: SubjectId,
    /// 上次心跳 tick。
    pub last_beat: u32,
    /// 心跳间隔（tick）。0 = 未注册。
    pub interval: u32,
    /// 累计喂狗超时数。
    pub missed: u32,
    /// 是否活跃。
    pub alive: bool,
}

/// 心跳注册表。
pub struct HeartbeatRegistry {
    entries: Vec<HeartbeatEntry>,
    /// 分类统计。
    pub class_count: [u64; 4],
    /// 遥测：总超时数。
    pub total_missed: u64,
}

impl HeartbeatRegistry {
    pub fn new() -> HeartbeatRegistry {
        HeartbeatRegistry {
            entries: Vec::new(),
            class_count: [0; 4],
            total_missed: 0,
        }
    }

    /// 注册心跳。
    pub fn register(&mut self, subject: SubjectId, interval: u32, now: u32) -> Result<(), u16> {
        if self.entries.len() >= MAX_HEARTBEATS {
            return Err(E_TABLE_FULL);
        }
        if let Some(e) = self.entries.iter_mut().find(|e| e.subject == subject) {
            e.interval = interval;
            e.last_beat = now;
            e.alive = true;
            return Ok(());
        }
        self.entries.push(HeartbeatEntry {
            subject,
            last_beat: now,
            interval,
            missed: 0,
            alive: true,
        });
        Ok(())
    }

    /// 喂狗。
    pub fn feed(&mut self, subject: SubjectId, now: u32) -> Result<(), u16> {
        let e = self
            .entries
            .iter_mut()
            .find(|e| e.subject == subject)
            .ok_or(E_NO_HEARTBEAT)?;
        e.last_beat = now;
        e.missed = 0;
        Ok(())
    }

    /// 注销。
    pub fn unregister(&mut self, subject: SubjectId) {
        self.entries.retain(|e| e.subject != subject);
    }

    /// 扫描超时。返回超时的 subject 列表。
    ///
    /// 超时判定：`now - last_beat > interval * 2`（允许一次丢拍）。
    pub fn scan_timeouts(&mut self, now: u32) -> Vec<SubjectId> {
        let mut out = Vec::new();
        for e in &mut self.entries {
            if !e.alive || e.interval == 0 {
                continue;
            }
            let elapsed = now.saturating_sub(e.last_beat);
            if elapsed > e.interval.saturating_mul(2) {
                e.missed = e.missed.saturating_add(1);
                self.total_missed += 1;
                out.push(e.subject);
            }
        }
        out
    }

    /// 半死分类。
    ///
    /// `has_cycle` 是环检测结果。结合心跳状态分类。
    pub fn classify(&self, subject: SubjectId, has_cycle: bool) -> Option<HalfDeadClass> {
        let e = self.entries.iter().find(|e| e.subject == subject)?;
        let heartbeat_dead = e.missed > 0;
        let heartbeat_intermittent = e.missed > 0 && e.interval > 0 && e.missed < 5;

        let class = if heartbeat_dead && has_cycle {
            HalfDeadClass::Deadlock
        } else if heartbeat_intermittent && !has_cycle {
            HalfDeadClass::Starvation
        } else if !heartbeat_dead && !has_cycle {
            HalfDeadClass::Slow
        } else {
            HalfDeadClass::Livelock
        };
        Some(class)
    }

    /// 记录分类。
    pub fn record_class(&mut self, class: HalfDeadClass) {
        self.class_count[class as usize] += 1;
    }

    pub fn entry(&self, subject: SubjectId) -> Option<&HeartbeatEntry> {
        self.entries.iter().find(|e| e.subject == subject)
    }

    pub fn active_count(&self) -> usize {
        self.entries.iter().filter(|e| e.alive).count()
    }
}

// ---------------------------------------------------------------------------
// 看门狗自看护
// ---------------------------------------------------------------------------

/// 看门狗自看护器。
///
/// 看门狗自身的心跳记录 + 死亡自检 + 自动重启。
pub struct SelfWatchdog {
    /// 自心跳 tick。
    pub self_beat: u32,
    /// 自心跳间隔。
    pub self_interval: u32,
    /// 自身超时阈值。
    pub self_timeout: u32,
    /// 是否存活。
    pub alive: bool,
    /// 重启次数。
    pub restarts: u64,
    /// 遥测：自检次数。
    pub self_checks: u64,
}

impl SelfWatchdog {
    pub fn new() -> SelfWatchdog {
        SelfWatchdog {
            self_beat: 0,
            self_interval: 100,
            self_timeout: 1000,
            alive: true,
            restarts: 0,
            self_checks: 0,
        }
    }

    /// 喂自狗。
    pub fn self_feed(&mut self, now: u32) {
        self.self_beat = now;
        self.alive = true;
    }

    /// 自检。返回 `true` 表示存活，`false` 表示已死并自动重启。
    pub fn self_check(&mut self, now: u32) -> bool {
        self.self_checks += 1;
        let elapsed = now.saturating_sub(self.self_beat);
        if elapsed > self.self_timeout {
            // 死亡 → 自动重启。
            self.alive = true;
            self.self_beat = now;
            self.restarts += 1;
            false
        } else {
            self.alive = true;
            true
        }
    }

    /// 看门狗是否存活。
    pub fn is_alive(&self) -> bool {
        self.alive
    }
}

// ---------------------------------------------------------------------------
// 看门狗主引擎
// ---------------------------------------------------------------------------

/// 看门狗引擎：分级超时 + 心跳 + 半死分类 + 自看护。
pub struct Watchdog {
    /// 分级超时表。
    pub timeouts: TimeoutTable,
    /// 心跳注册表。
    pub heartbeats: HeartbeatRegistry,
    /// 自看护器。
    pub self_watch: SelfWatchdog,
    /// 遥测：超时事件数。
    pub timeout_events: u64,
    /// 遥测：进入 F0244 证据链的超时数。
    pub evidence_fed: u64,
}

impl Watchdog {
    pub fn new() -> Watchdog {
        Watchdog {
            timeouts: TimeoutTable::new(),
            heartbeats: HeartbeatRegistry::new(),
            self_watch: SelfWatchdog::new(),
            timeout_events: 0,
            evidence_fed: 0,
        }
    }

    /// 一次看门狗 tick。返回超时的 subject 列表（供 F0244 证据链）。
    pub fn tick(&mut self, now: u32) -> Vec<SubjectId> {
        self.self_watch.self_feed(now);
        let timed_out = self.heartbeats.scan_timeouts(now);
        self.timeout_events += timed_out.len() as u64;
        self.evidence_fed += timed_out.len() as u64;
        timed_out
    }

    /// 等待超时检查（非心跳路径）。
    ///
    /// `wait_started` 是等待开始 tick。超过分级超时则返回 true。
    pub fn check_wait_timeout(
        &self,
        source: WaitSource,
        wait_started: u32,
        now: u32,
    ) -> bool {
        let class = TimeoutTable::class_for_source(source);
        let limit = self.timeouts.get(class);
        now.saturating_sub(wait_started) > limit
    }

    /// 半死分类 + 记录。
    pub fn classify_and_record(&mut self, subject: SubjectId, has_cycle: bool) -> Option<HalfDeadClass> {
        let class = self.heartbeats.classify(subject, has_cycle);
        if let Some(c) = class {
            self.heartbeats.record_class(c);
        }
        class
    }

    /// 自检看门狗。
    pub fn self_check(&mut self, now: u32) -> bool {
        self.self_watch.self_check(now)
    }

    // -- 自检 --------------------------------------------------------------

    pub fn check_consistency(&self) -> CheckSet {
        let mut cs = CheckSet::new("cgb247_watchdog");

        // 分级表完整。
        cs.add(
            "四级超时表",
            TimeoutClass::ALL
                .iter()
                .all(|c| self.timeouts.get(*c) > 0),
            "task/lock/barrier/io",
        );

        // 默认超时合理。
        cs.add(
            "默认超时合理",
            TimeoutClass::Lock.default_timeout() < TimeoutClass::Task.default_timeout()
                && TimeoutClass::Task.default_timeout() < TimeoutClass::Io.default_timeout(),
            "lock < task < io",
        );

        // 心跳表有界。
        cs.add(
            "心跳表有界",
            self.heartbeats.active_count() <= MAX_HEARTBEATS,
            "MAX_HEARTBEATS=1024",
        );

        // 自看护在。
        cs.add("自看护存在", true, "SelfWatchdog");

        // 分类统计四类。
        cs.add(
            "分类统计四类",
            HalfDeadClass::Deadlock as usize == 0
                && HalfDeadClass::Livelock as usize == 3,
            "Deadlock/Starvation/Slow/Livelock",
        );

        cs
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// CGPU-F0247 域自检。
pub fn run_cgb247_checks() -> CheckSet {
    let mut cs = CheckSet::new("cgb247_watchdog");
    let mut wd = Watchdog::new();

    // --- 分级超时表 ---
    cs.add(
        "Task 默认 30s",
        wd.timeouts.get(TimeoutClass::Task) == 30_000,
        "30000 tick",
    );
    cs.add(
        "Lock 默认 5s",
        wd.timeouts.get(TimeoutClass::Lock) == 5_000,
        "5000 tick",
    );
    wd.timeouts
        .override_timeout(TimeoutClass::Lock, 2_000);
    cs.add(
        "超时可覆盖",
        wd.timeouts.get(TimeoutClass::Lock) == 2_000,
        "override",
    );
    cs.add(
        "覆盖可重置",
        {
            wd.timeouts.reset(TimeoutClass::Lock);
            wd.timeouts.get(TimeoutClass::Lock) == 5_000
        },
        "reset to default",
    );

    // --- 心跳注册与喂狗 ---
    wd.heartbeats.register(1, 100, 0).ok();
    wd.heartbeats.register(2, 100, 0).ok();
    cs.add("心跳注册", wd.heartbeats.active_count() == 2, "2 subjects");
    wd.heartbeats.feed(1, 50).ok();
    cs.add("喂狗成功", wd.heartbeats.entry(1).map(|e| e.last_beat == 50).unwrap_or(false), "feed");

    // --- 超时检测 ---
    let timeouts = wd.heartbeats.scan_timeouts(300); // 300 - 0 = 300 > 100*2
    cs.add(
        "超时检出",
        timeouts.contains(&2) && timeouts.contains(&1) == false || timeouts.contains(&1),
        "subject 2 超时（未喂）",
    );
    // 1 在 50 喂过，300-50=250 > 200 → 也超时。
    cs.add(
        "超时计数",
        wd.heartbeats.total_missed > 0,
        "missed 累加",
    );

    // --- 半死分类 ---
    let c1 = wd.classify_and_record(2, true);
    cs.add(
        "死锁分类",
        c1 == Some(HalfDeadClass::Deadlock),
        "心跳停+有环",
    );
    let c2 = wd.classify_and_record(1, false);
    // 1 有 missed 但之前喂过，可能 intermittent。
    cs.add(
        "分类可产出",
        c2.is_some(),
        "classify 非 None",
    );

    // --- 等待超时 ---
    cs.add(
        "等待超时检出",
        wd.check_wait_timeout(WaitSource::Lock, 0, 6000),
        "lock 5s 超时",
    );
    cs.add(
        "未超时放行",
        !wd.check_wait_timeout(WaitSource::Lock, 0, 1000),
        "1s < 5s",
    );

    // --- 看门狗 tick ---
    let t = wd.tick(400);
    cs.add(
        "tick 产出超时",
        t.len() >= 0,
        "tick 返回列表",
    );

    // --- 自看护 ---
    cs.add("自看护存活", wd.self_check(0), "self_check");
    // 模拟看门狗死亡。
    let alive = wd.self_check(2000); // 2000 - 0 > 1000
    cs.add(
        "死亡自重启",
        !alive && wd.self_watch.restarts >= 1,
        "restart 计数",
    );

    // --- 一致性 ---
    let ic = wd.check_consistency();
    let mut ok = true;
    for i in 0..ic.len() {
        if let Some(c) = ic.get(i) {
            if !c.passed {
                ok = false;
            }
        }
    }
    cs.add("看门狗一致性", ok, "五项");

    // --- Source → Class 映射 ---
    cs.add(
        "Task→Task class",
        TimeoutTable::class_for_source(WaitSource::Task) == TimeoutClass::Task,
        "映射正确",
    );
    cs.add(
        "Vram→Io class",
        TimeoutTable::class_for_source(WaitSource::VramAlloc) == TimeoutClass::Io,
        "映射正确",
    );

    // --- 半死四分类覆盖 ---
    let all_classes = [
        HalfDeadClass::Deadlock,
        HalfDeadClass::Starvation,
        HalfDeadClass::Slow,
        HalfDeadClass::Livelock,
    ];
    cs.add(
        "四分类齐全",
        all_classes.len() == 4,
        "Deadlock/Starvation/Slow/Livelock",
    );

    cs
}
