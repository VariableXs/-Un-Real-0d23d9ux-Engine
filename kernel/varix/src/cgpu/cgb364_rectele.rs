//! CGPU-F0364 · 配方遥测（判据段 0x0940）
//! 遥测：配方命中率（激活配方 vs 实际场景匹配率）；修正频率（运
//! 行时修正触发次数）；切换延迟分布；冲突次数按配方对统计——四
//! 指标进 B08 总线。判据：四指标、进总线、判据。
//!
//! 锚点原文摘录（施工书 1619 行）：
//! 「配方命中率（激活配方 vs 实际场景匹配率）；修正频率（运行时
//! 修正触发次数）；切换延迟分布；冲突次数按配方对统计——四指标
//! 进 B08 总线。判据：四指标、进总线、判据。」
//!
//! 诊断码段 0x0940（本模块独占：0x0940~0x094F）。
//! no_std + alloc 零 unwrap（测试除外），全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0940;

/// 命中率采样窗（帧）。
pub const HIT_WINDOW_FRAMES: usize = 64;

/// 切换延迟桶边界（毫秒，12 桶对数式）：1/2/4/8/16/32/64/128/256/
/// 512/1024/2048——2048 以上进末桶。
pub const LATENCY_BUCKET_EDGES_MS: [u32; 12] =
    [1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048];

/// B08 总线指标槽位（判据「进总线」的槽位表——四指标各占一槽）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MetricSlot {
    /// 槽 1：配方命中率。
    HitRate,
    /// 槽 2：修正频率。
    FixRate,
    /// 槽 3：切换延迟分布。
    SwitchLatency,
    /// 槽 4：冲突次数（按配方对）。
    ConflictByPair,
}

impl MetricSlot {
    /// 槽位号（1 起——总线可寻址）。
    pub fn slot(&self) -> u8 {
        match self {
            MetricSlot::HitRate => 1,
            MetricSlot::FixRate => 2,
            MetricSlot::SwitchLatency => 3,
            MetricSlot::ConflictByPair => 4,
        }
    }
    /// 闭集遍历（总线注册核依赖稳定顺序）。
    pub fn all() -> [MetricSlot; 4] {
        [MetricSlot::HitRate, MetricSlot::FixRate, MetricSlot::SwitchLatency, MetricSlot::ConflictByPair]
    }
}

/// 命中率账（判据「配方命中率」）：激活配方与实际场景逐帧比对。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HitLedger {
    /// 命中帧数。
    pub hits: u32,
    /// 采样帧数。
    pub sampled: u32,
}

impl HitLedger {
    /// 记一帧（matched=激活配方与实际场景一致）。
    pub fn record(&mut self, matched: bool) -> Result<(), u16> {
        if self.sampled >= HIT_WINDOW_FRAMES as u32 {
            return Err(SEG_BASE);
        }
        self.sampled += 1;
        if matched {
            self.hits += 1;
        }
        Ok(())
    }
    /// 命中率（千分比）：空窗显性 None 不发零值谎报（B08 纪律）。
    pub fn rate_permille(&self) -> Option<u32> {
        if self.sampled == 0 {
            None
        } else {
            Some((self.hits as u64 * 1000 / self.sampled as u64) as u32)
        }
    }
    /// 窗口满核（判据「进总线」前置：满窗才发布）。
    pub fn full(&self) -> bool {
        self.sampled == HIT_WINDOW_FRAMES as u32
    }
}

/// 修正账（判据「修正频率」）：运行时修正触发计数 + 频率。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FixLedger {
    /// 修正触发次数。
    pub fixes: u32,
    /// 观测帧数。
    pub frames: u32,
}

impl FixLedger {
    /// 记一帧（可选修正事件）。
    pub fn record_frame(&mut self, fix_fired: bool) {
        self.frames += 1;
        if fix_fired {
            self.fixes += 1;
        }
    }
    /// 修正频率（每千帧次数）；零帧显性 None。
    pub fn rate_per_kframe(&self) -> Option<u32> {
        if self.frames == 0 {
            None
        } else {
            Some((self.fixes as u64 * 1000 / self.frames as u64) as u32)
        }
    }
}

/// 延迟直方图（判据「切换延迟分布」）：12 桶 + 溢出计数，Σ入桶+
/// 溢出 ≡ Σ推送（守恒）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LatencyHist {
    /// 12 桶计数。
    pub buckets: [u32; 12],
    /// 溢出（≥2048ms）计数。
    pub overflow: u32,
    /// 推送总数。
    pub pushed: u32,
}

impl LatencyHist {
    /// 空直方图。
    pub fn new() -> Self {
        LatencyHist { buckets: [0; 12], overflow: 0, pushed: 0 }
    }
    /// 桶定位（判据「切换延迟分布」的分桶语义）。
    pub fn bucket_of(ms: u32) -> Option<usize> {
        for (i, &e) in LATENCY_BUCKET_EDGES_MS.iter().enumerate() {
            if ms <= e {
                return Some(i);
            }
        }
        None
    }
    /// 推送一个延迟样本。
    pub fn push(&mut self, ms: u32) -> Result<(), u16> {
        if ms == 0 {
            return Err(SEG_BASE + 1);
        }
        self.pushed += 1;
        match Self::bucket_of(ms) {
            Some(b) => self.buckets[b] += 1,
            None => self.overflow += 1,
        }
        Ok(())
    }
    /// 守恒核：Σ桶 + 溢出 ≡ 推送数。
    pub fn conserved(&self) -> bool {
        let sum: u32 = self.buckets.iter().sum();
        sum + self.overflow == self.pushed
    }
    /// P50 桶（首个累计过半的桶——整数分位，诚实标注桶粒度）。
    pub fn p50_bucket(&self) -> Option<usize> {
        if self.pushed == 0 {
            return None;
        }
        let half = (self.pushed + 1) / 2;
        let mut acc = 0u32;
        for (i, &c) in self.buckets.iter().enumerate() {
            acc += c;
            if acc >= half {
                return Some(i);
            }
        }
        None
    }
}

/// 冲突对账行（判据「冲突次数按配方对统计」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PairCount {
    /// 配方对（规范化小号在前）。
    pub pair: (u8, u8),
    /// 冲突次数。
    pub count: u32,
}

impl PairCount {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.pair.0 > self.pair.1 || self.pair.0 > 2 || self.pair.1 > 2 {
            return Err(SEG_BASE + 2);
        }
        Ok(())
    }
}

/// 遥测总线发布器（判据「进总线」主判据）：四指标槽位注册 + 发布
/// 前置自检（命中率满窗、延迟守恒、对账行合法）——不达标槽不发。
pub struct TelemetryBus {
    /// 已注册槽位（位图 bit0~3 ↔ 槽 1~4）。
    pub registered: u8,
    /// 命中率账。
    pub hit: HitLedger,
    /// 修正账。
    pub fix: FixLedger,
    /// 延迟直方图。
    pub lat: LatencyHist,
    /// 冲突对账。
    pub pairs: Vec<PairCount>,
}

impl TelemetryBus {
    /// 空总线。
    pub fn new() -> Self {
        TelemetryBus { registered: 0, hit: HitLedger { hits: 0, sampled: 0 }, fix: FixLedger { fixes: 0, frames: 0 }, lat: LatencyHist::new(), pairs: Vec::new() }
    }
    /// 注册槽位。
    pub fn register(&mut self, s: MetricSlot) {
        self.registered |= 1 << (s.slot() - 1);
    }
    /// 发布自检（判据「进总线」守卫）：注册位图必须覆盖四槽；四账
    /// 各自前置满足才可发布。
    pub fn publish_ready(&self) -> Result<(), u16> {
        if self.registered != 0b1111 {
            return Err(SEG_BASE + 3);
        }
        if !self.hit.full() {
            return Err(SEG_BASE + 4);
        }
        if !self.lat.conserved() {
            return Err(SEG_BASE + 5);
        }
        for p in &self.pairs {
            p.validate()?;
        }
        Ok(())
    }
    /// 发布快照（四指标齐套）。
    pub fn publish(&self) -> Result<(u32, u32, usize, u32), u16> {
        self.publish_ready()?;
        let hit = self.hit.rate_permille().ok_or(SEG_BASE + 6)?;
        let fix = self.fix.rate_per_kframe().ok_or(SEG_BASE + 7)?;
        let p50 = self.lat.p50_bucket().unwrap_or(0);
        let conflicts: u32 = self.pairs.iter().map(|p| p.count).sum();
        Ok((hit, fix, p50, conflicts))
    }
}

impl Default for TelemetryBus {
    fn default() -> Self {
        Self::new()
    }
}

/// 槽位表冻结核（判据「进总线」守卫）：四槽连续 1..=4 不跳号。
pub fn slot_table_frozen() -> Result<(), u16> {
    for (i, s) in MetricSlot::all().iter().enumerate() {
        if s.slot() as usize != i + 1 {
            return Err(SEG_BASE + 8);
        }
    }
    Ok(())
}

/// 遥测诊断明细（错误码 → 中文可操作建议）。
pub fn diagnose(code: u16) -> &'static str {
    let d = code.wrapping_sub(SEG_BASE);
    match d {
        0 => "命中窗已满——先发布再开新窗",
        1 => "延迟样本必须 >0",
        2 => "配方对非法（规范化小号在前、值 ≤2）",
        3 => "槽位注册不全（四槽必须齐）",
        4 => "命中率窗未满——不满不发（B08 纪律）",
        5 => "延迟直方图不守恒",
        6 => "命中率空窗——无可发布",
        7 => "修正频率零帧——无可发布",
        8 => "槽位表跳号——冻结被破坏",
        _ => "未知诊断码——查本模块 SEG_BASE 0x0940",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_rate_and_fix_rate() {
        let mut h = HitLedger { hits: 0, sampled: 0 };
        for i in 0..64 {
            h.record(i % 2 == 0).unwrap();
        }
        assert!(h.full());
        assert_eq!(h.rate_permille(), Some(500));
        assert_eq!(h.record(true).unwrap_err(), SEG_BASE);
        let mut f = FixLedger { fixes: 0, frames: 0 };
        for i in 0..1000 {
            f.record_frame(i % 100 == 0);
        }
        assert_eq!(f.rate_per_kframe(), Some(10));
    }

    #[test]
    fn latency_hist_conservation() {
        let mut h = LatencyHist::new();
        for ms in [1u32, 2, 3, 17, 100, 5000] {
            h.push(ms).unwrap();
        }
        assert!(h.conserved());
        assert_eq!(h.pushed, 6);
        assert_eq!(h.overflow, 1);
        // 中位数 3（1,2,3,17,100 的中位）→ 桶 2（2^2=4 > 3 ≥ 2^1）。
        assert_eq!(h.p50_bucket(), Some(2));
        assert_eq!(LatencyHist::bucket_of(2048), Some(11));
        // 纯映射语义：0 落首桶（拒绝闸在 push，不在映射）。
        assert_eq!(LatencyHist::bucket_of(0), Some(0));
        assert_eq!(h.push(0).unwrap_err(), SEG_BASE + 1);
    }

    #[test]
    fn bus_publish_gate() {
        let mut bus = TelemetryBus::new();
        // 未注册全四槽：拒发。
        bus.register(MetricSlot::HitRate);
        assert_eq!(bus.publish_ready().unwrap_err(), SEG_BASE + 3);
        for s in MetricSlot::all() {
            bus.register(s);
        }
        // 命中窗未满：拒发。
        assert_eq!(bus.publish_ready().unwrap_err(), SEG_BASE + 4);
        for i in 0..64 {
            bus.hit.record(i % 4 != 0).unwrap();
        }
        bus.fix.record_frame(true);
        bus.lat.push(4).unwrap();
        bus.pairs.push(PairCount { pair: (0, 1), count: 3 });
        let (hit, fix, p50, conflicts) = bus.publish().unwrap();
        assert_eq!(hit, 750);
        assert_eq!(fix, 1000);
        assert_eq!(p50, 2);
        assert_eq!(conflicts, 3);
        assert!(slot_table_frozen().is_ok());
        assert!(diagnose(SEG_BASE + 4).contains("满"));
    }
}
