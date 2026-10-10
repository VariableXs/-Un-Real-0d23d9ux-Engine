//! VE-F4206 · 一致性扫描平台（VE-U 域 · 一致性域 · U06）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4206`
//!
//! **判据（锚点原文）**：三模式、插件化、五元组、永不跳过、聚合去重、判据。
//!
//! **职责定位（锚点原文）**：一致性扫描平台（扫描调度（全量/增量/定向三模式）
//! + 扫描器插件化（规则引擎驱动）+ 结果归一（发现项五元组：位置/规则/严重度/
//! 证据/建议））。
//!
//! **数据结构（锚点原文·家族格式）**：扫描平台（三模式）；插件接口（冻结）；归一册。
//!
//! **错误路径与降级矩阵（锚点原文）**：插件崩溃→隔离+继续扫描；结果风暴→聚合
//! 去重；调度漂移→对版。
//!
//! **性能逐项分解（锚点原文）**：扫描 O(目标)；隔离 O(插件)；聚合 O(发现)。
//!
//! **跨批对接点（锚点原文）**：F4204 规则单源；F4241 验证平台衔接；CI 联动。
//!
//! **无障碍与隐私（锚点原文）**：无障碍规则扫描永不跳过（三模式均含——域本色）；
//! 扫描不读用户数据（红线）。
//!
//! # 一、三模式的「永不跳过」是怎么做成结构事实的
//!
//! 「无障碍规则扫描永不跳过（三模式均含）」如果靠调用方自觉，增量与定向模式
//! 迟早把 a11y 规则漏在白名单外——而且漏了不会有人发现。本平台把并集做进
//! **调度器本体**：增量/定向模式在把范围交给规则引擎**之前**，先无条件并入
//! 全部 `is_a11y` 规则码（[`ScanReport::a11y_injected`] 记账，0x4604）。
//! 于是「某模式下 a11y 规则没被扫」在类型上不可表达——调用方就算把白名单
//! 写成空的，a11y 规则照样全量入场。这是域本色主场的结构性兑现，不是承诺。
//!
//! # 二、插件崩溃在 no_std 里如何建模
//!
//! 内核 panic=abort，真崩溃无法捕获——「插件崩溃→隔离+继续扫描」建模为
//! **协作式故障协议**：插件返回 [`Result::Err`][Result]（[`PluginFault`]：
//! 崩溃/超步算/违约三型），平台收到即把该插件关进隔离册（[`PluginRegistry`]
//! 的 quarantine 面），本轮其余插件照常供给谓词绑定，扫描继续。挂起防护走
//! **步算预算**（no_std 无墙钟，与 F1810/F2407 同纪律：可机检的先机检）——
//! 插件申报的步算超预算按故障处置。隔离 O(插件)：隔离动作是查表+置位，
//! 不触碰其他插件的任何状态。
//!
//! # 三、调度漂移→对版
//!
//! 规则册不是恒定的：F4204 引擎可登记/注销规则。增量/定向模式拿的是「变更集/
//! 目标集 + 基线版摘要」——若基线摘要与当前规则册实算摘要不符（[`rule_digest`]），
//! 说明调度依据已漂移，此时**升格为全量重扫**（对版的安全语义：拿旧调度单跑
//! 新规则册，漏扫比多扫贵得多）并记 0x4603。全量模式无需基线，同时把本次摘要
//! 写进报告供下轮当基线（[`ScanReport::rule_digest`]）。
//!
//! # 四、结果归一与聚合去重
//!
//! 规则引擎的 [`Finding`][veu04_engine::Finding] 是引擎视角（实测/期望/处置），
//! 平台视角需要五元组：**位置/规则/严重度/证据/建议**（[`NormalizedFinding`]）。
//! 映射关系冻结在归一册（[`NormRegister`]）里：位置=规则来源引用的上游条目号
//! （被测对象住在哪）；证据=实测正文原样（不改写不截断）；建议=处置+期望生成的
//! 人话；未知 kind 走显性兜底（0x4606）不静默。**结果风暴→聚合去重**：同
//! （规则，位置）键合并计数、严重度取最高、证据保首见、a11y 标记粘滞
//! （[`aggregate`]），BTreeMap 有序输出保证确定性——同输入两次扫描的归一册
//! 逐位一致（J 域确定性口径在 U 域的延续）。
//!
//! # 五、隐私红线（结构性）
//!
//! [`PluginQuery`]/[`PluginReport`]/[`NormalizedFinding`] 的字段是**闭集**：
//! 只有规则码、谓词名、布尔真值、来源条目号与正文——没有任何用户数据字段，
//! 也没有未来加字段的口子（加字段 = 改冻结接口 = 走 ADR）。扫描不读用户数据
//! 由类型形状保证，不靠纪律条款。

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::veu01_arch::{fnv1a64_hex, Severity};
use super::veu03_registry::ContractRegistry;
use super::veu04_engine::{
    Action, Coverage, FindingKind, PredicateBindings, RuleEngine, RuleInputs,
};

// ---------------------------------------------------------------------------
// 一、诊断码（独占 0x46 段）
// ---------------------------------------------------------------------------

/// 扫描平台诊断码（独占 `0x46xx` 段；与 0x39 F1811 / 0x45 / 0x47 邻段不重叠）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScanCode(pub u16);

impl ScanCode {
    /// 插件故障被隔离（扫描继续）。
    pub const PLUGIN_QUARANTINED: ScanCode = ScanCode(0x4601);
    /// 谓词冲突：两插件对同一谓词给出不同真值（保首见 + 记账）。
    pub const PREDICATE_CONFLICT: ScanCode = ScanCode(0x4602);
    /// 调度漂移对版：基线摘要与规则册不符，已升格全量重扫。
    pub const DRIFT_RECONCILED: ScanCode = ScanCode(0x4603);
    /// 无障碍并集注入（永不跳过的记账面）。
    pub const A11Y_UNION_INJECTED: ScanCode = ScanCode(0x4604);
    /// 结果风暴聚合生效。
    pub const STORM_AGGREGATED: ScanCode = ScanCode(0x4605);
    /// 证据被归一册替换为断因说明（非实测原样——消费端据此分辨证据性质）。
    pub const EVIDENCE_SUBSTITUTED: ScanCode = ScanCode(0x4606);
    /// 插件接口版本不符被拒（冻结接口的版本闸）。
    pub const IFACE_REJECTED: ScanCode = ScanCode(0x4607);
    /// 重名插件被拒。
    pub const DUPLICATE_PLUGIN: ScanCode = ScanCode(0x4608);
    /// 插件申报步算超预算（挂起防护，按故障处置）。
    pub const STEP_BUDGET_BREACH: ScanCode = ScanCode(0x4609);
    /// 规则册摘要已记账（供下轮当基线）。
    pub const DIGEST_RECORDED: ScanCode = ScanCode(0x460A);

    /// 段标识。
    pub const SEGMENT: u16 = 0x46;

    /// 人话解释（无障碍：文本规范可读）。
    pub const fn explain(self) -> &'static str {
        match self.0 {
            0x4601 => "a scanner plugin faulted and was quarantined; the scan continued with the rest",
            0x4602 => "two plugins disagree on one predicate; first report wins and the conflict is on record",
            0x4603 => "schedule drift detected: baseline digest mismatches the rulebook; escalated to a full rescan",
            0x4604 => "accessibility rules were force-unioned into the scan scope (never-skipped is structural)",
            0x4605 => "a finding storm was aggregated and deduplicated",
            0x4606 => "evidence was substituted from the normalization register (cause note, not raw measurement)",
            0x4607 => "a plugin was rejected: its interface version does not match the frozen v1 contract",
            0x4608 => "a plugin was rejected: the name is already registered",
            0x4609 => "a plugin declared step usage beyond the budget; treated as a fault",
            0x460A => "the rulebook digest was recorded for the next scheduling round",
            _ => "unknown scan diagnostic code",
        }
    }
}

// ---------------------------------------------------------------------------
// 二、三模式与调度请求
// ---------------------------------------------------------------------------

/// 扫描调度三模式（锚点：全量/增量/定向）。
///
/// Default=全量（调度缺省语义：没有依据降范围时，全量是唯一安全缺省）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScanMode {
    /// 全量：规则册全扫（O(规则)）。
    Full,
    /// 增量：只跑变更集（O(变更)）——a11y 并集仍强制入场。
    Incremental,
    /// 定向：只跑指定目标（O(目标)）——a11y 并集仍强制入场。
    Targeted,
}

impl Default for ScanMode {
    fn default() -> Self {
        ScanMode::Full
    }
}

impl ScanMode {
    /// 规范名。
    pub const fn name(self) -> &'static str {
        match self {
            ScanMode::Full => "full",
            ScanMode::Incremental => "incremental",
            ScanMode::Targeted => "targeted",
        }
    }
}

/// 调度请求（字段闭集：无用户数据字段——隐私红线，见头注§五）。
#[derive(Clone, Debug, Default)]
pub struct ScanRequest {
    /// 请求模式。
    pub mode: ScanMode,
    /// 增量模式：变更规则码集（引擎侧对未知码显性登记，不静默）。
    pub changed: Vec<String>,
    /// 定向模式：目标规则码集。
    pub targets: Vec<String>,
    /// 调度基线摘要（上轮 [`ScanReport::rule_digest`]；增量/定向必填——
    /// 缺失或不符即漂移，升格全量对版）。
    pub base_digest: Option<String>,
}

// ---------------------------------------------------------------------------
// 三、插件接口（冻结 v1）与隔离册
// ---------------------------------------------------------------------------

/// 插件接口版本（冻结 v1；变更走 ADR，版本闸在 [`PluginRegistry::register`]）。
pub const PLUGIN_IFACE_VERSION: u32 = 1;

/// 插件查询（字段闭集：规则码 + 步算预算——无用户数据）。
#[derive(Clone, Debug)]
pub struct PluginQuery {
    /// 本轮扫描范围（已含 a11y 并集）的规则码。
    pub rule_codes: Vec<String>,
    /// 步算预算（协作式挂起防护：申报超预算按故障处置）。
    pub step_budget: u32,
}

/// 插件报告（字段闭集：谓词绑定 + 步算申报——无用户数据）。
#[derive(Clone, Debug, Default)]
pub struct PluginReport {
    /// 谓词真值绑定（谓词名 → 本插件实测的条件真值）。
    pub bindings: PredicateBindings,
    /// 申报步算（真实工作单元计数，超 [`PluginQuery::step_budget`] 即故障）。
    pub declared_steps: u32,
}

/// 插件故障（崩溃协议的载体；no_std panic=abort 下真崩溃以故障返回建模）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PluginFault {
    /// 崩溃（插件自报不可恢复错误）。
    Crashed,
    /// 步算超预算（挂起防护）。
    OverBudget,
    /// 违约（报告形状不满足冻结契约）。
    Violated,
}

impl PluginFault {
    /// 中文名（隔离册读屏用）。
    pub const fn zh(self) -> &'static str {
        match self {
            PluginFault::Crashed => "崩溃",
            PluginFault::OverBudget => "步算超预算",
            PluginFault::Violated => "契约违约",
        }
    }
}

/// 扫描器插件接口（**冻结 v1**：两签名，加签名=破坏冻结走 ADR）。
pub trait ScannerPlugin {
    /// 插件名（注册册唯一键）。
    fn name(&self) -> &'static str;

    /// 对查询范围执行观测：产出谓词绑定；故障走 Err（隔离协议）。
    fn observe(&self, query: &PluginQuery) -> Result<PluginReport, PluginFault>;
}

/// 隔离册条目（谁、因何、何时被隔离）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuarantineEntry {
    /// 插件名。
    pub plugin: &'static str,
    /// 故障型。
    pub fault: PluginFault,
    /// 人话说明。
    pub note: String,
}

/// 插件注册册（注册/隔离/存活查询；隔离 O(插件)=查表+置位）。
#[derive(Default)]
pub struct PluginRegistry {
    active: Vec<&'static str>,
    quarantined: Vec<QuarantineEntry>,
    iface_version: u32,
}

impl PluginRegistry {
    /// 空册（接口版本=冻结 v1）。
    pub fn new() -> PluginRegistry {
        PluginRegistry {
            active: Vec::new(),
            quarantined: Vec::new(),
            iface_version: PLUGIN_IFACE_VERSION,
        }
    }

    /// 注册插件（重名拒 0x4608；接口版本不符拒 0x4607——版本闸是插件侧
    /// 声明的 `declared_iface` 与册版本比对）。
    pub fn register(
        &mut self,
        declared_iface: u32,
        name: &'static str,
        codes: &mut Vec<ScanCode>,
    ) -> Result<(), ()> {
        if declared_iface != self.iface_version {
            codes.push(ScanCode::IFACE_REJECTED);
            return Err(());
        }
        if self.active.contains(&name) || self.quarantined.iter().any(|q| q.plugin == name) {
            codes.push(ScanCode::DUPLICATE_PLUGIN);
            return Err(());
        }
        self.active.push(name);
        Ok(())
    }

    /// 隔离（幂等：重复故障不重复入册，但刷新说明）。
    pub fn quarantine(&mut self, name: &'static str, fault: PluginFault, note: String) {
        if let Some(q) = self.quarantined.iter_mut().find(|q| q.plugin == name) {
            q.note = note;
            return;
        }
        self.active.retain(|n| *n != name);
        self.quarantined.push(QuarantineEntry {
            plugin: name,
            fault,
            note,
        });
    }

    /// 存活插件名表。
    pub fn active(&self) -> &[&'static str] {
        &self.active
    }

    /// 隔离册只读。
    pub fn quarantined(&self) -> &[QuarantineEntry] {
        &self.quarantined
    }
}

// ---------------------------------------------------------------------------
// 四、归一册（冻结映射）与五元组
// ---------------------------------------------------------------------------

/// 归一五元组（锚点：位置/规则/严重度/证据/建议）+ 聚合附元。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NormalizedFinding {
    /// 位置：规则来源引用的上游条目号（被测对象住在哪）。
    pub location: String,
    /// 规则码。
    pub rule: String,
    /// 严重度。
    pub severity: Severity,
    /// 证据：实测正文原样（不改写不截断；引用断时为断因说明）。
    pub evidence: String,
    /// 建议：处置+期望生成的人话（归一册模板）。
    pub suggestion: String,
    /// 是否无障碍发现项（聚合粘滞：任一见项即真）。
    pub is_a11y: bool,
    /// 聚合计数（同键发现项数；未聚合=1）。
    pub occurrences: u32,
}

/// 证据归一结果（Raw=实测正文原样；Fixed=归一册断因说明）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NormEvidence {
    /// 实测正文原样（不改写）。
    Raw,
    /// 归一册固定断因说明。
    Fixed(&'static str),
}

/// 归一册（kind→模板的冻结映射；kind 集合对账由穷尽 match 钉死）。
pub struct NormRegister;

impl NormRegister {
    /// 建议：处置段（Fix/Block 两向）+ 期望值 → 人话。
    pub fn suggestion(action: Action, expect: &str) -> String {
        match action {
            Action::Fix => format!("修复：对齐期望「{}」", expect),
            Action::Block => format!("阻断：须满足「{}」方可开工", expect),
        }
    }

    /// 证据归一：实测原样（Raw）或册内断因说明（Fixed）。
    /// FindingKind 是封闭枚举，漏 kind = 编译错——归一册与引擎 kind 集合
    /// 的对账由类型系统钉死，不靠人记。
    pub fn evidence(kind: FindingKind, _actual: &str) -> NormEvidence {
        match kind {
            FindingKind::Mismatch => NormEvidence::Raw,
            FindingKind::BrokenRef => NormEvidence::Fixed("引用断：判据正文取不到或上游已改，本项结论不可信"),
            FindingKind::UnknownRule => NormEvidence::Fixed("调度名单含册外规则码"),
        }
    }

    /// 严重度归一：引用断/未知规则恒阻断（判据不可信比不合规更严重），
    /// 不合规沿用引擎判定的严重度。
    pub const fn severity(kind: FindingKind, engine_severity: Severity) -> Severity {
        match kind {
            FindingKind::Mismatch => engine_severity,
            // UnknownRule / BrokenRef 归一册钉死阻断（不可降级）。
            FindingKind::BrokenRef => Severity::Blocking,
            FindingKind::UnknownRule => Severity::Blocking,
        }
    }
}

// ---------------------------------------------------------------------------
// 五、扫描编排（三模式调度 + 插件驱动 + 归一 + 聚合）
// ---------------------------------------------------------------------------

/// 规则册摘要（对版锚）：对全部（规则码, 来源内容哈希）做 FNV-1a。
///
/// 规则增删或上游换版都会移动摘要——增量/定向模式据此发现调度漂移。
pub fn rule_digest(engine: &RuleEngine) -> String {
    let mut buf: Vec<u8> = Vec::new();
    for r in engine.rules() {
        buf.extend_from_slice(r.code.as_bytes());
        buf.push(0x1F);
        buf.extend_from_slice(r.origin.content_hash.as_bytes());
        buf.push(0x1E);
    }
    fnv1a64_hex(&buf)
}

/// 范围解析结果（调度器内部产物）。
struct Scope {
    codes: Vec<String>,
    a11y_injected: usize,
    escalated_to_full: bool,
}

/// 解析扫描范围：三模式分派 + a11y 并集注入 + 漂移对版。
fn resolve_scope(engine: &RuleEngine, req: &ScanRequest, codes: &mut Vec<ScanCode>) -> Scope {
    let a11y: Vec<String> = engine
        .rules()
        .iter()
        .filter(|r| r.is_a11y)
        .map(|r| r.code.clone())
        .collect();
    let (mut list, escalated) = match req.mode {
        ScanMode::Full => (engine.rules().iter().map(|r| r.code.clone()).collect(), false),
        ScanMode::Incremental | ScanMode::Targeted => {
            // 调度漂移→对版：非全量模式必须带基线摘要且与实算一致；
            // 不符/缺失 = 调度依据已过期 → 升格全量（安全语义：漏扫比多扫贵）。
            let current = rule_digest(engine);
            let drifted = match &req.base_digest {
                Some(b) => *b != current,
                None => true,
            };
            if drifted {
                codes.push(ScanCode::DRIFT_RECONCILED);
                (engine.rules().iter().map(|r| r.code.clone()).collect(), true)
            } else {
                let src: &Vec<String> = if req.mode == ScanMode::Incremental {
                    &req.changed
                } else {
                    &req.targets
                };
                (src.clone(), false)
            }
        }
    };
    // 去重（保序）：O(范围)。
    let mut seen: Vec<String> = Vec::new();
    list.retain(|c| {
        if seen.contains(c) {
            false
        } else {
            seen.push(c.clone());
            true
        }
    });
    // a11y 并集注入（永不跳过的结构兑现）：缺谁补谁，注入量记账。
    let mut injected = 0usize;
    for a in a11y {
        if !seen.contains(&a) {
            list.push(a.clone());
            seen.push(a);
            injected += 1;
        }
    }
    if injected > 0 {
        codes.push(ScanCode::A11Y_UNION_INJECTED);
    }
    Scope {
        codes: list,
        a11y_injected: injected,
        escalated_to_full: escalated,
    }
}

/// 插件驱动阶段：跑全部存活插件，收集谓词绑定，故障即隔离。
///
/// 谓词冲突（两插件对同一谓词给不同真值）保首见并记 0x4602——不静默吞，
/// 也不崩溃重扫（冲突是数据问题，隔离语义留给故障路径）。
pub fn run_plugins(
    plugins: &mut PluginRegistry,
    implementations: &[&dyn ScannerPlugin],
    query: &PluginQuery,
    bindings: &mut PredicateBindings,
    codes: &mut Vec<ScanCode>,
) -> usize {
    let mut faulted = 0usize;
    for imp in implementations {
        let name = imp.name();
        if !plugins.active().contains(&name) {
            continue; // 已隔离：跳过（隔离 O(插件)=查表）。
        }
        match imp.observe(query) {
            Ok(rep) => {
                if rep.declared_steps > query.step_budget {
                    plugins.quarantine(
                        name,
                        PluginFault::OverBudget,
                        format!("declared {} steps over budget {}", rep.declared_steps, query.step_budget),
                    );
                    codes.push(ScanCode::STEP_BUDGET_BREACH);
                    codes.push(ScanCode::PLUGIN_QUARANTINED);
                    faulted += 1;
                    continue;
                }
                for (pred, v) in rep.bindings {
                    if let Some(existing) = bindings.iter_mut().find(|(p, _)| *p == pred) {
                        if existing.1 != v {
                            // 冲突：保首见 + 记账（不静默不崩溃）。
                            codes.push(ScanCode::PREDICATE_CONFLICT);
                        }
                    } else {
                        bindings.push((pred, v));
                    }
                }
            }
            Err(fault) => {
                plugins.quarantine(
                    name,
                    fault.clone(),
                    format!("plugin reported fault: {}", fault.zh()),
                );
                codes.push(ScanCode::PLUGIN_QUARANTINED);
                faulted += 1;
            }
        }
    }
    faulted
}

/// 聚合去重（结果风暴的承接面；O(发现)——单趟 BTreeMap 归并）。
///
/// 键=（规则码, 位置）；计数合并、严重度取最高（Blocking>Warning）、
/// 证据保首见、a11y 粘滞。BTreeMap 有序输出 ⇒ 同输入两次聚合逐位一致。
pub fn aggregate(findings: Vec<NormalizedFinding>) -> (Vec<NormalizedFinding>, bool) {
    let storm = findings.len() > STORM_THRESHOLD;
    let mut map: BTreeMap<(String, String), NormalizedFinding> = BTreeMap::new();
    for f in findings {
        let key = (f.rule.clone(), f.location.clone());
        match map.get_mut(&key) {
            Some(slot) => {
                slot.occurrences += 1;
                if slot.severity == Severity::Warning && f.severity == Severity::Blocking {
                    slot.severity = Severity::Blocking;
                }
                slot.is_a11y |= f.is_a11y;
            }
            None => {
                map.insert(key, f);
            }
        }
    }
    (map.into_values().collect(), storm)
}

/// 风暴阈值：原始发现项超此数记风暴码（聚合照做——聚合就是承接手段）。
pub const STORM_THRESHOLD: usize = 512;

/// 扫描报告（对接 F4241 验证平台与 CI 的承载面）。
#[derive(Clone, Debug)]
pub struct ScanReport {
    /// 生效模式（漂移对版后可能与请求模式不同——升格全量如实记账）。
    pub mode_effective: ScanMode,
    /// 引擎覆盖声明（全量/仅变更——透传自 F4204 Verdict）。
    pub coverage_label: &'static str,
    /// 归一+聚合后的发现项（确定性序）。
    pub findings: Vec<NormalizedFinding>,
    /// 原始发现项数（聚合前）。
    pub raw_count: usize,
    /// 聚合后条目数。
    pub aggregated_count: usize,
    /// 是否风暴（原始数超 [`STORM_THRESHOLD`]）。
    pub storm: bool,
    /// 本轮被隔离的插件数。
    pub quarantined_now: usize,
    /// a11y 并集注入条数（0=范围本就全含；非全量模式恒 >0 或 0 且全含）。
    pub a11y_injected: usize,
    /// 是否发生了漂移升格。
    pub escalated: bool,
    /// 规则册摘要（本轮实算；下轮调度基线）。
    pub rule_digest: String,
    /// 实际执行规则数 / 条件跳过数（透传）。
    pub executed: usize,
    pub skipped: usize,
    /// 诊断码账（本报告全部事件，判据逐条对拍）。
    pub codes: Vec<ScanCode>,
}

impl ScanReport {
    /// CI 联动单行（人话、可 grep、不替代结构化报告）。
    pub fn ci_line(&self) -> String {
        format!(
            "scan[{}] mode={} raw={} agg={} quarantined={} a11y+={} escalated={} digest={}",
            if self.findings.is_empty() { "CLEAN" } else { "FINDINGS" },
            self.mode_effective.name(),
            self.raw_count,
            self.aggregated_count,
            self.quarantined_now,
            self.a11y_injected,
            self.escalated,
            &self.rule_digest[..16.min(self.rule_digest.len())],
        )
    }
}

/// 扫描主入口：三模式调度 → 插件驱动 → 引擎执行 → 归一 → 聚合。
///
/// `plugins` 是注册册（隔离状态跨轮保持）；`implementations` 是本轮可用的
/// 插件实现表（已被隔离的自动跳过）。
pub fn scan(
    engine: &mut RuleEngine,
    registry: &ContractRegistry,
    plugins: &mut PluginRegistry,
    implementations: &[&dyn ScannerPlugin],
    req: ScanRequest,
    inputs: &RuleInputs,
) -> ScanReport {
    let mut codes: Vec<ScanCode> = Vec::new();

    // 一、调度（三模式 + a11y 并集 + 漂移对版）。
    let scope = resolve_scope(engine, &req, &mut codes);
    let mode_effective = if scope.escalated_to_full {
        ScanMode::Full
    } else {
        req.mode
    };

    // 二、插件驱动（谓词绑定供给方；崩溃→隔离+继续）。
    // 未登记的实现自动入册：同 crate 编译即面对同一冻结接口常量，登记的
    // 版本闸在构造上不可能拒绝（重名已被隔离/在册检查滤除）——唯一可达
    // 的拒绝路径不存在，故 let _ 是被注释证实的不可达，不是静默。
    for imp in implementations {
        let name = imp.name();
        let known = plugins.active().contains(&name)
            || plugins.quarantined().iter().any(|q| q.plugin == name);
        if !known {
            let _ = plugins.register(PLUGIN_IFACE_VERSION, name, &mut codes);
        }
    }
    let mut bindings: PredicateBindings = Vec::new();
    let query = PluginQuery {
        rule_codes: scope.codes.clone(),
        step_budget: PLUGIN_STEP_BUDGET,
    };
    let quarantined_now = run_plugins(plugins, implementations, &query, &mut bindings, &mut codes);

    // 三、引擎执行（F4204 单源：全量走 run_full，其余走增量白名单通道）。
    let verdict = if mode_effective == ScanMode::Full {
        engine.run_full(inputs, registry, &bindings)
    } else {
        engine.run_incremental(&scope.codes, inputs, registry, &bindings)
    };

    // 四、归一（五元组；未知 kind 显性兜底）。
    let mut raw: Vec<NormalizedFinding> = Vec::new();
    for f in &verdict.findings {
        let (location, is_a11y) = match engine.rule(&f.rule) {
            Some(r) => (r.origin.upstream.clone(), r.is_a11y),
            // 未知规则（增量白名单错字）无册可查：位置=规则码自身，
            // a11y 标记以发现项自带为准（引擎侧恒 false，如实透传）。
            None => (f.rule.clone(), f.is_a11y),
        };
        // 证据归一：Mismatch=实测正文原样（主路径，不替换）；
        // 引用断/未知规则=归一册断因说明（替换面，0x4606 记账——
        // 消费端据此分辨「证据是实测」还是「证据是断因」）。
        let (evidence, substituted) = match NormRegister::evidence(f.kind, &f.actual) {
            NormEvidence::Raw => (f.actual.clone(), false),
            NormEvidence::Fixed(t) => (t.to_string(), true),
        };
        if substituted {
            codes.push(ScanCode::EVIDENCE_SUBSTITUTED);
        }
        raw.push(NormalizedFinding {
            location,
            rule: f.rule.clone(),
            severity: NormRegister::severity(f.kind, f.severity),
            evidence,
            suggestion: NormRegister::suggestion(f.action, &f.expect),
            is_a11y,
            occurrences: 1,
        });
    }
    let raw_count = raw.len();

    // 五、聚合去重（风暴承接）。
    let (findings, storm) = aggregate(raw);
    let aggregated_count = findings.len();
    if storm {
        codes.push(ScanCode::STORM_AGGREGATED);
    }

    // 六、摘要记账（下轮调度基线）。
    let digest = rule_digest(engine);
    codes.push(ScanCode::DIGEST_RECORDED);

    ScanReport {
        mode_effective,
        coverage_label: match verdict.coverage {
            Coverage::Full => "full",
            Coverage::Incremental => "incremental(partial)",
        },
        findings,
        raw_count,
        aggregated_count,
        storm,
        quarantined_now,
        a11y_injected: scope.a11y_injected,
        escalated: scope.escalated_to_full,
        rule_digest: digest,
        executed: verdict.executed,
        skipped: verdict.skipped,
        codes,
    }
}

/// 插件步算预算（no_std 无墙钟，协作式步算口径——头注§二）。
pub const PLUGIN_STEP_BUDGET: u32 = 4096;

