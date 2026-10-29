//! CGPU-F0807 · 认证有效期与重认证（目标 320 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥320 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0807，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：认证有效期与重认证——证书（Cert）生命周期中的
//!   有效窗口管理：宽限期判定、到期重认证、吊销与刷新账本；
//!   全部时间用整数刻度（tick），窗口闭集判定。
//!
//! 设计纪律：
//! - 有效窗口闭集三段：Valid（issue ≤ t < expiry）/ Grace
//!   （expiry ≤ t < expiry+GRACE_TICKS，仍可用但必须排队重认证）/
//!   Expired（t ≥ expiry+GRACE_TICKS，拒绝服务）。
//! - 重认证：Valid→Renewed（新窗口 = 旧 expiry + VALID_TICKS），
//!   每证书重认证次数上限 RENEW_LIMIT，超限必须换发（口径外拒绝）。
//! - 吊销（Revoke）为终态：任何状态下吊销即失效，不可逆。
//! - 证书账本定容：CERT_CAP=48，超容逐出最旧终态证书
//!   （无则拒绝），守恒恒等式：total_certs == held + evicted。
//! - 纯功能线：no_std + alloc，整数算术，确定性；
//!   诊断段 0x6F60 本单独占，错误码 SEG_BASE+1..+N 闭集。

#![allow(dead_code)]
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x6F60，批内 CGPU-F0807 独占）。
pub const SEG_BASE: u16 = 0x6F60;

/// 证书编号重复。
pub const E_DUP: u16 = SEG_BASE + 1;
/// 证书账本已满且无可逐出终态项。
pub const E_CAP: u16 = SEG_BASE + 2;
/// 生命周期操作非法（状态机跳步）。
pub const E_STATE: u16 = SEG_BASE + 3;
/// 未知证书。
pub const E_NOCERT: u16 = SEG_BASE + 4;
/// 口径非法（窗口长度为 0）。
pub const E_WINDOW: u16 = SEG_BASE + 5;
/// 已过宽限期（拒绝服务）。
pub const E_EXPIRED: u16 = SEG_BASE + 6;
/// 重认证次数超限。
pub const E_RENEW: u16 = SEG_BASE + 7;
/// 守恒破坏。
pub const E_LEDGER: u16 = SEG_BASE + 8;

/// 证书账本容量。
pub const CERT_CAP: usize = 48;
/// 有效期（刻度）。
pub const VALID_TICKS: u64 = 720;
/// 宽限期（刻度）。
pub const GRACE_TICKS: u64 = 48;
/// 重认证次数上限。
pub const RENEW_LIMIT: u32 = 4;

/// 有效窗口段位（闭集三段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowPhase {
    /// 有效。
    Valid,
    /// 宽限。
    Grace,
    /// 已失效。
    Expired,
}

impl WindowPhase {
    pub fn ord(self) -> u8 {
        match self {
            WindowPhase::Valid => 0,
            WindowPhase::Grace => 1,
            WindowPhase::Expired => 2,
        }
    }
}

/// 证书状态（闭集四态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CertState {
    Active,
    Revoked,
    Retired,
}

impl CertState {
    pub fn is_final(self) -> bool {
        matches!(self, CertState::Revoked | CertState::Retired)
    }
    pub fn ord(self) -> u8 {
        match self {
            CertState::Active => 0,
            CertState::Revoked => 1,
            CertState::Retired => 2,
        }
    }
}

/// 证书。
#[derive(Clone, Debug)]
pub struct Cert {
    /// 编号。
    pub id: u32,
    /// 持有者。
    pub subject: String,
    /// 签发时刻。
    pub issued_at: u64,
    /// 当前到期时刻（重认证会前移）。
    pub expires_at: u64,
    /// 状态。
    pub state: CertState,
    /// 重认证次数。
    pub renews: u32,
}

impl Cert {
    /// 窗口段位判定（闭集三段）。
    pub fn phase_at(&self, t: u64) -> WindowPhase {
        if t < self.expires_at {
            WindowPhase::Valid
        } else if t < self.expires_at + GRACE_TICKS {
            WindowPhase::Grace
        } else {
            WindowPhase::Expired
        }
    }
    /// 服务可用性：Active 且非 Expired 段。
    pub fn usable_at(&self, t: u64) -> bool {
        self.state == CertState::Active && self.phase_at(t) != WindowPhase::Expired
    }
}

/// 证书账本。
#[derive(Clone, Debug)]
pub struct CertLedger {
    ring: Vec<Cert>,
    evicted: u64,
    total_certs: u64,
}

impl CertLedger {
    pub fn new() -> Self {
        CertLedger {
            ring: Vec::new(),
            evicted: 0,
            total_certs: 0,
        }
    }
    /// 签发：窗口 [t, t+VALID_TICKS)。
    pub fn issue(&mut self, id: u32, subject: &str, t: u64) -> Result<(), u16> {
        if subject.is_empty() {
            return Err(E_WINDOW);
        }
        if self.ring.iter().any(|c| c.id == id) {
            return Err(E_DUP);
        }
        if self.ring.len() >= CERT_CAP {
            let p = self.ring.iter().position(|c| c.state.is_final());
            match p {
                Some(idx) => {
                    self.ring.remove(idx);
                    self.evicted += 1;
                }
                None => return Err(E_CAP),
            }
        }
        self.ring.push(Cert {
            id,
            subject: subject.to_string(),
            issued_at: t,
            expires_at: t + VALID_TICKS,
            state: CertState::Active,
            renews: 0,
        });
        self.total_certs += 1;
        Ok(())
    }
    fn pos(&self, id: u32) -> Result<usize, u16> {
        self.ring.iter().position(|c| c.id == id).ok_or(E_NOCERT)
    }
    /// 重认证：Active + （Valid 或 Grace 段）才能重认证；
    /// Expired 段必须换发（拒绝）。
    pub fn renew(&mut self, id: u32, t: u64) -> Result<u64, u16> {
        let p = self.pos(id)?;
        let c = &mut self.ring[p];
        if c.state != CertState::Active {
            return Err(E_STATE);
        }
        if c.phase_at(t) == WindowPhase::Expired {
            return Err(E_EXPIRED);
        }
        if c.renews >= RENEW_LIMIT {
            return Err(E_RENEW);
        }
        c.renews += 1;
        c.expires_at += VALID_TICKS;
        Ok(c.expires_at)
    }
    /// 吊销：任何状态均可吊销（终态）。
    pub fn revoke(&mut self, id: u32) -> Result<(), u16> {
        let p = self.pos(id)?;
        self.ring[p].state = CertState::Revoked;
        Ok(())
    }
    /// 退役：Active → Retired（正常换发路径）。
    pub fn retire(&mut self, id: u32) -> Result<(), u16> {
        let p = self.pos(id)?;
        if self.ring[p].state != CertState::Active {
            return Err(E_STATE);
        }
        self.ring[p].state = CertState::Retired;
        Ok(())
    }
    /// 访问判定：可用性 + 段位。
    pub fn access(&self, id: u32, t: u64) -> Result<WindowPhase, u16> {
        let p = self.pos(id)?;
        let c = &self.ring[p];
        if !c.usable_at(t) {
            return Err(E_EXPIRED);
        }
        Ok(c.phase_at(t))
    }
    /// 守恒审计：total_certs == held + evicted。
    pub fn audit(&self) -> Result<(), u16> {
        if self.total_certs != self.ring.len() as u64 + self.evicted {
            return Err(E_LEDGER);
        }
        Ok(())
    }
    pub fn held(&self) -> usize {
        self.ring.len()
    }
    pub fn phase_tally(&self, t: u64) -> [u32; 3] {
        let mut a = [0u32; 3];
        for c in &self.ring {
            if c.state == CertState::Active {
                a[c.phase_at(t).ord() as usize] += 1;
            }
        }
        a
    }
}

/// 自例程：主路径（签发→重认证→退役）+ 宽限路径 + 过期拒绝 + 吊销 + 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut led = CertLedger::new();
    // 主路径 1：签发 → Valid → 重认证 → 前移 → 退役。
    led.issue(1, "gpu_agent_a", 100)?;
    led.audit()?;
    if led.access(1, 100)? != WindowPhase::Valid {
        return Err(E_WINDOW);
    }
    if led.access(1, 100 + VALID_TICKS - 1)? != WindowPhase::Valid {
        return Err(E_WINDOW);
    }
    let e1 = led.renew(1, 200)?;
    if e1 != 100 + VALID_TICKS * 2 {
        return Err(E_WINDOW);
    }
    led.retire(1)?;
    if led.access(1, 200) != Err(E_EXPIRED) {
        return Err(E_STATE);
    }
    // 主路径 2：宽限段可用但 Expired 段拒绝。
    led.issue(2, "gpu_agent_b", 0)?;
    // 窗口 [0,720)；宽限 [720,768)；768 起失效。
    if led.access(2, 730)? != WindowPhase::Grace {
        return Err(E_WINDOW);
    }
    if led.access(2, 767)? != WindowPhase::Grace {
        return Err(E_WINDOW);
    }
    // 宽限段内允许重认证。
    led.renew(2, 740)?;
    if led.access(2, 740)? != WindowPhase::Valid {
        return Err(E_WINDOW);
    }
    // 主路径 3：Expire 后重认证拒绝（必须换发）。
    led.issue(3, "gpu_agent_c", 0)?;
    if led.renew(3, VALID_TICKS + GRACE_TICKS + 1) != Err(E_EXPIRED) {
        return Err(E_EXPIRED);
    }
    if led.access(3, VALID_TICKS + GRACE_TICKS) != Err(E_EXPIRED) {
        return Err(E_EXPIRED);
    }
    // 主路径 4：重认证次数超限。
    led.issue(4, "gpu_agent_d", 0)?;
    for i in 0..RENEW_LIMIT {
        let t = i as u64 * 100;
        led.renew(4, t)?;
    }
    if led.renew(4, 0) != Err(E_RENEW) {
        return Err(E_RENEW);
    }
    // 主路径 5：吊销终态——立即失效且不可重认证。
    led.issue(5, "gpu_agent_e", 0)?;
    led.revoke(5)?;
    if led.ring_state(5)? != CertState::Revoked {
        return Err(E_STATE);
    }
    // 吊销幂等（重复吊销仍 Revoked），但生命周期操作全拒绝。
    led.revoke(5)?;
    if led.renew(5, 1) != Err(E_STATE) || led.access(5, 1) != Err(E_EXPIRED) {
        return Err(E_STATE);
    }
    // 错误路径 1：重复编号 / 空主体。
    if led.issue(1, "dup", 0) != Err(E_DUP) {
        return Err(E_DUP);
    }
    if led.issue(9, "", 0) != Err(E_WINDOW) {
        return Err(E_WINDOW);
    }
    // 错误路径 2：未知证书。
    if led.access(99, 0) != Err(E_NOCERT) || led.renew(99, 0) != Err(E_NOCERT) {
        return Err(E_NOCERT);
    }
    // 段位统计：t=0 时 Active 证书 2（2,3 Valid）+4 Valid → Valid=3。
    let tally = led.phase_tally(0);
    if tally[WindowPhase::Valid.ord() as usize] != 3 {
        return Err(E_LEDGER);
    }
    // 定容逐出：52 张证书（前 48 张 Revoked），再入 4 张触发逐出。
    let mut led2 = CertLedger::new();
    for i in 1..=48u32 {
        let s = alloc::format!("s{}", i);
        led2.issue(i, &s, 0)?;
        led2.revoke(i)?;
    }
    for i in 49..=52u32 {
        let s = alloc::format!("s{}", i);
        led2.issue(i, &s, 0)?;
    }
    led2.audit()?;
    if led2.held() != CERT_CAP || led2.evicted != 4 || led2.total_certs != 52 {
        return Err(E_LEDGER);
    }
    // 满且全在册（Active）→ 拒绝。
    let mut led3 = CertLedger::new();
    for i in 1..=48u32 {
        let s = alloc::format!("a{}", i);
        led3.issue(i, &s, 0)?;
    }
    if led3.issue(49, "no room", 0) != Err(E_CAP) {
        return Err(E_CAP);
    }
    Ok(0)
}

impl CertLedger {
    /// 只读状态查询。
    pub fn ring_state(&self, id: u32) -> Result<CertState, u16> {
        self.ring
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.state)
            .ok_or(E_NOCERT)
    }
}
