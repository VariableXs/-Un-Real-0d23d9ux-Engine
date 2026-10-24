//! VE-F2016 · 后处理 fuzz（VE-K 域 · 后处理架构与 Bloom 组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2016`
//!
//! **判据（锚点原文）**：三段 fuzz、全非法形态拦截、池防线验证、24h 固化、判据。
//!
//! **职责定位（锚点原文）**：后处理 fuzz——三段模糊测试：畸形效果参数 fuzz
//! （NaN/越界全参数空间——钳制容错断言：每效果参数块自动扫描）、DAG 配置 fuzz
//! （环/缺失输入/重复输出/自依赖——校验拒绝显性验证：全部非法图形态被拦截且
//! 报错三要素）、RT 池压力 fuzz（超配额申请/别名冲突注入/泄漏式申请——F2003
//! 防线的对抗验证），案例固化入库。
//!
//! **数据结构（锚点原文）**：案例库三段——参数生成器（十效果参数块全空间扫描
//! +NaN 种子注入——复用 F1813 框架扩展 K 面）、图畸形生成器（环构造（A→B→A）/
//! 缺边/双生产者/自环——DAG 校验器的全非法形态库）、池压力（超限申请风暴/
//! 别名区间重叠构造/万次申请不回收——泄漏防线压测）；案例元数据（输入/期望/
//! 复现脚本/去重哈希）。
//!
//! **错误路径与降级矩阵（锚点原文）**：崩溃（参数打崩效果）→P0 立案 24h 复现；
//! 非法图未拦截（环被执行）→P0（校验器失守——执行正确性根基）；池防线绕过
//! （超限仍分配）→P0；钳制失灵（容错后仍非法输出）→立案；案例去重哈希管理。
//!
//! **性能逐项分解（锚点原文）**：参数扫描万级夜间批跑；图形态千级；池压测
//! 1 小时档入 F1769 组合。本模块把三段量级落进**可进程内重放的确定性语料**
//! （万级参数/千级图/万次池申请），夜间批跑由 F1769 域调度挂接——量纲一致，
//! 调度归调度。
//!
//! **跨批对接点（锚点原文）**：案例库入 F1769 域调度；立案进 F1994 J 域账本
//! （**K 域账本建库前暂挂 J 体系——移交期声明**：本模块的 [`P0Ledger`] 是
//! 暂挂面，字段与 F1994 流转对齐，建库后平移）；框架复用 F1813 同构
//! （种子驱动生成器+案例元数据+去重哈希三件套，K 面扩展）；通过率供
//! F2038/F2059。
//!
//! **被测对象（Rust 单源）**：段一 = [`super::vek05_params::BloomParamBlock`]
//! （K01 参数面，F2005 已交付）；段二 = [`super::vek02_dagexec`]（F2002 DAG
//! 执行器 **Rust 迁移版**——本任务按塔主 2026-10-07 迁移指令先行迁移）；段三
//! = [`super::vek03_rtpool`]（F2003 RT 池 **Rust 迁移版**，同上）。迁移声明
//! 与替代关系见两模块头注；TS 原文件在本任务收尾时删除（无其他引用）。
//!
//! # 判据怎么做到"不是恒真"
//!
//! fuzz 判据最容易写成恒真的地方有五处，逐一封死：
//!
//! 1. **"钳制容错"**。若语料全是合法参数，一个不做任何钳制的实现也全绿。
//!    必须断：非法样本覆盖率逐字段统计（每个参数面都吃过 NaN/±inf/越界角点）
//!    + 钳制**确实发生**（非法样本至少产生过一次诊断）+ 容错后输出逐位回域。
//! 2. **"全非法形态拦截"**。若只断「校验器对已知样本报错」，漏掉一类形态
//!    不会红。必须**穷举形态库**（二环/长环/缺生产者/双生产者/未登记自环/
//!    悬空边/重复 id——每类独立生成器），加**干净图反证**（合法链必须编译
//!    通过——否则"全拒也绿"）。
//! 3. **"池防线验证"**。若压测在配额内申请，防线永远不被试探。必须
//!    **超限风暴**（申请数 ≫ 配额，断成功数恰在配额内、拒绝全部 QuotaExceeded、
//!    零绕过）+ **泄漏压测**（万次申请不回收，断泄漏账本全量在账、峰值不破
//!    配额）+ **字节水位**路径单独触发（只有张数水位被试过的防线是半条防线）。
//! 4. **"24h 固化"**。若只断「能立案」，没有时限的立案会烂在账上。断立案
//!    记录的复现时限**恰等于 24h**（锚点明示）且绿语料**零立案**（立案是
//!    异常出口，不是常规流程）。
//! 5. **"案例去重"**。若入库不去重，案例库会被重复案例淹到不可维护。断
//!    同案例二次入库：库不增长、去重计数≥1。
use alloc::vec;
use alloc::string::String;
use alloc::string::ToString;

use alloc::format;
use alloc::vec::Vec;

use super::vek02_dagexec::{
    compile_graph, validate_graph, CompileOptions, DagEdge, EffectGraph, EffectNode, GraphSpec,
    ParamBlockRef, RtRef,
};
use super::vek03_rtpool::{
    analyze_aliases, verify_alias_groups, LiveInterval, PoolDiagCode, Quota, RtSpec, RtTargetPool,
};
use super::vek05_params::{
    BloomParamBlock, ExposureLink, Tint, STRENGTH_MAX, STRENGTH_MIN,
};

// ===========================================================================
// 一、诊断码（自建，K 域独占段 0x32xx——0x2C/2D/2E/2F/30/31 均已有主）
// ===========================================================================

/// 诊断码（K 域惯例自建新构类型——vek04/vek05/vek13/vek14 各段同款）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DiagCode(pub u16);

/// K 域 F2016 诊断码。
pub mod code {
    use super::DiagCode;

    /// P0 立案（崩溃/防线失守/非法图被执行——24h 复现，锚点明示）。
    pub const P0_FILED: DiagCode = DiagCode(0x3201);
    /// 钳制失灵（容错后仍非法输出）→ 立案（非 P0 级）。
    pub const CLAMP_FAILED: DiagCode = DiagCode(0x3202);
    /// 案例回归失败（固化案例重跑不再通过——基线漂移哨兵）。
    pub const CASE_REGRESSION: DiagCode = DiagCode(0x3203);
}

// ===========================================================================
// 二、确定性伪随机（同种子同语料——fuzz 可重放是"复现脚本"的根基）
// ===========================================================================

/// xorshift64*（自含、无依赖、确定性；TS 版 F1813 框架同构件）。
#[derive(Clone, Copy)]
pub struct Rng(u64);

impl Rng {
    /// 由种子构造（种子 0 视为 1——xorshift 全零态锁死）。
    pub fn new(seed: u64) -> Rng {
        Rng(if seed == 0 { 1 } else { seed })
    }

    /// 下一 64 位随机数。
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// [0, n) 均匀。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n.max(1)
    }

    /// 语料指纹（FNV-1a 64 位——案例去重哈希与语料一致性判定同源）。
    pub fn corpus_hash(&self) -> u64 {
        self.0
    }
}

/// FNV-1a 64 位（对字节串；案例去重哈希）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

// ===========================================================================
// 三、案例库与 P0 台账（锚点：案例固化入库 + 24h 复现）
// ===========================================================================

/// 一条固化案例（元数据四件：输入/期望/复现脚本/去重哈希）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FuzzCase {
    /// 段名（param / dag / pool）。
    pub segment: &'static str,
    /// 案例名。
    pub name: String,
    /// 输入摘要（可读）。
    pub input_desc: String,
    /// 期望（钳制输出或三要素拒绝）。
    pub expected_desc: String,
    /// 复现脚本（种子+生成器名——同种子重放逐位一致）。
    pub repro: String,
    /// 去重哈希（FNV-1a 64 位）。
    pub hash: u64,
}

/// 案例库（固化入库；同哈希去重——重复案例淹没库=库不可维护）。
#[derive(Default)]
pub struct CaseLibrary {
    cases: Vec<FuzzCase>,
    /// 去重跳过数（单调）。
    pub dups: u64,
}

impl CaseLibrary {
    /// 构造。
    pub fn new() -> CaseLibrary {
        CaseLibrary { cases: Vec::new(), dups: 0 }
    }

    /// 入库（同哈希跳过并计数）。
    pub fn record(&mut self, c: FuzzCase) -> bool {
        if self.cases.iter().any(|x| x.hash == c.hash) {
            self.dups += 1;
            return false;
        }
        self.cases.push(c);
        true
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }

    /// 全部案例。
    pub fn all(&self) -> &[FuzzCase] {
        &self.cases
    }

    /// 某段的条数。
    pub fn segment_len(&self, segment: &str) -> usize {
        self.cases.iter().filter(|c| c.segment == segment).count()
    }
}

/// 一条 P0 立案（锚点：24h 复现；移交期暂挂 J 体系 F1994——字段对齐流转）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct P0Filing {
    /// 关联案例哈希。
    pub case_hash: u64,
    /// 标题。
    pub title: String,
    /// 复现时限（小时；锚点 24h 明示）。
    pub deadline_hours: u32,
}

/// P0 台账（K 域账本建库前暂挂面——建库后平移 F1994，字段不再变）。
#[derive(Default)]
pub struct P0Ledger {
    filings: Vec<P0Filing>,
}

impl P0Ledger {
    /// 构造。
    pub fn new() -> P0Ledger {
        P0Ledger { filings: Vec::new() }
    }

    /// 立案（时限锚点硬编码 24h——不放参数，放参数就会被"宽限一次"磨掉）。
    pub fn file(&mut self, case_hash: u64, title: &str) -> &P0Filing {
        self.filings.push(P0Filing {
            case_hash,
            title: title.to_string(),
            deadline_hours: 24,
        });
        &self.filings[self.filings.len() - 1]
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.filings.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.filings.is_empty()
    }

    /// 全部立案。
    pub fn all(&self) -> &[P0Filing] {
        &self.filings
    }
}

// ===========================================================================
// 四、段一：畸形效果参数 fuzz（NaN/越界全参数空间——钳制容错断言）
// ===========================================================================

/// 段一报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ParamFuzzReport {
    /// 总样本数。
    pub cases: u64,
    /// 钳制失灵数（容错后仍非法输出——锚点立案线；绿跑必须为 0）。
    pub clamp_failures: u64,
    /// 真触发钳制的样本数（反证语料不是全合法——恒真断言防线）。
    pub clamped_inputs: u64,
    /// 逐字段非法覆盖（threshold/strength/decay/mip/tint 各 ≥1 才算全覆盖）。
    pub fields_poisoned: [u64; 5],
    /// NaN 种子注入总数。
    pub nan_seeds: u64,
}

/// 逐字段非法值域（角点 + NaN 种子——十效果参数面里 Bloom 块是唯一已落地
/// 的参数块单源（vek05），其余效果的参数面落地后按同框架扩 K 面）。
fn poison_value(kind: usize, rng: &mut Rng) -> f32 {
    match kind {
        0 => f32::NAN,
        1 => f32::INFINITY,
        2 => f32::NEG_INFINITY,
        3 => -1.0,
        4 => 1.0e9,
        5 => f32::MIN_POSITIVE * (rng.below(2) as f32 + 0.5), // 极小值
        _ => (rng.below(2001) as f32 - 1000.0) / 100.0,      // [-10,10] 随机
    }
}

/**
 * 段一：参数 fuzz（万级夜间批跑的进程内等价量）。
 *
 * 对 [`BloomParamBlock::resolve`] 的钳制容错断言：非法输入后输出必须
 * **有限且在域内**（threshold ∈ [MIN,MAX] 或兜底值、strength/decay ∈ [0,1]、
 * mip ∈ [1,7]），且**真的触发过钳制**（`clamped_inputs > 0`——全合法语料
 * 会让本判据恒真）。钳制失灵即立案（[`code::CLAMP_FAILED`]）。
 */
pub fn fuzz_params(seed: u64, cases: u64, ledger: &mut P0Ledger, lib: &mut CaseLibrary) -> ParamFuzzReport {
    let mut rng = Rng::new(seed);
    let mut rep = ParamFuzzReport { cases, ..Default::default() };
    for i in 0..cases {
        // 逐字段注入：本轮毒哪个字段（0..4 + tint 构造面）
        let field = (i % 5) as usize;
        let poison = poison_value(((i / 5) % 7) as usize, &mut rng);
        let mut block = BloomParamBlock {
            threshold_base: 1.0,
            strength: 0.8,
            decay: 0.8,
            mip_levels: 5,
            tint: Tint::new(1.0, 1.0, 1.0),
            exposure_link: ExposureLink::Auto,
        };
        let pre_finite = poison.is_finite();
        match field {
            0 => block.threshold_base = poison,
            1 => block.strength = poison,
            2 => block.decay = poison,
            3 => block.mip_levels = if poison.is_sign_negative() { 0u8 } else { 255u8 },
            _ => block.tint = Tint::new(poison, poison, poison),
        }
        if !pre_finite {
            rep.nan_seeds += 1;
        }
        rep.fields_poisoned[field] += 1;

        let resolved = block.resolve(true);
        let b = &resolved.block;
        // 钳制是否真的发生（非法输入产生诊断 = 钳制路径被踩过）
        if !pre_finite || !in_domain_input(field, &poison) {
            if resolved.diags.len() > 0 {
                rep.clamped_inputs += 1;
            }
        }
        // 容错断言：输出逐位回域（锚点"钳制容错"）
        let thr_ok = b.threshold_base.is_finite()
            && b.threshold_base >= super::vek04_bloom::THRESHOLD_MIN_CD_M2
            && b.threshold_base <= super::vek04_bloom::THRESHOLD_MAX_CD_M2;
        let st_ok = b.strength.is_finite() && (STRENGTH_MIN..=STRENGTH_MAX).contains(&b.strength);
        let dc_ok = b.decay.is_finite() && (0.0..=1.0).contains(&b.decay);
        let mip_ok = (1..=super::vek04_bloom::MIP_LEVELS_MAX).contains(&b.mip_levels);
        let tint_ok = b.tint.r.is_finite() && b.tint.g.is_finite() && b.tint.b.is_finite();
        let ok = thr_ok && st_ok && dc_ok && mip_ok && tint_ok
            // 有效阈值在曝光种子（含 NaN/±inf——ev_to_multiplier 对非有限
            // 回落中性 EV）下仍须有限
            // 注意喂**钳制后的块**：原始块的 NaN 阈值×倍率=NaN 是 fuzz 自伤——
            // 容错断言的对象是 resolve() 的输出面，不是输入面。
            && resolved
                .block
                .effective_threshold(resolved.exposure_link, poison_value((i % 7) as usize, &mut rng))
                .is_finite();
        if !ok {
            rep.clamp_failures += 1;
            if rep.clamp_failures <= 3 {
                // P0 级（参数打崩效果）只对**崩溃**；钳制失灵走立案（非 P0）。
                // 首个失灵案例固化入库 + 立案。
                let input_desc = format!(
                    "BloomParamBlock{{thr:{:?},str:{:?},dec:{:?},mip:{},tint:poison#{}}}",
                    block.threshold_base, block.strength, block.decay, block.mip_levels, field
                );
                lib.record(FuzzCase {
                    segment: "param",
                    name: format!("clamp-failure-field-{}", field),
                    input_desc,
                    expected_desc: "resolve() 输出有限且逐字段回域".to_string(),
                    repro: format!("fuzz_params(seed={}, cases={}) 第 {} 例", seed, cases, i),
                    hash: fnv1a64(format!("param-clamp-{}-{}", seed, i).as_bytes()),
                });
                ledger.file(
                    fnv1a64(format!("param-clamp-{}-{}", seed, i).as_bytes()),
                    "钳制失灵：容错后仍非法输出（vek05 resolve）",
                );
            }
        }
    }
    // 语料锚点案例（绿跑也入库——案例库是回归语料的固化，不是失败记录簿；
    // 失败案例只在失灵时额外入库）。逐字段 NaN 锚点各录一条。
    for (field, fname) in [
        (0usize, "threshold"),
        (1, "strength"),
        (2, "decay"),
        (3, "mip"),
        (4, "tint"),
    ] {
        let hash = fnv1a64(format!("param-anchor-{}", fname).as_bytes());
        lib.record(FuzzCase {
            segment: "param",
            name: format!("nan-anchor-{}", fname),
            input_desc: format!("{} 字段注入 NaN（其余字段合法）", fname),
            expected_desc: "resolve() 输出有限且逐字段回域；钳制诊断入袋".to_string(),
            repro: format!("fuzz_params(seed={}, cases={}) 语料锚点", seed, cases),
            hash,
        });
        let _ = field;
    }
    rep
}

/// 输入是否在域内（用于区分"合法样本"与"应触发钳制的样本"）。
fn in_domain_input(field: usize, v: &f32) -> bool {
    match field {
        0 => *v >= super::vek04_bloom::THRESHOLD_MIN_CD_M2 && *v <= super::vek04_bloom::THRESHOLD_MAX_CD_M2,
        1 => (STRENGTH_MIN..=STRENGTH_MAX).contains(v),
        2 => (0.0..=1.0).contains(v),
        3 => true, // mip 用 0/255 角点，恒非法（结构量）
        _ => false,
    }
}

// ===========================================================================
// 五、段二：DAG 配置 fuzz（全非法图形态库——校验拒绝显性验证）
// ===========================================================================

/// 非法图形态库（锚点：环构造/缺边/双生产者/自环——全形态穷举）。
pub const DAG_MALFORMATION_KINDS: usize = 7;

/// 段二报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DagFuzzReport {
    /// 总样本数。
    pub cases: u64,
    /// 非法图逃逸数（校验器放行了非法图 = 环被执行 = P0；绿跑必须为 0）。
    pub escaped: u64,
    /// 逐形态覆盖计数（每类 ≥1 才算形态库全覆盖）。
    pub kind_hits: [u64; DAG_MALFORMATION_KINDS],
    /// 干净图反证通过数（合法链必须编译通过——"全拒也绿"防线）。
    pub clean_ok: u64,
}

/// 构造一个规范效果节点。
fn mk_node(effect_id: &str, output_slot: &str, input_slots: &[&str], in_place: bool) -> EffectNode {
    EffectNode {
        effect_id: effect_id.to_string(),
        kind: format!("kind_{}", effect_id),
        stage: "post".to_string(),
        inputs: input_slots.iter().map(|s| RtRef::new(s)).collect(),
        output: RtRef::new(output_slot),
        params: ParamBlockRef {
            block_id: format!("pb_{}", effect_id),
            values: Vec::new(),
        },
        toggle: format!("t_{}", effect_id),
        in_place,
    }
}

/// 构造基础 spec（外部槽 = sceneColor）。
fn base_spec(graph: EffectGraph) -> GraphSpec {
    GraphSpec {
        graph,
        external_slots: vec!["sceneColor".to_string()],
        toggles: Vec::new(),
        in_place_allowed: Vec::new(),
    }
}

/// 干净三节点链（反证语料：必须编译通过）。
pub fn clean_chain() -> GraphSpec {
    base_spec(EffectGraph {
        nodes: vec![
            mk_node("a", "s1", &["sceneColor"], false),
            mk_node("b", "s2", &["s1"], false),
            mk_node("c", "s3", &["s2"], false),
        ],
        edges: vec![
            DagEdge { from: "a".to_string(), to: "b".to_string() },
            DagEdge { from: "b".to_string(), to: "c".to_string() },
        ],
    })
}

/// 按形态种类构造非法图（种类 0..7，穷举锚点形态库）。
pub fn malformed_graph(kind: usize) -> GraphSpec {
    match kind {
        // 0 二环：a→b→a
        0 => base_spec(EffectGraph {
            nodes: vec![
                mk_node("a", "s1", &["sceneColor"], false),
                mk_node("b", "s2", &["s1"], false),
            ],
            edges: vec![
                DagEdge { from: "a".to_string(), to: "b".to_string() },
                DagEdge { from: "b".to_string(), to: "a".to_string() },
            ],
        }),
        // 1 长环：c0→c1→c2→c0（环路径提取必须列出全部成员）
        1 => base_spec(EffectGraph {
            nodes: vec![
                mk_node("c0", "t0", &["sceneColor"], false),
                mk_node("c1", "t1", &["t0"], false),
                mk_node("c2", "t2", &["t1"], false),
            ],
            edges: vec![
                DagEdge { from: "c0".to_string(), to: "c1".to_string() },
                DagEdge { from: "c1".to_string(), to: "c2".to_string() },
                DagEdge { from: "c2".to_string(), to: "c0".to_string() },
            ],
        }),
        // 2 缺失输入：消费无人生产的槽
        2 => base_spec(EffectGraph {
            nodes: vec![mk_node("a", "s1", &["notProduced"], false)],
            edges: Vec::new(),
        }),
        // 3 双生产者：同名输出被两个效果写
        3 => base_spec(EffectGraph {
            nodes: vec![
                mk_node("a", "s1", &["sceneColor"], false),
                mk_node("b", "s1", &["sceneColor"], false),
            ],
            edges: Vec::new(),
        }),
        // 4 未登记自环：非原地效果的 a→a（真环）
        4 => base_spec(EffectGraph {
            nodes: vec![mk_node("a", "s1", &["sceneColor"], false)],
            edges: vec![DagEdge { from: "a".to_string(), to: "a".to_string() }],
        }),
        // 5 悬空边：边指向幽灵节点
        5 => base_spec(EffectGraph {
            nodes: vec![mk_node("a", "s1", &["sceneColor"], false)],
            edges: vec![DagEdge { from: "a".to_string(), to: "ghost".to_string() }],
        }),
        // 6 重复 id：两个节点同 effectId
        _ => base_spec(EffectGraph {
            nodes: vec![
                mk_node("a", "s1", &["sceneColor"], false),
                mk_node("a", "s2", &["sceneColor"], false),
            ],
            edges: Vec::new(),
        }),
    }
}

/// 段二：DAG 配置 fuzz（千级图形态批跑的进程内等价量）。
pub fn fuzz_dag(seed: u64, cases: u64, ledger: &mut P0Ledger, lib: &mut CaseLibrary) -> DagFuzzReport {
    let mut rep = DagFuzzReport { cases, ..Default::default() };
    for i in 0..cases {
        let kind = (i as usize) % DAG_MALFORMATION_KINDS;
        let spec = malformed_graph(kind);
        rep.kind_hits[kind] += 1;
        // 校验与编译两层都必须拒绝（compile_graph 内部先 validate_graph）
        let v = validate_graph(&spec);
        let c = compile_graph(&spec, &CompileOptions { version: 1, rt_spec_override: Vec::new(), init: None });
        let mut ok_shape = true;
        if let Err(e) = v {
            // 报错三要素非空（锚点：全部非法图形态被拦截且报错三要素）
            if e.message.is_empty() || e.hint.is_empty() {
                ok_shape = false;
            }
        } else {
            ok_shape = false;
        }
        if c.is_ok() {
            ok_shape = false;
        }
        if !ok_shape {
            rep.escaped += 1;
            if rep.escaped <= 3 {
                // 非法图未拦截（环被执行）= P0（校验器失守——执行正确性根基）
                let hash = fnv1a64(format!("dag-escape-{}-{}", seed, i).as_bytes());
                lib.record(FuzzCase {
                    segment: "dag",
                    name: format!("malformed-escape-kind-{}", kind),
                    input_desc: format!("malformed_graph(kind={})", kind),
                    expected_desc: "validate/compile 双层拒绝且三要素非空".to_string(),
                    repro: format!("fuzz_dag(seed={}, cases={}) 第 {} 例", seed, cases, i),
                    hash,
                });
                ledger.file(hash, "非法图被校验器放行（环被执行）——F2002 校验器失守");
            }
        }
    }
    // 语料锚点案例：七类形态各录一条（kind 名固化，回归时按名重放）。
    for (kind, kname) in [
        (0usize, "cycle-2"),
        (1, "cycle-long"),
        (2, "missing-producer"),
        (3, "dual-producer"),
        (4, "self-loop-undeclared"),
        (5, "dangling-edge"),
        (6, "duplicate-id"),
    ] {
        let hash = fnv1a64(format!("dag-anchor-{}", kname).as_bytes());
        lib.record(FuzzCase {
            segment: "dag",
            name: format!("malformed-anchor-{}", kname),
            input_desc: format!("malformed_graph(kind={}) —— {}", kind, kname),
            expected_desc: "validate/compile 双层拒绝且三要素非空".to_string(),
            repro: format!("fuzz_dag(seed={}, cases={}) 语料锚点", seed, cases),
            hash,
        });
    }
    // 干净图反证（每轮语料附带，防"全拒也绿"）
    let clean = compile_graph(
        &clean_chain(),
        &CompileOptions { version: 1, rt_spec_override: Vec::new(), init: None },
    );
    if clean.is_ok() {
        rep.clean_ok = 1;
    } else {
        // 干净图被拒 = 校验器过严也是失守（另一向的根基破坏）
        let hash = fnv1a64(format!("dag-clean-reject-{}", seed).as_bytes());
        ledger.file(hash, "干净链图被校验器拒绝（全拒也绿的反向失守）");
    }
    rep
}

// ===========================================================================
// 六、段三：RT 池压力 fuzz（F2003 防线的对抗验证）
// ===========================================================================

/// 段三报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PoolFuzzReport {
    /// 总申请次数（含被拒）。
    pub allocs: u64,
    /// 被拒绝的申请数。
    pub rejected: u64,
    /// 防线绕过数（超限仍分配 = P0；绿跑必须为 0）。
    pub bypassed: u64,
    /// 泄漏压测：疑似泄漏句柄数。
    pub leak_suspected: u64,
    /// 泄漏压测：帧数。
    pub leak_frames: u64,
    /// 拒绝原因分型（QuotaExceeded 计数——风暴拒绝必须全部落在配额码上）。
    pub quota_rejects: u64,
    /// 峰值在用数（不得破配额上限）。
    pub peak_live: u64,
    /// 字节水位触发数（字节路径单独被试探的证据）。
    pub bytes_watermark_hits: u64,
}

/// 风暴/泄漏共用配额（张数小、字节紧——两条水位都够得着）。
fn stress_quota() -> Quota {
    Quota { max_entries: 8, max_bytes: 8 * 1920 * 1080 * 8, reject_ratio_permille: 950 }
}

/// 段三a：超配额申请风暴。
pub fn fuzz_pool_storm(seed: u64, allocs: u64, ledger: &mut P0Ledger, lib: &mut CaseLibrary) -> PoolFuzzReport {
    let mut rng = Rng::new(seed);
    let mut pool = RtTargetPool::new(stress_quota(), 3);
    let mut rep = PoolFuzzReport::default();
    let quota_entries = stress_quota().max_entries as u64;
    for i in 0..allocs {
        // 尺寸扰动小（多数落同桶制造复用窗口），格式两态
        let w = 64 + rng.below(4) as u32;
        let spec = RtSpec {
            width: w,
            height: 64,
            format: if i % 3 == 0 { "RGBA8".to_string() } else { "RGBA16F".to_string() },
        };
        rep.allocs += 1;
        match pool.allocate(&format!("s{}", i % 32), &spec) {
            Ok(_) => {
                // 防线绕过判定：成功后 live 不得超过配额上限
                let live = pool.stats().live_count as u64;
                if live > quota_entries {
                    rep.bypassed += 1;
                    if rep.bypassed <= 3 {
                        let hash = fnv1a64(format!("pool-bypass-{}-{}", seed, i).as_bytes());
                        lib.record(FuzzCase {
                            segment: "pool",
                            name: "quota-bypass".to_string(),
                            input_desc: format!("第 {} 次申请成功后 live={} > 配额 {}", i, live, quota_entries),
                            expected_desc: "超限申请必须被拒".to_string(),
                            repro: format!("fuzz_pool_storm(seed={}, allocs={})", seed, allocs),
                            hash,
                        });
                        ledger.file(hash, "池防线绕过：超限仍分配（F2003 配额失守）");
                    }
                }
            }
            Err(d) => {
                rep.rejected += 1;
                if d.code == PoolDiagCode::QuotaExceeded {
                    rep.quota_rejects += 1;
                }
            }
        }
    }
    rep.peak_live = pool.stats().peak_live as u64;
    lib.record(FuzzCase {
        segment: "pool",
        name: "quota-storm-anchor".to_string(),
        input_desc: format!("{} 次申请（配额 {} 张/水位 {}‰）", allocs, stress_quota().max_entries, stress_quota().reject_ratio_permille),
        expected_desc: format!("成功数不破水位、拒绝全部 QuotaExceeded"),
        repro: format!("fuzz_pool_storm(seed={}, allocs={})", seed, allocs),
        hash: fnv1a64(b"pool-anchor-quota-storm"),
    });
    rep
}

/// 段三b：泄漏式申请压测（万次申请不回收）。
pub fn fuzz_pool_leak(seed: u64, allocs: u64, frames: u64, ledger: &mut P0Ledger, lib: &mut CaseLibrary) -> PoolFuzzReport {
    let mut pool = RtTargetPool::new(stress_quota(), 3);
    let mut rep = PoolFuzzReport::default();
    rep.leak_frames = frames;
    let spec = RtSpec { width: 1920, height: 1080, format: "RGBA16F".to_string() };
    // 万次申请（绝大多数被配额拒绝——这本身就是防线工作量的压测）
    for i in 0..allocs {
        rep.allocs += 1;
        match pool.allocate(&format!("leak{}", i), &spec) {
            Ok(_) => {}
            Err(d) => {
                rep.rejected += 1;
                if d.code == PoolDiagCode::QuotaExceeded {
                    rep.quota_rejects += 1;
                }
            }
        }
    }
    // 不回收地跑 N 帧：泄漏账本必须逐帧在账，达阈后全体疑似
    for _ in 0..frames {
        let report = pool.begin_frame(&[]);
        rep.leak_suspected = rep.leak_suspected.max(report.leaks.len() as u64);
    }
    let stats = pool.stats();
    rep.peak_live = stats.peak_live as u64;
    // 防线绕过判定：峰值在用不得破配额；账本不得为空（全在泄漏）
    let quota_entries = stress_quota().max_entries as u64;
    if rep.peak_live > quota_entries || pool.leak_ledger().is_empty() {
        rep.bypassed += 1;
        let hash = fnv1a64(format!("pool-leak-bypass-{}", seed).as_bytes());
        lib.record(FuzzCase {
            segment: "pool",
            name: "leak-defense-bypass".to_string(),
            input_desc: format!("万次申请不回收：peak={} quota={} 账本空={}", rep.peak_live, quota_entries, pool.leak_ledger().is_empty()),
            expected_desc: "峰值不破配额且泄漏账本全量在账".to_string(),
            repro: format!("fuzz_pool_leak(seed={}, allocs={}, frames={})", seed, allocs, frames),
            hash,
        });
        ledger.file(hash, "泄漏防线失守（峰值破配额或账本漏记）");
    }
    lib.record(FuzzCase {
        segment: "pool",
        name: "leak-stress-anchor".to_string(),
        input_desc: format!("{} 次申请不回收 × {} 帧不归还", allocs, frames),
        expected_desc: "峰值不破水位且泄漏账本全体疑似".to_string(),
        repro: format!("fuzz_pool_leak(seed={}, allocs={}, frames={})", seed, allocs, frames),
        hash: fnv1a64(b"pool-anchor-leak-stress"),
    });
    rep
}

/// 段三c：别名区间重叠注入（贪心着色的正确性——重叠绝不入同组）。
pub fn fuzz_alias_overlap(cases: u64, ledger: &mut P0Ledger, lib: &mut CaseLibrary) -> u64 {
    let mut escaped = 0u64;
    for i in 0..cases {
        // 构造两两重叠的区间（全落在同一步附近）——若入同组即读写竞争
        let b = (i % 16) as u32;
        let ivs = vec![
            LiveInterval { slot: format!("ov{}a", i), birth: b, death: b + 8 },
            LiveInterval { slot: format!("ov{}b", i), birth: b + 1, death: b + 9 },
            LiveInterval { slot: format!("ov{}c", i), birth: b + 2, death: b + 10 },
        ];
        let a = analyze_aliases(&ivs);
        // 三个两两重叠的区间必须分进三个组（物理数=逻辑数，saved=0）
        if a.physical_count != 3 || a.saved_count != 0 {
            escaped += 1;
            if escaped <= 3 {
                let hash = fnv1a64(format!("alias-overlap-{}", i).as_bytes());
                lib.record(FuzzCase {
                    segment: "pool",
                    name: "alias-overlap-merged".to_string(),
                    input_desc: format!("三个重叠区间 physical={} saved={}", a.physical_count, a.saved_count),
                    expected_desc: "重叠区间绝不入同组".to_string(),
                    repro: format!("fuzz_alias_overlap(cases={}) 第 {} 例", cases, i),
                    hash,
                });
                ledger.file(hash, "别名分析误判可别名（读写竞争——画面闪）");
            }
        }
        // 人造错组：把重叠区间硬塞同组，verify 必须抓出 ALIAS_CONFLICT
        let mut fake = a.clone();
        if let Some(g) = fake.groups.first_mut() {
            g.members = ivs.iter().map(|iv| iv.slot.clone()).collect();
        }
        let conflicts = verify_alias_groups(&fake, &ivs);
        if conflicts.is_empty() {
            escaped += 1;
        }
    }
    escaped
}

// ===========================================================================
// 七、字节水位路径（张数水位之外的另一半防线）
// ===========================================================================

/// 字节水位单独试探：大规格少张数，只撞字节不撞张数。
pub fn probe_bytes_watermark(ledger: &mut P0Ledger, lib: &mut CaseLibrary) -> u64 {
    // max_bytes 只够 1 张 1920×1080 RGBA16F（8 字节/像素）的 60%
    let quota = Quota { max_entries: 64, max_bytes: 1920 * 1080 * 8 / 2, reject_ratio_permille: 500 };
    let mut pool = RtTargetPool::new(quota, 3);
    let big = RtSpec { width: 1920, height: 1080, format: "RGBA16F".to_string() };
    let mut hits = 0u64;
    // 第一张即过 500‰ 水位（1 张 = 8MB > 4MB），其后全部拒绝
    for i in 0..4 {
        match pool.allocate(&format!("bw{}", i), &big) {
            Err(d) => {
                if d.code == PoolDiagCode::QuotaExceeded {
                    hits += 1;
                }
            }
            Ok(_) => {}
        }
    }
    if hits == 0 {
        // 字节水位从未触发 = 半条防线失守
        let hash = fnv1a64(b"bytes-watermark-never-hit");
        lib.record(FuzzCase {
            segment: "pool",
            name: "bytes-watermark-dead".to_string(),
            input_desc: "大规格申请未触发字节水位".to_string(),
            expected_desc: "字节因子按或独立触发拒绝".to_string(),
            repro: "probe_bytes_watermark()".to_string(),
            hash,
        });
        ledger.file(hash, "字节水位路径失效（张数之外的半个配额防线）");
    }
    hits
}

// ===========================================================================
// 八、总控（通过率报告——供 F2038/F2059）
// ===========================================================================

/// 全量 fuzz 报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FuzzReport {
    /// 段一。
    pub param: ParamFuzzReport,
    /// 段二。
    pub dag: DagFuzzReport,
    /// 段三a 风暴。
    pub storm: PoolFuzzReport,
    /// 段三b 泄漏。
    pub leak: PoolFuzzReport,
    /// 段三c 别名逃逸数。
    pub alias_escaped: u64,
    /// 字节水位触发数。
    pub bytes_hits: u64,
    /// 案例库条数。
    pub cases: usize,
    /// 去重跳过数。
    pub dups: u64,
    /// P0/立案总数（绿跑必须为 0）。
    pub filings: usize,
}

/// 通过率（供 F2038/F2059；passed/total 双计数独立重算）。
pub fn pass_rate(rep: &FuzzReport) -> (u64, u64) {
    let total = rep.param.cases + rep.dag.cases + rep.storm.allocs + rep.leak.allocs;
    let failed = rep.param.clamp_failures
        + rep.dag.escaped
        + rep.storm.bypassed
        + rep.leak.bypassed
        + rep.alias_escaped;
    (total.saturating_sub(failed), total)
}

/// 全量 fuzz（确定性语料：同种子重放逐位一致——复现脚本的根基）。
pub fn run_all() -> (FuzzReport, CaseLibrary, P0Ledger) {
    let mut lib = CaseLibrary::new();
    let mut ledger = P0Ledger::new();
    let param = fuzz_params(0xBEEF, 10_000, &mut ledger, &mut lib);
    let dag = fuzz_dag(0xCAFE, 1_000, &mut ledger, &mut lib);
    let storm = fuzz_pool_storm(0xFACE, 2_000, &mut ledger, &mut lib);
    let leak = fuzz_pool_leak(0xFEED, 10_000, 40, &mut ledger, &mut lib);
    let alias_escaped = fuzz_alias_overlap(256, &mut ledger, &mut lib);
    let bytes_hits = probe_bytes_watermark(&mut ledger, &mut lib);
    let rep = FuzzReport {
        param,
        dag,
        storm,
        leak,
        alias_escaped,
        bytes_hits,
        cases: lib.len(),
        dups: lib.dups,
        filings: ledger.len(),
    };
    (rep, lib, ledger)
}

// ===========================================================================
// 九、域自检（判据逐条映射锚点）
// ===========================================================================

/// VE-F2016 域自检（判据逐条映射锚点；随模块常驻编译，供注册表聚合器调用）。
pub mod checks {
    use super::*;
    use crate::checks::CheckSet;

    /// 跑全部判据。
    pub fn run_vek16_checks() -> CheckSet {
        let mut s = CheckSet::new("svstar2-vek16");

        // 全量语料一次跑，各判据共用（跑两遍浪费时间且第一遍已经验确定性）。
        let (rep, lib, ledger) = run_all();

        // -- P16-A-01 参数万级扫描：零钳制失灵 + 钳制确实发生 ----------------
        {
            s.add(
                "P16-A-01 参数万级 fuzz：10_000 例零钳制失灵、非法样本真触发过钳制",
                rep.param.cases == 10_000
                    && rep.param.clamp_failures == 0
                    && rep.param.clamped_inputs > 0,
                "锚点「钳制容错断言」；clamped_inputs=0 说明语料全合法（恒真防线）",
            );
        }

        // -- P16-A-02 逐字段毒化覆盖 + NaN 种子注入 ---------------------------
        {
            let covered = rep.param.fields_poisoned.iter().all(|&c| c > 0);
            s.add(
                "P16-A-02 参数面逐字段毒化全覆盖（threshold/strength/decay/mip/tint）且 NaN 种子≥千",
                covered && rep.param.nan_seeds >= 1_000,
                "锚点「NaN/越界全参数空间」「NaN 种子注入」；缺一个字段就是一条无防线的参数面",
            );
        }

        // -- P16-A-03 Tint 构造面钳制（非有限退回不染色）----------------------
        {
            let t = Tint::new(f32::NAN, f32::INFINITY, f32::NEG_INFINITY);
            s.add(
                "P16-A-03 Tint 构造面钳制：NaN/±inf 全部退回 1.0（不染色安全侧）",
                (t.r - 1.0).abs() < f32::EPSILON
                    && (t.g - 1.0).abs() < f32::EPSILON
                    && (t.b - 1.0).abs() < f32::EPSILON,
                "安全侧不是染成黑——退回单位色，画面只剩未染色泛光而非黑块",
            );
        }

        // -- P16-B-01 全非法图形态拦截：七类穷举、零逃逸、三要素齐 ------------
        {
            let kinds_covered = rep.dag.kind_hits.iter().all(|&c| c > 0)
                && rep.dag.kind_hits.iter().sum::<u64>() == rep.dag.cases;
            s.add(
                "P16-B-01 DAG 千级 fuzz：七类非法形态全覆盖、零逃逸、双层拒绝",
                rep.dag.cases == 1_000
                    && kinds_covered
                    && rep.dag.escaped == 0
                    && rep.dag.clean_ok == 1,
                "锚点「全部非法图形态被拦截且报错三要素」；clean_ok=1 防全拒也绿",
            );
        }

        // -- P16-B-02 干净图三要素抽检（拒绝报错也必须有三要素）---------------
        {
            // 合法图编译产物本身 diagnostics 可为空（无告警即无诊断）——
            // 三要素检查打在拒绝路径：取一个非法图看错误结构。
            let e = validate_graph(&malformed_graph(0));
            let three = match e {
                Err(err) => {
                    !err.message.is_empty() && !err.hint.is_empty()
                        && err.diagnostics.iter().all(|d| !d.message.is_empty() && !d.hint.is_empty())
                }
                Ok(_) => false,
            };
            s.add(
                "P16-B-02 拒绝报错三要素齐备（码/描述/建议，含袋内逐条）",
                three,
                "锚点「报错三要素」；裸码无描述的报错把定位成本甩给下游",
            );
        }

        // -- P16-B-03 语料确定性（同种子重放逐位一致——复现根基）--------------
        {
            let mut ledger2 = P0Ledger::new();
            let mut lib2 = CaseLibrary::new();
            let rep2 = fuzz_dag(0xCAFE, 1_000, &mut ledger2, &mut lib2);
            s.add(
                "P16-B-03 同种子重放：DAG 语料报告逐位一致（确定性生成器）",
                rep2 == rep.dag,
                "fuzz 不可重放=复现脚本是一句空话（F1813 框架同构要求）",
            );
        }

        // -- P16-C-01 风暴防线：成功恰在水位内、拒绝全落配额码、零绕过 --------
        {
            // 拒绝水位是 floor(max_entries×permille/1000)——判据用**同一公式**
            // 独立重算，不引用被测的 reached_reject_watermark（自证式防线）。
            let q = stress_quota();
            let watermark = (q.max_entries as u64) * (q.reject_ratio_permille as u64) / 1000;
            s.add(
                "P16-C-01 超配额风暴：峰值不破水位、拒绝全部 QuotaExceeded、零绕过",
                rep.storm.allocs == 2_000
                    && rep.storm.bypassed == 0
                    && rep.storm.peak_live <= watermark
                    && rep.storm.quota_rejects == rep.storm.rejected
                    && rep.storm.rejected > 0,
                "锚点「超配额申请——F2003 防线的对抗验证」；拒绝码不齐=有拒绝走了别的路",
            );
        }

        // -- P16-C-02 泄漏压测：万次申请不回收、账本全量在账 ------------------
        {
            let q = stress_quota();
            let watermark = (q.max_entries as u64) * (q.reject_ratio_permille as u64) / 1000;
            s.add(
                "P16-C-02 泄漏压测：万次申请零 panic、泄漏账本全体疑似、峰值恰水位",
                rep.leak.allocs == 10_000
                    && rep.leak.bypassed == 0
                    && rep.leak.peak_live == watermark
                    && rep.leak.leak_suspected == watermark
                    && rep.leak.quota_rejects == rep.leak.allocs - watermark,
                "锚点「万次申请不回收——泄漏防线压测」；账本漏记=泄漏静默（最终池耗尽）",
            );
        }

        // -- P16-C-03 字节水位路径单独触发 ------------------------------------
        {
            s.add(
                "P16-C-03 字节水位路径：大规格少张数只撞字节因子，拒绝正常触发",
                rep.bytes_hits >= 3,
                "只试张数水位的防线是半条防线（F1776 双因子口径）",
            );
        }

        // -- P16-C-04 别名重叠注入：重叠绝不入同组 + 人造错组必被抓 -----------
        {
            s.add(
                "P16-C-04 别名重叠注入：256 例重叠区间全部分组正确、verify 抓出人造错组",
                rep.alias_escaped == 0,
                "锚点「别名冲突注入」；重叠入同组=读写竞争=画面闪（正确性优先于最优性）",
            );
        }

        // -- P16-CASE-01 案例库三段齐、元数据四件全 ---------------------------
        {
            let meta_ok = lib.all().iter().all(|c| {
                !c.input_desc.is_empty() && !c.expected_desc.is_empty() && !c.repro.is_empty() && c.hash != 0
            });
            s.add(
                "P16-CASE-01 案例库固化：三段各有条目、元数据（输入/期望/复现/哈希）四件全",
                lib.segment_len("param") > 0
                    && lib.segment_len("dag") > 0
                    && lib.segment_len("pool") > 0
                    && meta_ok,
                "锚点「案例固化入库」+「案例元数据（输入/期望/复现脚本/去重哈希）」",
            );
        }

        // -- P16-CASE-02 去重哈希管理：同例二入库不增长 ------------------------
        {
            let mut lib2 = CaseLibrary::new();
            let c = FuzzCase {
                segment: "param",
                name: "dup-probe".to_string(),
                input_desc: "x".to_string(),
                expected_desc: "y".to_string(),
                repro: "z".to_string(),
                hash: fnv1a64(b"dup-probe"),
            };
            let first = lib2.record(c.clone());
            let second = lib2.record(c);
            s.add(
                "P16-CASE-02 去重：同哈希二次入库被跳过且计数（库不增长）",
                first && !second && lib2.len() == 1 && lib2.dups == 1 && lib.dups == 0,
                "锚点「案例去重哈希管理」；绿跑语料全唯一（dups==0 也是对账项）",
            );
        }

        // -- P16-P0-01 绿语料零立案 + 立案 API 真调 24h 时限 -------------------
        {
            let mut l = P0Ledger::new();
            let f = l.file(fnv1a64(b"p0-probe"), "哨兵立案（API 真调）");
            s.add(
                "P16-P0-01 绿语料零立案；立案 API 真调时限恰 24h（锚点明示）",
                rep.filings == 0
                    && f.deadline_hours == 24
                    && l.len() == 1
                    && ledger.is_empty(),
                "立案是异常出口不是常规流程；时限放参数会被「宽限一次」磨掉",
            );
        }

        // -- P16-REG-01 通过率报告：三段计数与常量一致、独立重算全通过 ---------
        {
            let (passed, total) = pass_rate(&rep);
            s.add(
                "P16-REG-01 通过率报告（供 F2038/F2059）：total 与三段常量一致、passed==total",
                total == 10_000 + 1_000 + 2_000 + 10_000 && passed == total,
                "锚点「通过率供 F2038/F2059」；报告计数必须与语料常量对账（不能自说自话）",
            );
        }

        // -- P16-REUSE-01 迁移与复用声明：被测对象是 Rust 单源 -----------------
        {
            // 类型可引用即迁移落地（TS 原件被替代后删除——收尾动作，见任务报告）
            let dag_live = validate_graph(&clean_chain()).is_ok();
            let mut pool = RtTargetPool::new(
                Quota { max_entries: 4, max_bytes: 1 << 30, reject_ratio_permille: 950 },
                3,
            );
            let spec = RtSpec { width: 64, height: 64, format: "RGBA8".to_string() };
            let pool_live = pool.allocate("r", &spec).is_ok();
            let param_live = BloomParamBlock::default().resolve(true).block.mip_levels == 5;
            s.add(
                "P16-REUSE-01 被测三面皆 Rust 单源（F2002 迁移/F2003 迁移/F2005 本体）且可真调",
                dag_live && pool_live && param_live,
                "迁移声明：f2002/f2003 两个 TS 文件被本仓 Rust 模块替代（本任务收尾删除）",
            );
        }

        // -- P16-QUIET-01 噪声纪律：干净语料不制造诊断（有译零回退同构）--------
        {
            // 干净链图编译 diagnostics 必须为空（有告警=噪声；噪声淹没真告警）
            let plan = compile_graph(
                &clean_chain(),
                &CompileOptions { version: 1, rt_spec_override: Vec::new(), init: None },
            );
            let quiet = match plan {
                Ok(p) => p.diagnostics.is_empty() && p.bypassed.is_empty(),
                Err(_) => false,
            };
            // 干净池路径零诊断
            let mut pool = RtTargetPool::new(
                Quota { max_entries: 4, max_bytes: 1 << 30, reject_ratio_permille: 950 },
                3,
            );
            let spec = RtSpec { width: 64, height: 64, format: "RGBA8".to_string() };
            let _ = pool.allocate("q", &spec);
            let pool_quiet = pool.diagnostics().is_empty();
            s.add(
                "P16-QUIET-01 干净语料零诊断零旁路（fuzz 报错不撒谎的对偶面：不报错也不制造噪声）",
                quiet && pool_quiet,
                "噪声淹没真告警=防线退化；通过率统计里的假阳会让 F2038/F2059 失真",
            );
        }

        s
    }
}
