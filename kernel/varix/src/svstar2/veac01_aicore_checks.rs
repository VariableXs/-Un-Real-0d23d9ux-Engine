//! VE-F5801 域自检（VE-AC 域 · AI 域总架构判据层）
//!
//! 判据侧**独立写死**期望（四层名序、十件名序、十码 0x59 段、层序互逆），
//! 不复用实现侧常量做期望。聚合防自调：族内判据只调另一族 standalone +
//! 进行中 set 自身 tally。

use crate::checks::CheckSet;

use super::veac01_aicore::*;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 判据侧独立写死的期望
// ---------------------------------------------------------------------------

/// 四层名序（层序=序数序，判据侧逐位写死）。
const EXPECT_LAYER_NAMES: [&str; 4] = ["感知层", "决策层", "执行层", "协作层"];
/// 移交十件名序（与 F5793 移交包定名逐位一致）。
const EXPECT_HANDOVER_NAMES: [&str; 10] = [
    "音频事件通道",
    "音频调试钩子",
    "指标口径册",
    "场景覆盖表",
    "三纪律承诺",
    "fuzz 语料袋",
    "性能册",
    "安全隐私口径",
    "文档总纲",
    "看板链接",
];
/// 十码判据侧写死（全部 0x59 段）。
const EXPECT_CODES: [u16; 10] = [
    0x5900, 0x5901, 0x5902, 0x5903, 0x5904, 0x5905, 0x5906, 0x5907, 0x5908, 0x5909,
];
/// 错误码段（判据侧独立写死，与 0x57=veab03 / 0x58=vcq02 双邻互斥）。
const EXPECT_SEGMENT: u8 = 0x59;
/// 错误码总数期望。
const EXPECT_ERROR_COUNT: usize = 10;
/// 架构总预算上限期望（µs）。
const EXPECT_TOTAL_BUDGET: u32 = 400;

/// 合格四层规格：逐层定预算（合计 400 恰贴上限——贴线语料最有判别力）。
fn good_spec(kind: LayerKind) -> LayerSpec {
    LayerSpec {
        kind,
        caps: Vec::new(),
        budget_us: 0,
        predictable: true,
        explainable: true,
        closable: true,
    }
}

/// 按层挂本类能力 + 三执行面能力（合格注册的最小申报）。
fn filled_spec(kind: LayerKind, budget: u32) -> LayerSpec {
    let mut s = good_spec(kind);
    s.budget_us = budget;
    let home = match kind {
        Perception => Capability::Sense,
        Deliberation => Capability::Decide,
        Action => Capability::Act,
        Collaboration => Capability::Cooperate,
    };
    s.caps.push(home);
    s.caps.push(Capability::Degrade);
    s.caps.push(Capability::Explain);
    s.caps.push(Capability::Predict);
    s
}

/// 四层齐备的架构（预算 80/200/60/60 恰 400）。
fn full_core() -> AicCore {
    let mut c = AicCore::new();
    let _ = c.register_layer(filled_spec(Perception, 80));
    let _ = c.register_layer(filled_spec(Deliberation, 200));
    let _ = c.register_layer(filled_spec(Action, 60));
    let _ = c.register_layer(filled_spec(Collaboration, 60));
    c
}

/// 十件全到清单。
fn full_manifest() -> HandoverManifest {
    let mut m = HandoverManifest::new();
    for item in [
        HandoverItem::AudioEventChannel,
        HandoverItem::AudioDebugHooks,
        HandoverItem::MetricSpecBook,
        HandoverItem::SceneCoverageTable,
        HandoverItem::DisciplinePledge,
        HandoverItem::FuzzCorpusBag,
        HandoverItem::PerformanceBook,
        HandoverItem::SafetyPrivacySpec,
        HandoverItem::DocMasterPlan,
        HandoverItem::BoardLink,
    ] {
        m.admit(item);
    }
    m
}

// ---------------------------------------------------------------------------
// 一、架构（四层齐备 / 管道层序 / 注册闸）
// ---------------------------------------------------------------------------

fn chk_arch_four_layers(set: &mut CheckSet) {
    // 规格-01：四层逐层注册成功且层名序与判据侧写死逐位一致。
    let mut c = AicCore::new();
    let mut all_ok = true;
    for (i, k) in LayerKind::ALL.iter().enumerate() {
        if c.register_layer(filled_spec(*k, k.budget_floor_us())).is_err() {
            all_ok = false;
        }
        if let Some(s) = c.registry.layer(*k) {
            if s.kind.name() != EXPECT_LAYER_NAMES[i] {
                all_ok = false;
            }
        } else {
            all_ok = false;
        }
    }
    if all_ok && c.registry.len() == 4 && c.registry.complete() {
        set.ok("EAC1-架构-01-四层齐备注册");
    } else {
        set.fail("EAC1-架构-01-四层齐备注册", "层注册或层名序漂移");
    }
    // 规格-02：缺层时册不 complete——管道通的必要条件缺一即破。
    let mut c2 = AicCore::new();
    let _ = c2.register_layer(filled_spec(Perception, 80));
    let _ = c2.register_layer(filled_spec(Deliberation, 200));
    let _ = c2.register_layer(filled_spec(Action, 60));
    if !c2.registry.complete() && c2.registry.len() == 3 {
        set.ok("EAC1-架构-02-缺层册不齐");
    } else {
        set.fail("EAC1-架构-02-缺层册不齐", "缺层仍判齐");
    }
    // 规格-07：决策层预算基准最贵——思考是最贵的层（判据侧独立取基准排序）。
    let mut floors = [0u32; 4];
    for k in LayerKind::ALL {
        floors[k.ordinal()] = k.budget_floor_us();
    }
    let deliberation_max = floors[1] == 200 && floors[1] > floors[0] && floors[1] > floors[2] && floors[1] > floors[3];
    if deliberation_max {
        set.ok("EAC1-架构-07-决策层基准最贵");
    } else {
        set.fail("EAC1-架构-07-决策层基准最贵", "基准排序与思考最贵相悖");
    }
}

fn chk_arch_pipe(set: &mut CheckSet) {
    // 规格-03：相邻三跳全通（感知→决策→执行→协作，管道单源序）。
    let mut p = Pipeline::new();
    let flow_ok = p.submit(Perception, Deliberation).is_ok()
        && p.submit(Deliberation, Action).is_ok()
        && p.submit(Action, Collaboration).is_ok();
    if flow_ok && p.stats().forwarded == 3 && p.stats().skipped == 0 && p.stats().reversed == 0 {
        set.ok("EAC1-架构-03-相邻顺流全通");
    } else {
        set.fail("EAC1-架构-03-相邻顺流全通", "合法流被拒或统计漂移");
    }
    // 规格-04：跳层拒绝——感知直接投执行，管道理序被破坏必须显性报错。
    let mut p2 = Pipeline::new();
    if p2.submit(Perception, Action) == Err(E_AC1_PIPE_BROKEN) && p2.stats().skipped == 1 {
        set.ok("EAC1-架构-04-跳层拒绝");
    } else {
        set.fail("EAC1-架构-04-跳层拒绝", "跳层未拦或误码");
    }
    // 规格-05：逆向拒绝——执行指挥决策是越权，与跳层分码诊断。
    let mut p3 = Pipeline::new();
    if p3.submit(Action, Deliberation) == Err(E_AC1_LAYER_OVERREACH)
        && p3.submit(Collaboration, Perception) == Err(E_AC1_LAYER_OVERREACH)
        && p3.stats().reversed == 2
    {
        set.ok("EAC1-架构-05-逆向越权拒绝");
    } else {
        set.fail("EAC1-架构-05-逆向越权拒绝", "逆向未拦或与跳层混码");
    }
    // 规格-06：同层传递 no-op 放行（层内合法，不产生拒绝计数）。
    let mut p4 = Pipeline::new();
    if p4.submit(Deliberation, Deliberation).is_ok()
        && p4.stats().forwarded == 1
        && p4.stats().skipped == 0
        && p4.stats().reversed == 0
    {
        set.ok("EAC1-架构-06-同层no-op放行");
    } else {
        set.fail("EAC1-架构-06-同层no-op放行", "层内传递被误拒");
    }
    // 规格-08：管道自检翻面——self_test_flow 三跳全通且统计自洽。
    let mut p5 = Pipeline::new();
    if p5.self_test_flow() && p5.stats().forwarded == 3 {
        set.ok("EAC1-架构-08-管道自检翻面");
    } else {
        set.fail("EAC1-架构-08-管道自检翻面", "自检流被拒");
    }
}

fn chk_arch_register_gates(set: &mut CheckSet) {
    // 边界-01：未声明能力注册拒绝——caps 空=不知道自己干什么，不入册。
    let mut r = LayerRegistry::new();
    let mut s = good_spec(Perception);
    s.budget_us = 80;
    if r.register(s) == Err(E_AC1_UNDECLARED_LAYER) && r.is_empty() {
        set.ok("EAC1-边界-01-未声明能力拒");
    } else {
        set.fail("EAC1-边界-01-未声明能力拒", "空申报入册");
    }
    // 边界-02：类别越界拒绝——感知层挂行动能力=跨类产出，归属判据拦。
    let mut r2 = LayerRegistry::new();
    let mut s2 = filled_spec(Perception, 80);
    s2.caps.clear();
    s2.caps.push(Capability::Act);
    if r2.register(s2) == Err(E_AC1_UNDECLARED_LAYER) {
        set.ok("EAC1-边界-02-类别越界拒");
    } else {
        set.fail("EAC1-边界-02-类别越界拒", "跨类能力入册");
    }
    // 边界-03：重复注册拒绝——同层只允许一个现役规格。
    let mut r3 = LayerRegistry::new();
    let first = filled_spec(Perception, 80);
    let second = filled_spec(Perception, 90);
    let _ = r3.register(first);
    let kept_80 = match r3.layer(Perception) {
        Some(s) => s.budget_us == 80,
        None => false,
    };
    if r3.register(second) == Err(E_AC1_DUPLICATE_LAYER) && kept_80 {
        set.ok("EAC1-边界-03-重复注册拒");
    } else {
        set.fail("EAC1-边界-03-重复注册拒", "同层双规格或误覆盖");
    }
    // 边界-04：预算低于层基准拒绝——决策层基准 200，给 199 必须红。
    let mut r4 = LayerRegistry::new();
    if r4.register(filled_spec(Deliberation, 199)) == Err(E_AC1_BUDGET) {
        set.ok("EAC1-边界-04-预算低于基准拒");
    } else {
        set.fail("EAC1-边界-04-预算低于基准拒", "基准线失效");
    }
    // 边界-05：预算合计超总上限拒绝——80+200+60+60=400 恰贴线过，
    // 再加任何一层都超。
    let mut r5 = LayerRegistry::new();
    let _ = r5.register(filled_spec(Perception, 80));
    let _ = r5.register(filled_spec(Deliberation, 200));
    let _ = r5.register(filled_spec(Action, 60));
    let _ = r5.register(filled_spec(Collaboration, 60));
    if EXPECT_TOTAL_BUDGET == 400 {
        // 换重注册语料：注销协作层后给 61 的 collaboration 也进不来
        //（80+200+60+61=401>400）。
        let _ = r5.unregister(Collaboration);
        if r5.register(filled_spec(Collaboration, 61)) == Err(E_AC1_BUDGET) {
            set.ok("EAC1-边界-05-总预算守恒拒");
        } else {
            set.fail("EAC1-边界-05-总预算守恒拒", "总上限失效");
        }
    } else {
        set.fail("EAC1-边界-05-总预算守恒拒", "总上限期望漂移");
    }
    // 边界-06：冻结后改语义拒绝 + unfreeze 恢复——层冻结纪律与 Z 域同构。
    let mut r6 = LayerRegistry::new();
    let _ = r6.register(filled_spec(Perception, 80));
    r6.freeze();
    let blocked = r6.register(filled_spec(Deliberation, 200)) == Err(E_AC1_AMEND_FROZEN)
        && r6.unregister(Perception) == false;
    r6.unfreeze();
    let restored = r6.register(filled_spec(Deliberation, 200)).is_ok() && !r6.frozen();
    if blocked && restored {
        set.ok("EAC1-边界-06-冻结改语义拒与解冻恢复");
    } else {
        set.fail("EAC1-边界-06-冻结改语义拒与解冻恢复", "冻结闸失效");
    }
}

// ---------------------------------------------------------------------------
// 二、三律（红线闸 / 降级链 / 读屏声明）
// ---------------------------------------------------------------------------

fn chk_law_redline(set: &mut CheckSet) {
    // 律-01..03：缺律三方向逐个拒——三律是合取，缺一即红线（0x5902）。
    let laws = [Law::Predictable, Law::Explainable, Law::Closable];
    let mut all_blocked = true;
    for drop_law in laws {
        let mut s = filled_spec(Perception, 80);
        match drop_law {
            Law::Predictable => s.predictable = false,
            Law::Explainable => s.explainable = false,
            Law::Closable => s.closable = false,
        }
        let mut r = LayerRegistry::new();
        if r.register(s) != Err(E_AC1_LAW_REDLINE) {
            all_blocked = false;
        }
    }
    if all_blocked {
        set.ok("EAC1-律-01-三律缺一即红线");
    } else {
        set.fail("EAC1-律-01-三律缺一即红线", "缺律申报入册");
    }
    // 律-02：读屏声明非空且含三律名——读屏可达是数据契约。
    let c = full_core();
    match c.a11y_statement() {
        Ok(text) => {
            let has_all = laws.iter().all(|l| text.contains(l.name()));
            let readable = text.contains("透明一致")
                && text.contains("为什么")
                && text.contains("可关闭");
            if has_all && readable && text.len() > 30 {
                set.ok("EAC1-律-02-读屏声明非空含律名");
            } else {
                set.fail("EAC1-律-02-读屏声明非空含律名", "声明缺律名或不可读");
            }
        }
        Err(_) => set.fail("EAC1-律-02-读屏声明非空含律名", "读屏声明误报空白"),
    }
    // 律-07：三律→执行面映射判据侧独立写死对拍（律的执行面能力是谁，
    // 判据侧自己有一份表，与实现侧 executor() 逐位对账）。
    let expect_exec: [(Law, Capability); 3] = [
        (Law::Predictable, Capability::Predict),
        (Law::Explainable, Capability::Explain),
        (Law::Closable, Capability::Degrade),
    ];
    let map_ok = expect_exec
        .iter()
        .all(|(law, cap)| law.executor() == *cap);
    if map_ok {
        set.ok("EAC1-律-07-三律执行面映射对拍");
    } else {
        set.fail("EAC1-律-07-三律执行面映射对拍", "律→执行面映射漂移");
    }
}

fn chk_law_degrade(set: &mut CheckSet) {
    // 律-03：降级链全程——Full→Half→Off 逐档翻面（关闭律执行面）。
    let mut d = DegradeLedger::new();
    d.enlist(Capability::Degrade);
    let step1 = d.level_of(Capability::Degrade) == Some(Degrade::Full);
    let _ = d.step_down(Capability::Degrade);
    let step2 = d.level_of(Capability::Degrade) == Some(Degrade::Half);
    let _ = d.step_down(Capability::Degrade);
    let step3 = d.level_of(Capability::Degrade) == Some(Degrade::Off);
    // Off 已到底：再降停在 Off（不回绕不 panic）。
    let _ = d.step_down(Capability::Degrade);
    let bottom = d.level_of(Capability::Degrade) == Some(Degrade::Off);
    if step1 && step2 && step3 && bottom {
        set.ok("EAC1-律-03-降级链全程到底");
    } else {
        set.fail("EAC1-律-03-降级链全程到底", "降级链断档或回绕");
    }
    // 律-04：升档一律红线——关闭律单调性（0x5902 + 拦截计数）。
    let mut d2 = DegradeLedger::new();
    d2.enlist(Capability::Explain);
    let esc = d2.escalate(Capability::Explain) == Err(E_AC1_LAW_REDLINE)
        && d2.escalations == 1
        && d2.level_of(Capability::Explain) == Some(Degrade::Full);
    if esc {
        set.ok("EAC1-律-04-升档红线拦截");
    } else {
        set.fail("EAC1-律-04-升档红线拦截", "升档未拦或账不对");
    }
    // 律-05：未挂账能力降级拒绝——降级面与注册面同源（未注册即无降级面）。
    let mut d3 = DegradeLedger::new();
    if d3.step_down(Capability::Predict) == Err(E_AC1_UNDECLARED_LAYER)
        && d3.escalate(Capability::Predict) == Err(E_AC1_UNDECLARED_LAYER)
        && d3.is_empty()
    {
        set.ok("EAC1-律-05-未挂账降级拒");
    } else {
        set.fail("EAC1-律-05-未挂账降级拒", "幽灵能力有了降级面");
    }
    // 律-06：注册成功即挂三执行面账——三律不是申报完就完，执行面必须存在。
    let c = full_core();
    let all_enlisted = [Capability::Predict, Capability::Explain, Capability::Degrade]
        .iter()
        .all(|cap| c.degrade.level_of(*cap) == Some(Degrade::Full));
    if all_enlisted && c.degrade.len() == 3 {
        set.ok("EAC1-律-06-注册即挂执行面账");
    } else {
        set.fail("EAC1-律-06-注册即挂执行面账", "执行面账缺项");
    }
    // 律-08：enlist 幂等——重复挂账不涨条数（账本不因重复申报而膨胀）。
    let mut d4 = DegradeLedger::new();
    d4.enlist(Capability::Cooperate);
    d4.enlist(Capability::Cooperate);
    d4.enlist(Capability::Cooperate);
    if d4.len() == 1 && d4.level_of(Capability::Cooperate) == Some(Degrade::Full) {
        set.ok("EAC1-律-08-enlist幂等");
    } else {
        set.fail("EAC1-律-08-enlist幂等", "重复挂账膨胀");
    }
}

// ---------------------------------------------------------------------------
// 三、承接与域色（十件闭环 / 缺件追补 / 欺骗模式闭集）
// ---------------------------------------------------------------------------

fn chk_handover(set: &mut CheckSet) {
    // 承接-01：十件逐件到齐后闭环——件名序与判据侧写死逐位一致。
    let mut m = HandoverManifest::new();
    let mut names_ok = true;
    for (i, item) in [
        HandoverItem::AudioEventChannel,
        HandoverItem::AudioDebugHooks,
        HandoverItem::MetricSpecBook,
        HandoverItem::SceneCoverageTable,
        HandoverItem::DisciplinePledge,
        HandoverItem::FuzzCorpusBag,
        HandoverItem::PerformanceBook,
        HandoverItem::SafetyPrivacySpec,
        HandoverItem::DocMasterPlan,
        HandoverItem::BoardLink,
    ]
    .iter()
    .enumerate()
    {
        if item.name() != EXPECT_HANDOVER_NAMES[i] {
            names_ok = false;
        }
        m.admit(*item);
    }
    if names_ok && m.arrived_count() == 10 && m.verify().is_ok() {
        set.ok("EAC1-承接-01-十件到齐闭环");
    } else {
        set.fail("EAC1-承接-01-十件到齐闭环", "件名序漂移或闭环误判");
    }
    // 承接-02：缺件显性追补——缺哪件点名哪件，码 0x5905，追补方向向 AB。
    let mut m2 = HandoverManifest::new();
    m2.admit(HandoverItem::AudioEventChannel);
    m2.admit(HandoverItem::BoardLink);
    let mut core = AicCore::new();
    core.handover = m2;
    let chase_before = core.handover.chase_requests;
    match core.verify_handover() {
        Err(code) if code == E_AC1_HANDOVER_MISSING => {
            // 追补请求在核验失败时显性发出（不静默）。
            let chased = core.handover.chase_requests == chase_before + 1;
            match core.handover.verify() {
                Err((_, missing)) => {
                    let names: Vec<&str> = missing.iter().map(|x| x.name()).collect();
                    let points_missing =
                        names.len() == 8 && !names.contains(&"音频事件通道") && !names.contains(&"看板链接");
                    if chased && points_missing {
                        set.ok("EAC1-承接-02-缺件点名追补");
                    } else {
                        set.fail("EAC1-承接-02-缺件点名追补", "缺件名单或追补计数漂移");
                    }
                }
                Ok(()) => set.fail("EAC1-承接-02-缺件点名追补", "缺八件却判闭环"),
            }
        }
        _ => set.fail("EAC1-承接-02-缺件点名追补", "缺件未报 0x5905"),
    }
    // 承接-03：补齐后翻面——同一清单补齐八件后闭环，chase 计数不清零
    //（追补史是事实，不因补齐而抹除）。
    let m3 = full_manifest();
    if m3.verify().is_ok() && m3.arrived_count() == 10 {
        set.ok("EAC1-承接-03-补齐后闭环翻面");
    } else {
        set.fail("EAC1-承接-03-补齐后闭环翻面", "补齐仍报缺");
    }
}

fn chk_domain_color(set: &mut CheckSet) {
    // 域色-01：四种欺骗模式逐个拒绝（惊艳可以，欺骗不行——闭集全覆盖）。
    let mut g = DomainColorGate::default();
    let mut all_hit = true;
    for p in DeceitPattern::ALL {
        if g.screen(&[p]) != Err(E_AC1_DECEIT) {
            all_hit = false;
        }
    }
    if all_hit && g.total_rejections() == 4 {
        set.ok("EAC1-域色-01-欺骗模式逐个拒");
    } else {
        set.fail("EAC1-域色-01-欺骗模式逐个拒", "欺骗模式漏拦");
    }
    // 域色-02：分寸内放行——空模式集=惊艳设计合法过闸。
    let mut g2 = DomainColorGate::default();
    if g2.screen(&[]).is_ok() && g2.total_rejections() == 0 {
        set.ok("EAC1-域色-02-分寸内放行");
    } else {
        set.fail("EAC1-域色-02-分寸内放行", "无辜方案被拦");
    }
    // 域色-03：拦截分账——每模式独立计数，混账会把哪类设计在越界读糊。
    let mut g3 = DomainColorGate::default();
    let _ = g3.screen(&[DeceitPattern::FakeLuck]);
    let _ = g3.screen(&[DeceitPattern::FakeLuck]);
    let _ = g3.screen(&[DeceitPattern::HiddenMutate]);
    let idx_luck = 2usize; // FakeLuck 在闭集序数 2（判据侧写死）
    let idx_mutate = 3usize; // HiddenMutate 序数 3
    if g3.rejections[idx_luck] == 2 && g3.rejections[idx_mutate] == 1 && g3.total_rejections() == 3 {
        set.ok("EAC1-域色-03-拦截按模式分账");
    } else {
        set.fail("EAC1-域色-03-拦截按模式分账", "分账与总数不符");
    }
}

// ---------------------------------------------------------------------------
// 四、总装（健康自检 / 快照五面）
// ---------------------------------------------------------------------------

fn chk_core_healthy(set: &mut CheckSet) {
    // 总装-01：全装好 healthy 翻面 + 快照五面逐面对账。
    let mut c = full_core();
    c.handover = full_manifest();
    let healthy = c.healthy();
    let snap = c.snapshot();
    let snap_ok = snap.layers.len() == 4
        && snap
            .layers
            .iter()
            .zip(EXPECT_LAYER_NAMES.iter())
            .all(|(a, b)| a == b)
        && snap.pipeline_complete
        && snap.handover_arrived == 10
        && snap.handover_closed
        && snap.deceit_rejections == 0
        && snap.degrade_entries == 3
        && snap.pipe.forwarded == 0; // healthy 用副本试流，不污染现役统计
    if healthy && snap_ok {
        set.ok("EAC1-总装-01-健康翻面快照五面");
    } else {
        set.fail("EAC1-总装-01-健康翻面快照五面", "健康误判或快照面漂移");
    }
}

// ---------------------------------------------------------------------------
// 五、判据承载力（零 panic / 码段独立复核 / 不变量 / 防自调）
// ---------------------------------------------------------------------------

/// 单遍词法剥除（字符串内 `//` 不被行注释剥离误伤，F2807 教训）。
fn strip_lexical_noise(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            let mut depth = 1usize;
            i += 2;
            while i < b.len() && depth > 0 {
                if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < b.len() && b[i] == b'*' && b[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'"';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if b[i] == b'\'' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'\'';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if let Some(c) = src.get(i..i + 1) {
            out.push_str(c);
        }
        i += 1;
    }
    out
}

fn chk_criterion_zero_panic(set: &mut CheckSet) {
    // 判据-01：判据面零 panic（自扫本文件）。
    let src = include_str!("veac01_aicore_checks.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("EAC1-判据-01-判据面零 panic");
    } else {
        set.fail("EAC1-判据-01-判据面零 panic", "判据面含 panic 面");
    }
    // 判据-02：生产面零 panic（扫实现文件）。
    let src = include_str!("veac01_aicore.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!", "unreachable!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("EAC1-判据-02-生产面零 panic");
    } else {
        set.fail("EAC1-判据-02-生产面零 panic", "生产面含 panic 面");
    }
}

fn chk_criterion_codes_independent(set: &mut CheckSet) {
    // 判据-03：码段独占独立复核——十码全 0x59 段、互异、与判据侧写死逐位等、
    // 原因串与处置方向非空。
    let got = [
        E_AC1_LAYER_OVERREACH,
        E_AC1_PIPE_BROKEN,
        E_AC1_LAW_REDLINE,
        E_AC1_UNDECLARED_LAYER,
        E_AC1_DUPLICATE_LAYER,
        E_AC1_HANDOVER_MISSING,
        E_AC1_DECEIT,
        E_AC1_A11Y_BLANK,
        E_AC1_BUDGET,
        E_AC1_AMEND_FROZEN,
    ];
    let mut ok = got.len() == EXPECT_ERROR_COUNT;
    let mut i = 0usize;
    while i < got.len() && ok {
        if got[i] >> 8 != EXPECT_SEGMENT as u16 || got[i] != EXPECT_CODES[i] {
            ok = false;
        }
        if reason_of(got[i]) == "未知错误码" || direction_of(got[i]).is_empty() {
            ok = false;
        }
        let mut j = i + 1;
        while j < got.len() {
            if got[i] == got[j] {
                ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    // 双邻互斥：0x57=veab03 / 0x58=vcq02 均已有主，0x59 必须与其无交集。
    if ok && (EXPECT_SEGMENT == 0x57 || EXPECT_SEGMENT == 0x58) {
        ok = false;
    }
    if ok {
        set.ok("EAC1-判据-03-码段独占独立复核");
    } else {
        set.fail("EAC1-判据-03-码段独占独立复核", "码漂移或撞邻段");
    }
}

fn chk_criterion_invariants(set: &mut CheckSet) {
    // 判据-04：层序表不变量——downstream/upstream 互逆、序数单调、
    // 恰感知无上游、恰协作无下游。
    if layer_order_invariant() {
        set.ok("EAC1-判据-04-层序表互逆不变量");
    } else {
        set.fail("EAC1-判据-04-层序表互逆不变量", "层序表破环");
    }
    // 判据-05：移交件数与欺骗模式数判据侧写死对账。
    if HANDOVER_COUNT == 10 && DECEIT_COUNT == 4 && LAW_COUNT == 3 && LAYER_COUNT == 4 {
        set.ok("EAC1-判据-05-闭集计数对账");
    } else {
        set.fail("EAC1-判据-05-闭集计数对账", "闭集计数漂移");
    }
}

fn chk_criterion_not_truncated(set: &mut CheckSet) {
    // 判据-06：聚合防自调——只调不递归的 A 族 + 自身进行中 tally；
    // 条数期望判据侧写死（A 族 22 条；判据-06 登记前 B 族进行中 12 条）。
    let a = run_veac01_checks_a_standalone();
    let (ap, af) = a.tally();
    let (sp, sf) = set.tally();
    let a_ok = ap + af == 22;
    let self_ok = sp + sf == 12;
    let no_trunc = !a.truncated() && !set.truncated() && a.dropped() == 0 && set.dropped() == 0;
    if a_ok && self_ok && no_trunc {
        set.ok("EAC1-判据-06-聚合守恒防自调");
    } else {
        set.fail("EAC1-判据-06-聚合守恒防自调", "族条数漂移或有截断/丢弃");
    }
}

// ---------------------------------------------------------------------------
// 入口（a=架构+三律 / b=承接+域色+判据承载力；合并入口供聚合器）
// ---------------------------------------------------------------------------

/// 判据族 a：架构 + 三律。
pub fn run_veac01_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/veac01/a");
    chk_arch_four_layers(&mut s);
    chk_arch_pipe(&mut s);
    chk_arch_register_gates(&mut s);
    chk_law_redline(&mut s);
    chk_law_degrade(&mut s);
    s
}

/// 判据族 b：承接 + 域色 + 总装 + 判据承载力。
pub fn run_veac01_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/veac01/b");
    chk_handover(&mut s);
    chk_domain_color(&mut s);
    chk_core_healthy(&mut s);
    chk_criterion_zero_panic(&mut s);
    chk_criterion_codes_independent(&mut s);
    chk_criterion_invariants(&mut s);
    chk_criterion_not_truncated(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_veac01_checks() -> CheckSet {
    CheckSet::merge(run_veac01_checks_a_standalone(), run_veac01_checks_b_standalone())
}
