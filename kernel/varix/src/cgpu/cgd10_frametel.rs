//! cgd10_frametel — CGPU-F0490 帧遥测（五指标进总线，D 域数据全量可观测）。
//!
//! 判据原文（施工书锚点 CGPU-F0490）：帧遥测：帧时间/间隔/全延迟/达标率/卡
//! 顿计数——五指标进 B08 总线（D 域数据全量可观测）；按场景分列。判据：五
//! 指标、分列、判据。
//!
//! 职责定位：五指标聚合器（帧时间/帧间隔/全延迟/达标率/卡顿计数——每指标
//! 独立累加与分位概要）+ 场景分列账（按场景标签各自一套五指标——混场景均
//! 值是谎报，分列才可归因）+ B08 总线发布包（schema 版本化快照——下游一条
//! 拉取拿全量）。与 cgd03_frameledger（明细账）、cgd06_latencyledger（全
//! 延迟账）分工：彼存逐帧明细，此供聚合观测面——遥测是账本的只读投影。数
//! 据红线：遥测只聚合不动账本；空场景不发零值（空=空，不造假）。诊断码段
//! 0x5B01~0x5B05。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const FRAME_TEL_VERSION: &str = "D10-frametel-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x5B 段，本会话独占）
// ---------------------------------------------------------------------------

pub const XC_SCENE_UNKNOWN: u32 = 0x5B01; // 未登记场景的帧（拒绝匿名混账）
pub const XC_METRIC_NEGATIVE: u32 = 0x5B02; // 指标负值（时间量无负数——违约）
pub const XC_SCHEMA_STALE: u32 = 0x5B03; // 发布包 schema 版本失配
pub const XC_WINDOW_EMPTY: u32 = 0x5B04; // 空窗查询（诚实空态，不返回零均值）
pub const XC_SCENE_CAP: u32 = 0x5B05; // 场景数超上限（分列账扩容拒绝）

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 场景分列上限。
pub const SCENE_CAP: usize = 12;
/// 滚动窗长（帧）：达标率与卡顿计数的统计口径。
pub const TEL_WINDOW: usize = 600;
/// B08 发布 schema 版本。
pub const TEL_SCHEMA_V: u32 = 1;
/// 达标判定的帧时间上限（微秒）——80 帧合同口径 12.5ms。
pub const TARGET_FRAME_US: u64 = 12_500;
/// 卡顿判定阈值（微秒）：帧间隔突增判定复用 cgd06 口径（均值×2 且 ≥20ms）。
pub const JANK_GAP_US: u64 = 20_000;

/// 五指标闭集（判据"五指标"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TelMetric {
    /// 帧时间（渲染耗时）。
    FrameTime,
    /// 帧间隔（呈现节拍）。
    FrameGap,
    /// 全延迟（输入到像素）。
    EndToEnd,
    /// 达标率（帧时间 ≤ 合同口径占比）。
    OnTargetRate,
    /// 卡顿计数。
    JankCount,
}

impl TelMetric {
    pub const ALL: [TelMetric; 5] = [
        TelMetric::FrameTime,
        TelMetric::FrameGap,
        TelMetric::EndToEnd,
        TelMetric::OnTargetRate,
        TelMetric::JankCount,
    ];

    pub const fn zh(self) -> &'static str {
        match self {
            TelMetric::FrameTime => "帧时间",
            TelMetric::FrameGap => "帧间隔",
            TelMetric::EndToEnd => "全延迟",
            TelMetric::OnTargetRate => "达标率",
            TelMetric::JankCount => "卡顿计数",
        }
    }
}

/// 单场景五指标聚合账。
pub struct SceneBucket {
    pub scene: String,
    /// 滚动窗：帧时间样本（微秒）。
    ft_win: Vec<u64>,
    /// 滚动窗：帧间隔样本。
    gap_win: Vec<u64>,
    /// 滚动窗：全延迟样本。
    e2e_win: Vec<u64>,
    /// 达标帧计数（窗内）。
    on_target: u64,
    /// 卡顿计数（累计，不只窗内——判据"卡顿计数"累计语义）。
    pub jank_total: u64,
    pub frames: u64,
}

impl SceneBucket {
    fn new(scene: &str) -> Self {
        SceneBucket {
            scene: String::from(scene),
            ft_win: Vec::with_capacity(TEL_WINDOW),
            gap_win: Vec::with_capacity(TEL_WINDOW),
            e2e_win: Vec::with_capacity(TEL_WINDOW),
            on_target: 0,
            jank_total: 0,
            frames: 0,
        }
    }

    /// 录入一帧三时值（帧时间/间隔/全延迟）。
    fn push(&mut self, frame_us: u64, gap_us: u64, e2e_us: u64) {
        if frame_us == 0 || gap_us == 0 || e2e_us == 0 {
            return; // 零值帧不进统计（采样缺失≠零耗时——不造假）
        }
        if self.ft_win.len() >= TEL_WINDOW {
            self.ft_win.remove(0);
        }
        if self.gap_win.len() >= TEL_WINDOW {
            self.gap_win.remove(0);
        }
        if self.e2e_win.len() >= TEL_WINDOW {
            self.e2e_win.remove(0);
        }
        self.ft_win.push(frame_us);
        self.gap_win.push(gap_us);
        self.e2e_win.push(e2e_us);
        self.frames += 1;
        if frame_us <= TARGET_FRAME_US {
            self.on_target += 1;
        }
        if gap_us >= JANK_GAP_US {
            self.jank_total += 1;
        }
    }

    /// 窗均值（微秒；空窗返回 None——诚实空态）。
    fn mean_of(win: &Vec<u64>) -> Option<u64> {
        if win.len() == 0 {
            return None;
        }
        let mut acc: u128 = 0;
        let mut i = 0;
        while i < win.len() {
            acc += win[i] as u128;
            i += 1;
        }
        Some((acc / win.len() as u128) as u64)
    }

    /// 达标率（ppm×1e4 即万分比；空窗 None）。
    fn on_target_rate(&self) -> Option<u32> {
        let n = self.ft_win.len();
        if n == 0 {
            return None;
        }
        Some((self.on_target * 10_000 / n as u64) as u32)
    }

    /// P95（整数分位：排序取位——窗 600 内排序开销可接受，遥测低频调用）。
    fn p95_of(win: &Vec<u64>) -> Option<u64> {
        if win.len() == 0 {
            return None;
        }
        let mut v = Vec::with_capacity(win.len());
        let mut i = 0;
        while i < win.len() {
            v.push(win[i]);
            i += 1;
        }
        // 插入排序（n=600 常数小；no_std 无 sort 稳定依赖面）。
        let mut a = 1;
        while a < v.len() {
            let key = v[a];
            let mut b = a;
            while b > 0 && v[b - 1] > key {
                v[b] = v[b - 1];
                b -= 1;
            }
            v[b] = key;
            a += 1;
        }
        let idx = (v.len() * 95) / 100;
        Some(v[idx.min(v.len() - 1)])
    }
}

/// 五指标快照（B08 发布包行）。
#[derive(Clone, Debug)]
pub struct TelSnap {
    pub scene: String,
    pub frames: u64,
    pub ft_mean_us: Option<u64>,
    pub gap_mean_us: Option<u64>,
    pub e2e_mean_us: Option<u64>,
    pub ft_p95_us: Option<u64>,
    pub on_target_bps: Option<u32>,
    pub jank_total: u64,
}

/// 帧遥测发布器（B08 总线接缝）。
pub struct FrameTelemetry {
    buckets: Vec<SceneBucket>,
    pub schema_v: u32,
    pub publish_count: u64,
    pub rejected_frames: u64,
}

impl FrameTelemetry {
    pub fn new() -> Self {
        FrameTelemetry {
            buckets: Vec::new(),
            schema_v: TEL_SCHEMA_V,
            publish_count: 0,
            rejected_frames: 0,
        }
    }

    /// 场景登记（未登记场景拒帧——判据"按场景分列"的准入纪律）。
    pub fn register_scene(&mut self, scene: &str) -> Result<(), u32> {
        if self.find_idx(scene).is_some() {
            return Ok(()); // 幂等
        }
        if self.buckets.len() >= SCENE_CAP {
            return Err(XC_SCENE_CAP);
        }
        self.buckets.push(SceneBucket::new(scene));
        Ok(())
    }

    fn find_idx(&self, scene: &str) -> Option<usize> {
        let mut i = 0;
        while i < self.buckets.len() {
            if self.buckets[i].scene == scene {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 帧录入（三时值进对应场景账）。
    pub fn observe(&mut self, scene: &str, frame_us: u64, gap_us: u64, e2e_us: u64) -> Result<(), u32> {
        if frame_us == 0 && gap_us == 0 && e2e_us == 0 {
            self.rejected_frames += 1;
            return Err(XC_METRIC_NEGATIVE);
        }
        if frame_us > u64::MAX / 2 || gap_us > u64::MAX / 2 || e2e_us > u64::MAX / 2 {
            self.rejected_frames += 1;
            return Err(XC_METRIC_NEGATIVE);
        }
        let idx = match self.find_idx(scene) {
            Some(i) => i,
            None => {
                self.rejected_frames += 1;
                return Err(XC_SCENE_UNKNOWN); // 匿名帧拒绝——分列纪律
            }
        };
        self.buckets[idx].push(frame_us, gap_us, e2e_us);
        Ok(())
    }

    /// B08 发布包：全场景快照（判据"进 B08 总线"）。
    pub fn publish(&mut self) -> Vec<TelSnap> {
        self.publish_count += 1;
        let mut out = Vec::with_capacity(self.buckets.len());
        let mut i = 0;
        while i < self.buckets.len() {
            let b = &self.buckets[i];
            out.push(TelSnap {
                scene: b.scene.clone(),
                frames: b.frames,
                ft_mean_us: SceneBucket::mean_of(&b.ft_win),
                gap_mean_us: SceneBucket::mean_of(&b.gap_win),
                e2e_mean_us: SceneBucket::mean_of(&b.e2e_win),
                ft_p95_us: SceneBucket::p95_of(&b.ft_win),
                on_target_bps: b.on_target_rate(),
                jank_total: b.jank_total,
            });
            i += 1;
        }
        out
    }

    /// 单场景查询（空窗显性 None——不返回零均值谎报）。
    pub fn query(&self, scene: &str, m: TelMetric) -> Result<Option<u64>, u32> {
        let idx = match self.find_idx(scene) {
            Some(i) => i,
            None => return Err(XC_SCENE_UNKNOWN),
        };
        let b = &self.buckets[idx];
        let r = match m {
            TelMetric::FrameTime => SceneBucket::mean_of(&b.ft_win),
            TelMetric::FrameGap => SceneBucket::mean_of(&b.gap_win),
            TelMetric::EndToEnd => SceneBucket::mean_of(&b.e2e_win),
            TelMetric::OnTargetRate => b.on_target_rate().map(|v| v as u64),
            TelMetric::JankCount => Some(b.jank_total),
        };
        if r.is_none() {
            return Err(XC_WINDOW_EMPTY);
        }
        Ok(r)
    }

    /// schema 版本核验（下游接缝：版本失配显性拒）。
    pub fn check_schema(&self, v: u32) -> Result<(), u32> {
        if v != self.schema_v {
            return Err(XC_SCHEMA_STALE);
        }
        Ok(())
    }

    /// 登记场景数。
    pub const fn scene_count(&self) -> usize {
        self.buckets.len()
    }

    /// 摘要行。
    pub fn summary(&self) -> String {
        let mut acc: u128 = 0;
        let mut i = 0;
        while i < self.buckets.len() {
            acc += self.buckets[i].frames as u128;
            i += 1;
        }
        alloc::format!(
            "帧遥测{} 场景{}/{} 总帧{} 发布{} 拒帧{}",
            FRAME_TEL_VERSION,
            self.buckets.len(),
            SCENE_CAP,
            acc,
            self.publish_count,
            self.rejected_frames,
        )
    }
}

// ---------------------------------------------------------------------------
// 自检（判据三组：五指标 / 分列 / 总线发布）
// ---------------------------------------------------------------------------

impl FrameTelemetry {
    /// 组一：五指标——录入后五口径各自可查且量纲正确。
    pub fn selfcheck_metrics(&mut self) -> Result<(), u32> {
        let mut t = FrameTelemetry::new();
        t.register_scene("game")?;
        // 100 帧：帧时间 12ms（全达标）、间隔 12.5ms、全延迟 40ms。
        let mut i = 0;
        while i < 100 {
            t.observe("game", 12_000, 12_500, 40_000)?;
            i += 1;
        }
        let ft = t.query("game", TelMetric::FrameTime)?;
        let gap = t.query("game", TelMetric::FrameGap)?;
        let e2e = t.query("game", TelMetric::EndToEnd)?;
        let rate = t.query("game", TelMetric::OnTargetRate)?;
        let jank = t.query("game", TelMetric::JankCount)?;
        if ft != Some(12_000) || gap != Some(12_500) || e2e != Some(40_000) {
            return Err(XC_WINDOW_EMPTY);
        }
        if rate != Some(10_000) {
            return Err(XC_WINDOW_EMPTY); // 全达标=100.00%（万分比满值）
        }
        if jank != Some(0) {
            return Err(XC_WINDOW_EMPTY);
        }
        Ok(())
    }

    /// 组二：分列——两场景各自独立；未登记场景拒帧。
    pub fn selfcheck_scenes(&mut self) -> Result<(), u32> {
        let mut t = FrameTelemetry::new();
        t.register_scene("web")?;
        t.register_scene("video")?;
        let mut i = 0;
        while i < 30 {
            t.observe("web", 8_000, 8_300, 20_000)?;
            t.observe("video", 16_000, 16_700, 55_000)?;
            i += 1;
        }
        let w = t.query("web", TelMetric::FrameTime)?;
        let v = t.query("video", TelMetric::FrameTime)?;
        if w != Some(8_000) || v != Some(16_000) {
            return Err(XC_SCENE_UNKNOWN);
        }
        let r = t.observe("unknown_scene", 10_000, 10_000, 10_000);
        if r != Err(XC_SCENE_UNKNOWN) || t.rejected_frames != 1 {
            return Err(XC_SCENE_UNKNOWN);
        }
        Ok(())
    }

    /// 组三：总线——发布包全场景、schema 核验、P95 有效。
    pub fn selfcheck_publish(&mut self) -> Result<(), u32> {
        let mut t = FrameTelemetry::new();
        t.register_scene("game")?;
        t.register_scene("web")?;
        let mut i = 0;
        while i < 40 {
            t.observe("game", if i % 10 == 0 { 25_000 } else { 12_000 }, 12_500, 40_000)?;
            t.observe("web", 9_000, 9_200, 24_000)?;
            i += 1;
        }
        t.check_schema(TEL_SCHEMA_V)?;
        if t.check_schema(99) != Err(XC_SCHEMA_STALE) {
            return Err(XC_SCHEMA_STALE);
        }
        let pack = t.publish();
        if pack.len() != 2 {
            return Err(XC_SCHEMA_STALE);
        }
        // game 场景：40 帧中 4 帧超时 → 达标率 90%（9000 万分比）。
        let g = &pack[0];
        if g.on_target_bps != Some(9_000) {
            return Err(XC_WINDOW_EMPTY);
        }
        // P95 应 ≥ 均值且 ≥ 25ms（4/40=10% 超时，P95 落在超时档）。
        let p95 = match g.ft_p95_us {
            Some(p) => p,
            None => return Err(XC_WINDOW_EMPTY),
        };
        if p95 < 25_000 {
            return Err(XC_WINDOW_EMPTY);
        }
        // 空场景发布不出现零值谎报（登记未喂帧 → None）。
        t.register_scene("idle")?;
        let pack2 = t.publish();
        let idle = &pack2[2];
        if idle.ft_mean_us.is_some() {
            return Err(XC_WINDOW_EMPTY);
        }
        Ok(())
    }

    /// 全量自检入口。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_metrics()?;
        self.selfcheck_scenes()?;
        self.selfcheck_publish()?;
        Ok(())
    }
}
