//! CGPU-F0444 · 仲裁快照与热迁移预留（目标 300 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 热迁移预留：仲裁器状态快照（账户表/规则/预测状态的可快照性设计）；
//!   热迁移挂载点（快照→新实例恢复的迁移路径预留）；诚实标注（一期
//!   快照用于崩溃恢复、热迁移为预留）；快照一致性（快照瞬间无半态）。
//!   判据：快照设计、迁移预留、诚实标注、一致性、判据。
//!
//! 设计纪律：
//! - 快照 = 三段完整状态（账户预算表/启用规则集/预测 EWMA 状态），
//!   带 CRC 校验——恢复时先验 CRC 再落账（坏快照不静默加载）。
//! - 一致性（无半态）：快照由单一 `snapshot()` 原子生成——生成期间
//!   无中间态暴露（实现上：全量克隆后统一打包，无分批半态）。
//! - 迁移挂载点预留：`restore_into` 结构可用（快照 → 新实例装载），
//!   但热迁移在线语义（不停机搬迁）为预留——`start_livemigrate`
//!   一期恒拒（诚实标注枚举 MigrationStage::CrashRecoveryOnly）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0EF0 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0EF0，全仓独占）。
pub const SEG_BASE: u16 = 0x0EF0;

pub const E_SNAP_CRC: u16 = SEG_BASE + 1; // 快照 CRC 红（坏快照拒载）
pub const E_SNAP_EMPTY: u16 = SEG_BASE + 2; // 空快照（无账户）
pub const E_SNAP_HALF: u16 = SEG_BASE + 3; // 半态快照（三段长度不一致）
pub const E_SNAP_MIG: u16 = SEG_BASE + 4; // 一期预留：热迁移未开放
pub const E_SNAP_DST: u16 = SEG_BASE + 5; // 恢复目标非法（非空实例）

/// 迁移成熟度（诚实标注）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrationStage {
    /// 一期：快照仅用于崩溃恢复。
    CrashRecoveryOnly,
    /// 预留：在线热迁移。
    LiveMigrate,
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for b in data {
        crc ^= *b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// 账户态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcctState {
    pub acct: u32,
    pub budget: u64,
    pub granted: u64,
}

/// 规则态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleState {
    pub rule_id: u32,
    pub weight: u64,
    pub enabled: bool,
}

/// 预测态（EWMA 摘要）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PredState {
    pub ewma_x100: u64,
    pub alpha_pm: u32,
}

/// 仲裁器（可快照实例）。
#[derive(Debug, Default)]
pub struct ArbiterState {
    pub accounts: Vec<AcctState>,
    pub rules: Vec<RuleState>,
    pub pred: Option<PredState>,
}

impl ArbiterState {
    /// 预算守恒视图（快照一致性的一部分）。
    pub fn balanced(&self) -> bool {
        for a in &self.accounts {
            if a.granted > a.budget {
                return false;
            }
        }
        true
    }
}

/// 快照（三段 + CRC——无半态的完整打包）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArbSnapshot {
    pub accounts: Vec<AcctState>,
    pub rules: Vec<RuleState>,
    pub pred: Option<PredState>,
    pub crc: u32,
}

impl ArbSnapshot {
    fn body_bytes(&self) -> Vec<u8> {
        let mut v = Vec::new();
        for a in &self.accounts {
            v.extend_from_slice(&a.acct.to_le_bytes());
            v.extend_from_slice(&a.budget.to_le_bytes());
            v.extend_from_slice(&a.granted.to_le_bytes());
        }
        for r in &self.rules {
            v.extend_from_slice(&r.rule_id.to_le_bytes());
            v.extend_from_slice(&r.weight.to_le_bytes());
            v.push(r.enabled as u8);
        }
        if let Some(p) = self.pred {
            v.extend_from_slice(&p.ewma_x100.to_le_bytes());
            v.extend_from_slice(&p.alpha_pm.to_le_bytes());
        } else {
            v.push(0xFF);
        }
        v
    }

    /// CRC 自检。
    pub fn crc_ok(&self) -> bool {
        self.crc == crc32(&self.body_bytes())
    }

    /// 三段一致性：无半态（账户与规则内无跨段账目矛盾—— granted ≤ budget）。
    pub fn no_half_state(&self) -> bool {
        self.accounts.iter().all(|a| a.granted <= a.budget)
    }
}

/// 快照管理器。
#[derive(Debug)]
pub struct SnapManager {
    pub stage: MigrationStage,
    pub snaps: Vec<ArbSnapshot>,
}

impl SnapManager {
    pub fn new(stage: MigrationStage) -> Self {
        Self {
            stage,
            snaps: Vec::new(),
        }
    }

    /// 原子快照：全量克隆统一打包（无分批半态）。
    pub fn snapshot(&mut self, s: &ArbiterState) -> Result<u32, u16> {
        if s.accounts.is_empty() {
            return Err(E_SNAP_EMPTY);
        }
        if !s.balanced() {
            return Err(E_SNAP_HALF);
        }
        let snap = ArbSnapshot {
            accounts: s.accounts.clone(),
            rules: s.rules.clone(),
            pred: s.pred,
            crc: 0,
        };
        let crc = crc32(&snap.body_bytes());
        let snap = ArbSnapshot { crc, ..snap };
        self.snaps.push(snap.clone());
        Ok(snap.crc)
    }

    /// 恢复挂载点：快照 → 新实例（空实例才准挂——覆盖恢复不覆盖活实例）。
    pub fn restore_into(&self, idx: usize, dst: &mut ArbiterState) -> Result<(), u16> {
        let snap = self.snaps.get(idx).ok_or(E_SNAP_EMPTY)?;
        if !snap.crc_ok() {
            return Err(E_SNAP_CRC);
        }
        if !snap.no_half_state() {
            return Err(E_SNAP_HALF);
        }
        if !dst.accounts.is_empty() {
            return Err(E_SNAP_DST); // 活实例拒绝覆盖（崩溃恢复只进空壳）。
        }
        dst.accounts = snap.accounts.clone();
        dst.rules = snap.rules.clone();
        dst.pred = snap.pred;
        Ok(())
    }

    /// 热迁移预留：一期恒拒（诚实标注）。
    pub fn start_livemigrate(&self) -> Result<(), u16> {
        Err(E_SNAP_MIG)
    }

    pub fn snap_count(&self) -> usize {
        self.snaps.len()
    }
}

// ==========================================================================
// 判据（施工书：快照设计、迁移预留、诚实标注、一致性、判据）
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> ArbiterState {
        ArbiterState {
            accounts: vec![
                AcctState { acct: 1, budget: 500, granted: 200 },
                AcctState { acct: 2, budget: 300, granted: 0 },
            ],
            rules: vec![RuleState { rule_id: 10, weight: 40, enabled: true }],
            pred: Some(PredState { ewma_x100: 12_500, alpha_pm: 200 }),
        }
    }

    #[test]
    fn a01_快照设计_三段完整与CRC() {
        let mut m = SnapManager::new(MigrationStage::CrashRecoveryOnly);
        let crc = m.snapshot(&state()).unwrap();
        assert_eq!(m.snap_count(), 1);
        // CRC 自证：同状态同 CRC。
        let mut m2 = SnapManager::new(MigrationStage::CrashRecoveryOnly);
        assert_eq!(m2.snapshot(&state()), Ok(crc));
        // CRC 算法标准向量。
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        // 空状态拒。
        let e = ArbiterState::default();
        assert_eq!(m.snapshot(&e), Err(E_SNAP_EMPTY));
    }

    #[test]
    fn a02_快照一致性_无半态() {
        let mut m = SnapManager::new(MigrationStage::CrashRecoveryOnly);
        // 半态（granted > budget）拒拍。
        let mut half = state();
        half.accounts[0].granted = 600;
        assert!(!half.balanced());
        assert_eq!(m.snapshot(&half), Err(E_SNAP_HALF));
        assert_eq!(m.snap_count(), 0);
        // 健康状态拍出后自检通过。
        m.snapshot(&state()).unwrap();
        assert!(m.snaps[0].no_half_state());
        assert!(m.snaps[0].crc_ok());
    }

    #[test]
    fn a03_恢复挂载点_空壳才准挂() {
        let mut m = SnapManager::new(MigrationStage::CrashRecoveryOnly);
        m.snapshot(&state()).unwrap();
        // 空壳恢复：三段完整回来。
        let mut dst = ArbiterState::default();
        assert_eq!(m.restore_into(0, &mut dst), Ok(()));
        assert_eq!(dst.accounts.len(), 2);
        assert_eq!(dst.accounts[0].granted, 200);
        assert_eq!(dst.rules[0].weight, 40);
        assert_eq!(dst.pred.map(|p| p.alpha_pm), Some(200));
        // 活实例拒覆盖。
        let mut live = state();
        assert_eq!(m.restore_into(0, &mut live), Err(E_SNAP_DST));
        // 越界快照拒。
        let mut e = ArbiterState::default();
        assert_eq!(m.restore_into(9, &mut e), Err(E_SNAP_EMPTY));
    }

    #[test]
    fn a04_坏快照拒载_CRC防线() {
        let mut m = SnapManager::new(MigrationStage::CrashRecoveryOnly);
        m.snapshot(&state()).unwrap();
        // 篡改快照内容 → CRC 红 → 恢复拒（不静默加载脏账）。
        let mut bad = m.snaps[0].clone();
        bad.accounts[0].granted = 999;
        assert!(!bad.crc_ok());
        m.snaps[0] = bad;
        let mut dst = ArbiterState::default();
        assert_eq!(m.restore_into(0, &mut dst), Err(E_SNAP_CRC));
        assert!(dst.accounts.is_empty()); // 未被污染。
    }

    #[test]
    fn a05_热迁移预留_诚实标注() {
        let m = SnapManager::new(MigrationStage::CrashRecoveryOnly);
        // 一期热迁移恒拒。
        assert_eq!(m.start_livemigrate(), Err(E_SNAP_MIG));
        let r = SnapManager::new(MigrationStage::LiveMigrate);
        // 预留态同样拒（二期开放前不假装可用）。
        assert_eq!(r.start_livemigrate(), Err(E_SNAP_MIG));
        assert_eq!(r.stage, MigrationStage::LiveMigrate);
    }
}
