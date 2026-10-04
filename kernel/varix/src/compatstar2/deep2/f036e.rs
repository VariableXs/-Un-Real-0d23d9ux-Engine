//! F036 深化批次三 · 遥测聚合面（compatstar2/deep2 · G-A-36）。
//!
//! 批次一深化覆盖 JSON 转义/隐私扫描/合并策略，批次二覆盖 JSONL 帧/
//! 饱和计数/同日去重/v1→v2 迁移；本批补齐【功能定义】「API 使用采样」
//! 全语义对齐的执行/边界/注入面：对数直方图分桶（2 的幂桶界——启动
//! 耗时/内存峰值等遥测量的聚合容器）、P50/P95/P99 分位估计（累积计数
//! 线性插值法，桶内均匀假设）、采样率账本（采样/丢弃/预算三账，预算
//! 10000 条、超预算按 1/8 降采样——主册「超预算自动切低频采样」的执行
//! 面）、会话滚动窗口去重（60s 时间窗同指纹去重，16 槽环形替换最旧）。
//!
//! 判据对账：主册 G-A-36【功能定义】API 使用采样 +【设计细节】采样开销
//! 预算 1% 超限降采样 +【状态与异常】超阈值自动降采样并标注；桶界 2 的
//! 幂为 ETW/性能日志直方图通行口径（MS 性能遥测惯例语义对拍）。
//!
//! 零堆纪律：定长 17 桶 + 定长 16 槽环形，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 直方图桶数 17（桶界为 2 的幂：1,2,4,…,65536；0 值落入首桶）。
pub const BUCKETS: usize = 17;
/// 桶界上沿 65536 = 2^16（超过钳到顶桶并记账——边界显性化）。
pub const TOP_EDGE: u64 = 1 << 16;
/// 桶界表（EDGES[i] = 2^i，单调递增——分桶定位的唯一依据）。
pub const EDGES: [u64; BUCKETS] =
    [1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384, 32768, 65536];
/// 采样预算 10000 条（主册【设计细节】1% 开销预算的量化口径）。
pub const SAMPLE_BUDGET: u32 = 10_000;
/// 降采样掩码 1/8（计数器 & 7 == 0 才入——主册 DOWNSAMPLE_N=8 同源）。
pub const DOWNSAMPLE_MASK: u32 = 7;
/// 会话去重时间窗 60s（滚动窗口——窗内同指纹只记一次）。
pub const SESSION_WINDOW_S: u64 = 60;
/// 会话去重环形容量 16（满则替换最旧——定长滚动纪律）。
pub const SESSION_RING: usize = 16;

// ---------------------------------------------------------------------------
// 对数直方图 + 分位估计
// ---------------------------------------------------------------------------

/// 对数直方图：17 桶计数 + 越界钳顶记账 + 分位估计。
pub struct LogHistogram {
    /// 各桶计数（桶 i 覆盖 (EDGES[i-1], EDGES[i]]；桶 0 含 0 值）。
    pub counts: [u64; BUCKETS],
    /// 总入账条数。
    pub total: u64,
    /// 越界（> 65536）被钳到顶桶的条数（边界记账面）。
    pub clamped: u64,
}

impl LogHistogram {
    pub const fn new() -> Self {
        LogHistogram { counts: [0; BUCKETS], total: 0, clamped: 0 }
    }

    /// 落桶：v ≤ EDGES[i] 的最小 i 即桶 i；v > 65536 钳到顶桶并记账。
    /// 返回落入的桶号。
    pub fn record(&mut self, v: u64) -> usize {
        let mut b = BUCKETS - 1;
        for (i, &e) in EDGES.iter().enumerate() {
            if v <= e {
                b = i;
                break;
            }
        }
        if v > TOP_EDGE {
            self.clamped += 1;
        }
        self.counts[b] += 1;
        self.total += 1;
        b
    }

    /// 分位估计（permille：500/950/990）——累积计数定位桶，桶内按均匀
    /// 假设线性插值，四舍五入（加 count/2 再整除）。空图返回 0。
    pub fn percentile(&self, permille: u32) -> u64 {
        if self.total == 0 {
            return 0;
        }
        let r = (self.total * permille as u64 / 1000).clamp(1, self.total);
        let mut cum = 0u64;
        for i in 0..BUCKETS {
            let c = self.counts[i];
            // 前桶未命中保证 cum < r，故此处 c ≥ r - cum > 0（无除零）。
            if cum + c >= r {
                let low = if i == 0 { 0 } else { EDGES[i - 1] };
                let width = EDGES[i] - low;
                return low + ((r - cum) * width + c / 2) / c;
            }
            cum += c;
        }
        EDGES[BUCKETS - 1]
    }
}

// ---------------------------------------------------------------------------
// 采样率账本
// ---------------------------------------------------------------------------

/// 采样率账本：采样/丢弃/预算三账。预算内全收；超预算按 1/8 降采样
/// （计数器 & 7 == 0 才入）——主册【状态与异常】「超预算自动降采样」。
pub struct SamplingLedger {
    /// 已采样（入账）条数（含超预算期降采样后仍入账的）。
    pub sampled: u32,
    /// 降采样丢弃条数。
    pub dropped: u32,
    /// 超预算期尝试计数（1/8 判定的滚动基准）。
    pub overspill: u32,
}

impl SamplingLedger {
    pub const fn new() -> Self {
        SamplingLedger { sampled: 0, dropped: 0, overspill: 0 }
    }

    /// 尝试入账一条：预算内直收；超预算 overspill & 7 == 0 才入。
    pub fn admit(&mut self) -> bool {
        if self.sampled < SAMPLE_BUDGET {
            self.sampled += 1;
            return true;
        }
        let take = self.overspill & DOWNSAMPLE_MASK == 0;
        if take {
            self.sampled += 1;
        } else {
            self.dropped += 1;
        }
        self.overspill += 1;
        take
    }

    /// 账面自洽（三账闭合）：未触预算期采样+丢弃 == 尝试数；触预算后
    /// 采样+丢弃 == 预算 + 超预算尝试。
    pub fn consistent(&self) -> bool {
        if self.overspill == 0 {
            self.dropped == 0
        } else {
            self.sampled + self.dropped == SAMPLE_BUDGET + self.overspill
        }
    }
}

// ---------------------------------------------------------------------------
// 会话滚动窗口去重
// ---------------------------------------------------------------------------

/// 会话滚动窗口去重：60s 内同指纹去重；定长 16 槽环形，满则替换最旧。
pub struct SessionDedup {
    fp: [u64; SESSION_RING],
    ts: [u64; SESSION_RING],
    cursor: usize,
    /// 环内有效条数（≤ 16）。
    pub count: usize,
    /// 窗口内去重命中数（合并账面）。
    pub hits: u32,
}

impl SessionDedup {
    pub const fn new() -> Self {
        SessionDedup { fp: [0; SESSION_RING], ts: [0; SESSION_RING], cursor: 0, count: 0, hits: 0 }
    }

    /// 登记：true = 60s 窗内已有同指纹（应去重）；false = 新会话（入环，
    /// 环满替换最旧槽位）。
    pub fn record(&mut self, fingerprint: u64, now_s: u64) -> bool {
        for i in 0..self.count {
            if self.fp[i] == fingerprint && now_s.saturating_sub(self.ts[i]) <= SESSION_WINDOW_S {
                self.hits += 1;
                return true;
            }
        }
        let slot = self.cursor;
        self.fp[slot] = fingerprint;
        self.ts[slot] = now_s;
        self.cursor = (self.cursor + 1) % SESSION_RING;
        if self.count < SESSION_RING {
            self.count += 1;
        }
        false
    }
}

/// 域自检（深化批次三）。
pub fn run_f036e_checks() -> CheckSet {
    let mut cs = CheckSet::new("F036-stardraft-d3");
    // 1) 桶界单调且为 2 的幂：17 桶，上沿恰为 65536。
    let edges_pow2 = EDGES.iter().enumerate().all(|(i, &e)| e == 1u64 << i);
    cs.add("hist_edges_pow2", BUCKETS == 17 && edges_pow2 && EDGES[BUCKETS - 1] == TOP_EDGE, "");
    // 2) 落桶：0/1→桶0、2→桶1、3/4→桶2、5→桶3、65536→顶桶（边界归上沿桶）。
    let mut h = LogHistogram::new();
    let b0 = h.record(0);
    let b0b = h.record(1);
    let b1 = h.record(2);
    let b2 = h.record(4);
    let b3 = h.record(5);
    let btop = h.record(65_536);
    cs.add(
        "hist_bucket_assignment",
        b0 == 0 && b0b == 0 && b1 == 1 && b2 == 2 && b3 == 3 && btop == BUCKETS - 1 && h.total == 6,
        "",
    );
    // 3) 越界钳到顶桶并记账（不静默丢点）。
    let mut hc = LogHistogram::new();
    let _ = hc.record(65_537);
    let _ = hc.record(u64::MAX);
    cs.add("hist_clamp_ledger", hc.counts[BUCKETS - 1] == 2 && hc.clamped == 2, "");
    // 4) P50/P95/P99 插值：100 条全落 (2,4] 桶 → 50 分位 3、95/99 分位 4。
    let mut hq = LogHistogram::new();
    for _ in 0..100 {
        let _ = hq.record(3);
    }
    cs.add(
        "hist_percentile_interp",
        hq.total == 100 && hq.percentile(500) == 3 && hq.percentile(950) == 4 && hq.percentile(990) == 4,
        "",
    );
    // 5) 分位跨桶累积：桶0 计 10、桶1 计 10 → P50 落首桶上沿 1、P99 落 2。
    let mut hm = LogHistogram::new();
    for _ in 0..10 {
        let _ = hm.record(1);
    }
    for _ in 0..10 {
        let _ = hm.record(2);
    }
    cs.add("hist_percentile_crosstalk", hm.percentile(500) == 1 && hm.percentile(990) == 2, "");
    // 6) 采样账本：预算内全收且三账闭合。
    let mut sl = SamplingLedger::new();
    for _ in 0..100 {
        sl.admit();
    }
    cs.add("sampling_in_budget", sl.sampled == 100 && sl.dropped == 0 && sl.consistent(), "");
    // 7) 超预算 1/8 降采样：预算外 16 次尝试恰 2 次入账（&7==0 于 0/8）、14 丢弃。
    let mut sb = SamplingLedger::new();
    for _ in 0..SAMPLE_BUDGET {
        sb.admit();
    }
    for _ in 0..16 {
        sb.admit();
    }
    cs.add(
        "sampling_downsample_eighth",
        sb.sampled == SAMPLE_BUDGET + 2 && sb.dropped == 14 && sb.consistent(),
        "",
    );
    // 8) 会话去重窗口边界：60s 整命中去重、61s 出窗新会话。
    let mut sd = SessionDedup::new();
    let first = sd.record(0xA11CE, 100);
    let dup = sd.record(0xA11CE, 160);
    let late = sd.record(0xA11CE, 221);
    cs.add("session_dedup_window", !first && dup && sd.hits == 1 && !late && sd.count == 2, "");
    // 9) 去重环 16 槽：第 17 个不同指纹替换最旧（count 恒 16、cursor 进到 1）。
    let mut sr = SessionDedup::new();
    for k in 0..17u64 {
        let _ = sr.record(1000 + k, 10 + k);
    }
    cs.add("session_ring_replace", sr.count == SESSION_RING && sr.cursor == 1, "");
    // 10) 被替换的最旧指纹已出环：1000 再入不去重（新会话入账）。
    let readd = sr.record(1000, 100);
    cs.add("session_ring_evicted", !readd && sr.cursor == 2, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_empty_and_single() {
        let mut h = LogHistogram::new();
        assert_eq!(h.percentile(500), 0, "空图分位如实返回 0");
        let _ = h.record(4);
        assert_eq!(h.percentile(500), 4, "单值落在桶上沿 → 估计恰等原值");
        let mut h1 = LogHistogram::new();
        let _ = h1.record(1);
        assert_eq!(h1.percentile(990), 1, "首桶单值 P99 仍为 1");
    }

    #[test]
    fn budget_exactly_then_eighth() {
        let mut sl = SamplingLedger::new();
        for _ in 0..SAMPLE_BUDGET {
            assert!(sl.admit(), "预算内必收");
        }
        for _ in 0..8 {
            sl.admit();
        }
        assert_eq!(sl.sampled, SAMPLE_BUDGET + 1, "预算外 8 次恰 1 次入账（&7==0 于 0）");
        assert_eq!(sl.dropped, 7);
        assert!(sl.consistent());
    }

    #[test]
    fn deep3_checks_all_green() {
        let cs = run_f036e_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
