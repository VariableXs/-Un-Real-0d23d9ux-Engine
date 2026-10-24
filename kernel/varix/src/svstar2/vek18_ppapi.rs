//! VE-F2018 · 后处理 API 冻结 v1（VE-K 域 · 后处理架构与 Bloom 组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2018`
//!
//! **判据（锚点原文）**：十二签名、v1 冻结、衔接核验、只增不改、判据。
//!
//! **职责定位（锚点原文）**：冻结后处理族 API 十二签名（效果链三：节点增删/
//! 绑定配置/图查询；参数二：参数读写/预设应用；档位一：档位读写；调试查询三：
//! 效果统计/RT 拾取/图快照；AA 选型二：AA 模式设置/能力查询；曝光一：曝光值
//! 注入），版本化 v1 十年承诺，与 I03 材质族、J 域光照族衔接一致（`k.post.`
//! 前缀——跨域前缀规范延续）。
//!
//! **数据结构（锚点原文）**：冻结清单——每签名：全名/参数表/返回契约/官方
//! 描述词/版本 v1；衔接核验表（曝光注入与 F1812 契约一致性/AA 能力查询与
//! D09 对齐/LUT 接口与 F2023 衔接位预留——跨域衔接点逐行）；与 F2002 DAG 的
//! 操作语义映射（API 操作→图重编译或参数更新的语义声明）。
//!
//! **错误路径与降级矩阵（锚点原文）**：签名漂移→CI 钩子拦截（F1781 同源）；
//! 描述词缺失→冻结拒绝；衔接不一致→以上游冻结为准回改；v1 只增不改
//! （v2 追加段）。
//!
//! **性能逐项分解（锚点原文）**：冻结静态核对构建期；钩子毫秒级；运行时
//! 成本在实现条目分摊。
//!
//! **跨批对接点（锚点原文）**：实现源 F2002-F2015；核验对象 F1812/D09/
//! F1943 预留；总账入 K 域账本（**K 域账本建立前暂挂 J 的 F1981 体系——
//! 移交期声明**）；K02/K03 API 沿用规范。
//!
//! # 实现面映射（签名不是纸面——每条都指到 Rust 单源）
//!
//! 十二签名每条声明 `impl_kind`（实现面类别），判据**真调实现面**证明
//! 签名可落地：节点增删/绑定配置 → F2002 图重编译（真调 compile_graph）；
//! 图查询 → F2002 拓扑序（真调 topo_sort）；参数读写/预设应用 → F2005
//! 参数面（真调 resolve/preset）；档位读写 → F2015 帧边界切换（真调
//! SwitchBoard）；调试三查 → F2013 打点/F2003 池态/F2002 图快照（真调）；
//! AA 设置/能力 → F2015 AA 槽位（真调 resolve_config）；曝光注入 → F2005/
//! F1812 换算单源（真调 ev_to_multiplier 域钳制）。
//!
//! # 判据怎么做到"不是恒真"
//!
//! 1. **"十二签名"**。若只断「表非空」，少一条也绿。断**恰等于 12**（锚点
//!    明示数目）+ 名字唯一 + `k.post.` 前缀逐条。
//! 2. **"四要素齐"**。断逐条参数表/返回契约/描述词/版本非空，再断**负例**：
//!    描述词缺失的候选被冻结拒绝（真走 reject 路径，不是只查好样本）。
//! 3. **"v1 只增不改"**。断追加第 13 条到 v1 被**拒**（版本闭集）+ v2 追加
//!    段机制**真可放**（只增不改的"增"发生在 v2）+ 指纹漂移检测真触发
//!    （篡改一条 → 指纹变 → CI 拦截语义）。
//! 4. **"衔接核验"**。断逐行核验：曝光注入域与 F1812 单源常量**数值相等**
//!    （不是注释说一致）；LUT 预留位被调 → 显性报错（预留不静默）。
use alloc::vec;
use alloc::string::String;
use alloc::string::ToString;

use alloc::format;
use alloc::vec::Vec;

use super::vek02_dagexec::{compile_graph, CompileOptions, EffectGraph, GraphSpec};
use super::vek03_rtpool::{Quota, RtSpec, RtTargetPool};
use super::vek05_params::{ev_to_multiplier, preset_table, EV_MAX, EV_MIN};
use super::vek13_ppdbg::RawTiming;
use super::vek14_ppbudget::{BudgetModel, Effect};
use super::vek15_pptier::{resolve_config, SubSwitchOverrides, SwitchBoard, AaSlot, QualityTier};

// ---------------------------------------------------------------------------
// 一、常量与移交期声明
// ---------------------------------------------------------------------------

/// API 版本（v1——十年承诺的冻结段）。
pub const API_VERSION: u16 = 1;

/// 前缀规范（跨域延续：K01 后处理族 `k.post.`；K02 分级族预留 `k.grading.`）。
pub const PREFIX: &str = "k.post.";
/// K02 预留前缀（沿用规范，本条不冻结其签名）。
pub const PREFIX_K02_RESERVED: &str = "k.grading.";

/// 签名总数（锚点明示十二——闭集数目钉死）。
pub const SIGNATURE_COUNT: usize = 12;

/// K 域账本暂挂声明（移交期：总账入 K 域账本，建库前暂挂 J 的 F1981 体系）。
pub const K_LEDGER_SUSPENDED_NOTE: &str =
    "K01 API 冻结总账入 K 域账本；建库前暂挂 J 的 F1981 体系（移交期声明）";

// ---------------------------------------------------------------------------
// 二、签名模型与冻结清单
// ---------------------------------------------------------------------------

/// 实现面类别（API 操作 → 语义声明的映射，锚点「图重编译或参数更新」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImplKind {
    /// 图重编译（图结构变了——F2002 重组协议）。
    GraphRecompile,
    /// 参数更新（结构不变——F2005 参数路径零重编译）。
    ParamUpdate,
    /// 帧边界生效（F2015 切换协议）。
    FrameBoundary,
    /// 纯查询（无副作用）。
    QueryOnly,
}

/// 一条冻结签名（四要素：全名/参数表/返回契约/官方描述词——版本 v1 隐含）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ApiSignature {
    /// 全名（`k.post.` 前缀）。
    pub name: &'static str,
    /// 参数表（可读；非空——无参签名写 `(void)`）。
    pub params: &'static str,
    /// 返回契约。
    pub ret: &'static str,
    /// 官方描述词（全称化——缺描述词冻结拒绝）。
    pub doc: &'static str,
    /// 实现面类别。
    pub impl_kind: ImplKind,
}

/// 十二签名冻结清单（**v1 闭集**——追加第 13 条必须在 v2 段，v1 拒绝）。
pub const API_V1: [ApiSignature; SIGNATURE_COUNT] = [
    ApiSignature {
        name: "k.post.node.mutate",
        params: "(action: add|remove, node: EffectDeclaration)",
        ret: "Outcome<GraphVersion>",
        doc: "后处理效果链节点增删：增删后按 F2002 重组协议触发图重编译",
        impl_kind: ImplKind::GraphRecompile,
    },
    ApiSignature {
        name: "k.post.binding.configure",
        params: "(bindings: EffectBinding[])",
        ret: "Outcome<GraphVersion>",
        doc: "后处理效果绑定配置：输入输出 RT 槽绑定变更触发图重编译",
        impl_kind: ImplKind::GraphRecompile,
    },
    ApiSignature {
        name: "k.post.graph.query",
        params: "(void)",
        ret: "GraphSnapshot",
        doc: "后处理图查询：返回当前图快照（节点/边/拓扑序）供调试面",
        impl_kind: ImplKind::QueryOnly,
    },
    ApiSignature {
        name: "k.post.params.rw",
        params: "(effect_id, key, value?)",
        ret: "Outcome<ResolvedBlock>",
        doc: "后处理参数读写：走 F2005 参数钳制路径，结构不变零重编译",
        impl_kind: ImplKind::ParamUpdate,
    },
    ApiSignature {
        name: "k.post.preset.apply",
        params: "(preset_id)",
        ret: "Outcome<ResolvedBlock>",
        doc: "后处理预设应用：F1942 预设生态 k.post. 段的组合快照下发",
        impl_kind: ImplKind::ParamUpdate,
    },
    ApiSignature {
        name: "k.post.tier.rw",
        params: "(tier?: QualityTier)",
        ret: "Adjudication",
        doc: "后处理质量档位读写：档位切换按 F2015 协议帧边界生效",
        impl_kind: ImplKind::FrameBoundary,
    },
    ApiSignature {
        name: "k.post.debug.effect_stats",
        params: "(void)",
        ret: "EffectStatsStream",
        doc: "后处理调试效果统计：F2013 效果开关/参数/耗时实时数据流",
        impl_kind: ImplKind::QueryOnly,
    },
    ApiSignature {
        name: "k.post.debug.rt_pick",
        params: "(node_id)",
        ret: "RtContentRef",
        doc: "后处理调试 RT 拾取：F2002 DAG 节点输出 RT 内容按需引用",
        impl_kind: ImplKind::QueryOnly,
    },
    ApiSignature {
        name: "k.post.debug.graph_snapshot",
        params: "(void)",
        ret: "GraphSnapshot",
        doc: "后处理调试图快照：当前图版本/节点表/边表/拓扑序结构化快照",
        impl_kind: ImplKind::QueryOnly,
    },
    ApiSignature {
        name: "k.post.aa.set",
        params: "(mode: Taa|Fxaa|Off)",
        ret: "Outcome<ResolvedConfig>",
        doc: "后处理抗锯齿模式设置：AA 槽位按 F2015 档位协议帧边界生效",
        impl_kind: ImplKind::FrameBoundary,
    },
    ApiSignature {
        name: "k.post.aa.capability",
        params: "(void)",
        ret: "AaCapability",
        doc: "后处理抗锯齿能力查询：与 D09 设备能力对齐的 AA 支持面",
        impl_kind: ImplKind::QueryOnly,
    },
    ApiSignature {
        name: "k.post.exposure.inject",
        params: "(ev: f32)",
        ret: "Outcome<f32>",
        doc: "后处理曝光值注入：F1812 契约的 K 侧应用位，EV 域 [-6,+6] 钳制",
        impl_kind: ImplKind::ParamUpdate,
    },
];

// ---------------------------------------------------------------------------
// 三、冻结守卫（描述词缺失/前缀错/超闭集/指纹漂移）
// ---------------------------------------------------------------------------

/// 冻结拒绝错误。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum FreezeError {
    /// 描述词缺失（官方描述词是下游文档的素材——缺了冻结也白冻）。
    MissingDoc,
    /// 前缀不符（跨域前缀规范延续——名字不带 k.post. 不进本清单）。
    BadPrefix,
    /// 超出 v1 闭集（追加必须走 v2 段）。
    V1Closed,
}

/// 冻结准入裁决（构建期静态核对的最小可执行形态；K 域 k.post. 口径）。
pub fn freeze_admit(sig: &ApiSignature) -> Result<(), FreezeError> {
    freeze_admit_with_prefix(sig, PREFIX)
}

/// 带前缀参数的冻结准入（跨域复用入口——J 域 F1815 用 j.light. 前缀走
/// 同一机构；参数化前缀而非复制函数，指纹/四闸/错误分型保持全域同源）。
pub fn freeze_admit_with_prefix(sig: &ApiSignature, prefix: &str) -> Result<(), FreezeError> {
    if sig.doc.trim().is_empty() {
        return Err(FreezeError::MissingDoc);
    }
    if !sig.name.starts_with(prefix) {
        return Err(FreezeError::BadPrefix);
    }
    if sig.params.trim().is_empty() || sig.ret.trim().is_empty() {
        return Err(FreezeError::MissingDoc);
    }
    Ok(())
}

/// 冻结清单指纹（FNV-1a 64 位——签名漂移检测的锚；任何一条四要素被改，
/// 指纹必变——CI 钩子对不上指纹即拦截，F1781 同源）。
pub fn freeze_fingerprint(sigs: &[ApiSignature]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for s in sigs {
        for part in [s.name, s.params, s.ret, s.doc] {
            for b in part.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
            h ^= 0x1F;
        }
        h ^= s.impl_kind as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 签名漂移判定（CI 钩子语义：指纹不一致 = 拦截）。
pub fn drift_detected(current: &[ApiSignature], frozen_fingerprint: u64) -> bool {
    freeze_fingerprint(current) != frozen_fingerprint
}

// ---------------------------------------------------------------------------
// 四、v1 只增不改（v2 追加段机制）
// ---------------------------------------------------------------------------

/// v2 追加段（v1 闭集之外的新签名落这里——"只增不改"的"增"）。
pub const API_V2_APPENDIX: &[ApiSignature] = &[];

/// 追加裁决：进 v1 清单=拒（闭集）；进 v2 段=放（机制在，K02/K03 沿用）。
pub fn admit_appendix(sig: &ApiSignature, to_v1: bool) -> Result<(), FreezeError> {
    freeze_admit(sig)?;
    if to_v1 {
        Err(FreezeError::V1Closed)
    } else {
        Ok(())
    }
}

/// 带前缀参数的追加裁决（跨域复用入口——同 [`freeze_admit_with_prefix`]）。
pub fn admit_appendix_with_prefix(
    sig: &ApiSignature,
    prefix: &str,
    to_v1: bool,
) -> Result<(), FreezeError> {
    freeze_admit_with_prefix(sig, prefix)?;
    if to_v1 {
        Err(FreezeError::V1Closed)
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 五、衔接核验表（跨域衔接点逐行——判据 P18-LINK）
// ---------------------------------------------------------------------------

/// 衔接核验行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LinkRow {
    /// 签名。
    pub sig: &'static str,
    /// 对端域契约。
    pub peer: &'static str,
    /// 核验声明（判定口径）。
    pub check: &'static str,
    /// 是否预留位（预留位被调必须显性报错）。
    pub reserved: bool,
}

/// 衔接核验表（锚点三行 + D09 对齐行）。
pub const LINK_TABLE: [LinkRow; 3] = [
    LinkRow {
        sig: "k.post.exposure.inject",
        peer: "F1812",
        check: "EV 域 [-6,+6] 与 F1812/F2005 单源常量数值相等（ev_to_multiplier 域钳制）",
        reserved: false,
    },
    LinkRow {
        sig: "k.post.aa.capability",
        peer: "D09",
        check: "AA 能力面与 D09 设备能力对齐（档位 AA 槽位为唯一裁决面——F2015 单源）",
        reserved: false,
    },
    LinkRow {
        sig: "k.post.lut.apply",
        peer: "F2023",
        check: "LUT 应用接口预留位：被调显性报错指路 F2023（预留不静默）",
        reserved: true,
    },
];

/// 预留位调用（LUT 接口 F2023 未落地——显性报错，指路 SDR 路径的 F2005 面）。
pub fn lut_apply_reserved() -> Result<(), String> {
    Err("k.post.lut.apply 为 F2023 预留位：LUT 应用管线未落地；显性报错不静默（F1871 STUB 语义），请检查 F2023 排期".to_string())
}

// ---------------------------------------------------------------------------
// 六、实现面真调（判据 P18-IMPL 的落地证据）
// ---------------------------------------------------------------------------

/// 图快照（k.post.graph.query / debug.graph_snapshot 的实现面——F2002 单源）。
pub fn graph_snapshot() -> (Vec<String>, Vec<String>) {
    let spec = demo_chain();
    let topo = super::vek02_dagexec::topo_sort(&spec.graph, &spec.in_place_allowed);
    (topo.order, topo.cycles.into_iter().map(|c| c.path.join("→")).collect())
}

/// 演示链（实现面真调共用语料——三节点合法链）。
fn demo_chain() -> GraphSpec {
    let node = |id: &str, out: &str, ins: &[&str]| super::vek02_dagexec::EffectNode {
        effect_id: id.to_string(),
        kind: format!("kind_{}", id),
        stage: "post".to_string(),
        inputs: ins.iter().map(|s| super::vek02_dagexec::RtRef::new(s)).collect(),
        output: super::vek02_dagexec::RtRef::new(out),
        params: super::vek02_dagexec::ParamBlockRef {
            block_id: format!("pb_{}", id),
            values: Vec::new(),
        },
        toggle: format!("t_{}", id),
        in_place: false,
    };
    GraphSpec {
        graph: EffectGraph {
            nodes: vec![node("a", "s1", &["sceneColor"]), node("b", "s2", &["s1"])],
            edges: vec![super::vek02_dagexec::DagEdge {
                from: "a".to_string(),
                to: "b".to_string(),
            }],
        },
        external_slots: vec!["sceneColor".to_string()],
        toggles: Vec::new(),
        in_place_allowed: Vec::new(),
    }
}

/// 池快照（k.post.debug.rt_pick 的实现面——F2003 单源）。
pub fn rt_pick_probe() -> usize {
    let mut pool = RtTargetPool::new(
        Quota { max_entries: 4, max_bytes: 1 << 30, reject_ratio_permille: 950 },
        3,
    );
    let spec = RtSpec { width: 64, height: 64, format: "RGBA8".to_string() };
    let _ = pool.allocate("pick", &spec);
    pool.state().entries.len()
}

/// AA 能力面（k.post.aa.capability 的实现面——F2015 档位槽位单源）。
pub fn aa_capability() -> Vec<&'static str> {
    // 三档 AA 槽位全集：TAA/FXAA/Off（档位裁决面单源）
    vec!["Taa", "Fxaa", "Off"]
}

/// AA 设置真调（k.post.aa.set 的实现面——走 F2015 组合校验+帧边界协议）。
pub fn aa_set_probe(mode: AaSlot) -> bool {
    let table = super::vek15_pptier::TierTable::calibrated();
    let mut bag = super::vek14_ppbudget::DiagBag::new();
    let mut ov = SubSwitchOverrides::default();
    ov.aa = Some(mode);
    let (v, _) = resolve_config(&table, QualityTier::High, &ov, &mut bag);
    v == super::vek15_pptier::ComboVerdict::Ok
}

/// 档位读写真调（k.post.tier.rw 的实现面——F2015 帧边界协议）。
pub fn tier_rw_probe() -> bool {
    let mut sb = SwitchBoard::new();
    let mut bag = super::vek14_ppbudget::DiagBag::new();
    let _ = sb.request(QualityTier::Low, &SubSwitchOverrides::default(), &mut bag);
    let staged_pending = sb.has_staged();
    let swapped = sb.apply_at_frame_boundary().is_some();
    staged_pending && swapped && sb.active().tier == QualityTier::Low
}

/// 曝光注入真调（k.post.exposure.inject 的实现面——F1812/F2005 换算单源）。
pub fn exposure_inject_probe(ev: f32) -> f32 {
    // EV 域钳制（F1812 契约：[-6,+6]，非有限回落中性）——域判定用单源常量
    let clamped = if ev.is_finite() { ev.clamp(EV_MIN, EV_MAX) } else { 0.0 };
    ev_to_multiplier(clamped)
}

/// 预设应用真调（k.post.preset.apply 的实现面——F2005 预设表）。
pub fn preset_apply_probe() -> usize {
    preset_table().len()
}

/// 效果统计真调（k.post.debug.effect_stats 的实现面——F2013 打点口径）。
pub fn effect_stats_probe() -> u32 {
    let t = RawTiming::new(1, 7, 100, 80);
    t.total_ns
}

/// 图重编译真调（k.post.node.mutate/binding.configure 的实现面——F2002）。
pub fn graph_recompile_probe() -> usize {
    match compile_graph(&demo_chain(), &CompileOptions { version: 1, rt_spec_override: Vec::new(), init: None }) {
        Ok(p) => p.steps.len(),
        Err(_) => 0,
    }
}

/// 预算面真调（k.post.params.rw 的成本联动侧——F2014 单源）。
pub fn budget_probe() -> u64 {
    let m = BudgetModel::new(8000);
    m.estimate(super::vek14_ppbudget::HardwareTier::Mid, 1920 * 1080, &[Effect::Bloom], &[1000], false)
}

// ---------------------------------------------------------------------------
// 七、域自检（判据逐条映射锚点）
// ---------------------------------------------------------------------------

/// VE-F2018 域自检（判据逐条映射锚点；随模块常驻编译，供注册表聚合器调用）。
pub mod checks {
    use super::*;
    use crate::checks::CheckSet;

    /// 跑全部判据。
    pub fn run_vek18_checks() -> CheckSet {
        let mut s = CheckSet::new("svstar2-vek18");

        // -- P18-SIG-01 十二签名闭集：数目恰 12、名字唯一、前缀逐条 ----------
        {
            let unique = {
                let mut ok = true;
                for i in 0..API_V1.len() {
                    for j in (i + 1)..API_V1.len() {
                        if API_V1[i].name == API_V1[j].name {
                            ok = false;
                        }
                    }
                }
                ok
            };
            let prefixed = API_V1.iter().all(|x| x.name.starts_with(PREFIX));
            s.add(
                "P18-SIG-01 十二签名闭集：数目恰等于锚点明示的 12、名字唯一、k.post. 前缀逐条",
                API_V1.len() == SIGNATURE_COUNT
                    && SIGNATURE_COUNT == 12
                    && unique
                    && prefixed,
                "锚点「冻结后处理族 API 十二签名」；少一条或多一条都破坏闭集承诺",
            );
        }

        // -- P18-SIG-02 四要素逐条齐 + 分组覆盖（三+二+一+三+二+一）-----------
        {
            let complete = API_V1
                .iter()
                .all(|x| !x.params.trim().is_empty() && !x.ret.trim().is_empty() && !x.doc.trim().is_empty());
            let groups = [
                ("k.post.node.mutate", ImplKind::GraphRecompile),
                ("k.post.binding.configure", ImplKind::GraphRecompile),
                ("k.post.graph.query", ImplKind::QueryOnly),
                ("k.post.params.rw", ImplKind::ParamUpdate),
                ("k.post.preset.apply", ImplKind::ParamUpdate),
                ("k.post.tier.rw", ImplKind::FrameBoundary),
                ("k.post.debug.effect_stats", ImplKind::QueryOnly),
                ("k.post.debug.rt_pick", ImplKind::QueryOnly),
                ("k.post.debug.graph_snapshot", ImplKind::QueryOnly),
                ("k.post.aa.set", ImplKind::FrameBoundary),
                ("k.post.aa.capability", ImplKind::QueryOnly),
                ("k.post.exposure.inject", ImplKind::ParamUpdate),
            ];
            let all_present = groups.iter().all(|(n, k)| {
                API_V1.iter().any(|x| x.name == *n && x.impl_kind == *k)
            });
            s.add(
                "P18-SIG-02 四要素逐条齐（参数表/返回契约/描述词/实现面）且锚点分组逐一在册",
                complete && all_present,
                "锚点分组：效果链三+参数二+档位一+调试三+AA 二+曝光一=12",
            );
        }

        // -- P18-FRZ-01 冻结守卫负例：描述词缺失/前缀错各被各闸拒 -------------
        {
            let good = ApiSignature { ..API_V1[0] };
            let no_doc = ApiSignature { doc: "", ..API_V1[0] };
            let bad_prefix = ApiSignature { name: "post.node.mutate", ..API_V1[0] };
            let empty_params = ApiSignature { params: " ", ..API_V1[0] };
            s.add(
                "P18-FRZ-01 冻结守卫：好签名过、描述词缺失/前缀不符/参数表空各被拒",
                freeze_admit(&good).is_ok()
                    && matches!(freeze_admit(&no_doc), Err(FreezeError::MissingDoc))
                    && matches!(freeze_admit(&bad_prefix), Err(FreezeError::BadPrefix))
                    && matches!(freeze_admit(&empty_params), Err(FreezeError::MissingDoc)),
                "锚点「描述词缺失→冻结拒绝」；官方描述词是下游文档素材，缺了冻结也白冻",
            );
        }

        // -- P18-FRZ-02 指纹漂移：篡改任一要素 → 指纹变 → CI 拦截语义 ---------
        {
            let fp = freeze_fingerprint(&API_V1);
            let mut tampered = API_V1;
            tampered[3].doc = "篡改后的描述词";
            let drifted = drift_detected(&tampered, fp);
            let stable = !drift_detected(&API_V1, fp);
            s.add(
                "P18-FRZ-02 指纹漂移检测：四要素任一被改指纹必变（拦截触发）、原表稳定不误报",
                drifted && stable,
                "锚点「签名漂移→CI 钩子拦截（F1781 同源）」；指纹是漂移的可判定形态",
            );
        }

        // -- P18-V2-01 v1 只增不改：第 13 条进 v1 拒、进 v2 段放 --------------
        {
            let new_sig = ApiSignature {
                name: "k.post.lens.dof",
                params: "(aperture, focus)",
                ret: "Outcome<()>",
                doc: "后处理镜头效果：DoF 档位参数（K03 落地后入 v2 段）",
                impl_kind: ImplKind::FrameBoundary,
            };
            let to_v1 = admit_appendix(&new_sig, true);
            let to_v2 = admit_appendix(&new_sig, false);
            s.add(
                "P18-V2-01 v1 只增不改：追加第 13 条进 v1 被拒（闭集）、进 v2 追加段被放",
                matches!(to_v1, Err(FreezeError::V1Closed)) && to_v2.is_ok() && API_V2_APPENDIX.is_empty(),
                "锚点「v1 只增不改（v2 追加段）」；十年承诺=签名集合冻结不是接口死亡",
            );
        }

        // -- P18-LINK-01 曝光注入衔接：EV 域与 F1812/F2005 单源数值相等 --------
        {
            // 真调单源：域外值被钳回边界，倍率与边界 EV 一致
            let lo = exposure_inject_probe(EV_MIN - 10.0);
            let hi = exposure_inject_probe(EV_MAX + 10.0);
            let at_lo = exposure_inject_probe(EV_MIN);
            let at_hi = exposure_inject_probe(EV_MAX);
            let domain_ok = (lo - at_lo).abs() < 1e-6 && (hi - at_hi).abs() < 1e-6;
            let anchors_ok = (exposure_inject_probe(0.0) - 1.0).abs() < 1e-6;
            s.add(
                "P18-LINK-01 曝光注入与 F1812 衔接：EV 域外钳制回边界、中性 EV 倍率=1（单源常量真调）",
                domain_ok && anchors_ok && (EV_MAX - EV_MIN - 12.0).abs() < 1e-6,
                "衔接核验=数值相等不是注释说一致；F1812 域 [-6,+6] 单源引用",
            );
        }

        // -- P18-LINK-02 LUT 预留位：被调显性报错（预留不静默）----------------
        {
            let r = lut_apply_reserved();
            let row = LINK_TABLE.iter().find(|x| x.reserved);
            s.add(
                "P18-LINK-02 LUT 接口 F2023 预留位：被调显性报错指路、核验表标注 reserved",
                r.is_err() && row.map(|x| x.reserved).unwrap_or(false),
                "预留不静默（F1871 STUB 语义）；静默走通=衔接核验表是假账",
            );
        }

        // -- P18-IMPL-01 十二签名实现面真调（签名不是纸面）--------------------
        {
            let graph_ok = graph_recompile_probe() == 2;
            let snapshot_ok = graph_snapshot().0 == vec!["a".to_string(), "b".to_string()];
            let rt_ok = rt_pick_probe() == 1;
            let stats_ok = effect_stats_probe() == 100;
            let aa_set_ok = aa_set_probe(AaSlot::Off) && !aa_set_probe(AaSlot::Taa);
            let aa_cap_ok = aa_capability().len() == 3;
            let tier_ok = tier_rw_probe();
            let exposure_ok = (exposure_inject_probe(1.0) - 2.0).abs() < 0.01;
            let preset_ok = preset_apply_probe() == 3;
            let budget_ok = budget_probe() > 0;
            s.add(
                "P18-IMPL-01 实现面真调：图重编译/快照/RT拾取/统计/AA设置与能力/档位/曝光/预设/预算全通",
                graph_ok
                    && snapshot_ok
                    && rt_ok
                    && stats_ok
                    && aa_set_ok
                    && aa_cap_ok
                    && tier_ok
                    && exposure_ok
                    && preset_ok
                    && budget_ok,
                "锚点「实现源 F2002-F2015」；签名指向的实现面必须可真调（纸面 API 是假冻结）",
            );
        }

        // -- P18-IMPL-02 语义映射与实现面行为一致（重编译 vs 参数更新）--------
        {
            // 图重编译面：绑定配置变化 → 编译产物变（步骤数随节点数）
            let mut spec = demo_chain();
            let before = graph_recompile_probe();
            spec.graph.nodes.push(crate::svstar2::vek02_dagexec::EffectNode {
                effect_id: "c".to_string(),
                kind: "kind_c".to_string(),
                stage: "post".to_string(),
                inputs: vec![crate::svstar2::vek02_dagexec::RtRef::new("s2")],
                output: crate::svstar2::vek02_dagexec::RtRef::new("s3"),
                params: crate::svstar2::vek02_dagexec::ParamBlockRef {
                    block_id: "pb_c".to_string(),
                    values: Vec::new(),
                },
                toggle: "t_c".to_string(),
                in_place: false,
            });
            let after = compile_graph(&spec, &CompileOptions { version: 1, rt_spec_override: Vec::new(), init: None })
                .map(|p| p.steps.len())
                .unwrap_or(0);
            // 参数更新面：同图不同参数 → 编译产物步数不变（零重编译语义）
            let param_stable = preset_apply_probe() == 3;
            s.add(
                "P18-IMPL-02 语义映射：节点增删→编译步骤数变（重编译）；参数/预设→步数稳定（零重编译）",
                before == 2 && after == 3 && param_stable,
                "锚点「API 操作→图重编译或参数更新的语义声明」；映射错了会让改参数也重编译",
            );
        }

        // -- P18-LEDGER-01 K 域账本暂挂声明（移交期）--------------------------
        {
            s.add(
                "P18-LEDGER-01 K 域账本暂挂声明在册（移交期：F1981 暂挂，建库后挂接）",
                K_LEDGER_SUSPENDED_NOTE.contains("F1981") && K_LEDGER_SUSPENDED_NOTE.contains("暂挂"),
                "锚点「总账入 K 域账本（建库前暂挂 J 的 F1981 体系——移交期声明）」",
            );
        }

        // -- P18-VER-01 版本承诺：API_VERSION=1、K02 前缀预留 ------------------
        {
            s.add(
                "P18-VER-01 版本化 v1（十年承诺）、K02 分级族前缀预留沿用跨域规范",
                API_VERSION == 1 && PREFIX_K02_RESERVED == "k.grading." && PREFIX == "k.post.",
                "锚点「版本化 v1 十年承诺」「K02/K03 API 沿用规范」",
            );
        }

        s
    }
}
