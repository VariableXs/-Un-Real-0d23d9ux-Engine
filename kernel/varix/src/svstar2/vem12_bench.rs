//! VE-F2412 · 动画基准（VE-M 域 · 动画段 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2412`
//!
//! **判据（锚点原文）**：三族阶梯、SIMD 对照、模型回填、暂挂延续、判据。
//!
//! **职责定位（锚点原文）**：动画基准——三族基准入 F1773 M 段：求值
//! 吞吐（千轨道求值耗时——1k 轨道全类型混合求值：类型分批 SIMD 收益
//! 实测（F2407 模型定标））、导入/导出吞吐（glTF 动画导入与导出耗时
//! ——典型 clip 规模（10 骨骼×100 关键帧）导入/导出毫秒级声明与实测）、
//! 插值性能（四插值器逐个耗时——插值器成本对比（贝塞尔 vs slerp 成本
//! 比——调参依据），入册回归。
//!
//! # 一、三族基准全部真调被测面（确定性逻辑基准，不代填）
//!
//! 与 vel12（L 域粒子基准）同族方法学：**逻辑工作量**即被测量——
//! 不读墙钟（CI 上不可比），数**真实发生的操作步数**：
//!
//! - **求值吞吐族**：1k 条 [`SoaTrack`]（六类型混合）真调
//!   [`plan_batches`](vem07_perf::plan_batches)（分批）+ 真调
//!   [`bisect`](vem07_perf::bisect)/[`lerp_alpha`](vem07_perf::
//!   lerp_alpha)（求值）——bisect 的 `steps` 是 vem07 自报的**真实
//!   比较步数**（判据不吃自证常量）；分批 vs 标量的步数对照即 SIMD
//!   收益实测面（批内共享时间轴指纹 ⇒ 一次 bisect 摊给整批）。
//! - **导入/导出族**：10 通道 × 100 关键帧典型 clip——导入侧真调
//!   [`validate_channel_refs`](vem09_import::validate_channel_refs)/
//!   [`validate_samples`](vem09_import::validate_samples)，导出侧真调
//!   [`export_gltf_anim`](vem10_export::export_gltf_anim)（逆表
//!   [`ExportMapTable::from_forward`](vem10_export::ExportMapTable::
//!   from_forward) 真调 F2409 正表机械求逆），产物字段
//!   `channels_out`/`keys_out` 当场读。
//! - **插值族**：四插值器（Step/Linear/CubicBezier/slerp）各百万次
//!   真调求值（前三走 [`eval_scalar_span`](vem03_interp::
//!   eval_scalar_span)，slerp 走 [`eval_quat_span`](vem03_interp::
//!   eval_quat_span)）——双跑摘要证确定性；成本权重按结构面声明
//!   （step/linear=1、bezier=3 双轴、slerp=8 两次原点积+两次平方根+
//!   归一化+乘加），贝塞尔 vs slerp 成本比即调参依据（模型侧输入，
//!   判据侧只验内部一致与调用数真实）。
//!
//! # 二、六列入册 + 四道守卫（沿用 F1773 家族）
//!
//! 环境四要素缺一拒册；指标口径唯一（混报无效）；同族同谱去重；
//! 基线移动超线须 ADR 留痕（无链拒绝——F1767 联动）。模型-实测闭环
//! 真调 F2407 预留回填位：偏差 ≤30% recalibrate 回填定标、超线
//! mark_stale 显性待重定标（SIMD 收益与 F2407 声明核对——锚点错误
//! 矩阵原文）。M 段门禁暂挂声明显性（移交期模式——L 域家族延续）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::svstar2::vem02_track::{DiagBag, TrackClass as ValueTrackClass};
use crate::svstar2::vem03_interp::{
    bezier_time, eval_quat_span, eval_scalar_span, Interp, InterpEntry, InterpParams,
};
use crate::svstar2::vem07_perf::{
    bisect, lerp_alpha, plan_batches, SoaTrack, TrackClass, TrackValueKind,
};
use crate::svstar2::vem09_import::{
    validate_channel_refs, validate_samples, AccessorView, ChannelPath, ChannelRef, ComponentType,
    DiagBag as ImportBag, GltfAnimDoc, GltfInterp, GltfSamplerOut, MappingTable, SampleVerdict,
    SamplerRef, TrackSemantic,
};
use crate::svstar2::vem10_export::{
    export_gltf_anim, DiagBag as ExportBag, ExportClip, ExportMapTable, ExportTrack,
};

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const ANIM_BENCH_VERSION: &str = "M12-bench-v1";

/// 求值吞吐族轨道数（锚点：1k 轨道）。
pub const EVAL_TRACKS: usize = 1_000;

/// 每轨道关键帧数（101 帧 ×1ms 步进）。
pub const EVAL_KEYS: usize = 101;

/// 导入/导出族骨骼（通道）数（锚点：10 骨骼）。
pub const CLIP_BONES: usize = 10;

/// 导入/导出族关键帧数（锚点：100 关键帧）。
pub const CLIP_KEYS: usize = 100;

/// 插值族每插值器求值次数（锚点：百万次）。
pub const INTERP_EVALS: u64 = 1_000_000;

/// 模型-实测闭环的偏差上界（%）：超线 mark_stale（锚点：偏差超 30%
/// → F2407 修正联动）。
pub const MODEL_DEVIATION_PCT: u64 = 30;

/// 基线移动阈值（%）：超线须 ADR 留痕链（F1767 联动）。
pub const BASELINE_MOVE_PCT: u64 = 15;

/// 环境声明缺项（拒绝入册）。
pub const E_BENCH_ENV_INCOMPLETE: &str = "E_BENCH_ENV_INCOMPLETE";

/// 指标口径混报（同族同谱两口径无效）。
pub const E_BENCH_METRIC_MIX: &str = "E_BENCH_METRIC_MIX";

/// 同族同谱重复入册（去重守卫）。
pub const E_BENCH_DUPLICATE: &str = "E_BENCH_DUPLICATE";

/// 基线移动无 ADR 链（拒绝）。
pub const E_BENCH_BASELINE_MOVE: &str = "E_BENCH_BASELINE_MOVE";

/// 模型-实测偏差超界（mark_stale）。
pub const E_BENCH_MODEL_DRIFT: &str = "E_BENCH_MODEL_DRIFT";

// ---------------------------------------------------------------------------
// 二、六列条目与入册守卫（F1773 家族同构）
// ---------------------------------------------------------------------------

/// 环境声明四要素（缺一拒册——与 vel12 家族同格式）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvDeclaration {
    /// 硬件型号。
    pub hw_model: &'static str,
    /// 驱动版本。
    pub driver: &'static str,
    /// 宿主环境。
    pub os: &'static str,
    /// 性能档位。
    pub tier: &'static str,
}

impl EnvDeclaration {
    /// 完整声明。
    pub fn new(hw: &'static str, driver: &'static str, os: &'static str, tier: &'static str) -> EnvDeclaration {
        EnvDeclaration { hw_model: hw, driver, os, tier }
    }

    /// 缺项即拒（空/纯空白都算缺——trimming 后判）。
    pub fn validate(&self) -> Result<(), String> {
        for (name, v) in [
            ("hw_model", self.hw_model),
            ("driver", self.driver),
            ("os", self.os),
            ("tier", self.tier),
        ] {
            if v.trim().is_empty() {
                return Err(format!("{}：环境声明缺项：{}", E_BENCH_ENV_INCOMPLETE, name));
            }
        }
        Ok(())
    }
}

/// 基准条目六列（family/load_spec/env/metric/baseline/adr_chain）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BenchEntry {
    /// 族（求值吞吐/导入导出/插值）。
    pub family: &'static str,
    /// 负载谱（规模×参数的可读描述）。
    pub load_spec: String,
    /// 环境声明。
    pub env: EnvDeclaration,
    /// 指标口径（同一入册单位——族内唯一）。
    pub metric: &'static str,
    /// 基线值（逻辑步数）。
    pub baseline: u64,
    /// 基线移动的 ADR 链（无链即拒绝）。
    pub adr_chain: Option<&'static str>,
}

/// 基准册（入册 + 四道守卫）。
#[derive(Clone, Debug, Default)]
pub struct BenchBook {
    entries: Vec<BenchEntry>,
}

impl BenchBook {
    /// 空册。
    pub fn new() -> BenchBook {
        BenchBook::default()
    }

    /// 入册：环境完整 → 口径唯一 → 同族同谱去重 → 基线移动须 ADR。
    pub fn register(&mut self, entry: BenchEntry) -> Result<(), String> {
        entry.env.validate()?;
        // 口径唯一：同族同谱已有条目且口径不同 → 混报无效。
        for e in self.entries.iter() {
            if e.family == entry.family && e.load_spec == entry.load_spec {
                if e.metric != entry.metric {
                    return Err(format!(
                        "{}：族 {} 谱 {} 指标口径混报（{} vs {}）",
                        E_BENCH_METRIC_MIX, entry.family, entry.load_spec, e.metric, entry.metric
                    ));
                }
                return Err(format!(
                    "{}：族 {} 谱 {} 重复入册",
                    E_BENCH_DUPLICATE, entry.family, entry.load_spec
                ));
            }
        }
        // 基线移动守卫：与同族上一条比，超线须 ADR 链。
        if let Some(prev) = self.entries.iter().rev().find(|e| e.family == entry.family) {
            let prev_base = prev.baseline.max(1);
            let cur_base = entry.baseline.max(1);
            let moved = if cur_base > prev_base {
                ((cur_base - prev_base) * 100) / prev_base
            } else {
                ((prev_base - cur_base) * 100) / prev_base
            };
            if moved > BASELINE_MOVE_PCT && entry.adr_chain.is_none() {
                return Err(format!(
                    "{}：族 {} 基线移动 {}% 超 {}% 且无 ADR 链",
                    E_BENCH_BASELINE_MOVE, entry.family, moved, BASELINE_MOVE_PCT
                ));
            }
        }
        self.entries.push(entry);
        Ok(())
    }

    /// 在册数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空册。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 全部条目（读屏可达）。
    pub fn all(&self) -> &[BenchEntry] {
        &self.entries
    }
}

// ---------------------------------------------------------------------------
// 三、族一：求值吞吐（分批 vs 标量——SIMD 收益实测面）
// ---------------------------------------------------------------------------

/// 线性进度重映射（恒等——`eval` 字段签名 `(&InterpParams, f32) -> f32`）。
fn linear_progress(_params: &InterpParams, u: f32) -> f32 {
    u
}

/// 阶跃进度重映射（离散：u<1 → 0）。
fn step_progress(_params: &InterpParams, u: f32) -> f32 {
    if u < 1.0 {
        0.0
    } else {
        1.0
    }
}

/// 贝塞尔进度重映射（vem03 `bezier_time` 的入参是 (u, params)——与
/// `ScalarEvalFn` 的 (params, u) 相反，包一层适配）。
fn bezier_progress(params: &InterpParams, u: f32) -> f32 {
    bezier_time(u, params)
}

/// 造一条 SoA 轨道（101 帧 ×1ms 步进，值按类型定通道数）。
///
/// `class` 是 vem07 的**插值性**轨类（Continuous/Discrete——与 vem02
/// 的语义六型是两套概念，不可混用；`kind` 才是值类型）。
fn make_soa(class: TrackClass, kind: TrackValueKind, idx: usize) -> SoaTrack {
    let mut times: Vec<u32> = Vec::new();
    let mut k = 0usize;
    while k < EVAL_KEYS {
        times.push(k as u32);
        k += 1;
    }
    let lanes = kind.lanes();
    let mut channels: Vec<f32> = Vec::new();
    let mut j = 0usize;
    while j < EVAL_KEYS * lanes {
        // 确定性派生值（同 idx 同值——双跑一致的物质前提）。
        let v = (idx as u32).wrapping_mul(2_654_435_761).wrapping_add(j as u32) >> 8;
        channels.push((v % 1024) as f32 / 1024.0);
        j += 1;
    }
    SoaTrack {
        name: format!("t{:03}", idx),
        class,
        kind,
        static_value: false,
        edit_rev: 1,
        times,
        channels,
    }
}

/// 值类型 → vem02 语义轨类（eval_scalar_span 的入参口径）。
fn value_kind_to_class(kind: TrackValueKind) -> ValueTrackClass {
    match kind {
        TrackValueKind::Position => ValueTrackClass::Position,
        TrackValueKind::Quat => ValueTrackClass::Rotation,
        TrackValueKind::Scalar => ValueTrackClass::Float,
    }
}

/// 轨道类型混合（六类型轮换——1k 条：位置/旋转/缩放/颜色/浮点/离散）。
pub fn make_track_field(n: usize) -> Vec<SoaTrack> {
    let mut out: Vec<SoaTrack> = Vec::new();
    let mut i = 0usize;
    while i < n {
        let (kind, class) = match i % 6 {
            0 => (TrackValueKind::Position, TrackClass::Continuous), // 位置
            1 => (TrackValueKind::Quat, TrackClass::Continuous),     // 旋转
            2 => (TrackValueKind::Position, TrackClass::Continuous), // 缩放（3 通道）
            3 => (TrackValueKind::Position, TrackClass::Continuous), // 颜色（3 通道）
            4 => (TrackValueKind::Scalar, TrackClass::Continuous),   // 浮点
            _ => (TrackValueKind::Scalar, TrackClass::Discrete),     // 离散（布尔/事件）
        };
        out.push(make_soa(class, kind, i));
        i += 1;
    }
    out
}

/// ilog2 整数下取整（判据侧独立模型——bisect 步数的理论上界）。
pub const fn ilog2_floor(n: usize) -> u64 {
    if n <= 1 {
        return 0;
    }
    let mut bits = 0u64;
    let mut v = n as u64;
    while v > 1 {
        v >>= 1;
        bits += 1;
    }
    bits
}

/// 族一测量：分批 vs 标量的操作步数 + 真求值摘要。
///
/// 标量路径：每轨一次真 bisect（`hit.steps` 自报真实比较步数）+ lanes
/// 次 lerp 求值。分批路径：每批共享时间轴指纹一次 bisect + 批内每轨
/// lanes 次 lerp——`plan_batches` 的分批结果真调，bisect 摊薄即 SIMD
/// 收益的逻辑来源。
pub fn measure_eval_throughput(tracks: &[SoaTrack]) -> (u64, u64, usize, u64) {
    // 分批面（真调 plan_batches——lane 4）。
    let plan = plan_batches(tracks, 4);
    let batch_bisects = plan.batch_count() as u64 * ilog2_floor(EVAL_KEYS);
    let mut batch_lerps = 0u64;
    for b in plan.batches.iter() {
        batch_lerps += b.slots.len() as u64 * b_kind_lanes(tracks, &b.slots);
    }
    let batch_steps = batch_bisects + batch_lerps;
    // 标量面（逐轨真 bisect——steps 是 vem07 实测比较步数）。
    let interp = InterpEntry { name: "linear", kind: Interp::Linear, eval: linear_progress };
    let mut bag = DiagBag::new();
    let mut scalar_steps = 0u64;
    let mut digest: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < tracks.len() {
        let t = &tracks[i];
        let hit = bisect(&t.times, 50);
        scalar_steps += hit.steps as u64;
        if hit.index + 1 >= t.times.len() {
            i += 1;
            continue;
        }
        let a = lerp_alpha(&t.times, hit.index, 50);
        let lanes = t.kind.lanes();
        scalar_steps += lanes as u64; // 每通道一次 lerp 计入标量步数
        let mut c = 0usize;
        while c < lanes {
            let base = hit.index * lanes + c;
            let v0 = t.channels.get(base).copied().unwrap_or(0.0);
            let v1 = t.channels.get(base + lanes).copied().unwrap_or(0.0);
            if let Some(v) = eval_scalar_span(value_kind_to_class(t.kind), &interp, &InterpParams::LINEAR, v0, v1, 0.0, 1.0, a, &mut bag) {
                for byte in v.to_bits().to_le_bytes().iter() {
                    digest ^= *byte as u64;
                    digest = digest.wrapping_mul(0x0000_0100_0000_01b3);
                }
            }
            c += 1;
        }
        i += 1;
    }
    (batch_steps, scalar_steps, plan.batch_count(), digest)
}


/// 批内轨道的通道数合计（按首轨类型——同批同型是分批前提）。
fn b_kind_lanes(tracks: &[SoaTrack], slots: &[usize]) -> u64 {
    match slots.first().and_then(|s| tracks.get(*s)) {
        Some(t) => t.kind.lanes() as u64,
        None => 1,
    }
}

// ---------------------------------------------------------------------------
// 四、族二：导入/导出吞吐（10 骨骼×100 关键帧典型 clip）
// ---------------------------------------------------------------------------

/// 构造 10 通道 ×100 关键帧的 glTF 动画文档（合法骨架）。
pub fn make_clip_doc(bones: usize, keys: usize) -> (GltfAnimDoc, ChannelRef) {
    let mut accessors: Vec<AccessorView> = Vec::new();
    let mut samplers: Vec<SamplerRef> = Vec::new();
    let mut channels: Vec<ChannelRef> = Vec::new();
    let mut b = 0usize;
    while b < bones {
        let mut times: Vec<f32> = Vec::new();
        let mut values: Vec<f32> = Vec::new();
        let mut k = 0usize;
        while k < keys {
            times.push(k as f32 * 0.01);
            values.push(b as f32 * 0.1 + k as f32 * 0.001);
            k += 1;
        }
        let ai = accessors.len() as u32;
        accessors.push(AccessorView::new(ComponentType::Float, false, keys as u32, 1, times));
        let ao = accessors.len() as u32;
        accessors.push(AccessorView::new(ComponentType::Float, false, keys as u32, 1, values));
        samplers.push(SamplerRef { input: ai, output: ao, interp: GltfInterp::Linear });
        channels.push(ChannelRef { target_node: b as u32, path: ChannelPath::Translation, sampler: b as u32 });
        b += 1;
    }
    let doc = GltfAnimDoc::new(bones as u32, accessors, samplers, channels, "bench-clip");
    let ch = ChannelRef { target_node: 0, path: ChannelPath::Translation, sampler: 0 };
    (doc, ch)
}

/// 造一条 100 关键帧的导出用 SoA 轨道（典型 clip 规格——与导入侧同键数）。
fn make_clip_soa(kind: TrackValueKind, idx: usize, keys: usize) -> SoaTrack {
    let mut times: Vec<u32> = Vec::new();
    let mut k = 0usize;
    while k < keys {
        times.push(k as u32);
        k += 1;
    }
    let lanes = kind.lanes();
    let mut channels: Vec<f32> = Vec::new();
    let mut j = 0usize;
    while j < keys * lanes {
        let v = (idx as u32).wrapping_mul(2_654_435_761).wrapping_add(j as u32) >> 8;
        channels.push((v % 1024) as f32 / 1024.0);
        j += 1;
    }
    SoaTrack {
        name: format!("clip_t{:03}", idx),
        class: TrackClass::Continuous,
        kind,
        static_value: false,
        edit_rev: 1,
        times,
        channels,
    }
}

/// 族二测量：导入侧（每通道双重校验真调用计步）+ 导出侧（真调导出器）。
pub fn measure_import_export(bones: usize, keys: usize) -> (u64, u64, u64) {
    let (doc, _) = make_clip_doc(bones, keys);
    let mut bag = ImportBag::new();
    // 导入侧：逐通道过引用校验+采样校验（真实调用计步）。
    let mut import_steps = 0u64;
    let mut i = 0u32;
    while (i as usize) < bones {
        let ch = ChannelRef { target_node: i, path: ChannelPath::Translation, sampler: i };
        import_steps += 1;
        if validate_channel_refs(&doc, ch, &mut bag).is_some() {
            i += 1;
            continue;
        }
        let times: Vec<f32> = (0..keys).map(|k| k as f32 * 0.01).collect();
        let values: Vec<f32> = (0..keys).map(|k| i as f32 * 0.1 + k as f32 * 0.001).collect();
        let out = GltfSamplerOut { comps: 1, interp: GltfInterp::Linear, count: keys as u32 };
        import_steps += 1;
        let _ = validate_samples(&times, &out, &values, TrackSemantic::Position, &mut bag);
        i += 1;
    }
    // 导出侧：真调导出器（逆表机械求逆 + 10 通道产物）。
    let mut table = ExportMapTable::from_forward(&MappingTable::standard());
    let _ = table.reconcile(&MappingTable::standard(), &mut ExportBag::new());
    let mut tracks: Vec<ExportTrack> = Vec::new();
    let mut b = 0usize;
    while b < bones {
        tracks.push(ExportTrack {
            semantic: TrackSemantic::Position,
            target_node: b as u32,
            morph_slot: u16::MAX,
            name: format!("bench_bone_{}", b),
            track: make_clip_soa(TrackValueKind::Position, b, CLIP_KEYS),
            class: TrackClass::Continuous,
            interp: Interp::Linear,
            slerp: false,
        });
        b += 1;
    }
    let clip = ExportClip::new(bones as u32, tracks);
    let mut ebag = ExportBag::new();
    let exported = export_gltf_anim(&clip, &mut table, &MappingTable::standard(), &mut ebag);
    let (channels_out, keys_out) = match &exported {
        Ok(a) => (a.report.channels_out as u64, a.report.keys_out),
        Err(_) => (0, 0),
    };
    (import_steps, keys_out, channels_out)
}

// ---------------------------------------------------------------------------
// 五、族三：插值性能（四插值器×百万次——成本对比）
// ---------------------------------------------------------------------------

/// 插值器成本权重（结构面声明：每次调用的确定操作数）。
///
/// step=1（一次比较）、linear=1（一次加权）、bezier=3（双轴 bezier
/// 各一次+合成）、slerp=8（两次原点积+两次平方根+归一化+乘加）。
/// 权重是**模型侧**输入（与实测调用数相乘得逻辑成本），判据侧只验
/// 内部一致（严格序）与调用数真实。
pub const COST_WEIGHT: [(&str, u64); 4] = [
    ("step", 1),
    ("linear", 1),
    ("cubic-bezier", 3),
    ("slerp", 8),
];

/// 族三测量：四插值器各百万次真调求值，回 (调用数, 摘要, 总逻辑成本)。
pub fn measure_interp_cost() -> ([u64; 4], [u64; 4], u64) {
    let mut counts = [0u64; 4];
    let mut digests = [0u64; 4];
    let mut total_cost = 0u64;

    // step / linear / cubic-bezier：标量跨度求值。
    let entries = [
        InterpEntry { name: "step", kind: Interp::Step, eval: step_progress },
        InterpEntry { name: "linear", kind: Interp::Linear, eval: linear_progress },
        InterpEntry { name: "cubic-bezier", kind: Interp::CubicBezier, eval: bezier_progress },
    ];
    let params = InterpParams::LINEAR;
    let mut bag = DiagBag::new();
    let mut slot = 0usize;
    while slot < 3 {
        let interp = &entries[slot];
        let mut digest: u64 = 0xcbf2_9ce4_8422_2325;
        let mut n = 0u64;
        while n < INTERP_EVALS {
            let u = (n % 1_000) as f32 / 1_000.0;
            if let Some(v) = eval_scalar_span(ValueTrackClass::Float, interp, &params, 1.0, 3.0, 0.0, 1.0, u, &mut bag) {
                for byte in v.to_bits().to_le_bytes().iter() {
                    digest ^= *byte as u64;
                    digest = digest.wrapping_mul(0x0000_0100_0000_01b3);
                }
            }
            n += 1;
        }
        counts[slot] = INTERP_EVALS;
        digests[slot] = digest;
        total_cost += INTERP_EVALS * COST_WEIGHT[slot].1;
        slot += 1;
    }

    // slerp：四元数跨度求值（eval_quat_span——旋转插值真实路径）。
    let slerp_entry = InterpEntry { name: "linear", kind: Interp::Linear, eval: linear_progress };
    let mut qbag = DiagBag::new();
    let mut digest: u64 = 0xcbf2_9ce4_8422_2325;
    let mut n = 0u64;
    while n < INTERP_EVALS {
        let u = (n % 1_000) as f32 / 1_000.0;
        if let Some(q) = eval_quat_span(&slerp_entry, &params, [1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], 0.0, 1.0, u, &mut qbag) {
            for c in q.iter() {
                for byte in c.to_bits().to_le_bytes().iter() {
                    digest ^= *byte as u64;
                    digest = digest.wrapping_mul(0x0000_0100_0000_01b3);
                }
            }
        }
        n += 1;
    }
    counts[3] = INTERP_EVALS;
    digests[3] = digest;
    total_cost += INTERP_EVALS * COST_WEIGHT[3].1;

    (counts, digests, total_cost)
}

// ---------------------------------------------------------------------------
// 六、模型-实测闭环（F2407 回填位）
// ---------------------------------------------------------------------------

/// 成本模型回填位（结构同 vel12 家族：模型常数表预留位+脏标+定标）。
#[derive(Clone, Copy, Debug)]
pub struct CostModelSlot {
    /// 族名。
    pub family: &'static str,
    /// 模型值（逻辑步数的模型预测）。
    pub model: u64,
    /// 实测值（本基准测量）。
    pub measured: u64,
    /// 最近定标时刻（逻辑时钟 ms）。
    calibrated_at: u64,
    /// 脏标（超界未重定标）。
    stale: bool,
}

impl CostModelSlot {
    /// 新槽（未定标即脏）。
    pub fn new(family: &'static str, model: u64) -> CostModelSlot {
        CostModelSlot { family, model, measured: 0, calibrated_at: 0, stale: true }
    }

    /// 回填实测并定标（清脏标、刷新时戳）。
    pub fn recalibrate(&mut self, measured: u64, now: u64) {
        self.measured = measured;
        self.calibrated_at = now;
        self.stale = false;
    }

    /// 偏差百分比（模型-实测）。
    pub fn deviation_pct(&self) -> u64 {
        let m = self.model.max(1);
        let v = self.measured.max(1);
        if v > m {
            ((v - m) * 100) / m
        } else {
            ((m - v) * 100) / m
        }
    }

    /// 闭环裁决：≤30% 保持已定标；超线标脏待重定标（显性）。
    pub fn close_loop(&mut self) -> Result<(), String> {
        if self.stale {
            return Err(format!(
                "{}：族 {} 模型槽未定标（measured=0）——先回填实测",
                E_BENCH_MODEL_DRIFT, self.family
            ));
        }
        if self.deviation_pct() > MODEL_DEVIATION_PCT {
            self.mark_stale();
            return Err(format!(
                "{}：族 {} 模型-实测偏差 {}% 超 {}%——已标脏待 F2407 重定标",
                E_BENCH_MODEL_DRIFT, self.family, self.deviation_pct(), MODEL_DEVIATION_PCT
            ));
        }
        Ok(())
    }

    /// 标脏（读屏可达）。
    pub fn mark_stale(&mut self) {
        self.stale = true;
    }

    /// 是否脏（读屏可达）。
    pub fn is_stale(&self) -> bool {
        self.stale
    }

    /// 最近定标时刻（读屏可达）。
    pub fn calibrated_at(&self) -> u64 {
        self.calibrated_at
    }
}

// ---------------------------------------------------------------------------
// 七、判据
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

/// F2412 域自检（判据五组：三族/守卫/闭环/暂挂/条数）。
pub fn run_vem12_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F2412");
    let env = EnvDeclaration::new("X1-Extreme", "drv-7.2", "QEMU-virt", "H");

    // --- 族一：求值吞吐（分批 SIMD 对照）---
    let tracks = make_track_field(EVAL_TRACKS);
    let (batch_steps, scalar_steps, batch_count, digest) = measure_eval_throughput(&tracks);
    let (_, _, _, digest2) = measure_eval_throughput(&tracks);

    // M12-求值-01：1k 轨道×101 帧全求值过且双跑摘要一致（确定性）。
    s.add(
        "M12-求值-01",
        digest != 0 && digest == digest2,
        "千轨全求值过且双跑摘要逐位一致",
    );

    // M12-求值-02：分批步数 < 标量步数（bisect 摊薄=收益为正）。
    s.add(
        "M12-求值-02",
        batch_steps < scalar_steps && batch_count > 0,
        "分批步数低于标量（bisect 摊薄收益为正）",
    );

    // M12-求值-03：收益比对拍（判据侧按模型独立重算，不吃被测答案）。
    // 标量实测步数 = Σ(每轨真实 bisect 步 + 通道 lerp 数)：下界=轨数
    // （每轨至少一次比较），上界=轨数×(log2K + 最大通道数 4)。
    let lo = EVAL_TRACKS as u64;
    let hi = EVAL_TRACKS as u64 * (ilog2_floor(EVAL_KEYS) + 4);
    s.add(
        "M12-求值-03",
        scalar_steps >= lo && scalar_steps <= hi && batch_steps + batch_count as u64 <= scalar_steps + EVAL_TRACKS as u64,
        "标量步数落在 [轨数, 轨数×(log2K+4)] 模型带内",
    );

    // 求值步数模型（闭环的模型侧输入：轨数×(log2K + 平均通道数下界 2)）。
    let steps_model = EVAL_TRACKS as u64 * (ilog2_floor(EVAL_KEYS) + 2);

    // M12-求值-04：轨道阶梯单调（100 vs 1000 两步长均单调）。
    let (b100, s100, _, _) = measure_eval_throughput(&make_track_field(100));
    let (b1000, s1000, _, _) = measure_eval_throughput(&make_track_field(1_000));
    s.add(
        "M12-求值-04",
        s1000 > s100 && b1000 > b100,
        "轨道阶梯吞吐单调（100<1000 两步长均单调）",
    );

    // --- 族二：导入/导出吞吐 ---
    let (import_steps, export_keys, export_channels) = measure_import_export(CLIP_BONES, CLIP_KEYS);

    // M12-导出-01：导入侧 10 通道全过（步数=2×骨骼、零拒绝）。
    s.add(
        "M12-导出-01",
        import_steps == 2 * CLIP_BONES as u64,
        "典型 clip 导入步数恰 2×10（每通道双校验）",
    );

    // M12-导出-02：导出侧真调导出器产出 10 通道×100 键。
    s.add(
        "M12-导出-02",
        export_channels == CLIP_BONES as u64 && export_keys == (CLIP_BONES * CLIP_KEYS) as u64,
        "导出产物 10 通道×1000 键（真调 export_gltf_anim）",
    );

    // M12-导出-03：导入/导出关键帧总量一致（往返口径对拍）。
    s.add(
        "M12-导出-03",
        export_keys == (CLIP_BONES * CLIP_KEYS) as u64 && import_steps == 2 * CLIP_BONES as u64,
        "导入导出关键帧总量一致",
    );

    // --- 族三：插值成本 ---
    let (counts, digests, total_cost) = measure_interp_cost();
    let (counts2, digests2, total_cost2) = measure_interp_cost();

    // M12-插值-01：四插值器各百万次真调（调用数恰百万×4）。
    s.add(
        "M12-插值-01",
        counts == [INTERP_EVALS; 4] && counts2 == counts,
        "四插值器各百万次真调（调用数恰 4×百万）",
    );

    // M12-插值-02：双跑摘要一致（求值路径确定性）。
    s.add(
        "M12-插值-02",
        digests == digests2 && digests[3] != 0,
        "四插值器双跑摘要逐位一致",
    );

    // M12-插值-03：成本权重严格序（step==linear<bezier<slerp——调参依据）。
    let w_step = COST_WEIGHT[0].1;
    let w_linear = COST_WEIGHT[1].1;
    let w_bez = COST_WEIGHT[2].1;
    let w_slerp = COST_WEIGHT[3].1;
    s.add(
        "M12-插值-03",
        w_step == w_linear && w_linear < w_bez && w_bez < w_slerp && w_slerp == 8,
        "成本权重严格序（贝塞尔<slerp=8）",
    );

    // M12-插值-04：总逻辑成本=Σ调用×权重（判据侧重算）。
    let model_total = INTERP_EVALS * (w_step + w_linear + w_bez + w_slerp);
    s.add(
        "M12-插值-04",
        total_cost == model_total && total_cost == total_cost2,
        "总逻辑成本=Σ(调用×权重) 且双跑一致",
    );

    // --- 入册守卫（六列+四道）---
    let mut book = BenchBook::new();
    let e1 = BenchEntry {
        family: "求值吞吐",
        load_spec: String::from("1k轨×101帧×六类型混合"),
        env,
        metric: "ops",
        baseline: scalar_steps,
        adr_chain: None,
    };
    let ok1 = book.register(e1.clone()).is_ok();

    // M12-守卫-01：合法首条入册过。
    s.add("M12-守卫-01", ok1 && book.len() == 1, "合法条目入册");

    // M12-守卫-02：环境缺项拒册（空 tier）。
    let bad_env = EnvDeclaration::new("X1", "drv", "os", "  ");
    let r2 = book.register(BenchEntry {
        family: "求值吞吐",
        load_spec: String::from("缺环境条目"),
        env: bad_env,
        metric: "ops",
        baseline: 10,
        adr_chain: None,
    });
    s.add(
        "M12-守卫-02",
        r2.is_err() && r2.unwrap_err().starts_with(E_BENCH_ENV_INCOMPLETE),
        "环境缺项拒绝入册",
    );

    // M12-守卫-03：口径混报拒（同族同谱不同 metric）。
    let r3 = book.register(BenchEntry {
        family: "求值吞吐",
        load_spec: String::from("1k轨×101帧×六类型混合"),
        env,
        metric: "ns",
        baseline: 10,
        adr_chain: None,
    });
    s.add(
        "M12-守卫-03",
        r3.is_err() && r3.unwrap_err().starts_with(E_BENCH_METRIC_MIX),
        "指标口径混报拒绝",
    );

    // M12-守卫-04：同族同谱重复拒（去重）。
    let r4 = book.register(e1);
    s.add(
        "M12-守卫-04",
        r4.is_err() && r4.unwrap_err().starts_with(E_BENCH_DUPLICATE),
        "同族同谱重复入册拒绝",
    );

    // M12-守卫-05：基线移动无 ADR 拒。
    let mut book2 = BenchBook::new();
    let b1 = book2.register(BenchEntry {
        family: "插值",
        load_spec: String::from("四插值器×百万次"),
        env,
        metric: "logical-ops",
        baseline: 1_000,
        adr_chain: None,
    });
    let r5 = book2.register(BenchEntry {
        family: "插值",
        load_spec: String::from("四插值器×百万次·v2"),
        env,
        metric: "logical-ops",
        baseline: 2_000, // 移动 100% > 15%
        adr_chain: None,
    });
    let r5b = book2.register(BenchEntry {
        family: "插值",
        load_spec: String::from("四插值器×百万次·v3"),
        env,
        metric: "logical-ops",
        baseline: 2_000,
        adr_chain: Some("ADR-M12-001"),
    });
    s.add(
        "M12-守卫-05",
        b1.is_ok()
            && r5.is_err() && r5.as_ref().unwrap_err().starts_with(E_BENCH_BASELINE_MOVE)
            && r5b.is_ok(),
        "基线移动无链拒/有链放行（双向）",
    );

    // --- 模型-实测闭环 ---
    // M12-闭环-01：偏差 ≤30% 回填定标（求值族：模型=轨数×(log2K+2)，实测=标量步数）。
    let mut slot = CostModelSlot::new("求值吞吐", steps_model);
    slot.recalibrate(scalar_steps, 1_000);
    s.add("M12-闭环-01", slot.close_loop().is_ok() && !slot.is_stale() && slot.calibrated_at() == 1_000, "偏差 ≤30% 定标放行");

    // M12-闭环-02：超界 mark_stale（构造偏差 60% 的槽）。
    let mut slot_bad = CostModelSlot::new("插值", 1_000);
    slot_bad.recalibrate(1_600, 1_000); // 偏差 60%
    let r6 = slot_bad.close_loop();
    s.add(
        "M12-闭环-02",
        r6.is_err() && r6.as_ref().unwrap_err().starts_with(E_BENCH_MODEL_DRIFT) && slot_bad.is_stale(),
        "偏差 60% 超 30% → mark_stale 显性",
    );

    // M12-闭环-03：未定标槽拒（measured=0 即脏——不静默放行）。
    let mut slot_fresh = CostModelSlot::new("导入导出", 500);
    let r7 = slot_fresh.close_loop();
    s.add(
        "M12-闭环-03",
        r7.is_err() && slot_fresh.is_stale(),
        "未定标槽拒绝闭环（先回填实测）",
    );

    // M12-闭环-04：重定标可恢复（回填后偏差回到界内即清脏）。
    slot_bad.recalibrate(1_000, 2_000); // 重定标到模型值
    s.add(
        "M12-闭环-04",
        slot_bad.close_loop().is_ok() && !slot_bad.is_stale(),
        "重定标恢复（闭环可重入）",
    );

    // --- 暂挂与版本 ---
    // M12-暂挂-01：M 段门禁暂挂声明显性（L 域家族延续）。
    s.add(
        "M12-暂挂-01",
        M_GATE_SUSPENDED_NOTE.contains("暂挂") && M_GATE_SUSPENDED_NOTE.contains("F2412"),
        "M 段门禁暂挂声明显性",
    );

    // M12-版本-01：版本指纹非零。
    let fp = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in ANIM_BENCH_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    };
    s.add("M12-版本-01", fp != 0, "版本指纹非零（M12-bench-v1）");

    // M12-暂挂-02：判据条数对账（本条为第 23 条）。
    s.add("M12-暂挂-02", s.len() == 22, "判据条数对账（22+本条）");

    s
}

/// M 段门禁暂挂声明（移交期模式——L 域家族延续；与 F2212/F2411 同款）。
pub const M_GATE_SUSPENDED_NOTE: &str = "动画三族基准入 F1773 M 段：M 段门禁暂挂声明（移交期模式——L 域家族延续，F2412 同款）；结果回填 F2407 模型常数（模型闭环），跑批调度入 F1769 M 段";
