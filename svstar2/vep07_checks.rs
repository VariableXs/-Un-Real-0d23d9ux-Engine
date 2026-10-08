//! VE-F3007 · 动效组合与编排图判据
//!
//! **锚点判据（原文）**：三模式、DSL 冻结、仲裁可预期、整体坍缩、图单源、判据。
//!
//! # 一、判据怎么做到"不是恒真"
//!
//! 组合最容易写成恒真断言的地方有五处，本模块逐一封死：
//!
//! 1. **"三模式编译规则不同"**。若只断「组合后节点数 = A+B」，一个「三模式全用
//!    `AfterComplete` 连所有节点」的实现照样绿——节点数对得上，但时序全错。
//!    必须**逐模式断边类型**：Relay 引入的边必须全是 `AfterComplete`、Overlay 必须
//!    含 `Parallel`、Decorate 的 B 侧**时长必须恒0**。
//! 2. **"DSL 冻结"**。若语料里只有正确写法，判据恒绿。必须造**六类偏离语料**：
//!    少参、多参、缺右括号、段内有空格、模式不在冻结集、结果名撞既有图——
//!    每类都要拿到专属错误码。
//! 3. **"仲裁可预期"**。这是本判据最关键的一条。若只断「赢家优先级最高」，一个
//!    「同优先级时按**图节点 ID** 决定」的实现在语料里也能过。必须**打乱输入顺序
//!    重放**：同一组声明换个 `claim` 调用顺序，裁决结果必须逐位相同。
//! 4. **"整体坍缩"**。若只断「reduce 下时长为 0」，一个「只把 A 侧归零、B 侧照跑」
//!    的实现能骗过部分语料。必须**两棵树都非空**，且断 B 侧实例时长也归零。
//! 5. **"图单源"**。必须断组合产物仍是 `OrchGraph`（能直接交给 F3005 的 `compile`），
//!    而不是在组合层另建一套结构后自己编译。
//!
//! # 二、方向：被拒才是合格
//!
//! 三条编译规则的**成功路径**也断，但重点在**该拒的确实拒了、拒绝时给出可定位信息**。
//! 一个「什么都拒」的实现骗不过副作用断言（节点数守恒、边类型分布）。

use crate::checks::CheckSet;
use crate::svstar2::vep03_token::Lane;
use crate::svstar2::vep05_orch::{compile, CompileParams, EdgeKind};
use crate::svstar2::vep07_compose::*;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 辅助
// ---------------------------------------------------------------------------

/// 编排图别名（显式写出便于空图构造）。
pub type OrchGraphAlias = crate::svstar2::vep05_orch::OrchGraph;

/// 图构造失败时的空图兜底（判据层不留 panic 面）。
fn empty_graph() -> OrchGraphAlias {
    OrchGraphAlias {
        nodes: Vec::new(),
        edges: Vec::new(),
        topological: Vec::new(),
        nest_depth: 0,
        parallel_groups: Vec::new(),
    }
}

/// 取一条两节点图；失败退到空图。
fn g(prefix: &str, dur: u32) -> OrchGraphAlias {
    match demo_graph(prefix, dur) {
        Ok(x) => x,
        Err(_) => empty_graph(),
    }
}

/// 取一个图；失败退到空图。
fn g_or_empty(r: Result<OrchGraphAlias, crate::svstar2::vep03_token::MotionTokenError>) -> OrchGraphAlias {
    match r {
        Ok(x) => x,
        Err(_) => empty_graph(),
    }
}

/// 取错误码（泛化：任意 `Result<T, MotionTokenError>` 都可取其 `code`）。
fn code_of<T>(r: Result<T, crate::svstar2::vep03_token::MotionTokenError>) -> &'static str {
    match r {
        Ok(_) => "<no-error>",
        Err(e) => e.code,
    }
}

// ---------------------------------------------------------------------------
// 判据一：三模式（锚点判据 1）
// ---------------------------------------------------------------------------

fn chk_three_modes(cs: &mut CheckSet) {
    // 模式全集与往返互逆。
    let rt = (0..MODE_COUNT).all(|i| ComposeMode::from_index(i).map(|m| m.index() == i).unwrap_or(false));
    cs.add("E07-三模式-索引往返互逆", rt, "三模式索引须恒等");

    // 三模式短码互异（防模式表塌缩）。
    let wires = [ComposeMode::Relay.wire(), ComposeMode::Overlay.wire(), ComposeMode::Decorate.wire()];
    let uniq = wires[0] != wires[1] && wires[1] != wires[2] && wires[0] != wires[2];
    cs.add("E07-三模式-短码互异", uniq, "三模式短码须互异");

    let ga = g("ra", 100);
    let gb = g("rb", 50);

    // 逐模式编译，逐条断**节点守恒**（A 2 + B 2 = 4）。
    let mut i = 0usize;
    let mut node_ok = true;
    while i < MODE_COUNT {
        let m = ComposeMode::from_index(i);
        match m {
            Some(mode) => {
                if let Ok(c) = compile_compose("c", mode, &ga, &gb) {
                    if c.actual_nodes() != 4 || c.total_nodes() != 4 {
                        node_ok = false;
                    }
                } else {
                    node_ok = false;
                }
            }
            None => node_ok = false,
        }
        i += 1;
    }
    cs.add("E07-三模式-节点守恒A加B", node_ok, "每模式组合后节点数须为 A2+B2=4");

    // 逐模式边类型钉死（**关键**：防"三模式都用同一边"）。
    // Relay：A 全部节点 → AfterComplete 边；Overlay：含 Parallel 边；
    // Decorate：B 侧时长恒 0。
    let relay_edges_right = match compile_compose("c", ComposeMode::Relay, &ga, &gb) {
        Ok(c) => {
            let cross: Vec<EdgeKind> = c
                .graph
                .edges
                .iter()
                .filter(|e| e.from < c.b_offset && e.to >= c.b_offset)
                .map(|e| e.kind)
                .collect();
            // A 有 2 节点 ⇒ 跨图边应恰好 2 条且全是 AfterComplete。
            cross.len() == 2 && cross.iter().all(|k| *k == EdgeKind::AfterComplete)
        }
        Err(_) => false,
    };
    cs.add(
        "E07-三模式-接力边全为前置完成",
        relay_edges_right,
        "Relay 跨图边须恰 2 条且全为 AfterComplete",
    );

    let overlay_has_parallel = match compile_compose("c", ComposeMode::Overlay, &ga, &gb) {
        Ok(c) => c.graph.edges.iter().any(|e| e.kind == EdgeKind::Parallel),
        Err(_) => false,
    };
    cs.add(
        "E07-三模式-并行含平行边",
        overlay_has_parallel,
        "Overlay 须含 Parallel 边",
    );

    // Decorate：B 侧时长恒 0（修饰不改变主体时序）。
    let decorate_zero = match compile_compose("c", ComposeMode::Decorate, &ga, &gb) {
        Ok(c) => {
            c.graph
                .nodes
                .iter()
                .filter(|n| n.id >= c.b_offset)
                .all(|n| n.duration_ms == 0)
                && c.graph
                    .nodes
                    .iter()
                    .filter(|n| n.id < c.b_offset)
                    .any(|n| n.duration_ms > 0)
        }
        Err(_) => false,
    };
    cs.add(
        "E07-三模式-修饰侧时长归零",
        decorate_zero,
        "Decorate 的 B 侧时长须恒 0，A 侧保持非零",
    );

    // B 图 ID 整体平移（两图节点 0 不得撞成同一节点）。
    let offset_ok = match compile_compose("c", ComposeMode::Overlay, &ga, &gb) {
        Ok(c) => {
            c.b_offset == 2 && c.graph.nodes.iter().any(|n| n.id == 2) && c.graph.nodes.iter().any(|n| n.id == 3)
        }
        Err(_) => false,
    };
    cs.add("E07-三模式-B节点ID整体平移", offset_ok, "B 节点须平移到 2/3，不得与 A 撞号");

    // 空图组合须拒（两侧分别试）。
    let empty_reject = code_of(compile_compose("c", ComposeMode::Relay, &empty_graph(), &gb))
        == E_COMPOSE_EMPTY
        || code_of(compile_compose("c", ComposeMode::Relay, &empty_graph(), &gb)) == E_NODE_RANGE;
    let empty_reject2 = code_of(compile_compose("c", ComposeMode::Relay, &ga, &empty_graph()))
        == E_COMPOSE_EMPTY
        || code_of(compile_compose("c", ComposeMode::Relay, &ga, &empty_graph())) == E_NODE_RANGE;
    cs.add(
        "E07-三模式-空图组合被拒",
        empty_reject && empty_reject2,
        "A 或 B 为空须被拒",
    );
}

// ---------------------------------------------------------------------------
// 判据二：DSL 冻结（锚点判据 2）
// ---------------------------------------------------------------------------

fn chk_dsl_frozen(cs: &mut CheckSet) {
    // 语法全集条目非空且互异（冻结表自身要钉死）。
    let gw = DSL_GRAMMAR;
    let gw_uniq = gw[0] != gw[1] && gw[1] != gw[2] && gw[2] != gw[3] && gw[0] != gw[3];
    cs.add("E07-DSL-语法表非空互异", gw_uniq && gw[0] == "compose", "语法表须含 compose 且条目互异");

    // 正确语料⇒成功（三种模式各一）。
    let mut p = DslParser::new();
    let _ = p.declare("A");
    let _ = p.declare("B");
    let ok_relay = p.parse_compose("compose(C1,A,B,relay)");
    let ok_overlay = p.parse_compose("compose(C2,A,B,overlay)");
    let ok_dec = p.parse_compose("compose(C3,A,B,decorate)");
    let three_ok = ok_relay.is_ok() && ok_overlay.is_ok() && ok_dec.is_ok();
    cs.add("E07-DSL-三模式语法均可解析", three_ok, "relay/overlay/decorate 三条正确语料须解析成功");

    // 声明序进入 decl.seq（仲裁"最近优先"的来源）。
    let seq_ok = p
        .decls()
        .iter()
        .map(|d| d.seq)
        .collect::<Vec<u32>>()
        == vec![0, 1, 2];
    cs.add("E07-DSL-声明序单调递增", seq_ok, "三条声明的 seq 须为 0/1/2");

    // 六类偏离语料逐条被拒，且拿到专属码。
    let mut q = DslParser::new();
    let _ = q.declare("A");
    let _ = q.declare("B");

    // ① 少参（3 段）
    let e1 = q.parse_compose("compose(C,A,B)").err();
    cs.add(
        "E07-DSL-少参被拒",
        e1.as_ref().map(|e| e.code).unwrap_or("<none>") == E_DSL_SYNTAX,
        "三段参数须拒为 E_DSL_SYNTAX",
    );
    // ② 多参（5 段）
    let e2 = q.parse_compose("compose(C,A,B,relay,extra)").err();
    cs.add(
        "E07-DSL-多参被拒",
        e2.as_ref().map(|e| e.code).unwrap_or("<none>") == E_DSL_SYNTAX,
        "五段参数须拒为 E_DSL_SYNTAX",
    );
    // ③ 缺右括号
    let e3 = q.parse_compose("compose(C,A,B,relay").err();
    cs.add(
        "E07-DSL-缺右括号被拒",
        e3.as_ref().map(|e| e.code).unwrap_or("<none>") == E_DSL_SYNTAX,
        "缺右括号须拒",
    );
    // ④ 段内空格
    let e4 = q.parse_compose("compose(C,A, B,relay)").err();
    cs.add(
        "E07-DSL-段内空格被拒",
        e4.as_ref().map(|e| e.code).unwrap_or("<none>") == E_DSL_SYNTAX,
        "段内空格须拒（语法冻结不接受空白）",
    );
    // ⑤ 模式不在冻结集（专属码 E_DSL_MODE，不是语法码）
    let e5 = q.parse_compose("compose(C,A,B,sequential)").err();
    cs.add(
        "E07-DSL-非冻结模式被拒",
        e5.as_ref().map(|e| e.code).unwrap_or("<none>") == E_DSL_MODE,
        "sequential 不在冻结集，须拿 E_DSL_MODE",
    );
    // ⑥ 未知名引用（专属码 E_DSL_UNKNOWN）
    let e6 = q.parse_compose("compose(C,A,ZZZ,relay)").err();
    cs.add(
        "E07-DSL-未知名被拒",
        e6.as_ref().map(|e| e.code).unwrap_or("<none>") == E_DSL_UNKNOWN,
        "引用未声明图须拿 E_DSL_UNKNOWN",
    );

    // 诊断三要素齐备（现象/原因/下一步）——读屏可查。
    let three_part = e1
        .as_ref()
        .map(|e| !e.why.is_empty() && !e.next.is_empty() && !e.what.is_empty())
        .unwrap_or(false);
    cs.add("E07-DSL-诊断三要素齐备", three_part, "语法错诊断须含现象/原因/下一步");

    // 结果名撞既有图须拒。
    let mut r = DslParser::new();
    let _ = r.declare("A");
    let _ = r.declare("B");
    let _ = r.parse_compose("compose(C,A,B,relay)");
    let e7 = r.parse_compose("compose(C,A,B,overlay)").err();
    cs.add(
        "E07-DSL-结果名撞既有被拒",
        e7.as_ref().map(|e| e.code).unwrap_or("<none>") == E_COMPOSE_DUP,
        "结果名 C 已被占用，须拒",
    );

    // 图名重复声明须拒。
    let mut s = DslParser::new();
    let _ = s.declare("A");
    let e8 = s.declare("A").err();
    cs.add(
        "E07-DSL-图名重复被拒",
        e8.as_ref().map(|e| e.code).unwrap_or("<none>") == E_COMPOSE_DUP,
        "同名图重复声明须拒",
    );
}

// ---------------------------------------------------------------------------
// 判据三：仲裁可预期（锚点判据 3）
// ---------------------------------------------------------------------------

fn chk_arbitration(cs: &mut CheckSet) {
    let mc = |n: u32, p: &str, pri: i32, seq: u32| match PropClaim::new(n, p, pri, seq) {
        Ok(c) => Some(c),
        Err(_) => None,
    };

    // 优先级高者胜。
    let mut a1 = ConflictArbiter::new();
    if let (Some(x), Some(y)) = (mc(1, "opacity", 5, 0), mc(2, "opacity", 9, 1)) {
        a1.claim(x);
        a1.claim(y);
    }
    let v1 = a1.arbitrate();
    let hi_wins = v1.first().map(|v| v.winner == 2).unwrap_or(false);
    cs.add("E07-仲裁-优先级高者胜", hi_wins, "优先级 9 的节点 2 须胜出");

    // 同优先级⇒最近声明者胜（seq 大者）。
    let mut a2 = ConflictArbiter::new();
    if let (Some(x), Some(y)) = (mc(1, "blur", 5, 0), mc(2, "blur", 5, 1)) {
        a2.claim(x);
        a2.claim(y);
    }
    let v2 = a2.arbitrate();
    let recent_wins = v2.first().map(|v| v.winner == 2 && v.fallback_used).unwrap_or(false);
    cs.add("E07-仲裁-同优先级最近者胜", recent_wins, "seq 更大的节点 2 须胜出且标记兜底");

    // **可预期性关键判据**：打乱 claim 顺序重放，裁决须逐位相同。
    // 一个"按图节点 ID 决定"的实现在这里会露馅。
    let mut fwd = ConflictArbiter::new();
    if let (Some(a), Some(b), Some(c)) = (mc(3, "scale", 7, 0), mc(1, "scale", 7, 1), mc(2, "scale", 7, 2)) {
        fwd.claim(a);
        fwd.claim(b);
        fwd.claim(c);
    }
    let mut rev = ConflictArbiter::new();
    if let (Some(a), Some(b), Some(c)) = (mc(3, "scale", 7, 0), mc(1, "scale", 7, 1), mc(2, "scale", 7, 2)) {
        rev.claim(c);
        rev.claim(b);
        rev.claim(a);
    }
    let fv = fwd.arbitrate();
    let rv = rev.arbitrate();
    let order_independent = fv == rv;
    cs.add(
        "E07-仲裁-打乱输入序结果不变",
        order_independent,
        "正序与逆序登记同组声明，裁决须逐位相同",
    );

    // 三属性互不干扰（分组正确）。
    let mut a3 = ConflictArbiter::new();
    if let (Some(x), Some(y), Some(z)) = (
        mc(1, "opacity", 1, 0),
        mc(2, "opacity", 2, 1),
        mc(3, "blur", 1, 2),
    ) {
        a3.claim(x);
        a3.claim(y);
        a3.claim(z);
    }
    let v3 = a3.arbitrate();
    let grouped = v3.len() == 1 && v3[0].prop == "opacity" && v3[0].loser_count() == 1;
    cs.add("E07-仲裁-按属性分组只裁决冲突组", grouped, "blur 单声明不参与裁决，只裁决 opacity");

    // 已登记互不冲突属性对⇒不算冲突（不裁决）。
    let mut a4 = ConflictArbiter::new();
    a4.allow_pair("opacity", "transform");
    if let (Some(x), Some(y)) = (mc(1, "opacity", 1, 0), mc(2, "transform", 2, 1)) {
        a4.claim(x);
        a4.claim(y);
    }
    let v4 = a4.arbitrate();
    let no_conflict = v4.is_empty();
    cs.add(
        "E07-仲裁-已登记属性对免裁决",
        no_conflict && a4.pair_count() == 1,
        "登记 opacity>transform 后二者不裁决",
    );

    // 兜底时必须出诊断（默认可预期红线的可见面）。
    let mut a5 = ConflictArbiter::new();
    if let (Some(x), Some(y)) = (mc(1, "opacity", 5, 0), mc(2, "opacity", 5, 1)) {
        a5.claim(x);
        a5.claim(y);
    }
    let _ = a5.arbitrate();
    let diag = !a5.diagnostics().is_empty();
    cs.add("E07-仲裁-兜底出诊断", diag, "最近优先兜底须留诊断");

    // 播报含属性名与节点（读屏可查）。
    let sp = a5.spoken();
    cs.add(
        "E07-仲裁-播报含属性与节点",
        sp.contains("opacity"),
        "仲裁播报须含属性名",
    );

    // 属性名非法须拒（专属码）。
    let e = PropClaim::new(1, "", 1, 0).err();
    cs.add(
        "E07-仲裁-非法属性名被拒",
        e.as_ref().map(|x| x.code).unwrap_or("<none>") == E_PROP_CONFLICT,
        "空属性名须拒",
    );
}

// ---------------------------------------------------------------------------
// 判据四：整体坍缩 + 图单源（锚点判据 4、5）
// ---------------------------------------------------------------------------

fn chk_collapse_single_source(cs: &mut CheckSet) {
    let ga = g("ca", 100);
    let gb = g("cb", 80);

    // 图单源：组合产物能直接交给 F3005 的 compile。
    let compiled_ok = match compile_compose("C", ComposeMode::Overlay, &ga, &gb) {
        Ok(c) => compile(&c.graph, &CompileParams::normal(Lane::Normal)).is_ok(),
        Err(_) => false,
    };
    cs.add(
        "E07-单源-组合图可直接编译",
        compiled_ok,
        "组合产物须能被 F3005 compile 直接消费",
    );

    // 常规泳道：实例数 = A 2 + B 2 = 4，总时长非零。
    let normal_ok = match compile_compose("C", ComposeMode::Overlay, &ga, &gb) {
        Ok(c) => {
            let mut arb = ConflictArbiter::new();
            let e = eval_compose(&c, &mut arb, Lane::Normal);
            e.instance_count() == 4 && e.total_duration_ms() > 0
        }
        Err(_) => false,
    };
    cs.add("E07-单源-常规泳道四实例", normal_ok, "常规泳道须编译出 4 实例且时长非零");

    // **整体坍缩**（关键）：reduce 下所有实例时长 0 —— 两侧都归零，不允许部分活动。
    let collapse_ok = match compile_compose("C", ComposeMode::Overlay, &ga, &gb) {
        Ok(c) => {
            let mut arb = ConflictArbiter::new();
            let e = eval_compose(&c, &mut arb, Lane::Reduced);
            // 不只是"总时长为 0"，而是**逐实例**都为 0（防"只归零一侧"）。
            e.compiled
                .instances
                .iter()
                .all(|i| i.duration_ms == 0 && i.offset_in_stage_ms == 0)
                && e.compiled.instances.len() == 4
        }
        Err(_) => false,
    };
    cs.add(
        "E07-坍缩-reduce逐实例全归零",
        collapse_ok,
        "reduce 泳道须 4 个实例时长与偏移全为 0（不允许部分坍缩）",
    );

    // 仲裁接入求值：冲突经仲裁器裁决并进结果。
    let arb_ok = match compile_compose("C", ComposeMode::Overlay, &ga, &gb) {
        Ok(c) => {
            let mut arb = ConflictArbiter::new();
            if let (Some(x), Some(y)) = (
                PropClaim::new(1, "opacity", 1, 0).ok(),
                PropClaim::new(2, "opacity", 2, 1).ok(),
            ) {
                arb.claim(x);
                arb.claim(y);
            }
            let e = eval_compose(&c, &mut arb, Lane::Normal);
            e.has_conflict() && e.fallback_count() == 0
        }
        Err(_) => false,
    };
    cs.add(
        "E07-求值-仲裁结果接入",
        arb_ok,
        "优先级不同不算兜底，但须裁决出冲突",
    );

    // 组合视图：行数等于节点数，修饰侧时长为 0。
    let view_ok = match compile_compose("C", ComposeMode::Decorate, &ga, &gb) {
        Ok(c) => {
            let mut arb = ConflictArbiter::new();
            let e = eval_compose(&c, &mut arb, Lane::Normal);
            let v = build_view(&c, &e);
            v.row_count() == 4
                && v.rows.iter().filter(|r| r.side == 1).all(|r| r.duration_ms == 0)
                && v.rows.iter().any(|r| r.side == 0 && r.duration_ms > 0)
        }
        Err(_) => false,
    };
    cs.add(
        "E07-视图-行数与侧别正确",
        view_ok,
        "组合视图行数 4，修饰侧时长全 0",
    );
}

// ---------------------------------------------------------------------------
// 判据五：环检测 + 深度上限 + 契约纪律
// ---------------------------------------------------------------------------

fn chk_cycle_contract(cs: &mut CheckSet) {
    // 链含重复⇒环（A 修饰 B 修饰 A）。
    let cyc: Vec<String> = vec![
        String::from("A"),
        String::from("B"),
        String::from("A"),
    ];
    let no_parent = |_n: &str| -> Option<String> { None };
    let e_cycle = verify_chain(&cyc, &no_parent).err();
    cs.add(
        "E07-环-循环组合被拒",
        e_cycle.as_ref().map(|e| e.code).unwrap_or("<none>") == E_COMPOSE_CYCLE,
        "A→B→A 须拒为 E_COMPOSE_CYCLE",
    );
    // 环诊断含环链（三要素）。
    let cyc_msg = e_cycle.as_ref().map(|e| e.why.contains('→')).unwrap_or(false);
    cs.add("E07-环-诊断含环链", cyc_msg, "环诊断须给出环上名字链");

    // 正常链不被误拒（防"一律拒"）。
    let ok_chain: Vec<String> = vec![String::from("X"), String::from("Y")];
    let ok = verify_chain(&ok_chain, &no_parent);
    cs.add("E07-环-正常链通过", ok.is_ok(), "无重复链须通过");

    // 深度超限⇒拒（复用 F3005 的 MAX_NEST_DEPTH=8）。
    let deep: Vec<String> = (0..(MAX_CHAIN + 2))
        .map(|i| format!("n{}", i))
        .collect();
    let e_deep = verify_chain(&deep, &no_parent).err();
    cs.add(
        "E07-环-深度超限被拒",
        e_deep.as_ref().map(|e| e.code).unwrap_or("<none>") == E_COMPOSE_DEPTH,
        "链长超 MAX_NEST_DEPTH 须拒",
    );

    // 恰好等于上限⇒通过（边界不误拒）。
    let edge: Vec<String> = (0..MAX_CHAIN).map(|i| format!("n{}", i)).collect();
    cs.add(
        "E07-环-深度恰等于上限通过",
        verify_chain(&edge, &no_parent).is_ok(),
        "链长恰等于上限须通过",
    );

    // 三协议版本非空互异。
    let vers = [
        COMPOSE_PROTOCOL_VERSION,
        DSL_PROTOCOL_VERSION,
        ARBITRATION_PROTOCOL_VERSION,
    ];
    cs.add(
        "E07-契约-三协议版本非空互异",
        vers.iter().all(|v| !v.is_empty()) && vers[0] != vers[1] && vers[1] != vers[2],
        "组合/DSL/仲裁协议版本须非空互异",
    );

    // 诊断码逐条互异。
    const MY_CODES: [&str; 10] = [
        E_DSL_SYNTAX,
        E_DSL_MODE,
        E_DSL_UNKNOWN,
        E_COMPOSE_EMPTY,
        E_COMPOSE_CYCLE,
        E_COMPOSE_DEPTH,
        E_PROP_CONFLICT,
        E_DECORATE_DURATION,
        E_NODE_RANGE,
        E_COMPOSE_DUP,
    ];
    let mut uniq = true;
    let mut i = 0usize;
    while i < MY_CODES.len() {
        let mut j = i + 1;
        while j < MY_CODES.len() {
            if MY_CODES[i] == MY_CODES[j] {
                uniq = false;
            }
            j += 1;
        }
        i += 1;
    }
    cs.add("E07-契约-诊断码逐条互异", uniq, "组合域 10 个诊断码不得重复");

    // 模式表序号由常量推导（判据索引不硬编）。
    let derived = ComposeMode::Relay.index() == 0
        && ComposeMode::Overlay.index() == 1
        && ComposeMode::Decorate.index() == 2
        && MODE_COUNT == 3;
    cs.add("E07-契约-模式序号由常量推导", derived, "三模式序号须 0/1/2 且 MODE_COUNT=3");

    // 边类型映射逐条钉死（三模式映射表本身是契约）。
    let map_ok = ComposeMode::Relay.edge_kind() == EdgeKind::AfterComplete
        && ComposeMode::Overlay.edge_kind() == EdgeKind::Parallel
        && ComposeMode::Decorate.edge_kind() == EdgeKind::AfterComplete;
    cs.add(
        "E07-契约-模式边类型映射固定",
        map_ok,
        "Relay/Decorate⇒AfterComplete，Overlay⇒Parallel",
    );

    // 视图播报含模式与两侧计数（读屏可查）。
    let spoken_ok = match compile_compose("C", ComposeMode::Relay, &g("va", 60), &g("vb", 60)) {
        Ok(c) => {
            let mut arb = ConflictArbiter::new();
            let e = eval_compose(&c, &mut arb, Lane::Normal);
            let v = build_view(&c, &e);
            let s = v.spoken();
            s.contains("relay") && s.contains("节点")
        }
        Err(_) => false,
    };
    cs.add("E07-契约-视图播报含模式", spoken_ok, "视图播报须含模式短码与节点数");
}

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

/// A 批：三模式 + DSL 冻结。
pub fn run_vep07_checks_a() -> CheckSet {
    let mut cs = CheckSet::new("vep07-compose-a");
    chk_three_modes(&mut cs);
    chk_dsl_frozen(&mut cs);
    cs
}

/// B 批：仲裁可预期 + 整体坍缩 + 图单源 + 环与契约。
pub fn run_vep07_checks_b() -> CheckSet {
    let mut cs = CheckSet::new("vep07-compose-b");
    chk_arbitration(&mut cs);
    chk_collapse_single_source(&mut cs);
    chk_cycle_contract(&mut cs);
    cs
}