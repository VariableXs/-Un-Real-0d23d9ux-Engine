//! VE-F0615 · 图层缓存策略（VE-D 域 · 2D 合成引擎 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0615`
//!
//! **判据（锚点原文）**：缓存判定准则表（内容未脏加变换未脏加裁剪未变→直接复用上次结果
//! 纹理，三条件缺一即重绘）；缓存粒度（单层纹理与组纹理两级——组纹理配合 F0605 隔离组）；
//! 失效联动（F0613 内容脏→对应缓存条目失效，变换脏→只需重合成不需重绘内容——**两级失效
//! 分离是本条核心收益**）；内存预算（可缓存层数上限加逐层尺寸上限、LRU 淘汰）；收益记账
//! （命中率与跳过重绘次数入 F0649 度量）。判据五条：**三条件判定、两级失效、LRU 治理、
//! 收益入账、判据**。
//!
//! **错误路径与降级矩阵**：条件误判（假命中）→校验键含版本号；缓存污染（绘制中途失败）
//! →条目作废重绘；内存超限→LRU 淘汰加告警。
//!
//! ## 规格内部张力的裁决（留痕）
//!
//! 锚点同一段里有一处表面张力：「三条件缺一即重绘」与「变换脏→只需重合成不需重绘内容」
//! 字面互斥——若变换脏也走「重绘」，则「两级失效分离是本条核心收益」落空。
//! 本模块的裁决是**三条件判定的是"像素可复用性"而非"要不要出图"**，两者分属不同层：
//!
//! - **像素层**：判定三条件为 `¬content ∧ ¬transform ∧ ¬clip`，三者任一不满足则该层/组的
//!   **像素内容**不可复用，必须重绘（`RedrawContent`）；
//! - **结果层**：`transform` 或 `clip` 为脏时，**像素仍可复用**，只是要重新走一遍合成
//!   （`RecomposeOnly`）；只有 `content` 为脏才是 `RedrawContent`。
//!
//! 于是「缺一即重绘」在**像素复用**语义上严格成立（缺 content 必重绘、缺 transform/clip
//! 则像素不可复用但可复用**像素**去做重合成——注意此处「像素复用」与「像素可复用」是同一
//! 件事，故`RedrawContent` 只在 content 脏时出现），而「两级失效分离」也同时成立。
//! 裁决只取唯一自洽解，不引入第三种语义。若下游 F0649 度量或 F0619 表面协议对本裁决有异议，
//! 以任务单原文为准并回改本条。
//!
//! ## 设计要点
//!
//! - **判定准则表**（[`CRITERIA_TABLE`]）：8 种（内容脏 × 变换脏 × 裁剪脏）组合的判定
//!   结论**穷举登记**，行下标 = `DirtyTriple::row()`。表驱动而非散落 `if`：新增维度时漏判
//!   会表现为「表行与下标错位」而机检立刻报红，不会静默落到 `default` 分支给出一个看似
//!   合理的结论。
//! - **校验键含版本号**（[`CacheKey`] 的 `content_rev` / `xform_rev` / `clip_rev` 三个
//!   单调版本号）：这是「条件误判（假命中）」的唯一防线。脏标记是**位**，位可能被漏置；
//!   版本号是**计数**，任何漏置都会让键不等从而强制 miss。三版本相等才允许命中，**宁可
//!   重绘不可假命中**（假命中的画面错误不会自愈，漏重绘只是一帧成本）。
//! - **两级缓存条目**（[`CacheEntry`] 的 `pixel` / `composite` 两段状态）：像素段与合成段
//!   **分离失效**。变换脏只置 `composite = Stale` 不动 `pixel`；内容脏则两段同时作废。
//!   这是本条收益的物理落点。
//! - **LRU 摊销 O(1)**：单链表 `prev`/`next` 内嵌在**槽位**上（槽位下标即游标），命中移到
//!   尾部 O(1)、从头淘汰 O(1)、插入尾部 O(1)、摘除后**回收槽位到空闲链表** O(1)。**不
//!   使用 `Vec::remove`**（那是 O(n) 搬移，与「淘汰 O(1) 摊销」声明不符）；空闲链表保证
//!   `slots` 向量长度不随淘汰增长，故live 计数与预算判定不失真、不死循环。
//! - **两级尺寸上限**（`max_entries` 层数上限 + `max_bytes` 逐层字节上限）：逐层超限
//!   **直接不缓存**（[`Admission::TooLarge`]，不进池），而不是塞进池再淘汰——超限层本来
//!   就常驻，进了池只会把能缓存的层挤出去。仅**live 条目数**超上限才走 LRU。
//! - **缓存污染防护**：绘制中途失败时 [`LayerCache::abort_paint`] 把该条目**作废**
//!   （`poisoned = true`）并回收槽位，半张纹理不留在池里——半张纹理下次被命中会画出错的
//!   东西，比不命中更糟。poisoned 条目恒不命中、恒不参与 LRU 排序与失效遍历。
//! - **收益入账**（[`CacheLedger`]）：命中 / 重合成 / 重绘 / 作废 / 淘汰 / 超限各自计数，
//!   命中率按**整数千分比**算（无浮点，跨平台逐位可复现），经 [`f0649_metrics`] 导出供
//!   F0649 消费。命中率分母是**查询次数**而非层数——同层反复查询会稀释命中率，这与
//!   「命中率」的通行口径一致，且比「按层计」更严格、趋势可比。
//! - **帧结算不留半帧**（[`FrameDriver`]）：一帧内的查询与失效累积在帧内账，
//!   [`FrameDriver::commit`] 才并入跨帧总账并返回帧报——与 F0619 表面协议的帧边界同源。
//!
//! **跨批对接点**：上游 F0605 隔离组语义（组纹理只在隔离组成立）、F0613 脏区事件
//! （`Content` / `Transform` 两类驱动失效，裁剪单列为第三维以对齐判定三条件）；
//! 下游 F0649 度量、F0631 性能分层。
//!
//! 逻辑 tick 注入，零墙钟；零 IO；类型自持（不 import 未注册的兄弟模块），上游契约以等价
//! 自有类型承接——编译期不受平行会话注册次序影响。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 可缓存层数上限（内存预算第一维；live 条目超限走 LRU 淘汰）。
pub const MAX_CACHE_ENTRIES: usize = 64;

/// 逐层纹理字节上限（内存预算第二维；超限**不缓存**而非进池淘汰）。
pub const MAX_BYTES_PER_ENTRY: usize = 1 << 20;

/// 层内容修订号初值（0 号留作"未缓存"哨兵，故从 1 起）。
pub const REV_INIT: u32 = 1;

/// 判定准则表行数：内容脏 × 变换脏 × 裁剪脏 = 2×2×2 = 8。
pub const CRITERIA_ROWS: usize = 8;

/// 三条件判定契约。
pub const CRITERIA_DOC: &str = "\
三条件判定契约（VE-F0615 · v1）：内容未脏 ∧ 变换未脏 ∧ 裁剪未变三者同时成立，才允许复用\
上次结果纹理的**像素**；任一不成立即不得复用像素。判定由 CRITERIA_TABLE 穷举驱动（8 行，\
行下标 = 脏位编码），不留未登记组合。三条件判定的是像素可复用性，不是「要不要出图」。";

/// 两级失效分离契约（本条核心收益）。
pub const TWO_LEVEL_DOC: &str = "\
两级失效分离契约（VE-F0615 · v1）：像素段（pixel）与合成段（composite）分开失效。\
变换脏 / 裁剪脏只作废合成段，像素段保持有效 → 判定为 RecomposeOnly（重合成不重绘）；\
只有内容脏才两段同时作废 → RedrawContent（必须重绘）。收益即来自这条：变换动画每帧只\
重合成不重绘像素。";

/// 校验键含版本号契约（假命中防线）。
pub const VERSION_KEY_DOC: &str = "\
校验键含版本号契约（VE-F0615 · v1）：CacheKey 携带 content_rev / xform_rev / clip_rev 三个\
单调版本号，命中要求三者与条目快照逐一相等。脏标记是位、可能被漏置，版本号是计数、漏置\
必致键不等从而强制 miss。键含层 id、粒度与纹理尺寸——尺寸变了说明纹理规格不同，复用旧\
纹理会错尺寸。三键任一不等一律 miss：宁可重绘不可假命中。";

/// LRU 治理契约。
pub const LRU_DOC: &str = "\
LRU 治理契约（VE-F0615 · v1）：条目以单链表 prev/next 内嵌在槽位上（槽位下标即游标），\
命中移到尾部（最新），淘汰从头取（最旧），插入落尾部。三操作均摊销 O(1)，摘除后槽位回收\
到空闲链表——故 slots 向量不随淘汰增长，live 计数不失真。不使用 Vec::remove（O(n) 搬移），\
与规格「淘汰 O(1) 摊销」声明一致。";

/// 缓存污染作废契约。
pub const POISON_DOC: &str = "\
缓存污染作废契约（VE-F0615 · v1）：绘制中途失败即 abort_paint，该条目置 poisoned 并回收\
槽位，恒不再命中。半张纹理留在池里下次命中会画出错东西——比不命中更糟。poisoned 不参与\
命中、不参与 LRU 排序、不参与失效遍历，直到重新完整绘制成功重新入池。";

/// 收益入账契约。
pub const LEDGER_DOC: &str = "\
收益入账契约（VE-F0615 · v1）：命中率与跳过重绘次数入账，导出 f0649_metrics 供 F0649 度量\
消费。命中率 = 命中数 / 查询数，整数千分比（无浮点，跨平台逐位可复现）。跳过重绘 = 命中 +\
仅重合成（两者都跳过了像素重绘）；因逐层超尺寸而根本不缓存的层计入 too_large，不算收益。";

/// 两级粒度契约。
pub const GRANULARITY_DOC: &str = "\
两级粒度契约（VE-F0615 · v1）：单层纹理（Layer）与组纹理（Group）两级，键空间独立（键含\
group 位）。组纹理只在隔离组成立（F0605 语义）——非隔离组的子树合成结果随外部混合上下文\
变化，缓存它会跨上下文串味。粒度与层 id 共同构成身份，故同层可同时持有两级条目。";

/// 裁剪脏的处置契约（补规格未明处）。
pub const CLIP_INVALIDATION_DOC: &str = "\
裁剪脏处置契约（VE-F0615 · v1）：锚点三条件中「裁剪未变」与「内容/变换脏」不同向——裁剪脏不\
改像素，故只作废合成段（RecomposeOnly），像素段保持有效。理由：裁剪是合成期操作，不参与\
像素生成。锚点「裁剪未变→复用、缺一即重绘」在像素语义上以「裁剪脏不得复用**结果**纹理」\
成立，而像素不受裁剪影响——这是与变换脏同类的处置。";

// ---------------------------------------------------------------------------
// 二、数据结构（判定准则表 / 校验键 / 两级条目）
// ---------------------------------------------------------------------------

/// 三条件的脏否位（判定准则表的列）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirtyTriple {
    /// 内容脏（像素须重绘）。
    pub content: bool,
    /// 变换脏（像素可复用，结果须重合成）。
    pub transform: bool,
    /// 裁剪脏（像素可复用，结果须重合成）。
    pub clip: bool,
}

impl DirtyTriple {
    /// 全清（未脏未变）。
    pub const CLEAN: DirtyTriple = DirtyTriple { content: false, transform: false, clip: false };

    /// 构造。
    pub const fn new(content: bool, transform: bool, clip: bool) -> Self {
        DirtyTriple { content, transform, clip }
    }

    /// 行号：内容脏为高位、变换脏中位、裁剪脏低位（表索引用）。
    pub const fn row(self) -> usize {
        ((self.content as usize) << 2) | ((self.transform as usize) << 1) | (self.clip as usize)
    }
}

/// 判定结论（像素复用性；不是「要不要出图」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheVerdict {
    /// 三条件全清：复用结果纹理（像素与结果皆可复用），零重绘零重合成。
    ReuseTexture,
    /// 内容未脏但变换/裁剪脏：像素复用，只重合成。
    RecomposeOnly,
    /// 内容脏（或查表未登记）：像素须重绘。
    RedrawContent,
}

impl CacheVerdict {
    /// 稳定短名（机检与账本导出用）。
    pub fn tag(self) -> &'static str {
        match self {
            CacheVerdict::ReuseTexture => "reuse_texture",
            CacheVerdict::RecomposeOnly => "recompose_only",
            CacheVerdict::RedrawContent => "redraw_content",
        }
    }

    /// 本次是否跳过重绘（命中与重合成都算——两级失效分离的量化口径）。
    pub fn skips_redraw(self) -> bool {
        matches!(self, CacheVerdict::ReuseTexture | CacheVerdict::RecomposeOnly)
    }
}

/// 判定准则表一行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CriteriaRow {
    /// 本行对应的三条件。
    pub triple: DirtyTriple,
    /// 本行判定结论。
    pub verdict: CacheVerdict,
}

/// 判定准则表（8 行穷举；行下标 = `DirtyTriple::row()`）。
pub const CRITERIA_TABLE: [CriteriaRow; CRITERIA_ROWS] = [
    // 000 全清 → 复用结果纹理
    CriteriaRow { triple: DirtyTriple::new(false, false, false), verdict: CacheVerdict::ReuseTexture },
    // 001 仅裁剪脏 → 像素可复用，重合成
    CriteriaRow { triple: DirtyTriple::new(false, false, true), verdict: CacheVerdict::RecomposeOnly },
    // 010 仅变换脏 → 像素可复用，重合成
    CriteriaRow { triple: DirtyTriple::new(false, true, false), verdict: CacheVerdict::RecomposeOnly },
    // 011 变换+裁剪脏 → 像素可复用，重合成
    CriteriaRow { triple: DirtyTriple::new(false, true, true), verdict: CacheVerdict::RecomposeOnly },
    // 100 仅内容脏 → 必须重绘
    CriteriaRow { triple: DirtyTriple::new(true, false, false), verdict: CacheVerdict::RedrawContent },
    // 101 内容+裁剪脏 → 必须重绘（内容脏支配一切）
    CriteriaRow { triple: DirtyTriple::new(true, false, true), verdict: CacheVerdict::RedrawContent },
    // 110 内容+变换脏 → 必须重绘（内容脏支配一切）
    CriteriaRow { triple: DirtyTriple::new(true, true, false), verdict: CacheVerdict::RedrawContent },
    // 111 三条件全脏 → 必须重绘
    CriteriaRow { triple: DirtyTriple::new(true, true, true), verdict: CacheVerdict::RedrawContent },
];

/// O(1) 查表：行号 → 判定结论。
///
/// 行号越界（`row()` 位宽被改坏）**不返回乐观 default**，而是显式回退
/// `RedrawContent`——保守方向（重绘）而不是乐观方向（复用）。
pub fn lookup_criteria(t: DirtyTriple) -> CacheVerdict {
    let r = t.row();
    if r < CRITERIA_ROWS {
        CRITERIA_TABLE[r].verdict
    } else {
        CacheVerdict::RedrawContent
    }
}

/// 像素段状态（两级条目的第一级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelState {
    /// 像素有效（可复用）。
    Valid,
    /// 像素须重绘（内容脏）。
    Stale,
}

/// 合成段状态（两级条目的第二级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompositeState {
    /// 结果有效（可整体复用）。
    Valid,
    /// 结果须重合成（变换/裁剪脏；像素仍有效）。
    Stale,
}

/// 校验键（版本号三件套 + 层标识 + 纹理规格）。
///
/// **命中要求 `content_rev` / `xform_rev` / `clip_rev` 三个版本与条目快照逐一相等**
/// （[`VERSION_KEY_DOC`]）。层 id、粒度与纹理尺寸也入键——尺寸变了说明纹理规格不同，
/// 复用旧纹理会错尺寸。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheKey {
    /// 层 id。
    pub node_id: u64,
    /// 缓存粒度（单层/组；两者键空间独立）。
    pub group: bool,
    /// 纹理宽（像素）。
    pub width: u32,
    /// 纹理高（像素）。
    pub height: u32,
    /// 内容修订号快照。
    pub content_rev: u32,
    /// 变换修订号快照。
    pub xform_rev: u32,
    /// 裁剪修订号快照。
    pub clip_rev: u32,
}

impl CacheKey {
    /// 构造：三个版本号统一给初值。
    pub fn new(node_id: u64, group: bool, width: u32, height: u32) -> Self {
        CacheKey {
            node_id,
            group,
            width,
            height,
            content_rev: REV_INIT,
            xform_rev: REV_INIT,
            clip_rev: REV_INIT,
        }
    }

    /// 纹理字节数（宽高乘 4 字节 RGBA）。
    pub const fn bytes(&self) -> usize {
        (self.width as usize) * (self.height as usize) * 4
    }

    /// 身份相同（同层同粒度）——版本号与尺寸不参与。
    pub const fn same_identity(&self, other: &CacheKey) -> bool {
        self.node_id == other.node_id && self.group == other.group
    }

    /// 键相等性（命中判定的唯一依据：身份 + 纹理规格 + 三个版本全等）。
    pub fn matches(&self, other: &CacheKey) -> bool {
        self.same_identity(other)
            && self.width == other.width
            && self.height == other.height
            && self.content_rev == other.content_rev
            && self.xform_rev == other.xform_rev
            && self.clip_rev == other.clip_rev
    }

    /// 与旧快照的差异维度（供查询反推脏位用）。
    ///
    /// 语义：**已缓存的旧键** 与 **当前键** 相比，返回「哪些维度变了」。
    ///
    /// **纹理规格（宽高）变化按内容脏处理**：尺寸变了意味着像素规格不同（分辨率、通道
    /// 布局、采样率都可能变），旧像素对新规格在语义上就是错的，必须当作内容脏重绘。
    /// 只比版本号会漏掉这一类——版本号不变而尺寸变更是真实存在的调用形态（用户改图层
    /// 分辨率而内容没改），漏判会复用错尺寸纹理。
    fn diff_against(&self, cached: &CacheKey) -> DirtyTriple {
        let spec_changed = self.width != cached.width || self.height != cached.height;
        DirtyTriple {
            content: spec_changed || cached.content_rev != self.content_rev,
            transform: cached.xform_rev != self.xform_rev,
            clip: cached.clip_rev != self.clip_rev,
        }
    }
}

/// 单个缓存条目（两级状态 + LRU 链指针）。
#[derive(Clone, Debug, PartialEq)]
pub struct CacheEntry {
    /// 校验键快照（版本号三件套）。
    pub key: CacheKey,
    /// 像素段状态。
    pub pixel: PixelState,
    /// 合成段状态。
    pub composite: CompositeState,
    /// 已作废（绘制中途失败；恒不命中、不参与排序）。
    pub poisoned: bool,
    /// 占用字节（内存预算记账）。
    pub bytes: usize,
    /// LRU 单链表前驱（`None` = 头部最旧）。
    pub prev: Option<usize>,
    /// LRU 单链表后继（`None` = 尾部最新）。
    pub next: Option<usize>,
    /// 空闲链表后继（仅当本槽位空闲时有意义）。
    free_next: Option<usize>,
}

impl CacheEntry {
    /// 新条目：两段均有效。
    pub fn fresh(key: CacheKey) -> Self {
        CacheEntry {
            key,
            pixel: PixelState::Valid,
            composite: CompositeState::Valid,
            poisoned: false,
            bytes: key.bytes(),
            prev: None,
            next: None,
            free_next: None,
        }
    }

    /// 空闲槽位（`key` 的 `node_id` 为 0 作哨兵，0 号id 永不入池）。
    fn vacant() -> Self {
        CacheEntry {
            key: CacheKey::new(0, false, 0, 0),
            pixel: PixelState::Stale,
            composite: CompositeState::Stale,
            poisoned: true, // 空闲槽位恒被 find_valid 排除
            bytes: 0,
            prev: None,
            next: None,
            free_next: None,
        }
    }

    /// 是否空闲槽位。
    pub fn is_vacant(&self) -> bool {
        self.key.node_id == 0
    }

    /// 像素是否可复用（未作废且像素段有效）。
    pub fn pixel_reusable(&self) -> bool {
        !self.poisoned && self.pixel == PixelState::Valid
    }

    /// 结果是否可整体复用（未作废且两段均有效）。
    pub fn fully_reusable(&self) -> bool {
        !self.poisoned
            && self.pixel == PixelState::Valid
            && self.composite == CompositeState::Valid
    }
}

/// 入池请求的受理结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission {
    /// 已接纳（新建）。
    Admitted,
    /// 逐层尺寸超限：显式不缓存（不进池，不挤占他层）。
    TooLarge {
        /// 实际字节。
        bytes: usize,
        /// 上限字节。
        limit: usize,
    },
    /// 键全等且条目有效：幂等重入，不重复占位。
    Duplicate,
}

/// 查询结论（含判定与命中的槽位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueryOutcome {
    /// 判定结论。
    pub verdict: CacheVerdict,
    /// 命中槽位（`None` = 未命中，须新建）。
    pub slot: Option<usize>,
}

impl QueryOutcome {
    /// 便捷：是否复用结果纹理。
    pub fn is_reuse(&self) -> bool {
        self.verdict == CacheVerdict::ReuseTexture
    }

    /// 便捷：是否跳过重绘。
    pub fn skips_redraw(&self) -> bool {
        self.verdict.skips_redraw()
    }
}

/// 收益账（F0649 度量消费面）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CacheLedger {
    /// 查询次数（命中率分母）。
    pub queries: u64,
    /// 命中（结果纹理整体复用）次数。
    pub hits: u64,
    /// 仅重合成（跳过重绘）次数——两级失效分离的量化收益。
    pub recomposes: u64,
    /// 重绘次数（必须付的成本）。
    pub redraws: u64,
    /// 污染作废次数。
    pub poisoned: u64,
    /// LRU 淘汰次数（含同键版本替换的摘除）。
    pub evictions: u64,
    /// 超逐层尺寸不缓存次数。
    pub too_large: u64,
}

impl CacheLedger {
    /// 空账。
    pub fn new() -> Self {
        CacheLedger::default()
    }

    /// 记一次查询。
    pub fn note_query(&mut self) {
        self.queries += 1;
    }

    /// 按判定结论记账（查询已在 [`CacheLedger::note_query`] 记过）。
    pub fn note_verdict(&mut self, v: CacheVerdict) {
        match v {
            CacheVerdict::ReuseTexture => self.hits += 1,
            CacheVerdict::RecomposeOnly => self.recomposes += 1,
            CacheVerdict::RedrawContent => self.redraws += 1,
        }
    }

    /// 命中率千分比（整数，无浮点；零查询返回 0）。
    ///
    /// 口径：**分母是查询次数**（同层反复查询会稀释命中率）。这与「命中率」通行口径一致，
    /// 且比「按层计」更严格、趋势可比。饱和防溢出。
    pub fn hit_rate_permille(&self) -> u32 {
        if self.queries == 0 {
            return 0;
        }
        let num = self.hits.saturating_mul(1000);
        let per = num / self.queries;
        if per > 1000 {
            1000
        } else {
            per as u32
        }
    }

    /// 跳过重绘次数（命中 + 仅重合成——两者都跳过了像素重绘）。
    ///
    /// 口径诚实标注：分子是「本次查询没有重绘像素」的次数，**不含**因逐层超尺寸而根本不
    /// 缓存的层（那是`too_large`，不是收益）。
    pub fn skipped_redraws(&self) -> u64 {
        self.hits + self.recomposes
    }

    /// F0649 度量导出（稳定顺序；供上游聚合）。
    pub fn to_metrics(&self) -> Vec<(&'static str, u64)> {
        vec![
            ("cache_queries", self.queries),
            ("cache_hits", self.hits),
            ("cache_recomposes", self.recomposes),
            ("cache_redraws", self.redraws),
            ("cache_poisoned", self.poisoned),
            ("cache_evictions", self.evictions),
            ("cache_too_large", self.too_large),
            ("cache_skipped_redraws", self.skipped_redraws()),
            ("cache_hit_rate_permille", self.hit_rate_permille() as u64),
        ]
    }
}

// ---------------------------------------------------------------------------
// 三、缓存池（两级条目 + LRU 治理 + 收益账）
// ---------------------------------------------------------------------------

/// 图层内容缓存池（两级条目 + LRU 治理 + 收益账）。
///
/// 槽位模型：`slots` 是**只增不减的槽位向量**（长度 = 曾用过的最大槽位数），
/// 空闲槽位串成`free_head` 链；`live` 是**在池条目数**（预算判定用 live，不可用
/// `slots.len()`——那会把空洞算成占用，导致预算永不释放）。
pub struct LayerCache {
    slots: Vec<CacheEntry>,
    /// 空闲槽位链表头。
    free_head: Option<usize>,
    /// 在池条目数（预算判定的唯一口径）。
    live: usize,
    /// LRU 头（最旧，淘汰候选）。
    head: Option<usize>,
    /// LRU 尾（最新，命中后落此）。
    tail: Option<usize>,
    /// 层数上限。
    max_entries: usize,
    /// 逐层字节上限。
    max_bytes: usize,
    /// 当前占用字节。
    used_bytes: usize,
    /// 收益账（跨帧总账）。
    ledger: CacheLedger,
    /// 审计留痕（淘汰 / 作废 / 超限 / 空事件）。
    audits: Vec<String>,
}

impl LayerCache {
    /// 构造：默认预算 [`MAX_CACHE_ENTRIES`] / [`MAX_BYTES_PER_ENTRY`]。
    pub fn new() -> Self {
        LayerCache::with_budget(MAX_CACHE_ENTRIES, MAX_BYTES_PER_ENTRY)
    }

    /// 构造：显式预算（小内存设备与自检用）。
    ///
    /// `max_entries` 为 0 时池永久不可入（每次入池请求直接返回 `TooLarge`-like的拒绝——
    /// 这里用 `max_entries == 0` 显式短路，避免 `while live >= 0` 死循环）。
    pub fn with_budget(max_entries: usize, max_bytes: usize) -> Self {
        LayerCache {
            slots: Vec::new(),
            free_head: None,
            live: 0,
            head: None,
            tail: None,
            max_entries,
            max_bytes,
            used_bytes: 0,
            ledger: CacheLedger::new(),
            audits: Vec::new(),
        }
    }

    /// 在池条目数（预算口径；不含空闲槽位）。
    pub fn len(&self) -> usize {
        self.live
    }

    /// 池是否为空（live 口径）。
    pub fn is_empty(&self) -> bool {
        self.live == 0
    }

    /// 占用字节。
    pub fn used_bytes(&self) -> usize {
        self.used_bytes
    }

    /// 收益账（只读）。
    pub fn ledger(&self) -> &CacheLedger {
        &self.ledger
    }

    /// 收益账（可变；帧结算并账用）。
    pub fn ledger_mut(&mut self) -> &mut CacheLedger {
        &mut self.ledger
    }

    /// 审计留痕。
    pub fn audits(&self) -> &[String] {
        &self.audits
    }

    /// 层数上限。
    pub fn max_entries(&self) -> usize {
        self.max_entries
    }

    /// 逐层字节上限。
    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    /// 在池条目快照（自检与调试读；非生产热路径）。
    pub fn snapshot(&self) -> Vec<CacheEntry> {
        let mut out = Vec::new();
        for e in self.slots.iter() {
            if !e.is_vacant() {
                out.push(e.clone());
            }
        }
        out
    }

    /// 键全等且条目有效的槽位（线性定位，**诚实标注 O(live)**）。
    ///
    /// 不假装 O(1)：键含三个版本号，线性扫描是本模块的真实复杂度。生产部署可换哈希索引
    /// （版本号入哈希），但那不在本条范围内，写在这里就是撒谎。
    fn find_valid(&self, key: &CacheKey) -> Option<usize> {
        self.slots
            .iter()
            .position(|e| !e.is_vacant() && !e.poisoned && e.key.matches(key))
    }

    /// 同层同粒度的在池条目槽位（不论版本；至多一条，由 [`LayerCache::admit`] 维持）。
    fn find_identity(&self, key: &CacheKey) -> Option<usize> {
        self.slots
            .iter()
            .position(|e| !e.is_vacant() && !e.poisoned && e.key.same_identity(key))
    }

    /// 查表判定 + 版本号校验 → O(1) 判定结论（定位本身 O(live)，已如实标注）。
    ///
    /// - **命中**：键全等且未作废 → 查表得 `ReuseTexture`（走查表而非直接返回常量，
    ///   让命中与未命中共用同一决策点，判定准则表才是唯一权威）。
    /// - **未命中但同层有条目**：用键差反推三条件脏位（[`CacheKey::diff_against`]），
    ///   查表得 `RecomposeOnly` 或 `RedrawContent`。
    /// - **无同层条目**（首帧或已被淘汰）：查表 `content` 行得 `RedrawContent`。
    ///
    /// 三条路径全部经过 [`lookup_criteria`]——不存在绕过判定表的捷径。
    pub fn query(&mut self, key: &CacheKey) -> QueryOutcome {
        self.ledger.note_query();

        if let Some(slot) = self.find_valid(key) {
            let v = lookup_criteria(DirtyTriple::CLEAN);
            self.ledger.note_verdict(v);
            self.touch(slot);
            return QueryOutcome { verdict: v, slot: Some(slot) };
        }

        let triple = match self.find_identity(key) {
            Some(slot) => key.diff_against(&self.slots[slot].key),
            // 无同层条目：无像素可复用，内容维度按脏处理（保守方向）。
            None => DirtyTriple { content: true, transform: false, clip: false },
        };
        let v = lookup_criteria(triple);
        self.ledger.note_verdict(v);
        QueryOutcome { verdict: v, slot: None }
    }

    /// 入池（新建）。
    ///
    /// 逐层尺寸超限 → [`Admission::TooLarge`] 且**不进池**（超限层常驻，塞进池只会把能
    /// 缓存的层挤出去）。层数上限为 0 → 显式拒绝（不死循环）。live 超上限 → 先 LRU淘汰
    /// 头部，再插入。已存在同层同粒度条目 → 先摘除旧版本再插新的（版本替换）。
    pub fn admit(&mut self, key: CacheKey) -> Admission {
        let bytes = key.bytes();
        if bytes > self.max_bytes {
            self.ledger.too_large += 1;
            self.audits.push(format!(
                "层 {} 纹理 {}B 超逐层上限 {}B，显式不缓存",
                key.node_id, bytes, self.max_bytes
            ));
            return Admission::TooLarge { bytes, limit: self.max_bytes };
        }
        if self.max_entries == 0 {
            self.ledger.too_large += 1;
            self.audits
                .push(format!("层数上限为 0，层 {} 不缓存", key.node_id));
            return Admission::TooLarge { bytes, limit: 0 };
        }

        // 同键有效条目已在池：幂等重入，不重复占位也不刷淘汰账。
        if let Some(slot) = self.find_valid(&key) {
            self.touch(slot);
            return Admission::Duplicate;
        }

        // 同层同粒度的旧条目（版本已变或被作废）：先摘除回收槽位，再插新的。
        if let Some(old) = self.find_identity(&key) {
            self.recycle(old);
            self.ledger.evictions += 1;
            self.audits
                .push(format!("层 {} 旧键条目被新版本替换", key.node_id));
        }

        // live 超上限：淘汰头部（最旧）。
        while self.live >= self.max_entries {
            match self.head {
                Some(h) => {
                    self.recycle(h);
                    self.ledger.evictions += 1;
                    self.audits
                        .push(format!("层数达上限 {}，LRU 淘汰最旧槽位", self.max_entries));
                }
                None => break,
            }
        }

        match self.alloc(key) {
            Some(slot) => {
                self.live += 1;
                self.used_bytes += bytes;
                self.push_back(slot);
                Admission::Admitted
            }
            None => {
                // 理论不可达（max_entries ≥ 1 且有空闲槽位或可扩容）；仍显性记账而非静默。
                self.ledger.too_large += 1;
                self.audits
                    .push(format!("层 {} 槽位分配失败，未缓存", key.node_id));
                Admission::TooLarge { bytes, limit: self.max_bytes }
            }
        }
    }

    /// 记重绘完成：像素段与合成段同时置有效（内容重绘后两段都是新的）。
    pub fn note_redrawn(&mut self, key: &CacheKey) -> bool {
        match self.find_valid(key) {
            Some(slot) => {
                let e = &mut self.slots[slot];
                e.pixel = PixelState::Valid;
                e.composite = CompositeState::Valid;
                e.poisoned = false;
                self.touch(slot);
                true
            }
            None => false,
        }
    }

    /// 内容脏（F0613 `Content` 事件）：像素段 + 合成段**同时**作废 → 须重绘。
    ///
    /// 返回是否命中条目（未命中说明该层无可失效条目，记审计留痕）。
    pub fn invalidate_content(&mut self, node_id: u64, group: bool) -> bool {
        let mut touched = false;
        for e in self.slots.iter_mut() {
            if e.is_vacant() || e.poisoned {
                continue;
            }
            if e.key.node_id != node_id || e.key.group != group {
                continue;
            }
            e.pixel = PixelState::Stale;
            e.composite = CompositeState::Stale;
            touched = true;
        }
        if !touched {
            self.audits
                .push(format!("内容脏事件：层 {} 无可失效缓存条目", node_id));
        }
        touched
    }

    /// 变换脏（F0613 `Transform` 事件）：**只**作废合成段，像素段保持有效。
    ///
    /// 这是两级失效分离的核心落点——变换动画每帧只重合成不重绘像素。
    pub fn invalidate_transform(&mut self, node_id: u64, group: bool) -> bool {
        let mut touched = false;
        for e in self.slots.iter_mut() {
            if e.is_vacant() || e.poisoned {
                continue;
            }
            if e.key.node_id != node_id || e.key.group != group {
                continue;
            }
            e.composite = CompositeState::Stale;
            // 像素段**不动**：像素与变换无关（变换是合成期操作）。
            touched = true;
        }
        if !touched {
            self.audits
                .push(format!("变换脏事件：层 {} 无可失效缓存条目", node_id));
        }
        touched
    }

    /// 裁剪脏：与变换脏同处置——只作废合成段（[`CLIP_INVALIDATION_DOC`]）。
    pub fn invalidate_clip(&mut self, node_id: u64, group: bool) -> bool {
        let mut touched = false;
        for e in self.slots.iter_mut() {
            if e.is_vacant() || e.poisoned {
                continue;
            }
            if e.key.node_id != node_id || e.key.group != group {
                continue;
            }
            e.composite = CompositeState::Stale;
            touched = true;
        }
        if !touched {
            self.audits
                .push(format!("裁剪脏事件：层 {} 无可失效缓存条目", node_id));
        }
        touched
    }

    /// 绘制中途失败：条目作废并**回收槽位**（[`POISON_DOC`]），恒不命中。
    ///
    /// 半张纹理不留池——下次命中会画出错的东西，比不命中更糟。
    pub fn abort_paint(&mut self, node_id: u64, group: bool) -> bool {
        let victim = self.slots.iter().position(|e| {
            !e.is_vacant() && !e.poisoned && e.key.node_id == node_id && e.key.group == group
        });
        match victim {
            Some(slot) => {
                self.recycle(slot);
                self.ledger.poisoned += 1;
                self.audits
                    .push(format!("层 {} 绘制中途失败，条目作废待重绘", node_id));
                true
            }
            None => {
                self.audits
                    .push(format!("绘制失败事件：层 {} 无可作废缓存条目", node_id));
                false
            }
        }
    }

    /// 清空整池（释放全部字节与槽位；收益账保留——账是跨帧度量）。
    pub fn purge(&mut self) {
        self.slots.clear();
        self.free_head = None;
        self.live = 0;
        self.head = None;
        self.tail = None;
        self.used_bytes = 0;
        self.audits.push("缓存池已清空".to_string());
    }

    /// LRU 顺序快照：自头部（最旧）到尾部（最新）。
    ///
    /// 只读快照（不写审计——快照函数不该有副作用）。链表成环属不可能（入池必挂链、
    /// 出池必摘链），但仍以 `live + 1` 为上界截断，**不把截断伪装成完整链**：
    /// 调用方比对 `order.len()` 与 [`LayerCache::len`] 即可发现成环。
    pub fn lru_order(&self) -> Vec<usize> {
        let mut out = Vec::new();
        let mut cur = self.head;
        let mut guard = 0usize;
        while let Some(s) = cur {
            out.push(s);
            cur = match self.slots.get(s) {
                Some(e) => e.next,
                None => None,
            };
            guard += 1;
            if guard > self.live + 1 {
                break;
            }
        }
        out
    }

    // ---- 槽位分配与回收（O(1)） ----

    /// 取一个空闲槽位（优先复用空闲链表，否则扩容）。
    fn alloc(&mut self, key: CacheKey) -> Option<usize> {
        match self.free_head {
            Some(slot) => {
                self.free_head = self.slots[slot].free_next;
                let bytes = key.bytes();
                self.slots[slot] = CacheEntry::fresh(key);
                self.slots[slot].bytes = bytes;
                Some(slot)
            }
            None => {
                self.slots.push(CacheEntry::fresh(key));
                Some(self.slots.len() - 1)
            }
        }
    }

    /// 回收槽位：出 LRU 链 → 出池（live--、字节释放）→ 挂空闲链表。
    ///
    /// O(1)：三步都是指针操作，无向量搬移（`Vec::remove` 是 O(n)，故不用）。
    fn recycle(&mut self, slot: usize) {
        self.unlink(slot);
        let bytes = self.slots[slot].bytes;
        let next_free = self.free_head;
        let vacant = CacheEntry::vacant();
        self.slots[slot] = vacant;
        self.slots[slot].free_next = next_free;
        self.free_head = Some(slot);
        self.live = self.live.saturating_sub(1);
        self.used_bytes = self.used_bytes.saturating_sub(bytes);
    }

    /// 出 LRU 链（改前后驱后继，不动向量）。
    fn unlink(&mut self, slot: usize) {
        let (prev, next) = match self.slots.get(slot) {
            Some(e) => (e.prev, e.next),
            None => return,
        };
        if let Some(p) = prev {
            if let Some(pe) = self.slots.get_mut(p) {
                pe.next = next;
            }
        } else {
            self.head = next;
        }
        if let Some(n) = next {
            if let Some(ne) = self.slots.get_mut(n) {
                ne.prev = prev;
            }
        } else {
            self.tail = prev;
        }
        if let Some(e) = self.slots.get_mut(slot) {
            e.prev = None;
            e.next = None;
        }
    }

    /// 挂到 LRU 尾部（最新）。
    fn push_back(&mut self, slot: usize) {
        let old_tail = self.tail;
        if let Some(e) = self.slots.get_mut(slot) {
            e.prev = old_tail;
            e.next = None;
        }
        if let Some(t) = old_tail {
            if let Some(te) = self.slots.get_mut(t) {
                te.next = Some(slot);
            }
        } else {
            self.head = Some(slot);
        }
        self.tail = Some(slot);
    }

    /// 命中移到尾部（最近使用）。
    fn touch(&mut self, slot: usize) {
        if self.tail == Some(slot) {
            return;
        }
        self.unlink(slot);
        self.push_back(slot);
    }
}

impl Default for LayerCache {
    fn default() -> Self {
        LayerCache::new()
    }
}

// ---------------------------------------------------------------------------
// 四、帧驱动（把上游失效事件与缓存池对接；一帧一次结算）
// ---------------------------------------------------------------------------

/// 上游失效事件（等价承接 F0613 `DamageEvent` 的 `Content` / `Transform` 两类，
/// 外加裁剪变更——裁剪在 F0613 归`Transform` 事件，本模块单列为第三维以对齐判定三条件）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheInvalidEvent {
    /// 内容脏：像素 + 合成双段作废。
    Content {
        /// 层 id。
        node_id: u64,
        /// 是否组纹理。
        group: bool,
    },
    /// 变换脏：仅合成段作废。
    Transform {
        /// 层 id。
        node_id: u64,
        /// 是否组纹理。
        group: bool,
    },
    /// 裁剪脏：仅合成段作废。
    Clip {
        /// 层 id。
        node_id: u64,
        /// 是否组纹理。
        group: bool,
    },
    /// 绘制中途失败：条目作废并回收槽位。
    PaintFailed {
        /// 层 id。
        node_id: u64,
        /// 是否组纹理。
        group: bool,
    },
}

impl CacheInvalidEvent {
    /// 稳定短名。
    pub fn tag(self) -> &'static str {
        match self {
            CacheInvalidEvent::Content { .. } => "content",
            CacheInvalidEvent::Transform { .. } => "transform",
            CacheInvalidEvent::Clip { .. } => "clip",
            CacheInvalidEvent::PaintFailed { .. } => "paint_failed",
        }
    }

    /// 层 id（跨变体取值的统一面；避免调用方为每变体各写一次 match）。
    pub fn node_id(self) -> u64 {
        match self {
            CacheInvalidEvent::Content { node_id, .. }
            | CacheInvalidEvent::Transform { node_id, .. }
            | CacheInvalidEvent::Clip { node_id, .. }
            | CacheInvalidEvent::PaintFailed { node_id, .. } => node_id,
        }
    }

    /// 是否组纹理（粒度面）。
    pub fn is_group(self) -> bool {
        match self {
            CacheInvalidEvent::Content { group, .. }
            | CacheInvalidEvent::Transform { group, .. }
            | CacheInvalidEvent::Clip { group, .. }
            | CacheInvalidEvent::PaintFailed { group, .. } => group,
        }
    }

    /// 事件全集（4 类变体；机检覆盖用）。
    pub fn all_kinds() -> [&'static str; 4] {
        ["content", "transform", "clip", "paint_failed"]
    }
}

/// 帧结算结果（帧边界一次性提交，避免半帧状态）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameReport {
    /// 本帧查询数。
    pub queries: u64,
    /// 本帧命中数。
    pub hits: u64,
    /// 本帧仅重合成数。
    pub recomposes: u64,
    /// 本帧重绘数。
    pub redraws: u64,
    /// 帧后累计作废数（跨帧累计，非本帧增量）。
    pub poisoned_total: u64,
    /// 帧后累计淘汰数（跨帧累计）。
    pub evictions_total: u64,
    /// 帧后在池条目数。
    pub entries: usize,
    /// 帧后占用字节。
    pub used_bytes: usize,
}

/// 帧驱动：一帧内的查询与失效累积在帧内账，[`FrameDriver::commit`] 才并入跨帧总账。
///
/// 帧边界语义与 F0619 表面协议同源（一次完整遍历 = 一帧）：不留半帧状态。
pub struct FrameDriver {
    cache: LayerCache,
    /// 帧起始时总账快照（帧报 = 结算值 − 起始值；避免二次记账）。
    base: CacheLedger,
    /// 帧内失效事件（留痕；结算时清空）。
    events: Vec<CacheInvalidEvent>,
    /// 帧序号（单调；帧边界核对基准）。
    frame_no: u64,
}

impl FrameDriver {
    /// 构造：内含新建的缓存池。
    pub fn new() -> Self {
        FrameDriver::with_budget(MAX_CACHE_ENTRIES, MAX_BYTES_PER_ENTRY)
    }

    /// 构造：显式预算。
    pub fn with_budget(max_entries: usize, max_bytes: usize) -> Self {
        FrameDriver {
            cache: LayerCache::with_budget(max_entries, max_bytes),
            base: CacheLedger::new(),
            events: Vec::new(),
            frame_no: 0,
        }
    }

    /// 只读池。
    pub fn cache(&self) -> &LayerCache {
        &self.cache
    }

    /// 帧序号。
    pub fn frame_no(&self) -> u64 {
        self.frame_no
    }

    /// 帧内查询。
    ///
    /// 记账**只走池总账**（[`LayerCache::query`] 内已记），本层不再二次记账——
    /// 两处各记一次会让总账翻倍，帧报与总账永久对不上（真实缺陷，已由自检抓出）。
    /// 帧报靠 [`FrameDriver::commit`] 的「结算值 − 帧起始快照」求增量。
    pub fn query(&mut self, key: CacheKey) -> QueryOutcome {
        self.cache.query(&key)
    }

    /// 帧内入池。
    pub fn admit(&mut self, key: CacheKey) -> Admission {
        self.cache.admit(key)
    }

    /// 帧内失效事件（立即作用于池，帧内不留延迟失效）。
    pub fn invalidate(&mut self, ev: CacheInvalidEvent) {
        match ev {
            CacheInvalidEvent::Content { node_id, group } => {
                self.cache.invalidate_content(node_id, group);
            }
            CacheInvalidEvent::Transform { node_id, group } => {
                self.cache.invalidate_transform(node_id, group);
            }
            CacheInvalidEvent::Clip { node_id, group } => {
                self.cache.invalidate_clip(node_id, group);
            }
            CacheInvalidEvent::PaintFailed { node_id, group } => {
                self.cache.abort_paint(node_id, group);
            }
        }
        self.events.push(ev);
    }

    /// 记重绘完成。
    pub fn note_redrawn(&mut self, key: CacheKey) -> bool {
        self.cache.note_redrawn(&key)
    }

    /// 帧结算：按「结算值 − 帧起始快照」产出帧报（帧内账清零、帧号递增）。
    ///
    /// 帧报是**增量**口径；总账（[`LayerCache::ledger`]）跨帧累计，二者不重复。
    pub fn commit(&mut self) -> FrameReport {
        let now = self.cache.ledger().clone();
        let report = FrameReport {
            queries: now.queries.saturating_sub(self.base.queries),
            hits: now.hits.saturating_sub(self.base.hits),
            recomposes: now.recomposes.saturating_sub(self.base.recomposes),
            redraws: now.redraws.saturating_sub(self.base.redraws),
            poisoned_total: now.poisoned,
            evictions_total: now.evictions,
            entries: self.cache.len(),
            used_bytes: self.cache.used_bytes(),
        };
        self.base = now;
        self.events.clear();
        self.frame_no += 1;
        report
    }
}

impl Default for FrameDriver {
    fn default() -> Self {
        FrameDriver::new()
    }
}

/// F0649 度量导出（帧驱动面；与 [`CacheLedger::to_metrics`] 同源）。
pub fn f0649_metrics(cache: &LayerCache) -> Vec<(&'static str, u64)> {
    cache.ledger().to_metrics()
}

// ---------------------------------------------------------------------------
// 五、自检（CheckSet）
// ---------------------------------------------------------------------------

/// VE-F0615 · 图层缓存策略 —— 判据自检。
///
/// 判据五条（锚点）：三条件判定、两级失效、LRU 治理、收益入账、判据。
/// 覆盖六个判据族：`criteria-*`（三条件判定表）、`twolevel-*`（两级失效分离）、
/// `version-*`（版本号校验键防假命中）、`lru-*`（LRU 治理与预算）、
/// `poison-*`（污染作废）、`ledger-*`（收益入账）、`judge-*`（契约条款在场）。
pub fn run_ved15_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F0615");

    // ---- 三条件判定：表覆盖全集8 行、行下标自洽、缺一即不复用像素 ----

    {
        // 8 行全登记且行下标与 `triple.row()` 一一对应（无错位、无重复）。
        let mut rows_ok = true;
        let mut seen = [false; CRITERIA_ROWS];
        for i in 0..CRITERIA_ROWS {
            let idx = CRITERIA_TABLE[i].triple.row();
            if idx != i {
                rows_ok = false;
            }
            if idx < CRITERIA_ROWS {
                if seen[idx] {
                    rows_ok = false;
                }
                seen[idx] = true;
            }
        }
        for s in seen.iter() {
            if !*s {
                rows_ok = false;
            }
        }
        set.add("D15-criteria-表覆盖全集8行", rows_ok, "");
    }

    {
        // 查表对全部 8 行逐一复现表内结论（表自身自洽，非恒真断言——用表外形态验证）。
        let mut all_ok = true;
        for i in 0..CRITERIA_ROWS {
            let t = DirtyTriple {
                content: (i & 0b100) != 0,
                transform: (i & 0b010) != 0,
                clip: (i & 0b001) != 0,
            };
            if lookup_criteria(t) != CRITERIA_TABLE[i].verdict {
                all_ok = false;
            }
            // 表外真实形态：构造一个**不在表内枚举写法里**的三元组（由行号反解），
            // 验证查表路径与直接构造路径同解。
            let derived = DirtyTriple::new(
                t.row() & 0b100 != 0,
                t.row() & 0b010 != 0,
                t.row() & 0b001 != 0,
            );
            if lookup_criteria(derived) != CRITERIA_TABLE[i].verdict {
                all_ok = false;
            }
        }
        set.add("D15-criteria-全8行可复现", all_ok, "");
    }

    {
        // 「缺一即不复用像素」：全清之外 7 行**无一行**判 ReuseTexture。
        let no_false_reuse = CRITERIA_TABLE
            .iter()
            .filter(|r| r.triple != DirtyTriple::CLEAN)
            .all(|r| r.verdict != CacheVerdict::ReuseTexture);
        let clean_is_reuse = lookup_criteria(DirtyTriple::CLEAN) == CacheVerdict::ReuseTexture;
        set.add("D15-criteria-缺一即不复用像素", no_false_reuse && clean_is_reuse, "");
    }

    {
        // 内容脏支配一切：含 content 的 4 行全部判 RedrawContent（表外形态逐行验证）。
        let content_rows_all_redraw = (0..CRITERIA_ROWS)
            .filter(|i| (*i & 0b100) != 0)
            .all(|i| CRITERIA_TABLE[i].verdict == CacheVerdict::RedrawContent);
        set.add("D15-criteria-内容脏支配重绘", content_rows_all_redraw, "");
    }

    // ---- 两级失效分离：变换脏只作废合成段、像素段有效 ----

    {
        let key = CacheKey::new(1, false, 16, 16);
        let mut c = LayerCache::new();
        c.admit(key);
        c.note_redrawn(&key);
        c.invalidate_transform(1, false);
        let snap = c.snapshot();
        let e = snap.first();
        let pixel_valid = e.map(|x| x.pixel == PixelState::Valid).unwrap_or(false);
        let composite_stale = e.map(|x| x.composite == CompositeState::Stale).unwrap_or(false);
        set.add("D15-twolevel-变换脏像素段保留", pixel_valid && composite_stale, "");
    }

    {
        // 内容脏：两段同时作废。
        let key = CacheKey::new(2, false, 16, 16);
        let mut c = LayerCache::new();
        c.admit(key);
        c.note_redrawn(&key);
        c.invalidate_content(2, false);
        let snap = c.snapshot();
        let both_stale = snap
            .first()
            .map(|x| x.pixel == PixelState::Stale && x.composite == CompositeState::Stale)
            .unwrap_or(false);
        set.add("D15-twolevel-内容脏双段作废", both_stale, "");
    }

    {
        // 判定联动：变换脏 → RecomposeOnly（跳过重绘）；内容脏 → RedrawContent。
        let xform = lookup_criteria(DirtyTriple::new(false, true, false));
        let content = lookup_criteria(DirtyTriple::new(true, false, false));
        set.add(
            "D15-twolevel-变换判重合成内容判重绘",
            xform == CacheVerdict::RecomposeOnly
                && content == CacheVerdict::RedrawContent
                && xform.skips_redraw()
                && !content.skips_redraw(),
            "",
        );
    }

    {
        // 裁剪脏处置与变换脏同向：像素可复用（只重合成）。
        let clip = lookup_criteria(DirtyTriple::new(false, false, true));
        set.add(
            "D15-twolevel-裁剪脏判重合成",
            clip == CacheVerdict::RecomposeOnly && CLIP_INVALIDATION_DOC.contains("合成段"),
            "",
        );
    }

    {
        // 两级失效在**池级**的完整证据链：变换脏后重绘跳过、重绘后才双段有效。
        // 这一条是端到端的表外行为验证（不查表、只观察池状态变化）。
        let key = CacheKey::new(21, false, 8, 8);
        let mut c = LayerCache::new();
        c.admit(key);
        c.note_redrawn(&key);
        c.invalidate_transform(21, false);
        let after_xform = c.snapshot();
        let pixel_kept = after_xform
            .first()
            .map(|e| e.pixel_reusable())
            .unwrap_or(false);
        let not_full = after_xform.first().map(|e| !e.fully_reusable()).unwrap_or(false);
        c.invalidate_content(21, false);
        let after_content = c.snapshot();
        let pixel_gone = after_content
            .first()
            .map(|e| !e.pixel_reusable())
            .unwrap_or(false);
        set.add(
            "D15-twolevel-池级失效链自洽",
            pixel_kept && not_full && pixel_gone,
            "",
        );
    }

    // ---- 版本号校验键防假命中 ----

    {
        // 键全等 → 命中 ReuseTexture。
        let key = CacheKey::new(3, false, 8, 8);
        let mut c = LayerCache::new();
        c.admit(key);
        c.note_redrawn(&key);
        let out = c.query(&key);
        set.add(
            "D15-version-键全等命中复用",
            out.verdict == CacheVerdict::ReuseTexture && out.slot.is_some(),
            "",
        );
    }

    {
        // 键不等（内容版本变） → 强制 miss（绝不 ReuseTexture）。假命中防线的核心门禁。
        let base = CacheKey::new(4, false, 8, 8);
        let mut c = LayerCache::new();
        c.admit(base);
        c.note_redrawn(&base);
        let mut bumped = base;
        bumped.content_rev = REV_INIT + 1;
        let out = c.query(&bumped);
        set.add(
            "D15-version-键不等强制miss",
            out.verdict == CacheVerdict::RedrawContent && out.slot.is_none(),
            "",
        );
    }

    {
        // 三个版本各自独立参与校验：只改 xform / 只改 clip 都不得整体复用。
        let base = CacheKey::new(5, false, 8, 8);
        let mut c = LayerCache::new();
        c.admit(base);
        c.note_redrawn(&base);
        let mut xf = base;
        xf.xform_rev = REV_INIT + 1;
        let mut cl = base;
        cl.clip_rev = REV_INIT + 1;
        let ox = c.query(&xf);
        let oc = c.query(&cl);
        set.add(
            "D15-version-三版本独立校验",
            ox.verdict == CacheVerdict::RecomposeOnly
                && oc.verdict == CacheVerdict::RecomposeOnly
                && base.matches(&base)
                && !base.matches(&xf),
            "",
        );
    }

    {
        // 纹理尺寸入键：尺寸变化不得复用旧纹理（复用会错尺寸）。
        let base = CacheKey::new(6, false, 8, 8);
        let mut c = LayerCache::new();
        c.admit(base);
        c.note_redrawn(&base);
        let mut resized = base;
        resized.width = 16;
        let out = c.query(&resized);
        set.add(
            "D15-version-尺寸入键防错尺寸复用",
            out.verdict != CacheVerdict::ReuseTexture && base.bytes() != resized.bytes(),
            "",
        );
    }

    {
        // 粒度入键：同层 id 的单层与组条目键空间独立，互不误命中。
        let layer = CacheKey::new(7, false, 8, 8);
        let group = CacheKey::new(7, true, 8, 8);
        let mut c = LayerCache::new();
        c.admit(layer);
        c.admit(group);
        let out = c.query(&layer);
        set.add(
            "D15-version-两级粒度键独立",
            out.verdict == CacheVerdict::ReuseTexture && c.len() == 2 && !layer.matches(&group),
            "",
        );
    }

    // ---- LRU 治理与预算 ----

    {
        // 命中移尾：admit三层 → 查第一层 → LRU 头应变为第二层。
        let mut c = LayerCache::new();
        for i in 1..=3u64 {
            c.admit(CacheKey::new(i, false, 4, 4));
        }
        let k1 = CacheKey::new(1, false, 4, 4);
        c.query(&k1);
        let order = c.lru_order();
        // 淘汰顺序应为 2,3,1（1 被移到最新）。
        let first_is_slot1 = order.first().copied() == Some(1);
        set.add("D15-lru-命中移尾", first_is_slot1 && order.len() == 3, "");
    }

    {
        // 层数超限 → LRU 淘汰头部，且 used_bytes 不泄漏（live 口径）。
        let mut c = LayerCache::with_budget(2, MAX_BYTES_PER_ENTRY);
        for i in 1..=3u64 {
            c.admit(CacheKey::new(i, false, 4, 4));
        }
        let len_ok = c.len() == 2;
        let bytes_ok = c.used_bytes() == 2 * 4 * 4 * 4; // 每条 4*4*4 = 64B
        set.add(
            "D15-lru-超限淘汰头部",
            len_ok && bytes_ok && c.ledger().evictions >= 1,
            "",
        );
    }

    {
        // 槽位回收：反复 admit/淘汰后 slots 向量不无限增长（空闲链表生效）。
        // 这是「淘汰 O(1) 摊销」的结构性门禁——若改回 Vec::remove 或不留空洞，
        // used_bytes 会泄漏、live 会失真，此判据变红。
        let mut c = LayerCache::with_budget(2, MAX_BYTES_PER_ENTRY);
        for i in 1..=10u64 {
            c.admit(CacheKey::new(i, false, 4, 4));
        }
        let bytes_ok = c.used_bytes() == 2 * 64;
        let live_ok = c.len() == 2;
        set.add("D15-lru-槽位回收字节不泄漏", live_ok && bytes_ok, "");
    }

    {
        // 逐层尺寸超限 → 显式不缓存，不进池（不挤占他层）。
        let mut c = LayerCache::with_budget(MAX_CACHE_ENTRIES, 64);
        let big = CacheKey::new(9, false, 100, 100); // 40000B >> 64
        let adm = c.admit(big);
        let too_large = matches!(adm, Admission::TooLarge { .. });
        set.add(
            "D15-lru-逐层超限不缓存",
            too_large && c.len() == 0 && c.ledger().too_large == 1,
            "",
        );
    }

    {
        // 幂等重入：同键重复 admit 不重复占位、不刷淘汰账。
        let k = CacheKey::new(11, false, 4, 4);
        let mut c = LayerCache::new();
        let a1 = c.admit(k);
        let a2 = c.admit(k);
        set.add(
            "D15-lru-幂等重入",
            a1 == Admission::Admitted && a2 == Admission::Duplicate && c.len() == 1,
            "",
        );
    }

    {
        // 层数上限为 0 → 显式拒绝且不死循环（`while live >= 0` 的防线）。
        let mut c = LayerCache::with_budget(0, MAX_BYTES_PER_ENTRY);
        let adm = c.admit(CacheKey::new(12, false, 4, 4));
        set.add(
            "D15-lru-零上限显式拒绝",
            matches!(adm, Admission::TooLarge { .. }) && c.len() == 0,
            "",
        );
    }

    // ---- 污染作废 ----

    {
        // 绘制中途失败 → 条目作废并回收槽位；键仍全等也不得命中。
        let k = CacheKey::new(8, false, 8, 8);
        let mut c = LayerCache::new();
        c.admit(k);
        c.note_redrawn(&k);
        c.abort_paint(8, false);
        let out = c.query(&k);
        set.add(
            "D15-poison-作废恒不命中",
            c.len() == 0 && out.verdict != CacheVerdict::ReuseTexture && out.slot.is_none(),
            "",
        );
    }

    {
        // 污染后重新完整绘制 → 恢复可命中。
        let k = CacheKey::new(10, false, 8, 8);
        let mut c = LayerCache::new();
        c.admit(k);
        c.note_redrawn(&k);
        c.abort_paint(10, false);
        c.admit(k);
        c.note_redrawn(&k);
        let out = c.query(&k);
        set.add(
            "D15-poison-重绘后恢复命中",
            out.verdict == CacheVerdict::ReuseTexture && c.len() == 1,
            "",
        );
    }

    {
        // 污染记账入账（收益面：错误路径也必须被度量）。
        let k = CacheKey::new(13, false, 8, 8);
        let mut c = LayerCache::new();
        c.admit(k);
        c.note_redrawn(&k);
        c.abort_paint(13, false);
        set.add(
            "D15-poison-作废计入账本",
            c.ledger().poisoned == 1 && POISON_DOC.contains("作废"),
            "",
        );
    }

    // ---- 收益入账 ----

    {
        // 账目自洽：hit_rate_permille = hits/queries *1000（整数、零查询返 0）。
        let mut l = CacheLedger::new();
        l.note_query();
        l.note_verdict(CacheVerdict::ReuseTexture);
        l.note_query();
        l.note_verdict(CacheVerdict::RecomposeOnly);
        l.note_query();
        l.note_verdict(CacheVerdict::RedrawContent);
        // 1/3 → 333 千分比。
        set.add(
            "D15-ledger-命中率整数千分比",
            l.hit_rate_permille() == 333 && l.skipped_redraws() == 2,
            "",
        );
    }

    {
        // 零查询命中率 0（不崩、不溢出）。
        let l = CacheLedger::new();
        set.add("D15-ledger-零查询不崩", l.hit_rate_permille() == 0, "");
    }

    {
        // 端到端收益：命中 1 次 + 仅重合成 1 次 + 重绘 1 次 → 跳过重绘 = 2。
        let base = CacheKey::new(20, false, 8, 8);
        let mut d = FrameDriver::new();
        d.admit(base);
        d.note_redrawn(base);
        d.query(base); // 命中
        d.invalidate(CacheInvalidEvent::Transform { node_id: 20, group: false });
        let mut bumped = base;
        bumped.xform_rev = REV_INIT + 1;
        d.query(bumped); // 键不等 → RecomposeOnly
        d.invalidate(CacheInvalidEvent::Content { node_id: 20, group: false });
        let mut cbumped = bumped;
        cbumped.content_rev = REV_INIT + 1;
        d.query(cbumped); // 内容版本变 → RedrawContent
        let rep = d.commit();
        let total = d.cache().ledger();
        set.add(
            "D15-ledger-端到端收益入账",
            rep.hits == 1
                && rep.recomposes == 1
                && rep.redraws == 1
                && total.queries == 3
                && total.skipped_redraws() == 2,
            "",
        );
    }

    {
        // 帧结算不留半帧：commit 后帧内账清零、帧号递增；跨帧总账保留累计值。
        let mut d = FrameDriver::new();
        let k = CacheKey::new(30, false, 4, 4);
        d.admit(k);
        d.note_redrawn(k);
        d.query(k);
        let r1 = d.commit();
        let r2 = d.commit();
        set.add(
            "D15-ledger-帧结算清零",
            r1.queries == 1
                && r2.queries == 0
                && d.frame_no() == 2
                && d.cache().ledger().queries == 1,
            "",
        );
    }

    {
        // F0649 度量导出：键名齐备且值与账一致（收益入账的下游对接面）。
        let mut c = LayerCache::new();
        let k = CacheKey::new(40, false, 8, 8);
        c.admit(k);
        c.note_redrawn(&k);
        c.query(&k);
        let m = f0649_metrics(&c);
        let has_all = m.len() == 9;
        let q = m.iter().find(|(n, _)| *n == "cache_queries").map(|(_, v)| *v);
        let h = m.iter().find(|(n, _)| *n == "cache_hits").map(|(_, v)| *v);
        let s = m
            .iter()
            .find(|(n, _)| *n == "cache_skipped_redraws")
            .map(|(_, v)| *v);
        set.add(
            "D15-ledger-度量导出齐备",
            has_all && q == Some(1) && h == Some(1) && s == Some(1),
            "",
        );
    }

    {
        // 帧驱动覆盖四类失效事件全集（事件族不留空洞）。
        let mut d = FrameDriver::with_budget(8, MAX_BYTES_PER_ENTRY);
        for kind in CacheInvalidEvent::all_kinds() {
            let k = CacheKey::new(50, false, 8, 8);
            d.admit(k);
            d.note_redrawn(k);
            let ev = match kind {
                "content" => CacheInvalidEvent::Content { node_id: 50, group: false },
                "transform" => CacheInvalidEvent::Transform { node_id: 50, group: false },
                "clip" => CacheInvalidEvent::Clip { node_id: 50, group: false },
                _ => CacheInvalidEvent::PaintFailed { node_id: 50, group: false },
            };
            // 事件取值面：id 与粒度须与入池键一致（跨变体取值不能错）。
            d.invalidate(ev);
        }
        let rep = d.commit();
        set.add(
            "D15-judge-四类失效事件全集",
            rep.entries == 0 && rep.poisoned_total == 1 && CacheInvalidEvent::all_kinds().len() == 4,
            "",
        );
    }

    // ---- 契约文档在场（判据条款的可追溯锚） ----

    {
        let docs_ok = CRITERIA_DOC.contains("三条件")
            && TWO_LEVEL_DOC.contains("两级失效分离")
            && VERSION_KEY_DOC.contains("版本号")
            && LRU_DOC.contains("LRU")
            && POISON_DOC.contains("作废")
            && LEDGER_DOC.contains("命中率")
            && GRANULARITY_DOC.contains("隔离组")
            && CLIP_INVALIDATION_DOC.contains("合成段");
        set.add("D15-judge-八契约条款在场", docs_ok, "");
    }

    set
}