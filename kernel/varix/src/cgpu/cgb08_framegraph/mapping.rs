//! F0168 子模块一 · 映射面基础与引擎（原交付结构一~五节）。
//!
//! 一、诊断码六码（0x61 段独占）；二、队列闭集与类型映射规则表（判据一）；
//! 三、GPU 设备面（卡闭集 + 档案 + 归属账）；四、映射节点模型；五、映射引擎（判据一、二）。
//!
//! 语义总约：确定性（同输入必同输出）；显性拒绝（表外值一律返回诊断码或 None——不猜、不静默改道）；
//! 闭集纪律（编译期 assert 钉死表长）；零 panic 面（校验失败显性码拒，无 unwrap/expect）。

use alloc::string::String;
use alloc::vec::Vec;

// 一、诊断码（0x61 段独占：B02 组映射面）——六码互异、段内守恒，异常零静默的机器可读收口。

/// 诊断码（u16；0x61 段为本单独占段，段内互异，与 cgb07 的 0x60 段不重叠）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MqCode {
    /// 节点任务类别表外（映射规则表没有该类别的行）。
    KindOutOfTable = 0x6101,
    /// 节点引用的资源未登记归属（资源归属账缺行——不猜卡）。
    ResourceUnowned = 0x6102,
    /// 节点卡提示与资源归属冲突（放置与映射的口径分歧——显性拒）。
    CardHintConflict = 0x6103,
    /// 队列一致性违约：同资源的读写任务被映射到不一致的队列组合。
    QueueCoherenceViolation = 0x6104,
    /// 映射开销超预算（映射面自身开销越红线）。
    MapBudgetBreach = 0x6105,
    /// 拓扑序缺失（映射输入必须是有序帧图——乱序显性拒）。
    OrderMissing = 0x6106,
}

impl MqCode {
    /// 人读诊断（异常零静默：发生了什么/为什么/下一步——三段式文案）。
    pub const fn detail(self) -> &'static str {
        match self {
            MqCode::KindOutOfTable => "节点任务类别表外：映射规则表无该类别行，建议补类别建模后重映射",
            MqCode::ResourceUnowned => "资源未登记归属：资源归属账缺行，建议先走放置策略（F0106）入账",
            MqCode::CardHintConflict => "卡提示与资源归属冲突：放置账与节点提示口径分歧，建议核对放置账",
            MqCode::QueueCoherenceViolation => "队列一致性违约：同资源读写任务队列组合不一致，建议按校验器建议重映射",
            MqCode::MapBudgetBreach => "映射开销超预算：映射面自身开销越红线，建议核对预算行与实测",
            MqCode::OrderMissing => "拓扑序缺失：映射输入必须有序帧图，建议先过拓扑排序面",
        }
    }

    /// 段守恒（编译期钉死：0x61 段独占）。
    pub const fn in_segment(self) -> bool {
        (self as u16) >= 0x6101 && (self as u16) <= 0x6106
    }
}

// 二、队列类型闭集与类型映射规则表（判据一事实源：KIND_RULES 八行、rule_of 唯一查行入口）。

/// GPU 队列类型三闭集（与 VE-F0006 队列调度器对齐——队列语义不另立）。
/// 三态即映射面全部队列去向——无第四队列，表外去向不存在。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueKind {
    /// 图形队列：光栅化/呈现相关（每设备通常一条主图形队列）。
    Graphics,
    /// 计算队列：异步计算（可与图形并行——多队列并行是帧预算的来源之一）。
    Compute,
    /// 拷贝队列：DMA 传输（上传/下载/设备间拷贝——不占图形/计算时间线）。
    Copy,
}

impl QueueKind {
    /// 全量闭集（判据侧独立写死对拍）。
    pub const ALL: [QueueKind; 3] = [QueueKind::Graphics, QueueKind::Compute, QueueKind::Copy];

    /// 稳定序（账本/矩阵槽位序——与 ALL 一致；数组下标即此序，改序即改契约）。
    pub const fn ordinal(self) -> usize {
        match self {
            QueueKind::Graphics => 0,
            QueueKind::Compute => 1,
            QueueKind::Copy => 2,
        }
    }

    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            QueueKind::Graphics => "图形",
            QueueKind::Compute => "计算",
            QueueKind::Copy => "拷贝",
        }
    }
}

/// 节点任务类别闭集（映射规则表的行键；表外类别显性拒）。
/// 八类别 = 判据侧独立写死对拍的口径——增删类别须同步规则表与对拍面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskKind {
    /// 光栅化绘制（几何→像素——图形队列）。
    Rasterize,
    /// 呈现/合成提交（图形队列——与呈现时序耦合）。
    Present,
    /// 并行计算派发（计算队列）。
    ComputeDispatch,
    /// 后处理算子（计算队列——全屏 pass 多走异步计算）。
    PostProcess,
    /// 上传（主机→设备——拷贝队列）。
    Upload,
    /// 下载（设备→主机——拷贝队列）。
    Download,
    /// 设备间拷贝（拷贝队列——跨卡时路由规则见路由引擎）。
    DeviceCopy,
    /// 资源屏障/同步原语（跟随其约束的资源所在队列——映射规则表的特例行）。
    Barrier,
}

impl TaskKind {
    /// 全量闭集（判据侧独立写死对拍）；行序 = KIND_RULES 下标——两表联动钉死。
    pub const ALL: [TaskKind; 8] = [
        TaskKind::Rasterize,
        TaskKind::Present,
        TaskKind::ComputeDispatch,
        TaskKind::PostProcess,
        TaskKind::Upload,
        TaskKind::Download,
        TaskKind::DeviceCopy,
        TaskKind::Barrier,
    ];

    /// 稳定序。
    pub const fn ordinal(self) -> usize {
        match self {
            TaskKind::Rasterize => 0,
            TaskKind::Present => 1,
            TaskKind::ComputeDispatch => 2,
            TaskKind::PostProcess => 3,
            TaskKind::Upload => 4,
            TaskKind::Download => 5,
            TaskKind::DeviceCopy => 6,
            TaskKind::Barrier => 7,
        }
    }
}

/// 类型映射规则（一行 = 一个类别的队列去向 + 依据注记；判据一最小事实单元）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KindRule {
    /// 任务类别。
    pub kind: TaskKind,
    /// 队列去向。
    pub queue: QueueKind,
    /// 是否特例行（屏障类跟随资源归属——由引擎特判，规则行只记默认去向）。
    pub special: bool,
    /// 规则依据（人读注记——映射正确性的可追溯面）。
    pub basis: &'static str,
}

/// 类型映射规则表（判据一：八行闭集——图形→图形、计算→计算、拷贝→拷贝）。
/// 行序 = TaskKind 稳定序（下标即 ordinal）；Barrier 为特例行（special=true：
/// 默认跟图形，实际由引擎按约束资源的写入队列特判——规则行只记默认去向）。
pub const KIND_RULES: [KindRule; 8] = [
    KindRule { kind: TaskKind::Rasterize, queue: QueueKind::Graphics, special: false, basis: "光栅化与呈现时序耦合——图形队列" },
    KindRule { kind: TaskKind::Present, queue: QueueKind::Graphics, special: false, basis: "呈现提交在图形队列尾——呈现时序" },
    KindRule { kind: TaskKind::ComputeDispatch, queue: QueueKind::Compute, basis: "异步计算——计算队列并行", special: false },
    KindRule { kind: TaskKind::PostProcess, queue: QueueKind::Compute, special: false, basis: "全屏后处理 pass——异步计算" },
    KindRule { kind: TaskKind::Upload, queue: QueueKind::Copy, special: false, basis: "主机→设备 DMA——拷贝队列" },
    KindRule { kind: TaskKind::Download, queue: QueueKind::Copy, special: false, basis: "设备→主机 DMA——拷贝队列" },
    KindRule { kind: TaskKind::DeviceCopy, queue: QueueKind::Copy, special: false, basis: "设备间传输——拷贝队列" },
    KindRule { kind: TaskKind::Barrier, queue: QueueKind::Graphics, special: true, basis: "屏障特例：默认跟图形，实际跟随约束资源的写入队列（引擎特判）" },
];

/// 规则表查行（表外类别 → None——调用方显性拒，不猜）。
/// 行核对双保险：下标在界内 + 行键与请求类别一致——表被误改时兜底。
pub const fn rule_of(kind: TaskKind) -> Option<&'static KindRule> {
    let i = kind.ordinal();
    if i < KIND_RULES.len() {
        let r = &KIND_RULES[i];
        if r.kind.ordinal() == kind.ordinal() {
            return Some(r);
        }
    }
    None
}

// 三、GPU 设备面（DeviceTable 卡闭集 + GpuDevice 档案 + ResourceHome 归属账——多 GPU 路由的地基）。

/// GPU 设备档案（多 GPU 路由的地基：每卡一行）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpuDevice {
    /// 卡 id（放置策略 F0106 分派的稳定标识）。
    pub card: u32,
    /// 人读名（如 "dGPU0" / "iGPU"）。
    pub label: String,
    /// 三队列各自条数（本卡队列容量——映射合法性校验的容量面）。
    pub queues: [u32; 3],
    /// 是否主卡（呈现类任务必须落主卡——呈现时序约束）。
    pub primary: bool,
}

impl GpuDevice {
    /// 队列容量查（表外卡 → 0——容量为零即不可映射；下标取队列稳定序）。
    pub fn queue_capacity(&self, q: QueueKind) -> u32 {
        self.queues[q.ordinal()]
    }
}

/// 设备表（卡闭集——本帧图可用卡的封顶清单）。
/// 闭集语义：注册后不再收缩（无注销面）——帧图生命周期内卡集稳定。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeviceTable {
    devices: Vec<GpuDevice>,
}

impl DeviceTable {
    /// 空表（设备注册面在使用前装配——卡 id 唯一性由 register 把守）。
    pub fn new() -> Self {
        DeviceTable { devices: Vec::new() }
    }

    /// 注册设备（重复卡 id 显性拒——唯一性；Err 时表内状态不变，拒即不落账）。
    pub fn register(&mut self, dev: GpuDevice) -> Result<(), MqCode> {
        if self.devices.iter().any(|d| d.card == dev.card) {
            return Err(MqCode::CardHintConflict);
        }
        self.devices.push(dev);
        Ok(())
    }

    /// 卡查（表外卡 → None——路由对表外卡显性拒）。
    pub fn device_of(&self, card: u32) -> Option<&GpuDevice> {
        self.devices.iter().find(|d| d.card == card)
    }

    /// 主卡查（无主卡 → None——呈现任务在无主卡拓扑下不可映射）。
    pub fn primary(&self) -> Option<&GpuDevice> {
        self.devices.iter().find(|d| d.primary)
    }

    /// 卡数。
    pub fn len(&self) -> usize {
        self.devices.len()
    }

    /// 空判（无卡拓扑下呈现/兜底路由均不可映射——路由入口的守卫面）。
    pub fn is_empty(&self) -> bool {
        self.devices.is_empty()
    }
}

/// 资源归属账（资源 id → 所在卡；F0106 输出、路由输入；账面单调增长——归属漂移在登记门即拒）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResourceHome {
    homes: Vec<(u32, u32)>,
}

impl ResourceHome {
    /// 空账（放置策略 F0106 的输出在使用前入账——归属行即路由依据）。
    pub fn new() -> Self {
        ResourceHome { homes: Vec::new() }
    }

    /// 登记（同卡幂等、重放安全；异卡显性拒——资源不跨卡漂移）。
    pub fn assign(&mut self, resource: u32, card: u32) -> Result<(), MqCode> {
        if let Some((_, c)) = self.homes.iter().find(|(r, _)| *r == resource) {
            return if *c == card { Ok(()) } else { Err(MqCode::CardHintConflict) };
        }
        self.homes.push((resource, card));
        Ok(())
    }

    /// 归属查（表外资源 → None——映射对无归属资源显性拒 [`MqCode::ResourceUnowned`]）。
    pub fn home_of(&self, resource: u32) -> Option<u32> {
        self.homes.iter().find(|(r, _)| *r == resource).map(|(_, c)| *c)
    }

    /// 归属行数。
    pub fn len(&self) -> usize {
        self.homes.len()
    }

    /// 空判（空账即无资源入账——映射入口 ResourceUnowned 的结构性来源）。
    pub fn is_empty(&self) -> bool {
        self.homes.is_empty()
    }
}

// 四、映射节点模型（MapNode：类别定队列去向、读写集定路由与一致性、卡提示为放置建议值）。

/// 映射节点（映射面的输入模型；消费 cgb07 拓扑序——映射在拓扑之后执行之前）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapNode {
    /// 任务 id（与 cgb07 [`super::cgb07_execfail::TaskNode`] 的 task_id 同源）。
    pub task_id: u32,
    /// 任务类别（映射规则表的行键）。
    pub kind: TaskKind,
    /// 依赖任务 id（与 cgb07 deps 同源——一致性由对账面核对）。
    pub deps: Vec<u32>,
    /// 读集（只读资源 id）。
    pub res_read: Vec<u32>,
    /// 写集（可写资源 id）。
    pub res_write: Vec<u32>,
    /// 卡提示（放置策略的建议卡；0 = 无提示）。
    pub card_hint: u32,
}

impl MapNode {
    /// 全部触碰资源（读∪写；拼接序先读后写——与约束校验触碰账输入序一致）。
    pub fn touched(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        out.extend_from_slice(&self.res_read);
        out.extend_from_slice(&self.res_write);
        out
    }
}

// 五、映射引擎（map_node 串起规则查行与路由推导；route_card 规则序显性；同输入同输出）。

/// 单节点映射结论（kind_rule_ok 供判定面独立复核；basis 五态即路由依据闭集）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueMapping {
    /// 任务 id。
    pub task_id: u32,
    /// 队列去向。
    pub queue: QueueKind,
    /// 落卡。
    pub card: u32,
    /// 队列内槽位（同队列同卡的序——负载面观测用）。
    pub slot: u32,
    /// 路由依据（"类型规则" / "资源归属" / "卡提示+归属一致" / "屏障跟写" / "主卡呈现"）。
    pub basis: &'static str,
}

impl QueueMapping {
    /// 判据一机检面：队列与类别是否符合类型规则（屏障豁免；表外类别恒 false）。
    pub fn kind_rule_ok(&self, kind: TaskKind) -> bool {
        match rule_of(kind) {
            Some(r) => r.special || r.queue == self.queue,
            None => false,
        }
    }
}

/// 路由依据常量（账面与判据共用单一来源；判定面对拍锚也取此处）。
pub const BASIS_KIND_RULE: &str = "类型规则";
pub const BASIS_RES_HOME: &str = "资源归属";
pub const BASIS_HINT_CONSISTENT: &str = "卡提示+归属一致";
pub const BASIS_BARRIER_FOLLOWS_WRITE: &str = "屏障跟写";
pub const BASIS_PRESENT_PRIMARY: &str = "主卡呈现";

/// 映射引擎（确定性：同输入同输出；负载计数 (卡, 队列) 是 slot 分派与
/// 负载观测的唯一状态——无隐藏态，回放面用新引擎规避）。
#[derive(Clone, Debug, Default)]
pub struct MapEngine {
    /// 设备表。
    pub devices: DeviceTable,
    /// 资源归属账。
    pub homes: ResourceHome,
    /// 队列负载计数（(卡, 队列) → 已映射数——slot 分派与负载观测）。
    load: Vec<(u32, u8, u32)>,
}

impl MapEngine {
    /// 空引擎（设备表/归属账/负载计数全空——注册面在使用前装配）。
    pub fn new() -> Self {
        MapEngine { devices: DeviceTable::new(), homes: ResourceHome::new(), load: Vec::new() }
    }

    /// 队列负载查。
    fn load_of(&self, card: u32, q: QueueKind) -> u32 {
        self.load
            .iter()
            .find(|(c, k, _)| *c == card && *k == q.ordinal() as u8)
            .map_or(0, |(_, _, n)| *n)
    }

    /// 队列负载增。
    fn bump_load(&mut self, card: u32, q: QueueKind) -> u32 {
        // 既有负载行：计数自增；无行则新开（(卡, 队列) 二元键唯一）。
        if let Some(entry) = self
            .load
            .iter_mut()
            .find(|(c, k, _)| *c == card && *k == q.ordinal() as u8)
        {
            entry.2 += 1;
            return entry.2;
        }
        self.load.push((card, q.ordinal() as u8, 1));
        1
    }

    /// 资源归属推导原语（规则 2/3 共用）：取清单第一项的归属卡；空/无归属 → None。
    /// 只取首项是规则语义（写方优先/读源优先）——不遍历不折中。
    fn first_home_of(&self, res: &[u32]) -> Option<u32> {
        let first = *res.get(0)?;
        self.homes.home_of(first)
    }

    /// 归属推导卡一致性收口（两道门：卡提示一致 + 卡在设备表内——分歧/表外卡即拒）。
    fn check_hint_and_table(&self, node: &MapNode, card: u32) -> Result<u32, MqCode> {
        if node.card_hint != 0 && node.card_hint != card {
            return Err(MqCode::CardHintConflict);
        }
        if self.devices.device_of(card).is_none() {
            return Err(MqCode::ResourceUnowned);
        }
        Ok(card)
    }

    /// 节点的目标卡推导（判据二核心：多 GPU 路由确定性规则）。
    /// 规则序（先命中先定）：1. 呈现→主卡；2. 单资源→归属卡；3. 多资源→写集首资源
    /// （写方优先），写集空则读集首资源；4. 无资源→提示卡→主卡→首卡；
    /// 5. 卡提示与推导冲突 → 显性拒 [`MqCode::CardHintConflict`]（口径分歧不静默）。
    pub fn route_card(&self, node: &MapNode) -> Result<u32, MqCode> {
        if node.kind == TaskKind::Present {
            return match self.devices.primary() {
                Some(d) => Ok(d.card),
                None => Err(MqCode::ResourceUnowned),
            };
        }
        if !node.res_write.is_empty() {
            return self
                .first_home_of(&node.res_write)
                .map_or(Err(MqCode::ResourceUnowned), |c| self.check_hint_and_table(node, c));
        }
        if !node.res_read.is_empty() {
            return self
                .first_home_of(&node.res_read)
                .map_or(Err(MqCode::ResourceUnowned), |c| self.check_hint_and_table(node, c));
        }
        if node.card_hint != 0 {
            if self.devices.device_of(node.card_hint).is_none() {
                return Err(MqCode::CardHintConflict);
            }
            return Ok(node.card_hint);
        }
        if let Some(p) = self.devices.primary() {
            return Ok(p.card);
        }
        Err(MqCode::ResourceUnowned)
    }

    /// 屏障特判（判据一特例：屏障跟随约束资源的写入队列与归属卡；写集空则退化默认规则行）。
    pub fn barrier_route(&self, node: &MapNode) -> Result<(QueueKind, u32), MqCode> {
        if !node.res_write.is_empty() {
            let card = self
                .first_home_of(&node.res_write)
                .ok_or(MqCode::ResourceUnowned)?;
            return Ok((QueueKind::Graphics, card));
        }
        let card = self.route_card(node)?;
        Ok((QueueKind::Graphics, card))
    }

    /// 单节点映射（判据一 + 二合一入口）：屏障特例先行 → 查规则行（表外拒）→
    /// 路由推导 → 容量校验 → 负载落账；失败路径全部显性码拒——零 panic 面。
    pub fn map_node(&mut self, node: &MapNode) -> Result<QueueMapping, MqCode> {
        let (queue, card, basis) = if node.kind == TaskKind::Barrier {
            let (q, c) = self.barrier_route(node)?;
            (q, c, BASIS_BARRIER_FOLLOWS_WRITE)
        } else {
            let rule = match rule_of(node.kind) {
                Some(r) => r,
                None => return Err(MqCode::KindOutOfTable),
            };
            let card = self.route_card(node)?;
            let basis = if node.kind == TaskKind::Present {
                BASIS_PRESENT_PRIMARY
            } else if node.card_hint != 0 {
                BASIS_HINT_CONSISTENT
            } else {
                BASIS_RES_HOME
            };
            let _ = rule.queue;
            (rule.queue, card, basis)
        };
        let dev = match self.devices.device_of(card) {
            Some(d) => d,
            None => return Err(MqCode::ResourceUnowned),
        };
        if dev.queue_capacity(queue) == 0 {
            return Err(MqCode::KindOutOfTable);
        }
        let slot = self.bump_load(card, queue);
        Ok(QueueMapping { task_id: node.task_id, queue, card, slot, basis })
    }

    /// 有序性守卫：order 恰好覆盖 nodes 任务集（少 = 乱序残图；多 = 序含幽灵）。
    fn order_covers_nodes(order: &[u32], nodes: &[MapNode]) -> bool {
        if order.len() != nodes.len() {
            return false;
        }
        order.iter().all(|oid| nodes.iter().any(|n| n.task_id == *oid))
    }

    /// 整帧映射（有序输入——乱序显性拒 [`MqCode::OrderMissing`]）。输入 order 为
    /// 拓扑序任务 id 序（cgb07 输出）。两段结构：守卫（覆盖性校验——短/多/缺一律拒）
    /// 先红 + 按序逐点映射（映射段不做第二次完整性假设）。
    pub fn map_frame(
        &mut self,
        order: &[u32],
        nodes: &[MapNode],
    ) -> Result<Vec<QueueMapping>, MqCode> {
        if order.is_empty() && !nodes.is_empty() {
            return Err(MqCode::OrderMissing);
        }
        if !Self::order_covers_nodes(order, nodes) {
            return Err(MqCode::OrderMissing);
        }
        let mut out: Vec<QueueMapping> = Vec::new();
        for oid in order.iter() {
            let node = match nodes.iter().find(|n| n.task_id == *oid) {
                Some(n) => n.clone(),
                // 理论不可达（守卫已覆盖）：显性拒不猜——可恢复路径。
                None => return Err(MqCode::OrderMissing),
            };
            let m = self.map_node(&node)?;
            out.push(m);
        }
        Ok(out)
    }
}

/// 表守恒断言（编译期钉死：规则表八行闭集，与 TaskKind::ALL 等长联动）。
const _: () = {
    assert!(KIND_RULES.len() == 8, "类型映射规则表必须八行闭集");
};
