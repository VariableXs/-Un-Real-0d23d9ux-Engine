//! CGPU-F0809 · 认证遥测（目标 300 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥300 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0809，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：认证遥测——认证生命周期事件（签发/重认证/吊销/拒绝）
//!   的遥测发射、事件环形账本、整数聚合计数器与异常率告警口径；
//!   只产数据不出网，确定性无时钟依赖。
//!
//! 设计纪律：
//! - 事件类别闭集五类：Issued / Renewed / Revoked / Denied /
//!   Anomaly；每事件带整数刻度与主体哈希（脱敏）。
//! - 事件账本定容：TELEM_CAP=100，超容覆盖最旧并计数，
//!   守恒恒等式：total_events == held + overwritten。
//! - 计数器闭集：五类各一个 u64 计数器 + 总数，恒等式：
//!   Σ五类 == total（破坏即 Err）。
//! - 告警口径：最近 LOOKBACK=32 个事件中 Denied+Anomaly 占比
///  ≥ DENY_RATE_PM(375‰) → 告警置位（整数千分比，无浮点）。
//! - 纯功能线：no_std + alloc，整数算术，确定性；
//!   诊断段 0x6FA0 本单独占，错误码 SEG_BASE+1..+N 闭集。

#![allow(dead_code)]
use alloc::vec::Vec;

/// 诊断段基址（0x6FA0，批内 CGPU-F0809 独占）。
pub const SEG_BASE: u16 = 0x6FA0;

/// 事件口径非法（刻度回退或哈希为 0）。
pub const E_EVENT: u16 = SEG_BASE + 1;
/// 事件账本为空即聚合。
pub const E_EMPTY: u16 = SEG_BASE + 2;
/// 计数器守恒破坏。
pub const E_COUNTER: u16 = SEG_BASE + 3;
/// 守恒破坏。
pub const E_LEDGER: u16 = SEG_BASE + 4;

/// 事件账本容量。
pub const TELEM_CAP: usize = 100;
/// 告警回看窗口。
pub const LOOKBACK: usize = 32;
/// 告警阈值（千分比）。
pub const DENY_RATE_PM: u32 = 375;

/// FNV-1a 64 位摘要。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 事件类别（闭集五类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvKind {
    Issued,
    Renewed,
    Revoked,
    Denied,
    Anomaly,
}

impl EvKind {
    pub fn ord(self) -> u8 {
        match self {
            EvKind::Issued => 0,
            EvKind::Renewed => 1,
            EvKind::Revoked => 2,
            EvKind::Denied => 3,
            EvKind::Anomaly => 4,
        }
    }
    /// 是否坏信号（参与告警分母/分子）。
    pub fn is_bad(self) -> bool {
        matches!(self, EvKind::Denied | EvKind::Anomaly)
    }
}

/// 遥测事件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TelemEvent {
    /// 类别。
    pub kind: EvKind,
    /// 刻度。
    pub tick: u64,
    /// 主体哈希（0 非法）。
    pub subject_hash: u64,
}

/// 事件环形账本 + 计数器。
#[derive(Clone, Debug)]
pub struct TelemRing {
    ring: Vec<Option<TelemEvent>>,
    w: usize,
    held: usize,
    overwritten: u64,
    total: u64,
    counters: [u64; 5],
    last_tick: u64,
}

impl TelemRing {
    pub fn new() -> Self {
        TelemRing {
            ring: Vec::new(),
            w: 0,
            held: 0,
            overwritten: 0,
            total: 0,
            counters: [0; 5],
            last_tick: 0,
        }
    }
    /// 发射事件：刻度必须单调不减，哈希非零。
    pub fn emit(&mut self, e: TelemEvent) -> Result<(), u16> {
        if e.subject_hash == 0 || e.tick < self.last_tick {
            return Err(E_EVENT);
        }
        if self.held < TELEM_CAP {
            self.ring.push(Some(e));
            self.held += 1;
        } else {
            self.ring[self.w] = Some(e);
            self.overwritten += 1;
        }
        self.w = (self.w + 1) % TELEM_CAP;
        self.total += 1;
        self.counters[e.kind.ord() as usize] += 1;
        self.last_tick = e.tick;
        Ok(())
    }
    /// 守恒审计：total == held + overwritten；Σ计数 == total。
    pub fn audit(&self) -> Result<(), u16> {
        if self.total != self.held as u64 + self.overwritten {
            return Err(E_LEDGER);
        }
        let sum: u64 = self.counters.iter().sum();
        if sum != self.total {
            return Err(E_COUNTER);
        }
        Ok(())
    }
    /// 视图（旧→新）。
    pub fn view(&self) -> Vec<TelemEvent> {
        let start = if self.held < TELEM_CAP { 0 } else { self.w };
        let mut out = Vec::with_capacity(self.held);
        for i in 0..self.held {
            let idx = (start + i) % TELEM_CAP;
            if let Some(Some(e)) = self.ring.get(idx) {
                out.push(*e);
            }
        }
        out
    }
    /// 告警口径：最近 LOOKBACK 个事件中坏信号千分比 ≥ 阈值。
    pub fn alarm(&self) -> Result<bool, u16> {
        let v = self.view();
        if v.is_empty() {
            return Err(E_EMPTY);
        }
        let n = v.len().min(LOOKBACK);
        let tail = &v[v.len() - n..];
        let bad = tail.iter().filter(|e| e.kind.is_bad()).count();
        let rate = (bad * 1000) / n;
        Ok(rate >= DENY_RATE_PM as usize)
    }
    pub fn total(&self) -> u64 {
        self.total
    }
    pub fn held(&self) -> usize {
        self.held
    }
    pub fn counter(&self, k: EvKind) -> u64 {
        self.counters[k.ord() as usize]
    }
}

/// 认证遥测会话：把生命周期动作映射为事件发射（闭集四动作）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CertAction {
    Issue,
    Renew,
    Revoke,
    Deny,
}

impl CertAction {
    /// 动作 → 事件类别（闭集映射）。
    pub fn to_kind(self) -> EvKind {
        match self {
            CertAction::Issue => EvKind::Issued,
            CertAction::Renew => EvKind::Renewed,
            CertAction::Revoke => EvKind::Revoked,
            CertAction::Deny => EvKind::Denied,
        }
    }
}

/// 动作发射封装。
pub fn emit_action(ring: &mut TelemRing, act: CertAction, tick: u64, subject: &str) -> Result<(), u16> {
    let h = fnv1a64(subject.as_bytes());
    if h == 0 {
        return Err(E_EVENT);
    }
    ring.emit(TelemEvent {
        kind: act.to_kind(),
        tick,
        subject_hash: h,
    })
}

/// 自例程：主路径（正常流不告警）+ 告警流 + 覆盖守恒 + 错误路径。
pub fn drill() -> Result<u32, u16> {
    // 主路径 1：正常流——签发 20 + 重认证 10 + 吊销 2 → 坏信号占比低 → 不告警。
    let mut r = TelemRing::new();
    for i in 0..20u64 {
        emit_action(&mut r, CertAction::Issue, i, "agent-a")?;
    }
    for i in 20..30u64 {
        emit_action(&mut r, CertAction::Renew, i, "agent-a")?;
    }
    for i in 30..32u64 {
        emit_action(&mut r, CertAction::Revoke, i, "agent-a")?;
    }
    r.audit()?;
    if r.total() != 32 || r.counter(EvKind::Issued) != 20 || r.counter(EvKind::Renewed) != 10 {
        return Err(E_COUNTER);
    }
    if r.alarm()? {
        return Err(E_COUNTER);
    }
    // 主路径 2：坏信号流——最近 32 个中 12+ 个 Denied → 告警。
    // 32 个事件中需要 bad*1000/32 >= 375 → bad >= 12。
    let mut r2 = TelemRing::new();
    for i in 0..20u64 {
        emit_action(&mut r2, CertAction::Issue, i, "agent-b")?;
    }
    for i in 20..32u64 {
        emit_action(&mut r2, CertAction::Deny, i, "agent-b")?;
    }
    // 12 bad / 32 = 375‰ → 恰好告警。
    if !r2.alarm()? {
        return Err(E_COUNTER);
    }
    // 边界：11 bad / 32 = 343‰ → 不告警。
    let mut r3 = TelemRing::new();
    for i in 0..21u64 {
        emit_action(&mut r3, CertAction::Issue, i, "agent-c")?;
    }
    for i in 21..32u64 {
        emit_action(&mut r3, CertAction::Deny, i, "agent-c")?;
    }
    if r3.alarm()? {
        return Err(E_COUNTER);
    }
    // 主路径 3：定容覆盖——灌 130 个事件进 100 容账本。
    let mut r4 = TelemRing::new();
    for i in 0..130u64 {
        emit_action(&mut r4, CertAction::Issue, i, "agent-d")?;
    }
    r4.audit()?;
    if r4.held() != TELEM_CAP || r4.counter(EvKind::Issued) != 130 {
        return Err(E_LEDGER);
    }
    // 覆盖后最旧事件 tick=30。
    let v = r4.view();
    if v[0].tick != 30 || v[99].tick != 129 {
        return Err(E_LEDGER);
    }
    // 错误路径 1：刻度回退。
    let mut r5 = TelemRing::new();
    emit_action(&mut r5, CertAction::Issue, 10, "agent-e")?;
    if emit_action(&mut r5, CertAction::Issue, 9, "agent-e") != Err(E_EVENT) {
        return Err(E_EVENT);
    }
    // 错误路径 2：空账本告警。
    if TelemRing::new().alarm() != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    // 错误路径 3：Anomaly 也计坏信号。
    let mut r6 = TelemRing::new();
    r6.emit(TelemEvent {
        kind: EvKind::Anomaly,
        tick: 1,
        subject_hash: fnv1a64(b"agent-f"),
    })?;
    if !r6.alarm()? {
        return Err(E_COUNTER);
    }
    // 闭集映射核对。
    if CertAction::Issue.to_kind() != EvKind::Issued
        || CertAction::Renew.to_kind() != EvKind::Renewed
        || CertAction::Revoke.to_kind() != EvKind::Revoked
        || CertAction::Deny.to_kind() != EvKind::Denied
    {
        return Err(E_EVENT);
    }
    Ok(0)
}
