//! vef73_reparch —— VE-F5421 复制总架构（AA02 组开工）
//! ============================================================
//! 职责（锚点原文逐条落实）：
//!   · 复制定位：让所有人看到同一个世界——状态同步是多人体验的生命线
//!   · 架构四段：权威状态 → 快照/增量 → 传输 → 客户端应用——段间数据契约冻结
//!   · 承接 F5405 三类三策略宪法——复制层宪法执行组
//!   · 错误路径：段间失配→对拍；宪法破坏→拒绝红线级；契约破坏→版本拦截
//!   · 性能：流水线 O(状态量)；拦截 O(1)；对拍 O(1)
//!   · 对接：F5405 宪法上游；F5422 权威下游；F5440 双签闸
//!
//! 宪法（F5405 上游执行卡）：
//!   宪一 · 服务端权威：权威状态唯一真源在服务端，客户端只预测不决定——
//!          客户端写权威是红线级违宪（0xEB02），API 结构上不暴露旁路。
//!   宪二 · 三类三策略：高频态（位置类）/低频态（属性类）/一次性事件三类，
//!          各绑定专属策略（不可靠最新/可靠有序/可靠恰好一次）——
//!          类→策略映射是静态宪法矩阵与纯函数双写（对拍判据），
//!          publish API 不暴露策略参数，选错在类型上不可达。
//!   宪三 · 契约冻结：四段间三接口（S1→S2/S2→S3/S3→S4）数据契约
//!          （schema 版本 + 载荷封印）冻结在案；版本不符版本拦截（0xEB03）、
//!          封印失配段间对拍拦截（0xEB01）、解冻需双签（0xEB0A）。
//!
//! 诊断码段 0xEB（十码，本单独占）：
//!   0xEB01 段间契约失配（载荷封印对拍不符）
//!   0xEB02 宪法红线（客户端写权威）
//!   0xEB03 契约版本拦截（schema 版本不符）
//!   0xEB04 未注册状态类（三类闭集外）
//!   0xEB05 策略违例（一次性事件 seq 乱序）
//!   0xEB06 段序破坏（跳段/回段）
//!   0xEB07 快照锚点缺失（增量无基线）
//!   0xEB08 频次超限（低频窗口内重发）
//!   0xEB09 会话未注册 / 越权调冻结
//!   0xEB0A 解除冻结缺双签
//!
//! 两权分立：Maintainer（冻结/解冻契约、注册状态类、调频次预算、双签）/
//!   Runtime（advance/publish/write/predict/apply/audit 只读面）。
//!   mount 唯一入口；FNV-1a 64 封印 + 篡改检出。
//! ============================================================

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ============================================================
// 第一节 · 全局常量
// ============================================================

/// 复制协议 schema 版本（契约冻结的对象）
pub const REPL_SCHEMA_VER: u32 = 1;
/// 流水线深度（架构四段闭集）
pub const PIPELINE_DEPTH: usize = 4;
/// 段间契约接口数（四段三接口）
pub const CONTRACT_IFACES: usize = 3;
/// 低频频次窗口（tick）：窗口内重复 publish 视为超限
pub const LOWFREQ_WINDOW_TICKS: u64 = 12; // 5Hz @ 60Hz 步长
/// 全量锚点周期（tick）：周期快照宪法
pub const ANCHOR_PERIOD_TICKS: u64 = 32;
/// 审计账满容
pub const AUDIT_CAP: usize = 256;
/// 双签签名人最小长度
pub const SIGNER_MIN_LEN: usize = 2;
/// 冻结理由最小长度
pub const FREEZE_REASON_MIN: usize = 4;

// ============================================================
// 第二节 · 诊断码段 0xEB
// ============================================================

pub const XC_CONTRACT_SEAL: u16 = 0xEB01; // 段间契约失配
pub const XC_CONSTITUTION_RED: u16 = 0xEB02; // 宪法红线：客户端写权威
pub const XC_CONTRACT_VER: u16 = 0xEB03; // 契约版本拦截
pub const XC_CLASS_UNKNOWN: u16 = 0xEB04; // 未注册状态类
pub const XC_POLICY_VIOLATION: u16 = 0xEB05; // 策略违例（事件乱序）
pub const XC_STAGE_ORDER: u16 = 0xEB06; // 段序破坏
pub const XC_ANCHOR_ABSENT: u16 = 0xEB07; // 快照锚点缺失
pub const XC_FREQ_EXCEEDED: u16 = 0xEB08; // 频次超限
pub const XC_SESSION_OR_ROLE: u16 = 0xEB09; // 会话未注册/越权
pub const XC_DUAL_SIGN: u16 = 0xEB0A; // 解除冻结缺双签

// ============================================================
// 第三节 · FNV-1a 64
// ============================================================

pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = FNV_OFFSET;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

pub fn seal_str(s: &str) -> u64 {
    fnv1a64(s.as_bytes())
}

// ============================================================
// 第四节 · 定点数转串
// ============================================================

pub fn push_u64(out: &mut String, mut v: u64) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    while n > 0 {
        n -= 1;
        out.push(buf[n] as char);
    }
}

// ============================================================
// 第五节 · 状态三类（F5405 宪法闭集）
// ============================================================

/// 状态三类：高频态（位置类）/低频态（属性类）/一次性事件
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StateClass {
    /// 高频态：位置/朝向/速度——每 tick 都可能变
    HighFreq,
    /// 低频态：属性/装备/血量——变更驱动
    LowFreq,
    /// 一次性事件：爆炸/拾取/击杀——恰好一次
    OneShotEvent,
}

impl StateClass {
    pub fn as_str(self) -> &'static str {
        match self {
            StateClass::HighFreq => "高频态",
            StateClass::LowFreq => "低频态",
            StateClass::OneShotEvent => "一次性事件",
        }
    }

    pub fn index(self) -> usize {
        match self {
            StateClass::HighFreq => 0,
            StateClass::LowFreq => 1,
            StateClass::OneShotEvent => 2,
        }
    }

    pub fn all() -> [StateClass; 3] {
        [StateClass::HighFreq, StateClass::LowFreq, StateClass::OneShotEvent]
    }

    pub fn from_index(i: u16) -> Option<StateClass> {
        match i {
            0 => Some(StateClass::HighFreq),
            1 => Some(StateClass::LowFreq),
            2 => Some(StateClass::OneShotEvent),
            _ => None,
        }
    }

    /// 三类的频次预算（‰ of tick-rate；高频=每 tick 可发=1000‰）
    pub fn freq_budget_permille(self) -> u32 {
        match self {
            StateClass::HighFreq => 1000, // 每 tick 可发
            StateClass::LowFreq => 83,    // ≈5Hz @60Hz（1/12 tick）
            StateClass::OneShotEvent => 0, // 事件无频次预算（seq 纪律管）
        }
    }
}

// ============================================================
// 第六节 · 三策略（宪法矩阵 ↔ 纯函数双写）
// ============================================================

/// 三策略闭集
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CopyPolicy {
    /// 不可靠最新：只发最新值，丢了等下帧（高频态专属）
    UnreliableLatest,
    /// 可靠有序：重传至确认，顺序保证（低频态专属）
    ReliableOrdered,
    /// 可靠恰好一次：seq 幂等去重（一次性事件专属）
    ReliableOnce,
}

impl CopyPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            CopyPolicy::UnreliableLatest => "不可靠最新",
            CopyPolicy::ReliableOrdered => "可靠有序",
            CopyPolicy::ReliableOnce => "可靠恰好一次",
        }
    }

    pub fn index(self) -> usize {
        match self {
            CopyPolicy::UnreliableLatest => 0,
            CopyPolicy::ReliableOrdered => 1,
            CopyPolicy::ReliableOnce => 2,
        }
    }

    pub fn all() -> [CopyPolicy; 3] {
        [CopyPolicy::UnreliableLatest, CopyPolicy::ReliableOrdered, CopyPolicy::ReliableOnce]
    }
}

/// 宪法矩阵：类→策略（静态闭集表）
pub const CLASS_POLICY_MATRIX: [CopyPolicy; 3] = [
    CopyPolicy::UnreliableLatest, // 高频态
    CopyPolicy::ReliableOrdered,  // 低频态
    CopyPolicy::ReliableOnce,     // 一次性事件
];

/// 宪法纯函数：类→策略（与矩阵双写对拍——判据面）
pub fn policy_for(class: StateClass) -> CopyPolicy {
    match class {
        StateClass::HighFreq => CopyPolicy::UnreliableLatest,
        StateClass::LowFreq => CopyPolicy::ReliableOrdered,
        StateClass::OneShotEvent => CopyPolicy::ReliableOnce,
    }
}

/// 双写自检：矩阵与纯函数三格全等
pub fn selfcheck_matrix_dual() -> bool {
    for c in StateClass::all().iter() {
        if CLASS_POLICY_MATRIX[c.index()] != policy_for(*c) {
            return false;
        }
    }
    true
}

// ============================================================
// 第七节 · 架构四段（流水线闭集）
// ============================================================

/// 四段流水线闭集：权威状态 → 快照/增量 → 传输 → 客户端应用
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PipelineStage {
    /// S1 权威状态：服务端唯一真源
    Authoritative,
    /// S2 快照/增量：三类分策略打包
    SnapshotDelta,
    /// S3 传输：通道质量与票据
    Transport,
    /// S4 客户端应用：预测+插值（只读）
    ClientApply,
}

impl PipelineStage {
    pub fn as_str(self) -> &'static str {
        match self {
            PipelineStage::Authoritative => "权威状态",
            PipelineStage::SnapshotDelta => "快照/增量",
            PipelineStage::Transport => "传输",
            PipelineStage::ClientApply => "客户端应用",
        }
    }

    pub fn index(self) -> usize {
        match self {
            PipelineStage::Authoritative => 0,
            PipelineStage::SnapshotDelta => 1,
            PipelineStage::Transport => 2,
            PipelineStage::ClientApply => 3,
        }
    }

    pub fn all() -> [PipelineStage; 4] {
        [
            PipelineStage::Authoritative,
            PipelineStage::SnapshotDelta,
            PipelineStage::Transport,
            PipelineStage::ClientApply,
        ]
    }

    pub fn from_index(i: usize) -> Option<PipelineStage> {
        match i {
            0 => Some(PipelineStage::Authoritative),
            1 => Some(PipelineStage::SnapshotDelta),
            2 => Some(PipelineStage::Transport),
            3 => Some(PipelineStage::ClientApply),
            _ => None,
        }
    }

    /// 段职责一句话（架构册渲染事实源）
    pub fn duty(self) -> &'static str {
        match self {
            PipelineStage::Authoritative => "服务端权威状态唯一真源，只服务端可写",
            PipelineStage::SnapshotDelta => "按三类三策略打包快照与增量，锚点周期兜底",
            PipelineStage::Transport => "通道传输，时序票据与质量采样",
            PipelineStage::ClientApply => "客户端只读应用：预测先行+插值平滑",
        }
    }
}

// ============================================================
// 第八节 · 段间契约（冻结对象）
// ============================================================

/// 段间契约：schema 版本 + 载荷封印（冻结制）
#[derive(Clone, Copy)]
pub struct StageContract {
    /// 接口序（0=S1→S2, 1=S2→S3, 2=S3→S4）
    pub iface: usize,
    pub from: PipelineStage,
    pub to: PipelineStage,
    pub schema_ver: u32,
    /// 载荷形态封印（描述性字符串的 FNV——形态变化即失配）
    pub payload_seal: u64,
    pub frozen_tick: u64,
}

/// 三接口契约形态描述（封印对象；形态定死即契约冻结）
pub const IFACE_PAYLOAD_SHAPES: [&str; 3] = [
    "auth-state{classes=3,ver,bodies[]}", // S1→S2
    "delta-pack{anchor,dirty[],events[]}", // S2→S3
    "wire-frame{seq,tickets[],apply[]}",  // S3→S4
];

/// 预冻结契约表（构造期由 Maintainer 冻结入册）
pub fn canonical_payload_seal(iface: usize) -> Option<u64> {
    if iface >= CONTRACT_IFACES {
        return None;
    }
    Some(seal_str(IFACE_PAYLOAD_SHAPES[iface]))
}

// ============================================================
// 第九节 · 侧别与对端
// ============================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Peer {
    /// 服务端：权威侧（可写权威状态）
    Server,
    /// 客户端：只读侧（只可预测与应用）
    Client,
}

impl Peer {
    pub fn as_str(self) -> &'static str {
        match self {
            Peer::Server => "服务端",
            Peer::Client => "客户端",
        }
    }
}

// ============================================================
// 第十节 · 状态类登记注册制
// ============================================================

/// 已注册状态类条目
#[derive(Clone)]
pub struct ClassEntry {
    pub class: StateClass,
    pub registered_tick: u64,
    /// 最近一次 publish 的 tick（频次纪律）
    pub last_publish_tick: u64,
    /// publish 计数（遥测）
    pub publish_count: u64,
    /// 事件去重最高 seq（OneShot 恰好一次）
    pub max_event_seq: u64,
}

// ============================================================
// 第十一节 · 复制会话（四段游标 + 契约对拍）
// ============================================================

/// 段推进判定闭集
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StageVerdict {
    Pending,
    Pass,
    Mismatch,
    Blocked,
}

/// 复制会话
pub struct ReplSession {
    pub session_id: u32,
    /// 当前已到达段（游标；严格递增推进）
    pub cursor: usize,
    /// 各段 verdict
    pub verdicts: [StageVerdict; PIPELINE_DEPTH],
    /// 推进审计计数
    pub advances: u32,
    pub created_tick: u64,
}

impl ReplSession {
    fn new(id: u32, tick: u64) -> ReplSession {
        ReplSession {
            session_id: id,
            cursor: usize::MAX, // 未开始
            verdicts: [
                StageVerdict::Pending,
                StageVerdict::Pending,
                StageVerdict::Pending,
                StageVerdict::Pending,
            ],
            advances: 0,
            created_tick: tick,
        }
    }

    /// 是否已走完全部四段
    pub fn completed(&self) -> bool {
        self.cursor == PIPELINE_DEPTH - 1
            && self.verdicts[PIPELINE_DEPTH - 1] == StageVerdict::Pass
    }
}

// ============================================================
// 第十二节 · 复制引擎（宪法执行 + 契约对拍 + 两权分立）
// ============================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditEvent {
    SessionCreated,
    StageAdvanced,
    StageOrderBroken,
    ContractVerRejected,
    ContractSealMismatch,
    ConstitutionRed,
    ClassRegistered,
    ClassUnknown,
    FreqExceeded,
    EventDeduped,
    PolicyViolation,
    AnchorMissing,
    AnchorStale,
    AnchorTaken,
    ContractFrozen,
    ContractUnfrozen,
    DualSigned,
    RoleViolation,
    SessionUnknown,
}

impl AuditEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            AuditEvent::SessionCreated => "session-created",
            AuditEvent::StageAdvanced => "stage-advanced",
            AuditEvent::StageOrderBroken => "stage-order-broken",
            AuditEvent::ContractVerRejected => "contract-ver-rejected",
            AuditEvent::ContractSealMismatch => "contract-seal-mismatch",
            AuditEvent::ConstitutionRed => "constitution-red",
            AuditEvent::ClassRegistered => "class-registered",
            AuditEvent::ClassUnknown => "class-unknown",
            AuditEvent::FreqExceeded => "freq-exceeded",
            AuditEvent::EventDeduped => "event-deduped",
            AuditEvent::PolicyViolation => "policy-violation",
            AuditEvent::AnchorMissing => "anchor-missing",
            AuditEvent::AnchorStale => "anchor-stale",
            AuditEvent::AnchorTaken => "anchor-taken",
            AuditEvent::ContractFrozen => "contract-frozen",
            AuditEvent::ContractUnfrozen => "contract-unfrozen",
            AuditEvent::DualSigned => "dual-signed",
            AuditEvent::RoleViolation => "role-violation",
            AuditEvent::SessionUnknown => "session-unknown",
        }
    }
}

struct AuditEntry {
    tick: u64,
    ev: AuditEvent,
    code: u16,
}

pub struct AuditLog {
    entries: Vec<AuditEntry>,
    dropped: usize,
}

impl AuditLog {
    fn new() -> AuditLog {
        AuditLog {
            entries: Vec::new(),
            dropped: 0,
        }
    }

    fn record(&mut self, tick: u64, ev: AuditEvent, code: u16) {
        if self.entries.len() >= AUDIT_CAP {
            self.entries.remove(0);
            self.dropped += 1;
        }
        self.entries.push(AuditEntry { tick, ev, code });
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.len() == 0
    }

    pub fn dropped(&self) -> usize {
        self.dropped
    }

    pub fn at(&self, i: usize) -> Option<(u64, AuditEvent, u16)> {
        if i >= self.entries.len() {
            return None;
        }
        let e = &self.entries[i];
        Some((e.tick, e.ev, e.code))
    }

    pub fn last_of(&self, ev: AuditEvent) -> Option<(u64, u16)> {
        for e in self.entries.iter().rev() {
            if e.ev == ev {
                return Some((e.tick, e.code));
            }
        }
        None
    }

    pub fn count_of(&self, ev: AuditEvent) -> usize {
        let mut n = 0usize;
        for e in self.entries.iter() {
            if e.ev == ev {
                n += 1;
            }
        }
        n
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    /// 维护权：冻结/解冻契约、注册状态类、双签
    Maintainer,
    /// 运行权：会话推进/publish/写权威（服务端侧）/预测/应用/审计
    Runtime,
}

/// 契约冻结状态
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FreezeState {
    /// 未冻结（维护期）
    Open,
    /// 已冻结（运行期；契约对拍生效）
    Frozen,
    /// 解冻请求中（第一签已落）
    ThawPending,
}

pub struct ReplicArch {
    pub role: Role,
    /// 已注册状态类（注册制：闭集外的类拒绝）
    classes: Vec<ClassEntry>,
    /// 会话表
    sessions: Vec<ReplSession>,
    next_session: u32,
    /// 三接口契约（冻结制；None=未冻结）
    contracts: [Option<StageContract>; CONTRACT_IFACES],
    pub freeze: FreezeState,
    /// 解冻签名（双签）
    thaw_signers: Vec<String>,
    /// 全量锚点最近 tick（会话无关——引擎级锚点簿）
    last_anchor_tick: Option<u64>,
    /// 低频窗口超限计数
    freq_violations: u32,
    /// 宪法红线计数（一生累计）
    constitution_reds: u32,
    /// 事件去重计数（恰好一次语义的产出）
    event_dedups: u64,
    /// 引擎封印
    seal: u64,
    mounted: bool,
    pub audit: AuditLog,
}

impl ReplicArch {
    /// mount 唯一入口
    pub fn mount(role: Role, tick: u64) -> ReplicArch {
        let mut src = String::new();
        src.push_str("vef73-reparch|role=");
        src.push_str(match role {
            Role::Maintainer => "maintainer",
            Role::Runtime => "runtime",
        });
        src.push_str("|ver=");
        push_u64(&mut src, REPL_SCHEMA_VER as u64);
        ReplicArch {
            role,
            classes: Vec::new(),
            sessions: Vec::new(),
            next_session: 1,
            contracts: [None, None, None],
            freeze: FreezeState::Open,
            thaw_signers: Vec::new(),
            last_anchor_tick: None,
            freq_violations: 0,
            constitution_reds: 0,
            event_dedups: 0,
            seal: seal_str(&src),
            mounted: true,
            audit: AuditLog::new(),
        }
    }

    pub fn seal(&self) -> u64 {
        self.seal
    }

    pub fn verify_seal(&self) -> bool {
        let mut src = String::new();
        src.push_str("vef73-reparch|role=");
        src.push_str(match self.role {
            Role::Maintainer => "maintainer",
            Role::Runtime => "runtime",
        });
        src.push_str("|ver=");
        push_u64(&mut src, REPL_SCHEMA_VER as u64);
        seal_str(&src) == self.seal
    }

    fn require_maintainer(&mut self, tick: u64) -> Result<(), u16> {
        if self.role != Role::Maintainer {
            self.audit.record(tick, AuditEvent::RoleViolation, XC_SESSION_OR_ROLE);
            return Err(XC_SESSION_OR_ROLE);
        }
        Ok(())
    }

    // ---------- Maintainer 专属 ----------

    /// 注册状态类（注册制：闭集外 0xEB04）
    pub fn register_class(&mut self, class: StateClass, tick: u64) -> Result<(), u16> {
        self.require_maintainer(tick)?;
        for c in self.classes.iter() {
            if c.class == class {
                // 幂等：已注册直接成功
                return Ok(());
            }
        }
        self.classes.push(ClassEntry {
            class,
            registered_tick: tick,
            last_publish_tick: 0,
            publish_count: 0,
            max_event_seq: 0,
        });
        self.audit.record(tick, AuditEvent::ClassRegistered, 0);
        Ok(())
    }

    /// 类是否已注册
    pub fn class_known(&self, class: StateClass) -> bool {
        for c in self.classes.iter() {
            if c.class == class {
                return true;
            }
        }
        false
    }

    /// 类在注册表中的位置（publish 用；未注册返回 None）
    fn class_pos(&self, class: StateClass) -> Option<usize> {
        let mut i = 0usize;
        for c in self.classes.iter() {
            if c.class == class {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 冻结三接口契约（Maintainer；理由过短拒）
    pub fn freeze_contracts(&mut self, reason: &str, tick: u64) -> Result<(), u16> {
        self.require_maintainer(tick)?;
        if reason.len() < FREEZE_REASON_MIN {
            return Err(XC_SESSION_OR_ROLE);
        }
        let mut all = true;
        for iface in 0..CONTRACT_IFACES {
            let shape_seal = match canonical_payload_seal(iface) {
                Some(s) => s,
                None => {
                    all = false;
                    break;
                }
            };
            self.contracts[iface] = Some(StageContract {
                iface,
                from: PipelineStage::from_index(iface).unwrap_or(PipelineStage::Authoritative),
                to: PipelineStage::from_index(iface + 1).unwrap_or(PipelineStage::ClientApply),
                schema_ver: REPL_SCHEMA_VER,
                payload_seal: shape_seal,
                frozen_tick: tick,
            });
        }
        if !all {
            return Err(XC_CONTRACT_SEAL);
        }
        self.freeze = FreezeState::Frozen;
        self.audit.record(tick, AuditEvent::ContractFrozen, 0);
        Ok(())
    }

    /// 契约引用（只读）
    pub fn contract(&self, iface: usize) -> Option<&StageContract> {
        if iface >= CONTRACT_IFACES {
            return None;
        }
        self.contracts[iface].as_ref()
    }

    /// 解冻（双签：两个不同签名人）
    pub fn unfreeze(&mut self, signer: &str, tick: u64) -> Result<FreezeState, u16> {
        self.require_maintainer(tick)?;
        if self.freeze == FreezeState::Open {
            return Err(XC_DUAL_SIGN);
        }
        if signer.len() < SIGNER_MIN_LEN {
            return Err(XC_DUAL_SIGN);
        }
        for s in self.thaw_signers.iter() {
            if s == signer {
                return Err(XC_DUAL_SIGN); // 同签人重复
            }
        }
        self.thaw_signers.push(alloc::format!("{}", signer));
        if self.thaw_signers.len() >= 2 {
            self.thaw_signers.clear();
            self.freeze = FreezeState::Open;
            for c in self.contracts.iter_mut() {
                *c = None;
            }
            self.audit.record(tick, AuditEvent::DualSigned, 0);
            self.audit.record(tick, AuditEvent::ContractUnfrozen, 0);
            return Ok(FreezeState::Open);
        }
        self.freeze = FreezeState::ThawPending;
        self.audit.record(tick, AuditEvent::DualSigned, 0);
        Ok(self.freeze)
    }

    // ---------- 会话 ----------

    /// 创建复制会话（Runtime 可用）
    pub fn create_session(&mut self, tick: u64) -> Result<u32, u16> {
        if !self.mounted {
            return Err(XC_SESSION_OR_ROLE);
        }
        let id = self.next_session;
        self.next_session += 1;
        self.sessions.push(ReplSession::new(id, tick));
        self.audit.record(tick, AuditEvent::SessionCreated, 0);
        Ok(id)
    }

    fn session_index(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        for s in self.sessions.iter() {
            if s.session_id == id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 段推进（四段严格顺序 + 契约对拍）：
    ///   · 会话不存在 → 0xEB09
    ///   · 跳段/回段 → 0xEB06（段间失配的结构级形态之一）
    ///   · schema 版本不符 → 0xEB03（版本拦截）
    ///   · 载荷封印失配 → 0xEB01（段间对拍）
    ///   · 冻结未生效 → 0xEB01（运行期必须在冻结态推进）
    pub fn advance(
        &mut self,
        session_id: u32,
        stage: PipelineStage,
        payload_ver: u32,
        payload_seal: u64,
        tick: u64,
    ) -> Result<StageVerdict, u16> {
        // 会话存在性（拿索引，后面直接按索引操作避免双重借用）
        let sid = match self.session_index(session_id) {
            Some(i) => i,
            None => {
                self.audit.record(tick, AuditEvent::SessionUnknown, XC_SESSION_OR_ROLE);
                return Err(XC_SESSION_OR_ROLE);
            }
        };
        // 期望段 = 游标 + 1（游标 MAX 表示未开始，期望 S1）
        let cur = self.sessions[sid].cursor;
        let want_idx = if cur == usize::MAX { 0usize } else { cur + 1 };
        if want_idx >= PIPELINE_DEPTH {
            self.audit.record(tick, AuditEvent::StageOrderBroken, XC_STAGE_ORDER);
            return Err(XC_STAGE_ORDER); // 已走完不允许再推
        }
        if stage.index() != want_idx {
            self.audit.record(tick, AuditEvent::StageOrderBroken, XC_STAGE_ORDER);
            self.sessions[sid].verdicts[stage.index()] = StageVerdict::Blocked;
            return Err(XC_STAGE_ORDER);
        }
        // 冻结态前置：运行期推进必须在冻结态
        if self.freeze != FreezeState::Frozen {
            self.audit.record(tick, AuditEvent::ContractSealMismatch, XC_CONTRACT_SEAL);
            return Err(XC_CONTRACT_SEAL);
        }
        // 契约对拍（O(1)）：
        // S1（index 0）是起始段无进入接口；S2/S3/S4 分别对应 iface 0/1/2。
        let iface_in = if want_idx == 0 {
            usize::MAX // S1 无进入接口
        } else {
            want_idx - 1
        };
        if iface_in != usize::MAX {
            let c = match self.contracts[iface_in] {
                Some(c) => c,
                None => {
                    self.audit.record(tick, AuditEvent::ContractSealMismatch, XC_CONTRACT_SEAL);
                    return Err(XC_CONTRACT_SEAL);
                }
            };
            if c.from.index() + 1 != stage.index() || c.to != stage {
                self.audit.record(tick, AuditEvent::ContractSealMismatch, XC_CONTRACT_SEAL);
                return Err(XC_CONTRACT_SEAL);
            }
            if payload_ver != c.schema_ver {
                self.audit.record(tick, AuditEvent::ContractVerRejected, XC_CONTRACT_VER);
                self.sessions[sid].verdicts[stage.index()] = StageVerdict::Mismatch;
                return Err(XC_CONTRACT_VER);
            }
            if payload_seal != c.payload_seal {
                self.audit.record(tick, AuditEvent::ContractSealMismatch, XC_CONTRACT_SEAL);
                self.sessions[sid].verdicts[stage.index()] = StageVerdict::Mismatch;
                return Err(XC_CONTRACT_SEAL);
            }
        }
        // 通过：游标推进 + verdict 置 Pass
        self.sessions[sid].cursor = want_idx;
        self.sessions[sid].verdicts[stage.index()] = StageVerdict::Pass;
        self.sessions[sid].advances += 1;
        self.audit.record(tick, AuditEvent::StageAdvanced, 0);
        Ok(StageVerdict::Pass)
    }

    /// 会话查询
    pub fn session(&self, id: u32) -> Option<&ReplSession> {
        for s in self.sessions.iter() {
            if s.session_id == id {
                return Some(s);
            }
        }
        None
    }

    // ---------- 权威写闸（宪法红线） ----------

    /// 写权威状态：只服务端可写（宪一执行）。
    /// 客户端调用 → 0xEB02 红线拒绝 + 审计 + 计数。
    pub fn write_auth(&mut self, peer: Peer, tick: u64) -> Result<(), u16> {
        if peer == Peer::Client {
            self.constitution_reds += 1;
            self.audit.record(tick, AuditEvent::ConstitutionRed, XC_CONSTITUTION_RED);
            return Err(XC_CONSTITUTION_RED);
        }
        // 服务端写权威后，S1 段事实成立（这里只做闸与审计）
        self.audit.record(tick, AuditEvent::StageAdvanced, 0);
        Ok(())
    }

    /// 客户端预测通道（合法：客户端只预测不决定）
    pub fn predict(&mut self, tick: u64) -> Result<(), u16> {
        self.audit.record(tick, AuditEvent::StageAdvanced, 0);
        Ok(())
    }

    /// 客户端应用通道（S4 只读应用）
    pub fn client_apply(&mut self, tick: u64) -> Result<(), u16> {
        self.audit.record(tick, AuditEvent::StageAdvanced, 0);
        Ok(())
    }

    // ---------- publish（三类三策略执行） ----------

    /// 发布状态（策略由宪法矩阵内部决定——API 不暴露策略参数）：
    ///   HighFreq：每 tick 可发（不可靠最新——丢了等下帧）
    ///   LowFreq：频次窗口纪律（窗口内重复发 0xEB08）
    ///   OneShotEvent：seq 严格递增（乱序 0xEB05）；重复 seq 静默去重
    ///   类未注册 → 0xEB04
    pub fn publish(
        &mut self,
        class: StateClass,
        event_seq: u64,
        tick: u64,
    ) -> Result<CopyPolicy, u16> {
        let pos = match self.class_pos(class) {
            Some(p) => p,
            None => {
                self.audit.record(tick, AuditEvent::ClassUnknown, XC_CLASS_UNKNOWN);
                return Err(XC_CLASS_UNKNOWN);
            }
        };
        let policy = policy_for(class); // 宪法矩阵唯一出处
        // 逐类纪律（按注册表实际位置访问）
        if class == StateClass::LowFreq {
            let last = self.classes[pos].last_publish_tick;
            if last > 0 && tick > last && tick - last < LOWFREQ_WINDOW_TICKS {
                self.freq_violations += 1;
                self.audit.record(tick, AuditEvent::FreqExceeded, XC_FREQ_EXCEEDED);
                return Err(XC_FREQ_EXCEEDED);
            }
        }
        if class == StateClass::OneShotEvent {
            let max_seq = self.classes[pos].max_event_seq;
            if event_seq <= max_seq && max_seq > 0 {
                // 重复 seq：恰好一次语义 → 静默去重（不投递、不报错）
                self.event_dedups += 1;
                self.audit.record(tick, AuditEvent::EventDeduped, 0);
                return Ok(policy);
            }
            if event_seq > max_seq + 1 && max_seq > 0 {
                // 乱序（跳 seq）：可靠恰好一次要求严格递增
                self.audit.record(tick, AuditEvent::PolicyViolation, XC_POLICY_VIOLATION);
                return Err(XC_POLICY_VIOLATION);
            }
            self.classes[pos].max_event_seq = event_seq;
        }
        self.classes[pos].last_publish_tick = tick;
        self.classes[pos].publish_count += 1;
        Ok(policy)
    }

    // ---------- 锚点簿（周期快照宪法） ----------

    /// 打全量锚点（S2 打包前置）
    pub fn take_anchor(&mut self, tick: u64) {
        self.last_anchor_tick = Some(tick);
        self.audit.record(tick, AuditEvent::AnchorTaken, 0);
    }

    /// 增量打包前置闸：锚点必须存在且未过期。
    ///   无锚点 → 0xEB07；锚点过期（超 ANCHOR_PERIOD_TICKS）→ 0xEB07（stale）。
    pub fn delta_gate(&mut self, tick: u64) -> Result<(), u16> {
        match self.last_anchor_tick {
            None => {
                self.audit.record(tick, AuditEvent::AnchorMissing, XC_ANCHOR_ABSENT);
                Err(XC_ANCHOR_ABSENT)
            }
            Some(t) => {
                if tick > t && tick - t > ANCHOR_PERIOD_TICKS {
                    self.audit.record(tick, AuditEvent::AnchorStale, XC_ANCHOR_ABSENT);
                    Err(XC_ANCHOR_ABSENT)
                } else {
                    Ok(())
                }
            }
        }
    }

    pub fn anchor_age(&self, tick: u64) -> Option<u64> {
        self.last_anchor_tick.map(|t| if tick > t { tick - t } else { 0 })
    }

    // ---------- 统计 ----------

    pub fn class_count(&self) -> usize {
        self.classes.len()
    }

    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    pub fn freq_violations(&self) -> u32 {
        self.freq_violations
    }

    pub fn constitution_reds(&self) -> u32 {
        self.constitution_reds
    }

    pub fn event_dedups(&self) -> u64 {
        self.event_dedups
    }
}

// ============================================================
// 第十三节 · 自检聚合
// ============================================================

/// 自检一：四段闭集与三接口形状
pub fn selfcheck_stages() -> bool {
    for i in 0..PIPELINE_DEPTH {
        match PipelineStage::from_index(i) {
            Some(s) => {
                if s.index() != i {
                    return false;
                }
            }
            None => return false,
        }
    }
    if PipelineStage::from_index(PIPELINE_DEPTH).is_some() {
        return false;
    }
    // 三接口形状封印非零
    for iface in 0..CONTRACT_IFACES {
        match canonical_payload_seal(iface) {
            Some(s) => {
                if s == 0 {
                    return false;
                }
            }
            None => return false,
        }
    }
    if canonical_payload_seal(CONTRACT_IFACES).is_some() {
        return false;
    }
    true
}

/// 自检二：三类三策略双写 + 频次预算
pub fn selfcheck_classes() -> bool {
    if !selfcheck_matrix_dual() {
        return false;
    }
    for c in StateClass::all().iter() {
        match StateClass::from_index(c.index() as u16) {
            Some(k) => {
                if k != *c {
                    return false;
                }
            }
            None => return false,
        }
    }
    if StateClass::from_index(3).is_some() {
        return false;
    }
    // 高频=每 tick 可发
    if StateClass::HighFreq.freq_budget_permille() != 1000 {
        return false;
    }
    true
}

/// 自检三：定点转串
pub fn selfcheck_num() -> bool {
    let mut s = String::new();
    push_u64(&mut s, 0);
    if s != "0" {
        return false;
    }
    let mut s2 = String::new();
    push_u64(&mut s2, 123456);
    s2 == "123456"
}
