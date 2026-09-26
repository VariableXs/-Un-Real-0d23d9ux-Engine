//! F041 帧率账本（perfstar · G-B-01）——合成器逐帧记账。
//!
//! 主册判据（验收标准第一句）：
//! **账本打点自身开销 <0.1% CPU 实测；帧率图与实测录屏逐帧对得上（抽 10 帧核对）。**
//!
//! 功能定义（G-B-01）：每帧记录**合成耗时、脏区面积、提交耗时、等待时间**四项，
//! 写入性能账本（与监视器同源，B-2001 纪律）；账本按滚动窗口保留 60 秒逐帧 +
//! 24 小时分钟聚合。帧耗时测量点三处（输入处理/合成/提交）分段计时——归因
//! （F042）的最小粒度是「哪一段慢」。
//!
//! 【设计细节】落地口径：
//! - 独立阈值：合成 6ms / 提交 3ms / 输入 1.5ms / 等待不做红线（主册原文）；
//! - 80fps 红线 = 12.5ms（强调色实线语义）、60fps 线 = 16.6ms（灰虚线语义），
//!   本模块以常量供给呈现面（F041 交互设计），视觉分级由监视器渲染；
//! - 窗口拖动场景打专项标签（bit 标记，压力测试复现用）；
//! - 合成器崩溃重启 → 账本标注断点（不伪造连续）；
//! - 账本写入自身超预算（>0.1% CPU）→ 自动降采样（每 2 帧记 1 帧）并标注。
//!
//! 【存储口径 · 诚实推导】主册「逐帧环形缓冲内存 64KB（60s×1000fps 上限）」：
//! 64KB 物理装不下 60000 条四项记录（任何 ≥4B 记录都装不下），故本实现取
//! **记录 10B（四项 u16 + 脏区 permille 打包）× 6553 条 = 64KB 硬预算**；
//! 原生保真保留 60s 至 **109fps**；更高帧率按 `ratio = ceil(fps×60/6553)`
//! 自适应降采样保 60s 窗口（1000fps 口径 → 每 16 帧记 1 帧，标注降采样位）。
//! 「1000fps 上限」按输入事件率口径理解，不是存储口径——此偏差在完成报告
//! 台账登记（AI-K1 · 偏差项 D1）。
//!
//! 零堆纪律：定长数组，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 合成段红线（主册设计细节：合成 6ms）。
pub const REDLINE_COMPOSE_US: u16 = 6_000;
/// 提交段红线（主册：提交 3ms）。
pub const REDLINE_COMMIT_US: u16 = 3_000;
/// 输入段红线（主册：输入 1.5ms）。
pub const REDLINE_INPUT_US: u16 = 1_500;
/// 80fps 线 = 12.5ms（监视器强调色实线）。
pub const LINE_80FPS_US: u32 = 12_500;
/// 60fps 线 = 16.6ms（监视器灰虚线）。
pub const LINE_60FPS_US: u32 = 16_600;
/// 打点自身开销红线：0.1% CPU = 千分之一。
pub const SELF_COST_PERMILLE_CAP: u32 = 1;

/// 逐帧记录字节数：四项 u16（input/compose/commit/wait）+ 2B 脏区 permille 与
/// 标志位打包。
pub const RECORD_BYTES: usize = 10;
/// 环形容量：64KB / 10B = 6553 条（64KB 硬预算，主册【数据与存储】）。
pub const RING_CAP: usize = 65_536 / RECORD_BYTES; // 6553
/// 保真 60s 的原生帧率上界：6553 / 60 ≈ 109fps。
pub const NATIVE_FPS_FOR_60S: u32 = (RING_CAP / 60) as u32; // 109
/// 分钟聚合桶数：24h × 60（主册：24 小时分钟聚合）。
pub const MINUTE_BUCKETS: usize = 1_440;
/// 脏区 permille 上限（1000 = 整屏）。
pub const DIRTY_PERMILLE_MAX: u16 = 1_000;

// 记录标志位（脏区 u16 高位打包；permille 占低 10 位）。
const FLAG_DOWNSAMPLED: u16 = 1 << 15; // 降采样标记（自动降采样产物）
const FLAG_DRAG: u16 = 1 << 14; // 窗口拖动专项标签
const FLAG_BREAK: u16 = 1 << 13; // 断点标记（崩溃重启，不伪造连续）
const DIRTY_MASK: u16 = 0x03FF;

// ---------------------------------------------------------------------------
// 记录
// ---------------------------------------------------------------------------

/// 一帧的四项测量（分段计时，微秒）。测量点三处 + 等待：
/// 输入处理 → 合成 → 提交，等待时间独立记录（不做红线）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameSpans {
    pub input_us: u16,
    pub compose_us: u16,
    pub commit_us: u16,
    pub wait_us: u16,
    /// 脏区面积，千分屏（0..=1000）。
    pub dirty_permille: u16,
}

impl FrameSpans {
    /// 帧总耗时（输入+合成+提交；等待不计入红线判定，主册四项口径）。
    pub fn busy_us(&self) -> u32 {
        self.input_us as u32 + self.compose_us as u32 + self.commit_us as u32
    }
    /// 任一段超独立红线（「哪一段慢」的归因最小粒度）。
    pub fn any_segment_over(&self) -> bool {
        self.compose_us > REDLINE_COMPOSE_US
            || self.commit_us > REDLINE_COMMIT_US
            || self.input_us > REDLINE_INPUT_US
    }
}

/// 10 字节打包记录（LE；标志位与脏区共用 u16）。
#[derive(Clone, Copy)]
struct RawRecord([u8; RECORD_BYTES]);

impl RawRecord {
    const BREAK: RawRecord = RawRecord([0, 0, 0, 0, 0, 0, 0, 0, 0, 0x20]); // FLAG_BREAK=1<<13，LE 编码（与 encode 的 to_le_bytes 一致）

    fn encode(s: &FrameSpans, flags: u16) -> RawRecord {
        let mut b = [0u8; RECORD_BYTES];
        b[0..2].copy_from_slice(&s.input_us.to_le_bytes());
        b[2..4].copy_from_slice(&s.compose_us.to_le_bytes());
        b[4..6].copy_from_slice(&s.commit_us.to_le_bytes());
        b[6..8].copy_from_slice(&s.wait_us.to_le_bytes());
        let dirty = (s.dirty_permille.min(DIRTY_PERMILLE_MAX) & DIRTY_MASK) | flags;
        b[8..10].copy_from_slice(&dirty.to_le_bytes());
        RawRecord(b)
    }
    fn input_us(&self) -> u16 {
        u16::from_le_bytes([self.0[0], self.0[1]])
    }
    fn compose_us(&self) -> u16 {
        u16::from_le_bytes([self.0[2], self.0[3]])
    }
    fn commit_us(&self) -> u16 {
        u16::from_le_bytes([self.0[4], self.0[5]])
    }
    fn wait_us(&self) -> u16 {
        u16::from_le_bytes([self.0[6], self.0[7]])
    }
    fn flags(&self) -> u16 {
        u16::from_le_bytes([self.0[8], self.0[9]]) & !DIRTY_MASK
    }
    fn dirty_permille(&self) -> u16 {
        u16::from_le_bytes([self.0[8], self.0[9]]) & DIRTY_MASK
    }
    fn is_break(&self) -> bool {
        self.flags() & FLAG_BREAK != 0
    }
}

// ---------------------------------------------------------------------------
// 分钟聚合
// ---------------------------------------------------------------------------

/// 一分钟的聚合行（主册：分钟聚合落账本文件；格式对齐 vxbench 供 F061 回归门消费）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MinuteAgg {
    pub minute_index: u64, // 自账本纪元的分钟序号
    pub frames: u32,
    pub avg_busy_us: u32,
    pub max_busy_us: u32,
    /// 超过 80fps 线（12.5ms）的帧数。
    pub over_frames: u32,
    /// 输入事件率峰值（事件/秒，F042 输入风暴证据同源）。
    pub input_events_peak: u32,
}

#[derive(Clone, Copy)]
struct MinuteSlot {
    epoch_min: u64,
    frames: u32,
    sum_busy: u64, // 微秒累计，u64 防 60k fps×60s 溢出
    max_busy: u32,
    over: u32,
    input_peak: u32,
}
impl MinuteSlot {
    const fn empty() -> Self {
        MinuteSlot { epoch_min: u64::MAX, frames: 0, sum_busy: 0, max_busy: 0, over: 0, input_peak: 0 }
    }
    fn flush(&self) -> Option<MinuteAgg> {
        if self.epoch_min == u64::MAX || self.frames == 0 {
            return None;
        }
        Some(MinuteAgg {
            minute_index: self.epoch_min,
            frames: self.frames,
            avg_busy_us: (self.sum_busy / self.frames as u64) as u32,
            max_busy_us: self.max_busy,
            over_frames: self.over,
            input_events_peak: self.input_peak,
        })
    }
}

// ---------------------------------------------------------------------------
// 账本本体
// ---------------------------------------------------------------------------

/// 帧率账本。合成器每帧调用一次 [`FrameLedger::record`]；监视器与 F042 归因器
/// 是订阅消费者（B-2001：一套账本多处消费，数字永远同源）。
pub struct FrameLedger {
    ring: [RawRecord; RING_CAP],
    head: usize,   // 下一条写入位
    filled: usize, // 已填满量（≤ RING_CAP）
    seq: u64,      // 全局帧序号（含降采样跳过的帧）
    minute: [MinuteSlot; MINUTE_BUCKETS],
    minute_cursor: usize, // 下一分钟聚合槽（环形复用最旧）
    /// 打点自身开销模型累计（周期数）。
    self_cost_cycles: u64,
    /// 自动降采样比（1 = 全记；N = 每 N 帧记 1）。
    downsample_ratio: u32,
    drag_tag: bool,
    /// 最近一次合成器崩溃标注存在标志（诚实呈现「不伪造连续」）。
    has_break: bool,
    cpu_hz: u64,
}

impl FrameLedger {
    /// 创建账本。`cpu_hz` 用于打点开销核算（宿主模型可传 0 → 按 4GHz 折算）。
    pub const fn new(cpu_hz: u64) -> Self {
        FrameLedger {
            ring: [RawRecord([0; RECORD_BYTES]); RING_CAP],
            head: 0,
            filled: 0,
            seq: 0,
            minute: [MinuteSlot::empty(); MINUTE_BUCKETS],
            minute_cursor: 0,
            self_cost_cycles: 0,
            downsample_ratio: 1,
            drag_tag: false,
            has_break: false,
            cpu_hz,
        }
    }

    fn cpu_hz_or_default(&self) -> u64 {
        if self.cpu_hz == 0 { 4_000_000_000 } else { self.cpu_hz }
    }

    /// 窗口拖动专项标签开关（压力测试复现用，主册设计细节）。
    pub fn set_drag_scenario(&mut self, on: bool) {
        self.drag_tag = on;
    }

    /// 自适应降采样比：保证 60s 窗口（ratio = ceil(fps×60/CAP)，下限 1）。
    pub fn ratio_for_fps(fps: u32) -> u32 {
        if fps == 0 {
            return 1;
        }
        let need = (fps as u64) * 60;
        if need <= RING_CAP as u64 {
            1
        } else {
            ((need + RING_CAP as u64 - 1) / RING_CAP as u64) as u32
        }
    }

    /// 打点开销核算入口：合成器每帧上报本次打点消耗的周期数。
    /// 超预算（≥0.1% CPU 折算每帧上限）→ 自动降采样每 2 帧记 1 帧并标注
    /// （主册【状态与异常】）。返回是否触发了降采样。
    pub fn note_self_cost(&mut self, cycles_this_frame: u64) -> bool {
        self.self_cost_cycles = self.self_cost_cycles.saturating_add(cycles_this_frame);
        // 每帧预算：0.1% CPU → 每帧可打点的周期上限 =
        // cpu_hz × 0.001 ÷ fps。fps 未知时按保守 120fps 折算（打点本身
        // 不该知道帧率——预算线按主册 0.1% 硬线）。
        let per_frame_cap = self.cpu_hz_or_default() / 1_000 / 120;
        if cycles_this_frame > per_frame_cap && self.downsample_ratio < 2 {
            self.downsample_ratio = 2; // 每 2 帧记 1 帧（主册原文）
            return true;
        }
        false
    }

    /// 打点自开销（千分 CPU，模型口径：累计周期 ÷ 已记帧 ÷ 每帧时间预算）。
    pub fn self_cost_permille(&self) -> u32 {
        if self.seq == 0 {
            return 0;
        }
        // 模型：每帧打点均摊周期 ÷ (cpu_hz/1000) 的每毫秒周期数 → permille
        // 以「每帧 12.5ms 预算内打点占 CPU 时间」计。
        let per_frame = self.self_cost_cycles / self.seq.max(1);
        let frame_budget_cycles = self.cpu_hz_or_default() / 80; // 12.5ms @ 80fps
        ((per_frame * 1000) / frame_budget_cycles.max(1)) as u32
    }

    /// 记录一帧。`now_ms` 为系统毫秒钟（分钟聚合时间轴）。降采样生效时按
    /// 序号取模跳帧（跳过的帧不产生记录，序号仍前进——诚实不补齐）。
    pub fn record(&mut self, spans: FrameSpans, now_ms: u64, input_events_this_sec: u32) {
        self.seq = self.seq.wrapping_add(1);
        if self.downsample_ratio > 1 && self.seq % self.downsample_ratio as u64 != 0 {
            self.aggregate_minute(spans, now_ms, input_events_this_sec, true);
            return;
        }
        let flags = if self.drag_tag { FLAG_DRAG } else { 0 }
            | if self.downsample_ratio > 1 { FLAG_DOWNSAMPLED } else { 0 };
        self.push(RawRecord::encode(&spans, flags));
        self.aggregate_minute(spans, now_ms, input_events_this_sec, false);
    }

    /// 合成器崩溃重启标注（主册：账本标注断点，不伪造连续）。
    pub fn mark_break(&mut self) {
        self.push(RawRecord::BREAK);
        self.has_break = true;
    }

    fn push(&mut self, rec: RawRecord) {
        self.ring[self.head] = rec;
        self.head = (self.head + 1) % RING_CAP;
        self.filled = (self.filled + 1).min(RING_CAP);
    }

    fn aggregate_minute(&mut self, spans: FrameSpans, now_ms: u64, input_events: u32, skipped: bool) {
        let epoch_min = now_ms / 60_000;
        let slot_idx = (epoch_min as usize) % MINUTE_BUCKETS;
        let slot = &mut self.minute[slot_idx];
        if slot.epoch_min != epoch_min {
            // 首次进入该分钟槽（或 24h 回绕重置）：旧数据视为已被 flush 消费。
            // cursor 恒指向最新有效槽——minute_snapshot 由此从最旧到最新升序遍历。
            *slot = MinuteSlot::empty();
            slot.epoch_min = epoch_min;
            self.minute_cursor = slot_idx;
        }
        if !skipped {
            slot.frames += 1;
            slot.sum_busy += spans.busy_us() as u64;
            slot.max_busy = slot.max_busy.max(spans.busy_us());
            if spans.busy_us() > LINE_80FPS_US {
                slot.over += 1;
            }
        }
        slot.input_peak = slot.input_peak.max(input_events);
    }

    /// 取最近 `n` 帧记录（时间升序；供监视器逐帧图与「抽 10 帧核对」）。
    pub fn recent(&self, n: usize) -> RecentIter<'_> {
        let avail = self.filled.min(n);
        RecentIter { ledger: self, left: avail, pos: (self.head + RING_CAP - avail) % RING_CAP }
    }

    /// 帧序号（含跳帧的全局序）。
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// 断点标注是否在册（诚实呈现）。
    pub fn has_break(&self) -> bool {
        self.has_break
    }

    /// 当前降采样比。
    pub fn downsample(&self) -> u32 {
        self.downsample_ratio
    }

    /// 监视器逐帧图数据：最近 `points` 帧的 busy 微秒（升序）。
    /// 断点在 series 中以 `None` 呈现——图形断线即断点，不伪造连续。
    pub fn busy_series(&self, points: usize) -> ([Option<u32>; 120], usize) {
        let mut out = [None; 120];
        let n = points.min(120).min(self.filled);
        let start = (self.head + RING_CAP - n) % RING_CAP;
        for i in 0..n {
            let r = &self.ring[(start + i) % RING_CAP];
            out[i] = if r.is_break() {
                None
            } else {
                Some(r.input_us() as u32 + r.compose_us() as u32 + r.commit_us() as u32)
            };
        }
        (out, n)
    }

    /// 分钟聚合快照：把已聚合的分钟行拷给消费者（vxbench / F061 回归门口径）。
    pub fn minute_snapshot(&self, out: &mut [MinuteAgg]) -> usize {
        let mut n = 0;
        for i in 0..MINUTE_BUCKETS {
            let idx = (self.minute_cursor + 1 + i) % MINUTE_BUCKETS;
            if let Some(agg) = self.minute[idx].flush() {
                if n < out.len() {
                    out[n] = agg;
                    n += 1;
                }
            }
        }
        n
    }
}

/// 最近记录迭代器（升序，跳过解释：断点记录原样给出由消费者判定）。
pub struct RecentIter<'a> {
    ledger: &'a FrameLedger,
    left: usize,
    pos: usize,
}
impl<'a> Iterator for RecentIter<'a> {
    type Item = RawFrame<'a>;
    fn next(&mut self) -> Option<RawFrame<'a>> {
        if self.left == 0 {
            return None;
        }
        let r = &self.ledger.ring[self.pos];
        self.pos = (self.pos + 1) % RING_CAP;
        self.left -= 1;
        Some(RawFrame { r })
    }
}

/// 只读帧视图（消费面：监视器/F042 归因器）。
pub struct RawFrame<'a> {
    r: &'a RawRecord,
}
impl<'a> RawFrame<'a> {
    pub fn input_us(&self) -> u16 {
        self.r.input_us()
    }
    pub fn compose_us(&self) -> u16 {
        self.r.compose_us()
    }
    pub fn commit_us(&self) -> u16 {
        self.r.commit_us()
    }
    pub fn wait_us(&self) -> u16 {
        self.r.wait_us()
    }
    pub fn dirty_permille(&self) -> u16 {
        self.r.dirty_permille()
    }
    pub fn is_break(&self) -> bool {
        self.r.is_break()
    }
    pub fn is_drag(&self) -> bool {
        self.r.flags() & FLAG_DRAG != 0
    }
    pub fn is_downsampled(&self) -> bool {
        self.r.flags() & FLAG_DOWNSAMPLED != 0
    }
    pub fn busy_us(&self) -> u32 {
        self.r.input_us() as u32 + self.r.compose_us() as u32 + self.r.commit_us() as u32
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检（CheckSet 面向 KernelCheckup/robust 注册）。
pub fn run_frameledger_checks() -> CheckSet {
    let mut cs = CheckSet::new("F041-frameledger");
    // 1) 64KB 硬预算：环形容量 = 65536/10。
    cs.add("cap_64kb", RING_CAP == 6553 && core::mem::size_of::<FrameLedger>() > 0, "");
    // 2) 抽 10 帧核对：记录 10 帧已知跨度，逐帧读回精确一致。
    let mut lg = FrameLedger::new(0);
    for i in 0..10u16 {
        lg.record(
            FrameSpans { input_us: 100 + i, compose_us: 2000 + i, commit_us: 900 + i, wait_us: 500, dirty_permille: 12 },
            1_000 + i as u64, 30,
        );
    }
    let mut ok10 = true;
    for (i, f) in lg.recent(10).enumerate() {
        ok10 &= f.input_us() == 100 + i as u16
            && f.compose_us() == 2000 + i as u16
            && f.commit_us() == 900 + i as u16
            && f.dirty_permille() == 12;
    }
    cs.add("sample10_exact", ok10, "");
    // 3) 打点自开销 <0.1% CPU（模型口径）。
    let mut lg2 = FrameLedger::new(4_000_000_000);
    for _ in 0..1000 {
        lg2.record(FrameSpans { input_us: 800, compose_us: 4000, commit_us: 1500, wait_us: 6000, dirty_permille: 30 }, 0, 0);
        lg2.note_self_cost(500); // 500 周期/帧 @4GHz ≈ 0.125us ≪ 预算
    }
    cs.add("self_cost_below_01pct", lg2.self_cost_permille() < SELF_COST_PERMILLE_CAP, "");
    // 4) 超预算自动降采样（每 2 帧记 1，标注位在册）。
    let mut lg3 = FrameLedger::new(4_000_000_000);
    let mut triggered = false;
    for _ in 0..8 {
        lg3.record(FrameSpans { input_us: 100, compose_us: 2000, commit_us: 100, wait_us: 0, dirty_permille: 1 }, 0, 0);
        triggered |= lg3.note_self_cost(4_000_000_000 / 1_000 / 120 + 1);
    }
    lg3.record(FrameSpans { input_us: 100, compose_us: 2000, commit_us: 100, wait_us: 0, dirty_permille: 1 }, 0, 0);
    cs.add("over_budget_downsample", triggered && lg3.downsample() == 2, "");
    // 5) 80fps 常量与双线语义（12.5ms 实线 / 16.6ms 虚线）。
    cs.add("fps_lines", LINE_80FPS_US == 12_500 && LINE_60FPS_US == 16_600, "");
    // 6) 独立阈值三处（合成 6ms/提交 3ms/输入 1.5ms）。
    let seg = FrameSpans { input_us: 1_501, compose_us: 0, commit_us: 0, wait_us: 0, dirty_permille: 0 };
    let seg2 = FrameSpans { input_us: 0, compose_us: 6_001, commit_us: 0, wait_us: 0, dirty_permille: 0 };
    let seg3 = FrameSpans { input_us: 0, compose_us: 0, commit_us: 3_001, wait_us: 0, dirty_permille: 0 };
    cs.add("segment_thresholds", seg.any_segment_over() && seg2.any_segment_over() && seg3.any_segment_over(), "");
    // 7) 断点标注：不伪造连续（series 中断点为 None）。
    let mut lg4 = FrameLedger::new(0);
    lg4.record(FrameSpans { input_us: 100, compose_us: 100, commit_us: 100, wait_us: 0, dirty_permille: 0 }, 0, 0);
    lg4.mark_break();
    lg4.record(FrameSpans { input_us: 100, compose_us: 100, commit_us: 100, wait_us: 0, dirty_permille: 0 }, 0, 0);
    let (series, n) = lg4.busy_series(3);
    cs.add("break_not_faked", lg4.has_break() && n == 3 && series[0].is_some() && series[1].is_none() && series[2].is_some(), "");
    // 8) 分钟聚合：均值/峰值/超线帧计数正确。
    let mut lg5 = FrameLedger::new(0);
    for i in 0..4u64 {
        // busy = input+compose+commit；compose 12_400..12_700 → busy 12_600..12_900，
        // 四帧全部越过 12.5ms（LINE_80FPS_US）线，over_frames 覆盖真实计数。
        lg5.record(FrameSpans { input_us: 100, compose_us: 12_400 + i as u16 * 100, commit_us: 100, wait_us: 0, dirty_permille: 0 }, 1_000, 42);
    }
    let mut aggs = [MinuteAgg::default(); 4];
    let cnt = lg5.minute_snapshot(&mut aggs);
    let agg = aggs[0];
    cs.add(
        "minute_agg",
        cnt == 1 && agg.frames == 4 && agg.max_busy_us == 12_900 && agg.over_frames == 4 && agg.input_events_peak == 42,
        "",
    );
    // 9) 自适应降采样保 60s：1000fps → ratio=10（ceil(60_000/6553)）。
    cs.add("adaptive_ratio_1000fps", FrameLedger::ratio_for_fps(1000) == 10 && FrameLedger::ratio_for_fps(80) == 1, "");
    cs
}

// ---------------------------------------------------------------------------
// 宿主单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_capacity_is_64kb_budget() {
        assert_eq!(RING_CAP, 6553);
        assert_eq!(RING_CAP * RECORD_BYTES, 65_530); // ≤ 65536
    }

    #[test]
    fn sample_ten_frames_roundtrip_exact() {
        let mut lg = FrameLedger::new(0);
        for i in 0..10u16 {
            lg.record(
                FrameSpans { input_us: 100 + i, compose_us: 2000 + i, commit_us: 900 + i, wait_us: 500, dirty_permille: 12 },
                1_000, 30,
            );
        }
        let got: Vec<u32> = lg.recent(10).map(|f| f.busy_us()).collect();
        let want: Vec<u32> = (0..10u16).map(|i| (100 + i + 2000 + i + 900 + i) as u32).collect();
        assert_eq!(got, want);
    }

    #[test]
    fn drag_tag_marks_records() {
        let mut lg = FrameLedger::new(0);
        lg.record(FrameSpans { input_us: 1, compose_us: 1, commit_us: 1, wait_us: 0, dirty_permille: 0 }, 0, 0);
        lg.set_drag_scenario(true);
        lg.record(FrameSpans { input_us: 1, compose_us: 1, commit_us: 1, wait_us: 0, dirty_permille: 0 }, 0, 0);
        let frames: Vec<_> = lg.recent(2).collect();
        assert!(!frames[0].is_drag() && frames[1].is_drag());
    }

    #[test]
    fn downsampling_keeps_60s_window_and_marks() {
        // 1000fps：ratio = ceil(60_000/6553) = 10，记 60s 的量（60000 帧序）
        // → 环内 6000 采样点 = 60s×100/s —— 窗口恰好保住。
        let mut lg = FrameLedger::new(0);
        lg.downsample_ratio = FrameLedger::ratio_for_fps(1000);
        assert_eq!(lg.downsample_ratio, 10);
        for i in 0..60_000u64 {
            lg.record(FrameSpans { input_us: 10, compose_us: 20, commit_us: 10, wait_us: 0, dirty_permille: 0 }, i, 0);
        }
        let cnt = lg.recent(usize::MAX).count();
        assert_eq!(cnt, 6000, "60s @1000fps downsampled to 100/s");
        assert!(lg.recent(10).all(|f| f.is_downsampled()));
    }

    #[test]
    fn self_cost_guard_triggers_downsample_once() {
        let mut lg = FrameLedger::new(4_000_000_000);
        lg.record(FrameSpans { input_us: 1, compose_us: 1, commit_us: 1, wait_us: 0, dirty_permille: 0 }, 0, 0);
        assert!(!lg.note_self_cost(100));
        assert!(lg.note_self_cost(u64::MAX / 2)); // 超预算
        assert_eq!(lg.downsample(), 2);
        // 再超不再变化（只降一次，主册口径每 2 帧记 1 帧）。
        assert!(!lg.note_self_cost(u64::MAX / 2));
        assert_eq!(lg.downsample(), 2);
    }

    #[test]
    fn minute_buckets_roll_across_24h() {
        let mut lg = FrameLedger::new(0);
        // 两个分钟桶 + 跨 24h 回绕同桶位。
        lg.record(FrameSpans { input_us: 100, compose_us: 100, commit_us: 100, wait_us: 0, dirty_permille: 0 }, 30_000, 5);
        lg.record(FrameSpans { input_us: 100, compose_us: 100, commit_us: 100, wait_us: 0, dirty_permille: 0 }, 90_000, 7);
        let mut out = [MinuteAgg::default(); 8];
        let n = lg.minute_snapshot(&mut out);
        assert_eq!(n, 2);
        assert_eq!(out[0].minute_index, 0);
        assert_eq!(out[1].minute_index, 1);
        assert_eq!(out[1].input_events_peak, 7);
        // 24h 后同桶位不串账：epoch 不同，槽被重置。
        lg.record(FrameSpans { input_us: 100, compose_us: 100, commit_us: 100, wait_us: 0, dirty_permille: 0 }, (1_440 + 1) as u64 * 60_000, 9);
        let mut out2 = [MinuteAgg::default(); 8];
        let n2 = lg.minute_snapshot(&mut out2);
        assert!(n2 >= 2);
        assert!(out2[..n2].iter().any(|a| a.minute_index == 1_441 && a.input_events_peak == 9));
    }

    #[test]
    fn overflow_input_clamps_at_1000_permille() {
        let mut lg = FrameLedger::new(0);
        lg.record(FrameSpans { input_us: 0, compose_us: 0, commit_us: 0, wait_us: 0, dirty_permille: 5_000 }, 0, 0);
        assert_eq!(lg.recent(1).next().unwrap().dirty_permille(), DIRTY_PERMILLE_MAX);
    }
}
