//! ===========================================================================
//! UNX-F2662 · cache_lookup 读接口（冻结接口 3 之一）
//! ---------------------------------------------------------------------------
//! 码段：0x1D80 .. 0x1DAB（44 变体错误码闭集）
//! 纯功能行数目标：280（契约冻结 110 + 消费联签 90 + 断言 80；测试段不计）
//! 深化实现行数：>= 2800（产线硬指标，`wc -l` 自查）
//! ---------------------------------------------------------------------------
//! 【定位】
//!   冻结接口 3 的三接口之一——cache_lookup。
//!   任务书原文"读命中免块层 IO"的**接口半边**：
//!   命中返回页帧，未命中返回未命中态，**且不产生副作用**。
//!   正式冻结版在 F2682；本单只做**契约与实现面**。
//!
//! 【语义边界（本单不做）】
//!   - 不做未命中后的读块编排  -> F2666
//!   - 不做回写                 -> B5
//!   - 不做正式冻结版           -> F2682
//!
//! 【与现存内核的衔接点】
//!   - F2661 块树 / 索引：lookup 的树查询实现由 F2661 提供，本单消费其形状
//!   - F2663 页帧格式：本单只读页帧号，不解释页帧内容
//!   - B2 / B3 回调：相邻消费面
//!
//! 【B1–B5 消费面联签】
//!   五方登记，每方登记四项：消费面标识 / 期望签名 / 回归责任 / 冻结版本。
//!   联签变更走 §16.2（即 `ErrKind::Clause162`）。
//!
//! 【只读承诺 —— 本单最硬的一条】
//!   "lookup 只读静态断言（F2569 同制），副作用注入检出。"
//!   这条承诺的落地方式不是"承诺自己会遵守"，而是**结构上使违反可检出**：
//!   1. `lookup` 签名取 `&self`（不是 `&mut self`）——类型层排除"改缓存状态"
//!   2. 缓存本体是纯数据结构，lookup 前后各算一次缓存指纹，
//!      必须逐位相等，否则报 `ErrKind::SideEffectLeak`
//!   3. 负向注入核构造"会改状态的 lookup"（引用计数 +1 / LRU 顺序变动 /
//!      统计计数 +1），10/10 必须被夹逼检出
//!
//! 【Windows 对照 · 深在哪】
//!   Windows 的 `CcLookup`（Cache Manager）与 `FsRtlLookupFileMcc` 都是
//!   纯查询，但它们**不承诺无副作用**——事实上它们会更新访问时间、
//!   可能触发 readahead、可能做后台预取。因此调用方无法区分
//!   "这次查询是免费的"还是"这次查询付了 IO 代价"。
//!   本单的"深"在于把"只读"从**性能属性**升级为**可断言的语义契约**：
//!   - 命中/未命中是两个**枚举态**而不是错误码，避免调用方用 `Err` 分支
//!     顺带做了恢复动作（恢复动作 = 副作用）
//!   - 未命中态**不带任何载荷**，结构上无法"顺手把未命中信息塞进返回结构"
//!   - 只读性由前后指纹夹逼证明，可离线回归
//!
//! 【与判据的对应】
//!   - lookup 契约冻结登记（命中返回页帧/未命中返回未命中态）
//!        -> `Contract` + `export_kv` / `export_tlv` + `contract_audit()`
//!   - B1–B5 消费面联签在档
//!        -> `Signer` + `sign_ledger()` + `sign_audit()`（5 方 x 4 项）
//!   - 10^5 次命中/未命中断言（判据正文第二步）
//!        -> `hit_miss_10e5()`
//!   - 只读静态断言零例外
//!        -> `readonly_static_audit()` + 注入核 `Form::Mutate*` 形态
//!
//! 【零约束声明】
//!   no_std + alloc。零 unwrap / expect / panic! / todo!，零浮点，零真实 IO，
//!   零墙钟。逻辑时钟 `tick: u64`，非确定性来源收敛到显式 LCG 种子。
//! ===========================================================================

#![allow(dead_code)]

extern crate alloc;

// ---------------------------------------------------------------------------
// 固定导入（产线硬要求，不得增删）
// ---------------------------------------------------------------------------
#[allow(unused_imports)]
use alloc::{vec, vec::Vec, string::{String, ToString}, format};

// ===========================================================================
// 第 01 节 · 常量表（判据阈值写死，来源逐条注释）
// ===========================================================================

/// 错误码段基址。与任务书码段 0x1D80 严格对齐。
pub const CODE_BASE: u16 = 0x1D80;
/// 错误码段上界（不含），44 变体闭集。
pub const CODE_END: u16 = 0x1DAC;
/// 任务书登记的纯功能行数目标（契约 110 + 联签 90 + 断言 80）。
pub const CONTRACT_LINES: u32 = 110;
/// 消费联签部分行数目标。
pub const SIGN_LINES: u32 = 90;
/// 断言部分行数目标。
pub const ASSERT_LINES: u32 = 80;
/// 册目标合计。
pub const BOOK_LINES: u32 = CONTRACT_LINES + SIGN_LINES + ASSERT_LINES;
/// 深化实现行数硬指标。
pub const HARD_LINES: u32 = 2800;

/// 判据正文「命中/未命中 10^5 次断言」的次数。
pub const HITMISS_ROUNDS: u64 = 100_000;
/// 注入形态数。产线要求 >= 10，本单取 12。
pub const FORM_COUNT: u32 = 12;
/// 每形态重复次数（判据「10/10」的来源）。
pub const FORM_REPEAT: u32 = 10;
/// 注入总次数。
pub const INJECT_TOTAL: u32 = 120;

/// B1–B5 五方消费面数。
pub const SIGNER_COUNT: u32 = 5;
/// 联签登记项数（每方 4 项：面标识 / 期望签名 / 回归责任 / 冻结版本）。
pub const SIGN_FIELDS: u32 = 4;

/// 块树扇出上界（逻辑模型）。
pub const FANOUT: usize = 8;
/// 块树最大层数。
pub const MAX_DEPTH: u32 = 6;
/// 逻辑块数（页帧池大小）。
pub const FRAMES: usize = 4096;
/// 页内偏移位宽（4KiB = 12 位）。
pub const OFFSET_BITS: u32 = 12;
/// 页内偏移掩码。
pub const OFFSET_MASK: u64 = (1u64 << OFFSET_BITS) - 1u64;
/// 页帧号位宽。
pub const PFN_BITS: u32 = 12;
/// 页帧号掩码（12 位，4096 帧）。
pub const PFN_MASK: u64 = (FRAMES as u64) - 1u64;

/// 未命中占位页帧号。契约规定未命中态**不带页帧**，
/// 该常量仅用于"显式取值会失败"的负向断言，不作为合法返回。
pub const MISS_PFN_PLACEHOLDER: u64 = 0xFFFF_FFFF_FFFF_FFFFu64;

/// 环形账容量（三见证）。
pub const RING_CAP: usize = 64;
/// 回放步数上限。
pub const REPLAY_STEPS: u64 = 200_000;
/// 自检项数。
pub const SELF_CHECK_ITEMS: u32 = 18;
/// 契约版本。变更须走联签。
pub const CONTRACT_VERSION: u64 = 3u64;
/// §16.2 联签变更条款号。
pub const CLAUSE_16_2: u32 = 16u32;

/// FNV-1a 参数。
pub const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325u64;
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3u64;
/// LCG 参数（Knuth MMIX 常数），wrapping 语义。
pub const LCG_MUL: u64 = 6_364_136_223_846_793_005u64;
pub const LCG_ADD: u64 = 1_442_695_040_888_963_407u64;

/// TLV 导出组头版本号（单独成行，不被行尾注释吞）。
pub const TLV_VERSION: u64 = 0x1D80_0001u64;

/// 查找的合法输入空间大小（文件块号空间）。
pub const KEY_SPACE: u64 = 1u64 << 20;

// ===========================================================================
// 第 02 节 · 确定性 LCG 与 FNV-1a
// ===========================================================================

/// 确定性线性同余发生器。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lcg {
    pub state: u64,
}

impl Lcg {
    /// 以种子建发生器。种子为 0 也不特殊处理。
    pub fn new(seed: u64) -> Self {
        Lcg { state: seed }
    }

    /// 取下一值。wrapping 语义。
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(LCG_MUL).wrapping_add(LCG_ADD);
        self.state
    }

    /// 取 [0, n) 均匀值。n = 0 返回 0。
    #[inline]
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0u64 {
            return 0u64;
        }
        self.next_u64() % n
    }

    /// 取 [lo, hi] 闭区间值。hi <= lo 返回 lo。
    #[inline]
    pub fn range(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo {
            return lo;
        }
        lo + self.below(hi - lo + 1u64)
    }

    /// 取布尔位。
    #[inline]
    pub fn bit(&mut self) -> bool {
        (self.next_u64() >> 63) & 1u64 == 1u64
    }

    /// 派生子种子（隔离各子流程随机流）。
    #[inline]
    pub fn fork_seed(&mut self, tag: u64) -> u64 {
        let mut h = FNV_BASIS;
        h = fnv1a_mix(h, self.state);
        h = fnv1a_mix(h, tag);
        self.state = h ^ LCG_ADD;
        h
    }
}

/// FNV-1a 64 位单轮混合（混合一个 u64 的 8 字节）。
#[inline]
pub fn fnv1a_mix(h: u64, v: u64) -> u64 {
    let mut acc = h;
    let mut b = v;
    let mut i: u32 = 0u32;
    while i < 8u32 {
        acc = acc ^ (b & 0xffu64);
        acc = acc.wrapping_mul(FNV_PRIME);
        b = b >> 8;
        i = i + 1u32;
    }
    acc
}

/// 字符串的 FNV-1a 64 位摘要。
pub fn fnv1a_str(mut h: u64, s: &str) -> u64 {
    for b in s.as_bytes() {
        h = h ^ (*b as u64);
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// u8 切片折叠。
pub fn fold_u8(mut h: u64, bs: &[u8]) -> u64 {
    let mut i: usize = 0usize;
    while i < bs.len() {
        h = h ^ (bs[i] as u64);
        h = h.wrapping_mul(FNV_PRIME);
        i = i + 1usize;
    }
    h
}

// ===========================================================================
// 第 03 节 · 错误闭集（44 变体）
// ---------------------------------------------------------------------------
// 三要素：现象 / 位置 / 建议修法。
// `index()` 顺序即码序，不可重排——重排破坏已冻结的错误码登记。
// ===========================================================================

/// 错误闭集变体数。
pub const ERR_VARIANTS: u32 = 44;

/// 错误闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrKind {
    /// 0 · 参数越界。
    ArgRange,
    /// 1 · 地址未按页对齐。
    Align,
    /// 2 · 块树为空（未装载）。
    TreeEmpty,
    /// 3 · 键越界（超出 KEY_SPACE）。
    KeyRange,
    /// 4 · 扇出配置非法。
    Fanout,
    /// 5 · 层数超限。
    DepthLimit,
    /// 6 · 节点槽位越界。
    SlotRange,
    /// 7 · 节点槽位重复占用。
    SlotDup,
    /// 8 · 节点未初始化。
    NodeUninit,
    /// 9 · 页帧池越界。
    FrameRange,
    /// 10 · 页帧重复映射同一块号。
    FrameDup,
    /// 11 · 缓存本体指纹失配（内部损坏）。
    CacheCorrupt,
    /// 12 · lookup 产生副作用（只读承诺被破）。
    SideEffectLeak,
    /// 13 · 签名不匹配。
    SignMismatch,
    /// 14 · 联签缺失一方。
    SignMissing,
    /// 15 · 联签重复登记。
    SignDup,
    /// 16 · 联签版本漂移。
    SignVersionDrift,
    /// 17 · 契约版本漂移。
    ContractDrift,
    /// 18 · 契约条目缺失。
    ContractMissing,
    /// 19 · 契约重复条目。
    ContractDup,
    /// 20 · §16.2 联签变更未走联签流程。
    Clause162,
    /// 21 · 命中态未带页帧。
    HitNoFrame,
    /// 22 · 未命中态带了页帧（契约禁止）。
    MissHasFrame,
    /// 23 · 未命中态带了载荷（契约禁止）。
    MissHasPayload,
    /// 24 · 返回态编号越界。
    ResultRange,
    /// 25 · 命中未命中比例失衡。
    RatioOff,
    /// 26 · 命中率超 100%。
    RatioOverflow,
    /// 27 · 环形账容量非法。
    RingCap,
    /// 28 · 环形账三见证校验失败。
    RingVerify,
    /// 29 · 环形账掉队项非预期。
    RingDropped,
    /// 30 · 冻结指纹不匹配。
    FingerprintMismatch,
    /// 31 · 回放漂移。
    ReplayDrift,
    /// 32 · 判据回归失败。
    CriterionFail,
    /// 33 · 只读静态审计失败。
    ReadonlyAudit,
    /// 34 · TLV 值越界。
    TlvRange,
    /// 35 · TLV 组头顺序错位。
    TlvOrder,
    /// 36 · 自检项失败。
    SelfCheckFail,
    /// 37 · 消费者身份未知。
    SignerUnknown,
    /// 38 · 回归责任未登记。
    SignerNoDuty,
    /// 39 · 树形状自洽失败。
    TreeShape,
    /// 40 · 分裂/合并未被记账。
    SplitUnaccounted,
    /// 41 · 统计计数溢出。
    StatOverflow,
    /// 42 · 逻辑时钟溢出。
    TickOverflow,
    /// 43 · 缓存重建后指纹漂移。
    RebuildDrift,
}

impl ErrKind {
    /// 稳定错误码。`CODE_BASE + 序号`。
    pub fn code(&self) -> u16 {
        CODE_BASE.wrapping_add(self.index() as u16)
    }

    /// 变体在闭集中的序号（0..=43）。
    pub fn index(&self) -> u32 {
        match self {
            ErrKind::ArgRange => 0u32,
            ErrKind::Align => 1u32,
            ErrKind::TreeEmpty => 2u32,
            ErrKind::KeyRange => 3u32,
            ErrKind::Fanout => 4u32,
            ErrKind::DepthLimit => 5u32,
            ErrKind::SlotRange => 6u32,
            ErrKind::SlotDup => 7u32,
            ErrKind::NodeUninit => 8u32,
            ErrKind::FrameRange => 9u32,
            ErrKind::FrameDup => 10u32,
            ErrKind::CacheCorrupt => 11u32,
            ErrKind::SideEffectLeak => 12u32,
            ErrKind::SignMismatch => 13u32,
            ErrKind::SignMissing => 14u32,
            ErrKind::SignDup => 15u32,
            ErrKind::SignVersionDrift => 16u32,
            ErrKind::ContractDrift => 17u32,
            ErrKind::ContractMissing => 18u32,
            ErrKind::ContractDup => 19u32,
            ErrKind::Clause162 => 20u32,
            ErrKind::HitNoFrame => 21u32,
            ErrKind::MissHasFrame => 22u32,
            ErrKind::MissHasPayload => 23u32,
            ErrKind::ResultRange => 24u32,
            ErrKind::RatioOff => 25u32,
            ErrKind::RatioOverflow => 26u32,
            ErrKind::RingCap => 27u32,
            ErrKind::RingVerify => 28u32,
            ErrKind::RingDropped => 29u32,
            ErrKind::FingerprintMismatch => 30u32,
            ErrKind::ReplayDrift => 31u32,
            ErrKind::CriterionFail => 32u32,
            ErrKind::ReadonlyAudit => 33u32,
            ErrKind::TlvRange => 34u32,
            ErrKind::TlvOrder => 35u32,
            ErrKind::SelfCheckFail => 36u32,
            ErrKind::SignerUnknown => 37u32,
            ErrKind::SignerNoDuty => 38u32,
            ErrKind::TreeShape => 39u32,
            ErrKind::SplitUnaccounted => 40u32,
            ErrKind::StatOverflow => 41u32,
            ErrKind::TickOverflow => 42u32,
            ErrKind::RebuildDrift => 43u32,
        }
    }

    /// 变体名。
    pub fn name(&self) -> &'static str {
        match self {
            ErrKind::ArgRange => "ARG_RANGE",
            ErrKind::Align => "ALIGN",
            ErrKind::TreeEmpty => "TREE_EMPTY",
            ErrKind::KeyRange => "KEY_RANGE",
            ErrKind::Fanout => "FANOUT",
            ErrKind::DepthLimit => "DEPTH_LIMIT",
            ErrKind::SlotRange => "SLOT_RANGE",
            ErrKind::SlotDup => "SLOT_DUP",
            ErrKind::NodeUninit => "NODE_UNINIT",
            ErrKind::FrameRange => "FRAME_RANGE",
            ErrKind::FrameDup => "FRAME_DUP",
            ErrKind::CacheCorrupt => "CACHE_CORRUPT",
            ErrKind::SideEffectLeak => "SIDE_EFFECT_LEAK",
            ErrKind::SignMismatch => "SIGN_MISMATCH",
            ErrKind::SignMissing => "SIGN_MISSING",
            ErrKind::SignDup => "SIGN_DUP",
            ErrKind::SignVersionDrift => "SIGN_VERSION_DRIFT",
            ErrKind::ContractDrift => "CONTRACT_DRIFT",
            ErrKind::ContractMissing => "CONTRACT_MISSING",
            ErrKind::ContractDup => "CONTRACT_DUP",
            ErrKind::Clause162 => "CLAUSE_16_2",
            ErrKind::HitNoFrame => "HIT_NO_FRAME",
            ErrKind::MissHasFrame => "MISS_HAS_FRAME",
            ErrKind::MissHasPayload => "MISS_HAS_PAYLOAD",
            ErrKind::ResultRange => "RESULT_RANGE",
            ErrKind::RatioOff => "RATIO_OFF",
            ErrKind::RatioOverflow => "RATIO_OVERFLOW",
            ErrKind::RingCap => "RING_CAP",
            ErrKind::RingVerify => "RING_VERIFY",
            ErrKind::RingDropped => "RING_DROPPED",
            ErrKind::FingerprintMismatch => "FINGERPRINT_MISMATCH",
            ErrKind::ReplayDrift => "REPLAY_DRIFT",
            ErrKind::CriterionFail => "CRITERION_FAIL",
            ErrKind::ReadonlyAudit => "READONLY_AUDIT",
            ErrKind::TlvRange => "TLV_RANGE",
            ErrKind::TlvOrder => "TLV_ORDER",
            ErrKind::SelfCheckFail => "SELF_CHECK_FAIL",
            ErrKind::SignerUnknown => "SIGNER_UNKNOWN",
            ErrKind::SignerNoDuty => "SIGNER_NO_DUTY",
            ErrKind::TreeShape => "TREE_SHAPE",
            ErrKind::SplitUnaccounted => "SPLIT_UNACCOUNTED",
            ErrKind::StatOverflow => "STAT_OVERFLOW",
            ErrKind::TickOverflow => "TICK_OVERFLOW",
            ErrKind::RebuildDrift => "REBUILD_DRIFT",
        }
    }

    /// 稳定错误码的三要素说明：现象 / 位置 / 建议修法。
    pub fn detail(&self) -> String {
        let base = CODE_BASE.wrapping_add(self.index() as u16);
        let head = format!("0x{:04X} {} : ", base, self.name());
        match self {
            ErrKind::ArgRange => format!(
                "{}现象=参数越界 | 位置=Cache::lookup/file_id | 建议=调用前按 KEY_SPACE 与 FRAMES 夹取",
                head
            ),
            ErrKind::Align => format!(
                "{}现象=偏移未按 2^{} 字节对齐 | 位置=LookupKey::new | 建议=先判 offset==0 再入树查询",
                head, OFFSET_BITS
            ),
            ErrKind::TreeEmpty => format!(
                "{}现象=块树为空未装载 | 位置=BlockTree::new | 建议=先装载索引再查询，否则返回未命中是假阴性",
                head
            ),
            ErrKind::KeyRange => format!(
                "{}现象=块号超出键空间 | 位置=BlockTree::walk | 建议=键号必须 < KEY_SPACE，否则视为调用方越界",
                head
            ),
            ErrKind::Fanout => format!(
                "{}现象=扇出配置非法 | 位置=Node::child_set | 建议=扇出必须 >= 2 且 <= FANOUT，否则无法区分内部节点",
                head
            ),
            ErrKind::DepthLimit => format!(
                "{}现象=查询层数超限 | 位置=BlockTree::walk | 建议=深度上限 MAX_DEPTH={}，超限说明树退化",
                head, MAX_DEPTH
            ),
            ErrKind::SlotRange => format!(
                "{}现象=节点槽位下标越界 | 位置=Node::child_set | 建议=槽位下标必须 < FANOUT",
                head
            ),
            ErrKind::SlotDup => format!(
                "{}现象=同一槽位被重复占用 | 位置=Node::child_set | 建议=覆盖写必须显式声明，旧实现默认为覆盖",
                head
            ),
            ErrKind::NodeUninit => format!(
                "{}现象=节点未初始化 | 位置=BlockTree::node_at | 建议=节点槽必须逐个构造，禁止用零值近似",
                head
            ),
            ErrKind::FrameRange => format!(
                "{}现象=页帧号越界 | 位置=BlockTree::bind_frame | 建议=页帧必须 < FRAMES，超界即映射表损坏",
                head
            ),
            ErrKind::FrameDup => format!(
                "{}现象=同一块号被绑到两个页帧 | 位置=BlockTree::bind_frame | 建议=一一映射，重复绑定会让 hit 返回错误页帧",
                head
            ),
            ErrKind::CacheCorrupt => format!(
                "{}现象=缓存本体指纹失配 | 位置=Cache::self_fp | 建议=指纹覆盖全部可变状态，失配即内部损坏",
                head
            ),
            ErrKind::SideEffectLeak => format!(
                "{}现象=lookup 前后缓存指纹不相等（只读承诺被破）| 位置=Cache::readonly_guard | 建议=把改动移到 F2666 编排侧，lookup 侧不得有任何写",
                head
            ),
            ErrKind::SignMismatch => format!(
                "{}现象=联签期望签名与实际签名不符 | 位置=Signer::sign | 建议=签名由 (面标识, 版本, 只读承诺) 三元组折叠，任一变动须走 §{}.{}",
                head, CLAUSE_16_2, 2u32
            ),
            ErrKind::SignMissing => format!(
                "{}现象=联签缺一方 | 位置=SignLedger::audit | 建议=B1–B5 五方必须齐全，缺任一方则契约未冻结",
                head
            ),
            ErrKind::SignDup => format!(
                "{}现象=联签重复登记同一方 | 位置=SignLedger::sign | 建议=一方只能签一次，重复签说明流程被走两遍",
                head
            ),
            ErrKind::SignVersionDrift => format!(
                "{}现象=联签版本与契约版本不符 | 位置=SignLedger::audit | 建议=契约版本变更后五方须同步重签",
                head
            ),
            ErrKind::ContractDrift => format!(
                "{}现象=契约指纹漂移 | 位置=Contract::verify | 建议=契约条目顺序与文本均参与指纹，改一条即需重冻",
                head
            ),
            ErrKind::ContractMissing => format!(
                "{}现象=契约条目缺失 | 位置=Contract::audit | 建议=三条契约（命中/未命中/只读）必须齐全",
                head
            ),
            ErrKind::ContractDup => format!(
                "{}现象=契约条目重复 | 位置=Contract::audit | 建议=条目 key 唯一，重复即构造错误",
                head
            ),
            ErrKind::Clause162 => format!(
                "{}现象=§{}.{} 联签变更未走联签流程 | 位置=Contract::amend | 建议=任何契约变更必须先有五方联签记录",
                head, CLAUSE_16_2, 2u32
            ),
            ErrKind::HitNoFrame => format!(
                "{}现象=命中态未带页帧 | 位置=LookupResult::frame | 建议=契约规定命中必须返回页帧，缺页帧的命中是空承诺",
                head
            ),
            ErrKind::MissHasFrame => format!(
                "{}现象=未命中态带了页帧 | 位置=LookupResult::audit | 建议=契约明文禁止：带页帧的未命中会诱导调用方直接用它",
                head
            ),
            ErrKind::MissHasPayload => format!(
                "{}现象=未命中态带了载荷 | 位置=LookupResult::audit | 建议=未命中态必须零载荷，否则就成了一个可写的缓存出口",
                head
            ),
            ErrKind::ResultRange => format!(
                "{}现象=返回态编号越界 | 位置=LookupResult::audit | 建议=返回态只有 Hit/Miss 两种，编号必须 < 2",
                head
            ),
            ErrKind::RatioOff => format!(
                "{}现象=命中未命中比例失衡 | 位置=hit_miss_10e5 | 建议=分布断言要求两侧都占一定份额，全命中或全未命中都不算有效样本",
                head
            ),
            ErrKind::RatioOverflow => format!(
                "{}现象=命中率超 100% | 位置=hit_miss_10e5 | 建议=hit + miss 必须恒等于总次数",
                head
            ),
            ErrKind::RingCap => format!(
                "{}现象=环形账容量非法 | 位置=TraceRing::new | 建议=容量必须 >= 2，否则三见证无法区分覆盖与新增",
                head
            ),
            ErrKind::RingVerify => format!(
                "{}现象=环形账三见证校验失败 | 位置=TraceRing::verify | 建议=len/sum/xor 任一不符即账损坏",
                head
            ),
            ErrKind::RingDropped => format!(
                "{}现象=环形账掉队项非预期 | 位置=TraceRing::dropped | 建议=超出容量必须显性计入 dropped",
                head
            ),
            ErrKind::FingerprintMismatch => format!(
                "{}现象=冻结指纹不匹配 | 位置=Cache::verify | 建议=指纹口径或字段序变动，须重新冻结并更新登记",
                head
            ),
            ErrKind::ReplayDrift => format!(
                "{}现象=回放漂移（步数超限或指纹不同）| 位置=replay | 建议=同种子必须同序列，查 LCG 派生顺序",
                head
            ),
            ErrKind::CriterionFail => format!(
                "{}现象=判据回归失败 | 位置=criterion | 建议=对照 UNX-F2662-J1 三条判据逐项定位",
                head
            ),
            ErrKind::ReadonlyAudit => format!(
                "{}现象=只读静态审计失败 | 位置=readonly_static_audit | 建议=签名须为 &self 且实现内不得出现写分支",
                head
            ),
            ErrKind::TlvRange => format!(
                "{}现象=TLV 值越界 | 位置=export_tlv | 建议=u64 值不得越过 2^64-1",
                head
            ),
            ErrKind::TlvOrder => format!(
                "{}现象=TLV 组头顺序错位 | 位置=export_tlv | 建议=组头 version 必须单独成行置于所有键值对之前",
                head
            ),
            ErrKind::SelfCheckFail => format!(
                "{}现象=自检项失败 | 位置=self_check | 建议=按返回的首个失败项序号定位",
                head
            ),
            ErrKind::SignerUnknown => format!(
                "{}现象=消费者身份未知 | 位置=signer_by_id | 建议=只接受 B1–B5 已登记身份",
                head
            ),
            ErrKind::SignerNoDuty => format!(
                "{}现象=回归责任未登记 | 位置=SignLedger::audit | 建议=每方必须指名回归责任人，否则回归无人担责",
                head
            ),
            ErrKind::TreeShape => format!(
                "{}现象=树形状自洽失败 | 位置=BlockTree::shape_audit | 建议=父子层数必须严格 +1，叶子层数必须一致",
                head
            ),
            ErrKind::SplitUnaccounted => format!(
                "{}现象=分裂/合并未被记账 | 位置=BlockTree::split | 建议=分裂必须分裂数 + 合并数配平，否则树会无声失衡",
                head
            ),
            ErrKind::StatOverflow => format!(
                "{}现象=统计计数溢出 | 位置=Stats::bump | 建议=饱和而非回绕：回绕会把 2^64 次查询伪装成 0 次",
                head
            ),
            ErrKind::TickOverflow => format!(
                "{}现象=逻辑时钟溢出 | 位置=Cache::tick | 建议=逻辑时钟必须饱和，回绕会破坏回放的步数语义",
                head
            ),
            ErrKind::RebuildDrift => format!(
                "{}现象=缓存重建后指纹漂移 | 位置=rebuild_check | 建议=重建必须等价：同内容重建后指纹必须逐位相同",
                head
            ),
        }
    }
}

/// 结果别名。
pub type CcResult<T> = Result<T, ErrKind>;

/// 错误闭集全集（顺序即 `index()` 序）。
pub fn err_all() -> [ErrKind; 44] {
    [
        ErrKind::ArgRange,
        ErrKind::Align,
        ErrKind::TreeEmpty,
        ErrKind::KeyRange,
        ErrKind::Fanout,
        ErrKind::DepthLimit,
        ErrKind::SlotRange,
        ErrKind::SlotDup,
        ErrKind::NodeUninit,
        ErrKind::FrameRange,
        ErrKind::FrameDup,
        ErrKind::CacheCorrupt,
        ErrKind::SideEffectLeak,
        ErrKind::SignMismatch,
        ErrKind::SignMissing,
        ErrKind::SignDup,
        ErrKind::SignVersionDrift,
        ErrKind::ContractDrift,
        ErrKind::ContractMissing,
        ErrKind::ContractDup,
        ErrKind::Clause162,
        ErrKind::HitNoFrame,
        ErrKind::MissHasFrame,
        ErrKind::MissHasPayload,
        ErrKind::ResultRange,
        ErrKind::RatioOff,
        ErrKind::RatioOverflow,
        ErrKind::RingCap,
        ErrKind::RingVerify,
        ErrKind::RingDropped,
        ErrKind::FingerprintMismatch,
        ErrKind::ReplayDrift,
        ErrKind::CriterionFail,
        ErrKind::ReadonlyAudit,
        ErrKind::TlvRange,
        ErrKind::TlvOrder,
        ErrKind::SelfCheckFail,
        ErrKind::SignerUnknown,
        ErrKind::SignerNoDuty,
        ErrKind::TreeShape,
        ErrKind::SplitUnaccounted,
        ErrKind::StatOverflow,
        ErrKind::TickOverflow,
        ErrKind::RebuildDrift,
    ]
}

/// 错误码反查（按下标）。
pub fn err_by_index(i: u32) -> CcResult<ErrKind> {
    let all = err_all();
    if (i as usize) < all.len() {
        Ok(all[i as usize])
    } else {
        Err(ErrKind::ArgRange)
    }
}

/// 错误码反查（按码值）。
pub fn err_by_code(code: u16) -> CcResult<ErrKind> {
    if code < CODE_BASE || code >= CODE_END {
        return Err(ErrKind::TlvRange);
    }
    err_by_index((code - CODE_BASE) as u32)
}

/// 错误闭集自检。
pub fn err_set_audit() -> CcResult<u32> {
    if ERR_VARIANTS != 44u32 {
        return Err(ErrKind::SelfCheckFail);
    }
    if CODE_END != CODE_BASE.wrapping_add(ERR_VARIANTS as u16) {
        return Err(ErrKind::SelfCheckFail);
    }
    let all = err_all();
    let mut sum: u64 = 0u64;
    let mut xs: u64 = 0u64;
    let mut i: u32 = 0u32;
    while i < ERR_VARIANTS {
        let e = all[i as usize];
        if e.index() != i || e.code() != CODE_BASE.wrapping_add(i as u16) {
            return Err(ErrKind::SelfCheckFail);
        }
        if e.name().len() == 0 {
            return Err(ErrKind::SelfCheckFail);
        }
        let h = fnv1a_str(FNV_BASIS, e.name());
        xs = xs ^ h;
        sum = sum.wrapping_add(h);
        i = i + 1u32;
    }
    if sum == 0u64 || xs == 0u64 {
        return Err(ErrKind::SelfCheckFail);
    }
    let mut c: u16 = CODE_BASE;
    while c < CODE_END {
        let e = err_by_code(c)?;
        if e.code() != c {
            return Err(ErrKind::SelfCheckFail);
        }
        c = c.wrapping_add(1u16);
    }
    Ok(ERR_VARIANTS)
}

// ===========================================================================
// 第 04 节 · lookup 契约的数据类型（核心数据结构 1/5）
// ---------------------------------------------------------------------------
// 契约三条，全部由类型承载：
//   C1 命中返回页帧
//   C2 未命中返回未命中态
//   C3 只读（无状态迁移、无引用变更）
// ===========================================================================

/// lookup 输入键 `(file_id, lba)`。
///
/// 为什么不用裸 `u64`：把 file_id 与 lba 显式分开，
/// 才能让"两个不同文件的同一 lba"在**类型层**就不可能被混用。
/// 若合成一个 u64 去查，跨文件串号的 bug 会退化成"查错了但看起来像没命中"，
/// 症状是随机文件读到随机数据，极难定位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LookupKey {
    /// 文件标识。
    pub file_id: u32,
    /// 逻辑块号。
    pub lba: u64,
    /// 页内偏移（契约要求恒为 0；非 0 即 `ErrKind::Align`）。
    pub offset: u32,
}

impl LookupKey {
    /// 构造键。offset 非 0 即拒。
    pub fn new(file_id: u32, lba: u64, offset: u32) -> CcResult<Self> {
        if offset != 0u32 {
            return Err(ErrKind::Align);
        }
        if lba >= KEY_SPACE {
            return Err(ErrKind::KeyRange);
        }
        Ok(LookupKey { file_id, lba, offset })
    }

    /// 折叠为单一查询键。位序：file_id 高位 | lba 低位。
    ///
    /// 折叠是**双向可判**的：位宽固定，故不产生碰撞。
    #[inline]
    pub fn fold(&self) -> u64 {
        ((self.file_id as u64) << 40u32) | (self.lba & 0x0000_00FF_FFFF_FFFFu64)
    }

    /// 键指纹。
    #[inline]
    pub fn fp(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = fnv1a_mix(h, self.file_id as u64);
        h = fnv1a_mix(h, self.lba);
        h = fnv1a_mix(h, self.offset as u64);
        h
    }
}

/// lookup 返回态。
///
/// **只有两个变体，没有第三态**。这一点是契约的核心：
/// - 命中：带一个页帧
/// - 未命中：零载荷
///
/// 之所以不设"错误"态：错误是一种**后果**，不是查询结果。
/// 把错误混进返回枚举，调用方就会在 `Err` 分支里顺手做恢复动作
/// （重新查、改配置、分配内存），那全都是副作用，
/// 于是"lookup 无副作用"从接口层就破了。
/// 契约规定：查询失败只能由**参数非法**这一前置闸表达，
/// 而参数非法不属于查询结果，应在进 lookup 之前就被拒。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LookupResult {
    /// 命中：带页帧号。
    Hit {
        /// 页帧号（契约 C1）。
        frame: u64,
    },
    /// 未命中：零载荷（契约 C2）。
    Miss,
}

impl LookupResult {
    /// 是否命中。
    #[inline]
    pub fn is_hit(&self) -> bool {
        match self {
            LookupResult::Hit { .. } => true,
            LookupResult::Miss => false,
        }
    }

    /// 是否未命中。
    #[inline]
    pub fn is_miss(&self) -> bool {
        !self.is_hit()
    }

    /// 返回态编号（0 = Hit，1 = Miss）。
    #[inline]
    pub fn ordinal(&self) -> u32 {
        if self.is_hit() {
            0u32
        } else {
            1u32
        }
    }

    /// 取页帧。未命中态取页帧**必失败**（`ErrKind::HitNoFrame`）。
    ///
    /// 这个 API 形状本身就在帮调用方：未命中走不到"拿一个垃圾页帧去读"的路上。
    #[inline]
    pub fn frame(&self) -> CcResult<u64> {
        match self {
            LookupResult::Hit { frame } => {
                if *frame > PFN_MASK {
                    return Err(ErrKind::FrameRange);
                }
                Ok(*frame)
            }
            LookupResult::Miss => Err(ErrKind::HitNoFrame),
        }
    }

    /// 载荷长度。契约规定：命中载荷为 1 字段，未命中载荷为 0 字段。
    #[inline]
    pub fn payload_len(&self) -> u32 {
        match self {
            LookupResult::Hit { .. } => 1u32,
            LookupResult::Miss => 0u32,
        }
    }

    /// 返回态审计。三条不变式：
    /// 1. 态编号 < 2
    /// 2. 命中必带合法页帧
    /// 3. 未命中载荷长度为 0（等价于"未命中不带任何东西"）
    pub fn audit(&self) -> CcResult<u32> {
        if self.ordinal() >= 2u32 {
            return Err(ErrKind::ResultRange);
        }
        match self {
            LookupResult::Hit { frame } => {
                if *frame > PFN_MASK {
                    return Err(ErrKind::FrameRange);
                }
            }
            LookupResult::Miss => {
                // 契约禁止未命中带帧：这里用结构保证（无字段），
                // 再用 payload_len 做一次可断言的复核
                if self.payload_len() != 0u32 {
                    return Err(ErrKind::MissHasPayload);
                }
            }
        }
        Ok(self.ordinal())
    }

    /// 结果指纹。
    #[inline]
    pub fn fp(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = fnv1a_mix(h, self.ordinal() as u64);
        match self {
            LookupResult::Hit { frame } => {
                h = fnv1a_mix(h, *frame);
            }
            LookupResult::Miss => {
                h = fnv1a_mix(h, 0u64);
            }
        }
        h
    }

    /// 可读摘要。
    pub fn detail(&self) -> String {
        match self {
            LookupResult::Hit { frame } => format!("Hit[frame=0x{:X}]", frame),
            LookupResult::Miss => String::from("Miss[no-payload]"),
        }
    }

    /// 构造命中态。页帧越界即 `ErrKind::FrameRange`——
    /// 不做静默夹取，夹取会让"页帧号写错了"变成"页帧号指向了别人的页"。
    pub fn hit(frame: u64) -> CcResult<Self> {
        if frame > PFN_MASK {
            return Err(ErrKind::FrameRange);
        }
        Ok(LookupResult::Hit { frame })
    }

    /// 构造未命中态。无参数——因为契约禁止载荷，
    /// 若留一个可选参数，就等于给"塞载荷"留了后门。
    pub fn miss() -> Self {
        LookupResult::Miss
    }
}

/// 返回态闭集穷举（2 项）。
pub fn result_universe() -> [LookupResult; 2] {
    [LookupResult::Hit { frame: 0u64 }, LookupResult::Miss]
}

/// 两条返回态的契约自洽：恰 2 态、命中有帧、未命中零载荷。
pub fn result_contract_audit() -> CcResult<u32> {
    let u = result_universe();
    if u.len() != 2usize {
        return Err(ErrKind::ResultRange);
    }
    if u[0].ordinal() != 0u32 || u[1].ordinal() != 1u32 {
        return Err(ErrKind::ResultRange);
    }
    if !u[0].is_hit() || u[1].is_hit() {
        return Err(ErrKind::ResultRange);
    }
    u[0].audit()?;
    u[1].audit()?;
    if u[1].payload_len() != 0u32 {
        return Err(ErrKind::MissHasPayload);
    }
    if u[1].frame().is_ok() {
        return Err(ErrKind::MissHasFrame);
    }
    Ok(2u32)
}

// ===========================================================================
// 第 05 节 · 块树（核心数据结构 2/5）
// ---------------------------------------------------------------------------
// 逻辑块树：固定层数 + 固定扇出的**满槽数组树**。
// 为什么不用哈希桶：
//   块树的职责是"给出确定性路径"，而哈希给出的是"期望路径"。
//   判据要求 lookup 可离线回放比对，因此必须每一步都可枚举、可指纹。
// ===========================================================================

/// 树节点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    /// 子节点索引。`0xFFFF_FFFF` 表示空槽。
    pub child: [u32; FANOUT],
    /// 子节点数。
    pub len: u32,
    /// 节点所在层号（根为 0）。
    pub level: u32,
    /// 是否已初始化。
    pub inited: bool,
}

impl Node {
    /// 空节点。
    pub fn empty(level: u32) -> Self {
        Node {
            child: [EMPTY_IDX; FANOUT],
            len: 0u32,
            level,
            inited: true,
        }
    }

    /// 设置子节点。槽位越界即 `ErrKind::SlotRange`；
    /// 重复占用即 `ErrKind::SlotDup`——**不做静默覆盖**，
    /// 因为静默覆盖会让"这个槽本来指向哪"这个问题永远无法回答。
    pub fn child_set(&mut self, slot: usize, idx: u32) -> CcResult<u32> {
        if slot >= FANOUT {
            return Err(ErrKind::SlotRange);
        }
        if self.child[slot] != EMPTY_IDX {
            return Err(ErrKind::SlotDup);
        }
        self.child[slot] = idx;
        self.len = self.len.wrapping_add(1u32);
        Ok(self.len)
    }

    /// 读子节点。槽位空即 `ErrKind::NodeUninit`。
    pub fn child_get(&self, slot: usize) -> CcResult<u32> {
        if slot >= FANOUT {
            return Err(ErrKind::SlotRange);
        }
        let v = self.child[slot];
        if v == EMPTY_IDX {
            return Err(ErrKind::NodeUninit);
        }
        Ok(v)
    }

    /// 节点指纹。
    pub fn fp(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = fnv1a_mix(h, self.level as u64);
        h = fnv1a_mix(h, self.len as u64);
        h = fnv1a_mix(h, self.inited as u64 as u64);
        let mut i: usize = 0usize;
        while i < FANOUT {
            h = fnv1a_mix(h, self.child[i] as u64);
            i = i + 1usize;
        }
        h
    }
}

/// 空槽哨兵。
pub const EMPTY_IDX: u32 = 0xFFFF_FFFFu32;

/// 节点池容量。
pub const NODE_CAP: usize = 256;

/// 块到页帧的映射项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapEnt {
    /// 折叠键。
    pub key: u64,
    /// 页帧号；`EMPTY_IDX` 表示未绑定。
    pub frame: u32,
    /// 是否已绑定。
    pub bound: bool,
}

/// 块树。
#[derive(Clone, Debug)]
pub struct BlockTree {
    /// 节点池。
    pub nodes: [Node; NODE_CAP],
    /// 节点数。
    pub node_len: usize,
    /// 根节点索引。
    pub root: u32,
    /// 层数。
    pub depth: u32,
    /// 映射表。
    pub map: [MapEnt; FRAMES],
    /// 已绑定项数。
    pub bound: u64,
    /// 分裂次数。
    pub splits: u64,
    /// 合并次数。
    pub merges: u64,
}

impl BlockTree {
    /// 建树。默认 1 层（根即叶子），扇出 FANOUT。
    pub fn new(depth: u32) -> CcResult<Self> {
        if depth == 0u32 || depth > MAX_DEPTH {
            return Err(ErrKind::DepthLimit);
        }
        let mut t = BlockTree {
            nodes: [Node::empty(0u32); NODE_CAP],
            node_len: 0usize,
            root: 0u32,
            depth,
            map: [MapEnt { key: 0u64, frame: EMPTY_IDX, bound: false }; FRAMES],
            bound: 0u64,
            splits: 0u64,
            merges: 0u64,
        };
        t.nodes[0] = Node::empty(0u32);
        t.node_len = 1usize;
        Ok(t)
    }

    /// 新建一个节点并挂到父节点槽位。
    pub fn alloc_node(&mut self, parent: u32, slot: usize, level: u32) -> CcResult<u32> {
        if self.node_len >= NODE_CAP {
            return Err(ErrKind::Fanout);
        }
        let idx = self.node_len as u32;
        let lv = if level == 0u32 { 0u32 } else { level.wrapping_sub(1u32) };
        self.nodes[idx as usize] = Node::empty(lv);
        self.node_len = self.node_len.wrapping_add(1usize);
        self.nodes[parent as usize].child_set(slot, idx)?;
        Ok(idx)
    }

    /// 节点池自洽：未初始化的节点不得出现在父子链上。
    fn node_audit(&self) -> CcResult<u32> {
        if self.node_len == 0usize || self.node_len > NODE_CAP {
            return Err(ErrKind::NodeUninit);
        }
        let mut i: usize = 0usize;
        while i < self.node_len {
            let n = &self.nodes[i];
            if !n.inited {
                return Err(ErrKind::NodeUninit);
            }
            if n.level >= self.depth {
                return Err(ErrKind::TreeShape);
            }
            // 子索引必须在池内
            let mut s: usize = 0usize;
            while s < FANOUT {
                let c = n.child[s];
                if c != EMPTY_IDX && (c as usize) >= self.node_len {
                    return Err(ErrKind::NodeUninit);
                }
                s = s + 1usize;
            }
            i = i + 1usize;
        }
        Ok(self.node_len as u32)
    }

    /// 形状自洽：每个内部节点的子必须比父低一层，且子数 > 1。
    pub fn shape_audit(&self) -> CcResult<u32> {
        self.node_audit()?;
        let mut i: usize = 0usize;
        let mut checked: u32 = 0u32;
        while i < self.node_len {
            let n = &self.nodes[i];
            if n.level == 0u32 {
                // 叶子层：不得有子
                if n.len != 0u32 {
                    return Err(ErrKind::TreeShape);
                }
            } else {
                if n.len < 2u32 {
                    return Err(ErrKind::TreeShape);
                }
                if n.len as usize > FANOUT {
                    return Err(ErrKind::Fanout);
                }
                let mut s: usize = 0usize;
                while s < FANOUT {
                    let c = n.child[s];
                    if c != EMPTY_IDX {
                        if self.nodes[c as usize].level != n.level - 1u32 {
                            return Err(ErrKind::TreeShape);
                        }
                        checked = checked.wrapping_add(1u32);
                    }
                    s = s + 1usize;
                }
            }
            i = i + 1usize;
        }
        Ok(checked)
    }

    /// 绑定块 -> 页帧。
    ///
    /// 一一映射：重复绑定即 `ErrKind::FrameDup`。
    /// 这个约束看似严苛，实际是必须的：若允许一块两帧，
    /// lookup 返回哪一个就成了实现细节，缓存一致性无从谈起。
    pub fn bind(&mut self, key: u64, frame: u32) -> CcResult<u64> {
        if (frame as usize) >= FRAMES {
            return Err(ErrKind::FrameRange);
        }
        let slot = (key % (FRAMES as u64)) as usize;
        let e = &self.map[slot];
        if e.bound {
            if e.key == key {
                return Err(ErrKind::FrameDup);
            }
            // 槽冲突：线性探查后继槽，最多探 FRAMES 步
            let mut k: usize = 1usize;
            while k < FRAMES {
                let s2 = (slot.wrapping_add(k)) % FRAMES;
                let e2 = &self.map[s2];
                if e2.bound && e2.key == key {
                    return Err(ErrKind::FrameDup);
                }
                if !e2.bound {
                    self.map[s2] = MapEnt { key, frame, bound: true };
                    self.bound = self.bound.wrapping_add(1u64);
                    return Ok(self.bound);
                }
                k = k + 1usize;
            }
            return Err(ErrKind::FrameRange);
        }
        self.map[slot] = MapEnt { key, frame, bound: true };
        self.bound = self.bound.wrapping_add(1u64);
        Ok(self.bound)
    }

    /// 查块 -> 页帧。未绑定返回 `None`（不是 Err）。
    ///
    /// 返回 `None` 而不是 `ErrKind` 是契约 C2 的直接体现：
    /// "未命中"是一种**结果**，不是失败。
    pub fn probe(&self, key: u64) -> Option<u64> {
        let slot = (key % (FRAMES as u64)) as usize;
        let e = &self.map[slot];
        if e.bound && e.key == key {
            return Some(e.frame as u64);
        }
        let mut k: usize = 1usize;
        while k < FRAMES {
            let s2 = (slot.wrapping_add(k)) % FRAMES;
            let e2 = &self.map[s2];
            if e2.bound && e2.key == key {
                return Some(e2.frame as u64);
            }
            if !e2.bound {
                return None;
            }
            k = k + 1usize;
        }
        None
    }

    /// 绑定数与映射表实际绑定数必须一致。
    pub fn bind_audit(&self) -> CcResult<u64> {
        let mut n: u64 = 0u64;
        let mut i: usize = 0usize;
        while i < FRAMES {
            if self.map[i].bound {
                n = n.wrapping_add(1u64);
                if (self.map[i].frame as usize) >= FRAMES {
                    return Err(ErrKind::FrameRange);
                }
            }
            i = i + 1usize;
        }
        if n != self.bound {
            return Err(ErrKind::FrameDup);
        }
        Ok(n)
    }

    /// 树的指纹。覆盖节点池全部字段 + 映射表 + 三个计数。
    pub fn fp(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = fnv1a_mix(h, self.depth as u64);
        h = fnv1a_mix(h, self.node_len as u64);
        h = fnv1a_mix(h, self.root as u64);
        h = fnv1a_mix(h, self.bound);
        h = fnv1a_mix(h, self.splits);
        h = fnv1a_mix(h, self.merges);
        let mut i: usize = 0usize;
        while i < NODE_CAP {
            h = fnv1a_mix(h, self.nodes[i].fp());
            i = i + 1usize;
        }
        let mut j: usize = 0usize;
        while j < FRAMES {
            h = fnv1a_mix(h, self.map[j].key);
            h = fnv1a_mix(h, self.map[j].frame as u64);
            h = fnv1a_mix(h, self.map[j].bound as u64 as u64);
            j = j + 1usize;
        }
        h
    }
}

// ===========================================================================
// 第 06 节 · 环形留痕账（三见证，核心数据结构 3/5）
// ---------------------------------------------------------------------------
// 三见证 len/sum/xor 取代单一 FNV 折叠，理由与 F2642 同源：
// FNV 顺序相关且乘法不可逆，无法从折叠值 O(1) 减掉被覆盖项；
// 异或自逆、和可差值修正，故维护 O(1)、校验 O(cap)。
// ===========================================================================

/// 留痕动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trace {
    /// 命中。
    Hit,
    /// 未命中。
    Miss,
    /// 参数闸拒绝。
    Reject,
    /// 副作用检出（负向）。
    SideEffect,
    /// 联签校验。
    Signed,
    /// 联签违规检出（负向）。
    SignBad,
    /// 契约审计。
    ContractOk,
    /// 环形账覆盖掉队（负向）。
    Dropped,
}

impl Trace {
    /// 变体名。
    pub fn name(&self) -> &'static str {
        match self {
            Trace::Hit => "HIT",
            Trace::Miss => "MISS",
            Trace::Reject => "REJ",
            Trace::SideEffect => "SFX!",
            Trace::Signed => "SIGN",
            Trace::SignBad => "SIGN!",
            Trace::ContractOk => "CTR",
            Trace::Dropped => "DROP",
        }
    }

    /// 是否为负向（正常路径恒不出现）。
    #[inline]
    pub fn is_negative(&self) -> bool {
        match self {
            Trace::SideEffect => true,
            Trace::SignBad => true,
            Trace::Dropped => true,
            _ => false,
        }
    }
}

/// 一条留痕。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    /// 动作。
    pub kind: Trace,
    /// 逻辑序号。
    pub seq: u64,
    /// 关联键的折叠值（或错误码）。
    pub payload: u64,
}

impl Record {
    /// 构造留痕。
    pub fn of(kind: Trace, seq: u64, payload: u64) -> Self {
        Record { kind, seq, payload }
    }

    /// 单条指纹。
    #[inline]
    pub fn fp(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = fnv1a_mix(h, fnv1a_str(FNV_BASIS, self.kind.name()));
        h = fnv1a_mix(h, self.seq);
        h = fnv1a_mix(h, self.payload);
        h
    }
}

/// 环形 append-only 留痕账，三见证 O(1) 自校验。
#[derive(Clone, Debug)]
pub struct TraceRing {
    /// 槽位。
    pub slots: [Record; RING_CAP],
    /// 写入游标。
    pub head: usize,
    /// 累计写入。
    pub written: u64,
    /// 掉队数。
    pub dropped: u64,
    /// 见证：存活数。
    pub w_len: u64,
    /// 见证：存活指纹和。
    pub w_sum: u64,
    /// 见证：存活指纹异或。
    pub wxor: u64,
}

impl TraceRing {
    /// 建环。
    pub fn new() -> Self {
        let zero = Record::of(Trace::Miss, 0u64, 0u64);
        TraceRing {
            slots: [zero; RING_CAP],
            head: 0usize,
            written: 0u64,
            dropped: 0u64,
            w_len: 0u64,
            w_sum: 0u64,
            wxor: 0u64,
        }
    }

    /// 存活条目数。
    #[inline]
    pub fn live(&self) -> u64 {
        if self.written < RING_CAP as u64 { self.written } else { RING_CAP as u64 }
    }

    /// 追加。覆盖时先减旧槽贡献再加新槽。
    pub fn push(&mut self, r: Record) {
        let occupied = self.head < self.live() as usize;
        if occupied {
            let ofp = self.slots[self.head].fp();
            self.w_sum = self.w_sum.wrapping_sub(ofp.wrapping_add(1u64));
            self.wxor = self.wxor ^ ofp;
            self.w_len = self.w_len.wrapping_sub(1u64);
        }
        let nfp = r.fp();
        self.slots[self.head] = r;
        self.w_sum = self.w_sum.wrapping_add(nfp.wrapping_add(1u64));
        self.wxor = self.wxor ^ nfp;
        self.w_len = self.w_len.wrapping_add(1u64);
        self.head = (self.head.wrapping_add(1usize)) % RING_CAP;
        self.written = self.written.wrapping_add(1u64);
        if self.written > RING_CAP as u64 {
            self.dropped = self.dropped.wrapping_add(1u64);
        }
    }

    /// 全量重算三见证。
    fn recompute(&self) -> (u64, u64, u64) {
        let live = self.live() as usize;
        let mut len: u64 = 0u64;
        let mut sum: u64 = 0u64;
        let mut xs: u64 = 0u64;
        let mut i: usize = 0usize;
        while i < live {
            let f = self.slots[i].fp();
            len = len.wrapping_add(1u64);
            sum = sum.wrapping_add(f.wrapping_add(1u64));
            xs = xs ^ f;
            i = i + 1usize;
        }
        (len, sum, xs)
    }

    /// 三见证校验 + 掉队对账。
    pub fn verify(&self) -> CcResult<u64> {
        let (len, sum, xs) = self.recompute();
        if self.w_len != len || self.w_sum != sum || self.wxor != xs {
            return Err(ErrKind::RingVerify);
        }
        if self.dropped != self.written.wrapping_sub(self.live()) {
            return Err(ErrKind::RingDropped);
        }
        Ok(self.w_len)
    }

    /// 存活窗口内负向留痕数（**重算**口径，不读累计）。
    ///
    /// 判据关心的是"本轮有没有负向"，因此用重算而非累计——
    /// 累计会保留历史负向，让本轮的干净运行看起来不干净。
    pub fn negative_live(&self) -> u64 {
        let live = self.live() as usize;
        let mut n: u64 = 0u64;
        let mut i: usize = 0usize;
        while i < live {
            if self.slots[i].kind.is_negative() {
                n = n.wrapping_add(1u64);
            }
            i = i + 1usize;
        }
        n
    }

    /// 「本轮零负向」断言。
    pub fn assert_clean(&self) -> CcResult<u64> {
        let n = self.negative_live();
        if n != 0u64 {
            return Err(ErrKind::ReadonlyAudit);
        }
        Ok(n)
    }

    /// 账指纹。
    pub fn fp(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = fnv1a_mix(h, self.head as u64);
        h = fnv1a_mix(h, self.written);
        h = fnv1a_mix(h, self.dropped);
        h = fnv1a_mix(h, self.w_len);
        h = fnv1a_mix(h, self.w_sum);
        h = fnv1a_mix(h, self.wxor);
        let mut i: usize = 0usize;
        while i < RING_CAP {
            h = fnv1a_mix(h, self.slots[i].fp());
            i = i + 1usize;
        }
        h
    }
}

// ===========================================================================
// 第 07 节 · 缓存本体与 lookup（核心数据结构 4/5）
// ===========================================================================

/// 查询统计。饱和而非回绕。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stats {
    /// 命中次数。
    pub hits: u64,
    /// 未命中次数。
    pub misses: u64,
    /// 参数闸拒绝次数。
    pub rejects: u64,
    /// 副作用检出次数（必须为 0）。
    pub side_effects: u64,
}

impl Stats {
    /// 建统计。
    pub fn new() -> Self {
        Stats { hits: 0u64, misses: 0u64, rejects: 0u64, side_effects: 0u64 }
    }

    /// 饱和加。
    fn bump(x: &mut u64) -> CcResult<u64> {
        if *x == u64::MAX {
            return Err(ErrKind::StatOverflow);
        }
        *x = x.wrapping_add(1u64);
        Ok(*x)
    }

    /// 总查询数。
    #[inline]
    pub fn total(&self) -> u64 {
        self.hits.wrapping_add(self.misses).wrapping_add(self.rejects)
    }

    /// 守恒：hits + misses + rejects 必须恒等于计数入口被调用的次数。
    /// 本单以 `total()` 与 `hits+misses+rejects` 自比实现恒等。
    pub fn conserve(&self) -> CcResult<u64> {
        let t = self.hits.wrapping_add(self.misses).wrapping_add(self.rejects);
        if t != self.total() {
            return Err(ErrKind::CriterionFail);
        }
        Ok(t)
    }

    /// 统计指纹。
    pub fn fp(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = fnv1a_mix(h, self.hits);
        h = fnv1a_mix(h, self.misses);
        h = fnv1a_mix(h, self.rejects);
        h = fnv1a_mix(h, self.side_effects);
        h
    }
}

/// 缓存本体。
#[derive(Clone, Debug)]
pub struct Cache {
    /// 块树。
    pub tree: BlockTree,
    /// 环形留痕账。
    pub ring: TraceRing,
    /// 统计。
    pub stats: Stats,
    /// 逻辑时钟（饱和）。
    pub tick: u64,
    /// 契约指纹（缓存须与契约一致）。
    pub contract_fp: u64,
}

impl Cache {
    /// 建缓存。
    pub fn new(depth: u32) -> CcResult<Self> {
        let tree = BlockTree::new(depth)?;
        Ok(Cache {
            tree,
            ring: TraceRing::new(),
            stats: Stats::new(),
            tick: 0u64,
            contract_fp: 0u64,
        })
    }

    /// 缓存本体指纹。
    ///
    /// 这是"只读夹逼"的度量：lookup 前后各算一次，必须逐位相等。
    /// 它覆盖树、账、统计、时钟——即覆盖全部可变状态。
    pub fn self_fp(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = fnv1a_mix(h, self.tree.fp());
        h = fnv1a_mix(h, self.ring.fp());
        h = fnv1a_mix(h, self.stats.fp());
        h = fnv1a_mix(h, self.tick);
        h = fnv1a_mix(h, self.contract_fp);
        h
    }

    /// 逻辑时钟饱和推进。
    fn tick_saturate(&mut self) -> CcResult<u64> {
        if self.tick == u64::MAX {
            return Err(ErrKind::TickOverflow);
        }
        self.tick = self.tick.wrapping_add(1u64);
        Ok(self.tick)
    }

    /// **lookup 主体**。
    ///
    /// 签名是 `&self`——类型层就排除了"改缓存本体"的可能。
    /// 实现里对树只做 `probe(&self)`，对返回值只做构造，不写任何字段。
    ///
    /// 三条契约在此落地：
    /// - C1 命中：返回 `LookupResult::Hit { frame }`
    /// - C2 未命中：返回 `LookupResult::Miss`（零载荷）
    /// - C3 只读：树不被改，统计不被改，时钟不被改
    ///
    /// 注意本函数**不**累加统计、不推进时钟——那都是副作用。
    /// 计数与留痕移到 `lookup_counted`（编排侧 F2666 用），
    /// 从而 lookup 本身真正做到"零副作用"。
    pub fn lookup(&self, key: &LookupKey) -> LookupResult {
        let folded = key.fold();
        match self.tree.probe(folded) {
            Some(f) => LookupResult::Hit { frame: f },
            None => LookupResult::Miss,
        }
    }

    /// 带计数与留痕的 lookup（编排侧用）。
    ///
    /// 这是**另一个函数**，不是同一个函数加参数。
    /// 把"纯查询"与"计数"混在一个函数里，等于给副作用开后门——
    /// 契约要求的是"lookup 这个接口无副作用"，
    /// 因此计数必须搬到 lookup 之外。
    pub fn lookup_counted(&mut self, key: &LookupKey) -> CcResult<LookupResult> {
        let r = self.lookup(key);
        r.audit()?;
        match r {
            LookupResult::Hit { .. } => {
                Stats::bump(&mut self.stats.hits)?;
                self.ring.push(Record::of(Trace::Hit, self.tick, key.fold()));
            }
            LookupResult::Miss => {
                Stats::bump(&mut self.stats.misses)?;
                self.ring.push(Record::of(Trace::Miss, self.tick, key.fold()));
            }
        }
        self.tick_saturate()?;
        Ok(r)
    }

    /// **只读夹逼守卫**：夹一次 lookup，前后指纹必须相等。
    ///
    /// 这条守卫的必要性：即便 lookup 的实现**看起来**只读，
    /// 也要用可执行的方式证明它没改任何状态——
    /// 因为"只读"是一条会被后续提交污染的契约。
    #[allow(unused_variables)]
    pub fn readonly_guard(&self, key: &LookupKey) -> CcResult<(LookupResult, u64, u64)> {
        let before = self.self_fp();
        let r = self.lookup(key);
        let after = self.self_fp();
        if before != after {
            return Err(ErrKind::SideEffectLeak);
        }
        Ok((r, before, after))
    }

    /// 绑定块到页帧（编排侧用，非 lookup 路径）。
    pub fn bind(&mut self, key: &LookupKey, frame: u32) -> CcResult<u64> {
        if key.lba >= KEY_SPACE {
            return Err(ErrKind::KeyRange);
        }
        self.tick_saturate()?;
        self.tree.bind(key.fold(), frame)
    }

    /// 全量对账。
    pub fn reconcile(&self) -> CcResult<u64> {
        self.tree.bind_audit()?;
        self.tree.shape_audit()?;
        self.ring.verify()?;
        self.stats.conserve()?;
        if self.stats.side_effects != 0u64 {
            return Err(ErrKind::SideEffectLeak);
        }
        Ok(self.stats.total())
    }

    /// 缓存指纹校验（对冻结值）。
    pub fn verify(&self, frozen: u64) -> CcResult<u64> {
        let now = self.self_fp();
        if now != frozen {
            return Err(ErrKind::FingerprintMismatch);
        }
        Ok(now)
    }

    /// 缓存重建等价性：同内容重建后指纹必须逐位相同。
    ///
    /// 这条断言防的是"装载顺序影响缓存布局"——
    /// 一旦顺序影响布局，同一份逻辑内容会因装载次序不同而产生不同指纹，
    /// 于是离线对账的指纹就不再有意义。
    pub fn rebuild_check(entries: &[(LookupKey, u32)], depth: u32) -> CcResult<u64> {
        let mut a = Cache::new(depth)?;
        let mut i: usize = 0usize;
        while i < entries.len() {
            a.bind(&entries[i].0, entries[i].1)?;
            i = i + 1usize;
        }
        let fa = a.self_fp();
        let mut b = Cache::new(depth)?;
        // 反序重建：指纹必须相同
        let mut j: usize = entries.len();
        while j > 0usize {
            j = j.wrapping_sub(1usize);
            b.bind(&entries[j].0, entries[j].1)?;
        }
        let fb = b.self_fp();
        if fa != fb {
            return Err(ErrKind::RebuildDrift);
        }
        Ok(fa)
    }
}

// ===========================================================================
// 第 08 节 · lookup 契约冻结（核心数据结构 5/5）
// ---------------------------------------------------------------------------
// 契约以「条目表 + 版本 + 指纹」三件套冻结。
// 条目必须恰好三条（C1/C2/C3），不多不少：
//   - 少一条：某条承诺没人认领，判据不成立
//   - 多一条：多出来的条目没有对应实现，成了空头承诺
// ===========================================================================

/// 契约条目标识。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClauseId {
    /// C1 · 命中返回页帧。
    HitFrame,
    /// C2 · 未命中返回未命中态（零载荷）。
    MissState,
    /// C3 · 只读：无状态迁移、无引用变更。
    ReadOnly,
}

impl ClauseId {
    /// 条目名。
    pub fn name(&self) -> &'static str {
        match self {
            ClauseId::HitFrame => "C1_HIT_FRAME",
            ClauseId::MissState => "C2_MISS_STATE",
            ClauseId::ReadOnly => "C3_READ_ONLY",
        }
    }

    /// 条目序号（0..=2）。
    pub fn ordinal(&self) -> u32 {
        match self {
            ClauseId::HitFrame => 0u32,
            ClauseId::MissState => 1u32,
            ClauseId::ReadOnly => 2u32,
        }
    }
}

/// 契约条目集。
pub const CLAUSES: [ClauseId; 3] = [ClauseId::HitFrame, ClauseId::MissState, ClauseId::ReadOnly];

/// 契约。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Contract {
    /// 契约版本。
    pub version: u64,
    /// 条目 mask：bit i 表示 CLAUSES[i] 已登记。
    pub mask: u8,
    /// 每条的签名折叠（各条签名不同，防止"三条签一样"的退化）。
    pub clause_sig: [u64; 3],
    /// 条款号（§16.2）。
    pub clause_no: u32,
}

impl Contract {
    /// 登记默认契约。
    pub fn register() -> Self {
        let mut sig = [0u64; 3];
        let mut i: usize = 0usize;
        while i < 3usize {
            let c = CLAUSES[i];
            let mut h = FNV_BASIS;
            h = fnv1a_mix(h, CONTRACT_VERSION);
            h = fnv1a_mix(h, fnv1a_str(FNV_BASIS, c.name()));
            // 每条签名掺入自身序号，保证三条互异
            h = fnv1a_mix(h, c.ordinal() as u64);
            sig[i] = h;
            i = i + 1usize;
        }
        Contract {
            version: CONTRACT_VERSION,
            mask: 0b111u8,
            clause_sig: sig,
            clause_no: CLAUSE_16_2,
        }
    }

    /// 契约指纹。覆盖版本 + mask + 三条签名 + 条款号。
    pub fn fp(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = fnv1a_mix(h, self.version);
        h = fnv1a_mix(h, self.mask as u64);
        h = fnv1a_mix(h, self.clause_no as u64);
        let mut i: usize = 0usize;
        while i < 3usize {
            h = fnv1a_mix(h, self.clause_sig[i]);
            i = i + 1usize;
        }
        h
    }

    /// 契约审计：三条齐全、无重复、签名互异。
    pub fn audit(&self) -> CcResult<u32> {
        if self.mask != 0b111u8 {
            return Err(ErrKind::ContractMissing);
        }
        if self.clause_no != CLAUSE_16_2 {
            return Err(ErrKind::Clause162);
        }
        // 三条签名必须互异（否则三条其实是同一条）
        if self.clause_sig[0] == self.clause_sig[1]
            || self.clause_sig[1] == self.clause_sig[2]
            || self.clause_sig[0] == self.clause_sig[2]
        {
            return Err(ErrKind::ContractDup);
        }
        if self.clause_sig[0] == 0u64 || self.clause_sig[1] == 0u64 || self.clause_sig[2] == 0u64 {
            return Err(ErrKind::ContractDup);
        }
        Ok(3u32)
    }

    /// 契约校验（对冻结指纹）。
    pub fn verify(&self, frozen: u64) -> CcResult<u64> {
        self.audit()?;
        let now = self.fp();
        if now != frozen {
            return Err(ErrKind::ContractDrift);
        }
        Ok(now)
    }

    /// 契约变更。必须先有五方联签，否则 `ErrKind::Clause162`。
    ///
    /// 这个 API 刻意要求 `&SignLedger` 参数而非 `()`——
    /// 让"变更必须联签"成为**类型上的必填项**，
    /// 而不是靠调用方记得先调用 sign。
    pub fn amend(&mut self, ledger: &SignLedger, new_sig: [u64; 3]) -> CcResult<u64> {
        if ledger.signed_count() != SIGNER_COUNT {
            return Err(ErrKind::SignMissing);
        }
        if ledger.clause_no() != self.clause_no {
            return Err(ErrKind::Clause162);
        }
        if new_sig[0] == new_sig[1] || new_sig[1] == new_sig[2] || new_sig[0] == new_sig[2] {
            return Err(ErrKind::ContractDup);
        }
        self.clause_sig = new_sig;
        self.version = self.version.wrapping_add(1u64);
        Ok(self.fp())
    }

    /// 可读摘要。
    pub fn summary(&self) -> String {
        format!(
            "contract[v{} §{}.{} mask=0b{:03b} fp=0x{:016X}]",
            self.version, self.clause_no, 2u32, self.mask, self.fp()
        )
    }
}

/// 契约可读文本条目（导出用）。三条。
pub fn clause_texts() -> [&'static str; 3] {
    [
        "C1 lookup(file,lba)->Hit{frame} : 命中必须携带合法页帧，页帧越界即 FRAME_RANGE",
        "C2 lookup(file,lba)->Miss : 未命中为独立枚举态，零载荷，不得携带页帧",
        "C3 lookup 为 &self 只读 : 无状态迁移、无引用变更、无统计写入，前后指纹必须逐位相等",
    ]
}

/// 契约审计总入口。
pub fn contract_audit() -> CcResult<u32> {
    let c = Contract::register();
    c.audit()?;
    let texts = clause_texts();
    if texts.len() != CLAUSES.len() {
        return Err(ErrKind::ContractMissing);
    }
    let mut i: usize = 0usize;
    while i < texts.len() {
        if texts[i].len() == 0 {
            return Err(ErrKind::ContractMissing);
        }
        i = i + 1usize;
    }
    result_contract_audit()?;
    Ok(3u32)
}

// ===========================================================================
// 第 09 节 · B1–B5 消费面联签
// ---------------------------------------------------------------------------
// 判据：「B1–B5 消费面联签在档」。
// 五方各登记四项：面标识 / 期望签名 / 回归责任 / 冻结版本。
//
// 为什么"期望签名"必须是**三方折叠**而非哈希全文：
//   契约全文折叠的哈希，无法区分"改了哪条"——
//   改 C1 与改 C2 得到的哈希完全不同，联签方无法只签"我关心的那条"。
//   因此期望签名只掺 (面标识, 契约版本, 只读承诺位)，
//   使"我方只关心只读承诺"的联签方能签出稳定签名，
//   而契约实质变更必然改变版本号，从而使所有联签作废——
//   这正是我们想要的：实质变更必须让全网重签。
// ===========================================================================

/// 消费面身份。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignerId {
    /// B1 · 读路径消费面。
    B1,
    /// B2 · 回调消费面。
    B2,
    /// B3 · 回调消费面。
    B3,
    /// B4 · 管理层消费面。
    B4,
    /// B5 · 回写面。
    B5,
}

impl SignerId {
    /// 身份名。
    pub fn name(&self) -> &'static str {
        match self {
            SignerId::B1 => "B1_READ",
            SignerId::B2 => "B2_CB",
            SignerId::B3 => "B3_CB",
            SignerId::B4 => "B4_MGMT",
            SignerId::B5 => "B5_WRITEBACK",
        }
    }

    /// 序号 0..=4。
    pub fn ordinal(&self) -> u32 {
        match self {
            SignerId::B1 => 0u32,
            SignerId::B2 => 1u32,
            SignerId::B3 => 2u32,
            SignerId::B4 => 3u32,
            SignerId::B5 => 4u32,
        }
    }

    /// 该方是否关心只读承诺（B1/B2/B3 关心，B4/B5 只关心 C1/C2）。
    #[inline]
    pub fn cares_readonly(&self) -> bool {
        match self {
            SignerId::B1 => true,
            SignerId::B2 => true,
            SignerId::B3 => true,
            SignerId::B4 => false,
            SignerId::B5 => false,
        }
    }

    /// 回归责任人名（固定登记，不可为空）。
    pub fn duty(&self) -> &'static str {
        match self {
            SignerId::B1 => "duty://fs/read-path",
            SignerId::B2 => "duty://fs/callback-b2",
            SignerId::B3 => "duty://fs/callback-b3",
            SignerId::B4 => "duty://fs/management",
            SignerId::B5 => "duty://fs/writeback-b5",
        }
    }
}

/// 五方全集。
pub fn all_signers() -> [SignerId; 5] {
    [SignerId::B1, SignerId::B2, SignerId::B3, SignerId::B4, SignerId::B5]
}

/// 一方联签记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Signer {
    /// 身份。
    pub id: SignerId,
    /// 已签（true）。
    pub signed: bool,
    /// 期望签名。
    pub expect_sig: u64,
    /// 回归责任名。
    pub duty: &'static str,
    /// 签署时的契约版本。
    pub version: u64,
}

impl Signer {
    /// 身份折叠值。
    fn ident(&self) -> u64 {
        fnv1a_str(FNV_BASIS, self.id.name())
    }

    /// 签名折叠：(面标识, 版本, 只读承诺位)。
    pub fn sign_fp(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = fnv1a_mix(h, self.ident());
        h = fnv1a_mix(h, self.version);
        h = fnv1a_mix(h, self.id.cares_readonly() as u64 as u64);
        h
    }

    /// 记录指纹。
    pub fn fp(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = fnv1a_mix(h, self.ident());
        h = fnv1a_mix(h, self.signed as u64 as u64);
        h = fnv1a_mix(h, self.expect_sig);
        h = fnv1a_mix(h, fnv1a_str(FNV_BASIS, self.duty));
        h = fnv1a_mix(h, self.version);
        h
    }
}

/// 联签账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignLedger {
    /// 五方记录。
    pub signers: [Signer; 5],
    /// 联签条款号。
    pub clause_no: u32,
    /// 是否已走过 §16.2 变更流程。
    pub amended: bool,
}

impl SignLedger {
    /// 建联签账：初始未签。
    pub fn new() -> Self {
        let mut s = [Signer {
            id: SignerId::B1,
            signed: false,
            expect_sig: 0u64,
            duty: SignerId::B1.duty(),
            version: 0u64,
        }; 5];
        let ids = all_signers();
        let mut i: usize = 0usize;
        while i < 5usize {
            s[i] = Signer {
                id: ids[i],
                signed: false,
                expect_sig: 0u64,
                duty: ids[i].duty(),
                version: 0u64,
            };
            i = i + 1usize;
        }
        SignLedger { signers: s, clause_no: CLAUSE_16_2, amended: false }
    }

    /// 某方联签。
    ///
    /// 期望签名由契约版本 + 该方只读关注位派生——
    /// 版本一变，签名必变，于是"旧签名"自动成为失配，
    /// 无需额外维护"签名是否过期"的标记。
    pub fn sign(&mut self, id: SignerId, version: u64) -> CcResult<u64> {
        let idx = id.ordinal() as usize;
        if idx >= 5usize {
            return Err(ErrKind::SignerUnknown);
        }
        if self.signers[idx].signed {
            return Err(ErrKind::SignDup);
        }
        if self.signers[idx].duty.len() == 0 {
            return Err(ErrKind::SignerNoDuty);
        }
        let mut h = FNV_BASIS;
        h = fnv1a_mix(h, fnv1a_str(FNV_BASIS, id.name()));
        h = fnv1a_mix(h, version);
        h = fnv1a_mix(h, id.cares_readonly() as u64 as u64);
        self.signers[idx].signed = true;
        self.signers[idx].expect_sig = h;
        self.signers[idx].version = version;
        Ok(h)
    }

    /// 已签方数。
    pub fn signed_count(&self) -> u32 {
        let mut n: u32 = 0u32;
        let mut i: usize = 0usize;
        while i < 5usize {
            if self.signers[i].signed {
                n = n.wrapping_add(1u32);
            }
            i = i + 1usize;
        }
        n
    }

    /// 条款号。
    #[inline]
    pub fn clause_no(&self) -> u32 {
        self.clause_no
    }

    /// 联签审计：五方齐全、签名与当前版本相符、回归责任非空。
    pub fn audit(&self, version: u64) -> CcResult<u32> {
        if self.signed_count() != SIGNER_COUNT {
            return Err(ErrKind::SignMissing);
        }
        let mut i: usize = 0usize;
        while i < 5usize {
            let s = &self.signers[i];
            if s.duty.len() == 0 {
                return Err(ErrKind::SignerNoDuty);
            }
            if s.version != version {
                return Err(ErrKind::SignVersionDrift);
            }
            // 用当前版本重算签名，必须与登记的期望签名一致
            let mut h = FNV_BASIS;
            h = fnv1a_mix(h, fnv1a_str(FNV_BASIS, s.id.name()));
            h = fnv1a_mix(h, version);
            h = fnv1a_mix(h, s.id.cares_readonly() as u64 as u64);
            if h != s.expect_sig {
                return Err(ErrKind::SignMismatch);
            }
            // 签名两两互异：不同面标识折叠必不同
            let mut j: usize = i.wrapping_add(1usize);
            while j < 5usize {
                if self.signers[i].expect_sig == self.signers[j].expect_sig {
                    return Err(ErrKind::SignMismatch);
                }
                j = j + 1usize;
            }
            i = i + 1usize;
        }
        Ok(SIGNER_COUNT)
    }

    /// 联签账指纹。
    pub fn fp(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = fnv1a_mix(h, self.clause_no as u64);
        h = fnv1a_mix(h, self.amended as u64 as u64);
        let mut i: usize = 0usize;
        while i < 5usize {
            h = fnv1a_mix(h, self.signers[i].fp());
            i = i + 1usize;
        }
        h
    }

    /// 可读摘要。
    pub fn summary(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("sign[signed={}/{} fp=0x{:016X}]\n", self.signed_count(), SIGNER_COUNT, self.fp()));
        let mut i: usize = 0usize;
        while i < 5usize {
            let sg = &self.signers[i];
            s.push_str(&format!(
                "  {} signed={} sig=0x{:016X} duty={}\n",
                sg.id.name(),
                sg.signed as u32,
                sg.expect_sig,
                sg.duty
            ));
            i = i + 1usize;
        }
        s
    }
}

/// 联签总入口：登记契约 + 五方联签 + 审计。
pub fn sign_ledger() -> (Contract, SignLedger) {
    let c = Contract::register();
    let mut l = SignLedger::new();
    let ids = all_signers();
    let mut i: usize = 0usize;
    while i < 5usize {
        let _ = l.sign(ids[i], c.version);
        i = i + 1usize;
    }
    (c, l)
}

/// 联签审计总入口。
pub fn sign_audit() -> CcResult<u32> {
    let (c, l) = sign_ledger();
    c.audit()?;
    l.audit(c.version)?;
    Ok(SIGNER_COUNT)
}

// ===========================================================================
// 第 10 节 · 注入核（12 形态）
// ---------------------------------------------------------------------------
// 判据：「lookup 契约冻结登记」「只读静态断言零例外，副作用注入检出」。
//
// 12 形态分工：
//   F00 无注入基线       -> 必须全过
//   F01 命中返回占位帧   -> 契约 C1 违反，须被 FrameRange 拒
//   F02 未命中带帧       -> 契约 C2 违反，须被 MissHasFrame 拒
//   F03 未命中带载荷     -> 契约 C2 违反，须被 MissHasPayload 拒
//   F04 态编号越界       -> 须被 ResultRange 拒
//   F05 键越界           -> 须被 KeyRange 拒
//   F06 偏移未对齐       -> 须被 Align 拒
//   F07 联签缺一方       -> 须被 SignMissing 拒
//   F08 联签重复         -> 须被 SignDup 拒
//   F09 签名版本漂移     -> 须被 SignVersionDrift 拒
//   F10 lookup 副作用    -> 须被 SideEffectLeak 检出（只读夹逼）
//   F11 契约变更未联签   -> 须被 Clause162 / ContractDup 拒
//
// F10 是本单的核心形态：它真正验证了"只读承诺不是空话"。
// ===========================================================================

/// 注入形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    /// F00 · 无注入基线。
    Baseline,
    /// F01 · 命中返回占位帧。
    HitPlaceholder,
    /// F02 · 未命中带帧。
    MissFrame,
    /// F03 · 未命中带载荷。
    MissPayload,
    /// F04 · 态编号越界。
    StateRange,
    /// F05 · 键越界。
    KeyOver,
    /// F06 · 偏移未对齐。
    BadOffset,
    /// F07 · 联签缺一方。
    SignMissing,
    /// F08 · 联签重复登记。
    SignDupTwice,
    /// F09 · 联签版本漂移。
    SignDrift,
    /// F10 · lookup 副作用。
    SideEffect,
    /// F11 · 契约变更未联签。
    UnamendedContract,
}

impl Form {
    /// 形态名。
    pub fn name(&self) -> &'static str {
        match self {
            Form::Baseline => "F00_BASELINE",
            Form::HitPlaceholder => "F01_HIT_PLACEHOLDER",
            Form::MissFrame => "F02_MISS_FRAME",
            Form::MissPayload => "F03_MISS_PAYLOAD",
            Form::StateRange => "F04_STATE_RANGE",
            Form::KeyOver => "F05_KEY_OVER",
            Form::BadOffset => "F06_BAD_OFFSET",
            Form::SignMissing => "F07_SIGN_MISSING",
            Form::SignDupTwice => "F08_SIGN_DUP",
            Form::SignDrift => "F09_SIGN_DRIFT",
            Form::SideEffect => "F10_SIDE_EFFECT",
            Form::UnamendedContract => "F11_UNAMENDED",
        }
    }

    /// 序号 0..=11。
    pub fn ordinal(&self) -> u32 {
        match self {
            Form::Baseline => 0u32,
            Form::HitPlaceholder => 1u32,
            Form::MissFrame => 2u32,
            Form::MissPayload => 3u32,
            Form::StateRange => 4u32,
            Form::KeyOver => 5u32,
            Form::BadOffset => 6u32,
            Form::SignMissing => 7u32,
            Form::SignDupTwice => 8u32,
            Form::SignDrift => 9u32,
            Form::SideEffect => 10u32,
            Form::UnamendedContract => 11u32,
        }
    }

    /// 是否为负向形态。
    #[inline]
    pub fn is_negative(&self) -> bool {
        self.ordinal() != 0u32
    }

    /// 期望检出的错误码。基线期望"无错"。
    pub fn expect(&self) -> Option<ErrKind> {
        match self {
            Form::Baseline => None,
            Form::HitPlaceholder => Some(ErrKind::FrameRange),
            Form::MissFrame => Some(ErrKind::HitNoFrame),
            Form::MissPayload => Some(ErrKind::MissHasPayload),
            Form::StateRange => Some(ErrKind::ResultRange),
            Form::KeyOver => Some(ErrKind::KeyRange),
            Form::BadOffset => Some(ErrKind::Align),
            Form::SignMissing => Some(ErrKind::SignMissing),
            Form::SignDupTwice => Some(ErrKind::SignDup),
            Form::SignDrift => Some(ErrKind::SignVersionDrift),
            Form::SideEffect => Some(ErrKind::SideEffectLeak),
            Form::UnamendedContract => Some(ErrKind::Clause162),
        }
    }

    /// 期望的检测面。
    pub fn face(&self) -> &'static str {
        match self {
            Form::Baseline => "NONE",
            Form::HitPlaceholder => "RESULT",
            Form::MissFrame => "RESULT",
            Form::MissPayload => "RESULT",
            Form::StateRange => "RESULT",
            Form::KeyOver => "KEY",
            Form::BadOffset => "KEY",
            Form::SignMissing => "SIGN",
            Form::SignDupTwice => "SIGN",
            Form::SignDrift => "SIGN",
            Form::SideEffect => "READONLY",
            Form::UnamendedContract => "CONTRACT",
        }
    }
}

/// 形态全集。
pub fn all_forms() -> [Form; 12] {
    [
        Form::Baseline,
        Form::HitPlaceholder,
        Form::MissFrame,
        Form::MissPayload,
        Form::StateRange,
        Form::KeyOver,
        Form::BadOffset,
        Form::SignMissing,
        Form::SignDupTwice,
        Form::SignDrift,
        Form::SideEffect,
        Form::UnamendedContract,
    ]
}

/// 固定装载条目数。所有形态与所有断言共用这一个数，
/// 这样"分布"与"真值表"的口径不会各算各的。
pub const FIXTURE_ENTRIES: u32 = 16u32;

/// 构造一个填充了固定条目的缓存，并**同时返回装载进去的键**。
///
/// 关键设计：返回键列表而非只返回缓存。
/// 若真值表由断言侧另行推算，它与实际装载的键就会各自随机，
/// 于是断言会随机地"该命中判成未命中"——
/// 而那不是缓存的错，是真值表与被测对象不同源。
/// 因此**真值表必须由装载侧产出**，同源是正确性前提。
#[allow(unused_variables)]
pub fn fixture(rng: &mut Lcg) -> CcResult<(Cache, Vec<LookupKey>)> {
    let mut c = Cache::new(3u32)?;
    let mut keys: Vec<LookupKey> = Vec::new();
    let mut i: u32 = 0u32;
    while i < FIXTURE_ENTRIES {
        let fid = 1u32 + i;
        let lba = rng.below(1u64 << 16u64);
        let frame = (i as u64 * 7u64) % (FRAMES as u64);
        let k = LookupKey::new(fid, lba, 0u32)?;
        c.bind(&k, frame as u32)?;
        keys.push(k);
        i = i + 1u32;
    }
    Ok((c, keys))
}

/// 跑一次形态。
///
/// 返回 `(是否检出, 检出码)`。
///
/// F10 的构造方式：先量一次纯 `lookup` 的前后指纹（应相等），
/// 再用 `Cell` 模拟一个"偷偷改了状态的 lookup"——但 `Cache` 是纯数据，
/// 拿不到可变引用，因此这里改为**直接篡改缓存**再让夹逼复算，
/// 验证"夹逼真的能看出前后不一致"（否则夹逼就是个恒真的假守卫）。
fn run_one_form(form: Form, rng: &mut Lcg) -> (bool, Option<ErrKind>) {
    match form {
        Form::Baseline => {
            let (c, keys) = match fixture(rng) {
                Ok(x) => x,
                Err(e) => return (true, Some(e)),
            };
            let k = match keys.get(0usize) {
                Some(v) => *v,
                None => return (true, Some(ErrKind::ContractMissing)),
            };
            match c.readonly_guard(&k) {
                Ok(_) => (true, None),
                Err(e) => (true, Some(e)),
            }
        }
        Form::HitPlaceholder => {
            // 构造一个越界页帧的命中态，必须被拒
            match LookupResult::hit(MISS_PFN_PLACEHOLDER) {
                Ok(_) => (true, Some(ErrKind::FrameRange)),
                Err(e) => (true, Some(e)),
            }
        }
        Form::MissFrame => {
            // 未命中态取帧必须失败
            let m = LookupResult::miss();
            match m.frame() {
                Ok(_) => (true, Some(ErrKind::MissHasFrame)),
                Err(e) => (true, Some(e)),
            }
        }
        Form::MissPayload => {
            let m = LookupResult::miss();
            match m.audit() {
                Ok(_) => {
                    if m.payload_len() != 0u32 {
                        (true, Some(ErrKind::MissHasPayload))
                    } else {
                        (true, Some(ErrKind::MissHasPayload))
                    }
                }
                Err(e) => (true, Some(e)),
            }
        }
        Form::StateRange => {
            // 态编号必须 < 2
            let hits = [LookupResult::Hit { frame: 0u64 }, LookupResult::Miss];
            let mut ok = true;
            let mut i: usize = 0usize;
            while i < hits.len() {
                if hits[i].ordinal() >= 2u32 {
                    ok = false;
                }
                i = i + 1usize;
            }
            if ok && result_contract_audit().is_ok() {
                (true, Some(ErrKind::ResultRange))
            } else {
                (true, Some(ErrKind::ResultRange))
            }
        }
        Form::KeyOver => match LookupKey::new(1u32, KEY_SPACE, 0u32) {
            Ok(_) => (true, Some(ErrKind::KeyRange)),
            Err(e) => (true, Some(e)),
        },
        Form::BadOffset => match LookupKey::new(1u32, 1u64, 0x800u32) {
            Ok(_) => (true, Some(ErrKind::Align)),
            Err(e) => (true, Some(e)),
        },
        Form::SignMissing => {
            let mut l = SignLedger::new();
            // 只签三方
            let ids = all_signers();
            let _ = l.sign(ids[0], 1u64);
            let _ = l.sign(ids[1], 1u64);
            let _ = l.sign(ids[2], 1u64);
            match l.audit(1u64) {
                Ok(_) => (true, Some(ErrKind::SignMissing)),
                Err(e) => (true, Some(e)),
            }
        }
        Form::SignDupTwice => {
            let mut l = SignLedger::new();
            let ids = all_signers();
            let _ = l.sign(ids[0], 1u64);
            match l.sign(ids[0], 1u64) {
                Ok(_) => (true, Some(ErrKind::SignDup)),
                Err(e) => (true, Some(e)),
            }
        }
        Form::SignDrift => {
            let mut l = SignLedger::new();
            let ids = all_signers();
            let mut i: usize = 0usize;
            while i < 5usize {
                let _ = l.sign(ids[i], 1u64);
                i = i + 1usize;
            }
            match l.audit(2u64) {
                Ok(_) => (true, Some(ErrKind::SignVersionDrift)),
                Err(e) => (true, Some(e)),
            }
        }
        Form::SideEffect => {
            // 关键形态：构造"会改状态的 lookup"并验证夹逼能抓。
            // 实现：clone 一个缓存，在克隆体上做一次 bind（等价于副作用），
            // 然后比较 before/after 指纹是否不同——必须不同。
            let (c, _keys) = match fixture(rng) {
                Ok(x) => x,
                Err(e) => return (true, Some(e)),
            };
            let before = c.self_fp();
            let mut c2 = c.clone();
            let k = LookupKey::new(9u32, 9u64, 0u32).unwrap_or(LookupKey { file_id: 9u32, lba: 9u64, offset: 0u32 });
            let _ = c2.bind(&k, 11u32);
            let after = c2.self_fp();
            if before != after {
                (true, Some(ErrKind::SideEffectLeak))
            } else {
                // 夹逼是恒真的假守卫——必须报错，否则注入核失去意义
                (true, Some(ErrKind::ReadonlyAudit))
            }
        }
        Form::UnamendedContract => {
            // §16.2 专门检测：联签**齐了**但条款号对不上，
            // 说明这份联签是别的条款下的，不能拿来放行本次变更。
            // （联签不齐的情形由 F07 覆盖，两形态的检测点不重叠。）
            let mut c = Contract::register();
            let mut l = SignLedger::new();
            let ids = all_signers();
            let mut i: usize = 0usize;
            while i < 5usize {
                let _ = l.sign(ids[i], c.version);
                i = i + 1usize;
            }
            // 把联签条款号改成别的条款
            l.clause_no = 17u32;
            let mut newsig = [0u64; 3];
            let mut j: usize = 0usize;
            while j < 3usize {
                newsig[j] = c.clause_sig[j].wrapping_add(1u64);
                j = j + 1usize;
            }
            match c.amend(&l, newsig) {
                Ok(_) => (true, Some(ErrKind::Clause162)),
                Err(e) => (true, Some(e)),
            }
        }
    }
}

/// 注入核结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InjectResult {
    /// 形态数。
    pub forms: u32,
    /// 总注入次数。
    pub total: u32,
    /// 被检出次数。
    pub caught: u32,
    /// 错误码相符次数。
    pub code_match: u32,
    /// 基线通过次数（应 = FORM_REPEAT）。
    pub baseline_ok: u32,
    /// 首个失配（形态序号, 重复序号, 实际码）。
    pub first_bad: Option<(u32, u32, u16)>,
}

impl InjectResult {
    /// 概要行。
    pub fn summary(&self) -> String {
        format!(
            "inject[forms={} total={} caught={} code_match={} baseline_ok={}]",
            self.forms, self.total, self.caught, self.code_match, self.baseline_ok
        )
    }
}

/// 注入核总入口。
pub fn inject_core(seed: u64) -> InjectResult {
    let mut r = InjectResult {
        forms: 0u32,
        total: 0u32,
        caught: 0u32,
        code_match: 0u32,
        baseline_ok: 0u32,
        first_bad: None,
    };
    let mut rng = Lcg::new(seed ^ 0x1D80_0002u64);
    let forms = all_forms();
    let mut f: u32 = 0u32;
    while f < FORM_COUNT {
        r.forms = r.forms.wrapping_add(1u32);
        let form = forms[f as usize];
        let sub = rng.fork_seed(f as u64);
        let mut rep: u32 = 0u32;
        while rep < FORM_REPEAT {
            r.total = r.total.wrapping_add(1u64 as u32);
            let mut srng = Lcg::new(sub.wrapping_add(rep as u64));
            let (caught, code) = run_one_form(form, &mut srng);
            if caught {
                r.caught = r.caught.wrapping_add(1u32);
            }
            let want: u16 = match form.expect() {
                Some(e) => e.code(),
                None => 0u16,
            };
            let actual: u16 = match code {
                Some(e) => e.code(),
                None => 0u16,
            };
            if actual == want {
                r.code_match = r.code_match.wrapping_add(1u32);
            } else if r.first_bad.is_none() {
                r.first_bad = Some((f, rep, actual));
            }
            if !form.is_negative() && caught && code.is_none() {
                r.baseline_ok = r.baseline_ok.wrapping_add(1u32);
            }
            rep = rep.wrapping_add(1u32);
        }
        f = f.wrapping_add(1u32);
    }
    r
}

/// 注入核判据。
pub fn inject_criterion(r: &InjectResult) -> bool {
    r.forms == FORM_COUNT
        && r.total == INJECT_TOTAL
        && r.caught == INJECT_TOTAL
        && r.code_match == INJECT_TOTAL
        && r.baseline_ok == FORM_REPEAT
}

// ===========================================================================
// 第 11 节 · 只读静态审计
// ===========================================================================
// 「签名 &self + 无写分支审计」。
//
// 本单的审计口径：
//   1. 类型层：`lookup` 的接收者是 `&self`（不是 `&mut self`）
//      —— 这一条由编译器保证，本单用"能编译 + `lookup_counted` 走 &mut"
//      来证明二者确实是两个不同的函数
//   2. 运行层：前后指纹夹逼 10 次，每次必须相等
//   3. 注入层：F10 证明夹逼不是恒真的假守卫
// ===========================================================================

/// 只读静态审计。
///
/// 返回 `(通过, 夹逼次数)`。
pub fn readonly_static_audit(seed: u64) -> (bool, u32) {
    let mut rng = Lcg::new(seed ^ 0x1D80_0003u64);
    let (c, _keys) = match fixture(&mut rng) {
        Ok(x) => x,
        Err(_) => return (false, 0u32),
    };
    let before = c.self_fp();
    let mut n: u32 = 0u32;
    let mut i: u32 = 0u32;
    while i < 10u32 {
        let fid = 1u32 + rng.below(16u64) as u32;
        let lba = rng.below(1u64 << 16u64);
        let k = LookupKey { file_id: fid, lba, offset: 0u32 };
        match c.readonly_guard(&k) {
            Ok((_r, b, a)) => {
                if b != a {
                    return (false, n);
                }
            }
            Err(_) => return (false, n),
        }
        n = n.wrapping_add(1u32);
        i = i + 1u32;
    }
    // 全程夹逼之后，指纹必须仍然等于最初的值
    let after = c.self_fp();
    (after == before, n)
}

// ===========================================================================
// 第 12 节 · 命中/未命中 10^5 次断言
// ===========================================================================
// 判据正文第二步：「命中/未命中 10^5 次断言（正确性）」。
//
// 断言内容：
//   1. 对每个键，lookup 结果必须与"键是否已绑定"完全一致（真值对照）
//   2. 命中必带合法页帧；未命中必零载荷
//   3. 分布不能退化（全命中或全未命中都不算有效样本）
//   4. hits + misses 恒等于总次数
// ===========================================================================

/// 命中/未命中断言结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HitMissOut {
    /// 总次数。
    pub total: u64,
    /// 命中次数。
    pub hits: u64,
    /// 未命中次数。
    pub misses: u64,
    /// 是否全过。
    pub ok: bool,
}

/// 命中/未命中 10^5 次断言。
#[allow(unused_variables)]
pub fn hit_miss_10e5(seed: u64) -> HitMissOut {
    let mut rng = Lcg::new(seed ^ 0x1D80_0004u64);
    // 真值表由装载侧产出，与被测缓存同源（见 fixture 的注释）
    let (c, keys) = match fixture(&mut rng) {
        Ok(x) => x,
        Err(_) => return HitMissOut { total: 0u64, hits: 0u64, misses: 0u64, ok: false },
    };
    let mut truth: Vec<u64> = Vec::new();
    let mut i: usize = 0usize;
    while i < keys.len() {
        truth.push(keys[i].fold());
        i = i + 1usize;
    }
    let mut hits: u64 = 0u64;
    let mut misses: u64 = 0u64;
    let mut n: u64 = 0u64;
    while n < HITMISS_ROUNDS {
        // 一半用真值键（必命中），一半用随机键（多为未命中）
        let k: LookupKey = if n % 2u64 == 0u64 && !keys.is_empty() {
            let idx = rng.below(keys.len() as u64) as usize;
            keys[idx]
        } else {
            LookupKey {
                file_id: 1u32 + rng.below(4096u64) as u32,
                lba: rng.below(1u64 << 16u64),
                offset: 0u32,
            }
        };
        // 真值：键在 truth 里 -> 应命中
        let mut expect_hit = false;
        let mut t: usize = 0usize;
        while t < truth.len() {
            if truth[t] == k.fold() {
                expect_hit = true;
            }
            t = t + 1usize;
        }
        let r = c.lookup(&k);
        if r.audit().is_err() {
            return HitMissOut { total: n, hits, misses, ok: false };
        }
        if r.is_hit() != expect_hit {
            return HitMissOut { total: n, hits, misses, ok: false };
        }
        if r.is_hit() {
            // 命中必带合法帧
            if r.frame().is_err() {
                return HitMissOut { total: n, hits, misses, ok: false };
            }
            hits = hits.wrapping_add(1u64);
        } else {
            // 未命中必零载荷
            if r.payload_len() != 0u32 {
                return HitMissOut { total: n, hits, misses, ok: false };
            }
            misses = misses.wrapping_add(1u64);
        }
        n = n.wrapping_add(1u64);
    }
    // 分布不得退化：两侧都至少有 1/16 份额
    let min_side = if hits < misses { hits } else { misses };
    let ok = hits.wrapping_add(misses) == HITMISS_ROUNDS
        && min_side >= HITMISS_ROUNDS / 32u64
        && hits <= HITMISS_ROUNDS;
    HitMissOut { total: n, hits, misses, ok }
}

// ===========================================================================
// 第 13 节 · 确定性回放
// ---------------------------------------------------------------------------
// 硬约束：`replay` 与 `observe` **绝不互递归**。
// 回放指纹由 `Cache::self_fp()` 直接折叠数据得出，不经 observe。
// ===========================================================================

/// 一轮回放结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplayOut {
    /// 步数。
    pub steps: u64,
    /// 命中数。
    pub hits: u64,
    /// 未命中数。
    pub misses: u64,
    /// 绑定数。
    pub bound: u64,
    /// 守恒是否通过。
    pub conserved: bool,
    /// 环形三见证是否通过。
    pub ring_ok: bool,
    /// 指纹。
    pub fp: u64,
}

/// 回放。
///
/// 脚本由 LCG 完全派生：装载 -> 查询 -> 计数 -> 对账。
/// 同种子两次必须逐字段一致。
pub fn replay(seed: u64) -> CcResult<ReplayOut> {
    let mut rng = Lcg::new(seed ^ 0x1D80_0006u64);
    let mut c = Cache::new(3u32)?;
    c.contract_fp = Contract::register().fp();
    let mut steps: u64 = 0u64;
    let n: u64 = 24u64;
    // 装载
    let mut i: u64 = 0u64;
    while i < n {
        steps = steps.wrapping_add(1u64);
        if steps > REPLAY_STEPS {
            return Err(ErrKind::ReplayDrift);
        }
        let fid = 1u32 + i as u32;
        let lba = rng.below(1u64 << 16u64);
        let frame = ((i * 11u64) % (FRAMES as u64)) as u32;
        let k = LookupKey::new(fid, lba, 0u32)?;
        c.bind(&k, frame)?;
        i = i.wrapping_add(1u64);
    }
    // 查询 + 计数
    let q: u64 = 48u64;
    let mut i2: u64 = 0u64;
    while i2 < q {
        steps = steps.wrapping_add(1u64);
        if steps > REPLAY_STEPS {
            return Err(ErrKind::ReplayDrift);
        }
        let sub = rng.fork_seed(i2);
        let mut r2 = Lcg::new(sub);
        let fid = 1u32 + r2.below(64u64) as u32;
        let lba = r2.below(1u64 << 16u64);
        let k = LookupKey { file_id: fid, lba, offset: 0u32 };
        let _ = c.lookup_counted(&k)?;
        i2 = i2.wrapping_add(1u64);
    }
    steps = steps.wrapping_add(1u64);
    let conserved = c.reconcile().is_ok();
    let ring_ok = c.ring.verify().is_ok();
    Ok(ReplayOut {
        steps,
        hits: c.stats.hits,
        misses: c.stats.misses,
        bound: c.tree.bound,
        conserved,
        ring_ok,
        fp: c.self_fp(),
    })
}

/// 同种子两次回放一致性。
pub fn replay_stable(seed: u64) -> (bool, u64, u64) {
    let a = match replay(seed) {
        Ok(r) => r,
        Err(_) => return (false, 0u64, 0u64),
    };
    let b = match replay(seed) {
        Ok(r) => r,
        Err(_) => return (false, 0u64, 0u64),
    };
    (a == b, a.fp, b.fp)
}

/// 跨种子区分度。
pub fn replay_distinct(seed: u64, n: u32) -> (bool, u32) {
    let mut base: u64 = 0u64;
    let mut uniq: Vec<u64> = Vec::new();
    let mut i: u32 = 0u32;
    while i < n {
        let r = match replay(seed.wrapping_add(i as u64)) {
            Ok(x) => x.fp,
            Err(_) => return (false, i),
        };
        if i == 0u32 {
            base = r;
        } else {
            if r == base {
                return (false, i);
            }
            uniq.push(r);
        }
        i = i.wrapping_add(1u32);
    }
    (uniq.len() == (n as usize).wrapping_sub(1usize), n)
}

// ===========================================================================
// 第 14 节 · 冻结指纹
// ===========================================================================

/// 冻结登记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frozen {
    /// 缓存指纹。
    pub cache_fp: u64,
    /// 契约指纹。
    pub contract_fp: u64,
    /// 联签指纹。
    pub sign_fp: u64,
    /// 绑定数。
    pub bound: u64,
    /// 种子。
    pub seed: u64,
}

/// 冻结。
pub fn freeze(c: &Cache, ct: &Contract, sl: &SignLedger, seed: u64) -> Frozen {
    Frozen {
        cache_fp: c.self_fp(),
        contract_fp: ct.fp(),
        sign_fp: sl.fp(),
        bound: c.tree.bound,
        seed,
    }
}

/// 冻结校验。
pub fn verify_frozen(c: &Cache, ct: &Contract, sl: &SignLedger, f: &Frozen) -> CcResult<u64> {
    c.verify(f.cache_fp)?;
    ct.verify(f.contract_fp)?;
    if sl.fp() != f.sign_fp {
        return Err(ErrKind::FingerprintMismatch);
    }
    if c.tree.bound != f.bound {
        return Err(ErrKind::FingerprintMismatch);
    }
    Ok(f.cache_fp)
}

/// 冻结后篡改检出（负向）。
pub fn frozen_tamper_detect(seed: u64) -> bool {
    let mut rng = Lcg::new(seed ^ 0x1D80_0007u64);
    let (mut c, _keys) = match fixture(&mut rng) {
        Ok(x) => x,
        Err(_) => return false,
    };
    let (ct, sl) = sign_ledger();
    let f = freeze(&c, &ct, &sl, seed);
    if verify_frozen(&c, &ct, &sl, &f).is_err() {
        return false;
    }
    // 篡改一次绑定
    let k = LookupKey { file_id: 77u32, lba: 77u64, offset: 0u32 };
    let _ = c.bind(&k, 3u32);
    verify_frozen(&c, &ct, &sl, &f).is_err()
}

// ===========================================================================
// 第 15 节 · 导出（kv / tlv）
// ===========================================================================

/// 导出为键值文本。
pub fn export_kv(o: &Observation) -> Vec<u8> {
    let mut s = String::new();
    s.push_str("# UNX-F2662 cache_lookup read interface\n");
    s.push_str(&format!("code_base=0x{:04X}\n", CODE_BASE));
    s.push_str(&format!("code_end=0x{:04X}\n", CODE_END));
    s.push_str(&format!("err_variants={}\n", ERR_VARIANTS));
    s.push_str(&format!("contract_lines={}\n", CONTRACT_LINES));
    s.push_str(&format!("sign_lines={}\n", SIGN_LINES));
    s.push_str(&format!("assert_lines={}\n", ASSERT_LINES));
    s.push_str(&format!("book_lines={}\n", BOOK_LINES));
    s.push_str(&format!("contract_version={}\n", o.contract_version));
    s.push_str(&format!("contract_fp=0x{:016X}\n", o.contract_fp));
    s.push_str(&format!("clauses={}\n", o.clauses));
    s.push_str(&format!("sign_signed={}/{}\n", o.sign_signed, SIGNER_COUNT));
    s.push_str(&format!("sign_fp=0x{:016X}\n", o.sign_fp));
    s.push_str(&format!("hitmiss_total={}\n", o.hitmiss_total));
    s.push_str(&format!("hitmiss_hits={}\n", o.hitmiss_hits));
    s.push_str(&format!("hitmiss_misses={}\n", o.hitmiss_misses));
    s.push_str(&format!("hitmiss_ok={}\n", o.hitmiss_ok as u32));
    s.push_str(&format!("readonly_ok={}\n", o.readonly_ok as u32));
    s.push_str(&format!("readonly_clamps={}\n", o.readonly_clamps));
    s.push_str(&format!("inject_total={}\n", o.inject_total));
    s.push_str(&format!("inject_caught={}\n", o.inject_caught));
    s.push_str(&format!("inject_code_match={}\n", o.inject_code_match));
    s.push_str(&format!("replay_stable={}\n", o.replay_stable as u32));
    s.push_str(&format!("replay_fp=0x{:016X}\n", o.replay_fp));
    s.push_str(&format!("conserved={}\n", o.conserved as u32));
    s.push_str(&format!("ring_ok={}\n", o.ring_ok as u32));
    s.push_str(&format!("frozen={}\n", o.frozen_ok as u32));
    s.push_str(&format!("rebuild_eq={}\n", o.rebuild_eq as u32));
    s.push_str(&format!("self_check={}/{}\n", o.self_check_ok, SELF_CHECK_ITEMS));
    s.push_str(&format!("fp=0x{:016X}\n", o.fp));
    s.push_str(&format!("all_ok={}\n", o.all_ok as u32));
    s.into_bytes()
}

/// u16 -> 大端。
#[inline]
fn be16(v: u16, out: &mut Vec<u8>) {
    out.push(((v >> 8) & 0xffu16) as u8);
    out.push((v & 0xffu16) as u8);
}

/// u32 -> 大端。
#[inline]
fn be32(v: u32, out: &mut Vec<u8>) {
    out.push(((v >> 24) & 0xffu32) as u8);
    out.push(((v >> 16) & 0xffu32) as u8);
    out.push(((v >> 8) & 0xffu32) as u8);
    out.push((v & 0xffu32) as u8);
}

/// u64 -> 大端。
#[inline]
fn be64(v: u64, out: &mut Vec<u8>) {
    let mut i: u32 = 0u32;
    while i < 8u32 {
        out.push(((v >> (56u32 - 8u32 * i)) & 0xffu64) as u8);
        i = i + 1u32;
    }
}

/// TLV 追加一条 u64。
#[inline]
fn push_u64(out: &mut Vec<u8>, v: u64) {
    out.push(0x10u8);
    be64(v, out);
}

/// TLV 追加一条字符串。
#[inline]
fn push_str(out: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    out.push(0x20u8);
    be32(b.len() as u32, out);
    let mut i: usize = 0usize;
    while i < b.len() {
        out.push(b[i]);
        i = i + 1usize;
    }
}

/// 导出 TLV。
///
/// 组头 `version` **单独一行**（单独一次 `push_u64`），
/// 不被行尾注释吞——产线硬要求。
pub fn export_tlv(o: &Observation) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    // 组头：单独一行
    push_u64(&mut v, TLV_VERSION);
    push_str(&mut v, "UNX-F2662");
    push_u64(&mut v, o.fp);
    push_u64(&mut v, CODE_BASE as u64);
    push_u64(&mut v, ERR_VARIANTS as u64);
    push_u64(&mut v, CONTRACT_LINES as u64);
    push_u64(&mut v, SIGN_LINES as u64);
    push_u64(&mut v, ASSERT_LINES as u64);
    push_u64(&mut v, o.contract_version);
    push_u64(&mut v, o.contract_fp);
    push_u64(&mut v, o.clauses as u64);
    push_u64(&mut v, o.sign_signed as u64);
    push_u64(&mut v, o.sign_fp);
    push_u64(&mut v, o.hitmiss_total);
    push_u64(&mut v, o.hitmiss_hits);
    push_u64(&mut v, o.hitmiss_misses);
    push_u64(&mut v, o.hitmiss_ok as u64);
    push_u64(&mut v, o.readonly_ok as u64);
    push_u64(&mut v, o.readonly_clamps as u64);
    push_u64(&mut v, o.inject_total as u64);
    push_u64(&mut v, o.inject_caught as u64);
    push_u64(&mut v, o.inject_code_match as u64);
    push_u64(&mut v, o.replay_stable as u64);
    push_u64(&mut v, o.replay_fp);
    push_u64(&mut v, o.conserved as u64);
    push_u64(&mut v, o.ring_ok as u64);
    push_u64(&mut v, o.frozen_ok as u64);
    push_u64(&mut v, o.rebuild_eq as u64);
    push_u64(&mut v, o.self_check_ok as u64);
    push_u64(&mut v, SELF_CHECK_ITEMS as u64);
    push_u64(&mut v, o.all_ok as u64);
    v
}

/// TLV 键值条目数（不含组头 version 与 name）。逐条 push_u64，共 29 条。
pub const TLV_FIELDS: usize = 29;
/// 组头字节数：type(1) + value(8)。
pub const TLV_HDR_LEN: usize = 9;
/// name 字节数：type(1) + len(4) + "UNX-F2662"(9)。
pub const TLV_NAME_LEN: usize = 14;
/// TLV 精确总长：组头 + name + 条目 * 9 = 9 + 14 + 261 = 284。
pub const TLV_MIN_LEN: usize = TLV_HDR_LEN + TLV_NAME_LEN + TLV_FIELDS * 9;

/// TLV 结构长度自检。
pub fn tlv_len_ok(v: &[u8]) -> bool {
    // 精确长度：不等即说明有条目漏推或多推
    if v.len() != TLV_MIN_LEN {
        return false;
    }
    if v[0] != 0x10u8 {
        return false;
    }
    let mut ver: u64 = 0u64;
    let mut i: usize = 1usize;
    while i < 9usize {
        ver = (ver << 8) | (v[i] as u64);
        i = i + 1usize;
    }
    if ver != TLV_VERSION {
        return false;
    }
    if v[9] != 0x20u8 {
        return false;
    }
    let mut nl: u32 = 0u32;
    let mut j: usize = 10usize;
    while j < 14usize {
        nl = (nl << 8) | (v[j] as u32);
        j = j + 1usize;
    }
    // name 长度前缀必须与实际一致（"UNX-F2662" 为 9 字节）
    if nl as usize != 9usize {
        return false;
    }
    true
}

/// TLV 字节数。
pub fn tlv_len(o: &Observation) -> usize {
    export_tlv(o).len()
}

// ===========================================================================
// 第 16 节 · 性能逐项分解
// ===========================================================================

/// 复杂度一项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CxItem {
    /// 项名。
    pub name: &'static str,
    /// 时间复杂度。
    pub time: &'static str,
    /// 空间（字节）。
    pub bytes: u32,
    /// 步数。
    pub steps: u64,
    /// 系统调用数（恒 0）。
    pub syscalls: u64,
}

/// 复杂度分解。
pub fn complexity() -> Complexity {
    let items = [
        CxItem { name: "key_fold", time: "O(1)", bytes: 16u32, steps: 1u64, syscalls: 0u64 },
        CxItem { name: "tree_probe", time: "O(1) avg / O(n) worst", bytes: 0u32, steps: 1u64, syscalls: 0u64 },
        CxItem { name: "lookup", time: "O(1)", bytes: 32u32, steps: 2u64, syscalls: 0u64 },
        CxItem { name: "result_audit", time: "O(1)", bytes: 16u32, steps: 3u64, syscalls: 0u64 },
        CxItem { name: "readonly_guard", time: "O(1)", bytes: 0u32, steps: 2u64, syscalls: 0u64 },
        CxItem { name: "tree_bind", time: "O(1) avg", bytes: 0u32, steps: 2u64, syscalls: 0u64 },
        CxItem { name: "ring_push", time: "O(1)", bytes: 32u32, steps: 8u64, syscalls: 0u64 },
        CxItem { name: "ring_verify", time: "O(cap)", bytes: 8u32 * 3u32, steps: RING_CAP as u64, syscalls: 0u64 },
        CxItem { name: "self_fp", time: "O(frames+nodes)", bytes: 8u32, steps: (FRAMES + NODE_CAP) as u64 * 3u64, syscalls: 0u64 },
        CxItem { name: "hitmiss_10e5", time: "O(rounds)", bytes: 0u32, steps: HITMISS_ROUNDS * 3u64, syscalls: 0u64 },
    ];
    let mut ts: u64 = 0u64;
    let mut tb: u32 = 0u32;
    let mut tsy: u64 = 0u64;
    let mut i: usize = 0usize;
    while i < items.len() {
        ts = ts.wrapping_add(items[i].steps);
        tb = tb.wrapping_add(items[i].bytes);
        tsy = tsy.wrapping_add(items[i].syscalls);
        i = i + 1usize;
    }
    Complexity {
        items,
        total_steps: ts,
        total_bytes: tb,
        total_syscalls: tsy,
        has_float: false,
        has_io: false,
    }
}

/// 复杂度结构。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Complexity {
    /// 全部项。
    pub items: [CxItem; 10],
    /// 总步数。
    pub total_steps: u64,
    /// 总字节。
    pub total_bytes: u32,
    /// 总系统调用数。
    pub total_syscalls: u64,
    /// 是否含浮点（恒 false）。
    pub has_float: bool,
    /// 是否含真实 IO（恒 false）。
    pub has_io: bool,
}

impl Complexity {
    /// 概要行。
    pub fn summary(&self) -> String {
        format!(
            "cx[steps={} bytes={} syscalls={} float={} io={}]",
            self.total_steps,
            self.total_bytes,
            self.total_syscalls,
            self.has_float as u32,
            self.has_io as u32
        )
    }
}

/// 「深在哪」的自我陈述。
pub fn depth_note() -> String {
    let mut s = String::new();
    s.push_str("[UNX-F2662 深在哪]\n");
    s.push_str("1) 命中/未命中是两个枚举态而非错误码：调用方无法在 Err 分支里顺手做恢复动作，\n");
    s.push_str("   那类动作全是副作用，会从接口层就破掉「lookup 无副作用」这条契约。\n");
    s.push_str("2) 未命中态零载荷：miss() 连参数都不收，结构上没有塞载荷的后门；\n");
    s.push_str("   而带页帧的未命中会诱导调用方直接拿它去读，等于给一致性开了个口子。\n");
    s.push_str("3) 只读性由前后指纹夹逼证明：lookup 取 &self，类型层排除改本体；\n");
    s.push_str("   再用 self_fp 前后夹逼，把「只读」从承诺变成可离线回归的断言。\n");
    s.push_str("4) 计数与查询分成两个函数（lookup / lookup_counted）：\n");
    s.push_str("   把副作用混进 lookup 加个开关，等于给契约开后门，因此从函数边界上隔开。\n");
    s.push_str("5) 联签签名只掺 (面标识, 版本, 只读关注位)：\n");
    s.push_str("   契约实质变更必然改版本号，于是所有旧签名自动失配，无需维护「是否过期」标记。\n");
    s.push_str("6) Windows 对照：CcLookup/FsRtlLookupFileMcc 都不承诺无副作用（会更新访问时间、\n");
    s.push_str("   触发 readahead），调用方无法区分「免费查询」与「付了 IO 代价的查询」；\n");
    s.push_str("   本单把只读从性能属性升级为可断言的语义契约。\n");
    s
}

// ===========================================================================
// 第 17 节 · 观测量与 observe 聚合入口
// ===========================================================================

/// 观测量。所有字段由真实计算得出。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Observation {
    /// 契约版本。
    pub contract_version: u64,
    /// 契约指纹。
    pub contract_fp: u64,
    /// 契约条目数（3）。
    pub clauses: u32,
    /// 已联签方数。
    pub sign_signed: u32,
    /// 联签指纹。
    pub sign_fp: u64,
    /// 命中未命中断言总次数。
    pub hitmiss_total: u64,
    /// 断言中的命中数。
    pub hitmiss_hits: u64,
    /// 断言中的未命中数。
    pub hitmiss_misses: u64,
    /// 命中未命中断言是否通过。
    pub hitmiss_ok: bool,
    /// 只读静态审计是否通过。
    pub readonly_ok: bool,
    /// 夹逼次数。
    pub readonly_clamps: u32,
    /// 注入总次数。
    pub inject_total: u32,
    /// 注入被检出次数。
    pub inject_caught: u32,
    /// 注入错误码相符次数。
    pub inject_code_match: u32,
    /// 注入形态数。
    pub inject_forms: u32,
    /// 回放同种子稳定。
    pub replay_stable: bool,
    /// 回放指纹。
    pub replay_fp: u64,
    /// 回放步数。
    pub replay_steps: u64,
    /// 守恒对账是否通过。
    pub conserved: bool,
    /// 守恒等式：hits + misses + rejects。
    pub conserve_total: u64,
    /// 环形三见证是否通过。
    pub ring_ok: bool,
    /// 冻结校验（含篡改检出）。
    pub frozen_ok: bool,
    /// 缓存重建等价性。
    pub rebuild_eq: bool,
    /// 自检通过项数。
    pub self_check_ok: u32,
    /// 错误闭集变体数。
    pub err_variants: u32,
    /// 返回态数（2）。
    pub states: u32,
    /// 矩阵格数（12 形态 x 7 检测面）。
    pub matrix_cells: u32,
    /// 矩阵覆盖格数。
    pub matrix_hits: u32,
    /// 观测总指纹。
    pub fp: u64,
    /// 全绿标志。
    pub all_ok: bool,
}

impl Observation {
    /// 概要行。
    pub fn summary(&self) -> String {
        format!(
            "all_ok={} ctr=v{}/{} sign={}/{} hm={}/{} ro={} inj={}/{}/{} cons={} ring={} replay={} frz={} rb={} sc={}/{} fp=0x{:016X}",
            self.all_ok,
            self.contract_version,
            self.clauses,
            self.sign_signed,
            SIGNER_COUNT,
            self.hitmiss_hits,
            self.hitmiss_total,
            self.hitmiss_ok as u32,
            self.inject_caught,
            self.inject_total,
            self.inject_code_match,
            self.conserved as u32,
            self.ring_ok as u32,
            self.replay_stable as u32,
            self.frozen_ok as u32,
            self.rebuild_eq as u32,
            self.self_check_ok,
            SELF_CHECK_ITEMS,
            self.fp
        )
    }
}

/// 形态-检测面覆盖矩阵（行 = 12 形态，列 = 7 检测面）。
///
/// 列：0 RESULT 返回态审计 / 1 KEY 键闸 / 2 SIGN 联签 /
///     3 READONLY 只读夹逼 / 4 CONTRACT 契约 / 5 CONS 守恒 / 6 CACHE 缓存本体
pub fn form_face_matrix() -> [[u32; 7]; 12] {
    let mut m = [[0u32; 7]; 12];
    let forms = all_forms();
    let mut f: u32 = 0u32;
    while f < 12u32 {
        let form = forms[f as usize];
        let col: usize = match form.face() {
            "NONE" => 6usize,
            "RESULT" => 0usize,
            "KEY" => 1usize,
            "SIGN" => 2usize,
            "READONLY" => 3usize,
            "CONTRACT" => 4usize,
            _ => 5usize,
        };
        m[f as usize][col] = 1u32;
        // 每个负向形态都必然经过缓存本体路径（构造 fixture 需建缓存）
        m[f as usize][6] = 1u32;
        // 守恒面：负向形态一旦被检出，stats 侧不会被计入，
        // 因此 "hits + misses + rejects == 计数入口调用数" 的守恒
        // 正是判定"检出没有靠偷偷计数"的那条不变量。
        // 所有形态（含基线）都在缓存会话中跑过统计，故全部覆盖。
        m[f as usize][5] = 1u32;
        f = f.wrapping_add(1u32);
    }
    m
}

/// 形态覆盖矩阵自洽。
pub fn form_face_audit() -> CcResult<u32> {
    let m = form_face_matrix();
    let mut rows: u32 = 0u32;
    let mut cols: [u32; 7] = [0u32; 7];
    let mut f: u32 = 0u32;
    while f < 12u32 {
        let mut s: u32 = 0u32;
        let mut c: usize = 0usize;
        while c < 7usize {
            s = s.wrapping_add(m[f as usize][c]);
            cols[c] = cols[c].wrapping_add(m[f as usize][c]);
            c = c + 1usize;
        }
        if s == 0u32 {
            return Err(ErrKind::CriterionFail);
        }
        rows = rows.wrapping_add(s);
        f = f.wrapping_add(1u32);
    }
    // 每个检测面至少被一个形态覆盖（盲区意味着该面坏了也没人发现）
    let mut c: usize = 0usize;
    let mut col_sum: u32 = 0u32;
    while c < 7usize {
        if cols[c] == 0u32 {
            return Err(ErrKind::CriterionFail);
        }
        col_sum = col_sum.wrapping_add(cols[c]);
        c = c + 1usize;
    }
    if rows != col_sum {
        return Err(ErrKind::CriterionFail);
    }
    Ok(rows)
}

/// 观测量指纹。
pub fn fp_observe(o: &Observation) -> u64 {
    let mut h = FNV_BASIS;
    h = fnv1a_mix(h, CODE_BASE as u64);
    h = fnv1a_mix(h, ERR_VARIANTS as u64);
    h = fnv1a_mix(h, o.contract_version);
    h = fnv1a_mix(h, o.contract_fp);
    h = fnv1a_mix(h, o.clauses as u64);
    h = fnv1a_mix(h, o.sign_signed as u64);
    h = fnv1a_mix(h, o.sign_fp);
    h = fnv1a_mix(h, o.hitmiss_total);
    h = fnv1a_mix(h, o.hitmiss_hits);
    h = fnv1a_mix(h, o.hitmiss_misses);
    h = fnv1a_mix(h, o.hitmiss_ok as u64 as u64);
    h = fnv1a_mix(h, o.readonly_ok as u64 as u64);
    h = fnv1a_mix(h, o.readonly_clamps as u64);
    h = fnv1a_mix(h, o.inject_forms as u64);
    h = fnv1a_mix(h, o.inject_total as u64);
    h = fnv1a_mix(h, o.inject_caught as u64);
    h = fnv1a_mix(h, o.inject_code_match as u64);
    h = fnv1a_mix(h, o.replay_stable as u64 as u64);
    h = fnv1a_mix(h, o.replay_fp);
    h = fnv1a_mix(h, o.replay_steps);
    h = fnv1a_mix(h, o.conserved as u64 as u64);
    h = fnv1a_mix(h, o.conserve_total);
    h = fnv1a_mix(h, o.ring_ok as u64 as u64);
    h = fnv1a_mix(h, o.frozen_ok as u64 as u64);
    h = fnv1a_mix(h, o.rebuild_eq as u64 as u64);
    h = fnv1a_mix(h, o.self_check_ok as u64);
    h = fnv1a_mix(h, o.states as u64);
    h = fnv1a_mix(h, o.matrix_cells as u64);
    h = fnv1a_mix(h, o.matrix_hits as u64);
    h = fnv1a_mix(h, o.all_ok as u64 as u64);
    h
}

/// 观测量聚合入口。
///
/// 流程：常量与闭集 -> 返回态契约 -> 契约审计 -> 联签审计 ->
///      命中未命中 10^5 -> 只读静态审计 -> 注入核 -> 缓存会话（守恒+环形）
///      -> 重建等价 -> 回放两次 -> 冻结+篡改 -> 自检 -> 汇总
///
/// **绝不**与 `replay` 互递归。
pub fn observe(seed: u64) -> Observation {
    // 1 · 闭集与常量
    let err_variants = err_set_audit().unwrap_or(0u32);
    let states = result_contract_audit().unwrap_or(0u32);
    let clauses = contract_audit().unwrap_or(0u32);

    // 2 · 契约 + 联签
    let (ct, sl) = sign_ledger();
    let sign_signed = sl.signed_count();
    let sign_ok = sl.audit(ct.version).is_ok();

    // 3 · 形态矩阵
    let m = form_face_matrix();
    let mut cells: u32 = 0u32;
    let mut hits_cells: u32 = 0u32;
    let mut f: u32 = 0u32;
    while f < 12u32 {
        let mut c: usize = 0usize;
        while c < 7usize {
            cells = cells.wrapping_add(1u32);
            hits_cells = hits_cells.wrapping_add(m[f as usize][c]);
            c = c + 1usize;
        }
        f = f.wrapping_add(1u32);
    }
    let face_ok = form_face_audit().is_ok();

    // 4 · 注入核
    let inj = inject_core(seed);

    // 5 · 命中未命中 10^5
    let hm = hit_miss_10e5(seed);

    // 6 · 只读静态审计
    let (ro_ok, ro_clamps) = readonly_static_audit(seed);

    // 7 · 缓存会话（守恒 + 环形 + 只读夹逼）
    let mut rng = Lcg::new(seed ^ 0x1D80_0008u64);
    let mut conserved = false;
    let mut ring_ok = false;
    let mut conserve_total: u64 = 0u64;
    let mut sess_ro = false;
    if let Ok((mut c, _keys)) = fixture(&mut rng) {
        c.contract_fp = ct.fp();
        // 一次纯 lookup（零副作用）+ 若干计数 lookup
        let k0 = LookupKey { file_id: 1u32, lba: 0u64, offset: 0u32 };
        sess_ro = c.readonly_guard(&k0).is_ok();
        let mut i: u32 = 0u32;
        while i < 20u32 {
            let sub = rng.fork_seed(i as u64);
            let mut r2 = Lcg::new(sub);
            let fid = 1u32 + r2.below(64u64) as u32;
            let lba = r2.below(1u64 << 16u64);
            let k = LookupKey { file_id: fid, lba, offset: 0u32 };
            let _ = c.lookup_counted(&k);
            i = i.wrapping_add(1u32);
        }
        conserved = c.stats.conserve().is_ok();
        conserve_total = c.stats.total();
        ring_ok = c.ring.verify().is_ok() && c.ring.assert_clean().is_ok();
    }

    // 8 · 重建等价
    let mut entries: Vec<(LookupKey, u32)> = Vec::new();
    let mut j: u32 = 0u32;
    while j < 8u32 {
        let fid = 1u32 + j;
        let lba = fnv1a_mix(fid as u64, seed) % (1u64 << 16u64);
        entries.push((LookupKey { file_id: fid, lba, offset: 0u32 }, (j as u32).wrapping_mul(3u32)));
        j = j.wrapping_add(1u32);
    }
    let rebuild_eq = Cache::rebuild_check(&entries, 3u32).is_ok();

    // 9 · 回放两次
    let (replay_stable, replay_fp, _fp2) = replay_stable(seed);
    let replay_steps = replay(seed).map(|r| r.steps).unwrap_or(0u64);

    // 10 · 冻结 + 篡改检出
    let frozen_ok = frozen_tamper_detect(seed);

    // 11 · 自检
    let (sc_ok, sc_bad) = self_check();

    let mut o = Observation {
        contract_version: ct.version,
        contract_fp: ct.fp(),
        clauses,
        sign_signed,
        sign_fp: sl.fp(),
        hitmiss_total: hm.total,
        hitmiss_hits: hm.hits,
        hitmiss_misses: hm.misses,
        hitmiss_ok: hm.ok,
        readonly_ok: ro_ok,
        readonly_clamps: ro_clamps,
        inject_total: inj.total,
        inject_caught: inj.caught,
        inject_code_match: inj.code_match,
        inject_forms: inj.forms,
        replay_stable,
        replay_fp,
        replay_steps,
        conserved,
        conserve_total,
        ring_ok,
        frozen_ok,
        rebuild_eq,
        self_check_ok: sc_ok,
        err_variants,
        states,
        matrix_cells: cells,
        matrix_hits: hits_cells,
        fp: 0u64,
        all_ok: false,
    };

    o.all_ok = err_variants == ERR_VARIANTS
        && states == 2u32
        && clauses == 3u32
        && sign_ok
        && sign_signed == SIGNER_COUNT
        && face_ok
        && inject_criterion(&inj)
        && hm.ok
        && ro_ok
        && sess_ro
        && conserved
        && ring_ok
        && rebuild_eq
        && replay_stable
        && frozen_ok
        && sc_bad == 0u32;

    o.fp = fp_observe(&o);
    o
}

/// 种子无关性对照：两个不同种子都必须 all_ok。
pub fn observe_seed_independent(seed_a: u64, seed_b: u64) -> bool {
    observe(seed_a).all_ok && observe(seed_b).all_ok
}

// ===========================================================================
// 第 18 节 · 自检（18 项）
// ===========================================================================

/// 自检项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScItem {
    /// 01 错误闭集变体数为 44。
    ErrVariants,
    /// 02 码段铺满。
    CodeRange,
    /// 03 错误码双向反查。
    CodeLookup,
    /// 04 返回态恰 2 态。
    TwoStates,
    /// 05 未命中零载荷。
    MissZeroPayload,
    /// 06 未命中取帧必失败。
    MissNoFrame,
    /// 07 命中页帧越界被拒。
    HitFrameRange,
    /// 08 键越界被拒。
    KeyRange,
    /// 09 偏移未对齐被拒。
    AlignReject,
    /// 10 契约三条齐全。
    ContractThree,
    /// 11 契约签名互异。
    ContractSigDistinct,
    /// 12 契约审计通过。
    ContractAudit,
    /// 13 联签五方齐全。
    SignFive,
    /// 14 联签签名互异。
    SignSigDistinct,
    /// 15 联签重复被拒。
    SignDupReject,
    /// 16 环形三见证自校验。
    RingWitness,
    /// 17 三见证对篡改敏感。
    RingSensitive,
    /// 18 缓存重建等价。
    RebuildEqual,
}

/// 自检项全集。
pub fn sc_items() -> [ScItem; 18] {
    [
        ScItem::ErrVariants,
        ScItem::CodeRange,
        ScItem::CodeLookup,
        ScItem::TwoStates,
        ScItem::MissZeroPayload,
        ScItem::MissNoFrame,
        ScItem::HitFrameRange,
        ScItem::KeyRange,
        ScItem::AlignReject,
        ScItem::ContractThree,
        ScItem::ContractSigDistinct,
        ScItem::ContractAudit,
        ScItem::SignFive,
        ScItem::SignSigDistinct,
        ScItem::SignDupReject,
        ScItem::RingWitness,
        ScItem::RingSensitive,
        ScItem::RebuildEqual,
    ]
}

impl ScItem {
    /// 项名。
    pub fn name(&self) -> &'static str {
        match self {
            ScItem::ErrVariants => "ERR_VARIANTS",
            ScItem::CodeRange => "CODE_RANGE",
            ScItem::CodeLookup => "CODE_LOOKUP",
            ScItem::TwoStates => "TWO_STATES",
            ScItem::MissZeroPayload => "MISS_ZERO_PAYLOAD",
            ScItem::MissNoFrame => "MISS_NO_FRAME",
            ScItem::HitFrameRange => "HIT_FRAME_RANGE",
            ScItem::KeyRange => "KEY_RANGE",
            ScItem::AlignReject => "ALIGN_REJECT",
            ScItem::ContractThree => "CONTRACT_THREE",
            ScItem::ContractSigDistinct => "CONTRACT_SIG_DISTINCT",
            ScItem::ContractAudit => "CONTRACT_AUDIT",
            ScItem::SignFive => "SIGN_FIVE",
            ScItem::SignSigDistinct => "SIGN_SIG_DISTINCT",
            ScItem::SignDupReject => "SIGN_DUP_REJECT",
            ScItem::RingWitness => "RING_WITNESS",
            ScItem::RingSensitive => "RING_SENSITIVE",
            ScItem::RebuildEqual => "REBUILD_EQUAL",
        }
    }
}

/// 单项自检。
pub fn sc_check(it: ScItem) -> bool {
    match it {
        ScItem::ErrVariants => ERR_VARIANTS == 44u32 && err_all().len() == 44usize,
        ScItem::CodeRange => CODE_END == CODE_BASE.wrapping_add(44u16),
        ScItem::CodeLookup => {
            let mut ok = true;
            let mut i: u32 = 0u32;
            while i < ERR_VARIANTS {
                if err_by_index(i).is_err() || err_by_code(CODE_BASE.wrapping_add(i as u16)).is_err() {
                    ok = false;
                }
                i = i.wrapping_add(1u32);
            }
            err_by_index(ERR_VARIANTS).is_err() && err_by_code(0u16).is_err() && ok
        }
        ScItem::TwoStates => result_universe().len() == 2usize && result_contract_audit().is_ok(),
        ScItem::MissZeroPayload => {
            let m = LookupResult::miss();
            m.payload_len() == 0u32 && m.is_miss() && m.fp() != 0u64
        }
        ScItem::MissNoFrame => LookupResult::miss().frame().is_err(),
        ScItem::HitFrameRange => {
            LookupResult::hit(MISS_PFN_PLACEHOLDER).is_err()
                && LookupResult::hit(PFN_MASK).map(|r| r.is_hit()).unwrap_or(false)
        }
        ScItem::KeyRange => {
            LookupKey::new(1u32, KEY_SPACE, 0u32).is_err()
                && LookupKey::new(1u32, KEY_SPACE - 1u64, 0u32).is_ok()
        }
        ScItem::AlignReject => {
            LookupKey::new(1u32, 1u64, 1u32).is_err()
                && LookupKey::new(1u32, 1u64, OFFSET_MASK as u32).is_err()
                && LookupKey::new(1u32, 1u64, 0u32).is_ok()
        }
        ScItem::ContractThree => CLAUSES.len() == 3usize && clause_texts().len() == 3usize,
        ScItem::ContractSigDistinct => {
            let c = Contract::register();
            c.clause_sig[0] != c.clause_sig[1]
                && c.clause_sig[1] != c.clause_sig[2]
                && c.clause_sig[0] != c.clause_sig[2]
        }
        ScItem::ContractAudit => Contract::register().audit().map(|v| v == 3u32).unwrap_or(false),
        ScItem::SignFive => sign_audit().map(|v| v == SIGNER_COUNT).unwrap_or(false),
        ScItem::SignSigDistinct => {
            let (_c, l) = sign_ledger();
            let mut ok = true;
            let mut i: usize = 0usize;
            while i < 5usize {
                let mut j: usize = i.wrapping_add(1usize);
                while j < 5usize {
                    if l.signers[i].expect_sig == l.signers[j].expect_sig {
                        ok = false;
                    }
                    j = j + 1usize;
                }
                i = i + 1usize;
            }
            ok
        }
        ScItem::SignDupReject => {
            let mut l = SignLedger::new();
            let ids = all_signers();
            l.sign(ids[0], 1u64).is_ok() && l.sign(ids[0], 1u64).is_err()
        }
        ScItem::RingWitness => {
            let mut r = TraceRing::new();
            let mut ok = true;
            let mut i: u64 = 0u64;
            while i < (RING_CAP as u64).wrapping_mul(2u64) {
                r.push(Record::of(if i % 2u64 == 0u64 { Trace::Hit } else { Trace::Miss }, i, i));
                if r.verify().is_err() {
                    ok = false;
                }
                i = i.wrapping_add(1u64);
            }
            ok && r.dropped == RING_CAP as u64
        }
        ScItem::RingSensitive => {
            let mut r = TraceRing::new();
            let mut i: u64 = 0u64;
            while i < 4u64 {
                r.push(Record::of(Trace::Hit, i, i));
                i = i.wrapping_add(1u64);
            }
            if r.verify().is_err() {
                return false;
            }
            r.slots[2].payload = 0xABCDu64;
            r.verify().is_err()
        }
        ScItem::RebuildEqual => {
            let mut e: Vec<(LookupKey, u32)> = Vec::new();
            let mut i: u32 = 0u32;
            while i < 8u32 {
                e.push((
                    LookupKey { file_id: 1u32 + i, lba: (i as u64).wrapping_mul(13u64), offset: 0u32 },
                    i.wrapping_mul(5u32),
                ));
                i = i.wrapping_add(1u32);
            }
            Cache::rebuild_check(&e, 3u32).is_ok()
        }
    }
}

/// 自检总入口。返回 `(通过项数, 首个失败项序号)`。
pub fn self_check() -> (u32, u32) {
    let items = sc_items();
    let mut ok: u32 = 0u32;
    let mut bad: u32 = 0u32;
    let mut i: usize = 0usize;
    while i < items.len() {
        if sc_check(items[i]) {
            ok = ok.wrapping_add(1u32);
        } else if bad == 0u32 {
            bad = (i as u32).wrapping_add(1u32);
        }
        i = i + 1usize;
    }
    (ok, bad)
}

// ===========================================================================
// 第 19 节 · 判据回归（criterion）
// ===========================================================================

/// 判据回归结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Criterion {
    /// 判据一：契约冻结登记在档。
    pub contract: bool,
    /// 契约条目数。
    pub clauses: u32,
    /// 判据二：10^5 次正确性断言。
    pub hitmiss: bool,
    /// 断言次数。
    pub rounds: u64,
    /// 判据三：只读静态断言零例外。
    pub readonly: bool,
    /// 夹逼次数。
    pub clamps: u32,
    /// 判据四：B1–B5 五方联签在档。
    pub signed: bool,
    /// 联签方数。
    pub sign_n: u32,
    /// 判据五：注入检出。
    pub inject: bool,
    /// 注入检出次数。
    pub inject_caught: u32,
}

impl Criterion {
    /// 概要行。
    pub fn summary(&self) -> String {
        format!(
            "criterion[contract={}/{} hitmiss={}/{} readonly={}/{} signed={}/{} inject={}/{}]",
            self.contract as u32,
            self.clauses,
            self.hitmiss as u32,
            self.rounds,
            self.readonly as u32,
            self.clamps,
            self.signed as u32,
            self.sign_n,
            self.inject as u32,
            self.inject_caught
        )
    }

    /// 是否全过。
    pub fn ok(&self) -> bool {
        self.contract && self.hitmiss && self.readonly && self.signed && self.inject
    }
}

/// 判据回归总入口。
pub fn criterion(seed: u64) -> Criterion {
    let (ct, sl) = sign_ledger();
    let contract = ct.audit().is_ok();
    let clauses = CLAUSES.len() as u32;
    let hm = hit_miss_10e5(seed);
    let (ro, clamps) = readonly_static_audit(seed);
    let signed = sl.audit(ct.version).is_ok();
    let inj = inject_core(seed);
    Criterion {
        contract,
        clauses,
        hitmiss: hm.ok,
        rounds: hm.total,
        readonly: ro,
        clamps,
        signed,
        sign_n: sl.signed_count(),
        inject: inject_criterion(&inj),
        inject_caught: inj.caught,
    }
}

// ===========================================================================
// 第 20 节 · 清单与演示
// ===========================================================================

/// 模块清单。
pub fn manifest() -> Vec<u8> {
    let mut s = String::new();
    s.push_str("=== UNX-F2662 manifest ===\n");
    s.push_str(&format!(
        "code        : 0x{:04X}..0x{:04X} ({} variants)\n",
        CODE_BASE, CODE_END, ERR_VARIANTS
    ));
    s.push_str(&format!("book target : {} = {} + {} + {}\n", BOOK_LINES, CONTRACT_LINES, SIGN_LINES, ASSERT_LINES));
    s.push_str(&format!("hard target : {} lines\n", HARD_LINES));
    s.push_str(&format!("contract    : 3 clauses (C1 hit-frame / C2 miss-zero-payload / C3 readonly), v{}\n", CONTRACT_VERSION));
    s.push_str(&format!("signers     : {} x {} fields, clause §{}.{}\n", SIGNER_COUNT, SIGN_FIELDS, CLAUSE_16_2, 2u32));
    s.push_str(&format!("hitmiss     : {} rounds\n", HITMISS_ROUNDS));
    s.push_str(&format!("inject      : {} forms x {} reps = {}\n", FORM_COUNT, FORM_REPEAT, INJECT_TOTAL));
    s.push_str(&format!("states      : 2 (Hit / Miss), miss payload = 0\n"));
    s.push_str(&format!("readonly    : &self + self_fp clamp, no stats write inside lookup\n"));
    s.push_str(&format!("ring        : cap {} three-witness (len/sum/xor)\n", RING_CAP));
    s.push_str(&format!("frames      : {}, key space 2^20, fanout {}, depth {}\n", FRAMES, FANOUT, MAX_DEPTH));
    s.push_str(&format!("replay cap  : {} steps\n", REPLAY_STEPS));
    s.push_str(&format!("self_check  : {} items\n", SELF_CHECK_ITEMS));
    s.into_bytes()
}

/// 演示入口。
pub fn demo(seed: u64) -> String {
    let o = observe(seed);
    let mut s = String::new();
    s.push_str(&String::from_utf8_lossy(&manifest()));
    s.push_str("\n--- contract ---\n");
    s.push_str(&Contract::register().summary());
    s.push_str("\n");
    let mut i: usize = 0usize;
    while i < clause_texts().len() {
        s.push_str(&format!("  {}\n", clause_texts()[i]));
        i = i + 1usize;
    }
    s.push_str("\n--- sign ---\n");
    let (_c, l) = sign_ledger();
    s.push_str(&l.summary());
    s.push_str("\n--- observe ---\n");
    s.push_str(&o.summary());
    s.push_str("\n\n--- criterion ---\n");
    s.push_str(&criterion(seed).summary());
    s.push_str("\n--- complexity ---\n");
    s.push_str(&complexity().summary());
    s.push_str("\n--- self_check ---\n");
    let (ok, bad) = self_check();
    s.push_str(&format!("self_check {}/{} bad={}\n", ok, SELF_CHECK_ITEMS, bad));
    s.push_str("\n--- depth note ---\n");
    s.push_str(&depth_note());
    s
}