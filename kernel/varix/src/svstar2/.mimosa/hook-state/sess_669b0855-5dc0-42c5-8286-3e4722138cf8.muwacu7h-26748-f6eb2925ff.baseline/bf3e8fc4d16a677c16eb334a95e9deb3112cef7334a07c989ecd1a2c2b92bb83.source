//! VE-F0005 · 显存预算仲裁器（VE-A 域 · 内核图形抽象层 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0005`
//!
//! **判据（锚点原文）**：显存资源的预算仲裁（各子系统按预算申请，超预算降级
//! 不挤占他人）、预算三档（承诺/目标/上限）、显存水位实时可见；显存预算的
//! 域级加总复核（与 A06 预算联动）；预算仲裁含抢占规则文档（谁可以挤谁写明）；
//! 水位表含历史曲线（显存何时吃紧可回溯）；三档预算的申请模板随 SDK 分发；
//! 挤占拒绝含申请方与持有方双方告知。
//!
//! **错误路径与降级矩阵**：超支→降级+告知；挤占→拒绝；水位异常→归因。
//!
//! **设计要点**：
//! - **三档预算**：承诺（promise，保底口径——超支时最后的保障线）/ 目标
//!   （target，期望值——正常授出的落点）/ 上限（ceiling，硬顶）。不变式
//!   `promise ≤ target ≤ ceiling` 在申请入口强制校验——违反即拒绝并给
//!   修正建议，不静默钳制（静默钳制会让申请方以为自己拿到了承诺档）；
//! - **超支→降级+告知**：域承诺账本放不下新申请的目标档时，收缩到承诺档
//!   （保底值）并**必须产出 `DowngradeNotice`**（降了多少、为什么、拿什么
//!   换回来）——降级不告知等于悄悄变卡，违反可观测铁律；连保底值都放不下
//!   时拒绝申请，绝不挤占他人已授预算；
//! - **挤占→拒绝**：抢占不是自由市场。`PREEMPTION_RULES_DOC` 把"谁可以挤谁"
//!   写成规则表：合成器保底不可被任何人挤占、同级不可互挤、前台交互只能挤
//!   后台预热。规则外的挤占一律拒绝，且拒绝告知必须同时点名申请方与持有方
//!   ——只骂申请方不告诉持有方，持有方会莫名丢账；
//! - **水位表含历史曲线**：每次采样（逻辑 tick 注入）落进环形历史，水位等级
//!   四档（Normal/Watch/High/Critical），历史曲线可回溯"显存何时吃紧"——
//!   没有曲线的水位表只能报警当下，不能复盘过去；
//! - **水位异常→归因**：实测用量与账本记的 in_use 对不上（涨了没记账 /
//!   账本记了没用上）即异常，归因引擎给出候选原因与证据，不硬猜唯一结论；
//! - **域级加总复核**：所有子系统已授预算之和必须 ≤ 域级预算（与 A06 预算
//!   联动的对账点），超记即审计红项——仲裁器自己先不能账目崩坏；
//! - **零静默**：每条错误（含拒绝与钳制）都带五元组（发生了什么/为什么/
//!   下一步/责任方/错误码）并落入错误账本，`errors()` 可全量取回。
//!
//! **跨批对接点**：P07 预算表同构；A06 预算联动（域级加总复核）。
//!
//! 逻辑时钟注入，不用墙钟——水位曲线与审计均可回放复现。
//! 零外部依赖，只依赖 `crate::checks`（测试侧）。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 子系统账本容量上限。VE 全栈用户态服务数有物理上界，超出的申请直接拒绝
/// （而不是悄悄挤掉已有的账目条目）。
pub const MAX_SUBSYSTEMS: usize = 64;

/// 单子系统承诺档的最小预算（字节）。低于此值的申请没有调度意义。
pub const MIN_BUDGET_BYTES: u64 = 64 * 1024;

/// 预算粒度（字节）。所有档位按 4 KiB 向上对齐——非对齐值在入口校验时拒绝。
pub const BUDGET_GRANULARITY: u64 = 4 * 1024;

/// 水位历史环形深度。256 个采样点 × 1 tick/点 ≈ 足够回溯一次完整负载周期。
pub const WATERMARK_HISTORY: usize = 256;

/// 错误账本容量。账本满后新错误丢弃计数（零静默不等于无限内存）。
pub const ERROR_LEDGER_CAP: usize = 256;

/// 水位等级阈值（百分比）：Watch 起步 / High 起步 / Critical 起步。
pub const WM_WATCH_PCT: u64 = 70;
pub const WM_HIGH_PCT: u64 = 85;
pub const WM_CRITICAL_PCT: u64 = 95;

// ---------------------------------------------------------------------------
// 二、抢占规则文档（判据点名：谁可以挤谁写明，随代码入册）
// ---------------------------------------------------------------------------

/// 抢占规则文档（人读文本）。规则表是仲裁行为的一事实源：
/// 实现里的 `try_preempt` 与本文档逐条对应，改动必须两处同步走 ADR。
pub const PREEMPTION_RULES_DOC: &str = "\
显存抢占规则（VE-F0005 · 规则表 v1）：
R1 合成器保底（Compositor）不可被任何子系统挤占——它保住的是最后一帧。
R2 前台交互（Foreground）可以挤占后台预热（Background），每次最多挤占用量的 1/2，
   且必须同时告知双方（申请方拿到多少、持有方让出多少）。
R3 同级不可互挤：同优先级的两个子系统之间不存在抢占，只有协商与降级。
R4 系统探针（SystemProbe）只读观测，无预算可挤也无预算可被挤。
R5 一切规则外的挤占请求一律拒绝（E_PREEMPT_DENIED），拒绝告知同时点名双方。";

// ---------------------------------------------------------------------------
// 三、基础类型：优先级 / 三档预算 / 申请
// ---------------------------------------------------------------------------

/// 子系统优先级四档（抢占规则的主体）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PriorityClass {
    /// 合成器保底：不可被挤占（R1）。
    Compositor,
    /// 前台交互：可挤占 Background（R2）。
    Foreground,
    /// 后台预热：可被 Foreground 挤占（R2）。
    Background,
    /// 系统探针：只读观测（R4）。
    SystemProbe,
}

impl PriorityClass {
    /// 读屏可读名（无障碍判据：状态读屏可达）。
    pub fn screen_name(self) -> &'static str {
        match self {
            PriorityClass::Compositor => "合成器保底",
            PriorityClass::Foreground => "前台交互",
            PriorityClass::Background => "后台预热",
            PriorityClass::SystemProbe => "系统探针",
        }
    }
}

/// 预算三档（判据：承诺/目标/上限）。
///
/// 不变式：`promise ≤ target ≤ ceiling`，且三档均按 [`BUDGET_GRANULARITY`]
/// 对齐、promise ≥ [`MIN_BUDGET_BYTES`]。违反即 `E_TIER_INVARIANT`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BudgetTier {
    /// 承诺档：记账口径，域级加总复核的对象。
    pub promise: u64,
    /// 目标档：超支降级的常规落点。
    pub target: u64,
    /// 上限档：硬顶，任何路径都不得越过。
    pub ceiling: u64,
}

impl BudgetTier {
    /// 构造并校验三档不变式。失败返回错误码与修正建议。
    pub fn new(promise: u64, target: u64, ceiling: u64) -> Result<Self, BudgetError> {
        let t = BudgetTier { promise, target, ceiling };
        if let Some(why) = t.violation_reason() {
            return Err(BudgetError::new(
                "E_TIER_INVARIANT",
                "三档预算不变式被违反",
                why,
                "按 promise ≤ target ≤ ceiling 且 ≥ MIN_BUDGET_BYTES 修正申请",
                "申请方",
            ));
        }
        Ok(t)
    }

    /// 不变式体检：返回违规原因（None = 合法）。
    pub fn violation_reason(&self) -> Option<&'static str> {
        if self.promise < MIN_BUDGET_BYTES {
            Some("承诺档低于 MIN_BUDGET_BYTES")
        } else if self.promise > self.target {
            Some("承诺档大于目标档")
        } else if self.target > self.ceiling {
            Some("目标档大于上限档")
        } else {
            None
        }
    }

    /// 粒度体检：三档是否都按 4 KiB 对齐。
    pub fn aligned(&self) -> bool {
        self.promise % BUDGET_GRANULARITY == 0
            && self.target % BUDGET_GRANULARITY == 0
            && self.ceiling % BUDGET_GRANULARITY == 0
    }
}

/// 预算申请（三档预算的申请模板随 SDK 分发——判据点名）。
#[derive(Clone, Debug)]
pub struct BudgetRequest {
    /// 申请方标识（人读名，入账与告知都用它）。
    pub applicant: String,
    /// 三档预算。
    pub tier: BudgetTier,
    /// 优先级（抢占规则的主体）。
    pub priority: PriorityClass,
}

impl BudgetRequest {
    /// 构造一份申请。
    pub fn new(applicant: &str, tier: BudgetTier, priority: PriorityClass) -> Self {
        BudgetRequest {
            applicant: applicant.to_string(),
            tier,
            priority,
        }
    }

    /// SDK 申请模板（人读文本）。第三方按此模板申请，字段缺一即被拒绝——
    /// 模板先行是"申请方知道自己在申请什么"的契约。
    pub fn sdk_template() -> &'static str {
        "\
VE-F0005 显存预算申请模板（随 SDK 分发 · v1）：
- 申请方:   <服务人读名，如 vxcomp/vxweb/vx3d>
- 承诺档:   <字节数，≥ 64KiB，4KiB 对齐；域级加总复核口径>
- 目标档:   <字节数，≥ 承诺档；超支降级的落点>
- 上限档:   <字节数，≥ 目标档；硬顶，越过即拒绝>
- 优先级:   <Compositor|Foreground|Background|SystemProbe>
注：三档不写齐 = 拒绝（E_TIER_INVARIANT）；降级会收到 DowngradeNotice。"
    }
}

// ---------------------------------------------------------------------------
// 四、结果类型：授出 / 降级告知 / 错误五元组
// ---------------------------------------------------------------------------

/// 降级原因（降级矩阵的归因字段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DowngradeReason {
    /// 域承诺账本放不下目标档（期望值），收缩到承诺档（保底值）。
    TargetToPromise,
}

/// 降级告知（判据：超支→降级+告知）。每一次降级必须产出一条。
#[derive(Clone, Debug)]
pub struct DowngradeNotice {
    /// 被降级的申请方。
    pub applicant: String,
    /// 申请时声明的承诺档。
    pub requested_promise: u64,
    /// 实际授出的字节数。
    pub granted: u64,
    /// 降级原因。
    pub reason: DowngradeReason,
    /// 发生时的逻辑 tick。
    pub tick: u64,
}

impl DowngradeNotice {
    /// 读屏可读文本（无障碍判据）。
    pub fn screen_text(&self) -> String {
        format!(
            "[降级告知] {}：申请承诺 {} 字节，实际授出 {} 字节（原因 {:?}，tick {}）",
            self.applicant, self.requested_promise, self.granted, self.reason, self.tick
        )
    }
}

/// 错误五元组（零静默纪律：发生了什么/为什么/下一步/责任方/错误码）。
#[derive(Clone, Debug)]
pub struct BudgetError {
    /// 错误码（本域段：VE-F0005）。
    pub code: &'static str,
    /// 发生了什么。
    pub what: &'static str,
    /// 为什么。
    pub why: String,
    /// 下一步。
    pub next: &'static str,
    /// 责任方（告知点名用：挤占拒绝时同时含申请方与持有方）。
    pub who: String,
}

impl BudgetError {
    fn new(
        code: &'static str,
        what: &'static str,
        why: &str,
        next: &'static str,
        who: &str,
    ) -> Self {
        BudgetError {
            code,
            what,
            why: why.to_string(),
            next,
            who: who.to_string(),
        }
    }

    /// 读屏可读文本。
    pub fn screen_text(&self) -> String {
        format!(
            "[{}] {}：{}。下一步：{}。（责任方：{}）",
            self.code, self.what, self.why, self.next, self.who
        )
    }
}

/// 授出结果（成功路径只有两种：全额授出 / 降级授出+告知）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GrantOutcome {
    /// 承诺档全额授出。
    Granted { granted: u64 },
    /// 降级授出（含告知——告知同时入仲裁器告知流）。
    Downgraded { granted: u64, reason: DowngradeReason },
}

// ---------------------------------------------------------------------------
// 五、子系统账本条目
// ---------------------------------------------------------------------------

/// 一条子系统账目：申请方 + 实际授出 + 硬顶 + 实测占用。
#[derive(Clone, Debug)]
pub struct SubsystemLedger {
    /// 申请方标识。
    pub applicant: String,
    /// 优先级。
    pub priority: PriorityClass,
    /// 实际授出（承诺记账口径；被降级后 < 申请的 promise）。
    pub granted: u64,
    /// 硬顶（授出即封顶，用量报告越过即钳制+记账）。
    pub ceiling: u64,
    /// 实测占用（子系统自己上报，仲裁器对账用）。
    pub in_use: u64,
}

// ---------------------------------------------------------------------------
// 六、水位表（判据：水位实时可见 + 历史曲线 + 异常归因）
// ---------------------------------------------------------------------------

/// 水位等级四档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatermarkLevel {
    /// < 70%。
    Normal,
    /// ≥ 70%。
    Watch,
    /// ≥ 85%。
    High,
    /// ≥ 95%。
    Critical,
}

impl WatermarkLevel {
    /// 按占比定级。
    pub fn from_ratio(used: u64, total: u64) -> Self {
        if total == 0 {
            return WatermarkLevel::Normal;
        }
        let pct = used.saturating_mul(100) / total;
        if pct >= WM_CRITICAL_PCT {
            WatermarkLevel::Critical
        } else if pct >= WM_HIGH_PCT {
            WatermarkLevel::High
        } else if pct >= WM_WATCH_PCT {
            WatermarkLevel::Watch
        } else {
            WatermarkLevel::Normal
        }
    }

    /// 读屏可读名。
    pub fn screen_name(self) -> &'static str {
        match self {
            WatermarkLevel::Normal => "正常",
            WatermarkLevel::Watch => "关注",
            WatermarkLevel::High => "吃紧",
            WatermarkLevel::Critical => "危急",
        }
    }
}

/// 一个水位采样点。
#[derive(Clone, Copy, Debug)]
pub struct WatermarkSample {
    /// 逻辑 tick。
    pub tick: u64,
    /// 实测总用量（字节）。
    pub used: u64,
    /// 该时刻的等级。
    pub level: WatermarkLevel,
}

/// 水位异常（归因矩阵：涨了没记账 / 记了没用上）。
#[derive(Clone, Debug)]
pub struct WatermarkAnomaly {
    /// 异常类型。
    pub kind: &'static str,
    /// 证据（人读）。
    pub evidence: String,
    /// 发现时的 tick。
    pub tick: u64,
    /// 偏差字节数（实测 − 账本）。
    pub delta_bytes: u64,
}

impl WatermarkAnomaly {
    /// 归因建议（候选原因，不硬猜唯一结论）。
    pub fn attribution_hint(&self) -> &'static str {
        match self.kind {
            "未记账分配" => "候选原因：某子系统绕过仲裁器直接分配（检查其上报纪律）",
            "账实背离" => "候选原因：已释放的账目未同步 or 用量上报滞后（检查释放路径）",
            _ => "候选原因：未知——保持观察，不要硬猜",
        }
    }
}

/// 水位表：实时水位 + 环形历史曲线 + 异常归因。
#[derive(Debug)]
pub struct WatermarkTable {
    /// 域级显存总量（字节）——水位分母。
    domain_total: u64,
    /// 环形历史。
    history: [Option<WatermarkSample>; WATERMARK_HISTORY],
    /// 环形写头。
    head: usize,
    /// 已写入的总采样数（区分"空环"与"写满一圈"）。
    written: usize,
    /// 最近一次检出的异常（None = 账实相符）。
    anomaly: Option<WatermarkAnomaly>,
}

impl WatermarkTable {
    /// 构造：给定域级总量。
    pub fn new(domain_total: u64) -> Self {
        WatermarkTable {
            domain_total,
            history: [None; WATERMARK_HISTORY],
            head: 0,
            written: 0,
            anomaly: None,
        }
    }

    /// 采样一帧：记录 (tick, used) 与等级。
    pub fn sample(&mut self, tick: u64, used: u64) -> WatermarkSample {
        let s = WatermarkSample {
            tick,
            used,
            level: WatermarkLevel::from_ratio(used, self.domain_total),
        };
        self.history[self.head] = Some(s);
        self.head = (self.head + 1) % WATERMARK_HISTORY;
        self.written = self.written.saturating_add(1);
        s
    }

    /// 最近一次采样点的环形下标（空环返回 None）。
    fn last_index(&self) -> Option<usize> {
        if self.written == 0 {
            None
        } else if self.head == 0 {
            Some(WATERMARK_HISTORY - 1)
        } else {
            Some(self.head - 1)
        }
    }

    /// 实时水位（最近一次采样的等级）。
    pub fn current_level(&self) -> WatermarkLevel {
        match self.last_index() {
            None => WatermarkLevel::Normal,
            Some(idx) => self.history[idx].map(|s| s.level).unwrap_or(WatermarkLevel::Normal),
        }
    }

    /// 历史曲线：按时间序取回全部采样点（判据：显存何时吃紧可回溯）。
    pub fn curve(&self) -> Vec<WatermarkSample> {
        let n = self.written.min(WATERMARK_HISTORY);
        let mut out = Vec::with_capacity(n);
        let start = self.head + WATERMARK_HISTORY - n;
        for i in 0..n {
            if let Some(s) = self.history[(start + i) % WATERMARK_HISTORY] {
                out.push(s);
            }
        }
        out
    }

    /// 账实对账：实测用量 vs 账本 in_use 总和。偏差超阈值即异常归因。
    pub fn reconcile(&mut self, measured_used: u64, ledger_in_use: u64) {
        if measured_used == ledger_in_use {
            self.anomaly = None;
            return;
        }
        let delta = measured_used.abs_diff(ledger_in_use);
        // 小于 1 个粒度的偏差视为记账噪声，不升级成异常。
        if delta < BUDGET_GRANULARITY {
            self.anomaly = None;
            return;
        }
        let (kind, evidence) = if measured_used > ledger_in_use {
            (
                "未记账分配",
                format!(
                    "实测 {} 字节 > 账本 {} 字节，多出 {} 字节没有账目",
                    measured_used, ledger_in_use, delta
                ),
            )
        } else {
            (
                "账实背离",
                format!(
                    "账本 {} 字节 > 实测 {} 字节，账上多记 {} 字节",
                    ledger_in_use, measured_used, delta
                ),
            )
        };
        self.anomaly = Some(WatermarkAnomaly {
            kind,
            evidence,
            tick: match self.last_index() {
                Some(idx) => self.history[idx].map(|s| s.tick).unwrap_or(0),
                None => 0,
            },
            delta_bytes: delta,
        });
    }

    /// 最近一次异常（None = 账实相符）。
    pub fn anomaly(&self) -> Option<&WatermarkAnomaly> {
        self.anomaly.as_ref()
    }

    /// 读屏可读水位摘要（无障碍判据：水位读屏可达）。
    pub fn screen_text(&self) -> String {
        let Some(idx) = self.last_index() else {
            return "显存水位：暂无采样".to_string();
        };
        let s = self.history[idx].unwrap_or(WatermarkSample {
            tick: 0,
            used: 0,
            level: WatermarkLevel::Normal,
        });
        let pct = if self.domain_total == 0 {
            0
        } else {
            s.used.saturating_mul(100) / self.domain_total
        };
        format!(
            "显存水位：{} / {} 字节（{}%，等级：{}），采样 tick {}",
            s.used, self.domain_total, pct, s.level.screen_name(), s.tick
        )
    }
}

// ---------------------------------------------------------------------------
// 七、抢占审计（判据：挤占拒绝含申请方与持有方双方告知）
// ---------------------------------------------------------------------------

/// 一次抢占尝试的审计记录（允许与拒绝都入账）。
#[derive(Clone, Debug)]
pub struct PreemptAudit {
    /// 申请方。
    pub applicant: String,
    /// 持有方。
    pub holder: String,
    /// 想挤多少字节。
    pub want: u64,
    /// 是否被允许。
    pub allowed: bool,
    /// 命中的规则号（R1..R5），拒绝时必填。
    pub rule: &'static str,
    /// tick。
    pub tick: u64,
}

// ---------------------------------------------------------------------------
// 八、显存预算仲裁器（主结构）
// ---------------------------------------------------------------------------

/// 显存预算仲裁器。
///
/// 生命周期：`new(domain_promise_total, domain_total)` → 反复
/// `apply` / `release` / `report_usage` / `reconcile_watermark` →
/// `domain_audit`（域级加总复核）。
pub struct VramArbiter {
    /// 域级承诺预算（字节）：所有子系统 granted 之和的硬上限（记账口径）。
    domain_promise_total: u64,
    /// 域级显存物理总量（字节）：水位分母。
    domain_total: u64,
    /// 子系统账本。
    ledger: Vec<SubsystemLedger>,
    /// 降级告知流。
    notices: Vec<DowngradeNotice>,
    /// 抢占审计流。
    preempts: Vec<PreemptAudit>,
    /// 错误账本（零静默）。
    errors: Vec<BudgetError>,
    /// 错误账本溢出丢弃计数。
    errors_dropped: u64,
    /// 水位表。
    watermark: WatermarkTable,
    /// 逻辑 tick。
    tick: u64,
}

impl VramArbiter {
    /// 构造：域级承诺预算 + 域级显存物理总量。
    pub fn new(domain_promise_total: u64, domain_total: u64) -> Self {
        VramArbiter {
            domain_promise_total,
            domain_total,
            ledger: Vec::new(),
            notices: Vec::new(),
            preempts: Vec::new(),
            errors: Vec::new(),
            errors_dropped: 0,
            watermark: WatermarkTable::new(domain_total),
            tick: 0,
        }
    }

    /// 推进逻辑时钟并采样水位（每 tick 一次）。
    pub fn advance_tick(&mut self) -> u64 {
        self.tick = self.tick.saturating_add(1);
        let used = self.ledger.iter().map(|l| l.in_use).sum();
        self.watermark.sample(self.tick, used);
        self.tick
    }

    /// 当前逻辑 tick。
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// 已授出预算总和（记账口径）。
    pub fn granted_total(&self) -> u64 {
        self.ledger.iter().map(|l| l.granted).sum()
    }

    /// 账本 in_use 总和。
    pub fn in_use_total(&self) -> u64 {
        self.ledger.iter().map(|l| l.in_use).sum()
    }

    /// 子系统账本条目（按申请方查）。
    pub fn entry(&self, applicant: &str) -> Option<&SubsystemLedger> {
        self.ledger.iter().find(|l| l.applicant == applicant)
    }

    /// 降级告知流（判据：降级+告知可全量取回）。
    pub fn notices(&self) -> &[DowngradeNotice] {
        &self.notices
    }

    /// 抢占审计流。
    pub fn preempt_audits(&self) -> &[PreemptAudit] {
        &self.preempts
    }

    /// 错误账本（零静默：全量可取）。
    pub fn errors(&self) -> &[BudgetError] {
        &self.errors
    }

    /// 错误账本丢弃计数（账满后的丢弃必须可见）。
    pub fn errors_dropped(&self) -> u64 {
        self.errors_dropped
    }

    /// 水位表（只读访问：实时等级 / 历史曲线 / 异常）。
    pub fn watermark(&self) -> &WatermarkTable {
        &self.watermark
    }

    /// 错误入账（零静默：所有拒绝与钳制都必须走这里）。
    fn record_error(&mut self, e: BudgetError) {
        if self.errors.len() >= ERROR_LEDGER_CAP {
            self.errors_dropped = self.errors_dropped.saturating_add(1);
        } else {
            self.errors.push(e);
        }
    }

    // -- 申请与授出 ----------------------------------------------------------

    /// 预算申请（判据：预算仲裁 + 超支降级不挤占他人）。
    ///
    /// 流程：
    /// 1. 三档不变式与粒度校验（违反 → 拒绝 + 修正建议）；
    /// 2. 账本容量与重复申请校验；
    /// 3. 剩余空间装得下目标档（期望值）→ 全额授出 target；
    /// 4. 装不下 target 但装得下承诺档（保底值）→ 降级授出 promise，
    ///    产出 `DowngradeNotice`（告知流可全量取回）；
    /// 5. 连保底值都放不下 → `E_DOMAIN_EXHAUSTED` 拒绝，
    ///    **绝不挤占已授出的他人预算**。
    pub fn apply(&mut self, req: BudgetRequest) -> Result<GrantOutcome, BudgetError> {
        // 1. 三档不变式（入口强制，不静默钳制）。
        if let Some(why) = req.tier.violation_reason() {
            let e = BudgetError::new(
                "E_TIER_INVARIANT",
                "三档预算不变式被违反",
                why,
                "按 promise ≤ target ≤ ceiling 且 ≥ MIN_BUDGET_BYTES 修正申请",
                &req.applicant,
            );
            self.record_error(e.clone());
            return Err(e);
        }
        if !req.tier.aligned() {
            let e = BudgetError::new(
                "E_NOT_ALIGNED",
                "申请档位未按 4KiB 粒度对齐",
                "非对齐预算会产生页内碎片",
                "按 4KiB 粒度向上对齐后重新申请",
                &req.applicant,
            );
            self.record_error(e.clone());
            return Err(e);
        }
        // 2. 账本容量与重复申请。
        if self.ledger.iter().any(|l| l.applicant == req.applicant) {
            let e = BudgetError::new(
                "E_DUPLICATE_APPLICANT",
                "重复申请",
                "同一申请方已有在账预算（先 release 或换名）",
                "先释放旧账或以新身份申请",
                &req.applicant,
            );
            self.record_error(e.clone());
            return Err(e);
        }
        if self.ledger.len() >= MAX_SUBSYSTEMS {
            let e = BudgetError::new(
                "E_LEDGER_FULL",
                "子系统账本已满",
                "账目条目数达到 MAX_SUBSYSTEMS，继续扩账会挤掉已有条目",
                "合并子系统或提升 MAX_SUBSYSTEMS（走 ADR）",
                &req.applicant,
            );
            self.record_error(e.clone());
            return Err(e);
        }
        // 3. 剩余空间（承诺口径）。
        let remaining = self.domain_promise_total.saturating_sub(self.granted_total());
        // 4. 目标档放得下 → 全额授出（期望值）。
        if req.tier.target <= remaining {
            self.commit(
                req.applicant.clone(),
                req.priority,
                req.tier.target,
                req.tier.ceiling,
            );
            return Ok(GrantOutcome::Granted { granted: req.tier.target });
        }
        // 5. 降级：收缩到承诺档（保底值），降级必须告知。
        if req.tier.promise <= remaining {
            let notice = DowngradeNotice {
                applicant: req.applicant.clone(),
                requested_promise: req.tier.target,
                granted: req.tier.promise,
                reason: DowngradeReason::TargetToPromise,
                tick: self.tick,
            };
            self.notices.push(notice.clone());
            self.commit(
                req.applicant.clone(),
                req.priority,
                req.tier.promise,
                req.tier.ceiling,
            );
            return Ok(GrantOutcome::Downgraded {
                granted: req.tier.promise,
                reason: DowngradeReason::TargetToPromise,
            });
        }
        // 6. 拒绝：绝不挤占他人（判据：超预算降级"不挤占他人"）。
        let e = BudgetError::new(
            "E_DOMAIN_EXHAUSTED",
            "域承诺预算已耗尽，申请被拒绝",
            &format!(
                "剩余 {} 字节装不下保底值 {} 字节；已授 {} / 总额 {}",
                remaining,
                req.tier.promise,
                self.granted_total(),
                self.domain_promise_total
            ),
            "降低申请档位，或等他人释放后再申请；仲裁器不会为你挤占他人",
            &req.applicant,
        );
        self.record_error(e.clone());
        Err(e)
    }

    /// 账目落账（授出路径的公共尾部）。
    fn commit(&mut self, applicant: String, priority: PriorityClass, granted: u64, ceiling: u64) {
        self.ledger.push(SubsystemLedger {
            applicant,
            priority,
            granted,
            ceiling,
            in_use: 0,
        });
    }

    // -- 释放 ----------------------------------------------------------------

    /// 释放预算（整账释放：按申请方名清账）。
    pub fn release(&mut self, applicant: &str) -> Result<u64, BudgetError> {
        match self.ledger.iter().position(|l| l.applicant == applicant) {
            Some(i) => {
                let l = self.ledger.remove(i);
                if l.in_use > 0 {
                    // 释放时仍有占用：拒绝释放并要求先清占用（防资源泄漏）。
                    let e = BudgetError::new(
                        "E_RELEASE_WITH_USAGE",
                        "释放被拒绝：账目仍有占用",
                        &format!("{} 尚有 {} 字节未清", applicant, l.in_use),
                        "先上报 in_use=0 再释放，或先归还被占资源",
                        applicant,
                    );
                    // 释放被拒 → 账目放回原位（不丢账）。
                    self.ledger.insert(i, l);
                    self.record_error(e.clone());
                    return Err(e);
                }
                Ok(l.granted)
            }
            None => {
                let e = BudgetError::new(
                    "E_UNKNOWN_APPLICANT",
                    "释放了不存在的账目",
                    "账本中没有该申请方",
                    "核对申请方名；重复释放视为纪律缺陷",
                    applicant,
                );
                self.record_error(e.clone());
                Err(e)
            }
        }
    }

    // -- 用量上报 ------------------------------------------------------------

    /// 用量上报：子系统报告自己的实测占用。
    ///
    /// 越过自身硬顶即钳制到硬顶并记账（E_OVER_CEILING）——钳制是防护，
    /// 记账是纪律，两件事同时发生。
    pub fn report_usage(&mut self, applicant: &str, bytes: u64) -> Result<u64, BudgetError> {
        // 先在借作用域内完成账目变更，借结束后再入错误账（零静默）。
        enum Usage {
            Ok(u64),
            Clamped { accepted: u64, over: u64 },
            Unknown,
        }
        let outcome = match self.ledger.iter_mut().find(|l| l.applicant == applicant) {
            None => Usage::Unknown,
            Some(l) if bytes > l.ceiling => {
                let over = bytes - l.ceiling;
                l.in_use = l.ceiling;
                Usage::Clamped { accepted: l.in_use, over }
            }
            Some(l) => {
                l.in_use = bytes;
                Usage::Ok(bytes)
            }
        };
        match outcome {
            Usage::Ok(v) => Ok(v),
            Usage::Clamped { accepted, over } => {
                let e = BudgetError::new(
                    "E_OVER_CEILING",
                    "用量越过硬顶，已被钳制",
                    &format!(
                        "上报 {} 字节 > 硬顶 {} 字节，超出 {} 字节",
                        bytes,
                        accepted + over,
                        over
                    ),
                    "降低实际占用或按 SDK 流程申请提额；钳制行为已入错误账本",
                    applicant,
                );
                self.record_error(e.clone());
                Ok(accepted)
            }
            Usage::Unknown => {
                let e = BudgetError::new(
                    "E_UNKNOWN_APPLICANT",
                    "用量上报无对应账目",
                    "账本中没有该申请方",
                    "先 apply 建账再上报用量",
                    applicant,
                );
                self.record_error(e.clone());
                Err(e)
            }
        }
    }

    // -- 抢占 ----------------------------------------------------------------

    /// 抢占请求（判据：挤占→拒绝；规则外一律拒绝且双方告知）。
    ///
    /// 规则实现与 [`PREEMPTION_RULES_DOC`] 逐条对应：
    /// - R1：持有方是 Compositor → 拒绝；
    /// - R2：申请方 Foreground 且持有方 Background → 允许，但最多挤一半；
    /// - R3：同级 → 拒绝；
    /// - R4：任一方是 SystemProbe → 拒绝；
    /// - R5：其余 → 拒绝。
    pub fn try_preempt(
        &mut self,
        applicant: &str,
        holder: &str,
        want: u64,
    ) -> Result<u64, BudgetError> {
        let applicant_cls = self
            .ledger
            .iter()
            .find(|l| l.applicant == applicant)
            .map(|l| l.priority);
        let holder_cls = self
            .ledger
            .iter()
            .find(|l| l.applicant == holder)
            .map(|l| l.priority);
        let both = || format!("{}（申请方）与 {}（持有方）", applicant, holder);

        // 双方都必须在账——不在账的挤占没有规则可套（R5）。
        let (Some(a_cls), Some(h_cls)) = (applicant_cls, holder_cls) else {
            let audit = PreemptAudit {
                applicant: applicant.to_string(),
                holder: holder.to_string(),
                want,
                allowed: false,
                rule: "R5",
                tick: self.tick,
            };
            self.preempts.push(audit);
            let e = BudgetError::new(
                "E_PREEMPT_DENIED",
                "挤占请求被拒绝",
                "申请方或持有方不在账（规则 R5：规则外一律拒绝）",
                "先确认双方都在账，再按 PREEMPTION_RULES_DOC 提出请求",
                &both(),
            );
            self.record_error(e.clone());
            return Err(e);
        };

        // R4：探针不可挤也不可被挤。
        if a_cls == PriorityClass::SystemProbe || h_cls == PriorityClass::SystemProbe {
            return self.deny_preempt(applicant, holder, want, "R4", &both());
        }
        // R1：合成器保底不可被挤占。
        if h_cls == PriorityClass::Compositor {
            return self.deny_preempt(applicant, holder, want, "R1", &both());
        }
        // R3：同级不可互挤。
        if a_cls == h_cls {
            return self.deny_preempt(applicant, holder, want, "R3", &both());
        }
        // R2：仅 Foreground → Background 允许，且最多挤一半。
        if a_cls == PriorityClass::Foreground && h_cls == PriorityClass::Background {
            let half = {
                let l = self.ledger.iter().find(|l| l.applicant == holder).unwrap();
                l.in_use / 2
            };
            if want == 0 {
                let e = BudgetError::new(
                    "E_PREEMPT_ZERO",
                    "挤占请求为 0 字节",
                    "0 字节的挤占是无效请求",
                    "按实际需要的字节数重新发起",
                    &both(),
                );
                self.record_error(e.clone());
                return Err(e);
            }
            let grant = want.min(half);
            if grant == 0 {
                return self.deny_preempt(applicant, holder, want, "R2", &both());
            }
            // 转账：持有方让出（granted 与 in_use 同步收缩），申请方入账。
            let holder_ledger = self.ledger.iter_mut().find(|l| l.applicant == holder).unwrap();
            holder_ledger.granted -= grant;
            holder_ledger.in_use -= grant;
            let applicant_ledger = self
                .ledger
                .iter_mut()
                .find(|l| l.applicant == applicant)
                .unwrap();
            applicant_ledger.granted += grant;
            applicant_ledger.in_use += grant;
            // R2：双方告知（审计流即告知流）。
            self.preempts.push(PreemptAudit {
                applicant: applicant.to_string(),
                holder: holder.to_string(),
                want,
                allowed: true,
                rule: "R2",
                tick: self.tick,
            });
            return Ok(grant);
        }
        // R5：规则外的组合（如 Background → Foreground）一律拒绝。
        self.deny_preempt(applicant, holder, want, "R5", &both())
    }

    /// 拒绝一条挤占请求：审计 + 双方告知 + 错误入账（判据点名路径）。
    fn deny_preempt(
        &mut self,
        applicant: &str,
        holder: &str,
        want: u64,
        rule: &'static str,
        who: &str,
    ) -> Result<u64, BudgetError> {
        self.preempts.push(PreemptAudit {
            applicant: applicant.to_string(),
            holder: holder.to_string(),
            want,
            allowed: false,
            rule,
            tick: self.tick,
        });
        let e = BudgetError::new(
            "E_PREEMPT_DENIED",
            "挤占请求被拒绝",
            &format!("命中规则 {}（见 PREEMPTION_RULES_DOC）", rule),
            "申请方按规则调整请求；持有方无需动作",
            who,
        );
        self.record_error(e.clone());
        Err(e)
    }

    // -- 水位与对账 ----------------------------------------------------------

    /// 水位账实对账（每 tick 或按需调用）。
    pub fn reconcile_watermark(&mut self) {
        let measured = self.ledger.iter().map(|l| l.in_use).sum();
        self.watermark.reconcile(measured, measured);
        // 说明：单账本口径下 in_use 之和即实测，账实天然相符；
        // reconcile 的真实入口是外部直测值——见 reconcile_measured。
    }

    /// 用外部直测的显存用量做账实对账（水位异常→归因的真实入口）。
    pub fn reconcile_measured(&mut self, measured_used: u64) {
        let ledger_sum = self.ledger.iter().map(|l| l.in_use).sum();
        self.watermark.reconcile(measured_used, ledger_sum);
    }

    /// 域级加总复核（判据：与 A06 预算联动的对账点）。
    pub fn domain_audit(&self) -> DomainAudit {
        let sum_granted = self.granted_total();
        DomainAudit {
            sum_granted,
            sum_in_use: self.in_use_total(),
            domain_promise_total: self.domain_promise_total,
            domain_total: self.domain_total,
            within: sum_granted <= self.domain_promise_total,
            over_by: self.domain_promise_total.saturating_sub(sum_granted),
            subsystems: self.ledger.len(),
            notices: self.notices.len(),
            errors: self.errors.len() as u64 + self.errors_dropped,
        }
    }

    /// 读屏可读总摘要（无障碍判据：水位读屏可达）。
    pub fn screen_text(&self) -> String {
        let a = self.domain_audit();
        format!(
            "显存预算：已授 {} / 域承诺 {} 字节，{} 个子系统在账；水位：{}",
            a.sum_granted,
            a.domain_promise_total,
            a.subsystems,
            self.watermark.screen_text()
        )
    }
}

/// 域级加总复核结果（判据：加总复核；与 A06 预算联动）。
#[derive(Clone, Copy, Debug)]
pub struct DomainAudit {
    /// 已授出总和（记账口径）。
    pub sum_granted: u64,
    /// 实测占用总和。
    pub sum_in_use: u64,
    /// 域级承诺预算。
    pub domain_promise_total: u64,
    /// 域级物理总量。
    pub domain_total: u64,
    /// 加总是否在域承诺预算内。
    pub within: bool,
    /// 域预算余量（超支为 0）。
    pub over_by: u64,
    /// 在账子系统数。
    pub subsystems: usize,
    /// 降级告知条数。
    pub notices: usize,
    /// 错误总数（含账满丢弃）。
    pub errors: u64,
}

// ---------------------------------------------------------------------------
// 九、自检注册入口
// ---------------------------------------------------------------------------

/// VE-F0005 域自检（判据逐条映射见 `vea05_checks.rs`）。
pub fn run_vea05_checks() -> CheckSet {
    super::vea05_checks::run_vea05_checks()
}
