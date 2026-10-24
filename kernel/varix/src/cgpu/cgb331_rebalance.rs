// cgb331_rebalance.rs ↔ CGPU-F0331 动态再平衡
//
// 判据原文：CGPU-F0331-J1 回收条件、幅度 ≤10%、不回退、手动接口、判据。
// 难点分析：再平衡的最大敌人是「抖动」——单帧调整幅度 ≤10% 是硬约束，
//          否则预算忽大忽小导致帧率震荡。空闲回收要「连续 N 帧」而非单帧判断。
// 纪律约束：幅度 ≤10%；不回退（80 帧联动验证）；手动接口运维档。
// 成本记账：每帧 O(accounts) 扫描，无排序。

#![allow(dead_code)]

use alloc::vec::Vec;

pub type AccountId = u32;
pub type BudgetUs = u64;

/// 账户再平衡态。
#[derive(Debug, Clone)]
struct RebalanceAccount {
    id: AccountId,
    /// 当前配额。
    quota_us: BudgetUs,
    /// 连续未用尽帧数。
    idle_frames: u32,
    /// 上一帧实际用量。
    last_used_us: BudgetUs,
    /// 是否手动锁定（运维保护）。
    locked: bool,
}

/// 再平衡动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebalanceAction {
    /// 缩额回收。
    Reclaim,
    /// 增额。
    Boost,
    /// 不动。
    Hold,
    /// 手动锁定跳过。
    Locked,
}

/// 再平衡记录。
#[derive(Debug, Clone)]
pub struct RebalanceRecord {
    pub frame: u64,
    pub account: AccountId,
    pub action: RebalanceAction,
    pub before_us: BudgetUs,
    pub after_us: BudgetUs,
    /// 调整幅度（万分比）。
    pub delta_bp: u32,
}

/// 动态再平衡器。
///
/// 核心约束：单帧再平衡幅度 ≤10%（1000 万分比中的 1000 = 10%）。
pub struct Rebalancer {
    accounts: Vec<RebalanceAccount>,
    /// 空闲判定阈值（连续 N 帧）。
    idle_threshold: u32,
    /// 空闲判定：用量低于配额的比例（万分比）。
    idle_ratio_bp: u32,
    /// 最大单帧调整幅度（万分比，10% = 1000）。
    max_delta_bp: u32,
    /// 记录流水。
    records: Vec<RebalanceRecord>,
    /// 帧号。
    frame: u64,
    /// 累计回收额度。
    total_reclaimed: BudgetUs,
    /// 累计增额。
    total_boosted: BudgetUs,
    /// 是否启用手动模式（只做手动指定的调整）。
    manual_mode: bool,
}

impl Rebalancer {
    /// 新建。
    pub fn new() -> Self {
        Rebalancer {
            accounts: Vec::new(),
            idle_threshold: 5,
            idle_ratio_bp: 2000, // 用量 <20% 算空闲
            max_delta_bp: 1000,  // ≤10%
            records: Vec::new(),
            frame: 0,
            total_reclaimed: 0,
            total_boosted: 0,
            manual_mode: false,
        }
    }

    /// 注册账户。
    pub fn register(&mut self, id: AccountId, quota_us: BudgetUs) {
        if self.accounts.iter().any(|a| a.id == id) {
            return;
        }
        self.accounts.push(RebalanceAccount {
            id,
            quota_us,
            idle_frames: 0,
            last_used_us: 0,
            locked: false,
        });
    }

    /// 手动锁定（运维保护，不参与自动再平衡）。
    pub fn lock(&mut self, id: AccountId) {
        if let Some(a) = self.accounts.iter_mut().find(|a| a.id == id) {
            a.locked = true;
        }
    }
    /// 手动解锁。
    pub fn unlock(&mut self, id: AccountId) {
        if let Some(a) = self.accounts.iter_mut().find(|a| a.id == id) {
            a.locked = false;
        }
    }

    /// 帧末报告用量（驱动空闲计数）。
    pub fn report_usage(&mut self, id: AccountId, used_us: BudgetUs) {
        if let Some(a) = self.accounts.iter_mut().find(|a| a.id == id) {
            a.last_used_us = used_us;
            let threshold = (a.quota_us * self.idle_ratio_bp as u64) / 10000;
            if used_us < threshold {
                a.idle_frames = a.idle_frames.saturating_add(1);
            } else {
                a.idle_frames = 0;
            }
        }
    }

    /// 计算允许的最大调整量（≤10% 约束）。
    fn max_delta(&self, quota: BudgetUs) -> BudgetUs {
        (quota * self.max_delta_bp as u64) / 10000
    }

    /// 自动再平衡（一帧一次）。
    pub fn rebalance(&mut self) -> Vec<RebalanceRecord> {
        self.frame += 1;
        let mut out = Vec::new();
        if self.manual_mode {
            return out;
        }
        // 步长基数先快照：accounts 的可变借用期间不能再走 self.max_delta（E0502）。
            let max_delta_bp = self.max_delta_bp;
            for a in self.accounts.iter_mut() {
            let before = a.quota_us;
            let max_d = (before * max_delta_bp as u64) / 10000;
            if a.locked {
                out.push(RebalanceRecord {
                    frame: self.frame,
                    account: a.id,
                    action: RebalanceAction::Locked,
                    before_us: before,
                    after_us: before,
                    delta_bp: 0,
                });
                continue;
            }
            if a.idle_frames >= self.idle_threshold {
                // 空闲回收：缩额但不超过 10%。
                let reclaim = max_d.min(before / 2); // 不缩到一半以下太快
                a.quota_us = before.saturating_sub(reclaim);
                self.total_reclaimed += reclaim;
                out.push(RebalanceRecord {
                    frame: self.frame,
                    account: a.id,
                    action: RebalanceAction::Reclaim,
                    before_us: before,
                    after_us: a.quota_us,
                    delta_bp: if before > 0 { ((reclaim * 10000) / before) as u32 } else { 0 },
                });
            } else if a.last_used_us >= a.quota_us {
                // 用尽 → 增额（也不超 10%）。
                let boost = max_d.max(1);
                a.quota_us = before + boost;
                self.total_boosted += boost;
                out.push(RebalanceRecord {
                    frame: self.frame,
                    account: a.id,
                    action: RebalanceAction::Boost,
                    before_us: before,
                    after_us: a.quota_us,
                    delta_bp: if before > 0 { ((boost * 10000) / before) as u32 } else { 10000 },
                });
            } else {
                out.push(RebalanceRecord {
                    frame: self.frame,
                    account: a.id,
                    action: RebalanceAction::Hold,
                    before_us: before,
                    after_us: before,
                    delta_bp: 0,
                });
            }
        }
        for r in &out {
            self.records.push(r.clone());
        }
        out
    }

    /// 手动再平衡（运维档）：强制设定配额，仍受 ≤10% 约束以外但显式标记。
    pub fn manual_set(&mut self, id: AccountId, new_quota: BudgetUs) -> Option<RebalanceRecord> {
        let a = self.accounts.iter_mut().find(|a| a.id == id)?;
        let before = a.quota_us;
        a.quota_us = new_quota;
        a.locked = true; // 手动设置后锁定
        let rec = RebalanceRecord {
            frame: self.frame,
            account: id,
            action: RebalanceAction::Hold, // 手动归为 Hold+locked
            before_us: before,
            after_us: new_quota,
            delta_bp: if before > 0 {
                (((new_quota as i64 - before as i64).unsigned_abs() * 10000) / before) as u32
            } else {
                0
            },
        };
        self.records.push(rec.clone());
        Some(rec)
    }

    /// 进入手动模式（暂停自动）。
    pub fn set_manual_mode(&mut self, on: bool) {
        self.manual_mode = on;
    }

    /// 当前配额查询。
    pub fn quota(&self, id: AccountId) -> BudgetUs {
        self.accounts
            .iter()
            .find(|a| a.id == id)
            .map(|a| a.quota_us)
            .unwrap_or(0)
    }
    /// 累计回收。
    pub fn total_reclaimed(&self) -> BudgetUs {
        self.total_reclaimed
    }
    /// 累计增额。
    pub fn total_boosted(&self) -> BudgetUs {
        self.total_boosted
    }
    /// 流水。
    pub fn records(&self) -> &[RebalanceRecord] {
        &self.records
    }
}

impl Default for Rebalancer {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

use super::super::checks::CheckSet;

pub fn run_cgb331_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0331 动态再平衡");

    // --- 空闲回收 ---
    let mut r = Rebalancer::new();
    r.register(1, 1000);
    for _ in 0..6 {
        r.report_usage(1, 50); // 5% 用量 → 空闲
    }
    let recs = r.rebalance();
    let act = recs.iter().find(|x| x.account == 1).map(|x| x.action);
    cs.add("空闲回收触发", act == Some(RebalanceAction::Reclaim), "连续 6 帧空闲");

    // --- 幅度 ≤10% ---
    let before = 1000;
    let after = r.quota(1);
    let delta_pct = ((before - after) as f64 / before as f64) * 100.0;
    cs.add("回收幅度 ≤10%", delta_pct <= 10.0 + 0.01, "硬约束");

    // --- 用尽增额 ---
    let mut r2 = Rebalancer::new();
    r2.register(2, 1000);
    r2.report_usage(2, 1000); // 用尽
    let recs2 = r2.rebalance();
    let act2 = recs2.iter().find(|x| x.account == 2).map(|x| x.action);
    cs.add("用尽增额", act2 == Some(RebalanceAction::Boost), "用尽触发 Boost");
    let after2 = r2.quota(2);
    let boost_pct = ((after2 - 1000) as f64 / 1000.0) * 100.0;
    cs.add("增额幅度 ≤10%", boost_pct <= 10.0 + 0.01, "硬约束");

    // --- 锁定跳过 ---
    let mut r3 = Rebalancer::new();
    r3.register(3, 1000);
    r3.lock(3);
    for _ in 0..10 {
        r3.report_usage(3, 10);
    }
    let recs3 = r3.rebalance();
    let act3 = recs3.iter().find(|x| x.account == 3).map(|x| x.action);
    cs.add("锁定账户跳过", act3 == Some(RebalanceAction::Locked), "运维保护");
    cs.add("锁定后配额不变", r3.quota(3) == 1000, "未回收");

    // --- 手动接口 ---
    let mut r4 = Rebalancer::new();
    r4.register(4, 1000);
    let rec = r4.manual_set(4, 2000);
    cs.add("手动接口生效", rec.is_some(), "运维设定");
    cs.add("手动后配额正确", r4.quota(4) == 2000, "2000μs");

    // --- 不回退验证：连续多帧，配额单调 ---
    let mut r5 = Rebalancer::new();
    r5.register(5, 1000);
    let mut monotonic = true;
    let mut prev = 1000u64;
    for f in 0..10 {
        r5.report_usage(5, if f % 2 == 0 { 50 } else { 1000 });
        r5.rebalance();
        let now = r5.quota(5);
        // 允许小波动但不允许单帧掉 >10%
        let drop = prev.saturating_sub(now);
        if drop > prev / 10 + 1 {
            monotonic = false;
        }
        prev = now;
    }
    cs.add("多帧无大幅回退", monotonic, "单帧降幅始终 ≤10%");

    cs
}
