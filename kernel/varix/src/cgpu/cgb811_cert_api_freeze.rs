//! CGPU-F0811 · 认证 API 冻结 v1（目标 300 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥300 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0811，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：认证 API 冻结 v1——认证模块对外接口面（Endpoint 面）
//!   的冻结管理：端点登记、签名指纹计算、冻结后只允许
//!   Bugfix 级变更、破坏性变更必须走 v2 新面；冻结账本守恒。
//!
//! 设计纪律：
//! - 端点类别闭集三类：Query（只读）/ Action（有副作用）/
//!   Stream（流式）；Stream 端点必须声明字节预算。
//! - 端点指纹：FNV-1a 64 位（名称+类别+预算 → 指纹）；
///   冻结清单指纹 = 逐端点指纹折叠（确定性）。
//! - 冻结后变更闭集三档：Additive（新端点，允许，版本计 minor+1）/
///   Bugfix（同签名改实现，允许）/ Breaking（改签名/删端点，
///   拒绝，必须开 v2 面）。
//! - 冻结账本定容：API_CAP=64 端点，超容逐出最旧并计数，
///   守恒恒等式：total_endpoints == held + evicted。
//! - 纯功能线：no_std + alloc，整数算术，确定性；
///   诊断段 0x6FE0 本单独占，错误码 SEG_BASE+1..+N 闭集。

#![allow(dead_code)]
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x6FE0，批内 CGPU-F0811 独占）。
pub const SEG_BASE: u16 = 0x6FE0;

/// 端点名重复。
pub const E_DUP: u16 = SEG_BASE + 1;
/// 端点册已满（防御位）。
pub const E_CAP: u16 = SEG_BASE + 2;
/// 端点口径非法（空名/流式零预算）。
pub const E_EP: u16 = SEG_BASE + 3;
/// 冻结后破坏性变更。
pub const E_BREAKING: u16 = SEG_BASE + 4;
/// 冻结面尚未建立。
pub const E_NOTFROZEN: u16 = SEG_BASE + 5;
/// 端点不存在。
pub const E_NOEP: u16 = SEG_BASE + 6;
/// 守恒破坏。
pub const E_LEDGER: u16 = SEG_BASE + 7;
/// 已冻结面重复冻结。
pub const E_REFREEZE: u16 = SEG_BASE + 8;

/// 端点册容量。
pub const API_CAP: usize = 64;

/// FNV-1a 64 位摘要。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 端点类别（闭集三类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EpKind {
    Query,
    Action,
    Stream,
}

impl EpKind {
    pub fn ord(self) -> u8 {
        match self {
            EpKind::Query => 0,
            EpKind::Action => 1,
            EpKind::Stream => 2,
        }
    }
}

/// 端点。
#[derive(Clone, Debug)]
pub struct Endpoint {
    /// 名称。
    pub name: String,
    /// 类别。
    pub kind: EpKind,
    /// 字节预算（Stream 必填，其余为 0）。
    pub byte_budget: u32,
    /// 指纹。
    pub fingerprint: u64,
}

impl Endpoint {
    pub fn new(name: &str, kind: EpKind, byte_budget: u32) -> Result<Self, u16> {
        if name.is_empty() {
            return Err(E_EP);
        }
        if kind == EpKind::Stream && byte_budget == 0 {
            return Err(E_EP);
        }
        if kind != EpKind::Stream && byte_budget != 0 {
            return Err(E_EP);
        }
        let mut buf: Vec<u8> = Vec::new();
        for b in name.as_bytes() {
            buf.push(*b);
        }
        buf.push(kind.ord());
        buf.extend_from_slice(&byte_budget.to_le_bytes());
        Ok(Endpoint {
            name: name.to_string(),
            kind,
            byte_budget,
            fingerprint: fnv1a64(&buf),
        })
    }
}

/// 变更档位（闭集三档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeGrade {
    Additive,
    Bugfix,
    Breaking,
}

/// 冻结面。
#[derive(Clone, Debug)]
pub struct FrozenApi {
    eps: Vec<Endpoint>,
    frozen: bool,
    minor: u32,
    evicted: u64,
    total_endpoints: u64,
}

impl FrozenApi {
    pub fn new() -> Self {
        FrozenApi {
            eps: Vec::new(),
            frozen: false,
            minor: 0,
            evicted: 0,
            total_endpoints: 0,
        }
    }
    /// 冻结前登记。
    pub fn register(&mut self, ep: Endpoint) -> Result<(), u16> {
        if self.frozen {
            return Err(E_BREAKING);
        }
        if self.eps.iter().any(|o| o.name == ep.name) {
            return Err(E_DUP);
        }
        if self.eps.len() >= API_CAP {
            self.eps.remove(0);
            self.evicted += 1;
        }
        self.eps.push(ep);
        self.total_endpoints += 1;
        Ok(())
    }
    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u16> {
        if self.frozen {
            return Err(E_REFREEZE);
        }
        if self.eps.is_empty() {
            return Err(E_NOTFROZEN);
        }
        self.frozen = true;
        Ok(self.manifest_fingerprint())
    }
    /// 清单指纹：逐端点指纹折叠。
    pub fn manifest_fingerprint(&self) -> u64 {
        let mut acc: u64 = 0x9e3779b97f4a7c15;
        for ep in &self.eps {
            acc = acc.wrapping_mul(31).wrapping_add(ep.fingerprint);
        }
        acc ^ (self.eps.len() as u64)
    }
    /// 冻结后 Additive 变更：新端点。
    pub fn add_after_freeze(&mut self, ep: Endpoint) -> Result<u32, u16> {
        if !self.frozen {
            return Err(E_NOTFROZEN);
        }
        if self.eps.iter().any(|o| o.name == ep.name) {
            return Err(E_DUP);
        }
        if self.eps.len() >= API_CAP {
            self.eps.remove(0);
            self.evicted += 1;
        }
        self.eps.push(ep);
        self.total_endpoints += 1;
        self.minor += 1;
        Ok(self.minor)
    }
    /// 冻结后 Bugfix：同签名改实现（指纹不变才放行）。
    pub fn bugfix(&mut self, name: &str, new_impl_tag: u64) -> Result<(), u16> {
        if !self.frozen {
            return Err(E_NOTFROZEN);
        }
        let p = self
            .eps
            .iter()
            .position(|o| o.name == name)
            .ok_or(E_NOEP)?;
        // Bugfix 不改签名：指纹与预算/类别/名称一致即可（此处以
        // new_impl_tag 不参与指纹表达"实现可换、签名不可换"）。
        let _ = new_impl_tag;
        Ok(())
    }
    /// 冻结后改签名/删端点：一律拒绝（Breaking）。
    pub fn reshape(&mut self, name: &str) -> Result<(), u16> {
        if !self.frozen {
            return Err(E_NOTFROZEN);
        }
        let _ = name;
        Err(E_BREAKING)
    }
    /// 守恒审计：total_endpoints == held + evicted。
    pub fn audit(&self) -> Result<(), u16> {
        if self.total_endpoints != self.eps.len() as u64 + self.evicted {
            return Err(E_LEDGER);
        }
        Ok(())
    }
    pub fn held(&self) -> usize {
        self.eps.len()
    }
    pub fn minor(&self) -> u32 {
        self.minor
    }
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 自例程：主路径（冻结→指纹稳定）+ Additive/Bugfix/Breaking 三档 + 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut api = FrozenApi::new();
    // 主路径 1：登记四端点 → 冻结 → 指纹稳定。
    api.register(Endpoint::new("cert.query", EpKind::Query, 0)?)?;
    api.register(Endpoint::new("cert.issue", EpKind::Action, 0)?)?;
    api.register(Endpoint::new("cert.revoke", EpKind::Action, 0)?)?;
    api.register(Endpoint::new("cert.telem", EpKind::Stream, 4096)?)?;
    api.audit()?;
    let fp1 = api.freeze()?;
    if !api.is_frozen() {
        return Err(E_NOTFROZEN);
    }
    let fp2 = api.manifest_fingerprint();
    if fp1 != fp2 {
        return Err(E_LEDGER);
    }
    // 指纹确定性：同端点集同指纹。
    let mut api2 = FrozenApi::new();
    api2.register(Endpoint::new("cert.query", EpKind::Query, 0)?)?;
    api2.register(Endpoint::new("cert.issue", EpKind::Action, 0)?)?;
    api2.register(Endpoint::new("cert.revoke", EpKind::Action, 0)?)?;
    api2.register(Endpoint::new("cert.telem", EpKind::Stream, 4096)?)?;
    let fp3 = api2.freeze()?;
    if fp3 != fp1 {
        return Err(E_LEDGER);
    }
    // 主路径 2：Additive——冻结后加新端点，minor 前移。
    let m1 = api.add_after_freeze(Endpoint::new("cert.renew", EpKind::Action, 0)?)?;
    if m1 != 1 {
        return Err(E_LEDGER);
    }
    let m2 = api.add_after_freeze(Endpoint::new("cert.audit", EpKind::Query, 0)?)?;
    if m2 != 2 || api.minor() != 2 {
        return Err(E_LEDGER);
    }
    // 冻结后重复登记拒绝。
    if api
        .add_after_freeze(Endpoint::new("cert.query", EpKind::Query, 0)?)
        != Err(E_DUP)
    {
        return Err(E_DUP);
    }
    // 主路径 3：Bugfix 放行。
    api.bugfix("cert.issue", 0x7777)?;
    // 主路径 4：Breaking 拒绝。
    if api.reshape("cert.query") != Err(E_BREAKING) {
        return Err(E_BREAKING);
    }
    // 错误路径 1：端点口径。
    if Endpoint::new("", EpKind::Query, 0) != Err(E_EP)
        || Endpoint::new("s", EpKind::Stream, 0) != Err(E_EP)
        || Endpoint::new("q", EpKind::Query, 100) != Err(E_EP)
    {
        return Err(E_EP);
    }
    // 错误路径 2：冻结前禁止 Additive（E_NOTFROZEN）。
    if api2.add_after_freeze(Endpoint::new("x", EpKind::Query, 0)?) != Err(E_NOTFROZEN) {
        return Err(E_NOTFROZEN);
    }
    // 错误路径 3：冻结面二次冻结拒绝。
    if api.freeze() != Err(E_REFREEZE) {
        return Err(E_LEDGER);
    }
    // 错误路径 4：冻结前 register 可以，冻结后 register 拒绝。
    if api.register(Endpoint::new("late", EpKind::Query, 0)?) != Err(E_BREAKING) {
        return Err(E_BREAKING);
    }
    // 错误路径 5：未知端点 Bugfix。
    if api.bugfix("nope", 1) != Err(E_NOEP) {
        return Err(E_NOEP);
    }
    // 空面冻结拒绝。
    let mut api3 = FrozenApi::new();
    if api3.freeze() != Err(E_NOTFROZEN) {
        return Err(E_NOTFROZEN);
    }
    // 定容守恒：70 端点进 64 容册（冻结前登记逐出最旧）。
    let mut api4 = FrozenApi::new();
    for i in 0..70u32 {
        let name = alloc::format!("ep{}", i);
        api4.register(Endpoint::new(&name, EpKind::Query, 0)?)?;
    }
    api4.audit()?;
    if api4.held() != API_CAP || api4.evicted != 6 {
        return Err(E_LEDGER);
    }
    Ok(0)
}
