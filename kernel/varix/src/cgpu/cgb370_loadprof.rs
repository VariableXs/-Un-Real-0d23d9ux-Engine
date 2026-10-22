//! CGPU-F0370 · 历史负载画像（判据段 0x09A0）
//! 画像：账户负载时间线（每账户逐帧消耗历史——降采样存储）；画
//! 像聚合（按时段/按场景聚合视图）；存储上限（滚动窗口——内存配
//! 额，超限淘汰最旧）；画像查询（任意账户任意时段）。判据：时间
//! 线、聚合、配额、查询、判据。
//!
//! 锚点原文摘录（施工书 1645 行）：
//! 「账户负载时间线（每账户逐帧消耗历史——降采样存储）；画像聚合
//! （按时段/按场景聚合视图）；存储上限（滚动窗口——内存配额，超
//! 限淘汰最旧）；画像查询（任意账户任意时段）。判据：时间线、聚
//! 合、配额、查询、判据。」
//!
//! 诊断码段 0x09A0（本模块独占：0x09A0~0x09AF）。
//! no_std + alloc 零 unwrap（测试除外），全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x09A0;

/// 单账户时间线存储上限（判据「存储上限」：滚动窗 256 帧原始 +
/// 64 槽降采样，内存配额写死）。
pub const TIMELINE_RAW_CAP: usize = 256;
pub const DOWNSAMPLE_SLOTS: usize = 64;

/// 降采样压缩比（4 帧原始 → 1 槽）：配额来源注释。
pub const DOWNSAMPLE_RATIO: usize = 4;

/// 账户时间线（判据「时间线」一等公民）：滚动原始窗 + 降采样层。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Timeline {
    /// 账户号。
    pub account: u32,
    /// 原始滚动窗。
    pub raw: Vec<u32>,
    /// 降采样槽（每槽 = RATIO 帧均值，槽满才落）。
    pub slots: Vec<u32>,
    /// 累计帧数（单调——查询的时轴锚）。
    pub total_frames: u64,
}

impl Timeline {
    /// 空时间线。
    pub fn new(account: u32) -> Result<Self, u16> {
        if account == 0 {
            return Err(SEG_BASE);
        }
        Ok(Timeline { account, raw: Vec::new(), slots: Vec::new(), total_frames: 0 })
    }
    /// 推帧（判据「存储上限」主判据）：原始窗超限淘汰最旧（滚动
    /// 语义），每 RATIO 帧落一降采样槽。
    pub fn push(&mut self, v: u32) -> Result<(), u16> {
        if self.raw.len() >= TIMELINE_RAW_CAP {
            self.raw.remove(0);
        }
        self.raw.push(v);
        self.total_frames += 1;
        if self.total_frames % DOWNSAMPLE_RATIO as u64 == 0 {
            let n = self.raw.len();
            let tail = &self.raw[n - DOWNSAMPLE_RATIO..];
            let sum: u64 = tail.iter().map(|&x| x as u64).sum();
            let mean = (sum / DOWNSAMPLE_RATIO as u64) as u32;
            if self.slots.len() >= DOWNSAMPLE_SLOTS {
                self.slots.remove(0);
            }
            self.slots.push(mean);
        }
        Ok(())
    }
    /// 配额核（判据「配额」守卫）：两层数组永不超配额。
    pub fn quota_ok(&self) -> bool {
        self.raw.len() <= TIMELINE_RAW_CAP && self.slots.len() <= DOWNSAMPLE_SLOTS
    }
}

/// 聚合视图（判据「聚合」产出）：时段均值 + 峰值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AggView {
    /// 覆盖帧数。
    pub frames: u64,
    /// 均值（千分比口径原样）。
    pub mean: u32,
    /// 峰值。
    pub peak: u32,
}

/// 时段聚合（判据「聚合」主判据）：取降采样层最近 n 槽聚合；n=0
/// 或超槽显性拒绝；空槽显性 None 不发零值谎报。
pub fn aggregate_slots(t: &Timeline, slots: usize) -> Result<Option<AggView>, u16> {
    if slots == 0 {
        return Err(SEG_BASE + 1);
    }
    if slots > DOWNSAMPLE_SLOTS {
        return Err(SEG_BASE + 2);
    }
    if t.slots.is_empty() {
        return Ok(None);
    }
    let take = slots.min(t.slots.len());
    let win = &t.slots[t.slots.len() - take..];
    let sum: u64 = win.iter().map(|&v| v as u64).sum();
    let peak = *win.iter().max().ok_or(SEG_BASE + 3)?;
    Ok(Some(AggView {
        frames: take as u64 * DOWNSAMPLE_RATIO as u64,
        mean: (sum / take as u64) as u32,
        peak,
    }))
}

/// 画像查询（判据「查询」主判据）：任意账户在注册表内按号取时间
/// 线；未注册账户显性 None；时段以「最近 n 帧」表达，n 超窗则给
/// 全窗（诚实标注覆盖数）。
pub fn query_timeline<'a>(
    registry: &'a [Timeline],
    account: u32,
    last_n: usize,
) -> Result<Option<(&'a Timeline, usize)>, u16> {
    if account == 0 {
        return Err(SEG_BASE + 4);
    }
    let t = registry.iter().find(|t| t.account == account);
    match t {
        None => Ok(None),
        Some(t) => {
            let n = last_n.min(t.raw.len());
            Ok(Some((t, n)))
        }
    }
}

/// 场景聚合标签（判据「聚合」的场景维）：按时段/场景双维聚合。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SceneTag {
    /// 游戏。
    Game,
    /// 视频。
    Video,
    /// 桌面。
    Desktop,
}

impl SceneTag {
    /// 标签号。
    pub fn ordinal(&self) -> u8 {
        match self {
            SceneTag::Game => 0,
            SceneTag::Video => 1,
            SceneTag::Desktop => 2,
        }
    }
}

/// 场景聚合行（账户 × 场景 → 均值快照）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SceneAggRow {
    /// 账户号。
    pub account: u32,
    /// 场景标签。
    pub scene: SceneTag,
    /// 场景内均值。
    pub mean: u32,
    /// 样本数。
    pub samples: u32,
}

impl SceneAggRow {
    /// 合法性：零样本行不发（B08 纪律——不发零值谎报）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.account == 0 || self.samples == 0 {
            return Err(SEG_BASE + 5);
        }
        Ok(())
    }
}

/// 场景聚合器（判据「聚合」场景维主判据）：按（账户,场景）累积
/// 样本再出快照——同键幂等累积。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SceneAggregator {
    /// 行集。
    pub rows: Vec<SceneAggRow>,
}

impl SceneAggregator {
    /// 空。
    pub fn new() -> Self {
        SceneAggregator { rows: Vec::new() }
    }
    /// 累积一帧样本（同键行均值滚动更新）。
    pub fn observe(&mut self, account: u32, scene: SceneTag, value: u32) -> Result<(), u16> {
        if account == 0 {
            return Err(SEG_BASE + 6);
        }
        if let Some(r) = self.rows.iter_mut().find(|r| r.account == account && r.scene == scene) {
            // 滚动均值：mean' = (mean×(n-1) + v) / n（整数保守）。
            let n = r.samples + 1;
            r.mean = ((r.mean as u64 * (n - 1) as u64 + value as u64) / n as u64) as u32;
            r.samples = n;
            return Ok(());
        }
        self.rows.push(SceneAggRow { account, scene, mean: value, samples: 1 });
        Ok(())
    }
    /// 快照（判据「聚合」输出）：全行合法化核。
    pub fn snapshot(&self) -> Result<Vec<SceneAggRow>, u16> {
        for r in &self.rows {
            r.validate()?;
        }
        Ok(self.rows.clone())
    }
}

impl Default for SceneAggregator {
    fn default() -> Self {
        Self::new()
    }
}

/// 画像指纹（可审计面）。
pub fn timeline_fp(t: &Timeline) -> u64 {
    let mut acc: u64 = 0xcbf29ce484222325;
    acc = acc.wrapping_mul(0x100000001b3) ^ t.account as u64;
    acc = acc.wrapping_mul(0x100000001b3) ^ t.total_frames;
    for v in &t.slots {
        acc = acc.wrapping_mul(0x100000001b3) ^ *v as u64;
    }
    acc
}

/// 画像诊断明细（错误码 → 中文可操作建议）。
pub fn diagnose(code: u16) -> &'static str {
    let d = code.wrapping_sub(SEG_BASE);
    match d {
        0 => "账户号为 0",
        1 => "聚合槽数为 0",
        2 => "聚合槽数超上限（64）",
        3 => "内部错误：非空槽无峰值",
        4 => "查询账户为 0",
        5 => "聚合行账户为 0 或零样本",
        6 => "观测账户为 0",
        _ => "未知诊断码——查本模块 SEG_BASE 0x09A0",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline_quota_and_downsample() {
        let mut t = Timeline::new(1).unwrap();
        for i in 0..600u32 {
            t.push(i % 100).unwrap();
        }
        assert!(t.quota_ok());
        assert_eq!(t.raw.len(), TIMELINE_RAW_CAP);
        assert_eq!(t.slots.len(), DOWNSAMPLE_SLOTS);
        assert_eq!(t.total_frames, 600);
        assert!(timeline_fp(&t) != 0);
        assert_eq!(Timeline::new(0).unwrap_err(), SEG_BASE);
    }

    #[test]
    fn aggregation_views() {
        let mut t = Timeline::new(2).unwrap();
        for _ in 0..40 {
            t.push(500).unwrap();
        }
        // 40 帧 = 10 槽。
        assert_eq!(t.slots.len(), 10);
        let agg = aggregate_slots(&t, 5).unwrap().unwrap();
        assert_eq!(agg.mean, 500);
        assert_eq!(agg.peak, 500);
        assert_eq!(agg.frames, 20);
        // 空槽显性 None。
        let empty = Timeline::new(3).unwrap();
        assert!(aggregate_slots(&empty, 4).unwrap().is_none());
        assert_eq!(aggregate_slots(&t, 0).unwrap_err(), SEG_BASE + 1);
        assert_eq!(aggregate_slots(&t, 65).unwrap_err(), SEG_BASE + 2);
    }

    #[test]
    fn query_and_scene_agg() {
        let mut reg = Vec::new();
        let mut t = Timeline::new(7).unwrap();
        for _ in 0..30 {
            t.push(200).unwrap();
        }
        reg.push(t);
        // 命中查询。
        let (got, n) = query_timeline(&reg, 7, 10).unwrap().unwrap();
        assert_eq!(got.account, 7);
        assert_eq!(n, 10);
        // 未注册显性 None。
        assert!(query_timeline(&reg, 8, 10).unwrap().is_none());
        // 场景聚合：同键幂等。
        let mut agg = SceneAggregator::new();
        for _ in 0..5 {
            agg.observe(1, SceneTag::Game, 600).unwrap();
        }
        agg.observe(1, SceneTag::Video, 300).unwrap();
        let snap = agg.snapshot().unwrap();
        assert_eq!(snap.len(), 2);
        assert_eq!(snap[0].samples, 5);
        assert_eq!(snap[0].mean, 600);
        assert_eq!(agg.observe(0, SceneTag::Game, 1).unwrap_err(), SEG_BASE + 6);
        assert!(diagnose(SEG_BASE + 5).contains("零样本"));
    }
}
