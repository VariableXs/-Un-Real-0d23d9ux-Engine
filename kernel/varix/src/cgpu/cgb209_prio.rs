//! CGPU-F0209 · 帧任务优先级体系深化（判据段 0x06B0）
//! 帧任务优先级的完整体系：优先级维度分离（时间紧迫度 × 业务重要
//! 性 × 用户可见度——三维独立评分，综合优先级 = 加权融合而非简单
//! 排序）、优先级继承（依赖链的优先级传播：高优先级任务的依赖自动
//! 提升——优先级反转的预防性解决）、优先级范围（1-255 级——足够
//! 细粒度但不过度）。判据：三维评分模型文档、继承传播正确、范围
//! 全覆盖、优先级与 CGPU-C 预算仲裁联动、性能。
//!
//! 诊断码段 0x06B0（本模块独占：0x06B0~0x06BF）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x06B0;

/// 优先级范围下限（判据「范围全覆盖」）。
pub const PRIO_MIN: u8 = 1;

/// 优先级范围上限。
pub const PRIO_MAX: u8 = 255;

/// 三维权重默认值（千分比）：紧迫 500 / 重要 300 / 可见 200。
pub const W_URGENCY: u32 = 500;
pub const W_IMPORTANCE: u32 = 300;
pub const W_VISIBILITY: u32 = 200;

/// 三维独立评分（判据「优先级维度分离」载体）：三维各自 0~1000。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TriScore {
    /// 时间紧迫度（0~1000，deadline 临近度）。
    pub urgency: u32,
    /// 业务重要性（0~1000）。
    pub importance: u32,
    /// 用户可见度（0~1000，可见元素 > 不可见）。
    pub visibility: u32,
}

impl TriScore {
    /// 合法性：三维各自封顶。
    pub fn validate(&self) -> Result<(), u16> {
        for v in [self.urgency, self.importance, self.visibility] {
            if v > 1000 {
                return Err(SEG_BASE);
            }
        }
        Ok(())
    }
    /// 加权融合（判据「综合优先级 = 加权融合而非简单排序」核心）：
    /// 综合分 ×1000 → 优先级 1..=255 线性投影（高分 = 高优先级 =
    /// 小号）。权重和必须为 1000。
    pub fn fused_priority(&self, w: (u32, u32, u32)) -> Result<u8, u16> {
        self.validate()?;
        if w.0 + w.1 + w.2 != 1000 {
            return Err(SEG_BASE + 1);
        }
        let fused = (self.urgency as u64 * w.0 as u64
            + self.importance as u64 * w.1 as u64
            + self.visibility as u64 * w.2 as u64)
            / 1000;
        // 投影：分越高优先级号越小（1 = 最高）。fused ∈ [0,1000]。
        let prio = PRIO_MAX as u64 - fused * (PRIO_MAX as u64 - PRIO_MIN as u64) / 1000;
        Ok(prio.clamp(PRIO_MIN as u64, PRIO_MAX as u64) as u8)
    }
}

/// 优先级继承传播（判据「继承传播正确」核心）：依赖链上高优先级
/// 任务的全部（传递）依赖自动提升到「不高于依赖者」——继承取
/// min(自身, 依赖者)（号小 = 优先级高）。
pub fn inherit_priority(direct: &mut [u8], edges: &[(u32, u32)]) -> Result<u32, u16> {
    if direct.is_empty() {
        return Err(SEG_BASE + 2);
    }
    let n = direct.len();
    for &p in direct.iter() {
        if !(PRIO_MIN..=PRIO_MAX).contains(&p) {
            return Err(SEG_BASE + 3);
        }
    }
    for &(u, v) in edges {
        if u == 0 || v == 0 || u as usize > n || v as usize > n || u == v {
            return Err(SEG_BASE + 4);
        }
    }
    // 迭代传播直至不动点（环拒绝：迭代超 n 轮仍有变化即有环）。
    let mut rounds = 0u32;
    loop {
        let mut changed = false;
        for &(u, v) in edges {
            // v 依赖 u：u 优先级不得低于 v（号不得大于 v）。
            if direct[u as usize - 1] > direct[v as usize - 1] {
                direct[u as usize - 1] = direct[v as usize - 1];
                changed = true;
            }
        }
        if !changed {
            break;
        }
        rounds += 1;
        if rounds > n as u32 {
            return Err(SEG_BASE + 5);
        }
    }
    Ok(rounds)
}

/// 继承正确性对拍（判据「继承传播正确」验证面）：传播后任意边的
/// 两端必须满足 dep 端优先级号 ≤ 依赖者端（不反转）。
pub fn inheritance_invariant(direct: &[u8], edges: &[(u32, u32)]) -> Result<(), u16> {
    for &(u, v) in edges {
        let iu = u as usize - 1;
        let iv = v as usize - 1;
        if iu >= direct.len() || iv >= direct.len() {
            return Err(SEG_BASE + 6);
        }
        if direct[iu] > direct[iv] {
            return Err(SEG_BASE + 7);
        }
    }
    Ok(())
}

/// 范围全覆盖扫描（判据「范围全覆盖」主判据）：255 级优先级的
/// 边界语义——1 最高、255 最低、0 非法。
pub fn range_semantics_check() -> Result<(), u16> {
    let hi = TriScore { urgency: 1000, importance: 1000, visibility: 1000 };
    let lo = TriScore { urgency: 0, importance: 0, visibility: 0 };
    let w = (W_URGENCY, W_IMPORTANCE, W_VISIBILITY);
    let p_hi = hi.fused_priority(w)?;
    let p_lo = lo.fused_priority(w)?;
    if p_hi != PRIO_MIN {
        return Err(SEG_BASE + 8);
    }
    if p_lo != PRIO_MAX {
        return Err(SEG_BASE + 9);
    }
    Ok(())
}

/// 帧任务（优先级体系视图）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PrioTask {
    /// 任务号。
    pub id: u32,
    /// 三维评分。
    pub score: TriScore,
    /// 预算申请（微秒）。
    pub budget_us: u32,
}

impl PrioTask {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.id == 0 || self.budget_us == 0 {
            return Err(SEG_BASE + 10);
        }
        self.score.validate()
    }
    /// 融合优先级号。
    pub fn prio(&self) -> Result<u8, u16> {
        self.score.fused_priority((W_URGENCY, W_IMPORTANCE, W_VISIBILITY))
    }
}

/// 优先级-预算仲裁联动（判据「优先级与 CGPU-C 预算仲裁联动」核心）：
/// 预算总池有限时按优先级号升序（高优先级在前）批准，低优先级任务
/// 得到剩余池的按比例削减。返回 (批准表, 拒绝数)。
pub fn budget_arbitration(
    pool_us: u64,
    tasks: &[PrioTask],
) -> Result<(Vec<(u32, u64)>, usize), u16> {
    if tasks.is_empty() || pool_us == 0 {
        return Err(SEG_BASE + 11);
    }
    let mut order: Vec<(u8, u32, u32)> = Vec::new();
    for t in tasks {
        t.validate()?;
        order.push((t.prio()?, t.id, t.budget_us));
    }
    order.sort_by(|a, b| a.0.cmp(&b.0));
    let mut remaining = pool_us;
    let mut granted: Vec<(u32, u64)> = Vec::new();
    let mut denied = 0usize;
    for &(prio, id, want) in &order {
        let want64 = want as u64;
        if remaining >= want64 {
            granted.push((id, want64));
            remaining -= want64;
        } else if remaining > 0 {
            // 按比例削减：给剩余池的（优先级越高削得越少——高优先
            // 级至少保留 80%，低优先级最多拿到剩余）。
            let floor_permille = if prio <= 64 {
                1000u64
            } else if prio <= 128 {
                900
            } else {
                800
            };
            let give = (remaining * floor_permille / 1000).min(want64);
            if give > 0 {
                granted.push((id, give));
                remaining -= give;
            } else {
                denied += 1;
            }
        } else {
            denied += 1;
        }
    }
    Ok((granted, denied))
}

/// 仲裁公平性审计（判据「预算仲裁联动」审计面）：批准表必须满足
/// ① 高优先级任务获批 ≥ 其申请的 80% ② 拒绝者的优先级号全部 ≥
/// 任一被削减者的优先级号（不牺牲高优先级）。
pub fn arbitration_fairness(
    tasks: &[PrioTask],
    granted: &[(u32, u64)],
    denied: usize,
) -> Result<(), u16> {
    if granted.is_empty() {
        return Err(SEG_BASE + 12);
    }
    let mut max_denied_prio: u8 = 0;
    for t in tasks {
        let p = t.prio()?;
        let got = granted.iter().find(|(id, _)| *id == t.id);
        match got {
            Some((_, amt)) => {
                // 只审计高优先级（≤128）的下限保护。
                if p <= 128 && *amt * 10 < t.budget_us as u64 * 8 {
                    return Err(SEG_BASE + 13);
                }
            }
            None => max_denied_prio = max_denied_prio.max(p),
        }
    }
    if denied > 0 && max_denied_prio == 0 {
        return Err(SEG_BASE + 14);
    }
    // 被削减者（有批准但 < 80%）不得优先级号小于被拒者。
    for (id, amt) in granted {
        let t = tasks.iter().find(|t| t.id == *id).ok_or(SEG_BASE + 15)?;
        let p = t.prio()?;
        if *amt * 10 < t.budget_us as u64 * 8 && p > max_denied_prio && max_denied_prio > 0 {
            return Err(SEG_BASE + 16);
        }
    }
    Ok(())
}

/// 继承传播性能核算（判据「性能」面）：传播轮数上界 = 依赖深度；
/// 深链（n 节点全链）恰好 n-1 轮收敛——超出即异常。
pub fn propagation_rounds_bound(rounds: u32, chain_len: u32) -> Result<(), u16> {
    if chain_len == 0 || rounds == 0 {
        return Err(SEG_BASE + 17);
    }
    if rounds > chain_len.saturating_sub(1).max(1) {
        return Err(SEG_BASE + 18);
    }
    Ok(())
}

/// 三维评分模型文档指纹（判据「三维评分模型文档」锚点）：权重三
/// 元组 + 范围边界的冻结指纹——权重变更必须显式改版。
pub fn score_model_fp(w: (u32, u32, u32)) -> Result<u64, u16> {
    if w.0 + w.1 + w.2 != 1000 {
        return Err(SEG_BASE + 19);
    }
    let mut acc: u64 = 0xcbf29ce484222325;
    acc = acc.wrapping_mul(0x100000001b3) ^ w.0 as u64;
    acc = acc.wrapping_mul(0x100000001b3) ^ w.1 as u64;
    acc = acc.wrapping_mul(0x100000001b3) ^ w.2 as u64;
    acc = acc.wrapping_mul(0x100000001b3) ^ PRIO_MIN as u64;
    acc = acc.wrapping_mul(0x100000001b3) ^ PRIO_MAX as u64;
    Ok(acc)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fused_projection_monotonic() {
        let hi = TriScore { urgency: 900, importance: 800, visibility: 700 };
        let lo = TriScore { urgency: 100, importance: 200, visibility: 300 };
        let w = (W_URGENCY, W_IMPORTANCE, W_VISIBILITY);
        let p_hi = hi.fused_priority(w).unwrap();
        let p_lo = lo.fused_priority(w).unwrap();
        assert!(p_hi < p_lo);
        assert!((PRIO_MIN..=PRIO_MAX).contains(&p_hi));
        assert!((PRIO_MIN..=PRIO_MAX).contains(&p_lo));
    }
    #[test]
    fn weights_must_sum_1000() {
        let s = TriScore { urgency: 500, importance: 500, visibility: 500 };
        assert!(s.fused_priority((400, 300, 200)).is_err());
        assert!(s.fused_priority((500, 300, 200)).is_ok());
    }
    #[test]
    fn inheritance_propagates_and_invariant() {
        // 链 3→2→1（1 依赖 2，2 依赖 3）：v=3 高优先级（10 号），
        // 上游 2、1 自动提升。
        let mut prios = vec![100u8, 100, 10];
        let edges = vec![(2u32, 3u32), (1, 2)]; // 1→2: 1 依赖 2; 2→3: 2 依赖 3。
        let rounds = inherit_priority(&mut prios, &edges).unwrap();
        assert_eq!(prios, vec![10, 10, 10]);
        inheritance_invariant(&prios, &edges).unwrap();
        propagation_rounds_bound(rounds, 3).unwrap();
    }
    #[test]
    fn cycle_rejected_in_inheritance() {
        let mut prios = vec![50u8, 50, 50];
        let edges = vec![(1u32, 2), (2, 3), (3, 1)];
        assert!(inherit_priority(&mut prios, &edges).is_err());
    }
    #[test]
    fn range_semantics_full() {
        assert!(range_semantics_check().is_ok());
    }
    #[test]
    fn arbitration_priority_first() {
        let tasks = vec![
            PrioTask { id: 1, score: TriScore { urgency: 100, importance: 100, visibility: 100 }, budget_us: 5000 },
            PrioTask { id: 2, score: TriScore { urgency: 900, importance: 900, visibility: 900 }, budget_us: 3000 },
        ];
        let (granted, denied) = budget_arbitration(4000, &tasks).unwrap();
        // 高优先级任务 2 先拿满 3000，任务 1 拿剩余 1000。
        assert_eq!(granted.len(), 2);
        assert_eq!(granted[0], (2, 3000));
        assert_eq!(granted[1], (1, 1000));
        assert_eq!(denied, 0);
        arbitration_fairness(&tasks, &granted, denied).unwrap();
    }
    #[test]
    fn arbitration_denies_low_first() {
        let tasks = vec![
            PrioTask { id: 1, score: TriScore { urgency: 50, importance: 50, visibility: 50 }, budget_us: 9000 },
            PrioTask { id: 2, score: TriScore { urgency: 950, importance: 950, visibility: 950 }, budget_us: 2000 },
        ];
        let (granted, denied) = budget_arbitration(3000, &tasks).unwrap();
        assert_eq!(granted[0], (2, 2000));
        assert_eq!(granted.len(), 2); // 任务 1 拿 1000（削减）。
        assert_eq!(denied, 0);
        arbitration_fairness(&tasks, &granted, denied).unwrap();
    }
    #[test]
    fn model_fp_frozen() {
        let f1 = score_model_fp((500, 300, 200)).unwrap();
        let f2 = score_model_fp((500, 300, 200)).unwrap();
        assert_eq!(f1, f2);
        assert!(score_model_fp((400, 400, 200)).is_err());
    }
}
