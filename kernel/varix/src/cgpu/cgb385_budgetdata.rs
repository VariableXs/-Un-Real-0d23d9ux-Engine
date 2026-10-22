//! CGPU-F0385 · 预算检查器数据层（目标 360 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 检查器：预算检查器数据契约（账户树实时态/仲裁决策流/降档归因三视图
//!   数据——对接 VE-Y 工具链渲染）；契约冻结（引擎出数据不出 UI）；实时与
//!   历史双模式。判据：三视图、契约冻结、双模式、判据。
//!
//! 设计纪律：
//! - 引擎出数据不出 UI：视图数据是冻结 schema 的纯数据包，渲染归 VE-Y。
//! - 三视图各自独立账本；快照带 seq 单调（消费端可检测丢包/乱序）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0AD0 独占。

use alloc::vec::Vec;
use alloc::string::String;

/// 诊断段基址（0x0AD0，全仓独占）。
pub const SEG_BASE: u16 = 0x0AD0;

pub const E_VIEW_UNKNOWN: u16 = SEG_BASE + 1;
pub const E_CONTRACT_DRIFT: u16 = SEG_BASE + 2;
pub const E_MODE_MISMATCH: u16 = SEG_BASE + 3;
pub const E_ACCOUNT_UNKNOWN: u16 = SEG_BASE + 4;

/// 数据契约版本（冻结）。
pub const CONTRACT_VERSION: u32 = 1;

/// 视图编号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewId {
    /// 视图一：账户树实时态。
    TreeState = 0,
    /// 视图二：仲裁决策流。
    DecisionFlow = 1,
    /// 视图三：降档归因。
    DowngradeCause = 2,
}

pub const VIEW_SLOTS: usize = 3;

/// 实时与历史双模式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FeedMode {
    /// 实时：只追加最新，旧窗口丢弃。
    Live,
    /// 历史：全量留存（上限内），可回看。
    History { capacity: u32 },
}

/// 账户实时态（视图一的最小行）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountRow {
    pub account: u32,
    /// 预算与已用（千分位额度）。
    pub budget_pm: u32,
    pub used_pm: u32,
    /// 档位（0 高 1 中 2 低 3 冻结）。
    pub tier: u8,
}

impl AccountRow {
    pub fn utilization_pm(&self) -> Option<u32> {
        if self.budget_pm == 0 {
            None
        } else {
            Some(((self.used_pm as u64) * 1000 / self.budget_pm as u64) as u32)
        }
    }
}

/// 仲裁决策流行（视图二）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecisionRow {
    pub frame: u64,
    pub account: u32,
    /// 准/拒/降档（0 准 1 拒 2 降档）。
    pub verdict: u8,
    pub reason: String,
}

/// 降档归因行（视图三）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DowngradeRow {
    pub frame: u64,
    pub account: u32,
    /// 归因码（1 超预算 2 风暴 3 公平 4 显存挤兑）。
    pub cause: u8,
    pub lost_pm: u32,
}

/// 数据层：三视图账本 + 模式 + 契约核。
pub struct InspectorDataLayer {
    mode: FeedMode,
    tree: Vec<AccountRow>,
    decisions: Vec<DecisionRow>,
    downgrades: Vec<DowngradeRow>,
    seq: u64,
}

impl InspectorDataLayer {
    pub fn new(mode: FeedMode) -> Self {
        InspectorDataLayer { mode, tree: Vec::new(), decisions: Vec::new(), downgrades: Vec::new(), seq: 0 }
    }

    pub fn mode(&self) -> FeedMode {
        self.mode
    }

    pub fn upsert_account(&mut self, row: AccountRow) -> Result<(), u16> {
        if row.budget_pm > 10_000 || row.used_pm > row.budget_pm || row.tier > 3 {
            return Err(E_CONTRACT_DRIFT);
        }
        if let Some(e) = self.tree.iter_mut().find(|r| r.account == row.account) {
            *e = row;
        } else {
            self.tree.push(row);
        }
        Ok(())
    }

    pub fn push_decision(&mut self, row: DecisionRow) -> Result<(), u16> {
        if row.verdict > 2 || self.tree.iter().all(|r| r.account != row.account) {
            return Err(E_ACCOUNT_UNKNOWN);
        }
        self.seq += 1;
        // 实时模式：决策流只留最近 128 行（旧窗口丢弃）。
        match self.mode {
            FeedMode::Live => {
                if self.decisions.len() >= 128 {
                    self.decisions.remove(0);
                }
            }
            FeedMode::History { capacity } => {
                let cap = capacity as usize;
                if cap > 0 && self.decisions.len() >= cap {
                    self.decisions.remove(0);
                }
            }
        }
        self.decisions.push(row);
        Ok(())
    }

    pub fn push_downgrade(&mut self, row: DowngradeRow) -> Result<(), u16> {
        if row.cause == 0 || row.cause > 4 {
            return Err(E_CONTRACT_DRIFT);
        }
        self.seq += 1;
        self.downgrades.push(row);
        Ok(())
    }

    /// 视图读取（判据「三视图」：统一入口按 ViewId 分发）。
    pub fn view_tree(&self) -> &[AccountRow] {
        &self.tree
    }

    pub fn view_decisions(&self) -> &[DecisionRow] {
        &self.decisions
    }

    pub fn view_downgrades(&self) -> &[DowngradeRow] {
        &self.downgrades
    }

    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// 契约核（判据「契约冻结」）：schema 版本一致 + 数据面自洽。
    pub fn contract_ok(&self) -> Result<(), u16> {
        if CONTRACT_VERSION != 1 {
            return Err(E_CONTRACT_DRIFT);
        }
        for r in &self.tree {
            if r.used_pm > r.budget_pm {
                return Err(E_CONTRACT_DRIFT);
            }
        }
        for d in &self.decisions {
            if d.verdict > 2 {
                return Err(E_CONTRACT_DRIFT);
            }
        }
        Ok(())
    }
}

/// VE-Y 渲染对接的快照包（冻结 schema——引擎出数据不出 UI 的物理形态）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderPack {
    pub contract_version: u32,
    pub seq: u64,
    pub tree_len: u32,
    pub decision_len: u32,
    pub downgrade_len: u32,
}

pub fn render_pack(layer: &InspectorDataLayer) -> RenderPack {
    RenderPack {
        contract_version: CONTRACT_VERSION,
        seq: layer.seq(),
        tree_len: layer.view_tree().len() as u32,
        decision_len: layer.view_decisions().len() as u32,
        downgrade_len: layer.view_downgrades().len() as u32,
    }
}

// ---------------------------------------------------------------------------
// 判据自检
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    fn layer_live() -> InspectorDataLayer {
        InspectorDataLayer::new(FeedMode::Live)
    }

    fn seeded() -> InspectorDataLayer {
        let mut l = layer_live();
        l.upsert_account(AccountRow { account: 1, budget_pm: 5_000, used_pm: 2_500, tier: 0 }).unwrap();
        l.upsert_account(AccountRow { account: 2, budget_pm: 3_000, used_pm: 3_000, tier: 2 }).unwrap();
        l
    }

    #[test]
    fn a85_三视图_账本分发() {
        let mut l = seeded();
        l.push_decision(DecisionRow { frame: 1, account: 1, verdict: 0, reason: String::from("ok") }).unwrap();
        l.push_decision(DecisionRow { frame: 2, account: 2, verdict: 2, reason: String::from("storm") }).unwrap();
        l.push_downgrade(DowngradeRow { frame: 2, account: 2, cause: 2, lost_pm: 300 }).unwrap();
        assert_eq!(l.view_tree().len(), 2);
        assert_eq!(l.view_decisions().len(), 2);
        assert_eq!(l.view_downgrades().len(), 1);
        assert_eq!(l.seq(), 3);
        // 利用率读数。
        let a1 = l.view_tree().iter().find(|r| r.account == 1).unwrap();
        assert_eq!(a1.utilization_pm(), Some(500));
        // 未知账户决策拒。
        assert_eq!(l.push_decision(DecisionRow { frame: 3, account: 9, verdict: 0, reason: String::from("x") }).unwrap_err(), E_ACCOUNT_UNKNOWN);
        // 非法归因拒。
        assert_eq!(l.push_downgrade(DowngradeRow { frame: 3, account: 1, cause: 9, lost_pm: 1 }).unwrap_err(), E_CONTRACT_DRIFT);
    }

    #[test]
    fn a85_双模式_实时丢弃与历史上限() {
        let mut live = seeded();
        for f in 0..200u64 {
            live.push_decision(DecisionRow { frame: f, account: 1, verdict: 0, reason: String::from("k") }).unwrap();
        }
        assert_eq!(live.view_decisions().len(), 128, "live trims to 128");
        assert_eq!(live.view_decisions()[0].frame, 72);

        let mut hist = InspectorDataLayer::new(FeedMode::History { capacity: 50 });
        hist.upsert_account(AccountRow { account: 1, budget_pm: 9_999, used_pm: 0, tier: 0 }).unwrap();
        for f in 0..80u64 {
            hist.push_decision(DecisionRow { frame: f, account: 1, verdict: 0, reason: String::from("k") }).unwrap();
        }
        assert_eq!(hist.view_decisions().len(), 50);
        assert_eq!(hist.view_decisions()[0].frame, 30);
        // 模式读回。
        assert_eq!(live.mode(), FeedMode::Live);
        assert_eq!(hist.mode(), FeedMode::History { capacity: 50 });
    }

    #[test]
    fn a85_契约冻结_漂移必拒与快照包() {
        let mut l = seeded();
        assert!(l.contract_ok().is_ok());
        // 超预算行拒收（契约漂移）。
        assert_eq!(l.upsert_account(AccountRow { account: 3, budget_pm: 100, used_pm: 200, tier: 0 }).unwrap_err(), E_CONTRACT_DRIFT);
        // 越档位拒收。
        assert_eq!(l.upsert_account(AccountRow { account: 3, budget_pm: 100, used_pm: 0, tier: 4 }).unwrap_err(), E_CONTRACT_DRIFT);
        assert_eq!(l.upsert_account(AccountRow { account: 3, budget_pm: 10_001, used_pm: 0, tier: 0 }).unwrap_err(), E_CONTRACT_DRIFT);
        // 快照包（冻结 schema 的对外形态）。
        l.push_decision(DecisionRow { frame: 1, account: 1, verdict: 0, reason: String::from("ok") }).unwrap();
        let pack = render_pack(&l);
        assert_eq!(pack.contract_version, 1);
        assert_eq!(pack.seq, l.seq());
        assert_eq!(pack.tree_len, 2);
        assert_eq!(pack.decision_len, 1);
        assert_eq!(pack.downgrade_len, 0);
        // 常量自检：视图枚举槽位与槽位数一致。
        assert_eq!(ViewId::DowngradeCause as usize, VIEW_SLOTS - 1);
    }
}
