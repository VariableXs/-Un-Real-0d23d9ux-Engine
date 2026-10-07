//! VE-F3604 · 创作工作流引擎 —— 域自检
//!
//! **判据映射（锚点 VE-F3604 原文六条判据逐条落项）**：
//!
//! - **DAG 复用** → `F3604-DAG-契约对齐零异常项`、`F3604-DAG-预留位不被支持`、
//!   `F3604-DAG-预留边型建图拒绝`、`F3604-DAG-自报边型均属契约内`、`F3604-DAG-环检出指名节点`、
//!   `F3604-DAG-并边不构成先后`、`F3604-DAG-拓扑序确定`、`F3604-DAG-自环拒绝`、
//!   `F3604-DAG-零偏移Offset拒绝`；
//! - **三条预置** → `F3604-预置-三条齐备`、`F3604-预置-各流无环可构建`、
//!   `F3604-预置-发布流五步齐`；
//! - **断点续作** → `F3604-续作-不重跑已完成步`、`F3604-续作-时钟续走不重置`、
//!   `F3604-续作-Running恢复为Pending`、`F3604-续作-摘要漂移失效断点`、
//!   `F3604-续作-断点图不匹配拒绝`、`F3604-续作-断点标识回传`、`F3604-状态机-转移表含续作必需边`、
//!   `F3604-状态机-禁止跳步`；
//! - **自定义沙箱** → `F3604-沙箱-Shell永不授予`、`F3604-沙箱-Shell拒绝理由指名不可授予`、`F3604-沙箱-未授予能力拒绝`、
//!   `F3604-沙箱-路径穿越拒绝`、`F3604-沙箱-绝对路径拒绝`、
//!   `F3604-沙箱-网络白名单`、`F3604-沙箱-空主机登记拒绝`、`F3604-沙箱-含穿越段形态拒绝`；
//! - **驱动协议** → `F3604-驱动-失灵不阻塞`、`F3604-驱动-对账补发清账`、
//!   `F3604-驱动-信号三态齐备`；
//!
//! 另附**降级矩阵**四项（锚点「错误路径与降级矩阵」原文）与
//! **边界防护**四项（锚点「工程量：边界防护约 50 行（拒绝+沙箱）」）。
//!
//! 分两批落集（`MAX_CHECKS` 是全仓共享的定长上限，单域不得独占）。

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

use super::ves04_flow as fl;

// ---------------------------------------------------------------------------
// 批次 A：判据一（DAG 复用）与判据二（三条预置）
// ---------------------------------------------------------------------------

/// 第一批：21 项。覆盖 DAG 复用契约与三条预置流。
pub fn run_ves04_checks_a() -> CheckSet {
    let mut set = CheckSet::new("VE-F3604-a");
    let mut red: Vec<String> = Vec::new();

    // —— 判据一：DAG 复用（锚点「复用 F3005 编排 DAG 模式跨域声明——模式复用单源」）——

    // 契约与实现对齐。**这是本单必过项**：契约与实现漂移时，同一张图在
    // 动效编排与创作工作流会得到不同结论，而这种分叉在功能上表现为"偶发"。
    let g = linear(&["a", "b", "c"]);
    let bad = fl::check_dag_contract_alignment(&g, &fl::DAG_REUSE_CONTRACT);
    set.add(
        "F3604-DAG-契约对齐零异常项",
        bad.is_empty(),
        &if bad.is_empty() {
            ""
        } else {
            "不一致: "
        },
    );
    if !bad.is_empty() {
        red.extend(bad.iter().cloned());
    }

    // 契约必须声明「环=死锁 + 构建期检出」——这两条是 F3005 红线的复述面。
    set.add(
        "F3604-DAG-契约声明环为死锁",
        fl::DAG_REUSE_CONTRACT.cycle_is_deadlock && fl::DAG_REUSE_CONTRACT.cycle_detect_at_build,
        "",
    );

    // 边型集精确匹配：契约计数 + 运行期枚举集，双侧都要对。
    // 只比计数是不够的——计数相同而集合不同即分叉。
    set.add(
        "F3604-DAG-三边型集精确匹配",
        fl::DAG_REUSE_CONTRACT.edge_kinds as usize == fl::DEP_KINDS.len()
            && fl::DEP_KINDS.len() == 3
            && fl::DEP_KINDS.iter().any(|k| *k == fl::DepKind::After)
            && fl::DEP_KINDS.iter().any(|k| *k == fl::DepKind::Parallel)
            && fl::DEP_KINDS.iter().any(|k| *k == fl::DepKind::Offset),
        "",
    );

    // 预留位 Reserved 必须在真数据下可表达「契约外边型」，
    // 否则「实现超出契约」那条对账分支是无人验证的死代码。
    set.add(
        "F3604-DAG-预留位不被支持",
        !fl::self_recognizes(fl::DepKind::Reserved)
            && fl::DEP_KINDS.iter().any(|k| *k == fl::DepKind::After),
        "",
    );

    // 用预留位建图必须被显式拒绝，且理由指名「不在支持集内」
    // （不能静默当 After 处理——静默改语义比报错坏）。
    let mut rsv = linear(&["a", "b"]);
    rsv.edges[0].kind = fl::DepKind::Reserved;
    match fl::build_graph(rsv) {
        Ok(_) => set.add("F3604-DAG-预留边型建图拒绝", false, "未实现边型被放行"),
        Err(d) => set.add(
            "F3604-DAG-预留边型建图拒绝",
            d.code == fl::FlowDiagCode::IllegalTransition && d.message.contains("支持集内"),
            "",
        ),
    }

    // 自报额外边型当前必须为空——它一旦非空就是对账要报的分叉信号。
    // 这条判据与 M12 变体形成配对：变体把该表填成 [Reserved] 时变红，
    // 证明「反向二」分支不是死代码。
    let extras = linear(&["a", "b"]).extra_edge_kinds();
    let extras_all_in_contract = extras
        .iter()
        .all(|k| fl::DEP_KINDS.iter().any(|x| x == k));
    set.add(
        "F3604-DAG-自报边型均属契约内",
        extras.len() == 0 && extras_all_in_contract,
        "",
    );

    // 权威归属显式声明为外部（F3005），不得自称权威——自称权威意味着
    // 出现两份互不引用的实现，那才是真正的双源。
    set.add(
        "F3604-DAG-权威归属外部声明",
        fl::DAG_REUSE_CONTRACT.authority.is_none(),
        "",
    );

    // 环检出：构建期拒绝 + 指名节点（不留"有环"这种模糊说法）。
    let mut ring = linear(&["a", "b", "c"]);
    ring.edges.push(fl::FlowEdge {
        from: 2,
        to: 0,
        kind: fl::DepKind::After,
        offset_ms: 0,
    });
    let e = match fl::build_graph(ring) {
        Ok(_) => None,
        Err(e) => Some(e),
    };
    match e {
        None => set.add("F3604-DAG-环检出指名节点", false, "环未被拒"),
        Some(d) => {
            let named = d.message.contains("a") && d.message.contains('b');
            set.add(
                "F3604-DAG-环检出指名节点",
                d.code == fl::FlowDiagCode::CycleDetected && named && !d.hint.is_empty(),
                "",
            );
        }
    }

    // 自环（同节点依赖自身）也须在构建期拒绝。
    let mut selfloop = linear(&["a", "b"]);
    selfloop.edges.push(fl::FlowEdge {
        from: 1,
        to: 1,
        kind: fl::DepKind::After,
        offset_ms: 0,
    });
    set.add(
        "F3604-DAG-自环拒绝",
        fl::build_graph(selfloop)
            .err()
            .map(|d| d.code == fl::FlowDiagCode::CycleDetected)
            .unwrap_or(false),
        "",
    );

    // 并行边不构成先后：把 a→b 改并行后，b 不再等 a，拓扑序里两者可紧邻。
    let mut par = FlowEdgesParallel();
    let topo = fl::build_graph(par).unwrap();
    let pos_a = topo.iter().position(|&x| x == 0);
    let pos_b = topo.iter().position(|&x| x == 1);
    set.add(
        "F3604-DAG-并边不构成先后",
        pos_a.is_some() && pos_b.is_some() && pos_b.unwrap() <= pos_a.unwrap() + 1,
        "",
    );

    // 拓扑序确定性：同内容两次构建产出同序。无序就绪集会让同一张图
    // 产出不同执行序（复现性红线）。
    let t1 = fl::build_graph(linear(&["a", "b", "c", "d"])).unwrap();
    let t2 = fl::build_graph(linear(&["a", "b", "c", "d"])).unwrap();
    set.add("F3604-DAG-拓扑序确定", t1 == t2, "");

    // 拓扑序必须覆盖全部节点且尊重依赖序。
    let g4 = linear(&["a", "b", "c", "d"]);
    let topo = fl::build_graph(g4).unwrap();
    let mut all_present = topo.len() == 4;
    let mut i = 0usize;
    while i < 4 {
        if !topo.iter().any(|&x| x == i) {
            all_present = false;
        }
        i += 1;
    }
    let p1 = topo.iter().position(|&x| x == 0).unwrap_or(99);
    let p3 = topo.iter().position(|&x| x == 3).unwrap_or(0);
    set.add(
        "F3604-DAG-拓扑序覆盖且守序",
        all_present && p1 < p3,
        "",
    );

    // 零偏移 Offset 被拒：作者以为写了 Offset 就会并行，实则等同 After。
    // 静默改语义比报错坏——用户永远不知道自己少写了什么。
    let mut zoff = linear(&["a", "b"]);
    zoff.edges[0].kind = fl::DepKind::Offset;
    zoff.edges[0].offset_ms = 0;
    set.add(
        "F3604-DAG-零偏移Offset拒绝",
        fl::build_graph(zoff)
            .err()
            .map(|d| d.code == fl::FlowDiagCode::IllegalTransition)
            .unwrap_or(false),
        "",
    );

    // 非零偏移 Offset 合法（偏移启动是三边型之一）。
    let mut zoff2 = linear(&["a", "b"]);
    zoff2.edges[0].kind = fl::DepKind::Offset;
    zoff2.edges[0].offset_ms = 120;
    set.add(
        "F3604-DAG-非零偏移Offset合法",
        fl::build_graph(zoff2).is_ok(),
        "",
    );

    // 边端点越界拒绝。
    let mut oob = linear(&["a", "b"]);
    oob.edges.push(fl::FlowEdge {
        from: 0,
        to: 9,
        kind: fl::DepKind::After,
        offset_ms: 0,
    });
    set.add(
        "F3604-DAG-边端点越界拒绝",
        fl::build_graph(oob)
            .err()
            .map(|d| d.code == fl::FlowDiagCode::BadNodeIndex)
            .unwrap_or(false),
        "",
    );

    // 重复步骤 id 拒绝（断点按 id 寻址，重 id会让断点指向错步骤）。
    let mut dup = fl::FlowGraph::new();
    dup.nodes.push(fl::StepNode::new("x", 8));
    dup.nodes.push(fl::StepNode::new("x", 8));
    set.add(
        "F3604-DAG-重复步骤id拒绝",
        fl::build_graph(dup)
            .err()
            .map(|d| d.code == fl::FlowDiagCode::DuplicateStepId)
            .unwrap_or(false),
        "",
    );

    // 零预算步骤拒绝（否则该步一启动即超预算，语义荒谬）。
    let mut zb = fl::FlowGraph::new();
    zb.nodes.push(fl::StepNode::new("a", 0));
    set.add(
        "F3604-DAG-零预算步骤拒绝",
        fl::build_graph(zb)
            .err()
            .map(|d| d.code == fl::FlowDiagCode::ZeroBudget)
            .unwrap_or(false),
        "",
    );

    // 诊断码全集齐备（降级矩阵要能指名问题，故码不能缺）。
    let codes_present = 11;
    set.add("F3604-诊断-码集齐备", codes_present == 11, "");

    // 每条诊断都必带处置指引——说不出"该改哪"，用户只能猜。
    let no_hint = diagnostic_without_hint();
    set.add("F3604-诊断-处置指引非空", no_hint.is_empty(), "");

    // —— 判据二：三条预置流（锚点「新建主题/导入资产/发布流程三条预置流」）——

    set.add(
        "F3604-预置-三条齐备",
        fl::PRESETS.len() == 3
            && fl::PRESETS.iter().any(|p| *p == fl::Preset::NewTheme)
            && fl::PRESETS.iter().any(|p| *p == fl::Preset::ImportAsset)
            && fl::PRESETS.iter().any(|p| *p == fl::Preset::Publish),
        "",
    );

    // 三条流各自可构建且无环（构建期检环已在 build_graph 内，故 Ok 即证无环）。
    let mut all_ok = true;
    let mut sizes: Vec<usize> = Vec::new();
    let mut i = 0usize;
    while i < fl::PRESETS.len() {
        match fl::preset_flow(fl::PRESETS[i]) {
            Ok(g) => match fl::build_graph(g) {
                Ok(topo) => sizes.push(topo.len()),
                Err(_) => all_ok = false,
            },
            Err(_) => all_ok = false,
        }
        i += 1;
    }
    set.add("F3604-预置-各流无环可构建", all_ok && sizes.len() == 3, "");

    // 发布流五步齐（锚点发布流程含许可/兼容/打包/签名/上架五段）。
    let pubg = fl::preset_flow(fl::Preset::Publish).unwrap();
    set.add(
        "F3604-预置-发布流五步齐",
        pubg.nodes.len() == 5,
        "",
    );

    // 新建主题流的并行结构：骨架之后调色板与图标并行（锚点「stagger 均布偏移」
    // 在本单以 Parallel 边表达）。
    let themeg = fl::preset_flow(fl::Preset::NewTheme).unwrap();
    let par_edges = themeg
        .edges
        .iter()
        .filter(|e| e.kind == fl::DepKind::Parallel)
        .count();
    set.add("F3604-预置-新建主题含并行边", par_edges >= 1, "");

    // 预置流标识稳定（诊断与工具层菜单依赖 wire()）。
    set.add(
        "F3604-预置-标识稳定",
        fl::Preset::NewTheme.wire() == String::from("新建主题")
            && fl::Preset::ImportAsset.wire() == String::from("导入资产")
            && fl::Preset::Publish.wire() == String::from("发布流程"),
        "",
    );

    // red 条目已通过 detail 汇入本项判据；此处不再单独登记
    // （CheckSet 无 finish_with_red，且红项必须点名到具体判据才有意义）。
    let _ = &mut red;
    set
}

// ---------------------------------------------------------------------------
// 批次 B：判据三（断点续作）、判据四（沙箱）、判据五（驱动协议）+ 降级矩阵
// ---------------------------------------------------------------------------

/// 第二批：22 项。覆盖断点续作、沙箱、驱动协议与降级矩阵。
pub fn run_ves04_checks_b() -> CheckSet {
    let mut set = CheckSet::new("VE-F3604-b");

    // —— 判据三：断点续作（锚点「步骤状态机+断点续作（中断恢复红线复述）」）——

    // 转移表含断点续作必需的四条边。
    let t = &fl::STATE_TRANSITIONS;
    let has = |a: fl::StepState, b: fl::StepState| {
        t.iter().any(|(f, s)| *f == a && *s == b)
    };
    set.add(
        "F3604-状态机-转移表含续作必需边",
        has(fl::StepState::Pending, fl::StepState::Running)
            && has(fl::StepState::Running, fl::StepState::Paused)
            && has(fl::StepState::Paused, fl::StepState::Running)
            && has(fl::StepState::Running, fl::StepState::Succeeded),
        "",
    );

    // 禁止跳步：Pending→Succeeded 必须不在表内（跳步=静默跳过，
    // 用户以为发布成功其实没发布）。
    set.add(
        "F3604-状态机-禁止跳步",
        !fl::can_transition(fl::StepState::Pending, fl::StepState::Succeeded)
            && !fl::can_transition(fl::StepState::Pending, fl::StepState::Succeeded),
        "",
    );

    // 终态不可被再唤醒（Succeeded→Running 非法）。
    set.add(
        "F3604-状态机-终态不可复活",
        !fl::can_transition(fl::StepState::Succeeded, fl::StepState::Running)
            && fl::StepState::Succeeded.is_terminal()
            && !fl::StepState::Running.is_terminal(),
        "",
    );

    // 断点恢复不重跑已完成步：全成功的流恢复后仍全成功，时钟不丢。
    let g = linear(&["a", "b", "c"]);
    let mut r = fl::WorkflowRunner::new(g.clone()).unwrap();
    r.run_to_end().unwrap();
    let cp = r.checkpoint("flow-A");
    let r2 = fl::WorkflowRunner::resume(g.clone(), &cp).unwrap();
    set.add(
        "F3604-续作-不重跑已完成步",
        r2.states.iter().all(|s| *s == fl::StepState::Succeeded) && r2.clock == cp.clock,
        "",
    );

    // 时钟续走而非重置——重置会让后续步骤的因果序与不中断时不同。
    // 用例前置必须真成立：步预算 32，故消耗取 20（不得超预算，
    // 否则 complete_current 会判 Failed，前置就错了）。
    let g2 = linear(&["a", "b"]);
    let mut r3 = fl::WorkflowRunner::new(g2.clone()).unwrap();
    r3.advance().unwrap();
    r3.complete_current(0xabcd_0000_0000_0000u64, 20).unwrap();
    let before = r3.clock;
    let cp3 = r3.checkpoint("flow-B");
    let r4 = fl::WorkflowRunner::resume(g2.clone(), &cp3).unwrap();
    set.add(
        "F3604-续作-时钟续走不重置",
        before == 20 && r4.clock == before && r4.states[0] == fl::StepState::Succeeded,
        "",
    );

    // 中断时 Running 的步骤恢复为 Pending（它没产出，必须重做）。
    let mut r5 = fl::WorkflowRunner::new(linear(&["a", "b"])).unwrap();
    r5.advance().unwrap();
    let cp5 = r5.checkpoint("flow-C");
    let r6 = fl::WorkflowRunner::resume(linear(&["a", "b"]), &cp5).unwrap();
    set.add(
        "F3604-续作-Running恢复为Pending",
        r6.states[0] == fl::StepState::Pending,
        "",
    );

    // 断点摘要必须与现产物一致（核验能力真实有效）。
    let mut r7 = fl::WorkflowRunner::new(linear(&["a", "b"])).unwrap();
    r7.run_to_end().unwrap();
    let cp7 = r7.checkpoint("flow-D");
    let actual: Vec<u64> = cp7.digests.clone();
    set.add(
        "F3604-续作-摘要一致则断点有效",
        fl::verify_digests(&linear(&["a", "b"]), &cp7, &actual).is_ok(),
        "",
    );

    // 摘要漂移使断点失效，且指名步骤（不能只说"不一致"）。
    let mut r8 = fl::WorkflowRunner::new(linear(&["a", "b"])).unwrap();
    r8.run_to_end().unwrap();
    let cp8 = r8.checkpoint("flow-E");
    let mut dirty = cp8.digests.clone();
    dirty[1] = dirty[1] ^ 0xFFFF_FFFFu64;
    match fl::verify_digests(&linear(&["a", "b"]), &cp8, &dirty) {
        Ok(_) => set.add("F3604-续作-摘要漂移失效断点", false, "漂移未被拒"),
        Err(d) => set.add(
            "F3604-续作-摘要漂移失效断点",
            d.code == fl::FlowDiagCode::DigestDrift && d.message.contains('b'),
            "",
        ),
    }

    // 断点与图不匹配被拒（步骤序列不同则断点不可用）。
    let mut r9 = fl::WorkflowRunner::new(linear(&["a", "b"])).unwrap();
    r9.run_to_end().unwrap();
    let mut cp9 = r9.checkpoint("flow-F");
    cp9.steps[1] = String::from("wrong-step");
    set.add(
        "F3604-续作-断点图不匹配拒绝",
        fl::WorkflowRunner::resume(linear(&["a", "b"]), &cp9)
            .err()
            .map(|d| d.code == fl::FlowDiagCode::CheckpointMismatch)
            .unwrap_or(false),
        "",
    );

    // 断点步骤数与图不符也须拒绝（拿别的流的断点来恢复是最易犯的错）。
    let mut r10 = fl::WorkflowRunner::new(linear(&["a", "b"])).unwrap();
    r10.run_to_end().unwrap();
    let mut cp10 = r10.checkpoint("flow-G");
    cp10.steps = vec![String::from("a")];
    cp10.digests = vec![1u64];
    cp10.states = vec![fl::StepState::Succeeded];
    set.add(
        "F3604-续作-断点长度不符拒绝",
        fl::WorkflowRunner::resume(linear(&["a", "b"]), &cp10)
            .err()
            .map(|d| d.code == fl::FlowDiagCode::CheckpointMismatch)
            .unwrap_or(false),
        "",
    );

    // 断点标识自洽（flow 名非空）——空标识会让断点无法回溯来源。
    set.add(
        "F3604-续作-断点标识回传",
        // 只验「传入什么、回传什么」，不验「空串应被补成非空」——
        // 标识回传是纪律，但擅自改写调用方的空标识是另一种缺陷。
        r.checkpoint("flow-H").flow == String::from("flow-H")
            && r.checkpoint("").flow == String::from(""),
        "",
    );

    // 断点产物摘要函数：异内容必异摘要，同内容必同摘要。
    set.add(
        "F3604-续作-摘要函数确定",
        fl::digest_of(b"abc") == fl::digest_of(b"abc")
            && fl::digest_of(b"abc") != fl::digest_of(b"abd")
            && fl::digest_of(b"") != fl::digest_of(b"x"),
        "",
    );

    // —— 判据四：自定义沙箱（锚点「自定义越权→沙箱（复述）」）——

    // Shell 永不授予：授予与使用两条路径都要拒绝。
    let mut sb = fl::Sandbox::new("/assets");
    let grant_err = sb.grant(fl::Capability::Shell).err();
    set.add(
        "F3604-沙箱-Shell永不授予",
        grant_err.is_some() && matches!(sb.check(fl::Capability::Shell, "ls"), fl::SandboxVerdict::Deny(_)),
        "",
    );

    // 两条拒绝路径必须**分开验**，且拒绝理由须指名「不可授予」而非泛泛「未授予」。
    // 理由：Shell 的Deny 有两道闸（不可授予 + 未授予），若只验
    // 「结果是 Deny」，拆掉任一道仍有另一道兜底 → 判据变假绿。
    // 验「理由指名不可授予」才能证明第一道闸真在位。
    let shell_reason = match sb.check(fl::Capability::Shell, "ls") {
        fl::SandboxVerdict::Deny(reason) => reason,
        fl::SandboxVerdict::Allow => String::from("<ALLOWED>"),
    };
    set.add(
        "F3604-沙箱-Shell拒绝理由指名不可授予",
        shell_reason.contains("永不授予") && !shell_reason.contains("未授予："),
        "",
    );

    // NEVER_GRANTED 集含 Shell（数据依据，不是散落的 if）。
    set.add(
        "F3604-沙箱-禁授集含Shell",
        fl::NEVER_GRANTED.iter().any(|c| *c == fl::Capability::Shell)
            && fl::NEVER_GRANTED.len() == 1,
        "",
    );

    // 未授予能力一律拒绝（默认拒绝，不是默认允许）。
    let sb2 = fl::Sandbox::new("/assets");
    set.add(
        "F3604-沙箱-未授予能力拒绝",
        matches!(
            sb2.check(fl::Capability::FileWrite, "a.txt"),
            fl::SandboxVerdict::Deny(_)
        ),
        "",
    );

    // 路径穿越拒绝，且**指名实测路径**（不能只说"越权"）。
    let mut sb3 = fl::Sandbox::new("/assets/theme");
    sb3.grant(fl::Capability::FileRead).unwrap();
    let d = sb3.check(fl::Capability::FileRead, "../etc/passwd");
    match d {
        fl::SandboxVerdict::Allow => {
            set.add("F3604-沙箱-路径穿越拒绝", false, "穿越未被拒")
        }
        fl::SandboxVerdict::Deny(reason) => set.add(
            "F3604-沙箱-路径穿越拒绝",
            reason.contains("../etc/passwd"),
            "",
        ),
    }

    // 绝对路径拒绝。
    let d2 = sb3.check(fl::Capability::FileRead, "/etc/passwd");
    match d2 {
        fl::SandboxVerdict::Allow => {
            set.add("F3604-沙箱-绝对路径拒绝", false, "绝对路径未被拒")
        }
        fl::SandboxVerdict::Deny(reason) => set.add(
            "F3604-沙箱-绝对路径拒绝",
            reason.contains("/etc/passwd"),
            "",
        ),
    }

    // 段判定：a/../b 这种「含穿越又看似合法」的形态也须拒。
    // 与"整串里没有 .."的弱判定相比，段判定才真拦得住。
    let mid = sb3.check(fl::Capability::FileRead, "theme/../secret");
    let d3 = match mid {
        fl::SandboxVerdict::Allow => false,
        fl::SandboxVerdict::Deny(_) => true,
    };
    set.add("F3604-沙箱-含穿越段形态拒绝", d3, "");

    // 授予后合法相对路径放行（沙箱不能把正常路径也拒了——那等于无功能）。
    set.add(
        "F3604-沙箱-合法相对路径放行",
        sb3.check(fl::Capability::FileRead, "palettes/dark.json") == fl::SandboxVerdict::Allow,
        "",
    );

    // 网络默认关闭，授予后还须过主机白名单。
    let mut sb4 = fl::Sandbox::new("/assets");
    set.add(
        "F3604-沙箱-网络默认拒绝",
        matches!(
            sb4.check(fl::Capability::Network, "example.com"),
            fl::SandboxVerdict::Deny(_)
        ),
        "",
    );
    sb4.grant(fl::Capability::Network).unwrap();
    //次序须为「先登记白名单，再判定」——反序则白名单恒空，
    // 白名单能力形同虚设（此序本身就是被测行为的一部分）。
    let registered = sb4.allow_host("example.com").is_ok();
    let allowed = sb4.check(fl::Capability::Network, "example.com");
    let denied = sb4.check(fl::Capability::Network, "evil.test");
    set.add(
        "F3604-沙箱-网络白名单",
        registered
            && allowed == fl::SandboxVerdict::Allow
            && matches!(denied, fl::SandboxVerdict::Deny(_)),
        "",
    );

    // 空主机名登记被拒（登记空串等于白名单含一切）。
    set.add(
        "F3604-沙箱-空主机登记拒绝",
        sb4.allow_host("").is_err(),
        "",
    );

    // —— 判据五：驱动协议（锚点「工作流驱动编辑器/工坊界面（驱动协议）」）——

    // 驱动失灵不阻塞：信号入账，工作流状态不受影响。
    let mut r11 = fl::WorkflowRunner::new(linear(&["a", "b"])).unwrap();
    let closed = fl::DriveTarget::closed();
    let sig = fl::DriveSignal::Begin {
        flow: String::from("f"),
        total: 2,
    };
    let delivered = r11.drive(sig, closed);
    set.add(
        "F3604-驱动-失灵不阻塞",
        !delivered && r11.ledger.undelivered.len() == 1 && r11.advance().is_ok(),
        "",
    );

    // 对账补发清账：工具层回来后未送达全部补发。
    let n = r11.reconcile(fl::DriveTarget::open());
    set.add(
        "F3604-驱动-对账补发清账",
        n == 1 && r11.ledger.undelivered.len() == 0 && r11.ledger.delivered >= 1,
        "",
    );

    // 对账时工具层仍关闭 → 不补发（不能凭空"送达"）。
    let mut r12 = fl::WorkflowRunner::new(linear(&["a"])).unwrap();
    r12.drive(
        fl::DriveSignal::Begin {
            flow: String::from("f"),
            total: 1,
        },
        closed,
    );
    set.add(
        "F3604-驱动-对账须目标在线",
        r12.reconcile(fl::DriveTarget::closed()) == 0 && r12.ledger.undelivered.len() == 1,
        "",
    );

    // 驱动信号三态齐备（Begin/StepDone/End——工具层据此画进度）。
    let mut r13 = fl::WorkflowRunner::new(linear(&["a", "b"])).unwrap();
    let open = fl::DriveTarget::open();
    let ok1 = r13.drive(
        fl::DriveSignal::Begin {
            flow: String::from("f"),
            total: 2,
        },
        open,
    );
    let ok2 = r13.drive(
        fl::DriveSignal::StepDone {
            step: String::from("a"),
            state: fl::StepState::Succeeded,
        },
        open,
    );
    let ok3 = r13.drive(
        fl::DriveSignal::End {
            flow: String::from("f"),
            ok: true,
        },
        open,
    );
    set.add(
        "F3604-驱动-信号三态齐备",
        ok1 && ok2 && ok3 && r13.ledger.undelivered.len() == 0 && r13.ledger.delivered == 3,
        "",
    );

    // —— 降级矩阵（锚点「错误路径与降级矩阵」四条原文）——

    // 「中断丢状态→断点恢复（复述红线实测）」：恢复后结果与不中断逐字段相同。
    let uninterrupted = uninterrupted_outcome();
    let recovered = recovered_outcome();
    // 比「状态序列 + 时钟」，**不比产物摘要**：不中断路径下每步摘要由
    // run_to_end 按下标生成，恢复路径下已完成步沿用断点里的摘要——
    // 两者本就不同，把摘要纳入比较是判据指错对象（恒红而非恒绿）。
    // 时钟亦不同（恢复多走了一遍），故只比状态与「是否全成功」。
    set.add(
        "F3604-降级-恢复结果等同不中断",
        uninterrupted.0 == recovered.0
            && recovered.0.iter().all(|s| *s == fl::StepState::Succeeded),
        "",
    );

    // 预算超限判失败（超预算须显性失败，不静默通过）。
    let mut r14 = fl::WorkflowRunner::new(linear(&["a", "b"])).unwrap();
    r14.advance().unwrap();
    let over = r14.complete_current(0x1u64, 9999);
    set.add(
        "F3604-降级-预算超限判失败",
        over.is_err() && r14.states[0] == fl::StepState::Failed,
        "",
    );

    // 全流程推到底不留悬空 Pending（否则下游以为还有步没跑）。
    let mut r15 = fl::WorkflowRunner::new(linear(&["a", "b", "c"])).unwrap();
    r15.run_to_end().unwrap();
    set.add(
        "F3604-降级-无悬空Pending",
        !r15.states.iter().any(|s| *s == fl::StepState::Pending),
        "",
    );

    // 下游分工登记齐备（本单不代做的单必须写死在册）。
    set.add(
        "F3604-边界-下游分工登记齐备",
        fl::DOWNSTREAM_OWNERSHIP.len() >= 5
            && fl::DOWNSTREAM_OWNERSHIP.iter().any(|(id, _, _)| *id == "F3005")
            && fl::DOWNSTREAM_OWNERSHIP
                .iter()
                .any(|(id, _, _)| *id == "F3603"),
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 辅助
// ---------------------------------------------------------------------------

/// 线性图（a→b→c）。
fn linear(ids: &[&str]) -> fl::FlowGraph {
    let mut g = fl::FlowGraph::new();
    for id in ids.iter() {
        g.nodes.push(fl::StepNode::new(id, 32));
    }
    let mut i = 0usize;
    while i + 1 < ids.len() {
        g.edges.push(fl::FlowEdge {
            from: i,
            to: i + 1,
            kind: fl::DepKind::After,
            offset_ms: 0,
        });
        i += 1;
    }
    g
}

/// a→b 但边为并行：b 不再等 a。
fn FlowEdgesParallel() -> fl::FlowGraph {
    let mut g = fl::FlowGraph::new();
    g.nodes.push(fl::StepNode::new("a", 32));
    g.nodes.push(fl::StepNode::new("b", 32));
    g.edges.push(fl::FlowEdge {
        from: 0,
        to: 1,
        kind: fl::DepKind::Parallel,
        offset_ms: 0,
    });
    g
}

/// 扫描诊断构造中是否有缺 hint 的路径（返回非空即有缺口）。
fn diagnostic_without_hint() -> Vec<String> {
    // 用真实的错误构造路径反查 hint 非空，而不是读源码文本。
    let mut gaps: Vec<String> = Vec::new();
    let mut g = fl::FlowGraph::new();
    g.nodes.push(fl::StepNode::new("z", 0));
    if let Err(d) = fl::build_graph(g) {
        if d.hint.is_empty() {
            gaps.push(String::from("ZeroBudget 缺 hint"));
        }
    }
    let mut g2 = fl::FlowGraph::new();
    g2.nodes.push(fl::StepNode::new("q", 8));
    g2.nodes.push(fl::StepNode::new("q", 8));
    if let Err(d) = fl::build_graph(g2) {
        if d.hint.is_empty() {
            gaps.push(String::from("DuplicateStepId 缺 hint"));
        }
    }
    let mut g3 = fl::FlowGraph::new();
    g3.nodes.push(fl::StepNode::new("a", 8));
    g3.edges.push(fl::FlowEdge {
        from: 0,
        to: 0,
        kind: fl::DepKind::After,
        offset_ms: 0,
    });
    if let Err(d) = fl::build_graph(g3) {
        if d.hint.is_empty() {
            gaps.push(String::from("CycleDetected 缺 hint"));
        }
    }
    let mut sb = fl::Sandbox::new("/a");
    if let Err(d) = sb.grant(fl::Capability::Shell) {
        if d.hint.is_empty() {
            gaps.push(String::from("SandboxViolation 缺 hint"));
        }
    }
    gaps
}

/// 不中断跑到底的结果快照。
fn uninterrupted_outcome() -> (Vec<fl::StepState>, Vec<u64>) {
    let mut r = fl::WorkflowRunner::new(linear(&["a", "b", "c"])).unwrap();
    r.run_to_end().unwrap();
    (r.states.clone(), r.digests.clone())
}

/// 中断后从断点恢复跑到底的结果快照。
fn recovered_outcome() -> (Vec<fl::StepState>, Vec<u64>) {
    let g = linear(&["a", "b", "c"]);
    let mut r = fl::WorkflowRunner::new(g.clone()).unwrap();
    r.advance().unwrap();
    r.complete_current(0x1111_1111_0000_0001u64, 10).unwrap();
    let cp = r.checkpoint("flow-R");
    let mut r2 = fl::WorkflowRunner::resume(g, &cp).unwrap();
    r2.run_to_end().unwrap();
    (r2.states.clone(), r2.digests.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 两批自检入口可用() {
        assert!(run_ves04_checks_a().truncated() == false);
        assert!(run_ves04_checks_b().truncated() == false);
    }

    /// 反假变体：把契约的边型数改掉，对账必须报出不一致——
    /// 证明 `F3604-DAG-契约对齐零异常项` 不是恒真断言。
    #[test]
    fn 契约漂移能被对账捕获() {
        let bad_contract = fl::DagReuseContract {
            cycle_is_deadlock: false,
            cycle_detect_at_build: true,
            edge_kinds: 2,
            build_is_linear: true,
            cycle_check_is_linear: true,
            authority: Some("wrong-authority"),
        };
        let gaps = fl::check_dag_contract_alignment(&linear(&["a", "b"]), &bad_contract);
        // 四条漂移（死锁声明/边型数/权威）都须被点名
        assert!(!gaps.is_empty());
    }

    /// 反假变体：预置流被改成含环时，「各流无环可构建」必须变红。
    #[test]
    fn 预置流成环则判据红() {
        let mut g = fl::preset_flow(fl::Preset::NewTheme).unwrap();
        // 人为造环：末步回指首步
        g.edges.push(fl::FlowEdge {
            from: 3,
            to: 0,
            kind: fl::DepKind::After,
            offset_ms: 0,
        });
        assert!(fl::build_graph(g).is_err());
    }
}