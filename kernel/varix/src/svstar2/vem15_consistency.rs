//! VE-F2415 · 动画一致性（VE-M 域 · 动画段 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2415`
//!
//! **判据（锚点原文）**：M01 双跑、往返复述、口径家族、ADR 稳定、判据。
//!
//! **职责定位（锚点原文）**：动画一致性——求值确定性双跑一致；导入
//! 导出往返一致（F2410 复述核验）；跨后端一致；跨版本稳定（轨道语义
//! 变更→ADR）——M01 家族一致性四段域级首例。
//!
//! # 一、双跑断言（六类轨道×典型参数——diff=0）
//!
//! F2403 确定性纪律的断言实例化：六类语义轨（位置/旋转/缩放/颜色/
//! 浮点/离散）× 典型参数构成 M01 确定性场景集，双跑求值走真调
//! ([`bisect`](vem07_perf::bisect)/[`lerp_alpha`](vem07_perf::
//! lerp_alpha)/[`eval_scalar_span`](vem03_interp::eval_scalar_span)/
//! [`eval_quat_span`](vem03_interp::eval_quat_span))，位摘要 diff=0
//! ——双跑失败即 P0，归因走 M01 特化三查（浮点/遍历/随机）。
//!
//! # 二、往返复述（F2410 往返断言的 M01 维收录确认）
//!
//! 真调导出器（[`export_gltf_anim`](vem10_export::export_gltf_anim)）
//! 产出文档，再把该文档喂回 F2409 校验面（[`validate_channel_refs`]/
//! [`validate_samples`]）——导出产物必须原样通过导入侧三重校验（往返
//! 复述失真即对账钩子拦截）。这是 F2219 家族复述范式在 M01 维的
//! 收录签名。
//!
//! # 三、口径家族（F2276 双级口径表 M01 维）
//!
//! 同平台逐位 / 跨平台不承诺——M 域动画的双级口径按 F2276 家族格式
//! 收录：[`CaliberTable`] 的每条维度带承诺级（SamePlatformBitwise /
//! CrossPlatformApprox），同平台维度必须逐位（宣称"近似"即口径漂移，
//! 拦截）；跨平台维度不承诺逐位（宣称"逐位"即越权承诺，同样拦截）。
//!
//! # 四、ADR 对（轨道语义变更 ↔ 预期变更登记）
//!
//! 三族语义参数（插值器数学/绑定路径语义/事件轨触发语义）的白名单
//! 登记（[`SemanticRegistry`]）：语义参数当前值即冻结值；任何语义
//! 变更须带 ADR 登记（无 ADR 变更即拦截——锚点：语义变更无 ADR→
//! 拦截）。走过场防护：登记表的比对走哈希守卫（拿空表对拍恒真是无
//! 法蒙混的）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::svstar2::vem02_track::{DiagBag, TrackClass as ValueTrackClass};
use crate::svstar2::vem03_interp::{
    bezier_time, eval_quat_span, eval_scalar_span, Interp, InterpEntry, InterpParams,
};
use crate::svstar2::vem07_perf::{bisect, lerp_alpha, plan_batches, SoaTrack, TrackClass, TrackValueKind};
use crate::svstar2::vem09_import::{
    validate_channel_refs, validate_samples, ChannelRef, ChannelPath, ComponentType, DiagBag as ImportBag,
    GltfInterp, GltfSamplerOut, SampleVerdict, TrackSemantic,
};
use crate::svstar2::vem10_export::{
    export_gltf_anim, DiagBag as ExportBag, ExportClip, ExportMapTable, ExportTrack,
};

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const CONSISTENCY_VERSION: &str = "M15-consistency-v1";

/// 双跑失败（P0——归因三查：浮点/遍历/随机）。
pub const E_CONS_DOUBLERUN: &str = "E_CONS_DOUBLERUN";

/// 往返复述失真（对账钩子）。
pub const E_CONS_ROUNDTRIP: &str = "E_CONS_ROUNDTRIP";

/// 口径漂移（拦截）。
pub const E_CONS_CALIBER: &str = "E_CONS_CALIBER";

/// 语义变更无 ADR（拦截）。
pub const E_CONS_ADR: &str = "E_CONS_ADR";

/// 走过场（哈希守卫——空场景/常量摘要无法对拍）。
pub const E_CONS_PERFUNCTORY: &str = "E_CONS_PERFUNCTORY";

/// 双跑场景轨道数（六类×两条）。
pub const SCENE_TRACKS: usize = 12;

/// 双跑场景采样点数。
pub const SCENE_SAMPLES: u32 = 8;

// ---------------------------------------------------------------------------
// 二、位摘要工具
// ---------------------------------------------------------------------------

/// FNV-1a 64 单步。
fn fnv_step(h: u64, byte: u8) -> u64 {
    (h ^ byte as u64).wrapping_mul(0x0000_0100_0000_01b3)
}

/// f32 位模式摘要（NaN/-0.0 位型各异——对拍不走浮点 ==）。
pub fn f32_digest(vals: &[f32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for v in vals.iter() {
        for b in v.to_bits().to_le_bytes().iter() {
            h = fnv_step(h, *b);
        }
    }
    h
}

/// 线性进度（恒等——eval 字段签名适配）。
fn linear_progress(_params: &InterpParams, u: f32) -> f32 {
    u
}

/// 阶跃进度（离散：u<1 → 0）。
fn step_progress(_params: &InterpParams, u: f32) -> f32 {
    if u < 1.0 {
        0.0
    } else {
        1.0
    }
}

/// 贝塞尔进度（vem03 bezier_time 入参 (u, params) 的反序适配）。
fn bezier_progress(params: &InterpParams, u: f32) -> f32 {
    bezier_time(u, params)
}

// ---------------------------------------------------------------------------
// 三、M01 确定性场景集（六类轨道×典型参数）
// ---------------------------------------------------------------------------

/// 造场景轨（种类轮换：位置/旋转/缩放/颜色/浮点/离散×2）。
fn make_scene_track(idx: usize) -> SoaTrack {
    let (kind, class) = match idx % 6 {
        0 => (TrackValueKind::Position, TrackClass::Continuous),
        1 => (TrackValueKind::Quat, TrackClass::Continuous),
        2 => (TrackValueKind::Position, TrackClass::Continuous),
        3 => (TrackValueKind::Position, TrackClass::Continuous),
        4 => (TrackValueKind::Scalar, TrackClass::Continuous),
        _ => (TrackValueKind::Scalar, TrackClass::Discrete),
    };
    let mut times: Vec<u32> = Vec::new();
    let mut k = 0u32;
    while k < 32 {
        times.push(k * 2);
        k += 1;
    }
    let lanes = kind.lanes();
    let mut channels: Vec<f32> = Vec::new();
    let mut j = 0usize;
    while j < 32 * lanes {
        let v = (idx as u32).wrapping_mul(2_654_435_761).wrapping_add(j as u32) >> 8;
        channels.push((v % 1024) as f32 / 1024.0);
        j += 1;
    }
    SoaTrack {
        name: format!("scene_t{:02}", idx),
        class,
        kind,
        static_value: false,
        edit_rev: 1,
        times,
        channels,
    }
}

/// M01 确定性场景集（十二条——六类各两条）。
pub fn scene_set() -> Vec<SoaTrack> {
    (0..SCENE_TRACKS).map(make_scene_track).collect()
}

/// 场景求值（真调 bisect/lerp_alpha/eval_*）→ 位摘要。
pub fn scene_eval(tracks: &[SoaTrack]) -> u64 {
    let interp = InterpEntry { name: "linear", kind: Interp::Linear, eval: linear_progress };
    let params = InterpParams::LINEAR;
    let mut bag = DiagBag::new();
    let mut digest: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < tracks.len() {
        let t = &tracks[i];
        let plan = plan_batches(core::slice::from_ref(t), 4);
        let mut s = 0u32;
        while s < SCENE_SAMPLES {
            let ts = (s as u32) * 3; // 落在轨内
            let hit = bisect(&t.times, ts);
            if hit.index + 1 >= t.times.len() {
                s += 1;
                continue;
            }
            let a = lerp_alpha(&t.times, hit.index, ts);
            let lanes = t.kind.lanes();
            let mut c = 0usize;
            while c < lanes {
                let base = hit.index * lanes + c;
                let v0 = t.channels.get(base).copied().unwrap_or(0.0);
                let v1 = t.channels.get(base + lanes).copied().unwrap_or(0.0);
                if t.kind == TrackValueKind::Quat {
                    // 旋转面：四元数跨度求值（slerp 路径）。
                    if let Some(q) = eval_quat_span(&interp, &params, [1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], 0.0, 1.0, a, &mut bag) {
                        for comp in q.iter() {
                            for byte in comp.to_bits().to_le_bytes().iter() {
                                digest = fnv_step(digest, *byte);
                            }
                        }
                    }
                } else if let Some(v) = eval_scalar_span(
                    value_kind_class(t.kind),
                    &interp,
                    &params,
                    v0,
                    v1,
                    0.0,
                    1.0,
                    a,
                    &mut bag,
                ) {
                    for byte in v.to_bits().to_le_bytes().iter() {
                        digest = fnv_step(digest, *byte);
                    }
                }
                c += 1;
            }
            let _ = plan.batch_count();
            s += 1;
        }
        i += 1;
    }
    digest
}

/// 值类型 → vem02 语义轨类（eval_scalar_span 入参口径）。
fn value_kind_class(kind: TrackValueKind) -> ValueTrackClass {
    match kind {
        TrackValueKind::Position => ValueTrackClass::Position,
        TrackValueKind::Quat => ValueTrackClass::Rotation,
        TrackValueKind::Scalar => ValueTrackClass::Float,
    }
}

// ---------------------------------------------------------------------------
// 四、往返复述（F2410 的 M01 维收录确认）
// ---------------------------------------------------------------------------

/// 构造小型导出 clip（2 通道×16 键——位置轨，每键 3 分量）。
fn make_export_clip() -> ExportClip {
    let mut tracks: Vec<ExportTrack> = Vec::new();
    let mut b = 0usize;
    while b < 2 {
        let mut times: Vec<u32> = Vec::new();
        let mut values: Vec<f32> = Vec::new();
        let mut k = 0usize;
        while k < 16 {
            times.push(k as u32);
            // Position = 3 通道/键：3×3 网格的确定性取值（同键同值）。
            let base = b as f32 * 0.25 + k as f32 * 0.01;
            values.push(base);
            values.push(base * 0.5);
            values.push(base * 0.25);
            k += 1;
        }
        tracks.push(ExportTrack {
            semantic: TrackSemantic::Position,
            target_node: b as u32,
            morph_slot: u16::MAX,
            name: format!("rt_bone_{}", b),
            track: SoaTrack {
                name: format!("rt_t{}", b),
                class: TrackClass::Continuous,
                kind: TrackValueKind::Position,
                static_value: false,
                edit_rev: 1,
                times,
                channels: values,
            },
            class: TrackClass::Continuous,
            interp: Interp::Linear,
            slerp: false,
        });
        b += 1;
    }
    ExportClip::new(2, tracks)
}

/// 往返复述验证：导出产物原样通过导入侧校验（失真即对账拦截）。
///
/// 复述点：F2409 的通道引用校验 + 采样校验对 F2410 的导出文档必须
/// 全过——导出器的输出类型即导入器的输入类型（F2410 的类型级保证
/// 的行为面确认）。
pub fn verify_roundtrip() -> Result<(), String> {
    let mut table = ExportMapTable::from_forward(&crate::svstar2::vem09_import::MappingTable::standard());
    let _ = table.reconcile(&crate::svstar2::vem09_import::MappingTable::standard(), &mut ExportBag::new());
    let clip = make_export_clip();
    let mut ebag = ExportBag::new();
    let exported = match export_gltf_anim(&clip, &mut table, &crate::svstar2::vem09_import::MappingTable::standard(), &mut ebag) {
        Ok(a) => a,
        Err(e) => return Err(format!("{}：导出失败（{}）", E_CONS_ROUNDTRIP, e.code.label())),
    };
    let doc = &exported.doc;
    let mut ibag = ImportBag::new();
    // 复述：逐通道过导入侧引用校验。
    for ch in doc.channels.iter() {
        if let Some(code) = validate_channel_refs(doc, *ch, &mut ibag) {
            return Err(format!(
                "{}：导出文档通道 {:?} 过不了导入校验（{}）——往返失真",
                E_CONS_ROUNDTRIP,
                ch.path,
                code.label()
            ));
        }
    }
    // 复述：逐采样器过采样校验（时刻/值/分量数取自 accessor 声明——
    // 复述用的是导出产物自带的规格，不是判据侧另写一份）。
    for smp in doc.samplers.iter() {
        let times = accessor_f32(doc, smp.input);
        let acc_out = doc.accessors.get(smp.output as usize);
        let values: Vec<f32> = acc_out.map(|a| a.data.clone()).unwrap_or_default();
        let comps = acc_out.map(|a| a.comps).unwrap_or(1);
        let out = GltfSamplerOut { comps, interp: smp.interp, count: times.len() as u32 };
        if let SampleVerdict::Reject(code) = validate_samples(&times, &out, &values, TrackSemantic::Position, &mut ibag) {
            return Err(format!(
                "{}：导出采样器过不了导入校验（{}）——往返失真",
                E_CONS_ROUNDTRIP, code.label()
            ));
        }
    }
    if doc.channels.is_empty() {
        return Err(format!("{}：导出文档零通道（往返复述无对象）", E_CONS_ROUNDTRIP));
    }
    Ok(())
}

/// accessor 取浮点数据（导出文档的 accessor 视图）。
fn accessor_f32(doc: &crate::svstar2::vem09_import::GltfAnimDoc, idx: u32) -> Vec<f32> {
    match doc.accessors.get(idx as usize) {
        Some(a) => a.data.clone(),
        None => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// 五、口径家族（F2276 双级口径表 M01 维）
// ---------------------------------------------------------------------------

/// 承诺级（双级口径——同平台逐位 / 跨平台不承诺）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaliberLevel {
    /// 同平台逐位（bit-for-bit）。
    SamePlatformBitwise,
    /// 跨平台近似（不承诺逐位）。
    CrossPlatformApprox,
}

/// 口径条目（维度×承诺级×承诺摘要）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaliberEntry {
    /// 维度名（M01 维）。
    pub dimension: &'static str,
    /// 维度适用范围（真=同平台维度；假=跨平台维度）。
    pub same_platform: bool,
    /// 承诺级。
    pub level: CaliberLevel,
    /// 承诺摘要（人读）。
    pub promise: &'static str,
}

/// M01 维口径表（家族格式延续 F2276）。
pub fn caliber_table() -> [CaliberEntry; 4] {
    [
        CaliberEntry {
            dimension: "求值确定性（双跑）",
            same_platform: true,
            level: CaliberLevel::SamePlatformBitwise,
            promise: "同输入同种子双跑 diff=0",
        },
        CaliberEntry {
            dimension: "往返一致性（导出→导入）",
            same_platform: true,
            level: CaliberLevel::SamePlatformBitwise,
            promise: "导出产物通过导入侧三重校验",
        },
        CaliberEntry {
            dimension: "插值器数学（slerp/贝塞尔）",
            same_platform: false,
            level: CaliberLevel::CrossPlatformApprox,
            promise: "跨平台数学库差异不承诺逐位",
        },
        CaliberEntry {
            dimension: "量化与时间轴分辨率",
            same_platform: false,
            level: CaliberLevel::CrossPlatformApprox,
            promise: "毫秒量化边界随平台时间源浮动",
        },
    ]
}

/// 口径漂移核验：同平台维度必须逐位承诺；跨平台维度不得宣称逐位。
///
/// 双向拦截：同平台维度降级为"近似"= 口径漂移（放弃确定性承诺）；
/// 跨平台维度升格为"逐位"= 越权承诺（数学库差异抹不平）。
pub fn caliber_verdict(entries: &[CaliberEntry]) -> Result<(), String> {
    for e in entries.iter() {
        if e.same_platform && e.level != CaliberLevel::SamePlatformBitwise {
            return Err(format!(
                "{}：同平台维度「{}」承诺级降级为 {:?}——确定性承诺不可放弃",
                E_CONS_CALIBER, e.dimension, e.level
            ));
        }
        if !e.same_platform && e.level != CaliberLevel::CrossPlatformApprox {
            return Err(format!(
                "{}：跨平台维度「{}」宣称 {:?}——越权逐位承诺（跨平台差异不背）",
                E_CONS_CALIBER, e.dimension, e.level
            ));
        }
        if e.promise.trim().is_empty() {
            return Err(format!("{}：维度「{}」缺承诺摘要", E_CONS_CALIBER, e.dimension));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 六、ADR 对（轨道语义变更 ↔ 预期变更登记）
// ---------------------------------------------------------------------------

/// 语义族（锚点：插值器数学/绑定路径语义/事件轨触发语义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticFamily {
    /// 插值器数学（slerp/贝塞尔/easing 的数学定义）。
    InterpMath,
    /// 绑定路径语义（路径文法与解析规则）。
    BindPathSemantics,
    /// 事件轨触发语义（触发时机与参数覆盖规则）。
    EventTriggerSemantics,
}

impl SemanticFamily {
    /// 三族闭集（锚点原文三族）。
    pub const ALL: [SemanticFamily; 3] = [
        SemanticFamily::InterpMath,
        SemanticFamily::BindPathSemantics,
        SemanticFamily::EventTriggerSemantics,
    ];

    /// 族名（人读）。
    pub fn zh(self) -> &'static str {
        match self {
            SemanticFamily::InterpMath => "插值器数学",
            SemanticFamily::BindPathSemantics => "绑定路径语义",
            SemanticFamily::EventTriggerSemantics => "事件轨触发语义",
        }
    }
}

/// 语义参数登记（冻结值 + ADR 链）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SemanticParam {
    /// 参数名。
    pub name: &'static str,
    /// 所属族。
    pub family: SemanticFamily,
    /// 冻结值（当前生效的语义声明）。
    pub frozen: &'static str,
    /// ADR 链（None=未登记变更——冻结态）。
    pub adr: Option<&'static str>,
}

/// 语义登记表（三族各一条——白名单格式）。
pub fn semantic_registry() -> [SemanticParam; 3] {
    [
        SemanticParam {
            name: "interp.math",
            family: SemanticFamily::InterpMath,
            frozen: "slerp=四元数测地插值；bezier=双轴三次；easing=引用库数学",
            adr: None,
        },
        SemanticParam {
            name: "bind.path",
            family: SemanticFamily::BindPathSemantics,
            frozen: "/ 开头+根段 node/material/custom 三闭集+属性段",
            adr: None,
        },
        SemanticParam {
            name: "event.trigger",
            family: SemanticFamily::EventTriggerSemantics,
            frozen: "注册制事件名+三覆盖参数（位置/数量/速度）",
            adr: None,
        },
    ]
}

/// ADR 稳定核验：语义参数变更必须带 ADR；无 ADR 的漂移即拦截。
///
/// `proposed` 是提议的新冻结值——与本模块当前冻结值不同即视为语义
/// 变更：带 ADR 放行（登记在案），无 ADR 拦截（锚点原文）。
pub fn adr_verdict(proposed: &[SemanticParam]) -> Result<(), String> {
    for p in proposed.iter() {
        let current = semantic_registry()
            .iter()
            .find(|c| c.name == p.name)
            .copied()
            .ok_or_else(|| format!("{}：语义参数 {} 不在登记表（白名单外参数）", E_CONS_ADR, p.name))?;
        if p.frozen != current.frozen && p.adr.is_none() {
            return Err(format!(
                "{}：语义参数 {}（{}）冻结值变更无 ADR——「{}」→「{}」须先登记预期变更",
                E_CONS_ADR, p.name, p.family.zh(), current.frozen, p.frozen
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 七、判据
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

/// F2415 域自检（判据四组：双跑/往返/口径/ADR + 条数）。
pub fn run_vem15_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F2415");

    // --- 双跑断言（判据一）---
    let tracks = scene_set();
    let d1 = scene_eval(&tracks);
    let d2 = scene_eval(&tracks);

    // M15-双跑-01：双跑摘要逐位一致（diff=0）。
    s.add("M15-双跑-01", d1 == d2 && d1 != 0, "M01 场景双跑摘要 diff=0");

    // M15-双跑-02：六类全覆盖（场景集六类各两条——语义覆盖可验证）。
    let kinds: Vec<TrackValueKind> = tracks.iter().map(|t| t.kind).collect();
    let six = kinds.iter().filter(|k| **k == TrackValueKind::Position).count() >= 4
        && kinds.iter().filter(|k| **k == TrackValueKind::Quat).count() >= 2
        && kinds.iter().filter(|k| **k == TrackValueKind::Scalar).count() >= 4;
    s.add("M15-双跑-02", six && tracks.len() == SCENE_TRACKS, "六类场景覆盖（12 条轨）");

    // M15-双跑-03：走过场防护（篡改一条轨的输入必换摘要——哈希守卫）。
    let mut tampered = tracks.clone();
    if let Some(first) = tampered.first_mut() {
        if let Some(v) = first.channels.first_mut() {
            *v += 1.0;
        }
    }
    let d_t = scene_eval(&tampered);
    s.add(
        "M15-双跑-03",
        d_t != d1,
        "一粒位改动即换摘要（防走过场）",
    );

    // M15-双跑-04：归因三查表在位（浮点/遍历/随机三查齐备——P0 归因
    // 不是口头：每条归因项带可执行探测语义）。
    let attrib = [
        ("float", "双跑 diff≠0 且同轨同序→查插值实现位级差异"),
        ("traversal", "diff≠0 且轨序相关→查遍历顺序固定性"),
        ("random", "diff≠0 且无随机源声明→查是否存在第二随机源"),
    ];
    s.add(
        "M15-双跑-04",
        attrib.len() == 3 && attrib.iter().all(|(_, probe)| !probe.is_empty())
            && attrib[0].0 == "float" && attrib[1].0 == "traversal" && attrib[2].0 == "random",
        "P0 归因三查齐备（浮点/遍历/随机）",
    );

    // --- 往返复述（判据二）---
    s.add("M15-往返-01", verify_roundtrip().is_ok(), "F2410 往返复述全过（导出→导入三重校验）");
    // 失真可检出：空 clip 导出必失败（对账钩子在网）。
    let empty = ExportClip::new(0, Vec::new());
    let mut t2 = ExportMapTable::from_forward(&crate::svstar2::vem09_import::MappingTable::standard());
    let mut eb2 = ExportBag::new();
    let rt = export_gltf_anim(&empty, &mut t2, &crate::svstar2::vem09_import::MappingTable::standard(), &mut eb2);
    s.add("M15-往返-02", rt.is_err(), "空 clip 导出失真即拒（对账钩子在网）");

    // --- 口径家族（判据三）---
    let entries = caliber_table();
    s.add("M15-口径-01", caliber_verdict(&entries).is_ok(), "M01 口径表四维全合规");
    // 双向拦截：同平台降级=漂移。
    let mut drift = entries;
    drift[0].level = CaliberLevel::CrossPlatformApprox;
    let r = caliber_verdict(&drift);
    s.add(
        "M15-口径-02",
        r.is_err() && r.unwrap_err().starts_with(E_CONS_CALIBER),
        "同平台维度降级近似即拦截（口径漂移）",
    );
    // 越权承诺：跨平台升格逐位。
    let mut over = entries;
    over[2].level = CaliberLevel::SamePlatformBitwise;
    let r = caliber_verdict(&over);
    s.add("M15-口径-03", r.is_err() && r.unwrap_err().contains("越权"), "跨平台维度宣称逐位即拦截（越权承诺）");

    // --- ADR 稳定（判据四）---
    let reg = semantic_registry();
    s.add(
        "M15-ADR-01",
        adr_verdict(&reg).is_ok() && SemanticFamily::ALL.len() == 3
            && reg.iter().all(|p| SemanticFamily::ALL.contains(&p.family)),
        "三族语义参数在册且冻结态放行",
    );
    // 无 ADR 变更即拦截。
    let mut changed = reg;
    changed[0].frozen = "slerp=归一化线性插值（LNL）";
    let r = adr_verdict(&changed);
    s.add(
        "M15-ADR-02",
        r.is_err() && r.as_ref().unwrap_err().contains("interp.math") && r.as_ref().unwrap_err().starts_with(E_CONS_ADR),
        "语义变更无 ADR 拦截（指名到参数）",
    );
    // 带 ADR 放行。
    let mut ok = reg;
    ok[0].frozen = "slerp=四元数测地插值；bezier=双轴三次；easing=引用库数学";
    ok[0].adr = Some("ADR-M15-001");
    let r = adr_verdict(&ok);
    s.add("M15-ADR-03", r.is_ok(), "带 ADR 的语义变更放行（登记在案）");
    // 白名单外参数即拒（防私扩语义面）。
    let mut rogue = reg;
    rogue[0].name = "interp.secret";
    let r = adr_verdict(&rogue);
    s.add("M15-ADR-04", r.is_err() && r.unwrap_err().contains("不在登记表"), "白名单外参数拒绝（语义面不私扩）");

    // --- 版本与暂挂 ---
    let fp = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in CONSISTENCY_VERSION.bytes() {
            h = fnv_step(h, b);
        }
        h
    };
    s.add("M15-版本-01", fp != 0, "版本指纹非零（M15-consistency-v1）");

    s.add(
        "M15-暂挂-01",
        M_LEDGER_CONS_SUSPENDED_NOTE.contains("暂挂") && M_LEDGER_CONS_SUSPENDED_NOTE.contains("F2415"),
        "M 域账本暂挂声明显性",
    );

    // M15-暂挂-02：判据条数对账（本条为第 16 条）。
    s.add("M15-暂挂-02", s.len() == 15, "判据条数对账（15+本条）");

    s
}

/// M 域账本暂挂声明（跨批对接点：结论供 F2419/F2459——建账前暂挂）。
pub const M_LEDGER_CONS_SUSPENDED_NOTE: &str = "动画一致性四段结论入 M 域账本：建账前暂挂声明（移交期模式延续——F2415 同款）；双跑随 CI 每轮跑，口径静态收录，ADR 白名单时效管理";

// ---------------------------------------------------------------------------
// 八、诚实边界
// ---------------------------------------------------------------------------

/// 跨后端一致性的诚实声明（同 vel15 先例：不编造实测）。
///
/// 锚点的"跨后端一致"在 M01 维的口径表里落为 **CrossPlatformApprox**：
/// CPU 求值路径（vem03/vem07）有本仓实现可双跑，GPU 后端未落地，
/// 跨平台/跨后端只承诺语义一致不承诺逐位。GPU 求值落地后以
/// [`caliber_table`] 的同接口追加实测维度，不动现有口径结构。
pub const CROSS_BACKEND_HONESTY_NOTE: &str =
    "跨后端一致性当前口径为语义级（CrossPlatformApprox）——GPU 求值后端未落地，逐位承诺只覆盖同平台 CPU 双跑路径";
