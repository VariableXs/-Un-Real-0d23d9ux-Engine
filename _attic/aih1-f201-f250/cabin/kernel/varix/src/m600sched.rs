//! m600sched — VARIX-M600 AI-03 调度与实时域 (F051~F075)
//!
//! 感知延迟预算器/交互泳道/音频安全线程/帧节拍器/优先级反转拆弹/
//! 抢占延迟仪/核间负载舞谱/能量比例调度/前台提权法庭/后台节流公约/
//! 实时审计器/截止期谱系/长任务拆分器/核亲和画布/温度响应调度/
//! 电池档调度/抖动追踪器/调度公平账本/中断归并器/自旋成本模型/
//! 优先级继承树/调度回放沙盒/负载预测器/微突发吸收器/调度年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F051 — 感知延迟预算器：一帧 16ms，超支记账
// ===========================================================================

pub const PERCEPTION_BUDGET_MS: u32 = 16;

#[derive(Clone, Copy, Debug, Default)]
pub struct SchedLatencyLedger {
    pub spent_us: u64,
    pub overspends: u32,
}

impl SchedLatencyLedger {
    /// 记一笔延迟；超过感知预算记超支。
    pub fn charge(&mut self, us: u32) -> bool {
        self.spent_us += us as u64;
        if us > PERCEPTION_BUDGET_MS * 1000 {
            self.overspends += 1;
            false
        } else {
            true
        }
    }

    pub fn within_budget(&self) -> bool {
        self.overspends == 0
    }
}

// ===========================================================================
// F052 — 交互泳道：输入事件给 5 拍加速，逐拍衰减
// ===========================================================================

pub const LANE_BOOST_TICKS: u32 = 5;

#[derive(Clone, Copy, Debug, Default)]
pub struct SchedLane {
    pub boost_ticks: u32,
}

impl SchedLane {
    pub fn on_input(&mut self) {
        self.boost_ticks = LANE_BOOST_TICKS;
    }

    pub fn tick(&mut self) {
        if self.boost_ticks > 0 {
            self.boost_ticks -= 1;
        }
    }

    pub fn boosted(&self) -> bool {
        self.boost_ticks > 0
    }
}

// ===========================================================================
// F053 — 音频安全线程：队列水位既不能欠也不能过
// ===========================================================================

pub const AUDIO_MIN_QUEUE_MS: u32 = 2;
pub const AUDIO_MAX_QUEUE_MS: u32 = 10;

/// 队列水位低于下限会欠载（爆音），高于上限延迟超标。
pub fn audio_queue_safe(queue_ms: u32) -> bool {
    queue_ms >= AUDIO_MIN_QUEUE_MS && queue_ms <= AUDIO_MAX_QUEUE_MS
}

// ===========================================================================
// F054 — 帧节拍器：欠帧追赶最多补 2 帧
// ===========================================================================

pub const CATCHUP_CAP_FRAMES: u32 = 2;

/// 距上次提交欠了几帧（封顶 CATCHUP_CAP_FRAMES，防止死亡行军）。
pub fn frames_owed(last_frame_ms: u64, now_ms: u64, interval_ms: u32) -> u32 {
    if now_ms <= last_frame_ms || interval_ms == 0 {
        return 0;
    }
    let behind = ((now_ms - last_frame_ms) / interval_ms as u64) as u32;
    if behind > CATCHUP_CAP_FRAMES {
        CATCHUP_CAP_FRAMES
    } else {
        behind
    }
}

// ===========================================================================
// F055 — 优先级反转拆弹：持锁者继承等待者的优先级
// ===========================================================================

/// 数值越大优先级越高；继承取两者较高者。
pub const fn inherit_prio(owner_prio: u8, waiter_prio: u8) -> u8 {
    if waiter_prio > owner_prio {
        waiter_prio
    } else {
        owner_prio
    }
}

// ===========================================================================
// F056 — 抢占延迟仪：记录最坏抢占延迟
// ===========================================================================

#[derive(Clone, Copy, Debug, Default)]
pub struct PreemptMeter {
    pub worst_us: u32,
    pub samples: u64,
}

impl PreemptMeter {
    pub fn observe(&mut self, preempt_us: u32) {
        self.samples += 1;
        if preempt_us > self.worst_us {
            self.worst_us = preempt_us;
        }
    }
}

// ===========================================================================
// F057 — 核间负载舞谱：把活派给最闲的核（并列取编号小者）
// ===========================================================================

pub const DANCE_CORES: usize = 4;

pub fn least_loaded(loads: &[u32; DANCE_CORES]) -> usize {
    let mut best = 0usize;
    let mut i = 1usize;
    while i < DANCE_CORES {
        if loads[i] < loads[best] {
            best = i;
        }
        i += 1;
    }
    best
}

// ===========================================================================
// F058 — 能量比例调度：任务打包到尽量少的核
// ===========================================================================

/// 需要的核数 = ceil(tasks / cap)；cap 为 0 视为不调度。
pub fn cores_for(tasks: u32, per_core_cap: u32) -> u32 {
    if tasks == 0 || per_core_cap == 0 {
        0
    } else {
        (tasks + per_core_cap - 1) / per_core_cap
    }
}

// ===========================================================================
// F059 — 前台提权法庭：前台且未在 boosted 才批准，拒绝记账
// ===========================================================================

#[derive(Clone, Copy, Debug, Default)]
pub struct BoostCourt {
    pub granted: u32,
    pub denied: u32,
    pub fg_boosted: bool,
}

impl BoostCourt {
    /// 前台申请提权；已 boost 状态下重复申请驳回。
    pub fn request(&mut self, foreground: bool) -> bool {
        if foreground && !self.fg_boosted {
            self.fg_boosted = true;
            self.granted += 1;
            true
        } else {
            self.denied += 1;
            false
        }
    }

    /// 前台退场，解除 boost。
    pub fn resign(&mut self) {
        self.fg_boosted = false;
    }
}

// ===========================================================================
// F060 — 后台节流公约：有前台时后台只许 20% 份额
// ===========================================================================

pub const BG_SHARE_PERMILLE: u32 = 200;

pub const fn bg_slice_permille(active_fg_tasks: u32) -> u32 {
    if active_fg_tasks == 0 {
        1000
    } else {
        BG_SHARE_PERMILLE
    }
}

// ===========================================================================
// F061 — 实时审计器：RT 线程必须 prio 1~99 且申报截止期
// ===========================================================================

pub const RT_PRIO_MIN: u8 = 1;
pub const RT_PRIO_MAX: u8 = 99;

pub const fn rt_valid(prio: u8, has_deadline: bool) -> bool {
    prio >= RT_PRIO_MIN && prio <= RT_PRIO_MAX && has_deadline
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RtAudit {
    pub violations: u32,
}

impl RtAudit {
    pub fn audit(&mut self, prio: u8, has_deadline: bool) -> bool {
        if rt_valid(prio, has_deadline) {
            true
        } else {
            self.violations += 1;
            false
        }
    }
}

// ===========================================================================
// F062 — 截止期谱系：绝对截止期 = 释放点 + 相对截止期
// ===========================================================================

pub const fn abs_deadline(release_ns: u64, relative_dl_ns: u64) -> u64 {
    release_ns + relative_dl_ns
}

/// EDF：早者先行；并列取先申报者。
pub const fn edf_earlier(dl_a: u64, dl_b: u64) -> u64 {
    if dl_b < dl_a {
        dl_b
    } else {
        dl_a
    }
}

// ===========================================================================
// F063 — 长任务拆分器：切成不超过上限的片
// ===========================================================================

/// 片数 = ceil(runtime / max_slice)；上限为 0 视为不可拆。
pub fn slices_for(runtime_ms: u32, max_slice_ms: u32) -> u32 {
    if runtime_ms == 0 || max_slice_ms == 0 {
        0
    } else {
        (runtime_ms + max_slice_ms - 1) / max_slice_ms
    }
}

// ===========================================================================
// F064 — 核亲和画布：8 核位掩码，挑第一个允许核
// ===========================================================================

pub const CANVAS_CORES: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct AffinityCanvas {
    pub mask: [bool; CANVAS_CORES],
}

impl AffinityCanvas {
    pub const fn new() -> AffinityCanvas {
        AffinityCanvas { mask: [false; CANVAS_CORES] }
    }

    pub fn set_core(&mut self, core: usize) -> bool {
        if core < CANVAS_CORES {
            self.mask[core] = true;
            true
        } else {
            false
        }
    }

    pub fn clear_core(&mut self, core: usize) -> bool {
        if core < CANVAS_CORES {
            self.mask[core] = false;
            true
        } else {
            false
        }
    }

    /// 第一个允许核；全禁返回 None。
    pub fn pick(&self) -> Option<usize> {
        let mut i = 0usize;
        while i < CANVAS_CORES {
            if self.mask[i] {
                return Some(i);
            }
            i += 1;
        }
        None
    }
}

// ===========================================================================
// F065 — 温度响应调度：温度档位降频帽
// ===========================================================================

pub const THERMAL_WARM_C: u32 = 70;
pub const THERMAL_HOT_C: u32 = 85;

pub const fn thermal_cap_permille(temp_c: u32) -> u32 {
    if temp_c >= THERMAL_HOT_C {
        300
    } else if temp_c >= THERMAL_WARM_C {
        600
    } else {
        1000
    }
}

// ===========================================================================
// F066 — 电池档调度：电量决定可用核数
// ===========================================================================

pub const BATTERY_MID_PERMILLE: u32 = 500;
pub const BATTERY_LOW_PERMILLE: u32 = 200;

pub const fn battery_cores(battery_permille_of_1000: u32) -> u32 {
    if battery_permille_of_1000 > BATTERY_MID_PERMILLE {
        8
    } else if battery_permille_of_1000 >= BATTERY_LOW_PERMILLE {
        4
    } else {
        2
    }
}

// ===========================================================================
// F067 — 抖动追踪器：抖动 EWMA，旧 7 新 3
// ===========================================================================

pub const fn jitter_ewma(cur_permille: u32, sample_permille: u32) -> u32 {
    (cur_permille * 7 + sample_permille * 3) / 10
}

// ===========================================================================
// F068 — 调度公平账本：vruntime 按权重计账
// ===========================================================================

/// vruntime += delta * 1000 / weight；权重 0 不计账。
pub const fn vruntime_step(vruntime_ns: u64, delta_ns: u32, weight: u32) -> u64 {
    if weight == 0 {
        return vruntime_ns;
    }
    vruntime_ns + (delta_ns as u64) * 1000 / weight as u64
}

// ===========================================================================
// F069 — 中断归并器：窗口内最多送 16 条，其余记合并
// ===========================================================================

pub const IRQ_WINDOW_MAX: u32 = 16;

#[derive(Clone, Copy, Debug, Default)]
pub struct IrqCoalescer {
    pub pending: u32,
    pub merged: u32,
    pub windows: u32,
}

impl IrqCoalescer {
    pub fn raise(&mut self, n: u32) {
        self.pending += n;
    }

    /// 一个归并窗口：最多送 IRQ_WINDOW_MAX 条，超窗部分记为已合并（丢弃）。
    pub fn flush(&mut self) -> u32 {
        let delivered = if self.pending > IRQ_WINDOW_MAX { IRQ_WINDOW_MAX } else { self.pending };
        self.merged += self.pending - delivered;
        self.pending = 0;
        self.windows += 1;
        delivered
    }
}

// ===========================================================================
// F070 — 自旋成本模型：短持锁自旋划算，长持锁去睡
// ===========================================================================

pub const SPIN_WORTHWHILE_US: u32 = 10;
/// 每次自旋迭代成本 2us。
pub const SPIN_ITER_COST_US: u32 = 2;

pub const fn spin_worthwhile(hold_us: u32) -> bool {
    hold_us > 0 && hold_us <= SPIN_WORTHWHILE_US
}

pub const fn spin_cost_us(iterations: u32) -> u32 {
    iterations.saturating_mul(SPIN_ITER_COST_US)
}

// ===========================================================================
// F071 — 优先级继承树：锁链上向上传播最高优先级
// ===========================================================================

/// 链上最高优先级（空链为 0）。
pub fn chain_prio(pris: &[u8]) -> u8 {
    let mut best = 0u8;
    let mut i = 0usize;
    while i < pris.len() {
        if pris[i] > best {
            best = pris[i];
        }
        i += 1;
    }
    best
}

// ===========================================================================
// F072 — 调度回放沙盒：事件序号必须严格递增才可回放
// ===========================================================================

pub const REPLAY_LOG_CAP: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct ReplayLog {
    seqs: [u32; REPLAY_LOG_CAP],
    len: usize,
}

impl ReplayLog {
    pub const fn new() -> ReplayLog {
        ReplayLog { seqs: [0; REPLAY_LOG_CAP], len: 0 }
    }

    /// 记录事件；序号未严格递增或日志满都拒绝。
    pub fn record(&mut self, seq: u32) -> bool {
        if self.len >= REPLAY_LOG_CAP {
            return false;
        }
        if self.len > 0 && seq <= self.seqs[self.len - 1] {
            return false;
        }
        self.seqs[self.len] = seq;
        self.len += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.len
    }

    /// 回放有效性：已录事件本身保证递增（record 入口把关）。
    pub fn replay_ok(&self) -> bool {
        true
    }

    pub fn seq_at(&self, i: usize) -> u32 {
        self.seqs[i]
    }
}

// ===========================================================================
// F073 — 负载预测器：EWMA 预测，旧 8 新 2
// ===========================================================================

pub const fn load_predict(prev_load: u32, sample_load: u32) -> u32 {
    (prev_load * 800 + sample_load * 200) / 1000
}

// ===========================================================================
// F074 — 微突发吸收器：定容队列吞突发，溢出记账
// ===========================================================================

#[derive(Clone, Copy, Debug)]
pub struct BurstBuf {
    pub cap: u32,
    pub queued: u32,
    pub dropped: u32,
}

impl BurstBuf {
    pub const fn new(cap: u32) -> BurstBuf {
        BurstBuf { cap, queued: 0, dropped: 0 }
    }

    /// 吸收 n 个事件，返回实际吸收数；装不下的记账丢弃。
    pub fn absorb(&mut self, n: u32) -> u32 {
        let free = self.cap - self.queued;
        let taken = if n > free { free } else { n };
        self.dropped += n - taken;
        self.queued += taken;
        taken
    }

    /// 一次排空，返回排掉的数量。
    pub fn drain(&mut self) -> u32 {
        let out = self.queued;
        self.queued = 0;
        out
    }
}

// ===========================================================================
// F075 — 调度年报：章节完备性 + 年度总量账
// ===========================================================================

pub const SCHED_REPORT_SECTIONS: [&str; 5] =
    ["latency", "lanes", "fairness", "energy", "jitter"];

#[derive(Clone, Copy, Debug, Default)]
pub struct SchedYear {
    pub frames_beat: u64,
    pub boosts_granted: u32,
    pub migrations: u32,
}

impl SchedYear {
    pub fn record_frame(&mut self) {
        self.frames_beat += 1;
    }
    pub fn record_boost(&mut self) {
        self.boosts_granted += 1;
    }
    pub fn record_migration(&mut self) {
        self.migrations += 1;
    }
}

pub fn sched_report_complete(sections_filled: u32) -> bool {
    sections_filled >= SCHED_REPORT_SECTIONS.len() as u32
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m600sched_checks() -> CheckSet {
    let mut set = CheckSet::new("m600sched");

    // F051 感知延迟预算器
    let mut led = SchedLatencyLedger::default();
    let c1 = led.charge(8000);
    let spent_mid = led.spent_us;
    let c2 = led.charge(17000);
    let spent_end = led.spent_us;
    set.add(
        "F051 perception budget",
        c1 && spent_mid == 8000 && !c2 && spent_end == 25000 && led.overspends == 1,
        "one overspend booked",
    );
    set.add("F051 budget verdict", !led.within_budget(), "overspend visible");

    // F052 交互泳道
    let mut lane = SchedLane::default();
    lane.on_input();
    let boost0 = lane.boost_ticks;
    lane.tick();
    lane.tick();
    let boost2 = lane.boost_ticks;
    lane.tick();
    lane.tick();
    lane.tick();
    let boost5 = lane.boost_ticks;
    lane.tick();
    let boost6 = lane.boost_ticks;
    set.add(
        "F052 lane boost decay",
        boost0 == 5 && boost2 == 3 && boost5 == 0 && boost6 == 0 && !lane.boosted(),
        "5-tick decay to rest",
    );

    // F053 音频安全线程
    set.add(
        "F053 audio watermarks",
        !audio_queue_safe(1) && audio_queue_safe(2) && audio_queue_safe(10)
            && !audio_queue_safe(11),
        "2ms..10ms inclusive",
    );

    // F054 帧节拍器
    set.add(
        "F054 frames owed",
        frames_owed(0, 16, 16) == 1 && frames_owed(0, 0, 16) == 0,
        "exact beat counts one",
    );
    set.add(
        "F054 catchup capped",
        frames_owed(0, 80, 16) == CATCHUP_CAP_FRAMES && frames_owed(0, 16, 0) == 0,
        "no death march",
    );

    // F055 优先级反转拆弹
    set.add(
        "F055 prio inheritance",
        inherit_prio(3, 7) == 7 && inherit_prio(9, 4) == 9 && inherit_prio(5, 5) == 5,
        "max of owner and waiter",
    );

    // F056 抢占延迟仪
    let mut pm = PreemptMeter::default();
    pm.observe(200);
    pm.observe(150);
    pm.observe(900);
    let worst = pm.worst_us;
    let samples = pm.samples;
    set.add(
        "F056 preempt worst",
        worst == 900 && samples == 3,
        "max tracked across samples",
    );

    // F057 核间负载舞谱
    set.add(
        "F057 dance pick",
        least_loaded(&[5, 3, 7, 3]) == 1 && least_loaded(&[0, 0, 0, 0]) == 0,
        "least loaded, ties low",
    );

    // F058 能量比例调度
    set.add(
        "F058 cores packing",
        cores_for(10, 4) == 3 && cores_for(8, 4) == 2,
        "ceil packing",
    );
    set.add(
        "F058 cores degenerate",
        cores_for(0, 4) == 0 && cores_for(10, 0) == 0,
        "nothing to schedule",
    );

    // F059 前台提权法庭
    let mut court = BoostCourt::default();
    let r1 = court.request(true);
    let granted1 = court.granted;
    let r2 = court.request(true);
    let denied1 = court.denied;
    set.add(
        "F059 boost once",
        r1 && granted1 == 1 && !r2 && denied1 == 1,
        "repeat boost denied",
    );
    court.resign();
    let r3 = court.request(false);
    let r4 = court.request(true);
    let granted2 = court.granted;
    set.add(
        "F059 boost after resign",
        !r3 && r4 && granted2 == 2,
        "background refused, fg re-granted",
    );

    // F060 后台节流公约
    set.add(
        "F060 bg throttle",
        bg_slice_permille(0) == 1000 && bg_slice_permille(1) == 200
            && bg_slice_permille(100) == 200,
        "fg presence throttles bg",
    );

    // F061 实时审计器
    let mut audit = RtAudit::default();
    let ok = audit.audit(50, true);
    let bad_low = audit.audit(0, true);
    let bad_high = audit.audit(100, true);
    let bad_dl = audit.audit(50, false);
    let violations = audit.violations;
    set.add(
        "F061 rt audit",
        ok && !bad_low && !bad_high && !bad_dl && violations == 3,
        "prio window and deadline",
    );
    set.add("F061 rt valid bounds", rt_valid(1, true) && rt_valid(99, true), "1..99 inclusive");

    // F062 截止期谱系
    set.add(
        "F062 abs deadline",
        abs_deadline(100, 50) == 150 && abs_deadline(0, 0) == 0,
        "release plus relative",
    );
    set.add(
        "F062 edf order",
        edf_earlier(30, 50) == 30 && edf_earlier(50, 30) == 30 && edf_earlier(40, 40) == 40,
        "earlier deadline first",
    );

    // F063 长任务拆分器
    set.add(
        "F063 slice math",
        slices_for(13, 5) == 3 && slices_for(10, 5) == 2 && slices_for(3, 5) == 1,
        "ceil slicing",
    );
    set.add(
        "F063 slice degenerate",
        slices_for(5, 0) == 0 && slices_for(0, 5) == 0,
        "no cap or no work, no slices",
    );

    // F064 核亲和画布
    let mut cv = AffinityCanvas::new();
    let empty_pick = cv.pick();
    cv.set_core(2);
    cv.set_core(5);
    let pick1 = cv.pick();
    cv.clear_core(2);
    let pick2 = cv.pick();
    let bad_core = cv.set_core(8);
    set.add(
        "F064 affinity canvas",
        empty_pick.is_none() && pick1 == Some(2) && pick2 == Some(5) && !bad_core,
        "first allowed core wins",
    );

    // F065 温度响应调度
    set.add(
        "F065 thermal caps",
        thermal_cap_permille(69) == 1000 && thermal_cap_permille(70) == 600
            && thermal_cap_permille(84) == 600 && thermal_cap_permille(85) == 300,
        "tier boundaries exact",
    );

    // F066 电池档调度
    set.add(
        "F066 battery tiers",
        battery_cores(501) == 8 && battery_cores(500) == 4 && battery_cores(200) == 4
            && battery_cores(199) == 2,
        "cores shrink with battery",
    );

    // F067 抖动追踪器
    set.add(
        "F067 jitter ewma",
        jitter_ewma(100, 200) == 130 && jitter_ewma(0, 1000) == 300 && jitter_ewma(0, 0) == 0,
        "7:3 blend",
    );

    // F068 调度公平账本
    set.add(
        "F068 vruntime weights",
        vruntime_step(0, 10, 1000) == 10 && vruntime_step(10, 10, 500) == 30,
        "lighter weight accrues faster",
    );
    set.add(
        "F068 vruntime zero weight",
        vruntime_step(10, 10, 0) == 10,
        "weight 0 books nothing",
    );

    // F069 中断归并器
    let mut irq = IrqCoalescer::default();
    irq.raise(30);
    let d1 = irq.flush();
    let merged1 = irq.merged;
    let pending1 = irq.pending;
    set.add(
        "F069 coalesce window",
        d1 == 16 && merged1 == 14 && pending1 == 0,
        "16 delivered, 14 merged",
    );
    irq.raise(5);
    let d2 = irq.flush();
    let windows2 = irq.windows;
    set.add("F069 coalesce small burst", d2 == 5 && windows2 == 2, "small burst fully delivered");

    // F070 自旋成本模型
    set.add(
        "F070 spin verdict",
        !spin_worthwhile(0) && spin_worthwhile(1) && spin_worthwhile(10)
            && !spin_worthwhile(11),
        "short holds spin",
    );
    set.add(
        "F070 spin cost",
        spin_cost_us(0) == 0 && spin_cost_us(5) == 10,
        "2us per iteration",
    );

    // F071 优先级继承树
    set.add(
        "F071 chain propagation",
        chain_prio(&[3, 7, 5]) == 7 && chain_prio(&[]) == 0 && chain_prio(&[1]) == 1,
        "max up the chain",
    );

    // F072 调度回放沙盒
    let mut log = ReplayLog::new();
    let e1 = log.record(1);
    let e2 = log.record(2);
    let e3 = log.record(3);
    let e_dup = log.record(3);
    let e4 = log.record(9);
    let log_len = log.len();
    set.add(
        "F072 replay ordering",
        e1 && e2 && e3 && !e_dup && e4 && log_len == 4 && log.seq_at(0) == 1,
        "strictly increasing only",
    );
    let mut full = ReplayLog::new();
    let mut s = 1u32;
    while s <= 8 {
        full.record(s);
        s += 1;
    }
    let full_len = full.len();
    let overflow = full.record(9);
    set.add(
        "F072 replay capacity",
        full_len == REPLAY_LOG_CAP && !overflow && full.replay_ok(),
        "8 events max",
    );

    // F073 负载预测器
    set.add(
        "F073 load ewma",
        load_predict(1000, 0) == 800 && load_predict(0, 1000) == 200
            && load_predict(500, 500) == 500,
        "8:2 blend",
    );

    // F074 微突发吸收器
    let mut bb = BurstBuf::new(10);
    let t1 = bb.absorb(4);
    let t2 = bb.absorb(9);
    let dropped = bb.dropped;
    set.add(
        "F074 burst absorb",
        t1 == 4 && t2 == 6 && dropped == 3 && bb.queued == 10,
        "overflow booked as dropped",
    );
    let drained = bb.drain();
    let after = bb.queued;
    set.add("F074 burst drain", drained == 10 && after == 0, "drain empties");

    // F075 调度年报
    let mut year = SchedYear::default();
    year.record_frame();
    year.record_frame();
    year.record_frame();
    year.record_boost();
    year.record_boost();
    year.record_migration();
    set.add(
        "F075 sched yearbook",
        SCHED_REPORT_SECTIONS.len() == 5 && year.frames_beat == 3
            && year.boosts_granted == 2 && year.migrations == 1,
        "yearly totals accounted",
    );
    set.add(
        "F075 sched report sections",
        sched_report_complete(5) && !sched_report_complete(4),
        "five sections required",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f052_lane_boost_decay() {
        let mut lane = SchedLane::default();
        lane.on_input();
        assert_eq!(lane.boost_ticks, 5);
        let mut i = 0;
        while i < 5 {
            assert!(lane.boosted());
            lane.tick();
            i += 1;
        }
        assert!(!lane.boosted());
        lane.tick();
        assert_eq!(lane.boost_ticks, 0);
    }

    #[test]
    fn f054_metronome_catchup_cap() {
        assert_eq!(frames_owed(0, 16, 16), 1);
        assert_eq!(frames_owed(0, 32, 16), 2);
        assert_eq!(frames_owed(0, 160, 16), 2);
        assert_eq!(frames_owed(100, 100, 16), 0);
    }

    #[test]
    fn f059_boost_court_rules() {
        let mut c = BoostCourt::default();
        assert!(c.request(true));
        assert!(!c.request(true));
        assert_eq!(c.denied, 1);
        c.resign();
        assert!(!c.request(false));
        assert!(c.request(true));
        assert_eq!(c.granted, 2);
    }

    #[test]
    fn f067_jitter_blend() {
        assert_eq!(jitter_ewma(100, 200), 130);
        assert_eq!(jitter_ewma(130, 0), 91);
        assert_eq!(jitter_ewma(0, 0), 0);
    }

    #[test]
    fn f072_replay_strict_order() {
        let mut log = ReplayLog::new();
        assert!(log.record(5));
        assert!(log.record(6));
        assert!(!log.record(6));
        assert!(!log.record(4));
        assert!(log.record(7));
        assert_eq!(log.len(), 3);
    }

    #[test]
    fn m600sched_selfcheck_all_pass() {
        let set = run_m600sched_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
