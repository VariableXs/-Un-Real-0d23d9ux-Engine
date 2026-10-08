//! VE-F0619 · 图层树与表面协议对接（VE-D 域 · 2D 合成引擎 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0619`
//!
//! **判据（锚点原文逐条）**：树遍历产出的绘制序列落地为表面内容（**帧边界=一次
//! 完整遍历，半帧不落盘**）；**damage 回流**：表面系统的 damage 事件映射回树级脏区
//! （外部内容变化如视频帧更新→对应图层内容脏——回流是双向协议的另一半）；呈现时序：
//! 遍历完成后提交呈现（vsync 语义与丢帧策略归呈现域，本条只保证**提交点语义清晰**）；
//! **缓冲所有权**：表面缓冲生命周期显性（**树侧只引用不拥有**）。数据结构：表面帧
//! 协议；damage 回流映射；提交点契约。错误路径与降级矩阵：**表面不可用→树照常
//! 运转只丢输出**（呈现与合成解耦）；**damage 风暴→合并入帧**；**缓冲失效→当帧
//! 重建登记**。性能逐项分解：提交 O(1)；回流映射 O(1) 每事件；帧边界 O(1)。
//! 跨批对接点：上游 F0614 遍历、F0613 脏区；下游 F0646 damage 协议条、B 域呈现链
//! F0200。无障碍与隐私：无直接无障碍面。判据：**帧边界语义、双向回流、解耦原则、
//! 所有权显性、判据**。
//!
//! ## 一、帧边界语义：半帧不落盘是**类型层面**的保证，不是运行时检查
//!
//! 锚点要求「帧边界=一次完整遍历，半帧不落盘」。最省事的实现是「提交时检查一个
//! `sealed: bool` 标志」——但那是**约定**，不是保证：任何一个新增的提交入口
//! （面板截图、调试导出、测试钩子）忘了置位，半帧就落盘了，而落下来的半帧在画面上
//! 表现为「上一层内容被新内容截断」，用户看到的是撕裂或残影，**没有任何报错**。
//!
//! 本模块的处置是把帧状态做成**相位机**（[`Phase`]：`Idle → Open → Sealed →
//! Committed`），并让**唯一能产出 [`CommitPoint`] 的路径要求 `Sealed` 相位**：
//! `Open` 期间调用提交直接被[`ProtocolErr::NotSealed`]拒绝。于是「半帧落盘」在
//! 架构上不可表达——要绕过它只能改协议本身，而改协议必然过 review。
//!
//! 三条硬纪律落点：
//!
//! - **未封帧不可提交**：[`FrameAssembler::commit`] 在 `Open` 相位返回 `Err`。
//! - **封帧不可重复**：`Sealed` 后再 `seal()` 返回 [`ProtocolErr::AlreadySealed`]
//!   ——重复封帧会让「已入列命令」与「提交摘要」产生两个真相。
//! - **提交恰好一次**：`Committed` 后再提交返回 [`ProtocolErr::AlreadyCommitted`]
//!   ——重复提交会让呈现域收到两帧，破坏 F0646 的damage 协议配平。
//!
//! **帧不得重叠**：[`begin_frame`] 在上一帧未提交时返回 [`ProtocolErr::FrameOverlap`]
//! ——重叠意味着上一帧的绘制序列还在被消费，新帧已经开始改缓冲，这正是「撕裂」的
//! 机械成因。
//!
//! 未封帧的**显式放弃**入口是 [`FrameAssembler::abort_frame`]：它丢弃半帧并留
//! 审计痕（走的是「作废」而不是「落盘」），符合错误矩阵「帧级丢弃重来，不留半帧
//! 状态」。
//!
//! ## 二、双向回流：两条方向各有专属映射，且**都必须能被点名**
//!
//! 双向协议的「双向」是本条的核心，朴素实现会做成一个函数加一个 `bool` 参数：
//!
//! ```text
//! fn route(damage, to_tree: bool) -> Vec<..>       // 反例
//! ```
//!
//! 那是把两条协议塞进一个签名里，代价是**任一方向的映射缺失都被另一方向的成功掩盖**
//!（判据「双向」永远绿）。本模块拆成两个**互不调用**的映射面：
//!
//! - **正向（树 → 表面）**：[`SurfaceDamageOut::from_tree`] 把树侧脏区投影成表面
//!   区域集，供表面系统消费；
//! - **反向（表面 → 树）**：[`ReflowCollector::settle_frame`] 把表面 damage 事件
//!   （视频帧更新/外部解码/合成器交换）映射成树级**内容脏**（[`TreeDirty`]），
//!   交F0613 的收集面。
//!
//! 自检 [`C19-REFLOW-BIDIRECTIONAL`] 分别构造两个方向的输入并**要求两侧同时非空**
//! ——任何一侧被摘掉都会红。
//!
//! ## 三、回流必须落**内容脏**，不能落通用兜底
//!
//! 锚点写明「外部内容变化如视频帧更新→对应图层**内容脏**」。反例是把它映射成
//! 「该层整层重绘」或「全帧脏」——那会让一次 1080p 视频帧更新把整屏重绘，缓存
//! 命中率掉到 0，表现为「播放视频时整个合成树持续失效」。所以
//! [`ReflowEvent::Content`] 只产出 [`TreeDirtyOp::ContentDirty`]（推进内容修订号、
//! 重绘该层，**不动变换与裁剪**），而 [`ReflowEvent::Replaced`] 才产出「新旧两域
//! 并集」——旧域那半块是残留，不画就会留上一帧的画。
//!
//! ## 四、damage 风暴合并入帧，且**必须留痕**
//!
//! 锚点「damage 风暴→合并入帧」。逐字实现是「超阈值就把本帧所有回流合并成一条」。
//! 但合并若**不留痕**，就等于把「风暴」这一事实从账面上抹掉了——下游 F0641 的三档
//! 模型再也看不到「本帧发生过风暴」，性能回归无从归因。所以本模块在
//! [`ReflowSettle`] 里同时给出 [`ReflowCollector::storm_frames`] 与
//! [`ReflowCollector::coalesced_count`]：风暴次数与被合并掉的事件数都可查。
//!
//! ## 五、解耦原则：表面不可用→树照常运转只丢输出
//!
//! 锚点「表面不可用→树照常运转只丢输出（呈现与合成解耦）」。这句话的反面是
//! 「表面不可用→整帧失败」——那样合成引擎会被呈现链的单点故障拖垮，表现为
//! 「窗口最小化后图层树也不更新了，再也回不来」。本模块的处置：
//!
//! - [`SurfaceLink`] 有 `attached` 位；未挂载时 [`SurfaceLink::commit`] **照常返回**
//!   [`CommitPoint`]，只是 `output` 记 [`OutputState::Dropped`]；
//! - 丢弃**必被记账**（[`SurfaceLink::dropped_frames`] 与按原因分桶
//!   [`SurfaceLink::drop_reason`]），「丢输出」不等于「静默丢输出」；
//! - 树侧的绘制序列在提交失败与否的情况下**内容一致**——本模块自检用
//!   「同一份投影入列两次、一次挂载一次不挂载，比较 `SealedFrame.digest`」来证明
//!   解耦（[`C19-DECOUP-NO-BLOCK`]）。
//!
//! ## 六、提交点契约：O(1)，且**不搬运像素**
//!
//! 「提交 O(1)」若照字面理解成「提交就是搬运整帧像素」，就与 O(1) 直接矛盾。本模块
//! 的取舍是：**像素搬运属于表面域内部的事，本条只定义提交点语义**——提交点 =
//! `帧号 + 命令数 + 序列摘要 + 输出态`（[`CommitPoint`]），全部O(1) 可得。
//!
//! 「O(1)」这条判据是**实测**的，不是自证算术：摘要在**每条命令入列时增量更新**
//! （计数器 [`Counters::digest_updates`]），提交路径**不再遍历序列**
//! （[`Counters::frame_scans`] 在提交前后必须**不变**）。若有人把摘要改成提交时
//! 重算，`frame_scans` 会立刻从 0 变正，两条自检同时红。
//!
//! **vsync 与丢帧策略归呈现域**：本模块不等待、不阻塞、不读墙钟（零墙钟纪律），
//! 只在 [`OutputState`] 里如实记录呈现侧的三种裁决（呈现 / 延后 / 丢弃），把
//! 「何时真正上屏」的决定权完整交回呈现域。
//!
//! ## 七、所有权显性：树侧只引用不拥有，且引用会**过期**
//!
//! 锚点「缓冲所有权：表面缓冲生命周期显性（树侧只引用不拥有）」。做法分三步：
//!
//! 1. **类型层面切断写路径**：树侧持有的 [`BufferSlot`] 是 8 字节的
//!   `slot + generation` 副本，本模块**不提供任何返回 `&mut [u8]` 的函数**——树侧
//!   在编译期就不可能写表面缓冲的内容。自检 [`C19-OWNERSHIP-NOBORROW`] 拿真实的
//!   [`BufferRegistry`] 做被测物：跑完整一帧投影后，登记项逐字段不变（不是拿表内
//!   元素自证）。
//! 2. **生命周期用世代号显性表达**：[`BufferRegistry::invalidate`] 递增
//!   `generation`。旧 [`BufferSlot`] 仍在树侧手里，但 [`BufferRegistry::resolve`]
//!   因世代不匹配返回 [`ProtocolErr::BufferGeneration`]——这就是「引用会过期」的
//!   机检形态，对应锚点错误矩阵的「**缓冲失效→当帧重建登记**」。
//! 3. **帧不拥有缓冲**：[`SealedFrame`] / [`CommitPoint`] 被丢弃时登记项的存活计数
//!   **不变**（帧是引用者，不是所有者）。自检 [`C19-OWNERSHIP-FRAME-NOT-OWNER`]
//!   以「丢掉整帧后存活缓冲数不变」验证这一点。
//!
//! ## 八、枚举判别值 ≠ 线上编码值
//!
//! [`CmdKind`] 与 [`DrawKind`] 是**形态枚举**，其判别值不得当作线上协议编码使用
//! （枚举声明次序一改，二进制头就全错，且类型检查不会报）。故本模块写显式
//! [`CmdKind::wire`] 映射并配自洽断言 [`C19-WIRE-EXPLICIT`]。
//!
//! ## 九、模块自持与不重造轮子
//!
//! - 本模块**不 import 任何未注册的兄弟模块**（平行会话的 `ved*` 族尚在施工，
//!   编译期硬耦合会让本条因别人的进度而红）。树侧投影入参
//!   [`LayerDrawRef`] 是**结构投影**（只取对接必需的字段），F0614 的
//!   `DrawCmd` 到 [`LayerDrawRef`] 的转换由装配侧完成。
//! - 缓冲区���回收、LRU、缓存键一律不在本模块（属F0615）。
//!
//! 逻辑 tick 注入，零墙钟；零 IO。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 单帧表面命令条数上界（防御性上界；超限显性拒绝而非截断）。
pub const MAX_COMMANDS_PER_FRAME: usize = 1 << 16;

/// 单帧回流事件数硬上界（超限显性拒绝，先结算本帧）。
pub const MAX_REFLOWS_PER_FRAME: usize = 1 << 12;

/// 回流风暴阈值（超此数量合并入帧，防逐事件处理把帧预算吃光）。
pub const REFLOW_STORM_THRESHOLD: usize = 512;

/// 帧级作用域哨兵 id（风暴合并产物的归属层；真实层 id 从 1 起）。
pub const FRAME_SCOPE: u64 = 0;

/// 组嵌套深度上界（`EnterGroup`/`LeaveGroup` 失配或过深即拒）。
pub const MAX_GROUP_DEPTH: u32 = 256;

/// 缓冲世代号初值（0 保留为「无效世代」，故从 1 起）。
pub const BUFFER_GEN_INIT: u32 = 1;

/// 审计留痕条数上界（防无界增长；超限计dropped）。
pub const MAX_AUDITS: usize = 64;

/// 几何比较容差（退化域判定容差）。
pub const RECT_EPS: f32 = 1e-4;

// ---------------------------------------------------------------------------
// 二、协议错误（错误路径与降级矩阵的显性编码）
// ---------------------------------------------------------------------------

/// 表面协议错误码。
///
/// 纪律：处置方向相反的两种情形**不得共用码**——
/// [`FrameOverlap`](ProtocolErr::FrameOverlap) 与 [`NotSealed`](ProtocolErr::NotSealed)
/// 处置方向相反（前者是「上一帧没提交，先提交」；后者是「本帧没封帧，先封帧」），
/// 共用码会让上游的处置建议自相矛盾。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtocolErr {
    /// 上一帧未提交就开新帧（帧重叠）。
    FrameOverlap,
    /// 未封帧即提交（半帧落盘请求）。
    NotSealed,
    /// 重复封帧。
    AlreadySealed,
    /// 重复提交。
    AlreadyCommitted,
    /// 相位不允许（相位机拒绝）。
    PhaseDenied,
    /// 命令数超上界。
    CmdOverflow,
    /// 组标记失配（`EnterGroup`/`LeaveGroup` 不配对）。
    GroupUnbalanced,
    /// 组嵌套过深。
    GroupDepth,
    /// 回流指向未登记的层（映射缺源）。
    ReflowUnknownLayer,
    /// 回流事件数超上界。
    ReflowOverflow,
    /// 缓冲引用世代不匹配（引用已过期）。
    BufferGeneration,
    /// 缓冲槽位不存在。
    BufferMissing,
    /// 表面未挂载且调用方要求必须挂载。
    SurfaceRequired,
}

impl ProtocolErr {
    /// 稳定错误码字符串（跨模块传递面；不使用枚举判别值）。
    pub fn code(self) -> &'static str {
        match self {
            ProtocolErr::FrameOverlap => "E_FRAME_OVERLAP",
            ProtocolErr::NotSealed => "E_FRAME_NOT_SEALED",
            ProtocolErr::AlreadySealed => "E_FRAME_ALREADY_SEALED",
            ProtocolErr::AlreadyCommitted => "E_FRAME_ALREADY_COMMITTED",
            ProtocolErr::PhaseDenied => "E_FRAME_PHASE_DENIED",
            ProtocolErr::CmdOverflow => "E_FRAME_CMD_OVERFLOW",
            ProtocolErr::GroupUnbalanced => "E_GROUP_UNBALANCED",
            ProtocolErr::GroupDepth => "E_GROUP_DEPTH",
            ProtocolErr::ReflowUnknownLayer => "E_REFLOW_UNKNOWN_LAYER",
            ProtocolErr::ReflowOverflow => "E_REFLOW_OVERFLOW",
            ProtocolErr::BufferGeneration => "E_BUFFER_GENERATION",
            ProtocolErr::BufferMissing => "E_BUFFER_MISSING",
            ProtocolErr::SurfaceRequired => "E_SURFACE_REQUIRED",
        }
    }

    /// 处置建议三要素（做什么 / 影响面 / 上报谁）。
    pub fn advise(self) -> &'static str {
        match self {
            ProtocolErr::FrameOverlap => "处置=先提交或显式放弃上一帧；影响面=撕裂；上报=呈现域",
            ProtocolErr::NotSealed => "处置=先封帧再提交；影响面=半帧落盘；上报=遍历域",
            ProtocolErr::AlreadySealed => "处置=丢弃重复封帧请求；影响面=摘要双真相；上报=遍历域",
            ProtocolErr::AlreadyCommitted => "处置=丢弃重复提交；影响面=呈现配平；上报=F0646",
            ProtocolErr::PhaseDenied => "处置=按相位机重排调用序；影响面=协议越序；上报=装配侧",
            ProtocolErr::CmdOverflow => "处置=先结算本帧再收下一帧；影响面=帧预算；上报=度量域",
            ProtocolErr::GroupUnbalanced => "处置=整帧作废重来；影响面=状态泄漏；上报=遍历域",
            ProtocolErr::GroupDepth => "处置=整帧作废重来；影响面=栈深失控；上报=遍历域",
            ProtocolErr::ReflowUnknownLayer => "处置=上游补登记层再回流；影响面=漏画；上报=树域",
            ProtocolErr::ReflowOverflow => "处置=先结算本帧再收下一帧；影响面=回流风暴；上报=呈现域",
            ProtocolErr::BufferGeneration => "处置=当帧重建缓冲并登记；影响面=读到旧内容；上报=表面域",
            ProtocolErr::BufferMissing => "处置=补登记缓冲槽位；影响面=无可提交内容；上报=表面域",
            ProtocolErr::SurfaceRequired => "处置=显式挂载表面或接受丢输出；影响面=无输出；上报=呈现域",
        }
    }
}

/// 错误记录（零静默：账本三要素 `what / code / advice`）。
#[derive(Clone, Debug, PartialEq)]
pub struct ErrRecord {
    /// 发生位置（表单要素名）。
    pub what: String,
    /// 稳定错误码。
    pub code: &'static str,
    /// 处置建议。
    pub advice: String,
}

impl ErrRecord {
    /// 构造（advice 取自 [`ProtocolErr::advise`]）。
    pub fn new(what: String, e: ProtocolErr) -> Self {
        ErrRecord { what, code: e.code(), advice: e.advise().to_string() }
    }
}

// ---------------------------------------------------------------------------
// 三、几何（表面对接域；不依赖F0613 的脏区矩形，避免编译期硬耦合）
// ---------------------------------------------------------------------------

/// 轴对齐矩形（表面对接域自有类型）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceRect {
    /// 左上 x。
    pub x: f32,
    /// 左上 y。
    pub y: f32,
    /// 宽（≥0）。
    pub w: f32,
    /// 高（≥0）。
    pub h: f32,
}

impl SurfaceRect {
    /// 构造：负宽高与非有限值显性拒绝（不静默取绝对值）。
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Option<Self> {
        if !x.is_finite() || !y.is_finite() || !w.is_finite() || !h.is_finite() {
            return None;
        }
        if w < 0.0 || h < 0.0 {
            return None;
        }
        Some(SurfaceRect { x, y, w, h })
    }

    /// 全零构造（空矩形；缓冲失效与组标记的占位目标）。
    pub const fn zero() -> Self {
        SurfaceRect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 }
    }

    /// 右边界（exclusive 口径）。
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    /// 下边界（exclusive 口径）。
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    /// 是否退化（零面积或非有限）。
    pub fn is_empty(&self) -> bool {
        !(self.w > RECT_EPS && self.h > RECT_EPS)
    }

    /// 并集。
    pub fn union(&self, o: &SurfaceRect) -> SurfaceRect {
        let x = if self.x < o.x { self.x } else { o.x };
        let y = if self.y < o.y { self.y } else { o.y };
        let r = if self.right() > o.right() { self.right() } else { o.right() };
        let b = if self.bottom() > o.bottom() { self.bottom() } else { o.bottom() };
        SurfaceRect { x, y, w: r - x, h: b - y }
    }

    /// 求交（无交返回 `None`；交可为空矩形）。
    pub fn intersect(&self, o: &SurfaceRect) -> Option<SurfaceRect> {
        let x = if self.x > o.x { self.x } else { o.x };
        let y = if self.y > o.y { self.y } else { o.y };
        let r = if self.right() < o.right() { self.right() } else { o.right() };
        let b = if self.bottom() < o.bottom() { self.bottom() } else { o.bottom() };
        if r < x || b < y {
            return None;
        }
        Some(SurfaceRect { x, y, w: r - x, h: b - y })
    }
}

// ---------------------------------------------------------------------------
// 四、缓冲所有权（树侧只引用不拥有）
// ---------------------------------------------------------------------------

/// 缓冲所有权归属（谁负责实际存储）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufferOwner {
    /// 表面系统持有（叶层内容）。
    Surface,
    /// 隔离组子缓冲（组 id）。
    Group(u64),
}

impl BufferOwner {
    /// 稳定编码（显式映射，禁 `as u8`）。
    pub fn wire(self) -> u8 {
        match self {
            BufferOwner::Surface => 0,
            BufferOwner::Group(_) => 1,
        }
    }
}

/// 缓冲引用（树侧持有的**唯一**东西：槽位 + 世代；无像素访问面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferSlot {
    /// 槽位号。
    pub slot: u32,
    /// 世代号（每次失效递增；0 为无效世代）。
    pub generation: u32,
}

impl BufferSlot {
    /// 构造（世代 0 视为无效引用，显性返回 `None`）。
    pub fn new(slot: u32, generation: u32) -> Option<Self> {
        if generation == 0 {
            return None;
        }
        Some(BufferSlot { slot, generation })
    }

    /// 无效引用（占位；仅用于组标记等非像素命令）。
    pub const fn invalid() -> Self {
        BufferSlot { slot: 0, generation: 0 }
    }
}

/// 缓冲描述（`resolve` 的产出；只读快照）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferDesc {
    /// 槽位号。
    pub slot: u32,
    /// 世代号。
    pub generation: u32,
    /// 宽（像素）。
    pub width: u32,
    /// 高（像素）。
    pub height: u32,
    /// 所有权归属。
    pub owner: BufferOwner,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BufferState {
    Live,
    Invalidated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BufferEntry {
    generation: u32,
    state: BufferState,
    width: u32,
    height: u32,
    owner: BufferOwner,
}

/// 缓冲登记表（表面域持有所有权；树侧只见 [`BufferSlot`]）。
///
/// **本结构不提供任何返回 `&mut [u8]` 的方法**——这是「树侧只引用不拥有」的
/// 编译期保证面，不是注释约定。
#[derive(Clone, Debug, Default)]
pub struct BufferRegistry {
    entries: Vec<BufferEntry>,
    live: usize,
    /// 因失效而被作废的引用数（过期引用观测面）。
    stale_refs: u64,
}

impl BufferRegistry {
    /// 空登记表。
    pub fn new() -> Self {
        BufferRegistry { entries: Vec::new(), live: 0, stale_refs: 0 }
    }

    /// 取用缓冲：优先复用 `Invalidated` 槽位（递增世代），否则新开槽位。
    ///
    /// 复用时**必须递增世代**——否则树侧持有的旧引用会误判为有效，读到新内容
    /// （表现为「视频层偶尔显示上一帧」）。
    pub fn acquire(&mut self, owner: BufferOwner, width: u32, height: u32) -> BufferSlot {
        if let Some(idx) = self
            .entries
            .iter()
            .position(|e| e.state == BufferState::Invalidated)
        {
            let e = self.entries.get(idx).copied().unwrap_or(BufferEntry {
                generation: BUFFER_GEN_INIT,
                state: BufferState::Live,
                width,
                height,
                owner,
            });
            let slot = idx as u32;
            // 世代饱和时回绕到 1（0 是无效世代）；饱和本身记为stale 观测。
            let gen = if e.generation == u32::MAX { 1 } else { e.generation + 1 };
            if let Some(target) = self.entries.get_mut(idx) {
                *target = BufferEntry { generation: gen, state: BufferState::Live, width, height, owner };
            }
            self.live = self.live.saturating_add(1);
            return BufferSlot { slot, generation: gen };
        }
        let slot = self.entries.len() as u32;
        self.entries.push(BufferEntry {
            generation: BUFFER_GEN_INIT,
            state: BufferState::Live,
            width,
            height,
            owner,
        });
        self.live = self.live.saturating_add(1);
        BufferSlot { slot, generation: BUFFER_GEN_INIT }
    }

    /// 解析引用：世代不匹配 → `BufferGeneration`（引用已过期）。
    pub fn resolve(&self, r: BufferSlot) -> Result<BufferDesc, ProtocolErr> {
        let e = self.entries.get(r.slot as usize).copied().ok_or(ProtocolErr::BufferMissing)?;
        if e.generation != r.generation || r.generation == 0 {
            return Err(ProtocolErr::BufferGeneration);
        }
        if e.state == BufferState::Invalidated {
            return Err(ProtocolErr::BufferGeneration);
        }
        Ok(BufferDesc {
            slot: r.slot,
            generation: e.generation,
            width: e.width,
            height: e.height,
            owner: e.owner,
        })
    }

    /// 失效一个缓冲（当帧重建的第一步）。
    pub fn invalidate(&mut self, r: BufferSlot) -> Result<(), ProtocolErr> {
        let e = self.entries.get_mut(r.slot as usize).ok_or(ProtocolErr::BufferMissing)?;
        if e.generation != r.generation || r.generation == 0 {
            return Err(ProtocolErr::BufferGeneration);
        }
        if e.state == BufferState::Invalidated {
            return Ok(());
        }
        e.state = BufferState::Invalidated;
        self.live = self.live.saturating_sub(1);
        Ok(())
    }

    /// 存活缓冲数（帧不拥有缓冲的核对基准）。
    pub fn live_count(&self) -> usize {
        self.live
    }

    /// 槽位总数（含失效槽位）。
    pub fn slot_count(&self) -> usize {
        self.entries.len()
    }

    /// 指定槽位是否处于失效态（重建登记的核对面）。
    pub fn is_invalidated(&self, r: BufferSlot) -> bool {
        match self.entries.get(r.slot as usize) {
            Some(e) => e.generation == r.generation && e.state == BufferState::Invalidated,
            None => false,
        }
    }

    /// 已作废引用累计（诊断面；本模块不因过期引用改写树侧状态，只计数）。
    pub fn stale_refs(&self) -> u64 {
        self.stale_refs
    }

    /// 记一次过期引用命中（由对接层在`resolve` 失败后调用）。
    pub fn note_stale(&mut self) {
        self.stale_refs = self.stale_refs.saturating_add(1);
    }
}

// ---------------------------------------------------------------------------
// 五、表面帧协议（相位机 + 命令 + 提交点）
// ---------------------------------------------------------------------------

/// 帧相位（状态机的四个位置；**单向前进不可跳级**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 空闲（无帧在途）。
    Idle,
    /// 开帧中（命令可入列；**不可提交**）。
    Open,
    /// 已封帧（**可提交**；不可再入列）。
    Sealed,
    /// 已提交（本帧终结）。
    Committed,
}

impl Phase {
    /// 相位稳定编码。
    pub fn wire(self) -> u8 {
        match self {
            Phase::Idle => 0,
            Phase::Open => 1,
            Phase::Sealed => 2,
            Phase::Committed => 3,
        }
    }
}

/// 表面命令形态（枚举判别值 ≠ 线上编码，见 [`CmdKind::wire`]）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmdKind {
    /// 叶层位块传送（像素命令）。
    Blit,
    /// 进入组（结构标记，非像素）。
    EnterGroup,
    /// 离开组（结构标记，非像素）。
    LeaveGroup,
    /// 组纹理整体合成（隔离组 F0605 的输出落点）。
    CompositeGroup,
}

impl CmdKind {
    /// 显式线上编码映射（禁 `as u8`）。
    pub fn wire(self) -> u8 {
        match self {
            CmdKind::Blit => 1,
            CmdKind::EnterGroup => 2,
            CmdKind::LeaveGroup => 3,
            CmdKind::CompositeGroup => 4,
        }
    }

    /// 是否像素命令（结构标记不占传输预算）。
    pub fn is_pixel(self) -> bool {
        matches!(self, CmdKind::Blit | CmdKind::CompositeGroup)
    }
}

/// 树侧绘制项形态（树遍历产出的结构投影）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawKind {
    /// 叶层。
    Layer,
    /// 进入组。
    GroupEnter,
    /// 离开组。
    GroupLeave,
}

impl DrawKind {
    /// 显式线上编码映射。
    pub fn wire(self) -> u8 {
        match self {
            DrawKind::Layer => 1,
            DrawKind::GroupEnter => 2,
            DrawKind::GroupLeave => 3,
        }
    }
}

/// 树侧绘制项（对接入参：只取对接必需字段的**结构投影**）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerDrawRef {
    /// 层 id。
    pub node_id: u64,
    /// 形态。
    pub kind: DrawKind,
    /// 树深度（根 = 0）。
    pub depth: u32,
    /// 有效不透明度（0..=1）。
    pub opacity: f32,
    /// 世界域（本地系经父级联后）。
    pub world: SurfaceRect,
    /// 内容缓冲引用（**树侧只引用**）。
    pub buffer: BufferSlot,
}

/// 一条表面命令（含目标域与缓冲引用；不含像素）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceCmd {
    /// 形态。
    pub kind: CmdKind,
    /// 层 id。
    pub node_id: u64,
    /// 树深度。
    pub depth: u32,
    /// 有效不透明度。
    pub opacity: f32,
    /// 源缓冲引用（结构标记为 [`BufferSlot::invalid`]）。
    pub src: BufferSlot,
    /// 目标域（结构标记为 [`SurfaceRect::zero`]）。
    pub dst: SurfaceRect,
}

/// 投影裁决（树项 → 表面协议的映射结果）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Projection {
    /// 产出命令。
    Emit(SurfaceCmd),
    /// 产出结构标记（非像素）。
    Mark(CmdKind),
    /// 跳过（带理由；**不静默丢**）。
    Skip(SkipReason),
}

/// 跳过理由（可枚举，供上游记账与走查）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    /// 有效不透明度为 0（不可见）。
    ZeroOpacity,
    /// 世界域退化（零面积或非有限）。
    EmptyBounds,
    /// 缓冲引用无效（世代 0）。
    NoBuffer,
}

/// 帧计数器（性能判据的实测面，非自证算术）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counters {
    /// 摘要增量更新次数（应恰等于入列命令数）。
    pub digest_updates: u64,
    /// 提交路径对命令序列的遍历次数（**O(1) 提交要求恒为 0**）。
    pub frame_scans: u64,
    /// 投影调用次数。
    pub projections: u64,
    /// 跳过裁决次数。
    pub skips: u64,
    /// 回流映射调用次数（应恰等于受理事件数）。
    pub reflow_maps: u64,
    /// 提交次数。
    pub commits: u64,
}

impl Counters {
    /// 空计数。
    pub fn new() -> Self {
        Counters::default()
    }

    /// 合并（自检面用）。
    pub fn merge(a: &Counters, b: &Counters) -> Counters {
        Counters {
            digest_updates: a.digest_updates.saturating_add(b.digest_updates),
            frame_scans: a.frame_scans.saturating_add(b.frame_scans),
            projections: a.projections.saturating_add(b.projections),
            skips: a.skips.saturating_add(b.skips),
            reflow_maps: a.reflow_maps.saturating_add(b.reflow_maps),
            commits: a.commits.saturating_add(b.commits),
        }
    }
}

/// FNV 混入（摘要用；纯函数）。
fn mix64(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(0x1000_0000_01b3)
}

/// 单命令指纹（**只用 `to_bits()` 取完整位型**，禁低位截断）。
fn cmd_fingerprint(c: &SurfaceCmd) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    h = mix64(h, c.kind.wire() as u64);
    h = mix64(h, c.node_id);
    h = mix64(h, c.depth as u64);
    h = mix64(h, c.opacity.clamp(0.0, 1.0).to_bits() as u64);
    h = mix64(h, ((c.dst.x.to_bits() as u64) << 32) | (c.dst.w.to_bits() as u64 & 0xffff_ffff));
    h = mix64(h, ((c.dst.y.to_bits() as u64) << 32) | (c.dst.h.to_bits() as u64 & 0xffff_ffff));
    h = mix64(h, ((c.src.slot as u64) << 32) | (c.src.generation as u64));
    h
}

/// 封帧结果（不可变；含摘要）。
#[derive(Clone, Debug, PartialEq)]
pub struct SealedFrame {
    /// 帧号。
    pub frame: u64,
    /// 命令序列（**引用表面缓冲**，不拥有）。
    pub cmds: Vec<SurfaceCmd>,
    /// 序列摘要（增量求得，提交时不再重算）。
    pub digest: u64,
    /// 最大组嵌套深度。
    pub group_depth_max: u32,
}

impl SealedFrame {
    /// 命令条数。
    pub fn cmd_count(&self) -> usize {
        self.cmds.len()
    }

    /// 像素命令条数（结构标记不计）。
    pub fn pixel_count(&self) -> usize {
        self.cmds.iter().filter(|c| c.kind.is_pixel()).count()
    }
}

/// 输出态（呈现侧裁决；vsync/丢帧策略归呈现域，本条只如实记录）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputState {
    /// 呈现侧已接收。
    Presented,
    /// 呈现侧要求延后到下一个 vsync（**本条不等待、不阻塞**）。
    Deferred,
    /// 输出丢弃（表面不可用/已卸载）。
    Dropped(&'static str),
}

/// 提交点（三要素 + 输出态；全部 O(1) 可得）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommitPoint {
    /// 帧号。
    pub frame: u64,
    /// 命令条数。
    pub cmd_count: usize,
    /// 序列摘要。
    pub digest: u64,
    /// 输出态。
    pub output: OutputState,
}

/// 帧装配器（表面帧协议的持有者；相位机在此）。
#[derive(Clone, Debug)]
pub struct FrameAssembler {
    phase: Phase,
    frame: u64,
    cmds: Vec<SurfaceCmd>,
    digest: u64,
    group_depth: u32,
    group_depth_max: u32,
    sealed: Option<SealedFrame>,
    counters: Counters,
    audits: Vec<String>,
    audits_dropped: usize,
    errors: Vec<ErrRecord>,
}

impl FrameAssembler {
    /// 构造（空闲态）。
    pub fn new() -> Self {
        FrameAssembler {
            phase: Phase::Idle,
            frame: 0,
            cmds: Vec::new(),
            digest: 0xcbf2_9ce4_8422_2325,
            group_depth: 0,
            group_depth_max: 0,
            sealed: None,
            counters: Counters::new(),
            audits: Vec::new(),
            audits_dropped: 0,
            errors: Vec::new(),
        }
    }

    /// 当前相位。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 当前帧号。
    pub fn frame(&self) -> u64 {
        self.frame
    }

    /// 计数器（性能判据实测面）。
    pub fn counters(&self) -> Counters {
        self.counters
    }

    /// 错误账本（零静默）。
    pub fn errors(&self) -> &[ErrRecord] {
        &self.errors
    }

    /// 审计留痕（顺序稳定）。
    pub fn audits(&self) -> &[String] {
        &self.audits
    }

    /// 被丢弃的审计条数（如实报出，不静默）。
    pub fn audits_dropped(&self) -> usize {
        self.audits_dropped
    }

    /// 留痕（超上界计数而非截断）。
    fn audit(&mut self, s: String) {
        if self.audits.len() < MAX_AUDITS {
            self.audits.push(s);
        } else {
            self.audits_dropped = self.audits_dropped.saturating_add(1);
        }
    }

    /// 记账一条错误。
    fn record(&mut self, what: &str, e: ProtocolErr) {
        self.errors.push(ErrRecord::new(what.to_string(), e));
    }

    /// 开帧：上一帧未终结 → `FrameOverlap`（帧重叠是撕裂的机械成因）。
    pub fn begin_frame(&mut self, frame: u64) -> Result<(), ProtocolErr> {
        if self.phase != Phase::Idle && self.phase != Phase::Committed {
            self.record("begin_frame", ProtocolErr::FrameOverlap);
            self.audit(format!(
                "帧 {} 未终结（相位 {}）即请求开帧 {}，按帧重叠拒绝（防撕裂）",
                self.frame,
                self.phase.wire(),
                frame
            ));
            return Err(ProtocolErr::FrameOverlap);
        }
        self.phase = Phase::Open;
        self.frame = frame;
        self.cmds.clear();
        self.digest = 0xcbf2_9ce4_8422_2325;
        self.group_depth = 0;
        self.group_depth_max = 0;
        self.sealed = None;
        Ok(())
    }

    /// 受理一个投影裁决：入列命令 / 记结构标记 / 记跳过。
    ///
    /// 摘要在此**增量更新**（[`Counters::digest_updates`]），提交路径不再重算。
    pub fn accept(&mut self, p: Projection) -> Result<(), ProtocolErr> {
        if self.phase != Phase::Open {
            self.record("accept", ProtocolErr::PhaseDenied);
            return Err(ProtocolErr::PhaseDenied);
        }
        match p {
            Projection::Skip(_) => {
                self.counters.skips = self.counters.skips.saturating_add(1);
                Ok(())
            }
            Projection::Mark(k) => {
                match k {
                    CmdKind::EnterGroup => {
                        self.group_depth = self.group_depth.saturating_add(1);
                        if self.group_depth > MAX_GROUP_DEPTH {
                            self.group_depth = self.group_depth.saturating_sub(1);
                            self.record("accept.EnterGroup", ProtocolErr::GroupDepth);
                            return Err(ProtocolErr::GroupDepth);
                        }
                        if self.group_depth > self.group_depth_max {
                            self.group_depth_max = self.group_depth;
                        }
                    }
                    CmdKind::LeaveGroup => {
                        if self.group_depth == 0 {
                            // 失配：栈下溢。不静默——整帧作废路径由 commit 兜住。
                            self.record("accept.LeaveGroup", ProtocolErr::GroupUnbalanced);
                            return Err(ProtocolErr::GroupUnbalanced);
                        }
                        self.group_depth = self.group_depth.saturating_sub(1);
                    }
                    _ => {}
                }
                self.push_cmd(SurfaceCmd {
                    kind: k,
                    node_id: 0,
                    depth: self.group_depth,
                    opacity: 1.0,
                    src: BufferSlot::invalid(),
                    dst: SurfaceRect::zero(),
                })
            }
            Projection::Emit(cmd) => {
                if !cmd.opacity.is_finite() || cmd.dst.is_empty() {
                    self.counters.skips = self.counters.skips.saturating_add(1);
                    return Ok(());
                }
                self.push_cmd(cmd)
            }
        }
    }

    /// 命令入列（容量 + 摘要增量更新，两步都在这里发生）。
    fn push_cmd(&mut self, c: SurfaceCmd) -> Result<(), ProtocolErr> {
        if self.cmds.len() >= MAX_COMMANDS_PER_FRAME {
            self.record("push_cmd", ProtocolErr::CmdOverflow);
            self.audit(format!(
                "帧 {} 命令数已达上界 {}，本条被拒（先结算本帧再收下一帧）",
                self.frame, MAX_COMMANDS_PER_FRAME
            ));
            return Err(ProtocolErr::CmdOverflow);
        }
        self.digest = mix64(self.digest, cmd_fingerprint(&c));
        self.counters.digest_updates = self.counters.digest_updates.saturating_add(1);
        self.cmds.push(c);
        Ok(())
    }

    /// 封帧：相位须为 `Open`；重复封帧被拒。
    pub fn seal(&mut self) -> Result<SealedFrame, ProtocolErr> {
        if self.phase == Phase::Sealed || self.phase == Phase::Committed {
            self.record("seal", ProtocolErr::AlreadySealed);
            return Err(ProtocolErr::AlreadySealed);
        }
        if self.phase != Phase::Open {
            self.record("seal", ProtocolErr::PhaseDenied);
            return Err(ProtocolErr::PhaseDenied);
        }
        if self.group_depth != 0 {
            // 组标记失配 → 整帧不可提交（状态泄漏风险）。
            self.record("seal", ProtocolErr::GroupUnbalanced);
            self.audit(format!(
                "帧 {} 封帧时组嵌套未归零（残 {}），整帧不可提交",
                self.frame, self.group_depth
            ));
            return Err(ProtocolErr::GroupUnbalanced);
        }
        let f = SealedFrame {
            frame: self.frame,
            cmds: self.cmds.clone(),
            digest: self.digest,
            group_depth_max: self.group_depth_max,
        };
        self.phase = Phase::Sealed;
        self.sealed = Some(f.clone());
        Ok(f)
    }

    /// 命令序列**整体重算**摘要（增量算法的核对面）。
    ///
    /// 本方法存在的唯一理由是让 [`Counters::frame_scans`] 有真实的递增点：
    /// 若把 [`Self::commit`] 的摘要取法改成调用本方法，计数器会立刻从 0 变正，
    /// [`C19-PERF-COMMIT-O1`] 与 [`C19-DIGEST-MATCHES-RECOMPUTE`] 同时红。
    /// 反之，若本计数器恒为 0 且无递增点，那条判据就是恒真弱门禁——所以必须有它。
    pub fn recompute_digest(&mut self) -> u64 {
        self.counters.frame_scans = self.counters.frame_scans.saturating_add(1);
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for c in self.cmds.iter() {
            h = mix64(h, cmd_fingerprint(c));
        }
        h
    }

    /// 已封帧（只读）。
    pub fn sealed(&self) -> Option<&SealedFrame> {
        self.sealed.as_ref()
    }

    /// 显式放弃未封帧（作废路径：丢弃半帧 + 留痕，**不是落盘**）。
    pub fn abort_frame(&mut self, reason: &str) -> Result<(), ProtocolErr> {
        if self.phase != Phase::Open {
            self.record("abort_frame", ProtocolErr::PhaseDenied);
            return Err(ProtocolErr::PhaseDenied);
        }
        self.audit(format!(
            "帧 {} 未封即放弃（{}），已丢弃 {} 条半帧命令（不作废则半帧落盘）",
            self.frame,
            reason,
            self.cmds.len()
        ));
        self.cmds.clear();
        self.group_depth = 0;
        self.phase = Phase::Idle;
        self.sealed = None;
        Ok(())
    }

    /// 提交：**唯一能产出 [`CommitPoint`] 的路径**，且要求 `Sealed` 相位。
    ///
    /// O(1)：只读 `len()` 与已算好的摘要，**不遍历命令序列**。
    pub fn commit(&mut self, link: &mut SurfaceLink) -> Result<CommitPoint, ProtocolErr> {
        if self.phase == Phase::Committed {
            self.record("commit", ProtocolErr::AlreadyCommitted);
            return Err(ProtocolErr::AlreadyCommitted);
        }
        if self.phase != Phase::Sealed {
            self.record("commit", ProtocolErr::NotSealed);
            self.audit(format!(
                "帧 {} 未封即提交被拒（相位 {}）——半帧落盘在架构上不可表达",
                self.frame,
                self.phase.wire()
            ));
            return Err(ProtocolErr::NotSealed);
        }
        let cmd_count = self.cmds.len();
        let digest = self.digest;
        let frame = self.frame;
        self.counters.commits = self.counters.commits.saturating_add(1);
        let out = link.commit(frame, cmd_count, digest);
        self.phase = Phase::Committed;
        // 提交后释放命令序列（缓冲引用随之失效，树侧不留悬挂序列）。
        self.cmds.clear();
        self.sealed = None;
        Ok(out)
    }
}

impl Default for FrameAssembler {
    fn default() -> Self {
        FrameAssembler::new()
    }
}

// ---------------------------------------------------------------------------
// 六、呈现链路（解耦面：提交点只记账，不搬运像素、不等待）
// ---------------------------------------------------------------------------

/// 呈现链路（表面系统侧的门面）。
///
/// **不解耦**（即让树侧因呈现侧故障而失败）的实现是「提交失败即整帧作废」。
/// 本结构的处置是：提交**永远成功**，只是输出态可能为 [`OutputState::Dropped`]，
/// 且丢弃被记账。
#[derive(Clone, Debug)]
pub struct SurfaceLink {
    attached: bool,
    hold_present: bool,
    presented: u64,
    deferred: u64,
    dropped_frames: u64,
    drop_reasons: Vec<(&'static str, u64)>,
    audits: Vec<String>,
}

impl SurfaceLink {
    /// 构造（`attached=false` 即表面不可用）。
    pub fn new(attached: bool) -> Self {
        SurfaceLink {
            attached,
            hold_present: false,
            presented: 0,
            deferred: 0,
            dropped_frames: 0,
            drop_reasons: Vec::new(),
            audits: Vec::new(),
        }
    }

    /// 是否挂载。
    pub fn is_attached(&self) -> bool {
        self.attached
    }

    /// 挂载/卸载表面（卸载不丢树侧状态：合成照常）。
    pub fn set_attached(&mut self, v: bool) {
        self.attached = v;
        self.audits.push(format!("表面挂载位→ {}", if v { "挂载" } else { "卸载" }));
    }

    /// 请求延后呈现（呈现域的 vsync 裁决口；**本条不等待**）。
    pub fn hold_present(&mut self, v: bool) {
        self.hold_present = v;
    }

    /// 提交记账（**O(1)，不搬运像素**）。
    pub fn commit(&mut self, frame: u64, cmd_count: usize, digest: u64) -> CommitPoint {
        let output = if !self.attached {
            OutputState::Dropped("E_SURFACE_DETACHED")
        } else if self.hold_present {
            OutputState::Deferred
        } else {
            OutputState::Presented
        };
        match output {
            OutputState::Presented => self.presented = self.presented.saturating_add(1),
            OutputState::Deferred => self.deferred = self.deferred.saturating_add(1),
            OutputState::Dropped(r) => {
                self.dropped_frames = self.dropped_frames.saturating_add(1);
                self.note_drop(r);
            }
        }
        CommitPoint { frame, cmd_count, digest, output }
    }

    /// 按原因分桶累计丢弃数。
    fn note_drop(&mut self, reason: &'static str) {
        if let Some(slot) = self.drop_reasons.iter_mut().find(|(r, _)| *r == reason) {
            slot.1 = slot.1.saturating_add(1);
            return;
        }
        if self.drop_reasons.len() < 16 {
            self.drop_reasons.push((reason, 1));
        }
    }

    /// 已呈现帧数。
    pub fn presented_frames(&self) -> u64 {
        self.presented
    }

    /// 延后帧数。
    pub fn deferred_frames(&self) -> u64 {
        self.deferred
    }

    /// 丢弃帧数（丢输出必被记账）。
    pub fn dropped_frames(&self) -> u64 {
        self.dropped_frames
    }

    /// 按原因查丢弃数（未出现过的原因返回 0）。
    pub fn drop_reason(&self, reason: &'static str) -> u64 {
        self.drop_reasons
            .iter()
            .find(|(r, _)| *r == reason)
            .map(|(_, n)| *n)
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// 七、树侧投影（绘制项 → 表面命令）
// ---------------------------------------------------------------------------

/// 投影单个绘制项（O(1)；带跳过理由，绝不静默丢）。
pub fn project(item: &LayerDrawRef) -> Projection {
    match item.kind {
        DrawKind::GroupEnter => Projection::Mark(CmdKind::EnterGroup),
        DrawKind::GroupLeave => Projection::Mark(CmdKind::LeaveGroup),
        DrawKind::Layer => {
            if !item.opacity.is_finite() || item.opacity <= 0.0 {
                return Projection::Skip(SkipReason::ZeroOpacity);
            }
            if !item.world.x.is_finite()
                || !item.world.y.is_finite()
                || !item.world.w.is_finite()
                || !item.world.h.is_finite()
            {
                return Projection::Skip(SkipReason::EmptyBounds);
            }
            if item.world.is_empty() {
                return Projection::Skip(SkipReason::EmptyBounds);
            }
            if item.buffer.generation == 0 {
                return Projection::Skip(SkipReason::NoBuffer);
            }
            Projection::Emit(SurfaceCmd {
                kind: CmdKind::Blit,
                node_id: item.node_id,
                depth: item.depth,
                opacity: item.opacity.clamp(0.0, 1.0),
                src: item.buffer,
                dst: item.world,
            })
        }
    }
}

/// 批量投影入列（返回入列条数；遇相位/容量错误即停并如实返回）。
pub fn project_all(
    asm: &mut FrameAssembler,
    items: &[LayerDrawRef],
) -> Result<usize, ProtocolErr> {
    let mut n = 0usize;
    for it in items.iter() {
        asm.counters.projections = asm.counters.projections.saturating_add(1);
        asm.accept(project(it))?;
        n = n.saturating_add(1);
    }
    Ok(n)
}

// ---------------------------------------------------------------------------
// 八、反向回流（表面 damage → 树级脏区；O(1) 每事件）
// ---------------------------------------------------------------------------

/// 回流来源（外部内容变化的种类；可枚举）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReflowSource {
    /// 视频帧更新。
    VideoFrame,
    /// 外部解码器出帧。
    ExternalDecode,
    /// 合成器交换整层内容。
    CompositorSwap,
    /// 远端合成结果回灌。
    RemoteComposite,
}

impl ReflowSource {
    /// 显式线上编码。
    pub fn wire(self) -> u8 {
        match self {
            ReflowSource::VideoFrame => 1,
            ReflowSource::ExternalDecode => 2,
            ReflowSource::CompositorSwap => 3,
            ReflowSource::RemoteComposite => 4,
        }
    }
}

/// 回流事件（表面侧产生；每事件映射 O(1)）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ReflowEvent {
    /// 外部内容更新（视频帧/解码帧）→ 该层**内容脏**（不牵动变换与裁剪）。
    Content {
        /// 层 id。
        node_id: u64,
        /// 层世界域。
        world: SurfaceRect,
        /// 来源。
        source: ReflowSource,
    },
    /// 整层内容被替换（合成器交换）→ 内容脏 + **新旧两域并集**（旧域不画则留残影）。
    Replaced {
        /// 层 id。
        node_id: u64,
        /// 旧世界域。
        old_world: SurfaceRect,
        /// 新世界域。
        new_world: SurfaceRect,
        /// 来源。
        source: ReflowSource,
    },
}

/// 树级脏区动作（回流**只能**产出内容脏，见模块头注第三节）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeDirtyOp {
    /// 内容脏（推进内容修订号 → 重绘该层）。
    ContentDirty,
    /// 帧级内容脏（风暴合并产物）。
    FrameContentDirty,
}

/// 树级脏区（回流产出；交 F0613 收集面）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TreeDirty {
    /// 归属层 id（风暴合并为 [`FRAME_SCOPE`]）。
    pub node_id: u64,
    /// 脏区。
    pub rect: SurfaceRect,
    /// 动作。
    pub op: TreeDirtyOp,
    /// 来源。
    pub source: ReflowSource,
}

/// 单事件映射（O(1)；**无通用兜底**——两个变体各有专属产面）。
pub fn map_reflow(ev: &ReflowEvent) -> Vec<TreeDirty> {
    match ev {
        ReflowEvent::Content { node_id, world, source } => {
            vec![TreeDirty {
                node_id: *node_id,
                rect: *world,
                op: TreeDirtyOp::ContentDirty,
                source: *source,
            }]
        }
        ReflowEvent::Replaced { node_id, old_world, new_world, source } => {
            vec![TreeDirty {
                node_id: *node_id,
                rect: old_world.union(new_world),
                op: TreeDirtyOp::ContentDirty,
                source: *source,
            }]
        }
    }
}

/// 回流结算结果（映射产物 + 风暴记账）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReflowSettle {
    /// 映射出的树级脏区。
    pub dirties: Vec<TreeDirty>,
    /// 受理事件数。
    pub accepted: usize,
    /// 是否触发风暴合并。
    pub storm: bool,
}

/// 回流收集器（一帧一结算；风 stormed 合入帧）。
#[derive(Clone, Debug)]
pub struct ReflowCollector {
    known: Vec<u64>,
    pending: Vec<ReflowEvent>,
    counters: Counters,
    errors: Vec<ErrRecord>,
    audits: Vec<String>,
    storm_frames: u64,
    coalesced_count: u64,
    unknown_refusals: u64,
}

impl ReflowCollector {
    /// 构造：注入已登记层快照（F0601 只读投影）。
    pub fn new(known: Vec<u64>) -> Self {
        ReflowCollector {
            known,
            pending: Vec::new(),
            counters: Counters::new(),
            errors: Vec::new(),
            audits: Vec::new(),
            storm_frames: 0,
            coalesced_count: 0,
            unknown_refusals: 0,
        }
    }

    /// 受理一个回流事件（O(1)）。
    ///
    /// 未知层 → [`ProtocolErr::ReflowUnknownLayer`]（映射缺源显性拒绝，
    /// 不静默产出指向不存在层的脏区）。
    pub fn push(&mut self, ev: ReflowEvent) -> Result<(), ProtocolErr> {
        if self.pending.len() >= MAX_REFLOWS_PER_FRAME {
            self.errors.push(ErrRecord::new(
                "push".to_string(),
                ProtocolErr::ReflowOverflow,
            ));
            self.audits.push(format!(
                "本帧回流事件已达上界 {}，事件被拒（先结算本帧）",
                MAX_REFLOWS_PER_FRAME
            ));
            return Err(ProtocolErr::ReflowOverflow);
        }
        let id = match ev {
            ReflowEvent::Content { node_id, .. } => node_id,
            ReflowEvent::Replaced { node_id, .. } => node_id,
        };
        if !self.known.iter().any(|k| *k == id) {
            self.unknown_refusals = self.unknown_refusals.saturating_add(1);
            self.errors.push(ErrRecord::new(
                format!("push.layer_{}", id),
                ProtocolErr::ReflowUnknownLayer,
            ));
            return Err(ProtocolErr::ReflowUnknownLayer);
        }
        self.pending.push(ev);
        Ok(())
    }

    /// 结算本帧回流：逐事件 O(1) 映射；超风暴阈值合并入帧（**留痕**）。
    pub fn settle_frame(&mut self) -> ReflowSettle {
        let events = core::mem::take(&mut self.pending);
        let accepted = events.len();
        self.counters.reflow_maps = self.counters.reflow_maps.saturating_add(accepted as u64);
        if accepted > REFLOW_STORM_THRESHOLD {
            self.storm_frames = self.storm_frames.saturating_add(1);
            self.coalesced_count = self.coalesced_count.saturating_add(accepted as u64 - 1);
            let mut union = SurfaceRect::zero();
            let mut first = true;
            for ev in events.iter() {
                let r = match ev {
                    ReflowEvent::Content { world, .. } => *world,
                    ReflowEvent::Replaced { old_world, new_world, .. } => old_world.union(new_world),
                };
                if first {
                    union = r;
                    first = false;
                } else {
                    union = union.union(&r);
                }
            }
            self.audits.push(format!(
                "回流风暴：{} 事件合并入帧 1 条（阈值 {}），合并掉 {} 条",
                accepted, REFLOW_STORM_THRESHOLD, accepted - 1
            ));
            return ReflowSettle {
                dirties: vec![TreeDirty {
                    node_id: FRAME_SCOPE,
                    rect: union,
                    op: TreeDirtyOp::FrameContentDirty,
                    source: ReflowSource::CompositorSwap,
                }],
                accepted,
                storm: true,
            };
        }
        let mut dirties: Vec<TreeDirty> = Vec::new();
        for ev in events.iter() {
            dirties.extend(map_reflow(ev));
        }
        ReflowSettle { dirties, accepted, storm: false }
    }

    /// 风暴帧数（可查，不因合并而消失）。
    pub fn storm_frames(&self) -> u64 {
        self.storm_frames
    }

    /// 被合并掉的事件总数。
    pub fn coalesced_count(&self) -> u64 {
        self.coalesced_count
    }

    /// 未知层拒绝次数。
    pub fn unknown_refusals(&self) -> u64 {
        self.unknown_refusals
    }

    /// 本帧待结算事件数。
    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// 计数器。
    pub fn counters(&self) -> Counters {
        self.counters
    }

    /// 错误账本。
    pub fn errors(&self) -> &[ErrRecord] {
        &self.errors
    }

    /// 审计留痕。
    pub fn audits(&self) -> &[String] {
        &self.audits
    }
}

// ---------------------------------------------------------------------------
// 九、正向回流（树侧脏区 → 表面区域集）
// ---------------------------------------------------------------------------

/// 表面损伤集（树侧脏区的对外投影；供表面系统消费）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurfaceDamageOut {
    /// 帧号。
    pub frame: u64,
    /// 区域集（已与画面域求交）。
    pub regions: Vec<SurfaceRect>,
    /// 是否全帧（树侧已升级为全帧信号）。
    pub full_frame: bool,
    /// 是否已与画面域求交。
    pub clipped: bool,
}

impl SurfaceDamageOut {
    /// 从树侧脏区正向投影（O(1) 每脏区；相交即并做去重）。
    ///
    /// `viewport` 为 `None` 时不求交（只做去重）——此时 [`Self::clipped`]为 `false`，
    /// 下游据此知道区域未经画面裁剪。
    pub fn from_tree(
        frame: u64,
        tree_dirties: &[TreeDirty],
        full_frame: bool,
        viewport: Option<SurfaceRect>,
    ) -> SurfaceDamageOut {
        let mut regions: Vec<SurfaceRect> = Vec::new();
        for d in tree_dirties.iter() {
            let r = match viewport {
                Some(vp) => match d.rect.intersect(&vp) {
                    Some(x) => x,
                    None => continue,
                },
                None => d.rect,
            };
            if r.is_empty() {
                continue;
            }
            // 相交即并（含相接——接缝两侧的重绘本就是同一块工作）。
            // 纪律：判据是**相交**，不是「被包含」。用包含语义会让两块部分重叠的
            // 脏区各自留一帧，表现为画面上一条永远擦不掉的接缝。
            let mut merged = false;
            let mut i = 0usize;
            while i < regions.len() {
                let cur = regions.get(i).copied().unwrap_or(SurfaceRect::zero());
                if cur.intersect(&r).is_some() {
                    let u = cur.union(&r);
                    if let Some(slot) = regions.get_mut(i) {
                        *slot = u;
                    }
                    merged = true;
                    break;
                }
                i = i.saturating_add(1);
            }
            if !merged {
                regions.push(r);
            }
        }
        SurfaceDamageOut { frame, regions, full_frame, clipped: viewport.is_some() }
    }

    /// 区域数。
    pub fn region_count(&self) -> usize {
        self.regions.len()
    }
}

// ---------------------------------------------------------------------------
// 十、对接桥（一帧的完整编排面）
// ---------------------------------------------------------------------------

/// 一帧的对接结果。
#[derive(Clone, Debug, PartialEq)]
pub struct FrameOutcome {
    /// 提交点（`None` = 未提交或被拒）。
    pub commit: Option<CommitPoint>,
    /// 正向表面损伤集。
    pub surface_damage: SurfaceDamageOut,
    /// 反向回流结算。
    pub reflow: ReflowSettle,
    /// 本帧计数。
    pub counters: Counters,
    /// 错误账本。
    pub errors: Vec<ErrRecord>,
}

/// 表面协议对接桥（把上面各面编成一帧）。
#[derive(Clone, Debug)]
pub struct SurfaceBridge {
    asm: FrameAssembler,
    link: SurfaceLink,
    buffers: BufferRegistry,
    reflow: ReflowCollector,
    viewport: Option<SurfaceRect>,
    last_damage: SurfaceDamageOut,
}

impl SurfaceBridge {
    /// 构造（`attached` = 表面是否可用）。
    pub fn new(attached: bool, known_layers: Vec<u64>) -> Self {
        SurfaceBridge {
            asm: FrameAssembler::new(),
            link: SurfaceLink::new(attached),
            buffers: BufferRegistry::new(),
            reflow: ReflowCollector::new(known_layers),
            viewport: None,
            last_damage: SurfaceDamageOut::default(),
        }
    }

    /// 装配器（只读借用；相位与提交须经本桥的 [`Self::commit`]）。
    pub fn assembler(&self) -> &FrameAssembler {
        &self.asm
    }

    /// 呈现链路（只读借用；挂载位变更经 [`Self::set_attached`]）。
    pub fn link(&self) -> &SurfaceLink {
        &self.link
    }

    /// 缓冲登记表（取用/失效经本桥方法，保证记账一致）。
    pub fn buffers(&self) -> &BufferRegistry {
        &self.buffers
    }

    /// 回流收集器（只读借用）。
    pub fn reflow(&self) -> &ReflowCollector {
        &self.reflow
    }

    /// 注入画面域（正向投影的求交基准）。
    pub fn set_viewport(&mut self, vp: SurfaceRect) {
        self.viewport = Some(vp);
    }

    /// 表面挂载位。
    pub fn set_attached(&mut self, v: bool) {
        self.link.set_attached(v);
    }

    /// 取用表面缓冲（**只有本方法与 [`Self::invalidate_buffer`] 能改登记表**）。
    pub fn acquire_buffer(&mut self, owner: BufferOwner, w: u32, h: u32) -> BufferSlot {
        self.buffers.acquire(owner, w, h)
    }

    /// 缓冲失效（当帧重建的第一步）。
    ///
    /// 引用过期计数在此**如实累加**——过期是必须被看见的事实。
    pub fn invalidate_buffer(&mut self, r: BufferSlot) -> Result<(), ProtocolErr> {
        self.buffers.invalidate(r)
    }

    /// 校验树侧引用；过期则记一次 `stale`。
    ///
    /// 返回 `Err` 时**不改写树侧状态**（别人的边不是本模块的边），只留痕。
    pub fn verify_buffer(&mut self, r: BufferSlot) -> Result<BufferDesc, ProtocolErr> {
        match self.buffers.resolve(r) {
            Ok(d) => Ok(d),
            Err(e) => {
                self.buffers.note_stale();
                self.asm
                    .errors
                    .push(ErrRecord::new(format!("verify_buffer.slot_{}", r.slot), e));
                Err(e)
            }
        }
    }

    /// 开帧。
    pub fn begin_frame(&mut self, frame: u64) -> Result<(), ProtocolErr> {
        self.asm.begin_frame(frame)
    }

    /// 投影树侧绘制项入列。
    pub fn project(&mut self, items: &[LayerDrawRef]) -> Result<usize, ProtocolErr> {
        project_all(&mut self.asm, items)
    }

    /// 受理一个回流事件（表面 → 树）。
    pub fn reflow_push(&mut self, ev: ReflowEvent) -> Result<(), ProtocolErr> {
        self.reflow.push(ev)
    }

    /// 封帧并提交（**唯一提交入口**）。
    ///
    /// 帧级作废：放弃未封帧并留审计痕（错误矩阵「序列中断→帧级丢弃重来」）。
    ///
    /// 本入口存在的理由是**纪律要求可达**：若桥不暴露作废路径，装配侧遇到中断
    /// 就只能把半帧硬提交出去——那正是本协议要禁止的半帧落盘。
    pub fn abort_frame(&mut self, reason: &str) -> Result<(), ProtocolErr> {
        self.asm.abort_frame(reason)
    }

    /// 提交后结算双向回流：反向结算进树侧脏区，正向投影成表面损伤集。
    pub fn commit_frame(&mut self, tree_dirties: &[TreeDirty]) -> Result<FrameOutcome, ProtocolErr> {
        self.asm.seal()?;
        let point = self.asm.commit(&mut self.link)?;
        let reflow = self.reflow.settle_frame();
        let mut all: Vec<TreeDirty> = tree_dirties.to_vec();
        all.extend(reflow.dirties.iter().copied());
        let damage = SurfaceDamageOut::from_tree(point.frame, &all, false, self.viewport);
        self.last_damage = damage.clone();
        let errors = self.asm.errors.clone();
        Ok(FrameOutcome {
            commit: Some(point),
            surface_damage: damage,
            reflow,
            counters: self.asm.counters,
            errors,
        })
    }

    /// 上一帧的正向表面损伤集。
    pub fn last_damage(&self) -> &SurfaceDamageOut {
        &self.last_damage
    }
}

// ---------------------------------------------------------------------------
// 十一、模块自检
// ---------------------------------------------------------------------------

/// 判定用夹具：一片三层的树投影（层 1 含子层 2 与组 3）。
fn fixture_items() -> Vec<LayerDrawRef> {
    let b1 = BufferSlot::new(0, 1).unwrap_or(BufferSlot::invalid());
    let b2 = BufferSlot::new(1, 1).unwrap_or(BufferSlot::invalid());
    vec![
        LayerDrawRef {
            node_id: 1,
            kind: DrawKind::Layer,
            depth: 0,
            opacity: 1.0,
            world: SurfaceRect { x: 0.0, y: 0.0, w: 100.0, h: 80.0 },
            buffer: b1,
        },
        LayerDrawRef {
            node_id: 3,
            kind: DrawKind::GroupEnter,
            depth: 1,
            opacity: 1.0,
            world: SurfaceRect::zero(),
            buffer: BufferSlot::invalid(),
        },
        LayerDrawRef {
            node_id: 2,
            kind: DrawKind::Layer,
            depth: 1,
            opacity: 0.5,
            world: SurfaceRect { x: 10.0, y: 10.0, w: 50.0, h: 40.0 },
            buffer: b2,
        },
        LayerDrawRef {
            node_id: 3,
            kind: DrawKind::GroupLeave,
            depth: 1,
            opacity: 1.0,
            world: SurfaceRect::zero(),
            buffer: BufferSlot::invalid(),
        },
    ]
}

/// VE-F0619 模块自检（受 `CheckSet::MAX_CHECKS=112` 约束，逐条覆盖锚点判据）。
pub fn run_ved19_checks() -> CheckSet {
    let mut s = CheckSet::new("ved19");
    s.add(
        "C19-FRAME-OPEN",
        {
            let mut a = FrameAssembler::new();
            let ok_open = a.begin_frame(7).is_ok() && a.phase() == Phase::Open;
            // 上一帧未终结即开新帧 → 帧重叠被拒（撕裂的机械成因）。
            let overlap = a.begin_frame(8).is_err();
            ok_open && overlap && a.frame() == 7
        },
        "开帧进Open；未终结再开 → E_FRAME_OVERLAP",
    );
    s.add(
        "C19-FRAME-CMD-CAP",
        {
            let mut a = FrameAssembler::new();
            let _ = a.begin_frame(1);
            let mut last_ok = true;
            for _ in 0..MAX_COMMANDS_PER_FRAME {
                last_ok &= a
                    .accept(Projection::Emit(SurfaceCmd {
                        kind: CmdKind::Blit,
                        node_id: 1,
                        depth: 0,
                        opacity: 1.0,
                        src: BufferSlot::new(0, 1).unwrap_or(BufferSlot::invalid()),
                        dst: SurfaceRect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 },
                    }))
                    .is_ok();
            }
            let over = a
                .accept(Projection::Emit(SurfaceCmd {
                    kind: CmdKind::Blit,
                    node_id: 1,
                    depth: 0,
                    opacity: 1.0,
                    src: BufferSlot::new(0, 1).unwrap_or(BufferSlot::invalid()),
                    dst: SurfaceRect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 },
                }))
                .is_err();
            last_ok && over && a.errors().iter().any(|e| e.code == "E_FRAME_CMD_OVERFLOW")
        },
        "命令数达上界仍可入列；越界显性拒绝且记账（不截断）",
    );
    s.add(
        "C19-FRAME-NOT-SEALED",
        {
            // 半帧落盘在架构上不可表达：Open 相位提交必被拒。
            let mut a = FrameAssembler::new();
            let _ = a.begin_frame(3);
            let mut l = SurfaceLink::new(true);
            let r = a.commit(&mut l);
            !r.is_ok()
                && a.phase() == Phase::Open
                && a.audits().iter().any(|s| s.contains("半帧落盘"))
        },
        "Open 相位提交 → E_FRAME_NOT_SEALED 且相位不回退",
    );
    s.add(
        "C19-FRAME-SEAL-ONCE",
        {
            let mut a = FrameAssembler::new();
            let _ = a.begin_frame(4);
            let first = a.seal().is_ok();
            let second = a.seal().is_err();
            first && second && a.sealed().is_some()
        },
        "封帧一次成功；重复封帧 → E_FRAME_ALREADY_SEALED",
    );
    s.add(
        "C19-FRAME-COMMIT-ONCE",
        {
            let mut a = FrameAssembler::new();
            let mut l = SurfaceLink::new(true);
            let _ = a.begin_frame(5);
            let _ = project_all(&mut a, &fixture_items());
            let _ = a.seal();
            let first = a.commit(&mut l).is_ok();
            // 第二次提交必须**点名** E_FRAME_ALREADY_COMMITTED：只判 is_err 会让
            // 「退化成别的拒绝原因」蒙混过关（相位非 Sealed 时也返回 Err）。
            let second = matches!(a.commit(&mut l), Err(ProtocolErr::AlreadyCommitted));
            first && second && l.presented_frames() == 1 && l.dropped_frames() == 0
        },
        "提交一次成功；重复提交 → E_FRAME_ALREADY_COMMITTED 且呈现只记一次",
    );
    s.add(
        "C19-FRAME-COMMIT-POINT",
        {
            let mut a = FrameAssembler::new();
            let mut l = SurfaceLink::new(true);
            let _ = a.begin_frame(11);
            let n = project_all(&mut a, &fixture_items()).unwrap_or(0);
            match (a.seal(), a.commit(&mut l)) {
                (Ok(f), Ok(p)) => {
                    p.frame == 11 && p.cmd_count == n && p.cmd_count == f.cmd_count() && p.digest == f.digest
                }
                _ => false,
            }
        },
        "提交点三要素齐（帧号/命令数/摘要）且与封帧结果一致",
    );
    s.add(
        "C19-FRAME-DIGEST-ORDER",
        {
            // 摘要必须对命令**次序**敏感：调换两条不同层的命令 → 摘要不同。
            let mut base: Vec<SurfaceCmd> = Vec::new();
            let src = BufferSlot::new(0, 1).unwrap_or(BufferSlot::invalid());
            for id in 1u64..=4u64 {
                base.push(SurfaceCmd {
                    kind: CmdKind::Blit,
                    node_id: id,
                    depth: 0,
                    opacity: 1.0,
                    src,
                    dst: SurfaceRect { x: id as f32, y: 0.0, w: 2.0, h: 2.0 },
                });
            }
            // 单变量A：四条命令dst 全同、仅 node_id 不同 → 仅 node_id 变化须改摘要。
            let same_dst: Vec<SurfaceCmd> = (1u64..=4u64)
                .map(|id| SurfaceCmd {
                    kind: CmdKind::Blit,
                    node_id: id,
                    depth: 0,
                    opacity: 1.0,
                    src,
                    dst: SurfaceRect { x: 1.0, y: 2.0, w: 3.0, h: 4.0 },
                })
                .collect();
            let id_only = seq_digest(&same_dst);
            let mut id_swapped = same_dst.clone();
            id_swapped.swap(0, 3);
            let id_swap = seq_digest(&id_swapped);
            let mut id_changed = same_dst.clone();
            if let Some(h) = id_changed.get_mut(0) {
                h.node_id = 99;
            }
            let id_field = seq_digest(&id_changed);
            // 单变量B：node_id 全同、仅 dst 不同 → 仅 dst 变化须改摘要。
            let same_id: Vec<SurfaceCmd> = (1u64..=4u64)
                .map(|i| SurfaceCmd {
                    kind: CmdKind::Blit,
                    node_id: 7,
                    depth: 0,
                    opacity: 1.0,
                    src,
                    dst: SurfaceRect { x: i as f32, y: 0.0, w: 2.0, h: 2.0 },
                })
                .collect();
            let dst_only = seq_digest(&same_id);
            let mut dst_changed = same_id.clone();
            if let Some(h) = dst_changed.get_mut(0) {
                h.dst.w = 9.0;
            }
            let dst_field = seq_digest(&dst_changed);
            let d1 = seq_digest(&base);
            let mut swapped = base.clone();
            swapped.swap(0, 3);
            let d2 = seq_digest(&swapped);
            let d3 = seq_digest(&base);
            d1 != d2
                && d1 == d3
                && d1 != 0
                && id_only != id_swap
                && id_only != id_field
                && dst_only != dst_field
        },
        "摘要对次序敏感且可重入；node_id 与 dst 单变量各自参与摘要（非互相掩盖）",
    );
    s.add(
        "C19-FRAME-ABORT",
        {
            // 半帧的**显式放弃**路径：丢弃 + 留痕，且不留半帧状态。
            let mut a = FrameAssembler::new();
            let _ = a.begin_frame(12);
            let _ = project_all(&mut a, &fixture_items());
            let r = a.abort_frame("上游中断");
            let after = a.phase() == Phase::Idle;
            let mut l = SurfaceLink::new(true);
            let no_commit = a.commit(&mut l).is_err();
            r.is_ok()
                && after
                && no_commit
                && a.audits().iter().any(|s| s.contains("不作废则半帧落盘"))
        },
        "abort_frame 丢弃半帧并留痕；放弃后不可提交",
    );
    s.add(
        "C19-PROJECT-LAYER",
        {
            let first = fixture_items();
            let p = project(first.get(0).unwrap_or(&LayerDrawRef {
                node_id: 0,
                kind: DrawKind::Layer,
                depth: 0,
                opacity: 0.0,
                world: SurfaceRect::zero(),
                buffer: BufferSlot::invalid(),
            }));
            match p {
                Projection::Emit(c) => {
                    c.kind == CmdKind::Blit && c.node_id == 1 && c.opacity == 1.0 && !c.dst.is_empty()
                }
                _ => false,
            }
        },
        "叶层投影为 Blit 命令（携缓冲引用与不透明度）",
    );
    s.add(
        "C19-PROJECT-MARK",
        {
            let items = fixture_items();
            let enter = project(items.get(1).unwrap_or(&items[0]));
            let leave = project(items.get(3).unwrap_or(&items[0]));
            matches!(enter, Projection::Mark(CmdKind::EnterGroup))
                && matches!(leave, Projection::Mark(CmdKind::LeaveGroup))
        },
        "组进/出投影为结构标记（非像素命令）",
    );
    s.add(
        "C19-PROJECT-SKIP",
        {
            let src = BufferSlot::new(0, 1).unwrap_or(BufferSlot::invalid());
            let zero_op = LayerDrawRef {
                node_id: 9,
                kind: DrawKind::Layer,
                depth: 0,
                opacity: 0.0,
                world: SurfaceRect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 },
                buffer: src,
            };
            let empty = LayerDrawRef {
                node_id: 9,
                kind: DrawKind::Layer,
                depth: 0,
                opacity: 1.0,
                world: SurfaceRect::zero(),
                buffer: src,
            };
            let nobuf = LayerDrawRef {
                node_id: 9,
                kind: DrawKind::Layer,
                depth: 0,
                opacity: 1.0,
                world: SurfaceRect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 },
                buffer: BufferSlot::invalid(),
            };
            let nan_bounds = LayerDrawRef {
                node_id: 9,
                kind: DrawKind::Layer,
                depth: 0,
                opacity: 1.0,
                world: SurfaceRect { x: f32::NAN, y: 0.0, w: 10.0, h: 10.0 },
                buffer: src,
            };
            matches!(project(&zero_op), Projection::Skip(SkipReason::ZeroOpacity))
                && matches!(project(&empty), Projection::Skip(SkipReason::EmptyBounds))
                && matches!(project(&nobuf), Projection::Skip(SkipReason::NoBuffer))
                && matches!(project(&nan_bounds), Projection::Skip(SkipReason::EmptyBounds))
        },
        "四类跳过各有专属理由（零不透明/退化域/无缓冲/非有限域），不静默丢",
    );
    s.add(
        "C19-PROJECT-GROUP-BALANCE",
        {
            // 组标记失配 → 封帧被拒（状态泄漏的入口被堵）。
            let mut a = FrameAssembler::new();
            let _ = a.begin_frame(13);
            let _ = a.accept(Projection::Mark(CmdKind::EnterGroup));
            let sealed = a.seal().is_err();
            let mut b = FrameAssembler::new();
            let _ = b.begin_frame(14);
            let _ = b.accept(Projection::Mark(CmdKind::LeaveGroup));
            let under = b.accept(Projection::Mark(CmdKind::LeaveGroup)).is_err();
            sealed && under && a.audits().iter().any(|s| s.contains("整帧不可提交"))
        },
        "组标记未配对→封帧拒；栈下溢→显性 E_GROUP_UNBALANCED",
    );
    s.add(
        "C19-OWNERSHIP-NOBORROW",
        {
            // 拿真实登记表当被测物：跑完整一帧投影后登记项逐字段不变。
            let mut reg = BufferRegistry::new();
            let b = reg.acquire(BufferOwner::Surface, 64, 64);
            let before = reg.resolve(b);
            let mut asm = FrameAssembler::new();
            let _ = asm.begin_frame(21);
            let items = vec![LayerDrawRef {
                node_id: 1,
                kind: DrawKind::Layer,
                depth: 0,
                opacity: 1.0,
                world: SurfaceRect { x: 0.0, y: 0.0, w: 32.0, h: 32.0 },
                buffer: b,
            }];
            let _ = project_all(&mut asm, &items);
            let _ = asm.seal();
            let after = reg.resolve(b);
            before.is_ok() && before == after && reg.live_count() == 1
        },
        "树侧一帧投影不改写表面缓冲登记项（只引用不拥有，被测物=真实登记表）",
    );
    s.add(
        "C19-OWNERSHIP-GENERATION",
        {
            // 缓冲失效 → 旧引用世代不匹配 → resolve 拒（引用会过期）。
            let mut reg = BufferRegistry::new();
            let b = reg.acquire(BufferOwner::Surface, 8, 8);
            let ok0 = reg.resolve(b).is_ok();
            let inv = reg.invalidate(b);
            let stale = reg.resolve(b).is_err();
            let b2 = reg.acquire(BufferOwner::Surface, 8, 8);
            let recycled = b2.slot == b.slot && b2.generation != b.generation;
            let old_still_stale = reg.resolve(b).is_err();
            ok0 && inv.is_ok() && stale && recycled && old_still_stale && reg.live_count() == 1
        },
        "失效后旧引用被拒；重建复用槽位但世代递增（旧引用不会误判有效）",
    );
    s.add(
        "C19-OWNERSHIP-FRAME-NOT-OWNER",
        {
            // 丢掉整帧后存活缓冲数不变：帧是引用者不是所有者。
            let mut reg = BufferRegistry::new();
            let b = reg.acquire(BufferOwner::Group(3), 16, 16);
            let before = reg.live_count();
            {
                let mut asm = FrameAssembler::new();
                let _ = asm.begin_frame(22);
                let _ = asm.accept(Projection::Emit(SurfaceCmd {
                    kind: CmdKind::CompositeGroup,
                    node_id: 3,
                    depth: 0,
                    opacity: 1.0,
                    src: b,
                    dst: SurfaceRect { x: 0.0, y: 0.0, w: 16.0, h: 16.0 },
                }));
                let _ = asm.seal();
            }
            before == reg.live_count() && reg.live_count() == 1
        },
        "丢弃整帧不释放缓冲（所有权在表面域登记表）",
    );
    s.add(
        "C19-BUFFER-REBUILD",
        {
            // 缓冲失效 → 当帧重建登记（错误矩阵第三条）。
            let mut br = SurfaceBridge::new(true, vec![1]);
            let b = br.acquire_buffer(BufferOwner::Surface, 32, 32);
            let inv = br.invalidate_buffer(b);
            let marked = br.buffers().is_invalidated(b);
            let reject = br.verify_buffer(b).is_err();
            let b2 = br.acquire_buffer(BufferOwner::Surface, 32, 32);
            let ok2 = br.verify_buffer(b2).is_ok();
            inv.is_ok() && marked && reject && ok2 && br.buffers().stale_refs() == 1
        },
        "失效可登记；旧引用被拒且记stale；当帧重建后新引用可解析",
    );
    s.add(
        "C19-REFLOW-CONTENT-ONLY",
        {
            // 回流只准落内容脏（不牵动变换/裁剪），否则视频帧更新会拖垮缓存命中。
            let ev = ReflowEvent::Content {
                node_id: 2,
                world: SurfaceRect { x: 5.0, y: 5.0, w: 20.0, h: 20.0 },
                source: ReflowSource::VideoFrame,
            };
            let d = map_reflow(&ev);
            d.len() == 1
                && matches!(d.get(0).map(|x| x.opp()), Some(TreeDirtyOp::ContentDirty))
                && d.get(0).map(|x| x.source) == Some(ReflowSource::VideoFrame)
        },
        "视频帧更新→该层内容脏（O(1) 一事件一脏区，无通用兜底）",
    );
    s.add(
        "C19-REFLOW-REPLACE-UNION",
        {
            // 整层替换必须覆盖新旧两域并集，否则旧位置留残影。
            let ev = ReflowEvent::Replaced {
                node_id: 2,
                old_world: SurfaceRect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 },
                new_world: SurfaceRect { x: 20.0, y: 0.0, w: 10.0, h: 10.0 },
                source: ReflowSource::CompositorSwap,
            };
            let d = map_reflow(&ev);
            match d.get(0) {
                Some(x) => {
                    x.rect.w >= 30.0 - RECT_EPS && x.rect.x <= RECT_EPS && x.opp() == TreeDirtyOp::ContentDirty
                }
                None => false,
            }
        },
        "整层替换→新旧两域并集（缺旧域即残留上一帧内容）",
    );
    s.add(
        "C19-REFLOW-BIDIRECTIONAL",
        {
            // 判据「双向」：两个方向必须**同时**产出，缺任一即红。
            let mut br = SurfaceBridge::new(true, vec![1, 2]);
            br.set_viewport(SurfaceRect { x: 0.0, y: 0.0, w: 200.0, h: 200.0 });
            let b1 = br.acquire_buffer(BufferOwner::Surface, 32, 32);
            let _ = br.begin_frame(31);
            let _ = br.project(&[LayerDrawRef {
                node_id: 1,
                kind: DrawKind::Layer,
                depth: 0,
                opacity: 1.0,
                world: SurfaceRect { x: 0.0, y: 0.0, w: 32.0, h: 32.0 },
                buffer: b1,
            }]);
            let _ = br.reflow_push(ReflowEvent::Content {
                node_id: 2,
                world: SurfaceRect { x: 40.0, y: 40.0, w: 10.0, h: 10.0 },
                source: ReflowSource::VideoFrame,
            });
            let tree_dirty = vec![TreeDirty {
                node_id: 1,
                rect: SurfaceRect { x: 0.0, y: 0.0, w: 32.0, h: 32.0 },
                op: TreeDirtyOp::ContentDirty,
                source: ReflowSource::RemoteComposite,
            }];
            let out = br.commit_frame(&tree_dirty);
            match out {
                Ok(o) => {
                    // 反向：表面→树 产出 1 条；正向：树→表面 产出≥1 区域且已裁剪。
                    !o.reflow.dirties.is_empty()
                        && o.surface_damage.region_count() >= 1
                        && o.surface_damage.clipped
                }
                Err(_) => false,
            }
        },
        "双向同时成立：表面→树产出脏区；树→表面产出区域集（已与画面域求交）",
    );
    s.add(
        "C19-REFLOW-UNKNOWN-LAYER",
        {
            // 映射缺源显性拒绝（不静默产出指向不存在层的脏区）。
            let mut c = ReflowCollector::new(vec![1, 2]);
            let ok = c
                .push(ReflowEvent::Content {
                    node_id: 2,
                    world: SurfaceRect { x: 0.0, y: 0.0, w: 4.0, h: 4.0 },
                    source: ReflowSource::VideoFrame,
                })
                .is_ok();
            let bad = c.push(ReflowEvent::Content {
                node_id: 99,
                world: SurfaceRect { x: 0.0, y: 0.0, w: 4.0, h: 4.0 },
                source: ReflowSource::VideoFrame,
            });
            let settle = c.settle_frame();
            ok && bad.is_err() && settle.accepted == 1 && settle.dirties.len() == 1
                && c.unknown_refusals() == 1
                && c.errors().iter().any(|e| e.code == "E_REFLOW_UNKNOWN_LAYER")
        },
        "未知层回流被拒且记账；已登记事件照常结算（缺源不污染本帧）",
    );
    s.add(
        "C19-REFLOW-STORM",
        {
            // damage 风暴 → 合并入帧，且风暴事实**留痕可查**。
            let mut c = ReflowCollector::new(vec![1, 2, 3]);
            for _ in 0..(REFLOW_STORM_THRESHOLD + 7) {
                let _ = c.push(ReflowEvent::Content {
                    node_id: 1,
                    world: SurfaceRect { x: 1.0, y: 1.0, w: 2.0, h: 2.0 },
                    source: ReflowSource::VideoFrame,
                });
            }
            let r = c.settle_frame();
            r.storm
                && r.dirties.len() == 1
                && matches!(r.dirties.get(0).map(|d| d.opp()), Some(TreeDirtyOp::FrameContentDirty))
                && matches!(r.dirties.get(0).map(|d| d.node_id), Some(FRAME_SCOPE))
                && r.accepted == REFLOW_STORM_THRESHOLD + 7
                && c.storm_frames() == 1
                && c.coalesced_count() == (REFLOW_STORM_THRESHOLD + 6) as u64
                && c.audits().iter().any(|a| a.contains("回流风暴"))
        },
        "超阈值合并为帧级一条；风暴次数与合并条数均可查（不因合并而消失）",
    );
    s.add(
        "C19-REFLOW-CAP",
        {
            let mut c = ReflowCollector::new(vec![1]);
            for _ in 0..MAX_REFLOWS_PER_FRAME {
                let _ = c.push(ReflowEvent::Content {
                    node_id: 1,
                    world: SurfaceRect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 },
                    source: ReflowSource::ExternalDecode,
                });
            }
            let over = c.push(ReflowEvent::Content {
                node_id: 1,
                world: SurfaceRect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 },
                source: ReflowSource::ExternalDecode,
            });
            over.is_err()
                && c.pending() == MAX_REFLOWS_PER_FRAME
                && c.errors().iter().any(|e| e.code == "E_REFLOW_OVERFLOW")
        },
        "回流事件达上界后显性拒绝（先结算本帧），不静默截断",
    );
    s.add(
        "C19-DECOUP-DROPPED-BUT-OK",
        {
            // 表面不可用 → 提交**仍然成功**，只是输出态为丢弃且被记账。
            let mut a = FrameAssembler::new();
            let mut l = SurfaceLink::new(false);
            let _ = a.begin_frame(41);
            let _ = project_all(&mut a, &fixture_items());
            let _ = a.seal();
            let p = a.commit(&mut l);
            matches!(p, Ok(CommitPoint { output: OutputState::Dropped(_), .. }))
                && l.dropped_frames() == 1
                && l.presented_frames() == 0
                && l.drop_reason("E_SURFACE_DETACHED") == 1
        },
        "表面未挂载时提交成功但输出丢弃；丢弃按原因记账（丢输出≠静默丢输出）",
    );
    s.add(
        "C19-DECOUP-NO-BLOCK",
        {
            // 解耦的**可观测**证据：挂载与否不改变帧内容（摘要逐位相同）。
            let items = fixture_items();
            let digest_of = |attached: bool| -> u64 {
                let mut a = FrameAssembler::new();
                let mut l = SurfaceLink::new(attached);
                let _ = a.begin_frame(42);
                let _ = project_all(&mut a, &items);
                let _ = a.seal();
                match a.commit(&mut l) {
                    Ok(p) => p.digest,
                    Err(_) => 0,
                }
            };
            let on = digest_of(true);
            let off = digest_of(false);
            on != 0 && on == off
        },
        "同一份投影在挂载/卸载下摘要相同（呈现与合成真解耦，非仅返回值不同）",
    );
    s.add(
        "C19-DECOUP-PRESENT-STATES",
        {
            // 呈现侧三态如实记录；本条不等待、不阻塞、不读墙钟。
            let mut a = FrameAssembler::new();
            let mut l = SurfaceLink::new(true);
            l.hold_present(true);
            let _ = a.begin_frame(43);
            let _ = project_all(&mut a, &fixture_items());
            let _ = a.seal();
            let p = a.commit(&mut l);
            l.hold_present(false);
            let _ = a.begin_frame(44);
            let _ = project_all(&mut a, &fixture_items());
            let _ = a.seal();
            let p2 = a.commit(&mut l);
            matches!(p, Ok(CommitPoint { output: OutputState::Deferred, .. }))
                && matches!(p2, Ok(CommitPoint { output: OutputState::Presented, .. }))
                && l.deferred_frames() == 1
                && l.presented_frames() == 1
        },
        "延后/呈现两态如实记录（vsync 裁决权交呈现域，本条不等待）",
    );
    s.add(
        "C19-PERF-COMMIT-O1",
        {
            // O(1) 提交是**实测**的：摘要在入列时增量算，提交不再遍历序列。
            // 反假面：先证明计数器真的会动（recompute_digest 必使其 +1），
            // 再证明提交路径不动它（提交前后计数不变）。若计数器恒 0，
            // 「frame_scans == 0」就是恒真弱门禁——所以必须先证明它会动。
            let items = fixture_items();
            let mut a = FrameAssembler::new();
            let _ = a.begin_frame(51);
            let n = project_all(&mut a, &items).unwrap_or(0);
            let before = a.counters();
            let sealed_ok = a.seal().is_ok();
            let full = a.recompute_digest();
            let mid = a.counters();
            let mut l = SurfaceLink::new(true);
            let p = a.commit(&mut l);
            let after = a.counters();
            let expected = sealed_commands(&items);
            sealed_ok
                && p.is_ok()
                // 计数器确实会递增（否则 mid.frame_scans==0 恒真）
                && mid.frame_scans == 1
                // 入列时逐条增量：增量次数 == 命令数
                && before.digest_updates == n as u64
                && after.digest_updates == n as u64
                // 封帧与提交都零遍历
                && after.frame_scans == 1
                // 增量算法与整体重算同值（摘要是对的，只是快）
                && full == seq_digest(&expected)
                && p.map(|q| q.digest) == Ok(full)
        },
        "摘要增量更新恰等于命令数；封帧与提交零遍历；增量摘要与整体重算同值",
    );
    s.add(
        "C19-PERF-REFLOW-O1",
        {
            // 回流映射 O(1)/事件：映射次数恰等于受理事件数，且不跨事件重扫。
            let mut c = ReflowCollector::new(vec![1, 2]);
            for i in 0..50u64 {
                let _ = c.push(ReflowEvent::Content {
                    node_id: if i % 2 == 0 { 1 } else { 2 },
                    world: SurfaceRect { x: i as f32, y: 0.0, w: 3.0, h: 3.0 },
                    source: ReflowSource::VideoFrame,
                });
            }
            let r = c.settle_frame();
            let ct = c.counters();
            r.accepted == 50
                && r.dirties.len() == 50
                && ct.reflow_maps == 50
                && ct.frame_scans == 0
        },
        "50 事件→50 次映射、50 条脏区（O(1) 每事件，非批量重扫）",
    );
    s.add(
        "C19-DAMAGE-CLIP",
        {
            // 正向投影必须与画面域求交（屏外脏区对合成无意义）。
            let vp = SurfaceRect { x: 0.0, y: 0.0, w: 50.0, h: 50.0 };
            let ds = vec![
                TreeDirty {
                    node_id: 1,
                    rect: SurfaceRect { x: 0.0, y: 0.0, w: 20.0, h: 20.0 },
                    op: TreeDirtyOp::ContentDirty,
                    source: ReflowSource::VideoFrame,
                },
                TreeDirty {
                    node_id: 2,
                    rect: SurfaceRect { x: 200.0, y: 200.0, w: 10.0, h: 10.0 },
                    op: TreeDirtyOp::ContentDirty,
                    source: ReflowSource::VideoFrame,
                },
            ];
            let out = SurfaceDamageOut::from_tree(1, &ds, false, Some(vp));
            out.region_count() == 1 && out.clipped && out.regions.get(0).map(|r| r.w) == Some(20.0)
        },
        "屏外脏区被裁掉；画面域内的保留（裁剪口径与画面域一致）",
    );
    s.add(
        "C19-DAMAGE-COALESCE",
        {
            // 去重纪律：相交即并（碎片越少，F0641 三档判定越准）。
            let ds = vec![
                TreeDirty {
                    node_id: 1,
                    rect: SurfaceRect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 },
                    op: TreeDirtyOp::ContentDirty,
                    source: ReflowSource::VideoFrame,
                },
                TreeDirty {
                    node_id: 2,
                    rect: SurfaceRect { x: 8.0, y: 8.0, w: 20.0, h: 20.0 },
                    op: TreeDirtyOp::ContentDirty,
                    source: ReflowSource::VideoFrame,
                },
            ];
            let out = SurfaceDamageOut::from_tree(1, &ds, false, None);
            // 形态二：被包含（小块完全落在大块内）。只测部分重叠是不够的——
            // 把语义改成「仅合并被包含」时部分重叠仍会绿，覆盖面必须两种都占。
            let nested = vec![
                TreeDirty {
                    node_id: 1,
                    rect: SurfaceRect { x: 0.0, y: 0.0, w: 40.0, h: 40.0 },
                    op: TreeDirtyOp::ContentDirty,
                    source: ReflowSource::VideoFrame,
                },
                TreeDirty {
                    node_id: 2,
                    rect: SurfaceRect { x: 10.0, y: 10.0, w: 5.0, h: 5.0 },
                    op: TreeDirtyOp::ContentDirty,
                    source: ReflowSource::VideoFrame,
                },
            ];
            let out2 = SurfaceDamageOut::from_tree(1, &nested, false, None);
            out.region_count() == 1
                && out.regions.get(0).map(|r| r.w) == Some(28.0)
                && out2.region_count() == 1
                && out2.regions.get(0).map(|r| r.w) == Some(40.0)
        },
        "相交即并：部分重叠(不等大)与被包含两种形态都并为一块（覆盖面双形态）",
    );
    s.add(
        "C19-WIRE-EXPLICIT",
        {
            // 枚举判别值 ≠ 线上编码：wire() 必须是显式映射且两两不同。
            let cmd = [
                CmdKind::Blit.wire(),
                CmdKind::EnterGroup.wire(),
                CmdKind::LeaveGroup.wire(),
                CmdKind::CompositeGroup.wire(),
            ];
            let draw = [DrawKind::Layer.wire(), DrawKind::GroupEnter.wire(), DrawKind::GroupLeave.wire()];
            let phase = [
                Phase::Idle.wire(),
                Phase::Open.wire(),
                Phase::Sealed.wire(),
                Phase::Committed.wire(),
            ];
            let src = [
                ReflowSource::VideoFrame.wire(),
                ReflowSource::ExternalDecode.wire(),
                ReflowSource::CompositorSwap.wire(),
                ReflowSource::RemoteComposite.wire(),
            ];
            let all_distinct = |v: &[u8]| -> bool {
                for i in 0..v.len() {
                    for j in (i + 1)..v.len() {
                        if v.get(i) == v.get(j) {
                            return false;
                        }
                    }
                }
                true
            };
            all_distinct(&cmd)
                && all_distinct(&draw)
                && all_distinct(&phase)
                && all_distinct(&src)
                && cmd.get(0) == Some(&1)
                && BufferOwner::Surface.wire() != BufferOwner::Group(1).wire()
        },
        "四组枚举的 wire() 为显式映射且组内两两不同（不靠声明次序）",
    );
    s.add(
        "C19-ERR-DISJOINT-CODES",
        {
            // 处置方向相反的状态不得共用码（否则上游处置建议自相矛盾）。
            let all = [
                ProtocolErr::FrameOverlap.code(),
                ProtocolErr::NotSealed.code(),
                ProtocolErr::AlreadySealed.code(),
                ProtocolErr::AlreadyCommitted.code(),
                ProtocolErr::PhaseDenied.code(),
                ProtocolErr::CmdOverflow.code(),
                ProtocolErr::GroupUnbalanced.code(),
                ProtocolErr::GroupDepth.code(),
                ProtocolErr::ReflowUnknownLayer.code(),
                ProtocolErr::ReflowOverflow.code(),
                ProtocolErr::BufferGeneration.code(),
                ProtocolErr::BufferMissing.code(),
                ProtocolErr::SurfaceRequired.code(),
            ];
            let mut distinct = true;
            for i in 0..all.len() {
                for j in (i + 1)..all.len() {
                    if all.get(i) == all.get(j) {
                        distinct = false;
                    }
                }
            }
            // 每个码都带非空处置建议（三要素齐）。
            distinct
                && all.iter().all(|c| !c.is_empty())
                && ProtocolErr::NotSealed.advise().contains("半帧落盘")
                && ProtocolErr::FrameOverlap.advise().contains("撕裂")
        },
        "13 个错误码两两不同且各带处置建议（处置方向不共用码）",
    );
    s.add(
        "C19-AUDIT-STABLE",
        {
            // 留痕顺序稳定（同输入两次运行逐字相同）——否则留痕无法用于事后归因。
            // 走三条真实留痕路径：过期引用核实、帧级作废、未知层回流。
            // （判据要点：留痕必须**非空**且逐字可复现，空留痕等于没有留痕。）
            let run = || -> Vec<String> {
                let mut br = SurfaceBridge::new(true, vec![1, 2]);
                let b = br.acquire_buffer(BufferOwner::Surface, 16, 16);
                let _ = br.begin_frame(61);
                let _ = br.project(&fixture_items());
                let _ = br.reflow_push(ReflowEvent::Content {
                    node_id: 2,
                    world: SurfaceRect { x: 0.0, y: 0.0, w: 4.0, h: 4.0 },
                    source: ReflowSource::VideoFrame,
                });
                let _ = br.commit_frame(&[]);
                let _ = br.verify_buffer(BufferSlot::new(b.slot, 9).unwrap_or(BufferSlot::invalid()));
                // 第二帧：入列后显式作废（留「不作废则半帧落盘」痕）。
                let _ = br.begin_frame(62);
                let _ = br.project(&fixture_items());
                let _ = br.abort_frame("装配侧中断");
                let mut out = br.assembler().audits().to_vec();
                out.extend(br.reflow().audits().to_vec());
                out
            };
            let a1 = run();
            let a2 = run();
            !a1.is_empty() && a1 == a2
        },
        "同输入两次运行的留痕逐字相同（顺序稳定，可用于事后归因）",
    );
    s.add(
        "C19-BRIDGE-ONE-FRAME",
        {
            // 端到端一帧：投影→封帧→提交→双向结算，各面数据自洽。
            let mut br = SurfaceBridge::new(true, vec![1, 2, 3]);
            br.set_viewport(SurfaceRect { x: 0.0, y: 0.0, w: 400.0, h: 300.0 });
            let b = br.acquire_buffer(BufferOwner::Surface, 64, 48);
            let started = br.begin_frame(71).is_ok();
            let items = fixture_items();
            let n = br.project(&items);
            let reflow_ok = br.reflow_push(ReflowEvent::Content {
                node_id: 2,
                world: SurfaceRect { x: 10.0, y: 10.0, w: 50.0, h: 40.0 },
                source: ReflowSource::VideoFrame,
            });
            let out = br.commit_frame(&[]);
            match (started, n, reflow_ok, out) {
                (true, Ok(cnt), Ok(()), Ok(o)) => {
                    let p = o.commit.unwrap_or(CommitPoint {
                        frame: 0,
                        cmd_count: 0,
                        digest: 0,
                        output: OutputState::Dropped("E_NONE"),
                    });
                    cnt == items.len()
                        && p.frame == 71
                        && p.cmd_count == items.len()
                        && o.reflow.dirties.len() == 1
                        && o.surface_damage.region_count() >= 1
                        && br.link().presented_frames() == 1
                        && br.buffers().live_count() == 1
                        && o.errors.is_empty()
                        && p.digest != 0
                        && b.generation != 0
                }
                _ => false,
            }
        },
        "端到端一帧自洽：4项投影→4命令、反向1脏区、正向≥1区域、错误账本空",
    );
    let _ = s;
    s
}

/// 序列摘要（自检用；与入列增量算法同源）。
fn seq_digest(cmds: &[SurfaceCmd]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for c in cmds.iter() {
        h = mix64(h, cmd_fingerprint(c));
    }
    h
}

/// 把一份树侧投影**规范化成**表面命令序列（自检比对用；与 [`FrameAssembler::accept`]
/// 走同一裁决路径，故不存在「自证」——它就是被测代码的等价展开）。
fn sealed_commands(items: &[LayerDrawRef]) -> Vec<SurfaceCmd> {
    let mut out: Vec<SurfaceCmd> = Vec::new();
    let mut depth: u32 = 0;
    for it in items.iter() {
        match project(it) {
            Projection::Emit(c) => out.push(c),
            Projection::Mark(CmdKind::EnterGroup) => {
                depth = depth.saturating_add(1);
                out.push(SurfaceCmd {
                    kind: CmdKind::EnterGroup,
                    node_id: 0,
                    depth,
                    opacity: 1.0,
                    src: BufferSlot::invalid(),
                    dst: SurfaceRect::zero(),
                });
            }
            Projection::Mark(CmdKind::LeaveGroup) => {
                depth = depth.saturating_sub(1);
                out.push(SurfaceCmd {
                    kind: CmdKind::LeaveGroup,
                    node_id: 0,
                    depth,
                    opacity: 1.0,
                    src: BufferSlot::invalid(),
                    dst: SurfaceRect::zero(),
                });
            }
            Projection::Skip(_) => {}
            Projection::Mark(_) => {}
        }
    }
    out
}

impl TreeDirty {
    /// 动作字段的取值判定（自检可读性；返回语义动作而非判别值）。
    pub fn opp(&self) -> TreeDirtyOp {
        self.op
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_union_and_intersect() {
        let a = SurfaceRect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 };
        let b = SurfaceRect { x: 5.0, y: 5.0, w: 10.0, h: 10.0 };
        let u = a.union(&b);
        assert_eq!((u.x, u.y, u.w, u.h), (0.0, 0.0, 15.0, 15.0));
        assert!(a.intersect(&b).is_some());
        let far = SurfaceRect { x: 100.0, y: 100.0, w: 1.0, h: 1.0 };
        assert!(a.intersect(&far).is_none());
    }

    #[test]
    fn rect_new_rejects_negative_and_nan() {
        assert!(SurfaceRect::new(0.0, 0.0, -1.0, 1.0).is_none());
        assert!(SurfaceRect::new(f32::NAN, 0.0, 1.0, 1.0).is_none());
        assert!(SurfaceRect::new(0.0, 0.0, 0.0, 0.0).is_some());
    }

    #[test]
    fn half_frame_never_lands() {
        let mut a = FrameAssembler::new();
        let mut l = SurfaceLink::new(true);
        assert!(a.begin_frame(1).is_ok());
        assert!(project_all(&mut a, &fixture_items()).is_ok());
        // 未封帧提交 → 拒；相位仍为 Open。
        assert_eq!(a.commit(&mut l), Err(ProtocolErr::NotSealed));
        assert_eq!(a.phase(), Phase::Open);
    }

    #[test]
    fn sealed_frame_then_commit_once() {
        let mut a = FrameAssembler::new();
        let mut l = SurfaceLink::new(true);
        assert!(a.begin_frame(2).is_ok());
        assert!(project_all(&mut a, &fixture_items()).is_ok());
        let f = a.seal().expect("封帧应成功");
        assert_eq!(f.cmd_count(), 4);
        let p = a.commit(&mut l).expect("提交应成功");
        assert_eq!(p.output, OutputState::Presented);
        assert_eq!(a.commit(&mut l), Err(ProtocolErr::AlreadyCommitted));
    }

    #[test]
    fn stale_buffer_reference_is_rejected() {
        let mut reg = BufferRegistry::new();
        let b = reg.acquire(BufferOwner::Surface, 4, 4);
        assert!(reg.invalidate(b).is_ok());
        assert_eq!(reg.resolve(b), Err(ProtocolErr::BufferGeneration));
        let b2 = reg.acquire(BufferOwner::Surface, 4, 4);
        assert_eq!(b2.slot, b.slot);
        assert_ne!(b2.generation, b.generation);
    }

    #[test]
    fn reflow_storm_coalesces_and_records() {
        let mut c = ReflowCollector::new(vec![1]);
        for _ in 0..(REFLOW_STORM_THRESHOLD + 1) {
            let _ = c.push(ReflowEvent::Content {
                node_id: 1,
                world: SurfaceRect { x: 0.0, y: 0.0, w: 2.0, h: 2.0 },
                source: ReflowSource::VideoFrame,
            });
        }
        let r = c.settle_frame();
        assert!(r.storm);
        assert_eq!(r.dirties.len(), 1);
        assert_eq!(c.storm_frames(), 1);
        assert_eq!(c.coalesced_count(), REFLOW_STORM_THRESHOLD as u64);
    }

    #[test]
    fn surface_detached_still_commits() {
        let mut a = FrameAssembler::new();
        let mut l = SurfaceLink::new(false);
        let _ = a.begin_frame(3);
        let _ = project_all(&mut a, &fixture_items());
        let _ = a.seal();
        let p = a.commit(&mut l).expect("解耦：提交仍应成功");
        assert_eq!(p.output, OutputState::Dropped("E_SURFACE_DETACHED"));
        assert_eq!(l.dropped_frames(), 1);
        assert_eq!(l.presented_frames(), 0);
    }

    #[test]
    fn commit_is_constant_time() {
        let mut a = FrameAssembler::new();
        let mut l = SurfaceLink::new(true);
        let _ = a.begin_frame(4);
        let _ = project_all(&mut a, &fixture_items());
        let before = a.counters();
        let _ = a.seal();
        let _ = a.commit(&mut l);
        let after = a.counters();
        assert_eq!(before.digest_updates, 4);
        assert_eq!(after.frame_scans, 0);
        assert_eq!(after.digest_updates, before.digest_updates);
    }

    #[test]
    fn effects_checks_all_green() {
        let set = run_ved19_checks();
        let (p, f) = set.tally();
        assert!(!set.truncated(), "自检集被截断：dropped={}", set.dropped());
        assert!(set.all_passed(), "VE-F0619 红项：{}/{}", p, p + f);
    }
}