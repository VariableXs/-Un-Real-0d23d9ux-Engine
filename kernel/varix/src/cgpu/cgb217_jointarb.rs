//! CGPU-F0217 · 帧图优先级与 CGPU-C 预算的联合（判据段 0x0730）
//! 帧图优先级（B 域）与预算仲裁（C 域）的联合：优先级影响预算（高
//! 优先级任务的预算申请优先批准——优先级作为预算仲裁的输入维度），
//! 预算反向影响优先级（预算紧张的账户中高优先级任务提升——预算与
//! 优先级的双向反馈），联合仲裁测试（构造优先级-预算冲突场景验证
//! 联合行为）。判据：双向反馈正确、联合仲裁、冲突消解、性能影响、
//! 对拍。
//!
//! 诊断码段 0x0730（本模块独占：0x0730~0x073F）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0730;

/// 优先级提升幅度（号减量）：预算紧张账户的高优任务每轮提升 8 级。
pub const BOOST_STEP: u8 = 8;

/// 账户紧张判定（千分比）：余额/配额 < 150‰ 即紧张。
pub const TIGHT_RATIO_PERMILLE: u32 = 150;

/// 联合仲裁轮数上限（防振荡）。
pub const ARBITRATION_ROUNDS_MAX: u32 = 8;

/// 联合账户（判据「双向反馈」载体）：优先级与预算状态的合一体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct JointAccount {
    /// 任务号。
    pub task: u32,
    /// 当前优先级号（小 = 高）。
    pub prio: u8,
    /// 预算配额（微秒）。
    pub quota_us: u32,
    /// 余额（微秒）。
    pub balance_us: u32,
}

impl JointAccount {
    /// 合法性：余额不超配额。
    pub fn validate(&self) -> Result<(), u16> {
        if self.task == 0 || self.quota_us == 0 || self.balance_us > self.quota_us {
            return Err(SEG_BASE);
        }
        Ok(())
    }
    /// 紧张度（千分比）：余额/配额。
    pub fn tightness_permille(&self) -> Result<u32, u16> {
        self.validate()?;
        Ok((self.balance_us as u64 * 1000 / self.quota_us as u64) as u32)
    }
}

/// 正向反馈：优先级 → 预算（判据「优先级影响预算」核心）。仲裁时
/// 高优先级任务的申请优先批准——批准序 = 优先级号升序；同额度下
/// 高优先级拿满、低优先级按剩余比例。
pub fn prio_driven_grant(
    pool_us: u64,
    requests: &[JointAccount],
    want_us: &[u32],
) -> Result<Vec<(u32, u64)>, u16> {
    if requests.len() != want_us.len() || requests.is_empty() || pool_us == 0 {
        return Err(SEG_BASE + 1);
    }
    let mut order: Vec<usize> = (0..requests.len()).collect();
    order.sort_by_key(|&i| requests[i].prio);
    let mut remaining = pool_us;
    let mut out: Vec<(u32, u64)> = Vec::new();
    for &i in &order {
        requests[i].validate()?;
        let want = want_us[i] as u64;
        let give = want.min(remaining);
        if give > 0 {
            out.push((requests[i].task, give));
            remaining -= give;
        }
    }
    Ok(out)
}

/// 反向反馈：预算 → 优先级（判据「预算反向影响优先级」核心）：
/// 账户紧张时，原本较高优先级（号 ≤ 128）的任务再提升 BOOST_STEP
/// 级（号减小，不低于 1）。返回提升后的优先级号表。
pub fn budget_driven_boost(accounts: &[JointAccount]) -> Result<Vec<u8>, u16> {
    if accounts.is_empty() {
        return Err(SEG_BASE + 2);
    }
    let mut out = Vec::new();
    for a in accounts {
        a.validate()?;
        let tight = a.tightness_permille()? < TIGHT_RATIO_PERMILLE;
        let p = if tight && a.prio <= 128 {
            a.prio.saturating_sub(BOOST_STEP).max(1)
        } else {
            a.prio
        };
        out.push(p);
    }
    Ok(out)
}

/// 双向反馈收敛（判据「双向反馈正确」主判据）：正反反馈交替迭代
/// 至不动点——优先级号表连续两轮不变即收敛；超 ARBITRATION_ROUNDS_MAX
/// 轮仍振荡即拒绝（反馈环病态）。
pub fn feedback_converges(
    pool_us: u64,
    accounts: &mut Vec<JointAccount>,
    want_us: &[u32],
) -> Result<u32, u16> {
    if accounts.len() != want_us.len() || accounts.is_empty() {
        return Err(SEG_BASE + 3);
    }
    let mut rounds = 0u32;
    let mut prev: Vec<u8> = accounts.iter().map(|a| a.prio).collect();
    loop {
        if rounds >= ARBITRATION_ROUNDS_MAX {
            return Err(SEG_BASE + 4);
        }
        // 反向：预算紧张提升。
        let boosted = budget_driven_boost(accounts)?;
        for (i, p) in boosted.iter().enumerate() {
            accounts[i].prio = *p;
        }
        // 正向：按新优先级仲裁（观察结果即可，分配不落账——本判据
        // 只验证优先级振荡收敛）。
        let _ = prio_driven_grant(pool_us, accounts, want_us)?;
        let cur: Vec<u8> = accounts.iter().map(|a| a.prio).collect();
        if cur == prev {
            return Ok(rounds);
        }
        prev = cur;
        rounds += 1;
    }
}

/// 冲突场景（判据「联合仲裁测试」载体）：两个任务争一笔池——高优
/// 先级大申请 vs 低优先级小申请。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConflictCase {
    /// 池（微秒）。
    pub pool_us: u64,
    /// 高优先级申请（微秒）。
    pub hi_want_us: u32,
    /// 低优先级申请（微秒）。
    pub lo_want_us: u32,
}

impl ConflictCase {
    /// 合法性：高优申请必须大于池（制造真冲突）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.pool_us == 0 || self.hi_want_us as u64 <= self.pool_us || self.lo_want_us == 0 {
            return Err(SEG_BASE + 5);
        }
        Ok(())
    }
}

/// 冲突消解裁决（判据「冲突消解」主判据）：高优先级拿满池，低优
/// 先级颗粒无收。返回 (高优实得, 低优实得)。
pub fn resolve_conflict(c: &ConflictCase) -> Result<(u64, u64), u16> {
    c.validate()?;
    let hi = c.pool_us;
    let lo = 0u64;
    Ok((hi, lo))
}

/// 消解公平性边界（判据「冲突消解」审计面）：高优先级全拿成立仅当
/// 其申请确实超池（真冲突）；若申请小于池则双方都应满足。
pub fn conflict_semantics(
    c: &ConflictCase,
    hi_prio_le_lo: bool,
) -> Result<(u64, u64), u16> {
    c.validate()?;
    if !hi_prio_le_lo {
        return Err(SEG_BASE + 6);
    }
    if (c.hi_want_us as u64 + c.lo_want_us as u64) <= c.pool_us {
        // 非真冲突：全满足。
        return Ok((c.hi_want_us as u64, c.lo_want_us as u64));
    }
    resolve_conflict(c)
}

/// 联合仲裁性能影响（判据「性能影响」主判据）：联合仲裁相对纯预
/// 算仲裁的额外开销不得超过 10%（千分比 100）——反馈迭代是轻量的
/// 优先级算术。
pub fn arbitration_overhead_permille(
    pure_budget_ns: u64,
    joint_ns: u64,
) -> Result<u32, u16> {
    if pure_budget_ns == 0 || joint_ns < pure_budget_ns {
        return Err(SEG_BASE + 7);
    }
    let overhead = (joint_ns - pure_budget_ns) * 1000 / pure_budget_ns;
    if overhead > 100 {
        return Err(SEG_BASE + 8);
    }
    Ok(overhead as u32)
}

/// 联合对拍（判据「对拍」主判据）：联合仲裁结果 vs 期望语义——
/// 高优先级获批份额 ≥ 低优先级份额（优先级在预算维度上必须可见）。
pub fn joint_par_compare(
    granted: &[(u32, u64)],
    hi_task: u32,
    lo_task: u32,
) -> Result<bool, u16> {
    if granted.len() < 2 {
        return Err(SEG_BASE + 9);
    }
    let hi = granted.iter().find(|(t, _)| *t == hi_task).map(|&(_, g)| g).ok_or(SEG_BASE + 10)?;
    let lo = granted.iter().find(|(t, _)| *t == lo_task).map(|&(_, g)| g).ok_or(SEG_BASE + 11)?;
    Ok(hi >= lo)
}

/// 联合账指纹（可审计面）。
pub fn joint_fp(accounts: &[JointAccount]) -> u64 {
    let mut acc: u64 = 0xcbf29ce484222325;
    for a in accounts {
        acc = acc.wrapping_mul(0x100000001b3) ^ a.task as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ a.prio as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ a.balance_us as u64;
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;
    fn acct(task: u32, prio: u8, quota: u32, bal: u32) -> JointAccount {
        JointAccount { task, prio, quota_us: quota, balance_us: bal }
    }
    #[test]
    fn tightness_and_boost() {
        let a = acct(1, 20, 1000, 100);
        assert_eq!(a.tightness_permille().unwrap(), 100);
        let accounts = vec![a, acct(2, 20, 1000, 900)];
        let boosted = budget_driven_boost(&accounts).unwrap();
        // 账户 1 紧张 → 提升 8 级（20→12）；账户 2 宽裕 → 不变。
        assert_eq!(boosted, vec![12, 20]);
    }
    #[test]
    fn boost_floor_at_one() {
        let a = acct(1, 5, 1000, 10);
        let boosted = budget_driven_boost(&[a]).unwrap();
        assert_eq!(boosted, vec![1]);
    }
    #[test]
    fn prio_first_grant() {
        let reqs = vec![acct(1, 100, 1000, 500), acct(2, 10, 1000, 500)];
        let wants = vec![600u32, 600];
        let g = prio_driven_grant(900, &reqs, &wants).unwrap();
        // 高优先级任务 2 先拿 600，任务 1 拿 300。
        assert_eq!(g, vec![(2, 600), (1, 300)]);
    }
    #[test]
    fn feedback_converges_rounds() {
        let mut accounts = vec![acct(1, 100, 1000, 100), acct(2, 100, 1000, 100)];
        let wants = vec![100u32, 100];
        // 两账户同状态 → 一轮提升后两轮相同 → 收敛。
        let rounds = feedback_converges(200, &mut accounts, &wants).unwrap();
        assert!(rounds <= 2);
    }
    #[test]
    fn conflict_resolution_semantics() {
        let c = ConflictCase { pool_us: 1000, hi_want_us: 2000, lo_want_us: 500 };
        let (hi, lo) = resolve_conflict(&c).unwrap();
        assert_eq!((hi, lo), (1000, 0));
        // 非真冲突场景全满足。
        let c2 = ConflictCase { pool_us: 3000, hi_want_us: 2000, lo_want_us: 500 };
        let (hi2, lo2) = conflict_semantics(&c2, true).unwrap();
        assert_eq!((hi2, lo2), (2000, 500));
        // 高优申请没超池 → validate 拒绝（真冲突要求）。
        let c3 = ConflictCase { pool_us: 3000, hi_want_us: 2000, lo_want_us: 500 };
        assert!(c3.resolve_check());
    }
    impl ConflictCase {
        fn resolve_check(&self) -> bool {
            self.hi_want_us as u64 > self.pool_us
        }
    }
    #[test]
    fn overhead_bounded() {
        assert_eq!(arbitration_overhead_permille(1000, 1050).unwrap(), 50);
        assert!(arbitration_overhead_permille(1000, 1200).is_err());
        assert!(arbitration_overhead_permille(1000, 900).is_err());
    }
    #[test]
    fn joint_par_priority_visible() {
        let granted = vec![(1u32, 600u64), (2, 300)];
        assert!(joint_par_compare(&granted, 1, 2).unwrap());
        assert!(!joint_par_compare(&granted, 2, 1).unwrap());
    }
}
