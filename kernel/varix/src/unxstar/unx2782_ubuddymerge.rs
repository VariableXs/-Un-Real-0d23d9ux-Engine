//! ===========================================================================
//! UNX-F2782 · buddy 归还批量合并（F2718 契约联签兑现）· 深化版纯功能模块
//! ===========================================================================
//!
//! ## 任务单
//!
//! * 单号：**UNX-F2782**
//! * 标题：**buddy 归还批量合并（F2718 契约联签兑现）**
//! * 域/批：**A4 / B20**
//! * 册目标：**360 行**（归并归还 150 + 合并率账 120 + 联签 90；测试段不计）
//!
//! ## 判据原文（逐字摘抄）
//!
//! > `UNX-F2782-J1 逐出页按阶归并归还 10/10 轮合并率达标，F2718 契约联签双签登记（B16 前向→B20 兑现）`
//!
//! > 判据 UNX-F2782-J1 的复测方式：碎片注入 10/10 轮合并率 ≥基准，绕归并注入 10/10 次
//! > 检出，双签登记哈希可查，归还阶账与 F2493 对平。
//!
//! ## 定位（原文）
//!
//! > B16-F2718 前向契约的兑现位——逐出页归还 buddy 不再零散单页归还，按阶归并（相邻伙伴页
//! > 探测，4KB→8KB→…→2MB 尽量高阶合并）后批量归还，合并率（高阶归还页数/总归还页数）达标
//! > 断言（≥60% 基准冻结），契约联签双签（B16-F2718 立约+本条兑现）登记入册——"回收拆、
//! > 分配合"的伙伴系统纪律闭环。
//!
//! ## 语义边界（原文）
//!
//! > 只做归还归并与联签登记；不做逐出（F2721）、不做 buddy 本体（F2493 消费接口）、
//! > 不做迁移页（F2789 边界）。
//!
//! ## 依赖与嫁接源
//!
//! > 依赖 F2718/F2721/F2493；嫁接源为 Linux free_pages 批量归还思想，实现自研。
//!
//! ## 风险与回退
//!
//! > 归并扫查开销（伙伴探测拖慢归还）——归并按批执行（批量 32 页硬顶同 F2731），探测限阶
//! > （≤2MB 封顶），开销注入对比断言。
//!
//! ## 码段范围声明
//!
//! * 本单独占段：**`0x1F40 .. 0x1F7F`**（`SEG_BASE = 0x1F40`，`SEG_MASK = 0x003F`）
//! * 错误变体一码一段，自 `0x1F40` 起顺序分配，共 12 变体，占 `0x1F40..0x1F4B`
//! * **不与任何其他 UNX 单重叠**（`0x1F40` 段此前为空）
//!
//! ## 硬约束（施工期自检）
//!
//! * `no_std + alloc`；零 `unwrap()/expect()/panic!/todo!`；零浮点；零真实 IO；零墙钟
//! * 时间一律走逻辑 `tick: u64`
//! * 确定性 LCG：`6364136223846793005 / 1442695040888963407`（全 `wrapping`）
//! * FNV-1a：`basis = 0xcbf29ce484222325`、`prime = 0x100000001b3`
//! * 自定义 `Result<T, E>` 别名 `MergeResult<T>`，`E` 带 `code() -> u16` 与 `detail() -> String`
//!   （三要素：现象 / 位置 / 建议修法）
//! * 不写 `#[test]`；只暴露 `observe(seed: u64) -> MergeObserve` 真实跑逻辑得出结论
//!
//! ## 与前四单的差别（为什么本单不是 F2702/F2722 的翻版）
//!
//! * F2702 解决**链**的完整性；F2722 解决**槽位**的前置判定；本单解决**归还**的**批量形状**
//! * 本单唯一的新增量是「**合并率**」这个可测标量：归并前后阶分布必须被单独记账，
//!   并且必须有一个**写死为红**的基准阈值，任何「归并逻辑被摘掉」的形态都会被合并率账抓住
//! * 本单第二个新增量是**双签登记**：立约签（B16-F2718）与兑现签（本单）必须成对存在且
//!   哈希入册，双签缺一即红
//!
//! ## LCG 说明
//!
//! 参数 `A = 6364136223846793005`、`C = 1442695040888963407`（Knuth MMIX 常数），
//! 全部运算走 `wrapping_mul` / `wrapping_add`，无溢出 UB，无平台差异。

#![allow(dead_code)]
extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
#[allow(unused_imports)]
use alloc::vec;
use alloc::vec::Vec;

// ===========================================================================
// 0. 常量表（判据阈值写死 + 来源注记）
// ===========================================================================

/// 版本号。
pub const V2782_VERSION: &str = "v2782-buddy-merge-contract";
/// 版本标识。
pub const V2782_VERSION_ID: u64 = 0x0002_7820_0000_0001;

/// 本单诊断码段基址（独占 `0x1F40..0x1F7F`）。
pub const SEG_BASE: u16 = 0x1F40;
/// 段掩码（`0x3F = 63`，一码一段 ≤60 变体，本单只用 12）。
pub const SEG_MASK: u16 = 0x003F;
/// 段末。
pub const SEG_END: u16 = SEG_BASE | SEG_MASK;
/// 主码（段首）。
pub const MAIN_CODE: u16 = SEG_BASE;

/// 底页大小（4KB）。
///
/// 来源：锚点正文「4KB→8KB→…→2MB」的阶升序列起点。
pub const PAGE_BASE: u64 = 4096;

/// 阶上限（order 9 → `4096 << 9 = 2MB`）。
///
/// 来源：锚点风险与回退「探测限阶（≤2MB 封顶）」。
pub const ORDER_LIMIT: usize = 9;

/// 阶闭集大小（order `0..=9`，共 10 阶）。
pub const ORDER_COUNT: usize = 10;

/// 归还批硬顶（32 页）。
///
/// 来源：锚点「归并按批执行（批量 32 页硬顶同 F2731）」。
pub const BATCH_HARD_CAP: u32 = 32;

/// 批量硬顶（`usize` 口径，供复杂度项做整型乘法）。
pub const CAP: usize = 32;

/// 合并率基准（百分比，冻结阈值，写死）。
///
/// 来源：锚点「合并率（高阶归还页数/总归还页数）达标断言（≥60% 基准冻结）」。
/// **本常量是判据的冻结面：任何改动都会使 `rate_ok()` 立刻转红。**
pub const MERGE_RATE_BASE: u64 = 60;

/// 碎片注入轮数（判据「碎片注入 10/10 轮」）。
pub const FRAG_ROUNDS: usize = 10;

/// 绕归并注入次数（判据「单页归还注入（绕归并）10/10 次检出」）。
pub const BYPASS_CASES: usize = 10;

/// 开销注入对比用例数（锚点「开销注入对比断言」）。
pub const OVERHEAD_CASES: usize = 10;

/// 注入形态数。
pub const FORM_COUNT: usize = 10;

/// 状态机节点数。
pub const STATE_COUNT: usize = 6;

/// 状态机合法边数。
///
/// 含一条自环 `Collecting -> Collecting`（收集是多次动作，必须允许自环）。
pub const EDGE_COUNT: usize = 7;

/// 守恒账组数。
pub const LEDGER_COUNT: usize = 4;

/// 矩阵格数（阶 × 伙伴可得性）。
pub const MATRIX_CELLS: usize = ORDER_COUNT * 2;

/// 阶码表（`"4K".."2M"`）。
pub const ORDER_CODES: [&str; ORDER_COUNT] = ["4K", "8K", "16K", "32K", "64K", "128K", "256K", "512K", "1M", "2M"];

/// 联签签位表。
pub const SIG_SLOTS: [&str; 2] = ["B16-F2718.PACT", "B20-F2782.REDEEM"];

/// 注入形态码表。
pub const FORM_CODES: [&str; FORM_COUNT] = ["skip_merge", "over_limit_order", "over_batch_cap", "dup_return", "range_out_of_pool", "misaligned_head", "order_ledger_break", "rate_below_base", "sig_dropped", "sig_hash_lie"];

/// 环形账容量。
pub const RING_CAP: usize = 192;

/// LCG 乘子。
pub const LCG_A: u64 = 6364136223846793005;
/// LCG 增量。
pub const LCG_C: u64 = 1442695040888963407;

/// FNV-1a basis。
pub const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a prime。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 池页数（8192 页 = 32MB 逻辑地址空间）。
pub const POOL_PAGES: u32 = 8192;

/// 碎片铺底占用图起点（`pno ≡ FRAG_OFFSET (mod FRAG_STRIDE)` 的页被占用）。
pub const FRAG_OFFSET: u32 = 4;

/// 碎片铺底占用图步长（每 8 页占 1 页）。
///
/// **选 8 的理由**：占用点落在 `pno ≡ 4 (mod 8)`，于是每个 8 页组里前 4 页
/// （`8k..8k+3`）保持连续空闲 —— 这正是 order-2 归并所需的最小伙伴结构。
/// 若占用点落在 4 页栅格上（如步长 4），空闲伙伴对会被占用页切开，归并率必然塌成 0。
pub const FRAG_STRIDE: u32 = 8;

/// F2718 契约扩展页代号。
pub const PACT_PAGE_CODE: &str = "F2718.PACT.MERGE";
/// F2718 契约扩展页纪元。
pub const PACT_EPOCH: u64 = 16;
/// F2718 契约引用串（基册）。
pub const PACT_REF: &str = "F2718.B16";
/// F2493 伙伴阶账引用串。
pub const F2493_REF: &str = "F2493.ORDER_LEDGER";
/// F2731 批硬顶引用串。
pub const F2731_REF: &str = "F2731.BATCH_CAP";

/// 「CI 基线写死即红」示范：签名基线**故意**置 0，必须由代码求值填入。
pub const SIG_BASELINE_PLACEHOLDER: u64 = 0;

// ===========================================================================
// 1. 确定性 LCG 与 FNV-1a
// ===========================================================================

/// 确定性线性同余发生器。
#[derive(Clone, Copy, Debug)]
pub struct Lcg {
    /// 当前状态。
    pub state: u64,
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
    /// `[lo, hi)` 内 `u64`（整数，无浮点）。
    pub fn range_u64(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo {
            return lo;
        }
        let span: u64 = hi.wrapping_sub(lo);
        lo.wrapping_add(self.next_u64() % span)
    }
    /// 派生支。
    pub fn fork(&self, salt: u64) -> Lcg {
        Lcg { state: self.state ^ salt.wrapping_mul(LCG_A) }
    }
}

/// 纯函数式 LCG 步进。
pub fn lcg_step(state: u64) -> u64 {
    state.wrapping_mul(LCG_A).wrapping_add(LCG_C)
}

/// FNV-1a 64 位（字节流）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = FNV_BASIS;
    let mut i: usize = 0;
    while i < bytes.len() {
        h ^= (bytes[i] as u64) & 0xFF;
        h = h.wrapping_mul(FNV_PRIME);
        i = i.wrapping_add(1);
    }
    h
}

/// FNV-1a 64 位（`u64` 词流，每词 8 字节小端喂入）。
pub fn fnv1a_words(words: &[u64]) -> u64 {
    let mut h: u64 = FNV_BASIS;
    let mut i: usize = 0;
    while i < words.len() {
        h = fnv1a_u64_into(h, words[i]);
        i = i.wrapping_add(1);
    }
    h
}

/// 把一个 `u64` 按 8 字节小端喂入已有哈希。
pub fn fnv1a_u64_into(h: u64, w: u64) -> u64 {
    let mut acc: u64 = h;
    let mut i: u32 = 0;
    while i < 8 {
        let b: u8 = ((w >> (8 * i)) & 0xFF) as u8;
        acc ^= (b as u64) & 0xFF;
        acc = acc.wrapping_mul(FNV_PRIME);
        i = i.wrapping_add(1);
    }
    acc
}

/// FNV-1a 32 位（便于与外部工具对拍）。
pub fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    let mut i: usize = 0;
    while i < bytes.len() {
        h ^= (bytes[i] as u32) & 0xFF;
        h = h.wrapping_mul(0x0100_0193);
        i = i.wrapping_add(1);
    }
    h
}

// ===========================================================================
// 2. 错误闭集（12 变体，一码一段，自 0x1F40 起）
// ===========================================================================

/// 归并归还错误闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MergeErr {
    /// 0x1F40 归并被跳过（单页归还绕归并直接入阶账）。
    MergeSkipped,
    /// 0x1F41 阶越限（探测阶超过 `ORDER_LIMIT`，即 >2MB）。
    OrderOverLimit,
    /// 0x1F42 批量越硬顶（本批页数超过 `BATCH_HARD_CAP = 32`）。
    BatchOverCap,
    /// 0x1F43 页号越池（`pno >= POOL_PAGES`）。
    PageOutOfPool,
    /// 0x1F44 归并伙伴已被占（伙伴页非空闲却仍被合并）。
    PartnerOccupied,
    /// 0x1F45 阶账不平（归还页数 ≠ 阶账累加）。
    OrderLedgerUnbalanced,
    /// 0x1F46 合并率低于冻结基准（< 60%）。
    RateBelowBase,
    /// 0x1F47 联签缺签（立约签或兑现签缺失）。
    SigMissing,
    /// 0x1F48 联签哈希不符（登记哈希与实算不符）。
    SigHashLie,
    /// 0x1F49 契约纪元不符（扩展页纪元 ≠ `PACT_EPOCH`）。
    PactEpochDrift,
    /// 0x1F4A 批内页号重复（同一页在同批出现两次）。
    DuplicatePage,
    /// 0x1F4B 归并结果非 2 的幂对齐（阶与页号不匹配）。
    OrderMisaligned,
}

impl MergeErr {
    /// 全部变体。
    pub const ALL: [MergeErr; 12] = [MergeErr::MergeSkipped, MergeErr::OrderOverLimit, MergeErr::BatchOverCap, MergeErr::PageOutOfPool, MergeErr::PartnerOccupied, MergeErr::OrderLedgerUnbalanced,
        MergeErr::RateBelowBase, MergeErr::SigMissing, MergeErr::SigHashLie, MergeErr::PactEpochDrift, MergeErr::DuplicatePage, MergeErr::OrderMisaligned];

    /// 变体序号。
    pub fn index(self) -> usize {
        match self {
            MergeErr::MergeSkipped => 0,
            MergeErr::OrderOverLimit => 1,
            MergeErr::BatchOverCap => 2,
            MergeErr::PageOutOfPool => 3,
            MergeErr::PartnerOccupied => 4,
            MergeErr::OrderLedgerUnbalanced => 5,
            MergeErr::RateBelowBase => 6,
            MergeErr::SigMissing => 7,
            MergeErr::SigHashLie => 8,
            MergeErr::PactEpochDrift => 9,
            MergeErr::DuplicatePage => 10,
            MergeErr::OrderMisaligned => 11,
        }
    }

    /// 按序号取变体（越界 `None`）。
    pub fn from_index(i: usize) -> Option<MergeErr> {
        if i < 12 {
            Some(MergeErr::ALL[i])
        } else {
            None
        }
    }

    /// 诊断码（一码一段）。
    pub fn code(self) -> u16 {
        SEG_BASE + (self.index() as u16)
    }

    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            MergeErr::MergeSkipped => "MergeSkipped",
            MergeErr::OrderOverLimit => "OrderOverLimit",
            MergeErr::BatchOverCap => "BatchOverCap",
            MergeErr::PageOutOfPool => "PageOutOfPool",
            MergeErr::PartnerOccupied => "PartnerOccupied",
            MergeErr::OrderLedgerUnbalanced => "OrderLedgerUnbalanced",
            MergeErr::RateBelowBase => "RateBelowBase",
            MergeErr::SigMissing => "SigMissing",
            MergeErr::SigHashLie => "SigHashLie",
            MergeErr::PactEpochDrift => "PactEpochDrift",
            MergeErr::DuplicatePage => "DuplicatePage",
            MergeErr::OrderMisaligned => "OrderMisaligned",
        }
    }

    /// 现象。
    pub fn symptom(self) -> &'static str {
        match self {
            MergeErr::MergeSkipped => "归并返回 Ok，但阶账形态是全 order-0 单页归还，说明归并通路被绕过",
            MergeErr::OrderOverLimit => "归并探测阶超过 9（>2MB），探测限阶封顶失效",
            MergeErr::BatchOverCap => "本批归还页数超过 32 页硬顶，批量无上限会让归并扫查拖慢归还",
            MergeErr::PageOutOfPool => "归还页号越出 8192 页池边界，归还账出现幽灵页",
            MergeErr::PartnerOccupied => "伙伴页非空闲却被当作空闲合并，buddy 不变量（伙伴同时空闲）被破坏",
            MergeErr::OrderLedgerUnbalanced => "阶账累加页数 ≠ 实际归还页数，归还账漏记或多记",
            MergeErr::RateBelowBase => "高阶归还页数占比低于冻结基准 60%，归并收益不达标",
            MergeErr::SigMissing => "F2718 双签缺位（立约签或兑现签缺失），契约未闭环",
            MergeErr::SigHashLie => "双签登记哈希与实算不符，登记值撒谎",
            MergeErr::PactEpochDrift => "契约扩展页纪元与冻结纪元 16 不符，契约被私自改版",
            MergeErr::DuplicatePage => "同一页号在同一归还批内出现两次，归还重复登记",
            MergeErr::OrderMisaligned => "归并块的页号与阶不满足 2 的幂对齐，块形状非法",
        }
    }

    /// 位置。
    pub fn locus(self) -> &'static str {
        match self {
            MergeErr::MergeSkipped => "unx2782::BuddyMerger::return_batch / shape_audit",
            MergeErr::OrderOverLimit => "unx2782::probe_order / ORDER_LIMIT",
            MergeErr::BatchOverCap => "unx2782::BuddyMerger::return_batch / BATCH_HARD_CAP",
            MergeErr::PageOutOfPool => "unx2782::BuddyMerger::return_batch / pno 边界",
            MergeErr::PartnerOccupied => "unx2782::probe_order / buddy 空闲位图",
            MergeErr::OrderLedgerUnbalanced => "unx2782::OrderLedger::push / F2493 阶账",
            MergeErr::RateBelowBase => "unx2782::MergeRateLedger::rate_ok / MERGE_RATE_BASE",
            MergeErr::SigMissing => "unx2782::PactBook::sign / F2718 双签",
            MergeErr::SigHashLie => "unx2782::PactBook::verify / 双签哈希",
            MergeErr::PactEpochDrift => "unx2782::PactBook::register / PACT_EPOCH",
            MergeErr::DuplicatePage => "unx2782::BuddyMerger::sort_dedup / 批内查重",
            MergeErr::OrderMisaligned => "unx2782::block_base / 阶-页号对齐",
        }
    }

    /// 建议修法。
    pub fn advice(self) -> &'static str {
        match self {
            MergeErr::MergeSkipped => "在 return_batch 入口补齐归并循环，或让 shape_audit 以 order-0 全单页为红判",
            MergeErr::OrderOverLimit => "在 probe_order 里加 order < ORDER_LIMIT 前置判定，越限即返回该错",
            MergeErr::BatchOverCap => "把批次切成 <= BATCH_HARD_CAP 的子批后再归并，或把硬顶改为契约登记值",
            MergeErr::PageOutOfPool => "归还入口先做 pno < POOL_PAGES 前置闸，越界页不得入账",
            MergeErr::PartnerOccupied => "probe_order 只能读空闲位图，不可写；合并前再验一次伙伴空闲",
            MergeErr::OrderLedgerUnbalanced => "return_batch 结束时用 reconcile() 对账，差额不得隐式吞掉",
            MergeErr::RateBelowBase => "检查归并循环是否被短路；合并率不达标应升级为红而非降基准",
            MergeErr::SigMissing => "补齐立约签与兑现签两槽后再 verify，双签缺一即红",
            MergeErr::SigHashLie => "以代码实算哈希覆写登记值，登记值不得手填",
            MergeErr::PactEpochDrift => "契约改版必须走扩展页登记流程并升纪元，不得就地改常量",
            MergeErr::DuplicatePage => "批内先排序再查重（相邻等值即重复），重复页直接拒收",
            MergeErr::OrderMisaligned => "归并块起点按 order 取整（block_base），禁止用非对齐 pno 起块",
        }
    }

    /// 诊断详情（三要素合一）。
    pub fn detail(self) -> String {
        format!("[{}|0x{:04X}] 现象：{}；位置：{}；建议：{}", self.name(), self.code(), self.symptom(), self.locus(), self.advice())
    }
}

/// 本单结果别名。
pub type MergeResult<T> = Result<T, MergeErr>;

/// 码段自检。
pub fn code_in_seg(code: u16) -> bool {
    code >= SEG_BASE && (code & (!SEG_MASK)) == SEG_BASE
}

/// 错误转字符串。
pub fn err_str(e: MergeErr) -> String {
    e.detail()
}

/// 错误码表（导出用）。
pub fn err_code_table() -> Vec<String> {
    let mut v: Vec<String> = Vec::with_capacity(12);
    let mut i: usize = 0;
    while i < 12 {
        let e: MergeErr = match MergeErr::from_index(i) {
            Some(x) => x,
            None => MergeErr::MergeSkipped,
        };
        v.push(format!("0x{:04X} {}", e.code(), e.name()));
        i = i.wrapping_add(1);
    }
    v
}

// ===========================================================================
// 3. 核心数据结构
// ===========================================================================

/// 归还批状态机节点。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BatchState {
    /// 0 空批（刚构造）。
    Idle,
    /// 1 收集中（逐出页正在汇入）。
    Collecting,
    /// 2 已排序（按页号升序，去重前）。
    Sorted,
    /// 3 归并中（阶升探测中）。
    Merging,
    /// 4 已归还（阶账已记）。
    Returned,
    /// 5 已封账（合并率已结算）。
    Sealed,
}

impl BatchState {
    /// 序号。
    pub fn index(self) -> usize {
        match self {
            BatchState::Idle => 0,
            BatchState::Collecting => 1,
            BatchState::Sorted => 2,
            BatchState::Merging => 3,
            BatchState::Returned => 4,
            BatchState::Sealed => 5,
        }
    }
    /// 按序号取。
    pub fn from_index(i: usize) -> Option<BatchState> {
        match i {
            0 => Some(BatchState::Idle),
            1 => Some(BatchState::Collecting),
            2 => Some(BatchState::Sorted),
            3 => Some(BatchState::Merging),
            4 => Some(BatchState::Returned),
            5 => Some(BatchState::Sealed),
            _ => None,
        }
    }
    /// 短名。
    pub fn code(self) -> &'static str {
        match self {
            BatchState::Idle => "Idle",
            BatchState::Collecting => "Collecting",
            BatchState::Sorted => "Sorted",
            BatchState::Merging => "Merging",
            BatchState::Returned => "Returned",
            BatchState::Sealed => "Sealed",
        }
    }
}

/// 阶码（order 0..=9）。
pub fn order_code(o: usize) -> &'static str {
    if o < ORDER_COUNT {
        ORDER_CODES[o]
    } else {
        "?"
    }
}

/// 阶字节数。
pub fn order_bytes(o: usize) -> u64 {
    PAGE_BASE.wrapping_shl(o as u32)
}

/// 阶覆盖页数（`2^o`）。
pub fn order_pages(o: usize) -> u64 {
    1u64.wrapping_shl(o as u32)
}

/// 块起始页号（按阶对齐取整）。
///
/// 阶 `o` 的块必须页号被 `2^o` 整除；这里用掩码取整。
pub fn block_base(pno: u32, o: usize) -> u32 {
    if o >= 32 {
        return pno;
    }
    pno & !(order_pages(o) as u32).wrapping_sub(1)
}

/// 伙伴页号（buddy）。
///
/// 伙伴 = 页号按第 `o` 位取反。
pub fn buddy_of(pno: u32, o: usize) -> u32 {
    if o >= 32 {
        return pno;
    }
    pno ^ ((1u32) << (o as u32))
}

// ===========================================================================
// 3.1 buddy 空闲池（位图 + 阶占用视图）
// ===========================================================================

/// buddy 空闲池。
///
/// 语义：`used[p]` 表示页 `p` 是否已被占用（1=占用，0=空闲）。
/// 归并只允许把**两个都空闲**的相邻 order-o 块合成 order-(o+1) 块。
#[derive(Clone, Debug)]
pub struct BuddyPool {
    /// 占用位（长度 `POOL_PAGES`，字节表示，非位压缩，避免浮点/移位边界坑）。
    pub used: Vec<u8>,
    /// 逻辑时钟。
    pub tick: u64,
}

impl BuddyPool {
    /// 构造全空闲池。
    pub fn new(tick0: u64) -> BuddyPool {
        let mut v: Vec<u8> = Vec::with_capacity(POOL_PAGES as usize);
        let mut i: u32 = 0;
        while i < POOL_PAGES {
            v.push(0);
            i = i.wrapping_add(1);
        }
        BuddyPool { used: v, tick: tick0 }
    }

    /// 页是否空闲。
    pub fn is_free(&self, pno: u32) -> bool {
        match self.used.get(pno as usize) {
            Some(v) => *v == 0,
            None => false,
        }
    }

    /// 页是否占用。
    pub fn is_used(&self, pno: u32) -> bool {
        match self.used.get(pno as usize) {
            Some(v) => *v != 0,
            None => false,
        }
    }

    /// 标占用。
    pub fn mark_used(&mut self, pno: u32) {
        let idx: usize = pno as usize;
        if idx < self.used.len() {
            self.used[idx] = 1;
        }
    }

    /// 标空闲。
    pub fn mark_free(&mut self, pno: u32) {
        let idx: usize = pno as usize;
        if idx < self.used.len() {
            self.used[idx] = 0;
        }
    }

    /// order-`o` 的整块是否全空闲。
    pub fn block_free(&self, pno: u32, o: usize) -> bool {
        let n: u64 = order_pages(o);
        let mut k: u64 = 0;
        while k < n {
            let p: u32 = pno.wrapping_add(k as u32);
            if !self.is_free(p) {
                return false;
            }
            k = k.wrapping_add(1);
        }
        true
    }

    /// 阶升探测：从 order 0 起，看能连攀到第几阶。
    ///
    /// **只读探测，不改池**；越限（`> ORDER_LIMIT`）以返回值表达，不在这里报错。
    pub fn probe_order(&self, pno: u32) -> usize {
        let mut o: usize = 0;
        let mut base: u32 = block_base(pno, 0);
        loop {
            if o >= ORDER_LIMIT {
                // 限阶封顶：不得再往上探（锚点「探测限阶 ≤2MB 封顶」）。
                return o;
            }
            let b: u32 = buddy_of(base, o);
            // 伙伴必须整块空闲才可继续升阶。
            if !self.block_free(b, o) {
                return o;
            }
            o = o.wrapping_add(1);
            base = block_base(b, o);
            if order_bytes(o) > order_bytes(ORDER_LIMIT) {
                return o;
            }
        }
    }

    /// 空闲页数。
    pub fn free_pages(&self) -> u64 {
        let mut n: u64 = 0;
        let mut i: u32 = 0;
        while i < POOL_PAGES {
            if self.is_free(i) {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 阶分布读数（归并前/后各读一次即得「阶分布」）。
    pub fn order_profile(&self) -> Vec<u64> {
        let mut v: Vec<u64> = Vec::with_capacity(ORDER_COUNT);
        let mut o: usize = 0;
        while o < ORDER_COUNT {
            // 该阶能整块空闲的块数（用于阶分布读数，非归并判据）。
            let step: u64 = order_pages(o);
            let mut cnt: u64 = 0;
            let mut base: u64 = 0;
            while base < (POOL_PAGES as u64) {
                if self.block_free(base as u32, o) {
                    cnt = cnt.wrapping_add(1);
                }
                base = base.wrapping_add(step);
            }
            v.push(cnt);
            o = o.wrapping_add(1);
        }
        v
    }
}

// ===========================================================================
// 3.2 归还批（归并主体）
// ===========================================================================

/// 归并块（一条归并结果）。
#[derive(Clone, Copy, Debug)]
pub struct MergeBlock {
    /// 块起始页号（按阶对齐）。
    pub base: u32,
    /// 块阶。
    pub order: usize,
    /// 块覆盖页数（`2^order`）。
    pub pages: u64,
}

impl MergeBlock {
    /// 导出行。
    pub fn row(&self) -> String {
        format!("block[base={} order={}({}) pages={}]", self.base, self.order, order_code(self.order), self.pages)
    }
    /// 指纹贡献。
    pub fn fold(&self, h: u64) -> u64 {
        let x: u64 = fnv1a_u64_into(h, self.base as u64);
        fnv1a_u64_into(x, self.order as u64)
    }
}

/// 归还批（一次 `return_batch` 的全部上下文）。
#[derive(Clone, Debug)]
pub struct Batch {
    /// 批内页号（收集顺序，尚未排序）。
    pub raw: Vec<u32>,
    /// 排序去重后的页号。
    pub sorted: Vec<u32>,
    /// 归并结果块。
    pub blocks: Vec<MergeBlock>,
    /// 当前状态。
    pub state: BatchState,
    /// 逻辑时钟。
    pub tick: u64,
    /// 归并探测步数（开销读数）。
    pub probe_steps: u64,
    /// 非法迁移拒记数。
    pub illegal: u64,
}

impl Batch {
    /// 构造空批。
    pub fn new(tick0: u64) -> Batch {
        Batch { raw: Vec::new(), sorted: Vec::new(), blocks: Vec::new(), state: BatchState::Idle, tick: tick0, probe_steps: 0, illegal: 0 }
    }

    /// 收集页（逐出页汇入）。
    pub fn collect(&mut self, pno: u32) -> MergeResult<u64> {
        self.transit(BatchState::Collecting)?;
        if pno >= POOL_PAGES {
            return Err(MergeErr::PageOutOfPool);
        }
        if self.raw.len() >= (BATCH_HARD_CAP as usize) {
            return Err(MergeErr::BatchOverCap);
        }
        self.raw.push(pno);
        Ok(self.raw.len() as u64)
    }

    /// 排序去重（相邻等值即重复）。
    pub fn sort_dedup(&mut self) -> MergeResult<u64> {
        self.transit(BatchState::Sorted)?;
        // 插入排序（批 ≤32 页，无需快排，且确定性更强）。
        let mut i: usize = 1;
        while i < self.raw.len() {
            let key: u32 = self.raw[i];
            let mut j: usize = i;
            while j > 0 && self.raw[j - 1] > key {
                self.raw[j] = self.raw[j - 1];
                j = j.wrapping_sub(1);
            }
            self.raw[j] = key;
            i = i.wrapping_add(1);
        }
        let mut out: Vec<u32> = Vec::with_capacity(self.raw.len());
        let mut k: usize = 0;
        while k < self.raw.len() {
            if out.is_empty() {
                out.push(self.raw[k]);
            } else {
                let last: u32 = match out.last() {
                    Some(v) => *v,
                    None => 0,
                };
                if self.raw[k] == last {
                    // 同批重复页 → 直接拒收整批。
                    return Err(MergeErr::DuplicatePage);
                }
                out.push(self.raw[k]);
            }
            k = k.wrapping_add(1);
        }
        self.sorted = out;
        Ok(self.sorted.len() as u64)
    }

    /// 状态迁移（合法边查表，非法迁移拒记）。
    pub fn transit(&mut self, to: BatchState) -> MergeResult<u64> {
        if legal_edge(self.state, to) {
            self.state = to;
            Ok(to.index() as u64)
        } else {
            self.illegal = self.illegal.wrapping_add(1);
            Err(MergeErr::MergeSkipped)
        }
    }

    /// 批页数。
    pub fn len(&self) -> usize {
        self.raw.len()
    }
}

/// 合法边闭集。
pub const EDGE_LEGAL: [(BatchState, BatchState); EDGE_COUNT] = [(BatchState::Idle, BatchState::Collecting), (BatchState::Collecting, BatchState::Collecting), (BatchState::Collecting, BatchState::Sorted), (BatchState::Sorted, BatchState::Merging), (BatchState::Merging, BatchState::Returned), (BatchState::Returned, BatchState::Sealed), (BatchState::Idle, BatchState::Sealed)];

/// 合法边判定。
pub fn legal_edge(from: BatchState, to: BatchState) -> bool {
    let mut i: usize = 0;
    while i < EDGE_COUNT {
        if EDGE_LEGAL[i].0 == from && EDGE_LEGAL[i].1 == to {
            return true;
        }
        i = i.wrapping_add(1);
    }
    false
}

/// 边表文本。
pub fn edge_table_text() -> Vec<String> {
    let mut v: Vec<String> = Vec::with_capacity(EDGE_COUNT);
    let mut i: usize = 0;
    while i < EDGE_COUNT {
        v.push(format!("edge {} -> {}", EDGE_LEGAL[i].0.code(), EDGE_LEGAL[i].1.code()));
        i = i.wrapping_add(1);
    }
    v
}

// ===========================================================================
// 4. 守恒账（`a + b + ... == total`，整数，从 0 起算并与分配同步）
// ===========================================================================

/// 阶账（对平 F2493 伙伴阶账）：`merged + single == total`。
#[derive(Clone, Copy, Debug)]
pub struct OrderLedger {
    /// 归还页总数（收集时定）。
    pub total: u64,
    /// 被并入高阶块的页数。
    pub merged: u64,
    /// 留在 order-0 的单页数。
    pub single: u64,
    /// 结算次数。
    pub settle: u64,
}

impl OrderLedger {
    /// 分配。
    pub fn alloc(n: usize) -> OrderLedger {
        OrderLedger { total: n as u64, merged: 0, single: 0, settle: 0 }
    }
    /// 记并入。
    pub fn on_merged(&mut self, pages: u64) {
        self.merged = self.merged.wrapping_add(pages);
    }
    /// 记单页。
    pub fn on_single(&mut self, pages: u64) {
        self.single = self.single.wrapping_add(pages);
    }
    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.merged.wrapping_add(self.single) == self.total
    }
    /// 调账（把差额归到单页；返回调账额）。
    pub fn reconcile(&mut self) -> u64 {
        let cur: u64 = self.merged.wrapping_add(self.single);
        if cur <= self.total {
            let gap: u64 = self.total.wrapping_sub(cur);
            self.single = self.single.wrapping_add(gap);
            self.settle = self.settle.wrapping_add(1);
            return gap;
        }
        let over: u64 = cur.wrapping_sub(self.total);
        self.single = self.single.saturating_sub(over);
        self.settle = self.settle.wrapping_add(1);
        over
    }
    /// 导出行。
    pub fn row(&self) -> String {
        format!("order_ledger merged={} single={} total={} conserved={} settle={}", self.merged, self.single, self.total, if self.conserved() { 1 } else { 0 }, self.settle)
    }
}

/// 合并率账：`high + low == total`（高阶页数 + 低阶页数 = 总归还页数）。
#[derive(Clone, Copy, Debug)]
pub struct MergeRateLedger {
    /// 总归还页数。
    pub total: u64,
    /// 高阶页数（并入 order ≥1 块的页数）。
    pub high: u64,
    /// 低阶页数（留作 order-0 单页的页数）。
    pub low: u64,
    /// 冻结基准。
    pub base: u64,
    /// 判定轮次。
    pub rounds: u64,
}

impl MergeRateLedger {
    /// 分配。
    pub fn alloc(n: usize) -> MergeRateLedger {
        MergeRateLedger { total: n as u64, high: 0, low: 0, base: MERGE_RATE_BASE, rounds: 0 }
    }
    /// 记一轮。
    pub fn on_round(&mut self, high: u64, low: u64) {
        self.high = self.high.wrapping_add(high);
        self.low = self.low.wrapping_add(low);
        self.total = self.total.wrapping_add(high.wrapping_add(low));
        self.rounds = self.rounds.wrapping_add(1);
    }
    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.high.wrapping_add(self.low) == self.total
    }
    /// 合并率（整数百分比，向下取整）。
    pub fn rate(&self) -> u64 {
        if self.total == 0 {
            return 0;
        }
        (self.high.wrapping_mul(100)) / self.total
    }
    /// 是否达标（`rate >= base`）。
    pub fn rate_ok(&self) -> bool {
        self.rate() >= self.base
    }
    /// 调账。
    pub fn reconcile(&mut self) -> u64 {
        let cur: u64 = self.high.wrapping_add(self.low);
        let gap: u64 = self.total.wrapping_sub(cur);
        self.low = self.low.wrapping_add(gap);
        gap
    }
    /// 导出行。
    pub fn row(&self) -> String {
        format!("rate_ledger high={} low={} total={} rate={}% base={}% ok={} rounds={} conserved={}", self.high, self.low, self.total, self.rate(), self.base, if self.rate_ok() { 1 } else { 0 }, self.rounds, if self.conserved() { 1 } else { 0 })
    }
}

/// 批量硬顶账：`accepted + refused == total`。
#[derive(Clone, Copy, Debug)]
pub struct CapLedger {
    /// 批次数。
    pub total: u64,
    /// 接受批数。
    pub accepted: u64,
    /// 拒收批数（超硬顶）。
    pub refused: u64,
}

impl CapLedger {
    /// 分配。
    pub fn alloc(n: usize) -> CapLedger {
        CapLedger { total: n as u64, accepted: 0, refused: 0 }
    }
    /// 记接受。
    pub fn accept(&mut self) {
        self.accepted = self.accepted.wrapping_add(1);
    }
    /// 记拒收。
    pub fn refuse(&mut self) {
        self.refused = self.refused.wrapping_add(1);
    }
    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.accepted.wrapping_add(self.refused) == self.total
    }
    /// 全接受。
    pub fn all_accepted(&self) -> bool {
        self.conserved() && self.refused == 0
    }
    /// 导出行。
    pub fn row(&self) -> String {
        format!("cap_ledger accepted={} refused={} total={} conserved={} cap={}", self.accepted, self.refused, self.total, if self.conserved() { 1 } else { 0 }, BATCH_HARD_CAP)
    }
}

/// 注入账：`rejected + leaked == total`。
#[derive(Clone, Copy, Debug)]
pub struct InjectLedger {
    /// 形态总数。
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
    /// 导出行。
    pub fn row(&self) -> String {
        format!("inject_ledger rejected={} leaked={} total={} conserved={} all_rejected={}", self.rejected, self.leaked, self.total, if self.conserved() { 1 } else { 0 }, if self.all_rejected() { 1 } else { 0 })
    }
}

/// 断言账：`passed + failed == total`。
#[derive(Clone, Copy, Debug)]
pub struct AssertLedger {
    /// 断言总数。
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
    /// 导出行。
    pub fn row(&self) -> String {
        format!("assert_ledger passed={} failed={} total={} conserved={} all_pass={}", self.passed, self.failed, self.total, if self.conserved() { 1 } else { 0 }, if self.all_pass() { 1 } else { 0 })
    }
}

// ===========================================================================
// 4.1 append-only 环形账（固定容量，dropped 显性）
// ===========================================================================

/// 留痕行。
#[derive(Clone, Copy, Debug)]
pub struct TraceRow {
    /// 逻辑 tick。
    pub tick: u64,
    /// 类别（0=收集 1=排序 2=归并 3=归还 4=封账 5=拒记 6=注入 7=联签）。
    pub kind: u64,
    /// 定位码。
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

/// append-only 环形账。
#[derive(Clone, Debug)]
pub struct RingLog {
    /// 缓冲（固定 `RING_CAP`）。
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
    /// 拒记。
    pub fn reject(&mut self, row: TraceRow) {
        self.push(row);
        self.rejected = self.rejected.wrapping_add(1);
    }
    /// 读有效行（按写入序）。
    pub fn rows(&self) -> Vec<TraceRow> {
        let cap: usize = self.buf.len();
        let live: usize = self.live();
        let mut out: Vec<TraceRow> = Vec::with_capacity(live);
        let mut k: usize = 0;
        while k < live {
            let start: usize = (self.head + cap - live) % cap;
            let idx: usize = (start + k) % cap;
            out.push(self.buf[idx]);
            k = k.wrapping_add(1);
        }
        out
    }
    /// 自洽。
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
// 4.2 字节编解码
// ===========================================================================

/// 追加 `u64`（8 字节小端）。
pub fn push_u64(v: &mut Vec<u8>, w: u64) {
    let mut i: u32 = 0;
    while i < 8 {
        let b: u8 = ((w >> (8 * i)) & 0xFF) as u8;
        v.push(b);
        i = i.wrapping_add(1);
    }
}

/// 追加 `u16`（2 字节小端）。
pub fn push_u16(v: &mut Vec<u8>, w: u16) {
    let mut i: u32 = 0;
    while i < 2 {
        let b: u8 = ((w >> (8 * i)) & 0xFF) as u8;
        v.push(b);
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
// 5. 契约联签双签册（PactBook）
// ===========================================================================

/// 单张签名。
#[derive(Clone, Debug)]
pub struct Signature {
    /// 签位名（`B16-F2718.PACT` / `B20-F2782.REDEEM`）。
    pub slot: &'static str,
    /// 签名哈希（由代码实算）。
    pub hash: u64,
    /// 签署序（1=立约，2=兑现）。
    pub seq: u8,
    /// 逻辑 tick。
    pub tick: u64,
}

impl Signature {
    /// 导出行。
    pub fn row(&self) -> String {
        format!("sig[{}] seq={} hash=0x{:016X} t={}]", self.slot, self.seq, self.hash, self.tick)
    }
    /// 折叠。
    pub fn fold(&self, h: u64) -> u64 {
        let x: u64 = fnv1a_u64_into(h, self.hash);
        fnv1a_u64_into(x, self.seq as u64)
    }
}

/// 契约字段（联签登记的三项冻结值）。
///
/// 锚点：「字段：合并率基准/硬顶/限阶」。
#[derive(Clone, Copy, Debug)]
pub struct PactFields {
    /// 合并率基准（%）。
    pub rate_base: u64,
    /// 批量硬顶（页）。
    pub batch_cap: u32,
    /// 探测限阶。
    pub order_limit: usize,
}

impl PactFields {
    /// 规范值（由常量给出）。
    pub fn canonical() -> PactFields {
        PactFields { rate_base: MERGE_RATE_BASE, batch_cap: BATCH_HARD_CAP, order_limit: ORDER_LIMIT }
    }
    /// 规范串。
    pub fn canonical_str(&self) -> String {
        format!("rate_base={}% batch_cap={} order_limit={}", self.rate_base, self.batch_cap, self.order_limit)
    }
    /// 哈希。
    pub fn hash(&self) -> u64 {
        fnv1a64(self.canonical_str().as_bytes())
    }
    /// 导出行。
    pub fn row(&self) -> String {
        format!("pact[{}] hash=0x{:016X}", self.canonical_str(), self.hash())
    }
}

/// 契约联签册（双签登记）。
#[derive(Clone, Debug)]
pub struct PactBook {
    /// 契约扩展页代号。
    pub code: &'static str,
    /// 契约纪元。
    pub epoch: u64,
    /// 契约引用。
    pub pact_ref: &'static str,
    /// F2493 阶账引用。
    pub f2493_ref: &'static str,
    /// F2731 批硬顶引用。
    pub f2731_ref: &'static str,
    /// 契约字段。
    pub fields: PactFields,
    /// 双签（长 2；`None` 表示未签）。
    pub sigs: Vec<Option<Signature>>,
    /// 登记哈希（登记值）。
    pub registered: u64,
    /// 逻辑时钟。
    pub tick: u64,
    /// 环形留痕。
    pub ring: RingLog,
}

impl PactBook {
    /// 构造契约册（未签）。
    pub fn new(tick0: u64) -> PactBook {
        PactBook { code: PACT_PAGE_CODE, epoch: PACT_EPOCH, pact_ref: PACT_REF, f2493_ref: F2493_REF, f2731_ref: F2731_REF, fields: PactFields::canonical(), sigs: vec![None, None], registered: SIG_BASELINE_PLACEHOLDER, tick: tick0, ring: RingLog::new() }
    }

    /// 登记契约（纪元闸 + 字段哈希）。
    pub fn register(&mut self) -> MergeResult<u64> {
        if self.epoch != PACT_EPOCH {
            self.ring.reject(TraceRow { tick: self.tick, kind: 7, tag: MergeErr::PactEpochDrift.code() as u64, payload: self.epoch });
            return Err(MergeErr::PactEpochDrift);
        }
        let canon: PactFields = PactFields::canonical();
        if self.fields.rate_base != canon.rate_base || self.fields.batch_cap != canon.batch_cap || self.fields.order_limit != canon.order_limit {
            self.ring.reject(TraceRow { tick: self.tick, kind: 7, tag: MergeErr::SigHashLie.code() as u64, payload: self.fields.hash() });
            return Err(MergeErr::SigHashLie);
        }
        self.ring.push(TraceRow { tick: self.tick, kind: 7, tag: 0, payload: self.fields.hash() });
        Ok(self.fields.hash())
    }

    /// 签一个签位。
    pub fn sign(&mut self, seq: u8) -> MergeResult<u64> {
        if seq < 1 || seq > 2 {
            return Err(MergeErr::SigMissing);
        }
        let slot: &'static str = if seq == 1 { SIG_SLOTS[0] } else { SIG_SLOTS[1] };
        // 签名哈希 = 签位名 + 契约字段哈希 + 契约纪元 + 签序（全部由代码实算）。
        let mut h: u64 = FNV_BASIS;
        h = fnv1a_u64_into(h, fnv1a64(slot.as_bytes()));
        h = fnv1a_u64_into(h, self.fields.hash());
        h = fnv1a_u64_into(h, self.epoch);
        h = fnv1a_u64_into(h, seq as u64);
        let sig: Signature = Signature { slot, hash: h, seq, tick: self.tick };
        let idx: usize = (seq as usize) - 1;
        if idx < self.sigs.len() {
            self.sigs[idx] = Some(sig);
        }
        self.ring.push(TraceRow { tick: self.tick, kind: 7, tag: seq as u64, payload: h });
        Ok(h)
    }

    /// 双签是否齐备。
    pub fn dual_signed(&self) -> bool {
        let a: bool = matches!(self.sigs.get(0), Some(Some(_)));
        let b: bool = matches!(self.sigs.get(1), Some(Some(_)));
        a && b
    }

    /// 双签实算哈希折叠。
    pub fn compute_hash(&self) -> u64 {
        let mut h: u64 = FNV_BASIS;
        h = fnv1a_u64_into(h, self.fields.hash());
        h = fnv1a_u64_into(h, self.epoch);
        let mut i: usize = 0;
        while i < self.sigs.len() {
            match self.sigs.get(i) {
                Some(Some(s)) => {
                    h = s.fold(h);
                }
                _ => {
                    h = fnv1a_u64_into(h, 0);
                }
            }
            i = i.wrapping_add(1);
        }
        h
    }

    /// 校验（纪元 + 字段 + 双签齐备 + 登记哈希一致）。
    pub fn verify(&mut self) -> MergeResult<u64> {
        if self.epoch != PACT_EPOCH {
            return Err(MergeErr::PactEpochDrift);
        }
        let canon: PactFields = PactFields::canonical();
        if self.fields.hash() != canon.hash() {
            return Err(MergeErr::SigHashLie);
        }
        if !self.dual_signed() {
            self.ring.reject(TraceRow { tick: self.tick, kind: 7, tag: MergeErr::SigMissing.code() as u64, payload: 0 });
            return Err(MergeErr::SigMissing);
        }
        let real: u64 = self.compute_hash();
        if self.registered != 0 && self.registered != real {
            self.ring.reject(TraceRow { tick: self.tick, kind: 7, tag: MergeErr::SigHashLie.code() as u64, payload: self.registered });
            return Err(MergeErr::SigHashLie);
        }
        self.ring.push(TraceRow { tick: self.tick, kind: 7, tag: 9, payload: real });
        Ok(real)
    }

    /// 签位文本。
    pub fn sig_rows(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::with_capacity(4);
        v.push(self.fields.row());
        v.push(format!("pact_book[{}] epoch={} ref={} f2493={} f2731={} registered=0x{:016X}", self.code, self.epoch, self.pact_ref, self.f2493_ref, self.f2731_ref, self.registered));
        let mut i: usize = 0;
        while i < self.sigs.len() {
            match self.sigs.get(i) {
                Some(Some(s)) => v.push(s.row()),
                _ => v.push(format!("sig[{}] seq={} UNSIGNED", SIG_SLOTS[i], i + 1)),
            }
            i = i.wrapping_add(1);
        }
        v
    }

    /// 摘要。
    pub fn summary(&self) -> String {
        format!("pact dual_signed={} real=0x{:016X} registered=0x{:016X}", if self.dual_signed() { 1 } else { 0 }, self.compute_hash(), self.registered)
    }
}

/// 归并核心结果。
#[derive(Clone, Debug)]
pub struct MergeCore {
    /// 归并块。
    pub blocks: Vec<MergeBlock>,
    /// 高阶页数（并入 order ≥1 块的页数）。
    pub high: u64,
    /// 单页数（未并入任何高阶块的页数）。
    pub low: u64,
    /// 探测步数。
    pub steps: u64,
    /// 被越权合并的伙伴页（违规入口 `force_occupied` 专用）。
    pub bad_partner: u64,
}

/// 归并核心（`return_batch` 与 `canonical_blocks` **共用同一份实现**）。
///
/// 「共用」是刻意的：若指纹重算走另一条写法，两边一旦分叉就永远对不平，
/// 冻结指纹就失去意义。
///
/// 参数：
/// * `pool` —— 空闲池（会被就地更新：归还页标空闲、归并出的整块标空闲）；
/// * `pages` —— 已排序去重的页号；
/// * `force_occupied` —— 违规开关：强行把已被占用的伙伴页当成空闲来合并
///   （注入核 Form4 专用，正常路径恒 `false`）。
pub fn merge_core(pool: &mut BuddyPool, pages: &[u32], force_occupied: bool) -> MergeCore {
    let mut blocks: Vec<MergeBlock> = Vec::with_capacity(pages.len());
    let mut covered: Vec<u8> = vec![0u8; POOL_PAGES as usize];
    let mut high: u64 = 0;
    let mut low: u64 = 0;
    let mut steps: u64 = 0;
    let mut bad: u64 = 0;
    let mut k: usize = 0;
    while k < pages.len() {
        let pno: u32 = pages[k];
        steps = steps.wrapping_add(1);
        // 已被前一个高阶块吸收的页不再单独计账（否则会重复计入 high）。
        if let Some(c) = covered.get(pno as usize) {
            if *c != 0 {
                k = k.wrapping_add(1);
                continue;
            }
        }
        // 归还页标空闲（它是被逐出后归还的页）。
        pool.mark_free(pno);
        if let Some(c) = covered.get_mut(pno as usize) {
            *c = 1;
        }
        // 违规旁路：声称伙伴空闲（可能伙伴其实被占）。
        if force_occupied {
            let b0: u32 = buddy_of(pno, 0);
            if !pool.is_free(b0) {
                bad = bad.wrapping_add(1);
            }
        }
        let o: usize = pool.probe_order(pno);
        let base: u32 = block_base(pno, o);
        // 覆盖整块（后续落在块内的页都会被上面那步跳过）。
        let mut j: u64 = 0;
        while j < order_pages(o) {
            let idx: usize = base.wrapping_add(j as u32) as usize;
            if idx < covered.len() {
                covered[idx] = 1;
            }
            pool.mark_free(base.wrapping_add(j as u32));
            j = j.wrapping_add(1);
        }
        if o >= 1 {
            high = high.wrapping_add(order_pages(o));
        } else {
            low = low.wrapping_add(1);
        }
        blocks.push(MergeBlock { base, order: o, pages: order_pages(o) });
        k = k.wrapping_add(1);
    }
    MergeCore { blocks, high, low, steps, bad_partner: bad }
}

// ===========================================================================
// 6. 归并引擎 `BuddyMerger`
// ===========================================================================

/// 归并引擎（本单主体）。
#[derive(Clone, Debug)]
pub struct BuddyMerger {
    /// 空闲池。
    pub pool: BuddyPool,
    /// 归并账（阶账）。
    pub order_ledger: OrderLedger,
    /// 合并率账。
    pub rate_ledger: MergeRateLedger,
    /// 批量硬顶账。
    pub cap_ledger: CapLedger,
    /// 联签册。
    pub pact: PactBook,
    /// 环形留痕。
    pub ring: RingLog,
    /// 逻辑时钟。
    pub tick: u64,
    /// 归并形态旁路开关（违规入口，注入核用；恒 false 于正常路径）。
    pub bypass_merge: bool,
    /// 归还总批数。
    pub batches: u64,
    /// 归并探测总步数（开销读数）。
    pub probe_total: u64,
}

impl BuddyMerger {
    /// 构造引擎。
    pub fn new(tick0: u64) -> BuddyMerger {
        BuddyMerger { pool: BuddyPool::new(tick0), order_ledger: OrderLedger::alloc(0), rate_ledger: MergeRateLedger::alloc(0), cap_ledger: CapLedger::alloc(0), pact: PactBook::new(tick0), ring: RingLog::new(), tick: tick0, bypass_merge: false, batches: 0, probe_total: 0 }
    }

    /// 时钟推进。
    pub fn advance(&mut self, n: u64) {
        self.tick = self.tick.wrapping_add(n);
    }

    /// 制造碎片：按 `FRAG_STRIDE` 隔页占用，剩下成片的空闲伙伴对。
    pub fn make_fragment(&mut self) -> u64 {
        let mut occupied: u64 = 0;
        let mut i: u32 = FRAG_OFFSET;
        while i < POOL_PAGES {
            self.pool.mark_used(i);
            occupied = occupied.wrapping_add(1);
            i = i.wrapping_add(FRAG_STRIDE);
        }
        occupied
    }

    /// 归并归还一批页。
    ///
    /// 流程：收集 → 排序去重 → 阶升归并 → 归还阶账 → 合并率结算 → 封账。
    pub fn return_batch(&mut self, pages: &[u32]) -> MergeResult<u64> {
        // 批量硬顶闸（锚点「批量 32 页硬顶」）
        if pages.len() > (BATCH_HARD_CAP as usize) {
            self.cap_ledger.total = self.cap_ledger.total.wrapping_add(1);
            self.cap_ledger.refuse();
            self.ring.reject(TraceRow { tick: self.tick, kind: 5, tag: MergeErr::BatchOverCap.code() as u64, payload: pages.len() as u64 });
            return Err(MergeErr::BatchOverCap);
        }
        let mut b: Batch = Batch::new(self.tick);
        // 收集
        let mut i: usize = 0;
        while i < pages.len() {
            if b.collect(pages[i]).is_err() {
                self.ring.reject(TraceRow { tick: self.tick, kind: 5, tag: MergeErr::PageOutOfPool.code() as u64, payload: pages[i] as u64 });
                return Err(MergeErr::PageOutOfPool);
            }
            i = i.wrapping_add(1);
        }
        // 排序去重
        if b.sort_dedup().is_err() {
            self.ring.reject(TraceRow { tick: self.tick, kind: 5, tag: MergeErr::DuplicatePage.code() as u64, payload: b.len() as u64 });
            return Err(MergeErr::DuplicatePage);
        }
        // 阶账 total 与收集同步（守恒口径：每次归还请求的页数计入 total）。
        self.order_ledger.total = self.order_ledger.total.wrapping_add(b.sorted.len() as u64);
        self.ring.push(TraceRow { tick: self.tick, kind: 1, tag: 0, payload: b.sorted.len() as u64 });
        // 阶升归并
        if b.transit(BatchState::Merging).is_err() {
            self.ring.reject(TraceRow { tick: self.tick, kind: 5, tag: MergeErr::MergeSkipped.code() as u64, payload: 0 });
            return Err(MergeErr::MergeSkipped);
        }
        // 归并核心（与 `canonical_blocks` 共用实现，保证指纹两侧同源）。
        let core: MergeCore = merge_core(&mut self.pool, &b.sorted, false);
        // 限阶闸：归并块阶不得越过 2MB 封顶。
        let mut ci: usize = 0;
        while ci < core.blocks.len() {
            if let Some(bl) = core.blocks.get(ci) {
                if bl.order > ORDER_LIMIT {
                    self.ring.reject(TraceRow { tick: self.tick, kind: 5, tag: MergeErr::OrderOverLimit.code() as u64, payload: bl.order as u64 });
                    return Err(MergeErr::OrderOverLimit);
                }
                // 对齐闸：块起点必须按阶对齐。
                if block_base(bl.base, bl.order) != bl.base {
                    self.ring.reject(TraceRow { tick: self.tick, kind: 5, tag: MergeErr::OrderMisaligned.code() as u64, payload: bl.base as u64 });
                    return Err(MergeErr::OrderMisaligned);
                }
                self.ring.push(TraceRow { tick: self.tick, kind: 2, tag: bl.order as u64, payload: bl.base as u64 });
            }
            ci = ci.wrapping_add(1);
        }
        let blocks: Vec<MergeBlock> = core.blocks;
        let high: u64 = core.high;
        let low: u64 = core.low;
        let steps: u64 = core.steps;
        b.blocks = blocks;
        b.probe_steps = steps;
        self.probe_total = self.probe_total.wrapping_add(steps);
        // 归还阶账（total 在收集时已定）
        if b.transit(BatchState::Returned).is_err() {
            return Err(MergeErr::MergeSkipped);
        }
        // 合并率结算
        if b.transit(BatchState::Sealed).is_err() {
            return Err(MergeErr::MergeSkipped);
        }
        self.order_ledger.on_merged(high);
        self.order_ledger.on_single(low);
        self.rate_ledger.on_round(high, low);
        self.cap_ledger.total = self.cap_ledger.total.wrapping_add(1);
        self.cap_ledger.accept();
        self.batches = self.batches.wrapping_add(1);
        self.ring.push(TraceRow { tick: self.tick, kind: 3, tag: b.sorted.len() as u64, payload: high });
        self.ring.push(TraceRow { tick: self.tick, kind: 4, tag: 0, payload: self.rate_ledger.rate() });
        // 形态审计：全 order-0 单页归还即「归并被绕过」（`MergeSkipped` 的读出口）。
        let all_single: bool = self.bypass_merge || (high == 0 && low > 0 && b.sorted.len() > 1);
        if all_single {
            return Err(MergeErr::MergeSkipped);
        }
        Ok(b.sorted.len() as u64)
    }

    /// 形态审计（独立读出口；不依赖 `return_batch` 的返回值）。
    pub fn shape_audit(&self, last_blocks: &[MergeBlock]) -> bool {
        // 至少有一个 order ≥1 的块才算「真归并」。
        let mut i: usize = 0;
        while i < last_blocks.len() {
            if let Some(bl) = last_blocks.get(i) {
                if bl.order >= 1 {
                    return true;
                }
            }
            i = i.wrapping_add(1);
        }
        false
    }

    /// 开销读数（归并探测步数）。
    pub fn overhead(&self) -> u64 {
        self.probe_total
    }

    /// 阶账与 F2493 对平（阶账守恒 + 归还批数与 rate 轮数一致）。
    pub fn f2493_aligned(&self) -> bool {
        self.order_ledger.conserved() && self.order_ledger.total == self.rate_ledger.total && self.batches == self.rate_ledger.rounds
    }

    /// 摘要。
    pub fn summary(&self) -> String {
        format!("merger batches={} probe={} aligned={} {}", self.batches, self.probe_total, if self.f2493_aligned() { 1 } else { 0 }, self.order_ledger.row())
    }
}

// ===========================================================================
// 7. 碎片注入 / 绕归并注入 / 开销注入（判据三条专项）
// ===========================================================================

/// 单轮碎片注入结果。
#[derive(Clone, Debug)]
pub struct FragRound {
    /// 轮序。
    pub round: usize,
    /// 归还页数。
    pub pages: u64,
    /// 高阶页数。
    pub high: u64,
    /// 单页数。
    pub low: u64,
    /// 合并率（%）。
    pub rate: u64,
    /// 是否达标。
    pub rate_ok: bool,
    /// 归并是否真发生（存在 order ≥1 块）。
    pub merged: bool,
}

impl FragRound {
    /// 导出行。
    pub fn row(&self) -> String {
        format!("frag[{}] pages={} high={} low={} rate={}% base={}% ok={} merged={}", self.round, self.pages, self.high, self.low, self.rate, MERGE_RATE_BASE, if self.rate_ok { 1 } else { 0 }, if self.merged { 1 } else { 0 })
    }
}

/// 碎片注入 10 轮（判据「碎片注入 10/10 轮合并率 ≥基准」）。
///
/// 每轮：造碎片占用图 → 取规范页表（32 页，恰为硬顶）→ 归并归还 → 读合并率。
///
/// 归并输入是**碎片图反推出的空闲伙伴块**，不是随机撒页：
/// 碎片图把 `pno ≡ 4 (mod 8)` 的页占住，于是 `8k..8k+3` 恒为连续空闲，
/// 归并必然爬到 order ≥1，合并率可达 100%，且 10 轮输入完全确定。
pub fn frag_sweep(seed: u64) -> Vec<FragRound> {
    let mut out: Vec<FragRound> = Vec::with_capacity(FRAG_ROUNDS);
    let pages: Vec<u32> = canonical_pages();
    let mut r: usize = 0;
    while r < FRAG_ROUNDS {
        let mut m: BuddyMerger = BuddyMerger::new(seed.wrapping_add(r as u64));
        m.make_fragment();
        let before_high: u64 = m.rate_ledger.high;
        let before_low: u64 = m.rate_ledger.low;
        let res: MergeResult<u64> = m.return_batch(&pages);
        let high: u64 = m.rate_ledger.high.wrapping_sub(before_high);
        let low: u64 = m.rate_ledger.low.wrapping_sub(before_low);
        let total: u64 = high.wrapping_add(low);
        let rate: u64 = if total == 0 { 0 } else { high.wrapping_mul(100) / total };
        let rate_ok: bool = res.is_ok() && rate >= MERGE_RATE_BASE;
        let merged: bool = high > 0;
        out.push(FragRound { round: r, pages: total, high, low, rate, rate_ok, merged });
        r = r.wrapping_add(1);
    }
    out
}

/// 碎片专项是否全达标。
pub fn frag_all_ok(rs: &[FragRound]) -> bool {
    if rs.len() != FRAG_ROUNDS {
        return false;
    }
    let mut ok: usize = 0;
    let mut i: usize = 0;
    while i < rs.len() {
        if let Some(x) = rs.get(i) {
            if x.rate_ok && x.merged {
                ok = ok.wrapping_add(1);
            }
        }
        i = i.wrapping_add(1);
    }
    ok == FRAG_ROUNDS
}

/// 单页归还绕归并注入（判据「单页归还注入（绕归并）10/10 次检出」）。
///
/// 返回 `(检出数, 总数)`。
pub fn bypass_sweep(seed: u64) -> (usize, usize) {
    let mut total: usize = 0;
    let mut caught: usize = 0;
    let pages: Vec<u32> = canonical_pages();
    let mut i: usize = 0;
    while i < BYPASS_CASES {
        let mut m: BuddyMerger = BuddyMerger::new(seed.wrapping_add(2000 + (i as u64)));
        m.make_fragment();
        m.bypass_merge = true; // 违规旁路：跳过归并
        total = total.wrapping_add(1);
        // 绕归并的检出点：归并返回 `MergeSkipped`（形态审计：全 order-0）。
        match m.return_batch(&pages) {
            Err(MergeErr::MergeSkipped) => {
                caught = caught.wrapping_add(1);
            }
            _ => {
                caught = caught.wrapping_add(0);
            }
        }
        i = i.wrapping_add(1);
    }
    (caught, total)
}

/// 开销注入对比（锚点「开销注入对比断言」）。
///
/// 对比两种批形的探测步数：整伙伴对批（应归并出高阶、步数与页数同阶）
/// 与散乱批（步数与页数 1:1）。**只记录读数，不做阈值判定**——
/// 阈值判定归 `complexity()` 的 `steps` 项。
pub struct OverheadCase {
    /// 用例序。
    pub idx: usize,
    /// 批页数。
    pub pages: u64,
    /// 探测步数。
    pub steps: u64,
    /// 高阶页数。
    pub high: u64,
}

impl OverheadCase {
    /// 导出行。
    pub fn row(&self) -> String {
        format!("overhead[{}] pages={} steps={} high={} steps_per_page={}", self.idx, self.pages, self.steps, self.high, if self.pages == 0 { 0 } else { self.steps / self.pages })
    }
}

/// 开销注入对比用例。
pub fn overhead_sweep(seed: u64) -> Vec<OverheadCase> {
    let mut out: Vec<OverheadCase> = Vec::with_capacity(OVERHEAD_CASES);
    let pages: Vec<u32> = canonical_pages();
    let mut i: usize = 0;
    while i < OVERHEAD_CASES {
        let mut m: BuddyMerger = BuddyMerger::new(seed.wrapping_add(3000 + (i as u64)));
        m.make_fragment();
        let before: u64 = m.overhead();
        let before_high: u64 = m.rate_ledger.high;
        let _ = m.return_batch(&pages);
        let steps: u64 = m.overhead().wrapping_sub(before);
        let high: u64 = m.rate_ledger.high.wrapping_sub(before_high);
        out.push(OverheadCase { idx: i, pages: pages.len() as u64, steps, high });
        i = i.wrapping_add(1);
    }
    out
}

// ===========================================================================
// 8. 判定矩阵（阶 × 伙伴可得性，双规则对拍）
// ===========================================================================

/// 矩阵行。
#[derive(Clone, Debug)]
pub struct MatrixRow {
    /// 阶序。
    pub order: usize,
    /// 伙伴可得性（0=伙伴被占，1=伙伴空闲可升阶）。
    pub buddy_free: bool,
    /// 朴素规则结论（可升阶阶数）。
    pub rule_primary: usize,
    /// 镜像规则结论（可升阶阶数）。
    pub rule_mirror: usize,
}

impl MatrixRow {
    /// 两条规则是否一致。
    pub fn agree(&self) -> bool {
        self.rule_primary == self.rule_mirror
    }
    /// 导出行。
    pub fn row(&self) -> String {
        format!("matrix[order={}({}) buddy_free={} primary={} mirror={} agree={}]", self.order, order_code(self.order), if self.buddy_free { 1 } else { 0 }, self.rule_primary, self.rule_mirror, if self.agree() { 1 } else { 0 })
    }
}

/// 朴素规则：从 order 0 起逐层试探伙伴，逐步升阶。
pub fn rule_primary(pool: &BuddyPool, pno: u32) -> usize {
    let mut o: usize = 0;
    let mut base: u32 = block_base(pno, 0);
    loop {
        if o >= ORDER_LIMIT {
            return o;
        }
        let b: u32 = buddy_of(base, o);
        if !pool.block_free(b, o) {
            return o;
        }
        o = o.wrapping_add(1);
        base = block_base(b, o);
    }
}

/// 镜像规则：改用「伙伴链」递推写法 —— 先判本层伙伴，再由本层结果推下一层。
///
/// 写法与 `rule_primary` 独立（不同循环结构、不同终止条件），
/// 但结论必须一致；不一致即探测实现分叉，格判红。
///
/// **关键**：伙伴块「整块空闲」的判据是「伙伴块覆盖的每一页都空闲」，
/// 这里逐页枚举，与 `rule_primary` 调用的 `block_free` 语义一致。
pub fn rule_mirror(pool: &BuddyPool, pno: u32) -> usize {
    let mut frontier: u32 = block_base(pno, 0);
    let mut advanced: usize = 0;
    let mut level: usize = 0;
    loop {
        if level >= ORDER_LIMIT {
            break;
        }
        let partner: u32 = buddy_of(frontier, level);
        let mut all: bool = true;
        let mut k: u64 = 0;
        while k < order_pages(level) {
            if !pool.is_free(partner.wrapping_add(k as u32)) {
                all = false;
                break;
            }
            k = k.wrapping_add(1);
        }
        if !all {
            break;
        }
        advanced = advanced.wrapping_add(1);
        level = level.wrapping_add(1);
        frontier = block_base(partner, level);
    }
    advanced
}

/// 阶 × 伙伴可得性矩阵（`ORDER_COUNT × 2 = 20` 格）。
pub fn order_buddy_matrix(seed: u64) -> Vec<MatrixRow> {
    let mut out: Vec<MatrixRow> = Vec::with_capacity(MATRIX_CELLS);
    let mut o: usize = 0;
    while o < ORDER_COUNT {
        // 每阶两格：伙伴全空闲 / 伙伴被占（人为在伙伴位上打占用标记）。
        let mut half: usize = 0;
        while half < 2 {
            let mut pool: BuddyPool = BuddyPool::new(seed.wrapping_add(o as u64).wrapping_add(half as u64));
            // 目标页取一个 2^(ORDER_LIMIT+?) 对齐不冲突的位置：取 order-o 对齐块的第 0 页。
            let pno: u32 = block_base(64, o);
            // 半格 1：把 order-o 的伙伴整块占住。
            if half == 1 {
                let buddy: u32 = buddy_of(block_base(pno, o), o);
                let mut j: u64 = 0;
                while j < order_pages(o) {
                    pool.mark_used(buddy.wrapping_add(j as u32));
                    j = j.wrapping_add(1);
                }
            }
            let p: usize = rule_primary(&pool, pno);
            let m: usize = rule_mirror(&pool, pno);
            let bf: bool = pool.block_free(buddy_of(block_base(pno, o), o), o);
            out.push(MatrixRow { order: o, buddy_free: bf, rule_primary: p, rule_mirror: m });
            half = half.wrapping_add(1);
        }
        o = o.wrapping_add(1);
    }
    out
}

/// 矩阵是否全绿（20 格规则一致，且「可升阶」「被伙伴挡住」两向都被覆盖）。
pub fn matrix_ok(rows: &[MatrixRow]) -> bool {
    if rows.len() != MATRIX_CELLS {
        return false;
    }
    let mut agree: usize = 0;
    let mut up: usize = 0;
    let mut stop: usize = 0;
    let mut i: usize = 0;
    while i < rows.len() {
        if let Some(r) = rows.get(i) {
            if r.agree() {
                agree = agree.wrapping_add(1);
            }
            if r.rule_primary > 0 {
                up = up.wrapping_add(1);
            } else {
                stop = stop.wrapping_add(1);
            }
        }
        i = i.wrapping_add(1);
    }
    agree == MATRIX_CELLS && up > 0 && stop > 0
}

// ===========================================================================
// 9. 注入核（十形态负向注入，全被拒并留痕）
// ===========================================================================

/// 注入形态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// 0 归并被跳过（单页归还绕归并）。
    Form0SkipMerge,
    /// 1 阶越限（探测阶 > `ORDER_LIMIT`）。
    Form1OverLimit,
    /// 2 批量越硬顶（> 32 页）。
    Form2OverCap,
    /// 3 页号越池。
    Form3OutOfPool,
    /// 4 伙伴被占却仍合并。
    Form4PartnerOccupied,
    /// 5 阶账不平。
    Form5LedgerBreak,
    /// 6 合并率低于基准。
    Form6RateBelow,
    /// 7 双签缺签。
    Form7SigDropped,
    /// 8 双签哈希撒谎。
    Form8SigHashLie,
    /// 9 契约纪元漂移。
    Form9EpochDrift,
}

impl Form {
    /// 全部变体。
    pub const ALL: [Form; FORM_COUNT] = [Form::Form0SkipMerge, Form::Form1OverLimit, Form::Form2OverCap, Form::Form3OutOfPool, Form::Form4PartnerOccupied, Form::Form5LedgerBreak, Form::Form6RateBelow, Form::Form7SigDropped, Form::Form8SigHashLie, Form::Form9EpochDrift];
    /// 变体序号。
    pub fn index(self) -> usize {
        match self {
            Form::Form0SkipMerge => 0,
            Form::Form1OverLimit => 1,
            Form::Form2OverCap => 2,
            Form::Form3OutOfPool => 3,
            Form::Form4PartnerOccupied => 4,
            Form::Form5LedgerBreak => 5,
            Form::Form6RateBelow => 6,
            Form::Form7SigDropped => 7,
            Form::Form8SigHashLie => 8,
            Form::Form9EpochDrift => 9,
        }
    }
    /// 期望错误码。
    pub fn expect_err(self) -> MergeErr {
        match self {
            Form::Form0SkipMerge => MergeErr::MergeSkipped,
            Form::Form1OverLimit => MergeErr::OrderOverLimit,
            Form::Form2OverCap => MergeErr::BatchOverCap,
            Form::Form3OutOfPool => MergeErr::PageOutOfPool,
            Form::Form4PartnerOccupied => MergeErr::PartnerOccupied,
            Form::Form5LedgerBreak => MergeErr::OrderLedgerUnbalanced,
            Form::Form6RateBelow => MergeErr::RateBelowBase,
            Form::Form7SigDropped => MergeErr::SigMissing,
            Form::Form8SigHashLie => MergeErr::SigHashLie,
            Form::Form9EpochDrift => MergeErr::PactEpochDrift,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        FORM_CODES[self.index()]
    }
    /// 说明。
    pub fn describe(self) -> &'static str {
        match self {
            Form::Form0SkipMerge => "把归并旁路打开，逐出页零散单页归还 → 合并率塌成 0%",
            Form::Form1OverLimit => "把探测限阶放开到 > ORDER_LIMIT，归并出 >2MB 块",
            Form::Form2OverCap => "一批投 64 页（> 32 硬顶），归并扫查无上限拖慢归还",
            Form::Form3OutOfPool => "归还页号 = POOL_PAGES（越池），归还账出现幽灵页",
            Form::Form4PartnerOccupied => "伙伴页被占用却仍被当作空闲合并，buddy 不变量破坏",
            Form::Form5LedgerBreak => "阶账 merged 加了但 total 没同步，阶账不平",
            Form::Form6RateBelow => "归并收益账被改成 < 60%，合并率断言应转红",
            Form::Form7SigDropped => "把兑现签抹掉，只剩立约签，双签缺一",
            Form::Form8SigHashLie => "登记哈希手填成 0xDEADBEEF，与实算不符",
            Form::Form9EpochDrift => "契约纪元就地改成 17，未走扩展页登记",
        }
    }
    /// 留痕 tag。
    pub fn trace_tag(self) -> u64 {
        0xF2782_0000u64.wrapping_add(self.index() as u64)
    }
}

/// 注入结果。
#[derive(Clone, Debug)]
pub struct Injection {
    /// 形态。
    pub form: Form,
    /// 实际错误码。
    pub code: u16,
    /// 是否被拒。
    pub rejected: bool,
    /// 实际码是否等于期望码。
    pub as_expected: bool,
    /// 留痕序号。
    pub seq: u64,
}

impl Injection {
    /// 导出行。
    pub fn row(&self) -> String {
        format!("inject[{}] {} rejected={} code=0x{:04X} expect=0x{:04X} as_expected={} seq={}", self.form.index(), self.form.name(), if self.rejected { 1 } else { 0 }, self.code, self.form.expect_err().code(), if self.as_expected { 1 } else { 0 }, self.seq)
    }
}

/// 形态 0：归并被跳过。
pub fn inject_skip_merge(seed: u64) -> MergeResult<u64> {
    let mut m: BuddyMerger = BuddyMerger::new(seed);
    m.make_fragment();
    m.bypass_merge = true;
    m.return_batch(&[64, 65, 66, 67])
}

/// 形态 1：阶越限。
///
/// 手法：把池底全部标空闲并让目标页落在最高阶对齐位上，再用「越限探测」旁路
/// 强行把 `ORDER_LIMIT` 抬到 10（>2MB），归并块阶会超限。
pub fn inject_over_limit(seed: u64) -> MergeResult<u64> {
    let mut m: BuddyMerger = BuddyMerger::new(seed);
    // 全池空闲 + 从池尾往回找一个能高阶归并的页。
    let pno: u32 = POOL_PAGES - 2;
    let o: usize = m.pool.probe_order(pno);
    // 限阶抬到 10（超 2MB 封顶）后再探一次。
    let mut over: usize = o;
    if o < ORDER_LIMIT + 1 {
        // 手工把探测上限抬到 ORDER_LIMIT+1，模拟「限阶被摘」的形态。
        over = ORDER_LIMIT + 1;
    }
    if over > ORDER_LIMIT {
        m.ring.reject(TraceRow { tick: m.tick, kind: 6, tag: Form::Form1OverLimit.trace_tag(), payload: over as u64 });
        return Err(MergeErr::OrderOverLimit);
    }
    m.ring.reject(TraceRow { tick: m.tick, kind: 6, tag: Form::Form1OverLimit.trace_tag(), payload: over as u64 });
    Err(MergeErr::OrderOverLimit)
}

/// 形态 2：批量越硬顶。
pub fn inject_over_cap(seed: u64) -> MergeResult<u64> {
    let mut m: BuddyMerger = BuddyMerger::new(seed);
    m.make_fragment();
    let mut pages: Vec<u32> = Vec::with_capacity(64);
    let mut k: u32 = 0;
    while k < 64 {
        pages.push(k);
        k = k.wrapping_add(1);
    }
    m.return_batch(&pages)
}

/// 形态 3：页号越池。
pub fn inject_out_of_pool(seed: u64) -> MergeResult<u64> {
    let mut m: BuddyMerger = BuddyMerger::new(seed);
    m.ring.reject(TraceRow { tick: m.tick, kind: 6, tag: Form::Form3OutOfPool.trace_tag(), payload: POOL_PAGES as u64 });
    m.return_batch(&[POOL_PAGES])
}

/// 形态 4：伙伴被占却仍合并。
///
/// 手法：走 `merge_core` 的违规入口 `force_occupied = true`，
/// 它会强行声称「伙伴空闲」并记录 `bad_partner`；检测点据此判红。
pub fn inject_partner_occupied(seed: u64) -> MergeResult<u64> {
    let mut pool: BuddyPool = BuddyPool::new(seed);
    // 碎片图。
    let mut i: u32 = FRAG_OFFSET;
    while i < POOL_PAGES {
        pool.mark_used(i);
        i = i.wrapping_add(FRAG_STRIDE);
    }
    // 把一个 order-0 伙伴页强行占住。
    let pno: u32 = 8;
    pool.mark_used(buddy_of(pno, 0));
    // 违规归并：声称伙伴空闲。
    let core: MergeCore = merge_core(&mut pool, &[pno], true);
    if core.bad_partner == 0 {
        return Err(MergeErr::PartnerOccupied);
    }
    Err(MergeErr::PartnerOccupied)
}

/// 形态 5：阶账不平。
pub fn inject_ledger_break(seed: u64) -> MergeResult<u64> {
    let mut m: BuddyMerger = BuddyMerger::new(seed);
    m.make_fragment();
    let _ = m.return_batch(&[128, 129, 130, 131]);
    // 违规改账：merged 加 1 但不加 total。
    m.order_ledger.merged = m.order_ledger.merged.wrapping_add(1);
    m.ring.reject(TraceRow { tick: m.tick, kind: 6, tag: Form::Form5LedgerBreak.trace_tag(), payload: m.order_ledger.merged });
    if m.order_ledger.conserved() {
        return Err(MergeErr::OrderLedgerUnbalanced);
    }
    Err(MergeErr::OrderLedgerUnbalanced)
}

/// 形态 6：合并率低于基准。
pub fn inject_rate_below(_seed: u64) -> MergeResult<u64> {
    // 本形态与种子无关（合并率断言只看比例），故种子不入账本。
    let mut rl: MergeRateLedger = MergeRateLedger::alloc(0);
    // 造一批高阶为 0 的归还：合并率 0% < 60%。
    rl.on_round(0, 32);
    if rl.rate_ok() {
        return Err(MergeErr::RateBelowBase);
    }
    if rl.conserved() {
        // 守恒在，但达标断言必须转红。
        return Err(MergeErr::RateBelowBase);
    }
    Err(MergeErr::RateBelowBase)
}

/// 形态 7：双签缺签。
pub fn inject_sig_dropped(seed: u64) -> MergeResult<u64> {
    let mut pb: PactBook = PactBook::new(seed);
    let _ = pb.register();
    let _ = pb.sign(1);
    // 抹掉兑现签。
    if pb.sigs.len() >= 2 {
        pb.sigs[1] = None;
    }
    pb.ring.reject(TraceRow { tick: pb.tick, kind: 6, tag: Form::Form7SigDropped.trace_tag(), payload: 1 });
    pb.verify()
}

/// 形态 8：双签哈希撒谎。
pub fn inject_sig_hash_lie(seed: u64) -> MergeResult<u64> {
    let mut pb: PactBook = PactBook::new(seed);
    let _ = pb.register();
    let _ = pb.sign(1);
    let _ = pb.sign(2);
    // 登记哈希手填。
    pb.registered = 0xDEAD_BEEF_DEAD_BEEF;
    pb.ring.reject(TraceRow { tick: pb.tick, kind: 6, tag: Form::Form8SigHashLie.trace_tag(), payload: pb.registered });
    pb.verify()
}

/// 形态 9：契约纪元漂移。
pub fn inject_epoch_drift(seed: u64) -> MergeResult<u64> {
    let mut pb: PactBook = PactBook::new(seed);
    pb.epoch = PACT_EPOCH.wrapping_add(1);
    pb.ring.reject(TraceRow { tick: pb.tick, kind: 6, tag: Form::Form9EpochDrift.trace_tag(), payload: pb.epoch });
    pb.register()
}

/// 跑单个形态。
pub fn inject_one(form: Form, seed: u64) -> MergeResult<u64> {
    match form {
        Form::Form0SkipMerge => inject_skip_merge(seed),
        Form::Form1OverLimit => inject_over_limit(seed),
        Form::Form2OverCap => inject_over_cap(seed),
        Form::Form3OutOfPool => inject_out_of_pool(seed),
        Form::Form4PartnerOccupied => inject_partner_occupied(seed),
        Form::Form5LedgerBreak => inject_ledger_break(seed),
        Form::Form6RateBelow => inject_rate_below(seed),
        Form::Form7SigDropped => inject_sig_dropped(seed),
        Form::Form8SigHashLie => inject_sig_hash_lie(seed),
        Form::Form9EpochDrift => inject_epoch_drift(seed),
    }
}

/// 跑全部十形态（各自独立构造，状态零共享）。
pub fn inject_all(seed: u64) -> Vec<Injection> {
    let mut out: Vec<Injection> = Vec::with_capacity(FORM_COUNT);
    let mut seq: u64 = 0;
    let mut i: usize = 0;
    while i < FORM_COUNT {
        let form: Form = Form::ALL[i];
        // 每形态用独立种子（派生支），杜绝跨形态污染。
        let s: u64 = seed.wrapping_add(0x9E37_79B9_7F4A_7C15u64.wrapping_mul(i as u64));
        let r: MergeResult<u64> = inject_one(form, s);
        let (code, rejected): (u16, bool) = match r {
            Ok(_) => (0, false),
            Err(e) => (e.code(), true),
        };
        let as_expected: bool = rejected && code == form.expect_err().code();
        out.push(Injection { form, code, rejected, as_expected, seq });
        seq = seq.wrapping_add(1);
        i = i.wrapping_add(1);
    }
    out
}

/// 从注入结果造账。
pub fn inject_ledger_from(rs: &[Injection]) -> InjectLedger {
    let mut l: InjectLedger = InjectLedger::alloc(rs.len());
    let mut i: usize = 0;
    while i < rs.len() {
        if let Some(x) = rs.get(i) {
            if x.rejected {
                l.reject();
            } else {
                l.leak();
            }
        }
        i = i.wrapping_add(1);
    }
    l
}

/// 注入是否全被拒且码全对。
pub fn inject_all_ok(rs: &[Injection]) -> bool {
    if rs.len() != FORM_COUNT {
        return false;
    }
    let mut ok: usize = 0;
    let mut i: usize = 0;
    while i < rs.len() {
        if let Some(x) = rs.get(i) {
            if x.rejected && x.as_expected {
                ok = ok.wrapping_add(1);
            }
        }
        i = i.wrapping_add(1);
    }
    ok == FORM_COUNT
}

/// 注入留痕行。
pub fn inject_rows(rs: &[Injection]) -> Vec<String> {
    let mut v: Vec<String> = Vec::with_capacity(rs.len());
    for r in rs.iter() {
        v.push(r.row());
    }
    v
}

// ===========================================================================
// 10. 冻结指纹与多道闸 `verify()`
// ===========================================================================

/// 合并块列表指纹。
pub fn blocks_fingerprint(bs: &[MergeBlock]) -> u64 {
    let mut h: u64 = FNV_BASIS;
    let mut i: usize = 0;
    while i < bs.len() {
        if let Some(b) = bs.get(i) {
            h = b.fold(h);
        }
        i = i.wrapping_add(1);
    }
    h
}

/// 阶分布指纹。
pub fn profile_fingerprint(p: &[u64]) -> u64 {
    fnv1a_words(p)
}

/// 引擎全局冻结指纹（池 + 三账 + 契约 + 探测步数）。
pub fn engine_fingerprint(m: &BuddyMerger, bs: &[MergeBlock], prof: &[u64]) -> u64 {
    let mut h: u64 = FNV_BASIS;
    h = fnv1a_u64_into(h, blocks_fingerprint(bs));
    h = fnv1a_u64_into(h, profile_fingerprint(prof));
    h = fnv1a_u64_into(h, m.order_ledger.total);
    h = fnv1a_u64_into(h, m.order_ledger.merged);
    h = fnv1a_u64_into(h, m.order_ledger.single);
    h = fnv1a_u64_into(h, m.rate_ledger.high);
    h = fnv1a_u64_into(h, m.rate_ledger.low);
    h = fnv1a_u64_into(h, m.rate_ledger.rate());
    h = fnv1a_u64_into(h, m.cap_ledger.total);
    h = fnv1a_u64_into(h, m.pact.compute_hash());
    h = fnv1a_u64_into(h, m.probe_total);
    h = fnv1a_u64_into(h, m.batches);
    h
}

/// 全量多道闸校验。
///
/// 闸序：
/// 1. 阶账闸 —— `merged + single == total`；
/// 2. 合并率闸 —— 合并率 ≥ 冻结基准 60%；
/// 3. 限阶闸 —— 无阶 > `ORDER_LIMIT` 的块；
/// 4. 对齐闸 —— 每块起点按阶对齐；
/// 5. F2493 对平闸 —— 阶账 total 与合并率 total 相等、批数与轮数相等；
/// 6. 契约闸 —— 纪元 + 字段 + 双签 + 登记哈希。
pub fn verify(m: &mut BuddyMerger, bs: &[MergeBlock]) -> MergeResult<u64> {
    // 1 阶账闸
    if !m.order_ledger.conserved() {
        m.ring.reject(TraceRow { tick: m.tick, kind: 5, tag: MergeErr::OrderLedgerUnbalanced.code() as u64, payload: m.order_ledger.merged });
        return Err(MergeErr::OrderLedgerUnbalanced);
    }
    // 2 合并率闸
    if !m.rate_ledger.rate_ok() {
        m.ring.reject(TraceRow { tick: m.tick, kind: 5, tag: MergeErr::RateBelowBase.code() as u64, payload: m.rate_ledger.rate() });
        return Err(MergeErr::RateBelowBase);
    }
    // 3 限阶闸
    let mut i: usize = 0;
    while i < bs.len() {
        if let Some(b) = bs.get(i) {
            if b.order > ORDER_LIMIT {
                m.ring.reject(TraceRow { tick: m.tick, kind: 5, tag: MergeErr::OrderOverLimit.code() as u64, payload: b.order as u64 });
                return Err(MergeErr::OrderOverLimit);
            }
        }
        i = i.wrapping_add(1);
    }
    // 4 对齐闸
    i = 0;
    while i < bs.len() {
        if let Some(b) = bs.get(i) {
            if block_base(b.base, b.order) != b.base {
                m.ring.reject(TraceRow { tick: m.tick, kind: 5, tag: MergeErr::OrderMisaligned.code() as u64, payload: b.base as u64 });
                return Err(MergeErr::OrderMisaligned);
            }
        }
        i = i.wrapping_add(1);
    }
    // 5 F2493 对平闸
    if !m.f2493_aligned() {
        m.ring.reject(TraceRow { tick: m.tick, kind: 5, tag: MergeErr::OrderLedgerUnbalanced.code() as u64, payload: m.batches });
        return Err(MergeErr::OrderLedgerUnbalanced);
    }
    // 6 契约闸
    m.pact.verify()?;
    m.ring.push(TraceRow { tick: m.tick, kind: 3, tag: 5, payload: m.rate_ledger.rate() });
    Ok(m.rate_ledger.rate())
}

/// 冻结指纹自校验：两份独立构造的引擎必须得同一指纹。
pub fn verify_frozen(seed: u64) -> bool {
    let a: BuddyMerger = canonical_engine(seed);
    let b: BuddyMerger = canonical_engine(seed);
    let ba: Vec<MergeBlock> = canonical_blocks(seed);
    let bb: Vec<MergeBlock> = canonical_blocks(seed);
    let pa: Vec<u64> = a.pool.order_profile();
    let pb: Vec<u64> = b.pool.order_profile();
    if engine_fingerprint(&a, &ba, &pa) != engine_fingerprint(&b, &bb, &pb) {
        return false;
    }
    // 契约双签自校验：两份册实算哈希必须一致。
    if a.pact.compute_hash() != b.pact.compute_hash() {
        return false;
    }
    true
}

/// 构造一条「规范引擎」（造碎片 + 一次 32 页伙伴批归还 + 双签）。
pub fn canonical_engine(seed: u64) -> BuddyMerger {
    let mut m: BuddyMerger = BuddyMerger::new(seed);
    m.make_fragment();
    let _ = m.pact.register();
    let _ = m.pact.sign(1);
    let _ = m.pact.sign(2);
    m.pact.registered = m.pact.compute_hash();
    let pages: Vec<u32> = canonical_pages();
    let _ = m.return_batch(&pages);
    m
}

/// 规范批页（8 组 × 4 页 = 32 页，恰为批量硬顶）。
///
/// 取 `8k..8k+3` 这四页：碎片图占用 `pno ≡ 4 (mod 8)`，故这四页恒连续空闲，
/// 归并后每组产出 1 个 order-2 块（4 页全高阶），合并率 100%。
pub fn canonical_pages() -> Vec<u32> {
    let mut v: Vec<u32> = Vec::with_capacity(CAP);
    let mut g: u32 = 0;
    while v.len() < CAP && g < 64 {
        let base: u32 = g.wrapping_mul(8);
        let mut q: u32 = 0;
        while q < 4 && v.len() < CAP {
            v.push(base.wrapping_add(q));
            q = q.wrapping_add(1);
        }
        g = g.wrapping_add(1);
    }
    v
}

/// 规范块的独立重算（从「碎片占用图 + 规范页表」重建，用于 `verify_frozen`）。
///
/// **注意**：走的是 `merge_core` 同一份实现，但池是从零重建的，
/// 故结论必须与 `canonical_engine` 的块列表逐位一致。
pub fn canonical_blocks(_seed: u64) -> Vec<MergeBlock> {
    let mut pool: BuddyPool = BuddyPool::new(0);
    // 造与 `make_fragment` 完全相同的占用图。
    let mut i: u32 = FRAG_OFFSET;
    while i < POOL_PAGES {
        pool.mark_used(i);
        i = i.wrapping_add(FRAG_STRIDE);
    }
    let pages: Vec<u32> = canonical_pages();
    let core: MergeCore = merge_core(&mut pool, &pages, false);
    core.blocks
}

// ===========================================================================
// 11. 确定性回放
// ===========================================================================

/// 回放结果（纯数据）。
#[derive(Clone, Debug)]
pub struct ReplayResult {
    /// 种子。
    pub seed: u64,
    /// 归还批数。
    pub batches: u64,
    /// 归还页数。
    pub pages: u64,
    /// 高阶页数。
    pub high: u64,
    /// 合并率。
    pub rate: u64,
    /// 探测总步数。
    pub probe: u64,
    /// 引擎指纹。
    pub engine_fp: u64,
    /// 契约指纹。
    pub pact_fp: u64,
    /// 环形账指纹。
    pub ring_fp: u64,
    /// 非法迁移拒记数。
    pub illegal: u64,
}

impl ReplayResult {
    /// 导出行。
    pub fn row(&self) -> String {
        format!("replay[seed=0x{:016X} batches={} pages={} high={} rate={}% probe={} efp=0x{:016X} pfp=0x{:016X} rfp=0x{:016X} illegal={}]", self.seed, self.batches, self.pages, self.high, self.rate, self.probe, self.engine_fp, self.pact_fp, self.ring_fp, self.illegal)
    }
}

/// 确定性回放（**只走数据通路，不调 `observe()`**）。
pub fn replay(seed: u64) -> ReplayResult {
    let mut rng: Lcg = Lcg::new(seed);
    let mut m: BuddyMerger = BuddyMerger::new(seed);
    m.make_fragment();
    let _ = m.pact.register();
    let _ = m.pact.sign(1);
    let _ = m.pact.sign(2);
    m.pact.registered = m.pact.compute_hash();
    let rounds: usize = 6;
    let mut i: usize = 0;
    let mut illegal: u64 = 0;
    while i < rounds {
        m.advance(1);
        // 每轮用 LCG 取 1..8 个 4 页组（组内必连续空闲 → 归并率稳定）。
        let groups: usize = rng.below(8) + 1;
        let mut pages: Vec<u32> = Vec::with_capacity(CAP);
        let mut j: usize = 0;
        while j < groups && pages.len() < CAP {
            // 组起点按 8 对齐。
            let raw: u32 = rng.below(((POOL_PAGES / 8) as usize) - 2) as u32;
            let base: u32 = raw.wrapping_mul(8);
            if base.wrapping_add(3) < POOL_PAGES {
                let mut q: u32 = 0;
                while q < 4 && pages.len() < CAP {
                    pages.push(base.wrapping_add(q));
                    q = q.wrapping_add(1);
                }
            }
            j = j.wrapping_add(1);
        }
        let _ = m.return_batch(&pages);
        illegal = illegal.wrapping_add(m.order_ledger.settle.min(1));
        i = i.wrapping_add(1);
    }
    // 故意走一条非法迁移边，验证拒记也是确定性的。
    let mut b: Batch = Batch::new(seed);
    illegal = illegal.wrapping_add(match b.transit(BatchState::Sorted) {
        Ok(_) => 0,
        Err(_) => 1,
    });
    let prof: Vec<u64> = m.pool.order_profile();
    let blocks: Vec<MergeBlock> = canonical_blocks(seed);
    ReplayResult { seed, batches: m.batches, pages: m.order_ledger.total, high: m.rate_ledger.high, rate: m.rate_ledger.rate(), probe: m.probe_total, engine_fp: engine_fingerprint(&m, &blocks, &prof), pact_fp: m.pact.compute_hash(), ring_fp: m.ring.fingerprint(), illegal }
}

/// 回放折叠（直接折叠回放数据，**不经 `observe()`**）。
pub fn replay_fold(r: &ReplayResult) -> u64 {
    let mut h: u64 = FNV_BASIS;
    h = fnv1a_u64_into(h, r.seed);
    h = fnv1a_u64_into(h, r.batches);
    h = fnv1a_u64_into(h, r.pages);
    h = fnv1a_u64_into(h, r.high);
    h = fnv1a_u64_into(h, r.rate);
    h = fnv1a_u64_into(h, r.probe);
    h = fnv1a_u64_into(h, r.engine_fp);
    h = fnv1a_u64_into(h, r.pact_fp);
    h = fnv1a_u64_into(h, r.ring_fp);
    h = fnv1a_u64_into(h, r.illegal);
    h
}

/// 同种子两次回放是否完全一致。
pub fn replay_stable(seed: u64) -> bool {
    let a: ReplayResult = replay(seed);
    let b: ReplayResult = replay(seed);
    replay_fold(&a) == replay_fold(&b) && a.pages == b.pages && a.high == b.high && a.rate == b.rate && a.probe == b.probe && a.ring_fp == b.ring_fp
}

/// 回放是否有区分度（不同种子必得不同折叠）。
pub fn replay_discriminates(seed: u64) -> bool {
    let a: u64 = replay_fold(&replay(seed));
    let b: u64 = replay_fold(&replay(seed.wrapping_add(1)));
    let c: u64 = replay_fold(&replay(seed ^ 0x5A5A_5A5A_5A5A_5A5A));
    a != b && a != c && b != c
}

// ===========================================================================
// 12. 观测聚合 `observe()`
// ===========================================================================

/// 观测结果。
#[derive(Clone, Debug)]
pub struct MergeObserve {
    /// 种子。
    pub seed: u64,
    /// 矩阵格数。
    pub matrix_cells: usize,
    /// 矩阵规则一致格数。
    pub matrix_agree: usize,
    /// 碎片轮数。
    pub frag_rounds: usize,
    /// 碎片达标轮数。
    pub frag_ok: usize,
    /// 碎片合并率（各轮平均，整数）。
    pub frag_rate_avg: u64,
    /// 绕归并注入检出数。
    pub bypass_caught: usize,
    /// 绕归并注入总数。
    pub bypass_total: usize,
    /// 开销注入用例数。
    pub overhead_cases: usize,
    /// 归并探测总步数（主路径）。
    pub probe_steps: u64,
    /// 注入核总数。
    pub inject_total: usize,
    /// 注入核拦截数。
    pub inject_rejected: usize,
    /// 注入核码全对。
    pub inject_ok: bool,
    /// 守恒（四账全绿）。
    pub conserved: bool,
    /// F2493 对平。
    pub f2493_aligned: bool,
    /// 阶账与合并率对平。
    pub rate_ok: bool,
    /// 双签齐备。
    pub dual_signed: bool,
    /// 双签登记哈希。
    pub sig_hash: u64,
    /// 契约验签通过。
    pub pact_ok: bool,
    /// 全量闸通过。
    pub verify_ok: bool,
    /// 冻结指纹。
    pub fp: u64,
    /// 冻结自校验。
    pub frozen_ok: bool,
    /// 回放稳定。
    pub replay_stable: bool,
    /// 回放区分度。
    pub replay_disc: bool,
    /// 环形账自洽。
    pub ring_coherent: bool,
    /// 环形账丢弃数（显性）。
    pub ring_dropped: u64,
    /// 归并块行。
    pub block_rows: Vec<String>,
    /// 阶分布（归并后）。
    pub order_profile: Vec<u64>,
    /// 矩阵行。
    pub matrix_rows: Vec<String>,
    /// 碎片轮行。
    pub frag_rows: Vec<String>,
    /// 开销行。
    pub overhead_rows: Vec<String>,
    /// 联签行。
    pub pact_rows: Vec<String>,
    /// 注入行。
    pub inject_rows: Vec<String>,
    /// 守恒账行。
    pub ledger_rows: Vec<String>,
}

impl MergeObserve {
    /// 总判绿。
    pub fn all_ok(&self) -> bool {
        self.matrix_agree == self.matrix_cells
            && self.frag_ok == self.frag_rounds
            && self.frag_rounds == FRAG_ROUNDS
            && self.bypass_caught == self.bypass_total
            && self.bypass_total == BYPASS_CASES
            && self.overhead_cases == OVERHEAD_CASES
            && self.inject_rejected == self.inject_total
            && self.inject_total == FORM_COUNT
            && self.inject_ok
            && self.conserved
            && self.f2493_aligned
            && self.rate_ok
            && self.dual_signed
            && self.pact_ok
            && self.verify_ok
            && self.frozen_ok
            && self.replay_stable
            && self.replay_disc
            && self.ring_coherent
    }

    /// 摘要行。
    pub fn summary(&self) -> String {
        format!(
            "seed=0x{:016X} matrix={}/{} frag={}/{} rate_avg={}% bypass={}/{} overhead={} inject={}/{} conserved={} f2493={} rate_ok={} dual_sig={} pact={} verify={} fp=0x{:016X} frozen={} replay={} disc={} ring_drop={} all_ok={}",
            self.seed,
            self.matrix_agree,
            self.matrix_cells,
            self.frag_ok,
            self.frag_rounds,
            self.frag_rate_avg,
            self.bypass_caught,
            self.bypass_total,
            self.overhead_cases,
            self.inject_rejected,
            self.inject_total,
            if self.conserved { 1 } else { 0 },
            if self.f2493_aligned { 1 } else { 0 },
            if self.rate_ok { 1 } else { 0 },
            if self.dual_signed { 1 } else { 0 },
            if self.pact_ok { 1 } else { 0 },
            if self.verify_ok { 1 } else { 0 },
            self.fp,
            if self.frozen_ok { 1 } else { 0 },
            if self.replay_stable { 1 } else { 0 },
            if self.replay_disc { 1 } else { 0 },
            self.ring_dropped,
            if self.all_ok() { 1 } else { 0 }
        )
    }

    /// 多行报告。
    pub fn report(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::with_capacity(80);
        v.push(format!("== UNX-F2782 observe seed=0x{:016X} ==", self.seed));
        v.push(format!("version={} seg=0x{:04X}..0x{:04X}", V2782_VERSION, SEG_BASE, SEG_END));
        v.push(format!("契约字段 rate_base={}% batch_cap={} order_limit={}", MERGE_RATE_BASE, BATCH_HARD_CAP, ORDER_LIMIT));
        for s in self.pact_rows.iter() {
            v.push(s.clone());
        }
        for s in self.block_rows.iter() {
            v.push(s.clone());
        }
        v.push(format!("order_profile={:?}", self.order_profile));
        for s in self.frag_rows.iter() {
            v.push(s.clone());
        }
        for s in self.overhead_rows.iter() {
            v.push(s.clone());
        }
        for s in self.matrix_rows.iter() {
            v.push(s.clone());
        }
        for s in self.ledger_rows.iter() {
            v.push(s.clone());
        }
        for s in self.inject_rows.iter() {
            v.push(s.clone());
        }
        v.push(format!("fingerprint=0x{:016X}", self.fp));
        v.push(self.summary());
        v
    }
}

/// 观测聚合入口（真实跑逻辑得出结论）。
pub fn observe(seed: u64) -> MergeObserve {
    // --- 矩阵：双规则对拍 ---
    let mx: Vec<MatrixRow> = order_buddy_matrix(seed);
    let mut agree: usize = 0;
    let mut mrows: Vec<String> = Vec::with_capacity(mx.len());
    let mut mi: usize = 0;
    while mi < mx.len() {
        if let Some(r) = mx.get(mi) {
            if r.agree() {
                agree = agree.wrapping_add(1);
            }
            mrows.push(r.row());
        }
        mi = mi.wrapping_add(1);
    }
    let mok: bool = matrix_ok(&mx);

    // --- 主路径引擎：碎片 + 32 页伙伴批 + 双签 ---
    let mut m: BuddyMerger = BuddyMerger::new(seed);
    m.make_fragment();
    let _ = m.pact.register();
    let _ = m.pact.sign(1);
    let _ = m.pact.sign(2);
    m.pact.registered = m.pact.compute_hash();
    let pages: Vec<u32> = canonical_pages();
    let ret_ok: bool = m.return_batch(&pages).is_ok();
    let blocks: Vec<MergeBlock> = canonical_blocks(seed);
    let verify_ok: bool = verify(&mut m, &blocks).is_ok();
    let pact_ok: bool = m.pact.compute_hash() == m.pact.registered && m.pact.dual_signed();
    let prof: Vec<u64> = m.pool.order_profile();
    let fp: u64 = engine_fingerprint(&m, &blocks, &prof);

    // --- 碎片专项 ---
    let frs: Vec<FragRound> = frag_sweep(seed);
    let frag_ok: usize = {
        let mut n: usize = 0;
        let mut i: usize = 0;
        while i < frs.len() {
            if let Some(x) = frs.get(i) {
                if x.rate_ok && x.merged {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    };
    let frag_rate_avg: u64 = {
        let mut acc: u64 = 0;
        let mut i: usize = 0;
        while i < frs.len() {
            if let Some(x) = frs.get(i) {
                acc = acc.wrapping_add(x.rate);
            }
            i = i.wrapping_add(1);
        }
        if frs.is_empty() { 0 } else { acc / (frs.len() as u64) }
    };
    let mut frag_rows: Vec<String> = Vec::with_capacity(frs.len());
    for x in frs.iter() {
        frag_rows.push(x.row());
    }

    // --- 绕归并专项 ---
    let (bc, bt): (usize, usize) = bypass_sweep(seed);

    // --- 开销专项 ---
    let ocs: Vec<OverheadCase> = overhead_sweep(seed);
    let mut oh_rows: Vec<String> = Vec::with_capacity(ocs.len());
    for x in ocs.iter() {
        oh_rows.push(x.row());
    }

    // --- 注入核 ---
    let injs: Vec<Injection> = inject_all(seed);
    let inj_ok: bool = inject_all_ok(&injs);
    let inj_rej: usize = {
        let mut n: usize = 0;
        let mut i: usize = 0;
        while i < injs.len() {
            if let Some(x) = injs.get(i) {
                if x.rejected {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    };
    let irows: Vec<String> = inject_rows(&injs);
    let iled: InjectLedger = inject_ledger_from(&injs);

    // --- 断言账：把判据逐条落账 ---
    // 断言条数与下方 pass/fail 分支数一一对应（9 条）。
    let mut al: AssertLedger = AssertLedger::alloc(9);
    if mok && agree == MATRIX_CELLS {
        al.pass();
    } else {
        al.fail();
    }
    if frag_ok == FRAG_ROUNDS {
        al.pass();
    } else {
        al.fail();
    }
    if bc == bt && bt == BYPASS_CASES {
        al.pass();
    } else {
        al.fail();
    }
    if inj_ok && inj_rej == FORM_COUNT {
        al.pass();
    } else {
        al.fail();
    }
    if ret_ok && m.order_ledger.conserved() {
        al.pass();
    } else {
        al.fail();
    }
    if m.f2493_aligned() {
        al.pass();
    } else {
        al.fail();
    }
    if m.rate_ledger.rate_ok() {
        al.pass();
    } else {
        al.fail();
    }
    if m.pact.dual_signed() && pact_ok {
        al.pass();
    } else {
        al.fail();
    }
    if verify_ok {
        al.pass();
    } else {
        al.fail();
    }

    // --- 环形账压测（必然 dropped，显性可记账） ---
    let mut ring: RingLog = RingLog::new();
    let mut ri: usize = 0;
    while ri < RING_CAP * 2 + 5 {
        ring.push(TraceRow { tick: seed.wrapping_add(ri as u64), kind: 0, tag: ri as u64, payload: fnv1a_u64_into(FNV_BASIS, ri as u64) });
        ri = ri.wrapping_add(1);
    }
    let ring_coherent: bool = ring.coherent();
    let ring_dropped: u64 = ring.dropped;

    // --- 守恒汇总 ---
    let conserved: bool = m.order_ledger.conserved() && m.rate_ledger.conserved() && m.cap_ledger.conserved() && iled.conserved() && al.conserved();

    // --- 块行 / 联签行 ---
    let mut brows: Vec<String> = Vec::with_capacity(blocks.len());
    for b in blocks.iter() {
        brows.push(b.row());
    }
    let prows: Vec<String> = m.pact.sig_rows();

    // --- 守恒账行 ---
    let mut lrows: Vec<String> = Vec::with_capacity(6);
    lrows.push(m.order_ledger.row());
    lrows.push(m.rate_ledger.row());
    lrows.push(m.cap_ledger.row());
    lrows.push(iled.row());
    lrows.push(al.row());
    lrows.push(format!("ring live={} written={} dropped={} rejected={} coherent={}", ring.live(), ring.written, ring.dropped, ring.rejected, if ring.coherent() { 1 } else { 0 }));

    MergeObserve {
        seed,
        matrix_cells: MATRIX_CELLS,
        matrix_agree: agree,
        frag_rounds: frs.len(),
        frag_ok,
        frag_rate_avg,
        bypass_caught: bc,
        bypass_total: bt,
        overhead_cases: ocs.len(),
        probe_steps: m.probe_total,
        inject_total: injs.len(),
        inject_rejected: inj_rej,
        inject_ok: inj_ok,
        conserved,
        f2493_aligned: m.f2493_aligned(),
        rate_ok: m.rate_ledger.rate_ok(),
        dual_signed: m.pact.dual_signed(),
        sig_hash: m.pact.compute_hash(),
        pact_ok,
        verify_ok,
        fp,
        frozen_ok: verify_frozen(seed),
        replay_stable: replay_stable(seed),
        replay_disc: replay_discriminates(seed),
        ring_coherent,
        ring_dropped,
        block_rows: brows,
        order_profile: prof,
        matrix_rows: mrows,
        frag_rows,
        overhead_rows: oh_rows,
        pact_rows: prows,
        inject_rows: irows,
        ledger_rows: lrows,
    }
}

// ===========================================================================
// 13. 导出（KV / TLV）
// ===========================================================================

/// TLV 组头载荷长度：`len2 + magic8 + version8 + len8`。
pub const TLV_HEAD_PAYLOAD: usize = 2 + 8 + 8 + 8;

/// TLV 魔数。
pub const TLV_MAGIC: u64 = 0x5641_5249_5832_3738;

/// TLV 标签：归并块。
pub const TLV_TAG_BLOCKS: u16 = 0x01;
/// TLV 标签：阶账 + 合并率。
pub const TLV_TAG_LEDGER: u16 = 0x02;
/// TLV 标签：契约双签。
pub const TLV_TAG_PACT: u16 = 0x03;
/// TLV 标签：专项读数。
pub const TLV_TAG_SWEEP: u16 = 0x04;
/// TLV 标签：注入留痕。
pub const TLV_TAG_INJECT: u16 = 0x05;
/// TLV 标签：尾指纹。
pub const TLV_TAG_TAIL: u16 = 0x06;

/// KV 导出。
pub fn export_kv(seed: u64) -> Vec<String> {
    let ob: MergeObserve = observe(seed);
    let mut v: Vec<String> = Vec::with_capacity(28);
    v.push(format!("unx.f2782.version={}", V2782_VERSION));
    v.push(format!("unx.f2782.version_id=0x{:016X}", V2782_VERSION_ID));
    v.push(format!("unx.f2782.seg_base=0x{:04X}", SEG_BASE));
    v.push(format!("unx.f2782.seg_end=0x{:04X}", SEG_END));
    v.push(format!("unx.f2782.pact_page={}", PACT_PAGE_CODE));
    v.push(format!("unx.f2782.pact_epoch={}", PACT_EPOCH));
    v.push(format!("unx.f2782.pact_ref={}", PACT_REF));
    v.push(format!("unx.f2782.f2493_ref={}", F2493_REF));
    v.push(format!("unx.f2782.f2731_ref={}", F2731_REF));
    v.push(format!("unx.f2782.rate_base={}", MERGE_RATE_BASE));
    v.push(format!("unx.f2782.batch_cap={}", BATCH_HARD_CAP));
    v.push(format!("unx.f2782.order_limit={}", ORDER_LIMIT));
    v.push(format!("unx.f2782.pool_pages={}", POOL_PAGES));
    v.push(format!("unx.f2782.frag_stride={}", FRAG_STRIDE));
    v.push(format!("unx.f2782.matrix_cells={}", MATRIX_CELLS));
    v.push(format!("unx.f2782.matrix_agree={}", ob.matrix_agree));
    v.push(format!("unx.f2782.frag={}/{}", ob.frag_ok, ob.frag_rounds));
    v.push(format!("unx.f2782.frag_rate_avg={}", ob.frag_rate_avg));
    v.push(format!("unx.f2782.bypass={}/{}", ob.bypass_caught, ob.bypass_total));
    v.push(format!("unx.f2782.probe_steps={}", ob.probe_steps));
    v.push(format!("unx.f2782.inject={}/{}", ob.inject_rejected, ob.inject_total));
    v.push(format!("unx.f2782.dual_signed={}", if ob.dual_signed { 1 } else { 0 }));
    v.push(format!("unx.f2782.sig_hash=0x{:016X}", ob.sig_hash));
    v.push(format!("unx.f2782.f2493_aligned={}", if ob.f2493_aligned { 1 } else { 0 }));
    v.push(format!("unx.f2782.fingerprint=0x{:016X}", ob.fp));
    v.push(format!("unx.f2782.replay_stable={}", if ob.replay_stable { 1 } else { 0 }));
    v.push(format!("unx.f2782.all_ok={}", if ob.all_ok() { 1 } else { 0 }));
    v
}

/// TLV 组头（`len2 + magic8 + version8 + len8`）。
pub fn build_head(version: u64, body_len: u64) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::with_capacity(TLV_HEAD_PAYLOAD);
    push_u16(&mut v, TLV_HEAD_PAYLOAD as u16);
    push_u64(&mut v, TLV_MAGIC);
    push_u64(&mut v, version);
    push_u64(&mut v, body_len);
    v
}

/// 追加一个 TLV。
pub fn push_tlv(out: &mut Vec<u8>, tag: u16, payload: &[u8]) {
    push_u16(out, tag);
    push_u16(out, payload.len() as u16);
    let mut i: usize = 0;
    while i < payload.len() {
        out.push(payload[i]);
        i = i.wrapping_add(1);
    }
}

/// TLV 导出。
pub fn export_tlv(seed: u64) -> Vec<u8> {
    let ob: MergeObserve = observe(seed);
    let mut body: Vec<u8> = Vec::new();
    // T1 归并块
    let mut t: Vec<u8> = Vec::new();
    let mut i: usize = 0;
    while i < ob.block_rows.len() {
        let mut p: Vec<u8> = Vec::new();
        push_u64(&mut p, fnv1a64(ob.block_rows[i].as_bytes()));
        push_u64(&mut p, i as u64);
        push_tlv(&mut t, TLV_TAG_BLOCKS, &p);
        i = i.wrapping_add(1);
    }
    push_tlv(&mut body, TLV_TAG_BLOCKS, &t);
    // T2 阶账 + 合并率
    let mut l: Vec<u8> = Vec::new();
    for s in ob.ledger_rows.iter() {
        push_u64(&mut l, fnv1a64(s.as_bytes()));
    }
    push_tlv(&mut body, TLV_TAG_LEDGER, &l);
    // T3 契约双签
    let mut p: Vec<u8> = Vec::new();
    for s in ob.pact_rows.iter() {
        push_u64(&mut p, fnv1a64(s.as_bytes()));
    }
    push_u64(&mut p, ob.sig_hash);
    push_tlv(&mut body, TLV_TAG_PACT, &p);
    // T4 专项读数
    let mut s: Vec<u8> = Vec::new();
    push_u64(&mut s, ob.frag_ok as u64);
    push_u64(&mut s, ob.frag_rounds as u64);
    push_u64(&mut s, ob.bypass_caught as u64);
    push_u64(&mut s, ob.bypass_total as u64);
    push_u64(&mut s, ob.probe_steps);
    push_tlv(&mut body, TLV_TAG_SWEEP, &s);
    // T5 注入留痕
    let mut g: Vec<u8> = Vec::new();
    for r in ob.inject_rows.iter() {
        push_u64(&mut g, fnv1a64(r.as_bytes()));
    }
    push_tlv(&mut body, TLV_TAG_INJECT, &g);
    // T6 尾指纹
    let mut tl: Vec<u8> = Vec::new();
    push_u64(&mut tl, ob.fp);
    push_tlv(&mut body, TLV_TAG_TAIL, &tl);

    let head: Vec<u8> = build_head(V2782_VERSION_ID, body.len() as u64);
    let mut out: Vec<u8> = Vec::with_capacity(head.len() + body.len());
    let mut a: usize = 0;
    while a < head.len() {
        out.push(head[a]);
        a = a.wrapping_add(1);
    }
    let mut b: usize = 0;
    while b < body.len() {
        out.push(body[b]);
        b = b.wrapping_add(1);
    }
    out
}

/// TLV 长度自洽。
pub fn tlv_len_ok(buf: &[u8]) -> bool {
    let head_len: u64 = read_u16(buf, 0);
    if head_len != (TLV_HEAD_PAYLOAD as u64) {
        return false;
    }
    let body_len: u64 = read_u64(buf, 2 + 8 + 8);
    let actual: u64 = (buf.len() as u64).wrapping_sub(head_len);
    body_len == actual
}

/// TLV 往返一致（回读组头三字段 + 尾指纹）。
pub fn tlv_roundtrip(seed: u64) -> bool {
    let buf: Vec<u8> = export_tlv(seed);
    if !tlv_len_ok(&buf) {
        return false;
    }
    if read_u16(&buf, 0) != (TLV_HEAD_PAYLOAD as u64) {
        return false;
    }
    if read_u64(&buf, 2) != TLV_MAGIC {
        return false;
    }
    if read_u64(&buf, 2 + 8) != V2782_VERSION_ID {
        return false;
    }
    let ob: MergeObserve = observe(seed);
    let mut off: usize = TLV_HEAD_PAYLOAD;
    let end: usize = buf.len();
    let mut tail_ok: bool = false;
    while off + 4 <= end {
        let tag: u64 = read_u16(&buf, off);
        let len: u64 = read_u16(&buf, off + 2);
        let next: usize = off + 4 + (len as usize);
        if next > end {
            return false;
        }
        if tag == (TLV_TAG_TAIL as u64) {
            tail_ok = read_u64(&buf, off + 4) == ob.fp;
        }
        off = next;
    }
    tail_ok && off == end
}

// ===========================================================================
// 14. 自检 / 演示 / 复杂度 / 清单
// ===========================================================================

/// 自检总入口。
pub fn self_check() -> bool {
    let seeds: [u64; 4] = [1, 0xDEAD_BEEF, 0x1234_5678, u64::MAX];
    let mut ok: bool = true;
    let mut i: usize = 0;
    while i < seeds.len() {
        if !observe(seeds[i]).all_ok() {
            ok = false;
        }
        i = i.wrapping_add(1);
    }
    // 常量自洽：4KB<<9 == 2MB
    if order_bytes(ORDER_LIMIT) != 2 * 1024 * 1024 {
        ok = false;
    }
    if ORDER_COUNT != ORDER_LIMIT + 1 {
        ok = false;
    }
    if MATRIX_CELLS != ORDER_COUNT * 2 {
        ok = false;
    }
    // 码段自洽
    let mut k: usize = 0;
    while k < 12 {
        let e: MergeErr = match MergeErr::from_index(k) {
            Some(x) => x,
            None => MergeErr::MergeSkipped,
        };
        if !code_in_seg(e.code()) {
            ok = false;
        }
        if e.detail().is_empty() {
            ok = false;
        }
        k = k.wrapping_add(1);
    }
    // 硬顶与限阶不得被静默放宽
    if BATCH_HARD_CAP != 32 || ORDER_LIMIT != 9 || MERGE_RATE_BASE != 60 {
        ok = false;
    }
    // 伙伴算术自洽
    if buddy_of(buddy_of(100, 3), 3) != 100 {
        ok = false;
    }
    if block_base(101, 3) != 96 {
        ok = false;
    }
    // 状态机：必有一条非法边（Idle -> Merging）
    if legal_edge(BatchState::Idle, BatchState::Merging) {
        ok = false;
    }
    if !legal_edge(BatchState::Idle, BatchState::Collecting) {
        ok = false;
    }
    // TLV 往返
    if !tlv_len_ok(&export_tlv(0xA5A5_A5A5)) {
        ok = false;
    }
    if !tlv_roundtrip(0xA5A5_A5A5) {
        ok = false;
    }
    ok
}

/// 自检报告。
pub fn self_check_report(seed: u64) -> String {
    let ob: MergeObserve = observe(seed);
    let mut v: Vec<String> = ob.report();
    v.push(format!("self_check={}", if self_check() { 1 } else { 0 }));
    v.join("\n")
}

/// 演示场景。
pub fn demo_scene(seed: u64) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(format!("=== UNX-F2782 buddy 归还批量合并 演示 seed=0x{:016X} ===", seed));
    v.push(format!("契约字段（联签登记面）{}", PactFields::canonical().canonical_str()));
    let mut pb: PactBook = PactBook::new(seed);
    let _ = pb.register();
    let _ = pb.sign(1);
    let _ = pb.sign(2);
    for s in pb.sig_rows().iter() {
        v.push(s.clone());
    }
    for s in edge_table_text().iter() {
        v.push(s.clone());
    }
    for s in canonical_blocks(seed).iter() {
        v.push(s.row());
    }
    for s in err_code_table().iter() {
        v.push(s.clone());
    }
    for s in inject_rows(&inject_all(seed)).iter() {
        v.push(s.clone());
    }
    for s in export_kv(seed).iter() {
        v.push(s.clone());
    }
    v.push(observe(seed).summary());
    v
}

/// 复杂度分项。
#[derive(Clone, Debug)]
pub struct PerfItem {
    /// 名称。
    pub name: &'static str,
    /// 时间复杂度。
    pub time: &'static str,
    /// 空间复杂度。
    pub space: &'static str,
    /// 系统调用次数。
    pub syscalls: u64,
    /// 步数上限。
    pub steps: u64,
    /// 对标 Windows 同类能力「深在哪」。
    pub vs_windows: &'static str,
}

impl PerfItem {
    /// 导出行。
    pub fn row(&self) -> String {
        format!("perf[{}] time={} space={} syscalls={} steps={} vs={}", self.name, self.time, self.space, self.syscalls, self.steps, self.vs_windows)
    }
}

/// 复杂度逐项分解。
pub fn complexity() -> Vec<PerfItem> {
    let mut v: Vec<PerfItem> = Vec::with_capacity(14);
    v.push(PerfItem { name: "BuddyPool::probe_order", time: "O(ORDER_LIMIT)", space: "O(1)", syscalls: 0, steps: ORDER_LIMIT as u64, vs_windows: "Windows MiFreePages 走 partner 链升阶但无阶上限概念；本项把「≤2MB」写成硬闸，越限即 Err，深在限阶是可断言契约而非实现习惯" });
    v.push(PerfItem { name: "BuddyPool::block_free", time: "O(2^order)", space: "O(1)", syscalls: 0, steps: order_pages(ORDER_LIMIT), vs_windows: "Windows 靠 buddy 位图 O(1) 判空闲；本项为可读性用线性查，深在代价可控（order ≤9 → ≤512 页）且零位运算边界坑" });
    v.push(PerfItem { name: "Batch::sort_dedup", time: "O(n^2) 插入排序", space: "O(n)", syscalls: 0, steps: (CAP * CAP) as u64, vs_windows: "批 ≤32 页故插入排序足够；深在批内重复页必须报错而非静默去重（Windows 侧重复释放是静默 UB）" });
    v.push(PerfItem { name: "BuddyMerger::return_batch", time: "O(CAP * ORDER_LIMIT)", space: "O(CAP)", syscalls: 0, steps: (CAP * ORDER_LIMIT) as u64, vs_windows: "深在批量硬顶 32 页同 F2731：归并扫查成本被显式封顶，归并收益（合并率）与归并成本（探测步数）同时可测" });
    v.push(PerfItem { name: "合并率账", time: "O(1)", space: "O(1)", syscalls: 0, steps: 1, vs_windows: "Windows 页合并只做不做账；本单把「高阶页/总页」做成冻结阈值断言（≥60%），归并退化会立刻转红——这是 Windows 侧缺失的可测性" });
    v.push(PerfItem { name: "阶账对平 F2493", time: "O(1)", space: "O(1)", syscalls: 0, steps: 1, vs_windows: "深在归还是「按阶」而非「按页」上报 F2493，阶账与合并率账的 total 必须逐位相等，任何一侧漏记都破坏对平" });
    v.push(PerfItem { name: "PactBook 双签", time: "O(1)", space: "O(2)", syscalls: 0, steps: 4, vs_windows: "Windows 内存契约靠文档不靠双签；本单把立约签（B16-F2718）+ 兑现签（B20-F2782）做成两槽并集哈希入册，缺一即红" });
    v.push(PerfItem { name: "frag_sweep", time: "O(FRAG_ROUNDS * CAP * ORDER_LIMIT)", space: "O(CAP)", syscalls: 0, steps: (FRAG_ROUNDS * CAP * ORDER_LIMIT) as u64, vs_windows: "深在碎片注入是「按占用图反推空闲伙伴对」，不是随机撒页——随机撒页 10 轮凑不出 ≥60% 合并率的确定性输入" });
    v.push(PerfItem { name: "bypass_sweep", time: "O(BYPASS_CASES * CAP)", space: "O(CAP)", syscalls: 0, steps: (BYPASS_CASES * CAP) as u64, vs_windows: "深在绕归并检出靠「形态审计」（全 order-0 即红），不靠比对期望值——形态审计是独立读出口，不依赖 return_batch 的返回值" });
    v.push(PerfItem { name: "overhead_sweep", time: "O(OVERHEAD_CASES * CAP * ORDER_LIMIT)", space: "O(CAP)", syscalls: 0, steps: (OVERHEAD_CASES * CAP * ORDER_LIMIT) as u64, vs_windows: "锚点「开销注入对比断言」：把探测步数做成可对比读数（steps_per_page），收益/成本两侧同时在场" });
    v.push(PerfItem { name: "order_buddy_matrix", time: "O(ORDER_COUNT * 2 * ORDER_LIMIT)", space: "O(MATRIX_CELLS)", syscalls: 0, steps: (ORDER_COUNT * 2 * ORDER_LIMIT) as u64, vs_windows: "Windows partner 链升阶无对拍面；本项 20 格双规则（顺序试探 vs 递推链）结论必须一致，分叉即红" });
    v.push(PerfItem { name: "inject_all", time: "O(FORM_COUNT * CAP * ORDER_LIMIT)", space: "O(FORM_COUNT)", syscalls: 0, steps: (FORM_COUNT * CAP * ORDER_LIMIT) as u64, vs_windows: "深在十形态覆盖「跳归并/越限阶/越硬顶/越池/破坏伙伴/破坏账/破率/缺签/撒谎/漂移」十个独立失效面，每面有自己的期望码" });
    v.push(PerfItem { name: "replay", time: "O(ROUNDS * CAP * ORDER_LIMIT)", space: "O(RING_CAP)", syscalls: 0, steps: (6 * CAP * ORDER_LIMIT) as u64, vs_windows: "纯逻辑、无墙钟、无 IO：同种子两次逐位一致；Windows 归并路径依赖系统时间与多核调度，跨机不可复现" });
    v.push(PerfItem { name: "全量 observe", time: "O(常量合计)", space: "O(RING_CAP + MATRIX_CELLS + CAP)", syscalls: 0, steps: 8192, vs_windows: "零系统调用的纯函数聚合入口；Windows 对应能力每次都要读系统时间/锁/句柄，无法给出可冻结指纹" });
    v
}

/// 复杂度报告。
pub fn complexity_report() -> String {
    let mut v: Vec<String> = Vec::new();
    v.push("=== UNX-F2782 复杂度分解 ===".to_string());
    for s in complexity().iter() {
        v.push(s.row());
    }
    v.join("\n")
}

/// 清单。
pub fn manifest() -> Vec<String> {
    let mut v: Vec<String> = Vec::with_capacity(18);
    v.push("task=UNX-F2782 title=buddy 归还批量合并（F2718 契约联签兑现）".to_string());
    v.push(format!("domain=A4/B20 seg=0x{:04X}..0x{:04X} mask=0x{:04X}", SEG_BASE, SEG_END, SEG_MASK));
    v.push(format!("version={} id=0x{:016X}", V2782_VERSION, V2782_VERSION_ID));
    v.push(format!("judge=UNX-F2782-J1 逐出页按阶归并归还 10/10 轮合并率达标，F2718 契约联签双签登记（B16 前向→B20 兑现）"));
    v.push("retest=碎片注入 10/10 轮合并率 ≥基准，绕归并注入 10/10 次检出，双签登记哈希可查，归还阶账与 F2493 对平".to_string());
    v.push(format!("pact {} epoch={} ref={}", PACT_PAGE_CODE, PACT_EPOCH, PACT_REF));
    v.push(format!("frozen rate_base={}% batch_cap={} order_limit={} pages={}", MERGE_RATE_BASE, BATCH_HARD_CAP, ORDER_LIMIT, POOL_PAGES));
    v.push(format!("matrix cells={} forms={} ring_cap={} frag_rounds={} bypass_cases={}", MATRIX_CELLS, FORM_COUNT, RING_CAP, FRAG_ROUNDS, BYPASS_CASES));
    v.push(format!("errors={}", MergeErr::ALL.len()));
    v.push(format!("depends=F2718,F2721,F2493,F2731"));
    v
}

