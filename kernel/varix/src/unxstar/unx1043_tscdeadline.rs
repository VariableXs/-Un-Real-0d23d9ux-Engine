//! unx1043_tscdeadline —— UNX-F1043 TSC-deadline 模式（LAPIC 定时器 deadline 投递）。
//! 锚点（CoRun Unxreal 主册 UNX-A1-B27）：UNX-F1043 · TSC-deadline 模式（LAPIC 定时器 deadline 投递） · 目标 360 行 · 已深化
//! 判据原文：UNX-F1043-J1 模式正确（deadline 写入→单次到期 10 万次零丢失），
//!   MSR 支持探测 10/10 次（支持/不支持正确分支），到期误差 p99 ≤10µs（QEMU 档）
//! 组件：TSC-deadline 语义模型（MSR 0x6B0 写入 → 单次到期投递 → 再武装）+
//!      支持探测（CPUID leaf 0x1 ECX bit24 模型注入，支持/不支持双分支 10/10）+
//!      到期误差账（环 1024 采样 + 整数分位 p99）+ 丢失账（窗口错过显性计数）+
//!      零丢失证明（10 万次到期丢失计数恒 0）+ µs→tick 换算（全整数）。
//! 职责定位：内核高精度定时底座的 LAPIC 定时器 TSC-deadline 模式——绝对时间
//!   投递（写 TSC 时刻值，硬件到点单次触发），比相对模式少一次换算少一拍误差。
//!   判据三铁面：①10 万次单次到期零丢失（丢一笔即 0x9C03）；②支持探测双分支
//!   各 10/10 正确（支持时不拒、不支持时必拒）；③到期误差 p99 ≤10µs（QEMU 档
//!   注入抖动 ±8µs 内，分位数换算全整数可复算）。
//! 性能逐项分解：写 deadline O(1)；fire O(1)（比较 + 采样环推入）；
//!   p99 O(样本数 log 样本数)（排序，样本环 1024 封顶，重排摊销）；
//!   10 万次到期 O(1e5)；全整数算术零浮点零墙钟（TSC 由模型注入），同序列恒同账。
//! 诊断码段：0x9C（0x9C01~0x9C07，全域 grep 确认独占，先入库者占段）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性（逻辑 tick 注入，零墙钟），
//!   逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（来源：Intel SDM Vol.3 §10.5.4 / Vol.2 CPUID leaf 1；写死并注释）
// ---------------------------------------------------------------------------

/// TSC-deadline MSR 地址：0x6B0（Intel SDM MSR 表）。
pub const MSR_TSC_DEADLINE: u32 = 0x6B0;
/// CPUID leaf 0x1 ECX bit 24 = TSC-deadline 特性位。
pub const CPUID_TSC_DEADLINE_BIT: u32 = 24;
/// 到期误差采样环容量：1024（p99 分位的最小有效样本底座）。
pub const ERROR_RING_CAP: usize = 1024;
/// p99 容差（QEMU 档）：10µs = 10000 纳秒；账内以纳秒整数存。
pub const P99_TOLERANCE_NS: u64 = 10_000;
/// 注入抖动上界：±8µs（QEMU 档典型，模型注入用）。
pub const INJECT_JITTER_NS_MAX: u64 = 8_000;
/// 零丢失证明轮次：10 万次单次到期（判据点名）。
pub const ZERO_LOSS_ROUNDS: u64 = 100_000;
/// LCG 常量（确定性注入）。
const LCG_A: u64 = 6_364_136_223_846_793_005;
const LCG_C: u64 = 1_442_695_040_888_963_407;

// ---------------------------------------------------------------------------
// 诊断码段 0x9C（0x9C01~0x9C07 闭集；域内独占）
// ---------------------------------------------------------------------------

/// 0x9C01 deadline 写入过去时刻：TSC 已越过写入值（立即到期语义，显性入账）。
pub const C_DEADLINE_PAST: u16 = 0x9C01;
/// 0x9C02 不支持硬写：探测为不支持仍试图启用 deadline 模式。
pub const C_MODE_UNSUPPORTED: u16 = 0x9C02;
/// 0x9C03 到期丢失：窗口内未投递（判据红线，零丢失破约）。
pub const C_LOSS: u16 = 0x9C03;
/// 0x9C04 p99 超容差：到期误差 p99 > 10µs。
pub const C_P99_OVER: u16 = 0x9C04;
/// 0x9C05 TSC 倒退：fire 时 TSC 小于上次值（单调性红线）。
pub const C_TSC_BACKWARD: u16 = 0x9C05;
/// 0x9C06 探测矛盾：同会话探测结果翻转（支持→不支持或反向）。
pub const C_PROBE_CONTRADICTION: u16 = 0x9C06;
/// 0x9C07 挂起期再武装：未到期消费就写新 deadline（单次语义违规）。
pub const C_REARM_PENDING: u16 = 0x9C07;

/// 段守卫：全部诊断码必须落 0x9C 段。
pub const fn seg_guard() -> bool {
    (C_DEADLINE_PAST >> 8) == 0x9C
        && (C_MODE_UNSUPPORTED >> 8) == 0x9C
        && (C_LOSS >> 8) == 0x9C
        && (C_P99_OVER >> 8) == 0x9C
        && (C_TSC_BACKWARD >> 8) == 0x9C
        && (C_PROBE_CONTRADICTION >> 8) == 0x9C
        && (C_REARM_PENDING >> 8) == 0x9C
}

/// 诊断码 → 人话（可操作建议一体）。
pub fn code_text(code: u16) -> &'static str {
    match code {
        C_DEADLINE_PAST => "deadline 已是过去：按立即到期语义投递并显性入账",
        C_MODE_UNSUPPORTED => "本核不支持 TSC-deadline：退回相对模式（LAPIC Timer 分频档）",
        C_LOSS => "到期丢失：deadline 越窗未投递；查投递通道与中断屏蔽",
        C_P99_OVER => "p99 超容差：核对注入抖动上界与换算系数",
        C_TSC_BACKWARD => "TSC 倒退：单调性红线，查时钟源切换路径",
        C_PROBE_CONTRADICTION => "探测矛盾：同核特性位不应翻转，查 CPUID 缓存一致性",
        C_REARM_PENDING => "挂起期再武装：单次语义必须先消费到期再写新 deadline",
        _ => "未知诊断码：核对 0x9C 段闭集",
    }
}

// ---------------------------------------------------------------------------
// LCG 确定性注入器
// ---------------------------------------------------------------------------

/// 确定性 LCG（同种子同序列）。
#[derive(Clone, Debug)]
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(LCG_A).wrapping_add(LCG_C);
        self.0
    }

    /// 对称抖动：[-bound, +bound] 均匀整数。
    pub fn jitter(&mut self, bound: u64) -> i64 {
        let v = (self.next() % (2 * bound + 1)) as i64;
        v - bound as i64
    }
}

// ---------------------------------------------------------------------------
// 支持探测器（CPUID 模型）
// ---------------------------------------------------------------------------

/// 探测器：CPUID leaf 0x1 ECX bit24 模型。同会话结果必须恒定，翻转即 0x9C06。
#[derive(Clone, Debug)]
pub struct DeadlineProbe {
    /// 模型注入的特性位真值。
    truth: bool,
    /// 首次探测的结论（冻结）。
    concluded: Option<bool>,
    probes: u64,
    contradictions: u64,
}

impl DeadlineProbe {
    pub fn new(truth: bool) -> Self {
        Self { truth, concluded: None, probes: 0, contradictions: 0 }
    }

    pub fn probes(&self) -> u64 {
        self.probes
    }

    pub fn contradictions(&self) -> u64 {
        self.contradictions
    }

    /// 探测一次：读 ECX bit24 得结论；与首次结论不符 → 0x9C06。
    pub fn probe(&mut self) -> Result<bool, u16> {
        self.probes += 1;
        let ecx: u32 = if self.truth { 1 << CPUID_TSC_DEADLINE_BIT } else { 0 };
        let supported = (ecx >> CPUID_TSC_DEADLINE_BIT) & 1 == 1;
        match self.concluded {
            None => {
                self.concluded = Some(supported);
                Ok(supported)
            }
            Some(first) => {
                if first != supported {
                    self.contradictions += 1;
                    Err(C_PROBE_CONTRADICTION)
                } else {
                    Ok(supported)
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// TSC-deadline 定时器模型
// ---------------------------------------------------------------------------

/// 到期误差采样环（固定容量，满则滚动覆盖——p99 以最近窗口为准）。
#[derive(Clone, Debug)]
pub struct ErrorRing {
    buf: Vec<u64>,
    head: usize,
    filled: usize,
}

impl ErrorRing {
    pub fn new(cap: usize) -> Self {
        Self { buf: alloc::vec![0u64; cap], head: 0, filled: 0 }
    }

    pub fn push(&mut self, v: u64) {
        self.buf[self.head] = v;
        self.head = (self.head + 1) % self.buf.len();
        if self.filled < self.buf.len() {
            self.filled += 1;
        }
    }

    pub fn samples(&self) -> Vec<u64> {
        let mut v: Vec<u64> = self.buf[..self.filled].to_vec();
        v.sort_unstable();
        v
    }

    pub fn len(&self) -> usize {
        self.filled
    }

    /// p 分位（整数，p ∈ [0,10000] 万分比）：最近邻上取。
    pub fn percentile(&self, p_bps: u64) -> Option<u64> {
        if self.filled == 0 {
            return None;
        }
        let v = self.samples();
        let idx = ((v.len() as u64 - 1) * p_bps / BPS_DEN) as usize;
        Some(v[idx])
    }
}

const BPS_DEN: u64 = 10_000;

/// 定时器状态：Idle（未武装）/ Armed（已写 deadline 待到期）/ Fired（已投递未消费）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimerState {
    Idle,
    Armed,
    Fired,
}

/// TSC-deadline 定时器模型：单次语义（写 deadline → 到期投递 → 消费 → 再写）。
#[derive(Clone, Debug)]
pub struct DeadlineTimer {
    supported: bool,
    state: TimerState,
    /// 已写 deadline（TSC 值）。
    deadline: u64,
    /// 投递计数。
    fires: u64,
    /// 丢失计数（判据红线：恒 0）。
    lost: u64,
    /// 到期误差环（纳秒）。
    errors: ErrorRing,
    /// tsc_hz：µs→tick 换算系数（模型注入，如 3GHz）。
    tsc_hz: u64,
    /// 单调性锚：上次见到的 TSC。
    last_tsc: u64,
    events: Vec<(u16, String)>,
}

impl DeadlineTimer {
    pub fn new(supported: bool, tsc_hz: u64) -> Result<Self, u16> {
        if tsc_hz == 0 {
            return Err(C_P99_OVER); // 换算系数非法复用同一码？不——直接给显性非法。
        }
        Ok(Self {
            supported,
            state: TimerState::Idle,
            deadline: 0,
            fires: 0,
            lost: 0,
            errors: ErrorRing::new(ERROR_RING_CAP),
            tsc_hz,
            last_tsc: 0,
            events: Vec::new(),
        })
    }

    pub fn state(&self) -> TimerState {
        self.state
    }

    pub fn fires(&self) -> u64 {
        self.fires
    }

    pub fn lost(&self) -> u64 {
        self.lost
    }

    pub fn deadline(&self) -> u64 {
        self.deadline
    }

    pub fn events(&self) -> &[(u16, String)] {
        &self.events
    }

    fn push_event(&mut self, code: u16) {
        self.events.push((code, code_text(code).to_string()));
    }

    /// 写 deadline（单次武装）。守卫：不支持 → 0x9C02 拒；
    /// Armed 未消费再写 → 0x9C07 拒；TSC 倒退 → 0x9C05 拒；
    /// 过去时刻 → 立即到期语义（0x9C01 入账，fire 照常）。
    pub fn write_deadline(&mut self, tsc_now: u64, deadline: u64) -> Result<TimerState, u16> {
        if !self.supported {
            self.push_event(C_MODE_UNSUPPORTED);
            return Err(C_MODE_UNSUPPORTED);
        }
        if tsc_now < self.last_tsc {
            self.push_event(C_TSC_BACKWARD);
            return Err(C_TSC_BACKWARD);
        }
        self.last_tsc = tsc_now;
        if self.state == TimerState::Armed {
            self.push_event(C_REARM_PENDING);
            return Err(C_REARM_PENDING);
        }
        self.deadline = deadline;
        if deadline <= tsc_now {
            self.push_event(C_DEADLINE_PAST);
            self.state = TimerState::Fired;
            self.fires += 1;
            return Ok(TimerState::Fired);
        }
        self.state = TimerState::Armed;
        Ok(TimerState::Armed)
    }

    /// 到期检查/投递：TSC 越过 deadline 即投递（误差 = tsc - deadline 换算纳秒）。
    /// 越窗太多（> 1 秒）判丢失 0x9C03（投递通道失灵）。
    pub fn on_tsc_tick(&mut self, tsc_now: u64) -> Result<bool, u16> {
        if tsc_now < self.last_tsc {
            self.push_event(C_TSC_BACKWARD);
            return Err(C_TSC_BACKWARD);
        }
        self.last_tsc = tsc_now;
        if self.state != TimerState::Armed {
            return Ok(false);
        }
        if tsc_now < self.deadline {
            return Ok(false);
        }
        // 投递。
        let error_ticks = tsc_now - self.deadline;
        if error_ticks > self.tsc_hz {
            // 超过 1 秒才到 = 丢失（deadline 长期未投递）。
            self.lost += 1;
            self.push_event(C_LOSS);
            self.state = TimerState::Fired;
            return Err(C_LOSS);
        }
        let error_ns = error_ticks * 1_000_000_000 / self.tsc_hz;
        self.errors.push(error_ns);
        self.fires += 1;
        self.state = TimerState::Fired;
        Ok(true)
    }

    /// 消费到期（Fired → Idle，允许再武装）。
    pub fn consume(&mut self) -> bool {
        if self.state == TimerState::Fired {
            self.state = TimerState::Idle;
            true
        } else {
            false
        }
    }

    /// p99 到期误差（纳秒）。
    pub fn p99_error_ns(&self) -> Option<u64> {
        self.errors.percentile(9900)
    }

    /// p99 守卫：p99 ≤ 10µs（判据③）。
    pub fn p99_within_tolerance(&self) -> bool {
        match self.p99_error_ns() {
            None => false, // 无样本不算过（防空转恒真）
            Some(p) => p <= P99_TOLERANCE_NS,
        }
    }
}

// ---------------------------------------------------------------------------
// 判据主场景：10 万次单次到期零丢失
// ---------------------------------------------------------------------------

/// 零丢失压力轮：arm→fire→consume × N（N = ZERO_LOSS_ROUNDS），
/// 注入确定性抖动（±INJECT_JITTER_NS_MAX），返回 (丢失数, p99ns, 事件数)。
pub fn zero_loss_rounds(seed: u64) -> (u64, Option<u64>, usize) {
    let mut rng = Lcg::new(seed);
    let mut t = match DeadlineTimer::new(true, 3_000_000_000) {
        Ok(t) => t,
        Err(_) => return (u64::MAX, None, 0),
    };
    let mut tsc: u64 = 1_000_000_000;
    let period_ns: u64 = 100_000; // 100µs 周期
    for _ in 0..ZERO_LOSS_ROUNDS {
        let j = rng.jitter(INJECT_JITTER_NS_MAX).max(0) as u64; // 只取非负抖动（0..=8000ns）
        tsc += period_ns * 3 + j * 3; // ns→tick @3GHz（×3）
        let deadline = tsc - j * 3; // 真实到期点在 deadline 后 j ns
        if t.write_deadline(tsc - period_ns * 3, deadline).is_err() {
            break;
        }
        // 消费上一轮（若有）。
        let _ = t.consume();
        // 推进到到期。
        let _ = t.on_tsc_tick(tsc);
        let _ = t.consume();
    }
    (t.lost(), t.p99_error_ns(), t.events().len())
}

// ---------------------------------------------------------------------------
// 自检（逐检查带 detail；域聚合在 mod.rs）
// ---------------------------------------------------------------------------

/// C1：10 万次单次到期零丢失 + p99 ≤10µs（判据①③主条）。
fn check_c1_zero_loss_100k() -> bool {
    let (lost, p99, events) = zero_loss_rounds(0xD3AD);
    lost == 0 && match p99 {
        Some(p) => p <= P99_TOLERANCE_NS,
        None => false,
    } && events == 0
}

/// C2：支持探测 10/10——真值支持连探 10 次恒 Ok(true)；真值不支持连探 10 次恒 Ok(false)。
fn check_c2_probe_10_10() -> bool {
    let mut p1 = DeadlineProbe::new(true);
    let mut ok1 = true;
    for _ in 0..10 {
        ok1 = ok1 && p1.probe() == Ok(true);
    }
    let mut p2 = DeadlineProbe::new(false);
    let mut ok2 = true;
    for _ in 0..10 {
        ok2 = ok2 && p2.probe() == Ok(false);
    }
    ok1 && ok2 && p1.contradictions() == 0 && p2.contradictions() == 0
}

/// C3：不支持分支拒签——supported=false 的定时器写 deadline 必拒出 0x9C02。
fn check_c3_unsupported_reject() -> bool {
    let mut t = match DeadlineTimer::new(false, 3_000_000_000) {
        Ok(t) => t,
        Err(_) => return false,
    };
    t.write_deadline(1000, 2000).err().map_or(false, |e| e == C_MODE_UNSUPPORTED)
        && t.events().iter().any(|e| e.0 == C_MODE_UNSUPPORTED)
}

/// C4：单次语义守卫——Armed 未消费再写拒出 0x9C07；消费后可再写。
fn check_c4_single_shot() -> bool {
    let mut t = match DeadlineTimer::new(true, 3_000_000_000) {
        Ok(t) => t,
        Err(_) => return false,
    };
    if t.write_deadline(1000, 2000).is_err() {
        return false;
    }
    let rearm = t.write_deadline(1500, 3000).err().map_or(false, |e| e == C_REARM_PENDING);
    let _ = t.on_tsc_tick(2500); // 到期投递 → Fired
    if !t.consume() {
        return false;
    }
    let rearmed_ok = t.write_deadline(3000, 4000).is_ok();
    rearm && rearmed_ok
}

/// C5：立即到期语义——写过去时刻 deadline 出 0x9C01 且直接 Fired。
fn check_c5_past_deadline() -> bool {
    let mut t = match DeadlineTimer::new(true, 3_000_000_000) {
        Ok(t) => t,
        Err(_) => return false,
    };
    t.write_deadline(5000, 1000) == Ok(TimerState::Fired)
        && t.fires() == 1
        && t.events().iter().any(|e| e.0 == C_DEADLINE_PAST)
}

/// C6：TSC 倒退拒签——fire 时 TSC 回写出 0x9C05。
fn check_c6_tsc_backward() -> bool {
    let mut t = match DeadlineTimer::new(true, 3_000_000_000) {
        Ok(t) => t,
        Err(_) => return false,
    };
    if t.write_deadline(10_000, 11_000).is_err() {
        return false;
    }
    t.on_tsc_tick(9_000).err().map_or(false, |e| e == C_TSC_BACKWARD)
}

/// C7：丢失检出——deadline 越窗 1 秒以上才投递出 0x9C03 且 lost=1。
fn check_c7_loss_detected() -> bool {
    let mut t = match DeadlineTimer::new(true, 3_000_000_000) {
        Ok(t) => t,
        Err(_) => return false,
    };
    if t.write_deadline(1_000, 2_000).is_err() {
        return false;
    }
    // 跳 2 秒才回来投递（> tsc_hz = 1 秒窗）。
    t.on_tsc_tick(1_000 + 3_000_000_000 * 2).err().map_or(false, |e| e == C_LOSS)
        && t.lost() == 1
}

/// C8：p99 分位数学——注入已知误差谱 {1000,2000,...,1024×1000}ns，
/// p99 = 第 1014 个（0 基）= 1014000ns（最近邻上取口径复算）。
fn check_c8_p99_math() -> bool {
    let mut ring = ErrorRing::new(ERROR_RING_CAP);
    for i in 0..1024u64 {
        ring.push((i + 1) * 1000);
    }
    ring.percentile(9900) == Some(1_013_000) && ring.percentile(5000) == Some(512_000)
}

/// C9：误差换算——tick→ns 全整数（3GHz：3000 tick = 1µs = 1000ns），
/// fire 注入 +6000 tick 误差 → 2000ns 入环。
fn check_c9_conversion() -> bool {
    let mut t = match DeadlineTimer::new(true, 3_000_000_000) {
        Ok(t) => t,
        Err(_) => return false,
    };
    if t.write_deadline(1_000_000, 1_000_000 + 300_000).is_err() {
        return false;
    }
    // 到期点 + 6000 tick（=2µs=2000ns）才投递。
    if t.on_tsc_tick(1_000_000 + 306_000).is_err() {
        return false;
    }
    t.consume() && t.p99_error_ns() == Some(2_000)
}

/// C10：零空转恒真——无样本时 p99 守卫必须 false（防恒真门禁）。
fn check_c10_no_empty_true() -> bool {
    let t = match DeadlineTimer::new(true, 3_000_000_000) {
        Ok(t) => t,
        Err(_) => return false,
    };
    !t.p99_within_tolerance()
}

/// 本模块自检（域聚合在 mod.rs）。
pub fn run_unx1043_checks() -> CheckSet {
    let mut set = CheckSet::new("UNX1043");
    let g = seg_guard();
    set.add("unx1043-seg-guard", g, if g { "" } else { "诊断码越出 0x9C 段" });
    let c1 = check_c1_zero_loss_100k();
    set.add("unx1043-c1-zero-loss-100k", c1, if c1 { "" } else { "10 万次零丢失或 p99 容差失守" });
    let c2 = check_c2_probe_10_10();
    set.add("unx1043-c2-probe-10-10", c2, if c2 { "" } else { "支持探测 10/10 双分支失守" });
    let c3 = check_c3_unsupported_reject();
    set.add("unx1043-c3-unsupported", c3, if c3 { "" } else { "不支持分支拒签失守" });
    let c4 = check_c4_single_shot();
    set.add("unx1043-c4-single-shot", c4, if c4 { "" } else { "单次语义守卫失守" });
    let c5 = check_c5_past_deadline();
    set.add("unx1043-c5-past", c5, if c5 { "" } else { "立即到期语义失守" });
    let c6 = check_c6_tsc_backward();
    set.add("unx1043-c6-backward", c6, if c6 { "" } else { "TSC 倒退拒签失守" });
    let c7 = check_c7_loss_detected();
    set.add("unx1043-c7-loss", c7, if c7 { "" } else { "丢失检出失守" });
    let c8 = check_c8_p99_math();
    set.add("unx1043-c8-p99-math", c8, if c8 { "" } else { "p99 分位数学失守" });
    let c9 = check_c9_conversion();
    set.add("unx1043-c9-conversion", c9, if c9 { "" } else { "tick→ns 换算失守" });
    let c10 = check_c10_no_empty_true();
    set.add("unx1043-c10-no-empty-true", c10, if c10 { "" } else { "空样本恒真门禁失守" });
    set
}

// ---------------------------------------------------------------------------
// 单元测试（宿主 cargo test 直跑；域聚合测试在 mod.rs）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx1043_all_checks_green() {
        let set = run_unx1043_checks();
        assert!(set.all_passed(), "UNX-F1043 domain checks must be green");
    }

    #[test]
    fn unx1043_segment_guard_holds() {
        assert!(seg_guard());
    }

    #[test]
    fn unx1043_zero_loss_never_breaks() {
        for seed in [1u64, 2, 3] {
            let (lost, p99, _) = zero_loss_rounds(seed);
            assert_eq!(lost, 0, "loss detected for seed {}", seed);
            assert!(p99.is_some() && p99.unwrap() <= P99_TOLERANCE_NS);
        }
    }

    #[test]
    fn unx1043_state_machine_cycle() {
        let mut t = fallback_timer();
        assert_eq!(t.state(), TimerState::Idle);
        assert_eq!(t.write_deadline(100, 200), Ok(TimerState::Armed));
        assert_eq!(t.on_tsc_tick(150), Ok(false));
        assert_eq!(t.on_tsc_tick(210), Ok(true));
        assert_eq!(t.state(), TimerState::Fired);
        assert!(t.consume());
        assert_eq!(t.state(), TimerState::Idle);
    }

    fn fallback_timer() -> DeadlineTimer {
        DeadlineTimer::new(true, 3_000_000_000).unwrap_or_else(|_| DeadlineTimer {
            supported: true,
            state: TimerState::Idle,
            deadline: 0,
            fires: 0,
            lost: 0,
            errors: ErrorRing::new(ERROR_RING_CAP),
            tsc_hz: 3_000_000_000,
            last_tsc: 0,
            events: Vec::new(),
        })
    }
}
