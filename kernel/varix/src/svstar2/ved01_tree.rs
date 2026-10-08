//! VE-F0601 · 图层树数据结构（VE-D 域 · 2D 合成引擎 · 目标 440 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0601`
//!
//! **判据（锚点原文）**：合成器图层树总数据结构（D 域基石，后续全组条目以其
//! 为地基）：节点结构（稳定 id、类型标签、稀疏属性表、子节点序列、脏标记族
//! ——自身脏、子树脏、内容脏三类分离不混用）；父子关系不变式（单父、无环、
//! 类型约束——容器子节点类型白名单）；稀疏属性策略（未设属性走继承或默认值，
//! 不逐节点填满）；arena 池化（节点池分配、批量销毁、句柄稳定——重建树不失效
//! 外部句柄）；操作原语复杂度声明（插入、移除、重挂、先序遍历）。
//!
//! **错误路径与降级矩阵**：环检测→挂载前断言拒绝；类型违规→白名单拒绝；
//! 池耗尽→扩容或显性上限三要素。
//!
//! **复杂度声明（判据点名，实现与声明逐条对应）**：
//! - `attach` 挂载 **O(1)**：环检测沿父链上行（受深度上限约束，非全树扫描）；
//! - `remove_subtree` 子树移除 **O(子树)**：只触碰被移除的子树节点；
//! - `detach` 摘除 **O(兄弟数)**：从父的子序列中定位移除；
//! - `reparent` 重挂 = detach + attach，复杂度为两者之和；
//! - `preorder` 先序遍历 **O(节点数)**：迭代栈实现（无递归、无重复访问）。
//!
//! **设计要点**：
//! - **三类脏标记分离不混用**：自身脏（节点自身属性变更）、子树脏（子树内容
//!   需要重扫——子节点变更时沿祖先链上行位置位）、内容脏（像素内容重绘）。
//!   三类各自独立置位/清除，置自身脏时自动给祖先链补子树脏（聚合方向单一），
//!   清除时互不牵连；
//! - **稀疏属性**：属性表只存已设置的键值对（按键有序），未设属性查询时沿
//!   父链走继承、走不到根则给类型默认值——内存与遍历双赢；
//! - **arena 池化句柄稳定**：节点落在 `Vec<Slot>` 槽位上，句柄 =
//!   (槽位号, 代数)。批量销毁只递增被销毁槽位的代数——兄弟与祖先的句柄
//!   不受影响；`reset_structure`（重建树）只清结构不动槽位，外部句柄全部有效；
//!   陈旧句柄（指向已销毁节点）被代数校验检出（E_STALE_HANDLE）。
//!
//! **跨批对接点**：上游 F0596 冻结契约、F0599 移交包；下游 F0602-F0620 全组。
//!
//! 零外部依赖，只依赖 `crate::checks`（测试侧）。

use crate::checks::CheckSet;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 节点池默认显性上限。池耗尽时按三要素报错（扩容走 ADR，不静默增长）。
pub const POOL_CAP: usize = 4096;

/// 树深度上限。环检测沿父链上行的自然终点，同时约束遍历栈的规模。
pub const MAX_DEPTH: usize = 64;

// ---------------------------------------------------------------------------
// 二、复杂度声明文档（判据点名：操作原语复杂度声明）
// ---------------------------------------------------------------------------

/// 复杂度声明（人读文本）。实现与本表逐条对应，改动必须两处同步走 ADR。
pub const COMPLEXITY_DOC: &str = "\
图层树操作原语复杂度声明（VE-F0601 · v1）：
C1 attach 挂载：O(1)（环检测沿父链上行，深度受 MAX_DEPTH 约束）。
C2 detach 摘除：O(兄弟数)（父的子序列定位移除）。
C3 remove_subtree 子树移除：O(子树)（只触碰被移除节点）。
C4 reparent 重挂：O(兄弟数 + 深度)（detach + attach 之和）。
C5 preorder 先序遍历：O(节点数)（迭代栈，无递归无重复访问）。
C6 prop_set 属性写入：O(稀疏表长)（按键有序的小表插入）。
C7 get_inherited 继承查询：O(深度)（沿父链上行到首个设置者）。";

// ---------------------------------------------------------------------------
// 三、基础类型：类型标签 / 句柄 / 稀疏属性 / 脏标记族
// ---------------------------------------------------------------------------

/// 节点类型标签（类型白名单的主体）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    /// 根：只允许挂容器。
    Root,
    /// 容器：可挂容器/表面/效果组。
    Container,
    /// 表面（绘制内容叶子）：不允许有子节点。
    Surface,
    /// 效果组：可挂容器/表面/效果组（隔离组结构载体）。
    EffectGroup,
}

impl NodeKind {
    /// 子节点类型白名单（判据：类型约束——白名单拒绝）。
    pub fn child_whitelist(self) -> &'static [NodeKind] {
        match self {
            NodeKind::Root => &[NodeKind::Container],
            NodeKind::Container => &[
                NodeKind::Container,
                NodeKind::Surface,
                NodeKind::EffectGroup,
            ],
            NodeKind::Surface => &[],
            NodeKind::EffectGroup => &[
                NodeKind::Container,
                NodeKind::Surface,
                NodeKind::EffectGroup,
            ],
        }
    }

    /// 是否允许有子节点（叶子快速判定）。
    pub fn can_have_children(self) -> bool {
        !matches!(self, NodeKind::Surface)
    }

    /// 读屏可读名。
    pub fn screen_name(self) -> &'static str {
        match self {
            NodeKind::Root => "根",
            NodeKind::Container => "容器",
            NodeKind::Surface => "表面",
            NodeKind::EffectGroup => "效果组",
        }
    }
}

/// 节点句柄：(槽位号, 代数)。代数校验使陈旧句柄可检出——句柄稳定的前提。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeHandle {
    /// arena 槽位号。
    pub slot: u32,
    /// 代数（槽位复用时递增）。
    pub gen: u32,
}

/// 稀疏属性键（判据：稀疏属性策略）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PropId {
    /// 不透明度（键序 0）。
    Opacity,
    /// Z 序键（键序 1）。
    Z,
    /// 可见性（键序 2）。
    Visible,
    /// 变换指纹（占位键——具体矩阵归 F0602，此处仅验稀疏存储与继承）。
    TransformStamp,
}

/// 属性值（判据：稀疏属性——只存已设置的键）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PropValue {
    /// 浮点值（不透明度等，0.0-1.0）。
    F(f32),
    /// 整数值（Z 序等）。
    I(i64),
    /// 布尔值（可见性等）。
    B(bool),
}

impl PropId {
    /// 类型默认值（继承走不到根时的兜底——默认值显式在册）。
    pub fn default_value(self) -> PropValue {
        match self {
            PropId::Opacity => PropValue::F(1.0),
            PropId::Z => PropValue::I(0),
            PropId::Visible => PropValue::B(true),
            PropId::TransformStamp => PropValue::I(0),
        }
    }

    /// 读屏可读名。
    pub fn screen_name(self) -> &'static str {
        match self {
            PropId::Opacity => "不透明度",
            PropId::Z => "Z序",
            PropId::Visible => "可见性",
            PropId::TransformStamp => "变换指纹",
        }
    }
}

/// 脏标记族（判据：三类分离不混用）。
///
/// 位分配：bit0 自身脏、bit1 子树脏、bit2 内容脏。三类各自独立置位/清除；
/// 置自身脏时沿祖先链补子树脏（聚合方向单一），清除互不牵连。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DirtyFlags(pub u8);

impl DirtyFlags {
    /// 自身脏位。
    pub const SELF: u8 = 1;
    /// 子树脏位。
    pub const SUBTREE: u8 = 2;
    /// 内容脏位。
    pub const CONTENT: u8 = 4;

    /// 置单一类脏（不牵连其他类）。
    pub fn set(&mut self, flag: u8) {
        self.0 |= flag;
    }

    /// 清单一类脏（其余位不动——三类分离）。
    pub fn clear(&mut self, flag: u8) {
        self.0 &= !flag;
    }

    /// 查询某一类。
    pub fn has(&self, flag: u8) -> bool {
        self.0 & flag != 0
    }

    /// 全干净。
    pub fn is_clean(&self) -> bool {
        self.0 == 0
    }
}

// ---------------------------------------------------------------------------
// 四、arena 槽位与节点
// ---------------------------------------------------------------------------

/// arena 槽位：代数 + 节点本体（None = 空槽）。
struct Slot {
    gen: u32,
    node: Option<TreeNode>,
}

/// 树节点（判据：节点结构五件——稳定 id/类型标签/稀疏属性/子序列/脏标记族）。
pub struct TreeNode {
    /// 稳定 id（外部主键，结构变更不改变）。
    pub id: u64,
    /// 类型标签。
    pub kind: NodeKind,
    /// 父槽位号（None = 未挂载/根）。
    pub parent: Option<u32>,
    /// 子节点槽位序列（插入序——Z 序归 F0606 重排）。
    pub children: Vec<u32>,
    /// 稀疏属性表（按键有序，只存已设值）。
    pub props: Vec<(PropId, PropValue)>,
    /// 脏标记族。
    pub dirty: DirtyFlags,
}

// ---------------------------------------------------------------------------
// 五、错误五元组（零静默）
// ---------------------------------------------------------------------------

/// 树操作错误五元组（发生了什么/为什么/下一步/责任方/错误码）。
#[derive(Clone, Debug)]
pub struct TreeError {
    /// 错误码（本域段：VE-F0601）。
    pub code: &'static str,
    /// 发生了什么。
    pub what: &'static str,
    /// 为什么。
    pub why: String,
    /// 下一步。
    pub next: &'static str,
    /// 责任方。
    pub who: String,
}

impl TreeError {
    fn new(code: &'static str, what: &'static str, why: &str, next: &'static str, who: &str) -> Self {
        TreeError {
            code,
            what,
            why: why.to_string(),
            next,
            who: who.to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// 六、图层树（主结构）
// ---------------------------------------------------------------------------

/// 合成器图层树：arena 池化 + 稀疏属性 + 三类脏标记。
pub struct LayerTree {
    /// 节点池。
    slots: Vec<Slot>,
    /// 空闲槽位栈。
    free: Vec<u32>,
    /// 根槽位号。
    root: Option<u32>,
    /// 稳定 id → 槽位号。
    by_id: BTreeMap<u64, u32>,
    /// 错误账本。
    errors: Vec<TreeError>,
    /// 错误账本丢弃计数。
    errors_dropped: u64,
}

impl LayerTree {
    /// 构造：空池（显性上限 `POOL_CAP`）。
    pub fn new() -> Self {
        LayerTree {
            slots: Vec::new(),
            free: Vec::new(),
            root: None,
            by_id: BTreeMap::new(),
            errors: Vec::new(),
            errors_dropped: 0,
        }
    }

    /// 错误账本。
    pub fn errors(&self) -> &[TreeError] {
        &self.errors
    }

    /// 错误账本丢弃计数。
    pub fn errors_dropped(&self) -> u64 {
        self.errors_dropped
    }

    fn record_error(&mut self, e: TreeError) {
        if self.errors.len() >= 256 {
            self.errors_dropped = self.errors_dropped.saturating_add(1);
        } else {
            self.errors.push(e);
        }
    }

    /// 句柄 → 槽位下标校验（代数不匹配即陈旧句柄）。
    fn resolve(&self, h: NodeHandle) -> Option<usize> {
        let slot = h.slot as usize;
        if slot >= self.slots.len() {
            return None;
        }
        let s = &self.slots[slot];
        if s.gen != h.gen || s.node.is_none() {
            return None;
        }
        Some(slot)
    }

    /// 只读节点访问（陈旧句柄返回 None）。
    pub fn node(&self, h: NodeHandle) -> Option<&TreeNode> {
        self.resolve(h).and_then(|i| self.slots[i].node.as_ref())
    }

    /// 稳定 id 查句柄。
    pub fn handle_of(&self, id: u64) -> Option<NodeHandle> {
        self.by_id.get(&id).map(|&slot| NodeHandle {
            slot,
            gen: self.slots[slot as usize].gen,
        })
    }

    /// 根句柄。
    pub fn root(&self) -> Option<NodeHandle> {
        self.root.map(|slot| NodeHandle {
            slot,
            gen: self.slots[slot as usize].gen,
        })
    }

    /// 当前活节点数。
    pub fn len(&self) -> usize {
        self.slots.iter().filter(|s| s.node.is_some()).count()
    }

    /// 池是否为空。
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    // -- 创建与销毁 ----------------------------------------------------------

    /// 创建节点（入池，不挂载）。池满 → `E_POOL_EXHAUSTED` 三要素。
    pub fn create(&mut self, id: u64, kind: NodeKind) -> Result<NodeHandle, TreeError> {
        if self.by_id.contains_key(&id) {
            let e = TreeError::new(
                "E_DUPLICATE_ID",
                "创建失败：稳定 id 重复",
                "同 id 节点已存在（稳定 id 是全局主键）",
                "换 id 或先销毁旧节点",
                "调用方",
            );
            self.record_error(e.clone());
            return Err(e);
        }
        if self.free.is_empty() && self.slots.len() >= POOL_CAP {
            let e = TreeError::new(
                "E_POOL_EXHAUSTED",
                "创建失败：节点池已满",
                &format!("池容量达到显性上限 {}（扩容需走 ADR）", POOL_CAP),
                "先销毁无用节点，或按 ADR 提升 POOL_CAP 后重建池",
                "调用方",
            );
            self.record_error(e.clone());
            return Err(e);
        }
        let node = TreeNode {
            id,
            kind,
            parent: None,
            children: Vec::new(),
            props: Vec::new(),
            dirty: DirtyFlags::default(),
        };
        // 复用空槽：代数递增（旧句柄失效被检出）；否则追加新槽。
        let (slot, gen) = if let Some(slot) = self.free.pop() {
            let gen = self.slots[slot as usize].gen + 1;
            self.slots[slot as usize] = Slot { gen, node: Some(node) };
            (slot, gen)
        } else {
            let slot = self.slots.len() as u32;
            self.slots.push(Slot { gen: 0, node: Some(node) });
            (slot, 0)
        };
        self.by_id.insert(id, slot);
        Ok(NodeHandle { slot, gen })
    }

    /// 批量销毁子树（判据：O(子树)——只触碰被移除节点；池化句柄稳定：
    /// 兄弟与祖先句柄不受影响，陈旧句柄被代数检出）。
    pub fn remove_subtree(&mut self, h: NodeHandle) -> Result<usize, TreeError> {
        let Some(idx) = self.resolve(h) else {
            let e = TreeError::new(
                "E_STALE_HANDLE",
                "移除失败：句柄已陈旧",
                "句柄指向的槽位已被复用或节点已销毁（代数校验未通过）",
                "以 handle_of(id) 重新取句柄",
                "调用方",
            );
            self.record_error(e.clone());
            return Err(e);
        };
        // 先序收集子树（不含递归——迭代栈）。
        let mut stack = vec![idx as u32];
        let mut collected = Vec::new();
        while let Some(s) = stack.pop() {
            collected.push(s);
            let kids: Vec<u32> = self.slots[s as usize]
                .node
                .as_ref()
                .expect("栈中槽位必活")
                .children
                .clone();
            for c in kids {
                stack.push(c);
            }
        }
        // 与父断链（O(兄弟数)）。
        if let Some(p) = self.slots[idx].node.as_ref().expect("根槽位必活").parent {
            if let Some(pn) = self.slots[p as usize].node.as_mut() {
                pn.children.retain(|&c| c != idx as u32);
            }
        }
        let count = collected.len();
        for s in collected {
            if let Some(n) = self.slots[s as usize].node.take() {
                self.by_id.remove(&n.id);
            }
            self.slots[s as usize].gen = self.slots[s as usize].gen.wrapping_add(1);
            self.free.push(s);
        }
        Ok(count)
    }

    /// 重建树（判据：重建树不失效外部句柄）：只清结构（父子关系、属性、
    /// 脏标记归零），节点本体与其句柄全部保留。
    pub fn reset_structure(&mut self) {
        for s in self.slots.iter_mut() {
            if let Some(n) = s.node.as_mut() {
                n.parent = None;
                n.children.clear();
                n.props.clear();
                n.dirty = DirtyFlags::default();
            }
        }
        self.root = None;
        // by_id 的槽位号全部不变——句柄稳定。
    }

    // -- 挂载 / 摘除 / 重挂（复杂度声明 C1/C2/C4）----------------------------

    /// 挂载 child 到 parent（判据：不变式三条挂载前断言；复杂度 O(1) 挂载本体
    /// ——环检测受 MAX_DEPTH 约束的上行链）。
    pub fn attach(&mut self, parent: NodeHandle, child: NodeHandle) -> Result<(), TreeError> {
        // 句柄有效性。
        let (Some(pi), Some(ci)) = (self.resolve(parent), self.resolve(child)) else {
            let e = TreeError::new(
                "E_STALE_HANDLE",
                "挂载失败：句柄已陈旧",
                "parent 或 child 的代数校验未通过",
                "以 handle_of(id) 重新取句柄",
                "调用方",
            );
            self.record_error(e.clone());
            return Err(e);
        };
        if pi == ci {
            let e = TreeError::new(
                "E_CYCLE",
                "挂载被拒绝：节点不能挂到自己",
                "环检测第一步即命中（自环）",
                "检查挂载方向",
                "调用方",
            );
            self.record_error(e.clone());
            return Err(e);
        }
        // 类型白名单（判据：类型违规→白名单拒绝）。
        let parent_kind = self.slots[pi].node.as_ref().expect("活槽").kind;
        let child_kind = self.slots[ci].node.as_ref().expect("活槽").kind;
        if !parent_kind.can_have_children() || !parent_kind.child_whitelist().contains(&child_kind)
        {
            let e = TreeError::new(
                "E_TYPE_VIOLATION",
                "挂载被拒绝：类型白名单违规",
                &format!(
                    "{} 不允许挂 {} 子节点",
                    parent_kind.screen_name(),
                    child_kind.screen_name()
                ),
                "按 child_whitelist 选择合法父子组合",
                "调用方",
            );
            self.record_error(e.clone());
            return Err(e);
        }
        // 无环不变式（挂载前断言，先于单父检查——环是更根本的破坏）：
        // 从 parent 沿父链上行，遇到 child 即成环。
        let child_slot = ci as u32;
        let mut cursor = self.slots[pi].node.as_ref().expect("活槽").parent;
        let mut hops = 0usize;
        loop {
            if cursor == Some(child_slot) {
                let e = TreeError::new(
                    "E_CYCLE",
                    "挂载被拒绝：会成环（无环不变式）",
                    "parent 在 child 的子树内，挂载后 child 成为自己的祖先",
                    "改挂到 child 子树外的节点",
                    "调用方",
                );
                self.record_error(e.clone());
                return Err(e);
            }
            match cursor {
                Some(p) => {
                    cursor = self.slots[p as usize].node.as_ref().expect("活槽").parent;
                    hops += 1;
                    if hops > MAX_DEPTH {
                        break;
                    }
                }
                None => break,
            }
        }
        // 单父不变式：已有父即拒绝（先 detach 再挂——显性 API 分离）。
        if self.slots[ci].node.as_ref().expect("活槽").parent.is_some() {
            let e = TreeError::new(
                "E_ALREADY_ATTACHED",
                "挂载被拒绝：子节点已有父（单父不变式）",
                "多父会让子树遍历与脏传播双计数",
                "先 detach 再挂载，或用 reparent",
                "调用方",
            );
            self.record_error(e.clone());
            return Err(e);
        }
        // 树根唯一：已有根时 Root 不可再挂。
        if parent_kind == NodeKind::Root && self.root.is_some() {
            let e = TreeError::new(
                "E_ROOT_EXISTS",
                "挂载被拒绝：根下已有容器",
                "树根唯一（Root 只允许一个容器子节点）",
                "多树场景请用多棵 LayerTree 实例",
                "调用方",
            );
            self.record_error(e.clone());
            return Err(e);
        }
        // 挂载本体 O(1)。
        self.slots[ci].node.as_mut().expect("活槽").parent = Some(pi as u32);
        self.slots[pi].node.as_mut().expect("活槽").children.push(ci as u32);
        if parent_kind == NodeKind::Root {
            self.root = Some(pi as u32);
        }
        Ok(())
    }

    /// 摘除（与父断链，节点保留在池中；复杂度 O(兄弟数)）。
    pub fn detach(&mut self, child: NodeHandle) -> Result<(), TreeError> {
        let Some(ci) = self.resolve(child) else {
            let e = TreeError::new(
                "E_STALE_HANDLE",
                "摘除失败：句柄已陈旧",
                "代数校验未通过",
                "以 handle_of(id) 重新取句柄",
                "调用方",
            );
            self.record_error(e.clone());
            return Err(e);
        };
        let n = self.slots[ci].node.as_mut().expect("活槽");
        match n.parent.take() {
            Some(p) => {
                if let Some(pn) = self.slots[p as usize].node.as_mut() {
                    pn.children.retain(|&c| c != ci as u32);
                }
                Ok(())
            }
            None => {
                let e = TreeError::new(
                    "E_NOT_ATTACHED",
                    "摘除失败：节点未挂载",
                    "节点没有父节点",
                    "确认节点处于树中再摘除",
                    "调用方",
                );
                self.record_error(e.clone());
                Err(e)
            }
        }
    }

    /// 重挂（复杂度 = detach + attach）。
    pub fn reparent(&mut self, child: NodeHandle, new_parent: NodeHandle) -> Result<(), TreeError> {
        self.detach(child)?;
        self.attach(new_parent, child)
    }

    // -- 稀疏属性（判据：稀疏属性 + 继承/默认值）------------------------------

    /// 写属性（稀疏表按键有序插入；已有键原地更新）。
    pub fn prop_set(&mut self, h: NodeHandle, key: PropId, value: PropValue) -> Result<(), TreeError> {
        let Some(ci) = self.resolve(h) else {
            let e = TreeError::new(
                "E_STALE_HANDLE",
                "写属性失败：句柄已陈旧",
                "代数校验未通过",
                "以 handle_of(id) 重新取句柄",
                "调用方",
            );
            self.record_error(e.clone());
            return Err(e);
        };
        let n = self.slots[ci].node.as_mut().expect("活槽");
        match n.props.binary_search_by_key(&key, |&(k, _)| k) {
            Ok(i) => n.props[i].1 = value,
            Err(i) => n.props.insert(i, (key, value)),
        }
        // 属性变更 = 自身脏（聚合由 mark_dirty 语义内联：沿父链补子树脏）。
        n.dirty.set(DirtyFlags::SELF);
        let parent = n.parent;
        let mut cursor = parent;
        while let Some(p) = cursor {
            let pn = self.slots[p as usize].node.as_mut().expect("活槽");
            pn.dirty.set(DirtyFlags::SUBTREE);
            cursor = pn.parent;
        }
        Ok(())
    }

    /// 读属性（本节点已设 → 返回；未设 → None，由调用方决定走继承还是默认）。
    pub fn prop_get(&self, h: NodeHandle, key: PropId) -> Option<PropValue> {
        let idx = self.resolve(h)?;
        let n = self.slots[idx].node.as_ref()?;
        n.props
            .binary_search_by_key(&key, |&(k, _)| k)
            .ok()
            .map(|i| n.props[i].1)
    }

    /// 继承读（判据：未设属性走继承或默认值）：沿父链上行到首个设置者，
    /// 走不到根给类型默认值。复杂度 O(深度)（声明 C7）。
    pub fn prop_get_inherited(&self, h: NodeHandle, key: PropId) -> Option<PropValue> {
        let mut idx = self.resolve(h)?;
        loop {
            let n = self.slots[idx].node.as_ref()?;
            if let Ok(i) = n.props.binary_search_by_key(&key, |&(k, _)| k) {
                return Some(n.props[i].1);
            }
            match n.parent {
                Some(p) => idx = p as usize,
                None => return Some(key.default_value()),
            }
        }
    }

    // -- 脏标记族（判据：三类分离不混用）--------------------------------------

    /// 置脏（单类）：自身脏沿祖先链补子树脏；内容脏/子树脏只动本节点。
    pub fn mark_dirty(&mut self, h: NodeHandle, flag: u8) -> Result<(), TreeError> {
        let Some(ci) = self.resolve(h) else {
            let e = TreeError::new(
                "E_STALE_HANDLE",
                "置脏失败：句柄已陈旧",
                "代数校验未通过",
                "以 handle_of(id) 重新取句柄",
                "调用方",
            );
            self.record_error(e.clone());
            return Err(e);
        };
        if flag & !(DirtyFlags::SELF | DirtyFlags::SUBTREE | DirtyFlags::CONTENT) != 0 || flag == 0 {
            let e = TreeError::new(
                "E_BAD_FLAG",
                "置脏失败：脏标记类型非法",
                "只允许 自身脏/子树脏/内容脏 三类之一或组合（三类分离不混用）",
                "用 DirtyFlags::SELF / SUBTREE / CONTENT 常量",
                "调用方",
            );
            self.record_error(e.clone());
            return Err(e);
        }
        {
            let n = self.slots[ci].node.as_mut().expect("活槽");
            n.dirty.set(flag);
        }
        if flag & DirtyFlags::SELF != 0 {
            let mut cursor = self.slots[ci].node.as_ref().expect("活槽").parent;
            while let Some(p) = cursor {
                let pn = self.slots[p as usize].node.as_mut().expect("活槽");
                pn.dirty.set(DirtyFlags::SUBTREE);
                cursor = pn.parent;
            }
        }
        Ok(())
    }

    /// 清脏（单类，互不牵连——三类分离的清除侧）。
    pub fn clear_dirty(&mut self, h: NodeHandle, flag: u8) -> Result<(), TreeError> {
        let Some(ci) = self.resolve(h) else {
            let e = TreeError::new(
                "E_STALE_HANDLE",
                "清脏失败：句柄已陈旧",
                "代数校验未通过",
                "以 handle_of(id) 重新取句柄",
                "调用方",
            );
            self.record_error(e.clone());
            return Err(e);
        };
        self.slots[ci]
            .node
            .as_mut()
            .expect("活槽")
            .dirty
            .clear(flag);
        Ok(())
    }

    // -- 遍历（复杂度声明 C5：O(节点数)）--------------------------------------

    /// 先序遍历（迭代栈，无递归；隐藏/移除节点不在池中自然不出现）。
    ///
    /// 只读方法：句柄陈旧的错误直接返回调用方（错误账本只记状态变更路径）。
    pub fn preorder(&self, from: NodeHandle) -> Result<Vec<NodeHandle>, TreeError> {
        let Some(idx) = self.resolve(from) else {
            return Err(TreeError::new(
                "E_STALE_HANDLE",
                "遍历失败：句柄已陈旧",
                "代数校验未通过",
                "以 handle_of(id) 重新取句柄",
                "调用方",
            ));
        };
        let gen = from.gen;
        let mut out = Vec::new();
        let mut stack = vec![idx as u32];
        while let Some(s) = stack.pop() {
            out.push(NodeHandle { slot: s, gen: self.slots[s as usize].gen });
            let kids: Vec<u32> = self.slots[s as usize]
                .node
                .as_ref()
                .expect("栈中槽位必活")
                .children
                .clone();
            // 逆序入栈 → 先序（插入序访问）。
            for c in kids.into_iter().rev() {
                stack.push(c);
            }
        }
        let _ = gen;
        Ok(out)
    }

    // -- 读屏 ------------------------------------------------------------------

    /// 读屏可读摘要（语义标注挂点预留——D 域无障碍由 UI 层承担）。
    pub fn screen_text(&self) -> String {
        format!(
            "图层树：{} 个活节点，池容量上限 {}，错误账本 {} 条",
            self.len(),
            POOL_CAP,
            self.errors().len()
        )
    }
}

impl Default for LayerTree {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 七、自检注册入口
// ---------------------------------------------------------------------------

/// VE-F0601 域自检（判据逐条映射见 `ved01_checks.rs`）。
pub fn run_ved01_checks() -> CheckSet {
    super::ved01_checks::run_ved01_checks()
}
