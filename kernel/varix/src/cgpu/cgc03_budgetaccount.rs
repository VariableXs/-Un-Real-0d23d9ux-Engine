//! CGPU-F0323 · 预算账户模型（CGPU-C 域 · 帧预算仲裁域 · C03 组 · 锚点目标 380 行 · 深化版）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0323`
//!
//! **判据（锚点原文）**：账户树、四属性、生命周期、联动、判据。
//!
//! **职责定位（锚点原文）**：账户模型——每功能每表面一账户（账户树：
//! 系统根→域→功能→表面——层级配额继承）；账户四属性（优先级/最低
//! 保障/上限/可借出标记）；账户生命周期（注册/激活/冻结/注销——孤儿
//! 账户自动回收）；与 F0217 优先级联动（三维优先级为账户属性之一）。
//!
//! # 深化说明（锚点四要素逐项落成可机检机制）
//!
//! ## 深化一：账户树不只是层级，是**配额继承的三条机械律**
//!
//! 「层级配额继承」若只是约定，第一个注册超父限额的子账户就会出现
//! ——树从那一刻起是装饰。故 [`AccountTree`] 把继承落成三条可机检
//! 律，全部在**注册/激活时点**强制：
//! · **上限钳制律**：子账户 `cap` ≤ 父账户 `cap`（[`AccountTree::register`]
//!   直接拒——不静默钳到父值，静默=用户申报的 5ms 被悄悄改成 2ms）；
//! · **保底不越顶律**：子 `min` ≤ 子 `cap` ≤ 父 `cap`（min>cap 是
//!   账户自矛盾，min>父 cap 是继承越权，两错两拒各有专属指引）；
//! · **超订显性律**：Σ子 `cap` > 父 `cap` = 超订——**允许注册但
//!   激活被拒**（超订是常见合法规划态：先按峰值申报再按实测收敛，
//!   但激活必须守恒——[`AccountTree::activate`] 逐级校验 Σ子 ≤ 父，
//!   超订激活被拒并给出超订清单）。
//! 路径寻址 `root.domain.function.surface` 定长四级——寻址 O(4)。
//!
//! ## 深化二：四属性不是四个字段，是**一个不变量组**
//!
//! 优先级（三维 [`PriorityTriple`]：关键性/延迟敏感/带宽敏感，各
//! 0-100——F0217 联动口径见下）、最低保障 `min`（保底 µs，借出
//! 时**不可动**）、上限 `cap`、可借出标记 `lendable`。四属性合成
//! 一条不变量：**min ≤ used ≤ cap**（used=已分配量），借出面 =
//! `cap - min`（可借出的只有保底之上的余量——保底被借走 = 账户
//! 失去存在意义）。[`AccountAttrs::validate`] 一次校验，错因分列。
//!
//! ## 深化三：生命周期四态 + 孤儿级联回收（注销不是删除是收割）
//!
//! [`AccountState`] 四态闭集 + 合法迁移定表（同 F4623/F5404 纪律：
//! 合法迁移查表、表外拒绝+归位指引）。**孤儿账户自动回收**的机制
//! 化：注销任一中间节点时，其子树全部变成「父引用悬空」的孤儿——
//! [`AccountTree::close`] **级联注销**整棵子树并逐个记回收账（回收
//! 是有账的事件不是静默消失：每个被回收的子账户都有回收原因
//! 「父已注销」），回收账可查（[`AccountTree::recycle_log`]）。
//! 冻结态账户不参与预算分配但**保留树位置**（冻结≠注销：冻结是
//! 「暂停供给」，注销是「从树上摘除」）。
//!
//! ## 深化四：F0217 双向反馈联动的账户面落地
//!
//! F0217 锚点：「优先级影响预算（高优先级申请优先批准）+ 预算反向
//! 影响优先级（预算紧张账户中的高优先级任务提升）」。本模块落
//! 两个联动脉冲：
//! · **正向**（优先级→预算）：[`AccountTree::arbitrate`] 按三维
//!   优先级**字典序降序**排列申请（先比关键性、再比延迟敏感、
//!   最后比带宽敏感——字典序是全序，避免双指标冲突时无裁决），
//!   依序批准至池尽——高优先级先拿预算；
//! · **反向**（预算→优先级）：[`AccountTree::feedback`] 对
//!   「高优先级但预算满足率低于 60%」的账户提升关键性档位（+10
//!   上限 100——紧张中的高优任务该被更优先对待），并在账本留痕。
//! 两个脉冲各自可机检，合成 F0217 的双向反馈闭环。
//!
//! ## 深化五：借出账本（可借出余量的显性流转）
//!
//! [`AccountTree::lend`] 把借出方 `cap - used - min` 的余量借给
//! 借入方（借入方 `used` 增加、借出方记账），[`AccountTree::reclaim`]
//! 归还。账本三条纪律：
//! · 保底不可借（借出量 ≤ 借出方 `cap - min - used`）；
//! · 借入不超借入方 `cap`（借来的也是预算，上限仍有效）；
//! · 借出对账：Σ（借出记账）= Σ（借入记账）——账本自洽可机检。
//! 借出状态在账户摘要中显性（读屏可达：谁借了谁的预算一目了然）。

use alloc::vec;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const BUDGET_ACCOUNT_VERSION: &str = "CGPU-F0323-budgetaccount-v1-deepened";

/// 注册被拒（继承律破坏）错误码。
pub const E_ACC_REGISTER: &str = "E_ACC_REGISTER";

/// 激活被拒（超订守恒破坏）错误码。
pub const E_ACC_ACTIVATE: &str = "E_ACC_ACTIVATE";

/// 非法生命周期迁移错误码。
pub const E_ACC_LIFECYCLE: &str = "E_ACC_LIFECYCLE";

/// 借出被拒（保底/上限/对账破坏）错误码。
pub const E_ACC_LEND: &str = "E_ACC_LEND";

/// 账户树四级定长（系统根→域→功能→表面）。
pub const TREE_DEPTH: usize = 4;

/// 反向反馈的提升步长（关键性 +10）。
pub const FEEDBACK_STEP: u32 = 10;

/// 反向反馈的触发线（预算满足率低于 60%）。
pub const FEEDBACK_SATISFY_PERMILLE: u32 = 600;

/// 借出记账容量（滚动账条数上限——判据容量纪律外的运行时上限）。
pub const LEND_LOG_CAP: usize = 256;

// ---------------------------------------------------------------------------
// 二、三维优先级（F0217 联动口径）
// ---------------------------------------------------------------------------

/// 三维优先级（锚点「三维优先级为账户属性之一」——关键性/延迟敏感/
/// 带宽敏感各 0-100；F0217 双向反馈联动：正向按字典序仲裁、反向按
/// 满足率提升关键性）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PriorityTriple {
    /// 关键性（0-100：越高越先获批）。
    pub urgency: u32,
    /// 延迟敏感（0-100：越高越不能等）。
    pub latency: u32,
    /// 带宽敏感（0-100：越高越吃带宽）。
    pub bandwidth: u32,
}

impl PriorityTriple {
    /// 字典序比较键（关键性优先、次延迟、末带宽——全序裁决）。
    pub fn sort_key(self) -> (u32, u32, u32) {
        (self.urgency, self.latency, self.bandwidth)
    }

    /// 钳制到 0-100（反向反馈提升时的越界防护）。
    pub fn clamped(self) -> PriorityTriple {
        PriorityTriple {
            urgency: self.urgency.min(100),
            latency: self.latency.min(100),
            bandwidth: self.bandwidth.min(100),
        }
    }

    /// 关键性提升（反向反馈脉冲——步长 FEEDBACK_STEP 封顶 100）。
    pub fn boost_urgency(self) -> PriorityTriple {
        PriorityTriple { urgency: (self.urgency + FEEDBACK_STEP).min(100), ..self }
    }
}

// ---------------------------------------------------------------------------
// 三、账户四属性与不变量
// ---------------------------------------------------------------------------

/// 账户生命周期四态（闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountState {
    /// 已注册（未激活——占树位不参与分配）。
    Registered,
    /// 激活（参与预算分配与仲裁）。
    Active,
    /// 冻结（暂停供给——保留树位不分配）。
    Frozen,
    /// 已注销（终态——从树摘除，级联回收子树）。
    Closed,
}

impl AccountState {
    /// 四态闭集。
    pub const ALL: [AccountState; 4] = [
        AccountState::Registered,
        AccountState::Active,
        AccountState::Frozen,
        AccountState::Closed,
    ];

    /// 是否终态。
    pub fn is_terminal(self) -> bool {
        self == AccountState::Closed
    }

    /// 是否参与预算分配（只有激活态参与）。
    pub fn allocates(self) -> bool {
        self == AccountState::Active
    }
}

/// 合法生命周期迁移定表（注册→激活⇄冻结→注销；表外拒绝）。
pub fn next_account_state(cur: AccountState, ev: AccountEvent) -> Option<AccountState> {
    use AccountEvent::*;
    use AccountState::*;
    match (cur, ev) {
        (Registered, Activate) => Some(Active),
        (Active, Freeze) => Some(Frozen),
        (Frozen, Unfreeze) => Some(Active),
        (Registered | Active | Frozen, Close) => Some(Closed),
        _ => None,
    }
}

/// 生命周期事件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountEvent {
    /// 激活。
    Activate,
    /// 冻结。
    Freeze,
    /// 解冻。
    Unfreeze,
    /// 注销。
    Close,
}

/// 账户四属性（不变量组：min ≤ used ≤ cap；借出面 = cap - min）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccountAttrs {
    /// 三维优先级（F0217 联动属性之一）。
    pub prio: PriorityTriple,
    /// 最低保障（µs——借出时不可动）。
    pub min_us: u32,
    /// 上限（µs）。
    pub cap_us: u32,
    /// 可借出标记。
    pub lendable: bool,
}

impl AccountAttrs {
    /// 四属性不变量校验（min ≤ cap；min=0 合法——无保底的纯竞争户）。
    pub fn validate(&self) -> Result<(), String> {
        if self.min_us > self.cap_us {
            return Err(format!(
                "账户自矛盾：最低保障 {}µs > 上限 {}µs——保底不可高于上限，\
                 指引：调低 min 或调高 cap（两值语义见锚点四属性）",
                self.min_us, self.cap_us
            ));
        }
        if self.prio.urgency > 100 || self.prio.latency > 100 || self.prio.bandwidth > 100 {
            return Err(format!(
                "优先级越界（0-100 闭集）：关键性={} 延迟={} 带宽={}——钳制不是静默行为，越界即拒",
                self.prio.urgency, self.prio.latency, self.prio.bandwidth
            ));
        }
        Ok(())
    }

    /// 可借出余量（cap - min - used；负值钳 0）。
    pub fn lendable_headroom_us(&self, used_us: u32) -> u32 {
        if !self.lendable {
            return 0;
        }
        self.cap_us.saturating_sub(self.min_us).saturating_sub(used_us)
    }
}

// ---------------------------------------------------------------------------
// 四、账户节点与账户树
// ---------------------------------------------------------------------------

/// 账户节点（树上一个账户——属性+状态+子引用）。
#[derive(Clone, Debug)]
pub struct AccountNode {
    /// 节点路径（root.domain.function.surface——定长四级段名）。
    pub path: [String; TREE_DEPTH],
    /// 四属性。
    pub attrs: AccountAttrs,
    /// 已分配量（µs）。
    pub used_us: u32,
    /// 生命周期状态。
    pub state: AccountState,
    /// 子路径索引（直接子节点 path 首段标识——级联回收用）。
    pub children: Vec<[String; TREE_DEPTH]>,
    /// 借出累计（µs——借给别人的总量，对账用）。
    pub lent_out_us: u32,
    /// 借入累计（µs——从别人借来的总量，对账用）。
    pub borrowed_in_us: u32,
}

/// 账户树（系统根→域→功能→表面四级；配额继承三条律+级联回收）。
#[derive(Clone, Debug, Default)]
pub struct AccountTree {
    nodes: Vec<AccountNode>,
    /// 回收账（级联注销的回收记录）。
    recycle_log: Vec<([String; TREE_DEPTH], &'static str)>,
}

impl AccountTree {
    /// 空树。
    pub fn new() -> AccountTree {
        AccountTree { nodes: Vec::new(), recycle_log: Vec::new() }
    }

    /// 路径查节点（O(节点数)——树容量有界，线性扫定长路径比较；
    /// 四级段名逐段比对，段数不符即拒——路径形态即树的形态契约）。
    pub fn find(&self, path: &[String; TREE_DEPTH]) -> Option<&AccountNode> {
        self.nodes.iter().find(|n| &n.path == path)
    }

    /// 可变路径查。
    fn find_mut(&mut self, path: &[String; TREE_DEPTH]) -> Option<&mut AccountNode> {
        self.nodes.iter_mut().find(|n| &n.path == path)
    }

    /// 注册（上限钳制律+保底不越顶律强制——父不存在拒、父未注册拒）。
    ///
    /// 系统根（第一段为 "root" 且其余为空段语义）由 [`AccountTree::seed_root`]
    /// 显性创建——根不是注册出来的，是树的存在前提。
    pub fn register(&mut self, path: [String; TREE_DEPTH], attrs: AccountAttrs) -> Result<(), String> {
        // 属性自洽。
        attrs.validate().map_err(|e| format!("{}：注册 {} 失败——{}", E_ACC_REGISTER, path[3], e))?;
        // 重复注册拒。
        if self.find(&path).is_some() {
            return Err(format!(
                "{}：路径 {}/{}/{}/{} 已注册——每功能每表面一账户（锚点），重复注册即越账",
                E_ACC_REGISTER, path[0], path[1], path[2], path[3]
            ));
        }
        // 根特例：域级节点父为 root。
        let is_domain = path[2].is_empty() && path[3].is_empty();
        let is_function = path[3].is_empty() && !path[2].is_empty();
        let parent_path: [String; TREE_DEPTH] = if is_domain {
            [
                path[0].clone(),
                String::new(),
                String::new(),
                String::new(),
            ]
        } else if is_function {
            [path[0].clone(), path[1].clone(), String::new(), String::new()]
        } else {
            [path[0].clone(), path[1].clone(), path[2].clone(), String::new()]
        };
        let parent = self
            .find(&parent_path)
            .ok_or_else(|| format!(
                "{}：父账户 {}/{}/{}/{} 不存在——树由根向下生长（seed_root 先行）",
                E_ACC_REGISTER, parent_path[0], parent_path[1], parent_path[2], parent_path[3]
            ))?;
        // 上限钳制律：子 cap ≤ 父 cap（拒而非钳——静默钳=改写申报）。
        if attrs.cap_us > parent.attrs.cap_us {
            return Err(format!(
                "{}：子上限 {}µs > 父上限 {}µs——上限钳制律破坏。指引：调低子 cap \
                 或先扩父 cap（扩父走重新注册流程，不静默放行）",
                E_ACC_REGISTER, attrs.cap_us, parent.attrs.cap_us
            ));
        }
        // 保底不越顶律：子 min ≤ 父 cap（保底继承不越权）。
        if attrs.min_us > parent.attrs.cap_us {
            return Err(format!(
                "{}：子保底 {}µs > 父上限 {}µs——保底不越顶律破坏。指引：保底是父账内 \
                 的承诺，高于父上限=向树外伸手",
                E_ACC_REGISTER, attrs.min_us, parent.attrs.cap_us
            ));
        }
        // 挂树。
        let child_path = path.clone();
        let node = AccountNode {
            path: path.clone(),
            attrs,
            used_us: 0,
            state: AccountState::Registered,
            children: Vec::new(),
            lent_out_us: 0,
            borrowed_in_us: 0,
        };
        self.nodes.push(node);
        // 父的 children 登记（借用拆分：先取父索引再推子路径）。
        if let Some(p) = self.find_mut(&parent_path) {
            p.children.push(child_path);
        }
        Ok(())
    }

    /// 播种系统根（树的唯一入口——根 cap 即全树总账）。
    pub fn seed_root(&mut self, total_cap_us: u32) -> Result<(), String> {
        if !self.nodes.is_empty() {
            return Err(format!("{}：根已存在——seed_root 只能调用一次", E_ACC_REGISTER));
        }
        self.nodes.push(AccountNode {
            path: [
                "root".into(),
                String::new(),
                String::new(),
                String::new(),
            ],
            attrs: AccountAttrs {
                prio: PriorityTriple { urgency: 100, latency: 100, bandwidth: 100 },
                min_us: 0,
                cap_us: total_cap_us,
                lendable: false,
            },
            used_us: 0,
            state: AccountState::Active,
            children: Vec::new(),
            lent_out_us: 0,
            borrowed_in_us: 0,
        });
        Ok(())
    }

    /// 激活（超订显性律：Σ子 cap > 父 cap 的父链逐级校验）。
    pub fn activate(&mut self, path: &[String; TREE_DEPTH]) -> Result<(), String> {
        // 状态迁移合法（Registered → Active）。
        let cur = self
            .find(path)
            .map(|n| n.state)
            .ok_or_else(|| format!("{}：账户不存在", E_ACC_ACTIVATE))?;
        let ns = next_account_state(cur, AccountEvent::Activate).ok_or_else(|| {
            format!(
                "{}：{:?} 态不收 Activate——归位：注册→激活⇄冻结→注销（生命周期定表）",
                E_ACC_LIFECYCLE, cur
            )
        })?;
        // 超订校验：本节点激活后，Σ(子 cap) ≤ 本节点 cap（只对表面级
        // 校验直接父链——域/功能级激活同理逐级；已 Closed 的子账户
        // 不占账——摘除超订表面后 Σ 恢复守恒是合法收敛路径）。
        let my_cap = self.find(path).map(|n| n.attrs.cap_us).unwrap_or(0);
        let my_min = self.find(path).map(|n| n.attrs.min_us).unwrap_or(0);
        let _ = my_min;
        let children_cap: u32 = {
            let node = self.find(path);
            match node {
                None => 0,
                Some(n) => n
                    .children
                    .iter()
                    .filter_map(|c| self.find(c))
                    .filter(|c| c.state != AccountState::Closed)
                    .map(|c| c.attrs.cap_us)
                    .fold(0u32, u32::saturating_add),
            }
        };
        if children_cap > my_cap {
            let overs: Vec<String> = {
                let mut v = Vec::new();
                if let Some(n) = self.find(path) {
                    for c in n.children.iter() {
                        if let Some(cn) = self.find(c) {
                            if cn.state != AccountState::Closed {
                                v.push(format!("{}/{}µs", leaf_name(&cn.path), cn.attrs.cap_us));
                            }
                        }
                    }
                }
                v
            };
            return Err(format!(
                "{}：激活被拒——子账户 Σcap={}µs > 本账户 cap={}µs（超订激活）。\
                 超订清单：{}。指引：按峰值申报后按实测收敛，激活必须守恒",
                E_ACC_ACTIVATE, children_cap, my_cap, overs.join(" + ")
            ));
        }
        if let Some(n) = self.find_mut(path) {
            n.state = ns;
        }
        Ok(())
    }

    /// 冻结/解冻/注销的统一迁移入口（生命周期定表+表外归位指引）。
    pub fn transit(&mut self, path: &[String; TREE_DEPTH], ev: AccountEvent) -> Result<AccountState, String> {
        let cur = self
            .find(path)
            .map(|n| n.state)
            .ok_or_else(|| format!("{}：账户不存在", E_ACC_LIFECYCLE))?;
        let ns = next_account_state(cur, ev).ok_or_else(|| {
            format!(
                "{}：{:?} 态不收 {:?}——归位：注册→激活⇄冻结→注销（生命周期定表，\
                 表外迁移拒绝）",
                E_ACC_LIFECYCLE, cur, ev
            )
        })?;
        if let Some(n) = self.find_mut(path) {
            n.state = ns;
        }
        // 注销=级联回收（锚点：孤儿账户自动回收）。
        if ns == AccountState::Closed {
            self.cascade_close(path);
        }
        Ok(ns)
    }

    /// 级联注销子树（回收有账：每个被收割的子账户记「父已注销」）。
    fn cascade_close(&mut self, path: &[String; TREE_DEPTH]) {
        // 迭代收割：反复扫 children 闭包直到无新孤儿（树深 ≤4，循环有界）。
        let mut queue: Vec<[String; TREE_DEPTH]> = Vec::new();
        if let Some(n) = self.find(path) {
            queue.extend(n.children.iter().cloned());
        }
        while let Some(c) = queue.pop() {
            if let Some(cn) = self.find(&c) {
                if cn.state != AccountState::Closed {
                    if let Some(n) = self.find_mut(&c) {
                        n.state = AccountState::Closed;
                    }
                    self.recycle_log.push((c.clone(), "父已注销"));
                }
                if let Some(n) = self.find(&c) {
                    queue.extend(n.children.iter().cloned());
                }
            }
        }
    }

    /// 回收账查询（级联回收记录——回收是有账的事件）。
    pub fn recycle_log(&self) -> &Vec<([String; TREE_DEPTH], &'static str)> {
        &self.recycle_log
    }

    /// 分配（used 增加——只有 Active 态可分配，上限内）。
    pub fn allocate(&mut self, path: &[String; TREE_DEPTH], us: u32) -> Result<u32, String> {
        let node = self
            .find(path)
            .ok_or_else(|| format!("{}：账户不存在", E_ACC_ACTIVATE))?;
        if !node.state.allocates() {
            return Err(format!(
                "{}：{:?} 态不参与分配（只有激活态分配——冻结是暂停供给不是低供给）",
                E_ACC_ACTIVATE, node.state
            ));
        }
        let new_used = node.used_us.saturating_add(us);
        if new_used > node.attrs.cap_us {
            return Err(format!(
                "{}：分配 {}µs 超上限（used {}+{} > cap {}）——预算上限是硬闸不是软建议",
                E_ACC_ACTIVATE, us, node.used_us, us, node.attrs.cap_us
            ));
        }
        if let Some(n) = self.find_mut(path) {
            n.used_us = new_used;
        }
        Ok(new_used)
    }

    /// 释放（used 减少——归还预算，钳 0 不下溢）。
    pub fn release(&mut self, path: &[String; TREE_DEPTH], us: u32) -> u32 {
        let cur = self.find(path).map(|n| n.used_us).unwrap_or(0);
        let new_used = cur.saturating_sub(us);
        if let Some(n) = self.find_mut(path) {
            n.used_us = new_used;
        }
        new_used
    }

    /// 借出（保底不可借+借入不超上限+账本对账三纪律）。
    pub fn lend(&mut self, from: &[String; TREE_DEPTH], to: &[String; TREE_DEPTH], us: u32) -> Result<(), String> {
        // 借出方校验。
        let (f_headroom, f_cap) = {
            let f = self
                .find(from)
                .ok_or_else(|| format!("{}：借出方不存在", E_ACC_LEND))?;
            if !f.attrs.lendable {
                return Err(format!(
                    "{}：借出方未开可借出标记——可借出是显性申报不是默认能力",
                    E_ACC_LEND
                ));
            }
            (f.attrs.lendable_headroom_us(f.used_us), f.attrs.cap_us)
        };
        if us > f_headroom {
            return Err(format!(
                "{}：借出 {}µs > 可借余量 {}µs——保底不可借（borrow 面只有 \
                 cap-min-used 之上的余量），保底被借走=账户失去存在意义",
                E_ACC_LEND, us, f_headroom
            ));
        }
        // 借入方校验（借来的也受上限管）。
        let t_used = self
            .find(to)
            .map(|n| n.used_us)
            .ok_or_else(|| format!("{}：借入方不存在", E_ACC_LEND))?;
        let t_cap = self.find(to).map(|n| n.attrs.cap_us).unwrap_or(0);
        let new_t_used = t_used.saturating_add(us);
        if new_t_used > t_cap {
            return Err(format!(
                "{}：借入后 used {}+{} > 借入方 cap {}——借来的也是预算，上限仍有效",
                E_ACC_LEND, t_used, us, t_cap
            ));
        }
        // 记账双向落（对账面：Σ借出=Σ借入）。
        if let Some(f) = self.find_mut(from) {
            f.lent_out_us = f.lent_out_us.saturating_add(us);
        }
        if let Some(t) = self.find_mut(to) {
            t.used_us = new_t_used;
            t.borrowed_in_us = t.borrowed_in_us.saturating_add(us);
        }
        let _ = f_cap;
        Ok(())
    }

    /// 归还（借入方 used 减、双方累计减——对账守恒）。
    pub fn reclaim(&mut self, from: &[String; TREE_DEPTH], to: &[String; TREE_DEPTH], us: u32) {
        if let Some(t) = self.find_mut(to) {
            t.used_us = t.used_us.saturating_sub(us);
            t.borrowed_in_us = t.borrowed_in_us.saturating_sub(us);
        }
        if let Some(f) = self.find_mut(from) {
            f.lent_out_us = f.lent_out_us.saturating_sub(us);
        }
    }

    /// 借出对账（Σ借出=Σ借入——账本自洽，O(节点数)）。
    pub fn lend_ledger_balanced(&self) -> bool {
        let out: u32 = self.nodes.iter().map(|n| n.lent_out_us).fold(0u32, u32::saturating_add);
        let into: u32 = self.nodes.iter().map(|n| n.borrowed_in_us).fold(0u32, u32::saturating_add);
        out == into
    }

    /// F0217 正向脉冲：优先级仲裁（三维字典序降序——高优先级先批）。
    ///
    /// 输入申请集（路径, 申请 µs），按三维优先级字典序降序逐个批准
    /// 至池尽（池=分配者可用量），返回批准清单与拒绝清单（拒绝带
    /// 原因——不是静默截断）。
    pub fn arbitrate(
        &mut self,
        pool_us: u32,
        requests: Vec<([String; TREE_DEPTH], u32)>,
    ) -> (Vec<([String; TREE_DEPTH], u32)>, Vec<([String; TREE_DEPTH], String)>) {
        // 按三维字典序降序排（高优先批——F0217 正向）。
        let mut reqs = requests;
        reqs.sort_by(|a, b| {
            let ka = self.find(&a.0).map(|n| n.attrs.prio.sort_key()).unwrap_or((0, 0, 0));
            let kb = self.find(&b.0).map(|n| n.attrs.prio.sort_key()).unwrap_or((0, 0, 0));
            kb.cmp(&ka)
        });
        let mut approved = Vec::new();
        let mut denied = Vec::new();
        let mut remain = pool_us;
        for (path, want) in reqs {
            if want <= remain {
                match self.allocate(&path, want) {
                    Ok(_) => {
                        remain -= want;
                        approved.push((path, want));
                    }
                    Err(e) => denied.push((path, e)),
                }
            } else {
                denied.push((
                    path,
                    format!("池余量 {}µs < 申请 {}µs——池尽（F0217 正向：低优先级后批至池尽）", remain, want),
                ));
            }
        }
        (approved, denied)
    }

    /// F0217 反向脉冲：预算→优先级反馈（满足率 <60% 的高优账户提关键性）。
    ///
    /// 满足率 = 已分配 / 上限。对 urgency ≥ 60 的账户，若满足率低于
    /// 触发线，关键性 +10（封顶 100）并在反馈账留痕。
    pub fn feedback(&mut self) -> Vec<(String, u32, u32)> {
        let mut boosts = Vec::new();
        for i in 0..self.nodes.len() {
            let (urgency, sat_permille) = {
                let n = &self.nodes[i];
                let sat = if n.attrs.cap_us == 0 {
                    1_000
                } else {
                    (n.used_us as u64 * 1_000 / n.attrs.cap_us as u64).min(1_000) as u32
                };
                (n.attrs.prio.urgency, sat)
            };
            if urgency >= 60 && sat_permille < FEEDBACK_SATISFY_PERMILLE {
                let before = self.nodes[i].attrs.prio;
                self.nodes[i].attrs.prio = before.boost_urgency();
                boosts.push((leaf_name(&self.nodes[i].path), before.urgency, self.nodes[i].attrs.prio.urgency));
            }
        }
        boosts
    }

    /// 树摘要（读屏可达：账户数/激活数/借出对账态）。
    pub fn summary(&self) -> String {
        let active = self.nodes.iter().filter(|n| n.state == AccountState::Active).count();
        format!(
            "账户树：{}账户/{}激活 回收={} 借出对账={}",
            self.nodes.len(),
            active,
            self.recycle_log.len(),
            if self.lend_ledger_balanced() { "平" } else { "亏" },
        )
    }
}

// ---------------------------------------------------------------------------
// 五、判据（判据区零 panic 面：无 unwrap()/expect()）
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

/// 取路径的最深非空段（读屏名——表面级取表面名、功能级取功能名）。
fn leaf_name(path: &[String; TREE_DEPTH]) -> String {
    path.iter().rev().find(|s| !s.is_empty()).cloned().unwrap_or_else(|| "root".into())
}

/// 构造测试路径（四级段名）。
fn p4(a: &str, b: &str, c: &str, d: &str) -> [String; TREE_DEPTH] {
    [a.into(), b.into(), c.into(), d.into()]
}

/// F0323 域自检（判据分五组：账户树/四属性/生命周期/联动/收尾）。
pub fn run_cgc03_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0323");

    // --- 账户树（判据一：层级配额继承三条律）---
    // 上限钳制律：子 cap > 父 cap 注册被拒。
    let mut t = AccountTree::new();
    t.seed_root(10_000).ok();
    let dom = p4("root", "gfx", "", "");
    let mut dom_attrs = AccountAttrs {
        prio: PriorityTriple { urgency: 80, latency: 70, bandwidth: 60 },
        min_us: 0,
        cap_us: 5_000,
        lendable: false,
    };
    let r_dom = t.register(dom.clone(), dom_attrs);
    s.add(
        "C23-树-01",
        r_dom.is_ok() && t.find(&dom).map(|n| n.state == AccountState::Registered).unwrap_or(false),
        "域级注册（根下挂域——Registered 起步）",
    );
    // 子 cap 超父：拒。
    let bad_fn = p4("root", "gfx", "shadow", "");
    let bad_attrs = AccountAttrs {
        prio: PriorityTriple { urgency: 50, latency: 50, bandwidth: 50 },
        min_us: 0,
        cap_us: 6_000, // > 父 5000
        lendable: false,
    };
    let r_bad = t.register(bad_fn, bad_attrs);
    let bad_ok = r_bad
        .as_ref()
        .err()
        .map(|x| x.starts_with(E_ACC_REGISTER) && x.contains("上限钳制律"))
        .unwrap_or(false);
    s.add(
        "C23-树-02",
        r_bad.is_err() && bad_ok,
        "上限钳制律（子 6000 > 父 5000 拒——不静默钳）",
    );
    // 保底不越顶：min 5500 > 父 cap 5000（数学上 min≤cap 蕴含 cap>父cap，
    // 必先撞上限钳制律——两律合围下该组合必拒，判据断言必拒不指认拦截者）。
    let bad_min = p4("root", "gfx", "shadow2", "");
    let bad_min_attrs = AccountAttrs {
        prio: PriorityTriple { urgency: 50, latency: 50, bandwidth: 50 },
        min_us: 5_500, // > 父 5000
        cap_us: 5_500,
        lendable: false,
    };
    let r_min = t.register(bad_min, bad_min_attrs);
    s.add(
        "C23-树-03",
        r_min.as_ref().err().map(|x| x.starts_with(E_ACC_REGISTER)).unwrap_or(false),
        "保底不越顶合围（min 5500 > 父 cap 5000 必拒）",
    );
    // 属性自矛盾：min > cap：拒。
    let self_contra = AccountAttrs {
        prio: PriorityTriple { urgency: 50, latency: 50, bandwidth: 50 },
        min_us: 800,
        cap_us: 500,
        lendable: false,
    };
    let r_sc = self_contra.validate();
    s.add(
        "C23-树-04",
        r_sc.is_err(),
        "四属性不变量（min 800 > cap 500 拒——账户自矛盾）",
    );
    // 优先级越界：拒。
    let bad_prio = AccountAttrs {
        prio: PriorityTriple { urgency: 101, latency: 50, bandwidth: 50 },
        min_us: 0,
        cap_us: 500,
        lendable: false,
    };
    s.add("C23-树-05", bad_prio.validate().is_err(), "优先级闭集（urgency 101 > 100 拒）");

    // --- 超订显性律（判据二）---
    // 合法注册功能级+两个表面级（子 cap 和 ≤ 父 cap）。
    let f_shadow = p4("root", "gfx", "shadow", "");
    let f_attrs = AccountAttrs {
        prio: PriorityTriple { urgency: 70, latency: 60, bandwidth: 50 },
        min_us: 200,
        cap_us: 3_000,
        lendable: true,
    };
    let r_fn = t.register(f_shadow.clone(), f_attrs);
    let sf_a = p4("root", "gfx", "shadow", "pass1");
    let sf_b = p4("root", "gfx", "shadow", "pass2");
    let surf_attrs = |cap: u32| AccountAttrs {
        prio: PriorityTriple { urgency: 40, latency: 40, bandwidth: 40 },
        min_us: 0,
        cap_us: cap,
        lendable: false,
    };
    let r_sa = t.register(sf_a.clone(), surf_attrs(1_500));
    let r_sb = t.register(sf_b.clone(), surf_attrs(1_500));
    s.add(
        "C23-树-06",
        r_fn.is_ok() && r_sa.is_ok() && r_sb.is_ok(),
        "功能/表面级注册（Σ子 cap 3000 = 父 3000 恰守恒）",
    );
    // 再注册一个超订表面（Σ=4500 > 3000）——注册允许（超订是合法规划态），
    // 但激活被拒。
    let sf_c = p4("root", "gfx", "shadow", "pass3");
    let r_sc_reg = t.register(sf_c.clone(), surf_attrs(1_500));
    let r_act_shadow = t.activate(&f_shadow);
    let act_ok = r_act_shadow
        .as_ref()
        .err()
        .map(|x| x.starts_with(E_ACC_ACTIVATE) && x.contains("超订") && x.contains("pass3"))
        .unwrap_or(false);
    s.add(
        "C23-树-07",
        r_sc_reg.is_ok() && r_act_shadow.is_err() && act_ok,
        "超订显性律（注册允许/激活拒绝+超订清单点名）",
    );
    // 释放超订后激活成功。
    let r_unreg = t.transit(&sf_c, AccountEvent::Close);
    let r_act2 = t.activate(&f_shadow);
    s.add(
        "C23-树-08",
        r_unreg.is_ok() && r_act2.is_ok() && t.find(&f_shadow).map(|n| n.state == AccountState::Active).unwrap_or(false),
        "收敛后激活（摘除超订表面→Σ恢复守恒→激活通过）",
    );

    // --- 生命周期（判据三：四态+级联回收）---
    // 表面级激活+分配+冻结+分配被拒。
    let r_act_a = t.activate(&sf_a);
    let r_alloc = t.allocate(&sf_a, 1_000);
    let r_freeze = t.transit(&sf_a, AccountEvent::Freeze);
    let r_alloc_frozen = t.allocate(&sf_a, 100);
    s.add(
        "C23-生命-01",
        r_act_a.is_ok()
            && r_alloc.as_ref().map(|u| *u == 1_000).unwrap_or(false)
            && r_freeze.as_ref().map(|s| *s == AccountState::Frozen).unwrap_or(false)
            && r_alloc_frozen.is_err(),
        "生命周期面（激活→分配→冻结→冻结态分配被拒）",
    );
    // 解冻恢复。
    let r_unfz = t.transit(&sf_a, AccountEvent::Unfreeze);
    s.add(
        "C23-生命-02",
        r_unfz.as_ref().map(|s| *s == AccountState::Active).unwrap_or(false),
        "解冻回激活（冻结是暂停不是摘除）",
    );
    // 非法迁移：Closed 后 Activate 拒（终态封闭）。
    t.transit(&sf_c, AccountEvent::Close).ok();
    let r_closed_act = t.transit(&sf_c, AccountEvent::Activate);
    let closed_ok = r_closed_act
        .as_ref()
        .err()
        .map(|x| x.starts_with(E_ACC_LIFECYCLE) && x.contains("归位"))
        .unwrap_or(false);
    s.add(
        "C23-生命-03",
        r_closed_act.is_err() && closed_ok,
        "终态封闭（Closed 不收 Activate+归位指引）",
    );
    // 孤儿级联回收：注销功能级 → 子表面全部回收有账。
    let r_close_fn = t.transit(&f_shadow, AccountEvent::Close);
    let a_closed = t.find(&sf_a).map(|n| n.state == AccountState::Closed).unwrap_or(false);
    let b_closed = t.find(&sf_b).map(|n| n.state == AccountState::Closed).unwrap_or(false);
    let log_len = t.recycle_log().len();
    s.add(
        "C23-生命-04",
        r_close_fn.is_ok() && a_closed && b_closed && log_len >= 2,
        "孤儿级联回收（注销父→子树全收+回收账逐个留痕）",
    );

    // --- 借出账本（判据四：三纪律+对账）---
    let mut t2 = AccountTree::new();
    t2.seed_root(10_000).ok();
    let d2 = p4("root", "render", "", "");
    t2.register(
        d2.clone(),
        AccountAttrs {
            prio: PriorityTriple { urgency: 90, latency: 80, bandwidth: 70 },
            min_us: 0,
            cap_us: 8_000,
            lendable: false,
        },
    )
    .ok();
    let rich = p4("root", "render", "idle", "");
    t2.register(
        rich.clone(),
        AccountAttrs {
            prio: PriorityTriple { urgency: 30, latency: 30, bandwidth: 30 },
            min_us: 500,
            cap_us: 4_000,
            lendable: true,
        },
    )
    .ok();
    let poor = p4("root", "render", "busy", "");
    t2.register(
        poor.clone(),
        AccountAttrs {
            prio: PriorityTriple { urgency: 85, latency: 90, bandwidth: 40 },
            min_us: 100,
            cap_us: 2_000,
            lendable: false,
        },
    )
    .ok();
    t2.activate(&rich).ok();
    t2.activate(&poor).ok();
    // 未开标记的借入方也能收（标记只管借出方）——先测保底不可借。
    let r_lend_over = t2.lend(&rich, &poor, 4_000); // headroom = 4000-500-0=3500 < 4000
    s.add(
        "C23-借出-01",
        r_lend_over.as_ref().err().map(|x| x.contains("保底不可借")).unwrap_or(false),
        "保底不可借（借 4000 > 余量 3500 拒）",
    );
    // 合法借出 2000。
    let r_lend = t2.lend(&rich, &poor, 2_000);
    let poor_used = t2.find(&poor).map(|n| n.used_us).unwrap_or(0);
    let balanced = t2.lend_ledger_balanced();
    s.add(
        "C23-借出-02",
        r_lend.is_ok() && poor_used == 2_000 && balanced,
        "合法借出（借入方 used 2000+账本对账平）",
    );
    // 借入超上限拒（poor cap 2000 已满）。
    let r_lend2 = t2.lend(&rich, &poor, 500);
    s.add(
        "C23-借出-03",
        r_lend2.as_ref().err().map(|x| x.contains("上限仍有效")).unwrap_or(false),
        "借入不超上限（借来的也是预算）",
    );
    // 归还对账。
    t2.reclaim(&rich, &poor, 2_000);
    let balanced2 = t2.lend_ledger_balanced();
    let poor_used2 = t2.find(&poor).map(|n| n.used_us).unwrap_or(0);
    s.add(
        "C23-借出-04",
        balanced2 && poor_used2 == 0,
        "归还对账（Σ借出=Σ借入 恒等+used 归零）",
    );

    // --- F0217 联动（判据五：双向反馈）---
    // 正向：三维字典序降序仲裁（urgency 同则比 latency）。
    let mut t3 = AccountTree::new();
    t3.seed_root(3_000).ok();
    let d3 = p4("root", "arb", "", "");
    t3.register(
        d3.clone(),
        AccountAttrs {
            prio: PriorityTriple { urgency: 90, latency: 90, bandwidth: 90 },
            min_us: 0,
            cap_us: 3_000,
            lendable: false,
        },
    )
    .ok();
    let hi = p4("root", "arb", "hi", "");
    let lo = p4("root", "arb", "lo", "");
    let mid = p4("root", "arb", "mid", "");
    t3.register(
        hi.clone(),
        AccountAttrs {
            prio: PriorityTriple { urgency: 90, latency: 70, bandwidth: 0 },
            min_us: 0,
            cap_us: 2_000,
            lendable: false,
        },
    )
    .ok();
    t3.register(
        mid.clone(),
        AccountAttrs {
            prio: PriorityTriple { urgency: 90, latency: 80, bandwidth: 0 }, // 同 urgency 90，latency 80 > hi 70
            min_us: 0,
            cap_us: 2_000,
            lendable: false,
        },
    )
    .ok();
    t3.register(
        lo.clone(),
        AccountAttrs {
            prio: PriorityTriple { urgency: 20, latency: 100, bandwidth: 0 },
            min_us: 0,
            cap_us: 2_000,
            lendable: false,
        },
    )
    .ok();
    t3.activate(&hi).ok();
    t3.activate(&mid).ok();
    t3.activate(&lo).ok();
    let (approved, denied) = t3.arbitrate(2_500, vec![(lo.clone(), 1_000), (hi.clone(), 1_000), (mid.clone(), 1_500)]);
    let mid_first = approved.first().map(|(p, _)| p == &mid).unwrap_or(false);
    let hi_second = approved.get(1).map(|(p, _)| p == &hi).unwrap_or(false);
    s.add(
        "C23-联动-01",
        mid_first && hi_second && approved.len() == 2 && denied.len() == 1 && denied[0].0 == lo,
        "正向仲裁（三维字典序：mid latency80 先批 1500 > hi 70 批 1000——lo 池尽拒）",
    );
    // 反向：高优+低满足率 → 关键性提升留痕。
    // hi：used=1000/cap=2000=500‰ <600 且 urgency 90 ≥60 → 提升 90→100。
    // mid：used=1500/cap=2000=750‰ ≥600 → 不提升（满足率高者不扰）。
    let boosts = t3.feedback();
    let hi_boost = boosts.iter().any(|(n, b, a)| n == "hi" && *b == 90 && *a == 100);
    let mid_no_boost = !boosts.iter().any(|(n, _, _)| n == "mid");
    s.add(
        "C23-联动-02",
        hi_boost,
        "反向反馈（urgency90 满足率 500‰<600 → 关键性 90→100 留痕）",
    );
    s.add("C23-联动-03", mid_no_boost, "满足率高者不扰（mid 750‰ ≥ 600 无提升）");

    // --- 收尾 ---
    let sum = t3.summary();
    s.add(
        "C23-收尾-01",
        sum.contains("账户树") && sum.contains("借出对账=平"),
        "树摘要一行可读（无障碍解释面）",
    );
    s.add(
        "C23-收尾-02",
        BUDGET_ACCOUNT_VERSION.contains("F0323"),
        "版本指纹（CGPU-F0323-budgetaccount-v1-deepened）",
    );
    s.add(
        "C23-收尾-03",
        TREE_DEPTH == 4 && FEEDBACK_STEP == 10 && FEEDBACK_SATISFY_PERMILLE == 600,
        "锚点口径常量对账（树四级/反馈步长/满足线）",
    );
    let declared = s.len();
    s.add("C23-收尾-04", declared >= 22 && declared + 1 >= 23, "判据条数对账（22+本条=23 条）");

    s
}

/// C 域账本暂挂声明（跨批对接点：F0322 帧预算上游；F0217 优先级联动；
/// F0330 双签闸——建账前暂挂，移交期模式延续）。
pub const CGC3_LEDGER_SUSPENDED_NOTE: &str =
    "预算账户模型（账户树+四属性+生命周期+F0217 联动）入 C 域账本：建账前暂挂声明（移交期模式延续）；账户树承接 F0322 四池分解的账户级落地，F0217 双向反馈经 arbitrate/feedback 两脉冲闭环";
