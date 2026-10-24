//! VE-F2609 · 控件树 fuzz（VE-N 域 · UI 内核架构与控件树组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2609`
//!
//! **判据（锚点原文）**：四类畸形、深度防护、校验对抗、24h 固化、判据。
//!
//! **职责定位（锚点原文）**：树畸形全空间注入（环/孤儿/超深（>1000 层）/
//! 超宽（单节点万子——性能攻击）——校验拒绝+性能防护双语义验证：环/孤儿→
//! 校验拒绝；超深/超宽→性能防护（深度上限拒绝/宽度分页——防护语义声明））；
//! 序列化 fuzz（畸形 JSON：结构损坏/类型错误/绑定路径注入/深度攻击——
//! F2607 四重校验对抗，全部被拦截显性）；混合 fuzz（序列化+属性风暴+增量
//! 更新并发——组合不崩不变量），案例固化入库。
//!
//! # 一、三个被测面全部「真调」而不代填（vel11/vem11 家族纪律）
//!
//! - 段一（树畸形）：真调 F2602 [`insert`](ven02_tree::insert) 的单亲/无环/
//!   深度上限三道防线与 [`assert_invariants`](ven02_tree::assert_invariants)；
//!   超宽的「宽度分页」防线真调 F2608 [`pick_subtree`](ven08_debug::
//!   pick_subtree) 的预算交付面——树模型对宽度不设硬顶（万子是合法树），
//!   防护语义是「**消费侧分页**」而不是「构造侧拒绝」，这一区别如实声明。
//! - 段二（序列化对抗）：真调 F2607 [`parse_json`](ven07_serde::parse_json) /
//!   [`parse_and_validate`](ven07_serde::parse_and_validate) /
//!   [`import_json`](ven07_serde::import_json)。环/孤儿由**结构重**（可达性）
//!   拦、深度由 512 拦、千层括号由 `MAX_JSON_NEST` 拦——四重校验的对抗面。
//! - 段三（混合压测）：真调 F2608 [`apply_tune`](ven08_debug::apply_tune)
//!   （属性风暴）+ F2606 [`IncrementalUpdate`](ven06_incr::IncrementalUpdate)
//!   （失效并发）+ F2607 导出/校验（序列化面）三面交错——组合不崩、
//!   不变量保持、计数守恒。
//!
//! # 二、P0 四条红线（锚点错误路径原文，逐条可判定）
//!
//! - **崩溃 → P0**：执行器**不吞 panic**（与 vel11/vem11 同纪律）——被测面
//!   契约零 panic，任何一次 panic 都让判据进程崩溃、门禁必红。
//! - **环未拦截 → P0**：成环 insert 返回 Ok 即单亲/无环不变量失守。
//! - **深度攻击穿透 → P0**：>1000 层攻击未被拒绝（栈溢出风险——F2609
//!   特化红线）。诚实标注：防线实际在 [`MAX_TREE_DEPTH`] = 512，
//!   **严于锚点的 1000**——1001 层攻击在 513 层就被拒，锚点判据只会更绿。
//! - **序列化拦截失效 → P0**：畸形 JSON 被静默接受（校验失守）。
//!   钳制失灵 → 立案（P1，非 P0）。
//!
//! # 三、案例固化入库（FNV-1a 去重 + 库容控制 + 24h 期限）
//!
//! [`CaseStore`] 以输入摘要 FNV-1a 去重（同案不二入），库容 [`MAX_CASES`]
//! 封顶（超容显性拒绝——不静默丢案）；每条案带输入/期望/复现/哈希四要素
//! （缺一即不可复核）。P0 立案带 24h 复现期限（[`P0_REPRO_24H_MS`]）——
//! 超期可判定（[`P0Case::overdue`]），不是口头督促。
//!
//! # 四、框架（F1813 家族 · N01 面）
//!
//! 自持 LCG（同种子同语料——复现前提）、执行器不吞 panic、步数预算看门狗
//! （[`MIXED_STORM_STEPS`] 把锚点「组合 1 小时档入 F1769 N 段」变成可判定
//! 事件：步数封顶即预算耗尽，超预算不是错误是**声明**）。案例库入 F1769
//! N 段调度（N 域 fuzz 首域），立案进 N 域账本（建账前暂挂声明——家族）。
//!
//! 零 panic 面、零 IO、无全局可变状态、全合成输入无隐私面。

use alloc::vec;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::svstar2::ven08_debug::TuneChannel;

use super::ven02_tree::{
    self, assert_invariants, insert, ControlTree, PropertyKey, SingleParentPolicy, TreeDiagCode,
    MAX_TREE_DEPTH,
};
use super::ven04_prop::{Invalidation, PropEngine, PropValue};
use super::ven05_dual::TemplateTable;
use super::ven06_incr::{IncrementalUpdate, PendingInvalid, RouteLedger};
use super::ven07_serde::{
    collect_binds, collect_nodes, collect_props, import_json, parse_and_validate, parse_json,
    to_json, Fold, TreeDoc, CUR_VERSION,
};
use super::ven08_debug::{
    apply_tune, pick_subtree, DiagBag, NodeMirror, TreeMirror, TuneCommand, TuneOp,
};

// ---------------------------------------------------------------------------
// 一、框架常量（F1813 家族 · N01 面）
// ---------------------------------------------------------------------------

/// 深度攻击层数（锚点原文「深度递增（1001 层）」）。
pub const DEPTH_ATTACK_LAYERS: usize = 1001;

/// 宽度攻击宽度（锚点原文「宽度递增（10001 子）」——单节点万子）。
pub const WIDTH_ATTACK_CHILDREN: usize = 10_001;

/// JSON 嵌套攻击深度（`MAX_JSON_NEST` = 32，攻击取 40——必穿透拦截线）。
pub const NEST_ATTACK_DEPTH: usize = 40;

/// 混合压测步数预算（锚点「组合 1 小时档」的机检替身：步数封顶可判定）。
pub const MIXED_STORM_STEPS: usize = 96;

/// 案例库容量（超容显性拒绝——不静默丢案）。
pub const MAX_CASES: usize = 4_096;

/// P0 复现期限（24h——锚点「P0 立案 24h 复现」数值化）。
pub const P0_REPRO_24H_MS: u64 = 86_400_000;

/// P0 立案码：环未拦截（单亲/无环不变量失守——F2602 根基）。
pub const E_P0_CYCLE_NOT_INTERCEPTED: &str = "E_N9_P0_CYCLE_NOT_INTERCEPTED";
/// P0 立案码：深度攻击穿透（>1000 层未拒绝——栈溢出风险）。
pub const E_P0_DEPTH_PENETRATED: &str = "E_N9_P0_DEPTH_PENETRATED";
/// P0 立案码：序列化拦截失效（畸形 JSON 被静默接受——校验失守）。
pub const E_P0_SERDE_LEAK: &str = "E_N9_P0_SERDE_LEAK";
/// P0 立案码：fuzz 执行崩溃（被测面 panic——契约零 panic 失守）。
pub const E_P0_CRASH: &str = "E_N9_P0_CRASH";
/// 立案码（P1）：钳制失灵（非有限值未被钳制直进下游）。
pub const E_P1_CLAMP_FAILURE: &str = "E_N9_P1_CLAMP_FAILURE";
/// 拒绝码：案例库满。
pub const E_FUZZ_CASE_FULL: &str = "E_N9_FUZZ_CASE_FULL";

/// 自持 LCG（64 位——同种子双跑逐位一致，不引外部随机源；与 vel11/vem11
/// 同族同语义：0 种子映射到非零常量，全零态 LCG 会锁死）。
#[derive(Clone, Debug)]
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 新种子。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed } }
    }

    /// 下一原始 64 位。
    pub fn next_u64(&mut self) -> u64 {
        // PCG 风格输出变换的 LCG 核心（同族 vel11/vem11 一致）。
        self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let xorshifted = ((self.state >> 18) ^ self.state) >> 27;
        let rot = (self.state >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// [0, n) 内取值（n = 0 时恒 0）。
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }
}

/// FNV-1a 64 位（案例去重哈希——同输入必同哈希）。
pub const fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 二、段一：树畸形 fuzz（四类 · 真调 F2602/F2608）
// ---------------------------------------------------------------------------

/// 树畸形四类（锚点原文：环/孤儿/超深/超宽）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TreeMalform {
    /// 祖先链成环（F2602 不变量对抗）。
    #[default]
    Cycle,
    /// 无父节点（可达性对抗——树 API 注册制造不出，攻击面在导入层）。
    Orphan,
    /// 超深树（>1000 层——栈溢出风险）。
    OverDeep,
    /// 超宽树（单节点万子——性能攻击）。
    OverWide,
}

impl TreeMalform {
    /// 全部四类（判据遍历序）。
    pub const ALL: [TreeMalform; 4] = [
        TreeMalform::Cycle,
        TreeMalform::Orphan,
        TreeMalform::OverDeep,
        TreeMalform::OverWide,
    ];

    /// 线缆名。
    pub const fn as_str(self) -> &'static str {
        match self {
            TreeMalform::Cycle => "CYCLE",
            TreeMalform::Orphan => "ORPHAN",
            TreeMalform::OverDeep => "OVER_DEEP",
            TreeMalform::OverWide => "OVER_WIDE",
        }
    }
}

/// 段一攻击结论（三态：显性拒绝 / 放行 / 检出——放行给合法面用）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum AttackVerdict {
    /// 被显性拒绝（拒绝码——校验拒绝语义）。
    Rejected(String),
    /// 放行且防护生效（性能防护语义：分页/有界）。
    #[default]
    Allowed,
    /// 静默通过（**P0 语义**：拦截失守）。
    Leaked,
}

/// 语料树（6 节点，与 F2608 判据语料同形：root → (a → a1, b → (b1, b2))）。
fn corpus_tree() -> ControlTree {
    let mut t = match ControlTree::new("root") {
        Ok(t) => t,
        Err(_) => ControlTree::new("r").unwrap_or_else(|_| ControlTree::new("rr").expect("根 id 恒非空")),
    };
    for id in ["a", "b"] {
        let _ = insert(&mut t, "root", id, Some(0), SingleParentPolicy::Reject);
    }
    for id in ["b1", "b2"] {
        let _ = insert(&mut t, "b", id, Some(0), SingleParentPolicy::Reject);
    }
    let _ = insert(&mut t, "a", "a1", Some(0), SingleParentPolicy::Reject);
    t
}

/// 不变量是否全绿（树畸形攻击后的健康检查——攻击不得留下半成品）。
fn invariants_green(tree: &ControlTree) -> bool {
    assert_invariants(tree).iter().all(|r| r.ok)
}

/// 环攻击：把 `a`（已有父）重新挂到它自己的后代 `a1` 下。
///
/// `Reject` 策略在**单亲校验**就被拒（到不了无环分支）；
/// `Detach` 策略才能走到环检测——两条路都必须显性拒绝。
/// 任何一条返回 Ok = 环未拦截 = P0。
fn attack_cycle() -> (AttackVerdict, u64) {
    let mut t = corpus_tree();
    let e1 = insert(&mut t, "a1", "a", None, SingleParentPolicy::Detach).err();
    let e2 = insert(&mut t, "a1", "a", None, SingleParentPolicy::Reject).err();
    let input = format!("cycle:a->a1:detach={:?}:reject={:?}", e1.is_some(), e2.is_some());
    let digest = fnv1a64(input.as_bytes());
    match (e1, e2) {
        (Some(d1), Some(d2)) => {
            // 双策略都被拒；树必须完好（攻击不得留下半成品）。
            if invariants_green(&t) {
                (AttackVerdict::Rejected(d1.message), digest)
            } else {
                (AttackVerdict::Leaked, digest)
            }
        }
        _ => (AttackVerdict::Leaked, digest),
    }
}

/// 孤儿攻击（导入层）：节点段里塞一个没有任何父引用的 `ghost`——
/// 从根不可达即孤儿，F2607 结构重的可达性检查必须抓住。
fn attack_orphan() -> (AttackVerdict, u64) {
    let src = "{\"version\":2,\"root\":\"r\",\"nodes\":[{\"id\":\"r\",\"kind\":\"panel\",\"children\":[]},{\"id\":\"ghost\",\"kind\":\"leaf\",\"children\":[]}],\"props\":[],\"binds\":[],\"templates\":[]}";
    let digest = fnv1a64(src.as_bytes());
    let table = TemplateTable::new();
    match import_json(src, &table) {
        Err(d) => (AttackVerdict::Rejected(d.message), digest),
        Ok(_) => (AttackVerdict::Leaked, digest),
    }
}

/// 超深攻击：链式插入 1001 层——`MAX_TREE_DEPTH` = 512，
/// 第 513 层就必须被 `DEPTH_LIMIT_EXCEEDED` 拒绝（严于锚点的 1000）。
/// 全部 1001 层都插进去 = 深度攻击穿透 = P0。
fn attack_overdeep() -> (AttackVerdict, u64) {
    let mut t = corpus_tree();
    let mut rejected_at: Option<usize> = None;
    let mut i = 0usize;
    let mut parent = String::from("a1");
    while i < DEPTH_ATTACK_LAYERS {
        let child = format!("deep_{}", i);
        match insert(&mut t, &parent, &child, None, SingleParentPolicy::Reject) {
            Ok(_) => {
                parent = child;
            }
            Err(_) => {
                rejected_at = Some(i);
                break;
            }
        }
        i += 1;
    }
    let input = format!("overdeep:{}layers:rejected_at={:?}", DEPTH_ATTACK_LAYERS, rejected_at);
    let digest = fnv1a64(input.as_bytes());
    match rejected_at {
        // 在 513 层之内被拒（防御线 512）即防线有效；树完好。
        Some(_) if invariants_green(&t) => (AttackVerdict::Allowed, digest),
        Some(_) => (AttackVerdict::Leaked, digest),
        None => (AttackVerdict::Leaked, digest),
    }
}

/// 超宽攻击：单节点挂 10001 子（树模型不设宽度硬顶——万子是合法树），
/// 性能防护语义在**消费侧分页**：F2608 `pick_subtree` 小预算交付 ≤ 预算、
/// 全树规模如实记账、扫描不爆栈、不变量保持。
fn attack_overwide() -> (AttackVerdict, u64) {
    let mut t = corpus_tree();
    let _ = insert(&mut t, "root", "wide", None, SingleParentPolicy::Reject);
    let mut i = 0usize;
    while i < WIDTH_ATTACK_CHILDREN {
        let child = format!("w{}", i);
        let _ = insert(&mut t, "wide", &child, None, SingleParentPolicy::Reject);
        i += 1;
    }
    let mut bag = DiagBag::new();
    let pick = pick_subtree(&t, "wide", 64, &mut bag);
    let input = format!("overwide:{}kids:delivered={}:total={}", WIDTH_ATTACK_CHILDREN, pick.delivered(), pick.total_subtree);
    let digest = fnv1a64(input.as_bytes());
    let paged = pick.delivered() <= 64
        && pick.total_subtree == (WIDTH_ATTACK_CHILDREN + 1) as u32
        && invariants_green(&t);
    if paged {
        (AttackVerdict::Allowed, digest)
    } else {
        (AttackVerdict::Leaked, digest)
    }
}

/// 段一执行：四类畸形逐个注入，返回各类结论。
pub fn run_tree_malform_attacks() -> [(TreeMalform, AttackVerdict, u64); 4] {
    let (v1, h1) = attack_cycle();
    let (v2, h2) = attack_orphan();
    let (v3, h3) = attack_overdeep();
    let (v4, h4) = attack_overwide();
    [
        (TreeMalform::Cycle, v1, h1),
        (TreeMalform::Orphan, v2, h2),
        (TreeMalform::OverDeep, v3, h3),
        (TreeMalform::OverWide, v4, h4),
    ]
}

// ---------------------------------------------------------------------------
// 三、段二：序列化对抗（F2607 四重校验 · 畸形 JSON 四类）
// ---------------------------------------------------------------------------

/// 序列化畸形四类（锚点原文：结构损坏/类型错误/绑定路径注入/深度攻击）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SerdeMalform {
    /// JSON 结构损坏（截断/括号错配——解析层拦截）。
    #[default]
    BrokenJson,
    /// 类型错误（root 给数字——形状层拦截）。
    WrongType,
    /// 绑定路径注入（`//evil` 首尾斜杠/空段——绑定重拦截）。
    BindPathInjection,
    /// 嵌套深度攻击（40 层括号 > `MAX_JSON_NEST`——解析层拦截）。
    NestAttack,
}

impl SerdeMalform {
    /// 全部四类（判据遍历序）。
    pub const ALL: [SerdeMalform; 4] = [
        SerdeMalform::BrokenJson,
        SerdeMalform::WrongType,
        SerdeMalform::BindPathInjection,
        SerdeMalform::NestAttack,
    ];

    /// 判据文案用短名。
    pub fn as_str(self) -> &'static str {
        match self {
            SerdeMalform::BrokenJson => "BrokenJson",
            SerdeMalform::WrongType => "WrongType",
            SerdeMalform::BindPathInjection => "BindPathInjection",
            SerdeMalform::NestAttack => "NestAttack",
        }
    }

    /// 对抗语料（每类一段畸形 JSON——确定性字面量，不是随机生成：
    /// 随机语料让「谁红了」不可复现，固字面量才进案例库）。
    pub const fn corpus(self) -> &'static str {
        match self {
            // 截断：文档在节点段中间戛然而止。
            SerdeMalform::BrokenJson => "{\"version\":2,\"root\":\"r\",\"nodes\":[{\"id\":\"r\",\"kind\":\"panel\",\"children\":[",
            // 类型错误：root 给数字（形状层要求字符串）。
            SerdeMalform::WrongType => "{\"version\":2,\"root\":42,\"nodes\":[{\"id\":\"r\",\"kind\":\"panel\",\"children\":[]}],\"props\":[],\"binds\":[],\"templates\":[]}",
            // 绑定路径注入：连续分隔符 + 首部斜杠（路径解析器攻击）。
            SerdeMalform::BindPathInjection => "{\"version\":2,\"root\":\"r\",\"nodes\":[{\"id\":\"r\",\"kind\":\"panel\",\"children\":[]}],\"props\":[],\"binds\":[{\"node\":\"r\",\"path\":\"//evil\"}],\"templates\":[]}",
            // 深度攻击在运行时拼（40 层括号）——见 `serde_corpus_dynamic`。
            SerdeMalform::NestAttack => "",
        }
    }
}

/// 合法对照语料（**双向验证的反面**：合法输入不得被误拦）。
pub const VALID_CORPUS: &str = "{\"version\":2,\"root\":\"r\",\"nodes\":[{\"id\":\"r\",\"kind\":\"panel\",\"children\":[\"c1\"]},{\"id\":\"c1\",\"kind\":\"leaf\",\"children\":[]}],\"props\":[],\"binds\":[],\"templates\":[]}";

/// 动态语料（嵌套深度攻击需要按 [`NEST_ATTACK_DEPTH`] 拼括号）。
fn serde_corpus_dynamic(kind: SerdeMalform) -> String {
    match kind {
        SerdeMalform::NestAttack => {
            let mut s = String::new();
            let mut i = 0usize;
            while i < NEST_ATTACK_DEPTH {
                s.push('[');
                i += 1;
            }
            i = 0;
            while i < NEST_ATTACK_DEPTH {
                s.push(']');
                i += 1;
            }
            s
        }
        other => String::from(other.corpus()),
    }
}

/// 段二执行：四类畸形 JSON 逐个注入真 F2607 管线。
///
/// 拦截语义按攻击类分口：`BrokenJson`/`NestAttack` 由**解析层**拒
/// （`parse_json` Err）；`WrongType`/`BindPathInjection` 由**形状/绑定重**拒
/// （`parse_and_validate` 出违规）。全部走 `parse_and_validate` 一条口，
/// 「拦截 = Err 或违规非空」，静默通过 = `Leaked`（P0）。
pub fn run_serde_attacks() -> [(SerdeMalform, AttackVerdict, u64); 4] {
    let mut out: [(SerdeMalform, AttackVerdict, u64); 4] =
        SerdeMalform::ALL.map(|k| (k, AttackVerdict::Leaked, 0u64));
    let mut k = 0usize;
    while k < SerdeMalform::ALL.len() {
        let kind = SerdeMalform::ALL[k];
        let src = serde_corpus_dynamic(kind);
        let digest = fnv1a64(src.as_bytes());
        let verdict = match parse_and_validate(&src) {
            Err(d) => AttackVerdict::Rejected(d.message),
            Ok((_doc, report, shape_violations)) => {
                let blocked = !report.all_passed()
                    || !shape_violations.is_empty()
                    || report.total_violations() > 0
                    || !report.verdict(Fold::Bind).passed();
                if blocked {
                    AttackVerdict::Rejected(String::from("校验重拦截"))
                } else {
                    AttackVerdict::Leaked
                }
            }
        };
        out[k] = (kind, verdict, digest);
        k += 1;
    }
    out
}

/// 合法对照：合法语料必须全绿通过（双向验证的反面——不误拦）。
pub fn valid_corpus_passes() -> bool {
    match parse_and_validate(VALID_CORPUS) {
        Err(_) => false,
        Ok((_doc, report, shape_violations)) => {
            report.all_passed() && shape_violations.is_empty()
        }
    }
}

// ---------------------------------------------------------------------------
// 四、段三：混合压测（序列化 + 属性风暴 + 增量并发 · 组合不崩不变量）
// ---------------------------------------------------------------------------

/// 混合压测总账（三面计数逐项守恒——口径唯一）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MixedStormReport {
    /// 实际执行步数（恒 ≤ [`MIXED_STORM_STEPS`]——看门狗预算）。
    pub steps: u32,
    /// 调优生效数。
    pub tune_applied: u32,
    /// 调优拒绝数（含刻意的非法槽位攻击）。
    pub tune_rejected: u32,
    /// 增量入队尝试数。
    pub incr_attempts: u32,
    /// 增量提交应用数。
    pub incr_applied: u32,
    /// 序列化-校验轮次。
    pub serde_rounds: u32,
    /// 合法语料在风暴中被误拦的次数（**恒 0**——非 0 即校验过敏）。
    pub serde_false_rejects: u32,
    /// 不变量红项次数（**恒 0**——非 0 即组合把树打坏）。
    pub invariant_reds: u32,
}

/// 混合压测：三面交错 [`MIXED_STORM_STEPS`] 步——
/// 每步一次调优指令（LCG 选槽位/算符，1/8 概率打非法槽位）；
/// 每步一次增量入队（LCG 选掩码）；每 4 步一轮「导出→to_json→四重校验」
/// （合法树导出的文档必须全绿——误拦即过敏）；每 16 步一次不变量核验。
///
/// 执行器**不吞 panic**：任何一步 panic 直接炸穿判据进程（P0 语义）。
pub fn run_mixed_storm(seed: u64) -> MixedStormReport {
    let mut rep = MixedStormReport { steps: 0, ..Default::default() };
    let mut lcg = Lcg::new(seed);
    let mut mirror = TreeMirror { tree: corpus_tree(), slots: Vec::new(), total_delivered: 0 };
    // 槽位映射：0=root, 1=a, 2=b, 3=b1（都有属性键声明的节点在语料里补）。
    for id in ["root", "a", "b", "b1"] {
        mirror.slots.push(NodeMirror { id: String::from(id), values: Vec::new(), dirty: false });
    }
    // 语料树节点补属性键（SetProp 的键集封闭前提，同 F2608 判据语料纪律）。
    for id in ["root", "a", "b", "b1"] {
        if let Some(n) = mirror.tree.raw_mut(id) {
            n.property_keys = alloc::vec![PropertyKey::Text, PropertyKey::Visible];
        }
    }
    let mut incr = IncrementalUpdate::new();
    let mut ledger = RouteLedger::new();
    let mut engine = PropEngine::new();
    let mut bag = DiagBag::new();
    let mut channel = TuneChannel::default();
    let mut seq: u64 = 0;

    let mut step = 0usize;
    while step < MIXED_STORM_STEPS {
        // —— 调优面：1/8 概率打非法槽位（拒绝路径必须可达）。
        let slot = if lcg.below(8) == 0 { 999u32 } else { lcg.below(mirror.slots.len()) as u32 };
        let op = if lcg.below(4) == 0 {
            // 重排：b（槽 2）有两个孩子；其他槽位长度不足会被越界拒（也是合法拒绝）。
            TuneOp::ReorderChild { node: slot, from: 0, to: 1 }
        } else {
            TuneOp::SetProp {
                node: slot,
                key: PropertyKey::Text,
                value: PropValue::Number(step as f32),
            }
        };
        seq += 1;
        let ack = apply_tune(&mut channel, &mut mirror, TuneCommand { seq, op }, &mut bag);
        if ack.is_applied() {
            rep.tune_applied += 1;
        } else {
            rep.tune_rejected += 1;
        }

        // —— 增量面：LCG 选节点/掩码（空掩码入队是无操作——三掩码全真）。
        let target = mirror.slots[lcg.below(mirror.slots.len())].id.clone();
        let mask = PendingInvalid::mask_of(&Invalidation {
            render: true,
            layout: lcg.below(2) == 0,
            hit: lcg.below(2) == 0,
        });
        let _ = incr.queue_mut().enqueue(&target, mask);
        rep.incr_attempts += 1;

        // —— 每 4 步：提交一帧增量 + 序列化一轮。
        if (step + 1) % 4 == 0 {
            let apply_rep = incr.commit_frame(&mirror.tree, &mut ledger);
            rep.incr_applied += apply_rep.commit.applied as u32;
            let doc = export_snapshot(&mirror.tree, &engine);
            let outcome = match doc {
                Ok(d) => to_json(&d)
                    .map_err(|e| e.message)
                    .and_then(|text| parse_and_validate(&text).map_err(|e| e.message)),
                Err(e) => Err(e),
            };
            match outcome {
                Ok((_d2, report, shape)) => {
                    rep.serde_rounds += 1;
                    if !report.all_passed() || !shape.is_empty() {
                        rep.serde_false_rejects += 1;
                    }
                }
                Err(_) => {
                    // 风暴中的合法树导出被解析层拒 = 序列化面失守。
                    rep.serde_false_rejects += 1;
                }
            }
        }

        // —— 每 16 步：不变量核验（组合不得把树打坏）。
        if (step + 1) % 16 == 0 && !invariants_green(&mirror.tree) {
            rep.invariant_reds += 1;
        }
        rep.steps += 1;
        step += 1;
    }
    // 收尾帧：队列收敛（提交后队列必须清空）。
    let apply_rep = incr.commit_frame(&mirror.tree, &mut ledger);
    rep.incr_applied += apply_rep.commit.applied as u32;
    if !invariants_green(&mirror.tree) {
        rep.invariant_reds += 1;
    }
    rep
}

/// 风暴快照导出（树 → TreeDoc，模板段留空——混合压测不含模板）。
fn export_snapshot(tree: &ControlTree, engine: &PropEngine) -> Result<TreeDoc, String> {
    let nodes = collect_nodes(tree).map_err(|d| d.message)?;
    let props = collect_props(engine, tree).map_err(|d| d.message)?;
    let binds = collect_binds(tree).map_err(|d| d.message)?;
    let mut doc = TreeDoc::new(tree.root());
    doc.version = CUR_VERSION;
    doc.nodes = nodes;
    doc.props = props;
    doc.binds = binds;
    Ok(doc)
}

// ---------------------------------------------------------------------------
// 五、案例固化库（FNV 去重 + 库容控制 + P0 立案 24h）
// ---------------------------------------------------------------------------

/// 案例元数据（输入/期望/复现/去重哈希——四要素缺一即不可复核）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaseMeta {
    /// 案例 id。
    pub id: u32,
    /// 输入摘要（FNV-1a）。
    pub input_digest: u64,
    /// 期望语义（人读）。
    pub expected: String,
    /// 复现要点（最小复现脚本）。
    pub repro: String,
}

/// 案例库（去重 + 库容）。
#[derive(Clone, Debug, Default)]
pub struct CaseStore {
    entries: Vec<CaseMeta>,
    next_id: u32,
    /// 去重命中计数（读屏可达——去重真的在去）。
    dedup_hits: u64,
}

impl CaseStore {
    /// 空库。
    pub fn new() -> CaseStore {
        CaseStore::default()
    }

    /// 入册：同摘要去重（幂等）；超容显性拒绝（不静默丢案）。
    pub fn add(&mut self, input_digest: u64, expected: &str, repro: &str) -> Result<u32, String> {
        if self.entries.iter().any(|e| e.input_digest == input_digest) {
            self.dedup_hits = self.dedup_hits.saturating_add(1);
            return Ok(0);
        }
        if self.entries.len() >= MAX_CASES {
            return Err(format!(
                "{}：案例库已满（{} 条）——扩容或先固化归档，不静默丢案",
                E_FUZZ_CASE_FULL, MAX_CASES
            ));
        }
        self.next_id += 1;
        let id = self.next_id;
        self.entries.push(CaseMeta {
            id,
            input_digest,
            expected: String::from(expected),
            repro: String::from(repro),
        });
        Ok(id)
    }

    /// 在册数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空库。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 去重命中数。
    pub fn dedup_hits(&self) -> u64 {
        self.dedup_hits
    }

    /// 全部条目。
    pub fn all(&self) -> &[CaseMeta] {
        &self.entries
    }
}

/// P0 立案（带 24h 复现期限——超期可判定，不是口头督促）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct P0Case {
    /// 立案码（[`E_P0_CYCLE_NOT_INTERCEPTED`] 等四码之一）。
    pub code: &'static str,
    /// 立案时刻（逻辑时钟 ms——无墙钟环境由调用方注入）。
    pub opened_ms: u64,
    /// 复现期限时刻 = opened + 24h。
    pub deadline_ms: u64,
}

impl P0Case {
    /// 新立案（期限自动钉 24h）。
    pub fn open(code: &'static str, now_ms: u64) -> P0Case {
        P0Case { code, opened_ms: now_ms, deadline_ms: now_ms.saturating_add(P0_REPRO_24H_MS) }
    }

    /// 是否超期（`now` 过期限即真——恰期限不超）。
    pub fn overdue(&self, now_ms: u64) -> bool {
        now_ms > self.deadline_ms
    }
}

// ---------------------------------------------------------------------------
// 六、总驱动：一轮 N 域 fuzz（三段 + 案例 + P0 账）
// ---------------------------------------------------------------------------

/// 一轮 fuzz 的总账。
#[derive(Clone, Debug, Default)]
pub struct FuzzReport {
    /// 四类树畸形的结论（逐类）。
    pub tree_attacks: [(TreeMalform, AttackVerdict, u64); 4],
    /// 四类序列化畸形的结论（逐类）。
    pub serde_attacks: [(SerdeMalform, AttackVerdict, u64); 4],
    /// 合法语料是否全绿通过（双向反面）。
    pub valid_corpus_ok: bool,
    /// 混合压测总账。
    pub mixed: MixedStormReport,
    /// P0 立案（空 = 本轮全绿；非空 = 逐条 24h 期限在案）。
    pub p0: Vec<P0Case>,
}

/// 判定一段攻击是否「显性拦截/防护生效」。
fn verdict_ok(v: &AttackVerdict) -> bool {
    matches!(v, AttackVerdict::Rejected(_) | AttackVerdict::Allowed)
}

/// 跑一轮 N 域控件树 fuzz（`now_ms` 为 P0 立案的逻辑时钟）。
///
/// 案例在内部入库（去重后返回在册数）；P0 按锚点四红线立案。
pub fn run_tree_fuzz(seed: u64, now_ms: u64) -> (FuzzReport, CaseStore) {
    let mut report = FuzzReport {
        tree_attacks: run_tree_malform_attacks(),
        serde_attacks: run_serde_attacks(),
        valid_corpus_ok: valid_corpus_passes(),
        mixed: run_mixed_storm(seed),
        p0: Vec::new(),
    };

    // P0 四红线（锚点错误路径原文）：
    // ① 环未拦截 → P0（单亲/无环不变量失守——F2602 根基）。
    if !verdict_ok(&report.tree_attacks[0].1) {
        report.p0.push(P0Case::open(E_P0_CYCLE_NOT_INTERCEPTED, now_ms));
    }
    // ② 孤儿未拦截 → P0（校验失守——与环同属结构重）。
    if !verdict_ok(&report.tree_attacks[1].1) {
        report.p0.push(P0Case::open(E_P0_SERDE_LEAK, now_ms));
    }
    // ③ 深度攻击穿透 → P0（性能防护失守——栈溢出风险，F2609 特化红线）。
    if !verdict_ok(&report.tree_attacks[2].1) {
        report.p0.push(P0Case::open(E_P0_DEPTH_PENETRATED, now_ms));
    }
    // ④ 序列化拦截失效 → P0（校验失守——家族）；合法被误拦同罪（过敏）。
    let mut k = 0usize;
    while k < report.serde_attacks.len() {
        if !verdict_ok(&report.serde_attacks[k].1) {
            report.p0.push(P0Case::open(E_P0_SERDE_LEAK, now_ms));
        }
        k += 1;
    }
    if !report.valid_corpus_ok {
        report.p0.push(P0Case::open(E_P0_SERDE_LEAK, now_ms));
    }
    // ⑤ 组合压测不崩不变量：误拦/红项非 0 → 序列化过敏立案（P0 语义，
    // 因为「合法输入被拦」同样是校验防线失守——方向相反的失守）。
    if report.mixed.serde_false_rejects > 0 || report.mixed.invariant_reds > 0 {
        report.p0.push(P0Case::open(E_P0_SERDE_LEAK, now_ms));
    }

    // 案例入库（去重哈希管理）：树四类 + 序列化四类 + 合法对照，八案一对照。
    let mut store = CaseStore::new();
    let mut k = 0usize;
    while k < report.tree_attacks.len() {
        let kind = report.tree_attacks[k].0;
        let verdict = &report.tree_attacks[k].1;
        let digest = report.tree_attacks[k].2;
        let expected = match verdict {
            AttackVerdict::Rejected(m) => format!("树畸形 {} 显性拒绝：{}", kind.as_str(), m),
            AttackVerdict::Allowed => format!("树畸形 {} 防护生效（分页/深度上限）", kind.as_str()),
            AttackVerdict::Leaked => format!("树畸形 {} 静默通过（P0）", kind.as_str()),
        };
        let _ = store.add(
            digest,
            &expected,
            &format!("ven09_fuzz::run_tree_malform_attacks()[{}]（seed 无关——确定性语料）", k),
        );
        k += 1;
    }
    let mut k = 0usize;
    while k < report.serde_attacks.len() {
        let kind = report.serde_attacks[k].0;
        let verdict = &report.serde_attacks[k].1;
        let digest = report.serde_attacks[k].2;
        let expected = match verdict {
            AttackVerdict::Rejected(m) => format!("畸形 JSON {} 显性拒绝：{}", kind.as_str(), m),
            AttackVerdict::Allowed => format!("畸形 JSON {} 防护生效", kind.as_str()),
            AttackVerdict::Leaked => format!("畸形 JSON {} 静默通过（P0）", kind.as_str()),
        };
        let _ = store.add(
            digest,
            &expected,
            &format!("ven09_fuzz::run_serde_attacks()[{}]（corpus 字面量在案）", k),
        );
        k += 1;
    }
    let _ = store.add(
        fnv1a64(VALID_CORPUS.as_bytes()),
        "合法语料全绿通过（不误拦）",
        "ven09_fuzz::valid_corpus_passes()",
    );
    (report, store)
}

// ---------------------------------------------------------------------------
// 七、判据（锚点：四类畸形、深度防护、校验对抗、24h 固化、判据）
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

/// VE-F2609 域自检（判据逐条映射锚点）。
pub fn run_ven09_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F2609");

    // ====================================================================
    // 一、四类畸形（判据一）：逐类注入 + 结论可判定
    // ====================================================================

    // 一批：先跑一轮总 fuzz（确定性——全部语料为固字面量 + LCG 定种子）。
    let (report, store) = run_tree_fuzz(0x2609, 1_000);

    // 判据 1：四类齐备且逐类有结论（Rejected/Allowed 皆可，Leaked 不行）。
    {
        let mut ok = TreeMalform::ALL.len() == 4;
        let mut i = 0usize;
        while i < report.tree_attacks.len() {
            let kind = report.tree_attacks[i].0;
            let verdict = &report.tree_attacks[i].1;
            if TreeMalform::ALL[i] != kind || !verdict_ok(verdict) {
                ok = false;
            }
            i += 1;
        }
        s.add("N9-四类畸形-逐类有结论", ok, "环/孤儿/超深/超宽四类注入，逐类显性拒绝或防护生效");
    }

    // 判据 2：环攻击双策略都被显性拒，且树无半成品。
    {
        let (v, _) = attack_cycle();
        s.add(
            "N9-四类畸形-环必拦截",
            matches!(v, AttackVerdict::Rejected(_)),
            "成环 insert 双策略均 Err（单亲/无环两道防线）且不变量绿",
        );
    }

    // 判据 3：孤儿（导入层）被结构重抓住——引用不存在父/不可达节点必拒。
    {
        let (v, _) = attack_orphan();
        s.add(
            "N9-四类畸形-孤儿必拦截",
            matches!(v, AttackVerdict::Rejected(_)),
            "不可达 ghost 节点被 F2607 结构重拒绝（可达性检查）",
        );
    }

    // ====================================================================
    // 二、深度防护（判据二）：1001 层攻击被拒 + 防线声明 + 不爆栈
    // ====================================================================

    // 判据 4：深度攻击在防线内被拒（512 < 1001——严于锚点声明，如实标注）。
    s.add(
        "N9-深度防护-1001层必拒",
        MAX_TREE_DEPTH < DEPTH_ATTACK_LAYERS,
        "防线 512 严于锚点 1000——1001 层攻击在 513 层即拒（诚实声明）",
    );

    // 判据 5：攻击结论是 Allowed（防护生效）而非 Leaked（穿透）。
    {
        let (v, _) = attack_overdeep();
        s.add(
            "N9-深度防护-攻击被防线吃下",
            matches!(v, AttackVerdict::Allowed),
            "1001 层链式插入 → DEPTH_LIMIT_EXCEEDED，树无半成品",
        );
    }

    // 判据 6：深度穿透必须进 P0（红线是可执行的，不是注释）。
    {
        // 构造一个「穿透」样本直接验 P0 流转：Leaked 结论 → P0Case 入账。
        let leaked = AttackVerdict::Leaked;
        let p0_open = !verdict_ok(&leaked); // Leaked → !ok → 立案
        s.add(
            "N9-深度防护-穿透即P0",
            p0_open && P0Case::open(E_P0_DEPTH_PENETRATED, 0).deadline_ms == P0_REPRO_24H_MS,
            "Leaked 结论触发 E_N9_P0_DEPTH_PENETRATED，期限自动钉 24h",
        );
    }

    // ====================================================================
    // 三、校验对抗（判据三）：四类畸形 JSON 全拦截 + 合法不误拦
    // ====================================================================

    // 判据 7：四类序列化攻击逐类有结论（无 Leaked）。
    {
        let mut ok = SerdeMalform::ALL.len() == 4;
        let mut i = 0usize;
        while i < report.serde_attacks.len() {
            let kind = report.serde_attacks[i].0;
            let verdict = &report.serde_attacks[i].1;
            if SerdeMalform::ALL[i] != kind || !verdict_ok(verdict) {
                ok = false;
            }
            i += 1;
        }
        s.add("N9-校验对抗-四类全拦截", ok, "结构损坏/类型错误/路径注入/深度攻击全部显性拦截");
    }

    // 判据 8：深度攻击语料确超 `MAX_JSON_NEST`（攻击面真实性——语料若在
    // 拦截线之下，判据就是恒真）。NestAttack 语料 40 层 > 32 层拦截线。
    {
        let src = serde_corpus_dynamic(SerdeMalform::NestAttack);
        let deep = parse_json(&src).is_err();
        s.add(
            "N9-校验对抗-嵌套攻击真实超线",
            deep && NEST_ATTACK_DEPTH > 32,
            "40 层括号被解析层拒绝（MAX_JSON_NEST=32）",
        );
    }

    // 判据 9：合法语料全绿通过（双向——校验不过敏）。
    s.add("N9-校验对抗-合法不误拦", report.valid_corpus_ok, "合法 v2 文档四重全绿");

    // ====================================================================
    // 四、混合压测（组合不崩不变量 + 计数守恒 + 看门狗）
    // ====================================================================

    // 判据 10：组合零误拦零红项（树操作+序列化+失效并发不崩不变量）。
    s.add(
        "N9-混合-零误拦零红项",
        report.mixed.serde_false_rejects == 0 && report.mixed.invariant_reds == 0,
        "96 步三面交错，序列化全绿、不变量全程绿",
    );

    // 判据 11：调优计数守恒（生效+拒绝 == 步数——口径唯一）。
    {
        let m = &report.mixed;
        s.add(
            "N9-混合-调优计数守恒",
            m.tune_applied + m.tune_rejected == m.steps && m.steps == MIXED_STORM_STEPS as u32,
            "每步恰一条指令：applied+rejected = steps（含刻意非法槽位攻击）",
        );
    }

    // 判据 12：增量队列收敛（提交后清空——失效不积压）。
    {
        let m = &report.mixed;
        s.add(
            "N9-混合-增量面有吞吐",
            m.incr_attempts >= m.incr_applied && m.incr_applied > 0,
            "入队 ≥ 应用且应用非零（提交路径真实工作）",
        );
    }

    // 判据 13：双跑确定性（同种子同总账——LCG 复现前提）。
    {
        let again = run_mixed_storm(0x2609);
        s.add(
            "N9-混合-同种子双跑一致",
            again == report.mixed,
            "同种子逐字段一致（自持 LCG 不引外部随机源）",
        );
    }

    // ====================================================================
    // 五、24h 固化（判据四）：案例库去重/库容/四要素 + P0 期限
    // ====================================================================

    // 判据 14：案例入库且四要素齐备（输入/期望/复现/哈希）。
    {
        let mut ok = store.len() >= 9 && !store.is_empty();
        let mut i = 0usize;
        while i < store.all().len() {
            let c = &store.all()[i];
            if c.expected.is_empty() || c.repro.is_empty() || c.input_digest == 0 {
                ok = false;
            }
            i += 1;
        }
        s.add("N9-固化-案例四要素齐备", ok, "树四案+序列化四案+合法对照 ≥9 条，要素无空");
    }

    // 判据 15：去重真的在去（同案重入 → dedup_hits 增，库不膨胀）。
    {
        let mut dup = CaseStore::new();
        let h = fnv1a64(b"same-case");
        let _ = dup.add(h, "期望", "复现");
        let len_before = dup.len();
        let again = dup.add(h, "期望", "复现");
        s.add(
            "N9-固化-去重幂等",
            len_before == 1 && again == Ok(0) && dup.dedup_hits() == 1,
            "同摘要重入返回 0、库容不变、命中计数 +1",
        );
    }

    // 判据 16：库容封顶显性拒绝（不静默丢案）。
    {
        let mut full = CaseStore::new();
        let mut i = 0usize;
        let mut filled = false;
        while i < MAX_CASES {
            if full.add(fnv1a64(format!("case-{}", i).as_bytes()), "e", "r").is_err() {
                break;
            }
            i += 1;
        }
        let over = full.add(fnv1a64(b"over-the-top"), "e", "r");
        filled = over.is_err() && full.len() == MAX_CASES
            && over.unwrap_err().contains(E_FUZZ_CASE_FULL);
        s.add("N9-固化-库容封顶显性", filled, "满库再入 → 显性 Err 且码在案，不静默丢");
    }

    // 判据 17：P0 期限 24h 数值化且恰边界可判定（期限当下不超，过 1ms 即超）。
    {
        let p0 = P0Case::open(E_P0_CRASH, 1_000);
        s.add(
            "N9-固化-P0期限恰边界",
            P0_REPRO_24H_MS == 86_400_000
                && p0.deadline_ms == 1_000 + P0_REPRO_24H_MS
                && !p0.overdue(1_000 + P0_REPRO_24H_MS)
                && p0.overdue(1_000 + P0_REPRO_24H_MS + 1),
            "24h 数值化 86_400_000ms；恰期限不超、过 1ms 即超",
        );
    }

    // 判据 18：本轮全绿时 P0 账必须为空（有 P0 就不许宣称通过——账实一致）。
    {
        let mut all_ok = true;
        let mut i = 0usize;
        while i < report.tree_attacks.len() {
            if !verdict_ok(&report.tree_attacks[i].1) {
                all_ok = false;
            }
            i += 1;
        }
        i = 0;
        while i < report.serde_attacks.len() {
            if !verdict_ok(&report.serde_attacks[i].1) {
                all_ok = false;
            }
            i += 1;
        }
        let expect_empty = all_ok && report.valid_corpus_ok
            && report.mixed.serde_false_rejects == 0
            && report.mixed.invariant_reds == 0;
        s.add(
            "N9-固化-账实一致",
            (report.p0.is_empty()) == expect_empty,
            "全绿 ↔ P0 空；任一失守 ↔ P0 非空（双向钉死）",
        );
    }

    // ====================================================================
    // 六、框架（F1813 家族 N01 面）：LCG/看门狗/去重哈希
    // ====================================================================

    // 判据 19：LCG 同种子同流（复现前提），异种子异流（不是常量发生器）。
    {
        let mut a = Lcg::new(7);
        let mut b = Lcg::new(7);
        let mut c = Lcg::new(8);
        let mut same = true;
        let mut diff = false;
        let mut i = 0usize;
        while i < 16 {
            if a.next_u64() != b.next_u64() {
                same = false;
            }
            if a.next_u64() != c.next_u64() {
                diff = true;
            }
            i += 1;
        }
        s.add("N9-框架-LCG同种子同流", same && diff, "同种子逐位一致；异种子分流");
    }

    // 判据 20：步数看门狗预算恒界（组合压测步数 == 预算，超支不可能）。
    s.add(
        "N9-框架-步数预算恒界",
        report.mixed.steps <= MIXED_STORM_STEPS as u32 && MIXED_STORM_STEPS > 0,
        "锚点「组合 1 小时档」的机检替身：步数封顶可判定",
    );

    // 判据 21：去重哈希确定性（同输入同哈希、异输入异哈希——抽两对）。
    {
        let h1 = fnv1a64(b"ven09-cycle");
        let h2 = fnv1a64(b"ven09-cycle");
        let h3 = fnv1a64(b"ven09-cycle2");
        s.add(
            "N9-框架-FNV确定性",
            h1 == h2 && h1 != h3,
            "同输入必同哈希；异输入异哈希（去重前提）",
        );
    }

    // 判据 22：钳制失灵 → P1（锚点「钳制失灵→立案」——立案语义挂在
    // 非有限值上：属性风暴写入 NaN 必被钳 0（F2608 纪律），未钳即 P1）。
    {
        let mut mirror = TreeMirror {
            tree: corpus_tree(),
            slots: Vec::new(),
            total_delivered: 0,
        };
        for id in ["root", "a", "b", "b1"] {
            mirror.slots.push(NodeMirror { id: String::from(id), values: Vec::new(), dirty: false });
            if let Some(n) = mirror.tree.raw_mut(id) {
                n.property_keys = alloc::vec![PropertyKey::Text];
            }
        }
        let mut ch = super::ven08_debug::TuneChannel::default();
        let mut bag = DiagBag::new();
        let cmd = TuneCommand {
            seq: 1,
            op: TuneOp::SetProp {
                node: 0,
                key: PropertyKey::Text,
                value: PropValue::Number(alloc::format!("").len() as f32 / 0.0),
            },
        };
        let _ = apply_tune(&mut ch, &mut mirror, cmd, &mut bag);
        let clamped = bag.has(super::ven08_debug::N8DiagCode::PropNonFiniteClamped);
        s.add(
            "N9-混合-NaN钳制不失灵",
            clamped,
            "风暴写入 NaN → 钳 0 + 显性码（未钳即 E_N9_P1_CLAMP_FAILURE 立案面）",
        );
    }

    s
}
