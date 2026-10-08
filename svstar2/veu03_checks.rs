//! VE-F4203 · 域自检（判据逐条对应，见 `veu03_registry.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 单一事实源（同 ID 同版本唯一 + 归并建议）→ `U03-单一-*`
//! - 五字段冻结 → `U03-元模-*`
//! - 唯一性（键内 + 版本谱系）→ `U03-唯一-*`
//! - 引用计数（记账/撤订/催办）→ `U03-引用-*`
//! - 判据必填 → `U03-判据-*`
//! - 生命周期（合法迁移/非法拒+留痕/废止闸）→ `U03-生命-*`
//! - 四能力齐备 → `U03-能力-*`
//! - 错误路径零静默 → `U03-错误-*`
//!
//! 零墙钟、零 IO，回归可复现。
//!
//! 自检项按**判据族收敛播报**（细项逐条判定不减少；全绿族并为一行，
//! 有红族逐条出声）——口径与理由见 `veu01_checks.rs` 的 `FamilyTally`。

use super::veu02_model::DomainTag;
use super::veu03_registry::*;
use crate::checks::CheckSet;

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 判据族前缀表（长前缀在前）。
const FAMILIES: [&str; 9] = [
    "U03-单一-",
    "U03-元模-",
    "U03-唯一-",
    "U03-引用-",
    "U03-判据-",
    "U03-生命-",
    "U03-能力-",
    "U03-错误-",
    "U03-收敛-",
];

fn family_of(name: &'static str) -> &'static str {
    for f in FAMILIES.iter() {
        if name.starts_with(f) {
            return f;
        }
    }
    "U03-能力-"
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
                set.add("U03-族账-该族细项全绿", true, fam);
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

/// 一条合格判据（省去每处重复长串）。
fn ok_criteria() -> String {
    "本契约保证行为可被验证，且无障碍判据不可省".to_string()
}

/// 一条完整元模型（省去每处重复五字段）。
fn meta(id: &str, version: &str, provider: DomainTag, cons: Vec<DomainTag>) -> ContractMeta {
    ContractMeta::new(id, version, provider, cons, &ok_criteria())
}

// ---------------------------------------------------------------------------
// 一、单一事实源（判据「单一事实源」）
// ---------------------------------------------------------------------------

fn chk_single_source(set: &mut FamilyTally) {
    let mut r = standard_registry();

    // 同 ID 同版本二次注册 → 拒绝（**不许覆盖**，见头注§一）。
    let dup = r.register(meta(
        "U03-CTR-FOCUS",
        "v1",
        DomainTag::T,
        vec![DomainTag::U],
    ));
    set.add(
        "U03-单一-同键二次注册被拒",
        matches!(&dup, Err(e) if e.code == E_CONTRACT_DUP),
        "覆盖会让已按旧版编译的下游在无通知下换掉契约语义",
    );

    // 拒绝时**册内原条目未被改动**（拒绝路径不得改状态）。
    set.add(
        "U03-单一-被拒不改册内状态",
        r.entries().len() == 2
            && r.lookup("U03-CTR-FOCUS", "v1")
                .map(|e| e.meta.provider == DomainTag::S)
                .unwrap_or(false),
        "拒绝若顺手覆盖了原条目，唯一性就只剩报错动作没有实际约束力",
    );

    // 归并建议必须给全三条出路（锚点降级矩阵第一格明文「拒绝+归并建议」）。
    let advice_text = match &dup {
        Err(e) => e.screen_text(),
        Ok(_) => String::new(),
    };
    set.add(
        "U03-单一-拒绝给出归并建议",
        advice_text.contains("归并建议") && advice_text.contains("升版"),
        "只说「已存在」而不说「那该怎么办」，调用方会换个写法再来一遍",
    );
    set.add(
        "U03-单一-归并建议含消费方路径",
        advice_text.contains("add_consumer"),
        "多数重复其实是「只是消费方不同」，须直接给出该走的那个口",
    );

    // 反向对照：合法新版本**必须能注册**（否则唯一性成了「不许更新」）。
    set.add(
        "U03-单一-新版本可注册",
        r.register(meta(
            "U03-CTR-FOCUS",
            "v2",
            DomainTag::S,
            vec![DomainTag::T, DomainTag::U],
        ))
        .is_ok(),
        "唯一性键是 (ID,版本)；若按 ID 判则版本机制形同虚设",
    );
}

// ---------------------------------------------------------------------------
// 二、五字段冻结（判据「五字段冻结」）
// ---------------------------------------------------------------------------

fn chk_meta_fields(set: &mut FamilyTally) {
    set.add(
        "U03-元模-五字段恰五",
        ContractRegistry::fields_frozen() && META_FIELDS.len() == 5,
        "锚点明文五字段；多一项（如备注）就留下「同名不同内容」的后门",
    );
    set.add(
        "U03-元模-字段清单锚点对齐",
        META_FIELDS[0] == "id"
            && META_FIELDS[1] == "version"
            && META_FIELDS[2] == "provider"
            && META_FIELDS[3] == "consumers"
            && META_FIELDS[4] == "criteria",
        "ID/版本/提供方/消费方/判据——顺序与命名均为契约，不可漂移",
    );

    // 残缺元模型被拒，且**指名缺哪个字段**。
    let mut r = standard_registry();
    let no_cons = r.register(ContractMeta::new(
        "U03-CTR-NOCONS",
        "v1",
        DomainTag::S,
        vec![],
        &ok_criteria(),
    ));
    set.add(
        "U03-元模-残缺元模型被拒",
        matches!(&no_cons, Err(e) if e.code == E_META_INCOMPLETE),
        "五字段缺一不得入册",
    );
    let msg = match &no_cons {
        Err(e) => e.screen_text(),
        Ok(_) => String::new(),
    };
    set.add(
        "U03-元模-指名缺失字段",
        msg.contains("consumers"),
        "只说「不完整」等于让人自己猜哪个字段错了",
    );

    // 指纹：消费方**顺序不同不算另一份契约**。
    let a = meta("U03-X", "v1", DomainTag::S, vec![DomainTag::T, DomainTag::U]);
    let b = meta("U03-X", "v1", DomainTag::S, vec![DomainTag::U, DomainTag::T]);
    set.add(
        "U03-元模-消费方顺序不影响指纹",
        a.digest() == b.digest(),
        "消费方是集合不是序列；顺序敏感会让同一份契约算出两个指纹",
    );
    // 但判据不同必须算出不同指纹。
    let c = ContractMeta::new(
        "U03-X",
        "v1",
        DomainTag::S,
        vec![DomainTag::T, DomainTag::U],
        "另一套判据",
    );
    set.add(
        "U03-元模-判据不同指纹不同",
        a.digest() != c.digest(),
        "指纹须覆盖判据，否则换判据不改指纹= 同一份契约可被静默替换",
    );
}

// ---------------------------------------------------------------------------
// 三、唯一性（键内唯一 + 版本谱系）
// ---------------------------------------------------------------------------

fn chk_uniqueness(set: &mut FamilyTally) {
    let mut r = standard_registry();
    set.add(
        "U03-唯一-标准册两两无重复",
        {
            let mut ids: Vec<(String, String)> = r
                .entries()
                .iter()
                .map(|e| (e.meta.id.clone(), e.meta.version.clone()))
                .collect();
            let n = ids.len();
            ids.sort();
            ids.dedup();
            ids.len() == n
        },
        "同 ID 同版本在册只能一条",
    );
    // 版本谱系纵深防御：绕过 register 直接调谱系检查也应拒。
    set.add(
        "U03-唯一-谱系检查纵深防御",
        matches!(
            r.check_version_lineage(&meta("U03-CTR-FOCUS", "v1", DomainTag::S, vec![DomainTag::U])),
            Err(e) if e.code == E_VERSION_LINEAGE_DUP
        ),
        "唯一性不能只靠一个入口拦；纵深防御防旁路",
    );
    // 多版本共存合法。
    set.add(
        "U03-唯一-多版本共存",
        {
            r.register(meta("U03-CTR-THEME", "v2", DomainTag::T, vec![DomainTag::U]))
                .is_ok()
                && r.versions_of("U03-CTR-THEME").len() == 2
        },
        "同 ID 多版本共存是版本机制的正常形态，不该被当重复",
    );
    // 当前版本取最大。
    set.add(
        "U03-唯一-当前版本取最新",
        r.current("U03-CTR-THEME")
            .map(|e| e.meta.version == "v2")
            .unwrap_or(false),
        "「当前版本」须有确定口径，否则各处引用可能指向不同版本",
    );
}

// ---------------------------------------------------------------------------
// 四、引用计数（判据「引用计数」）
// ---------------------------------------------------------------------------

fn chk_reference_counting(set: &mut FamilyTally) {
    let mut r = standard_registry();

    set.add(
        "U03-引用-初始计数正确",
        r.ref_count("U03-CTR-FOCUS", "v1") == 2,
        "FOCUS 有 T/U 两个消费方",
    );
    // 重复引用被拒（否则计数虚高、永不归零）。
    set.add(
        "U03-引用-重复引用被拒",
        matches!(
            r.add_consumer("U03-CTR-FOCUS", "v1", DomainTag::T),
            Err(e) if e.code == E_REF_ALREADY
        ),
        "同一消费方对同一契约只能一条有效引用",
    );
    set.add(
        "U03-引用-被拒后计数不变",
        r.ref_count("U03-CTR-FOCUS", "v1") == 2,
        "拒绝若仍加计数，则每次重试都让计数虚高",
    );
    // 撤订只标记不删行（头注§三）。
    let n = r
        .deregister("U03-CTR-FOCUS", "v1", DomainTag::T)
        .expect("撤订应成功");
    set.add(
        "U03-引用-撤订后计数递减",
        n == 1 && r.ref_count("U03-CTR-FOCUS", "v1") == 1,
        "撤订须真实递减，否则引用数永不归零",
    );
    set.add(
        "U03-引用-撤订留痕不删行",
        r.refs().iter().any(|x| {
            x.id == "U03-CTR-FOCUS" && x.consumer == DomainTag::T && x.revoked
        }),
        "删行则「曾引用过」不可反查，审计断链",
    );
    set.add(
        "U03-引用-有效消费方可查",
        r.consumers("U03-CTR-FOCUS", "v1") == vec![DomainTag::U],
        "引用方名单须在册内可查，否则「还有人用吗」只能全量扫模块声明",
    );
    // 撤订不存在的引用被拒。
    set.add(
        "U03-引用-无引用撤订被拒",
        matches!(
            r.deregister("U03-CTR-FOCUS", "v1", DomainTag::S),
            Err(e) if e.code == E_REF_UNKNOWN
        ),
        "S 是提供方不是消费方；错撤订会把计数带偏",
    );

    // 悬空引用：未声明消费方 → 催办 + 拒（降级矩阵第二格）。
    let mut r2 = standard_registry();
    let dangling = r2.add_consumer("U03-CTR-FOCUS", "v1", DomainTag::S);
    set.add(
        "U03-引用-未声明消费方被拒",
        matches!(&dangling, Err(e) if e.code == E_REF_UNKNOWN),
        "未声明的消费者不能靠引用把自己加进来",
    );
    set.add(
        "U03-引用-悬空引用已催办",
        r2.dunning().iter().any(|d| d.consumer == DomainTag::S),
        "悬空引用须留催办痕迹，否则没人知道要补声明",
    );
    // 催办次数累加不覆盖。
    let _ = r2.add_consumer("U03-CTR-FOCUS", "v1", DomainTag::S);
    set.add(
        "U03-引用-催办次数累加",
        r2.dunning()
            .iter()
            .find(|d| d.consumer == DomainTag::S)
            .map(|d| d.notices >= 2)
            .unwrap_or(false),
        "覆盖次数会让「催了没人管」不可见",
    );
    set.add(
        "U03-引用-催办件读屏可达",
        r2.dunning()
            .iter()
            .all(|d| d.screen_line().contains("催办")),
        "催办项必须能念出来，否则它只是册里一行没人看的字",
    );
}

// ---------------------------------------------------------------------------
// 五、判据必填（判据「判据必填」，域本色）
// ---------------------------------------------------------------------------

fn chk_criteria_required(set: &mut FamilyTally) {
    let mut r = standard_registry();
    let empty = r.register(ContractMeta::new(
        "U03-CTR-NOCRIT",
        "v1",
        DomainTag::S,
        vec![DomainTag::U],
        "",
    ));
    set.add(
        "U03-判据-空判据被拒",
        matches!(&empty, Err(e) if e.code == E_META_CRITERIA_EMPTY),
        "无障碍判据必填位；留到用起来才发现，补判据要动所有消费方",
    );
    let msg = match &empty {
        Err(e) => e.screen_text(),
        Ok(_) => String::new(),
    };
    set.add(
        "U03-判据-拒绝给出补判据句式",
        msg.contains("判据"),
        "必填位被拒时要告诉对方要写成什么样，否则只会再提交一次空的",
    );
    set.add(
        "U03-判据-被拒未入册",
        r.lookup("U03-CTR-NOCRIT", "v1").is_none(),
        "报错不等于没入册；须真的没写进去",
    );
    // 标准册每条判据非空（**基线自身须干净**）。
    set.add(
        "U03-判据-标准册判据齐备",
        r.entries().iter().all(|e| !e.meta.criteria.trim().is_empty()),
        "基线里若有空判据，所有「判据必填」的验证都建在坏基线上",
    );
    set.add(
        "U03-判据-判据字数可读屏",
        r.entries()
            .iter()
            .all(|e| e.meta.screen_line().contains("判据")),
        "读屏行须带判据字数，便于判断是不是敷衍了事",
    );
}

// ---------------------------------------------------------------------------
// 六、生命周期（判据「生命周期」+ 降级矩阵第三格）
// ---------------------------------------------------------------------------

fn chk_lifecycle(set: &mut FamilyTally) {
    let mut r = standard_registry();

    // 合法迁移。
    set.add(
        "U03-生命-注册即已注册态",
        r.lookup("U03-CTR-THEME", "v1")
            .map(|e| e.lifecycle == Lifecycle::Registered)
            .unwrap_or(false),
        "注册成功即进入已注册态",
    );
    set.add(
        "U03-生命-已注册可冻结",
        r.transition("U03-CTR-THEME", Lifecycle::Frozen, "判据齐备")
            .is_ok(),
        "已注册→已冻结是合法迁移",
    );
    // 非法迁移：回退。
    let back = r.transition("U03-CTR-THEME", Lifecycle::Registered, "想回退");
    set.add(
        "U03-生命-回退迁移被拒",
        matches!(&back, Err(e) if e.code == E_LC_ILLEGAL),
        "生命周期单向前进；回退会让「上一态是什么」永远不可推断",
    );
    // 草拟→已冻结被拒（绕过注册入口）。
    set.add(
        "U03-生命-草拟跳级冻结被拒",
        !Lifecycle::Draft.may_move_to(Lifecycle::Frozen),
        "没注册过就冻= 凭空出现一份没人登记过的已冻结契约",
    );
    set.add(
        "U03-生命-废止为终态",
        !Lifecycle::Retired.may_move_to(Lifecycle::Frozen)
            && !Lifecycle::Retired.may_move_to(Lifecycle::Registered),
        "终态不可复活",
    );
    // 迁移表完备性：四态 × 四态 = 16 组合中恰 4 条合法。
    let legal = (0..Lifecycle::ALL.len())
        .flat_map(|i| (0..Lifecycle::ALL.len()).map(move |j| (i, j)))
        .filter(|(i, j)| Lifecycle::ALL[*i].may_move_to(Lifecycle::ALL[*j]))
        .count();
    set.add(
        "U03-生命-合法迁移恰四条",
        legal == 4,
        "草拟→已注册、已注册→已冻结、已注册→已废止、已冻结→已废止",
    );

    // 留痕：被拒的迁移也须留痕（头注§四）。
    set.add(
        "U03-生命-非法迁移已留痕",
        r.traces().iter().any(|t| t.rejected),
        "被拒不留痕 ⇒ 调用方反复重试同一个非法迁移，看起来像没报错也没生效",
    );
    set.add(
        "U03-生命-留痕含合法迁移",
        r.traces().iter().any(|t| !t.rejected),
        "留痕不是只记失败；成功迁移也是审计证据",
    );
    set.add(
        "U03-生命-留痕指名依据",
        r.traces()
            .iter()
            .filter(|t| t.rejected)
            .all(|t| !t.reason.trim().is_empty()),
        "留痕须写明依据，否则留了等于没留",
    );

    // 废止闸：仍有引用不得废止（头注§三）。
    let retire = r.transition("U03-CTR-FOCUS", Lifecycle::Retired, "想废止");
    set.add(
        "U03-生命-有引用废止被拒",
        matches!(&retire, Err(e) if e.code == E_LC_ILLEGAL),
        "仍在用的契约不能被废止，否则消费方第二天才发现",
    );
    set.add(
        "U03-生命-废止拒绝已留痕",
        r.traces()
            .iter()
            .any(|t| t.rejected && t.to == Lifecycle::Retired),
        "废止被拒属审计事件，须可反查是谁想废止",
    );
    // 撤订到零后可废止（**真能走通**，防闸过严）。
    let mut r2 = standard_registry();
    r2.deregister("U03-CTR-FOCUS", "v1", DomainTag::T)
        .expect("撤订");
    r2.deregister("U03-CTR-FOCUS", "v1", DomainTag::U)
        .expect("撤订");
    set.add(
        "U03-生命-引用归零后可废止",
        r2.transition("U03-CTR-FOCUS", Lifecycle::Retired, "引用已清零")
            .is_ok(),
        "闸门若把合法路径也堵死，废止能力形同虚设",
    );
    // 状态码往返。
    set.add(
        "U03-生命-状态码往返一致",
        Lifecycle::ALL
            .iter()
            .all(|l| Lifecycle::from_code(l.code()) == Some(*l)),
        "码是留痕与外部系统的引用凭据",
    );
}

// ---------------------------------------------------------------------------
// 七、四能力齐备 + 错误零静默 + 自检
// ---------------------------------------------------------------------------

fn chk_capabilities_and_errors(set: &mut FamilyTally) {
    let r = standard_registry();

    // 四能力各有实测对象。
    set.add(
        "U03-能力-注册可检索",
        r.lookup("U03-CTR-FOCUS", "v1").is_some(),
        "能力一+二：注册与检索",
    );
    set.add(
        "U03-能力-引用计数可查",
        r.ref_count("U03-CTR-FOCUS", "v1") > 0,
        "能力三：引用计数",
    );
    set.add(
        "U03-能力-生命周期可读",
        r.entries()
            .iter()
            .any(|e| e.lifecycle == Lifecycle::Frozen),
        "能力四：生命周期",
    );

    // 错误五元组齐发（抽样四条真实错误）。
    let mut r2 = standard_registry();
    let errs = [
        r2.register(meta("U03-CTR-FOCUS", "v1", DomainTag::T, vec![DomainTag::U])).err(),
        r2.add_consumer("U03-CTR-NOPE", "v1", DomainTag::U).err(),
        r2.transition("U03-CTR-THEME", Lifecycle::Draft, "回退").err(),
        r2.register(ContractMeta::new(
            "U03-CTR-NC",
            "v1",
            DomainTag::S,
            vec![DomainTag::U],
            "",
        ))
        .err(),
    ];
    let complete = errs.iter().all(|e| match e {
        Some(err) => err.is_complete() && !err.next.trim().is_empty(),
        None => false,
    });
    set.add(
        "U03-错误-五元组齐发",
        complete,
        "码/现象/原因/下一步/责任方缺一即不合格",
    );
    set.add(
        "U03-错误-每类错误都有代表样本",
        errs.iter().all(|e| e.is_some()),
        "抽样须真的报错；若某条路径不报错说明该门禁形同虚设",
    );
    set.add(
        "U03-错误-错误码唯一",
        {
            let codes = [
                E_CONTRACT_DUP,
                E_CONTRACT_UNKNOWN,
                E_META_INCOMPLETE,
                E_META_CRITERIA_EMPTY,
                E_META_FIELDS_DRIFT,
                E_REF_UNKNOWN,
                E_REF_ALREADY,
                E_DEREG_IN_USE,
                E_LC_ILLEGAL,
                E_LC_UNKNOWN,
                E_VERSION_LINEAGE_DUP,
                E_CAP,
            ];
            let mut u = codes.to_vec();
            let n = u.len();
            u.sort_unstable();
            u.dedup();
            u.len() == n
        },
        "两码同义会让对不上账，故障定位时无从分辨",
    );

    // 自检：标准册自身无问题（计数自洽 + 唯一 + 字段冻结）。
    let issues = r.self_audit();
    set.add(
        "U03-能力-标准册自检无问题",
        issues.is_empty(),
        "基线自身不干净则所有判据都建在坏基线上",
    );
    set.add(
        "U03-能力-读屏总览可达",
        r.screen_text().contains("契约注册册"),
        "注册册必须能被读屏念出来",
    );
    set.add(
        "U03-能力-条目读屏含生命周期",
        r.entries()
            .iter()
            .all(|e| r.screen_text().contains(e.lifecycle.zh())),
        "每条都须带生命周期，否则看不出哪条已冻结",
    );
}

// ---------------------------------------------------------------------------
// 聚合入口
// ---------------------------------------------------------------------------

pub fn run_veu03_checks() -> CheckSet {
    let mut tally = FamilyTally::new();
    chk_single_source(&mut tally);
    chk_meta_fields(&mut tally);
    chk_uniqueness(&mut tally);
    chk_reference_counting(&mut tally);
    chk_criteria_required(&mut tally);
    chk_lifecycle(&mut tally);
    chk_capabilities_and_errors(&mut tally);

    let (fam_total, fam_green) = tally.families();
    let mut set = CheckSet::new("veu03-registry");
    tally.flush(&mut set);

    set.add(
        "U03-收敛-标准态族账全绿",
        fam_green == fam_total,
        "收敛只压播报不改判定；族账有红即细项有红",
    );
    set.add(
        "U03-收敛-族数不超登记表",
        fam_total <= FAMILIES.len(),
        "族前缀未登记会静默并入兜底族，等于丢失分组",
    );
    set.add(
        "U03-收敛-未触容量上限",
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
    fn u03_duplicate_key_rejected() {
        let mut r = standard_registry();
        let e = r
            .register(meta("U03-CTR-FOCUS", "v1", DomainTag::T, vec![DomainTag::U]))
            .expect_err("同键须拒");
        assert_eq!(e.code, E_CONTRACT_DUP);
        assert!(e.next.contains("归并建议"));
    }

    #[test]
    fn u03_empty_criteria_rejected() {
        let mut r = standard_registry();
        let e = r
            .register(ContractMeta::new(
                "U03-CTR-X",
                "v1",
                DomainTag::S,
                vec![DomainTag::U],
                "",
            ))
            .expect_err("空判据须拒");
        assert_eq!(e.code, E_META_CRITERIA_EMPTY);
    }

    #[test]
    fn u03_illegal_transition_is_traced() {
        let mut r = standard_registry();
        let before = r.traces().len();
        let _ = r.transition("U03-CTR-THEME", Lifecycle::Draft, "回退");
        assert!(r.traces().len() > before, "被拒迁移必须留痕");
        assert!(r.traces().iter().any(|t| t.rejected));
    }

    #[test]
    fn u03_retire_blocked_while_referenced() {
        let mut r = standard_registry();
        let e = r
            .transition("U03-CTR-FOCUS", Lifecycle::Retired, "废止")
            .expect_err("有引用不得废止");
        assert_eq!(e.code, E_LC_ILLEGAL);
    }

    #[test]
    fn u03_deregister_only_marks() {
        let mut r = standard_registry();
        r.deregister("U03-CTR-FOCUS", "v1", DomainTag::T)
            .expect("撤订");
        // 行还在，只是标了 revoked。
        assert!(r.refs().len() >= 3);
        assert!(r.ref_count("U03-CTR-FOCUS", "v1") == 1);
    }

    #[test]
    fn u03_self_audit_clean() {
        let r = standard_registry();
        assert!(r.self_audit().is_empty(), "标准册自身须干净：{:?}", r.self_audit());
    }

    #[test]
    fn u03_all_criteria_pass() {
        let s = run_veu03_checks();
        let (_p, f) = s.tally();
        assert_eq!(f, 0, "存在红项");
        assert!(!s.truncated());
    }
}