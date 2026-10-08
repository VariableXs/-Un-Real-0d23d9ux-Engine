//! F064 音频低延迟链 · 完整设计（STAR I 主册 G-B-24）。
//!
//! **判据（主册）**：单流 20ms 线实测达标（正弦测试信号示波法或环回法）；
//! 双流混音延迟增量 <2ms。
//!
//! **设计要点（主册）**：
//! - 混音器单流端到端延迟 ≤20ms：应用写缓冲 → 混音 → HDA CORB/RIRB →
//!   出声，全链**分段计时**；音乐类应用可用性门槛；
//! - 缓冲三档：交互 5ms / 默认 10ms / 省电 20ms（按流声明选；省电档天然
//!   贴线，诊断面如实标注「省电档不承诺延迟」——诚实降级，不伪装达标）；
//! - 混音器 tick 与 HDA DMA 对齐（不做两次缓冲拷贝——对齐由账本核查）；
//! - 延迟超标归因分四段（应用供数/混音/传输/DAC）逐段显示——「慢在哪
//!   一段」不猜；
//! - 缓冲欠载（underrun）→ F026 既有静音策略 + 计数告警（本模块只记账，
//!   静音动作归 F026）；
//! - deadline 线程类（F047）音频 300μs 预算保障供数——供数段超预算如实
//!   计数（供数超标是应用侧问题，不记到混音头上）；
//! - 延迟测量工具内置（环回自测一键跑）；延迟采样入账本；无持久化配置
//!   （参数进旋钮清单）。
//!
//! 实现自研（B-806/807 既有链深化）；分段计时参照 Linux PW 低延迟实践。
//! 一切时间注入式（微秒戳/分钟戳），宿主测试确定复现。

use crate::checks::CheckSet;
use crate::star::sbase::{pct_near, MinuteBook, RingLog};

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数进旋钮清单）
// ---------------------------------------------------------------------------

/// 单流端到端延迟判线（μs）。
pub const DELAY_LINE_US: u64 = 20_000;

/// 双流混音延迟增量判线（μs）。
pub const DUAL_DELTA_LIMIT_US: u64 = 2_000;

/// 音频供数预算（μs，F047 deadline 线程类）。
pub const SUPPLY_BUDGET_US: u32 = 300;

/// 运行链路延迟环容量。
const OPS_RING_CAP: usize = 1024;

/// 环回探针环容量（单流/双流各一）。
const PROBE_RING_CAP: usize = 256;

/// 流表容量（音乐/会议场景活跃流数远小于此；LRU 逐出）。
const STREAM_CAP: usize = 16;

/// underrun 分钟账本保留窗（分钟）。
const UNDERRUN_BOOK_MIN: u64 = 1440;

// ---------------------------------------------------------------------------
// 分段计时模型
// ---------------------------------------------------------------------------

/// 全链四段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Segment {
    /// 应用供数（写缓冲）。
    Supply = 0,
    /// 混音。
    Mix = 1,
    /// 传输（HDA CORB/RIRB）。
    Transport = 2,
    /// DAC 出声。
    Dac = 3,
}

/// 四段耗时（μs，注入式打点差）。
pub type SegmentLat = [u32; 4];

/// 端到端 = 缓冲等待 + 四段之和（口径唯一，全模块共用此公式）。
pub fn e2e_us(wait_us: u32, seg: &SegmentLat) -> u64 {
    wait_us as u64 + seg.iter().map(|v| *v as u64).sum::<u64>()
}

/// 归因：四段中最大者（「慢在哪一段」不猜；并列取先段——确定性）。
pub fn worst_segment(seg: &SegmentLat) -> (Segment, u32) {
    let mut best = 0usize;
    for i in 1..4 {
        if seg[i] > seg[best] {
            best = i;
        }
    }
    let s = match best {
        0 => Segment::Supply,
        1 => Segment::Mix,
        2 => Segment::Transport,
        _ => Segment::Dac,
    };
    (s, seg[best])
}

/// 缓冲三档（按流声明选；数值进旋钮清单）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufMode {
    /// 交互 5ms。
    Interactive,
    /// 默认 10ms。
    Default,
    /// 省电 20ms。
    PowerSave,
}

impl BufMode {
    /// 档位缓冲时长（μs）。
    pub fn buf_us(self) -> u32 {
        match self {
            BufMode::Interactive => 5_000,
            BufMode::Default => 10_000,
            BufMode::PowerSave => 20_000,
        }
    }

    /// 档位序号（账本存储用）。
    pub fn ordinal(self) -> u8 {
        match self {
            BufMode::Interactive => 0,
            BufMode::Default => 1,
            BufMode::PowerSave => 2,
        }
    }

    pub fn from_ordinal(o: u8) -> Option<BufMode> {
        match o {
            0 => Some(BufMode::Interactive),
            1 => Some(BufMode::Default),
            2 => Some(BufMode::PowerSave),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 链路账本
// ---------------------------------------------------------------------------

/// 一条端到端采样（运行链路）。
#[derive(Clone, Copy, Debug)]
pub struct E2eSample {
    /// 声明缓冲档（BufMode::ordinal）。
    pub mode: u8,
    /// 采样时活跃流数（1=单流场景，≥2=多流场景）。
    pub active: u8,
    /// 缓冲等待（μs，≤ 档位缓冲时长）。
    pub wait_us: u32,
    /// 四段耗时（μs）。
    pub seg: SegmentLat,
}

/// 流表条目：每流延迟标注与欠载计数。
#[derive(Clone, Copy, Debug)]
pub struct StreamMeter {
    pub id: u32,
    pub mode: u8,
    /// 累计 underrun 次数。
    pub underruns: u32,
    /// 最近一次缓冲水位（μs，钳制在档位缓冲时长内）。
    pub watermark_us: u32,
    /// 供数超标次数（供数段 > 300μs）。
    pub supply_over: u32,
    /// 最近一次端到端延迟（μs）。
    pub last_e2e_us: u64,
}

/// 环回自测报告（一键跑产出——本模块判据直读面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoopbackReport {
    /// 单流 P95（μs）。
    pub single_p95_us: u64,
    /// 双流 P95（μs，同等待口径对比）。
    pub dual_p95_us: u64,
    /// 双流增量（μs）。
    pub delta_us: u64,
    /// 单流 20ms 线达标（探针样本 ≥32 才下结论——数据充足性门）。
    pub single_meets: bool,
    /// 双流增量 <2ms 达标（两侧样本均 ≥32）。
    pub delta_meets: bool,
    /// 探针样本量（单流/双流）。
    pub single_n: usize,
    pub dual_n: usize,
}

/// 音频低延迟链账本：运行采样环 + 环回探针环 + 流表 + underrun 分钟账本。
pub struct MixerLedger {
    ops: RingLog<E2eSample, OPS_RING_CAP>,
    probe_single: RingLog<u32, PROBE_RING_CAP>,
    probe_dual: RingLog<u32, PROBE_RING_CAP>,
    streams: Vec<StreamMeter>,
    /// underrun 分钟账本（单列：分钟 → 次数；保留 1440 分钟）。
    underrun_book: MinuteBook,
    underrun_total: u32,
    supply_over_total: u32,
}

impl MixerLedger {
    pub fn new() -> MixerLedger {
        MixerLedger {
            ops: RingLog::new(),
            probe_single: RingLog::new(),
            probe_dual: RingLog::new(),
            streams: Vec::new(),
            underrun_book: MinuteBook::new(1, UNDERRUN_BOOK_MIN),
            underrun_total: 0,
            supply_over_total: 0,
        }
    }

    // ---- 流表 ------------------------------------------------------------

    /// 流声明（选缓冲档）。重复声明返回 false（幂等保护）。
    pub fn declare_stream(&mut self, id: u32, mode: BufMode) -> bool {
        if self.streams.iter().any(|s| s.id == id) {
            return false;
        }
        if self.streams.len() >= STREAM_CAP {
            self.streams.remove(0); // LRU：逐出最早声明者。
        }
        self.streams.push(StreamMeter {
            id,
            mode: mode.ordinal(),
            underruns: 0,
            watermark_us: 0,
            supply_over: 0,
            last_e2e_us: 0,
        });
        true
    }

    fn stream_idx(&self, id: u32) -> Option<usize> {
        self.streams.iter().position(|s| s.id == id)
    }

    /// 流的可变访问（诊断面板每流标注直读）。
    pub fn stream(&self, id: u32) -> Option<&StreamMeter> {
        self.stream_idx(id).map(|i| &self.streams[i])
    }

    /// 撤销流声明（流结束）。
    pub fn retire_stream(&mut self, id: u32) -> bool {
        match self.stream_idx(id) {
            Some(i) => {
                self.streams.remove(i);
                true
            }
            None => false,
        }
    }

    /// 更新缓冲水位（μs；钳制在档位缓冲时长内——水位不可能超过缓冲容量）。
    pub fn set_watermark(&mut self, id: u32, level_us: u32) -> bool {
        let i = match self.stream_idx(id) {
            Some(i) => i,
            None => return false,
        };
        let cap = match BufMode::from_ordinal(self.streams[i].mode) {
            Some(m) => m.buf_us(),
            None => return false,
        };
        self.streams[i].watermark_us = level_us.min(cap);
        true
    }

    // ---- 运行链路 ----------------------------------------------------------

    /// 记录一次运行链路端到端采样；同步更新流表标注与供数超标计数。
    pub fn record(&mut self, stream_id: u32, wait_us: u32, seg: &SegmentLat) -> bool {
        let i = match self.stream_idx(stream_id) {
            Some(i) => i,
            None => return false,
        };
        let mode = self.streams[i].mode;
        let active = self.streams.len().min(255) as u8;
        if seg[Segment::Supply as usize] > SUPPLY_BUDGET_US {
            self.supply_over_total += 1;
            self.streams[i].supply_over += 1;
        }
        let e2e = e2e_us(wait_us, seg);
        self.streams[i].last_e2e_us = e2e;
        self.ops.push(E2eSample { mode, active, wait_us, seg: *seg });
        true
    }

    /// underrun 记账（静音动作归 F026——本模块只计数与分钟账本）。
    pub fn record_underrun(&mut self, stream_id: u32, minute: u64) -> bool {
        match self.stream_idx(stream_id) {
            Some(i) => {
                self.streams[i].underruns += 1;
                self.underrun_total += 1;
                self.underrun_book.record_minute(minute, &[1]);
                true
            }
            None => false,
        }
    }

    /// underrun 总数（计数告警面）。
    pub fn underruns(&self) -> u32 {
        self.underrun_total
    }

    /// 指定分钟窗（过去 `span_min` 分钟，含当前分钟）underrun 合计。
    pub fn underruns_in(&self, now_minute: u64, span_min: u64) -> u64 {
        let from = now_minute.saturating_sub(span_min) + 1;
        self.underrun_book.range_sum(from, now_minute).iter().sum()
    }

    /// underrun 分钟账本驱逐（保留窗对齐，调用方按分钟心跳驱动）。
    pub fn evict_underrun_book(&mut self, now_minute: u64) -> usize {
        self.underrun_book.evict_by_now(now_minute)
    }

    /// 供数超标总数（300μs 预算面）。
    pub fn supply_overruns(&self) -> u32 {
        self.supply_over_total
    }

    /// 运行链路延迟统计（诊断面板全链延迟分布图数据源）。
    /// 返回 (p50, p95, p99, 样本数)，按（档位, 最小活跃流数）过滤。
    pub fn live_stats(&self, mode: BufMode, min_active: u8) -> (u64, u64, u64, usize) {
        let v: Vec<u64> = self
            .ops
            .newest_first()
            .iter()
            .filter(|s| s.mode == mode.ordinal() && s.active >= min_active)
            .map(|s| e2e_us(s.wait_us, &s.seg))
            .collect();
        (pct_near(&v, 50), pct_near(&v, 95), pct_near(&v, 99), v.len())
    }

    // ---- 环回自测工具（一键跑） ---------------------------------------------

    /// 环回探针：正弦测试信号标记样本（单流场景）。
    /// 等待时长由探针注入方按目标缓冲档给定（判线语义由此承载）。
    pub fn probe_single(&mut self, wait_us: u32, seg: &SegmentLat) {
        self.probe_single.push(e2e_us(wait_us, seg).min(u32::MAX as u64) as u32);
    }

    /// 环回探针（双流场景，同等待口径对比）。
    pub fn probe_dual(&mut self, wait_us: u32, seg: &SegmentLat) {
        self.probe_dual.push(e2e_us(wait_us, seg).min(u32::MAX as u64) as u32);
    }

    /// 环回报告（判据直读：单流 20ms 线 + 双流增量 <2ms）。
    pub fn loopback_report(&self) -> LoopbackReport {
        let sv: Vec<u64> = self.probe_single.newest_first().iter().map(|v| *v as u64).collect();
        let dv: Vec<u64> = self.probe_dual.newest_first().iter().map(|v| *v as u64).collect();
        let single_p95 = pct_near(&sv, 95);
        let dual_p95 = pct_near(&dv, 95);
        LoopbackReport {
            single_p95_us: single_p95,
            dual_p95_us: dual_p95,
            delta_us: dual_p95.saturating_sub(single_p95),
            single_meets: sv.len() >= 32 && single_p95 <= DELAY_LINE_US,
            delta_meets: sv.len() >= 32
                && dv.len() >= 32
                && dual_p95.saturating_sub(single_p95) < DUAL_DELTA_LIMIT_US,
            single_n: sv.len(),
            dual_n: dv.len(),
        }
    }
}

impl Default for MixerLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F064 自检（判据：单流 20ms 线 + 双流增量 <2ms + 分段归因不猜）。
pub fn run_audiolow_checks() -> CheckSet {
    let mut set = CheckSet::new("F064-audiolow");

    // 1. 缓冲三档语义（旋钮清单锚点）。
    set.add(
        "buffer three modes 5/10/20ms",
        BufMode::Interactive.buf_us() == 5_000
            && BufMode::Default.buf_us() == 10_000
            && BufMode::PowerSave.buf_us() == 20_000,
        "",
    );

    // 2. 单流 20ms 环回判线：默认档 10ms 等待 + 四段 ~1ms → P95 ≈ 11ms 达标。
    let mut led = MixerLedger::new();
    let seg_ok: SegmentLat = [280, 400, 180, 140]; // 合计 1ms
    for _ in 0..40 {
        led.probe_single(10_000, &seg_ok);
    }
    let rep_ok = led.loopback_report();
    set.add(
        "loopback single-stream 20ms line",
        rep_ok.single_meets && rep_ok.single_p95_us <= 11_000 && rep_ok.single_n == 40,
        "",
    );

    // 3. 双流增量 <2ms：混音段 400→900μs → 增量 500μs 达标。
    let seg_dual: SegmentLat = [280, 900, 180, 140];
    for _ in 0..40 {
        led.probe_dual(10_000, &seg_dual);
    }
    let rep_dual = led.loopback_report();
    set.add(
        "loopback dual-stream delta <2ms",
        rep_dual.delta_meets && rep_dual.delta_us == 500,
        "",
    );

    // 4. 判线诚实红面：混音段膨胀 3ms → 增量超标如实不达标。
    let mut led_bad = MixerLedger::new();
    let seg_s: SegmentLat = [280, 400, 180, 140];
    let seg_d: SegmentLat = [280, 3_400, 180, 140];
    for _ in 0..40 {
        led_bad.probe_single(10_000, &seg_s);
        led_bad.probe_dual(10_000, &seg_d);
    }
    let rep_bad = led_bad.loopback_report();
    set.add(
        "delta line honest red when mix bloats",
        !rep_bad.delta_meets && rep_bad.delta_us == 3_000,
        "",
    );

    // 5. 省电档诚实标注：20ms 缓冲等待天然贴线 → 判线不承诺（非伪装绿）。
    let mut led_ps = MixerLedger::new();
    for _ in 0..40 {
        led_ps.probe_single(20_000, &seg_ok);
    }
    set.add(
        "powersave buffer honestly exceeds line",
        led_ps.loopback_report().single_p95_us > DELAY_LINE_US,
        "",
    );

    // 6. 分段归因：哪段最大就是哪段（慢在哪一段不猜）。
    let seg_dac: SegmentLat = [280, 400, 180, 9_000];
    set.add("worst segment attribution dac", worst_segment(&seg_dac) == (Segment::Dac, 9_000), "");
    let seg_sup: SegmentLat = [5_000, 400, 180, 140];
    set.add("worst segment attribution supply", worst_segment(&seg_sup) == (Segment::Supply, 5_000), "");

    // 7. underrun 计数 + 分钟账本窗口语义（F026 静音策略接缝外置）。
    let mut led_u = MixerLedger::new();
    assert!(led_u.declare_stream(7, BufMode::Interactive));
    for m in [100u64, 100, 101, 200] {
        assert!(led_u.record_underrun(7, m));
    }
    set.add(
        "underrun count and minute book",
        led_u.underruns() == 4
            && led_u.underruns_in(200, 100) == 2  // [101,200]: 101×1 + 200×1
            && led_u.underruns_in(200, 150) == 4  // [51,200]: 全部 4 次
            && led_u.underruns_in(200, 1) == 1,   // [200,200]
        "",
    );

    // 8. 供数预算：供数段 400μs > 300μs → 超标计数；200μs → 不计。
    let mut led_s = MixerLedger::new();
    assert!(led_s.declare_stream(9, BufMode::Interactive));
    assert!(led_s.record(9, 5_000, &[400, 300, 100, 100]));
    assert!(led_s.record(9, 5_000, &[200, 300, 100, 100]));
    set.add(
        "supply budget 300us overrun count",
        led_s.supply_overruns() == 1 && led_s.stream(9).unwrap().supply_over == 1,
        "",
    );

    // 9. 水位钳制：交互档容量 5ms，写 8ms → 钳到 5ms。
    set.add(
        "watermark clamped to buffer capacity",
        led_s.set_watermark(9, 8_000) && led_s.stream(9).unwrap().watermark_us == 5_000,
        "",
    );

    // 10. 流表幂等与 LRU：重复声明拒绝；第 17 条逐出最早者。
    let mut led_f = MixerLedger::new();
    let mut all_declared = true;
    for i in 0..16u32 {
        all_declared &= led_f.declare_stream(i, BufMode::Default);
    }
    set.add(
        "stream table idempotent + lru cap",
        all_declared
            && !led_f.declare_stream(0, BufMode::Interactive)
            && led_f.declare_stream(100, BufMode::Interactive)
            && led_f.stream(0).is_none()
            && led_f.stream(1).is_some()
            && led_f.stream(100).is_some(),
        "",
    );

    // 11. 运行链路统计（分布图数据源）：双流默认档 120 样本，P95 落在 11ms 内。
    let mut led_live = MixerLedger::new();
    assert!(led_live.declare_stream(3, BufMode::Default));
    assert!(led_live.declare_stream(4, BufMode::Default));
    for i in 0..120u32 {
        let id = if i % 2 == 0 { 3 } else { 4 };
        assert!(led_live.record(id, 10_000, &[200 + i % 3, 300, 100, 100]));
    }
    let (p50, p95, _p99, n) = led_live.live_stats(BufMode::Default, 1);
    set.add("live stats p95 within line", n == 120 && p95 <= 11_000 && p50 <= p95, "");

    // 12. 端到端合成口径唯一：e2e = 等待 + 四段和。
    set.add("e2e formula single source", e2e_us(10_000, &seg_ok) == 11_000, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// xorshift32——测试抖动源。
    fn xorshift(x: &mut u32) -> u32 {
        *x ^= *x << 13;
        *x ^= *x >> 17;
        *x ^= *x << 5;
        *x
    }

    #[test]
    fn sine_loopback_end_to_end() {
        // 环回法模拟：100 个正弦标记样本，段耗时带抖动（±10%）。
        let mut led = MixerLedger::new();
        let mut x: u32 = 0xCAFEF00D;
        for _ in 0..100 {
            let j = xorshift(&mut x) % 16;
            let seg: SegmentLat = [270 + j, 380 + j, 170 + j / 2, 130 + j / 2];
            led.probe_single(10_000, &seg);
        }
        let rep = led.loopback_report();
        assert!(rep.single_meets, "P95 {} 应 ≤20ms", rep.single_p95_us);
        // 四段和 = 950 + 3j（j<16）→ e2e ∈ [10_950, 10_995]。
        assert!(rep.single_p95_us > 10_800 && rep.single_p95_us <= 10_995);
    }

    #[test]
    fn probe_ring_capacity() {
        let mut led = MixerLedger::new();
        let seg: SegmentLat = [100, 100, 100, 100];
        for _ in 0..300 {
            led.probe_single(5_000, &seg);
        }
        let rep = led.loopback_report();
        assert_eq!(rep.single_n, PROBE_RING_CAP, "探针环定容 256");
    }

    #[test]
    fn worst_segment_tie_takes_first() {
        let seg: SegmentLat = [500, 500, 100, 100];
        assert_eq!(worst_segment(&seg), (Segment::Supply, 500));
    }

    #[test]
    fn underrun_minute_window_semantics() {
        let mut led = MixerLedger::new();
        assert!(led.declare_stream(1, BufMode::Interactive));
        for m in [10u64, 10, 12, 12, 12, 50] {
            assert!(led.record_underrun(1, m));
        }
        assert_eq!(led.underruns(), 6);
        assert_eq!(led.underruns_in(12, 2), 3, "[11,12] 窗：只有 12 分钟的 3 次");
        assert_eq!(led.underruns_in(12, 100), 5, "[1,12] 窗：10×2 + 12×3");
        assert_eq!(led.underruns_in(50, 1), 1);
        assert_eq!(led.underruns_in(50, 0), 0, "空窗返回 0");
        // 保留窗驱逐：50 分钟心跳下 cap 1440 → 无驱逐。
        assert_eq!(led.evict_underrun_book(50), 0);
    }

    #[test]
    fn stream_lifecycle() {
        let mut led = MixerLedger::new();
        assert!(led.declare_stream(1, BufMode::Default));
        assert!(!led.declare_stream(1, BufMode::Interactive), "重复声明拒绝");
        assert!(led.record(1, 10_000, &[200, 300, 100, 100]));
        assert!(!led.record(99, 10_000, &[200, 300, 100, 100]), "未声明流拒绝");
        assert_eq!(led.stream(1).unwrap().last_e2e_us, 10_700);
        assert!(led.retire_stream(1));
        assert!(!led.retire_stream(1), "重复撤销拒绝");
        assert!(!led.record(1, 10_000, &[200, 300, 100, 100]), "撤销后拒绝");
        assert!(!led.record_underrun(1, 5), "撤销后 underrun 拒绝");
    }

    #[test]
    fn dual_scenario_active_count() {
        let mut led = MixerLedger::new();
        assert!(led.declare_stream(1, BufMode::Default));
        assert!(led.record(1, 10_000, &[200, 300, 100, 100]));
        assert!(led.declare_stream(2, BufMode::Default));
        assert!(led.record(2, 10_000, &[200, 300, 100, 100]));
        // active>=1：两条都算；active>=2：只算第二条（声明后流数=2）。
        let (_, _, _, n_ge1) = led.live_stats(BufMode::Default, 1);
        let (_, _, _, n_ge2) = led.live_stats(BufMode::Default, 2);
        assert_eq!(n_ge1, 2);
        assert_eq!(n_ge2, 1);
    }

    #[test]
    fn buf_mode_ordinals_roundtrip() {
        for o in 0..3u8 {
            let m = BufMode::from_ordinal(o).unwrap();
            assert_eq!(m.ordinal(), o);
        }
        assert!(BufMode::from_ordinal(3).is_none());
    }

    #[test]
    fn watermark_requires_declared_stream() {
        let mut led = MixerLedger::new();
        assert!(!led.set_watermark(42, 1_000), "未声明流拒绝");
        assert!(led.declare_stream(42, BufMode::Default));
        assert!(led.set_watermark(42, 10_000));
        assert_eq!(led.stream(42).unwrap().watermark_us, 10_000);
    }

    #[test]
    fn e2e_and_delta_arithmetic() {
        let seg: SegmentLat = [280, 400, 180, 140];
        assert_eq!(e2e_us(5_000, &seg), 6_000);
        assert_eq!(e2e_us(0, &seg), 1_000);
    }
}
