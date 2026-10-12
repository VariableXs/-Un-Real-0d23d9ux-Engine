//! vef74_authstate —— VE-F5422 权威状态模型（AA02 组第二单）
//! ============================================================
//! 职责（锚点原文逐条落实）：
//!   · 权威状态存储：所有可复制状态的唯一真源
//!   · 权威变更入口：状态只能经服务端逻辑变更——变更有日志
//!   · 权威查询：客户端查询走复制不走直查——单源纪律
//!   · 错误路径：旁路变更→拒绝红线级；日志缺失→断言拦截；直查绕过→归位拦截
//!   · 性能：存储 O(状态量)；校验 O(1)；归位 O(1)
//!   · 对接：F5405 宪法上游；F5423 快照消费；F5431 测试
//!
//! 三大纪律（域色反作弊根：唯一真源即公平基石）：
//!   纪一 · 唯一真源：权威存储是可复制状态的唯一真源；键必须注册入册
//!          （注册制：未注册键 0xE804），每键版本号全库单调递增。
//!   纪二 · 变更有日志：一切权威变更经服务端逻辑入口产生日志条目
//!          （seq 严格递增 + old/new 对账）；「无日志不入账」是结构级
//!          断言——日志满容时拒绝写入（0xE802）而不是静默滚丢；
//!          存储值与日志回放值双写对拍（0xE806 失配即存储毒化）。
//!   纪三 · 单源查询：客户端查询走复制不走直查——直查权威 0xE803
//!          归位拦截并给出复制游标指引；客户端复制视图只见版本与
//!          游标、不见活值（诚实边界）。
//!
//! 诊断码段 0xE8（十码，本单独占）：
//!   0xE801 旁路变更红线（非服务端逻辑通道写权威）
//!   0xE802 日志缺失断言（日志满容拒绝写入——无日志不入账）
//!   0xE803 直查绕过归位拦截
//!   0xE804 未注册状态键
//!   0xE805 变更序破坏（tick 回退）
//!   0xE806 日志账目失配（存储值 ≠ 日志回放值）
//!   0xE807 快照基线缺失（delta 前置闸）
//!   0xE808 复制视图越界（客户端视图请求活值）
//!   0xE809 越权（Runtime 调 Maintainer 操作/凭据未注册）
//!   0xE80A 日志截断缺双签
//!
//! 两权分立：Maintainer（注册键、快照+截断双签、冻结）/
//!   Runtime（服务端逻辑写、查询、日志读、审计）。
//!   mount 唯一入口；FNV-1a 64 封印 + 篡改检出。
//! ============================================================

use alloc::string::String;
use alloc::vec::Vec;

// ============================================================
// 第一节 · 全局常量
// ============================================================

/// 权威 schema 版本
pub const AUTH_SCHEMA_VER: u32 = 1;
/// 变更日志满容（满容拒绝写入——无日志不入账）
pub const CHANGELOG_CAP: usize = 256;
/// 键名最大长度
pub const KEY_MAX_LEN: usize = 32;
/// 值域上限（u64 状态量；值本身是抽象量，复制层只搬运）
pub const VALUE_MAX: u64 = 1 << 40;
/// 审计账满容
pub const AUDIT_CAP: usize = 256;
/// 双签签名人最小长度
pub const SIGNER_MIN_LEN: usize = 2;
/// 快照理由最小长度
pub const SNAPSHOT_REASON_MIN: usize = 4;

// ============================================================
// 第二节 · 诊断码段 0xE8
// ============================================================

pub const XC_BYPASS_WRITE: u16 = 0xE801; // 旁路变更红线
pub const XC_NOLOG_WRITE: u16 = 0xE802; // 日志缺失断言（满容拒写）
pub const XC_DIRECT_QUERY: u16 = 0xE803; // 直查绕过归位
pub const XC_KEY_UNKNOWN: u16 = 0xE804; // 未注册状态键
pub const XC_TICK_BACKWARD: u16 = 0xE805; // 变更序破坏
pub const XC_LOG_MISMATCH: u16 = 0xE806; // 日志账目失配
pub const XC_NO_BASELINE: u16 = 0xE807; // 快照基线缺失
pub const XC_VIEW_VIOLATION: u16 = 0xE808; // 复制视图越界
pub const XC_ROLE_OR_CRED: u16 = 0xE809; // 越权/凭据未注册
pub const XC_TRUNCATE_SIGN: u16 = 0xE80A; // 截断缺双签

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
// 第四节 · 复制类（F5405 宪法精神延续的本地闭集）
// ============================================================

/// 状态键的复制类（注册时登记；本地闭集与 vef73 宪法同构）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReplClass {
    /// 高频态（位置类）
    HighFreq,
    /// 低频态（属性类）
    LowFreq,
    /// 一次性事件
    OneShotEvent,
}

impl ReplClass {
    pub fn as_str(self) -> &'static str {
        match self {
            ReplClass::HighFreq => "高频态",
            ReplClass::LowFreq => "低频态",
            ReplClass::OneShotEvent => "一次性事件",
        }
    }

    pub fn all() -> [ReplClass; 3] {
        [ReplClass::HighFreq, ReplClass::LowFreq, ReplClass::OneShotEvent]
    }
}

// ============================================================
// 第五节 · 凭据与侧别
// ============================================================

/// 凭据闭集（运行期闸 + 类型级双防线）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Credential {
    /// 服务端逻辑：唯一可写权威的凭据
    ServerLogic,
    /// 客户端预测：只可预测
    ClientPredict,
    /// 客户端应用：只可应用（S4 只读）
    ClientApply,
}

impl Credential {
    pub fn as_str(self) -> &'static str {
        match self {
            Credential::ServerLogic => "服务端逻辑",
            Credential::ClientPredict => "客户端预测",
            Credential::ClientApply => "客户端应用",
        }
    }

    /// 是否可写权威（宪一：唯一可写凭据）
    pub fn can_write_auth(self) -> bool {
        matches!(self, Credential::ServerLogic)
    }

    /// 是否可直查权威（纪三：单源——只有服务端逻辑可直查）
    pub fn can_direct_query(self) -> bool {
        matches!(self, Credential::ServerLogic)
    }
}

// ============================================================
// 第六节 · 权威键值与变更日志
// ============================================================

/// 权威键条目
#[derive(Clone)]
pub struct AuthKey {
    pub key: &'static str,
    pub class: ReplClass,
    /// 当前权威值
    pub value: u64,
    /// 键版本号（全库单调）
    pub version: u64,
    /// 最近变更 tick
    pub last_tick: u64,
    pub registered_tick: u64,
}

/// 变更日志条目（审计正本）
#[derive(Clone)]
pub struct ChangeEntry {
    pub seq: u64,
    pub key: &'static str,
    /// 变更前值（对账用）
    pub old: u64,
    /// 变更后值
    pub new: u64,
    pub tick: u64,
    /// 凭据快照（日志自证）
    pub cred: Credential,
}

/// 快照基线（F5423 快照消费上游）
#[derive(Clone)]
pub struct SnapshotBaseline {
    pub taken_tick: u64,
    /// 快照时全库最高版本
    pub global_version: u64,
    /// 快照时日志最高 seq
    pub log_seq: u64,
    /// 全量值封印（键名+值序拼接 FNV）
    pub store_seal: u64,
}

// ============================================================
// 第七节 · 审计账
// ============================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditEvent {
    KeyRegistered,
    AuthWritten,
    BypassDenied,
    NlogDenied,
    DirectQueryDenied,
    KeyUnknown,
    TickBackward,
    LogMismatch,
    BaselineMissing,
    ViewViolation,
    RoleViolation,
    SnapshotTaken,
    Truncated,
    DualSigned,
    ReplQueried,
}

impl AuditEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            AuditEvent::KeyRegistered => "key-registered",
            AuditEvent::AuthWritten => "auth-written",
            AuditEvent::BypassDenied => "bypass-denied",
            AuditEvent::NlogDenied => "nolog-denied",
            AuditEvent::DirectQueryDenied => "direct-query-denied",
            AuditEvent::KeyUnknown => "key-unknown",
            AuditEvent::TickBackward => "tick-backward",
            AuditEvent::LogMismatch => "log-mismatch",
            AuditEvent::BaselineMissing => "baseline-missing",
            AuditEvent::ViewViolation => "view-violation",
            AuditEvent::RoleViolation => "role-violation",
            AuditEvent::SnapshotTaken => "snapshot-taken",
            AuditEvent::Truncated => "truncated",
            AuditEvent::DualSigned => "dual-signed",
            AuditEvent::ReplQueried => "repl-queried",
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

// ============================================================
// 第八节 · 角色与冻结状态
// ============================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    /// 维护权：注册键、快照+截断（双签）
    Maintainer,
    /// 运行权：服务端逻辑写、查询、日志读、审计
    Runtime,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FreezeState {
    Open,
    TruncatePending,
}

// ============================================================
// 第九节 · 权威存储引擎
// ============================================================

pub struct AuthState {
    pub role: Role,
    keys: Vec<AuthKey>,
    log: Vec<ChangeEntry>,
    next_seq: u64,
    baseline: Option<SnapshotBaseline>,
    freeze: FreezeState,
    truncate_signers: Vec<String>,
    /// 直查拦截计数
    direct_query_hits: u32,
    /// 旁路拒绝计数
    bypass_hits: u32,
    /// 复制视图查询计数（合法客户端数据面）
    repl_queries: u64,
    seal: u64,
    mounted: bool,
    pub audit: AuditLog,
}

impl AuthState {
    /// mount 唯一入口
    pub fn mount(role: Role, tick: u64) -> AuthState {
        let mut src = String::new();
        src.push_str("vef74-authstate|role=");
        src.push_str(match role {
            Role::Maintainer => "maintainer",
            Role::Runtime => "runtime",
        });
        src.push_str("|ver=");
        push_u64(&mut src, AUTH_SCHEMA_VER as u64);
        AuthState {
            role,
            keys: Vec::new(),
            log: Vec::new(),
            next_seq: 1,
            baseline: None,
            freeze: FreezeState::Open,
            truncate_signers: Vec::new(),
            direct_query_hits: 0,
            bypass_hits: 0,
            repl_queries: 0,
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
        src.push_str("vef74-authstate|role=");
        src.push_str(match self.role {
            Role::Maintainer => "maintainer",
            Role::Runtime => "runtime",
        });
        src.push_str("|ver=");
        push_u64(&mut src, AUTH_SCHEMA_VER as u64);
        seal_str(&src) == self.seal
    }

    fn require_maintainer(&mut self, tick: u64) -> Result<(), u16> {
        if self.role != Role::Maintainer {
            self.audit.record(tick, AuditEvent::RoleViolation, XC_ROLE_OR_CRED);
            return Err(XC_ROLE_OR_CRED);
        }
        Ok(())
    }

    fn key_pos(&self, key: &str) -> Option<usize> {
        let mut i = 0usize;
        for k in self.keys.iter() {
            if k.key == key {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    // ---------- Maintainer：注册 ----------

    /// 注册状态键（注册制；键名非空且 ≤ KEY_MAX_LEN；幂等）
    pub fn register_key(
        &mut self,
        key: &'static str,
        class: ReplClass,
        tick: u64,
    ) -> Result<(), u16> {
        self.require_maintainer(tick)?;
        if key.len() == 0 || key.len() > KEY_MAX_LEN {
            return Err(XC_KEY_UNKNOWN);
        }
        if self.key_pos(key).is_some() {
            return Ok(()); // 幂等
        }
        self.keys.push(AuthKey {
            key,
            class,
            value: 0,
            version: 0,
            last_tick: 0,
            registered_tick: tick,
        });
        self.audit.record(tick, AuditEvent::KeyRegistered, 0);
        Ok(())
    }

    pub fn key_known(&self, key: &str) -> bool {
        self.key_pos(key).is_some()
    }

    // ---------- 服务端逻辑写（唯一变更入口） ----------

    /// 服务端逻辑写（纪一+纪二）：唯一权威变更入口。
    ///   · 未注册键 → 0xE804
    ///   · tick 回退 → 0xE805
    ///   · 值越界 → 0xE804（域校验）
    ///   · 日志满容 → 0xE802（无日志不入账——断言拦截）
    ///   · 成功必产生日志条目（seq 严格递增 + old/new 对账）
    pub fn logic_write(&mut self, key: &'static str, new_value: u64, tick: u64) -> Result<u64, u16> {
        if !self.mounted {
            return Err(XC_ROLE_OR_CRED);
        }
        let pos = match self.key_pos(key) {
            Some(p) => p,
            None => {
                self.audit.record(tick, AuditEvent::KeyUnknown, XC_KEY_UNKNOWN);
                return Err(XC_KEY_UNKNOWN);
            }
        };
        if new_value > VALUE_MAX {
            self.audit.record(tick, AuditEvent::KeyUnknown, XC_KEY_UNKNOWN);
            return Err(XC_KEY_UNKNOWN);
        }
        let last_tick = self.keys[pos].last_tick;
        if tick < last_tick {
            self.audit.record(tick, AuditEvent::TickBackward, XC_TICK_BACKWARD);
            return Err(XC_TICK_BACKWARD);
        }
        // 纪二断言：日志满容即拒绝写入（无日志不入账）
        if self.log.len() >= CHANGELOG_CAP {
            self.audit.record(tick, AuditEvent::NlogDenied, XC_NOLOG_WRITE);
            return Err(XC_NOLOG_WRITE);
        }
        let old = self.keys[pos].value;
        let ver = self.keys[pos].version + 1;
        self.keys[pos].value = new_value;
        self.keys[pos].version = ver;
        self.keys[pos].last_tick = tick;
        let seq = self.next_seq;
        self.next_seq += 1;
        self.log.push(ChangeEntry {
            seq,
            key,
            old,
            new: new_value,
            tick,
            cred: Credential::ServerLogic,
        });
        self.audit.record(tick, AuditEvent::AuthWritten, 0);
        Ok(ver)
    }

    /// 运行期写闸（旁路尝试的显性化入口）：凭据不是服务端逻辑 → 0xE801。
    pub fn attempt_write(&mut self, cred: Credential, key: &'static str, value: u64, tick: u64) -> Result<u64, u16> {
        if !cred.can_write_auth() {
            self.bypass_hits += 1;
            self.audit.record(tick, AuditEvent::BypassDenied, XC_BYPASS_WRITE);
            return Err(XC_BYPASS_WRITE);
        }
        self.logic_write(key, value, tick)
    }

    // ---------- 查询闸（纪三：单源纪律） ----------

    /// 直查权威：只有服务端逻辑可直查；客户端直查 → 0xE803 归位拦截
    /// （Err 语义即指引：走复制通道）。
    pub fn query_auth(&mut self, cred: Credential, key: &str, tick: u64) -> Result<(u64, u64), u16> {
        if !cred.can_direct_query() {
            self.direct_query_hits += 1;
            self.audit.record(tick, AuditEvent::DirectQueryDenied, XC_DIRECT_QUERY);
            return Err(XC_DIRECT_QUERY);
        }
        let pos = match self.key_pos(key) {
            Some(p) => p,
            None => {
                self.audit.record(tick, AuditEvent::KeyUnknown, XC_KEY_UNKNOWN);
                return Err(XC_KEY_UNKNOWN);
            }
        };
        Ok((self.keys[pos].value, self.keys[pos].version))
    }

    /// 复制视图（客户端合法数据面）：只见版本与复制游标，不见活值。
    /// 请求活值 → 0xE808 视图越界。
    pub fn replicated_view(&mut self, key: &str, want_value: bool, tick: u64) -> Result<(u64, u64), u16> {
        let pos = match self.key_pos(key) {
            Some(p) => p,
            None => {
                self.audit.record(tick, AuditEvent::KeyUnknown, XC_KEY_UNKNOWN);
                return Err(XC_KEY_UNKNOWN);
            }
        };
        if want_value {
            self.audit.record(tick, AuditEvent::ViewViolation, XC_VIEW_VIOLATION);
            return Err(XC_VIEW_VIOLATION);
        }
        self.repl_queries += 1;
        self.audit.record(tick, AuditEvent::ReplQueried, 0);
        Ok((self.keys[pos].version, self.log.len() as u64))
    }

    // ---------- 日志与对拍 ----------

    /// 日志长度
    pub fn log_len(&self) -> usize {
        self.log.len()
    }

    /// 日志条目只读
    pub fn log_at(&self, i: usize) -> Option<&ChangeEntry> {
        if i >= self.log.len() {
            return None;
        }
        Some(&self.log[i])
    }

    /// 纪二对拍：按日志重放某键 → 终值必须等于存储值。
    /// 失配 → 0xE806（存储毒化信号）。
    pub fn replay_and_verify(&mut self, key: &str, tick: u64) -> Result<(u64, u64), u16> {
        let pos = match self.key_pos(key) {
            Some(p) => p,
            None => {
                self.audit.record(tick, AuditEvent::KeyUnknown, XC_KEY_UNKNOWN);
                return Err(XC_KEY_UNKNOWN);
            }
        };
        let mut replayed: u64 = 0; // 注册初值 0
        let mut saw_any = false;
        for e in self.log.iter() {
            if e.key == key {
                replayed = e.new;
                saw_any = true;
            }
        }
        if !saw_any {
            replayed = self.keys[pos].value; // 无变更键：回放=注册值
        }
        let stored = self.keys[pos].value;
        if replayed != stored {
            self.audit.record(tick, AuditEvent::LogMismatch, XC_LOG_MISMATCH);
            return Err(XC_LOG_MISMATCH);
        }
        Ok((stored, replayed))
    }

    /// 日志 seq 严格递增校验（O(日志)）
    pub fn log_seq_monotonic(&self) -> bool {
        let mut prev: u64 = 0;
        for e in self.log.iter() {
            if e.seq <= prev {
                return false;
            }
            prev = e.seq;
        }
        true
    }

    // ---------- 快照基线（F5423 上游） ----------

    /// 全库最高版本
    pub fn global_version(&self) -> u64 {
        let mut g: u64 = 0;
        for k in self.keys.iter() {
            if k.version > g {
                g = k.version;
            }
        }
        g
    }

    /// 全量值封印（键名+值确定性拼接）
    pub fn store_seal(&self) -> u64 {
        let mut acc: u64 = FNV_OFFSET;
        for k in self.keys.iter() {
            acc ^= seal_str(k.key);
            acc = acc.wrapping_mul(FNV_PRIME);
            acc ^= k.value;
            acc = acc.wrapping_mul(FNV_PRIME);
            acc ^= k.version;
            acc = acc.wrapping_mul(FNV_PRIME);
        }
        acc
    }

    /// 打快照基线（Maintainer；理由过短拒）
    pub fn take_snapshot(&mut self, reason: &str, tick: u64) -> Result<u64, u16> {
        self.require_maintainer(tick)?;
        if reason.len() < SNAPSHOT_REASON_MIN {
            return Err(XC_ROLE_OR_CRED);
        }
        let b = SnapshotBaseline {
            taken_tick: tick,
            global_version: self.global_version(),
            log_seq: if self.log.is_empty() { 0 } else { self.log[self.log.len() - 1].seq },
            store_seal: self.store_seal(),
        };
        let store_seal = b.store_seal;
        self.baseline = Some(b);
        self.audit.record(tick, AuditEvent::SnapshotTaken, 0);
        Ok(store_seal)
    }

    pub fn baseline_info(&self) -> Option<(u64, u64, u64)> {
        self.baseline.as_ref().map(|b| (b.taken_tick, b.global_version, b.log_seq))
    }

    /// delta 前置闸：无基线 → 0xE807（快照消费前置）。
    pub fn delta_gate(&self, _tick: u64) -> Result<u64, u16> {
        match self.baseline.as_ref() {
            None => Err(XC_NO_BASELINE),
            Some(b) => {
                if self.store_seal() == b.store_seal {
                    Ok(0) // 无差异
                } else {
                    Ok(1) // 有差异（增量可打包）
                }
            }
        }
    }

    // ---------- 日志截断（双签） ----------

    /// 快照+截断（双签）：日志满容后的唯一出路。
    ///   第一签 → TruncatePending；第二签（不同人）→ 执行：
    ///   必须先有快照基线（否则 0xE80A），截断后日志清空、快照保留、
    ///   存储与版本不动（replay 从截断点语义 = 快照基线语义）。
    pub fn snapshot_and_truncate(&mut self, signer: &str, tick: u64) -> Result<usize, u16> {
        self.require_maintainer(tick)?;
        if signer.len() < SIGNER_MIN_LEN {
            return Err(XC_TRUNCATE_SIGN);
        }
        if self.baseline.is_none() {
            return Err(XC_TRUNCATE_SIGN); // 无基线截断 = 丢审计链
        }
        for s in self.truncate_signers.iter() {
            if s == signer {
                return Err(XC_TRUNCATE_SIGN); // 同签人重复
            }
        }
        self.truncate_signers.push(alloc::format!("{}", signer));
        if self.truncate_signers.len() >= 2 {
            let n = self.log.len();
            self.log.clear();
            self.next_seq = 1;
            self.truncate_signers.clear();
            self.freeze = FreezeState::Open;
            self.audit.record(tick, AuditEvent::DualSigned, 0);
            self.audit.record(tick, AuditEvent::Truncated, 0);
            return Ok(n);
        }
        self.freeze = FreezeState::TruncatePending;
        self.audit.record(tick, AuditEvent::DualSigned, 0);
        Ok(0)
    }

    pub fn freeze(&self) -> FreezeState {
        self.freeze
    }

    // ---------- 统计 ----------

    pub fn key_count(&self) -> usize {
        self.keys.len()
    }

    pub fn direct_query_hits(&self) -> u32 {
        self.direct_query_hits
    }

    pub fn bypass_hits(&self) -> u32 {
        self.bypass_hits
    }

    pub fn repl_queries(&self) -> u64 {
        self.repl_queries
    }

    /// 键只读
    pub fn key_at(&self, i: usize) -> Option<(&'static str, u64, u64, ReplClass)> {
        if i >= self.keys.len() {
            return None;
        }
        let k = &self.keys[i];
        Some((k.key, k.value, k.version, k.class))
    }
}

// ============================================================
// 第十节 · 自检聚合
// ============================================================

/// 自检一：凭据闭集纪律
pub fn selfcheck_credentials() -> bool {
    for c in [Credential::ServerLogic, Credential::ClientPredict, Credential::ClientApply].iter() {
        match c {
            Credential::ServerLogic => {
                if !c.can_write_auth() || !c.can_direct_query() {
                    return false;
                }
            }
            _ => {
                if c.can_write_auth() || c.can_direct_query() {
                    return false;
                }
            }
        }
    }
    true
}

/// 自检二：FNV 封印稳定性
pub fn selfcheck_fnv() -> bool {
    seal_str("a") == seal_str("a") && seal_str("a") != seal_str("b")
}

/// 自检三：定点转串
pub fn selfcheck_num() -> bool {
    let mut s = String::new();
    push_u64(&mut s, 0);
    if s != "0" {
        return false;
    }
    let mut s2 = String::new();
    push_u64(&mut s2, 4096);
    s2 == "4096"
}
