//! VE-F3607 · 创作资产验证器（VE-S 域 · 批次 S01 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3607`
//!
//! **判据（锚点原文六条）**：五段验证、引用闭合、许可复用、自检分级、
//! 单源复述、判据。逐条落位：
//! - **五段验证**：[`validate`] 流水五段依次执行——① schema 校验（**真复用**
//!   F3204 [`veq04_type`] 单源：类型名/线上码双口径解析 + [`TypeRegistry`]
//!   注册核验，不另造第二份类型表）② 引用完整性（闭合断言，见下）
//!   ③ 许可核验（三查，见下）④ 无障碍自检（分级**不阻断**，见下）
//!   ⑤ 安全扫描（检出即 P0，见下）。任一阻断段不过 → [`Verdict::Blocked`]。
//! - **引用闭合**：资产内引用必须闭合在发布集合内——引用缺失资产=
//!   发布即坏，这是**实测红线**不是文档约定：[`PublishSet`] 排序建集 +
//!   二分查 `O(引用·logN)`，缺一个即 `SCHEMA` 外的第二阻断源
//!   [`V07Code::REF_MISSING`]。
//! - **许可复用**：素材许可三查（存在/覆盖分发/有效），任一不成立即
//!   阻断——许可是发布的前置条件不是扣分项。
//! - **自检分级**：无障碍差的资产**降评级不阻断**（分级处置、降级显性）：
//!   A（全过）/B（缺一项）/C（缺两项），降级必带**原因清单**——
//!   降级未显性（评级低于 A 而原因清单为空）→ 立案
//!   [`V07Code::A11Y_DOWNGRADE_SILENT`]（P1）。
//! - **单源复述**：F2919（素材许可三查）与 F3303（安全扫描）两单源
//!   尚未落库，本模块按锚点原文做**跨域复述声明**（[`F2919_RESTATED_NOTE`]
//!   / [`F3303_RESTATED_NOTE`]）：语义按两单锚点复述、本地实现、
//!   判据钉住复述件非空且指名单号——两单落库后由对账钩子接管
//!   （同 F2207 与 I03 语义对齐的复述范式）。
//! - **判据**：`ves07_checks.rs` 逐条映射（独立重算 + 反向语料 + 双向验证）。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 引用缺失→阻断发布（红线实测） | [`V07Code::REF_MISSING`] + [`Verdict::Blocked`]，缺失清单逐条列出 |
//! | 许可缺→阻断（复述红线） | [`V07Code::LICENSE_MISSING`]/[`V07Code::LICENSE_SCOPE`]/[`V07Code::LICENSE_EXPIRED`] 三查逐条对应 |
//! | 无障碍降级未显性→立案（分级红线） | [`V07Code::A11Y_DOWNGRADE_SILENT`]（P1）：评级 < A 而原因清单为空即立案 |
//! | 安全检出→P0（复述） | [`V07Code::SCAN_HIT`]（P0）：检出即阻断，不静默放行 |
//!
//! 另两条自设防御：**阻断无理由**（verdict=Blocked 而 blocked_reasons 为空
//! → [`V07Code::BLOCK_NO_REASON`] 立案——阻断是给创作端看的，没有理由的
//! 阻断等于把人锁在门外不说为什么）；**双口径类型漂移**（en 与 wire 同时
//! 声明且解析不一致 → 拒，声明二义比声明缺失更难排查）。
//!
//! **码段**：独占 **0x37xx**（F3606 占 0x36xx，0x37 空闲实测后占用）。
//!
//! **性能逐项分解（锚点口径）**：五段 O(流水)（顺序一遍）；闭合
//! O(引用·logN)（发布集合排序一次建集 + 逐引用二分）；分级 O(1)
//! （两布尔查表）；扫描 O(复用)（findings 计数直读）。
//!
//! 零 panic 面、零 IO、零墙钟；无全局可变状态（状态由调用方持有）。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use super::veq01_pipeline::Outcome;
use super::veq04_type::{std_registry, ResourceType, TypeRegistry};

// ---------------------------------------------------------------------------
// 一、诊断码（独占 0x37xx 码段；0x36xx 归 F3606，0x38xx 预留）
// ---------------------------------------------------------------------------

/// 诊断码（本单自建，独立于 Q 域 [`QDiagCode`]——跨域码经 bridge 映射，
/// 不直接复用他人码型）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct V07Code(pub u16);

impl V07Code {
    /// schema：声明类型未知名/未知码。
    pub const SCHEMA_TYPE_UNKNOWN: V07Code = V07Code(0x3701);
    /// schema：双口径（en 与 wire）声明漂移。
    pub const SCHEMA_DUAL_DRIFT: V07Code = V07Code(0x3702);
    /// 引用完整性：引用缺失资产（阻断红线）。
    pub const REF_MISSING: V07Code = V07Code(0x3703);
    /// 许可三查之一：许可记录缺失。
    pub const LICENSE_MISSING: V07Code = V07Code(0x3704);
    /// 许可三查之二：不覆盖分发用途。
    pub const LICENSE_SCOPE: V07Code = V07Code(0x3705);
    /// 许可三查之三：许可失效/过期。
    pub const LICENSE_EXPIRED: V07Code = V07Code(0x3706);
    /// 无障碍：降级（显性记账，不阻断）。
    pub const A11Y_DOWNGRADE: V07Code = V07Code(0x3707);
    /// 无障碍：降级未显性（评级 < A 而原因清单为空）→ 立案 P1。
    pub const A11Y_DOWNGRADE_SILENT: V07Code = V07Code(0x3708);
    /// 安全扫描：检出（→ P0 阻断）。
    pub const SCAN_HIT: V07Code = V07Code(0x3709);
    /// 阻断无理由（verdict=Blocked 而 blocked_reasons 空）→ 立案。
    pub const BLOCK_NO_REASON: V07Code = V07Code(0x370A);

    /// 人话标签（读屏可达）。
    pub const fn label(self) -> &'static str {
        match self {
            V07Code::SCHEMA_TYPE_UNKNOWN => "声明类型无法解析，schema 校验拒绝",
            V07Code::SCHEMA_DUAL_DRIFT => "en 与 wire 双口径声明漂移",
            V07Code::REF_MISSING => "引用缺失资产，发布即坏，阻断",
            V07Code::LICENSE_MISSING => "许可记录缺失，阻断",
            V07Code::LICENSE_SCOPE => "许可不覆盖分发用途，阻断",
            V07Code::LICENSE_EXPIRED => "许可失效或过期，阻断",
            V07Code::A11Y_DOWNGRADE => "无障碍自检降级（显性记账，不阻断）",
            V07Code::A11Y_DOWNGRADE_SILENT => "无障碍降级未显性，立案",
            V07Code::SCAN_HIT => "安全扫描检出，P0 阻断",
            V07Code::BLOCK_NO_REASON => "阻断未附理由，立案",
            other => {
                let _ = other;
                "未登记诊断码"
            }
        }
    }

    /// 全部码（家族完整性判据用）。
    pub const ALL: [V07Code; 10] = [
        V07Code::SCHEMA_TYPE_UNKNOWN,
        V07Code::SCHEMA_DUAL_DRIFT,
        V07Code::REF_MISSING,
        V07Code::LICENSE_MISSING,
        V07Code::LICENSE_SCOPE,
        V07Code::LICENSE_EXPIRED,
        V07Code::A11Y_DOWNGRADE,
        V07Code::A11Y_DOWNGRADE_SILENT,
        V07Code::SCAN_HIT,
        V07Code::BLOCK_NO_REASON,
    ];
}

/// 严重度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// 记账不阻断。
    Minor,
    /// 显性告警。
    Major,
    /// 立案 P1（降级未显性/阻断无理由）。
    P1,
    /// 安全检出 P0（复述红线）。
    P0,
}

/// 一条诊断。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: V07Code,
    pub severity: Severity,
}

/// 诊断袋。
#[derive(Clone, Debug, Default)]
pub struct DiagBag {
    items: Vec<Diagnostic>,
}

impl DiagBag {
    #[allow(clippy::new_without_default)]
    pub fn new() -> DiagBag {
        DiagBag { items: Vec::new() }
    }

    pub fn push(&mut self, code: V07Code, severity: Severity) {
        self.items.push(Diagnostic { code, severity });
    }

    pub fn items(&self) -> &[Diagnostic] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 某码出现次数（独立计数不合并）。
    pub fn count(&self, code: V07Code) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.items.len() {
            if let Some(d) = self.items.get(i) {
                if d.code == code {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn has(&self, code: V07Code) -> bool {
        self.count(code) > 0
    }

    /// 各严重级计数（P0/P1 是一等公民）。
    pub fn count_severity(&self, s: Severity) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.items.len() {
            if let Some(d) = self.items.get(i) {
                if d.severity == s {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 人话渲染。
    pub fn render(&self) -> String {
        let mut out = String::new();
        let mut i = 0usize;
        while i < self.items.len() {
            if let Some(d) = self.items.get(i) {
                let sev = match d.severity {
                    Severity::Minor => "MINOR",
                    Severity::Major => "MAJOR",
                    Severity::P1 => "P1",
                    Severity::P0 => "P0",
                };
                let _ = out.push_str(&format!("[{}/{}] {}\n", sev, d.code.0, d.code.label()));
            }
            i += 1;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 二、单源复述声明（F2919 / F3303 尚未落库——按锚点原文跨域复述）
// ---------------------------------------------------------------------------

/// F2919（素材许可三查）复述声明：三查语义按 F2919 锚点复述——
/// ① 许可记录存在；② 许可范围覆盖发布分发用途；③ 许可处于有效期内
/// 且未被撤销。本模块本地实现三查谓词，F2919 落库后由对账钩子接管
/// （复述件非空且指名单号由判据钉住——复述不是可以烂在注释里的话）。
pub const F2919_RESTATED_NOTE: &str =
    "许可三查复述自 F2919（素材许可）：存在/覆盖分发/有效三谓词逐条独立，\
     任一不成立即阻断发布；本实现为跨域复述件，F2919 落库后切换真单源";

/// F3303（安全扫描）复述声明：检出即 P0 阻断、不静默放行、扫描结果
/// 只消费不二次裁定（裁定权归扫描单源）。本模块本地实现检出直通，
/// F3303 落库后由对账钩子接管。
pub const F3303_RESTATED_NOTE: &str =
    "安全扫描复述自 F3303：检出计数非零即 P0 阻断且不静默放行；\
     本实现为跨域复述件，F3303 落库后切换真单源";

// ---------------------------------------------------------------------------
// 三、输入模型
// ---------------------------------------------------------------------------

/// 素材许可记录（三查的载体）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct LicenseRecord {
    /// 查一：许可记录存在。
    pub present: bool,
    /// 查二：许可范围覆盖发布分发用途。
    pub covers_distribution: bool,
    /// 查三：许可处于有效期内且未被撤销。
    pub valid: bool,
}

impl LicenseRecord {
    /// 三查逐条（顺序即 [`F2919_RESTATED_NOTE`] 复述序）。
    pub fn three_queries(&self) -> [bool; 3] {
        [self.present, self.covers_distribution, self.valid]
    }
}

/// 无障碍自检输入（两基线项，锚点：对比度/语义标注）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct A11ySelfCheck {
    /// 对比度基线自查。
    pub contrast_ok: bool,
    /// 语义标注自查。
    pub semantic_labeled: bool,
}

/// 无障碍评级（A 全过 / B 缺一项 / C 缺两项）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum A11yGrade {
    /// 两项全过。
    A,
    /// 缺一项。
    B,
    /// 缺两项。
    C,
}

/// 降级原因（原因清单的元素）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DowngradeReason {
    /// 对比度未过基线。
    Contrast,
    /// 语义标注缺失。
    Semantic,
}

impl DowngradeReason {
    pub const fn label(self) -> &'static str {
        match self {
            DowngradeReason::Contrast => "对比度未过基线",
            DowngradeReason::Semantic => "语义标注缺失",
        }
    }
}

/// 待验证的创作资产。
#[derive(Clone, Debug, Default)]
pub struct AssetCandidate {
    /// 资产 id（发布集合内的主键）。
    pub asset_id: u32,
    /// 声明类型名（F3204 十类 en 名；与 wire 至少给一个）。
    pub declared_type_en: Option<String>,
    /// 声明线上码（与 en 至少给一个；两者同时给必须一致）。
    pub declared_type_wire: Option<u8>,
    /// 引用的资产 id（闭合断言对象）。
    pub refs: Vec<u32>,
    /// 许可记录。
    pub license: LicenseRecord,
    /// 无障碍自检。
    pub a11y: A11ySelfCheck,
    /// 安全扫描检出计数（复述件直通消费）。
    pub scan_findings: u32,
}

/// 发布集合（闭合断言的域）。
///
/// `new` 排序建集一次 `O(N logN)`，[`PublishSet::contains`] 二分
/// `O(logN)`——逐资产验证时集合只建一次，闭合成本 `O(引用·logN)`
/// 即锚点「闭合 O(引用)」口径。
#[derive(Clone, Debug)]
pub struct PublishSet {
    sorted: Vec<u32>,
}

impl PublishSet {
    pub fn new(mut ids: Vec<u32>) -> PublishSet {
        ids.sort_unstable();
        PublishSet { sorted: ids }
    }

    /// 二分查存在性。
    pub fn contains(&self, id: u32) -> bool {
        let mut lo = 0usize;
        let mut hi = self.sorted.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            match self.sorted.get(mid) {
                Some(v) if *v < id => lo = mid + 1,
                Some(v) if *v == id => return true,
                _ => hi = mid,
            }
        }
        false
    }

    /// 集合规模。
    pub fn len(&self) -> usize {
        self.sorted.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sorted.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 四、五段验证流水
// ---------------------------------------------------------------------------

/// 无障碍评级与原因（分级段产物）。
#[derive(Clone, Debug, PartialEq)]
pub struct A11yOutcome {
    /// 评级（降评级**不阻断**）。
    pub grade: A11yGrade,
    /// 降级原因清单（grade == A 恒空；grade < A **恒非空**——空即立案）。
    pub reasons: Vec<DowngradeReason>,
}

/// 无障碍分级（O(1)：两布尔查表）。
pub fn a11y_grade(check: &A11ySelfCheck, bag: &mut DiagBag) -> A11yOutcome {
    let mut reasons: Vec<DowngradeReason> = Vec::new();
    if !check.contrast_ok {
        reasons.push(DowngradeReason::Contrast);
    }
    if !check.semantic_labeled {
        reasons.push(DowngradeReason::Semantic);
    }
    let grade = match reasons.len() {
        0 => A11yGrade::A,
        1 => A11yGrade::B,
        _ => A11yGrade::C,
    };
    if grade != A11yGrade::A {
        bag.push(V07Code::A11Y_DOWNGRADE, Severity::Minor);
        // 降级未显性（评级 < A 而原因清单为空）在当前构造下不可达
        // （reasons 与 grade 同源派生），但**消费端重组**的 report 仍可能
        // 出现该态——立案判定放在 [`validate_report_consistency`] 统一执行。
    }
    A11yOutcome { grade, reasons }
}

/// 报告一致性核验（分级红线 + 阻断红线的**独立复核口**）：
/// ① 评级 < A 而原因清单为空 → 立案 P1（[`V07Code::A11Y_DOWNGRADE_SILENT`]）；
/// ② verdict=Blocked 而阻断理由清单为空 → 立案（[`V07Code::BLOCK_NO_REASON`]）。
/// 返回立案数。把一致性从构造函数里拿出来单独成口，是因为报告可能被
/// 消费端逐字段重组——一致性必须**对重组后的最终报告**成立才算成立。
pub fn validate_report_consistency(report: &ValidationReport, bag: &mut DiagBag) -> u32 {
    let mut cases = 0u32;
    if report.a11y.grade != A11yGrade::A && report.a11y.reasons.is_empty() {
        bag.push(V07Code::A11Y_DOWNGRADE_SILENT, Severity::P1);
        cases += 1;
    }
    if report.verdict == Verdict::Blocked && report.blocked_reasons.is_empty() {
        bag.push(V07Code::BLOCK_NO_REASON, Severity::P1);
        cases += 1;
    }
    cases
}

/// 发布裁决。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 五段阻断段全过（无障碍评级不影响裁决）。
    Publishable,
    /// 至少一个阻断段不过。
    Blocked,
}

/// 验证报告（五段流水产物）。
#[derive(Clone, Debug)]
pub struct ValidationReport {
    /// 段一：schema 校验通过。
    pub schema_ok: bool,
    /// 段二：引用闭合通过。
    pub refs_closed: bool,
    /// 缺失引用清单（闭合失败时逐条列出——排查者不该自己算差集）。
    pub missing_refs: Vec<u32>,
    /// 段三：许可三查通过。
    pub license_ok: bool,
    /// 段四：无障碍评级（不进裁决）。
    pub a11y: A11yOutcome,
    /// 段五：安全扫描干净。
    pub scan_clean: bool,
    /// 裁决。
    pub verdict: Verdict,
    /// 阻断理由清单（Blocked 时**恒非空**，空即立案）。
    pub blocked_reasons: Vec<&'static str>,
}

/// 段一：schema 校验（**真复用 F3204 单源**）。
///
/// 双口径：en 名与 wire 码至少给一个；同时给必须解析到同一类型
/// （[`V07Code::SCHEMA_DUAL_DRIFT`]）；解析不出（[`V07Code::
/// SCHEMA_TYPE_UNKNOWN`]）或未在 F3204 标准注册表登记 → 不过。
fn schema_check(asset: &AssetCandidate, registry: &TypeRegistry, bag: &mut DiagBag) -> bool {
    let by_en = match asset.declared_type_en.as_deref() {
        Some(s) => ResourceType::from_en(s),
        None => None,
    };
    let by_wire = match asset.declared_type_wire {
        Some(w) => ResourceType::from_wire(w),
        None => None,
    };
    let resolved = match (by_en, by_wire) {
        (Some(a), Some(b)) => {
            if a != b {
                bag.push(V07Code::SCHEMA_DUAL_DRIFT, Severity::Major);
                return false;
            }
            Some(a)
        }
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => {
            bag.push(V07Code::SCHEMA_TYPE_UNKNOWN, Severity::Major);
            return false;
        }
    };
    match resolved {
        Some(t) => {
            if registry.is_registered(t) {
                true
            } else {
                // 十类闭集内但未登记（std_registry 全登记，正常构造不可达；
                // 保留是复用 F3204 注册制语义的防御——注册制拒绝必须显性）。
                bag.push(V07Code::SCHEMA_TYPE_UNKNOWN, Severity::Major);
                false
            }
        }
        None => {
            bag.push(V07Code::SCHEMA_TYPE_UNKNOWN, Severity::Major);
            false
        }
    }
}

/// 段二：引用闭合（O(引用·logN)，缺失逐条列清单）。
fn refs_check(asset: &AssetCandidate, set: &PublishSet, bag: &mut DiagBag) -> (bool, Vec<u32>) {
    let mut missing: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < asset.refs.len() {
        if let Some(r) = asset.refs.get(i) {
            if !set.contains(*r) {
                // 同一缺失只记一次（重复引用同一缺失资产不算两个洞）。
                let mut seen = false;
                let mut j = 0usize;
                while j < missing.len() {
                    if let Some(m) = missing.get(j) {
                        if *m == *r {
                            seen = true;
                        }
                    }
                    j += 1;
                }
                if !seen {
                    missing.push(*r);
                }
            }
        }
        i += 1;
    }
    if missing.is_empty() {
        (true, missing)
    } else {
        bag.push(V07Code::REF_MISSING, Severity::Major);
        (false, missing)
    }
}

/// 段三：许可三查（复述件，任一不成立即阻断）。
fn license_check(license: &LicenseRecord, bag: &mut DiagBag) -> bool {
    let q = license.three_queries();
    let mut ok = true;
    if !q[0] {
        bag.push(V07Code::LICENSE_MISSING, Severity::Major);
        ok = false;
    }
    if !q[1] {
        bag.push(V07Code::LICENSE_SCOPE, Severity::Major);
        ok = false;
    }
    if !q[2] {
        bag.push(V07Code::LICENSE_EXPIRED, Severity::Major);
        ok = false;
    }
    ok
}

/// 段五：安全扫描（复述件：检出非零即 P0）。
fn scan_check(findings: u32, bag: &mut DiagBag) -> bool {
    if findings > 0 {
        bag.push(V07Code::SCAN_HIT, Severity::P0);
        false
    } else {
        true
    }
}

/// 五段验证流水（本模块唯一入口）。
///
/// 阻断段：schema / 闭合 / 许可 / 安全；非阻断段：无障碍（只评级）。
/// Blocked 时 blocked_reasons **恒非空**（逐段追加人话理由）——
/// 该不变量由 [`validate_report_consistency`] 独立复核。
pub fn validate(
    asset: &AssetCandidate,
    set: &PublishSet,
    registry: &TypeRegistry,
    bag: &mut DiagBag,
) -> ValidationReport {
    // 段一：schema（复用 F3204）。
    let schema_ok = schema_check(asset, registry, bag);
    // 段二：引用闭合。
    let (refs_closed, missing_refs) = refs_check(asset, set, bag);
    // 段三：许可三查。
    let license_ok = license_check(&asset.license, bag);
    // 段四：无障碍分级（不阻断）。
    let a11y = a11y_grade(&asset.a11y, bag);
    // 段五：安全扫描。
    let scan_clean = scan_check(asset.scan_findings, bag);

    let mut blocked_reasons: Vec<&'static str> = Vec::new();
    if !schema_ok {
        blocked_reasons.push("schema 校验未过：声明类型无法解析或双口径漂移");
    }
    if !refs_closed {
        blocked_reasons.push("引用缺失资产，发布即坏（闭合断言红线）");
    }
    if !license_ok {
        blocked_reasons.push("许可三查未全过：缺记录/不覆盖分发/失效");
    }
    if !scan_clean {
        blocked_reasons.push("安全扫描检出，P0");
    }
    let verdict = if blocked_reasons.is_empty() {
        Verdict::Publishable
    } else {
        Verdict::Blocked
    };

    ValidationReport {
        schema_ok,
        refs_closed,
        missing_refs,
        license_ok,
        a11y,
        scan_clean,
        verdict,
        blocked_reasons,
    }
}

/// 标准注册表入口（复用 F3204 单源的便捷包装——判据用它做「真复用」
/// 的对拍锚：十类声明全部可解析且全登记）。
pub fn f3204_registry() -> TypeRegistry {
    match std_registry() {
        Outcome::Ok { value, .. } => value,
        // 不可达兜底：std_registry 对十类全登记（F3204 判据钉住），此处
        // 返回空注册表会让所有 schema 判红而不是 panic——零 panic 面。
        _ => TypeRegistry::new(),
    }
}

// ---------------------------------------------------------------------------
// 五、家族声明与冒烟
// ---------------------------------------------------------------------------

/// 码标签互异。
pub fn labels_unique() -> bool {
    let mut i = 0usize;
    while i < V07Code::ALL.len() {
        let mut j = i + 1;
        while j < V07Code::ALL.len() {
            let a = match V07Code::ALL.get(i) {
                Some(c) => *c,
                None => return true,
            };
            let b = match V07Code::ALL.get(j) {
                Some(c) => *c,
                None => return true,
            };
            if a.label() == b.label() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 复述件齐备（非空 + 指名单号）。
pub fn restated_notes_present() -> bool {
    F2919_RESTATED_NOTE.contains("F2919")
        && F3303_RESTATED_NOTE.contains("F3303")
        && !F2919_RESTATED_NOTE.is_empty()
        && !F3303_RESTATED_NOTE.is_empty()
}

/// 码段双向钉死：全部码在 0x37 段且不在 0x36（F3606）段。
pub fn codes_in_own_segment() -> bool {
    let mut i = 0usize;
    while i < V07Code::ALL.len() {
        match V07Code::ALL.get(i) {
            Some(c) => {
                if c.0 >> 8 != 0x37 {
                    return false;
                }
            }
            None => return true,
        }
        i += 1;
    }
    true
}

/// 冒烟：好资产全过 + 坏资产逐段红。
pub fn smoke() -> String {
    let registry = f3204_registry();
    let set = PublishSet::new(vec![1, 2, 3]);
    // 好资产。
    let mut bag = DiagBag::new();
    let good = AssetCandidate {
        asset_id: 1,
        declared_type_en: Some(String::from("texture")),
        declared_type_wire: None,
        refs: vec![2, 3],
        license: LicenseRecord { present: true, covers_distribution: true, valid: true },
        a11y: A11ySelfCheck { contrast_ok: true, semantic_labeled: true },
        scan_findings: 0,
    };
    let good_r = validate(&good, &set, &registry, &mut bag);
    // 坏资产：引用缺失 + 许可缺 + 扫描检出 + 无障碍缺一项。
    let mut bag2 = DiagBag::new();
    let bad = AssetCandidate {
        asset_id: 2,
        declared_type_en: Some(String::from("no_such_type")),
        declared_type_wire: None,
        refs: vec![3, 99],
        license: LicenseRecord { present: false, covers_distribution: true, valid: false },
        a11y: A11ySelfCheck { contrast_ok: false, semantic_labeled: true },
        scan_findings: 2,
    };
    let bad_r = validate(&bad, &set, &registry, &mut bag2);
    let _ = validate_report_consistency(&bad_r, &mut bag2);
    let mut s = String::new();
    let _ = s.push_str(&format!(
        "好资产 verdict={:?} grade={:?} 诊断={} 条\n",
        good_r.verdict,
        good_r.a11y.grade,
        bag.len()
    ));
    let _ = s.push_str(&format!(
        "坏资产 verdict={:?} 缺引用={:?} 阻断理由={} 条 P0={} P1={}\n",
        bad_r.verdict,
        bad_r.missing_refs,
        bad_r.blocked_reasons.len(),
        bag2.count_severity(Severity::P0),
        bag2.count_severity(Severity::P1),
    ));
    s
}
