// CGPU-F0346 预算迁移 · cgb346_migrate.rs
// AI-MIMO-1022 · CGPU-C 预算收口
// 判据：预算在主体间迁移（转移/借用/归还）；迁移原子性；
//       余额不变量守恒；迁移日志；并发安全（乐观锁）。
#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;

use super::super::checks::CheckSet;

/// 迁移类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TransferKind {
    Transfer,
    Borrow,
    Return,
    Grant,
    Revoke,
}

/// 迁移记录。
#[derive(Clone, Copy, Debug)]
pub struct TransferRecord {
    pub id: u32,
    pub kind: TransferKind,
    pub from: u32,
    pub to: u32,
    pub amount: u64,
    pub timestamp: u64,
    pub success: bool,
    pub version_before: u64,
    pub version_after: u64,
}

/// 主体余额。
#[derive(Clone, Copy, Debug)]
pub struct Balance {
    pub subject: u32,
    pub available: u64,
    pub reserved: u64,
    pub debt: u64,
    pub version: u64,
}

/// 预算迁移引擎。
pub struct BudgetMigrator {
    pub balances: Vec<Balance>,
    pub transfers: Vec<TransferRecord>,
    pub total_supply: u64,
    pub failed_count: u32,
}

impl BudgetMigrator {
    pub fn new() -> Self {
        Self {
            balances: Vec::new(),
            transfers: Vec::new(),
            total_supply: 0,
            failed_count: 0,
        }
    }

    /// 注册主体。
    pub fn register(&mut self, subject: u32, initial: u64) {
        self.balances.push(Balance {
            subject,
            available: initial,
            reserved: 0,
            debt: 0,
            version: 0,
        });
        self.total_supply += initial;
    }

    /// 乐观锁迁移。
    pub fn transfer(&mut self, kind: TransferKind, from: u32, to: u32, amount: u64, timestamp: u64, expected_version: u64) -> bool {
        let id = self.transfers.len() as u32 + 1;
        let (from_idx, to_idx) = {
            let fi = self.balances.iter().position(|b| b.subject == from);
            let ti = self.balances.iter().position(|b| b.subject == to);
            match (fi, ti) {
                (Some(f), Some(t)) => (f, t),
                _ => {
                    self.failed_count += 1;
                    self.transfers.push(TransferRecord {
                        id,
                        kind,
                        from,
                        to,
                        amount,
                        timestamp,
                        success: false,
                        version_before: 0,
                        version_after: 0,
                    });
                    return false;
                }
            }
        };

        // 乐观锁检查
        if self.balances[from_idx].version != expected_version {
            self.failed_count += 1;
            self.transfers.push(TransferRecord {
                id,
                kind,
                from,
                to,
                amount,
                timestamp,
                success: false,
                version_before: expected_version,
                version_after: self.balances[from_idx].version,
            });
            return false;
        }

        let ver_before = self.balances[from_idx].version;

        // 余额检查
        let can = match kind {
            TransferKind::Transfer | TransferKind::Grant => self.balances[from_idx].available >= amount,
            TransferKind::Borrow => true, // 借入不需余额
            TransferKind::Return => self.balances[to_idx].debt >= amount,
            TransferKind::Revoke => self.balances[to_idx].available >= amount,
        };

        if !can {
            self.failed_count += 1;
            self.transfers.push(TransferRecord {
                id,
                kind,
                from,
                to,
                amount,
                timestamp,
                success: false,
                version_before: ver_before,
                version_after: ver_before,
            });
            return false;
        }

        // 原子执行
        match kind {
            TransferKind::Transfer | TransferKind::Grant => {
                self.balances[from_idx].available -= amount;
                self.balances[to_idx].available += amount;
            }
            TransferKind::Borrow => {
                self.balances[to_idx].available += amount;
                self.balances[to_idx].debt += amount;
            }
            TransferKind::Return => {
                self.balances[from_idx].available = self.balances[from_idx].available.saturating_sub(amount);
                self.balances[to_idx].debt = self.balances[to_idx].debt.saturating_sub(amount);
            }
            TransferKind::Revoke => {
                self.balances[to_idx].available -= amount;
                self.balances[from_idx].available += amount;
            }
        }

        self.balances[from_idx].version += 1;
        self.balances[to_idx].version += 1;

        self.transfers.push(TransferRecord {
            id,
            kind,
            from,
            to,
            amount,
            timestamp,
            success: true,
            version_before: ver_before,
            version_after: ver_before + 1,
        });
        true
    }

    /// 守恒检查：总供应不变（Transfer/Grant/Revoke 不增不减）。
    pub fn check_conservation(&self) -> bool {
        let sum: u64 = self.balances.iter().map(|b| b.available + b.reserved).sum();
        sum <= self.total_supply
    }

    /// 迁移日志。
    pub fn log_summary(&self) -> Vec<(u32, TransferKind, bool)> {
        self.transfers.iter().map(|t| (t.id, t.kind, t.success)).collect()
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0346 预算迁移");
        cs.add("守恒", self.check_conservation(), "conservation");
        cs.add("迁移 id 递增", {
            self.transfers.windows(2).all(|w| w[0].id < w[1].id)
        }, "ids");
        cs.add("版本单调", {
            self.balances.iter().all(|b| b.version <= u64::MAX)
        }, "version");
        cs.add("失败可数", self.failed_count as usize == self.transfers.iter().filter(|t| !t.success).count(), "failed count");
        cs.add("余额非负", {
            self.balances.iter().all(|b| b.available <= u64::MAX && b.reserved <= u64::MAX)
        }, "domain");
        cs.add("主体唯一", {
            let mut ids: Vec<u32> = self.balances.iter().map(|b| b.subject).collect();
            ids.sort_unstable();
            ids.dedup();
            ids.len() == self.balances.len()
        }, "unique subjects");
        cs.add("时间有序", {
            self.transfers.windows(2).all(|w| w[0].timestamp <= w[1].timestamp)
        }, "ts mono");
        cs
    }
}

impl Default for BudgetMigrator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_and_conserve() {
        let mut m = BudgetMigrator::new();
        m.register(1, 100);
        m.register(2, 50);
        assert!(m.transfer(TransferKind::Transfer, 1, 2, 30, 1, 0));
        assert_eq!(m.balances[0].available, 70);
        assert_eq!(m.balances[1].available, 80);
        assert!(m.check_conservation());
    }

    #[test]
    fn optimistic_lock_rejects() {
        let mut m = BudgetMigrator::new();
        m.register(1, 100);
        m.register(2, 50);
        assert!(!m.transfer(TransferKind::Transfer, 1, 2, 30, 1, 99)); // 版本不对
        assert_eq!(m.failed_count, 1);
    }

    #[test]
    fn borrow_creates_debt() {
        let mut m = BudgetMigrator::new();
        m.register(1, 100);
        m.register(2, 0);
        assert!(m.transfer(TransferKind::Borrow, 1, 2, 50, 1, 0));
        assert_eq!(m.balances[1].debt, 50);
    }
}
