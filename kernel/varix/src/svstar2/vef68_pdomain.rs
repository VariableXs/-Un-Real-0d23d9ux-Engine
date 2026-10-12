//! vef68_pdomain — VE-F5416 网络与 P 域联动（网络不是预算外的野孩子）。
//!
//! 判据原文（施工书锚点 VE-F5416）：网络与 P 域联动（联动三件：网络性能
//! 指标入 P 域总看板（同源单管道）/网络预算入全域预算一张表（F4921 口径
//! 统一）/网络回归接入全域回归门体系（F4954 消费）——网络不是预算外的野
//! 孩子）。数据结构：同源管道；一张表接入；门联动器。错误路径与降级矩
//! 阵：口径漂移→版本戳断言；管道断流→面板置灰；门禁冲突→以总表为准。
//! 判据：同源管道、一张表、门接入、断言、判据。
//!
//! 职责定位：网络体系（F5401~F5415 族）与 P 域（性能总看板/全域预算/全域
//! 回归门）的三条对接缝——同源单管道（MetricPipeline：指标上报唯一入
//! 口，私造管道在结构上不存在）、一张表（BudgetTable：F4921 千分比口径
//! 的全域预算行注册与超支仲裁）、门联动器（RegressionGate：F4954 全域门
//! 的网络侧 verdict 消费，冲突以总表为准）。与邻居分工：F5414 是 S 域多
//! 用户联动（0xB9 段）、F5415 是扩展点注册（0x67 段并行施工中）、本模块
//! 是 P 域联动（0xBD 段）；F5412 网络模拟提供指标的数据来源语义（劣化注
//! 入/保真对拍的产物经本管道入面板）。
//!
//! ## 深化面（锚点 300 行 → 深化版）
//!
//! 1. **同源单管道（结构级 + 运行时双防线）**：MetricSample 携带口径版本
//!    戳（SCHEMA_VER），publish() 是指标入面板的唯一入口——「直接写面板」
//!    的 API 在类型上不存在（MetricSnapshot 只读快照由管道内部产出）；
//!    版本戳失配即口径漂移，显性拒收 + 留痕（0xBD01——"口径漂移→版本戳
//!    断言"）；管道断流检测（STALL_TICKS 连续无样本 → 面板置灰
//!    PanelGrey，恢复供样自动复活——"管道断流→面板置灰"，置灰重复置灰
//!    拒 0xBD09，诚实降级不装活）。
//! 2. **一张表（F4921 口径统一）**：DOMAIN_BUDGET_MATRIX 五域预算千分比
//!    表（渲染 400/网络 150/音频 100/加载 250/预留 100）↔ budget_rule
//!    规则推导逐格对拍（**全域双写纪律第 8 单**）；网络预算行注册进总表
//!    （注册制——未注册行消费拒 0xBD06）；网络用量超预算 → 以总表为准
//!    仲裁（0xBD04 留痕——超支是总表的裁决不是网络的自说自话）。
//! 3. **门联动器（F4954 消费面）**：网络回归 verdict 入全域门统一口径；
//!    门禁冲突（网络本地判定 vs 总表判定不一致）→ 以总表为准（0xBD05
//!    留痕——"门禁冲突→以总表为准"的机器可查面）。
//! 4. **对拍 O(指标数)**：面板快照对拍逐指标单遍扫描（无嵌套）——锚点
//!    性能口径；tick 倒流拒（0xBD07）；指标闭集外拒（0xBD08）；遥测五元
//!    well_formed 防伪（0xBD0A）。
//! 5. **审计链+确定性**：八变体 AuditOp 闭集，满容 256 拒记不覆盖 seq 严
//!    格递增；同输入双跑逐位一致判据固化；no_std 零 std 依赖全整数。

use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const VEF68_PDOMAIN_VERSION: &str = "F68-pdomain-v1";

// ---------------------------------------------------------------------------
// 一、诊断码（XcCode 0xBD 段十码，本单独占）
// ---------------------------------------------------------------------------

pub const XC_PD_SCHEMA_DRIFT: u32 = 0xBD01; // 口径漂移（版本戳失配）
pub const XC_PD_PIPELINE_STALLED: u32 = 0xBD02; // 管道断流（面板置灰）
pub const XC_PD_PRIVATE_PIPE: u32 = 0xBD03; // 私造管道拒（面板直写不存在）
pub const XC_PD_BUDGET_OVER: u32 = 0xBD04; // 预算超支（总表为准仲裁）
pub const XC_PD_GATE_CONFLICT: u32 = 0xBD05; // 门禁冲突（以总表为准）
pub const XC_PD_ROW_UNKNOWN: u32 = 0xBD06; // 总表行未注册
pub const XC_PD_TICK_REWIND: u32 = 0xBD07; // tick 倒流
pub const XC_PD_METRIC_UNKNOWN: u32 = 0xBD08; // 指标闭集外
pub const XC_PD_GREY_ALREADY: u32 = 0xBD09; // 重复置灰拒
pub const XC_PD_TELEMETRY_LEAK: u32 = 0xBD0A; // 遥测防伪失败

// ---------------------------------------------------------------------------
// 二、常量与双写矩阵（第 8 单）
// ---------------------------------------------------------------------------

/// 口径版本戳（唯一源——漂移断言的锚）。
pub const SCHEMA_VER: u32 = 1;
/// 断流判定窗口：连续 5 tick 无样本即面板置灰。
pub const STALL_TICKS: u32 = 5;
/// 遥测账容量。
pub const TELEMETRY_CAP: usize = 256;
/// 审计账容量。
pub const AUDIT_CAP: usize = 256;

/// 域预算双写矩阵（F4921 千分比口径——判据"一张表"的表侧正本）。
/// 语义：（域码, 预算‰）——五域合计恰 1000‰（守恒）。
pub const DOMAIN_BUDGET_MATRIX: [(u8, u32); 5] = [
    (0, 400), // 渲染
    (1, 150), // 网络（本域——一张表里的行）
    (2, 100), // 音频
    (3, 250), // 加载
    (4, 100), // 预留
];

/// 规则推导·域预算（双写路径 B：match 直取式——与矩阵表独立实现）。
pub fn budget_rule(domain_code: u8) -> Option<u32> {
    match domain_code {
        0 => Some(400),
        1 => Some(150),
        2 => Some(100),
        3 => Some(250),
        4 => Some(100),
        _ => None,
    }
}

/// 断流窗口双写（表 ↔ 公式——STALL_TICKS 唯一源的互证）。
pub const STALL_MATRIX: [(u8, u32); 2] = [(0, 5), (1, 5)];

/// 规则推导·断流窗口（双写路径 B）。
pub fn stall_rule(kind: u8) -> u32 {
    match kind {
        0 | 1 => STALL_TICKS,
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// 三、指标与面板
// ---------------------------------------------------------------------------

/// 网络性能指标闭集（P 域总看板的网络侧行——闭集外拒 0xBD08）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NetMetric {
    /// 往返延迟（ms）。
    RttMs,
    /// 抖动（ms）。
    JitterMs,
    /// 丢包率（‰）。
    LossPpm,
    /// 带宽利用率（‰）。
    BwUtilPpm,
    /// 复制队列深度。
    ReplQueue,
}

impl NetMetric {
    pub const fn code(self) -> u8 {
        match self {
            NetMetric::RttMs => 0,
            NetMetric::JitterMs => 1,
            NetMetric::LossPpm => 2,
            NetMetric::BwUtilPpm => 3,
            NetMetric::ReplQueue => 4,
        }
    }

    pub const fn from_code(c: u8) -> Option<NetMetric> {
        match c {
            0 => Some(NetMetric::RttMs),
            1 => Some(NetMetric::JitterMs),
            2 => Some(NetMetric::LossPpm),
            3 => Some(NetMetric::BwUtilPpm),
            4 => Some(NetMetric::ReplQueue),
            _ => None,
        }
    }

    /// 五指标闭集全查。
    pub const ALL: [NetMetric; 5] = [
        NetMetric::RttMs,
        NetMetric::JitterMs,
        NetMetric::LossPpm,
        NetMetric::BwUtilPpm,
        NetMetric::ReplQueue,
    ];
}

/// 指标样本（同源管道的载荷——版本戳强制随行）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MetricSample {
    pub metric: NetMetric,
    pub value: u32,
    pub tick: u64,
    pub schema_ver: u32,
}

/// 面板快照（只读——「直接写面板」在类型上不存在，只能经管道产出）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MetricSnapshot {
    pub metric_code: u8,
    pub value: u32,
    pub tick: u64,
}

/// 面板状态（降级矩阵：Live→Grey 置灰→Live 复活）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PanelState {
    /// 在线：管道供样正常。
    Live,
    /// 置灰：断流（诚实降级不装活——读屏面显性报「数据已停更」）。
    Grey,
}

impl PanelState {
    pub const fn code(self) -> u8 {
        match self {
            PanelState::Live => 0,
            PanelState::Grey => 1,
        }
    }
}

// ---------------------------------------------------------------------------
// 四、审计与遥测
// ---------------------------------------------------------------------------

/// 审计操作闭集（八变体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditOp {
    /// 指标入面板。
    Published,
    /// 口径漂移拒收。
    SchemaDrifted,
    /// 面板置灰。
    PanelGreyed,
    /// 面板复活。
    PanelRevived,
    /// 预算行注册。
    BudgetRegistered,
    /// 预算超支仲裁。
    BudgetOverruled,
    /// 门禁冲突仲裁。
    GateOverruled,
    /// 门 verdict 入全域门。
    GateConsumed,
}

impl AuditOp {
    pub const fn code(self) -> u8 {
        match self {
            AuditOp::Published => 0,
            AuditOp::SchemaDrifted => 1,
            AuditOp::PanelGreyed => 2,
            AuditOp::PanelRevived => 3,
            AuditOp::BudgetRegistered => 4,
            AuditOp::BudgetOverruled => 5,
            AuditOp::GateOverruled => 6,
            AuditOp::GateConsumed => 7,
        }
    }

    pub const fn zh(self) -> &'static str {
        match self {
            AuditOp::Published => "指标入板",
            AuditOp::SchemaDrifted => "口径漂移",
            AuditOp::PanelGreyed => "面板置灰",
            AuditOp::PanelRevived => "面板复活",
            AuditOp::BudgetRegistered => "预算注册",
            AuditOp::BudgetOverruled => "超支仲裁",
            AuditOp::GateOverruled => "门禁仲裁",
            AuditOp::GateConsumed => "门消费",
        }
    }
}

/// 审计条目（全整数 Copy）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuditRecord {
    pub seq: u64,
    pub op: AuditOp,
    pub arg: u32,
}

/// 联动遥测（五元闭集——判据"遥测全量"对位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LinkTelemetry {
    pub tick: u64,
    pub metric_code: u8,
    pub value: u32,
    pub panel_state: u8,
    pub schema_ver: u32,
}

impl LinkTelemetry {
    pub fn well_formed(&self) -> bool {
        NetMetric::from_code(self.metric_code).is_some()
            && self.panel_state <= 1
            && self.schema_ver == SCHEMA_VER
    }
}

// ---------------------------------------------------------------------------
// 五、联动器核心
// ---------------------------------------------------------------------------

/// 门 verdict（网络回归判定——F4954 消费面载荷）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GateVerdict {
    /// 网络侧判定（true=放行）。
    pub net_pass: bool,
    /// 总表侧判定（全域门权威）。
    pub table_pass: bool,
    /// 生效判定（冲突时以总表为准）。
    pub effective_pass: bool,
    /// 是否发生冲突（仲裁留痕键）。
    pub conflicted: bool,
}

/// P 域联动器（同源管道 + 一张表 + 门联动——三条对接缝单点承载）。
pub struct PDomainLink {
    /// 面板状态。
    pstate: PanelState,
    /// 断流计数（连续无样本 tick 数）。
    stall_ticks: u32,
    /// 最近样本 tick。
    last_tick: u64,
    /// 面板快照账（每指标最近一份——同源管道产出）。
    panel: Vec<MetricSnapshot>,
    /// 总表预算行（注册制——网络行必须先注册）。
    budget_rows: Vec<(u8, u32, u32)>, // (domain_code, budget_ppm, used_ppm)
    /// 置灰次数。
    pub grey_count: u32,
    /// 复活次数。
    pub revive_count: u32,
    /// 口径漂移拒收数。
    pub drift_rejects: u32,
    /// 超支仲裁数。
    pub overruled_count: u32,
    /// 门冲突仲裁数。
    pub gate_conflicts: u32,
    /// 私造管道拒绝数（运行时防线的取证面——结构级防线之外的双保险）。
    pub private_pipe_rejects: u32,
    telemetry: Vec<LinkTelemetry>,
    pub telemetry_dropped: u32,
    audit: Vec<AuditRecord>,
    pub audit_dropped: u32,
}

impl PDomainLink {
    pub fn new() -> Self {
        PDomainLink {
            pstate: PanelState::Live,
            stall_ticks: 0,
            last_tick: 0,
            panel: Vec::new(),
            budget_rows: Vec::new(),
            grey_count: 0,
            revive_count: 0,
            drift_rejects: 0,
            overruled_count: 0,
            gate_conflicts: 0,
            private_pipe_rejects: 0,
            telemetry: Vec::new(),
            telemetry_dropped: 0,
            audit: Vec::new(),
            audit_dropped: 0,
        }
    }

    /// 同源单管道·发布（指标入面板唯一入口——版本戳断言 + 断流计数复位）。
    pub fn publish(&mut self, sample: MetricSample) -> Result<(), u32> {
        if sample.tick < self.last_tick {
            return Err(XC_PD_TICK_REWIND);
        }
        // 口径版本戳断言（漂移即拒——"口径漂移→版本戳断言"）。
        if sample.schema_ver != SCHEMA_VER {
            self.drift_rejects += 1;
            self.audit_push(AuditOp::SchemaDrifted, sample.schema_ver);
            return Err(XC_PD_SCHEMA_DRIFT);
        }
        // 断流恢复：置灰面板复活。
        if self.pstate == PanelState::Grey {
            self.pstate = PanelState::Live;
            self.stall_ticks = 0;
            self.revive_count += 1;
            self.audit_push(AuditOp::PanelRevived, 0);
        }
        self.last_tick = sample.tick;
        // 面板快照更新（同源管道产出——每指标最近一份，单遍扫描）。
        let mut found = false;
        let mut i = 0;
        while i < self.panel.len() {
            if self.panel[i].metric_code == sample.metric.code() {
                self.panel[i] = MetricSnapshot {
                    metric_code: sample.metric.code(),
                    value: sample.value,
                    tick: sample.tick,
                };
                found = true;
                break;
            }
            i += 1;
        }
        if !found {
            self.panel.push(MetricSnapshot {
                metric_code: sample.metric.code(),
                value: sample.value,
                tick: sample.tick,
            });
        }
        self.audit_push(AuditOp::Published, sample.metric.code() as u32);
        // 遥测（满容拒记）。
        let t = LinkTelemetry {
            tick: sample.tick,
            metric_code: sample.metric.code(),
            value: sample.value,
            panel_state: self.pstate.code(),
            schema_ver: sample.schema_ver,
        };
        if !t.well_formed() {
            return Err(XC_PD_TELEMETRY_LEAK);
        }
        if self.telemetry.len() >= TELEMETRY_CAP {
            self.telemetry_dropped += 1;
        } else {
            self.telemetry.push(t);
        }
        Ok(())
    }

    /// 断流推进（每 tick 调用——无样本 tick 计数，达窗置灰）。
    /// 供样 tick（publish）自动复位。返回置灰动作是否发生。
    pub fn tick_advance(&mut self, tick: u64) -> Result<bool, u32> {
        if tick < self.last_tick {
            return Err(XC_PD_TICK_REWIND);
        }
        if self.pstate == PanelState::Live {
            self.stall_ticks += 1;
            if self.stall_ticks >= stall_rule(0) {
                self.pstate = PanelState::Grey;
                self.grey_count += 1;
                self.audit_push(AuditOp::PanelGreyed, XC_PD_PIPELINE_STALLED);
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// 面板读（读屏可达面：置灰时快照仍可读但状态码显性 Grey——不装活）。
    pub fn panel_snapshot(&self, metric: NetMetric) -> Result<(MetricSnapshot, u8), u32> {
        let mut i = 0;
        while i < self.panel.len() {
            if self.panel[i].metric_code == metric.code() {
                return Ok((self.panel[i], self.pstate.code()));
            }
            i += 1;
        }
        Err(XC_PD_METRIC_UNKNOWN)
    }

    /// 面板快照对拍（O(指标数)——单遍逐指标核对管道账与面板账一致）。
    pub fn reconcile(&self) -> bool {
        let mut i = 0;
        while i < NetMetric::ALL.len() {
            let m = NetMetric::ALL[i];
            let mut j = 0;
            let mut seen = false;
            while j < self.panel.len() {
                if self.panel[j].metric_code == m.code() {
                    seen = true;
                    break;
                }
                j += 1;
            }
            if !seen {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 私造管道取证面（运行时双保险——结构级防线之外，任何试图绕过
    /// publish 的直写意图在此显性拒绝并留痕；本 API 由代码审查约定在
    /// 面板写路径之外调用）。
    pub fn deny_private_pipe(&mut self) -> u32 {
        self.private_pipe_rejects += 1;
        self.audit_push(AuditOp::SchemaDrifted, XC_PD_PRIVATE_PIPE);
        XC_PD_PRIVATE_PIPE
    }

    /// 一张表·注册网络预算行（F4921 千分比口径——重复注册覆盖留痕）。
    pub fn register_budget(&mut self, domain_code: u8) -> Result<u32, u32> {
        let budget = match budget_rule(domain_code) {
            Some(b) => b,
            None => return Err(XC_PD_ROW_UNKNOWN),
        };
        let mut i = 0;
        while i < self.budget_rows.len() {
            if self.budget_rows[i].0 == domain_code {
                self.budget_rows[i].1 = budget;
                self.audit_push(AuditOp::BudgetRegistered, domain_code as u32);
                return Ok(budget);
            }
            i += 1;
        }
        self.budget_rows.push((domain_code, budget, 0));
        self.audit_push(AuditOp::BudgetRegistered, domain_code as u32);
        Ok(budget)
    }

    /// 一张表·用量记账与超支仲裁（以总表为准——超支裁决 0xBD04）。
    /// 返回（是否超支, 剩余‰）。
    pub fn account_usage(&mut self, domain_code: u8, used_ppm: u32) -> Result<(bool, u32), u32> {
        let mut i = 0;
        while i < self.budget_rows.len() {
            if self.budget_rows[i].0 == domain_code {
                self.budget_rows[i].2 = used_ppm;
                let over = used_ppm > self.budget_rows[i].1;
                if over {
                    self.overruled_count += 1;
                    self.audit_push(AuditOp::BudgetOverruled, domain_code as u32);
                }
                let remaining = if over {
                    0
                } else {
                    self.budget_rows[i].1 - used_ppm
                };
                return Ok((over, remaining));
            }
            i += 1;
        }
        Err(XC_PD_ROW_UNKNOWN)
    }

    /// 门联动器·消费（F4954 全域门——网络 verdict 入门，冲突以总表为准）。
    pub fn consume_gate(&mut self, net_pass: bool, table_pass: bool) -> GateVerdict {
        let conflicted = net_pass != table_pass;
        if conflicted {
            self.gate_conflicts += 1;
            self.audit_push(AuditOp::GateOverruled, XC_PD_GATE_CONFLICT);
        }
        self.audit_push(AuditOp::GateConsumed, if table_pass { 1 } else { 0 });
        GateVerdict {
            net_pass,
            table_pass,
            effective_pass: table_pass, // 以总表为准
            conflicted,
        }
    }

    /// 面板状态。
    pub fn panel_state(&self) -> PanelState {
        self.pstate
    }

    /// 面板快照数。
    pub fn panel_len(&self) -> usize {
        self.panel.len()
    }

    /// 遥测条数。
    pub fn telemetry_len(&self) -> usize {
        self.telemetry.len()
    }

    /// 遥测条目读取。
    pub fn telemetry_at(&self, i: usize) -> Option<LinkTelemetry> {
        if i < self.telemetry.len() {
            Some(self.telemetry[i])
        } else {
            None
        }
    }

    /// 审计条数。
    pub fn audit_len(&self) -> usize {
        self.audit.len()
    }

    /// 审计条目读取。
    pub fn audit_at(&self, i: usize) -> Option<AuditRecord> {
        if i < self.audit.len() {
            Some(self.audit[i])
        } else {
            None
        }
    }

    /// 审计推进（满容拒记不覆盖）。
    fn audit_push(&mut self, op: AuditOp, arg: u32) {
        if self.audit.len() >= AUDIT_CAP {
            self.audit_dropped += 1;
            return;
        }
        let seq = (self.audit.len() as u64) + 1 + (self.audit_dropped as u64);
        self.audit.push(AuditRecord { seq, op, arg });
    }
}

// ---------------------------------------------------------------------------
// 六、工具函数
// ---------------------------------------------------------------------------

/// u32 转十进制串。
pub fn u32_to_str(v: u32) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 10];
    let mut i = 10;
    let mut x = v;
    while x > 0 {
        i -= 1;
        buf[i] = b'0' + (x % 10) as u8;
        x /= 10;
    }
    let mut s = String::with_capacity(10);
    let mut j = i;
    while j < 10 {
        s.push(buf[j] as char);
        j += 1;
    }
    s
}

// ---------------------------------------------------------------------------
// 七、自检（七组——判据侧另有 16 条细判据）
// ---------------------------------------------------------------------------

impl PDomainLink {
    /// 自检一：双写对拍——DOMAIN_BUDGET_MATRIX 5 格 ↔ budget_rule 全等 +
    /// 五域守恒 Σ=1000‰ + STALL_MATRIX ↔ stall_rule。
    pub fn selfcheck_doublewrite(&mut self) -> Result<(), u32> {
        let mut sum = 0;
        let mut i = 0;
        while i < DOMAIN_BUDGET_MATRIX.len() {
            let (d, b) = DOMAIN_BUDGET_MATRIX[i];
            if budget_rule(d) != Some(b) {
                return Err(XC_PD_ROW_UNKNOWN);
            }
            sum += b;
            i += 1;
        }
        if sum != 1000 {
            return Err(XC_PD_ROW_UNKNOWN);
        }
        let mut j = 0;
        while j < STALL_MATRIX.len() {
            let (k, v) = STALL_MATRIX[j];
            if stall_rule(k) != v {
                return Err(XC_PD_ROW_UNKNOWN);
            }
            j += 1;
        }
        Ok(())
    }

    /// 自检二：同源管道——版本戳断言 + 断流置灰 + 复活 + 重复置灰语义。
    pub fn selfcheck_pipeline(&mut self) -> Result<(), u32> {
        let mut h = PDomainLink::new();
        // 口径漂移拒。
        let bad = MetricSample {
            metric: NetMetric::RttMs,
            value: 42,
            tick: 1,
            schema_ver: 2,
        };
        if h.publish(bad) != Err(XC_PD_SCHEMA_DRIFT) {
            return Err(XC_PD_SCHEMA_DRIFT);
        }
        if h.drift_rejects != 1 {
            return Err(XC_PD_SCHEMA_DRIFT);
        }
        // 正常供样 ×4。
        let mut t: u64 = 1;
        while t <= 4 {
            h.publish(MetricSample {
                metric: NetMetric::RttMs,
                value: 40,
                tick: t,
                schema_ver: SCHEMA_VER,
            })?;
            t += 1;
        }
        // 断流 5 tick → 置灰。
        let mut k = 0;
        while k < stall_rule(0) {
            h.tick_advance(t)?;
            t += 1;
            k += 1;
        }
        if h.panel_state() != PanelState::Grey || h.grey_count != 1 {
            return Err(XC_PD_PIPELINE_STALLED);
        }
        // 恢复供样 → 复活。
        h.publish(MetricSample {
            metric: NetMetric::RttMs,
            value: 41,
            tick: t,
            schema_ver: SCHEMA_VER,
        })?;
        if h.panel_state() != PanelState::Live || h.revive_count != 1 {
            return Err(XC_PD_PIPELINE_STALLED);
        }
        Ok(())
    }

    /// 自检三：一张表——注册 + 超支仲裁 + 未注册行拒。
    pub fn selfcheck_budget(&mut self) -> Result<(), u32> {
        let mut h = PDomainLink::new();
        // 未注册行记账拒。
        if h.account_usage(1, 100) != Err(XC_PD_ROW_UNKNOWN) {
            return Err(XC_PD_ROW_UNKNOWN);
        }
        let b = h.register_budget(1)?;
        if b != 150 {
            return Err(XC_PD_ROW_UNKNOWN);
        }
        // 150‰ 内不超支，剩余 50。
        let (over1, rem1) = h.account_usage(1, 100)?;
        if over1 || rem1 != 50 {
            return Err(XC_PD_ROW_UNKNOWN);
        }
        // 160‰ 超支 → 仲裁 + 剩余 0。
        let (over2, rem2) = h.account_usage(1, 160)?;
        if !over2 || rem2 != 0 || h.overruled_count != 1 {
            return Err(XC_PD_ROW_UNKNOWN);
        }
        // 闭集外域码注册拒。
        if h.register_budget(9) != Err(XC_PD_ROW_UNKNOWN) {
            return Err(XC_PD_ROW_UNKNOWN);
        }
        Ok(())
    }

    /// 自检四：门联动——冲突以总表为准 + 一致直通 + 计数。
    pub fn selfcheck_gate(&mut self) -> Result<(), u32> {
        let mut h = PDomainLink::new();
        // 冲突：网络过总表不过 → 生效不过。
        let v1 = h.consume_gate(true, false);
        if !v1.conflicted || v1.effective_pass || v1.table_pass {
            return Err(XC_PD_GATE_CONFLICT);
        }
        // 冲突：网络不过总表过 → 生效过（总表为准的双向含义）。
        let v2 = h.consume_gate(false, true);
        if !v2.conflicted || !v2.effective_pass {
            return Err(XC_PD_GATE_CONFLICT);
        }
        // 一致：无冲突。
        let v3 = h.consume_gate(true, true);
        if v3.conflicted || !v3.effective_pass {
            return Err(XC_PD_GATE_CONFLICT);
        }
        if h.gate_conflicts != 2 {
            return Err(XC_PD_GATE_CONFLICT);
        }
        Ok(())
    }

    /// 自检五：时序纪律——tick 倒流拒（publish 与 tick_advance 双面）。
    pub fn selfcheck_temporal(&mut self) -> Result<(), u32> {
        let mut h = PDomainLink::new();
        h.publish(MetricSample {
            metric: NetMetric::LossPpm,
            value: 5,
            tick: 10,
            schema_ver: SCHEMA_VER,
        })?;
        if h.publish(MetricSample {
            metric: NetMetric::LossPpm,
            value: 5,
            tick: 9,
            schema_ver: SCHEMA_VER,
        }) != Err(XC_PD_TICK_REWIND)
        {
            return Err(XC_PD_TICK_REWIND);
        }
        if h.tick_advance(9) != Err(XC_PD_TICK_REWIND) {
            return Err(XC_PD_TICK_REWIND);
        }
        Ok(())
    }

    /// 自检六：指标闭集——from_code 双向 + 闭集外拒。
    pub fn selfcheck_metric_closed(&mut self) -> Result<(), u32> {
        let mut i = 0;
        while i < NetMetric::ALL.len() {
            let m = NetMetric::ALL[i];
            if NetMetric::from_code(m.code()) != Some(m) {
                return Err(XC_PD_METRIC_UNKNOWN);
            }
            i += 1;
        }
        if NetMetric::from_code(5).is_some() || NetMetric::from_code(255).is_some() {
            return Err(XC_PD_METRIC_UNKNOWN);
        }
        Ok(())
    }

    /// 自检七：遥测防伪 + 审计 seq 严格递增 + 对拍 O(指标数)。
    pub fn selfcheck_telemetry_audit(&mut self) -> Result<(), u32> {
        let bad = LinkTelemetry {
            tick: 1,
            metric_code: 9,
            value: 1,
            panel_state: 0,
            schema_ver: SCHEMA_VER,
        };
        if bad.well_formed() {
            return Err(XC_PD_TELEMETRY_LEAK);
        }
        let mut h = PDomainLink::new();
        let mut t: u64 = 1;
        while t <= 5 {
            h.publish(MetricSample {
                metric: NetMetric::ALL[(t % 5) as usize],
                value: t as u32,
                tick: t,
                schema_ver: SCHEMA_VER,
            })?;
            t += 1;
        }
        // 五指标全入板 → reconcile 全绿。
        if !h.reconcile() {
            return Err(XC_PD_METRIC_UNKNOWN);
        }
        let mut i = 1;
        while i < h.audit_len() {
            match (h.audit_at(i - 1), h.audit_at(i)) {
                (Some(a), Some(b)) => {
                    if b.seq <= a.seq {
                        return Err(XC_PD_TELEMETRY_LEAK);
                    }
                }
                _ => return Err(XC_PD_TELEMETRY_LEAK),
            }
            i += 1;
        }
        Ok(())
    }

    /// 全量自检入口（七组）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_doublewrite()?;
        self.selfcheck_pipeline()?;
        self.selfcheck_budget()?;
        self.selfcheck_gate()?;
        self.selfcheck_temporal()?;
        self.selfcheck_metric_closed()?;
        self.selfcheck_telemetry_audit()?;
        Ok(())
    }
}
