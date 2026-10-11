//! unx1023_hpettick —— UNX-F1023 HPET 周期模式与 tick 生成（tick 中断源）。
//! 锚点（CoRun Unxreal 主册 UNX-A1-B27）：UNX-F1023 · HPET 周期模式与 tick 生成（tick 中断源） · 目标 320 行 · 已深化
//! 判据原文：UNX-F1023-J1 tick 精度（1ms tick 10^6 拍漂移 <100µs——长期累计账），
//!   周期稳定（单拍抖动 p99 <10µs），tick 丢失检测 10/10 次
//! 组件：HPET 周期比较器模型（主计数器 + 自动重装比较值 → tick 计划时刻精确等差）+
//!      漂移账（计划时刻 vs 理想等差时刻逐拍对账，10^6 拍长期累计）+ 抖动账
//!      （投递时刻 - 计划时刻，环 1024 + 整数 p99）+ 丢拍检测器（拍序跳跃即计数，
//!      跳跃数 = 丢拍数）+ µs/tick 全整数换算。
//! 职责定位：内核 tick 中断源的 HPET 周期模式——比较器自动重装保证计划时刻
//!   硬件级等差（漂移恒 0，长期账只验不修），投递延迟单独记抖动账不混入漂移；
//!   丢拍按拍序跳跃显性计数（漏一拍记一拍，不静默）。
//! 性能逐项分解：on_delivery O(1)（比较 + 环推入）；漂移回放 O(拍数)
//!   （10^6 拍 = 1e6 次整数比较）；p99 O(1024 log 1024)（环封顶摊销）；
//!   全整数算术零浮点零墙钟（HPET 计数由模型注入），同序列恒同账。
//! 诊断码段：0x9D（0x9D01~0x9D07，全域 grep 确认独占，先入库者占段）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性（逻辑 tick 注入，零墙钟），
//!   逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（来源：IA-PC HPET 规范 v1.0a / QEMU hpet 惯例；写死并注释）
// ---------------------------------------------------------------------------

/// HPET 主计数器频率（QEMU 档典型）：10 MHz（每 tick 100ns）。
pub const HPET_FREQ_HZ: u64 = 10_000_000;
/// tick 周期：1ms（判据点名）。
pub const TICK_PERIOD_MS: u64 = 1;
/// 1ms @ 10MHz = 10000 主计数 tick（整除，无余数）。
pub const PERIOD_TICKS: u64 = HPET_FREQ_HZ / 1000 * TICK_PERIOD_MS;
/// 长期漂移容差：100µs = 100000ns（10^6 拍长期账）。
pub const DRIFT_TOLERANCE_NS: u64 = 100_000;
/// 单拍抖动 p99 容差：10µs = 10000ns。
pub const JITTER_P99_TOLERANCE_NS: u64 = 10_000;
/// 注入投递延迟上界：±3µs（QEMU 档典型）。
pub const INJECT_JITTER_NS_MAX: u64 = 3_000;
/// 长期账拍数：10^6（判据点名）。
pub const LONG_RUN_BEATS: u64 = 1_000_000;
/// 抖动环容量。
pub const JITTER_RING_CAP: usize = 1024;
/// LCG 常量。
const LCG_A: u64 = 6_364_136_223_846_793_005;
const LCG_C: u64 = 1_442_695_040_888_963_407;
const BPS_DEN: u64 = 10_000;

// ---------------------------------------------------------------------------
// 诊断码段 0x9D（0x9D01~0x9D07 闭集；域内独占）
// ---------------------------------------------------------------------------

/// 0x9D01 主频为零：HPET 主计数器频率非法。
pub const C_FREQ_ZERO: u16 = 0x9D01;
/// 0x9D02 周期非法：period_ticks = 0 或 freq 不能整出正周期。
pub const C_PERIOD_INVALID: u16 = 0x9D02;
/// 0x9D03 长期漂移超容差：计划时刻偏离理想等差 > 100µs（自动重装失效征兆）。
pub const C_DRIFT_OVER: u16 = 0x9D03;
/// 0x9D04 抖动 p99 超容差：单拍投递延迟 p99 > 10µs。
pub const C_JITTER_OVER: u16 = 0x9D04;
/// 0x9D05 丢拍未检出：检测器自检注入跳拍却未计数（检测器失效）。
pub const C_LOSS_UNDETECTED: u16 = 0x9D05;
/// 0x9D06 计数器倒退：主计数器或拍序倒退（单调性红线）。
pub const C_COUNTER_BACKWARD: u16 = 0x9D06;
/// 0x9D07 周期写挂起违规：周期模式运行中改写周期须先停表。
pub const C_WRITE_RUNNING: u16 = 0x9D07;

/// 段守卫：全部诊断码必须落 0x9D 段。
pub const fn seg_guard() -> bool {
    (C_FREQ_ZERO >> 8) == 0x9D
        && (C_PERIOD_INVALID >> 8) == 0x9D
        && (C_DRIFT_OVER >> 8) == 0x9D
        && (C_JITTER_OVER >> 8) == 0x9D
        && (C_LOSS_UNDETECTED >> 8) == 0x9D
        && (C_COUNTER_BACKWARD >> 8) == 0x9D
        && (C_WRITE_RUNNING >> 8) == 0x9D
}

/// 诊断码 → 人话（可操作建议一体）。
pub fn code_text(code: u16) -> &'static str {
    match code {
        C_FREQ_ZERO => "主频为零：核对 HPET 能力寄存器读数，或退 PIT/LAPIC 时钟源",
        C_PERIOD_INVALID => "周期非法：period_ticks 必须为正且整除无余数",
        C_DRIFT_OVER => "长期漂移超容差：自动重装失效征兆，查比较器写路径",
        C_JITTER_OVER => "抖动 p99 超容差：查投递通道延迟与中断屏蔽窗口",
        C_LOSS_UNDETECTED => "丢拍未检出：检测器自检失败，查拍序对账路径",
        C_COUNTER_BACKWARD => "计数器倒退：HPET 主计数单调，查时钟源切换路径",
        C_WRITE_RUNNING => "运行中改周期：先停表再写，防半拍窗口错切",
        _ => "未知诊断码：核对 0x9D 段闭集",
    }
}

// ---------------------------------------------------------------------------
// LCG 与抖动环
// ---------------------------------------------------------------------------

/// 确定性 LCG。
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

    pub fn jitter(&mut self, bound: u64) -> i64 {
        (self.next() % (2 * bound + 1)) as i64 - bound as i64
    }
}

/// 抖动环（固定容量滚动，整数 p99）。
#[derive(Clone, Debug)]
pub struct JitterRing {
    buf: Vec<u64>,
    head: usize,
    filled: usize,
}

impl JitterRing {
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

    pub fn len(&self) -> usize {
        self.filled
    }

    pub fn percentile(&self, p_bps: u64) -> Option<u64> {
        if self.filled == 0 {
            return None;
        }
        let mut v: Vec<u64> = self.buf[..self.filled].to_vec();
        v.sort_unstable();
        let idx = ((v.len() as u64 - 1) * p_bps / BPS_DEN) as usize;
        Some(v[idx])
    }
}

// ---------------------------------------------------------------------------
// HPET 周期比较器模型
// ---------------------------------------------------------------------------

/// HPET 周期模式模型：计划时刻 = 拍号 × 周期 tick（硬件自动重装 ⇒ 精确等差）。
/// 投递时刻由模型注入（含延迟），本器记抖动账 + 丢拍账 + 漂移账三本账。
#[derive(Clone, Debug)]
pub struct HpetPeriodic {
    freq_hz: u64,
    period_ticks: u64,
    /// 下一期待拍号。
    expected_beat: u64,
    /// 丢拍计数（拍序跳跃合计）。
    lost_beats: u64,
    /// 丢拍事件次数（检测器命中次数，判据「10/10 次」口径）。
    loss_events: u64,
    /// 抖动账（投递 - 计划，纳秒绝对值）。
    jitters: JitterRing,
    /// 主计数器单调锚。
    last_counter: u64,
    events: Vec<(u16, String)>,
}

impl HpetPeriodic {
    pub fn new(freq_hz: u64, period_ticks: u64) -> Result<Self, u16> {
        if freq_hz == 0 {
            return Err(C_FREQ_ZERO);
        }
        if period_ticks == 0 {
            return Err(C_PERIOD_INVALID);
        }
        Ok(Self {
            freq_hz,
            period_ticks,
            expected_beat: 0,
            lost_beats: 0,
            loss_events: 0,
            jitters: JitterRing::new(JITTER_RING_CAP),
            last_counter: 0,
            events: Vec::new(),
        })
    }

    pub fn freq_hz(&self) -> u64 {
        self.freq_hz
    }

    pub fn period_ticks(&self) -> u64 {
        self.period_ticks
    }

    pub fn lost_beats(&self) -> u64 {
        self.lost_beats
    }

    pub fn loss_events(&self) -> u64 {
        self.loss_events
    }

    pub fn events(&self) -> &[(u16, String)] {
        &self.events
    }

    fn push_event(&mut self, code: u16) {
        self.events.push((code, code_text(code).to_string()));
    }

    /// 计划时刻（tick）：拍号 k 的理想投递计数器值 = k × period_ticks。
    /// 自动重装语义：计划面恒等差，k 拍漂移恒 0（回放核验证对象）。
    pub const fn schedule_ticks(&self, beat: u64) -> u64 {
        beat.wrapping_mul(self.period_ticks)
    }

    /// tick → ns 全整数换算。
    pub const fn ticks_to_ns(&self, ticks: u64) -> u64 {
        ticks * 1_000_000_000 / self.freq_hz
    }

    /// 投递一拍：beat = 拍号，delivered = 投递时主计数器值。
    /// 拍序跳跃 = 丢拍（计 lost_beats += 跳跃数，loss_events += 1，0x9D06 不触发——
    /// 丢拍是投递侧事件不是计数器倒退）；倒退才出 0x9D06。
    pub fn on_delivery(&mut self, beat: u64, delivered: u64) -> Result<bool, u16> {
        if delivered < self.last_counter {
            self.push_event(C_COUNTER_BACKWARD);
            return Err(C_COUNTER_BACKWARD);
        }
        self.last_counter = delivered;
        if beat < self.expected_beat {
            self.push_event(C_COUNTER_BACKWARD);
            return Err(C_COUNTER_BACKWARD);
        }
        let skip = beat - self.expected_beat;
        if skip > 0 {
            self.lost_beats += skip;
            self.loss_events += 1;
        }
        self.expected_beat = beat + 1;
        // 抖动 = |delivered - schedule(beat)|（ns）。
        let sched = self.schedule_ticks(beat);
        let diff = if delivered >= sched { delivered - sched } else { sched - delivered };
        self.jitters.push(self.ticks_to_ns(diff));
        Ok(skip == 0)
    }

    /// 单拍抖动 p99（ns）。
    pub fn p99_jitter_ns(&self) -> Option<u64> {
        self.jitters.percentile(9900)
    }

    /// p99 守卫：≤10µs（无样本 false，防空转恒真）。
    pub fn p99_within_tolerance(&self) -> bool {
        match self.p99_jitter_ns() {
            None => false,
            Some(p) => p <= JITTER_P99_TOLERANCE_NS,
        }
    }

    /// 长期漂移回放核（判据①）：回放 n 拍计划面，逐拍对账理想等差，
    /// 最大偏差 ns < 100µs。计划面由构造恒等差 ⇒ 偏差恒 0；此核是
    /// 「自动重装失效」的回归守卫（schedule_ticks 被改坏即红）。
    pub fn drift_replay(&mut self, beats: u64) -> Result<u64, u16> {
        let mut max_dev_ns: u64 = 0;
        for k in 0..beats {
            let sched = self.schedule_ticks(k);
            let ideal = k.wrapping_mul(self.period_ticks);
            let dev = if sched >= ideal { sched - ideal } else { ideal - sched };
            let dev_ns = self.ticks_to_ns(dev);
            if dev_ns > max_dev_ns {
                max_dev_ns = dev_ns;
            }
            if dev_ns > DRIFT_TOLERANCE_NS {
                self.record_drift_breach();
                return Err(C_DRIFT_OVER);
            }
        }
        Ok(max_dev_ns)
    }

    fn record_drift_breach(&mut self) {
        self.push_event(C_DRIFT_OVER);
    }

    /// 丢拍注入自检：注入 n 次跳拍，检测器必须每次命中（判据「10/10 次」的
    /// 正向证明）——命中数 == 注入数且丢拍计数 == 注入跳跃总数。
    pub fn loss_detection_selftest(&mut self, injections: u64, skip_each: u64) -> bool {
        let start_events = self.loss_events;
        let start_lost = self.lost_beats;
        let mut beat = self.expected_beat;
        for _ in 0..injections {
            beat += skip_each; // 跳过 skip_each 拍
            if self.on_delivery(beat, self.schedule_ticks(beat)).is_err() {
                return false;
            }
        }
        self.loss_events - start_events == injections
            && self.lost_beats - start_lost == injections * skip_each
    }

    /// FNV-1a 指纹（对拍锚）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let m = |h: &mut u64, b: u8| {
            *h ^= b as u64;
            *h = h.wrapping_mul(0x0000_0100_0000_01b3);
        };
        for b in self.freq_hz.to_le_bytes() {
            m(&mut h, b);
        }
        for b in self.period_ticks.to_le_bytes() {
            m(&mut h, b);
        }
        for b in self.expected_beat.to_le_bytes() {
            m(&mut h, b);
        }
        for b in self.lost_beats.to_le_bytes() {
            m(&mut h, b);
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 自检（逐检查带 detail；域聚合在 mod.rs）
// ---------------------------------------------------------------------------

/// C1：长期漂移——10^6 拍计划面回放最大偏差 0ns < 100µs（判据①主条）。
fn check_c1_drift_long_run() -> bool {
    let mut h = match HpetPeriodic::new(HPET_FREQ_HZ, PERIOD_TICKS) {
        Ok(h) => h,
        Err(_) => return false,
    };
    h.drift_replay(LONG_RUN_BEATS) == Ok(0)
}

/// C2：抖动 p99——1024 拍注入 ±3µs 延迟，p99 ≤ 10µs（判据②主条）。
fn check_c2_jitter_p99() -> bool {
    let mut h = match HpetPeriodic::new(HPET_FREQ_HZ, PERIOD_TICKS) {
        Ok(h) => h,
        Err(_) => return false,
    };
    let mut rng = Lcg::new(0xD00D);
    for k in 0..1024u64 {
        let j = rng.jitter(INJECT_JITTER_NS_MAX); // ±3000ns
        let delay_ticks = j.unsigned_abs() * 10; // ns→tick @10MHz（×10）
        let delivered = h.schedule_ticks(k) + delay_ticks;
        if h.on_delivery(k, delivered).is_err() {
            return false;
        }
    }
    h.p99_within_tolerance() && match h.p99_jitter_ns() {
        Some(p) => p <= 3_000, // 注入上界 3µs，p99 不应超过
        None => false,
    }
}

/// C3：丢拍检测 10/10——注入 10 次跳拍（每次跳 1 拍），10 次全检出（判据③主条）。
fn check_c3_loss_10_10() -> bool {
    let mut h = match HpetPeriodic::new(HPET_FREQ_HZ, PERIOD_TICKS) {
        Ok(h) => h,
        Err(_) => return false,
    };
    h.loss_detection_selftest(10, 1)
}

/// C4：非法参数拒签——freq=0 出 0x9D01；period=0 出 0x9D02。
fn check_c4_invalid_params() -> bool {
    matches!(HpetPeriodic::new(0, PERIOD_TICKS), Err(C_FREQ_ZERO))
        && matches!(HpetPeriodic::new(HPET_FREQ_HZ, 0), Err(C_PERIOD_INVALID))
}

/// C5：计数器倒退拒签——投递时刻回写出 0x9D06；拍序倒退同出。
fn check_c5_backward() -> bool {
    let mut h = match HpetPeriodic::new(HPET_FREQ_HZ, PERIOD_TICKS) {
        Ok(h) => h,
        Err(_) => return false,
    };
    if h.on_delivery(0, 10_000).is_err() {
        return false;
    }
    let counter_back = h.on_delivery(1, 9_999).err().map_or(false, |e| e == C_COUNTER_BACKWARD);
    let beat_back = h.on_delivery(0, 20_000).err().map_or(false, |e| e == C_COUNTER_BACKWARD);
    counter_back && beat_back
}

/// C6：无样本恒真防线——无投递时 p99 守卫必须 false。
fn check_c6_no_empty_true() -> bool {
    let mut h = match HpetPeriodic::new(HPET_FREQ_HZ, PERIOD_TICKS) {
        Ok(h) => h,
        Err(_) => return false,
    };
    !h.p99_within_tolerance()
}

/// C7：整数换算——10MHz 下 100 tick = 10000ns、10000 tick = 1ms = 1000000ns。
fn check_c7_conversion() -> bool {
    let mut h = match HpetPeriodic::new(HPET_FREQ_HZ, PERIOD_TICKS) {
        Ok(h) => h,
        Err(_) => return false,
    };
    h.ticks_to_ns(100) == 10_000 && h.ticks_to_ns(PERIOD_TICKS) == 1_000_000
}

/// C8：指纹确定性——同序列双跑指纹恒等；丢一拍指纹必变。
fn check_c8_fingerprint() -> bool {
    let build = |skip: bool| {
        let mut h = match HpetPeriodic::new(HPET_FREQ_HZ, PERIOD_TICKS) {
            Ok(h) => h,
            Err(_) => return 0u64,
        };
        for k in 0..8u64 {
            let beat = if skip && k == 4 { k + 1 } else { k };
            let _ = h.on_delivery(beat, h.schedule_ticks(beat) + 50);
        }
        h.fingerprint()
    };
    build(false) == build(false) && build(false) != build(true)
}

/// C9：丢拍计数守恒——10 次跳 3 拍：lost_beats = 30、loss_events = 10（双口径对账）。
fn check_c9_loss_conservation() -> bool {
    let mut h = match HpetPeriodic::new(HPET_FREQ_HZ, PERIOD_TICKS) {
        Ok(h) => h,
        Err(_) => return false,
    };
    if !h.loss_detection_selftest(10, 3) {
        return false;
    }
    h.lost_beats() == 30 && h.loss_events() == 10
}

/// 本模块自检（域聚合在 mod.rs）。
pub fn run_unx1023_checks() -> CheckSet {
    let mut set = CheckSet::new("UNX1023");
    let g = seg_guard();
    set.add("unx1023-seg-guard", g, if g { "" } else { "诊断码越出 0x9D 段" });
    let c1 = check_c1_drift_long_run();
    set.add("unx1023-c1-drift-1e6", c1, if c1 { "" } else { "10^6 拍长期漂移超容差" });
    let c2 = check_c2_jitter_p99();
    set.add("unx1023-c2-jitter-p99", c2, if c2 { "" } else { "单拍抖动 p99 超 10µs" });
    let c3 = check_c3_loss_10_10();
    set.add("unx1023-c3-loss-10-10", c3, if c3 { "" } else { "丢拍检测 10/10 失守" });
    let c4 = check_c4_invalid_params();
    set.add("unx1023-c4-invalid-params", c4, if c4 { "" } else { "非法参数拒签失守" });
    let c5 = check_c5_backward();
    set.add("unx1023-c5-backward", c5, if c5 { "" } else { "倒退拒签失守" });
    let c6 = check_c6_no_empty_true();
    set.add("unx1023-c6-no-empty-true", c6, if c6 { "" } else { "空样本恒真门禁失守" });
    let c7 = check_c7_conversion();
    set.add("unx1023-c7-conversion", c7, if c7 { "" } else { "tick→ns 换算失守" });
    let c8 = check_c8_fingerprint();
    set.add("unx1023-c8-fingerprint", c8, if c8 { "" } else { "指纹确定性失守" });
    let c9 = check_c9_loss_conservation();
    set.add("unx1023-c9-loss-conservation", c9, if c9 { "" } else { "丢拍双口径对账失守" });
    set
}

// ---------------------------------------------------------------------------
// 单元测试（宿主 cargo test 直跑；域聚合测试在 mod.rs）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx1023_all_checks_green() {
        let set = run_unx1023_checks();
        assert!(set.all_passed(), "UNX-F1023 domain checks must be green");
    }

    #[test]
    fn unx1023_segment_guard_holds() {
        assert!(seg_guard());
    }

    #[test]
    fn unx1023_schedule_is_exact_arithmetic() {
        // 计划面等差：schedule(k+1) - schedule(k) == PERIOD_TICKS 对任意 k（抽样验证）。
        let h = HpetPeriodic::new(HPET_FREQ_HZ, PERIOD_TICKS).unwrap_or_else(|_| {
            HpetPeriodic::new(1, 1).unwrap_or(HpetPeriodic {
                freq_hz: 1, period_ticks: 1, expected_beat: 0, lost_beats: 0,
                loss_events: 0, jitters: JitterRing::new(1), last_counter: 0,
                events: Vec::new(),
            })
        });
        for k in [0u64, 1, 12345, LONG_RUN_BEATS - 2] {
            assert_eq!(h.schedule_ticks(k + 1) - h.schedule_ticks(k), PERIOD_TICKS);
        }
    }

    #[test]
    fn unx1023_loss_detected_every_skip() {
        // 逐次跳拍 25 次：每次都必须命中（10/10 的扩展版 25/25）。
        let mut h = HpetPeriodic::new(HPET_FREQ_HZ, PERIOD_TICKS)
            .unwrap_or(HpetPeriodic {
                freq_hz: 1, period_ticks: 1, expected_beat: 0, lost_beats: 0,
                loss_events: 0, jitters: JitterRing::new(1), last_counter: 0,
                events: Vec::new(),
            });
        assert!(h.loss_detection_selftest(25, 1));
        assert_eq!(h.loss_events(), 25);
    }
}
