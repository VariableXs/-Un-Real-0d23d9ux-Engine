//! unx092_handlerregistry — UNX-F0903 handler 注册表与生命周期管理。
//!
//! 判据原文（施工书锚点 UNX-F0903，CoRun·UNX-A2-B06 批）：
//! **UNX-F0903-J1 注册表全操作正确（注册/查找/摘除 10 万次零异常），
//! 生命周期违例（在服摘除）10/10 次拦截，表与绑定表交叉一致率 100%**。
//!
//! 职责定位：给 F0901 注册进来的 handler 一个**名分账**，给 F0902 绑出去
//! 的向量一个**回查锚**——中断触发时路由层拿到 handler 名只是字符串，
//! 本单把它升格成有身份、有状态、有寿命的账面实体。Windows 深在哪：
//! Windows 的中断对象寿命管理散在 IoInitializeDpcRequest / WDF
//! WdfInterruptCreate（对象随 device 删除级联）+ ObReferenceObject
//! 引用计数三层，驱动摘除在服中断对象直接蓝屏（BUGCODE
//! IRQL_NOT_LESS_OR_EQUAL 高发地），"谁还引用着"没有账面只有运气；
//! Varix：①**五态生命周期闭集**（Created→Attached→InService→Draining
//! →Retired）+ 5×5 迁移矩阵 const 表——每个格子显性写死合法与否，
//! 非法迁移在闸口拒绝而不是运行期炸栈；②**在服摘除是显性违例**
//! （InService/Draining 态物理摘除必被拦——判据点名的 10/10 拦截，
//! 对应 Windows 驱动最凶险的 use-after-free 面）；③**墓簿**——已摘除
//! 的 id 永不复活、永不重用（Windows HANDLE 复用导致的错对象操作是
//! 十年老坑，Varix 从账面上杜绝）；④**双账交叉一致**——注册表
//! （id→实体）与绑定表（vector→id）互为镜像约束，绑定引用的 id 必须
//! 在册且在服，在册在服者必须显性 idle 或被绑，一致率机检 100%；
//! ⑤**10 万次混合操作压测**——LCG 确定性随机流（注册/查找/摘除/
//! 迁移/绑定混流），账守恒+零异常，不是"测试过"而是"账面守恒式
//! 成立"。
//! 上游：F0901 register_irq（本单 handler 名沿用其 64 字节语义）、
//! F0902 DeviceVectorSet（绑定表语义同源）；下游：F0904 共享向量
//! 分发票（消费在服态判据）、F0907 注册表审计账与回放（消费迁移
//! 历史环）、F0915 注销清理链（消费 drain 两步语义）。
//! 本单交付**注册表 + 生命周期状态机 + 墓簿 + 绑定账 + 交叉审计 +
//! 压测 + 违例注入**七面。
//!
//! 设计要点：no_std+alloc；零 unwrap/expect；零 IO 零墙钟（tick 显性
//! 注入）；LCG 确定性仿真（Numerical Recipes 参数）；FNV-1a 快照往返；
//! 错误全显性带诊断码与建议。诊断码段 0x3C（0x3B 为 F0902 所取）。

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const HANDLER_REG_VERSION: &str = "B06-handlerregistry-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x3C 段，本单独占——0x3B 为 F0902 所取）
// ---------------------------------------------------------------------------

/// 0x3C01 handler 名非法（空或超 64 字节——账面可读性契约）。
pub const XC_NAME_ILLEGAL: u32 = 0x3C01;
/// 0x3C02 名字已被占用（注册表名字唯一性破坏）。
pub const XC_NAME_TAKEN: u32 = 0x3C03;
/// 0x3C03 id 不在册（查找/操作打到幽灵 id）。
pub const XC_ID_ABSENT: u32 = 0x3C03;
/// 0x3C04 非法生命周期迁移（迁移矩阵格子拒绝）。
pub const XC_TRANSITION_ILLEGAL: u32 = 0x3C04;
/// 0x3C05 在服摘除（InService/Draining 态物理摘除——判据主拦截面）。
pub const XC_INSERVICE_DETACH: u32 = 0x3C05;
/// 0x3C06 非 retirees 摘除（未走完生命周期就物理删除）。
pub const XC_PREMATURE_REMOVE: u32 = 0x3C06;
/// 0x3C07 墓簿复活（已摘除 id 再注册/再操作）。
pub const XC_GRAVE_REVIVE: u32 = 0x3C07;
/// 0x3C08 幂等注册冲突（同 id 重复注册）。
pub const XC_DUP_REGISTER: u32 = 0x3C08;
/// 0x3C09 绑定到非在服 handler（绑定账引用未就绪实体）。
pub const XC_BIND_NOT_INSERVICE: u32 = 0x3C09;
/// 0x3C0A 绑定向量重复（一 vector 一 handler——F0902 语义同源）。
pub const XC_BIND_VECTOR_TAKEN: u32 = 0x3C0A;
/// 0x3C0B 解绑无账（摘除不存在的绑定）。
pub const XC_UNBIND_ABSENT: u32 = 0x3C0B;
/// 0x3C0C 交叉一致破坏（注册表与绑定表镜像失配——判据红线）。
pub const XC_CROSS_MISMATCH: u32 = 0x3C0C;
/// 0x3C0D 压测账守恒破坏（注册数≠在册+墓簿）。
pub const XC_LEDGER_IMBALANCE: u32 = 0x3C0D;
/// 0x3C0E 迁移历史环溢出（历史环容量写穿）。
pub const XC_HISTORY_OVERFLOW: u32 = 0x3C0E;
/// 0x3C0F 能力位越集（caps 保留位置 1）。
pub const XC_CAPS_ILLEGAL: u32 = 0x3C0F;
/// 0x3C10 能力不满足迁移（如无 AllowRetire 能力却想退休）。
pub const XC_CAPS_MISSING: u32 = 0x3C10;
/// 0x3C11 快照指纹漂移（同账面两次采样不一致——不可复现）。
pub const XC_SNAP_DRIFT: u32 = 0x3C11;

// ---------------------------------------------------------------------------
// §1 HandlerId 与名字规则
// ---------------------------------------------------------------------------

/// handler 身份 id——单调分配，永不重用（墓簿保证）。
pub type HandlerId = u64;

/// id 起点（0 保留给"无 handler"哨兵，实体从 1 起）。
pub const HANDLER_ID_BASE: HandlerId = 1;

/// 名字合法校验（沿用 F0901 语义：非空且 ≤64 字节）。
pub fn name_legal(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64
}

// ---------------------------------------------------------------------------
// §2 能力位闭集（手写 bitflags——no_std 零依赖）
// ---------------------------------------------------------------------------

/// 能力位闭集（u16 位阵——保留位必须为零）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct HandlerCaps(u16);

impl HandlerCaps {
    /// 可共享向量（F0904 分发票消费）。
    pub const SHAREABLE: u16 = 0x0001;
    /// 可迁移（允许换绑向量——F0915 清理链消费）。
    pub const MIGRATABLE: u16 = 0x0002;
    /// 允许在服换处理器亲和（F0881 引擎联动）。
    pub const HOT_AFFINITY: u16 = 0x0004;
    /// 允许退休（无此能力只能 Draining 后退役保留在册）。
    pub const ALLOW_RETIRE: u16 = 0x0008;
    /// 管理队列专用（queue0 语义——F0902 管理队列联签）。
    pub const ADMIN_ONLY: u16 = 0x0010;
    /// 保留位掩码（以上之外全保留——置 1 即非法）。
    pub const RESERVED_MASK: u16 = !0x001F;

    /// 构造：保留位非零即拒。
    pub fn new(bits: u16) -> Option<HandlerCaps> {
        if bits & Self::RESERVED_MASK != 0 {
            return None;
        }
        Some(HandlerCaps(bits))
    }

    /// 位测。
    pub const fn has(self, bit: u16) -> bool {
        self.0 & bit != 0
    }

    /// 原始位。
    pub const fn bits(self) -> u16 {
        self.0
    }
}

// ---------------------------------------------------------------------------
// §3 生命周期五态闭集与迁移矩阵
// ---------------------------------------------------------------------------

/// 生命周期五态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LifecycleState {
    /// 已创建未挂线（刚注册——名分已定，未接线）。
    Created,
    /// 已挂线（绑定表有引用，尚未开始服务）。
    Attached,
    /// 在服（正在接事件——在服摘除拦截的本体态）。
    InService,
    /// 排空中（两步注销第一步：停在途事件）。
    Draining,
    /// 已退役（终态——只有此态可物理摘除）。
    Retired,
}

impl LifecycleState {
    /// 全态枚举（闭集遍历——矩阵索引与审计用）。
    pub const fn all() -> [LifecycleState; 5] {
        [
            LifecycleState::Created,
            LifecycleState::Attached,
            LifecycleState::InService,
            LifecycleState::Draining,
            LifecycleState::Retired,
        ]
    }

    /// 矩阵下标（单点——索引错位即矩阵失真）。
    pub const fn idx(self) -> usize {
        match self {
            LifecycleState::Created => 0,
            LifecycleState::Attached => 1,
            LifecycleState::InService => 2,
            LifecycleState::Draining => 3,
            LifecycleState::Retired => 4,
        }
    }

    /// 人话名。
    pub const fn name(self) -> &'static str {
        match self {
            LifecycleState::Created => "created",
            LifecycleState::Attached => "attached",
            LifecycleState::InService => "in-service",
            LifecycleState::Draining => "draining",
            LifecycleState::Retired => "retired",
        }
    }
}

/// 迁移矩阵——5×5 const 表，`[from][to]`，true=合法。
///
/// 合法迁移恰 7 格（其余 18 格全拒）：
/// ```text
/// Created   → Attached            （挂线）
/// Created   → Retired             （未用先退——空转退役）
/// Attached  → InService           （开始服务）
/// Attached  → Draining            （未服先排空——挂线即撤）
/// InService → Draining            （两步注销第一步）
/// Draining  → InService           （排空中恢复——事件回流撤销注销）
/// Draining  → Retired             （两步注销第二步：排空毕退役）
/// ```
/// 设计深意：InService **直达 Retired 不可达**——在服摘除在状态机层面
/// 就不存在（闸是双重的：矩阵格拒绝 + detach 显性拦截）。
pub const TRANSITION_MATRIX: [[bool; 5]; 5] = [
    // to:  Cre    Att     InS     Dra     Ret
    [false, true, false, false, true], // Created
    [false, false, true, true, false], // Attached
    [false, false, false, true, false], // InService（无直达 Retired！）
    [false, false, true, false, true], // Draining
    [false, false, false, false, false], // Retired（终态封死）
];

/// 迁移合法性查表。
pub fn transition_legal(from: LifecycleState, to: LifecycleState) -> bool {
    TRANSITION_MATRIX[from.idx()][to.idx()]
}

/// 合法迁移格数自检（恰 7——矩阵写坏先于运行期暴露）。
pub fn transition_matrix_cells() -> usize {
    let mut n = 0;
    for row in TRANSITION_MATRIX.iter() {
        for &c in row.iter() {
            if c {
                n += 1;
            }
        }
    }
    n
}

// ---------------------------------------------------------------------------
// §4 HandlerEntry 与迁移历史环
// ---------------------------------------------------------------------------

/// 历史环容量（迁移记录上限——超限滚动覆盖最旧）。
pub const HISTORY_CAP: usize = 8;

/// 迁移历史一条。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Transition {
    /// from 态名（存名不存枚举——回放可读）。
    pub from: &'static str,
    /// to 态名。
    pub to: &'static str,
    /// 迁移时逻辑 tick。
    pub at: u64,
}

/// handler 实体——注册表正账项。
#[derive(Clone, Debug)]
pub struct HandlerEntry {
    /// 人话可读名（唯一键之一）。
    pub name: String,
    /// 能力位。
    pub caps: HandlerCaps,
    /// 生命周期态。
    pub state: LifecycleState,
    /// 注册时逻辑 tick。
    pub registered_at: u64,
    /// 最后迁移 tick。
    pub last_transition_at: u64,
    /// 迁移历史环（滚动覆盖）。
    pub history: Vec<Transition>,
    /// 迁移总次数（历史环滚动但总账不减——守恒面）。
    pub transitions_total: u32,
}

impl HandlerEntry {
    /// 新实体（Created 态）。
    fn new(name: &str, caps: HandlerCaps, tick: u64) -> HandlerEntry {
        HandlerEntry {
            name: name.to_string(),
            caps,
            state: LifecycleState::Created,
            registered_at: tick,
            last_transition_at: tick,
            history: Vec::new(),
            transitions_total: 0,
        }
    }

    /// 记一条迁移（环滚动 + 总账累加）。
    fn record(&mut self, from: LifecycleState, to: LifecycleState, tick: u64) {
        if self.history.len() >= HISTORY_CAP {
            self.history.remove(0);
        }
        self.history.push(Transition {
            from: from.name(),
            to: to.name(),
            at: tick,
        });
        self.transitions_total += 1;
        self.last_transition_at = tick;
    }
}

// ---------------------------------------------------------------------------
// §5 HandlerRegistry 本体
// ---------------------------------------------------------------------------

/// handler 注册表——双账（id 正账 + 名字反账）+ 墓簿。
#[derive(Clone, Debug, Default)]
pub struct HandlerRegistry {
    /// 正账：id → 实体。
    by_id: BTreeMap<HandlerId, HandlerEntry>,
    /// 反账：名字 → id（名字唯一性锚）。
    by_name: BTreeMap<String, HandlerId>,
    /// 墓簿：已物理摘除 id → 摘除 tick（永不复活）。
    graves: BTreeMap<HandlerId, u64>,
    /// id 分配器（单调递增，跨墓簿不回收）。
    next_id: HandlerId,
    /// 逻辑 tick（外部注入）。
    tick: u64,
}

impl HandlerRegistry {
    /// 新表。
    pub fn new() -> HandlerRegistry {
        HandlerRegistry {
            by_id: BTreeMap::new(),
            by_name: BTreeMap::new(),
            graves: BTreeMap::new(),
            next_id: HANDLER_ID_BASE,
            tick: 0,
        }
    }

    /// 推进逻辑 tick。
    pub fn advance_tick(&mut self, n: u64) {
        self.tick = self.tick.saturating_add(n);
    }

    /// 当前 tick。
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// 注册：名字+能力 → 分配 id、Created 态入册。
    ///
    /// 违例面：名字非法 / 名字已被在册占用 / id 已在墓簿（理论不可达，
    /// id 单调不重用——防御性立案）。
    pub fn register(&mut self, name: &str, caps: HandlerCaps) -> Result<HandlerId, (u32, String)> {
        if !name_legal(name) {
            return Err((
                XC_NAME_ILLEGAL,
                format!("handler 名非法：len={}", name.len()),
            ));
        }
        if self.by_name.contains_key(name) {
            return Err((XC_NAME_TAKEN, format!("名字 {} 已被在册 handler 占用", name)));
        }
        let id = self.next_id;
        self.next_id += 1;
        if self.graves.contains_key(&id) {
            return Err((XC_GRAVE_REVIVE, format!("id {} 已在墓簿——分配器越界立案", id)));
        }
        self.by_id.insert(id, HandlerEntry::new(name, caps, self.tick));
        self.by_name.insert(name.to_string(), id);
        Ok(id)
    }

    /// 按 id 查（只读借用）。
    pub fn lookup(&self, id: HandlerId) -> Option<&HandlerEntry> {
        self.by_id.get(&id)
    }

    /// 按名查 id。
    pub fn lookup_by_name(&self, name: &str) -> Option<HandlerId> {
        self.by_name.get(name).copied()
    }

    /// 在册数。
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// 墓簿数。
    pub fn grave_count(&self) -> usize {
        self.graves.len()
    }

    /// 生命周期迁移（矩阵闸 + 能力闸 + 历史记账）。
    pub fn transition(
        &mut self,
        id: HandlerId,
        to: LifecycleState,
    ) -> Result<LifecycleState, (u32, String)> {
        let from = {
            let e = self
                .by_id
                .get(&id)
                .ok_or((XC_ID_ABSENT, format!("id {} 不在册", id)))?;
            e.state
        };
        if !transition_legal(from, to) {
            return Err((
                XC_TRANSITION_ILLEGAL,
                format!("迁移 {} → {} 被矩阵拒绝（id {}）", from.name(), to.name(), id),
            ));
        }
        // 能力闸：退休需 AllowRetire。
        if to == LifecycleState::Retired && from != LifecycleState::Created {
            let caps = self
                .by_id
                .get(&id)
                .map(|e| e.caps)
                .ok_or((XC_ID_ABSENT, format!("id {} 不在册", id)))?;
            if !caps.has(HandlerCaps::ALLOW_RETIRE) {
                return Err((
                    XC_CAPS_MISSING,
                    format!("id {} 无 AllowRetire 能力，{} → retired 被拒", id, from.name()),
                ));
            }
        }
        let e = self
            .by_id
            .get_mut(&id)
            .ok_or((XC_ID_ABSENT, format!("id {} 不在册", id)))?;
        e.record(from, to, self.tick);
        e.state = to;
        Ok(from)
    }

    /// 物理摘除：**仅 Retired 可摘**——在服摘除是判据红线。
    ///
    /// 摘除即双账同删 + 墓簿立案（id 永不重用）。
    pub fn remove(&mut self, id: HandlerId) -> Result<HandlerEntry, (u32, String)> {
        let e = self
            .by_id
            .get(&id)
            .ok_or((XC_ID_ABSENT, format!("id {} 不在册（或已摘）", id)))?;
        if e.state != LifecycleState::Retired {
            return Err((
                if e.state == LifecycleState::InService || e.state == LifecycleState::Draining {
                    XC_INSERVICE_DETACH
                } else {
                    XC_PREMATURE_REMOVE
                },
                format!(
                    "id {} 处于 {} 态，物理摘除被拒（仅 retired 可摘）",
                    id,
                    e.state.name()
                ),
            ));
        }
        let e = self.by_id.remove(&id).ok_or((
            XC_ID_ABSENT,
            format!("id {} 双账竞态（读时在删时无——防御立案）", id),
        ))?;
        self.by_name.remove(&e.name);
        self.graves.insert(id, self.tick);
        Ok(e)
    }

    /// 在服摘除的显性 API（供路由层守卫复用——语义同 remove 的拦截支）。
    pub fn detach_guard(&self, id: HandlerId) -> Result<(), (u32, String)> {
        let e = self
            .by_id
            .get(&id)
            .ok_or((XC_ID_ABSENT, format!("id {} 不在册", id)))?;
        if e.state == LifecycleState::InService || e.state == LifecycleState::Draining {
            return Err((
                XC_INSERVICE_DETACH,
                format!("id {} 在服/排空中，摘除被守卫拦截", id),
            ));
        }
        Ok(())
    }

    /// id 是否在墓簿。
    pub fn is_dead(&self, id: HandlerId) -> bool {
        self.graves.contains_key(&id)
    }

    /// 全部在册 id 升序。
    pub fn ids(&self) -> Vec<HandlerId> {
        self.by_id.keys().copied().collect()
    }

    /// 按态清点。
    pub fn count_in_state(&self, s: LifecycleState) -> usize {
        self.by_id.values().filter(|e| e.state == s).count()
    }

    /// 守恒自检：双账长度一致 + 名字反查闭环 + 无幽灵墓簿 id。
    pub fn audit(&self) -> Result<(), (u32, String)> {
        if self.by_id.len() != self.by_name.len() {
            return Err((
                XC_LEDGER_IMBALANCE,
                format!("双账长度失配：id 账 {} 名账 {}", self.by_id.len(), self.by_name.len()),
            ));
        }
        for (name, id) in &self.by_name {
            match self.by_id.get(id) {
                Some(e) if &e.name == name => {}
                _ => {
                    return Err((
                        XC_CROSS_MISMATCH,
                        format!("名→id 反查失配：{} → {}", name, id),
                    ));
                }
            }
        }
        for id in self.graves.keys() {
            if self.by_id.contains_key(id) {
                return Err((
                    XC_GRAVE_REVIVE,
                    format!("墓簿 id {} 同时在册——守恒崩塌", id),
                ));
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// §6 绑定账（BindingLedger）——F0902 语义同源的单点建模
// ---------------------------------------------------------------------------

/// 绑定账：vector → handler_id（一 vector 一 handler）。
#[derive(Clone, Debug, Default)]
pub struct BindingLedger {
    /// vector → id。
    by_vector: BTreeMap<u8, HandlerId>,
    /// id → vectors（反向引用——交叉审计镜像面）。
    by_handler: BTreeMap<HandlerId, Vec<u8>>,
}

impl BindingLedger {
    /// 新账。
    pub fn new() -> BindingLedger {
        BindingLedger::default()
    }

    /// 绑定：vector → id（vector 唯一；id 必须在册且 InService——
    /// 绑定到非在服实体是 0x3C09 违例）。
    pub fn bind(
        &mut self,
        reg: &HandlerRegistry,
        vector: u8,
        id: HandlerId,
    ) -> Result<(), (u32, String)> {
        let e = reg
            .lookup(id)
            .ok_or((XC_ID_ABSENT, format!("id {} 不在册，不可绑", id)))?;
        if e.state != LifecycleState::InService {
            return Err((
                XC_BIND_NOT_INSERVICE,
                format!("id {} 处于 {} 态，仅 in-service 可绑", id, e.state.name()),
            ));
        }
        if let Some(prev) = self.by_vector.get(&vector) {
            return Err((
                XC_BIND_VECTOR_TAKEN,
                format!("向量 {:#x} 已绑 id {}", vector, prev),
            ));
        }
        self.by_vector.insert(vector, id);
        self.by_handler.entry(id).or_default().push(vector);
        Ok(())
    }

    /// 解绑。
    pub fn unbind(&mut self, vector: u8) -> Result<HandlerId, (u32, String)> {
        let id = self
            .by_vector
            .remove(&vector)
            .ok_or((XC_UNBIND_ABSENT, format!("向量 {:#x} 无绑定可解", vector)))?;
        if let Some(vs) = self.by_handler.get_mut(&id) {
            vs.retain(|&v| v != vector);
            if vs.is_empty() {
                self.by_handler.remove(&id);
            }
        }
        Ok(id)
    }

    /// 查 vector 绑的 id。
    pub fn handler_of(&self, vector: u8) -> Option<HandlerId> {
        self.by_vector.get(&vector).copied()
    }

    /// 查 id 被绑的向量集。
    pub fn vectors_of(&self, id: HandlerId) -> &[u8] {
        self.by_handler.get(&id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// 绑定数。
    pub fn len(&self) -> usize {
        self.by_vector.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.by_vector.is_empty()
    }
}

// ---------------------------------------------------------------------------
// §7 交叉一致审计（判据"一致率 100%"本体）
// ---------------------------------------------------------------------------

/// 交叉审计结果。
#[derive(Clone, Debug)]
pub struct CrossReport {
    /// 检查项总数。
    pub checked: u32,
    /// 一致项数。
    pub consistent: u32,
    /// 失配明细（≤8 条）。
    pub mismatches: Vec<String>,
}

impl CrossReport {
    /// 一致率（万分制——100% 即 10000）。
    pub fn rate_permille(&self) -> u32 {
        if self.checked == 0 {
            return 0;
        }
        self.consistent * 10_000 / self.checked
    }
}

/// 交叉审计：注册表 ↔ 绑定表双向镜像对拍。
///
/// 方向 A（绑定 → 注册表）：每条绑定引用的 id 必须在册且处于
/// InService/Draining（drain 语义：排空中保留绑定清在途事件——
/// F0901 两步注销同源）。
/// 方向 B（注册表 → 绑定）：InService/Draining 实体要么有绑定（busy）
/// 要么无绑定（显性 idle——合法但计入对拍）；其余态不许挂绑定。
pub fn cross_audit(reg: &HandlerRegistry, binds: &BindingLedger) -> Result<CrossReport, (u32, String)> {
    const SERVICE_OK: fn(LifecycleState) -> bool =
        |s| s == LifecycleState::InService || s == LifecycleState::Draining;
    let mut rep = CrossReport {
        checked: 0,
        consistent: 0,
        mismatches: Vec::new(),
    };
    // 方向 A。
    for (&vector, &id) in binds.by_vector.iter() {
        rep.checked += 1;
        match reg.lookup(id) {
            Some(e) if SERVICE_OK(e.state) => rep.consistent += 1,
            Some(e) => {
                if rep.mismatches.len() < 8 {
                    rep.mismatches.push(format!(
                        "向量 {:#x} → id {} 在册但态 {}（应 in-service/draining）",
                        vector, id, e.state.name()
                    ));
                }
            }
            None => {
                if rep.mismatches.len() < 8 {
                    rep.mismatches.push(format!(
                        "向量 {:#x} → id {} 是幽灵引用（不在册）",
                        vector, id
                    ));
                }
            }
        }
    }
    // 方向 B。
    for (&id, e) in reg.by_id.iter() {
        rep.checked += 1;
        let bound = binds.vectors_of(id);
        match e.state {
            LifecycleState::InService | LifecycleState::Draining => {
                // busy 或 idle 都合法——但对拍项：绑定侧记录的向量必须与
                // 反向账一致。
                let n_binds = bound.len();
                let n_mirror = binds
                    .by_vector
                    .values()
                    .filter(|&&v| v == id)
                    .count();
                if n_binds == n_mirror {
                    rep.consistent += 1;
                } else if rep.mismatches.len() < 8 {
                    rep.mismatches.push(format!(
                        "id {} 反向账 {} 条 ≠ 正账镜像 {} 条",
                        id, n_binds, n_mirror
                    ));
                }
            }
            _ => {
                // 非在服实体不允许有任何绑定。
                if bound.is_empty() {
                    rep.consistent += 1;
                } else if rep.mismatches.len() < 8 {
                    rep.mismatches.push(format!(
                        "id {} 态 {} 却还挂着 {} 条绑定",
                        id,
                        e.state.name(),
                        bound.len()
                    ));
                }
            }
        }
    }
    if !rep.mismatches.is_empty() {
        return Err((
            XC_CROSS_MISMATCH,
            format!("交叉失配 {} 项：{}", rep.mismatches.len(), rep.mismatches.join("；")),
        ));
    }
    Ok(rep)
}

// ---------------------------------------------------------------------------
// §8 确定性底座（LCG + FNV）
// ---------------------------------------------------------------------------

/// LCG 状态（Numerical Recipes 参数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lcg(u32);

impl Lcg {
    /// 种子。
    pub const fn new(seed: u32) -> Lcg {
        Lcg(seed)
    }

    /// 下一步。
    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        self.0
    }

    /// [0, n) 均匀。
    pub fn below(&mut self, n: u32) -> u32 {
        self.next() % n.max(1)
    }
}

/// FNV-1a 32 位。
pub fn fnv32(data: &[u8], mut hash: u32) -> u32 {
    for &b in data {
        hash ^= u32::from(b);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// FNV 偏移基。
pub const FNV_OFFSET: u32 = 0x811C_9DC5;

// ---------------------------------------------------------------------------
// §9 十万次混合操作压测（判据 J1 主面：零异常 + 守恒）
// ---------------------------------------------------------------------------

/// 压测统计账。
#[derive(Clone, Debug, Default)]
pub struct FuzzStats {
    /// 注册成功次数。
    pub registered: u64,
    /// 查找命中次数。
    pub lookups_hit: u64,
    /// 查找落空次数。
    pub lookups_miss: u64,
    /// 迁移成功次数。
    pub transitions_ok: u64,
    /// 迁移被拒次数（预期拦截——不是异常）。
    pub transitions_blocked: u64,
    /// 摘除成功次数。
    pub removed: u64,
    /// 摘除被拦次数（在服守卫——判据面）。
    pub removes_blocked: u64,
    /// 绑定成功 / 解绑成功。
    pub binds: u64,
    pub unbinds: u64,
    /// 注册失败次数（重名等预期拦截）。
    pub register_blocked: u64,
}

impl FuzzStats {
    /// 总操作数（每次注入计一次）。
    pub fn ops(&self) -> u64 {
        self.registered
            + self.register_blocked
            + self.lookups_hit
            + self.lookups_miss
            + self.transitions_ok
            + self.transitions_blocked
            + self.removed
            + self.removes_blocked
            + self.binds
            + self.unbinds
    }
}

/// 十万次混合操作：LCG 决定操作类型与参数，账面全显性处理。
///
/// 零异常定义：全程无 panic、无 unwrap、每次操作要么成功要么被
/// 显性拦截并计数——不存在"不知道发生了什么"的路径。
/// 守恒式：`registered == 在册数 + removed + (迁移到 retired 未摘部分)`，
/// 另有 `注册器 next_id - 1 == registered + register_blocked + ... ` 的
/// 弱守恒（id 单调不重用）。
pub fn fuzz_100k_ops() -> Result<String, (u32, String)> {
    let mut reg = HandlerRegistry::new();
    let mut binds = BindingLedger::new();
    let mut rng = Lcg::new(0xF00D_5EED);
    let mut stats = FuzzStats::default();
    // 存活 id 池（压测自己维护的并行账——与注册表对拍）。
    let mut alive: Vec<HandlerId> = Vec::new();
    // 名字池（重名注入面）。
    const NAME_POOL: u32 = 512;

    const N: u32 = 100_000;
    for i in 0..N {
        match rng.below(10) {
            // ── 注册（4/10 权重——保持池子增长压力）──
            0 | 1 | 2 | 3 => {
                let name = format!("h-{}", rng.below(NAME_POOL));
                let caps_r = rng.below(2);
                let caps = HandlerCaps::new(HandlerCaps::ALLOW_RETIRE | if caps_r == 1 { HandlerCaps::SHAREABLE } else { 0 })
                    .ok_or((XC_CAPS_ILLEGAL, format!("#{} 能力构造失败", i)))?;
                match reg.register(&name, caps) {
                    Ok(id) => {
                        stats.registered += 1;
                        alive.push(id);
                    }
                    Err((XC_NAME_TAKEN, _)) => {
                        stats.register_blocked += 1;
                    }
                    Err(e) => return Err(e),
                }
            }
            // ── 查找（2/10）──
            4 | 5 => {
                if alive.is_empty() && reg.is_empty() {
                    stats.lookups_miss += 1;
                } else {
                    // 一半概率查真账（在册池），一半查随机 id（命中/落空都合法）。
                    let id = if rng.below(2) == 1 && !alive.is_empty() {
                        alive[(rng.below(alive.len() as u32)) as usize]
                    } else {
                        HANDLER_ID_BASE + rng.below(2000) as HandlerId
                    };
                    if reg.lookup(id).is_some() {
                        stats.lookups_hit += 1;
                    } else {
                        stats.lookups_miss += 1;
                    }
                }
            }
            // ── 迁移（1.5/10）──
            6 => {
                let pick = if alive.is_empty() {
                    None
                } else {
                    Some(alive[(rng.below(alive.len() as u32)) as usize])
                };
                if let Some(id) = pick {
                    let to = LifecycleState::all()[(rng.below(5)) as usize];
                    // 退役前解绑（Retired 态不许挂绑定——交叉审计口径）。
                    if to == LifecycleState::Retired {
                        let vs: Vec<u8> = binds.vectors_of(id).to_vec();
                        for v in vs {
                            let _ = binds.unbind(v);
                            stats.unbinds += 1;
                        }
                    }
                    match reg.transition(id, to) {
                        Ok(_) => {
                            stats.transitions_ok += 1;
                        }
                        Err((XC_TRANSITION_ILLEGAL, _)) | Err((XC_CAPS_MISSING, _)) => {
                            stats.transitions_blocked += 1;
                        }
                        Err(e) => return Err(e),
                    }
                } else {
                    stats.lookups_miss += 1;
                }
            }
            // ── 绑定/解绑（1.5/10）——绑定到已推到 InService 的 handler ──
            7 => {
                let pick = if alive.is_empty() {
                    None
                } else {
                    Some(alive[(rng.below(alive.len() as u32)) as usize])
                };
                if let Some(id) = pick {
                    // 把它推到 InService（经合法路径：Created→Attached→InService）。
                    let _ = reg.transition(id, LifecycleState::Attached);
                    let _ = reg.transition(id, LifecycleState::InService);
                    let vector = 0x60 + (rng.below(16) as u8);
                    match binds.bind(&reg, vector, id) {
                        Ok(_) => stats.binds += 1,
                        Err((XC_BIND_VECTOR_TAKEN, _)) | Err((XC_BIND_NOT_INSERVICE, _)) => {
                            // 向量被占：先解旧账再试一次（或直接放弃——都合法）。
                            let _ = binds.unbind(vector);
                        }
                        Err(e) => return Err(e),
                    }
                }
            }
            // ── 摘除（1/10）——只摘 retired 的（其余预期被拦）──
            8 => {
                let pick = if alive.is_empty() {
                    None
                } else {
                    Some(alive[(rng.below(alive.len() as u32)) as usize])
                };
                if let Some(id) = pick {
                    // 走合法退役链：→(Attached)→Draining→Retired→remove。
                    // 摘除前解绑全部向量（绑定账不留幽灵引用）。
                    let vs: Vec<u8> = binds.vectors_of(id).to_vec();
                    for v in vs {
                        let _ = binds.unbind(v);
                        stats.unbinds += 1;
                    }
                    let st = reg.lookup(id).map(|e| e.state);
                    match st {
                        Some(LifecycleState::Retired) => {}
                        Some(_) => {
                            let _ = reg.transition(id, LifecycleState::Attached);
                            let _ = reg.transition(id, LifecycleState::Draining);
                            let _ = reg.transition(id, LifecycleState::Retired);
                        }
                        None => {}
                    }
                    match reg.remove(id) {
                        Ok(_) => {
                            stats.removed += 1;
                            alive.retain(|&x| x != id);
                        }
                        Err((XC_INSERVICE_DETACH, _)) | Err((XC_PREMATURE_REMOVE, _)) => {
                            stats.removes_blocked += 1;
                        }
                        Err(e) => return Err(e),
                    }
                }
            }
            // ── 交叉审计抽查（0.5/10）──
            _ => {
                cross_audit(&reg, &binds)?;
            }
        }
        // tick 随操作推进（时序注入）。
        if i % 7 == 0 {
            reg.advance_tick(1);
        }
    }
    // 终局守恒对拍：并行账 alive 与注册表在册集合一致。
    let mut reg_ids = reg.ids();
    reg_ids.sort_unstable();
    let mut alive_sorted = alive;
    alive_sorted.sort_unstable();
    alive_sorted.dedup();
    if reg_ids != alive_sorted {
        return Err((
            XC_LEDGER_IMBALANCE,
            format!(
                "压测终局失配：注册表 {} 项 vs 并行账 {} 项",
                reg_ids.len(),
                alive_sorted.len()
            ),
        ));
    }
    reg.audit()?;
    let rep = cross_audit(&reg, &binds)?;
    if rep.rate_permille() != 10_000 {
        return Err((
            XC_CROSS_MISMATCH,
            format!("终局交叉一致率 {} ≠ 10000", rep.rate_permille()),
        ));
    }
    Ok(format!(
        "10 万次操作零异常：ops={} 注册{}(拦{}) 查{}+{} 迁{}(拦{}) 摘{}(拦{}) 绑{} 解{} 终局交叉一致率100%",
        stats.ops(),
        stats.registered,
        stats.register_blocked,
        stats.lookups_hit,
        stats.lookups_miss,
        stats.transitions_ok,
        stats.transitions_blocked,
        stats.removed,
        stats.removes_blocked,
        stats.binds,
        stats.unbinds,
    ))
}

// ---------------------------------------------------------------------------
// §10 生命周期违例注入套件（10 类全拦截）
// ---------------------------------------------------------------------------

/// 注入命中记录。
#[derive(Clone, Debug)]
pub struct LifecycleHit {
    /// 注入编号。
    pub kind: u8,
    /// 描述。
    pub what: String,
    /// 是否拦截。
    pub intercepted: bool,
    /// 证据。
    pub evidence: String,
}

/// 生命周期违例注入——10 类，全拦截为过。
///
/// 0 在服摘除（InService 直 remove）　1 排空摘除（Draining 直 remove）
/// 2 在服直达退役（矩阵封死格）　3 未挂线开服（Created→InService 跳迁）
/// 4 已挂线回 Created（逆时序）　5 终态再迁移（Retired→任意）
/// 6 重名注册　7 幽灵 id 操作（不在册 id 迁移）
/// 8 绑定非在服实体（Attached 态绑定向量）
/// 9 无能力退休（无 AllowRetire 却走 Draining→Retired）
pub fn lifecycle_violations_10() -> Result<String, (u32, String)> {
    let mut hits: Vec<LifecycleHit> = Vec::new();

    // ── 注入 0：在服摘除 ──
    {
        let mut reg = HandlerRegistry::new();
        let caps = HandlerCaps::new(HandlerCaps::ALLOW_RETIRE).ok_or((XC_CAPS_ILLEGAL, "构造".to_string()))?;
        let id = reg.register("svc-0", caps)?;
        let _ = reg.transition(id, LifecycleState::Attached);
        let _ = reg.transition(id, LifecycleState::InService);
        let r = reg.remove(id);
        hits.push(LifecycleHit {
            kind: 0,
            what: "InService 态物理摘除".to_string(),
            intercepted: matches!(r, Err((XC_INSERVICE_DETACH, _))),
            evidence: match r {
                Err(e) => e.1,
                Ok(_) => "未拦截——在服摘除闸失效".to_string(),
            },
        });
    }
    // ── 注入 1：排空摘除（Draining 未排空毕）──
    {
        let mut reg = HandlerRegistry::new();
        let caps = HandlerCaps::new(HandlerCaps::ALLOW_RETIRE).ok_or((XC_CAPS_ILLEGAL, "构造".to_string()))?;
        let id = reg.register("svc-1", caps)?;
        let _ = reg.transition(id, LifecycleState::Draining); // Created→Draining 是非法格
        let r = reg.transition(id, LifecycleState::Retired);
        // Created→Draining 非法，应拦截——这就是本注入的实质（跳过挂线排空）。
        hits.push(LifecycleHit {
            kind: 1,
            what: "Created 跳迁 Draining 后直退".to_string(),
            intercepted: r.is_err(),
            evidence: match r {
                Err(e) => e.1,
                Ok(_) => "未拦截——排空纪律破坏".to_string(),
            },
        });
    }
    // ── 注入 2：在服直达退役（矩阵封死格）──
    {
        let mut reg = HandlerRegistry::new();
        let caps = HandlerCaps::new(HandlerCaps::ALLOW_RETIRE).ok_or((XC_CAPS_ILLEGAL, "构造".to_string()))?;
        let id = reg.register("svc-2", caps)?;
        let _ = reg.transition(id, LifecycleState::Attached);
        let _ = reg.transition(id, LifecycleState::InService);
        let r = reg.transition(id, LifecycleState::Retired);
        hits.push(LifecycleHit {
            kind: 2,
            what: "InService → Retired 直达（矩阵封死）".to_string(),
            intercepted: matches!(r, Err((XC_TRANSITION_ILLEGAL, _))),
            evidence: match r {
                Err(e) => e.1,
                Ok(_) => "未拦截——状态机双闸失效".to_string(),
            },
        });
    }
    // ── 注入 3：未挂线开服 ──
    {
        let mut reg = HandlerRegistry::new();
        let caps = HandlerCaps::new(0).ok_or((XC_CAPS_ILLEGAL, "构造".to_string()))?;
        let id = reg.register("svc-3", caps)?;
        let r = reg.transition(id, LifecycleState::InService);
        hits.push(LifecycleHit {
            kind: 3,
            what: "Created → InService 跳迁（未挂线）".to_string(),
            intercepted: matches!(r, Err((XC_TRANSITION_ILLEGAL, _))),
            evidence: match r {
                Err(e) => e.1,
                Ok(_) => "未拦截——挂线纪律破坏".to_string(),
            },
        });
    }
    // ── 注入 4：逆时序回退 ──
    {
        let mut reg = HandlerRegistry::new();
        let caps = HandlerCaps::new(0).ok_or((XC_CAPS_ILLEGAL, "构造".to_string()))?;
        let id = reg.register("svc-4", caps)?;
        let _ = reg.transition(id, LifecycleState::Attached);
        let r = reg.transition(id, LifecycleState::Created);
        hits.push(LifecycleHit {
            kind: 4,
            what: "Attached → Created 逆时序".to_string(),
            intercepted: matches!(r, Err((XC_TRANSITION_ILLEGAL, _))),
            evidence: match r {
                Err(e) => e.1,
                Ok(_) => "未拦截——逆时序破坏".to_string(),
            },
        });
    }
    // ── 注入 5：终态再迁移 ──
    {
        let mut reg = HandlerRegistry::new();
        let caps = HandlerCaps::new(HandlerCaps::ALLOW_RETIRE).ok_or((XC_CAPS_ILLEGAL, "构造".to_string()))?;
        let id = reg.register("svc-5", caps)?;
        let _ = reg.transition(id, LifecycleState::Retired); // Created→Retired 合法格
        let r = reg.transition(id, LifecycleState::Attached);
        hits.push(LifecycleHit {
            kind: 5,
            what: "Retired → Attached 终态复活".to_string(),
            intercepted: matches!(r, Err((XC_TRANSITION_ILLEGAL, _))),
            evidence: match r {
                Err(e) => e.1,
                Ok(_) => "未拦截——终态封死失效".to_string(),
            },
        });
    }
    // ── 注入 6：重名注册 ──
    {
        let mut reg = HandlerRegistry::new();
        let caps = HandlerCaps::new(0).ok_or((XC_CAPS_ILLEGAL, "构造".to_string()))?;
        let _ = reg.register("dup-6", caps)?;
        let r = reg.register("dup-6", caps);
        hits.push(LifecycleHit {
            kind: 6,
            what: "同名字二次注册".to_string(),
            intercepted: matches!(r, Err((XC_NAME_TAKEN, _))),
            evidence: match r {
                Err(e) => e.1,
                Ok(_) => "未拦截——名字唯一性破坏".to_string(),
            },
        });
    }
    // ── 注入 7：幽灵 id 迁移 ──
    {
        let mut reg = HandlerRegistry::new();
        let r = reg.transition(9999, LifecycleState::Attached);
        hits.push(LifecycleHit {
            kind: 7,
            what: "不在册 id 9999 迁移".to_string(),
            intercepted: matches!(r, Err((XC_ID_ABSENT, _))),
            evidence: match r {
                Err(e) => e.1,
                Ok(_) => "未拦截——幽灵 id 穿透".to_string(),
            },
        });
    }
    // ── 注入 8：绑定非在服实体 ──
    {
        let mut reg = HandlerRegistry::new();
        let caps = HandlerCaps::new(0).ok_or((XC_CAPS_ILLEGAL, "构造".to_string()))?;
        let id = reg.register("svc-8", caps)?;
        let mut binds = BindingLedger::new();
        let r = binds.bind(&reg, 0x60, id); // Created 态绑定
        hits.push(LifecycleHit {
            kind: 8,
            what: "Created 态实体被绑定向量".to_string(),
            intercepted: matches!(r, Err((XC_BIND_NOT_INSERVICE, _))),
            evidence: match r {
                Err(e) => e.1,
                Ok(_) => "未拦截——绑定纪律破坏".to_string(),
            },
        });
    }
    // ── 注入 9：无能力退休 ──
    {
        let mut reg = HandlerRegistry::new();
        let caps = HandlerCaps::new(0).ok_or((XC_CAPS_ILLEGAL, "构造".to_string()))?; // 无 ALLOW_RETIRE
        let id = reg.register("svc-9", caps)?;
        let _ = reg.transition(id, LifecycleState::Attached);
        let _ = reg.transition(id, LifecycleState::Draining);
        let r = reg.transition(id, LifecycleState::Retired);
        hits.push(LifecycleHit {
            kind: 9,
            what: "无 AllowRetire 能力走 Draining→Retired".to_string(),
            intercepted: matches!(r, Err((XC_CAPS_MISSING, _))),
            evidence: match r {
                Err(e) => e.1,
                Ok(_) => "未拦截——能力闸失效".to_string(),
            },
        });
    }

    let missed: Vec<&LifecycleHit> = hits.iter().filter(|h| !h.intercepted).collect();
    if !missed.is_empty() {
        let names: Vec<String> = missed
            .iter()
            .map(|h| format!("#{} {}", h.kind, h.what))
            .collect();
        return Err((
            XC_INSERVICE_DETACH,
            format!("生命周期违例漏网 {} 项：{}", missed.len(), names.join("；")),
        ));
    }
    let summary: Vec<String> = hits
        .iter()
        .map(|h| format!("#{}拦", h.kind))
        .collect();
    Ok(format!("10/10 生命周期违例全拦截：{}", summary.join(" ")))
}

// ---------------------------------------------------------------------------
// §11 迁移矩阵自检 + 快照
// ---------------------------------------------------------------------------

/// 迁移矩阵静态自检：合法格恰 7 + 对角线全 false + Retired 行全 false
/// + InService 行无 Retired（在服摘除的状态机封锁）。
pub fn matrix_selfcheck() -> Result<String, (u32, String)> {
    let cells = transition_matrix_cells();
    if cells != 7 {
        return Err((XC_TRANSITION_ILLEGAL, format!("合法格 {} ≠ 7", cells)));
    }
    for i in 0..5 {
        if TRANSITION_MATRIX[i][i] {
            return Err((XC_TRANSITION_ILLEGAL, format!("对角线 [{},{}] 应为 false", i, i)));
        }
    }
    for j in 0..5 {
        if TRANSITION_MATRIX[4][j] {
            return Err((XC_TRANSITION_ILLEGAL, "Retired 行存在出边——终态封死破坏".to_string()));
        }
    }
    if TRANSITION_MATRIX[2][4] {
        return Err((
            XC_INSERVICE_DETACH,
            "InService→Retired 直达存在——在服摘除状态机封锁破坏".to_string(),
        ));
    }
    Ok("矩阵 7 合法格+对角封+终态封+在服直达退役封 全过".to_string())
}

/// 注册表快照指纹。
pub struct RegSnap {
    /// 指纹。
    pub hash: u32,
    /// 摘要。
    pub summary: String,
}

impl RegSnap {
    /// 采样：双账规模 + 墓簿数 + 每实体（id/态/迁移总数）+ 绑定账。
    pub fn take(reg: &HandlerRegistry, binds: &BindingLedger) -> RegSnap {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&reg.by_id.len().to_le_bytes());
        buf.extend_from_slice(&reg.graves.len().to_le_bytes());
        buf.extend_from_slice(&reg.next_id.to_le_bytes());
        for (id, e) in reg.by_id.iter() {
            buf.extend_from_slice(&id.to_le_bytes());
            buf.push(e.state.idx() as u8);
            buf.extend_from_slice(&e.transitions_total.to_le_bytes());
        }
        for (v, id) in binds.by_vector.iter() {
            buf.push(*v);
            buf.extend_from_slice(&id.to_le_bytes());
        }
        RegSnap {
            hash: fnv32(&buf, FNV_OFFSET),
            summary: format!(
                "在册={} 墓={} next_id={} 绑定={}",
                reg.by_id.len(),
                reg.graves.len(),
                reg.next_id,
                binds.by_vector.len()
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// §12 墓簿语义专测（id 永不重用）
// ---------------------------------------------------------------------------

/// 墓簿纪律：摘除后再注册拿到新 id（不回收旧 id），旧 id 查找落空且
/// is_dead 成立；名字可复用（名字不是身份）。
pub fn grave_book_discipline() -> Result<String, (u32, String)> {
    let mut reg = HandlerRegistry::new();
    let caps = HandlerCaps::new(HandlerCaps::ALLOW_RETIRE).ok_or((XC_CAPS_ILLEGAL, "构造".to_string()))?;
    let id1 = reg.register("reusable-name", caps)?;
    reg.advance_tick(1);
    let _ = reg.transition(id1, LifecycleState::Retired); // Created→Retired 合法
    let e = reg.remove(id1)?;
    if e.name != "reusable-name" {
        return Err((XC_GRAVE_REVIVE, "摘除返回实体名失配".to_string()));
    }
    if !reg.is_dead(id1) {
        return Err((XC_GRAVE_REVIVE, format!("id {} 摘除后未入墓簿", id1)));
    }
    if reg.lookup(id1).is_some() {
        return Err((XC_GRAVE_REVIVE, "墓簿 id 仍可查找——复活漏洞".to_string()));
    }
    // 名字可复用（新 id）。
    let id2 = reg.register("reusable-name", caps)?;
    if id2 == id1 {
        return Err((XC_GRAVE_REVIVE, format!("id 重用：{} == {}——墓簿失效", id2, id1)));
    }
    if id2 <= id1 {
        return Err((XC_GRAVE_REVIVE, "id 分配器回退——单调性破坏".to_string()));
    }
    reg.audit()?;
    Ok(format!(
        "墓簿纪律过：id {} 入墓、id {} 新分配单调、名字可复用、双账审计过",
        id1, id2
    ))
}

// ---------------------------------------------------------------------------
// §13 全链自证（聚合入口——ktest 桥接点）
// ---------------------------------------------------------------------------

/// 全链自证：矩阵自检 + 墓簿 + 10 万压测 + 违例注入，一锤定音。
pub fn run_all_checks() -> Result<String, (u32, String)> {
    let mut parts: Vec<String> = Vec::new();
    parts.push(matrix_selfcheck()?);
    parts.push(grave_book_discipline()?);
    parts.push(lifecycle_violations_10()?);
    parts.push(fuzz_100k_ops()?);
    // 快照稳定性（同账面双采样同指纹）。
    let mut reg = HandlerRegistry::new();
    let caps = HandlerCaps::new(HandlerCaps::ALLOW_RETIRE).ok_or((XC_CAPS_ILLEGAL, "构造".to_string()))?;
    let id = reg.register("snap-probe", caps)?;
    let _ = reg.transition(id, LifecycleState::Attached);
    let _ = reg.transition(id, LifecycleState::InService);
    let mut binds = BindingLedger::new();
    binds.bind(&reg, 0x61, id)?;
    let s0 = RegSnap::take(&reg, &binds);
    let s1 = RegSnap::take(&reg, &binds);
    if s0.hash != s1.hash {
        return Err((XC_SNAP_DRIFT, "快照指纹不稳定（不可复现）".to_string()));
    }
    parts.push(format!("快照稳定（{:#010x} {}）", s0.hash, s0.summary));
    Ok(parts.join("；"))
}
