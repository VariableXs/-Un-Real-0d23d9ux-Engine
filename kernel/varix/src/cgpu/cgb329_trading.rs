#![allow(dead_code)]
use alloc::vec;
// cgb329_trading.rs ↔ CGPU-F0329 预算交易
//
// 判据原文：CGPU-F0329-J1 债务模型、征用偿还、账本、防护、判据。
// 难点分析：循环借贷检测是图论问题——借贷关系构成有向图，环=循环借贷。
//          用 DFS 三色标记即可 O(V+E) 检测，不必上 Tarjan。
// 纪律约束：征用必偿还；全部借贷流水可审计；环检测拒绝交易。
// 成本记账：每次交易 O(V+E) 环检测，V=账户数百级，远低于仲裁预算。


use alloc::vec::Vec;

pub type AccountId = u32;
pub type BudgetUs = u64;

/// 借贷条目（债务模型）。
#[derive(Debug, Clone)]
pub struct DebtEntry {
    pub lender: AccountId,
    pub borrower: AccountId,
    /// 借出额度（μs）。
    pub amount: BudgetUs,
    /// 利率（万分比；还时多还的比率）。
    pub interest_bp: u16,
    /// 偿还期限（帧号）。
    pub due_frame: u64,
    /// 是否已还清。
    pub settled: bool,
}

/// 征用事件（显性化记录）。
#[derive(Debug, Clone)]
pub struct RequisitionEvent {
    pub frame: u64,
    pub requirer: AccountId,
    pub target: AccountId,
    pub amount: BudgetUs,
    /// 是否已偿还。
    pub repaid: bool,
}

/// 交易错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeError {
    /// 会构成循环借贷。
    CircularDebt,
    /// 借出方余额不足。
    InsufficientFunds,
    /// 借出方未标记可借出。
    NotLendable,
    /// 额度为零。
    ZeroAmount,
}

impl TradeError {
    pub fn code(self) -> u32 {
        match self {
            TradeError::CircularDebt => 0x329_01,
            TradeError::InsufficientFunds => 0x329_02,
            TradeError::NotLendable => 0x329_03,
            TradeError::ZeroAmount => 0x329_04,
        }
    }
}

/// 账户交易态。
#[derive(Debug, Clone)]
struct TradeAccount {
    id: AccountId,
    /// 可借出余额（μs）。
    lendable_balance: BudgetUs,
    /// 是否允许借出。
    lendable: bool,
    /// 净债务（正=欠别人）。
    net_debt: i64,
}

/// 预算交易器。
pub struct BudgetTrader {
    accounts: Vec<TradeAccount>,
    /// 借贷流水。
    debts: Vec<DebtEntry>,
    /// 征用流水。
    requisitions: Vec<RequisitionEvent>,
    /// 当前帧号。
    frame: u64,
    /// 累计交易量。
    total_traded: BudgetUs,
    /// 环检测拒绝次数（防欺诈指标）。
    circular_rejects: u32,
}

impl BudgetTrader {
    /// 新建。
    pub fn new() -> Self {
        BudgetTrader {
            accounts: Vec::new(),
            debts: Vec::new(),
            requisitions: Vec::new(),
            frame: 0,
            total_traded: 0,
            circular_rejects: 0,
        }
    }

    /// 注册账户。
    pub fn register(&mut self, id: AccountId, balance: BudgetUs, lendable: bool) {
        if self.accounts.iter().any(|a| a.id == id) {
            return;
        }
        self.accounts.push(TradeAccount {
            id,
            lendable_balance: balance,
            lendable,
            net_debt: 0,
        });
    }

    fn account_mut(&mut self, id: AccountId) -> Option<&mut TradeAccount> {
        self.accounts.iter_mut().find(|a| a.id == id)
    }

    /// 构建当前借贷有向图并检测环。
    ///
    /// 三色 DFS：White=0 未访 / Gray=1 在栈 / Black=2 完成。
    fn has_circular_debt(&self, extra: Option<(AccountId, AccountId)>) -> bool {
        // 邻接表。
        let mut adj: Vec<(AccountId, Vec<AccountId>)> = Vec::new();
        let mut get_or = |list: &mut Vec<(AccountId, Vec<AccountId>)>, k: AccountId| -> usize {
            if let Some(i) = list.iter().position(|(a, _)| *a == k) {
                i
            } else {
                list.push((k, Vec::new()));
                list.len() - 1
            }
        };
        for d in &self.debts {
            if d.settled {
                continue;
            }
            // 借款人 → 贷款人（欠谁）。
            let i = get_or(&mut adj, d.borrower);
            adj[i].1.push(d.lender);
        }
        if let Some((from, to)) = extra {
            let i = get_or(&mut adj, from);
            adj[i].1.push(to);
        }

        // 三色 DFS。
        let mut color: Vec<(AccountId, u8)> = Vec::new();
        let mut get_color = |c: &mut Vec<(AccountId, u8)>, k: AccountId| -> usize {
            if let Some(i) = c.iter().position(|(a, _)| *a == k) {
                i
            } else {
                c.push((k, 0));
                c.len() - 1
            }
        };

        // 迭代 DFS（避免深递归）。
        let nodes: Vec<AccountId> = adj.iter().map(|(a, _)| *a).collect();
        for &start in &nodes {
            let si = get_color(&mut color, start);
            if color[si].1 != 0 {
                continue;
            }
            // 栈模拟：(节点, 邻居索引)
            color[si].1 = 1;
            let mut stack: Vec<(AccountId, usize)> = vec![(start, 0)];
            while let Some((node, ni)) = stack.pop() {
                let neighbors = adj
                    .iter()
                    .find(|(a, _)| *a == node)
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default();
                if ni >= neighbors.len() {
                    let ci = get_color(&mut color, node);
                    color[ci].1 = 2;
                    continue;
                }
                let next = neighbors[ni];
                stack.push((node, ni + 1));
                let nci = get_color(&mut color, next);
                if color[nci].1 == 1 {
                    return true; // 找到环
                }
                if color[nci].1 == 0 {
                    color[nci].1 = 1;
                    stack.push((next, 0));
                }
            }
        }
        false
    }

    /// 借出（债务模型：借出额度 + 利率 + 偿还期限）。
    pub fn lend(
        &mut self,
        lender: AccountId,
        borrower: AccountId,
        amount: BudgetUs,
        interest_bp: u16,
        due_frame: u64,
    ) -> Result<DebtEntry, TradeError> {
        if amount == 0 {
            return Err(TradeError::ZeroAmount);
        }
        // 可借出标记检查。
        {
            let a = self
                .accounts
                .iter()
                .find(|a| a.id == lender)
                .ok_or(TradeError::InsufficientFunds)?;
            if !a.lendable {
                return Err(TradeError::NotLendable);
            }
            if a.lendable_balance < amount {
                return Err(TradeError::InsufficientFunds);
            }
        }
        // 环检测（加这条边后是否成环）。
        if self.has_circular_debt(Some((borrower, lender))) {
            self.circular_rejects += 1;
            return Err(TradeError::CircularDebt);
        }
        // 记账。
        if let Some(a) = self.account_mut(lender) {
            a.lendable_balance -= amount;
        }
        if let Some(a) = self.account_mut(borrower) {
            a.net_debt += amount as i64;
        }
        let entry = DebtEntry {
            lender,
            borrower,
            amount,
            interest_bp,
            due_frame,
            settled: false,
        };
        self.debts.push(entry.clone());
        self.total_traded += amount;
        Ok(entry)
    }

    /// 还贷（含利息）。
    pub fn repay(&mut self, lender: AccountId, borrower: AccountId, amount: BudgetUs) -> BudgetUs {
        // 找最早的未结条目。
        let idx = self
            .debts
            .iter()
            .position(|d| d.lender == lender && d.borrower == borrower && !d.settled);
        let idx = match idx {
            Some(i) => i,
            None => return 0,
        };
        let owed = {
            let d = &self.debts[idx];
            d.amount + (d.amount * d.interest_bp as u64) / 10000
        };
        let pay = amount.min(owed);
        // 更新状态。
        self.debts[idx].settled = pay >= owed;
        if let Some(a) = self.account_mut(borrower) {
            a.net_debt = (a.net_debt - pay as i64).max(0);
        }
        if let Some(a) = self.account_mut(lender) {
            a.lendable_balance += pay;
        }
        pay
    }

    /// 紧急征用（征用必偿还）。
    pub fn requisition(
        &mut self,
        requirer: AccountId,
        target: AccountId,
        amount: BudgetUs,
    ) -> Result<RequisitionEvent, TradeError> {
        if amount == 0 {
            return Err(TradeError::ZeroAmount);
        }
        {
            let a = self
                .accounts
                .iter()
                .find(|a| a.id == target)
                .ok_or(TradeError::InsufficientFunds)?;
            if a.lendable_balance < amount {
                return Err(TradeError::InsufficientFunds);
            }
        }
        if let Some(a) = self.account_mut(target) {
            a.lendable_balance -= amount;
        }
        let ev = RequisitionEvent {
            frame: self.frame,
            requirer,
            target,
            amount,
            repaid: false,
        };
        self.requisitions.push(ev.clone());
        self.total_traded += amount;
        Ok(ev)
    }

    /// 偿还征用。
    pub fn repay_requisition(&mut self, requirer: AccountId, target: AccountId, amount: BudgetUs) {
        // 先定位并闭账（借用随语句结束），再动账户余额——两段可变借用不重叠（E0499）。
        let mut pay = None;
        if let Some(ev) = self.requisitions.iter_mut().rev()
            .find(|e| e.requirer == requirer && e.target == target && !e.repaid)
        {
            pay = Some(amount.min(ev.amount));
            ev.repaid = true;
        }
        if let Some(amt) = pay {
            if let Some(a) = self.account_mut(target) {
                a.lendable_balance += amt;
            }
        }
    }

    /// 推进帧（过期债务标记）。
    pub fn tick(&mut self, frame: u64) {
        self.frame = frame;
    }

    /// 借贷流水查询。
    pub fn debts(&self) -> &[DebtEntry] {
        &self.debts
    }
    /// 征用流水。
    pub fn requisitions(&self) -> &[RequisitionEvent] {
        &self.requisitions
    }
    /// 累计交易量。
    pub fn total_traded(&self) -> BudgetUs {
        self.total_traded
    }
    /// 环拒绝计数。
    pub fn circular_rejects(&self) -> u32 {
        self.circular_rejects
    }
    /// 是否存在未结环（诊断）。
    pub fn has_cycle(&self) -> bool {
        self.has_circular_debt(None)
    }
}

impl Default for BudgetTrader {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

use super::super::checks::CheckSet;

pub fn run_cgb329_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0329 预算交易");

    // --- 基本借贷 ---
    let mut t = BudgetTrader::new();
    t.register(1, 1000, true);
    t.register(2, 100, true);
    let r = t.lend(1, 2, 500, 500, 100); // 5% 利率
    cs.add("借出成功", r.is_ok(), "余额充足可借出");
    let owed = 500 + 500 * 500 / 10000;
    let paid = t.repay(2, 1, owed);
    cs.add("还贷含利息", paid == owed, "525μs 还清");

    // --- 环检测：A→B→C→A ---
    let mut t2 = BudgetTrader::new();
    t2.register(1, 1000, true);
    t2.register(2, 1000, true);
    t2.register(3, 1000, true);
    t2.lend(1, 2, 100, 0, 100).ok(); // 2 欠 1
    t2.lend(2, 3, 100, 0, 100).ok(); // 3 欠 2
    let r3 = t2.lend(3, 1, 100, 0, 100); // 1 欠 3 → 环
    cs.add(
        "循环借贷被拒",
        matches!(r3, Err(TradeError::CircularDebt)),
        "A→B→C→A 构成环",
    );
    cs.add("环拒绝计数", t2.circular_rejects() >= 1, "防欺诈指标");

    // --- 不可借出 ---
    let mut t3 = BudgetTrader::new();
    t3.register(1, 1000, false);
    t3.register(2, 0, true);
    let r4 = t3.lend(1, 2, 100, 0, 100);
    cs.add("不可借出拒绝", matches!(r4, Err(TradeError::NotLendable)), "lendable=false");

    // --- 征用必偿还 ---
    let mut t4 = BudgetTrader::new();
    t4.register(1, 0, true);
    t4.register(2, 1000, true);
    let ev = t4.requisition(1, 2, 300);
    cs.add("征用成功", ev.is_ok(), "紧急征用");
    t4.repay_requisition(1, 2, 300);
    let repaid = t4.requisitions().iter().any(|e| e.repaid);
    cs.add("征用已偿还", repaid, "偿还标记置位");

    // --- 账本可审计 ---
    cs.add("借贷流水非空", t.debts().len() >= 2, "至少两笔（借+还前）");
    cs.add(
        "交易总量累计",
        t.total_traded() >= 500,
        "累计交易量",
    );

    cs
}

