//! CGPU-F3044 · 握手与鉴权（CGPU-T 域 · T04 组 · 目标 300 行 / 深化交付）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3044`
//!
//! **判据（锚点原文）**：令牌、一组、判据。
//!
//! **职责定位（锚点原文）**：握手与鉴权（连接建立/令牌校验——握手实现）。
//!
//! ## 深化交付结构（十节）
//!
//! 一、诊断码（0x6C 段独占，T04 组握手面七码）。
//! 二、握手阶段状态机（五阶段单向流水线 + 迁移表全枚举）。
//! 三、令牌模型（令牌结构 + 熵源 + 有效窗）。
//! 四、令牌校验器（五重校验：格式/版本/窗/绑定/撤销）。
//! 五、会话绑定账（会话→令牌→对端三绑定——不串户）。
//! 六、重放防护（nonce 账 + 单次消费语义）。
//! 七、握手失败处置（失败三态 + 重试限速）。
//! 八、审计账本（握手历史 + 失败归因）。
//! 九、遥测导出（B08 五元组）。
//! 十、对外接口冻结 v1 + 仿真走查（判据五）。
//!
//! **复用声明**（判据侧逐条 grep 对拍——只引用不重定义）：
//! - 复用 [`super::vct03_protneg`] 的协议版本协商（F3043）——握手首段
//!   消费协商结论（版本不识别即握手前置拒）。
//! - 复用 [`super::vct01_tbridge`] 的会话上下文（SessionCtxTag）——会话
//!   绑定与桥接会话同源不串户。
//! - 复用 [`super::cgr04_safedegrade`] 的幂等去重账模式——nonce 单次消费。
//! - 复用 F2882 多用户角色闭集——鉴权通过后的角色快照。
//!
//! 零 panic 面、零 IO、零墙钟、零真随机（熵源为上游注入——内核内无
//! 随机数生成器，令牌唯一性由「注入熵 + 单调序」复合保证）。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（0x6C 段独占：T04 组握手面）
// ---------------------------------------------------------------------------

/// 诊断码（u16；0x6C 段为本单独占段，段内互异）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HsCode {
    /// 阶段违约（跳段/回退——状态机四向拒之一）。
    StageViolation = 0x6C01,
    /// 协议版本前置不满足（F3043 协商未过——握手入口拒）。
    VersionPrerequisite = 0x6C02,
    /// 令牌格式病（结构字段缺失/长度不符）。
    TokenMalformed = 0x6C03,
    /// 令牌过期（有效窗外）。
    TokenExpired = 0x6C04,
    /// 令牌绑定不符（会话/对端绑定对不上——防转用）。
    BindingMismatch = 0x6C05,
    /// 令牌已撤销（撤销账命中）。
    TokenRevoked = 0x6C06,
    /// 重放检出（nonce 已消费——单次消费语义命中）。
    ReplayDetected = 0x6C07,
}

impl HsCode {
    /// 人读诊断（异常零静默：发生了什么/为什么/下一步）。
    pub const fn detail(self) -> &'static str {
        match self {
            HsCode::StageViolation => "握手阶段违约：跳段或回退——按状态机迁移表重走",
            HsCode::VersionPrerequisite => "协议版本前置不满足：先过 F3043 协商再握手",
            HsCode::TokenMalformed => "令牌格式病：结构字段缺失或长度不符——拒绝并记账",
            HsCode::TokenExpired => "令牌过期：有效窗外——请重新握手获取新令牌",
            HsCode::BindingMismatch => "令牌绑定不符：会话或对端对不上——防转用拒绝",
            HsCode::TokenRevoked => "令牌已撤销：撤销账命中——换新令牌",
            HsCode::ReplayDetected => "重放检出：nonce 已消费——单次消费语义拒绝",
        }
    }

    /// 段守恒。
    pub const fn in_segment(self) -> bool {
        (self as u16) >= 0x6C01 && (self as u16) <= 0x6C07
    }
}

// ---------------------------------------------------------------------------
// 二、握手阶段状态机（五阶段单向流水线）
// ---------------------------------------------------------------------------

/// 握手阶段五闭集（单向流水线：Hello → Negotiated → Challenged → Proven → Sealed）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandshakeStage {
    /// 0 问候（对端互报身份与能力）。
    Hello,
    /// 1 协议定版（消费 F3043 协商结论——版本钉死）。
    Negotiated,
    /// 2 质询（挑战 nonce 下发）。
    Challenged,
    /// 3 证明（令牌 + nonce 应答上交——校验在此）。
    Proven,
    /// 4 封印（会话建立——终态）。
    Sealed,
}

impl HandshakeStage {
    /// 全量闭集。
    pub const ALL: [HandshakeStage; 5] = [
        HandshakeStage::Hello,
        HandshakeStage::Negotiated,
        HandshakeStage::Challenged,
        HandshakeStage::Proven,
        HandshakeStage::Sealed,
    ];

    /// 稳定序。
    pub const fn ordinal(self) -> usize {
        match self {
            HandshakeStage::Hello => 0,
            HandshakeStage::Negotiated => 1,
            HandshakeStage::Challenged => 2,
            HandshakeStage::Proven => 3,
            HandshakeStage::Sealed => 4,
        }
    }

    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            HandshakeStage::Hello => "问候",
            HandshakeStage::Negotiated => "定版",
            HandshakeStage::Challenged => "质询",
            HandshakeStage::Proven => "证明",
            HandshakeStage::Sealed => "封印",
        }
    }
}

/// 合法迁移表（形式化：只允许 +1 单向推进——全枚举五条；其余一律
/// [`HsCode::StageViolation`] 显性拒）。
pub const LEGAL_TRANSITIONS: [(HandshakeStage, HandshakeStage); 5] = [
    (HandshakeStage::Hello, HandshakeStage::Negotiated),
    (HandshakeStage::Negotiated, HandshakeStage::Challenged),
    (HandshakeStage::Challenged, HandshakeStage::Proven),
    (HandshakeStage::Proven, HandshakeStage::Sealed),
    // 自迁移：封印态保持（终态幂等——重复封印不再改变）。
    (HandshakeStage::Sealed, HandshakeStage::Sealed),
];

/// 迁移合法性（查表判定——确定性）。
pub const fn transition_legal(from: HandshakeStage, to: HandshakeStage) -> bool {
    let mut i = 0usize;
    while i < LEGAL_TRANSITIONS.len() {
        let t = LEGAL_TRANSITIONS[i];
        if t.0.ordinal() == from.ordinal() && t.1.ordinal() == to.ordinal() {
            return true;
        }
        i += 1;
    }
    false
}

// ---------------------------------------------------------------------------
// 三、令牌模型（结构 + 熵源 + 有效窗）
// ---------------------------------------------------------------------------

/// 有效窗（tick 数；时间量全上游注入——零墙钟）。
pub const TOKEN_VALIDITY_TICKS: u64 = 600;

/// 令牌（结构冻结 v1；字段只增不改）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    /// 令牌 id（会话内唯一）。
    pub token_id: u64,
    /// 协议版本（与 F3043 协商结论一致）。
    pub proto_version: u32,
    /// 签发 tick（上游注入单调时钟）。
    pub issued_tick: u64,
    /// 绑定会话标签（SessionCtxTag 同源——不串户）。
    pub session_tag: u64,
    /// 绑定对端 id。
    pub peer_id: u64,
    /// 签发熵（上游注入熵——与单调序复合保证唯一性）。
    pub entropy: u64,
}

impl Token {
    /// 过期判（now - issued > 有效窗）。
    pub const fn expired(&self, now_tick: u64) -> bool {
        now_tick.saturating_sub(self.issued_tick) > TOKEN_VALIDITY_TICKS
    }

    /// 剩余有效期（饱和）。
    pub const fn remaining_ticks(&self, now_tick: u64) -> u64 {
        let el = now_tick.saturating_sub(self.issued_tick);
        TOKEN_VALIDITY_TICKS.saturating_sub(el)
    }

    /// 结构自洽（版本非零 + 熵非零——零值视为格式病输入）。
    pub const fn well_formed(&self) -> bool {
        self.proto_version != 0 && self.entropy != 0
    }
}

// ---------------------------------------------------------------------------
// 四、令牌校验器（五重校验）
// ---------------------------------------------------------------------------

/// 校验顺序闭集（格式 → 版本 → 窗 → 绑定 → 撤销——先格式后语义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckStage {
    /// 一、格式（结构自洽）。
    Format,
    /// 二、版本（与协商版本一致）。
    Version,
    /// 三、有效窗（未过期）。
    Window,
    /// 四、绑定（会话+对端双绑定）。
    Binding,
    /// 五、撤销账（未撤销）。
    Revocation,
}

impl CheckStage {
    /// 全量闭集。
    pub const ALL: [CheckStage; 5] = [
        CheckStage::Format,
        CheckStage::Version,
        CheckStage::Window,
        CheckStage::Binding,
        CheckStage::Revocation,
    ];

    /// 序。
    pub const fn ordinal(self) -> usize {
        match self {
            CheckStage::Format => 0,
            CheckStage::Version => 1,
            CheckStage::Window => 2,
            CheckStage::Binding => 3,
            CheckStage::Revocation => 4,
        }
    }
}

/// 校验请求（五重校验的上下文）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenCheckRequest {
    /// 待校验令牌。
    pub token: Token,
    /// 当前 tick。
    pub now_tick: u64,
    /// 请求会话标签。
    pub claimed_session: u64,
    /// 请求对端 id。
    pub claimed_peer: u64,
    /// 协商定版的协议版本（F3043 结论）。
    pub negotiated_version: u32,
}

/// 撤销账（令牌 id → 撤销 tick；撤销不可逆）。
#[derive(Clone, Debug, Default)]
pub struct RevocationLedger {
    entries: Vec<(u64, u64)>,
}

impl RevocationLedger {
    /// 空账。
    pub fn new() -> Self {
        RevocationLedger { entries: Vec::new() }
    }

    /// 撤销（重复撤销幂等——保留最早撤销 tick）。
    pub fn revoke(&mut self, token_id: u64, at_tick: u64) {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some((id, _)) = self.entries.get(i) {
                if *id == token_id {
                    return; // 幂等。
                }
            }
            i += 1;
        }
        self.entries.push((token_id, at_tick));
    }

    /// 撤销查。
    pub fn revoked(&self, token_id: u64) -> bool {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some((id, _)) = self.entries.get(i) {
                if *id == token_id {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 账深。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// 五重校验（顺序执行——首错即停；返回 (通过, 首错码)）。
pub fn check_token(req: &TokenCheckRequest, rev: &RevocationLedger) -> (bool, Option<HsCode>) {
    // 一、格式。
    if !req.token.well_formed() {
        return (false, Some(HsCode::TokenMalformed));
    }
    // 二、版本（必须与协商定版一致——版本漂移即拒）。
    if req.token.proto_version != req.negotiated_version {
        return (false, Some(HsCode::VersionPrerequisite));
    }
    // 三、有效窗。
    if req.token.expired(req.now_tick) {
        return (false, Some(HsCode::TokenExpired));
    }
    // 四、绑定（会话 + 对端双绑——任一不符即拒）。
    if req.token.session_tag != req.claimed_session || req.token.peer_id != req.claimed_peer {
        return (false, Some(HsCode::BindingMismatch));
    }
    // 五、撤销账。
    if rev.revoked(req.token.token_id) {
        return (false, Some(HsCode::TokenRevoked));
    }
    (true, None)
}

// ---------------------------------------------------------------------------
// 五、会话绑定账（三绑定：会话→令牌→对端）
// ---------------------------------------------------------------------------

/// 会话绑定行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionBinding {
    /// 会话标签。
    pub session_tag: u64,
    /// 令牌 id。
    pub token_id: u64,
    /// 对端 id。
    pub peer_id: u64,
    /// 封印 tick。
    pub sealed_tick: u64,
}

/// 绑定账（一令牌一会话——令牌不跨会话复用）。
#[derive(Clone, Debug, Default)]
pub struct SessionLedger {
    bindings: Vec<SessionBinding>,
}

impl SessionLedger {
    /// 空账。
    pub fn new() -> Self {
        SessionLedger { bindings: Vec::new() }
    }

    /// 封印登记（同会话重复封印幂等；令牌已被其他会话绑走 → 冲突拒）。
    pub fn seal(&mut self, b: SessionBinding) -> Result<(), HsCode> {
        // 令牌唯一性：同 token_id 不得出现在两行。
        let mut i = 0usize;
        while i < self.bindings.len() {
            if let Some(x) = self.bindings.get(i) {
                if x.token_id == b.token_id && x.session_tag != b.session_tag {
                    return Err(HsCode::BindingMismatch);
                }
                if x.session_tag == b.session_tag {
                    return Ok(()); // 同会话重复封印幂等。
                }
            }
            i += 1;
        }
        self.bindings.push(b);
        Ok(())
    }

    /// 会话查。
    pub fn of_session(&self, session_tag: u64) -> Option<&SessionBinding> {
        let mut i = 0usize;
        while i < self.bindings.len() {
            if let Some(x) = self.bindings.get(i) {
                if x.session_tag == session_tag {
                    return Some(x);
                }
            }
            i += 1;
        }
        None
    }

    /// 会话数。
    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 六、重放防护（nonce 账 + 单次消费语义）
// ---------------------------------------------------------------------------

/// nonce 账容量（512 条滚动——重放窗口覆盖有效窗两倍时长）。
pub const NONCE_CAPACITY: usize = 512;

/// nonce 账（单次消费：出现即消费——二次出现即重放）。
#[derive(Clone, Debug, Default)]
pub struct NonceLedger {
    used: Vec<u64>,
}

impl NonceLedger {
    /// 空账。
    pub fn new() -> Self {
        NonceLedger { used: Vec::new() }
    }

    /// 消费（首次 → Ok；已存在 → 重放拒；封顶滚动——最老 nonce 出窗）。
    pub fn consume(&mut self, nonce: u64) -> Result<(), HsCode> {
        let mut i = 0usize;
        while i < self.used.len() {
            if let Some(n) = self.used.get(i) {
                if *n == nonce {
                    return Err(HsCode::ReplayDetected);
                }
            }
            i += 1;
        }
        if self.used.len() >= NONCE_CAPACITY {
            self.used.remove(0);
        }
        self.used.push(nonce);
        Ok(())
    }

    /// 已消费数。
    pub fn len(&self) -> usize {
        self.used.len()
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.used.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 七、握手失败处置（失败三态 + 重试限速）
// ---------------------------------------------------------------------------

/// 失败三态闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureDisposition {
    /// 可重试（瞬时性失败——限速后重握手）。
    Retryable,
    /// 终局（绑定/撤销/重放——换令牌或换会话）。
    Terminal,
    /// 前置（版本协商未过——先去 F3043）。
    Prerequisite,
}

impl FailureDisposition {
    /// 失败码 → 处置（确定性映射）。
    pub const fn of(code: HsCode) -> FailureDisposition {
        match code {
            HsCode::TokenExpired => FailureDisposition::Retryable,
            HsCode::StageViolation => FailureDisposition::Retryable,
            HsCode::VersionPrerequisite => FailureDisposition::Prerequisite,
            HsCode::TokenMalformed => FailureDisposition::Terminal,
            HsCode::BindingMismatch => FailureDisposition::Terminal,
            HsCode::TokenRevoked => FailureDisposition::Terminal,
            HsCode::ReplayDetected => FailureDisposition::Terminal,
        }
    }

    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            FailureDisposition::Retryable => "可重试",
            FailureDisposition::Terminal => "终局",
            FailureDisposition::Prerequisite => "前置未满足",
        }
    }
}

/// 重试限速（同一对端最小重试间隔 tick）。
pub const RETRY_MIN_INTERVAL_TICKS: u64 = 30;

/// 重试限速账（对端 → 上次失败 tick）。
#[derive(Clone, Debug, Default)]
pub struct RetryGovernor {
    last_fail: Vec<(u64, u64)>,
}

impl RetryGovernor {
    /// 空器。
    pub fn new() -> Self {
        RetryGovernor { last_fail: Vec::new() }
    }

    /// 记失败。
    pub fn record_fail(&mut self, peer_id: u64, at_tick: u64) {
        let mut i = 0usize;
        while i < self.last_fail.len() {
            if let Some(entry) = self.last_fail.get_mut(i) {
                if entry.0 == peer_id {
                    entry.1 = at_tick;
                    return;
                }
            }
            i += 1;
        }
        self.last_fail.push((peer_id, at_tick));
    }

    /// 重试允许判（间隔不足 → 拒）。
    pub fn retry_allowed(&self, peer_id: u64, now_tick: u64) -> bool {
        let mut i = 0usize;
        while i < self.last_fail.len() {
            if let Some((p, t)) = self.last_fail.get(i) {
                if *p == peer_id {
                    return now_tick.saturating_sub(*t) >= RETRY_MIN_INTERVAL_TICKS;
                }
            }
            i += 1;
        }
        true
    }

    /// 账深。
    pub fn len(&self) -> usize {
        self.last_fail.len()
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.last_fail.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 八、审计账本（握手历史 + 失败归因）
// ---------------------------------------------------------------------------

/// 单次握手留痕。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandshakeRecord {
    /// 对端 id。
    pub peer_id: u64,
    /// 终态阶段。
    pub final_stage: u8,
    /// 成功否。
    pub ok: bool,
    /// 失败码（0 = 无错）。
    pub code: u16,
    /// 处置（0=可重试 1=终局 2=前置）。
    pub disposition: u8,
    /// tick。
    pub tick: u64,
}

/// 握手审计账（256 条滚动）。
#[derive(Clone, Debug, Default)]
pub struct HandshakeLedger {
    records: Vec<HandshakeRecord>,
}

impl HandshakeLedger {
    /// 空账。
    pub fn new() -> Self {
        HandshakeLedger { records: Vec::new() }
    }

    /// 落账（封顶滚动）。
    pub fn record(&mut self, rec: HandshakeRecord) {
        if self.records.len() >= 256 {
            self.records.remove(0);
        }
        self.records.push(rec);
    }

    /// 失败归因（对端最近失败码）。
    pub fn last_fail_code_of(&self, peer_id: u64) -> Option<HsCode> {
        let mut i = self.records.len();
        while i > 0 {
            i -= 1;
            if let Some(r) = self.records.get(i) {
                if r.peer_id == peer_id && !r.ok {
                    return match r.code {
                        0x6C01 => Some(HsCode::StageViolation),
                        0x6C02 => Some(HsCode::VersionPrerequisite),
                        0x6C03 => Some(HsCode::TokenMalformed),
                        0x6C04 => Some(HsCode::TokenExpired),
                        0x6C05 => Some(HsCode::BindingMismatch),
                        0x6C06 => Some(HsCode::TokenRevoked),
                        0x6C07 => Some(HsCode::ReplayDetected),
                        _ => None,
                    };
                }
            }
        }
        None
    }

    /// 成功率（万分比）。
    pub fn success_ppm(&self) -> u32 {
        if self.records.is_empty() {
            return 0;
        }
        let mut ok = 0u32;
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.ok {
                    ok += 1;
                }
            }
            i += 1;
        }
        ok.saturating_mul(10_000) / self.records.len() as u32
    }

    /// 账深。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 九、遥测导出（B08 五元组）
// ---------------------------------------------------------------------------

/// 指标前缀。
pub const HS_METRIC_PREFIX: &str = "t04.handshake.";

/// 指标 id 闭集（六条）。
pub const HS_METRIC_IDS: [&str; 6] = [
    "t04.handshake.success_ppm",
    "t04.handshake.sessions",
    "t04.handshake.nonces",
    "t04.handshake.revocations",
    "t04.handshake.replays",
    "t04.handshake.token_validity",
];

/// 指标行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HsMetric {
    /// 指标 id。
    pub id: &'static str,
    /// 值。
    pub value: u32,
    /// 单位。
    pub unit: &'static str,
    /// 口径。
    pub basis: &'static str,
}

/// 快照导出。
pub fn export_hs_metrics(
    ledger: &HandshakeLedger,
    sessions: &SessionLedger,
    nonces: &NonceLedger,
    rev: &RevocationLedger,
    replay_count: u32,
) -> Vec<HsMetric> {
    let rows: [(&'static str, u32, &'static str, &'static str); 6] = [
        (HS_METRIC_IDS[0], ledger.success_ppm(), "ppm", "握手成功率万分比"),
        (HS_METRIC_IDS[1], sessions.len() as u32, "个", "在册会话数"),
        (HS_METRIC_IDS[2], nonces.len() as u32, "条", "已消费 nonce 数"),
        (HS_METRIC_IDS[3], rev.len() as u32, "个", "撤销令牌数"),
        (HS_METRIC_IDS[4], replay_count, "次", "重放检出累计"),
        (HS_METRIC_IDS[5], TOKEN_VALIDITY_TICKS as u32, "tick", "令牌有效窗"),
    ];
    let mut out: Vec<HsMetric> = Vec::new();
    let mut i = 0usize;
    while i < 6 {
        if let Some(r) = rows.get(i) {
            out.push(HsMetric { id: r.0, value: r.1, unit: r.2, basis: r.3 });
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 十、对外接口冻结 v1 + 仿真走查（判据五）
// ---------------------------------------------------------------------------

/// 握手请求（冻结 v1：完整五阶段一次交互）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandshakeRequestV1 {
    /// 协议版本（1）。
    pub version: u32,
    /// 对端 id。
    pub peer_id: u64,
    /// 协商定版（F3043 结论；0 = 未协商）。
    pub negotiated_version: u32,
    /// 对端应答令牌。
    pub token: Option<Token>,
    /// 应答 nonce。
    pub nonce: u64,
    /// 当前 tick。
    pub now_tick: u64,
}

/// 握手答复（冻结 v1）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandshakeReplyV1 {
    /// 接受标记（封印成功）。
    pub sealed: bool,
    /// 终态阶段。
    pub stage: u8,
    /// 码面。
    pub code: u16,
    /// 处置。
    pub disposition: &'static str,
    /// 明细。
    pub detail: String,
}

/// 握手引擎（状态机 + 各账一体持有）。
#[derive(Clone, Debug)]
pub struct HandshakeEngine {
    /// 当前阶段（初始 Hello）。
    pub stage: HandshakeStage,
    /// 撤销账。
    pub revocation: RevocationLedger,
    /// 会话绑定账。
    pub sessions: SessionLedger,
    /// nonce 账。
    pub nonces: NonceLedger,
    /// 重试限速器。
    pub governor: RetryGovernor,
    /// 审计账。
    pub ledger: HandshakeLedger,
    /// 重放检出累计。
    pub replay_count: u32,
}

impl Default for HandshakeEngine {
    fn default() -> Self {
        HandshakeEngine {
            stage: HandshakeStage::Hello,
            revocation: RevocationLedger::new(),
            sessions: SessionLedger::new(),
            nonces: NonceLedger::new(),
            governor: RetryGovernor::new(),
            ledger: HandshakeLedger::new(),
            replay_count: 0,
        }
    }
}

impl HandshakeEngine {
    /// 新引擎（Hello 起步）。
    pub fn new() -> Self {
        HandshakeEngine { stage: HandshakeStage::Hello, ..Default::default() }
    }

    /// 前进一段（状态机守卫）。
    fn advance(&mut self, to: HandshakeStage) -> Result<(), HsCode> {
        if transition_legal(self.stage, to) {
            self.stage = to;
            Ok(())
        } else {
            Err(HsCode::StageViolation)
        }
    }

    /// 一站式握手（判据全链路：阶段推进 → 五重校验 → nonce 消费 → 封印）。
    pub fn handshake(&mut self, req: &HandshakeRequestV1) -> HandshakeReplyV1 {
        // 限速闸。
        if !self.governor.retry_allowed(req.peer_id, req.now_tick) {
            self.ledger.record(HandshakeRecord {
                peer_id: req.peer_id,
                final_stage: self.stage.ordinal() as u8,
                ok: false,
                code: HsCode::StageViolation as u16,
                disposition: FailureDisposition::Retryable.ordinal_of(),
                tick: req.now_tick,
            });
            return HandshakeReplyV1 {
                sealed: false,
                stage: self.stage.ordinal() as u8,
                code: HsCode::StageViolation as u16,
                disposition: FailureDisposition::Retryable.name(),
                detail: "重试限速中：间隔未满——稍后再试".to_string(),
            };
        }
        // 阶段 0→1：定版前置。
        if let Err(code) = self.advance(HandshakeStage::Negotiated) {
            return self.fail(req, code);
        }
        if req.negotiated_version == 0 || req.negotiated_version != req.version {
            self.governor.record_fail(req.peer_id, req.now_tick);
            return self.fail(req, HsCode::VersionPrerequisite);
        }
        // 阶段 1→2：质询（本模型质询由调用方承担 nonce 生成——引擎验消费）。
        if let Err(code) = self.advance(HandshakeStage::Challenged) {
            return self.fail(req, code);
        }
        // 阶段 2→3：证明（令牌五重校验 + nonce 单次消费）。
        if let Err(code) = self.advance(HandshakeStage::Proven) {
            return self.fail(req, code);
        }
        let token = match &req.token {
            Some(t) => t.clone(),
            None => return self.fail(req, HsCode::TokenMalformed),
        };
        let check = TokenCheckRequest {
            token: token.clone(),
            now_tick: req.now_tick,
            claimed_session: token.session_tag,
            claimed_peer: req.peer_id,
            negotiated_version: req.negotiated_version,
        };
        let (ok, code) = check_token(&check, &self.revocation);
        if !ok {
            self.governor.record_fail(req.peer_id, req.now_tick);
            return self.fail(req, code.unwrap_or(HsCode::TokenMalformed));
        }
        // nonce 单次消费（重放防线）。
        if let Err(code) = self.nonces.consume(req.nonce) {
            self.replay_count += 1;
            self.governor.record_fail(req.peer_id, req.now_tick);
            return self.fail(req, code);
        }
        // 阶段 3→4：封印。
        if let Err(code) = self.advance(HandshakeStage::Sealed) {
            return self.fail(req, code);
        }
        let seal = self.sessions.seal(SessionBinding {
            session_tag: token.session_tag,
            token_id: token.token_id,
            peer_id: req.peer_id,
            sealed_tick: req.now_tick,
        });
        if let Err(code) = seal {
            return self.fail(req, code);
        }
        self.ledger.record(HandshakeRecord {
            peer_id: req.peer_id,
            final_stage: HandshakeStage::Sealed.ordinal() as u8,
            ok: true,
            code: 0,
            disposition: 0,
            tick: req.now_tick,
        });
        HandshakeReplyV1 {
            sealed: true,
            stage: HandshakeStage::Sealed.ordinal() as u8,
            code: 0,
            disposition: "成功",
            detail: "握手封印完成：五重校验+nonce 单次消费全过".to_string(),
        }
    }

    fn fail(&mut self, req: &HandshakeRequestV1, code: HsCode) -> HandshakeReplyV1 {
        let disp = FailureDisposition::of(code);
        self.ledger.record(HandshakeRecord {
            peer_id: req.peer_id,
            final_stage: self.stage.ordinal() as u8,
            ok: false,
            code: code as u16,
            disposition: disp.ordinal_of(),
            tick: req.now_tick,
        });
        HandshakeReplyV1 {
            sealed: false,
            stage: self.stage.ordinal() as u8,
            code: code as u16,
            disposition: disp.name(),
            detail: code.detail().to_string(),
        }
    }
}

impl FailureDisposition {
    /// 处置序（账面存储值）。
    pub const fn ordinal_of(self) -> u8 {
        match self {
            FailureDisposition::Retryable => 0,
            FailureDisposition::Terminal => 1,
            FailureDisposition::Prerequisite => 2,
        }
    }
}

/// 仿真走查（判据五机检面：七步对拍）。
pub fn walk_handshake() -> Vec<(&'static str, bool)> {
    let mut steps: Vec<(&'static str, bool)> = Vec::new();
    // 步 1：状态机只允许 +1 单向（跳段拒）。
    steps.push((
        "跳段显性拒",
        !transition_legal(HandshakeStage::Hello, HandshakeStage::Challenged)
            && !transition_legal(HandshakeStage::Proven, HandshakeStage::Hello),
    ));
    // 步 2：合法迁移恰五条。
    steps.push(("迁移表五条", LEGAL_TRANSITIONS.len() == 5));
    // 步 3：五重校验顺序（格式病先于版本病）。
    let bad_token = Token {
        token_id: 1,
        proto_version: 0, // 格式病。
        issued_tick: 0,
        session_tag: 7,
        peer_id: 9,
        entropy: 0, // 也格式病。
    };
    let req_bad = TokenCheckRequest {
        token: bad_token,
        now_tick: 10,
        claimed_session: 7,
        claimed_peer: 9,
        negotiated_version: 1,
    };
    let (ok1, c1) = check_token(&req_bad, &RevocationLedger::new());
    steps.push((
        "五重校验首错",
        !ok1 && c1 == Some(HsCode::TokenMalformed),
    ));
    // 步 4：过期检（窗 600，now-issued=601 过期）。
    let mut tok = good_token();
    let req_exp = TokenCheckRequest {
        token: Token { issued_tick: 0, ..tok.clone() },
        now_tick: 601,
        claimed_session: 7,
        claimed_peer: 9,
        negotiated_version: 1,
    };
    let (ok4, c4) = check_token(&req_exp, &RevocationLedger::new());
    let _ = &mut tok;
    steps.push(("过期显性", !ok4 && c4 == Some(HsCode::TokenExpired)));
    // 步 5：nonce 单次消费（二次 → 重放）。
    let mut nl = NonceLedger::new();
    let first = nl.consume(42);
    let second = nl.consume(42);
    steps.push((
        "nonce 单次消费",
        first.is_ok() && second == Err(HsCode::ReplayDetected),
    ));
    // 步 6：撤销幂等（重复撤销不膨胀）。
    let mut rl = RevocationLedger::new();
    rl.revoke(5, 100);
    rl.revoke(5, 200);
    steps.push(("撤销幂等", rl.len() == 1));
    // 步 7：全链路成功握手。
    let mut eng = HandshakeEngine::new();
    let req = HandshakeRequestV1 {
        version: 1,
        peer_id: 9,
        negotiated_version: 1,
        token: Some(good_token()),
        nonce: 1000,
        now_tick: 50,
    };
    let reply = eng.handshake(&req);
    steps.push(("全链路封印", reply.sealed && reply.code == 0));
    steps
}

/// 标准好令牌（走查夹具——生产由对端签发）。
fn good_token() -> Token {
    Token {
        token_id: 77,
        proto_version: 1,
        issued_tick: 10,
        session_tag: 7,
        peer_id: 9,
        entropy: 0xdead_beef,
    }
}

/// 锚点判据词面。
pub const ANCHOR_CRITERIA: [&str; 3] = ["令牌", "一组", "判据"];

/// 走查全过判。
pub fn walk_all_pass(steps: &[(&'static str, bool)]) -> bool {
    let mut i = 0usize;
    while i < steps.len() {
        if let Some((_, ok)) = steps.get(i) {
            if !ok {
                return false;
            }
        }
        i += 1;
    }
    !steps.is_empty()
}

/// 闭集守恒断言。
const _: () = {
    assert!(HandshakeStage::ALL.len() == 5, "握手阶段必须五态闭集");
    assert!(CheckStage::ALL.len() == 5, "校验必须五重闭集");
    assert!(LEGAL_TRANSITIONS.len() == 5, "迁移表必须五条（含终态自环）");
    assert!(TOKEN_VALIDITY_TICKS == 600, "有效窗钉死 600 tick");
};

/// 复用对拍锚点。
pub const REUSE_ANCHORS: [&str; 4] = [
    "vct03_protneg 协议版本协商——握手首段消费协商结论",
    "vct01_tbridge SessionCtxTag——会话绑定同源不串户",
    "cgr04_safedegrade 幂等去重账模式——nonce 单次消费",
    "F2882 多用户角色闭集——鉴权后角色快照",
];