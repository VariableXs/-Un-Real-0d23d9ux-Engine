//! CGPU-F1123 · 预算池分级（CGPU-H 域 · VH03 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1123`
//!
//! **判据（锚点原文）**：三级、账户树、守恒、水位、判据。
//!
//! # 一、三级池语义与配额（分级表）
//!
//! F1121 已建三级池骨架（[`crate::svstar2::vch01_budgetpool::PoolKind`]），
//! 本域补**分级策略表**（[`tier_policy`]）：每级给出显存上限百分比与
//! 借出语义——进程池（各引擎会话，上限 70%，可借出）、系统池
//! （系统 UI/合成器，上限 20%，不可借出）、保留池（应急+驱动保留，
//! 上限 10%，不可借出）。Σ上限 = [`TIER_POLICY_SUM_PCT`] = 100，
//! 池构造期逐级核对配额不越策略上限。
//!
//! # 二、池配额分配：进程内再分账户（账户树）
//!
//! 复用 C 域账户树理念：进程（及各级）池内在分账户——树形切分，
//! 每节点配额 **Σ子配额 ≤ 父配额**（[`AccountTree::open_account`] 构造期
//! 把关）；扣费沿祖先链逐层校验后逐层提交（[`AccountTree::charge`]），
//! 任一层超配额即拒绝且**无部分提交**。深度 [`MAX_TREE_DEPTH`]、
//! 容量 [`MAX_ACCOUNTS`] 封顶。
//!
//! # 三、池校验：双层守恒
//!
//! 池层守恒复用 F1121（`PoolSet::audit`：Σ池.used ≤ 总显存 + 账实
//! 相符）；树层守恒为本域新增（[`AccountTree::conservation`]：每节点
//! Σ子配额 ≤ 父配额且 Σ子占用 ≤ 父占用，checked 防回绕）。
//! [`TieredPools::audit`] 双层齐查。
//!
//! # 四、池水位：三级各自监控 + 告警联动
//!
//! 复用 F1121 水位四档（Empty/Normal/High/Critical），三级池**各自**
//! 判定互不牵连（[`TieredPools::poll_alerts`]）；状态**边缘触发**出
//! [`AlertEvent`]（升 High / 升 Critical / 降回常规各一事件，稳定期
//! 不重复发——防告警风暴），事件环形容量 [`ALERT_CAP`]，seq 单调。
//!
//! # 要点六：诊断码独占 0x50xx 段（H03 续编）
//!
//! vch01 占 0x5001-0x5006、vch02 占 0x5007-0x500A，本域续编
//! 0x500B-0x500F，与已占用段不重叠（码段判据用 `!=` 防自判死）。

use crate::svstar2::vch01_budgetpool::{BudgetPool, PoolKind, PoolSet, WaterLevel};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（H03 续编 0x500B-0x500F）
// ---------------------------------------------------------------------------

/// 三级策略越限（配额超策略上限 / 扣费超配额）。
pub const ERR_TIER_POLICY: u16 = 0x500B;
/// 账户树形态错（父不存在 / 超深度 / 超容量 / 子不存在）。
pub const ERR_TREE_SHAPE: u16 = 0x500C;
/// 账户树守恒破坏（Σ子 > 父 / 释放超占用）。
pub const ERR_TREE_CONSERVATION: u16 = 0x500D;
/// 告警联动状态不齐（追踪表满 / 句柄对不上树）。
pub const ERR_ALERT_STATE: u16 = 0x500E;
/// 追踪表满（handle_map 容量 [`MAX_TRACKED`] 用尽）。
pub const ERR_TRACK_FULL: u16 = 0x500F;

// ---------------------------------------------------------------------------
// 二、三级策略表（分级语义与配额上限）
// ---------------------------------------------------------------------------

/// 三级池分级策略行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TierPolicy {
    /// 显存上限百分比（配额不得越过）。
    pub max_pct_of_vram: u64,
    /// 借出语义：进程池可借出（高压联动放行），系统/保留池不可。
    pub borrowable: bool,
    /// 三级语义标签（锚点原文措辞）。
    pub label: &'static str,
}

/// Σ三级上限百分比（守恒自检锚）。
pub const TIER_POLICY_SUM_PCT: u64 = 100;

/// 按池级取策略（固定表，编译期语义）。
pub fn tier_policy(kind: PoolKind) -> TierPolicy {
    match kind {
        PoolKind::Process => TierPolicy {
            max_pct_of_vram: 70,
            borrowable: true,
            label: "进程池（各引擎会话）",
        },
        PoolKind::System => TierPolicy {
            max_pct_of_vram: 20,
            borrowable: false,
            label: "系统池（系统 UI/合成器）",
        },
        PoolKind::Reserved => TierPolicy {
            max_pct_of_vram: 10,
            borrowable: false,
            label: "保留池（应急+驱动保留）",
        },
    }
}

/// Σ策略上限 = 100（判据独立重算对拍）。
pub fn tier_policy_sum_pct() -> u64 {
    tier_policy(PoolKind::Process).max_pct_of_vram
        .saturating_add(tier_policy(PoolKind::System).max_pct_of_vram)
        .saturating_add(tier_policy(PoolKind::Reserved).max_pct_of_vram)
}

// ---------------------------------------------------------------------------
// 三、账户树（进程内再分账户，复用 C 域账户树理念）
// ---------------------------------------------------------------------------

/// 账户节点 id（0 = 根）。
pub type AccountId = usize;
/// 根账户 id。
pub const ACCOUNT_ROOT: AccountId = 0;
/// 树深上限（根为 0 层，最深 [`MAX_TREE_DEPTH`] - 1 层）。
pub const MAX_TREE_DEPTH: usize = 4;
/// 账户容量上限。
pub const MAX_ACCOUNTS: usize = 64;

/// 账户节点：配额 + 占用 + 子账户表。
#[derive(Debug, Clone)]
pub struct AccountNode {
    /// 账户标签（人读）。
    pub label: String,
    /// 父账户（根为 [`Option::None`]）。
    pub parent: Option<AccountId>,
    /// 层深（根 0）。
    pub depth: usize,
    /// 配额（字节）。
    pub quota: u64,
    /// 占用（字节，charge/release 逐层同步）。
    pub used: u64,
    /// 子账户 id 表。
    pub children: Vec<AccountId>,
}

/// 账户树：根配额 = 池配额，逐级切分。
#[derive(Debug, Clone)]
pub struct AccountTree {
    nodes: Vec<AccountNode>,
    total_quota: u64,
}

impl AccountTree {
    /// 构造：单根，根配额 = 池配额。
    pub fn new(total_quota: u64) -> AccountTree {
        AccountTree {
            nodes: vec![AccountNode {
                label: String::from("root"),
                parent: None,
                depth: 0,
                quota: total_quota,
                used: 0,
                children: Vec::new(),
            }],
            total_quota: total_quota,
        }
    }

    /// 根配额（= 池配额）。
    pub fn total_quota(&self) -> u64 {
        self.total_quota
    }

    /// 账户总数（含根）。
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// 节点只读访问。
    pub fn node(&self, id: AccountId) -> Option<&AccountNode> {
        self.nodes.get(id)
    }

    /// id → 根的祖先链（含自身；id 非法返回空表）。
    fn chain_ids(&self, id: AccountId) -> Vec<AccountId> {
        let mut chain = Vec::new();
        let mut cur = id;
        loop {
            match self.nodes.get(cur) {
                None => return Vec::new(), // 孤儿/越界：整链作废
                Some(n) => {
                    chain.push(cur);
                    match n.parent {
                        None => return chain, // 到根
                        Some(p) => cur = p,
                    }
                }
            }
        }
    }

    /// 开子账户：父存在、不越深、不越容、Σ兄弟新配额 ≤ 父配额。
    pub fn open_account(
        &mut self,
        parent: AccountId,
        quota: u64,
        label: &str,
    ) -> Result<AccountId, u16> {
        let (p_depth, p_quota) = match self.nodes.get(parent) {
            None => return Err(ERR_TREE_SHAPE),
            Some(n) => (n.depth, n.quota),
        };
        if p_depth + 1 >= MAX_TREE_DEPTH {
            return Err(ERR_TREE_SHAPE); // 子层深 = p_depth+1 ≤ MAX-1
        }
        if self.nodes.len() >= MAX_ACCOUNTS {
            return Err(ERR_TREE_SHAPE);
        }
        // Σ(兄弟配额) + 新配额 ≤ 父配额（checked）
        let mut sum = quota;
        for i in 0..self.nodes.len() {
            if self.nodes[i].parent == Some(parent) {
                sum = match sum.checked_add(self.nodes[i].quota) {
                    Some(s) => s,
                    None => return Err(ERR_TREE_CONSERVATION),
                };
            }
        }
        if sum > p_quota {
            return Err(ERR_TIER_POLICY);
        }
        let id = self.nodes.len();
        self.nodes.push(AccountNode {
            label: String::from(label),
            parent: Some(parent),
            depth: p_depth + 1,
            quota: quota,
            used: 0,
            children: Vec::new(),
        });
        self.nodes[parent].children.push(id);
        Ok(id)
    }

    /// 扣费：沿祖先链逐层校验（used+bytes ≤ quota），全过才逐层提交
    /// ——任一层超配额即整体拒绝，**无部分提交**。
    pub fn charge(&mut self, id: AccountId, bytes: u64) -> Result<(), u16> {
        let chain = self.chain_ids(id);
        if chain.is_empty() {
            return Err(ERR_TREE_SHAPE);
        }
        // 校验段（只读快照）
        for &nid in chain.iter() {
            let n = &self.nodes[nid];
            let next = match n.used.checked_add(bytes) {
                Some(v) => v,
                None => return Err(ERR_TIER_POLICY),
            };
            if next > n.quota {
                return Err(ERR_TIER_POLICY);
            }
        }
        // 提交段（逐层累加）
        for &nid in chain.iter() {
            self.nodes[nid].used += bytes; // 已 checked，必不回绕
        }
        Ok(())
    }

    /// 归还：沿祖先链逐层扣减；超占用即守恒破坏立案。
    pub fn release(&mut self, id: AccountId, bytes: u64) -> Result<(), u16> {
        let chain = self.chain_ids(id);
        if chain.is_empty() {
            return Err(ERR_TREE_SHAPE);
        }
        for &nid in chain.iter() {
            if self.nodes[nid].used < bytes {
                return Err(ERR_TREE_CONSERVATION);
            }
        }
        for &nid in chain.iter() {
            self.nodes[nid].used -= bytes;
        }
        Ok(())
    }

    /// 树层守恒：根配额对账 + 每节点 Σ子配额 ≤ 父配额且 Σ子占用 ≤ 父占用
    /// （checked 防回绕；子 id 必须有效）。
    pub fn conservation(&self) -> Result<(), u16> {
        match self.nodes.get(ACCOUNT_ROOT) {
            None => return Err(ERR_TREE_SHAPE),
            Some(root) => {
                if root.quota != self.total_quota {
                    return Err(ERR_TREE_CONSERVATION);
                }
            }
        }
        for i in 0..self.nodes.len() {
            let mut sum_q = 0u64;
            let mut sum_u = 0u64;
            for &c in self.nodes[i].children.iter() {
                let cn = match self.nodes.get(c) {
                    None => return Err(ERR_TREE_SHAPE),
                    Some(n) => n,
                };
                sum_q = match sum_q.checked_add(cn.quota) {
                    Some(v) => v,
                    None => return Err(ERR_TREE_CONSERVATION),
                };
                sum_u = match sum_u.checked_add(cn.used) {
                    Some(v) => v,
                    None => return Err(ERR_TREE_CONSERVATION),
                };
            }
            if sum_q > self.nodes[i].quota || sum_u > self.nodes[i].used {
                return Err(ERR_TREE_CONSERVATION);
            }
        }
        Ok(())
    }

    /// [判据设施] 注入守恒坏态（配额/占用定向膨胀），仅供判据验证
    /// 守恒闸能抓红；功能路径永不调用（saturating 防回绕）。
    #[doc(hidden)]
    pub fn inject_corruption_for_probe(&mut self, id: AccountId, quota_add: u64, used_add: u64) {
        if let Some(n) = self.nodes.get_mut(id) {
            n.quota = n.quota.saturating_add(quota_add);
            n.used = n.used.saturating_add(used_add);
        }
    }
}

// ---------------------------------------------------------------------------
// 四、水位告警联动（三级各自监控 + 边缘触发事件环）
// ---------------------------------------------------------------------------

/// 告警事件环形容量。
pub const ALERT_CAP: usize = 128;
/// 句柄追踪表容量（handle → 池/账户映射）。
pub const MAX_TRACKED: usize = 512;

/// 告警类别（边缘触发三式）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertKind {
    /// 升入高水位（≥ 80%）。
    PoolHigh,
    /// 升入危急（≥ 95%，含 High→Critical 跳档）。
    PoolCritical,
    /// 降回常规/空闲。
    PoolRecovered,
}

/// 告警事件：seq 单调，池 + 类别 + 占用千分比快照。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlertEvent {
    /// 全序序号。
    pub seq: u64,
    /// 哪一级池。
    pub pool: PoolKind,
    /// 类别。
    pub kind: AlertKind,
    /// 事件时占用千分比（零配额池有占用记 1000）。
    pub used_permille: u64,
}

/// 三级池分级总成：池组（复用 F1121）+ 各级账户树 + 告警事件环。
#[derive(Debug, Clone)]
pub struct TieredPools {
    pools: PoolSet,
    trees: [AccountTree; 3],
    alerts: Vec<AlertEvent>,
    next_seq: u64,
    last_levels: [WaterLevel; 3],
    handle_map: Vec<(u64, PoolKind, AccountId)>,
}

/// 占用千分比（零配额池有占用记 1000）。
fn permille(used: u64, quota: u64) -> u64 {
    if quota == 0 {
        if used > 0 {
            1000
        } else {
            0
        }
    } else {
        used.saturating_mul(1000) / quota
    }
}

impl TieredPools {
    /// 构造：池组守恒（复用 F1121）+ 逐级策略上限核对 +
    /// 每级账户树根配额 = 池配额。
    pub fn new(total_vram: u64, quotas: [u64; 3]) -> Result<TieredPools, u16> {
        // 策略上限：quota * 100 > max_pct * vram 即越限（整数防截断）
        for i in 0..3usize {
            let kind = PoolKind::ALL[i];
            let pol = tier_policy(kind);
            let lhs = match quotas[i].checked_mul(100) {
                Some(v) => v,
                None => return Err(ERR_TIER_POLICY),
            };
            let rhs = match pol.max_pct_of_vram.checked_mul(total_vram) {
                Some(v) => v,
                None => return Err(ERR_TIER_POLICY),
            };
            if lhs > rhs {
                return Err(ERR_TIER_POLICY);
            }
        }
        let pools = match PoolSet::new(total_vram, quotas) {
            Ok(p) => p,
            Err(_) => return Err(ERR_TIER_POLICY), // H01 守恒码映射为 H03 口径
        };
        Ok(TieredPools {
            pools: pools,
            trees: [
                AccountTree::new(quotas[PoolKind::Process.index()]),
                AccountTree::new(quotas[PoolKind::System.index()]),
                AccountTree::new(quotas[PoolKind::Reserved.index()]),
            ],
            alerts: Vec::new(),
            next_seq: 1,
            last_levels: [WaterLevel::Empty; 3],
            handle_map: Vec::new(),
        })
    }

    /// 池只读透传（水位监控直接读 F1121 池账）。
    pub fn pool(&self, kind: PoolKind) -> &BudgetPool {
        self.pools.pool(kind)
    }

    /// 池可变透传。
    pub fn pool_mut(&mut self, kind: PoolKind) -> &mut BudgetPool {
        self.pools.pool_mut(kind)
    }

    /// 池级账户树只读访问。
    pub fn tree(&self, kind: PoolKind) -> &AccountTree {
        &self.trees[kind.index()]
    }

    /// 开账户（路由到对应级树）。
    pub fn open_account(
        &mut self,
        kind: PoolKind,
        parent: AccountId,
        quota: u64,
        label: &str,
    ) -> Result<AccountId, u16> {
        self.trees[kind.index()].open_account(parent, quota, label)
    }

    /// 扣费（原子双记账）：树层 charge 成功后池层分配；树失败无副作用，
    /// 池成功树失败则池侧回滚——任一层拒绝即整体拒绝。
    pub fn charge(
        &mut self,
        kind: PoolKind,
        account: AccountId,
        bytes: u64,
    ) -> Result<u64, u16> {
        if self.trees[kind.index()].node(account).is_none() {
            return Err(ERR_TREE_SHAPE);
        }
        if self.handle_map.len() >= MAX_TRACKED {
            return Err(ERR_TRACK_FULL);
        }
        let handle = match self.pools.allocate(kind, account as u32, bytes) {
            Ok(h) => h,
            Err(_) => return Err(ERR_TIER_POLICY), // 池配额/守恒拒绝
        };
        if self.trees[kind.index()].charge(account, bytes).is_err() {
            // 树层拒绝：池侧回滚，保持双账一致
            let _ = self.pools.release(kind, handle);
            return Err(ERR_TIER_POLICY);
        }
        self.handle_map.push((handle, kind, account));
        Ok(handle)
    }

    /// 归还（原子双记账）：池层释放得字节数，树层沿链扣减，追踪表摘除。
    pub fn release(&mut self, kind: PoolKind, handle: u64) -> Result<u64, u16> {
        let pos = match self.handle_map.iter().position(|e| e.0 == handle && e.1 == kind) {
            None => return Err(ERR_ALERT_STATE),
            Some(p) => p,
        };
        let account = self.handle_map[pos].2;
        let bytes = match self.pools.release(kind, handle) {
            Ok(b) => b,
            Err(_) => return Err(ERR_TIER_POLICY),
        };
        if self.trees[kind.index()].release(account, bytes).is_err() {
            return Err(ERR_TREE_CONSERVATION); // 账面已破，audit 兜底立案
        }
        self.handle_map.swap_remove(pos);
        Ok(bytes)
    }

    /// 水位轮询（边缘触发）：三级各自判定，与上次档位比较出事件——
    /// 升 High / 升 Critical / 降回常规各一式；稳定期零事件（防风暴）。
    /// 返回本次新增事件数。
    pub fn poll_alerts(&mut self) -> usize {
        let mut added = 0usize;
        for i in 0..3usize {
            let kind = PoolKind::ALL[i];
            let level = self.pools.pool(kind).water_level();
            let prev = self.last_levels[i];
            let fired = if level == WaterLevel::Critical && prev != WaterLevel::Critical {
                Some(AlertKind::PoolCritical)
            } else if level == WaterLevel::High
                && prev != WaterLevel::High
                && prev != WaterLevel::Critical
            {
                Some(AlertKind::PoolHigh)
            } else if (prev == WaterLevel::High || prev == WaterLevel::Critical)
                && (level == WaterLevel::Normal || level == WaterLevel::Empty)
            {
                Some(AlertKind::PoolRecovered)
            } else {
                None
            };
            if let Some(kind_ev) = fired {
                let used = self.pools.pool(kind).used();
                let quota = self.pools.pool(kind).quota();
                let ev = AlertEvent {
                    seq: self.next_seq,
                    pool: kind,
                    kind: kind_ev,
                    used_permille: permille(used, quota),
                };
                self.next_seq += 1;
                self.alerts.push(ev);
                added += 1;
                if self.alerts.len() > ALERT_CAP {
                    self.alerts.remove(0); // 环形：挤掉最老
                }
            }
            self.last_levels[i] = level;
        }
        added
    }

    /// 事件表只读（时间序）。
    pub fn alerts(&self) -> &[AlertEvent] {
        &self.alerts
    }

    /// 按类别计数（联动统计口径）。
    pub fn alert_count(&self, kind: AlertKind) -> usize {
        let mut n = 0usize;
        for e in self.alerts.iter() {
            if e.kind == kind {
                n += 1;
            }
        }
        n
    }

    /// 双层审计：池层守恒（复用 F1121）+ 三树守恒（本域）+
    /// 追踪表与池账一致（表长 = Σ池存活分配数；表内句柄池侧可查）。
    pub fn audit(&self) -> Result<(), u16> {
        self.pools.audit().map_err(|_| ERR_TREE_CONSERVATION)?;
        for i in 0..3usize {
            self.trees[i].conservation()?;
        }
        let mut tracked = 0usize;
        for i in 0..3usize {
            tracked = match tracked.checked_add(self.pools.pool(PoolKind::ALL[i]).alloc_count()) {
                Some(v) => v,
                None => return Err(ERR_TREE_CONSERVATION),
            };
        }
        if tracked != self.handle_map.len() {
            return Err(ERR_ALERT_STATE);
        }
        for e in self.handle_map.iter() {
            if self.pools.pool(e.1).lookup(e.0).is_none() {
                return Err(ERR_ALERT_STATE);
            }
            if self.trees[e.1.index()].node(e.2).is_none() {
                return Err(ERR_TREE_CONSERVATION);
            }
        }
        Ok(())
    }
}
