//! VE-F5415 · 网络扩展点（vef67_netext）
//!
//! 网络体系**自身可生长**的两条注册接口——**自定义传输适配注册**
//! （冻结接口+性能预算审批：接口版本冻结制，预算超总账拦截）与
//! **自定义复制策略注册**（声明式策略：同步字段白名单+频率+条件
//! 三声明——AA02 消费面产出 PolicyGrant 凭证）；**扩展走门禁同审**
//! （传输与策略同过一扇审查门：接口版本✓/预算✓/声明律✓/沙箱令
//! 牌✓ 四查——扩展产物同受审查是域本色不是额外恩惠）。**沙箱隔
//! 离**：注册持 RegistryToken 令牌制（签发-验签两步，伪令牌/零令
//! 牌一律拒——F0847 生产隔离闸同型）。审查全程 AuditRow 台账
//! （谁注册/预算多少/审查结论——审查不留痕等于没审）。
//!
//! 数据结构（锚点原文）：传输注册接口；策略注册接口。
//!
//! 错误路径与降级矩阵：**越权注册→拒绝**（无令牌/伪令牌 E_NE_NO_
//! TOKEN——沙箱面）；**传输超支→预算拦截**（累计预算和+新预算>
//! 总账 E_NE_BUDGET_EXCEED——O(1) 查账）；**策略违律→驳回**
//! （Always+超频/白名单外字段 E_NE_POLICY_VIOLATION——声明式不
//! 合律就驳回）。性能：注册 O(1)；拦截 O(1)；驳回 O(1)（全部查
//! 表不扫描）。跨批对接点：F4878/F5018 口径上游；F5419 联调；
//! AA02 复制消费面（PolicyGrant）。
//! 无障碍：审查台账读屏可达（spoken）。隐私面：台账只记预算与
//! 结论不记内容。
//!
//! 诊断码 0x67 段五码互异：`E_NE_NO_TOKEN` / `E_NE_BUDGET_EXCEED`
//! / `E_NE_POLICY_VIOLATION` / `E_NE_DUP_NAME` / `E_NE_IF_VERSION`。

use alloc::vec;
use crate::checks::CheckSet;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 版本与常量（阈值独立声明，判据独立手算对拍）
// ---------------------------------------------------------------------------

/// 网络扩展点版本。
pub const NETEXT_VERSION: &str = "F67-netext-v1";
/// 传输接口冻结版本（版本拦截锚——不匹配即拒）。
pub const TRANSPORT_IF_VERSION: u32 = 3;
/// 网络总预算（bps——注册审批的上限账）。
pub const TOTAL_BUDGET_BPS: u64 = 1_000_000_000; // 1 Gbps
/// 复制策略频率上限（Hz——Always 律的频率红线）。
pub const REPL_MAX_HZ: u32 = 60;
/// 复制条件闭集大小（声明律用）。
pub const REPL_COND_COUNT: usize = 3;

// ---------------------------------------------------------------------------
// 诊断码与原因文案（0x67 段五码互异；reason 全量显性）
// ---------------------------------------------------------------------------

/// 越权注册：无令牌或伪令牌（沙箱隔离面）。
pub const E_NE_NO_TOKEN: u16 = 0x6701;
/// 预算拦截：累计预算和+新预算超总账。
pub const E_NE_BUDGET_EXCEED: u16 = 0x6702;
/// 策略违律：声明不合复制律（Always 超频/白名单外字段）。
pub const E_NE_POLICY_VIOLATION: u16 = 0x6703;
/// 重名注册：同名传输已注册。
pub const E_NE_DUP_NAME: u16 = 0x6704;
/// 接口版本拦截：注册接口版本与冻结版不符。
pub const E_NE_IF_VERSION: u16 = 0x6705;

/// 诊断码 → 原因文案（全量显性，缺省分支兜底）。
pub fn reason_of(code: u16) -> &'static str {
    match code {
        E_NE_NO_TOKEN => "越权注册：无令牌或伪令牌，沙箱隔离面拒绝",
        E_NE_BUDGET_EXCEED => "预算拦截：累计预算和加新预算超总账",
        E_NE_POLICY_VIOLATION => "策略违律：声明不合复制律，驳回",
        E_NE_DUP_NAME => "重名注册：同名传输已注册",
        E_NE_IF_VERSION => "接口版本拦截：注册接口版本与冻结版不符",
        _ => "未知诊断码",
    }
}

/// 立案簿（0x67 段事件账——有界不静默）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagNote {
    pub code: u16,
    pub subject: String,
    pub detail: String,
}

#[derive(Debug)]
pub struct DiagBag {
    notes: Vec<DiagNote>,
    cap: usize,
    dropped: usize,
}

impl DiagBag {
    pub fn new() -> DiagBag {
        DiagBag { notes: Vec::new(), cap: 64, dropped: 0 }
    }

    pub fn push(&mut self, code: u16, subject: &str, detail: &str) {
        if self.notes.len() >= self.cap {
            self.notes.remove(0);
            self.dropped += 1;
        }
        self.notes.push(DiagNote {
            code,
            subject: subject.to_string(),
            detail: detail.to_string(),
        });
    }

    pub fn len(&self) -> usize {
        self.notes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    pub fn count_of(&self, code: u16) -> usize {
        self.notes.iter().filter(|n| n.code == code).count()
    }

    pub fn has(&self, code: u16) -> bool {
        self.count_of(code) > 0
    }

    pub fn spoken(&self) -> String {
        if self.notes.is_empty() {
            return "诊断簿空".to_string();
        }
        let mut out = format!("诊断簿共{}条", self.notes.len());
        for n in self.notes.iter() {
            out.push('；');
            out.push_str(reason_of(n.code));
            out.push('：');
            out.push_str(&n.subject);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 沙箱令牌（签发-验签两步——F0847 生产隔离闸同型）
// ---------------------------------------------------------------------------

/// 注册令牌（签发方持有的随机基数——验签比对）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistryToken {
    pub holder: u32,
    pub secret: u64,
}

/// 令牌签发器（登记持有人与密钥——验签 O(1) 查表）。
#[derive(Debug, Default)]
pub struct TokenIssuer {
    issued: BTreeMap<u32, u64>,
    pub granted: u32,
    pub rejected: core::cell::Cell<u32>,
}

impl TokenIssuer {
    pub fn new() -> TokenIssuer {
        TokenIssuer::default()
    }

    /// 签发（持有人首次登记——重复签发覆盖旧密钥）。
    pub fn grant(&mut self, holder: u32, secret: u64) -> RegistryToken {
        self.issued.insert(holder, secret);
        self.granted += 1;
        RegistryToken { holder, secret }
    }

    /// 验签（O(1) 查表——持有人不存在或密钥不符即伪令牌）。
    pub fn verify(&self, t: &RegistryToken) -> bool {
        match self.issued.get(&t.holder) {
            Some(s) if *s == t.secret => true,
            _ => {
                let cur = self.rejected.get();
                self.rejected.set(cur + 1);
                false
            }
        }
    }

    /// 已签发人数。
    pub fn holders(&self) -> usize {
        self.issued.len()
    }
}

// ---------------------------------------------------------------------------
// 传输注册接口（冻结面——版本拦截+预算审批）
// ---------------------------------------------------------------------------

/// 传输类别闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportKind {
    /// 可靠流（TCP 类）。
    Reliable,
    /// 不可靠数据报（UDP 类）。
    Unreliable,
}

impl TransportKind {
    pub fn zh(self) -> &'static str {
        match self {
            TransportKind::Reliable => "可靠流",
            TransportKind::Unreliable => "不可靠数据报",
        }
    }
}

/// 传输适配契约（冻结接口——字段闭集声明制）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransportContract {
    /// 接口版本（冻结拦截锚——与 TRANSPORT_IF_VERSION 比对）。
    pub if_version: u32,
    /// 注册名（唯一键）。
    pub name: &'static str,
    /// 类别。
    pub kind: TransportKind,
    /// 会话预算（bps）。
    pub session_budget_bps: u64,
    /// 每帧预算（bps）。
    pub per_frame_budget_bps: u64,
}

/// 传输注册回执。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransportGrant {
    pub reg_id: u32,
    pub name: &'static str,
    pub budget_bps: u64,
}

/// 审查台账单条（审查不留痕等于没审）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditRow {
    pub reg_id: u32,
    pub who: u32,
    pub kind_zh: &'static str,
    pub budget_bps: u64,
    /// 审查结论：true=通过。
    pub passed: bool,
}

// ---------------------------------------------------------------------------
// 复制策略注册接口（声明式——AA02 消费）
// ---------------------------------------------------------------------------

/// 复制条件闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplCond {
    /// 每帧同步。
    Always,
    /// 变化才同步。
    OnChange,
    /// 固定节拍。
    OnTick,
}

impl ReplCond {
    pub const COUNT: usize = REPL_COND_COUNT;

    pub fn all() -> [ReplCond; ReplCond::COUNT] {
        [ReplCond::Always, ReplCond::OnChange, ReplCond::OnTick]
    }

    pub fn zh(self) -> &'static str {
        match self {
            ReplCond::Always => "每帧",
            ReplCond::OnChange => "变化时",
            ReplCond::OnTick => "节拍",
        }
    }
}

/// 复制策略声明（声明式——字段白名单+频率+条件）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplicationPolicy {
    /// 策略名。
    pub name: &'static str,
    /// 同步字段白名单（声明制——白名单外字段违律）。
    pub fields: Vec<&'static str>,
    /// 频率（Hz）。
    pub rate_hz: u32,
    /// 条件。
    pub cond: ReplCond,
}

impl ReplicationPolicy {
    /// 声明律审查（O(白名单)）：
    /// - Always 律：每帧全量同步频率不得超 REPL_MAX_HZ；
    /// - 白名单律：字段必须在 FIELD_WHITELIST 内；
    /// - 空白名单：无字段可同步=无意义声明，违律。
    pub fn audit_law(&self) -> Result<(), u16> {
        if self.fields.is_empty() {
            return Err(E_NE_POLICY_VIOLATION);
        }
        for f in &self.fields {
            if !FIELD_WHITELIST.iter().any(|w| w == f) {
                return Err(E_NE_POLICY_VIOLATION);
            }
        }
        if self.cond == ReplCond::Always && self.rate_hz > REPL_MAX_HZ {
            return Err(E_NE_POLICY_VIOLATION);
        }
        Ok(())
    }
}

/// 同步字段白名单（声明律基准——AA02 域字段闭集）。
pub const FIELD_WHITELIST: [&str; 8] = [
    "transform", "velocity", "health", "animation_state", "inventory",
    "chat_text", "input_state", "world_time",
];

/// 策略消费凭证（AA02 消费面——注册通过即产出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PolicyGrant {
    pub pol_id: u32,
    pub name: &'static str,
    pub rate_hz: u32,
    pub cond: ReplCond,
}

// ---------------------------------------------------------------------------
// 扩展门禁（同审——传输与策略同一扇门四查）
// ---------------------------------------------------------------------------

/// 网络扩展门禁（注册总闸）。
#[derive(Debug)]
pub struct NetExtGate {
    /// 令牌签发器。
    pub issuer: TokenIssuer,
    /// 已注册传输（名→契约）。
    pub transports: BTreeMap<&'static str, TransportContract>,
    /// 已注册策略（名→声明）。
    pub policies: BTreeMap<&'static str, ReplicationPolicy>,
    /// 累计已批预算（bps——预算审批的 O(1) 查账锚）。
    pub committed_bps: u64,
    /// 审查台账（审查不留痕等于没审）。
    pub audit: Vec<AuditRow>,
    /// 发号器。
    next_id: u32,
    /// 立案簿。
    pub bag: DiagBag,
}

impl NetExtGate {
    pub fn new() -> NetExtGate {
        NetExtGate {
            issuer: TokenIssuer::new(),
            transports: BTreeMap::new(),
            policies: BTreeMap::new(),
            committed_bps: 0,
            audit: Vec::new(),
            next_id: 1,
            bag: DiagBag::new(),
        }
    }

    /// 传输注册（同审四查：令牌→版本→重名→预算——全 O(1)）。
    pub fn register_transport(
        &mut self,
        token: &RegistryToken,
        c: TransportContract,
    ) -> Result<TransportGrant, u16> {
        // 查 1：沙箱令牌（越权注册→拒绝）。
        if !self.issuer.verify(token) {
            self.bag.push(E_NE_NO_TOKEN, c.name, "无令牌或伪令牌");
            return Err(E_NE_NO_TOKEN);
        }
        // 查 2：接口版本（冻结拦截）。
        if c.if_version != TRANSPORT_IF_VERSION {
            self.bag.push(
                E_NE_IF_VERSION,
                c.name,
                &format!("版本{}≠冻结{}", c.if_version, TRANSPORT_IF_VERSION),
            );
            return Err(E_NE_IF_VERSION);
        }
        // 查 3：重名。
        if self.transports.contains_key(c.name) {
            self.bag.push(E_NE_DUP_NAME, c.name, "同名传输已注册");
            return Err(E_NE_DUP_NAME);
        }
        // 查 4：预算审批（累计和+新>总账→拦截——O(1) 加法查账）。
        if self.committed_bps + c.session_budget_bps > TOTAL_BUDGET_BPS {
            self.bag.push(
                E_NE_BUDGET_EXCEED,
                c.name,
                &format!("累计{}+新{}>总{}", self.committed_bps, c.session_budget_bps, TOTAL_BUDGET_BPS),
            );
            return Err(E_NE_BUDGET_EXCEED);
        }
        // 四查全过——注册入账+审查台账。
        let reg_id = self.next_id;
        self.next_id += 1;
        self.transports.insert(c.name, c);
        self.committed_bps += c.session_budget_bps;
        self.audit.push(AuditRow {
            reg_id,
            who: token.holder,
            kind_zh: c.kind.zh(),
            budget_bps: c.session_budget_bps,
            passed: true,
        });
        Ok(TransportGrant { reg_id, name: c.name, budget_bps: c.session_budget_bps })
    }

    /// 策略注册（同审四查：令牌→重名→违律驳回→产出 AA02 凭证）。
    pub fn register_policy(
        &mut self,
        token: &RegistryToken,
        p: ReplicationPolicy,
    ) -> Result<PolicyGrant, u16> {
        // 查 1：沙箱令牌。
        if !self.issuer.verify(token) {
            self.bag.push(E_NE_NO_TOKEN, p.name, "无令牌或伪令牌");
            return Err(E_NE_NO_TOKEN);
        }
        // 查 2：重名。
        if self.policies.contains_key(p.name) {
            self.bag.push(E_NE_DUP_NAME, p.name, "同名策略已注册");
            return Err(E_NE_DUP_NAME);
        }
        // 查 3：声明律（违律→驳回）。
        if let Err(code) = p.audit_law() {
            self.bag.push(code, p.name, "声明不合复制律");
            return Err(code);
        }
        // 全过——入账+凭证+台账。
        let pol_id = self.next_id;
        self.next_id += 1;
        self.audit.push(AuditRow {
            reg_id: pol_id,
            who: token.holder,
            kind_zh: "复制策略",
            budget_bps: 0,
            passed: true,
        });
        self.policies.insert(p.name, p.clone());
        Ok(PolicyGrant { pol_id, name: p.name, rate_hz: p.rate_hz, cond: p.cond })
    }

    /// 审查台账读屏摘要。
    pub fn audit_spoken(&self) -> String {
        let passed = self.audit.iter().filter(|r| r.passed).count();
        format!(
            "审查台账{}条通过{} 预算{}/{}bps",
            self.audit.len(), passed, self.committed_bps, TOTAL_BUDGET_BPS
        )
    }
}

// ---------------------------------------------------------------------------
// 判据（九项——传输注册/预算拦截/重名/策略注册/违律驳回/同审台账/
// 越权拒/版本拦截/全链编排）
// ---------------------------------------------------------------------------

/// F5415 判据入口（返回式 CheckSet——全部预期常量独立手算）。
pub fn run_vef67_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5415");

    // 传-01 传输注册 O(1)：合法注册→回执+台账+预算入账恰值。
    {
        let mut g = NetExtGate::new();
        let tok = g.issuer.grant(7, 0xAABB);
        let c = TransportContract {
            if_version: TRANSPORT_IF_VERSION,
            name: "kcp-adapter",
            kind: TransportKind::Reliable,
            session_budget_bps: 200_000_000,
            per_frame_budget_bps: 3_000_000,
        };
        let r = g.register_transport(&tok, c);
        let ok = match r {
            Ok(gr) => {
                gr.reg_id == 1
                    && gr.budget_bps == 200_000_000
                    && g.committed_bps == 200_000_000
                    && g.audit.len() == 1
                    && g.audit[0].passed
                    && g.audit[0].who == 7
                    && g.audit[0].kind_zh == "可靠流"
            }
            Err(_) => false,
        };
        if ok {
            s.ok("F5415-传-01-传输注册O1台账恰值");
        } else {
            s.fail("F5415-传-01-传输注册O1台账恰值", "回执或台账不符");
        }
    }

    // 传-02 预算拦截：累计 800M+新 300M>总 1G→E_NE_BUDGET_EXCEED。
    {
        let mut g = NetExtGate::new();
        let tok = g.issuer.grant(1, 0x11);
        let mk = |name: &'static str, bps: u64| TransportContract {
            if_version: TRANSPORT_IF_VERSION,
            name,
            kind: TransportKind::Unreliable,
            session_budget_bps: bps,
            per_frame_budget_bps: bps / 60,
        };
        let _ = g.register_transport(&tok, mk("a", 500_000_000));
        let _ = g.register_transport(&tok, mk("b", 300_000_000));
        // 手算：累计 800M；新 300M → 1100M > 1000M 拦截。
        let r = g.register_transport(&tok, mk("c", 300_000_000));
        // 边界恰值：新 200M → 恰好 1000M=总账（≤ 不超）放行。
        let r2 = g.register_transport(&tok, mk("d", 200_000_000));
        let ok = matches!(r, Err(E_NE_BUDGET_EXCEED))
            && g.bag.count_of(E_NE_BUDGET_EXCEED) == 1
            && matches!(r2, Ok(gr) if gr.budget_bps == 200_000_000)
            && g.committed_bps == 1_000_000_000;
        if ok {
            s.ok("F5415-传-02-预算拦截边界恰值");
        } else {
            s.fail("F5415-传-02-预算拦截边界恰值", "拦截时机或边界不符");
        }
    }

    // 传-03 重名拒：同名二次注册→E_NE_DUP_NAME。
    {
        let mut g = NetExtGate::new();
        let tok = g.issuer.grant(1, 0x22);
        let c = TransportContract {
            if_version: TRANSPORT_IF_VERSION,
            name: "dup",
            kind: TransportKind::Reliable,
            session_budget_bps: 1_000_000,
            per_frame_budget_bps: 100_000,
        };
        let _ = g.register_transport(&tok, c);
        let r = g.register_transport(&tok, c);
        let ok = matches!(r, Err(E_NE_DUP_NAME))
            && g.bag.count_of(E_NE_DUP_NAME) == 1
            && g.transports.len() == 1; // 不重复入账
        if ok {
            s.ok("F5415-传-03-重名拒绝不重复入账");
        } else {
            s.fail("F5415-传-03-重名拒绝不重复入账", "重名判定不符");
        }
    }

    // 策-01 策略注册：合法声明→AA02 凭证+台账。
    {
        let mut g = NetExtGate::new();
        let tok = g.issuer.grant(3, 0x33);
        let p = ReplicationPolicy {
            name: "pos-onchange",
            fields: vec!["transform", "velocity"],
            rate_hz: 30,
            cond: ReplCond::OnChange,
        };
        let r = g.register_policy(&tok, p);
        let ok = match r {
            Ok(gr) => {
                gr.pol_id == 1
                    && gr.rate_hz == 30
                    && gr.cond == ReplCond::OnChange
                    && g.policies.len() == 1
                    && g.audit.len() == 1
                    && g.audit[0].kind_zh == "复制策略"
            }
            Err(_) => false,
        };
        if ok {
            s.ok("F5415-策-01-策略注册AA02凭证产出");
        } else {
            s.fail("F5415-策-01-策略注册AA02凭证产出", "凭证或台账不符");
        }
    }

    // 策-02 违律驳回三态：Always 超频/白名单外字段/空白名单。
    {
        let mut g = NetExtGate::new();
        let tok = g.issuer.grant(1, 0x44);
        let over = ReplicationPolicy {
            name: "always-120hz",
            fields: vec!["transform"],
            rate_hz: 120, // > REPL_MAX_HZ=60
            cond: ReplCond::Always,
        };
        let outsider = ReplicationPolicy {
            name: "bad-field",
            fields: vec!["transform", "credit_card_number"], // 白名单外
            rate_hz: 30,
            cond: ReplCond::OnChange,
        };
        let empty = ReplicationPolicy {
            name: "empty-fields",
            fields: vec![],
            rate_hz: 30,
            cond: ReplCond::OnTick,
        };
        let r1 = g.register_policy(&tok, over);
        let r2 = g.register_policy(&tok, outsider);
        let r3 = g.register_policy(&tok, empty);
        let ok = r1 == Err(E_NE_POLICY_VIOLATION)
            && r2 == Err(E_NE_POLICY_VIOLATION)
            && r3 == Err(E_NE_POLICY_VIOLATION)
            && g.bag.count_of(E_NE_POLICY_VIOLATION) == 3
            && g.policies.is_empty();
        if ok {
            s.ok("F5415-策-02-违律驳回三态全拦");
        } else {
            s.fail("F5415-策-02-违律驳回三态全拦", "驳回面不全");
        }
    }

    // 门-01 同审门禁：传输+策略同过一扇门——两类注册同台账可查。
    {
        let mut g = NetExtGate::new();
        let tok = g.issuer.grant(5, 0x55);
        let _ = g.register_transport(&tok, TransportContract {
            if_version: TRANSPORT_IF_VERSION,
            name: "t1",
            kind: TransportKind::Unreliable,
            session_budget_bps: 10_000_000,
            per_frame_budget_bps: 200_000,
        });
        let _ = g.register_policy(&tok, ReplicationPolicy {
            name: "p1",
            fields: vec!["health"],
            rate_hz: 10,
            cond: ReplCond::OnTick,
        });
        // 手算：台账 2 条（传输 1+策略 1）全通过。
        let ok = g.audit.len() == 2
            && g.audit.iter().all(|r| r.passed)
            && g.audit[0].kind_zh == "不可靠数据报"
            && g.audit[1].kind_zh == "复制策略"
            && g.audit_spoken().contains("审查台账2条通过2");
        if ok {
            s.ok("F5415-门-01-同审门禁同一台账");
        } else {
            s.fail("F5415-门-01-同审门禁同一台账", "同审台账不符");
        }
    }

    // 门-02 越权拒：零令牌（未签发持有人）与伪令牌（密钥错）全拒。
    {
        let mut g = NetExtGate::new();
        let _real = g.issuer.grant(9, 0x66);
        let ghost = RegistryToken { holder: 42, secret: 0x00 }; // 未签发
        let forged = RegistryToken { holder: 9, secret: 0xDEAD }; // 密钥错
        let c = TransportContract {
            if_version: TRANSPORT_IF_VERSION,
            name: "x",
            kind: TransportKind::Reliable,
            session_budget_bps: 1_000_000,
            per_frame_budget_bps: 10_000,
        };
        let r1 = g.register_transport(&ghost, c);
        let r2 = g.register_transport(&forged, c);
        let ok = r1 == Err(E_NE_NO_TOKEN)
            && r2 == Err(E_NE_NO_TOKEN)
            && g.bag.count_of(E_NE_NO_TOKEN) == 2
            && g.transports.is_empty()
            && g.committed_bps == 0
            && g.issuer.holders() == 1;
        if ok {
            s.ok("F5415-门-02-沙箱越权伪令牌全拒");
        } else {
            s.fail("F5415-门-02-沙箱越权伪令牌全拒", "沙箱面不全");
        }
    }

    // 冻-01 接口版本拦截：版本 2≠冻结 3→E_NE_IF_VERSION。
    {
        let mut g = NetExtGate::new();
        let tok = g.issuer.grant(1, 0x77);
        let old = TransportContract {
            if_version: TRANSPORT_IF_VERSION - 1,
            name: "legacy",
            kind: TransportKind::Reliable,
            session_budget_bps: 1_000_000,
            per_frame_budget_bps: 10_000,
        };
        let r = g.register_transport(&tok, old);
        let ok = r == Err(E_NE_IF_VERSION)
            && g.bag.count_of(E_NE_IF_VERSION) == 1
            && g.committed_bps == 0;
        if ok {
            s.ok("F5415-冻-01-接口冻结版本拦截");
        } else {
            s.fail("F5415-冻-01-接口冻结版本拦截", "版本拦截不符");
        }
    }

    // 编-01 全链编排：合法传输 2+策略 1 注册→预算守恒恰值→台账齐
    // →读屏摘要非空→全程零 panic。
    {
        let mut g = NetExtGate::new();
        let tok = g.issuer.grant(2, 0x88);
        let g1 = g.register_transport(&tok, TransportContract {
            if_version: TRANSPORT_IF_VERSION,
            name: "net-a",
            kind: TransportKind::Reliable,
            session_budget_bps: 400_000_000,
            per_frame_budget_bps: 6_000_000,
        });
        let g2 = g.register_transport(&tok, TransportContract {
            if_version: TRANSPORT_IF_VERSION,
            name: "net-b",
            kind: TransportKind::Unreliable,
            session_budget_bps: 350_000_000,
            per_frame_budget_bps: 5_000_000,
        });
        let p1 = g.register_policy(&tok, ReplicationPolicy {
            name: "world-sync",
            fields: vec!["transform", "world_time"],
            rate_hz: 60, // 恰在 Always 上限内（≤60 合律）
            cond: ReplCond::Always,
        });
        let chain_ok = matches!(&g1, Ok(a) if a.reg_id == 1)
            && matches!(&g2, Ok(b) if b.reg_id == 2)
            && matches!(&p1, Ok(p) if p.pol_id == 3 && p.rate_hz == 60)
            && g.committed_bps == 750_000_000 // 400M+350M 手算
            && g.audit.len() == 3
            && g.audit.iter().all(|r| r.passed)
            && g.transports.len() == 2
            && g.policies.len() == 1
            && g.bag.is_empty()
            && g.audit_spoken().contains("750000000");
        if chain_ok {
            s.ok("F5415-编-01-注册审批台账全链O1对账");
        } else {
            s.fail("F5415-编-01-注册审批台账全链O1对账", "编排对账不符");
        }
    }

    s
}
