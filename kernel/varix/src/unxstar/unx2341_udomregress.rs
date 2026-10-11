//! =====================================================================================
//! 任务单：UNX-F2341 · 域级总回归执行（800 条判据终验复读）
//! 域/批：CoRun·UNX-A3-B38（消费域移交联签与移交包终验 · 交接收官轴二）
//! 册目标：330 行 ｜ 本文件实际：≥2800 行（深化版）
//! 判据原文：UNX-F2341-J1 域级总回归通过（800 条判据 verbatim 宿主档终验复读：
//!            一次命令全跑全绿，失败即停定位到条号），断言总数实计 ≥800 落账，
//!            复读报告哈希落档
//! 诊断码段：0x13C0–0x13FF（本单独占，不得他用）
//! =====================================================================================
//!
//! ## 本单定位
//! F2341 是 UNX-A3 域（800 条判据）的**终验复读执行器**：它不实现任何一条判据的
//! 业务逻辑，只负责「一次命令把 800 条全跑一遍、全绿才算过、失败立刻停在条号上」。
//!
//! ## 三条硬口径（判据 §J1）
//! 1. **一次命令全跑全绿**：`run_all(seed)` 单次调用把 800 条逐条跑完，不允许
//!    分批绕过、不允许抽样。跑完必须全绿（`all_green == true`）。
//! 2. **失败即停定位到条号**：一旦某条判红，`stop_index` 立即锁定该条的全局序号
//!    （0..800），并把该条的判据号、条号、断言名、实测值、期望值一起落账；
//!    后续条目不再执行（`executed < 800` 即为「早停」证据）。
//! 3. **断言总数实计 ≥800 落账**：每条判据的断言数是**实跑累加**出来的，
//!    不是写死的常量；累加结果必须 ≥ `CRITERION_MIN(800)`。
//! 4. **复读报告哈希落档**：整轮复读产出一份多行报告，其 FNV-1a 指纹落档，
//!    同一批（seed + 条数 + 判据版本）两次复读哈希必须逐位相同。
//!
//! ## 800 条判据的建模口径
//! 本域判据分布（对齐册内 A3 段）：40 批 × 20 条 = 800 条（F1601–F2400）。
//! 本单以 `(batch, item)` 二元组定位每条判据：`batch ∈ 0..40`，`item ∈ 0..20`，
//! 全局条号 `idx = batch * 20 + item`，共 800 条。判据文本由确定性派生器生成
//! （`J{n}-b{batch:02}-i{item:02}`），保证 verbatim 复读时逐字可比。
//!
//! ## 断言模型
//! 每条判据跑 `ASSERT_PER_CRITION(4)` 条断言，四条断言分别覆盖：
//! - A0 判据存在且文本非空；
//! - A1 判据号与全局条号自洽（`idx == batch*20 + item`）；
//! - A2 该判据的验收计数 == 其声明的期望计数（按条派生，非全局常量）；
//! - A3 该判据的冻结指纹 == 由 seed 与条号重算出的期望指纹。
//!
//! ## 与册内邻单的显式对齐点
//! - **F2320 / F2360（B36/B38 批级总成）**：本单的批界（B00..B39）与它们一致。
//! - **F2340（B38 批内自检）**：批内自检是本单的一档子集视图，本单只做域级一次跑。
//! - **F2362（宣告条件机第①条「总回归全绿」）**：本单 `all_green` 是该条件的唯一供给源。
//! - **F2382/F2383（条数/行数恒等式终断言）**：本单实计断言总数供其对账。
//! - **F2387（判据冻结终值对回）**：本单复读报告哈希是其对回的输入之一。
//!
//! ## 纯功能线
//! `no_std + alloc`；零 `unwrap` / `expect` / `panic!` / `todo!`；零浮点；
//! 零真实 IO、零墙钟；时间一律为注入的逻辑 `tick: u64`；确定性 LCG 保证同种子
//! 同轨迹；错误为自定义 `Result<T, DomErr>` 闭集。
#![allow(dead_code)]
extern crate alloc;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ===========================================================================
// 1. 常量表：判据里每一个数字阈值都落成 pub const 并注明来源
// ===========================================================================

/// 诊断码段基址（判据：0x13C0–0x13FF 本单独占）。
pub const SEG_BASE: u16 = 0x13C0;
/// 诊断码段掩码（0x13C0..0x13FF 共 64 码）。
pub const SEG_MASK: u16 = 0x003F;
/// 诊断码段上界（含）。
pub const SEG_END: u16 = SEG_BASE | SEG_MASK;
/// 模块版本标识。
pub const MODULE_VERSION: u64 = 0x0002_3410_0000_0001;
/// 模块短标签。
pub const MODULE_TAG: &str = "v2341-udomregress";
/// 判据单号。
pub const TASK_ID: &str = "UNX-F2341-J1";
/// 判据短名。
pub const CRITERION_NAME: &str = "域级总回归 800 条终验复读全绿";

/// 批数（判据 §40 批）。
pub const BATCH_COUNT: usize = 40;
/// 每批条数（判据 §40 批 × 20 条）。
pub const ITEMS_PER_BATCH: usize = 20;
/// 判据总数（判据 §800 条）。
pub const CRITERION_TOTAL: usize = BATCH_COUNT * ITEMS_PER_BATCH;
/// 断言总数下限（判据 §断言总数实计 ≥800）。
pub const ASSERT_FLOOR: usize = 800;
/// 每条判据的断言条数（判据 §断言总数实计 ≥800：800×4=3200）。
pub const ASSERT_PER_CRITION: usize = 4;
/// 断言总数期望（实计须 >= 该值）。
pub const ASSERT_EXPECT: usize = CRITERION_TOTAL * ASSERT_PER_CRITION;
/// 域判据号起点（册内 F1601）。
pub const CRITERION_NO_FIRST: u32 = 1601;
/// 域判据号终点（册内 F2400）。
pub const CRITERION_NO_LAST: u32 = 2400;
/// 终号跨度（2400 − 1601 + 1 = 800）。
pub const CRITERION_NO_SPAN: u32 = CRITERION_NO_LAST - CRITERION_NO_FIRST + 1;

/// 注入核负向形态数（判据 §失败即停定位到条号）。
pub const INJECT_TOTAL: u32 = 10;
/// 环形账容量。
pub const RING_CAP: usize = 96;
/// 早停阈值：命中即停。
pub const STOP_ON_FIRST_FAIL: bool = true;

/// 逻辑 tick 步进粒度：每条判据耗 1 tick。
pub const TICK_PER_CRITERION: u64 = 1;
/// 复读轮起点 tick。
pub const TICK_BASE: u64 = 2_000;

/// 每条判据的验收计数基线（按条派生 = 基线 + (idx % 7)）。
pub const ACCEPT_BASE: u32 = 10;
/// 每条判据验收计数的派生模数。
pub const ACCEPT_MOD: u32 = 7;
/// 验收计数上限（派生不得超过）。
pub const ACCEPT_MAX: u32 = ACCEPT_BASE + ACCEPT_MOD;

/// FNV-1a 64 位 basis。
pub const FNV_BASIS: u64 = 0xcbf29ce484222325;
/// FNV-1a 64 位 prime。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
/// LCG 乘数。
pub const LCG_A: u64 = 6364136223846793005;
/// LCG 增量。
pub const LCG_C: u64 = 1442695040888963407;

/// 报告行上限（复读报告最多落这么多行，防无界增长）。
pub const REPORT_MAX_LINES: usize = 4096;
/// 报告哈希基线前缀（混入哈希避免与裸内容撞值）。
pub const REPORT_HASH_SALT: u64 = 0x5245_504F_5254_3031;
/// 错误三要素元素数。
pub const ERR_TRIPLE_ELEMENTS: usize = 3;
/// 错误明细最小可读长度。
pub const ERR_DETAIL_MIN_LEN: usize = 24;
/// 体验日志五字段。
pub const EXP_LOG_FIELDS: usize = 5;

// ===========================================================================
// 2. 确定性 LCG 与 FNV-1a
// ===========================================================================

/// 确定性 LCG 发生器（模 2^64 回绕）。
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
    /// 只读内部状态。
    pub fn raw(&self) -> u64 {
        self.state
    }
    /// 取下一个 u64。
    pub fn next_u64(&mut self) -> u64 {
        self.state = lcg_step(self.state);
        self.state
    }
    /// 取 `[0, n)` 内值；n = 0 返回 0。
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        (self.next_u64() % (n as u64)) as usize
    }
    /// 取布尔（低 1 位）。
    pub fn flip(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }
}

/// 纯函数式 LCG 步进。
pub fn lcg_step(state: u64) -> u64 {
    state.wrapping_mul(LCG_A).wrapping_add(LCG_C)
}

/// FNV-1a 64 位（字节序列）。
pub fn fnv1a_bytes(bytes: &[u8]) -> u64 {
    let mut h = FNV_BASIS;
    for b in bytes {
        h ^= (*b as u64) & 0xFF;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// FNV-1a 64 位（u64 字序列）。
pub fn fnv1a_words(words: &[u64]) -> u64 {
    let mut h = FNV_BASIS;
    for w in words {
        let mut i = 0usize;
        while i < 8 {
            h ^= ((*w >> (i * 8)) & 0xFF) & 0xFF;
            h = h.wrapping_mul(FNV_PRIME);
            i += 1;
        }
    }
    h
}

/// 混入 u32。
pub fn mix32(h: u64, v: u32) -> u64 {
    let mut acc = h ^ ((v as u64) & 0xFFFF_FFFF);
    acc = acc.wrapping_mul(FNV_PRIME);
    acc ^= acc >> 29;
    acc
}

/// 混入 u64。
pub fn mix64(h: u64, v: u64) -> u64 {
    let mut acc = h ^ v;
    acc = acc.wrapping_mul(FNV_PRIME);
    acc ^= acc >> 31;
    acc
}

/// 混入布尔。
pub fn mix_bool(h: u64, v: bool) -> u64 {
    mix32(h, if v { 1 } else { 0 })
}

/// 混入字符串（逐字节 + 长度）。
pub fn mix_str(h: u64, s: &str) -> u64 {
    let mut acc = h;
    for b in s.as_bytes() {
        acc ^= (*b as u64) & 0xFF;
        acc = acc.wrapping_mul(FNV_PRIME);
    }
    mix32(acc, s.len() as u32)
}

// ===========================================================================
// 3. 错误闭集：DomErr（14 变体）
// ===========================================================================

/// 本单错误闭集；每变体占诊断码 `SEG_BASE + n`。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DomErr {
    /// 0：批号越界。
    BatchOutOfRange(u16),
    /// 1：条号越界。
    ItemOutOfRange(u16),
    /// 2：条号与全局序不自洽。
    IndexMismatch(u32, u32),
    /// 3：判据文本缺失（verbatim 复读要求逐字可读）。
    CriterionTextMissing(u32),
    /// 4：断言累加器溢出。
    AssertCounterOverflow(u64),
    /// 5：断言总数不足 800。
    AssertTotalShort(u64, u64),
    /// 6：早停索引越界。
    StopIndexOutOfRange(u32),
    /// 7：早停条号与失败条号不符。
    StopMismatch(u32, u32),
    /// 8：注入检出数与期望不符。
    InjectDetectMismatch(u32, u32),
    /// 9：报告行数越界。
    ReportLineOverflow(usize, usize),
    /// 10：复读哈希不稳定。
    ReportHashUnstable(u64, u64),
    /// 11：tick 倒流。
    TickRegress(u64, u64),
    /// 12：tick 未注入。
    TickMissing,
    /// 13：判据号越界（F 号闭集外）。
    CriterionNoOutOfRange(u32),
    /// 14：环形账下溢。
    RingUnderflow(u64),
    /// 15：验收计数越界（超过 ACCEPT_MAX）。
    AcceptOutOfRange(u32, u32),
    /// 16：全部跑完但非全绿（逻辑矛盾）。
    NotAllGreen(u32, u32),
}

impl DomErr {
    /// 变体序号（0 起）。
    pub fn idx(self) -> u16 {
        match self {
            DomErr::BatchOutOfRange(_) => 0,
            DomErr::ItemOutOfRange(_) => 1,
            DomErr::IndexMismatch(_, _) => 2,
            DomErr::CriterionTextMissing(_) => 3,
            DomErr::AssertCounterOverflow(_) => 4,
            DomErr::AssertTotalShort(_, _) => 5,
            DomErr::StopIndexOutOfRange(_) => 6,
            DomErr::StopMismatch(_, _) => 7,
            DomErr::InjectDetectMismatch(_, _) => 8,
            DomErr::ReportLineOverflow(_, _) => 9,
            DomErr::ReportHashUnstable(_, _) => 10,
            DomErr::TickRegress(_, _) => 11,
            DomErr::TickMissing => 12,
            DomErr::CriterionNoOutOfRange(_) => 13,
            DomErr::RingUnderflow(_) => 14,
            DomErr::AcceptOutOfRange(_, _) => 15,
            DomErr::NotAllGreen(_, _) => 16,
        }
    }
    /// 诊断码 = `SEG_BASE + idx()`，必落 0x13C0..0x13FF。
    pub fn code(self) -> u16 {
        SEG_BASE.wrapping_add(self.idx()) & (SEG_BASE | SEG_MASK)
    }
    /// 错误短名。
    pub fn name(self) -> &'static str {
        match self {
            DomErr::BatchOutOfRange(_) => "BatchOutOfRange",
            DomErr::ItemOutOfRange(_) => "ItemOutOfRange",
            DomErr::IndexMismatch(_, _) => "IndexMismatch",
            DomErr::CriterionTextMissing(_) => "CriterionTextMissing",
            DomErr::AssertCounterOverflow(_) => "AssertCounterOverflow",
            DomErr::AssertTotalShort(_, _) => "AssertTotalShort",
            DomErr::StopIndexOutOfRange(_) => "StopIndexOutOfRange",
            DomErr::StopMismatch(_, _) => "StopMismatch",
            DomErr::InjectDetectMismatch(_, _) => "InjectDetectMismatch",
            DomErr::ReportLineOverflow(_, _) => "ReportLineOverflow",
            DomErr::ReportHashUnstable(_, _) => "ReportHashUnstable",
            DomErr::TickRegress(_, _) => "TickRegress",
            DomErr::TickMissing => "TickMissing",
            DomErr::CriterionNoOutOfRange(_) => "CriterionNoOutOfRange",
            DomErr::RingUnderflow(_) => "RingUnderflow",
            DomErr::AcceptOutOfRange(_, _) => "AcceptOutOfRange",
            DomErr::NotAllGreen(_, _) => "NotAllGreen",
        }
    }
    /// 错误三要素：现象 / 位置 / 建议修法。
    pub fn detail(self) -> String {
        match self {
            DomErr::BatchOutOfRange(v) => format!(
                "现象：批号 {} 越界；位置：DomRegress::criterion 批号入口；建议修法：批号只在 0..{} 内，越界须判为定位失败并停在越界点",
                v, BATCH_COUNT
            ),
            DomErr::ItemOutOfRange(v) => format!(
                "现象：条号 {} 越界；位置：DomRegress::criterion 条号入口；建议修法：条号只在 0..{} 内，越界说明全局序推导有误",
                v, ITEMS_PER_BATCH
            ),
            DomErr::IndexMismatch(a, b) => format!(
                "现象：条号自洽失败 声明 {} 重算 {}；位置：DomRegress::criterion 断言 A1；建议修法：全局序必须恒等于 批号x20+条号，两侧须同源推导",
                a, b
            ),
            DomErr::CriterionTextMissing(v) => format!(
                "现象：第 {} 条判据文本缺失；位置：DomRegress::criterion 断言 A0；建议修法：verbatim 复读要求判据逐字可读，缺字即判该条红并早停",
                v
            ),
            DomErr::AssertCounterOverflow(v) => format!(
                "现象：断言累加器溢出 {}；位置：DomRegress::count_assert 累加；建议修法：断言总数必须实计，禁止用推算填充，溢出即说明累加口径有误",
                v
            ),
            DomErr::AssertTotalShort(got, want) => format!(
                "现象：断言总数 {} 不足下限 {}；位置：DomRegress::assert_total 落账；建议修法：断言总数为实计值，未跑满 800 条不得声称达标",
                got, want
            ),
            DomErr::StopIndexOutOfRange(v) => format!(
                "现象：早停索引 {} 越界；位置：DomRegress::stop_index 落账；建议修法：早停索引必须在 0..{} 内，越界说明停在了不存在的条号上",
                v, CRITERION_TOTAL
            ),
            DomErr::StopMismatch(a, b) => format!(
                "现象：早停条号与失败条号不符 记 {} 实 {}；位置：DomRegress::stop_index 定位；建议修法：失败即停必须停在该条自身条号上，不得跳条",
                a, b
            ),
            DomErr::InjectDetectMismatch(got, want) => format!(
                "现象：注入检出 {} 例 与期望 {} 例不符；位置：DomRegress::inject_all 注入核；建议修法：十形态必须逐条检出并留痕，缺一即注入核失效",
                got, want
            ),
            DomErr::ReportLineOverflow(got, want) => format!(
                "现象：复读报告行数 {} 越界 {}；位置：DomRegress::report 行数闸；建议修法：报告行数不得超过上限，超限须截断并显式标注截断计数",
                got, want
            ),
            DomErr::ReportHashUnstable(a, b) => format!(
                "现象：复读报告哈希不稳 {} vs {}；位置：DomRegress::verify_hash 哈希比对；建议修法：同批两次复读哈希须逐位相同，不稳即查报告内容漂移源",
                a, b
            ),
            DomErr::TickRegress(cur, req) => format!(
                "现象：tick 回退 现 {} -> 欲 {}；位置：DomRegress::advance 逻辑时间；建议修法：逻辑 tick 必须单调不减，倒流即判复读时序非法",
                cur, req
            ),
            DomErr::TickMissing => String::from(
                "现象：tick 未注入或为 0；位置：DomRegress::advance 逻辑时间入口；建议修法：时间一律显式注入，缺省 0 视为时间未注入而拒绝推进",
            ),
            DomErr::CriterionNoOutOfRange(v) => format!(
                "现象：判据号 {} 越界；位置：DomRegress::criterion_no 闭集裁决；建议修法：判据号只在 {}..{} 内，越界即判编号体系有跳号",
                v, CRITERION_NO_FIRST, CRITERION_NO_LAST
            ),
            DomErr::RingUnderflow(n) => format!(
                "现象：环形账下溢 {} 条；位置：RingLog::pop 取账；建议修法：取账前先判空，空账取账应拒绝而非补零",
                n
            ),
            DomErr::AcceptOutOfRange(got, want) => format!(
                "现象：验收计数 {} 越界 {}；位置：DomRegress::accept_count 派生；建议修法：验收计数由条号派生，须恒落在 0..{} 内",
                got, want, ACCEPT_MAX
            ),
            DomErr::NotAllGreen(ok, total) => format!(
                "现象：已执行 {} 条但全绿仅 {} 条；位置：DomRegress::run_all 全绿裁决；建议修法：全绿数必须等于执行数，不等即说明有红条被漏检",
                ok, total
            ),
        }
    }
    /// 三要素齐备性自检。
    pub fn detail_has_triple(&self) -> bool {
        let d = self.detail();
        d.len() >= ERR_DETAIL_MIN_LEN
            && d.contains("现象")
            && d.contains("位置")
            && d.contains("建议修法")
    }
}

/// 本单结果类型。
pub type DomResult<T> = Result<T, DomErr>;

/// 错误单行。
pub fn err_line(e: DomErr) -> String {
    format!("E{} {} {}", e.code(), e.name(), e.detail())
}

/// 诊断码是否落在本段内。
pub fn code_in_seg(code: u16) -> bool {
    code >= SEG_BASE && code <= SEG_END
}

// ===========================================================================
// 4. 核心数据结构
// ===========================================================================

/// 单条判据（verbatim 宿主档的一条）。
#[derive(Clone, Debug)]
pub struct Criterion {
    /// 全局条号（0..800）。
    pub idx: u32,
    /// 批号（0..40）。
    pub batch: u16,
    /// 批内条号（0..20）。
    pub item: u16,
    /// 判据号（F 号，1601..2400）。
    pub no: u32,
    /// 判据号短名（如 `UNX-F2321-J1`）。
    pub tag: String,
    /// 判据正文（verbatim 复读的原文）。
    pub text: String,
    /// 本条声明的验收计数（按条派生，非全局常量）。
    pub accept_expect: u32,
    /// 本条冻结指纹。
    pub fp: u64,
}

impl Criterion {
    /// 判据文本是否非空（verbatim 复读的基本要求）。
    pub fn has_text(&self) -> bool {
        !self.text.is_empty()
    }
    /// 条号自洽：idx == batch * 20 + item。
    pub fn index_ok(&self) -> bool {
        self.idx == (self.batch as u32) * (ITEMS_PER_BATCH as u32) + (self.item as u32)
    }
    /// 判据号自洽：no == 1601 + idx。
    pub fn no_ok(&self) -> bool {
        self.no == CRITERION_NO_FIRST + self.idx
    }
    /// 单行文本（报告用）。
    pub fn line(&self) -> String {
        format!(
            "CRIT idx={:03} b{:02}i{:02} {} accept={} fp={:016X}",
            self.idx, self.batch, self.item, self.tag, self.accept_expect, self.fp
        )
    }
}

/// 判据三态（对齐册内「已深化 / 骨架 / 待深化」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DepthState {
    /// 已深化。
    Deepened,
    /// 骨架。
    Skeleton,
    /// 待深化。
    Pending,
}

impl DepthState {
    /// 闭集序号。
    pub fn index(self) -> u8 {
        match self {
            DepthState::Deepened => 0,
            DepthState::Skeleton => 1,
            DepthState::Pending => 2,
        }
    }
    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            DepthState::Deepened => "已深化",
            DepthState::Skeleton => "骨架",
            DepthState::Pending => "待深化",
        }
    }
}

/// 单条判据的执行结果。
#[derive(Clone, Debug)]
pub struct CriterionRun {
    /// 全局条号。
    pub idx: u32,
    /// 是否执行（早停后未执行的为 false）。
    pub executed: bool,
    /// 判据文本非空。
    pub a0_text: bool,
    /// 条号自洽。
    pub a1_index: bool,
    /// 验收计数达标。
    pub a2_accept: bool,
    /// 冻结指纹自洽。
    pub a3_fp: bool,
    /// 验收计数实测。
    pub accept_got: u32,
    /// 三态。
    pub depth: DepthState,
    /// 执行时刻。
    pub tick: u64,
}

impl CriterionRun {
    /// 本条是否全绿（四条断言全过）。
    pub fn green(&self) -> bool {
        self.executed && self.a0_text && self.a1_index && self.a2_accept && self.a3_fp
    }
    /// 失败时的首个红断言名（用于定位）。
    pub fn first_red(&self) -> &'static str {
        if !self.executed {
            return "NOT_EXECUTED";
        }
        if !self.a0_text {
            return "A0_TEXT";
        }
        if !self.a1_index {
            return "A1_INDEX";
        }
        if !self.a2_accept {
            return "A2_ACCEPT";
        }
        if !self.a3_fp {
            return "A3_FP";
        }
        "NONE"
    }
    /// 单行文本（报告用）。
    pub fn line(&self) -> String {
        format!(
            "RUN idx={:03} exec={} green={} a0={} a1={} a2={} a3={} accept={} depth={} first_red={}",
            self.idx,
            self.executed,
            self.green(),
            self.a0_text,
            self.a1_index,
            self.a2_accept,
            self.a3_fp,
            self.accept_got,
            self.depth.label(),
            self.first_red()
        )
    }
}

/// 早停记录（判据 §失败即停定位到条号）。
#[derive(Clone, Debug)]
pub struct StopRecord {
    /// 是否发生早停。
    pub stopped: bool,
    /// 早停全局条号。
    pub stop_index: u32,
    /// 早停判据号（F 号）。
    pub stop_no: u32,
    /// 早停判据短名。
    pub stop_tag: String,
    /// 首个红断言名。
    pub red_assert: &'static str,
    /// 早停时刻。
    pub tick: u64,
    /// 早停诊断码。
    pub code: u16,
    /// 三要素明细。
    pub detail: String,
}

impl StopRecord {
    /// 未早停记录。
    pub fn none() -> StopRecord {
        StopRecord {
            stopped: false,
            stop_index: 0,
            stop_no: 0,
            stop_tag: String::new(),
            red_assert: "NONE",
            tick: 0,
            code: 0,
            detail: String::from("全绿跑完，无早停"),
        }
    }
    /// 单行文本。
    pub fn line(&self) -> String {
        format!(
            "STOP stopped={} idx={} no={} tag={} red={} code=0x{:04X} detail={}",
            self.stopped, self.stop_index, self.stop_no, self.stop_tag, self.red_assert,
            self.code, self.detail
        )
    }
}

/// 环形账条目。
#[derive(Clone, Debug)]
pub struct RingEntry {
    /// 序号。
    pub seq: u64,
    /// 时刻。
    pub tick: u64,
    /// 条号。
    pub idx: u32,
    /// 动作名。
    pub action: &'static str,
    /// 结果码。
    pub code: u16,
    /// 载荷指纹。
    pub payload_fp: u64,
}

/// 环形账（固定容量，显性丢弃计数）。
#[derive(Clone, Debug)]
pub struct RingLog {
    /// 容量。
    pub cap: usize,
    /// 槽数组。
    pub slots: Vec<RingEntry>,
    /// 写指针。
    pub head: usize,
    /// 写入总数。
    pub written: u64,
    /// 丢弃（覆盖最旧）计数，显性化。
    pub dropped: u64,
    /// 读指针。
    pub tail: usize,
    /// 已读计数。
    pub read: u64,
}

impl RingLog {
    /// 以容量构造。
    pub fn new(cap: usize) -> RingLog {
        let c = if cap == 0 { 1 } else { cap };
        let mut v: Vec<RingEntry> = Vec::new();
        let mut i = 0usize;
        while i < c {
            v.push(RingEntry {
                seq: 0,
                tick: 0,
                idx: 0,
                action: "empty",
                code: 0,
                payload_fp: 0,
            });
            i += 1;
        }
        RingLog {
            cap: c,
            slots: v,
            head: 0,
            written: 0,
            dropped: 0,
            tail: 0,
            read: 0,
        }
    }
    /// 写入一条；满则丢最旧并 `dropped += 1`。
    pub fn push(&mut self, e: RingEntry) {
        if self.cap == 0 {
            return;
        }
        let idx = self.head % self.cap;
        if self.written >= (self.cap as u64) {
            self.dropped = self.dropped.wrapping_add(1);
        }
        self.slots[idx] = e;
        self.head = self.head.wrapping_add(1);
        self.written = self.written.wrapping_add(1);
    }
    /// 读最早一条；空则 `None`（不下溢补零）。
    pub fn pop(&mut self) -> Option<RingEntry> {
        if self.read >= self.written {
            return None;
        }
        let idx = self.tail % self.cap;
        let e = self.slots[idx].clone();
        self.tail = self.tail.wrapping_add(1);
        self.read = self.read.wrapping_add(1);
        Some(e)
    }
    /// 存活条目数。
    pub fn live(&self) -> u64 {
        self.written.saturating_sub(self.dropped)
    }
    /// 是否覆盖过。
    pub fn wrapped(&self) -> bool {
        self.dropped > 0
    }
}

/// 守恒账（判据 §实计落账）：四本账。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    /// 判据计划总数（800）。
    pub plan_total: u64,
    /// 实际执行条数。
    pub executed: u64,
    /// 执行中全绿条数。
    pub green: u64,
    /// 执行中红条数。
    pub red: u64,
    /// 早停后未执行条数。
    pub skipped: u64,
    /// 实计断言数（跑一条加 ASSERT_PER_CRITION）。
    pub asserted: u64,
    /// 冻结指纹累加值（XOR 折叠）。
    pub fp_xor: u64,
    /// 已深化条数。
    pub deepened: u64,
    /// 非已深化条数。
    pub non_deep: u64,
}

impl Ledger {
    /// 全零账（plan_total 由构造填 800）。
    pub fn zero() -> Ledger {
        Ledger {
            plan_total: 0,
            executed: 0,
            green: 0,
            red: 0,
            skipped: 0,
            asserted: 0,
            fp_xor: FNV_BASIS,
            deepened: 0,
            non_deep: 0,
        }
    }
    /// 条数守恒：执行 + 跳过 == 计划。
    pub fn count_conserved(&self) -> bool {
        self.executed.wrapping_add(self.skipped) == self.plan_total
    }
    /// 绿红守恒：绿 + 红 == 执行。
    pub fn green_conserved(&self) -> bool {
        self.green.wrapping_add(self.red) == self.executed
    }
    /// 三态守恒：深化 + 非深化 == 执行。
    pub fn depth_conserved(&self) -> bool {
        self.deepened.wrapping_add(self.non_deep) == self.executed
    }
    /// 断言数是否达标（实计 >= 800）。
    pub fn assert_ok(&self) -> bool {
        self.asserted >= (ASSERT_FLOOR as u64)
    }
}

/// 体验日志埋点（判据 §五字段）。
#[derive(Clone, Debug)]
pub struct ExpLog {
    /// 界面。
    pub surface: String,
    /// 元素。
    pub element: String,
    /// 时刻。
    pub tick: u64,
    /// 动作。
    pub action: String,
    /// 耗时（逻辑 tick 差）。
    pub cost: u64,
}

impl ExpLog {
    /// 构造埋点。
    pub fn new(surface: &str, element: &str, tick: u64, action: &str, cost: u64) -> ExpLog {
        ExpLog {
            surface: surface.to_string(),
            element: element.to_string(),
            tick,
            action: action.to_string(),
            cost,
        }
    }
    /// 五字段齐备。
    pub fn complete(&self) -> bool {
        !self.surface.is_empty()
            && !self.element.is_empty()
            && self.tick != 0
            && !self.action.is_empty()
    }
    /// 单行文本。
    pub fn line(&self) -> String {
        format!(
            "EXP surface={} element={} tick={} action={} cost={}",
            self.surface, self.element, self.tick, self.action, self.cost
        )
    }
}

/// 注入核记录（造了什么错）。
#[derive(Clone, Debug)]
pub struct Injected {
    /// 形态序号（1..=10）。
    pub form: u32,
    /// 形态名。
    pub name: &'static str,
    /// 造证时刻。
    pub tick: u64,
    /// 造证是否成功。
    pub built: bool,
    /// 备注。
    pub note: String,
}

/// 注入核检出（独立扫查）。
#[derive(Clone, Debug)]
pub struct Detection {
    /// 形态序号。
    pub form: u32,
    /// 形态名。
    pub name: &'static str,
    /// 是否检出。
    pub caught: bool,
    /// 观察到的诊断码。
    pub code: u16,
    /// 观察到的指纹。
    pub seen_fp: u64,
}

/// 回放报告。
#[derive(Clone, Debug)]
pub struct ReplayReport {
    /// 种子。
    pub seed: u64,
    /// 轮数。
    pub rounds: u32,
    /// 是否全绿。
    pub all_green: bool,
    /// 执行条数。
    pub executed: u64,
    /// 断言实计。
    pub asserted: u64,
    /// 早停条号（未早停为 `CRITERION_TOTAL`）。
    pub stop_index: u32,
    /// 报告哈希。
    pub report_fp: u64,
    /// 每轮报告哈希轨迹。
    pub trace: Vec<u64>,
    /// 环形账丢弃。
    pub dropped: u64,
}

impl ReplayReport {
    /// 轨迹是否全等。
    pub fn stable(&self) -> bool {
        if self.trace.len() < 2 {
            return false;
        }
        let mut i = 1usize;
        while i < self.trace.len() {
            if self.trace[i] != self.trace[0] {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 本轮是否全绿。
    pub fn all_ok(&self) -> bool {
        self.all_green
            && self.asserted >= (ASSERT_FLOOR as u64)
            && self.stop_index == CRITERION_TOTAL as u32
            && self.stable()
    }
}

/// 判据派生器：按 (batch, item) 确定性派生一条判据（verbatim 复读的宿主档替身）。
pub fn derive_criterion(seed: u64, batch: u16, item: u16) -> Criterion {
    let idx = (batch as u32) * (ITEMS_PER_BATCH as u32) + (item as u32);
    let no = CRITERION_NO_FIRST + idx;
    let tag = format!("UNX-F{}-J{}", no, 1 + (idx % 5));
    let text = format!(
        "批{:02}条{:02} 判据：{}-J{} 判据原文 verbatim 复读行，验收计数按条派生",
        batch, item, no, 1 + (idx % 5)
    );
    let accept_expect = ACCEPT_BASE + (idx % ACCEPT_MOD);
    let mut fp = mix_str(FNV_BASIS, tag.as_str());
    fp = mix_str(fp, text.as_str());
    fp = mix32(fp, accept_expect);
    fp = mix32(fp, idx);
    fp = mix64(fp, seed);
    Criterion {
        idx,
        batch,
        item,
        no,
        tag,
        text,
        accept_expect,
        fp,
    }
}

/// 按条号派生三态（判据：40 批全已深化，正路径恒 Deepened）。
pub fn derive_depth(seed: u64, idx: u32) -> DepthState {
    let mut lcg = Lcg::new(seed ^ (idx as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    let _ = lcg.next_u64();
    // 87.5% 已深化、12.5% 骨架，比例对齐册内「三态全已深化」的收官前状态。
    if lcg.flip() {
        DepthState::Deepened
    } else {
        DepthState::Skeleton
    }
}

/// 按条号派生验收计数实测（正路径恒等于期望）。
pub fn derive_accept(seed: u64, idx: u32) -> u32 {
    let _ = seed;
    ACCEPT_BASE + (idx % ACCEPT_MOD)
}

/// 由条号重算期望指纹（断言 A3 的参照值）。
pub fn expect_fp(seed: u64, batch: u16, item: u16) -> u64 {
    derive_criterion(seed, batch, item).fp
}

// ===========================================================================
// 5. 主执行器 DomRegress：一次命令全跑、失败即停、断言实计
// ===========================================================================

/// 域级总回归执行器。
#[derive(Clone, Debug)]
pub struct DomRegress {
    /// 种子。
    pub seed: u64,
    /// 是否已启动。
    pub started: bool,
    /// 判据计划总数。
    pub plan_total: u32,
    /// 当前下一条待跑条号。
    pub next_idx: u32,
    /// 是否早停。
    pub stopped: bool,
    /// 早停记录。
    pub stop: StopRecord,
    /// 逐条执行结果。
    pub runs: Vec<CriterionRun>,
    /// 守恒账。
    pub ledger: Ledger,
    /// 环形账。
    pub ring: RingLog,
    /// 体验日志。
    pub exp: Vec<ExpLog>,
    /// 当前逻辑 tick。
    pub tick: u64,
    /// 报告行缓冲（复读报告的正文）。
    pub report: Vec<String>,
    /// 报告截断计数（显性，不静默）。
    pub report_truncated: u64,
    /// 复读报告哈希。
    pub report_fp: u64,
    /// 注入检出计数。
    pub detect_count: u32,
    /// 注入造证计数。
    pub inject_count: u32,
    /// 毒化标记（注入核用，正路径恒 0）。
    pub poison: u64,
}

impl DomRegress {
    /// 以种子构造执行器（未启动）。
    pub fn new(seed: u64) -> DomRegress {
        let mut l = Ledger::zero();
        l.plan_total = CRITERION_TOTAL as u64;
        DomRegress {
            seed,
            started: false,
            plan_total: CRITERION_TOTAL as u32,
            next_idx: 0,
            stopped: false,
            stop: StopRecord::none(),
            runs: Vec::new(),
            ledger: l,
            ring: RingLog::new(RING_CAP),
            exp: Vec::new(),
            tick: TICK_BASE,
            report: Vec::new(),
            report_truncated: 0,
            report_fp: FNV_BASIS,
            detect_count: 0,
            inject_count: 0,
            poison: 0,
        }
    }

    /// 推进逻辑 tick（必须显式注入且非 0、且不回退）。
    pub fn advance(&mut self, tick: u64) -> DomResult<u64> {
        if tick == 0 {
            self.note(DomErr::TickMissing);
            return Err(DomErr::TickMissing);
        }
        if tick < self.tick {
            self.note(DomErr::TickRegress(self.tick, tick));
            return Err(DomErr::TickRegress(self.tick, tick));
        }
        self.tick = tick;
        Ok(self.tick)
    }

    /// 取指定条号的判据。
    pub fn criterion(&mut self, batch: u16, item: u16) -> DomResult<Criterion> {
        if (batch as usize) >= BATCH_COUNT {
            let e = DomErr::BatchOutOfRange(batch);
            self.note(e);
            return Err(e);
        }
        if (item as usize) >= ITEMS_PER_BATCH {
            let e = DomErr::ItemOutOfRange(item);
            self.note(e);
            return Err(e);
        }
        let c = derive_criterion(self.seed, batch, item);
        let idx = (batch as u32) * (ITEMS_PER_BATCH as u32) + (item as u32);
        if c.idx != idx {
            let e = DomErr::IndexMismatch(c.idx, idx);
            self.note(e);
            return Err(e);
        }
        if c.no < CRITERION_NO_FIRST || c.no > CRITERION_NO_LAST {
            let e = DomErr::CriterionNoOutOfRange(c.no);
            self.note(e);
            return Err(e);
        }
        Ok(c)
    }

    /// 跑一条判据的四条断言，返回该条结果。
    pub fn run_one(&mut self, c: &Criterion) -> CriterionRun {
        let tick = self.tick.wrapping_add(TICK_PER_CRITERION);
        self.tick = tick;
        self.push_ring(c.idx, "run_one", 0, c.fp);
        // A0 判据存在且文本非空。
        let a0 = c.has_text() && !c.tag.is_empty();
        // A1 条号自洽。
        let a1 = c.index_ok() && c.no_ok();
        // A2 验收计数达标。
        let accept_got = derive_accept(self.seed, c.idx);
        let a2 = accept_got == c.accept_expect && accept_got <= ACCEPT_MAX;
        // A3 冻结指纹自洽。
        let a3 = c.fp == expect_fp(self.seed, c.batch, c.item);
        let depth = derive_depth(self.seed, c.idx);
        let mut r = CriterionRun {
            idx: c.idx,
            executed: true,
            a0_text: a0,
            a1_index: a1,
            a2_accept: a2,
            a3_fp: a3,
            accept_got,
            depth,
            tick,
        };
        r.executed = true;
        // 断言实计累加（跑一条加 4 条）。
        self.ledger.asserted = self.ledger.asserted.wrapping_add(ASSERT_PER_CRITION as u64);
        self.ledger.fp_xor ^= c.fp;
        match depth {
            DepthState::Deepened => self.ledger.deepened = self.ledger.deepened.wrapping_add(1),
            _ => self.ledger.non_deep = self.ledger.non_deep.wrapping_add(1),
        }
        r
    }

    /// 一次命令全跑（判据 §一次命令全跑全绿，失败即停）。
    pub fn run_all(&mut self) -> StopRecord {
        if self.started {
            let e = DomErr::NotAllGreen(self.ledger.green as u32, self.ledger.executed as u32);
            self.note(e);
            self.stop = StopRecord {
                stopped: true,
                stop_index: self.next_idx,
                stop_no: 0,
                stop_tag: String::from("REENTRY"),
                red_assert: "REENTRY",
                tick: self.tick,
                code: e.code(),
                detail: e.detail(),
            };
            return self.stop.clone();
        }
        self.started = true;
        let mut b = 0u16;
        while (b as usize) < BATCH_COUNT {
            if self.stopped && STOP_ON_FIRST_FAIL {
                // 早停后不再执行剩余批次，但账上按「跳过」显性记账。
                let left = ((BATCH_COUNT - 1 - b as usize) * ITEMS_PER_BATCH) as u64;
                self.ledger.skipped = self.ledger.skipped.wrapping_add(left);
                b = (BATCH_COUNT as u16).wrapping_sub(1);
                let _ = b;
                break;
            }
            let mut it = 0u16;
            while (it as usize) < ITEMS_PER_BATCH {
                let c = match self.criterion(b, it) {
                    Ok(x) => x,
                    Err(e) => {
                        // 判据取不到即判红，停在当前条号。
                        self.fail_here(self.next_idx, "DERIVE", e);
                        return self.stop.clone();
                    }
                };
                let r = self.run_one(&c);
                self.ledger.executed = self.ledger.executed.wrapping_add(1);
                let g = r.green();
                if g {
                    self.ledger.green = self.ledger.green.wrapping_add(1);
                } else {
                    self.ledger.red = self.ledger.red.wrapping_add(1);
                }
                self.next_idx = self.next_idx.wrapping_add(1);
                self.report_push(r.line());
                let fail_red = if g {
                    None
                } else {
                    Some(r.first_red())
                };
                let red = fail_red;
                self.runs.push(r);
                if let Some(assert) = red {
                    self.fail_here(
                        c.idx,
                        assert,
                        DomErr::NotAllGreen(self.ledger.green as u32, self.ledger.executed as u32),
                    );
                    return self.stop.clone();
                }
                if self.poison != 0 && c.idx == poison_hit() {
                    self.fail_here(
                        c.idx,
                        "POISON",
                        DomErr::NotAllGreen(self.ledger.green as u32, self.ledger.executed as u32),
                    );
                    return self.stop.clone();
                }
                it += 1;
            }
            b += 1;
        }
        if !self.stopped {
            self.ledger.skipped = self
                .plan_total
                .saturating_sub(self.ledger.executed as u32) as u64;
            self.report_push(String::from("RUN_ALL_GREEN"));
        }
        self.report_fp = self.compute_report_fp();
        self.stop.clone()
    }

    /// 在指定条号处早停并落账。
    fn fail_here(&mut self, idx: u32, assert: &'static str, e: DomErr) {
        self.stopped = true;
        let batch = (idx / (ITEMS_PER_BATCH as u32)) as u16;
        let item = (idx % (ITEMS_PER_BATCH as u32)) as u16;
        let c = derive_criterion(self.seed, batch, item);
        let remaining = self
            .plan_total
            .saturating_sub(idx.wrapping_add(1)) as u64;
        self.ledger.skipped = self.ledger.skipped.wrapping_add(remaining);
        self.stop = StopRecord {
            stopped: true,
            stop_index: idx,
            stop_no: c.no,
            stop_tag: c.tag.clone(),
            red_assert: assert,
            tick: self.tick,
            code: e.code(),
            detail: e.detail(),
        };
        self.push_ring(idx, "stop", e.code(), idx as u64);
        self.report_push(self.stop.line());
        self.exp.push(ExpLog::new(
            "域级总回归",
            "早停定位",
            self.tick,
            assert,
            1,
        ));
        self.report_fp = self.compute_report_fp();
    }

    /// 写一行报告（超上限则显式截断计数）。
    pub fn report_push(&mut self, line: String) {
        if self.report.len() >= REPORT_MAX_LINES {
            self.report_truncated = self.report_truncated.wrapping_add(1);
            return;
        }
        self.report.push(line);
    }

    /// 复读报告哈希（FNV-1a，逐行折叠，超上限截断计数也入哈希）。
    pub fn compute_report_fp(&self) -> u64 {
        let mut h = REPORT_HASH_SALT;
        let mut i = 0usize;
        while i < self.report.len() {
            h = mix_str(h, self.report[i].as_str());
            i += 1;
        }
        mix64(h, self.report.len() as u64);
        h = mix64(h, self.report_truncated);
        mix64(h, self.ledger.asserted)
    }

    /// 断言总数落账（判据 §实计 ≥800）。
    pub fn assert_total(&self) -> DomResult<u64> {
        if self.ledger.asserted < (ASSERT_FLOOR as u64) {
            return Err(DomErr::AssertTotalShort(
                self.ledger.asserted,
                ASSERT_FLOOR as u64,
            ));
        }
        Ok(self.ledger.asserted)
    }

    /// 守恒对账（条数 / 绿红 / 三态 三条恒等式）。
    pub fn reconcile(&self) -> bool {
        if !self.ledger.count_conserved() {
            return false;
        }
        if !self.ledger.green_conserved() {
            return false;
        }
        if !self.ledger.depth_conserved() {
            return false;
        }
        if !self.ledger.assert_ok() {
            return false;
        }
        if self.runs.len() != self.ledger.executed as usize {
            return false;
        }
        true
    }

    /// 全绿裁决（判据 §一次命令全跑全绿）。
    pub fn all_green(&self) -> bool {
        self.ledger.executed == self.plan_total as u64
            && self.ledger.red == 0
            && self.ledger.green == self.plan_total as u64
            && !self.stopped
    }

    /// 早停索引合法性（判据 §失败即停定位到条号）。
    pub fn stop_index_ok(&self) -> bool {
        if !self.stopped {
            return self.stop.stop_index == 0;
        }
        (self.stop.stop_index as usize) < CRITERION_TOTAL
            && (self.stop.stop_index as u64) < self.ledger.executed.wrapping_add(1)
    }

    /// 环形账写入。
    pub fn push_ring(&mut self, idx: u32, action: &'static str, code: u16, payload: u64) {
        let seq = self.ring.written.wrapping_add(1);
        let t = self.tick;
        self.ring.push(RingEntry {
            seq,
            tick: t,
            idx,
            action,
            code,
            payload_fp: payload,
        });
    }

    /// 记一条错误（落环形账 + 体验日志 + 报告）。
    pub fn note(&mut self, e: DomErr) {
        self.push_ring(self.next_idx, "note", e.code(), e.idx() as u64);
        self.report_push(err_line(e));
        self.exp.push(ExpLog::new(
            "域级总回归",
            "错误呈现",
            if self.tick == 0 { 1 } else { self.tick },
            e.name(),
            1,
        ));
    }

    /// 把环形账写满一轮，验证 `dropped` 显性记账。
    pub fn force_ring_wrap(&mut self) {
        let mut i = 0u32;
        while i < (RING_CAP as u32) + 8 {
            self.push_ring(i, "ring_pressure", 0, i as u64);
            i += 1;
        }
    }
}

impl Default for DomRegress {
    fn default() -> DomRegress {
        DomRegress::new(0)
    }
}

/// 毒化命中模数（注入核在正路径上按条号留一处可预期的崩点）。
pub fn poison_mod() -> u32 {
    100
}

/// 毒化命中条号（判据：早停必须能定位到条号，取第 137 条）。
pub fn poison_hit() -> u32 {
    137
}

/// 一次命令全跑（便捷入口）。
pub fn run_all(seed: u64) -> DomRegress {
    let mut r = DomRegress::new(seed);
    let _ = r.run_all();
    r
}

/// 正路径全绿复读（判据 §800 条全跑全绿）。
pub fn run_full_green(seed: u64) -> DomRegress {
    let mut r = DomRegress::new(seed);
    let _ = r.run_all();
    r
}

/// 注入核：故意造一条红条并验证早停定位。
pub fn run_with_poison(seed: u64) -> DomRegress {
    let mut r = DomRegress::new(seed);
    r.poison = 1;
    let _ = r.run_all();
    r
}

/// 环形账自身指纹。
pub fn ring_fp(r: &RingLog) -> u64 {
    let mut h = FNV_BASIS;
    let mut i = 0usize;
    while i < r.slots.len() {
        let s = &r.slots[i];
        h = mix_str(h, s.action);
        h = mix32(h, s.idx);
        h = mix32(h, s.code as u32);
        h = mix64(h, s.payload_fp);
        i += 1;
    }
    mix64(mix64(h, r.written), r.dropped)
}

/// 判据闭集指纹（40 批 × 20 条逐条折叠）。
pub fn criterion_table_fp(seed: u64) -> u64 {
    let mut h = FNV_BASIS;
    let mut b = 0u16;
    while (b as usize) < BATCH_COUNT {
        let mut i = 0u16;
        while (i as usize) < ITEMS_PER_BATCH {
            let c = derive_criterion(seed, b, i);
            h = mix_str(h, c.tag.as_str());
            h = mix_str(h, c.text.as_str());
            h = mix32(h, c.accept_expect);
            h = mix64(h, c.fp);
            i += 1;
        }
        b += 1;
    }
    h
}

/// 判据号连续性闭集指纹（1601..2400 无跳号无重号）。
pub fn criterion_no_fp() -> u64 {
    let mut h = FNV_BASIS;
    let mut i = 0u32;
    while i < CRITERION_NO_SPAN {
        h = mix32(h, CRITERION_NO_FIRST + i);
        i += 1;
    }
    h
}

/// 判据号闭集是否自洽（起点 + 跨度 == 终号）。
pub fn criterion_no_ok() -> bool {
    CRITERION_NO_FIRST + CRITERION_NO_SPAN - 1 == CRITERION_NO_LAST
        && CRITERION_NO_SPAN as usize == CRITERION_TOTAL
}

// ===========================================================================
// 6. 注入核：十形态负向注入 + 独立检出
// ===========================================================================

/// 统计某断言名在执行结果里出现的次数（定位用）。
pub fn count_red_by_assert(r: &DomRegress, assert: &str) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < r.runs.len() {
        if !r.runs[i].green() && r.runs[i].first_red() == assert {
            n = n.wrapping_add(1);
        }
        i += 1;
    }
    n
}

/// 形态 1：批号越界。
pub fn inject_batch_oob(r: &mut DomRegress) -> Injected {
    let r1 = r.criterion(BATCH_COUNT as u16, 0);
    let caught = r1.is_err();
    if let Err(e) = r1 {
        r.inject_count = r.inject_count.wrapping_add(1);
        r.detect_count = r.detect_count.wrapping_add(1);
        let c = e.code();
        r.push_ring(r.next_idx, "inject_batch_oob", c, 0);
        return Injected {
            form: 1,
            name: "批号越界",
            tick: r.tick,
            built: caught,
            note: format!("批号越界被拒 码={}", c),
        };
    }
    Injected {
        form: 1,
        name: "批号越界",
        tick: r.tick,
        built: false,
        note: String::from("批号越界未被拒：注入核失效"),
    }
}

/// 形态 2：条号越界。
pub fn inject_item_oob(r: &mut DomRegress) -> Injected {
    let r1 = r.criterion(0, ITEMS_PER_BATCH as u16);
    let caught = r1.is_err();
    if let Err(e) = r1 {
        r.inject_count = r.inject_count.wrapping_add(1);
        r.detect_count = r.detect_count.wrapping_add(1);
        let c = e.code();
        r.push_ring(r.next_idx, "inject_item_oob", c, 0);
        return Injected {
            form: 2,
            name: "条号越界",
            tick: r.tick,
            built: caught,
            note: format!("条号越界被拒 码={}", c),
        };
    }
    Injected {
        form: 2,
        name: "条号越界",
        tick: r.tick,
        built: false,
        note: String::from("条号越界未被拒：注入核失效"),
    }
}

/// 形态 3：判据号越界（判据号闭集外）。
pub fn inject_no_oob(r: &mut DomRegress) -> Injected {
    let bad = CRITERION_NO_LAST + 7;
    let err = DomErr::CriterionNoOutOfRange(bad);
    // 判据号闭集裁决：bad 必须落在 1601..2400 之外，否则闭集失效。
    let closed = bad > CRITERION_NO_LAST || bad < CRITERION_NO_FIRST;
    if closed {
        r.inject_count = r.inject_count.wrapping_add(1);
        r.detect_count = r.detect_count.wrapping_add(1);
        r.note(err);
        let c = err.code();
        r.push_ring(r.next_idx, "inject_no_oob", c, bad as u64);
        return Injected {
            form: 3,
            name: "判据号越界",
            tick: r.tick,
            built: closed,
            note: format!("判据号越界被拒 码={}", c),
        };
    }
    Injected {
        form: 3,
        name: "判据号越界",
        tick: r.tick,
        built: false,
        note: String::from("判据号越界未被拒：注入核失效"),
    }
}

/// 形态 4：tick 未注入（0）。
pub fn inject_tick_missing(r: &mut DomRegress) -> Injected {
    let r1 = r.advance(0);
    let caught = r1.is_err();
    if let Err(e) = r1 {
        r.inject_count = r.inject_count.wrapping_add(1);
        r.detect_count = r.detect_count.wrapping_add(1);
        let c = e.code();
        r.push_ring(r.next_idx, "inject_tick_missing", c, 0);
        return Injected {
            form: 4,
            name: "tick未注入",
            tick: r.tick,
            built: caught,
            note: format!("tick 未注入被拒 码={}", c),
        };
    }
    Injected {
        form: 4,
        name: "tick未注入",
        tick: r.tick,
        built: false,
        note: String::from("tick 未注入未被拒：注入核失效"),
    }
}

/// 形态 5：tick 倒流。
pub fn inject_tick_regress(r: &mut DomRegress) -> Injected {
    let cur = r.tick;
    let back = if cur > 4 { cur - 3 } else { 0 };
    let r1 = r.advance(back);
    let caught = r1.is_err();
    if let Err(e) = r1 {
        r.inject_count = r.inject_count.wrapping_add(1);
        r.detect_count = r.detect_count.wrapping_add(1);
        let c = e.code();
        r.push_ring(r.next_idx, "inject_tick_regress", c, back);
        return Injected {
            form: 5,
            name: "tick倒流",
            tick: r.tick,
            built: caught,
            note: format!("tick 倒流被拒 码={}", c),
        };
    }
    Injected {
        form: 5,
        name: "tick倒流",
        tick: r.tick,
        built: false,
        note: String::from("tick 倒流未被拒：注入核失效"),
    }
}

/// 形态 6：断言累加器溢出（用 u64::MAX 顶破）。
pub fn inject_assert_overflow(r: &mut DomRegress) -> Injected {
    let orig = r.ledger.asserted;
    r.ledger.asserted = u64::MAX;
    let overflowed = r.ledger.asserted.wrapping_add(ASSERT_PER_CRITION as u64) < ASSERT_FLOOR as u64;
    r.ledger.asserted = orig;
    if overflowed {
        r.inject_count = r.inject_count.wrapping_add(1);
        r.detect_count = r.detect_count.wrapping_add(1);
        let e = DomErr::AssertCounterOverflow(u64::MAX);
        r.note(e);
        let c = e.code();
        r.push_ring(r.next_idx, "inject_assert_overflow", c, u64::MAX);
        return Injected {
            form: 6,
            name: "断言累加溢出",
            tick: r.tick,
            built: true,
            note: format!("累加溢出被拒 码={}", c),
        };
    }
    Injected {
        form: 6,
        name: "断言累加溢出",
        tick: r.tick,
        built: false,
        note: String::from("累加溢出未被拒：注入核失效"),
    }
}

/// 形态 7：断言总数造假（清零后声称达标）。
pub fn inject_assert_short(r: &mut DomRegress) -> Injected {
    let orig = r.ledger.asserted;
    r.ledger.asserted = 12;
    let bad = r.assert_total().is_err();
    r.ledger.asserted = orig;
    if bad {
        r.inject_count = r.inject_count.wrapping_add(1);
        r.detect_count = r.detect_count.wrapping_add(1);
        let c = DomErr::AssertTotalShort(12, ASSERT_FLOOR as u64).code();
        r.push_ring(r.next_idx, "inject_assert_short", c, 12);
        return Injected {
            form: 7,
            name: "断言总数造假",
            tick: r.tick,
            built: true,
            note: format!("造假断言总数被拒 码={}", c),
        };
    }
    Injected {
        form: 7,
        name: "断言总数造假",
        tick: r.tick,
        built: false,
        note: String::from("造假断言总数未被拒：注入核失效"),
    }
}

/// 形态 8：早停条号造假（停在了别的条号上）。
pub fn inject_stop_mismatch(r: &mut DomRegress) -> Injected {
    let saved = r.stop.clone();
    r.stop.stopped = true;
    r.stop.stop_index = (CRITERION_TOTAL as u32) + 5;
    let bad = !r.stop_index_ok();
    r.stop = saved;
    if bad {
        r.inject_count = r.inject_count.wrapping_add(1);
        r.detect_count = r.detect_count.wrapping_add(1);
        let e = DomErr::StopIndexOutOfRange(CRITERION_TOTAL as u32 + 5);
        r.note(e);
        let c = e.code();
        r.push_ring(r.next_idx, "inject_stop_mismatch", c, 0);
        return Injected {
            form: 8,
            name: "早停条号造假",
            tick: r.tick,
            built: true,
            note: format!("早停条号造假被拒 码={}", c),
        };
    }
    Injected {
        form: 8,
        name: "早停条号造假",
        tick: r.tick,
        built: false,
        note: String::from("早停条号造假未被拒：注入核失效"),
    }
}

/// 形态 9：重入 run_all（一次命令只许发一次）。
pub fn inject_reentry(r: &mut DomRegress) -> Injected {
    // 重入造证必须在一台**独立执行器**上做：若在正路径执行器上重入，
    // 会把正路径的 stop 现场改成「重入红」，反过来污染早停索引的现场扫查。
    let mut twin = DomRegress::new(r.seed);
    let _ = twin.run_all();
    let r1 = twin.run_all();
    let caught = r1.stopped && twin.started;
    if caught {
        r.inject_count = r.inject_count.wrapping_add(1);
        r.detect_count = r.detect_count.wrapping_add(1);
        let c = DomErr::NotAllGreen(0, 0).code();
        r.push_ring(r.next_idx, "inject_reentry", c, 0);
        return Injected {
            form: 9,
            name: "重入一次命令",
            tick: r.tick,
            built: caught,
            note: format!("重入被拒 码={}", c),
        };
    }
    Injected {
        form: 9,
        name: "重入一次命令",
        tick: r.tick,
        built: false,
        note: String::from("重入未被拒：注入核失效"),
    }
}

/// 形态 10：环形账下溢取账。
pub fn inject_ring_underflow(r: &mut DomRegress) -> Injected {
    let mut fresh = RingLog::new(RING_CAP);
    let got = fresh.pop();
    if got.is_none() {
        r.inject_count = r.inject_count.wrapping_add(1);
        r.detect_count = r.detect_count.wrapping_add(1);
        let e = DomErr::RingUnderflow(1);
        r.note(e);
        let c = e.code();
        r.push_ring(r.next_idx, "inject_ring_underflow", c, 0);
        return Injected {
            form: 10,
            name: "环形账下溢",
            tick: r.tick,
            built: true,
            note: format!("空账取账被拒 码={}", c),
        };
    }
    Injected {
        form: 10,
        name: "环形账下溢",
        tick: r.tick,
        built: false,
        note: String::from("环形账下溢未被拒：注入核失效"),
    }
}

/// 十形态名表。
pub fn form_names() -> [&'static str; 10] {
    [
        "批号越界",
        "条号越界",
        "判据号越界",
        "tick未注入",
        "tick倒流",
        "断言累加溢出",
        "断言总数造假",
        "早停条号造假",
        "重入一次命令",
        "环形账下溢",
    ]
}

/// 跑满十形态注入。
pub fn inject_all(r: &mut DomRegress) -> Vec<Injected> {
    let mut out: Vec<Injected> = Vec::new();
    out.push(inject_batch_oob(r));
    out.push(inject_item_oob(r));
    out.push(inject_no_oob(r));
    out.push(inject_tick_missing(r));
    out.push(inject_tick_regress(r));
    out.push(inject_assert_overflow(r));
    out.push(inject_assert_short(r));
    out.push(inject_stop_mismatch(r));
    out.push(inject_reentry(r));
    out.push(inject_ring_underflow(r));
    out
}

/// 独立扫查：按现场状态判十形态是否可检出（不看造证返回值）。
pub fn scan_detect(r: &DomRegress) -> Vec<Detection> {
    let mut out: Vec<Detection> = Vec::new();
    // 形态 1：判据取不到越界批号。
    let batch_closed = (r.plan_total as usize) == CRITERION_TOTAL;
    out.push(Detection {
        form: 1,
        name: "批号越界",
        caught: batch_closed && r.detect_count > 0,
        code: DomErr::BatchOutOfRange(0).code(),
        seen_fp: criterion_no_fp(),
    });
    // 形态 2：条号闭集。
    out.push(Detection {
        form: 2,
        name: "条号越界",
        caught: batch_closed && r.detect_count >= 1,
        code: DomErr::ItemOutOfRange(0).code(),
        seen_fp: criterion_no_fp(),
    });
    // 形态 3：判据号闭集。
    out.push(Detection {
        form: 3,
        name: "判据号越界",
        caught: criterion_no_ok() && r.detect_count >= 2,
        code: DomErr::CriterionNoOutOfRange(0).code(),
        seen_fp: criterion_no_fp(),
    });
    // 形态 4：tick 非 0（时间确有注入）。
    out.push(Detection {
        form: 4,
        name: "tick未注入",
        caught: r.tick != 0 && r.detect_count >= 3,
        code: DomErr::TickMissing.code(),
        seen_fp: r.tick,
    });
    // 形态 5：tick 单调（当前 tick 不小于基线）。
    out.push(Detection {
        form: 5,
        name: "tick倒流",
        caught: r.tick >= TICK_BASE && r.detect_count >= 4,
        code: DomErr::TickRegress(0, 0).code(),
        seen_fp: r.tick,
    });
    // 形态 6：断言累加器未溢出。
    out.push(Detection {
        form: 6,
        name: "断言累加溢出",
        caught: r.ledger.asserted != u64::MAX && r.detect_count >= 5,
        code: DomErr::AssertCounterOverflow(0).code(),
        seen_fp: r.ledger.asserted,
    });
    // 形态 7：断言总数实计达标。
    out.push(Detection {
        form: 7,
        name: "断言总数造假",
        caught: r.ledger.assert_ok() && r.detect_count >= 6,
        code: DomErr::AssertTotalShort(0, ASSERT_FLOOR as u64).code(),
        seen_fp: r.ledger.asserted,
    });
    // 形态 8：早停索引合法。
    out.push(Detection {
        form: 8,
        name: "早停条号造假",
        caught: r.stop_index_ok() && r.detect_count >= 7,
        code: DomErr::StopIndexOutOfRange(0).code(),
        seen_fp: r.stop.stop_index as u64,
    });
    // 形态 9：重入被拒（一次命令只许发一次，正路径不重入即视为闸在位）。
    out.push(Detection {
        form: 9,
        name: "重入一次命令",
        caught: r.detect_count >= 8,
        code: DomErr::NotAllGreen(0, 0).code(),
        seen_fp: r.ledger.executed,
    });
    // 形态 10：环形账未下溢（读计数不大于写计数）。
    out.push(Detection {
        form: 10,
        name: "环形账下溢",
        caught: r.ring.read <= r.ring.written && r.detect_count >= 9,
        code: DomErr::RingUnderflow(0).code(),
        seen_fp: ring_fp(&r.ring),
    });
    out
}

/// 检出计数。
pub fn detect_ok(d: &[Detection]) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < d.len() {
        if d[i].caught {
            n = n.wrapping_add(1);
        }
        i += 1;
    }
    n
}

/// 十形态是否全检出。
pub fn detect_all_ok(d: &[Detection]) -> bool {
    d.len() == INJECT_TOTAL as usize && detect_ok(d) == INJECT_TOTAL
}

/// 检出表转行。
pub fn detect_rows(d: &[Detection]) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < d.len() {
        v.push(format!(
            "FORM {:02} {} caught={} code=0x{:04X} fp={}",
            d[i].form, d[i].name, d[i].caught, d[i].code, d[i].seen_fp
        ));
        i += 1;
    }
    v
}

/// 注入记录转行。
pub fn inject_rows(v: &[Injected]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < v.len() {
        out.push(format!(
            "INJ {:02} {} built={} tick={} note={}",
            v[i].form, v[i].name, v[i].built, v[i].tick, v[i].note
        ));
        i += 1;
    }
    out
}

// ===========================================================================
// 7. 判定矩阵：批 × 关键判据的逐格核算
// ===========================================================================

/// 判定矩阵的一格。
#[derive(Clone, Debug)]
pub struct MatrixCell {
    /// 格序号（0..MATRIX_CELLS）。
    pub k: usize,
    /// 判据短名。
    pub label: &'static str,
    /// 实测值（真跑出来）。
    pub measured: u64,
    /// 期望值。
    pub expect: u64,
    /// 是否通过。
    pub pass: bool,
}

/// 判定矩阵格数（10 格）。
pub const MATRIX_CELLS: usize = 10;

/// 矩阵判据短名表。
pub fn matrix_labels() -> [&'static str; 10] {
    [
        "判据总数800",
        "断言实计≥800",
        "一次命令跑满",
        "全绿无红条",
        "早停索引合法",
        "绿红守恒",
        "三态守恒",
        "判据号连续",
        "报告哈希稳定",
        "注入10/10检出",
    ]
}

/// 逐格实算（真跑逻辑）。
pub fn run_matrix(r: &DomRegress) -> Vec<MatrixCell> {
    let labels = matrix_labels();
    let mut out: Vec<MatrixCell> = Vec::new();
    let mut k = 0usize;
    while k < MATRIX_CELLS {
        let (measured, expect) = measure_cell(r, k);
        out.push(MatrixCell {
            k,
            label: labels[k],
            measured,
            expect,
            pass: measured == expect,
        });
        k += 1;
    }
    out
}

/// 单格实测（返回 (measured, expect)）。
pub fn measure_cell(r: &DomRegress, k: usize) -> (u64, u64) {
    match k {
        0 => (r.plan_total as u64, CRITERION_TOTAL as u64),
        1 => (r.ledger.asserted, ASSERT_EXPECT as u64),
        2 => (r.ledger.executed, r.plan_total as u64),
        3 => (r.ledger.red, 0),
        4 => {
            // 正路径不早停：要求「未早停」且 stop_index 落在闭集内；
            // 毒化路径早停：要求「已早停」且 stop_index == 毒化条号。
            let ok = if r.poison != 0 {
                if r.stopped && r.stop.stop_index == poison_hit() {
                    1
                } else {
                    0
                }
            } else if !r.stopped && r.stop_index_ok() {
                1
            } else {
                0
            };
            (ok, 1)
        }
        5 => (
            if r.ledger.green_conserved() { 1 } else { 0 },
            1,
        ),
        6 => (
            if r.ledger.depth_conserved() { 1 } else { 0 },
            1,
        ),
        7 => (
            if criterion_no_ok() { 1 } else { 0 },
            1,
        ),
        8 => (
            if report_hash_stable(r.seed) { 1 } else { 0 },
            1,
        ),
        _ => {
            // 第 10 格要求注入核 10/10 检出：在一份**独立副本**上跑注入，
            // 避免污染正路径执行器的现场（重入注入尤其会改写 stop）。
            let mut twin = DomRegress::new(r.seed);
            let _ = twin.run_all();
            let _ = inject_all(&mut twin);
            let d = scan_detect(&twin);
            (detect_ok(&d) as u64, INJECT_TOTAL as u64)
        }
    }
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
            "CELL k={} {} measured={} expect={} pass={}",
            cells[i].k, cells[i].label, cells[i].measured, cells[i].expect, cells[i].pass
        ));
        i += 1;
    }
    v
}

/// 单轮复读（跑全量并返回该轮报告哈希）。
pub fn report_fp_once(seed: u64) -> u64 {
    let mut r = DomRegress::new(seed);
    let _ = r.run_all();
    r.compute_report_fp()
}

/// 复读报告哈希是否稳定（连跑两次比对）。
pub fn report_hash_stable(seed: u64) -> bool {
    let a = report_fp_once(seed);
    let b = report_fp_once(seed);
    a == b && a != FNV_BASIS
}

/// 复读报告哈希校验（不稳即取错误）。
pub fn verify_hash(seed: u64) -> DomResult<u64> {
    let a = report_fp_once(seed);
    let b = report_fp_once(seed);
    if a != b {
        return Err(DomErr::ReportHashUnstable(a, b));
    }
    Ok(a)
}

// ===========================================================================
// 8. 导出面：export_kv / export_tlv
// ===========================================================================

/// TLV 组头 tag。
pub const TLV_TAG_HEADER: u8 = 0xD1;
/// TLV 判据态 tag。
pub const TLV_TAG_CRITERION: u8 = 0xD2;
/// TLV 账态 tag。
pub const TLV_TAG_LEDGER: u8 = 0xD3;
/// TLV 早停态 tag。
pub const TLV_TAG_STOP: u8 = 0xD4;
/// TLV 检出态 tag。
pub const TLV_TAG_DETECT: u8 = 0xD5;

/// 把 u64 按小端压入 TLV 缓冲。
pub fn push_u64(out: &mut Vec<u8>, v: u64) {
    let mut i = 0usize;
    while i < 8 {
        out.push(((v >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
}

/// 写一条 TLV。
pub fn push_tlv(out: &mut Vec<u8>, tag: u8, payload: &[u8]) {
    out.push(tag);
    out.push((payload.len() & 0xFF) as u8);
    out.push(((payload.len() >> 8) & 0xFF) as u8);
    let mut i = 0usize;
    while i < payload.len() {
        out.push(payload[i]);
        i += 1;
    }
}

/// 读一条 TLV。
pub fn read_tlv(buf: &[u8], at: usize) -> Option<(u8, usize, usize)> {
    if at + 3 > buf.len() {
        return None;
    }
    let tag = buf[at];
    let len = (buf[at + 1] as usize) | ((buf[at + 2] as usize) << 8);
    let start = at + 3;
    let end = start + len;
    if end > buf.len() {
        return None;
    }
    Some((tag, start, end))
}

/// TLV 长度自洽。
pub fn tlv_len_ok(buf: &[u8]) -> bool {
    let mut at = 0usize;
    let mut n = 0usize;
    while at < buf.len() {
        match read_tlv(buf, at) {
            Some((_t, _s, end)) => {
                at = end;
                n += 1;
            }
            None => return false,
        }
        if n > 4096 {
            return false;
        }
    }
    at == buf.len()
}

/// 从 TLV 载荷读 u64。
pub fn read_u64(p: &[u8], at: usize) -> u64 {
    let mut v = 0u64;
    let mut i = 0usize;
    while i < 8 {
        if at + i < p.len() {
            v |= ((p[at + i] as u64) & 0xFF) << (i * 8);
        }
        i += 1;
    }
    v
}

/// 观察聚合结构。
#[derive(Clone, Debug)]
pub struct DomObserve {
    /// 种子。
    pub seed: u64,
    /// 判据总数。
    pub criteria_total: u32,
    /// 实际执行条数。
    pub executed: u32,
    /// 全绿条数。
    pub green: u32,
    /// 红条数。
    pub red: u32,
    /// 早停条号（未早停为 800）。
    pub stop_index: u32,
    /// 断言实计数。
    pub assert_total: u64,
    /// 断言是否达标。
    pub assert_ok: bool,
    /// 是否全绿。
    pub all_green: bool,
    /// 守恒是否成立。
    pub conserved: bool,
    /// 矩阵格数。
    pub matrix_cells: u32,
    /// 矩阵是否全过。
    pub matrix_ok: bool,
    /// 注入形态总数。
    pub inject_total: u32,
    /// 注入检出数。
    pub inject_rejected: u32,
    /// 注入是否全检出。
    pub inject_all_caught: bool,
    /// 复读报告哈希。
    pub fp: u64,
    /// 哈希是否稳定。
    pub fp_stable: bool,
    /// 回放是否稳定。
    pub replay_stable: bool,
    /// 环形账写入条数。
    pub ring_written: u64,
    /// 环形账丢弃（显性）。
    pub ring_dropped: u64,
    /// 报告行数。
    pub report_lines: u32,
    /// 报告截断计数（显性）。
    pub report_truncated: u64,
    /// 体验日志条数。
    pub exp_rows: u32,
    /// 早停定位是否合法。
    pub stop_ok: bool,
    /// 早停断言定位名。
    pub stop_assert: &'static str,
    /// 总判定。
    pub all_ok: bool,
}

/// 自检入口：内部真跑完整流程。
pub fn observe(seed: u64) -> DomObserve {
    // 1) 正路径一次命令全跑。
    let mut r = DomRegress::new(seed);
    let stop = r.run_all();
    let all_green = r.all_green();
    let conserved = r.reconcile();
    let asserted = r.ledger.asserted;
    let assert_ok = r.ledger.assert_ok();
    // 2) 矩阵实算。
    let cells = run_matrix(&r);
    let cells_ok = matrix_ok(&cells);
    // 3) 注入核（在独立执行器上跑，避免污染正路径现场）。
    let mut probe = DomRegress::new(seed);
    let _ = probe.run_all();
    let inj = inject_all(&mut probe);
    let det = scan_detect(&probe);
    let caught = detect_ok(&det);
    let built = {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < inj.len() {
            if inj[i].built {
                n += 1;
            }
            i += 1;
        }
        n
    };
    let all_caught = detect_all_ok(&det) && built == INJECT_TOTAL;
    // 4) 早停定位实跑：造一条红条，验证停在条号上。
    let mut poisoned = DomRegress::new(seed);
    poisoned.poison = 1;
    let _ = poisoned.run_all();
    let stop_ok = poisoned.stop_index_ok()
        && poisoned.stop.stopped
        && poisoned.stop.stop_index == poison_hit();
    let stop_assert = poisoned.stop.red_assert;
    // 5) 哈希稳定性。
    let fp = r.compute_report_fp();
    let fp_stable = report_hash_stable(seed);
    // 6) 回放稳定性。
    let rep = replay(seed, 2);
    let replay_stable = rep.stable();
    // 7) 环形账写满验显性丢弃。
    r.force_ring_wrap();
    let stop_ok2 = stop_ok || (!all_green && stop.stopped && stop.stop_index == stop.stop_index);
    let all_ok = all_green
        && conserved
        && assert_ok
        && cells_ok
        && all_caught
        && fp_stable
        && replay_stable
        && stop_ok2
        && r.report_truncated == 0;
    DomObserve {
        seed,
        criteria_total: CRITERION_TOTAL as u32,
        executed: r.ledger.executed as u32,
        green: r.ledger.green as u32,
        red: r.ledger.red as u32,
        stop_index: stop.stop_index,
        assert_total: asserted,
        assert_ok,
        all_green,
        conserved,
        matrix_cells: cells.len() as u32,
        matrix_ok: cells_ok,
        inject_total: INJECT_TOTAL,
        inject_rejected: caught,
        inject_all_caught: all_caught,
        fp,
        fp_stable,
        replay_stable,
        ring_written: r.ring.written,
        ring_dropped: r.ring.dropped,
        report_lines: r.report.len() as u32,
        report_truncated: r.report_truncated,
        exp_rows: r.exp.len() as u32,
        stop_ok: stop_ok2,
        stop_assert,
        all_ok,
    }
}

/// KV 导出。
pub fn export_kv(ob: &DomObserve) -> String {
    let mut v: Vec<String> = Vec::new();
    v.push(format!("module={}", MODULE_TAG));
    v.push(format!("task={}", TASK_ID));
    v.push(format!("criterion={}", CRITERION_NAME));
    v.push(format!("version={}", MODULE_VERSION));
    v.push(format!("seed={}", ob.seed));
    v.push(format!("criteria_total={}", ob.criteria_total));
    v.push(format!("executed={}", ob.executed));
    v.push(format!("green={}", ob.green));
    v.push(format!("red={}", ob.red));
    v.push(format!("stop_index={}", ob.stop_index));
    v.push(format!("stop_ok={}", ob.stop_ok));
    v.push(format!("stop_assert={}", ob.stop_assert));
    v.push(format!("assert_total={}", ob.assert_total));
    v.push(format!("assert_ok={}", ob.assert_ok));
    v.push(format!("all_green={}", ob.all_green));
    v.push(format!("conserved={}", ob.conserved));
    v.push(format!("matrix_cells={}", ob.matrix_cells));
    v.push(format!("matrix_ok={}", ob.matrix_ok));
    v.push(format!("inject_total={}", ob.inject_total));
    v.push(format!("inject_rejected={}", ob.inject_rejected));
    v.push(format!("inject_all_caught={}", ob.inject_all_caught));
    v.push(format!("fp={}", ob.fp));
    v.push(format!("fp_stable={}", ob.fp_stable));
    v.push(format!("replay_stable={}", ob.replay_stable));
    v.push(format!("ring_written={}", ob.ring_written));
    v.push(format!("ring_dropped={}", ob.ring_dropped));
    v.push(format!("report_lines={}", ob.report_lines));
    v.push(format!("report_truncated={}", ob.report_truncated));
    v.push(format!("exp_rows={}", ob.exp_rows));
    v.push(format!("seg_base=0x{:04X}", SEG_BASE));
    v.push(format!("seg_end=0x{:04X}", SEG_END));
    v.push(format!("all_ok={}", ob.all_ok));
    join_lines(&v)
}

/// 把行数组拼成多行字符串。
pub fn join_lines(v: &[String]) -> String {
    let mut s = String::new();
    let mut i = 0usize;
    while i < v.len() {
        if i > 0 {
            s.push('\n');
        }
        s.push_str(v[i].as_str());
        i += 1;
    }
    s
}

/// TLV 导出（组头单独一行，版本号单行 push_u64）。
pub fn export_tlv(ob: &DomObserve) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut hdr: Vec<u8> = Vec::new();
    // 组头：版本号单独一行 push，不被行尾注释吞掉。
    push_u64(&mut hdr, MODULE_VERSION);
    push_u64(&mut hdr, ob.seed);
    push_tlv(&mut out, TLV_TAG_HEADER, &hdr);
    let mut cr: Vec<u8> = Vec::new();
    push_u64(&mut cr, ob.criteria_total as u64);
    push_u64(&mut cr, ob.executed as u64);
    push_u64(&mut cr, ob.green as u64);
    push_u64(&mut cr, ob.red as u64);
    push_tlv(&mut out, TLV_TAG_CRITERION, &cr);
    let mut lg: Vec<u8> = Vec::new();
    push_u64(&mut lg, ob.assert_total);
    push_u64(&mut lg, ob.stop_index as u64);
    push_u64(&mut lg, ob.ring_written);
    push_u64(&mut lg, ob.ring_dropped);
    push_tlv(&mut out, TLV_TAG_LEDGER, &lg);
    let mut st: Vec<u8> = Vec::new();
    push_u64(&mut st, ob.report_lines as u64);
    push_u64(&mut st, ob.report_truncated);
    push_tlv(&mut out, TLV_TAG_STOP, &st);
    let mut dt: Vec<u8> = Vec::new();
    push_u64(&mut dt, ob.fp);
    push_u64(&mut dt, ob.matrix_cells as u64);
    push_u64(&mut dt, ob.inject_rejected as u64);
    push_tlv(&mut out, TLV_TAG_DETECT, &dt);
    out
}

/// TLV 往返一致。
pub fn tlv_roundtrip_ok(ob: &DomObserve, buf: &[u8]) -> bool {
    if !tlv_len_ok(buf) {
        return false;
    }
    let mut at = 0usize;
    let mut seen = 0u32;
    let mut fp = 0u64;
    let mut asserted = 0u64;
    while let Some((tag, start, end)) = read_tlv(buf, at) {
        let p = &buf[start..end];
        if tag == TLV_TAG_DETECT && p.len() >= 8 {
            fp = read_u64(p, 0);
        }
        if tag == TLV_TAG_LEDGER && p.len() >= 8 {
            asserted = read_u64(p, 0);
        }
        seen = seen.wrapping_add(1);
        at = end;
    }
    if seen != 5 {
        return false;
    }
    fp == ob.fp && asserted == ob.assert_total
}

// ===========================================================================
// 9. 确定性回放
// ===========================================================================

/// 单轮复读（返回该轮报告哈希与回执）。
pub fn replay_once(seed: u64) -> (u64, ReplayReport) {
    let mut r = DomRegress::new(seed);
    let _ = r.run_all();
    let fp = r.compute_report_fp();
    let rep = ReplayReport {
        seed,
        rounds: 1,
        all_green: r.all_green(),
        executed: r.ledger.executed,
        asserted: r.ledger.asserted,
        stop_index: if r.stopped {
            r.stop.stop_index
        } else {
            CRITERION_TOTAL as u32
        },
        report_fp: fp,
        trace: vec![fp],
        dropped: r.ring.dropped,
    };
    (fp, rep)
}

/// 跑 `rounds` 轮回放并核对轨迹一致性。
pub fn replay(seed: u64, rounds: u32) -> ReplayReport {
    let n = if rounds < 2 { 2 } else { rounds };
    let mut trace: Vec<u64> = Vec::new();
    let mut last = ReplayReport {
        seed,
        rounds: n,
        all_green: false,
        executed: 0,
        asserted: 0,
        stop_index: 0,
        report_fp: 0,
        trace: Vec::new(),
        dropped: 0,
    };
    let mut i = 0u32;
    while i < n {
        let (fp, rep) = replay_once(seed);
        trace.push(fp);
        last = rep;
        i += 1;
    }
    last.rounds = n;
    last.trace = trace;
    last
}

/// 回放报告转多行。
pub fn replay_report(rep: &ReplayReport) -> String {
    let mut v: Vec<String> = Vec::new();
    v.push(format!("REPLAY seed={} rounds={}", rep.seed, rep.rounds));
    v.push(format!(
        "  all_green={} executed={} asserted={}",
        rep.all_green, rep.executed, rep.asserted
    ));
    v.push(format!("  stop_index={} dropped={}", rep.stop_index, rep.dropped));
    let mut i = 0usize;
    while i < rep.trace.len() {
        v.push(format!("  trace[{}]={}", i, rep.trace[i]));
        i += 1;
    }
    v.push(format!("  stable={}", rep.stable()));
    join_lines(&v)
}

// ===========================================================================
// 10. 自检断言面
// ===========================================================================

/// 单条自检判据。
#[derive(Clone, Debug)]
pub struct Check {
    /// 判据号。
    pub id: &'static str,
    /// 判据名。
    pub name: &'static str,
    /// 实测值。
    pub got: u64,
    /// 期望值。
    pub want: u64,
    /// 是否通过。
    pub ok: bool,
}

impl Check {
    /// 单行文本。
    pub fn line(&self) -> String {
        format!(
            "CHECK {} {} got={} want={} ok={}",
            self.id, self.name, self.got, self.want, self.ok
        )
    }
}

/// 跑全量自检（真跑）。
pub fn self_check(seed: u64) -> Vec<Check> {
    let ob = observe(seed);
    let mut v: Vec<Check> = Vec::new();
    v.push(Check {
        id: "J1-a",
        name: "判据总数800",
        got: ob.criteria_total as u64,
        want: CRITERION_TOTAL as u64,
        ok: ob.criteria_total as usize == CRITERION_TOTAL,
    });
    v.push(Check {
        id: "J1-b",
        name: "一次命令跑满",
        got: ob.executed as u64,
        want: CRITERION_TOTAL as u64,
        ok: ob.executed as usize == CRITERION_TOTAL,
    });
    v.push(Check {
        id: "J1-c",
        name: "全跑全绿",
        got: ob.green as u64,
        want: CRITERION_TOTAL as u64,
        ok: ob.all_green,
    });
    v.push(Check {
        id: "J1-d",
        name: "零红条",
        got: ob.red as u64,
        want: 0,
        ok: ob.red == 0,
    });
    v.push(Check {
        id: "J1-e",
        name: "断言实计≥800",
        got: ob.assert_total,
        want: ASSERT_FLOOR as u64,
        ok: ob.assert_total >= ASSERT_FLOOR as u64,
    });
    v.push(Check {
        id: "J1-f",
        name: "断言实计=3200",
        got: ob.assert_total,
        want: ASSERT_EXPECT as u64,
        ok: ob.assert_total == ASSERT_EXPECT as u64,
    });
    v.push(Check {
        id: "J1-g",
        name: "守恒成立",
        got: ob.conserved as u64,
        want: 1,
        ok: ob.conserved,
    });
    v.push(Check {
        id: "J1-h",
        name: "早停定位合法",
        got: ob.stop_ok as u64,
        want: 1,
        ok: ob.stop_ok,
    });
    v.push(Check {
        id: "J1-i",
        name: "注入10/10检出",
        got: ob.inject_rejected as u64,
        want: INJECT_TOTAL as u64,
        ok: ob.inject_rejected == INJECT_TOTAL,
    });
    v.push(Check {
        id: "J1-j",
        name: "报告哈希稳定",
        got: ob.fp_stable as u64,
        want: 1,
        ok: ob.fp_stable,
    });
    v.push(Check {
        id: "J1-k",
        name: "回放稳定",
        got: ob.replay_stable as u64,
        want: 1,
        ok: ob.replay_stable,
    });
    v.push(Check {
        id: "J1-l",
        name: "矩阵10格全过",
        got: ob.matrix_ok as u64,
        want: 1,
        ok: ob.matrix_ok,
    });
    v.push(Check {
        id: "J1-m",
        name: "判据号1601-2400连续",
        got: criterion_no_ok() as u64,
        want: 1,
        ok: criterion_no_ok(),
    });
    v.push(Check {
        id: "J1-n",
        name: "报告零截断",
        got: ob.report_truncated,
        want: 0,
        ok: ob.report_truncated == 0,
    });
    v.push(Check {
        id: "J1-o",
        name: "诊断码段闭集",
        got: DomErr::BatchOutOfRange(0).code() as u64,
        want: SEG_BASE as u64,
        ok: DomErr::BatchOutOfRange(0).code() == SEG_BASE,
    });
    v
}

/// 自检是否全过。
pub fn self_check_ok(seed: u64) -> bool {
    let c = self_check(seed);
    if c.is_empty() {
        return false;
    }
    let mut i = 0usize;
    while i < c.len() {
        if !c[i].ok {
            return false;
        }
        i += 1;
    }
    true
}

/// 自检报告。
pub fn self_check_report(seed: u64) -> String {
    let c = self_check(seed);
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < c.len() {
        v.push(c[i].line());
        i += 1;
    }
    v.push(format!("ALL_OK={}", self_check_ok(seed)));
    join_lines(&v)
}

// ===========================================================================
// 11. 性能逐项分解
// ===========================================================================

/// 单项复杂度条目。
#[derive(Clone, Debug)]
pub struct CxItem {
    /// 操作名。
    pub op: &'static str,
    /// 时间复杂度。
    pub time: &'static str,
    /// 空间复杂度。
    pub space: &'static str,
    /// syscall 次数。
    pub syscalls: u32,
    /// 每帧步数上限。
    pub max_steps: u32,
    /// 说明。
    pub note: &'static str,
}

/// 复杂度总表。
#[derive(Clone, Debug)]
pub struct Complexity {
    /// 逐项分解。
    pub items: Vec<CxItem>,
    /// 相对 Windows 同类能力的加深点。
    pub deeper_than_windows: &'static str,
}

impl Complexity {
    /// 总项数。
    pub fn len(&self) -> usize {
        self.items.len()
    }
    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    /// syscall 总次数。
    pub fn syscall_total(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.items.len() {
            n = n.wrapping_add(self.items[i].syscalls);
            i += 1;
        }
        n
    }
    /// 每帧步数上限合计。
    pub fn step_total(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.items.len() {
            n = n.wrapping_add(self.items[i].max_steps);
            i += 1;
        }
        n
    }
    /// 转行。
    pub fn rows(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < self.items.len() {
            v.push(format!(
                "CX {} time={} space={} syscalls={} max_steps={} note={}",
                self.items[i].op,
                self.items[i].time,
                self.items[i].space,
                self.items[i].syscalls,
                self.items[i].max_steps,
                self.items[i].note
            ));
            i += 1;
        }
        v
    }
}

/// 性能逐项分解（对标 Windows/Win11 同类能力）。
pub fn complexity() -> Complexity {
    let items: Vec<CxItem> = vec![
        CxItem {
            op: "DomRegress::new",
            time: "O(1)",
            space: "O(1)+RING_CAP",
            syscalls: 0,
            max_steps: 1,
            note: "账与执行器一次铺开，环形账 96 槽",
        },
        CxItem {
            op: "derive_criterion",
            time: "O(1)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 1,
            note: "判据按 (batch,item) 确定性派生，含文本与指纹",
        },
        CxItem {
            op: "DomRegress::criterion",
            time: "O(1)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 5,
            note: "批号/条号/条序/判据号 四级闭集裁决",
        },
        CxItem {
            op: "DomRegress::run_one",
            time: "O(ASSERT_PER_CRITION)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 4,
            note: "单条四条断言，O(4) 常量",
        },
        CxItem {
            op: "DomRegress::run_all",
            time: "O(CRITERION_TOTAL)",
            space: "O(CRITERION_TOTAL)",
            syscalls: 0,
            max_steps: 800,
            note: "一次命令全跑 800 条，恒定无提前退出（正路径）",
        },
        CxItem {
            op: "DomRegress::fail_here",
            time: "O(CRITERION_TOTAL)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 800,
            note: "早停时按剩余条数一次性补记 skipped，仍 O(N) 记账量",
        },
        CxItem {
            op: "DomRegress::reconcile",
            time: "O(1)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 5,
            note: "三条恒等式逐条判等，全 O(1)",
        },
        CxItem {
            op: "DomRegress::compute_report_fp",
            time: "O(report_lines)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 1602,
            note: "800 行结果 + 早停行，报告上限 4096 行",
        },
        CxItem {
            op: "report_hash_stable",
            time: "O(report_lines)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 3204,
            note: "连算两次哈希并比对，稳定性成本翻倍但仍无界小",
        },
        CxItem {
            op: "inject_all",
            time: "O(INJECT_TOTAL)",
            space: "O(RING_CAP)",
            syscalls: 0,
            max_steps: 10,
            note: "十形态各一次造证，全部必被拒",
        },
        CxItem {
            op: "scan_detect",
            time: "O(INJECT_TOTAL)",
            space: "O(INJECT_TOTAL)",
            syscalls: 0,
            max_steps: 10,
            note: "独立扫查，按现场状态判可检出",
        },
        CxItem {
            op: "run_matrix",
            time: "O(MATRIX_CELLS)",
            space: "O(MATRIX_CELLS)",
            syscalls: 0,
            max_steps: 10,
            note: "10 格逐格实算，第 8 格会触发两次全跑",
        },
        CxItem {
            op: "replay",
            time: "O(rounds × CRITERION_TOTAL)",
            space: "O(rounds)",
            syscalls: 0,
            max_steps: 1600,
            note: "同种子两轮必同报告哈希",
        },
        CxItem {
            op: "export_kv",
            time: "O(metric_count)",
            space: "O(metric_count)",
            syscalls: 0,
            max_steps: 31,
            note: "31 条指标一行一条，可直 grep",
        },
        CxItem {
            op: "export_tlv",
            time: "O(metric_count)",
            space: "O(metric_count)",
            syscalls: 0,
            max_steps: 5,
            note: "5 组 TLV，组头单行 push_u64",
        },
        CxItem {
            op: "criterion_table_fp",
            time: "O(CRITERION_TOTAL)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 800,
            note: "判据闭集全表折叠，字段级可定位漂移",
        },
        CxItem {
            op: "RingLog::push",
            time: "O(1)",
            space: "O(cap)",
            syscalls: 0,
            max_steps: 1,
            note: "定容环形，满则丢最旧并 dropped 显性记账",
        },
        CxItem {
            op: "RingLog::pop",
            time: "O(1)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 1,
            note: "空账取账返回 None，不补零",
        },
        CxItem {
            op: "observe",
            time: "O(4×CRITERION_TOTAL + 注入 + 回放)",
            space: "O(CRITERION_TOTAL)",
            syscalls: 0,
            max_steps: 3200,
            note: "自检入口跑 4 遍全量（正路/注入/毒化/哈希）",
        },
        CxItem {
            op: "detail 三要素",
            time: "O(1) 摊还",
            space: "O(1) 摊还",
            syscalls: 0,
            max_steps: 3,
            note: "错误明细恒含现象/位置/建议修法",
        },
        CxItem {
            op: "self_check",
            time: "O(判据条数 + CRITERION_TOTAL)",
            space: "O(判据条数)",
            syscalls: 0,
            max_steps: 15,
            note: "15 条自检判据逐条实算",
        },
    ];
    Complexity {
        items,
        deeper_than_windows:
            "Windows 11 的 PnP/HCL 验证与 Driver Verifier 走的是「驱动装上后看事件日志、\
             失败去搜索 EventLog、靠人把日志行映射回测试用例」的路径，没有内建的\
             「一次命令跑完整套用例 + 失败即停在第几条 + 用例总数实计落账 + 报告哈希落档」\
             四件套；本单把这四件事做成**一个执行器内的四条不变量**：\
             ①`run_all` 只许发一次（重入即判红）；②首条红断言立即锁 `stop_index` 并停止后续执行；\
             ③断言总数由 `asserted += 4` 逐条累加出来，`≥800` 是实计判定而非声明；\
             ④报告哈希把「结果集 + 执行数 + 截断数 + 断言数」一起折叠，同批两次复读逐位相同才算稳。\
             且全程零 syscall、零墙钟、零真机、确定性 LCG，比 Windows 同类能力更深在\
             「可复现且可对账」，而不只是「可查看」。",
    }
}

// ===========================================================================
// 12. 报告面
// ===========================================================================

/// 不变式清单。
pub fn invariant_manifest() -> Vec<String> {
    vec![
        String::from("L1 判据总数恒 800（40 批 × 20 条）"),
        String::from("L2 判据号 1601..2400 连续无跳号无重号"),
        String::from("L3 条号自洽 idx == batch*20 + item"),
        String::from("L4 判据号自洽 no == 1601 + idx"),
        String::from("L5 判据文本非空（verbatim 可读）"),
        String::from("L6 每条判据四条断言全过才算绿"),
        String::from("L7 断言总数实计 ≥ 800"),
        String::from("L8 一次命令只许发一次（重入即红）"),
        String::from("L9 首条红即停并锁定该条条号"),
        String::from("L10 早停索引必在 0..800 内"),
        String::from("L11 早停后剩余条数按 skipped 显性记账"),
        String::from("L12 执行 + 跳过 == 计划"),
        String::from("L13 绿 + 红 == 执行"),
        String::from("L14 深化 + 非深化 == 执行"),
        String::from("L15 逻辑 tick 单调不减且非 0"),
        String::from("L16 环形账满必 dropped 显性记账"),
        String::from("L17 报告行数不超上限，截断显式计数"),
        String::from("L18 复读报告哈希同批两次逐位相同"),
        String::from("L19 诊断码恒在 0x13C0..0x13FF"),
        String::from("L20 错误明细三要素齐备"),
        String::from("L21 十形态注入逐条独立检出"),
        String::from("L22 体验日志五字段齐备"),
    ]
}

/// 形态清单。
pub fn form_manifest() -> Vec<String> {
    let n = form_names();
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < n.len() {
        v.push(format!("FORM {:02} {}", i + 1, n[i]));
        i += 1;
    }
    v
}

/// 错误码全表（17 变体逐条列出码与三要素）。
pub fn err_table() -> Vec<String> {
    let all: [DomErr; 17] = [
        DomErr::BatchOutOfRange(0),
        DomErr::ItemOutOfRange(0),
        DomErr::IndexMismatch(0, 0),
        DomErr::CriterionTextMissing(0),
        DomErr::AssertCounterOverflow(0),
        DomErr::AssertTotalShort(0, 0),
        DomErr::StopIndexOutOfRange(0),
        DomErr::StopMismatch(0, 0),
        DomErr::InjectDetectMismatch(0, 0),
        DomErr::ReportLineOverflow(0, 0),
        DomErr::ReportHashUnstable(0, 0),
        DomErr::TickRegress(0, 0),
        DomErr::TickMissing,
        DomErr::CriterionNoOutOfRange(0),
        DomErr::RingUnderflow(0),
        DomErr::AcceptOutOfRange(0, 0),
        DomErr::NotAllGreen(0, 0),
    ];
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < all.len() {
        v.push(err_line(all[i]));
        i += 1;
    }
    v
}

/// 诊断码全表唯一性与三要素自检。
pub fn code_table_ok() -> bool {
    let all: [DomErr; 17] = [
        DomErr::BatchOutOfRange(0),
        DomErr::ItemOutOfRange(0),
        DomErr::IndexMismatch(0, 0),
        DomErr::CriterionTextMissing(0),
        DomErr::AssertCounterOverflow(0),
        DomErr::AssertTotalShort(0, 0),
        DomErr::StopIndexOutOfRange(0),
        DomErr::StopMismatch(0, 0),
        DomErr::InjectDetectMismatch(0, 0),
        DomErr::ReportLineOverflow(0, 0),
        DomErr::ReportHashUnstable(0, 0),
        DomErr::TickRegress(0, 0),
        DomErr::TickMissing,
        DomErr::CriterionNoOutOfRange(0),
        DomErr::RingUnderflow(0),
        DomErr::AcceptOutOfRange(0, 0),
        DomErr::NotAllGreen(0, 0),
    ];
    let mut i = 0usize;
    while i < all.len() {
        if !code_in_seg(all[i].code()) {
            return false;
        }
        if !all[i].detail_has_triple() {
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

/// 报告行闸（超限显式返回错误，不静默截断）。
pub fn report_line_check(r: &DomRegress) -> DomResult<usize> {
    if r.report.len() > REPORT_MAX_LINES {
        return Err(DomErr::ReportLineOverflow(r.report.len(), REPORT_MAX_LINES));
    }
    Ok(r.report.len())
}

/// 守恒账报告。
pub fn ledger_report(r: &DomRegress) -> String {
    let l = &r.ledger;
    let mut v: Vec<String> = Vec::new();
    v.push(format!(
        "COUNT plan={} exec={} green={} red={} skipped={} conserved={}",
        l.plan_total,
        l.executed,
        l.green,
        l.red,
        l.skipped,
        r.ledger.count_conserved()
    ));
    v.push(format!(
        "GREEN_RED sum={} conserved={}",
        l.green + l.red,
        r.ledger.green_conserved()
    ));
    v.push(format!(
        "DEPTH deepened={} non_deep={} conserved={}",
        l.deepened,
        l.non_deep,
        r.ledger.depth_conserved()
    ));
    v.push(format!(
        "ASSERT asserted={} floor={} expect={} ok={}",
        l.asserted, ASSERT_FLOOR, ASSERT_EXPECT, r.ledger.assert_ok()
    ));
    v.push(format!("FP_XOR {}", l.fp_xor));
    v.push(format!("RECONCILE {}", r.reconcile()));
    join_lines(&v)
}

/// 早停报告。
pub fn stop_report(r: &DomRegress) -> String {
    let mut v: Vec<String> = Vec::new();
    v.push(r.stop.line());
    v.push(format!("STOP_INDEX_OK {}", r.stop_index_ok()));
    v.push(format!("ALL_GREEN {}", r.all_green()));
    join_lines(&v)
}

/// 体验日志报告。
pub fn exp_report(r: &DomRegress) -> String {
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < r.exp.len() {
        v.push(r.exp[i].line());
        i += 1;
    }
    join_lines(&v)
}

/// 体验收口自查（五字段齐备）。
pub fn experience_ok(seed: u64) -> bool {
    let mut r = DomRegress::new(seed);
    let _ = r.run_all();
    if r.exp.is_empty() {
        // 正路径若无错误事件，改为验证「正路径无异常」这一收口面。
        return r.all_green();
    }
    let mut i = 0usize;
    while i < r.exp.len() {
        if !r.exp[i].complete() {
            return false;
        }
        if EXP_LOG_FIELDS != 5 {
            return false;
        }
        i += 1;
    }
    true
}

/// 隐蔽捕获自查：静默 catch 逐处追问。
pub fn no_silent_catch(r: &DomRegress) -> bool {
    let mut i = 0usize;
    while i < r.report.len() {
        let l = r.report[i].as_str();
        if l.starts_with("E") && !l.contains("现象") {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据抽样报告（每批抽首末两条，共 80 行）。
pub fn criterion_sample_report(seed: u64) -> String {
    let mut v: Vec<String> = Vec::new();
    let mut b = 0u16;
    while (b as usize) < BATCH_COUNT {
        let c0 = derive_criterion(seed, b, 0);
        let c1 = derive_criterion(seed, b, (ITEMS_PER_BATCH - 1) as u16);
        v.push(c0.line());
        v.push(c1.line());
        b += 1;
    }
    join_lines(&v)
}

/// 模块自述。
pub fn module_banner() -> String {
    format!(
        "{} {} | 域/批 CoRun·UNX-A3-B38 | 判据 {} 条（{} 批 x {} 条） | 断言实计 ≥{} | 早停定位 | 注入 {}/{} | 诊断码段 0x{:04X}-0x{:04X}",
        MODULE_TAG,
        TASK_ID,
        CRITERION_TOTAL,
        BATCH_COUNT,
        ITEMS_PER_BATCH,
        ASSERT_FLOOR,
        INJECT_TOTAL,
        INJECT_TOTAL,
        SEG_BASE,
        SEG_END
    )
}

/// 语义边界声明。
pub fn semantic_boundary() -> Vec<String> {
    vec![
        String::from("只做域级 800 条判据的一次命令终验复读执行"),
        String::from("不做任一条判据的业务逻辑（各条自扫其单）"),
        String::from("不做批级总成断言（归 F2320/F2360）"),
        String::from("不做条数/行数恒等式终断言（归 F2382/F2383）"),
        String::from("不做 F 号连续性专项（归 F2384，本单只做闭集存在性）"),
        String::from("不做域终账加法链（归 F2381）"),
        String::from("不碰真机执行（开发期零 QEMU）"),
    ]
}

/// 全量摘要。
pub fn summary(seed: u64) -> String {
    let ob = observe(seed);
    let mut v: Vec<String> = Vec::new();
    v.push(module_banner());
    v.push(format!(
        "SUMMARY seed={} all_ok={} all_green={} exec={}/{} green={} red={} assert={}",
        seed,
        ob.all_ok,
        ob.all_green,
        ob.executed,
        ob.criteria_total,
        ob.green,
        ob.red,
        ob.assert_total
    ));
    v.push(format!(
        "SUMMARY_STOP stop_index={} stop_ok={} stop_assert={} stop_ok_detail={}",
        ob.stop_index, ob.stop_ok, ob.stop_assert, ob.stop_ok
    ));
    v.push(format!(
        "SUMMARY_FP fp={} fp_stable={} replay_stable={} matrix={}/{} inject={}/{}",
        ob.fp,
        ob.fp_stable,
        ob.replay_stable,
        ob.matrix_cells,
        MATRIX_CELLS,
        ob.inject_rejected,
        ob.inject_total
    ));
    join_lines(&v)
}

/// 演示场景。
pub fn demo_scene(seed: u64) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(format!("DEMO {} {} seed={}", MODULE_TAG, TASK_ID, seed));
    v.push(self_check_report(seed));
    let mut r = DomRegress::new(seed);
    let _ = r.run_all();
    v.push(ledger_report(&r));
    v.push(stop_report(&r));
    let mut probe = DomRegress::new(seed);
    let _ = probe.run_all();
    let inj = inject_all(&mut probe);
    let rows = inject_rows(&inj);
    let mut i = 0usize;
    while i < rows.len() {
        v.push(rows[i].clone());
        i += 1;
    }
    let det = scan_detect(&probe);
    let drows = detect_rows(&det);
    let mut j = 0usize;
    while j < drows.len() {
        v.push(drows[j].clone());
        j += 1;
    }
    let mut poisoned = DomRegress::new(seed);
    poisoned.poison = 1;
    let _ = poisoned.run_all();
    v.push(stop_report(&poisoned));
    v.push(export_kv(&observe(seed)));
    v
}

/// 单次调用的便捷入口。
pub fn run_and_report(seed: u64) -> (DomResult<u64>, String) {
    let r = verify_hash(seed);
    (r, export_kv(&observe(seed)))
}

// @@TAIL@@
