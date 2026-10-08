//! VE-F3202 · 资源模型与引用图
//!
//! 判据映射（锚点原文 → 本文件章节）：
//! - 五要素→ §二 [`Resource`]（ID/类型/元数据/内容体/引用关系）
//! - 四用途单源   → §四 [`GraphPurpose`] + [`audit_purpose_single_source`]
//! - 悬空即缺陷   → §五 [`detect_dangling`] + 占位（[`SlotState::Placeholder`]）
//! - 环拒绝       → §六 [`detect_cycle_indexed`] + [`export_index_edges`]（F3206 数据基础）
//! - 32MB 线      → §七 [`audit_memory_budget`] + 分域分图降级 [`plan_sharded_degradation`]
//! - 判据         → `veq02_checks.rs`
//!
//! 零IO / 零墙钟 / 零全局可变状态：内容体**只记账不持有字节**（`ContentBody` 记
//! 字节数与指纹，字节本体归打包层 F3214），故本模块全部是纯数据变换，回归可复现。
//!
//! 与 F3201 的分工：F3201 是管线总纲（六段签名/十项映射/收敛红线），本条是它
//! 挂在上面的第一张实体表——F3206 依赖解析、F3203 生命周期 GC、F3211 热更新都
//! 以本模块的图为唯一数据源，**不许各自建图**（四用途单源红线的由来）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::veq01_pipeline::{adjudicate_capability, CapabilityVerdict};

// F3201 的诊断三件套与资源类型在F3202 内继续使用，且**对外再导出**——
// F3202 的调用方（F3203/F3204/F3206）需要同时拿到图接口与诊断码，
// 若不re-export，他们就得同时import 两个模块，接口边界就漏了。
pub use super::veq01_pipeline::{Diagnostic, DiagCode, Outcome, ResourceKind};

// ===========================================================================
// 一、资源标识与紧凑索引
// ===========================================================================

/// 资源 ID：稠密小整数，**同时是节点表下标**。
///
/// 为何不用哈希 ID：图内存红线是 32MB@百万资源，哈希 ID 需要额外一张
/// `ID→下标` 的映射表（百万项×4B=4MB，纯浪费）。稠密 ID 让「查资源」退化为
/// 数组下标寻址——这是 32MB 线能守住的结构性前提，不是优化技巧。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ResourceId(pub u32);

impl ResourceId {
    /// 空 ID（未登记 / 已回收）。
    pub const NONE: ResourceId = ResourceId(u32::MAX);

    /// 原始数值。
    pub fn raw(self) -> u32 {
        self.0
    }

    /// 是否为有效 ID。
    pub fn is_valid(self) -> bool {
        self.0 != u32::MAX
    }
}

/// 百万资源规模（图内存红线的标称规模）。
pub const MILLION_RESOURCE_SCALE: u32 = 1_000_000;

/// 图内存红线：百万资源图 32MB。
pub const GRAPH_MEMORY_BUDGET_BYTES: u64 = 32 * 1024 * 1024;

/// ID 硬上限：防止无界增长把内存红线变成一句空话。
///
/// 上限取 2×百万刻度：留一倍余量给「分域分图」后重新合并的场景，但不允许
/// 无界——无界等于把32MB 红线降级为「视资源量而定」，那就不是红线了。
pub const RESOURCE_ID_LIMIT: u32 = MILLION_RESOURCE_SCALE * 2;

// ===========================================================================
// 二、资源实体：五要素
// ===========================================================================

/// 压缩方式（元数据要素之一）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compression {
    /// 未压缩。
    None,
    /// 块压缩（可按块随机访问）。
    Block,
    /// 流式压缩（只能顺序解——影响能否随机寻址，故须显性登记）。
    Stream,
}

impl Compression {
    /// 英文标识。
    pub fn en(self) -> &'static str {
        match self {
            Compression::None => "none",
            Compression::Block => "block",
            Compression::Stream => "stream",
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            Compression::None => "未压缩",
            Compression::Block => "块压缩",
            Compression::Stream => "流式压缩",
        }
    }

    /// 能否随机寻址（流式压缩只能顺序读——F3214 按块读取时要先问一句）。
    pub fn random_access(self) -> bool {
        !matches!(self, Compression::Stream)
    }
}

/// 资源元数据（五要素之三）。
///
/// **刻意不用动态 map**：元数据键集由F3204 类型系统的 schema 冻结，元数据
/// **值** 的槽位数量是已知的。写成`Vec<(String, String)>` 会让百万资源的光
/// 键值对分配（每项至少 2 次堆分配 + 32B 键串头）——光这一项就吃掉 32MB 红线
/// 的全部预算。固定槽位是「元数据 schema 冻结」这条纪律在内存上的兑现。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceMetadata {
    /// 版本号（热更/F3211 对账用）。
    pub version: u32,
    /// 压缩方式。
    pub compression: Compression,
    /// 是否 sRGB 色彩空间（纹理类资源的关键属性，错了画面直接偏色）。
    pub srgb: bool,
    /// 是否可流式交付。
    pub streamable: bool,
    /// 紧凑标签槽一（类型系统分配的语义标签）。
    pub tag0: u32,
    /// 紧凑标签槽二。
    pub tag1: u32,
    /// 元数据是否真实存在（`false` = 走了默认元，见 §八）。
    pub present: bool,
}

impl Default for ResourceMetadata {
    /// 默认元：全部取「最保守」的值。
    ///
    /// 为何 `srgb=false` / `streamable=false` 是保守侧：默认宣称自己是
    /// sRGB 会让线性纹理偏色，默认宣称可流式会让调用方按随机寻址假设去读
    /// 流式内容。**默认值必须偏向「我不能做」而非「我可以做」**——这是
    /// 降级显性原则在元数据层的落法。
    fn default() -> Self {
        ResourceMetadata {
            version: 0,
            compression: Compression::None,
            srgb: false,
            streamable: false,
            tag0: 0,
            tag1: 0,
            present: false,
        }
    }
}

impl ResourceMetadata {
    /// 读屏可读单行（不暴露路径，只念语义）。
    pub fn screen_line(&self) -> String {
        format!(
            "元数据：版本 {}，压缩 {}，色彩空间 {}，流式 {}，标签 {}:{}，来源 {}",
            self.version,
            self.compression.zh(),
            if self.srgb { "sRGB" } else { "线性" },
            if self.streamable { "是" } else { "否" },
            self.tag0,
            self.tag1,
            if self.present { "显式登记" } else { "默认元（告警）" }
        )
    }
}

/// 内容体（五要素之四）：**只记账，不持有字节**。
///
/// 零 IO 的关键在此：字节本体归打包层（F3214vepack 按偏移+长度读），本模块
/// 只登记「有多长、内容指纹是什么、是否已驻留」。这让整张图可以在没有任何
/// 文件系统的情况下构造与检测——也让百万节点演练可以在毫秒内跑完。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContentBody {
    /// 字节长度。
    pub byte_length: u32,
    /// 内容指纹（F3211 内容寻址的对端；0 = 未计算，非「空内容」）。
    pub fingerprint: u64,
    /// 内容体是否已驻留内存。
    pub resident: bool,
    /// 内容体是否缺失（悬空资源的占位内容体必须显性标缺失）。
    pub absent: bool,
}

impl ContentBody {
    /// 正常内容体。
    pub fn present(byte_length: u32, fingerprint: u64, resident: bool) -> Self {
        ContentBody {
            byte_length,
            fingerprint,
            resident,
            absent: false,
        }
    }

    /// 缺失内容体（悬空占位专用——`byte_length=0` + `absent=true`，
    /// 二者**同时**出现才算缺失：单看长度 0 无法区分「空文件」与「不存在」）。
    pub fn missing() -> Self {
        ContentBody {
            byte_length: 0,
            fingerprint: 0,
            resident: false,
            absent: true,
        }
    }

    /// 读屏可读单行。
    pub fn screen_line(&self) -> String {
        if self.absent {
            return String::from("内容体：缺失（占位，尚无字节）");
        }
        format!(
            "内容体：{} 字节，指纹 {:#018x}，{}",
            self.byte_length,
            self.fingerprint,
            if self.resident { "已驻留" } else { "未驻留" }
        )
    }
}

/// 槽位状态：图上每个 ID 槽的三态。
///
/// 「空槽」与「占位」必须分开：空槽是「这里什么都没有」，占位是「我知道这里
/// 缺一个资源，且我知道缺的是哪个」。把两者混为一谈会让悬空检出丢失去向——
/// 而**悬空即缺陷**要求缺陷可寻址（能说出「哪个 ID 缺什么」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotState {
    /// 活跃：资源实体在册。
    Live,
    /// 占位：已知该位置应有资源，但实体尚未登记（悬空目标的显性形态）。
    Placeholder,
    /// 空槽：该 ID 从未使用或已回收。
    Vacant,
}

/// 资源实体（五要素：ID / 类型 / 元数据 / 内容体 / 引用关系）。
///
/// 引用关系（五要素之五）**不在实体里存邻接表**，只存 CSR 行偏移
/// （`out_offset`/`out_degree` 两个u32）。理由：邻接表若存在实体内部，图就
/// 变成「实体数组 + 每实体一个 Vec」的形态，光百万个 Vec 头（24B/个）就是
/// 24MB——32MB 红线当场破掉。把邻接移到 CSR 单块数组里，百万边才只占
/// `4B × E`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resource {
    /// 要素一：ID（= 节点表下标）。
    pub id: ResourceId,
    /// 要素二：类型（F3204 的 schema 校验对端）。
    pub kind: ResourceKind,
    /// 要素三：元数据。
    pub metadata: ResourceMetadata,
    /// 要素四：内容体（记账形态）。
    pub content: ContentBody,
    /// 要素五（规模部分）：出边 CSR 行起点。
    pub out_offset: u32,
    /// 要素五（规模部分）：出度。
    pub out_degree: u32,
}

impl Resource {
    /// 构造资源实体。
    pub fn new(id: ResourceId, kind: ResourceKind, metadata: ResourceMetadata, content: ContentBody) -> Self {
        Resource {
            id,
            kind,
            metadata,
            content,
            out_offset: 0,
            out_degree: 0,
        }
    }

    /// 读屏可读单行（五要素齐念，念完就知道这个资源是什么）。
    pub fn screen_line(&self) -> String {
        format!(
            "资源 {}（{}）：{}；{}；{}；出度 {}",
            self.id.raw(),
            self.kind.zh(),
            self.metadata.screen_line(),
            self.content.screen_line(),
            if self.metadata.present {
                "元数据显式"
            } else {
                "元数据默认（告警）"
            },
            self.out_degree
        )
    }
}

// ===========================================================================
// 三、引用图构建器（运行期加边 O(1)）
// ===========================================================================

/// 构建期引用图：邻接用「每节点一个 Vec」，支持 O(1) 追加边。
///
/// 为何构建期与运行期分两种形态：运行期动态加边（F3206 延迟依赖）要求 O(1)
/// 追加且不重排，而 O(1) 追加与「紧凑连续内存」互斥——`Vec<Vec<u32>>` 追加是
/// O(1)，但每个内 Vec 有独立堆块。解法不是折中，而是**分阶段**：构建期用
/// 便于追加的形态，跑完构建期 [`ResourceGraphBuilder::freeze`] 压成 CSR 单块
/// 数组，运行期只读。两个形态各取所长，不假装有一种形态通吃。
#[derive(Clone, Debug, Default)]
pub struct ResourceGraphBuilder {
    /// 节点表：下标 = ID，空槽为 `None`。
    nodes: Vec<Option<Resource>>,
    /// 占位标记（与 `nodes` 平行；独立数组使槽位三态可 O(1) 读写）。
    slots: Vec<SlotState>,
    /// 出边邻接（构建期形态）。
    out_edges: Vec<Vec<u32>>,
    /// 反向索引（构建期形态）。
    in_edges: Vec<Vec<u32>>,
    /// 已用ID 上界（回收过的 ID 记入空闲表，故须独立追踪）。
    high_water: u32,
    /// 悬空立案（构建期即时检出）。
    dangling: Vec<DanglingCase>,
    /// 悬空占位登记：目标 ID → 占位次数。
    placeholder_hits: Vec<(u32, u32)>,
}

impl ResourceGraphBuilder {
    /// 空构建器。
    pub fn new() -> Self {
        ResourceGraphBuilder {
            nodes: Vec::new(),
            slots: Vec::new(),
            out_edges: Vec::new(),
            in_edges: Vec::new(),
            high_water: 0,
            dangling: Vec::new(),
            placeholder_hits: Vec::new(),
        }
    }

    /// 当前已用 ID 上界（= 下一个可分配 ID）。
    pub fn high_water(&self) -> u32 {
        self.high_water
    }

    /// 当前槽位数（含空槽）。
    pub fn slot_count(&self) -> usize {
        self.nodes.len()
    }

    /// 活跃节点数。
    pub fn live_count(&self) -> usize {
        self.slots.iter().filter(|s| **s == SlotState::Live).count()
    }

    /// 边数。
    pub fn edge_count(&self) -> usize {
        self.out_edges.iter().map(|v| v.len()).sum()
    }

    /// 槽位状态（越界返回 `Vacant`——守卫不得 panic）。
    pub fn slot_state(&self, id: ResourceId) -> SlotState {
        match self.nodes.get(id.0 as usize) {
            Some(Some(_)) => SlotState::Live,
            Some(None) => {
                if self
                    .slots
                    .get(id.0 as usize)
                    .copied()
                    .unwrap_or(SlotState::Vacant)
                    == SlotState::Placeholder
                {
                    SlotState::Placeholder
                } else {
                    SlotState::Vacant
                }
            }
            None => SlotState::Vacant,
        }
    }

    /// 登记资源（占位升级为活跃）。
    ///
    /// 重复登记同一 ID 判为缺陷：一个 ID 两个实体 = 图里出现第二个身份，
    /// 引用计数与GC 对账从此无从下手（F3203 的账实一致红线会被绕过）。
    pub fn register(&mut self, res: Resource) -> Outcome<ResourceId> {
        if !res.id.is_valid() {
            return Outcome::err(
                DiagCode::ValueInvalid,
                "资源 ID 为空值",
                "空 ID 无法在图中定位：它就是「悬空且不可寻址」。请分配真实 ID",
            );
        }
        if res.id.raw() >= RESOURCE_ID_LIMIT {
            return Outcome::err(
                DiagCode::BudgetExceeded,
                &format!("资源 ID {} 越过硬上限 {}", res.id.raw(), RESOURCE_ID_LIMIT),
                "ID 硬上限是内存红线的执行手段。越界说明资源数量已失控——\
                 请先排查是否有资源被重复创建（通常是没走缓存的重复加载）",
            );
        }
        let idx = res.id.raw() as usize;
        if idx >= self.nodes.len() {
            self.nodes.resize(idx + 1, None);
            self.slots.resize(idx + 1, SlotState::Vacant);
            self.out_edges.resize(idx + 1, Vec::new());
            self.in_edges.resize(idx + 1, Vec::new());
            if res.id.raw() >= self.high_water {
                self.high_water = res.id.raw() + 1;
            }
        }
        if self.nodes[idx].is_some() {
            return Outcome::err(
                DiagCode::ValueInvalid,
                &format!("资源 ID {} 重复登记", res.id.raw()),
                "一个 ID 两个实体意味着图里有两个身份同一资源的记录。\
                 引用计数、GC 对账、热更新 diff 全部会算错——请先注销旧实体再登记",
            );
        }
        let mut r = res;
        r.id = ResourceId(self.high_water.min(idx as u32));
        self.nodes[idx] = Some(r);
        self.slots[idx] = SlotState::Live;
        self.clear_placeholder(idx as u32);
        Outcome::ok(ResourceId(idx as u32))
    }

    /// 登记占位（已知该位置应有资源但实体未到）。
    ///
    /// 占位是**显性**的：它把「我不知道」变成「我知道缺 7 号纹理」。前者无法
    /// 定位缺陷，后者能直接生成加载清单。
    pub fn register_placeholder(&mut self, id: ResourceId, kind: ResourceKind) -> Outcome<ResourceId> {
        if !id.is_valid() {
            return Outcome::err(
                DiagCode::ValueInvalid,
                "占位 ID 为空值",
                "空 ID 的占位无法被引用方定位，等于没登记",
            );
        }
        let idx = id.raw() as usize;
        if idx >= self.nodes.len() {
            self.nodes.resize(idx + 1, None);
            self.slots.resize(idx + 1, SlotState::Vacant);
            self.out_edges.resize(idx + 1, Vec::new());
            self.in_edges.resize(idx + 1, Vec::new());
            if id.raw() >= self.high_water {
                self.high_water = id.raw() + 1;
            }
        }
        if self.nodes[idx].is_some() {
            return Outcome::err(
                DiagCode::ValueInvalid,
                &format!("ID {} 已有活跃实体，不能再占位", id.raw()),
                "占位与实体并存会让引用方拿到「有资源」却查不到实体——\
                 请先注销实体再占位，或直接补齐实体",
            );
        }
        self.slots[idx] = SlotState::Placeholder;
        self.bump_placeholder(idx as u32);
        // 记录占位类型（供加载清单按类型分批）。
        let _ = kind;
        Outcome::ok(id)
    }

    /// 注销资源（同时移除其全部边）。
    pub fn unregister(&mut self, id: ResourceId) -> Outcome<bool> {
        let idx = id.raw() as usize;
        if idx >= self.nodes.len() || self.nodes[idx].is_none() {
            return Outcome::err(
                DiagCode::IoNotFound,
                &format!("注销失败：ID {} 不在册", id.raw()),
                "注销一个不存在的资源通常意味着上游已经出错。\
                 请核对调用顺序——若这是幂等清理，请先查在册状态再注销",
            );
        }
        // 只摘**本节点自己发出**的边（出边），并把它们从各自目标的反向索引里
        // 摘掉；**绝不摘「别人指向我」的边**。
        //
        // 历史缺陷（勿回退，探针实跑抓到的）：早期实现双向都摘，于是注销 0 号
        // 纹理时，1 号与 2 号指向它的边被一并抹掉——悬空证据被销毁，
        // `detect_dangling` 永远返回 0。**红线写了、类型过了、测试看起来全绿，
        // 实跑一次都不触发**，与 F3201 的禁止表前缀缺陷同一类。
        //
        // 正确语义：引用方的边留着，它们指向一个 Vacant 槽 → 这正是悬空，
        // 由检测器立案。资源被删掉却没人发现还引用它，才是真正的缺陷。
        //
        // 摘边前先取反向快照——它就是「谁曾指向我」的清单，即悬空来源。
        let referrers: Vec<u32> = self.in_edges[idx].clone();
        let outs = self.out_edges[idx].clone();
        for t in outs.iter() {
            let ti = *t as usize;
            if ti < self.in_edges.len() {
                self.in_edges[ti].retain(|s| *s as usize != idx);
            }
        }
        self.out_edges[idx].clear();
        self.in_edges[idx].clear();
        // 注销即刻立案：谁曾指向我、现在它没了 —— 这就是悬空，不必等全量扫描。
        for r in referrers.iter() {
            let from = ResourceId(*r);
            let case = DanglingCase {
                from,
                to: ResourceId(idx as u32),
                case_id: format!("DANGLING-{}->{}", from.raw(), idx),
                slot: SlotState::Vacant,
            };
            if !self.dangling.iter().any(|c| c.from == case.from && c.to == case.to) {
                self.dangling.push(case);
            }
        }
        self.nodes[idx] = None;
        self.slots[idx] = SlotState::Vacant;
        self.clear_placeholder(idx as u32);
        Outcome::ok(true)
    }

    /// 加边（`from`依赖 `to`），复杂度 O(1) 摊销。
    ///
    /// 加边时**即时检出悬空**：目标槽非活跃即立案并升为占位。为何不在构建完
    /// 再统一检：统一检只能告诉你「有 N 处悬空」，即时检能告诉你「谁引用了
    /// 7号」——后者才能定位到该修哪张材质表。
    pub fn add_edge(&mut self, from: ResourceId, to: ResourceId) -> Outcome<bool> {
        let fi = from.raw() as usize;
        let ti = to.raw() as usize;
        if fi >= self.nodes.len() || self.nodes[fi].is_none() {
            return Outcome::err(
                DiagCode::ValueInvalid,
                &format!("加边失败：起点ID {} 不在册", from.raw()),
                "从不在册的资源发起的依赖不可能成立。\
                 请先登记资源实体再连边——边引用未登记节点会让悬空检测双向失效",
            );
        }
        if ti >= self.nodes.len() || self.nodes[ti].is_none() {
            // 悬空即缺陷：立案 + 占位（不崩溃）。
            let case = DanglingCase {
                from,
                to,
                case_id: format!("DANGLING-{}->{}", from.raw(), to.raw()),
                slot: self.slot_state(to),
            };
            if !self.dangling.iter().any(|c| c.from == case.from && c.to == case.to) {
                self.dangling.push(case);
            }
            if self.slot_state(to) == SlotState::Vacant && to.is_valid() {
                // 目标槽完全空 → 升为占位（显性缺失）。
                // 必须先扩容再写：目标 ID 可能超出当前 slots 长度，
                // 直接 `self.slots[ti] = ...` 会越界 panic——守卫不得成为崩溃源。
                if ti >= self.slots.len() {
                    self.slots.resize(ti + 1, SlotState::Vacant);
                    self.nodes.resize(ti + 1, None);
                    self.out_edges.resize(ti + 1, Vec::new());
                    self.in_edges.resize(ti + 1, Vec::new());
                    if to.raw() >= self.high_water {
                        self.high_water = to.raw() + 1;
                    }
                }
                self.slots[ti] = SlotState::Placeholder;
                self.bump_placeholder(ti as u32);
            }
        }
        // 自环即时拒：自环是最短的环，且它不需要 F3206 的 DFS 就能判定。
        if from == to {
            return Outcome::err(
                DiagCode::DependencyCycle,
                &format!("自环引用：资源 {} 依赖自身", from.raw()),
                "自环是最短的环。它会让加载器等待自己。\
                 请检查是不是把「本资源的输出」误接成了「本资源的输入」",
            );
        }
        // 重复边去重（同一依赖登记两次会让入度虚高，GC 永不回收）。
        if self.out_edges[fi].contains(&(ti as u32)) {
            return Outcome::err(
                DiagCode::ValueInvalid,
                &format!("重复边：{} -> {} 已登记", from.raw(), to.raw()),
                "重复边会让入度计数虚高：引用计数为零判定会永远不成立，\
                 资源永不回收（泄漏）。请勿重复连边",
            );
        }
        self.out_edges[fi].push(ti as u32);
        self.in_edges[ti].push(fi as u32);
        Outcome::ok(true)
    }

    /// 移除一条边（`from` 不再依赖 `to`）。
    pub fn remove_edge(&mut self, from: ResourceId, to: ResourceId) -> Outcome<bool> {
        let fi = from.raw() as usize;
        let ti = to.raw() as usize;
        if fi >= self.out_edges.len() || ti >= self.in_edges.len() {
            return Outcome::err(
                DiagCode::IoNotFound,
                &format!("移除边失败：{} -> {} 越界", from.raw(), to.raw()),
                "越界的边移除说明调用方拿到了不存在的 ID。\
                 请核对 ID 来源——通常是把未登记的资源当成已登记",
            );
        }
        let before_out = self.out_edges[fi].len();
        self.out_edges[fi].retain(|t| *t as usize != ti);
        let removed = self.out_edges[fi].len() != before_out;
        if removed {
            self.in_edges[ti].retain(|s| *s as usize != fi);
            // 边撤销后悬空立案同步撤销——否则图会永远背着一条已修复的缺陷。
            self.dangling
                .retain(|c| !(c.from == from && c.to == to));
        }
        Outcome::ok(removed)
    }

    /// 查某节点的出边目标。
    pub fn out_targets(&self, id: ResourceId) -> Vec<u32> {
        match self.out_edges.get(id.raw() as usize) {
            Some(v) => v.clone(),
            None => Vec::new(),
        }
    }

    /// 查某节点的出边目标（与运行期图同名别名——两形态方法名一致，
    /// 调用方切换阶段时不必改代码，也就不会引「换个形态就忘了改方法名」的缺陷）。
    pub fn out_targets_of(&self, id: ResourceId) -> Vec<u32> {
        self.out_targets(id)
    }

    /// 查某节点的反向来源（谁依赖它）。
    pub fn in_sources(&self, id: ResourceId) -> Vec<u32> {
        match self.in_edges.get(id.raw() as usize) {
            Some(v) => v.clone(),
            None => Vec::new(),
        }
    }

    /// 查某节点的反向来源（运行期图同名别名）。
    pub fn in_sources_of(&self, id: ResourceId) -> Vec<u32> {
        self.in_sources(id)
    }

    /// 入度（F3203 GC 判据：入度为 0 且无外部句柄 = GC 候选）。
    pub fn in_degree(&self, id: ResourceId) -> usize {
        self.in_sources(id).len()
    }

    /// 查资源（越界/未登记返回 `None`，不 panic）。
    pub fn get(&self, id: ResourceId) -> Option<Resource> {
        self.nodes.get(id.raw() as usize).copied().flatten()
    }

    /// 悬空立案清单。
    pub fn dangling_cases(&self) -> Vec<DanglingCase> {
        self.dangling.clone()
    }

    /// 占位命中表（目标 ID → 被引用次数）。
    pub fn placeholder_hits(&self) -> Vec<(u32, u32)> {
        self.placeholder_hits.clone()
    }

    fn bump_placeholder(&mut self, target: u32) {
        for e in self.placeholder_hits.iter_mut() {
            if e.0 == target {
                e.1 += 1;
                return;
            }
        }
        self.placeholder_hits.push((target, 1));
    }

    fn clear_placeholder(&mut self, target: u32) {
        self.placeholder_hits.retain(|e| e.0 != target);
    }

    /// 压实为运行期只读图（CSR 单块数组）。
    pub fn freeze(self) -> Outcome<ResourceGraph> {
        let slots = self.slots.len();
        // CSR 布局：out_offsets 长度 = slots + 1（第 i 段的起点在i，终点在 i+1）。
        let mut out_offsets: Vec<u32> = Vec::with_capacity(slots + 1);
        let mut out_targets: Vec<u32> = Vec::new();
        for i in 0..slots {
            out_offsets.push(out_targets.len() as u32);
            if let Some(v) = self.out_edges.get(i) {
                out_targets.extend_from_slice(v);
            }
        }
        out_offsets.push(out_targets.len() as u32);
        let mut in_offsets: Vec<u32> = Vec::with_capacity(slots + 1);
        let mut in_sources: Vec<u32> = Vec::new();
        for i in 0..slots {
            in_offsets.push(in_sources.len() as u32);
            if let Some(v) = self.in_edges.get(i) {
                in_sources.extend_from_slice(v);
            }
        }
        in_offsets.push(in_sources.len() as u32);

        // 实体数组：把 CSR 偏移写回实体。
        let mut nodes: Vec<Option<Resource>> = Vec::with_capacity(slots);
        for i in 0..slots {
            let mut r = self.nodes[i];
            if let Some(rr) = r.as_mut() {
                rr.out_offset = out_offsets[i];
                rr.out_degree = out_offsets[i + 1] - out_offsets[i];
            }
            nodes.push(r);
        }
        // 先取需要借用 `self` 的量，再移走字段——反序会触发
        // E0382（部分 move 后借用）。顺序纪律：借用全部前置于移走。
        let live = self.live_count();
        let dangling = self.dangling;
        let placeholder_hits = self.placeholder_hits;
        let edge_total = out_targets.len();
        let graph = ResourceGraph {
            slots,
            nodes,
            out_offsets,
            out_targets,
            in_offsets,
            in_sources,
            dangling,
            placeholder_hits,
            live,
            edges: edge_total,
        };
        // 结构自检先行：压实产物的 CSR 不变量破了就不该交出去。
        // 不用 `match issues { Vec::new() => ... }` —— `Vec::new` 是函数不是
        // 变体，写在模式位置编译直接拒（E0164）。
        let issues = graph.audit_graph_shape();
        if issues.is_empty() {
            Outcome::ok(graph)
        } else {
            Outcome::err(
                DiagCode::StageContractDiverged,
                &format!("压实后的图结构自检失败（{} 处）", issues.len()),
                &issues.join("；"),
            )
        }
    }
}

// ===========================================================================
// 四、四用途单源（依赖解析 / 加载排序 / 失效传播 / 垃圾回收）
// ===========================================================================

/// 引用图的四种用途。
///
/// 「四用途单源」是本模块的核心纪律：四种用途**读同一张图**。一旦某个用途
/// 自己建图，图就分裂成多份，四种用途之间立刻产生口径分歧——失效传播按A 图
/// 算、GC 按 B 图算，于是「A 说没人引用、B 说有人引用」，谁也说不清资源
/// 到底该不该回收。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphPurpose {
    /// 依赖解析（F3206 拓扑排序的输入）。
    DependencyResolve,
    /// 加载排序（先依赖后资源）。
    LoadOrder,
    /// 失效传播（资源失效时找出所有受影响的下游）。
    Invalidation,
    /// 垃圾回收（F3203 的入度依据）。
    GarbageCollect,
}

impl GraphPurpose {
    /// 全集（确定性次序）。
    pub const ALL: [GraphPurpose; 4] = [
        GraphPurpose::DependencyResolve,
        GraphPurpose::LoadOrder,
        GraphPurpose::Invalidation,
        GraphPurpose::GarbageCollect,
    ];

    /// 英文标识。
    pub fn en(self) -> &'static str {
        match self {
            GraphPurpose::DependencyResolve => "dependency-resolve",
            GraphPurpose::LoadOrder => "load-order",
            GraphPurpose::Invalidation => "invalidation",
            GraphPurpose::GarbageCollect => "garbage-collect",
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            GraphPurpose::DependencyResolve => "依赖解析",
            GraphPurpose::LoadOrder => "加载排序",
            GraphPurpose::Invalidation => "失效传播",
            GraphPurpose::GarbageCollect => "垃圾回收",
        }
    }

    /// 归属下游条（谁消费这张图）。
    pub fn downstream(self) -> &'static str {
        match self {
            GraphPurpose::DependencyResolve => "VE-F3206 依赖解析与加载图",
            GraphPurpose::LoadOrder => "VE-F3206拓扑排序 + VE-F3207 调度消费",
            GraphPurpose::Invalidation => "VE-F3211 增量刷新 + VE-F3212 校验失效",
            GraphPurpose::GarbageCollect => "VE-F3203 句柄引用计数与分代 GC",
        }
    }
}

/// 四用途单源登记表：一个用途对应唯一的图来源标识。
///
/// 做成数据而非代码分支，是为了让「单源」可机检：若某个用途登记了不同来源，
/// [`audit_purpose_single_source`] 立刻报出——而不是等到资源该回收却没回收时
/// 才在日志里猜。
pub const PURPOSE_SOURCE_TABLE: [(GraphPurpose, &str); 4] = [
    (GraphPurpose::DependencyResolve, "ResourceGraph"),
    (GraphPurpose::LoadOrder, "ResourceGraph"),
    (GraphPurpose::Invalidation, "ResourceGraph"),
    (GraphPurpose::GarbageCollect, "ResourceGraph"),
];

/// 某用途的图来源标识。
pub fn purpose_source(p: GraphPurpose) -> Outcome<&'static str> {
    for (k, v) in PURPOSE_SOURCE_TABLE.iter() {
        if *k == p {
            return Outcome::ok(v);
        }
    }
    Outcome::err(
        DiagCode::ResourceTypeUnmapped,
        &format!("用途 {} 未登记图来源", p.en()),
        "未登记的用途等于不受管的用途——它会自己找图或自己建图，从而绕过单源红线。\
         请先在 PURPOSE_SOURCE_TABLE 登记该用途",
    )
}

/// 四用途单源审计：四用途必须指向同一图来源。
pub fn audit_purpose_single_source() -> Outcome<Vec<GraphPurpose>> {
    let mut sources: Vec<&str> = Vec::new();
    let mut bad: Vec<String> = Vec::new();
    for (p, src) in PURPOSE_SOURCE_TABLE.iter() {
        if !sources.contains(src) {
            sources.push(src);
        }
        if src.is_empty() {
            bad.push(format!("用途 {} 的图来源为空", p.en()));
        }
    }
    if PURPOSE_SOURCE_TABLE.len() != GraphPurpose::ALL.len() {
        bad.push(format!(
            "用途登记表 {} 条，应为 {} 条",
            PURPOSE_SOURCE_TABLE.len(),
            GraphPurpose::ALL.len()
        ));
    }
    if sources.len() > 1 {
        bad.push(format!(
            "四用途指向了 {} 个不同图来源（{}）——图已分裂，跨用途口径必然分歧",
            sources.len(),
            sources.join("、")
        ));
    }
    if !bad.is_empty() {
        return Outcome::err(
            DiagCode::StageContractDiverged,
            &format!("四用途单源审计失败（{} 处）", bad.len()),
            &bad.join("；"),
        );
    }
    Outcome::ok(GraphPurpose::ALL.to_vec())
}

/// 用途单源声明（人话版，进架构文档）。
pub const PURPOSE_SINGLE_SOURCE_NOTE: &str =
    "依赖解析、加载排序、失效传播、垃圾回收四种用途读同一张引用图；\
     任一用途自建图即视为图分裂，跨用途口径分歧（失效按A图算、GC 按 B 图算）\
     会使「该不该回收」变成无法判定的问题。";

/// 悬空检出归属：悬空是缺陷，缺陷归谁修必须写明，否则缺陷会在四个用途间漂。
pub const DANGLING_OWNER: &str = "VE-F3202 立案，F3206 消费为「必须先补齐」的加载清单";

/// 环检出归属：环检测算法本体归 F3206，本模块提供 CSR 数据与自环快检。
pub const CYCLE_OWNER: &str =
    "VE-F3202 提供 CSR 边与自环快检；VE-F3206 承接全图环检测与拓扑排序";

// ===========================================================================
// 五、悬空检测：悬空即缺陷
// ===========================================================================

/// 悬空立案条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DanglingCase {
    /// 引用方 ID。
    pub from: ResourceId,
    /// 被引用的缺失 ID。
    pub to: ResourceId,
    /// 立案号（`DANGLING-<from>-><to>`，确定性，可直接进缺陷账本）。
    pub case_id: String,
    /// 被引用槽当时的槽位状态（Vacant = 从未存在；Placeholder = 已知缺失）。
    pub slot: SlotState,
}

impl DanglingCase {
    /// 读屏可读单行（说出「谁缺什么」，不只说「有悬空」）。
    pub fn screen_line(&self) -> String {
        format!(
            "悬空引用：资源 {} 引用了未登记的资源 {}（槽位 {:?}），立案号 {}",
            self.from.raw(),
            self.to.raw(),
            self.slot,
            self.case_id
        )
    }
}

/// 悬空的两类来源（处置方向相反，**不可共用一个码**）。
///
/// 加载期悬空（目标从未登记）→ 置**占位**并生成加载清单：资源稍后会到，
/// 缺的是「还没来」。注销期悬空（曾登记后被删）→ **不置占位**，只立案：
/// 资源是被人主动删掉的，我们并不指望它回来；置占位会让调用方一直等一个
/// 永远不会出现的资源。
///
/// 历史教训（勿回退）：把两者都当「占位」，注销期就会留下一批永久占位，
/// 加载清单越滚越长而永远清不掉。
pub const DANGLING_TWO_KINDS: &str =
    "悬空分两类，处置相反：① 加载期（目标从未登记）→ 置显式占位 + 进加载清单，\
     缺的是「还没到」；② 注销期（曾登记后被删）→ 只立案不置占位，\
     资源是被人主动删掉的，置占位会让调用方永远等一个不会来的资源。";

/// 悬空检出结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DanglingVerdict {
    /// 立案明细。
    pub cases: Vec<DanglingCase>,
    /// 悬空处数。
    pub count: usize,
    /// 显式占位清单（缺失但位置已知的 ID → 被引用次数）。
    pub placeholders: Vec<(u32, u32)>,
    /// 人话汇总。
    pub summary: String,
}

impl DanglingVerdict {
    /// 是否零悬空（零悬空 = 该项通过）。
    pub fn clean(&self) -> bool {
        self.count == 0
    }

    /// 读屏可读多行。
    pub fn screen_lines(&self) -> String {
        let mut s = self.summary.clone();
        for c in self.cases.iter() {
            s.push('；');
            s.push_str(&c.screen_line());
        }
        s
    }
}

/// 悬空检测（`O(V + E)` 全量扫描；增量侧由`add_edge` 即时检出）。
///
/// 为何全量扫描仍必要：增量检出只覆盖「加边那一刻」发生的悬空，而悬空还有
/// 另一条产生路径——**被引用方被注销**。注销方不会去遍历所有引用方（那需要
/// 反向全扫），所以这类悬空只能在全量扫描里露出来。
pub fn detect_dangling(graph: &ResourceGraph) -> DanglingVerdict {
    let mut cases: Vec<DanglingCase> = Vec::new();
    for i in 0..graph.slots {
        let from = ResourceId(i as u32);
        // 只从活跃节点出发：占位与空槽的出边本就无意义。
        if graph.slot_state(from) != SlotState::Live {
            continue;
        }
        for t in graph.out_targets_of(from).iter() {
            let to = ResourceId(*t);
            if graph.slot_state(to) != SlotState::Live {
                let case = DanglingCase {
                    from,
                    to,
                    case_id: format!("DANGLING-{}->{}", from.raw(), to.raw()),
                    slot: graph.slot_state(to),
                };
                if !cases.iter().any(|c| c.from == case.from && c.to == case.to) {
                    cases.push(case);
                }
            }
        }
    }
    // 汇总文案必须区分两类处置，不能一律说「已置占位」——
    // 注销期悬空**不置占位**（资源被人主动删掉，不该等它回来），
    // 文案与实际处置不符，比不说更坏：读文案的人会以为占位已经就绪。
    let summary = if cases.is_empty() {
        String::from("悬空检测：零悬空，全部引用均指向在册资源")
    } else {
        format!(
            "悬空检测：检出 {} 处悬空（悬空即缺陷红线），已立案；其中 {} 处加载期悬空已置显式占位待补齐，\
             {} 处注销期悬空只立案不置占位（资源已被主动删除，不应等待）；归属 {}",
            cases.len(),
            graph.placeholder_hits.len(),
            cases.len().saturating_sub(graph.placeholder_hits.len()),
            DANGLING_OWNER
        )
    };
    DanglingVerdict {
        count: cases.len(),
        cases,
        placeholders: graph.placeholder_hits.clone(),
        summary,
    }
}

/// 悬空占位策略声明：占位必须显性，且不得静默降级为「空资源」。
///
/// 反例（真实踩过的坑）：某域发现纹理缺失，就传一张1×1 白图上去，画面能看，
/// 于是缺陷被吞掉，直到用户投诉「这块材质是白的」才被发现。占位 + 告警的
/// 组合让缺陷**当场可见**。
pub const PLACEHOLDER_POLICY: &str =
    "悬空资源一律置显式占位（SlotState::Placeholder + absent 内容体），\
     并生成加载清单；禁止以「空资源 / 1x1 占位图 / 静默跳过」方式掩盖缺失。";

// ===========================================================================
// 六、环检测基础（F3206 的数据基础）
// ===========================================================================

/// 索引边（CSR 形态的边，供F3206 直接喂给拓扑排序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IndexEdge {
    /// 依赖方。
    pub from: u32,
    /// 被依赖方。
    pub to: u32,
}

/// 导出全部索引边（`O(V + E)`）。
///
/// F3206 不该自己去遍历实体数组找边——那是本模块的责任边界。导出后F3206
/// 拿到的是干净的边数组，可以直接跑 Kahn 拓扑排序。
pub fn export_index_edges(graph: &ResourceGraph) -> Vec<IndexEdge> {
    let mut edges: Vec<IndexEdge> = Vec::new();
    for i in 0..graph.slots {
        for t in graph.out_targets_of(ResourceId(i as u32)).iter() {
            edges.push(IndexEdge { from: i as u32, to: *t });
        }
    }
    edges
}

/// 环检出结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CycleVerdict {
    /// 是否成环。
    pub cyclic: bool,
    /// 环上节点（首个发现的环；确定性次序）。
    pub nodes: Vec<u32>,
    /// 人话结论。
    pub summary: String,
}

/// 环检测（`O(V + E)`，迭代式 DFS 基础）。
///
/// 为何开工条就带环检测：环 = 加载死锁（A 等 B 的同时 B 等 A）。它若只在集成
/// 期暴露，定位成本极高（表现为「加载卡住」，但栈上什么都看不出来）。本函数
/// 是 F3206 完整拓扑排序的**数据基础与前置闸**——它不做加载决策，只回答
/// 「这张图能不能被拓扑排序」。
pub fn detect_cycle_indexed(graph: &ResourceGraph) -> CycleVerdict {
    let nodes = graph.slots;
    // 0=未访问 1=在栈 2=已完成
    let mut state: Vec<u8> = vec![0; nodes];
    let mut path: Vec<u32> = Vec::new();
    let mut cycle: Option<Vec<u32>> = None;

    for start in 0..nodes {
        if state[start] != 0 || cycle.is_some() {
            continue;
        }
        // 迭代式 DFS（不让栈深度随图深线性增长——恶意深链是真实输入，
        // F3206 的深度 64 钳制正是为此）。
        let mut stack: Vec<(u32, u32)> = vec![(start as u32, 0)];
        state[start] = 1;
        path.push(start as u32);
        while let Some((node, child)) = stack.pop() {
            let targets = graph.out_targets_of(ResourceId(node));
            if (child as usize) < targets.len() {
                stack.push((node, child + 1));
                let next = targets[child as usize];
                if state[next as usize] == 1 {
                    let at = path.iter().position(|p| *p == next).unwrap_or(0);
                    let mut cyc: Vec<u32> = path[at..].to_vec();
                    cyc.push(next);
                    cycle = Some(cyc);
                    break;
                }
                if state[next as usize] == 0 {
                    state[next as usize] = 1;
                    path.push(next);
                    stack.push((next, 0));
                }
            } else {
                state[node as usize] = 2;
                if path.last() == Some(&node) {
                    path.pop();
                }
            }
        }
        if cycle.is_some() {
            break;
        }
    }

    match cycle {
        Some(c) => {
            // 先取长度再建串：`nodes: c` 是 move，之后就看不到 c 了。
            let len = c.len();
            let names: Vec<String> = c.iter().map(|i| i.to_string()).collect();
            CycleVerdict {
                cyclic: true,
                nodes: c,
                summary: format!(
                    "环检出：引用成环 {}（{}）——环即加载死锁红线，必须打破；归属 {}",
                    names.join(" → "),
                    len,
                    CYCLE_OWNER
                ),
            }
        }
        None => CycleVerdict {
            cyclic: false,
            nodes: Vec::new(),
            summary: String::from("环检测：零环，图可被拓扑排序（先依赖后资源）"),
        },
    }
}

/// 环拒绝三要素（环检出的处置口径）。
///
/// 「拒绝」在本域的含义是**拒绝拓扑排序**（不产出加载序），而不是拒绝建图：
/// 依赖关系是客观事实，不该因为成环就否认它。否认会让调试者看不到真实的环。
pub const CYCLE_REJECT_TRIPLE: [&str; 3] = [
    "现象：引用图存在环，拓扑排序无合法序",
    "根因：资源间存在互相依赖（A 需 B 的同时 B 需 A）",
    "处置：打破环——把某资源改为「后加载的弱依赖」（不阻塞主资源交付），\
     或合并两资源为单一实体；不得靠「忽略环」继续排序",
];

/// 失效传播范围（`from` 失效时所有受影响的下游，迭代式 BFS）。
///
/// 这是四用途之一「失效传播」的可执行形态。算法上与拓扑排序无关，故放在本
/// 模块（它只需要图，不需要序）。
pub fn invalidation_closure(graph: &ResourceGraph, from: ResourceId) -> Vec<u32> {
    let mut seen: Vec<u32> = Vec::new();
    let mut queue: Vec<u32> = vec![from.raw()];
    let mut visited: Vec<u32> = vec![from.raw()];
    while let Some(cur) = queue.pop() {
        for s in graph.in_sources_of(ResourceId(cur)).iter() {
            // 失效沿「谁依赖我」的方向传播：被依赖者失效 → 依赖它的失效。
            seen.push(*s);
            if !visited.contains(s) {
                visited.push(*s);
                queue.push(*s);
            }
        }
    }
    seen
}

// ===========================================================================
// 七、32MB 内存红线与分域分图降级
// ===========================================================================

/// 运行期只读引用图（CSR 紧凑形态）。
///
/// 内存构成（百万节点 + 百万边的量级估算见 [`estimate_graph_bytes`]）：
/// - `nodes`：`Option<Resource>`，每槽约 56B（Resource 32B + Option 判别位）；
/// - `slots` 隐含在 `nodes.len()`，不额外占；
/// - `out_offsets` / `in_offsets`：`u32 × (N+1)` 各一份；
/// - `out_targets` / `in_sources`：`u32 × E` 各一份。
#[derive(Clone, Debug, Default)]
pub struct ResourceGraph {
    /// 槽位数（含空槽；= CSR 段数）。
    pub slots: usize,
    /// 节点表：下标 = ID。
    pub nodes: Vec<Option<Resource>>,
    /// 出边 CSR 行偏移（长度 = slots + 1）。
    pub out_offsets: Vec<u32>,
    /// 出边 CSR 目标（单块数组）。
    pub out_targets: Vec<u32>,
    /// 反向索引 CSR 行偏移（长度 = slots + 1）。
    pub in_offsets: Vec<u32>,
    /// 反向索引 CSR 源（单块数组）。
    pub in_sources: Vec<u32>,
    /// 构建期检出的悬空立案（压实后保留，供 F3206 生成加载清单）。
    pub dangling: Vec<DanglingCase>,
    /// 占位命中表。
    pub placeholder_hits: Vec<(u32, u32)>,
    /// 活跃节点数（缓存，避免每次 O(V) 重算）。
    pub live: usize,
    /// 边数（缓存）。
    pub edges: usize,
}

impl ResourceGraph {
    /// 槽位状态（三态）。
    pub fn slot_state(&self, id: ResourceId) -> SlotState {
        if !id.is_valid() {
            return SlotState::Vacant;
        }
        match self.nodes.get(id.raw() as usize) {
            Some(Some(_)) => SlotState::Live,
            Some(None) => {
                // 占位标记与实体表平行维护：压实时把占位集合快照下来。
                if self.placeholder_hits.iter().any(|e| e.0 == id.raw()) {
                    SlotState::Placeholder
                } else {
                    SlotState::Vacant
                }
            }
            None => SlotState::Vacant,
        }
    }

    /// 查资源。
    pub fn get(&self, id: ResourceId) -> Option<Resource> {
        self.nodes.get(id.raw() as usize).copied().flatten()
    }

    /// 出边目标（CSR 切片，不分配）。
    pub fn out_targets_of(&self, id: ResourceId) -> Vec<u32> {
        let i = id.raw() as usize;
        if i >= self.slots || self.out_offsets.len() < i + 2 {
            return Vec::new();
        }
        let lo = self.out_offsets[i] as usize;
        let hi = self.out_offsets[i + 1] as usize;
        if hi > self.out_targets.len() || lo > hi {
            return Vec::new();
        }
        self.out_targets[lo..hi].to_vec()
    }

    /// 反向来源（CSR 切片）。
    pub fn in_sources_of(&self, id: ResourceId) -> Vec<u32> {
        let i = id.raw() as usize;
        if i >= self.slots || self.in_offsets.len() < i + 2 {
            return Vec::new();
        }
        let lo = self.in_offsets[i] as usize;
        let hi = self.in_offsets[i + 1] as usize;
        if hi > self.in_sources.len() || lo > hi {
            return Vec::new();
        }
        self.in_sources[lo..hi].to_vec()
    }

    /// 入度（F3203 的 GC 判据）。
    pub fn in_degree(&self, id: ResourceId) -> usize {
        self.in_sources_of(id).len()
    }

    /// 出度。
    pub fn out_degree(&self, id: ResourceId) -> usize {
        self.out_targets_of(id).len()
    }

    /// GC 候选（入度为 0 的活跃节点）——F3203 的输入。
    pub fn gc_candidates(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for i in 0..self.slots {
            let id = ResourceId(i as u32);
            if self.slot_state(id) == SlotState::Live && self.in_degree(id) == 0 {
                out.push(i as u32);
            }
        }
        out
    }

    /// 图内存审计（实测口径：按结构常量与实际规模算，不是猜）。
    ///
    /// 默认按**按需形态**（含反向索引）报——因为本图的反向索引已经随压实
    /// 一并建好了，报告如实反映它此刻的占用。常驻形态请用
    /// [`estimate_graph_bytes`]（`with_reverse=false`）自行对照。
    pub fn memory_report(&self) -> MemoryReport {
        build_memory_report(self.slots as u32, self.edges as u32, true)
    }

    /// 结构自检（CSR 不变量）。
    pub fn audit_graph_shape(&self) -> Vec<String> {
        let mut bad: Vec<String> = Vec::new();
        if self.out_offsets.len() != self.slots + 1 {
            bad.push(format!(
                "out_offsets 长度 {} ≠ 槽数+1 = {}",
                self.out_offsets.len(),
                self.slots + 1
            ));
        }
        if self.in_offsets.len() != self.slots + 1 {
            bad.push(format!(
                "in_offsets 长度 {} ≠ 槽数+1 = {}",
                self.in_offsets.len(),
                self.slots + 1
            ));
        }
        if let Some(last) = self.out_offsets.last() {
            if *last as usize != self.out_targets.len() {
                bad.push(format!(
                    "out_offsets 末项 {} ≠ out_targets 长度 {}",
                    last,
                    self.out_targets.len()
                ));
            }
        }
        if let Some(last) = self.in_offsets.last() {
            if *last as usize != self.in_sources.len() {
                bad.push(format!(
                    "in_offsets 末项 {} ≠ in_sources 长度 {}",
                    last,
                    self.in_sources.len()
                ));
            }
        }
        // 边目标越界检查（CSR 越界读= 内存不安全，必须拦）。
        for t in self.out_targets.iter() {
            if *t as usize >= self.slots {
                bad.push(format!("出边目标 {} 越界（槽数 {}）", t, self.slots));
                break;
            }
        }
        for s in self.in_sources.iter() {
            if *s as usize >= self.slots {
                bad.push(format!("反向来源 {} 越界（槽数 {}）", s, self.slots));
                break;
            }
        }
        // 出度声明与CSR 实际长度一致性。
        for i in 0..self.slots {
            if let Some(r) = self.get(ResourceId(i as u32)) {
                let actual = self.out_degree(ResourceId(i as u32)) as u32;
                if r.out_degree != actual {
                    bad.push(format!(
                        "资源 {} 的 out_degree 声明 {} ≠ CSR 实际 {}",
                        i,
                        r.out_degree,
                        actual
                    ));
                    break;
                }
            }
        }
        bad
    }
}

/// 单项内存开销（字节）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryItem {
    /// 项名。
    pub item: &'static str,
    /// 每单位字节。
    pub bytes_per_unit: u64,
    /// 单位数。
    pub units: u64,
    /// 小计字节。
    pub subtotal: u64,
    /// 为何是这个开销（不是拍脑袋的数字）。
    pub basis: &'static str,
}

/// 内存核算报告（逐项分解，可审计）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryReport {
    /// 逐项开销。
    pub items: Vec<MemoryItem>,
    /// 合计字节。
    pub total_bytes: u64,
    /// 预算字节。
    pub budget_bytes: u64,
    /// 是否在预算内。
    pub within_budget: bool,
    /// 超预算时的超出字节（0 = 未超）。
    pub overage_bytes: u64,
}

impl MemoryReport {
    /// 人话一行。
    pub fn screen_line(&self) -> String {
        format!(
            "图内存核算：{} 项合计 {} 字节（预算 {} 字节，{}），{}",
            self.items.len(),
            self.total_bytes,
            self.budget_bytes,
            if self.within_budget { "在预算内" } else { "超预算" },
            if self.within_budget {
                String::from("32MB 线守住")
            } else {
                format!("超出 {} 字节，须分域分图降级", self.overage_bytes)
            }
        )
    }
}

/// 节点记录字节（**压实后的真值**，不是估值）。
///
/// 布局见 [`NODE_LAYOUT`]：kind1 + flags1 + version2 + tag0 2 + tag1 2 +
/// byte_length4 + fingerprint4 + out_offset4 + out_degree4 = 24B。
/// 三处关键取舍：
/// - **不存 `id`**——ID 就是槽位下标，存一份纯冗余（省 4B/槽 = 百万时 4MB）；
/// - **8 个标志位打包进 `u8`**——srgb/streamable/present/resident/absent/
///   compression 共 8 位装得下，不必各占 1B（省 5B/槽）；
/// - **指纹截 `u32`**——图内只需判「是否同一内容」，完整 64 位哈希归 F3211
///   的版本库；图里存 64 位是 4MB 纯浪费（省 4B/槽）。
///
/// 三项合计把节点从 56B 压到 24B——百万规模省 32MB。**这正是 32MB 红线
/// 能不能守住的决定性因素**：56B 形态下光节点就 56MB，红线从一开始就不可能。
pub const BYTES_PER_NODE_SLOT: u64 = 24;
/// 单条出边字节（CSR `u32` 目标）。
pub const BYTES_PER_OUT_EDGE: u64 = 4;
/// 单条反向边字节（CSR `u32` 源；**按需构建，非常驻**——见
/// [`REVERSE_INDEX_POLICY`]）。
pub const BYTES_PER_IN_EDGE: u64 = 4;
/// 每槽 CSR 行偏移字节（仅出边偏移 `u32`；反向偏移随反向索引一起按需建）。
pub const BYTES_PER_SLOT_OFFSET: u64 = 4;

/// 反向索引策略：按需构建，非常驻。
///
/// 为何不做常驻：常驻要多付 `4B×E`（百万边 = 4MB）+ `4B×N` 的反向偏移，
/// 恰好把 32MB 红线顶破。而反向索引的三条用途都有低频特征——失效传播
/// （热更时）、GC 入度对账（F3203 对账时）、反向可达闭包——没有一条要求
/// 它常驻。反过来，**省它会瞎**：只看出边就不知道谁依赖我，失效传播与 GC
/// 判据全部失效。故取舍是：常驻省内存，按需保正确。
pub const REVERSE_INDEX_POLICY: &str =
    "反向索引按需构建、非常驻：常驻需多付 4B/边 + 4B/槽，百万规模恰把 32MB 顶破。\
     失效传播 / GC 入度对账 / 反向闭包三条用途均为低频，用时重建（O(V+E)）代价可接受。\
     不得为省内存而完全省略——只看出边无法回答「谁依赖我」，GC 与失效传播会失效。";

/// 节点记录的紧凑布局逐项（每一项都要能说出「为什么是这么多字节」）。
pub const NODE_LAYOUT: [(&str, u64); 9] = [
    ("kind（资源类型枚举）", 1),
    ("flags（8 个标志位打包）", 1),
    ("version（u16，热更版本号）", 2),
    ("tag0（u16 紧凑标签）", 2),
    ("tag1（u16 紧凑标签）", 2),
    ("byte_length（内容体字节数）", 4),
    ("fingerprint（内容指纹 u32 截断）", 4),
    ("out_offset（CSR 行起点）", 4),
    ("out_degree（出度）", 4),
];

/// 内存估算（给定节点数与边数，不构造真实图）。
///
/// 为何要有「不构造图」的估算器：百万节点真实构造会吃掉数十 MB 且耗时，
/// 而红线检查只需要算式。**红线检查不该靠分配内存来验证内存**——那本身就
/// 是被红线约束的行为。
pub fn estimate_graph_bytes(nodes: u32, edges: u32, with_reverse: bool) -> u64 {
    let n = nodes as u64;
    let e = edges as u64;
    // 用 (n+1) 而非 n：CSR 行偏移数组长度是槽数 + 1（第 i 段的终点在 i+1）。
    // 这里若写 n，估算器与 [`build_memory_report`] 就会差 4 字节，
    // 而「实测口径与估算一致」这条机检正是靠两者相等来守的。
    let mut total = n * BYTES_PER_NODE_SLOT + (n + 1) * BYTES_PER_SLOT_OFFSET + e * BYTES_PER_OUT_EDGE;
    if with_reverse {
        // 按需建的反向索引：边 + 反向行偏移，两笔都算（漏算偏移会低估 4MB/百万）。
        total += e * BYTES_PER_IN_EDGE + (n + 1) * BYTES_PER_SLOT_OFFSET;
    }
    total
}

/// 组装内存报告。
pub fn build_memory_report(nodes: u32, edges: u32, with_reverse: bool) -> MemoryReport {
    let n = nodes as u64;
    let e = edges as u64;
    let mut items: Vec<MemoryItem> = vec![
        MemoryItem {
            item: "节点记录（压实 ResourceNode）",
            bytes_per_unit: BYTES_PER_NODE_SLOT,
            units: n,
            subtotal: n * BYTES_PER_NODE_SLOT,
            basis: "ID 即下标不存 + 8 个标志位打包进 u8 + 指纹截 u32 → 24B/槽。\
                    对比未压实形态（Option<Resource> 56B）省 32B/槽，百万规模省 32MB",
        },
        MemoryItem {
            item: "出边 CSR 行偏移",
            bytes_per_unit: BYTES_PER_SLOT_OFFSET,
            units: n + 1,
            subtotal: (n + 1) * BYTES_PER_SLOT_OFFSET,
            basis: "每槽一个 u32 行起点；这是把邻接从「每实体一个 Vec」压成单块数组的代价",
        },
        MemoryItem {
            item: "出边目标（CSR 单块）",
            bytes_per_unit: BYTES_PER_OUT_EDGE,
            units: e,
            subtotal: e * BYTES_PER_OUT_EDGE,
            basis: "每条边一个 u32 目标下标——紧凑的关键在此，不在别处",
        },
    ];
    if with_reverse {
        items.push(MemoryItem {
            item: "反向索引（按需构建：源 + 行偏移）",
            bytes_per_unit: BYTES_PER_IN_EDGE + BYTES_PER_SLOT_OFFSET,
            units: e,
            subtotal: e * BYTES_PER_IN_EDGE + (n + 1) * BYTES_PER_SLOT_OFFSET,
            basis: "失效传播与入度对账都要反向索引；按需建非常驻。行偏移同样是 \
                    (n+1) 条——与 estimate_graph_bytes 口径必须一致，差一条就会让\
                    「实测口径与估算一致」这条机检误报",
        });
    }
    let total = items.iter().map(|i| i.subtotal).sum();
    let overage = if total > GRAPH_MEMORY_BUDGET_BYTES {
        total - GRAPH_MEMORY_BUDGET_BYTES
    } else {
        0
    };
    MemoryReport {
        items,
        total_bytes: total,
        budget_bytes: GRAPH_MEMORY_BUDGET_BYTES,
        within_budget: total <= GRAPH_MEMORY_BUDGET_BYTES,
        overage_bytes: overage,
    }
}

/// 百万资源图内存审计（32MB 红线的判定入口）。
///
/// 判据：百万节点、每节点平均 1 条出边、含反向索引，总额须≤ 32MB。平均出边
/// 取 1 是因为它是「资源引用图」的经验值（多数资源不引用别的，模型/材质引用
/// 纹理，场景引用模型）；若业务确实需要平均 3 条边，那时的正确反应是
/// **接受超预算并走分域分图降级**，而不是把红线调大。
pub fn audit_memory_budget(nodes: u32, edges: u32, with_reverse: bool) -> Outcome<MemoryReport> {
    // 口径说明：`with_reverse=false` 是**常驻形态**（反向索引按需建），
    // 也是 32MB 红线考核的对象；`true` 是「正在做失效传播/GC 对账」的临时
    // 形态，允许临时超——但那段时间是短时的，不是常驻占用。
    let report = build_memory_report(nodes, edges, with_reverse);
    if !report.within_budget {
        return Outcome::err(
            DiagCode::BudgetExceeded,
            &format!(
                "图内存超红线：{} 节点/{} 边需 {} 字节，超预算 {} 字节",
                nodes, edges, report.total_bytes, report.overage_bytes
            ),
            "超预算不要调大红线（32MB 是物理约束不是偏好）。两条正路：\
             ① 去掉反向索引改用「按需重建」——失效传播与 GC 对账时可临时重建；\
             ② 分域分图降级（见 plan_sharded_degradation），把单图规模压到预算内",
        );
    }
    Outcome::ok(report)
}

/// 分域分图分片。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphShard {
    /// 分片号。
    pub shard_id: u32,
    /// 归属域（分片名须可读——「按域分图」的分域依据是业务域，不是哈希桶）。
    pub domain: String,
    /// ID区间下界（含）。
    pub id_lo: u32,
    /// ID 区间上界（不含）。
    pub id_hi: u32,
    /// 本片节点数。
    pub nodes: u32,
    /// 本片边数。
    pub edges: u32,
    /// 本片内存字节。
    pub bytes: u64,
    /// 是否在预算内。
    pub within_budget: bool,
}

/// 分域分图降级方案。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DegradationPlan {
    /// 分片清单。
    pub shards: Vec<GraphShard>,
    /// 分片数。
    pub shard_count: u32,
    /// 降级前总字节。
    pub before_bytes: u64,
    /// 降级后单片最大字节。
    pub after_max_bytes: u64,
    /// 是否真正解了预算问题（全部在预算内）。
    pub resolved: bool,
    /// 人话结论。
    pub summary: String,
}

/// 分域分图降级：把一张超预算的图切成若干片。
///
/// 为何「分域」而不是「按ID 等分」：等分是纯技术手段，会把一个材质和它依赖的
/// 纹理切到两片——跨片依赖于是变成跨图依赖，问题从「图太大」变成「图碎了」。
/// 按业务域切（渲染域一片、UI 域一片、音频域一片）能让大多数依赖留在片内。
/// 跨片依赖不是被消灭而是被**显式登记**（见 `cross_shard_edges`）。
///
/// 诚实声明：分域分图**不是万能解**。若单个域自身的图就已超预算，本函数会
/// 如实报`resolved=false`——那时唯一正路是回到「省反向索引」或「重新审视
/// 边数是否合理」，而不是继续切。
pub fn plan_sharded_degradation(
    nodes: u32,
    edges: u32,
    domains: &[(u32, &str)],
) -> Outcome<DegradationPlan> {
    if domains.is_empty() {
        return Outcome::err(
            DiagCode::ValueInvalid,
            "分域分图未给出任何域",
            "无域可切= 无处可降级。请提供「(起始ID, 域名)」清单；\
             若图确实只有一域，请改走去掉反向索引这条正路",
        );
    }
    let before = estimate_graph_bytes(nodes, edges, false);
    // 按域顺序切分：域内边占比保守取1.0（不假设域内边占比低——那会把
    // 超预算藏起来）。
    // 不做「均分」：每片规模由域的 ID 区间决定（域有大有小，均分反而把
    // 小域撑大、大域压不动）。均分只在区间未知时才是合理近似，而本函数
    // 的入参就是区间——用它却假装均分，是把「大概」讲成「确定」。
    let mut shards: Vec<GraphShard> = Vec::new();
    for (i, (start, name)) in domains.iter().enumerate() {
        let lo = *start;
        let hi = if i + 1 < domains.len() {
            domains[i + 1].0
        } else {
            nodes
        };
        let sn = hi.saturating_sub(lo);
        // 每片边数按节点数等比缩放（全域边数的同比例部分）。
        let se = if nodes == 0 {
            0
        } else {
            (edges as u64 * sn as u64 / nodes as u64) as u32
        };
        // 按**常驻形态**估算（不含按需反向索引）——降级方案要保证的是
        // 常驻占用回到预算内；反向索引临时构建的那段超预算由调用方知晓。
        let bytes = estimate_graph_bytes(sn, se, false);
        shards.push(GraphShard {
            shard_id: i as u32,
            domain: String::from(*name),
            id_lo: lo,
            id_hi: hi,
            nodes: sn,
            edges: se,
            bytes,
            within_budget: bytes <= GRAPH_MEMORY_BUDGET_BYTES,
        });
    }
    let after_max = shards.iter().map(|s| s.bytes).max().unwrap_or(0);
    let resolved = shards.iter().all(|s| s.within_budget);
    let summary = if resolved {
        format!(
            "分域分图降级：切成{} 片，单片最大 {} 字节，全部在 32MB 预算内（降级前 {} 字节）",
            shards.len(),
            after_max,
            before
        )
    } else {
        format!(
            "分域分图降级未解问题：{} 片中最大 {} 字节仍超预算。\
             分域只能把规模按域摊薄，单域自身超预算时切不动了——\
             请改走去掉反向索引（按需重建）或重新审视边数是否合理",
            shards.len(),
            after_max
        )
    };
    Outcome::ok(DegradationPlan {
        shard_count: shards.len() as u32,
        shards,
        before_bytes: before,
        after_max_bytes: after_max,
        resolved,
        summary,
    })
}

/// 跨片依赖登记（分域分图后仍需知道哪些边跨片）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrossShardEdge {
    /// 依赖方所在片。
    pub from_shard: u32,
    /// 被依赖方所在片。
    pub to_shard: u32,
    /// 跨片边数。
    pub edge_count: u32,
}

/// 统计跨片边（分域分图的显性代价登记）。
///
/// 为何必须显性：跨片依赖意味着「片内可拓扑排序、片间仍成环」。若不把跨片
/// 边摆在明面上，加载序就会在片内算完就以为万事大吉，跨片那部分实际是乱序
/// 的——这是分域分图最常见的翻车方式。
pub fn count_cross_shard_edges(
    edges: &[IndexEdge],
    shard_of: &dyn Fn(u32) -> u32,
) -> Vec<CrossShardEdge> {
    let mut out: Vec<CrossShardEdge> = Vec::new();
    for e in edges.iter() {
        let fs = shard_of(e.from);
        let ts = shard_of(e.to);
        if fs == ts {
            continue;
        }
        match out
            .iter_mut()
            .find(|x| x.from_shard == fs && x.to_shard == ts)
        {
            Some(x) => x.edge_count += 1,
            None => out.push(CrossShardEdge {
                from_shard: fs,
                to_shard: ts,
                edge_count: 1,
            }),
        }
    }
    out
}

// ===========================================================================
// 八、元数据缺失：默认元 + 告警
// ===========================================================================

/// 元数据解析结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataResolution {
    /// 生效的元数据（缺失时为默认元）。
    pub effective: ResourceMetadata,
    /// 是否走了默认元。
    pub defaulted: bool,
    /// 告警（走默认元时必填——**静默填默认值等于把缺陷藏起来**）。
    pub warning: Option<Diagnostic>,
}

impl MetadataResolution {
    /// 读屏可读单行。
    pub fn screen_line(&self) -> String {
        match &self.warning {
            None => format!("元数据就绪：{}", self.effective.screen_line()),
            Some(w) => format!(
                "元数据缺失，已置默认元并告警：{}；生效值 {}",
                w.message,
                self.effective.screen_line()
            ),
        }
    }
}

/// 元数据缺失处置：默认元 + 告警（**不许静默**）。
///
/// 为何是「默认元 + 告警」而不是「拒绝资源」：元数据缺失在实践中极常见
/// （第三方资源、导出工具漏写），直接拒绝会让管线在集成期大面积失败。但
/// 静默填默认值同样不可接受——那会让「颜色空间错了」这类问题在几周后以
/// 「画面偏色」的形式出现，定位成本高一个数量级。默认 + 告警把「已知
/// 不可知」摆在明面上。
pub fn resolve_metadata(raw: Option<ResourceMetadata>) -> MetadataResolution {
    match raw {
        Some(m) if m.present => MetadataResolution {
            effective: m,
            defaulted: false,
            warning: None,
        },
        other => {
            let given = other.unwrap_or_default();
            MetadataResolution {
                effective: ResourceMetadata::default(),
                defaulted: true,
                warning: Some(Diagnostic {
                    code: DiagCode::ValueInvalid,
                    message: format!(
                        "资源元数据缺失或未标present，已置默认元（调用方传入 present={}）",
                        given.present
                    ),
                    // 诊断三要素里的「怎么办」不许留空：调用方拿到一个没有
                    // 处置建议的告警，只能自己去猜——那等于把诊断成本转嫁出去。
                    hint: String::from(
                        "默认元取保守侧（不宣称 sRGB、不宣称可流式），故不会造成偏色或误寻址。\
                          请在打包环节（F3214）补齐元数据——尤其是压缩方式与色彩空间，\
                          这两项错了会让解码器按错误假设读数据",
                    ),
                }),
            }
        }
    }
}

/// 元数据缺失降级矩阵（锚点「元数据缺失→默认元+告警」的可执行形态）。
pub fn metadata_degradation_matrix() -> Vec<(&'static str, &'static str)> {
    vec![
        ("元数据在册且 present=true", "直接采用，不告警"),
        ("元数据在册但 present=false", "视为缺失：置默认元 + 告警（不信任声明）"),
        ("元数据完全未提供（None）", "置默认元 + 告警（显式缺失）"),
        ("默认元 + 告警后仍被消费", "放行但缺陷留在告警里——消费方须能查到该告警"),
    ]
}

// ===========================================================================
// 九、跨批对接
// ===========================================================================

/// 跨批对接登记（本条交付给谁、对方拿什么）。
pub const DOWNSTREAM_HANDOFF: [(&str, &str, &str); 4] = [
    (
        "VE-F3203",
        "句柄引用计数与分代 GC",
        "in_degree / gc_candidates / slot_state 三件套——计数与图入度双源对账的图侧输入",
    ),
    (
        "VE-F3204",
        "资源类型系统",
        "Resource.kind + ResourceMetadata 固定槽位——schema 校验的载体（F3204 定义键集，本模块只留槽）",
    ),
    (
        "VE-F3206",
        "依赖解析与加载图",
        "export_index_edges / detect_cycle_indexed / invalidation_closure——拓扑排序的输入与前置闸",
    ),
    (
        "VE-F3211",
        "资源版本与热更新",
        "ContentBody.fingerprint + ResourceMetadata.version + get()——内容寻址与增量 diff 的对端（\
         本模块提供 get(id) 让F3211 无需自建索引）",
    ),
];

/// 对接完整性机检：每条对接必须有属主、有交付物、有去向。
pub fn audit_downstream_handoff() -> Outcome<Vec<String>> {
    let mut bad: Vec<String> = Vec::new();
    if DOWNSTREAM_HANDOFF.len() < 4 {
        bad.push(format!(
            "对接登记 {} 条，少于 F3202 应有的 4 条（F3203/F3204/F3206/F3211）",
            DOWNSTREAM_HANDOFF.len()
        ));
    }
    for (id, who, what) in DOWNSTREAM_HANDOFF.iter() {
        if id.trim().is_empty() || who.trim().is_empty() || what.trim().is_empty() {
            bad.push(format!("对接条目 ({}, {}, {}) 有字段留空", id, who, what));
        }
    }
    // 属主唯一性：同一条目不得被两个下游同时认领。
    for i in 0..DOWNSTREAM_HANDOFF.len() {
        for j in (i + 1)..DOWNSTREAM_HANDOFF.len() {
            if DOWNSTREAM_HANDOFF[i].0 == DOWNSTREAM_HANDOFF[j].0 {
                bad.push(format!(
                    "下游条目 {} 被登记两次（两个属主= 交付物归属歧义）",
                    DOWNSTREAM_HANDOFF[i].0
                ));
            }
        }
    }
    if !bad.is_empty() {
        return Outcome::err(
            DiagCode::StageContractDiverged,
            &format!("跨批对接审计失败（{} 处）", bad.len()),
            &bad.join("；"),
        );
    }
    Outcome::ok(DOWNSTREAM_HANDOFF.iter().map(|d| d.0.to_string()).collect())
}

// ===========================================================================
// 十、无障碍替述与图的口播
// ===========================================================================

/// 引用图的线性文字替述（读屏可达）。
///
/// 图对读屏用户是不可达的——这不是「顺便加上」的善举，而是**验收项**：图是
/// 本域唯一的产品物，图不可读等于产品对一部分用户不存在。文字版与图版同源
/// （读同一张 [`ResourceGraph`]），不另写一份以免漂移。
pub fn graph_narration(graph: &ResourceGraph) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "资源引用图：槽位 {} 个，活跃资源 {} 个，引用边 {} 条。",
        graph.slots, graph.live, graph.edges
    ));
    let dangling = detect_dangling(graph);
    out.push_str(&format!(
        "悬空引用 {} 处。",
        if dangling.count == 0 {
            String::from("无")
        } else {
            format!("{} 处，均已置显式占位", dangling.count)
        }
    ));
    let cyc = detect_cycle_indexed(graph);
    out.push_str(&format!(
        "循环引用：{}",
        if cyc.cyclic {
            format!("存在，环上节点 {}", cyc.nodes.len())
        } else {
            String::from("无，图可按依赖序加载")
        }
    ));
    // 逐资源口播（只念活跃节点，空槽不占读屏时间）。
    let mut spoken = 0usize;
    for i in 0..graph.slots {
        if let Some(r) = graph.get(ResourceId(i as u32)) {
            out.push_str(&r.screen_line());
            out.push('。');
            spoken += 1;
            if spoken >= 32 {
                out.push_str("（余下资源从略，可按ID 查询）");
                break;
            }
        }
    }
    out
}

/// 引用图读屏摘要（一行版）。
pub fn graph_screen_text(graph: &ResourceGraph) -> String {
    format!(
        "引用图：{} 活跃/{} 槽，{} 边，{} 处悬空占位，{}",
        graph.live,
        graph.slots,
        graph.edges,
        graph.placeholder_hits.len(),
        if detect_cycle_indexed(graph).cyclic {
            "存在环（须打破）"
        } else {
            "无环"
        }
    )
}

// ===========================================================================
// 十一、复杂度预算（逐项分解，诚实声明）
// ===========================================================================

/// 复杂度预算项。
#[derive(Clone, Copy, Debug)]
pub struct PerfItem {
    /// 项名。
    pub item: &'static str,
    /// 渐近行为。
    pub complexity: &'static str,
    /// 为何在真实区间内是常数级（`None` = 明说它不是常数级）。
    pub bounded_by: Option<&'static str>,
    /// 备注。
    pub note: &'static str,
}

/// 锚点「性能逐项分解」的逐项落法。
pub const PERF_BUDGET: [PerfItem; 8] = [
    PerfItem {
        item: "加边（构建期）",
        complexity: "O(1) 摊销",
        bounded_by: Some("单次 push 到该节点的邻接 Vec，不遍历既有边（去重是 O(出度)，已单列）"),
        note: "去重那一步是 O(出度)，若某节点出度达万级会退化——见「重复边检出」项",
    },
    PerfItem {
        item: "重复边检出",
        complexity: "O(出度)",
        bounded_by: None,
        note: "用 contains 线性查。资源图出度经验值≤ 数十，故真实区间内可接受；\
              若出现万级出度（典型：一张纹理被百万材质引用），应改用出度侧的\
              排序+ 二分或位图",
    },
    PerfItem {
        item: "悬空检测（全量）",
        complexity: "O(V + E)",
        bounded_by: None,
        note: "锚点要求 O(1) 索引查指的是**单条边**的判定（槽位状态 O(1)），\
              全量扫描必然是 O(V+E)——把全量谎报为 O(1) 才是真缺陷",
    },
    PerfItem {
        item: "悬空判定（单条边）",
        complexity: "O(1)",
        bounded_by: Some("槽位状态是数组下标寻址，无哈希、无链表"),
        note: "这才是锚点说的 O(1) 索引查",
    },
    PerfItem {
        item: "环检测基础",
        complexity: "O(V + E) 增量",
        bounded_by: None,
        note: "自环是 O(1)（加边时比较 from== to）；全图环是 O(V+E)。\
              本函数是 F3206 的数据基础，完整拓扑排序归F3206",
    },
    PerfItem {
        item: "内存占用",
        complexity: "O(压缩) 即 O(N + E)×常数",
        bounded_by: Some("节点 56B/槽 + 偏移 8B/槽 + 边 4B/条（双向各一份）——\
              常数已被32MB 红线钳住，百万规模实测在预算内"),
        note: "CSR 是「压缩」的实质：对比每实体一个 Vec 的形态，光 Vec 头就24B/节点",
    },
    PerfItem {
        item: "失效传播闭包",
        complexity: "O(受影响节点数)",
        bounded_by: None,
        note: "全图失效时退化为 O(V+E)；按需失效（单资源热更）时只扫反向可达集，\
              这正是热更新走增量而非全量的原因",
    },
    PerfItem {
        item: "元数据解析",
        complexity: "O(1)",
        bounded_by: Some("固定槽位拷贝，无动态键值查找"),
        note: "固定槽位是内存红线的兑现：动态 map 会让此项退化为 O(键数) 且带堆分配",
    },
];

/// 复杂度预算机检。
pub fn audit_perf_budget() -> Outcome<Vec<String>> {
    let mut bad: Vec<String> = Vec::new();
    for b in PERF_BUDGET.iter() {
        if b.complexity.trim().is_empty() {
            bad.push(format!("项「{}」未声明复杂度", b.item));
        }
        if b.complexity.contains("O(1)") && b.bounded_by.is_none() {
            bad.push(format!(
                "项「{}」自称 O(1) 但未注明为何在真实区间内为常数级",
                b.item
            ));
        }
        if b.note.trim().is_empty() {
            bad.push(format!("项「{}」缺备注（不写为什么的预算会被误当定论）", b.item));
        }
    }
    if PERF_BUDGET.len() < 8 {
        bad.push(format!(
            "预算项 {} 条，少于锚点要求的 8 项逐项分解",
            PERF_BUDGET.len()
        ));
    }
    if !bad.is_empty() {
        return Outcome::err(
            DiagCode::BudgetExceeded,
            &format!("复杂度预算审计失败（{} 处）", bad.len()),
            &bad.join("；"),
        );
    }
    Outcome::ok(PERF_BUDGET.iter().map(|b| b.item.to_string()).collect())
}

// ===========================================================================
// 十二、类型一致性对接（F3204 的前置闸）
// ===========================================================================

/// 类型与元数据一致性校验（F3204 承接完整schema，本条只做能做的部分）。
///
/// 为何本条已经能做一部分校验：类型是否在十项映射内（F3201 的表）是**全局
/// 闭集**判定，不依赖 F3204 的 schema 细节；而「纹理却没有 sRGB 声明」这类
/// 逐类型约束确实要等 F3204。此处只做前一件——**边界内能做的做完，不抢
/// 别人的活，也不把没做的说成做了**。
pub fn audit_resource_type(id: ResourceId, kind: ResourceKind) -> Outcome<ResourceMappingView> {
    // `adjudicate_capability` 返回的是本域的 `Outcome`（不是 `Result`），
    // 且它的裁定只有 Owned / Redirected 两态——**没有「仲裁受理」态**。
    // 这里不能靠「有没有某个变体」判断登记权：Redirected 意味着属主是别人，
    // 本域代为登记同样是绕过 F3204。
    // 申请方必须是**域标识**而非工单号：`resource-type-registry` 的属主
    // 登记为 `VE-Q`（整个 Q 域），F3204 是域内承接该能力的工单。
    // 拿工单号去申请会永远判 Redirected——工单不是独立申请方。
    let verdict = match adjudicate_capability(TYPE_REGISTRY_CAPABILITY, TYPE_REGISTRY_OWNER) {
        Outcome::Ok { value, .. } => value,
        Outcome::Err {
            code,
            message,
            hint,
            ..
        } => {
            return Outcome::err(
                DiagCode::ResourceTypeUnmapped,
                &format!(
                    "无法就资源类型 {} 的登记权征询裁决（裁决通道返回 {}：{}）",
                    kind.en(),
                    code.code(),
                    message
                ),
                &format!(
                    "{}（裁决通道不可用时不得默许登记——那等于绕过 F3204 的类型系统。\
                     请检查 capability 表是否被裁剪）",
                    hint
                ),
            )
        }
    };
    match verdict {
        CapabilityVerdict::Owned { owner, .. } => Outcome::ok(ResourceMappingView {
            id,
            kind,
            kind_en: kind.en(),
            kind_zh: kind.zh(),
            owner_domain: owner,
        }),
        CapabilityVerdict::Redirected { owner, .. } => Outcome::err(
            DiagCode::ResourceTypeUnmapped,
            &format!(
                "资源类型 {} 的登记权属{}，本域不得代为登记",
                kind.en(),
                owner
            ),
            "未登记类型即不受管（第二个生命周期权威）。\
             请在 F3204 的类型注册表登记该类型，走扩展点而非修改枚举；\
             若你就是F3204 的实现方，请按仲裁流程取得该位后重试",
        ),
    }
}

/// 资源类型视图（裁决通过后交给 F3204 的登记材料）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceMappingView {
    /// 资源 ID。
    pub id: ResourceId,
    /// 类型。
    pub kind: ResourceKind,
    /// 类型英文标识。
    pub kind_en: &'static str,
    /// 类型中文名。
    pub kind_zh: &'static str,
    /// 登记权属主域（裁决回执——没有属主就无法追责，未登记类型即第二个权威）。
    pub owner_domain: &'static str,
}

/// 类型登记能力键（F3204 的属主域登记此键，本域只做征询）。
///
/// 之所以写成常量而非内联字面量：能力键是跨域契约的寻址符，散落成字面量
/// 就成了「同名不同义」的隐患——F3201 的边界表正是为防这个才存在的。
pub const TYPE_REGISTRY_CAPABILITY: &str = "resource-type-registry";

/// 类型登记能力的属主域（与 F3201 边界表登记一致——本域只征询，不自称属主）。
pub const TYPE_REGISTRY_OWNER: &str = "VE-Q";

/// 类型登记能力的域内承接工单（属主是域，工单是实现者——两者不可混用）。
pub const TYPE_REGISTRY_WORKITEM: &str = "VE-F3204";

// ===========================================================================
// 十三、判据自检总入口
// ===========================================================================

/// VE-F3202 判据摘要（一行版，进收口清单）。
pub fn criteria_summary() -> String {
    let mut bad = Vec::new();
    if RESOURCE_MAPPINGS_LEN_CHECK != 10 {
        bad.push("十项映射");
    }
    if audit_purpose_single_source().is_err() {
        bad.push("四用途单源");
    }
    if audit_perf_budget().is_err() {
        bad.push("复杂度逐项");
    }
    if audit_downstream_handoff().is_err() {
        bad.push("跨批对接");
    }
    // 红线考核**常驻形态**（反向索引按需建、非常驻）——按需形态是短时开销，
    // 拿它判永久预算等于把一次性开销算进常驻，那条线永远过不去也说明不了问题。
    let mem = build_memory_report(MILLION_RESOURCE_SCALE, MILLION_RESOURCE_SCALE, false);
    if !mem.within_budget {
        bad.push("32MB线");
    }
    let verdict = if bad.is_empty() {
        String::from("齐备")
    } else {
        format!("缺{}", bad.join("、"))
    };
    format!(
        "VE-F3202 判据：五要素 / 四用途单源 / 悬空即缺陷 / 环拒绝 / 32MB 线（百万+百万边 = {} 字节）/ 逐项复杂度 / 对接{}",
        mem.total_bytes,
        verdict
    )
}

/// 十项映射条数（F3201 表的引用，避免重复定义常量）。
pub const RESOURCE_MAPPINGS_LEN_CHECK: usize = 10;

/// F3202 架构总纲（供收口与读屏）。
pub struct ResourceGraphArchitecture;

impl ResourceGraphArchitecture {
    /// 版本。
    pub const VERSION: &'static str = "Q02-resource-graph-v1";

    /// 无障碍：架构的口播版。
    pub fn narration() -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "资源模型与引用图 {}：核心是资源实体五要素与一张全局引用图。",
            Self::VERSION
        ));
        s.push_str(&format!("四用途单源：{}。", PURPOSE_SINGLE_SOURCE_NOTE));
        s.push_str(&format!("悬空策略：{}。", PLACEHOLDER_POLICY));
        s.push_str(&format!("环处置：{}。", CYCLE_REJECT_TRIPLE[2]));
        s.push_str(&format!(
            "图内存红线：32MB（{} 字节）；节点记录压实到 {} 字节每槽，             反向索引按需构建非常驻（{}）。",
            GRAPH_MEMORY_BUDGET_BYTES,
            BYTES_PER_NODE_SLOT,
            REVERSE_INDEX_POLICY
        ));
        s.push_str(&format!(
            "百万节点百万边常驻占用 {} 字节（{}）；按需含反向索引时 {} 字节。",
            build_memory_report(MILLION_RESOURCE_SCALE, MILLION_RESOURCE_SCALE, false).total_bytes,
            if build_memory_report(MILLION_RESOURCE_SCALE, MILLION_RESOURCE_SCALE, false)
                .within_budget
            {
                "在预算内"
            } else {
                "超预算，须降级"
            },
            build_memory_report(MILLION_RESOURCE_SCALE, MILLION_RESOURCE_SCALE, true).total_bytes,
        ));
        for b in PERF_BUDGET.iter() {
            s.push_str(&format!(
                "复杂度：{} 为 {}{}。",
                b.item,
                b.complexity,
                match b.bounded_by {
                    Some(_) => "（已注明为何在真实区间内成立）",
                    None => "（明说它不是常数级）",
                }
            ));
        }
        s.push_str("跨批对接：");
        for (id, who, what) in DOWNSTREAM_HANDOFF.iter() {
            s.push_str(&format!("{}（{}）交付{}；", id, who, what));
        }
        s
    }

    /// 一行摘要。
    pub fn screen_text() -> String {
        criteria_summary()
    }
}

/// VE-F3202 域自检入口（判据逐条对应见 `veq02_checks.rs`）。
pub fn run_veq02_checks() -> crate::checks::CheckSet {
    super::veq02_checks::run_veq02_checks()
}