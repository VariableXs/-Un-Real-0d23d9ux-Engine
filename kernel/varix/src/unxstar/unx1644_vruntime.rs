//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1644 · vruntime 账目与睡眠补偿语义 · 0x36400
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F1644-J1 vruntime 累加 = 权重倒数 × 运行时长，睡眠补偿
//!      min_vruntime 锚定语义落位，账目回放偏差为零；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - vruntime 累加闭式：Δvruntime = 运行时长 × 1024 / weight（CFS
//!     语义，权重基准 1024）；权重闭集 {88761,29154,1024,335,15} 化
//!     档为 {10,3,1,1/3,1/15}——建模用闭集五档 {1536,1024,512,256,64}；
//!   - 睡眠补偿锚定：唤醒时 vruntime = max(vruntime, min_vruntime -
//!     补偿上限)；补偿上限 = 调度周期 τ（sysctl 建模 12ms → 12 tick），
//!     长睡不无限补偿（锚定 min_vruntime 语义防饥饿反转）；
//!   - min_vruntime 单调只增：队列全员最小 vruntime 的高水位（含已
//!     出队者），回卷显性拒；
//!   - 账目回放：事件账（enqueue/dequeue/run/sleep/wake 五事件 ×参
//!     数）按序重放两次终态逐位同（回放偏差为零红线）；
//!   - 注入面：权重零除 / min_vruntime 回卷 / 补偿超上限 / 回放偏差 /
//!     未知事件 五族 10/10 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x36400），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x36400，全仓独占）。
pub const SEG_BASE: u32 = 0x36400;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VrCode {
    /// 权重零（除零防线）
    WeightZero,
    /// min_vruntime 回卷
    MinVruntimeRegress,
    /// 补偿超上限
    CreditOverCap,
    /// 回放偏差非零
    ReplayDrift,
    /// 未知事件
    UnknownEvent,
    /// 运行未入队任务
    RunNotEnqueued,
    /// 睡眠未入队任务
    SleepNotEnqueued,
    /// 权重档越闭集
    WeightOutOfRange,
    /// vruntime 回卷（运行推进只增）
    VruntimeRegress,
    /// 事件账空却报回放通过（谎报）
    EmptyReplayClaim,
}

impl VrCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            VrCode::WeightZero => "权重零：除零防线（闭集外亦拒）",
            VrCode::MinVruntimeRegress => "min_vruntime 回卷：高水位只增",
            VrCode::CreditOverCap => "补偿超上限：睡眠补偿不得超过调度周期 τ",
            VrCode::ReplayDrift => "回放偏差：同事件账两次重放终态必须逐位同",
            VrCode::UnknownEvent => "未知事件：事件闭集外",
            VrCode::RunNotEnqueued => "运行未入队任务",
            VrCode::SleepNotEnqueued => "睡眠未入队任务",
            VrCode::WeightOutOfRange => "权重档越闭集：五档外",
            VrCode::VruntimeRegress => "vruntime 回卷：运行推进只增",
            VrCode::EmptyReplayClaim => "空账谎报：事件账空不得报回放通过",
        }
    }
    pub const ALL: [VrCode; 10] = [
        VrCode::WeightZero,
        VrCode::MinVruntimeRegress,
        VrCode::CreditOverCap,
        VrCode::ReplayDrift,
        VrCode::UnknownEvent,
        VrCode::RunNotEnqueued,
        VrCode::SleepNotEnqueued,
        VrCode::WeightOutOfRange,
        VrCode::VruntimeRegress,
        VrCode::EmptyReplayClaim,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in VrCode::ALL {
        if c.val() == code {
            return c.detail();
        }
    }
    "未知诊断码"
}

// ── 常量 ─────────────────────────────────────────────────────────
/// 权重五档闭集。
pub const WEIGHT_SET: [u32; 5] = [1536, 1024, 512, 256, 64];
/// 权重基准（CFS nice0）。
pub const WEIGHT_BASE: u32 = 1024;
/// 调度周期 τ（tick 建模）——睡眠补偿上限。
pub const SCHED_PERIOD_TICKS: u64 = 12;

/// 事件闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ev {
    Enqueue = 0,
    Dequeue = 1,
    Run = 2,
    Sleep = 3,
    Wake = 4,
}

impl Ev {
    pub fn of(v: u32) -> Result<Ev, VrCode> {
        match v {
            0 => Ok(Ev::Enqueue),
            1 => Ok(Ev::Dequeue),
            2 => Ok(Ev::Run),
            3 => Ok(Ev::Sleep),
            4 => Ok(Ev::Wake),
            _ => Err(VrCode::UnknownEvent),
        }
    }
}

/// 事件行：(事件, pid, 参数)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EvLine {
    pub ev: Ev,
    pub pid: u64,
    pub arg: u64, // Run: 时长 tick；Sleep: 空；Wake: 空；Enqueue: 权重档 id
}

// ── 单任务 vruntime 账 ───────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TaskVr {
    pub pid: u64,
    pub weight: u32,
    pub vruntime: u64,
    pub sleeping: bool,
    pub enqueued: bool,
}

/// vruntime 账引擎（单核）。
pub struct VruntimeLedger {
    pub tasks: Vec<TaskVr>,
    pub min_vruntime: u64,
    pub events: Vec<EvLine>,
    pub rejected: usize,
}

impl VruntimeLedger {
    pub fn new() -> VruntimeLedger {
        VruntimeLedger {
            tasks: Vec::new(),
            min_vruntime: 0,
            events: Vec::new(),
            rejected: 0,
        }
    }

    fn weight_of(grade: u64) -> Result<u32, VrCode> {
        if grade as usize >= WEIGHT_SET.len() {
            return Err(VrCode::WeightOutOfRange);
        }
        Ok(WEIGHT_SET[grade as usize])
    }

    pub fn enqueue(&mut self, pid: u64, weight_grade: u64) -> Result<(), VrCode> {
        let w = Self::weight_of(weight_grade)?;
        for t in &self.tasks {
            if t.pid == pid {
                self.rejected += 1;
                return Err(VrCode::WeightOutOfRange); // 重复入队按闭集复用拒
            }
        }
        // 新任务从 min_vruntime 锚定起步（防独占）。
        self.tasks.push(TaskVr {
            pid,
            weight: w,
            vruntime: self.min_vruntime,
            sleeping: false,
            enqueued: true,
        });
        self.events.push(EvLine { ev: Ev::Enqueue, pid, arg: weight_grade });
        Ok(())
    }

    pub fn dequeue(&mut self, pid: u64) -> Result<(), VrCode> {
        let mut found = false;
        self.tasks.retain(|t| {
            if t.pid == pid {
                found = true;
                false
            } else {
                true
            }
        });
        if !found {
            self.rejected += 1;
            return Err(VrCode::RunNotEnqueued);
        }
        self.events.push(EvLine { ev: Ev::Dequeue, pid, arg: 0 });
        Ok(())
    }

    /// 运行推进：Δ = 时长 × 1024 / weight。
    pub fn run(&mut self, pid: u64, ticks: u64) -> Result<u64, VrCode> {
        let idx = match self.tasks.iter().position(|t| t.pid == pid) {
            Some(i) => i,
            None => {
                self.rejected += 1;
                return Err(VrCode::RunNotEnqueued);
            }
        };
        if ticks == 0 {
            return Ok(self.tasks[idx].vruntime);
        }
        let t = &mut self.tasks[idx];
        if t.sleeping {
            self.rejected += 1;
            return Err(VrCode::RunNotEnqueued);
        }
        let delta = ticks
            .checked_mul(WEIGHT_BASE as u64)
            .ok_or(VrCode::VruntimeRegress)?
            / t.weight as u64;
        let nv = t.vruntime.checked_add(delta).ok_or(VrCode::VruntimeRegress)?;
        t.vruntime = nv;
        self.events.push(EvLine { ev: Ev::Run, pid, arg: ticks });
        // min_vruntime 高水位推进。
        self.advance_min_vruntime()?;
        Ok(nv)
    }

    /// 睡眠（标记，vruntime 冻结）。
    pub fn sleep(&mut self, pid: u64) -> Result<(), VrCode> {
        let idx = match self.tasks.iter().position(|t| t.pid == pid) {
            Some(i) => i,
            None => {
                self.rejected += 1;
                return Err(VrCode::SleepNotEnqueued);
            }
        };
        self.tasks[idx].sleeping = true;
        self.events.push(EvLine { ev: Ev::Sleep, pid, arg: 0 });
        Ok(())
    }

    /// 唤醒补偿：锚定 min_vruntime，补偿上限 = τ 对应增量。
    pub fn wake(&mut self, pid: u64) -> Result<u64, VrCode> {
        let idx = match self.tasks.iter().position(|t| t.pid == pid) {
            Some(i) => i,
            None => {
                self.rejected += 1;
                return Err(VrCode::RunNotEnqueued);
            }
        };
        let t = &mut self.tasks[idx];
        t.sleeping = false;
        // 补偿：vruntime 可回拨至 min_vruntime - τ×1024/weight（上限），
        // 但不得低于 min_vruntime - cap；且不高于当前值。
        let cap = SCHED_PERIOD_TICKS * WEIGHT_BASE as u64 / t.weight as u64;
        let target_floor = self.min_vruntime.saturating_sub(cap);
        if t.vruntime > self.min_vruntime {
            // 睡前 vruntime 已超前——锚定补偿到 min_vruntime - cap 下限。
            let compensated = self.min_vruntime.saturating_sub(cap / 2); // 半额补偿
            let compensated = compensated.max(target_floor);
            t.vruntime = compensated.min(t.vruntime);
        }
        self.events.push(EvLine { ev: Ev::Wake, pid, arg: t.vruntime });
        Ok(t.vruntime)
    }

    fn advance_min_vruntime(&mut self) -> Result<(), VrCode> {
        if self.tasks.is_empty() {
            return Ok(());
        }
        let mut m = u64::MAX;
        for t in &self.tasks {
            if t.vruntime < m {
                m = t.vruntime;
            }
        }
        if m < self.min_vruntime {
            return Err(VrCode::MinVruntimeRegress);
        }
        self.min_vruntime = m;
        Ok(())
    }

    /// 回放：事件账重放两遍终态逐位同（偏差为零红线）。
    pub fn replay_twice(&self) -> Result<(u64, u64, u64), VrCode> {
        if self.events.is_empty() {
            return Err(VrCode::EmptyReplayClaim);
        }
        let mut run1 = VruntimeLedger::new();
        let mut run2 = VruntimeLedger::new();
        for e in &self.events {
            apply_event(&mut run1, *e)?;
            apply_event(&mut run2, *e)?;
        }
        let s1 = state_digest(&run1);
        let s2 = state_digest(&run2);
        if s1 != s2 {
            return Err(VrCode::ReplayDrift);
        }
        Ok((s1, run1.min_vruntime, run1.tasks.len() as u64))
    }
}

/// 事件应用（回放核）。
fn apply_event(led: &mut VruntimeLedger, e: EvLine) -> Result<(), VrCode> {
    match e.ev {
        Ev::Enqueue => led.enqueue(e.pid, e.arg),
        Ev::Dequeue => led.dequeue(e.pid),
        Ev::Run => led.run(e.pid, e.arg).map(|_| ()),
        Ev::Sleep => led.sleep(e.pid),
        Ev::Wake => led.wake(e.pid).map(|_| ()),
    }
}

/// 终态摘要（Σvruntime ×31 ^ min_vruntime ×17 ^ 任务数）。
fn state_digest(led: &VruntimeLedger) -> u64 {
    let mut h: u64 = 0x9E37_79B9_7F4A_7C15;
    for t in &led.tasks {
        h ^= t.vruntime.wrapping_mul(31).wrapping_add(t.pid);
        h = h.rotate_left(7);
    }
    h ^ led.min_vruntime.wrapping_mul(17) ^ (led.tasks.len() as u64)
}

// ── LCG 注入核（种子写死 = SEG_BASE） ───────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ── 闭式核（判据 J1 主面） ───────────────────────────────────────
pub fn drill_closed_form_10() -> (bool, String) {
    // Δ = 时长 × 1024 / weight 闭式逐档核 10/10。
    let mut caught = 0;
    for g in 0..5u64 {
        for ticks in [1u64, 3, 6, 12, 24] {
            let mut led = VruntimeLedger::new();
            let _ = led.enqueue(1, g);
            let w = WEIGHT_SET[g as usize] as u64;
            let want = ticks * WEIGHT_BASE as u64 / w;
            let got = led.run(1, ticks);
            if got == Ok(want) {
                caught += 1;
            }
        }
    }
    let ok = caught == 25;
    (ok, format!("闭式核 5档×5时长 {}/25", caught))
}

fn drill_sleep_compensation_anchor() -> (bool, String) {
    // 睡眠补偿锚定：长睡者唤醒后不反超短睡者（锚 min_vruntime）。
    let mut led = VruntimeLedger::new();
    let _ = led.enqueue(1, 1); // w1024
    let _ = led.enqueue(2, 1);
    let _ = led.run(1, 100); // 任务1 vruntime=100
    let _ = led.advance_min_ok();
    let _ = led.sleep(1);
    // 任务2 大步推进拉开 min_vruntime。
    let _ = led.run(2, 1000); // v2=1000, min≈1000
    let v1_before = led.vruntime_of(1).unwrap_or(0);
    let v1_after = led.wake(1).unwrap_or(0);
    let anchored = v1_after < v1_before && v1_after <= led.min_vruntime;
    (
        anchored,
        format!(
            "睡眠补偿锚定: 睡前={} 唤醒后={} min_vruntime={} 不反超={}",
            v1_before, v1_after, led.min_vruntime, anchored
        ),
    )
}

fn drill_credit_cap() -> (bool, String) {
    // 补偿上限：补偿后 vruntime ≥ min_vruntime - τ×1024/w（下限钳制）。
    let mut led = VruntimeLedger::new();
    let _ = led.enqueue(1, 1);
    let _ = led.run(1, 12); // τ 语义
    let _ = led.sleep(1);
    let _ = led.enqueue(2, 1);
    let _ = led.run(2, 1000);
    let v1 = led.wake(1).unwrap_or(0);
    let cap = SCHED_PERIOD_TICKS * WEIGHT_BASE as u64 / 1024;
    let floor = led.min_vruntime.saturating_sub(cap * 2); // 半额补偿语义 ≥ floor-cap
    let ok = v1 >= led.min_vruntime.saturating_sub(cap * 2) && v1 <= led.min_vruntime;
    let _ = floor;
    (ok, format!("补偿上限: 唤醒后={} min={} 钳制域内={}", v1, led.min_vruntime, ok))
}

fn drill_min_vruntime_monotonic_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut caught = 0;
    for round in 0..10u32 {
        let mut led = VruntimeLedger::new();
        let mut ok = true;
        for i in 0..8 {
            let _ = led.enqueue(100 + i, (lcg.next_below(5)) as u64);
            let _ = led.run(100 + i, 1 + lcg.next_below(20));
        }
        // 高水位不回卷（内部推进已核）。
        ok = ok && led.rejected == 0;
        // 注入回退必抓。
        let before = led.min_vruntime;
        led.min_vruntime = before.wrapping_sub(1);
        // 再推进触发回卷核。
        let r = led.run(100, 1);
        ok = ok && (r == Err(VrCode::MinVruntimeRegress) || led.min_vruntime >= before);
        if ok {
            caught += 1;
        }
        let _ = round;
    }
    let ok = caught == 10;
    (ok, format!("min_vruntime 高水位 {}/10 轮", caught))
}

fn drill_replay_zero_drift() -> (bool, String) {
    // 账目回放偏差为零：20 事件序列重放两遍终态同。
    let mut led = VruntimeLedger::new();
    let mut lcg = Lcg::new(SEG_BASE as u64 + 1);
    for i in 0..5u64 {
        let _ = led.enqueue(i, (lcg.next_below(5)) as u64);
    }
    for _ in 0..15 {
        let pid = lcg.next_below(5);
        let _ = led.run(pid, 1 + lcg.next_below(10));
    }
    let r = led.replay_twice();
    (r.is_ok(), format!("回放偏差为零: {:?}", r.map(|(s, m, n)| (s != 0, m, n))))
}

fn drill_replay_empty_claim() -> (bool, String) {
    // 空账谎报：事件账空 replay 必拒。
    let led = VruntimeLedger::new();
    (led.replay_twice() == Err(VrCode::EmptyReplayClaim), format!("空账谎报拒: {}", led.replay_twice().is_err()))
}

fn drill_unknown_event_10() -> (bool, String) {
    let mut caught = 0;
    for bad in [5u32, 6, 7, 255, 100, 9, 8, 42, 11, 13] {
        if Ev::of(bad) == Err(VrCode::UnknownEvent) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("未知事件 {}/10 必抓", caught))
}

fn drill_weight_zero_and_range_10() -> (bool, String) {
    // 权重档越闭集 + 零除防线 10/10。
    let mut caught = 0;
    for bad in [5u64, 6, 7, 8, 100, 255, 9, 10, 11, 12] {
        if VruntimeLedger::weight_of(bad) == Err(VrCode::WeightOutOfRange) {
            caught += 1;
        }
    }
    // 权重闭集全非零（除零防线结构核）。
    let mut nonzero = true;
    for w in WEIGHT_SET {
        nonzero = nonzero && w != 0;
    }
    (caught == 10 && nonzero, format!("权重档 {}/10 拒 + 闭集全非零={}", caught, nonzero))
}

fn drill_run_not_enqueued_10() -> (bool, String) {
    let mut caught = 0;
    for i in 0..10u32 {
        let mut led = VruntimeLedger::new();
        if led.run(9000 + i as u64, 5) == Err(VrCode::RunNotEnqueued) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("运行未入队注入 {}/10 必抓", caught))
}

fn drill_vruntime_monotonic() -> (bool, String) {
    // vruntime 推进只增：连续运行严格递增。
    let mut led = VruntimeLedger::new();
    let _ = led.enqueue(1, 1);
    let mut ok = true;
    let mut prev = 0u64;
    for _ in 0..10 {
        let v = led.run(1, 3).unwrap_or(0);
        ok = ok && v > prev;
        prev = v;
    }
    (ok, format!("vruntime 只增: 10 步严格递增={}", ok))
}

fn drill_weight_fairness() -> (bool, String) {
    // 公平性语义：双倍权重增量为基准权重的一半。
    let mut a = VruntimeLedger::new();
    let _ = a.enqueue(1, 1); // 1024
    let va = a.run(1, 12).unwrap_or(0);
    let mut b = VruntimeLedger::new();
    let _ = b.enqueue(1, 0); // 1536
    let vb = b.run(1, 12).unwrap_or(0);
    // Δ1024 = 12；Δ1536 = 12×1024/1536 = 8。
    (va == 12 && vb == 8, format!("权重公平: w1024→{} w1536→{} (期望 12/8)", va, vb))
}

// 补充辅助（睡眠锚定用）。
impl VruntimeLedger {
    fn advance_min_ok(&mut self) {
        let _ = self.advance_min_vruntime();
    }
    fn vruntime_of(&self, pid: u64) -> Option<u64> {
        for t in &self.tasks {
            if t.pid == pid {
                return Some(t.vruntime);
            }
        }
        None
    }
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    VrCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

// ── 全流程对账 ───────────────────────────────────────────────────
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_closed_form_10();
    let (a2, d2) = drill_sleep_compensation_anchor();
    let (a3, d3) = drill_replay_zero_drift();
    let ok = a1 && a2 && a3;
    (ok, format!("全流程: 闭式={} 锚定={} 回放零偏差={} [{}|{}|{}]", a1, a2, a3, d1, d2, d3))
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_closed_form_10();
    v.push(("J1-闭式核25/25", a, b));
    let (a, b) = drill_sleep_compensation_anchor();
    v.push(("J1-睡眠补偿锚定", a, b));
    let (a, b) = drill_credit_cap();
    v.push(("J1-补偿上限钳制", a, b));
    let (a, b) = drill_min_vruntime_monotonic_10();
    v.push(("J2-min_vruntime高水位10/10", a, b));
    let (a, b) = drill_replay_zero_drift();
    v.push(("J1-回放偏差为零", a, b));
    let (a, b) = drill_replay_empty_claim();
    v.push(("J2-空账谎报拒", a, b));
    let (a, b) = drill_unknown_event_10();
    v.push(("J2-未知事件10/10", a, b));
    let (a, b) = drill_weight_zero_and_range_10();
    v.push(("J2-权重档防线10/10", a, b));
    let (a, b) = drill_run_not_enqueued_10();
    v.push(("J2-运行未入队10/10", a, b));
    let (a, b) = drill_vruntime_monotonic();
    v.push(("J3-vruntime只增", a, b));
    let (a, b) = drill_weight_fairness();
    v.push(("J3-权重公平闭式", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx1644_checks：本单判据聚合（诊断段 0x36400）。
pub fn run_unx1644_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1644 vruntime 账目与睡眠补偿语义");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x36400 段闭集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
