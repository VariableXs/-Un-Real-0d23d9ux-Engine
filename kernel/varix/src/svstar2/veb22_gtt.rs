//! VE-F0222 · Intel 显存管理对接（GTT）（VE-B 域 · Intel 核显组 · 目标 440 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0222`
//!
//! **判据（锚点原文五条）**：GGTT 语义、批写回滚、LRU 驱逐、引用计数、判据。
//!
//! GTT 是 CPU 侧地址到显卡物理地址（GPA）的翻译层。Intel 把它拆成两张表：
//! **GGTT**（全局，内核与物理寻址模式用）与**每进程 GTT（ppGTT）**（各 VM/
//! 各上下文私有）。两者是**两个独立地址空间**——把 ppGTT 条目写进 GGTT，
//! 设备会按全局语义翻译出一个错误的物理地址，表现为越权读写而非报错。
//! 故本单的表实例**按 kind 分池**，任何跨表操作一律拒绝。
//!
//! # 头注要点（每条都是判据的反面，写在这里供判据引用）
//!
//! ## 要点一：页表批写的「整批回滚」由**先验证后落笔**结构性保证
//!
//! 「批写失败→整批回滚」若做成「逐条写、坏了再撤」，撤回路径自己也可能
//! 失败（写 PTE 中途断电语义在软件模型里不存在，但**部分应用**的中间态
//! 会被下游 F0224 读到）。故本单把批写做成**两段式**：stage 阶段逐条
//! 验证（对齐/范围/与现役绑定不重叠），任一条非法 ⇒ `Err` 且 **PTE 写
//! 计数不变、页表逐字节未动**——「回滚」不需要执行，因为**从未半提交**。
//! 这是比 journal 回滚更强的保证，且可判据化（非法批 ⇒ 零写入）。
//!
//! ## 要点二：aperture 耗尽的 LRU 驱逐**只逐未被引用的绑定**
//!
//! refcount>0 的绑定正被 GPU 流水引用，逐掉就是 use-after-free；
//! fence 未签的 pending 释放项占用还不属于可分配空间，同样不可逐。
//! 故驱逐候选 = 同表内 refcount==0 且非 pending 的绑定，按 `last_use`
//! 最旧优先。**驱逐重试有上限**（[`EVICT_RETRY_MAX`]）：上限内凑不齐
//! 空间 ⇒ `ApertureExhausted` 明确失败，不无限驱逐（无限驱逐会把
//! 长生命周期绑定全清光，退化为「每次分配都全表驱逐」）。
//!
//! ## 要点三：驱逐决策**限频**——O(候选数) 的扫描不能每笔分配都跑
//!
//! 驱逐扫描是 O(候选数)，若不加限频，aperture 高水位时每笔分配都触发
//! 全表扫描 ⇒ 抖动。故驱逐决策带**冷却窗口**（[`EVICT_COOLDOWN_TICKS`]）：
//! 距上次驱逐扫描不足冷却 tick 数的请求直接按「无可逐」处理进入下一轮
//! 重试或失败。限频本身入账（`evict_deferred`），可判据。
//!
//! ## 要点四：引用计数是**全生命周期契约**，不是参考信息
//!
//! `acquire`（A 域分配池/映射/围栏三契约的引用点）+1、`release_ref` -1；
//! **refcount>0 时 unbind 拒绝**（正被引用的映射解绑 = 下游 F0224 提交
//! 引用悬空）；refcount 溢出用 `checked_add` 拒绝而非回绕（回绕后
//! refcount 变 0，绑定会在下一次驱逐中被逐掉——这是最隐蔽的 UAF）。
//!
//! ## 要点五：大页 4K/2M 分级，**显式要求与自动选择语义不同**
//!
//! `Auto`：对齐且代际支持 ⇒ 2M（PTE 数 ÷512）；不对齐 ⇒ 降 4K 且
//! **降级可观测**（绑定记录实际页类）。`Force2M`：不对齐或代际不支持
//! ⇒ **专属错误码拒绝**，不静默降级——显式要求被静默降级，调用方会按
//! 2M 的 PTE 预算排布下游结构，实际拿到 4K 时预算爆掉。代际门控与
//! F0221 的 [`GenTier`] 对接：基线档 4K only，Xe 档起支持 2M。
//!
//! ## 要点六：unbind 的空间回收**等 fence**
//!
//! 解绑时 PTE **立即清除**（设备不得再翻译到这批页），但 aperture 区间
//! 归入 pending，**fence 签到后才真正可再分配**——GPU 流水可能还在读
//! 旧页内容，区间提前复用就是数据踩踏。这是 A 域围栏契约在 GTT 侧的
//! 兑现点，`fence_flush(signaled)` 由调用方注入签到谓词。

// ---------------------------------------------------------------------------
// 导入（no_std 三件套 + format）
// ---------------------------------------------------------------------------

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::veb21_ident::GenTier;

// ---------------------------------------------------------------------------
// 常量与页几何
// ---------------------------------------------------------------------------

/// 4K 页（字节）。
pub const PAGE_4K: u64 = 4096;
/// 2M 大页（字节）。
pub const PAGE_2M: u64 = 2 * 1024 * 1024;
/// 每 2M 大页覆盖的 4K 页数（512）。
pub const PAGES_PER_2M: u64 = PAGE_2M / PAGE_4K;

/// aperture 大小（GGTT 与 ppGTT **各自**独立 256 MiB 窗口）。
pub const APERTURE_SIZE: u64 = 256 * 1024 * 1024;

/// 单次分配上限（64 MiB）——防一笔绑定吃光整窗，给驱逐留意义。
pub const MAX_ALLOC_BYTES: u64 = 64 * 1024 * 1024;

/// 绑定槽位上限（O(1) 查找由槽位直接下标承担）。
pub const MAX_BINDINGS: usize = 128;

/// pending 释放队列上限（超限 ⇒ 驱逐与解绑照常，队列满时最旧强收——
/// **如实记账**，不静默丢弃）。
pub const MAX_PENDING: usize = 64;

/// 驱逐重试上限（要点二）。
pub const EVICT_RETRY_MAX: u32 = 3;

/// 驱逐决策冷却窗口（tick 数，要点三）。
pub const EVICT_COOLDOWN_TICKS: u64 = 8;

// ---------------------------------------------------------------------------
// 诊断码（新域独占码段 0x2Fxx；下游 F0229 错误解析按位消费）
// ---------------------------------------------------------------------------

/// GTT 域错误（**自建诊断码**，不扩下游封闭枚举）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GErr {
    /// 分配池句柄非法（base 非 4K 对齐或越过池契约）。
    BadPoolBase,
    /// 长度为 0。
    ZeroLength,
    /// 单笔分配超上限。
    TooLarge,
    /// aperture 耗尽（驱逐重试上限内凑不齐空间）。
    ApertureExhausted,
    /// 绑定区间冲突（拒绝并三要素，见 [`ConflictInfo`]）。
    BindConflict,
    /// 目标绑定仍被引用（refcount>0）。
    StillReferenced,
    /// 引用计数回绕拒绝。
    RefcountOverflow,
    /// 批写含非法条目（整批拒绝，零写入）。
    BatchInvalidEntry,
    /// Force2M 但区间不对齐。
    LargePageMisaligned,
    /// 代际不支持 2M 大页（F0221 基线档）。
    LargePageUnsupported,
    /// 跨表操作（GGTT/ppGTT 语义互斥）。
    TableMismatch,
    /// 引用计数不越零（对 refcount==0 的绑定再 release）。
    RefcountUnderflow,
    /// 目标不存在（票号越界或已释放）。
    NotFound,
}

/// 冲突三要素（锚点「绑定冲突→拒绝并三要素」）：
/// 想要的区间、已占的区间、占用者票号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConflictInfo {
    /// 想要的 aperture 偏移。
    pub want_off: u64,
    /// 想要的字节数。
    pub want_len: u64,
    /// 已占者的 aperture 偏移。
    pub have_off: u64,
    /// 已占者的字节数。
    pub have_len: u64,
    /// 已占者的绑定票号。
    pub have_id: u32,
}

impl GErr {
    pub const fn code(self) -> u32 {
        match self {
            GErr::BadPoolBase => 0x2F01,
            GErr::ZeroLength => 0x2F02,
            GErr::TooLarge => 0x2F03,
            GErr::ApertureExhausted => 0x2F04,
            GErr::BindConflict => 0x2F05,
            GErr::StillReferenced => 0x2F06,
            GErr::RefcountOverflow => 0x2F07,
            GErr::BatchInvalidEntry => 0x2F08,
            GErr::LargePageMisaligned => 0x2F09,
            GErr::LargePageUnsupported => 0x2F0A,
            GErr::TableMismatch => 0x2F0B,
            GErr::RefcountUnderflow => 0x2F0C,
            GErr::NotFound => 0x2F0D,
        }
    }
    /// 人类可读说明（拒绝必带专属原因，不可共用占位串）。
    pub fn reason(self) -> String {
        match self {
            GErr::BadPoolBase => String::from("分配池基址非 4K 对齐"),
            GErr::ZeroLength => String::from("绑定长度为 0"),
            GErr::TooLarge => String::from("单笔分配超上限"),
            GErr::ApertureExhausted => String::from("aperture 耗尽且驱逐重试已达上限"),
            GErr::BindConflict => String::from("绑定区间与现役绑定冲突"),
            GErr::StillReferenced => String::from("目标绑定仍被引用不可解绑"),
            GErr::RefcountOverflow => String::from("引用计数将回绕已拒绝"),
            GErr::BatchInvalidEntry => String::from("页表批写含非法条目整批拒绝"),
            GErr::LargePageMisaligned => String::from("强制 2M 大页但区间不对齐"),
            GErr::LargePageUnsupported => String::from("当前代际不支持 2M 大页"),
            GErr::TableMismatch => String::from("跨表操作被拒绝"),
            GErr::RefcountUnderflow => String::from("引用计数不越零（对零引用再释放）"),
            GErr::NotFound => String::from("绑定票号不存在"),
        }
    }
    /// 全集（供判据遍历互异性）。
    pub const ALL: [GErr; 13] = [
        GErr::BadPoolBase,
        GErr::ZeroLength,
        GErr::TooLarge,
        GErr::ApertureExhausted,
        GErr::BindConflict,
        GErr::StillReferenced,
        GErr::RefcountOverflow,
        GErr::BatchInvalidEntry,
        GErr::LargePageMisaligned,
        GErr::LargePageUnsupported,
        GErr::TableMismatch,
        GErr::RefcountUnderflow,
        GErr::NotFound,
    ];
}

// ---------------------------------------------------------------------------
// 表种类与页类
// ---------------------------------------------------------------------------

/// GTT 表种类（**两个独立地址空间**）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TableKind {
    /// 全局 GTT。
    Ggtt,
    /// 每进程 GTT。
    PpGtt,
}

impl TableKind {
    pub const fn index(self) -> usize {
        match self {
            TableKind::Ggtt => 0,
            TableKind::PpGtt => 1,
        }
    }
    /// 线上编码（显式映射，禁 `enum as u8`）。
    pub const fn code(self) -> u8 {
        match self {
            TableKind::Ggtt => 0,
            TableKind::PpGtt => 1,
        }
    }
    pub const ALL: [TableKind; 2] = [TableKind::Ggtt, TableKind::PpGtt];
}

/// 页类（4K/2M 分级）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PageClass {
    /// 4K 页。
    P4k,
    /// 2M 大页。
    P2m,
}

impl PageClass {
    /// 页大小（字节）。
    pub const fn bytes(self) -> u64 {
        match self {
            PageClass::P4k => PAGE_4K,
            PageClass::P2m => PAGE_2M,
        }
    }
    /// 页内偏移掩码（addr & mask == 0 即按该页对齐）。
    pub const fn align_mask(self) -> u64 {
        self.bytes() - 1
    }
    pub const fn label(self) -> &'static str {
        match self {
            PageClass::P4k => "4K",
            PageClass::P2m => "2M",
        }
    }
    pub const ALL: [PageClass; 2] = [PageClass::P4k, PageClass::P2m];
}

/// 页策略（要点五：显式要求与自动选择语义不同）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PagePolicy {
    /// 自动：对齐且支持 ⇒ 2M，否则降 4K（降级可观测）。
    Auto,
    /// 强制 4K。
    Force4k,
    /// 强制 2M（不满足即专属错误码拒绝，不静默降级）。
    Force2m,
}

// ---------------------------------------------------------------------------
// PTE 编码与批写
// ---------------------------------------------------------------------------

/// PTE 位约定：bit0=present，bit1=page size（1=2M），bit12 起为 GPA 高位。
/// **显式编码函数**（禁散落的位算术，判据逐位对拍）。
pub const fn pte_encode(gpa: u64, page: PageClass, present: bool) -> u64 {
    let mut v = gpa & !0xFFF;
    if present {
        v |= 1;
    }
    if matches!(page, PageClass::P2m) {
        v |= 2;
    }
    v
}

/// PTE 解码自检用：present 位。
pub const fn pte_present(v: u64) -> bool {
    v & 1 != 0
}
/// PTE 解码自检用：大页位。
pub const fn pte_is_2m(v: u64) -> bool {
    v & 2 != 0
}
/// PTE 解码自检用：GPA（剥标志位）。
pub const fn pte_gpa(v: u64) -> u64 {
    v & !0xFFF
}

/// 页表条目批写缓冲（锚点数据结构一）。
///
/// 批**携带其目标表**：构造时钉死 kind，向另一张表提交即
/// `TableMismatch`（跨表批写 = 把 ppGTT 条目喂给 GGTT 的语义事故，
/// 在提交口拦住而不是靠调用方自觉）。
/// 两段式：本结构只收条目，`Gtt::pte_batch_commit` 先验证后落笔。
/// **任一条非法 ⇒ 整批拒绝**（要点一：先验证后落笔 ⇒ 结构性整批回滚）。
pub struct PteBatch {
    table: TableKind,
    entries: Vec<(u64, u64)>, // (PTE 下标, 编码值)
}

impl PteBatch {
    /// 为指定表新建批。
    pub fn for_table(table: TableKind) -> PteBatch {
        PteBatch { table, entries: Vec::new() }
    }
    /// 批的目标表。
    pub fn table(&self) -> TableKind {
        self.table
    }
    /// 批内条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// 追加一条（暂存，不落笔）。
    pub fn push(&mut self, pte_index: u64, value: u64) {
        self.entries.push((pte_index, value));
    }
    /// 遍历条目（判据侧独立对拍用；顺序即写入序）。
    pub fn iter(&self) -> alloc::vec::IntoIter<(u64, u64)> {
        self.entries.clone().into_iter()
    }
}

// ---------------------------------------------------------------------------
// 代际能力对接（上游 F0221）
// ---------------------------------------------------------------------------

/// GTT 能力（由 [`GenTier`] 推导：基线档 4K only，Xe 档起支持 2M）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GttCaps {
    /// 支持 2M 大页。
    pub large_page: bool,
}

impl GttCaps {
    /// 由 F0221 分型档推导（**集成点**：分型错了，这里就会拿错页几何）。
    pub const fn for_tier(tier: GenTier) -> GttCaps {
        match tier {
            GenTier::Baseline => GttCaps { large_page: false },
            GenTier::XeStandard | GenTier::XeLatest => GttCaps { large_page: true },
        }
    }
}

// ---------------------------------------------------------------------------
// 绑定与票据
// ---------------------------------------------------------------------------

/// 绑定记录（锚点数据结构二：vma×页数×引用计数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Binding {
    /// aperture 偏移（vma，本表地址空间内）。
    pub vma: u64,
    /// 字节数。
    pub len: u64,
    /// 4K 页数（**记账口径恒为 4K 页**，与实际页类无关——大页只是
    /// PTE 表达压缩，记账用 4K 页数才能与字节账对得上）。
    pub pages_4k: u64,
    /// 实际页类（Auto 降级时这里与请求不同 ⇒ 降级可观测）。
    pub page: PageClass,
    /// 引用计数。
    pub refcount: u32,
    /// 所属表。
    pub table: TableKind,
    /// 解绑时记录的 fence 序号（active 态无意义，填 0）。
    pub fence_seq: u64,
    /// 最后使用 tick（LRU 序）。
    pub last_use: u64,
    /// 池基址（GPA 起点）。
    pub pool_base: u64,
}

/// 绑定票号（O(1) 查找 = 槽位下标直取）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BindTicket {
    pub raw: u32,
}

/// pending 释放项（fence 签到前占着 aperture 区间）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PendingRelease {
    pub vma: u64,
    pub len: u64,
    pub pages_4k: u64,
    pub table: TableKind,
    pub fence_seq: u64,
}

/// 运行账本（全部聚合计数，判据侧独立重算对拍）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GttStats {
    /// 成功 bind 次数。
    pub binds: u32,
    /// 成功 unbind（进 pending）次数。
    pub unbinds: u32,
    /// 驱逐的绑定数。
    pub evicted: u32,
    /// 驱逐扫描次数（每次重试算一次）。
    pub evict_sweeps: u32,
    /// 被冷却窗口 defer 的驱逐决策数。
    pub evict_deferred: u32,
    /// 批写提交次数。
    pub batch_commits: u32,
    /// 批写整批拒绝次数。
    pub batch_rejected: u32,
    /// PTE 写入次数（成功落笔才计）。
    pub pte_writes: u64,
    /// PTE 清除次数（unbind/evict）。
    pub pte_clears: u64,
    /// Auto 策略降级 4K 的次数。
    pub large_downgrades: u32,
    /// fence 签到后释放的 pending 数。
    pub fence_freed: u32,
    /// pending 队列满被迫强收的次数。
    pub pending_forced: u32,
}

impl GttStats {
    pub const fn zero() -> GttStats {
        GttStats {
            binds: 0,
            unbinds: 0,
            evicted: 0,
            evict_sweeps: 0,
            evict_deferred: 0,
            batch_commits: 0,
            batch_rejected: 0,
            pte_writes: 0,
            pte_clears: 0,
            large_downgrades: 0,
            fence_freed: 0,
            pending_forced: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// 单表 aperture 状态
// ---------------------------------------------------------------------------

/// 一张表的 aperture：PTE 阵列（区间占用以 slots 里的绑定为准，
/// pending 区间由 pending 队列承载——不在这里重复记账，避免双源漂移）。
struct Aperture {
    /// PTE 阵列（下标 = aperture 偏移 / 4K；**全表统一 4K 粒度存储**，
    /// 2M 大页表现为步长 512 的条目写入——判据可逐条对拍）。
    ptes: Vec<u64>,
}

impl Aperture {
    fn new() -> Aperture {
        let n = (APERTURE_SIZE / PAGE_4K) as usize;
        Aperture { ptes: vec![0u64; n] }
    }
    /// PTE 下标（偏移必须 4K 对齐且在窗内——调用方先验证）。
    fn pte_index(vma: u64) -> u64 {
        vma / PAGE_4K
    }
    /// 读 PTE。
    fn read_pte(&self, vma: u64) -> Option<u64> {
        let i = Self::pte_index(vma) as usize;
        if i < self.ptes.len() {
            Some(self.ptes[i])
        } else {
            None
        }
    }
    /// 写 PTE（调用方保证已验证）。
    fn write_pte(&mut self, vma: u64, value: u64) {
        let i = Self::pte_index(vma) as usize;
        if i < self.ptes.len() {
            self.ptes[i] = value;
        }
    }
}

// ---------------------------------------------------------------------------
// GTT 总控
// ---------------------------------------------------------------------------

/// Intel GTT 对接总控（一张实例 = 两个地址空间）。
pub struct Gtt {
    pub caps: GttCaps,
    tick: u64,
    pub stats: GttStats,
    /// 绑定槽位（O(1) 直取；`None` = 空槽）。
    slots: [Option<Binding>; MAX_BINDINGS],
    /// 两张表的 aperture。
    apertures: [Aperture; 2],
    /// pending 释放队列。
    pending: Vec<PendingRelease>,
    /// 最近一次冲突三要素（供拒绝方与判据读取）。
    pub last_conflict: Option<ConflictInfo>,
    /// 上次驱逐扫描的 tick（冷却窗口）。
    last_evict_tick: u64,
}

/// 单笔分配的页几何决策结果。
pub struct PageDecision {
    pub page: PageClass,
    pub downgraded: bool,
}

impl Gtt {
    /// 构造（能力由 F0221 分型档推导）。
    pub fn new(tier: GenTier) -> Gtt {
        Gtt {
            caps: GttCaps::for_tier(tier),
            tick: 0,
            stats: GttStats::zero(),
            slots: [None; MAX_BINDINGS],
            apertures: [Aperture::new(), Aperture::new()],
            pending: Vec::new(),
            last_conflict: None,
            last_evict_tick: 0,
        }
    }

    /// 当前 tick。
    pub fn now(&self) -> u64 {
        self.tick
    }

    /// 推进时钟（调用方每完成一轮操作推一格；LRU 序由 tick 承载）。
    pub fn advance(&mut self) {
        self.tick += 1;
    }

    /// 活跃绑定数（两表合计）。
    pub fn active_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < MAX_BINDINGS {
            if self.slots[i].is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 活跃 4K 页数（两表合计；**记账口径恒 4K 页**）。
    pub fn active_pages_4k(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < MAX_BINDINGS {
            if let Some(b) = self.slots[i] {
                n += b.pages_4k;
            }
            i += 1;
        }
        n
    }

    /// pending 项数。
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// O(1) 查找（票号 = 槽位下标；**不扫描**——判据断操作数恰为 1）。
    pub fn lookup(&self, t: BindTicket) -> Option<Binding> {
        if (t.raw as usize) < MAX_BINDINGS {
            self.slots[t.raw as usize]
        } else {
            None
        }
    }

    /// 页几何决策（要点五）。返回 Err(专属码) 表示显式要求不可满足。
    fn decide_page(&mut self, policy: PagePolicy, off: u64, len: u64) -> Result<PageDecision, GErr> {
        let aligned_2m = off & (PAGE_2M - 1) == 0 && len & (PAGE_2M - 1) == 0 && len > 0;
        match policy {
            PagePolicy::Force4k => Ok(PageDecision { page: PageClass::P4k, downgraded: false }),
            PagePolicy::Force2m => {
                if !self.caps.large_page {
                    Err(GErr::LargePageUnsupported)
                } else if !aligned_2m {
                    Err(GErr::LargePageMisaligned)
                } else {
                    Ok(PageDecision { page: PageClass::P2m, downgraded: false })
                }
            }
            PagePolicy::Auto => {
                if self.caps.large_page && aligned_2m {
                    Ok(PageDecision { page: PageClass::P2m, downgraded: false })
                } else {
                    if self.caps.large_page {
                        // 有能力但没对齐 ⇒ 降级可观测。
                        self.stats.large_downgrades += 1;
                    }
                    Ok(PageDecision { page: PageClass::P4k, downgraded: true })
                }
            }
        }
    }

    /// 在单表内找首个能放 [need_pages_4k) 的空闲偏移（first-fit，
    /// 以 active 绑定区间为准；pending 区间由调用方在候选判断时排除）。
    fn find_free(&self, table: TableKind, need_4k: u64, skip: u32) -> Option<u64> {
        let _ = skip;
        // 收集本表全部占用区间（active 绑定；pending 在传入前已剔出候选，
        // 但其区间**仍占用**——pending 项从 self.pending 里查同表同区间）。
        let mut used: Vec<(u64, u64)> = Vec::new();
        let mut i = 0usize;
        while i < MAX_BINDINGS {
            if let Some(b) = self.slots[i] {
                if b.table == table {
                    used.push((b.vma, b.len));
                }
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < self.pending.len() {
            let p = self.pending[j];
            if p.table == table {
                used.push((p.vma, p.len));
            }
            j += 1;
        }
        // first-fit：按偏移排序后找相邻占用之间的洞。
        used.sort();
        let need = need_4k * PAGE_4K;
        let mut cursor = 0u64;
        let mut k = 0usize;
        while k < used.len() {
            let (off, len) = used[k];
            if off >= cursor && off - cursor >= need {
                return Some(cursor);
            }
            // cursor 推进到本段末尾（checked：len>0 恒成立）。
            let end = off.checked_add(len)?;
            cursor = if end > cursor { end } else { cursor };
            k += 1;
        }
        if APERTURE_SIZE - cursor >= need {
            return Some(cursor);
        }
        None
    }

    /// 区间是否与现役绑定冲突（返回三要素）。
    fn find_conflict(&self, table: TableKind, off: u64, len: u64) -> Option<ConflictInfo> {
        let end = off + len; // 调用方已保证不溢出
        let mut i = 0usize;
        while i < MAX_BINDINGS {
            if let Some(b) = self.slots[i] {
                if b.table == table {
                    let b_end = b.vma + b.len;
                    if off < b_end && b.vma < end {
                        return Some(ConflictInfo {
                            want_off: off,
                            want_len: len,
                            have_off: b.vma,
                            have_len: b.len,
                            have_id: i as u32,
                        });
                    }
                }
            }
            i += 1;
        }
        None
    }

    /// 驱逐决策（要点二/三）：返回本次实际逐掉的票号列表。
    /// `need_4k`：还差多少 4K 页；只逐 refcount==0 的绑定。
    fn evict_sweep(&mut self, table: TableKind, need_4k: u64) -> Vec<u32> {
        // 冷却窗口（要点三）：距上次扫描不足冷却 tick ⇒ defer。
        if self.tick.saturating_sub(self.last_evict_tick) < EVICT_COOLDOWN_TICKS {
            self.stats.evict_deferred += 1;
            return Vec::new();
        }
        self.last_evict_tick = self.tick;
        self.stats.evict_sweeps += 1;
        // 候选：本表 refcount==0 且非 pending，按 last_use 最旧优先。
        // pending 判定：其区间在 pending 队列中（unbind 已进队）——
        // pending 的绑定本体已出槽，故槽内绑定天然非 pending。
        let mut cands: Vec<(u64, u32)> = Vec::new(); // (last_use, id)
        let mut freed_4k = 0u64;
        let mut i = 0usize;
        while i < MAX_BINDINGS {
            if let Some(b) = self.slots[i] {
                if b.table == table && b.refcount == 0 {
                    cands.push((b.last_use, i as u32));
                }
            }
            i += 1;
        }
        cands.sort();
        let mut out: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < cands.len() {
            // 凑够即停（O(候选数)，逐个判定）。
            if freed_4k >= need_4k {
                break;
            }
            let id = cands[k].1;
            if let Some(b) = self.slots[id as usize] {
                // 逐：清 PTE + 出槽。
                self.clear_ptes_of(b);
                freed_4k += b.pages_4k;
                self.slots[id as usize] = None;
                self.stats.evicted += 1;
                out.push(id);
            }
            k += 1;
        }
        out
    }

    /// 清一个绑定覆盖的全部 PTE（4K 粒度统一处理）。
    fn clear_ptes_of(&mut self, b: Binding) {
        let n = b.pages_4k;
        let mut i = 0u64;
        while i < n {
            let vma = b.vma + i * PAGE_4K;
            self.apertures[b.table.index()].write_pte(vma, 0);
            i += 1;
        }
        self.stats.pte_clears += n;
    }

    /// 验证一批 PTE 条目（要点一：stage 阶段，任一非法即整批拒绝）。
    ///
    /// 实现口径：条目一次性快照后**排序去重**做重复下标检测
    /// （O(n log n)，替代逐条全批重扫的 O(n²) 与逐次全量克隆——
    /// 锚点性能口径「批写 O(页数/批)」），再线性扫窗口与保留位。
    fn validate_batch(&self, b: &PteBatch, pte_span: u64) -> Result<(), GErr> {
        let entries: Vec<(u64, u64)> = b.iter().collect();
        // 批膨胀防御：条目数远超页数 ⇒ 构造错误。
        if entries.len() as u64 > pte_span * 2 + 8 {
            return Err(GErr::BatchInvalidEntry);
        }
        // 同批内下标重复 ⇒ 非法（同一 PTE 被写两遍是批构造错误）：
        // 下标排序后相邻比对，一次扫出全部重复。
        let mut idxs: Vec<u64> = Vec::with_capacity(entries.len());
        let mut e = 0usize;
        while e < entries.len() {
            idxs.push(entries[e].0);
            e += 1;
        }
        idxs.sort();
        let mut k = 1usize;
        while k < idxs.len() {
            if idxs[k] == idxs[k - 1] {
                return Err(GErr::BatchInvalidEntry);
            }
            k += 1;
        }
        // 线性扫：下标在窗内 + present 条目的保留位（bit2~11）必须为 0。
        // 保留位检查只扫外 12 位——bit0/bit1 是 present/页大小标志位活在
        // 外 12 位里，对齐检查若扫到 bit0 会把每个合法 present 条目都
        // 判成「未对齐」（烟测实测）。GPA 对齐本身由 pte_encode 清低
        // 12 位 + 池基址对齐双保险。
        let mut i = 0usize;
        while i < entries.len() {
            let (idx, value) = entries[i];
            if idx >= (APERTURE_SIZE / PAGE_4K) as u64 {
                return Err(GErr::BatchInvalidEntry);
            }
            if pte_present(value) && value & 0xFFC != 0 {
                return Err(GErr::BatchInvalidEntry);
            }
            i += 1;
        }
        Ok(())
    }

    /// 提交一批 PTE（**本单的一级功能**：页表条目批量写，锚点明文）。
    ///
    /// 两段式：先整批验证（表匹配/下标在窗/无重复/GPA 对齐），任一非法
    /// ⇒ `Err` 且**零写入**（整批回滚由「从未半提交」结构性保证，要点一）；
    /// 全部合法 ⇒ 一次性落笔（apply 不失败）。`bind` 内部批写也走本口。
    pub fn pte_batch_commit(
        &mut self,
        table: TableKind,
        b: &PteBatch,
        expect_span: u64,
    ) -> Result<(), GErr> {
        // 跨表提交拒绝（TableMismatch 的真实产生点）。
        if b.table() != table {
            return Err(GErr::TableMismatch);
        }
        self.validate_batch(b, expect_span)?;
        self.apply_batch(table, b);
        Ok(())
    }

    /// 批落笔（**仅由 [`Self::pte_batch_commit`] 在验证通过后调用**；
    /// 本函数不失败 ⇒ 无半提交）。
    fn apply_batch(&mut self, table: TableKind, b: &PteBatch) {
        let mut it = b.iter();
        while let Some((idx, value)) = it.next() {
            let vma = idx * PAGE_4K;
            self.apertures[table.index()].write_pte(vma, value);
            self.stats.pte_writes += 1;
        }
        self.stats.batch_commits += 1;
    }

    /// 绑定（主流程）。
    ///
    /// `pool_base`：分配池给的 GPA 基址（4K 对齐）；`len`：字节数；
    /// `policy`：页策略；`fence_seq`：本绑定的围栏序号（解绑时用）。
    pub fn bind(
        &mut self,
        table: TableKind,
        pool_base: u64,
        len: u64,
        policy: PagePolicy,
        fence_seq: u64,
    ) -> Result<BindTicket, GErr> {
        // —— 基础校验（边界防护）——
        if pool_base & (PAGE_4K - 1) != 0 {
            return Err(GErr::BadPoolBase);
        }
        if len == 0 {
            return Err(GErr::ZeroLength);
        }
        if len > MAX_ALLOC_BYTES {
            return Err(GErr::TooLarge);
        }
        // 找空槽（先于几何决策：无槽也属于容量不足）。
        let mut slot: Option<usize> = None;
        let mut i = 0usize;
        while i < MAX_BINDINGS {
            if self.slots[i].is_none() {
                slot = Some(i);
                break;
            }
            i += 1;
        }
        let slot = match slot {
            Some(s) => s,
            None => return Err(GErr::ApertureExhausted), // 槽位耗尽按容量耗尽报
        };

        // —— 页几何决策（要点五）——
        // 先按 4K 试找位置（偏移未定时 2M 对齐判定以「将分配到的偏移」为准，
        // 故先找 4K 粒度空洞，再做页决策，必要时按 2M 对齐重找）。
        let pages_4k = (len + PAGE_4K - 1) / PAGE_4K;
        // 主循环：找位 → 页决策 → （Force2m/Auto-2M 时）按 2M 对齐校正。
        let mut attempt = 0u32;
        loop {
            let off = match self.find_free(table, pages_4k, slot as u32) {
                Some(o) => o,
                None => {
                    // —— 驱逐路径（要点二/三）——
                    if attempt >= EVICT_RETRY_MAX {
                        return Err(GErr::ApertureExhausted);
                    }
                    attempt += 1;
                    let evicted = self.evict_sweep(table, pages_4k);
                    if evicted.is_empty() && self.stats.evict_deferred > 0 {
                        // 被冷却窗口 defer ⇒ 本轮无空间可凑，继续重试
                        // 也只能等 tick 推进；如实进入下一轮。
                        continue;
                    }
                    continue;
                }
            };
            let dec = self.decide_page(policy, off, len)?;
            // 2M 需要 vma 2M 对齐；first-fit 给的 4K 对齐偏移不保证。
            // 两个分支（天然对齐 / 需抬到边界）**都必须过冲突检查**——
            // 抬边界会跳过别的洞，恰好可能撞上现役绑定。
            let target = if matches!(dec.page, PageClass::P2m) && off & (PAGE_2M - 1) != 0 {
                let off2 = (off + PAGE_2M - 1) & !(PAGE_2M - 1);
                if off2.checked_add(len).ok_or(GErr::TooLarge)? > APERTURE_SIZE {
                    // 顶到窗尾 ⇒ 驱逐重试无法改变 first-fit 结果的确定性，
                    // 直接按耗尽报（如实，不伪装成可重试）。
                    return Err(GErr::ApertureExhausted);
                }
                off2
            } else {
                off
            };
            if let Some(c) = self.find_conflict(table, target, len) {
                self.last_conflict = Some(c);
                return Err(GErr::BindConflict);
            }
            return self.commit_bind(table, pool_base, len, pages_4k, dec, target, fence_seq, slot);
        }
    }

    /// 落槽 + 批写 PTE（bind 的最后一段；冲突已查、几何已定）。
    fn commit_bind(
        &mut self,
        table: TableKind,
        pool_base: u64,
        len: u64,
        pages_4k: u64,
        dec: PageDecision,
        off: u64,
        fence_seq: u64,
        slot: usize,
    ) -> Result<BindTicket, GErr> {
        // —— 构造 PTE 批（锚点数据结构一；批携带目标表）——
        let mut batch = PteBatch::for_table(table);
        // 2M 时每条 PTE 覆盖 512 个 4K 槽位（present 全部同值写入）。
        let step: u64 = match dec.page {
            PageClass::P4k => 1,
            PageClass::P2m => PAGES_PER_2M,
        };
        let mut p = 0u64;
        while p < pages_4k {
            let vma = off + p * PAGE_4K;
            let gpa = pool_base + p * PAGE_4K;
            let idx = Aperture::pte_index(vma);
            let val = pte_encode(gpa, dec.page, true);
            batch.push(idx, val);
            p += step;
        }
        // 先验证后落笔（要点一）：与一级批写口同一验证，失败 ⇒ 整批拒绝零写入。
        if let Err(e) = self.pte_batch_commit(table, &batch, pages_4k) {
            self.stats.batch_rejected += 1;
            return Err(e);
        }
        // —— 落槽 ——
        let b = Binding {
            vma: off,
            len,
            pages_4k,
            page: dec.page,
            refcount: 0,
            table,
            fence_seq,
            last_use: self.tick,
            pool_base,
        };
        self.slots[slot] = Some(b);
        self.stats.binds += 1;
        Ok(BindTicket { raw: slot as u32 })
    }

    /// 引用 +1（A 域三契约的引用点；回绕拒绝）。
    pub fn acquire(&mut self, t: BindTicket) -> Result<(), GErr> {
        self.acquire_many(t, 1)
    }

    /// 批量引用 +n（A 域批量引用的合法入口，如 F0224 一次提交引用
    /// N 个上下文；**批量入口让回绕成为可真实到达的路径**——
    /// n 超过 u32 余量即 `RefcountOverflow`，绝不回绕）。
    pub fn acquire_many(&mut self, t: BindTicket, n: u32) -> Result<(), GErr> {
        let b = match self.lookup(t) {
            Some(b) => b,
            None => return Err(GErr::NotFound),
        };
        let rc = b.refcount.checked_add(n).ok_or(GErr::RefcountOverflow)?;
        self.slots[t.raw as usize] = Some(Binding { refcount: rc, ..b });
        Ok(())
    }

    /// 引用 -1（不越零）。
    pub fn release_ref(&mut self, t: BindTicket) -> Result<u32, GErr> {
        let b = match self.lookup(t) {
            Some(b) => b,
            None => return Err(GErr::NotFound),
        };
        if b.refcount == 0 {
            return Err(GErr::RefcountUnderflow); // 不越零：零引用无可释放
        }
        let rc = b.refcount - 1;
        self.slots[t.raw as usize] = Some(Binding { refcount: rc, ..b });
        Ok(rc)
    }

    /// 刷新 last_use（LRU 触点：调用方每真实使用一次推一次）。
    pub fn touch(&mut self, t: BindTicket) -> Result<(), GErr> {
        let b = match self.lookup(t) {
            Some(b) => b,
            None => return Err(GErr::NotFound),
        };
        self.slots[t.raw as usize] = Some(Binding { last_use: self.tick, ..b });
        Ok(())
    }

    /// 解绑（refcount 必须 0；PTE 立即清，aperture 区间进 pending 等 fence）。
    pub fn unbind(&mut self, t: BindTicket) -> Result<(), GErr> {
        let b = match self.lookup(t) {
            Some(b) => b,
            None => return Err(GErr::NotFound),
        };
        if b.refcount > 0 {
            return Err(GErr::StillReferenced);
        }
        self.clear_ptes_of(b);
        // pending 队列满 ⇒ 最旧强收（如实记账，不静默丢弃）。
        if self.pending.len() >= MAX_PENDING {
            // 找最旧（fence_seq 最小）直接放行其区间。
            let mut oldest = 0usize;
            let mut k = 1usize;
            while k < self.pending.len() {
                if self.pending[k].fence_seq < self.pending[oldest].fence_seq {
                    oldest = k;
                }
                k += 1;
            }
            self.pending.remove(oldest);
            self.stats.pending_forced += 1;
        }
        self.pending.push(PendingRelease {
            vma: b.vma,
            len: b.len,
            pages_4k: b.pages_4k,
            table: b.table,
            fence_seq: b.fence_seq,
        });
        self.slots[t.raw as usize] = None;
        self.stats.unbinds += 1;
        Ok(())
    }

    /// fence 签到冲刷：把已签到 pending 的区间放行。
    /// `signaled`：调用方注入的签到谓词（A 域围栏契约的兑现点）。
    pub fn fence_flush(&mut self, signaled: fn(u64) -> bool) -> u32 {
        let mut kept: Vec<PendingRelease> = Vec::new();
        let mut freed = 0u32;
        let mut i = 0usize;
        while i < self.pending.len() {
            let p = self.pending[i];
            if signaled(p.fence_seq) {
                freed += 1;
            } else {
                kept.push(p);
            }
            i += 1;
        }
        self.pending = kept;
        self.stats.fence_freed += freed;
        freed
    }

    /// 读一个 PTE（判据/调试用；越界 None）。
    pub fn read_pte(&self, table: TableKind, vma: u64) -> Option<u64> {
        self.apertures[table.index()].read_pte(vma)
    }

    /// 无障碍读屏摘要（聚合口径，不含任何地址与内容——隐私红线：
    /// aperture 布局属于资产布局信息）。
    pub fn status_summary(&self) -> String {
        let mut s = String::from("显存映射：活跃 ");
        s.push_str(&self.active_count().to_string());
        s.push_str(" 项；等待回收 ");
        s.push_str(&self.pending_count().to_string());
        s.push_str(" 项；驱逐 ");
        s.push_str(&self.stats.evicted.to_string());
        s.push_str(" 次；大页降级 ");
        s.push_str(&self.stats.large_downgrades.to_string());
        s.push_str(" 次");
        s
    }
}
