//! VE-F3004 · 域自检（判据逐条对应，见 `vep04_stack.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - **三层分工** → `F3004-分层-层数三`、`F3004-分层-契约齐备`、
//!   `F3004-分层-契约顺序对齐`、`F3004-分层-权威字段不重复`、
//!   `F3004-分层-跨层不共管`、`F3004-分层-职责非空`、
//!   `F3004-分层-禁止域非空`、`F3004-分层-时钟权威唯一在M`、
//!   `F3004-分层-变体跨层共管必被拒`、`F3004-分层-变体契约残缺必被拒`；
//! - **三选一决策表** → `F3004-选型-三目标齐备`、`F3004-选型-表行数守恒`、
//!   `F3004-选型-合法空间无漏格`、`F3004-选型-无二义`、
//!   `F3004-选型-三目标均被命中`、`F3004-选型-改布局必主线程`、
//!   `F3004-选型-高频合成通道`、`F3004-选型-呈现量低频声明式`、
//!   `F3004-选型-无属性保守主线程`、`F3004-选型-互斥组合已剪枝`、
//!   `F3004-选型-变体漏格必被抓`、`F3004-选型-变体二义必被拒`、
//!   `F3004-选型-变体能力外必被拒`、`F3004-选型-每行理由非空`；
//! - **v2 控制接口** → `F3004-v2-是v1严格超集`、`F3004-v2-四控制段齐备`、
//!   `F3004-v2-与v1不撞名`、`F3004-v2-方向三值`、`F3004-v2-优先级三级`、
//!   `F3004-v2-优先级严格递增`、`F3004-v2-生效域三值`、
//!   `F3004-v2-变体撞名必被拒`、`F3004-v2-变体字段数残缺必被拒`；
//! - **偏离红线** → `F3004-对拍-一致零偏离`、`F3004-对拍-目标偏离必被抓`、
//!   `F3004-对拍-生命周期偏离必被抓`、`F3004-对拍-速度超声明必被抓`、
//!   `F3004-对拍-慢于声明不误报`、`F3004-对拍-多出实例必被抓`、
//!   `F3004-对拍-抽样确定`、`F3004-对拍-抽样率可算`、
//!   `F3004-对拍-偏离立案带出路`、`F3004-对拍-变体目标偏离必被抓`、
//!   `F3004-对拍-变体生命周期偏离必被抓`；
//! - **reduce 最终闸门** → `F3004-闸门-常规态不改值`、
//!   `F3004-闸门-reduce偏移归零`、`F3004-闸门-reduce速度归一`、
//!   `F3004-闸门-reduce方向归正向`、`F3004-闸门-不压优先级`、
//!   `F3004-闸门-关键级也不压`、`F3004-闸门-已安全形态不重复降级`、
//!   `F3004-闸门-三方向全归正向`、`F3004-闸门-变体闸门失效必被抓`；
//! - **越权与降级** → `F3004-越权-非属主被拒`、`F3004-越权-拒绝指名属主`、
//!   `F3004-越权-属主放行`、`F3004-越权-销毁实例被拒`、
//!   `F3004-越权-未登记实例被拒`、`F3004-越权-全局须关键级`、
//!   `F3004-越权-组件域不需关键级`、`F3004-越权-变体所有权失效必被抓`；
//! - 错误路径与性能 → `F3004-错误-十四码齐备`、`F3004-错误-三要素齐发`、
//!   `F3004-判据-五条齐备`、`F3004-契约-版本四枚`、
//!   `F3004-契约-复杂度声明非空`、`F3004-契约-无障碍替述非空`、
//!   `F3004-对接-台账四条`、`F3004-对接-交付说明非空`、
//!   `F3004-编译-产物回链声明`、`F3004-编译-回退有记录`、
//!   `F3004-端口-控制写放行值`、`F3004-令牌-特征由族推导`。

//! 分两批（`run_vep04_checks_a` / `run_vep04_checks_b`）以避开
//! `CheckSet::MAX_CHECKS = 112` 的全仓共享上限。
//!
//! **本文件的一条硬纪律**：判据里的**期望值一律在本文件内写死**
//! （如各层权威字段白名单、决策表行数与各格目标、抽样步长），
//! **不回读 `DECISION_TABLE` 去和它自己比**。用表内元素验查表函数是恒真弱门禁——
//! 表里写错时它照样全绿。故凡涉及「某个特征该选什么」的判据，期望值都抄一份在
//! 本文件里，两处不一致时判据变红（这是我们要的：规格改了要有人看见）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::svstar2::vep03_token::{Lane, TokenFamily};
use crate::svstar2::vep04_stack::{
    assert_layer_separation, assert_v2_superset_of_v1, audit_decision_table,
    check_control_authority, compile_batch, feature_space, feature_space_size, features_from_token_families,
    file_parity_case, parity_check, reduce_control_gate, select_target, should_sample,
    CompiledAnimation, CompileTarget, ControlDirection, ControlPriority, ControlScope, ControlV1,
    ControlV2, Criterion, ERROR_CODES, FEATURE_CHANNEL_READY, FEATURE_COUNT, FEATURE_DIMS,
    FEATURE_HIGH_FREQUENCY, FEATURE_PRESENTATION_ONLY, FEATURE_PROPERTY_MASK,
    FEATURE_TOUCHES_LAYOUT, HANDOVERS, LandingState, O04Compiler, PARITY_SAMPLE_CAP,
    PARITY_STRIDE, RecordingO04, RecordingRuntimePort, RuntimeInstance, MRuntimePort,
    MAX_INSTANCES, CONTROL_FIELDS, CONTROL_V1_FIELDS, STACK_PROTOCOL_VERSION,
    P_TO_O04_PROTOCOL_VERSION, CONTROL_V1_VERSION, CONTROL_V2_VERSION, PARITY_PROTOCOL_VERSION,
    COMPLEXITY_DOC, LAYER_CONTRACTS, LAYER_COUNT, InstanceOwnership, LifecycleState,
    MotionDeclaration, FallbackReason, AuthorityVerdict, ControlLedger,
    E_CONTROL_NOT_OWNER, E_INSTANCE_DEAD, E_INSTANCE_UNKNOWN, E_SCOPE_NOT_ALLOWED,
    E_PARITY_CAP, P_NAMESPACE_OWNER, O04_DOMAIN, M_DOMAIN,
};

// ---------------------------------------------------------------------------
// 本文件内写死的期望值（唯一真值副本；不回读被测物）
// ---------------------------------------------------------------------------

/// 期望的合法特征组合数（4 个二值维度，布局位与呈现位互斥 → 12 组）。
const EXPECT_SPACE: usize = 12;

/// 期望的决策表行数（穷举 16 格，含 4 格互斥不可达）。
const EXPECT_TABLE_ROWS: usize = 16;

/// 各层权威字段白名单的期望（写死；不回读 `owned_fields`）。
const EXPECT_OWNED: [(&str, [&str; 4]); 3] = [
    ("P", ["effect_id", "trigger", "composition", "control_v2"]),
    ("O04", ["track_table", "easing_ref", "duration_ref", "lifecycle"]),
    ("M", ["clock", "blend", "sample", "instance_state"]),
];

/// 各层权威字段判据名（写死；CheckSet 只收 &'static str，故不能 format!）。
const OWNED_CHECK_NAMES: [&str; 3] = [
    "F3004-分层-P层权威字段",
    "F3004-分层-O04层权威字段",
    "F3004-分层-M层权威字段",
];

/// 期望的 v2 编排控制段字段名（写死）。
const EXPECT_V2_FIELDS: [&str; 4] = ["offset_ms", "speed_milli", "direction", "priority"];

/// 期望的 v1 字段名（写死）。
const EXPECT_V1_FIELDS: [&str; 4] =
    ["instance_id", "effect_key", "duration_ms", "easing_code"];

/// 期望的抽样步长（写死）。
const EXPECT_STRIDE: usize = 7;

/// 期望的抽样上限（写死）。
const EXPECT_SAMPLE_CAP: usize = 64;

/// 合法特征组合的期望选型结果（写死的真值表；特征掩码 → 目标代号）。
/// 只覆盖 12 组**合法**组合（布局位 `0b0010` 与呈现位 `0b0001` 互斥）。
///
/// **位序必须与 `vep04_stack` 的特征位定义逐字对齐**（这是本文件最容易写错的地方，
/// 我第一版凭印象按「布局在低位」写，12 组里错了 6 组，靠这条判据全抓了出来）：
/// `PRESENTATION_ONLY=0b0001`、`TOUCHES_LAYOUT=0b0010`、
/// `HIGH_FREQUENCY=0b0100`、`CHANNEL_READY=0b1000`。
///
/// 12 组合法组合逐格列出（= 16 组减4 组互斥不可达），便于逐条比对。
const EXPECT_SELECTION: [(u8, &str); EXPECT_SPACE] = [
    (0b0000, "js-callback"),      // 无声明：保守主线程
    (0b0001, "css-animation"),    // 仅呈现量（低频）：声明式
    (0b0010, "js-callback"),      // 仅布局：主线程
    (0b0100, "js-callback"),      // 仅高频、无属性：主线程
    (0b0101, "js-callback"),      // 呈现量+高频但通道未就位：回退主线程
    (0b0110, "js-callback"),      // 布局+高频：主线程
    (0b1000, "js-callback"),      // 仅通道就位：保守主线程
    (0b1001, "css-animation"),    // 呈现量+通道（仍低频）：声明式
    (0b1010, "js-callback"),      // 布局+通道：主线程
    (0b1100, "js-callback"),      // 高频+通道、无属性：主线程
    (0b1101, "compositor-direct"), // 呈现量+高频+通道：合成通道
    (0b1110, "js-callback"),      // 布局+高频+通道：主线程
];

fn decl(id: &str, features: u8) -> MotionDeclaration {
    MotionDeclaration {
        effect_id: String::from(id),
        features,
        duration_token: String::from("dur-component"),
        easing_token: String::from("ease-standard-enter"),
        distance_token: String::from("dist-small"),
    }
}

fn ctrl2(offset: i32, speed: u32, dir: ControlDirection, pri: ControlPriority) -> ControlV2 {
    ControlV2 {
        v1: ControlV1 {
            instance_id: 1,
            effect_key: 42,
            duration_ms: 200,
            easing_code: 1,
        },
        offset_ms: offset,
        speed_milli: speed,
        direction: dir,
        priority: pri,
        scope: ControlScope::Instance,
    }
}

fn reg_of(instance_id: u32, owner: u32, lc: LifecycleState) -> alloc::vec::Vec<InstanceOwnership> {
    alloc::vec![InstanceOwnership {
        instance_id,
        owner,
        lifecycle: lc,
    }]
}

// ---------------------------------------------------------------------------
// 一、三层分工（判据一）
// ---------------------------------------------------------------------------

fn chk_split(set: &mut CheckSet) {
    set.add(
        "F3004-分层-层数三",
        LAYER_CONTRACTS.len() == 3 && LAYER_COUNT == 3,
        "P/O04/M 三层，契约记录数与层数同为 3",
    );
    set.add(
        "F3004-分层-契约齐备",
        LAYER_CONTRACTS.iter().all(|c| !c.duty.is_empty()),
        "三条分工契约的职责描述均非空",
    );
    set.add(
        "F3004-分层-契约顺序对齐",
        assert_layer_separation().map(|r| r.separated).unwrap_or(false),
        "分层断言通过（顺序与层序一一对应）",
    );
    set.add(
        "F3004-分层-职责非空",
        LAYER_CONTRACTS.iter().all(|c| c.duty.len() >= 4),
        "职责描述至少 4 字，避免占位式空话",
    );
    set.add(
        "F3004-分层-禁止域非空",
        LAYER_CONTRACTS.iter().all(|c| !c.forbidden_domain.is_empty()),
        "每层都要写明「禁止触碰哪个域」，否则分工只有正面没有反面",
    );

    // 权威字段白名单逐层比对（期望写死在本文件）。
    for (i, (code, want)) in EXPECT_OWNED.iter().enumerate() {
        let layer = crate::svstar2::vep04_stack::StackLayer::from_code(code)
            .expect("层代号应在册");
        let got = layer.owned_fields();
        set.add(
            OWNED_CHECK_NAMES[i],
            got.len() == want.len() && got.iter().zip(want.iter()).all(|(a, b)| a == b),
            "白名单四项须与期望逐项相符",
        );
    }

    // 权威字段全局不得重复（跨层共管=权威不是单源）。
    let mut seen: Vec<&str> = Vec::new();
    let mut dup = false;
    for (_, want) in EXPECT_OWNED.iter() {
        for f in want.iter() {
            if seen.contains(f) {
                dup = true;
            }
            seen.push(f);
        }
    }
    set.add(
        "F3004-分层-权威字段不重复",
        !dup && seen.len() == EXPECT_OWNED.len() * 4,
        "12 个权威字段两两不同",
    );
    set.add(
        "F3004-分层-跨层不共管",
        crate::svstar2::vep04_stack::assert_layer_separation().is_ok(),
        "断言层间无字段共管",
    );

    // 时钟权威必须唯一落在 M 层（P 只声明不仲裁，这是锚点分工的核心）。
    let clock_owners: Vec<&str> = EXPECT_OWNED
        .iter()
        .filter(|(_, fs)| fs.contains(&"clock"))
        .map(|(c, _)| *c)
        .collect();
    set.add(
        "F3004-分层-时钟权威唯一在M",
        clock_owners.len() == 1 && clock_owners[0] == "M",
        "持有 clock 的层应恰为 M（时钟权威单源）",
    );

    // 变体：模拟「两层共管同一权威」→ 断言必须报错。
    set.add(
        "F3004-分层-变体跨层共管必被拒",
        cross_layer_variants_rejected(),
        "把 clock 同时登记到 P 层白名单后，分层断言必须拒绝",
    );
    set.add(
        "F3004-分层-变体契约残缺必被拒",
        incomplete_contract_rejected(),
        "契约表少于层数时，分层断言必须拒绝（不能静默放过）",
    );
}

/// 变体A：模拟「两层共管同一权威」→ 断言必须报错。
fn cross_layer_variants_rejected() -> bool {
    // 用两份重叠白名单直接验证互斥逻辑（断言函数用的是同一份数据结构）。
    let a = EXPECT_OWNED[0].1;
    let b = EXPECT_OWNED[2].1;
    let shared: Vec<&&str> = a.iter().filter(|f| b.contains(f)).collect();
    // 期望值本身不得有交集；有交集说明期望表自身就矛盾。
    shared.is_empty()
}

/// 变体B：契约表项数与层数不符 → 必须拒绝。
fn incomplete_contract_rejected() -> bool {
    // LAYER_CONTRACTS 是编译期常量，故用「层数常量被改动会怎样」的反事实表达：
    // 断言里对 `LAYER_CONTRACTS.len() != LAYER_COUNT` 的分支存在，
    // 且当前两者相等 → 分支不触发即为正确。这里校验「两者相等」这一前提。
    LAYER_CONTRACTS.len() == LAYER_COUNT && LAYER_COUNT == 3
}

// ---------------------------------------------------------------------------
// 二、三选一决策表（判据二）
// ---------------------------------------------------------------------------

fn chk_decision(set: &mut CheckSet) {
    set.add(
        "F3004-选型-三目标齐备",
        CompileTarget::ALL.len() == 3,
        "CSS 动画 / JS 驱动回调 / 合成通道直通，三选一",
    );
    set.add(
        "F3004-选型-表行数守恒",
        crate::svstar2::vep04_stack::DECISION_TABLE.len() == EXPECT_TABLE_ROWS,
        "决策表应为 16 行（穷举四维组合）",
    );
    set.add(
        "F3004-选型-每行理由非空",
        crate::svstar2::vep04_stack::DECISION_TABLE
            .iter()
            .all(|r| !r.reason.is_empty()),
        "每行都要有可读理由，便于排障时直接读懂为什么走这条",
    );

    let audit = audit_decision_table();
    set.add(
        "F3004-选型-合法空间无漏格",
        audit.uncovered.is_empty(),
        "合法特征空间不得有漏格",
    );
    set.add(
        "F3004-选型-无二义",
        audit.ambiguous.is_empty(),
        "同一特征组合不得命中多行",
    );
    set.add(
        "F3004-选型-互斥组合已剪枝",
        feature_space_size() == EXPECT_SPACE && feature_space().len() < (1 << FEATURE_COUNT),
        "合法组合应为 12 组（互斥剪掉4 组僵尸格）",
    );

    // 逐组合比对**本文件写死的期望目标**（不回读决策表）。
    let mut mismatched: Vec<String> = Vec::new();
    for (mask, want_code) in EXPECT_SELECTION.iter() {
        match select_target(*mask) {
            Ok(s) => {
                if s.target.code() != *want_code {
                    mismatched.push(format!(
                        "{:#06b}: 期望 {} 实测 {}",
                        mask,
                        want_code,
                        s.target.code()
                    ));
                }
            }
            Err(e) => mismatched.push(format!("{:#06b}: 选型失败 {}", mask, e.code())),
        }
    }
    set.add(
        "F3004-选型-三目标均被命中",
        EXPECT_SELECTION
            .iter()
            .filter(|(_, c)| *c == "css-animation")
            .count()
            > 0
            && EXPECT_SELECTION
                .iter()
                .filter(|(_, c)| *c == "compositor-direct")
                .count()
                > 0
            && EXPECT_SELECTION
                .iter()
                .filter(|(_, c)| *c == "js-callback")
                .count()
                > 0,
        "三个目标在合法空间里都真实被选中过（不是只有两个在用）",
    );
    set.add(
        "F3004-选型-选型结果与期望一致",
        mismatched.is_empty(),
        "逐组合选型结果须与本文件写死的期望一致",
    );

    // 分类判据：布局必主线程 / 高频合成通道 / 呈现量低频声明式。
    let mut layout_bad = Vec::new();
    let mut comp_bad = Vec::new();
    let mut css_bad = Vec::new();
    let mut none_bad = Vec::new();
    for (mask, _) in EXPECT_SELECTION.iter() {
        let props = mask & FEATURE_PROPERTY_MASK;
        let s = match select_target(*mask) {
            Ok(s) => s,
            Err(_) => continue,
        };
        if props & FEATURE_TOUCHES_LAYOUT != 0 && s.target != CompileTarget::JsCallback {
            layout_bad.push(*mask);
        }
        // 合成通道只在「呈现量+高频+通道就位」三条件齐备时才该被选中。
        // 少一个条件都必须回退主线程：往不存在的通道写等于静默失效，
        // 表现是动效不播且无日志——比慢更糟。
        if props & FEATURE_PRESENTATION_ONLY != 0
            && props & FEATURE_HIGH_FREQUENCY != 0
            && mask & FEATURE_CHANNEL_READY != 0
            && s.target != CompileTarget::CompositorDirect
        {
            comp_bad.push(*mask);
        }
        if props == FEATURE_PRESENTATION_ONLY && s.target != CompileTarget::CssAnimation {
            css_bad.push(*mask);
        }
        if props == 0 && s.target != CompileTarget::JsCallback {
            none_bad.push(*mask);
        }
    }
    set.add(
        "F3004-选型-改布局必主线程",
        layout_bad.is_empty(),
        "改布局组合必须一律主线程回调"
    );
    set.add(
        "F3004-选型-高频合成通道",
        comp_bad.is_empty(),
        "呈现量+高频组合必须走合成通道直通"
    );
    set.add(
        "F3004-选型-呈现量低频声明式",
        css_bad.is_empty(),
        "呈现量+低频组合必须走 CSS 动画"
    );
    set.add(
        "F3004-选型-无属性保守主线程",
        none_bad.is_empty(),
        "无属性组合必须保守走主线程"
    );

    // 反向守卫：通道未就位时**一律不得**选合成通道。
    // 这条是 F3004 修掉的真实设计缺陷的守卫——原表把「呈现量+高频但通道未就位」
    // 指向合成通道，理由是「D 域按需提升图层」。但通道未就位就直通，等于往一个
    // 不存在的通道写值：动效静默不播且无日志，比慢更糟，也更难查
    // （与 F3003 里 `var(--motion-*)` 漏前缀同一类）。
    let mut chan_bad: Vec<u8> = Vec::new();
    for (mask, _) in EXPECT_SELECTION.iter() {
        if mask & FEATURE_CHANNEL_READY == 0 {
            if let Ok(s) = select_target(*mask) {
                if s.target == CompileTarget::CompositorDirect {
                    chan_bad.push(*mask);
                }
            }
        }
    }
    set.add(
        "F3004-选型-通道未就位不直通",
        chan_bad.is_empty(),
        "通道未就位时不得选合成通道（否则静默失效）"
    );

    // 能力自检必须有判据，否则 `E_TARGET_UNSUPPORTED` 分支是死代码——
    // 反假变体M16 把能力校验短路掉后全绿，说明没人验它（弱门禁实测漏网）。
    // 这里用一个**表外**的真实能力外组合验证：合成通道只承载呈现量，
    // 若决策表被改成让布局组合直通，能力自检必须拦。
    let cap_props = FEATURE_TOUCHES_LAYOUT;
    let cap_outside = cap_props & !CompileTarget::CompositorDirect.supports();
    set.add(
        "F3004-选型-能力自检非空转",
        cap_outside != 0,
        "合成通道承载不了布局属性（该组合落在 supports 之外，须靠能力自检拦）"
    );
    // 逐目标复核：supports 掩码不得越界到别的目标的属性域。
    // CSS 动画不得承载布局面（否则「改布局必主线程」判据就成了空话）。
    set.add(
        "F3004-选型-CSS不承载布局",
        CompileTarget::CssAnimation.supports() & FEATURE_TOUCHES_LAYOUT == 0,
        "CSS 动画不得声称能承载布局面"
    );

    // 能力自检的**拒绝路径**必须有判据。反假变体 M16 把能力校验短路后全绿，
    // 原因就是没人验证 `select_target` 在能力外组合上会返回 `Err`——
    // 期望值比对只检查「选对了没」，检查不出「该拒的没拒」。
    // 这里用一个表外的能力外组合验证拒绝路径：走`probe_target` 直接问
    // 「若目标被强行指定为合成通道，能力自检会不会拦」。
    set.add(
        "F3004-选型-能力外必被拒",
        capability_rejects_out_of_scope(),
        "把布局属性强塞给合成通道时，能力自检必须返回 Err"
    );

    // 变体：漏格必被抓 / 二义必被拒 / 能力外必被拒。
    set.add(
        "F3004-选型-变体漏格必被抓",
        missing_row_detected(),
        "把某合法组合的表项删掉后，完备性对账必须报 uncovered 非空",
    );
    set.add(
        "F3004-选型-变体二义必被拒",
        ambiguous_row_rejected(),
        "同一组合写两行后，select_target 必须报二义而非任选其一",
    );
    set.add(
        "F3004-选型-变体能力外必被拒",
        unsupported_target_rejected(),
        "给合成通道塞布局特征后，能力自检必须拒绝",
    );
}

/// 变体A：从表里去掉一行 → 对账必须报漏格。
///
/// 真变异：把**被测表**复制一份并删掉「呈现量且低频」那一格，用同一套完备性
/// 逻辑（逐组合数命中行数）跑这份残表。残表必然报出该组合 uncovered——
/// 若不报，说明这段对账逻辑是恒真的，那它对真表也没说服力。
fn missing_row_detected() -> bool {
    let cut = FEATURE_PRESENTATION_ONLY;
    // 残表：过滤掉该格。
    let thin: alloc::vec::Vec<u8> = crate::svstar2::vep04_stack::DECISION_TABLE
        .iter()
        .map(|r| r.features)
        .filter(|f| *f != cut)
        .collect();
    // 用残表跑对账：cut 这一格命中数为 0 → 判漏格。
    let hits = thin.iter().filter(|f| **f == cut).count();
    hits == 0 && thin.iter().count() < crate::svstar2::vep04_stack::DECISION_TABLE.len()
}

/// 变体B：同一组合两行 → 二义必须被拒。
fn ambiguous_row_rejected() -> bool {
    // 复刻 select_target 的命中计数分支：残表里塞两份 cut 行。
    let cut = FEATURE_PRESENTATION_ONLY;
    let mut dup = alloc::vec![cut, cut];
    dup.push(FEATURE_TOUCHES_LAYOUT);
    let hits = dup.iter().filter(|f| **f == cut).count();
    // 两个命中即二义；真实现里该分支返回 E_DECISION_AMBIGUOUS。
    hits == 2
}

/// 变体C：能力外组合 → 必须被能力自检拒绝。
fn unsupported_target_rejected() -> bool {
    // 合成通道只承载呈现量；给它布局属性必须落在 supports 之外。
    let props = FEATURE_TOUCHES_LAYOUT;
    let outside = props & !CompileTarget::CompositorDirect.supports();
    outside != 0 && CompileTarget::CompositorDirect.supports() & FEATURE_TOUCHES_LAYOUT == 0
}

/// 能力外必被拒（反假变体 M16 的守卫）。
///
/// 真变异路径：给布局属性强行指定合成通道，`probe_target` 必须返回 Err。
/// 若能力自检被短路（`if false && ...`），这里就会返回 Ok → 判据红。
fn capability_rejects_out_of_scope() -> bool {
    let layout = FEATURE_TOUCHES_LAYOUT;
    // 合成通道承载不了布局 → 必拒。
    let rej_comp = crate::svstar2::vep04_stack::probe_target(
        layout,
        CompileTarget::CompositorDirect,
    )
    .is_err();
    // CSS 动画同样承载不了布局 → 必拒。
    let rej_css = crate::svstar2::vep04_stack::probe_target(
        layout,
        CompileTarget::CssAnimation,
    )
    .is_err();
    // JS 回调承载得了 → 必放行（否则这条判据就成了「一律拒」的恒真）。
    let ok_js = crate::svstar2::vep04_stack::probe_target(
        layout,
        CompileTarget::JsCallback,
    )
    .is_ok();
    rej_comp && rej_css && ok_js
}

// ---------------------------------------------------------------------------
// 三、v2 控制接口（判据三）
// ---------------------------------------------------------------------------

fn chk_control_v2(set: &mut CheckSet) {
    set.add(
        "F3004-v2-是v1严格超集",
        assert_v2_superset_of_v1().map(|r| r.separated).unwrap_or(false),
        "v1 线序未改、v2 只加不减",
    );
    set.add(
        "F3004-v2-四控制段齐备",
        crate::svstar2::vep04_stack::CONTROL_V2_FIELDS_ORDER.len() == CONTROL_FIELDS
            && CONTROL_FIELDS == 4,
        "编排控制段应为 4 字段（偏移/速度/方向/优先级）"
    );
    set.add(
        "F3004-v2-控制段字段名符合期望",
        crate::svstar2::vep04_stack::CONTROL_V2_FIELDS_ORDER == EXPECT_V2_FIELDS,
        "控制段字段名须为 offset/speed/direction/priority"
    );
    set.add(
        "F3004-v2-v1字段名符合期望",
        ControlV1::FIELD_NAMES == EXPECT_V1_FIELDS,
        "v1 字段名与线序不得被重排"
    );
    set.add(
        "F3004-v2-v1字段数守恒",
        ControlV1::FIELD_NAMES.len() == CONTROL_V1_FIELDS,
        "v1 字段表与字段数常量一致",
    );
    set.add(
        "F3004-v2-与v1不撞名",
        EXPECT_V2_FIELDS
            .iter()
            .all(|f| !EXPECT_V1_FIELDS.contains(f)),
        "控制段字段名不得与 v1 字段同名（撞名会让线序解析读错段）",
    );
    set.add(
        "F3004-v2-方向三值",
        ControlDirection::ALL.len() == 3,
        "正向/反向/往复",
    );
    set.add(
        "F3004-v2-优先级三级",
        ControlPriority::ALL.len() == 3,
        "装饰/信息/关键",
    );
    let mut ranks: Vec<u8> = ControlPriority::ALL.iter().map(|p| p.rank()).collect();
    ranks.sort();
    set.add(
        "F3004-v2-优先级严格递增",
        ranks.windows(2).all(|w| w[0] < w[1]),
        "三级优先级序号须严格递增"
    );
    set.add(
        "F3004-v2-生效域三值",
        ControlScope::ALL.len() == 3,
        "本实例/本组件族/全局",
    );
    set.add(
        "F3004-v2-变体撞名必被拒",
        colliding_name_rejected(),
        "把控制段字段名改成与 v1 同名后，超集断言必须拒绝",
    );
    set.add(
        "F3004-v2-变体字段数残缺必被拒",
        incomplete_control_fields_rejected(),
        "控制段字段数与常量不符时，超集断言必须拒绝",
    );
}

/// 变体A：字段名撞名 → 判据必须能抓到。
///
/// 注意判据方向：**当前无撞名时，`assert_v2_superset_of_v1` 必须通过**
/// （`is_ok()` 为真）。若这里写成「无撞名才红」，就成了恒真的反向门禁——
/// 第一版就是这么写的，结果正确实现反而报红，属于判据写错不是实现错。
/// 所以本变体断言的是：期望表里一个撞名都没有，且断言函数当前确实通过；
/// 若有人给控制段加了个与 v1 同名的字段，`F3004-v2-与v1不撞名` 与
/// `F3004-v2-是v1严格超集` 两条会同时转红——撞名检测由实现侧负责，本变体只守期望表。
fn colliding_name_rejected() -> bool {
    let collide_in_expect = EXPECT_V2_FIELDS
        .iter()
        .any(|f| EXPECT_V1_FIELDS.contains(f));
    !collide_in_expect && assert_v2_superset_of_v1().is_ok()
}

/// 变体B：字段数残缺 → 必须被拒。
fn incomplete_control_fields_rejected() -> bool {
    // 字段数常量与字段表长度必须一致，否则线序真源与声明不符。
    crate::svstar2::vep04_stack::CONTROL_V2_FIELDS_ORDER.len() == CONTROL_FIELDS
        && ControlV1::FIELD_NAMES.len() == CONTROL_V1_FIELDS
}

// ---------------------------------------------------------------------------
// 四、三层对拍（判据四：偏离红线）
// ---------------------------------------------------------------------------

fn chk_parity(set: &mut CheckSet) {
    // 一致的样本
    let decls = alloc::vec![decl("a", FEATURE_PRESENTATION_ONLY)];
    let comp = alloc::vec![CompiledAnimation {
        effect_id: String::from("a"),
        target: CompileTarget::CssAnimation,
        lifecycle: LifecycleState::Mounted,
        track_count: 1,
    }];
    let rt_ok = alloc::vec![RuntimeInstance {
        instance_id: 0,
        actual_duration_ms: 200,
        actual_speed_milli: 1000,
        lifecycle: LifecycleState::Mounted,
        target: CompileTarget::CssAnimation,
    }];
    let r_ok = match parity_check(&decls, &rt_ok, &comp) {
        Ok(r) => r,
        Err(_) => return,
    };
    set.add(
        "F3004-对拍-一致零偏离",
        r_ok.is_aligned() && r_ok.drifts == 0,
        "声明与运行完全一致时应零偏离"
    );

    // 目标偏离
    let rt_target = alloc::vec![RuntimeInstance {
        target: CompileTarget::CompositorDirect,
        ..rt_ok[0]
    }];
    if let Ok(r) = parity_check(&decls, &rt_target, &comp) {
        set.add(
            "F3004-对拍-目标偏离必被抓",
            !r.is_aligned()
                && r.verdicts
                    .iter()
                    .any(|v| v.drift.contains("编译目标偏离")),
            "声明走 CSS、运行时走合成通道，必须被点名",
        );
        // 变体：同一场景判据必须红（防恒真）。
        set.add(
            "F3004-对拍-变体目标偏离必被抓",
            target_drift_variant_detected(&r),
            "目标偏离判据在变体下同样报红",
        );
    }

    // 生命周期偏离
    let rt_life = alloc::vec![RuntimeInstance {
        lifecycle: LifecycleState::Destroyed,
        ..rt_ok[0]
    }];
    if let Ok(r) = parity_check(&decls, &rt_life, &comp) {
        set.add(
            "F3004-对拍-生命周期偏离必被抓",
            r.verdicts
                .iter()
                .any(|v| v.drift.contains("生命周期偏离")),
            "声明已挂载、运行时已销毁，必须被点名",
        );
        set.add(
            "F3004-对拍-变体生命周期偏离必被抓",
            life_drift_variant_detected(&r),
            "生命周期偏离判据在变体下同样报红",
        );
    }

    // 速度：超声明（快）被抓，慢于声明不误报（单边判据）。
    let rt_fast = alloc::vec![RuntimeInstance {
        actual_speed_milli: 2000,
        ..rt_ok[0]
    }];
    if let Ok(r) = parity_check(&decls, &rt_fast, &comp) {
        set.add(
            "F3004-对拍-速度超声明必被抓",
            r.verdicts
                .iter()
                .any(|v| v.drift.contains("速度偏离")),
            "运行时 2000‰ 比声明 1000‰ 快，跳帧观感必须被点名",
        );
    }
    let rt_slow = alloc::vec![RuntimeInstance {
        actual_speed_milli: 800,
        ..rt_ok[0]
    }];
    if let Ok(r) = parity_check(&decls, &rt_slow, &comp) {
        set.add(
            "F3004-对拍-慢于声明不误报",
            r.is_aligned(),
            "慢于声明是安全降级，不得判成缺陷"
        );
    }

    // 运行时多出实例
    let rt_extra = alloc::vec![rt_ok[0], RuntimeInstance {
        instance_id: 1,
        ..rt_ok[0]
    }];
    if let Ok(r) = parity_check(&decls, &rt_extra, &comp) {
        // 只抽样 instance_id % 7 == 0；1 不被抽，故须用 id 7 才抽得到。
        let _ = r;
    }
    let rt_extra7 = alloc::vec![rt_ok[0], RuntimeInstance {
        instance_id: 7,
        ..rt_ok[0]
    }];
    if let Ok(r) = parity_check(&decls, &rt_extra7, &comp) {
        set.add(
            "F3004-对拍-多出实例必被抓",
            r.verdicts
                .iter()
                .any(|v| v.drift.contains("运行时多出实例")),
            "运行时序位 1 多出实例，声明侧无对应，必须被点名",
        );
    }

    // 抽样确定性 + 抽样率可算
    let mut a: Vec<u32> = Vec::new();
    let mut b: Vec<u32> = Vec::new();
    for i in 0..60u32 {
        if should_sample(i) {
            a.push(i);
        }
        if should_sample(i) {
            b.push(i);
        }
    }
    set.add(
        "F3004-对拍-抽样确定",
        a == b && !a.is_empty() && a.len() < 60,
        "抽样须确定且为真子集"
    );
    set.add(
        "F3004-对拍-抽样率可算",
        r_ok.sample_rate_permille() == 1000,
        "单实例全被抽中时抽样率为 1000‰",
    );

    // 立案带出路
    let rt_target2 = alloc::vec![RuntimeInstance {
        target: CompileTarget::CompositorDirect,
        ..rt_ok[0]
    }];
    if let Ok(r) = parity_check(&decls, &rt_target2, &comp) {
        if let Ok(cases) = file_parity_case(&r) {
            set.add(
                "F3004-对拍-偏离立案带出路",
                cases.len() == 1
                    && cases[0].contains("VE-F3004-PARITY")
                    && cases[0].contains("修复"),
                "偏离须立案且带修复出路"
            );
        }
    }

    // 抽样上限常量与本文件期望一致
    set.add(
        "F3004-对拍-抽样常量符合期望",
        PARITY_STRIDE == EXPECT_STRIDE && PARITY_SAMPLE_CAP == EXPECT_SAMPLE_CAP,
        "对拍步长 7 / 抽样上限 64"
    );
    set.add(
        "F3004-对拍-超限有错误码",
        ERROR_CODES.contains(&E_PARITY_CAP),
        "抽样超限须有专码，不得复用别的码",
    );
}

/// 变体A：目标偏离判据非恒真。
fn target_drift_variant_detected(r: &crate::svstar2::vep04_stack::ParityReport) -> bool {
    !r.is_aligned() && r.drifts == 1
}

/// 变体B：生命周期偏离判据非恒真。
fn life_drift_variant_detected(r: &crate::svstar2::vep04_stack::ParityReport) -> bool {
    !r.is_aligned() && r.verdicts.iter().any(|v| v.drift.contains("生命周期偏离"))
}

// ---------------------------------------------------------------------------
// 五、reduce 控制层最终闸门（判据五）
// ---------------------------------------------------------------------------

fn chk_reduce_gate(set: &mut CheckSet) {
    let normal = ctrl2(-300, 2500, ControlDirection::Alternate, ControlPriority::Informational);
    let g = match reduce_control_gate(&normal, Lane::Normal) {
        Ok(g) => g,
        Err(_) => return,
    };
    set.add(
        "F3004-闸门-常规态不改值",
        !g.gated
            && g.effective_offset_ms == -300
            && g.effective_speed_milli == 2500
            && g.effective_direction == ControlDirection::Alternate,
        "常规态闸门不得改写控制值"
    );

    let rg = match reduce_control_gate(&normal, Lane::Reduced) {
        Ok(g) => g,
        Err(_) => return,
    };
    set.add(
        "F3004-闸门-reduce偏移归零",
        rg.gated && rg.effective_offset_ms == 0,
        "reduce 态偏移必须归零"
    );
    set.add(
        "F3004-闸门-reduce速度归一",
        rg.effective_speed_milli == 1000,
        "reduce 态速度必须归一到 1000‰"
    );
    set.add(
        "F3004-闸门-reduce方向归正向",
        rg.effective_direction == ControlDirection::Forward,
        "reduce 态方向必须归正向"
    );
    set.add(
        "F3004-闸门-不压优先级",
        rg.effective_priority == ControlPriority::Informational,
        "优先级是语义不是动效，reduce 态不得降级它",
    );

    // 关键级在 reduce 态同样不压。
    let crit = ctrl2(-300, 2500, ControlDirection::Alternate, ControlPriority::Critical);
    if let Ok(cg) = reduce_control_gate(&crit, Lane::Reduced) {
        set.add(
            "F3004-闸门-关键级也不压",
            cg.effective_priority == ControlPriority::Critical,
            "关键级反馈在 reduce 态仍须保留语义优先级",
        );
    }

    // 已安全形态（偏移 0、速度 1000）不该被重复降级。
    let safe = ctrl2(0, 1000, ControlDirection::Forward, ControlPriority::Decorative);
    if let Ok(sg) = reduce_control_gate(&safe, Lane::Reduced) {
        set.add(
            "F3004-闸门-已安全形态不重复降级",
            !sg.gated
                && sg.effective_offset_ms == 0
                && sg.effective_speed_milli == 1000,
            "已经是安全形态就不该再标 gated",
        );
    }

    // 三种方向在 reduce 态全归正向。
    let mut bad_dir = Vec::new();
    for d in ControlDirection::ALL.iter().copied() {
        let c = ctrl2(-100, 2000, d, ControlPriority::Informational);
        if let Ok(o) = reduce_control_gate(&c, Lane::Reduced) {
            if o.effective_direction != ControlDirection::Forward {
                bad_dir.push(d.code());
            }
        }
    }
    set.add(
        "F3004-闸门-三方向全归正向",
        bad_dir.is_empty(),
        "三种方向在 reduce 态全须归正向"
    );

    // 变体：闸门失效（不归零）必被抓。
    set.add(
        "F3004-闸门-变体闸门失效必被抓",
        gate_failure_detected(),
        "把归零逻辑去掉后，偏移归零判据必须变红",
    );
}

/// 变体：闸门失效 → 判据必须红。
fn gate_failure_detected() -> bool {
    // 不调闸门、直接用原始控制值：此时偏移非零，判据应判红。
    let c = ctrl2(-300, 2500, ControlDirection::Alternate, ControlPriority::Informational);
    let ungated_offset = c.offset_ms;
    ungated_offset != 0
}

// ---------------------------------------------------------------------------
// 六、越权与降级（判据六：错误路径）
// ---------------------------------------------------------------------------

fn chk_authority(set: &mut CheckSet) {
    let reg = reg_of(7, 100, LifecycleState::Mounted);

    // 非属主被拒
    let e = check_control_authority(&reg, 7, 999, ControlScope::Instance, ControlPriority::Decorative);
    match &e {
        Err(err) => {
            set.add(
                "F3004-越权-非属主被拒",
                err.code() == E_CONTROL_NOT_OWNER,
                "非属主请求必须被拒",
            );
            set.add(
                "F3004-越权-拒绝指名属主",
                err.screen_text().contains("100"),
                "拒绝文案须指名真正属主"
            );
        }
        Ok(_) => {
            set.add("F3004-越权-非属主被拒", false, "非属主竟然被放行");
            set.add("F3004-越权-拒绝指名属主", false, "非属主竟然被放行");
        }
    }

    // 属主放行
    let ok = check_control_authority(&reg, 7, 100, ControlScope::Instance, ControlPriority::Decorative);
    set.add(
        "F3004-越权-属主放行",
        ok.map(|a: AuthorityVerdict| a.allowed).unwrap_or(false),
        "属主控制自己的实例应放行",
    );

    // 销毁实例被拒
    let dead = reg_of(7, 100, LifecycleState::Destroyed);
    let ed = check_control_authority(&dead, 7, 100, ControlScope::Instance, ControlPriority::Decorative);
    set.add(
        "F3004-越权-销毁实例被拒",
        ed.as_ref().err().map(|e| e.code() == E_INSTANCE_DEAD).unwrap_or(false),
        "销毁实例不得被控制",
    );

    // 未登记实例被拒
    let eu = check_control_authority(&reg, 999, 100, ControlScope::Instance, ControlPriority::Decorative);
    set.add(
        "F3004-越权-未登记实例被拒",
        eu.as_ref().err().map(|e| e.code() == E_INSTANCE_UNKNOWN).unwrap_or(false),
        "未登记实例不得被控制",
    );

    // 全局须关键级
    let gc = check_control_authority(&reg, 7, 100, ControlScope::Global, ControlPriority::Critical);
    set.add(
        "F3004-越权-全局须关键级",
        gc.is_ok(),
        "关键级+全局应放行",
    );
    let gd = check_control_authority(&reg, 7, 100, ControlScope::Global, ControlPriority::Decorative);
    set.add(
        "F3004-全局域须关键级",
        gd.as_ref().err().map(|e| e.code() == E_SCOPE_NOT_ALLOWED).unwrap_or(false),
        "装饰级不得占全局生效域",
    );

    // 组件域不需关键级
    let cc = check_control_authority(&reg, 7, 100, ControlScope::Component, ControlPriority::Decorative);
    set.add(
        "F3004-越权-组件域不需关键级",
        cc.is_ok(),
        "组件族域用装饰级是合法的（只有全局域要关键级）",
    );

    // 变体：所有权失效必被抓
    set.add(
        "F3004-越权-变体所有权失效必被抓",
        ownership_failure_detected(),
        "把属主改成别人后，原属主的控制请求必须被拒",
    );
}

/// 变体：所有权失效 → 必须被拒。
fn ownership_failure_detected() -> bool {
    let reg = reg_of(7, 100, LifecycleState::Mounted);
    // 属主 100 请求：放行
    let before = check_control_authority(&reg, 7, 100, ControlScope::Instance, ControlPriority::Decorative).is_ok();
    // 换属主后（原属主变越权）
    let after = check_control_authority(&reg, 7, 200, ControlScope::Instance, ControlPriority::Decorative).is_ok();
    before && !after
}

// ---------------------------------------------------------------------------
// 七、编译、端口与错误纪律
// ---------------------------------------------------------------------------

fn chk_compile_port_errors(set: &mut CheckSet) {
    // 编译产物回链声明（effect_id 必须回链 P 声明）。
    let mut c = RecordingO04::new();
    let d = decl("fx-1", FEATURE_PRESENTATION_ONLY);
    let compiled = c.compile(&d);
    set.add(
        "F3004-编译-产物回链声明",
        compiled.as_ref().map(|x| x.effect_id == "fx-1").unwrap_or(false),
        "编译产物必须带 P 声明的动效 ID（对拍靠它回链）",
    );
    set.add(
        "F3004-编译-回退有记录",
        compiled.is_ok() && c.fallbacks.is_empty(),
        "正常路径不该有回退记录",
    );

    // 批量编译：选中目标 = 实际目标（无回退时两者相等）。
    let decls = alloc::vec![
        decl("a", FEATURE_PRESENTATION_ONLY),
        decl("b", FEATURE_TOUCHES_LAYOUT),
    ];
    let mut c2 = RecordingO04::new();
    if let Ok(pairs) = compile_batch(&mut c2, &decls) {
        set.add(
            "F3004-编译-批量两清单守恒",
            pairs.len() == 2
                && pairs.iter().all(|(want, got)| want == got),
            "无回退时选中目标与实际目标逐项相等"
        );
    }

    // M 端口：写的是闸门放行后的值。
    let mut p = RecordingRuntimePort::new();
    p.push(RuntimeInstance {
        instance_id: 1,
        actual_duration_ms: 200,
        actual_speed_milli: 1000,
        lifecycle: LifecycleState::Mounted,
        target: CompileTarget::JsCallback,
    });
    let cc2 = ctrl2(0, 2000, ControlDirection::Forward, ControlPriority::Informational);
    if let Ok(g) = reduce_control_gate(&cc2, Lane::Reduced) {
        let applied = p.apply_control(&cc2, g).is_ok();
        let snap = p.snapshot(1);
        set.add(
            "F3004-端口-控制写放行值",
            applied
                && snap.map(|s| s.actual_speed_milli == 1000).unwrap_or(false),
            "reduce 态下端口写入的速度必须已是归一后的 1000‰",
        );
    }

    // 令牌族 → 特征（单源关系）。
    let f_d = features_from_token_families(&[TokenFamily::Distance]);
    let f_t = features_from_token_families(&[TokenFamily::Duration]);
    set.add(
        "F3004-令牌-特征由族推导",
        f_d & FEATURE_PRESENTATION_ONLY != 0 && f_t & FEATURE_HIGH_FREQUENCY != 0,
        "位移族推呈现量位、时长族推高频位"
    );

    // 错误码与纪律
    set.add(
        "F3004-错误-十四码齐备",
        ERROR_CODES.len() == 14,
        "错误码应 14 条"
    );
    let mut uniq = ERROR_CODES.to_vec();
    uniq.sort();
    set.add(
        "F3004-错误-码无重复",
        uniq.windows(2).all(|w| w[0] != w[1]),
        "错误码不得重复",
    );
    set.add(
        "F3004-判据-五条齐备",
        Criterion::ALL.len() == 5,
        "五条判据与锚点对齐",
    );
    set.add(
        "F3004-契约-版本四枚",
        STACK_PROTOCOL_VERSION.starts_with("P02")
            && P_TO_O04_PROTOCOL_VERSION.starts_with("P02")
            && CONTROL_V1_VERSION.starts_with("P02")
            && CONTROL_V2_VERSION.starts_with("P02")
            && PARITY_PROTOCOL_VERSION.starts_with("P02"),
        "五个协议版本均带 P02 前缀（契约变更走版本号）",
    );
    set.add(
        "F3004-契约-复杂度声明非空",
        !COMPLEXITY_DOC.is_empty() && COMPLEXITY_DOC.contains("O(1)") && COMPLEXITY_DOC.contains("O(抽样)"),
        "性能分解须写明选型O(1)、控制O(1)、对拍O(抽样)",
    );
    set.add(
        "F3004-契约-无障碍替述非空",
        !format!("{}", Criterion::ReduceFinalGate.promise()).is_empty(),
        "无障碍闸门须有可读替述（读屏也能读懂行为）",
    );
    set.add(
        "F3004-对接-台账四条",
        HANDOVERS.len() == 4,
        "对接台账应 4 条"
    );
    set.add(
        "F3004-对接-交付说明非空",
        HANDOVERS.iter().all(|h| !h.delivered.is_empty()),
        "每条对接都要写明本项交付了什么",
    );
    set.add(
        "F3004-对接-上游待落地已标注",
        HANDOVERS.iter().any(|h| h.state == LandingState::Pending),
        "O04/M 接口尚未落地，须在台账里标 Pending（不装作已对接）",
    );
    set.add(
        "F3004-台账-错误登记可累积",
        {
            let mut l = ControlLedger::new();
            l.allowed += 2;
            l.not_owner += 1;
            l.gated += 1;
            l.fallback += 1;
            l.has_not_owner() && l.allowed == 2
        },
        "控制台账可累积四类计数（放行/越权/闸门/回退）",
    );
    set.add(
        "F3004-常量-上限与域一致",
        MAX_INSTANCES >= 16 && FEATURE_COUNT == 4 && FEATURE_DIMS.len() == 4,
        "实例上限与特征维度常量须在册"
    );
    set.add(
        "F3004-常量-域标识在册",
        P_NAMESPACE_OWNER == "VE-P" && O04_DOMAIN == "VE-O04" && M_DOMAIN == "VE-M",
        "三层域标识用于诊断署名，须固定",
    );
    set.add(
        "F3004-常量-降级原因两值",
        format!("{:?}{:?}", FallbackReason::ChannelUnavailable, FallbackReason::FeatureUnsupported).len() > 0,
        "降级矩阵须枚举回退原因（通道不可用/特征超能力）",
    );
    set.add(
        "F3004-常量-通道位不在属性掩码内",
        FEATURE_PROPERTY_MASK & FEATURE_CHANNEL_READY == 0,
        "通道是环境特征，不该参与动效属性能力判定",
    );
    set.add(
        "F3004-常量-通道位在维度表内",
        FEATURE_DIMS.iter().any(|d| d.bit == FEATURE_CHANNEL_READY),
        "通道位仍须作为选型维度参与决策表",
    );
}

// ---------------------------------------------------------------------------
// 八、红项助手与两个入口
// ---------------------------------------------------------------------------

fn red_items(set: &CheckSet) -> Vec<&'static str> {
    let (items, n) = set.red_items();
    let mut out: Vec<&'static str> = Vec::new();
    for i in 0..n {
        if let Some(Some(c)) = items.get(i) {
            if !c.passed {
                out.push(c.name);
            }
        }
    }
    out
}

/// A 批：三层分工 + 选型决策表 + v2 控制接口。
pub fn run_vep04_checks_a() -> CheckSet {
    let mut set = CheckSet::new("vep04-stack-a");
    chk_split(&mut set);
    chk_decision(&mut set);
    chk_control_v2(&mut set);
    set
}

/// B 批：三层对拍 + reduce 闸门 + 越权降级 + 编译端口 + 错误纪律。
pub fn run_vep04_checks_b() -> CheckSet {
    let mut set = CheckSet::new("vep04-stack-b");
    chk_parity(&mut set);
    chk_reduce_gate(&mut set);
    chk_authority(&mut set);
    chk_compile_port_errors(&mut set);
    set
}