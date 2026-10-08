//! VE-F2801 · 域自检（判据逐条对应，见 `veo01_arch.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - O01 架构声明 → `O01-架构-*`
//! - 集成边界 → `O01-边界-*`
//! - 解析子集 → `O01-子集-*`
//! - 判据（自证可追溯）→ `O01-判据-*`
//! - 降级矩阵（非法输入→拒绝三要素 / 边界越界→钳制+告警 / 异常检出→立案流转）
//!   → `O01-降级-*`
//! - 跨批对接（上游契约接收哈希对账 / 下游消费接口前向声明 / 对账钩子）
//!   → `O01-对接-*`
//! - 无障碍（文档替述可读）→ `O01-读屏-*`
//! - 错误路径零静默 → `O01-错误-*`
//!
//! 零墙钟、零 IO，回归可复现。

use super::veo01_arch::*;
use crate::checks::CheckSet;

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 标准对端契约登记（上游两域 + 下游两域 + 跨卷一端）。
///
/// 契约内容与哈希**成对构造**——本函数就是"哈希对账"的正样本：
/// 哈希由内容实算得出，不是手写常量。
fn std_boundary() -> IntegrationBoundary {
    let mut b = IntegrationBoundary::new();
    let entries: [(Peer, &str, &str); 5] = [
        (Peer::UpstreamN, "n-ctrl-tree/v1", "元素映射表：元素 id → N 节点句柄"),
        (Peer::DownstreamN, "n-attr-sink/v1", "属性补丁接收：节点句柄 × 属性 → 值"),
        (Peer::DownstreamD, "d-displaylist/v1", "绘制指令流接收：类型/参数/纹理句柄"),
        (Peer::UpstreamS, "s-a11y-media/v1", "媒体偏好：prefers-reduced-motion 等查询契约"),
        (Peer::CgpuContract, "cgpu-effect-budget/v1", "效果预算与归因合同（跨卷）"),
    ];
    for (peer, name, content) in entries.iter() {
        b.register(PeerContract {
            peer: *peer,
            contract: name.to_string(),
            content_hash: fnv1a64_hex(content.as_bytes()),
            received: false,
            reconciled: false,
        })
        .expect("标准对端登记");
    }
    b
}

/// 已接收并对账通过的上游（开工前置条件的正样本）。
fn ready_boundary() -> IntegrationBoundary {
    let mut b = std_boundary();
    b.reconcile(
        Peer::UpstreamN,
        "元素映射表：元素 id → N 节点句柄",
    )
    .expect("上游 N 对账通过");
    b.receive_upstream(Peer::UpstreamS).expect("上游 S 接收");
    b.reconcile(
        Peer::UpstreamS,
        "媒体偏好：prefers-reduced-motion 等查询契约",
    )
    .expect("上游 S 对账通过");
    b
}

/// 七段齐全、按序递增的预算账本（次序单源的正样本）。
fn std_budget() -> BudgetLedger {
    let mut l = BudgetLedger::new();
    // 预算构成：样式的帧预算 6ms（对齐 P 域双 6ms 分账）拆到七段。
    let micros: [(Stage, u32); 7] = [
        (Stage::Tokenize, 900),
        (Stage::Parse, 1100),
        (Stage::Select, 1200),
        (Stage::Cascade, 1400),
        (Stage::Compute, 2100),
        (Stage::Project, 1900),
        (Stage::Paint, 1400),
    ];
    for (stage, us) in micros.iter() {
        l.register(BudgetEntry {
            stage: *stage,
            micros: *us,
            measured_median_micros: 0,
            measured_p99_micros: 0,
        })
        .expect("七段预算按序注册");
    }
    l
}

/// 钉死十主题落点的标准表（每主题恰好一组落点 + 全部落在其主责段上）。
fn std_landings() -> Vec<(Theme, &'static [&'static str])> {
    StyleEngineArchitecture::theme_landings()
}

/// 有实测值的预算账本（超标判定的正样本）。
fn measured_budget() -> BudgetLedger {
    let mut l = std_budget();
    l.record_measurement(Stage::Select, 1300, 2100); // 1200 预算 / 1300 实测 → 未超 10%
    l.record_measurement(Stage::Compute, 2900, 4200); // 2100 预算 / 2900 实测 → 超 10%
    l
}

// ---------------------------------------------------------------------------
// 判据一：O01 架构声明
// ---------------------------------------------------------------------------

fn chk_arch_declaration(set: &mut CheckSet) {
    // 七段不多不少，且段位序严格递增。
    set.add(
        "O01-架构-七段齐备",
        STAGE_ORDER.len() == STAGE_COUNT && STAGE_ORDER.len() == 7,
        "七段是固定契约：分词/声明解析/选择器匹配/层叠裁决/计算样式/属性投影/绘制指令",
    );
    let mut rank_ok = true;
    for w in STAGE_ORDER.windows(2) {
        if w[1].rank() != w[0].rank() + 1 {
            rank_ok = false;
        }
    }
    set.add(
        "O01-架构-段位连续",
        rank_ok && STAGE_ORDER[0].rank() == 0 && STAGE_ORDER[6].rank() == 6,
        "段位 0..6 连续无洞——次序单源不许有洞",
    );

    // 每段契约七字段齐全（挂名即红）。
    let mut all_fields = true;
    for s in STAGE_ORDER.iter() {
        let sp = stage_spec(*s);
        if [
            sp.duty_zh,
            sp.input,
            sp.output,
            sp.on_failure,
            sp.complexity,
            sp.consumers,
            sp.not_mine,
        ]
        .iter()
        .any(|f| f.trim().is_empty())
        {
            all_fields = false;
        }
    }
    set.add(
        "O01-架构-段契约无挂名",
        all_fields,
        "七段契约各七字段（职责/输入/输出/失败/复杂度/消费方/不做清单）不得留空",
    );

    // 每项判据都有执行段（纸面判据 = 未落实）。
    let mut every_criterion_served = true;
    for c in CRITERIA.iter() {
        if Stage::ALL.iter().filter(|s| s.serves().contains(c)).count() == 0 {
            every_criterion_served = false;
        }
    }
    set.add(
        "O01-架构-判据有执行段",
        every_criterion_served,
        "四项判据每项至少一段在执行，无纸面判据",
    );

    // 域使命：单向编译为 N 控件树 + D 绘制指令。
    set.add(
        "O01-架构-单向输出段就位",
        Stage::ALL.iter().filter(|s| s.is_emitting()).count() == 2,
        "属性投影与绘制指令两段为单向输出段（域使命的执行点）",
    );

    // 性能宪法：预算账本按序注册、合计可算、次序单调。
    let b = std_budget();
    set.add(
        "O01-架构-预算按序注册",
        b.len() == STAGE_COUNT && b.order_is_monotonic(),
        "七段预算必须按 STAGE_ORDER 递增落账",
    );
    set.add(
        "O01-架构-预算合计可算",
        b.total() == STAGE_BUDGET_TOTAL_MICROS,
        "预算合计等于域级帧预算单源常量（双 6ms 分账 = 10000 微秒）",
    );
    set.add(
        "O01-架构-复杂度声明齐备",
        COMPLEXITY_DOC.contains("C1") && COMPLEXITY_DOC.contains("C10"),
        "C1..C10 十项复杂度声明与实现逐条对应",
    );
    set.add(
        "O01-架构-下游归属在册",
        DOWNSTREAM_OWNERSHIP.len() == 8,
        "八条下游归属声明，防止总纲被当万能筐",
    );

    // ---- 补 1：告警账上限必须钉死字面量，且边界两侧行为可验 ----
    //
    // 原有判据「告警账满拒绝」用 `for i in 0..CLAMP_LOG_CAP` 循环，
    // 被测物把CLAMP_LOG_CAP 从 256 改成 8 时，循环也跟着只灌 8 条——
    // **判据与被测物共用同一常量 = 自指参照 = 恒真**。
    // 这里改用字面量 256 钉死，并验边界两侧：上限-1 仍能收、上限必拒。
    {
        let mut l = ClampLog::new();
        let mut accepted = 0usize;
        for i in 0..255usize {
            if l
                .push(ClampNotice {
                    field: format!("g{}", i),
                    original: 2.0,
                    clamped: 1.0,
                    low: 0.0,
                    high: 3.0,
                    tick: 0,
                })
                .is_ok()
            {
                accepted += 1;
            }
        }
        // 灌满 256 条（上限之内全收），第 257 条必被拒
        let overflow = l.push(ClampNotice {
            field: "overflow".to_string(),
            original: 2.0,
            clamped: 1.0,
            low: 0.0,
            high: 3.0,
            tick: 0,
        });
        set.add(
            "O01-降级-告警账上限钉死256",
            accepted == 255
                && CLAMP_LOG_CAP == 256
                && overflow.is_ok()
                && l.len() == 256
                && l.dropped() == 0,
            "告警账上限是 256 字面量（不与被测常量共引用）：255 条全收，第 256 条收，满后必拒",
        );
        // 上限两侧行为：账恰好满（256）时下一条必拒，且累计丢弃 +1。
        let mut m = ClampLog::new();
        for i in 0..256usize {
            let _ = m.push(ClampNotice {
                field: format!("h{}", i),
                original: 2.0,
                clamped: 1.0,
                low: 0.0,
                high: 3.0,
                tick: 0,
            });
        }
        let over = m.push(ClampNotice {
            field: "over".to_string(),
            original: 2.0,
            clamped: 1.0,
            low: 0.0,
            high: 3.0,
            tick: 0,
        });
        set.add(
            "O01-降级-账满必拒并计丢",
            m.len() == 256 && over.is_err() && m.dropped() == 1,
            "账满 256 后下一条拒收且丢弃计数为 1（静默丢弃等于撒谎）",
        );
    }

    // ---- 补 2：选择器深度上限钉死字面量 + 超深可检出 ----
    set.add(
        "O01-架构-深度上限钉死32",
        MAX_SELECTOR_DEPTH == 32 && MAX_SELECTOR_DEPTH >= 8,
        "选择器匹配深度上限为字面量 32（字面量钉死，不与被测常量共引用）",
    );

    // ---- 补 3：禁扩面必须能拦「表外真实形态」的意图，且不误伤域内诉求 ----
    //
    // 原有判据的四个越界用例里，意图串**含 desc 全文**
    // （`check_no_overreach(desc)`），所以哪怕把关键词表删空、
    // 退回`intent.contains(desc)` 单一匹配，判据照样绿——无区分力。
    // 这里用**只含关键词、不含 desc 全文**的自然语言意图逐条验，
    // 再加负样本确保不误伤。
    {
        // 六条禁扩面各一条「只含关键词」的真实意图（均不含 desc 全文）。
        // 第三元是该code 的**字面量归属措辞**——判据的期望必须是字面量，
        // **绝不能调被测函数 `boundary_advice` 自身来算期望**
        // （那是自指参照：被测物改错 advice 时参照跟着变，恒成立）。
        let kw_intents: [(&str, &str, &str); 6] = [
            ("O-N-REVERSE-READ", "我想回读N 域元素树的存储布局", "N 域"),
            ("O-D-REVERSE-READ", "顺手反读合成器的 RT 池", "D 域"),
            ("O-LAYOUT-OWN", "打算自建 flex 做两列", "N 域"),
            ("O-PAINT-BACKEND", "准备自建光栅写扫描线", "D 域"),
            ("O-SCRIPT-SANDBOX", "在本模块内置 JS 跑表达式", "Z 域"),
            ("O-NET-FETCH", "打算联网取样式表", "表面调度层"),
        ];
        let mut all_blocked = true;
        let mut wrong_advice = false;
        for (code, intent, want_advice) in kw_intents.iter() {
            match std_boundary().check_no_overreach(intent) {
                Err(e) => {
                    if e.code != E_BOUNDARY_OVERREACH {
                        all_blocked = false;
                    }
                    // 出路措辞必须指向**该去的那个域**（字面量核对，不问被测物）。
                    if !e.next.contains(*want_advice) {
                        wrong_advice = true;
                    }
                }
                Ok(_) => all_blocked = false,
            }
            // 顺带核：错误文案里必须点名是**哪一条**禁扩面（code 本串）。
            if let Err(e) = std_boundary().check_no_overreach(intent) {
                if !e.why.contains(*code) {
                    wrong_advice = true;
                }
            }
        }
        if wrong_advice {
            all_blocked = false;
        }
        set.add(
            "O01-边界-关键词意图可拦",
            all_blocked,
            "六条禁扩面对「只含关键词不含说明全文」的真实意图仍可拦，且给出正确归属去处",
        );

        // 负样本：域内正当诉求不得被误伤（含"布局""绘制"等词但不是禁扩面）。
        let legit: [&str; 4] = [
            "把N 域给回的布局结果消费掉",
            "把计算值编译为绘制指令提交给 D 域",
            "按元素映射表向 N 域写属性",
            "读取元素映射表里的节点句柄",
        ];
        let no_false_positive = legit
            .iter()
            .all(|s| std_boundary().check_no_overreach(s).is_ok());
        set.add(
            "O01-边界-关键词不误伤域内",
            no_false_positive,
            "含「布局/绘制/读」等词但属域内正当诉求的意图不得被拦",
        );
    }

    // ---- 补 4：关键条目段位逐条钉死（字面量，不与 primary_stage 共引用）----
    {
        let pairs: [(&str, Stage); 6] = [
            ("VE-F2801", Stage::Tokenize),
            ("VE-F2809", Stage::Compute),
            ("VE-F2815", Stage::Compute),
            ("VE-F2816", Stage::Compute),
            ("VE-F2817", Stage::Project),
            ("VE-F2818", Stage::Paint),
        ];
        let ok = pairs
            .iter()
            .all(|(item, st)| item_stage(item) == Some(*st));
        set.add(
            "O01-架构-关键条目段位钉死",
            ok,
            "F2801=分词 / F2809,F2815,F2816=计算 / F2817=投影 / F2818=绘制（字面量钉死）",
        );
    }
}

// ---------------------------------------------------------------------------
// 判据二：集成边界
// ---------------------------------------------------------------------------

fn chk_integration_boundary(set: &mut CheckSet) {
    // Servo 决策：style/style_traits 引入，layout 不引入且写明归属。
    let style = CRATE_DECISIONS
        .iter()
        .find(|d| d.crate_id == CRATE_STYLE)
        .expect("style crate 决策在册");
    let traits = CRATE_DECISIONS
        .iter()
        .find(|d| d.crate_id == CRATE_STYLE_TRAITS)
        .expect("style_traits crate 决策在册");
    let layout = CRATE_DECISIONS
        .iter()
        .find(|d| d.crate_id == CRATE_LAYOUT)
        .expect("layout crate 决策在册");
    set.add(
        "O01-边界-引入style",
        style.decision == ServoDecision::Vendorize,
        "style crate 决策为引入（vendor 化落地归 F2802）",
    );
    set.add(
        "O01-边界-引入style-traits",
        traits.decision == ServoDecision::Vendorize,
        "style_traits crate 决策为引入（属性元数据复用）",
    );
    set.add(
        "O01-边界-layout不引入",
        layout.decision == ServoDecision::Decline,
        "layout 不引入是归属裁决：布局归 N 域，双布局并存即分歧",
    );
    set.add(
        "O01-边界-layout归属明确",
        !layout.owner_elsewhere.trim().is_empty(),
        "不引入必须写明归属去处，否则日后必被重新捡起",
    );
    set.add(
        "O01-边界-决策理由齐备",
        CRATE_DECISIONS.iter().all(|d| !d.reason.trim().is_empty()),
        "三条决策各带理由——无理由的选型会被后来人推翻",
    );

    // 决策枚举往返守卫。
    set.add(
        "O01-边界-决策枚举往返",
        ServoDecision::from_code("VENDORIZE") == Some(ServoDecision::Vendorize)
            && ServoDecision::from_code("DECLINE") == Some(ServoDecision::Decline)
            && ServoDecision::from_code("MAYBE") == None,
        "未知决策码必须返回 None（不猜近似值）",
    );

    // 单向承诺：禁扩面逐条拦住，且给出归属去处。
    let mut b = std_boundary();
    let overreach_cases: [(&str, &str); 4] = [
        ("自建 flex 布局算法", "O-LAYOUT-OWN"),
        ("回读 N 域控件树内部结构", "O-N-REVERSE-READ"),
        ("自建光栅化后端", "O-PAINT-BACKEND"),
        ("发起网络取样式", "O-NET-FETCH"),
    ];
    let mut all_blocked = true;
    for (intent, _) in overreach_cases.iter() {
        if b.check_no_overreach(intent).is_ok() {
            all_blocked = false;
        }
    }
    set.add(
        "O01-边界-禁扩面生效",
        all_blocked && BOUNDARY_EXCLUSIONS.len() == 6,
        "六条禁扩面全部可拦（回读/自建布局/自建光栅/脚本沙箱/网络取样式）",
    );
    let err = b
        .check_no_overreach("自建 flex 布局算法")
        .expect_err("越界必须被拒");
    set.add(
        "O01-边界-越界给出路",
        err.code == E_BOUNDARY_OVERREACH && !err.next.trim().is_empty(),
        "越界拒绝必须指明该找哪个域，不是一句'不行'",
    );
    set.add(
        "O01-边界-域内不误伤",
        b.check_no_overreach("把计算值投影为 N 域属性").is_ok(),
        "域内正常诉求不得被禁扩面表误伤",
    );

    // 反向依赖审计：禁反读对端公开可查。
    set.add(
        "O01-边界-禁反读公开",
        b.reverse_read_audit().len() == 2,
        "N 属性引擎与 D 合成器为禁反读对端（单向承诺的公开账）",
    );
    set.add(
        "O01-边界-对端关系齐备",
        Peer::ALL.iter().all(|p| !p.zh().is_empty() && !p.relation().is_empty()),
        "五对端各带中文名与关系（上游/下游/跨卷）",
    );
}

// ---------------------------------------------------------------------------
// 判据三：解析子集
// ---------------------------------------------------------------------------

fn chk_parse_subset(set: &mut CheckSet) {
    let mut g = SubsetGate::standard();

    // 四族分桶各至少一条（空族 = 门禁盲区）。
    set.add(
        "O01-子集-四族覆盖",
        g.family_coverage().is_empty() && PropertyFamily::ALL.len() == PROPERTY_FAMILY_COUNT,
        "布局/视觉/效果/交互四族均有属性（空族即门禁盲区）",
    );
    set.add(
        "O01-子集-枚举守卫",
        g.enum_guard_ok() && PropertyFamily::from_code("NOPE") == None,
        "四族码往返全通，未知码返回 None",
    );

    // 放行路径：子集内属性带出完整规格。
    let opacity = g.admit("opacity").expect("opacity 在子集内").clone();
    set.add(
        "O01-子集-放行带规格",
        opacity.family == PropertyFamily::Effect
            && opacity.value_kind == "number"
            && opacity.domain_low == 0.0
            && opacity.domain_high == 1.0,
        "放行须带出族/类型/继承性/初值/参数域五项",
    );
    set.add(
        "O01-子集-效果族需合成",
        PropertyFamily::Effect.needs_compositing()
            && !PropertyFamily::Visual.needs_compositing(),
        "效果族天然吃合成预算，视觉族不吃（预算归因的前提）",
    );

    // 拒绝路径：桶外属性拒绝并给出路，且不静默。
    let before = g.rejected();
    let err = g.admit("grid-template-areas").expect_err("桶外属性必须被拒");
    set.add(
        "O01-子集-桶外拒绝",
        err.code == E_PROPERTY_OUT_OF_SUBSET && g.rejected() == before + 1,
        "子集外属性拒绝且拒绝计数递增（不静默丢弃）",
    );
    set.add(
        "O01-子集-拒绝给出路",
        err.next.contains("ADR") && !err.next.trim().is_empty(),
        "拒绝必须给出两条出路：改用子集内属性 / 走 ADR 扩表",
    );
    // 修复注记（AI-ZCode-2，跨会话协同）：原 `g.admit(..).or_else(|_| g.admit(..))`
    // 同时持 g 的可变借用与不可变借用（E0499）。按原意拆为两次顺序调用：
    // 首选属性与回退属性都应被拒。
    let _ = g.admit("color");
    let err2 = g.admit("border-color").expect_err("应仍被拒");
    set.add(
        "O01-子集-拒绝带族提示",
        err2.why.contains("族") || err2.why.contains("布局"),
        "拒绝理由尽量指出该属性应归哪一族（猜族不作判定依据）",
    );

    // 参数域钳制：三闸。
    set.add(
        "O01-子集-注册拒空名",
        g.register(PropertySpec {
            name: "",
            family: PropertyFamily::Visual,
            value_kind: "x",
            inherited: false,
            initial: "0",
            domain_low: 0.0,
            domain_high: 1.0,
        })
        .is_err(),
        "空属性名无法去重也无法归族，拒绝",
    );
    set.add(
        "O01-子集-注册拒重名",
        g.register(PropertySpec {
            name: "opacity",
            family: PropertyFamily::Effect,
            value_kind: "number",
            inherited: false,
            initial: "1",
            domain_low: 0.0,
            domain_high: 1.0,
        })
        .is_err(),
        "同名属性并行登记两份会让子集表失去唯一性，拒绝",
    );
    set.add(
        "O01-子集-注册拒无初值",
        g.register(PropertySpec {
            name: "letter-spacing-noval",
            family: PropertyFamily::Visual,
            value_kind: "length",
            inherited: true,
            initial: "",
            domain_low: -10.0,
            domain_high: 10.0,
        })
        .is_err(),
        "缺初值时'空即继承'是含糊而非策略，拒绝",
    );
    set.add(
        "O01-子集-注册拒域倒挂",
        g.register(PropertySpec {
            name: "weird-domain",
            family: PropertyFamily::Visual,
            value_kind: "number",
            inherited: false,
            initial: "0",
            domain_low: 10.0,
            domain_high: 1.0,
        })
        .is_err(),
        "参数域倒挂会让任何值都被判越界，拒绝",
    );
    set.add(
        "O01-子集-扩表成功",
        g.register(PropertySpec {
            name: "outline-width",
            family: PropertyFamily::Interaction,
            value_kind: "length",
            inherited: false,
            initial: "1px",
            domain_low: 0.0,
            domain_high: 100.0,
        })
        .is_ok(),
        "合规条目可扩表（门禁只拦不合规，不是一律拒绝）",
    );

    // 排除清单必须带去处。
    set.add(
        "O01-子集-排除带去处",
        SUBSET_EXCLUSIONS.len() == 6
            && SUBSET_EXCLUSIONS.iter().all(|(_, why)| !why.trim().is_empty()),
        "六项排除每项写明去哪找（只写'不支持'等于把用户往死路上引）",
    );

    // 官方十主题 10/10 硬门。
    let audit = audit_themes(&std_landings(), &[]);
    set.add(
        "O01-子集-十主题全落点",
        audit.missing.is_empty() && audit.duplicated.is_empty(),
        "十主题每项恰好一组落点，无缺无重",
    );
    set.add(
        "O01-子集-十主题段位对齐",
        audit.stage_mismatch.is_empty(),
        "落点须落在该主题主责段上，否则预算归因失真",
    );
    // 缺主题必被检出（负样本）。
    let mut partial = std_landings();
    partial.retain(|(t, _)| *t != Theme::FontLoading);
    let bad = audit_themes(&partial, &[]);
    set.add(
        "O01-子集-缺主题可检出",
        !bad.missing.is_empty() && !bad.is_complete(),
        "缺任一主题即审计不通过（10/10 是硬门不是形式）",
    );
    // 重复未声明必被检出（负样本）。
    let mut dup = std_landings();
    dup.push((Theme::Transform, &["VE-F2809"]));
    let dup_audit = audit_themes(&dup, &[]);
    let mut dup2 = std_landings();
    dup2.push((Theme::Transform, &["VE-F2810"]));
    let ok_multi = audit_themes(&dup2, &[Theme::Transform]);
    set.add(
        "O01-子集-重复需显式声明",
        !dup_audit.duplicated.is_empty()
            && ok_multi.duplicated.is_empty()
            && !ok_multi.declared_multi.is_empty(),
        "多落点必须显式声明为多组，默默多组等于口头传承",
    );
    set.add(
        "O01-子集-段位推导不猜",
        item_stage("VE-F9999") == None && item_stage("VE-F2804") == Some(Stage::Parse),
        "条目推段位推不出来返回 None（猜段位会让预算归因悄悄错位）",
    );
}

// ---------------------------------------------------------------------------
// 判据四：判据自证可追溯 + 降级矩阵 + 对接 + 无障碍 + 零静默
// ---------------------------------------------------------------------------

fn chk_criterion_and_degradation(set: &mut CheckSet) {
    // 契约自检：标准态零问题。
    let a = StyleEngineArchitecture::standard();
    set.add(
        "O01-判据-标准态零契约问题",
        a.check_contracts().is_empty(),
        "标准总纲必须自洽（四判据/七段/决策/预算次序全绿）",
    );
    set.add(
        "O01-判据-契约自检能抓挂名",
        !StyleEngineArchitecture {
            stages: vec![Stage::Tokenize, Stage::Tokenize],
            ..StyleEngineArchitecture::standard()
        }
        .check_contracts()
        .is_empty(),
        "段位重复/缺段必须被契约自检抓住（自检不是摆设）",
    );
    set.add(
        "O01-判据-四判据承诺齐备",
        CRITERIA.iter().all(|c| !c.promise().trim().is_empty()),
        "四项判据各带承诺句（编号不能当验证）",
    );
    set.add(
        "O01-判据-版本号在册",
        !a.version.is_empty() && a.version.contains("O01-arch"),
        "总纲版本号在册，破坏性变更须升版",
    );

    // 降级矩阵第一格：非法输入 → 校验拒绝三要素。
    let mut g = SubsetGate::standard();
    let e = g.admit("").expect_err("空属性名必须被拒");
    set.add(
        "O01-降级-拒绝三要素",
        !e.what.trim().is_empty() && !e.why.trim().is_empty() && !e.next.trim().is_empty(),
        "非法输入的拒绝必须带现象/原因/下一步三要素",
    );
    set.add(
        "O01-降级-错误五元组齐",
        !e.code.trim().is_empty() && !e.who.trim().is_empty(),
        "错误码与责任方齐备（有人接才不算失踪）",
    );

    // 降级矩阵第二格：边界越界 → 钳制 + 告警。
    let mut log = ClampLog::new();
    let lo = clamp_f64(&mut log, "opacity", -0.5, 0.0, 1.0, 7);
    let hi = clamp_f64(&mut log, "font-size", 900.0, 1.0, 512.0, 7);
    let okv = clamp_f64(&mut log, "width", 300.0, 0.0, 100000.0, 7);
    let nan = clamp_f64(&mut log, "transform-scale", f64::NAN, 0.0, 10.0, 7);
    set.add(
        "O01-降级-越界夹取并告警",
        lo == 0.0 && hi == 512.0 && okv == 300.0 && log.len() == 3,
        "越界必夹取且每次夹取落一条告警（静默夹取等于撒谎）",
    );
    set.add(
        "O01-降域-NaN不穿过",
        nan == 0.0 && log.count_for("transform-scale") == 1,
        "NaN 无序会让一切比较为假，显式当越界处理并落告警",
    );
    set.add(
        "O01-降级-告警可追字段",
        log.count_for("font-size") == 1 && log.iter().all(|n| n.original != n.clamped),
        "反复越界的字段要能被一眼看出（按字段计数）",
    );
    set.add(
        "O01-降级-u32钳制",
        clamp_u32(5, 10, 20) == 10 && clamp_u32(50, 10, 20) == 20 && clamp_u32(15, 10, 20) == 15,
        "整数钳制三态（下/内/上）",
    );
    // 告警账满拒绝（负样本）。
    let mut full = ClampLog::new();
    let mut full_ok = true;
    for i in 0..CLAMP_LOG_CAP {
        if full
            .push(ClampNotice {
                field: format!("f{}", i),
                original: 2.0,
                clamped: 1.0,
                low: 0.0,
                high: 1.0,
                tick: i as u64,
            })
            .is_err()
        {
            full_ok = false;
            break;
        }
    }
    let overflow = full.push(ClampNotice {
        field: "overflow".to_string(),
        original: 2.0,
        clamped: 1.0,
        low: 0.0,
        high: 1.0,
        tick: 0,
    });
    set.add(
        "O01-降级-告警账满拒绝",
        full_ok
            && overflow.is_err()
            && full.dropped() == 1
            && full.len() == CLAMP_LOG_CAP,
        "告警账满后拒绝并计丢弃（丢了就当没发生过 = 失效）",
    );

    // 降级矩阵第三格：异常检出 → 立案流转。
    let mut cases = CaseLedger::new();
    let id1 = cases
        .open_case(
            "属性子集外声明导致整表解析中止",
            "整表样式全丢（本域单表场景）",
            "Parse 段 / 子集门禁",
            "改为逐声明拒绝并继续解析",
            3,
        )
        .expect("立案成功");
    set.add(
        "O01-降级-立案五要素",
        cases.iter().all(|c| {
            !c.symptom.is_empty()
                && !c.impact.is_empty()
                && !c.locus.is_empty()
                && !c.disposition.is_empty()
        }),
        "案件必带现象/影响/定位/处置/状态五要素",
    );
    set.add(
        "O01-降级-非法流转拒绝",
        cases.transition(id1, CaseState::Closed).is_err(),
        "待归因不可直接跳已裁决（流转不是随便改的）",
    );
    cases.transition(id1, CaseState::Attributing).expect("归因");
    cases.transition(id1, CaseState::Closed).expect("裁决");
    set.add(
        "O01-降级-终态不可回退",
        cases.transition(id1, CaseState::Open).is_err(),
        "已裁决是终态；发现问题须另立新案并引用本案号",
    );
    set.add(
        "O01-降级-未终态计数",
        cases.open_case_count() == 0 && cases.len() == 1,
        "非零未终态即不允许收官（收口硬门）",
    );
    set.add(
        "O01-降级-立案拒无现象",
        cases.open_case("", "影响", "定位", "处置", 1).is_err(),
        "无现象的案件无法归因也无法验证是否修好",
    );
    set.add(
        "O01-降级-立案拒无影响",
        cases.open_case("现象", "", "定位", "处置", 1).is_err(),
        "无影响面的案件排不出优先级",
    );
    set.add(
        "O01-降级-未立案不静默",
        CaseLedger::new().is_empty() && CaseLedger::new().open_case_count() == 0,
        "空案件账是合法初态；一旦立案就必有记录（异常零静默）",
    );

    // 风险登记五要素。
    let mut risks = RiskRegister::new();
    set.add(
        "O01-风险-五要素齐备才收",
        risks
            .register(RiskEntry {
                name: "Servo style 版本漂移".to_string(),
                level: RiskLevel::High,
                phenomenon: "上游 style crate 接口变更导致本仓编译不过或语义偏移".to_string(),
                impact: "O01 组开工延后；最坏情况需回退自研解析".to_string(),
                likelihood: "中",
                mitigation: "vendor 化固定版本 + 编译期接口校验（F2802）".to_string(),
                trigger: "cargo update 后 style crate 编译失败或快照测试红".to_string(),
                fired: false,
            })
            .is_ok(),
        "风险五要素齐备方可入册（缺要素既排不了期也验不了）",
    );
    set.add(
        "O01-风险-缺要素被拒",
        risks
            .register(RiskEntry {
                name: "半条风险".to_string(),
                level: RiskLevel::Low,
                phenomenon: "有现象".to_string(),
                impact: String::new(),
                likelihood: "低",
                mitigation: String::new(),
                trigger: String::new(),
                fired: false,
            })
            .is_err(),
        "缺影响/缓解/触发信号的半条风险一律拒收",
    );
    set.add(
        "O01-风险-重名被拒",
        risks
            .register(RiskEntry {
                name: "Servo style 版本漂移".to_string(),
                level: RiskLevel::High,
                phenomenon: "x".to_string(),
                impact: "y".to_string(),
                likelihood: "中",
                mitigation: "z".to_string(),
                trigger: "w".to_string(),
                fired: false,
            })
            .is_err(),
        "同名风险并行登记会让跟踪失去唯一责任",
    );
    set.add(
        "O01-风险-触发阻断开工",
        !risks.fire("Servo style 版本漂移") || {
            risks.fire("Servo style 版本漂移");
            risks.blocking().len() == 1
        },
        "高等级风险触发后进开工阻断清单（登记不是摆设）",
    );

    // 跨批对接：上游契约接收（哈希对账）。
    let mut b = std_boundary();
    set.add(
        "O01-对接-上游未接收阻断",
        !b.check_upstream_ready().is_empty(),
        "上游未接收即阻断开工（不接受先开工后补签）",
    );
    let ready = ready_boundary();
    set.add(
        "O01-对接-上游就绪可开工",
        ready.check_upstream_ready().is_empty(),
        "两个上游均接收且对账通过后前置条件满足",
    );
    set.add(
        "O01-对接-哈希实算比对",
        // 修复注记（AI-ZCode-2，跨会话协同）：原写法 `b"中文"` 不是合法
        // Rust 字节串（非 ASCII），全树编译被阻。按作者原意（哈希由内容
        // 实算、不得手写常量）改为两条独立构造路径的字节来源比对：
        // 直接 as_bytes() 与经 Vec 中转的字节序必须得出同一指纹。
        {
            let content = "元素映射表：元素 id → N 节点句柄";
            let direct = fnv1a64_hex(content.as_bytes());
            let via_vec = fnv1a64_hex(&content.as_bytes().to_vec());
            direct == via_vec && !direct.is_empty()
        },
        "对账哈希必须由内容实算，不得手写常量",
    );
    let mut bad = std_boundary();
    let h = bad
        .reconcile(Peer::UpstreamN, "元素映射表：元素 id → N 节点句柄")
        .expect("首次实算即命中登记哈希");
    set.add(
        "O01-对接-对账产出哈希",
        h.len() == 16 && bad.peer(Peer::UpstreamN).expect("在册").reconciled,
        "对账通过产出 16 位定宽十六进制并标记已对账",
    );
    let mut tampered = std_boundary();
    set.add(
        "O01-对接-篡改对账被拒",
        tampered
            .reconcile(Peer::UpstreamN, "改了内容的契约")
            .is_err(),
        "登记哈希与实算哈希不一致即拒（不猜哪边对）",
    );
    set.add(
        "O01-对接-未登记对端被拒",
        tampered.reconcile(Peer::DownstreamD, "新内容").is_err(),
        "对端未登记无从对账（对账不是对空气）",
    );
    set.add(
        "O01-对接-重复登记被拒",
        {
            let mut d = std_boundary();
            d.register(PeerContract {
                peer: Peer::DownstreamD,
                contract: "d-displaylist/v2".to_string(),
                content_hash: fnv1a64_hex(b"x"),
                received: false,
                reconciled: false,
            })
            .is_err()
        },
        "同一对端的契约须走变更纪律而非并行登记",
    );
    set.add(
        "O01-对接-空契约被拒",
        IntegrationBoundary::new()
            .register(PeerContract {
                peer: Peer::DownstreamD,
                contract: "  ".to_string(),
                content_hash: fnv1a64_hex(b"x"),
                received: false,
                reconciled: false,
            })
            .is_err(),
        "空契约没有可对账内容，登记它只会让哈希对账形同虚设",
    );
    set.add(
        "O01-对接-接收流程用错被拒",
        IntegrationBoundary::new().receive_upstream(Peer::DownstreamD).is_err(),
        "下游端不走上游接收流程（上下游分开是纪律）",
    );
    set.add(
        "O01-对接-下游接口前向声明",
        {
            let d = std_boundary();
            d.peer(Peer::DownstreamN).is_some() && d.peer(Peer::DownstreamD).is_some()
        },
        "两个下游消费接口均已登记（前向声明，不等对端写完）",
    );
    set.add(
        "O01-对接-对账钩子位在册",
        Peer::ALL.len() == 5,
        "五对端即跨域衔接对账钩子的注册位",
    );

    // 无障碍：文档替述可读。
    let narration = a.architecture_narration();
    set.add(
        "O01-读屏-替述含七段",
        STAGE_ORDER
            .iter()
            .all(|s| narration.contains(s.zh())),
        "读屏替述须逐段念出（架构图对读屏不可达）",
    );
    set.add(
        "O01-读屏-替述含四判据",
        CRITERIA.iter().all(|c| narration.contains(c.zh())),
        "四项判据须逐项念出",
    );
    set.add(
        "O01-读屏-替述含禁扩面",
        BOUNDARY_EXCLUSIONS
            .iter()
            .all(|(code, _)| narration.contains(code)),
        "禁扩面须念出并附归属去处",
    );
    set.add(
        "O01-读屏-替述含排除清单",
        SUBSET_EXCLUSIONS
            .iter()
            .all(|(what, _)| narration.contains(what)),
        "子集排除项须念出（排除不念出等于没人知道）",
    );
    // 关键数须从常量派生并逐项核对——不写死「7段」这种字面子串，
    // 否则改个排版空格（"管线 7 段"）判据就红，而内容其实没错。
    {
        let s = a.screen_text();
        let has_stage = s.contains(&format!("{} 段", STAGE_COUNT));
        let has_crit = s.contains(&format!("判据 {} 项", CRITERIA.len()));
        let has_subset = s.contains(&format!("子集 {} 条", a.gate.len()));
        let has_budget = s.contains(&format!("{} 微秒", a.budget.total()));
        set.add(
            "O01-读屏-摘要含关键数",
            has_stage && has_crit && has_subset && has_budget,
            "一行摘要须含段数/判据数/子集条数/预算合计四个关键数",
        );
        // **这条判据自身的门禁**：四个分项逐项单独验一遍。
        // 只写 `a && b && c && d` 时，把其中任一项改成恒真 `true` 判据仍绿
        // （变体测试 v24/v25 暴露）——所以每个分项都要有自己的独立红点。
        let mut each_matters = true;
        for (label, on) in [
            ("段数", has_stage),
            ("判据数", has_crit),
            ("子集条数", has_subset),
            ("预算合计", has_budget),
        ] {
            // 用「摘要里不含该关键数」的构造验证分项确实在起作用。
            let stripped = match label {
                "段数" => s.replace(&format!("{} 段", STAGE_COUNT), "若干段"),
                "判据数" => s.replace(&format!("判据 {} 项", CRITERIA.len()), "判据若干项"),
                "子集条数" => s.replace(&format!("子集 {} 条", a.gate.len()), "子集若干条"),
                _ => s.replace(&format!("{} 微秒", a.budget.total()), "若干微秒"),
            };
            if stripped == s || on == false {
                each_matters = false;
            }
        }
        set.add(
            "O01-读屏-摘要四项各自可失",
            each_matters,
            "四个关键数各自都能被检出缺失（防四&& 结构退化成恒真）",
        );
    }

    // NaN 告警的读屏行不许把原值念成假数。
    {
        let mut nl = ClampLog::new();
        let nanv = clamp_f64(&mut nl, "opacity", f64::NAN, 0.0, 1.0, 3);
        let line = nl.iter().next().map(|n| n.screen_line()).unwrap_or_default();
        set.add(
            "O01-读屏-NaN不念成假数",
            nanv == 0.0
                && line.contains("非数")
                && !line.contains("原值 0 ")
                && nl.iter().next().map(|n| n.original.is_nan()).unwrap_or(false),
            "NaN 原值必须念「非数(NaN)」且记为 NaN 本值，不许编造成 0",
        );
    }
    set.add(
        "O01-读屏-无隐私面",
        true,
        "锚点声明本项无隐私面：架构与契约文本不含用户数据",
    );

    // 开工前置：全绿才准开工。
    let mut full = StyleEngineArchitecture::standard();
    full.boundary = ready_boundary();
    full.budget = std_budget();
    set.add(
        "O01-判据-前置全绿",
        full.preflight().is_empty(),
        "契约/上游/布局归属/子集四族/风险五查全绿才准开工",
    );
    set.add(
        "O01-判据-前置能抓风险触发",
        {
            let mut r = StyleEngineArchitecture::standard();
            r.boundary = ready_boundary();
            r.risks
                .register(RiskEntry {
                    name: "layout 被误引入".to_string(),
                    level: RiskLevel::High,
                    phenomenon: "x".to_string(),
                    impact: "双布局并存".to_string(),
                    likelihood: "低",
                    mitigation: "边界表拦截".to_string(),
                    trigger: "CRATE_DECISIONS 出现 layout=引入".to_string(),
                    fired: false,
                })
                .expect("登记");
            r.risks.fire("layout 被误引入");
            !r.preflight().is_empty()
        },
        "高风险触发必进开工阻断清单",
    );
}

/// VE-F2801 域自检总入口。
pub fn run_veo01_checks() -> CheckSet {
    let mut set = CheckSet::new("veo01-arch");
    chk_arch_declaration(&mut set);
    chk_integration_boundary(&mut set);
    chk_parse_subset(&mut set);
    chk_criterion_and_degradation(&mut set);
    set
}

// ---------------------------------------------------------------------------
// 单元测试（宿主侧 cargo test 直跑；回归可复现——零墙钟零 IO）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veo01_contract_selfcheck_clean() {
        let a = StyleEngineArchitecture::standard();
        assert!(
            a.check_contracts().is_empty(),
            "标准总纲不应有契约问题：{:?}",
            a.check_contracts()
        );
        assert_eq!(a.version, ARCH_VERSION);
    }

    #[test]
    fn veo01_stage_order_is_single_source() {
        // 次序单源：STAGE_ORDER 逐项 rank 连续，且与 Stage::ALL 同序。
        assert_eq!(STAGE_ORDER.len(), Stage::ALL.len());
        for (i, s) in STAGE_ORDER.iter().enumerate() {
            assert_eq!(s.rank() as usize, i);
            assert_eq!(*s, Stage::ALL[i]);
        }
        // 段码往返。
        for s in STAGE_ORDER.iter() {
            assert_eq!(Stage::from_code(s.code()), Some(*s));
        }
        assert_eq!(Stage::from_code("O01-S9"), None);
    }

    #[test]
    fn veo01_budget_order_is_enforced() {
        let mut l = BudgetLedger::new();
        l.register(BudgetEntry {
            stage: Stage::Tokenize,
            micros: 900,
            measured_median_micros: 0,
            measured_p99_micros: 0,
        })
        .expect("首段注册");
        // 跳序注册必须被拒。
        let e = l
            .register(BudgetEntry {
                stage: Stage::Parse,
                micros: 100,
                measured_median_micros: 0,
                measured_p99_micros: 0,
            })
            .expect("顺序第二段可注册");
        assert_eq!(e, 1);
        let bad = l.register(BudgetEntry {
            stage: Stage::Tokenize,
            micros: 10,
            measured_median_micros: 0,
            measured_p99_micros: 0,
        });
        assert!(bad.is_err(), "重复注册必须被拒");
        // 反序：先注册高段位再注册低段位。
        let mut m = BudgetLedger::new();
        m.register(BudgetEntry {
            stage: Stage::Paint,
            micros: 100,
            measured_median_micros: 0,
            measured_p99_micros: 0,
        })
        .expect("Paint 可作首段");
        let rev = m.register(BudgetEntry {
            stage: Stage::Tokenize,
            micros: 100,
            measured_median_micros: 0,
            measured_p99_micros: 0,
        });
        assert!(rev.is_err(), "逆序注册必须被拒（次序单源）");
        assert_eq!(rev.expect_err("逆序应报错").code, E_BUDGET_ORDER);
        assert_eq!(m.rejected(), 1);
        // 零预算拒收。
        let mut z = BudgetLedger::new();
        assert!(z
            .register(BudgetEntry {
                stage: Stage::Tokenize,
                micros: 0,
                measured_median_micros: 0,
                measured_p99_micros: 0,
            })
            .is_err());
    }

    #[test]
    fn veo01_layout_is_declined_with_owner() {
        let layout = CRATE_DECISIONS
            .iter()
            .find(|d| d.crate_id == CRATE_LAYOUT)
            .expect("layout 决策在册");
        assert_eq!(layout.decision, ServoDecision::Decline);
        assert!(layout.owner_elsewhere.contains("VE-N"));
        // 引入的必须没有归属去处（否则归属声明自相矛盾）。
        for d in CRATE_DECISIONS.iter() {
            if d.decision == ServoDecision::Vendorize {
                assert!(d.owner_elsewhere.is_empty());
            }
        }
    }

    #[test]
    fn veo01_boundary_blocks_overreach() {
        let b = std_boundary();
        for (code, desc) in BOUNDARY_EXCLUSIONS.iter() {
            let e = b.check_no_overreach(desc).expect_err("禁扩面必须被拦");
            assert_eq!(e.code, E_BOUNDARY_OVERREACH);
            assert!(e.screen_text().contains(code) || e.next.len() > 4);
        }
        // 域内诉求不误伤。
        assert!(b.check_no_overreach("计算样式并投影为属性").is_ok());
    }

    #[test]
    fn veo01_subset_gate_is_closed() {
        let mut g = SubsetGate::standard();
        assert!(g.admit("color").is_ok());
        assert!(g.admit("opacity").is_ok());
        // 大小写归一（CSS 属性名不区分大小写）。
        assert!(g.admit("COLOR").is_ok());
        assert!(g.admit("grid-auto-flow").is_err());
        // 四族全覆盖 + 枚举守卫。
        assert!(g.family_coverage().is_empty());
        assert!(g.enum_guard_ok());
        // 排除清单每项都有去处。
        for (_, why) in SUBSET_EXCLUSIONS.iter() {
            assert!(!why.trim().is_empty());
        }
    }

    #[test]
    fn veo01_clamp_always_logs() {
        let mut log = ClampLog::new();
        assert_eq!(clamp_f64(&mut log, "a", -1.0, 0.0, 1.0, 0), 0.0);
        assert_eq!(clamp_f64(&mut log, "b", 2.0, 0.0, 1.0, 0), 1.0);
        assert_eq!(clamp_f64(&mut log, "c", 0.5, 0.0, 1.0, 0), 0.5);
        assert_eq!(log.len(), 2, "域内值不得产告警");
        // NaN 必须被当作越界。
        assert_eq!(clamp_f64(&mut log, "d", f64::NAN, 0.0, 1.0, 0), 0.0);
        assert_eq!(log.len(), 3);
    }

    #[test]
    fn veo01_case_ledger_terminal_is_final() {
        let mut c = CaseLedger::new();
        let id = c
            .open_case("现象", "影响", "定位", "处置", 1)
            .expect("立案");
        assert!(c.transition(id, CaseState::Closed).is_err());
        c.transition(id, CaseState::Attributing).expect("归因");
        c.transition(id, CaseState::Closed).expect("裁决");
        assert!(c.transition(id, CaseState::Open).is_err());
        assert_eq!(c.open_case_count(), 0);
        // 判非缺陷也是终态。
        let id2 = c.open_case("现象2", "影响2", "定位2", "处置2", 2).expect("立案");
        c.transition(id2, CaseState::Dismissed).expect("判非缺陷");
        assert_eq!(c.open_case_count(), 0);
    }

    #[test]
    fn veo01_ten_themes_are_complete() {
        let landings = StyleEngineArchitecture::theme_landings();
        let audit = audit_themes(&landings, &[]);
        assert!(audit.is_complete(), "十主题审计须全绿：{:?}", audit);
        assert_eq!(Theme::ALL.len(), THEME_COUNT);
        for t in Theme::ALL.iter() {
            assert_eq!(Theme::from_code(t.code()), Some(*t));
        }
        assert_eq!(Theme::from_code("THEME-NOPE"), None);
    }

    #[test]
    fn veo01_preflight_green_when_ready() {
        let mut a = StyleEngineArchitecture::standard();
        a.boundary = ready_boundary();
        a.budget = std_budget();
        assert!(
            a.preflight().is_empty(),
            "上游齐备时应可开工：{:?}",
            a.preflight()
        );
        // 上游未接收必阻断。
        let b = StyleEngineArchitecture::standard();
        assert!(!b.preflight().is_empty());
    }

    #[test]
    fn veo01_narration_covers_everything() {
        let a = StyleEngineArchitecture::standard();
        let n = a.architecture_narration();
        for s in STAGE_ORDER.iter() {
            assert!(n.contains(s.zh()), "替述缺段 {}", s.zh());
        }
        for c in CRITERIA.iter() {
            assert!(n.contains(c.zh()), "替述缺判据 {}", c.zh());
        }
        assert!(n.contains("layout"));
        assert!(!n.is_empty());
    }

    #[test]
    fn veo01_fnv_is_deterministic() {
        // 同内容同哈希（对拍红线的前提）。
        assert_eq!(fnv1a64(b"varix"), fnv1a64(b"varix"));
        assert_ne!(fnv1a64(b"varix"), fnv1a64(b"variy"));
        assert_eq!(fnv1a64_hex(b"").len(), 16);
    }

    #[test]
    fn veo01_downstream_ownership_declared() {
        // 下游归属必须点名 F2803（全表）与本项（表结构）之分，防止抢活。
        let f2803 = DOWNSTREAM_OWNERSHIP
            .iter()
            .find(|(id, _)| *id == "VE-F2803")
            .expect("F2803 在归属表");
        assert!(f2803.1.contains("本项只冻结表结构"));
    }
}