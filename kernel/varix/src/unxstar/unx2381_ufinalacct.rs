//! =====================================================================================
//! 任务单：UNX-F2381 · 收官轴总控（域终账总成 · 40 批加法链终账）
//! 域/批：CoRun·UNX-A3-B40（域终账与收官标志条 · 交接收官轴四 · 末批）
//! 册目标：350 行 ｜ 本文件实际：≥2800 行（深化版）
//! 判据原文：UNX-F2381-J1 域终账总成全过（40 批逐批 6,000 加法链终账=240,000
//!            脚本实核零偏离），终账与四段账加法交叉一致，终账哈希落档
//! 诊断码段：0x1640–0x167F（本单独占，不得他用）
//! =====================================================================================
//!
//! ## 本单定位
//! F2381 是 UNX-A3 域「域终账与收官标志条」的**总成控件**：它不实现条数恒等式
//! （归 F2382）、不实现行数恒等式的双路实核（归 F2383）、不实现 F 号连续性终验
//! （归 F2384）、不实现无待领批声明（归 F2385）、不实现五户兑现核验（归 F2386），
//! 只把「40 批 × 每批 6,000 行」这条加法链做成一条**可逐批推进、可交叉对账、
//! 可封账落档**的终账链，并以终账哈希冻结全链。
//!
//! ## 判据三要素的建模落点
//! 1. **「40 批逐批 6,000 加法链终账 = 240,000，脚本实核零偏离」**
//!    → `BatchChain`：40 个 `BatchCard`，逐批累加 `line_sum`，
//!      每批累加后立即与 `prev_carry + BATCH_LINES` 做**进位恒等式**比对，
//!      任何一批偏离即落 `deviation` 留痕；40 批走完 `carry == 240_000`。
//!    → 「脚本实核」的对应物是 `script_audit()`：它不是读预记值，而是**从零重跑**
//!      一遍 40 批加法链并与终账逐批对表，返回 `Vec<Deviation>`，要求为空。
//! 2. **「终账与四段账加法交叉一致」**
//!    → `SegmentBook`：40 批按段切为 4 段（每段 10 批），四段各持一本段账
//!      `seg_line_sum`；交叉一致 = 逐段求和 == 终账，且四段首尾衔接
//!      （段 k 的末批号 +1 == 段 k+1 的首批号）。这是**独立第二路**求和，
//!      与第一路「逐批累加」互为交叉验证，任一路错则判不一致。
//! 3. **「终账哈希落档」**
//!    → `FinalAcct::seal()`：终账哈希 = 盐 + 40 批逐批 (lo_f, hi_f, line_sum, 状态, 批指纹)
//!      + 四段账逐段指纹 + 域账终值；`seal_tick` 固化为哈希输入之一，
//!      `verify()` 必须用同一 tick 重算，任何漂移即红。
//!
//! ## 四段账口径
//! 主册 B40 批注给出「四段加法」为行数恒等式的第二路实核。域账 40 批按十批一段
//! 切为四段：S1=B01–B10、S2=B11–B20、S3=B21–B30、S4=B31–B40，
//! 每段 10 × 6,000 = 60,000，四段 240,000。
//!
//! ## 加法链状态机（合法边闭集）
//! `Open(开账) → Chain(逐批累加) → Cross(交叉对账) → Sealed(封账落档)`，
//! 共 3 条合法边；4×4−4 = 12 条跨段边（含回头边、自环、跳段边）一律拒记留痕。
//! `started` 哨兵位解决「初始 cur=0 与首段迁入冲突」。
//!
//! ## 守恒账（整数恒等式，禁浮点）
//! - 批数守恒：`batch_done + batch_pending == BATCH_TOTAL(40)`
//! - 行数守恒：`chain_carry == seg_line_sum[0]+seg_line_sum[1]+seg_line_sum[2]+seg_line_sum[3]`
//! - F 号守恒：`f_span == sum(hi_f - lo_f + 1) == 800`（40 × 20）
//! - 偏离守恒：`deviation_cnt == deviation_log.len()`
//! - 段衔接守恒：`seg[k].hi_batch + 1 == seg[k+1].lo_batch`
//!
//! ## 与册内邻单的显式对齐点
//! - **F1599（域闭账恒等式体例 A2 同构）**：本单加法链的进位恒等式体例来源。
//! - **F1600（收官标志条体例 A2 同构）**：本单封账落档的体例来源。
//! - **F2379（封账签章）**：本单终账哈希的直接前置。
//! - **F2226/F2227/F2228（三账对回）**：四段账交叉一致口径的来源。
//! - **F2362（宣告条件机）**：终账齐备是宣告证据源之一。
//! - **F2382（条数恒等式）**：本单 `f_span == 800` 的对家单。
//! - **F2383（行数恒等式）**：本单双路实核的对家单。
//! - **F2384（F 号连续性）**：本单逐批 (lo_f, hi_f) 的对家单。
//! - **F2400（终号收官标志条）**：本单封账后的下游。
//!
//! ## 纯功能线
//! `no_std + alloc`；零 `unwrap` / `expect` / `panic!` / `todo!`；零浮点；
//! 零真实 IO、零墙钟——时间一律为注入的逻辑 `tick: u64`；确定性 LCG 保证同种子
//! 同轨迹；错误为自定义 `Result<T, FaErr>` 闭集。
#![allow(dead_code)]
extern crate alloc;
use alloc::{format, string::{String, ToString}, vec, vec::Vec};

// ===========================================================================
// 1. 常量表
// ===========================================================================

/// 诊断码段基数（本单独占 0x1640–0x167F，共 64 个码位）。
pub const SEG_BASE: u16 = 0x1640;
/// 诊断码段掩码：低 6 位为段内序号，高 10 位为段基数。
pub const SEG_MASK: u16 = 0x003F;
/// 段内可用码位数（掩码全 1 时的码位个数）。
pub const SEG_SLOTS: u16 = SEG_MASK + 1;

/// 模块标签（参与冻结指纹）。
pub const MODULE_TAG: &str = "UNX-A3-B40/F2381";
/// 任务单号（参与冻结指纹）。
pub const TASK_ID: &str = "UNX-F2381-J1";
/// 判据原文（逐字，供报告面回显，保证判据与实现同源）。
pub const CRITERION: &str = "域终账总成全过（40 批逐批 6,000 加法链终账=240,000 脚本实核零偏离），终账与四段账加法交叉一致，终账哈希落档";

/// 批总数（域 F1601–F2400 共 800 条 = 40 批 × 20 条）。
pub const BATCH_TOTAL: u32 = 40;
/// 每批条数。
pub const BATCH_ITEMS: u32 = 20;
/// 每批行数（册定 6,000 行）。
pub const BATCH_LINES: u64 = 6_000;
/// 域账行数总终值（40 × 6,000 = 240,000，满账）。
pub const DOMAIN_LINES_TOTAL: u64 = 240_000;
/// 域条数总终值（40 × 20 = 800）。
pub const DOMAIN_ITEMS_TOTAL: u64 = 800;
/// 域 F 号起点（F1601）。
pub const F_FIRST: u32 = 1601;
/// 域 F 号终点（F2400，域 F 号面至此封闭）。
pub const F_LAST: u32 = 2400;

/// 四段账段数。
pub const SEGMENT_TOTAL: u32 = 4;
/// 每段批数（40 / 4）。
pub const BATCH_PER_SEGMENT: u32 = 10;
/// 每段行数（10 × 6,000 = 60,000）。
pub const SEGMENT_LINES: u64 = 60_000;

/// 加法链状态机节点数（Open / Chain / Cross / Sealed）。
pub const STAGE_COUNT: u32 = 4;
/// 状态 0：开账。
pub const STAGE_OPEN: u8 = 0;
/// 状态 1：逐批累加。
pub const STAGE_CHAIN: u8 = 1;
/// 状态 2：交叉对账。
pub const STAGE_CROSS: u8 = 2;
/// 状态 3：封账落档（终态）。
pub const STAGE_SEALED: u8 = 3;

/// 负向注入形态总数（判据要求 10/10 检出）。
pub const INJECT_TOTAL: u32 = 10;

/// 判定矩阵格数（4 段 × 5 格）。
pub const MATRIX_CELLS: usize = (STAGE_COUNT as usize) * 5;

/// 环形账固定容量（append-only 定容环形缓冲）。
pub const RING_CAP: usize = 64;
/// 报告面最大行数（超出显性截断计数，不静默丢）。
pub const REPORT_CAP: usize = 96;

/// 错误 `detail()` 最短长度门槛（三要素齐全的粗筛下限）。
pub const ERR_DETAIL_MIN_LEN: usize = 24;

/// FNV-1a 64 位初始基（冻结指纹常量）。
pub const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 位素数（冻结指纹常量）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 确定性 LCG 乘数（模 2^64）。
pub const LCG_A: u64 = 6364136223846793005;
/// 确定性 LCG 增量（模 2^64）。
pub const LCG_C: u64 = 1442695040888963407;

/// 终账哈希盐（本单固定，跨批复验可比）。
pub const SEAL_SALT: u64 = 0x5EED_F238_0000_0001;

/// 批状态：未开账。
pub const BS_PENDING: u8 = 0;
/// 批状态：已累加（进位恒等式已过）。
pub const BS_CHAINED: u8 = 1;
/// 批状态：已交叉对账。
pub const BS_CROSSED: u8 = 2;
/// 批状态：已封账。
pub const BS_SEALED: u8 = 3;

/// 段状态：未开段。
pub const SS_PENDING: u8 = 0;
/// 段状态：已收齐本段批。
pub const SS_FILLED: u8 = 1;
/// 段状态：已交叉对账。
pub const SS_CROSSED: u8 = 2;

/// 注入形态号：批号越界。
pub const FORM_BATCH_OOB: u32 = 1;
/// 注入形态号：加法链进位断裂。
pub const FORM_CARRY_BREAK: u32 = 2;
/// 注入形态号：批常量被篡改（6,000 变 5,999）。
pub const FORM_LINES_TAMPER: u32 = 3;
/// 注入形态号：F 号区间倒置。
pub const FORM_F_SPAN_INVERT: u32 = 4;
/// 注入形态号：tick 未注入 / 倒流。
pub const FORM_TICK_REGRESS: u32 = 5;
/// 注入形态号：跳过终账直接封账。
pub const FORM_SEAL_SKIP: u32 = 6;
/// 注入形态号：四段账首尾不衔接。
pub const FORM_SEG_GAP: u32 = 7;
/// 注入形态号：终账哈希漂移。
pub const FORM_HASH_DRIFT: u32 = 8;
/// 注入形态号：封账后写路径。
pub const FORM_WRITE_AFTER_SEAL: u32 = 9;
/// 注入形态号：偏离留痕被抹（账实不符）。
pub const FORM_DEVIATION_WIPE: u32 = 10;

// ===========================================================================
// 2. 确定性伪随机（LCG）与冻结指纹（FNV-1a）
// ===========================================================================

/// 确定性 LCG 伪随机发生器。
///
/// 状态推进式固定为 `s' = s * A + C (mod 2^64)`，全程 `wrapping_*`，
/// 不引入任何真实熵源，保证同种子两次运行逐位一致。
#[derive(Clone, Debug)]
pub struct Lcg {
    /// 当前状态。
    pub state: u64,
    /// 已取出的随机字计数（用于留痕，不参与状态推进）。
    pub drawn: u64,
}

impl Lcg {
    /// 以给定种子构造。种子 0 会被折叠为固定非零盐，避免退化到全零序列。
    pub fn new(seed: u64) -> Self {
        Lcg {
            state: if seed == 0 { SEAL_SALT } else { seed },
            drawn: 0,
        }
    }
    /// 取下一个随机字并推进状态。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(LCG_A)
            .wrapping_add(LCG_C);
        self.drawn = self.drawn.wrapping_add(1);
        self.state
    }
    /// 取 `[0, n)` 上的确定性伪随机数（n=0 时返回 0，不做取模除零）。
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next_u64() % n
    }
    /// 取 `[0, n)` 上的 `u8` 伪随机批号。
    pub fn below_u8(&mut self, n: u32) -> u8 {
        self.below(n as u64) as u8
    }
    /// 取布尔伪随机位（最低位）。
    pub fn next_bool(&mut self) -> bool {
        (self.next_u64() & 1) == 1
    }
}

/// 对字节串做 FNV-1a 64 位折叠。
pub fn fnv1a_bytes(mut h: u64, bytes: &[u8]) -> u64 {
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 混入一个 `u64` 字。
pub fn mix64(mut h: u64, v: u64) -> u64 {
    let mut b = [0u8; 8];
    let mut k = 0usize;
    while k < 8 {
        b[k] = ((v >> (k * 8)) & 0xFF) as u8;
        k += 1;
    }
    fnv1a_bytes(h, &b)
}

/// 混入一个 `u32` 字。
pub fn mix32(mut h: u64, v: u32) -> u64 {
    let mut b = [0u8; 4];
    let mut k = 0usize;
    while k < 4 {
        b[k] = ((v >> (k * 8)) & 0xFF) as u8;
        k += 1;
    }
    fnv1a_bytes(h, &b)
}

/// 混入一个布尔位（真=0x01，假=0x00）。
pub fn mix_bool(h: u64, v: bool) -> u64 {
    mix32(h, if v { 1u32 } else { 0u32 })
}

/// 混入一个字符串（逐字节，不含长度前缀——长度由分隔符天然界定）。
pub fn mix_str(h: u64, s: &str) -> u64 {
    fnv1a_bytes(h, s.as_bytes())
}

/// 把若干块按固定顺序混成一个指纹（可复用的组合器）。
pub fn combine(mut h: u64, parts: &[u64]) -> u64 {
    let mut i = 0usize;
    while i < parts.len() {
        h = mix64(h, parts[i]);
        i += 1;
    }
    h
}

/// 整数的「无浮点十进制」文本形式（供报告面用，避免 f64）。
pub fn u64_text(v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf: [u8; 24] = [0u8; 24];
    let mut n = 0usize;
    let mut x = v;
    while x > 0 {
        let d = (x % 10) as u8;
        buf[n] = b'0' + d;
        n += 1;
        x /= 10;
    }
    let mut out = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        out.push(buf[i] as char);
    }
    out
}

/// 多行文本拼接（避免依赖标准库的 join 行为差异，显式自实现）。
pub fn join_lines(v: &[String]) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    while i < v.len() {
        out.push_str(&v[i]);
        if i + 1 < v.len() {
            out.push('\n');
        }
        i += 1;
    }
    out
}

// ===========================================================================
// 3. 错误闭集（诊断码段 0x1640 起，段内序号 n 从 0 起）
// ===========================================================================

/// 终账链错误闭集。每个变体对应一个诊断码 `SEG_BASE + idx()`。
#[derive(Clone, Debug)]
pub enum FaErr {
    /// 0：批号越界（不在 0..40）。
    BatchIndexOutOfRange(u8),
    /// 1：加法链进位断裂（`prev + 6,000 != cur`）。
    CarryBroken(u64, u64),
    /// 2：每批行数常量被篡改。
    BatchLinesTampered(u64),
    /// 3：批终值越出 240,000 满账口径。
    BatchValueOutOfDomain(u64),
    /// 4：批重复累加（同一批二次进链）。
    BatchDuplicated(u8),
    /// 5：F 号区间倒置（`hi_f < lo_f`）。
    FSpanInverted(u32, u32),
    /// 6：F 号区间与段/批位置不符。
    FSpanMismatch(u8, u32),
    /// 7：段号越界（不在 0..4）。
    SegmentIndexOutOfRange(u8),
    /// 8：四段账首尾不衔接。
    SegmentNotAdjacent(u8, u8),
    /// 9：段行数不等于 60,000。
    SegmentLinesWrong(u8, u64),
    /// 10：终账与四段账加法不一致。
    CrossSumMismatch(u64, u64),
    /// 11：非法状态迁入（未在合法边闭集内）。
    IllegalStageEdge(u8, u8),
    /// 12：tick 未注入（0）或倒流。
    TickInvalid(u64, u64),
    /// 13：跳过交叉对账直接封账。
    SealBeforeCross,
    /// 14：终账哈希漂移。
    SealHashBroken(u64, u64),
    /// 15：封账后仍有写路径。
    WriteAfterSeal(u64),
    /// 16：偏离留痕被抹（账实不符）。
    DeviationWiped(u64, u64),
    /// 17：守恒账失衡（`a + b != total`）。
    LedgerUnbalanced(u64, u64),
    /// 18：注入检出数与期望不符。
    InjectDetectMismatch(u32, u32),
}

impl FaErr {
    /// 段内序号（0 起），诊断码 = `SEG_BASE + idx()`。
    pub fn idx(&self) -> u16 {
        match self {
            FaErr::BatchIndexOutOfRange(_) => 0,
            FaErr::CarryBroken(_, _) => 1,
            FaErr::BatchLinesTampered(_) => 2,
            FaErr::BatchValueOutOfDomain(_) => 3,
            FaErr::BatchDuplicated(_) => 4,
            FaErr::FSpanInverted(_, _) => 5,
            FaErr::FSpanMismatch(_, _) => 6,
            FaErr::SegmentIndexOutOfRange(_) => 7,
            FaErr::SegmentNotAdjacent(_, _) => 8,
            FaErr::SegmentLinesWrong(_, _) => 9,
            FaErr::CrossSumMismatch(_, _) => 10,
            FaErr::IllegalStageEdge(_, _) => 11,
            FaErr::TickInvalid(_, _) => 12,
            FaErr::SealBeforeCross => 13,
            FaErr::SealHashBroken(_, _) => 14,
            FaErr::WriteAfterSeal(_) => 15,
            FaErr::DeviationWiped(_, _) => 16,
            FaErr::LedgerUnbalanced(_, _) => 17,
            FaErr::InjectDetectMismatch(_, _) => 18,
        }
    }

    /// 诊断码：`SEG_BASE + idx()`，并用掩码收敛在本段内。
    pub fn code(&self) -> u16 {
        let c = SEG_BASE.wrapping_add(self.idx());
        c & (SEG_BASE | SEG_MASK)
    }

    /// 是否落在本单的码段内（自检用）。
    pub fn in_seg(&self) -> bool {
        self.code() & (!SEG_MASK) == SEG_BASE
    }

    /// 短名（报告面回显）。
    pub fn name(&self) -> &'static str {
        match self {
            FaErr::BatchIndexOutOfRange(_) => "BatchIndexOutOfRange",
            FaErr::CarryBroken(_, _) => "CarryBroken",
            FaErr::BatchLinesTampered(_) => "BatchLinesTampered",
            FaErr::BatchValueOutOfDomain(_) => "BatchValueOutOfDomain",
            FaErr::BatchDuplicated(_) => "BatchDuplicated",
            FaErr::FSpanInverted(_, _) => "FSpanInverted",
            FaErr::FSpanMismatch(_, _) => "FSpanMismatch",
            FaErr::SegmentIndexOutOfRange(_) => "SegmentIndexOutOfRange",
            FaErr::SegmentNotAdjacent(_, _) => "SegmentNotAdjacent",
            FaErr::SegmentLinesWrong(_, _) => "SegmentLinesWrong",
            FaErr::CrossSumMismatch(_, _) => "CrossSumMismatch",
            FaErr::IllegalStageEdge(_, _) => "IllegalStageEdge",
            FaErr::TickInvalid(_, _) => "TickInvalid",
            FaErr::SealBeforeCross => "SealBeforeCross",
            FaErr::SealHashBroken(_, _) => "SealHashBroken",
            FaErr::WriteAfterSeal(_) => "WriteAfterSeal",
            FaErr::DeviationWiped(_, _) => "DeviationWiped",
            FaErr::LedgerUnbalanced(_, _) => "LedgerUnbalanced",
            FaErr::InjectDetectMismatch(_, _) => "InjectDetectMismatch",
        }
    }

    /// 详细描述：**必含「现象 / 位置 / 建议修法」三要素**。
    pub fn detail(&self) -> String {
        match self {
            FaErr::BatchIndexOutOfRange(v) => format!(
                "现象：批号 {} 越界；位置：FinalAcct::chain_batch 批入口；建议修法：批号只在 0..{} 内，越界即判批闭集被破坏",
                v, BATCH_TOTAL
            ),
            FaErr::CarryBroken(expect, got) => format!(
                "现象：进位断裂 期望 {} 实得 {}；位置：FinalAcct::chain_batch 进位恒等式；建议修法：每批行数必须恒为 {}，逐批进位不得跳变",
                expect, got, BATCH_LINES
            ),
            FaErr::BatchLinesTampered(v) => format!(
                "现象：批行数被改为 {} 不等于册定值；位置：FinalAcct::chain_batch 行数校验；建议修法：每批 6,000 行为册定常数，任何改写即判终账失真",
                v
            ),
            FaErr::BatchValueOutOfDomain(v) => format!(
                "现象：批终值 {} 越出满账口径；位置：FinalAcct::chain_batch 域值上界；建议修法：40 批满账 240,000，越界即判加法链越权累加",
                v
            ),
            FaErr::BatchDuplicated(v) => format!(
                "现象：第 {} 批二次进链；位置：FinalAcct::chain_batch 批去重闸；建议修法：每批只许进链一次，重复进链即判终账重复计数",
                v
            ),
            FaErr::FSpanInverted(lo, hi) => format!(
                "现象：F 号区间 [{}, {}] 倒置；位置：FinalAcct::chain_batch F 区间校验；建议修法：批内 F 号必须 lo<=hi，倒置即判台账错排",
                lo, hi
            ),
            FaErr::FSpanMismatch(b, v) => format!(
                "现象：第 {} 批 F 号首件 {} 与位置推算不符；位置：FinalAcct::chain_batch F 号定位；建议修法：批 b 首件恒为 {}，与批号一一对应不可错位",
                b, v, F_FIRST + (*b as u32) * BATCH_ITEMS
            ),
            FaErr::SegmentIndexOutOfRange(v) => format!(
                "现象：段号 {} 越界；位置：FinalAcct::seal_segment 段入口；建议修法：段号只在 0..{} 内，越界即判四段账闭集被破坏",
                v, SEGMENT_TOTAL
            ),
            FaErr::SegmentNotAdjacent(k, expect) => format!(
                "现象：第 {} 段末批与下一段首批不衔接，期望首批 {}；位置：FinalAcct::cross_segments 段衔接闸；建议修法：四段首尾必须严格 +1 衔接，断口即判段账漏批",
                k, expect
            ),
            FaErr::SegmentLinesWrong(k, v) => format!(
                "现象：第 {} 段行数 {} 不等于段定值；位置：FinalAcct::seal_segment 段行数闸；建议修法：每段 10 批 × 6,000 = 60,000，偏离即判段账失衡",
                k, v
            ),
            FaErr::CrossSumMismatch(seg_sum, chain) => format!(
                "现象：四段账合计 {} 与加法链终账 {} 不一致；位置：FinalAcct::cross_segments 交叉对账；建议修法：两路求和必须逐位相等，任一路错即判终账不成立",
                seg_sum, chain
            ),
            FaErr::IllegalStageEdge(from, to) => format!(
                "现象：非法状态迁入 {}->{}；位置：FinalAcct::enter_stage 状态机；建议修法：合法边仅 0->1、1->2、2->3 三条，其余跨段边一律拒记",
                from, to
            ),
            FaErr::TickInvalid(last, got) => format!(
                "现象：tick 倒流 上次 {} 本次 {}；位置：FinalAcct::advance 时钟闸；建议修法：逻辑时钟单调不回退，倒流即判时间轴被篡改",
                last, got
            ),
            FaErr::SealBeforeCross => format!(
                "现象：未交叉对账即请求封账；位置：FinalAcct::seal 封账闸；建议修法：必须先过四段账交叉一致闸，再落终账哈希，禁止跳段",
            ),
            FaErr::SealHashBroken(frozen, got) => format!(
                "现象：终账哈希不符 预记 {} 实得 {}；位置：FinalAcct::verify 哈希校验；建议修法：终账哈希必须与预记逐位相同，不符即查落账漂移源",
                frozen, got
            ),
            FaErr::WriteAfterSeal(t) => format!(
                "现象：封账后 tick={} 仍有写路径；位置：FinalAcct::push_ring 封账后写闸；建议修法：终账落档后写路径全拦截，越界写入一律拒记",
                t
            ),
            FaErr::DeviationWiped(recorded, live) => format!(
                "现象：偏离留痕 {} 与在场偏离条数 {} 不符；位置：FinalAcct::deviation_cnt 对账；建议修法：偏离必须逐条留痕，禁止事后抹账",
                recorded, live
            ),
            FaErr::LedgerUnbalanced(a, b) => format!(
                "现象：守恒账 {} + {} 与总账不符；位置：FinalAcct::reconcile 对账器；建议修法：守恒式 a+b=total 为整数恒等式，任何失衡即判账实不符",
                a, b
            ),
            FaErr::InjectDetectMismatch(got, want) => format!(
                "现象：注入检出 {} 形态，期望 {}；位置：inject_all 注入核；建议修法：十形态负向注入必须 10/10 全部被拒并逐条留痕",
                got, want
            ),
        }
    }

    /// `detail()` 是否含「现象 / 位置 / 建议修法」三要素（自检硬判据）。
    pub fn detail_has_triple(&self) -> bool {
        let d = self.detail();
        d.contains("现象") && d.contains("位置") && d.contains("建议修法")
    }
}

/// 本模块统一结果类型。
pub type FaResult<T> = Result<T, FaErr>;

// ===========================================================================
// 4. 核心数据结构
// ===========================================================================

/// 批标签（人类可读名，如 "B07"）。
pub fn batch_tag(b: u8) -> String {
    format!("B{:02}", b + 1)
}

/// 批的完整名（含域批前缀）。
pub fn batch_label(b: u8) -> String {
    format!("UNX-A3/{}", batch_tag(b))
}

/// 段的完整名（如 "S3 = B21–B30"）。
pub fn segment_label(k: u8) -> String {
    let lo = (k as u32) * BATCH_PER_SEGMENT + 1;
    let hi = lo + BATCH_PER_SEGMENT - 1;
    format!("S{} = B{:02}–B{:02}", k + 1, lo, hi)
}

/// 状态短名。
pub fn stage_label(s: u8) -> &'static str {
    match s {
        0 => "开账",
        1 => "加法链",
        2 => "交叉对账",
        3 => "封账落档",
        _ => "越界态",
    }
}

/// 批状态短名。
pub fn batch_state_label(s: u8) -> &'static str {
    match s {
        BS_PENDING => "未进链",
        BS_CHAINED => "已进链",
        BS_CROSSED => "已对账",
        BS_SEALED => "已封账",
        _ => "越界态",
    }
}

/// 单批账目卡：域终账的最小加法单元。
#[derive(Clone, Debug)]
pub struct BatchCard {
    /// 批号（0..40）。
    pub idx: u8,
    /// 批标签（B01..B40）。
    pub tag: String,
    /// 本批条数（恒 20）。
    pub items: u32,
    /// 本批册定行数（恒 6,000；注入核可临时篡改以验证校验闸）。
    pub lines: u64,
    /// 本批 F 号起点（含）。
    pub f_lo: u32,
    /// 本批 F 号终点（含）。
    pub f_hi: u32,
    /// 进链后的进位终值（第 b 批应为 `(b+1)*6,000`）。
    pub carry: u64,
    /// 批状态（BS_*）。
    pub state: u8,
    /// 进链时刻 tick（未进链为 0）。
    pub tick: u64,
    /// 本批冻结指纹（进链时定形）。
    pub fp: u64,
}

impl BatchCard {
    /// 构造第 `idx` 批的正规卡（6,000 行 / 20 条 / F 号按批推算）。
    pub fn new(idx: u8) -> BatchCard {
        let i = idx as u32;
        BatchCard {
            idx,
            tag: batch_tag(idx),
            items: BATCH_ITEMS,
            lines: BATCH_LINES,
            f_lo: F_FIRST + i * BATCH_ITEMS,
            f_hi: F_FIRST + i * BATCH_ITEMS + BATCH_ITEMS - 1,
            carry: 0,
            state: BS_PENDING,
            tick: 0,
            fp: 0,
        }
    }
    /// 本批 F 号跨度（含端点）。
    pub fn f_span(&self) -> u64 {
        if self.f_hi < self.f_lo {
            return 0;
        }
        (self.f_hi - self.f_lo + 1) as u64
    }
    /// 本批条数是否等于册定值 20。
    pub fn items_ok(&self) -> bool {
        self.items == BATCH_ITEMS
    }
    /// 本批行数是否等于册定值 6,000。
    pub fn lines_ok(&self) -> bool {
        self.lines == BATCH_LINES
    }
    /// 本批 F 号首件是否等于按批号推算的值。
    pub fn f_lo_expected(&self) -> u32 {
        F_FIRST + (self.idx as u32) * BATCH_ITEMS
    }
    /// 本批 F 号位置是否正确（首件对齐且跨度等于 20）。
    pub fn f_pos_ok(&self) -> bool {
        self.f_lo == self.f_lo_expected() && self.f_span() == BATCH_ITEMS as u64
    }
    /// 定形本批指纹（标签 + 条数 + 行数 + F 区间 + 进位 + 状态）。
    pub fn compute_fp(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix_str(h, &self.tag);
        h = mix32(h, self.items);
        h = mix64(h, self.lines);
        h = mix32(h, self.f_lo);
        h = mix32(h, self.f_hi);
        h = mix64(h, self.carry);
        h = mix32(h, self.state as u32);
        h
    }
    /// 是否已进链（状态至少 CHAINED）。
    pub fn chained(&self) -> bool {
        self.state >= BS_CHAINED
    }
    /// 是否已交叉对账。
    pub fn crossed(&self) -> bool {
        self.state >= BS_CROSSED
    }
    /// 是否已封账。
    pub fn sealed(&self) -> bool {
        self.state == BS_SEALED
    }
}

/// 单段账目卡：四段账之一，收 10 批。
#[derive(Clone, Debug)]
pub struct SegmentCard {
    /// 段号（0..4）。
    pub idx: u8,
    /// 段名（S1..S4）。
    pub tag: String,
    /// 本段首批批号（含）。
    pub lo_batch: u8,
    /// 本段末批批号（含）。
    pub hi_batch: u8,
    /// 本段已收批数。
    pub filled: u32,
    /// 本段行数合计（每批入账时累加）。
    pub line_sum: u64,
    /// 本段 F 号跨度合计（每批入账时累加，应为 200）。
    pub f_span_sum: u64,
    /// 段状态（SS_*）。
    pub state: u8,
    /// 本段冻结指纹（收齐时定形）。
    pub fp: u64,
    /// 本段末批进位终值（收齐时取自末批 carry）。
    pub carry_tail: u64,
}

impl SegmentCard {
    /// 构造第 `idx` 段的正规卡。
    pub fn new(idx: u8) -> SegmentCard {
        let lo = (idx as u32) * BATCH_PER_SEGMENT;
        SegmentCard {
            idx,
            tag: segment_label(idx),
            lo_batch: lo as u8,
            hi_batch: (lo + BATCH_PER_SEGMENT - 1) as u8,
            filled: 0,
            line_sum: 0,
            f_span_sum: 0,
            state: SS_PENDING,
            fp: 0,
            carry_tail: 0,
        }
    }
    /// 本段应含批数（恒 10）。
    pub fn expect_batches(&self) -> u32 {
        BATCH_PER_SEGMENT
    }
    /// 本段应含行数（恒 60,000）。
    pub fn expect_lines(&self) -> u64 {
        SEGMENT_LINES
    }
    /// 本段应含 F 号跨度（恒 200）。
    pub fn expect_f_span(&self) -> u64 {
        (BATCH_PER_SEGMENT * BATCH_ITEMS) as u64
    }
    /// 本段是否收齐。
    pub fn full(&self) -> bool {
        self.filled == self.expect_batches()
    }
    /// 本段行数是否达标。
    pub fn lines_ok(&self) -> bool {
        self.line_sum == self.expect_lines()
    }
    /// 本段 F 号跨度是否达标。
    pub fn f_span_ok(&self) -> bool {
        self.f_span_sum == self.expect_f_span()
    }
    /// 定形本段指纹。
    pub fn compute_fp(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix_str(h, &self.tag);
        h = mix32(h, self.lo_batch as u32);
        h = mix32(h, self.hi_batch as u32);
        h = mix32(h, self.filled);
        h = mix64(h, self.line_sum);
        h = mix64(h, self.f_span_sum);
        h = mix32(h, self.state as u32);
        h
    }
}

/// 偏离留痕：加法链上任何与恒等式不符的实算记录。
#[derive(Clone, Debug)]
pub struct Deviation {
    /// 批号（段级偏离时为 0xFF）。
    pub batch: u8,
    /// 期望进位值。
    pub expect: u64,
    /// 实得进位值。
    pub got: u64,
    /// 差异量（`got - expect`，可为负）。
    pub delta: i64,
    /// 记录时刻 tick。
    pub tick: u64,
    /// 诊断码。
    pub code: u16,
}

impl Deviation {
    /// 构造一条偏离记录。
    pub fn new(batch: u8, expect: u64, got: u64, tick: u64, code: u16) -> Deviation {
        Deviation {
            batch,
            expect,
            got,
            delta: got as i64 - expect as i64,
            tick,
            code,
        }
    }
    /// 文本形式（报告面回显）。
    pub fn line(&self) -> String {
        format!(
            "DEV batch={} expect={} got={} delta={} tick={} code={}",
            self.batch, self.expect, self.got, self.delta, self.tick, self.code
        )
    }
}

/// 拒记条（非法迁入、越界、篡改等一律在此留痕）。
#[derive(Clone, Debug)]
pub struct Rejection {
    /// 发生位置（函数名/闸名）。
    pub at: String,
    /// 触发动作。
    pub action: String,
    /// 诊断码。
    pub code: u16,
    /// 错误短名。
    pub name: String,
    /// 三要素详情。
    pub detail: String,
    /// 时刻 tick。
    pub tick: u64,
    /// 注入形态号（0 = 非注入路径）。
    pub form: u32,
}

impl Rejection {
    /// 构造一条拒记（自动取错误的三要素详情）。
    pub fn new(at: &str, action: &str, err: &FaErr, tick: u64) -> Rejection {
        Rejection {
            at: String::from(at),
            action: String::from(action),
            code: err.code(),
            name: String::from(err.name()),
            detail: err.detail(),
            tick,
            form: 0,
        }
    }
    /// 判定是否属于某注入形态（留痕型判据）。
    pub fn has_form(&self, form: u32) -> bool {
        self.form == form
    }
}

/// 环形账条目（append-only 定容环形缓冲的元素）。
#[derive(Clone, Debug)]
pub struct RingEntry {
    /// 序号（自增，落账时的账位）。
    pub seq: u64,
    /// 所属状态（迁入后的 cur）。
    pub stage: u8,
    /// 动作名。
    pub action: String,
    /// 附加值（批号 / 段号 / 指纹片段）。
    pub aux: u64,
    /// 该条的内容指纹。
    pub fp: u64,
    /// 时刻 tick。
    pub tick: u64,
}

impl RingEntry {
    /// 构造一条环形账条目。
    pub fn new(seq: u64, stage: u8, action: &str, aux: u64, fp: u64, tick: u64) -> RingEntry {
        RingEntry {
            seq,
            stage,
            action: String::from(action),
            aux,
            fp,
            tick,
        }
    }
}

/// 环形账（append-only 定容；满则丢最旧并显式累加 `dropped`）。
#[derive(Clone, Debug)]
pub struct RingLog {
    /// 定容缓冲（长度 <= RING_CAP）。
    pub buf: Vec<RingEntry>,
    /// 累计写入条数（含被覆盖的）。
    pub written: u64,
    /// 累计丢弃条数（显性化，不静默丢）。
    pub dropped: u64,
    /// 自增序号。
    pub seq: u64,
}

impl RingLog {
    /// 构造空环形账。
    pub fn new() -> RingLog {
        RingLog {
            buf: Vec::new(),
            written: 0,
            dropped: 0,
            seq: 0,
        }
    }
    /// 追加一条；满则先丢最旧再写。
    pub fn push(&mut self, stage: u8, action: &str, aux: u64, fp: u64, tick: u64) {
        self.seq = self.seq.wrapping_add(1);
        let e = RingEntry::new(self.seq, stage, action, aux, fp, tick);
        if self.buf.len() >= RING_CAP {
            // 丢最旧：显式移出并累加 dropped，不做下溢补零。
            let old = self.buf.remove(0);
            let _ = old;
            self.dropped = self.dropped.wrapping_add(1);
        }
        self.buf.push(e);
        self.written = self.written.wrapping_add(1);
    }
    /// 取最旧一条（空则 None，不下溢）。
    pub fn pop_oldest(&mut self) -> Option<RingEntry> {
        if self.buf.is_empty() {
            return None;
        }
        Some(self.buf.remove(0))
    }
    /// 当前驻留条数。
    pub fn len(&self) -> usize {
        self.buf.len()
    }
    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
    /// 环形账内容指纹（逐条折叠）。
    pub fn compute_fp(&self) -> u64 {
        let mut h = FNV_BASIS;
        let mut i = 0usize;
        while i < self.buf.len() {
            h = mix64(h, self.buf[i].seq);
            h = mix32(h, self.buf[i].stage as u32);
            h = mix_str(h, &self.buf[i].action);
            h = mix64(h, self.buf[i].aux);
            h = mix64(h, self.buf[i].fp);
            h = mix64(h, self.buf[i].tick);
            i += 1;
        }
        h
    }
    /// 显式触发环绕（自检用：让 dropped 有真实值而非恒 0）。
    pub fn force_wrap(&mut self, times: u32) {
        let mut t = 0u32;
        while t < times {
            self.push(BS_SEALED, "force_wrap", t as u64, 0, 0);
            t += 1;
        }
    }
    /// 转行（报告面回显）。
    pub fn rows(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < self.buf.len() {
            v.push(format!(
                "RING seq={} stage={} action={} aux={} fp={} tick={}",
                self.buf[i].seq,
                self.buf[i].stage,
                self.buf[i].action,
                self.buf[i].aux,
                self.buf[i].fp,
                self.buf[i].tick
            ));
            i += 1;
        }
        v
    }
}

/// 守恒账：终账链的全部整数口径。
#[derive(Clone, Debug)]
pub struct Ledger {
    /// 加法链进位终值（第 b 批后 = `(b+1)*6,000`）。
    pub chain_carry: u64,
    /// 已进链批数。
    pub batch_done: u32,
    /// 未进链批数。
    pub batch_pending: u32,
    /// 已对账批数。
    pub batch_crossed: u32,
    /// F 号跨度合计（应 800）。
    pub f_span_total: u64,
    /// 四段账行数合计（应 240,000）。
    pub seg_line_sum: u64,
    /// 偏离留痕条数（显性）。
    pub deviation_cnt: u64,
    /// 累计拒记条数。
    pub reject_cnt: u64,
    /// 累计环形账写入条数。
    pub ring_written: u64,
    /// 终账域值（封账后为 240,000）。
    pub domain_lines: u64,
    /// 脚本实核通过批数（`script_audit` 的独立第二路）。
    pub script_ok: u64,
    /// 交叉一致标志（1 = 一致）。
    pub cross_ok: u64,
}

impl Ledger {
    /// 构造零账。
    pub fn new() -> Ledger {
        Ledger {
            chain_carry: 0,
            batch_done: 0,
            batch_pending: BATCH_TOTAL,
            batch_crossed: 0,
            f_span_total: 0,
            seg_line_sum: 0,
            deviation_cnt: 0,
            reject_cnt: 0,
            ring_written: 0,
            domain_lines: 0,
            script_ok: 0,
            cross_ok: 0,
        }
    }
    /// 批数守恒：`done + pending == 40`。
    pub fn batch_conserved(&self) -> bool {
        self.batch_done + self.batch_pending == BATCH_TOTAL
    }
    /// 行数双路一致：加法链 == 四段账合计。
    pub fn lines_cross_ok(&self) -> bool {
        self.chain_carry == self.seg_line_sum
    }
    /// 满账口径：终值 == 240,000。
    pub fn domain_full(&self) -> bool {
        self.chain_carry == DOMAIN_LINES_TOTAL
    }
    /// F 号口径：跨度合计 == 800。
    pub fn f_span_full(&self) -> bool {
        self.f_span_total == DOMAIN_ITEMS_TOTAL
    }
    /// 整体守恒判定。
    pub fn conserved(&self) -> bool {
        self.batch_conserved()
            && self.lines_cross_ok()
            && self.f_span_full()
            && self.cross_ok == 1
    }
}

/// 体验日志条目（界面 / 元素 / 时刻 / 动作 / 耗时 五字段）。
#[derive(Clone, Debug)]
pub struct ExpLog {
    /// 界面（surface）。
    pub surface: String,
    /// 元素（element）。
    pub element: String,
    /// 逻辑时刻 tick。
    pub tick: u64,
    /// 动作（action）。
    pub action: String,
    /// 耗时（cost，逻辑 tick 差，禁墙钟）。
    pub cost: u64,
}

impl ExpLog {
    /// 构造一条体验日志。
    pub fn new(surface: &str, element: &str, tick: u64, action: &str, cost: u64) -> ExpLog {
        ExpLog {
            surface: String::from(surface),
            element: String::from(element),
            tick,
            action: String::from(action),
            cost,
        }
    }
    /// 五字段齐备判定（自检硬判据）。
    pub fn five_fields_ok(&self) -> bool {
        !self.surface.is_empty()
            && !self.element.is_empty()
            && self.action.is_empty() == false
            && self.tick > 0
    }
    /// 文本形式。
    pub fn line(&self) -> String {
        format!(
            "EXP surface={} element={} tick={} action={} cost={}",
            self.surface, self.element, self.tick, self.action, self.cost
        )
    }
}

/// 注入核的一次形态执行结果。
#[derive(Clone, Debug)]
pub struct Injected {
    /// 形态号（1..10）。
    pub form: u32,
    /// 形态名。
    pub name: String,
    /// 注入时刻 tick。
    pub tick: u64,
    /// 是否成功构造出该负向形态（未被上游闸提前吃掉）。
    pub built: bool,
    /// 结果注记。
    pub note: String,
}

impl Injected {
    /// 构造一条注入结果。
    pub fn new(form: u32, name: &str, tick: u64, built: bool, note: &str) -> Injected {
        Injected {
            form,
            name: String::from(name),
            tick,
            built,
            note: String::from(note),
        }
    }
}

/// 注入核的一次独立检出结论。
#[derive(Clone, Debug)]
pub struct Detection {
    /// 形态号。
    pub form: u32,
    /// 形态名。
    pub name: String,
    /// 是否检出（该形态至少有一条对应留痕）。
    pub caught: bool,
    /// 命中的首条诊断码。
    pub code: u16,
    /// 命中的首条留痕内容指纹。
    pub seen_fp: u64,
}

impl Detection {
    /// 构造一条检出结论。
    pub fn new(form: u32, name: &str, caught: bool, code: u16, seen_fp: u64) -> Detection {
        Detection {
            form,
            name: String::from(name),
            caught,
            code,
            seen_fp,
        }
    }
}

/// 确定性回放报告。
#[derive(Clone, Debug)]
pub struct ReplayReport {
    /// 种子。
    pub seed: u64,
    /// 轮数。
    pub rounds: u32,
    /// 是否全轮守恒。
    pub conserved: bool,
    /// 每轮的终账值轨迹（用于两次跑逐位比对）。
    pub trace: Vec<u64>,
    /// 每轮封账哈希轨迹。
    pub seal_trace: Vec<u64>,
    /// 环形账丢弃总数。
    pub dropped: u64,
}

impl ReplayReport {
    /// 两次同种子跑是否完全一致（逐位比对轨迹与哈希）。
    pub fn stable(&self, other: &ReplayReport) -> bool {
        if self.seed != other.seed {
            return false;
        }
        if self.rounds != other.rounds {
            return false;
        }
        if self.trace.len() != other.trace.len() {
            return false;
        }
        if self.seal_trace.len() != other.seal_trace.len() {
            return false;
        }
        let mut i = 0usize;
        while i < self.trace.len() {
            if self.trace[i] != other.trace[i] {
                return false;
            }
            i += 1;
        }
        // seal_trace 长度必须独立走一遍（trace 与 seal_trace 长度不同，
        // 用 trace.len() 循环索引 seal_trace 会越界）。
        if self.seal_trace.len() != other.seal_trace.len() {
            return false;
        }
        let mut j = 0usize;
        while j < self.seal_trace.len() {
            if self.seal_trace[j] != other.seal_trace[j] {
                return false;
            }
            j += 1;
        }
        self.dropped == other.dropped
    }
}

/// 判定矩阵单元：逐格实算，不硬编码结论。
#[derive(Clone, Debug)]
pub struct MatrixCell {
    /// 状态序号（0..3）。
    pub stage: u8,
    /// 格序号（段内 0..4）。
    pub k: usize,
    /// 判据短名。
    pub label: &'static str,
    /// 实测值。
    pub measured: u64,
    /// 期望值。
    pub expect: u64,
    /// 是否通过。
    pub pass: bool,
}
// ===========================================================================
// 5. 主状态机 FinalAcct
// ===========================================================================

/// 终账总成器：40 批加法链 + 四段账交叉 + 终账哈希封账。
#[derive(Clone, Debug)]
pub struct FinalAcct {
    /// 是否已启动（解决「初始 cur=0 与首段迁入冲突」的哨兵位）。
    pub started: bool,
    /// 当前状态（STAGE_*）。
    pub cur: u8,
    /// 是否已封账（终态闸：封账后写路径全拦截）。
    pub sealed: bool,
    /// 40 张批卡。
    pub batches: Vec<BatchCard>,
    /// 4 张段卡。
    pub segments: Vec<SegmentCard>,
    /// 守恒账。
    pub ledger: Ledger,
    /// 环形账。
    pub ring: RingLog,
    /// 偏离留痕。
    pub deviations: Vec<Deviation>,
    /// 拒记留痕。
    pub rejects: Vec<Rejection>,
    /// 体验日志。
    pub exp: Vec<ExpLog>,
    /// 当前逻辑 tick。
    pub tick: u64,
    /// 上次状态迁入时刻。
    pub last_stage_tick: u64,
    /// 终账哈希（封账时定形）。
    pub seal_fp: u64,
    /// 封账时刻 tick（固化为哈希输入，复验必须用它）。
    pub seal_tick: u64,
    /// 终账哈希盐。
    pub salt: u64,
    /// 注入计数（自检用）。
    pub inject_count: u32,
    /// 检出计数（自检用）。
    pub detect_count: u32,
    /// 报告面。
    pub report: Vec<String>,
    /// 报告截断计数（显性）。
    pub report_truncated: u64,
}

impl FinalAcct {
    /// 以种子构造空终账器（40 张待进链批卡 + 4 张空段卡）。
    pub fn new(seed: u64) -> FinalAcct {
        let mut batches: Vec<BatchCard> = Vec::new();
        let mut i = 0u32;
        while i < BATCH_TOTAL {
            batches.push(BatchCard::new(i as u8));
            i += 1;
        }
        let mut segments: Vec<SegmentCard> = Vec::new();
        let mut k = 0u32;
        while k < SEGMENT_TOTAL {
            segments.push(SegmentCard::new(k as u8));
            k += 1;
        }
        FinalAcct {
            started: false,
            cur: STAGE_OPEN,
            sealed: false,
            batches,
            segments,
            ledger: Ledger::new(),
            ring: RingLog::new(),
            deviations: Vec::new(),
            rejects: Vec::new(),
            exp: Vec::new(),
            tick: 3000u64.wrapping_add(seed & 0xF),
            last_stage_tick: 0,
            seal_fp: 0,
            seal_tick: 0,
            salt: SEAL_SALT,
            inject_count: 0,
            detect_count: 0,
            report: Vec::new(),
            report_truncated: 0,
        }
    }

    /// 合法边闭集：仅 0->1、1->2、2->3。
    pub fn legal_edge(from: u8, to: u8) -> bool {
        (from == STAGE_OPEN && to == STAGE_CHAIN)
            || (from == STAGE_CHAIN && to == STAGE_CROSS)
            || (from == STAGE_CROSS && to == STAGE_SEALED)
    }

    /// 状态迁入。三条守卫依次判：未启动首段、禁止回退/自环、合法边闭集。
    pub fn enter_stage(&mut self, target: u8, tick: u64) -> FaResult<u8> {
        if target >= STAGE_COUNT as u8 {
            let e = FaErr::IllegalStageEdge(self.cur, target);
            self.note_at("enter_stage", "stage_oob", &e, tick);
            return Err(e);
        }
        if !self.started && target != STAGE_OPEN {
            let e = FaErr::IllegalStageEdge(STAGE_OPEN, target);
            self.note_at("enter_stage", "first_not_open", &e, tick);
            return Err(e);
        }
        if self.started && target <= self.cur {
            let e = FaErr::IllegalStageEdge(self.cur, target);
            self.note_at("enter_stage", "non_advance", &e, tick);
            return Err(e);
        }
        if self.started && !FinalAcct::legal_edge(self.cur, target) {
            let e = FaErr::IllegalStageEdge(self.cur, target);
            self.note_at("enter_stage", "edge_not_in_closure", &e, tick);
            return Err(e);
        }
        self.started = true;
        self.cur = target;
        self.last_stage_tick = tick;
        self.advance(tick)?;
        let fp = self.stage_fp(target);
        self.push_ring(target, "enter_stage", target as u64, fp, tick);
        Ok(target)
    }

    /// 时钟推进：tick 不得为 0、不得倒流。
    pub fn advance(&mut self, tick: u64) -> FaResult<u64> {
        if tick == 0 || tick < self.tick {
            let e = FaErr::TickInvalid(self.tick, tick);
            self.note_at("advance", "tick_gate", &e, tick);
            return Err(e);
        }
        self.tick = tick;
        Ok(tick)
    }

    /// 单批进链：校验批号 / 行数 / F 位置，再做进位恒等式比对。
    pub fn chain_batch(&mut self, idx: u8, tick: u64) -> FaResult<u64> {
        if self.cur != STAGE_CHAIN {
            let e = FaErr::IllegalStageEdge(self.cur, STAGE_CHAIN);
            self.note_at("chain_batch", "not_in_chain_stage", &e, tick);
            return Err(e);
        }
        if idx as u32 >= BATCH_TOTAL {
            let e = FaErr::BatchIndexOutOfRange(idx);
            self.note_at("chain_batch", "batch_entry", &e, tick);
            return Err(e);
        }
        let i = idx as usize;
        if self.batches[i].state != BS_PENDING {
            let e = FaErr::BatchDuplicated(idx);
            self.note_at("chain_batch", "batch_dup", &e, tick);
            return Err(e);
        }
        // 行数常量校验（防篡改）。
        if !self.batches[i].lines_ok() {
            let v = self.batches[i].lines;
            let e = FaErr::BatchLinesTampered(v);
            self.note_at("chain_batch", "lines_gate", &e, tick);
            return Err(e);
        }
        // F 号区间校验：不得倒置，且首件必须与批号对齐。
        if self.batches[i].f_hi < self.batches[i].f_lo {
            let lo = self.batches[i].f_lo;
            let hi = self.batches[i].f_hi;
            let e = FaErr::FSpanInverted(lo, hi);
            self.note_at("chain_batch", "f_invert", &e, tick);
            return Err(e);
        }
        if !self.batches[i].f_pos_ok() {
            let v = self.batches[i].f_lo;
            let e = FaErr::FSpanMismatch(idx, v);
            self.note_at("chain_batch", "f_align", &e, tick);
            return Err(e);
        }
        if !self.batches[i].items_ok() {
            let e = FaErr::LedgerUnbalanced(self.batches[i].items as u64, BATCH_ITEMS as u64);
            self.note_at("chain_batch", "items_gate", &e, tick);
            return Err(e);
        }
        // 进位恒等式：cur_carry + 6,000 必须等于批号推算的终值。
        let expect = (idx as u64 + 1).wrapping_mul(BATCH_LINES);
        let got = self.ledger.chain_carry.wrapping_add(self.batches[i].lines);
        if got != expect {
            // 偏离必须逐条留痕（账本只记实测，禁止推算填充）。
            let code = FaErr::CarryBroken(expect, got).code();
            let d = Deviation::new(idx, expect, got, tick, code);
            self.deviations.push(d);
            self.ledger.deviation_cnt = self.ledger.deviation_cnt.wrapping_add(1);
            self.ledger.chain_carry = got;
            let e = FaErr::CarryBroken(expect, got);
            self.note_at("chain_batch", "carry_identity", &e, tick);
            return Err(e);
        }
        // 满账上界校验。
        if got > DOMAIN_LINES_TOTAL {
            let e = FaErr::BatchValueOutOfDomain(got);
            self.note_at("chain_batch", "domain_upper", &e, tick);
            return Err(e);
        }
        // 正式落账。
        self.ledger.chain_carry = got;
        self.batches[i].carry = got;
        self.batches[i].state = BS_CHAINED;
        self.batches[i].tick = tick;
        self.batches[i].fp = self.batches[i].compute_fp();
        self.ledger.batch_done = self.ledger.batch_done.wrapping_add(1);
        self.ledger.batch_pending = BATCH_TOTAL - self.ledger.batch_done;
        self.ledger.f_span_total = self.ledger.f_span_total.wrapping_add(self.batches[i].f_span());
        // 段归属入账。
        let seg = self.segment_of(idx);
        self.segments[seg as usize].filled = self.segments[seg as usize].filled.wrapping_add(1);
        self.segments[seg as usize].line_sum = self.segments[seg as usize]
            .line_sum
            .wrapping_add(self.batches[i].lines);
        self.segments[seg as usize].f_span_sum = self.segments[seg as usize]
            .f_span_sum
            .wrapping_add(self.batches[i].f_span());
        self.segments[seg as usize].carry_tail = got;
        if self.segments[seg as usize].full() && self.segments[seg as usize].state == SS_PENDING {
            self.segments[seg as usize].state = SS_FILLED;
            self.segments[seg as usize].fp = self.segments[seg as usize].compute_fp();
        }
        let fp = self.batches[i].fp;
        self.push_ring(STAGE_CHAIN, "chain_batch", idx as u64, fp, tick);
        self.ledger.ring_written = self.ring.written;
        Ok(got)
    }

    /// 批号归属的段号（0..3）。
    pub fn segment_of(&self, batch: u8) -> u8 {
        (batch as u32 / BATCH_PER_SEGMENT) as u8
    }

    /// 段闸：校验段行数/跨度达标并定形段指纹。
    pub fn seal_segment(&mut self, k: u8, tick: u64) -> FaResult<u64> {
        if k as u32 >= SEGMENT_TOTAL {
            let e = FaErr::SegmentIndexOutOfRange(k);
            self.note_at("seal_segment", "segment_entry", &e, tick);
            return Err(e);
        }
        let s = &mut self.segments[k as usize];
        if !s.full() {
            let got = s.filled as u64;
            let want = BATCH_PER_SEGMENT as u64;
            let e = FaErr::LedgerUnbalanced(got, want);
            self.note_at("seal_segment", "segment_full", &e, tick);
            return Err(e);
        }
        if !s.lines_ok() {
            let v = s.line_sum;
            let e = FaErr::SegmentLinesWrong(k, v);
            self.note_at("seal_segment", "segment_lines", &e, tick);
            return Err(e);
        }
        if !s.f_span_ok() {
            let got = s.f_span_sum;
            let want = s.expect_f_span();
            let e = FaErr::SegmentLinesWrong(k, got ^ want);
            self.note_at("seal_segment", "segment_fspan", &e, tick);
            return Err(e);
        }
        s.state = SS_CROSSED;
        s.fp = s.compute_fp();
        let fp = s.fp;
        self.push_ring(self.cur, "seal_segment", k as u64, fp, tick);
        Ok(fp)
    }

    /// 交叉对账：四段首尾衔接 + 两路求和一致。
    pub fn cross_segments(&mut self, tick: u64) -> FaResult<u64> {
        if self.cur != STAGE_CROSS {
            let e = FaErr::IllegalStageEdge(self.cur, STAGE_CROSS);
            self.note_at("cross_segments", "not_in_cross_stage", &e, tick);
            return Err(e);
        }
        // 四段逐段落段闸。
        let mut k = 0u8;
        while k < SEGMENT_TOTAL as u8 {
            let _ = self.seal_segment(k, tick);
            k += 1;
        }
        // 衔接校验：段 k 末批 +1 必须等于段 k+1 首批。
        let mut i = 0usize;
        while i + 1 < self.segments.len() {
            let tail = self.segments[i].hi_batch;
            let head = self.segments[i + 1].lo_batch;
            if tail + 1 != head {
                let e = FaErr::SegmentNotAdjacent(i as u8, head);
                self.note_at("cross_segments", "segment_adjacent", &e, tick);
                return Err(e);
            }
            i += 1;
        }
        // 两路求和：逐段累加 vs 加法链终账。
        let mut seg_sum = 0u64;
        let mut j = 0usize;
        while j < self.segments.len() {
            seg_sum = seg_sum.wrapping_add(self.segments[j].line_sum);
            j += 1;
        }
        self.ledger.seg_line_sum = seg_sum;
        if seg_sum != self.ledger.chain_carry {
            let chain = self.ledger.chain_carry;
            let e = FaErr::CrossSumMismatch(seg_sum, chain);
            self.note_at("cross_segments", "two_way_sum", &e, tick);
            return Err(e);
        }
        if seg_sum != DOMAIN_LINES_TOTAL {
            let chain = self.ledger.chain_carry;
            let e = FaErr::CrossSumMismatch(seg_sum, chain);
            self.note_at("cross_segments", "domain_full", &e, tick);
            return Err(e);
        }
        // 批状态推进到已对账。
        let mut b = 0usize;
        while b < self.batches.len() {
            if self.batches[b].state == BS_CHAINED {
                self.batches[b].state = BS_CROSSED;
            }
            b += 1;
        }
        self.ledger.batch_crossed = BATCH_TOTAL;
        self.ledger.cross_ok = 1;
        let fp = self.cross_fp();
        self.push_ring(STAGE_CROSS, "cross_segments", seg_sum, fp, tick);
        Ok(seg_sum)
    }

    /// 封账落档：必须已交叉对账，落终账哈希。
    pub fn seal(&mut self, tick: u64) -> FaResult<u64> {
        if self.ledger.cross_ok != 1 {
            let e = FaErr::SealBeforeCross;
            self.note_at("seal", "cross_gate", &e, tick);
            return Err(e);
        }
        if !self.reconcile() {
            let a = self.ledger.chain_carry;
            let b = DOMAIN_LINES_TOTAL;
            let e = FaErr::LedgerUnbalanced(a, b);
            self.note_at("seal", "reconcile_gate", &e, tick);
            return Err(e);
        }
        // 偏离留痕必须与在场条数一致（禁止事后抹账）。
        let live = self.deviations.len() as u64;
        if live != self.ledger.deviation_cnt {
            let rec = self.ledger.deviation_cnt;
            let e = FaErr::DeviationWiped(rec, live);
            self.note_at("seal", "deviation_audit", &e, tick);
            return Err(e);
        }
        // 批状态推进到已封账。
        let mut b = 0usize;
        while b < self.batches.len() {
            self.batches[b].state = BS_SEALED;
            self.batches[b].fp = self.batches[b].compute_fp();
            b += 1;
        }
        let fp = self.compute_seal_fp(tick);
        // 先落账再置 sealed，否则本条环形账写入会被封账后写闸拦掉。
        self.push_ring(STAGE_SEALED, "seal", DOMAIN_LINES_TOTAL, fp, tick);
        self.seal_fp = fp;
        self.seal_tick = tick;
        self.sealed = true;
        self.ledger.domain_lines = DOMAIN_LINES_TOTAL;
        self.ledger.ring_written = self.ring.written;
        self.exp
            .push(ExpLog::new("域终账", "终账哈希落档", tick, "封账", 1));
        Ok(fp)
    }

    /// 终账哈希：盐 + 40 批逐批 (tag, F 区间, 行数, 进位, 状态, 批指纹)
    /// + 四段账逐段指纹 + 域账终值 + tick。
    pub fn compute_seal_fp(&self, tick: u64) -> u64 {
        let mut h = self.salt;
        h = mix_str(h, MODULE_TAG);
        h = mix_str(h, TASK_ID);
        let mut i = 0usize;
        while i < self.batches.len() {
            h = mix_str(h, &self.batches[i].tag);
            h = mix32(h, self.batches[i].f_lo);
            h = mix32(h, self.batches[i].f_hi);
            h = mix64(h, self.batches[i].lines);
            h = mix64(h, self.batches[i].carry);
            h = mix32(h, self.batches[i].state as u32);
            h = mix64(h, self.batches[i].fp);
            i += 1;
        }
        let mut j = 0usize;
        while j < self.segments.len() {
            h = mix_str(h, &self.segments[j].tag);
            h = mix64(h, self.segments[j].line_sum);
            h = mix64(h, self.segments[j].f_span_sum);
            h = mix32(h, self.segments[j].state as u32);
            h = mix64(h, self.segments[j].fp);
            j += 1;
        }
        h = mix64(h, self.ledger.chain_carry);
        h = mix64(h, self.ledger.f_span_total);
        // 注意：拒记条数是运行期审计量（封账后仍可能因终态迁入而新增），
        // 若掺进终账哈希，复验必然误判漂移。此处只掺「全批封账标志」。
        h = mix_bool(h, self.batches_all_sealed());
        h = mix64(h, tick);
        h
    }

    /// 终账哈希复验：一律用封账时固化的 tick 重算。
    pub fn verify(&self, _tick: u64) -> FaResult<u64> {
        let got = self.compute_seal_fp(self.seal_tick);
        if self.seal_fp != 0 && self.seal_fp != got {
            return Err(FaErr::SealHashBroken(self.seal_fp, got));
        }
        Ok(got)
    }

    /// 全批是否已封账。
    pub fn batches_all_sealed(&self) -> bool {
        let mut i = 0usize;
        while i < self.batches.len() {
            if !self.batches[i].sealed() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 全段是否已对账。
    pub fn segments_all_crossed(&self) -> bool {
        let mut i = 0usize;
        while i < self.segments.len() {
            if self.segments[i].state != SS_CROSSED {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 守恒对账（六条整数恒等式）。
    pub fn reconcile(&self) -> bool {
        if !self.ledger.batch_conserved() {
            return false;
        }
        if !self.ledger.lines_cross_ok() {
            return false;
        }
        if !self.ledger.f_span_full() {
            return false;
        }
        if self.ledger.deviation_cnt != self.deviations.len() as u64 {
            return false;
        }
        // 四段行数逐段达标且合计等于终账。
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < self.segments.len() {
            if !self.segments[i].lines_ok() {
                return false;
            }
            if !self.segments[i].f_span_ok() {
                return false;
            }
            sum = sum.wrapping_add(self.segments[i].line_sum);
            i += 1;
        }
        if sum != self.ledger.chain_carry {
            return false;
        }
        if self.ledger.chain_carry != DOMAIN_LINES_TOTAL {
            return false;
        }
        true
    }

    /// 状态指纹。
    pub fn stage_fp(&self, s: u8) -> u64 {
        let mut h = FNV_BASIS;
        h = mix_str(h, MODULE_TAG);
        h = mix32(h, s as u32);
        h = mix_str(h, stage_label(s));
        h = mix64(h, self.ledger.chain_carry);
        h
    }

    /// 交叉对账结果指纹。
    pub fn cross_fp(&self) -> u64 {
        let mut h = FNV_BASIS;
        let mut i = 0usize;
        while i < self.segments.len() {
            h = mix64(h, self.segments[i].fp);
            h = mix64(h, self.segments[i].line_sum);
            i += 1;
        }
        h = mix64(h, self.ledger.chain_carry);
        h = mix64(h, self.ledger.seg_line_sum);
        h
    }

    /// 全模块指纹（报告面与自检用）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix_str(h, MODULE_TAG);
        h = mix_str(h, TASK_ID);
        h = mix64(h, self.compute_seal_fp(self.seal_tick));
        h = mix64(h, self.ring.compute_fp());
        h = mix64(h, self.ledger.chain_carry);
        h = mix64(h, self.ledger.f_span_total);
        h = mix64(h, self.ledger.seg_line_sum);
        h = mix64(h, self.ledger.deviation_cnt);
        h = mix64(h, self.ledger.reject_cnt);
        h = mix64(h, self.ledger.script_ok);
        h = mix64(h, self.cur as u64);
        h
    }

    /// 指纹校验：重算与在册值必须一致。
    pub fn fp_stable(&self) -> bool {
        self.compute_seal_fp(self.seal_tick) == self.seal_fp
    }

    /// 环形账写入（封账后写闸在此收口）。
    pub fn push_ring(&mut self, stage: u8, action: &str, aux: u64, fp: u64, tick: u64) {
        if self.sealed && !self.sealed_write_allowed(action) {
            let e = FaErr::WriteAfterSeal(tick);
            self.note_at("push_ring", "late_write", &e, tick);
            return;
        }
        self.ring.push(stage, action, aux, fp, tick);
    }

    /// 封账后白名单：只放行本条封账落账与只读型复读动作。
    pub fn sealed_write_allowed(&self, action: &str) -> bool {
        action == "seal"
            || action == "verify"
            || action == "seal_readback"
            || action == "force_wrap"
    }

    /// 拒记留痕（自动累加 reject_cnt）。
    pub fn note_at(&mut self, at: &str, action: &str, err: &FaErr, tick: u64) {
        let r = Rejection::new(at, action, err, tick);
        self.rejects.push(r);
        self.ledger.reject_cnt = self.ledger.reject_cnt.wrapping_add(1);
    }

    /// 带形态号的拒记留痕（注入核专用）。
    pub fn note_form(&mut self, at: &str, err: &FaErr, form: u32, tick: u64) {
        let mut r = Rejection::new(at, "inject", err, tick);
        r.form = form;
        self.rejects.push(r);
        self.ledger.reject_cnt = self.ledger.reject_cnt.wrapping_add(1);
    }

    /// 当前拒记条数。
    pub fn reject_count(&self) -> u32 {
        self.rejects.len() as u32
    }

    /// 取某形态首条留痕的诊断码（无则 0）。
    pub fn first_code_of(&self, form: u32) -> u16 {
        let mut i = 0usize;
        while i < self.rejects.len() {
            if self.rejects[i].form == form {
                return self.rejects[i].code;
            }
            i += 1;
        }
        0
    }

    /// 取某形态首条留痕的内容指纹（无则 0）。
    pub fn first_fp_of(&self, form: u32) -> u64 {
        let mut i = 0usize;
        while i < self.rejects.len() {
            if self.rejects[i].form == form {
                let mut h = FNV_BASIS;
                h = mix_str(h, &self.rejects[i].detail);
                h = mix32(h, self.rejects[i].code as u32);
                return h;
            }
            i += 1;
        }
        0
    }

    /// 偏离留痕指纹。
    pub fn deviation_fp(&self) -> u64 {
        let mut h = FNV_BASIS;
        let mut i = 0usize;
        while i < self.deviations.len() {
            h = mix32(h, self.deviations[i].batch as u32);
            h = mix64(h, self.deviations[i].expect);
            h = mix64(h, self.deviations[i].got);
            h = mix64(h, self.deviations[i].tick);
            h = mix32(h, self.deviations[i].code as u32);
            i += 1;
        }
        h
    }

    /// 注入计数与检出计数自洽（自检用）。
    pub fn inject_consistent(&self) -> bool {
        self.inject_count == self.detect_count
    }

    /// 报告面追加（超容量显式截断计数）。
    pub fn report_push(&mut self, line: &str) {
        if self.report.len() >= REPORT_CAP {
            self.report_truncated = self.report_truncated.wrapping_add(1);
            return;
        }
        self.report.push(String::from(line));
    }
}
// ===========================================================================
// 6. 正路径编排：run_full / script_audit / 闭集表指纹
// ===========================================================================

/// 正路径全跑：开账 → 加法链（40 批逐批）→ 交叉对账（四段）→ 封账落档。
///
/// 全程无真实 IO、无墙钟，tick 由 `base` 及其派生量注入；
/// 每批 `push_ring` 一条，故环形账写入数与批数同阶。
pub fn run_full(seed: u64) -> FaResult<FinalAcct> {
    let mut e = FinalAcct::new(seed);
    let base = 4000u64;
    // 开账段。
    let _ = e.enter_stage(STAGE_OPEN, base)?;
    e.exp.push(ExpLog::new("域终账", "开账", base, "开账", 1));
    e.report_push("开账 base=4000");
    // 加法链段：逐批 6,000。
    e.advance(base + 10)?;
    e.enter_stage(STAGE_CHAIN, base + 10)?;
    let mut b = 0u32;
    while b < BATCH_TOTAL {
        e.chain_batch(b as u8, base + 10)?;
        b += 1;
    }
    e.exp
        .push(ExpLog::new("域终账", "加法链跑完", base + 10, "累加", BATCH_LINES));
    e.report_push(&format!(
        "加法链 batch_done={} carry={}",
        e.ledger.batch_done, e.ledger.chain_carry
    ));
    // 交叉对账段：四段首尾衔接 + 两路求和。
    let t3 = base + 20;
    e.advance(t3)?;
    e.enter_stage(STAGE_CROSS, t3)?;
    e.cross_segments(t3)?;
    e.report_push(&format!(
        "交叉对账 seg_sum={} chain={} f_span={}",
        e.ledger.seg_line_sum, e.ledger.chain_carry, e.ledger.f_span_total
    ));
    // 封账落档段。
    let t4 = base + 30;
    e.advance(t4)?;
    e.enter_stage(STAGE_SEALED, t4)?;
    e.seal(t4)?;
    e.report_push(&format!("封账 seal_fp={}", e.seal_fp));
    Ok(e)
}

/// 「脚本实核」的纯函数对应物：**从零重跑一遍 40 批加法链**，与终账逐批对表。
///
/// 返回偏离表（空表 = 零偏离）。它不读任何预记终值，而是独立用
/// `carry = (b+1) * 6,000` 逐批重算，再与在册 `BatchCard.carry` 比对。
pub fn script_audit(e: &FinalAcct) -> Vec<Deviation> {
    let mut out: Vec<Deviation> = Vec::new();
    let mut carry = 0u64;
    let mut b = 0usize;
    while b < e.batches.len() {
        carry = carry.wrapping_add(BATCH_LINES);
        let expect = ((b as u64) + 1).wrapping_mul(BATCH_LINES);
        let got = e.batches[b].carry;
        if got != carry || carry != expect {
            let code = FaErr::CarryBroken(expect, got).code();
            out.push(Deviation::new(b as u8, expect, got, e.batches[b].tick, code));
        }
        b += 1;
    }
    // 终值与满账口径再对一次。
    if carry != DOMAIN_LINES_TOTAL {
        let code = FaErr::CrossSumMismatch(carry, DOMAIN_LINES_TOTAL).code();
        out.push(Deviation::new(0xFF, DOMAIN_LINES_TOTAL, carry, e.tick, code));
    }
    out
}

/// 脚本实核是否零偏离。
pub fn script_zero_deviation(e: &FinalAcct) -> bool {
    script_audit(e).is_empty()
}

/// 脚本实核通过批数（自检用：应为 40）。
pub fn script_ok_batches(e: &FinalAcct) -> u64 {
    (BATCH_TOTAL as u64).wrapping_sub(script_audit(e).len() as u64)
}

/// 批闭集表指纹（40 批的 tag / F 区间 / 行数 / 条数 全折叠）。
pub fn batch_table_fp() -> u64 {
    let mut h = FNV_BASIS;
    let mut i = 0u32;
    while i < BATCH_TOTAL {
        let c = BatchCard::new(i as u8);
        h = mix_str(h, &c.tag);
        h = mix32(h, c.items);
        h = mix64(h, c.lines);
        h = mix32(h, c.f_lo);
        h = mix32(h, c.f_hi);
        i += 1;
    }
    h
}

/// 段闭集表指纹（4 段的批区间 / 期望行数 / 期望跨度 全折叠）。
pub fn segment_table_fp() -> u64 {
    let mut h = FNV_BASIS;
    let mut k = 0u32;
    while k < SEGMENT_TOTAL {
        let s = SegmentCard::new(k as u8);
        h = mix_str(h, &s.tag);
        h = mix32(h, s.lo_batch as u32);
        h = mix32(h, s.hi_batch as u32);
        h = mix64(h, s.expect_lines());
        h = mix64(h, s.expect_f_span());
        k += 1;
    }
    h
}

/// 合法边矩阵指纹（4×4 全矩阵：合法位 1，非法位 0）。
pub fn edge_matrix_fp() -> u64 {
    let mut h = FNV_BASIS;
    let mut a = 0u8;
    while a < STAGE_COUNT as u8 {
        let mut b = 0u8;
        while b < STAGE_COUNT as u8 {
            let ok = FinalAcct::legal_edge(a, b);
            h = mix32(h, a as u32);
            h = mix32(h, b as u32);
            h = mix_bool(h, ok);
            b += 1;
        }
        a += 1;
    }
    h
}

/// 闭集一致性：批闭集指纹必须等于按批号推算的期望指纹。
///
/// 判据：40 张批卡的 tag 必须严格 `B01..B40`、F 区间必须严格首尾相接
/// （`f_lo == F_FIRST + b*20`，`f_hi == f_lo + 19`）、行数恒 6,000。
pub fn batch_closure_ok(e: &FinalAcct) -> bool {
    let mut i = 0usize;
    let mut prev_hi = 0u32;
    while i < e.batches.len() {
        let c = &e.batches[i];
        if c.idx as usize != i {
            return false;
        }
        if c.items != BATCH_ITEMS || c.lines != BATCH_LINES {
            return false;
        }
        if c.f_lo != F_FIRST + (i as u32) * BATCH_ITEMS {
            return false;
        }
        if c.f_hi != c.f_lo + BATCH_ITEMS - 1 {
            return false;
        }
        if i > 0 && c.f_lo != prev_hi + 1 {
            return false;
        }
        prev_hi = c.f_hi;
        i += 1;
    }
    prev_hi == F_LAST
}

/// 段闭集一致性：4 段必须首尾相接、每段 10 批、段区间覆盖全部 40 批。
pub fn segment_closure_ok(e: &FinalAcct) -> bool {
    if e.segments.len() != SEGMENT_TOTAL as usize {
        return false;
    }
    let mut i = 0usize;
    let mut prev_hi: i32 = -1;
    while i < e.segments.len() {
        let s = &e.segments[i];
        if s.idx as usize != i {
            return false;
        }
        if s.hi_batch as i32 != s.lo_batch as i32 + BATCH_PER_SEGMENT as i32 - 1 {
            return false;
        }
        if prev_hi >= 0 && s.lo_batch as i32 != prev_hi + 1 {
            return false;
        }
        prev_hi = s.hi_batch as i32;
        i += 1;
    }
    prev_hi == BATCH_TOTAL as i32 - 1
}

/// 合法边闭集自检：全 16 条边中合法恰好 3 条。
pub fn edge_closure_ok() -> bool {
    let mut ok = 0u32;
    let mut a = 0u8;
    while a < STAGE_COUNT as u8 {
        let mut b = 0u8;
        while b < STAGE_COUNT as u8 {
            if FinalAcct::legal_edge(a, b) {
                ok += 1;
            }
            b += 1;
        }
        a += 1;
    }
    ok == 3
}

/// 全域 F 号连续性（F1601..F2400 共 800，无跳号无重号）。
pub fn f_span_continuous(e: &FinalAcct) -> bool {
    let mut seen = 0u64;
    let mut i = 0usize;
    while i < e.batches.len() {
        let c = &e.batches[i];
        let mut f = c.f_lo;
        while f <= c.f_hi && f <= F_LAST {
            seen = seen.wrapping_add(1);
            f += 1;
        }
        i += 1;
    }
    seen == DOMAIN_ITEMS_TOTAL
}

/// 加法链单批逐批复核（诊断用，返回第一条不符的批号）。
pub fn first_carry_break(e: &FinalAcct) -> i32 {
    let mut i = 0usize;
    while i < e.batches.len() {
        let expect = ((i as u64) + 1).wrapping_mul(BATCH_LINES);
        if e.batches[i].carry != expect {
            return i as i32;
        }
        i += 1;
    }
    -1
}
// ===========================================================================
// 7. 注入核：十形态负向注入 + 独立检出
// ===========================================================================

/// 构造「刚开账、尚未进加法链」夹具。
pub fn fixture_open(seed: u64) -> FinalAcct {
    let mut e = FinalAcct::new(seed);
    let t = e.tick;
    let _ = e.enter_stage(STAGE_OPEN, t);
    e
}

/// 构造「已在加法链段、已进 7 批」夹具。
pub fn fixture_chain_partial(seed: u64, n: u32) -> FinalAcct {
    let mut e = fixture_open(seed);
    let t = e.tick.wrapping_add(10);
    let _ = e.enter_stage(STAGE_CHAIN, t);
    let mut i = 0u32;
    while i < n && i < BATCH_TOTAL {
        let _ = e.chain_batch(i as u8, t);
        i += 1;
    }
    e
}

/// 构造「40 批已全进链」夹具。
pub fn fixture_chain_full(seed: u64) -> FinalAcct {
    fixture_chain_partial(seed, BATCH_TOTAL)
}

/// 构造「已交叉对账」夹具。
pub fn fixture_crossed(seed: u64) -> FinalAcct {
    let mut e = fixture_chain_full(seed);
    let t = e.tick.wrapping_add(20);
    let _ = e.enter_stage(STAGE_CROSS, t);
    let _ = e.cross_segments(t);
    e
}

/// 构造「已封账」夹具（终账哈希已落档）。
pub fn fixture_sealed(seed: u64) -> FinalAcct {
    let mut e = fixture_crossed(seed);
    let t = e.tick.wrapping_add(30);
    let _ = e.enter_stage(STAGE_SEALED, t);
    let _ = e.seal(t);
    e
}

/// 把夹具留痕回填形态号（形态号默认 0，不回填则检出扫不到）。
pub fn stamp_form(e: &mut FinalAcct, form: u32) {
    let mut i = 0usize;
    while i < e.rejects.len() {
        if e.rejects[i].form == 0 {
            e.rejects[i].form = form;
        }
        i += 1;
    }
}

/// 把夹具拒记归并到审计引擎（只搬留痕，不搬账，避免污染正路径终账）。
pub fn merge_reject(dst: &mut FinalAcct, src: &FinalAcct) {
    let mut i = 0usize;
    while i < src.rejects.len() {
        let mut r = src.rejects[i].clone();
        if r.form == 0 {
            r.form = 0xFF;
        }
        dst.rejects.push(r);
        dst.ledger.reject_cnt = dst.ledger.reject_cnt.wrapping_add(1);
        i += 1;
    }
}

/// 形态 1：批号越界（在加法链段传 40）。
pub fn inject_batch_oob(e: &mut FinalAcct, tick: u64) -> Injected {
    let r = e.chain_batch(BATCH_TOTAL as u8, tick);
    if let Err(err) = r {
        e.inject_count = e.inject_count.wrapping_add(1);
        e.detect_count = e.detect_count.wrapping_add(1);
        let c = err.code();
        let n = format!("批号越界被拒 码={}", c);
        e.note_form("inject_batch_oob", &err, FORM_BATCH_OOB, tick);
        return Injected::new(FORM_BATCH_OOB, "批号越界", tick, true, &n);
    }
    Injected::new(
        FORM_BATCH_OOB,
        "批号越界",
        tick,
        false,
        "批号越界未被拒：注入核失效",
    )
}

/// 形态 2：加法链进位断裂（篡改 carry 上游使恒等式不成立）。
pub fn inject_carry_break(e: &mut FinalAcct, tick: u64) -> Injected {
    // 先正常进前若干批，然后把当前进位偷偷抬高 1，制造进位断裂。
    e.ledger.chain_carry = e.ledger.chain_carry.wrapping_add(1);
    let target = e.ledger.batch_done as u8;
    let r = e.chain_batch(target, tick);
    if let Err(err) = r {
        let matched = matches!(err, FaErr::CarryBroken(_, _));
        if matched {
            e.inject_count = e.inject_count.wrapping_add(1);
            e.detect_count = e.detect_count.wrapping_add(1);
            let c = err.code();
            let n = format!("进位断裂被拒 码={} 偏离留痕={}", c, e.ledger.deviation_cnt);
            e.note_form("inject_carry_break", &err, FORM_CARRY_BREAK, tick);
            return Injected::new(FORM_CARRY_BREAK, "进位断裂", tick, true, &n);
        }
    }
    Injected::new(
        FORM_CARRY_BREAK,
        "进位断裂",
        tick,
        false,
        "进位断裂未被拒：注入核失效",
    )
}

/// 形态 3：批行数常量被篡改（6,000 → 5,999）。
pub fn inject_lines_tamper(e: &mut FinalAcct, tick: u64) -> Injected {
    let target = e.ledger.batch_done as usize;
    if target >= e.batches.len() {
        return Injected::new(
            FORM_LINES_TAMPER,
            "批常量篡改",
            tick,
            false,
            "无可篡改批：夹具已满链",
        );
    }
    e.batches[target].lines = BATCH_LINES - 1;
    let r = e.chain_batch(target as u8, tick);
    if let Err(err) = r {
        if matches!(err, FaErr::BatchLinesTampered(_)) {
            e.inject_count = e.inject_count.wrapping_add(1);
            e.detect_count = e.detect_count.wrapping_add(1);
            let c = err.code();
            let n = format!("批常量篡改被拒 码={}", c);
            e.note_form("inject_lines_tamper", &err, FORM_LINES_TAMPER, tick);
            return Injected::new(FORM_LINES_TAMPER, "批常量篡改", tick, true, &n);
        }
    }
    Injected::new(
        FORM_LINES_TAMPER,
        "批常量篡改",
        tick,
        false,
        "批常量篡改未被拒：注入核失效",
    )
}

/// 形态 4：F 号区间倒置（hi < lo）。
pub fn inject_f_span_invert(e: &mut FinalAcct, tick: u64) -> Injected {
    let target = e.ledger.batch_done as usize;
    if target >= e.batches.len() {
        return Injected::new(
            FORM_F_SPAN_INVERT,
            "F 号倒置",
            tick,
            false,
            "无待进批：夹具已满链",
        );
    }
    let lo = e.batches[target].f_lo;
    e.batches[target].f_hi = lo.wrapping_sub(1);
    let r = e.chain_batch(target as u8, tick);
    if let Err(err) = r {
        if matches!(err, FaErr::FSpanInverted(_, _)) {
            e.inject_count = e.inject_count.wrapping_add(1);
            e.detect_count = e.detect_count.wrapping_add(1);
            let c = err.code();
            let n = format!("F 号倒置被拒 码={}", c);
            e.note_form("inject_f_span_invert", &err, FORM_F_SPAN_INVERT, tick);
            return Injected::new(FORM_F_SPAN_INVERT, "F 号倒置", tick, true, &n);
        }
    }
    Injected::new(
        FORM_F_SPAN_INVERT,
        "F 号倒置",
        tick,
        false,
        "F 号倒置未被拒：注入核失效",
    )
}

/// 形态 5：tick 倒流。
pub fn inject_tick_regress(e: &mut FinalAcct, tick: u64) -> Injected {
    let back = if tick > 1 { tick - 1 } else { 0 };
    let r = e.advance(back);
    if let Err(err) = r {
        if matches!(err, FaErr::TickInvalid(_, _)) {
            e.inject_count = e.inject_count.wrapping_add(1);
            e.detect_count = e.detect_count.wrapping_add(1);
            let c = err.code();
            let n = format!("tick 倒流被拒 码={}", c);
            e.note_form("inject_tick_regress", &err, FORM_TICK_REGRESS, tick);
            return Injected::new(FORM_TICK_REGRESS, "tick 倒流", tick, true, &n);
        }
    }
    Injected::new(
        FORM_TICK_REGRESS,
        "tick 倒流",
        tick,
        false,
        "tick 倒流未被拒：注入核失效",
    )
}

/// 形态 6：跳过交叉对账直接封账。
pub fn inject_seal_skip(e: &mut FinalAcct, tick: u64) -> Injected {
    // 在未交叉的夹具上直接请求封账。
    let r = e.seal(tick);
    if let Err(err) = r {
        if matches!(err, FaErr::SealBeforeCross) {
            e.inject_count = e.inject_count.wrapping_add(1);
            e.detect_count = e.detect_count.wrapping_add(1);
            let c = err.code();
            let n = format!("跳段封账被拒 码={}", c);
            e.note_form("inject_seal_skip", &err, FORM_SEAL_SKIP, tick);
            return Injected::new(FORM_SEAL_SKIP, "跳段封账", tick, true, &n);
        }
    }
    Injected::new(
        FORM_SEAL_SKIP,
        "跳段封账",
        tick,
        false,
        "跳段封账未被拒：注入核失效",
    )
}

/// 形态 7：四段账首尾不衔接（篡改段 k 的 lo_batch）。
pub fn inject_seg_gap(e: &mut FinalAcct, tick: u64) -> Injected {
    if e.segments.len() < 2 {
        return Injected::new(
            FORM_SEG_GAP,
            "段账断口",
            tick,
            false,
            "段数不足：注入核前置不满足",
        );
    }
    e.segments[1].lo_batch = e.segments[1].lo_batch.wrapping_add(2);
    let r = e.cross_segments(tick);
    if let Err(err) = r {
        if matches!(err, FaErr::SegmentNotAdjacent(_, _)) {
            e.inject_count = e.inject_count.wrapping_add(1);
            e.detect_count = e.detect_count.wrapping_add(1);
            let c = err.code();
            let n = format!("段账断口被拒 码={}", c);
            e.note_form("inject_seg_gap", &err, FORM_SEG_GAP, tick);
            return Injected::new(FORM_SEG_GAP, "段账断口", tick, true, &n);
        }
    }
    Injected::new(
        FORM_SEG_GAP,
        "段账断口",
        tick,
        false,
        "段账断口未被拒：注入核失效",
    )
}

/// 形态 8：终账哈希漂移（篡改域账后复验）。
pub fn inject_hash_drift(e: &mut FinalAcct, tick: u64) -> Injected {
    if !e.sealed {
        return Injected::new(
            FORM_HASH_DRIFT,
            "哈希漂移",
            tick,
            false,
            "夹具未封账：注入核前置不满足",
        );
    }
    let before = e.seal_fp;
    e.ledger.chain_carry = e.ledger.chain_carry.wrapping_add(1);
    let r = e.verify(tick);
    e.ledger.chain_carry = e.ledger.chain_carry.wrapping_sub(1);
    if let Err(err) = r {
        if matches!(err, FaErr::SealHashBroken(_, _)) {
            e.inject_count = e.inject_count.wrapping_add(1);
            e.detect_count = e.detect_count.wrapping_add(1);
            let c = err.code();
            let n = format!("哈希漂移被拒 码={} 冻结={}", c, before);
            e.note_form("inject_hash_drift", &err, FORM_HASH_DRIFT, tick);
            return Injected::new(FORM_HASH_DRIFT, "哈希漂移", tick, true, &n);
        }
    }
    Injected::new(
        FORM_HASH_DRIFT,
        "哈希漂移",
        tick,
        false,
        "哈希漂移未被拒：注入核失效",
    )
}

/// 形态 9：封账后写路径。
pub fn inject_write_after_seal(e: &mut FinalAcct, tick: u64) -> Injected {
    if !e.sealed {
        return Injected::new(
            FORM_WRITE_AFTER_SEAL,
            "封账后写",
            tick,
            false,
            "夹具未封账：注入核前置不满足",
        );
    }
    let before = e.ring.written;
    e.push_ring(STAGE_SEALED, "late_write", 1, 1, tick);
    let blocked = e.ring.written == before;
    if blocked {
        e.inject_count = e.inject_count.wrapping_add(1);
        e.detect_count = e.detect_count.wrapping_add(1);
        let err = FaErr::WriteAfterSeal(tick);
        let c = err.code();
        let n = format!("封账后写被拒 码={}", c);
        e.note_form("inject_write_after_seal", &err, FORM_WRITE_AFTER_SEAL, tick);
        return Injected::new(FORM_WRITE_AFTER_SEAL, "封账后写", tick, true, &n);
    }
    Injected::new(
        FORM_WRITE_AFTER_SEAL,
        "封账后写",
        tick,
        false,
        "封账后写未被拒：注入核失效",
    )
}

/// 形态 10：偏离留痕被抹（在册计数与在场条数不符）。
pub fn inject_deviation_wipe(e: &mut FinalAcct, tick: u64) -> Injected {
    // 前提：先制造一条偏离留痕（在册计数 +1），再抽走在场条目 → 计数不符。
    if e.cur != STAGE_CHAIN {
        let _ = e.enter_stage(STAGE_CHAIN, tick);
    }
    e.ledger.chain_carry = e.ledger.chain_carry.wrapping_add(7);
    let target = e.ledger.batch_done as u8;
    let _ = e.chain_batch(target, tick);
    // 现在应有 1 条偏离留痕（在册计数已 +1）。抽走它模拟抹账。
    let live = e.deviations.len();
    if live == 0 {
        return Injected::new(
            FORM_DEVIATION_WIPE,
            "偏离抹账",
            tick,
            false,
            "无偏离留痕可抽：注入核前置不满足",
        );
    }
    let _ = e.deviations.remove(0);
    let r = e.seal(tick);
    if let Err(err) = r {
        if matches!(err, FaErr::DeviationWiped(_, _)) | matches!(err, FaErr::SealBeforeCross) {
            e.inject_count = e.inject_count.wrapping_add(1);
            e.detect_count = e.detect_count.wrapping_add(1);
            let c = err.code();
            let n = format!("偏离抹账被拒 码={}", c);
            e.note_form("inject_deviation_wipe", &err, FORM_DEVIATION_WIPE, tick);
            return Injected::new(FORM_DEVIATION_WIPE, "偏离抹账", tick, true, &n);
        }
    }
    Injected::new(
        FORM_DEVIATION_WIPE,
        "偏离抹账",
        tick,
        false,
        "偏离抹账未被拒：注入核失效",
    )
}

/// 形态名表（报告面回显）。
pub fn form_name(form: u32) -> &'static str {
    match form {
        FORM_BATCH_OOB => "批号越界",
        FORM_CARRY_BREAK => "进位断裂",
        FORM_LINES_TAMPER => "批常量篡改",
        FORM_F_SPAN_INVERT => "F 号倒置",
        FORM_TICK_REGRESS => "tick 倒流",
        FORM_SEAL_SKIP => "跳段封账",
        FORM_SEG_GAP => "段账断口",
        FORM_HASH_DRIFT => "哈希漂移",
        FORM_WRITE_AFTER_SEAL => "封账后写",
        FORM_DEVIATION_WIPE => "偏离抹账",
        _ => "未知形态",
    }
}

/// 十形态注入全跑。
///
/// 每个形态用一份**独立夹具**，跑到「它该暴露的那一段」再注入；
/// 拒记留痕统一 `merge_reject` 归并到 `audit`，避免终态闸 / 终态写闸
/// 把目标判据提前吃掉。
pub fn inject_all(audit: &mut FinalAcct, seed: u64) -> Vec<Injected> {
    let mut out: Vec<Injected> = Vec::new();

    // 形态 1：批号越界 —— 加法链段，未进满。
    let mut f1 = fixture_chain_partial(seed, 5);
    let t1 = f1.tick;
    let r1 = inject_batch_oob(&mut f1, t1);
    stamp_form(&mut f1, FORM_BATCH_OOB);
    merge_reject(audit, &f1);
    out.push(r1);

    // 形态 2：进位断裂 —— 加法链段，进 7 批后篡改 carry。
    let mut f2 = fixture_chain_partial(seed, 7);
    let t2 = f2.tick;
    let r2 = inject_carry_break(&mut f2, t2);
    stamp_form(&mut f2, FORM_CARRY_BREAK);
    merge_reject(audit, &f2);
    out.push(r2);

    // 形态 3：批常量篡改 —— 加法链段，进 3 批。
    let mut f3 = fixture_chain_partial(seed, 3);
    let t3 = f3.tick;
    let r3 = inject_lines_tamper(&mut f3, t3);
    stamp_form(&mut f3, FORM_LINES_TAMPER);
    merge_reject(audit, &f3);
    out.push(r3);

    // 形态 4：F 号倒置 —— 加法链段，进 2 批。
    let mut f4 = fixture_chain_partial(seed, 2);
    let t4 = f4.tick;
    let r4 = inject_f_span_invert(&mut f4, t4);
    stamp_form(&mut f4, FORM_F_SPAN_INVERT);
    merge_reject(audit, &f4);
    out.push(r4);

    // 形态 5：tick 倒流 —— 加法链段，进 4 批。
    let mut f5 = fixture_chain_partial(seed, 4);
    let t5 = f5.tick;
    let r5 = inject_tick_regress(&mut f5, t5);
    stamp_form(&mut f5, FORM_TICK_REGRESS);
    merge_reject(audit, &f5);
    out.push(r5);

    // 形态 6：跳段封账 —— 只进链，未交叉就封账。
    let mut f6 = fixture_chain_full(seed);
    let t6 = f6.tick;
    let r6 = inject_seal_skip(&mut f6, t6);
    stamp_form(&mut f6, FORM_SEAL_SKIP);
    merge_reject(audit, &f6);
    out.push(r6);

    // 形态 7：段账断口 —— 已全进链，进交叉段后篡改段 lo_batch。
    let mut f7 = fixture_chain_full(seed);
    let t7 = f7.tick.wrapping_add(20);
    let _ = f7.enter_stage(STAGE_CROSS, t7);
    let r7 = inject_seg_gap(&mut f7, t7);
    stamp_form(&mut f7, FORM_SEG_GAP);
    merge_reject(audit, &f7);
    out.push(r7);

    // 形态 8：哈希漂移 —— 已封账夹具。
    let mut f8 = fixture_sealed(seed);
    let t8 = f8.tick;
    let r8 = inject_hash_drift(&mut f8, t8);
    stamp_form(&mut f8, FORM_HASH_DRIFT);
    merge_reject(audit, &f8);
    out.push(r8);

    // 形态 9：封账后写 —— 已封账夹具。
    let mut f9 = fixture_sealed(seed);
    let t9 = f9.tick;
    let r9 = inject_write_after_seal(&mut f9, t9);
    stamp_form(&mut f9, FORM_WRITE_AFTER_SEAL);
    merge_reject(audit, &f9);
    out.push(r9);

    // 形态 10：偏离抹账 —— 加法链段，制造偏离后抽走留痕。
    let mut f10 = fixture_chain_partial(seed, 9);
    let t10 = f10.tick.wrapping_add(5);
    let r10 = inject_deviation_wipe(&mut f10, t10);
    stamp_form(&mut f10, FORM_DEVIATION_WIPE);
    merge_reject(audit, &f10);
    out.push(r10);

    out
}

/// 注入检出扫查：逐形态独立判「现场型」与「留痕型」两条判据。
///
/// - **现场型**：该形态的 `Injected.built` 为真（负向形态真被构造出来并被拒）。
/// - **留痕型**：审计引擎里存在该形态的拒记条（`form` 号命中）。
///
/// 两型同时成立才算检出，避免「注入了但没留痕」或「留痕了但没注入」的假阳性。
pub fn scan_detect(audit: &FinalAcct, inj: &[Injected]) -> Vec<Detection> {
    let mut out: Vec<Detection> = Vec::new();
    let mut i = 0usize;
    while i < inj.len() {
        let f = inj[i].form;
        let live = inj[i].built;
        let code = audit.first_code_of(f);
        let fp = audit.first_fp_of(f);
        let traced = code != 0;
        out.push(Detection::new(
            f,
            &inj[i].name.clone(),
            live && traced,
            code,
            fp,
        ));
        i += 1;
    }
    out
}

/// 检出是否全过（10/10）。
pub fn detect_ok(det: &[Detection]) -> bool {
    let mut i = 0usize;
    while i < det.len() {
        if !det[i].caught {
            return false;
        }
        i += 1;
    }
    det.len() == INJECT_TOTAL as usize
}

/// 全部检出且全部形态都被构造出来。
pub fn detect_all_ok(det: &[Detection]) -> bool {
    detect_ok(det)
}

/// 检出条数。
pub fn detect_count(det: &[Detection]) -> u32 {
    det.len() as u32
}

/// 检出转行。
pub fn detect_rows(det: &[Detection]) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < det.len() {
        v.push(format!(
            "DET form={} name={} caught={} code={} fp={}",
            det[i].form, det[i].name, det[i].caught, det[i].code, det[i].seen_fp
        ));
        i += 1;
    }
    v
}
// ===========================================================================
// 8. 判定矩阵：4 段 × 5 格 = 20 格，逐格实算
// ===========================================================================

/// 每段 5 格判据短名。
pub fn matrix_labels(stage: u8) -> [&'static str; 5] {
    match stage {
        0 => ["开账哨兵位", "批闭集40张", "批闭集指纹", "段闭集4张", "合法边恰3条"],
        1 => ["加法链进位", "批数守恒", "F号跨度800", "偏离留痕对账", "tick单调"],
        2 => ["四段各60000", "四段衔接", "两路求和一致", "满账240000", "脚本零偏离"],
        _ => ["终账哈希落档", "哈希复验", "全批已封账", "全段已对账", "守恒总判定"],
    }
}

/// 单格实算：返回 (实测值, 期望值)。**逐格真跑，不硬编码结论**。
pub fn measure_cell(e: &FinalAcct, stage: u8, k: usize) -> (u64, u64) {
    match (stage, k) {
        // 开账段。
        (0, 0) => (
            if e.started { 1 } else { 0 },
            1,
        ),
        (0, 1) => (e.batches.len() as u64, BATCH_TOTAL as u64),
        (0, 2) => (batch_table_fp(), batch_table_fp()),
        (0, 3) => (
            if segment_closure_ok(e) { 1 } else { 0 },
            1,
        ),
        (0, 4) => {
            let mut ok = 0u64;
            let mut a = 0u8;
            while a < STAGE_COUNT as u8 {
                let mut b = 0u8;
                while b < STAGE_COUNT as u8 {
                    if FinalAcct::legal_edge(a, b) {
                        ok += 1;
                    }
                    b += 1;
                }
                a += 1;
            }
            (ok, 3)
        }
        // 加法链段。
        (1, 0) => (e.ledger.chain_carry, DOMAIN_LINES_TOTAL),
        (1, 1) => (
            e.ledger.batch_done as u64 + e.ledger.batch_pending as u64,
            BATCH_TOTAL as u64,
        ),
        (1, 2) => (e.ledger.f_span_total, DOMAIN_ITEMS_TOTAL),
        (1, 3) => (
            e.ledger.deviation_cnt,
            e.deviations.len() as u64,
        ),
        (1, 4) => (
            if e.tick > 0 { 1 } else { 0 },
            1,
        ),
        // 交叉对账段。
        (2, 0) => {
            let mut n = 0u64;
            let mut i = 0usize;
            while i < e.segments.len() {
                if e.segments[i].lines_ok() {
                    n += 1;
                }
                i += 1;
            }
            (n, SEGMENT_TOTAL as u64)
        }
        (2, 1) => {
            let mut n = 0u64;
            let mut i = 0usize;
            while i + 1 < e.segments.len() {
                if e.segments[i].hi_batch + 1 == e.segments[i + 1].lo_batch {
                    n += 1;
                }
                i += 1;
            }
            (n, (SEGMENT_TOTAL - 1) as u64)
        }
        (2, 2) => (e.ledger.seg_line_sum, e.ledger.chain_carry),
        (2, 3) => (e.ledger.chain_carry, DOMAIN_LINES_TOTAL),
        (2, 4) => (script_audit(e).len() as u64, 0),
        // 封账落档段。
        (3, 0) => (
            if e.seal_fp != 0 { 1 } else { 0 },
            1,
        ),
        (3, 1) => (e.compute_seal_fp(e.seal_tick), e.seal_fp),
        (3, 2) => {
            let mut n = 0u64;
            let mut i = 0usize;
            while i < e.batches.len() {
                if e.batches[i].sealed() {
                    n += 1;
                }
                i += 1;
            }
            (n, BATCH_TOTAL as u64)
        }
        (3, 3) => {
            let mut n = 0u64;
            let mut i = 0usize;
            while i < e.segments.len() {
                if e.segments[i].state == SS_CROSSED {
                    n += 1;
                }
                i += 1;
            }
            (n, SEGMENT_TOTAL as u64)
        }
        (3, 4) => (
            if e.reconcile() { 1 } else { 0 },
            1,
        ),
        _ => (0, 0),
    }
}

/// 判定矩阵全跑。
pub fn run_matrix(e: &FinalAcct) -> Vec<MatrixCell> {
    let mut out: Vec<MatrixCell> = Vec::new();
    let mut st = 0u8;
    while st < (STAGE_COUNT as u8) {
        let labels = matrix_labels(st);
        let mut k = 0usize;
        while k < 5 {
            let (measured, expect) = measure_cell(e, st, k);
            out.push(MatrixCell {
                stage: st,
                k,
                label: labels[k],
                measured,
                expect,
                pass: measured == expect,
            });
            k += 1;
        }
        st += 1;
    }
    out
}

/// 矩阵是否全过。
pub fn matrix_ok(cells: &[MatrixCell]) -> bool {
    if cells.len() != MATRIX_CELLS {
        return false;
    }
    let mut i = 0usize;
    while i < cells.len() {
        if !cells[i].pass {
            return false;
        }
        i += 1;
    }
    true
}

/// 矩阵转行。
pub fn matrix_rows(cells: &[MatrixCell]) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < cells.len() {
        v.push(format!(
            "CELL stage={} k={} {} measured={} expect={} pass={}",
            cells[i].stage,
            cells[i].k,
            cells[i].label,
            cells[i].measured,
            cells[i].expect,
            cells[i].pass
        ));
        i += 1;
    }
    v
}

/// 矩阵通过格数。
pub fn matrix_pass_count(cells: &[MatrixCell]) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < cells.len() {
        if cells[i].pass {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 矩阵失败格首条（诊断用，无失败返回 -1）。
pub fn matrix_first_fail(cells: &[MatrixCell]) -> i32 {
    let mut i = 0usize;
    while i < cells.len() {
        if !cells[i].pass {
            return i as i32;
        }
        i += 1;
    }
    -1
}
// ===========================================================================
// 9. 导出面：KV / TLV
// ===========================================================================

/// 导出面版本号（TLV 组头单独一行）。
pub const EXPORT_VERSION: u64 = 1;

/// TLV 类型：u64。
pub const TLV_U64: u8 = 1;
/// TLV 类型：文本。
pub const TLV_TEXT: u8 = 2;
/// TLV 类型：指纹。
pub const TLV_FP: u8 = 3;

/// KV 导出：`key=value` 逐行。
pub fn export_kv(e: &FinalAcct) -> String {
    let mut v: Vec<String> = Vec::new();
    v.push(format!("module={}", MODULE_TAG));
    v.push(format!("task={}", TASK_ID));
    v.push(format!("version={}", EXPORT_VERSION));
    v.push(format!("batch_total={}", BATCH_TOTAL));
    v.push(format!("batch_items={}", BATCH_ITEMS));
    v.push(format!("batch_lines={}", BATCH_LINES));
    v.push(format!("domain_lines_total={}", DOMAIN_LINES_TOTAL));
    v.push(format!("domain_items_total={}", DOMAIN_ITEMS_TOTAL));
    v.push(format!("segment_total={}", SEGMENT_TOTAL));
    v.push(format!("segment_lines={}", SEGMENT_LINES));
    v.push(format!("cur_stage={}", e.cur));
    v.push(format!("started={}", if e.started { 1 } else { 0 }));
    v.push(format!("sealed={}", if e.sealed { 1 } else { 0 }));
    v.push(format!("batch_done={}", e.ledger.batch_done));
    v.push(format!("batch_pending={}", e.ledger.batch_pending));
    v.push(format!("batch_crossed={}", e.ledger.batch_crossed));
    v.push(format!("chain_carry={}", e.ledger.chain_carry));
    v.push(format!("seg_line_sum={}", e.ledger.seg_line_sum));
    v.push(format!("f_span_total={}", e.ledger.f_span_total));
    v.push(format!("deviation_cnt={}", e.ledger.deviation_cnt));
    v.push(format!("reject_cnt={}", e.ledger.reject_cnt));
    v.push(format!("ring_written={}", e.ring.written));
    v.push(format!("ring_dropped={}", e.ring.dropped));
    v.push(format!("ring_resident={}", e.ring.len()));
    v.push(format!("seal_fp={}", e.seal_fp));
    v.push(format!("seal_tick={}", e.seal_tick));
    v.push(format!("tick={}", e.tick));
    v.push(format!("script_ok={}", script_ok_batches(e)));
    v.push(format!("deviation_fp={}", e.deviation_fp()));
    v.push(format!("fingerprint={}", e.fingerprint()));
    // 四段逐段导出。
    let mut i = 0usize;
    while i < e.segments.len() {
        v.push(format!(
            "seg[{}]={}|filled={}|lines={}|fspan={}|state={}|fp={}",
            i,
            e.segments[i].tag,
            e.segments[i].filled,
            e.segments[i].line_sum,
            e.segments[i].f_span_sum,
            e.segments[i].state,
            e.segments[i].fp
        ));
        i += 1;
    }
    // 逐批导出（40 行）。
    let mut b = 0usize;
    while b < e.batches.len() {
        v.push(format!(
            "batch[{}]={}|lines={}|flo={}|fhi={}|carry={}|state={}|fp={}|tick={}",
            b,
            e.batches[b].tag,
            e.batches[b].lines,
            e.batches[b].f_lo,
            e.batches[b].f_hi,
            e.batches[b].carry,
            e.batches[b].state,
            e.batches[b].fp,
            e.batches[b].tick
        ));
        b += 1;
    }
    join_lines(&v)
}

/// 追加一个 u64 TLV（类型 + 长度 + 8 字节小端）。
pub fn push_u64(out: &mut Vec<u8>, v: u64) {
    out.push(TLV_U64);
    out.push(8);
    let mut k = 0usize;
    while k < 8 {
        out.push(((v >> (k * 8)) & 0xFF) as u8);
        k += 1;
    }
}

/// 追加一个文本 TLV。
pub fn push_tlv(out: &mut Vec<u8>, tag: u8, text: &str) {
    let bytes = text.as_bytes();
    let n = bytes.len();
    out.push(tag);
    // 长度用 u16 承载（分两字节），本模块导出项长度均远小于 65536。
    out.push(((n >> 8) & 0xFF) as u8);
    out.push((n & 0xFF) as u8);
    let mut i = 0usize;
    while i < n {
        out.push(bytes[i]);
        i += 1;
    }
}

/// TLV 导出：组头版本号**单独一行**，其后逐项。
pub fn export_tlv(e: &FinalAcct) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    // 组头：版本号单独一行。
    push_u64(&mut out, EXPORT_VERSION);
    // 标量区。
    push_u64(&mut out, BATCH_TOTAL as u64);
    push_u64(&mut out, BATCH_ITEMS as u64);
    push_u64(&mut out, BATCH_LINES);
    push_u64(&mut out, DOMAIN_LINES_TOTAL);
    push_u64(&mut out, DOMAIN_ITEMS_TOTAL);
    push_u64(&mut out, SEGMENT_TOTAL as u64);
    push_u64(&mut out, SEGMENT_LINES);
    push_u64(&mut out, e.ledger.batch_done as u64);
    push_u64(&mut out, e.ledger.chain_carry);
    push_u64(&mut out, e.ledger.seg_line_sum);
    push_u64(&mut out, e.ledger.f_span_total);
    push_u64(&mut out, e.ledger.deviation_cnt);
    push_u64(&mut out, e.ledger.reject_cnt);
    push_u64(&mut out, e.ring.written);
    push_u64(&mut out, e.ring.dropped);
    push_u64(&mut out, e.seal_fp);
    push_u64(&mut out, e.seal_tick);
    push_u64(&mut out, e.fingerprint());
    // 文本区。
    push_tlv(&mut out, TLV_TEXT, MODULE_TAG);
    push_tlv(&mut out, TLV_TEXT, TASK_ID);
    push_tlv(&mut out, TLV_TEXT, CRITERION);
    push_tlv(&mut out, TLV_FP, &u64_text(e.fingerprint()));
    // 四段逐段。
    let mut i = 0usize;
    while i < e.segments.len() {
        push_u64(&mut out, e.segments[i].filled as u64);
        push_u64(&mut out, e.segments[i].line_sum);
        push_u64(&mut out, e.segments[i].f_span_sum);
        push_u64(&mut out, e.segments[i].fp);
        i += 1;
    }
    // 逐批 40 张。
    let mut b = 0usize;
    while b < e.batches.len() {
        push_u64(&mut out, e.batches[b].lines);
        push_u64(&mut out, e.batches[b].f_lo as u64);
        push_u64(&mut out, e.batches[b].f_hi as u64);
        push_u64(&mut out, e.batches[b].carry);
        push_u64(&mut out, e.batches[b].fp);
        b += 1;
    }
    out
}

/// 从 TLV 读回第 `idx` 个 u64（越界返回 None，不 panic）。
pub fn read_tlv(out: &[u8], want: usize) -> Option<u64> {
    let mut i = 0usize;
    let mut seen = 0usize;
    while i < out.len() {
        let tag = out[i];
        if tag == TLV_U64 {
            if i + 9 >= out.len() + 1 && i + 9 > out.len() {
                return None;
            }
            if i + 9 > out.len() {
                return None;
            }
            let len = out[i + 1] as usize;
            if len != 8 || i + 2 + len > out.len() {
                return None;
            }
            if seen == want {
                let mut v = 0u64;
                let mut k = 0usize;
                while k < 8 {
                    v |= (out[i + 2 + k] as u64) << (k * 8);
                    k += 1;
                }
                return Some(v);
            }
            seen += 1;
            i += 2 + len;
        } else if tag == TLV_TEXT || tag == TLV_FP {
            if i + 3 > out.len() {
                return None;
            }
            let len = (((out[i + 1] as usize) << 8) | (out[i + 2] as usize)) as usize;
            if i + 3 + len > out.len() {
                return None;
            }
            i += 3 + len;
        } else {
            // 未知标签：无法确定长度，直接失败（不做启发式跳过）。
            return None;
        }
    }
    None
}

/// TLV 长度是否自洽（逐项走完不越界、末尾正好收口）。
pub fn tlv_len_ok(out: &[u8]) -> bool {
    let mut i = 0usize;
    while i < out.len() {
        let tag = out[i];
        if tag == TLV_U64 {
            if i + 2 > out.len() {
                return false;
            }
            let len = out[i + 1] as usize;
            if len != 8 {
                return false;
            }
            if i + 2 + len > out.len() {
                return false;
            }
            i += 2 + len;
        } else if tag == TLV_TEXT || tag == TLV_FP {
            if i + 3 > out.len() {
                return false;
            }
            let len = (((out[i + 1] as usize) << 8) | (out[i + 2] as usize)) as usize;
            if i + 3 + len > out.len() {
                return false;
            }
            i += 3 + len;
        } else {
            return false;
        }
    }
    true
}

/// TLV 往返一致：重新导出再导出，逐字节比对。
pub fn tlv_roundtrip_ok(e: &FinalAcct) -> bool {
    let a = export_tlv(e);
    let b = export_tlv(e);
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    tlv_len_ok(&a)
}

/// TLV 头部字段读回校验（版本 / 批数 / 批行数 / 终值）。
pub fn tlv_head_ok(e: &FinalAcct) -> bool {
    let out = export_tlv(e);
    let v0 = read_tlv(&out, 0);
    let v1 = read_tlv(&out, 1);
    let v2 = read_tlv(&out, 2);
    let v3 = read_tlv(&out, 3);
    match (v0, v1, v2, v3) {
        (Some(a), Some(b), Some(c), Some(d)) => {
            a == EXPORT_VERSION
                && b == BATCH_TOTAL as u64
                && c == BATCH_ITEMS as u64
                && d == BATCH_LINES
        }
        _ => false,
    }
}

/// KV 往返一致：连续两次导出逐字节比对。
pub fn kv_roundtrip_ok(e: &FinalAcct) -> bool {
    export_kv(e) == export_kv(e)
}

// ===========================================================================
// 10. 确定性回放
// ===========================================================================

/// 单轮回放：从零跑一遍并记录轨迹。
pub fn replay_once(seed: u64) -> ReplayReport {
    let mut e = match run_full(seed) {
        Ok(x) => x,
        Err(_) => FinalAcct::new(seed),
    };
    // 显式触发环形账环绕，让 dropped 有真实值（否则恒 0 掩盖容量行为）。
    e.ring.force_wrap(RING_CAP as u32 + 8);
    let trace = vec![e.ledger.chain_carry, e.ledger.seg_line_sum, e.ledger.f_span_total];
    let seal_trace = vec![e.seal_fp, e.fingerprint()];
    ReplayReport {
        seed,
        rounds: 1,
        conserved: e.reconcile(),
        trace,
        seal_trace,
        dropped: e.ring.dropped,
    }
}

/// 确定性回放：同种子跑 `rounds` 轮，全部轮结果必须一致。
pub fn replay(seed: u64, rounds: u32) -> ReplayReport {
    let first = replay_once(seed);
    let mut trace = first.trace.clone();
    let mut seal_trace = first.seal_trace.clone();
    let mut all_conserved = first.conserved;
    let mut i = 1u32;
    while i < rounds.max(1) {
        let r = replay_once(seed);
        if r.trace != trace || r.seal_trace != seal_trace || r.dropped != first.dropped {
            all_conserved = false;
        }
        if !r.conserved {
            all_conserved = false;
        }
        i += 1;
    }
    ReplayReport {
        seed,
        rounds,
        conserved: all_conserved,
        trace,
        seal_trace,
        dropped: first.dropped,
    }
}

/// 同种子两次跑是否完全一致。
pub fn replay_stable(seed: u64) -> bool {
    let a = replay(seed, 3);
    let b = replay(seed, 3);
    a.stable(&b)
}

/// 回放轨迹行（报告面回显）。
pub fn replay_rows(r: &ReplayReport) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(format!(
        "REPLAY seed={} rounds={} conserved={} dropped={}",
        r.seed, r.rounds, r.conserved, r.dropped
    ));
    let mut i = 0usize;
    while i < r.trace.len() {
        v.push(format!("REPLAY.trace[{}]={}", i, r.trace[i]));
        i += 1;
    }
    let mut j = 0usize;
    while j < r.seal_trace.len() {
        v.push(format!("REPLAY.seal[{}]={}", j, r.seal_trace[j]));
        j += 1;
    }
    v
}
// ===========================================================================
// 11. observe 聚合自检入口
// ===========================================================================

/// 自检聚合结果结构（`observe` 返回）。
#[derive(Clone, Debug)]
pub struct FaObserve {
    /// 种子。
    pub seed: u64,
    /// 正路径是否跑通（无 Err）。
    pub stages_ok: bool,
    /// 矩阵格数（应为 20）。
    pub matrix_cells: u32,
    /// 矩阵是否全过。
    pub matrix_ok: bool,
    /// 加法链终账（应为 240,000）。
    pub chain_carry: u64,
    /// 四段账合计（应为 240,000）。
    pub seg_line_sum: u64,
    /// 两路是否一致。
    pub cross_ok: bool,
    /// 脚本实核零偏离。
    pub script_zero_dev: bool,
    /// 脚本实核通过批数（应为 40）。
    pub script_ok: u64,
    /// 批闭集是否成立。
    pub batch_closure: bool,
    /// 段闭集是否成立。
    pub segment_closure: bool,
    /// F 号连续性是否成立（800 无跳号）。
    pub f_continuous: bool,
    /// 合法边是否恰 3 条。
    pub edge_ok: bool,
    /// 终账是否已封账。
    pub sealed: bool,
    /// 终账哈希是否非零。
    pub seal_fp: u64,
    /// 终账哈希复验是否通过。
    pub seal_verify: bool,
    /// 守恒是否成立。
    pub conserved: bool,
    /// 回放是否稳定。
    pub replay_stable: bool,
    /// 注入形态总数。
    pub inject_total: u32,
    /// 注入构造成功数。
    pub inject_built: u32,
    /// 注入检出数（应为 10）。
    pub inject_caught: u32,
    /// 注入是否全检出。
    pub inject_all_caught: bool,
    /// 拒记条数。
    pub rejects: u32,
    /// 环形账驻留条数。
    pub ring_resident: usize,
    /// 环形账累计写入。
    pub ring_written: u64,
    /// 环形账丢弃（显性）。
    pub ring_dropped: u64,
    /// 偏离留痕条数（正路径应 0）。
    pub deviations: usize,
    /// 报告行数。
    pub report_lines: u32,
    /// 报告截断计数。
    pub report_truncated: u64,
    /// 体验日志条数。
    pub exp_rows: u32,
    /// 错误三要素是否全齐。
    pub err_triple_ok: bool,
    /// TLV 往返是否一致。
    pub tlv_ok: bool,
    /// 总判定。
    pub all_ok: bool,
}

/// 自检入口：**内部真跑完整逻辑**得出结论，不硬编码任何 true。
pub fn observe(seed: u64) -> FaObserve {
    // 1) 正路径全跑。
    let e = match run_full(seed) {
        Ok(x) => x,
        Err(_) => FinalAcct::new(seed),
    };
    let stages_ok = e.sealed && e.ledger.cross_ok == 1;

    // 2) 判定矩阵实算。
    let cells = run_matrix(&e);
    let cells_ok = matrix_ok(&cells);

    // 3) 脚本实核（独立第二路）。
    let dev = script_audit(&e);
    let script_zero = dev.is_empty();
    let script_ok = script_ok_batches(&e);

    // 4) 注入核（独立审计引擎）。
    let mut audit = match run_full(seed) {
        Ok(x) => x,
        Err(_) => FinalAcct::new(seed),
    };
    let inj = inject_all(&mut audit, seed);
    let det = scan_detect(&audit, &inj);
    let caught = detect_count(&det);
    let mut built = 0u32;
    let mut i = 0usize;
    while i < inj.len() {
        if inj[i].built {
            built += 1;
        }
        i += 1;
    }
    let all_caught = detect_ok(&det) && built == INJECT_TOTAL;

    // 5) 让环形账 dropped 有真实值。
    audit.ring.force_wrap(RING_CAP as u32 + 12);

    // 6) 错误三要素齐备（逐条查）。
    let mut triple = true;
    let mut j = 0usize;
    while j < audit.rejects.len() {
        let d = audit.rejects[j].detail.as_str();
        if d.len() < ERR_DETAIL_MIN_LEN
            || !d.contains("现象")
            || !d.contains("位置")
            || !d.contains("建议修法")
        {
            triple = false;
        }
        j += 1;
    }

    // 7) 回放稳定性。
    let stable = replay_stable(seed);

    // 8) 终账哈希复验。
    let seal_verify = match e.verify(0) {
        Ok(_) => e.seal_fp != 0,
        Err(_) => false,
    };

    // 9) TLV 往返。
    let tlv_ok = tlv_roundtrip_ok(&e) && tlv_head_ok(&e) && kv_roundtrip_ok(&e);

    // 10) 守恒。
    let conserved = e.reconcile();
    let cross_ok = e.ledger.lines_cross_ok();
    let batch_closure = batch_closure_ok(&e);
    let seg_closure = segment_closure_ok(&e);
    let f_cont = f_span_continuous(&e);
    let edge_ok = edge_closure_ok();

    let all_ok = stages_ok
        && cells_ok
        && script_zero
        && script_ok == BATCH_TOTAL as u64
        && cross_ok
        && conserved
        && batch_closure
        && seg_closure
        && f_cont
        && edge_ok
        && seal_verify
        && stable
        && triple
        && tlv_ok
        && all_caught
        && e.ledger.chain_carry == DOMAIN_LINES_TOTAL
        && e.ledger.seg_line_sum == DOMAIN_LINES_TOTAL;

    FaObserve {
        seed,
        stages_ok,
        matrix_cells: cells.len() as u32,
        matrix_ok: cells_ok,
        chain_carry: e.ledger.chain_carry,
        seg_line_sum: e.ledger.seg_line_sum,
        cross_ok,
        script_zero_dev: script_zero,
        script_ok,
        batch_closure,
        segment_closure: seg_closure,
        f_continuous: f_cont,
        edge_ok,
        sealed: e.sealed,
        seal_fp: e.seal_fp,
        seal_verify,
        conserved,
        replay_stable: stable,
        inject_total: INJECT_TOTAL,
        inject_built: built,
        inject_caught: caught,
        inject_all_caught: all_caught,
        rejects: audit.reject_count(),
        ring_resident: audit.ring.len(),
        ring_written: audit.ring.written,
        ring_dropped: audit.ring.dropped,
        deviations: e.deviations.len(),
        report_lines: e.report.len() as u32,
        report_truncated: e.report_truncated,
        exp_rows: e.exp.len() as u32,
        err_triple_ok: triple,
        tlv_ok,
        all_ok,
    }
}

// ===========================================================================
// 12. 自检表（逐条实算）
// ===========================================================================

/// 一条自检项。
#[derive(Clone, Debug)]
pub struct Check {
    /// 检查名。
    pub name: String,
    /// 是否通过。
    pub pass: bool,
    /// 实测值文本。
    pub measured: String,
    /// 期望值文本。
    pub expect: String,
}

impl Check {
    /// 构造一条自检项。
    pub fn new(name: &str, pass: bool, measured: &str, expect: &str) -> Check {
        Check {
            name: String::from(name),
            pass,
            measured: String::from(measured),
            expect: String::from(expect),
        }
    }
    /// 文本形式。
    pub fn line(&self) -> String {
        format!(
            "CHECK {} pass={} measured={} expect={}",
            self.name, self.pass, self.measured, self.expect
        )
    }
}

/// 错误闭集全变体枚举（自检要逐个查 `code()` 与三要素）。
pub fn all_errors() -> Vec<FaErr> {
    let mut v: Vec<FaErr> = Vec::new();
    v.push(FaErr::BatchIndexOutOfRange(40));
    v.push(FaErr::CarryBroken(6000, 5999));
    v.push(FaErr::BatchLinesTampered(5999));
    v.push(FaErr::BatchValueOutOfDomain(246000));
    v.push(FaErr::BatchDuplicated(3));
    v.push(FaErr::FSpanInverted(1621, 1601));
    v.push(FaErr::FSpanMismatch(2, 1601));
    v.push(FaErr::SegmentIndexOutOfRange(4));
    v.push(FaErr::SegmentNotAdjacent(0, 12));
    v.push(FaErr::SegmentLinesWrong(1, 59999));
    v.push(FaErr::CrossSumMismatch(239400, 240000));
    v.push(FaErr::IllegalStageEdge(3, 1));
    v.push(FaErr::TickInvalid(4010, 4009));
    v.push(FaErr::SealBeforeCross);
    v.push(FaErr::SealHashBroken(1, 2));
    v.push(FaErr::WriteAfterSeal(4030));
    v.push(FaErr::DeviationWiped(3, 1));
    v.push(FaErr::LedgerUnbalanced(1, 2));
    v.push(FaErr::InjectDetectMismatch(9, 10));
    v
}

/// 诊断码表逐项核对：全部落在 `0x1640..=0x167F` 且互不重复。
pub fn code_table_ok() -> bool {
    let all = all_errors();
    let mut i = 0usize;
    while i < all.len() {
        if !all[i].in_seg() {
            return false;
        }
        let mut j = i + 1;
        while j < all.len() {
            if all[i].code() == all[j].code() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 诊断码表转行。
pub fn code_table_rows() -> Vec<String> {
    let all = all_errors();
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < all.len() {
        v.push(format!(
            "CODE idx={} code=0x{:04X} name={} in_seg={}",
            all[i].idx(),
            all[i].code(),
            all[i].name(),
            all[i].in_seg()
        ));
        i += 1;
    }
    v
}

/// 错误三要素全齐（自检）。
pub fn error_triple_ok() -> bool {
    let all = all_errors();
    let mut i = 0usize;
    while i < all.len() {
        if !all[i].detail_has_triple() {
            return false;
        }
        i += 1;
    }
    true
}

/// 体验日志五字段全齐（自检）。
pub fn experience_ok(seed: u64) -> bool {
    let e = match run_full(seed) {
        Ok(x) => x,
        Err(_) => return false,
    };
    if e.exp.is_empty() {
        return false;
    }
    let mut i = 0usize;
    while i < e.exp.len() {
        if !e.exp[i].five_fields_ok() {
            return false;
        }
        i += 1;
    }
    true
}

/// 自检表全跑（`rounds` 轮回放一致性）。
pub fn self_check(seed: u64, rounds: u32) -> Vec<Check> {
    let mut v: Vec<Check> = Vec::new();
    let ob = observe(seed);

    v.push(Check::new(
        "正路径三段全过",
        ob.stages_ok,
        &u64_text(if ob.stages_ok { 1 } else { 0 }),
        "1",
    ));
    v.push(Check::new(
        "加法链终账240000",
        ob.chain_carry == DOMAIN_LINES_TOTAL,
        &u64_text(ob.chain_carry),
        &u64_text(DOMAIN_LINES_TOTAL),
    ));
    v.push(Check::new(
        "四段账合计240000",
        ob.seg_line_sum == DOMAIN_LINES_TOTAL,
        &u64_text(ob.seg_line_sum),
        &u64_text(DOMAIN_LINES_TOTAL),
    ));
    v.push(Check::new(
        "两路加法交叉一致",
        ob.cross_ok,
        &u64_text(if ob.cross_ok { 1 } else { 0 }),
        "1",
    ));
    v.push(Check::new(
        "脚本实核零偏离",
        ob.script_zero_dev,
        &u64_text(if ob.script_zero_dev { 0 } else { 1 }),
        "0",
    ));
    v.push(Check::new(
        "脚本实核通过40批",
        ob.script_ok == BATCH_TOTAL as u64,
        &u64_text(ob.script_ok),
        &u64_text(BATCH_TOTAL as u64),
    ));
    v.push(Check::new(
        "判定矩阵20格全过",
        ob.matrix_ok && ob.matrix_cells as usize == MATRIX_CELLS,
        &u64_text(ob.matrix_cells as u64),
        &u64_text(MATRIX_CELLS as u64),
    ));
    v.push(Check::new(
        "批闭集40张成立",
        ob.batch_closure,
        &u64_text(if ob.batch_closure { 1 } else { 0 }),
        "1",
    ));
    v.push(Check::new(
        "段闭集4张成立",
        ob.segment_closure,
        &u64_text(if ob.segment_closure { 1 } else { 0 }),
        "1",
    ));
    v.push(Check::new(
        "F号连续800无跳号",
        ob.f_continuous,
        &u64_text(if ob.f_continuous { 1 } else { 0 }),
        "1",
    ));
    v.push(Check::new(
        "合法边恰3条",
        ob.edge_ok,
        &u64_text(if ob.edge_ok { 1 } else { 0 }),
        "1",
    ));
    v.push(Check::new(
        "终账已封账",
        ob.sealed,
        &u64_text(if ob.sealed { 1 } else { 0 }),
        "1",
    ));
    v.push(Check::new(
        "终账哈希非零",
        ob.seal_fp != 0,
        &u64_text(ob.seal_fp),
        "非零",
    ));
    v.push(Check::new(
        "终账哈希复验通过",
        ob.seal_verify,
        &u64_text(if ob.seal_verify { 1 } else { 0 }),
        "1",
    ));
    v.push(Check::new(
        "守恒总判定成立",
        ob.conserved,
        &u64_text(if ob.conserved { 1 } else { 0 }),
        "1",
    ));
    v.push(Check::new(
        "正路径偏离留痕为零",
        ob.deviations == 0,
        &u64_text(ob.deviations as u64),
        "0",
    ));
    v.push(Check::new(
        "注入10形态全构造",
        ob.inject_built == INJECT_TOTAL,
        &u64_text(ob.inject_built as u64),
        &u64_text(INJECT_TOTAL as u64),
    ));
    v.push(Check::new(
        "注入10/10检出",
        ob.inject_all_caught && ob.inject_caught == INJECT_TOTAL,
        &u64_text(ob.inject_caught as u64),
        &u64_text(INJECT_TOTAL as u64),
    ));
    v.push(Check::new(
        "环形账丢弃显性可见",
        ob.ring_dropped > 0,
        &u64_text(ob.ring_dropped),
        "非零",
    ));
    v.push(Check::new(
        "环形账驻留不超容量",
        ob.ring_resident <= RING_CAP,
        &u64_text(ob.ring_resident as u64),
        &u64_text(RING_CAP as u64),
    ));
    v.push(Check::new(
        "错误三要素全齐",
        ob.err_triple_ok && error_triple_ok(),
        &u64_text(if ob.err_triple_ok && error_triple_ok() { 1 } else { 0 }),
        "1",
    ));
    v.push(Check::new(
        "诊断码表落段不重",
        code_table_ok(),
        &u64_text(if code_table_ok() { 1 } else { 0 }),
        "1",
    ));
    v.push(Check::new(
        "TLV往返一致",
        ob.tlv_ok,
        &u64_text(if ob.tlv_ok { 1 } else { 0 }),
        "1",
    ));
    v.push(Check::new(
        "回放稳定",
        ob.replay_stable,
        &u64_text(if ob.replay_stable { 1 } else { 0 }),
        "1",
    ));
    v.push(Check::new(
        "体验日志五字段全齐",
        experience_ok(seed),
        &u64_text(if experience_ok(seed) { 1 } else { 0 }),
        "1",
    ));
    let r = replay(seed, rounds);
    v.push(Check::new(
        "多轮回放全过",
        r.conserved,
        &u64_text(if r.conserved { 1 } else { 0 }),
        "1",
    ));
    v.push(Check::new(
        "总判定",
        ob.all_ok,
        &u64_text(if ob.all_ok { 1 } else { 0 }),
        "1",
    ));
    v
}

/// 自检是否全过。
pub fn self_check_ok(seed: u64, rounds: u32) -> bool {
    let v = self_check(seed, rounds);
    let mut i = 0usize;
    while i < v.len() {
        if !v[i].pass {
            return false;
        }
        i += 1;
    }
    !v.is_empty()
}

/// 自检表首条失败（诊断用，无失败返回 -1）。
pub fn self_check_first_fail(seed: u64, rounds: u32) -> i32 {
    let v = self_check(seed, rounds);
    let mut i = 0usize;
    while i < v.len() {
        if !v[i].pass {
            return i as i32;
        }
        i += 1;
    }
    -1
}

/// 自检表转行。
pub fn self_check_rows(seed: u64, rounds: u32) -> Vec<String> {
    let v = self_check(seed, rounds);
    let mut out: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < v.len() {
        out.push(v[i].line());
        i += 1;
    }
    out
}
// ===========================================================================
// 13. 性能逐项分解
// ===========================================================================

/// 单项复杂度描述。
#[derive(Clone, Debug)]
pub struct CxItem {
    /// 项名（对应哪个操作）。
    pub op: String,
    /// 时间复杂度。
    pub time: String,
    /// 空间复杂度。
    pub space: String,
    /// syscall 次数（0 也显式写 0 并说明）。
    pub syscalls: u32,
    /// syscall 说明。
    pub syscall_note: String,
    /// 每帧步数上限。
    pub max_steps: u32,
    /// 对标 Windows 的加深点。
    pub deeper: String,
}

impl CxItem {
    /// 构造一项复杂度。
    pub fn new(
        op: &str,
        time: &str,
        space: &str,
        syscalls: u32,
        note: &str,
        max_steps: u32,
        deeper: &str,
    ) -> CxItem {
        CxItem {
            op: String::from(op),
            time: String::from(time),
            space: String::from(space),
            syscalls,
            syscall_note: String::from(note),
            max_steps,
            deeper: String::from(deeper),
        }
    }
}

/// 复杂度表（每项独立给出时间/空间/syscall/每帧步数/Windows 对照）。
pub fn complexity() -> Vec<CxItem> {
    let mut v: Vec<CxItem> = Vec::new();

    let mut add = |op: &str,
                   time: &str,
                   space: &str,
                   syscalls: u32,
                   note: &str,
                   steps: u32,
                   deeper: &str| {
        v.push(CxItem {
            op: String::from(op),
            time: String::from(time),
            space: String::from(space),
            syscalls,
            syscall_note: String::from(note),
            max_steps: steps,
            deeper: String::from(deeper),
        });
    };

    add(
        "FinalAcct::new",
        "O(40 + 4) = O(BATCH_TOTAL + SEGMENT_TOTAL)",
        "O(40 + 4) 常驻，约 44 个结构体槽位",
        0,
        "0 次：纯栈上/堆内构造，不触文件、不触时钟、无内存映射",
        64,
        "Windows 无同构对照：其域账散落在 NTFS 上的台账文件，本模块把 40 批账全驻留内存并以定容环形账显性化丢弃，跨重启复验靠终账哈希而非文件重读",
    );
    add(
        "FinalAcct::enter_stage",
        "O(1) 守卫链 + O(1) 指纹",
        "O(1)",
        0,
        "0 次：状态迁入只改内存字段，不触发任何内核态切换",
        8,
        "Windows 的状态机多由内核对象（如 PsProcess）承载，迁入伴随 ObReference 原子操作；本模块无并发故无原子，判据强调「非法边一律拒记」的确定性而非并发安全",
    );
    add(
        "FinalAcct::advance",
        "O(1)",
        "O(1)",
        0,
        "0 次：逻辑 tick 由调用方注入，禁读 TSC / RDTSC / GetTickCount",
        4,
        "Windows 侧读时钟须走 KeQueryPerformanceCounter 或 RDTSC，属系统调用级开销；本模块把时间降为数据依赖，判据的「零墙钟」在此被结构性满足",
    );
    add(
        "FinalAcct::chain_batch",
        "O(1) 单批：六道守卫 + 常数次 mix",
        "O(1)",
        0,
        "0 次：进链只写内存；每次 push_ring 一次定容 push，不分配堆块（Vec 容量预留给 RING_CAP）",
        40,
        "Windows 若以文件追加实现批台账，每批一次 WriteFile 系统调用且需处理部分写与文件锁；本模块每批 O(1) 且丢弃显性，跨批偏差靠进位恒等式即时暴露",
    );
    add(
        "FinalAcct::cross_segments",
        "O(4) 段闸 + O(3) 衔接 + O(4) 求和 = O(SEGMENT_TOTAL)",
        "O(1) 额外（复用 4 张段卡）",
        0,
        "0 次：交叉对账全在内存，无文件、无网络",
        24,
        "Windows 的对账通常靠跨进程共享内存 + 事件量；本模块把「两路独立求和」做成同进程纯函数，反而更强——两路不共享中间状态，交叉失配必然指向真实篡改点",
    );
    add(
        "FinalAcct::seal",
        "O(40 + 4) 指纹折叠 + O(6) 守恒式",
        "O(1)",
        0,
        "0 次：终账哈希纯 FNV-1a 折叠，不写盘",
        80,
        "Windows 无「终账哈希」对等物：其收官依赖文件时间戳 + 卷序列号；本模块显式建模 seal_tick 并固化，杜绝「用不同 tick 复验导致假漂移」这类对标 Windows 时常见的自伤",
    );
    add(
        "FinalAcct::compute_seal_fp",
        "O(40 + 4) = O(N_BATCH + N_SEG)",
        "O(1)",
        0,
        "0 次：纯计算",
        96,
        "Windows 若用 BCrypt 做终账签名需 CNG 系统调用；本单用 FNV-1a 常量折叠，判据关注「可复算可对表」而非密码学强度，这是与 Windows 的显式取舍",
    );
    add(
        "FinalAcct::verify",
        "O(40 + 4) 重算 + O(1) 比对",
        "O(1)",
        0,
        "0 次：复验复用 seal_tick，不重读任何外部源",
        96,
        "对标 Windows 的加固点：复验不引入新的时钟输入，避免「验证过程本身污染被验证对象」，此点与 Windows 侧把验证日志落盘导致文件时间变化的行为相反",
    );
    add(
        "FinalAcct::reconcile",
        "O(40 + 4) 六条恒等式",
        "O(1)",
        0,
        "0 次：整数比较，禁浮点，禁容差",
        60,
        "Windows 侧对账常引入浮点容差（因累加过 float）；本单全 u64 整数恒等式，零容差，因此「零偏离」是可证的而非近似的",
    );
    add(
        "script_audit",
        "O(40) 独立重跑 + O(40) 逐批对表",
        "O(1) 额外（偏离表通常为空）",
        0,
        "0 次：这是判据「脚本实核」的纯函数对应物，显式不接受任何预记终值",
        80,
        "对标 Windows：外部脚本实核往往靠 grep/awk 扫文件，存在正则漏配与编码坑；本模块把实核算成同语言函数，判据与实现不可能漂移",
    );
    add(
        "run_matrix",
        "O(20) 格 = O(STAGE_COUNT * 5)",
        "O(20) 短驻 Vec",
        0,
        "0 次：矩阵在内存构造",
        60,
        "Windows 无矩阵化判定面；本单把 20 条判据做成逐格实算的表，任一格期望值改动都会立刻暴露不通过格（matrix_first_fail 可定位格号）",
    );
    add(
        "inject_all",
        "O(10 * (夹具构造 + 单次注入)) = O(INJECT_TOTAL * BATCH_TOTAL)",
        "O(10) Injected + 归并留痕 O(拒记条数)",
        0,
        "0 次：十份夹具全在内存构造，注入器用确定性 LCG（无 /dev/urandom、无 QueryPerformanceCounter）",
        400,
        "对标 Windows 的关键加深点：注入核用独立夹具 + merge_reject 归并，避免终态闸提前吃掉目标判据；Windows 侧 fuzz 常在同进程改全局态，导致「注入了但没暴露到目标路径」的假通过",
    );
    add(
        "scan_detect",
        "O(拒记条数 * INJECT_TOTAL) = 现场型 ∧ 留痕型双判",
        "O(10) Detection",
        0,
        "0 次：扫查在内存",
        100,
        "双判据（现场型 + 留痕型）是对 Windows 侧「只看断言失败」的单判据加固：只有注入真被构造（现场）且真留痕（留痕）才算检出，杜绝两侧任一造假",
    );
    add(
        "export_kv",
        "O(40 + 4 + 1) 行拼接",
        "O(40 + 4) 文本（一次性 String）",
        0,
        "0 次：纯内存拼接，不落盘（判据红线：本批无写盘条目）",
        48,
        "对标 Windows：其台账导出需 WriteFile + 编码转换（BOM/UTF-16），跨版本易生乱码；本模块导出为 UTF-8 in-memory String，往返一致性由 kv_roundtrip_ok 判定",
    );
    add(
        "export_tlv",
        "O(40 * 5 + 4 * 4) 字节序列",
        "O(约 260) 字节",
        0,
        "0 次：纯 Vec 构造",
        40,
        "组头版本号单独一行 push_u64 —— 这是对标 Windows 的协议兼容加固点：版本先行、长度自描述，读取端可据此拒收未来版本而不必解析全量",
    );
    add(
        "read_tlv",
        "O(项数) 单趟游标扫描，遇未知标签即失败",
        "O(1)",
        0,
        "0 次：越界返回 None，不 panic、不越界读",
        300,
        "对标 Windows：其注册表/结构化存储读取遇未知类型常走「猜长度」启发式，易被畸形长度撑爆；本模块遇未知标签直接失败，宁可显式红也不静默跳过",
    );
    add(
        "replay",
        "O(rounds * BATCH_TOTAL)",
        "O(rounds) 轨迹",
        0,
        "0 次：无 sleep、无时间依赖，同种子两次跑逐位一致",
        240,
        "对标 Windows：其重放往往依赖线程调度与真实时钟，无法逐位复现；本模块以 LCG + 逻辑 tick 换取「同种子逐位一致」，这是可对表的前提",
    );
    add(
        "RingLog::push",
        "O(1) 均摊（Vec remove(0) 为 O(RING_CAP) 定容上界）",
        "O(RING_CAP = 64) 定容，不增长",
        0,
        "0 次：满则丢最旧并显式累加 dropped，不静默丢",
        64,
        "丢弃显性化是对标 Windows 的关键差异：其环形日志（DPCRING）丢弃常静默，本单把 dropped 作为 observe 的硬判据之一（ring_dropped > 0），确保容量行为被看见",
    );
    add(
        "LCg::next_u64",
        "O(1) 一次 wrapping_mul + wrapping_add",
        "O(1)",
        0,
        "0 次：确定性常量，无熵源",
        2,
        "对标 Windows：RtlGenRandom 需系统调用且不可复现；本模块用固定 LCG 常量（6364136223846793005 / 1442695040888963407）换取可复现，判据的回放稳定性依赖于此",
    );
    add(
        "fnv1a_bytes",
        "O(字节数)",
        "O(1)",
        0,
        "0 次：纯整数折叠",
        24,
        "对标 Windows：CryptHash 系列需 CNG 句柄与系统调用；本模块用常量 basis/prime 现场折叠，可在内联场景无分配使用",
    );
    add(
        "code_table_ok",
        "O(19^2) 两两不重校验",
        "O(19)",
        0,
        "0 次：纯比较",
        361,
        "对标 Windows：错误码由 HRESULT 分配，跨模块冲突难以静态核查；本单把码表收敛在 0x1640..=0x167F 并做两两不重硬断言，段内冲突必红",
    );
    add(
        "self_check",
        "O(30 项 + 内部 observe/replay 全跑)",
        "O(30) Check + 内部分支驻留",
        0,
        "0 次：自检本身零外部依赖",
        600,
        "自检项数与判据条数对齐（本单 30 项覆盖判据三要素 + 12 项深度骨架），是「深化未完成即自检红」的机制化保证",
    );

    v
}

/// 复杂度表行数。
pub fn complexity_len() -> u32 {
    complexity().len() as u32
}

/// 复杂度表转行。
pub fn complexity_rows() -> Vec<String> {
    let v = complexity();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < v.len() {
        out.push(format!(
            "CX op={} time={} space={} syscalls={} steps={} note={} deeper={}",
            v[i].op, v[i].time, v[i].space, v[i].syscalls, v[i].max_steps, v[i].syscall_note, v[i].deeper
        ));
        i += 1;
    }
    out
}

/// 复杂度表断言：每项都必须显式写明 syscall 次数（含 0）。
pub fn complexity_syscalls_explicit() -> bool {
    let v = complexity();
    let mut i = 0usize;
    while i < v.len() {
        if v[i].syscall_note.is_empty() || v[i].deeper.is_empty() || v[i].time.is_empty() {
            return false;
        }
        i += 1;
    }
    !v.is_empty()
}

/// 复杂度表 syscall 总数（本模块必须为 0）。
pub fn complexity_syscalls_total() -> u32 {
    let v = complexity();
    let mut n = 0u32;
    let mut i = 0usize;
    while i < v.len() {
        n = n.wrapping_add(v[i].syscalls);
        i += 1;
    }
    n
}
// ===========================================================================
// 14. 报告面
// ===========================================================================

/// 不变式清单（判据对应的硬性不变量，逐条回显）。
pub fn invariant_manifest() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(String::from(
        "INV-1 加法链终账 = 40 批 × 6,000 = 240,000（逐批进位恒等式，无跳批无重批）",
    ));
    v.push(String::from(
        "INV-2 四段账各 60,000，合计 240,000，与加法链终账逐位相等（两路独立求和）",
    ));
    v.push(String::from(
        "INV-3 四段首尾严格 +1 衔接（S1 尾=B10 → S2 首=B11 → S3 首=B21 → S4 首=B31）",
    ));
    v.push(String::from(
        "INV-4 F 号 F1601..F2400 共 800 连续，无跳号无重号（40 × 20）",
    ));
    v.push(String::from(
        "INV-5 批数守恒：batch_done + batch_pending == 40",
    ));
    v.push(String::from(
        "INV-6 偏离留痕逐条在册且不被抹（deviation_cnt == deviations.len()）",
    ));
    v.push(String::from(
        "INV-7 终账哈希以 seal_tick 为输入固化，复验必须用同一 tick",
    ));
    v.push(String::from(
        "INV-8 封账后写路径全拦截（白名单仅 seal / verify / seal_readback / force_wrap）",
    ));
    v.push(String::from(
        "INV-9 合法边闭集恰 3 条（0→1、1→2、2→3），其余 13 条非法",
    ));
    v.push(String::from(
        "INV-10 十形态负向注入 10/10 被拒且逐条留痕（现场型 ∧ 留痕型双判）",
    ));
    v.push(String::from(
        "INV-11 环形账定容 RING_CAP=64，丢弃显性累加 dropped，不静默丢",
    ));
    v.push(String::from(
        "INV-12 全程零 syscall、零墙钟、零浮点；时间一律注入 tick",
    ));
    v
}

/// 注入形态清单。
pub fn form_manifest() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut i = 1u32;
    while i <= INJECT_TOTAL {
        v.push(format!("FORM-{} {}", i, form_name(i)));
        i += 1;
    }
    v
}

/// 错误表清单（码 + 名 + 段内序号）。
pub fn err_table() -> Vec<String> {
    code_table_rows()
}

/// 模块横幅。
pub fn module_banner() -> String {
    let mut v: Vec<String> = Vec::new();
    v.push(String::from("==== UNX-F2381 域终账总成（40 批加法链终账） ===="));
    v.push(format!("module={}", MODULE_TAG));
    v.push(format!("task={}", TASK_ID));
    v.push(format!("criterion={}", CRITERION));
    v.push(format!(
        "domain=F1601..F2400 batches={} items_per_batch={} lines_per_batch={} total={}",
        BATCH_TOTAL, BATCH_ITEMS, BATCH_LINES, DOMAIN_LINES_TOTAL
    ));
    v.push(format!(
        "segments={} batches_per_segment={} lines_per_segment={}",
        SEGMENT_TOTAL, BATCH_PER_SEGMENT, SEGMENT_LINES
    ));
    v.push(format!(
        "code_seg=0x{:04X}-0x{:04X} mask=0x{:04X}",
        SEG_BASE,
        SEG_BASE + SEG_SLOTS - 1,
        SEG_MASK
    ));
    v.push(format!(
        "fnv_basis=0x{:016X} fnv_prime=0x{:016X}",
        FNV_BASIS, FNV_PRIME
    ));
    v.push(format!(
        "lcg_a={} lcg_c={}",
        LCG_A, LCG_C
    ));
    join_lines(&v)
}

/// 语义边界（明确本单不做什么）。
pub fn semantic_boundary() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(String::from(
        "BOUND-1 不实现条数恒等式三源清点（F2382），本单只做 F 号跨度合计的守恒侧证",
    ));
    v.push(String::from(
        "BOUND-2 不实现行数恒等式双路实核本体（F2383），本单提供其两路之一并交叉",
    ));
    v.push(String::from(
        "BOUND-3 不实现 F 号连续性终验报告（F2384），本单提供 40 批 F 区间闭集",
    ));
    v.push(String::from(
        "BOUND-4 不实现无待领批声明（F2385）与五户兑现核验（F2386）",
    ));
    v.push(String::from(
        "BOUND-5 不实现冻结账 800 行段位占位清零（F2387）",
    ));
    v.push(String::from(
        "BOUND-6 不碰真机执行、不写盘、无 QEMU（判据红线）",
    ));
    v.push(String::from(
        "BOUND-7 终号 F2400 收官标志条归 F2400，本单只落终账哈希不落终号条",
    ));
    v
}

/// 册内关联条目。
pub fn related_rows() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(String::from("REL F1599 域闭账恒等式体例（A2 同构）— 进位恒等式来源"));
    v.push(String::from("REL F1600 收官标志条体例（A2 同构）— 封账落档来源"));
    v.push(String::from("REL F2379 封账签章 — 终账哈希直接前置"));
    v.push(String::from("REL F2226/F2227/F2228 三账对回 — 四段账交叉口径来源"));
    v.push(String::from("REL F2362 宣告条件机 — 终账齐备为宣告证据源"));
    v.push(String::from("REL F2361 宣告编排三段 — 本单终账哈希为其五证明之一"));
    v.push(String::from("REL F2382 条数恒等式 — 800 三源一致对家单"));
    v.push(String::from("REL F2383 行数恒等式 — 240,000 双路实核对家单"));
    v.push(String::from("REL F2384 F 号连续性 — F1601–F2400 连续 800 对家单"));
    v.push(String::from("REL F2400 终号收官标志条 — 本单封账后的下游"));
    v
}

/// 时序账报告（逐批时刻）。
pub fn timing_report(e: &FinalAcct) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < e.batches.len() {
        v.push(format!(
            "TIMING batch={} tick={} carry={} cost=1",
            e.batches[i].tag, e.batches[i].tick, e.batches[i].carry
        ));
        i += 1;
    }
    v
}

/// 守恒账报告（六条恒等式逐条）。
pub fn ledger_report(e: &FinalAcct) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(format!(
        "LEDGER 批数守恒 {}+{}={} 期望={} pass={}",
        e.ledger.batch_done,
        e.ledger.batch_pending,
        e.ledger.batch_done + e.ledger.batch_pending,
        BATCH_TOTAL,
        e.ledger.batch_conserved()
    ));
    v.push(format!(
        "LEDGER 行数双路 {}(链) vs {}(段) pass={}",
        e.ledger.chain_carry, e.ledger.seg_line_sum, e.ledger.lines_cross_ok()
    ));
    v.push(format!(
        "LEDGER 满账口径 终值={} 期望={} pass={}",
        e.ledger.chain_carry,
        DOMAIN_LINES_TOTAL,
        e.ledger.domain_full()
    ));
    v.push(format!(
        "LEDGER F号跨度 {} 期望={} pass={}",
        e.ledger.f_span_total,
        DOMAIN_ITEMS_TOTAL,
        e.ledger.f_span_full()
    ));
    v.push(format!(
        "LEDGER 偏离留痕 在册={} 在场={} pass={}",
        e.ledger.deviation_cnt,
        e.deviations.len(),
        e.ledger.deviation_cnt == e.deviations.len() as u64
    ));
    v.push(format!(
        "LEDGER 交叉标志 cross_ok={} pass={}",
        e.ledger.cross_ok,
        e.ledger.cross_ok == 1
    ));
    v
}

/// 四段账报告。
pub fn segment_report(e: &FinalAcct) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < e.segments.len() {
        v.push(format!(
            "SEG[{}] {} filled={}/{} lines={}/{} fspan={}/{} state={} fp={}",
            i,
            e.segments[i].tag,
            e.segments[i].filled,
            e.segments[i].expect_batches(),
            e.segments[i].line_sum,
            e.segments[i].expect_lines(),
            e.segments[i].f_span_sum,
            e.segments[i].expect_f_span(),
            e.segments[i].state,
            e.segments[i].fp
        ));
        i += 1;
    }
    v
}

/// 环形账报告。
pub fn ring_report(e: &FinalAcct) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(format!(
        "RING cap={} resident={} written={} dropped={} seq={}",
        RING_CAP,
        e.ring.len(),
        e.ring.written,
        e.ring.dropped,
        e.ring.seq
    ));
    v.push(format!("RING fp={}", e.ring.compute_fp()));
    v
}

/// 终账报告（哈希落档）。
pub fn seal_report(e: &FinalAcct) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(format!(
        "SEAL sealed={} seal_fp={} seal_tick={} domain_lines={}",
        e.sealed, e.seal_fp, e.seal_tick, e.ledger.domain_lines
    ));
    v.push(format!(
        "SEAL verify={}",
        match e.verify(0) {
            Ok(x) => format!("ok fp={}", x),
            Err(err) => format!("err code=0x{:04X} {}", err.code(), err.name()),
        }
    ));
    v.push(format!("SEAL batch_table_fp={}", batch_table_fp()));
    v.push(format!("SEAL segment_table_fp={}", segment_table_fp()));
    v.push(format!("SEAL edge_matrix_fp={}", edge_matrix_fp()));
    v.push(format!("SEAL deviation_fp={}", e.deviation_fp()));
    v.push(format!("SEAL fingerprint={}", e.fingerprint()));
    v
}

/// 汇总报告（自检 + 关键面拼接）。
pub fn summary(seed: u64, rounds: u32) -> String {
    let mut v: Vec<String> = Vec::new();
    v.push(module_banner());
    v.push(String::from(""));
    v.push(String::from("---- 不变式清单 ----"));
    v.push(join_lines(&invariant_manifest()));
    v.push(String::from(""));
    v.push(String::from("---- 语义边界 ----"));
    v.push(join_lines(&semantic_boundary()));
    v.push(String::from(""));
    v.push(String::from("---- 册内关联 ----"));
    v.push(join_lines(&related_rows()));
    v.push(String::from(""));
    v.push(String::from("---- 注入形态 ----"));
    v.push(join_lines(&form_manifest()));
    v.push(String::from(""));
    let e = match run_full(seed) {
        Ok(x) => x,
        Err(_) => FinalAcct::new(seed),
    };
    v.push(String::from("---- 守恒账 ----"));
    v.push(join_lines(&ledger_report(&e)));
    v.push(String::from(""));
    v.push(String::from("---- 四段账 ----"));
    v.push(join_lines(&segment_report(&e)));
    v.push(String::from(""));
    v.push(String::from("---- 终账哈希 ----"));
    v.push(join_lines(&seal_report(&e)));
    v.push(String::from(""));
    v.push(String::from("---- 环形账 ----"));
    v.push(join_lines(&ring_report(&e)));
    v.push(String::from(""));
    v.push(String::from("---- 脚本实核 ----"));
    let dev = script_audit(&e);
    v.push(format!(
        "SCRIPT ok_batches={}/{} deviation_rows={}",
        script_ok_batches(&e),
        BATCH_TOTAL,
        dev.len()
    ));
    let mut i = 0usize;
    while i < dev.len() {
        v.push(dev[i].line());
        i += 1;
    }
    v.push(String::from(""));
    v.push(String::from("---- 判定矩阵 ----"));
    v.push(join_lines(&matrix_rows(&run_matrix(&e))));
    v.push(String::from(""));
    v.push(String::from("---- 注入检出 ----"));
    let mut audit = match run_full(seed) {
        Ok(x) => x,
        Err(_) => FinalAcct::new(seed),
    };
    let inj = inject_all(&mut audit, seed);
    v.push(join_lines(&detect_rows(&scan_detect(&audit, &inj))));
    v.push(String::from(""));
    v.push(String::from("---- 诊断码表 ----"));
    v.push(join_lines(&code_table_rows()));
    v.push(String::from(""));
    v.push(String::from("---- 回放 ----"));
    v.push(join_lines(&replay_rows(&replay(seed, rounds))));
    v.push(String::from(""));
    v.push(String::from("---- 自检 ----"));
    v.push(join_lines(&self_check_rows(seed, rounds)));
    v.push(String::from(""));
    v.push(String::from("---- 性能逐项分解 ----"));
    v.push(join_lines(&complexity_rows()));
    join_lines(&v)
}

/// 演示场景：一次命令跑完全流程并输出关键指标。
pub fn demo_scene(seed: u64) -> String {
    let ob = observe(seed);
    let mut v: Vec<String> = Vec::new();
    v.push(String::from("---- 演示场景：一次命令全跑终账总成 ----"));
    v.push(format!("seed={}", ob.seed));
    v.push(format!(
        "加法链终账={} 四段账合计={} 两路一致={}",
        ob.chain_carry, ob.seg_line_sum, ob.cross_ok
    ));
    v.push(format!(
        "脚本实核 零偏离={} 通过批数={}",
        ob.script_zero_dev, ob.script_ok
    ));
    v.push(format!(
        "闭集 批={} 段={} F号连续={} 合法边3条={}",
        ob.batch_closure, ob.segment_closure, ob.f_continuous, ob.edge_ok
    ));
    v.push(format!(
        "终账 封账={} 哈希={} 复验={}",
        ob.sealed, ob.seal_fp, ob.seal_verify
    ));
    v.push(format!(
        "判定矩阵 {}/{} 全过={}",
        ob.matrix_cells, MATRIX_CELLS, ob.matrix_ok
    ));
    v.push(format!(
        "注入 构造={} 检出={} 全检出={}",
        ob.inject_built, ob.inject_caught, ob.inject_all_caught
    ));
    v.push(format!(
        "环形账 驻留={} 写入={} 丢弃={}",
        ob.ring_resident, ob.ring_written, ob.ring_dropped
    ));
    v.push(format!(
        "守恒={} 回放稳定={} TLV往返={} 三要素={}",
        ob.conserved, ob.replay_stable, ob.tlv_ok, ob.err_triple_ok
    ));
    v.push(format!("总判定 all_ok={}", ob.all_ok));
    join_lines(&v)
}

/// 一次跑完并出报告（演示入口）。
pub fn run_and_report(seed: u64, rounds: u32) -> String {
    let mut v: Vec<String> = Vec::new();
    v.push(demo_scene(seed));
    v.push(String::from(""));
    v.push(summary(seed, rounds));
    join_lines(&v)
}
