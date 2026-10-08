//! STAR-K2 共享底盘：分钟账本、旋钮注册表、环形事件日志、分位数工具。
//!
//! 十八项功能共用四件基础设施，全部零外部依赖、宿主测试直跑、内核
//! 镜像（no_std + alloc）可编译：
//!
//! - [`MinuteBook`] 分钟聚合账本——F059/F060/F062/F064/F065/F069 的
//!   「统计入账本分钟聚合；保留 30 天」共用同一条存储通路（一处一事实）；
//! - [`KnobReg`] 旋钮注册表——主册反复出现的「参数进旋钮清单」落位，
//!   钳制入档、变更留痕、版本号供 F069 拒切判据消费；
//! - [`RingLog`] 定容环形事件日志——唤醒原因、恢复记录等「最近 N 条」面；
//! - [`pct_near`] 最近邻分位数——P95/P99 判据的统一算法（最近邻秩，
//!   与主册 80fps 定义 P95≤12.5ms 的计算口径一致，全域只此一处）。
//!
//! 时间纪律：**一切时间由调用方以参数注入**（分钟戳/微秒戳），模块不持
//! 真实时钟——宿主测试可确定复现，内核侧由上层供给真值。

use alloc::vec;
use alloc::vec::Vec;
use core::cmp::Ordering;

// ---------------------------------------------------------------------------
// MinuteBook — 分钟聚合账本
// ---------------------------------------------------------------------------

/// 分钟槽：一个时间戳 + 一组计数器。
#[derive(Clone, Debug)]
struct MinuteSlot {
    stamp: u64,
    vals: Vec<u64>,
}

/// 分钟聚合账本：按分钟戳稀疏存槽，槽内定长计数器组。
///
/// 容量语义：`cap_minutes` 是保留窗口（默认 30 天 = 43,200 分钟），
/// `evict_to` 把窗口外的槽整体丢弃——「画像每分钟聚合入账本；保留
/// 30 天」的通用实现。同分钟重复记录为**累加**（merge）不是覆盖。
pub struct MinuteBook {
    stride: usize,
    cap_minutes: u64,
    slots: Vec<MinuteSlot>,
    /// 因累加被拒绝的越界计数器写次数（诊断面诚实记账）。
    pub clamped_writes: u64,
}

impl MinuteBook {
    /// 建账本：`stride` 个计数器/分钟，保留 `cap_minutes` 分钟。
    /// stride 为 0 或 cap 为 0 视为非法，回退最小可用账本（1 分钟 1 计数器）。
    pub fn new(stride: usize, cap_minutes: u64) -> MinuteBook {
        let stride = if stride == 0 { 1 } else { stride };
        let cap = if cap_minutes == 0 { 1 } else { cap_minutes };
        MinuteBook {
            stride,
            cap_minutes: cap,
            slots: Vec::new(),
            clamped_writes: 0,
        }
    }

    pub fn stride(&self) -> usize {
        self.stride
    }

    pub fn cap_minutes(&self) -> u64 {
        self.cap_minutes
    }

    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// 记录一分钟数据。`vals` 长度不足按缺省 0 补齐，超出部分丢弃计数。
    pub fn record_minute(&mut self, stamp: u64, vals: &[u64]) {
        if vals.len() > self.stride {
            self.clamped_writes += 1;
        }
        match self.slots.binary_search_by(|s| s.stamp.cmp(&stamp)) {
            Ok(idx) => {
                let slot = &mut self.slots[idx];
                for i in 0..self.stride.min(slot.vals.len()) {
                    let v = vals.get(i).copied().unwrap_or(0);
                    slot.vals[i] = slot.vals[i].saturating_add(v);
                }
            }
            Err(pos) => {
                let mut full = vec![0u64; self.stride];
                for i in 0..self.stride {
                    full[i] = vals.get(i).copied().unwrap_or(0);
                }
                self.slots.insert(pos, MinuteSlot { stamp, vals: full });
            }
        }
    }

    /// 把 `[from_min, to_min]` 区间按计数器逐列求和（区间外槽不参与）。
    pub fn range_sum(&self, from_min: u64, to_min: u64) -> Vec<u64> {
        let mut out = vec![0u64; self.stride];
        if from_min > to_min {
            return out;
        }
        for slot in &self.slots {
            if slot.stamp < from_min {
                continue;
            }
            if slot.stamp > to_min {
                break;
            }
            for i in 0..self.stride.min(slot.vals.len()) {
                out[i] = out[i].saturating_add(slot.vals[i]);
            }
        }
        out
    }

    /// 最近 `n` 个槽（新→旧）。窗口大于现存槽数时按现存返回。
    pub fn latest(&self, n: usize) -> Vec<(u64, &[u64])> {
        let mut out = Vec::new();
        let start = self.slots.len().saturating_sub(n);
        for slot in self.slots[start..].iter().rev() {
            out.push((slot.stamp, slot.vals.as_slice()));
        }
        out
    }

    /// 驱逐 `cutoff_min` 之前的槽（保留窗口外数据不再驻内存）。
    pub fn evict_to(&mut self, cutoff_min: u64) -> usize {
        let before = self.slots.len();
        self.slots.retain(|s| s.stamp >= cutoff_min);
        before - self.slots.len()
    }

    /// 按绝对时间窗对齐驱逐：`now_min` 之前 `cap_minutes` 外的槽全丢。
    pub fn evict_by_now(&mut self, now_min: u64) -> usize {
        let cutoff = now_min.saturating_sub(self.cap_minutes);
        self.evict_to(cutoff)
    }
}

// ---------------------------------------------------------------------------
// KnobReg — 旋钮注册表
// ---------------------------------------------------------------------------

/// 单个旋钮：名、当前值、边界、缺省、单位与备注。
#[derive(Clone, Copy, Debug)]
pub struct Knob {
    pub name: &'static str,
    pub val: i64,
    pub min: i64,
    pub max: i64,
    pub def: i64,
    pub unit: &'static str,
    pub note: &'static str,
}

impl Knob {
    /// 把 `v` 钳制进 `[min, max]`。
    pub fn clamp(&self, v: i64) -> i64 {
        v.clamp(self.min, self.max)
    }
}

/// 一次旋钮变更（审计留痕：改了什么、从几到几、何时）。
#[derive(Clone, Copy, Debug)]
pub struct KnobChange {
    pub knob: &'static str,
    pub from: i64,
    pub to: i64,
    pub stamp: u64,
}

/// 旋钮注册表：所有可调参数的唯一户口。
///
/// 纪律：`set` 永远成功但**钳制入档**（越界值贴边界），并把每次真实
/// 变化写入变更留痕（定容环）；`version` 随声明集变化递增——F069 的
/// 「档位参数缺失（旋钮表版本不齐）→ 拒绝切换」以此为判据源。
pub struct KnobReg {
    knobs: Vec<Knob>,
    changes: [Option<KnobChange>; KNOB_CHANGE_RING],
    change_head: usize,
    change_len: usize,
    version: u32,
}

/// 变更留痕环容量（40 条足够覆盖一次档位切换的全联动写入）。
pub const KNOB_CHANGE_RING: usize = 40;

impl KnobReg {
    pub fn new() -> KnobReg {
        KnobReg {
            knobs: Vec::new(),
            changes: [None; KNOB_CHANGE_RING],
            change_head: 0,
            change_len: 0,
            version: 0,
        }
    }

    /// 声明（或幂等重声明）一个旋钮。重复声明同名单同参数为 no-op；
    /// 同名不同参视为调用方缺陷，按覆盖处理并把 version 推进。
    pub fn declare(
        &mut self,
        name: &'static str,
        def: i64,
        min: i64,
        max: i64,
        unit: &'static str,
        note: &'static str,
    ) {
        let (lo, hi) = if min <= max { (min, max) } else { (max, min) };
        let def = def.clamp(lo, hi);
        if let Some(k) = self.knobs.iter_mut().find(|k| k.name == name) {
            if k.def != def || k.min != lo || k.max != hi {
                *k = Knob { name, val: def, min: lo, max: hi, def, unit, note };
                self.version = self.version.wrapping_add(1);
            }
            return;
        }
        self.knobs.push(Knob { name, val: def, min: lo, max: hi, def, unit, note });
        self.version = self.version.wrapping_add(1);
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn get(&self, name: &str) -> Option<i64> {
        self.knobs.iter().find(|k| k.name == name).map(|k| k.val)
    }

    /// 取值，缺省回退 `fallback`（读路径永不失败——调用方显式给兜底值）。
    pub fn get_or(&self, name: &str, fallback: i64) -> i64 {
        self.get(name).unwrap_or(fallback)
    }

    pub fn knob(&self, name: &str) -> Option<&Knob> {
        self.knobs.iter().find(|k| k.name == name)
    }

    /// 设值：钳制入档、真实变化才留痕。返回生效值。
    pub fn set(&mut self, name: &str, v: i64, stamp: u64) -> Option<i64> {
        let idx = self.knobs.iter().position(|k| k.name == name)?;
        let eff = self.knobs[idx].clamp(v);
        let from = self.knobs[idx].val;
        if eff != from {
            let knob_name = self.knobs[idx].name;
            self.knobs[idx].val = eff;
            self.push_change(KnobChange { knob: knob_name, from, to: eff, stamp });
        }
        Some(eff)
    }

    /// 单旋钮复位缺省。
    pub fn reset(&mut self, name: &str, stamp: u64) -> Option<i64> {
        let d = self.knob(name)?.def;
        self.set(name, d, stamp)
    }

    /// 全量复位（档位切换装载参数表用）；返回复位个数。
    pub fn reset_all(&mut self, stamp: u64) -> usize {
        let defs: Vec<(&'static str, i64)> =
            self.knobs.iter().map(|k| (k.name, k.def)).collect();
        let mut n = 0;
        for (name, d) in defs {
            if self.set(name, d, stamp).is_some() {
                n += 1;
            }
        }
        n
    }

    fn push_change(&mut self, c: KnobChange) {
        self.changes[self.change_head] = Some(c);
        self.change_head = (self.change_head + 1) % KNOB_CHANGE_RING;
        self.change_len = (self.change_len + 1).min(KNOB_CHANGE_RING);
    }

    /// 变更留痕（新→旧，最多 KNOB_CHANGE_RING 条）。
    pub fn change_log(&self) -> Vec<KnobChange> {
        let mut out = Vec::new();
        for i in 0..self.change_len {
            let idx = (self.change_head + KNOB_CHANGE_RING - 1 - i) % KNOB_CHANGE_RING;
            if let Some(c) = self.changes[idx] {
                out.push(c);
            }
        }
        out
    }

    /// 全部旋钮快照（诊断面/档位摘要悬浮）。
    pub fn snapshot(&self) -> Vec<Knob> {
        self.knobs.clone()
    }
}

impl Default for KnobReg {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// RingLog — 定容环形事件日志
// ---------------------------------------------------------------------------

/// 定容环形事件日志：满了覆最旧，读取恒为新→旧。
///
/// `T: Copy` —— 与既有 power/x2wakesrc 同纪律：事件记录全部小拷贝体，
/// 不进堆。容量在类型层冻结，运行期零分配。
pub struct RingLog<T: Copy, const N: usize> {
    buf: [Option<T>; N],
    head: usize,
    len: usize,
}

impl<T: Copy, const N: usize> RingLog<T, N> {
    pub fn new() -> RingLog<T, N> {
        RingLog { buf: [const { None }; N], head: 0, len: 0 }
    }

    pub fn push(&mut self, item: T) {
        self.buf[self.head] = Some(item);
        self.head = (self.head + 1) % N;
        self.len = (self.len + 1).min(N);
    }

    /// 最新在前（index 0 = 最新一条）。
    pub fn newest_first(&self) -> Vec<T> {
        let mut out = Vec::new();
        for i in 0..self.len {
            let idx = (self.head + N - 1 - i) % N;
            if let Some(t) = self.buf[idx] {
                out.push(t);
            }
        }
        out
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn clear(&mut self) {
        self.buf = [const { None }; N];
        self.head = 0;
        self.len = 0;
    }
}

impl<T: Copy, const N: usize> Default for RingLog<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 分位数与直方
// ---------------------------------------------------------------------------

/// 最近邻秩分位数：`pct`∈[0,100]，输入**无需预排序**（内部排序副本）。
///
/// 全域统一口径：P95 = 最近邻秩法。空样本回 0（调用方以样本数>0 判据
/// 自行把关——0 样本的 P99 没有意义，不该伪装成达标）。
pub fn pct_near(samples: &[u64], pct: u32) -> u64 {
    if samples.is_empty() {
        return 0;
    }
    let pct = pct.min(100) as usize;
    let mut v = Vec::with_capacity(samples.len());
    v.extend_from_slice(samples);
    v.sort_unstable();
    let rank = (v.len() * pct + 99) / 100; // ceil，最近邻秩
    let rank = rank.clamp(1, v.len());
    v[rank - 1]
}

/// 延迟直方：等宽桶 + 溢出桶。
pub struct Hist {
    /// 桶宽（单位与样本一致，微秒常见）。
    pub bucket_us: u32,
    counts: Vec<u64>,
    overflow: u64,
    total: u64,
    max: u64,
}

impl Hist {
    /// `buckets` 个等宽桶，超出最高桶的样本计入溢出桶（不丢弃——
    /// 「溢出」本身是诊断信息）。
    pub fn new(bucket_us: u32, buckets: usize) -> Hist {
        let buckets = buckets.max(1);
        Hist {
            bucket_us: if bucket_us == 0 { 1 } else { bucket_us },
            counts: vec![0; buckets],
            overflow: 0,
            total: 0,
            max: 0,
        }
    }

    pub fn record(&mut self, v_us: u64) {
        let idx = (v_us / self.bucket_us as u64) as usize;
        if idx < self.counts.len() {
            self.counts[idx] += 1;
        } else {
            self.overflow += 1;
        }
        self.total += 1;
        self.max = self.max.max(v_us);
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn overflow_count(&self) -> u64 {
        self.overflow
    }

    pub fn max_seen(&self) -> u64 {
        self.max
    }

    /// 桶计数（诊断面渲染分布图用）。
    pub fn bucket_counts(&self) -> &[u64] {
        &self.counts
    }

    /// 超过 `threshold_us` 的样本占比（百万分比 ppm），样本为 0 时回 None。
    pub fn ppm_over(&self, threshold_us: u64) -> Option<u64> {
        if self.total == 0 {
            return None;
        }
        let mut over = self.overflow;
        let start = (threshold_us / self.bucket_us as u64) as usize;
        if start < self.counts.len() {
            // 桶 start 内含 [start*bw, (start+1)*bw)，逐样本精度不做——
            // 桶粒度近似并如实标注（bucket granularity）。
            for c in &self.counts[start + 1..] {
                over += c;
            }
        }
        Some(over * 1_000_000 / self.total)
    }
}

/// 饱和减法（u64）——全域统一用，杜绝 underflow panic。
pub fn sat_sub(a: u64, b: u64) -> u64 {
    a.checked_sub(b).unwrap_or(0)
}

/// 把分钟戳换算成「天」序号（30 天保留窗的通用换算）。
pub fn day_of(stamp_min: u64) -> u64 {
    stamp_min / 1440
}

/// 区间比较辅助：`v` 是否在闭区间 `[lo, hi]`。
pub fn in_range(v: u64, lo: u64, hi: u64) -> bool {
    match lo.cmp(&hi) {
        Ordering::Less | Ordering::Equal => v >= lo && v <= hi,
        Ordering::Greater => v >= hi && v <= lo,
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

/// 共享底盘自检（聚合进 star 域）。
pub fn run_sbase_checks() -> CheckSet {
    let mut set = CheckSet::new("star-sbase");
    let mut book = MinuteBook::new(2, 100);
    book.record_minute(10, &[1, 2]);
    book.record_minute(10, &[3, 4]);
    book.record_minute(5, &[10, 20]);
    set.add("mb merge same minute", book.slot_count() == 2, "");
    let s = book.range_sum(5, 10);
    set.add("mb range sum", s[0] == 14 && s[1] == 26, "");
    set.add("mb evict", { book.evict_to(10); book.slot_count() == 1 }, "");

    let mut reg = KnobReg::new();
    reg.declare("k", 50, 0, 100, "ms", "demo");
    set.add("knob clamp", reg.set("k", 500, 1) == Some(100), "");
    set.add("knob log", reg.change_log().len() == 1, "");
    set.add("knob version", reg.version() >= 1, "");
    set.add("knob reset", reg.reset("k", 2) == Some(50), "");

    let mut ring: RingLog<u32, 4> = RingLog::new();
    for i in 0..6u32 {
        ring.push(i);
    }
    let items = ring.newest_first();
    set.add("ring newest first", items[0] == 5 && items.len() == 4, "");

    let samples = [10u64, 20, 30, 40];
    set.add("pct p50", pct_near(&samples, 50) == 20, "");
    set.add("pct p95", pct_near(&samples, 95) == 40, "");
    set.add("pct p100", pct_near(&samples, 100) == 40, "");
    set.add("pct empty", pct_near(&[], 99) == 0, "");

    let mut h = Hist::new(10, 3);
    h.record(5);
    h.record(15);
    h.record(999);
    set.add("hist buckets", h.bucket_counts()[0] == 1 && h.bucket_counts()[1] == 1, "");
    set.add("hist overflow", h.overflow_count() == 1 && h.total() == 3, "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minute_book_merge_range_evict() {
        let mut b = MinuteBook::new(3, 60);
        b.record_minute(100, &[1, 2, 3]);
        b.record_minute(100, &[1, 1, 1]);
        b.record_minute(101, &[0, 0, 7]);
        assert_eq!(b.slot_count(), 2);
        let s = b.range_sum(100, 101);
        assert_eq!((s[0], s[1], s[2]), (2, 3, 11));
        b.record_minute(200, &[9, 9, 9]);
        assert_eq!(b.evict_by_now(200), 2);
        assert_eq!(b.slot_count(), 1);
    }

    #[test]
    fn minute_book_short_vals_pad_zero() {
        let mut b = MinuteBook::new(2, 10);
        b.record_minute(1, &[5]);
        let s = b.range_sum(0, u64::MAX);
        assert_eq!((s[0], s[1]), (5, 0));
    }

    #[test]
    fn knob_reg_clamp_log_version() {
        let mut r = KnobReg::new();
        r.declare("w", 5, 1, 10, "s", "window");
        assert_eq!(r.get_or("w", 0), 5);
        assert_eq!(r.set("w", 0, 1), Some(1));
        assert_eq!(r.set("w", 99, 2), Some(10));
        let log = r.change_log();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].to, 10);
        assert_eq!(log[0].from, 1);
        r.reset_all(3);
        assert_eq!(r.get_or("w", 0), 5);
        assert!(r.version() >= 1);
    }

    #[test]
    fn knob_redeclare_same_is_noop() {
        let mut r = KnobReg::new();
        r.declare("a", 1, 0, 2, "x", "");
        let v0 = r.version();
        r.declare("a", 1, 0, 2, "x", "");
        assert_eq!(r.version(), v0);
        r.declare("a", 2, 0, 2, "x", "");
        assert_eq!(r.version(), v0 + 1);
        assert_eq!(r.get_or("a", 0), 2);
    }

    #[test]
    fn ring_log_wraparound() {
        let mut ring: RingLog<u64, 3> = RingLog::new();
        for i in 0..7u64 {
            ring.push(i);
        }
        let items = ring.newest_first();
        assert_eq!(items, vec![6, 5, 4]);
        ring.clear();
        assert!(ring.is_empty());
    }

    #[test]
    fn percentile_nearest_rank() {
        let mut v: Vec<u64> = (1..=100).collect();
        assert_eq!(pct_near(&v, 95), 95);
        assert_eq!(pct_near(&v, 99), 99);
        v.truncate(4);
        assert_eq!(pct_near(&v, 95), 4);
    }

    #[test]
    fn hist_ppm_and_sat() {
        let mut h = Hist::new(100, 10);
        for i in 0..1000u64 {
            h.record(i);
        }
        assert_eq!(h.total(), 1000);
        // 10 桶 × 桶宽 100 = 覆盖 0..999，无溢出。
        assert_eq!(h.overflow_count(), 0);
        // 桶粒度近似：阈值 100 → 从桶 2 起计入，桶 2..10 = 800 样本。
        assert_eq!(h.ppm_over(100).unwrap(), 800_000);
        assert_eq!(sat_sub(3, 5), 0);
        assert!(in_range(5, 5, 5));
        assert_eq!(day_of(2900), 2);
    }
}
