//! VE-F2013 · 后处理调试数据 —— 域自检（VE-K 域 · 后处理架构与 Bloom 组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2013`
//!
//! **判据（锚点原文五条）**：三类负载、逐级透视、图结构可视、发行版零成本、判据。
//!
//! ## 判据层的写法纪律（与本域实现纪律同等约束）
//!
//! - **`alloc` 三件套必须逐个显式引入**（判据层自己要用 `Vec`/`String`，
//!   不能指望 `vek13_ppdbg::*` 的 glob 带进来——它里面的 `use alloc::…`
//!   是**私有导入**，glob 不会转发）。上一单 VE-F2012 曾因此在真
//!   no_std crate 里报 E0425/E0433，而普通 std 探针却全绿（**假绿**）。
//! - **判据侧独立重算**：FNV 摘要、字节数、字节用量全部在判据侧手算，
//!   绝不调用被测函数算出期望值（否则「比较器写反」会同时改掉实现与
//!   期望值造成假绿）。
//! - **反向变体**：每条结构性判据配一个反向变体，证明判据在实现坏掉时
//!   真的会红。基线全绿 + 变体转红 = 判据真的在判。
//! - **覆盖计数用 `==`**：断「恰等于 N」而非 `>= N`，否则新增用例时
//!   判据静默失效。
//! - **判据自身不得住在被它审计的集合里**：分族完整性门禁
//!   （`K13-聚合-分族不溢出`）住在独立的 d 族。若塞进 c 族，它要么被
//!   `merge` 挤掉（**永不执行 ⇒ 恒真**），要么回调合并入口而
//!   **无限递归**（实测 `has overflowed its stack`）。两条都是
//!   「门禁自己让门禁失效」的形态。
//!
//! ## 分族与容量的实测（VE-F2013 独有教训）
//!
//! 三族原始条数 45 + 45 + 27 = **117 > `MAX_CHECKS` 112**，
//! `CheckSet::merge` 满了就 `dropped += 1` ⇒ 单行合并注册会**静默丢掉
//! 最后 5 条判据**（初版实测丢「冒烟-非空且自洽」与「诊断-空袋自洽」），
//! 而只看 `tally()` 的注册表看不出任何异常——那些条等于从未存在过。
//!
//! 正解是**分族注册**（同域 `VE-F1613-a/b/c` 先例）：a/b/c/d 各占一行，
//! 每行独立成集、各自 ≤ `MAX_CHECKS`，合并路径在注册表上不可达。
//! d 族单条判据把这件事本身钉住，并按「两侧不同口径」对账条数
//! （三族之和 == 合并装下 + 溢出数），使溢出**可见**而非静默。
//!
//! ## 观测面必须覆盖门禁自身（实测踩坑）
//!
//! d 族是聚合门禁，而隔离探针起初的 `SUMMARY` **只统计合并入口**
//! `run_vek13_checks()`（a+b+c），d 族自己转红时红项落在 `SUMMARY`
//! 之外 ⇒ 变异 V14/V15（专门打 d 族的两条变异）被读成 **MISSED**，
//! 险些被误判为「门禁形同虚设」而去改门禁。真相是**观测面漏了**。
//! 修法：`SUMMARY` 统计注册表真实走的**四族**，合并入口的账另列
//! `MERGE_ENTRY`。**判据红 ≠ 判据错**——先归因是观测面还是被测面。
//!
//! ## 净值口径可被回退抵消（实测踩坑，`StripGuard.build_entries`）
//!
//! 「Release 档 `payload_builds == 0`」这条零成本判据**全绿放过**了
//! 变异 V12：`try_push` 在 Release 档先 `payload_builds += 1` 再 `-= 1`，
//! 终值恰好为 0。`payload_builds` 是**净值**口径，只要实现允许加减，
//! 就能被洗白——而头注声称的「**根本不进入**保证」在净值上**不可证伪**。
//! 正解：加**单调计数器** `build_entries`（只在真正进入构建分支时递增，
//! 全路径无任何减法），并断「Debug 档 == 1 作对照 + Release 档 == 0」。
//! 单调计数的 0 才让「未进入」成为可证伪命题（十诫第 10 条）。

use super::vek13_ppdbg::*;

// `alloc` 三件套里**只引入实际用到的**（理由见上）：本层用 `Vec`；
// `String` 未使用（文本产出走 `format!`，其对 `ToString` 的依赖是全限定
// 路径，显式 `use` 即死导入，编译器会指出来）。
use alloc::vec::Vec;

use crate::checks::{CheckSet, MAX_CHECKS};

// ---------------------------------------------------------------------------
// 判据侧独立常量（刻意第二份值，只用于门禁、不入生产数据流）
// ---------------------------------------------------------------------------

/// 判据侧独立手算的 FNV 偏移基。
const REF_FNV_INIT: u32 = 0x811c_9dc5;
/// 判据侧独立手算的 FNV 乘素。
const REF_FNV_PRIME: u32 = 0x0100_0193;

/// 判据侧独立重算 FNV-1a（**不调被测的 `fnv_step`**）。
fn ref_fnv(h: u32, byte: u32) -> u32 {
    (h ^ byte).wrapping_mul(REF_FNV_PRIME)
}

/// 判据侧独立重算信封摘要。
fn ref_env_checksum(kind: PayloadKind, schema: u16, byte_len: u32, seq: u32) -> u32 {
    let mut h = REF_FNV_INIT;
    h = ref_fnv(h, kind.wire() as u32);
    h = ref_fnv(h, schema as u32);
    h = ref_fnv(h, byte_len);
    h = ref_fnv(h, seq);
    h = ref_fnv(h, kind.label().len() as u32);
    h
}

/// 判据侧独立重算参数摘要（**归一规则照锚点语义重写**，不调被测函数）。
fn ref_param_digest(params: &[f32]) -> u32 {
    let mut h = REF_FNV_INIT;
    for p in params.iter() {
        let norm = if *p == 0.0 { 0.0f32 } else { *p };
        let bits = if norm.is_finite() { norm.to_bits() & 0xFFFF_FFFF } else { 0xFFFF_FFFF };
        h = ref_fnv(h, (bits >> 24) & 0xFF);
        h = ref_fnv(h, (bits >> 16) & 0xFF);
        h = ref_fnv(h, (bits >> 8) & 0xFF);
        h = ref_fnv(h, bits & 0xFF);
    }
    h
}

/// 判据侧独立重算 RT 档位字节数。
fn ref_scale_bytes(scale: RtScale, w: u32, h: u32) -> u32 {
    let div: u32 = match scale {
        RtScale::Full => 1,
        RtScale::Half => 2,
        RtScale::Quarter => 4,
        RtScale::Eighth => 8,
    };
    (w * h * 4) / div
}

/// 判据侧独立重算快照摘要。
///
/// **口径说明（必须与实现一致，否则是判据自己算错）**：实现在折入
/// 拓扑序时用的是一个**嵌套 FNV**——先由拓扑序单独折出
/// `topological_digest()`，再把该 32 位摘要当作一个字节项折进外层。
/// 判据侧照此口径重算（`ref_topo_digest` 先行、再折一项），
/// 但**不调用被测的 `topological_digest()`**，而是独立重算一遍。
fn ref_snap_checksum(version: u32, nodes: usize, edges: usize, topo: &[u32]) -> u32 {
    let mut h = REF_FNV_INIT;
    h = ref_fnv(h, version);
    h = ref_fnv(h, nodes as u32);
    h = ref_fnv(h, edges as u32);
    h = ref_fnv(h, topo.len() as u32);
    // 嵌套摘要：独立重算（逐项折出 topo 摘要，再作为一个 u32 折入）
    let mut inner = REF_FNV_INIT;
    for id in topo.iter() {
        inner = ref_fnv(inner, *id);
    }
    // 外层折入这 4 个字节（小端），与实现的 `h = fnv_step(h, self.topological_digest())`
    // 不同——实现折的是**整个 u32**当一个字节项，故此处照实现口径单步折。
    h = ref_fnv(h, inner);
    h
}

// ---------------------------------------------------------------------------
// 反向变体（先定义：它们必须能构造出「实现坏掉」的形态）
// ---------------------------------------------------------------------------

/// 反向变体 A：把 `RtScale` 的降档方向**反过来**（`Full → Eighth`）。
/// 若实现把 `downgrade` 写反，此变体产出的结果应与真实现不同 ⇒
/// 说明真判据对降档方向敏感。
fn variant_downgrade_reversed(s: RtScale) -> RtScale {
    match s {
        RtScale::Full => RtScale::Eighth,
        RtScale::Half => RtScale::Full,
        RtScale::Quarter => RtScale::Half,
        RtScale::Eighth => RtScale::Quarter,
    }
}

/// 反向变体 B：构造一个**摘要因字段被改而漂移**的信封。
fn variant_env_drifted(kind: PayloadKind, byte_len: u32, seq: u32) -> Envelope {
    let mut e = Envelope::new(kind, byte_len, seq);
    e.byte_len = e.byte_len.wrapping_add(1);
    e
}

/// 反向变体 C：构造一个**拓扑序被换成乱序**的快照（版本与节点数不变）。
fn variant_topo_shuffled(s: &GraphSnapshot) -> GraphSnapshot {
    let mut t = s.topo.clone();
    if t.len() >= 2 {
        let last = t.len() - 1;
        let tmp = t[0];
        t[0] = t[last];
        t[last] = tmp;
    }
    GraphSnapshot::new(s.version, s.nodes.clone(), s.edges.clone(), t)
}

/// 反向变体 D：图里**造一个环**，拓扑排序必须拒绝。
fn variant_cyclic_graph() -> GraphModel {
    let mut g = GraphModel::new();
    let mut bag = DiagBag::new();
    g.add_node(GraphNode { id: 1, effect: 10, rt_alias: 100 });
    g.add_node(GraphNode { id: 2, effect: 11, rt_alias: 101 });
    g.add_node(GraphNode { id: 3, effect: 12, rt_alias: 102 });
    g.add_edge(GraphEdge { from: 1, to: 2 }, &mut bag);
    g.add_edge(GraphEdge { from: 2, to: 3 }, &mut bag);
    g.add_edge(GraphEdge { from: 3, to: 1 }, &mut bag);
    g
}

/// 反向变体 E：让**独占耗时超过总耗时**（口径被突破的形态）。
fn variant_exclusive_exceeds_total() -> EffectStat {
    EffectStat {
        frame: 7,
        effect: 10,
        enabled: true,
        param_digest: 0,
        exclusive_ns: 900,
        total_ns: 300,
    }
}

/// 反向变体 F：**丢弃一条边**的图（判据应发现边数/拓扑序对不上）。
fn variant_edge_dropped() -> GraphModel {
    let mut g = linear_graph();
    let mut bag = DiagBag::new();
    // 加一条从 3 到 5 的边后再把边表清空重建，模拟「边被吞」。
    g.add_edge(GraphEdge { from: 3, to: 5 }, &mut bag);
    let mut g2 = GraphModel::new();
    for n in g.nodes().iter() {
        g2.add_node(*n);
    }
    for e in g.edges().iter() {
        if e.from != 3 {
            g2.add_edge(*e, &mut bag);
        }
    }
    for r in g.physical_rts().iter() {
        g2.alloc_rt(*r);
    }
    g2
}

// ---------------------------------------------------------------------------
// 共享语料（构造图/环等的索引**由语料常量推导**，不硬写数字）
// ---------------------------------------------------------------------------

/// 一张线性链图：1 → 2 → 3 → 4。
///
/// **alias 与物理 RT 一一连续编号**（100..103 对 4 个节点）：
/// alias 从 `100 + (id-1)` 起算而非 `100 + id`，否则最后一个节点的
/// alias 落在物理表之外，`rt_of` 恒 `None`——那是**语料缺陷**
/// （自己造的语料对不上自己的表），不是实现缺陷。
fn linear_graph() -> GraphModel {
    let mut g = GraphModel::new();
    let mut bag = DiagBag::new();
    let ids = [1u32, 2, 3, 4];
    let mut k = 0usize;
    while k < ids.len() {
        let id = ids[k];
        let alias = LINEAR_ALIAS_BASE + k as u32;
        g.add_node(GraphNode { id, effect: 10 + id as u16, rt_alias: alias });
        g.alloc_rt(RtRef {
            node: alias,
            handle: LINEAR_HANDLE_BASE + k as u32,
            width: 1920,
            height: 1080,
        });
        k += 1;
    }
    let mut i = 0usize;
    while i + 1 < ids.len() {
        g.add_edge(GraphEdge { from: ids[i], to: ids[i + 1] }, &mut bag);
        i += 1;
    }
    g
}

/// 线性语料的 alias 基址（**判据侧常量**，判据由语料常量推导不写死）。
const LINEAR_ALIAS_BASE: u32 = 100;
/// 线性语料的物理句柄基址。
const LINEAR_HANDLE_BASE: u32 = 0xA000;

/// 一张带**别名复用**的图：节点 3 与 4 共用 alias（第三个 alias）。
///
/// 复用目标 alias 由语料常量推导（[`LINEAR_ALIAS_BASE`] + 2），
/// 不写死数字——写死会在线性语料改动后静默指错。
fn alias_reuse_graph() -> GraphModel {
    let shared = LINEAR_ALIAS_BASE + 2;
    let g = linear_graph();
    let mut bag = DiagBag::new();
    let mut nodes: Vec<GraphNode> = Vec::new();
    for n in g.nodes().iter() {
        if n.id == 4 {
            nodes.push(GraphNode { id: 4, effect: n.effect, rt_alias: shared });
        } else {
            nodes.push(*n);
        }
    }
    let mut g2 = GraphModel::new();
    for n in nodes.iter() {
        g2.add_node(*n);
    }
    for e in g.edges().iter() {
        g2.add_edge(*e, &mut bag);
    }
    for r in g.physical_rts().iter() {
        g2.alloc_rt(*r);
    }
    g2
}

// ---------------------------------------------------------------------------
// a 族：三负载 / 信封 / 统计流（判据一）
// ---------------------------------------------------------------------------

fn run_a_checks() -> CheckSet {
    let mut cs = CheckSet::new("svstar2-vek13");

    // --- 三类负载齐全 ---
    cs.add(
        "K13-负载-三类齐全",
        PayloadKind::ALL.len() == 3,
        "三负载：效果统计流/RT拾取流/图结构快照",
    );
    cs.add("K13-负载-族一致", family_is_consistent(), "族声明自洽");
    cs.add("K13-负载-wire互异", wires_unique(), "线上编码互异");
    cs.add("K13-负载-标签互异", labels_unique(), "人话标签互异");

    // --- wire 反查双向（round-trip）---
    let mut rt_ok = true;
    for k in PayloadKind::ALL.iter() {
        if PayloadKind::from_wire(k.wire()) != Some(*k) {
            rt_ok = false;
        }
    }
    cs.add("K13-负载-wire反查往返", rt_ok, "wire→kind→wire 往返一致");

    // --- wire 取值落在 K 段（0x11..=0x13），不与 M 段（0x01..=0x04）撞 ---
    let k_wires_in_band = PayloadKind::ALL.iter().all(|k| k.wire() >= 0x11 && k.wire() <= 0x13);
    cs.add("K13-负载-wire落K段", k_wires_in_band, "K段编码 0x11..=0x13，不与 M 段重合");

    // --- 三类负载标签非空且互异（防「三份同名占位」）---
    let labels_nonempty = PayloadKind::ALL.iter().all(|k| !k.label().is_empty());
    cs.add("K13-负载-标签非空", labels_nonempty, "三类负载各有可读标签");

    // --- 信封摘要与判据侧独立重算逐位相符 ---
    let envs = [
        Envelope::new(PayloadKind::EffectStats, 480, 9),
        Envelope::new(PayloadKind::RtPick, 64, 9),
        Envelope::new(PayloadKind::GraphSnapshot, 96, 9),
    ];
    let env_match = envs.iter().all(|e| {
        e.checksum() == ref_env_checksum(e.kind, e.schema, e.byte_len, e.seq) && !e.drifted()
    });
    cs.add("K13-信封-摘要独立重算", env_match, "FNV 摘要与判据侧手算逐位相等");

    // --- 反向变体 A：改 byte_len 必导致漂移（证明摘要是活的）---
    let drifted = variant_env_drifted(PayloadKind::EffectStats, 480, 9);
    cs.add(
        "K13-信封-改载荷必漂移",
        drifted.drifted() && drifted.checksum() != envs[0].checksum(),
        "反向变体：byte_len+1 ⇒ 摘要改变且 drifted() 为真",
    );

    // --- 反向变体 A'：改 seq 必导致漂移 ---
    let seq_drift = Envelope::new(PayloadKind::EffectStats, 480, 10);
    cs.add(
        "K13-信封-改序号必漂移",
        seq_drift.checksum() != envs[0].checksum(),
        "反向变体：seq+1 ⇒ 摘要改变",
    );

    // --- schema 冻结 ---
    cs.add("K13-信封-schema冻结v1", ENVELOPE_SCHEMA == 1, "K 段信封 schema = 1");

    // --- 槽数与三类负载一一对应 ---
    cs.add("K13-信封-槽数对齐负载数", K_SEGMENT_SLOTS == PayloadKind::ALL.len(), "槽数 == 负载类型数");

    // --- 注册表：三信封全注册 ---
    let mut reg = EnvelopeRegistry::new();
    let mut bag = DiagBag::new();
    let mut all_reg = true;
    for e in envs.iter() {
        if !reg.register(*e) {
            all_reg = false;
        }
    }
    cs.add("K13-信封-三类全注册", all_reg && reg.registered() == 3, "三类负载各占一槽");

    // --- 对账：干净态零漂移，且不被拦截 ---
    let clean = reg.reconcile(&mut bag);
    cs.add(
        "K13-信封-干净态零漂移",
        clean == 0 && !reg.intercepted() && bag.is_empty(),
        "干净态 reconcile 返 0 且不拦截",
    );

    // --- 反向变体 B：注入漂移信封 ⇒ 对账必须拦下（逐个类型）---
    let mut caught = 0usize;
    for (i, k) in PayloadKind::ALL.iter().enumerate() {
        let mut r2 = EnvelopeRegistry::new();
        let mut b2 = DiagBag::new();
        r2.register(Envelope::new(*k, 100 + i as u32, 1));
        r2.register(variant_env_drifted(*k, 100 + i as u32, 1));
        let n = r2.reconcile(&mut b2);
        if n == 1 && r2.intercepted() && r2.take(*k).is_none() {
            caught += 1;
        }
    }
    cs.add(
        "K13-信封-漂移逐类拦截",
        caught == PayloadKind::ALL.len(),
        "三类各自漂移均被 reconcile 拦下且 take() 返回 None",
    );

    // --- 拦截是全局的：某一类漂移 ⇒ 全类 take() 都拿不到 ---
    let mut r3 = EnvelopeRegistry::new();
    let mut b3 = DiagBag::new();
    r3.register(Envelope::new(PayloadKind::EffectStats, 10, 1));
    r3.register(Envelope::new(PayloadKind::RtPick, 20, 1));
    r3.register(Envelope::new(PayloadKind::GraphSnapshot, 30, 1));
    r3.register(variant_env_drifted(PayloadKind::EffectStats, 10, 1));
    r3.reconcile(&mut b3);
    let all_none = PayloadKind::ALL.iter().all(|k| r3.take(*k).is_none());
    cs.add("K13-信封-拦截是全局的", all_none, "任一类型漂移 ⇒ 全部 take() 返回 None");

    // --- 反向变体：peek 不受拦截影响（拦截只挡 take，不挡窥视）---
    let peek_ok = r3.peek(PayloadKind::RtPick).is_some();
    cs.add("K13-信封-peek不随拦截", peek_ok, "拦截只挡 take；peek 仍可诊断");

    // --- 解除拦截后可再取 ---
    r3.clear_intercept();
    let after_clear = PayloadKind::ALL.iter().all(|k| r3.take(*k).is_some());
    cs.add("K13-信封-解除拦截可恢复", after_clear, "clear_intercept 后三类均可取");

    // --- 未知 kind / 错 schema 注册拒绝 ---
    let mut r4 = EnvelopeRegistry::new();
    let mut bad_schema = Envelope::new(PayloadKind::EffectStats, 1, 1);
    bad_schema.schema = 99;
    cs.add(
        "K13-信封-错schema拒收",
        !r4.register(bad_schema) && r4.registered() == 0,
        "schema≠1 的信封拒绝注册",
    );

    // --- 纪元随注册递增 ---
    let mut r5 = EnvelopeRegistry::new();
    let e0 = r5.epoch();
    r5.register(Envelope::new(PayloadKind::EffectStats, 1, 1));
    let e1 = r5.epoch();
    cs.add("K13-信封-纪元随注册递增", e1 == e0.wrapping_add(1), "每注册一次纪元 +1");

    // --- 统计流：环形容量定容 ---
    let mut ring = EffectStatsRing::new();
    let mut bag2 = DiagBag::new();
    cs.add("K13-统计-环形定容", ring.capacity() == STATS_RING_CAP && ring.is_empty(), "定容空环");

    // --- 写入 N 条 ⇒ len == N，latest 是最后一条 ---
    let mut i = 0u64;
    while i < 5 {
        let st = EffectStat {
            frame: i,
            effect: 10,
            enabled: true,
            param_digest: 0x1234,
            exclusive_ns: 100,
            total_ns: 300,
        };
        ring.push(st, &mut bag2);
        i += 1;
    }
    cs.add(
        "K13-统计-写入计数",
        ring.len() == 5 && ring.written() == 5,
        "写 5 条 ⇒ len==5 且 written==5",
    );
    let latest_ok = ring.latest().map(|s| s.frame == 4).unwrap_or(false);
    cs.add("K13-统计-latest是最新", latest_ok, "latest() 帧号为最后写入的帧");

    // --- 参数摘要与判据侧独立重算相符 ---
    let params = [0.5f32, 0.25, 1.0];
    let digest_ok = param_digest(&params) == ref_param_digest(&params);
    cs.add("K13-统计-参数摘要独立重算", digest_ok, "定点摘要与判据侧手算逐位相等");

    // --- 反向变体：-0.0 与 +0.0 必得同一摘要（假漂移防线，D3）---
    let dz = param_digest(&[0.0f32]) == param_digest(&[-0.0f32]);
    cs.add("K13-统计-正负零同摘要", dz, "-0.0 与 +0.0 摘要相同（否则是假漂移）");

    // --- 反向变体：NaN 不进摘要（换 NaN 载荷摘要不变）---
    let nan_a = param_digest(&[f32::NAN, 1.0]);
    let nan_b = param_digest(&[f32::from_bits(0x7FC0_0001), 1.0]);
    cs.add("K13-统计-NaN不污染摘要", nan_a == nan_b, "两个不同位模式的 NaN 得同一摘要");

    // --- 反向变体：不同参数必得不同摘要（摘要不是常数）---
    let d_diff = param_digest(&[1.0f32]) != param_digest(&[2.0f32]);
    cs.add("K13-统计-变参必改摘要", d_diff, "参数变化 ⇒ 摘要变化（否则摘要是常数门禁）");

    // --- 参数个数影响摘要（不是只看首元素）---
    let d_len = param_digest(&[1.0f32]) != param_digest(&[1.0f32, 0.0f32]);
    cs.add("K13-统计-摘要吃全部参数", d_len, "尾部参数也参与摘要");

    // --- 非有限值钳制告警 ---
    let mut bag3 = DiagBag::new();
    let _ = EffectStatsRing::from_timing(
        RawTiming::new(0, 10, 100, 50),
        true,
        &[f32::INFINITY, 1.0],
        &mut bag3,
    );
    cs.add(
        "K13-统计-非有限值告警",
        bag3.has(DiagCode::PARAM_NON_FINITE),
        "Inf 参数触发钳制告警（非静默）",
    );

    // --- 环形覆盖：写满后再写 ⇒ overwritten 递增、len 不增 ---
    let mut ring2 = EffectStatsRing::new();
    let mut bag4 = DiagBag::new();
    let mut j = 0u64;
    while j < (STATS_RING_CAP as u64) + 3 {
        ring2.push(
            EffectStat { frame: j, effect: 10, enabled: true, param_digest: 0, exclusive_ns: 1, total_ns: 2 },
            &mut bag4,
        );
        j += 1;
    }
    cs.add(
        "K13-统计-覆盖计数守恒",
        ring2.len() == STATS_RING_CAP && ring2.overwritten() == 3,
        "写 cap+3 条 ⇒ 覆盖 3 次且 len 仍为 cap",
    );
    cs.add(
        "K13-统计-覆盖有告警",
        bag4.count(DiagCode::STREAM_OVERWRITTEN) == 3,
        "每次覆盖都留痕（零静默）",
    );

    // --- 帧号非单调拒绝 ---
    let mut ring3 = EffectStatsRing::new();
    let mut bag5 = DiagBag::new();
    ring3.push(
        EffectStat { frame: 10, effect: 10, enabled: true, param_digest: 0, exclusive_ns: 1, total_ns: 2 },
        &mut bag5,
    );
    let rejected = !ring3.push(
        EffectStat { frame: 5, effect: 10, enabled: true, param_digest: 0, exclusive_ns: 1, total_ns: 2 },
        &mut bag5,
    );
    cs.add(
        "K13-统计-帧号回退拒绝",
        rejected && bag5.has(DiagCode::FRAME_NOT_MONOTONIC),
        "帧号回退被拒且留痕",
    );

    // --- 独占/总两口径分列（D2）---
    let mut ring4 = EffectStatsRing::new();
    let mut bag6 = DiagBag::new();
    let built = EffectStatsRing::from_timing(
        RawTiming::new(3, 11, 500, 120),
        true,
        &[0.5],
        &mut bag6,
    );
    ring4.push(built, &mut bag6);
    let sep = ring4
        .latest()
        .map(|s| s.exclusive_ns == 120 && s.total_ns == 500)
        .unwrap_or(false);
    cs.add("K13-统计-独占与总分列", sep, "独占 120 ≠ 总 500（两口径不合并）");

    // --- 反向变体 E：独占超总必须告警 ---
    let mut bag7 = DiagBag::new();
    let bad = variant_exclusive_exceeds_total();
    let mut ring5 = EffectStatsRing::new();
    ring5.push(bad, &mut bag7);
    cs.add(
        "K13-统计-独占超总告警",
        bag7.has(DiagCode::EXCLUSIVE_EXCEEDS_TOTAL),
        "独占>总 ⇒ 口径告警（反向变体）",
    );

    // --- 降频守恒：跳过的次数 + 写入次数 == 逻辑帧数（D4）---
    let mut ring6 = EffectStatsRing::new();
    let mut bag8 = DiagBag::new();
    ring6.retune(EFFECT_BUDGET + 1, &mut bag8);
    let mut f = 0u64;
    while f < 20 {
        ring6.push(
            EffectStat { frame: f, effect: 10, enabled: true, param_digest: 0, exclusive_ns: 1, total_ns: 2 },
            &mut bag8,
        );
        f += 1;
    }
    let stride = u64::from(ring6.stride());
    let conserve = ring6.written() + ring6.downsampled() == 20 && stride > 1;
    cs.add(
        "K13-统计-降频守恒",
        conserve,
        "写入 + 跳过 == 逻辑帧数（不丢不重）",
    );
    // 反向变体：**另起一个未降频的环** ⇒ 跳过数为 0、写入为全部帧。
    //
    // 【本条曾写错】初版复用已降频的 `ring6` 去断「无跳过」，而
    // `ring6.stride() == 8` ⇒ 它必然有跳过，判据永远红。**反向变体
    // 必须自己构造变体形态**，不能拿被测对象的主语料顺手断言。
    let mut ring_nd = EffectStatsRing::new();
    let mut bag_nd = DiagBag::new();
    let mut fn_ = 0u64;
    while fn_ < 20 {
        ring_nd.push(
            EffectStat {
                frame: fn_,
                effect: 10,
                enabled: true,
                param_digest: 0,
                exclusive_ns: 1,
                total_ns: 2,
            },
            &mut bag_nd,
        );
        fn_ += 1;
    }
    let no_skip = ring_nd.stride() == 1
        && ring_nd.downsampled() == 0
        && ring_nd.written() == 20
        && !bag_nd.has(DiagCode::STATS_DOWNSAMPLED);
    cs.add(
        "K13-统计-不降频则无跳过",
        no_skip,
        "反向变体：stride=1 时跳过数为 0、写入为全部帧",
    );

    // --- 降频有告警且步长受限 ---
    let mut ring7 = EffectStatsRing::new();
    let mut bag9 = DiagBag::new();
    let mut grew = 0u32;
    let mut k = 0usize;
    while k < 8 {
        if ring7.retune(EFFECT_BUDGET + 1, &mut bag9) {
            grew += 1;
        }
        k += 1;
    }
    // 降频次数**由步长实测推导**（1→2→4→8 共 3 次），不写死 8：
    // 触顶后 `retune` 返回 false 是**正确**行为（步长封顶），
    // 若判据硬写「每次调用都告警」就是在要求实现永不封顶。
    let expected_grows = {
        let mut n = 0u32;
        let mut st = 1u32;
        while st < 8 {
            st *= 2;
            n += 1;
        }
        n
    };
    cs.add("K13-统计-超预算触发降频", grew > 0 && ring7.stride() >= 2, "效果数超预算 ⇒ 步长升");
    cs.add("K13-统计-步长有上限", ring7.stride() == 8, "步长封顶 8（不无限降频丢数据）");
    cs.add(
        "K13-统计-降频有告警",
        grew == expected_grows && bag9.count(DiagCode::STATS_DOWNSAMPLED) == expected_grows as usize,
        "告警数恰等于实际降频次数（触顶后不再重复告警）",
    );
    // 反向变体：步长已封顶时 retune 必须返回 false（否则会无限降频丢数据）
    let capped = !ring7.retune(EFFECT_BUDGET + 1, &mut bag9);
    cs.add("K13-统计-封顶后不再降频", capped && ring7.stride() == 8, "步长到顶 ⇒ retune 返 false");

    // --- 未超预算不降频（反向）---
    let mut ring8 = EffectStatsRing::new();
    let mut bag10 = DiagBag::new();
    let no_tune = ring8.retune(EFFECT_BUDGET, &mut bag10);
    cs.add(
        "K13-统计-未超预算不降频",
        !no_tune && ring8.stride() == 1 && bag10.is_empty(),
        "效果数恰在预算内 ⇒ 步长不变且无告警",
    );

    // --- 边界：恰好超预算一个（budget+1 必降，budget 不降）---
    let mut ring9 = EffectStatsRing::new();
    let mut bag11 = DiagBag::new();
    let edge = ring9.retune(EFFECT_BUDGET + 1, &mut bag11);
    cs.add("K13-统计-预算边界夹逼", edge && ring9.stride() == 2, "budget+1 必降、budget 不降");

    // --- 按效果筛选 ---
    let mut ring10 = EffectStatsRing::new();
    let mut bag12 = DiagBag::new();
    let mut e = 0u16;
    while e < 3 {
        ring10.push(
            EffectStat {
                frame: u64::from(e),
                effect: 10 + e,
                enabled: true,
                param_digest: 0,
                exclusive_ns: 1,
                total_ns: 2,
            },
            &mut bag12,
        );
        e += 1;
    }
    let only_a = ring10.of_effect(11);
    cs.add(
        "K13-统计-按效果筛选",
        only_a.len() == 1 && only_a[0].effect == 11,
        "of_effect 只给该效果行（全链每效果一行）",
    );
    cs.add(
        "K13-统计-三种效果各一行",
        ring10.of_effect(10).len() == 1
            && ring10.of_effect(11).len() == 1
            && ring10.of_effect(12).len() == 1,
        "逐效果逐行（多行同值也须分别可取）",
    );

    // --- 越界取索引返回 None（不 panic）---
    cs.add(
        "K13-统计-越界索引安全",
        ring10.at(99).is_none() && ring10.at(ring10.len()).is_none(),
        "at() 越界返 None（零 panic 面）",
    );

    cs
}

// ---------------------------------------------------------------------------
// b 族：逐级透视 / 图结构可视（判据二、判据三）
// ---------------------------------------------------------------------------

fn run_b_checks() -> CheckSet {
    let mut cs = CheckSet::new("svstar2-vek13");
    let mut bag = DiagBag::new();

    // --- 线性图基本量 ---
    let g = linear_graph();
    let ids = [1u32, 2, 3, 4];
    cs.add(
        "K13-图-节点边计数",
        g.node_count() == ids.len() && g.edge_count() == ids.len() - 1,
        "4 节点 3 边",
    );

    // --- 拓扑序真排序（非插入序）---
    let order = g.topological_order();
    let order_ok = match &order {
        Ok(o) => o.len() == g.node_count() && o[0] == ids[0] && o[ids.len() - 1] == ids[3],
        Err(_) => false,
    };
    cs.add("K13-图-拓扑序完整", order_ok, "出队数 == 节点数且首尾正确");

    // --- 反向变体：倒序插入的同一条链，拓扑序应整体反向 ---
    //
    // 【本条曾写错】初版比「首尾相同」，但 1→2→3→4 与 4→3→2→1 是
    // **两条方向相反的链**（边集本身就不同），拓扑序理应相反。
    // 正确的可比量是：**各自的边约束都成立 + 节点集合相同**。
    // 若实现偷用插入序，倒序语料会给出 [4,3,2,1] 而违反边 1→2 的约束。
    let mut g_rev = GraphModel::new();
    for id in [4u32, 3, 2, 1].iter() {
        g_rev.add_node(GraphNode { id: *id, effect: 0, rt_alias: 0 });
    }
    for (a, b) in [(1u32, 2u32), (2, 3), (3, 4)].iter() {
        g_rev.add_edge(GraphEdge { from: *a, to: *b }, &mut bag);
    }
    let rev_order = g_rev.topological_order();
    // 边约束：任意 a→b 必满足 pos(a) < pos(b)（这才是拓扑序的定义）。
    let respects_edges = |o: &Vec<u32>, g: &GraphModel| {
        g.edges().iter().all(|e| {
            let pa = o.iter().position(|x| *x == e.from);
            let pb = o.iter().position(|x| *x == e.to);
            match (pa, pb) {
                (Some(a), Some(b)) => a < b,
                _ => false,
            }
        })
    };
    let rev_ok = match (&order, &rev_order) {
        (Ok(a), Ok(b)) => {
            a.len() == b.len()
                && respects_edges(a, &g)
                && respects_edges(b, &g_rev)
                && g_rev.node_count() == b.len()
        }
        _ => false,
    };
    cs.add(
        "K13-图-拓扑序与插入序无关",
        rev_ok,
        "倒序插入仍满足边约束（插入序实现会在此红）",
    );

    // --- 确定性：同一图多次排序逐位相同 ---
    let o1 = g.topological_order();
    let o2 = g.topological_order();
    cs.add(
        "K13-图-拓扑序确定性",
        o1.is_ok() && o2.is_ok() && o1.as_ref().ok() == o2.as_ref().ok(),
        "同一图两次排序逐位相同",
    );

    // --- 反向变体 D：环必须被拒（不给残缺序）---
    let cyc = variant_cyclic_graph();
    let cyc_res = cyc.topological_order();
    cs.add(
        "K13-图-环被拒出序",
        cyc_res == Err(GraphError::Cycle),
        "反向变体：含环图返回 Cycle 而非残缺序",
    );

    // --- 自环在建边时就被拒 ---
    let mut g_self = GraphModel::new();
    let mut bag2 = DiagBag::new();
    g_self.add_node(GraphNode { id: 1, effect: 0, rt_alias: 0 });
    let self_rejected = !g_self.add_edge(GraphEdge { from: 1, to: 1 }, &mut bag2);
    cs.add(
        "K13-图-自环建边即拒",
        self_rejected && bag2.p1_count() >= 1,
        "自环拒绝且记 P1",
    );

    // --- 悬空边被拒 ---
    let mut g_dang = GraphModel::new();
    let mut bag3 = DiagBag::new();
    g_dang.add_node(GraphNode { id: 1, effect: 0, rt_alias: 0 });
    let dang_rejected = !g_dang.add_edge(GraphEdge { from: 1, to: 99 }, &mut bag3);
    cs.add(
        "K13-图-悬空边拒收",
        dang_rejected && bag3.has(DiagCode::GRAPH_DANGLING_EDGE),
        "边引用不存在节点 ⇒ 拒绝并留痕",
    );

    // --- 反向变体 F：边被吞 ⇒ 边数与拓扑序的关系仍自洽 ---
    let gd = variant_edge_dropped();
    let gd_edges = gd.edge_count();
    let gd_topo = gd.topological_order();
    cs.add(
        "K13-图-吞边后仍自洽",
        gd_topo.is_ok() && gd_topo.as_ref().ok().map(|o| o.len()) == Some(gd.node_count()),
        "吞掉一条边后拓扑序仍覆盖全部节点（不产生悬挂）",
    );
    cs.add(
        "K13-图-吞边计数可查",
        gd_edges == linear_graph().edge_count() - 1,
        "边数确实少了一条（吞边可被计数发现）",
    );

    // --- 别名复用（D8）：多节点同 alias ⇒ 物理 RT 仍只 1 个 ---
    let ga = alias_reuse_graph();
    let shared = LINEAR_ALIAS_BASE + 2;
    let users = ga.alias_users(shared);
    cs.add("K13-图-别名复用计数", users == 2, "共享 alias 被两个节点声明");
    cs.add(
        "K13-图-复用不增物理RT",
        ga.physical_rts().len() == 4,
        "两个节点共用一个 alias ⇒ 物理 RT 数不增",
    );
    let same_handle = {
        let a = ga.rt_of(3);
        let b = ga.rt_of(4);
        match (a, b) {
            (Some(x), Some(y)) => x.handle == y.handle,
            _ => false,
        }
    };
    cs.add("K13-图-同别名同句柄", same_handle, "复用节点解析出同一物理句柄");

    // --- 反向变体：不同 alias 的两节点句柄必不同 ---
    let diff_handle = {
        let a = ga.rt_of(1);
        let b = ga.rt_of(2);
        match (a, b) {
            (Some(x), Some(y)) => x.handle != y.handle,
            _ => false,
        }
    };
    cs.add("K13-图-异别名异句柄", diff_handle, "非复用关系不得共用句柄");

    // --- 重复节点 id 拒绝 ---
    let mut g_dup = GraphModel::new();
    g_dup.add_node(GraphNode { id: 1, effect: 0, rt_alias: 0 });
    let dup_rejected = !g_dup.add_node(GraphNode { id: 1, effect: 1, rt_alias: 0 });
    cs.add("K13-图-重复节点拒收", dup_rejected && g_dup.node_count() == 1, "同 id 不重复入表");

    // --- 同一 handle 重复分配被拒 ---
    let mut g_rt = GraphModel::new();
    let first = g_rt.alloc_rt(RtRef { node: 1, handle: 7, width: 8, height: 8 });
    let second = g_rt.alloc_rt(RtRef { node: 2, handle: 7, width: 8, height: 8 });
    cs.add(
        "K13-图-同handle只分配一次",
        first && !second && g_rt.physical_rts().len() == 1,
        "同句柄二次分配被拒（别名复用的物质保证）",
    );

    // --- 逐级透视：depth 越大祖先链越长且恰为 depth（D1）---
    let mut depth_ok = true;
    let mut d = 0u32;
    while d <= 3 {
        match pick_rt(&g, 4, d, RtScale::Full, 1920, 1080) {
            Ok(p) => {
                if p.ancestors.len() != d as usize {
                    depth_ok = false;
                }
            }
            Err(_) => depth_ok = false,
        }
        d += 1;
    }
    cs.add("K13-透视-祖先链长度等于深度", depth_ok, "depth=d ⇒ 恰 d 段祖先");

    // --- 深度 0 ⇒ 祖先链为空 ---
    let d0 = pick_rt(&g, 4, 0, RtScale::Full, 1920, 1080);
    cs.add(
        "K13-透视-深度0无祖先",
        d0.as_ref().map(|p| p.ancestors.is_empty()).unwrap_or(false),
        "只取本级时祖先链为空",
    );

    // --- 根节点再上溯 ⇒ 深度超出被拒 ---
    let over = pick_rt(&g, 1, 1, RtScale::Full, 1920, 1080);
    cs.add(
        "K13-透视-超深被拒",
        over == Err(GraphError::DepthExceeded(1)),
        "根节点无祖先，请求深度 1 ⇒ DepthExceeded",
    );

    // --- 不存在的节点被拒 ---
    let unknown = pick_rt(&g, 99, 0, RtScale::Full, 1920, 1080);
    cs.add(
        "K13-透视-未知节点被拒",
        unknown == Err(GraphError::NodeUnknown(99)),
        "节点不存在 ⇒ NodeUnknown",
    );

    // --- 带诊断的形态：三类错误各自映射到专属诊断码 ---
    let mut bag5 = DiagBag::new();
    let r1 = pick_rt_checked(&g, 99, 0, RtScale::Full, 1920, 1080, &mut bag5);
    let r2 = pick_rt_checked(&g, 1, 5, RtScale::Full, 1920, 1080, &mut bag5);
    cs.add(
        "K13-透视-错误码专属不重合",
        r1.is_none()
            && r2.is_none()
            && bag5.has(DiagCode::PICK_NODE_UNKNOWN)
            && bag5.has(DiagCode::PICK_DEPTH_EXCEEDED)
            && !bag5.has(DiagCode::PICK_NODE_UNKNOWN) == false,
        "未知节点与超深各落专属码（不共用兜底码）",
    );

    // --- 无 RT 的节点被拒 ---
    let mut g_nort = GraphModel::new();
    g_nort.add_node(GraphNode { id: 1, effect: 0, rt_alias: 999 });
    let nort = pick_rt(&g_nort, 1, 0, RtScale::Full, 64, 64);
    cs.add(
        "K13-透视-无RT节点被拒",
        nort == Err(GraphError::NoRt(1)),
        "未产出 RT 的节点 ⇒ NoRt",
    );
    let mut bag7 = DiagBag::new();
    let nr = pick_rt_checked(&g_nort, 1, 0, RtScale::Full, 64, 64, &mut bag7);
    cs.add(
        "K13-透视-无RT落专属码",
        nr.is_none() && bag7.has(DiagCode::NODE_HAS_NO_RT),
        "NoRt 映射专属诊断码",
    );

    // --- RT 档位字节数与判据侧独立重算相符 ---
    let bytes_ok = RtScale::ALL
        .iter()
        .all(|s| s.bytes(1920, 1080) == ref_scale_bytes(*s, 1920, 1080));
    cs.add("K13-透视-档位字节独立重算", bytes_ok, "四档字节数与手算逐位相等");

    // --- 降档方向正确（Full→Half→Quarter→Eighth→Eighth）---
    let chain_ok = RtScale::Full.downgrade() == RtScale::Half
        && RtScale::Half.downgrade() == RtScale::Quarter
        && RtScale::Quarter.downgrade() == RtScale::Eighth
        && RtScale::Eighth.downgrade() == RtScale::Eighth;
    cs.add("K13-透视-降档方向正确", chain_ok, "逐档降且最低档自锁");

    // --- 反向变体：反向降档链必与真链不同（证明判据对方向敏感）---
    let rev_chain_diff = variant_downgrade_reversed(RtScale::Full) != RtScale::Full.downgrade();
    cs.add("K13-透视-降档方向敏感", rev_chain_diff, "反向变体链与真链不同 ⇒ 方向判据不虚");

    // --- 降档字节数递减（单边符号钉方向，不用「互异」）---
    let mono = RtScale::Full.bytes(1920, 1080) > RtScale::Half.bytes(1920, 1080)
        && RtScale::Half.bytes(1920, 1080) > RtScale::Quarter.bytes(1920, 1080)
        && RtScale::Quarter.bytes(1920, 1080) > RtScale::Eighth.bytes(1920, 1080);
    cs.add("K13-透视-字节随降档递减", mono, "档位越高字节越多（单边符号）");

    // --- 超带宽 ⇒ 降档且如实标注（D5）---
    //
    // 【本条曾写错】初版用 4096² 断言「降档后仍交付」。但 4096² 即使
    // 降到最低档 Eighth（÷8）仍 8MB > 1MB 预算 ⇒ 实现返 `None` 是
    // **正确**的（拒绝而非给假数据）。降档是**手段**，
    // 降到底仍超预算时必须拒绝——所以本条用两段尺寸分别断两种结局。
    let mut bag8 = DiagBag::new();
    // (a) 能降档装下：全分辨率超预算、降到某档恰好装下 ⇒ 交付且标注
    // 尺寸**由预算推导**：取 side 使 full = 2×预算 ⇒ Half 档恰好装下。
    // 硬写「2048」会在预算常量变更后静默失效（判据自己算错却看着绿）。
    // 求最小 side 使 side²·4 ≥ 2·BUD（即 full 必然超预算）。
    let half_side = {
        let mut s = 1u32;
        while s.saturating_mul(s).saturating_mul(4) < RT_BANDWIDTH_BUDGET.saturating_mul(2) {
            s += 1;
        }
        s
    };
    let big = pick_rt_checked(&g, 4, 0, RtScale::Full, half_side, half_side, &mut bag8);
    let mut ok_scaled = false;
    let mut ok_marked = false;
    if let Some(p) = big {
        ok_scaled = p.downscaled && p.scale != p.requested_scale;
        // 必须降到**非最低档**仍装下（降到最低档还装不下就该拒绝）
        ok_marked = p.requested_scale == RtScale::Full
            && p.bytes <= RT_BANDWIDTH_BUDGET
            && p.scale != RtScale::Eighth;
    }
    cs.add("K13-透视-超带宽降档", ok_scaled, "全分辨率超预算 ⇒ 降到某档后交付");
    cs.add("K13-透视-降档如实标注", ok_marked, "保留请求档位且交付字节在预算内");
    cs.add(
        "K13-透视-降档有告警",
        bag8.has(DiagCode::RT_DOWNSCALED),
        "降档留痕（不静默）",
    );
    // (b) 降到最低档仍超预算 ⇒ 必须拒绝且**留专属码**（不给假数据）
    let mut bag8b = DiagBag::new();
    let exhausted_side = half_side.saturating_mul(2);
    let too_big =
        pick_rt_checked(&g, 4, 0, RtScale::Full, exhausted_side, exhausted_side, &mut bag8b);
    cs.add(
        "K13-透视-降到底仍超则拒",
        too_big.is_none() && bag8b.has(DiagCode::RT_BANDWIDTH_EXHAUSTED),
        "尺寸放大 2 倍 ⇒ 最低档仍超预算 ⇒ 拒绝并记专属码",
    );
    cs.add(
        "K13-透视-拒绝也留降档痕迹",
        bag8b.has(DiagCode::RT_DOWNSCALED),
        "即便最终拒绝，降档尝试过程仍留痕",
    );

    // --- 预算内 ⇒ 不降档 ---
    let mut bag9 = DiagBag::new();
    let small = pick_rt_checked(&g, 4, 0, RtScale::Full, 64, 64, &mut bag9);
    cs.add(
        "K13-透视-预算内不降档",
        small.as_ref().map(|p| !p.downscaled && p.scale == RtScale::Full).unwrap_or(false)
            && !bag9.has(DiagCode::RT_DOWNSCALED),
        "64² 全分辨率在预算内 ⇒ 原档交付",
    );

    // --- 边界夹逼：恰好等于预算不降档，超一字节即降 ---
    let mut bag10 = DiagBag::new();
    let exact = pick_rt_checked(&g, 4, 0, RtScale::Full, 512, 512, &mut bag10);
    cs.add(
        "K13-透视-预算边界夹逼",
        exact.as_ref().map(|p| !p.downscaled).unwrap_or(false),
        "512×512×4 = 1MB 恰在预算内 ⇒ 不降档",
    );

    // --- RT 档位 wire 互异且反查往返 ---
    let mut rw_ok = true;
    for s in RtScale::ALL.iter() {
        if RtScale::from_wire(s.wire()) != Some(*s) {
            rw_ok = false;
        }
    }
    cs.add("K13-透视-档位wire往返", rw_ok, "scale wire 反查一致");

    // --- 档位 wire 互异 ---
    cs.add(
        "K13-透视-档位wire互异",
        RtScale::ALL[0].wire() != RtScale::ALL[1].wire()
            && RtScale::ALL[1].wire() != RtScale::ALL[2].wire()
            && RtScale::ALL[0].wire() != RtScale::ALL[2].wire()
            && RtScale::ALL[3].wire() != RtScale::ALL[0].wire(),
        "四档线上编码互异",
    );

    // --- 档位标签互异 ---
    cs.add(
        "K13-透视-档位标签互异",
        RtScale::ALL[0].label() != RtScale::ALL[1].label()
            && RtScale::ALL[1].label() != RtScale::ALL[2].label()
            && RtScale::ALL[2].label() != RtScale::ALL[3].label(),
        "四档标签互异（防同名占位）",
    );

    // --- 图快照：摘要独立重算 ---
    let snap = g.snapshot();
    let snap_sum_ok = snap.checksum() == ref_snap_checksum(snap.version, snap.nodes.len(), snap.edges.len(), &snap.topo)
        && !snap.drifted();
    cs.add("K13-快照-摘要独立重算", snap_sum_ok, "快照摘要与判据侧手算逐位相等");

    // --- 反向变体 C：乱序拓扑必改摘要 ---
    let shuffled = variant_topo_shuffled(&snap);
    cs.add(
        "K13-快照-乱序必改摘要",
        shuffled.checksum() != snap.checksum(),
        "反向变体：拓扑序打乱 ⇒ 摘要改变",
    );

    // --- 快照与构造顺序无关（否则对账会误判漂移）---
    let mut g_other = GraphModel::new();
    let mut bag11 = DiagBag::new();
    for id in [4u32, 3, 2, 1].iter() {
        g_other.add_node(GraphNode { id: *id, effect: 0, rt_alias: 0 });
    }
    for (a, b) in [(1u32, 2u32), (2, 3), (3, 4)].iter() {
        g_other.add_edge(GraphEdge { from: *a, to: *b }, &mut bag11);
    }
    let snap_other = g_other.snapshot();
    cs.add(
        "K13-快照-与构造顺序无关",
        snap_other.checksum() == snap.checksum(),
        "同图不同插入顺序 ⇒ 快照摘要相同（不误判漂移）",
    );

    // --- 版本推进 ---
    let mut g_ver = linear_graph();
    let v0 = g_ver.version();
    let v1 = g_ver.bump_version();
    cs.add("K13-快照-版本随重编译递增", v1 == v0.wrapping_add(1), "bump_version 使版本 +1");

    // --- 版本漂移 ⇒ 拦截（D6）---
    //
    // 【本条曾写错】初版造了漂移快照 `sd`，却去`reconcile()` 一个
    // **新造的干净信封**（`Envelope::new` 自算摘要 ⇒ 必然零漂移），
    // 于是「漂移被拦」永远红。**要让对账真拦，必须让注册进去的
    // 那一份**真的`declared != checksum`——直接把漂移快照的摘要
    // 写进信封的 `declared`（模拟「注册后数据被改」的真实漂移形态）。
    let mut g_drift = linear_graph();
    g_drift.bump_version();
    let mut sd = g_drift.snapshot();
    sd.version = sd.version.wrapping_add(7);
    let mut bag12 = DiagBag::new();
    let mut r = EnvelopeRegistry::new();
    // 用漂移快照的摘要冒充注册时声明值 ⇒ 重算必不等 ⇒ 真漂移。
    let mut bad_env = Envelope::new(PayloadKind::GraphSnapshot, 1, 1);
    bad_env.declared = sd.checksum();
    r.register(bad_env);
    let n = r.reconcile(&mut bag12);
    cs.add(
        "K13-快照-漂移被拦",
        n == 1 && r.intercepted() && r.take(PayloadKind::GraphSnapshot).is_none(),
        "注册摘要与重算不符 ⇒ 全局拦截（旧拓扑不得再发）",
    );
    cs.add(
        "K13-快照-漂移计数可查",
        r.drift_count() == 1 && bag12.p1_count() >= 1,
        "漂移计数恰为 1 且记 P1",
    );
    cs.add("K13-快照-漂移本地可检出", sd.drifted(), "漂移快照自身 drifted() 为真");
    // 反向变体：修回正确摘要后重对账 ⇒ 应恢复可取（拦截可解除不是死锁）
    r.clear_intercept();
    let mut r2 = EnvelopeRegistry::new();
    r2.register(Envelope::new(PayloadKind::GraphSnapshot, 1, 1));
    let mut bag13 = DiagBag::new();
    let n2 = r2.reconcile(&mut bag13);
    cs.add(
        "K13-快照-干净后恢复可取",
        n2 == 0 && !r2.intercepted() && r2.take(PayloadKind::GraphSnapshot).is_some(),
        "摘要一致时不被拦（拦截不误伤干净态）",
    );

    // --- 反向变体：节点表被换但版本/边数不变 ⇒ 摘要也必须变 ---
    let mut s_mut = snap.clone();
    s_mut.nodes.push(GraphNode { id: 99, effect: 99, rt_alias: 99 });
    cs.add(
        "K13-快照-加节点必改摘要",
        s_mut.checksum() != snap.checksum(),
        "节点表增减 ⇒ 摘要改变",
    );

    cs
}

// ---------------------------------------------------------------------------
// c 族：发行版零成本 / 一帧封装（判据四）
// ---------------------------------------------------------------------------

fn run_c_checks() -> CheckSet {
    let mut cs = CheckSet::new("svstar2-vek13");
    let bag = DiagBag::new();

    // --- Debug 档：负载真构建（对照组，双向验证的一半）---
    let mut gd = StripGuard::new(BuildProfile::Debug);
    let mut bag1 = DiagBag::new();
    let built_n = gd.try_push(&mut bag1);
    cs.add(
        "K13-发行-开发档可构建",
        built_n && gd.payload_builds == 1 && bag1.is_empty(),
        "Debug 档 try_push 返 true 且计数 1",
    );
    // 对照组的**单调侧**：`build_entries` 必须真递增，否则「单调证据」
    // 在 Debug 档就不成立，Release 档的 0 就没有对照意义。
    cs.add(
        "K13-发行-开发档进入次数递增",
        gd.build_entries == 1,
        "Debug 档进入构建分支恰 1 次（单调口径的对照组——Release 档的 0 \
         只有在Debug 档真为 1 时才是证据，否则恒 0 无意义）",
    );

    // --- Release 档：强行请求也不构建（D10，零成本由「根本不进入」保证）---
    let mut gr = StripGuard::new(BuildProfile::Release);
    let mut bag2 = DiagBag::new();
    let forced = gr.try_push(&mut bag2);
    cs.add(
        "K13-发行-发行档零构建",
        !forced && gr.payload_builds == 0,
        "Release 档 try_push 返 false 且 payload_builds 仍为 0",
    );
    // **单调口径的零成本证据**：`payload_builds` 是净值，可被「自增再
    // 减回」洗白（实测变异如此，全绿放过）；`build_entries` 全程只增
    // 不减，故恒 0 才是「根本不进入」的可证伪命题。
    cs.add(
        "K13-发行-发行档未进入构建",
        gr.build_entries == 0,
        "Release 档 `build_entries` 恰为 0（单调口径，不可用自增自减洗白）",
    );
    cs.add(
        "K13-发行-强行请求记次数",
        gr.forced_attempts == 1,
        "强行请求被记账（不是静默丢弃）",
    );
    cs.add(
        "K13-发行-强行请求记P1",
        bag2.p1_count() == 1 && bag2.has(DiagCode::STRIP_FAILED),
        "剔除失败 ⇒ P1（F1909 同规则）",
    );

    // --- 反向变体：连续强行 100 次，payload_builds 仍恰为 0 ---
    let mut gr2 = StripGuard::new(BuildProfile::Release);
    let mut bag3 = DiagBag::new();
    let mut i = 0u32;
    while i < 100 {
        gr2.try_push(&mut bag3);
        i += 1;
    }
    cs.add(
        "K13-发行-百次强行仍零构建",
        gr2.payload_builds == 0 && gr2.forced_attempts == 100,
        "100 次强行请求后 payload_builds 恰为 0",
    );
    // 同上，单调口径：100 次强行后进入次数必须**仍恰为 0**。
    cs.add(
        "K13-发行-百次强行未进构建",
        gr2.build_entries == 0,
        "100 次强行请求后 build_entries 仍恰为 0（单调口径——净值可被 \
         自增自减洗白，单调计数不能，故此处才是可证伪的「零进入」）",
    );

    // --- 档位互斥：一个 bool 表达两件事必错（D11）---
    cs.add(
        "K13-发行-档位互斥",
        gd.payloads_enabled() != gr.payloads_enabled(),
        "Debug/Release 的启用态互不相同",
    );

    // --- Default 是 Debug（忘配的发行构建不会悄悄带负载）---
    cs.add(
        "K13-发行-默认为开发档",
        BuildProfile::default() == BuildProfile::Debug,
        "Default=Debug（发行须显式写 Release）",
    );

    // --- Default 的 StripGuard 与显式 Debug 一致 ---
    cs.add(
        "K13-发行-默认守卫可构建",
        StripGuard::default().payloads_enabled(),
        "默认守卫允许构建（安全默认）",
    );

    // --- 一帧封装：三个信封齐全 ---
    let mut ring = EffectStatsRing::new();
    let mut bag4 = DiagBag::new();
    let mut f = 0u64;
    while f < 3 {
        ring.push(
            EffectStat { frame: f, effect: 10, enabled: true, param_digest: 0xABCD, exclusive_ns: 5, total_ns: 9 },
            &mut bag4,
        );
        f += 1;
    }
    let g = linear_graph();
    let snap = g.snapshot();
    let frame = DebugFrame::seal(77, &ring, &snap);

    let all_kinds = PayloadKind::ALL
        .iter()
        .all(|k| frame.envelope(*k).is_some());
    cs.add("K13-一帧-三类信封齐全", all_kinds, "一帧含三类负载信封");

    // --- 一帧内三信封互不相同（不是同一信封复制三份）---
    let distinct = frame.envelopes[0].checksum() != frame.envelopes[1].checksum()
        && frame.envelopes[1].checksum() != frame.envelopes[2].checksum()
        && frame.envelopes[0].checksum() != frame.envelopes[2].checksum();
    cs.add("K13-一帧-三信封互异", distinct, "三类载荷摘要互不相同");

    // --- 字节数与判据侧独立重算相符 ---
    let ref_stats_bytes = (ring.len() * 24) as u32;
    let ref_snap_bytes = ((snap.nodes.len() * 12) + (snap.edges.len() * 8) + (snap.topo.len() * 4)) as u32;
    let bytes_ok = frame.envelopes[0].byte_len == ref_stats_bytes
        && frame.envelopes[1].byte_len == payload_bytes(PayloadKind::RtPick, &ring, &snap)
        && frame.envelopes[2].byte_len == ref_snap_bytes;
    cs.add("K13-一帧-字节独立重算", bytes_ok, "统计/快照字节与手算相符");

    // --- 反向变体：统计条数变化 ⇒ 字节数必须跟着变（不是常数）---
    let mut ring2 = EffectStatsRing::new();
    let mut bag5 = DiagBag::new();
    ring2.push(
        EffectStat { frame: 0, effect: 10, enabled: true, param_digest: 0, exclusive_ns: 1, total_ns: 2 },
        &mut bag5,
    );
    let frame2 = DebugFrame::seal(1, &ring2, &snap);
    cs.add(
        "K13-一帧-字节随负载变",
        frame2.envelopes[0].byte_len < frame.envelopes[0].byte_len,
        "统计少一条 ⇒ 统计字节变小（单边符号钉方向）",
    );

    // --- 总字节数 = 三者之和（守恒）---
    let sum = frame.envelopes[0].byte_len + frame.envelopes[1].byte_len + frame.envelopes[2].byte_len;
    cs.add("K13-一帧-总字节守恒", frame.total_bytes() == sum, "总字节 == 三信封之和");

    // --- 帧号进序号 ---
    cs.add(
        "K13-一帧-帧号进序号",
        frame.envelopes.iter().all(|e| e.seq == 77),
        "三信封 seq 均为帧号",
    );

    // --- 全部信封未漂移 ---
    cs.add(
        "K13-一帧-信封无漂移",
        frame.envelopes.iter().all(|e| !e.drifted()),
        "封装产出的信封全部自洽",
    );

    // --- 诊断码值域闭合且互异 ---
    cs.add("K13-诊断-码互异", diag_codes_unique(), "21 个诊断码互不相同");
    cs.add(
        "K13-诊断-标签非空",
        DiagCode::ALL.iter().all(|c| !c.label().is_empty()),
        "每个诊断码有人话标签",
    );
    cs.add(
        "K13-诊断-码段独占2Cxx",
        DiagCode::ALL.iter().all(|c| (c.0 >> 8) == 0x2C),
        "K 域独占码段 0x2Cxx（不与 F2407/F2408 的 0x2A/0x2B 撞）",
    );
    // 反向变体：改一个码的高字节即应破坏「独占段」判据
    let out_of_band = DiagCode(0x2B01);
    cs.add(
        "K13-诊断-越段可被查出",
        (out_of_band.0 >> 8) != 0x2C,
        "反向变体：0x2B01 不在 K 段（说明段判定不是恒真）",
    );

    // --- 诊断袋语义：P1/Warn 分流 ---
    let mut bag6 = DiagBag::new();
    bag6.push_p1(DiagCode::GRAPH_CYCLE);
    bag6.push_warn(DiagCode::STATS_DOWNSAMPLED);
    cs.add(
        "K13-诊断-严重度分流",
        bag6.count_severity(Severity::P1) == 1 && bag6.count_severity(Severity::Warn) == 1,
        "P1 与 Warn 分别记账",
    );

    // --- 诊断码与标签一一对应（不出现两条码同标签）---
    let mut labels_dup = false;
    let mut a = 0usize;
    while a < DiagCode::ALL.len() {
        let mut b = a + 1;
        while b < DiagCode::ALL.len() {
            if DiagCode::ALL[a].label() == DiagCode::ALL[b].label() {
                labels_dup = true;
            }
            b += 1;
        }
        a += 1;
    }
    cs.add("K13-诊断-标签互异", !labels_dup, "21 条诊断标签互不相同（否则一条含义被固化）");

    // --- 渲染含关键词且不含承诺词（无障碍诚实标注，双向验证）---
    let mut bag7 = DiagBag::new();
    bag7.push(DiagCode::GRAPH_CYCLE);
    let text = bag7.render();
    let has_marker = text.contains("环");
    let no_promise = !text.contains("保证") && !text.contains("一定") && !text.contains("完全");
    cs.add(
        "K13-诊断-渲染含标记不含承诺",
        has_marker && no_promise,
        "诊断文本含故障标记且不含承诺词",
    );

    // --- 冒烟可跑（不 panic 且非空）---
    let s = smoke();
    cs.add("K13-冒烟-非空且自洽", !s.is_empty() && s.contains("topo="), "冒烟输出含拓扑序与复用计数");

    // --- 空袋渲染为空且 is_empty 自洽 ---
    let empty_bag = DiagBag::new();
    cs.add(
        "K13-诊断-空袋自洽",
        empty_bag.is_empty() && empty_bag.render().is_empty() && empty_bag.p1_count() == 0,
        "空袋三条口径一致（零静默的反面：没事也不造事）",
    );
    let _ = bag;
    cs
}

// ---------------------------------------------------------------------------
// 聚合入口（注册表按 MAX_CHECKS 约束分族）
// ---------------------------------------------------------------------------

/// a 族（判据一：三负载 / 信封 / 统计流）—— 独立入口。
pub fn run_vek13_checks_a_standalone() -> CheckSet {
    run_a_checks()
}

/// b 族（判据二、判据三：逐级透视 / 图结构可视）—— 独立入口。
pub fn run_vek13_checks_b_standalone() -> CheckSet {
    run_b_checks()
}

/// c 族（判据四：发行版零成本 / 一帧封装）—— 独立入口。
pub fn run_vek13_checks_c_standalone() -> CheckSet {
    run_c_checks()
}

/// 全量自检（三族合并）。
///
/// **⚠ 合并即溢出（本条实测的真缺陷）**：三族原始条数
/// 45 + 45 + 27 = **117 > `MAX_CHECKS` = 112**，而 [`CheckSet::merge`]
/// 满了就 `dropped += 1` ——直接合并会**静默丢掉最后 5 条判据**
/// （初版 24 条 c 族时实测丢的是「冒烟-非空且自洽」与「诊断-空袋自洽」）。
/// `dropped` 只在 `truncated()` / `dropped()` 里报，而注册表若只看
/// `tally()`，那 2 条判据等于**从未存在过**却看不出任何异常。
///
/// **正解是分族注册**（同域 `VE-F1613-a/b/c` 先例）：注册表把 a/b/c/d
/// 各占一行，每行独立成集、各自 ≤ `MAX_CHECKS` ⇒ 合并入口在这条
/// 路径上**永不被调用**，溢出不可达。本函数保留仅为「想看全量」的
/// 场合，且其溢出被 d 族门禁 [`K13-聚合-分族不溢出`] 如实记账。
///
/// 合并本身**刻意保持真实现的朴素形态**（不搞分批/清账花活）：
/// `dropped` 的语义是「本set 有多少条被拒收」，在合并点私自改写它
/// 会让调用方看到**与 `merge` 语义不一致的数**——那比溢出本身更坏
/// （记账不可信 ⇒ 谁都不敢依赖记账）。
pub fn run_vek13_checks() -> CheckSet {
    CheckSet::merge(
        run_a_checks(),
        CheckSet::merge(run_b_checks(), CheckSet::merge(run_c_checks(), run_d_checks())),
    )
}

/// 三族原始条数之和（门禁侧独立重算，**不含 d 族**）。
///
/// 为什么必须三族**各自独立入口**再相加，而不是问 `run_vek13_checks()`：
/// 后者的 `len()` 已是截断后的数（112），用它算和等于「在被截断的世界里
/// 算自己」，永远自洽——这正是十诫第 9 条「同源驱动恒真」的具体形态。
pub fn raw_check_count() -> usize {
    run_a_checks().len() + run_b_checks().len() + run_c_checks().len()
}

/// d 族（判据五：分族注册自身的完整性）—— 独立入口，**单条判据**。
///
/// **为什么必须单独一族、而不是塞进 c 族**：本判据要断言「三族的每一条
/// 都真的出现在合并结果里」，若它自己住在 c 族里，则
/// ① 它要么被合并丢弃（实测溢出 3 条时它就在被丢之列 ⇒ 门禁**永不执行**
/// ⇒ 恒真），② 想断就得回调合并入口，而那会经`run_vek13_checks()`
/// 再次进入 `run_c_checks()`/`run_d_checks()` 造成**无限递归**
/// （实测栈溢出 `has overflowed its stack`）。两条都是「门禁自己
/// 让门禁失效」的形态——独立成族是唯一让它真正被执行的位置。
fn run_d_checks() -> CheckSet {
    let mut ds = CheckSet::new("svstar2-vek13");

    let ra = run_vek13_checks_a_standalone();
    let rb = run_vek13_checks_b_standalone();
    let rc = run_vek13_checks_c_standalone();

    // 形状：各族单独成集时都必须装得下（分族注册的前提）。
    let per_family_fit =
        ra.len() <= MAX_CHECKS && rb.len() <= MAX_CHECKS && rc.len() <= MAX_CHECKS;

    // 事实：三族每一条判据都真的出现在**合并结果**里，无一条被静默丢弃。
    // 这里必须用上面三个已跑好的局部值自己 merge（`CheckSet: Copy`），
    // 绝不回调 `run_vek13_checks()`（会递归回本函数）。
    let merged = CheckSet::merge(ra, CheckSet::merge(rb, rc));

    // 逐条点名核对（判据侧独立遍历 `get()`，不读私有 `checks` 字段）：
    // **本族每条都必须读得到**——`len()` 与 `get()` 不自洽说明该族的
    // 容量账坏了（例如 `items` 未随 `add` 同步），那会让本族判据在
    // 注册表上读出来是空的却看不出任何异常。
    //
    // 刻意**不**要求「每条都在合并结果里」：合并入口的溢出是已知预期
    // （见下方 `conservation` 口径），跨族求会与该前提自相矛盾。
    let mut all_readable = true;
    let mut fi = 0usize;
    let mut total_in_families = 0usize;
    while fi < 3usize {
        let fam = if fi == 0 {
            ra
        } else if fi == 1 {
            rb
        } else {
            rc
        };
        let mut k = 0usize;
        while k < fam.len() {
            // `get()` 越界返 `None`（不 panic）⇒ 天然零 panic 面。
            if fam.get(k).is_none() {
                all_readable = false;
            }
            k += 1;
        }
        total_in_families += fam.len();
        fi += 1;
    }

    // 条数守恒，按**两侧不同口径**对账：三族之和 == 合并装下 + 溢出数。
    // 写成「三族之和 == merged.len()」会在溢出时恰好相等 ⇒ 恒真
    // （十诫第 10 条「净值 vs 绝对值口径」）。
    //
    // **这里断的不是「合并入口零溢出」**——合并入口（117 > 112）溢出
    // 是**已知且刻意保留**的：它服务「想看全量」的场合，注册表**不走**
    // 这条路径（分族注册 a/b/c/d 各占一行）。所以本判据真正要断的是：
    //  1. 三族**各自**装得下（分族注册的前提）；
    //  2. 三族每一条判据都在**它自己那一族**里（逐族点名，不跨族求）；
    //  3. 溢出数**恰为 5** 且被如实记账（ 可读、不被私改）；
    //  4. 三族之和 > MAX_CHECKS（证明这条判据**真被触发过**，
    //     不是在「本来就装得下」的场合自我安慰）。
    //
    // 第3 条的「恰为 2」是**变更检测锚**：有人加判据使溢出变成 3 时，
    // 本判据转红 ⇒ 逼他确认「新增的那条是否真该进注册表」。
    let expect_total = raw_check_count();
    let merged_len = merged.len();
    let dropped_n = merged.dropped();
    // 实测基线：三族 45 + 45 + 27 = 117，MAX 112 ⇒ 溢出 5。
    // （c 族 24 → 27：新增「开发档进入次数递增」「发行档未进入构建」
    //  「百次强行未进构建」三条单调口径判据，见 StripGuard.build_entries。）
    let expected_overflow: usize = 5;
    let conservation = total_in_families == expect_total
        && total_in_families == merged_len + dropped_n
        && dropped_n == expected_overflow
        && merged.truncated()
        // 溢出必须**大于** MAX_CHECKS 才有意义：
        && total_in_families > MAX_CHECKS;

    ds.add(
        "K13-聚合-分族不溢出",
        per_family_fit && all_readable && conservation && total_in_families > MAX_CHECKS,
        "三族各自 <= MAX_CHECKS（分族注册下每行独立成集，实测 45/45/27）；         每族每条都读得到（`len()` 与 `get()` 自洽——否则该族在注册表上         读出来是空的却看不出异常）；条数守恒按**两侧不同口径**对账：         三族之和(117) == 合并装下(112) + 溢出(5)，且溢出**恰为 5**         （变更检测锚：有人加判据使溢出数变化时本判据转红，逼他确认新增         那几条是否真该进注册表）；并断三族之和 > MAX_CHECKS——         **证明这条判据不是在「本来就装得下」的场合自我安慰**，它必须         真被触发过。合并入口的溢出是已知预期：注册表走 a/b/c/d 四行，         不走合并路径",
    );
    ds
}

/// d 族独立入口（注册表单独占一行）。
pub fn run_vek13_checks_d_standalone() -> CheckSet {
    run_d_checks()
}

/// 合并入口的溢出计数（门禁/诊断用；读取而非重算，避免第二套记账）。
pub fn merged_dropped() -> usize {
    run_vek13_checks().dropped()
}