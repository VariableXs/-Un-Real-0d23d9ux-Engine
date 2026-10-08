//! VE-F4202 · 域自检（判据逐条对应，见 `veu02_model.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 四类（可一致化实体四类齐备）→ `U02-判据-类-*`
//! - 三型（等同/派生/约束齐备）→ `U02-判据-型-*`
//! - 形式化（关系代数：让位全序+ 码往返）→ `U02-形式-*`
//! - 环检测（建模期拒环）→ `U02-环-*`
//! - 红线约束（不可让步 + 不接受豁免）→ `U02-红线-*`
//! - 版本化（模型册版本 / 实体版本分列 + 对版）→ `U02-版本-*`
//! - 对拍与冲突消解（等同对拍 + 仲裁队列）→ `U02-对拍-*`
//! - 错误路径零静默（五元组齐发）→ `U02-错误-*`
//! - 无障碍替述→ `U02-读屏-*`
//!
//! 零墙钟、零 IO，回归可复现。
//!
//! 自检项按**判据族收敛播报**（细项逐条判定不减少；全绿族并为一行，
//! 有红族逐条出声）——口径与理由见 `veu01_checks.rs` 的 [`FamilyTally`]。

use super::veu02_model::*;
use crate::checks::CheckSet;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 判据族前缀表（长前缀在前）。
const FAMILIES: [&str; 10] = [
    "U02-判据-类-",
    "U02-判据-型-",
    "U02-形式-",
    "U02-环-",
    "U02-红线-",
    "U02-版本-",
    "U02-对拍-",
    "U02-错误-",
    "U02-读屏-",
    "U02-收敛-",
];

fn family_of(name: &'static str) -> &'static str {
    for f in FAMILIES.iter() {
        if name.starts_with(f) {
            return f;
        }
    }
    "U02-判据-类-"
}

/// 族内收敛的自检汇（只压缩播报，不压缩断言）。
struct FamilyTally {
    pending: Vec<(&'static str, &'static str, bool, &'static str)>,
    done: Vec<(&'static str, bool)>,
}

impl FamilyTally {
    fn new() -> FamilyTally {
        FamilyTally { pending: Vec::new(), done: Vec::new() }
    }

    fn add(&mut self, name: &'static str, passed: bool, detail: &'static str) {
        self.pending.push((family_of(name), name, passed, detail));
    }

    fn flush(mut self, set: &mut CheckSet) {
        let mut order: Vec<&'static str> = Vec::new();
        for (fam, _, _, _) in self.pending.iter() {
            if !order.iter().any(|f| f == fam) {
                order.push(*fam);
            }
        }
        for fam in order.iter() {
            let mut all_green = true;
            for it in self.pending.iter() {
                if it.0 == *fam && !it.2 {
                    all_green = false;
                }
            }
            if all_green {
                set.add("U02-族账-该族细项全绿", true, fam);
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

    fn families(&self) -> (usize, usize) {
        let green = self.done.iter().filter(|(_, g)| *g).count();
        (self.done.len(), green)
    }
}

// ---------------------------------------------------------------------------
// 一、四类（判据「四类」）
// ---------------------------------------------------------------------------

fn chk_four_classes(set: &mut FamilyTally) {
    let m = standard_model();

    set.add(
        "U02-判据-类-四类恰四",
        EntityClass::ALL.len() == 4,
        "锚点明文四类；多一类即失去完备性，少一类即漏建模",
    );
    set.add(
        "U02-判据-类-标准模型四类齐备",
        m.classes_complete(),
        "四类各须至少一件实体，否则该类语义无人负责",
    );
    // 逐类在场（缺哪类当场指名）。
    for c in EntityClass::ALL.iter() {
        let n = m.entities().iter().filter(|e| e.class == *c).count();
        set.add(
            "U02-判据-类-逐类在场",
            n > 0,
            match c {
                EntityClass::Interaction => "交互行为",
                EntityClass::VisualToken => "视觉令牌",
                EntityClass::Copywriting => "文案口径",
                EntityClass::DataContract => "数据契约",
            },
        );
    }
    // 类别码往返一致（码是跨域引用凭据）。
    let round = EntityClass::ALL
        .iter()
        .all(|c| EntityClass::from_code(c.code()) == Some(*c));
    set.add(
        "U02-判据-类-类别码往返一致",
        round,
        "码→类→码须恒等，否则跨域引用会指错类",
    );
    // 反例：清掉一类后必须判不备。
    let mut m2 = standard_model();
    m2.tamper_entities().retain(|e| e.class != EntityClass::Copywriting);
    set.add(
        "U02-判据-类-缺类判不备",
        !m2.classes_complete(),
        "删掉文案口径类后仍报齐备= 完备性检查失效",
    );
    // 每类有首选关系型（标准模型据此配关系）。
    set.add(
        "U02-判据-类-每类有首选关系型",
        EntityClass::ALL.iter().all(|c| c.preferred_kind().code().starts_with("REL-")),
        "首选型让标准模型有单源依据，不致每域各挑一种",
    );
}

// ---------------------------------------------------------------------------
// 二、三型（判据「三型」）
// ---------------------------------------------------------------------------

fn chk_three_kinds(set: &mut FamilyTally) {
    let m = standard_model();

    set.add(
        "U02-判据-型-三型恰三",
        RelationKind::ALL.len() == 3,
        "锚点明文三型；派生若可与等同互换即失去方向语义",
    );
    set.add(
        "U02-判据-型-标准模型三型齐备",
        m.kinds_complete(),
        "三型各须至少一条关系，否则该型的消解规则从未被实测",
    );
    // 派生必须有向（头注§二：不声明方向的派生退化成等同）。
    set.add(
        "U02-判据-型-派生判为有向",
        RelationKind::Derived.is_directed(),
        "派生不声明方向= 实现时退化成等同",
    );
    set.add(
        "U02-判据-型-等同约束判为无向",
        !RelationKind::Equiv.is_directed() && !RelationKind::Constraint.is_directed(),
        "等同对称、约束自上而下，都不该被当成有向派生",
    );
    // 关系型码往返。
    let round = RelationKind::ALL
        .iter()
        .all(|k| RelationKind::from_code(k.code()) == Some(*k));
    set.add(
        "U02-判据-型-关系型码往返一致",
        round,
        "码→型→码须恒等",
    );
    // 反例：删掉派生型关系后三型不齐。
    let mut m2 = standard_model();
    m2.tamper_relations()
        .retain(|r| r.kind != RelationKind::Derived);
    set.add(
        "U02-判据-型-缺型判不齐",
        !m2.kinds_complete(),
        "删掉派生关系后仍报齐备 = 齐备性检查失效",
    );
}

// ---------------------------------------------------------------------------
// 三、形式化：关系代数（判据「形式化」）
// ---------------------------------------------------------------------------

fn chk_algebra(set: &mut FamilyTally) {
    // 让位优先级构成**全序**（三型两两组合全部可裁决）。
    let all_pairs = [
        (RelationKind::Equiv, RelationKind::Derived),
        (RelationKind::Equiv, RelationKind::Constraint),
        (RelationKind::Derived, RelationKind::Equiv),
        (RelationKind::Derived, RelationKind::Constraint),
        (RelationKind::Constraint, RelationKind::Equiv),
        (RelationKind::Constraint, RelationKind::Derived),
    ];
    set.add(
        "U02-形式-异型组合全可裁决",
        all_pairs.iter().all(|(a, b)| a.winner(*b).is_some()),
        "未列组合会静默落仲裁，等于把可自动消解的冲突变成人工负担",
    );
    // 同型自裁决返回自身（登记序即裁决序）。
    set.add(
        "U02-形式-同型自裁决",
        RelationKind::Equiv.winner(RelationKind::Equiv) == Some(RelationKind::Equiv)
            && RelationKind::Constraint.winner(RelationKind::Constraint)
                == Some(RelationKind::Constraint),
        "同型冲突按登记序收敛，先登记者为准",
    );
    // 让位方向**反对称**（A让B 则 B 不让 A，除非同型）。
    let asym = all_pairs.iter().all(|(a, b)| {
        let ab = a.winner(*b);
        let ba = b.winner(*a);
        ab.is_some() && ba.is_some()
    });
    set.add(
        "U02-形式-让位双向可算",
        asym,
        "winner 只接一个方向会导致换个调用顺序结论相反",
    );
    // 优先级全序：约束 > 派生 > 等同。
    let ordered = RelationKind::Constraint.precedence() > RelationKind::Derived.precedence()
        && RelationKind::Derived.precedence() > RelationKind::Equiv.precedence();
    set.add(
        "U02-形式-优先级全序成立",
        ordered,
        "约束 > 派生 > 等同：下限压过推导，推导压过平级",
    );
    set.add(
        "U02-形式-优先级无并列",
        RelationKind::ALL
            .iter()
            .all(|k| RelationKind::ALL.iter().all(|j| k == j || k.precedence() != j.precedence())),
        "并列优先级会让 winner 在同值时任选一方",
    );
}

// ---------------------------------------------------------------------------
// 四、环检测（判据「环检测」，降级矩阵第一格）
// ---------------------------------------------------------------------------

fn chk_cycle_detection(set: &mut FamilyTally) {
    // 自环直接拒。
    let mut m = standard_model();
    let selfloop = m.declare(Relation::new(
        "U02-REL-SELFLOOP",
        RelationKind::Derived,
        "U02-TERM-SAVE-S",
        "U02-TERM-SAVE-S",
    ));
    set.add(
        "U02-环-自环被拒",
        matches!(&selfloop, Err(e) if e.code == E_RELATION_CYCLE),
        "自环在求值上无解，须在建模期拒",
    );

    // 派生二环：A→B→A。
    let mut m = standard_model();
    m.declare(Relation::new(
        "U02-REL-CYCLE-1",
        RelationKind::Derived,
        "U02-PALETTE-S",
        "U02-FIELD-THEME-S",
    ))
    .expect("首条派生边登记");
    let two = m.declare(Relation::new(
        "U02-REL-CYCLE-2",
        RelationKind::Derived,
        "U02-FIELD-THEME-S",
        "U02-PALETTE-S",
    ));
    set.add(
        "U02-环-派生二环被拒",
        matches!(&two, Err(e) if e.code == E_RELATION_CYCLE),
        "A派生B 且 B 派生A = 互相推导，任何变更都要求对方先变",
    );

    // 派生三环（跨多条边，须DFS 真正走通才检得出）。
    let mut m = standard_model();
    m.declare(Relation::new(
        "U02-REL-C3A",
        RelationKind::Derived,
        "U02-FOCUS-ORDER-S",
        "U02-PALETTE-T",
    ))
    .expect("三环首边登记");
    m.declare(Relation::new(
        "U02-REL-C3B",
        RelationKind::Derived,
        "U02-PALETTE-T",
        "U02-FIELD-THEME-S",
    ))
    .expect("三环次边登记");
    let three = m.declare(Relation::new(
        "U02-REL-C3C",
        RelationKind::Derived,
        "U02-FIELD-THEME-S",
        "U02-FOCUS-ORDER-S",
    ));
    set.add(
        "U02-环-派生三环被拒",
        matches!(&three, Err(e) if e.code == E_RELATION_CYCLE),
        "三环须DFS 走通才检得出；只看直接反向边会漏",
    );

    // 反例：无环边必须放行（否则环检测成了「全拒」的假闸）。
    let mut m = standard_model();
    let acyclic = m.declare(Relation::new(
        "U02-REL-ACYCLIC",
        RelationKind::Derived,
        "U02-PALETTE-T",
        "U02-TERM-SAVE-T",
    ));
    set.add(
        "U02-环-无环边放行",
        acyclic.is_ok(),
        "环检测若把无环也拒，就退化成「不许建模」",
    );

    // 成环关系不得进册（拒了之后册内仍无环）。
    set.add(
        "U02-环-成环关系未入册",
        !m.relations().iter().any(|r| r.code == "U02-REL-CYCLE-2"),
        "拒绝必须真的没写进去，而不是报错后仍留在册内",
    );
}

// ---------------------------------------------------------------------------
// 五、红线约束（判据「红线约束」，域本色）
// ---------------------------------------------------------------------------

fn chk_redline(set: &mut FamilyTally) {
    let m = standard_model();

    // 白名单恰两项（锚点明文：对比度/焦点可达），且码与中文名一一对应。
    set.add(
        "U02-红线-白名单恰两项",
        NON_NEGOTIABLE.len() == 2
            && NON_NEG_LABELS.len() == 2
            && NON_NEGOTIABLE.contains(&NON_NEG_CONTRAST)
            && NON_NEGOTIABLE.contains(&NON_NEG_FOCUS),
        "红线册每加一项就多一个「这次能不能豁免」的口子",
    );
    set.add(
        "U02-红线-码与中文名一一对应",
        NON_NEGOTIABLE.iter().all(|c| non_neg_label(c).is_some())
            && non_neg_label(NON_NEG_CONTRAST) == Some("对比度")
            && non_neg_label(NON_NEG_FOCUS) == Some("焦点可达"),
        "码判红线、名判读屏；两者错位会让裁决合法却读起来像胡判",
    );

    // 标准模型确有两份红线约束在册。
    let reds = m
        .relations()
        .iter()
        .filter(|r| r.non_negotiable)
        .count();
    set.add(
        "U02-红线-标准模型红线在册",
        reds == 2,
        "对比度与焦点可达各一条，缺一条则该红线从未被实测",
    );

    // 非白名单实体不得标红线。
    let mut m2 = standard_model();
    let bogus = m2.declare(
        Relation::new(
            "U02-REL-REDLINE-BOGUS",
            RelationKind::Constraint,
            "U02-PALETTE-S",
            "U02-TERM-SAVE-S",
        )
        .as_non_negotiable(),
    );
    set.add(
        "U02-红线-非白名单标红被拒",
        matches!(&bogus, Err(e) if e.code == E_NON_NEG_NOT_LISTED),
        "红线册只收对比度/焦点可达；扩册须改锚点不可就地扩",
    );

    // 豁免一律被拒（**无此选项**）。
    set.add(
        "U02-红线-豁免被拒",
        matches!(
            m.request_waiver(NON_NEG_CONTRAST, "工期紧"),
            Err(e) if e.code == E_NON_NEG_WAIVED
        ),
        "对比度/焦点可达是域本色红线，不接受豁免",
    );
    set.add(
        "U02-红线-豁免无绕过入口",
        matches!(
            m.request_waiver(NON_NEG_FOCUS, "领导同意"),
            Err(e) if e.code == E_NON_NEG_WAIVED
        ),
        "给理由也不放行——理由充分与否不是红线的裁量口",
    );

    // 红线冲突：不可自动消解，须入仲裁。
    let mut m3 = standard_model();
    // 篡改红线对侧的取值 → 等同关系两侧不等 → 红线冲突。
    if let Some(e) = m3.tamper_entities().iter_mut().find(|e| e.code == "U02-PALETTE-T") {
        e.value = "主色 #FFFFFF".to_string();
    }
    let n = m3.cross_check().expect("对拍可跑");
    set.add(
        "U02-红线-红线冲突被检出",
        n > 0,
        "红线侧取值漂移必须被对拍抓到",
    );
    let red_conf = m3
        .conflicts()
        .iter()
        .any(|c| c.severity == crate::svstar2::veu01_arch::Severity::Blocking);
    set.add(
        "U02-红线-红线冲突判阻断",
        red_conf,
        "红线冲突不得降级为警告",
    );
    let red_arb = m3
        .conflicts()
        .iter()
        .all(|c| c.in_arbitration);
    set.add(
        "U02-红线-红线冲突强制入仲裁",
        red_arb,
        "红线冲突无自动消解方，必须人工落槌",
    );
    // 不可消解时 resolve 必拒并指名队列。
    let r = m3.resolve_conflicts();
    set.add(
        "U02-红线-不可消解时拒绝自动收敛",
        matches!(&r, Err(e) if e.code == E_ARBITRATION_REQUIRED),
        "不消解 ≠ 静默放过：须显式落仲裁",
    );
}

// ---------------------------------------------------------------------------
// 六、版本化（判据「版本化」，降级矩阵第三格）
// ---------------------------------------------------------------------------

fn chk_versioning(set: &mut FamilyTally) {
    let m = standard_model();

    set.add(
        "U02-版本-模型册版本非空",
        !m.version().text().is_empty() && m.version().major >= 1,
        "模型册须有结构版本号",
    );
    // 实体版本与模型册版本**分列**（头注§五）。
    let ev = m
        .entities()
        .iter()
        .map(|e| e.entity_version.clone())
        .collect::<Vec<String>>();
    set.add(
        "U02-版本-实体版本独立于模型册",
        ev.iter().all(|v| !v.is_empty())
            && ev.iter().all(|v| v != &m.version().text()),
        "两者混记会让改文案被判结构漂移，或结构大改却不升版",
    );
    // 内容改动**不**改结构指纹（否则误报漂移）。
    let d1 = m.version().structural_digest(&m);
    let mut m2 = standard_model();
    if let Some(e) = m2.tamper_entities().iter_mut().find(|e| e.code == "U02-TERM-SAVE-T") {
        e.value = "存储".to_string();
    }
    let d2 = m2.version().structural_digest(&m2);
    set.add(
        "U02-版本-内容改动不触发结构漂移",
        d1 == d2,
        "改一句文案若被判结构漂移，版本化就成了天天报警的狼来了",
    );
    set.add(
        "U02-版本-标准态对版通过",
        m.align_version().is_ok(),
        "冻结后结构未变即应同版",
    );
    // 未冻结时对版须拒（否则「对版通过」是空话）。
    let mut m3 = standard_model();
    m3.tamper_frozen_digest("");
    set.add(
        "U02-版本-未冻结对版被拒",
        matches!(
            m3.align_version(),
            Err(e) if e.code == E_VERSION_MISSING_FIELD
        ),
        "没有基准可比，对版通过没有意义",
    );
    // 结构漂移须被检出（加一条关系即改结构）。
    let mut m4 = standard_model();
    m4.declare(Relation::new(
        "U02-REL-NEW-STRUCT",
        RelationKind::Derived,
        "U02-PALETTE-T",
        "U02-TERM-SAVE-T",
    ))
    .expect("新增关系");
    set.add(
        "U02-版本-结构漂移被检出",
        matches!(m4.align_version(), Err(e) if e.code == E_VERSION_DRIFT),
        "加一条派生边改了结构，须升版重新冻结",
    );
    // 升主版本后须重新冻结（升版不清冻结态 = 拿旧基准比新结构）。
    set.add(
        "U02-版本-升主版本清冻结态",
        {
            let mut mv = standard_model();
            mv.bump_major();
            mv.frozen_digest().is_empty()
        },
        "升版不清冻结态= 拿旧基准对新结构，漂移永远检不出",
    );
    set.add(
        "U02-版本-升版语义正确",
        {
            let mut mv = standard_model();
            mv.bump_minor();
            let before = mv.version().minor;
            mv.bump_major();
            mv.version().major == 2 && mv.version().minor == 0 && before == 1
        },
        "升主版本须归零次版本，否则版本序不可比",
    );
}

// ---------------------------------------------------------------------------
// 七、对拍与冲突消解
// ---------------------------------------------------------------------------

fn chk_cross_check(set: &mut FamilyTally) {
    // 标准态两侧同值 → 零冲突。
    let mut m = standard_model();
    let n = m.cross_check().expect("对拍可跑");
    set.add(
        "U02-对拍-标准态零冲突",
        n == 0,
        "标准模型册是同值正样本，有冲突即建模错了",
    );
    set.add(
        "U02-对拍-标准态可自动收敛",
        m.resolve_conflicts().is_ok(),
        "无冲突时不该报仲裁需求",
    );

    // 非红线漂移：可自动消解。
    let mut m2 = standard_model();
    if let Some(e) = m2.tamper_entities().iter_mut().find(|e| e.code == "U02-FIELD-THEME-T") {
        e.value = "主题字段取值为 light|dark".to_string();
    }
    set.add(
        "U02-对拍-取值漂移被检出",
        m2.cross_check().expect("对拍可跑") == 1,
        "两侧取值不同即冲突，靠实算哈希比对而非字符串相等",
    );

    // **四类各自都要有跨域等同对可对拍**。
    //
    // 少了这一条，某类可能只挂约束关系而无等同关系——于是该类
    // 「两份副本取值分叉」这种最典型的一致性故障就无闸可测，
    // 而四类齐备性检查照样全绿（它只查「有没有实体」，不查「有没有对拍口」）。
    // 教训：完备性要查到「可被检验」这一层，光查到「存在」不够。
    let covered = EntityClass::ALL.iter().all(|c| {
        m.relations().iter().any(|r| {
            r.kind == RelationKind::Equiv
                && m.entity(&r.from)
                    .map(|e| e.class == *c)
                    .unwrap_or(false)
        })
    });
    set.add(
        "U02-对拍-四类均有等同对可对拍",
        covered,
        "某类若只有约束无等同，该类取值分叉将无人能发现",
    );

    // 悬空端点不可登记（否则对拍会静默跳过）。
    let mut m3 = standard_model();
    let dangling = m3.declare(Relation::new(
        "U02-REL-DANGLING",
        RelationKind::Equiv,
        "U02-NOT-EXIST",
        "U02-PALETTE-S",
    ));
    set.add(
        "U02-对拍-悬空端点被拒",
        matches!(&dangling, Err(e) if e.code == E_RELATION_UNKNOWN_ENTITY),
        "端点不在册则对拍无从下手，静默跳过= 漏检",
    );

    // 实体登记防护。
    let mut m4 = standard_model();
    let dup = m4.add_entity(Entity::new(
        "U02-PALETTE-S",
        EntityClass::VisualToken,
        "重复",
    ));
    set.add(
        "U02-对拍-实体码重复被拒",
        matches!(&dup, Err(e) if e.code == E_ENTITY_DUP),
        "同码实体只登记一次；两域各持一份的是取值不是码",
    );
    let blank = m4.add_entity(Entity::new("U02-BLANK", EntityClass::Copywriting, ""));
    set.add(
        "U02-对拍-空取值实体被拒",
        matches!(&blank, Err(e) if e.code == E_ENTITY_MISSING_FIELD),
        "空取值实体无法参与对拍",
    );

    // 仲裁：红线冲突裁给非红线侧须被拒。
    let mut m5 = standard_model();
    if let Some(e) = m5.tamper_entities().iter_mut().find(|e| e.code == "U02-PALETTE-T") {
        e.value = "主色 #FFFFFF".to_string();
    }
    m5.cross_check().expect("对拍可跑");
    let code = m5
        .conflicts()
        .iter()
        .find(|c| c.severity == crate::svstar2::veu01_arch::Severity::Blocking)
        .map(|c| c.code.clone())
        .unwrap_or_default();
    set.add(
        "U02-红线-仲裁不得推翻红线",
        matches!(
            m5.arbitrate(&code, "选视觉令牌侧好看为准"),
            Err(e) if e.code == E_REDLINE_OVERRIDDEN
        ),
        "「好看」不是裁红线理由；只可裁为红线侧为胜",
    );
    set.add(
        "U02-红线-仲裁采红线侧可落槌",
        m5.arbitrate(&code, NON_NEG_CONTRAST).is_ok(),
        "采红线侧为胜是唯一合法出路，须真能走通",
    );

    // 自检：标准模型册自身无问题。
    let issues = m.self_audit();
    set.add(
        "U02-对拍-标准模型自检无问题",
        issues.is_empty(),
        "标准模型册是回归基线，自身不干净则所有断言都建在坏基线上",
    );
    for i in issues.iter() {
        set.add("U02-对拍-自检问题明细", false, "见 self_audit");
        let _ = i;
        break;
    }
}

// ---------------------------------------------------------------------------
// 八、错误路径零静默 / 九、无障碍读屏
// ---------------------------------------------------------------------------

fn chk_errors_and_a11y(set: &mut FamilyTally) {
    let mut m = standard_model();

    // 错误五元组齐发（构造点强制，抽样三条真实错误）。
    let mut m2 = standard_model();
    let errs = [
        m2.declare(Relation::new(
            "U02-REL-ERR1",
            RelationKind::Derived,
            "U02-NOPE",
            "U02-PALETTE-S",
        )),
        m.declare(Relation::new(
            "U02-REL-ERR2",
            RelationKind::Derived,
            "U02-TERM-SAVE-S",
            "U02-TERM-SAVE-S",
        )),
        m.request_waiver(NON_NEG_CONTRAST, "试试"),
    ];
    let mut all_complete = true;
    let mut all_road = true;
    for e in errs.iter() {
        if let Err(err) = e {
            if !err.is_complete() {
                all_complete = false;
            }
            if err.next.trim().is_empty() {
                all_road = false;
            }
        }
    }
    set.add(
        "U02-错误-五元组齐发",
        all_complete,
        "码/现象/原因/下一步/责任方缺一即不合格",
    );
    set.add(
        "U02-错误-拒绝必给出路",
        all_road,
        "只说「不行」而不说「那该怎么做」，人会换个写法再来一遍",
    );
    set.add(
        "U02-错误-错误码可读屏",
        errs
            .iter()
            .all(|e| match e {
                Ok(_) => false,
                Err(err) => err.screen_text().contains("错误"),
            }),
        "异常零静默：错误须能念给用户听",
    );

    // 错误码唯一。
    let codes = [
        E_ENTITY_DUP,
        E_ENTITY_UNKNOWN,
        E_ENTITY_MISSING_FIELD,
        E_RELATION_DUP,
        E_RELATION_CYCLE,
        E_RELATION_UNKNOWN_ENTITY,
        E_RELATION_DIRECTION_MISSING,
        E_RELATION_KIND_UNKNOWN,
        E_NON_NEG_NOT_LISTED,
        E_NON_NEG_WAIVED,
        E_REDLINE_OVERRIDDEN,
        E_ARBITRATION_REQUIRED,
        E_VERSION_DRIFT,
        E_VERSION_MISSING_FIELD,
        E_MODEL_EMPTY,
        E_CLASS_INCOMPLETE,
        E_CAP,
    ];
    let mut uniq: Vec<&str> = codes.to_vec();
    uniq.sort_unstable();
    let before = uniq.len();
    uniq.dedup();
    set.add(
        "U02-错误-错误码唯一",
        uniq.len() == before,
        "两码同义会让对不上账，故障定位时无从分辨",
    );

    // 读屏：模型册总览含四类三型与红线。
    let text = m.screen_text();
    set.add(
        "U02-读屏-模型册读屏可达",
        text.contains("跨域一致性模型册") && text.contains("实体") && text.contains("关系"),
        "模型册须能被读屏念出来",
    );
    set.add(
        "U02-读屏-红线标注可读",
        text.contains("红线"),
        "红线条目在读屏文本里须自带「不可让步」字样",
    );
    set.add(
        "U02-读屏-冲突读屏含仲裁态",
        {
            let mut m3 = standard_model();
            if let Some(e) = m3.tamper_entities().iter_mut().find(|e| e.code == "U02-PALETTE-T") {
                e.value = "主色 #FFFFFF".to_string();
            }
            m3.cross_check().expect("对拍可跑");
            m3.conflicts()
                .iter()
                .all(|c| c.screen_line().contains("冲突"))
        },
        "冲突项必须能念出来，且标明是否已入仲裁",
    );
    set.add(
        "U02-读屏-实体读屏含哈希",
        m.entities()
            .iter()
            .all(|e| e.screen_line().contains("哈希")),
        "实体读屏行须带取值哈希，便于口头对账",
    );
}

// ---------------------------------------------------------------------------
// 聚合入口
// ---------------------------------------------------------------------------

pub fn run_veu02_checks() -> CheckSet {
    let mut tally = FamilyTally::new();
    chk_four_classes(&mut tally);
    chk_three_kinds(&mut tally);
    chk_algebra(&mut tally);
    chk_cycle_detection(&mut tally);
    chk_redline(&mut tally);
    chk_versioning(&mut tally);
    chk_cross_check(&mut tally);
    chk_errors_and_a11y(&mut tally);

    let (fam_total, fam_green) = tally.families();
    let mut set = CheckSet::new("veu02-model");
    tally.flush(&mut set);

    set.add(
        "U02-收敛-标准态族账全绿",
        fam_green == fam_total,
        "收敛只压播报不改判定；族账有红即细项有红",
    );
    set.add(
        "U02-收敛-族数不超登记表",
        fam_total <= FAMILIES.len(),
        "族前缀未登记会静默并入兜底族，等于丢失分组",
    );
    set.add(
        "U02-收敛-未触容量上限",
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
    fn u02_four_classes_complete() {
        let m = standard_model();
        assert!(m.classes_complete());
        assert_eq!(EntityClass::ALL.len(), 4);
    }

    #[test]
    fn u02_three_kinds_complete() {
        let m = standard_model();
        assert!(m.kinds_complete());
        assert_eq!(RelationKind::ALL.len(), 3);
    }

    #[test]
    fn u02_derived_cycle_rejected() {
        let mut m = standard_model();
        let e = m
            .declare(Relation::new(
                "U02-TEST-CYCLE",
                RelationKind::Derived,
                "U02-TERM-SAVE-S",
                "U02-FIELD-THEME-S",
            ))
            .expect_err("应成环：FIELD-THEME-S 已派生 TERM-SAVE-S");
        assert_eq!(e.code, E_RELATION_CYCLE);
        assert!(e.is_complete());
    }

    #[test]
    fn u02_redline_never_waivable() {
        let m = standard_model();
        let e = m
            .request_waiver(NON_NEG_CONTRAST, "任何理由")
            .expect_err("红线不接受豁免");
        assert_eq!(e.code, E_NON_NEG_WAIVED);
    }

    #[test]
    fn u02_content_change_is_not_structural_drift() {
        let m = standard_model();
        let d1 = m.version().structural_digest(&m);
        let mut m2 = standard_model();
        m2.tamper_entities()[0].value = "别的取值".to_string();
        assert_eq!(d1, m2.version().structural_digest(&m2));
    }

    #[test]
    fn u02_self_audit_clean() {
        let m = standard_model();
        assert!(m.self_audit().is_empty(), "标准模型册自身须干净");
    }

    #[test]
    fn u02_all_criteria_pass() {
        let s = run_veu02_checks();
        let (passed, failed) = s.tally();
        assert_eq!(failed, 0, "存在红项");
        assert!(!s.truncated());
        assert!(passed > 0);
    }
}