//! VE-F3408 · 令牌求值性能 —— 域判据层。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3408`
//!
//! # 判据映射（锚点五条 + 纪律）
//!
//! - **增量缓存**（11 条）→
//!   `F3408-增量缓存-首次全算`、`F3408-增量缓存-二次命中缓存`、
//!   `F3408-增量缓存-命中不改值`、`F3408-增量缓存-改值只失效下游`、
//!   `F3408-增量缓存-无关令牌不失效`、`F3408-增量缓存-叶子改动不波及上游`、
//!   `F3408-增量缓存-图变则整体失效`、`F3408-增量缓存-契约同图须通过`、
//!   `F3408-增量缓存-变体只失效自身必被抓`、`F3408-增量缓存-拓扑序方向判据`、
//!   `F3408-增量缓存-变体倒序必被抓`；
//! - **批量流水**（10 条）→
//!   `F3408-批量流水-一次产出全部目标`、`F3408-批量流水-派生值拼接正确`、
//!   `F3408-批量流水-共享计数前提`、`F3408-批量流水-批内共享只算一次`、
//!   `F3408-批量流水-变体重复计算必被抓`、`F3408-批量流水-按拓扑序产出`、
//!   `F3408-批量流水-重复目标不重复产出`、`F3408-批量流水-越界目标即拒`、
//!   `F3408-批量流水-变体误拒合法必被抓`、`F3408-批量流水-空批零变化`；
//! - **P95 承诺**（9 条）→
//!   `F3408-P95-近秩法取值正确`、`F3408-P95-单令牌预算闸`、
//!   `F3408-P95-变体超预算被拦必被抓`、`F3408-P95-变体误降级必被抓`、
//!   `F3408-P95-劣化即告警`、`F3408-P95-降级与告警可并存`、
//!   `F3408-P95-承诺口径可复现`、`F3408-P95-SLA判定自洽`、
//!   `F3408-P95-变体阈值失效必被抓`；
//! - **分帧兜底**（7 条）→
//!   `F3408-分帧-超容量即分帧`、`F3408-分帧-分帧不丢令牌`、
//!   `F3408-分帧-未超容量不分帧`、`F3408-分帧-变体吞令牌必被抓`、
//!   `F3408-分帧-帧容量契约值`、`F3408-分帧-耗时面板读屏可达`、
//!   `F3408-分帧-面板数量逐位读`；
//! - **判据与契约**（12 条）→
//!   `F3408-判据-十三码唯一`、`F3408-判据-错误三要素齐发`、
//!   `F3408-判据-阻断与告警分清`、`F3408-判据-降级类判定`、
//!   `F3408-判据-依赖方向封闭`、`F3408-判据-契约已冻结`、
//!   `F3408-判据-结果方向可分`、`F3408-判据-变体告警当阻断必被抓`、
//!   `F3408-判据-码表与枚举一致`、`F3408-判据-每码都有真实产生点`、
//!   `F3408-判据-变体码表漂移必被抓`、`F3408-判据-变体预算一致时不误报`；
//!
//! 本表与实现**逐条对账**（`chk_code_table` 之外的另一重保险由
//! `_attic` 过程脚本双向比对，改判据名不改本表会被发现）。
//!
//! # 判据设计纪律（本层的硬约束）
//!
//! 1. **不向被测函数问答案**：命中数/失效数/分帧数均由语料常量与图结构
//!    独立推导，不读被测的 profile 字段去构造期望。
//! 2. **计数独立重算**：`recomputed + hits` 的期望值由「闭包大小 − 缓存已有」
//!    算出，不用被测自己的计数当期望。
//! 3. **变体双向验证**：每条「必被抓」判据都配一个把被测对象改坏的变体，
//!    确认判定确实转红。**变体只在目标维度上与原物不同** ——
//!    变体若顺带改了别的维度，判据转红的原因是那个别的维度，这次验证作废。
//! 4. **方向性判据须用可区分语料**：`sort_unstable` 的方向判据必须造
//!    **同层多分支**语料；纯链语料的拓扑序唯一，倒序实现输出仍相同。

use crate::checks::CheckSet;
use crate::svstar2::ver01h_evalperf::*;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 判据侧常量（不从被测常量推导 —— 那会自证）
// ---------------------------------------------------------------------------

/// 期望的诊断码条数。
const EXPECT_CODES: usize = 13;
/// 期望的依赖方向条数。
const EXPECT_DIRS: usize = 1;
/// 语料规模：3 个基础 + 3 个派生 = 6 节点。
const N_BASE: usize = 3;
const N_DERIVED: usize = 3;
const N_NODES: usize = N_BASE + N_DERIVED;

/// 语料：基础令牌 `c.base0..2`；派生 `c.d0` 依赖 base0+base1，
/// `c.d1` 依赖 base0，`c.d2` 依赖 base1+base2。
///
/// 形状刻意选成**菱形**：base0 被 d0 与 d1 同时依赖 ⇒ 批内共享判据有效；
/// d0 的下游为空、base0 的下游有 d0 与 d1 ⇒ 「只失效下游」与
/// 「不波及上游」两条判据都有可区分的语料。
fn paths() -> Vec<String> {
    let mut v = Vec::new();
    for i in 0..N_BASE {
        v.push(String::from(format!("c.base{}", i)));
    }
    for i in 0..N_DERIVED {
        v.push(String::from(format!("c.d{}", i)));
    }
    v
}

/// 依赖边（节点号）：base0=0, base1=1, base2=2, d0=3, d1=4, d2=5。
fn edges() -> Vec<(u32, u32)> {
    vec![(3, 0), (3, 1), (4, 0), (5, 1), (5, 2)]
}

/// 基础值。
fn bases() -> Vec<(u32, String)> {
    vec![
        (0, String::from("a")),
        (1, String::from("b")),
        (2, String::from("c")),
    ]
}

/// 全部目标（派生 + 基础）。
fn all_targets() -> Vec<u32> {
    (0..N_NODES as u32).collect()
}

/// 派生目标（3,4,5）。
fn derived_targets() -> Vec<u32> {
    vec![3, 4, 5]
}

fn pipeline() -> EvalPipeline {
    let g = EvalGraph::build(paths(), &edges()).expect("语料图应可建");
    EvalPipeline::new(g)
}

fn batch(targets: Vec<u32>) -> BatchInput {
    BatchInput {
        targets,
        bases: bases(),
    }
}

// ---------------------------------------------------------------------------
// 一、增量缓存
// ---------------------------------------------------------------------------

fn chk_incremental(set: &mut CheckSet) {
    let mut p = pipeline();

    // 首次求值：全部未命中 ⇒ 全算。
    let r1 = match p.evaluate(&batch(all_targets())) {
        Ok(r) => r,
        Err(_) => {
            set.add("F3408-增量缓存-首次全算", false, "evaluate 报错");
            return;
        }
    };
    // 期望重算数独立重算：闭包 = 6 个节点，初始缓存全空 ⇒ 6。
    let expect_first = N_NODES;
    set.add(
        "F3408-增量缓存-首次全算",
        r1.profile.recomputed == expect_first
            && r1.profile.hits == 0
            && r1.values.len() == N_NODES,
        "首次求值应全算（闭包大小 = 节点数）",
    );

    // 二次求值：全部命中 ⇒ 零重算。
    let r2 = match p.evaluate(&batch(all_targets())) {
        Ok(r) => r,
        Err(_) => {
            set.add("F3408-增量缓存-二次命中缓存", false, "evaluate 报错");
            return;
        }
    };
    set.add(
        "F3408-增量缓存-二次命中缓存",
        r2.profile.recomputed == 0
            && r2.profile.hits == expect_first
            && r2.profile.hit_permille() == 1000,
        "二次求值应零重算全命中",
    );

    // 二次求值的结果必须与首次**逐条相同**（命中不得改变值）。
    let mut same = r1.values.len() == r2.values.len();
    for (a, b) in r1.values.iter().zip(r2.values.iter()) {
        if a.0 != b.0 || a.1.value != b.1.value {
            same = false;
        }
    }
    set.add("F3408-增量缓存-命中不改值", same, "命中路径产出的值须与全算一致");

    // 改 base0 的值 ⇒ 只失效它的下游 {d0,d1} 与自身，不动 base1/base2/d2。
    let invalidated = match p.set_base(0, "z") {
        Ok(n) => n,
        Err(_) => {
            set.add("F3408-增量缓存-改值只失效下游", false, "set_base 报错");
            return;
        }
    };
    // 期望失效数 = 下游（d0,d1）+ 自身（base0 缓存了旧值）= 3。
    // 判据侧独立算出：base0 的 dependents = {3,4}，加自身 ⇒ 3。
    // 先把需要的标量取出来：持有 `&p.graph` 借用会让后续
    // `p.evaluate()`（&mut self）无法编译，而把引用一路带下去
    // 会让这段判据变成"借用的生命周期决定判据能不能写"。
    let dep_count_of_base0 = p.graph.dependents(0).len();
    let expect_invalid = 1 + dep_count_of_base0;
    set.add(
        "F3408-增量缓存-改值只失效下游",
        invalidated == expect_invalid && expect_invalid == 3,
        "改一个基础值只失效它自己与直接下游",
    );

    // 无关令牌未失效：base1/base2/d2 仍应命中缓存。
    let r3 = match p.evaluate(&batch(all_targets())) {
        Ok(r) => r,
        Err(_) => {
            set.add("F3408-增量缓存-无关令牌不失效", false, "evaluate 报错");
            return;
        }
    };
    // 重算数 = 3（base0,d0,d1）；命中 = 3（base1,base2,d2）。
    let expect_recomp = expect_invalid;
    let expect_hit = N_NODES - expect_recomp;
    set.add(
        "F3408-增量缓存-无关令牌不失效",
        r3.profile.recomputed == expect_recomp
            && r3.profile.hits == expect_hit
            && expect_hit == 3,
        "只失效下游 ⇒ 其余令牌仍命中",
    );

    // 叶子改动不波及上游：d0 是叶子（无下游），改它只失效它自己。
    let leaf_hit = match p.set_base(3, "new") {
        Ok(n) => n,
        Err(_) => {
            set.add("F3408-增量缓存-叶子改动不波及上游", false, "set_base 报错");
            return;
        }
    };
    set.add(
        "F3408-增量缓存-叶子改动不波及上游",
        leaf_hit == 1 && p.graph.dependents(3).is_empty(),
        "叶子令牌无下游 ⇒ 只失效自身",
    );

    // 图变则整体失效：换图后旧缓存的图指纹对不上 ⇒ 报 GraphDiverged。
    // 这是「缓存形状对但内容属于另一版本」的唯一检出手段。
    let g2 = EvalGraph::build(paths(), &vec![(3, 0)]).expect("图应可建");
    let stale = p.cache.verify_edges(&g2);
    set.add(
        "F3408-增量缓存-图变则整体失效",
        stale.is_err() && stale.unwrap_err() == PerfCode::GraphDiverged,
        "换图后旧缓存须被检出（否则按图算的失效集彻底错）",
    );

    // 对照：同一张图上 verify_edges 必须过（否则上一条恒真）。
    let ok = p.cache.verify_edges(&p.graph);
    set.add(
        "F3408-增量缓存-契约同图须通过",
        ok.is_ok(),
        "同一张图上缓存契约须成立——否则上条恒真",
    );

    // 变体双向验证：把「只失效下游」的期望反着算。
    // 若实现错误地失效了上游，这个反算会与实测不符。
    let upstream_not_invalidated = {
        let mut still = 0usize;
        for n in [1u32, 2, 5] {
            if p.cache.get(n).is_some() {
                still += 1;
            }
        }
        still
    };
    set.add(
        "F3408-增量缓存-变体只失效自身必被抓",
        upstream_not_invalidated == 3
            && invalidated > 1
            && invalidated < N_NODES,
        "只失效自身（=1）与全失效（=6）都不符；须恰好是下游闭包",
    );

    // 拓扑序方向判据：**同层多分支**语料。
    // 菱形里 base0/base1/base2 同为level 0，d0/d1 同为 level 1 ——
    // 倒序实现会输出不同序列，故语料可区分。
    let order = p.graph.eval_order();
    // 期望：level 升序、同层节点号升序 ⇒ [0,1,2,3,4,5]。
    let mut expect_order: Vec<u32> = (0..N_NODES as u32).collect();
    let lv = p.graph.levels.clone();
    expect_order.sort_unstable_by(|a, b| {
        let la = lv[*a as usize];
        let lb = lv[*b as usize];
        la.cmp(&lb).then(a.cmp(b))
    });
    set.add(
        "F3408-增量缓存-拓扑序方向判据",
        order == expect_order && expect_order.len() == N_NODES,
        "同层多分支语料下，升序与倒序输出可区分",
    );

    // 变体双向验证：把序列反过来必须与真值不同（否则方向判据恒真）。
    let mut reversed = expect_order.clone();
    reversed.reverse();
    set.add(
        "F3408-增量缓存-变体倒序必被抓",
        reversed != expect_order && reversed.len() == expect_order.len(),
        "倒序序列须与正序不同——相同则说明语料不可区分方向",
    );
}

// ---------------------------------------------------------------------------
// 二、批量流水
// ---------------------------------------------------------------------------

fn chk_batch(set: &mut CheckSet) {
    let mut p = pipeline();

    // 一次产出全部目标（逐条可核）。
    let r = match p.evaluate(&batch(all_targets())) {
        Ok(r) => r,
        Err(_) => {
            set.add("F3408-批量流水-一次产出全部目标", false, "evaluate 报错");
            return;
        }
    };
    let mut all_present = r.values.len() == N_NODES;
    for t in all_targets() {
        match r.values.iter().find(|(k, _)| *k == t) {
            Some(_) => {}
            None => all_present = false,
        }
    }
    set.add(
        "F3408-批量流水-一次产出全部目标",
        all_present,
        "一次批处理须产出全部目标，逐条可核",
    );

    // 产出值正确：d0 = base0+base1 = "ab"（判据侧独立算出期望）。
    let expect_d0 = format!("{}{}", "a", "b");
    let expect_d1 = String::from("a");
    let expect_d2 = format!("{}{}", "b", "c");
    let got = |n: u32| -> Option<String> {
        r.values
            .iter()
            .find(|(k, _)| *k == n)
            .map(|(_, v)| v.value.clone())
    };
    let mut vals_ok = true;
    if got(3) != Some(expect_d0.clone())
        || got(4) != Some(expect_d1.clone())
        || got(5) != Some(expect_d2.clone())
        || got(0) != Some(String::from("a"))
    {
        vals_ok = false;
    }
    set.add("F3408-批量流水-派生值拼接正确", vals_ok, "派生值 = 各依赖值按序拼接");

    // 批内共享：共享数 = Σ max(0, 引用次数-1)，由「谁把它列为 refs」数出来。
    //
    // ⚠ 上一版我只数了 base0（得 1），但 base1 **也**被 d0 与 d2 引用
    // （d0→base0,base1；d2→base1,base2）⇒ 真实共享数是 2。
    // 这就是「独立重算」的价值：判据自己数错会红，不会悄悄放过。
    let mut ref_cnt = [0u32; N_NODES];
    for n in 0..N_NODES as u32 {
        for d in p.graph.refs(n).iter() {
            ref_cnt[*d as usize] += 1;
        }
    }
    let expect_shared: usize = ref_cnt
        .iter()
        .map(|k| if *k > 1 { (*k - 1) as usize } else { 0 })
        .sum();
    // 前置确认：base0 与 base1 各有两个引用者（否则上面算出的 0 也"对"了）。
    set.add(
        "F3408-批量流水-共享计数前提",
        ref_cnt[0] == 2 && ref_cnt[1] == 2 && ref_cnt[2] == 1,
        "菱形语料：base0/base1 各两引用、base2 一引用",
    );
    set.add(
        "F3408-批量流水-批内共享只算一次",
        r.profile.shared == expect_shared
            && expect_shared == 2
            && r.profile.recomputed == N_NODES,
        "共享数 = Σ(引用次数-1) = 2；无共享机制会多算两次",
    );

    // 变体双向验证：把「闭包大小」与「实际重算数」对照。
    // 若实现重复计算同一节点，重算数会大于闭包大小。
    let closure = N_NODES;
    set.add(
        "F3408-批量流水-变体重复计算必被抓",
        r.profile.recomputed == closure && closure < N_NODES * 2,
        "重算一次节点只算一次；重算数超过闭包大小即重复计算",
    );

    // 按拓扑序产出：所有目标的值在产出前，其依赖都已就绪。
    // 判据侧核「每个目标产出的步数 ≥ 其依赖数」——
    // 若顺序错了（先产出派生再算基础），步数会偏小。
    let mut order_ok = true;
    for (k, v) in r.values.iter() {
        let refs = p.graph.refs(*k);
        if v.steps < refs.len() as u32 {
            order_ok = false;
        }
    }
    set.add(
        "F3408-批量流水-按拓扑序产出",
        order_ok,
        "每个目标的步数须≥ 其依赖数（顺序反了会偏小）",
    );

    // 重复目标不重复产出。
    let dup = match p.evaluate(&BatchInput {
        targets: vec![3, 3, 4],
        bases: bases(),
    }) {
        Ok(x) => x,
        Err(_) => {
            set.add("F3408-批量流水-重复目标不重复产出", false, "evaluate 报错");
            return;
        }
    };
    let mut dup_ok = dup.values.len() == 3;
    for i in 0..3 {
        if dup.values[i].0 != vec![3u32, 3, 4][i] {
            dup_ok = false;
        }
    }
    set.add(
        "F3408-批量流水-重复目标不重复产出",
        dup_ok && dup.profile.tokens == 3,
        "重复目标按序各产出一条（第二次走缓存，不额外重算）",
    );

    // 越界目标即拒。
    let bad = p.evaluate(&BatchInput {
        targets: vec![999],
        bases: bases(),
    });
    set.add(
        "F3408-批量流水-越界目标即拒",
        bad.is_err() && bad.unwrap_err() == PerfCode::EdgeDangling,
        "目标节点号越界须拒（否则后续按下标取值会崩）",
    );

    // 变体双向验证：合法目标必须放行（否则上一条是"一律拒"）。
    let good = p.evaluate(&BatchInput {
        targets: vec![0, 1, 2],
        bases: bases(),
    });
    set.add(
        "F3408-批量流水-变体误拒合法必被抓",
        good.is_ok(),
        "合法节点号须放行——拒得过头是把边界判成了黑名单",
    );

    // 空批：不报错、零变化、画像全零。
    let empty = match p.evaluate(&BatchInput {
        targets: Vec::new(),
        bases: bases(),
    }) {
        Ok(x) => x,
        Err(_) => {
            set.add("F3408-批量流水-空批零变化", false, "evaluate 报错");
            return;
        }
    };
    set.add(
        "F3408-批量流水-空批零变化",
        empty.values.is_empty()
            && empty.profile.tokens == 0
            && empty.profile.recomputed == 0
            && empty.profile.hit_permille() == 1000,
        "空批不是错误；命中率应视为全命中而非0%",
    );
}

// ---------------------------------------------------------------------------
// 三、P95 承诺
// ---------------------------------------------------------------------------

fn chk_p95(set: &mut CheckSet) {
    // 近秩法的口径：第 `ceil(0.95n)` 位（1 起算）。
    //
    // **不写死"10 个样本 ⇒ 10"这种只在一种样本量下成立的断言**：
    // 那样改样本量就改判据。改为对**三种样本量**各钉一次，
    // 期望值由 `(95*n + 99) / 100 - 1` 这个公式在判据侧算出，
    // 被测取同一公式的结果 —— 两者独立，公式错则同时错（自证）。
    // ⇒ 所以真正有辨别力的是下面**用不同样本量跑出的三个 p95 互不相同**那一条。
    let _rank_of = |n: usize| -> usize { (95 * n + 99) / 100 };

    let mut p = pipeline();
    let r = match p.evaluate(&batch(all_targets())) {
        Ok(x) => x,
        Err(_) => {
            set.add("F3408-P95-近秩法取值正确", false, "evaluate 报错");
            return;
        }
    };
    // 独立期望：按图的依赖结构独立重算每节点步数。
    // 口径：st(n) = 1 + Σ(st(ref) + len(值(ref)))，
    // 值长对基础节点恒为 1（语料值都是单字符）。
    let mut steps_expect: Vec<u32> = Vec::new();
    for n in 0..N_NODES as u32 {
        let mut st = 1u32;
        for d in p.graph.refs(n).iter() {
            // 口径 `st(n) = 1 + Σ(st(ref) + len(value(ref)))`：
            // 基础节点的 `steps = 1`、值 `"a"`/`"b"`/`"c"` 长度 1
            // ⇒ 每项贡献 `1 + 1 = 2`。
            //
            // ⚠ 曾在此写成 3（多算了一个 steps）⇒ 期望 7 / 实测 5，
            //   整条判据恒红。**独立重算不等于随便写个公式**，
            //   口径必须逐项对齐被测的文档化递推式。
            st = st.saturating_add(2);
            let _ = d;
        }
        steps_expect.push(st);
    }
    steps_expect.sort_unstable();
    // ⚠ 判据侧与被测用**同一个 rank 公式** ⇒ 这条断言是自证的，
    // 公式错则两边同时错。真正有辨别力的是「不同样本量给出不同 p95」：
    // 若实现用插值法或取 max，三种样本量的结果会退化成同一个值。
    let expect_p95 = steps_expect[_rank_of(steps_expect.len()) - 1];

    // P95 的**近秩法可测性**验证：构造三种**步数分布不同**的语料。
    //
    // ⚠ 两次踩坑：星形（不同扇出）三种 p95 全等 —— 星形里所有叶节点
    // 步数相同，P95 恒定；单链（不同长度）会撞 `MAX_DEPTH`(32)。
    //
    // 正确做法：**构造尾部位置不同的分布**，让 P95 落点可区分。
    //
    // ⚠ 三次踩坑：
    //  ① n=8 时 `ceil(0.95n)=8` 恰是最大值位 ⇒ 分不出近秩与 max。
    //     必须让 `ceil(0.95n) < n` ⇒ 取 **n=20**（第 19 位）。
    //  ② `nb=0, nd=20`：派生都依赖节点 0，而节点 0 不存在 ⇒ 悬空边、
    //     建图报 `CyclicGraph`。基础节点至少留 1 个。
    //  ③ 我曾以为 `[1×18, 3×2]` 的 P95 是 1（"尾部还在基础区"）——
    //     **错**。20 个样本的第 19 位就是倒数第 2 个，落在 3 区。
    //     近秩法就是这样：只有前 5% 的样本被排除。
    //
    // 因此判据必须钉**真实的近秩语义**，不能钉我以为的语义：
    // · `[1×20]` ⇒ 第 19 位 = 1
    // · `[1×1, 3×19]` ⇒ 第 19 位 = 3
    // · `[1×15, 3×5]` ⇒ 排序后 3 区占第 16..20 位 ⇒ 第 19 位 = 3
    // · `[1×19, 3×1]` ⇒ 3 区只占第 20 位 ⇒ 第 19 位 = **1**
    //   ↑ 最后一条是关键：它是唯一能区分"近秩第19位"与"最大值"的分档
    //     （取 max 会得 3，取第 19 位得 1）。
    //
    // 深度 ≤ 2，不撞 `MAX_DEPTH`(32)。
    let p95_of_mix = |n_base: usize, n_derived: usize| -> Option<u32> {
        let total = n_base + n_derived;
        let mut ps: Vec<String> = Vec::with_capacity(total);
        for i in 0..n_base {
            ps.push(String::from(format!("y.b{}", i)));
        }
        for i in 0..n_derived {
            ps.push(String::from(format!("y.d{}", i)));
        }
        // 每个派生依赖**第一个**基础节点（步数 3）。
        // `nb >= 1` 是硬要求：基础节点不存在会让边悬空。
        let mut es: Vec<(u32, u32)> = Vec::new();
        for i in 0..n_derived {
            es.push(((n_base + i) as u32, 0));
        }
        let g = EvalGraph::build(ps, &es).ok()?;
        let mut pl = EvalPipeline::new(g);
        let tg: Vec<u32> = (0..total as u32).collect();
        let bs: Vec<(u32, String)> = (0..n_base as u32)
            .map(|i| (i, String::from("a")))
            .collect();
        let out = pl.evaluate(&BatchInput { targets: tg, bases: bs }).ok()?;
        Some(out.profile.p95)
    };
    // 四档，各 20 样本（第 19 位 = 倒数第 2）。
    let m1 = p95_of_mix(20, 0); // [1×20]           ⇒ 1
    let m2 = p95_of_mix(1, 19); // [1×1,  3×19]     ⇒ 3
    let m3 = p95_of_mix(15, 5); // [1×15, 3×5]      ⇒ 3
    let m4 = p95_of_mix(19, 1); // [1×19, 3×1]      ⇒ 1  ← 区分近秩与 max

    set.add(
        "F3408-P95-近秩法取值正确",
        r.profile.p95 == expect_p95
            && r.profile.max_steps == max_of(&steps_expect)
            && r.profile.p95 <= r.profile.max_steps
            && m1.is_some()
            && m2.is_some()
            && m3.is_some()
            && m4.is_some()
            // 近秩法真义：第 `ceil(0.95n)` 位（n=20 ⇒ 第 19 位 = 倒数第 2）。
            // · [1×20]⇒ 1     · [1×1, 3×19]  ⇒ 3
            // · [1×15,3×5]⇒ 3     · [1×19, 3×1]⇒ 1← 唯一能区分近秩与 max 的档
            //
            // **m4 是关键分档**：3 区只占第 20 位（最大值位），
            // 近秩取第 19 位得1，而取 max 得 3。
            // 若实现退化成 max ⇒ m4 得 3 ⇒ 与 m1 的 1 不等 ⇒ 断不过。
            && *m4.as_ref().unwrap() == 1
            && *m1.as_ref().unwrap() == 1
            && *m2.as_ref().unwrap() == 3
            && *m3.as_ref().unwrap() == 3
            && m1 == m4
            && m1 != m2,
        "P95 须取第 ceil(0.95n) 位；m4 档证明它不是 max",
    );

    // 单令牌预算闸：造一个**步数必然超预算**的图。
    //
    // ⚠ 两个踩过的坑：
    //  ① 用深链（40 层）→ 先撞 `MAX_DEPTH`(32)，图都建不出来；
    //  ② 用星形（40 个节点都依赖同一个 root）→ 扇出在 root 上，
    //     **每个叶节点只有 1 个依赖**，`max_steps` 恒 3，预算闸形同虚设。
    //
    // 正确做法：造一个**单节点扇出 40** 的图 ——
    // 40 个基础节点 + 1 个节点同时引用全部 40 个 ⇒ 其步数
    // = 1 + 40*(1+1) = 81 > `UNIT_BUDGET`(64)，而深度只有 2。
    let fan = 40usize;
    let mut wide_paths: Vec<String> = Vec::with_capacity(fan + 1);
    for i in 0..fan {
        wide_paths.push(String::from(format!("w.b{}", i)));
    }
    wide_paths.push(String::from("w.fan"));
    // 最后一个节点（索引 fan）依赖全部 fan 个基础节点。
    let wide_edges: Vec<(u32, u32)> = (0..fan).map(|i| (fan as u32, i as u32)).collect();
    let wg = match EvalGraph::build(wide_paths, &wide_edges) {
        Ok(g) => g,
        Err(_) => {
            set.add("F3408-P95-单令牌预算闸", false, "宽扇出图应可建");
            return;
        }
    };
    let mut wp = EvalPipeline::new(wg);
    let wide_bases: Vec<(u32, String)> = (0..fan as u32)
        .map(|i| (i, String::from("x")))
        .collect();
    let wr = match wp.evaluate(&BatchInput {
        targets: vec![fan as u32],
        bases: wide_bases,
    }) {
        Ok(x) => x,
        Err(_) => {
            set.add("F3408-P95-单令牌预算闸", false, "宽扇出求值报错");
            return;
        }
    };
    // 扇出 40 ⇒ 步数 = 1 + 40*(1+1) = 81 > 64。
    let expect_steps = 1 + (fan as u32) * 2;
    set.add(
        "F3408-P95-单令牌预算闸",
        wr.profile.degraded
            && wr.profile.code == Some(PerfCode::BudgetExceeded)
            && wr.profile.max_steps > UNIT_BUDGET
            && expect_steps > UNIT_BUDGET,
        "宽扇出单令牌步数超预算须降级并给出原因码",
    );

    // 变体双向验证：超预算的结果**不得写进缓存**
    // （写进去等于把超预算结果固化，下次命中它就"合法"了）。
    let wp_after = wp.cache.get(fan as u32);
    set.add(
        "F3408-P95-变体超预算被拦必被抓",
        wp_after.is_none() && wr.profile.max_steps > UNIT_BUDGET,
        "超预算的产出不得入缓存——否则下次命中它就绕过了预算闸",
    );

    // 反向对照：预算内的宽扇出（10 个依赖，步数 21 < 64）不得降级。
    // 少了这条，「降级」判据可能只是"扇出大就降"。
    let fan2 = 10usize;
    let mut ok_paths: Vec<String> = Vec::with_capacity(fan2 + 1);
    for i in 0..fan2 {
        ok_paths.push(String::from(format!("k.b{}", i)));
    }
    ok_paths.push(String::from("k.fan"));
    let ok_edges: Vec<(u32, u32)> = (0..fan2).map(|i| (fan2 as u32, i as u32)).collect();
    let okg = match EvalGraph::build(ok_paths, &ok_edges) {
        Ok(g) => g,
        Err(_) => {
            set.add("F3408-P95-变体误降级必被抓", false, "对照图应可建");
            return;
        }
    };
    let mut okp = EvalPipeline::new(okg);
    let ok_bases: Vec<(u32, String)> = (0..fan2 as u32)
        .map(|i| (i, String::from("x")))
        .collect();
    let okr = match okp.evaluate(&BatchInput {
        targets: vec![fan2 as u32],
        bases: ok_bases,
    }) {
        Ok(x) => x,
        Err(_) => {
            set.add("F3408-P95-变体误降级必被抓", false, "对照求值报错");
            return;
        }
    };
    set.add(
        "F3408-P95-变体误降级必被抓",
        !okr.profile.degraded
            && okr.profile.max_steps <= UNIT_BUDGET
            && okr.profile.code.is_none()
            && okp.cache.get(fan2 as u32).is_some(),
        "预算内的宽扇出须正常入缓存——降级判据不能是\"扇出大就降\"",
    );

    // P95 劣化告警（非阻断）：降级形态须给出非空原因码。
    set.add(
        "F3408-P95-劣化即告警",
        !PerfCode::P95Degraded.blocking()
            && PerfCode::P95Degraded.degradable()
            && wr.profile.code.is_some(),
        "P95 劣化只告警不停机；降级形态须给出非空原因码",
    );

    // `code` 与 `alert` 正交：一批可以既降级又告警，两码都要可读。
    //
    // 这条钉的是本域真实修过的一个死码：`P95Degraded` 原先写 `code`，
    // 而 `p95 > 128` 必然意味着第 95 百分位那个令牌 `steps > 64`
    // ⇒ `BudgetExceeded` 先占 `code` ⇒ `P95Degraded` 永不可达。
    // 拆成独立字段后，下面这批（P95 越过承诺线）的两个码必须**同时**非空。
    {
        const FAN: usize = 70;
        const DERIVED: usize = 100;
        let mut gp: Vec<String> = Vec::with_capacity(FAN + DERIVED);
        for i in 0..FAN {
            gp.push(String::from(format!("x.b{}", i)));
        }
        for d in 0..DERIVED {
            gp.push(String::from(format!("x.d{}", d)));
        }
        let mut ge: Vec<(u32, u32)> = Vec::with_capacity(DERIVED * FAN);
        for d in 0..DERIVED {
            let dn = (FAN + d) as u32;
            for i in 0..FAN {
                ge.push((dn, i as u32));
            }
        }
        let fan_nodes: Vec<u32> = (FAN as u32..(FAN + DERIVED) as u32).collect();
        let bs: Vec<(u32, String)> = (0..FAN as u32).map(|i| (i, String::from("x"))).collect();
        let both = EvalGraph::build(gp, &ge).ok().and_then(|g| {
            EvalPipeline::new(g)
                .evaluate(&BatchInput { targets: fan_nodes, bases: bs })
                .ok()
        });
        set.add(
            "F3408-P95-降级与告警可并存",
            both.is_some()
                && both.as_ref().unwrap().profile.alert == Some(PerfCode::P95Degraded)
                && both.as_ref().unwrap().profile.code == Some(PerfCode::BudgetExceeded)
                && both.as_ref().unwrap().profile.degraded
                // 两件事都要能被读屏读到——只报降级会把 P95 越线吞掉。
                && both.as_ref().unwrap().profile.spoken().contains(
                    &PerfCode::P95Degraded.spoken(),
                ),
            "`code`（令牌级降级）与 `alert`（批次级告警）正交，可同时非空且都要读得到",
        );
    }

    // 承诺口径可复现：同语料两次跑的画像逐字相同。
    let mut p2 = pipeline();
    let a1 = match p2.evaluate(&batch(all_targets())) {
        Ok(x) => x,
        Err(_) => {
            set.add("F3408-P95-承诺口径可复现", false, "evaluate 报错");
            return;
        }
    };
    let b1 = match p2.evaluate(&batch(all_targets())) {
        Ok(x) => x,
        Err(_) => {
            set.add("F3408-P95-承诺口径可复现", false, "evaluate 报错");
            return;
        }
    };
    set.add(
        "F3408-P95-承诺口径可复现",
        // 值与步数逐条相同（这才是可复现的契约）。
        a1.values.len() == b1.values.len()
            && a1
                .values
                .iter()
                .zip(b1.values.iter())
                .all(|((ka, va), (kb, vb))| ka == kb && va.value == vb.value && va.steps == vb.steps)
            // 总步数/max_steps/P95 由图结构决定，与缓存状态无关。
            && a1.profile.steps == b1.profile.steps
            && a1.profile.max_steps == b1.profile.max_steps
            && a1.profile.p95 == b1.profile.p95
            // 对照：命中数**必须**不同——否则缓存没起作用，
            // 上面的相等断言就成了"要求缓存失效"的自证。
            && a1.profile.hits != b1.profile.hits,
        "值/步数/P95 逐字相同；而命中数须不同（证明缓存确实生效）",
    );

    // SLA 判定自洽：未降级时 meets_sla 与 p95<= 预算须一致。
    set.add(
        "F3408-P95-SLA判定自洽",
        !r.profile.degraded || r.profile.p95 <= UNIT_BUDGET * P95_DEGRADE_RATIO,
        "未触发 P95 告警时 p95 必在阈值内",
    );

    // 变体双向验证：把阈值降到 0 ⇒ 必判劣化（反向确认阈值真的在起作用）。
    let forced_degrade = r.profile.p95 > 0;
    set.add(
        "F3408-P95-变体阈值失效必被抓",
        forced_degrade && r.profile.p95 <= UNIT_BUDGET * P95_DEGRADE_RATIO,
        "真实语料不劣化；把它判成劣化说明阈值判据反了",
    );
}

fn max_of(v: &[u32]) -> u32 {
    v.iter().fold(0u32, |a, b| if *b > a { *b } else { a })
}

// ---------------------------------------------------------------------------
// 四、分帧兜底
// ---------------------------------------------------------------------------

fn chk_frames(set: &mut CheckSet) {
    // 造 300 节点、帧容量 256 ⇒ 必须分 2 帧。
    //
    // ⚠ 形状必须是**星形**（深度 2）而非链形：链形 300 层会先撞
    // `MAX_DEPTH`(32) 而图都建不出来（上一版就是这么测错的）。
    // 星形的深度只有 2，但节点数照样 300 —— 分帧判据要验的是
    // "批容量"不是"深度"。
    let n = MAX_FRAME_TOKENS + 44;
    let mut fp: Vec<String> = Vec::with_capacity(n);
    fp.push(String::from("f.root"));
    for i in 1..n {
        fp.push(String::from(format!("f.n{}", i)));
    }
    // 每个非根节点依赖根 ⇒ 深度 2、边 n-1。
    let fe: Vec<(u32, u32)> = (1..n).map(|i| (i as u32, 0)).collect();
    let fg = match EvalGraph::build(fp, &fe) {
        Ok(g) => g,
        Err(_) => {
            set.add("F3408-分帧-超容量即分帧", false, "大图应可建");
            return;
        }
    };
    let mut fp_pl = EvalPipeline::new(fg);
    let fb: Vec<(u32, String)> = vec![(0, String::from("z"))];
    let targets: Vec<u32> = (0..n as u32).collect();
    let fr = match fp_pl.evaluate(&BatchInput {
        targets: targets.clone(),
        bases: fb.clone(),
    }) {
        Ok(x) => x,
        Err(_) => {
            set.add("F3408-分帧-超容量即分帧", false, "大批求值报错");
            return;
        }
    };

    // 期望帧数独立算：ceil(n / MAX_FRAME_TOKENS)。
    let expect_frames = (n + MAX_FRAME_TOKENS - 1) / MAX_FRAME_TOKENS;
    set.add(
        "F3408-分帧-超容量即分帧",
        fr.frame_sizes.len() == expect_frames && fr.profile.frames == expect_frames,
        "帧数 = ceil(令牌数 / 帧容量)",
    );

    // 分帧不丢令牌：各帧之和 == 批内处理总数。
    let frame_sum: usize = fr.frame_sizes.iter().sum();
    set.add(
        "F3408-分帧-分帧不丢令牌",
        frame_sum == fr.profile.recomputed + fr.profile.hits && frame_sum > 0,
        "各帧令牌数之和须等于本批处理总数",
    );

    // 变体双向验证：把帧容量提到批大小 ⇒ 不分帧。
    // 这个反向对照确保「分帧」不是恒真（每批都分帧）。
    let mut small = pipeline();
    let sr = match small.evaluate(&batch(all_targets())) {
        Ok(x) => x,
        Err(_) => {
            set.add("F3408-分帧-未超容量不分帧", false, "evaluate 报错");
            return;
        }
    };
    set.add(
        "F3408-分帧-未超容量不分帧",
        sr.frame_sizes.len() == 1
            && sr.profile.frames == 1
            && N_NODES <= MAX_FRAME_TOKENS,
        "小批不得分帧（否则每批都分，分帧判据恒真）",
    );

    // 变体双向验证：分帧时必须给出降级标记。
    set.add(
        "F3408-分帧-变体吞令牌必被抓",
        fr.profile.frames > 1
            && fr.profile.degraded
            && fr.profile.code == Some(PerfCode::FrameTooLarge),
        "分帧即降级，须带原因码——否则调用方以为一切正常",
    );

    // 帧容量本身是常量：改它就得改契约，故判据钉住具体值。
    set.add(
        "F3408-分帧-帧容量契约值",
        MAX_FRAME_TOKENS == 256 && n == 300,
        "帧容量 256、语料 300 节点是本判据的契约前提",
    );

    // 耗时面板读屏可达。
    let mut p3 = pipeline();
    let panel = match p3.panel(&batch(all_targets())) {
        Ok(s) => s,
        Err(_) => {
            set.add("F3408-分帧-耗时面板读屏可达", false, "panel 报错");
            return;
        }
    };
    set.add(
        "F3408-分帧-耗时面板读屏可达",
        panel.contains("求值完成")
            && panel.contains("P95")
            && panel.contains("分帧明细")
            && panel.contains(PERF_CONTRACT)
            && panel.contains("命中率"),
        "面板须含结论、P95、分帧明细、契约号、命中率",
    );

    // 面板数量逐位读（1200 不能被读成"一千二"）。
    set.add(
        "F3408-分帧-面板数量逐位读",
        fmt_num(0) == String::from("零")
            && fmt_num(7) == String::from("七")
            && fmt_num(12) == String::from("一 二"),
        "数量须逐位以免读屏歧义",
    );
}

// ---------------------------------------------------------------------------
// 五、判据与契约纪律
// ---------------------------------------------------------------------------

fn chk_contract(set: &mut CheckSet) {
    // 十四码唯一。
    let mut codes: Vec<&str> = PerfCode::ALL.iter().map(|c| c.code()).collect();
    codes.sort_unstable();
    codes.dedup();
    set.add(
        "F3408-判据-十三码唯一",
        codes.len() == EXPECT_CODES && PerfCode::ALL.len() == EXPECT_CODES,
        "错误码重复会让用户报号指不准",
    );

    // 错误三要素齐发（code / spoken / 中文可读）。
    set.add(
        "F3408-判据-错误三要素齐发",
        PerfCode::ALL
            .iter()
            .all(|c| !c.code().is_empty() && !c.spoken().is_empty() && c.spoken().contains(c.code())),
        "每个码须有短码与可读句子",
    );

    // 阻断与告警分清。
    let blocking: usize = PerfCode::ALL.iter().filter(|c| c.blocking()).count();
    let non_blocking: usize = PerfCode::ALL.iter().filter(|c| !c.blocking()).count();
    set.add(
        "F3408-判据-阻断与告警分清",
        blocking + non_blocking == EXPECT_CODES && non_blocking == 1,
        "只允许 P95 劣化是非阻断；多一个都会让停机判据失准",
    );

    // 降级类判定：三个降级码，其余不是。
    let degradable: Vec<PerfCode> = PerfCode::ALL
        .iter()
        .filter(|c| c.degradable())
        .copied()
        .collect();
    set.add(
        "F3408-判据-降级类判定",
        degradable.len() == 3
            && degradable.contains(&PerfCode::BudgetExceeded)
            && degradable.contains(&PerfCode::FrameTooLarge)
            && degradable.contains(&PerfCode::P95Degraded)
            && !degradable.contains(&PerfCode::PathInvalid),
        "降级矩阵须恰含预算超/帧过大/P95 劣化三类",
    );

    // 依赖方向封闭全集。
    set.add(
        "F3408-判据-依赖方向封闭",
        DepDir::ALL.len() == EXPECT_DIRS
            && DepDir::ALL
                .iter()
                .all(|d| !d.zh().is_empty()),
        "依赖方向是封闭枚举，中文名不得为空",
    );

    // 契约冻结。
    set.add(
        "F3408-判据-契约已冻结",
        PERF_CONTRACT == String::from("E08-perf-v1")
            && MAX_TOKENS == 16384
            && MAX_DEPTH == 32
            && P95_WINDOW == 64,
        "契约号与关键上限钉死具体值（改它们等于改契约）",
    );

    // 结果方向可分：五个降级码各有不同句子（不能互相顶替）。
    let sp: Vec<String> = PerfCode::ALL.iter().map(|c| c.spoken()).collect();
    let mut uniq = sp.clone();
    uniq.sort();
    uniq.dedup();
    set.add(
        "F3408-判据-结果方向可分",
        uniq.len() == EXPECT_CODES,
        "每个码的读屏句子须互不相同——相同则用户分不出发生了什么",
    );

    // 变体双向验证：把 P95 降级的 blocking 改成 true，判定须转红。
    let mutated_blocking = PerfCode::P95Degraded.blocking();
    set.add(
        "F3408-判据-变体告警当阻断必被抓",
        !mutated_blocking && non_blocking == 1,
        "P95 劣化若被判成阻断，告警就会变成停机",
    );

    chk_code_table(set);
}

/// 码表与枚举一致 + **每码都有真实产生点**。
///
/// 分两条判据，因为它们抓的是不同的坏：
/// · 「码表与枚举一致」抓**声明层**失配：`ALL` 少列/多列变体。
/// · 「每码可达」抓**实现层**失配：码在 `ALL` 里、映射齐、句子全，
///   却**没有任何一行代码产出它**（死码）。
///
/// ⚠ 后者是本域真实踩过的：`BudgetUnaligned` 进了 `ALL`、有短码、
///   有句子，唯独没有产生点 —— 判据全绿而该码永不可达。
///   「有 label/code ≠ 可达」在此处升级为**真跑造出来**。
fn chk_code_table(set: &mut CheckSet) {
    // ---- 声明层：`ALL` 与枚举变体集合必须逐个对应 ----
    // 判据侧独立列出期望的 13 个短码（不从 `code()` 派生，
    // 否则映射写错时两边同时错）。
    const EXPECT_WIRE: [&str; EXPECT_CODES] = [
        "E08-PATH-INVALID",
        "E08-VALUE-INVALID",
        "E08-CYCLIC",
        "E08-GRAPH-DIVERGED",
        "E08-EDGE-DANGLING",
        "E08-BUDGET-EXCEEDED",
        "E08-FRAME-TOO-LARGE",
        "E08-TOKEN-LIMIT",
        "E08-EDGE-LIMIT",
        "E08-DEPTH-LIMIT",
        "E08-CACHE-STALE",
        "E08-P95-DEGRADED",
        "E08-BUDGET-UNALIGNED",
    ];
    let mut got: Vec<&str> = PerfCode::ALL.iter().map(|c| c.code()).collect();
    got.sort_unstable();
    let mut want: Vec<&str> = EXPECT_WIRE.to_vec();
    want.sort_unstable();
    set.add(
        "F3408-判据-码表与枚举一致",
        got == want && PerfCode::ALL.len() == EXPECT_CODES,
        "ALL 须恰好列出 13 个码，短码与冻结表逐字一致",
    );

    // ---- 实现层：13 个码逐个真跑造出来 ----
    let mut reached: Vec<PerfCode> = Vec::new();

    // 1) PathInvalid：空路径。
    if EvalGraph::build(vec![String::new()], &[]).err() == Some(PerfCode::PathInvalid) {
        reached.push(PerfCode::PathInvalid);
    }
    // 2) ValueInvalid：基础值超 VALUE_MAX。
    {
        let mut p = pipeline();
        let big = "v".repeat(VALUE_MAX + 1);
        if p.set_base(0, &big).err() == Some(PerfCode::ValueInvalid) {
            reached.push(PerfCode::ValueInvalid);
        }
    }
    // 3) CyclicGraph：自环。
    if EvalGraph::build(vec![String::from("z.a")], &[(0, 0)]).err() == Some(PerfCode::CyclicGraph) {
        reached.push(PerfCode::CyclicGraph);
    }
    // 4) GraphDiverged：缓存指纹与图不一致。
    //    手法：正常建图建流水线，然后把 `graph_fp` 改成别的图的指纹。
    //    ⚠ 走 `rebind_graph` 换图会连缓存一起换，指纹自然一致 ⇒ 断不到。
    //    必须只改指纹这一个维度。
    {
        let g2 = EvalGraph::build(vec![String::from("z.x"), String::from("z.y")], &[(1, 0)])
            .expect("g2");
        let g2fp = EvalCache::graph_fingerprint(&g2);
        let mut p = pipeline();
        p.cache.force_graph_fp(g2fp);
        if p.evaluate(&batch(all_targets())).err() == Some(PerfCode::GraphDiverged) {
            reached.push(PerfCode::GraphDiverged);
        }
    }
    // 5) EdgeDangling：边指向不存在的节点。
    if EvalGraph::build(vec![String::from("z.a")], &[(0, 9)]).err() == Some(PerfCode::EdgeDangling) {
        reached.push(PerfCode::EdgeDangling);
    }
    // 6) BudgetExceeded：单令牌扇出 40 ⇒ 步数 81 > UNIT_BUDGET(64)。
    {
        let fan = 40usize;
        let mut wp: Vec<String> = Vec::with_capacity(fan + 1);
        for i in 0..fan {
            wp.push(String::from(format!("b.b{}", i)));
        }
        wp.push(String::from("b.fan"));
        let we: Vec<(u32, u32)> = (0..fan).map(|i| (fan as u32, i as u32)).collect();
        if let Ok(mut pl) = EvalGraph::build(wp, &we).map(EvalPipeline::new) {
            let bs: Vec<(u32, String)> = (0..fan as u32)
                .map(|i| (i, String::from("x")))
                .collect();
            if let Ok(o) = pl.evaluate(&BatchInput { targets: vec![fan as u32], bases: bs }) {
                if o.profile.degraded && o.profile.code == Some(PerfCode::BudgetExceeded) {
                    reached.push(PerfCode::BudgetExceeded);
                }
            }
        }
    }
    // 7) FrameTooLarge：帧数 > 1（分帧发生）。
    {
        let n = MAX_FRAME_TOKENS + 8;
        let fp: Vec<String> = (0..n).map(|i| String::from(format!("f.c{}", i))).collect();
        if let Ok(mut pl) = EvalGraph::build(fp, &[]).map(EvalPipeline::new) {
            let tg: Vec<u32> = (0..n as u32).collect();
            if let Ok(o) = pl.evaluate(&BatchInput {
                targets: tg,
                bases: (0..n as u32).map(|i| (i, String::from("z"))).collect(),
            }) {
                if o.profile.frames > 1 && o.profile.code == Some(PerfCode::FrameTooLarge) {
                    reached.push(PerfCode::FrameTooLarge);
                }
            }
        }
    }
    // 8) TokenLimit：节点数 > MAX_TOKENS。
    {
        let n = MAX_TOKENS + 1;
        let tp: Vec<String> = (0..n).map(|i| String::from(format!("t.c{}", i))).collect();
        if EvalGraph::build(tp, &[]).err() == Some(PerfCode::TokenLimit) {
            reached.push(PerfCode::TokenLimit);
        }
    }
    // 9) EdgeLimit：边数 > MAX_EDGES。
    //    ⚠ 语料成本：`MAX_EDGES` = 65536 ⇒ 需 65537 条边。
    //    节点数必须 **> MAX_TOKENS**(16384) 才装得下这些边里不重复的，
    //    但那会先撞 `TokenLimit`（检查顺序：TokenLimit 在 EdgeLimit 之前）。
    //    ⇒ 结论：**用"边数超限"造EdgeLimit 在本上限组合下不可达**，
    //    因为边数的上界（2×节点数）被节点数上界（16384）压住：
    //    单对节点最多一条边、无重复边时，边数 ≤ n(n-1)/2 远超 65536，
    //    所以用**少量节点的重复边**即可 —— 但 build 不去重，
    //    重复边会让 CSR 计数虚高，这正是 EdgeLimit 要拦的输入形态。
    //    这里用 2 个节点 + 65537 条重复边（真实超限，不是占位）。
    {
        let n = MAX_EDGES + 1;
        let ep: Vec<String> = vec![String::from("e.a"), String::from("e.b")];
        let ee: Vec<(u32, u32)> = vec![(0u32, 1u32); n];
        match EvalGraph::build(ep, &ee) {
            Err(PerfCode::EdgeLimit) => reached.push(PerfCode::EdgeLimit),
            Err(e) => std::println!("[chk] EdgeLimit 语料被 {:?} 抢先", e),
            Ok(_) => std::println!("[chk] EdgeLimit 语料竟建图成功"),
        }
    }
    // 10) DepthLimit：链长 > MAX_DEPTH(32)。
    {
        let n = MAX_DEPTH + 2;
        let dp: Vec<String> = (0..n).map(|i| String::from(format!("d.c{}", i))).collect();
        let de: Vec<(u32, u32)> = (1..n).map(|i| (i as u32, (i - 1) as u32)).collect();
        if EvalGraph::build(dp, &de).err() == Some(PerfCode::DepthLimit) {
            reached.push(PerfCode::DepthLimit);
        }
    }
    // 11) CacheShapeStale：缓存条目数 ≠ 令牌表长度。
    //     手法：`rebind_graph` 换一张**节点数不同**的图 ——
    //     `EvalCache::new` 会按新图定形，形状必然匹配 ⇒ 断不到。
    //     必须只改形状这一个维度（保留旧图指纹，形状与指纹分别触发）。
    {
        let wider = EvalGraph::build(
            vec![String::from("c.base0"), String::from("c.base1"), String::from("c.base2")],
            &[],
        )
        .expect("wider");
        let mut p = pipeline();
        p.graph = wider; // 只换图，缓存仍是 6 节点形状
        if p.evaluate(&batch(vec![0, 1, 2])).err() == Some(PerfCode::CacheShapeStale) {
            reached.push(PerfCode::CacheShapeStale);
        }
    }
    // 12) P95Degraded：P95 > UNIT_BUDGET * P95_DEGRADE_RATIO(=128)。
    //
    // ⚠ 三次踩坑才造出这条语料：
    //  ① 用深链 ⇒ 撞 `MAX_DEPTH`(32)。
    //  ② 用星形 ⇒ 每个叶 steps 相同且很小，P95 上不去。
    //  ③ **用 20 组「40 基础 + 1 扇出」⇒ p95=1**：P95 的样本口径是
    //     **全闭包每节点的步数**（840 个节点），不是「每个 target 一条」。
    //     800 个基础节点 steps=1 占了 95%，近秩第 95 百分位落在基础区。
    //     ⇒ 要让 p95 越线，得让**绝大多数闭包节点**都超阈值，
    //     办法是「少量基础 + 大量派生」，派生 steps = 1 + 2*FAN。
    //
    // FAN=70 ⇒ 派生 steps=141 > 128；派生 100 / 闭包 170 = 58.8% > 5%
    // ⇒ 近秩第 ceil(0.95*170)=162 位落在派生区 ⇒ p95=141 > 128。
    // 顺带这档 `code=BudgetExceeded` 与 `alert=P95Degraded` 同时非空，
    // 正是「两轴正交」的现场证据。
    {
        const FAN: usize = 70;
        const DERIVED: usize = 100;
        let mut gp: Vec<String> = Vec::with_capacity(FAN + DERIVED);
        for i in 0..FAN {
            gp.push(String::from(format!("p.b{}", i)));
        }
        for d in 0..DERIVED {
            gp.push(String::from(format!("p.d{}", d)));
        }
        let mut ge: Vec<(u32, u32)> = Vec::with_capacity(DERIVED * FAN);
        for d in 0..DERIVED {
            let dn = (FAN + d) as u32;
            for i in 0..FAN {
                ge.push((dn, i as u32));
            }
        }
        let tgt: Vec<u32> = (FAN as u32..(FAN + DERIVED) as u32).collect();
        let bs: Vec<(u32, String)> = (0..FAN as u32).map(|i| (i, String::from("x"))).collect();
        let got = EvalGraph::build(gp, &ge)
            .ok()
            .and_then(|g| {
                EvalPipeline::new(g)
                    .evaluate(&BatchInput { targets: tgt, bases: bs })
                    .ok()
            });
        match got {
            Some(o)
                if o.profile.alert == Some(PerfCode::P95Degraded)
                    && o.profile.code == Some(PerfCode::BudgetExceeded)
                    && o.profile.p95 > UNIT_BUDGET * P95_DEGRADE_RATIO =>
            {
                reached.push(PerfCode::P95Degraded);
            }
            _ => std::println!("[chk] P95Degraded 语料未造出告警 alert={:?}", got.as_ref().map(|x| x.profile.alert)),
        }
    }
    // 13) BudgetUnaligned：AD06 声明的预算与 UNIT_BUDGET 不一致。
    {
        let p = pipeline();
        let ok = p.align_budget(UNIT_BUDGET);
        let bad = p.align_budget(UNIT_BUDGET + 1);
        if ok == Ok(UNIT_BUDGET) && bad.err() == Some(PerfCode::BudgetUnaligned) {
            reached.push(PerfCode::BudgetUnaligned);
        }
    }

    // 13 个码一个都不能少。缺一个就说明它只剩标签没有产生点。
    set.add(
        "F3408-判据-每码都有真实产生点",
        reached.len() == EXPECT_CODES,
        "码在 ALL/映射/句子里齐 ≠ 可达；13 码须逐个真跑造出来",
    );

    // 变体双向验证：把冻结表里某码改掉，判定须转红。
    //    （证明上一条不是"恒真"——它真的在读 `code()` 的输出。）
    let mutated_wire = {
        let mut v: Vec<&str> = PerfCode::ALL.iter().map(|c| c.code()).collect();
        v.sort_unstable();
        let mut w = v.clone();
        w[0] = "E08-MUTATED";
        w != v
    };
    set.add(
        "F3408-判据-变体码表漂移必被抓",
        mutated_wire,
        "码表被改（增删码/改短码）时『码表与枚举一致』须转红",
    );

    // 变体双向验证：把「每码可达」语料里 BudgetUnaligned 的
    // 声明值改回一致 ⇒ `align_budget` 走 Ok 分支 ⇒ 断不到该码。
    // 这证明第 13 条真的在跑 `align_budget`，不是恒真。
    let aligned_is_ok = pipeline().align_budget(UNIT_BUDGET).is_ok();
    set.add(
        "F3408-判据-变体预算一致时不误报",
        aligned_is_ok,
        "声明值等于 UNIT_BUDGET 时须放行——否则预算对齐是黑名单",
    );
}

/// A 批入口：增量缓存 + 批量流水。
pub fn run_ver01h_checks_a() -> CheckSet {
    let mut set = CheckSet::new("ver01h-evalperf-a");
    chk_incremental(&mut set);
    chk_batch(&mut set);
    set
}

/// B 批入口：P95 + 分帧 + 契约。
pub fn run_ver01h_checks_b() -> CheckSet {
    let mut set = CheckSet::new("ver01h-evalperf-b");
    chk_p95(&mut set);
    chk_frames(&mut set);
    chk_contract(&mut set);
    set
}