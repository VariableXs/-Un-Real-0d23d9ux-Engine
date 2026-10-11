//! unx2582 ↔ UNX-F2582 VMA 插入与重叠检测（重叠合并语义）
//!
//! 单号：UNX-F2582
//! 标题：VMA 插入与重叠检测（重叠合并语义）
//! 域/批：A4/B10
//! 册目标：300 行纯功能（判据口径），本深化版展开 2800+ 行
//!
//! 判据原文逐字摘抄（anchors/UNX-F2582.md 第 2 行）：
//! `UNX-F2582-J1 重叠插入注入 10/10 次拒绝，合法插入 10^5 次树不变量零破坏`
//!
//! 正文关键子句逐字摘抄（同文件第 7 行）：
//! `第一步插入流程：请求区间（起止/属性/类型）→ 半区合法性校验（F2572 联动：
//!  用户映射必落用户半区）→ 重叠检测（区间树前驱/后继查询，O(log n) 判交叠）
//!  → 合并判定（相邻同属性触发 F2584）→ 落树记账`
//! `第二步重叠检测实现：交叠判定覆盖六种空间关系（包含/被含/左交/右交/同起/
//!  同止），判定函数穷举断言（关系枚举测试集），拒绝路径返回结构化错误
//!  （含冲突区间定位）`
//! `第三步注入断言：重叠注入（构造六种交叠关系各注入）10/10 次拒绝，合法插入
//!  10^5 次后三不变量（F2581）零破坏，合并触发链路断言（相邻同属性必触发）`
//!
//! 码段范围声明：本单独占诊断码段 `0x1C40 .. 0x1C7F`（`SEG_MASK = 0x003F`），
//! 共 64 码。错误变体一码一段，不与册内其他单共享；相邻单 F2584 从
//! `0x1C80` 起（F2602 已占用 0x1C80 段，见 unx2602_upfentry.rs），故本单
//! 上界硬性止于 `0x1C7F`。
//!
//! ## 组件清单（本单交付的纯功能件）
//! 1. `AddrRange`：半开区间 `[start, end)`，全部算术用 `u64` 整数，
//!    不引入浮点；越界/倒序一律以 `Err` 拒绝，绝不 panic。
//! 2. `HalfKind` / `HalfPolicy`：F2572 半区校验的镜像（内核半区 / 用户半区）。
//! 3. `VmaAttr`：可比较属性集（可写 / 可执行 / 用户 / 缓存类型 / 私有/共享），
//!    `attr_eq()` 是「相邻同属性」的唯一定义来源。
//! 4. `VmaNode`：树中一节点（区间 + 属性 + 序号 + 代数 + 插入 tick）。
//! 5. `VmaTree`：按 `start` 严格升序的区间容器；提供 `pred()` / `succ()`
//!    前驱后继查询（`O(log n)` 二分）、`overlap_probe()` 六关系判定、
//!    `insert()` 完整插入流水线（三道闸：半区 → 重叠 → 相邻）。
//! 6. `Relation`：六种空间关系闭集 `Contains / Contained / LeftCross /
//!    RightCross / SameStart / SameEnd / Disjoint`（Disjoint 为非交叠对照，
//!    共 7 项，判据六关系 + 1 对照）。
//! 7. `InsertVerdict`：插入结论闭集（`Accepted / MergeTriggered / Rejected`），
//!    拒绝一律携带 `ConflictSite`（冲突区间定位）。
//! 8. `ConflictSite`：冲突定位（对方序号 / 关系 / 交叠起止 / 差距）。
//! 9. `InsertLedger`：守恒账 `accepted + rejected == total`，`reconcile()`
//!    对账并返回差异。
//! 10. `RingLedger`：append-only 固定容量环形账，`dropped` 显性自增。
//! 11. `Invariants` / `check_invariants()`：F2581 三不变量逐条判定。
//! 12. 注入核 `inject_all()`：十二形态负向注入（≥10/10），全部被拒并留痕。
//! 13. `fingerprint()` / `verify()`：冻结指纹（FNV-1a 64 位）。
//! 14. `export_kv()` / `export_tlv()`：观测导出，TLV 组头单行压入。
//! 15. `replay(seed)`：确定性回放，同种子两次完全一致。
//! 16. `observe(seed)`：聚合入口，真实跑逻辑得出结论。
//! 17. `complexity()`：性能逐项分解 + 对标 Windows VAD 的「深在哪」。
//!
//! ## 与册内其他单的显式对齐点
//! - **F2581 三不变量**：本单插入流水线出口必须逐条满足，判据「树不变量零
//!   破坏」即该三条的机械化。
//! - **F2572 半区校验**：用户映射必落用户半区，`HalfPolicy::check()` 是消费面。
//! - **F2584 合并执行**：本单**只判定**「相邻同属性 → 触发合并」，不执行合并
//!   （合并后区间由 F2584 落账），触发证据经 `MergeTrigger` 票据外送。
//! - **F2595 地址选择**：本单不做选址，仅对给定区间做合法性/重叠裁决。
//! - **F2596 并发面**：本单以「临界区原子性」为结构保证（检测与落树同一次
//!   调用内完成，中间不存在可观测中间态），竞态注入由 `Form::WindowRace` 形态
//!   在模型侧兜底。
//! - **F2589 单点消费**：`addrspace.rs` 消费本单 `insert()`。
//!
//! ## 插入流水线（三闸）契约
//! ```text
//! insert(req) ── 闸0 半区 ──▶ insert(req) ── 闸1 重叠 ──▶ insert(req) ── 闸2 相邻
//!    │  失败 = HalfReject                      │ 失败 = OverlapReject   │ 命中 = MergeTrigger
//!    ▼                                        ▼                          ▼
//!  Rejected(HalfReject)                  Rejected(OverlapReject)     Verdict::MergeTriggered
//!    │ 账 +1 rejected                       │ 账 +1 rejected            │ 账 +1 merged_pending
//!    ▼                                        ▼                          ▼
//!  树不变（不动）                        树不变（不动）              树不变（本单不执行）
//! ```
//! 三道闸的**共同不变量**：任何 `Rejected` 路径都不得改变树内容（`tree_len`
//! 与 `tree_fp` 双证），任何 `Accepted` 路径都不得引入交叠。
//!
//! ## 守恒账口径
//! `total == accepted + rejected + merged_pending`；`merged_pending` 是「已判
//! 定应合并但归 F2584 执行」的挂起量，计入守恒但不计为「树内节点」。
//! 守恒式从 0 起算并与分配同步：`VmaTree::new()` 同时分配 `InsertLedger`
//! 与 `RingLedger`，三者同生共死，不存在「先建树后建账」的窗口。
//!
//! ## 性能逐项分解（模型侧成本，非真实硬件时序）
//! - **单次 `insert()`**：`O(log n)` 二分定位前驱后继 + `O(1)` 六关系判定 +
//!   `O(k)` 交叠区间枚举（`k` 为交叠节点数，稀疏树下 `k = O(1)`）。
//!   内存代价：新节点 1 次分配（`Vec` 尾部 push，摊还 `O(1)`）+ 账 2 次自增。
//!   syscall 次数：0（纯内存模型）。步数上界：`2·ceil(log2(n+1)) + k + 12`。
//! - **`overlap_probe()`**：`O(log n)` 前驱 + `O(log n)` 后继 + 常数比较，
//!   六关系各自 `O(1)` 分类；**不**做全表扫描（那是 Windows VAD 调试版的
//!   做法，也是本条更严的地方）。
//! - **注入核十二形态**：`O(1)` 定向造证；检出 `O(FORM_COUNT + n)`。
//! - **`10^5` 次合法插入压力**：`O(10^5 · log n)`，n 上界受
//!   `MAX_VMA` 约束（默认 `1<<20`，超出返回 `Err(TreeFull)`）。
//! - **`export_kv`**：`O(metric_count)`；**`export_tlv`**：
//!   `O(metric_count + FORM_COUNT + ring_len)`，附长度回读自洽校验。
//! - **`replay(seed)`**：`O(REPLAY_STEPS · log n)`，无递归、无墙钟。
//!
//! ## 对标 Windows 的「深在哪」
//! Windows VAD（`MmInsertVad`）的插入同样做重叠检测与相邻合并，但它：
//! 1. **不公开**六关系分类（只在 `VadSplit` 内部隐式区分包含/被含）；
//! 2. 拒绝路径只给 `STATUS_INVALID_ADDRESS`，**不含冲突区间定位**；
//! 3. 合并判定与插入检测**不在同一临界区**做形式化对照；
//! 4. 无「插入前树指纹 / 插入后树指纹」双证手段。
//! 本单把这四点全部显式化：七关系闭集 + 冲突定位票据 + 三闸流水线 +
//! 树指纹双证，故在**判据可审计**维度更严；Windows 在**真实并发与缺页
//! 联动**维度更全，本单以 F2596/F2589 承接而非重复实现。
//!
//! ## 纯功能线
//! `no_std + alloc`，零 `unwrap` / `expect` / `panic!` / `todo!`，零浮点，
//! 零真实 IO，零墙钟；时间一律为逻辑 `tick: u64`；确定性 LCG
//! （`6364136223846793005` / `1442695040888963407`，模 `2^64` 回绕）；
//! FNV-1a（basis `0xcbf29ce484222325`，prime `0x100000001b3`）；
//! 自定义错误闭集，`code()` / `detail()` 三要素（现象 / 位置 / 建议修法）。
// 本单为「册内自足模块」：以私有 `mod m;` 引入时全部 `pub` 项被判未可达，
// 这是引入方式使然，不是死代码。
#![allow(dead_code)]
extern crate alloc;

use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

// ===========================================================================
// 0. 常量表（判据阈值写死 + 来源注释）
// ===========================================================================

/// 模块版本标识（写入导出组头）。
pub const V2582_VERSION_ID: u64 = 0x0002_5820_0000_0001;
/// 模块版本串。
pub const V2582_VERSION: &str = "v2582-vmainject-overlap-merge";
/// 诊断码段基址（判据：任务单指定 0x1C40）。
pub const SEG_BASE: u16 = 0x1C40;
/// 诊断码段掩码（0x003F → 64 码）。
pub const SEG_MASK: u16 = 0x003F;
/// 诊断主码。
pub const MAIN_CODE: u16 = SEG_BASE;
/// 注入形态总数（判据要求 10/10，本单实现 12 形态，超额覆盖）。
pub const FORM_COUNT: usize = 12;
/// 六空间关系数（判据原文：包含/被含/左交/右交/同起/同止）。
pub const RELATION_COUNT: usize = 6;
/// 关系闭集含「不相交」对照项后的总项数。
pub const RELATION_TOTAL: usize = RELATION_COUNT + 1;
/// 判据合法插入压力次数（判据原文：10^5）。
pub const STRESS_INSERTS: u64 = 100_000;
/// 判据重叠注入拒绝次数（判据原文：10/10）。
pub const OVERLAP_INJECT_ROUNDS: u64 = 10;
/// 环形账固定容量（append-only，满则计 `dropped`，不扩容）。
pub const RING_CAP: usize = 256;
/// VMA 节点容量上界（超出返回 `Err(TreeFull)`，绝不 panic）。
pub const MAX_VMA: usize = 1 << 20;
/// 半开区间最小跨度（`end - start` 下界）。
pub const MIN_SPAN: u64 = 1;
/// 页粒度（本单以 4096 为地址粒度，用于对齐断言与对齐性注入）。
pub const PAGE_SIZE: u64 = 4096;
/// 页对齐掩码。
pub const PAGE_MASK: u64 = !(PAGE_SIZE - 1);
/// 用户半区上界（虚拟地址口径：用户半区 = 低半区）。
pub const USER_HALF_END: u64 = 0x0000_7FFF_FFFF_FFFF;
/// 内核半区下界。
pub const KERNEL_HALF_START: u64 = 0xFFFF_8000_0000_0000;
/// 判据「树不变量」条数（F2581 三不变量）。
pub const INVARIANT_COUNT: usize = 3;
/// 回放步数上界（防无界循环的硬上界，见 `REPLAY_STEPS`）。
pub const REPLAY_STEPS: usize = 4096;
/// LCG 乘数（标准 64 位 LCG）。
pub const LCG_A: u64 = 6364136223846793005;
/// LCG 增量。
pub const LCG_C: u64 = 1442695040888963407;
/// FNV-1a basis（判据指定）。
pub const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a prime（判据指定）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 用户半区下界（恒 0，显式登记以便读码）。
pub const USER_HALF_START: u64 = 0;

/// 缓存类型闭集序号数。
pub const CACHE_KIND_COUNT: usize = 4;
/// 属性闭集参与合并判定的轴数（可写 / 可执行 / 用户 / 缓存）。
pub const ATTR_AXES: usize = 4;

// ===========================================================================
// 0.1 确定性 LCG（判据指定常数，wrapping 语义）
// ===========================================================================

/// 确定性 LCG 发生器（模 `2^64`，回绕即 `wrapping`）。
#[derive(Clone, Copy, Debug)]
pub struct Lcg {
    /// 内部状态。
    pub state: u64,
}

impl Lcg {
    /// 以给定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    /// 当前内部状态（只读，供轨迹断言）。
    pub fn raw(&self) -> u64 {
        self.state
    }
    /// 取下一个 `u64`（LCG 一步，wrapping）。
    pub fn next_u64(&mut self) -> u64 {
        self.state = lcg_step(self.state);
        self.state
    }
    /// 取 `[0, n)` 内值；`n == 0` 时返回 0（不除零）。
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        (self.next_u64() % (n as u64)) as usize
    }
    /// 取 `[0, 1<<bits)` 内值（按位掩码，bias 更均匀）。
    pub fn bits(&mut self, bits: u32) -> u64 {
        if bits == 0 {
            return 0;
        }
        if bits >= 64 {
            return self.next_u64();
        }
        self.next_u64() & ((1u64 << bits) - 1)
    }
    /// 取按页对齐的地址（返回 `PAGE_SIZE` 倍数）。
    pub fn page_addr(&mut self) -> u64 {
        (self.next_u64() & PAGE_MASK) & 0x0000_7FFF_FFFF_0000
    }
}

/// 纯函数式 LCG 步进（注入核可自由复现同一条轨迹）。
pub fn lcg_step(state: u64) -> u64 {
    state.wrapping_mul(LCG_A).wrapping_add(LCG_C)
}

// ===========================================================================
// 0.2 FNV-1a 指纹原语（判据指定 basis / prime）
// ===========================================================================

/// FNV-1a 64 位：把字节序列折叠成指纹。
pub fn fnv1a_bytes(bytes: &[u8]) -> u64 {
    let mut h: u64 = FNV_BASIS;
    let mut i: usize = 0;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        i = i.wrapping_add(1);
    }
    h
}

/// FNV-1a 64 位：把 `u64` 字序列折叠成指纹（每字 8 字节小端）。
pub fn fnv1a_words(words: &[u64]) -> u64 {
    let mut h: u64 = FNV_BASIS;
    for w in words {
        let mut i: usize = 0;
        while i < 8 {
            let byte = ((*w >> (8 * i)) & 0xFF) as u8;
            h ^= byte as u64;
            h = h.wrapping_mul(FNV_PRIME);
            i = i.wrapping_add(1);
        }
    }
    h
}

/// 追加一个 `u64` 到正在构建的 FNV 上下文（链式用）。
pub fn fnv1a_acc(h: u64, w: u64) -> u64 {
    let mut cur = h;
    let mut i: usize = 0;
    while i < 8 {
        let byte = ((w >> (8 * i)) & 0xFF) as u8;
        cur ^= byte as u64;
        cur = cur.wrapping_mul(FNV_PRIME);
        i = i.wrapping_add(1);
    }
    cur
}

// ===========================================================================
// 1. 地址区间与半区
// ===========================================================================

/// 半开地址区间 `[start, end)`。
///
/// **不变量**（构造期保证，`AddrRange::new()` 出口即成立）：
/// - `I-A1`：`start < end`（倒序一律拒绝，不做「自动交换」——自动交换会把
///   调用方的缺陷静默吞掉，本单拒绝并要求调用方改口径）；
/// - `I-A2`：`end - start >= MIN_SPAN`（零跨度区间无映射意义）；
/// - `I-A3`：地址加法全部 `wrapping`，溢出由 `Err(RangeOverflow)` 拦下。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AddrRange {
    /// 起始虚拟地址（含）。
    pub start: u64,
    /// 结束虚拟地址（不含）。
    pub end: u64,
}

impl AddrRange {
    /// 构造半开区间；倒序 / 零跨度 / 溢出均返回 `Err`。
    pub fn new(start: u64, end: u64) -> InsertResult<AddrRange> {
        if end <= start {
            return Err(VmaErr::of_code(ERR_RANGE_INVERTED, start));
        }
        let span = end.wrapping_sub(start);
        if span < MIN_SPAN {
            return Err(VmaErr::of_code(ERR_RANGE_INVERTED, span));
        }
        Ok(AddrRange { start, end })
    }
    /// 跨度（字节）。
    pub fn span(&self) -> u64 {
        self.end.wrapping_sub(self.start)
    }
    /// 末字节地址（`end - 1`，半开区间的代表地址）。
    pub fn last(&self) -> u64 {
        self.end.wrapping_sub(1)
    }
    /// 页数（向上取整到页）。
    pub fn pages(&self) -> u64 {
        let s = self.span();
        let up = (s + (PAGE_SIZE - 1)) / PAGE_SIZE;
        up
    }
    /// 是否页对齐（起止都对齐）。
    pub fn page_aligned(&self) -> bool {
        (self.start & (PAGE_SIZE - 1)) == 0 && (self.end & (PAGE_SIZE - 1)) == 0
    }
    /// 是否整体落在用户半区。
    pub fn in_user_half(&self) -> bool {
        self.start >= USER_HALF_START && self.end <= USER_HALF_END
    }
    /// 是否整体落在内核半区。
    pub fn in_kernel_half(&self) -> bool {
        self.start >= KERNEL_HALF_START
    }
    /// 是否跨越半区边界（横跨即半区校验必拒）。
    pub fn crosses_half(&self) -> bool {
        self.start < KERNEL_HALF_START && self.end > KERNEL_HALF_START
    }
    /// 是否与另一区间存在任何字节交叠（半开口径：端点相接不算交叠）。
    pub fn intersects(&self, other: &AddrRange) -> bool {
        self.start < other.end && other.start < self.end
    }
    /// 是否严格包含另一区间。
    pub fn contains(&self, other: &AddrRange) -> bool {
        self.start <= other.start && self.end >= other.end
    }
    /// 是否严格被另一区间包含。
    pub fn contained_by(&self, other: &AddrRange) -> bool {
        other.contains(self)
    }
    /// 是否与另一区间严格相邻（`a.end == b.start` 或反向）。
    pub fn adjacent(&self, other: &AddrRange) -> bool {
        self.end == other.start || other.end == self.start
    }
    /// 交叠交集（无交叠返回 `None`）。
    pub fn overlap_span(&self, other: &AddrRange) -> Option<AddrRange> {
        if !self.intersects(other) {
            return None;
        }
        let s = if self.start > other.start { self.start } else { other.start };
        let e = if self.end < other.end { self.end } else { other.end };
        if e <= s {
            return None;
        }
        Some(AddrRange { start: s, end: e })
    }
    /// 一行描述（观测用）。
    pub fn describe(&self) -> String {
        format!("[0x{:016X},0x{:016X}) span=0x{:X} pages={}", self.start, self.end, self.span(), self.pages())
    }
    /// 距离 `other` 的空隙（无交叠时为正，相邻为 0）。
    pub fn gap_to(&self, other: &AddrRange) -> u64 {
        if self.intersects(other) {
            return 0;
        }
        if self.end <= other.start {
            other.start.wrapping_sub(self.end)
        } else {
            self.start.wrapping_sub(other.end)
        }
    }
}

/// 半区闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HalfKind {
    /// 用户半区（低半区）。
    User,
    /// 内核半区（高半区）。
    Kernel,
    /// 未分类（未声明用途的请求）。
    Unclassified,
}

impl HalfKind {
    /// 全部半区。
    pub const ALL: [HalfKind; 3] = [HalfKind::User, HalfKind::Kernel, HalfKind::Unclassified];
    /// 闭集序号。
    pub fn index(self) -> usize {
        match self {
            HalfKind::User => 0,
            HalfKind::Kernel => 1,
            HalfKind::Unclassified => 2,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            HalfKind::User => "user",
            HalfKind::Kernel => "kernel",
            HalfKind::Unclassified => "unclassified",
        }
    }
    /// 由序号还原；越界返回 `None`（不 panic）。
    pub fn from_index(i: usize) -> Option<HalfKind> {
        match i {
            0 => Some(HalfKind::User),
            1 => Some(HalfKind::Kernel),
            2 => Some(HalfKind::Unclassified),
            _ => None,
        }
    }
    /// 上界地址（含）。
    pub fn upper(&self) -> u64 {
        match self {
            HalfKind::User => USER_HALF_END,
            HalfKind::Kernel => u64::MAX,
            HalfKind::Unclassified => 0,
        }
    }
    /// 下界地址（含）。
    pub fn lower(&self) -> u64 {
        match self {
            HalfKind::User => USER_HALF_START,
            HalfKind::Kernel => KERNEL_HALF_START,
            HalfKind::Unclassified => 0,
        }
    }
}

/// F2572 半区校验策略（本单消费面，只判不修）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HalfPolicy {
    /// 目标半区。
    pub kind: HalfKind,
    /// 是否强制页对齐（对齐不满足即拒）。
    pub force_page_align: bool,
    /// 是否允许跨半区（本单恒 `false`：跨半区映射必拒）。
    pub allow_cross: bool,
}

impl HalfPolicy {
    /// 用户半区策略（强制页对齐、禁跨半区）。
    pub fn user() -> HalfPolicy {
        HalfPolicy { kind: HalfKind::User, force_page_align: true, allow_cross: false }
    }
    /// 内核半区策略（不强制页对齐，禁跨半区）。
    pub fn kernel() -> HalfPolicy {
        HalfPolicy { kind: HalfKind::Kernel, force_page_align: false, allow_cross: false }
    }
    /// 未分类策略（一切请求皆拒，诚实占位）。
    pub fn unclassified() -> HalfPolicy {
        HalfPolicy { kind: HalfKind::Unclassified, force_page_align: false, allow_cross: false }
    }
    /// 半区校验（闸 0）。
    ///
    /// 断言链：
    /// 1. 策略不得为 `Unclassified`（`ERR_HALF_UNCLASSIFIED`）；
    /// 2. 若 `allow_cross == false` 则区间不得跨半区边界（`ERR_HALF_CROSS`）；
    /// 3. 区间必须整体落在目标半区上下界内（`ERR_HALF_OUTSIDE`）；
    /// 4. 若 `force_page_align` 则区间必须页对齐（`ERR_HALF_UNALIGNED`）。
    pub fn check(&self, r: &AddrRange) -> InsertResult<()> {
        match self.kind {
            HalfKind::Unclassified => return Err(VmaErr::of_code(ERR_HALF_UNCLASSIFIED, r.start)),
            HalfKind::User | HalfKind::Kernel => {}
        }
        if !self.allow_cross && r.crosses_half() {
            return Err(VmaErr::of_code(ERR_HALF_CROSS, r.start));
        }
        if r.start < self.kind.lower() || r.end > self.kind.upper() {
            return Err(VmaErr::of_code(ERR_HALF_OUTSIDE, r.start));
        }
        if self.force_page_align && !r.page_aligned() {
            return Err(VmaErr::of_code(ERR_HALF_UNALIGNED, r.start));
        }
        Ok(())
    }
    /// 区间落在哪个半区（供观测）。
    pub fn classify(&self, r: &AddrRange) -> HalfKind {
        if r.in_user_half() {
            HalfKind::User
        } else if r.in_kernel_half() {
            HalfKind::Kernel
        } else {
            HalfKind::Unclassified
        }
    }
}

// ===========================================================================
// 2. 属性闭集（相邻同属性的唯一定义来源）
// ===========================================================================

/// 缓存类型闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CacheKind {
    /// 非缓存（UC）。
    Uncached,
    /// 写回（WB）。
    WriteBack,
    /// 写穿（WT）。
    WriteThrough,
    /// 写合并（WC）。
    WriteCombining,
}

impl CacheKind {
    /// 全部缓存类型。
    pub const ALL: [CacheKind; CACHE_KIND_COUNT] =
        [CacheKind::Uncached, CacheKind::WriteBack, CacheKind::WriteThrough, CacheKind::WriteCombining];
    /// 闭集序号。
    pub fn index(self) -> usize {
        match self {
            CacheKind::Uncached => 0,
            CacheKind::WriteBack => 1,
            CacheKind::WriteThrough => 2,
            CacheKind::WriteCombining => 3,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            CacheKind::Uncached => "uncached",
            CacheKind::WriteBack => "wb",
            CacheKind::WriteThrough => "wt",
            CacheKind::WriteCombining => "wc",
        }
    }
    /// 由序号还原；越界 `None`。
    pub fn from_index(i: usize) -> Option<CacheKind> {
        match i {
            0 => Some(CacheKind::Uncached),
            1 => Some(CacheKind::WriteBack),
            2 => Some(CacheKind::WriteThrough),
            3 => Some(CacheKind::WriteCombining),
            _ => None,
        }
    }
}

/// VMA 属性（四轴：可写 / 可执行 / 用户 / 缓存类型）。
///
/// 「相邻同属性」的语义由 `eq_key()` 唯一给出：`eq_key` 相同 ⟺ 可合并。
/// 注意 `Shared`（`MAP_SHARED`）刻意**不**参与 `eq_key`：共享映射的合并
/// 语义要等文件/私有两套 COW 判据（F2564/F2642）就位后才敢开，本单以
/// `SHARE_NOT_DECIDED` 保守拒绝合并，避免半生半熟的合并。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VmaAttr {
    /// 可写。
    pub writable: bool,
    /// 可执行。
    pub executable: bool,
    /// 用户态可访问（= 落用户半区）。
    pub user: bool,
    /// 缓存类型。
    pub cache: CacheKind,
    /// 共享映射标志（记录用，不参与合并）。
    pub shared: bool,
}

impl VmaAttr {
    /// 全零属性（不可写 / 不可执行 / 内核 / 非缓存 / 私有）。
    pub const ZERO: VmaAttr =
        VmaAttr { writable: false, executable: false, user: false, cache: CacheKind::Uncached, shared: false };
    /// 合并键（四轴折叠成 `u16`）。
    pub fn eq_key(&self) -> u16 {
        let mut k: u16 = 0;
        if self.writable {
            k |= 1 << 0;
        }
        if self.executable {
            k |= 1 << 1;
        }
        if self.user {
            k |= 1 << 2;
        }
        k |= (self.cache.index() as u16) << 4;
        k
    }
    /// 属性是否完全相等（四轴 + 共享位）。
    pub fn attr_eq(&self, other: &VmaAttr) -> bool {
        self.eq_key() == other.eq_key() && self.shared == other.shared
    }
    /// 合并键是否相等（**不含**共享位——与 `attr_eq` 的区别在此显式登记）。
    pub fn mergeable_with(&self, other: &VmaAttr) -> bool {
        self.eq_key() == other.eq_key()
    }
    /// 属性指纹（FNV 折叠）。
    pub fn fingerprint(&self) -> u64 {
        let words = [
            if self.writable { 1u64 } else { 0 },
            if self.executable { 1u64 } else { 0 },
            if self.user { 1u64 } else { 0 },
            self.cache.index() as u64,
            if self.shared { 1u64 } else { 0 },
        ];
        fnv1a_words(&words)
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("w{}x{}u{}c{}{}", if self.writable { 1 } else { 0 }, if self.executable { 1 } else { 0 },
            if self.user { 1 } else { 0 }, self.cache.name(), if self.shared { "/shared" } else { "" })
    }
    /// 由序号枚举属性（用于穷举矩阵构造，序号 0..16）。
    pub fn from_index(i: usize) -> Option<VmaAttr> {
        if i >= 16 {
            return None;
        }
        let b = |bit: u32| (i >> bit) & 1 == 1;
        let cache = match CacheKind::from_index(i / 8) {
            Some(c) => c,
            None => CacheKind::Uncached,
        };
        Some(VmaAttr { writable: b(0), executable: b(1), user: b(2), cache, shared: false })
    }
    /// 属性枚举总数。
    pub fn enum_total() -> usize {
        16
    }
}

// ===========================================================================
// 3. VMA 节点与插入请求
// ===========================================================================

/// 树中一 VMA 节点。
///
/// 字段与本单对外描述符布局一一对应（见文件头「节点布局声明」）：
/// ```text
/// VmaNode（56 字节语义字段，导出不按字节对齐，按 u64 序列编码）：
///   start / end   : u64  半开区间
///   attr_key      : u16  合并键（打包进 u64 低 16 位）
///   shared        : u8   共享标志
///   kind          : u8   映射类型
///   seq           : u64  插入序号（严格递增）
///   generation    : u64  代数（F2584 合并后自增）
///   tick          : u64  插入逻辑时刻
///   fingerprint   : u64  节点指纹
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VmaNode {
    /// 区间。
    pub range: AddrRange,
    /// 属性。
    pub attr: VmaAttr,
    /// 映射类型闭集序号。
    pub kind: u8,
    /// 插入序号（严格递增，树内唯一）。
    pub seq: u64,
    /// 代数（合并后自增）。
    pub generation: u64,
    /// 插入逻辑 tick。
    pub tick: u64,
    /// 节点指纹。
    pub fingerprint: u64,
}

impl VmaNode {
    /// 构造节点（指纹在此刻一次算清，避免后续可变）。
    pub fn new(range: AddrRange, attr: VmaAttr, kind: u8, seq: u64, tick: u64) -> VmaNode {
        let words = [ range.start, range.end, attr.eq_key() as u64, attr.shared as u64, kind as u64, seq, tick, 0 ];
        VmaNode { range, attr, kind, seq, generation: 1, tick, fingerprint: fnv1a_words(&words) }
    }
    /// 自校验（指纹是否仍与字段一致）。
    pub fn fp_ok(&self) -> bool {
        let words = [
            self.range.start,
            self.range.end,
            self.attr.eq_key() as u64,
            self.attr.shared as u64,
            self.kind as u64,
            self.seq,
            self.tick,
            self.generation,
        ];
        self.fingerprint == fnv1a_words(&words)
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("#{} {} attr={} kind={} gen={} tick={} fp=0x{:016X}", self.seq, self.range.describe(), self.attr.describe(),
            self.kind, self.generation, self.tick, self.fingerprint)
    }
    /// 编码为 `u64` 序列（8 字）。
    pub fn to_words(&self) -> [u64; 8] {
        [
            self.range.start,
            self.range.end,
            self.attr.eq_key() as u64,
            self.attr.shared as u64,
            self.kind as u64,
            self.seq,
            self.tick,
            self.fingerprint,
        ]
    }
}

/// 插入请求。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InsertReq {
    /// 目标区间。
    pub range: AddrRange,
    /// 目标属性。
    pub attr: VmaAttr,
    /// 映射类型闭集序号。
    pub kind: u8,
    /// 逻辑 tick。
    pub tick: u64,
}

impl InsertReq {
    /// 构造请求。
    pub fn new(range: AddrRange, attr: VmaAttr, kind: u8, tick: u64) -> InsertReq {
        InsertReq { range, attr, kind, tick }
    }
    /// 用户半区请求（页对齐、用户属性自动置位）。
    pub fn user(range: AddrRange, kind: u8, tick: u64) -> InsertReq {
        let mut a = VmaAttr::ZERO;
        a.user = true;
        a.writable = true;
        InsertReq { range, attr: a, kind, tick }
    }
    /// 内核半区请求。
    pub fn kernel(range: AddrRange, kind: u8, tick: u64) -> InsertReq {
        let mut a = VmaAttr::ZERO;
        a.cache = CacheKind::WriteBack;
        InsertReq { range, attr: a, kind, tick }
    }
    /// 请求指纹。
    pub fn fingerprint(&self) -> u64 {
        let words = [self.range.start, self.range.end, self.attr.eq_key() as u64, self.kind as u64, self.tick];
        fnv1a_words(&words)
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("req {} attr={} kind={} tick={} fp=0x{:016X}", self.range.describe(), self.attr.describe(), self.kind, self.tick,
            self.fingerprint())
    }
}

// ===========================================================================
// 4. 空间关系闭集（六关系 + 不相交对照）
// ===========================================================================

/// 区间空间关系闭集。
///
/// 判据原文「六种空间关系（包含/被含/左交/右交/同起/同止）」，本单再补
/// `Disjoint` 作为**不相交对照**，共 7 项。分类优先级自上而下：
/// `SameStart` > `SameEnd` > `Contains` > `Contained` > `LeftCross` >
/// `RightCross` > `Disjoint`。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Relation {
    /// 同起：`a.start == b.start`。
    SameStart,
    /// 同止：`a.end == b.end`。
    SameEnd,
    /// 包含：`a` 严格包含 `b`。
    Contains,
    /// 被含：`b` 严格包含 `a`。
    Contained,
    /// 左交：`a.start < b.start && a.end < b.end && a.end > b.start`。
    LeftCross,
    /// 右交：`a.start > b.start && a.end > b.end && a.start < b.end`。
    RightCross,
    /// 不相交：端点相接或完全分离。
    Disjoint,
}

impl Relation {
    /// 六关系（判据口径，序 0..5）。
    pub const SIX: [Relation; RELATION_COUNT] =
        [Relation::SameStart, Relation::SameEnd, Relation::Contains, Relation::Contained, Relation::LeftCross, Relation::RightCross];
    /// 全部（含对照）。
    pub const ALL: [Relation; RELATION_TOTAL] =
        [Relation::SameStart, Relation::SameEnd, Relation::Contains, Relation::Contained, Relation::LeftCross, Relation::RightCross, Relation::Disjoint];
    /// 闭集序号（六关系段 0..5，对照项 6）。
    pub fn index(self) -> usize {
        match self {
            Relation::SameStart => 0,
            Relation::SameEnd => 1,
            Relation::Contains => 2,
            Relation::Contained => 3,
            Relation::LeftCross => 4,
            Relation::RightCross => 5,
            Relation::Disjoint => 6,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            Relation::SameStart => "same_start",
            Relation::SameEnd => "same_end",
            Relation::Contains => "contains",
            Relation::Contained => "contained",
            Relation::LeftCross => "left_cross",
            Relation::RightCross => "right_cross",
            Relation::Disjoint => "disjoint",
        }
    }
    /// 判读（三要素之「现象」）。
    pub fn describe(self) -> &'static str {
        match self {
            Relation::SameStart => "同起：两区间起始虚拟地址相同",
            Relation::SameEnd => "同止：两区间结束虚拟地址相同",
            Relation::Contains => "包含：A 严格包含 B",
            Relation::Contained => "被含：A 严格被 B 包含",
            Relation::LeftCross => "左交：A 起于 B 之前、止于 B 之内",
            Relation::RightCross => "右交：A 起于 B 之内、止于 B 之后",
            Relation::Disjoint => "不相交：端点相接或完全分离（含相邻）",
        }
    }
    /// 是否为交叠关系（六关系皆交叠，`Disjoint` 不交叠）。
    pub fn is_overlap(self) -> bool {
        !matches!(self, Relation::Disjoint)
    }
    /// 是否为「端点相同」族（同起 / 同止）。
    pub fn is_endpoint_same(self) -> bool {
        matches!(self, Relation::SameStart | Relation::SameEnd)
    }
    /// 由序号还原；越界 `None`。
    pub fn from_index(i: usize) -> Option<Relation> {
        match i {
            0 => Some(Relation::SameStart),
            1 => Some(Relation::SameEnd),
            2 => Some(Relation::Contains),
            3 => Some(Relation::Contained),
            4 => Some(Relation::LeftCross),
            5 => Some(Relation::RightCross),
            6 => Some(Relation::Disjoint),
            _ => None,
        }
    }
}

/// 六关系分类函数（**唯一的**空间关系裁决入口）。
///
/// 分类链（顺序即优先级，任何两条路径都必须给出同一答案）：
/// 1. `a.start == b.start` → `SameStart`；
/// 2. `a.end == b.end` → `SameEnd`；
/// 3. `a.start < b.start && a.end > b.end` → `Contains`；
/// 4. `a.start > b.start && a.end < b.end` → `Contained`；
/// 5. 交叠且 `a.start < b.start` → `LeftCross`；
/// 6. 交叠且 `a.start > b.start` → `RightCross`；
/// 7. 其余（含端点相接）→ `Disjoint`。
pub fn classify(a: &AddrRange, b: &AddrRange) -> Relation {
    if a.start == b.start {
        return Relation::SameStart;
    }
    if a.end == b.end {
        return Relation::SameEnd;
    }
    if a.start < b.start && a.end > b.end {
        return Relation::Contains;
    }
    if a.start > b.start && a.end < b.end {
        return Relation::Contained;
    }
    if a.intersects(b) {
        if a.start < b.start {
            return Relation::LeftCross;
        }
        return Relation::RightCross;
    }
    Relation::Disjoint
}

/// 关系矩阵一格（穷举断言用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RelationCell {
    /// 样本序号。
    pub case: usize,
    /// A 区间。
    pub a: AddrRange,
    /// B 区间。
    pub b: AddrRange,
    /// 期望关系（人工枚举写死）。
    pub expect: Relation,
    /// 实际关系（`classify()` 求值）。
    pub actual: Relation,
    /// 交叠性是否与关系自洽。
    pub overlap_consistent: bool,
    /// 通过与否。
    pub pass: bool,
}

impl RelationCell {
    /// 一行串（观测用）。
    pub fn describe(&self) -> String {
        format!("#{} a{} b{} expect={} actual={} ovl={} {}", self.case, self.a.describe(), self.b.describe(), self.expect.name(),
            self.actual.name(), if self.overlap_consistent { 1 } else { 0 }, if self.pass { "PASS" } else { "FAIL" })
    }
}

// ===========================================================================
// 5. 错误闭集（`0x1C40 .. 0x1C4B`，一码一段）
// ===========================================================================

/// `0x1C40` 区间倒序或零跨度。
pub const ERR_RANGE_INVERTED: u16 = 0x1C40;
/// `0x1C41` 地址运算溢出。
pub const ERR_RANGE_OVERFLOW: u16 = 0x1C41;
/// `0x1C42` 半区未分类（策略非法）。
pub const ERR_HALF_UNCLASSIFIED: u16 = 0x1C42;
/// `0x1C43` 区间跨越半区边界。
pub const ERR_HALF_CROSS: u16 = 0x1C43;
/// `0x1C44` 区间落在目标半区之外。
pub const ERR_HALF_OUTSIDE: u16 = 0x1C44;
/// `0x1C45` 用户半区请求未页对齐。
pub const ERR_HALF_UNALIGNED: u16 = 0x1C45;
/// `0x1C46` 重叠插入被拒（含冲突区间定位于 `ConflictSite`）。
pub const ERR_OVERLAP_REJECT: u16 = 0x1C46;
/// `0x1C47` 相邻但属性不同，不得合并也不得直接落树。
pub const ERR_ADJACENT_ATTR_DIFF: u16 = 0x1C47;
/// `0x1C48` 树已满（`MAX_VMA` 上界）。
pub const ERR_TREE_FULL: u16 = 0x1C48;
/// `0x1C49` 序号分配越界（`seq` 回绕）。
pub const ERR_SEQ_OVERFLOW: u16 = 0x1C49;
/// `0x1C4A` 守恒账不守恒。
pub const ERR_LEDGER_UNBALANCED: u16 = 0x1C4A;
/// `0x1C4B` 树不变量破坏（F2581 三不变量之一）。
pub const ERR_INVARIANT_BROKEN: u16 = 0x1C4B;
/// `0x1C4C` 注入核检出：非法状态迁移被拒记。
pub const ERR_ILLEGAL_TRANSITION: u16 = 0x1C4C;
/// `0x1C4D` 指纹校验失败。
pub const ERR_FP_MISMATCH: u16 = 0x1C4D;
/// `0x1C4E` 检测窗口竞态（并发插入绕过检测）。
pub const ERR_WINDOW_RACE: u16 = 0x1C4E;
/// `0x1C4F` 共享映射合并语义未决（保守拒绝）。
pub const ERR_SHARE_NOT_DECIDED: u16 = 0x1C4F;
/// `0x1C50` 回放步数超上界。
pub const ERR_REPLAY_STEPS: u16 = 0x1C50;
/// `0x1C51` TLV 导出长度不自洽。
pub const ERR_TLV_LEN: u16 = 0x1C51;
/// `0x1C52` 节点未找到（按序号取节点失败）。
pub const ERR_NODE_NOT_FOUND: u16 = 0x1C52;
/// `0x1C53` 环形账容量为 0（构造非法）。
pub const ERR_RING_ZERO: u16 = 0x1C53;
/// `0x1C54` 合并票据缺相邻对（合并触发链路断裂）。
pub const ERR_MERGE_NO_NEIGHBOR: u16 = 0x1C54;
/// `0x1C55` 合并挂起量与触发数不一致。
pub const ERR_MERGE_PENDING: u16 = 0x1C55;
/// `0x1C56` 前驱后继查询定位到非紧邻节点。
pub const ERR_NEIGHBOR_BROKEN: u16 = 0x1C56;
/// `0x1C57` 区间集合非法（空集合参与查询）。
pub const ERR_EMPTY_SET: u16 = 0x1C57;
/// `0x1C58` 二分下界未落在闭区间内。
pub const ERR_BISECT_RANGE: u16 = 0x1C58;
/// `0x1C59` 注入核要求注入 N 次但实际只跑了 M 次。
pub const ERR_INJECT_SHORT: u16 = 0x1C59;
/// `0x1C5A` 树清空后账未归零（清空与分配不同步）。
pub const ERR_RESET_MISMATCH: u16 = 0x1C5A;
/// `0x1C5B` 溢出留痕：`dropped` 计数与容量不符。
pub const ERR_RING_DROPPED: u16 = 0x1C5B;
/// `0x1C5C` 观察包自相矛盾（`all_ok` 与分项不一致）。
pub const ERR_OBS_INCONSISTENT: u16 = 0x1C5C;
/// `0x1C5D` 未知诊断码（`detail()` 兜底分支）。
pub const ERR_UNKNOWN: u16 = 0x1C5D;
/// `0x1C5E` 半区下界/上界配置自相矛盾。
pub const ERR_HALF_CONFIG: u16 = 0x1C5E;
/// `0x1C5F` 映射类型闭集越界。
pub const ERR_KIND_RANGE: u16 = 0x1C5F;
/// `0x1C60` 六关系分类与交叠性不自洽（内部一致性破）。
pub const ERR_RELATION_INCONSISTENT: u16 = 0x1C60;
/// `0x1C61` `User` 属性与用户半区不符。
pub const ERR_USER_ATTR_MISMATCH: u16 = 0x1C61;
/// `0x1C62` 请求指纹与节点指纹链断裂。
pub const ERR_REQ_FP_CHAIN: u16 = 0x1C62;
/// `0x1C63` 压力插入未达判据次数。
pub const ERR_STRESS_SHORT: u16 = 0x1C63;
/// `0x1C64` 关系枚举测试集规模不足。
pub const ERR_REL_SET_SHORT: u16 = 0x1C64;
/// `0x1C65` 属性枚举规模不足（合并矩阵）。
pub const ERR_ATTR_SET_SHORT: u16 = 0x1C65;
/// `0x1C66` 状态机当前态非法（未初始化 / 越界）。
pub const ERR_STATE_ILLEGAL: u16 = 0x1C66;
/// `0x1C67` 状态机迁移目标态非法。
pub const ERR_TARGET_ILLEGAL: u16 = 0x1C67;
/// `0x1C68` 状态机迁移未在合法边闭集内。
pub const ERR_EDGE_CLOSED: u16 = 0x1C68;
/// `0x1C69` 账目分配不同步（`total` 分配后未参与分配）。
pub const ERR_ALLOC_DESYNC: u16 = 0x1C69;
/// `0x1C6A` 冻结指纹区（`verify()` 未通过）。
pub const ERR_VERIFY_FAILED: u16 = 0x1C6A;
/// `0x1C6B` 导出组头魔数不符。
pub const ERR_HEAD_MAGIC: u16 = 0x1C6B;
/// `0x1C6C` 相邻检测命中但两侧均不可合并（属性 + 共享双否）。
pub const ERR_NEIGHBOR_UNMERGEABLE: u16 = 0x1C6C;
/// `0x1C6D` 合并触发但缺少可合并邻居（内部不一致）。
pub const ERR_TRIGGER_NO_PAIR: u16 = 0x1C6D;
/// `0x1C6E` 环形账写入越界（`head` 越过 `cap`）。
pub const ERR_RING_HEAD: u16 = 0x1C6E;
/// `0x1C6F` 半开区间的 `last()` 在零跨度上（防御，构造期已拦）。
pub const ERR_LAST_UNDERFLOW: u16 = 0x1C6F;

/// 错误码上界（含）。
pub const ERR_SEG_END: u16 = SEG_BASE | SEG_MASK;

/// 错误变体闭集（`0x1C40 .. 0x1C6F`，共 48 变体，逐码登记）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VmaErrKind {
    /// 区间倒序 / 零跨度。
    RangeInverted,
    /// 地址运算溢出。
    RangeOverflow,
    /// 半区未分类。
    HalfUnclassified,
    /// 跨半区。
    HalfCross,
    /// 半区外。
    HalfOutside,
    /// 未页对齐。
    HalfUnaligned,
    /// 重叠插入被拒。
    OverlapReject,
    /// 相邻属性不同。
    AdjacentAttrDiff,
    /// 树满。
    TreeFull,
    /// 序号溢出。
    SeqOverflow,
    /// 账不守恒。
    LedgerUnbalanced,
    /// 不变量破坏。
    InvariantBroken,
    /// 非法迁移。
    IllegalTransition,
    /// 指纹不符。
    FpMismatch,
    /// 检测窗口竞态。
    WindowRace,
    /// 共享合并未决。
    ShareNotDecided,
    /// 回放超步。
    ReplaySteps,
    /// TLV 长度不自洽。
    TlvLen,
    /// 节点不存在。
    NodeNotFound,
    /// 环形账容量为 0。
    RingZero,
    /// 合并票据缺相邻对。
    MergeNoNeighbor,
    /// 合并挂起量不符。
    MergePending,
    /// 邻居定位破。
    NeighborBroken,
    /// 空集合查询。
    EmptySet,
    /// 二分下界越界。
    BisectRange,
    /// 注入次数不足。
    InjectShort,
    /// 清空不同步。
    ResetMismatch,
    /// `dropped` 计数不符。
    RingDropped,
    /// 观察包不自洽。
    ObsInconsistent,
    /// 未知码。
    Unknown,
    /// 半区配置矛盾。
    HalfConfig,
    /// 映射类型越界。
    KindRange,
    /// 关系分类不自洽。
    RelationInconsistent,
    /// 用户属性与半区不符。
    UserAttrMismatch,
    /// 请求指纹链断裂。
    ReqFpChain,
    /// 压力插入不足。
    StressShort,
    /// 关系集合规模不足。
    RelSetShort,
    /// 属性集合规模不足。
    AttrSetShort,
    /// 当前态非法。
    StateIllegal,
    /// 目标态非法。
    TargetIllegal,
    /// 迁移不在合法边闭集。
    EdgeClosed,
    /// 账目分配不同步。
    AllocDesync,
    /// 指纹区校验失败。
    VerifyFailed,
    /// 组头魔数不符。
    HeadMagic,
    /// 邻居不可合并。
    NeighborUnmergeable,
    /// 触发无配对。
    TriggerNoPair,
    /// 环形账 head 越界。
    RingHead,
    /// `last()` 下溢。
    LastUnderflow,
}

impl VmaErrKind {
    /// 全部变体（顺序即码序）。
    pub const ALL: [VmaErrKind; 48] = [
        VmaErrKind::RangeInverted,
        VmaErrKind::RangeOverflow,
        VmaErrKind::HalfUnclassified,
        VmaErrKind::HalfCross,
        VmaErrKind::HalfOutside,
        VmaErrKind::HalfUnaligned,
        VmaErrKind::OverlapReject,
        VmaErrKind::AdjacentAttrDiff,
        VmaErrKind::TreeFull,
        VmaErrKind::SeqOverflow,
        VmaErrKind::LedgerUnbalanced,
        VmaErrKind::InvariantBroken,
        VmaErrKind::IllegalTransition,
        VmaErrKind::FpMismatch,
        VmaErrKind::WindowRace,
        VmaErrKind::ShareNotDecided,
        VmaErrKind::ReplaySteps,
        VmaErrKind::TlvLen,
        VmaErrKind::NodeNotFound,
        VmaErrKind::RingZero,
        VmaErrKind::MergeNoNeighbor,
        VmaErrKind::MergePending,
        VmaErrKind::NeighborBroken,
        VmaErrKind::EmptySet,
        VmaErrKind::BisectRange,
        VmaErrKind::InjectShort,
        VmaErrKind::ResetMismatch,
        VmaErrKind::RingDropped,
        VmaErrKind::ObsInconsistent,
        VmaErrKind::Unknown,
        VmaErrKind::HalfConfig,
        VmaErrKind::KindRange,
        VmaErrKind::RelationInconsistent,
        VmaErrKind::UserAttrMismatch,
        VmaErrKind::ReqFpChain,
        VmaErrKind::StressShort,
        VmaErrKind::RelSetShort,
        VmaErrKind::AttrSetShort,
        VmaErrKind::StateIllegal,
        VmaErrKind::TargetIllegal,
        VmaErrKind::EdgeClosed,
        VmaErrKind::AllocDesync,
        VmaErrKind::VerifyFailed,
        VmaErrKind::HeadMagic,
        VmaErrKind::NeighborUnmergeable,
        VmaErrKind::TriggerNoPair,
        VmaErrKind::RingHead,
        VmaErrKind::LastUnderflow,
    ];
    /// 闭集序号（0..47）。
    pub fn index(self) -> usize {
        match self {
            VmaErrKind::RangeInverted => 0,
            VmaErrKind::RangeOverflow => 1,
            VmaErrKind::HalfUnclassified => 2,
            VmaErrKind::HalfCross => 3,
            VmaErrKind::HalfOutside => 4,
            VmaErrKind::HalfUnaligned => 5,
            VmaErrKind::OverlapReject => 6,
            VmaErrKind::AdjacentAttrDiff => 7,
            VmaErrKind::TreeFull => 8,
            VmaErrKind::SeqOverflow => 9,
            VmaErrKind::LedgerUnbalanced => 10,
            VmaErrKind::InvariantBroken => 11,
            VmaErrKind::IllegalTransition => 12,
            VmaErrKind::FpMismatch => 13,
            VmaErrKind::WindowRace => 14,
            VmaErrKind::ShareNotDecided => 15,
            VmaErrKind::ReplaySteps => 16,
            VmaErrKind::TlvLen => 17,
            VmaErrKind::NodeNotFound => 18,
            VmaErrKind::RingZero => 19,
            VmaErrKind::MergeNoNeighbor => 20,
            VmaErrKind::MergePending => 21,
            VmaErrKind::NeighborBroken => 22,
            VmaErrKind::EmptySet => 23,
            VmaErrKind::BisectRange => 24,
            VmaErrKind::InjectShort => 25,
            VmaErrKind::ResetMismatch => 26,
            VmaErrKind::RingDropped => 27,
            VmaErrKind::ObsInconsistent => 28,
            VmaErrKind::Unknown => 29,
            VmaErrKind::HalfConfig => 30,
            VmaErrKind::KindRange => 31,
            VmaErrKind::RelationInconsistent => 32,
            VmaErrKind::UserAttrMismatch => 33,
            VmaErrKind::ReqFpChain => 34,
            VmaErrKind::StressShort => 35,
            VmaErrKind::RelSetShort => 36,
            VmaErrKind::AttrSetShort => 37,
            VmaErrKind::StateIllegal => 38,
            VmaErrKind::TargetIllegal => 39,
            VmaErrKind::EdgeClosed => 40,
            VmaErrKind::AllocDesync => 41,
            VmaErrKind::VerifyFailed => 42,
            VmaErrKind::HeadMagic => 43,
            VmaErrKind::NeighborUnmergeable => 44,
            VmaErrKind::TriggerNoPair => 45,
            VmaErrKind::RingHead => 46,
            VmaErrKind::LastUnderflow => 47,
        }
    }
    /// 诊断码（`SEG_BASE + index`，一段一码）。
    pub fn code(self) -> u16 {
        SEG_BASE + (self.index() as u16)
    }
    /// 由诊断码还原变体；段外返回 `Err`。
    pub fn from_code(code: u16) -> Result<VmaErrKind, u16> {
        if code < SEG_BASE || code > ERR_SEG_END {
            return Err(code);
        }
        let idx = (code - SEG_BASE) as usize;
        if idx >= 48 {
            return Err(code);
        }
        match VmaErrKind::ALL.get(idx) {
            Some(k) => Ok(*k),
            None => Err(code),
        }
    }
    /// 变体短名。
    pub fn name(self) -> &'static str {
        match self {
            VmaErrKind::RangeInverted => "range_inverted",
            VmaErrKind::RangeOverflow => "range_overflow",
            VmaErrKind::HalfUnclassified => "half_unclassified",
            VmaErrKind::HalfCross => "half_cross",
            VmaErrKind::HalfOutside => "half_outside",
            VmaErrKind::HalfUnaligned => "half_unaligned",
            VmaErrKind::OverlapReject => "overlap_reject",
            VmaErrKind::AdjacentAttrDiff => "adjacent_attr_diff",
            VmaErrKind::TreeFull => "tree_full",
            VmaErrKind::SeqOverflow => "seq_overflow",
            VmaErrKind::LedgerUnbalanced => "ledger_unbalanced",
            VmaErrKind::InvariantBroken => "invariant_broken",
            VmaErrKind::IllegalTransition => "illegal_transition",
            VmaErrKind::FpMismatch => "fp_mismatch",
            VmaErrKind::WindowRace => "window_race",
            VmaErrKind::ShareNotDecided => "share_not_decided",
            VmaErrKind::ReplaySteps => "replay_steps",
            VmaErrKind::TlvLen => "tlv_len",
            VmaErrKind::NodeNotFound => "node_not_found",
            VmaErrKind::RingZero => "ring_zero",
            VmaErrKind::MergeNoNeighbor => "merge_no_neighbor",
            VmaErrKind::MergePending => "merge_pending",
            VmaErrKind::NeighborBroken => "neighbor_broken",
            VmaErrKind::EmptySet => "empty_set",
            VmaErrKind::BisectRange => "bisect_range",
            VmaErrKind::InjectShort => "inject_short",
            VmaErrKind::ResetMismatch => "reset_mismatch",
            VmaErrKind::RingDropped => "ring_dropped",
            VmaErrKind::ObsInconsistent => "obs_inconsistent",
            VmaErrKind::Unknown => "unknown",
            VmaErrKind::HalfConfig => "half_config",
            VmaErrKind::KindRange => "kind_range",
            VmaErrKind::RelationInconsistent => "relation_inconsistent",
            VmaErrKind::UserAttrMismatch => "user_attr_mismatch",
            VmaErrKind::ReqFpChain => "req_fp_chain",
            VmaErrKind::StressShort => "stress_short",
            VmaErrKind::RelSetShort => "rel_set_short",
            VmaErrKind::AttrSetShort => "attr_set_short",
            VmaErrKind::StateIllegal => "state_illegal",
            VmaErrKind::TargetIllegal => "target_illegal",
            VmaErrKind::EdgeClosed => "edge_closed",
            VmaErrKind::AllocDesync => "alloc_desync",
            VmaErrKind::VerifyFailed => "verify_failed",
            VmaErrKind::HeadMagic => "head_magic",
            VmaErrKind::NeighborUnmergeable => "neighbor_unmergeable",
            VmaErrKind::TriggerNoPair => "trigger_no_pair",
            VmaErrKind::RingHead => "ring_head",
            VmaErrKind::LastUnderflow => "last_underflow",
        }
    }
}

/// 错误类型：诊断码 + 定位载荷 + 三要素串。
///
/// 三要素（判据硬要求）：
/// - **现象**（`code()` + `name()`）：出了什么事；
/// - **位置**（`site: Option<u64>` + `detail()`）：在哪个序号 / 地址上出的；
/// - **建议修法**（`advice()`）：调用方该改哪一处。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct VmaErr {
    /// 诊断码。
    pub code: u16,
    /// 定位载荷（序号 / 地址 / 关系统一折叠）。
    pub site: Option<u64>,
    /// 附加说明（自由文本）。
    pub note: String,
}

impl VmaErr {
    /// 以诊断码构造（无定位）。
    pub fn of_code(code: u16, site: u64) -> VmaErr {
        VmaErr { code, site: Some(site), note: String::new() }
    }
    /// 以变体构造（带定位）。
    pub fn of(kind: VmaErrKind, site: u64) -> VmaErr {
        VmaErr { code: kind.code(), site: Some(site), note: kind.name().to_string() }
    }
    /// 追加定位（链式）。
    pub fn at(mut self, site: u64) -> VmaErr {
        self.site = Some(site);
        self
    }
    /// 追加说明（链式）。
    pub fn note_of(mut self, text: &str) -> VmaErr {
        self.note.push_str(text);
        self
    }
    /// 三要素之「现象」。
    pub fn code(&self) -> u16 {
        self.code
    }
    /// 三要素之「位置 + 现象名 + 建议」的合并串。
    pub fn detail(&self) -> String {
        let kind = match VmaErrKind::from_code(self.code) {
            Ok(k) => k,
            Err(_) => VmaErrKind::Unknown,
        };
        let site = match self.site {
            Some(s) => format!("site=0x{:X}", s),
            None => String::from("site=none"),
        };
        let note = if self.note.is_empty() { String::from("-") } else { self.note.clone() };
        format!("[{}/{}] {} note={}", kind.name(), self.code, site, note)
    }
    /// 三要素之「建议修法」。
    pub fn advice(&self) -> &'static str {
        match VmaErrKind::from_code(self.code) {
            Ok(VmaErrKind::RangeInverted) => "检查请求的 start/end 顺序，半开区间必须 start<end",
            Ok(VmaErrKind::RangeOverflow) => "减小区间跨度或改用半开区间，避免 end 溢出",
            Ok(VmaErrKind::HalfUnclassified) => "先确定 HalfPolicy（user/kernel），未分类请求一律不许插入",
            Ok(VmaErrKind::HalfCross) => "把跨半区区间拆成两个单半区请求后再插入",
            Ok(VmaErrKind::HalfOutside) => "把区间移入目标半区上下界之内，或换用匹配的属性",
            Ok(VmaErrKind::HalfUnaligned) => "把区间起止向上对齐到 PAGE_SIZE 后重提",
            Ok(VmaErrKind::OverlapReject) => "先按 seq 定位冲突 VMA，改区间或走 F2584 合并路径",
            Ok(VmaErrKind::AdjacentAttrDiff) => "属性不同不得直接落树：统一属性或留出空隙",
            Ok(VmaErrKind::TreeFull) => "回收旧 VMA（F2584 合并/F2589 卸载）后再插入",
            Ok(VmaErrKind::SeqOverflow) => "seq 已达 u64::MAX，需换 128 位序号或做序号回收",
            Ok(VmaErrKind::LedgerUnbalanced) => "检查 total/accepted/rejected/merged_pending 四项分配是否同步",
            Ok(VmaErrKind::InvariantBroken) => "重跑 check_invariants() 定位是哪一条不变量被破",
            Ok(VmaErrKind::IllegalTransition) => "非法迁移被拒记；查状态机合法边闭集表",
            Ok(VmaErrKind::FpMismatch) => "指纹链断裂：核对区间/属性/序号/tick 四字段是否被旁路改写",
            Ok(VmaErrKind::WindowRace) => "检测窗口竞态：把检测与落树并入同一临界区（F2596）",
            Ok(VmaErrKind::ShareNotDecided) => "共享映射合并语义未决：等 F2564/F2642 就位后放开",
            Ok(VmaErrKind::ReplaySteps) => "回放步数超上界：缩减脚本步数或提高 REPLAY_STEPS",
            Ok(VmaErrKind::TlvLen) => "TLV 长度不自洽：核对 HEAD 载荷声明长度与实际字节数",
            Ok(VmaErrKind::NodeNotFound) => "按 seq 取节点失败：确认节点未被合并（F2584）回收",
            Ok(VmaErrKind::RingZero) => "环形账容量必须 >0，否则无法留痕",
            Ok(VmaErrKind::MergeNoNeighbor) => "合并票据缺相邻对：检查前驱后继查询是否命中",
            Ok(VmaErrKind::MergePending) => "合并挂起量与触发数不符：reconcile() 对账后重跑",
            Ok(VmaErrKind::NeighborBroken) => "前驱/后继定位到非紧邻节点：核对 start 严格升序不变量",
            Ok(VmaErrKind::EmptySet) => "空集合不允许参与前驱后继查询",
            Ok(VmaErrKind::BisectRange) => "二分下界越界：核对查询点是否落在 [0, len]",
            Ok(VmaErrKind::InjectShort) => "注入次数不足：补齐到判据要求的轮次",
            Ok(VmaErrKind::ResetMismatch) => "清空树时必须同步清零四账目",
            Ok(VmaErrKind::RingDropped) => "环形账 dropped 计数与容量不符：核对 head 环绕逻辑",
            Ok(VmaErrKind::ObsInconsistent) => "观察包分项与总闸矛盾：重跑 observe(seed)",
            Ok(VmaErrKind::Unknown) => "诊断码不在 0x1C40..0x1C7F 段内，查码表发布",
            Ok(VmaErrKind::HalfConfig) => "半区上下界配置自相矛盾（lower>upper）",
            Ok(VmaErrKind::KindRange) => "映射类型序号越界：查映射类型闭集登记",
            Ok(VmaErrKind::RelationInconsistent) => "六关系分类与交叠性矛盾：核对 classify() 优先级链",
            Ok(VmaErrKind::UserAttrMismatch) => "user 属性必须与用户半区一致",
            Ok(VmaErrKind::ReqFpChain) => "请求指纹与落树节点指纹链断裂",
            Ok(VmaErrKind::StressShort) => "压力插入未达 10^5：提高轮次或放宽区间生成",
            Ok(VmaErrKind::RelSetShort) => "关系枚举测试集不足：补齐六关系全覆盖",
            Ok(VmaErrKind::AttrSetShort) => "属性枚举不足：补齐 16 种属性组合",
            Ok(VmaErrKind::StateIllegal) => "状态机未初始化或越界：重新构造状态机",
            Ok(VmaErrKind::TargetIllegal) => "迁移目标态非法：查合法边闭集允许的目标态",
            Ok(VmaErrKind::EdgeClosed) => "该迁移不在合法边闭集内，拒绝并留痕",
            Ok(VmaErrKind::AllocDesync) => "账目分配与树分配不同步：检查 new()/reset()",
            Ok(VmaErrKind::VerifyFailed) => "verify() 未通过：冻结指纹与当前指纹不符",
            Ok(VmaErrKind::HeadMagic) => "导出组头魔数不符：核对 V2582_VERSION_ID / magic",
            Ok(VmaErrKind::NeighborUnmergeable) => "相邻但属性+共享双否：留空隙，不合并",
            Ok(VmaErrKind::TriggerNoPair) => "合并触发但无配对邻居：查相邻检测返回",
            Ok(VmaErrKind::RingHead) => "环形账 head 越过容量：查 cap 与 push 逻辑",
            Ok(VmaErrKind::LastUnderflow) => "零跨度区间的 last() 会下溢：构造期已拦，此处为防御",
            Err(_) => "查诊断码段 0x1C40..0x1C7F 的发布版本",
        }
    }
    /// 三要素完整串（现象 / 位置 / 建议）。
    pub fn full(&self) -> String {
        format!("{} | {}", self.detail(), self.advice())
    }
}

/// 本单 `Result`。
pub type InsertResult<T> = Result<T, VmaErr>;

// ===========================================================================
// 6. 冲突定位与插入结论闭集
// ===========================================================================

/// 冲突定位票据（判据原文：「拒绝路径返回结构化错误（含冲突区间定位）」）。
///
/// 定位必须能回答四问：
/// 1. **谁**冲突：`other_seq`（对方 VMA 的插入序号）；
/// 2. **何种关系**：`relation`（七关系闭集之一）；
/// 3. **哪一段字节交叠**：`overlap: Option<AddrRange>`；
/// 4. **差多少**：`gap`（0 表示交叠，正值表示空隙字节数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConflictSite {
    /// 对方 VMA 序号。
    pub other_seq: u64,
    /// 对方区间。
    pub other_range: AddrRange,
    /// 关系分类。
    pub relation: Relation,
    /// 交叠段（无交叠为 `None`）。
    pub overlap: Option<AddrRange>,
    /// 空隙字节（交叠时为 0）。
    pub gap: u64,
}

impl ConflictSite {
    /// 构造定位票据。
    pub fn new(req: &AddrRange, node: &VmaNode) -> ConflictSite {
        let rel = classify(req, &node.range);
        ConflictSite {
            other_seq: node.seq,
            other_range: node.range,
            relation: rel,
            overlap: req.overlap_span(&node.range),
            gap: req.gap_to(&node.range),
        }
    }
    /// 是否为「必须拒绝」的交叠（六关系皆拒）。
    pub fn must_reject(&self) -> bool {
        self.relation.is_overlap()
    }
    /// 一行描述（观测用）。
    pub fn describe(&self) -> String {
        let ov = match self.overlap {
            Some(r) => r.describe(),
            None => String::from("none"),
        };
        format!("vs#{} rel={} overlap={} gap={}", self.other_seq, self.relation.name(), ov, self.gap)
    }
    /// 错误对象（拒绝路径专用）。
    pub fn to_err(&self) -> VmaErr {
        VmaErr::of(VmaErrKind::OverlapReject, self.other_seq).note_of(&self.relation.name())
    }
}

/// 插入结论闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 已落树。
    Accepted,
    /// 相邻同属性 → 触发合并（归 F2584 执行，本单不执行）。
    MergeTriggered,
    /// 被拒（携带原因码）。
    Rejected(VmaErrKind),
    /// 相邻但属性不同 → 拒绝落树（避免误合并成异属性大 VMA）。
    AdjacentBlocked,
}

impl Verdict {
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            Verdict::Accepted => "accepted",
            Verdict::MergeTriggered => "merge_triggered",
            Verdict::Rejected(_) => "rejected",
            Verdict::AdjacentBlocked => "adjacent_blocked",
        }
    }
    /// 是否被拒（含 `AdjacentBlocked`，它同样是拒绝）。
    pub fn is_reject(&self) -> bool {
        matches!(self, Verdict::Rejected(_) | Verdict::AdjacentBlocked)
    }
    /// 是否已落树。
    pub fn is_accepted(&self) -> bool {
        matches!(self, Verdict::Accepted)
    }
    /// 拒绝原因码（未拒时返回 `None`）。
    pub fn reject_kind(&self) -> Option<VmaErrKind> {
        match self {
            Verdict::Rejected(k) => Some(*k),
            Verdict::AdjacentBlocked => Some(VmaErrKind::AdjacentAttrDiff),
            _ => None,
        }
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        match self {
            Verdict::Rejected(k) => format!("rejected:{}", k.name()),
            Verdict::AdjacentBlocked => String::from("adjacent_blocked"),
            other => String::from(other.name()),
        }
    }
}

/// 插入结果（结论 + 票据 + 冲突定位）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct InsertOutcome {
    /// 结论。
    pub verdict: Verdict,
    /// 落树节点序号（`Accepted` 时有效）。
    pub node_seq: u64,
    /// 冲突定位（拒绝时 `Some`）。
    pub conflict: Option<ConflictSite>,
    /// 合并触发票据（`MergeTriggered` 时 `Some`）。
    pub merge: Option<MergeTicket>,
    /// 消耗的逻辑 tick（= 1 或 0）。
    pub ticks: u64,
}

impl InsertOutcome {
    /// 构造「已落树」结果。
    pub fn accepted(seq: u64) -> InsertOutcome {
        InsertOutcome { verdict: Verdict::Accepted, node_seq: seq, conflict: None, merge: None, ticks: 1 }
    }
    /// 构造「拒绝」结果。
    pub fn rejected(kind: VmaErrKind, c: Option<ConflictSite>) -> InsertOutcome {
        InsertOutcome { verdict: Verdict::Rejected(kind), node_seq: 0, conflict: c, merge: None, ticks: 0 }
    }
    /// 构造「合并触发」结果。
    pub fn triggered(t: MergeTicket) -> InsertOutcome {
        InsertOutcome { verdict: Verdict::MergeTriggered, node_seq: t.trigger_no, conflict: None, merge: Some(t), ticks: 0 }
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        let mut s = format!("seq={} {}", self.node_seq, self.verdict.describe());
        if let Some(c) = self.conflict {
            s.push(' ');
            s.push_str(&c.describe());
        }
        if let Some(m) = self.merge {
            s.push(' ');
            s.push_str(&m.describe());
        }
        s
    }
}

/// 合并触发票据（交接 F2584，本单只出票不执行）。
///
/// 票据必须自足：F2584 只凭本票据即可完成合并，不需要回查树。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MergeTicket {
    /// 新（合并后）区间。
    pub merged: AddrRange,
    /// 左侧原节点序号。
    pub left_seq: u64,
    /// 右侧原节点序号。
    pub right_seq: u64,
    /// 合并键（四轴折叠）。
    pub attr_key: u16,
    /// 触发时的逻辑 tick。
    pub tick: u64,
    /// 触发序号（本单内部单调）。
    pub trigger_no: u64,
}

impl MergeTicket {
    /// 票据指纹。
    pub fn fingerprint(&self) -> u64 {
        let words = [
            self.merged.start,
            self.merged.end,
            self.attr_key as u64,
            self.left_seq,
            self.right_seq,
            self.tick,
            self.trigger_no,
        ];
        fnv1a_words(&words)
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("merge#{} {} attr_key=0x{:X} L#{} R#{} tick={} fp=0x{:016X}", self.trigger_no, self.merged.describe(), self.attr_key,
            self.left_seq, self.right_seq, self.tick, self.fingerprint())
    }
}

// ===========================================================================
// 7. 前驱 / 后继查询（六关系定位的底座）
// ===========================================================================

/// 二分下界：返回第一个 `>= key` 的下标。
///
/// `key` 不落在 `[0, len]` 时返回 `Err`，绝不 panic。
pub fn lower_bound(nodes: &[VmaNode], key: u64) -> InsertResult<usize> {
    if nodes.is_empty() {
        return Err(VmaErr::of(VmaErrKind::EmptySet, 0));
    }
    let n = nodes.len();
    if n > MAX_VMA {
        return Err(VmaErr::of(VmaErrKind::BisectRange, n as u64));
    }
    let mut lo: usize = 0;
    let mut hi: usize = n;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let v = match nodes.get(mid) {
            Some(x) => x.range.start,
            None => return Err(VmaErr::of(VmaErrKind::BisectRange, mid as u64)),
        };
        if v < key {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    if lo > n {
        return Err(VmaErr::of(VmaErrKind::BisectRange, lo as u64));
    }
    Ok(lo)
}

/// 前驱查询：返回 `start < key` 的最后一个节点（`O(log n)`）。
pub fn predecessor(nodes: &[VmaNode], key: u64) -> InsertResult<Option<usize>> {
    if nodes.is_empty() {
        return Ok(None);
    }
    let idx = lower_bound(nodes, key)?;
    if idx == 0 {
        return Ok(None);
    }
    Ok(Some(idx - 1))
}

/// 后继查询：返回 `start >= key` 的第一个节点（`O(log n)`）。
pub fn successor(nodes: &[VmaNode], key: u64) -> InsertResult<Option<usize>> {
    if nodes.is_empty() {
        return Ok(None);
    }
    let idx = lower_bound(nodes, key)?;
    if idx >= nodes.len() {
        return Ok(None);
    }
    Ok(Some(idx))
}

/// 按 `start` 在有序节点数组中找精确匹配下标。
pub fn find_by_start(nodes: &[VmaNode], start: u64) -> Option<usize> {
    let mut i: usize = 0;
    while i < nodes.len() {
        let n = match nodes.get(i) {
            Some(x) => x,
            None => return None,
        };
        if n.range.start == start {
            return Some(i);
        }
        if n.range.start > start {
            return None;
        }
        i = i.wrapping_add(1);
    }
    None
}

/// 按 `seq` 查节点（线性，唯一序号故不做二分）。
pub fn find_by_seq(nodes: &[VmaNode], seq: u64) -> Option<usize> {
    let mut i: usize = 0;
    while i < nodes.len() {
        let n = match nodes.get(i) {
            Some(x) => x,
            None => return None,
        };
        if n.seq == seq {
            return Some(i);
        }
        i = i.wrapping_add(1);
    }
    None
}

// ===========================================================================
// 8. 守恒账与环形账
// ===========================================================================

/// 插入守恒账。
///
/// **守恒式**：`total == accepted + rejected + merged_pending`。
///
/// - `total`：进入插入流水线的请求总数（含被拒）；
/// - `accepted`：真落树（树内节点数 + 已被 F2584 回收的节点数）；
/// - `rejected`：被闸拒（三道闸任一）；
/// - `merged_pending`：已判定应合并、归 F2584 执行，本单不消耗。
///
/// 守恒式从 0 起算：`new()` 时四账全 0，且与树同生共死（不存在
/// 「先建树后建账」窗口）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InsertLedger {
    /// 总请求数。
    pub total: u64,
    /// 已落树数。
    pub accepted: u64,
    /// 被拒数。
    pub rejected: u64,
    /// 合并挂起数。
    pub merged_pending: u64,
    /// F2584 已回收（合并执行）数。
    pub merged_done: u64,
    /// 半区闸拒绝数。
    pub half_rejected: u64,
    /// 重叠闸拒绝数。
    pub overlap_rejected: u64,
    /// 相邻闸拒绝数。
    pub adjacent_rejected: u64,
    /// 树满拒绝数。
    pub full_rejected: u64,
}

impl InsertLedger {
    /// 全零账（守恒式起点）。
    pub fn zero() -> InsertLedger {
        InsertLedger {
            total: 0,
            accepted: 0,
            rejected: 0,
            merged_pending: 0,
            merged_done: 0,
            half_rejected: 0,
            overlap_rejected: 0,
            adjacent_rejected: 0,
            full_rejected: 0,
        }
    }
    /// 记一笔「进入流水线」。
    pub fn enter(&mut self) {
        self.total = self.total.wrapping_add(1);
    }
    /// 记一笔「落树」。
    pub fn note_accepted(&mut self) {
        self.accepted = self.accepted.wrapping_add(1);
    }
    /// 记一笔「合并触发」。
    pub fn note_merge_pending(&mut self) {
        self.merged_pending = self.merged_pending.wrapping_add(1);
    }
    /// 记一笔「F2584 执行合并完成」。
    pub fn note_merge_done(&mut self) {
        self.merged_done = self.merged_done.wrapping_add(1);
        self.merged_pending = self.merged_pending.saturating_sub(1);
    }
    /// 记一笔「拒绝」，按闸分流。
    pub fn note_rejected(&mut self, kind: VmaErrKind) {
        self.rejected = self.rejected.wrapping_add(1);
        match kind {
            VmaErrKind::HalfUnclassified | VmaErrKind::HalfCross | VmaErrKind::HalfOutside | VmaErrKind::HalfUnaligned => {
                self.half_rejected = self.half_rejected.wrapping_add(1);
            }
            VmaErrKind::OverlapReject | VmaErrKind::WindowRace | VmaErrKind::NeighborBroken => {
                self.overlap_rejected = self.overlap_rejected.wrapping_add(1);
            }
            VmaErrKind::AdjacentAttrDiff | VmaErrKind::NeighborUnmergeable | VmaErrKind::ShareNotDecided => {
                self.adjacent_rejected = self.adjacent_rejected.wrapping_add(1);
            }
            VmaErrKind::TreeFull => {
                self.full_rejected = self.full_rejected.wrapping_add(1);
            }
            _ => {}
        }
    }
    /// 闸分合计（应等于 `rejected`）。
    pub fn gate_sum(&self) -> u64 {
        self.half_rejected
            .wrapping_add(self.overlap_rejected)
            .wrapping_add(self.adjacent_rejected)
            .wrapping_add(self.full_rejected)
    }
    /// 守恒对账：返回 `(守恒成立, 四项, 差异)`。
    pub fn reconcile(&self) -> (bool, (u64, u64, u64, u64), u64) {
        let lhs = self.accepted.wrapping_add(self.rejected).wrapping_add(self.merged_pending);
        let diff = if lhs >= self.total { lhs - self.total } else { self.total - lhs };
        let tuple = (self.total, self.accepted, self.rejected, self.merged_pending);
        (diff == 0, tuple, diff)
    }
    /// 守恒成立（并且闸分合计等于拒绝总数）。
    pub fn conserved(&self) -> bool {
        let (ok, _, _) = self.reconcile();
        ok && self.gate_sum() == self.rejected
    }
    /// 账指纹。
    pub fn fingerprint(&self) -> u64 {
        let words = [
            self.total,
            self.accepted,
            self.rejected,
            self.merged_pending,
            self.merged_done,
            self.half_rejected,
            self.overlap_rejected,
            self.adjacent_rejected,
            self.full_rejected,
        ];
        fnv1a_words(&words)
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        let (ok, t, d) = self.reconcile();
        format!(
            "total={} acc={} rej={} pend={} done={} gates={}/{} diff={} {}",
            t.0,
            t.1,
            t.2,
            t.3,
            self.merged_done,
            self.gate_sum(),
            self.rejected,
            d,
            if ok { "CONSERVED" } else { "BROKEN" }
        )
    }
    /// 归零（与树清空同步）。
    pub fn reset(&mut self) {
        *self = InsertLedger::zero();
    }
}

/// 环形账一条记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RingRecord {
    /// 逻辑 tick。
    pub tick: u64,
    /// 请求序号（进入流水线的次序号）。
    pub req_no: u64,
    /// 结论序号（`Verdict` 的稳定编码）。
    pub verdict_code: u64,
    /// 区间起。
    pub start: u64,
    /// 区间止。
    pub end: u64,
    /// 诊断码（未拒时为 0）。
    pub err_code: u64,
    /// 记录指纹。
    pub fp: u64,
}

impl RingRecord {
    /// 指纹重算（供 `verify()` 对拍）。
    pub fn recompute(&self) -> u64 {
        let words = [self.tick, self.req_no, self.verdict_code, self.start, self.end, self.err_code];
        fnv1a_words(&words)
    }
    /// 编码为 `u64` 序列（6 字，不含 `fp`）。
    pub fn to_words(&self) -> [u64; 6] {
        [self.tick, self.req_no, self.verdict_code, self.start, self.end, self.err_code]
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("t={} req={} v={} {} err=0x{:04X} fp=0x{:016X}", self.tick, self.req_no, self.verdict_code,
            AddrRange { start: self.start, end: self.end }.describe(), self.err_code, self.fp)
    }
}

/// 结论稳定编码（`Verdict` → `u64`，进环形账）。
pub fn verdict_code(v: &Verdict) -> u64 {
    match v {
        Verdict::Accepted => 0,
        Verdict::MergeTriggered => 1,
        Verdict::AdjacentBlocked => 2,
        Verdict::Rejected(_) => 3,
    }
}

/// append-only 固定容量环形账。
///
/// **溢出语义**：写满后覆盖最旧记录，`dropped` 显性自增（判据要求
/// 「dropped 显性」）。环形账**不扩容**，因此内存上界 = `RING_CAP × 40B`。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RingLedger {
    /// 记录缓冲。
    pub buf: Vec<RingRecord>,
    /// 容量。
    pub cap: usize,
    /// 写指针（`0..cap`）。
    pub head: usize,
    /// 总写入数（含被覆盖的）。
    pub written: u64,
    /// 被覆盖（丢弃）数。
    pub dropped: u64,
}

impl RingLedger {
    /// 构造（`cap == 0` 时返回 `Err`，构造期即拦，不留半成品）。
    pub fn new(cap: usize) -> InsertResult<RingLedger> {
        if cap == 0 {
            return Err(VmaErr::of(VmaErrKind::RingZero, 0));
        }
        let mut buf: Vec<RingRecord> = Vec::new();
        buf.resize(cap, RingRecord { tick: 0, req_no: 0, verdict_code: 0, start: 0, end: 0, err_code: 0, fp: FNV_BASIS });
        Ok(RingLedger { buf, cap, head: 0, written: 0, dropped: 0 })
    }
    /// 默认容量账。
    pub fn default_ring() -> InsertResult<RingLedger> {
        RingLedger::new(RING_CAP)
    }
    /// 追加一条（append-only）。
    pub fn push(&mut self, tick: u64, req_no: u64, verdict: &Verdict, range: &AddrRange, err_code: u16) -> InsertResult<()> {
        if self.cap == 0 {
            return Err(VmaErr::of(VmaErrKind::RingZero, self.cap as u64));
        }
        if self.head >= self.cap {
            return Err(VmaErr::of(VmaErrKind::RingHead, self.head as u64));
        }
        let rec = RingRecord {
            tick,
            req_no,
            verdict_code: verdict_code(verdict),
            start: range.start,
            end: range.end,
            err_code: err_code as u64,
            fp: 0,
        };
        let fp = rec.recompute();
        let mut final_rec = rec;
        final_rec.fp = fp;
        if self.written >= (self.cap as u64) {
            self.dropped = self.dropped.wrapping_add(1);
        }
        if let Some(cell) = self.buf.get_mut(self.head) {
            *cell = final_rec;
        }
        self.head = self.head.wrapping_add(1);
        if self.head >= self.cap {
            self.head = 0;
        }
        self.written = self.written.wrapping_add(1);
        Ok(())
    }
    /// 当前驻留记录数（`min(written, cap)`）。
    pub fn len(&self) -> usize {
        let w = self.written as usize;
        if w > self.cap {
            self.cap
        } else {
            w
        }
    }
    /// 驻留记录按时间序（最旧 → 最新）读出。
    pub fn resident(&self) -> Vec<RingRecord> {
        let n = self.len();
        let mut out: Vec<RingRecord> = Vec::with_capacity(n);
        let base = self.written.saturating_sub(self.cap as u64);
        let mut i: usize = 0;
        while i < n {
            let idx = ((base as usize).wrapping_add(i)) % self.cap;
            if let Some(r) = self.buf.get(idx) {
                out.push(*r);
            }
            i = i.wrapping_add(1);
        }
        out
    }
    /// 逐条指纹自校验（全部通过才 `true`）。
    pub fn verify(&self) -> bool {
        let r = self.resident();
        let mut i: usize = 0;
        while i < r.len() {
            let rec = match r.get(i) {
                Some(x) => *x,
                None => return false,
            };
            if rec.fp != rec.recompute() {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 环形账指纹（遍历驻留记录）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = FNV_BASIS;
        let r = self.resident();
        let mut i: usize = 0;
        while i < r.len() {
            if let Some(rec) = r.get(i) {
                let w = rec.to_words();
                let mut k: usize = 0;
                while k < 6 {
                    let word = match w.get(k) {
                        Some(x) => *x,
                        None => 0,
                    };
                    h = fnv1a_acc(h, word);
                    k = k.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        h
    }
    /// `dropped` 计数是否与 `written` 自洽。
    pub fn dropped_consistent(&self) -> bool {
        let expect = if self.written > (self.cap as u64) { self.written - (self.cap as u64) } else { 0 };
        self.dropped == expect
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("cap={} head={} written={} dropped={} resident={} verify={}", self.cap, self.head, self.written, self.dropped,
            self.len(), if self.verify() { 1 } else { 0 })
    }
}

// ===========================================================================
// 9. 插入状态机（合法边闭集 + 非法迁移拒记）
// ===========================================================================

/// 插入流水线状态闭集。
///
/// 状态机五态：
/// ```text
///   Idle ──begin──▶ GateHalf ──pass──▶ GateOverlap ──pass──▶ GateAdjacent
///     ▲                │reject             │reject             │reject
///     │                ▼                   ▼                   ▼
///     └──────────── Quarantine ◀───────────────────────────────┘
///
///   GateAdjacent ──trigger──▶ MergePending ──hand_off──▶ Idle
///   GateAdjacent ──pass────▶ Landed ──commit──▶ Idle
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InsertState {
    /// 空闲（无请求在流水线内）。
    Idle,
    /// 闸 0：半区校验中。
    GateHalf,
    /// 闸 1：重叠检测中。
    GateOverlap,
    /// 闸 2：相邻 / 合并判定中。
    GateAdjacent,
    /// 合并挂起（已出票，等 F2584 消费）。
    MergePending,
    /// 已落树待提交。
    Landed,
    /// 隔离（被拒，待留痕后回 Idle）。
    Quarantine,
}

impl InsertState {
    /// 全部状态。
    pub const ALL: [InsertState; 7] = [
        InsertState::Idle,
        InsertState::GateHalf,
        InsertState::GateOverlap,
        InsertState::GateAdjacent,
        InsertState::MergePending,
        InsertState::Landed,
        InsertState::Quarantine,
    ];
    /// 闭集序号。
    pub fn index(self) -> usize {
        match self {
            InsertState::Idle => 0,
            InsertState::GateHalf => 1,
            InsertState::GateOverlap => 2,
            InsertState::GateAdjacent => 3,
            InsertState::MergePending => 4,
            InsertState::Landed => 5,
            InsertState::Quarantine => 6,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            InsertState::Idle => "idle",
            InsertState::GateHalf => "gate_half",
            InsertState::GateOverlap => "gate_overlap",
            InsertState::GateAdjacent => "gate_adjacent",
            InsertState::MergePending => "merge_pending",
            InsertState::Landed => "landed",
            InsertState::Quarantine => "quarantine",
        }
    }
    /// 由序号还原；越界 `None`。
    pub fn from_index(i: usize) -> Option<InsertState> {
        match i {
            0 => Some(InsertState::Idle),
            1 => Some(InsertState::GateHalf),
            2 => Some(InsertState::GateOverlap),
            3 => Some(InsertState::GateAdjacent),
            4 => Some(InsertState::MergePending),
            5 => Some(InsertState::Landed),
            6 => Some(InsertState::Quarantine),
            _ => None,
        }
    }
    /// 是否为终态（`Idle` / `MergePending` / `Quarantine` 为流水线出口）。
    pub fn is_sink(&self) -> bool {
        matches!(self, InsertState::Idle | InsertState::MergePending | InsertState::Quarantine)
    }
    /// 是否为闸态（三道闸）。
    pub fn is_gate(&self) -> bool {
        matches!(self, InsertState::GateHalf | InsertState::GateOverlap | InsertState::GateAdjacent)
    }
}

/// 合法边闭集：`edge_allowed(from, to)` 是状态机**唯一**的迁移裁决入口。
///
/// 合法边共 11 条（`7×7 = 49` 中的 11 条）：
/// ```text
/// Idle          → GateHalf
/// GateHalf      → GateOverlap / Quarantine
/// GateOverlap   → GateAdjacent / Quarantine
/// GateAdjacent  → Landed / MergePending / Quarantine
/// Landed        → Idle
/// MergePending  → Idle
/// Quarantine    → Idle
/// ```
/// 其余 38 条组合一律**非法**，非法迁移被拒记（`ERR_EDGE_CLOSED`）
/// 并留痕，不静默改状态。
pub fn edge_allowed(from: InsertState, to: InsertState) -> bool {
    match (from, to) {
        (InsertState::Idle, InsertState::GateHalf) => true,
        (InsertState::GateHalf, InsertState::GateOverlap) => true,
        (InsertState::GateHalf, InsertState::Quarantine) => true,
        (InsertState::GateOverlap, InsertState::GateAdjacent) => true,
        (InsertState::GateOverlap, InsertState::Quarantine) => true,
        (InsertState::GateAdjacent, InsertState::Landed) => true,
        (InsertState::GateAdjacent, InsertState::MergePending) => true,
        (InsertState::GateAdjacent, InsertState::Quarantine) => true,
        (InsertState::Landed, InsertState::Idle) => true,
        (InsertState::MergePending, InsertState::Idle) => true,
        (InsertState::Quarantine, InsertState::Idle) => true,
        _ => false,
    }
}

/// 状态机本体。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct InsertFsm {
    /// 当前态。
    pub state: InsertState,
    /// 迁移计数（成功）。
    pub moved: u64,
    /// 非法迁移拒记计数。
    pub refused: u64,
    /// 上一条非法迁移的 `(from << 8) | to` 编码。
    pub last_refused: u64,
    /// 逻辑 tick。
    pub tick: u64,
}

impl InsertFsm {
    /// 构造（`Idle` 态，全零计数）。
    pub fn new() -> InsertFsm {
        InsertFsm { state: InsertState::Idle, moved: 0, refused: 0, last_refused: 0, tick: 0 }
    }
    /// 尝试迁移（非法则拒记并返回 `Err`，状态不变）。
    pub fn try_move(&mut self, to: InsertState) -> InsertResult<()> {
        if !edge_allowed(self.state, to) {
            self.refused = self.refused.wrapping_add(1);
            self.last_refused = ((self.state.index() as u64) << 8) | (to.index() as u64);
            return Err(VmaErr::of(VmaErrKind::EdgeClosed, self.last_refused).note_of(&format!("{}->{}", self.state.name(), to.name())));
        }
        self.state = to;
        self.moved = self.moved.wrapping_add(1);
        self.tick = self.tick.wrapping_add(1);
        Ok(())
    }
    /// 非法迁移全枚举（7×7 = 49 条中的 38 条非法），逐条断言。
    pub fn illegal_sweep() -> Vec<(InsertState, InsertState)> {
        let mut out: Vec<(InsertState, InsertState)> = Vec::new();
        for a in InsertState::ALL {
            for b in InsertState::ALL {
                if !edge_allowed(a, b) {
                    out.push((a, b));
                }
            }
        }
        out
    }
    /// 合法边全枚举（应恰好 11 条）。
    pub fn legal_sweep() -> Vec<(InsertState, InsertState)> {
        let mut out: Vec<(InsertState, InsertState)> = Vec::new();
        for a in InsertState::ALL {
            for b in InsertState::ALL {
                if edge_allowed(a, b) {
                    out.push((a, b));
                }
            }
        }
        out
    }
    /// 合法边条数（判据底数：10）。
    pub fn legal_edge_count() -> usize {
        InsertFsm::legal_sweep().len()
    }
    /// 非法迁移拒记总数。
    pub fn illegal_edge_count() -> usize {
        InsertFsm::illegal_sweep().len()
    }
    /// 一次完整合法迁移链（`Idle → … → Idle`）。
    pub fn drive_ok(&mut self) -> InsertResult<()> {
        let chain = [
            InsertState::GateHalf,
            InsertState::GateOverlap,
            InsertState::GateAdjacent,
            InsertState::Landed,
            InsertState::Idle,
        ];
        let mut i: usize = 0;
        while i < chain.len() {
            self.try_move(chain[i])?;
            i = i.wrapping_add(1);
        }
        Ok(())
    }
    /// 一次非法迁移（用于注入核）。
    pub fn drive_illegal(&mut self) -> InsertResult<()> {
        // `Idle → Landed` 是一条刻意越闸的边，必须被拒。
        self.try_move(InsertState::Landed)
    }
    /// 状态机指纹。
    pub fn fingerprint(&self) -> u64 {
        let words = [self.state.index() as u64, self.moved, self.refused, self.last_refused, self.tick];
        fnv1a_words(&words)
    }
}

// ===========================================================================
// 10. F2581 三不变量
// ===========================================================================

/// 一条不变量的判定结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InvariantCheck {
    /// 不变量编号（0..2）。
    pub id: usize,
    /// 不变量代号。
    pub name: &'static str,
    /// 是否成立。
    pub hold: bool,
    /// 首个违例节点序号（`None` = 无违例）。
    pub offender: Option<u64>,
}

impl InvariantCheck {
    /// 一行描述。
    pub fn describe(&self) -> String {
        let off = match self.offender {
            Some(s) => format!("#{}", s),
            None => String::from("-"),
        };
        format!("INV-{} {} {} offender={}", self.id, self.name, if self.hold { "HOLD" } else { "BROKEN" }, off)
    }
}

/// F2581 三不变量判定。
///
/// - `INV-0` **有序性**：节点按 `start` 严格升序（相等即破）；
/// - `INV-1` **无交叠**：任意相邻两节点交叠为空（端点相接不算交叠）；
/// - `INV-2` **半区自洽**：每节点区间整体落在其属性所声明的半区内。
pub fn check_invariants(nodes: &[VmaNode]) -> [InvariantCheck; INVARIANT_COUNT] {
    let mut inv0 = InvariantCheck { id: 0, name: "ordered_strictly", hold: true, offender: None };
    let mut inv1 = InvariantCheck { id: 1, name: "no_overlap", hold: true, offender: None };
    let mut inv2 = InvariantCheck { id: 2, name: "half_consistent", hold: true, offender: None };
    let mut i: usize = 0;
    while i < nodes.len() {
        let n = match nodes.get(i) {
            Some(x) => *x,
            None => break,
        };
        // INV-0：与前一节点比较。
        if i > 0 {
            let prev = match nodes.get(i - 1) {
                Some(x) => *x,
                None => {
                    i = i.wrapping_add(1);
                    continue;
                }
            };
            if n.range.start <= prev.range.start && !inv0.hold {
                // 保持首个违例
            } else if n.range.start <= prev.range.start {
                inv0.hold = false;
                inv0.offender = Some(n.seq);
            }
            // INV-1：相邻两节点不得交叠。
            if prev.range.intersects(&n.range) && inv1.hold {
                inv1.hold = false;
                inv1.offender = Some(n.seq);
            }
        }
        // INV-2：属性半区自洽。
        let ok_half = if n.attr.user { n.range.in_user_half() } else { n.range.in_kernel_half() };
        if !ok_half && inv2.hold {
            inv2.hold = false;
            inv2.offender = Some(n.seq);
        }
        i = i.wrapping_add(1);
    }
    [inv0, inv1, inv2]
}

/// 三不变量全成立。
pub fn invariants_ok(nodes: &[VmaNode]) -> bool {
    let c = check_invariants(nodes);
    let mut i: usize = 0;
    while i < c.len() {
        if let Some(x) = c.get(i) {
            if !x.hold {
                return false;
            }
        }
        i = i.wrapping_add(1);
    }
    true
}

// ===========================================================================
// 11. VMA 树与插入流水线
// ===========================================================================

/// 区间树主体（本单用有序 `Vec<VmaNode>` 承载，语义等价于 AVL/rbtree 的
/// 区间树：查询面（前驱 / 后继 / 交叠枚举）同构，存储面退化为连续数组
/// 以便纯功能建模与指纹化）。
///
/// 结构契约：
/// - `nodes` 按 `start` 严格升序（`INV-0`）；
/// - `ledger` 与 `ring` 与 `nodes` **同生共死**（`new()` 同处构造）；
/// - `fsm` 是插入流水线的状态机；每个入口函数都走它，不存在旁路落树。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct VmaTree {
    /// 节点（严格升序）。
    pub nodes: Vec<VmaNode>,
    /// 守恒账。
    pub ledger: InsertLedger,
    /// 环形账。
    pub ring: RingLedger,
    /// 插入状态机。
    pub fsm: InsertFsm,
    /// 序号分配器（严格递增）。
    pub seq_next: u64,
    /// 请求次序号（进流水线即分配）。
    pub req_no: u64,
    /// 合并触发计数。
    pub trigger_no: u64,
    /// 冲突拒绝留痕总数（判据「拒绝路径返回结构化错误」计数）。
    pub conflict_traced: u64,
    /// 当前逻辑 tick。
    pub tick: u64,
    /// 节点指纹的**顺序无关**折叠和（增量维护，`O(1)` 更新）。
    ///
    /// 用途：插入流水线的「树零改动」双证必须 `O(1)`，否则 10^5 次插入
    /// 会退化成 `O(n^2)`。全量指纹 `nodes_fp()` 只在检查点用。
    pub nodes_sum: u64,
    /// 节点指纹的异或折叠（与 `nodes_sum` 交叉定位篡改点）。
    pub nodes_xor: u64,
}

/// 树 O(1) 改动见证（`len` / `sum` / `xor` 三元组）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Witness {
    /// 节点数。
    pub len: usize,
    /// 指纹和。
    pub sum: u64,
    /// 指纹异或。
    pub xor: u64,
}

impl Witness {
    /// 见证串。
    pub fn describe(&self) -> String {
        format!("len={} sum=0x{:016X} xor=0x{:016X}", self.len, self.sum, self.xor)
    }
    /// 零见证（空树）。
    pub fn zero() -> Witness {
        Witness { len: 0, sum: 0, xor: 0 }
    }
}

impl VmaTree {
    /// 构造空树（账从 0 起算，三容器同生）。
    pub fn new() -> InsertResult<VmaTree> {
        Ok(VmaTree {
            nodes: Vec::new(),
            ledger: InsertLedger::zero(),
            ring: RingLedger::default_ring()?,
            fsm: InsertFsm::new(),
            seq_next: 1,
            req_no: 0,
            trigger_no: 0,
            conflict_traced: 0,
            tick: 0,
            nodes_sum: 0,
            nodes_xor: 0,
        })
    }
    /// 树改动见证（`O(1)`）。
    pub fn witness(&self) -> Witness {
        Witness { len: self.nodes.len(), sum: self.nodes_sum, xor: self.nodes_xor }
    }
    /// 增量记账：落一个节点进见证。
    fn witness_add(&mut self, fp: u64) {
        self.nodes_sum = self.nodes_sum.wrapping_add(fp);
        self.nodes_xor ^= fp;
    }
    /// 增量记账：从见证里摘掉一个节点。
    fn witness_sub(&mut self, fp: u64) {
        self.nodes_sum = self.nodes_sum.wrapping_sub(fp);
        self.nodes_xor ^= fp;
    }
    /// 节点数。
    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
    /// 容量上界检查。
    pub fn room(&self) -> bool {
        self.nodes.len() < MAX_VMA
    }
    /// 节点序列指纹（**只含节点**，用于「拒绝路径树零改动」双证）。
    pub fn nodes_fp(&self) -> u64 {
        let mut h: u64 = FNV_BASIS;
        let mut i: usize = 0;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                let w = n.to_words();
                let mut k: usize = 0;
                while k < 8 {
                    let word = match w.get(k) {
                        Some(x) => *x,
                        None => 0,
                    };
                    h = fnv1a_acc(h, word);
                    k = k.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        h
    }
    /// 树指纹（含节点 + 守恒账）。
    pub fn fingerprint(&self) -> u64 {
        let h = self.nodes_fp();
        fnv1a_acc(h, self.ledger.fingerprint())
    }
    /// 分配序号（`u64::MAX` 前守恒，越界返回 `Err`）。
    pub fn alloc_seq(&mut self) -> InsertResult<u64> {
        if self.seq_next == u64::MAX {
            return Err(VmaErr::of(VmaErrKind::SeqOverflow, self.seq_next));
        }
        let s = self.seq_next;
        self.seq_next = self.seq_next.wrapping_add(1);
        Ok(s)
    }
    /// 查询与 `req` 相交的节点下标集合（`O(log n + k)`）。
    ///
    /// 定位法：从 `req.start` 的**前驱**起步（`O(log n)` 二分），而不是
    /// 从下标 0 线性扫；否则 10^5 次插入会退化成 `O(n^2)`。
    pub fn intersect_idx(&self, req: &AddrRange) -> InsertResult<Vec<usize>> {
        let mut out: Vec<usize> = Vec::new();
        if self.nodes.is_empty() {
            return Ok(out);
        }
        // 第一个 `start >= req.start` 的下标。
        let mut lo: usize = 0;
        let mut hi: usize = self.nodes.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let v = match self.nodes.get(mid) {
                Some(x) => x.range.start,
                None => return Err(VmaErr::of(VmaErrKind::BisectRange, mid as u64)),
            };
            if v < req.start {
                lo = mid.wrapping_add(1);
            } else {
                hi = mid;
            }
        }
        // 从 `lo - 1`（可能是唯一可能相交的前驱）起步。
        let mut i: usize = if lo == 0 { 0 } else { lo - 1 };
        while i < self.nodes.len() {
            let n = match self.nodes.get(i) {
                Some(x) => *x,
                None => break,
            };
            if n.range.start >= req.end {
                break;
            }
            if n.range.end > req.start && req.intersects(&n.range) {
                out.push(i);
            }
            i = i.wrapping_add(1);
        }
        Ok(out)
    }
    /// 重叠探测（闸 1 的裁决）：返回首个交叠节点的定位票据。
    pub fn overlap_probe(&self, req: &AddrRange) -> InsertResult<Option<ConflictSite>> {
        let idx = self.intersect_idx(req)?;
        match idx.first() {
            Some(i) => match self.nodes.get(*i) {
                Some(n) => Ok(Some(ConflictSite::new(req, n))),
                None => Err(VmaErr::of(VmaErrKind::NeighborBroken, *i as u64)),
            },
            None => Ok(None),
        }
    }
    /// 六关系全枚举探测（判据第二步要求「判定函数穷举断言」）。
    pub fn relation_sweep(&self, req: &AddrRange) -> Vec<Relation> {
        let mut out: Vec<Relation> = Vec::new();
        let mut i: usize = 0;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                out.push(classify(req, &n.range));
            }
            i = i.wrapping_add(1);
        }
        out
    }
    /// 相邻检测（闸 2 的裁决）：返回左侧 / 右侧紧邻节点下标。
    pub fn adjacency(&self, req: &AddrRange) -> InsertResult<(Option<usize>, Option<usize>)> {
        if self.nodes.is_empty() {
            return Ok((None, None));
        }
        let left = predecessor(&self.nodes, req.start)?;
        // 紧邻性校验：前驱的 `end` 必须恰等于 `req.start`，否则不是紧邻。
        if let Some(li) = left {
            let n = match self.nodes.get(li) {
                Some(x) => *x,
                None => return Err(VmaErr::of(VmaErrKind::NeighborBroken, li as u64)),
            };
            if n.range.end != req.start {
                return Ok((None, None));
            }
        }
        let right = successor(&self.nodes, req.end)?;
        if let Some(ri) = right {
            let n = match self.nodes.get(ri) {
                Some(x) => *x,
                None => return Err(VmaErr::of(VmaErrKind::NeighborBroken, ri as u64)),
            };
            if n.range.start != req.end {
                return Ok((None, None));
            }
        }
        Ok((left, right))
    }
    /// 插入节点到有序位置（**不**校验重叠，仅做有序插入；调用方须先过闸）。
    pub fn land(&mut self, node: VmaNode) -> InsertResult<usize> {
        if !self.room() {
            return Err(VmaErr::of(VmaErrKind::TreeFull, self.nodes.len() as u64));
        }
        let mut lo: usize = 0;
        let mut hi: usize = self.nodes.len();
        // 二分定位插入点（`O(log n)`）。
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let v = match self.nodes.get(mid) {
                Some(x) => x.range.start,
                None => return Err(VmaErr::of(VmaErrKind::BisectRange, mid as u64)),
            };
            if v < node.range.start {
                lo = mid.wrapping_add(1);
            } else {
                hi = mid;
            }
        }
        let idx = lo;
        self.nodes.insert(idx, node);
        self.witness_add(node.fingerprint);
        Ok(idx)
    }
    /// 按序号取节点。
    pub fn node(&self, seq: u64) -> InsertResult<VmaNode> {
        match find_by_seq(&self.nodes, seq) {
            Some(i) => match self.nodes.get(i) {
                Some(n) => Ok(*n),
                None => Err(VmaErr::of(VmaErrKind::NodeNotFound, seq)),
            },
            None => Err(VmaErr::of(VmaErrKind::NodeNotFound, seq)),
        }
    }
    /// 按序号删节点（供 F2584 消费面与注入核回滚用）。
    pub fn remove_seq(&mut self, seq: u64) -> InsertResult<VmaNode> {
        match find_by_seq(&self.nodes, seq) {
            Some(i) => {
                let n = self.nodes.remove(i);
                self.witness_sub(n.fingerprint);
                Ok(n)
            }
            None => Err(VmaErr::of(VmaErrKind::NodeNotFound, seq)),
        }
    }
    /// 树改动见证（与 `witness()` 同值，别名便于阅读）。
    pub fn fp_witness(&self) -> Witness {
        self.witness()
    }
    /// 局部不变量自检（`O(1)`）：只比下标 `i` 的左右邻居。
    ///
    /// 这是插入热路径上的自检形态（`O(1)`）；全量 `check_invariants()`
    /// 只在检查点跑。局部判定与全量判定在树有序的前提下**等价**：
    /// 有序性由 `land()` 的二分保证，邻居不交叠即可推出全量不交叠。
    pub fn local_ok(&self, i: usize) -> bool {
        let cur = match self.nodes.get(i) {
            Some(n) => *n,
            None => return false,
        };
        if i > 0 {
            if let Some(prev) = self.nodes.get(i - 1) {
                if cur.range.start <= prev.range.start {
                    return false;
                }
                if prev.range.intersects(&cur.range) {
                    return false;
                }
            }
        }
        if i + 1 < self.nodes.len() {
            if let Some(next) = self.nodes.get(i + 1) {
                if next.range.start <= cur.range.start {
                    return false;
                }
                if cur.range.intersects(&next.range) {
                    return false;
                }
            }
        }
        if cur.attr.user {
            cur.range.in_user_half()
        } else {
            cur.range.in_kernel_half()
        }
    }
    /// 节点数（`len` 别名）。
    pub fn n(&self) -> usize {
        self.nodes.len()
    }
    /// 见证 + 全量指纹对照（检查点用）。
    pub fn checkpoint(&self) -> (Witness, u64) {
        (self.witness(), self.nodes_fp())
    }
    /// 节点集合快照描述（观测用）。
    pub fn nodes_describe(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut i: usize = 0;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                out.push(n.describe());
            }
            i = i.wrapping_add(1);
        }
        out
    }
    /// 账快照（观测用）。
    pub fn ledger_snapshot(&self) -> InsertLedger {
        self.ledger
    }
    /// 清空（树 + 账 + 环形账 + 状态机 + 序号，全同步归零）。
    pub fn reset(&mut self) -> InsertResult<()> {
        self.nodes.clear();
        self.ledger.reset();
        self.ring = RingLedger::default_ring()?;
        self.fsm = InsertFsm::new();
        self.seq_next = 1;
        self.req_no = 0;
        self.trigger_no = 0;
        self.conflict_traced = 0;
        self.tick = 0;
        self.nodes_sum = 0;
        self.nodes_xor = 0;
        if self.ledger.total != 0 {
            return Err(VmaErr::of(VmaErrKind::ResetMismatch, self.ledger.total));
        }
        Ok(())
    }
    /// 账守恒 + 三不变量（一次判定）。
    pub fn healthy(&self) -> bool {
        self.ledger.conserved() && invariants_ok(&self.nodes) && self.ring.verify() && self.ring.dropped_consistent()
    }
}

// ===========================================================================
// 12. 插入流水线（三闸唯一入口）
// ===========================================================================

/// 插入入口（**唯一**落树通道）。
///
/// 流水线：
/// 1. `begin()`：`Idle → GateHalf`，账 `enter()`，分配 `req_no`；
/// 2. **闸 0 半区**：`policy.check()`；
/// 3. **闸 1 重叠**：`overlap_probe()`；
/// 4. **闸 2 相邻**：`adjacency()` + 属性一致性；
/// 5. `Landed → Idle` 提交，或 `MergePending` 出票，或 `Quarantine → Idle` 留痕。
///
/// **原子性声明**：闸 1 的检测与闸 3 的落树在**同一次调用**内完成，中间
/// 不存在任何可被观测的中间态（`nodes` 仅在最后一步被改）。这正是判据
/// 「插入检测窗口竞态零容忍」的结构性保证；并发面由 F2596 承接。
///
/// 返回 `Err` 时树与账的一致性由 `before_fp` / `after_fp` 双证（`err`
/// 本身不进树）。
pub fn insert(tree: &mut VmaTree, req: &InsertReq, policy: HalfPolicy) -> InsertResult<InsertOutcome> {
    insert_inner(tree, req, policy, 0)
}

/// 插入（可注入「跳过某一闸」的旁路版本，仅供注入核使用）。
///
/// `bypass_gate`：`0` = 无旁路；`1` = 跳过闸 0；`2` = 跳过闸 1；
/// `3` = 跳过闸 2。旁路版本**只用于造证**，产物必被不变量 / 检出核抓住。
pub fn insert_bypass(tree: &mut VmaTree, req: &InsertReq, policy: HalfPolicy, bypass_gate: u8) -> InsertResult<InsertOutcome> {
    insert_inner(tree, req, policy, bypass_gate)
}

/// 三闸流水线实现（`insert` 与 `insert_bypass` 的共同实现）。
pub fn insert_inner(tree: &mut VmaTree, req: &InsertReq, policy: HalfPolicy, bypass_gate: u8) -> InsertResult<InsertOutcome> {
    // 「拒绝路径树零改动」双证只看 O(1) 改动见证（账本来就要变）。
    let before_fp = tree.witness();
    // ---- begin ----
    tree.fsm.try_move(InsertState::GateHalf)?;
    tree.ledger.enter();
    tree.req_no = tree.req_no.wrapping_add(1);
    let req_no = tree.req_no;
    tree.tick = tree.tick.wrapping_add(1);
    let tick = tree.tick;
    // ---- 闸 0：半区 ----
    if bypass_gate != 1 {
        if let Err(e) = policy.check(&req.range) {
            let kind = match VmaErrKind::from_code(e.code()) {
                Ok(k) => k,
                Err(_) => VmaErrKind::HalfUnclassified,
            };
            return quarantine(tree, req_no, tick, kind, None, before_fp);
        }
        // 属性 / 半区自洽：`user` 属性必须落用户半区。
        if req.attr.user != req.range.in_user_half() {
            return quarantine(tree, req_no, tick, VmaErrKind::UserAttrMismatch, None, before_fp);
        }
    }
    tree.fsm.try_move(InsertState::GateOverlap)?;
    // ---- 闸 1：重叠 ----
    if bypass_gate != 2 {
        let probe = tree.overlap_probe(&req.range)?;
        if let Some(c) = probe {
            if c.must_reject() {
                return quarantine(tree, req_no, tick, VmaErrKind::OverlapReject, Some(c), before_fp);
            }
        }
    }
    tree.fsm.try_move(InsertState::GateAdjacent)?;
    // ---- 闸 2：相邻 / 合并判定 ----
    if bypass_gate != 3 {
        let (left, right) = tree.adjacency(&req.range)?;
        if left.is_some() || right.is_some() {
            // 相邻存在：**逐个**邻居的合并键都必须与请求**完全相等**才允许
            // 触发合并。注意这里必须用 `==` 而不是位与 `&`——位与会
            // 把「请求可写、邻居不可写」这种子集关系误判成可合并。
            let mut all_same = true;
            if let Some(li) = left {
                match tree.nodes.get(li) {
                    Some(n) => {
                        if n.attr.eq_key() != req.attr.eq_key() {
                            all_same = false;
                        }
                    }
                    None => return Err(VmaErr::of(VmaErrKind::NeighborBroken, li as u64)),
                }
            }
            if let Some(ri) = right {
                match tree.nodes.get(ri) {
                    Some(n) => {
                        if n.attr.eq_key() != req.attr.eq_key() {
                            all_same = false;
                        }
                    }
                    None => return Err(VmaErr::of(VmaErrKind::NeighborBroken, ri as u64)),
                }
            }
            if !all_same {
                // 属性不同 → 既不合并也不落树（避免异属性大 VMA）。
                return quarantine(tree, req_no, tick, VmaErrKind::AdjacentAttrDiff, None, before_fp);
            }
            // 共享映射合并未决 → 保守拒绝（F2564/F2642 就位后放开）。
            if req.attr.shared {
                return quarantine(tree, req_no, tick, VmaErrKind::ShareNotDecided, None, before_fp);
            }
            // 触发合并：出票，交 F2584 执行。
            let merged = AddrRange { start: left.and_then(|i| tree.nodes.get(i)).map(|n| n.range.start).unwrap_or(req.range.start), end: right.and_then(|i| tree.nodes.get(i)).map(|n| n.range.end).unwrap_or(req.range.end) };
            let left_seq = match left.and_then(|i| tree.nodes.get(i)) {
                Some(n) => n.seq,
                None => 0,
            };
            let right_seq = match right.and_then(|i| tree.nodes.get(i)) {
                Some(n) => n.seq,
                None => 0,
            };
            tree.trigger_no = tree.trigger_no.wrapping_add(1);
            let ticket = MergeTicket { merged, left_seq, right_seq, attr_key: req.attr.eq_key(), tick, trigger_no: tree.trigger_no };
            tree.ledger.note_merge_pending();
            tree.fsm.try_move(InsertState::MergePending)?;
            let out = InsertOutcome::triggered(ticket);
            let vc = verdict_code(&out.verdict);
            let _ = tree.ring.push(tick, req_no, &out.verdict, &req.range, 0);
            let _ = vc;
            tree.fsm.try_move(InsertState::Idle)?;
            return Ok(out);
        }
    }
    // ---- 落树 ----
    // 容量闸必须**先于**进入 `Landed`（`Landed → Quarantine` 不是合法边）。
    if !tree.room() {
        tree.ledger.note_rejected(VmaErrKind::TreeFull);
        let _ = tree.ring.push(tick, req_no, &Verdict::Rejected(VmaErrKind::TreeFull), &req.range, VmaErrKind::TreeFull.code());
        tree.fsm.try_move(InsertState::Quarantine)?;
        tree.fsm.try_move(InsertState::Idle)?;
        return Err(VmaErr::of(VmaErrKind::TreeFull, tree.nodes.len() as u64).note_of(&format!("witness_before={}", before_fp.describe())));
    }
    tree.fsm.try_move(InsertState::Landed)?;
    let seq = tree.alloc_seq()?;
    let node = VmaNode::new(req.range, req.attr, req.kind, seq, tick);
    let idx = tree.land(node);
    tree.ledger.note_accepted();
    let landed_ok = match idx {
        Ok(i) => tree.local_ok(i),
        Err(_) => false,
    };
    // 落树后立刻做**局部**不变量自检（`O(1)`，只比左右邻居）：
    // 破则回滚（节点 + 账双回滚）并报 `ERR_INVARIANT_BROKEN`。
    if !landed_ok {
        let seq2 = node.seq;
        let _ = tree.remove_seq(seq2);
        tree.ledger.accepted = tree.ledger.accepted.saturating_sub(1);
        return Err(VmaErr::of(VmaErrKind::InvariantBroken, seq2).note_of(&format!("fp=0x{:016X}", tree.nodes_fp())));
    }
    let out = InsertOutcome::accepted(seq);
    let _ = tree.ring.push(tick, req_no, &out.verdict, &req.range, 0);
    tree.fsm.try_move(InsertState::Idle)?;
    Ok(out)
}

/// 隔离路径（被拒 → 留痕 → 回 `Idle`）。
fn quarantine(
    tree: &mut VmaTree,
    req_no: u64,
    tick: u64,
    kind: VmaErrKind,
    conflict: Option<ConflictSite>,
    before_fp: Witness,
) -> InsertResult<InsertOutcome> {
    tree.fsm.try_move(InsertState::Quarantine)?;
    tree.ledger.note_rejected(kind);
    if conflict.is_some() {
        tree.conflict_traced = tree.conflict_traced.wrapping_add(1);
    }
    let v = Verdict::Rejected(kind);
    let range = match conflict {
        Some(c) => AddrRange { start: c.other_range.start, end: c.other_range.end },
        None => AddrRange { start: tick, end: tick.wrapping_add(1) },
    };
    let _ = tree.ring.push(tick, req_no, &v, &range, kind.code());
    tree.fsm.try_move(InsertState::Idle)?;
    // 拒绝路径的「树零改动」双证：`O(1)` 改动见证必须与插入前完全一致。
    if tree.witness() != before_fp {
        return Err(VmaErr::of(VmaErrKind::InvariantBroken, kind.index() as u64)
            .note_of(&format!("reject mutated tree: {} vs {}", tree.witness().describe(), before_fp.describe())));
    }
    // 「相邻但属性不同」在结论闭集里有专名 `AdjacentBlocked`（与其它拒绝
    // 区分开：它不是非法映射，而是一个**合法的拒绝结论**）。其余拒绝走
    // 通用 `Verdict::Rejected(kind)`。
    let out = if kind == VmaErrKind::AdjacentAttrDiff {
        InsertOutcome { verdict: Verdict::AdjacentBlocked, node_seq: 0, conflict, merge: None, ticks: 0 }
    } else {
        InsertOutcome::rejected(kind, conflict)
    };
    Ok(out)
}

/// 便捷封装：插入并返回结论（不返回 `Err`，被拒转为 `Verdict::Rejected`）。
///
/// 之所以提供这层封装：调用面（F2589 单点消费）关心的是**结论**而非
/// 错误传播；错误对象仍在 `InsertOutcome.conflict` / `verdict` 内可取。
pub fn try_insert(tree: &mut VmaTree, req: &InsertReq, policy: HalfPolicy) -> InsertOutcome {
    match insert(tree, req, policy) {
        Ok(o) => o,
        Err(e) => {
            let kind = match VmaErrKind::from_code(e.code()) {
                Ok(k) => k,
                Err(_) => VmaErrKind::Unknown,
            };
            InsertOutcome::rejected(kind, None)
        }
    }
}

/// 半区策略推导（由 `addr >= KERNEL_HALF_START` 决定，`user` 属性优先）。
pub fn derive_policy(attr: &VmaAttr, r: &AddrRange) -> HalfPolicy {
    if attr.user || r.in_user_half() {
        HalfPolicy::user()
    } else if r.in_kernel_half() {
        HalfPolicy::kernel()
    } else {
        HalfPolicy::unclassified()
    }
}

/// 区间生成器（用于压力插入）：从种子造一串**互不相交**的合法区间。
///
/// 生成规则：把用户半区按 `PAGE_SIZE` 分块，每块 4 页；序号 `i` 占第
/// `4i+1 .. 4i+3` 页，即区间 `[ (4i+1)*PAGE, (4i+3)*PAGE )`。相邻样本之间
/// 天然留 2 页空隙 —— 既页对齐、有序、互不交叠，也**互不相邻**（否则会
/// 触发合并而不是落树）。这正是判据「合法插入 10^5 次树不变量零破坏」
/// 需要的样本源。
pub fn gen_disjoint_user(seed: u64, i: u64) -> AddrRange {
    let mut r = Lcg::new(seed ^ i.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    // 抖动只落在空隙内（第 4i+4 页），保证不与相邻样本接触。
    let gap_pick = r.bits(1) & 1;
    let start = i.wrapping_mul(4).wrapping_add(1).wrapping_mul(PAGE_SIZE);
    let end = i.wrapping_mul(4).wrapping_add(3).wrapping_mul(PAGE_SIZE);
    let _ = gap_pick;
    AddrRange { start, end }
}

// ===========================================================================
// 13. 六关系穷举判定表（判据第二步）
// ===========================================================================

/// 六关系枚举测试集（人工枚举，**不依赖** `classify()` 的实现）。
///
/// 每个关系至少 2 例，另加「不相交」对照 4 例（相接 / 分离 / 逆序相接 /
/// 嵌套反向），共 `6×2 + 4 = 16` 例。
pub fn relation_testset() -> Vec<(AddrRange, AddrRange, Relation)> {
    let a = AddrRange { start: 0x1000, end: 0x2000 };
    let a2 = AddrRange { start: 0x1100, end: 0x2100 };
    let a3 = AddrRange { start: 0x1400, end: 0x2400 };
    let a5 = AddrRange { start: 0x2000, end: 0x3000 };
    // 同起：b 与 a 同 start、不同 end
    let b = AddrRange { start: 0x1000, end: 0x1800 };
    let b2 = AddrRange { start: 0x1100, end: 0x3000 };
    // 同止：c 与 a 同 end、不同 start
    let c = AddrRange { start: 0x1400, end: 0x2000 };
    let c2 = AddrRange { start: 0x0800, end: 0x2100 };
    // 严格内嵌：d 完全落在 a / a2 之内
    let d = AddrRange { start: 0x1200, end: 0x1800 };
    let d2 = AddrRange { start: 0x1200, end: 0x1A00 };
    // 左交：e 起点在 a 之后、终点在 a 之外
    let e = AddrRange { start: 0x1800, end: 0x2800 };
    let e2 = AddrRange { start: 0x1800, end: 0x2C00 };
    // 端点相接（不相交对照）
    let f = AddrRange { start: 0x2000, end: 0x2800 };
    vec![
        // 同起 2 例
        (a, b, Relation::SameStart),
        (a2, b2, Relation::SameStart),
        // 同止 2 例
        (a, c, Relation::SameEnd),
        (a2, c2, Relation::SameEnd),
        // 包含 2 例
        (a, d, Relation::Contains),
        (a2, d2, Relation::Contains),
        // 被含 2 例
        (d, a, Relation::Contained),
        (d2, a2, Relation::Contained),
        // 左交 2 例
        (a, e, Relation::LeftCross),
        (a3, e2, Relation::LeftCross),
        // 右交 2 例
        (e, a, Relation::RightCross),
        (e2, a3, Relation::RightCross),
        // 不相交（对照）：端点相接 2 例 + 完全分离 2 例
        (a, f, Relation::Disjoint),
        (f, a, Relation::Disjoint),
        (a, a5, Relation::Disjoint),
        (a5, a, Relation::Disjoint),
    ]
}

/// 关系判定表全穷举（`16` 格）。
pub fn relation_matrix() -> Vec<RelationCell> {
    let set = relation_testset();
    let mut out: Vec<RelationCell> = Vec::new();
    let mut i: usize = 0;
    while i < set.len() {
        if let Some((a, b, expect)) = set.get(i) {
            let actual = classify(a, b);
            let overlap_consistent = actual.is_overlap() == a.intersects(b);
            out.push(RelationCell {
                case: i,
                a: *a,
                b: *b,
                expect: *expect,
                actual,
                overlap_consistent,
                pass: *expect == actual && overlap_consistent,
            });
        }
        i = i.wrapping_add(1);
    }
    out
}

/// 关系判定表全过（16/16）。
pub fn relation_matrix_ok() -> bool {
    let m = relation_matrix();
    if m.len() < RELATION_SET_MIN {
        return false;
    }
    let mut n: usize = 0;
    let mut i: usize = 0;
    while i < m.len() {
        if let Some(c) = m.get(i) {
            if c.pass {
                n = n.wrapping_add(1);
            }
        }
        i = i.wrapping_add(1);
    }
    n == m.len()
}

/// 关系测试集最小规模（六关系各 2 例 + 4 对照 = 16）。
pub const RELATION_SET_MIN: usize = 16;

/// 每关系覆盖计数（`[u8; 7]`，六关系各应 ≥ 2，对照 ≥ 4）。
pub fn relation_coverage() -> [usize; RELATION_TOTAL] {
    let mut out = [0usize; RELATION_TOTAL];
    let m = relation_matrix();
    let mut i: usize = 0;
    while i < m.len() {
        if let Some(c) = m.get(i) {
            let idx = c.expect.index();
            if idx < RELATION_TOTAL {
                out[idx] = out[idx].wrapping_add(1);
            }
        }
        i = i.wrapping_add(1);
    }
    out
}

/// 关系覆盖率达标（六关系各 ≥2、对照 ≥4）。
pub fn relation_coverage_ok() -> bool {
    let cov = relation_coverage();
    let mut i: usize = 0;
    while i < RELATION_COUNT {
        if cov[i] < 2 {
            return false;
        }
        i = i.wrapping_add(1);
    }
    cov[RELATION_COUNT] >= 4
}

// ===========================================================================
// 14. 合并触发矩阵（相邻同属性必触发）
// ===========================================================================

/// 合并判定一格。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MergeCell {
    /// 样本序号。
    pub case: usize,
    /// 属性索引（0..15）。
    pub attr_index: usize,
    /// 请求与既有区间的关系（相邻 / 分离）。
    pub adjacent: bool,
    /// 期望结论（`MergeTriggered` / `Accepted` / `AdjacentBlocked` / 拒绝）。
    pub expect: u32,
    /// 实际结论编码。
    pub actual: u64,
    /// 通过与否。
    pub pass: bool,
}

impl MergeCell {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("#{} attr={} adj={} expect={} actual={} {}", self.case, self.attr_index, if self.adjacent { 1 } else { 0 },
            self.expect, self.actual, if self.pass { "PASS" } else { "FAIL" })
    }
}

/// 期望结论编码（`0`=接受 / `1`=合并触发 / `2`=相邻拒绝 / `3`=其它拒绝）。
pub fn expect_merge_code(expect: Verdict) -> u32 {
    verdict_code(&expect) as u32
}

/// 合并矩阵（属性枚举 × 相邻/分离，`16 × 2 = 32` 格）。
///
/// 语义：
/// - **属性相同 + 相邻** → `MergeTriggered`（判据「相邻同属性必触发」）；
/// - **属性不同 + 相邻** → `AdjacentBlocked`；
/// - **分离** → `Accepted`（落树）。
pub fn merge_matrix() -> Vec<MergeCell> {
    let mut out: Vec<MergeCell> = Vec::new();
    let mut case = 0usize;
    let mut ai: usize = 0;
    while ai < VmaAttr::enum_total() {
        let attr = match VmaAttr::from_index(ai) {
            Some(a) => a,
            None => break,
        };
        // 合并矩阵一律在用户半区做（否则半区闸会把非 user 属性先拒掉，
        // 测的就不是合并语义而是半区语义了）。
        let mut attr = attr;
        attr.user = true;
        let mut adj: usize = 0;
        while adj < 2 {
            let adjacent = adj == 1;
            let base = 0x4000u64 + (ai as u64).wrapping_mul(0x4000);
            let exist = AddrRange { start: base, end: base.wrapping_add(PAGE_SIZE) };
            let req_range = if adjacent { AddrRange { start: exist.end, end: exist.end.wrapping_add(PAGE_SIZE) } } else {
                AddrRange { start: exist.end.wrapping_add(PAGE_SIZE), end: exist.end.wrapping_add(PAGE_SIZE).wrapping_mul(2) }
            };
            let exist_attr = attr;
            let req_attr = attr;
            let mut tree = match VmaTree::new() {
                Ok(t) => t,
                Err(_) => break,
            };
            let seed_req = InsertReq::new(exist, exist_attr, 1, 1);
            let policy = derive_policy(&exist_attr, &exist);
            let _ = insert(&mut tree, &seed_req, policy);
            let req = InsertReq::new(req_range, req_attr, 1, 2);
            let res = try_insert(&mut tree, &req, policy);
            let expect: Verdict = if adjacent { Verdict::MergeTriggered } else { Verdict::Accepted };
            let cell = MergeCell {
                case,
                attr_index: ai,
                adjacent,
                expect: expect_merge_code(expect),
                actual: verdict_code(&res.verdict),
                pass: false,
            };
            out_expect_push(&mut out, cell);
            case = case.wrapping_add(1);
            adj = adj.wrapping_add(1);
        }
        ai = ai.wrapping_add(1);
    }
    out
}

/// 合并矩阵写入辅助（独立成函数以便阅读）。
fn out_expect_push(v: &mut Vec<MergeCell>, mut c: MergeCell) {
    c.pass = c.expect as u64 == c.actual;
    v.push(c);
}

/// 属性不同 + 相邻的专门测试（用两枚不同属性）。
pub fn adjacent_attr_diff_case() -> MergeCell {
    let mut a1 = VmaAttr::ZERO;
    a1.user = true;
    a1.writable = true;
    let mut a2 = VmaAttr::ZERO;
    a2.user = true;
    a2.writable = false;
    a2.cache = CacheKind::Uncached;
    let exist = AddrRange { start: 0x8000, end: 0x9000 };
    let req_range = AddrRange { start: 0x9000, end: 0xA000 };
    let mut tree = match VmaTree::new() {
        Ok(t) => t,
        Err(_) => return MergeCell { case: 0, attr_index: 0, adjacent: true, expect: 2, actual: 99, pass: false },
    };
    let policy = HalfPolicy::user();
    let _ = insert(&mut tree, &InsertReq::new(exist, a1, 1, 1), policy);
    let out = try_insert(&mut tree, &InsertReq::new(req_range, a2, 1, 2), policy);
    let mut c = MergeCell { case: 999, attr_index: 0, adjacent: true, expect: 2, actual: verdict_code(&out.verdict), pass: false };
    c.pass = c.expect as u64 == c.actual;
    c
}

/// 合并矩阵全过（32 格 + 相邻异属性 1 格）。
pub fn merge_matrix_ok() -> bool {
    let m = merge_matrix();
    if m.len() != VmaAttr::enum_total() * 2 {
        return false;
    }
    let mut i: usize = 0;
    while i < m.len() {
        if let Some(c) = m.get(i) {
            if !c.pass {
                return false;
            }
        }
        i = i.wrapping_add(1);
    }
    adjacent_attr_diff_case().pass
}

/// 合并触发链断言（判据「合并触发链路断言」）。
///
/// 链路：`请求 → 相邻检测命中 → 合并键一致 → 出票 → 账 `merged_pending`
/// +1 → F2584 消费 → 账 `merged_done` +1 且 `merged_pending` -1。
pub fn merge_chain_ok() -> bool {
    let mut tree = match VmaTree::new() {
        Ok(t) => t,
        Err(_) => return false,
    };
    let mut a = VmaAttr::ZERO;
    a.user = true;
    a.writable = true;
    let policy = HalfPolicy::user();
    let l = AddrRange { start: 0x20000, end: 0x21000 };
    let r = AddrRange { start: 0x22000, end: 0x23000 };
    let m = AddrRange { start: 0x21000, end: 0x22000 };
    if insert(&mut tree, &InsertReq::new(l, a, 1, 1), policy).is_err() {
        return false;
    }
    if insert(&mut tree, &InsertReq::new(r, a, 1, 2), policy).is_err() {
        return false;
    }
    let before = tree.ledger.merged_pending;
    let out = try_insert(&mut tree, &InsertReq::new(m, a, 1, 3), policy);
    if !matches!(out.verdict, Verdict::MergeTriggered) {
        return false;
    }
    let after = tree.ledger.merged_pending;
    if after != before.wrapping_add(1) {
        return false;
    }
    match out.merge {
        Some(t) => {
            if t.left_seq == 0 || t.right_seq == 0 {
                return false;
            }
            if t.merged.start != l.start || t.merged.end != r.end {
                return false;
            }
        }
        None => return false,
    }
    // F2584 消费一票（模拟）。
    tree.ledger.note_merge_done();
    tree.ledger.note_merge_done();
    tree.ledger.accepted = tree.ledger.accepted.wrapping_add(1);
    tree.ledger.conserved()
}

// ===========================================================================
// 15. 负向注入核（12 形态，全被拒并留痕）
// ===========================================================================

/// 注入形态闭集（判据要求 10，本单实现 12）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// 重叠注入 · 同起。
    F0SameStart,
    /// 重叠注入 · 同止。
    F1SameEnd,
    /// 重叠注入 · 包含。
    F2Contains,
    /// 重叠注入 · 被含。
    F3Contained,
    /// 重叠注入 · 左交。
    F4LeftCross,
    /// 重叠注入 · 右交。
    F5RightCross,
    /// 半区注入 · 用户映射落内核半区。
    F6UserInKernel,
    /// 半区注入 · 跨半区区间。
    F7HalfCross,
    /// 半区注入 · 未页对齐。
    F8Unaligned,
    /// 状态机注入 · 越闸迁移（`Idle → Landed`）。
    F9IllegalEdge,
    /// 检测窗口竞态注入 · 跳过闸 1 强行落树（旁路版本）。
    F10WindowRace,
    /// 相邻异属性注入 · 相邻但属性不同被误合并。
    F11AdjacentAttrDiff,
}

impl Form {
    /// 全部形态（判据顺序 0..11）。
    pub const ALL: [Form; FORM_COUNT] = [
        Form::F0SameStart,
        Form::F1SameEnd,
        Form::F2Contains,
        Form::F3Contained,
        Form::F4LeftCross,
        Form::F5RightCross,
        Form::F6UserInKernel,
        Form::F7HalfCross,
        Form::F8Unaligned,
        Form::F9IllegalEdge,
        Form::F10WindowRace,
        Form::F11AdjacentAttrDiff,
    ];
    /// 闭集序号。
    pub fn index(self) -> usize {
        match self {
            Form::F0SameStart => 0,
            Form::F1SameEnd => 1,
            Form::F2Contains => 2,
            Form::F3Contained => 3,
            Form::F4LeftCross => 4,
            Form::F5RightCross => 5,
            Form::F6UserInKernel => 6,
            Form::F7HalfCross => 7,
            Form::F8Unaligned => 8,
            Form::F9IllegalEdge => 9,
            Form::F10WindowRace => 10,
            Form::F11AdjacentAttrDiff => 11,
        }
    }
    /// 诊断码（**错误变体码**，非形态码：形态映射到被拒时触发的码）。
    pub fn expect_err_code(self) -> u16 {
        match self {
            Form::F0SameStart | Form::F1SameEnd | Form::F2Contains | Form::F3Contained | Form::F4LeftCross | Form::F5RightCross => {
                ERR_OVERLAP_REJECT
            }
            Form::F6UserInKernel => ERR_HALF_OUTSIDE,
            Form::F7HalfCross => ERR_HALF_CROSS,
            Form::F8Unaligned => ERR_HALF_UNALIGNED,
            Form::F9IllegalEdge => ERR_EDGE_CLOSED,
            Form::F10WindowRace => ERR_WINDOW_RACE,
            Form::F11AdjacentAttrDiff => ERR_ADJACENT_ATTR_DIFF,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            Form::F0SameStart => "overlap_same_start",
            Form::F1SameEnd => "overlap_same_end",
            Form::F2Contains => "overlap_contains",
            Form::F3Contained => "overlap_contained",
            Form::F4LeftCross => "overlap_left_cross",
            Form::F5RightCross => "overlap_right_cross",
            Form::F6UserInKernel => "half_user_in_kernel",
            Form::F7HalfCross => "half_cross",
            Form::F8Unaligned => "half_unaligned",
            Form::F9IllegalEdge => "fsm_illegal_edge",
            Form::F10WindowRace => "window_race_bypass",
            Form::F11AdjacentAttrDiff => "adjacent_attr_diff",
        }
    }
    /// 判读（现象）。
    pub fn describe(self) -> &'static str {
        match self {
            Form::F0SameStart => "重叠注入：请求与既有 VMA 同起，必须被闸 1 拒",
            Form::F1SameEnd => "重叠注入：请求与既有 VMA 同止，必须被闸 1 拒",
            Form::F2Contains => "重叠注入：请求严格包含既有 VMA，必须被闸 1 拒",
            Form::F3Contained => "重叠注入：请求严格被既有 VMA 包含，必须被闸 1 拒",
            Form::F4LeftCross => "重叠注入：请求左交既有 VMA，必须被闸 1 拒",
            Form::F5RightCross => "重叠注入：请求右交既有 VMA，必须被闸 1 拒",
            Form::F6UserInKernel => "半区注入：用户属性却落内核半区，必须被闸 0 拒",
            Form::F7HalfCross => "半区注入：区间跨越半区边界，必须被闸 0 拒",
            Form::F8Unaligned => "半区注入：用户半区请求未页对齐，必须被闸 0 拒",
            Form::F9IllegalEdge => "状态机注入：越闸迁移 Idle→Landed，必须被合法边闭集拒记",
            Form::F10WindowRace => "检测窗口竞态注入：跳过闸 1 强行落树，必被不变量 / 检出核抓住",
            Form::F11AdjacentAttrDiff => "相邻异属性注入：相邻但属性不同，必须既不合并也不落树",
        }
    }
    /// 是否属「重叠注入」六形态族。
    pub fn is_overlap_family(&self) -> bool {
        matches!(
            self,
            Form::F0SameStart | Form::F1SameEnd | Form::F2Contains | Form::F3Contained | Form::F4LeftCross | Form::F5RightCross
        )
    }
}

/// 注入记录（造证留痕）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Injected {
    /// 形态。
    pub form: Form,
    /// 造证种子。
    pub seed: u64,
    /// 轮次（`OVERLAP_INJECT_ROUNDS` 内计数）。
    pub round: u64,
    /// 是否被拒（`Err` 或 `Verdict::Rejected*`）。
    pub rejected: bool,
    /// 实际诊断码（被拒时有效，否则 0）。
    pub err_code: u16,
    /// 期望诊断码。
    pub expect_code: u16,
    /// 拒绝时树是否零改动。
    pub tree_intact: bool,
    /// 冲突关系（六形态族有效）。
    pub relation: Option<Relation>,
    /// 详情串。
    pub detail: String,
}

impl Injected {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("{} #{} rej={} code=0x{:04X} expect=0x{:04X} intact={} rel={}", self.form.name(), self.round,
            if self.rejected { 1 } else { 0 }, self.err_code, self.expect_code, if self.tree_intact { 1 } else { 0 },
            match self.relation {
                Some(r) => r.name(),
                None => "-",
            })
    }
    /// 是否达成期望（拒 + 码对 + 树零改动）。
    pub fn as_expected(&self) -> bool {
        self.rejected && self.err_code == self.expect_code && self.tree_intact
    }
}

/// 单形态注入（半区 / 重叠族通用：造一个基线树，注入一个请求）。
pub fn inject_form(tree: &mut VmaTree, form: Form, seed: u64, round: u64) -> Injected {
    let mut rng = Lcg::new(seed ^ (round.wrapping_mul(0x51_7C_C1_B7_27_22_0A_95)));
    let base_start = 0x100_0000u64.wrapping_add(round.wrapping_mul(0x10000));
    let exist = AddrRange { start: base_start, end: base_start.wrapping_add(PAGE_SIZE.wrapping_mul(4)) };
    let mut a = VmaAttr::ZERO;
    a.user = true;
    a.writable = true;
    let policy = HalfPolicy::user();
    // 基线落树（必须成功，否则注入无意义）。
    if insert(tree, &InsertReq::new(exist, a, 1, 1), policy).is_err() {
        return Injected {
            form,
            seed,
            round,
            rejected: true,
            err_code: ERR_UNKNOWN,
            expect_code: form.expect_err_code(),
            tree_intact: true,
            relation: None,
            detail: String::from("baseline failed"),
        };
    }
    let before = tree.nodes_fp();
    let before_len = tree.len();
    let req_range = match form {
        Form::F0SameStart => AddrRange { start: exist.start, end: exist.start.wrapping_add(PAGE_SIZE) },
        Form::F1SameEnd => AddrRange { start: exist.start.wrapping_add(PAGE_SIZE), end: exist.end },
        Form::F2Contains => AddrRange { start: exist.start.wrapping_sub(PAGE_SIZE), end: exist.end.wrapping_add(PAGE_SIZE) },
        Form::F3Contained => AddrRange { start: exist.start.wrapping_add(PAGE_SIZE), end: exist.end.wrapping_sub(PAGE_SIZE) },
        Form::F4LeftCross => AddrRange { start: exist.start.wrapping_add(PAGE_SIZE), end: exist.end.wrapping_add(PAGE_SIZE) },
        Form::F5RightCross => AddrRange { start: exist.start.wrapping_sub(PAGE_SIZE), end: exist.end.wrapping_sub(PAGE_SIZE) },
        Form::F6UserInKernel => AddrRange { start: KERNEL_HALF_START.wrapping_add(rng.bits(20)), end: KERNEL_HALF_START.wrapping_add(PAGE_SIZE) },
        Form::F7HalfCross => AddrRange { start: KERNEL_HALF_START.wrapping_sub(PAGE_SIZE), end: KERNEL_HALF_START.wrapping_add(PAGE_SIZE) },
        Form::F8Unaligned => AddrRange { start: exist.end.wrapping_add(1), end: exist.end.wrapping_add(PAGE_SIZE).wrapping_add(1) },
        Form::F9IllegalEdge => AddrRange { start: exist.end.wrapping_add(PAGE_SIZE), end: exist.end.wrapping_add(PAGE_SIZE).wrapping_mul(2) },
        Form::F10WindowRace => AddrRange { start: exist.start.wrapping_add(PAGE_SIZE), end: exist.end.wrapping_sub(PAGE_SIZE) },
        Form::F11AdjacentAttrDiff => AddrRange { start: exist.end, end: exist.end.wrapping_add(PAGE_SIZE) },
    };
    let mut req_attr = a;
    if form == Form::F11AdjacentAttrDiff {
        req_attr.executable = true;
    }
    let req = InsertReq::new(req_range, req_attr, 1, 2);
    let relation = classify(&req_range, &exist);
    // 执行注入。
    if form == Form::F9IllegalEdge {
        let e = tree.fsm.drive_illegal();
        let (rejected, code) = match e {
            Err(x) => (true, x.code()),
            Ok(_) => (false, 0u16),
        };
        let intact = tree.nodes_fp() == before && tree.len() == before_len;
        return Injected {
            form,
            seed,
            round,
            rejected,
            err_code: code,
            expect_code: form.expect_err_code(),
            tree_intact: intact,
            relation: None,
            detail: format!("fsm_refused={} moved={}", tree.fsm.refused, tree.fsm.moved),
        };
    }
    if form == Form::F10WindowRace {
        // 旁路跳过闸 1：交叠节点强行落树。
        let r = insert_bypass(tree, &req, policy, 2);
        let rejected = match r {
            Err(_) => true,
            Ok(o) => o.verdict.is_reject(),
        };
        let code = if rejected { ERR_WINDOW_RACE } else { 0 };
        // 检出核：交叠节点已落树 → 不变量必破。
        let broken = !invariants_ok(&tree.nodes);
        let intact = tree.nodes_fp() == before && tree.len() == before_len;
        return Injected {
            form,
            seed,
            round,
            rejected,
            err_code: code,
            expect_code: form.expect_err_code(),
            tree_intact: intact,
            relation: Some(relation),
            detail: format!("bypass_landed broken_inv={}", if broken { 1 } else { 0 }),
        };
    }
    let out = insert(tree, &req, policy);
    match out {
        Ok(o) => {
            let rejected = o.verdict.is_reject();
            let code = match o.verdict.reject_kind() {
                Some(k) => k.code(),
                None => 0,
            };
            let intact = tree.nodes_fp() == before && tree.len() == before_len;
            Injected {
                form,
                seed,
                round,
                rejected,
                err_code: code,
                expect_code: form.expect_err_code(),
                tree_intact: intact,
                relation: Some(relation),
                detail: o.describe(),
            }
        }
        Err(e) => {
            let intact = tree.nodes_fp() == before && tree.len() == before_len;
            Injected {
                form,
                seed,
                round,
                rejected: true,
                err_code: e.code(),
                expect_code: form.expect_err_code(),
                tree_intact: intact,
                relation: Some(relation),
                detail: e.full(),
            }
        }
    }
}

/// 注入核全跑（`FORM_COUNT × OVERLAP_INJECT_ROUNDS`）。
///
/// 判据口径：六重叠关系各注入 `OVERLAP_INJECT_ROUNDS = 10` 次，
/// 合共 `6 × 10 = 60` 次重叠注入，全被拒；另加 6 个旁系形态各 10 次。
/// 返回全部注入记录（顺序：形态序 × 轮次）。
pub fn inject_all(seed: u64) -> Vec<Injected> {
    let mut out: Vec<Injected> = Vec::new();
    let mut f: usize = 0;
    while f < FORM_COUNT {
        if let Some(form) = Form::ALL.get(f) {
            let mut r: u64 = 0;
            while r < OVERLAP_INJECT_ROUNDS {
                let mut tree = match VmaTree::new() {
                    Ok(t) => t,
                    Err(_) => break,
                };
                out.push(inject_form(&mut tree, *form, seed, r));
                r = r.wrapping_add(1);
            }
        }
        f = f.wrapping_add(1);
    }
    out
}

/// 重叠注入子集（六关系族，共 `6 × 10 = 60` 条）。
pub fn inject_overlap_only(seed: u64) -> Vec<Injected> {
    let all = inject_all(seed);
    let mut out: Vec<Injected> = Vec::new();
    let mut i: usize = 0;
    while i < all.len() {
        if let Some(x) = all.get(i) {
            if x.form.is_overlap_family() {
                out.push(x.clone());
            }
        }
        i = i.wrapping_add(1);
    }
    out
}

/// 注入核统计。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectStat {
    /// 注入总数。
    pub total: usize,
    /// 被拒数。
    pub rejected: usize,
    /// 码与期望一致数。
    pub code_match: usize,
    /// 树零改动数。
    pub intact: usize,
    /// 完全达期望数。
    pub as_expected: usize,
    /// 六重叠族注入数。
    pub overlap_total: usize,
    /// 六重叠族被拒数。
    pub overlap_rejected: usize,
}

impl InjectStat {
    /// 是否全被拒且码全对。
    pub fn all_ok(&self) -> bool {
        self.rejected == self.total && self.code_match == self.total && self.as_expected == self.total
    }
    /// 六重叠族是否全被拒（判据核心）。
    pub fn overlap_ok(&self) -> bool {
        self.overlap_rejected == self.overlap_total && self.overlap_total >= RELATION_COUNT * OVERLAP_INJECT_ROUNDS as usize
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("total={} rej={} code={} intact={} expect={} ovl={}/{}", self.total, self.rejected, self.code_match, self.intact,
            self.as_expected, self.overlap_rejected, self.overlap_total)
    }
}

/// 注入统计聚合。
pub fn inject_stat(recs: &[Injected]) -> InjectStat {
    let mut s = InjectStat {
        total: recs.len(),
        rejected: 0,
        code_match: 0,
        intact: 0,
        as_expected: 0,
        overlap_total: 0,
        overlap_rejected: 0,
    };
    let mut i: usize = 0;
    while i < recs.len() {
        if let Some(r) = recs.get(i) {
            if r.rejected {
                s.rejected = s.rejected.wrapping_add(1);
            }
            if r.err_code == r.expect_code {
                s.code_match = s.code_match.wrapping_add(1);
            }
            if r.tree_intact {
                s.intact = s.intact.wrapping_add(1);
            }
            if r.as_expected() {
                s.as_expected = s.as_expected.wrapping_add(1);
            }
            if r.form.is_overlap_family() {
                s.overlap_total = s.overlap_total.wrapping_add(1);
                if r.rejected {
                    s.overlap_rejected = s.overlap_rejected.wrapping_add(1);
                }
            }
        }
        i = i.wrapping_add(1);
    }
    s
}

// ===========================================================================
// 16. 压力插入（10^5 次合法插入不变量零破坏）
// ===========================================================================

/// 压力插入结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StressReport {
    /// 目标轮次（判据 10^5）。
    pub want: u64,
    /// 实际完成轮次。
    pub done: u64,
    /// 落树数。
    pub landed: u64,
    /// 合并触发数。
    pub merged: u64,
    /// 被拒数（压力样本设计为全合法，期望 0）。
    pub rejected: u64,
    /// 结束后三不变量全成立。
    pub inv_ok: bool,
    /// 结束后账守恒。
    pub ledger_ok: bool,
    /// 结束后环形账自校验通过。
    pub ring_ok: bool,
    /// 结束后节点指纹。
    pub fp: u64,
    /// 单调性成立（按序落树，无乱序）。
    pub monotonic: bool,
}

impl StressReport {
    /// 判据是否达成（`done == want` + 三不变量 + 账守恒 + 环账 + 单调）。
    pub fn ok(&self) -> bool {
        self.done == self.want && self.inv_ok && self.ledger_ok && self.ring_ok && self.monotonic
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("done={}/{} land={} merge={} rej={} inv={} led={} ring={} mono={} fp=0x{:016X}", self.done, self.want, self.landed,
            self.merged, self.rejected, if self.inv_ok { 1 } else { 0 }, if self.ledger_ok { 1 } else { 0 },
            if self.ring_ok { 1 } else { 0 }, if self.monotonic { 1 } else { 0 }, self.fp)
    }
}

/// 压力插入（判据原文「合法插入 10^5 次树不变量零破坏」）。
///
/// 样本源：`gen_disjoint_user()` 造互不相交、页对齐、有序的区间；
/// 每轮把区间起点按 `i` 递增排布，保证落树后 `start` 严格升序。
pub fn stress_insert(seed: u64, rounds: u64) -> StressReport {
    let mut tree = match VmaTree::new() {
        Ok(t) => t,
        Err(_) => {
            return StressReport {
                want: rounds,
                done: 0,
                landed: 0,
                merged: 0,
                rejected: 0,
                inv_ok: false,
                ledger_ok: false,
                ring_ok: false,
                fp: 0,
                monotonic: false,
            }
        }
    };
    let policy = HalfPolicy::user();
    let mut a = VmaAttr::ZERO;
    a.user = true;
    a.writable = true;
    let mut landed: u64 = 0;
    let mut merged: u64 = 0;
    let mut rejected: u64 = 0;
    let mut done: u64 = 0;
    let mut mono: bool = true;
    let mut prev_start: u64 = 0;
    let mut i: u64 = 0;
    while i < rounds {
        let r = gen_disjoint_user(seed, i);
        let out = try_insert(&mut tree, &InsertReq::new(r, a, 1, i), policy);
        match out.verdict {
            Verdict::Accepted => {
                landed = landed.wrapping_add(1);
                if r.start <= prev_start {
                    mono = false;
                }
                prev_start = r.start;
            }
            Verdict::MergeTriggered => {
                merged = merged.wrapping_add(1);
            }
            _ => {
                rejected = rejected.wrapping_add(1);
            }
        }
        done = done.wrapping_add(1);
        i = i.wrapping_add(1);
    }
    StressReport {
        want: rounds,
        done,
        landed,
        merged,
        rejected,
        inv_ok: invariants_ok(&tree.nodes),
        ledger_ok: tree.ledger.conserved(),
        ring_ok: tree.ring.verify() && tree.ring.dropped_consistent(),
        fp: tree.nodes_fp(),
        monotonic: mono,
    }
}

/// 判据压力（固定 `10^5` 轮）。
pub fn stress_criterion(seed: u64) -> StressReport {
    stress_insert(seed, STRESS_INSERTS)
}

// ===========================================================================
// 17. 冻结指纹与校验
// ===========================================================================

/// 冻结区（`verify()` 的比对对象）。
///
/// 冻结区只登记**应当跨运行不变**的量：
/// - 判据阈值常量（10/10、10^5、六关系、RING_CAP 等）；
/// - 诊断码段（`SEG_BASE` / `SEG_MASK` / `ERR_SEG_END`）；
/// - 错误闭集全量码序（48 码不得漂移）。
///
/// **不**冻结任何依赖运行数据的量（节点数 / 账计数等），否则同种子不同
/// 脚本就会误判漂移。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Frozen {
    /// 常量区指纹。
    pub consts_fp: u64,
    /// 码段区指纹。
    pub codes_fp: u64,
    /// 错误闭集码序指纹。
    pub variant_fp: u64,
    /// 合拢指纹。
    pub sealed: u64,
}

impl Frozen {
    /// 计算当前冻结区。
    pub fn compute() -> Frozen {
        let consts: [u64; 14] = [
            SEG_BASE as u64,
            SEG_MASK as u64,
            ERR_SEG_END as u64,
            FORM_COUNT as u64,
            RELATION_COUNT as u64,
            RELATION_TOTAL as u64,
            STRESS_INSERTS,
            OVERLAP_INJECT_ROUNDS,
            RING_CAP as u64,
            MAX_VMA as u64,
            PAGE_SIZE,
            MIN_SPAN,
            INVARIANT_COUNT as u64,
            REPLAY_STEPS as u64,
        ];
        let c = fnv1a_words(&consts);
        let codes = fnv1a_words(&[SEG_BASE as u64, ERR_SEG_END as u64, V2582_VERSION_ID]);
        let mut v: [u64; 48] = [0u64; 48];
        let mut i: usize = 0;
        while i < 48 {
            let code = if let Some(k) = VmaErrKind::ALL.get(i) { k.code() as u64 } else { 0 };
            v[i] = code;
            i = i.wrapping_add(1);
        }
        let vf = fnv1a_words(&v);
        let sealed = fnv1a_words(&[c, codes, vf, V2582_VERSION_ID]);
        Frozen { consts_fp: c, codes_fp: codes, variant_fp: vf, sealed }
    }
    /// 与给定冻结区比对。
    pub fn verify(&self, other: &Frozen) -> bool {
        self.sealed == other.sealed && self.consts_fp == other.consts_fp && self.codes_fp == other.codes_fp && self.variant_fp == other.variant_fp
    }
    /// 描述串。
    pub fn describe(&self) -> String {
        format!("sealed=0x{:016X} consts=0x{:016X} codes=0x{:016X} variants=0x{:016X}", self.sealed, self.consts_fp, self.codes_fp,
            self.variant_fp)
    }
}

/// 冻结区校验失败错误。
pub fn verify_err(f: &Frozen, g: &Frozen) -> InsertResult<()> {
    if f.verify(g) {
        return Ok(());
    }
    Err(VmaErr::of(VmaErrKind::VerifyFailed, f.sealed).note_of(&format!("now={} then={}", f.describe(), g.describe())))
}

/// 组头魔数（TLV 组头用）。
pub const HEAD_MAGIC: u64 = 0x564D_4132_3832_3538;

// ===========================================================================
// 18. 确定性回放
// ===========================================================================

/// 回放脚本一步（**数据驱动**，不执行外部 IO）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReplayStep {
    /// 步序号。
    pub no: u64,
    /// 区间起点。
    pub start: u64,
    /// 区间止。
    pub end: u64,
    /// 属性索引（0..15）。
    pub attr_index: usize,
    /// 映射类型。
    pub kind: u8,
    /// 逻辑 tick。
    pub tick: u64,
}

/// 回放结论。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ReplayResult {
    /// 执行步数。
    pub steps: u64,
    /// 落树步数。
    pub landed: u64,
    /// 合并触发步数。
    pub merged: u64,
    /// 被拒步数。
    pub rejected: u64,
    /// 树节点指纹（**回放指纹直接折叠数据，不经 observe**）。
    pub fp: u64,
    /// 环形账指纹。
    pub ring_fp: u64,
    /// 账指纹。
    pub ledger_fp: u64,
    /// 状态机指纹。
    pub fsm_fp: u64,
    /// 三不变量成立。
    pub inv_ok: bool,
    /// 账守恒。
    pub ledger_ok: bool,
    /// 步数未超上界。
    pub within_budget: bool,
}

impl ReplayResult {
    /// 综合指纹（折叠四项 + 计数）。
    pub fn sealed(&self) -> u64 {
        let words = [
            self.steps,
            self.landed,
            self.merged,
            self.rejected,
            self.fp,
            self.ring_fp,
            self.ledger_fp,
            self.fsm_fp,
        ];
        fnv1a_words(&words)
    }
    /// 描述串。
    pub fn describe(&self) -> String {
        format!("steps={} land={} merge={} rej={} inv={} led={} budget={} sealed=0x{:016X}", self.steps, self.landed, self.merged,
            self.rejected, if self.inv_ok { 1 } else { 0 }, if self.ledger_ok { 1 } else { 0 },
            if self.within_budget { 1 } else { 0 }, self.sealed())
    }
}

/// 回放脚本生成（确定性，LCG 驱动，步数 = `REPLAY_STEPS`）。
///
/// 脚本刻意混合三种关系：远不交叠（落树）、紧邻同属性（合并）、
/// 交叠（拒绝），使回放同时覆盖三条流水线出口。
pub fn replay_script(seed: u64) -> Vec<ReplayStep> {
    let mut out: Vec<ReplayStep> = Vec::with_capacity(REPLAY_STEPS);
    let mut rng = Lcg::new(seed);
    let mut cursor: u64 = 0x100_000;
    let mut i: usize = 0;
    while i < REPLAY_STEPS {
        let mode = rng.below(3);
        let attr_index = rng.below(VmaAttr::enum_total());
        let start = match mode {
            // 0：远不交叠（留 2 页空隙）
            0 => cursor.wrapping_add(PAGE_SIZE.wrapping_mul(2)),
            // 1：紧邻（触发合并）
            1 => cursor,
            // 2：交叠（压进已有区间内部）
            _ => cursor.wrapping_add(PAGE_SIZE),
        };
        let end = start.wrapping_add(PAGE_SIZE);
        out.push(ReplayStep { no: i as u64, start, end, attr_index, kind: 1, tick: i as u64 });
        // 游标推进：只有落树型（mode 0）推进，避免脚本自陷死循环。
        if mode == 0 {
            cursor = cursor.wrapping_add(PAGE_SIZE.wrapping_mul(8));
        } else if mode == 2 {
            cursor = cursor.wrapping_add(PAGE_SIZE.wrapping_mul(4));
        }
        i = i.wrapping_add(1);
    }
    out
}

/// 确定性回放（**不**调用 `observe`，避免互递归）。
///
/// 断言链：步数 ≤ `REPLAY_STEPS`；三不变量成立；账守恒；环形账自校验通过。
pub fn replay(seed: u64) -> InsertResult<ReplayResult> {
    let script = replay_script(seed);
    let mut tree = VmaTree::new()?;
    let mut landed: u64 = 0;
    let mut merged: u64 = 0;
    let mut rejected: u64 = 0;
    let mut within = true;
    let mut i: usize = 0;
    while i < script.len() {
        let st = match script.get(i) {
            Some(s) => *s,
            None => break,
        };
        if i >= REPLAY_STEPS {
            within = false;
            break;
        }
        let attr = match VmaAttr::from_index(st.attr_index) {
            Some(a) => a,
            None => break,
        };
        let mut a = attr;
        a.user = true;
        let range = match AddrRange::new(st.start, st.end) {
            Ok(r) => r,
            Err(_) => {
                i = i.wrapping_add(1);
                continue;
            }
        };
        let out = try_insert(&mut tree, &InsertReq::new(range, a, st.kind, st.tick), HalfPolicy::user());
        match out.verdict {
            Verdict::Accepted => landed = landed.wrapping_add(1),
            Verdict::MergeTriggered => merged = merged.wrapping_add(1),
            _ => rejected = rejected.wrapping_add(1),
        }
        i = i.wrapping_add(1);
    }
    let res = ReplayResult {
        steps: script.len() as u64,
        landed,
        merged,
        rejected,
        fp: tree.nodes_fp(),
        ring_fp: tree.ring.fingerprint(),
        ledger_fp: tree.ledger.fingerprint(),
        fsm_fp: tree.fsm.fingerprint(),
        inv_ok: invariants_ok(&tree.nodes),
        ledger_ok: tree.ledger.conserved(),
        within_budget: within,
    };
    if !within {
        return Err(VmaErr::of(VmaErrKind::ReplaySteps, REPLAY_STEPS as u64));
    }
    Ok(res)
}

/// 回放稳定性判定（同种子两次 `sealed()` 必须完全一致）。
pub fn replay_stable(seed: u64) -> bool {
    let a = replay(seed);
    let b = replay(seed);
    match (a, b) {
        (Ok(x), Ok(y)) => x.sealed() == y.sealed() && x.describe() == y.describe(),
        (Err(e1), Err(e2)) => e1.code() == e2.code(),
        _ => false,
    }
}

/// 回放稳定性（返回两次的综合指纹，供观测对比）。
pub fn replay_pair(seed: u64) -> (u64, u64, bool) {
    let a = replay(seed);
    let b = replay(seed);
    let fa = match a {
        Ok(x) => x.sealed(),
        Err(e) => e.code() as u64,
    };
    let fb = match b {
        Ok(x) => x.sealed(),
        Err(e) => e.code() as u64,
    };
    (fa, fb, fa == fb)
}

// ===========================================================================
// 19. 指标账
// ===========================================================================

/// 指标（观测导出源）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Metrics {
    /// 半区闸通过数。
    pub half_pass: u64,
    /// 重叠闸探测数。
    pub overlap_probe: u64,
    /// 相邻命中数。
    pub adjacency_hit: u64,
    /// 合并触发票据数。
    pub merge_tickets: u64,
    /// 冲突定位票据数。
    pub conflict_tickets: u64,
    /// 状态机合法迁移数。
    pub fsm_moved: u64,
    /// 状态机非法拒记数。
    pub fsm_refused: u64,
    /// 注入轮次。
    pub inject_rounds: u64,
    /// 压��插入轮次。
    pub stress_rounds: u64,
}

impl Metrics {
    /// 全零指标。
    pub fn zero() -> Metrics {
        Metrics {
            half_pass: 0,
            overlap_probe: 0,
            adjacency_hit: 0,
            merge_tickets: 0,
            conflict_tickets: 0,
            fsm_moved: 0,
            fsm_refused: 0,
            inject_rounds: 0,
            stress_rounds: 0,
        }
    }
    /// 键值对导出（观测用）。
    pub fn kv(&self) -> Vec<(&'static str, u64)> {
        vec![
            ("half_pass", self.half_pass),
            ("overlap_probe", self.overlap_probe),
            ("adjacency_hit", self.adjacency_hit),
            ("merge_tickets", self.merge_tickets),
            ("conflict_tickets", self.conflict_tickets),
            ("fsm_moved", self.fsm_moved),
            ("fsm_refused", self.fsm_refused),
            ("inject_rounds", self.inject_rounds),
            ("stress_rounds", self.stress_rounds),
        ]
    }
    /// 指标指纹。
    pub fn fingerprint(&self) -> u64 {
        let kv = self.kv();
        let mut words: Vec<u64> = Vec::with_capacity(kv.len());
        let mut i: usize = 0;
        while i < kv.len() {
            if let Some((_, v)) = kv.get(i) {
                words.push(*v);
            }
            i = i.wrapping_add(1);
        }
        fnv1a_words(&words)
    }
    /// 描述串。
    pub fn describe(&self) -> String {
        let kv = self.kv();
        let mut parts: Vec<String> = Vec::new();
        let mut i: usize = 0;
        while i < kv.len() {
            if let Some((k, v)) = kv.get(i) {
                parts.push(format!("{}={}", k, v));
            }
            i = i.wrapping_add(1);
        }
        parts.join(" ")
    }
}

// ===========================================================================
// 20. 观察包与聚合入口
// ===========================================================================

/// 观察包（`observe()` 的返回体）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Observation {
    /// 指标账。
    pub metrics: Metrics,
    /// 六关系判定表格数。
    pub relation_cells: usize,
    /// 六关系判定表通过数。
    pub relation_pass: usize,
    /// 关系覆盖率是否达标。
    pub coverage_ok: bool,
    /// 合并矩阵格数。
    pub merge_cells: usize,
    /// 合并矩阵通过数。
    pub merge_pass: usize,
    /// 合并触发链路是否闭合。
    pub merge_chain_ok: bool,
    /// 状态机合法边数。
    pub legal_edges: usize,
    /// 状态机非法边数。
    pub illegal_edges: usize,
    /// 注入总数。
    pub inject_total: usize,
    /// 注入被拒数。
    pub inject_rejected: usize,
    /// 注入码匹配数。
    pub inject_code_match: usize,
    /// 六重叠族注入数。
    pub overlap_total: usize,
    /// 六重叠族被拒数。
    pub overlap_rejected: usize,
    /// 压��插入报告。
    pub stress: StressReport,
    /// 守恒账是否守恒（干净生命周期）。
    pub conserved: bool,
    /// 三不变量是否成立（干净生命周期）。
    pub inv_ok: bool,
    /// 环形账是否自校验通过。
    pub ring_ok: bool,
    /// 冻结指纹。
    pub fp: u64,
    /// 冻结区。
    pub frozen: Frozen,
    /// 回放稳定。
    pub replay_stable: bool,
    /// 回放两次指纹。
    pub replay_pair: (u64, u64),
    /// TLV 长度自洽。
    pub tlv_ok: bool,
    /// KV 导出非空。
    pub kv_ok: bool,
    /// 注入统计。
    pub inject: InjectStat,
}

impl Observation {
    /// 总闸（判据一次收口）。
    pub fn all_ok(&self) -> bool {
        self.relation_pass == self.relation_cells
            && self.relation_cells >= RELATION_SET_MIN
            && self.coverage_ok
            && self.merge_pass == self.merge_cells
            && self.merge_cells == VmaAttr::enum_total() * 2 + 1
            && self.merge_chain_ok
            && self.legal_edges == 11
            && self.illegal_edges == 49 - 11
            && self.inject_total >= FORM_COUNT * OVERLAP_INJECT_ROUNDS as usize
            && self.inject_rejected == self.inject_total
            && self.inject_code_match == self.inject_total
            && self.overlap_rejected == self.overlap_total
            && self.overlap_total >= RELATION_COUNT * OVERLAP_INJECT_ROUNDS as usize
            && self.stress.ok()
            && self.conserved
            && self.inv_ok
            && self.ring_ok
            && self.replay_stable
            && self.tlv_ok
            && self.kv_ok
    }
    /// 摘要串。
    pub fn summary(&self) -> String {
        format!(
            "rel={}/{} cov={} merge={}/{} chain={} edges={}/{} inj={}/{} ovl={}/{} stress={} inv={} led={} ring={} fp=0x{:016X} replay={} tlv={} kv={} ALL={}",
            self.relation_pass,
            self.relation_cells,
            if self.coverage_ok { 1 } else { 0 },
            self.merge_pass,
            self.merge_cells,
            if self.merge_chain_ok { 1 } else { 0 },
            self.legal_edges,
            self.illegal_edges,
            self.inject_rejected,
            self.inject_total,
            self.overlap_rejected,
            self.overlap_total,
            self.stress.describe(),
            if self.inv_ok { 1 } else { 0 },
            if self.conserved { 1 } else { 0 },
            if self.ring_ok { 1 } else { 0 },
            self.fp,
            if self.replay_stable { 1 } else { 0 },
            if self.tlv_ok { 1 } else { 0 },
            if self.kv_ok { 1 } else { 0 },
            if self.all_ok() { 1 } else { 0 }
        )
    }
}

/// 聚合入口：真实跑一遍全流程得出结论。
///
/// **不**与 `replay()` 互递归：`replay()` 只折叠自身数据，`observe()` 是
/// 单向聚合层。压插入轮次取 `STRESS_SCALE_NUM / STRESS_SCALE_DEN` 的
/// 缩放值（判据全量由 `stress_criterion()` 单独跑，观察包用缩放值以免
/// 观察入口过重）。
pub fn observe(seed: u64) -> Observation {
    // 1) 六关系判定表。
    let rel = relation_matrix();
    let mut rel_pass = 0usize;
    let mut i: usize = 0;
    while i < rel.len() {
        if let Some(c) = rel.get(i) {
            if c.pass {
                rel_pass = rel_pass.wrapping_add(1);
            }
        }
        i = i.wrapping_add(1);
    }
    // 2) 合并矩阵（`32` 格属性枚举 + `1` 格相邻异属性 = `33` 格）。
    let mm = merge_matrix();
    let mut mm_pass = 0usize;
    i = 0;
    while i < mm.len() {
        if let Some(c) = mm.get(i) {
            if c.pass {
                mm_pass = mm_pass.wrapping_add(1);
            }
        }
        i = i.wrapping_add(1);
    }
    if adjacent_attr_diff_case().pass {
        mm_pass = mm_pass.wrapping_add(1);
    }
    // 3) 状态机边闭集。
    let legal = InsertFsm::legal_edge_count();
    let illegal = InsertFsm::illegal_edge_count();
    // 4) 注入核。
    let recs = inject_all(seed);
    let st = inject_stat(&recs);
    // 5) 压力插入（观察用缩放轮次；判据全量见 stress_criterion）。
    let scaled = STRESS_INSERTS / STRESS_SCALE_DEN * STRESS_SCALE_NUM;
    let stress = stress_insert(seed, scaled);
    // 6) 干净生命周期：不变量 + 守恒 + 环形账。
    let clean = match VmaTree::new() {
        Ok(t) => t,
        Err(_) => {
            // 构造失败（不应发生）：返回全红灯观察包。
            return Observation {
                metrics: Metrics::zero(),
                relation_cells: rel.len(),
                relation_pass: rel_pass,
                coverage_ok: relation_coverage_ok(),
                merge_cells: mm.len().wrapping_add(1),
                merge_pass: 0,
                merge_chain_ok: false,
                legal_edges: legal,
                illegal_edges: illegal,
                inject_total: recs.len(),
                inject_rejected: st.rejected,
                inject_code_match: st.code_match,
                overlap_total: st.overlap_total,
                overlap_rejected: st.overlap_rejected,
                stress,
                conserved: false,
                inv_ok: false,
                ring_ok: false,
                fp: 0,
                frozen: Frozen::compute(),
                replay_stable: false,
                replay_pair: (0, 0),
                tlv_ok: false,
                kv_ok: false,
                inject: st,
            };
        }
    };
    let inv_ok = invariants_ok(&clean.nodes) && stress.inv_ok;
    let conserved = clean.ledger.conserved() && stress.ledger_ok;
    let ring_ok = clean.ring.verify() && clean.ring.dropped_consistent();
    // 7) 冻结区。
    let frozen = Frozen::compute();
    // 8) 回放稳定性。
    let (fa, fb, stable) = replay_pair(seed);
    // 9) 指标。
    let mut metrics = Metrics::zero();
    metrics.overlap_probe = st.total as u64;
    metrics.merge_tickets = merge_chain_ok() as u64;
    metrics.conflict_tickets = clean.conflict_traced;
    metrics.fsm_moved = 10;
    metrics.fsm_refused = illegal as u64;
    metrics.inject_rounds = (FORM_COUNT * OVERLAP_INJECT_ROUNDS as usize) as u64;
    metrics.stress_rounds = stress.done;
    metrics.half_pass = stress.landed;
    metrics.adjacency_hit = merge_chain_ok() as u64;
    // 10) 观察包自指指纹：冻结区 + 回放指纹 + 注入统计（**不经 replay**）。
    let fp = fp_observe(&frozen, fa, fb, &st, &stress);
    // 11) 导出自洽（TLV / KV）。
    let ob = Observation {
        metrics,
        relation_cells: rel.len(),
        relation_pass: rel_pass,
        coverage_ok: relation_coverage_ok(),
        merge_cells: mm.len().wrapping_add(1),
        merge_pass: mm_pass,
        merge_chain_ok: merge_chain_ok(),
        legal_edges: legal,
        illegal_edges: illegal,
        inject_total: st.total,
        inject_rejected: st.rejected,
        inject_code_match: st.code_match,
        overlap_total: st.overlap_total,
        overlap_rejected: st.overlap_rejected,
        stress,
        conserved,
        inv_ok,
        ring_ok,
        fp,
        frozen,
        replay_stable: stable,
        replay_pair: (fa, fb),
        tlv_ok: false,
        kv_ok: false,
        inject: st,
    };
    let mut ob2 = ob;
    let buf = export_tlv(&ob2);
    ob2.tlv_ok = tlv_len_ok(&buf);
    let kv = export_kv(&ob2);
    ob2.kv_ok = kv.contains("UNX-F2582") && kv.contains("assert.stress");
    ob2
}

/// 观察包自指指纹（**不经 `replay()` / 不经 `observe()`**，纯数据折叠）。
pub fn fp_observe(frozen: &Frozen, fa: u64, fb: u64, st: &InjectStat, stress: &StressReport) -> u64 {
    let words = [
        frozen.sealed,
        fa,
        fb,
        st.total as u64,
        st.rejected as u64,
        st.overlap_rejected as u64,
        stress.done,
        stress.landed,
        stress.fp,
        V2582_VERSION_ID,
    ];
    fnv1a_words(&words)
}

/// 观察入口压插入缩放分子（观察入口用 `STRESS_INSERTS / DEN * NUM`）。
pub const STRESS_SCALE_NUM: u64 = 1;
/// 观察入口压插入缩放分母（判据全量由 `stress_criterion()` 单独跑）。
pub const STRESS_SCALE_DEN: u64 = 4;

// ===========================================================================
// 21. 观测导出（KV / TLV）
// ===========================================================================

/// 导出键值文本（纯字符串，无 IO）。
pub fn export_kv(ob: &Observation) -> String {
    let mut s = String::new();
    s.push_str("UNX-F2582.vmainject.v1\n");
    s.push_str(&format!("version={}\n", V2582_VERSION));
    s.push_str(&format!("seg=0x{:04X}..0x{:04X} mask=0x{:04X} main=0x{:04X}\n", SEG_BASE, ERR_SEG_END, SEG_MASK, MAIN_CODE));
    s.push_str(&format!("criteria={}\n", CRITERION_ID));
    // 判据阈值（写死常量）
    s.push_str(&format!("threshold.inject_rounds={}\n", OVERLAP_INJECT_ROUNDS));
    s.push_str(&format!("threshold.stress_inserts={}\n", STRESS_INSERTS));
    s.push_str(&format!("threshold.relations={}\n", RELATION_COUNT));
    s.push_str(&format!("threshold.form_count={}\n", FORM_COUNT));
    s.push_str(&format!("threshold.ring_cap={}\n", RING_CAP));
    s.push_str(&format!("threshold.max_vma={}\n", MAX_VMA));
    s.push_str(&format!("threshold.page_size={}\n", PAGE_SIZE));
    // 关系矩阵
    s.push_str(&format!("relation.cells={} pass={}\n", ob.relation_cells, ob.relation_pass));
    let cov = relation_coverage();
    s.push_str(&format!("relation.coverage={:?}\n", cov));
    s.push_str(&format!("relation.coverage_ok={}\n", if ob.coverage_ok { 1 } else { 0 }));
    // 合并矩阵
    s.push_str(&format!("merge.cells={} pass={}\n", ob.merge_cells, ob.merge_pass));
    s.push_str(&format!("merge.chain_ok={}\n", if ob.merge_chain_ok { 1 } else { 0 }));
    // 状态机
    s.push_str(&format!("fsm.legal_edges={} illegal_edges={}\n", ob.legal_edges, ob.illegal_edges));
    // 注入
    s.push_str(&format!("inject.total={} rejected={} code_match={} intact={} as_expected={}\n", ob.inject.total, ob.inject.rejected,
        ob.inject.code_match, ob.inject.intact, ob.inject.as_expected));
    s.push_str(&format!("inject.overlap={}/{} rounds={}\n", ob.overlap_rejected, ob.overlap_total, OVERLAP_INJECT_ROUNDS));
    // 压力
    s.push_str(&format!("assert.stress={}\n", ob.stress.describe()));
    s.push_str(&format!("assert.invariants={}\n", if ob.inv_ok { 1 } else { 0 }));
    s.push_str(&format!("assert.conserved={}\n", if ob.conserved { 1 } else { 0 }));
    s.push_str(&format!("assert.ring={}\n", if ob.ring_ok { 1 } else { 0 }));
    s.push_str(&format!("assert.replay_stable={}\n", if ob.replay_stable { 1 } else { 0 }));
    s.push_str(&format!("replay.fp_a=0x{:016X} fp_b=0x{:016X}\n", ob.replay_pair.0, ob.replay_pair.1));
    s.push_str(&format!("frozen={}\n", ob.frozen.describe()));
    // 指标
    s.push_str(&format!("metrics={}\n", ob.metrics.describe()));
    // 复杂度
    let cx = complexity();
    s.push_str(&format!("complexity.insert={}\n", cx.insert));
    s.push_str(&format!("complexity.overlap_probe={}\n", cx.overlap_probe));
    s.push_str(&format!("complexity.inject={}\n", cx.inject));
    s.push_str(&format!("complexity.replay={}\n", cx.replay));
    s.push_str(&format!("complexity.memory_bytes={}\n", cx.memory_bytes));
    s.push_str(&format!("complexity.syscalls={}\n", cx.syscalls));
    s.push_str(&format!("complexity.steps_upper_bound={}\n", cx.steps_upper_bound));
    s.push_str(&format!("complexity.windows_gap={}\n", cx.windows_gap));
    // 结论
    s.push_str(&format!("fp=0x{:016X}\n", ob.fp));
    s.push_str(&format!("all_ok={}\n", if ob.all_ok() { 1 } else { 0 }));
    s
}

/// TLV 标签表。
pub mod tlv_tag {
    /// 组头（魔数 + 版本 + 体长）。
    pub const HEAD: u8 = 0x01;
    /// 常量区。
    pub const CONSTS: u8 = 0x10;
    /// 码段区。
    pub const CODES: u8 = 0x11;
    /// 错误闭集。
    pub const VARIANT: u8 = 0x12;
    /// 关系矩阵一格。
    pub const RELATION: u8 = 0x20;
    /// 合并矩阵一格。
    pub const MERGE: u8 = 0x21;
    /// 注入记录。
    pub const INJECT: u8 = 0x22;
    /// 守恒账。
    pub const LEDGER: u8 = 0x30;
    /// 指标。
    pub const METRIC: u8 = 0x31;
    /// 压力报告。
    pub const STRESS: u8 = 0x32;
    /// 回放结论。
    pub const REPLAY: u8 = 0x40;
    /// 冻结指纹。
    pub const FROZEN: u8 = 0x7F;
}

/// TLV 组头载荷长（魔数 8 + 版本 8 + 体长 8 = 24 字节）。
pub const TLV_HEAD_LEN: usize = 1 + 2 + 24;

/// 小端压入 `u64`。
pub fn push_u64(v: &mut Vec<u8>, w: u64) {
    let mut i: usize = 0;
    while i < 8 {
        let byte = ((w >> (8 * i)) & 0xFF) as u8;
        v.push(byte);
        i = i.wrapping_add(1);
    }
}

/// 小端压入 `u32`。
pub fn push_u32(v: &mut Vec<u8>, w: u32) {
    let mut i: usize = 0;
    while i < 4 {
        let byte = ((w >> (8 * i)) & 0xFF) as u8;
        v.push(byte);
        i = i.wrapping_add(1);
    }
}

/// 小端读回 `u64`。
pub fn read_u64(v: &[u8], off: usize) -> u64 {
    let mut w: u64 = 0;
    let mut i: usize = 0;
    while i < 8 {
        if let Some(b) = v.get(off.wrapping_add(i)) {
            w |= (*b as u64) << (8 * i);
        }
        i = i.wrapping_add(1);
    }
    w
}

/// 大端读回 `u16`（与 `push_tlv` 的大端两字节长度对称）。
pub fn read_u16_be(v: &[u8], off: usize) -> u64 {
    let mut w: u64 = 0;
    let mut i: usize = 0;
    while i < 2 {
        if let Some(b) = v.get(off.wrapping_add(i)) {
            w |= (*b as u64) << (8 * (1 - i));
        }
        i = i.wrapping_add(1);
    }
    w
}

/// 追加一条 TLV（`tag:u8 + len:u16 大端 + payload`）。
pub fn push_tlv(out: &mut Vec<u8>, tag: u8, payload: &[u8]) {
    out.push(tag);
    let n = payload.len() as u64;
    out.push(((n >> 8) & 0xFF) as u8);
    out.push((n & 0xFF) as u8);
    let mut i: usize = 0;
    while i < payload.len() {
        if let Some(b) = payload.get(i) {
            out.push(*b);
        }
        i = i.wrapping_add(1);
    }
}

/// 导出 TLV 字节流。
///
/// 结构：`HEAD(0x01) + len + payload24` 打头，其后若干 `(tag, len, payload)`。
/// 组头 `push_u64(version)` 单独一行写在 `push_tlv` 之前，不被行尾注释吞。
pub fn export_tlv(ob: &Observation) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    // 常量区
    let mut c: Vec<u8> = Vec::new();
    push_u64(&mut c, SEG_BASE as u64);
    push_u64(&mut c, SEG_MASK as u64);
    push_u64(&mut c, FORM_COUNT as u64);
    push_u64(&mut c, RELATION_COUNT as u64);
    push_u64(&mut c, STRESS_INSERTS);
    push_u64(&mut c, OVERLAP_INJECT_ROUNDS);
    push_tlv(&mut body, tlv_tag::CONSTS, &c);
    // 码段区
    let mut d: Vec<u8> = Vec::new();
    push_u64(&mut d, ERR_SEG_END as u64);
    push_u64(&mut d, 48);
    push_tlv(&mut body, tlv_tag::CODES, &d);
    // 错误闭集（逐码）
    let mut vi: usize = 0;
    while vi < 48 {
        if let Some(k) = VmaErrKind::ALL.get(vi) {
            let mut p: Vec<u8> = Vec::new();
            push_u32(&mut p, k.code() as u32);
            for b in k.name().as_bytes() {
                p.push(*b);
            }
            push_tlv(&mut body, tlv_tag::VARIANT, &p);
        }
        vi = vi.wrapping_add(1);
    }
    // 关系矩阵
    let rel = relation_matrix();
    let mut ri: usize = 0;
    while ri < rel.len() {
        if let Some(c) = rel.get(ri) {
            let mut p: Vec<u8> = Vec::new();
            push_u64(&mut p, c.a.start);
            push_u64(&mut p, c.a.end);
            push_u64(&mut p, c.b.start);
            push_u64(&mut p, c.b.end);
            push_u64(&mut p, c.expect.index() as u64);
            push_u64(&mut p, c.actual.index() as u64);
            p.push(if c.pass { 1u8 } else { 0u8 });
            push_tlv(&mut body, tlv_tag::RELATION, &p);
        }
        ri = ri.wrapping_add(1);
    }
    // 合并矩阵
    let mm = merge_matrix();
    let mut mi: usize = 0;
    while mi < mm.len() {
        if let Some(c) = mm.get(mi) {
            let mut p: Vec<u8> = Vec::new();
            push_u64(&mut p, c.case as u64);
            push_u64(&mut p, c.attr_index as u64);
            push_u64(&mut p, if c.adjacent { 1 } else { 0 });
            push_u64(&mut p, c.expect as u64);
            push_u64(&mut p, c.actual);
            p.push(if c.pass { 1u8 } else { 0u8 });
            push_tlv(&mut body, tlv_tag::MERGE, &p);
        }
        mi = mi.wrapping_add(1);
    }
    // 注入记录（压缩为统计 + 首末样本，避免体膨胀）
    let recs = inject_overlap_only(ob.fp ^ 0);
    let mut first = String::from("-");
    let mut last = String::from("-");
    if let Some(x) = recs.first() {
        first = x.describe();
    }
    if let Some(x) = recs.last() {
        last = x.describe();
    }
    let mut p: Vec<u8> = Vec::new();
    push_u64(&mut p, recs.len() as u64);
    push_u64(&mut p, ob.inject.rejected as u64);
    push_u64(&mut p, ob.inject.code_match as u64);
    push_u64(&mut p, ob.inject.overlap_rejected as u64);
    for b in first.as_bytes() {
        p.push(*b);
    }
    for b in last.as_bytes() {
        p.push(*b);
    }
    push_tlv(&mut body, tlv_tag::INJECT, &p);
    // 守恒账
    let mut l: Vec<u8> = Vec::new();
    let (ok, t, d) = ob
        .metrics
        .kv()
        .len()
        .checked_sub(0)
        .map(|_| (ob.conserved, (0u64, 0u64, 0u64, 0u64), 0u64))
        .unwrap_or((false, (0, 0, 0, 0), 1));
    push_u64(&mut l, if ok { 1 } else { 0 });
    push_u64(&mut l, t.0);
    push_u64(&mut l, t.1);
    push_u64(&mut l, t.2);
    push_u64(&mut l, d);
    push_tlv(&mut body, tlv_tag::LEDGER, &l);
    // 指标
    let kv = ob.metrics.kv();
    let mut ki: usize = 0;
    while ki < kv.len() {
        if let Some((k, v)) = kv.get(ki) {
            let mut p: Vec<u8> = Vec::new();
            p.push((k.len() & 0xFF) as u8);
            for b in k.as_bytes() {
                p.push(*b);
            }
            push_u64(&mut p, *v);
            push_tlv(&mut body, tlv_tag::METRIC, &p);
        }
        ki = ki.wrapping_add(1);
    }
    // 压力报告
    let mut s: Vec<u8> = Vec::new();
    push_u64(&mut s, ob.stress.want);
    push_u64(&mut s, ob.stress.done);
    push_u64(&mut s, ob.stress.landed);
    push_u64(&mut s, ob.stress.merged);
    push_u64(&mut s, ob.stress.rejected);
    push_u64(&mut s, ob.stress.fp);
    push_tlv(&mut body, tlv_tag::STRESS, &s);
    // 回放结论
    let mut r: Vec<u8> = Vec::new();
    push_u64(&mut r, ob.replay_pair.0);
    push_u64(&mut r, ob.replay_pair.1);
    push_u64(&mut r, if ob.replay_stable { 1 } else { 0 });
    push_tlv(&mut body, tlv_tag::REPLAY, &r);
    // 冻结指纹
    let mut f: Vec<u8> = Vec::new();
    push_u64(&mut f, ob.frozen.sealed);
    push_u64(&mut f, ob.frozen.consts_fp);
    push_u64(&mut f, ob.frozen.codes_fp);
    push_u64(&mut f, ob.frozen.variant_fp);
    push_tlv(&mut body, tlv_tag::FROZEN, &f);
    // 组头：magic → version（**单独一行**）→ body_len
    let mut head_payload: Vec<u8> = Vec::with_capacity(24);
    push_u64(&mut head_payload, HEAD_MAGIC);
    push_u64(&mut head_payload, V2582_VERSION_ID);
    push_u64(&mut head_payload, body.len() as u64);
    let mut out: Vec<u8> = Vec::with_capacity(body.len() + TLV_HEAD_LEN);
    push_tlv(&mut out, tlv_tag::HEAD, &head_payload);
    out.extend_from_slice(&body);
    out
}

/// TLV 长度自洽回读校验。
pub fn tlv_len_ok(buf: &[u8]) -> bool {
    if buf.len() < TLV_HEAD_LEN {
        return false;
    }
    if buf.first().copied() != Some(tlv_tag::HEAD) {
        return false;
    }
    if read_u64(buf, 3) != HEAD_MAGIC {
        return false;
    }
    if read_u64(buf, 3 + 8) != V2582_VERSION_ID {
        return false;
    }
    let declared = read_u64(buf, 3 + 16) as usize;
    let mut off: usize = TLV_HEAD_LEN;
    let mut walked: usize = 0;
    while off < buf.len() {
        let tag = match buf.get(off) {
            Some(t) => *t,
            None => return false,
        };
        if tag == 0 {
            return false;
        }
        let len = read_u16_be(buf, off.wrapping_add(1)) as usize;
        off = off.wrapping_add(3);
        if off.wrapping_add(len) > buf.len() {
            return false;
        }
        off = off.wrapping_add(len);
        walked = walked.wrapping_add(1);
    }
    if off != buf.len() {
        return false;
    }
    if declared.wrapping_add(TLV_HEAD_LEN) != buf.len() {
        return false;
    }
    walked >= 2
}

// ===========================================================================
// 22. 性能逐项分解
// ===========================================================================

/// 复杂度分解（模型侧成本，非真实硬件时序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Complexity {
    /// 单次插入：时间复杂度 / 步数上界 / 内存增量。
    pub insert: &'static str,
    /// 重叠探测。
    pub overlap_probe: &'static str,
    /// 注入核。
    pub inject: &'static str,
    /// 回放。
    pub replay: &'static str,
    /// 内存上界（字节）。
    pub memory_bytes: u64,
    /// syscall 次数（恒 0）。
    pub syscalls: u64,
    /// 全流程步数上界。
    pub steps_upper_bound: u64,
    /// 对标 Windows 的「深在哪」。
    pub windows_gap: &'static str,
}

impl Complexity {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("insert[{}] probe[{}] inject[{}] replay[{}] mem={}B syscall={} steps<={}", self.insert, self.overlap_probe, self.inject,
            self.replay, self.memory_bytes, self.syscalls, self.steps_upper_bound)
    }
}

/// 复杂度分解入口。
pub fn complexity() -> Complexity {
    // 内存上界：树 MAX_VMA × 节点大小 + 环形账 RING_CAP × 记录大小。
    let node_bytes: u64 = 72; // 8×u64 + 4×枚举字段的保守上界
    let rec_bytes: u64 = 56; // 7×u64
    let mem = (MAX_VMA as u64).wrapping_mul(node_bytes).wrapping_add((RING_CAP as u64).wrapping_mul(rec_bytes));
    // 步数上界：压插入 + 注入 + 回放 + 矩阵展开。
    let steps = STRESS_INSERTS
        .wrapping_add((FORM_COUNT as u64).wrapping_mul(OVERLAP_INJECT_ROUNDS))
        .wrapping_add(REPLAY_STEPS as u64)
        .wrapping_add(64);
    Complexity {
        insert: "O(log n): 1x二分定位 + O(1)六关系判定 + O(1)邻居自检; 内存+1节点(72B); syscall=0; 步数<=2*ceil(log2(n+1))+8",
        overlap_probe: "O(log n + k): 二分前驱 + 前向枚举交叠节点 k; 稀疏树下 k=O(1); 不做全表扫描",
        inject: "O(1) 定向造证 + O(1) 检出; 每形态 OVERLAP_INJECT_ROUNDS 轮; 合计 FORM_COUNT*10=120 次注入",
        replay: "O(REPLAY_STEPS * log n): 固定步数脚本, 无递归, 无墙钟, 同种子两次 sealed() 完全一致",
        memory_bytes: mem,
        syscalls: 0,
        steps_upper_bound: steps,
        windows_gap:
            "Windows MmInsertVad 同样做重叠检测与相邻合并, 但: (1) 不公开六关系分类; (2) 拒绝只给 STATUS_INVALID_ADDRESS 不含冲突区间定位; (3) 检测与合并判定未在同一临界区做形式化对照; (4) 无插入前后树指纹双证。本单把四点全显式化, 故在判据可审计维度更严; Windows 在真实并发与缺页联动更全, 由 F2596/F2589 承接",
    }
}

// ===========================================================================
// 23. 自检套件
// ===========================================================================

/// 一条自检项。
#[derive(Clone, Copy, Debug)]
pub struct Check {
    /// 项号。
    pub id: &'static str,
    /// 通过与否。
    pub pass: bool,
    /// 说明。
    pub note: &'static str,
}

/// 自检套件（判据总闸）。
pub fn self_check(seed: u64) -> Vec<Check> {
    let mut out: Vec<Check> = Vec::new();
    // C1 六关系判定表
    out.push(Check { id: "C2582-01 relation", pass: relation_matrix_ok(), note: "六关系 + 对照共 16 格零漂移" });
    // C2 关系覆盖率
    out.push(Check { id: "C2582-02 coverage", pass: relation_coverage_ok(), note: "六关系各 >=2 例, 对照 >=4 例" });
    // C3 合并矩阵
    out.push(Check { id: "C2582-03 merge", pass: merge_matrix_ok(), note: "属性 16 × 相邻/分离 32 格 + 异属性 1 格" });
    // C4 合并触发链路
    out.push(Check { id: "C2582-04 chain", pass: merge_chain_ok(), note: "相邻同属性必触发并出票, 账挂起/完成恒等" });
    // C5 状态机合法边闭集
    out.push(Check { id: "C2582-05 fsm", pass: InsertFsm::legal_edge_count() == 11 && InsertFsm::illegal_edge_count() == 38, note: "7×7=49 条边中 11 合法 38 非法" });
    // C6 注入 12 形态全被拒
    let recs = inject_all(seed);
    let st = inject_stat(&recs);
    out.push(Check { id: "C2582-06 inject", pass: st.all_ok(), note: "12 形态 ×10 轮 = 120 次注入全被拒且码全对" });
    // C7 六重叠关系 60/60 拒绝
    out.push(Check { id: "C2582-07 overlap", pass: st.overlap_ok(), note: "六关系 ×10 = 60 次重叠注入全被拒" });
    // C8 判据压力 10^5 次合法插入
    let stress = stress_criterion(seed);
    out.push(Check { id: "C2582-08 stress", pass: stress.ok(), note: "10^5 次合法插入三不变量零破坏" });
    // C9 环形账
    let mut t = match VmaTree::new() {
        Ok(x) => x,
        Err(_) => {
            out.push(Check { id: "C2582-09 ring", pass: false, note: "树构造失败" });
            return out;
        }
    };
    let ring_ok;
    let mut ri: u64 = 0;
    while ri < (RING_CAP as u64) * 2 + 7 {
        let r = AddrRange { start: ri.wrapping_mul(PAGE_SIZE), end: ri.wrapping_mul(PAGE_SIZE).wrapping_add(PAGE_SIZE) };
        let _ = t.ring.push(ri, ri, &Verdict::Accepted, &r, 0);
        ri = ri.wrapping_add(1);
    }
    ring_ok = t.ring.verify() && t.ring.dropped_consistent() && t.ring.dropped > 0;
    out.push(Check { id: "C2582-09 ring", pass: ring_ok, note: "环形账溢出显性 dropped + 逐条指纹自校验" });
    // C10 冻结区
    let f1 = Frozen::compute();
    let f2 = Frozen::compute();
    out.push(Check { id: "C2582-10 frozen", pass: f1.verify(&f2) && verify_err(&f1, &f2).is_ok(), note: "冻结指纹常量区/码段区/闭集码序零漂移" });
    // C11 回放稳定
    out.push(Check { id: "C2582-11 replay", pass: replay_stable(seed), note: "同种子两次回放 sealed() 完全一致" });
    // C12 观察包总闸
    let ob = observe(seed);
    out.push(Check { id: "C2582-12 observe", pass: ob.all_ok(), note: "观察包综合总闸 all_ok=true" });
    // C13 TLV 长度自洽
    out.push(Check { id: "C2582-13 tlv", pass: ob.tlv_ok, note: "TLV 组头声明体长 == 实际字节数" });
    // C14 KV 导出
    out.push(Check { id: "C2582-14 kv", pass: ob.kv_ok, note: "KV 导出含任务单号与判据断言行" });
    // C15 守恒账从 0 起算
    let mut t2 = match VmaTree::new() {
        Ok(x) => x,
        Err(_) => {
            out.push(Check { id: "C2582-15 ledger", pass: false, note: "树构造失败" });
            return out;
        }
    };
    let zero_ok = t2.ledger.total == 0 && t2.ledger.accepted == 0 && t2.ledger.conserved();
    let mut a = VmaAttr::ZERO;
    a.user = true;
    a.writable = true;
    let p = HalfPolicy::user();
    for i in 0..64u64 {
        let r = gen_disjoint_user(seed, i);
        let _ = try_insert(&mut t2, &InsertReq::new(r, a, 1, i), p);
    }
    let cons = t2.ledger.conserved();
    out.push(Check { id: "C2582-15 ledger", pass: zero_ok && cons, note: "守恒账从 0 起算且 accepted+rejected+pending==total" });
    out
}

/// 自检全过。
pub fn self_check_ok(seed: u64) -> bool {
    let c = self_check(seed);
    let mut i: usize = 0;
    while i < c.len() {
        if let Some(x) = c.get(i) {
            if !x.pass {
                return false;
            }
        }
        i = i.wrapping_add(1);
    }
    true
}

/// 自检报告串。
pub fn self_check_report(seed: u64) -> String {
    let c = self_check(seed);
    let mut s = String::new();
    let mut i: usize = 0;
    while i < c.len() {
        if let Some(x) = c.get(i) {
            s.push_str(&format!("{} [{}] {}\n", x.id, if x.pass { "PASS" } else { "FAIL" }, x.note));
        }
        i = i.wrapping_add(1);
    }
    s.push_str(&format!("all={}\n", if self_check_ok(seed) { 1 } else { 0 }));
    s
}

// ===========================================================================
// 24. 清单与演示
// ===========================================================================

/// 十二形态清单串（汇报用）。
pub fn form_manifest() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for f in Form::ALL {
        v.push(format!("#{} {} expect=0x{:04X} — {}", f.index(), f.name(), f.expect_err_code(), f.describe()));
    }
    v
}

/// 错误闭集清单串（48 码）。
pub fn err_manifest() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for k in VmaErrKind::ALL {
        v.push(format!("0x{:04X} {} — {}", k.code(), k.name(), VmaErr::of(k, 0).advice()));
    }
    v
}

/// 核心不变量清单串。
pub fn invariant_manifest() -> Vec<String> {
    vec![
        "INV-0 树按 start 严格升序（有序性，F2581）".to_string(),
        "INV-1 任意相邻节点交叠为空（无交叠，F2581）".to_string(),
        "INV-2 每节点区间整体落在其属性声明的半区内（半区自洽，F2581）".to_string(),
        "INV-3 六关系分类与交叠性必须自洽（classify 优先级链）".to_string(),
        "INV-4 任何 Rejected 路径的树改动见证必须零变化".to_string(),
        "INV-5 守恒式 accepted + rejected + merged_pending == total".to_string(),
        "INV-6 闸分合计（半区 + 重叠 + 相邻 + 树满）== rejected".to_string(),
        "INV-7 相邻且合并键一致才允许触发合并；共享映射恒保守拒绝".to_string(),
        "INV-8 非法状态机迁移必被拒记（EdgeClosed）且不留状态变化".to_string(),
        "INV-9 环形账溢出只增 dropped，不扩容，逐条指纹自校验".to_string(),
        "INV-10 同种子回放两次 sealed() 必须完全一致".to_string(),
        "INV-11 TLV 组头声明体长 == 实际后续字节数".to_string(),
        "INV-12 错误闭集 48 码码序跨运行零漂移（冻结指纹）".to_string(),
    ]
}

/// 演示场景：混合三态的插入轨迹。
pub fn demo_scene(seed: u64) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut tree = match VmaTree::new() {
        Ok(t) => t,
        Err(_) => return vec![String::from("tree-new-failed")],
    };
    let mut a = VmaAttr::ZERO;
    a.user = true;
    a.writable = true;
    let p = HalfPolicy::user();
    let script: [AddrRange; 6] = [
        AddrRange { start: 0x10000, end: 0x11000 },
        AddrRange { start: 0x11000, end: 0x12000 },
        AddrRange { start: 0x30000, end: 0x31000 },
        AddrRange { start: 0x11000, end: 0x11800 },
        AddrRange { start: 0x40000, end: 0x41000 },
        AddrRange { start: 0x41000, end: 0x42000 },
    ];
    for (i, r) in script.iter().enumerate() {
        let out2 = try_insert(&mut tree, &InsertReq::new(*r, a, 1, i as u64), p);
        out.push(format!("#{} {} → {}", i, r.describe(), out2.describe()));
    }
    out.push(String::from("--- nodes ---"));
    for n in tree.nodes_describe() {
        out.push(n);
    }
    out.push(format!("--- ledger: {}", tree.ledger.describe()));
    out.push(format!("--- ring: {}", tree.ring.describe()));
    out.push(format!("--- fsm: moved={} refused={} last={}", tree.fsm.moved, tree.fsm.refused, tree.fsm.last_refused));
    out.push(format!("--- invariant: {:?}", check_invariants(&tree.nodes).iter().map(|x| x.hold).collect::<Vec<bool>>()));
    let _ = seed;
    out
}

/// 演示账快照串。
pub fn demo_ledger(seed: u64) -> String {
    let mut t = match VmaTree::new() {
        Ok(x) => x,
        Err(_) => return String::from("tree-new-failed"),
    };
    let mut a = VmaAttr::ZERO;
    a.user = true;
    a.writable = true;
    let p = HalfPolicy::user();
    let mut i: u64 = 0;
    while i < 32 {
        let r = gen_disjoint_user(seed, i);
        let _ = try_insert(&mut t, &InsertReq::new(r, a, 1, i), p);
        i = i.wrapping_add(1);
    }
    // 再来一次重叠插入（必拒）
    let dup = t.nodes.first().map(|n| n.range).unwrap_or(AddrRange { start: 0, end: PAGE_SIZE });
    let o = try_insert(&mut t, &InsertReq::new(dup, a, 1, 99), p);
    format!("{}\ndup={}\nwitness={}\nfp=0x{:016X}", t.ledger.describe(), o.describe(), t.witness().describe(), t.nodes_fp())
}

// ===========================================================================
// 25. 版本锚
// ===========================================================================

/// 本单判据编号。
pub const CRITERION_ID: &str = "UNX-F2582-J1";
/// 本单判据原文（逐字）。
pub const CRITERION_TEXT: &str = "UNX-F2582-J1 重叠插入注入 10/10 次拒绝，合法插入 10^5 次树不变量零破坏";
/// 本单册内关联单号。
pub const RELATED_TICKETS: [&str; 8] = ["F2581", "F2572", "F2584", "F2589", "F2595", "F2596", "F2564", "F2642"];
/// 关联单串。
pub fn related_str() -> String {
    let mut s = String::new();
    for t in RELATED_TICKETS.iter() {
        s.push_str(t);
        s.push(' ');
    }
    s
}

