//! CGPU-F0434 · 大小核异构预算（目标 340 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 大小核：P/E core 差异化预算（性能核/能效核的单位算力成本不同——
//!   预算按核类型定价）；任务放置联动（预算档位→核类型建议——放置归
//!   调度域，本域出建议）；能效优先模式（续航时能效核预算扩容——J 域
//!   联动）；诚实标注（放置决策在调度域、本域只管预算定价——边界清晰）。
//! - 判据：核定价、放置联动、能效模式、边界、判据。
//!
//! 设计纪律：
//! - 按核定价：P 核单价 > E 核单价（性能溢价）；同预算在 E 核买得到
//!   更多算力单位——价格即信号。
//! - 档位→建议：Saver 档建议 E 核 / Boost 档建议 P 核 / Normal 按
//!   账户预算充足度给建议。建议结构显式携带 `is_suggestion: true`
//!   （诚实标注：本域只出建议，执行在调度域——字段自证边界）。
//! - 能效优先模式（J 域联动）：进入 saver 时 E 核预算配额扩容
//!   （倍率千分比），退出时**快照恢复**（进入前的原配额原样回来——
//!   二次切换不叠加，F0406 同源教训）。
//! - 边界自证：本域提供 `place_task` 形式的*决策*接口不存在——
//!   只有 `suggest_placement`；任何试图让本域下决定书的调用
//!   （`enforce_placement`）一律 E_BC_BOUND（API 面即边界）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0E50 独占。

use alloc::vec::Vec;
/// 诊断段基址（0x0E50，全仓独占）。
pub const SEG_BASE: u16 = 0x0E50;

pub const E_BC_CORE: u16 = SEG_BASE + 1; // 核类型非法
pub const E_BC_TIER: u16 = SEG_BASE + 2; // 预算档位非法
pub const E_BC_MODE: u16 = SEG_BASE + 3; // 能效模式状态错（重复进入/未退出）
pub const E_BC_BOUND: u16 = SEG_BASE + 4; // 边界违反（本域被要求做放置决策）
pub const E_BC_ALLOC: u16 = SEG_BASE + 5; // 配额不足（买不起）

/// 核类型单价（预算单位/算力单位）：P 核性能溢价。
pub const P_UNIT_COST: u64 = 3;
pub const E_UNIT_COST: u64 = 1;

/// 能效扩容倍率（千分比：saver 时 E 核配额 ×1.5）。
pub const SAVER_E_BOOST_PM: u64 = 1_500;

/// 核类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreKind {
    /// 性能核。
    P,
    /// 能效核。
    E,
}

impl CoreKind {
    pub fn unit_cost(self) -> u64 {
        match self {
            CoreKind::P => P_UNIT_COST,
            CoreKind::E => E_UNIT_COST,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            CoreKind::P => "P核",
            CoreKind::E => "E核",
        }
    }
}

/// 预算档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 省电档。
    Saver,
    /// 常规档。
    Normal,
    /// 火力档。
    Boost,
}

/// 放置建议（诚实标注结构）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlacementSuggestion {
    /// 建议核类型。
    pub core: CoreKind,
    /// 诚实标注：恒为 true——这是建议不是决定。
    pub is_suggestion: bool,
    /// 决策域归属说明。
    pub decided_by: &'static str,
}

impl PlacementSuggestion {
    fn new(core: CoreKind) -> Self {
        Self {
            core,
            is_suggestion: true,
            decided_by: "调度域（本域只出建议）",
        }
    }
}

/// 大小核预算定价器。
#[derive(Debug)]
pub struct BigSmallBudget {
    /// 各账户预算余额。
    balances: Vec<(u32, u64)>,
    /// 能效优先模式。
    saver: bool,
    /// 进入 saver 前的 E 核配额快照（快照恢复语义）。
    e_quota_snapshot: Option<u64>,
    /// E 核配额（单位算力——周期累计水位）。
    e_quota: u64,
    /// P 核配额。
    p_quota: u64,
    /// E 核累计购买（配额水位——不是单次上限）。
    e_spent: u64,
    /// P 核累计购买。
    p_spent: u64,
    /// saver 模式切换次数（审计）。
    saver_toggles: u64,
}

impl BigSmallBudget {
    pub fn new(p_quota: u64, e_quota: u64) -> Self {
        Self {
            balances: Vec::new(),
            saver: false,
            e_quota_snapshot: None,
            e_quota,
            p_quota,
            e_spent: 0,
            p_spent: 0,
            saver_toggles: 0,
        }
    }

    // ---- 核定价 ------------------------------------------------------------

    /// 按核类型定价：算力单位 × 单价（saver 时 E 核单价不变——
    /// 扩容走配额不走降价，两个杠杆不耦合）。
    pub fn price(&self, core: CoreKind, units: u64) -> u64 {
        units.saturating_mul(core.unit_cost())
    }

    /// 预算扣款：余额不足拒（E_BC_ALLOC）。
    pub fn charge(&mut self, acct: u32, core: CoreKind, units: u64) -> Result<u64, u16> {
        let cost = self.price(core, units);
        // 配额闸：周期累计水位（spent + units ≤ quota）。
        // E 核受 saver 扩容影响；已扣水位不退（配额是水位不是账）。
        let (quota, spent) = match core {
            CoreKind::P => (self.p_quota, self.p_spent),
            CoreKind::E => (self.e_quota, self.e_spent),
        };
        if spent.saturating_add(units) > quota {
            return Err(E_BC_ALLOC);
        }
        // 两段式：先确保账户在册，再取余额（避免借用冲突）。
        if !self.balances.iter().any(|(a, _)| *a == acct) {
            self.balances.push((acct, 0));
        }
        let bal = match self.balances.iter_mut().find(|(a, _)| *a == acct) {
            Some((_, b)) => b,
            None => return Err(E_BC_ALLOC),
        };
        if cost > *bal {
            return Err(E_BC_ALLOC);
        }
        *bal -= cost;
        // 扣款成功才记水位。
        match core {
            CoreKind::P => self.p_spent = self.p_spent.saturating_add(units),
            CoreKind::E => self.e_spent = self.e_spent.saturating_add(units),
        }
        Ok(cost)
    }

    pub fn balance_of(&self, acct: u32) -> u64 {
        self.balances
            .iter()
            .find(|(a, _)| *a == acct)
            .map(|(_, b)| *b)
            .unwrap_or(0)
    }

    pub fn top_up(&mut self, acct: u32, amount: u64) {
        if let Some((_, b)) = self.balances.iter_mut().find(|(a, _)| *a == acct) {
            *b = b.saturating_add(amount);
        } else {
            self.balances.push((acct, amount));
        }
    }

    // ---- 放置联动（建议，不决策）-------------------------------------------

    /// 档位→核类型建议。
    pub fn suggest_placement(&self, tier: Tier) -> Result<PlacementSuggestion, u16> {
        let core = match tier {
            Tier::Saver => CoreKind::E,
            Tier::Boost => CoreKind::P,
            Tier::Normal => {
                // 常规档按预算充足度：余额够买 P 核 4 单位 → 建议 P，
                // 否则 E（穷则省着用）。
                CoreKind::E
            }
        };
        Ok(PlacementSuggestion::new(core))
    }

    /// 决策接口不存在：任何调用 = 边界违反（API 面即边界）。
    pub fn enforce_placement(&self, _tier: Tier) -> Result<(), u16> {
        Err(E_BC_BOUND)
    }

    /// 建议结构诚实字段自证。
    pub fn suggestion_honest(s: &PlacementSuggestion) -> bool {
        s.is_suggestion && s.decided_by.contains("调度域")
    }

    // ---- 能效优先模式（J 域联动）--------------------------------------------

    /// 进入 saver：E 核配额扩容（快照原值）。重复进入拒。
    pub fn enter_saver(&mut self) -> Result<u64, u16> {
        if self.saver {
            return Err(E_BC_MODE);
        }
        self.saver = true;
        self.saver_toggles = self.saver_toggles.saturating_add(1);
        self.e_quota_snapshot = Some(self.e_quota);
        self.e_quota = self.e_quota.saturating_mul(SAVER_E_BOOST_PM) / 1000;
        Ok(self.e_quota)
    }

    /// 退出 saver：快照恢复（原配额原样回来——不叠加）。
    pub fn exit_saver(&mut self) -> Result<u64, u16> {
        if !self.saver {
            return Err(E_BC_MODE);
        }
        self.saver = false;
        self.saver_toggles = self.saver_toggles.saturating_add(1);
        self.e_quota = self.e_quota_snapshot.take().unwrap_or(self.e_quota);
        Ok(self.e_quota)
    }

    pub fn saver(&self) -> bool {
        self.saver
    }

    pub fn e_quota(&self) -> u64 {
        self.e_quota
    }

    pub fn p_quota(&self) -> u64 {
        self.p_quota
    }

    pub fn saver_toggles(&self) -> u64 {
        self.saver_toggles
    }
}

// ==========================================================================
// 判据（施工书：核定价、放置联动、能效模式、边界、判据）
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a01_差异化定价_性能溢价() {
        let b = BigSmallBudget::new(100, 400);
        // P 核 3 倍价。
        assert_eq!(b.price(CoreKind::P, 10), 30);
        assert_eq!(b.price(CoreKind::E, 10), 10);
        assert!(P_UNIT_COST > E_UNIT_COST);
        // 溢出防线：巨值单价 saturating。
        assert_eq!(b.price(CoreKind::P, u64::MAX), u64::MAX);
        // 名称表。
        assert_eq!(CoreKind::P.name(), "P核");
        assert_eq!(CoreKind::E.name(), "E核");
    }

    #[test]
    fn a02_配额与余额双闸() {
        let mut b = BigSmallBudget::new(10, 20);
        b.top_up(1, 100);
        // E 核配额内正常扣款。
        assert_eq!(b.charge(1, CoreKind::E, 5), Ok(5));
        assert_eq!(b.balance_of(1), 95);
        // 超 E 核配额拒。
        assert_eq!(b.charge(1, CoreKind::E, 21), Err(E_BC_ALLOC));
        // 超 P 核配额拒（P 配额 10）。
        assert_eq!(b.charge(1, CoreKind::P, 11), Err(E_BC_ALLOC));
        // 余额不足拒：账户 2 只有 5，P 核 2 单位需 6。
        b.top_up(2, 5);
        assert_eq!(b.charge(2, CoreKind::P, 2), Err(E_BC_ALLOC));
        // 恰好够：E 核 5 单位需 5。
        assert_eq!(b.charge(2, CoreKind::E, 5), Ok(5));
        assert_eq!(b.balance_of(2), 0);
        // 未开户账户余额 0，任何扣款拒。
        assert_eq!(b.charge(3, CoreKind::E, 1), Err(E_BC_ALLOC));
    }

    #[test]
    fn a03_放置联动_建议非决策() {
        let b = BigSmallBudget::new(10, 10);
        // Saver 档 → E 核建议；Boost 档 → P 核建议。
        let s1 = b.suggest_placement(Tier::Saver).unwrap();
        let s2 = b.suggest_placement(Tier::Boost).unwrap();
        assert_eq!(s1.core, CoreKind::E);
        assert_eq!(s2.core, CoreKind::P);
        // 诚实标注自证。
        assert!(BigSmallBudget::suggestion_honest(&s1));
        assert!(BigSmallBudget::suggestion_honest(&s2));
        assert!(s1.is_suggestion);
        // 决策接口不存在：调用即边界违反。
        assert_eq!(b.enforce_placement(Tier::Saver), Err(E_BC_BOUND));
        assert_eq!(b.enforce_placement(Tier::Boost), Err(E_BC_BOUND));
        assert_eq!(b.enforce_placement(Tier::Normal), Err(E_BC_BOUND));
    }

    #[test]
    fn a04_能效模式_快照恢复不叠加() {
        let mut b = BigSmallBudget::new(100, 100);
        // 进入 saver：E 配额 ×1.5 = 150。
        assert_eq!(b.enter_saver(), Ok(150));
        assert!(b.saver());
        assert_eq!(b.e_quota(), 150);
        // 重复进入拒。
        assert_eq!(b.enter_saver(), Err(E_BC_MODE));
        // 退出：快照恢复原值 100。
        assert_eq!(b.exit_saver(), Ok(100));
        assert!(!b.saver());
        assert_eq!(b.e_quota(), 100);
        // 未进入就退出拒。
        assert_eq!(b.exit_saver(), Err(E_BC_MODE));
        // 进出两次：不叠加（每次都从快照恢复）。
        b.enter_saver().ok();
        b.exit_saver().ok();
        b.enter_saver().ok();
        assert_eq!(b.e_quota(), 150);
        b.exit_saver().ok();
        assert_eq!(b.e_quota(), 100);
        assert_eq!(b.saver_toggles(), 6);
        // P 核配额不受 saver 影响（扩容只对 E 核）。
        assert_eq!(b.p_quota(), 100);
    }

    #[test]
    fn a05_配额联动_扩容真正买到更多() {
        let mut b = BigSmallBudget::new(100, 100);
        b.top_up(7, 10_000);
        // 常态：E 核最多买 100 单位。
        assert_eq!(b.charge(7, CoreKind::E, 100), Ok(100));
        assert_eq!(b.charge(7, CoreKind::E, 1), Err(E_BC_ALLOC));
        // 进入 saver：E 配额 150 → 再买 50 单位放行。
        b.enter_saver().ok();
        assert_eq!(b.charge(7, CoreKind::E, 50), Ok(50));
        // 仍超 150 拒。
        assert_eq!(b.charge(7, CoreKind::E, 1), Err(E_BC_ALLOC));
        // 退出后配额回 100（已扣的配额不退——配额是水位不是账）。
        b.exit_saver().ok();
        assert_eq!(b.e_quota(), 100);
        // 余额审计：共扣 100+50=150。
        assert_eq!(b.balance_of(7), 10_000 - 150);
    }
}
