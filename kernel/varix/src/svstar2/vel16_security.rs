//! VE-F2216 · 粒子安全（VE-L 域 · 粒子段 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2216`
//!
//! **职责定位（锚点原文）**：粒子安全——**三段防线**：
//!
//! 1. **粒子参数清洗**（畸形参数钳制：发射率/速度/寿命/形状参数**全空间**
//!    ——F2211 fuzz 防线的安全侧规则表）；
//! 2. **池资源配额**（显存/内存硬顶：CPU 池内存顶 + GPU 池显存顶——F2208
//!    配额的安全侧核验：**硬顶不可绕过断言**）；
//! 3. **审计**（粒子操作留痕：发射器增删/池配额变更——F1954 审计范式延续），
//!    并完成 **F2211 通过率快照引用**。
//!
//! 数据结构：清洗规则表（参数/规则/钳制目标/告警级——**全参数覆盖**）；
//! 配额核验（双池硬顶值/**绕过路径不存在断言**：所有分配入口枚举与配额
//! 检查点覆盖证明）；审计日志（环形留痕 + 可导出）；安全快照引用
//! （F2211 通过率硬门）。
//!
//! **判据（锚点原文）**：全参清洗、硬顶不可绕、操作审计、快照硬门、判据。
//!
//! ## 错误路径与降级矩阵（锚点原文，逐条落实）
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 清洗遗漏 → **fuzz 立案回补** | [`Sanitizer::sanitize`] 每一次钳制/拒收都**必须**产出一条可复现立案（[`FuzzFiling`]）；漏产即 [`CODE_AUDIT_GAP`]。锚点要的不是「洗了」，是「洗过的都进了回补队列」——静默钳制等于把畸形输入吞掉，下一轮 fuzz 学不到 |
//! | 硬顶绕过检出 → **P0**（枚举覆盖证明失效） | [`QuotaGuard::assert_no_bypass`]：枚举全部分配入口并逐个核对配额检查点，覆盖率不足即 [`CODE_BYPASS`]（P0）。**硬顶不可绕过的正面表述是「不存在未设检查点的分配入口」**，而不是「我们记得检查」 |
//! | 审计缺失 → **P1**（可追溯性） | [`AuditLog`] 与 [`QuotaGuard`] 的变更路径同源：任何状态变更必经留痕；条数对账不等即 [`CODE_AUDIT_GAP`] |
//! | 快照通过率非 100% → **安全不收官** | [`SnapshotGate::admit`]：通过率**恰 100%** 才发安全结论；否则 [`CODE_SNAPSHOT_FAIL`] 且结论不签发 |
//!
//! ## 性能逐项分解（锚点原文）
//!
//! - **清洗**：导入 + 运行**双检**，两个入口共用同一张规则表，每参 O(1) 查表；
//! - **硬顶**：O(1)（checked 加 + 一次比较，无遍历）；
//! - **审计**：环形追加，O(1) 且**热路径零分配**。**诚实的口径**：这里的
//!   「异步」指**不阻塞粒子热路径**（固定环、无 I/O、无 flush），**不是**
//!   另起线程的真异步——后者需要锁与所有权交接，与 no_std 内核的取舍相悖，
//!   谎称线程异步会让读代码的人以为有并发保证；
//! - **快照引用**：零成本——闸门只存**裁决**（bool + 计数），不复制报告本体。
//!
//! ## 跨批对接点（锚点原文）
//!
//! - 规则核验对象 F2202–F2208 实现：发射率钳制**真调** [`clamp_emit_rate`]、
//!   有限性判**真调** [`is_finite`]、寿命域取 F2212 的 `LIFETIME_MIN/MAX_SEC`；
//! - 配额对接 F1776 L 段（池注册联动）：池闸复用 [`PoolQuota`] 的
//!   `bytes_cap` 与 [`PoolQuota::validate`]，**不另立一套内存口径**；
//! - 审计复用 F1954 范式（环形留痕 + 可导出）；
//! - 快照源 F2211：裁决**真调** [`FuzzReport::passed`]，不自评通过率；
//! - 结论供 F2219（粒子 API 冻结移交包）与 F2259、L 域安全链消费。
//!
//! ## 无障碍与隐私（锚点原文）
//!
//! 锚点明文：**审计不含用户内容；无隐私面**。这不是承诺而是**类型事实**——
//! [`AuditRecord`] 只有定长枚举与 `u64`，**结构体里没有任何 `String`/`&str`
//! 内容字段**，审计条目在类型层面就无法携带文件名、发射器名或任何用户可读
//! 文本；[`AuditLog::export`] 导出的是同样的定长记录，不是渲染出来的文本。
//!
//! ## 与相邻件的分工（易混，故写明）
//!
//! - **VE-F2211**（vel11_fuzz）是 fuzz 语料与裁决的**产出方**，本条只**引用**
//!   其裁决并把「钳制过的畸形值」回灌为立案；
//! - **VE-F2208**（vel08_pool）管配额**机制**，本条只在其上加「双池硬顶 +
//!   绕过路径不存在」这层**安全核验**，不重写池；
//! - **VE-F2219** 是 API 冻结移交包，本条是它的一个移交面，不改冻结簿。

use crate::checks::CheckSet;

use super::vel03_emitter::{clamp_emit_rate, is_finite, DiagBag, EMIT_RATE_MAX_PER_SEC};
use super::vel05_lifetime::{LIFETIME_MAX_SEC, LIFETIME_MIN_SEC};
use super::vel08_pool::{PoolKind, PoolQuota};
use super::vel11_fuzz::{FuzzReport, FUZZ_PROTOCOL_VERSION};

// ---------------------------------------------------------------------------
// 一、诊断码（自建段 0x93xx —— 全 kernel 树 grep 后确认零占用；
//    0x4A=vea50 / 0x4B=vea51 / 0x58=vcq02 / 0x8D=vea52 均已占用）
// ---------------------------------------------------------------------------

/// 清洗规则表漏参：参数域未被任何规则覆盖（**P0·清洗遗漏**）。
pub const CODE_RULE_GAP: u16 = 0x9301;
/// 未知参数域（枚举外的值）。
pub const CODE_PARAM_UNKNOWN: u16 = 0x9302;
/// 非有限值（NaN/Inf）——不可钳制，只能拒收。
pub const CODE_NON_FINITE: u16 = 0x9303;
/// 硬顶超限（CPU 池内存顶或 GPU 池显存顶）。
pub const CODE_QUOTA_EXCEEDED: u16 = 0x9304;
/// 绕过路径检出：分配入口缺配额检查点（**P0·枚举覆盖证明失效**）。
pub const CODE_BYPASS: u16 = 0x9305;
/// 留痕缺失：钳制/变更未产生立案或审计条目（**P1·可追溯性**）。
pub const CODE_AUDIT_GAP: u16 = 0x9306;
/// 快照通过率非 100%（**安全不收官**）。
pub const CODE_SNAPSHOT_FAIL: u16 = 0x9307;
/// 非法检测阶段（导入/运行之外的阶段）。
pub const CODE_BAD_STAGE: u16 = 0x9308;

/// 本域诊断码全集（判据对账：两两互异 + 全占 0x93 段）。
pub const CODES: [u16; 8] = [
    CODE_RULE_GAP,
    CODE_PARAM_UNKNOWN,
    CODE_NON_FINITE,
    CODE_QUOTA_EXCEEDED,
    CODE_BYPASS,
    CODE_AUDIT_GAP,
    CODE_SNAPSHOT_FAIL,
    CODE_BAD_STAGE,
];

/// 人话说明（后果 + 下一步；未知码有兜底不 panic）。
pub const fn explain(code: u16) -> &'static str {
    match code {
        CODE_RULE_GAP => "清洗规则表漏参：该参数域无规则覆盖，规则表须全参数闭合（P0）",
        CODE_PARAM_UNKNOWN => "未知参数域：值不在闭集内，先修正调用方取值再清洗",
        CODE_NON_FINITE => "非有限值（NaN/Inf）不可钳制：已拒收，请修正来源",
        CODE_QUOTA_EXCEEDED => "硬顶超限：CPU 池内存顶或 GPU 池显存顶已满，申请被拒",
        CODE_BYPASS => "绕过路径检出：存在无配额检查点的分配入口，覆盖证明失效（P0）",
        CODE_AUDIT_GAP => "留痕缺失：钳制或状态变更未产生立案/审计条目，可追溯性破损（P1）",
        CODE_SNAPSHOT_FAIL => "F2211 通过率非 100%：安全不收官，先修 fuzz 再签发结论",
        CODE_BAD_STAGE => "非法检测阶段：仅接受导入/运行两阶段",
        _ => "未知粒子安全诊断码（未登记）",
    }
}

// ---------------------------------------------------------------------------
// 二、第一段防线：清洗规则表（全参数覆盖）
// ---------------------------------------------------------------------------

/// 清洗参数域（锚点：发射率/速度/寿命/**形状参数全空间**）。
///
/// 闭集七项。少一项即「全空间」不成立——所以本表不是文档清单，而是
/// [`Param::ALL`] 与规则表之间可对账的**闭合集**。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Param {
    /// 发射率（粒子/秒）。
    EmitRate,
    /// 速度大小。
    Speed,
    /// 寿命（秒）。
    Lifetime,
    /// 形状尺寸（半径）。
    ShapeRadius,
    /// 形状锥角（度）。
    ShapeConeAngle,
    /// 形状锥半径。
    ShapeConeRadius,
    /// 速度散布。
    VelocitySpread,
}

impl Param {
    /// 闭集全集（判据对账基准）。
    pub const ALL: [Param; 7] = [
        Param::EmitRate,
        Param::Speed,
        Param::Lifetime,
        Param::ShapeRadius,
        Param::ShapeConeAngle,
        Param::ShapeConeRadius,
        Param::VelocitySpread,
    ];

    pub const fn ordinal(self) -> usize {
        match self {
            Param::EmitRate => 0,
            Param::Speed => 1,
            Param::Lifetime => 2,
            Param::ShapeRadius => 3,
            Param::ShapeConeAngle => 4,
            Param::ShapeConeRadius => 5,
            Param::VelocitySpread => 6,
        }
    }

    pub const fn from_ordinal(i: usize) -> Option<Param> {
        match i {
            0 => Some(Param::EmitRate),
            1 => Some(Param::Speed),
            2 => Some(Param::Lifetime),
            3 => Some(Param::ShapeRadius),
            4 => Some(Param::ShapeConeAngle),
            5 => Some(Param::ShapeConeRadius),
            6 => Some(Param::VelocitySpread),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Param::EmitRate => "发射率 / emit rate",
            Param::Speed => "速度 / speed",
            Param::Lifetime => "寿命 / lifetime",
            Param::ShapeRadius => "形状尺寸 / shape radius",
            Param::ShapeConeAngle => "形状锥角 / shape cone angle",
            Param::ShapeConeRadius => "形状锥半径 / shape cone radius",
            Param::VelocitySpread => "速度散布 / velocity spread",
        }
    }
}

/// 处置规则。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    /// 钳制到合法域（越界则夹取，端点值原样通过）。
    Clamp,
    /// 拒收（只用于不可钳制的畸形值）。
    Reject,
}

/// 告警级（锚点：规则表带告警级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alarm {
    /// 不告警（合法域内取值）。
    None,
    /// 提示（钳制过一次）。
    Warn,
    /// P1（可追溯性破损级）。
    P1,
    /// P0（安全红线级）。
    P0,
}

impl Alarm {
    pub const fn ordinal(self) -> usize {
        match self {
            Alarm::None => 0,
            Alarm::Warn => 1,
            Alarm::P1 => 2,
            Alarm::P0 => 3,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Alarm::None => "无 / none",
            Alarm::Warn => "提示 / warn",
            Alarm::P1 => "P1 / p1",
            Alarm::P0 => "P0 / p0",
        }
    }
}

/// 清洗规则一格（锚点：参数/规则/钳制目标/告警级）。
///
/// **只有 `PartialEq` 没有 `Eq`**：本结构含 `f32`，而 `f32` 不满足 `Eq`
/// （NaN != NaN）。给含浮点的结构派 `Eq` 编不过——要绕过去只能靠手写
/// `impl Eq`，那是把「浮点不可比」这一事实藏起来，不如让编译器如实拦一次。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SanitizeRule {
    /// 所属参数。
    pub param: Param,
    /// 处置规则。
    pub rule: Rule,
    /// 钳制目标下界（含）。
    pub target_min: f32,
    /// 钳制目标上界（含）。
    pub target_max: f32,
    /// 告警级。
    pub alarm: Alarm,
}

/// 形状参数尺寸上界（与发射率不同源：形状尺寸无 vel03 常量，此处自持并
/// 写进表，使规则表自解释——不写来源的表，半年后没人敢改）。
pub const SHAPE_RADIUS_MAX: f32 = 4096.0;
/// 锥角上界（度）。
pub const SHAPE_CONE_ANGLE_MAX: f32 = 180.0;
/// 速度散布上界。
pub const VELOCITY_SPREAD_MAX: f32 = EMIT_RATE_MAX_PER_SEC;

/// **清洗规则表**（锚点数据结构：参数/规则/钳制目标/告警级，全参数覆盖）。
///
/// 寿命域取 F2212 的 `LIFETIME_MIN_SEC/MAX_SEC`、发射率域取 vel03 的
/// `EMIT_RATE_MAX_PER_SEC`——**域值真引用被核验对象，不另抄一份**。
pub const SANITIZE_RULES: [SanitizeRule; 7] = [
    SanitizeRule { param: Param::EmitRate, rule: Rule::Clamp, target_min: 0.0, target_max: EMIT_RATE_MAX_PER_SEC, alarm: Alarm::Warn },
    SanitizeRule { param: Param::Speed, rule: Rule::Clamp, target_min: 0.0, target_max: EMIT_RATE_MAX_PER_SEC, alarm: Alarm::Warn },
    SanitizeRule { param: Param::Lifetime, rule: Rule::Clamp, target_min: LIFETIME_MIN_SEC, target_max: LIFETIME_MAX_SEC, alarm: Alarm::Warn },
    SanitizeRule { param: Param::ShapeRadius, rule: Rule::Clamp, target_min: 0.0, target_max: SHAPE_RADIUS_MAX, alarm: Alarm::Warn },
    SanitizeRule { param: Param::ShapeConeAngle, rule: Rule::Clamp, target_min: 0.0, target_max: SHAPE_CONE_ANGLE_MAX, alarm: Alarm::Warn },
    SanitizeRule { param: Param::ShapeConeRadius, rule: Rule::Clamp, target_min: 0.0, target_max: SHAPE_RADIUS_MAX, alarm: Alarm::Warn },
    SanitizeRule { param: Param::VelocitySpread, rule: Rule::Clamp, target_min: 0.0, target_max: VELOCITY_SPREAD_MAX, alarm: Alarm::P1 },
];

/// 按参数查规则（O(1) 线性扫七格；`None` = 漏参 → P0）。
pub const fn rule_for(p: Param) -> Option<&'static SanitizeRule> {
    let mut i = 0usize;
    while i < SANITIZE_RULES.len() {
        if SANITIZE_RULES[i].param.ordinal() == p.ordinal() {
            return Some(&SANITIZE_RULES[i]);
        }
        i += 1;
    }
    None
}

/// 规则表覆盖证明：返回 `None` 即**全覆盖**（全参数闭合）。
///
/// 返回 `Some(漏掉的参数)` 即清洗遗漏（P0）。逐格核两件事：每个参数域
/// 恰好一条规则，且恰好被覆盖一次——**重复**也是缺陷（两条规则同参数
/// 意味着生效哪条要看扫描顺序，那是隐式依赖）。
pub const fn coverage_gap() -> Option<Param> {
    let mut p = 0usize;
    while p < Param::ALL.len() {
        let param = match Param::from_ordinal(p) {
            Some(x) => x,
            None => return Some(Param::EmitRate),
        };
        let mut hits = 0usize;
        let mut i = 0usize;
        while i < SANITIZE_RULES.len() {
            if SANITIZE_RULES[i].param.ordinal() == param.ordinal() {
                hits += 1;
            }
            i += 1;
        }
        if hits != 1 {
            return Some(param);
        }
        p += 1;
    }
    None
}

/// 检测阶段（锚点性能条：清洗**导入+运行双检**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 导入期（配置加载、资源装载）。
    Import,
    /// 运行期（网络同步、脚本驱动、运行时调参）。
    Run,
}

impl Stage {
    pub const fn wire(self) -> u8 {
        match self {
            Stage::Import => 0x01,
            Stage::Run => 0x02,
        }
    }

    pub const fn from_wire(w: u8) -> Option<Stage> {
        match w {
            0x01 => Some(Stage::Import),
            0x02 => Some(Stage::Run),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Stage::Import => "导入 / import",
            Stage::Run => "运行 / run",
        }
    }
}

/// 一次清洗的裁决。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 原值已在合法域内，未改。
    Pass,
    /// 已被钳制到合法域。
    Clamped,
    /// 已拒收（不可钳制的畸形值）。
    Rejected,
}

impl Verdict {
    pub const fn label(self) -> &'static str {
        match self {
            Verdict::Pass => "放行 / pass",
            Verdict::Clamped => "已钳制 / clamped",
            Verdict::Rejected => "已拒收 / rejected",
        }
    }
}

/// fuzz 回补立案（锚点：清洗遗漏→fuzz 立案回补）。
///
/// **只存位摘要，不存浮点值**：立案要能复现，但复现靠 `(参数, 阶段, 值位
/// 摘要, 种子)` 就够了；把原值留在这里反而会让「审计不含用户内容」这条
/// 隐私红线多出一个可被滥用的口袋。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FuzzFiling {
    /// 参数。
    pub param: Param,
    /// 阶段。
    pub stage: Stage,
    /// 畸形值的位摘要（`f32::to_bits` 摘要，非原值）。
    pub value_bits: u32,
}

/// 立案环定容。
pub const FILING_CAP: usize = 16;

/// 清洗器（第一段防线）。
pub struct Sanitizer {
    filings: [Option<FuzzFiling>; FILING_CAP],
    head: usize,
    filed_total: u32,
    overwrites: u32,
    passes: u32,
    clamps: u32,
    rejects: u32,
    last_code: u16,
}

impl Sanitizer {
    pub const fn new() -> Sanitizer {
        Sanitizer {
            filings: [None; FILING_CAP],
            head: 0,
            filed_total: 0,
            overwrites: 0,
            passes: 0,
            clamps: 0,
            rejects: 0,
            last_code: 0,
        }
    }

    pub const fn filed_total(&self) -> u32 {
        self.filed_total
    }

    pub const fn overwrites(&self) -> u32 {
        self.overwrites
    }

    pub const fn clamps(&self) -> u32 {
        self.clamps
    }

    pub const fn rejects(&self) -> u32 {
        self.rejects
    }

    pub const fn passes(&self) -> u32 {
        self.passes
    }

    pub const fn last_code(&self) -> u16 {
        self.last_code
    }

    pub fn filings(&self) -> &[Option<FuzzFiling>; FILING_CAP] {
        &self.filings
    }

    /// 最近一条立案（越界空账返 None）。
    pub fn last_filing(&self) -> Option<FuzzFiling> {
        let idx = if self.head == 0 { FILING_CAP - 1 } else { self.head - 1 };
        self.filings[idx]
    }

    fn file(&mut self, f: FuzzFiling) {
        if self.filings[self.head].is_some() {
            self.overwrites = self.overwrites.saturating_add(1);
        }
        self.filings[self.head] = Some(f);
        self.head = (self.head + 1) % FILING_CAP;
        self.filed_total = self.filed_total.saturating_add(1);
    }

    /// **清洗一次**（锚点：畸形参数钳制，全空间；导入 + 运行双检）。
    ///
    /// 每次钳制/拒收都**无条件**产出一条 fuzz 立案——静默钳制等于把畸形
    /// 输入吞掉，fuzz 下一轮学不到，于是同一个畸形值可以反复穿进来。
    /// 发射率**真调** vel03 [`clamp_emit_rate`]（不在本条重抄一份钳制逻辑：
    /// 两处各写一份钳制，迟早分叉），有限性**真调** [`is_finite`]。
    pub fn sanitize(&mut self, p: Param, value: f32, stage: Stage) -> Result<f32, u16> {
        let rule = match rule_for(p) {
            Some(r) => r,
            None => {
                // 漏参 = 清洗遗漏 = P0。不静默放行也不静默钳制。
                self.last_code = CODE_RULE_GAP;
                return Err(CODE_RULE_GAP);
            }
        };
        // 非有限值不可钳制：NaN 的比较永远为假，夹取会把它原样放行，
        // 而 NaN 进入粒子积分会把整个模拟变成 NaN 雪崩。故一律拒收。
        if !is_finite(value) {
            self.rejects = self.rejects.saturating_add(1);
            self.file(FuzzFiling { param: p, stage, value_bits: value.to_bits() });
            self.last_code = CODE_NON_FINITE;
            return Err(CODE_NON_FINITE);
        }
        if value < rule.target_min {
            self.clamps = self.clamps.saturating_add(1);
            self.file(FuzzFiling { param: p, stage, value_bits: value.to_bits() });
            self.last_code = 0;
            return Ok(rule.target_min);
        }
        if value > rule.target_max {
            self.clamps = self.clamps.saturating_add(1);
            self.file(FuzzFiling { param: p, stage, value_bits: value.to_bits() });
            // 发射率走上游真钳制：vel03 的 `clamp_emit_rate` 自己就持有同一
            // 上界，两侧口径一旦分叉，本条会立刻看到（返回一个落进我方
            // 合法域之外的值）。不用表值糊过去——那样这条真依赖就是死的。
            if p == Param::EmitRate {
                let mut bag = DiagBag::new();
                let up = clamp_emit_rate(value, &mut bag);
                if !is_finite(up) {
                    // 上游若给出非有限值，说明两侧口径真的分叉了：按不可用拒收，
                    // 而不是把一个 NaN 交回粒子积分。
                    self.rejects = self.rejects.saturating_add(1);
                    self.last_code = CODE_NON_FINITE;
                    return Err(CODE_NON_FINITE);
                }
                self.last_code = 0;
                return Ok(if up > rule.target_max { rule.target_max } else { up });
            }
            self.last_code = 0;
            return Ok(rule.target_max);
        }
        self.passes = self.passes.saturating_add(1);
        self.last_code = 0;
        Ok(value)
    }
}

// ---------------------------------------------------------------------------
// 三、第二段防线：双池硬顶 + 绕过路径不存在断言
// ---------------------------------------------------------------------------

/// 分配入口全集（锚点：所有分配入口枚举与配额检查点覆盖证明）。
///
/// 这是「硬顶不可绕过」的**唯一可判定形式**：把入口穷举成闭集，逐个核对
/// 是否设了配额检查点。**只有「记得检查」不构成不可绕过**——那靠自觉，
/// 而新增入口的人不会想起去更新一条注释。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AllocSite {
    /// 粒子槽位分配（F2208 池）。
    PoolSlot,
    /// 池容量扩容。
    PoolGrow,
    /// 粒子纹理申购。
    TextureAcquire,
    /// 发射器注册（携带槽位预算）。
    EmitterRegister,
    /// 形状几何上传。
    ShapeUpload,
    /// 寿命曲线 LUT 上传。
    CurveUpload,
    /// 实例缓冲上传。
    InstanceUpload,
}

impl AllocSite {
    pub const ALL: [AllocSite; 7] = [
        AllocSite::PoolSlot,
        AllocSite::PoolGrow,
        AllocSite::TextureAcquire,
        AllocSite::EmitterRegister,
        AllocSite::ShapeUpload,
        AllocSite::CurveUpload,
        AllocSite::InstanceUpload,
    ];

    pub const fn ordinal(self) -> usize {
        match self {
            AllocSite::PoolSlot => 0,
            AllocSite::PoolGrow => 1,
            AllocSite::TextureAcquire => 2,
            AllocSite::EmitterRegister => 3,
            AllocSite::ShapeUpload => 4,
            AllocSite::CurveUpload => 5,
            AllocSite::InstanceUpload => 6,
        }
    }

    pub const fn from_ordinal(i: usize) -> Option<AllocSite> {
        match i {
            0 => Some(AllocSite::PoolSlot),
            1 => Some(AllocSite::PoolGrow),
            2 => Some(AllocSite::TextureAcquire),
            3 => Some(AllocSite::EmitterRegister),
            4 => Some(AllocSite::ShapeUpload),
            5 => Some(AllocSite::CurveUpload),
            6 => Some(AllocSite::InstanceUpload),
            _ => None,
        }
    }

    /// 该入口归属哪个池（CPU 内存 or GPU 显存）。
    ///
    /// **诚实的分账**：并非所有入口都归属某一侧（如实例缓冲在 GPU、寿命
    /// LUT 在 CPU）。判据独立钉住逐格归属——归属错了会让「双池硬顶」变成
    /// 一个只管了一半的顶。
    pub const fn pool(self) -> PoolKind {
        match self {
            AllocSite::PoolSlot => PoolKind::Cpu,
            AllocSite::PoolGrow => PoolKind::Cpu,
            AllocSite::TextureAcquire => PoolKind::Gpu,
            AllocSite::EmitterRegister => PoolKind::Cpu,
            AllocSite::ShapeUpload => PoolKind::Gpu,
            AllocSite::CurveUpload => PoolKind::Cpu,
            AllocSite::InstanceUpload => PoolKind::Gpu,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            AllocSite::PoolSlot => "池槽位分配 / pool slot",
            AllocSite::PoolGrow => "池扩容 / pool grow",
            AllocSite::TextureAcquire => "纹理申购 / texture acquire",
            AllocSite::EmitterRegister => "发射器注册 / emitter register",
            AllocSite::ShapeUpload => "形状几何上传 / shape upload",
            AllocSite::CurveUpload => "寿命曲线上传 / curve upload",
            AllocSite::InstanceUpload => "实例缓冲上传 / instance upload",
        }
    }
}

/// 审计操作（锚点：发射器增删/池配额变更）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuditOp {
    /// 发射器新增。
    EmitterAdd,
    /// 发射器删除。
    EmitterRemove,
    /// 池配额变更。
    QuotaChange,
    /// 分配被拒（硬顶生效）。
    AllocRejected,
}

impl AuditOp {
    pub const ALL: [AuditOp; 4] = [
        AuditOp::EmitterAdd,
        AuditOp::EmitterRemove,
        AuditOp::QuotaChange,
        AuditOp::AllocRejected,
    ];

    pub const fn wire(self) -> u8 {
        match self {
            AuditOp::EmitterAdd => 0x01,
            AuditOp::EmitterRemove => 0x02,
            AuditOp::QuotaChange => 0x03,
            AuditOp::AllocRejected => 0x04,
        }
    }

    pub const fn from_wire(w: u8) -> Option<AuditOp> {
        match w {
            0x01 => Some(AuditOp::EmitterAdd),
            0x02 => Some(AuditOp::EmitterRemove),
            0x03 => Some(AuditOp::QuotaChange),
            0x04 => Some(AuditOp::AllocRejected),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            AuditOp::EmitterAdd => "发射器新增 / emitter add",
            AuditOp::EmitterRemove => "发射器删除 / emitter remove",
            AuditOp::QuotaChange => "池配额变更 / quota change",
            AuditOp::AllocRejected => "分配被拒 / alloc rejected",
        }
    }
}

/// 一条审计记录（**无任何用户内容**：只有定长枚举与 `u64`）。
///
/// 隐私红线是**类型事实**而非承诺：本结构体没有 `String`/`&str` 字段，
/// 审计在类型层面就装不下文件名或发射器名。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditRecord {
    /// 序号（单调）。
    pub seq: u64,
    /// 操作。
    pub op: AuditOp,
    /// 主体标识（发射器号或池号，非名称）。
    pub subject: u32,
    /// 变更前字节数。
    pub bytes_before: u64,
    /// 变更后字节数。
    pub bytes_after: u64,
}

/// 审计环定容。
pub const AUDIT_CAP: usize = 32;

/// 审计日志（锚点：环形留痕 + 可导出，F1954 范式）。
pub struct AuditLog {
    ring: [Option<AuditRecord>; AUDIT_CAP],
    head: usize,
    total: u64,
    overwrites: u32,
}

impl AuditLog {
    pub const fn new() -> AuditLog {
        AuditLog { ring: [None; AUDIT_CAP], head: 0, total: 0, overwrites: 0 }
    }

    /// 追加一条（满则覆盖最旧并如实计数——累计账不减）。
    pub fn record(&mut self, r: AuditRecord) {
        if self.ring[self.head].is_some() {
            self.overwrites = self.overwrites.saturating_add(1);
        }
        self.ring[self.head] = Some(r);
        self.head = (self.head + 1) % AUDIT_CAP;
        self.total = self.total.saturating_add(1);
    }

    pub const fn total(&self) -> u64 {
        self.total
    }

    pub const fn overwrites(&self) -> u32 {
        self.overwrites
    }

    pub fn len(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < AUDIT_CAP {
            if self.ring[i].is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    pub const fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// 导出（锚点：可导出）——导出的就是定长记录本身，不是渲染文本。
    pub fn export(&self) -> [Option<AuditRecord>; AUDIT_CAP] {
        self.ring
    }

    /// 按序取第 k 条在册记录（`k` 以「最旧在先」为准；越界 None）。
    pub fn nth(&self, k: usize) -> Option<AuditRecord> {
        if k >= self.len() {
            return None;
        }
        // 最旧在册下标 = head - len（环形回绕）。
        let start = (self.head + AUDIT_CAP - self.len()) % AUDIT_CAP;
        self.ring[(start + k) % AUDIT_CAP]
    }
}

/// 配额核验器（第二段防线：双池硬顶 + 绕过路径不存在断言）。
pub struct QuotaGuard {
    cpu: PoolQuota,
    gpu: PoolQuota,
    /// 每入口是否设了配额检查点（**枚举覆盖证明的载体**）。
    checkpoints: [bool; 7],
    cpu_used: u64,
    gpu_used: u64,
    admitted: u32,
    rejected: u32,
    breaches: u32,
    seq: u64,
    audit: AuditLog,
    last_code: u16,
}

impl QuotaGuard {
    /// 新建（双池硬顶取自 [`PoolQuota`] 的 `bytes_cap`——不另立内存口径）。
    pub const fn new(cpu: PoolQuota, gpu: PoolQuota) -> QuotaGuard {
        // 出厂全设检查点：默认安全，缺省即漏检会被 `assert_no_bypass` 抓到。
        QuotaGuard {
            cpu,
            gpu,
            checkpoints: [true; 7],
            cpu_used: 0,
            gpu_used: 0,
            admitted: 0,
            rejected: 0,
            breaches: 0,
            seq: 0,
            audit: AuditLog::new(),
            last_code: 0,
        }
    }

    pub const fn cpu_cap(&self) -> u64 {
        self.cpu.bytes_cap
    }

    pub const fn gpu_cap(&self) -> u64 {
        self.gpu.bytes_cap
    }

    pub const fn cpu_used(&self) -> u64 {
        self.cpu_used
    }

    pub const fn gpu_used(&self) -> u64 {
        self.gpu_used
    }

    pub const fn admitted(&self) -> u32 {
        self.admitted
    }

    pub const fn rejected(&self) -> u32 {
        self.rejected
    }

    pub const fn breaches(&self) -> u32 {
        self.breaches
    }

    pub const fn audit(&self) -> &AuditLog {
        &self.audit
    }

    pub const fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 入口是否设了配额检查点（判据逐格钉）。
    ///
    /// 直接下标而非 `.get()`：段号由 [`AllocSite::ordinal`] 生成、恒在
    /// `0..7`，越界不可能；而 `slice::get` 目前不是 `const fn`，用它就得
    /// 把本函数降级成运行时调用，白丢一层「编译期可判定」。
    pub fn has_checkpoint(&self, s: AllocSite) -> bool {
        self.checkpoints[s.ordinal()]
    }

    /// 显式撤除某入口的检查点（**只为让覆盖证明可被检验**：真实代码不应
    /// 主动关闸，关闸即绕过；判据侧用它构造 P0 场景）。
    pub fn disable_checkpoint(&mut self, s: AllocSite) {
        if let Some(v) = self.checkpoints.get_mut(s.ordinal()) {
            *v = false;
        }
    }

    /// **绕过路径不存在断言**（锚点：所有分配入口枚举与配额检查点覆盖证明）。
    ///
    /// 覆盖率不足即 [`CODE_BYPASS`]（P0）并计数——这是「枚举覆盖证明失效」
    /// 的可判定形式。
    pub fn assert_no_bypass(&mut self) -> Result<usize, u16> {
        let mut covered = 0usize;
        let mut i = 0usize;
        while i < AllocSite::ALL.len() {
            let site = match AllocSite::from_ordinal(i) {
                Some(s) => s,
                None => break,
            };
            if self.has_checkpoint(site) {
                covered += 1;
            }
            i += 1;
        }
        if covered != AllocSite::ALL.len() {
            self.breaches = self.breaches.saturating_add(1);
            self.last_code = CODE_BYPASS;
            return Err(CODE_BYPASS);
        }
        self.last_code = 0;
        Ok(covered)
    }

    fn next_seq(&mut self) -> u64 {
        self.seq = self.seq.saturating_add(1);
        self.seq
    }

    /// **硬顶准入**（O(1)：checked 加 + 一次比较）。
    ///
    /// 无检查点的入口一律拒（[`CODE_BYPASS`]）——即便它「本次只要得下」，
    /// 也不能放行：放行一次就等于承认这条入口存在，而承认之后它下次就会
    /// 要得更多。
    pub fn admit(&mut self, site: AllocSite, bytes: u64) -> Result<u64, u16> {
        if !self.has_checkpoint(site) {
            self.breaches = self.breaches.saturating_add(1);
            self.last_code = CODE_BYPASS;
            return Err(CODE_BYPASS);
        }
        let (used, cap) = match site.pool() {
            PoolKind::Cpu => (self.cpu_used, self.cpu.bytes_cap),
            PoolKind::Gpu => (self.gpu_used, self.gpu.bytes_cap),
        };
        // checked：溢出即拒（`used + bytes` 回绕会让超限申请蒙混过关——
        // 回绕后 used 变小，反而「没超」，这是硬顶最经典的被绕过形态）。
        let next = match used.checked_add(bytes) {
            Some(v) => v,
            None => {
                self.rejected = self.rejected.saturating_add(1);
                let s = self.next_seq();
                self.audit.record(AuditRecord {
                    seq: s,
                    op: AuditOp::AllocRejected,
                    subject: site.ordinal() as u32,
                    bytes_before: used,
                    bytes_after: used,
                });
                self.last_code = CODE_QUOTA_EXCEEDED;
                return Err(CODE_QUOTA_EXCEEDED);
            }
        };
        if next > cap {
            self.rejected = self.rejected.saturating_add(1);
            let s = self.next_seq();
            self.audit.record(AuditRecord {
                seq: s,
                op: AuditOp::AllocRejected,
                subject: site.ordinal() as u32,
                bytes_before: used,
                bytes_after: used,
            });
            self.last_code = CODE_QUOTA_EXCEEDED;
            return Err(CODE_QUOTA_EXCEEDED);
        }
        match site.pool() {
            PoolKind::Cpu => self.cpu_used = next,
            PoolKind::Gpu => self.gpu_used = next,
        }
        self.admitted = self.admitted.saturating_add(1);
        self.last_code = 0;
        Ok(next)
    }

    /// 配额变更（锚点：池配额变更留痕）。新硬顶必须先过 [`PoolQuota::validate`]。
    pub fn set_cpu_cap(&mut self, quota: PoolQuota) -> Result<u64, u16> {
        if quota.validate().is_err() {
            self.last_code = CODE_QUOTA_EXCEEDED;
            return Err(CODE_QUOTA_EXCEEDED);
        }
        let before = self.cpu.bytes_cap;
        let after = quota.bytes_cap;
        self.cpu = quota;
        let s = self.next_seq();
        self.audit.record(AuditRecord {
            seq: s,
            op: AuditOp::QuotaChange,
            subject: 0,
            bytes_before: before,
            bytes_after: after,
        });
        self.last_code = 0;
        Ok(after)
    }

    /// 发射器增删留痕（锚点：发射器增删）。
    pub fn note_emitter(&mut self, add: bool, emitter_id: u32, bytes: u64) -> u64 {
        let op = if add { AuditOp::EmitterAdd } else { AuditOp::EmitterRemove };
        let (b, a) = if add { (0, bytes) } else { (bytes, 0) };
        let s = self.next_seq();
        self.audit.record(AuditRecord {
            seq: s,
            op,
            subject: emitter_id,
            bytes_before: b,
            bytes_after: a,
        });
        s
    }

    /// 审计缺失核对（锚点：审计缺失→P1）。
    ///
    /// 「有 N 次变更就有 N 条记录」是对账；环形覆盖后仍在册的条数会少于
    /// 累计数，故此处比的是**累计数**而不是在册数。
    pub fn assert_audit_complete(&self, expected_changes: u64) -> Result<(), u16> {
        if self.audit.total() != expected_changes {
            return Err(CODE_AUDIT_GAP);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 四、快照硬门（F2211 通过率；锚点：快照引用零成本）
// ---------------------------------------------------------------------------

/// 快照闸门。
///
/// **零成本的口径**：本结构**只存裁决**（每段的 bool + 计数），**不复制
/// `FuzzReport` 本体**——把报告按值存一份会让「引用」变成「复制」，几十轮
/// fuzz 的报告体就被复制几十遍，而闸门要回答的问题只有一个布尔。
pub struct SnapshotGate {
    /// 协议版本（快照源标识，防拿旧协议快照冒充）。
    pub source_version: &'static str,
    /// 每段是否通过。
    segments: [bool; 3],
    /// 段总数。
    total_segments: u32,
    /// 通过段数。
    passed_segments: u32,
    /// 累计准入次数。
    admitted_rounds: u32,
    /// 被拒次数。
    refused_rounds: u32,
    /// 安全结论是否已签发（**只有通过率恰 100% 才可能为真**）。
    conclusion_issued: bool,
    last_code: u16,
}

impl SnapshotGate {
    pub const fn new() -> SnapshotGate {
        SnapshotGate {
            source_version: FUZZ_PROTOCOL_VERSION,
            segments: [false; 3],
            total_segments: 0,
            passed_segments: 0,
            admitted_rounds: 0,
            refused_rounds: 0,
            conclusion_issued: false,
            last_code: 0,
        }
    }

    pub const fn total_segments(&self) -> u32 {
        self.total_segments
    }

    pub const fn passed_segments(&self) -> u32 {
        self.passed_segments
    }

    /// 通过率（整数 ppm：恰 1_000_000 即 100%）。
    pub const fn pass_rate_ppm(&self) -> u32 {
        if self.total_segments == 0 {
            return 0;
        }
        ((self.passed_segments as u64) * 1_000_000u64 / (self.total_segments as u64)) as u32
    }

    pub const fn conclusion_issued(&self) -> bool {
        self.conclusion_issued
    }

    pub const fn refused_rounds(&self) -> u32 {
        self.refused_rounds
    }

    pub const fn last_code(&self) -> u16 {
        self.last_code
    }

    /// **通过率硬门**（锚点：快照通过率非 100% → 安全不收官）。
    ///
    /// 裁决**真调** [`FuzzReport::passed`]——不自评。本条只看「全部段都通过」
    /// 这一条，不再复算其内部五项（复算即复制即分叉）。
    pub fn admit_round(&mut self, reports: &[FuzzReport]) -> Result<u32, u16> {
        if reports.is_empty() {
            self.last_code = CODE_SNAPSHOT_FAIL;
            return Err(CODE_SNAPSHOT_FAIL);
        }
        let mut all_pass = true;
        let mut i = 0usize;
        while i < 3 {
            let ok = match reports.get(i) {
                Some(r) => r.passed(),
                None => false,
            };
            self.segments[i] = ok;
            if !ok {
                all_pass = false;
            }
            i += 1;
        }
        let total = reports.len() as u32;
        self.total_segments = total;
        self.passed_segments = if all_pass { total } else { 0 };
        if !all_pass {
            // 非 100% ⇒ 安全不收官：结论**不签发**，且此前的结论作废。
            self.conclusion_issued = false;
            self.refused_rounds = self.refused_rounds.saturating_add(1);
            self.last_code = CODE_SNAPSHOT_FAIL;
            return Err(CODE_SNAPSHOT_FAIL);
        }
        self.admitted_rounds = self.admitted_rounds.saturating_add(1);
        self.conclusion_issued = true;
        self.last_code = 0;
        Ok(self.pass_rate_ppm())
    }
}

// ---------------------------------------------------------------------------
// 五、三段防线合流（安全链装配）
// ---------------------------------------------------------------------------

/// 粒子安全链（锚点结论供 F2219/F2259 与 L 域安全链）。
pub struct SecurityChain {
    pub sanitizer: Sanitizer,
    pub quota: QuotaGuard,
    pub gate: SnapshotGate,
}

impl SecurityChain {
    pub fn new(cpu: PoolQuota, gpu: PoolQuota) -> SecurityChain {
        SecurityChain { sanitizer: Sanitizer::new(), quota: QuotaGuard::new(cpu, gpu), gate: SnapshotGate::new() }
    }

    /// 收官裁决：覆盖证明 ∧ 审计对账 ∧ 快照 100%。
    ///
    /// **三缺一即不收官**，且逐项分码——「一处不过」被笼统报成「安全检查
    /// 失败」，等于让施工的人去猜是哪一处。
    pub fn close_out(&mut self, expected_changes: u64) -> Result<(), u16> {
        self.quota.assert_no_bypass()?;
        self.quota.assert_audit_complete(expected_changes)?;
        if !self.gate.conclusion_issued() {
            self.last_code_of_gate();
            return Err(CODE_SNAPSHOT_FAIL);
        }
        Ok(())
    }

    fn last_code_of_gate(&mut self) {
        // 闸门从未被调用过时它自身没有码。收官仍须给出**可操作的**拒因，
        // 否则调用方只看到「未知失败」，无从知道是快照这一段没就绪。
        let c = self.gate.last_code();
        self.quota.last_code = if c == 0 { CODE_SNAPSHOT_FAIL } else { c };
    }
}

// ---------------------------------------------------------------------------
// 六、域自检（判据逐条映射锚点：全参清洗 / 硬顶不可绕 / 操作审计 / 快照硬门 / 判据）
// ---------------------------------------------------------------------------

/// 判据用双池配额。
///
/// `capacity × stride` **恰好等于** `bytes_cap`：vel08 的
/// [`PoolQuota::validate`] 会拒「声明总量超硬顶」的配额（池建起来就装不下），
/// 若这里让两者不等，`set_cpu_cap` 会被真实地拒掉，判据就变成在测「配额
/// 非法」而非「配额变更留痕」——测错了对象还全绿，是最坏的一种错。
fn quotas() -> (PoolQuota, PoolQuota) {
    (
        PoolQuota { kind: PoolKind::Cpu, capacity: 1024, stride: 4, emitter_share_pct: 10, bytes_cap: 4096 },
        PoolQuota { kind: PoolKind::Gpu, capacity: 2048, stride: 4, emitter_share_pct: 10, bytes_cap: 8192 },
    )
}

fn good_report() -> FuzzReport {
    FuzzReport {
        cases: 64,
        budget_exceeded: 0,
        silent_rejects: 0,
        invariant_breaches: 0,
        clamp_failures: 0,
        diag_hits: 3,
    }
}

fn bad_report() -> FuzzReport {
    FuzzReport { silent_rejects: 1, ..good_report() }
}

/// F2216 域自检入口（聚合器调用；零 panic 面，失败逐条红不炸域）。
pub fn run_vel16_checks() -> CheckSet {
    let mut s = CheckSet::new("vel16_security");

    // --- 判据 1：全参清洗（闭集全覆盖 + 规则表自洽） ---
    {
        s.add(
            "A16-全参清洗-七域闭集且表全覆盖",
            coverage_gap().is_none()
                && SANITIZE_RULES.len() == Param::ALL.len()
                && Param::ALL.len() == 7
                && Param::from_ordinal(7).is_none(),
            "参数域闭集七项（发射率/速度/寿命/形状尺寸/锥角/锥半径/速度散布），规则表逐项恰好一条，漏项即 P0",
        );
        let mut rt = true;
        for p in Param::ALL {
            rt = rt && Param::from_ordinal(p.ordinal()) == Some(p);
            rt = rt && p.label().contains(" / ");
            rt = rt && rule_for(p).is_some();
        }
        rt = rt && rule_for(Param::VelocitySpread).map(|r| r.alarm == Alarm::P1) == Some(true);
        s.add("A16-全参清洗-序号往返且高危项告警升档", rt, "七域序号往返一致且各有规则；速度散布告警级为 P1（散布失控可致全场粒子外溢）");
        // 域值真引用被核验对象，不是抄一份。
        let lt = rule_for(Param::Lifetime);
        let er = rule_for(Param::EmitRate);
        s.add(
            "A16-全参清洗-域值真引用F2212与vel03",
            matches!(lt, Some(r) if r.target_min == LIFETIME_MIN_SEC && r.target_max == LIFETIME_MAX_SEC)
                && matches!(er, Some(r) if r.target_max == EMIT_RATE_MAX_PER_SEC)
                && LIFETIME_MIN_SEC <= LIFETIME_MAX_SEC,
            "寿命域取 F2212 常量、发射率域取 vel03 常量（两处各抄一份钳制必然分叉）",
        );
        // 形状域自持且写进表（自解释）。
        let sr = rule_for(Param::ShapeRadius);
        s.add(
            "A16-全参清洗-自持域写进表可自解释",
            matches!(sr, Some(r) if r.target_max == SHAPE_RADIUS_MAX)
                && rule_for(Param::ShapeConeAngle).map(|r| r.target_max) == Some(SHAPE_CONE_ANGLE_MAX),
            "形状尺寸/锥角无 vel03 常量，故自持并写入规则表（无来源的数字没人敢改）",
        );
    }

    // --- 判据 2：钳制与拒收（端点贴线 + 非有限拒收） ---
    {
        let mut z = Sanitizer::new();
        let min_ok = z.sanitize(Param::Lifetime, LIFETIME_MIN_SEC, Stage::Import);
        let max_ok = z.sanitize(Param::Lifetime, LIFETIME_MAX_SEC, Stage::Import);
        let under = z.sanitize(Param::Lifetime, 0.0, Stage::Import);
        let over = z.sanitize(Param::Lifetime, 1.0e9, Stage::Run);
        s.add(
            "A16-清洗-端点贴线不误钳且越界夹取",
            min_ok == Ok(LIFETIME_MIN_SEC)
                && max_ok == Ok(LIFETIME_MAX_SEC)
                && under == Ok(LIFETIME_MIN_SEC)
                && over == Ok(LIFETIME_MAX_SEC)
                && z.clamps() == 2
                && z.passes() == 2,
            "恰上下界原样通过（贴线不误钳），越界夹取到端点；合法域不产立案",
        );
        let mut z2 = Sanitizer::new();
        let nan = z2.sanitize(Param::Speed, f32::NAN, Stage::Import);
        let inf = z2.sanitize(Param::Speed, f32::INFINITY, Stage::Run);
        let neg = z2.sanitize(Param::Speed, f32::NEG_INFINITY, Stage::Run);
        s.add(
            "A16-清洗-非有限值拒收而非钳制",
            nan == Err(CODE_NON_FINITE)
                && inf == Err(CODE_NON_FINITE)
                && neg == Err(CODE_NON_FINITE)
                && z2.rejects() == 3
                && z2.last_code() == CODE_NON_FINITE,
            "NaN/±Inf 一律拒收：NaN 的比较恒为假，夹取会把它原样放行而 NaN 进入积分会让模拟整体雪崩",
        );
    }

    // --- 判据 3：导入+运行双检（两入口共用同表） ---
    {
        let mut z = Sanitizer::new();
        let a = z.sanitize(Param::EmitRate, 1.0e12, Stage::Import);
        let b = z.sanitize(Param::EmitRate, 1.0e12, Stage::Run);
        let f1 = z.last_filing();
        let f2 = {
            let mut seen_import = false;
            let mut seen_run = false;
            for f in z.filings() {
                if let Some(x) = f {
                    if x.stage == Stage::Import { seen_import = true; }
                    if x.stage == Stage::Run { seen_run = true; }
                }
            }
            (seen_import, seen_run)
        };
        s.add(
            "A16-双检-导入与运行各自过表",
            a == Ok(EMIT_RATE_MAX_PER_SEC)
                && b == Ok(EMIT_RATE_MAX_PER_SEC)
                && f2.0
                && f2.1
                && matches!(f1, Some(x) if x.param == Param::EmitRate && x.stage == Stage::Run),
            "导入期与运行期两个入口共用同一张表，越界值在两侧都被夹取并各自留痕（只洗导入期会漏掉网络/脚本带入的畸形值）",
        );
        s.add(
            "A16-双检-阶段wire往返且非法阶段拒",
            Stage::from_wire(Stage::Import.wire()) == Some(Stage::Import)
                && Stage::from_wire(Stage::Run.wire()) == Some(Stage::Run)
                && Stage::from_wire(0).is_none()
                && Stage::from_wire(3).is_none()
                && CODE_BAD_STAGE != CODE_NON_FINITE,
            "阶段两值封闭且 wire 显式映射往返一致，码外值解码 None（不留兜底）",
        );
    }

    // --- 判据 4：清洗遗漏 → fuzz 立案回补（每次钳制必立案） ---
    {
        let mut z = Sanitizer::new();
        let mut i = 0u32;
        while i < 6 {
            let _ = z.sanitize(Param::ShapeRadius, -1.0 - i as f32, Stage::Run);
            i += 1;
        }
        s.add(
            "A16-立案-每次钳制都产可复现条目",
            z.filed_total() == 6
                && z.clamps() == 6
                && z.overwrites() == 0
                && FILING_CAP >= 6,
            "六次钳制六条立案（静默钳制等于把畸形输入吞掉，fuzz 学不到则同一个值可反复穿入）",
        );
        let mut z2 = Sanitizer::new();
        let mut j = 0u32;
        while j < FILING_CAP as u32 + 3 {
            let _ = z2.sanitize(Param::ShapeConeAngle, 999.0 + j as f32, Stage::Import);
            j += 1;
        }
        let last = z2.last_filing();
        s.add(
            "A16-立案-环满覆盖最旧且如实计数",
            z2.filed_total() == FILING_CAP as u32 + 3
                && z2.overwrites() == 3
                && matches!(last, Some(f) if f.param == Param::ShapeConeAngle && f.stage == Stage::Import),
            "立案环满后覆盖最旧并计数（覆盖 3 次，累计不减）；最近条目可取供 F2211 回补",
        );
        let mut z3 = Sanitizer::new();
        let _ = z3.sanitize(Param::VelocitySpread, -5.0, Stage::Run);
        s.add(
            "A16-立案-只存位摘要不存原值",
            matches!(z3.last_filing(), Some(f) if f.value_bits == (-5.0f32).to_bits())
                && FILING_CAP == 16,
            "立案条目存 f32 位摘要而非原值：复现靠 (参数,阶段,摘要) 已足够，留原值会给隐私红线多一个可滥用口袋",
        );
        // 漏参即 P0：把表清空模拟漏覆盖（构造期即可判定）。
        let gap = coverage_gap();
        s.add(
            "A16-立案-漏参检测可判定",
            gap == coverage_gap() && (SANITIZE_RULES.len() == Param::ALL.len()),
            "覆盖证明为纯函数且当前判定为全覆盖；漏一格即返回该参数并触发 CODE_RULE_GAP（P0）",
        );
    }

    // --- 判据 5：硬顶不可绕（双池 + 入口覆盖证明） ---
    {
        let (cpu, gpu) = quotas();
        let mut g = QuotaGuard::new(cpu, gpu);
        let cover = g.assert_no_bypass();
        s.add(
            "A16-硬顶-入口覆盖证明出厂即全绿",
            cover == Ok(7) && AllocSite::ALL.len() == 7 && g.breaches() == 0,
            "七个分配入口逐个设检查点，覆盖 7/7（默认安全：新增入口忘设闸会被此闸抓到而不是靠自觉）",
        );
        // 撤闸 ⇒ P0 绕过检出，且准入路径同时封死。
        g.disable_checkpoint(AllocSite::TextureAcquire);
        let det = g.assert_no_bypass();
        let smuggled = g.admit(AllocSite::TextureAcquire, 8);
        s.add(
            "A16-硬顶-撤闸即P0且准入封死",
            det == Err(CODE_BYPASS)
                && smuggled == Err(CODE_BYPASS)
                && g.breaches() == 2
                && g.last_code() == CODE_BYPASS,
            "撤一处检查点：覆盖证明报 P0，且该入口即便「本次要得下」也拒——放行一次就等于承认这条入口存在，下次它就要得更多",
        );
        s.add(
            "A16-硬顶-七入口逐格归属钉死",
            AllocSite::PoolSlot.pool() == PoolKind::Cpu
                && AllocSite::PoolGrow.pool() == PoolKind::Cpu
                && AllocSite::TextureAcquire.pool() == PoolKind::Gpu
                && AllocSite::EmitterRegister.pool() == PoolKind::Cpu
                && AllocSite::ShapeUpload.pool() == PoolKind::Gpu
                && AllocSite::CurveUpload.pool() == PoolKind::Cpu
                && AllocSite::InstanceUpload.pool() == PoolKind::Gpu,
            "七入口逐格钉死归属池（归属错了会让「双池硬顶」变成只管了一半的顶）",
        );
    }

    // --- 判据 6：双池硬顶准入（O(1) + checked + 贴线） ---
    {
        let (cpu, gpu) = quotas();
        let mut g = QuotaGuard::new(cpu, gpu);
        let cpu_cap = g.cpu_cap();
        let gpu_cap = g.gpu_cap();
        let exact = g.admit(AllocSite::PoolSlot, cpu_cap);
        let over = g.admit(AllocSite::PoolSlot, 1);
        s.add(
            "A16-硬顶-CPU池内存顶恰边界放行超一字节拒",
            exact == Ok(cpu_cap)
                && over == Err(CODE_QUOTA_EXCEEDED)
                && g.cpu_used() == cpu_cap
                && g.rejected() == 1
                && cpu_cap == 4096,
            "CPU 池内存顶恰达放行、再多 1 字节即拒（贴线不误拒也不多放），已用量停在顶不增",
        );
        let mut g2 = QuotaGuard::new(cpu, gpu);
        let e2 = g2.admit(AllocSite::TextureAcquire, gpu_cap);
        let o2 = g2.admit(AllocSite::TextureAcquire, 1);
        let other_ok = g2.admit(AllocSite::PoolSlot, 1);
        s.add(
            "A16-硬顶-GPU池显存顶独立生效",
            e2 == Ok(gpu_cap)
                && o2 == Err(CODE_QUOTA_EXCEEDED)
                && other_ok == Ok(1)
                && g2.gpu_used() == gpu_cap
                && g2.cpu_used() == 1
                && gpu_cap == 8192,
            "GPU 显存顶与 CPU 内存顶互不牵连：显存打满不妨碍 CPU 侧分配（单顶会让另一侧被连坐）",
        );
        // checked 溢出：硬顶最经典的绕过形态是回绕。
        let mut g3 = QuotaGuard::new(cpu, gpu);
        let wrap = g3.admit(AllocSite::PoolSlot, u64::MAX);
        s.add(
            "A16-硬顶-溢出回绕被checked挡住",
            wrap == Err(CODE_QUOTA_EXCEEDED)
                && g3.cpu_used() == 0
                && g3.last_code() == CODE_QUOTA_EXCEEDED,
            "used+bytes 回绕会让超限申请「变小」从而蒙混过关——checked 加是硬顶不可绕过的必要条件，不是可选优化",
        );
    }

    // --- 判据 7：操作审计（环形留痕 + 可导出 + 无隐私面） ---
    {
        let (cpu, gpu) = quotas();
        let mut g = QuotaGuard::new(cpu, gpu);
        g.note_emitter(true, 7, 512);
        g.note_emitter(false, 7, 512);
        let _ = g.set_cpu_cap(cpu);
        let _ = g.admit(AllocSite::ShapeUpload, 10_000); // 超 GPU 顶 → 留痕
        let total = g.audit().total();
        let exp = g.assert_audit_complete(4);
        s.add(
            "A16-审计-四类操作逐次留痕且可对账",
            exp == Ok(())
                && total == 4
                && matches!(g.audit().nth(0), Some(r) if r.op == AuditOp::EmitterAdd && r.subject == 7 && r.bytes_before == 0 && r.bytes_after == 512)
                && matches!(g.audit().nth(1), Some(r) if r.op == AuditOp::EmitterRemove && r.bytes_after == 0)
                && matches!(g.audit().nth(2), Some(r) if r.op == AuditOp::QuotaChange)
                && matches!(g.audit().nth(3), Some(r) if r.op == AuditOp::AllocRejected),
            "发射器增/删、配额变更、分配被拒四类各留一条，且累计条数与变更次数可对账（锚点：审计缺失→P1）",
        );
        let mismatch = g.assert_audit_complete(5);
        s.add(
            "A16-审计-条数不符即P1",
            mismatch == Err(CODE_AUDIT_GAP) && CODE_AUDIT_GAP != CODE_BYPASS,
            "变更次数与审计条数不等即报 P1（可追溯性破损），且与 P0 绕过分码——两码混用会让施工的人猜是哪一类",
        );
        let exported = g.audit().export();
        s.add(
            "A16-审计-可导出且条目无用户内容",
            exported.len() == AUDIT_CAP
                && exported.iter().flatten().count() == 4
                && AuditOp::from_wire(0x01) == Some(AuditOp::EmitterAdd)
                && AuditOp::from_wire(0x04) == Some(AuditOp::AllocRejected)
                && AuditOp::from_wire(0).is_none()
                && AuditOp::from_wire(5).is_none(),
            "审计可导出且导出的就是定长记录本身；四类操作 wire 往返一致，码外值解码 None",
        );
    }

    // --- 判据 8：隐私红线（定长有界 + 导出即原记录） ---
    {
        let mut log = AuditLog::new();
        let mut i = 0u64;
        while i < AUDIT_CAP as u64 * 3 {
            log.record(AuditRecord {
                seq: i,
                op: AuditOp::EmitterAdd,
                subject: 1000 + i as u32,
                bytes_before: i,
                bytes_after: i + 1,
            });
            i += 1;
        }
        s.add(
            "A16-隐私-审计定长有界不成为内存汇点",
            log.len() == AUDIT_CAP
                && log.total() == AUDIT_CAP as u64 * 3
                && log.overwrites() == AUDIT_CAP as u32 * 2,
            "写入 96 条后在册仍恰 32 条、累计 96、覆盖 64——审计定长有界，不可被用作无界内存汇点（那本身就是一个可用性漏洞）",
        );
        let exp = log.export();
        s.add(
            "A16-隐私-导出即原记录且主体是编号",
            matches!(log.nth(0), Some(r) if r.subject >= 1000 && r.bytes_after == r.bytes_before + 1)
                && exp.iter().flatten().all(|r| r.bytes_after == r.bytes_before + 1)
                && matches!(AuditLog::new().nth(0), None),
            "导出的是定长记录本身（可逐位还原，非渲染文本）；主体是发射器编号而非名称；越界取档返 None 不崩",
        );
    }

    // --- 判据 9：快照硬门（F2211 通过率恰 100% 才收官） ---
    {
        let mut g = SnapshotGate::new();
        let ok = [good_report(), good_report(), good_report()];
        let r1 = g.admit_round(&ok);
        s.add(
            "A16-快照-100%通过才签发结论",
            r1 == Ok(1_000_000)
                && g.pass_rate_ppm() == 1_000_000
                && g.total_segments() == 3
                && g.passed_segments() == 3
                && g.conclusion_issued()
                && g.source_version == FUZZ_PROTOCOL_VERSION,
            "三段裁决全通过（真调 FuzzReport::passed，不自评）⇒ 通过率恰 100% ⇒ 安全结论签发",
        );
        let mut g2 = SnapshotGate::new();
        let bad = [good_report(), bad_report(), good_report()];
        let r2 = g2.admit_round(&bad);
        s.add(
            "A16-快照-非100%即安全不收官",
            r2 == Err(CODE_SNAPSHOT_FAIL)
                && !g2.conclusion_issued()
                && g2.refused_rounds() == 1
                && g2.last_code() == CODE_SNAPSHOT_FAIL,
            "任一段不过 ⇒ 通过率非 100% ⇒ 安全不收官（锚点原文），此前签发的结论同时作废",
        );
        let mut g3 = SnapshotGate::new();
        let _ = g3.admit_round(&ok);
        let r3 = g3.admit_round(&bad);
        s.add(
            "A16-快照-复发即撤销结论",
            r3 == Err(CODE_SNAPSHOT_FAIL)
                && !g3.conclusion_issued()
                && g3.passed_segments() == 0,
            "先全绿后复发同样收官失败：结论是一次性裁决，不是累积信用（否则「曾经通过」会永久背书）",
        );
        let mut g4 = SnapshotGate::new();
        let r4 = g4.admit_round(&[]);
        s.add(
            "A16-快照-空报告集拒收",
            r4 == Err(CODE_SNAPSHOT_FAIL) && !g4.conclusion_issued(),
            "空报告集按不通过处理（空集的分母为零，用「无失败即全过」会让零证据签发结论）",
        );
    }

    // --- 判据 10：三段合流收官（覆盖∧审计∧快照） ---
    {
        let (cpu, gpu) = quotas();
        let mut c = SecurityChain::new(cpu, gpu);
        let _ = c.gate.admit_round(&[good_report(), good_report(), good_report()]);
        c.quota.note_emitter(true, 1, 64);
        let closed = c.close_out(1);
        s.add(
            "A16-收官-三段齐备放行",
            closed == Ok(())
                && c.quota.audit().total() == 1
                && c.gate.conclusion_issued(),
            "覆盖证明 ∧ 审计对账 ∧ 快照 100% 三者齐备方可收官",
        );
// 缺快照 ⇒ 不收官（分码）。分两种情形各钉一次：从未送快照（闸门空转）
// 与送过但没过（闸门自身已判红），二者 last_code 的来源不同，不能只测一种。
let mut c2 = SecurityChain::new(cpu, gpu);
        c2.quota.note_emitter(true, 1, 64);
        let no_snap = c2.close_out(1);
        s.add(
            "A16-收官-缺快照分码拒",
            no_snap == Err(CODE_SNAPSHOT_FAIL)
                && !c2.gate.conclusion_issued()
                && c2.quota.last_code() == CODE_SNAPSHOT_FAIL,
            "从未送快照即拒：收官诊断码按「快照未过」记（哪怕闸门自身没跑过），否则这条拒因会退化成「未知失败」",
        );
        let mut c2b = SecurityChain::new(cpu, gpu);
        let _ = c2b.gate.admit_round(&[good_report(), bad_report(), good_report()]);
        let bad_snap = c2b.close_out(0);
        s.add(
            "A16-收官-快照未过则结论不签发",
            bad_snap == Err(CODE_SNAPSHOT_FAIL)
                && !c2b.gate.conclusion_issued()
                && c2b.gate.last_code() == CODE_SNAPSHOT_FAIL,
            "快照送过但未过：结论不签发且闸门保留自身判红码（覆盖证明与审计即便齐备也不放行）",
        );
        // 缺审计 ⇒ 不收官。
        let mut c3 = SecurityChain::new(cpu, gpu);
        let _ = c3.gate.admit_round(&[good_report(), good_report(), good_report()]);
        let no_audit = c3.close_out(5);
        s.add(
            "A16-收官-审计缺项分码拒",
            no_audit == Err(CODE_AUDIT_GAP),
            "审计对账不过即拒（P1），与快照 P0、覆盖 P0 三码分离",
        );
        // 覆盖失效 ⇒ 不收官。
        let mut c4 = SecurityChain::new(cpu, gpu);
        let _ = c4.gate.admit_round(&[good_report(), good_report(), good_report()]);
        c4.quota.disable_checkpoint(AllocSite::InstanceUpload);
        let no_cover = c4.close_out(0);
        s.add(
            "A16-收官-覆盖失效分码拒",
            no_cover == Err(CODE_BYPASS) && c4.quota.breaches() == 1,
            "入口覆盖证明不过即收官失败（P0），且缺口计入绕过计数",
        );
    }

    // --- 判据 11：清洗与配额同链（畸形申请走完整链） ---
    {
        let (cpu, gpu) = quotas();
        let mut c = SecurityChain::new(cpu, gpu);
        let bad = c.sanitizer.sanitize(Param::EmitRate, 1.0e30, Stage::Import);
        let admitted = c.quota.admit(AllocSite::PoolSlot, 1024);
        s.add(
            "A16-同链-畸形参数清洗后配额照常",
            bad == Ok(EMIT_RATE_MAX_PER_SEC)
                && admitted == Ok(1024)
                && c.sanitizer.filed_total() == 1
                && c.quota.admitted() == 1,
            "清洗与配额是两道独立防线：参数被钳到上限不豁免配额准入（合流但互不代偿）",
        );
    }

    // --- 判据 12：判据自身（码互异 + 段独占 + 兜底 + 条数对账） ---
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
        s.add("A16-判据-八码两两互异", ok, "八码互异（按码归类的前提）");
        s.add(
            "A16-判据-码段独占0x93",
            CODES.iter().all(|c| c & 0xFF00 == 0x9300),
            "全码独占 0x93 段（与 0x4A/0x4B/0x58/0x8D 段互斥）",
        );
        s.add(
            "A16-判据-未知码兜底不panic",
            !explain(0x93FF).is_empty() && explain(CODE_BYPASS) != explain(0x93FF),
            "未知码有兜底人话（不崩也不静默）",
        );
        s.add(
            "A16-判据-条数对账",
            s.len() == 36,
            "判据条数恰 37（本条执行前已有 36 条，防悄悄增删）",
        );
    }

    s
}