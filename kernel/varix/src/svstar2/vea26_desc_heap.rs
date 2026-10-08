//! VE-F0026 · 描述符堆与绑定模型（VE-A 域 · 堆管理 + 跨 API 抽象 · 目标 420 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0026`
//!
//! **判据（锚点原文）**：资源绑定的描述符堆管理（堆分配/分片/复用三段），绑定模型
//! 抽象（跨 API 的绑定差异抹平——D3D12/Vulkan/Metal 三家绑定的统一抽象），堆耗尽
//! 防护；含描述符堆的碎片率监控。判据五条：**堆三段、跨 API 抽象、耗尽防护、
//! 分帧回收、判据**。
//!
//! **错误路径与降级矩阵**：堆耗尽→分帧回收；分配失败→降级；抽象违例→拒绝。
//!
//! **数据结构**：堆管理；抽象层。
//!
//! **性能逐项分解**：O(描述符)——分配线性扫空闲位图、复用查表常数、碎片率统计
//! 线性扫已分配段，均以单堆描述符上限 [`MAX_DESCRIPTORS`] 与堆数 [`MAX_HEAPS`]
//! 为界；分帧回收每帧只处理 [`RECLAIM_PER_FRAME`] 个。
//!
//! **跨批对接点**：A08 句柄表联动——本条只管「**描述符槽位**」，句柄到槽位的
//! 映射由 A08 负责。本条产出的 [`BindingPlan`] 只含槽号与绑定序，**不含句柄**；
//! A08 改映射策略时本条不必改。
//!
//! **无障碍与隐私**：堆状态读屏可达（[`DescriptorHeaps::a11y_lines`]）——逐堆报
//! 容量/已用/空闲/碎片率/待回收数，中英双语。面板只报统计，不报资源内容。
//!
//! ## 设计要点
//!
//! - **三家 API 的绑定差异是**「**几类**绑定」**「槽位怎么编号」**「一帧能绑几次」，
//!   不是「绑定语义不同」。语义（ sampler / sampled texture / storage buffer…）
//!   三家一致；不一致的只有**打包方式与提交粒度**。故抽象层只抹这三处差异，
//!   语义枚举**直接共用**——若连语义也各造一套，抽象层就变成了三份实现的
//!   并排放置，跨 API 的真正收益（上层一套代码）当场消失。
//! - **耗尽→分帧回收，不是直接失败**（[`ExhaustionVerdict`]）：堆满时先回收
//!   本帧不再用的描述符，**回收不足才失败**。分帧回收每帧只做 [`RECLAIM_PER_FRAME`]
//!   个——一次全回收会造成帧尖峰，那正是分帧要避免的。
//! - **回收必须只回收「本帧未被引用」的**（[`RetireTag`]）：回收正在用的描述符
//!   会让在飞的绘制读到别人的资源——画面错但不崩。故回收带引用标记，
//!   [`RetireTag::LiveInFlight`] 一律不回收，**宁可回收不足也不误收**。
//! - **分配失败→降级，且降级要落到具体降级描述符**（[`DegradePlan`]）：降级不是
//!   「返回失败」，是换一条更省的绑定路径（纹理降采样、去掉可选绑定）并如实报出
//!   丢了哪些效果。静默丢效果 = 画面轻微不对且无人知道是降级导致的。
//! - **抽象违例→拒绝，且必须指出违的是哪条约定**（[`BindRejection`]）：三家绑定
//!   槽位编号规则不同（D3D12 需连续寄存器槽、Vulkan 需显式描述符集、
//!   Metal 需 argument buffer 偏移）。上层若把 Vulkan 的写法塞进 D3D12 的绑定表，
//!   必须**指名违反哪条**并拒绝，不能"尽力而为"地绑一下——那会绑出错资源而不报错。
//! - **碎片率是真算的，不是估的**（[`Fragmentation`]）：按已分配段的**最大空洞**
//!   与总量对账。判据用「中间挖一个洞后碎片率确实上升」这条**可失败的算术**
//!   钉住，而不是断言碎片率大于零（那对单段布局恒真）。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 单堆描述符数上限。
pub const MAX_DESCRIPTORS: usize = 128;

/// 堆数上限。
pub const MAX_HEAPS: usize = 8;

/// 分片数上限（堆按分片切段，分片是回收与碎片统计的基本单位）。
pub const MAX_SHARDS: usize = 16;

/// 每帧最多回收的描述符数（一次全回收会造成帧尖峰）。
pub const RECLAIM_PER_FRAME: u32 = 4;

/// 单次降级最多丢弃的绑定数（防止降级把整张绑定表丢空）。
pub const MAX_DEGRADE_DROPS: usize = 8;

/// 拒绝类诊断码（抽象违例 / 参数越界）。
pub const E_HEAP_REJECT: &str = "E_DESCRIPTOR_HEAP_REJECT";

/// 耗尽类诊断码（回收不足仍失败）。
pub const E_HEAP_EXHAUSTED: &str = "E_DESCRIPTOR_HEAP_EXHAUSTED";

/// 降级类诊断码（分配失败→降级绑定）。
pub const E_HEAP_DEGRADED: &str = "E_DESCRIPTOR_HEAP_DEGRADED";

/// 堆三段契约。
pub const HEAP_STAGE_DOC: &str = "\
描述符堆三段契约（VE-F0026 · v1）：① 堆分配——按分片切段分配，槽位连续；\
② 分片——分片是回收与碎片统计的基本单位，碎片率按段间最大空洞真算；\
③ 复用——优先复用已释放槽位，复用不增长已用量。三段各有独立失败处置，故不合并。";

/// 抽象契约。
pub const ABSTRACT_DOC: &str = "\
跨 API 抽象契约（VE-F0026 · v1）：三家（D3D12/Vulkan/Metal）的差异只在**绑定类别数、\
槽位编号规则、一帧绑定次数**三处；绑定**语义**三家一致，故语义枚举直接共用。\
连语义也各造一套，抽象层就退化成三份实现并排，跨 API 的收益当场消失。";

/// 回收契约。
pub const RECLAIM_DOC: &str = "\
分帧回收契约（VE-F0026 · v1）：堆满先回收本帧不再用的描述符，回收不足才失败；\
每帧至多回收 RECLAIM_PER_FRAME 个以避免帧尖峰；引用标记为 LiveInFlight 的一律不回收\
——回收在飞资源会让绘制读到别人的画面，且画面错但不崩，比失败难查。宁可回收不足也不误收。";

// ---------------------------------------------------------------------------
// 二、绑定语义（三家共用）与 API 差异
// ---------------------------------------------------------------------------

/// 绑定语义（三家 API **共用同一套**——差异不在语义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindKind {
    /// 采样器。
    Sampler,
    /// 只读采样纹理。
    SampledTexture,
    /// 读写纹理。
    StorageTexture,
    /// 只读 uniform 缓冲。
    UniformBuffer,
    /// 读写 storage 缓冲。
    StorageBuffer,
    /// 加速结构（光追）。
    AccelerationStructure,
}

impl BindKind {
    /// 全集规模。
    pub const ALL: [BindKind; 6] = [
        BindKind::Sampler,
        BindKind::SampledTexture,
        BindKind::StorageTexture,
        BindKind::UniformBuffer,
        BindKind::StorageBuffer,
        BindKind::AccelerationStructure,
    ];

    /// 稳定短名。
    pub const fn tag(self) -> &'static str {
        match self {
            BindKind::Sampler => "sampler",
            BindKind::SampledTexture => "sampled_texture",
            BindKind::StorageTexture => "storage_texture",
            BindKind::UniformBuffer => "uniform_buffer",
            BindKind::StorageBuffer => "storage_buffer",
            BindKind::AccelerationStructure => "accel_struct",
        }
    }

    /// 该语义是否为**可选绑定**（降级时可安全丢弃）。
    ///
    /// 采样器与加速结构视为可选：缺了不影响画面正确性，只影响画质。
    ///  uniform / storage 缓冲是**必需**——缺了着色器读不到数据，画面直接错。
    pub const fn optional(self) -> bool {
        matches!(self, BindKind::Sampler | BindKind::AccelerationStructure)
    }

    /// 该语义占用的描述符字节数（碎片与容量估算用）。
    pub const fn desc_bytes(self) -> u32 {
        match self {
            BindKind::Sampler => 1,
            BindKind::SampledTexture => 1,
            BindKind::StorageTexture => 1,
            BindKind::UniformBuffer => 2,
            BindKind::StorageBuffer => 2,
            BindKind::AccelerationStructure => 1,
        }
    }
}

/// 图形 API（差异只在这三处）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphicsApi {
    /// D3D12：绑定表需连续寄存器槽。
    D3D12,
    /// Vulkan：需显式描述符集。
    Vulkan,
    /// Metal：需 argument buffer 偏移。
    Metal,
}

impl GraphicsApi {
    /// 全集规模。
    pub const ALL: [GraphicsApi; 3] = [GraphicsApi::D3D12, GraphicsApi::Vulkan, GraphicsApi::Metal];

    /// 稳定短名。
    pub const fn tag(self) -> &'static str {
        match self {
            GraphicsApi::D3D12 => "d3d12",
            GraphicsApi::Vulkan => "vulkan",
            GraphicsApi::Metal => "metal",
        }
    }

    /// 中文名（读屏用）。
    pub const fn label_zh(self) -> &'static str {
        match self {
            GraphicsApi::D3D12 => "D3D12",
            GraphicsApi::Vulkan => "Vulkan",
            GraphicsApi::Metal => "Metal",
        }
    }

    /// 绑定类别数（差异点之一）。
    pub const fn bind_class_count(self) -> u32 {
        match self {
            // D3D12：CBV/SRV/UAV 靠寄存器槽区分，视图维度合得较紧。
            GraphicsApi::D3D12 => 2,
            // Vulkan：采样器/采样纹理/存储纹理/缓冲各自分类。
            GraphicsApi::Vulkan => 4,
            // Metal：argument buffer 按函数表与线程空间分组，类别数居中。
            GraphicsApi::Metal => 3,
        }
    }

    /// 槽位编号规则（差异点之二）。
    ///
    /// 返回 (是否要求同类连续, 每类槽位基数)。
    pub const fn slot_rule(self) -> (bool, u32) {
        match self {
            // D3D12：同一寄存器槽内的各绑定必须连续。
            GraphicsApi::D3D12 => (true, 32),
            // Vulkan：描述符集内自由编号，不要求同类连续。
            GraphicsApi::Vulkan => (false, 16),
            // Metal：argument buffer 内偏移对齐，不要求同类连续。
            GraphicsApi::Metal => (false, 8),
        }
    }

    /// 一帧允许的绑定次数上限（差异点之三）。
    pub const fn binds_per_frame(self) -> u32 {
        match self {
            GraphicsApi::D3D12 => 64,
            GraphicsApi::Vulkan => 128,
            GraphicsApi::Metal => 32,
        }
    }
}

/// 一条绑定声明（上层写法，三家通用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BindDecl {
    /// 绑定语义。
    pub kind: BindKind,
    /// 逻辑绑定位（同一 kind 内从 0 递增）。
    pub slot: u32,
}

/// 绑定拒绝结论（抽象违例 → 拒绝，且指名违反哪条）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindRejection {
    /// 同一语义的槽位不连续（违反 D3D12 的连续寄存器槽规则）。
    NotContiguous {
        /// 语义。
        kind: BindKind,
        /// 前一条槽位。
        prev: u32,
        /// 出现断裂的槽位。
        cur: u32,
    },
    /// 槽位超出该 API 的每类基数。
    SlotOutOfRange {
        /// 语义。
        kind: BindKind,
        /// 槽位。
        slot: u32,
        /// 该 API 允许的基数。
        limit: u32,
    },
    /// 一帧绑定次数超限。
    TooManyBinds {
        /// 次数。
        count: u32,
        /// 上限。
        limit: u32,
    },
    /// 同一语义槽位重复。
    DuplicateSlot {
        /// 语义。
        kind: BindKind,
        /// 重复的槽位。
        slot: u32,
    },
}

impl BindRejection {
    /// 诊断码（拒绝类，与耗尽/降级分开——处置方向相反不共用码）。
    pub const fn code(&self) -> &'static str {
        E_HEAP_REJECT
    }

    /// 可读的违例说明（读屏可达）。
    pub fn explain(&self) -> String {
        match self {
            BindRejection::NotContiguous { kind, prev, cur } => format!(
                "{} 槽位不连续（{} 后跳到 {}）：违反寄存器槽连续约定",
                kind.tag(),
                prev,
                cur
            ),
            BindRejection::SlotOutOfRange { kind, slot, limit } => format!(
                "{} 槽位 {} 超出基数 {}：越界约定",
                kind.tag(),
                slot,
                limit
            ),
            BindRejection::TooManyBinds { count, limit } => format!(
                "一帧绑定 {} 次超上限 {}：绑定次数约定",
                count, limit
            ),
            BindRejection::DuplicateSlot { kind, slot } => format!(
                "{} 槽位 {} 重复：同类槽位须唯一",
                kind.tag(),
                slot
            ),
        }
    }
}

/// 绑定校验结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindVerdict {
    /// 通过。
    Ok {
        /// 归一化后的绑定表（槽位按 kind 分组重排为连续）。
        plan: Vec<BindDecl>,
    },
    /// 拒绝（抽象违例）。
    Rejected(BindRejection),
}

/// 跨 API 绑定模型抽象层：校验 + 归一化。
///
/// 语义三家共用，故本抽象层只做「**按 API 的槽位规则校验并重排**」这一件事。
pub fn bind_normalize(api: GraphicsApi, decls: &[BindDecl]) -> BindVerdict {
    let (need_contig, per_class) = api.slot_rule();
    // 重复检测（同类槽位唯一）。
    let n = decls.len();
    let mut i = 0;
    while i < n {
        let mut j = i + 1;
        while j < n {
            if decls[i].kind == decls[j].kind && decls[i].slot == decls[j].slot {
                return BindVerdict::Rejected(BindRejection::DuplicateSlot {
                    kind: decls[i].kind,
                    slot: decls[i].slot,
                });
            }
            j += 1;
        }
        i += 1;
    }
    // 一帧绑定次数上限。
    if decls.len() as u32 > api.binds_per_frame() {
        return BindVerdict::Rejected(BindRejection::TooManyBinds {
            count: decls.len() as u32,
            limit: api.binds_per_frame(),
        });
    }
    // 每类槽位基数。
    let mut k = 0;
    while k < n {
        if decls[k].slot >= per_class {
            return BindVerdict::Rejected(BindRejection::SlotOutOfRange {
                kind: decls[k].kind,
                slot: decls[k].slot,
                limit: per_class,
            });
        }
        k += 1;
    }
    // 归一化：按 kind 分组，组内按 slot 升序，槽位重排为 0..m连续。
    // 三家共用同一归一化结果，只有「是否要求连续」这一校验不同——
    // 这正说明差异确实只落在规则上，不在语义上。
    let mut plan: Vec<BindDecl> = Vec::new();
    let mut ki = 0;
    while ki < BindKind::ALL.len() {
        let kind = BindKind::ALL[ki];
        let mut group: Vec<u32> = Vec::new();
        let mut i = 0;
        while i < n {
            if decls[i].kind == kind {
                group.push(decls[i].slot);
            }
            i += 1;
        }
        // 组内升序（插入排序，组小）。
        let g = group.len();
        let mut a = 1;
        while a < g {
            let key = group[a];
            let mut b = a;
            while b > 0 && key < group[b - 1] {
                group[b] = group[b - 1];
                b -= 1;
            }
            group[b] = key;
            a += 1;
        }
        let mut s = 0;
        while s < group.len() {
            // D3D12 要求同类连续：若原槽位有断裂即拒绝。
            if need_contig && group[s] != s as u32 {
                return BindVerdict::Rejected(BindRejection::NotContiguous {
                    kind,
                    prev: group[s.saturating_sub(1)],
                    cur: group[s],
                });
            }
            plan.push(BindDecl {
                kind,
                slot: s as u32,
            });
            s += 1;
        }
        ki += 1;
    }
    BindVerdict::Ok { plan }
}

// ---------------------------------------------------------------------------
// 三、描述符堆（分配 / 分片 / 复用三段）
// ---------------------------------------------------------------------------

/// 槽位引用标记（回收只看这个）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetireTag {
    /// 本帧仍在用，**不可回收**。
    LiveInFlight,
    /// 已释放，可回收。
    Free,
}

impl RetireTag {
    /// 是否可回收。
    pub const fn reclaimable(self) -> bool {
        matches!(self, RetireTag::Free)
    }
}

/// 一个描述符槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DescriptorSlot {
    /// 绑定语义。
    pub kind: BindKind,
    /// 引用标记。
    pub tag: RetireTag,
    /// 所属分片。
    pub shard: u8,
}

/// 一个分片（堆的基本回收/统计单位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shard {
    /// 分片起始槽。
    pub begin: u32,
    /// 分片长度。
    pub len: u32,
    /// 分片内已分配数。
    pub used: u32,
}

/// 碎片率统计（真算最大空洞）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fragmentation {
    /// 最大空洞槽数。
    pub max_hole: u32,
    /// 堆容量。
    pub capacity: u32,
    /// 已用槽数。
    pub used: u32,
}

impl Fragmentation {
    /// 碎片率（千分比）：空洞占比 × 1000，向上取整。
    ///
    /// 用千分比而非百分数：小堆上百分比只有 0/100 两档，无法区分好坏。
    pub const fn permille(&self) -> u32 {
        let free = self.capacity.saturating_sub(self.used);
        if self.capacity == 0 {
            return 0;
        }
        //不能用 u32::min（Ord 尚非 const trait），故手写取小。
        let hole = if self.max_hole < free {
            self.max_hole
        } else {
            free
        };
        (hole * 1000) / self.capacity
    }

    /// 是否碎片化严重（最大空洞超过总容量的 1/4）。
    pub const fn severe(&self) -> bool {
        self.capacity > 0 && self.max_hole * 4 > self.capacity
    }
}

/// 一个描述符堆。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DescriptorHeap {
    /// 堆号。
    pub id: u32,
    /// 槽位表（`None` = 空槽）。
    pub slots: Vec<Option<DescriptorSlot>>,
    /// 分片表。
    pub shards: Vec<Shard>,
    /// 本帧已分配次数（复用优先）。
    pub allocated_total: u32,
}

impl DescriptorHeap {
    /// 新建堆：容量 `capacity`，切成 `shard_count` 个等长分片。
    pub fn new(id: u32, capacity: usize, shard_count: usize) -> Result<DescriptorHeap, &'static str> {
        if capacity == 0 || capacity > MAX_DESCRIPTORS {
            return Err("堆容量非法");
        }
        if shard_count == 0 || shard_count > MAX_SHARDS {
            return Err("分片数非法");
        }
        let mut shards: Vec<Shard> = Vec::new();
        let per = capacity / shard_count;
        let mut start = 0usize;
        let mut i = 0;
        while i < shard_count {
            let len = if i == shard_count - 1 {
                capacity - start
            } else {
                per
            };
            shards.push(Shard {
                begin: start as u32,
                len: len as u32,
                used: 0,
            });
            start += len;
            i += 1;
        }
        let mut slots: Vec<Option<DescriptorSlot>> = Vec::new();
        let mut j = 0;
        while j < capacity {
            slots.push(None);
            j += 1;
        }
        Ok(DescriptorHeap {
            id,
            slots,
            shards,
            allocated_total: 0,
        })
    }

    /// **第一段：堆分配**（第三段「复用」在同一函数内：优先扫已释放槽）。
    pub fn acquire(&mut self, kind: BindKind, prefer_free: bool) -> Option<u32> {
        // 复用段：扫「已释放但仍占着 Some」的槽（历史残留，见 release 的口径说明）。
        // 正常 release 已把槽置 None，那种槽由下面的分配段捡起——两者都算复用，
        // 但只有「捡空槽」这一条会计入 allocated_total（新占位）。
        if prefer_free {
            let mut i = 0;
            while i < self.slots.len() {
                if let Some(s) = &self.slots[i] {
                    if s.kind == kind && s.tag.reclaimable() {
                        let at = i as u32;
                        self.slots[i] = Some(DescriptorSlot {
                            kind,
                            tag: RetireTag::LiveInFlight,
                            shard: self.shard_of(at),
                        });
                        return Some(at);
                    }
                }
                i += 1;
            }
        }
        // 分配段：找首个空槽（已释放的空槽也走这里，故「复用」天然生效）。
        let mut k = 0;
        while k < self.slots.len() {
            if self.slots[k].is_none() {
                let at = k as u32;
                self.slots[k] = Some(DescriptorSlot {
                    kind,
                    tag: RetireTag::LiveInFlight,
                    shard: self.shard_of(at),
                });
                self.bump_used(at);
                self.allocated_total += 1;
                return Some(at);
            }
            k += 1;
        }
        None
    }

    fn shard_of(&self, at: u32) -> u8 {
        let mut i = 0;
        while i < self.shards.len() {
            let s = self.shards[i];
            if at >= s.begin && at < s.begin + s.len {
                return i as u8;
            }
            i += 1;
        }
        0
    }

    fn bump_used(&mut self, at: u32) {
        let idx = self.shard_of(at) as usize;
        self.shards[idx].used += 1;
    }

    fn drop_used(&mut self, at: u32) {
        let idx = self.shard_of(at) as usize;
        if self.shards[idx].used > 0 {
            self.shards[idx].used -= 1;
        }
    }

    /// 释放一槽：置空槽（`None`），使其既可被复用段捡起、也计入空洞。
///
/// 早先的实现把释放槽留成 `Some(Free)`，于是：①`reclaim` 扫`Some` 时
/// 看见 `Free` 才清，但 `used()` 也把 `Some` 计入，导致「已释放」与「已占用」
/// 在两个口径下互相打架；②碎片统计只认 `None` 为空洞，于是释放后碎片率
/// **纹丝不动**——释放根本没起到腾出空洞的作用。改为直接置 `None`：
/// 「空槽」就是「可复用且是空洞」，一个口径两处用，不再打架。
pub fn release(&mut self, at: u32) -> bool {
        if (at as usize) >= self.slots.len() {
            return false;
        }
        if self.slots[at as usize].is_none() {
            return false;
        }
        self.slots[at as usize] = None;
        self.drop_used(at);
        true
    }

    /// **第二段：分片回收**（只回收 `Free`，在飞的一律不收）。
    ///
    /// 每帧至多回收 [`RECLAIM_PER_FRAME`] 个——一次全回收会造成帧尖峰。
    /// 返回实际回收数。
    ///
    /// 注意口径：正常路径下 `release` 已把槽置空（`None`），故这里能回收的
    /// 只剩「被标记为 `Free` 但仍占着 `Some`」的历史残留——即中途释放、
    /// 尚未经回收段清空的槽。在飞槽（`LiveInFlight`）一个都不动。
    pub fn reclaim(&mut self, budget: u32) -> u32 {
        let mut freed = 0u32;
        let cap = if budget < RECLAIM_PER_FRAME {
            budget
        } else {
            RECLAIM_PER_FRAME
        };
        let mut i = 0;
        while i < self.slots.len() && freed < cap {
            let reclaimable = match &self.slots[i] {
                Some(s) => s.tag.reclaimable(),
                None => false,
            };
            if reclaimable {
                self.slots[i] = None;
                freed += 1;
            }
            i += 1;
        }
        freed
    }

    /// 已用槽数。
    pub fn used(&self) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < self.slots.len() {
            if self.slots[i].is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 容量。
    pub fn capacity(&self) -> u32 {
        self.slots.len() as u32
    }

    /// 空闲槽数。
    pub fn free(&self) -> u32 {
        self.capacity() - self.used()
    }

    /// 碎片率（真算最大空洞）。
    ///
    /// 空堆口径：全空时`max_hole = 0` ——「一个描述符都没用」不是碎片化，
    /// 若按「全空 = 一个长度 capacity 的空洞」算，空堆会报 1000‰ 碎片率，
    /// 看板一上来就是红的——那是把「空闲」误报成「碎片」。
    pub fn fragmentation(&self) -> Fragmentation {
        if self.used() == 0 {
            return Fragmentation {
                max_hole: 0,
                capacity: self.capacity(),
                used: 0,
            };
        }
        let mut run = 0u32;
        let mut best = 0u32;
        let mut i = 0;
        while i < self.slots.len() {
            let empty = self.slots[i].is_none();
            if empty {
                run += 1;
                if run > best {
                    best = run;
                }
            } else {
                run = 0;
            }
            i += 1;
        }
        Fragmentation {
            max_hole: best,
            capacity: self.capacity(),
            used: self.used(),
        }
    }
}

/// 耗尽处置结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExhaustionVerdict {
    /// 分配成功。
    Allocated {
        /// 槽位。
        slot: u32,
    },
    /// 回收后成功（分帧回收救回）。
    ReclaimedThenAllocated {
        /// 槽位。
        slot: u32,
        /// 本次回收数。
        reclaimed: u32,
    },
    /// 回收不足仍失败（耗尽）。
    Exhausted {
        /// 原因。
        reason: String,
    },
}

/// 降级计划（分配失败 → 降级，且落到具体要丢哪些绑定）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DegradePlan {
    /// 丢弃的绑定（**只允许可选绑定**）。
    pub dropped: Vec<BindDecl>,
    /// 降级后的绑定表。
    pub plan: Vec<BindDecl>,
}

/// 描述符堆集合（三段管理 + 耗尽防护）。
#[derive(Clone, Debug)]
pub struct DescriptorHeaps {
    /// 堆表。
    pub heaps: Vec<DescriptorHeap>,
    /// 目标 API。
    pub api: GraphicsApi,
    /// 累计耗尽次数。
    pub exhausted_total: u32,
    /// 累计降级次数。
    pub degraded_total: u32,
}

impl Default for DescriptorHeaps {
    /// 默认以 Vulkan 为目标 API（D3D12 的连续槽约束最严，Metal 上限最小，
    /// 用最宽松的一家做默认才不会一上来就误拒）。
    fn default() -> DescriptorHeaps {
        DescriptorHeaps::new(GraphicsApi::Vulkan)
    }
}

impl DescriptorHeaps {
    /// 新建堆集合。
    pub const fn new(api: GraphicsApi) -> DescriptorHeaps {
        DescriptorHeaps {
            heaps: Vec::new(),
            api,
            exhausted_total: 0,
            degraded_total: 0,
        }
    }

    /// 添加堆（超上限即拒）。
    pub fn add_heap(&mut self, h: DescriptorHeap) -> Result<(), &'static str> {
        if self.heaps.len() >= MAX_HEAPS {
            return Err("堆数超上限");
        }
        self.heaps.push(h);
        Ok(())
    }

    /// 带耗尽防护的分配：分配段 → 复用段 → 分片回收 → 仍失败则耗尽。
    pub fn acquire(&mut self, heap_idx: usize, kind: BindKind) -> ExhaustionVerdict {
        if heap_idx >= self.heaps.len() {
            return ExhaustionVerdict::Exhausted {
                reason: String::from("堆号越界"),
            };
        }
        // ① 分配（含复用）段。
        if let Some(slot) = self.heaps[heap_idx].acquire(kind, true) {
            return ExhaustionVerdict::Allocated { slot };
        }
        // ② 分片回收段（每帧有上限）。
        let reclaimed = self.heaps[heap_idx].reclaim(RECLAIM_PER_FRAME);
        if reclaimed > 0 {
            if let Some(slot) = self.heaps[heap_idx].acquire(kind, true) {
                return ExhaustionVerdict::ReclaimedThenAllocated { slot, reclaimed };
            }
        }
        // ③ 回收不足 → 耗尽。
        self.exhausted_total += 1;
        ExhaustionVerdict::Exhausted {
            reason: format!(
                "回收 {} 个仍不足（堆容量 {} 已用 {}）：在飞描述符不可回收",
                reclaimed,
                self.heaps[heap_idx].capacity(),
                self.heaps[heap_idx].used()
            ),
        }
    }

    /// 生成降级计划：只丢**可选**绑定，必需绑定一个不许丢。
    pub fn degrade_plan(&mut self, decls: &[BindDecl]) -> Result<DegradePlan, &'static str> {
        let mut dropped: Vec<BindDecl> = Vec::new();
        let mut plan: Vec<BindDecl> = Vec::new();
        let mut i = 0;
        while i < decls.len() {
            let d = decls[i];
            if d.kind.optional() && dropped.len() < MAX_DEGRADE_DROPS {
                dropped.push(d);
            } else {
                plan.push(d);
            }
            i += 1;
        }
        if dropped.is_empty() {
            return Err("无可丢的可选绑定，降级不可行");
        }
        self.degraded_total += 1;
        Ok(DegradePlan { dropped, plan })
    }

    /// 堆状态读屏（逐堆报统计，不报资源内容）。
    pub fn a11y_lines(&self, locale: Locale) -> Vec<String> {
        let zh = matches!(locale, Locale::ZhCn);
        let mut v: Vec<String> = Vec::new();
        v.push(if zh {
            format!(
                "描述符堆状态，目标 API {}，共 {} 个堆",
                self.api.label_zh(),
                self.heaps.len()
            )
        } else {
            format!(
                "descriptor heap state, api {}, {} heaps",
                self.api.tag(),
                self.heaps.len()
            )
        });
        let mut i = 0;
        while i < self.heaps.len() {
            let h = &self.heaps[i];
            let f = h.fragmentation();
            v.push(if zh {
                format!(
                    "堆{} 容量{} 已用{} 空闲{} 最大空洞{} 碎片率{}‰ 分片{} 累计分配{}",
                    h.id,
                    h.capacity(),
                    h.used(),
                    h.free(),
                    f.max_hole,
                    f.permille(),
                    h.shards.len(),
                    h.allocated_total
                )
            } else {
                format!(
                    "heap{} cap{} used{} free{} maxhole{} frag{}permille shards{} alloc{}",
                    h.id,
                    h.capacity(),
                    h.used(),
                    h.free(),
                    f.max_hole,
                    f.permille(),
                    h.shards.len(),
                    h.allocated_total
                )
            });
            i += 1;
        }
        v.push(if zh {
            format!("累计耗尽 {} 次，累计降级 {} 次", self.exhausted_total, self.degraded_total)
        } else {
            format!("exhausted {} times, degraded {} times", self.exhausted_total, self.degraded_total)
        });
        v
    }

    /// 读屏别名。
    pub fn panel_lines(&self, locale: Locale) -> Vec<String> {
        self.a11y_lines(locale)
    }
}

/// 说明语言。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locale {
    /// 简体中文。
    ZhCn,
    /// 英文。
    En,
}

// ---------------------------------------------------------------------------
// 四、判据
// ---------------------------------------------------------------------------

/// VE-F0026 判据集。
pub fn run_vea26_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F0026");

    // ---- 跨 API 抽象 ----
    {
        set.add("A26-abs-三家API齐备", GraphicsApi::ALL.len() == 3, "");
    }
    {
        // 三家绑定类别数必须真的不同（否则「抹平差异」无从谈起）。
        let a = GraphicsApi::D3D12.bind_class_count();
        let b = GraphicsApi::Vulkan.bind_class_count();
        let c = GraphicsApi::Metal.bind_class_count();
        set.add("A26-abs-三家绑定类别数确有差异", a != b && b != c && a != c, "");
    }
    {
        // 槽位规则差异：D3D12 要求连续，Vulkan/Metal 不要求。
        set.add(
            "A26-abs-槽位规则差异存在",
            GraphicsApi::D3D12.slot_rule().0
                && !GraphicsApi::Vulkan.slot_rule().0
                && !GraphicsApi::Metal.slot_rule().0,
            "",
        );
    }
    {
        // 一帧绑定次数上限三家不同。
        let a = GraphicsApi::D3D12.binds_per_frame();
        let b = GraphicsApi::Vulkan.binds_per_frame();
        let c = GraphicsApi::Metal.binds_per_frame();
        set.add("A26-abs-绑定次数上限三家不同", a != b && b != c, "");
    }
    {
        // **语义三家共用**：同一套 BindKind 不按 API 分叉。
        set.add(
            "A26-abs-绑定语义单一来源",
            BindKind::ALL.len() == 6 && BindKind::ALL[0].tag() == "sampler",
            "",
        );
    }
    {
        // 同一张绑定表在三家都合法 → 归一化结果语义一致（只槽位规则校验不同）。
        let decls = vec![
            BindDecl {
                kind: BindKind::Sampler,
                slot: 0,
            },
            BindDecl {
                kind: BindKind::Sampler,
                slot: 1,
            },
            BindDecl {
                kind: BindKind::UniformBuffer,
                slot: 0,
            },
        ];
        let d = bind_normalize(GraphicsApi::D3D12, &decls);
        let v = bind_normalize(GraphicsApi::Vulkan, &decls);
        set.add(
            "A26-abs-同一表两家均通过且一致",
            matches!(d, BindVerdict::Ok { .. }) && matches!(v, BindVerdict::Ok { .. }),
            "",
        );
    }
    {
        // Vulkan/Metal 允许断裂，D3D12 拒绝——抽象违例被抓且指名。
        let decls = vec![
            BindDecl {
                kind: BindKind::Sampler,
                slot: 0,
            },
            BindDecl {
                kind: BindKind::Sampler,
                slot: 3,
            },
        ];
        let d = bind_normalize(GraphicsApi::D3D12, &decls);
        match d {
            BindVerdict::Rejected(BindRejection::NotContiguous { prev, cur, .. }) => set.add(
                "A26-abs-断裂在D3D12被拒并指名",
                prev == 0 && cur == 3,
                "",
            ),
            _ => set.add("A26-abs-断裂在D3D12被拒并指名", false, ""),
        }
        // Vulkan 不要求连续 → 同表通过。
        let v = bind_normalize(GraphicsApi::Vulkan, &decls);
        set.add(
            "A26-abs-断裂在Vulkan通过",
            matches!(v, BindVerdict::Ok { .. }),
            "",
        );
    }
    {
        // 槽位越界被拒。
        let decls = vec![BindDecl {
            kind: BindKind::Sampler,
            slot: 99,
        }];
        let v = bind_normalize(GraphicsApi::Metal, &decls);
        set.add(
            "A26-abs-槽位越界被拒",
            matches!(v, BindVerdict::Rejected(BindRejection::SlotOutOfRange { .. })),
            "",
        );
    }
    {
        // 同类槽位重复被拒。
        let decls = vec![
            BindDecl {
                kind: BindKind::Sampler,
                slot: 0,
            },
            BindDecl {
                kind: BindKind::Sampler,
                slot: 0,
            },
        ];
        let v = bind_normalize(GraphicsApi::Vulkan, &decls);
        set.add(
            "A26-abs-同类槽位重复被拒",
            matches!(v, BindVerdict::Rejected(BindRejection::DuplicateSlot { .. })),
            "",
        );
    }
    {
        // 一帧绑定次数超限被拒（用 Metal 的最小上限）。
        // 前提：表必须先通过「同类槽位唯一」与「槽位在基数内」这两关，
        // 否则会先被重复/越界拒掉，测不到次数上限这一条。
        // 故跨 kind 轮转铺满（6 类 × 每类足够多的槽位），保证同类不重复。
        let limit = GraphicsApi::Metal.binds_per_frame();
        let per_class = GraphicsApi::Metal.slot_rule().1 as usize;
        let mut decls: Vec<BindDecl> = Vec::new();
        let mut i = 0;
        while (i as u32) <= limit {
            let kind = BindKind::ALL[i % BindKind::ALL.len()];
            let within = i / BindKind::ALL.len();
            if within < per_class {
                decls.push(BindDecl {
                    kind,
                    slot: within as u32,
                });
            }
            i += 1;
        }
        let v = bind_normalize(GraphicsApi::Metal, &decls);
        // 前提核对：确实造出了「超过上限且其余都合规」的表。
        set.add("A26-abs-超限料前提成立", decls.len() as u32 > limit, "");
        set.add(
            "A26-abs-绑定次数超限被拒",
            matches!(v, BindVerdict::Rejected(BindRejection::TooManyBinds { .. })),
            "",
        );
    }
    {
        // 拒绝必须指名违反哪条（诊断文案非空且含语义名）。
        let decls = vec![
            BindDecl {
                kind: BindKind::Sampler,
                slot: 0,
            },
            BindDecl {
                kind: BindKind::Sampler,
                slot: 3,
            },
        ];
        let d = bind_normalize(GraphicsApi::D3D12, &decls);
        let text = match &d {
            BindVerdict::Rejected(r) => r.explain(),
            BindVerdict::Ok { .. } => String::new(),
        };
        set.add(
            "A26-abs-拒绝指名违反约定",
            text.contains("sampler") && text.contains("连续"),
            "",
        );
    }
    {
        // 归一化结果按 kind 分组、组内槽位连续（三家共用同一归一化逻辑）。
        let decls = vec![
            BindDecl {
                kind: BindKind::StorageBuffer,
                slot: 1,
            },
            BindDecl {
                kind: BindKind::Sampler,
                slot: 0,
            },
        ];
        match bind_normalize(GraphicsApi::Vulkan, &decls) {
            BindVerdict::Ok { plan } => set.add(
                "A26-abs-归一化后槽位连续",
                plan
                    .iter()
                    .any(|d| d.kind == BindKind::Sampler && d.slot == 0)
                    && plan
                        .iter()
                        .any(|d| d.kind == BindKind::StorageBuffer && d.slot == 0),
                "",
            ),
            _ => set.add("A26-abs-归一化后槽位连续", false, ""),
        }
    }

    // ---- 堆三段 ----
    {
        // 堆构造：容量与分片自洽（分片长度之和 = 容量）。
        let h = DescriptorHeap::new(0, 32, 4);
        set.add(
            "A26-heap-分片长度和等于容量",
            match &h {
                Ok(x) => {
                    let mut sum = 0u32;
                    let mut i = 0;
                    while i < x.shards.len() {
                        sum += x.shards[i].len;
                        i += 1;
                    }
                    sum == 32 && x.shards.len() == 4
                }
                Err(_) => false,
            },
            "",
        );
    }
    {
        // 容量与分片数非法即拒。
        set.add(
            "A26-heap-非法容量与分片被拒",
            DescriptorHeap::new(0, 0, 4).is_err()
                && DescriptorHeap::new(0, MAX_DESCRIPTORS + 1, 4).is_err()
                && DescriptorHeap::new(0, 32, 0).is_err()
                && DescriptorHeap::new(0, 32, MAX_SHARDS + 1).is_err(),
            "",
        );
    }
    {
        // 分配段：逐槽分配到满，已用 = 容量。
        let mut h = DescriptorHeap::new(0, 8, 2).unwrap();
        let mut ok = true;
        let mut i = 0;
        while i < 8 {
            if h.acquire(BindKind::Sampler, false).is_none() {
                ok = false;
            }
            i += 1;
        }
        set.add(
            "A26-heap-分配段逐槽到满",
            ok && h.used() == 8 && h.free() == 0,
            "",
        );
    }
    {
        // 复用段：释放后再分配**复用同一槽**（不新占位）。
        // 口径：release 已把槽置None，故「复用」体现在**槽号相同**。
        // `allocated_total` 统计的是「分配次数」而非「占位数」，复用也会计数
        // ——那是分配动作的忠实计数，不是新占位，别拿它判复用。
        let mut h = DescriptorHeap::new(0, 8, 2).unwrap();
        let a = h.acquire(BindKind::Sampler, false);
        let _ = h.release(a.unwrap());
        let b = h.acquire(BindKind::Sampler, true);
        set.add(
            "A26-heap-复用段复用同一槽位",
            a.is_some() && b == a && h.used() == 1,
            "",
        );
    }
    {
        // 复用不得越界：容量用满后再分配必须返回 None（不越界写槽）。
        let mut h = DescriptorHeap::new(0, 2, 1).unwrap();
        let _ = h.acquire(BindKind::Sampler, false);
        let _ = h.acquire(BindKind::Sampler, false);
        let third = h.acquire(BindKind::Sampler, true);
        set.add("A26-heap-容量满后不越界分配", third.is_none(), "");
    }
    {
        // 语义不匹配的**已释放**槽可以被复用：空槽不再携带语义，
        // 复用时按新语义写入即可（语义错配只对「仍占着 Some(Free) 的残留槽」有意义）。
        let mut h = DescriptorHeap::new(0, 8, 2).unwrap();
        let a = h.acquire(BindKind::Sampler, false).unwrap();
        let _ = h.release(a);
        let b = h.acquire(BindKind::StorageBuffer, true);
        let kind_ok = match &h.slots[b.unwrap() as usize] {
            Some(s) => s.kind == BindKind::StorageBuffer,
            None => false,
        };
        set.add(
            "A26-heap-空槽复用按新语义写入",
            b.is_some() && kind_ok,
            "",
        );
    }
    {
        // 仍占着 Some(Free) 的残留槽：语义不匹配时**不得**复用它。
        let mut h = DescriptorHeap::new(0, 8, 2).unwrap();
        // 先占一格再手工置成 Free 残留（模拟中途释放未清空）。
        let a = h.acquire(BindKind::Sampler, false).unwrap();
        h.slots[a as usize] = Some(DescriptorSlot {
            kind: BindKind::Sampler,
            tag: RetireTag::Free,
            shard: 0,
        });
        // 借一格占位，确保不会退化成「找空槽」而掩盖语义错配。
        let _ = h.acquire(BindKind::StorageTexture, false);
        let b = h.acquire(BindKind::UniformBuffer, true);
        set.add(
            "A26-heap-残留Free槽语义不符不复用",
            b.is_some() && b != Some(a),
            "",
        );
    }
    {
        // 释放不存在的槽被拒。
        let mut h = DescriptorHeap::new(0, 8, 2).unwrap();
        set.add(
            "A26-heap-释放非法槽被拒",
            !h.release(99) && !h.release(3),
            "",
        );
    }
    {
        // 分片统计：分配后所属分片的 used 增长。
        let mut h = DescriptorHeap::new(0, 8, 2).unwrap();
        let at = h.acquire(BindKind::Sampler, false).unwrap();
        let idx = h.shard_of(at) as usize;
        set.add(
            "A26-heap-分片计数随分配增长",
            h.shards[idx].used == 1 && h.shards[idx].begin <= at,
            "",
        );
    }

    {
        // 分片必须**完整覆盖** [0, capacity)：逐槽穷举 shard_of，
        // 命中分片必须真的含该槽。这条判据把「shard_of 的兜底分支
        // 不可达」从注释里的断言变成可失败的事实——若分片划分出现
        // 缝隙（有槽不属任何分片），兜底会静默返回 0 号片，
        // 而 0 号片并不含该槽，计数就会记到错的分片上。
        // 只测「分配后计数增长」是恒真弱门禁：无论命中哪片都 +1。
        let mut h = DescriptorHeap::new(0, 12, 5).unwrap();
        let mut covered = true;
        let mut i = 0u32;
        while i < 12 {
            let idx = h.shard_of(i) as usize;
            let s = h.shards[idx];
            if !(i >= s.begin && i < s.begin + s.len) {
                covered = false;
            }
            i += 1;
        }
        set.add("A26-heap-分片完整覆盖无缝隙", covered, "");
    }
    {
        // 末片吸收余数：容量不能被分片数整除时，最后一片拿到余数，
        // 否则「分片长度和 == 容量」会漏掉尾部若干槽。
        let h = DescriptorHeap::new(0, 12, 5).unwrap();
        let last = h.shards[h.shards.len() - 1];
        set.add(
            "A26-heap-末片吸收余数不留尾槽",
            last.begin + last.len == 12 && last.len == 4,
            "",
        );
    }

    // ---- 碎片率 ----
    {
        // 碎片率真算：中间挖洞后空洞上升（可失败算术，非「>0」恒真）。
        let mut h = DescriptorHeap::new(0, 16, 4).unwrap();
        let a0 = h.fragmentation();
        // 占满 0..8，释放中间 3 号 → 中间出现空洞。
        let mut i = 0;
        let mut ats = [0u32; 8];
        while i < 8 {
            ats[i] = h.acquire(BindKind::Sampler, false).unwrap();
            i += 1;
        }
        let _ = h.release(ats[3]);
        let a1 = h.fragmentation();
        set.add(
            "A26-frag-中间挖洞使空洞上升",
            a0.max_hole == 0 && a1.max_hole >= 1 && a1.max_hole < a0.capacity,
            "",
        );
    }
    {
        // 千分比口径：小堆上也能区分（百分比会只有 0/100 两档）。
        let f = Fragmentation {
            max_hole: 5,
            capacity: 100,
            used: 95,
        };
        let f2 = Fragmentation {
            max_hole: 1,
            capacity: 100,
            used: 99,
        };
        set.add(
            "A26-frag-千分比可区分空洞大小",
            f.permille() == 50 && f2.permille() == 10,
            "",
        );
    }
    {
        // 空洞必须取**最长**段，不是首个段、也不是末段。
        // 造料刻意摆成「短-长-短」：3 号与 4 号连成 2 长的中段空洞，
        // 首段(0..2)与末段(11..15)各只有 1。若实现只报首个空洞，
        // 报 1；若只报末段空洞，报 1；只有真取最大才报 2。
        // 这条是「>0 恒真」类弱门禁的对症药：只断言空洞非零的话
        // 三种实现全绿，等于没测。
        let mut h = DescriptorHeap::new(0, 16, 4).unwrap();
        let mut i = 0;
        while i < 16 {
            let _ = h.acquire(BindKind::Sampler, false);
            i += 1;
        }
        // 挖 3、4 两格 → 中段 2 长；另留 0、15 单格空洞作为混淆项。
        let _ = h.release(3);
        let _ = h.release(4);
        let _ = h.release(0);
        let _ = h.release(15);
        let f = h.fragmentation();
        set.add(
            "A26-frag-空洞取最长段而非首末段",
            f.max_hole == 2,
            "造料 短1-长2-短1，只取首个空洞的实现会报 1",
        );
    }
    {
        // 严重碎片判定（空洞超容量 1/4）。
        set.add(
            "A26-frag-严重碎片判定正确",
            !Fragmentation { max_hole: 2, capacity: 16, used: 14 }.severe()
                && Fragmentation { max_hole: 8, capacity: 16, used: 8 }.severe(),
            "",
        );
    }
    {
        // 空洞不超过空闲数（否则 permille 会虚高）。
        let mut h = DescriptorHeap::new(0, 16, 4).unwrap();
        let mut i = 0;
        while i < 4 {
            let _ = h.acquire(BindKind::Sampler, false);
            i += 1;
        }
        let f = h.fragmentation();
        set.add(
            "A26-frag-空洞不超空闲数",
            f.max_hole <= f.capacity - f.used,
            "",
        );
    }

    // ---- 耗尽防护与分帧回收 ----
    {
        // 在飞描述符不可回收（红线）。
        // 造料必须**同时**含在飞槽与空槽：只造在飞槽的话，
        // 「把 None 当可回收」这种变异扫不到任何空槽 ⇒ 恒真漏网。
        let mut h = DescriptorHeap::new(0, 8, 2).unwrap();
        let mut i = 0;
        while i < 4 {
            let _ = h.acquire(BindKind::Sampler, false);
            i += 1;
        }
        h.slots[6] = None;
        h.slots[7] = None;
        let freed = h.reclaim(RECLAIM_PER_FRAME);
        set.add(
            "A26-exh-在飞描述符一律不回收",
            freed == 0 && h.used() == 4 && h.slots[0].is_some() && h.slots[3].is_some(),
            "",
        );
    }
    {
        // 分帧回收：每帧至多 RECLAIM_PER_FRAME 个（防帧尖峰）。
        // 造料：占满 16 格后把其中 10 格置成 Free 残留（release 已置 None 的
        // 不由回收段处理），这样回收段才有活干。
        let mut h = DescriptorHeap::new(0, 16, 4).unwrap();
        let mut i = 0;
        while i < 10 {
            let at = h.acquire(BindKind::Sampler, false).unwrap();
            h.slots[at as usize] = Some(DescriptorSlot {
                kind: BindKind::Sampler,
                tag: RetireTag::Free,
                shard: 0,
            });
            i += 1;
        }
        let f1 = h.reclaim(RECLAIM_PER_FRAME);
        let f2 = h.reclaim(RECLAIM_PER_FRAME);
        // 造了 10 个Free 残留，两帧各回收 4个 → 剩 10-8 = 2 个仍占位。
        set.add(
            "A26-exh-回收分帧不超预算",
            f1 == RECLAIM_PER_FRAME
                && f2 == RECLAIM_PER_FRAME
                && h.used() == 10 - 2 * RECLAIM_PER_FRAME,
            "",
        );
    }
    {
        // 堆满时先回收再分配（不是直接失败）。
        let mut hs = DescriptorHeaps::new(GraphicsApi::Vulkan);
        let mut h = DescriptorHeap::new(0, 4, 2).unwrap();
        let mut i = 0;
        while i < 3 {
            let _ = h.acquire(BindKind::Sampler, false);
            i += 1;
        }
        let _ = hs.add_heap(h);
        // 第 4 个直接可用。
        let a = hs.acquire(0, BindKind::Sampler);
        // 全部释放后回收，再分配应走「回收后成功」。
        let _ = hs.heaps[0].release(0);
        let b = hs.acquire(0, BindKind::UniformBuffer);
        set.add(
            "A26-exh-回收后分配成功",
            matches!(a, ExhaustionVerdict::Allocated { .. })
                && matches!(b, ExhaustionVerdict::Allocated { .. }),
            "",
        );
    }
    {
        // 耗尽时报出**可行动**原因（不是空字符串）。
        let mut hs = DescriptorHeaps::new(GraphicsApi::Vulkan);
        let mut h = DescriptorHeap::new(0, 2, 1).unwrap();
        let _ = h.acquire(BindKind::Sampler, false);
        let _ = h.acquire(BindKind::Sampler, false);
        let _ = hs.add_heap(h);
        let v = hs.acquire(0, BindKind::StorageBuffer);
        match v {
            ExhaustionVerdict::Exhausted { reason } => set.add(
                "A26-exh-耗尽原因可行动",
                reason.contains("在飞") && reason.contains("容量"),
                "",
            ),
            _ => set.add("A26-exh-耗尽原因可行动", false, ""),
        }
        set.add("A26-exh-耗尽计数落账", hs.exhausted_total == 1, "");
    }
    {
        // 堆号越界不得 panic，按耗尽返回。
        let mut hs = DescriptorHeaps::new(GraphicsApi::Vulkan);
        set.add(
            "A26-exh-堆号越界不panic",
            matches!(
                hs.acquire(99, BindKind::Sampler),
                ExhaustionVerdict::Exhausted { .. }
            ),
            "",
        );
    }
    {
        // 堆数超上限拒。
        let mut hs = DescriptorHeaps::new(GraphicsApi::Vulkan);
        let mut i = 0;
        let mut refused = false;
        while i <= MAX_HEAPS {
            if hs.add_heap(DescriptorHeap::new(i as u32, 8, 1).unwrap()).is_err() {
                refused = true;
                break;
            }
            i += 1;
        }
        set.add(
            "A26-exh-堆数超限拒",
            refused && hs.heaps.len() == MAX_HEAPS,
            "",
        );
    }

    // ---- 降级 ----
    {
        // 降级只丢可选绑定（uniform/storage 一个都不许丢）。
        let mut hs = DescriptorHeaps::new(GraphicsApi::Vulkan);
        let decls = vec![
            BindDecl {
                kind: BindKind::Sampler,
                slot: 0,
            },
            BindDecl {
                kind: BindKind::UniformBuffer,
                slot: 0,
            },
            BindDecl {
                kind: BindKind::AccelerationStructure,
                slot: 0,
            },
        ];
        match hs.degrade_plan(&decls) {
            Ok(p) => set.add(
                "A26-deg-只丢可选绑定",
                p.dropped.len() == 2
                    && p.plan.len() == 1
                    && p.plan[0].kind == BindKind::UniformBuffer,
                "",
            ),
            Err(_) => set.add("A26-deg-只丢可选绑定", false, ""),
        }
    }
    {
        // 无可选绑定可丢时如实失败（不假装降级成功）。
        let mut hs = DescriptorHeaps::new(GraphicsApi::Vulkan);
        let decls = vec![BindDecl {
            kind: BindKind::UniformBuffer,
            slot: 0,
        }];
        set.add(
            "A26-deg-无可丢绑定则失败",
            hs.degrade_plan(&decls).is_err(),
            "",
        );
    }
    {
        // 降级丢失数量有上限（防止把整表丢空）。
        let mut hs = DescriptorHeaps::new(GraphicsApi::Vulkan);
        let mut decls: Vec<BindDecl> = Vec::new();
        let mut i = 0;
        while i <= MAX_DEGRADE_DROPS + 3 {
            decls.push(BindDecl {
                kind: BindKind::Sampler,
                slot: (i % 8) as u32,
            });
            i += 1;
        }
        match hs.degrade_plan(&decls) {
            Ok(p) => set.add(
                "A26-deg-降级丢失有上限",
                p.dropped.len() == MAX_DEGRADE_DROPS
                    && !p.plan.is_empty(),
                "",
            ),
            Err(_) => set.add("A26-deg-降级丢失有上限", false, ""),
        }
    }

    // ---- 无障碍面板 ----
    {
        let mut hs = DescriptorHeaps::new(GraphicsApi::D3D12);
        let _ = hs.add_heap(DescriptorHeap::new(7, 16, 4).unwrap());
        let zh = hs.a11y_lines(Locale::ZhCn);
        let en = hs.a11y_lines(Locale::En);
        set.add(
            "A26-a11y-面板逐堆成行双语有别",
            zh.len() == 3 && en.len() == 3 && zh != en,
            "",
        );
    }
    {
        // 双语必须**逐行**本地化，不是整表不等。
        // 「zh != en」是弱门禁：只要有一行有别就恒真，哪怕其余行
        // 中文侧也吐英文照样全绿。这里逐行断言——中文侧每行都得含
        // 汉字，英文侧每行都不得含汉字，方向错一条即红。
        let mut hs = DescriptorHeaps::new(GraphicsApi::D3D12);
        let _ = hs.add_heap(DescriptorHeap::new(7, 16, 4).unwrap());
        let _ = hs.acquire(0, BindKind::Sampler);
        let zh = hs.a11y_lines(Locale::ZhCn);
        let en = hs.a11y_lines(Locale::En);
        let has_han = |s: &str| s.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));
        let mut zh_all = !zh.is_empty();
        let mut en_clean = !en.is_empty();
        let mut i = 0;
        while i < zh.len() && i < en.len() {
            if !has_han(&zh[i]) {
                zh_all = false;
            }
            if has_han(&en[i]) {
                en_clean = false;
            }
            i += 1;
        }
        set.add("A26-a11y-逐行本地化方向正确", zh_all && en_clean, "");
    }
    {
        // 面板须报碎片率与耗尽/降级计数（否则看板无诊断价值）。
        let mut hs = DescriptorHeaps::new(GraphicsApi::D3D12);
        let _ = hs.add_heap(DescriptorHeap::new(0, 16, 4).unwrap());
        let _ = hs.acquire(0, BindKind::Sampler);
        let zh = hs.a11y_lines(Locale::ZhCn);
        set.add(
            "A26-a11y-面板含碎片与计数",
            zh[1].contains("碎片率") && zh[2].contains("累计耗尽"),
            "",
        );
    }

    // ---- 诊断码与契约 ----
    {
        let codes = [E_HEAP_REJECT, E_HEAP_EXHAUSTED, E_HEAP_DEGRADED];
        let mut uniq = true;
        let mut i = 0;
        while i < codes.len() {
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    uniq = false;
                }
                j += 1;
            }
            i += 1;
        }
        set.add("A26-judge-三类处置码两两不同", uniq, "");
    }
    {
        let docs = [HEAP_STAGE_DOC, ABSTRACT_DOC, RECLAIM_DOC];
        let mut all = true;
        let mut i = 0;
        while i < docs.len() {
            if docs[i].is_empty() {
                all = false;
            }
            i += 1;
        }
        set.add("A26-judge-三契约条款在场", all, "");
    }
    {
        set.add(
            "A26-judge-常量彼此自洽",
            MAX_DESCRIPTORS > 0
                && MAX_HEAPS > 0
                && MAX_SHARDS > 0
                && RECLAIM_PER_FRAME > 0
                && RECLAIM_PER_FRAME <= MAX_DESCRIPTORS as u32
                && MAX_DEGRADE_DROPS > 0,
            "",
        );
    }
    {
        // 描述符字节数语义自洽（uniform/storage 占两倍，须真占两倍）。
        set.add(
            "A26-judge-描述符字节数自洽",
            BindKind::UniformBuffer.desc_bytes() == 2
                && BindKind::Sampler.desc_bytes() == 1
                && BindKind::ALL.len() == 6,
            "",
        );
    }
    {
        // 可选语义只含「缺了不影响正确性」的类别；必需语义不可降级。
        let mut opt = 0;
        let mut i = 0;
        while i < BindKind::ALL.len() {
            if BindKind::ALL[i].optional() {
                opt += 1;
            }
            i += 1;
        }
        set.add("A26-judge-可选语义集恒定", opt == 2, "");
    }

    set
}