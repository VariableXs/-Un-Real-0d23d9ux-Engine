//! VE-F3409 · 令牌调试工具 —— 域判据层。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3409`
//!
//! # 判据映射（锚点五条 + 纪律）
//!
//! - **反查链路**（9 条）→ 落账/双向反查/插入序/去重/会话隔离/无结果提示/非法拒；
//! - **求值追踪**（8 条）→ 闭包完整/节点事实/缓存翻转/拓扑序/依赖完整/超预算降级/守恒；
//! - **来源视图**（7 条）→ 投影一致/漂移检出/校准闭环/无漂移对照/撤销漂移/会话归因/指纹双向；
//! - **会话统一**（4 条）→ 绑定校验/越界拒/对票 mismatch/跨会话数据不混；
//! - **判据与契约**（8 条）→ 七码唯一/三要素/阻断与降级分清/码表一致/每码可达/契约冻结/逐位读屏/变体。
//!
//! # 判据设计纪律
//!
//! 1. 期望值由语料结构与图拓扑**独立重算**，不读被测字段当期望；
//! 2. 每条「必被抓」判据配变体双向验证，变体只在目标维度不同；
//! 3. 七个诊断码逐个**真跑造出来**（有短码 ≠ 可达）；
//! 4. 判据区零 panic 面：越界一律 match/`.get()` 记红。

use crate::checks::CheckSet;
use crate::svstar2::ver01h_evalperf::{BatchInput, BatchResult, EvalGraph, EvalPipeline};
use crate::svstar2::ver01i_debugtool::*;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 判据侧常量与语料（不从被测常量推导）
// ---------------------------------------------------------------------------

const EXPECT_CODES: usize = 7;
const EXPECT_SOURCES: usize = 5;

/// 语料图：菱形。b0..b2 基础；d0 依赖 b0+b1，d1 依赖 b0，d2 依赖 b1+b2。
fn paths() -> Vec<String> {
    ["g.b0", "g.b1", "g.b2", "g.d0", "g.d1", "g.d2"]
        .iter()
        .map(|s| String::from(*s))
        .collect()
}

fn edges() -> Vec<(u32, u32)> {
    vec![(3, 0), (3, 1), (4, 0), (5, 1), (5, 2)]
}

fn bases() -> Vec<(u32, String)> {
    vec![
        (0, String::from("a")),
        (1, String::from("b")),
        (2, String::from("c")),
    ]
}

fn build() -> EvalGraph {
    match EvalGraph::build(paths(), &edges()) {
        Ok(g) => g,
        Err(_) => panic!("语料图应可建"),
    }
}

/// 全批求值一次（targets = 全部节点）。
fn run_batch() -> (EvalGraph, BatchResult) {
    let g = build();
    let mut p = EvalPipeline::new(g);
    let tg: Vec<u32> = (0..6).collect();
    match p.evaluate(&BatchInput {
        targets: tg,
        bases: bases(),
    }) {
        Ok(r) => (p.graph, r),
        Err(_) => panic!("语料批应可求值"),
    }
}

// ---------------------------------------------------------------------------
// 一、反查链路（检查器）
// ---------------------------------------------------------------------------

fn chk_inspector(set: &mut CheckSet) {
    // 会话绑定：合法绑定 + 号往返。
    let s1 = match Session::bind("sess-A") {
        Ok(s) => s,
        Err(_) => {
            set.fail("F3409-反查-会话绑定", "bind 报错");
            return;
        }
    };
    set.add(
        "F3409-反查-会话绑定",
        s1.id() == "sess-A",
        "合法会话须绑定成功且号往返一致",
    );

    let mut ins = TokenInspector::new();

    // 落账：elem0→tok0, elem0→tok1, elem1→tok0。
    let ok1 = ins.record_query(&s1, "ui.panel.title", "c.brand");
    let ok2 = ins.record_query(&s1, "ui.panel.title", "c.size");
    let ok3 = ins.record_query(&s1, "ui.card.title", "c.brand");
    set.add(
        "F3409-反查-落账合法",
        ok1.is_ok() && ok2.is_ok() && ok3.is_ok() && ins.log.len() == 3,
        "三条合法查询须全落账（账本长度独立核对 = 3）",
    );

    // 元素反查令牌链：插入序、去重。
    let chain = ins.chain_of_element(&s1, "ui.panel.title");
    let chain_expect: Vec<String> = vec![String::from("c.brand"), String::from("c.size")];
    set.add(
        "F3409-反查-元素查令牌链",
        match chain {
            Ok(c) => c == chain_expect,
            Err(_) => false,
        },
        "元素反查须按插入序给出令牌链 [c.brand, c.size]",
    );

    // 令牌反查元素：c.brand 被 ui.panel.title 与 ui.card.title 使用。
    let users = ins.elements_of_token(&s1, "c.brand");
    let users_expect: Vec<String> =
        vec![String::from("ui.panel.title"), String::from("ui.card.title")];
    set.add(
        "F3409-反查-令牌查元素表",
        match users {
            Ok(u) => u == users_expect,
            Err(_) => false,
        },
        "令牌反查须给出全部使用元素，插入序",
    );

    // 反查无结果 → 提示级 NotFound（降级矩阵：反查失败→提示）。
    let miss = ins.chain_of_element(&s1, "ui.unknown");
    set.add(
        "F3409-反查-无结果即提示",
        miss.err() == Some(TraceCode::NotFound) && !TraceCode::NotFound.blocking(),
        "反查失败须报 NotFound 且为提示级不阻断",
    );

    // 非法输入逐向拒（真跑造码）。
    let bad_elem = ins.record_query(&s1, "", "c.brand");
    let bad_tok = ins.record_query(&s1, "ui.x", "");
    set.add(
        "F3409-反查-非法输入逐向拒",
        bad_elem.err() == Some(TraceCode::ElementInvalid)
            && bad_tok.err() == Some(TraceCode::TokenInvalid),
        "空元素/空令牌须分别报对应码",
    );

    // 去重语义：重复落账账本照记，反查链去重。
    let _ = ins.record_query(&s1, "ui.panel.title", "c.brand");
    let dup_chain = ins.chain_of_element(&s1, "ui.panel.title");
    set.add(
        "F3409-反查-链去重账不假去重",
        ins.log.len() == 4
            && match dup_chain {
                Ok(c) => c.len() == 2,
                Err(_) => false,
            },
        "重复查询账本记 4 条（事实不丢），反查链仍 2 项（投影去重）",
    );

    // 会话统一：他会话看不到本会话的账。
    let s2 = match Session::bind("sess-B") {
        Ok(s) => s,
        Err(_) => {
            set.fail("F3409-反查-会话隔离", "bind s2 报错");
            return;
        }
    };
    let cross = ins.chain_of_element(&s2, "ui.panel.title");
    set.add(
        "F3409-反查-会话隔离",
        cross.err() == Some(TraceCode::NotFound),
        "他会话反查本会话的元素须视同无记录（调试数据不混会话）",
    );

    // 会话号越界拒（真跑造码 SessionUnbound）。
    let long = "x".repeat(SESSION_MAX + 1);
    let too_long = Session::bind(&long);
    let empty = Session::bind("");
    set.add(
        "F3409-会话-越界即拒",
        too_long.err() == Some(TraceCode::SessionUnbound)
            && empty.err() == Some(TraceCode::SessionUnbound)
            && TraceCode::SessionUnbound.blocking(),
        "空/超长会话号须拒且为阻断级（X03 对接点）",
    );

    // 变体双向验证：把链期望打乱须与实测不同（否则上链判据恒真）。
    let mut flipped = chain_expect.clone();
    flipped.reverse();
    let real = ins.chain_of_element(&s1, "ui.panel.title").ok();
    set.add(
        "F3409-反查-变体倒序必被抓",
        real.is_some() && real.as_ref().map(|r| r.as_slice()) != Some(flipped.as_slice()),
        "倒序期望须与实测不同——相同则语料不可区分顺序",
    );
}

// ---------------------------------------------------------------------------
// 二、求值追踪
// ---------------------------------------------------------------------------

fn chk_tracer(set: &mut CheckSet) {
    let (graph, result) = run_batch();
    let s1 = match Session::bind("sess-T") {
        Ok(s) => s,
        Err(_) => {
            set.fail("F3409-追踪-会话绑定", "bind 报错");
            return;
        }
    };
    let tr = EvalTracer::new();

    // 闭包独立重算：d0 的闭包 = {d0} ∪ refs(d0)={b0,b1}，无更深依赖 ⇒ 3 节点。
    // （d1/d2 不依赖 d0，不在闭包里；判据侧按图边独立推导，不读被测。）
    const D0_CLOSURE: usize = 3;
    let t = match tr.trace(&s1, &graph, &result, 3) {
        Ok(t) => t,
        Err(_) => {
            set.fail("F3409-追踪-闭包完整", "trace 报错");
            return;
        }
    };
    set.add(
        "F3409-追踪-闭包完整",
        t.nodes.len() == D0_CLOSURE
            && t.target == "g.d0"
            && t.session == "sess-T"
            && !t.truncated,
        "d0 追踪须覆盖闭包 3 节点且带目标与会话号",
    );

    // 拓扑序：层级非降，且恰含 level 0 ×2 + level 1 ×1（判据侧独立数边）。
    let mut level_ok = true;
    for w in t.nodes.windows(2) {
        if w[0].level > w[1].level {
            level_ok = false;
        }
    }
    let lv0 = t.nodes.iter().filter(|n| n.level == 0).count();
    let lv1 = t.nodes.iter().filter(|n| n.level == 1).count();
    set.add(
        "F3409-追踪-拓扑序",
        level_ok && lv0 == 2 && lv1 == 1,
        "过程须按层级非降排列（2 基础 + 1 派生）",
    );

    // 依赖完整性：d0 节点的 refs 判据侧独立推导 = [g.b0, g.b1]。
    let d0node = t.nodes.iter().find(|n| n.token == "g.d0");
    let refs_expect: Vec<String> = vec![String::from("g.b0"), String::from("g.b1")];
    set.add(
        "F3409-追踪-依赖完整",
        match d0node {
            Some(n) => n.refs == refs_expect,
            None => false,
        },
        "派生节点的依赖链须逐名完整（g.b0, g.b1）",
    );

    // 缓存翻转对照：同图第二次全批全命中 ⇒ cached 翻转。
    let mut p2 = EvalPipeline::new(build());
    let tg: Vec<u32> = (0..6).collect();
    let _ = p2.evaluate(&BatchInput {
        targets: tg.clone(),
        bases: bases(),
    });
    let second = match p2.evaluate(&BatchInput {
        targets: tg,
        bases: bases(),
    }) {
        Ok(r) => r,
        Err(_) => {
            set.fail("F3409-追踪-缓存翻转", "二次求值报错");
            return;
        }
    };
    let t2 = match tr.trace(&s1, &p2.graph, &second, 3) {
        Ok(t) => t,
        Err(_) => {
            set.fail("F3409-追踪-缓存翻转", "二次 trace 报错");
            return;
        }
    };
    let first_all_live = t.nodes.iter().all(|n| !n.cached);
    let second_all_cached = t2.nodes.iter().all(|n| n.cached);
    set.add(
        "F3409-追踪-缓存翻转",
        first_all_live && second_all_cached,
        "首追全为实际计算、二追全为缓存命中——追踪如实反映求值方式",
    );

    // 超预算降级：预算 2 < 闭包 3 ⇒ 截断留痕、不丢已展开。
    let tight = EvalTracer {
        budget: 2,
    };
    let tt = match tight.trace(&s1, &graph, &result, 3) {
        Ok(t) => t,
        Err(_) => {
            set.fail("F3409-追踪-超预算降级", "紧预算 trace 报错");
            return;
        }
    };
    set.add(
        "F3409-追踪-超预算降级",
        tt.truncated && tt.withheld == 1 && tt.nodes.len() == 2,
        "预算 2 须展开 2 截留 1（降级矩阵：追踪开销→超预算降级）",
    );

    // 守恒：展开 + 截留 == 闭包（独立和）。
    set.add(
        "F3409-追踪-展开截留守恒",
        tt.nodes.len() + tt.withheld == D0_CLOSURE && t.nodes.len() + t.withheld == D0_CLOSURE,
        "展开数 + 截留数须恒等于闭包数（不丢节点）",
    );

    // 无效目标拒（真跑造码 TokenInvalid）。
    let bad = tr.trace(&s1, &graph, &result, 999);
    set.add(
        "F3409-追踪-无效目标即拒",
        bad.err() == Some(TraceCode::TokenInvalid) && TraceCode::TokenInvalid.blocking(),
        "越界目标须报 TokenInvalid 且阻断",
    );

    // 预算对齐（X03）：一致放行、不一致拒（真跑造码 TraceBudgetExceeded）。
    let aligned = tr.align_budget(MAX_TRACE_STEPS);
    let misaligned = tr.align_budget(MAX_TRACE_STEPS + 1);
    set.add(
        "F3409-追踪-预算对齐",
        aligned == Ok(MAX_TRACE_STEPS)
            && misaligned.err() == Some(TraceCode::TraceBudgetExceeded)
            && TraceCode::TraceBudgetExceeded.degradable()
            && !TraceCode::TraceBudgetExceeded.blocking(),
        "预算口径一致须放行并回实际值；不一致报降级码不静默收放",
    );

    // 变体：预算一致时不误报（否则对齐闸是黑名单）。
    set.add(
        "F3409-追踪-变体一致不误报",
        tr.align_budget(tr.budget).is_ok(),
        "声明值等于自身预算时须放行",
    );

    // 读屏：截断语料必须带「已截断」与 withheld 数。
    let sp = tt.spoken();
    set.add(
        "F3409-追踪-读屏可达",
        sp.contains("g.d0") && sp.contains("已截断") && sp.contains("求值追踪"),
        "追踪读屏须含目标、结论与截断事实",
    );
}

// ---------------------------------------------------------------------------
// 三、覆盖来源视图
// ---------------------------------------------------------------------------

fn chk_view(set: &mut CheckSet) {
    let s1 = match Session::bind("sess-V") {
        Ok(s) => s,
        Err(_) => {
            set.fail("F3409-视图-会话绑定", "bind 报错");
            return;
        }
    };
    let mut scope = SourceScope::new();

    // 新建即投影：空表空视图无漂移（对照，防「恒漂移」恒真）。
    set.add(
        "F3409-视图-新建无漂移",
        !scope.drifted() && scope.table.entries.is_empty(),
        "空表空视图须无漂移——否则漂移判据恒真",
    );

    // 落来源三笔后立即投影仍一致；随后改一笔 → 漂移。
    let a1 = scope.assign(&s1, "c.brand", SourceTag::Default);
    let a2 = scope.assign(&s1, "c.size", SourceTag::Theme);
    let a3 = scope.assign(&s1, "c.d0", SourceTag::Computed);
    let _ = scope.calibrate(); // 校准到三笔后的一致态
    set.add(
        "F3409-视图-落账合法",
        a1.is_ok() && a2.is_ok() && a3.is_ok() && scope.table.entries.len() == 3,
        "三笔来源须全落账",
    );
    let _ = scope.calibrate();
    set.add(
        "F3409-视图-投影一致",
        !scope.drifted(),
        "校准后表与视图指纹须一致",
    );

    // 改派 → 漂移 → 校准闭环（真跑造码 ViewDrifted 的产生事实）。
    let _ = scope.assign(&s1, "c.size", SourceTag::Component);
    let drifted_now = scope.drifted();
    let rep = scope.calibrate();
    set.add(
        "F3409-视图-漂移校准闭环",
        drifted_now
            && rep.drifted
            && rep.fixes == 1
            && !scope.drifted()
            && TraceCode::ViewDrifted.degradable()
            && !TraceCode::ViewDrifted.blocking(),
        "改派一笔须检出漂移、校准修复恰 1 条并回到一致（处置=校准非重画）",
    );

    // 撤销（remove）→「视图有、表无」漂移分支可达且被校准。
    let rm = scope.table.remove("c.brand");
    let drift2 = scope.drifted();
    let rep2 = scope.calibrate();
    set.add(
        "F3409-视图-撤销漂移可达",
        rm.is_ok() && drift2 && rep2.fixes == 1 && !scope.drifted(),
        "撤销指派须造成视图有表无的漂移并被校准（否则校准只对账改派，回收逃逸）",
    );

    // 撤销不存在的令牌 → NotFound（真跑造码）。
    let rm_miss = scope.table.remove("c.ghost");
    set.add(
        "F3409-视图-撤销不存在即提示",
        rm_miss.err() == Some(TraceCode::NotFound),
        "撤销不存在的指派须报 NotFound",
    );

    // 会话归因：touched_by 跟随最近改表会话（X03 可归因）。
    let s2 = match Session::bind("sess-V2") {
        Ok(s) => s,
        Err(_) => {
            set.fail("F3409-视图-会话归因", "bind s2 报错");
            return;
        }
    };
    let _ = scope.assign(&s1, "c.a", SourceTag::Default);
    let by1 = String::from(&scope.touched_by);
    let _ = scope.assign(&s2, "c.b", SourceTag::Scene);
    let by2 = String::from(&scope.touched_by);
    set.add(
        "F3409-视图-会话归因",
        by1 == "sess-V" && by2 == "sess-V2",
        "改表归因须跟随最近一次操作会话",
    );

    // 指纹口径双向：同表同指纹；任一来源变化指纹必变。
    let fp_before = scope.table.fingerprint();
    let fp_again = scope.table.fingerprint();
    let _ = scope.assign(&s2, "c.b", SourceTag::Theme);
    let fp_after = scope.table.fingerprint();
    set.add(
        "F3409-视图-指纹双向",
        fp_before == fp_again && fp_before != fp_after,
        "同表指纹须稳定；改一来源指纹必变（否则漂移检出靠碰运气）",
    );

    // 五级来源封闭全集：中文名互异非空。
    let zh: Vec<&str> = SourceTag::ALL.iter().map(|t| t.zh()).collect();
    let mut uniq = zh.clone();
    uniq.sort_unstable();
    uniq.dedup();
    set.add(
        "F3409-视图-来源封闭全集",
        SourceTag::ALL.len() == EXPECT_SOURCES && uniq.len() == EXPECT_SOURCES,
        "来源五级须封闭且中文名互异（读屏不歧义）",
    );

    // 视图读屏。
    let vp = scope.view.spoken();
    set.add(
        "F3409-视图-读屏可达",
        vp.contains("覆盖来源视图") && vp.contains("来源"),
        "视图读屏须逐条「令牌 = 来源」",
    );

    // 会话对票 mismatch（真跑造码 SessionMismatch）。
    let mismatch = s1.check("other-session");
    set.add(
        "F3409-会话-对票不匹配即拒",
        mismatch.err() == Some(TraceCode::SessionMismatch)
            && TraceCode::SessionMismatch.blocking(),
        "结果归属另一会话须拒绝呈报且阻断",
    );
}

// ---------------------------------------------------------------------------
// 四、判据与契约纪律
// ---------------------------------------------------------------------------

fn chk_contract(set: &mut CheckSet) {
    // 七码唯一。
    let mut codes: Vec<&str> = TraceCode::ALL.iter().map(|c| c.code()).collect();
    codes.sort_unstable();
    codes.dedup();
    set.add(
        "F3409-判据-七码唯一",
        codes.len() == EXPECT_CODES && TraceCode::ALL.len() == EXPECT_CODES,
        "错误码重复会让用户报号指不准",
    );

    // 三要素齐发。
    set.add(
        "F3409-判据-错误三要素齐发",
        TraceCode::ALL.iter().all(|c| {
            !c.code().is_empty() && !c.spoken().is_empty() && c.spoken().contains(c.code())
        }),
        "每个码须有短码与可读句子",
    );

    // 阻断/提示/降级分清：阻断恰 4（两非法+会话两闸），降级恰 2（预算/漂移）。
    let blocking = TraceCode::ALL.iter().filter(|c| c.blocking()).count();
    let degradable = TraceCode::ALL.iter().filter(|c| c.degradable()).count();
    let not_found_free = !TraceCode::NotFound.blocking() && !TraceCode::NotFound.degradable();
    set.add(
        "F3409-判据-阻断降级分清",
        blocking == 4 && degradable == 2 && not_found_free,
        "阻断 4 / 降级 2 / 提示 1，三档不得互相侵占",
    );

    // 码表与枚举一致（判据侧独立列出冻结表）。
    const EXPECT_WIRE: [&str; EXPECT_CODES] = [
        "E11-ELEMENT-INVALID",
        "E11-TOKEN-INVALID",
        "E11-SESSION-UNBOUND",
        "E11-NOT-FOUND",
        "E11-TRACE-BUDGET",
        "E11-VIEW-DRIFTED",
        "E11-SESSION-MISMATCH",
    ];
    let mut got: Vec<&str> = TraceCode::ALL.iter().map(|c| c.code()).collect();
    got.sort_unstable();
    let mut want: Vec<&str> = EXPECT_WIRE.to_vec();
    want.sort_unstable();
    set.add(
        "F3409-判据-码表与枚举一致",
        got == want,
        "ALL 须恰好列出 7 码且与冻结表逐字一致（E11 段独占）",
    );

    // 每码真实产生点：七个码逐个真跑造出来。
    let mut reached: Vec<TraceCode> = Vec::new();
    {
        let s = Session::bind("sess-R").ok();
        let has_s = s.is_some();
        // ElementInvalid
        if has_s {
            let mut i = TokenInspector::new();
            let sv = s.as_ref().unwrap();
            if i.record_query(sv, "", "t").err() == Some(TraceCode::ElementInvalid) {
                reached.push(TraceCode::ElementInvalid);
            }
            // TokenInvalid
            if i.record_query(sv, "e", "").err() == Some(TraceCode::TokenInvalid) {
                reached.push(TraceCode::TokenInvalid);
            }
        }
        // SessionUnbound
        if Session::bind("").err() == Some(TraceCode::SessionUnbound) {
            reached.push(TraceCode::SessionUnbound);
        }
        // NotFound
        if has_s {
            let sv = s.as_ref().unwrap();
            let i = TokenInspector::new();
            if i.chain_of_element(sv, "ui.none").err() == Some(TraceCode::NotFound) {
                reached.push(TraceCode::NotFound);
            }
        }
        // TraceBudgetExceeded
        if EvalTracer::new()
            .align_budget(MAX_TRACE_STEPS + 1)
            .err()
            == Some(TraceCode::TraceBudgetExceeded)
        {
            reached.push(TraceCode::TraceBudgetExceeded);
        }
        // ViewDrifted：改表不重投影即漂移
        {
            let mut sc = SourceScope::new();
            if let Some(sv) = s.as_ref() {
                let _ = sc.assign(sv, "c.t", SourceTag::Theme);
            }
            if sc.drifted() {
                reached.push(TraceCode::ViewDrifted);
            }
        }
        // SessionMismatch
        if let Ok(sv) = Session::bind("sess-R") {
            if sv.check("other").err() == Some(TraceCode::SessionMismatch) {
                reached.push(TraceCode::SessionMismatch);
            }
        }
    }
    set.add(
        "F3409-判据-每码真实产生点",
        reached.len() == EXPECT_CODES,
        "码在 ALL/映射/句子里齐 ≠ 可达；7 码须逐个真跑造出来",
    );

    // 契约冻结（判据侧钉死具体值）。
    set.add(
        "F3409-判据-契约已冻结",
        DEBUG_CONTRACT == "E11-debug-v1"
            && MAX_TRACE_STEPS == 256
            && MAX_QUERIES == 16384
            && ELEMENT_MAX == 256
            && SESSION_MAX == 64
            && INDEX_BUCKETS == 4096
            && INDEX_BUCKETS.is_power_of_two(),
        "契约号与关键上限钉死（改它们等于改契约）",
    );

    // 读屏数量逐位。
    set.add(
        "F3409-判据-数量逐位读",
        fmt_num(0) == "零" && fmt_num(7) == "七" && fmt_num(12) == "一 二",
        "数量读屏逐位，无进位歧义",
    );

    // 变体：码表漂移必被抓（冻结表任一位改掉须与实测不同）。
    let mutated = {
        let mut w = EXPECT_WIRE.to_vec();
        w[0] = "E11-MUTATED";
        let a: Vec<&str> = TraceCode::ALL.iter().map(|c| c.code()).collect();
        let mut sa = a.clone();
        sa.sort_unstable();
        let mut sw = w.to_vec();
        sw.sort_unstable();
        sa != sw
    };
    set.add(
        "F3409-判据-变体码表漂移必被抓",
        mutated,
        "冻结表被改（改码/增删）时『码表一致』判据须转红",
    );

    // 变体：非阻断码若被判成阻断须与实测不同。
    let blocking_cnt_if_alert_blocked = TraceCode::ALL
        .iter()
        .filter(|c| c.blocking() || **c == TraceCode::NotFound)
        .count();
    set.add(
        "F3409-判据-变体提示当阻断必被抓",
        blocking_cnt_if_alert_blocked != 4,
        "把提示级算进阻断会让三档计数漂移——判据须能区分",
    );
}

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

/// A 批：反查链路 + 会话统一。
pub fn run_ver01i_checks_a() -> CheckSet {
    let mut set = CheckSet::new("ver01i-debugtool-a");
    chk_inspector(&mut set);
    set
}

/// B 批：追踪 + 视图 + 契约。
pub fn run_ver01i_checks_b() -> CheckSet {
    let mut set = CheckSet::new("ver01i-debugtool-b");
    chk_tracer(&mut set);
    chk_view(&mut set);
    chk_contract(&mut set);
    set
}
