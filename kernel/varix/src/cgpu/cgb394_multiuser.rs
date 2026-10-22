//! CGPU-F0394 · 多用户预算预留（目标 300 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 多用户：S 域联动预留（F2881+ 多会话渲染——每用户配额的预算侧接口
//!   预留）；隔离原则（用户间不互窃——公平铁律的会话版声明）；诚实标注
//!   （一期单用户、多用户为预留）。判据：接口预留、隔离原则、诚实标注、
//!   判据。
//!
//! 设计纪律：
//! - 一期单用户：多用户接口全预留（形状冻结、行为诚实拒绝）。
//! - 隔离原则可静态验证：配额代数上「用户间转移必守恒且需双方在场」。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0B80 独占。

use alloc::vec::Vec;

/// 诊断段基址（0x0B80，全仓独占）。
pub const SEG_BASE: u16 = 0x0B80;

pub const E_MULTIUSER_UNAVAILABLE: u16 = SEG_BASE + 1;
pub const E_ISOLATION_VIOLATION: u16 = SEG_BASE + 2;
pub const E_SESSION_UNKNOWN: u16 = SEG_BASE + 3;
pub const E_TRANSFER_CONSERVATION: u16 = SEG_BASE + 4;

/// 预留接口版本。
pub const RESV_IFACE_VERSION: u32 = 1;

/// 会话配额形状（S 域 F2881+ 对齐位——预留冻结）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionQuota {
    pub session_id: u64,
    pub user_id: u64,
    /// 配额千分位。
    pub quota_pm: u32,
}

impl SessionQuota {
    /// 形状核（预留接口的契约面）。
    pub fn shape_ok(&self) -> bool {
        self.session_id != 0 && self.user_id != 0 && self.quota_pm <= 10_000
    }
}

/// 能力探测（诚实标注）：一期多会话渲染未上线。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MultiUserCapability {
    pub multi_session_ready: bool,
    pub per_user_quota_ready: bool,
}

impl MultiUserCapability {
    /// 诚实探测：一期恒为未就绪（预留接口不假装可用）。
    pub fn probe_honest() -> Self {
        MultiUserCapability { multi_session_ready: false, per_user_quota_ready: false }
    }
}

/// 多用户预算面（预留实现——一期只挂单会话）。
pub struct MultiUserBudget {
    cap: MultiUserCapability,
    sessions: Vec<SessionQuota>,
    /// 单会话上限（一期=1——诚实标注的数值面）。
    pub max_sessions_phase1: usize,
}

impl MultiUserBudget {
    pub fn new() -> Self {
        MultiUserBudget { cap: MultiUserCapability::probe_honest(), sessions: Vec::new(), max_sessions_phase1: 1 }
    }

    /// 预留接口：多会话挂载。一期单会话可挂；第二个会话显性拒（不假装）。
    pub fn attach_session(&mut self, q: SessionQuota) -> Result<(), u16> {
        if !q.shape_ok() {
            return Err(E_SESSION_UNKNOWN);
        }
        // 幂等核先行：同会话重挂不受一期单会话容量闸影响。
        if self.sessions.iter().any(|s| s.session_id == q.session_id) {
            return Ok(());
        }
        if self.sessions.len() >= self.max_sessions_phase1 {
            return Err(E_MULTIUSER_UNAVAILABLE);
        }
        self.sessions.push(q);
        Ok(())
    }

    /// 预留接口：跨用户配额转移。一期必拒（隔离原则的运行期防线）。
    pub fn transfer(&mut self, from: u64, to: u64, amount_pm: u32) -> Result<(), u16> {
        let _ = (from, to, amount_pm);
        Err(E_ISOLATION_VIOLATION)
    }

    pub fn sessions(&self) -> &[SessionQuota] {
        &self.sessions
    }

    pub fn capability(&self) -> MultiUserCapability {
        self.cap
    }
}

impl Default for MultiUserBudget {
    fn default() -> Self {
        Self::new()
    }
}

/// 隔离原则的静态代数核（判据「不互窃」——公平铁律会话版）：
/// 一组会话配额的总和 ≤ 全局池；任何转移若使总和增大即违约（代数可证）。
pub fn isolation_invariant(quotas: &[u32], pool_pm: u32) -> Result<(), u16> {
    let sum: u64 = quotas.iter().map(|&q| q as u64).sum();
    if sum > pool_pm as u64 {
        return Err(E_TRANSFER_CONSERVATION);
    }
    Ok(())
}

/// 转移守恒核（二期实现的契约预置）：转移前后总和不变才合法。
pub fn transfer_conservation(before: &[u32], after: &[u32]) -> Result<(), u16> {
    if before.len() != after.len() {
        return Err(E_TRANSFER_CONSERVATION);
    }
    let sb: u64 = before.iter().map(|&q| q as u64).sum();
    let sa: u64 = after.iter().map(|&q| q as u64).sum();
    if sb != sa {
        return Err(E_TRANSFER_CONSERVATION);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 判据自检
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    fn quota(sid: u64, uid: u64, q: u32) -> SessionQuota {
        SessionQuota { session_id: sid, user_id: uid, quota_pm: q }
    }

    #[test]
    fn a94_接口预留_单会话挂载与诚实拒() {
        let mut b = MultiUserBudget::new();
        assert_eq!(RESV_IFACE_VERSION, 1);
        // 一期：单会话可挂。
        b.attach_session(quota(1, 100, 5_000)).unwrap();
        assert_eq!(b.sessions().len(), 1);
        // 幂等。
        b.attach_session(quota(1, 100, 5_000)).unwrap();
        assert_eq!(b.sessions().len(), 1);
        // 第二会话：预留接口显性拒（不假装多用户已可用）。
        assert_eq!(b.attach_session(quota(2, 200, 3_000)).unwrap_err(), E_MULTIUSER_UNAVAILABLE);
        // 形状核：零 id / 超池拒。
        assert_eq!(b.attach_session(quota(0, 100, 1)).unwrap_err(), E_SESSION_UNKNOWN);
        assert_eq!(b.attach_session(quota(3, 100, 10_001)).unwrap_err(), E_SESSION_UNKNOWN);
    }

    #[test]
    fn a94_隔离原则_转移必拒与代数核() {
        let mut b = MultiUserBudget::new();
        b.attach_session(quota(1, 100, 5_000)).unwrap();
        // 运行期防线：跨用户转移必拒。
        assert_eq!(b.transfer(1, 2, 1_000).unwrap_err(), E_ISOLATION_VIOLATION);
        // 静态代数核：总和不超池。
        assert!(isolation_invariant(&[5_000, 3_000], 10_000).is_ok());
        assert_eq!(isolation_invariant(&[6_000, 5_000], 10_000).unwrap_err(), E_TRANSFER_CONSERVATION);
        // 转移守恒核：等量腾挪合法；增发非法。
        assert!(transfer_conservation(&[5_000, 3_000], &[4_000, 4_000]).is_ok());
        assert_eq!(transfer_conservation(&[5_000, 3_000], &[5_000, 4_000]).unwrap_err(), E_TRANSFER_CONSERVATION);
        assert_eq!(transfer_conservation(&[5_000], &[5_000, 1]).unwrap_err(), E_TRANSFER_CONSERVATION);
    }

    #[test]
    fn a94_诚实标注_能力探测恒未就绪() {
        let b = MultiUserBudget::new();
        let cap = b.capability();
        assert_eq!(cap, MultiUserCapability::probe_honest());
        assert!(!cap.multi_session_ready);
        assert!(!cap.per_user_quota_ready);
        assert_eq!(b.max_sessions_phase1, 1);
        // 错误码常量在册（诚实标注位）。
        assert_eq!(E_MULTIUSER_UNAVAILABLE, SEG_BASE + 1);
        assert_eq!(E_ISOLATION_VIOLATION, SEG_BASE + 2);
    }
}
