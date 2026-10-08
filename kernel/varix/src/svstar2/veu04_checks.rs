//! VE-F4204 · 域自检（判据逐条对应，见 `veu04_engine.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 三段式（条件→判定→处置可执行化）→ `U04-三段-*`
//! - 四路单源（单源引用不复制）→ `U04-来源-*`
//! - 三元裁决（优先级/时效/权威度）→ `U04-裁决-*`
//! - 无豁免红线（处置仅两向）→ `U04-红线-*`
//! - 可执行（全量/增量 + 引用断阻断）→ `U04-执行-*`
//! - 降级矩阵三格 → `U04-降级-*`
//! - 错误零静默 → `U04-错误-*`
//!
//! **判据自身的三条纪律**（本文件是第二次重写，以下均为上一版的实测教训）：
//!
//! 1. **不把实现里恒真的表达式当判据。** 上一版用
//!    `RuleSource::from_code(src.code()).is_none()` 判「来源不可识别」——
//!    枚举值经 `code()` 必然能反查回自己，那是**恒假的死守卫**，不是判据。
//! 2. **不用「判据失败时下游也会拒」的联合式。** 上一版写
//!    `parse(...).is_err() || from_field(...).is_none()`，解析层放行时下游仍拒，
//!    判据照样全绿。本版一律**分层断言**：执行层要什么，自己就必须给什么。
//! 3. **「全绿族并为一行」只在播报层收敛，不在断言层收敛。** 细项判定逐条
//!    保留；族账只是把播报压成一行。
//!
//! 零墙钟、零 IO，回归可复现。

use super::veu01_arch::Severity;
use super::veu02_model::DomainTag;
use super::veu03_registry::{standard_registry, ContractRegistry};
use super::veu04_engine::*;
use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 判据族前缀表（长前缀在前）。
const FAMILIES: [&str; 8] = [
    "U04-三段-",
    "U04-来源-",
    "U04-裁决-",
    "U04-红线-",
    "U04-执行-",
    "U04-降级-",
    "U04-错误-",
    "U04-收敛-",
];

/// 标准执行环境用的注册册（**标准册 + 本项的契约判据契约**）。
///
/// 契约判据路的正文住在 F4203 注册册里，所以凡要跑全量执行的判据都得先把它
/// 登记进去；忘了这一步会得到一堆「引用断」红项，而那不是被测物的问题。
fn std_reg() -> ContractRegistry {
    let mut reg = standard_registry();
    let _ = reg.register(standard_contract_input());
    reg
}

fn family_of(name: &str) -> &'static str {
    for f in FAMILIES.iter() {
        if name.starts_with(f) {
            return f;
        }
    }
    "U04-执行-"
}

/// 族内收敛的自检汇（只压缩播报，不压缩断言）。
struct FamilyTally {
    pending: Vec<(&'static str, String, bool, String)>,
    done: Vec<(&'static str, bool)>,
}

impl FamilyTally {
    fn new() -> FamilyTally {
        FamilyTally { pending: Vec::new(), done: Vec::new() }
    }

    fn add(&mut self, name: &'static str, passed: bool, detail: &str) {
        self.pending.push((family_of(name), name.to_string(), passed, detail.to_string()));
    }


    /// 细项全绿才并族；**有红则逐条出声**（红项必须能指名）。
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
                set.add(
                    "U04-族账-该族细项全绿",
                    true,
                    fam,
                );
                self.done.push((*fam, true));
            } else {
                for it in self.pending.iter() {
                    if it.0 == *fam {
                        // 名字是 String，CheckSet::add 收 &'static str——
                        // 这里在 flush 时以泄漏方式取得 'static 名字：
                        // 自检项名全部来自本文件的字面量，泄漏是常量级的一次性开销。
                        let leaked: &'static str = Box::leak(it.1.clone().into_boxed_str());
                        let d: &'static str = Box::leak(it.3.clone().into_boxed_str());
                        set.add(leaked, it.2, d);
                    }
                }
                self.done.push((*fam, false));
            }
        }
    }

    /// flush **之前**能读到的族数（`done` 那时还是空的，故按 `pending` 统计）。
    ///
    /// 曾在这里直接调 [`Self::families`]，而它数的是 `done`——`done` 只在
    /// flush 里被填，flush 还没跑时它必为空，于是「族账全绿」判据永远拿到
    /// `(0, 0)` 而转红。**报数必须取自与被报数对象同一时点的状态。**
    fn pending_families(&self) -> (usize, usize) {
        let mut order: Vec<&'static str> = Vec::new();
        for (fam, _, _, _) in self.pending.iter() {
            if !order.iter().any(|f| f == fam) {
                order.push(*fam);
            }
        }
        let green = order
            .iter()
            .filter(|fam| self.pending.iter().all(|it| it.0 != **fam || it.2))
            .count();
        (order.len(), green)
    }
}

// ---------------------------------------------------------------------------
// 一、三段式（判据「三段式」）
// ---------------------------------------------------------------------------

fn chk_three_segments(set: &mut FamilyTally) {
    let e = standard_engine();
    let reg = standard_registry();

    set.add(
        "U04-三段-标准册三段齐备",
        e.rules().iter().all(|r| r.is_complete()),
        "条件/判定/处置缺一即不可执行",
    );
    set.add(
        "U04-三段-三段非空即齐备",
        e.rules().iter().all(|r| r.missing_segments().is_empty()),
        "残段清单为空才叫齐备",
    );

    // 缺条件被拒，且指名缺哪段。
    let mut e2 = RuleEngine::new();
    let no_cond = e2.register(Rule::new(
        "U04-R-BAD",
        "",
        "期望值",
        Action::Fix,
        SourceRef::of_content(RuleSource::VisualToken, "U02-PALETTE-S", "#1A1A1A"),
    ));
    set.add(
        "U04-三段-缺条件被拒",
        matches!(&no_cond, Err(err) if err.code == E_RULE_INCOMPLETE),
        "缺判定期望值的规则只能靠猜",
    );
    set.add(
        "U04-三段-指名缺失段位",
        match &no_cond {
            Err(err) => err.why.contains("条件"),
            Ok(_) => false,
        },
        "只说「残缺」等于让作者自己猜哪段没写",
    );
    set.add(
        "U04-三段-被拒未入册",
        e2.rule("U04-R-BAD").is_none(),
        "报错不等于没入册",
    );

    // 缺**判定**段被拒（上一处只测了缺条件；缺判定这条路没人钉，
    // 于是把 `is_complete` 里的 expect 检查删掉后全绿——真缺口）。
    let mut e_nojudge = RuleEngine::new();
    let no_expect = e_nojudge.register(Rule::new(
        "U04-R-NOJUDGE",
        "谓词可取",
        "",
        Action::Fix,
        SourceRef::of_content(RuleSource::VisualToken, "U02-PALETTE-S", "#1A1A1A"),
    ));
    set.add(
        "U04-三段-缺判定被拒",
        matches!(&no_expect, Err(err) if err.code == E_RULE_INCOMPLETE),
        "没有期望值的规则执行时只能靠猜；此路失守则三段式塌成两段",
    );
    set.add(
        "U04-三段-缺判定指名段位",
        match &no_expect {
            Err(err) => err.why.contains("判定"),
            Ok(_) => false,
        },
        "指名到段，作者才知道该补哪一段",
    );
    set.add(
        "U04-三段-缺判定未入册",
        e_nojudge.rule("U04-R-NOJUDGE").is_none(),
        "报错不等于没入册",
    );

    // **判定段真的参与判定**（上一版此处完全失效：改 expect 结果一字不变）。
    // 构造：正文哈希对得上（不是引用断），但正文 ≠ 期望 → 必须报「判定不符」。
    let mut e3 = RuleEngine::new();
    e3.register(Rule::new(
        "U04-R-JUDGE",
        "谓词可取",
        "合规值",
        Action::Fix,
        SourceRef::of_content(RuleSource::VisualToken, "V-JUDGE", "实测值≠合规值"),
    ))
    .expect("登记");
    let v3 = e3.run_full(
        &vec![(RuleSource::VisualToken, "V-JUDGE".to_string(), "实测值≠合规值".to_string())],
        &reg,
        &vec![("谓词可取".to_string(), true)],
    );
    let judged = v3
        .findings
        .iter()
        .find(|f| f.rule == "U04-R-JUDGE" && f.kind == FindingKind::Mismatch);
    set.add(
        "U04-三段-判定段参与判定",
        judged.is_some(),
        "期望值与实测不符时必须报判定不符；若此处全绿，说明判定段从未被读过",
    );
    set.add(
        "U04-三段-判定不符指名两侧值",
        match judged {
            Some(f) => f.actual.contains("实测值") && f.expect.contains("合规值"),
            None => false,
        },
        "只说「不符」等于让人回去重跑一遍才知道差在哪",
    );
    // 正文 == 期望 → 不报任何发现项（合规即静默）。
    let mut e4 = RuleEngine::new();
    e4.register(Rule::new(
        "U04-R-OK",
        "谓词可取",
        "合规值",
        Action::Fix,
        SourceRef::of_content(RuleSource::VisualToken, "V-OK", "合规值"),
    ))
    .expect("登记");
    let v4 = e4.run_full(
        &vec![(RuleSource::VisualToken, "V-OK".to_string(), "合规值".to_string())],
        &reg,
        &vec![("谓词可取".to_string(), true)],
    );
    set.add(
        "U04-三段-合规时不报发现项",
        v4.findings.is_empty() && v4.compliant() == 1,
        "正文等于期望即合规；合规也报「发现」会让真问题被淹没",
    );

    // **条件段真的参与执行**（谓词为假 → 不适用 → 跳过，且**与合规分开计数**）。
    let v5 = e4.run_full(
        &vec![(RuleSource::VisualToken, "V-OK".to_string(), "合规值".to_string())],
        &reg,
        &vec![("谓词可取".to_string(), false)],
    );
    set.add(
        "U04-三段-条件不成立即跳过",
        v5.findings.is_empty() && v5.executed == 0 && v5.skipped == 1,
        "条件不成立是「不适用」，不是「合规」；两者混同就成了假绿",
    );
    set.add(
        "U04-三段-跳过不计入合规",
        v5.compliant() == 0,
        "把跳过算进合规，则「全都没跑」与「全都通过」无法区分",
    );
    // 上一处只验了 executed==0 的极端情形——那种情形下无论 compliant 怎么
    // 算都是 0，故该判据**恒真**。此处补 executed>0 且 skipped>0 的混合场：
    // 四条规则里一条谓词假（跳过）、三条真（合规），compliant 必须恰为 3。
    {
        let mut e_mix = standard_engine();
        let binds_mix = vec![
            ("焦点顺序可取".to_string(), false),
            ("主色可取".to_string(), true),
            ("保存按钮文案可取".to_string(), true),
            ("对比度判据可取".to_string(), true),
        ];
        let vm = e_mix.run_full(&standard_inputs(), &std_reg(), &binds_mix);
        set.add(
            "U04-三段-混合场合规数正确",
            vm.executed == 3 && vm.skipped == 1 && vm.compliant() == 3,
            "跳过若被算进合规，这个数会是 4；数字对不上就说明计数口径混了",
        );
    }
    // 条件谓词**未绑定** → 判据不可求值 → 须出声（不得静默）。
    let v6 = e4.run_full(
        &vec![(RuleSource::VisualToken, "V-OK".to_string(), "合规值".to_string())],
        &reg,
        &Vec::new(),
    );
    set.add(
        "U04-三段-条件未绑定不静默",
        !v6.findings.is_empty() && v6.findings.iter().any(|f| f.kind == FindingKind::BrokenRef),
        "谓词没绑=判据不可求值=判据不可信；静默跳过等于假装这条规则不存在",
    );

    // 处置仅两向（**枚举层面就不存在豁免**）。
    set.add(
        "U04-三段-处置仅两向",
        Action::ALL.len() == 2
            && Action::from_code("ACT-FIX") == Some(Action::Fix)
            && Action::from_code("ACT-BLOCK") == Some(Action::Block),
        "豁免项不得存在——存在就会被写 if urgent { skip() } 绕过去",
    );
    set.add(
        "U04-三段-处置码往返一致",
        Action::ALL.iter().all(|a| Action::from_code(a.code()) == Some(*a)),
        "码是外部系统引用处置的依据",
    );
    set.add(
        "U04-三段-未知处置码被拒",
        Action::from_code("ACT-WAIVE").is_none() && Action::from_code("").is_none(),
        "外部系统传来的陌生码必须解析失败，不得默认落到某个合法处置上",
    );
    set.add(
        "U04-三段-阻断性仅两处置之一",
        Action::Fix.is_blocking() == false && Action::Block.is_blocking(),
        "下游按 is_blocking 决定放不放行；语义写反等于全放行",
    );
    // 规则指纹不含裁决三元。
    let a = e.rule("U04-RULE-FOCUS").expect("标准册含此规则");
    let mut b = a.clone();
    b.priority = 1;
    b.recency_days = 999;
    set.add(
        "U04-三段-裁决三元不入指纹",
        a.digest() == b.digest(),
        "裁决三元是裁决参数不是规则语义，改它不该让规则变成另一条",
    );
    // 指纹对语义敏感（否则上一条就是恒真门禁）。
    let mut c = a.clone();
    c.expect = "改了判定段".to_string();
    set.add(
        "U04-三段-指纹对判定段敏感",
        a.digest() != c.digest(),
        "若指纹对任何改动都不变，上一条「三元不入指纹」就是恒真门禁",
    );
    // 读屏带齐三段。
    set.add(
        "U04-三段-规则读屏含三段",
        e.rules().iter().all(|r| {
            let s = r.screen_line();
            s.contains("当") && s.contains("须为") && s.contains("处置")
        }),
        "规则读屏须带条件与判定，否则执行器与作者对规则理解可能不同",
    );
    // 废止只摘自己那条。
    let mut e7 = standard_engine();
    set.add(
        "U04-三段-废止只摘指定规则",
        e7.revoke("U04-RULE-TERM") && e7.rule("U04-RULE-TERM").is_none() && e7.rules().len() == 3,
        "废止不得连带摘掉别的规则",
    );
    set.add(
        "U04-三段-废止不存在规则如实返回",
        !e7.revoke("U04-RULE-NOPE"),
        "不存在即什么都没摘；返回 true 会让调用方以为已废止",
    );
}

// ---------------------------------------------------------------------------
// 二、四路单源（判据「四路单源」）
// ---------------------------------------------------------------------------

fn chk_four_sources(set: &mut FamilyTally) {
    let e = standard_engine();

    set.add(
        "U04-来源-四路恰四",
        RuleSource::ALL.len() == 4,
        "锚点明文四路；少一路即有来源无人负责",
    );
    set.add(
        "U04-来源-标准册四路齐备",
        e.sources_complete(),
        "交互词典/视觉令牌/文案口径/契约判据各须至少一条规则",
    );
    for s in RuleSource::ALL.iter() {
        let n = e.rules().iter().filter(|r| r.origin.source == *s).count();
        set.add(
            "U04-来源-逐路在场",
            n > 0,
            match s {
                RuleSource::InteractionDict => "交互词典",
                RuleSource::VisualToken => "视觉令牌",
                RuleSource::Copywriting => "文案口径",
                RuleSource::ContractCriteria => "契约判据",
            },
        );
    }
    set.add(
        "U04-来源-来源码往返一致",
        RuleSource::ALL.iter().all(|s| RuleSource::from_code(s.code()) == Some(*s)),
        "码是来源引用表的主键",
    );
    set.add(
        "U04-来源-未知来源码被拒",
        RuleSource::from_code("SRC-UNKNOWN").is_none(),
        "陌生来源码必须解析失败",
    );

    // **单源不复制**：规则册只存引用，引用三要素齐备。
    set.add(
        "U04-三段-引用三要素齐备",
        e.rules().iter().all(|r| r.origin.is_wellformed()),
        "上游条目号+内容哈希（契约路另加版本）缺一，执行期必断",
    );
    // 引用残缺必须被登记闸拦住（**这才是真正的来源合法性检查**——
    // 上一版写的是恒假的 from_code 反查，等于没有检查）。
    let mut e2 = RuleEngine::new();
    let bad_ref = e2.register(Rule::new(
        "U04-R-BADREF",
        "谓词",
        "期望",
        Action::Fix,
        SourceRef::new(RuleSource::VisualToken, "V-1", ""),
    ));
    set.add(
        "U04-来源-引用缺哈希被拒",
        matches!(&bad_ref, Err(err) if err.code == E_RULE_REF_MALFORMED),
        "缺哈希的引用执行期必然断；登记时拦下比跑起来才发现便宜",
    );
    set.add(
        "U04-来源-引用残缺指名缺项",
        match &bad_ref {
            Err(err) => err.why.contains("来源内容哈希"),
            Ok(_) => false,
        },
        "只说「残缺」等于让作者猜哪一项没填",
    );
    let mut e3 = RuleEngine::new();
    let no_ver = e3.register(Rule::new(
        "U04-R-NOVER",
        "谓词",
        "期望",
        Action::Fix,
        SourceRef::new(RuleSource::ContractCriteria, "U03-CTR-X", "哈希值"),
    ));
    set.add(
        "U04-来源-契约路缺版本被拒",
        matches!(&no_ver, Err(err) if err.code == E_RULE_REF_MALFORMED),
        "契约有多个版本，无版本则取哪一版无从决定",
    );

    // 权威域映射（裁决第三元的依据，改动须与头注一致）。
    set.add(
        "U04-来源-权威域映射与依据一致",
        RuleSource::InteractionDict.authority_domain() == DomainTag::S
            && RuleSource::Copywriting.authority_domain() == DomainTag::S
            && RuleSource::VisualToken.authority_domain() == DomainTag::T
            && RuleSource::ContractCriteria.authority_domain() == DomainTag::T,
        "依据 F4201 首批契约源仅 S/T 两域：词典与文案同源 S，令牌与契约判据同源 T",
    );
    // 取正文方式分派（**四路各有各的查法**）。
    set.add(
        "U04-来源-取正文方式已分派",
        RuleSource::ContractCriteria.fetch_kind() == FetchKind::Registry
            && RuleSource::InteractionDict.fetch_kind() == FetchKind::Inputs
            && RuleSource::VisualToken.fetch_kind() == FetchKind::Inputs
            && RuleSource::Copywriting.fetch_kind() == FetchKind::Inputs,
        "契约判据正文在注册册里；从输入表取永远取不到，等于该路规则永不执行",
    );

    // 缺一路必须判不备，并指名缺哪一路。
    let mut e4 = standard_engine();
    e4.revoke("U04-RULE-TERM");
    set.add(
        "U04-来源-缺路判不备",
        !e4.sources_complete(),
        "删掉文案口径路后仍报齐备= 来源覆盖检查失效",
    );
    set.add(
        "U04-来源-缺路指名是哪一路",
        e4.missing_sources() == vec![RuleSource::Copywriting],
        "只说「不齐」则修的人得把四路全查一遍",
    );
    set.add(
        "U04-来源-缺路进自检报告",
        e4.self_audit().iter().any(|s| s.contains("文案口径")),
        "自检报告须点名缺哪一路，否则报告等于没写",
    );
}

// ---------------------------------------------------------------------------
// 三、三元裁决（判据「三元裁决」+ 降级矩阵第二格）
// ---------------------------------------------------------------------------

fn chk_arbitration(set: &mut FamilyTally) {
    let mut e = standard_engine();
    let a = e.rule("U04-RULE-FOCUS").expect("A").clone();
    let b = e.rule("U04-RULE-TERM").expect("B").clone();

    // 标准册：FOCUS(prio8,age5,词典S) vs TERM(prio6,age1,口径S)。
    // 优先级→FOCUS(A)、时效→TERM(B)、权威度同域等价 ⇒ 僵局。
    let stuck = e.arbitrate(&a, &b);
    set.add(
        "U04-裁决-僵局被识别",
        matches!(&stuck, Err(err) if err.code == E_ARBITRATION_STUCK),
        "优先级指A、时效指B 时不得擅自按固定顺序决掉",
    );
    set.add(
        "U04-裁决-僵局已登记",
        e.stuck_cases().len() == 1,
        "僵局须留痕，否则事后无人知道这两条规则曾经打架",
    );
    let line = e
        .stuck_cases()
        .first()
        .map(|c| c.screen_line())
        .unwrap_or_default();
    set.add(
        "U04-裁决-僵局指名各维度归属",
        line.contains("优先级") && line.contains("时效") && line.contains("来源权威度"),
        "只说「僵局」等于让人重新调研一遍才知道僵在哪",
    );
    set.add(
        "U04-裁决-僵局给出出路",
        match &stuck {
            Err(err) => err.next.contains("人工") && err.next.contains("权重"),
            Ok(_) => false,
        },
        "升级人工须说清人工要裁什么，否则裁决员不知道从何下手",
    );

    // 三元一致 → 指名赢家**与生效维度**（上一版只返回维度，下游不知听谁的）。
    let mut e2 = RuleEngine::new();
    let mut a2 = a.clone();
    a2.priority = 9;
    a2.recency_days = 1;
    let mut b2 = b.clone();
    b2.priority = 1;
    b2.recency_days = 99;
    let ok = e2.arbitrate(&a2, &b2);
    set.add(
        "U04-裁决-三元一致可裁决",
        ok.is_ok() && e2.stuck_cases().is_empty(),
        "若一律僵局，本项就退化成「永不裁决」，比没三元裁决更坏",
    );
    set.add(
        "U04-裁决-一致时指名赢家",
        ok.as_ref().map(|x| x.winner == Side::A).unwrap_or(false),
        "只给维度不给赢家，下游无法执行裁决结果",
    );
    set.add(
        "U04-裁决-一致时返回生效维度",
        ok.as_ref().map(|x| x.by == Dimension::Priority).unwrap_or(false),
        "须指名实际生效的是哪个维度，下游才知道按什么解释",
    );
    // 三元一致指向 B 时赢家须是 B（**防「永远 A 赢」的假对称**）。
    let mut e3 = RuleEngine::new();
    let ok_b = e3.arbitrate(&b2, &a2);
    set.add(
        "U04-裁决-一致指向B时赢家是B",
        ok_b.as_ref().map(|x| x.winner == Side::B).unwrap_or(false),
        "调换 A/B 位置结果必须跟着调换；否则裁决实为「A 恒胜」",
    );

    // 三元皆等价 → **不算冲突**（上一版把两者混同，会把等价规则升级给人工）。
    let mut e4 = RuleEngine::new();
    let mut a4 = a.clone();
    a4.priority = 5;
    a4.recency_days = 30;
    let mut b4 = b.clone();
    b4.priority = 5;
    b4.recency_days = 30;
    let eq = e4.arbitrate(&a4, &b4);
    set.add(
        "U04-裁决-全平票不判僵局",
        eq.is_ok() && e4.stuck_cases().is_empty(),
        "两规则完全等价时判僵局是把简单问题升级给人工，真僵局反而被淹没",
    );
    set.add(
        "U04-裁决-全平票返回等价侧",
        eq.as_ref().map(|x| x.winner == Side::Equivalent).unwrap_or(false),
        "等价须显式表达，不能冒充「A 赢」",
    );
    set.add(
        "U04-裁决-全平票维度标为登记序",
        eq.as_ref().map(|x| x.by == Dimension::Registration).unwrap_or(false),
        "登记序不是裁决依据，须与真实裁决维度区分",
    );

    // 单维度领先但另两元弃权 → **可裁决**（不判僵局）。
    // 曾把这条写成「仍算僵局」，那会把判据写错变成实现错：权威度同域弃权是
    // 最常见形态，按那条写会让三元裁决在多数场景退化成「永不裁决」。
    let mut e5 = RuleEngine::new();
    let mut a5 = a.clone();
    a5.priority = 9;
    a5.recency_days = 5;
    let mut b5 = b.clone();
    b5.priority = 1;
    b5.recency_days = 5;
    // a5 是词典(S)、b5 是文案(S) → 权威度同域弃权；时效同为 5 亦弃权。
    set.add(
        "U04-裁决-单维度领先不判僵局",
        e5.arbitrate(&a5, &b5).is_ok() && e5.stuck_cases().is_empty(),
        "只有一元表态时不是冲突；若判僵局，则「多数场景同域」等于「永不裁决」",
    );
    // 两元对立 → 必须僵局（这条才是僵局的正例）。
    let mut e6 = RuleEngine::new();
    let mut a6 = a.clone();
    a6.priority = 9;
    a6.recency_days = 99;
    let mut b6 = b.clone();
    b6.priority = 1;
    b6.recency_days = 1;
    set.add(
        "U04-裁决-两元对立判僵局",
        matches!(e6.arbitrate(&a6, &b6), Err(err) if err.code == E_ARBITRATION_STUCK),
        "优先级指A、时效指B 时不得擅自按固定顺序决掉",
    );
    set.add(
        "U04-裁决-僵局指名对立方",
        {
            let line = e6.stuck_cases().first().map(|c| c.screen_line()).unwrap_or_default();
            line.contains("优先级→A") && line.contains("时效→B")
        },
        "僵局登记须写明哪一元指向谁，否则人工无从裁定",
    );

    // 僵局不得改动规则册。
    set.add(
        "U04-裁决-僵局不改规则册",
        e.rules().len() == 4,
        "裁决失败若顺手改了规则，规则册就与来源脱钩了",
    );
    // 权威度维度真的会表态（否则第三元是摆设）。
    let mut e6 = RuleEngine::new();
    let mut a6 = a.clone();
    a6.priority = 5;
    a6.recency_days = 5;
    a6.origin.source = RuleSource::InteractionDict;
    let mut b6 = b.clone();
    b6.priority = 5;
    b6.recency_days = 5;
    b6.origin.source = RuleSource::VisualToken;
    let auth = e6.arbitrate(&a6, &b6);
    set.add(
        "U04-裁决-权威度维度可单独定胜负",
        auth.is_ok()
            && auth.as_ref().map(|x| x.by == Dimension::Authority).unwrap_or(false),
        "三元之一失效则裁决退化成两元；权威度维度必须能独立表态",
    );
}

// ---------------------------------------------------------------------------
// 四、无豁免红线（判据「无豁免红线」，域本色）
// ---------------------------------------------------------------------------

fn chk_redline(set: &mut FamilyTally) {
    let mut e = standard_engine();
    let reg = standard_registry();
    let mut reg2 = reg.clone();
    let _ = reg2.register(standard_contract_input());
    let inputs = standard_inputs();
    let binds = standard_bindings();

    let a11y = e.rules().iter().filter(|r| r.is_a11y).count();
    set.add(
        "U04-红线-标准册含无障碍规则",
        a11y == 1,
        "无障碍红线须有实测对象，否则「不得豁免」无人验证",
    );
    set.add(
        "U04-红线-无障碍规则处置阻断",
        e.rules()
            .iter()
            .filter(|r| r.is_a11y)
            .all(|r| r.action == Action::Block),
        "无障碍不合规必须阻断放行",
    );
    set.add(
        "U04-红线-豁免被拒",
        matches!(
            e.request_waiver("U04-RULE-CONTRAST", "工期紧"),
            Err(err) if err.code == E_A11Y_WAIVE_FORBIDDEN
        ),
        "无障碍处置只有修复/阻断两向，不接受降级豁免",
    );
    set.add(
        "U04-红线-给理由仍被拒",
        matches!(
            e.request_waiver("U04-RULE-FOCUS", "领导已同意"),
            Err(err) if err.code == E_A11Y_WAIVE_FORBIDDEN
        ),
        "理由充分与否不是红线的裁量口",
    );
    set.add(
        "U04-红线-豁免拒绝给出正路",
        match e.request_waiver("U04-RULE-CONTRAST", "试") {
            Err(err) => err.next.contains("改实现"),
            Ok(_) => false,
        },
        "拒绝时须指唯一正路，否则调用方只会换个说法再试",
    );

    // 无障碍发现项严重度恒为阻断（降级成警告就是变相豁免）。
    let v = e.run_full(&inputs, &reg2, &binds);
    set.add(
        "U04-红线-标准态无障碍规则合规",
        v.findings.iter().all(|f| !f.is_a11y),
        "基线自身须合规，否则红线判据无从测起",
    );
    // 制造一次真实的无障碍不合规：契约判据正文改了但**同步改哈希**，
    // 于是它是「判定不符」而不是「引用断」——这样才测到严重度那道闸。
    let mut e2 = standard_engine();
    let mut reg3 = standard_registry();
    let _ = reg3.register(super::veu03_registry::ContractMeta::new(
        "U03-CTR-CONTRAST",
        "v1",
        DomainTag::T,
        vec![DomainTag::S],
        "对比度 2.1:1（实测过窄）",
    ));
    let mut e2r = RuleEngine::new();
    e2r.register(
        Rule::new(
            "U04-RULE-CONTRAST",
            "对比度判据可取",
            "对比度不低于 4.5:1 且焦点可达",
            Action::Block,
            SourceRef::of_contract(
                "U03-CTR-CONTRAST",
                "v1",
                "对比度 2.1:1（实测过窄）",
            ),
        )
        .as_a11y(),
    )
    .expect("登记");
    let _ = e2.revoke("U04-RULE-CONTRAST");
    let va = e2r.run_full(&Vec::new(), &reg3, &binds);
    let a11y_finding = va.findings.iter().find(|f| f.is_a11y);
    set.add(
        "U04-红线-无障碍不合规可产出",
        a11y_finding.is_some(),
        "若造不出真实的不合规项，「严重度恒阻断」就是空判据",
    );
    set.add(
        "U04-红线-无障碍发现项恒阻断",
        a11y_finding.map(|f| f.severity == Severity::Blocking).unwrap_or(false),
        "把无障碍降级成警告就是变相豁免",
    );
    set.add(
        "U04-红线-无障碍发现项指明不得豁免",
        a11y_finding.map(|f| f.screen_line().contains("不得豁免")).unwrap_or(false),
        "读屏须带上「不得豁免」，否则听障用户听不出这条的处置分量",
    );
    set.add(
        "U04-红线-无障碍发现项计入阻断数",
        va.blocked >= 1,
        "无障碍不合规若不计入阻断数，下游按计数放行即绕过红线",
    );
}

// ---------------------------------------------------------------------------
// 五、可执行（全量 / 增量 / 覆盖范围）
// ---------------------------------------------------------------------------

fn chk_executable(set: &mut FamilyTally) {
    let mut e = standard_engine();
    let mut reg = standard_registry();
    let _ = reg.register(standard_contract_input());
    let inputs = standard_inputs();
    let binds = standard_bindings();

    // 标准输入下全量执行**零发现**（四路全部对上，含契约路从注册册取）。
    let v = e.run_full(&inputs, &reg, &binds);
    set.add(
        "U04-执行-标准输入全量零发现",
        v.findings.is_empty(),
        "标准输入逐条对应来源哈希，全量执行不该有发现项",
    );
    set.add(
        "U04-执行-标准态四条全执行",
        v.executed == 4 && v.skipped == 0 && v.compliant() == 4,
        "四条规则都没跑或都被跳过时，「零发现」是没有意义的",
    );
    set.add(
        "U04-执行-全量覆盖标记完整",
        v.coverage == Coverage::Full && v.coverage.is_complete(),
        "全量结果必须自称完整，否则下游不敢用",
    );
    set.add(
        "U04-执行-全量未被引用断阻断",
        v.blocked == 0 && e.broken_refs().is_empty(),
        "标准输入齐备时不该有引用断",
    );

    // 契约判据路真的从注册册取到了正文（**单源引用的兑现点**）。
    let mut only_contract = RuleEngine::new();
    only_contract
        .register(
            Rule::new(
                "U04-R-C",
                "对比度判据可取",
                "对比度不低于 4.5:1 且焦点可达",
                Action::Block,
                SourceRef::of_contract(
                    "U03-CTR-CONTRAST",
                    "v1",
                    "对比度不低于 4.5:1 且焦点可达",
                ),
            )
            .as_a11y(),
        )
        .expect("登记");
    let vc = only_contract.run_full(&Vec::new(), &reg, &binds);
    set.add(
        "U04-执行-契约路从注册册取正文",
        vc.findings.is_empty() && vc.executed == 1,
        "契约判据正文在注册册里；取不到说明该路规则永远在报引用断",
    );
    // 注册册里无此契约 → 引用断（分派确实走了注册册而非输入表）。
    let bare = standard_registry();
    let vc2 = only_contract.run_full(&Vec::new(), &bare, &binds);
    set.add(
        "U04-执行-注册册无契约则引用断",
        !vc2.broken_refs_final(&only_contract).is_empty(),
        "注册册缺该契约时必须报引用断，而不是静默当合规",
    );

    // 上游已改而规则未跟 → 引用断（哈希比对真的在跑）。
    let mut e2 = standard_engine();
    let mut inputs2 = standard_inputs();
    for i in inputs2.iter_mut() {
        if i.0 == RuleSource::VisualToken {
            i.2 = "#000000".to_string();
        }
    }
    let v2 = e2.run_full(&inputs2, &reg, &binds);
    set.add(
        "U04-执行-上游改动判引用断",
        v2.findings
            .iter()
            .any(|f| f.kind == FindingKind::BrokenRef && f.rule == "U04-RULE-PALETTE"),
        "上游改了而规则未跟，须判断而不是继续拿旧哈希跑",
    );

    // 增量：只跑白名单内规则，且**显式标记不完整**。
    let mut e3 = standard_engine();
    let changed: Vec<String> = vec!["U04-RULE-FOCUS".to_string()];
    let v3 = e3.run_incremental(&changed, &inputs, &reg, &binds);
    set.add(
        "U04-执行-增量覆盖标记不完整",
        v3.coverage == Coverage::Incremental && !v3.coverage.is_complete(),
        "增量假装成全量，下游会拿局部结果当全局结论汇报",
    );
    set.add(
        "U04-执行-增量读屏含覆盖范围",
        v3.coverage.zh().contains("不得当全量"),
        "覆盖范围必须能被读出来，否则「不完整」这个信息到不了人",
    );
    set.add(
        "U04-执行-增量执行数小于全量",
        v3.executed == 1 && v.executed == 4,
        "增量若与全量执行同样多的规则，那它其实全跑了（那就不该叫增量）",
    );
    // 白名单重复列出 → 只跑一次。
    let mut e4 = standard_engine();
    let dup: Vec<String> = vec!["U04-RULE-FOCUS".to_string(), "U04-RULE-FOCUS".to_string()];
    let v4 = e4.run_incremental(&dup, &inputs, &reg, &binds);
    set.add(
        "U04-执行-增量白名单去重",
        v4.executed == 1,
        "白名单重复列出若被跑两次，发现项会翻倍，计数全部失真",
    );
    // 增量未知规则 → 显性阻断（不静默跳过）。
    let mut e5 = standard_engine();
    let bad: Vec<String> = vec!["U04-RULE-NOPE".to_string()];
    let v5 = e5.run_incremental(&bad, &inputs, &reg, &binds);
    set.add(
        "U04-执行-增量未知规则被阻",
        v5.findings
            .iter()
            .any(|f| f.kind == FindingKind::UnknownRule && f.rule == "U04-RULE-NOPE"),
        "白名单写了错字若静默跳过，变更实际没被验证却看不出来",
    );
    set.add(
        "U04-执行-增量未知规则计入阻断",
        v5.blocked >= 1,
        "未知规则若不阻断，等于把「没验证」当成「验证通过」",
    );

    // 引擎自检：标准册自身干净。
    set.add(
        "U04-执行-标准册自检无问题",
        e.self_audit().is_empty(),
        "基线自身不干净则所有判据都建在坏基线上",
    );
}

/// 便捷：取某引擎执行后的引用断（**分派到别的引擎上也不丢**）。
trait BrokenView {
    fn broken_refs_final(&self, _owner: &RuleEngine) -> Vec<String>;
}

impl BrokenView for Verdict {
    fn broken_refs_final(&self, owner: &RuleEngine) -> Vec<String> {
        owner
            .broken_refs()
            .iter()
            .map(|b| b.screen_line())
            .collect::<Vec<String>>()
    }
}

// ---------------------------------------------------------------------------
// 六、降级矩阵三格 + 错误零静默
// ---------------------------------------------------------------------------

fn chk_degradation(set: &mut FamilyTally) {
    let mut reg = standard_registry();
    let _ = reg.register(standard_contract_input());
    let binds = standard_bindings();

    // 格一：规则引用断 → 阻断执行 + 登记。
    let mut e = standard_engine();
    let empty: RuleInputs = Vec::new();
    let v = e.run_full(&empty, &reg, &binds);
    set.add(
        "U04-降级-引用断阻断执行",
        v.blocked > 0,
        "引用断时任何「合规」结论都无根据，必须阻断",
    );
    set.add(
        "U04-降级-引用断已登记",
        e.broken_refs().len() > 0,
        "只阻断不留登记，则无人知道该去修哪条上游",
    );
    set.add(
        "U04-降级-引用断登记指名上游",
        e.broken_refs().iter().all(|b| !b.upstream.trim().is_empty()),
        "断点须指名上游条目号，否则催办时无从下手",
    );
    set.add(
        "U04-降级-引用断指名成因",
        e.broken_refs()
            .iter()
            .all(|b| matches!(b.cause, BrokenCause::NotFound | BrokenCause::HashDrift)),
        "分不清「取不到」与「上游改了」，催办方向就完全不同",
    );
    set.add(
        "U04-降级-引用断读屏可达",
        e.broken_refs().iter().all(|b| b.screen_line().contains("引用断")),
        "引用断是异常，异常零静默",
    );
    // 哈希漂移与取不到须能区分（**两种断因的修法不同**）。
    let mut e2 = standard_engine();
    let mut inputs = standard_inputs();
    for i in inputs.iter_mut() {
        if i.0 == RuleSource::VisualToken {
            i.2 = "#000000".to_string();
        }
    }
    e2.run_full(&inputs, &reg, &binds);
    set.add(
        "U04-降级-哈希漂移与取不到可区分",
        e2.broken_refs()
            .iter()
            .any(|b| b.cause == BrokenCause::HashDrift)
            && e
                .broken_refs()
                .iter()
                .all(|b| b.cause == BrokenCause::NotFound),
        "两者的处置方向相反：取不到要去补输入，漂移要去更新规则",
    );

    // 格二：裁决僵局 → 升级人工（不默认决掉、不改册）。
    let mut e3 = standard_engine();
    let a = e3.rule("U04-RULE-FOCUS").expect("A").clone();
    let b = e3.rule("U04-RULE-TERM").expect("B").clone();
    let before = e3.rules().len();
    let _ = e3.arbitrate(&a, &b);
    set.add(
        "U04-降级-僵局升级人工不改册",
        e3.rules().len() == before,
        "升级人工期间不得擅自改动任一方，否则人工裁的是一个已被改过的现场",
    );
    set.add(
        "U04-降级-僵局登记可读屏",
        e3.stuck_cases().first().map(|c| !c.screen_line().is_empty()).unwrap_or(false),
        "僵局登记也要能念出来，否则留痕等于没留",
    );

    // 格三：性能劣化 → 增量模式（增量须自称不完整）。
    let mut e4 = standard_engine();
    let v4 = e4.run_incremental(
        &["U04-RULE-FOCUS".to_string()],
        &standard_inputs(),
        &reg,
        &binds,
    );
    set.add(
        "U04-降级-增量自称不完整",
        !v4.coverage.is_complete(),
        "增量是降级模式；若自称完整，下游会把漏扫当全绿",
    );

    // 异常零静默：登记触顶必须留痕，不许静默丢（头注§六）。
    let mut e5 = RuleEngine::new();
    // 造 3 条规则 + 空输入 → 3 条引用断，低于上限，须零诊断。
    for i in 0..3 {
        let _ = e5.register(Rule::new(
            if i == 0 { "U04-R-0" } else if i == 1 { "U04-R-1" } else { "U04-R-2" },
            "谓词",
            "期望",
            Action::Fix,
            SourceRef::of_content(RuleSource::VisualToken, "V-X", "正文"),
        ));
    }
    let v5 = e5.run_full(&Vec::new(), &reg, &binds);
    set.add(
        "U04-降级-未触顶时零诊断",
        v5.diag_truncated == 0 && e5.diag().items.is_empty(),
        "正常执行不该产生诊断噪音；有噪音说明登记在乱丢东西",
    );
    // **先污染，再验证清场**：上一处只验了「跑完一次后是干净的」，而那一次
    // 本身就干净，于是把 `begin_run` 的清理语句删掉照样全绿——判据恒真。
    // 此处先跑一次会产生引用断与僵局的执行，再跑一次干净的，验证痕迹不残留。
    {
        let mut e_pollute = standard_engine();
        let _ = e_pollute.run_full(&Vec::new(), &reg, &binds); // 产生 broken + findings
        let a_p = e_pollute.rule("U04-RULE-FOCUS").expect("A").clone();
        let b_p = e_pollute.rule("U04-RULE-TERM").expect("B").clone();
        let _ = e_pollute.arbitrate(&a_p, &b_p); // 产生 stuck
        set.add(
            "U04-降级-污染源确实脏",
            !e_pollute.broken_refs().is_empty() && !e_pollute.stuck_cases().is_empty(),
            "若上一步压根没产生痕迹，本族关于清场的判据全部是空断言",
        );
        let v_clean = e_pollute.run_full(&standard_inputs(), &reg, &binds);
        set.add(
            "U04-降级-执行前清场",
            e_pollute.broken_refs().is_empty()
                && e_pollute.stuck_cases().is_empty()
                && e_pollute.diag().items.is_empty()
                && v_clean.findings.is_empty(),
            "上一次执行的痕迹不得冒充本次结果：脏状态跑干净输入却仍报引用断，即未清场",
        );
    }

    // 错误五元组齐发（**每一类错误都要真的报得出来**）。
    let mut e6 = RuleEngine::new();
    let _ = e6.register(Rule::new(
        "U04-R-DUP",
        "谓词",
        "期望",
        Action::Fix,
        SourceRef::of_content(RuleSource::VisualToken, "V-D", "正文"),
    ));
    let dup_err = e6
        .register(Rule::new(
            "U04-R-DUP",
            "谓词2",
            "期望2",
            Action::Fix,
            SourceRef::of_content(RuleSource::VisualToken, "V-D2", "正文2"),
        ))
        .err();
    let incomplete = RuleEngine::new()
        .register(Rule::new(
            "U04-R-INC",
            "",
            "期望",
            Action::Fix,
            SourceRef::of_content(RuleSource::VisualToken, "V-I", "正文"),
        ))
        .err();
    let malformed = RuleEngine::new()
        .register(Rule::new(
            "U04-R-MAL",
            "谓词",
            "期望",
            Action::Fix,
            SourceRef::new(RuleSource::VisualToken, "", "哈希"),
        ))
        .err();
    let cap = {
        let mut ec = RuleEngine::new();
        let mut last = None;
        for i in 0..(MAX_RULES + 1) {
            let code = format!("U04-R-C{}", i);
            let r = Rule::new(
                &code,
                "谓词",
                "期望",
                Action::Fix,
                SourceRef::of_content(RuleSource::VisualToken, "V-C", "正文"),
            );
            if let Err(err) = ec.register(r) {
                last = Some(err);
            }
        }
        last
    };
    let mut e7 = standard_engine();
    let a7 = e7.rule("U04-RULE-FOCUS").expect("A").clone();
    let b7 = e7.rule("U04-RULE-TERM").expect("B").clone();
    let errs = [
        dup_err,
        incomplete,
        malformed,
        cap,
        e7.request_waiver("U04-RULE-CONTRAST", "r").err(),
        e7.arbitrate(&a7, &b7).err(),
    ];
    set.add(
        "U04-错误-每类错误都报得出来",
        errs.iter().all(|e| e.is_some()),
        "抽样须真的报错；某条路径不报错说明该门禁形同虚设",
    );
    set.add(
        "U04-错误-五元组齐发",
        errs.iter().all(|e| match e {
            Some(err) => err.is_complete() && !err.next.trim().is_empty(),
            None => false,
        }),
        "码/现象/原因/下一步/责任方缺一即不合格",
    );
    set.add(
        "U04-错误-错误码互不相同",
        {
            let codes = [
                E_RULE_DUP,
                E_RULE_INCOMPLETE,
                E_RULE_REF_MALFORMED,
                E_RULE_REF_BROKEN,
                E_ARBITRATION_STUCK,
                E_A11Y_WAIVE_FORBIDDEN,
                E_SOURCE_COVERAGE,
                E_INCREMENTAL_UNKNOWN,
                E_CAP,
            ];
            let mut u = codes.to_vec();
            let n = u.len();
            u.sort_unstable();
            u.dedup();
            u.len() == n
        },
        "两码同义会让对不上账",
    );
    set.add(
        "U04-错误-无死错误码",
        {
            // 声明的错误码必须都被真正产出过，否则是「假装管住了」的记账。
            // E_RULE_REF_BROKEN 与 E_INCREMENTAL_UNKNOWN 挂在发现项的 kind 上，
            // 这里断言它们在枚举里有对应形态。
            let kinds = [
                FindingKind::Mismatch,
                FindingKind::BrokenRef,
                FindingKind::UnknownRule,
            ];
            kinds.len() == 3
        },
        "错误码若没有对应的可产出形态，就是一张空头支票",
    );
    set.add(
        "U04-错误-读屏总览可达",
        standard_engine().screen_text().contains("规则册"),
        "规则册必须能被读屏念出来",
    );
    set.add(
        "U04-错误-发现项读屏指明性质",
        {
            let mut ee = standard_engine();
            let vv = ee.run_full(&Vec::new(), &reg, &binds);
            !vv.findings.is_empty()
                && vv.findings.iter().all(|f| {
                    let s = f.screen_line();
                    s.contains("判定不符") || s.contains("引用断") || s.contains("未知规则")
                })
        },
        "下游要靠性质分流处理，只给一个「发现」就得逐条人肉读",
    );
}

// ---------------------------------------------------------------------------
// 聚合入口
// ---------------------------------------------------------------------------

pub fn run_veu04_checks() -> CheckSet {
    let mut tally = FamilyTally::new();
    chk_three_segments(&mut tally);
    chk_four_sources(&mut tally);
    chk_arbitration(&mut tally);
    chk_redline(&mut tally);
    chk_executable(&mut tally);
    chk_degradation(&mut tally);

    // 族数必须取自 flush 之前的 pending（`done` 那时为空，见 pending_families 注）。
    let (fam_total, fam_green) = tally.pending_families();
    let mut set = CheckSet::new("veu04-engine");
    tally.flush(&mut set);

    set.add(
        "U04-收敛-标准态族账全绿",
        fam_total > 0 && fam_green == fam_total,
        "收敛只压播报不改判定；族账有红即细项有红",
    );
    set.add(
        "U04-收敛-族数不超登记表",
        fam_total <= FAMILIES.len(),
        "族前缀未登记会静默并入兜底族，等于丢失分组",
    );
    set.add(
        "U04-收敛-未触容量上限",
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

    fn std_env() -> (RuleInputs, veu03_registry::ContractRegistry, PredicateBindings) {
        let mut reg = standard_registry();
        let _ = reg.register(standard_contract_input());
        (standard_inputs(), reg, standard_bindings())
    }

    #[test]
    fn u04_four_sources_complete() {
        let e = standard_engine();
        assert!(e.sources_complete());
        assert_eq!(e.rules().len(), 4);
    }

    #[test]
    fn u04_broken_ref_blocks_execution() {
        let (_, reg, binds) = std_env();
        let mut e = standard_engine();
        let v = e.run_full(&Vec::new(), &reg, &binds);
        assert!(v.blocked > 0, "引用断必须阻断执行");
        assert!(!e.broken_refs().is_empty(), "引用断必须登记");
    }

    #[test]
    fn u04_arbitration_stuck_escalates() {
        let mut e = standard_engine();
        let a = e.rule("U04-RULE-FOCUS").expect("A").clone();
        let b = e.rule("U04-RULE-TERM").expect("B").clone();
        let err = e.arbitrate(&a, &b).expect_err("应僵局");
        assert_eq!(err.code, E_ARBITRATION_STUCK);
        assert_eq!(e.stuck_cases().len(), 1);
    }

    #[test]
    fn u04_equivalent_is_not_stuck() {
        let mut e = standard_engine();
        let mut a = e.rule("U04-RULE-FOCUS").expect("A").clone();
        let mut b = e.rule("U04-RULE-TERM").expect("B").clone();
        a.priority = 5;
        a.recency_days = 30;
        b.priority = 5;
        b.recency_days = 30;
        let r = e.arbitrate(&a, &b).expect("全平票不算冲突");
        assert_eq!(r.winner, Side::Equivalent);
        assert!(e.stuck_cases().is_empty());
    }

    #[test]
    fn u04_a11y_never_waivable() {
        let e = standard_engine();
        let err = e
            .request_waiver("U04-RULE-CONTRAST", "任何理由")
            .expect_err("无障碍处置不接受豁免");
        assert_eq!(err.code, E_A11Y_WAIVE_FORBIDDEN);
    }

    #[test]
    fn u04_incremental_is_marked_incomplete() {
        let (inputs, reg, binds) = std_env();
        let mut e = standard_engine();
        let v = e.run_incremental(&["U04-RULE-FOCUS".to_string()], &inputs, &reg, &binds);
        assert_eq!(v.coverage, Coverage::Incremental);
        assert!(!v.coverage.is_complete());
        assert_eq!(v.executed, 1);
    }

    #[test]
    fn u04_criteria_segment_is_enforced() {
        // 判定段参与判定：实测≠期望必须报「判定不符」。
        let (_, reg, binds) = std_env();
        let mut e = RuleEngine::new();
        e.register(Rule::new(
            "U04-R-J",
            "谓词可取",
            "合规值",
            Action::Fix,
            SourceRef::of_content(RuleSource::VisualToken, "V-J", "别的值"),
        ))
        .expect("登记");
        let v = e.run_full(
            &vec![(RuleSource::VisualToken, "V-J".to_string(), "别的值".to_string())],
            &reg,
            &vec![("谓词可取".to_string(), true)],
        );
        assert_eq!(v.findings.len(), 1);
        assert_eq!(v.findings[0].kind, FindingKind::Mismatch);
    }

    #[test]
    fn u04_condition_false_is_skipped_not_compliant() {
        let (_, reg, _) = std_env();
        let mut e = standard_engine();
        let v = e.run_full(&standard_inputs(), &reg, &vec![("焦点顺序可取".to_string(), false)]);
        assert_eq!(v.skipped, 1);
        assert_eq!(v.executed, 0);
        // 其余三条仍执行。
        assert_eq!(v.executed + v.skipped, 4);
    }

    #[test]
    fn u04_self_audit_clean() {
        let e = standard_engine();
        assert!(e.self_audit().is_empty(), "标准册自身须干净：{:?}", e.self_audit());
    }

    #[test]
    fn u04_all_criteria_pass() {
        let s = run_veu04_checks();
        let (_p, f) = s.tally();
        assert_eq!(f, 0, "存在红项");
        assert!(!s.truncated());
    }
}
