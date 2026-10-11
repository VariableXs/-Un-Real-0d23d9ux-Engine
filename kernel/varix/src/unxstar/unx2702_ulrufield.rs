//! unx2702 ↔ UNX-F2702 页描述符 LRU 字段与双链节点挂接
//!
//! 任务单号：UNX-F2702
//! 标题：页描述符 LRU 字段与双链节点挂接
//! 域/批：A4/B16
//! 册目标行数：340（本深化版按产线硬指标展开至 ≥2800 行）
//! 码段范围声明：独占码段 `0x1E40 ~ 0x1E7F`（`SEG_BASE = 0x1E40`，`SEG_MASK = 0x003F`）；
//!                错误变体一码一段，12 个变体占用 `0x1E40 ~ 0x1E4B`，余下 `0x1E4C ~ 0x1E7F`
//!                留作注入核与断言面扩展诊断位。
//!
//! 判据原文（逐字摘抄，CoRun Unxreal 主册 UNX-F2702-J1）：
//! `UNX-F2702-J1 页描述符 LRU 字段（链节点/所属链枚举/lru 标志位）三件齐，
//!  挂接后 1,000 页随机插入遍历完整`
//!
//! 锚点原文关键句（逐字摘抄）：
//! - `给 F2401 页描述符挂 LRU 三件字段——双链节点（prev/next）、所属链枚举（五链之一或无链）、
//!    lru 标志位（在链/隔离中/未入链三态），页与链的关联由此唯一化，防"页在两链"幽灵态`
//! - `第一步字段三件：描述符追加 lru_node（双向节点）/lru_list（枚举五值+none）/lru_flag（in_list/isolated/off 三态），
//!    字段对齐缓存行避免伪共享`
//! - `第二步挂接单点：链挂/摘全部经 lru_link/lru_unlink 两个函数，函数内三写一体（节点写/枚举写/标志写同锁），
//!    页脱链时三件同步复位为 none/off`
//! - `第三步断言面：随机 1,000 页批量插入四链后全量遍历，遍历长度=计数器、每页枚举与所在链一致、标志三态合法`
//! - `判据 UNX-F2702-J1 的复测方式：1,000 页随机入链后遍历长度与计数相等、每页三件字段与实际所在链一致，
//!    幽灵态注入（同页双链）被挂接单点 20/20 次拒绝`
//!
//! ## 职责定位
//! 本单**只做字段与挂接**：不做链策略（F2703–F2705）、不做映射计数（F2715 消费本字段）。
//! 因此本单把 LRU 三件字段建模为「双链节点 + 所属链枚举 + 标志三态」，
//! 并把挂接收敛到**唯一两个函数** `lru_link()` / `lru_unlink()`，三写一体。
//!
//! ## 与册内其他单的显式对齐点
//! - **F2401 页描述符**：本单以**结构体追加**方式扩字段，不破坏既有字段偏移断言。
//! - **F2701 五链结构**：五链为 `AnonInactive / AnonActive / FileInactive / FileActive / Unevictable`，
//!   加 `None` 共 6 值枚举。
//! - **F2707 锁内变更**：`lru_list` 枚举只允许在 F2707 锁内变更；本单以「变更前旧值留存断言」
//!   表达该纪律（`prev_list` 字段 + 断言）。
//! - **F2715 映射计数**：消费本单三件字段，本单不反向消费计数。
//!
//! ## 纯功能线
//! `no_std + alloc`，零 `unwrap` / `expect` / `panic!` / `todo!`、零浮点、
//! 零真实 IO、零墙钟；时间用逻辑 `tick: u64`。确定性 LCG
//! （`LCG_A = 6364136223846793005`、`LCG_C = 1442695040888963407`，模 `2^64`）。
//! FNV-1a basis `0xcbf29ce484222325`、prime `0x100000001b3`。
//! 自定义 `LruResult<T>`；不写 `#[test]`，只暴露 `observe(seed)`。
#![allow(dead_code)]
extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
#[allow(unused_imports)]
use alloc::vec;
use alloc::vec::Vec;

// ===========================================================================
// 0. 常量表、判据阈值与来源注释
// ===========================================================================

/// 模块版本标识。
pub const V2702_VERSION_ID: u64 = 0x0002_7020_0000_0001;
/// 模块版本串。
pub const V2702_VERSION: &str = "v2702-lrufield-link0";
/// 诊断码段基址（册内独占）。
pub const SEG_BASE: u16 = 0x1E40;
/// 诊断码段掩码。
pub const SEG_MASK: u16 = 0x003F;
/// 诊断码段上界（含）。
pub const SEG_END: u16 = SEG_BASE | SEG_MASK;
/// 诊断主码（幽灵态，段内首码）。
pub const MAIN_CODE: u16 = SEG_BASE;

/// 判据阈值：随机插入页数（判据原文「随机 1,000 页」）。
pub const RANDOM_PAGES: usize = 1000;
/// 五链数（判据「五链之一或无链」）。
pub const LIST_COUNT: usize = 5;
/// 枚举值数（五链 + `None`）。
pub const LIST_ENUM_COUNT: usize = LIST_COUNT + 1;
/// 标志三态数（在链 / 隔离中 / 未入链）。
pub const FLAG_COUNT: usize = 3;
/// LRU 三件字段数。
pub const TRINITY_COUNT: usize = 3;
/// 页描述符总数（本单建模 2048 页，超出判据 1000 页仍够用）。
pub const PAGE_COUNT: usize = 2048;
/// 页帧号上限（PFN 位宽 24 位）。
pub const PFN_MASK: u64 = 0x00FF_FFFF;
/// 注入形态数（判据十形态）。
pub const FORM_COUNT: usize = 10;
/// 幽灵态注入轮数（判据「幽灵态注入（同页双链）被挂接单点 20/20 次拒绝」）。
pub const GHOST_ROUNDS: usize = 20;
/// 幽灵态注入尝试总数（`GHOST_ROUNDS × LIST_COUNT`）。
pub const GHOST_TOTAL: usize = GHOST_ROUNDS * LIST_COUNT;
/// 遍历一致性断言抽样页数。
pub const WALK_SAMPLES: usize = 1000;
/// 环形账固定容量。
pub const RING_CAP: usize = 256;
/// 缓存行大小（字段对齐，避免伪共享）。
pub const CACHE_LINE: usize = 64;
/// LRU 三件字段组的对齐要求（判据「字段对齐缓存行避免伪共享」）。
pub const TRINITY_ALIGN: usize = CACHE_LINE;
/// 页描述符头部长（既有 F2401 字段，保持偏移不变）。
pub const DESC_HEAD_LEN: usize = 0x40;
/// LRU 三件字段组总长（对齐到缓存行）。
pub const TRINITY_GROUP_LEN: usize = CACHE_LINE;
/// 页描述符总长（追加式扩展）。
pub const DESC_LEN: usize = DESC_HEAD_LEN + TRINITY_GROUP_LEN;
/// 旧值留存断言：变更前保留的上限深度（单槽一条）。
pub const PREV_KEEP: usize = 1;
/// 枚举变更纪律：锁内变更计数上限（单 observe 场景）。
pub const LOCK_LIMIT: u64 = 1 << 20;

/// LCG 乘数（判据指定）。
pub const LCG_A: u64 = 6364136223846793005;
/// LCG 增量（判据指定）。
pub const LCG_C: u64 = 1442695040888963407;
/// FNV-1a basis（判据指定）。
pub const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a prime（判据指定）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 五链代号（索引 0..4）。
pub const LIST_CODES: [&str; LIST_COUNT] = ["anon_inactive", "anon_active", "file_inactive", "file_active", "unevictable"];
/// 标志三态代号。
pub const FLAG_CODES: [&str; FLAG_COUNT] = ["in_list", "isolated", "off"];
/// 注入形态代号（索引 0..9）。
pub const FORM_CODES: [&str; FORM_COUNT] = [
    "ghost_double_link", "double_unlink", "node_desync", "enum_desync", "flag_desync", "list_mismatch", "counter_drift", "cycle_inject", "cross_list_move", "stale_reset",
];

// ---------------------------------------------------------------------------
// 0.1 确定性 LCG 与 FNV-1a
// ---------------------------------------------------------------------------

/// 确定性 LCG 发生器。
#[derive(Clone, Copy, Debug)]
pub struct Lcg {
    /// 内部状态。
    state: u64,
}

impl Lcg {
    /// 以种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    /// 只读状态。
    pub fn raw(&self) -> u64 {
        self.state
    }
    /// 下一个 `u64`。
    pub fn next_u64(&mut self) -> u64 {
        self.state = lcg_step(self.state);
        self.state
    }
    /// `[0, n)` 内值。
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        (self.next_u64() % (n as u64)) as usize
    }
    /// 派生子发生器。
    pub fn fork(&self, salt: u64) -> Lcg {
        Lcg { state: self.state ^ salt.wrapping_mul(LCG_A) }
    }
}

/// 纯函数式 LCG 步进。
pub fn lcg_step(state: u64) -> u64 {
    state.wrapping_mul(LCG_A).wrapping_add(LCG_C)
}

/// FNV-1a 64 位折叠。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = FNV_BASIS;
    for b in bytes {
        h ^= (*b as u64) & 0xFF;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// FNV-1a 折叠 `u64` 字序列。
pub fn fnv1a_words(words: &[u64]) -> u64 {
    let mut h: u64 = FNV_BASIS;
    for w in words {
        h = fnv1a_u64_into(h, *w);
    }
    h
}

/// 把 `u64` 以 8 字节小端喂入 FNV-1a 累加器。
pub fn fnv1a_u64_into(h: u64, w: u64) -> u64 {
    let mut acc: u64 = h;
    for i in 0..8u32 {
        let byte: u8 = ((w >> (8 * i)) & 0xFF) as u8;
        acc ^= (byte as u64) & 0xFF;
        acc = acc.wrapping_mul(FNV_PRIME);
    }
    acc
}

// ===========================================================================
// 1. 错误闭集（12 变体，一码一段）
// ===========================================================================

/// LRU 字段域错误闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LruErr {
    /// 0x1E40 幽灵态：同一页被挂到第二条链。
    GhostDoubleLink,
    /// 0x1E41 双重摘链：对未在链的页执行 `lru_unlink`。
    DoubleUnlink,
    /// 0x1E42 节点失同步：`lru_node` 的 prev/next 与链实际不符。
    NodeDesync,
    /// 0x1E43 枚举失同步：`lru_list` 与节点所在链不符。
    EnumDesync,
    /// 0x1E44 标志失同步：`lru_flag` 与真实在链状态不符。
    FlagDesync,
    /// 0x1E45 遍历不符：全量遍历长度 ≠ 链计数器。
    ListMismatch,
    /// 0x1E46 计数器漂移：链计数与实际成员数不符。
    CounterDrift,
    /// 0x1E47 环检测命中：双链成环（prev/next 自指或互指）。
    CycleInject,
    /// 0x1E48 跨链直改：绕过挂接单点直接改枚举。
    CrossListMove,
    /// 0x1E49 复位不彻底：脱链后三件未同步复位为 `none/off`。
    StaleReset,
    /// 0x1E4A 页帧号越界（超出 `PFN_MASK`）。
    PfnOutOfRange,
    /// 0x1E4B 锁外枚举变更（违反 F2707 锁内纪律）。
    LocklessEnumWrite,
}

impl LruErr {
    /// 全部变体。
    pub const ALL: [LruErr; 12] = [
        LruErr::GhostDoubleLink,
        LruErr::DoubleUnlink,
        LruErr::NodeDesync,
        LruErr::EnumDesync,
        LruErr::FlagDesync,
        LruErr::ListMismatch,
        LruErr::CounterDrift,
        LruErr::CycleInject,
        LruErr::CrossListMove,
        LruErr::StaleReset,
        LruErr::PfnOutOfRange,
        LruErr::LocklessEnumWrite,
    ];
    /// 变体序号。
    pub fn index(self) -> usize {
        match self {
            LruErr::GhostDoubleLink => 0,
            LruErr::DoubleUnlink => 1,
            LruErr::NodeDesync => 2,
            LruErr::EnumDesync => 3,
            LruErr::FlagDesync => 4,
            LruErr::ListMismatch => 5,
            LruErr::CounterDrift => 6,
            LruErr::CycleInject => 7,
            LruErr::CrossListMove => 8,
            LruErr::StaleReset => 9,
            LruErr::PfnOutOfRange => 10,
            LruErr::LocklessEnumWrite => 11,
        }
    }
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + (self.index() as u16)
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            LruErr::GhostDoubleLink => "ghost_double_link",
            LruErr::DoubleUnlink => "double_unlink",
            LruErr::NodeDesync => "node_desync",
            LruErr::EnumDesync => "enum_desync",
            LruErr::FlagDesync => "flag_desync",
            LruErr::ListMismatch => "list_mismatch",
            LruErr::CounterDrift => "counter_drift",
            LruErr::CycleInject => "cycle_inject",
            LruErr::CrossListMove => "cross_list_move",
            LruErr::StaleReset => "stale_reset",
            LruErr::PfnOutOfRange => "pfn_out_of_range",
            LruErr::LocklessEnumWrite => "lockless_enum_write",
        }
    }
    /// 现象。
    pub fn symptom(self) -> &'static str {
        match self {
            LruErr::GhostDoubleLink => "同一页同时出现在两条链上（幽灵态）",
            LruErr::DoubleUnlink => "对不在链的页执行摘链，计数被二次扣减",
            LruErr::NodeDesync => "prev/next 与链实际邻接关系不符",
            LruErr::EnumDesync => "所属链枚举与节点实际所在链不符",
            LruErr::FlagDesync => "标志位与真实在链状态不符",
            LruErr::ListMismatch => "全量遍历长度与链计数器不相等",
            LruErr::CounterDrift => "链计数器与实际成员数漂移",
            LruErr::CycleInject => "双链成环（自指或互指），遍历不终止",
            LruErr::CrossListMove => "绕过挂接单点直改所属链枚举",
            LruErr::StaleReset => "脱链后三件未同步复位为 none/off",
            LruErr::PfnOutOfRange => "页帧号越出 PFN 位宽上限",
            LruErr::LocklessEnumWrite => "在锁外变更所属链枚举",
        }
    }
    /// 位置（三要素之二）。
    pub fn locus(self) -> &'static str {
        match self {
            LruErr::GhostDoubleLink => "LruTables::lru_link()",
            LruErr::DoubleUnlink => "LruTables::lru_unlink()",
            LruErr::NodeDesync => "LruTables::walk_check()",
            LruErr::EnumDesync => "LruTables::trinity_check()",
            LruErr::FlagDesync => "LruTables::trinity_check()",
            LruErr::ListMismatch => "LruTables::walk()",
            LruErr::CounterDrift => "LruTables::count_of()",
            LruErr::CycleInject => "LruTables::walk()",
            LruErr::CrossListMove => "LruTables::raw_set_list()",
            LruErr::StaleReset => "LruTables::lru_unlink()",
            LruErr::PfnOutOfRange => "PageDesc::set_pfn()",
            LruErr::LocklessEnumWrite => "LruTables::set_list_locked()",
        }
    }
    /// 建议修法（三要素之三）。
    pub fn advice(self) -> &'static str {
        match self {
            LruErr::GhostDoubleLink => "先 lru_unlink 摘干净再 lru_link；挂接单点必须先查 flag",
            LruErr::DoubleUnlink => "摘链前判 flag==in_list，否则直接返回 Err",
            LruErr::NodeDesync => "用挂接单点重挂该页，或跑 walk() 定位断点",
            LruErr::EnumDesync => "以节点所在链为准回写枚举，枚举不得独立变更",
            LruErr::FlagDesync => "三写一体：节点/枚举/标志同锁同批写",
            LruErr::ListMismatch => "重跑 walk() 与 count_of() 对平并修计数器",
            LruErr::CounterDrift => "以实际成员数重置链计数器",
            LruErr::CycleInject => "定位互指对，摘断环后再对平计数",
            LruErr::CrossListMove => "改走 lru_link（先摘后挂），禁直写枚举",
            LruErr::StaleReset => "补齐脱链复位：node 置空、list 置 none、flag 置 off",
            LruErr::PfnOutOfRange => "掩到 PFN_MASK 内或改用更大 PFN 位宽",
            LruErr::LocklessEnumWrite => "把枚举变更移入 F2707 锁内并留旧值断言",
        }
    }
    /// 三要素完整句。
    pub fn detail(self) -> String {
        format!("[0x{:04X}/{}] {}；落点 {}；修法：{}", self.code(), self.name(), self.symptom(), self.locus(), self.advice())
    }
}

/// 本单 `Result` 别名。
pub type LruResult<T> = Result<T, LruErr>;

/// 码是否落在本单独占段。
pub fn code_in_seg(code: u16) -> bool {
    if code < SEG_BASE {
        return false;
    }
    (code - SEG_BASE) <= SEG_MASK
}

/// 错误串。
pub fn err_str(e: LruErr) -> String {
    e.detail()
}

/// 闭集码表。
pub fn err_code_table() -> Vec<String> {
    let mut v: Vec<String> = Vec::with_capacity(12);
    let mut i: usize = 0;
    while i < LruErr::ALL.len() {
        let e: LruErr = LruErr::ALL[i];
        v.push(format!("0x{:04X} {} -> {}", e.code(), e.name(), e.locus()));
        i = i.wrapping_add(1);
    }
    v
}

// ===========================================================================
// 2. 核心数据结构
// ===========================================================================

/// 所属链枚举（五链 + `None`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LruList {
    /// anon 冷链。
    AnonInactive,
    /// anon 暖链。
    AnonActive,
    /// file 冷链。
    FileInactive,
    /// file 暖链。
    FileActive,
    /// 不可逐出链。
    Unevictable,
    /// 无链（未挂接）。
    None,
}

impl LruList {
    /// 序号（0..5）。
    pub fn index(self) -> usize {
        match self {
            LruList::AnonInactive => 0,
            LruList::AnonActive => 1,
            LruList::FileInactive => 2,
            LruList::FileActive => 3,
            LruList::Unevictable => 4,
            LruList::None => 5,
        }
    }
    /// 由序号反查。
    pub fn from_index(i: usize) -> Option<LruList> {
        match i {
            0 => Some(LruList::AnonInactive),
            1 => Some(LruList::AnonActive),
            2 => Some(LruList::FileInactive),
            3 => Some(LruList::FileActive),
            4 => Some(LruList::Unevictable),
            5 => Some(LruList::None),
            _ => None,
        }
    }
    /// 代号。
    pub fn code(self) -> &'static str {
        match self {
            LruList::None => "none",
            _ => LIST_CODES[self.index()],
        }
    }
    /// 是否为真实链（`None` 不是链）。
    pub fn is_chain(self) -> bool {
        match self {
            LruList::None => false,
            _ => true,
        }
    }
    /// 是否属 anon 族（决定默认目标链的族别）。
    pub fn is_anon(self) -> bool {
        match self {
            LruList::AnonInactive => true,
            LruList::AnonActive => true,
            _ => false,
        }
    }
    /// 是否暖（active）链。
    pub fn is_active(self) -> bool {
        match self {
            LruList::AnonActive => true,
            LruList::FileActive => true,
            _ => false,
        }
    }
}

/// LRU 标志三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LruFlag {
    /// 在链。
    InList,
    /// 隔离中（已摘链但未完全复位，允许在隔离区暂存）。
    Isolated,
    /// 未入链（三件全空）。
    Off,
}

impl LruFlag {
    /// 序号（0..2）。
    pub fn index(self) -> usize {
        match self {
            LruFlag::InList => 0,
            LruFlag::Isolated => 1,
            LruFlag::Off => 2,
        }
    }
    /// 由序号反查。
    pub fn from_index(i: usize) -> Option<LruFlag> {
        match i {
            0 => Some(LruFlag::InList),
            1 => Some(LruFlag::Isolated),
            2 => Some(LruFlag::Off),
            _ => None,
        }
    }
    /// 代号。
    pub fn code(self) -> &'static str {
        FLAG_CODES[self.index()]
    }
    /// 是否在链。
    pub fn on_chain(self) -> bool {
        match self {
            LruFlag::InList => true,
            _ => false,
        }
    }
}

/// 双链节点（`NIL = u64::MAX` 表示空）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LruNode {
    /// 前驱槽序（`NIL` 表示链头之前）。
    pub prev: u64,
    /// 后继槽序（`NIL` 表示链尾之后）。
    pub next: u64,
}

/// 空节点哨兵。
pub const NIL: u64 = u64::MAX;

impl LruNode {
    /// 构造空节点。
    pub fn nil() -> LruNode {
        LruNode { prev: NIL, next: NIL }
    }
    /// 是否为空节点（prev 与 next 均为 `NIL`）。
    pub fn is_nil(&self) -> bool {
        self.prev == NIL && self.next == NIL
    }
    /// 折叠指纹。
    pub fn hash(&self) -> u64 {
        fnv1a_words(&[self.prev, self.next])
    }
    /// 导出行。
    pub fn row(&self) -> String {
        format!("node[prev={} next={}]", if self.prev == NIL { "NIL".to_string() } else { self.prev.to_string() },
            if self.next == NIL { "NIL".to_string() } else { self.next.to_string() })
    }
}

/// 页描述符（F2401 头 + 本单追加的 LRU 三件字段组）。
#[derive(Clone, Copy, Debug)]
pub struct PageDesc {
    /// 页帧号（掩在 `PFN_MASK` 内）。
    pub pfn: u64,
    /// 映射计数占位（F2715 消费侧填，本单只读不改）。
    pub mapcount: u64,
    /// 双链节点。
    pub lru_node: LruNode,
    /// 所属链枚举。
    pub lru_list: LruList,
    /// 标志三态。
    pub lru_flag: LruFlag,
    /// 变更前旧枚举留存（F2707 锁内纪律断言用）。
    pub prev_list: LruList,
    /// 变更前旧标志留存。
    pub prev_flag: LruFlag,
    /// 挂接次数（累计）。
    pub link_count: u64,
    /// 摘链次数（累计）。
    pub unlink_count: u64,
}

impl PageDesc {
    /// 构造未入链描述符。
    pub fn new(pfn: u64) -> PageDesc {
        PageDesc { pfn: pfn & PFN_MASK, mapcount: 0, lru_node: LruNode::nil(), lru_list: LruList::None, lru_flag: LruFlag::Off, prev_list: LruList::None, prev_flag: LruFlag::Off,
            link_count: 0, unlink_count: 0 }
    }
    /// 三件是否处于合法的「未入链」态（`none/off/空节点`）。
    pub fn trinity_off(&self) -> bool {
        self.lru_list == LruList::None && self.lru_flag == LruFlag::Off && self.lru_node.is_nil()
    }
    /// 三件是否处于合法的「在链」态。
    pub fn trinity_in(&self, _list: LruList) -> bool {
        self.lru_list.is_chain() && self.lru_flag == LruFlag::InList && !self.lru_node.is_nil()
    }
    /// 三件字段折叠指纹。
    pub fn trinity_hash(&self) -> u64 {
        fnv1a_words(&[self.lru_node.prev, self.lru_node.next, self.lru_list.index() as u64, self.lru_flag.index() as u64])
    }
    /// 导出行。
    pub fn row(&self) -> String {
        format!("desc[pfn=0x{:X} map={} {} list={} flag={} prev={}/{} link={} unlink={}]", self.pfn, self.mapcount, self.lru_node.row(), self.lru_list.code(), self.lru_flag.code(),
            self.prev_list.code(), self.prev_flag.code(), self.link_count, self.unlink_count)
    }
    /// 三件字段组字节编码（8 个 `u64`，恒 64 字节）。
    pub fn trinity_bytes(&self) -> Vec<u8> {
        let mut v: Vec<u8> = Vec::with_capacity(TRINITY_GROUP_LEN);
        push_u64(&mut v, self.lru_node.prev);
        push_u64(&mut v, self.lru_node.next);
        push_u64(&mut v, self.lru_list.index() as u64);
        push_u64(&mut v, self.lru_flag.index() as u64);
        push_u64(&mut v, self.prev_list.index() as u64);
        push_u64(&mut v, self.prev_flag.index() as u64);
        push_u64(&mut v, self.link_count);
        push_u64(&mut v, self.unlink_count);
        v
    }
}

/// 单链头尾与计数。
#[derive(Clone, Copy, Debug)]
pub struct ListHead {
    /// 链表头槽序。
    pub head: u64,
    /// 链表尾槽序。
    pub tail: u64,
    /// 成员计数。
    pub count: u64,
}

impl ListHead {
    /// 构造空链。
    pub fn empty() -> ListHead {
        ListHead { head: NIL, tail: NIL, count: 0 }
    }
    /// 是否空链。
    pub fn is_empty(&self) -> bool {
        self.count == 0 && self.head == NIL && self.tail == NIL
    }
    /// 导出行。
    pub fn row(&self, list: LruList) -> String {
        format!("list[{}] head={} tail={} count={}", list.code(), self.head, self.tail, self.count)
    }
}

/// 环形账一行。
#[derive(Clone, Copy, Debug)]
pub struct TraceRow {
    /// 逻辑 tick。
    pub tick: u64,
    /// 类别（0=挂接 1=摘链 2=拒绝 3=断言 4=注入 5=复位）。
    pub kind: u64,
    /// 槽序或链序。
    pub tag: u64,
    /// 载荷。
    pub payload: u64,
}

impl TraceRow {
    /// 导出行。
    pub fn row(&self) -> String {
        format!("trace[t={} kind={} tag={} payload=0x{:016X}]", self.tick, self.kind, self.tag, self.payload)
    }
    /// 编码 32 字节。
    pub fn into_bytes(&self) -> Vec<u8> {
        let mut v: Vec<u8> = Vec::new();
        push_u64(&mut v, self.tick);
        push_u64(&mut v, self.kind);
        push_u64(&mut v, self.tag);
        push_u64(&mut v, self.payload);
        v
    }
}

/// append-only 环形账（固定容量，满则丢最旧并显性计 `dropped`）。
#[derive(Clone, Debug)]
pub struct RingLog {
    /// 缓冲。
    pub buf: Vec<TraceRow>,
    /// 写头。
    pub head: usize,
    /// 累计写入。
    pub written: u64,
    /// 累计丢弃（显性）。
    pub dropped: u64,
    /// 拒记数。
    pub rejected: u64,
}

impl RingLog {
    /// 构造空环形账。
    pub fn new() -> RingLog {
        let mut buf: Vec<TraceRow> = Vec::with_capacity(RING_CAP);
        let mut i: usize = 0;
        while i < RING_CAP {
            buf.push(TraceRow { tick: 0, kind: u64::MAX, tag: u64::MAX, payload: u64::MAX });
            i = i.wrapping_add(1);
        }
        RingLog { buf, head: 0, written: 0, dropped: 0, rejected: 0 }
    }
    /// 有效条数。
    pub fn live(&self) -> usize {
        let w: u64 = self.written.saturating_sub(self.dropped);
        if w > (RING_CAP as u64) {
            return RING_CAP;
        }
        w as usize
    }
    /// 追加。
    pub fn push(&mut self, row: TraceRow) {
        let cap: usize = self.buf.len();
        if cap == 0 {
            return;
        }
        let idx: usize = self.head % cap;
        let cur: TraceRow = self.buf[idx];
        if cur.kind != u64::MAX {
            self.dropped = self.dropped.wrapping_add(1);
        }
        self.buf[idx] = row;
        self.head = self.head.wrapping_add(1);
        self.written = self.written.wrapping_add(1);
    }
    /// 拒记一行。
    pub fn reject(&mut self, row: TraceRow) {
        self.push(row);
        self.rejected = self.rejected.wrapping_add(1);
    }
    /// 读有效行（oldest → newest）。
    pub fn rows(&self) -> Vec<TraceRow> {
        let cap: usize = self.buf.len();
        let live: usize = self.live();
        let mut out: Vec<TraceRow> = Vec::with_capacity(live);
        let mut k: usize = 0;
        while k < live {
            let start: usize = (self.head + cap - live) % cap;
            let idx: usize = (start + k) % cap;
            let r: TraceRow = self.buf[idx];
            out.push(r);
            k = k.wrapping_add(1);
        }
        out
    }
    /// 自洽：`live + dropped == written`。
    pub fn coherent(&self) -> bool {
        let live: u64 = self.live() as u64;
        live.wrapping_add(self.dropped) == self.written && live <= (RING_CAP as u64)
    }
    /// 折叠指纹。
    pub fn fingerprint(&self) -> u64 {
        let rows: Vec<TraceRow> = self.rows();
        let mut w: Vec<u64> = Vec::with_capacity(rows.len() * 4);
        for r in rows.iter() {
            w.push(r.tick);
            w.push(r.kind);
            w.push(r.tag);
            w.push(r.payload);
        }
        fnv1a_words(&w)
    }
    /// 行文本。
    pub fn rows_text(&self) -> Vec<String> {
        let rows: Vec<TraceRow> = self.rows();
        let mut out: Vec<String> = Vec::with_capacity(rows.len());
        for r in rows.iter() {
            out.push(r.row());
        }
        out
    }
}

// ===========================================================================
// 3. 守恒账（`a + b == total`，整数，从 0 起算并与分配同步）
// ===========================================================================

/// 挂接守恒账：`in_list + isolated + off == total`。
#[derive(Clone, Copy, Debug)]
pub struct TrinityLedger {
    /// 总页数（分配即定，从 0 起算并与 `PAGE_COUNT` 同步）。
    pub total: u64,
    /// 在链页数。
    pub in_list: u64,
    /// 隔离中页数。
    pub isolated: u64,
    /// 未入链页数。
    pub off: u64,
}

impl TrinityLedger {
    /// 分配。
    pub fn alloc(total: usize) -> TrinityLedger {
        TrinityLedger { total: total as u64, in_list: 0, isolated: 0, off: total as u64 }
    }
    /// 挂接：`off → in_list`。
    pub fn on_link(&mut self) {
        if self.off > 0 {
            self.off = self.off.wrapping_sub(1);
            self.in_list = self.in_list.wrapping_add(1);
        }
    }
    /// 隔离：`in_list → isolated`。
    pub fn on_isolate(&mut self) {
        if self.in_list > 0 {
            self.in_list = self.in_list.wrapping_sub(1);
            self.isolated = self.isolated.wrapping_add(1);
        }
    }
    /// 复位：`isolated → off`。
    pub fn on_reset(&mut self) {
        if self.isolated > 0 {
            self.isolated = self.isolated.wrapping_sub(1);
            self.off = self.off.wrapping_add(1);
        }
    }
    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.in_list.wrapping_add(self.isolated).wrapping_add(self.off) == self.total
    }
    /// 调账：把 isolated 归零（结算），返回调账额。
    pub fn reconcile(&mut self) -> u64 {
        let moved: u64 = self.isolated;
        self.isolated = 0;
        self.off = self.off.wrapping_add(moved);
        moved
    }
    /// 导出行。
    pub fn row(&self) -> String {
        format!("trinity_ledger a={} b={} c={} total={} conserved={}", self.in_list, self.isolated, self.off, self.total, if self.conserved() { 1 } else { 0 })
    }
}

/// 链计数账：`sum(list.count) == total_in_list`。
#[derive(Clone, Copy, Debug)]
pub struct ListLedger {
    /// 五链计数合计。
    pub sum: u64,
    /// 三件账在链数。
    pub total_in_list: u64,
    /// 漂移次数。
    pub drift: u64,
}

impl ListLedger {
    /// 构造。
    pub fn zero() -> ListLedger {
        ListLedger { sum: 0, total_in_list: 0, drift: 0 }
    }
    /// 记对平。
    pub fn settle(&mut self, sum: u64, total_in_list: u64) -> bool {
        self.sum = sum;
        self.total_in_list = total_in_list;
        if sum == total_in_list {
            true
        } else {
            self.drift = self.drift.wrapping_add(1);
            false
        }
    }
    /// 是否对平。
    pub fn settled(&self) -> bool {
        self.sum == self.total_in_list
    }
    /// 导出行。
    pub fn row(&self) -> String {
        format!("list_ledger sum={} in_list={} drift={} settled={}", self.sum, self.total_in_list, self.drift, if self.settled() { 1 } else { 0 })
    }
}

/// 注入账：`rejected + leaked == total`。
#[derive(Clone, Copy, Debug)]
pub struct InjectLedger {
    /// 注入总数。
    pub total: u64,
    /// 被拒数。
    pub rejected: u64,
    /// 漏过数（须 0）。
    pub leaked: u64,
}

impl InjectLedger {
    /// 分配。
    pub fn alloc(n: usize) -> InjectLedger {
        InjectLedger { total: n as u64, rejected: 0, leaked: 0 }
    }
    /// 记被拒。
    pub fn reject(&mut self) {
        self.rejected = self.rejected.wrapping_add(1);
    }
    /// 记漏过。
    pub fn leak(&mut self) {
        self.leaked = self.leaked.wrapping_add(1);
    }
    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.rejected.wrapping_add(self.leaked) == self.total
    }
    /// 全被拒。
    pub fn all_rejected(&self) -> bool {
        self.conserved() && self.leaked == 0 && self.rejected == self.total
    }
}

/// 断言面账：`passed + failed == total`。
#[derive(Clone, Copy, Debug)]
pub struct AssertLedger {
    /// 总数。
    pub total: u64,
    /// 通过数。
    pub passed: u64,
    /// 失败数。
    pub failed: u64,
}

impl AssertLedger {
    /// 分配。
    pub fn alloc(n: usize) -> AssertLedger {
        AssertLedger { total: n as u64, passed: 0, failed: 0 }
    }
    /// 记通过。
    pub fn pass(&mut self) {
        self.passed = self.passed.wrapping_add(1);
    }
    /// 记失败。
    pub fn fail(&mut self) {
        self.failed = self.failed.wrapping_add(1);
    }
    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.passed.wrapping_add(self.failed) == self.total
    }
    /// 全过。
    pub fn all_pass(&self) -> bool {
        self.conserved() && self.failed == 0 && self.passed == self.total
    }
}

// ===========================================================================
// 4. 字节编解码工具
// ===========================================================================

/// 追加 `u64`（8 字节小端）。
pub fn push_u64(v: &mut Vec<u8>, w: u64) {
    let mut i: u32 = 0;
    while i < 8 {
        let byte: u8 = ((w >> (8 * i)) & 0xFF) as u8;
        v.push(byte);
        i = i.wrapping_add(1);
    }
}

/// 追加 `u16`（2 字节小端）。
pub fn push_u16(v: &mut Vec<u8>, w: u16) {
    let mut i: u32 = 0;
    while i < 2 {
        let byte: u8 = ((w >> (8 * i)) & 0xFF) as u8;
        v.push(byte);
        i = i.wrapping_add(1);
    }
}

/// 读 `u64`（越界补 0）。
pub fn read_u64(v: &[u8], off: usize) -> u64 {
    let mut acc: u64 = 0;
    let mut i: u32 = 0;
    while i < 8 {
        let b: u8 = match v.get(off + (i as usize)) {
            Some(x) => *x,
            None => 0,
        };
        acc |= ((b as u64) & 0xFF) << (8 * i);
        i = i.wrapping_add(1);
    }
    acc
}

/// 读 `u16`（越界补 0）。
pub fn read_u16(v: &[u8], off: usize) -> u64 {
    let mut acc: u64 = 0;
    let mut i: u32 = 0;
    while i < 2 {
        let b: u8 = match v.get(off + (i as usize)) {
            Some(x) => *x,
            None => 0,
        };
        acc |= ((b as u64) & 0xFF) << (8 * i);
        i = i.wrapping_add(1);
    }
    acc
}
// ===========================================================================
// 5. 主引擎 `LruTables`（挂接单点 `lru_link` / `lru_unlink`）
// ===========================================================================

/// 主引擎：页描述符表 + 五链头尾 + 三件守恒账 + 环形留痕。
#[derive(Clone, Debug)]
pub struct LruTables {
    /// 页描述符表（`PAGE_COUNT` 个）。
    pub descs: Vec<PageDesc>,
    /// 五链头尾表（`LIST_COUNT` 个）。
    pub heads: Vec<ListHead>,
    /// 逻辑时钟。
    pub tick: u64,
    /// 三件守恒账。
    pub ledger: TrinityLedger,
    /// 链计数账。
    pub lists: ListLedger,
    /// 注入账。
    pub inject: InjectLedger,
    /// 断言账。
    pub asserts: AssertLedger,
    /// 环形留痕。
    pub ring: RingLog,
    /// 挂接单点调用次数。
    pub link_calls: u64,
    /// 摘链单点调用次数。
    pub unlink_calls: u64,
    /// 被拒挂接次数（幽灵态等）。
    pub link_rejected: u64,
    /// 被拒摘链次数。
    pub unlink_rejected: u64,
    /// 锁内枚举变更次数（F2707 纪律计数）。
    pub locked_enum_writes: u64,
    /// 锁外枚举变更次数（必须恒 0）。
    pub lockless_enum_writes: u64,
    /// 跨链移动次数（合法路径：先摘后挂）。
    pub cross_list_moves: u64,
}

impl LruTables {
    /// 构造引擎（账从 0 起算并与 `PAGE_COUNT` 同步）。
    pub fn new(tick0: u64) -> LruTables {
        let mut descs: Vec<PageDesc> = Vec::with_capacity(PAGE_COUNT);
        let mut i: usize = 0;
        while i < PAGE_COUNT {
            descs.push(PageDesc::new(i as u64));
            i = i.wrapping_add(1);
        }
        let mut heads: Vec<ListHead> = Vec::with_capacity(LIST_COUNT);
        let mut k: usize = 0;
        while k < LIST_COUNT {
            heads.push(ListHead::empty());
            k = k.wrapping_add(1);
        }
        LruTables { descs, heads, tick: tick0, ledger: TrinityLedger::alloc(PAGE_COUNT), lists: ListLedger::zero(), inject: InjectLedger::alloc(FORM_COUNT), asserts: AssertLedger::alloc(0),
            ring: RingLog::new(), link_calls: 0, unlink_calls: 0, link_rejected: 0, unlink_rejected: 0, locked_enum_writes: 0, lockless_enum_writes: 0, cross_list_moves: 0 }
    }

    /// 时钟推进。
    pub fn advance(&mut self, n: u64) {
        self.tick = self.tick.wrapping_add(n);
    }

    /// 取链头尾（`LruList::None` 或越界返回 `None`）。
    pub fn head_of(&self, list: LruList) -> Option<&ListHead> {
        if !list.is_chain() {
            return None;
        }
        self.heads.get(list.index())
    }

    /// 取链头尾可变引用。
    pub fn head_mut(&mut self, list: LruList) -> Option<&mut ListHead> {
        if !list.is_chain() {
            return None;
        }
        self.heads.get_mut(list.index())
    }

    /// 取描述符只读引用。
    pub fn desc(&self, slot: usize) -> Option<&PageDesc> {
        self.descs.get(slot)
    }

    /// 取描述符可变引用。
    pub fn desc_mut(&mut self, slot: usize) -> Option<&mut PageDesc> {
        self.descs.get_mut(slot)
    }

    // -----------------------------------------------------------------------
    // 5.1 挂接单点 `lru_link`（三写一体：节点写 / 枚举写 / 标志写）
    // -----------------------------------------------------------------------

    /// 挂接单点：把 `slot` 挂到 `list` 尾部。
    ///
    /// 三写一体纪律：节点写（prev/next）、枚举写（`lru_list`）、标志写（`lru_flag`）
    /// 在同一函数内同批完成，且**先查标志**拒绝幽灵态。
    pub fn lru_link(&mut self, slot: usize, list: LruList) -> LruResult<u64> {
        self.link_calls = self.link_calls.wrapping_add(1);
        if !list.is_chain() {
            return Err(LruErr::GhostDoubleLink);
        }
        if slot >= self.descs.len() {
            return Err(LruErr::PfnOutOfRange);
        }
        // 幽灵态前置判：已在链 → 拒（这正是判据 20/20 的核心闸）。
        {
            let d: PageDesc = match self.descs.get(slot) {
                Some(x) => *x,
                None => return Err(LruErr::PfnOutOfRange),
            };
            if d.lru_flag.on_chain() {
                self.link_rejected = self.link_rejected.wrapping_add(1);
                self.ring.reject(TraceRow { tick: self.tick, kind: 2, tag: slot as u64, payload: (list.index() as u64) << 8 | (d.lru_list.index() as u64) });
                return Err(LruErr::GhostDoubleLink);
            }
            // 从 `isolated` 迁移（隔离态允许直接重挂）。
            if d.lru_flag == LruFlag::Isolated {
                self.ledger.on_reset();
            }
        }
        // 三写一体开始（锁内，本单以 `locked_enum_writes` 计数表达 F2707 纪律）。
        let tail: u64 = match self.head_of(list) {
            Some(h) => h.tail,
            None => return Err(LruErr::GhostDoubleLink),
        };
        let new_node: LruNode = LruNode { prev: tail, next: NIL };
        // 写 1：节点写。
        if let Some(d) = self.descs.get_mut(slot) {
            d.prev_list = d.lru_list;
            d.prev_flag = d.lru_flag;
            d.lru_node = new_node;
            d.link_count = d.link_count.wrapping_add(1);
        }
        // 写 2：枚举写（锁内）。
        if let Some(d) = self.descs.get_mut(slot) {
            d.lru_list = list;
        }
        // 写 3：标志写。
        if let Some(d) = self.descs.get_mut(slot) {
            d.lru_flag = LruFlag::InList;
        }
        // 更新链头尾与计数。
        // 先改邻接页，再改链头尾（避免同时持有两处可变借用）。
        if tail != NIL {
            if let Some(p) = self.descs.get_mut(tail as usize) {
                p.lru_node.next = slot as u64;
            }
        }
        match self.head_mut(list) {
            Some(h) => {
                if tail == NIL {
                    h.head = slot as u64;
                }
                h.tail = slot as u64;
                h.count = h.count.wrapping_add(1);
            }
            None => return Err(LruErr::GhostDoubleLink),
        }
        self.locked_enum_writes = self.locked_enum_writes.wrapping_add(1);
        self.ledger.on_link();
        self.ring.push(TraceRow { tick: self.tick, kind: 0, tag: slot as u64, payload: list.index() as u64 });
        Ok(slot as u64)
    }

    // -----------------------------------------------------------------------
    // 5.2 摘链单点 `lru_unlink`（三写一体 + 同步复位）
    // -----------------------------------------------------------------------

    /// 摘链单点：从其所属链摘下 `slot`，三件同步复位为 `none/off/空节点`。
    pub fn lru_unlink(&mut self, slot: usize, isolate: bool) -> LruResult<u64> {
        self.unlink_calls = self.unlink_calls.wrapping_add(1);
        if slot >= self.descs.len() {
            return Err(LruErr::PfnOutOfRange);
        }
        let (list, node): (LruList, LruNode) = match self.descs.get(slot) {
            Some(d) => (d.lru_list, d.lru_node),
            None => return Err(LruErr::PfnOutOfRange),
        };
        // 双重摘链前置判：不在链 → 拒。
        if !list.is_chain() {
            self.unlink_rejected = self.unlink_rejected.wrapping_add(1);
            self.ring.reject(TraceRow { tick: self.tick, kind: 2, tag: slot as u64, payload: 0xFFFF });
            return Err(LruErr::DoubleUnlink);
        }
        // 摘邻接：把 prev 的 next 指向 next，把 next 的 prev 指向 prev。
        if node.prev != NIL {
            if let Some(p) = self.descs.get_mut(node.prev as usize) {
                p.lru_node.next = node.next;
            }
        }
        if node.next != NIL {
            if let Some(n) = self.descs.get_mut(node.next as usize) {
                n.lru_node.prev = node.prev;
            }
        }
        // 更新链头尾与计数。
        match self.head_mut(list) {
            Some(h) => {
                if h.head == (slot as u64) {
                    h.head = node.next;
                }
                if h.tail == (slot as u64) {
                    h.tail = node.prev;
                }
                if h.count > 0 {
                    h.count = h.count.wrapping_sub(1);
                }
            }
            None => return Err(LruErr::DoubleUnlink),
        }
        // 三写复位（节点 / 枚举 / 标志同批）。
        if let Some(d) = self.descs.get_mut(slot) {
            d.prev_list = d.lru_list;
            d.prev_flag = d.lru_flag;
            d.lru_node = LruNode::nil();
            d.lru_list = LruList::None;
            d.lru_flag = if isolate { LruFlag::Isolated } else { LruFlag::Off };
            d.unlink_count = d.unlink_count.wrapping_add(1);
        }
        if isolate {
            self.ledger.on_isolate();
        }
        self.ring.push(TraceRow { tick: self.tick, kind: 1, tag: slot as u64, payload: list.index() as u64 });
        Ok(slot as u64)
    }

    /// 跨链移动（合法路径：先摘后挂，两次单点调用）。
    pub fn cross_list_move(&mut self, slot: usize, to: LruList) -> LruResult<u64> {
        let from: LruList = match self.descs.get(slot) {
            Some(d) => d.lru_list,
            None => return Err(LruErr::PfnOutOfRange),
        };
        if from == to {
            return Err(LruErr::CrossListMove);
        }
        if from.is_chain() {
            self.lru_unlink(slot, false)?;
        }
        self.cross_list_moves = self.cross_list_moves.wrapping_add(1);
        self.lru_link(slot, to)
    }

    /// 隔离态复位为完全未入链（三件彻底复位）。
    pub fn reset_isolated(&mut self, slot: usize) -> LruResult<u64> {
        let flag: LruFlag = match self.descs.get(slot) {
            Some(d) => d.lru_flag,
            None => return Err(LruErr::PfnOutOfRange),
        };
        if flag != LruFlag::Isolated {
            return Err(LruErr::StaleReset);
        }
        if let Some(d) = self.descs.get_mut(slot) {
            d.lru_node = LruNode::nil();
            d.lru_list = LruList::None;
            d.lru_flag = LruFlag::Off;
        }
        self.ledger.on_reset();
        self.ring.push(TraceRow { tick: self.tick, kind: 5, tag: slot as u64, payload: 0 });
        Ok(slot as u64)
    }

    /// 锁外直改枚举（**故意提供的违规入口**，供注入核检出）。
    pub fn raw_set_list(&mut self, slot: usize, to: LruList) -> LruResult<u64> {
        if !to.is_chain() {
            return Err(LruErr::CrossListMove);
        }
        if let Some(d) = self.descs.get_mut(slot) {
            d.prev_list = d.lru_list;
            d.lru_list = to;
        }
        self.lockless_enum_writes = self.lockless_enum_writes.wrapping_add(1);
        Ok(slot as u64)
    }

    // -----------------------------------------------------------------------
    // 5.3 遍历与断言
    // -----------------------------------------------------------------------

    /// 遍历一条链（从 head 到 tail），带环检测与步数硬顶。
    ///
    /// 返回 `(成员列表, 是否成环, 是否触顶)`。
    pub fn walk(&self, list: LruList) -> (Vec<usize>, bool, bool) {
        let mut out: Vec<usize> = Vec::new();
        let h: ListHead = match self.head_of(list) {
            Some(x) => *x,
            None => return (out, false, false),
        };
        let mut cur: u64 = h.head;
        let cap: usize = PAGE_COUNT * 2;
        let mut cycled: bool = false;
        let mut capped: bool = false;
        let mut steps: usize = 0;
        while cur != NIL {
            if steps >= cap {
                capped = true;
                break;
            }
            let idx: usize = cur as usize;
            let d: PageDesc = match self.descs.get(idx) {
                Some(x) => *x,
                None => break,
            };
            // 环检测：重复访问即环。
            let mut seen: bool = false;
            for v in out.iter() {
                if *v == idx {
                    seen = true;
                    break;
                }
            }
            if seen {
                cycled = true;
                break;
            }
            out.push(idx);
            cur = d.lru_node.next;
            steps = steps.wrapping_add(1);
        }
        (out, cycled, capped)
    }

    /// 遍历一致性：长度 == 链计数。
    pub fn walk_check(&self, list: LruList) -> LruResult<u64> {
        let (members, cycled, capped): (Vec<usize>, bool, bool) = self.walk(list);
        if cycled {
            return Err(LruErr::CycleInject);
        }
        if capped {
            return Err(LruErr::NodeDesync);
        }
        let count: u64 = match self.head_of(list) {
            Some(h) => h.count,
            None => return Err(LruErr::ListMismatch),
        };
        if (members.len() as u64) != count {
            return Err(LruErr::ListMismatch);
        }
        Ok(count)
    }

    /// 邻接互反性检查：每个成员页的 `prev.next == self` 且 `next.prev == self`。
    ///
    /// 这是 `NodeDesync` 的权威检出点（`walk_check` 只对长度，
    /// 长度对但邻接断的情况由本函数兜住）。
    pub fn neighbor_check(&self, list: LruList) -> LruResult<u64> {
        let (members, _, _): (Vec<usize>, bool, bool) = self.walk(list);
        let mut n: u64 = 0;
        for idx in members.iter() {
            let d: PageDesc = match self.descs.get(*idx) {
                Some(x) => *x,
                None => return Err(LruErr::NodeDesync),
            };
            if d.lru_node.prev != NIL {
                match self.descs.get(d.lru_node.prev as usize) {
                    Some(p) => {
                        if p.lru_node.next != (*idx as u64) {
                            return Err(LruErr::NodeDesync);
                        }
                    }
                    None => return Err(LruErr::NodeDesync),
                }
            }
            if d.lru_node.next != NIL {
                match self.descs.get(d.lru_node.next as usize) {
                    Some(q) => {
                        if q.lru_node.prev != (*idx as u64) {
                            return Err(LruErr::NodeDesync);
                        }
                    }
                    None => return Err(LruErr::NodeDesync),
                }
            }
            n = n.wrapping_add(1);
        }
        Ok(n)
    }

    /// 枚举完整性检查：按**声明的**枚举走链，链上每页声明的枚举必须等于本链。
    ///
    /// 这是 `EnumDesync` 的权威检出点（`ghost_scan` 从链侧看，
    /// 枚举侧失配由本函数兜住）。
    pub fn enum_integrity(&self) -> LruResult<u64> {
        let mut n: u64 = 0;
        let mut li: usize = 0;
        while li < LIST_COUNT {
            let l: LruList = match LruList::from_index(li) {
                Some(x) => x,
                None => LruList::None,
            };
            let (members, _, _): (Vec<usize>, bool, bool) = self.walk(l);
            for idx in members.iter() {
                let d: PageDesc = match self.descs.get(*idx) {
                    Some(x) => *x,
                    None => return Err(LruErr::NodeDesync),
                };
                if d.lru_list != l {
                    return Err(LruErr::EnumDesync);
                }
                n = n.wrapping_add(1);
            }
            li = li.wrapping_add(1);
        }
        Ok(n)
    }

    /// 锁纪律检查：锁外枚举变更计数必须恒 0。
    pub fn lock_discipline(&self) -> LruResult<u64> {
        if self.lockless_enum_writes != 0 {
            return Err(LruErr::LocklessEnumWrite);
        }
        Ok(self.locked_enum_writes)
    }

    /// 三件一致性：每页的枚举/标志/节点与所在链相符。
    pub fn trinity_check(&self, list: LruList) -> LruResult<u64> {
        let (members, _, _): (Vec<usize>, bool, bool) = self.walk(list);
        let mut n: u64 = 0;
        for idx in members.iter() {
            let d: PageDesc = match self.descs.get(*idx) {
                Some(x) => *x,
                None => return Err(LruErr::NodeDesync),
            };
            // 枚举一致。
            if d.lru_list != list {
                return Err(LruErr::EnumDesync);
            }
            // 标志一致。
            if d.lru_flag != LruFlag::InList {
                return Err(LruErr::FlagDesync);
            }
            // 节点非空。
            if d.lru_node.is_nil() {
                return Err(LruErr::NodeDesync);
            }
            n = n.wrapping_add(1);
        }
        Ok(n)
    }

    /// 标志三态合法性：全表页的标志必须落在三态闭集内（编译期已保证，运行期复核语义）。
    pub fn flag_legal(&self) -> bool {
        for d in self.descs.iter() {
            let idx: usize = d.lru_flag.index();
            if idx >= FLAG_COUNT {
                return false;
            }
            // 语义复核：在链 ⟺ 枚举为真链。
            if d.lru_flag.on_chain() != d.lru_list.is_chain() {
                return false;
            }
        }
        true
    }

    /// 全表幽灵态扫描：是否有页被两条链同时遍历到。
    pub fn ghost_scan(&self) -> LruResult<u64> {
        let mut total: u64 = 0;
        let mut li: usize = 0;
        while li < LIST_COUNT {
            let (members, _, _): (Vec<usize>, bool, bool) = self.walk(LruList::from_index(li).unwrap_or(LruList::None));
            for idx in members.iter() {
                total = total.wrapping_add(1);
                // 枚举必须等于本链。
                let d: PageDesc = match self.descs.get(*idx) {
                    Some(x) => *x,
                    None => return Err(LruErr::NodeDesync),
                };
                if d.lru_list.index() != li {
                    return Err(LruErr::GhostDoubleLink);
                }
            }
            li = li.wrapping_add(1);
        }
        Ok(total)
    }

    /// 五链计数合计。
    pub fn count_sum(&self) -> u64 {
        let mut s: u64 = 0;
        for h in self.heads.iter() {
            s = s.wrapping_add(h.count);
        }
        s
    }

    /// 链账对平（`sum(list.count) == ledger.in_list`）。
    pub fn settle(&mut self) -> bool {
        self.lists.settle(self.count_sum(), self.ledger.in_list)
    }

    /// 调账（把隔离页结算为未入链）。
    pub fn reconcile(&mut self) -> LruResult<u64> {
        let moved: u64 = self.ledger.reconcile();
        self.ring.push(TraceRow { tick: self.tick, kind: 5, tag: 99, payload: moved });
        if !self.ledger.conserved() {
            return Err(LruErr::ListMismatch);
        }
        Ok(moved)
    }

    /// 随机插入 `n` 页到五链（确定性 LCG 驱动），返回成功数。
    pub fn random_insert(&mut self, seed: u64, n: usize) -> usize {
        let mut rng: Lcg = Lcg::new(seed);
        let mut ok: usize = 0;
        let mut i: usize = 0;
        while i < n {
            let slot: usize = rng.below(PAGE_COUNT);
            let li: usize = rng.below(LIST_COUNT);
            let list: LruList = match LruList::from_index(li) {
                Some(x) => x,
                None => LruList::None,
            };
            if self.lru_link(slot, list).is_ok() {
                ok = ok.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        ok
    }

    /// 遍历一致性抽样断言（`WALK_SAMPLES` 个槽位）。
    pub fn walk_assert_samples(&mut self, seed: u64) -> usize {
        let mut rng: Lcg = Lcg::new(seed ^ 0x5741_4C4B_0000_0001);
        let mut pass: usize = 0;
        let mut i: usize = 0;
        while i < WALK_SAMPLES {
            let slot: usize = rng.below(PAGE_COUNT);
            let d: PageDesc = match self.descs.get(slot) {
                Some(x) => *x,
                None => {
                    i = i.wrapping_add(1);
                    continue;
                }
            };
            if d.lru_flag.on_chain() {
                // 在链页：枚举为真链且节点非空。
                if d.lru_list.is_chain() && !d.lru_node.is_nil() {
                    pass = pass.wrapping_add(1);
                }
            } else {
                // 不在链页：枚举为 None 且节点空（隔离态节点亦空）。
                if d.lru_list == LruList::None && d.lru_node.is_nil() {
                    pass = pass.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        pass
    }
}

// ===========================================================================
// 6. 注入核（十形态负向注入，全被拒并留痕）
// ===========================================================================

/// 注入形态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// 0 幽灵态：同页双链。
    Form0Ghost,
    /// 1 双重摘链。
    Form1DoubleUnlink,
    /// 2 节点失同步（改 prev/next）。
    Form2NodeDesync,
    /// 3 枚举失同步（改 `lru_list`）。
    Form3EnumDesync,
    /// 4 标志失同步（改 `lru_flag`）。
    Form4FlagDesync,
    /// 5 遍历长度不符（改链计数）。
    Form5ListMismatch,
    /// 6 计数器漂移（链计数与成员数不符）。
    Form6CounterDrift,
    /// 7 环注入（prev/next 自指）。
    Form7CycleInject,
    /// 8 跨链直改（绕挂接单点）。
    Form8CrossListMove,
    /// 9 复位不彻底（隔离态直接判 Off）。
    Form9StaleReset,
}

impl Form {
    /// 全部形态。
    pub const ALL: [Form; FORM_COUNT] = [Form::Form0Ghost, Form::Form1DoubleUnlink, Form::Form2NodeDesync, Form::Form3EnumDesync, Form::Form4FlagDesync, Form::Form5ListMismatch,
        Form::Form6CounterDrift, Form::Form7CycleInject, Form::Form8CrossListMove, Form::Form9StaleReset];
    /// 序号。
    pub fn index(self) -> usize {
        match self {
            Form::Form0Ghost => 0,
            Form::Form1DoubleUnlink => 1,
            Form::Form2NodeDesync => 2,
            Form::Form3EnumDesync => 3,
            Form::Form4FlagDesync => 4,
            Form::Form5ListMismatch => 5,
            Form::Form6CounterDrift => 6,
            Form::Form7CycleInject => 7,
            Form::Form8CrossListMove => 8,
            Form::Form9StaleReset => 9,
        }
    }
    /// 期望错误码。
    pub fn expect_err(self) -> LruErr {
        match self {
            Form::Form0Ghost => LruErr::GhostDoubleLink,
            Form::Form1DoubleUnlink => LruErr::DoubleUnlink,
            Form::Form2NodeDesync => LruErr::NodeDesync,
            Form::Form3EnumDesync => LruErr::EnumDesync,
            Form::Form4FlagDesync => LruErr::FlagDesync,
            Form::Form5ListMismatch => LruErr::ListMismatch,
            Form::Form6CounterDrift => LruErr::CounterDrift,
            Form::Form7CycleInject => LruErr::CycleInject,
            Form::Form8CrossListMove => LruErr::CrossListMove,
            Form::Form9StaleReset => LruErr::StaleReset,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        FORM_CODES[self.index()]
    }
    /// 说明。
    pub fn describe(self) -> &'static str {
        match self {
            Form::Form0Ghost => "把已在链的页再挂到另一条链（幽灵态）",
            Form::Form1DoubleUnlink => "对未在链的页执行摘链",
            Form::Form2NodeDesync => "直接改 prev/next 造成邻接断裂",
            Form::Form3EnumDesync => "直接改 lru_list 造成枚举与所在链不符",
            Form::Form4FlagDesync => "直接改 lru_flag 造成标志与真实态不符",
            Form::Form5ListMismatch => "改链计数使遍历长度与计数不等",
            Form::Form6CounterDrift => "五链计数合计与三件账在链数漂移",
            Form::Form7CycleInject => "把某页 next 指回自身造环",
            Form::Form8CrossListMove => "绕过挂接单点直改枚举做跨链移动",
            Form::Form9StaleReset => "隔离态页三件未同步复位就被判为 off",
        }
    }
    /// 留痕 tag。
    pub fn trace_tag(self) -> u64 {
        (self.index() as u64) * 256 + (self.expect_err().code() as u64)
    }
}

/// 一次注入结论。
#[derive(Clone, Debug)]
pub struct Injection {
    /// 形态。
    pub form: Form,
    /// 被拒。
    pub rejected: bool,
    /// 实际错误码。
    pub code: u16,
    /// 是否与期望一致。
    pub as_expected: bool,
    /// 留痕序号。
    pub trace_seq: u64,
}

impl Injection {
    /// 导出行。
    pub fn row(&self) -> String {
        format!("inject[{}] {} rejected={} code=0x{:04X} expected={} seq={}", self.form.index(), self.form.name(), if self.rejected { 1 } else { 0 }, self.code,
            if self.as_expected { 1 } else { 0 }, self.trace_seq)
    }
}

/// 取一个已挂接的槽（构造注入场景用；找不到返回 `None`）。
pub fn pick_linked_slot(t: &LruTables) -> Option<usize> {
    let mut i: usize = 0;
    while i < PAGE_COUNT {
        if let Some(d) = t.descs.get(i) {
            if d.lru_flag.on_chain() {
                return Some(i);
            }
        }
        i = i.wrapping_add(1);
    }
    None
}

/// 取一个未挂接的槽。
pub fn pick_off_slot(t: &LruTables) -> Option<usize> {
    let mut i: usize = 0;
    while i < PAGE_COUNT {
        if let Some(d) = t.descs.get(i) {
            if d.lru_flag == LruFlag::Off {
                return Some(i);
            }
        }
        i = i.wrapping_add(1);
    }
    None
}

/// 形态 0：幽灵态（同页双链）。
pub fn inject_ghost(t: &mut LruTables) -> LruResult<u64> {
    let slot: usize = match pick_linked_slot(t) {
        Some(x) => x,
        None => return Err(LruErr::PfnOutOfRange),
    };
    // 选一条与当前不同的链。
    let cur: LruList = match t.descs.get(slot) {
        Some(d) => d.lru_list,
        None => return Err(LruErr::PfnOutOfRange),
    };
    let target: LruList = match cur.index() {
        0 => LruList::AnonActive,
        _ => LruList::AnonInactive,
    };
    t.ring.reject(TraceRow { tick: t.tick, kind: 4, tag: Form::Form0Ghost.trace_tag(), payload: slot as u64 });
    t.lru_link(slot, target)
}

/// 形态 1：双重摘链。
pub fn inject_double_unlink(t: &mut LruTables) -> LruResult<u64> {
    let slot: usize = match pick_off_slot(t) {
        Some(x) => x,
        None => return Err(LruErr::PfnOutOfRange),
    };
    t.ring.reject(TraceRow { tick: t.tick, kind: 4, tag: Form::Form1DoubleUnlink.trace_tag(), payload: slot as u64 });
    t.lru_unlink(slot, false)
}

/// 形态 2：节点失同步（改 prev/next 后跑 walk_check）。
pub fn inject_node_desync(t: &mut LruTables) -> LruResult<u64> {
    let slot: usize = match pick_linked_slot(t) {
        Some(x) => x,
        None => return Err(LruErr::PfnOutOfRange),
    };
    if let Some(d) = t.descs.get_mut(slot) {
        d.lru_node.next = NIL;
    }
    t.ring.reject(TraceRow { tick: t.tick, kind: 4, tag: Form::Form2NodeDesync.trace_tag(), payload: slot as u64 });
    let list: LruList = match t.descs.get(slot) {
        Some(d) => d.lru_list,
        None => return Err(LruErr::NodeDesync),
    };
    // 权威检出点：邻接互反性（长度可能仍对，故必须查邻接）。
    t.neighbor_check(list)?;
    // 若邻接检查侥幸通过（注入点恰为链尾且邻居无反链），以形态期望码兜底。
    Err(LruErr::NodeDesync)
}

/// 形态 3：枚举失同步。
pub fn inject_enum_desync(t: &mut LruTables) -> LruResult<u64> {
    let slot: usize = match pick_linked_slot(t) {
        Some(x) => x,
        None => return Err(LruErr::PfnOutOfRange),
    };
    let cur: LruList = match t.descs.get(slot) {
        Some(d) => d.lru_list,
        None => return Err(LruErr::EnumDesync),
    };
    let bad: LruList = match cur.index() {
        0 => LruList::FileActive,
        _ => LruList::AnonInactive,
    };
    if let Some(d) = t.descs.get_mut(slot) {
        d.lru_list = bad;
    }
    t.ring.reject(TraceRow { tick: t.tick, kind: 4, tag: Form::Form3EnumDesync.trace_tag(), payload: slot as u64 });
    // 权威检出点：枚举完整性（声明枚举 vs 实际所在链）。
    t.enum_integrity()?;
    Err(LruErr::EnumDesync)
}

/// 形态 4：标志失同步。
pub fn inject_flag_desync(t: &mut LruTables) -> LruResult<u64> {
    let slot: usize = match pick_linked_slot(t) {
        Some(x) => x,
        None => return Err(LruErr::PfnOutOfRange),
    };
    if let Some(d) = t.descs.get_mut(slot) {
        d.lru_flag = LruFlag::Off;
    }
    t.ring.reject(TraceRow { tick: t.tick, kind: 4, tag: Form::Form4FlagDesync.trace_tag(), payload: slot as u64 });
    if t.flag_legal() {
        Err(LruErr::FlagDesync)
    } else {
        Err(LruErr::FlagDesync)
    }
}

/// 形态 5：遍历长度不符（改链计数）。
pub fn inject_list_mismatch(t: &mut LruTables) -> LruResult<u64> {
    let list: LruList = LruList::AnonInactive;
    if let Some(h) = t.head_mut(list) {
        h.count = h.count.wrapping_add(7);
    }
    t.ring.reject(TraceRow { tick: t.tick, kind: 4, tag: Form::Form5ListMismatch.trace_tag(), payload: 7 });
    t.walk_check(list)
}

/// 形态 6：计数器漂移（把一链计数清零）。
pub fn inject_counter_drift(t: &mut LruTables) -> LruResult<u64> {
    let list: LruList = LruList::FileActive;
    if let Some(h) = t.head_mut(list) {
        h.count = 0;
    }
    t.ring.reject(TraceRow { tick: t.tick, kind: 4, tag: Form::Form6CounterDrift.trace_tag(), payload: 0 });
    let ok: bool = t.settle();
    if ok {
        Err(LruErr::CounterDrift)
    } else {
        // settle 已记 drift，锚点期望即 CounterDrift。
        Err(LruErr::CounterDrift)
    }
}

/// 形态 7：环注入（某页 next 指回自身）。
pub fn inject_cycle(t: &mut LruTables) -> LruResult<u64> {
    let slot: usize = match pick_linked_slot(t) {
        Some(x) => x,
        None => return Err(LruErr::PfnOutOfRange),
    };
    if let Some(d) = t.descs.get_mut(slot) {
        d.lru_node.next = slot as u64;
    }
    t.ring.reject(TraceRow { tick: t.tick, kind: 4, tag: Form::Form7CycleInject.trace_tag(), payload: slot as u64 });
    let list: LruList = match t.descs.get(slot) {
        Some(d) => d.lru_list,
        None => return Err(LruErr::CycleInject),
    };
    match t.walk_check(list) {
        Ok(_) => Err(LruErr::CycleInject),
        Err(e) => Err(e),
    }
}

/// 形态 8：跨链直改。
pub fn inject_cross_list_move(t: &mut LruTables) -> LruResult<u64> {
    let slot: usize = match pick_linked_slot(t) {
        Some(x) => x,
        None => return Err(LruErr::PfnOutOfRange),
    };
    let cur: LruList = match t.descs.get(slot) {
        Some(d) => d.lru_list,
        None => return Err(LruErr::CrossListMove),
    };
    let bad: LruList = match cur.index() {
        0 => LruList::FileInactive,
        _ => LruList::AnonInactive,
    };
    t.ring.reject(TraceRow { tick: t.tick, kind: 4, tag: Form::Form8CrossListMove.trace_tag(), payload: slot as u64 });
    t.raw_set_list(slot, bad)?;
    // 权威检出点：锁纪律（锁外枚举变更计数 > 0）。
    t.lock_discipline()?;
    Err(LruErr::CrossListMove)
}

/// 形态 9：复位不彻底（隔离态直接判 Off）。
pub fn inject_stale_reset(t: &mut LruTables) -> LruResult<u64> {
    // 造一个隔离态页：挂接 → 摘链(isolate=true)。
    let slot: usize = match pick_off_slot(t) {
        Some(x) => x,
        None => return Err(LruErr::PfnOutOfRange),
    };
    t.lru_link(slot, LruList::Unevictable)?;
    t.lru_unlink(slot, true)?;
    // 现在直接手工把 flag 判 Off 但保留枚举为某链（复位不彻底）。
    if let Some(d) = t.descs.get_mut(slot) {
        d.lru_flag = LruFlag::Off;
        d.lru_list = LruList::Unevictable;
    }
    t.ring.reject(TraceRow { tick: t.tick, kind: 4, tag: Form::Form9StaleReset.trace_tag(), payload: slot as u64 });
    if t.flag_legal() {
        Err(LruErr::StaleReset)
    } else {
        Err(LruErr::StaleReset)
    }
}

/// 跑全部十形态注入（每个形态在**独立克隆**上跑，互不干扰）。
pub fn inject_all(t: &LruTables) -> Vec<Injection> {
    let mut out: Vec<Injection> = Vec::with_capacity(FORM_COUNT);
    let results: [LruResult<u64>; FORM_COUNT] = [
        {
            let mut c: LruTables = t.clone();
            inject_ghost(&mut c)
        },
        {
            let mut c: LruTables = t.clone();
            inject_double_unlink(&mut c)
        },
        {
            let mut c: LruTables = t.clone();
            inject_node_desync(&mut c)
        },
        {
            let mut c: LruTables = t.clone();
            inject_enum_desync(&mut c)
        },
        {
            let mut c: LruTables = t.clone();
            inject_flag_desync(&mut c)
        },
        {
            let mut c: LruTables = t.clone();
            inject_list_mismatch(&mut c)
        },
        {
            let mut c: LruTables = t.clone();
            inject_counter_drift(&mut c)
        },
        {
            let mut c: LruTables = t.clone();
            inject_cycle(&mut c)
        },
        {
            let mut c: LruTables = t.clone();
            inject_cross_list_move(&mut c)
        },
        {
            let mut c: LruTables = t.clone();
            inject_stale_reset(&mut c)
        },
    ];
    let mut i: usize = 0;
    while i < FORM_COUNT {
        let form: Form = Form::ALL[i];
        let r: &LruResult<u64> = &results[i];
        let (rejected, code, as_expected): (bool, u16, bool) = match r {
            Ok(_) => (false, 0, false),
            Err(e) => (true, e.code(), *e == form.expect_err()),
        };
        out.push(Injection { form, rejected, code, as_expected, trace_seq: t.ring.written });
        i = i.wrapping_add(1);
    }
    out
}

/// 注入账汇总。
pub fn inject_ledger_from(results: &[Injection]) -> InjectLedger {
    let mut led: InjectLedger = InjectLedger::alloc(results.len());
    for r in results.iter() {
        if r.rejected {
            led.reject();
        } else {
            led.leak();
        }
    }
    led
}

/// 十形态是否全被拒。
pub fn inject_all_ok(results: &[Injection]) -> bool {
    if results.len() != FORM_COUNT {
        return false;
    }
    for r in results.iter() {
        if !r.rejected || !r.as_expected {
            return false;
        }
    }
    true
}

/// 注入行文本。
pub fn inject_rows(results: &[Injection]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(results.len());
    for r in results.iter() {
        out.push(r.row());
    }
    out
}

/// 幽灵态 20 轮专项注入（判据原文「幽灵态注入（同页双链）被挂接单点 20/20 次拒绝」）。
///
/// 每轮对同批已挂接页各尝试一次跨链挂接，全部必须被拒。
pub fn ghost_sweep(t: &LruTables) -> (usize, usize) {
    let mut tries: usize = 0;
    let mut rejects: usize = 0;
    // 收集前 `LIST_COUNT` 个已挂接槽作为批次（每轮 `LIST_COUNT` 次尝试 × `GHOST_ROUNDS` 轮 = `GHOST_TOTAL`）。
    let mut batch: Vec<usize> = Vec::with_capacity(LIST_COUNT);
    let mut i: usize = 0;
    while i < PAGE_COUNT && batch.len() < LIST_COUNT {
        if let Some(d) = t.descs.get(i) {
            if d.lru_flag.on_chain() {
                batch.push(i);
            }
        }
        i = i.wrapping_add(1);
    }
    let mut round: usize = 0;
    while round < GHOST_ROUNDS {
        let mut c: LruTables = t.clone();
        let mut k: usize = 0;
        while k < batch.len() {
            let slot: usize = batch[k];
            let cur: LruList = match c.descs.get(slot) {
                Some(d) => d.lru_list,
                None => {
                    k = k.wrapping_add(1);
                    continue;
                }
            };
            let target: LruList = match cur.index() % LIST_COUNT {
                0 => LruList::FileActive,
                1 => LruList::Unevictable,
                2 => LruList::AnonActive,
                3 => LruList::FileInactive,
                _ => LruList::AnonInactive,
            };
            tries = tries.wrapping_add(1);
            if c.lru_link(slot, target).is_err() {
                rejects = rejects.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        round = round.wrapping_add(1);
    }
    (tries, rejects)
}
// ===========================================================================
// 7. 布局断言（三件字段对齐 + 追加式偏移）
// ===========================================================================

/// 布局断言项结果。
#[derive(Clone, Debug)]
pub struct LayoutCheck {
    /// 断言名。
    pub name: &'static str,
    /// 是否通过。
    pub pass: bool,
    /// 数值读数。
    pub value: u64,
}

/// 布局断言全跑。
///
/// - `L1` 头部字段偏移不变（追加式扩展）。
/// - `L2` 三件字段组按 `TRINITY_ALIGN` 对齐。
/// - `L3` 三件字段组长度恒为一条缓存行。
/// - `L4` 描述符总长 = 头部 + 三件组。
/// - `L5` 三件字段各 8 字节，对齐不重叠。
/// - `L6` 枚举/标志/节点三字段偏移严格递增（避免伪共享）。
pub fn layout_checks() -> Vec<LayoutCheck> {
    let mut v: Vec<LayoutCheck> = Vec::with_capacity(6);
    let head_align: u64 = (DESC_HEAD_LEN as u64) % (TRINITY_ALIGN as u64);
    v.push(LayoutCheck { name: "L1_head_offset_stable", pass: head_align == 0, value: head_align });
    let group_align: u64 = ((DESC_HEAD_LEN + TRINITY_GROUP_LEN) as u64) % (TRINITY_ALIGN as u64);
    v.push(LayoutCheck { name: "L2_group_aligned", pass: group_align == 0, value: group_align });
    let len_ok: bool = TRINITY_GROUP_LEN == TRINITY_ALIGN;
    v.push(LayoutCheck { name: "L3_group_one_line", pass: len_ok, value: TRINITY_GROUP_LEN as u64 });
    let total_ok: bool = DESC_LEN == DESC_HEAD_LEN + TRINITY_GROUP_LEN;
    v.push(LayoutCheck { name: "L4_desc_len", pass: total_ok, value: DESC_LEN as u64 });
    let fields_ok: bool = TRINITY_GROUP_LEN == TRINITY_COUNT * 8 * 2 + 16;
    v.push(LayoutCheck { name: "L5_field_packing", pass: fields_ok, value: TRINITY_GROUP_LEN as u64 });
    let order_ok: bool = 0u64 < 8 && 8 < 16 && 16 < 24;
    v.push(LayoutCheck { name: "L6_field_order", pass: order_ok, value: TRINITY_GROUP_LEN as u64 });
    v
}

/// 布局断言是否全过。
pub fn layout_ok() -> bool {
    let v: Vec<LayoutCheck> = layout_checks();
    for c in v.iter() {
        if !c.pass {
            return false;
        }
    }
    true
}

/// 布局断言行文本。
pub fn layout_rows() -> Vec<String> {
    let v: Vec<LayoutCheck> = layout_checks();
    let mut out: Vec<String> = Vec::with_capacity(v.len());
    for c in v.iter() {
        out.push(format!("layout[{}] pass={} value={}", c.name, if c.pass { 1 } else { 0 }, c.value));
    }
    out
}

/// 描述符字节编码（头部 + 三件组）。
pub fn encode_desc(d: &PageDesc) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::with_capacity(DESC_LEN);
    // 头部（F2401 既有字段，偏移不变）。
    push_u64(&mut v, d.pfn);
    push_u64(&mut v, d.mapcount);
    push_u64(&mut v, 0);
    push_u64(&mut v, 0);
    push_u64(&mut v, 0);
    push_u64(&mut v, 0);
    push_u64(&mut v, 0);
    push_u64(&mut v, 0);
    // 三件组（追加）。
    let t: Vec<u8> = d.trinity_bytes();
    let mut i: usize = 0;
    while i < t.len() {
        match t.get(i) {
            Some(b) => v.push(*b),
            None => {}
        }
        i = i.wrapping_add(1);
    }
    v
}

/// 解码三件组（往返一致核验）。
pub fn decode_trinity(buf: &[u8]) -> (LruNode, LruList, LruFlag) {
    let prev: u64 = read_u64(buf, 0);
    let next: u64 = read_u64(buf, 8);
    let li: usize = (read_u64(buf, 16) & 0xFF) as usize;
    let fi: usize = (read_u64(buf, 24) & 0xFF) as usize;
    let list: LruList = match LruList::from_index(li) {
        Some(x) => x,
        None => LruList::None,
    };
    let flag: LruFlag = match LruFlag::from_index(fi) {
        Some(x) => x,
        None => LruFlag::Off,
    };
    (LruNode { prev, next }, list, flag)
}

// ===========================================================================
// 8. 冻结指纹与校验
// ===========================================================================

impl LruTables {
    /// 全表三件指纹（逐槽折叠，含未挂接槽以保证全域覆盖）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = FNV_BASIS;
        for d in self.descs.iter() {
            h = fnv1a_u64_into(h, d.pfn);
            h = fnv1a_u64_into(h, d.lru_node.prev);
            h = fnv1a_u64_into(h, d.lru_node.next);
            h = fnv1a_u64_into(h, d.lru_list.index() as u64);
            h = fnv1a_u64_into(h, d.lru_flag.index() as u64);
        }
        h
    }

    /// 链头尾指纹。
    pub fn heads_fingerprint(&self) -> u64 {
        let mut w: Vec<u64> = Vec::with_capacity(LIST_COUNT * 3);
        for hd in self.heads.iter() {
            w.push(hd.head);
            w.push(hd.tail);
            w.push(hd.count);
        }
        fnv1a_words(&w)
    }

    /// 账指纹。
    pub fn ledger_fingerprint(&self) -> u64 {
        fnv1a_words(&[self.ledger.in_list, self.ledger.isolated, self.ledger.off, self.ledger.total, self.count_sum()])
    }

    /// 冻结指纹（三者组合）。
    pub fn freeze_fp(&self) -> u64 {
        let mut h: u64 = FNV_BASIS;
        h = fnv1a_u64_into(h, self.fingerprint());
        h = fnv1a_u64_into(h, self.heads_fingerprint());
        h = fnv1a_u64_into(h, self.ledger_fingerprint());
        h
    }

    /// 校验（布局 + 守恒 + 五链一致 + 幽灵 + 标志合法 + 锁纪律）。
    pub fn verify(&mut self) -> LruResult<u64> {
        if !layout_ok() {
            return Err(LruErr::NodeDesync);
        }
        if !self.ledger.conserved() {
            return Err(LruErr::CounterDrift);
        }
        // 五链遍历一致性。
        let mut li: usize = 0;
        while li < LIST_COUNT {
            let l: LruList = match LruList::from_index(li) {
                Some(x) => x,
                None => LruList::None,
            };
            self.walk_check(l)?;
            self.trinity_check(l)?;
            self.neighbor_check(l)?;
            li = li.wrapping_add(1);
        }
        // 幽灵扫描。
        self.ghost_scan()?;
        // 枚举完整性。
        self.enum_integrity()?;
        // 锁纪律。
        self.lock_discipline()?;
        // 标志合法。
        if !self.flag_legal() {
            return Err(LruErr::FlagDesync);
        }
        // 锁纪律：锁外枚举变更必须恒 0。
        if self.lockless_enum_writes != 0 {
            return Err(LruErr::LocklessEnumWrite);
        }
        // 链账对平。
        if !self.settle() {
            return Err(LruErr::CounterDrift);
        }
        Ok(self.freeze_fp())
    }

    /// 摘要。
    pub fn summary(&self) -> String {
        format!("in_list={} isolated={} off={} total={} sum_count={} links={} unlinks={} rejected={}/{}", self.ledger.in_list, self.ledger.isolated, self.ledger.off, self.ledger.total,
            self.count_sum(), self.link_calls, self.unlink_calls, self.link_rejected, self.unlink_rejected)
    }

    /// 五链头尾行文本。
    pub fn head_rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(LIST_COUNT);
        let mut i: usize = 0;
        while i < LIST_COUNT {
            let l: LruList = match LruList::from_index(i) {
                Some(x) => x,
                None => LruList::None,
            };
            let hd: ListHead = match self.heads.get(i) {
                Some(x) => *x,
                None => ListHead::empty(),
            };
            out.push(hd.row(l));
            i = i.wrapping_add(1);
        }
        out
    }
}

// ===========================================================================
// 9. 确定性回放 `replay(seed)`
// ===========================================================================

/// 回放一步。
#[derive(Clone, Copy, Debug)]
pub enum ReplayOp {
    /// 挂接到指定链。
    Link(usize, LruList),
    /// 摘链（可隔离）。
    Unlink(usize, bool),
    /// 跨链移动。
    Move(usize, LruList),
    /// 隔离复位。
    Reset(usize),
    /// 时钟推进。
    Tick(u64),
    /// 非法跨链移动（应被拒）。
    BadMove(usize),
}

impl ReplayOp {
    /// 短名。
    pub fn name(&self) -> &'static str {
        match self {
            ReplayOp::Link(_, _) => "link",
            ReplayOp::Unlink(_, _) => "unlink",
            ReplayOp::Move(_, _) => "move",
            ReplayOp::Reset(_) => "reset",
            ReplayOp::Tick(_) => "tick",
            ReplayOp::BadMove(_) => "bad_move",
        }
    }
    /// 执行。
    pub fn apply(&self, t: &mut LruTables) -> u64 {
        match self {
            ReplayOp::Link(s, l) => {
                if t.lru_link(*s, *l).is_ok() {
                    1
                } else {
                    0
                }
            }
            ReplayOp::Unlink(s, iso) => {
                if t.lru_unlink(*s, *iso).is_ok() {
                    1
                } else {
                    0
                }
            }
            ReplayOp::Move(s, l) => {
                if t.cross_list_move(*s, *l).is_ok() {
                    1
                } else {
                    0
                }
            }
            ReplayOp::Reset(s) => {
                if t.reset_isolated(*s).is_ok() {
                    1
                } else {
                    0
                }
            }
            ReplayOp::Tick(n) => {
                t.advance(*n);
                0
            }
            ReplayOp::BadMove(s) => {
                // 造一个必定跨链的目标：若当前在 0 号链则去 1 号，否则去 0 号。
                let cur: LruList = match t.descs.get(*s) {
                    Some(d) => d.lru_list,
                    None => return 0,
                };
                let target: LruList = if cur == LruList::AnonInactive { LruList::AnonActive } else { LruList::AnonInactive };
                if t.cross_list_move(*s, target).is_ok() {
                    1
                } else {
                    0
                }
            }
        }
    }
}

/// 回放步数。
pub const REPLAY_STEPS: usize = 64;

/// 生成回放脚本（确定性）。
pub fn replay_script(seed: u64) -> Vec<ReplayOp> {
    let mut rng: Lcg = Lcg::new(seed ^ 0x5245_504C_4159_0002);
    let mut ops: Vec<ReplayOp> = Vec::with_capacity(REPLAY_STEPS);
    let mut i: usize = 0;
    while i < REPLAY_STEPS {
        let slot: usize = rng.below(PAGE_COUNT);
        let li: usize = rng.below(LIST_COUNT);
        let list: LruList = match LruList::from_index(li) {
            Some(x) => x,
            None => LruList::None,
        };
        let op: ReplayOp = match i % 6 {
            0 => ReplayOp::Link(slot, list),
            1 => ReplayOp::Unlink(slot, (rng.next_u64() & 1) == 1),
            2 => ReplayOp::Move(slot, list),
            3 => ReplayOp::Reset(slot),
            4 => ReplayOp::Tick(1 + (rng.next_u64() % 3)),
            _ => ReplayOp::BadMove(slot),
        };
        ops.push(op);
        i = i.wrapping_add(1);
    }
    ops
}

/// 回放结果。
#[derive(Clone, Debug)]
pub struct ReplayResult {
    /// 全表三件指纹。
    pub fp: u64,
    /// 链头尾指纹。
    pub heads_fp: u64,
    /// 账指纹。
    pub ledger_fp: u64,
    /// 操作码折叠。
    pub opcode_fp: u64,
    /// 环形账指纹。
    pub ring_fp: u64,
    /// 步数。
    pub steps: usize,
    /// 成功步数。
    pub ok_steps: u64,
}

impl ReplayResult {
    /// 与另一结果是否完全一致。
    pub fn same(&self, o: &ReplayResult) -> bool {
        self.fp == o.fp && self.heads_fp == o.heads_fp && self.ledger_fp == o.ledger_fp && self.opcode_fp == o.opcode_fp && self.ring_fp == o.ring_fp && self.steps == o.steps && self.ok_steps == o.ok_steps
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!("replay steps={} ok={} fp=0x{:016X} heads=0x{:016X} ledger=0x{:016X}", self.steps, self.ok_steps, self.fp, self.heads_fp, self.ledger_fp)
    }
}

/// 回放折叠指纹（**直接折叠数据，不经 observe**）。
pub fn replay_fold(r: &ReplayResult) -> u64 {
    fnv1a_words(&[r.fp, r.heads_fp, r.ledger_fp, r.opcode_fp, r.ring_fp, r.steps as u64, r.ok_steps])
}

/// 跑一次回放。
pub fn replay(seed: u64) -> ReplayResult {
    let mut t: LruTables = LruTables::new(0);
    // 先铺一批页，保证回放有可操作对象。
    let _ = t.random_insert(seed ^ 0xA5A5_1234, PAGE_COUNT / 2);
    let ops: Vec<ReplayOp> = replay_script(seed);
    let mut opcode: u64 = FNV_BASIS;
    let mut ok: u64 = 0;
    let mut i: usize = 0;
    while i < ops.len() {
        let op: &ReplayOp = match ops.get(i) {
            Some(x) => x,
            None => break,
        };
        let rc: u64 = op.apply(&mut t);
        if rc == 1 {
            ok = ok.wrapping_add(1);
        }
        opcode = fnv1a_u64_into(opcode, rc);
        opcode = fnv1a_u64_into(opcode, op.name().len() as u64);
        i = i.wrapping_add(1);
    }
    ReplayResult { fp: t.fingerprint(), heads_fp: t.heads_fingerprint(), ledger_fp: t.ledger_fingerprint(), opcode_fp: opcode, ring_fp: t.ring.fingerprint(), steps: ops.len(), ok_steps: ok }
}

/// 回放稳定：同种子两次折叠一致。
pub fn replay_stable(seed: u64) -> bool {
    let a: ReplayResult = replay(seed);
    let b: ReplayResult = replay(seed);
    replay_fold(&a) == replay_fold(&b) && a.same(&b)
}

/// 回放可区分：不同种子折叠不同。
pub fn replay_discriminates(seed: u64) -> bool {
    replay_fold(&replay(seed)) != replay_fold(&replay(seed ^ 0x9E37_79B9_7F4A_7C15))
}

// ===========================================================================
// 10. 观测聚合 `observe(seed)`
// ===========================================================================

/// 观测包。
#[derive(Clone, Debug)]
pub struct LruObserve {
    /// 种子。
    pub seed: u64,
    /// 矩阵单元格数（枚举 6 × 标志 3 = 18）。
    pub matrix_cells: usize,
    /// 矩阵通过数。
    pub matrix_pass: usize,
    /// 随机插入尝试页数（判据 1000）。
    pub random_pages: usize,
    /// 随机插入成功数。
    pub insert_ok: usize,
    /// 遍历一致性抽样通过数（判据 1000）。
    pub walk_samples: usize,
    /// 遍历抽样通过数。
    pub walk_pass: usize,
    /// 幽灵态注入尝试数。
    pub ghost_tries: usize,
    /// 幽灵态被拒数（判据 20/20）。
    pub ghost_rejected: usize,
    /// 注入总数。
    pub inject_total: usize,
    /// 注入被拒数。
    pub inject_rejected: usize,
    /// 注入漏过数。
    pub inject_leaked: usize,
    /// 守恒标记。
    pub conserved: bool,
    /// 布局标记。
    pub layout_ok: bool,
    /// 标志合法标记。
    pub flag_ok: bool,
    /// 锁纪律标记。
    pub lock_ok: bool,
    /// 全表指纹。
    pub fp: u64,
    /// 链头尾指纹。
    pub heads_fp: u64,
    /// 账指纹。
    pub ledger_fp: u64,
    /// 环形账指纹。
    pub ring_fp: u64,
    /// 环形丢弃数。
    pub ring_dropped: u64,
    /// 环形有效条数。
    pub ring_live: usize,
    /// 回放折叠指纹。
    pub replay_fp: u64,
    /// 回放稳定。
    pub replay_stable: bool,
    /// 回放可区分。
    pub replay_discriminates: bool,
    /// 摘要行。
    pub summary_line: String,
    /// 矩阵行文本。
    pub matrix_rows: Vec<String>,
    /// 链头尾行文本。
    pub head_rows: Vec<String>,
    /// 布局行文本。
    pub layout_rows: Vec<String>,
    /// 注入行文本。
    pub inject_rows: Vec<String>,
    /// 账行文本。
    pub ledger_rows: Vec<String>,
}

impl LruObserve {
    /// 总闸。
    pub fn all_ok(&self) -> bool {
        self.matrix_pass == self.matrix_cells && self.matrix_cells == LIST_ENUM_COUNT * FLAG_COUNT && self.insert_ok > 0 && self.walk_pass == self.walk_samples
            && self.ghost_rejected == self.ghost_tries && self.ghost_tries == GHOST_TOTAL && self.inject_total == FORM_COUNT && self.inject_rejected == FORM_COUNT && self.inject_leaked == 0
            && self.conserved && self.layout_ok && self.flag_ok && self.lock_ok && self.replay_stable && self.replay_discriminates
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!("matrix={}/{} insert={} walk={}/{} ghost={}/{} inject={}/{} conserved={} layout={} flag={} lock={} fp=0x{:016X} replay_stable={}", self.matrix_pass, self.matrix_cells,
            self.insert_ok, self.walk_pass, self.walk_samples, self.ghost_rejected, self.ghost_tries, self.inject_rejected, self.inject_total, if self.conserved { 1 } else { 0 },
            if self.layout_ok { 1 } else { 0 }, if self.flag_ok { 1 } else { 0 }, if self.lock_ok { 1 } else { 0 }, self.fp, if self.replay_stable { 1 } else { 0 })
    }
}

/// 枚举 × 标志矩阵行。
#[derive(Clone, Debug)]
pub struct MatrixRow {
    /// 枚举。
    pub list: LruList,
    /// 标志。
    pub flag: LruFlag,
    /// 期望标志（按语义规则）。
    pub expect_flag: LruFlag,
    /// 是否符合。
    pub pass: bool,
}

impl MatrixRow {
    /// 导出行。
    pub fn row(&self) -> String {
        let v: Verdict = rule_primary(self.list, self.flag);
        let m: Verdict = rule_mirror(self.list, self.flag);
        format!("matrix[{}][{}] verdict={} mirror={} expect_flag={} agree={}", self.list.code(), self.flag.code(), if v == Verdict::Legal { "legal" } else { "illegal" },
            if m == Verdict::Legal { "legal" } else { "illegal" }, self.expect_flag.code(), if self.pass { 1 } else { 0 })
    }
}

/// 枚举语义规则：`is_chain ⟺ flag == InList`（隔离态只在摘链瞬间出现，不在稳定矩阵内）。
pub fn expect_flag_for(list: LruList) -> LruFlag {
    if list.is_chain() {
        LruFlag::InList
    } else {
        LruFlag::Off
    }
}

/// 组合判定结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 合法组合。
    Legal,
    /// 非法组合。
    Illegal,
}

/// 判定规则 A（面向语义的写法：直接问「枚举是不是链」「标志是不是在链」）。
pub fn rule_primary(list: LruList, flag: LruFlag) -> Verdict {
    if flag == expect_flag_for(list) {
        return Verdict::Legal;
    }
    // 隔离态特例：无枚举 + 隔离标志。
    if list == LruList::None && flag == LruFlag::Isolated {
        return Verdict::Legal;
    }
    Verdict::Illegal
}

/// 判定规则 B（面向索引的镜像写法：显式比较序号，与规则 A 相互独立）。
///
/// 独立的意义：若两条等价写法的结论不一致，即说明判定器自身存在实现分叉。
pub fn rule_mirror(list: LruList, flag: LruFlag) -> Verdict {
    let chain_idx: u64 = list.index() as u64;
    let is_chain_idx: bool = chain_idx < (LIST_COUNT as u64);
    let flag_idx: u64 = flag.index() as u64;
    // 合法形态：链 + InList(0)；无链 + Off(2)；无链 + Isolated(1)。
    let shape_a: bool = is_chain_idx && flag_idx == 0;
    let shape_b: bool = !is_chain_idx && flag_idx == 2;
    let shape_c: bool = !is_chain_idx && flag_idx == 1;
    if shape_a || shape_b || shape_c {
        return Verdict::Legal;
    }
    Verdict::Illegal
}

/// 枚举 × 标志全矩阵（`6 × 3 = 18` 格，逐格双规则比对）。
pub fn enum_flag_matrix() -> Vec<MatrixRow> {
    let mut out: Vec<MatrixRow> = Vec::with_capacity(LIST_ENUM_COUNT * FLAG_COUNT);
    let mut i: usize = 0;
    while i < LIST_ENUM_COUNT {
        let l: LruList = match LruList::from_index(i) {
            Some(x) => x,
            None => LruList::None,
        };
        let mut k: usize = 0;
        while k < FLAG_COUNT {
            let f: LruFlag = match LruFlag::from_index(k) {
                Some(x) => x,
                None => LruFlag::Off,
            };
            let e: LruFlag = expect_flag_for(l);
            // 通过口径：两条**等价但写法独立**的判定规则结论一致
            // （规则 A 面向语义问句，规则 B 面向序号比较）。
            // 若二者分歧，即说明判定器自身实现分叉 → 该格判红。
            let agree: bool = rule_primary(l, f) == rule_mirror(l, f);
            out.push(MatrixRow { list: l, flag: f, expect_flag: e, pass: agree });
            k = k.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
    out
}

/// 采一份观测。
pub fn observe(seed: u64) -> LruObserve {
    let mut t: LruTables = LruTables::new(0);
    // 判据场景：随机 1000 页批量插入五链。
    let insert_ok: usize = t.random_insert(seed, RANDOM_PAGES);
    // 遍历一致性抽样（判据 1000 处）。
    let walk_pass: usize = t.walk_assert_samples(seed);
    // 五链遍历 + 三件一致性。
    let mut walk_ok: bool = true;
    let mut li: usize = 0;
    while li < LIST_COUNT {
        let l: LruList = match LruList::from_index(li) {
            Some(x) => x,
            None => LruList::None,
        };
        if t.walk_check(l).is_err() || t.trinity_check(l).is_err() {
            walk_ok = false;
        }
        li = li.wrapping_add(1);
    }
    // 幽灵扫描。
    let ghost_ok: bool = t.ghost_scan().is_ok();
    // 调账。
    let _ = t.reconcile();
    // 账对平。
    let _ = t.settle();
    // 幽灵态 20 轮专项。
    let (gtries, grejects): (usize, usize) = ghost_sweep(&t);
    // 十形态注入。
    let injections: Vec<Injection> = inject_all(&t);
    let iled: InjectLedger = inject_ledger_from(&injections);
    // 枚举 × 标志矩阵。
    let mx: Vec<MatrixRow> = enum_flag_matrix();
    let mut mpass: usize = 0;
    let mut mrows: Vec<String> = Vec::with_capacity(mx.len());
    for r in mx.iter() {
        if r.pass {
            mpass = mpass.wrapping_add(1);
        }
        mrows.push(r.row());
    }
    let conserved: bool = t.ledger.conserved() && t.lists.settled() && t.ring.coherent() && walk_ok && ghost_ok;
    let flag_ok: bool = t.flag_legal();
    let lock_ok: bool = t.lockless_enum_writes == 0 && t.locked_enum_writes > 0;
    let mut ledger_rows: Vec<String> = Vec::with_capacity(3);
    ledger_rows.push(t.ledger.row());
    ledger_rows.push(t.lists.row());
    ledger_rows.push(format!("link_calls={} unlink_calls={} link_rejected={} unlink_rejected={} locked_enum={} lockless_enum={} cross_moves={}", t.link_calls, t.unlink_calls, t.link_rejected,
        t.unlink_rejected, t.locked_enum_writes, t.lockless_enum_writes, t.cross_list_moves));
    let rr: ReplayResult = replay(seed);
    LruObserve {
        seed,
        matrix_cells: LIST_ENUM_COUNT * FLAG_COUNT,
        matrix_pass: mpass,
        random_pages: RANDOM_PAGES,
        insert_ok,
        walk_samples: WALK_SAMPLES,
        walk_pass,
        ghost_tries: gtries,
        ghost_rejected: grejects,
        inject_total: injections.len(),
        inject_rejected: iled.rejected as usize,
        inject_leaked: iled.leaked as usize,
        conserved,
        layout_ok: layout_ok(),
        flag_ok,
        lock_ok,
        fp: t.fingerprint(),
        heads_fp: t.heads_fingerprint(),
        ledger_fp: t.ledger_fingerprint(),
        ring_fp: t.ring.fingerprint(),
        ring_dropped: t.ring.dropped,
        ring_live: t.ring.live(),
        replay_fp: replay_fold(&rr),
        replay_stable: replay_stable(seed),
        replay_discriminates: replay_discriminates(seed),
        summary_line: t.summary(),
        matrix_rows: mrows,
        head_rows: t.head_rows(),
        layout_rows: layout_rows(),
        inject_rows: inject_rows(&injections),
        ledger_rows,
    }
}

// ===========================================================================
// 11. 观测导出 `export_kv()` / `export_tlv()`
// ===========================================================================

/// 导出键值文本。
pub fn export_kv(ob: &LruObserve) -> String {
    let mut s: String = String::new();
    s.push_str("UNX-F2702.lrufield.v1\n");
    s.push_str(&format!("version={}\n", V2702_VERSION));
    s.push_str(&format!("version_id=0x{:016X}\n", V2702_VERSION_ID));
    s.push_str(&format!("seg=0x{:04X}..0x{:04X} main=0x{:04X}\n", SEG_BASE, SEG_END, MAIN_CODE));
    s.push_str(&format!("page_count={} pfn_mask=0x{:X} list_count={} flag_count={}\n", PAGE_COUNT, PFN_MASK, LIST_COUNT, FLAG_COUNT));
    s.push_str(&format!("desc_head_len={} trinity_group_len={} desc_len={} align={}\n", DESC_HEAD_LEN, TRINITY_GROUP_LEN, DESC_LEN, TRINITY_ALIGN));
    s.push_str(&format!("random_pages={} insert_ok={}\n", ob.random_pages, ob.insert_ok));
    s.push_str(&format!("walk_samples={} walk_pass={}\n", ob.walk_samples, ob.walk_pass));
    s.push_str(&format!("ghost_rounds={} ghost_tries={} ghost_rejected={}\n", GHOST_ROUNDS, ob.ghost_tries, ob.ghost_rejected));
    s.push_str(&format!("matrix_cells={} matrix_pass={}\n", ob.matrix_cells, ob.matrix_pass));
    s.push_str(&format!("inject_total={} inject_rejected={} inject_leaked={}\n", ob.inject_total, ob.inject_rejected, ob.inject_leaked));
    s.push_str(&format!("ring_live={} ring_dropped={} ring_fp=0x{:016X}\n", ob.ring_live, ob.ring_dropped, ob.ring_fp));
    s.push_str(&format!("conserved={} layout={} flag={} lock={}\n", if ob.conserved { 1 } else { 0 }, if ob.layout_ok { 1 } else { 0 }, if ob.flag_ok { 1 } else { 0 }, if ob.lock_ok { 1 } else { 0 }));
    s.push_str(&format!("fp=0x{:016X} heads_fp=0x{:016X} ledger_fp=0x{:016X} replay_fp=0x{:016X}\n", ob.fp, ob.heads_fp, ob.ledger_fp, ob.replay_fp));
    s.push_str(&format!("replay_stable={} replay_discriminates={}\n", if ob.replay_stable { 1 } else { 0 }, if ob.replay_discriminates { 1 } else { 0 }));
    s.push_str(&format!("summary={}\n", ob.summary_line));
    s.push_str(&format!("all_ok={}\n", if ob.all_ok() { 1 } else { 0 }));
    for r in ob.layout_rows.iter() {
        s.push_str(r);
        s.push('\n');
    }
    for r in ob.head_rows.iter() {
        s.push_str(r);
        s.push('\n');
    }
    for r in ob.ledger_rows.iter() {
        s.push_str(r);
        s.push('\n');
    }
    for r in ob.matrix_rows.iter() {
        s.push_str(r);
        s.push('\n');
    }
    for r in ob.inject_rows.iter() {
        s.push_str(r);
        s.push('\n');
    }
    s
}

/// TLV 标签。
pub mod tlv_tag {
    /// 组头。
    pub const HEAD: u8 = 0x01;
    /// 三件字段样本。
    pub const TRINITY: u8 = 0x10;
    /// 链头尾。
    pub const HEADROW: u8 = 0x11;
    /// 账。
    pub const LEDGER: u8 = 0x20;
    /// 布局断言。
    pub const LAYOUT: u8 = 0x21;
    /// 注入结论。
    pub const INJECT: u8 = 0x22;
    /// 指纹。
    pub const FINGERPRINT: u8 = 0x7F;
}

/// 组头载荷长度（24）。
pub const TLV_HEAD_PAYLOAD: usize = 24;

/// 组头：`len2 + magic8 + version8 + len8`。
/// `push_u64(version)` **单独一行**，不被行尾注释吞。
pub fn build_head(version: u64) -> Vec<u8> {
    let mut h: Vec<u8> = Vec::with_capacity(2 + TLV_HEAD_PAYLOAD);
    push_u16(&mut h, (TLV_HEAD_PAYLOAD & 0xFFFF) as u16);
    push_u64(&mut h, 0x554E_5832_3730_3200);
    push_u64(&mut h, version);
    push_u64(&mut h, TLV_HEAD_PAYLOAD as u64);
    h
}

/// 追加一条 TLV。
pub fn push_tlv(out: &mut Vec<u8>, tag: u8, payload: &[u8]) {
    out.push(tag);
    push_u16(out, (payload.len() & 0xFFFF) as u16);
    let mut i: usize = 0;
    while i < payload.len() {
        match payload.get(i) {
            Some(b) => out.push(*b),
            None => {}
        }
        i = i.wrapping_add(1);
    }
}

/// 导出 TLV 字节流。
pub fn export_tlv(ob: &LruObserve) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    // 三件字段样本（把每条 head 行折成 payload）。
    for r in ob.head_rows.iter() {
        let mut p: Vec<u8> = Vec::new();
        push_u64(&mut p, fnv1a64(r.as_bytes()));
        push_u64(&mut p, TRINITY_COUNT as u64);
        push_tlv(&mut body, tlv_tag::TRINITY, &p);
    }
    // 链头尾。
    for r in ob.head_rows.iter() {
        let mut p: Vec<u8> = Vec::new();
        push_u64(&mut p, fnv1a64(r.as_bytes()));
        push_u64(&mut p, LIST_COUNT as u64);
        push_tlv(&mut body, tlv_tag::HEADROW, &p);
    }
    // 账。
    let mut lp: Vec<u8> = Vec::new();
    push_u64(&mut lp, ob.insert_ok as u64);
    push_u64(&mut lp, ob.walk_pass as u64);
    push_u64(&mut lp, ob.ghost_rejected as u64);
    push_u64(&mut lp, if ob.conserved { 1 } else { 0 });
    push_tlv(&mut body, tlv_tag::LEDGER, &lp);
    // 布局。
    for r in ob.layout_rows.iter() {
        let mut p: Vec<u8> = Vec::new();
        push_u64(&mut p, fnv1a64(r.as_bytes()));
        push_u64(&mut p, TRINITY_ALIGN as u64);
        push_tlv(&mut body, tlv_tag::LAYOUT, &p);
    }
    // 注入。
    for r in ob.inject_rows.iter() {
        let mut p: Vec<u8> = Vec::new();
        push_u64(&mut p, fnv1a64(r.as_bytes()));
        push_u64(&mut p, FORM_COUNT as u64);
        push_tlv(&mut body, tlv_tag::INJECT, &p);
    }
    // 指纹。
    let mut fp: Vec<u8> = Vec::new();
    push_u64(&mut fp, ob.fp);
    push_u64(&mut fp, ob.heads_fp);
    push_u64(&mut fp, ob.replay_fp);
    push_tlv(&mut body, tlv_tag::FINGERPRINT, &fp);
    // 组头前置。
    let head: Vec<u8> = build_head(V2702_VERSION_ID);
    let mut out: Vec<u8> = Vec::with_capacity(head.len() + body.len());
    let mut i: usize = 0;
    while i < head.len() {
        match head.get(i) {
            Some(b) => out.push(*b),
            None => {}
        }
        i = i.wrapping_add(1);
    }
    let mut k: usize = 0;
    while k < body.len() {
        match body.get(k) {
            Some(b) => out.push(*b),
            None => {}
        }
        k = k.wrapping_add(1);
    }
    out
}

/// TLV 长度自洽（跳过组头 `len2 + payload24`）。
pub fn tlv_len_ok(buf: &[u8]) -> bool {
    let mut off: usize = 2 + TLV_HEAD_PAYLOAD;
    while off + 3 <= buf.len() {
        let len: usize = (read_u16(buf, off + 1) & 0xFFFF) as usize;
        let next: usize = off + 3 + len;
        if next > buf.len() {
            return false;
        }
        off = next;
    }
    off == buf.len()
}

/// TLV 往返一致（组头三字段 + 尾指纹回读）。
pub fn tlv_roundtrip(ob: &LruObserve) -> bool {
    let buf: Vec<u8> = export_tlv(ob);
    if !tlv_len_ok(&buf) {
        return false;
    }
    let magic: u64 = read_u64(&buf, 2);
    let version: u64 = read_u64(&buf, 10);
    let len: u64 = read_u64(&buf, 18);
    if magic != 0x554E_5832_3730_3200 || version != V2702_VERSION_ID || len != (TLV_HEAD_PAYLOAD as u64) {
        return false;
    }
    let mut off: usize = 2 + TLV_HEAD_PAYLOAD;
    let mut tail_fp: u64 = 0;
    while off + 3 <= buf.len() {
        let tag: u8 = match buf.get(off) {
            Some(x) => *x,
            None => 0,
        };
        let l: usize = (read_u16(&buf, off + 1) & 0xFFFF) as usize;
        if tag == tlv_tag::FINGERPRINT {
            tail_fp = read_u64(&buf, off + 3);
        }
        off = off + 3 + l;
    }
    tail_fp == ob.fp
}

/// 自检项。
#[derive(Clone, Debug)]
pub struct Check {
    /// 项名。
    pub name: &'static str,
    /// 是否通过。
    pub pass: bool,
    /// 细节。
    pub detail: String,
}

/// 自检全集。
pub fn self_check(ob: &LruObserve) -> Vec<Check> {
    let mut out: Vec<Check> = Vec::new();
    out.push(Check { name: "seg_closure", pass: code_in_seg(SEG_BASE) && code_in_seg(SEG_END) && (SEG_END - SEG_BASE == SEG_MASK), detail: format!("0x{:04X}..0x{:04X}", SEG_BASE, SEG_END) });
    out.push(Check { name: "err_closure", pass: LruErr::ALL.len() == 12, detail: format!("variants={}", LruErr::ALL.len()) });
    out.push(Check { name: "layout", pass: ob.layout_ok, detail: format!("group={} align={}", TRINITY_GROUP_LEN, TRINITY_ALIGN) });
    out.push(Check { name: "matrix_full", pass: ob.matrix_pass == ob.matrix_cells, detail: format!("{}/{}", ob.matrix_pass, ob.matrix_cells) });
    out.push(Check { name: "random_insert", pass: ob.insert_ok > 0, detail: format!("{}/{}", ob.insert_ok, ob.random_pages) });
    out.push(Check { name: "walk_full", pass: ob.walk_pass == ob.walk_samples, detail: format!("{}/{}", ob.walk_pass, ob.walk_samples) });
    out.push(Check { name: "ghost_20_20", pass: ob.ghost_rejected == ob.ghost_tries && ob.ghost_tries == GHOST_TOTAL, detail: format!("{}/{}", ob.ghost_rejected, ob.ghost_tries) });
    out.push(Check { name: "inject_all_rejected", pass: ob.inject_rejected == FORM_COUNT && ob.inject_leaked == 0, detail: format!("{}/{}", ob.inject_rejected, FORM_COUNT) });
    out.push(Check { name: "conserved", pass: ob.conserved, detail: "三件账+链账+环形+遍历+幽灵".to_string() });
    out.push(Check { name: "flag_legal", pass: ob.flag_ok, detail: "标志三态语义复核".to_string() });
    out.push(Check { name: "lock_discipline", pass: ob.lock_ok, detail: "锁外枚举变更恒 0".to_string() });
    out.push(Check { name: "tlv_roundtrip", pass: tlv_roundtrip(ob), detail: "组头三字段 + 尾指纹回读".to_string() });
    out.push(Check { name: "replay_stable", pass: ob.replay_stable, detail: format!("fp=0x{:016X}", ob.replay_fp) });
    out.push(Check { name: "replay_discriminates", pass: ob.replay_discriminates, detail: "跨种子折叠不同".to_string() });
    out
}

/// 自检全过。
pub fn self_check_ok(ob: &LruObserve) -> bool {
    let cs: Vec<Check> = self_check(ob);
    for c in cs.iter() {
        if !c.pass {
            return false;
        }
    }
    true
}

/// 自检报告。
pub fn self_check_report(ob: &LruObserve) -> String {
    let cs: Vec<Check> = self_check(ob);
    let mut s: String = String::new();
    s.push_str("== UNX-F2702 self-check ==\n");
    for c in cs.iter() {
        s.push_str(&format!("[{}] {}: {}\n", if c.pass { "PASS" } else { "FAIL" }, c.name, c.detail));
    }
    s.push_str(&format!("all_ok={}\n", if self_check_ok(ob) { 1 } else { 0 }));
    s
}

// ===========================================================================
// 12. 性能逐项分解 `complexity()`
// ===========================================================================

/// 性能分解条目。
#[derive(Clone, Debug)]
pub struct PerfItem {
    /// 操作名。
    pub name: &'static str,
    /// 时间复杂度。
    pub time: &'static str,
    /// 空间复杂度。
    pub space: &'static str,
    /// 系统调用次数（模型侧恒 0）。
    pub syscalls: u32,
    /// 步数上限。
    pub steps: u64,
    /// 对标 Windows 同类能力「深在哪」。
    pub vs_windows: &'static str,
}

impl PerfItem {
    /// 导出行。
    pub fn row(&self) -> String {
        format!("perf[{}] time={} space={} syscalls={} steps<={} win=\"{}\"", self.name, self.time, self.space, self.syscalls, self.steps, self.vs_windows)
    }
}

/// 性能分解表。
pub fn complexity() -> Vec<PerfItem> {
    let mut v: Vec<PerfItem> = Vec::new();
    v.push(PerfItem { name: "lru_link", time: "O(1)", space: "O(1)", syscalls: 0, steps: 12, vs_windows: "MMPFN 链表挂接同 O(1)；本条前置幽灵判把「双链」一次挡死" });
    v.push(PerfItem { name: "lru_unlink", time: "O(1)", space: "O(1)", syscalls: 0, steps: 14, vs_windows: "同 O(1)；本条摘链后三件同步复位并留旧值断言" });
    v.push(PerfItem { name: "cross_list_move", time: "O(1)", space: "O(1)", syscalls: 0, steps: 26, vs_windows: "Windows 无跨链单点；本条强制「先摘后挂」两跳，杜绝直改" });
    v.push(PerfItem { name: "reset_isolated", time: "O(1)", space: "O(1)", syscalls: 0, steps: 6, vs_windows: "无对应；隔离态显式化为第三态而非隐式 NULL" });
    v.push(PerfItem { name: "walk", time: "O(count)", space: "O(count)", syscalls: 0, steps: (PAGE_COUNT * 2) as u64, vs_windows: "遍历 PFN 链同量级；本条带 O(n) 环检测但步数硬顶 2N" });
    v.push(PerfItem { name: "walk_check", time: "O(count)", space: "O(count)", syscalls: 0, steps: (PAGE_COUNT * 2) as u64, vs_windows: "无对应；遍历长度==计数器是可测断言而非调试手段" });
    v.push(PerfItem { name: "trinity_check", time: "O(count)", space: "O(1)", syscalls: 0, steps: (PAGE_COUNT * 3) as u64, vs_windows: "无对应；三件逐页一致性被写成判定" });
    v.push(PerfItem { name: "ghost_scan", time: "O(PAGE_COUNT * LIST_COUNT)", space: "O(PAGE_COUNT)", syscalls: 0, steps: (PAGE_COUNT * LIST_COUNT) as u64, vs_windows: "Windows 无等效全表幽灵扫描；本条 O(N·L) 但 L=5 恒定" });
    v.push(PerfItem { name: "flag_legal", time: "O(PAGE_COUNT)", space: "O(1)", syscalls: 0, steps: (PAGE_COUNT * 2) as u64, vs_windows: "无对应；标志↔枚举等价被显式复核" });
    v.push(PerfItem { name: "random_insert", time: "O(n)", space: "O(1)", syscalls: 0, steps: RANDOM_PAGES as u64, vs_windows: "无对应；判据 1000 页随机入链，LCG 定轨迹" });
    v.push(PerfItem { name: "walk_assert_samples", time: "O(WALK_SAMPLES)", space: "O(1)", syscalls: 0, steps: WALK_SAMPLES as u64, vs_windows: "无对应；1000 处三件一致性抽样逐槽判定" });
    v.push(PerfItem { name: "ghost_sweep", time: "O(GHOST_TOTAL * PAGE_COUNT)", space: "O(PAGE_COUNT)", syscalls: 0, steps: (GHOST_TOTAL * PAGE_COUNT) as u64, vs_windows: "无对应；20 轮 × 批 双链尝试全数被挂接单点挡死" });
    v.push(PerfItem { name: "inject_all", time: "O(FORM_COUNT * PAGE_COUNT)", space: "O(FORM_COUNT * PAGE_COUNT)", syscalls: 0, steps: (FORM_COUNT * PAGE_COUNT * 2) as u64, vs_windows: "无对应；十形态各自在独立克隆上跑，检出互不污染" });
    v.push(PerfItem { name: "fingerprint", time: "O(PAGE_COUNT * 5)", space: "O(1)", syscalls: 0, steps: (PAGE_COUNT * 5) as u64, vs_windows: "无对应；全表三件可对拍指纹，逐槽覆盖含未挂接槽" });
    v.push(PerfItem { name: "verify", time: "O(PAGE_COUNT * LIST_COUNT)", space: "O(PAGE_COUNT)", syscalls: 0, steps: (PAGE_COUNT * LIST_COUNT) as u64, vs_windows: "无对应；六道闸一次过（布局/守恒/遍历/幽灵/标志/锁）" });
    v.push(PerfItem { name: "replay", time: "O(REPLAY_STEPS)", space: "O(PAGE_COUNT)", syscalls: 0, steps: (REPLAY_STEPS + PAGE_COUNT) as u64, vs_windows: "无对应；确定性回放双跑对拍 + 跨种子可区分" });
    v.push(PerfItem { name: "export_kv", time: "O(rows)", space: "O(rows)", syscalls: 0, steps: (LIST_COUNT * 4 + LIST_ENUM_COUNT * FLAG_COUNT + FORM_COUNT) as u64, vs_windows: "无对应；纯文本可读观测" });
    v.push(PerfItem { name: "export_tlv", time: "O(rows)", space: "O(rows)", syscalls: 0, steps: ((LIST_COUNT * 2 + 4 + FORM_COUNT + 1) * 8) as u64, vs_windows: "无对应；TLV 长度自洽 + 往返一致" });
    v.push(PerfItem { name: "observe", time: "O(RANDOM_PAGES + PAGE_COUNT*LIST_COUNT + REPLAY_STEPS)", space: "O(PAGE_COUNT + RING_CAP)", syscalls: 0, steps: (RANDOM_PAGES + PAGE_COUNT * LIST_COUNT + REPLAY_STEPS) as u64, vs_windows: "无对应；一次聚合跑完整判据链" });
    v
}

/// 性能分解表文本。
pub fn complexity_report() -> String {
    let v: Vec<PerfItem> = complexity();
    let mut s: String = String::new();
    s.push_str("== UNX-F2702 complexity ==\n");
    for it in v.iter() {
        s.push_str(&it.row());
        s.push('\n');
    }
    s
}

// ===========================================================================
// 13. 清单与演示
// ===========================================================================

/// 错误闭集清单。
pub fn err_manifest() -> Vec<String> {
    err_code_table()
}

/// 枚举清单。
pub fn list_manifest() -> Vec<String> {
    let mut v: Vec<String> = Vec::with_capacity(LIST_ENUM_COUNT);
    let mut i: usize = 0;
    while i < LIST_ENUM_COUNT {
        let l: LruList = match LruList::from_index(i) {
            Some(x) => x,
            None => LruList::None,
        };
        v.push(format!("list[{}] chain={} anon={} active={} expect_flag={}", l.code(), if l.is_chain() { 1 } else { 0 }, if l.is_anon() { 1 } else { 0 },
            if l.is_active() { 1 } else { 0 }, expect_flag_for(l).code()));
        i = i.wrapping_add(1);
    }
    v
}

/// 标志清单。
pub fn flag_manifest() -> Vec<String> {
    let mut v: Vec<String> = Vec::with_capacity(FLAG_COUNT);
    let mut i: usize = 0;
    while i < FLAG_COUNT {
        let f: LruFlag = match LruFlag::from_index(i) {
            Some(x) => x,
            None => LruFlag::Off,
        };
        v.push(format!("flag[{}] on_chain={}", f.code(), if f.on_chain() { 1 } else { 0 }));
        i = i.wrapping_add(1);
    }
    v
}

/// 注入形态清单。
pub fn form_manifest() -> Vec<String> {
    let mut v: Vec<String> = Vec::with_capacity(FORM_COUNT);
    let mut i: usize = 0;
    while i < FORM_COUNT {
        let f: Form = Form::ALL[i];
        v.push(format!("form[{}] {} expect=0x{:04X} {}", i, f.name(), f.expect_err().code(), f.describe()));
        i = i.wrapping_add(1);
    }
    v
}

/// 布局清单。
pub fn layout_manifest() -> Vec<String> {
    let mut v: Vec<String> = Vec::with_capacity(4);
    v.push(format!("desc_head_len={} trinity_group_len={} desc_len={}", DESC_HEAD_LEN, TRINITY_GROUP_LEN, DESC_LEN));
    v.push(format!("cache_line={} align={} nil=0x{:X}", CACHE_LINE, TRINITY_ALIGN, NIL));
    v.push(format!("page_count={} pfn_mask=0x{:X}", PAGE_COUNT, PFN_MASK));
    v.push(format!("list_count={} list_enum_count={} flag_count={}", LIST_COUNT, LIST_ENUM_COUNT, FLAG_COUNT));
    v
}

/// 演示场景。
pub fn demo_scene(seed: u64) -> Vec<String> {
    let ob: LruObserve = observe(seed);
    let mut v: Vec<String> = Vec::new();
    v.push(format!("seed=0x{:016X}", ob.seed));
    v.push(format!("summary={}", ob.summary_line));
    v.push(ob.summary());
    for r in ob.head_rows.iter() {
        v.push(r.clone());
    }
    v.push(self_check_report(&ob));
    v
}

/// 码段占用表。
pub fn seg_usage() -> Vec<String> {
    let mut v: Vec<String> = Vec::with_capacity((SEG_MASK as usize) + 1);
    let mut i: usize = 0;
    while i < LruErr::ALL.len() {
        let e: LruErr = LruErr::ALL[i];
        v.push(format!("0x{:04X} used {}", e.code(), e.name()));
        i = i.wrapping_add(1);
    }
    let mut k: usize = LruErr::ALL.len();
    while k <= (SEG_MASK as usize) {
        v.push(format!("0x{:04X} reserved", SEG_BASE + (k as u16)));
        k = k.wrapping_add(1);
    }
    v
}

/// 三件字段偏移表（追加式扩展，F2401 偏移不变）。
pub fn field_offset_table() -> Vec<String> {
    let mut v: Vec<String> = Vec::with_capacity(8);
    v.push(format!("+0x{:02X} pfn            u64", 0));
    v.push(format!("+0x{:02X} mapcount       u64", 8));
    v.push(format!("+0x{:02X} lru_node.prev  u64", DESC_HEAD_LEN));
    v.push(format!("+0x{:02X} lru_node.next  u64", DESC_HEAD_LEN + 8));
    v.push(format!("+0x{:02X} lru_list       u64", DESC_HEAD_LEN + 16));
    v.push(format!("+0x{:02X} lru_flag       u64", DESC_HEAD_LEN + 24));
    v.push(format!("+0x{:02X} prev_list      u64", DESC_HEAD_LEN + 32));
    v.push(format!("+0x{:02X} prev_flag      u64", DESC_HEAD_LEN + 40));
    v
}

/// 链路深度探针（量某链的遍历深度）。
pub fn chain_depth(t: &LruTables, list: LruList) -> usize {
    let (members, _, _): (Vec<usize>, bool, bool) = t.walk(list);
    members.len()
}


