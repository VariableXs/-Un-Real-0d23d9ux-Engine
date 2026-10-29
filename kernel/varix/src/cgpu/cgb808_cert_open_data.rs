//! CGPU-F0808 · 认证数据开放（目标 320 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥320 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0808，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：认证数据开放——把认证记录按开放等级（开放等级闭集）
//!   分级发布：导出过滤（只出允许字段）、访问记账、
//!   配额管控与开放账本守恒；敏感字段永不导出。
//!
//! 设计纪律：
//! - 开放等级闭集三级：Open（全字段）/ Metered（计费字段子集）/
//!   Restricted（仅存在性，不出内容）。
//! - 导出闭集五字段：cert_id（公开）/ subject_hash（公开）/
//!   verdict（公开）/ sign_count（Metered 及以上）/
//!   raw_subject（仅 Open）；Restricted 只出 cert_id 存在位。
//! - 访问账本定容：ACCESS_CAP=128，超容逐出最旧并计数，
//!   守恒恒等式：total_accesses == held + evicted。
//! - 配额口径：单请求者每刻度窗 QUOTA_WINDOW=16 内最多
//!   QUOTA_LIMIT=8 次访问，超限拒绝（429 口径）。
//! - 纯功能线：no_std + alloc，整数算术，确定性；
//!   诊断段 0x6F80 本单独占，错误码 SEG_BASE+1..+N 闭集。

#![allow(dead_code)]
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x6F80，批内 CGPU-F0808 独占）。
pub const SEG_BASE: u16 = 0x6F80;

/// 记录编号重复。
pub const E_DUP: u16 = SEG_BASE + 1;
/// 访问账本已满（防御位）。
pub const E_CAP: u16 = SEG_BASE + 2;
/// 开放等级或字段口径非法。
pub const E_LEVEL: u16 = SEG_BASE + 3;
/// 请求者不在册或记录不存在。
pub const E_NOREC: u16 = SEG_BASE + 4;
/// 配额超限。
pub const E_QUOTA: u16 = SEG_BASE + 5;
/// 守恒破坏。
pub const E_LEDGER: u16 = SEG_BASE + 6;

/// 访问账本容量。
pub const ACCESS_CAP: usize = 128;
/// 配额窗口（刻度）。
pub const QUOTA_WINDOW: u64 = 16;
/// 配额上限（次/窗）。
pub const QUOTA_LIMIT: u32 = 8;

/// FNV-1a 64 位摘要（脱敏口径）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 开放等级（闭集三级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenLevel {
    Open,
    Metered,
    Restricted,
}

impl OpenLevel {
    pub fn ord(self) -> u8 {
        match self {
            OpenLevel::Open => 0,
            OpenLevel::Metered => 1,
            OpenLevel::Restricted => 2,
        }
    }
}

/// 认证开放记录。
#[derive(Clone, Debug)]
pub struct OpenRecord {
    /// 证书编号。
    pub cert_id: u32,
    /// 主体哈希（脱敏：不存原文）。
    pub subject_hash: u64,
    /// 认证结论（0=过 1=缓 2=拒）。
    pub verdict: u8,
    /// 签发次数（Metered 字段）。
    pub sign_count: u32,
    /// 开放等级。
    pub level: OpenLevel,
}

impl OpenRecord {
    /// 构造：原文只用于算哈希，不入账。
    pub fn new(cert_id: u32, raw_subject: &str, verdict: u8, sign_count: u32, level: OpenLevel) -> Result<Self, u16> {
        if raw_subject.is_empty() || verdict > 2 {
            return Err(E_LEVEL);
        }
        Ok(OpenRecord {
            cert_id,
            subject_hash: fnv1a64(raw_subject.as_bytes()),
            verdict,
            sign_count,
            level,
        })
    }
}

/// 导出载荷：字段按开放等级填充（None=不导出）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportPayload {
    pub cert_id: Option<u32>,
    pub subject_hash: Option<u64>,
    pub verdict: Option<u8>,
    pub sign_count: Option<u32>,
    pub raw_subject: Option<String>,
}

/// 导出过滤：等级闭集判定。
pub fn export(rec: &OpenRecord) -> Result<ExportPayload, u16> {
    Ok(match rec.level {
        OpenLevel::Open => ExportPayload {
            cert_id: Some(rec.cert_id),
            subject_hash: Some(rec.subject_hash),
            verdict: Some(rec.verdict),
            sign_count: Some(rec.sign_count),
            raw_subject: None, // 原文从未入账，任何等级都不出。
        },
        OpenLevel::Metered => ExportPayload {
            cert_id: Some(rec.cert_id),
            subject_hash: Some(rec.subject_hash),
            verdict: Some(rec.verdict),
            sign_count: Some(rec.sign_count),
            raw_subject: None,
        },
        OpenLevel::Restricted => ExportPayload {
            cert_id: Some(rec.cert_id),
            subject_hash: None,
            verdict: None,
            sign_count: None,
            raw_subject: None,
        },
    })
}

/// 访问请求者。
#[derive(Clone, Debug)]
pub struct Requester {
    /// 请求者名。
    pub name: String,
    /// 最近访问刻度序列（定容 QUOTA_LIMIT 即可判定）。
    recent: Vec<u64>,
}

impl Requester {
    pub fn new(name: &str) -> Result<Self, u16> {
        if name.is_empty() {
            return Err(E_LEVEL);
        }
        Ok(Requester {
            name: name.to_string(),
            recent: Vec::new(),
        })
    }
    /// 配额判定：窗口 [t-QUOTA_WINDOW+1, t] 内次数 < LIMIT。
    pub fn allow(&mut self, t: u64) -> Result<(), u16> {
        self.recent.retain(|x| *x + QUOTA_WINDOW > t);
        if self.recent.len() as u32 >= QUOTA_LIMIT {
            return Err(E_QUOTA);
        }
        self.recent.push(t);
        Ok(())
    }
    pub fn recent_count(&self) -> u32 {
        self.recent.len() as u32
    }
}

/// 访问记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccessRec {
    pub cert_id: u32,
    pub requester_hash: u64,
    pub tick: u64,
}

/// 开放账本（数据册 + 访问环形账本）。
#[derive(Clone, Debug)]
pub struct OpenLedger {
    records: Vec<OpenRecord>,
    access_ring: Vec<AccessRec>,
    access_evicted: u64,
    total_accesses: u64,
    requesters: Vec<Requester>,
}

impl OpenLedger {
    pub fn new() -> Self {
        OpenLedger {
            records: Vec::new(),
            access_ring: Vec::new(),
            access_evicted: 0,
            total_accesses: 0,
            requesters: Vec::new(),
        }
    }
    /// 发布记录。
    pub fn publish(&mut self, r: OpenRecord) -> Result<(), u16> {
        if self.records.iter().any(|o| o.cert_id == r.cert_id) {
            return Err(E_DUP);
        }
        self.records.push(r);
        Ok(())
    }
    /// 注册请求者。
    pub fn add_requester(&mut self, name: &str) -> Result<(), u16> {
        if name.is_empty() || self.requesters.iter().any(|r| r.name == name) {
            return Err(E_DUP);
        }
        self.requesters.push(Requester::new(name)?);
        Ok(())
    }
    /// 开放访问：配额 + 等级过滤 + 记账。
    pub fn access(&mut self, requester: &str, cert_id: u32, t: u64) -> Result<ExportPayload, u16> {
        let rp = self
            .requesters
            .iter_mut()
            .find(|r| r.name == requester)
            .ok_or(E_NOREC)?;
        rp.allow(t)?;
        let rec = self
            .records
            .iter()
            .find(|r| r.cert_id == cert_id)
            .ok_or(E_NOREC)?;
        let payload = export(rec)?;
        // 记账（环形定容）。
        if self.access_ring.len() >= ACCESS_CAP {
            self.access_ring.remove(0);
            self.access_evicted += 1;
        }
        self.access_ring.push(AccessRec {
            cert_id,
            requester_hash: fnv1a64(requester.as_bytes()),
            tick: t,
        });
        self.total_accesses += 1;
        Ok(payload)
    }
    /// 守恒审计：total_accesses == held + evicted。
    pub fn audit(&self) -> Result<(), u16> {
        if self.total_accesses != self.access_ring.len() as u64 + self.access_evicted {
            return Err(E_LEDGER);
        }
        Ok(())
    }
    pub fn access_held(&self) -> usize {
        self.access_ring.len()
    }
    pub fn record_count(&self) -> usize {
        self.records.len()
    }
}

/// 自例程：主路径（三等级导出过滤）+ 配额路径 + 错误路径 + 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut led = OpenLedger::new();
    // 发布三条不同等级记录。
    let r_open = OpenRecord::new(1, "agent-alpha", 0, 12, OpenLevel::Open)?;
    let r_meter = OpenRecord::new(2, "agent-beta", 1, 7, OpenLevel::Metered)?;
    let r_restr = OpenRecord::new(3, "agent-gamma", 2, 3, OpenLevel::Restricted)?;
    led.publish(r_open)?;
    led.publish(r_meter)?;
    led.publish(r_restr)?;
    if led.publish(OpenRecord::new(1, "dup", 0, 0, OpenLevel::Open)?) != Err(E_DUP) {
        return Err(E_DUP);
    }
    if led.record_count() != 3 {
        return Err(E_CAP);
    }
    led.add_requester("auditor")?;
    if led.add_requester("auditor") != Err(E_DUP) || led.add_requester("") != Err(E_DUP) {
        return Err(E_DUP);
    }
    // 主路径 1：Open 导出——四字段有值，raw_subject 恒 None。
    let p1 = led.access("auditor", 1, 0)?;
    if p1.cert_id != Some(1)
        || p1.subject_hash.is_none()
        || p1.verdict != Some(0)
        || p1.sign_count != Some(12)
        || p1.raw_subject != None
    {
        return Err(E_LEVEL);
    }
    // 主路径 2：Metered 导出——同 Open 口径（等级差在计费侧，不在字段侧）。
    let p2 = led.access("auditor", 2, 1)?;
    if p2.sign_count != Some(7) || p2.verdict != Some(1) {
        return Err(E_LEVEL);
    }
    // 主路径 3：Restricted——只出 cert_id。
    let p3 = led.access("auditor", 3, 2)?;
    if p3.cert_id != Some(3)
        || p3.subject_hash != None
        || p3.verdict != None
        || p3.sign_count != None
    {
        return Err(E_LEVEL);
    }
    // 脱敏核验：同原文同哈希、异原文异哈希。
    let h1 = fnv1a64(b"agent-alpha");
    let h2 = fnv1a64(b"agent-alpha");
    let h3 = fnv1a64(b"agent-beta");
    if h1 != h2 || h1 == h3 {
        return Err(E_LEVEL);
    }
    // 配额路径：连发 8 次后第 9 次拒绝。
    let mut led2 = OpenLedger::new();
    led2.publish(OpenRecord::new(9, "sub", 0, 0, OpenLevel::Open)?)?;
    led2.add_requester("bot")?;
    for i in 0..QUOTA_LIMIT {
        led2.access("bot", 9, i as u64)?;
    }
    if led2.access("bot", 9, (QUOTA_LIMIT) as u64) != Err(E_QUOTA) {
        return Err(E_QUOTA);
    }
    // 窗口滑出：t=16 时最早访问（t=0）出窗（0+16>16 为假）→ 放行。
    led2.access("bot", 9, QUOTA_WINDOW)?;
    led2.audit()?;
    // 错误路径：未知请求者 / 未知记录。
    if led.access("nobody", 1, 0) != Err(E_NOREC) || led.access("auditor", 99, 0) != Err(E_NOREC) {
        return Err(E_NOREC);
    }
    // 构造口径非法。
    if OpenRecord::new(8, "", 0, 0, OpenLevel::Open) != Err(E_LEVEL)
        || OpenRecord::new(8, "s", 3, 0, OpenLevel::Open) != Err(E_LEVEL)
        || Requester::new("") != Err(E_LEVEL)
    {
        return Err(E_LEVEL);
    }
    // 定容守恒：灌 140 条访问进 128 容账本。
    let mut led3 = OpenLedger::new();
    led3.publish(OpenRecord::new(5, "s", 0, 0, OpenLevel::Open)?)?;
    led3.add_requester("flood")?;
    // 配额会挡：用多请求者轮转避开配额。
    for i in 0..10u32 {
        let name = alloc::format!("f{}", i);
        led3.add_requester(&name)?;
    }
    for i in 0..140u64 {
        let name = alloc::format!("f{}", (i % 10) as u32);
        led3.access(&name, 5, i)?;
    }
    led3.audit()?;
    if led3.access_held() != ACCESS_CAP || led3.access_evicted != 12 {
        return Err(E_LEDGER);
    }
    Ok(0)
}
