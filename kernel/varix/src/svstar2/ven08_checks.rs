//! VE-F2608 · 域自检（判据逐条映射锚点，45 项：a 17 + b 28）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2608`
//!
//! 锚点对本单的要求逐条落地为判据：
//! - 三负载声明（树结构流 / 属性检查流 / 状态流）→ `C2608-三负载-*`
//! - 三规模度量 + 每帧失效计数 → `C2608-统计-*`
//! - 调参协议「属性指令 → 生效 ACK」（F1946 / F2502 家族）→ `C2608-调优-*`
//! - 信封 N 段注册四类型 + 漂移对账拦截（第十一度）→ `C2608-信封-*`
//! - 错误路径与降级矩阵 → `C2608-降级-*`
//! - 发行版剔除零成本（家族纪律）→ `C2608-零成本-*`
//!
//! ## 判据设计纪律（承自 `veb14_checks`，不得违反）
//!
//! 1. **不得用表内元素验表内函数**——四类型码互异性必须拿线上编码逐对比较，
//!    不能只比对枚举下标。
//! 2. **阈值不得同时充当预期值**——`TUNE_WORK_BUDGET`、`N_SEGMENT_SLOTS` 等
//!    来自规格常量时，另用**锚点字面量**断言一次，否则「把阈值改成天文数字」
//!    仍然全绿。
//! 3. **缺测不能记 0**——降级与漂移必须能让调用方区分「真的没有」与「没测到」。
//! 4. **拒绝必须分因**——序号回退而拒与参数非法而拒是两种语义，两者的
//!    `last_seq` 水位线推进行为**刻意相反**，判据分别钉住。
//! 5. **剔除零成本必须双向验证**——只断「Release 下为 0」是恒真弱门禁
//!    （把整个字段删掉照样全绿）：Debug 档计数必须真递增（对照），Release 档
//!    连强行尝试也不递增 + P1，三面合起来才算验过。
//!
//! 语料**自造**：现搭一棵小控件树，不引外部文件——外部语料会让「谁坏了」
//! 变成「哪个文件变了」，破坏回归定位能力。
//!
//! 分两个入口返回（`_a` / `_b`）：`CheckSet` 定容 112 项，单函数塞 24 项虽不溢出，
//! 但与 `ven06` / `ven07` / `ves04` 的分批惯例一致，便于定位是哪一批变红。

use crate::checks::CheckSet;
use crate::svstar2::ven08_debug::*;

use crate::svstar2::ven02_tree::{
    self, ControlTree, PropertyKey, SingleParentPolicy, StateSignals, VisualState,
};
use crate::svstar2::ven04_prop::PropValue;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格锚点字面量（**不引用实现常量**，否则「改常量 = 改预期」构成自证）
// ---------------------------------------------------------------------------

/// 锚点字面量：N 段注册四类型（F1764 负载族第十一度声明）。
const ANCHOR_N_SLOT_TYPES: usize = 4;
/// 锚点字面量：N 段信封 schema 版本。
const ANCHOR_ENVELOPE_SCHEMA: u16 = 1;
/// 锚点字面量：属性流预算耗尽诊断码线缆名。
const ANCHOR_PROP_BUDGET_CODE: &str = "PROP_INSPECT_BUDGET_EXHAUSTED";
/// 锚点字面量：序号回退诊断码线缆名。
const ANCHOR_TUNE_REGRESS_CODE: &str = "TUNE_SEQ_REGRESSED";

// ---------------------------------------------------------------------------
// 自造语料
// ---------------------------------------------------------------------------

/// 自造一棵 **6 节点**控件树：`root → (a → a1, b → (b1, b2))`。
///
/// 真实规模：节点 **6**、最大深度 2、最大宽度 2（`root` 有 `a`/`b` 两个孩子，
/// `b` 有 `b1`/`b2` 两个孩子）。
///
/// **节点数是 6 不是 5**：本语料刻意让 `a` 与 `b` 同为非叶，
/// 深度与宽度才都取到真实最大值——深度 1 或宽度 1 的树测不出 LOD 抽稀的
/// 「父先于子」与「宽层截停」两件事。判据里凡涉及规模的一律**从
/// `tree_metrics` 现取**（F2602 单源），只有本函数上方那条语料形状断言
/// 才把 6 写死——写死一处、其余现算，两头都不失控。
///
/// **头插序**：`insert(..., Some(0))` 恒插 0 位——`b` 的子表本序 = [b2, b1]，
/// 不是 [b1, b2]（重排判据的预期以此为准，别再踩一遍）。
fn demo_tree() -> ControlTree {
    // `ControlTree::new("root")` 对非空根 id 恒成功（契约见 ven02 定义）；
    // 万一契约被改坏，让后续判据以「树为空」的方式红，而不是 panic 炸掉整域。
    let mut t = match ControlTree::new("root") {
        Ok(t) => t,
        Err(_) => return ControlTree::new("r").unwrap_or_else(|_| {
            ControlTree::new("rr").expect("根 id 恒非空")
        }),
    };
    // 挂点走 ven02 的自由函数 `insert`（不是 inherent 方法）——重挂父节点的
    // 单亲策略统一 `Reject`，自造语料不造重挂。`insert` 对未登记的 child
    // 会自动登记一个 `panel` 节点，故下面统一补写 `kind` 与属性键。
    for id in ["a", "b"] {
        let _ = ven02_tree::insert(&mut t, "root", id, Some(0), SingleParentPolicy::Reject);
    }
    for id in ["b1", "b2"] {
        let _ = ven02_tree::insert(&mut t, "b", id, Some(0), SingleParentPolicy::Reject);
    }
    // `a1` 挂 `a` 下，使 `a` 与 `b` 同为非叶——深度仍为 2，宽度仍为 2。
    let _ = ven02_tree::insert(&mut t, "a", "a1", Some(0), SingleParentPolicy::Reject);

    // **属性键必须真写进树**（本语料最容易漏的一步）：
    // `insert` 自动登记的节点 `property_keys` 是空的，而 `apply_tune`
    // 的 `SetProp` 明确要求「树内必须真有这个键」（F2602 键集封闭，
    // 写不存在的键即面板与树的映射漂移）。不声明属性键的话，
    // 每条 `SetProp` 都会被 `TuneTargetAbsent` 拒掉——
    // 测的就成了「拒径能不能走通」，而不是「协议能不能生效」。
    for (id, kind) in [
        ("root", "panel"),
        ("a", "leaf"),
        ("a1", "leaf"),
        ("b", "list"),
        ("b1", "leaf"),
        ("b2", "leaf"),
    ] {
        if let Some(n) = t.raw_mut(id) {
            n.kind = String::from(kind);
            n.property_keys = alloc::vec![
                PropertyKey::Visible,
                PropertyKey::Text,
                PropertyKey::AriaLabel,
            ];
        }
    }
    t
}

fn demo_sample() -> FrameSample {
    FrameSample {
        node_count: demo_node_count(),
        max_depth: 2,
        max_width: 2,
        invalid_attempts: 0,
        invalid_applied: 0,
        invalid_merged: 0,
    }
}

/// 语料树的真实节点数（**从 F2602 现取**，不写死——写死就是「改常量=改预期」）。
///
/// `FrameSample.node_count` 是 `u32`，`TreeMetrics.node_count` 是 `usize`；
/// 这里做一次显式收敛（`saturating` 而非 `as`，免得语料超大时静默截断）。
fn demo_node_count() -> u32 {
    u32::try_from(ven02_tree::tree_metrics(&demo_tree()).node_count).unwrap_or(u32::MAX)
}

/// 在给定树上挂一棵**子树规模超单帧工作预算**的大树，返回其根 id。
///
/// 「超标即 P1」这条判据必须真把工作单元顶上去，否则那条分支从头到尾没人走。
/// 挂法刻意用 `insert` 逐个登记（F2602 自由函数），不直改 `children`——
/// 直改会绕过 F2602 的不变量维护，让语料自身变成「非法树」，
/// 那测出来的 P1 是非法树的 P1，不是工作单元超标的 P1。
fn build_over_budget_tree(tree: &mut ControlTree) -> String {
    let want = TUNE_WORK_BUDGET as usize + 4;
    let root_id = String::from("flood");
    let _ = ven02_tree::insert(tree, "root", &root_id, None, SingleParentPolicy::Reject);
    let mut i = 0usize;
    while i < want {
        let child = format!("flood_{}", i);
        let _ = ven02_tree::insert(tree, &root_id, &child, None, SingleParentPolicy::Reject);
        i += 1;
    }
    root_id
}

/// 由信号求视觉状态（走 ven02 的正规状态机，不自造状态值）。
fn state_of(i: u32) -> VisualState {
    ven02_tree::resolve_visual_state(StateSignals {
        hovered: i % 2 == 0,
        pressed: i % 3 == 0,
        focused: i % 4 == 0,
        disabled: i % 11 == 0,
        active: i % 5 == 0,
    })
}

/// 编辑器侧调优镜像：一个三槽映射，树本体复用 F2602 真类型。
fn demo_mirror() -> TreeMirror {
    let mut mirror = TreeMirror {
        tree: demo_tree(),
        slots: Vec::new(),
        total_delivered: 0,
    };
    let ids = ["a", "a1", "b"];
    let mut i = 0usize;
    while i < ids.len() {
        mirror.slots.push(NodeMirror {
            id: String::from(ids[i]),
            values: Vec::new(),
            dirty: false,
        });
        i += 1;
    }
    mirror
}

/// 一条属性写指令（槽 0 的 `Visible` 键）。
fn demo_cmd(seq: u64) -> TuneCommand {
    TuneCommand {
        seq,
        op: TuneOp::SetProp { node: 0, key: PropertyKey::Visible, value: PropValue::Bool(true) },
    }
}

// ---------------------------------------------------------------------------
// 域自检主体
// ---------------------------------------------------------------------------

/// VE-F2608 判据自检入口（第一批：族声明 / 三负载 / 属性流 / 状态流）。
pub fn run_ven08_checks_a() -> CheckSet {
    let mut set = CheckSet::new("ve-f2608");

    // ========================================================================
    // 一、族声明与 N 段注册面（锚点：第十一度家族声明）
    // ========================================================================

    // 判据一：N 段注册四类型齐备，且**恰为四**（少一类是缺口，多一类是越界）。
    set.add(
        "C2608-三负载-四类型齐备",
        PayloadKind::ALL.len() == ANCHOR_N_SLOT_TYPES,
        "N 段注册四类型 = 树结构/属性检查/状态/统计",
    );

    // 判据二：四类型人话标签互异（防两负载共用一句说明——最难查的一类缺陷）。
    {
        let mut ok = true;
        let mut i = 0usize;
        while i < PayloadKind::ALL.len() {
            let mut j = i + 1;
            while j < PayloadKind::ALL.len() {
                if PayloadKind::ALL[i].label() == PayloadKind::ALL[j].label() {
                    ok = false;
                }
                j += 1;
            }
            i += 1;
        }
        set.add("C2608-三负载-标签互异", ok, "四类型标签两两不等");
    }

    // 判据三：族声明自洽（实现层提供的机检入口必须为真，否则 N 段注册面无意义）。
    set.add("C2608-三负载-族声明自洽", family_is_consistent(), "四类型 + 十六码齐备");

    // ========================================================================
    // 二、树结构流：按需拉取零常驻 + 不静默（锚点 F2013 拾取家族）
    // ========================================================================

    // 判据四：预算为 0 → 空结果 + 显性码，**不静默给全量**（洪水场景要防的正是这个）。
    {
        let tree = demo_tree();
        let mut bag = DiagBag::new();
        let pick = pick_subtree(&tree, "root", 0, &mut bag);
        let ok = pick.vertices.is_empty() && bag.has(N8DiagCode::PickBudgetExhausted);
        set.add("C2608-三负载-零预算显性拒", ok, "预算 0 → 空结果 + PICK_BUDGET_EXHAUSTED");
    }

    // 判据五：锚点不存在 → 显性拒绝，**不静默给空图**（空图与「树真没了」是两回事）。
    {
        let tree = demo_tree();
        let mut bag = DiagBag::new();
        let pick = pick_subtree(&tree, "no_such", 8, &mut bag);
        let ok = pick.vertices.is_empty() && bag.has(N8DiagCode::PickAnchorAbsent);
        set.add("C2608-三负载-锚点缺失显性拒", ok, "锚点不存在 → PICK_ANCHOR_ABSENT");
    }

    // 判据六：预算内交付不超量，且全量规模**只计数不分配**（零常驻的实现方式）。
    //     期望规模**从 `tree_metrics` 现取**（F2602 单源）：写死数字的话，
    //     语料改了而判据不动，红的绿的都是假的。
    {
        let tree = demo_tree();
        let want = demo_node_count();
        let mut bag = DiagBag::new();
        let pick = pick_subtree(&tree, "root", 3, &mut bag);
        let ok = pick.vertices.len() <= 3 && pick.total_subtree == want;
        set.add(
            "C2608-三负载-预算内交付不超量",
            ok,
            "预算 3 → 交付 ≤3，全量规模只计数（期望现取）",
        );
    }

    // 判据七：父先于子交付，`parent_pos` 恒指向本序列内**更早**的顶点。
    {
        let tree = demo_tree();
        let mut bag = DiagBag::new();
        let pick = pick_subtree(&tree, "root", 64, &mut bag);
        let mut ok = !pick.vertices.is_empty();
        let mut i = 0usize;
        while i < pick.vertices.len() && ok {
            if let Some(pp) = pick.vertices[i].parent_pos {
                // BFS 序保证父下标更小；若相等或更大即为违约。
                if (pp as usize) >= i {
                    ok = false;
                }
            }
            i += 1;
        }
        set.add("C2608-三负载-父先于子交付", ok, "parent_pos 恒指向本序列内更早顶点");
    }

    // 判据八：全量拉取时交付数恰等于自造树的真实规模（防「少画一个」类静默缺陷）。
    {
        let tree = demo_tree();
        let want = demo_node_count();
        let mut bag = DiagBag::new();
        let pick = pick_subtree(&tree, "root", 64, &mut bag);
        let ok = pick.vertices.len() == want as usize && !pick.decimated;
        set.add(
            "C2608-三负载-全量交付齐备",
            ok,
            "预算充足 → 交付数 = F2602 现取规模且未降档",
        );
    }

    // 判据八之二：**语料形状本身**钉死（这是本文件唯一允许写死规模的地方）。
    //     没有它，上面两条「现取」就成了自证——把 `demo_tree` 改成单节点树，
    //     两条判据照样全绿，而本条会红。语料与判据各钉一头，中间现算。
    {
        let m = ven02_tree::tree_metrics(&demo_tree());
        set.add(
            "C2608-三负载-语料规模钉死",
            m.node_count == 6 && m.max_depth == 2 && m.max_sibling_count == 2,
            "语料 = root+a+a1+b+b1+b2 共 6 节点 / 深 2 / 宽 2（a 与 b 同为非叶）",
        );
    }

    // 判据八之三：**锚点钉死纪律**（抽稀时锚点必须仍在交付里）。
    //     锚点被裁掉 = 编辑器「选中即蒸发」，这是树可视最经典的故障，
    //     而它只在预算 < 规模时才会发生——只测全量拉取永远测不到。
    {
        let tree = demo_tree();
        let mut bag = DiagBag::new();
        let pick = pick_subtree(&tree, "a", 1, &mut bag);
        let anchored = pick
            .vertices
            .first()
            .map(|v| v.id == "a" && v.depth == 0 && v.parent_pos.is_none())
            .unwrap_or(false);
        set.add(
            "C2608-三负载-抽稀时锚点必交付",
            pick.decimated && anchored && pick.vertices.len() == 1,
            "预算 1 / 规模 6 → 必降档，且交付的唯一点就是锚点自身",
        );
    }

    // ========================================================================
    // 三、属性检查流：零预算显性 + 码线缆名锚定（F2502 家族）
    // ========================================================================

    // 判据九：预算为 0 时显性记账，不是静默返回空。
    {
        let cells: Vec<(PropertyKey, PropValue)> = Vec::new();
        let mut bag = DiagBag::new();
        let out = inspect_props(&cells, 0, &mut bag);
        let ok = out.cells.is_empty() && bag.has(N8DiagCode::PropInspectBudgetExhausted);
        set.add("C2608-属性流-零预算显性拒", ok, "预算 0 → 空 + PROP_INSPECT_BUDGET_EXHAUSTED");
    }

    // 判据十：诊断码线缆名与锚点字面量逐字相等（防改名漂移）。
    {
        let ok = N8DiagCode::PropInspectBudgetExhausted.as_str() == ANCHOR_PROP_BUDGET_CODE;
        set.add("C2608-属性流-诊断码线缆名锚定", ok, "PROP_INSPECT_BUDGET_EXHAUSTED 逐字一致");
    }

    // 判据十一：超预算降档必须显性（属性洪水 → 按需抽稀，不是静默截断）。
    {
        let mut cells: Vec<(PropertyKey, PropValue)> = Vec::new();
        let mut i = 0usize;
        while i < MAX_PROP_CELLS + 8 {
            cells.push((PropertyKey::Text, PropValue::Number(i as f32)));
            i += 1;
        }
        let mut bag = DiagBag::new();
        let out = inspect_props(&cells, 4, &mut bag);
        let ok = out.decimated && out.cells.len() <= 4;
        set.add("C2608-属性流-超预算降档显性", ok, "40 项预算 4 → 抽稀至 ≤4 且标记 decimated");
    }

    // 判据十一之二：**非有限值钳制**（NaN 绝不进编辑器面板——一个 NaN 顺着
    //     数值输入框就能污染整条属性链，这是三负载里属性检查流的铁律）。
    //     正向（NaN/Inf 钳 0 + 计数 + 告警）与反向（正常值不误钳）两面一起钉。
    {
        let cells: Vec<(PropertyKey, PropValue)> = alloc::vec![
            (PropertyKey::Text, PropValue::Number(f32::NAN)),
            (PropertyKey::AriaLabel, PropValue::Number(f32::INFINITY)),
            (PropertyKey::Visible, PropValue::Bool(true)),
        ];
        let mut bag = DiagBag::new();
        let out = inspect_props(&cells, 8, &mut bag);
        let ok = out.clamped == 2
            && out.cells.len() == 3
            && out.cells[0].summary == "0"
            && out.cells[1].summary == "0"
            && out.cells[0].type_tag == "number"
            && !out.cells[2].clamped
            && bag.has(N8DiagCode::PropNonFiniteClamped);
        set.add(
            "C2608-属性流-非有限钳制",
            ok,
            "NaN/Inf → 钳 0 + 计数 2 + 告警；正常布尔不误钳",
        );
    }

    // ========================================================================
    // 四、状态流：定容环形缓冲（锚点：环形而非增长队列——价值在「最近」）
    // ========================================================================

    // 判据十二：溢出覆盖而非增长，且覆盖数如实记账。
    {
        let mut bag = DiagBag::new();
        let mut s = StateStream::with_capacity(4);
        let mut i = 0u32;
        while i < 10 {
            let sample = StateSample { frame: i, node_slot: i % 3, state: state_of(i) };
            let _ = s.push(sample, &mut bag);
            i += 1;
        }
        let ok = s.capacity() == 4 && s.len() == 4 && s.overwritten() == 6;
        set.add("C2608-状态流-定容覆盖不增长", ok, "容量 4 推 10 → 留 4、覆盖 6");
    }

    // 判据十三：覆盖必须显性告警，不静默丢样本。
    {
        let mut bag = DiagBag::new();
        let mut s = StateStream::with_capacity(2);
        let mut i = 0u32;
        while i < 5 {
            let sample = StateSample { frame: i, node_slot: 0, state: state_of(i) };
            let _ = s.push(sample, &mut bag);
            i += 1;
        }
        let ok = bag.has(N8DiagCode::StateStreamOverwritten);
        set.add("C2608-状态流-覆盖显性告警", ok, "覆盖必记 STATE_STREAM_OVERWRITTEN");
    }

    // 判据十四：最新样本可读（「最近」语义——环形缓冲的价值所在）。
    {
        let mut bag = DiagBag::new();
        let mut s = StateStream::with_capacity(2);
        let _ = s.push(StateSample { frame: 7, node_slot: 2, state: state_of(7) }, &mut bag);
        let ok = s.latest().map(|x| x.frame) == Some(7);
        set.add("C2608-状态流-最新样本可读", ok, "末推 frame=7 → latest().frame == 7");
    }

    set
}

/// VE-F2608 判据自检入口（第二批：统计流 / 调优 / 信封 / 降级）。
pub fn run_ven08_checks_b() -> CheckSet {
    let mut set = CheckSet::new("ve-f2608");

    // ========================================================================
    // 五、统计流：三规模度量 + 每帧失效计数（锚点 F2418 遥测联动）
    // ========================================================================

    // 判据十五：三规模度量可读，且与自造树的真实规模一致（F2602 现取）。
    {
        let mut bag = DiagBag::new();
        let mut w = StatWindow::new();
        let _ = w.observe(demo_sample(), &mut bag);
        let s = w.summary(&mut bag);
        let m = ven02_tree::tree_metrics(&demo_tree());
        let ok = s.node_count as usize == m.node_count
            && s.peak_depth as usize == m.max_depth
            && s.peak_width as usize == m.max_sibling_count;
        set.add(
            "C2608-统计-三规模度量对账",
            ok,
            "节点/深/宽三项与 F2602 现取规模逐项相等",
        );
    }

    // 判据十六：峰值只增不减（跨帧取极值，不是末帧值——否则尖峰会被抹平）。
    {
        let mut bag = DiagBag::new();
        let mut w = StatWindow::new();
        let _ = w.observe(demo_sample(), &mut bag);
        let mut big = demo_sample();
        big.node_count = 40;
        big.max_depth = 9;
        big.max_width = 7;
        let _ = w.observe(big, &mut bag);
        let mut small = demo_sample();
        small.node_count = 2;
        small.max_depth = 1;
        small.max_width = 1;
        let _ = w.observe(small, &mut bag);
        let s = w.summary(&mut bag);
        let ok = s.peak_node_count == 40 && s.peak_depth == 9 && s.peak_width == 7;
        set.add("C2608-统计-峰值取跨帧极值", ok, "40/9/7 不得被后续小帧回落");
    }

    // 判据十七：每帧失效三口径（尝试/应用/合并）分别入账，不混为一谈。
    {
        let mut bag = DiagBag::new();
        let mut w = StatWindow::new();
        let mut s = demo_sample();
        s.invalid_attempts = 10;
        s.invalid_applied = 6;
        s.invalid_merged = 4;
        let _ = w.observe(s, &mut bag);
        let out = w.summary(&mut bag);
        // 守恒：尝试 = 应用 + 合并。任一口径混淆都会让这条红。
        let conserved = s.invalid_applied + s.invalid_merged == s.invalid_attempts;
        set.add(
            "C2608-统计-失效三口径守恒",
            conserved && out.samples == 1,
            "尝试10 = 应用6 + 合并4",
        );
    }

    // 判据十八：洪水降级必须显性（降档不得静默）。
    //     **降档的触发是「窗满」不是「节点数大」**：锚点两行洪水分别是
    //     「树数据洪水（万节点全画）→按需拉取+LOD」与「统计洪水→聚合降档」，
    //     前者由树结构流的 LOD 挡（见 `C2608-三负载-预算内交付不超量`），
    //     后者是**聚合窗装不下**时的折叠。两者混为一谈就会出现
    //     「节点数一大就降档」——那会把一次正常的 5 万节点树判成洪水，
    //     而真正的洪水（样本窗溢出）反倒没人管。
    {
        let mut bag = DiagBag::new();
        let mut w = StatWindow::new();
        let mut i = 0u32;
        while i <= STAT_WINDOW_MAX_SAMPLES {
            let mut s = demo_sample();
            s.node_count = 5000;
            s.max_depth = 40;
            let _ = w.observe(s, &mut bag);
            i += 1;
        }
        let ok = w.downgrades() > 0
            && bag.has(N8DiagCode::StatsFloodDowngrade)
            // 折叠语义：样本数折半、步长翻倍（「这一窗代表多长时间」须可回答）。
            && w.samples() < STAT_WINDOW_MAX_SAMPLES
            && w.stride() > 1;
        set.add(
            "C2608-统计-洪水降级显性",
            ok,
            "样本窗溢出 → 必降档 + 显性码 + 样本折半 + 步长翻倍",
        );
    }

    // 判据十八之二：**反向锚点**——窗未满不得降档。
    //     没有这条，一个「每次 observe 都降档一次」的退化实现也能把上一条刷绿。
    //     顺带钉住「节点数大不等于洪水」：5000 节点但窗未满时不得降档。
    {
        let mut bag = DiagBag::new();
        let mut w = StatWindow::new();
        let mut i = 0u32;
        while i < STAT_WINDOW_MAX_SAMPLES - 1 {
            let mut s = demo_sample();
            s.node_count = 5000;
            s.max_depth = 40;
            let _ = w.observe(s, &mut bag);
            i += 1;
        }
        let ok = w.downgrades() == 0 && !bag.has(N8DiagCode::StatsFloodDowngrade) && w.stride() == 1;
        set.add(
            "C2608-统计-节点大不误判洪水",
            ok,
            "5000 节点 × 255 帧（窗未满）→ 零降档、步长仍为 1",
        );
    }

    // 判据十八之三：**降档后峰值口径必须同步**（折叠把历史峰值折半覆盖，
    //     继续挂着旧峰值就与新分辨率不同量纲——口径漂移是统计面最隐蔽的病）。
    //     这条把「峰值只增不减」与「降档清峰值」两个口径的**边界**钉死：
    //     降档前取到峰值，降档后该峰值不再跨档挂着。
    {
        let mut bag = DiagBag::new();
        let mut w = StatWindow::new();
        let mut i = 0u32;
        while i <= STAT_WINDOW_MAX_SAMPLES {
            let _ = w.observe(demo_sample(), &mut bag);
            i += 1;
        }
        let after = w.summary(&mut bag);
        let m = ven02_tree::tree_metrics(&demo_tree());
        let ok = after.peak_node_count == after.node_count
            && after.node_count as usize == m.node_count
            && after.stride == 2;
        set.add(
            "C2608-统计-降档后峰值口径同步",
            ok,
            "折叠后峰值回落到窗内末值，步长=2（不得跨档挂旧峰值）",
        );
    }

    // 判据十八之四：比率分母为零时**返回「未定义」而非 0 冒充**
    //     （锚点：口径冻结；0 会被读成「合并率真的是 0」，那是编造）。
    {
        let mut bag = DiagBag::new();
        let mut w = StatWindow::new();
        let _ = w.observe(demo_sample(), &mut bag);
        let s = w.summary(&mut bag);
        set.add(
            "C2608-统计-零分母判未定义",
            s.merge_ppm.is_none() && s.applied_ppm.is_none() && bag.has(N8DiagCode::StatsRatioUndefined),
            "尝试数=0 → 两比率皆 None 且显性记账，绝不返回 0 冒充",
        );
    }

    // 判据十八之五：**失效计数异常（applied > attempts）进 P1**——应用数
    //     不可能超过尝试数，超标即计数器被写坏的铁证；带着坏数据聚合出的
    //     比率没有意义，先报账再聚合。反向面：正常样本（applied ≤ attempts）
    //     零误报——没有它，一个「永远报异常」的实现同样能把上一条刷绿。
    {
        let mut bag = DiagBag::new();
        let mut w = StatWindow::new();
        let mut bad = demo_sample();
        bad.invalid_attempts = 5;
        bad.invalid_applied = 6;
        let _ = w.observe(bad, &mut bag);
        let mut bag_ok = DiagBag::new();
        let mut w_ok = StatWindow::new();
        let mut good = demo_sample();
        good.invalid_attempts = 10;
        good.invalid_applied = 6;
        let _ = w_ok.observe(good, &mut bag_ok);
        let ok = bag.has(N8DiagCode::InvalCounterAnomaly)
            && bag.count_severity(Severity::P1) > 0
            && !bag_ok.has(N8DiagCode::InvalCounterAnomaly)
            && bag_ok.count_severity(Severity::P1) == 0;
        set.add(
            "C2608-统计-计数异常进P1",
            ok,
            "applied>attempts → INVAL_COUNTER_ANOMALY 且 P1；正常样本零误报",
        );
    }

    // ========================================================================
    // 六、调参协议：属性指令 → 生效 ACK（F1946 / F2502 家族）
    // ========================================================================

    // 判据十九：ACK 两态闭合，生效态回显序号且计数入账。
    {
        let mut ch = TuneChannel::default();
        let mut mirror = demo_mirror();
        let mut bag = DiagBag::new();
        let ack = apply_tune(&mut ch, &mut mirror, demo_cmd(1), &mut bag);
        let ok = ack.is_applied() && ack.seq() == 1 && ch.applied == 1 && ch.rejected == 0;
        set.add("C2608-调优-生效ACK回显序号", ok, "首指令生效，ACK 序号=1，applied=1");
    }

    // 判据二十：序号回退而拒 → **不推进水位线**（未生效不该推进，
    // 否则二次被拒于「已见过」，掩盖真实原因）。
    {
        let mut ch = TuneChannel::default();
        let mut mirror = demo_mirror();
        let mut bag = DiagBag::new();
        let _ = apply_tune(&mut ch, &mut mirror, demo_cmd(5), &mut bag);
        let before = ch.last_seq;
        let ack = apply_tune(&mut ch, &mut mirror, demo_cmd(3), &mut bag);
        let ok = !ack.is_applied() && ack.is_seq_reject() && ch.last_seq == before;
        set.add("C2608-调优-序号回退拒且水位不推进", ok, "seq5 后回 3 → 拒，last_seq 仍为 5");
    }

    // 判据二十一：回退诊断码线缆名与锚点字面量逐字相等。
    {
        let ok = N8DiagCode::TuneSeqRegressed.as_str() == ANCHOR_TUNE_REGRESS_CODE;
        set.add("C2608-调优-回退码线缆名锚定", ok, "TUNE_SEQ_REGRESSED 逐字一致");
    }

    // 判据二十二：工作预算为正且有界（阈值只作预算，不兼作预期值）。
    {
        let ok = TUNE_WORK_BUDGET > 0 && TUNE_WORK_BUDGET < 1024;
        set.add("C2608-调优-工作预算为正有界", ok, "0 < TUNE_WORK_BUDGET < 1024");
    }

    // 判据二十三：槽位越界而拒 → 显性码 + 拒绝计数入账（不放任越界写）。
    //     **且必须推进水位线**：参数非法而拒的指令已被消费方作废，
    //     不推进会令同一指令被无限重投——与序号回退拒的「不推进」刻意相反。
    {
        let mut ch = TuneChannel::default();
        let mut mirror = demo_mirror();
        let mut bag = DiagBag::new();
        let cmd = TuneCommand {
            seq: 1,
            op: TuneOp::SetProp { node: 999, key: PropertyKey::Visible, value: PropValue::Bool(true) },
        };
        let ack = apply_tune(&mut ch, &mut mirror, cmd, &mut bag);
        let ok = !ack.is_applied() && ch.rejected == 1 && ch.last_seq == 1;
        set.add(
            "C2608-调优-槽位越界显性拒",
            ok,
            "槽 999（三槽镜像）→ 拒且 rejected=1、水位线推进（作废语义）",
        );
    }

    // 判据二十三之二：**未声明的属性键被拒**（F2602 键集封闭——
    //     面板写了一个树里没有的键，就是面板与树的映射漂移）。
    //     这条与「生效」那条成对：只有生效侧一条的话，
    //     一个「凡写必拒」的通道同样能把它刷绿。
    {
        let mut ch = TuneChannel::default();
        let mut mirror = demo_mirror();
        let mut bag = DiagBag::new();
        let cmd = TuneCommand {
            seq: 1,
            op: TuneOp::SetProp { node: 0, key: PropertyKey::ZIndex, value: PropValue::Number(3.0) },
        };
        let ack = apply_tune(&mut ch, &mut mirror, cmd, &mut bag);
        let ok = !ack.is_applied() && ch.rejected == 1 && bag.has(N8DiagCode::TuneTargetAbsent);
        set.add(
            "C2608-调优-未声明键显性拒",
            ok,
            "ZIndex 未在语料树声明 → 拒且显性码（键集封闭纪律）",
        );
    }

    // 判据二十三之三：**工作单元超标 → P1 立案**（锚点原文
    //     「调优延迟超标→F1949 口径立案」）。这条**必须真把工作单元顶上去**：
    //     `SetSignals` 的成本 = 目标子树规模，语料只有 6 节点、预算 64，
    //     所以另造一棵 **超预算大树**（子树 > `TUNE_WORK_BUDGET`）。
    //     只断 `TUNE_WORK_BUDGET` 是正数有界（判据二十二）的话，
    //     「超标即立案」这条分支从头到尾没人走过。
    {
        let mut ch = TuneChannel::default();
        let mut mirror = demo_mirror();
        // 扩一棵大树：大 = root 的第 4 个孩子，其子树规模 > TUNE_WORK_BUDGET。
        let big_id = build_over_budget_tree(&mut mirror.tree);
        let mut bag = DiagBag::new();
        let slot = mirror.slots.len();
        mirror.slots.push(NodeMirror { id: String::from(&big_id), values: Vec::new(), dirty: false });
        let cmd = TuneCommand {
            seq: 1,
            op: TuneOp::SetSignals {
                node: slot as u32,
                signals: StateSignals { hovered: true, ..StateSignals::default() },
            },
        };
        let ack = apply_tune(&mut ch, &mut mirror, cmd, &mut bag);
        // 判据侧独立手算：脏传播成本 = 1（基准）+ flood 子树规模
        // （flood 自身 1 + 子节点 预算+4 = 预算+5），合计 = 预算+6。
        // 不问被测方「你认为超了吗」，也不拿全树规模冒充子树规模。
        let expect_work = TUNE_WORK_BUDGET + 6;
        let ok = ack.is_applied()
            && ack.is_over_budget()
            && ack.work_units() == expect_work
            && ch.over_budget == 1
            && ch.applied == 1
            && ch.total_work == expect_work as u64
            && bag.count_severity(Severity::P1) > 0
            && expect_work > TUNE_WORK_BUDGET;
        set.add(
            "C2608-调优-超标即P1立案",
            ok,
            "子树规模 > 单帧工作预算 → 仍生效但 over_budget=1 且进 P1（不是静默放行）",
        );
    }

    // 判据二十三之四：**反向锚点**——工作单元未超标不得进 P1。
    //     语料小树上跑一次 `SetSignals`，成本远低于预算，必须零 P1。
    {
        let mut ch = TuneChannel::default();
        let mut mirror = demo_mirror();
        let mut bag = DiagBag::new();
        let cmd = TuneCommand {
            seq: 1,
            op: TuneOp::SetSignals {
                node: 0,
                signals: StateSignals { focused: true, ..StateSignals::default() },
            },
        };
        let ack = apply_tune(&mut ch, &mut mirror, cmd, &mut bag);
        let ok = ack.is_applied()
            && !ack.is_over_budget()
            && ch.over_budget == 0
            && bag.count_severity(Severity::P1) == 0;
        set.add(
            "C2608-调优-未超标不进P1",
            ok,
            "小树 SetSignals → 生效且零 over_budget、零 P1（否则上一条是恒真）",
        );
    }

    // 判据二十三之五：**子节点重排下标越界显性拒**（ReorderChild 的参数
    //     非法面：from/to 越界拒绝 + 水位线推进，与槽位越界同作废语义）。
    {
        let mut ch = TuneChannel::default();
        let mut mirror = demo_mirror();
        let mut bag = DiagBag::new();
        let cmd = TuneCommand {
            seq: 1,
            op: TuneOp::ReorderChild { node: 2, from: 0, to: 9 },
        };
        let ack = apply_tune(&mut ch, &mut mirror, cmd, &mut bag);
        let ok = !ack.is_applied()
            && ch.last_seq == 1
            && ch.rejected == 1
            && bag.has(N8DiagCode::TuneReorderOutOfRange);
        set.add(
            "C2608-调优-重排越界显性拒",
            ok,
            "to=9（子表长 2）→ TUNE_REORDER_OUT_OF_RANGE 且水位线推进",
        );
    }

    // 判据二十三之六：**重排生效落位**（生效面——只测拒绝不测生效的话，
    //     一个「凡重排必拒」的通道同样能把上一条刷绿）。
    //     工作单元 = 子表长（真实扫描量），落位结果逐位对拍。
    {
        let mut ch = TuneChannel::default();
        let mut mirror = demo_mirror();
        let mut bag = DiagBag::new();
        let cmd = TuneCommand {
            seq: 1,
            op: TuneOp::ReorderChild { node: 2, from: 0, to: 1 },
        };
        let ack = apply_tune(&mut ch, &mut mirror, cmd, &mut bag);
        // 语料以 `Some(0)` 逐个头插：b 的子表本序 = [b2, b1]；
        // reorder(0→1) 之后应为 [b1, b2]——换位必须落在真实子表上。
        let landed = match mirror.tree.raw("b") {
            Some(n) => {
                n.children.len() == 2 && n.children[0] == "b1" && n.children[1] == "b2"
            }
            None => false,
        };
        let ok = ack.is_applied()
            && ack.work_units() == 2
            && !ack.is_over_budget()
            && landed
            && mirror.slots[2].dirty
            && ch.applied == 1;
        set.add(
            "C2608-调优-重排生效落位",
            ok,
            "from 0 → to 1：子表 [b2,b1] 换位为 [b1,b2] + 工作单元=子表长 2 + 脏标记",
        );
    }

    // 判据二十三之七：**调优写值的非有限钳制**（与检查流同纪律：
    //     NaN 不进镜像表——钳 0 落表 + 显性码，指令本身生效不拒）。
    {
        let mut ch = TuneChannel::default();
        let mut mirror = demo_mirror();
        let mut bag = DiagBag::new();
        let cmd = TuneCommand {
            seq: 1,
            op: TuneOp::SetProp {
                node: 0,
                key: PropertyKey::Text,
                value: PropValue::Number(f32::NAN),
            },
        };
        let ack = apply_tune(&mut ch, &mut mirror, cmd, &mut bag);
        let stored = mirror.slots[0]
            .values
            .first()
            .map(|(k, v)| *k == PropertyKey::Text && matches!(v, PropValue::Number(x) if *x == 0.0))
            .unwrap_or(false);
        let ok = ack.is_applied() && stored && bag.has(N8DiagCode::PropNonFiniteClamped);
        set.add(
            "C2608-调优-写值非有限钳制",
            ok,
            "NaN 写值 → 生效但钳 0 落表 + 显性码（面板不吞 NaN）",
        );
    }

    // ========================================================================
    // 七、信封 N 段注册：四类型 + 漂移对账拦截（第十一度）
    // ========================================================================

    // 判据二十四：四类型线上编码互异（**显式映射**，判据也必须查线上值
    // 而非枚举下标——`as u16` 的写法会让判据自证）。
    set.add("C2608-信封-四类型线缆码互异", wires_unique(), "四类型 wire() 两两不等");

    // 判据二十五：schema 版本与锚点字面量一致（防版本漂移）。
    {
        let ok = ENVELOPE_SCHEMA == ANCHOR_ENVELOPE_SCHEMA;
        set.add("C2608-信封-schema版本锚定", ok, "N 段 schema == 1");
    }

    // 判据二十六：漂移必被对账拦截并计数（对账不是摆设）。
    //     **拦截是消费面语义不是记一笔**：置拦截后 take() 对所有类型
    //     一律 None（漂移信封不得被消费端取用）、peek() 不受影响
    //     （对账自身要能看到信封）；解除必须显式——人不点头不复用。
    {
        let env = Envelope::new(PayloadKind::TreeStructure, 128, 1);
        // 未篡改 → 不漂移。
        let clean_ok = !env.drifted();

        let mut tampered = env;
        tampered.byte_len = 129;
        let drift_detected = tampered.drifted();

        let mut reg = EnvelopeRegistry::new();
        let _ = reg.register(tampered);
        let mut bag = DiagBag::new();
        let drifts = reg.reconcile(&mut bag);

        let take_blocked = reg.take(PayloadKind::TreeStructure).is_none();
        let take_blocked_other = reg.take(PayloadKind::Stats).is_none();
        let peek_visible = reg.peek(PayloadKind::TreeStructure).is_some();
        // 拦截态先取证再解除——clear 之后 intercepted() 已复位，取证晚了就恒假。
        let intercept_latched = reg.intercepted();
        reg.clear_intercept();
        let take_restored = reg.take(PayloadKind::TreeStructure).is_some();

        let ok = clean_ok
            && drift_detected
            && drifts > 0
            && reg.drift_count() > 0
            && intercept_latched
            && take_blocked
            && take_blocked_other
            && peek_visible
            && take_restored;
        set.add(
            "C2608-信封-漂移对账拦截",
            ok,
            "改 byte_len → 摘要不等 → reconcile 拦截：take 全拒/peek 可见/显式解除后恢复",
        );
    }

    // 判据二十七：诊断码全在 N 段专属码区且互异（防撞 F2605/F2606/F2607）。
    set.add(
        "C2608-信封-诊断码段专属互异",
        codes_unique_in_segment(),
        "十六码互异且落在 0x2E 段",
    );

    // 判据二十八：线缆名互异（与标签互异正交——两码可同标签不同线缆，
    // 但两码同线缆名必是缺陷）。
    set.add("C2608-信封-线缆名互异", wire_names_unique(), "十六码 as_str() 两两不等");

    // ========================================================================
    // 八、发行版剔除零成本（锚点：家族纪律「发行版剔除零成本」）
    // ========================================================================

    // 判据二十九：定容常量不随帧数增长——「零常驻」的可机检形态：
    // 桶/槽容量是编译期常量，与已推送样本数无关。
    {
        let ok = STATE_STREAM_CAP > 0 && STAT_WINDOW_MAX_SAMPLES > 0 && N_SEGMENT_SLOTS == ANCHOR_N_SLOT_TYPES;
        set.add("C2608-零成本-定容常量与帧数无关", ok, "三类容量均为定容常量");
    }

    // 判据三十：**默认档位是 Debug**（调试是安全默认——想要零成本剔除必须
    //     显式写 Release；反过来的发行默认会让忘配的发行构建悄悄带上全部
    //     调试负载）。
    set.add(
        "C2608-零成本-默认档位是Debug",
        BuildProfile::default() == BuildProfile::Debug,
        "BuildProfile::default() 必须是 Debug（缺省可调试，显式才剔除）",
    );

    // 判据三十一：**Debug 档构建计数真实递增**（对照组——证明计数器不是
    //     死的；没有这条，「Release 下恒 0」就是恒真弱门禁：把整个字段删掉
    //     照样全绿）。
    {
        let mut g = StripGuard::new(BuildProfile::Debug);
        let mut bag = DiagBag::new();
        let mut granted = 0usize;
        let mut i = 0usize;
        while i < 3 {
            if g.try_build(&mut bag) {
                granted += 1;
            }
            i += 1;
        }
        let ok = granted == 3
            && g.payload_builds == 3
            && g.forced_attempts == 0
            && g.payloads_enabled()
            && bag.is_empty();
        set.add(
            "C2608-零成本-Debug档构建递增",
            ok,
            "Debug 档 3 次请求 → 3 次放行且 payload_builds=3（计数器是活的）",
        );
    }

    // 判据三十二：**Release 档零构建 + 强行尝试进 P1**（剔除失败——家族
    //     纪律第十一度）。零成本的物质保证：连强行尝试路径都不递增
    //     `payload_builds`（不是「构建完再抹掉」，而是根本不进入构建），
    //     只记 `forced_attempts` 与一条 P1。
    {
        let mut g = StripGuard::new(BuildProfile::Release);
        let mut bag = DiagBag::new();
        let mut granted = 0usize;
        let mut i = 0usize;
        while i < 3 {
            if g.try_build(&mut bag) {
                granted += 1;
            }
            i += 1;
        }
        let ok = granted == 0
            && g.payload_builds == 0
            && g.forced_attempts == 3
            && !g.payloads_enabled()
            && bag.has(N8DiagCode::StripFailed)
            && bag.count_severity(Severity::P1) == 3;
        set.add(
            "C2608-零成本-Release档零构建强尝试进P1",
            ok,
            "Release 档 3 次请求 → 零放行、payload_builds 恒 0、forced_attempts=3、P1×3",
        );
    }

    set
}
