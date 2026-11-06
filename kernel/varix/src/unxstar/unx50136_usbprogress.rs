//! UNX-F50136 传输进度诚实上报（字节级进度三要素）（0x43d20 段）
//!
//! 锚点：UNX-F50136-J1 进度与实际字节勾稽偏差 ≤1%；判据锚定 体验八。
//!
//! 进度条是最容易被「修饰」的界面：为了让用户看着舒服，后端把 99% 拖很久、
//! 最后一下跳到 100%，或者反过来在数据还没到时先报 100%。这两种都是谎报，
//! 只是方向不同。本模块把进度拆成**字节级三要素**，让每一帧都可对账：
//! * `actual` —— 传输层真实交付的字节数（唯一的事实来源）；
//! * `reported` —— 界面/日志拿到并展示的字节数（只能是 actual 的量化）；
//! * `total` —— 作业声明的总字节数（进度的分母）。
//!
//! 诚实性由三条可断言的性质构成：
//! 1. **不超报**：`reported <= actual`，界面永远不能比事实更超前；
//! 2. **滞后 ≤1%**：`actual - reported <= total/100`，量化误差被钉在 1% 内；
//! 3. **百分比一致**：四舍五入后的百分比与真实百分比相差不超过 1 个百分点。
//!
//! 量化函数 `quantize` 采用「千分位回乘」而不是「总长千分位整除」：
//! 先算 `permille = actual*1000/total`，再用 `permille*total/1000` 还原字节。
//! 前者误差是「千分之一总长」，后者在小总量下会被步长截断放大成几十个百分点
//! （例如 total=1500 时步长截断会把 900 报成 600）。这是本模块最关键的
//! 一处实现选择。
//!
//! 数据结构：
//! * `Meter` —— 进度状态机（Idle/Running/Done）+ 三要素账；
//! * `ProgressLog` —— 样本账（严格递增的 tick 与单调字节，重放可审计）；
//! * `StallWatch` —— 静默窗口看门狗（长时间无字节却「涨进度」即为谎报）；
//! * `Triple` / `Audit` —— 单帧对账值与整段作业的聚合报告。
//!
//! 错误路径：`ProgressError::code()` 返回 `SEG_BASE + n`，段内闭集；全部码值
//! 都在判据或注入核里被真实构造。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43d20 独占 0x20）。
pub const SEG_BASE: u32 = 0x43d20;

pub const E_ZERO_TOTAL: u16 = 1;
pub const E_OVERFLOW: u16 = 2;
pub const E_BACKWARD: u16 = 3;
pub const E_REPORT_BACK: u16 = 4;
pub const E_OVERCLAIM: u16 = 5;
pub const E_LAG: u16 = 6;
pub const E_PERCENT_DRIFT: u16 = 7;
pub const E_EXCEED_TOTAL: u16 = 8;
pub const E_NOT_RUNNING: u16 = 9;
pub const E_ALREADY_RUNNING: u16 = 10;
pub const E_FINISHED: u16 = 11;
pub const E_CAPACITY: u16 = 12;
pub const E_SAMPLE_ORDER: u16 = 13;
pub const E_STALL: u16 = 14;
pub const E_INCOMPLETE: u16 = 15;

/// 千分位量化分母：误差上限是总长的 1/1000。
pub const PERMILLE: u64 = 1_000;

/// 允许使用千分位量化的最小作业量（字节）。
///
/// 小于 1 KB 的作业直接按实际字节上报（零误差）：这类作业没有「进度条」
/// 的意义，量化反而会引入肉眼可见的跳变。
pub const MIN_TOTAL: u64 = 1_000;

/// 样本账容量上限（按 60 KB 级块计算，足够覆盖 4 MB 作业）。
pub const MAX_SAMPLES: usize = 64;

/// 默认静默窗口（tick 数）：超过该窗口没有新字节，就不允许进度继续上涨。
pub const STALL_WINDOW: u32 = 1_000;

/// 进度上报错误闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressError {
    ZeroTotal,
    Overflow,
    Backward,
    ReportBack,
    Overclaim,
    Lag,
    PercentDrift,
    ExceedTotal,
    NotRunning,
    AlreadyRunning,
    Finished,
    Capacity,
    SampleOrder,
    Stall,
    Incomplete,
}

impl ProgressError {
    /// 诊断码：段基址 + 段内序号（闭集，全仓唯一）。
    pub fn code(self) -> u32 {
        let n = match self {
            ProgressError::ZeroTotal => E_ZERO_TOTAL,
            ProgressError::Overflow => E_OVERFLOW,
            ProgressError::Backward => E_BACKWARD,
            ProgressError::ReportBack => E_REPORT_BACK,
            ProgressError::Overclaim => E_OVERCLAIM,
            ProgressError::Lag => E_LAG,
            ProgressError::PercentDrift => E_PERCENT_DRIFT,
            ProgressError::ExceedTotal => E_EXCEED_TOTAL,
            ProgressError::NotRunning => E_NOT_RUNNING,
            ProgressError::AlreadyRunning => E_ALREADY_RUNNING,
            ProgressError::Finished => E_FINISHED,
            ProgressError::Capacity => E_CAPACITY,
            ProgressError::SampleOrder => E_SAMPLE_ORDER,
            ProgressError::Stall => E_STALL,
            ProgressError::Incomplete => E_INCOMPLETE,
        };
        SEG_BASE + n as u32
    }

    /// 一行可读原因（诊断/日志共用）。
    pub fn message(self) -> &'static str {
        match self {
            ProgressError::ZeroTotal => "total byte count is zero",
            ProgressError::Overflow => "delivered byte counter overflowed",
            ProgressError::Backward => "actual bytes went backwards",
            ProgressError::ReportBack => "reported bytes went backwards",
            ProgressError::Overclaim => "reported more bytes than actually delivered",
            ProgressError::Lag => "reported bytes lag actual by more than 1%",
            ProgressError::PercentDrift => "claimed percent differs from reported bytes",
            ProgressError::ExceedTotal => "delivered bytes exceed declared total",
            ProgressError::NotRunning => "operation before begin",
            ProgressError::AlreadyRunning => "begin called twice",
            ProgressError::Finished => "operation after finish",
            ProgressError::Capacity => "sample ledger full",
            ProgressError::SampleOrder => "sample tick is not strictly increasing",
            ProgressError::Stall => "progress claimed across a silent window",
            ProgressError::Incomplete => "finish before all bytes delivered",
        }
    }
}

fn fnv_u64(mut h: u64, v: u64) -> u64 {
    let mut k = 0u32;
    while k < 8 {
        h ^= ((v >> (k * 8)) & 0xff) as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
        k += 1;
    }
    h
}

/// 进度三要素：实际、上报、总量。
///
/// 这是一个**值对象**而不是可变账本：把一帧的三种数字放在一起，
/// 偏差、超报、百分比差全部由它自己算，避免调用方各写一套口径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Triple {
    pub actual: u64,
    pub reported: u64,
    pub total: u64,
}

impl Triple {
    /// 滞后字节数（上报低于实际的部分；超报时按 0 计）。
    pub fn lag(&self) -> u64 {
        self.actual.saturating_sub(self.reported)
    }

    /// 是否超报（上报字节多于实际交付）。
    pub fn overclaim(&self) -> bool {
        self.reported > self.actual
    }

    /// 滞后占比（千分比，超过 1000 表示偏差超过 100%）。
    pub fn deviation_permille(&self) -> u64 {
        if self.total == 0 {
            return 0;
        }
        ((self.lag() as u128 * PERMILLE as u128) / self.total as u128) as u64
    }

    /// 百分比（0..=100，四舍五入前先截断，口径与界面一致）。
    pub fn percent_of(bytes: u64, total: u64) -> u32 {
        if total == 0 {
            return 0;
        }
        let p = ((bytes as u128).saturating_mul(100) / total as u128) as u64;
        if p > 100 {
            100
        } else {
            p as u32
        }
    }

    /// 实际进度百分比。
    pub fn percent_actual(&self) -> u32 {
        Triple::percent_of(self.actual, self.total)
    }

    /// 上报进度百分比。
    pub fn percent_reported(&self) -> u32 {
        Triple::percent_of(self.reported, self.total)
    }

    /// 两个百分比之间的差（个百分点）。
    pub fn percent_gap(&self) -> u32 {
        let a = self.percent_actual();
        let b = self.percent_reported();
        if a > b {
            a - b
        } else {
            b - a
        }
    }

    /// 本帧是否诚实：总量非零、未越总量、未超报、滞后 ≤1%。
    pub fn honest(&self) -> bool {
        self.total > 0
            && self.actual <= self.total
            && self.reported <= self.actual
            && self.lag() <= self.total / 100
    }
}

/// 把实际字节量化为准备上报的字节量。
///
/// 关键点：`permille` 是**千分位**而不是百分比，并且还原时用
/// `permille * total / 1000` 回乘总长——这样误差上限是总长的 1/1000 再加
/// 一个字节的取整，而不是 `total/1000` 步长本身。小总量（< 1 KB）直接
/// 按实际字节上报，避免量化在「几十字节的作业」上变成主要误差。
pub fn quantize(done: u64, total: u64) -> u64 {
    if total == 0 {
        return 0;
    }
    if total < MIN_TOTAL {
        return if done > total { total } else { done };
    }
    let permille = ((done as u128 * PERMILLE as u128) / total as u128) as u64;
    let reported = ((permille as u128 * total as u128) / PERMILLE as u128) as u64;
    if reported > done {
        done
    } else {
        reported
    }
}

/// 进度状态机的阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Running,
    Done,
}

/// 进度状态机：Idle → Running → Done，字节账随状态一起推进。
///
/// 状态机把「先 begin 再 advance/report，满额才能 finish」写进类型里；
/// 所有违反顺序的调用都返回具体错误码，而不是静默吞掉——进度谎报最常见
/// 的来源就是「状态错了但没人报」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Meter {
    phase: Phase,
    total: u64,
    done: u64,
    reported: u64,
    percent: u32,
}

impl Meter {
    /// 新建空闲进度计。
    pub fn new() -> Meter {
        Meter {
            phase: Phase::Idle,
            total: 0,
            done: 0,
            reported: 0,
            percent: 0,
        }
    }

    /// 开始一次作业。总量必须非零；重复开始与结束后开始都必拒。
    pub fn begin(&mut self, total: u64) -> Result<(), ProgressError> {
        match self.phase {
            Phase::Running => return Err(ProgressError::AlreadyRunning),
            Phase::Done => return Err(ProgressError::Finished),
            Phase::Idle => {}
        }
        if total == 0 {
            return Err(ProgressError::ZeroTotal);
        }
        self.phase = Phase::Running;
        self.total = total;
        self.done = 0;
        self.reported = 0;
        self.percent = 0;
        Ok(())
    }

    /// 提交实际交付的字节增量（唯一的事实来源）。
    pub fn advance(&mut self, delta: u64) -> Result<u64, ProgressError> {
        if self.phase != Phase::Running {
            return Err(if self.phase == Phase::Done {
                ProgressError::Finished
            } else {
                ProgressError::NotRunning
            });
        }
        let next = match self.done.checked_add(delta) {
            Some(v) => v,
            None => return Err(ProgressError::Overflow),
        };
        if next > self.total {
            return Err(ProgressError::ExceedTotal);
        }
        self.done = next;
        Ok(next)
    }

    /// 上报展示字节。必须不超过实际、不得回退、滞后不得超过总量的 1%。
    ///
    /// 返回本帧的滞后字节数，便于调用方直接打日志。
    pub fn report(&mut self, bytes: u64) -> Result<u64, ProgressError> {
        if self.phase != Phase::Running {
            return Err(if self.phase == Phase::Done {
                ProgressError::Finished
            } else {
                ProgressError::NotRunning
            });
        }
        if bytes > self.done {
            return Err(ProgressError::Overclaim);
        }
        if bytes < self.reported {
            return Err(ProgressError::ReportBack);
        }
        let lag = self.done - bytes;
        if lag > self.total / 100 {
            return Err(ProgressError::Lag);
        }
        self.reported = bytes;
        Ok(lag)
    }

    /// 上报一个整数百分比。必须与「已上报字节折算的百分比」相差 ≤1 个百分点。
    pub fn claim_percent(&mut self, percent: u32) -> Result<(), ProgressError> {
        if self.phase != Phase::Running {
            return Err(if self.phase == Phase::Done {
                ProgressError::Finished
            } else {
                ProgressError::NotRunning
            });
        }
        if percent > 100 {
            return Err(ProgressError::PercentDrift);
        }
        let want = Triple::percent_of(self.reported, self.total);
        let gap = if percent > want {
            percent - want
        } else {
            want - percent
        };
        if gap > 1 {
            return Err(ProgressError::PercentDrift);
        }
        self.percent = percent;
        Ok(())
    }

    /// 结束作业。实际字节必须已满额，否则是「提前宣布完成」。
    pub fn finish(&mut self) -> Result<Triple, ProgressError> {
        match self.phase {
            Phase::Done => return Err(ProgressError::Finished),
            Phase::Idle => return Err(ProgressError::NotRunning),
            Phase::Running => {}
        }
        if self.done < self.total {
            return Err(ProgressError::Incomplete);
        }
        self.phase = Phase::Done;
        Ok(self.triple())
    }

    /// 当前阶段。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 声明的总字节数。
    pub fn total(&self) -> u64 {
        self.total
    }

    /// 实际交付字节数。
    pub fn done(&self) -> u64 {
        self.done
    }

    /// 已上报字节数。
    pub fn reported(&self) -> u64 {
        self.reported
    }

    /// 最近一次上报的整数百分比。
    pub fn percent(&self) -> u32 {
        self.percent
    }

    /// 当前三要素快照。
    pub fn triple(&self) -> Triple {
        Triple {
            actual: self.done,
            reported: self.reported,
            total: self.total,
        }
    }
}

/// 单帧进度样本（tick 用于顺序复核，done/reported 为字节）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub tick: u32,
    pub done: u64,
    pub reported: u64,
}

/// 进度样本账。
///
/// 账本只接受三类合法样本：tick 严格递增、实际字节单调不减、上报字节单调不减。
/// 任何一条被破坏都拒绝入账——「回退的进度条」本身就是一种谎报。
#[derive(Clone, Debug)]
pub struct ProgressLog {
    samples: Vec<Sample>,
    cap: usize,
}

impl ProgressLog {
    /// 新建样本账（容量夹在 1..=MAX_SAMPLES）。
    pub fn new(cap: usize) -> ProgressLog {
        let c = if cap == 0 {
            1
        } else if cap > MAX_SAMPLES {
            MAX_SAMPLES
        } else {
            cap
        };
        ProgressLog {
            samples: Vec::with_capacity(c),
            cap: c,
        }
    }

    /// 入账一帧。容量、tick、实际、上报四项依次校验。
    pub fn push(&mut self, sample: Sample) -> Result<usize, ProgressError> {
        if self.samples.len() >= self.cap {
            return Err(ProgressError::Capacity);
        }
        if let Some(last) = self.samples.last() {
            if sample.tick <= last.tick {
                return Err(ProgressError::SampleOrder);
            }
            if sample.done < last.done {
                return Err(ProgressError::Backward);
            }
            if sample.reported < last.reported {
                return Err(ProgressError::ReportBack);
            }
        }
        self.samples.push(sample);
        Ok(self.samples.len() - 1)
    }

    /// 已入账样本数。
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// 只读样本切片。
    pub fn samples(&self) -> &[Sample] {
        &self.samples
    }

    /// 样本账摘要（FNV-1a 覆盖 tick/done/reported，用于复跑比对）。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ self.samples.len() as u64;
        let mut i = 0usize;
        while i < self.samples.len() {
            if let Some(s) = self.samples.get(i) {
                h = fnv_u64(h, s.tick as u64);
                h = fnv_u64(h, s.done);
                h = fnv_u64(h, s.reported);
            }
            i += 1;
        }
        h
    }
}

/// 静默窗口看门狗。
///
/// 传输卡住时进度条应当纹丝不动；如果两个样本之间隔了远大于窗口的 tick，
/// 而 done 却涨了，说明上报把「等待」算成了「传输」。零增量的心跳样本
/// 允许穿过窗口（用户需要看到「还活着」），但字节不许动。
#[derive(Clone, Debug)]
pub struct StallWatch {
    window: u32,
    last_tick: u32,
    last_done: u64,
    armed: bool,
}

impl StallWatch {
    /// 用给定窗口（tick 数）新建看门狗。
    pub fn new(window: u32) -> StallWatch {
        StallWatch {
            window,
            last_tick: 0,
            last_done: 0,
            armed: false,
        }
    }

    /// 观察一帧。首帧只用于摆位；之后窗口内涨字节即判静默跳变。
    pub fn observe(&mut self, sample: Sample) -> Result<(), ProgressError> {
        if !self.armed {
            self.armed = true;
            self.last_tick = sample.tick;
            self.last_done = sample.done;
            return Ok(());
        }
        if sample.tick <= self.last_tick {
            return Err(ProgressError::SampleOrder);
        }
        let gap = sample.tick - self.last_tick;
        if gap > self.window && sample.done > self.last_done {
            return Err(ProgressError::Stall);
        }
        self.last_tick = sample.tick;
        self.last_done = sample.done;
        Ok(())
    }

    /// 窗口大小。
    pub fn window(&self) -> u32 {
        self.window
    }
}

/// 整段作业的进度审计报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Audit {
    pub total: u64,
    pub samples: usize,
    pub max_lag: u64,
    pub max_lag_permille: u64,
    pub max_percent_gap: u32,
    pub overclaims: u32,
    pub complete: bool,
    pub final_done: u64,
    pub final_reported: u64,
    pub digest: u64,
}

/// 跑一段「按块交付 → 量化 → 上报 → 百分比申报」的诚实轨迹并审计。
///
/// 轨迹用固定伪随机步长（对 i 取模）而不是运行期随机，保证同一 total
/// 每次得到同一摘要——复跑不一致本身就是谎报的温床。
pub fn run_honest(total: u64) -> Result<Audit, ProgressError> {
    let mut meter = Meter::new();
    meter.begin(total)?;
    let mut log = ProgressLog::new(MAX_SAMPLES);
    let mut max_lag: u64 = 0;
    let mut max_lag_permille: u64 = 0;
    let mut max_percent_gap: u32 = 0;
    let mut overclaims: u32 = 0;

    let mut i = 0u32;
    while meter.done() < total {
        let remaining = total - meter.done();
        let mut delta = 60_000u64 + (i % 11) as u64 * 173;
        if delta > remaining {
            delta = remaining;
        }
        if delta == 0 {
            delta = 1;
        }
        let done = meter.advance(delta)?;
        let reported = quantize(done, total);
        meter.report(reported)?;

        let t = Triple {
            actual: done,
            reported,
            total,
        };
        // 量化函数已经 clamp，这里再做一次独立复核：超报是硬红线。
        if t.overclaim() {
            overclaims = overclaims.saturating_add(1);
        }
        if t.lag() > max_lag {
            max_lag = t.lag();
        }
        if t.deviation_permille() > max_lag_permille {
            max_lag_permille = t.deviation_permille();
        }
        if t.percent_gap() > max_percent_gap {
            max_percent_gap = t.percent_gap();
        }

        let p = Triple::percent_of(reported, total);
        meter.claim_percent(p)?;
        log.push(Sample {
            tick: i,
            done,
            reported,
        })?;
        i += 1;
    }

    let fin = meter.finish()?;
    Ok(Audit {
        total,
        samples: i as usize,
        max_lag,
        max_lag_permille,
        max_percent_gap,
        overclaims,
        complete: fin.actual == total && fin.reported == total,
        final_done: fin.actual,
        final_reported: fin.reported,
        digest: log.digest(),
    })
}

/// 诊断渲染：一行说清「总量、样本数、最大滞后、最大百分点差、超报数」。
pub fn render(a: &Audit) -> String {
    format!(
        "progress total={} samples={} lag={} ppm={} gap={} over={} complete={}",
        a.total,
        a.samples,
        a.max_lag,
        a.max_lag_permille,
        a.max_percent_gap,
        a.overclaims,
        a.complete
    )
}

// ---------------------------------------------------------------------------
// 注入核：十类坏进度必须被显式拒绝
// ---------------------------------------------------------------------------

pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 10usize;

    // 1 总量为零
    if Meter::new().begin(0).err() == Some(ProgressError::ZeroTotal) {
        caught += 1;
    }

    // 2 越总量
    let mut over = Meter::new();
    let _ = over.begin(1_000);
    if over.advance(1_001).err() == Some(ProgressError::ExceedTotal) {
        caught += 1;
    }

    // 3 计数器溢出（总量取满 u64）
    let mut big = Meter::new();
    let _ = big.begin(u64::MAX);
    let filled = big.advance(u64::MAX);
    if filled.is_ok() && big.advance(1).err() == Some(ProgressError::Overflow) {
        caught += 1;
    }

    // 4 超报：实际 500 却报 501
    let mut o = Meter::new();
    let _ = o.begin(1_000);
    let _ = o.advance(500);
    if o.report(501).err() == Some(ProgressError::Overclaim) {
        caught += 1;
    }

    // 5 滞后：实际 500 只报 400（差 100 > 10）
    let mut l = Meter::new();
    let _ = l.begin(1_000);
    let _ = l.advance(500);
    if l.report(400).err() == Some(ProgressError::Lag) {
        caught += 1;
    }

    // 6 上报回退：先报 8900 再报 8800
    let mut r = Meter::new();
    let _ = r.begin(10_000);
    let _ = r.advance(9_000);
    let first = r.report(8_900);
    if first.is_ok() && r.report(8_800).err() == Some(ProgressError::ReportBack) {
        caught += 1;
    }

    // 7 百分比漂移：上报 0 字节却宣称 100%
    let mut p = Meter::new();
    let _ = p.begin(1_000);
    let _ = p.advance(10);
    let _ = p.report(0);
    if p.claim_percent(100).err() == Some(ProgressError::PercentDrift) {
        caught += 1;
    }

    // 8 样本 tick 不递增
    let mut log = ProgressLog::new(8);
    let _ = log.push(Sample {
        tick: 5,
        done: 100,
        reported: 100,
    });
    let dup = log.push(Sample {
        tick: 5,
        done: 100,
        reported: 100,
    });
    if dup.err() == Some(ProgressError::SampleOrder) {
        caught += 1;
    }

    // 9 实际字节回退
    let mut back = ProgressLog::new(8);
    let _ = back.push(Sample {
        tick: 1,
        done: 100,
        reported: 100,
    });
    let rev = back.push(Sample {
        tick: 2,
        done: 99,
        reported: 100,
    });
    if rev.err() == Some(ProgressError::Backward) {
        caught += 1;
    }

    // 10 静默窗口内涨进度
    let mut watch = StallWatch::new(100);
    let _ = watch.observe(Sample {
        tick: 0,
        done: 0,
        reported: 0,
    });
    let jump = watch.observe(Sample {
        tick: 500,
        done: 65_536,
        reported: 65_536,
    });
    if jump.err() == Some(ProgressError::Stall) {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50136_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50136");
    let totals = [1_048_576u64, 131_072, 65_536, 4_096, 1_000];

    // J1-1 勾稽偏差 ≤1%：五种总量下逐帧滞后都不超过总量的百分之一
    let mut all_ok = true;
    let mut worst_ppm = 0u64;
    let mut worst_lag = 0u64;
    let mut i = 0usize;
    while i < totals.len() {
        let t = match totals.get(i) {
            Some(v) => *v,
            None => 0,
        };
        match run_honest(t) {
            Ok(a) => {
                if a.max_lag_permille > worst_ppm {
                    worst_ppm = a.max_lag_permille;
                }
                if a.max_lag > worst_lag {
                    worst_lag = a.max_lag;
                }
                if !(a.complete
                    && a.max_lag <= t / 100
                    && a.max_lag_permille <= 10)
                {
                    all_ok = false;
                }
            }
            Err(_) => all_ok = false,
        }
        i += 1;
    }
    let det = format!("worst_ppm={} worst_lag={}", worst_ppm, worst_lag);
    s.add_dyn("50136-J1-勾稽偏差≤1%", all_ok, &det);

    // J1-2 不超报：任何一帧 reported ≤ actual，整段零次超报
    let mut no_over = true;
    let mut k = 0usize;
    while k < totals.len() {
        let t = match totals.get(k) {
            Some(v) => *v,
            None => 0,
        };
        match run_honest(t) {
            Ok(a) => {
                let fin = Triple {
                    actual: a.final_done,
                    reported: a.final_reported,
                    total: t,
                };
                if a.overclaims != 0 || !fin.honest() || a.final_reported > a.final_done {
                    no_over = false;
                }
            }
            Err(_) => no_over = false,
        }
        k += 1;
    }
    s.add(
        "50136-J1-不超报",
        no_over,
        if no_over { "" } else { "上报字节超过实际交付" },
    );

    // J1-3 百分比一致：上报百分比与真实百分比相差 ≤1 个百分点
    let mut gap_ok = true;
    let mut worst_gap = 0u32;
    let mut g = 0usize;
    while g < totals.len() {
        let t = match totals.get(g) {
            Some(v) => *v,
            None => 0,
        };
        match run_honest(t) {
            Ok(a) => {
                if a.max_percent_gap > worst_gap {
                    worst_gap = a.max_percent_gap;
                }
                if a.max_percent_gap > 1 {
                    gap_ok = false;
                }
            }
            Err(_) => gap_ok = false,
        }
        g += 1;
    }
    let det = format!("worst_gap={}pp", worst_gap);
    s.add_dyn("50136-J1-百分比一致", gap_ok, &det);

    // J1-4 复跑确定：同参数两次得到同一样本摘要（digest 非零）
    let d1 = run_honest(1_048_576).ok().map(|a| a.digest);
    let d2 = run_honest(1_048_576).ok().map(|a| a.digest);
    let shown1 = match d1 {
        Some(v) => v,
        None => 0,
    };
    let det = format!("d1={} d2={}", shown1, shown1);
    let ok = d1.is_some() && d1 == d2 && d1 != Some(0);
    s.add_dyn("50136-J1-复跑确定", ok, &det);

    // J1-5 末端满额：交付满额后上报、百分比、三要素同时到 100%
    let mut m = Meter::new();
    let b = m.begin(1_000);
    let a1 = m.advance(1_000);
    let r1 = m.report(1_000);
    let c1 = m.claim_percent(100);
    let fin = m.finish();
    let ok = b.is_ok()
        && a1 == Ok(1_000)
        && r1 == Ok(0)
        && c1.is_ok()
        && fin.ok()
            == Some(Triple {
                actual: 1_000,
                reported: 1_000,
                total: 1_000,
            })
        && m.percent() == 100
        && m.phase() == Phase::Done
        && m.triple().honest();
    let det = format!("phase={:?} pct={}", m.phase(), m.percent());
    s.add_dyn("50136-J1-末端满额", ok, &det);

    // J1-6 状态机闭集：未开始/重复开始/结束后操作都必拒
    let mut st = Meter::new();
    let early = st.advance(1);
    let begin = st.begin(1_000);
    let twice = st.begin(1_000);
    let _ = st.advance(1_000);
    let done = st.finish();
    let after = st.advance(1);
    let again = st.finish();
    let reboot = st.begin(1);
    let ok = early.err() == Some(ProgressError::NotRunning)
        && begin.is_ok()
        && twice.err() == Some(ProgressError::AlreadyRunning)
        && done.is_ok()
        && after.err() == Some(ProgressError::Finished)
        && again.err() == Some(ProgressError::Finished)
        && reboot.err() == Some(ProgressError::Finished);
    s.add(
        "50136-J1-状态机",
        ok,
        if ok { "" } else { "进度状态机可被绕过" },
    );

    // J1-7 越总量必拒：且拒绝后实际字节不被污染
    let mut ex = Meter::new();
    let _ = ex.begin(1_000);
    let over = ex.advance(1_001);
    let untouched = ex.done() == 0;
    let ok = over.err() == Some(ProgressError::ExceedTotal) && untouched && ex.advance(1_000).is_ok();
    s.add(
        "50136-J1-越界必拒",
        ok,
        if ok { "" } else { "越总量后字节账被改写" },
    );

    // J1-8 溢出必杀：u64 满量程再 +1
    let mut big = Meter::new();
    let _ = big.begin(u64::MAX);
    let full = big.advance(u64::MAX);
    let ovf = big.advance(1);
    let ok = full == Ok(u64::MAX) && ovf.err() == Some(ProgressError::Overflow) && big.done() == u64::MAX;
    s.add(
        "50136-J1-溢出必杀",
        ok,
        if ok { "" } else { "计数器溢出未被拦截" },
    );

    // J1-9 滞后必拒：恰好 1% 放行，超过 1% 必拒
    let mut lag = Meter::new();
    let _ = lag.begin(10_000);
    let _ = lag.advance(5_000);
    let zero = lag.report(0);
    let edge = lag.report(4_900);
    let ok = zero.err() == Some(ProgressError::Lag)
        && edge == Ok(100)
        && lag.triple().lag() == 100;
    let det = format!("zero={:?} edge={:?}", zero.err(), edge.ok());
    s.add_dyn("50136-J1-滞后必拒", ok, &det);

    // J1-10 超报与回退必拒：边界（报满实际）放行
    let mut ov = Meter::new();
    let _ = ov.begin(10_000);
    let _ = ov.advance(9_000);
    let exact = ov.report(9_000);
    let high = ov.report(9_001);
    let low = ov.report(8_999);
    let ok = exact == Ok(0)
        && high.err() == Some(ProgressError::Overclaim)
        && low.err() == Some(ProgressError::ReportBack)
        && ov.reported() == 9_000;
    s.add(
        "50136-J1-超报回退",
        ok,
        if ok { "" } else { "超报或回退被收下" },
    );

    // J1-11 百分比漂移必拒：±1 个百分点放行，超过必拒
    let mut pc = Meter::new();
    let _ = pc.begin(1_000);
    let _ = pc.advance(10);
    let _ = pc.report(0);
    let want = Triple::percent_of(0, 1_000);
    let drift = pc.claim_percent(100);
    let near = pc.claim_percent(want + 1);
    let far = pc.claim_percent(want + 3);
    let ok = drift.err() == Some(ProgressError::PercentDrift)
        && near.is_ok()
        && far.err() == Some(ProgressError::PercentDrift)
        && pc.percent() == want + 1;
    s.add(
        "50136-J1-百分比漂移",
        ok,
        if ok { "" } else { "百分比与字节脱钩" },
    );

    // J1-12 样本纪律：容量、tick 序、实际回退、上报回退各有独立码
    let mut cap = ProgressLog::new(1);
    let first = cap.push(Sample {
        tick: 0,
        done: 0,
        reported: 0,
    });
    let full = cap.push(Sample {
        tick: 1,
        done: 1,
        reported: 1,
    });
    let mut ord = ProgressLog::new(8);
    let _ = ord.push(Sample {
        tick: 5,
        done: 100,
        reported: 100,
    });
    let dup = ord.push(Sample {
        tick: 5,
        done: 100,
        reported: 100,
    });
    let back = ord.push(Sample {
        tick: 6,
        done: 99,
        reported: 100,
    });
    let rep = ord.push(Sample {
        tick: 7,
        done: 100,
        reported: 99,
    });
    let ok = first.is_ok()
        && full.err() == Some(ProgressError::Capacity)
        && dup.err() == Some(ProgressError::SampleOrder)
        && back.err() == Some(ProgressError::Backward)
        && rep.err() == Some(ProgressError::ReportBack)
        && ord.len() == 1;
    let det = format!("len={} digest={}", ord.len(), ord.digest());
    s.add_dyn("50136-J1-样本纪律", ok, &det);

    // J1-13 静默跳变必拒：窗口内零增量心跳放行，涨字节必拒
    let mut watch = StallWatch::new(100);
    let arm = watch.observe(Sample {
        tick: 0,
        done: 0,
        reported: 0,
    });
    let beat = watch.observe(Sample {
        tick: 150,
        done: 0,
        reported: 0,
    });
    let step = watch.observe(Sample {
        tick: 160,
        done: 4_096,
        reported: 4_096,
    });
    let jump = watch.observe(Sample {
        tick: 500,
        done: 8_192,
        reported: 8_192,
    });
    let ok = arm.is_ok()
        && beat.is_ok()
        && step.is_ok()
        && jump.err() == Some(ProgressError::Stall);
    let det = format!("beat={:?} jump={:?}", beat.err(), jump.err());
    s.add_dyn("50136-J1-静默跳变", ok, &det);

    // J1-14 未完成必拒：半途 finish 必拒，补齐后才允许封账
    let mut inc = Meter::new();
    let _ = inc.begin(1_000);
    let _ = inc.advance(500);
    let early_fin = inc.finish();
    let _ = inc.advance(500);
    let _ = inc.report(1_000);
    let late_fin = inc.finish();
    let ok = early_fin.err() == Some(ProgressError::Incomplete) && late_fin.is_ok();
    s.add(
        "50136-J1-未完成必拒",
        ok,
        if ok { "" } else { "半途作业被宣布完成" },
    );

    // J1-15 错误码闭集：15 个码互异且全部落在段内
    let codes = [
        ProgressError::ZeroTotal,
        ProgressError::Overflow,
        ProgressError::Backward,
        ProgressError::ReportBack,
        ProgressError::Overclaim,
        ProgressError::Lag,
        ProgressError::PercentDrift,
        ProgressError::ExceedTotal,
        ProgressError::NotRunning,
        ProgressError::AlreadyRunning,
        ProgressError::Finished,
        ProgressError::Capacity,
        ProgressError::SampleOrder,
        ProgressError::Stall,
        ProgressError::Incomplete,
    ];
    let mut distinct = true;
    let mut in_seg = true;
    let mut x = 0usize;
    while x < codes.len() {
        let cx = codes[x].code();
        if cx < SEG_BASE + 1 || cx > SEG_BASE + 15 {
            in_seg = false;
        }
        let mut y = x + 1;
        while y < codes.len() {
            if codes[y].code() == cx {
                distinct = false;
            }
            y += 1;
        }
        x += 1;
    }
    let ok = distinct && in_seg;
    s.add(
        "50136-J1-错误码闭集",
        ok,
        if ok { "" } else { "码值重复或越段" },
    );

    // J1-16 注入核：十类坏进度全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50136-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50136_checks_green() {
        let set = run_unx50136_checks();
        assert!(set.all_passed(), "UNX-F50136 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50136_honest_within_one_percent() {
        let r = run_honest(1_048_576);
        assert!(r.is_ok());
        if let Ok(a) = r {
            assert!(a.complete);
            assert_eq!(a.overclaims, 0);
            assert!(a.max_lag <= a.total / 100);
            assert!(a.max_lag_permille <= 10);
            assert!(a.max_percent_gap <= 1);
        }
    }

    #[test]
    fn unx50136_quantize_never_overclaims() {
        let mut done = 0u64;
        while done < 100_000 {
            let q = quantize(done, 100_000);
            assert!(q <= done);
            assert!(done - q <= 100_000 / 100);
            done += 997;
        }
    }

    #[test]
    fn unx50136_overclaim_rejected() {
        let mut m = Meter::new();
        assert!(m.begin(1_000).is_ok());
        assert!(m.advance(500).is_ok());
        assert_eq!(m.report(501).err(), Some(ProgressError::Overclaim));
        assert_eq!(m.reported(), 0);
    }

    #[test]
    fn unx50136_stall_rejected() {
        let mut w = StallWatch::new(100);
        assert!(w
            .observe(Sample {
                tick: 0,
                done: 0,
                reported: 0,
            })
            .is_ok());
        assert_eq!(
            w.observe(Sample {
                tick: 500,
                done: 4_096,
                reported: 4_096,
            })
            .err(),
            Some(ProgressError::Stall)
        );
    }
}
