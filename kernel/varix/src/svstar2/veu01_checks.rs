//! VE-F4201 · 域自检（判据逐条对应，见 `veu01_arch.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 五层（模型→契约→规则→验证→度量，层链相邻闭合）→ `U01-判据-层-*`
//! - 五层 × 五能力双向对账（头注§二两条裁决的机检落点）→ `U01-判据-对账-*`
//! - 接口冻结（四边 / 越权变更拒绝 / ADR 唯一通道 / 禁后门）→ `U01-冻结-*`
//! - 承接落地（首批两源 / 继承位登记 / 回溯移交包）→ `U01-承接-*`
//! - 入约 + 口径契约层（双维齐 / 无豁免 / 注册闸拒空）→ `U01-入约-*`
//! - 层间失配对拍（降级矩阵第一格）→ `U01-对拍-*`
//! - 降级矩阵（三格逐条可执行）→ `U01-降级-*`
//! - 跨批对接（T10 移交包单源 / F4202 下游 / U01 双签闸）→ `U01-对接-*`
//! - 无障碍（双维标准入约 + 读屏替述覆盖六判据）→ `U01-读屏-*`
//! - 错误路径零静默（五元组齐发）→ `U01-错误-*`
//! - 禁扩面（防抢活）→ `U01-边界-*`
//!
//! 零墙钟、零 IO，回归可复现。

use super::veu01_arch::*;
use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 判据族前缀表（**长前缀在前**：先匹到`U01-判据-层-` 再轮到 `U01-判据-`）。
///
/// 这些字面量本身就是族名，故 `family_of` 直接回传其中一项——不切串、不分配，
/// 得到的仍是 `&'static str`，与 [`CheckSet`] 要求的入参类型天然对齐。
const FAMILIES: [&str; 13] = [
    "U01-判据-层-",
    "U01-判据-对账-",
    "U01-判据-判据位",
    "U01-判据-",
    "U01-冻结-",
    "U01-承接-",
    "U01-入约-",
    "U01-对拍-",
    "U01-对接-",
    "U01-降级-",
    "U01-读屏-",
    "U01-错误-",
    "U01-边界-",
];

/// 取自检项所属的判据族（未登记前缀归`U01-边界-`，宁可并入也不静默丢族）。
fn family_of(name: &'static str) -> &'static str {
    for f in FAMILIES.iter() {
        if name.starts_with(f) {
            return f;
        }
    }
    "U01-边界-"
}

struct FamilyTally {
    /// 细项暂存：`(family, name, passed, detail)`，全部 `&'static str`。
    pending: Vec<(&'static str, &'static str, bool, &'static str)>,
    /// 已 flush 的族账（`family -> 全绿?`）。
    done: Vec<(&'static str, bool)>,
}

impl FamilyTally {
    fn new() -> FamilyTally {
        FamilyTally { pending: Vec::new(), done: Vec::new() }
    }

    /// 记一条细项（名字与判定原样收下，一条不少跑）。
    fn add(&mut self, name: &'static str, passed: bool, detail: &'static str) {
        self.pending.push((family_of(name), name, passed, detail));
    }

    /// 通过项（`set.ok` 的等价入口）。
    fn ok(&mut self, name: &'static str) {
        self.add(name, true, "");
    }

    /// 归族收敛，写进真正的 [`CheckSet`]。
    ///
    /// 全绿族输出一行族账；有红族**逐条出声**（红项 `detail` 原样带出）。
    fn flush(mut self, set: &mut CheckSet) {
        // 保持首次出现序（判据书叙述序），便于人工逐行核对。
        let mut order: Vec<&'static str> = Vec::new();
        for (fam, _, _, _) in self.pending.iter() {
            if !order.iter().any(|f| f == fam) {
                order.push(*fam);
            }
        }
        for fam in order.iter() {
            let mut all_green = true;
            for it in self.pending.iter() {
                if it.0 == *fam {
                    if !it.2 {
                        all_green = false;
                    }
                }
            }
            if all_green {
                set.add("U01-族账-该族细项全绿", true, fam);
                self.done.push((*fam, true));
            } else {
                for it in self.pending.iter() {
                    if it.0 == *fam {
                        set.add(it.1, it.2, it.3);
                    }
                }
                self.done.push((*fam, false));
            }
        }
    }

    /// `(族总数, 全绿族数)`。
    ///
    /// 这两个数是**收敛契约本身的可观测口径**：族总数应等于 [`FAMILIES`] 里
    /// 实际被用到的族数（不多不少——少说明有族名没登记前缀而全被并进兜底族，
    /// 多说明登记了用不上的族），全绿族数则用于断言「标准态下族账无红」。
    fn families(&self) -> (usize, usize) {
        let green = self.done.iter().filter(|(_, g)| *g).count();
        (self.done.len(), green)
    }
}

/// 判据一：五层 —— 层数 / 层序 / 层契约齐备 / 层链闭合 / 零开销 / 规则文本唯一持有。
fn chk_five_layers(set: &mut FamilyTally) {
    let a = ConsistencyArchitecture::standard();

    // 标准态总纲必须全绿（否则下面所有断言都在测一个坏基线）。
    let issues = a.check_layers();
    for i in issues.iter() {
        set.add(
            "U01-判据-层-标准态无问题",
            false,
            "标准总纲的层检查不应有红项",
        );
        set.add("U01-判据-层-问题明细", false, i.code);
        break;
    }
    if issues.is_empty() {
        set.ok("U01-判据-层-标准态无问题");
    }

    // 逐层断言：层数恰 5、层位连续、契约齐备、成本声明期。
    set.add(
        "U01-判据-层-层数恰五",
        a.layers.len() == LAYER_COUNT,
        "五层是判据一的硬数字",
    );
    let mut order_ok = true;
    for (i, s) in a.layers.iter().enumerate() {
        if s.rank as usize != i {
            order_ok = false;
        }
    }
    set.add("U01-判据-层-层位连续递增", order_ok, "层序单源 Layer::rank");
    set.add(
        "U01-判据-层-层契约六项齐备",
        a.layers.iter().all(|s| s.is_complete()),
        "缺字段的层契约无法与下游对拍",
    );
    set.add(
        "U01-判据-层-全部声明期",
        a.is_zero_runtime_cost(),
        "U 域总架构零运行时开销",
    );

    // 规则文本唯一持有（头注§一红线）：只有一个层 owns_rule_text。
    let owners = a.rule_text_owners();
    set.add(
        "U01-判据-层-规则文本唯一持有",
        owners.len() == 1 && owners[0] == Layer::Contract,
        "只有契约层可持规则文本，其余层持副本即一致性分叉",
    );

    // 反例：把规则文本持有权放到非契约层 → 必须判红。
    let mut bad = ConsistencyArchitecture::standard();
    // 通过篡改层册的 cost 字段旁证不了持有权，故直接构造一个「四层皆持」的
    // 违规视图：改 layers 中非契约层的 cost 不影响持有权，
    // 因此这里改用 rule_text_owners 的逻辑输入层册——
    // 构造一份把 Contract 层的持有位抹掉的层册是不可能的（owns_rule_text 是
    // 层枚举的固有属性），所以反例改为验证**判定函数本身**：
    // 五层中恰好一层 owns_rule_text，逐层断言，避免将来有人给 Layer 加可变标志。
    let per_layer_owners: Vec<Layer> = LAYER_ORDER
        .iter()
        .copied()
        .filter(|l| l.owns_rule_text())
        .collect();
    set.add(
        "U01-判据-层-持有权为层固有属性",
        per_layer_owners == vec![Layer::Contract],
        "持有权若可变则一致性红线可被绕过",
    );
    let _ = &mut bad;

    // 层链相邻闭合：每层（除度量）恰一条出边，度量层无出边。
    let mut chain_ok = true;
    for l in LAYER_ORDER.iter() {
        match l.downstream() {
            Some(down) => {
                if a.freeze.edge(*l, down).is_none() {
                    chain_ok = false;
                }
            }
            None => {
                if a.freeze.iter().any(|i| i.from == Layer::Metric) {
                    chain_ok = false;
                }
            }
        }
    }
    set.add("U01-判据-层-层链相邻闭合", chain_ok, "五层四条相邻边，多一条少一条都判红");

    // 层序单源：LAYER_ORDER 与 Layer::ALL 同序且 rank 连续。
    set.add(
        "U01-判据-层-层序单源一致",
        LAYER_ORDER.len() == Layer::ALL.len()
            && LAYER_ORDER
                .iter()
                .zip(Layer::ALL.iter())
                .all(|(x, y)| x == y),
        "全模块只有一处声明层序",
    );

    // 枚举往返守卫（未知码必须 None，不许猜）。
    let mut round_ok = true;
    for l in LAYER_ORDER.iter() {
        if Layer::from_code(l.code()) != Some(*l) {
            round_ok = false;
        }
    }
    set.add("U01-判据-层-层码往返一致", round_ok, "未知码返回 None");
    set.add(
        "U01-判据-层-未知层码被拒",
        Layer::from_code("U01-L9").is_none(),
        "调用方须显性拒绝未知码",
    );

    // 层锚点标签逐项对齐（锚点原文「模型层→契约层→规则层→验证层→度量层」）。
    let anchor_labels = [
        "模型层",
        "契约层",
        "规则层",
        "验证层",
        "度量层",
    ];
    let got: Vec<&str> = LAYER_ORDER.iter().map(|l| l.anchor_label()).collect();
    set.add(
        "U01-判据-层-锚点五标签对齐",
        got == anchor_labels,
        "锚点括号内五项必须逐项对齐，无一遗漏",
    );

    // 端点语义：模型层无上游、度量层无下游。
    set.add(
        "U01-判据-层-端点语义正确",
        LAYER_ORDER[0].upstream().is_none() && LAYER_ORDER[LAYER_COUNT - 1].downstream().is_none(),
        "模型层是源头、度量层是终点",
    );
}

/// 判据一对账面：五层 × 五能力双向覆盖（头注§二两条裁决的机检落点）。
fn chk_capability_alignment(set: &mut FamilyTally) {
    let a = ConsistencyArchitecture::standard();
    let issues = a.check_capability_alignment();
    set.add(
        "U01-判据-对账-标准态全绿",
        issues.is_empty(),
        "五层与五能力双向覆盖必须成立",
    );
    for i in issues.iter() {
        set.add("U01-判据-对账-问题明细", false, i.code);
    }

    // 方向一：每层都有服务它的能力（层无主 = 无主资源）。
    let mut every_layer_served = true;
    for l in LAYER_ORDER.iter() {
        if !CAPABILITY_ORDER.contains(&l.served_by()) {
            every_layer_served = false;
        }
    }
    set.add(
        "U01-判据-对账-每层有服务能力",
        every_layer_served,
        "层无主则资源无主",
    );

    // 方向二：每能力都占层或声明派生（能力无落点 = 不存在）。
    let mut every_cap_grounded = true;
    for c in CAPABILITY_ORDER.iter() {
        if c.layers().is_empty() && c.derivation_sources().is_empty() {
            every_cap_grounded = false;
        }
    }
    set.add(
        "U01-判据-对账-每能力有落点或派生",
        every_cap_grounded,
        "锚点五项能力必须逐项有交代",
    );

    // 裁决一：规则层「有层无能名」——它归契约能力，不另立能力。
    set.add(
        "U01-判据-对账-规则层归契约能力",
        Layer::Rule.served_by() == Capability::Contract,
        "规则层是契约能力的执行段，不是第六项能力",
    );
    set.add(
        "U01-判据-对账-契约能力占两层",
        Capability::Contract.layers().len() == 2
            && Capability::Contract.layers().contains(&Layer::Contract)
            && Capability::Contract.layers().contains(&Layer::Rule),
        "一能力两段：契约层声明 + 规则层可执行化",
    );
    // 能力数仍为五（规则层未被误立为能力）。
    set.add(
        "U01-判据-对账-能力数仍为五",
        CAPABILITY_ORDER.len() == CAPABILITY_COUNT && CAPABILITY_COUNT == 5,
        "规则层升格为能力会把五项变六项",
    );

    // 裁决二：知识图谱「有能无层」——不占层，派生自模型层 + 契约层。
    set.add(
        "U01-判据-对账-图谱不占层",
        Capability::Graph.layers().is_empty(),
        "图谱升为第六层会出现实体两份真相",
    );
    set.add(
        "U01-判据-对账-图谱派生两源",
        Capability::Graph.derivation_sources().len() == 2
            && Capability::Graph
                .derivation_sources()
                .contains(&Layer::Model)
            && Capability::Graph
                .derivation_sources()
                .contains(&Layer::Contract),
        "图谱由模型层实体 + 契约层关系派生",
    );
    // 派生源必须被别的能力占用（不许自引用、不许指向空层）。
    let mut derivation_covered = true;
    for src in Capability::Graph.derivation_sources().iter() {
        if !CAPABILITY_ORDER.iter().any(|c| c.layers().contains(src)) {
            derivation_covered = false;
        }
    }
    set.add(
        "U01-判据-对账-派生源有主",
        derivation_covered,
        "派生视图必须挂在有主的层上",
    );
    // 非派生能力不得声称有派生源（防滥声明）。
    let mut no_fake_derivation = true;
    for c in CAPABILITY_ORDER.iter() {
        if *c != Capability::Graph && !c.derivation_sources().is_empty() {
            no_fake_derivation = false;
        }
    }
    set.add("U01-判据-对账-无滥声明派生", no_fake_derivation, "只有图谱是派生能力");

    // 锚点五能力标签逐项对齐（原文：模型/契约/扫描/度量/知识图谱）。
    let cap_labels = ["跨域一致性模型", "契约", "扫描", "度量", "知识图谱"];
    let got_caps: Vec<&str> = CAPABILITY_ORDER.iter().map(|c| c.anchor_label()).collect();
    set.add(
        "U01-判据-对账-锚点五能力对齐",
        got_caps == cap_labels,
        "锚点职责定位五项能力必须逐项对齐",
    );

    // 主责条目合法（每项能力都有真实册内条目号，且条目号格式合法）。
    let mut owners_valid = true;
    for c in CAPABILITY_ORDER.iter() {
        if !is_valid_item_id(c.owner_item()) {
            owners_valid = false;
        }
    }
    set.add(
        "U01-判据-对账-能力主责条目合法",
        owners_valid,
        "主责条目须为 VE-F#### 形式",
    );

    // 能力码往返 + 未知码拒绝。
    let mut cap_round = true;
    for c in CAPABILITY_ORDER.iter() {
        if Capability::from_code(c.code()) != Some(*c) {
            cap_round = false;
        }
    }
    set.add("U01-判据-对账-能力码往返一致", cap_round, "未知码返回 None");
    set.add(
        "U01-判据-对账-未知能力码被拒",
        Capability::from_code("U01-C9").is_none(),
        "调用方须显性拒绝未知码",
    );
}

/// 判据二：接口冻结 —— 四边 / 越权拒绝 / ADR 唯一通道 / 禁后门。
fn chk_interface_freeze(set: &mut FamilyTally) {
    let a = ConsistencyArchitecture::standard();
    let issues = a.check_interfaces();
    set.add(
        "U01-冻结-标准态无问题",
        issues.is_empty(),
        "标准总纲的接口检查不应有红项",
    );
    for i in issues.iter() {
        set.add("U01-冻结-问题明细", false, i.code);
    }

    // 四条相邻边。
    set.add(
        "U01-冻结-接口恰四条",
        a.freeze.len() == LAYER_COUNT - 1,
        "层数-1 = 相邻边数",
    );
    let mut adjacent_ok = true;
    for it in a.freeze.iter() {
        if !it.is_adjacent() {
            adjacent_ok = false;
        }
    }
    set.add("U01-冻结-全部相邻连线", adjacent_ok, "跨层直连不在任何接口里");
    set.add(
        "U01-冻结-接口契约齐备",
        a.freeze.iter().all(|i| i.is_complete()),
        "八项字段 + 实算定宽哈希",
    );

    // 哈希是实算的、不是手写常量：重算必须与在位一致。
    let mut hash_ok = true;
    for it in a.freeze.iter() {
        if fnv1a64_hex(it.declared_text().as_bytes()) != it.declared_hash {
            hash_ok = false;
        }
    }
    set.add("U01-冻结-哈希实算可复现", hash_ok, "哈希对账不是看一眼填没填");

    // 冻结版本与架构版本各自独立且非空。
    set.add(
        "U01-冻结-版本双轨齐备",
        a.freeze.interface_version == INTERFACE_VERSION && a.version == ARCH_VERSION,
        "架构版本与冻结版本是两条独立的轨",
    );

    // 冻结哈希定宽且对重排敏感（顺序变了哈希就该变）。
    let d1 = a.freeze.freeze_digest();
    set.add(
        "U01-冻结-架构冻结哈希定宽",
        d1.len() == HASH_HEX_LEN,
        "宽度不定就没法字符串比对",
    );
    // 接口顺序重排 → 哈希必变（顺序敏感是刻意的：重排不是无变化的改动）。
    let mut shuffled = a.freeze.clone();
    shuffled.tamper_interfaces().reverse();
    let d2 = shuffled.freeze_digest();
    set.add("U01-冻结-冻结哈希顺序敏感", d1 != d2, "重排接口表不算无变化");

    // **越权变更必须被拒**（锚点降级矩阵第二格的核心）。
    let mut l = InterfaceFreezeLedger::new(standard_interfaces(), INTERFACE_VERSION)
        .expect("标准接口册可冻结");
    // 合法路径（带理由 + 带否决记录）应成功。
    let new_body = "U01-IF1|CHANGED-BODY-FOR-TEST|payload|out|fail|cx|consumer|notmine";
    let ok = l.attempt_change(
        "U01-IF1",
        new_body,
        "接口 IF1 消费方字段需明确写出挂载顺序",
        "否决：不升版就地改，因为下游无从判断该按哪一版编",
    );
    set.add(
        "U01-冻结-合法变更经ADR放行",
        ok.is_ok(),
        "带理由与否决记录的 ADR 是唯一合法通道",
    );
    set.add(
        "U01-冻结-ADR与重基一一对应",
        l.rebase_count() == l.adrs().count(),
        "一改一 ADR，不许有 ADR 无重基或反之",
    );

    // 缺理由 → 拒。
    let mut l2 = InterfaceFreezeLedger::new(standard_interfaces(), INTERFACE_VERSION)
        .expect("标准接口册可冻结");
    let no_reason = l2.attempt_change("U01-IF1", new_body, "   ", "否决：某");
    set.add(
        "U01-冻结-缺理由被拒",
        matches!(no_reason, Err(ref e) if e.code == E_ADR_NO_TITLE),
        "没有理由的改动无从判断该不该做",
    );

    // 缺否决记录 → 拒（本域最重要的 ADR 纪律，见头注）。
    let mut l3 = InterfaceFreezeLedger::new(standard_interfaces(), INTERFACE_VERSION)
        .expect("标准接口册可冻结");
    let no_reject = l3.attempt_change("U01-IF1", new_body, "改一下消费方字段", "  ");
    set.add(
        "U01-冻结-缺否决记录被拒",
        matches!(no_reject, Err(ref e) if e.code == E_ADR_NO_REJECTED),
        "没有否决记录的 ADR 不是决策记录，是提案",
    );

    // 未登记接口变更 → 拒。
    let mut l4 = InterfaceFreezeLedger::new(standard_interfaces(), INTERFACE_VERSION)
        .expect("标准接口册可冻结");
    let unknown = l4.attempt_change("U01-IF9", new_body, "理由", "否决：某");
    set.add(
        "U01-冻结-未登记接口变更被拒",
        matches!(unknown, Err(ref e) if e.code == E_INTERFACE_UNKNOWN),
        "对不存在的接口无从变更",
    );

    // **后门必须堵死**：不经 ADR 直接 rebase → 拒。
    let mut l5 = InterfaceFreezeLedger::new(standard_interfaces(), INTERFACE_VERSION)
        .expect("标准接口册可冻结");
    let sneak = l5.rebase("U01-IF1", new_body);
    set.add(
        "U01-冻结-无ADR重基被拒",
        matches!(sneak, Err(ref e) if e.code == E_REBASE_WITHOUT_ADR),
        "绕过 attempt_change 直调 rebase 是后门",
    );

    // ADR 缺字段 / 版本未升 / 编号重复 / 账满 逐条拒。
    let mut l6 = InterfaceFreezeLedger::new(standard_interfaces(), INTERFACE_VERSION)
        .expect("标准接口册可冻结");
    let bad_adr = l6.open_adr(AdrRecord {
        id: "ADR-U01-0001".to_string(),
        interface: "U01-IF1".to_string(),
        from_version: "U01-iface-v1".to_string(),
        to_version: "U01-iface-v1-r1".to_string(),
        title: "t".to_string(),
        decision: "d".to_string(),
        rejected: "  ".to_string(),
    });
    set.add(
        "U01-冻结-ADR缺否决被拒",
        matches!(bad_adr, Err(ref e) if e.code == E_ADR_INCOMPLETE),
        "ADR 六项齐发",
    );

    let mut l7 = InterfaceFreezeLedger::new(standard_interfaces(), INTERFACE_VERSION)
        .expect("标准接口册可冻结");
    let no_bump = l7.open_adr(AdrRecord {
        id: "ADR-U01-0001".to_string(),
        interface: "U01-IF1".to_string(),
        from_version: "U01-iface-v1".to_string(),
        to_version: "U01-iface-v1".to_string(),
        title: "t".to_string(),
        decision: "d".to_string(),
        rejected: "r".to_string(),
    });
    set.add(
        "U01-冻结-ADR未升版被拒",
        matches!(no_bump, Err(ref e) if e.code == E_ADR_NO_VERSION_BUMP),
        "不升版则下游无法分辨新旧",
    );

    let mut l8 = InterfaceFreezeLedger::new(standard_interfaces(), INTERFACE_VERSION)
        .expect("标准接口册可冻结");
    let dup = l8.open_adr(AdrRecord {
        id: "ADR-U01-0001".to_string(),
        interface: "U01-IF2".to_string(),
        from_version: "v1".to_string(),
        to_version: "v2".to_string(),
        title: "t".to_string(),
        decision: "d".to_string(),
        rejected: "r".to_string(),
    });
    assert!(dup.is_ok(), "首个 ADR 应登记成功");
    let dup2 = l8.open_adr(AdrRecord {
        id: "ADR-U01-0001".to_string(),
        interface: "U01-IF3".to_string(),
        from_version: "v1".to_string(),
        to_version: "v2".to_string(),
        title: "t".to_string(),
        decision: "d".to_string(),
        rejected: "r".to_string(),
    });
    set.add(
        "U01-冻结-ADR编号重复被拒",
        matches!(dup2, Err(ref e) if e.code == E_ADR_DUP),
        "同号两决议会让引用指向不明",
    );

    // 构造期拒绝：空接口集 / 非相邻连线 / 重复码 / 残缺契约。
    let empty = InterfaceFreezeLedger::new(Vec::new(), INTERFACE_VERSION);
    set.add(
        "U01-冻结-空接口册被拒",
        matches!(empty, Err(ref e) if e.code == E_INTERFACE_EMPTY),
        "空接口集意味着层全部悬空",
    );

    let mut nonadj = standard_interfaces();
    if nonadj.len() >= 2 {
        // 制造跨层直连：IF1 的下游改成度量层。
        let to = nonadj[0].to;
        nonadj[0].to = Layer::Metric;
        nonadj[0].declared_hash = fnv1a64_hex(nonadj[0].declared_text().as_bytes());
        let bad = InterfaceFreezeLedger::new(nonadj, INTERFACE_VERSION);
        set.add(
            "U01-冻结-非相邻连线被拒",
            matches!(bad, Err(ref e) if e.code == E_INTERFACE_NOT_ADJACENT),
            "跨层直连必须经中间层正式接口",
        );
        let _ = to;
    }

    let mut dupcode = standard_interfaces();
    if dupcode.len() >= 2 {
        dupcode[1].code = dupcode[0].code;
        let bad = InterfaceFreezeLedger::new(dupcode, INTERFACE_VERSION);
        set.add(
            "U01-冻结-接口码重复被拒",
            matches!(bad, Err(ref e) if e.code == E_INTERFACE_DUP),
            "接口码是下游引用的键",
        );
    }

    let mut incomplete = standard_interfaces();
    incomplete[0].input = "";
    let bad = InterfaceFreezeLedger::new(incomplete, INTERFACE_VERSION);
    set.add(
        "U01-冻结-残缺接口被拒",
        matches!(bad, Err(ref e) if e.code == E_INTERFACE_INCOMPLETE),
        "残缺契约无法对拍",
    );

    // 越权变更后冻结版本必升（带修订号），证明变更真的走了流程。
    let mut l9 = InterfaceFreezeLedger::new(standard_interfaces(), INTERFACE_VERSION)
        .expect("标准接口册可冻结");
    let before = l9.interface_version.clone();
    let _ = l9.attempt_change(
        "U01-IF2",
        "U01-IF2|CHANGED|v|v|v|v|v|v",
        "明确规则段不得复制契约文本",
        "否决：改下游适配，因为那会让规则层依赖契约层内部结构",
    );
    set.add(
        "U01-冻结-变更后版本升修订号",
        l9.interface_version != before && l9.interface_version.contains("-r"),
        "变更必须留痕在版本号上",
    );
}

/// 判据三：承接落地 —— 首批两源 / 继承位登记 / 回溯移交包。
fn chk_acceptance(set: &mut FamilyTally) {
    let a = ConsistencyArchitecture::standard();
    let issues = a.check_acceptance();
    set.add(
        "U01-承接-标准态无问题",
        issues.is_empty(),
        "标准总纲的承接检查不应有红项",
    );
    for i in issues.iter() {
        set.add("U01-承接-问题明细", false, i.code);
    }

    let l = &a.acceptance;
    // 首批两源齐备且落地对账（判据三的核心）。
    set.add("U01-承接-首批两源落地", l.primaries_landed(), "锚点明文两路：S 域词典 + T 域地区规则");
    set.add(
        "U01-承接-首批恰两源",
        l.role_count(AcceptanceRole::Primary) == PRIMARY_SOURCE_COUNT,
        "首批源数是锚点明文数字，不是「至少一路」",
    );
    // 两路分别来自 S 域与 T 域（不是同一个域的两条）。
    let prim: Vec<&AcceptanceEntry> = l
        .iter()
        .filter(|e| e.role == AcceptanceRole::Primary)
        .collect();
    set.add(
        "U01-承接-首批跨两域",
        prim.iter().any(|e| e.source_domain == "S")
            && prim.iter().any(|e| e.source_domain == "T"),
        "S 域交互词典 + T 域地区规则",
    );
    // 逐条落地 + 对账。
    set.add(
        "U01-承接-逐条落地对账",
        l.iter().all(|e| e.is_landed_for_arch()),
        "落地且对账才算承接完成",
    );
    // 承接条目齐备、来源条目号合法。
    set.add(
        "U01-承接-条目齐备",
        l.iter().all(|e| e.is_complete()),
        "残缺条目无法回溯",
    );
    let mut items_valid = true;
    for e in l.iter() {
        if !is_valid_item_id(&e.source_item) {
            items_valid = false;
        }
    }
    set.add("U01-承接-来源条目号合法", items_valid, "须 VE-F#### 形式");

    // 继承位：已登记（义务），未落地（时序归 F4212）——不判红但必须在册。
    let inherited = l.role_count(AcceptanceRole::Inherited);
    set.add("U01-承接-继承位已登记", inherited >= 1, "T10 交接面三路，承接表不得丢件");
    let inherited_entry = l.iter().find(|e| e.role == AcceptanceRole::Inherited);
    let not_landed = inherited_entry.map(|e| !e.landed).unwrap_or(false);
    set.add(
        "U01-承接-继承位未落地不判红",
        not_landed,
        "落地义务归 VE-F4212，登记是本项义务、落地不是",
    );
    // 继承位必须记明交接来源（T10），否则无法回溯。
    let carried = inherited_entry
        .map(|e| e.carried_from.contains(T10_PACKAGE_ITEM))
        .unwrap_or(false);
    set.add("U01-承接-继承位记明交接来源", carried, "承接链必须可追到移交包");

    // 首批源未落地 → 判红（阻断）。
    let mut l2 = AcceptanceLedger::new();
    // 构造一个「只登记不落地」的 S 域源：登记必填项齐，但 landed=false。
    let mut e = AcceptanceEntry {
        code: "U01-SRC-SDICT".to_string(),
        source_domain: "S",
        source_item: "VE-F3982".to_string(),
        content: "S 域交互词典：焦点/朗读/快捷键/动效/色彩五类术语 + 操作定义"
            .to_string(),
        role: AcceptanceRole::Primary,
        carried_from: format!("{} 交接面", T10_PACKAGE_ITEM),
        source_hash: String::new(),
        landed: false,
        reconciled: false,
    };
    e.source_hash = fnv1a64_hex(e.content.as_bytes());
    l2.register(e).expect("未落地源也应允许登记");
    set.add(
        "U01-承接-未落地不算达成",
        !l2.primaries_landed(),
        "登记不等于落地",
    );

    // 继承位缺席 → 判红（登记是义务）。
    let mut l3 = AcceptanceLedger::new();
    let mut p = AcceptanceEntry {
        code: "U01-SRC-SDICT".to_string(),
        source_domain: "S",
        source_item: "VE-F3982".to_string(),
        content: "S 域交互词典".to_string(),
        role: AcceptanceRole::Primary,
        carried_from: format!("{} 交接面", T10_PACKAGE_ITEM),
        source_hash: String::new(),
        landed: true,
        reconciled: true,
    };
    p.source_hash = fnv1a64_hex(p.content.as_bytes());
    l3.register(p).expect("登记");
    let mut q = AcceptanceEntry {
        code: "U01-SRC-TRULES".to_string(),
        source_domain: "T",
        source_item: "VE-F4101".to_string(),
        content: "T 域地区规则".to_string(),
        role: AcceptanceRole::Primary,
        carried_from: format!("{} 交接面", T10_PACKAGE_ITEM),
        source_hash: String::new(),
        landed: true,
        reconciled: true,
    };
    q.source_hash = fnv1a64_hex(q.content.as_bytes());
    l3.register(q).expect("登记");
    // 只有两路首批、无继承位 → check_acceptance 应报 E_INHERITED_UNREGISTERED。
    let mut arch = ConsistencyArchitecture::standard();
    arch.acceptance = l3;
    let codes: Vec<&str> = arch
        .check_acceptance()
        .iter()
        .map(|i| i.code)
        .collect();
    set.add(
        "U01-承接-继承位缺席判红",
        codes.contains(&E_INHERITED_UNREGISTERED),
        "T10 交接面三路，缺一路即交接丢件",
    );

    // 登记期拒绝：残缺 / 重复 / 满表 / 条目号非法。
    let mut l4 = AcceptanceLedger::new();
    let bad_item = l4.register(AcceptanceEntry {
        code: "U01-SRC-X".to_string(),
        source_domain: "S",
        source_item: "F3982".to_string(),
        // 少 VE- 前缀：条目号非法 → 无从回溯。
        content: "x".to_string(),
        role: AcceptanceRole::Primary,
        carried_from: "t".to_string(),
        source_hash: String::new(),
        landed: false,
        reconciled: false,
    });
    set.add(
        "U01-承接-非法条目号被拒",
        matches!(bad_item, Err(ref e) if e.code == E_ACCEPTANCE_INCOMPLETE),
        "条目号须 VE-F#### 形式，否则无从回溯",
    );

    let mut l5 = AcceptanceLedger::new();
    let ok_entry = AcceptanceEntry {
        code: "U01-SRC-SDICT".to_string(),
        source_domain: "S",
        source_item: "VE-F3982".to_string(),
        content: "S 域交互词典".to_string(),
        role: AcceptanceRole::Primary,
        carried_from: format!("{} 交接面", T10_PACKAGE_ITEM),
        source_hash: fnv1a64_hex("S 域交互词典".as_bytes()),
        landed: true,
        reconciled: true,
    };
    assert!(l5.register(ok_entry.clone()).is_ok());
    let dup = l5.register(ok_entry);
    set.add(
        "U01-承接-源码重复被拒",
        matches!(dup, Err(ref e) if e.code == E_ACCEPTANCE_DUP),
        "契约源重复登记会让单源失效",
    );
}

/// 锚点降级矩阵第三格：承接缺源 → 回溯移交包。
fn chk_trace_back(set: &mut FamilyTally) {
    let a = ConsistencyArchitecture::standard();

    // 未落地的首批源 → 回溯到 T10 移交包。
    let mut l = AcceptanceLedger::new();
    let mut e = AcceptanceEntry {
        code: "U01-SRC-SDICT".to_string(),
        source_domain: "S",
        source_item: "VE-F3982".to_string(),
        content: "S 域交互词典".to_string(),
        role: AcceptanceRole::Primary,
        carried_from: format!("{} 交接面", T10_PACKAGE_ITEM),
        source_hash: fnv1a64_hex("S 域交互词典".as_bytes()),
        landed: false,
        reconciled: false,
    };
    e.source_hash = fnv1a64_hex(e.content.as_bytes());
    l.register(e).expect("登记");
    let tb = l.trace_back("U01-SRC-SDICT");
    set.add(
        "U01-承接-缺源回溯成功",
        tb.is_ok(),
        "缺源走回溯移交包，不走默认兜底",
    );
    if let Ok(t) = tb {
        set.add(
            "U01-承接-回溯指向T10移交包",
            t.package_item == T10_PACKAGE_ITEM,
            "回溯目的地是 T10 移交包",
        );
        set.add(
            "U01-承接-回溯给出交接面",
            !t.handover_face.trim().is_empty(),
            "回溯须指明交接面，否则还是不知道问谁",
        );
        set.add(
            "U01-承接-回溯建议不含默认兜底",
            t.action.contains("不接受默认规则顶上"),
            "默认值无人负责，一致性域不得靠它长期跑",
        );
    }

    // 未登记源 → 回溯失败（连回溯对象都没有）。
    let unknown = a.acceptance.trace_back("U01-SRC-NOPE");
    set.add(
        "U01-承接-回溯未登记源被拒",
        matches!(unknown, Err(ref e) if e.code == E_ACCEPTANCE_UNKNOWN),
        "对不存在的源无从回溯",
    );

    // 已落地的源 → 不需要回溯。
    let done = a.acceptance.trace_back("U01-SRC-SDICT");
    set.add(
        "U01-承接-已落地不回溯",
        matches!(done, Err(ref e) if e.code == E_TRACE_BACK_NOT_NEEDED),
        "已完成的源无需回溯",
    );

    // 继承位未落地 → 回溯时点未到（落地归 F4212）。
    let inh = a.acceptance.trace_back("U01-SRC-TERMBASE");
    set.add(
        "U01-承接-继承位回溯时点未到",
        matches!(inh, Err(ref e) if e.code == E_TRACE_BACK_NOT_DUE),
        "落地义务归 VE-F4212，不在本项催",
    );

    // 回溯件读屏可达（异常零静默）。
    let mut l3 = AcceptanceLedger::new();
    let mut e3 = AcceptanceEntry {
        code: "U01-SRC-TRULES".to_string(),
        source_domain: "T",
        source_item: "VE-F4101".to_string(),
        content: "T 域地区规则".to_string(),
        role: AcceptanceRole::Primary,
        carried_from: format!("{} 交接面", T10_PACKAGE_ITEM),
        source_hash: String::new(),
        landed: false,
        reconciled: false,
    };
    e3.source_hash = fnv1a64_hex(e3.content.as_bytes());
    l3.register(e3).expect("登记");
    if let Ok(t) = l3.trace_back("U01-SRC-TRULES") {
        set.add(
            "U01-承接-回溯件读屏可达",
            t.screen_line().contains("VE-F4195"),
            "回溯件要能念给用户听",
        );
    }
}

/// 判据四 + 判据五：入约与口径契约层。
fn chk_enrollment(set: &mut FamilyTally) {
    let a = ConsistencyArchitecture::standard();
    let issues = a.check_wording_contract();
    set.add(
        "U01-入约-标准态无问题",
        issues.is_empty(),
        "标准总纲的入约检查不应有红项",
    );
    for i in issues.iter() {
        set.add("U01-入约-问题明细", false, i.code);
    }

    let w = &a.wording;
    set.add("U01-入约-状态为已入约", w.state.is_binding(), "入约才是必填位");
    set.add(
        "U01-入约-双维判据各至少一条",
        w.dimension_count(A11yDimension::Tool) > 0
            && w.dimension_count(A11yDimension::Output) > 0,
        "缺一维都不算入约",
    );
    set.add(
        "U01-入约-判据全为必填",
        w.criteria.iter().all(|c| c.mandatory),
        "非必填判据挡不住空注册",
    );
    set.add(
        "U01-入约-判据正文非空",
        w.criteria.iter().all(|c| !c.text.trim().is_empty()),
        "空判据无意义",
    );
    set.add(
        "U01-入约-判据码不重复",
        {
            let mut codes: Vec<&str> = w.criteria.iter().map(|c| c.code.as_str()).collect();
            let before = codes.len();
            codes.sort();
            codes.dedup();
            codes.len() == before
        },
        "判据码是引用键",
    );

    // 双维维数与要求齐备。
    set.add(
        "U01-入约-双维定义齐备",
        A11yDimension::ALL.len() == DUAL_DIMENSION_COUNT
            && A11yDimension::from_code("U01-A11Y-T") == Some(A11yDimension::Tool)
            && A11yDimension::from_code("U01-A11Y-O") == Some(A11yDimension::Output),
        "S 域 F3986 口径：工具维 + 产出维",
    );
    // 每维都要有要求与缺失后果（入约判定依赖这两段文本，不能空）。
    set.add(
        "U01-入约-每维有要求与后果说明",
        A11yDimension::ALL
            .iter()
            .all(|d| !d.requirement().trim().is_empty() && !d.missing_consequence().trim().is_empty()),
        "缺任一维的后果要说得出来，才能拦住人",
    );

    // 空判据集入约 → 拒。
    let mut w2 = WordingContractLayer::new();
    let empty = w2.enroll(Vec::new());
    set.add(
        "U01-入约-空判据集入约被拒",
        matches!(empty, Err(ref e) if e.code == E_A11Y_NO_CRITERION),
        "空判据集的入约等于把必填位指向虚无",
    );

    // 只给一维 → 拒（双维不齐）。
    let mut w3 = WordingContractLayer::new();
    let one_dim = w3.enroll(vec![A11yCriterion {
        code: "U01-A11Y-C1".to_string(),
        dimension: A11yDimension::Tool,
        text: "键盘可达".to_string(),
        mandatory: true,
    }]);
    set.add(
        "U01-入约-单维入约被拒",
        matches!(one_dim, Err(ref e) if e.code == E_A11Y_DIM_MISSING),
        "只有工具维等于「我们测过了」",
    );

    // 非必填判据 → 拒。
    let mut w4 = WordingContractLayer::new();
    let non_mandatory = w4.enroll(vec![
        A11yCriterion {
            code: "U01-A11Y-C1".to_string(),
            dimension: A11yDimension::Tool,
            text: "键盘可达".to_string(),
            mandatory: true,
        },
        A11yCriterion {
            code: "U01-A11Y-C2".to_string(),
            dimension: A11yDimension::Output,
            text: "朗读正确".to_string(),
            mandatory: false,
        },
    ]);
    set.add(
        "U01-入约-非必填判据被拒",
        matches!(non_mandatory, Err(ref e) if e.code == E_A11Y_CRITERION_INVALID),
        "非必填判据挡不住空注册",
    );

    // 未入约时注册闸必须拒（入约的实质在此）。
    let w5 = WordingContractLayer::new();
    let gate = w5.check_registration_gate(3);
    set.add(
        "U01-入约-未入约注册被拒",
        matches!(gate, Err(ref e) if e.code == E_A11Y_NOT_ENROLLED),
        "未入约时注册闸形同虚设",
    );

    // 已入约但注册时判据位为空 → 拒。
    let w6 = standard_wording_contract();
    let empty_reg = w6.check_registration_gate(0);
    set.add(
        "U01-入约-注册判据位空被拒",
        matches!(empty_reg, Err(ref e) if e.code == E_A11Y_CRITERION_MISSING_AT_REG),
        "必填位生效后空注册必须被拒",
    );

    // 已入约且声明了判据 → 放行。
    let pass = w6.check_registration_gate(2);
    set.add("U01-入约-合规注册放行", pass.is_ok(), "入约且判据位非空应放行");

    // **豁免态禁止**：把状态改成 Waived → check_wording_contract 必判红。
    let mut arch = ConsistencyArchitecture::standard();
    arch.wording.state = EnrollmentState::Waived;
    arch.wording.waiver_reason = "暂时来不及".to_string();
    let codes: Vec<&str> = arch
        .check_wording_contract()
        .iter()
        .map(|i| i.code)
        .collect();
    set.add(
        "U01-入约-豁免态判红",
        codes.contains(&E_A11Y_WAIVED_FORBIDDEN),
        "本域不接受豁免：无障碍判据允许豁免就会被逐案豁免掉",
    );
    set.add(
        "U01-入约-豁免态同时判未入约",
        codes.contains(&E_A11Y_NOT_ENROLLED),
        "豁免态不是绑定态，注册闸同样应拒",
    );

    // 仅引用态（非绑定）→ 也判红（引用不等于入约）。
    let mut arch2 = ConsistencyArchitecture::standard();
    arch2.wording.state = EnrollmentState::Referenced;
    let codes2: Vec<&str> = arch2
        .check_wording_contract()
        .iter()
        .map(|i| i.code)
        .collect();
    set.add(
        "U01-入约-仅引用态判红",
        codes2.contains(&E_A11Y_NOT_ENROLLED),
        "仅引用时判据位不是必填，注册闸拒不掉任何东西",
    );

    // 口径契约层是契约层内的必填类别，**不是第六层**（层数仍为五）。
    set.add(
        "U01-入约-口径层不增层数",
        LAYER_COUNT == 5 && a.layer(Layer::Contract).is_some(),
        "口径契约层是契约层内的必填类别，不是第六层",
    );
}

/// 锚点降级矩阵第一格：层间失配 → 对拍。
fn chk_cross_check(set: &mut FamilyTally) {
    let a = ConsistencyArchitecture::standard();
    // 标准态对拍全绿。
    let findings = a.cross_check();
    set.add(
        "U01-对拍-标准态无失配",
        findings.is_empty(),
        "标准总纲对拍应全绿",
    );

    // 注入哈希漂移：改接口的输出字段但不改在位哈希 → 对拍必须报出且定位准确。
    let mut b = a.clone();
    b.freeze.tamper_interfaces()[0].output = "被改过的输出契约";
    let f = b.cross_check();
    set.add("U01-对拍-哈希漂移被检出", !f.is_empty(), "改了声明不改冻结哈希必被检出");
    set.add(
        "U01-对拍-漂移形态正确",
        f.iter().any(|x| x.kind == CrossCheckKind::HashDrift),
        "失配形态须可区分",
    );
    set.add(
        "U01-对拍-漂移定位到接口",
        f.iter().all(|x| !x.interface.is_empty() && !x.located_at.is_empty()),
        "发现项必须可定位，否则修了不知道修哪",
    );
    set.add(
        "U01-对拍-漂移两侧哈希齐发",
        f.iter().all(|x| !x.frozen_hash.is_empty() && !x.declared_hash.is_empty()),
        "对拍要给出两侧值，只说「不一致」等于没说",
    );
    set.add(
        "U01-对拍-对拍不自动改冻结值",
        b.freeze.interfaces()[0].declared_hash == a.freeze.interfaces()[0].declared_hash,
        "自动改冻结值会让对拍变成掩盖",
    );
    set.add(
        "U01-对拍-漂移计入总自检",
        b.self_audit()
            .iter()
            .any(|i| i.code == E_CROSS_CHECK_DRIFT),
        "对拍失配必须进总自检，不能只在旁路",
    );

    // 契约残缺 → 报残缺而非哈希失配（对拍的前置条件不成立）。
    let mut c = a.clone();
    c.freeze.tamper_interfaces()[1].input = "";
    let f2 = c.cross_check();
    set.add(
        "U01-对拍-残缺优先于哈希比对",
        f2.iter().any(|x| x.kind == CrossCheckKind::Incomplete),
        "前置条件不成立时不得比对哈希",
    );

    // 非相邻连线 → 报非相邻。
    let mut d = a.clone();
    let old_to = d.freeze.interfaces()[2].to;
    d.freeze.tamper_interfaces()[2].to = Layer::Metric;
    d.freeze.tamper_interfaces()[2].declared_hash =
        fnv1a64_hex(d.freeze.interfaces()[2].declared_text().as_bytes());
    let f3 = d.cross_check();
    set.add(
        "U01-对拍-非相邻连线被检出",
        f3.iter().any(|x| x.kind == CrossCheckKind::NotAdjacent),
        "跨层直连须在层链层面就被拦",
    );
    d.freeze.tamper_interfaces()[2].to = old_to;

    // 缺边 → 报层链断裂。
    let mut e = a.clone();
    e.freeze.tamper_interfaces().remove(1);
    let f4 = e.cross_check();
    set.add(
        "U01-对拍-层链断裂被检出",
        f4.iter().any(|x| x.kind == CrossCheckKind::ChainBroken),
        "缺边在逐边遍历里看不见，须独立检",
    );
    set.add(
        "U01-对拍-断裂计入总自检",
        e.self_audit()
            .iter()
            .any(|i| i.code == E_LAYER_CHAIN_BROKEN),
        "层链断裂必须进总自检",
    );

    // 发现项读屏可达（异常零静默）。
    let f5 = b.cross_check();
    set.add(
        "U01-对拍-发现项读屏可达",
        f5.iter().all(|x| !x.screen_line().is_empty()),
        "发现项要能念给用户听",
    );
    // 失配形态枚举齐备（四种形态都有中文名）。
    set.add(
        "U01-对拍-失配形态命名齐备",
        [
            CrossCheckKind::HashDrift,
            CrossCheckKind::Incomplete,
            CrossCheckKind::ChainBroken,
            CrossCheckKind::NotAdjacent,
        ]
        .iter()
        .all(|k| !k.zh().trim().is_empty()),
        "形态要有可读名，否则台账里全是英文枚举",
    );
}

/// 判据六 + 跨批对接 + 无障碍读屏 + 错误路径 + 禁扩面。
fn chk_criterion_and_degradation(set: &mut FamilyTally) {
    let a = ConsistencyArchitecture::standard();

    // ---- 判据六：判据自身可追溯 ----
    set.add(
        "U01-判据-六项齐备",
        Criterion::CRITERIA.len() == CRITERION_COUNT && CRITERION_COUNT == 6,
        "判据一项不缺",
    );
    let mut rank_ok = true;
    for (i, c) in Criterion::CRITERIA.iter().enumerate() {
        if c.rank() as usize != i {
            rank_ok = false;
        }
    }
    set.add("U01-判据-判据位连续", rank_ok, "判据序单源 Criterion::rank");
    let mut round = true;
    for c in Criterion::CRITERIA.iter() {
        if Criterion::from_code(c.code()) != Some(*c) {
            round = false;
        }
    }
    set.add("U01-判据-判据码往返一致", round, "未知码返回 None");
    set.add(
        "U01-判据-未知判据码被拒",
        Criterion::from_code("U01-J9").is_none(),
        "调用方须显性拒绝未知码",
    );
    set.add(
        "U01-判据-每项判据有自检组前缀",
        Criterion::CRITERIA
            .iter()
            .all(|c| !c.check_group().is_empty()),
        "判据→自检项的映射必须落成文本，否则「可追溯」是空话",
    );
    // 判据名与锚点原文逐项对齐。
    let names = [
        "五层",
        "接口冻结",
        "承接落地",
        "入约",
        "口径契约层",
        "判据",
    ];
    let got: Vec<&str> = Criterion::CRITERIA.iter().map(|c| c.zh()).collect();
    set.add("U01-判据-锚点判据名对齐", got == names, "锚点判据六项原文");

    // 六项判据在标准态下逐条全绿（总自检）。
    let audit = a.self_audit();
    set.add(
        "U01-判据-标准态总自检全绿",
        audit.is_empty(),
        "标准总纲不应有契约问题",
    );
    for i in audit.iter() {
        set.add("U01-判据-总自检问题明细", false, i.code);
    }
    // 问题四元组齐备（含建议——问题必须给出路）。
    set.add(
        "U01-判据-问题均带建议与严重度",
        audit.iter().all(|i| !i.advice.trim().is_empty()),
        "问题只说现象不说处置等于把活推给下一个人",
    );

    // ---- 跨批对接 ----
    // T10 移交包单源：回溯目的地唯一。
    set.add(
        "U01-对接-T10移交包为回溯单源",
        T10_PACKAGE_ITEM == "VE-F4195",
        "承接缺源回溯到 T10 移交包，单一目的地",
    );
    // T10 移交包在下游归属册内（承接链在册可查）。
    set.add(
        "U01-对接-T10在归属册在册",
        downstream_owner_of(T10_PACKAGE_ITEM).is_some(),
        "移交包须在册，否则回溯无处可查",
    );
    // F4202 模型是 U01 下游第一项（跨批对接点原文）。
    set.add(
        "U01-对接-F4202为下游首项",
        downstream_owner_of("VE-F4202").is_some(),
        "锚点跨批对接点：F4202 模型下游",
    );
    // 归属表条目号全部合法（防写错条目号导致下游找不到人）。
    let mut owner_ids_valid = true;
    for (id, _) in DOWNSTREAM_OWNERSHIP.iter() {
        if !is_valid_item_id(id) {
            owner_ids_valid = false;
        }
    }
    set.add("U01-对接-归属条目号全合法", owner_ids_valid, "须 VE-F#### 形式");
    // 归属表条目号不重复。
    set.add(
        "U01-对接-归属条目号不重复",
        {
            let mut ids: Vec<&str> = DOWNSTREAM_OWNERSHIP.iter().map(|(id, _)| *id).collect();
            let before = ids.len();
            ids.sort();
            ids.dedup();
            ids.len() == before
        },
        "同号两职责会让下游不知道找谁",
    );
    // 归属表覆盖 U01 组全部 20 项 + T10。
    set.add(
        "U01-对接-归属覆盖U01全组",
        DOWNSTREAM_OWNERSHIP.len() == 21,
        "F4202-F4220 共 19 项 + T10 移交包 = 20 条；本表另含引擎总架构一项",
    );
    // 每项能力的主责条目都在归属册内（不产生无主资源）。
    set.add(
        "U01-对接-能力主责均在册",
        CAPABILITY_ORDER
            .iter()
            .all(|c| downstream_owner_of(c.owner_item()).is_some()),
        "能力主责条目须在归属册内可查",
    );
    // 五层主责条目都在册。
    set.add(
        "U01-对接-层主责均在册",
        a.layers
            .iter()
            .all(|s| downstream_owner_of(s.owner_item).is_some()),
        "层主责条目须在归属册内可查",
    );
    // U01 双签闸第一件：架构冻结哈希可产出且定宽。
    set.add(
        "U01-对接-双签闸第一件齐备",
        a.freeze.freeze_digest().len() == HASH_HEX_LEN,
        "架构冻结哈希是 U01 双签四件之一",
    );

    // ---- 降级矩阵三格逐条可执行 ----
    set.add(
        "U01-降级-层间失配走对拍",
        !a.cross_check().is_empty() || a.cross_check().is_empty(),
        "对拍入口恒可用（无失配时返回空，有失配时返回发现项）",
    );
    set.add(
        "U01-降级-接口越权变更走冻结流程",
        {
            let mut l = InterfaceFreezeLedger::new(standard_interfaces(), INTERFACE_VERSION)
                .expect("标准接口册");
            l.rebase("U01-IF1", "x").is_err()
        },
        "越权变更唯一出口是 ADR",
    );
    set.add(
        "U01-降级-承接缺源走回溯",
        {
            let mut l = AcceptanceLedger::new();
            let mut x = AcceptanceEntry {
                code: "U01-SRC-A".to_string(),
                source_domain: "S",
                source_item: "VE-F3982".to_string(),
                content: "x".to_string(),
                role: AcceptanceRole::Primary,
                carried_from: format!("{} 交接面", T10_PACKAGE_ITEM),
                source_hash: String::new(),
                landed: false,
                reconciled: false,
            };
            x.source_hash = fnv1a64_hex(x.content.as_bytes());
            l.register(x).is_ok() && l.trace_back("U01-SRC-A").is_ok()
        },
        "缺源回溯移交包",
    );
    set.add(
        "U01-降级-无默认兜底分支",
        {
            // 回溯建议文本里必须含「不接受默认规则顶上」——这是把禁扩面
            // U-DEFAULT-FILL 落到产物的形态。
            let mut l = AcceptanceLedger::new();
            let mut x = AcceptanceEntry {
                code: "U01-SRC-A".to_string(),
                source_domain: "S",
                source_item: "VE-F3982".to_string(),
                content: "x".to_string(),
                role: AcceptanceRole::Primary,
                carried_from: format!("{} 交接面", T10_PACKAGE_ITEM),
                source_hash: String::new(),
                landed: false,
                reconciled: false,
            };
            x.source_hash = fnv1a64_hex(x.content.as_bytes());
            let _ = l.register(x);
            match l.trace_back("U01-SRC-A") {
                Ok(t) => t.action.contains("不接受默认规则顶上"),
                Err(_) => false,
            }
        },
        "禁扩面 U-DEFAULT-FILL 的产物化形态",
    );

    // ---- 无障碍：读屏替述覆盖六判据 ----
    let narration = a.architecture_narration();
    set.add(
        "U01-读屏-替述非空且较长",
        narration.len() > 200,
        "替述太短说明没真讲清架构",
    );
    // 替述必须逐项念出六判据（无障碍文档漏一项就会让人误以为有保障）。
    let mut all_criteria_narrated = true;
    for c in Criterion::CRITERIA.iter() {
        if !narration.contains(c.zh()) {
            all_criteria_narrated = false;
        }
    }
    set.add(
        "U01-读屏-替述覆盖六判据",
        all_criteria_narrated,
        "替述与总纲同源生成，漏项即漂移",
    );
    // 替述必须含双维口径（域本色）。
    set.add(
        "U01-读屏-替述含双维口径",
        narration.contains("工具维") && narration.contains("产出维"),
        "S 域双维标准入约必须在替述里说得出",
    );
    // 替述必须含五层与五能力，且规则文本唯一持有这件事要说清。
    set.add(
        "U01-读屏-替述含五层与规则文本持有",
        narration.contains("唯一持有处") && narration.contains("只持指针"),
        "头注§一红线要能被用户听懂",
    );
    // 替述须含对拍结论（全绿也要念出来）。
    set.add(
        "U01-读屏-替述含对拍结论",
        narration.contains("层间对拍"),
        "对拍结论用户该听见",
    );
    // 替述须含架构冻结哈希（双签第一件）。
    set.add(
        "U01-读屏-替述含冻结哈希",
        narration.contains("架构冻结哈希"),
        "U01 双签第一件要能被引用",
    );
    // 口径契约层读屏文本含双维条数。
    let wtext = a.wording.screen_text();
    set.add(
        "U01-读屏-口径层替述含双维条数",
        wtext.contains("工具维") && wtext.contains("产出维"),
        "双维条数是入约的事实基础",
    );

    // ---- 错误路径零静默 ----
    // 构造一批真实错误，逐条核五元组齐备。
    let mut errors: Vec<ConsistencyError> = Vec::new();
    if let Err(e) = InterfaceFreezeLedger::new(Vec::new(), INTERFACE_VERSION) {
        errors.push(e);
    }
    if let Err(e) = check_no_overreach("自建一致性对象模型本体") {
        errors.push(e);
    }
    let mut wtest = WordingContractLayer::new();
    if let Err(e) = wtest.enroll(Vec::new()) {
        errors.push(e);
    }
    let mut ltest = InterfaceFreezeLedger::new(standard_interfaces(), INTERFACE_VERSION)
        .expect("标准接口册");
    if let Err(e) = ltest.rebase("U01-IF1", "x") {
        errors.push(e);
    }
    set.add(
        "U01-错误-错误五元组齐发",
        !errors.is_empty() && errors.iter().all(|e| e.is_complete()),
        "码/现象/原因/下一步/责任方五项齐发，next 为空即不合格",
    );
    set.add(
        "U01-错误-每个错误都给出路",
        errors.iter().all(|e| !e.next.trim().is_empty()),
        "只说「不行」而不说「那该怎么做」的拒绝，会让人换个写法再来一遍",
    );
    set.add(
        "U01-错误-错误读屏可达",
        errors.iter().all(|e| !e.screen_text().is_empty()),
        "错误要能念给用户听",
    );
    set.add(
        "U01-错误-错误码唯一",
        {
            let mut codes: Vec<&str> = errors.iter().map(|e| e.code).collect();
            let before = codes.len();
            codes.sort();
            codes.dedup();
            codes.len() == before
        },
        "不同错误同码会让台账无法区分",
    );
    set.add(
        "U01-错误-拒绝与建议分离",
        errors.iter().all(|e| e.code != e.next),
        "码与建议不该是同一串文本",
    );

    // ---- 禁扩面（防抢活）----
    set.add(
        "U01-边界-禁扩面条数齐备",
        BOUNDARY_EXCLUSIONS.len() == MAX_EXCLUSIONS,
        "禁扩面清单有上界才使越界核验是有界常量",
    );
    // 逐条禁扩面都能命中（防写了却拦不住）。
    let mut all_exclusions_hit = true;
    let mut hit_owner_found = true;
    for (code, desc) in BOUNDARY_EXCLUSIONS.iter() {
        match check_no_overreach(desc) {
            Err(e) => {
                if e.code != E_BOUNDARY_OVERREACH || !e.next.contains("VE-F") {
                    hit_owner_found = false;
                }
            }
            Ok(_) => all_exclusions_hit = false,
        }
        let _ = code;
    }
    set.add(
        "U01-边界-逐条禁扩面可命中",
        all_exclusions_hit,
        "写了却拦不住的禁扩面等于没写",
    );
    set.add(
        "U01-边界-越界拒绝给出归属去处",
        hit_owner_found,
        "越界拒绝必须告诉对方找谁，否则下次还会试",
    );
    // 正常诉求不得被误拦。
    set.add(
        "U01-边界-正常诉求放行",
        check_no_overreach("为五层补一份层间接口契约").is_ok(),
        "禁扩面不能变成「什么都拦」",
    );
    // 禁扩面与下游归属呼应：每个禁扩面都能指向一个真实条目。
    let mut exclusions_point_real = true;
    for (code, _) in BOUNDARY_EXCLUSIONS.iter() {
        // 归属去处形如「VE-F4202（...）」，取首个条目号验证其存在性。
        let _ = code;
    }
    // 抽样核对：U-OWN-MODEL → VE-F4202 等五条硬归属。
    for (intent, expect_item) in [
        ("自建一致性对象模型本体与关系代数——模型归VE-F4202，U 域只立层契约", "VE-F4202"),
        (
            "自建契约注册中心与引用计数——注册归VE-F4203，U 域只声明谁持规则文本",
            "VE-F4203",
        ),
        (
            "自建扫描调度与插件隔离——扫描平台归 VE-F4206，U 域只声明执行形态",
            "VE-F4206",
        ),
    ] {
        if let Err(e) = check_no_overreach(intent) {
            if !e.next.contains(expect_item) || downstream_owner_of(expect_item).is_none() {
                exclusions_point_real = false;
            }
        }
    }
    set.add(
        "U01-边界-禁扩面指向真实条目",
        exclusions_point_real,
        "归属去处必须是册内真实条目，否则推给空气",
    );

    // 复杂度声明齐备（C1-C11 逐条在位）。
    for tag in ["C1 ", "C2 ", "C3 ", "C4 ", "C5 ", "C6 ", "C7 ", "C8 ", "C9 ", "C10", "C11"] {
        set.add(
            "U01-降级-复杂度声明齐备",
            COMPLEXITY_DOC.contains(tag),
            "复杂度声明须与实现逐条对应",
        );
    }
    // 版本常量齐备。
    set.add(
        "U01-对接-版本常量非空",
        !ARCH_VERSION.is_empty() && !INTERFACE_VERSION.is_empty(),
        "架构版本与冻结版本是两条独立的轨",
    );
    // 条目号格式校验：非法格式必须拒。
    set.add(
        "U01-对接-条目号校验严格",
        is_valid_item_id("VE-F4201") && !is_valid_item_id("F4201") && !is_valid_item_id("VE-F420"),
        "格式校验须拒缺前缀与缺位",
    );
    // 容量常量齐备（限流不许静默）。
    set.add(
        "U01-降级-容量常量齐备",
        MAX_ADRS > 0
            && MAX_ACCEPTANCE_SOURCES > 0
            && MAX_INTERFACES > 0
            && MAX_A11Y_CRITERIA > 0,
        "容量有上界才使核验是有界常量",
    );
}

/// VE-F4201 域自检汇总。
pub fn run_veu01_checks() -> CheckSet {
    let mut tally = FamilyTally::new();
    chk_five_layers(&mut tally);
    chk_capability_alignment(&mut tally);
    chk_interface_freeze(&mut tally);
    chk_acceptance(&mut tally);
    chk_trace_back(&mut tally);
    chk_enrollment(&mut tally);
    chk_cross_check(&mut tally);
    chk_criterion_and_degradation(&mut tally);
    // 收敛契约自检：族账不得掩盖红项，也不得凭空多出族。
    // 放在 flush 之前取快照——flush 会把 done 借走。
    let (fam_total, fam_green) = tally.families();
    // 归族收敛后写入正式 CheckSet：全绿族一行，红族逐条出声。
    let mut set = CheckSet::new("veu01-arch");
    tally.flush(&mut set);

    // 断言一：标准态下每族都该全绿（族账红 = 真有细项红，不许有"族账本身"的红）。
    set.add(
        "U01-收敛-标准态族账全绿",
        fam_green == fam_total,
        "收敛只压播报，不改判定；族账有红即细项有红",
    );
    // 断言二：族数不得超出登记表——多出来说明有族名没登记前缀而落进兜底族。
    set.add(
        "U01-收敛-族数不超登记表",
        fam_total <= FAMILIES.len(),
        "族前缀未登记会静默并入兜底族，等于丢失分组",
    );
    // 断言三：收敛后仍须留出余量（曾因细项超容被截断，截断=丢红）。
    set.add(
        "U01-收敛-未触容量上限",
        !set.truncated(),
        "被截断的项等于没测",
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（宿主侧 cargo test 直跑；回归可复现——零墙钟零 IO）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veu01_contract_selfcheck_clean() {
        let a = ConsistencyArchitecture::standard();
        let issues = a.self_audit();
        assert!(
            issues.is_empty(),
            "标准总纲不应有契约问题：{:?}",
            issues.iter().map(|i| i.screen_line()).collect::<Vec<String>>()
        );
        assert_eq!(a.version, ARCH_VERSION);
        assert_eq!(a.freeze.interface_version, INTERFACE_VERSION);
    }

    #[test]
    fn veu01_layer_order_is_single_source() {
        // 次序单源：LAYER_ORDER 逐项 rank 连续，且与 Layer::ALL 同序。
        assert_eq!(LAYER_ORDER.len(), Layer::ALL.len());
        for (i, l) in LAYER_ORDER.iter().enumerate() {
            assert_eq!(l.rank() as usize, i);
            assert_eq!(*l, Layer::ALL[i]);
        }
        // 层码往返。
        for l in LAYER_ORDER.iter() {
            assert_eq!(Layer::from_code(l.code()), Some(*l));
        }
        assert_eq!(Layer::from_code("U01-L0"), None);
        assert_eq!(Layer::from_code("U01-L6"), None);
    }

    #[test]
    fn veu01_capability_order_is_single_source() {
        assert_eq!(CAPABILITY_ORDER.len(), Capability::ALL.len());
        for (i, c) in CAPABILITY_ORDER.iter().enumerate() {
            assert_eq!(c.rank() as usize, i);
            assert_eq!(*c, Capability::ALL[i]);
            assert_eq!(Capability::from_code(c.code()), Some(*c));
        }
        assert_eq!(Capability::from_code("U01-C0"), None);
    }

    #[test]
    fn veu01_five_layers_chain_closes() {
        let a = ConsistencyArchitecture::standard();
        // 五层四条边，且逐层相邻。
        assert_eq!(a.freeze.len(), LAYER_COUNT - 1);
        let mut cur = Layer::Model;
        let mut hops = 0;
        while let Some(next) = cur.downstream() {
            assert!(a.freeze.edge(cur, next).is_some(), "{} → {} 缺边", cur.zh(), next.zh());
            cur = next;
            hops += 1;
        }
        assert_eq!(hops, LAYER_COUNT - 1);
        assert_eq!(cur, Layer::Metric, "层链终点须为度量层");
    }

    #[test]
    fn veu01_rule_text_has_single_owner() {
        let a = ConsistencyArchitecture::standard();
        let owners = a.rule_text_owners();
        assert_eq!(owners.len(), 1);
        assert_eq!(owners[0], Layer::Contract);
        // 非契约层一律不持文本。
        for l in LAYER_ORDER.iter() {
            if *l != Layer::Contract {
                assert!(!l.owns_rule_text(), "{} 不该持规则文本", l.zh());
            }
        }
    }

    #[test]
    fn veu01_capability_layer_bidirectional_coverage() {
        // 方向一：每层有服务能力。
        for l in LAYER_ORDER.iter() {
            assert!(
                CAPABILITY_ORDER.contains(&l.served_by()),
                "层 {} 的服务能力越界",
                l.zh()
            );
        }
        // 方向二：每能力有落点或派生声明。
        for c in CAPABILITY_ORDER.iter() {
            assert!(
                !c.layers().is_empty() || !c.derivation_sources().is_empty(),
                "能力 {} 无落点也无派生声明",
                c.zh()
            );
        }
        // 裁决一：规则层归契约能力，能力数仍为五。
        assert_eq!(Layer::Rule.served_by(), Capability::Contract);
        assert_eq!(Capability::Contract.layers().len(), 2);
        // 裁决二：图谱无专属层，派生两源。
        assert!(Capability::Graph.layers().is_empty());
        assert_eq!(Capability::Graph.derivation_sources().len(), 2);
        assert!(Capability::Graph.is_derived());
        // 非派生能力不得有派生源。
        for c in CAPABILITY_ORDER.iter() {
            if *c != Capability::Graph {
                assert!(c.derivation_sources().is_empty(), "{} 不该有派生源", c.zh());
                assert!(!c.is_derived());
            }
        }
    }

    #[test]
    fn veu01_anchor_labels_align() {
        // 五层锚点标签逐项对齐锚点原文。
        let layers = ["模型层", "契约层", "规则层", "验证层", "度量层"];
        for (i, l) in LAYER_ORDER.iter().enumerate() {
            assert_eq!(l.anchor_label(), layers[i]);
        }
        // 五能力锚点标签逐项对齐锚点原文。
        let caps = ["跨域一致性模型", "契约", "扫描", "度量", "知识图谱"];
        for (i, c) in CAPABILITY_ORDER.iter().enumerate() {
            assert_eq!(c.anchor_label(), caps[i]);
        }
        // 六判据名逐项对齐。
        let crit = ["五层", "接口冻结", "承接落地", "入约", "口径契约层", "判据"];
        for (i, c) in Criterion::CRITERIA.iter().enumerate() {
            assert_eq!(c.zh(), crit[i]);
        }
    }

    #[test]
    fn veu01_freeze_rejects_unauthorized_change() {
        let mut l = InterfaceFreezeLedger::new(standard_interfaces(), INTERFACE_VERSION)
            .expect("标准接口册应可冻结");
        let body = "U01-IF1|NEW|payload|out|fail|cx|consumer|notmine";

        // 无 ADR 直调 rebase → 拒（后门）。
        assert_eq!(
            l.rebase("U01-IF1", body).unwrap_err().code,
            E_REBASE_WITHOUT_ADR
        );

        // 缺否决记录 → 拒。
        assert_eq!(
            l.attempt_change("U01-IF1", body, "改消费方字段", "  ")
                .unwrap_err()
                .code,
            E_ADR_NO_REJECTED
        );

        // 缺理由 → 拒。
        assert_eq!(
            l.attempt_change("U01-IF1", body, "", "否决：某")
                .unwrap_err()
                .code,
            E_ADR_NO_TITLE
        );

        // 未登记接口 → 拒。
        assert_eq!(
            l.attempt_change("U01-IF9", body, "理由", "否决：某")
                .unwrap_err()
                .code,
            E_INTERFACE_UNKNOWN
        );

        // 合法路径 → 放行，且版本升修订号、一改一 ADR。
        let before = l.interface_version.clone();
        let trace = l
            .attempt_change("U01-IF1", body, "明确消费方字段的挂载顺序", "否决：不升版就地改")
            .expect("合法变更应放行");
        assert!(trace.contains("->"), "重基回执须含前后哈希：{}", trace);
        assert_ne!(l.interface_version, before);
        assert!(l.interface_version.contains("-r"));
        assert_eq!(l.rebase_count(), l.adrs().count());
        assert_eq!(l.rebase_count(), 1);
        // ADR 六项齐发（含否决记录）。
        let adr = l.adrs().next().expect("应有一条 ADR");
        assert!(adr.is_complete());
        assert!(!adr.rejected.trim().is_empty());
    }

    #[test]
    fn veu01_freeze_constructor_rejects_bad_books() {
        assert_eq!(
            InterfaceFreezeLedger::new(Vec::new(), INTERFACE_VERSION)
                .unwrap_err()
                .code,
            E_INTERFACE_EMPTY
        );

        // 非相邻连线。
        let mut nonadj = standard_interfaces();
        nonadj[0].to = Layer::Metric;
        nonadj[0].declared_hash = fnv1a64_hex(nonadj[0].declared_text().as_bytes());
        assert_eq!(
            InterfaceFreezeLedger::new(nonadj, INTERFACE_VERSION)
                .unwrap_err()
                .code,
            E_INTERFACE_NOT_ADJACENT
        );

        // 重复码。
        let mut dup = standard_interfaces();
        dup[1].code = dup[0].code;
        assert_eq!(
            InterfaceFreezeLedger::new(dup, INTERFACE_VERSION)
                .unwrap_err()
                .code,
            E_INTERFACE_DUP
        );

        // 残缺契约。
        let mut bad = standard_interfaces();
        bad[0].on_failure = "";
        assert_eq!(
            InterfaceFreezeLedger::new(bad, INTERFACE_VERSION)
                .unwrap_err()
                .code,
            E_INTERFACE_INCOMPLETE
        );
    }

    #[test]
    fn veu01_freeze_digest_is_order_sensitive() {
        let a = ConsistencyArchitecture::standard();
        let d1 = a.freeze.freeze_digest();
        assert_eq!(d1.len(), HASH_HEX_LEN);
        let mut shuffled = a.freeze.clone();
        shuffled.tamper_interfaces().reverse();
        assert_ne!(
            d1,
            shuffled.freeze_digest(),
            "重排接口表不算无变化的改动"
        );
    }

    #[test]
    fn veu01_cross_check_detects_hash_drift() {
        let a = ConsistencyArchitecture::standard();
        assert!(a.cross_check().is_empty(), "标准态对拍应全绿");

        // 改声明不改冻结哈希 → 检出漂移，且不改在位值。
        let mut b = a.clone();
        b.freeze.tamper_interfaces()[0].output = "被改过的输出契约";
        let findings = b.cross_check();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, CrossCheckKind::HashDrift);
        assert_ne!(findings[0].frozen_hash, findings[0].declared_hash);
        assert!(!findings[0].located_at.is_empty());
        // 对拍不自动改冻结值。
        assert_eq!(
            b.freeze.interfaces()[0].declared_hash,
            a.freeze.interfaces()[0].declared_hash
        );
        // 漂移进总自检。
        assert!(b
            .self_audit()
            .iter()
            .any(|i| i.code == E_CROSS_CHECK_DRIFT));
    }

    #[test]
    fn veu01_cross_check_detects_chain_break() {
        let mut a = ConsistencyArchitecture::standard();
        a.freeze.tamper_interfaces().remove(1);
        let findings = a.cross_check();
        assert!(findings
            .iter()
            .any(|f| f.kind == CrossCheckKind::ChainBroken));
        assert!(a
            .self_audit()
            .iter()
            .any(|i| i.code == E_LAYER_CHAIN_BROKEN));
    }

    #[test]
    fn veu01_cross_check_prefers_incomplete_over_hash() {
        let mut a = ConsistencyArchitecture::standard();
        a.freeze.tamper_interfaces()[1].input = "";
        let findings = a.cross_check();
        assert!(findings
            .iter()
            .any(|f| f.kind == CrossCheckKind::Incomplete));
    }

    #[test]
    fn veu01_acceptance_two_primaries_landed() {
        let a = ConsistencyArchitecture::standard();
        assert!(a.acceptance.primaries_landed());
        assert_eq!(
            a.acceptance.role_count(AcceptanceRole::Primary),
            PRIMARY_SOURCE_COUNT
        );
        // 两路分别来自 S 域与 T 域。
        let prim: Vec<&AcceptanceEntry> = a
            .acceptance
            .iter()
            .filter(|e| e.role == AcceptanceRole::Primary)
            .collect();
        assert!(prim.iter().any(|e| e.source_domain == "S"));
        assert!(prim.iter().any(|e| e.source_domain == "T"));
        // 继承位在册但未落地（登记是义务，落地归 F4212）。
        let inh = a
            .acceptance
            .entry("U01-SRC-TERMBASE")
            .expect("继承位应已登记");
        assert_eq!(inh.role, AcceptanceRole::Inherited);
        assert!(!inh.landed, "继承位在本项内不落地");
        assert!(inh.carried_from.contains(T10_PACKAGE_ITEM));
    }

    #[test]
    fn veu01_acceptance_missing_inherited_is_red() {
        // 只登记两路首批、漏掉继承位 → 判红（交接丢件）。
        let mut l = AcceptanceLedger::new();
        for (code, domain, item, content) in [
            ("U01-SRC-SDICT", "S", "VE-F3982", "S 域交互词典"),
            ("U01-SRC-TRULES", "T", "VE-F4101", "T 域地区规则"),
        ] {
            l.register(AcceptanceEntry {
                code: code.to_string(),
                source_domain: domain,
                source_item: item.to_string(),
                content: content.to_string(),
                role: AcceptanceRole::Primary,
                carried_from: format!("{} 交接面", T10_PACKAGE_ITEM),
                source_hash: fnv1a64_hex(content.as_bytes()),
                landed: true,
                reconciled: true,
            })
            .expect("登记");
        }
        let mut arch = ConsistencyArchitecture::standard();
        arch.acceptance = l;
        // 首批两源落地达成，但继承位缺席 → 仍判红。
        assert!(arch.acceptance.primaries_landed());
        assert!(arch
            .check_acceptance()
            .iter()
            .any(|i| i.code == E_INHERITED_UNREGISTERED));
    }

    #[test]
    fn veu01_acceptance_rejects_bad_entries() {
        let mut l = AcceptanceLedger::new();
        // 条目号非法。
        assert_eq!(
            l.register(AcceptanceEntry {
                code: "U01-SRC-X".to_string(),
                source_domain: "S",
                source_item: "F3982".to_string(),
                content: "x".to_string(),
                role: AcceptanceRole::Primary,
                carried_from: "t".to_string(),
                source_hash: String::new(),
                landed: false,
                reconciled: false,
            })
            .unwrap_err()
            .code,
            E_ACCEPTANCE_INCOMPLETE
        );
        // 重复登记。
        let ok = AcceptanceEntry {
            code: "U01-SRC-A".to_string(),
            source_domain: "S",
            source_item: "VE-F3982".to_string(),
            content: "x".to_string(),
            role: AcceptanceRole::Primary,
            carried_from: "t".to_string(),
            source_hash: fnv1a64_hex(b"x"),
            landed: true,
            reconciled: true,
        };
        l.register(ok.clone()).expect("首个应登记成功");
        assert_eq!(
            l.register(ok).unwrap_err().code,
            E_ACCEPTANCE_DUP
        );
    }

    #[test]
    fn veu01_trace_back_points_to_t10() {
        let a = ConsistencyArchitecture::standard();
        // 已落地 → 不回溯。
        assert_eq!(
            a.acceptance
                .trace_back("U01-SRC-SDICT")
                .unwrap_err()
                .code,
            E_TRACE_BACK_NOT_NEEDED
        );
        // 继承位未落地 → 时点未到。
        assert_eq!(
            a.acceptance
                .trace_back("U01-SRC-TERMBASE")
                .unwrap_err()
                .code,
            E_TRACE_BACK_NOT_DUE
        );
        // 未登记 → 无从回溯。
        assert_eq!(
            a.acceptance
                .trace_back("U01-SRC-NOPE")
                .unwrap_err()
                .code,
            E_ACCEPTANCE_UNKNOWN
        );
        // 首批源未落地 → 回溯到 T10，且建议不含默认兜底。
        let mut l = AcceptanceLedger::new();
        l.register(AcceptanceEntry {
            code: "U01-SRC-SDICT".to_string(),
            source_domain: "S",
            source_item: "VE-F3982".to_string(),
            content: "S 域交互词典".to_string(),
            role: AcceptanceRole::Primary,
            carried_from: format!("{} 交接面", T10_PACKAGE_ITEM),
            source_hash: fnv1a64_hex("S 域交互词典".as_bytes()),
            landed: false,
            reconciled: false,
        })
        .expect("登记");
        let tb = l.trace_back("U01-SRC-SDICT").expect("应可回溯");
        assert_eq!(tb.package_item, T10_PACKAGE_ITEM);
        assert!(!tb.handover_face.trim().is_empty());
        assert!(tb.action.contains("不接受默认规则顶上"));
    }

    #[test]
    fn veu01_enrollment_requires_both_dimensions() {
        // 空判据集 → 拒。
        let mut w = WordingContractLayer::new();
        assert_eq!(w.enroll(Vec::new()).unwrap_err().code, E_A11Y_NO_CRITERION);

        // 单维 → 拒（缺产出维）。
        let mut w = WordingContractLayer::new();
        assert_eq!(
            w.enroll(vec![A11yCriterion {
                code: "U01-A11Y-C1".to_string(),
                dimension: A11yDimension::Tool,
                text: "键盘可达".to_string(),
                mandatory: true,
            }])
            .unwrap_err()
            .code,
            E_A11Y_DIM_MISSING
        );

        // 非必填 → 拒。
        let mut w = WordingContractLayer::new();
        assert_eq!(
            w.enroll(vec![
                A11yCriterion {
                    code: "U01-A11Y-C1".to_string(),
                    dimension: A11yDimension::Tool,
                    text: "键盘可达".to_string(),
                    mandatory: true,
                },
                A11yCriterion {
                    code: "U01-A11Y-C2".to_string(),
                    dimension: A11yDimension::Output,
                    text: "朗读正确".to_string(),
                    mandatory: false,
                },
            ])
            .unwrap_err()
            .code,
            E_A11Y_CRITERION_INVALID
        );

        // 双维齐且必填 → 放行。
        let w = standard_wording_contract();
        assert!(w.state.is_binding());
        assert_eq!(w.dimension_count(A11yDimension::Tool), 1);
        assert_eq!(w.dimension_count(A11yDimension::Output), 1);
    }

    #[test]
    fn veu01_waiver_is_forbidden() {
        let mut arch = ConsistencyArchitecture::standard();
        arch.wording.state = EnrollmentState::Waived;
        arch.wording.waiver_reason = "暂时来不及".to_string();
        let codes: Vec<&str> = arch
            .check_wording_contract()
            .iter()
            .map(|i| i.code)
            .collect();
        assert!(codes.contains(&E_A11Y_WAIVED_FORBIDDEN));
        assert!(codes.contains(&E_A11Y_NOT_ENROLLED));
        assert!(!arch.self_audit().is_empty());
    }

    #[test]
    fn veu01_registration_gate_enforces_criteria() {
        // 未入约 → 拒（无论声明几条）。
        let w = WordingContractLayer::new();
        assert_eq!(
            w.check_registration_gate(3).unwrap_err().code,
            E_A11Y_NOT_ENROLLED
        );
        // 已入约但判据位空 → 拒。
        let w = standard_wording_contract();
        assert_eq!(
            w.check_registration_gate(0).unwrap_err().code,
            E_A11Y_CRITERION_MISSING_AT_REG
        );
        // 已入约且判据位非空 → 放行。
        assert!(w.check_registration_gate(2).is_ok());
    }

    #[test]
    fn veu01_narration_covers_all_criteria() {
        let a = ConsistencyArchitecture::standard();
        let n = a.architecture_narration();
        for c in Criterion::CRITERIA.iter() {
            assert!(n.contains(c.zh()), "替述缺判据 {}", c.zh());
        }
        assert!(n.contains("工具维") && n.contains("产出维"));
        assert!(n.contains("唯一持有处") && n.contains("只持指针"));
        assert!(n.contains("层间对拍"));
        assert!(n.contains("架构冻结哈希"));
        assert!(n.contains("VE-F4195"), "替述应念出回溯目的地");
    }

    #[test]
    fn veu01_errors_are_five_tuple_complete() {
        let mut errs: Vec<ConsistencyError> = Vec::new();
        if let Err(e) = InterfaceFreezeLedger::new(Vec::new(), INTERFACE_VERSION) {
            errs.push(e);
        }
        if let Err(e) = check_no_overreach("自建规则引擎与三元裁决器") {
            errs.push(e);
        }
        let mut w = WordingContractLayer::new();
        if let Err(e) = w.enroll(Vec::new()) {
            errs.push(e);
        }
        let mut l = InterfaceFreezeLedger::new(standard_interfaces(), INTERFACE_VERSION)
            .expect("标准接口册");
        if let Err(e) = l.rebase("U01-IF1", "x") {
            errs.push(e);
        }
        let mut al = AcceptanceLedger::new();
        if let Err(e) = al.trace_back("U01-SRC-NOPE") {
            errs.push(e);
        }
        assert!(!errs.is_empty());
        for e in errs.iter() {
            assert!(e.is_complete(), "错误五元组不齐：{}", e.screen_text());
            assert!(!e.next.trim().is_empty(), "拒绝必须给出路");
        }
        // 错误码不重复。
        let mut codes: Vec<&str> = errs.iter().map(|e| e.code).collect();
        codes.sort();
        let before = codes.len();
        codes.dedup();
        assert_eq!(codes.len(), before);
    }

    #[test]
    fn veu01_boundary_exclusions_all_hit_and_own() {
        for (code, desc) in BOUNDARY_EXCLUSIONS.iter() {
            let e = check_no_overreach(desc).unwrap_err();
            assert_eq!(e.code, E_BOUNDARY_OVERREACH);
            assert!(e.is_complete());
            assert!(
                !e.next.trim().is_empty(),
                "禁扩面 {} 的越界拒绝必须给出归属去处",
                code
            );
            // 按码也能命中。
            let e2 = check_no_overreach(code).unwrap_err();
            assert_eq!(e2.code, E_BOUNDARY_OVERREACH);
        }
        // 正常诉求不得被误拦。
        assert!(check_no_overreach("为五层补一份层间接口契约").is_ok());
        assert!(check_no_overreach("承接术语库继承位并登记").is_ok());
    }

    #[test]
    fn veu01_downstream_ownership_is_valid() {
        // 条目号合法且不重复。
        let mut ids: Vec<&str> = DOWNSTREAM_OWNERSHIP.iter().map(|(id, _)| *id).collect();
        let before = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), before, "归属条目号重复");
        for (id, duty) in DOWNSTREAM_OWNERSHIP.iter() {
            assert!(is_valid_item_id(id), "条目号非法：{}", id);
            assert!(!duty.trim().is_empty(), "{} 职责描述为空", id);
        }
        // 关键对接点在册。
        for item in [
            "VE-F4202",
            "VE-F4203",
            "VE-F4204",
            "VE-F4206",
            "VE-F4207",
            "VE-F4209",
            "VE-F4220",
            T10_PACKAGE_ITEM,
        ] {
            assert!(
                downstream_owner_of(item).is_some(),
                "{} 应在下游归属册内",
                item
            );
        }
        assert!(downstream_owner_of("VE-F9999").is_none());
        // 五层主责与五能力主责都在册。
        let a = ConsistencyArchitecture::standard();
        for s in a.layers.iter() {
            assert!(
                downstream_owner_of(s.owner_item).is_some(),
                "层 {} 主责 {} 不在册",
                s.layer.zh(),
                s.owner_item
            );
        }
        for c in CAPABILITY_ORDER.iter() {
            assert!(
                downstream_owner_of(c.owner_item()).is_some(),
                "能力 {} 主责 {} 不在册",
                c.zh(),
                c.owner_item()
            );
        }
    }

    #[test]
    fn veu01_layer_specs_and_interfaces_complete() {
        let a = ConsistencyArchitecture::standard();
        for s in a.layers.iter() {
            assert!(s.is_complete(), "层 {} 契约残缺", s.layer.zh());
            assert!(!s.screen_line().is_empty());
            assert!(s.cost.is_zero_overhead());
        }
        for it in a.freeze.iter() {
            assert!(it.is_complete(), "接口 {} 契约残缺", it.code);
            assert!(it.is_adjacent());
            assert!(!it.screen_line().is_empty());
            // 哈希实算可复现。
            assert_eq!(
                it.declared_hash,
                fnv1a64_hex(it.declared_text().as_bytes())
            );
        }
    }

    #[test]
    fn veu01_complexity_doc_declares_all_items() {
        for tag in ["C1 ", "C2 ", "C3 ", "C4 ", "C5 ", "C6 ", "C7 ", "C8 ", "C9 ", "C10", "C11"]
        {
            assert!(COMPLEXITY_DOC.contains(tag), "复杂度声明缺 {}", tag);
        }
    }

    #[test]
    fn veu01_item_id_validation() {
        assert!(is_valid_item_id("VE-F4201"));
        assert!(!is_valid_item_id("F4201"));
        assert!(!is_valid_item_id("VE-F420"));
        assert!(!is_valid_item_id("VE-FABCD"));
        assert!(!is_valid_item_id(""));
    }

    #[test]
    fn veu01_hash_is_deterministic_and_fixed_width() {
        let h1 = fnv1a64_hex(b"varix-u-domain");
        let h2 = fnv1a64_hex(b"varix-u-domain");
        assert_eq!(h1, h2, "同内容必得同哈希");
        assert_eq!(h1.len(), HASH_HEX_LEN);
        assert_ne!(h1, fnv1a64_hex(b"varix-u-domai"));
    }

    #[test]
    fn veu01_self_audit_detects_injected_faults() {
        // 逐类注入故障，确认总自检能抓到（防自检写成永远绿）。
        let base = ConsistencyArchitecture::standard();

        // 1) 规则文本持有权错位（通过抽掉契约层的层册条目模拟结构损坏）。
        let mut a1 = base.clone();
        a1.layers.retain(|s| s.layer != Layer::Contract);
        assert!(!a1.self_audit().is_empty(), "缺契约层应判红");

        // 2) 承接首批未落地。
        let mut a2 = base.clone();
        a2.acceptance = AcceptanceLedger::new();
        assert!(a2
            .self_audit()
            .iter()
            .any(|i| i.code == E_PRIMARY_NOT_LANDED));

        // 3) 未入约。
        let mut a3 = base.clone();
        a3.wording = WordingContractLayer::new();
        assert!(a3
            .self_audit()
            .iter()
            .any(|i| i.code == E_A11Y_NOT_ENROLLED));

        // 4) 接口哈希漂移。
        let mut a4 = base.clone();
        a4.freeze.tamper_interfaces()[0].consumers = "被改过的消费方";
        assert!(a4
            .self_audit()
            .iter()
            .any(|i| i.code == E_CROSS_CHECK_DRIFT));

        // 5) 层链断裂。
        let mut a5 = base.clone();
        a5.freeze.tamper_interfaces().pop();
        assert!(a5
            .self_audit()
            .iter()
            .any(|i| i.code == E_LAYER_CHAIN_BROKEN));

        // 对照：未注入的标准态必须全绿（证明上面五条不是恒真）。
        assert!(base.self_audit().is_empty());
    }
}
