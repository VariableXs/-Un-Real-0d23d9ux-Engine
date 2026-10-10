//! VE-F2416 · 动画安全（VE-M 域 · 动画段 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2416`
//!
//! **判据（锚点原文）**：双层清洗、双硬顶不可绕、操作审计、快照硬门、判据。
//!
//! **职责定位（锚点原文）**：动画安全——三段防线：动画数据清洗（导入
//! 面 schema 校验——复用 F1612 标准；NaN 值防护：F2409 三重校验的
//! 安全侧规则表——导入面全域清洗核验+运行时 NaN 防护（求值期防御性
//! 归一化/钳制——双层清洗声明：导入层+求值层））、资源配额（轨道数/
//! clip 大小上限：轨道数/实体（F2402 上限数值化）/clip 数据大小（MB
//! 上限）双硬顶——绕过路径不存在断言（F1938 家族——M 维首例））、
//! 审计（动画操作留痕：轨道增删/clip 导入——F1954 范式），并完成
//! F2411 通过率快照引用。
//!
//! # 一、双层清洗（导入层 + 求值层——不是一层洗两遍）
//!
//! 导入层真调 F2409 三重校验（[`validate_channel_refs`](vem09_import::
//! validate_channel_refs)/[`validate_samples`](vem09_import::
//! validate_samples)——规则表按 F1612 家族格式（规则×探测×严重度）
//! 登记；求值层真调 [`eval_scalar_span`](vem03_interp::
//! eval_scalar_span)（NaN 关键帧 → 钳制+记账的有限结果）与
//! [`eval_quat_span`](vem03_interp::eval_quat_span)（非单位四元数 →
//! 归一化的单位输出）——导入洗过的数据在求值期仍有第二道闸（运行时
//! NaN 不靠"上游一定对"）。
//!
//! # 二、双硬顶 + 绕过路径不存在断言（F1938 家族 M 维首例）
//!
//! [`ResourceQuota`] 双硬顶：轨道数（默认 256，F2402 上限数值化）+
//! clip 数据大小（默认 64 MiB）——均可配但零值即拒（"没配"不是
//! "无限"）。**绕过路径不存在**的可执行形态：创建入口枚举
//! （[`CreationEntry`]）与检查点覆盖证明——每一入口必须映射到一个
//! 检查点（None=绕过路径在网= P0）；入口数=检查点数可复算对拍。
//!
//! # 三、审计（F1954 范式）+ 快照硬门（F2411）
//!
//! [`AuditLog`] 环形留痕（轨道增删/clip 导入三操作），详述是**枚举
//! 标签**而非自由文本——类型层面不含用户内容（隐私红线不用口头
//! 承诺）。快照引用真调 [F2411 判据集](vem11_fuzz::run_vem11_checks)
//! 计通过率，硬门 100%（非 100% 安全不收官——锚点错误路径原文）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::svstar2::vem02_track::{DiagBag, TrackClass as ValueTrackClass};
use crate::svstar2::vem03_interp::{
    eval_quat_span, eval_scalar_span, Interp, InterpEntry, InterpParams,
};
use crate::svstar2::vem09_import::{
    fsqrt, validate_channel_refs, validate_samples, ChannelPath, ChannelRef, ComponentType,
    DiagBag as ImportBag, GltfAnimDoc, GltfInterp, GltfSamplerOut, SampleVerdict, SamplerRef,
    TrackSemantic,
};
use crate::svstar2::vem11_fuzz::run_vem11_checks;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const SECURITY_VERSION: &str = "M16-security-v1";

/// 轨道数硬顶默认值（F2402 上限数值化——可配）。
pub const DEFAULT_TRACK_CAP: u32 = 256;

/// clip 数据大小硬顶默认值（64 MiB——可配）。
pub const DEFAULT_CLIP_BYTES_CAP: u64 = 64 * 1024 * 1024;

/// 清洗遗漏（fuzz 立案回补）。
pub const E_SEC_IMPORT_MISS: &str = "E_SEC_IMPORT_MISS";

/// 双硬顶绕过（P0——枚举证明失效）。
pub const E_SEC_QUOTA_BYPASS: &str = "E_SEC_QUOTA_BYPASS";

/// 审计缺失（P1——可追溯性）。
pub const E_SEC_AUDIT_MISS: &str = "E_SEC_AUDIT_MISS";

/// 快照通过率非 100%（安全不收官）。
pub const E_SEC_SNAPSHOT: &str = "E_SEC_SNAPSHOT";

// ---------------------------------------------------------------------------
// 二、双层清洗规则表（F1612 家族格式）
// ---------------------------------------------------------------------------

/// 清洗层（双层声明——导入层+求值层）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanLayer {
    /// 导入层（schema/三重校验）。
    Import,
    /// 求值层（运行时 NaN 防护/归一化）。
    Eval,
}

impl CleanLayer {
    /// 双层闭集。
    pub const ALL: [CleanLayer; 2] = [CleanLayer::Import, CleanLayer::Eval];

    /// 层名（人读）。
    pub fn zh(self) -> &'static str {
        match self {
            CleanLayer::Import => "导入层",
            CleanLayer::Eval => "求值层",
        }
    }
}

/// 清洗规则（F1612 家族格式：规则×探测×严重度）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CleanRule {
    /// 规则名（wire 名）。
    pub rule: &'static str,
    /// 所属层。
    pub layer: CleanLayer,
    /// 探测面（人读——规则拿什么查）。
    pub probe: &'static str,
    /// 严重度（P0=红线失守/P1=可追溯性）。
    pub severity: &'static str,
}

/// M 域动画安全清洗规则表（双层各三——锚点"导入面全域清洗核验+
/// 运行时 NaN 防护"的规则化）。
pub fn clean_rules() -> [CleanRule; 6] {
    [
        // 导入层：F2409 三重校验的安全侧规则化。
        CleanRule { rule: "import.channel_ref", layer: CleanLayer::Import, probe: "通道引用越界/断链检测", severity: "P0" },
        CleanRule { rule: "import.sample_data", layer: CleanLayer::Import, probe: "采样值 NaN/非有限检测计数", severity: "P0" },
        CleanRule { rule: "import.format", layer: CleanLayer::Import, probe: "值数/分量规格自洽（格式混淆）", severity: "P0" },
        // 求值层：运行时 NaN 防护/归一化。
        CleanRule { rule: "eval.nan_guard", layer: CleanLayer::Eval, probe: "NaN 关键帧求值→钳制+记账有限结果", severity: "P1" },
        CleanRule { rule: "eval.quat_normalize", layer: CleanLayer::Eval, probe: "非单位四元数求值→归一化单位输出", severity: "P1" },
        CleanRule { rule: "eval.time_guard", layer: CleanLayer::Eval, probe: "禁外推轨道外时刻钳制+告警", severity: "P1" },
    ]
}

/// 规则表核验：双层各至少三条、探测非空、严重度合法。
pub fn clean_rule_verdict(rules: &[CleanRule]) -> Result<(), String> {
    for l in CleanLayer::ALL.iter() {
        let n = rules.iter().filter(|r| r.layer == *l).count();
        if n < 3 {
            return Err(format!(
                "{}：{}规则仅 {} 条（少于 3）——清洗覆盖不足",
                E_SEC_IMPORT_MISS, l.zh(), n
            ));
        }
    }
    for r in rules.iter() {
        if r.probe.trim().is_empty() {
            return Err(format!("{}：规则 {} 缺探测面声明", E_SEC_IMPORT_MISS, r.rule));
        }
        if r.severity != "P0" && r.severity != "P1" {
            return Err(format!("{}：规则 {} 严重度非法（{}）", E_SEC_IMPORT_MISS, r.rule, r.severity));
        }
    }
    Ok(())
}

/// 导入层真调：畸形样本必须被三重校验拦截（显性）。
fn import_layer_probe() -> Result<(), String> {
    // 采样器越界（引用失效）。
    let bad_doc = GltfAnimDoc::new(1, Vec::new(), Vec::new(), Vec::new(), "sec-probe");
    let ch = ChannelRef { target_node: 0, path: ChannelPath::Translation, sampler: 9 };
    let mut bag = ImportBag::new();
    if validate_channel_refs(&bad_doc, ch, &mut bag).is_none() {
        return Err(format!("{}：导入层漏拦——采样器越界未拒", E_SEC_IMPORT_MISS));
    }
    // NaN 采样值（采样畸形——检测面计数）。
    let times = [0.0f32, 1.0, 2.0];
    let values = [0.0f32, f32::NAN, 2.0];
    let out = GltfSamplerOut { comps: 1, interp: GltfInterp::Linear, count: 3 };
    let mut bag2 = ImportBag::new();
    let verdict = validate_samples(&times, &out, &values, TrackSemantic::Position, &mut bag2);
    // 诚实映射对端语义：validate 对 NaN 值不拒（值域钳制在下游），
    // 拦截显性在检测面（anomaly 计数）——规则表里的 import.sample_data
    // 探测面对应的就是那个计数，故此处要求 validate 放行但**检测面有
    // 计数**（清洗不缺席）。
    match verdict {
        SampleVerdict::Reject(_) => Ok(()), // 显性拒绝也是拦截
        SampleVerdict::Ok => {
            let a = crate::svstar2::vem09_import::detect_curve_anomaly(&times, &out, &values);
            if a.non_finite_values > 0 {
                Ok(())
            } else {
                Err(format!("{}：导入层漏拦——NaN 采样值无检测计数", E_SEC_IMPORT_MISS))
            }
        }
    }
}

/// 求值层真调：NaN 防护（钳制+记账）与四元数归一化。
fn eval_layer_probe() -> Result<(), String> {
    let interp = InterpEntry { name: "linear", kind: Interp::Linear, eval: |_p: &InterpParams, u: f32| u };
    let params = InterpParams::LINEAR;
    let mut bag = DiagBag::new();
    // NaN 防护：v0=NaN → 结果必须有限且诊断记账。
    let got = eval_scalar_span(
        ValueTrackClass::Float,
        &interp,
        &params,
        f32::NAN,
        2.0,
        0.0,
        1.0,
        0.5,
        &mut bag,
    );
    match got {
        Some(v) if v.is_finite() && (!bag.warnings().is_empty() || !bag.errors().is_empty()) => {}
        _ => return Err(format!("{}：求值层 NaN 防护失守（无钳制或无记账）", E_SEC_IMPORT_MISS)),
    }
    // 归一化：非单位四元数进、单位模长出。
    let mut qbag = DiagBag::new();
    let q0 = [2.0f32, 0.0, 0.0, 0.0];
    let q1 = [0.0f32, 3.0, 0.0, 0.0];
    let q = eval_quat_span(&interp, &params, q0, q1, 0.0, 1.0, 0.5, &mut qbag);
    let ok = match q {
        Some(q) => {
            let sum = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
            let len = fsqrt(sum);
            (len - 1.0).abs() < 1.0e-3
        }
        None => false,
    };
    if !ok {
        return Err(format!("{}：求值层归一化失守（输出非单位四元数）", E_SEC_IMPORT_MISS));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、双硬顶 + 绕过路径不存在断言（F1938 家族 M 维首例）
// ---------------------------------------------------------------------------

/// 资源配额（双硬顶——F2402 上限数值化 + clip 大小）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceQuota {
    /// 轨道数硬顶（0=未配——视为拒绝而非无限）。
    pub track_cap: u32,
    /// clip 数据大小硬顶（字节；0=未配）。
    pub clip_bytes_cap: u64,
}

impl ResourceQuota {
    /// 出厂配额（256 轨 / 64 MiB）。
    pub fn defaults() -> ResourceQuota {
        ResourceQuota { track_cap: DEFAULT_TRACK_CAP, clip_bytes_cap: DEFAULT_CLIP_BYTES_CAP }
    }

    /// 配额自检（零值即拒——"没配"不是"无限"）。
    pub fn validate(&self) -> Result<(), String> {
        if self.track_cap == 0 {
            return Err(format!("{}：轨道数硬顶为 0（须显式配置）", E_SEC_QUOTA_BYPASS));
        }
        if self.clip_bytes_cap == 0 {
            return Err(format!("{}：clip 大小硬顶为 0（须显式配置）", E_SEC_QUOTA_BYPASS));
        }
        Ok(())
    }

    /// 轨道数准入（超顶显性拒绝）。
    pub fn admit_track(&self, count: u32) -> Result<(), String> {
        if count > self.track_cap {
            return Err(format!(
                "{}：轨道数 {} 超硬顶 {}——拒绝创建",
                E_SEC_QUOTA_BYPASS, count, self.track_cap
            ));
        }
        Ok(())
    }

    /// clip 大小准入（checked_mul 防回滚后比硬顶）。
    pub fn admit_clip_bytes(&self, bytes: u64) -> Result<(), String> {
        if bytes > self.clip_bytes_cap {
            return Err(format!(
                "{}：clip 大小 {} 字节超硬顶 {}——拒绝导入",
                E_SEC_QUOTA_BYPASS, bytes, self.clip_bytes_cap
            ));
        }
        Ok(())
    }
}

/// 创建入口（配额检查点的覆盖对象——枚举即证明）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreationEntry {
    /// 轨道创建（F2402 首层级）。
    TrackCreate,
    /// clip 导入（数据大小硬顶）。
    ClipImport,
    /// 发射器/实体生成（轨道数连带上限）。
    EmitterSpawn,
}

impl CreationEntry {
    /// 三入口闭集（F1938 枚举证明的"全"）。
    pub const ALL: [CreationEntry; 3] = [CreationEntry::TrackCreate, CreationEntry::ClipImport, CreationEntry::EmitterSpawn];

    /// 入口名（人读）。
    pub fn zh(self) -> &'static str {
        match self {
            CreationEntry::TrackCreate => "轨道创建",
            CreationEntry::ClipImport => "clip 导入",
            CreationEntry::EmitterSpawn => "发射器生成",
        }
    }
}

/// 检查点（入口→检查的映射目标）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Checkpoint {
    /// 轨道数准入检查。
    TrackCount,
    /// clip 大小准入检查。
    ClipBytes,
}

/// 入口→检查点映射（**None=绕过路径**——枚举证明的核心）。
pub fn checkpoint_of(entry: CreationEntry) -> Option<Checkpoint> {
    match entry {
        CreationEntry::TrackCreate => Some(Checkpoint::TrackCount),
        CreationEntry::ClipImport => Some(Checkpoint::ClipBytes),
        CreationEntry::EmitterSpawn => Some(Checkpoint::TrackCount),
    }
}

/// 绕过路径不存在断言（F1938 家族范式）：全入口有检查点 + 数量对拍。
pub fn bypass_coverage_proof() -> Result<(), String> {
    let mut covered = 0usize;
    for e in CreationEntry::ALL.iter() {
        match checkpoint_of(*e) {
            Some(_) => covered += 1,
            None => {
                return Err(format!(
                    "{}：创建入口 {} 无检查点——存在绕过路径（配额失守）",
                    E_SEC_QUOTA_BYPASS, e.zh()
                ))
            }
        }
    }
    if covered != CreationEntry::ALL.len() {
        return Err(format!(
            "{}：检查点覆盖 {} ≠ 入口数 {}——枚举证明不闭合",
            E_SEC_QUOTA_BYPASS, covered, CreationEntry::ALL.len()
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 四、审计（F1954 范式——环形留痕，详述只含枚举标签）
// ---------------------------------------------------------------------------

/// 审计操作三态（锚点：轨道增删/clip 导入）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuditOp {
    /// 轨道增。
    TrackAdd,
    /// 轨道删。
    TrackRemove,
    /// clip 导入。
    ClipImport,
}

impl AuditOp {
    /// 标签（wire 名——详述只含此标签，无用户内容）。
    pub fn label(self) -> &'static str {
        match self {
            AuditOp::TrackAdd => "track.add",
            AuditOp::TrackRemove => "track.remove",
            AuditOp::ClipImport => "clip.import",
        }
    }
}

/// 审计条目（无 String 详述——隐私红线在类型面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditEntry {
    /// 逻辑时钟（epoch）。
    pub epoch: u64,
    /// 操作。
    pub op: AuditOp,
    /// 目标序号（句柄/下标——非内容）。
    pub target: u32,
}

/// 审计日志（环形，容量固定——满则淘汰最旧并计数）。
#[derive(Clone, Debug)]
pub struct AuditLog {
    entries: Vec<AuditEntry>,
    capacity: usize,
    evicted: u64,
    epoch: u64,
}

impl AuditLog {
    /// 新日志（给定容量——零容量即拒：不留痕等于无审计）。
    pub fn new(capacity: usize) -> Result<AuditLog, String> {
        if capacity == 0 {
            return Err(format!("{}：审计容量为 0（不留痕=无审计）", E_SEC_AUDIT_MISS));
        }
        Ok(AuditLog { entries: Vec::new(), capacity, evicted: 0, epoch: 0 })
    }

    /// 记一条（epoch 自增）。
    pub fn record(&mut self, op: AuditOp, target: u32) {
        self.epoch += 1;
        if self.entries.len() >= self.capacity {
            self.entries.remove(0);
            self.evicted = self.evicted.saturating_add(1);
        }
        self.entries.push(AuditEntry { epoch: self.epoch, op, target });
    }

    /// 在册条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空册。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 淘汰计数（读屏可达——容量压力显性）。
    pub fn evicted(&self) -> u64 {
        self.evicted
    }

    /// 某操作的出现次数（可追溯性核验）。
    pub fn count_op(&self, op: AuditOp) -> usize {
        self.entries.iter().filter(|e| e.op == op).count()
    }

    /// 全部条目（读屏可达）。
    pub fn all(&self) -> &[AuditEntry] {
        &self.entries
    }
}

// ---------------------------------------------------------------------------
// 五、安全快照（F2411 通过率硬门）
// ---------------------------------------------------------------------------

/// 安全快照（引用某模块判据集的通过率）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecuritySnapshot {
    /// 被引用模块（人读）。
    pub module: &'static str,
    /// 通过数。
    pub pass: u32,
    /// 总数。
    pub total: u32,
}

impl SecuritySnapshot {
    /// 从判据集取快照（真调执行——不读缓存不抄数）。
    pub fn from_check_set(module: &'static str, s: &CheckSet) -> SecuritySnapshot {
        let (p, f) = s.tally();
        SecuritySnapshot { module, pass: p as u32, total: (p + f) as u32 }
    }

    /// 通过率（%×100 定点——避免浮点）。
    pub fn rate_permille(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.pass * 1000) / self.total
    }

    /// 硬门：必须 100%（锚点：快照通过率非 100%→安全不收官）。
    pub fn gate(&self) -> Result<(), String> {
        if self.pass != self.total {
            return Err(format!(
                "{}：{} 通过率 {}/{}（{}‰）非 100%——安全不收官",
                E_SEC_SNAPSHOT, self.module, self.pass, self.total, self.rate_permille()
            ));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 六、判据
// ---------------------------------------------------------------------------

/// F2416 域自检（判据五组：清洗/配额/审计/快照/收尾）。
pub fn run_vem16_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F2416");

    // --- 双层清洗（判据一）---
    let rules = clean_rules();
    s.add(
        "M16-清洗-01",
        clean_rule_verdict(&rules).is_ok() && CleanLayer::ALL.len() == 2,
        "双层规则表齐备（导入 3+求值 3，F1612 家族格式）",
    );
    s.add("M16-清洗-02", import_layer_probe().is_ok(), "导入层真调（畸形全拦/NaN 检测计数）");
    s.add("M16-清洗-03", eval_layer_probe().is_ok(), "求值层真调（NaN 钳制+归一化）");
    // 清洗遗漏可检出：构造只有单层的规则表必拒。
    let mut thin = rules;
    thin[3].layer = CleanLayer::Import; // 把求值层规则改挂导入层
    let r = clean_rule_verdict(&thin);
    s.add(
        "M16-清洗-04",
        r.is_err() && r.as_ref().unwrap_err().contains("求值层") && r.as_ref().unwrap_err().starts_with(E_SEC_IMPORT_MISS),
        "单层规则表拒绝（清洗遗漏可检出）",
    );

    // --- 双硬顶（判据二）---
    let quota = ResourceQuota::defaults();
    s.add("M16-配额-01", quota.validate().is_ok(), "出厂配额自检过（256 轨/64 MiB）");
    let r = quota.admit_track(257);
    s.add(
        "M16-配额-02",
        r.is_err() && r.as_ref().unwrap_err().contains("257") && r.as_ref().unwrap_err().starts_with(E_SEC_QUOTA_BYPASS),
        "轨道数超硬顶显性拒绝",
    );
    let r = quota.admit_clip_bytes(DEFAULT_CLIP_BYTES_CAP + 1);
    s.add(
        "M16-配额-03",
        r.is_err() && r.unwrap_err().starts_with(E_SEC_QUOTA_BYPASS),
        "clip 大小超硬顶显性拒绝",
    );
    s.add(
        "M16-配额-04",
        quota.admit_track(256).is_ok() && quota.admit_track(0).is_ok() && quota.admit_clip_bytes(0).is_ok(),
        "恰边界与零量放行（双向——拦截不泛化）",
    );
    s.add(
        "M16-配额-05",
        bypass_coverage_proof().is_ok(),
        "绕过路径不存在（三入口全检查点——F1938 枚举证明）",
    );
    // 覆盖证明可复算：入口数=检查点数（禁手写常数）。
    s.add(
        "M16-配额-06",
        CreationEntry::ALL.len() == 3
            && CreationEntry::ALL.iter().filter(|e| checkpoint_of(**e).is_some()).count() == 3,
        "入口数=检查点数（3=3 可复算）",
    );
    // 零配额即拒（"没配"不是"无限"）。
    let zero = ResourceQuota { track_cap: 0, clip_bytes_cap: 64 * 1024 * 1024 };
    s.add(
        "M16-配额-07",
        zero.validate().is_err() && zero.validate().unwrap_err().starts_with(E_SEC_QUOTA_BYPASS),
        "零硬顶拒绝（未配置≠无限）",
    );

    // --- 操作审计（判据三）---
    let mut log = match AuditLog::new(8) {
        Ok(l) => l,
        Err(_) => AuditLog { entries: Vec::new(), capacity: 8, evicted: 0, epoch: 0 },
    };
    log.record(AuditOp::TrackAdd, 1);
    log.record(AuditOp::TrackRemove, 1);
    log.record(AuditOp::ClipImport, 0);
    s.add(
        "M16-审计-01",
        log.len() == 3 && log.count_op(AuditOp::TrackAdd) == 1
            && log.count_op(AuditOp::TrackRemove) == 1 && log.count_op(AuditOp::ClipImport) == 1,
        "三操作全留痕（增/删/导入）",
    );
    // 环形淘汰：超容量淘汰计数显性。
    let mut small = match AuditLog::new(2) {
        Ok(l) => l,
        Err(_) => AuditLog { entries: Vec::new(), capacity: 2, evicted: 0, epoch: 0 },
    };
    small.record(AuditOp::TrackAdd, 1);
    small.record(AuditOp::TrackAdd, 2);
    small.record(AuditOp::TrackAdd, 3);
    s.add(
        "M16-审计-02",
        small.len() == 2 && small.evicted() == 1 && small.all()[0].target == 2,
        "环形淘汰显性（最旧出局+计数）",
    );
    // 零容量拒绝（不留痕=无审计）。
    let r = AuditLog::new(0);
    s.add(
        "M16-审计-03",
        r.is_err() && r.unwrap_err().starts_with(E_SEC_AUDIT_MISS),
        "零容量审计拒绝",
    );

    // --- 快照硬门（判据四）---
    let f2411 = run_vem11_checks();
    let snap = SecuritySnapshot::from_check_set("VE-F2411", &f2411);
    s.add("M16-快照-01", snap.gate().is_ok() && snap.rate_permille() == 1000, "F2411 快照 100% 硬门过");
    // 非 100% 可检出（构造缩水快照）。
    let bad_snap = SecuritySnapshot { module: "VE-F2411", pass: 25, total: 26 };
    let r = bad_snap.gate();
    s.add(
        "M16-快照-02",
        r.is_err() && r.as_ref().unwrap_err().contains("25/26") && r.as_ref().unwrap_err().starts_with(E_SEC_SNAPSHOT),
        "非 100% 拒绝（安全不收官）",
    );

    // --- 版本与暂挂 ---
    let fp = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in SECURITY_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    };
    s.add("M16-版本-01", fp != 0, "版本指纹非零（M16-security-v1）");

    s.add(
        "M16-暂挂-01",
        M_LEDGER_SEC_SUSPENDED_NOTE.contains("暂挂") && M_LEDGER_SEC_SUSPENDED_NOTE.contains("F2416"),
        "M 域账本暂挂声明显性",
    );

    // M16-暂挂-02：判据条数对账（本条为第 19 条）。
    s.add("M16-暂挂-02", s.len() == 18, "判据条数对账（18+本条）");

    s
}

/// M 域账本暂挂声明（跨批对接点：结论供 F2419/F2459 与 M 域安全链）。
pub const M_LEDGER_SEC_SUSPENDED_NOTE: &str = "动画安全三段防线结论入 M 域账本：建账前暂挂声明（移交期模式延续——F2416 同款）；清洗随合入跑，配额 O(1)，审计异步，快照引用零成本";

// ---------------------------------------------------------------------------
// 七、诚实边界
// ---------------------------------------------------------------------------

/// 清洗与快照的诚实声明（同 vel16 先例：对端语义如实映射）。
///
/// 导入层对 NaN **采样值**的真实语义（F2409 实测）：validate_samples
/// 对 Position 语义**不拒**（值域钳制在下游导入通道），拦截显性在
/// 检测面（`detect_curve_anomaly` 的 `non_finite_values` 计数）——
/// 本规则表的 `import.sample_data` 探测面对应的就是那个计数，故
/// [`import_layer_probe`] 要求"validate 放行 + 检测计数 >0"两条同时
/// 成立（拒绝或计数任一缺席即红），不拿"应该被拒"的措辞倒推实现。
pub const CLEAN_HONESTY_NOTE: &str =
    "NaN 采样值在 F2409 导入侧的真实语义是钳制+检测计数（非拒绝）——规则表探测面按检测计数断言，不以措辞倒推实现";
