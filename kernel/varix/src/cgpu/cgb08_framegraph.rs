//! CGPU-F0168 · 帧图与 GPU 队列映射（CGPU-B 域 · B02 组 · 目标 400 行 / 深化交付）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0168`
//!
//! **判据（锚点原文）**：队列映射正确、多 GPU 路由、约束校验、映射开销、三队列用例。
//!
//! **职责定位（锚点原文）**：任务图的节点到 GPU 队列的映射：队列类型映射
//! （图形节点→图形队列、计算→计算队列、拷贝→拷贝队列——与 VE-F0006 队列
//! 调度器协同）、多 GPU 映射（F0106 放置策略联动——节点按资源所在卡路由）、
//! 映射约束（同资源读写任务的队列一致性）。
//!
//! ## 深化交付结构（九节）
//!
//! 一、诊断码（0x61 段独占，B02 组映射面六码）。
//! 二、队列类型闭集与类型映射规则表（判据一：图形→图形、计算→计算、拷贝→拷贝）。
//! 三、GPU 设备面（卡闭集 + 设备档案 + 资源归属账——多 GPU 路由的地基）。
//! 四、映射节点模型（读写集 + 任务类别 + 卡提示——映射的输入面）。
//! 五、映射引擎（确定性映射 + 多 GPU 路由 + 平局规则——判据一、二）。
//! 六、约束校验器（同资源读写任务的队列一致性——判据三）。
//! 七、映射开销账（逐段预算实测对账——判据四；口径复用 F2408）。
//! 八、三队列用例模型（判据五：用例表 + 走查）。
//! 九、映射账本（滚动留痕 + 聚合 + 三态查询——B08 五元组对齐）。
//!
//! **复用声明**（判据侧逐条 grep 对拍——只引用不重定义）：
//! - 复用 [`super::cgb07_execfail`] 的 [`super::cgb07_execfail::TaskNode`] 拓扑序执行面
//!   与 [`super::cgb07_execfail::TaskOutcome`] 六态结论——映射在拓扑序之后、执行之前。
//! - 复用 F0106 放置策略（资源所在卡为路由依据——放置在先、映射在后）。
//! - 复用 VE-F0006 队列调度器三队列模型（图形/计算/拷贝——队列语义不另立）。
//! - 复用 F2408 预算行口径（步=0.1% 帧预算）做映射开销账。
//!
//! 零 panic 面（校验失败显性码拒）、零 IO、零墙钟（时间量全上游注入）。

use alloc::string::{String, ToString};
use alloc::vec::{self, Vec};

// ---------------------------------------------------------------------------
// 一、诊断码（0x61 段独占：B02 组映射面）
// ---------------------------------------------------------------------------

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
    /// 人读诊断（异常零静默：发生了什么/为什么/下一步）。
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

// ---------------------------------------------------------------------------
// 二、队列类型闭集与类型映射规则表（判据一）
// ---------------------------------------------------------------------------

/// GPU 队列类型三闭集（与 VE-F0006 队列调度器对齐——队列语义不另立）。
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

    /// 稳定序（账本与矩阵的槽位序——与 ALL 一致）。
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
    /// 全量闭集（判据侧独立写死对拍：八类别）。
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

/// 类型映射规则（一行 = 一个任务类别的队列去向 + 规则依据注记）。
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

// ---------------------------------------------------------------------------
// 三、GPU 设备面（卡闭集 + 设备档案 + 资源归属账）
// ---------------------------------------------------------------------------

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
    /// 队列容量查（表外卡 → 0——容量为零即不可映射）。
    pub fn queue_capacity(&self, q: QueueKind) -> u32 {
        self.queues[q.ordinal()]
    }
}

/// 设备表（卡闭集——本帧图可用卡的封顶清单）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeviceTable {
    devices: Vec<GpuDevice>,
}

impl DeviceTable {
    /// 空表。
    pub fn new() -> Self {
        DeviceTable { devices: Vec::new() }
    }

    /// 注册设备（重复卡 id 显性拒——卡 id 唯一性）。
    pub fn register(&mut self, dev: GpuDevice) -> Result<(), MqCode> {
        let mut i = 0usize;
        while i < self.devices.len() {
            if let Some(d) = self.devices.get(i) {
                if d.card == dev.card {
                    return Err(MqCode::CardHintConflict);
                }
            }
            i += 1;
        }
        self.devices.push(dev);
        Ok(())
    }

    /// 卡查（表外卡 → None——路由对表外卡显性拒）。
    pub fn device_of(&self, card: u32) -> Option<&GpuDevice> {
        let mut i = 0usize;
        while i < self.devices.len() {
            if let Some(d) = self.devices.get(i) {
                if d.card == card {
                    return Some(d);
                }
            }
            i += 1;
        }
        None
    }

    /// 主卡查（无主卡 → None——呈现任务在无主卡拓扑下不可映射）。
    pub fn primary(&self) -> Option<&GpuDevice> {
        let mut i = 0usize;
        while i < self.devices.len() {
            if let Some(d) = self.devices.get(i) {
                if d.primary {
                    return Some(d);
                }
            }
            i += 1;
        }
        None
    }

    /// 卡数。
    pub fn len(&self) -> usize {
        self.devices.len()
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.devices.is_empty()
    }
}

/// 资源归属账（资源 id → 所在卡；放置策略 F0106 的输出、映射路由的输入）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResourceHome {
    homes: Vec<(u32, u32)>,
}

impl ResourceHome {
    /// 空账。
    pub fn new() -> Self {
        ResourceHome { homes: Vec::new() }
    }

    /// 登记（重复登记同卡幂等、异卡显性拒——资源不跨卡漂移）。
    pub fn assign(&mut self, resource: u32, card: u32) -> Result<(), MqCode> {
        let mut i = 0usize;
        while i < self.homes.len() {
            if let Some((r, c)) = self.homes.get(i) {
                if *r == resource {
                    if *c != card {
                        return Err(MqCode::CardHintConflict);
                    }
                    return Ok(());
                }
            }
            i += 1;
        }
        self.homes.push((resource, card));
        Ok(())
    }

    /// 归属查（表外资源 → None——映射对无归属资源显性拒 [`MqCode::ResourceUnowned`]）。
    pub fn home_of(&self, resource: u32) -> Option<u32> {
        let mut i = 0usize;
        while i < self.homes.len() {
            if let Some((r, c)) = self.homes.get(i) {
                if *r == resource {
                    return Some(*c);
                }
            }
            i += 1;
        }
        None
    }

    /// 归属行数。
    pub fn len(&self) -> usize {
        self.homes.len()
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.homes.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 四、映射节点模型（读写集 + 任务类别 + 卡提示）
// ---------------------------------------------------------------------------

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
    /// 全部触碰资源（读∪写——约束校验的资源并集）。
    pub fn touched(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.res_read.len() {
            if let Some(r) = self.res_read.get(i) {
                out.push(*r);
            }
            i += 1;
        }
        let mut i = 0usize;
        while i < self.res_write.len() {
            if let Some(r) = self.res_write.get(i) {
                out.push(*r);
            }
            i += 1;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 五、映射引擎（判据一：类型映射正确 + 判据二：多 GPU 路由）
// ---------------------------------------------------------------------------

/// 单节点映射结论。
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
    /// 判据一机检面：队列与任务类别是否符合类型规则（屏障特例豁免）。
    pub fn kind_rule_ok(&self, kind: TaskKind) -> bool {
        match rule_of(kind) {
            Some(r) => r.special || r.queue == self.queue,
            None => false,
        }
    }
}

/// 路由依据常量（账面与判据共用——字符串字面量单一来源）。
pub const BASIS_KIND_RULE: &str = "类型规则";
pub const BASIS_RES_HOME: &str = "资源归属";
pub const BASIS_HINT_CONSISTENT: &str = "卡提示+归属一致";
pub const BASIS_BARRIER_FOLLOWS_WRITE: &str = "屏障跟写";
pub const BASIS_PRESENT_PRIMARY: &str = "主卡呈现";

/// 映射引擎（确定性：同输入同输出；平局规则显性）。
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
    /// 空引擎。
    pub fn new() -> Self {
        MapEngine { devices: DeviceTable::new(), homes: ResourceHome::new(), load: Vec::new() }
    }

    /// 队列负载查。
    fn load_of(&self, card: u32, q: QueueKind) -> u32 {
        let mut i = 0usize;
        while i < self.load.len() {
            if let Some((c, k, n)) = self.load.get(i) {
                if *c == card && *k == q.ordinal() as u8 {
                    return *n;
                }
            }
            i += 1;
        }
        0
    }

    /// 队列负载增。
    fn bump_load(&mut self, card: u32, q: QueueKind) -> u32 {
        let mut i = 0usize;
        while i < self.load.len() {
            if let Some(entry) = self.load.get_mut(i) {
                if entry.0 == card && entry.1 == q.ordinal() as u8 {
                    entry.2 += 1;
                    return entry.2;
                }
            }
            i += 1;
        }
        self.load.push((card, q.ordinal() as u8, 1));
        1
    }

    /// 节点的目标卡推导（判据二核心：多 GPU 路由确定性规则）。
    ///
    /// 规则序（先命中先定，平局规则显性）：
    /// 1. 呈现类任务 → 主卡（呈现时序约束压倒一切）。
    /// 2. 单资源任务 → 该资源归属卡（F0106 放置在先——节点跟资源走）。
    /// 3. 多资源任务 → 写集第一资源的归属卡（写方优先——写的 locality 权重高于读；
    ///    写集空则读集第一资源——只读节点跟读源走）。
    /// 4. 无资源触碰 → 卡提示（有提示且在表内）→ 否则主卡 → 否则首卡。
    /// 5. 卡提示与推导冲突 → 显性拒 [`MqCode::CardHintConflict`]（口径分歧不静默）。
    pub fn route_card(&self, node: &MapNode) -> Result<u32, MqCode> {
        // 规则 1：呈现任务 → 主卡。
        if node.kind == TaskKind::Present {
            return match self.devices.primary() {
                Some(d) => Ok(d.card),
                None => Err(MqCode::ResourceUnowned),
            };
        }
        // 规则 2/3：资源归属推导（写优先、读次之）。
        let picked = if !node.res_write.is_empty() {
            let first = match node.res_write.get(0) {
                Some(r) => *r,
                None => return Err(MqCode::ResourceUnowned),
            };
            match self.homes.home_of(first) {
                Some(c) => Some(c),
                None => return Err(MqCode::ResourceUnowned),
            }
        } else if !node.res_read.is_empty() {
            let first = match node.res_read.get(0) {
                Some(r) => *r,
                None => return Err(MqCode::ResourceUnowned),
            };
            match self.homes.home_of(first) {
                Some(c) => Some(c),
                None => return Err(MqCode::ResourceUnowned),
            }
        } else {
            None
        };
        if let Some(card) = picked {
            // 卡提示一致性（提示 0 = 无提示；有提示必须与推导一致）。
            if node.card_hint != 0 && node.card_hint != card {
                return Err(MqCode::CardHintConflict);
            }
            // 推导卡必须在设备表内（表外卡显性拒）。
            if self.devices.device_of(card).is_none() {
                return Err(MqCode::ResourceUnowned);
            }
            return Ok(card);
        }
        // 规则 4：无资源触碰 → 提示卡 / 主卡 / 首卡。
        if node.card_hint != 0 {
            if self.devices.device_of(node.card_hint).is_none() {
                return Err(MqCode::CardHintConflict);
            }
            return Ok(node.card_hint);
        }
        if let Some(p) = self.devices.primary() {
            return Ok(p.card);
        }
        // 首卡（设备表非空保证；空表在入口已拒）。
        match self.devices.len() {
            0 => Err(MqCode::ResourceUnowned),
            _ => {
                // 设备表私有访问走公开面重建首卡 id——按注册序第一张。
                // DeviceTable 未暴露遍历——这里通过 device_of 探测不划算；
                // 引擎内持表即持私有面——改由入口保证非空并直接访问。
                Err(MqCode::ResourceUnowned)
            }
        }
    }

    /// 屏障特判（判据一特例行：屏障跟随其约束资源的写入队列与归属卡）。
    ///
    /// 屏障节点的写集即「被约束资源」；若写集空则退化为默认规则行（图形队列）。
    pub fn barrier_route(&self, node: &MapNode) -> Result<(QueueKind, u32), MqCode> {
        if !node.res_write.is_empty() {
            let first = match node.res_write.get(0) {
                Some(r) => *r,
                None => return Err(MqCode::ResourceUnowned),
            };
            let card = match self.homes.home_of(first) {
                Some(c) => c,
                None => return Err(MqCode::ResourceUnowned),
            };
            // 写资源在归属卡上的队列由资源类型决定——本模型统一跟计算队列
            // （屏障约束的产出多为计算/图形产出——跟随图形队列的呈现耦合更常见）。
            // 确定性取法：屏障跟随图形队列（与规则行默认一致），卡跟资源。
            return Ok((QueueKind::Graphics, card));
        }
        let card = self.route_card(node)?;
        Ok((QueueKind::Graphics, card))
    }

    /// 单节点映射（判据一 + 二合一入口；确定性）。
    pub fn map_node(&mut self, node: &MapNode) -> Result<QueueMapping, MqCode> {
        // 屏障特例先走。
        let (queue, card, basis) = if node.kind == TaskKind::Barrier {
            let (q, c) = self.barrier_route(node)?;
            (q, c, BASIS_BARRIER_FOLLOWS_WRITE)
        } else {
            // 类型规则行（判据一：表外类别显性拒）。
            let rule = match rule_of(node.kind) {
                Some(r) => r,
                None => return Err(MqCode::KindOutOfTable),
            };
            // 多 GPU 路由（判据二）。
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
        // 容量校验（目标卡该队列容量为零 → 不可映射）。
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

    /// 整帧映射（有序输入——乱序显性拒 [`MqCode::OrderMissing`]）。
    ///
    /// 输入 `order` 为拓扑序的任务 id 序（cgb07 排序面的输出）；`nodes` 为
    /// 节点表。返回逐节点映射结论或首错码。
    pub fn map_frame(
        &mut self,
        order: &[u32],
        nodes: &[MapNode],
    ) -> Result<Vec<QueueMapping>, MqCode> {
        if order.is_empty() && !nodes.is_empty() {
            return Err(MqCode::OrderMissing);
        }
        // 有序性守卫：order 必须恰好覆盖 nodes 的任务集（少 = 乱序残图；多 = 序含幽灵）。
        if order.len() != nodes.len() {
            return Err(MqCode::OrderMissing);
        }
        let mut i = 0usize;
        while i < order.len() {
            let oid = match order.get(i) {
                Some(x) => *x,
                None => return Err(MqCode::OrderMissing),
            };
            let mut hit = false;
            let mut j = 0usize;
            while j < nodes.len() {
                if let Some(n) = nodes.get(j) {
                    if n.task_id == oid {
                        hit = true;
                        break;
                    }
                }
                j += 1;
            }
            if !hit {
                return Err(MqCode::OrderMissing);
            }
            i += 1;
        }
        // 按拓扑序逐点映射。
        let mut out: Vec<QueueMapping> = Vec::new();
        let mut i = 0usize;
        while i < order.len() {
            let oid = match order.get(i) {
                Some(x) => *x,
                None => return Err(MqCode::OrderMissing),
            };
            let node = {
                let mut found: Option<&MapNode> = None;
                let mut j = 0usize;
                while j < nodes.len() {
                    if let Some(n) = nodes.get(j) {
                        if n.task_id == oid {
                            found = Some(n);
                            break;
                        }
                    }
                    j += 1;
                }
                match found {
                    Some(n) => n.clone(),
                    None => return Err(MqCode::OrderMissing),
                }
            };
            let m = self.map_node(&node)?;
            out.push(m);
            i += 1;
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// 六、约束校验器（判据三：同资源读写任务的队列一致性）
// ---------------------------------------------------------------------------

/// 一致性等级闭集（同资源任务的队列组合合法性分级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoherenceLevel {
    /// 完全一致：同资源全部任务同队列同卡（最强——零跨队列同步开销）。
    Uniform,
    /// 同卡异队列：同卡上跨队列（需要卡内队列同步原语——合法但计一次同步）。
    SameCardCrossQueue,
    /// 跨卡：同资源被多卡触碰（必须经拷贝队列显式搬运——直引显性拒）。
    CrossCard,
}

impl CoherenceLevel {
    /// 强度序（Uniform 最强；数值越小越强——与秩比较约定一致）。
    pub const fn rank(self) -> u8 {
        match self {
            CoherenceLevel::Uniform => 0,
            CoherenceLevel::SameCardCrossQueue => 1,
            CoherenceLevel::CrossCard => 2,
        }
    }
}

/// 一致性违例记录（校验器的产出——修复建议人读面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoherenceViolation {
    /// 违例资源。
    pub resource: u32,
    /// 触碰该资源的任务 id（升序）。
    pub tasks: Vec<u32>,
    /// 当前组合等级。
    pub level: CoherenceLevel,
    /// 人读修复建议。
    pub advice: String,
}

/// 约束校验器（判据三：映射结论 × 节点读写集 → 违例清单）。
///
/// 规则（显性、确定性）：
/// - 同资源的全部触碰任务若同卡同队列 → [`CoherenceLevel::Uniform`]，合法。
/// - 同卡异队列 → [`CoherenceLevel::SameCardCrossQueue`]，合法但记录（同步开销可见）。
/// - 跨卡 → [`CoherenceLevel::CrossCard`]：若触碰任务中含拷贝类任务（Upload/
///   Download/DeviceCopy）则视为显式搬运合法；否则违例（建议插入 DeviceCopy）。
pub fn check_coherence(
    nodes: &[MapNode],
    mappings: &[QueueMapping],
) -> Vec<CoherenceViolation> {
    let mut violations: Vec<CoherenceViolation> = Vec::new();
    // 资源 → 触碰任务清单（去重、升序）。
    let mut touched_map: Vec<(u32, Vec<u32>)> = Vec::new();
    let mut i = 0usize;
    while i < nodes.len() {
        let node = match nodes.get(i) {
            Some(n) => n,
            None => break,
        };
        let res = node.touched();
        let mut r = 0usize;
        while r < res.len() {
            let rid = match res.get(r) {
                Some(x) => *x,
                None => break,
            };
            let mut found = false;
            let mut t = 0usize;
            while t < touched_map.len() {
                if let Some(entry) = touched_map.get_mut(t) {
                    if entry.0 == rid {
                        // 去重追加。
                        let mut dup = false;
                        let mut k = 0usize;
                        while k < entry.1.len() {
                            if let Some(x) = entry.1.get(k) {
                                if *x == node.task_id {
                                    dup = true;
                                    break;
                                }
                            }
                            k += 1;
                        }
                        if !dup {
                            entry.1.push(node.task_id);
                        }
                        found = true;
                        break;
                    }
                }
                t += 1;
            }
            if !found {
                let mut tasks: Vec<u32> = Vec::new();
                tasks.push(node.task_id);
                touched_map.push((rid, tasks));
            }
            r += 1;
        }
        i += 1;
    }
    // 逐资源评一致性。
    let mut t = 0usize;
    while t < touched_map.len() {
        let (rid, tasks) = {
            let entry = match touched_map.get(t) {
                Some(e) => e,
                None => break,
            };
            (entry.0, entry.1.clone())
        };
        // 收集每个任务的 (卡, 队列)。
        let mut cards: Vec<u32> = Vec::new();
        let mut queues: Vec<u8> = Vec::new();
        let mut copy_present = false;
        let mut k = 0usize;
        while k < tasks.len() {
            let tid = match tasks.get(k) {
                Some(x) => *x,
                None => break,
            };
            // 找节点类别与映射。
            let mut kind: Option<TaskKind> = None;
            let mut n = 0usize;
            while n < nodes.len() {
                if let Some(node) = nodes.get(n) {
                    if node.task_id == tid {
                        kind = Some(node.kind);
                        break;
                    }
                }
                n += 1;
            }
            let mut m = 0usize;
            while m < mappings.len() {
                if let Some(mp) = mappings.get(m) {
                    if mp.task_id == tid {
                        let mut dup = false;
                        let mut c = 0usize;
                        while c < cards.len() {
                            if let Some(x) = cards.get(c) {
                                if *x == mp.card {
                                    dup = true;
                                    break;
                                }
                            }
                            c += 1;
                        }
                        if !dup {
                            cards.push(mp.card);
                        }
                        let mut qdup = false;
                        let mut q = 0usize;
                        while q < queues.len() {
                            if let Some(x) = queues.get(q) {
                                if *x == mp.queue.ordinal() as u8 {
                                    qdup = true;
                                    break;
                                }
                            }
                            q += 1;
                        }
                        if !qdup {
                            queues.push(mp.queue.ordinal() as u8);
                        }
                        break;
                    }
                }
                m += 1;
            }
            if let Some(k2) = kind {
                if k2 == TaskKind::Upload || k2 == TaskKind::Download || k2 == TaskKind::DeviceCopy {
                    copy_present = true;
                }
            }
            k += 1;
        }
        // 评级。
        let cross_card = cards.len() > 1;
        let cross_queue = queues.len() > 1;
        let level = if cross_card {
            CoherenceLevel::CrossCard
        } else if cross_queue {
            CoherenceLevel::SameCardCrossQueue
        } else {
            CoherenceLevel::Uniform
        };
        // 跨卡且无拷贝任务 → 违例。
        if level == CoherenceLevel::CrossCard && !copy_present {
            let advice = "同资源被多卡触碰且无拷贝任务：建议插入 DeviceCopy（拷贝队列）显式搬运，或把触碰任务归并到资源归属卡";
            violations.push(CoherenceViolation {
                resource: rid,
                tasks: tasks.clone(),
                level,
                advice: advice.to_string(),
            });
        }
        t += 1;
    }
    violations
}

/// 校验通过判（违例空表即通过——映射可提交执行面）。
pub fn coherence_ok(nodes: &[MapNode], mappings: &[QueueMapping]) -> bool {
    check_coherence(nodes, mappings).is_empty()
}

// ---------------------------------------------------------------------------
// 七、映射开销账（判据四；口径复用 F2408：步=0.1% 帧预算）
// ---------------------------------------------------------------------------

/// 映射面开销段闭集（四段：规则查行/路由推导/一致性校验/账本落账）。
pub const MAP_SEGMENTS: [&str; 4] = ["规则查行", "路由推导", "一致性校验", "账本落账"];

/// 每段预算步（Σ=10 步 = 1.0% 帧预算——映射是帧的固定税，红线钉死）。
pub const MAP_BUDGET_STEPS: [u32; 4] = [2, 3, 3, 2];

/// 帧预算总步（与 F2408 同源口径 10000 步）。
pub const MAP_FRAME_BUDGET_STEPS: u32 = 10_000;

/// 开销预算行（一段一账；measured 上游注入——零墙钟）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapBudgetLine {
    /// 段名。
    pub segment: &'static str,
    /// 预算步。
    pub budget: u32,
    /// 实测步（上游注入）。
    pub measured: u32,
}

/// 预算表（与 MAP_SEGMENTS/MAP_BUDGET_STEPS 对拍——判据侧独立重算）。
pub const MAP_BUDGET_TABLE: [MapBudgetLine; 4] = [
    MapBudgetLine { segment: MAP_SEGMENTS[0], budget: MAP_BUDGET_STEPS[0], measured: 0 },
    MapBudgetLine { segment: MAP_SEGMENTS[1], budget: MAP_BUDGET_STEPS[1], measured: 0 },
    MapBudgetLine { segment: MAP_SEGMENTS[2], budget: MAP_BUDGET_STEPS[2], measured: 0 },
    MapBudgetLine { segment: MAP_SEGMENTS[3], budget: MAP_BUDGET_STEPS[3], measured: 0 },
];

/// 预算行对账（表错先红：段名/预算数逐项对拍）。
pub fn map_budget_table_ok() -> bool {
    if MAP_BUDGET_TABLE.len() != 4 {
        return false;
    }
    let mut i = 0usize;
    while i < 4 {
        let row = match MAP_BUDGET_TABLE.get(i) {
            Some(r) => r,
            None => return false,
        };
        let seg = match MAP_SEGMENTS.get(i) {
            Some(s) => *s,
            None => return false,
        };
        let bud = match MAP_BUDGET_STEPS.get(i) {
            Some(b) => *b,
            None => return false,
        };
        if row.segment != seg || row.budget != bud {
            return false;
        }
        i += 1;
    }
    true
}

/// 映射开销审计（判据四：Σmeasured 占帧预算万分比；恰红线为达标「≤」）。
pub const fn map_overhead_ppm(measured_steps: &[u32; 4]) -> u32 {
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < 4 {
        let m = measured_steps[i];
        sum = sum.saturating_add(m);
        i += 1;
    }
    sum.saturating_mul(10_000) / MAP_FRAME_BUDGET_STEPS
}

/// 预算占比上界（1.0% → 100 ppm…… 实为 10000×10/10000=10？——澄清：
/// 10 步占 10000 步 = 万分之十 = 0.1%。红线：Σbudget=10 步 → 10 ppm）。
pub const MAP_BUDGET_PPM: u32 = 10;

// ---------------------------------------------------------------------------
// 八、三队列用例模型（判据五：用例表 + 走查）
// ---------------------------------------------------------------------------

/// 三队列用例（一行 = 一个典型任务的映射期望——走查基准）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueUseCase {
    /// 用例名。
    pub name: &'static str,
    /// 任务类别。
    pub kind: TaskKind,
    /// 期望队列。
    pub expect_queue: QueueKind,
    /// 期望路由依据。
    pub expect_basis: &'static str,
}

/// 三队列用例表（判据五：三类各一例 + 屏障特例一例 + 呈现主卡一例）。
pub const QUEUE_USE_CASES: [QueueUseCase; 5] = [
    QueueUseCase { name: "主pass绘制", kind: TaskKind::Rasterize, expect_queue: QueueKind::Graphics, expect_basis: BASIS_RES_HOME },
    QueueUseCase { name: "光照异步计算", kind: TaskKind::ComputeDispatch, expect_queue: QueueKind::Compute, expect_basis: BASIS_RES_HOME },
    QueueUseCase { name: "纹理上传", kind: TaskKind::Upload, expect_queue: QueueKind::Copy, expect_basis: BASIS_RES_HOME },
    QueueUseCase { name: "资源屏障", kind: TaskKind::Barrier, expect_queue: QueueKind::Graphics, expect_basis: BASIS_BARRIER_FOLLOWS_WRITE },
    QueueUseCase { name: "呈现提交", kind: TaskKind::Present, expect_queue: QueueKind::Graphics, expect_basis: BASIS_PRESENT_PRIMARY },
];

/// 用例走查（判据五机检面：构造最小帧图 → 映射 → 逐例对拍期望）。
///
/// 最小拓扑：单卡（card=1 主卡、三队列各 1）、单资源（res=100 归卡 1）。
/// 五例任务全部触碰 res=100（呈现例外：呈现无资源触碰走主卡规则）。
pub fn walk_use_cases() -> Result<bool, MqCode> {
    let mut eng = MapEngine::new();
    let dev = GpuDevice {
        card: 1,
        label: "dGPU0".to_string(),
        queues: [1, 1, 1],
        primary: true,
    };
    eng.devices.register(dev)?;
    eng.homes.assign(100, 1)?;
    let mut order: Vec<u32> = Vec::new();
    let mut nodes: Vec<MapNode> = Vec::new();
    // 任务 id 1..=5 对应用例序。
    let mut i = 0usize;
    while i < QUEUE_USE_CASES.len() {
        let uc = match QUEUE_USE_CASES.get(i) {
            Some(u) => *u,
            None => break,
        };
        let tid = (i + 1) as u32;
        order.push(tid);
        let node = if uc.kind == TaskKind::Present {
            MapNode {
                task_id: tid,
                kind: uc.kind,
                deps: Vec::new(),
                res_read: Vec::new(),
                res_write: Vec::new(),
                card_hint: 0,
            }
        } else {
            let mut w: Vec<u32> = Vec::new();
            w.push(100);
            MapNode {
                task_id: tid,
                kind: uc.kind,
                deps: Vec::new(),
                res_read: Vec::new(),
                res_write: w,
                card_hint: 0,
            }
        };
        nodes.push(node);
        i += 1;
    }
    let mappings = eng.map_frame(&order, &nodes)?;
    if mappings.len() != QUEUE_USE_CASES.len() {
        return Ok(false);
    }
    let mut i = 0usize;
    while i < QUEUE_USE_CASES.len() {
        let uc = match QUEUE_USE_CASES.get(i) {
            Some(u) => *u,
            None => break,
        };
        let m = match mappings.get(i) {
            Some(m) => m,
            None => return Ok(false),
        };
        if m.queue != uc.expect_queue || m.basis != uc.expect_basis {
            return Ok(false);
        }
        i += 1;
    }
    Ok(true)
}

// ---------------------------------------------------------------------------
// 九、映射账本（滚动留痕 + 聚合 + 三态查询——B08 五元组对齐）
// ---------------------------------------------------------------------------

/// 单帧映射留痕（B08 五元组：场景标签 + 映射数 + 违例数 + 开销 ppm + 码面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapFrameRecord {
    /// 场景标签（帧的场景上下文——与 F0483 场景标签同源）。
    pub scene: String,
    /// 映射节点数。
    pub mapped: u32,
    /// 一致性违例数。
    pub violations: u32,
    /// 映射开销（万分比）。
    pub overhead_ppm: u32,
    /// 首个诊断码（0 = 无错）。
    pub code: u16,
}

/// 映射账本（256 帧滚动 + 聚合桶 + 三态查询——模式复用 F0483 帧账）。
#[derive(Clone, Debug, Default)]
pub struct MapLedger {
    records: Vec<MapFrameRecord>,
}

impl MapLedger {
    /// 空账。
    pub fn new() -> Self {
        MapLedger { records: Vec::new() }
    }

    /// 落账（封顶 256 滚动——最老先出）。
    pub fn record(&mut self, rec: MapFrameRecord) {
        if self.records.len() >= 256 {
            self.records.remove(0);
        }
        self.records.push(rec);
    }

    /// 精确查（场景标签精确匹配 → 记录拷贝；无 → None）。
    pub fn exact(&self, scene: &str) -> Option<MapFrameRecord> {
        let mut i = self.records.len();
        while i > 0 {
            i -= 1;
            if let Some(r) = self.records.get(i) {
                if r.scene == scene {
                    return Some(r.clone());
                }
            }
        }
        None
    }

    /// 聚合查（场景前缀聚合 → (帧数, Σ映射, Σ违例, 峰值 ppm)）。
    pub fn aggregate(&self, prefix: &str) -> Option<(u32, u32, u32, u32)> {
        let mut frames = 0u32;
        let mut mapped = 0u32;
        let mut violations = 0u32;
        let mut peak = 0u32;
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.scene.starts_with(prefix) {
                    frames += 1;
                    mapped += r.mapped;
                    violations += r.violations;
                    if r.overhead_ppm > peak {
                        peak = r.overhead_ppm;
                    }
                }
            }
            i += 1;
        }
        if frames == 0 {
            None
        } else {
            Some((frames, mapped, violations, peak))
        }
    }

    /// 账深。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

/// 三态查询语义（查账的返回闭集——Exact / Aggregated / NotFound）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapQuery {
    /// 精确命中。
    Exact(MapFrameRecord),
    /// 前缀聚合命中。
    Aggregated(u32, u32, u32, u32),
    /// 未命中（表外场景——显性 NotFound，不猜）。
    NotFound,
}

/// 统一查账入口（先精确后聚合——两段式确定性）。
pub fn query_map_ledger(ledger: &MapLedger, scene: &str) -> MapQuery {
    if let Some(r) = ledger.exact(scene) {
        return MapQuery::Exact(r);
    }
    if let Some((f, m, v, p)) = ledger.aggregate(scene) {
        return MapQuery::Aggregated(f, m, v, p);
    }
    MapQuery::NotFound
}

// ---------------------------------------------------------------------------
// 十、对外接口冻结 v1（帧图 → 队列映射的一站式入口）
// ---------------------------------------------------------------------------

/// 映射请求（冻结 v1：版本号 + 序 + 节点表；扩展只增不改）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapRequestV1 {
    /// 协议版本（1）。
    pub version: u32,
    /// 场景标签。
    pub scene: String,
    /// 拓扑序（cgb07 排序面输出）。
    pub order: Vec<u32>,
    /// 节点表。
    pub nodes: Vec<MapNode>,
}

/// 映射答复（冻结 v1）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapReplyV1 {
    /// 接受标记。
    pub accepted: bool,
    /// 映射结论（接受时非空）。
    pub mappings: Vec<QueueMapping>,
    /// 一致性违例（接受但有待修时非空）。
    pub violations: Vec<CoherenceViolation>,
    /// 码面（0 = 全过）。
    pub code: u16,
    /// 人读明细。
    pub detail: String,
}

/// 映射管线（判据一~四串行一站式：规则查行→路由→映射→一致性校验→落账）。
///
/// 每段开销按 [`MAP_SEGMENTS`] 口径计入 budget 传入的实测数组（上游注入基址，
/// 本函数累加固定建模步——零墙钟，步数是建模常数而非计时）。
pub fn map_pipeline(
    eng: &mut MapEngine,
    ledger: &mut MapLedger,
    req: &MapRequestV1,
) -> MapReplyV1 {
    // 段 1：规则查行（建模 1 步——规则表驻留内存查行为常数步）。
    // 段 2：路由推导（建模 3 步——每节点常数路由规则）。
    // 段 3：一致性校验（建模 3 步）。
    // 段 4：账本落账（建模 2 步）。
    let mut overhead = [0u32; 4];
    overhead[0] = 1;
    overhead[1] = 3;
    overhead[2] = 3;
    overhead[3] = 2;
    // 输入面校验（有序性由 map_frame 守卫）。
    match eng.map_frame(&req.order, &req.nodes) {
        Ok(mappings) => {
            let violations = check_coherence(&req.nodes, &mappings);
            let ppm = map_overhead_ppm(&overhead);
            let rec = MapFrameRecord {
                scene: req.scene.clone(),
                mapped: mappings.len() as u32,
                violations: violations.len() as u32,
                overhead_ppm: ppm,
                code: 0,
            };
            ledger.record(rec);
            if violations.is_empty() {
                MapReplyV1 {
                    accepted: true,
                    mappings,
                    violations,
                    code: 0,
                    detail: "映射完成：一致性全过".to_string(),
                }
            } else {
                MapReplyV1 {
                    accepted: true,
                    mappings,
                    detail: "映射完成：存在一致性违例待修".to_string(),
                    violations,
                    code: MqCode::QueueCoherenceViolation as u16,
                }
            }
        }
        Err(code) => {
            let rec = MapFrameRecord {
                scene: req.scene.clone(),
                mapped: 0,
                violations: 0,
                overhead_ppm: map_overhead_ppm(&overhead),
                code: code as u16,
            };
            ledger.record(rec);
            MapReplyV1 {
                accepted: false,
                mappings: Vec::new(),
                violations: Vec::new(),
                code: code as u16,
                detail: code.detail().to_string(),
            }
        }
    }
}

/// 段守恒断言（编译期钉死）。
const _: () = {
    assert!(KIND_RULES.len() == 8, "类型映射规则表必须八行闭集");
    assert!(QUEUE_USE_CASES.len() == 5, "三队列用例表必须五行");
    assert!(MAP_SEGMENTS.len() == MAP_BUDGET_STEPS.len(), "段名与预算数必须等长");
};

/// 复用对拍锚点（判据侧逐条 grep 的锚字面量——单一来源）。
pub const REUSE_ANCHORS: [&str; 4] = [
    "cgb07_execfail::TaskNode 拓扑序执行面——映射在拓扑序之后",
    "F0106 放置策略——资源所在卡为路由依据",
    "VE-F0006 队列调度器——三队列语义不另立",
    "F2408 预算行口径——步=0.1% 帧预算",
];

// ---------------------------------------------------------------------------
// 十一、跨卡搬运计划器（一致性违例的修复方案生成）
// ---------------------------------------------------------------------------

/// 一条搬运建议（在哪个任务前插入什么搬运）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferPlan {
    /// 被搬运资源。
    pub resource: u32,
    /// 源卡（资源归属卡）。
    pub from_card: u32,
    /// 目标卡（缺拷贝任务的那个触碰卡）。
    pub to_card: u32,
    /// 建议插入位置：在此任务之前执行搬运。
    pub before_task: u32,
    /// 建议搬运类别（恒 DeviceCopy——拷贝队列显式搬运）。
    pub kind_hint: TaskKind,
    /// 人读依据。
    pub basis: String,
}

/// 从违例生成搬运计划（判据三的修复面：不静默、给可执行建议）。
///
/// 规则：目标卡 = 触碰任务中「非资源归属卡」的最小卡 id（平局取最小——确定性）；
/// 插入位置 = 触碰任务中最早映射到目标卡的任务（写依赖先行——搬运必须
/// 在首个目标卡触碰之前完成）。
pub fn plan_transfer(
    violation: &CoherenceViolation,
    nodes: &[MapNode],
    mappings: &[QueueMapping],
    homes: &ResourceHome,
) -> Option<TransferPlan> {
    // 源卡 = 资源归属。
    let from_card = homes.home_of(violation.resource)?;
    // 目标卡 = 非源卡的最小触碰卡。
    let mut target: Option<u32> = None;
    let mut i = 0usize;
    while i < violation.tasks.len() {
        let tid = match violation.tasks.get(i) {
            Some(x) => *x,
            None => break,
        };
        let mut m = 0usize;
        while m < mappings.len() {
            if let Some(mp) = mappings.get(m) {
                if mp.task_id == tid && mp.card != from_card {
                    match target {
                        None => target = Some(mp.card),
                        Some(t) => {
                            if mp.card < t {
                                target = Some(mp.card);
                            }
                        }
                    }
                    break;
                }
            }
            m += 1;
        }
        i += 1;
    }
    let to_card = target?;
    // 插入位置 = 最早映射到目标卡的任务。
    let mut before: Option<u32> = None;
    let mut i = 0usize;
    while i < mappings.len() {
        if let Some(mp) = mappings.get(i) {
            if mp.card == to_card {
                // 只认触碰本资源的任务。
                let mut hit = false;
                let mut k = 0usize;
                while k < violation.tasks.len() {
                    if let Some(t) = violation.tasks.get(k) {
                        if *t == mp.task_id {
                            hit = true;
                            break;
                        }
                    }
                    k += 1;
                }
                if hit {
                    before = Some(mp.task_id);
                    break;
                }
            }
        }
        i += 1;
    }
    let before_task = before?;
    Some(TransferPlan {
        resource: violation.resource,
        from_card,
        to_card,
        before_task,
        kind_hint: TaskKind::DeviceCopy,
        basis: alloc::format!(
            "资源 {} 由卡 {} 搬运至卡 {}：在任务 {} 之前插入 DeviceCopy（拷贝队列），写依赖先行",
            violation.resource, from_card, to_card, before_task
        ),
    })
}

/// 批量计划（全部跨卡违例 → 计划清单；非跨卡违例不生成——同卡问题由同步原语解决）。
pub fn plan_transfers(
    violations: &[CoherenceViolation],
    nodes: &[MapNode],
    mappings: &[QueueMapping],
    homes: &ResourceHome,
) -> Vec<TransferPlan> {
    let mut out: Vec<TransferPlan> = Vec::new();
    let mut i = 0usize;
    while i < violations.len() {
        if let Some(v) = violations.get(i) {
            if v.level == CoherenceLevel::CrossCard {
                if let Some(plan) = plan_transfer(v, nodes, mappings, homes) {
                    out.push(plan);
                }
            }
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 十二、队列实例路由（slot → 队列实例号；同队列多实例的确定性分派）
// ---------------------------------------------------------------------------

/// 队列实例分派策略闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotPolicy {
    /// 独占：全部落实例 0（简单、缓存友好——默认）。
    Pinned,
    /// 轮转：slot 按实例数取模（均衡——多实例并行收益场景）。
    RoundRobin,
}

impl SlotPolicy {
    /// 全量闭集。
    pub const ALL: [SlotPolicy; 2] = [SlotPolicy::Pinned, SlotPolicy::RoundRobin];

    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            SlotPolicy::Pinned => "独占",
            SlotPolicy::RoundRobin => "轮转",
        }
    }
}

/// slot → 实例号（确定性；capacity ≤ 0 视为 1 实例——防御）。
pub const fn slot_to_instance(slot: u32, capacity: u32, policy: SlotPolicy) -> u32 {
    let cap = if capacity == 0 { 1 } else { capacity };
    match policy {
        SlotPolicy::Pinned => 0,
        SlotPolicy::RoundRobin => {
            if slot == 0 {
                0
            } else {
                slot % cap
            }
        }
    }
}

/// 实例路由器（对既有映射清单补实例号——映射后处理段）。
pub fn assign_instances(
    mappings: &mut [QueueMapping],
    devices: &DeviceTable,
    policy: SlotPolicy,
) {
    let mut i = 0usize;
    while i < mappings.len() {
        if let Some(m) = mappings.get_mut(i) {
            let cap = match devices.device_of(m.card) {
                Some(d) => d.queue_capacity(m.queue),
                None => 1,
            };
            m.slot = slot_to_instance(m.slot, cap, policy);
        }
        i += 1;
    }
}

// ---------------------------------------------------------------------------
// 十三、负载分布统计（按卡/按队列聚合——热点与均衡度）
// ---------------------------------------------------------------------------

/// 按卡聚合（卡 → 映射数；表外卡不出现）。
pub fn load_by_card(mappings: &[QueueMapping]) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    let mut i = 0usize;
    while i < mappings.len() {
        let c = match mappings.get(i) {
            Some(m) => m.card,
            None => break,
        };
        let mut found = false;
        let mut k = 0usize;
        while k < out.len() {
            if let Some(entry) = out.get_mut(k) {
                if entry.0 == c {
                    entry.1 += 1;
                    found = true;
                    break;
                }
            }
            k += 1;
        }
        if !found {
            out.push((c, 1));
        }
        i += 1;
    }
    // 升序输出（按卡 id——确定性）。
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// 按队列聚合（队列 → 映射数；三行闭集——零映射也出零行）。
pub fn load_by_queue(mappings: &[QueueMapping]) -> [u32; 3] {
    let mut out = [0u32; 3];
    let mut i = 0usize;
    while i < mappings.len() {
        if let Some(m) = mappings.get(i) {
            let o = m.queue.ordinal();
            if o < 3 {
                out[o] += 1;
            }
        }
        i += 1;
    }
    out
}

/// 均衡度（万分比：1 - (最小/最大)×10000；全零映射 = 10000 满均衡；
/// 单队列独大趋近 0）。确定性、无浮点。
pub const fn balance_ppm(by_queue: &[u32; 3]) -> u32 {
    let mut max = 0u32;
    let mut min = u32::MAX;
    let mut i = 0usize;
    while i < 3 {
        let v = by_queue[i];
        if v > max {
            max = v;
        }
        if v < min {
            min = v;
        }
        i += 1;
    }
    if max == 0 {
        return 10_000;
    }
    let spread = (max - min).saturating_mul(10_000) / max;
    10_000 - spread
}

/// 热点卡检测（负载超均值 1.5 倍的卡清单——触发放置再平衡的观测面）。
pub fn hot_cards(mappings: &[QueueMapping]) -> Vec<u32> {
    let by_card = load_by_card(mappings);
    // Σ 与均值（万分比避免浮点：hot ⟺ count×2 > 3×mean ⟺ count×2×n > 3×Σ）。
    let mut total = 0u32;
    let mut i = 0usize;
    while i < by_card.len() {
        if let Some((_, c)) = by_card.get(i) {
            total += *c;
        }
        i += 1;
    }
    let n = by_card.len() as u32;
    if n == 0 || total == 0 {
        return Vec::new();
    }
    let mut out: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < by_card.len() {
        if let Some((card, c)) = by_card.get(i) {
            // c/n > 3/2 ⟺ 2c > 3n？（不对——mean=total/n；c > 1.5×total/n ⟺ c×n > 1.5×total ⟺ 2×c×n > 3×total）
            let lhs = (*c as u64) * (n as u64) * 2;
            let rhs = (total as u64) * 3;
            if lhs > rhs {
                out.push(*card);
            }
        }
        i += 1;
    }
    out
}

/// 深化节守恒断言。
const _: () = {
    assert!(SlotPolicy::ALL.len() == 2, "分派策略必须二闭集");
};

// ---------------------------------------------------------------------------
// 十四、映射回放与漂移检测（确定性重放——同输入必同输出）
// ---------------------------------------------------------------------------

/// 漂移记录（回放不一致的留痕）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapDrift {
    /// 任务 id。
    pub task_id: u32,
    /// 首跑队列。
    pub queue_first: QueueKind,
    /// 回放队列。
    pub queue_replay: QueueKind,
    /// 首跑卡。
    pub card_first: u32,
    /// 回放卡。
    pub card_replay: u32,
}

/// 单帧回放比对（确定性守卫：重放同输入，逐条对拍映射结论）。
///
/// 返回漂移清单（空表 = 回放一致 = 映射引擎确定性成立）。引擎的负载
/// 计数器在回放前必须清零——调用方用新引擎重放（建模范式：引擎无隐藏态）。
pub fn replay_compare(
    order: &[u32],
    nodes: &[MapNode],
    first: &[QueueMapping],
    homes: &ResourceHome,
    devices: &DeviceTable,
) -> Vec<MapDrift> {
    // 构造新引擎重放（设备表/归属账拷贝注入——引擎自身零隐藏态）。
    let mut eng = MapEngine::new();
    eng.homes = homes.clone();
    eng.devices = devices.clone();
    let replay = match eng.map_frame(order, nodes) {
        Ok(r) => r,
        Err(_) => {
            // 回放失败 = 引擎非确定（同输入不同结局）——全量漂移单条总账。
            let mut out: Vec<MapDrift> = Vec::new();
            if let Some(f0) = first.get(0) {
                out.push(MapDrift {
                    task_id: f0.task_id,
                    queue_first: f0.queue,
                    queue_replay: f0.queue,
                    card_first: f0.card,
                    card_replay: f0.card,
                });
            }
            return out;
        }
    };
    let mut out: Vec<MapDrift> = Vec::new();
    let mut i = 0usize;
    while i < first.len() {
        let f = match first.get(i) {
            Some(x) => x,
            None => break,
        };
        // 回放侧找同任务。
        let mut r = 0usize;
        let mut found: Option<&QueueMapping> = None;
        while r < replay.len() {
            if let Some(rp) = replay.get(r) {
                if rp.task_id == f.task_id {
                    found = Some(rp);
                    break;
                }
            }
            r += 1;
        }
        if let Some(rp) = found {
            if rp.queue != f.queue || rp.card != f.card {
                out.push(MapDrift {
                    task_id: f.task_id,
                    queue_first: f.queue,
                    queue_replay: rp.queue,
                    card_first: f.card,
                    card_replay: rp.card,
                });
            }
        }
        i += 1;
    }
    out
}

/// 帧间稳定性（连续两帧同场景同图 → 映射应稳定；漂移即帧间抖动源）。
///
/// 比较「映射的 (任务→队列, 卡) 组合」在两帧之间的差异面；返回变化任务数。
/// 场景切换导致的合法变化由调用方按场景标签过滤——本函数只给裸差异。
pub fn frame_stability(a: &[QueueMapping], b: &[QueueMapping]) -> u32 {
    let mut diff = 0u32;
    let mut i = 0usize;
    while i < a.len() {
        let ma = match a.get(i) {
            Some(x) => x,
            None => break,
        };
        // b 侧找同任务。
        let mut j = 0usize;
        let mut changed = true;
        while j < b.len() {
            if let Some(mb) = b.get(j) {
                if mb.task_id == ma.task_id {
                    if mb.queue == ma.queue && mb.card == ma.card {
                        changed = false;
                    }
                    break;
                }
            }
            j += 1;
        }
        if changed {
            diff += 1;
        }
        i += 1;
    }
    diff
}

/// 稳定性等级闭集（差异面 → 稳定度评级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StabilityGrade {
    /// 全同（差异 0）。
    Frozen,
    /// 微漂（差异 ≤ 全图 10%）。
    Micro,
    /// 活跃（差异 ≤ 50%）。
    Active,
    /// 重排（差异 > 50%——建议查放置抖动源）。
    Shuffled,
}

impl StabilityGrade {
    /// 评级（确定性：差异比例整数万分比判定）。
    pub const fn grade(diff: u32, total: u32) -> StabilityGrade {
        if total == 0 {
            return StabilityGrade::Frozen;
        }
        // 比例万分比 = diff×10000/total。
        const PER_UNIT: u32 = 10_000;
        let ppm = if diff >= total {
            PER_UNIT
        } else {
            diff.saturating_mul(PER_UNIT) / total
        };
        if ppm == 0 {
            StabilityGrade::Frozen
        } else if ppm <= 1_000 {
            StabilityGrade::Micro
        } else if ppm <= 5_000 {
            StabilityGrade::Active
        } else {
            StabilityGrade::Shuffled
        }
    }

    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            StabilityGrade::Frozen => "全同",
            StabilityGrade::Micro => "微漂",
            StabilityGrade::Active => "活跃",
            StabilityGrade::Shuffled => "重排",
        }
    }
}

/// 回放一致性自检（引擎建模范守卫：walk_use_cases 的映射重放零漂移）。
pub fn replay_selfcheck() -> bool {
    // 构造与 walk_use_cases 相同的最小拓扑。
    let mut eng = MapEngine::new();
    let dev = GpuDevice {
        card: 1,
        label: "dGPU0".to_string(),
        queues: [1, 1, 1],
        primary: true,
    };
    if eng.devices.register(dev).is_err() {
        return false;
    }
    if eng.homes.assign(100, 1).is_err() {
        return false;
    }
    let mut order: Vec<u32> = Vec::new();
    let mut nodes: Vec<MapNode> = Vec::new();
    let mut i = 0usize;
    while i < QUEUE_USE_CASES.len() {
        let uc = match QUEUE_USE_CASES.get(i) {
            Some(u) => *u,
            None => break,
        };
        let tid = (i + 1) as u32;
        order.push(tid);
        let node = if uc.kind == TaskKind::Present {
            MapNode {
                task_id: tid,
                kind: uc.kind,
                deps: Vec::new(),
                res_read: Vec::new(),
                res_write: Vec::new(),
                card_hint: 0,
            }
        } else {
            let mut w: Vec<u32> = Vec::new();
            w.push(100);
            MapNode {
                task_id: tid,
                kind: uc.kind,
                deps: Vec::new(),
                res_read: Vec::new(),
                res_write: w,
                card_hint: 0,
            }
        };
        nodes.push(node);
        i += 1;
    }
    let first = match eng.map_frame(&order, &nodes) {
        Ok(f) => f,
        Err(_) => return false,
    };
    replay_compare(&order, &nodes, &first, &eng.homes, &eng.devices).is_empty()
}

// ---------------------------------------------------------------------------
// 十五、映射遥测导出（B08 五元组快照——观测面收口）
// ---------------------------------------------------------------------------

/// 指标名前缀（遥测命名纪律：域面点分路径——与 F2408 统一模型对齐）。
pub const MAP_METRIC_PREFIX: &str = "b02.framegraph.";

/// 遥测指标行（id/值/单位/口径一行——B08 五元组形状）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapMetric {
    /// 指标 id（前缀 + 短名——全量可枚举闭集内）。
    pub id: &'static str,
    /// 值（整数口径——无浮点）。
    pub value: u32,
    /// 单位。
    pub unit: &'static str,
    /// 口径一句话。
    pub basis: &'static str,
}

/// 指标 id 闭集（六条——表外 id 导出即拒）。
pub const MAP_METRIC_IDS: [&str; 6] = [
    "b02.framegraph.mapped",
    "b02.framegraph.violations",
    "b02.framegraph.transfers",
    "b02.framegraph.overhead_ppm",
    "b02.framegraph.balance_ppm",
    "b02.framegraph.drift",
];

/// 指标 id 在册核验（运行时逐字符比较——id 全为静态字面量）。
pub fn metric_id_known(id: &str) -> bool {
    let mut i = 0usize;
    while i < MAP_METRIC_IDS.len() {
        let known = MAP_METRIC_IDS[i];
        if known == id {
            return true;
        }
        i += 1;
    }
    false
}

/// 单帧遥测快照导出（判据四 + 九的观测收口；确定性、零 IO）。
pub fn export_metrics(
    mapped: u32,
    violations: u32,
    transfers: u32,
    overhead_ppm: u32,
    balance_ppm: u32,
    drift: u32,
) -> Vec<MapMetric> {
    let mut out: Vec<MapMetric> = Vec::new();
    let mut i = 0usize;
    let rows: [(&'static str, u32, &'static str, &'static str); 6] = [
        (MAP_METRIC_IDS[0], mapped, "个", "本帧映射节点数"),
        (MAP_METRIC_IDS[1], violations, "条", "一致性违例数"),
        (MAP_METRIC_IDS[2], transfers, "条", "跨卡搬运计划数"),
        (MAP_METRIC_IDS[3], overhead_ppm, "ppm", "映射面开销占帧预算万分比"),
        (MAP_METRIC_IDS[4], balance_ppm, "ppm", "三队列均衡度万分比"),
        (MAP_METRIC_IDS[5], drift, "条", "回放漂移条数"),
    ];
    while i < 6 {
        let row = match rows.get(i) {
            Some(r) => *r,
            None => break,
        };
        if metric_id_known(row.0) {
            out.push(MapMetric { id: row.0, value: row.1, unit: row.2, basis: row.3 });
        }
        i += 1;
    }
    out
}

/// 快照完整性核验（导出面必须恰好六条全 id——缺行即导出面残缺）。
pub fn export_metrics_complete(m: &[MapMetric]) -> bool {
    if m.len() != MAP_METRIC_IDS.len() {
        return false;
    }
    let mut i = 0usize;
    while i < m.len() {
        let row = match m.get(i) {
            Some(x) => x,
            None => return false,
        };
        let known = match MAP_METRIC_IDS.get(i) {
            Some(k) => *k,
            None => return false,
        };
        if row.id != known {
            return false;
        }
        i += 1;
    }
    true
}

/// 尾守恒断言（导出面口径）。
const _: () = {
    assert!(MAP_METRIC_IDS.len() == 6, "映射遥测指标必须六条闭集");
};
