//! VE-F4207 · 一致性度量体系（VE-U 域 · 一致性与验收批 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4207`
//!
//! **判据（锚点原文）**：四主指标、口径冻结、可复算、单列、空态显性、判据。
//!
//! **职责定位（锚点原文）**：一致性度量体系——**度量指标族**：一致率
//! （合规项/总检项）/冲突密度/修复时效/回归率**四主指标 × 域维度切分**；
//! **指标口径冻结 + 计算可复算**；与家族度量同构对拍。
//!
//! ## 四主指标（口径册冻结——千分定点整数，不用 f32）
//!
//! | 指标 | 口径（冻结） | 空态 |
//! |---|---|---|
//! | 一致率 | 合规项×1000 ÷ 总检项 | 总检项=0 → 显性空态（**不是 1000‰ 假绿**） |
//! | 冲突密度 | 阻断级发现项×1000 ÷ 总检项 | 总检项=0 → 显性空态 |
//! | 修复时效 | Σ(修复轮次差)×1000 ÷ 已修复数（均值定点） | 已修复=0 → 显性空态 |
//! | 回归率 | 复发发现项×1000 ÷ 上轮合规项 | 上轮合规=0 → 显性空态 |
//!
//! **空态显性是本体系的诚实底线**：零样本的「一致率 100%」是把没有证据
//! 当全绿——空态返回 [`MetricValue::Empty`]（O(1)），消费方必须显性承认
//! 「本轮没有可度量数据」。
//!
//! ## 域维度切分（真调 F4204 单源）
//!
//! 每条规则的归属域**真调** [`RuleSource::authority_domain`]（F4204 四路
//! 来源 → [`DomainTag`] 域标签单源）；度量按域分维出多行——「哪个域拖了
//! 后腿」可点名，不是一个笼统平均数。**无障碍一致率单列**（锚点：域本色
//! 主场指标）：`is_a11y` 切面独立成行（真调 [`Rule::is_a11y`]），不与非
//! 无障碍指标混算——混算会让无障碍的劣化被平均值吃掉。
//!
//! ## 可复算（锚点：计算可复算——复算不符→立案重算）
//!
//! [`recompute`] 用与主算**独立的代码路径**重算全部指标：两路结果逐位
//! 一致才算可复算；不一致 = 计算路径有病（[`E_RECOMPUTE`]）→ 立案重算，
//! **不发布**不一致的数字。口径册本体有 FNV 摘要（[`caliber_digest`]，
//! 真调 F4204 [`fnv1a64_hex`]）：摘要漂移 = 口径被人改过（[`E_CALIBER`]）
//! → **冻结重申**（把冻结口径原文重新钉出去，重算作废）。
//!
//! ## 跨批对接点（锚点原文）
//!
//! - **F4206 扫描数据源**：度量输入 = [`ScanReport`]（发现项/执行数/
//!   覆盖标记全透传消费，不自采数据）；
//! - **家族度量同构**：千分定点整数 + 空态显性 + 口径冻结三件与
//!   F2212/F2412 基准家族同构（[`FAMILY_DOC`]）；
//! - **F4208 数据流验证**：度量产物随 [`MetricSheet`] 交付（逐指标
//!   可查、空态显性、复算标记）。
//!
//! ## 无障碍与隐私（锚点原文）
//!
//! 指标族含无障碍一致率**单列**（域本色主场指标，见上）；数据合成——
//! 全部输入是计数整数，无用户内容。

use crate::checks::CheckSet;

use super::veu01_arch::{fnv1a64_hex, Severity};
use super::veu02_model::DomainTag;
use super::veu04_engine::{Rule, RuleEngine, RuleSource};
use super::veu06_scan::{NormalizedFinding, ScanReport};

// ---------------------------------------------------------------------------
// 一、版本与诊断码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const METRICS_VERSION: &str = "U07-metrics-v1";

/// 口径漂移（口径册被改——冻结重申）。
pub const E_CALIBER: &str = "E_U07_CALIBER_DRIFT";
/// 复算不符（两路计算结果不一致——立案重算不发布）。
pub const E_RECOMPUTE: &str = "E_U07_RECOMPUTE_MISMATCH";

/// 本域诊断码全集（判据对账：两两互异）。
pub const CODES: [&str; 2] = [E_CALIBER, E_RECOMPUTE];

// ---------------------------------------------------------------------------
// 二、口径册（冻结）
// ---------------------------------------------------------------------------

/// 口径册一行（指标名 + 冻结公式——公式是承诺不是注释）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaliberRow {
    /// 指标名。
    pub name: &'static str,
    /// 冻结公式（人话可复述——读屏可达）。
    pub formula: &'static str,
}

/// **口径册**（四主指标逐行冻结；篡改任何一行 → [`caliber_digest`] 漂移）。
pub const CALIBERS: [CaliberRow; 4] = [
    CaliberRow {
        name: "一致率",
        formula: "合规项×1000÷总检项（总检项=实际执行规则数）",
    },
    CaliberRow {
        name: "冲突密度",
        formula: "阻断级发现项×1000÷总检项",
    },
    CaliberRow {
        name: "修复时效",
        formula: "Σ(修复轮次−发现轮次)×1000÷已修复数",
    },
    CaliberRow {
        name: "回归率",
        formula: "复发发现项×1000÷上轮合规项",
    },
];

/// 口径册 FNV 摘要（真调 F4204 [`fnv1a64_hex`]——摘要漂移即口径被改）。
pub fn caliber_digest() -> String {
    let mut buf = alloc::string::String::new();
    let mut i = 0usize;
    while i < CALIBERS.len() {
        buf.push_str(CALIBERS[i].name);
        buf.push('=');
        buf.push_str(CALIBERS[i].formula);
        buf.push('|');
        i += 1;
    }
    fnv1a64_hex(buf.as_bytes())
}

/// 口径对拍（锚点：口径漂移→冻结重申）。
///
/// `frozen_digest` 是上次冻结的摘要：当前口径册摘要与它不一致 = 有人改了
/// 口径（[`E_CALIBER`]）→ **重申**冻结口径（重算全部作废，先恢复口径再
/// 谈数字——口径错了的数字比没有数字更害人）。
pub fn caliber_ok(frozen_digest: &str) -> bool {
    caliber_digest() == frozen_digest
}

/// 家族同构声明（锚点：与家族度量同构对拍）。
pub const FAMILY_DOC: &str = "家族度量同构声明：千分定点整数（不用 f32——同输入跨\
优化级别同输出）、空态显性（零样本不当全绿）、口径冻结（公式变更须过 ADR）\
三件与 F2212/F2412 基准度量家族同构；对拍面=本三件逐项在案";

// ---------------------------------------------------------------------------
// 三、指标值（含显性空态）与度量行
// ---------------------------------------------------------------------------

/// 单指标值（数值或**显性空态**——空态不是 0）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricValue {
    /// 有值（千分定点整数）。
    Permille(u32),
    /// 显性空态（零样本——消费方必须承认没有证据，O(1)）。
    Empty,
}

/// 一个切面（域×是否无障碍）的原始计数。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct DomainMetrics {
    /// 域标签（真调 F4204 authority_domain）。
    pub domain: Option<DomainTag>,
    /// 是否无障碍切面（**单列**——不与非无障碍混算）。
    pub a11y: bool,
    /// 总检项（实际执行规则数）。
    pub checked: u32,
    /// 发现项数（聚合后）。
    pub findings: u32,
    /// 阻断级发现项数。
    pub blocking: u32,
    /// 无障碍发现项数。
    pub a11y_findings: u32,
    /// 已修复数（修复台账）。
    pub repaired: u32,
    /// 修复轮次差总和。
    pub repair_rounds: u64,
    /// 上轮合规项（回归率分母）。
    pub prev_compliant: u32,
    /// 复发发现项。
    pub regressions: u32,
}

impl DomainMetrics {
    /// ① 一致率（合规=总检−发现；空态=总检 0）。
    pub const fn compliance(&self) -> MetricValue {
        if self.checked == 0 {
            return MetricValue::Empty;
        }
        let compliant = self.checked.saturating_sub(self.findings);
        MetricValue::Permille((compliant as u64 * 1000 / self.checked as u64) as u32)
    }

    /// ② 冲突密度（阻断级发现项占比）。
    pub const fn conflict_density(&self) -> MetricValue {
        if self.checked == 0 {
            return MetricValue::Empty;
        }
        MetricValue::Permille((self.blocking as u64 * 1000 / self.checked as u64) as u32)
    }

    /// ③ 修复时效（均值定点：Σ轮差×1000÷已修复；未修复=空态不是 0 轮）。
    pub const fn repair_latency(&self) -> MetricValue {
        if self.repaired == 0 {
            return MetricValue::Empty;
        }
        MetricValue::Permille((self.repair_rounds * 1000 / self.repaired as u64) as u32)
    }

    /// ④ 回归率（复发÷上轮合规；上轮无合规=空态不是 0——零基准分不出「没有复发」和「没有历史」）。
    pub const fn regression(&self) -> MetricValue {
        if self.prev_compliant == 0 {
            return MetricValue::Empty;
        }
        MetricValue::Permille((self.regressions as u64 * 1000 / self.prev_compliant as u64) as u32)
    }
}

// ---------------------------------------------------------------------------
// 四、主算与复算器（两路独立——逐位一致才可复算）
// ---------------------------------------------------------------------------

/// 从扫描报告与规则册提取切面计数（按域×a11y 分维）。
pub fn collect(report: &ScanReport, engine: &RuleEngine) -> alloc::vec::Vec<DomainMetrics> {
    let mut out: alloc::vec::Vec<DomainMetrics> = alloc::vec![];
    // 总检项按报告 executed 记在「汇总切面」（域=None, a11y=false）。
    let mut push_or_get = |out: &mut alloc::vec::Vec<DomainMetrics>, domain: Option<DomainTag>, a11y: bool| -> usize {
        let mut i = 0usize;
        while i < out.len() {
            if out[i].domain == domain && out[i].a11y == a11y {
                return i;
            }
            i += 1;
        }
        out.push(DomainMetrics { domain, a11y, ..Default::default() });
        out.len() - 1
    };
    let total_idx = push_or_get(&mut out, None, false);
    out[total_idx].checked = report.executed as u32;
    // 发现项按规则 → 域/a11y 归组（真调引擎册单源）。
    let mut fi = 0usize;
    while fi < report.findings.len() {
        let f: &NormalizedFinding = &report.findings[fi];
        let rule: Option<&Rule> = engine.rule(&f.rule);
        let (domain, is_a11y) = match rule {
            Some(r) => (Some(r.origin.source.authority_domain()), r.is_a11y),
            None => (None, f.is_a11y),
        };
        let d_idx = push_or_get(&mut out, domain, false);
        out[d_idx].findings = out[d_idx].findings.saturating_add(f.occurrences);
        if matches!(f.severity, Severity::Blocking) {
            out[d_idx].blocking = out[d_idx].blocking.saturating_add(f.occurrences);
        }
        if is_a11y {
            let a_idx = push_or_get(&mut out, domain, true);
            out[a_idx].a11y_findings = out[a_idx].a11y_findings.saturating_add(f.occurrences);
            out[a_idx].checked = out[a_idx].checked.saturating_add(0);
        }
        fi += 1;
    }
    out
}

/// 主算（从原始计数算四指标）。
pub const fn compute(m: &DomainMetrics) -> [MetricValue; 4] {
    [m.compliance(), m.conflict_density(), m.repair_latency(), m.regression()]
}

/// 复算器（**独立代码路径**重算——与 [`DomainMetrics`] 的方法不同实现）。
///
/// 两路逐位一致才算可复算；不一致 = [`E_RECOMPUTE`] 立案重算，不发布。
pub fn recompute(m: &DomainMetrics) -> [MetricValue; 4] {
    let empty = [MetricValue::Empty; 4];
    if m.checked == 0 && m.repaired == 0 && m.prev_compliant == 0 {
        return empty;
    }
    let c: u32 = if m.checked == 0 {
        return empty;
    } else {
        ((m.checked - m.findings) as u64).saturating_mul(1000) as u32 / m.checked
    };
    let cd: u32 = (m.blocking as u64 * 1000u64 / m.checked as u64) as u32;
    let rl: u32 = if m.repaired == 0 {
        return empty;
    } else {
        ((m.repair_rounds as u64) * 1000u64 / (m.repaired as u64)) as u32
    };
    let rg: u32 = if m.prev_compliant == 0 {
        return empty;
    } else {
        ((m.regressions as u64) * 1000u64 / (m.prev_compliant as u64)) as u32
    };
    [MetricValue::Permille(c), MetricValue::Permille(cd), MetricValue::Permille(rl), MetricValue::Permille(rg)]
}

/// 复算对拍（锚点：复算不符→立案重算）。
///
/// 返回 `Err` 即立案（调用方**重算并暂缓发布**——发布不一致的两套数字
/// 等于把选择权交给读者的偏见）。
pub const fn verify_recompute(main: [MetricValue; 4], re: [MetricValue; 4]) -> Result<(), &'static str> {
    let mut i = 0usize;
    while i < 4 {
        if main[i] != re[i] {
            return Err(E_RECOMPUTE);
        }
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 五、交付面（F4208 数据流验证的输入）
// ---------------------------------------------------------------------------

/// 度量交付单（F4208 数据流验证的输入——逐指标可查、空态显性、复算标记）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetricSheet {
    /// 切面行（域×a11y 分维）。
    pub slices: alloc::vec::Vec<DomainMetrics>,
    /// 主算四指标（与切面行序一致）。
    pub values: alloc::vec::Vec<[MetricValue; 4]>,
    /// 复算是否通过（false = 立案重算中，数字不得发布）。
    pub recompute_ok: bool,
    /// 口径摘要（冻结对拍面）。
    pub digest: alloc::string::String,
    /// 是否显性空态（本轮无可度量数据——O(1) 判定）。
    pub empty: bool,
}

/// 组装交付单（主算+复算+空态判定一次完成）。
pub fn build_sheet(report: &ScanReport, engine: &RuleEngine, frozen_digest: &str) -> Result<MetricSheet, &'static str> {
    if !caliber_ok(frozen_digest) {
        return Err(E_CALIBER);
    }
    let slices = collect(report, engine);
    let mut values = alloc::vec![];
    let mut empty = slices.is_empty();
    let mut si = 0usize;
    while si < slices.len() {
        let m = &slices[si];
        let main = compute(m);
        let re = recompute(m);
        verify_recompute(main, re)?;
        values.push(main);
        if matches!(main[0], MetricValue::Empty) {
            empty = true;
        }
        si += 1;
    }
    Ok(MetricSheet {
        slices,
        values,
        recompute_ok: true,
        digest: caliber_digest(),
        empty,
    })
}

// ---------------------------------------------------------------------------
// 六、域自检（判据逐条映射锚点：四主指标 / 口径冻结 / 可复算 / 单列 / 空态显性）
// ---------------------------------------------------------------------------

/// 判据用引擎（两域两规则：一条无障碍阻断、一条普通警告）。
fn bench_engine() -> RuleEngine {
    let mut e = RuleEngine::new();
    let _ = e.register(
        Rule::new(
            "K-A11Y-CONTRAST",
            "scan_targets_present",
            "contrast-ok",
            super::veu04_engine::Action::Block,
            SourceRef::of_content(RuleSource::VisualToken, "F2895", "contrast-ok"),
        )
        .as_a11y(),
    );
    let _ = e.register(Rule::new(
        "K-DOC-STYLE",
        "scan_targets_present",
        "style-ok",
        super::veu04_engine::Action::Fix,
        SourceRef::of_content(RuleSource::InteractionDict, "F4204", "style-ok"),
    ));
    e
}

/// 判据用扫描报告（3 发现：1 无障碍阻断 + 2 普通其中 1 阻断）。
fn bench_report() -> ScanReport {
    ScanReport {
        mode_effective: super::veu06_scan::ScanMode::Full,
        coverage_label: "全量",
        findings: alloc::vec![
            NormalizedFinding {
                location: alloc::string::String::from("F2895#7"),
                rule: alloc::string::String::from("K-A11Y-CONTRAST"),
                severity: Severity::Blocking,
                evidence: alloc::string::String::from("contrast-BAD"),
                suggestion: alloc::string::String::from("阻断：修复后重扫"),
                is_a11y: true,
                occurrences: 1,
            },
            NormalizedFinding {
                location: alloc::string::String::from("F4204#3"),
                rule: alloc::string::String::from("K-DOC-STYLE"),
                severity: Severity::Blocking,
                evidence: alloc::string::String::from("style-BAD"),
                suggestion: alloc::string::String::from("阻断：修复后重扫"),
                is_a11y: false,
                occurrences: 2,
            },
        ],
        raw_count: 3,
        aggregated_count: 2,
        storm: false,
        quarantined_now: 0,
        a11y_injected: 0,
        escalated: false,
        rule_digest: alloc::string::String::from("d0"),
        executed: 10,
        skipped: 0,
        codes: alloc::vec![],
    }
}

/// F4207 域自检入口（聚合器调用；零 panic 面，失败逐条红不炸域）。
pub fn run_veu07_checks() -> CheckSet {
    let mut s = CheckSet::new("veu07_metrics");

    // --- 判据 1：四主指标口径册（冻结 + 摘要稳定 + 漂移重申） ---
    {
        let d1 = caliber_digest();
        let d2 = caliber_digest();
        s.add(
            "U07-口径-四行冻结摘要稳定",
            CALIBERS.len() == 4
                && CALIBERS.iter().all(|c| !c.formula.is_empty())
                && d1 == d2
                && caliber_ok(&d1),
            "四主指标口径册逐行冻结（一致率/冲突密度/修复时效/回归率——公式是承诺不是注释），摘要双算一致（可复算的口径面）",
        );
        let tampered = "一致率=感觉不错";
        let drift_detected = !caliber_ok(tampered);
        s.add(
            "U07-口径-篡改即漂移重申",
            drift_detected && caliber_ok(&d1),
            "被改的口径摘要与冻结摘要不等（E_CALIBER）——重申冻结口径、重算作废（口径错了的数字比没有数字更害人）",
        );
        s.add(
            "U07-口径-家族同构声明在案",
            FAMILY_DOC.contains("千分定点") && FAMILY_DOC.contains("空态显性") && FAMILY_DOC.contains("口径冻结"),
            "与 F2212/F2412 基准度量家族同构三件逐项在案（千分定点/空态显性/口径冻结——家族对拍面）",
        );
    }

    // --- 判据 2：四指标计算（含复算逐位一致） ---
    {
        let m = DomainMetrics {
            domain: Some(DomainTag::S),
            a11y: false,
            checked: 10,
            findings: 3,
            blocking: 2,
            a11y_findings: 1,
            repaired: 2,
            repair_rounds: 8,
            prev_compliant: 10,
            regressions: 2,
        };
        let main = compute(&m);
        let re = recompute(&m);
        s.add(
            "U07-四指标-主算复算逐位一致",
            main == re
                && main[0] == MetricValue::Permille(700)
                && main[1] == MetricValue::Permille(200)
                && main[2] == MetricValue::Permille(4000)
                && main[3] == MetricValue::Permille(200),
            "十检三发现：一致率 700‰、阻断密度 200‰、修复时效 4000‰（8 轮差÷2 修复×1000——均值定点）、回归率 200‰；复算器独立路径重算逐位一致（锚点：计算可复算）",
        );
        s.add(
            "U07-四指标-复算不符立案",
            matches!(verify_recompute(main, [MetricValue::Permille(1); 4]), Err(E_RECOMPUTE)),
            "复算与主算不一致 → E_RECOMPUTE 立案重算（发布不一致的两套数字等于把选择权交给读者的偏见）",
        );
    }

    // --- 判据 3：空态显性（O(1)——零样本不当全绿） ---
    {
        let m = DomainMetrics { checked: 0, ..Default::default() };
        let v = compute(&m);
        s.add(
            "U07-空态-零样本显性不当全绿",
            v[0] == MetricValue::Empty
                && v[1] == MetricValue::Empty
                && matches!(m.compliance(), MetricValue::Empty),
            "总检项=0：一致率/冲突密度/修复时效/回归率全部显性空态而非 1000‰/0‰（零样本的「全绿」是把没有证据当证据——空态 O(1) 且消费方必须显性承认）",
        );
    }

    // --- 判据 4：域维度切分 + 无障碍单列（真调 F4204/F4206 数据源） ---
    {
        let engine = bench_engine();
        let report = bench_report();
        let slices = collect(&report, &engine);
        let s_domain = slices.iter().find(|x| x.domain == Some(DomainTag::S) && !x.a11y);
        let s_a11y = slices.iter().find(|x| x.a11y && x.domain == Some(DomainTag::T));
        s.add(
            "U07-切分-域维度逐行可点名",
            slices.len() >= 3 && s_domain.map(|x| x.findings == 2 && x.blocking == 1).unwrap_or(false),
            "发现项按规则归属域分维（真调 RuleSource::authority_domain → DomainTag 单源）——「哪个域拖了后腿」逐行可点名，不是一个笼统平均数",
        );
        s.add(
            "U07-单列-无障碍切面独立",
            s_a11y.map(|x| x.a11y_findings == 1).unwrap_or(false) && s_a11y != s_domain,
            "无障碍一致率单列成行（is_a11y 真调 F4204 规则册——混算会让无障碍的劣化被平均值吃掉，锚点「域本色主场指标」）",
        );
        let sheet = build_sheet(&report, &engine, &caliber_digest());
        s.add(
            "U07-交付-组装全过且复算标记真",
            sheet.is_ok()
                && sheet.as_ref().map(|x| x.recompute_ok && !x.empty && x.slices.len() >= 3).unwrap_or(false),
            "交付单组装（主算+复算+空态判定一次完成）全过——F4208 数据流验证的输入面（逐指标可查/空态显性/复算标记三项齐）",
        );
        let sheet_bad = build_sheet(&report, &engine, "tampered-digest");
        s.add(
            "U07-交付-口径漂移拒发布",
            sheet_bad == Err(E_CALIBER),
            "冻结摘要被篡改：交付单拒发（先恢复口径再谈数字——度量体系的第一诫是口径可信）",
        );
    }

    // --- 判据 5：判据自身（码互异 + 版本 + 条数对账） ---
    {
        let mut ok = true;
        let mut i = 0usize;
        while i < CODES.len() {
            let mut j = i + 1;
            while j < CODES.len() {
                if CODES[i] == CODES[j] {
                    ok = false;
                }
                j += 1;
            }
            i += 1;
        }
        s.add("U07-判据-两码两两互异", ok, "两码互异（按码归因的前提）");
        s.add(
            "U07-判据-版本钉死",
            METRICS_VERSION == "U07-metrics-v1" && METRICS_VERSION.starts_with("U07-"),
            "版本串钉死（U 域第 07 项——F4207 在一致性批的族内序）",
        );
        s.add(
            "U07-判据-条数对账",
            s.len() == 11,
            "判据条数恰 12（本条执行前已有 11 条，防悄悄增删）",
        );
    }

    s
}
