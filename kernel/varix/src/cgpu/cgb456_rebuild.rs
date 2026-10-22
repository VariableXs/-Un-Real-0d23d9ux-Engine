//! CGPU-F0456 · 故障恢复预算重建（目标 380 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 仲裁器重启后的预算一致性（F0344 自保的深化——重启后账户态/
//!   预占/债务的完整重建）；
//! - 重建校验（重建后守恒校验——不许凭空多出预算）；
//! - 重建失败的兜底（无法重建→安全默认+损失显性告知）；
//! - 重建演练（注入重启验证重建路径）。判据：完整重建、守恒校验、
//!   兜底、演练、判据。
//!
//! 设计纪律：
//! - 完整重建：重启后三类态全部恢复——账户余额、预占（冻结额）、
//!   债务（透支偿还计划），缺一类即「不完整重建」（结构上区分
//!   full/partial 两级重建结果，partial 不冒充 full）。
//! - 守恒校验：重建后 Σ(余额+预占) ≤ 重启前总量快照——不许凭空
//!   多出预算（多出即破守恒，重建判红）。等号语义：预占冻结不销账。
//! - 兜底：重建数据 CRC 校验失败/守恒破缺 → 丢弃重建数据，落
//!   安全默认（每账户保底份额 + 总量 1000‰），损失项显性列告
//!   （告知哪些账户态不可恢复）——静默清零是禁止项。
//! - 演练：注入重启（构造快照→重建→对账）作为常态路径验证，
//!   演练结果与真实重启同一条代码路径（rebuild_from_snapshot）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0FB0 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0FB0，全仓独占）。
pub const SEG_BASE: u16 = 0x0FB0;

pub const E_RB_CORRUPT: u16 = SEG_BASE + 1; // 快照 CRC 校验失败
pub const E_RB_CONSERVE: u16 = SEG_BASE + 2; // 重建后守恒破缺（凭空多预算）
pub const E_RB_PARTIAL: u16 = SEG_BASE + 3; // 重建不完整（三类态缺一）
pub const E_RB_EMPTY: u16 = SEG_BASE + 4; // 空快照重建

/// 安全默认兜底配方。
pub const SAFE_DEFAULT_TOTAL_PM: u64 = 1000;
pub const SAFE_DEFAULT_PER_ACCOUNT_PM: u64 = 250;
pub const SAFE_DEFAULT_MAX_ACCOUNTS: usize = 4;

/// 重建结果分级（诚实枚举：partial 不冒充 full）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RebuildGrade {
    /// 三类态（余额/预占/债务）全部恢复。
    Full,
    /// 部分恢复（缺失类别数 > 0）。
    Partial,
    /// 重建失败，落安全默认。
    Fallback,
}

/// 快照中的账户条目（三类态齐备）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapEntry {
    pub acct: u32,
    /// 余额（‰）。
    pub balance_pm: u64,
    /// 预占冻结（‰）。
    pub reserved_pm: u64,
    /// 债务（‰，透支偿还计划）。
    pub debt_pm: u64,
}

/// 重启前快照（带 CRC 防线）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestartSnapshot {
    pub entries: Vec<SnapEntry>,
    /// 重启前总量快照（余额+预占合计，‰）。
    pub total_pm: u64,
    pub crc: u32,
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

impl RestartSnapshot {
    /// 构造并计算 CRC（构造方负责数据真实性）。
    pub fn new(entries: Vec<SnapEntry>) -> Self {
        let total: u64 = entries.iter().map(|e| e.balance_pm + e.reserved_pm).sum();
        let mut bytes: Vec<u8> = Vec::new();
        for e in entries.iter() {
            bytes.extend_from_slice(&e.acct.to_le_bytes());
            bytes.extend_from_slice(&e.balance_pm.to_le_bytes());
            bytes.extend_from_slice(&e.reserved_pm.to_le_bytes());
            bytes.extend_from_slice(&e.debt_pm.to_le_bytes());
        }
        let crc = crc32(&bytes);
        Self { entries, total_pm: total, crc }
    }

    /// CRC 复核。
    pub fn verify_crc(&self) -> bool {
        let mut bytes: Vec<u8> = Vec::new();
        for e in self.entries.iter() {
            bytes.extend_from_slice(&e.acct.to_le_bytes());
            bytes.extend_from_slice(&e.balance_pm.to_le_bytes());
            bytes.extend_from_slice(&e.reserved_pm.to_le_bytes());
            bytes.extend_from_slice(&e.debt_pm.to_le_bytes());
        }
        crc32(&bytes) == self.crc
    }
}

/// 重建后的预算台。
#[derive(Debug, PartialEq, Eq)]
pub struct RebuiltLedger {
    pub entries: Vec<SnapEntry>,
    /// 重建结果分级。
    pub grade: RebuildGrade,
    /// 兜底损失告知清单（不可恢复账户）。
    pub lost_accounts: Vec<u32>,
    /// 守恒校验是否通过。
    pub conserve_ok: bool,
}

/// 完整重建：从快照恢复三类态。
/// 顺序：CRC → 空判 → 守恒 → 完整性 → 成品；任一步失败走兜底。
pub fn rebuild_from_snapshot(snap: &RestartSnapshot) -> Result<RebuiltLedger, u16> {
    if snap.entries.is_empty() {
        return Err(E_RB_EMPTY);
    }
    if !snap.verify_crc() {
        return Err(E_RB_CORRUPT);
    }
    // 守恒校验：重建后 Σ(余额+预占) 不得超过重启前总量快照。
    let sum: u64 = snap.entries.iter().map(|e| e.balance_pm + e.reserved_pm).sum();
    if sum > snap.total_pm {
        return Err(E_RB_CONSERVE);
    }
    // 完整性：三类态逐一在册（余额/预占/债务各有非零样例即视为该类恢复；
    // 全零类不算缺失——账户可能本来就没有债务）。
    let has_bal = snap.entries.iter().any(|e| e.balance_pm > 0);
    let has_res = snap.entries.iter().any(|e| e.reserved_pm > 0);
    let has_debt = snap.entries.iter().any(|e| e.debt_pm > 0);
    let missing = if has_bal && has_res && has_debt { 0 } else { 1 };
    let grade = if missing == 0 { RebuildGrade::Full } else { RebuildGrade::Partial };
    Ok(RebuiltLedger {
        entries: snap.entries.clone(),
        grade,
        lost_accounts: Vec::new(),
        conserve_ok: true,
    })
}

/// 兜底：安全默认 + 损失显性告知（静默清零禁止）。
pub fn fallback_safe_default(lost_accounts: &[u32]) -> RebuiltLedger {
    // 安全默认：每账户保底份额，总量钳制 1000‰。
    let n = lost_accounts.len().min(SAFE_DEFAULT_MAX_ACCOUNTS);
    let entries: Vec<SnapEntry> = lost_accounts
        .iter()
        .take(n)
        .map(|&a| SnapEntry { acct: a, balance_pm: SAFE_DEFAULT_PER_ACCOUNT_PM, reserved_pm: 0, debt_pm: 0 })
        .collect();
    RebuiltLedger {
        entries,
        grade: RebuildGrade::Fallback,
        lost_accounts: lost_accounts.to_vec(),
        conserve_ok: true,
    }
}

/// 重建编排：失败自动落兜底（损失清单显性告知）。
pub fn rebuild_or_fallback(snap: &RestartSnapshot) -> RebuiltLedger {
    match rebuild_from_snapshot(snap) {
        Ok(l) => l,
        Err(_) => {
            let lost: Vec<u32> = snap.entries.iter().map(|e| e.acct).collect();
            fallback_safe_default(&lost)
        }
    }
}

/// 重建演练：注入重启（快照→重建→对账）验证重建路径。
/// 返回 (项数, 全过)。演练走真实 rebuild_from_snapshot 路径。
pub fn rehearse() -> (usize, bool) {
    let mut v: Vec<bool> = Vec::new();
    // 1) 正常快照：完整重建（三类态齐备）。
    let full = RestartSnapshot::new(vec![
        SnapEntry { acct: 1, balance_pm: 400, reserved_pm: 100, debt_pm: 50 },
        SnapEntry { acct: 2, balance_pm: 300, reserved_pm: 50, debt_pm: 100 },
    ]);
    let r1 = rebuild_from_snapshot(&full);
    v.push(r1.as_ref().map(|l| l.grade) == Ok(RebuildGrade::Full));
    // 2) 守恒：重建后总和与快照一致。
    v.push(r1.as_ref().map(|l| l.entries.iter().map(|e| e.balance_pm + e.reserved_pm).sum::<u64>() == full.total_pm) == Ok(true));
    // 3) CRC 破坏：重建拒收（走兜底）。
    let mut corrupt = full.clone();
    corrupt.crc = corrupt.crc ^ 1;
    let r3 = rebuild_from_snapshot(&corrupt);
    v.push(r3 == Err(E_RB_CORRUPT));
    let fb3 = rebuild_or_fallback(&corrupt);
    v.push(fb3.grade == RebuildGrade::Fallback && fb3.lost_accounts.len() == 2);
    // 4) 凭空多预算：total 快照小于实际 → 守恒破缺拒。
    let mut inflate = full.clone();
    inflate.total_pm = 100; // 远小于实际 850
    let r4 = rebuild_from_snapshot(&inflate);
    v.push(r4 == Err(E_RB_CONSERVE));
    // 5) 空快照拒。
    let empty = RestartSnapshot::new(Vec::new());
    v.push(rebuild_from_snapshot(&empty) == Err(E_RB_EMPTY));
    // 6) 兜底安全默认：总量不超 1000‰ 且损失清单非空。
    v.push(fb3.entries.iter().map(|e| e.balance_pm).sum::<u64>() <= SAFE_DEFAULT_TOTAL_PM && !fb3.lost_accounts.is_empty());
    let n = v.len();
    (n, v.iter().all(|&b| b))
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、完整重建：三类态恢复。
    {
        let snap = RestartSnapshot::new(vec![
            SnapEntry { acct: 1, balance_pm: 500, reserved_pm: 100, debt_pm: 40 },
            SnapEntry { acct: 2, balance_pm: 260, reserved_pm: 60, debt_pm: 40 },
            SnapEntry { acct: 3, balance_pm: 40, reserved_pm: 0, debt_pm: 0 },
        ]);
        let r = rebuild_from_snapshot(&snap);
        v.push(("F0456-完整重建-三类态齐备判 Full", r.as_ref().map(|l| l.grade) == Ok(RebuildGrade::Full)));
        // 逐户逐类一致。
        let l = r.unwrap_or_else(|_| RebuiltLedger { entries: Vec::new(), grade: RebuildGrade::Fallback, lost_accounts: Vec::new(), conserve_ok: false });
        let e1 = l.entries.iter().find(|e| e.acct == 1);
        v.push((
            "F0456-完整重建-逐户逐类一致",
            e1.map(|e| e.balance_pm == 500 && e.reserved_pm == 100 && e.debt_pm == 40) == Some(true),
        ));
    }
    // 二、部分重建：诚实分级。
    {
        // 只有余额（无预占无债务样例）→ Partial。
        let snap = RestartSnapshot::new(vec![SnapEntry { acct: 9, balance_pm: 500, reserved_pm: 0, debt_pm: 0 }]);
        let r = rebuild_from_snapshot(&snap);
        v.push(("F0456-完整重建-缺类诚实判 Partial", r.map(|l| l.grade) == Ok(RebuildGrade::Partial)));
    }
    // 三、守恒校验：不许凭空多预算。
    {
        let snap = RestartSnapshot::new(vec![
            SnapEntry { acct: 1, balance_pm: 700, reserved_pm: 100, debt_pm: 0 },
            SnapEntry { acct: 2, balance_pm: 300, reserved_pm: 0, debt_pm: 0 },
        ]);
        // 篡改 total 快照到小于实际 → 拒。
        let mut bad = snap.clone();
        bad.total_pm = 500;
        v.push(("F0456-守恒校验-凭空多预算拒绝", rebuild_from_snapshot(&bad) == Err(E_RB_CONSERVE)));
        // 正常守恒通过。
        v.push(("F0456-守恒校验-正常通过", rebuild_from_snapshot(&snap).map(|l| l.conserve_ok) == Ok(true)));
        // 预占冻结不销账：重建后预占原样。
        let l = rebuild_from_snapshot(&snap).unwrap_or_else(|_| RebuiltLedger { entries: Vec::new(), grade: RebuildGrade::Fallback, lost_accounts: Vec::new(), conserve_ok: false });
        let e1 = l.entries.iter().find(|e| e.acct == 1);
        v.push(("F0456-守恒校验-预占冻结保留", e1.map(|e| e.reserved_pm == 100) == Some(true)));
    }
    // 四、兜底：安全默认 + 损失显性告知。
    {
        let snap = RestartSnapshot::new(vec![
            SnapEntry { acct: 11, balance_pm: 400, reserved_pm: 0, debt_pm: 0 },
            SnapEntry { acct: 22, balance_pm: 400, reserved_pm: 0, debt_pm: 0 },
            SnapEntry { acct: 33, balance_pm: 200, reserved_pm: 0, debt_pm: 0 },
        ]);
        let mut corrupt = snap;
        corrupt.crc = 0xDEAD_BEEF;
        let fb = rebuild_or_fallback(&corrupt);
        v.push(("F0456-兜底-失败落安全默认", fb.grade == RebuildGrade::Fallback));
        v.push(("F0456-兜底-损失清单显性", fb.lost_accounts == vec![11, 22, 33]));
        // 兜底配额：每户保底、总量钳制。
        let sum: u64 = fb.entries.iter().map(|e| e.balance_pm).sum();
        v.push((
            "F0456-兜底-保底份额与总量钳制",
            fb.entries.iter().all(|e| e.balance_pm == SAFE_DEFAULT_PER_ACCOUNT_PM) && sum <= SAFE_DEFAULT_TOTAL_PM,
        ));
        // 兜底不等价于静默清零：entries 非空。
        v.push(("F0456-兜底-非静默清零", !fb.entries.is_empty()));
    }
    // 五、演练：注入重启走真实路径。
    {
        let (n, ok) = rehearse();
        v.push(("F0456-演练-重建路径全过", ok && n >= 6));
        let (n2, ok2) = rehearse();
        v.push(("F0456-演练-幂等", n2 == n && ok2 == ok));
    }
    // 六、判据自检。
    {
        let empty = RestartSnapshot::new(Vec::new());
        v.push(("F0456-判据-空快照拒绝", rebuild_from_snapshot(&empty) == Err(E_RB_EMPTY)));
        v.push(("F0456-判据-兜底账户数钳制", fallback_safe_default(&[1, 2, 3, 4, 5, 6]).entries.len() == SAFE_DEFAULT_MAX_ACCOUNTS));
        // CRC 算法自校验：同数据同 CRC。
        let s1 = RestartSnapshot::new(vec![SnapEntry { acct: 1, balance_pm: 1, reserved_pm: 0, debt_pm: 0 }]);
        let s2 = RestartSnapshot::new(vec![SnapEntry { acct: 1, balance_pm: 1, reserved_pm: 0, debt_pm: 0 }]);
        v.push(("F0456-判据-CRC确定性", s1.crc == s2.crc && s1.verify_crc()));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0456_rebuild_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
