//! CGPU-F0390 · 配额管理 API（目标 320 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 配额管理：热调整（运维档账户配额运行时调整——每笔变更审计留痕）；
//!   调整校验（调整后守恒校验——不许超卖）；权限模型（谁可调——对接权限
//!   域预留）；回滚（调整可回滚——历史可溯）。判据：热调整、审计、守恒
//!   校验、回滚、判据。
//!
//! 设计纪律：
//! - 热调整是「总额守恒」下的再分配：调整前后总额不变（不许超卖）。
//! - 每笔变更加审计链（序号单调、可回放、可回滚到任意笔）。
//! - 权限模型一期两角色（Viewer 只读 / Operator 可调），对接位预留。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0B30 独占。

use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;

/// 诊断段基址（0x0B30，全仓独占）。
pub const SEG_BASE: u16 = 0x0B30;

pub const E_NOT_CONSERVED: u16 = SEG_BASE + 1;
pub const E_ACCOUNT_UNKNOWN: u16 = SEG_BASE + 2;
pub const E_FORBIDDEN: u16 = SEG_BASE + 3;
pub const E_NO_HISTORY: u16 = SEG_BASE + 4;
pub const E_ADMIT_PENDING: u16 = SEG_BASE + 5;

/// 角色（权限模型——对接权限域预留位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Viewer,
    Operator,
}

impl Role {
    pub fn may_adjust(self) -> bool {
        matches!(self, Role::Operator)
    }
}

/// 审计行（每笔变更留痕——历史可溯的最小单位）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditEntry {
    pub seq: u64,
    pub account: u32,
    /// 调整前/后配额（千分位）。
    pub before_pm: u32,
    pub after_pm: u32,
    pub actor: String,
}

/// 配额账（账户 → 千分位配额）。
#[derive(Debug)]
pub struct QuotaLedger {
    quotas: Vec<(u32, u32)>,
    total_pm: u32,
    audit: Vec<AuditEntry>,
    seq: u64,
}

impl QuotaLedger {
    pub fn new(quotas: Vec<(u32, u32)>) -> Result<Self, u16> {
        let total: u32 = quotas.iter().map(|(_, q)| q).sum();
        if total > 10_000 {
            return Err(E_NOT_CONSERVED);
        }
        let mut seen = Vec::new();
        for (a, _) in &quotas {
            if seen.contains(a) {
                return Err(SEG_BASE + 6);
            }
            seen.push(*a);
        }
        Ok(QuotaLedger { quotas, total_pm: total, audit: Vec::new(), seq: 0 })
    }

    pub fn quota_of(&self, account: u32) -> Result<u32, u16> {
        self.quotas.iter().find(|(a, _)| *a == account).map(|(_, q)| *q).ok_or(E_ACCOUNT_UNKNOWN)
    }

    pub fn total(&self) -> u32 {
        self.total_pm
    }

    pub fn audit_len(&self) -> usize {
        self.audit.len()
    }

    /// 热调整（判据「热调整」）：运行时改配额，总额守恒校验内联。
    /// 守恒铁律：全账户配额和 ≤ 10_000（不许超卖——调整即时校验）。
    pub fn adjust(&mut self, role: Role, actor: &str, account: u32, new_quota_pm: u32) -> Result<(), u16> {
        if !role.may_adjust() {
            return Err(E_FORBIDDEN);
        }
        let before = self.quota_of(account)?;
        let new_total = self.total_pm as i64 - before as i64 + new_quota_pm as i64;
        if new_total > 10_000 || new_quota_pm > 10_000 || new_total < 0 {
            return Err(E_NOT_CONSERVED);
        }
        self.total_pm = new_total as u32;
        if let Some(e) = self.quotas.iter_mut().find(|(a, _)| *a == account) {
            e.1 = new_quota_pm;
        }
        // 审计留痕（序号单调）。
        self.seq += 1;
        self.audit.push(AuditEntry { seq: self.seq, account, before_pm: before, after_pm: new_quota_pm, actor: String::from(actor) });
        Ok(())
    }

    /// 回滚最近一笔（判据「回滚」）：恢复该笔调整前配额并再留痕。
    pub fn rollback_last(&mut self, actor: &str) -> Result<(), u16> {
        let last = self.audit.last().ok_or(E_NO_HISTORY)?.clone();
        self.quota_of(last.account)?;
        if let Some(e) = self.quotas.iter_mut().find(|(a, _)| *a == last.account) {
            e.1 = last.before_pm;
        }
        self.total_pm = (self.total_pm as i64 - last.after_pm as i64 + last.before_pm as i64) as u32;
        self.seq += 1;
        self.audit.push(AuditEntry { seq: self.seq, account: last.account, before_pm: last.after_pm, after_pm: last.before_pm, actor: String::from(actor) });
        Ok(())
    }

    /// 回滚到指定序号（历史可溯——逐笔逆放）。
    pub fn rollback_to(&mut self, actor: &str, target_seq: u64) -> Result<usize, u16> {
        // 快照先行：逆放过程中不再追加逐笔条目，链长有界不膨胀。
        let snapshot: Vec<AuditEntry> = self.audit.clone();
        let mut undone = 0usize;
        for entry in snapshot.iter().rev() {
            if entry.seq <= target_seq {
                break;
            }
            if let Some(e) = self.quotas.iter_mut().find(|(a, _)| *a == entry.account) {
                e.1 = entry.before_pm;
            }
            self.total_pm = (self.total_pm as i64 - entry.after_pm as i64 + entry.before_pm as i64) as u32;
            undone += 1;
        }
        if undone > 0 {
            // 汇总留痕一笔（account 0 = 回滚标记，守恒由逆放自身保证）。
            self.seq += 1;
            self.audit.push(AuditEntry { seq: self.seq, account: 0, before_pm: 0, after_pm: 0, actor: String::from(actor) });
        }
        Ok(undone)
    }

    /// 守恒校验（判据「不许超卖」）。
    pub fn conserved(&self) -> bool {
        self.quotas.iter().map(|(_, q)| q).sum::<u32>() <= 10_000 && self.quotas.iter().map(|(_, q)| q).sum::<u32>() == self.total_pm
    }

    /// 审计链完整性：序号 1..n 连续单调。
    pub fn audit_chain_ok(&self) -> bool {
        self.audit.iter().enumerate().all(|(i, e)| e.seq == i as u64 + 1)
    }
}

/// 权限域对接预留位（一期诚实标注：请求审批未实现）。
pub fn permission_admission(_principal: &str, _scope: &str) -> Result<Role, u16> {
    // 一期：对接域未上线，统一拒绝升级请求（不假装可用）。
    Err(E_ADMIT_PENDING)
}

// ---------------------------------------------------------------------------
// 判据自检
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    fn ledger() -> QuotaLedger {
        QuotaLedger::new(vec![(1, 4_000), (2, 3_000), (3, 2_000)]).unwrap()
    }

    #[test]
    fn a90_热调整_守恒内联() {
        let mut l = ledger();
        assert!(l.conserved());
        // 等量腾挪：1 拿 500 → 2。总不变。
        l.adjust(Role::Operator, "op1", 2, 3_500).unwrap();
        assert_eq!(l.quota_of(2).unwrap(), 3_500);
        assert!(l.conserved());
        // 从准备池补：总=9_000−2_000+3_000+500=10_500 > 10_000 → 拒。
        assert_eq!(l.adjust(Role::Operator, "op1", 3, 3_000).unwrap_err(), E_NOT_CONSERVED);
        // 从准备池补但守住 10_000：总=9_000−2_000+2_500+500=10_000 → 过。
        l.adjust(Role::Operator, "op1", 3, 2_500).unwrap();
        assert_eq!(l.total(), 10_000);
        assert!(l.conserved());
        // 未知账户拒。
        assert_eq!(l.adjust(Role::Operator, "op1", 9, 100).unwrap_err(), E_ACCOUNT_UNKNOWN);
    }

    #[test]
    fn a90_审计_留痕与链完整() {
        let mut l = ledger();
        l.adjust(Role::Operator, "op1", 2, 3_500).unwrap();
        l.adjust(Role::Operator, "op2", 1, 3_000).unwrap();
        assert_eq!(l.audit_len(), 2);
        assert!(l.audit_chain_ok());
        // 序号单调 1,2。
        assert_eq!(l.audit_len(), 2);
        // 权限：Viewer 拒。
        assert_eq!(l.adjust(Role::Viewer, "vw", 2, 1_000).unwrap_err(), E_FORBIDDEN);
        assert_eq!(l.audit_len(), 2, "denied adjust leaves no audit");
        // 权限域对接预留：审批未上线。
        assert_eq!(permission_admission("root", "*").unwrap_err(), E_ADMIT_PENDING);
        assert!(!Role::Viewer.may_adjust());
        assert!(Role::Operator.may_adjust());
    }

    #[test]
    fn a90_回滚_单笔与逐笔逆放() {
        let mut l = ledger();
        l.adjust(Role::Operator, "op1", 2, 3_500).unwrap(); // seq1: 2:3000→3500
        l.adjust(Role::Operator, "op1", 1, 4_500).unwrap(); // seq2: 1:4000→4500
        // 单笔回滚：1 回 4000。
        l.rollback_last("op1").unwrap();
        assert_eq!(l.quota_of(1).unwrap(), 4_000);
        assert_eq!(l.audit_len(), 3);
        assert!(l.audit_chain_ok());
        // 回滚到 seq1：快照内 seq>1 的两笔（seq2 调整 + seq3 单笔回滚）逆放，
        // 净效果 = seq1 后的态：1=4000、2=3500；汇总留痕 1 笔。
        let undone = l.rollback_to("op1", 1).unwrap();
        assert_eq!(undone, 2);
        assert_eq!(l.quota_of(1).unwrap(), 4_000);
        assert_eq!(l.quota_of(2).unwrap(), 3_500);
        // 空历史回滚拒。
        let mut fresh = ledger();
        assert_eq!(fresh.rollback_last("x").unwrap_err(), E_NO_HISTORY);
        // 回滚后守恒。
        assert!(l.conserved());
        // 初构超卖拒。
        assert_eq!(QuotaLedger::new(vec![(1, 9_000), (2, 2_000)]).unwrap_err(), E_NOT_CONSERVED);
        // 初构重复账户拒。
        assert_eq!(QuotaLedger::new(vec![(1, 100), (1, 200)]).unwrap_err(), SEG_BASE + 6);
    }
}
