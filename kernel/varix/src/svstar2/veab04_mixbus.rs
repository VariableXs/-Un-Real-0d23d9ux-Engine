//! VE-F5604 · 混音总线骨架（AB 域 · 音频域 · AB04 单 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5604`
//!
//! **判据（锚点原文）**：总线树、主总线兜底、拒绝成环、判据。
//!
//! **职责定位（锚点原文）**：混音总线骨架——总线树抽象与路由骨架
//! （AB03 深化的地基），主总线兜底任何声音必须有路可走；总线树深度
//! 上限写死（超过即拒绝建树）；主总线兜底事件全遥测——兜底是安全网
//! 不是常态路由；路由表支持批量导入导出——大型场景的路由配置可迁移
//! 可备份。
//!
//! ## 一、总线树：父链唯一，深度写死
//!
//! 总线只有两种来源：建树时挂到已存在的父（[`BusTree::create_bus`]）、
//! 重挂到新父（[`BusTree::reattach`]）。父链唯一 ⇒ 树结构天然成立，
//! 唯一能破坏树的是 reattach 挂到自己的后代——沿新父链上行遇到自己
//! 即拒绝（判据三「拒绝成环」，[`BusErr::CycleRejected`]）。深度上限
//! [`MAX_TREE_DEPTH`] 是写死常量不是配置：建树与重挂两处同时把关，
//! 超过即拒绝（锚点「超过即拒绝建树」），路由沿父链上行成本有界
//! （锚点「路由 O(深度)」）。
//!
//! ## 二、主总线兜底：安全网不是常态路由
//!
//! 任何声音查询路由，三种情形都**必须有路可走**（判据二）：表内有
//! 且目标在树内 → 正常路由；目标已不在树内（路由断裂）→ 主总线
//! 兜底；声音未登记（孤声）→ 主总线兜底 + 报提示。三种里后两种都
//! 进遥测账（[`RouteTable::fallback_events`]）——兜底事件全遥测，
//! 兜底率异常升高说明路由配置烂了，不是安全网好用（锚点原文的
//! 警示语义在 [`RouteTable::fallback_ratio_alert`] 落地）。
//!
//! ## 三、路由配置可迁移可备份
//!
//! 导出（[`RouteTable::export_routes`]）给纯数据快照；导入
//! （[`RouteTable::import_routes`]）逐条对树校验——目标总线不存在
//! 的条目拒绝并留原因，**部分成功是合法结局**（导入报告给通过数与
//! 拒绝清单，不整批静默丢弃也不整批假成功）。
//!
//! **对接**：AB03（空间音频骨架，上游总线消费方）、AB05+ 深化；
//! 无障碍：路由可查（[`RouteTable::route_line`] 读屏单行）。
//! 零 panic 面（下标走 `get`/`Option`）、零 IO、零墙钟（逻辑 tick
//! 注入）、无全局可变状态、no_std 零 std 依赖。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 总线树深度上限（写死；超过即拒绝建树/重挂——锚点原文「写死」）。
pub const MAX_TREE_DEPTH: usize = 8;

/// 总线容量上限（骨架台账；含主总线）。
pub const MAX_BUSES: usize = 64;

/// 主总线编号（根，深度 0；任何声音的最终去路）。
pub const MASTER_BUS_ID: u16 = 0;

/// 兜底率警示线：兜底次数超过在册绑定数的该倍数即提示（安全网不是常态路由）。
pub const FALLBACK_ALERT_RATIO: u32 = 2;

// ---------------------------------------------------------------------------
// 二、总线树骨架（判据一；拒绝成环判据三）
// ---------------------------------------------------------------------------

/// 总线编号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BusId(pub u16);

/// 总线节点（父链唯一 ⇒ 树结构成立）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BusNode {
    /// 总线编号。
    pub id: BusId,
    /// 人话名（读屏可查）。
    pub name: String,
    /// 父总线（主总线为 None）。
    pub parent: Option<BusId>,
    /// 当前深度（根=0）。
    pub depth: usize,
}

/// 总线操作错误（逐类可判定；拒绝都带原因不静默）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusErr {
    /// 父总线不存在。
    OrphanParent,
    /// 深度超上限（拒绝建树/重挂）。
    DepthExceeded,
    /// 重挂将成环（新父链上遇到自身）。
    CycleRejected,
    /// 总线容量满。
    CapacityFull,
    /// 路由目标不在树内（bind 拒绝；import 逐条拒绝）。
    UnknownBus,
}

impl BusErr {
    /// 人话标签（路由可查的账面文案）。
    pub const fn label(self) -> &'static str {
        match self {
            BusErr::OrphanParent => "父总线不存在",
            BusErr::DepthExceeded => "超过树深度上限，拒绝建树",
            BusErr::CycleRejected => "重挂将成环，拒绝建环",
            BusErr::CapacityFull => "总线容量满",
            BusErr::UnknownBus => "路由目标不在树内",
        }
    }
}

/// 总线树骨架（判据一：树抽象；主总线构造即在）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BusTree {
    nodes: Vec<BusNode>,
    next_id: u16,
}

impl BusTree {
    /// 新树：主总线（根）构造即在——「任何声音必须有路可走」的结构前提。
    pub fn new() -> BusTree {
        BusTree {
            nodes: vec![BusNode {
                id: BusId(MASTER_BUS_ID),
                name: "主总线".to_string(),
                parent: None,
                depth: 0,
            }],
            next_id: 1,
        }
    }

    /// 按编号查节点。
    pub fn node(&self, id: BusId) -> Option<&BusNode> {
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                if n.id == id {
                    return Some(n);
                }
            }
            i += 1;
        }
        None
    }

    /// 是否在树内。
    pub fn contains(&self, id: BusId) -> bool {
        self.node(id).is_some()
    }

    /// 建总线（挂到父；深度超限即拒绝——判据一「超过即拒绝建树」）。
    pub fn create_bus(&mut self, parent: BusId, name: &str) -> Result<BusId, BusErr> {
        let p_depth = match self.node(parent) {
            Some(n) => n.depth,
            None => return Err(BusErr::OrphanParent),
        };
        if p_depth + 1 > MAX_TREE_DEPTH {
            return Err(BusErr::DepthExceeded);
        }
        if self.nodes.len() >= MAX_BUSES {
            return Err(BusErr::CapacityFull);
        }
        let id = BusId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        self.nodes.push(BusNode {
            id,
            name: name.to_string(),
            parent: Some(parent),
            depth: p_depth + 1,
        });
        Ok(id)
    }

    /// 重挂（唯一可能成环的操作：沿新父链上行遇自身即拒绝——判据三）。
    pub fn reattach(&mut self, bus: BusId, new_parent: BusId) -> Result<(), BusErr> {
        if self.node(bus).is_none() {
            return Err(BusErr::OrphanParent);
        }
        let mut cursor = new_parent;
        let mut hops = 0usize;
        loop {
            if cursor == bus {
                return Err(BusErr::CycleRejected);
            }
            let node = match self.node(cursor) {
                Some(n) => n,
                None => return Err(BusErr::OrphanParent),
            };
            match node.parent {
                Some(p) => {
                    cursor = p;
                    hops += 1;
                    if hops > MAX_BUSES {
                        // 树内父链理论有界（父链唯一且根无父）；超界视为
                        // 结构异常拒绝，不给死循环机会。
                        return Err(BusErr::CycleRejected);
                    }
                }
                None => break,
            }
        }
        // 深度重算：new_depth = new_parent 深度 + 1；以 bus 为根的整棵
        // 子树随迁，深度差同步修正（超限即拒绝）。
        let new_parent_depth = match self.node(new_parent) {
            Some(n) => n.depth,
            None => return Err(BusErr::OrphanParent),
        };
        let old_depth = match self.node(bus) {
            Some(n) => n.depth,
            None => return Err(BusErr::OrphanParent),
        };
        let delta = new_parent_depth as i64 - old_depth as i64 + 1;
        if delta > 0 {
            let max_sub = self.max_subtree_depth(bus);
            if (max_sub as i64) + delta > MAX_TREE_DEPTH as i64 {
                return Err(BusErr::DepthExceeded);
            }
        }
        self.shift_subtree(bus, delta);
        // 父指针改写。
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get_mut(i) {
                if n.id == bus {
                    n.parent = Some(new_parent);
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 以 bus 为根的子树最大深度（相对 bus 的深度）。
    fn max_subtree_depth(&self, bus: BusId) -> usize {
        let base = match self.node(bus) {
            Some(n) => n.depth,
            None => return 0,
        };
        let mut max_d = base;
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                if self.is_descendant_or_self(n.id, bus) && n.depth > max_d {
                    max_d = n.depth;
                }
            }
            i += 1;
        }
        max_d - base
    }

    /// 是否为 bus 的后代（或自身）——沿父链上行判定，步数有界。
    fn is_descendant_or_self(&self, id: BusId, ancestor: BusId) -> bool {
        let mut cursor = id;
        let mut hops = 0usize;
        loop {
            if cursor == ancestor {
                return true;
            }
            let node = match self.node(cursor) {
                Some(n) => n,
                None => return false,
            };
            match node.parent {
                Some(p) => {
                    cursor = p;
                    hops += 1;
                    if hops > MAX_BUSES {
                        return false;
                    }
                }
                None => return false,
            }
        }
    }

    /// 子树深度整体平移（reattach 后修正；delta 可负）。
    fn shift_subtree(&mut self, bus: BusId, delta: i64) {
        // 先收集子树成员（不可变借用结束后再改写）。
        let mut members: Vec<BusId> = Vec::new();
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                if self.is_descendant_or_self(n.id, bus) {
                    members.push(n.id);
                }
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < members.len() {
            let m = match members.get(j) {
                Some(v) => *v,
                None => break,
            };
            let mut k = 0usize;
            while k < self.nodes.len() {
                if let Some(n) = self.nodes.get_mut(k) {
                    if n.id == m {
                        let nd = n.depth as i64 + delta;
                        n.depth = if nd < 0 { 0 } else { nd as usize };
                    }
                }
                k += 1;
            }
            j += 1;
        }
    }

    /// 从总线到根的路径（O(深度)；路由可查的结构面）。
    pub fn path_to_root(&self, id: BusId) -> Option<Vec<BusId>> {
        if !self.contains(id) {
            return None;
        }
        let mut path: Vec<BusId> = Vec::new();
        let mut cursor = id;
        let mut hops = 0usize;
        loop {
            path.push(cursor);
            if cursor == BusId(MASTER_BUS_ID) {
                break;
            }
            let node = match self.node(cursor) {
                Some(n) => n,
                None => break,
            };
            match node.parent {
                Some(p) => {
                    cursor = p;
                    hops += 1;
                    if hops > MAX_BUSES {
                        break;
                    }
                }
                None => break,
            }
        }
        Some(path)
    }

    /// 在册总线数。
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// 是否为空（构造后恒假——主总线构造即在）。
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

impl Default for BusTree {
    fn default() -> BusTree {
        BusTree::new()
    }
}

// ---------------------------------------------------------------------------
// 三、路由表（判据二：主总线兜底；批量导入导出；遥测）
// ---------------------------------------------------------------------------

/// 兜底事件（全遥测账的一行——安全网不是常态路由的账面依据）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FallbackEvent {
    /// 声音编号。
    pub voice: u16,
    /// 原因（路由断裂 / 孤声未登记）。
    pub why: &'static str,
    /// 逻辑 tick。
    pub tick: u64,
}

/// 路由结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteOutcome {
    /// 实际生效总线。
    pub bus: BusId,
    /// 是否走了主总线兜底。
    pub fallback: bool,
}

/// 路由表（绑定 + 兜底 + 遥测 + 批量导入导出）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteTable {
    bindings: Vec<(u16, BusId)>,
    fallback_events: Vec<FallbackEvent>,
    fallback_count: u32,
    tick: u64,
}

impl RouteTable {
    /// 新表。
    pub fn new() -> RouteTable {
        RouteTable {
            bindings: Vec::new(),
            fallback_events: Vec::new(),
            fallback_count: 0,
            tick: 0,
        }
    }

    /// 绑定声音到总线（目标不在树内拒绝——bind 只收合法目标）。
    pub fn bind(&mut self, voice: u16, bus: BusId, tree: &BusTree) -> Result<(), BusErr> {
        if !tree.contains(bus) {
            return Err(BusErr::UnknownBus);
        }
        let mut i = 0usize;
        while i < self.bindings.len() {
            if let Some((v, _)) = self.bindings.get(i) {
                if *v == voice {
                    if let Some(slot) = self.bindings.get_mut(i) {
                        slot.1 = bus;
                    }
                    return Ok(());
                }
            }
            i += 1;
        }
        self.bindings.push((voice, bus));
        Ok(())
    }

    /// 解绑。
    pub fn unbind(&mut self, voice: u16) -> bool {
        let mut i = 0usize;
        while i < self.bindings.len() {
            if let Some((v, _)) = self.bindings.get(i) {
                let found = *v == voice;
                if found {
                    self.bindings.remove(i);
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 路由（判据二：三种情形都有路走；后两种兜底+遥测）。
    ///
    /// - 表内有且目标在树内 → 正常；
    /// - 目标不在树内（路由断裂）→ 主总线兜底 + 事件；
    /// - 声音未登记（孤声）→ 主总线兜底 + 事件 + 报提示。
    pub fn route(&mut self, voice: u16, tree: &BusTree) -> RouteOutcome {
        self.tick = self.tick.saturating_add(1);
        let mut target: Option<BusId> = None;
        let mut i = 0usize;
        while i < self.bindings.len() {
            if let Some((v, b)) = self.bindings.get(i) {
                if *v == voice {
                    target = Some(*b);
                    break;
                }
            }
            i += 1;
        }
        match target {
            Some(b) if tree.contains(b) => RouteOutcome { bus: b, fallback: false },
            Some(b) => {
                self.fallback_count = self.fallback_count.saturating_add(1);
                self.fallback_events.push(FallbackEvent {
                    voice,
                    why: "路由断裂（目标总线已不在树内）",
                    tick: self.tick,
                });
                let _ = b;
                RouteOutcome { bus: BusId(MASTER_BUS_ID), fallback: true }
            }
            None => {
                self.fallback_count = self.fallback_count.saturating_add(1);
                self.fallback_events.push(FallbackEvent {
                    voice,
                    why: "孤声未登记，挂主总线并报提示",
                    tick: self.tick,
                });
                RouteOutcome { bus: BusId(MASTER_BUS_ID), fallback: true }
            }
        }
    }

    /// 批量导出（纯数据快照——可迁移可备份）。
    pub fn export_routes(&self) -> Vec<(u16, BusId)> {
        let mut out: Vec<(u16, BusId)> = Vec::new();
        let mut i = 0usize;
        while i < self.bindings.len() {
            if let Some(p) = self.bindings.get(i) {
                out.push(*p);
            }
            i += 1;
        }
        out
    }


    /// 批量导入（逐条对树校验；目标不在树内拒绝并留原因）。
    pub fn import_routes(&mut self, routes: &[(u16, BusId)], tree: &BusTree) -> ImportReport {
        let mut accepted = 0usize;
        let mut rejected: Vec<(u16, BusErr)> = Vec::new();
        let mut i = 0usize;
        while i < routes.len() {
            let (voice, bus) = match routes.get(i) {
                Some(p) => *p,
                None => break,
            };
            match self.bind(voice, bus, tree) {
                Ok(()) => accepted += 1,
                Err(e) => rejected.push((voice, e)),
            }
            i += 1;
        }
        ImportReport { accepted, rejected }
    }

    /// 兜底率警示（锚点「兜底是安全网不是常态路由」的机制化）：
    /// 兜底次数超过在册绑定数 × [`FALLBACK_ALERT_RATIO`] 即给提示行。
    pub fn fallback_ratio_alert(&self) -> Option<String> {
        let bound = self.bindings.len() as u32;
        let threshold = bound.saturating_mul(FALLBACK_ALERT_RATIO);
        if self.fallback_count > threshold && self.fallback_count > 0 {
            Some(format!(
                "路由警示：兜底{}次>在册绑定{}×{}，兜底是安全网不是常态路由，请检查路由配置",
                self.fallback_count, bound, FALLBACK_ALERT_RATIO
            ))
        } else {
            None
        }
    }

    /// 路由单行人话（无障碍：路由可查——读屏播报行）。
    pub fn route_line(&self, voice: u16, outcome: &RouteOutcome, tree: &BusTree) -> String {
        let name = match tree.node(outcome.bus) {
            Some(n) => n.name.clone(),
            None => "未知总线".to_string(),
        };
        if outcome.fallback {
            format!(
                "声音{} 路由：主总线兜底（事件已入遥测账），生效={}({})",
                voice, name, outcome.bus.0
            )
        } else {
            format!("声音{} 路由：{}({})", voice, name, outcome.bus.0)
        }
    }

    /// 兜底事件账。
    pub fn fallback_events(&self) -> &[FallbackEvent] {
        self.fallback_events.as_slice()
    }

    /// 兜底计数。
    pub const fn fallback_count(&self) -> u32 {
        self.fallback_count
    }

    /// 在册绑定数。
    pub fn binding_count(&self) -> usize {
        self.bindings.len()
    }

    /// 逻辑 tick。
    pub const fn tick(&self) -> u64 {
        self.tick
    }
}

/// 导入报告（逐条校验：部分成功是合法结局）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportReport {
    /// 通过并写入的条数。
    pub accepted: usize,
    /// 拒绝清单（voice, 原因）。
    pub rejected: Vec<(u16, BusErr)>,
}

impl Default for RouteTable {
    fn default() -> RouteTable {
        RouteTable::new()
    }
}
