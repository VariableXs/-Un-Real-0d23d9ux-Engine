//! VE-F4206 · 域自检（判据逐条对应，见 `veu06_scan.rs` 头注）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4206`
//!
//! 判据映射（锚点原文 → 自检项）：
//! - **三模式**（全量/增量/定向 + 漂移对版升格）→ `C06-MODE-*`/`C06-DRIFT-*`
//! - **插件化**（冻结接口/隔离+继续/步算预算/冲突保首见）→ `C06-PLUGIN-*`
//! - **五元组**（位置/规则/严重度/证据/建议，归一册冻结映射）→ `C06-NORM-*`
//! - **永不跳过**（a11y 并集结构性注入，违规照样被抓）→ `C06-A11Y-*`
//! - **聚合去重**（风暴承接/确定性序/严重度取高/粘滞）→ `C06-AGG-*`
//! - **加固判据**（码段/CI 行/红线结构）→ `C06-SEG-*`/`C06-CI-*`/`C06-PRIV-*`
//!
//! ## 判据侧设计纪律
//!
//! 1. **永不跳过必须有「违规在场」的正例**：a11y 并集判据不是断
//!    「a11y_injected>0」这种装饰字段，而是构造**a11y 规则违规且不在
//!    增量白名单**的语料——若平台漏扫它，五元组里就不会出现该发现项，
//!    判据当红。装饰字段判据抓不住「扫了但没扫到」。
//! 2. **聚合确定性双跑对拍**：同语料两次扫描的发现项逐位相等——
//!    BTreeMap 有序性是确定性承诺，不是实现细节。
//! 3. **隔离语义断全链**：故障后 `quarantined_now`、注册册隔离册条目、
//!    其余插件绑定仍生效（扫描继续）三处同时断——只断一处会让
//!    「隔离了但扫描也停了」这类伪实现溜过去。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::veu01_arch::Severity;
use super::veu03_registry::ContractRegistry;
use super::veu04_engine::{
    Action, PredicateBindings, Rule, RuleEngine, RuleInputs, RuleSource, SourceRef,
};
use super::veu06_scan::*;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 语料与插件
// ---------------------------------------------------------------------------

/// 标准语料：2 普通规则 + 1 a11y 规则。
///
/// `of_content` 的正文哈希对版要求「登记正文=输入正文」，故违规由
/// 「期望 ≠ 正文」表达：`violate_palette`/`violate_a11y` 各控一路。
fn corpus(violate_palette: bool, violate_a11y: bool) -> (RuleEngine, RuleInputs, ContractRegistry) {
    let palette_body = if violate_palette { "主色偏差" } else { "主色为令牌主色" };
    let a11y_body = if violate_a11y { "对比度 3.0" } else { "对比度 4.5" };
    let mut e = RuleEngine::new();
    let _ = e.register(Rule::new(
        "U06-RULE-FOCUS",
        "p_focus",
        "焦点顺序正确",
        Action::Fix,
        SourceRef::of_content(RuleSource::InteractionDict, "VE-F3982", "焦点顺序正确"),
    ));
    let _ = e.register(Rule::new(
        "U06-RULE-PALETTE",
        "p_palette",
        "主色为令牌主色",
        Action::Fix,
        SourceRef::of_content(RuleSource::VisualToken, "VE-F3983", palette_body),
    ));
    let _ = e.register(
        Rule::new(
            "U06-RULE-CONTRAST",
            "p_contrast",
            "对比度 4.5",
            Action::Block,
            SourceRef::of_content(RuleSource::VisualToken, "VE-F3984", a11y_body),
        )
        .as_a11y(),
    );
    let inputs: RuleInputs = vec![
        (
            RuleSource::InteractionDict,
            "VE-F3982".to_string(),
            "焦点顺序正确".to_string(),
        ),
        (
            RuleSource::VisualToken,
            "VE-F3983".to_string(),
            palette_body.to_string(),
        ),
        (
            RuleSource::VisualToken,
            "VE-F3984".to_string(),
            a11y_body.to_string(),
        ),
    ];
    (e, inputs, ContractRegistry::new())
}

/// 全真绑定。
fn bindings_all_true() -> PredicateBindings {
    vec![
        ("p_focus".to_string(), true),
        ("p_palette".to_string(), true),
        ("p_contrast".to_string(), true),
    ]
}

/// 报告全部绑定为真的插件。
struct TruthPlugin {
    name: &'static str,
    fault: Option<PluginFault>,
    steps: Option<u32>,
    palette_value: bool,
}

impl ScannerPlugin for TruthPlugin {
    fn name(&self) -> &'static str {
        self.name
    }
    fn observe(&self, _q: &PluginQuery) -> Result<PluginReport, PluginFault> {
        if let Some(f) = self.fault.as_ref() {
            return Err(f.clone());
        }
        Ok(PluginReport {
            bindings: vec![
                ("p_focus".to_string(), true),
                ("p_palette".to_string(), self.palette_value),
                ("p_contrast".to_string(), true),
            ],
            declared_steps: self.steps.unwrap_or(10),
        })
    }
}

/// 当前引擎摘要（测试取基线用）。
fn digest_of(e: &RuleEngine) -> String {
    rule_digest(e)
}

pub fn run_veu06_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-U/F4206");

    // ------------------------------------------------------------------
    // C06-MODE：三模式
    // ------------------------------------------------------------------

    // 全量：三规则全执行、合规语料零发现、覆盖=全量、零注入。
    {
        let (mut e, inputs, reg) = corpus(false,false);
        let mut plugins = PluginRegistry::new();
        let impls: Vec<&dyn ScannerPlugin> = vec![&TruthPlugin {
            name: "t1",
            fault: None,
            steps: None,
            palette_value: true,
        }];
        let req = ScanRequest {
            mode: ScanMode::Full,
            base_digest: None,
            ..ScanRequest::default()
        };
        let rep = scan(&mut e, &reg, &mut plugins, &impls, req, &inputs);
        set.add(
            "C06-MODE-01",
            rep.executed == 3 && rep.findings.is_empty() && rep.mode_effective == ScanMode::Full
                && rep.coverage_label == "full" && rep.a11y_injected == 0 && !rep.escalated,
            "全量模式：3 规则全执行、合规语料零发现、零注入零升格",
        );
    }

    // 增量：白名单只含 1 条非 a11y 规则 → 执行数恰为 1+a11y 并集注入。
    {
        let (mut e, inputs, reg) = corpus(false,false);
        let mut plugins = PluginRegistry::new();
        let impls: Vec<&dyn ScannerPlugin> = vec![&TruthPlugin {
            name: "t1",
            fault: None,
            steps: None,
            palette_value: true,
        }];
        let base = digest_of(&e);
        let req = ScanRequest {
            mode: ScanMode::Incremental,
            changed: vec!["U06-RULE-PALETTE".to_string()],
            base_digest: Some(base),
            ..ScanRequest::default()
        };
        let rep = scan(&mut e, &reg, &mut plugins, &impls, req, &inputs);
        set.add(
            "C06-MODE-02",
            rep.executed == 2 && rep.mode_effective == ScanMode::Incremental
                && rep.a11y_injected == 1 && rep.coverage_label == "incremental(partial)",
            "增量 O(变更)：白名单 1 条 + a11y 并集 1 条，执行数恰 2、不完整覆盖显性",
        );
    }

    // 定向：目标集同构语义。
    {
        let (mut e, inputs, reg) = corpus(false,false);
        let mut plugins = PluginRegistry::new();
        let impls: Vec<&dyn ScannerPlugin> = vec![&TruthPlugin {
            name: "t1",
            fault: None,
            steps: None,
            palette_value: true,
        }];
        let base = digest_of(&e);
        let req = ScanRequest {
            mode: ScanMode::Targeted,
            targets: vec!["U06-RULE-FOCUS".to_string()],
            base_digest: Some(base),
            ..ScanRequest::default()
        };
        let rep = scan(&mut e, &reg, &mut plugins, &impls, req, &inputs);
        set.add(
            "C06-MODE-03",
            rep.executed == 2 && rep.mode_effective == ScanMode::Targeted && rep.a11y_injected == 1,
            "定向模式：目标 1 条 + a11y 并集 1 条",
        );
    }

    // 模式闭集：恰三模式且名互异。
    {
        set.add(
            "C06-MODE-04",
            ScanMode::Full.name() == "full"
                && ScanMode::Incremental.name() == "incremental"
                && ScanMode::Targeted.name() == "targeted"
                && ScanMode::Full != ScanMode::Incremental
                && ScanMode::Incremental != ScanMode::Targeted,
            "三模式名互异（封闭枚举）",
        );
    }

    // ------------------------------------------------------------------
    // C06-A11Y：永不跳过（违规在场的正例——装饰字段判据抓不住真漏扫）
    // ------------------------------------------------------------------

    // a11y 规则违规、且不在增量白名单 → 五元组里仍必须出现它。
    {
        let (mut e, inputs, reg) = corpus(false, true); // a11y 规则违规
        let mut plugins = PluginRegistry::new();
        let impls: Vec<&dyn ScannerPlugin> = vec![&TruthPlugin {
            name: "t1",
            fault: None,
            steps: None,
            palette_value: true,
        }];
        let base = digest_of(&e);
        let req = ScanRequest {
            mode: ScanMode::Incremental,
            changed: vec!["U06-RULE-PALETTE".to_string()], // 不含 a11y 规则
            base_digest: Some(base),
            ..ScanRequest::default()
        };
        let rep = scan(&mut e, &reg, &mut plugins, &impls, req, &inputs);
        let a11y_hit = rep
            .findings
            .iter()
            .any(|f| f.rule == "U06-RULE-CONTRAST" && f.is_a11y);
        set.add(
            "C06-A11Y-01",
            a11y_hit && rep.a11y_injected >= 1
                && rep.codes.contains(&ScanCode::A11Y_UNION_INJECTED),
            "a11y 规则违规且不在白名单仍被扫出（永不跳过是结构事实不是承诺）",
        );
    }

    // 全量模式天然全含：违规 a11y 同样在场（三模式均含的另一半）。
    {
        let (mut e, inputs, reg) = corpus(false, true);
        let mut plugins = PluginRegistry::new();
        let impls: Vec<&dyn ScannerPlugin> = vec![&TruthPlugin {
            name: "t1",
            fault: None,
            steps: None,
            palette_value: true,
        }];
        let req = ScanRequest {
            mode: ScanMode::Full,
            ..ScanRequest::default()
        };
        let rep = scan(&mut e, &reg, &mut plugins, &impls, req, &inputs);
        let a11y_hit = rep.findings.iter().any(|f| f.rule == "U06-RULE-CONTRAST");
        set.add(
            "C06-A11Y-02",
            a11y_hit && rep.a11y_injected == 0,
            "全量模式违规 a11y 在场且注入量为 0（全含不虚报注入）",
        );
    }

    // ------------------------------------------------------------------
    // C06-DRIFT：调度漂移→对版
    // ------------------------------------------------------------------

    {
        let (mut e, inputs, reg) = corpus(false,false);
        let mut plugins = PluginRegistry::new();
        let impls: Vec<&dyn ScannerPlugin> = vec![&TruthPlugin {
            name: "t1",
            fault: None,
            steps: None,
            palette_value: true,
        }];
        let req = ScanRequest {
            mode: ScanMode::Incremental,
            changed: vec!["U06-RULE-PALETTE".to_string()],
            base_digest: Some("deadbeef".to_string()), // 错基线
            ..ScanRequest::default()
        };
        let rep = scan(&mut e, &reg, &mut plugins, &impls, req, &inputs);
        set.add(
            "C06-DRIFT-01",
            rep.escalated && rep.mode_effective == ScanMode::Full && rep.executed == 3
                && rep.codes.contains(&ScanCode::DRIFT_RECONCILED),
            "基线摘要不符 → 升格全量重扫（对版安全语义：漏扫比多扫贵）",
        );
    }

    {
        let (mut e, inputs, reg) = corpus(false,false);
        let mut plugins = PluginRegistry::new();
        let impls: Vec<&dyn ScannerPlugin> = vec![&TruthPlugin {
            name: "t1",
            fault: None,
            steps: None,
            palette_value: true,
        }];
        let req = ScanRequest {
            mode: ScanMode::Targeted,
            targets: vec!["U06-RULE-FOCUS".to_string()],
            base_digest: None, // 缺基线 = 漂移
            ..ScanRequest::default()
        };
        let rep = scan(&mut e, &reg, &mut plugins, &impls, req, &inputs);
        set.add(
            "C06-DRIFT-02",
            rep.escalated && rep.executed == 3,
            "缺基线摘要同样升格（非全量模式必须带基线）",
        );
    }

    {
        // 摘要对版可判定：引擎不变摘要稳定；注销规则摘要移动。
        let d1 = {
            let (mut e, inputs, reg) = corpus(false,false);
            let mut plugins = PluginRegistry::new();
            let impls: Vec<&dyn ScannerPlugin> = vec![&TruthPlugin {
                name: "t1",
                fault: None,
                steps: None,
                palette_value: true,
            }];
            let req = ScanRequest {
                mode: ScanMode::Full,
                ..ScanRequest::default()
            };
            let r1 = scan(&mut e, &reg, &mut plugins, &impls, req.clone(), &inputs);
            let _ = r1;
            digest_of(&e)
        };
        let (mut e2, _i, _r) = corpus(false,false);
        let d2 = digest_of(&e2);
        e2.revoke("U06-RULE-PALETTE");
        let d3 = digest_of(&e2);
        set.add(
            "C06-DRIFT-03",
            d1 == d2 && d2 != d3 && !d1.is_empty(),
            "摘要确定性（双跑一致）且对注销敏感（对版可判定）",
        );
    }

    // ------------------------------------------------------------------
    // C06-PLUGIN：插件化（冻结接口/隔离+继续/预算/冲突）
    // ------------------------------------------------------------------

    // 崩溃→隔离+继续：故障插件进册，其余插件绑定照常供给，扫描出结果。
    {
        let (mut e, inputs, reg) = corpus(false,false);
        let mut plugins = PluginRegistry::new();
        let mut codes = Vec::new();
        let _ = plugins.register(PLUGIN_IFACE_VERSION, "good", &mut codes);
        let _ = plugins.register(PLUGIN_IFACE_VERSION, "bad", &mut codes);
        let impls: Vec<&dyn ScannerPlugin> = vec![
            &TruthPlugin {
                name: "good",
                fault: None,
                steps: None,
                palette_value: true,
            },
            &TruthPlugin {
                name: "bad",
                fault: Some(PluginFault::Crashed),
                steps: None,
                palette_value: true,
            },
        ];
        let req = ScanRequest {
            mode: ScanMode::Full,
            ..ScanRequest::default()
        };
        let rep = scan(&mut e, &reg, &mut plugins, &impls, req, &inputs);
        set.add(
            "C06-PLUGIN-01",
            rep.quarantined_now == 1 && plugins.quarantined().len() == 1
                && plugins.quarantined()[0].fault == PluginFault::Crashed
                && plugins.active().contains(&"good")
                && rep.findings.is_empty()
                && rep.codes.contains(&ScanCode::PLUGIN_QUARANTINED),
            "崩溃插件隔离、好插件绑定仍生效、扫描照常出结果（隔离+继续）",
        );
    }

    // 隔离跨轮持久：第二轮不再执行故障插件（隔离 O(插件)）。
    {
        let (mut e, inputs, reg) = corpus(false,false);
        let mut plugins = PluginRegistry::new();
        let mut codes = Vec::new();
        let _ = plugins.register(PLUGIN_IFACE_VERSION, "bad", &mut codes);
        let impls: Vec<&dyn ScannerPlugin> = vec![&TruthPlugin {
            name: "bad",
            fault: Some(PluginFault::Crashed),
            steps: None,
            palette_value: true,
        }];
        let req = ScanRequest {
            mode: ScanMode::Full,
            ..ScanRequest::default()
        };
        let r1 = scan(&mut e, &reg, &mut plugins, &impls, req.clone(), &inputs);
        let r2 = scan(&mut e, &reg, &mut plugins, &impls, req, &inputs);
        set.add(
            "C06-PLUGIN-02",
            r1.quarantined_now == 1 && r2.quarantined_now == 0 && plugins.active().is_empty(),
            "隔离册跨轮持久：第二轮故障插件被跳过不重复入册",
        );
    }

    // 步算超预算：申报超预算按故障处置（挂起防护可机检面）。
    {
        let (mut e, inputs, reg) = corpus(false,false);
        let mut plugins = PluginRegistry::new();
        let mut codes = Vec::new();
        let _ = plugins.register(PLUGIN_IFACE_VERSION, "slow", &mut codes);
        let impls: Vec<&dyn ScannerPlugin> = vec![&TruthPlugin {
            name: "slow",
            fault: None,
            steps: Some(PLUGIN_STEP_BUDGET + 1),
            palette_value: true,
        }];
        let req = ScanRequest {
            mode: ScanMode::Full,
            ..ScanRequest::default()
        };
        let rep = scan(&mut e, &reg, &mut plugins, &impls, req, &inputs);
        set.add(
            "C06-PLUGIN-03",
            rep.quarantined_now == 1
                && plugins.quarantined()[0].fault == PluginFault::OverBudget
                && rep.codes.contains(&ScanCode::STEP_BUDGET_BREACH),
            "步算超预算按故障隔离（no_std 挂起防护，0x4609）",
        );
    }

    // 注册闸：重名拒 + 接口版本不符拒（冻结接口的版本闸）。
    {
        let mut plugins = PluginRegistry::new();
        let mut codes = Vec::new();
        let r1 = plugins.register(PLUGIN_IFACE_VERSION, "dup", &mut codes);
        let r2 = plugins.register(PLUGIN_IFACE_VERSION, "dup", &mut codes);
        let r3 = plugins.register(PLUGIN_IFACE_VERSION + 99, "other", &mut codes);
        set.add(
            "C06-PLUGIN-04",
            r1.is_ok() && r2.is_err() && r3.is_err()
                && codes.contains(&ScanCode::DUPLICATE_PLUGIN)
                && codes.contains(&ScanCode::IFACE_REJECTED)
                && PLUGIN_IFACE_VERSION == 1,
            "重名与版本不符双向拒绝（冻结 v1 接口闸）",
        );
    }

    // 谓词冲突：保首见 + 记账（不静默不崩溃）。
    {
        let mut plugins = PluginRegistry::new();
        let mut codes = Vec::new();
        let _ = plugins.register(PLUGIN_IFACE_VERSION, "first", &mut codes);
        let _ = plugins.register(PLUGIN_IFACE_VERSION, "second", &mut codes);
        let impls: Vec<&dyn ScannerPlugin> = vec![
            &TruthPlugin {
                name: "first",
                fault: None,
                steps: None,
                palette_value: true,
            },
            &TruthPlugin {
                name: "second",
                fault: None,
                steps: None,
                palette_value: false, // 冲突：p_palette 与 first 不一致
            },
        ];
        let mut bindings: PredicateBindings = Vec::new();
        let q = PluginQuery {
            rule_codes: vec![],
            step_budget: PLUGIN_STEP_BUDGET,
        };
        let faulted = run_plugins(&mut plugins, &impls, &q, &mut bindings, &mut codes);
        let palette = bindings.iter().find(|(p, _)| p == "p_palette").map(|(_, v)| *v);
        set.add(
            "C06-PLUGIN-05",
            faulted == 0 && palette == Some(true)
                && codes.iter().filter(|c| **c == ScanCode::PREDICATE_CONFLICT).count() == 1,
            "谓词冲突保首见且恰记一次账（0x4602）",
        );
    }

    // ------------------------------------------------------------------
    // C06-NORM：五元组归一（归一册冻结映射）
    // ------------------------------------------------------------------

    // 不合规发现项：位置=上游条目号、证据=实测原样、建议含期望与动作。
    {
        let (mut e, inputs, reg) = corpus(true,false); // PALETTE 违规
        let mut plugins = PluginRegistry::new();
        let impls: Vec<&dyn ScannerPlugin> = vec![&TruthPlugin {
            name: "t1",
            fault: None,
            steps: None,
            palette_value: true,
        }];
        let req = ScanRequest {
            mode: ScanMode::Full,
            ..ScanRequest::default()
        };
        let rep = scan(&mut e, &reg, &mut plugins, &impls, req, &inputs);
        let f = rep
            .findings
            .iter()
            .find(|f| f.rule == "U06-RULE-PALETTE")
            .expect("palette 违规必须在场");
        set.add(
            "C06-NORM-01",
            f.location == "VE-F3983" && f.evidence == "主色偏差"
                && f.suggestion.contains("主色为令牌主色") && f.suggestion.contains("修复")
                && f.severity == Severity::Warning && f.occurrences == 1 && !f.is_a11y,
            "五元组：位置/规则/严重度/证据原样/建议逐字段对拍",
        );
    }

    // 引用断：证据被册替换（0x4606）+ 严重度归一册钉死阻断。
    {
        let (mut e, mut inputs, reg) = corpus(false,false);
        // 抽走 FOCUS 的正文 → 引用断（NotFound）。
        inputs.remove(0);
        let mut plugins = PluginRegistry::new();
        let impls: Vec<&dyn ScannerPlugin> = vec![&TruthPlugin {
            name: "t1",
            fault: None,
            steps: None,
            palette_value: true,
        }];
        let req = ScanRequest {
            mode: ScanMode::Full,
            ..ScanRequest::default()
        };
        let rep = scan(&mut e, &reg, &mut plugins, &impls, req, &inputs);
        let brk = rep
            .findings
            .iter()
            .find(|f| f.rule == "U06-RULE-FOCUS")
            .expect("引用断必须在场");
        set.add(
            "C06-NORM-02",
            brk.severity == Severity::Blocking
                && brk.evidence.contains("引用断")
                && rep.codes.contains(&ScanCode::EVIDENCE_SUBSTITUTED),
            "引用断五元组：证据为册内断因说明（替换面记账）+严重度钉死阻断",
        );
    }

    // ------------------------------------------------------------------
    // C06-AGG：聚合去重
    // ------------------------------------------------------------------

    // 同键合并：计数/严重度取高/a11y 粘滞/证据保首见。
    {
        let mk = |sev: Severity, a11y: bool, ev: &str| NormalizedFinding {
            location: "L1".to_string(),
            rule: "R1".to_string(),
            severity: sev,
            evidence: ev.to_string(),
            suggestion: "s".to_string(),
            is_a11y: a11y,
            occurrences: 1,
        };
        let raw = vec![
            mk(Severity::Warning, false, "first"),
            mk(Severity::Blocking, false, "second"),
            mk(Severity::Warning, true, "third"),
        ];
        let (agg, storm) = aggregate(raw);
        set.add(
            "C06-AGG-01",
            agg.len() == 1 && agg[0].occurrences == 3 && agg[0].severity == Severity::Blocking
                && agg[0].is_a11y && agg[0].evidence == "first" && !storm,
            "同键三条合并：计数 3/严重度取高/a11y 粘滞/证据保首见",
        );
    }

    // 风暴：超阈值聚合生效 + 记账；聚合本身 O(发现) 承接风暴。
    {
        let mk = |i: usize| NormalizedFinding {
            location: alloc::format!("L{}", i % 7),
            rule: alloc::format!("R{}", i % 3),
            severity: Severity::Warning,
            evidence: "e".to_string(),
            suggestion: "s".to_string(),
            is_a11y: false,
            occurrences: 1,
        };
        let raw: Vec<NormalizedFinding> = (0..STORM_THRESHOLD + 100).map(mk).collect();
        let (agg, storm) = aggregate(raw);
        set.add(
            "C06-AGG-02",
            storm && agg.len() == 21 && agg.iter().all(|f| f.occurrences >= 2),
            "风暴语料 612→21 条（7 位置×3 规则），聚合承接 + 风暴标记",
        );
    }

    // 确定性双跑：同语料两次扫描的报告发现项逐位一致。
    {
        let run_once = || {
            let (mut e, inputs, reg) = corpus(true,false);
            let mut plugins = PluginRegistry::new();
            let impls: Vec<&dyn ScannerPlugin> = vec![&TruthPlugin {
                name: "t1",
                fault: None,
                steps: None,
                palette_value: true,
            }];
            let req = ScanRequest {
                mode: ScanMode::Full,
                ..ScanRequest::default()
            };
            let rep = scan(&mut e, &reg, &mut plugins, &impls, req, &inputs);
            (rep.findings, rep.rule_digest)
        };
        let (a, da) = run_once();
        let (b, db) = run_once();
        set.add(
            "C06-AGG-03",
            a == b && da == db,
            "同输入双跑发现项与摘要逐位一致（确定性口径延续）",
        );
    }

    // ------------------------------------------------------------------
    // C06-SEG / C06-CI / C06-PRIV：加固判据
    // ------------------------------------------------------------------

    {
        let all = [
            ScanCode::PLUGIN_QUARANTINED,
            ScanCode::PREDICATE_CONFLICT,
            ScanCode::DRIFT_RECONCILED,
            ScanCode::A11Y_UNION_INJECTED,
            ScanCode::STORM_AGGREGATED,
            ScanCode::EVIDENCE_SUBSTITUTED,
            ScanCode::IFACE_REJECTED,
            ScanCode::DUPLICATE_PLUGIN,
            ScanCode::STEP_BUDGET_BREACH,
            ScanCode::DIGEST_RECORDED,
        ];
        let seg_ok = all.iter().all(|c| (c.0 & 0xFF00) == ScanCode::SEGMENT << 8);
        let mut distinct = true;
        for i in 0..all.len() {
            for j in (i + 1)..all.len() {
                distinct &= all[i].0 != all[j].0;
            }
        }
        let explains = all.iter().all(|c| !c.explain().is_empty());
        set.add(
            "C06-SEG-01",
            seg_ok && distinct && explains && all.len() == 10
                && ScanCode::SEGMENT == 0x46 && 0x46 != 0x39 && 0x46 != 0x37,
            "十码全 0x46 段互异、解释齐备、与邻居段（0x39/0x37）字面不等",
        );
    }

    {
        let (mut e, inputs, reg) = corpus(false,false);
        let mut plugins = PluginRegistry::new();
        let impls: Vec<&dyn ScannerPlugin> = vec![&TruthPlugin {
            name: "t1",
            fault: None,
            steps: None,
            palette_value: true,
        }];
        let req = ScanRequest {
            mode: ScanMode::Full,
            ..ScanRequest::default()
        };
        let rep = scan(&mut e, &reg, &mut plugins, &impls, req, &inputs);
        let line = rep.ci_line();
        set.add(
            "C06-CI-01",
            line.contains("mode=full") && line.contains("raw=0") && line.contains("agg=0")
                && line.contains("CLEAN") && !rep.rule_digest.is_empty()
                && rep.codes.contains(&ScanCode::DIGEST_RECORDED),
            "CI 单行含模式/计数/结论/摘要（F4241 与 CI 联动承载面）",
        );
    }

    {
        // 隐私红线（结构面）：请求/查询/报告的字段闭集由构造器钉死——
        // ScanRequest 默认构造只含调度语义字段，插件往返只有规则码与谓词布尔。
        let req = ScanRequest::default();
        let q = PluginQuery {
            rule_codes: vec![],
            step_budget: PLUGIN_STEP_BUDGET,
        };
        let r = PluginReport::default();
        set.add(
            "C06-PRIV-01",
            req.changed.is_empty() && req.targets.is_empty() && req.base_digest.is_none()
                && q.rule_codes.is_empty() && r.bindings.is_empty() && r.declared_steps == 0
                && PLUGIN_STEP_BUDGET > 0,
            "调度/插件面字段闭集（无用户数据入参——扫描不读用户数据的类型形状）",
        );
    }

    // 归一册建议模板两向（Fix/Block）都含期望值——模板漏期望即红。
    {
        let f1 = NormRegister::suggestion(Action::Fix, "E1");
        let f2 = NormRegister::suggestion(Action::Block, "E2");
        set.add(
            "C06-NORM-03",
            f1.contains("E1") && f2.contains("E2") && f1 != f2,
            "建议模板双向（修复/阻断）各含期望值且互异",
        );
    }

    set
}
