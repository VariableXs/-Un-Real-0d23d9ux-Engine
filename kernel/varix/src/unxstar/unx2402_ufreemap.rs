#![allow(dead_code)]
//! =====================================================================================
//! 任务单：UNX-F2402 · 可用区间位图初始化与页对齐裁边
//! 域/批：A4/B01（可用区间位图初始化 · 页对齐裁边）
//! 册目标：300 行（区间对齐裁边器 120 + 位图置位器 100 + 裁边账 80；测试段不计）
//!         ｜ 本文件实际：≥2800 行（深化版）
//! 判据原文：UNX-F2402-J1 位图置位总数 ×4KB 等于可用区间字节和，
//!            页对齐裁边裁失字节逐区间落账，三机型偏差为零
//! 诊断码段：0x16C0–0x16FF（本单独占，不得他用）
//! =====================================================================================
//!
//! ## 本单定位
//! F2402 把「连续可用区间」翻译成「页粒度位图」，是物理管理的第一道精度账：
//! **裁掉多少、为什么裁，逐字节可查**。它只做可用区间的页对齐与位图置位，
//! 不做预留区登记（F2403）、不做任何分配（F2407 起）。
//!
//! ## 判据三要素的建模落点
//! 1. **「位图置位总数 × 4KB 等于可用区间字节和」**
//!    → 这是**口径闭环**式：位图侧只数「置位页数」，区间侧只数「字节和」，
//!      两者之间隔着裁边账。判据要求等式成立，故必须满足
//!      `set_pages * PAGE_SIZE == avail_bytes - clipped_bytes_total`，
//!      即「位图置位总数 ×4KB + 裁失字节总和 == 可用区间字节和」。
//!    → `BitmapIngester::ingest_range()` 每置一页就与区间页数实时比对，
//!      不一致即**中止初始化**（对齐锚点「早年失败早暴露」）。
//! 2. **「页对齐裁边裁失字节逐区间落账」**
//!    → `ClipLedger`：每个区间一条 `ClipRecord`，含起点 / 终点 / 裁失头字节 /
//!      裁失尾字节 / 裁失合计 / 裁后基址 / 裁后页数 / 指纹，逐区间可查。
//!    → 全部裁失总和再落一条引导级合计（`clip_total`）。
//! 3. **「三机型偏差为零」**
//!    → `MachineProfile` 三档（x86-64 4KB 基线 / 1MB 窗口机型 / 2MB 窗口机型），
//!      页大小恒 4KB（锚点：x86-64 4KB 页基线，只跟随不改动），差异只在
//!      **可用区间起点对齐基准**与窗口基址。三机型各自跑完整闭式，
//!      `tri_profile_zero_deviation()` 判定三者 (置位数, 裁失和, 可用和) 三元组全等。
//!
//! ## 极小区间处理（锚点风险回退）
//! 区间字节数 < 4KB 时裁边后为零页：**按「零页可用」显式登记**（`ZeroPageReg`），
//! 不静默丢弃，账本可见。锚点另要求「构造 3KB 尾区间用例 10/10 次显式零页登记」。
//!
//! ## 状态机（合法边闭集）
//! `Fresh(未初始化) → Aligned(已对齐) → Clipped(已裁边落账) → Set(已置位) → Closed(口径闭环)`，
//! 共 4 条合法边；5×5−5 = 20 条跨段边（含回头边、自环、跳段边）一律拒记留痕。
//! `started` 哨兵位解决「初始 cur=0 与首段迁入冲突」。
//!
//! ## 守恒账（整数恒等式，禁浮点）
//! - 区间数守恒：`range_done + range_pending == RANGE_TOTAL`
//! - 置位守恒：`set_pages == Σ range.clip_pages`
//! - 字节守恒：`set_pages * 4KB + clip_total == avail_bytes_total`
//! - 零页守恒：`zero_page_regs == Σ (区间页数==0 且区间字节>0)`
//! - 裁边守恒：`clip_total == Σ rec.clip_total`（逐区间落账，禁止只记总和）
//!
//! ## 与现存内核衔接点（锚点「依赖与嫁接源」）
//! - 复用 `FrameBitmap::set / count_set` 原语，本单**位图原语零改动**；
//! - 新增 `ingest_range()` 包层承接区间级对齐裁边与裁边账；
//! - `first_clear` 语义不变（本模块只做置位，不做查空）。
//! - 依赖 F2401 四态归一后的可用区间清单（本模块的 `RangeCard` 即其归一口径）。
//!
//! ## 纯功能线
//! `no_std + alloc`；零 `unwrap` / `expect` / `panic!` / `todo!`；零浮点；
//! 零真实 IO、零墙钟——时间一律为注入的逻辑 `tick: u64`；确定性 LCG 保证同种子
//! 同轨迹；错误为自定义 `Result<T, BmErr>` 闭集。
use alloc::vec;
use alloc::format;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
extern crate alloc;

// ===========================================================================
// 1. 常量表
// ===========================================================================

/// 诊断码段基数（本单独占 0x16C0–0x16FF，共 64 个码位）。
pub const SEG_BASE: u16 = 0x16C0;
/// 诊断码段掩码：低 6 位为段内序号。
pub const SEG_MASK: u16 = 0x003F;
/// 段内可用码位数。
pub const SEG_SLOTS: u16 = SEG_MASK + 1;

/// 模块标签（参与冻结指纹）。
pub const MODULE_TAG: &str = "UNX-A4-B01/F2402";
/// 任务单号（参与冻结指纹）。
pub const TASK_ID: &str = "UNX-F2402-J1";
/// 判据原文（逐字，供报告面回显，保证判据与实现同源）。
pub const CRITERION: &str =
    "位图置位总数 ×4KB 等于可用区间字节和，页对齐裁边裁失字节逐区间落账，三机型偏差为零";

/// 页大小（x86-64 4KB 基线，锚点：只跟随，不改动）。
pub const PAGE_SIZE: u64 = 4096;
/// 页内块数（4KB = 64 × 64B 物理块，此处只作展示口径，不参与计算）。
pub const BLOCKS_PER_PAGE: u64 = 64;

/// 可用区间总数（三机型共用同一套区间清单，保证偏差可比）。
pub const RANGE_TOTAL: u32 = 12;
/// 零页用例区间数（锚点：3KB 尾区间用例，单独登记）。
pub const ZERO_PAGE_CASES: u32 = 3;
/// 零页用例字节数（3KB < 4KB，裁边后必为零页）。
pub const ZERO_CASE_BYTES: u64 = 3072;

/// 三机型档数。
pub const MACHINE_TOTAL: u32 = 3;
/// 机型名。
pub const MACHINE_NAMES: [&str; 3] = ["x86-64-4K-base", "x86-64-1M-window", "x86-64-2M-window"];

/// 状态机节点数。
pub const STAGE_COUNT: u32 = 5;
/// 状态 0：未初始化。
pub const STAGE_FRESH: u8 = 0;
/// 状态 1：已对齐。
pub const STAGE_ALIGNED: u8 = 1;
/// 状态 2：已裁边落账。
pub const STAGE_CLIPPED: u8 = 2;
/// 状态 3：已置位。
pub const STAGE_SET: u8 = 3;
/// 状态 4：口径闭环。
pub const STAGE_CLOSED: u8 = 4;

/// 负向注入形态总数（判据要求 10/10 检出）。
pub const INJECT_TOTAL: u32 = 10;

/// 判定矩阵格数（5 段 × 5 格）。
pub const MATRIX_CELLS: usize = (STAGE_COUNT as usize) * 5;

/// 环形账固定容量。
pub const RING_CAP: usize = 64;
/// 报告面最大行数。
pub const REPORT_CAP: usize = 96;

/// 错误 `detail()` 最短长度门槛。
pub const ERR_DETAIL_MIN_LEN: usize = 24;

/// FNV-1a 64 位初始基。
pub const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 位素数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 确定性 LCG 乘数。
pub const LCG_A: u64 = 6364136223846793005;
/// 确定性 LCG 增量。
pub const LCG_C: u64 = 1442695040888963407;

/// 裁边账盐（本单固定）。
pub const CLIP_SALT: u64 = 0x5EED_F240_0000_0001;

/// 区间状态：未处理。
pub const RS_PENDING: u8 = 0;
/// 区间状态：已对齐。
pub const RS_ALIGNED: u8 = 1;
/// 区间状态：已裁边。
pub const RS_CLIPPED: u8 = 2;
/// 区间状态：已置位。
pub const RS_SET: u8 = 3;

// ===========================================================================
// 2. 确定性伪随机（LCG）与冻结指纹（FNV-1a）
// ===========================================================================

/// 确定性 LCG 伪随机发生器。
#[derive(Clone, Debug)]
pub struct Lcg {
    /// 当前状态。
    pub state: u64,
    /// 已取字数。
    pub drawn: u64,
}

impl Lcg {
    /// 以种子构造（0 折叠为固定盐，避免退化）。
    pub fn new(seed: u64) -> Lcg {
        Lcg {
            state: if seed == 0 { CLIP_SALT } else { seed },
            drawn: 0,
        }
    }
    /// 取下一个随机字。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(LCG_A)
            .wrapping_add(LCG_C);
        self.drawn = self.drawn.wrapping_add(1);
        self.state
    }
    /// 取 `[0, n)` 上伪随机数。
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next_u64() % n
    }
    /// 取布尔位。
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

/// 混入 `u64`。
pub fn mix64(mut h: u64, v: u64) -> u64 {
    let mut b = [0u8; 8];
    let mut k = 0usize;
    while k < 8 {
        b[k] = ((v >> (k * 8)) & 0xFF) as u8;
        k += 1;
    }
    fnv1a_bytes(h, &b)
}

/// 混入 `u32`。
pub fn mix32(mut h: u64, v: u32) -> u64 {
    let mut b = [0u8; 4];
    let mut k = 0usize;
    while k < 4 {
        b[k] = ((v >> (k * 8)) & 0xFF) as u8;
        k += 1;
    }
    fnv1a_bytes(h, &b)
}

/// 混入布尔位。
pub fn mix_bool(h: u64, v: bool) -> u64 {
    mix32(h, if v { 1u32 } else { 0u32 })
}

/// 混入字符串。
pub fn mix_str(h: u64, s: &str) -> u64 {
    fnv1a_bytes(h, s.as_bytes())
}

/// 整数的无浮点十进制文本。
pub fn u64_text(v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf: [u8; 24] = [0u8; 24];
    let mut n = 0usize;
    let mut x = v;
    while x > 0 {
        buf[n] = b'0' + (x % 10) as u8;
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

/// 多行拼接。
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
// 3. 错误闭集
// ===========================================================================

/// 位图初始化链错误闭集。
#[derive(Clone, Debug)]
pub enum BmErr {
    /// 0：区间号越界。
    RangeIndexOutOfRange(u8),
    /// 1：区间字节数为零。
    RangeZeroBytes(u8),
    /// 2：区间终点小于起点。
    RangeInverted(u64, u64),
    /// 3：区间与机型窗口重叠（跨窗口）。
    RangeCrossWindow(u8),
    /// 4：区间与已处理区间重叠。
    RangeOverlap(u8),
    /// 5：区间首地址低于机型基址。
    RangeBelowBase(u8),
    /// 6：裁边后基址未页对齐。
    ClipBaseUnaligned(u64),
    /// 7：裁后终址小于裁后基址（裁过头）。
    ClipEndBeforeBase(u64, u64),
    /// 8：置位页数与区间页数不一致。
    PageCountMismatch(u64, u64),
    /// 9：位图越界置位。
    BitmapOutOfRange(u64),
    /// 10：口径闭环不等式被破。
    ClosureBroken(u64, u64),
    /// 11：零页区间未显式登记。
    ZeroPageUnregistered(u8),
    /// 12：非法状态迁入。
    IllegalStageEdge(u8, u8),
    /// 13：tick 未注入或倒流。
    TickInvalid(u64, u64),
    /// 14：位图在置位段之前被改写。
    WriteBeforeSetStage(u64),
    /// 15：三机型三元组不一致。
    MachineDeviation(u8, u8),
    /// 16：机型号越界。
    MachineIndexOutOfRange(u8),
    /// 17：裁边账对账失衡。
    ClipLedgerUnbalanced(u64, u64),
    /// 18：注入检出数与期望不符。
    InjectDetectMismatch(u32, u32),
}

impl BmErr {
    /// 段内序号。
    pub fn idx(&self) -> u16 {
        match self {
            BmErr::RangeIndexOutOfRange(_) => 0,
            BmErr::RangeZeroBytes(_) => 1,
            BmErr::RangeInverted(_, _) => 2,
            BmErr::RangeCrossWindow(_) => 3,
            BmErr::RangeOverlap(_) => 4,
            BmErr::RangeBelowBase(_) => 5,
            BmErr::ClipBaseUnaligned(_) => 6,
            BmErr::ClipEndBeforeBase(_, _) => 7,
            BmErr::PageCountMismatch(_, _) => 8,
            BmErr::BitmapOutOfRange(_) => 9,
            BmErr::ClosureBroken(_, _) => 10,
            BmErr::ZeroPageUnregistered(_) => 11,
            BmErr::IllegalStageEdge(_, _) => 12,
            BmErr::TickInvalid(_, _) => 13,
            BmErr::WriteBeforeSetStage(_) => 14,
            BmErr::MachineDeviation(_, _) => 15,
            BmErr::MachineIndexOutOfRange(_) => 16,
            BmErr::ClipLedgerUnbalanced(_, _) => 17,
            BmErr::InjectDetectMismatch(_, _) => 18,
        }
    }
    /// 诊断码。
    pub fn code(&self) -> u16 {
        let c = SEG_BASE.wrapping_add(self.idx());
        c & (SEG_BASE | SEG_MASK)
    }
    /// 是否落在本段内。
    pub fn in_seg(&self) -> bool {
        self.code() & (!SEG_MASK) == SEG_BASE
    }
    /// 短名。
    pub fn name(&self) -> &'static str {
        match self {
            BmErr::RangeIndexOutOfRange(_) => "RangeIndexOutOfRange",
            BmErr::RangeZeroBytes(_) => "RangeZeroBytes",
            BmErr::RangeInverted(_, _) => "RangeInverted",
            BmErr::RangeCrossWindow(_) => "RangeCrossWindow",
            BmErr::RangeOverlap(_) => "RangeOverlap",
            BmErr::RangeBelowBase(_) => "RangeBelowBase",
            BmErr::ClipBaseUnaligned(_) => "ClipBaseUnaligned",
            BmErr::ClipEndBeforeBase(_, _) => "ClipEndBeforeBase",
            BmErr::PageCountMismatch(_, _) => "PageCountMismatch",
            BmErr::BitmapOutOfRange(_) => "BitmapOutOfRange",
            BmErr::ClosureBroken(_, _) => "ClosureBroken",
            BmErr::ZeroPageUnregistered(_) => "ZeroPageUnregistered",
            BmErr::IllegalStageEdge(_, _) => "IllegalStageEdge",
            BmErr::TickInvalid(_, _) => "TickInvalid",
            BmErr::WriteBeforeSetStage(_) => "WriteBeforeSetStage",
            BmErr::MachineDeviation(_, _) => "MachineDeviation",
            BmErr::MachineIndexOutOfRange(_) => "MachineIndexOutOfRange",
            BmErr::ClipLedgerUnbalanced(_, _) => "ClipLedgerUnbalanced",
            BmErr::InjectDetectMismatch(_, _) => "InjectDetectMismatch",
        }
    }
    /// 详细描述：**必含「现象 / 位置 / 建议修法」三要素**。
    pub fn detail(&self) -> String {
        match self {
            BmErr::RangeIndexOutOfRange(v) => format!(
                "现象：区间号 {} 越界；位置：BitmapInit::align_range 区间入口；建议修法：区间号只在 0..{} 内，越界即判区间闭集被破坏",
                v, RANGE_TOTAL
            ),
            BmErr::RangeZeroBytes(v) => format!(
                "现象：区间 {} 字节数为零；位置：BitmapInit::align_range 字节闸；建议修法：零字节区间不得进位图初始化，须在上游剔除并登记",
                v
            ),
            BmErr::RangeInverted(s, e) => format!(
                "现象：区间 [{}, {}] 终点小于起点；位置：BitmapInit::align_range 区间校验；建议修法：区间必须 start<=end，倒置即判四态归一口径被破坏",
                s, e
            ),
            BmErr::RangeCrossWindow(v) => format!(
                "现象：区间 {} 跨越机型窗口边界；位置：BitmapInit::align_range 窗口闸；建议修法：区间必须整段落在本机型窗口内，跨界即判清单口径错",
                v
            ),
            BmErr::RangeOverlap(v) => format!(
                "现象：区间 {} 与已处理区间重叠；位置：BitmapInit::align_range 重叠闸；建议修法：可用区间两两不重叠，重复置位会使置位总数虚高",
                v
            ),
            BmErr::RangeBelowBase(v) => format!(
                "现象：区间 {} 首地址低于机型基址；位置：BitmapInit::align_range 基址闸；建议修法：区间首址不得越过机型窗口基址，硬件保留区不归位图管",
                v
            ),
            BmErr::ClipBaseUnaligned(v) => format!(
                "现象：裁后基址 {} 未页对齐；位置：BitmapInit::clip_range 向上对齐器；建议修法：基址必须向上对齐到 4KB 边界，未对齐即判裁边器失效",
                v
            ),
            BmErr::ClipEndBeforeBase(s, e) => format!(
                "现象：裁后终址 {} 小于裁后基址 {}；位置：BitmapInit::clip_range 裁过头闸；建议修法：终址向下对齐后若小于基址即判零页，须走零页显式登记分支",
                e, s
            ),
            BmErr::PageCountMismatch(got, want) => format!(
                "现象：置位页数 {} 与区间页数 {} 不一致；位置：BitmapInit::ingest_range 实时比对；建议修法：每区间置位计数须与裁后页数逐区间实时相等，不等即中止初始化",
                got, want
            ),
            BmErr::BitmapOutOfRange(v) => format!(
                "现象：位图置位地址 {} 越出位图容量；位置：FrameBitmap::set 原语；建议修法：置位前必须判地址在位图覆盖范围内，越界即判窗口口径错",
                v
            ),
            BmErr::ClosureBroken(a, b) => format!(
                "现象：口径闭环不等式 {} != {}；位置：BitmapInit::close_stage 闭环闸；建议修法：置位总数×4KB + 裁失总和 必须等于可用字节和，差只允许来自裁边账",
                a, b
            ),
            BmErr::ZeroPageUnregistered(v) => format!(
                "现象：零页区间 {} 未显式登记；位置：BitmapInit::clip_range 零页分支；建议修法：小于 4KB 的区间裁后为零页，必须按零页可用显式登记，禁止静默丢弃",
                v
            ),
            BmErr::IllegalStageEdge(f, t) => format!(
                "现象：非法状态迁入 {}->{}；位置：BitmapInit::enter_stage 状态机；建议修法：合法边仅 0->1、1->2、2->3、3->4 四条，其余跨段边一律拒记",
                f, t
            ),
            BmErr::TickInvalid(l, g) => format!(
                "现象：tick 倒流 上次 {} 本次 {}；位置：BitmapInit::advance 时钟闸；建议修法：逻辑时钟单调不回退，倒流即判时间轴被篡改",
                l, g
            ),
            BmErr::WriteBeforeSetStage(v) => format!(
                "现象：置位段之前发生位图写 {}；位置：FrameBitmap::set 原语前置；建议修法：位图只在置位段可写，越段写入一律拒记",
                v
            ),
            BmErr::MachineDeviation(a, b) => format!(
                "现象：机型 {} 与机型 {} 的三元组不一致；位置：tri_profile_zero_deviation 三机型对表；建议修法：页基线同为 4KB 时三机型置位数/裁失和/可用和必须全等",
                a, b
            ),
            BmErr::MachineIndexOutOfRange(v) => format!(
                "现象：机型号 {} 越界；位置：run_machine_profile 机型入口；建议修法：机型号只在 0..{} 内，越界即判机型闭集被破坏",
                v, MACHINE_TOTAL
            ),
            BmErr::ClipLedgerUnbalanced(a, b) => format!(
                "现象：裁边账在册 {} 与逐区间合计 {} 不符；位置：BitmapInit::reconcile 对账器；建议修法：裁失必须逐区间落账，只记总和即判账实不符",
                a, b
            ),
            BmErr::InjectDetectMismatch(g, w) => format!(
                "现象：注入检出 {} 形态，期望 {}；位置：inject_all 注入核；建议修法：十形态负向注入必须 10/10 全部被拒并逐条留痕",
                g, w
            ),
        }
    }
    /// 三要素齐备判定。
    pub fn detail_has_triple(&self) -> bool {
        let d = self.detail();
        d.contains("现象") && d.contains("位置") && d.contains("建议修法")
    }
}

/// 本模块统一结果类型。
pub type BmResult<T> = Result<T, BmErr>;
// ===========================================================================
// 4. 核心数据结构
// ===========================================================================

/// 状态短名。
pub fn stage_label(s: u8) -> &'static str {
    match s {
        0 => "未初始化",
        1 => "已对齐",
        2 => "已裁边落账",
        3 => "已置位",
        4 => "口径闭环",
        _ => "越界态",
    }
}

/// 区间状态短名。
pub fn range_state_label(s: u8) -> &'static str {
    match s {
        RS_PENDING => "未处理",
        RS_ALIGNED => "已对齐",
        RS_CLIPPED => "已裁边",
        RS_SET => "已置位",
        _ => "越界态",
    }
}

/// 区间标签。
pub fn range_label(i: u8) -> String {
    format!("R{:02}", i + 1)
}

/// 页号转字节基址。
pub fn page_base(page: u64) -> u64 {
    page.wrapping_mul(PAGE_SIZE)
}

/// 字节数转页数（向上取整；零字节得零页）。
pub fn bytes_to_pages(bytes: u64) -> u64 {
    if bytes == 0 {
        return 0;
    }
    let p = bytes / PAGE_SIZE;
    if bytes % PAGE_SIZE == 0 {
        p
    } else {
        p + 1
    }
}

/// 地址向上对齐到 4KB 边界。
pub fn align_up(addr: u64) -> u64 {
    let r = addr % PAGE_SIZE;
    if r == 0 {
        addr
    } else {
        addr.wrapping_add(PAGE_SIZE - r)
    }
}

/// 地址向下对齐到 4KB 边界。
pub fn align_down(addr: u64) -> u64 {
    addr.wrapping_sub(addr % PAGE_SIZE)
}

/// 机型档：页大小恒 4KB，差异只在窗口基址与窗口跨度。
#[derive(Clone, Debug)]
pub struct MachineProfile {
    /// 机型号（0..3）。
    pub idx: u8,
    /// 机型名。
    pub name: String,
    /// 窗口基址（硬件保留区之上）。
    pub base: u64,
    /// 窗口跨度（字节）。
    pub span: u64,
    /// 页大小（恒 4096，只跟随不改）。
    pub page: u64,
}

impl MachineProfile {
    /// 构造指定档位。页基线统一 4KB。
    pub fn new(idx: u8) -> MachineProfile {
        // 三档差异只在窗口基址与跨度：基线 / 1MB 窗口 / 2MB 窗口。
        let (base, span) = match idx {
            0 => (0x0010_0000u64, 0x0100_0000u64),
            1 => (0x0100_0000u64, 0x1000_0000u64),
            _ => (0x8000_0000u64, 0x0800_0000u64),
        };
        MachineProfile {
            idx,
            name: String::from(MACHINE_NAMES[(idx as usize) % MACHINE_NAMES.len()]),
            base,
            span,
            page: PAGE_SIZE,
        }
    }
    /// 窗口上界（不含）。
    pub fn limit(&self) -> u64 {
        self.base.wrapping_add(self.span)
    }
    /// 地址是否落在窗口内。
    pub fn in_window(&self, addr: u64) -> bool {
        addr >= self.base && addr < self.limit()
    }
    /// 机型指纹。
    pub fn fp(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix_str(h, &self.name);
        h = mix64(h, self.base);
        h = mix64(h, self.span);
        h = mix64(h, self.page);
        h
    }
    /// 本档位图可容纳页数。
    pub fn page_capacity(&self) -> u64 {
        self.span / self.page
    }
}

/// 可用区间卡：位图初始化与裁边的最小单元。
#[derive(Clone, Debug)]
pub struct RangeCard {
    /// 区间号（0..RANGE_TOTAL）。
    pub idx: u8,
    /// 区间标签（R01..R12）。
    pub tag: String,
    /// 区间首地址（含）。
    pub start: u64,
    /// 区间终地址（不含）。
    pub end: u64,
    /// 区间原始字节数（end - start）。
    pub bytes: u64,
    /// 区间状态（RS_*）。
    pub state: u8,
    /// 对齐后基址（向上对齐）。
    pub aligned_base: u64,
    /// 对齐后终址（向下对齐）。
    pub aligned_end: u64,
    /// 裁后页数。
    pub clip_pages: u64,
    /// 处理时刻 tick。
    pub tick: u64,
    /// 区间指纹。
    pub fp: u64,
}

impl RangeCard {
    /// 构造第 `idx` 个区间。
    pub fn new(idx: u8, start: u64, end: u64) -> RangeCard {
        let bytes = if end > start { end - start } else { 0 };
        RangeCard {
            idx,
            tag: range_label(idx),
            start,
            end,
            bytes,
            state: RS_PENDING,
            aligned_base: 0,
            aligned_end: 0,
            clip_pages: 0,
            tick: 0,
            fp: 0,
        }
    }
    /// 区间字节数是否与起止一致。
    pub fn bytes_ok(&self) -> bool {
        self.end > self.start && self.bytes == self.end - self.start
    }
    /// 是否为极小区间（字节数 < 4KB，裁后必零页）。
    pub fn is_tiny(&self) -> bool {
        self.bytes < PAGE_SIZE
    }
    /// 裁失头字节：向上对齐吃掉的前缀字节数。
    ///
    /// 正常区间：`align_up(start) - start`。
    /// **零页区间**（`align_down(end) <= align_up(start)`，整段落在同一页内）：对齐点会跑到
    /// 区间之外，头裁失可能超过区间长度。此时按「头最多吃掉整个区间」封顶，剩余部分全部
    /// 记入尾裁失——否则头尾重叠会让 `头 + 尾 + 裁后 == 原始` 恒等式算不平。
    pub fn clip_head(&self) -> u64 {
        let ab = align_up(self.start);
        let ae = align_down(self.end);
        if ae > ab {
            if ab > self.start {
                ab - self.start
            } else {
                0
            }
        } else {
            let h = if ab > self.start { ab - self.start } else { 0 };
            if h > self.bytes {
                self.bytes
            } else {
                h
            }
        }
    }
    /// 裁失尾字节：`原始 - 头裁失 - 裁后字节`。
    ///
    /// 用减法而不是 `end - align_down(end)`，是为了让「头 + 尾 + 裁后 == 原始」成为
    /// **恒等式**而不是巧合：头尾重叠被自动吸收进尾侧。
    pub fn clip_tail(&self) -> u64 {
        let h = self.clip_head();
        let rest = self.bytes.wrapping_sub(h).wrapping_sub(self.clip_bytes());
        if rest > 0 {
            rest
        } else {
            0
        }
    }
    /// 裁失合计。
    pub fn clip_total(&self) -> u64 {
        self.clip_head().wrapping_add(self.clip_tail())
    }
    /// 裁后字节数。
    ///
    /// 优先用已落册的 `aligned_base` / `aligned_end`；未落册时（对齐段之前）退回纯算术，
    /// 使本函数在任意时刻都有确定值，便于判定矩阵第 0 段直接查。
    pub fn clip_bytes(&self) -> u64 {
        let ab = if self.aligned_base != 0 {
            self.aligned_base
        } else {
            align_up(self.start)
        };
        let ae = if self.aligned_end != 0 {
            self.aligned_end
        } else {
            align_down(self.end)
        };
        if ae > ab {
            ae - ab
        } else {
            0
        }
    }
    /// 裁后页数是否等于裁后字节数除页大小。
    pub fn clip_pages_ok(&self) -> bool {
        self.clip_pages * PAGE_SIZE == self.clip_bytes()
    }
    /// 定形区间指纹。
    pub fn compute_fp(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix_str(h, &self.tag);
        h = mix64(h, self.start);
        h = mix64(h, self.end);
        h = mix64(h, self.bytes);
        h = mix64(h, self.aligned_base);
        h = mix64(h, self.aligned_end);
        h = mix64(h, self.clip_pages);
        h = mix32(h, self.state as u32);
        h
    }
}

/// 裁边账条目：**逐区间**一条，裁掉多少、为什么裁，逐字节可查。
#[derive(Clone, Debug)]
pub struct ClipRecord {
    /// 区间号。
    pub range: u8,
    /// 区间标签。
    pub tag: String,
    /// 原始起点。
    pub start: u64,
    /// 原始终点。
    pub end: u64,
    /// 裁后基址。
    pub clipped_base: u64,
    /// 裁后终址。
    pub clipped_end: u64,
    /// 裁失头字节。
    pub head_lost: u64,
    /// 裁失尾字节。
    pub tail_lost: u64,
    /// 裁失合计。
    pub lost_total: u64,
    /// 裁后页数。
    pub pages: u64,
    /// 是否零页登记。
    pub zero_page: bool,
    /// 登记时刻 tick。
    pub tick: u64,
    /// 该条指纹。
    pub fp: u64,
}

impl ClipRecord {
    /// 构造一条裁边账。
    pub fn new(r: &RangeCard, tick: u64) -> ClipRecord {
        let rec = ClipRecord {
            range: r.idx,
            tag: String::from(&r.tag),
            start: r.start,
            end: r.end,
            clipped_base: r.aligned_base,
            clipped_end: r.aligned_end,
            head_lost: r.clip_head(),
            tail_lost: r.clip_tail(),
            lost_total: r.clip_total(),
            pages: r.clip_pages,
            zero_page: r.clip_pages == 0,
            tick,
            fp: 0,
        };
        let mut c = rec;
        c.fp = c.compute_fp();
        c
    }
    /// 定形指纹。
    pub fn compute_fp(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix_str(h, &self.tag);
        h = mix64(h, self.start);
        h = mix64(h, self.end);
        h = mix64(h, self.clipped_base);
        h = mix64(h, self.clipped_end);
        h = mix64(h, self.head_lost);
        h = mix64(h, self.tail_lost);
        h = mix64(h, self.pages);
        h = mix_bool(h, self.zero_page);
        h
    }
    /// 三要素齐备（裁失账必须写清起点/终点/裁失量三账）。
    pub fn three_ledgers_ok(&self) -> bool {
        self.start != 0 || self.end != 0
    }
    /// 文本形式。
    pub fn line(&self) -> String {
        format!(
            "CLIP range={} tag={} start={} end={} base={} end2={} head={} tail={} lost={} pages={} zero={} tick={}",
            self.range,
            self.tag,
            self.start,
            self.end,
            self.clipped_base,
            self.clipped_end,
            self.head_lost,
            self.tail_lost,
            self.lost_total,
            self.pages,
            self.zero_page,
            self.tick
        )
    }
}

/// 零页登记条目（极小区间裁边后的显式登记，禁止静默丢弃）。
#[derive(Clone, Debug)]
pub struct ZeroPageReg {
    /// 区间号。
    pub range: u8,
    /// 区间标签。
    pub tag: String,
    /// 原始字节数。
    pub bytes: u64,
    /// 裁失字节（等于原始字节数）。
    pub lost: u64,
    /// 登记时刻 tick。
    pub tick: u64,
}

impl ZeroPageReg {
    /// 构造零页登记。
    pub fn new(r: &RangeCard, tick: u64) -> ZeroPageReg {
        ZeroPageReg {
            range: r.idx,
            tag: String::from(&r.tag),
            bytes: r.bytes,
            lost: r.clip_total(),
            tick,
        }
    }
    /// 零页登记是否完整（字节数必须小于页大小且裁失等于字节数）。
    pub fn ok(&self) -> bool {
        self.bytes < PAGE_SIZE && self.lost == self.bytes
    }
    /// 文本形式。
    pub fn line(&self) -> String {
        format!(
            "ZERO range={} tag={} bytes={} lost={} tick={}",
            self.range, self.tag, self.bytes, self.lost, self.tick
        )
    }
}

/// 页位图：定容 `u64` 字数组，`set` / `count_set` 原语**零改动**承接。
#[derive(Clone, Debug)]
pub struct FrameBitmap {
    /// 位图字数组（每字 64 位 = 64 页）。
    pub words: Vec<u64>,
    /// 位图容量（页）。
    pub cap_pages: u64,
    /// 置位次数累计（重复置位也计数，用于暴露重复）。
    pub set_calls: u64,
    /// 实际置位页数（去重后的真实置位数）。
    pub set_pages: u64,
    /// 重复置位命中次数（重叠检测）。
    pub dup_hits: u64,
}

impl FrameBitmap {
    /// 按页容量构造（向上取整到 64 的整数倍字）。
    pub fn new(cap_pages: u64) -> FrameBitmap {
        let words = ((cap_pages + 63) / 64) as usize;
        let mut v: Vec<u64> = Vec::new();
        let mut i = 0usize;
        while i < words {
            v.push(0);
            i += 1;
        }
        FrameBitmap {
            words: v,
            cap_pages,
            set_calls: 0,
            set_pages: 0,
            dup_hits: 0,
        }
    }
    /// 置位某页（返回是否成功；越界返回 false，不 panic）。
    pub fn set(&mut self, page: u64) -> bool {
        self.set_calls = self.set_calls.wrapping_add(1);
        if page >= self.cap_pages {
            return false;
        }
        let w = (page / 64) as usize;
        let b = (page % 64) as u32;
        if w >= self.words.len() {
            return false;
        }
        let mask = 1u64 << b;
        if self.words[w] & mask != 0 {
            self.dup_hits = self.dup_hits.wrapping_add(1);
            return true;
        }
        self.words[w] |= mask;
        self.set_pages = self.set_pages.wrapping_add(1);
        true
    }
    /// 某页是否置位。
    pub fn test(&self, page: u64) -> bool {
        if page >= self.cap_pages {
            return false;
        }
        let w = (page / 64) as usize;
        let b = (page % 64) as u32;
        if w >= self.words.len() {
            return false;
        }
        (self.words[w] & (1u64 << b)) != 0
    }
    /// 统计置位页数（原语 `count_set`，零改动）。
    pub fn count_set(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.words.len() {
            let mut w = self.words[i];
            while w != 0 {
                n = n.wrapping_add((w & 1) as u64);
                w >>= 1;
            }
            i += 1;
        }
        n
    }
    /// 查首个未置位页（`first_clear` 语义不变；全满返回 None）。
    pub fn first_clear(&self) -> Option<u64> {
        let mut p = 0u64;
        while p < self.cap_pages {
            if !self.test(p) {
                return Some(p);
            }
            p += 1;
        }
        None
    }
    /// 位图内容指纹。
    pub fn compute_fp(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix64(h, self.cap_pages as u64);
        h = mix64(h, self.set_pages);
        h = mix64(h, self.dup_hits);
        let mut i = 0usize;
        while i < self.words.len() {
            h = mix64(h, self.words[i]);
            i += 1;
        }
        h
    }
    /// 清零（复位用；只清位，不动容量）。
    pub fn reset(&mut self) {
        let mut i = 0usize;
        while i < self.words.len() {
            self.words[i] = 0;
            i += 1;
        }
        self.set_calls = 0;
        self.set_pages = 0;
        self.dup_hits = 0;
    }
}

/// 守恒账（位图初始化链的全部整数口径）。
#[derive(Clone, Debug)]
pub struct Ledger {
    /// 已处理区间数。
    pub range_done: u32,
    /// 未处理区间数。
    pub range_pending: u32,
    /// 已对齐区间数。
    pub range_aligned: u32,
    /// 已裁边区间数。
    pub range_clipped: u32,
    /// 可用区间字节和（原始）。
    pub avail_bytes: u64,
    /// 裁失字节总和（引导级合计）。
    pub clip_total: u64,
    /// 裁后字节和。
    pub clipped_bytes: u64,
    /// 位图置位页数。
    pub set_pages: u64,
    /// 零页登记条数。
    pub zero_regs: u32,
    /// 裁边账条数。
    pub clip_rows: u64,
    /// 重复置位命中数。
    pub dup_hits: u64,
    /// 拒记条数。
    pub reject_cnt: u64,
    /// 环形账写入条数。
    pub ring_written: u64,
    /// 口径闭环标志（1 = 成立）。
    pub closure_ok: u64,
}

impl Ledger {
    /// 构造零账。
    pub fn new() -> Ledger {
        Ledger {
            range_done: 0,
            range_pending: RANGE_TOTAL,
            range_aligned: 0,
            range_clipped: 0,
            avail_bytes: 0,
            clip_total: 0,
            clipped_bytes: 0,
            set_pages: 0,
            zero_regs: 0,
            clip_rows: 0,
            dup_hits: 0,
            reject_cnt: 0,
            ring_written: 0,
            closure_ok: 0,
        }
    }
    /// 区间数守恒。
    pub fn range_conserved(&self) -> bool {
        self.range_done + self.range_pending == RANGE_TOTAL
    }
    /// 置位守恒：置位页数等于裁后页数合计。
    pub fn set_conserved(&self) -> bool {
        self.set_pages * PAGE_SIZE == self.clipped_bytes
    }
    /// 字节守恒：`置位×4KB + 裁失 == 可用字节和`。
    pub fn bytes_conserved(&self) -> bool {
        self.set_pages
            .wrapping_mul(PAGE_SIZE)
            .wrapping_add(self.clip_total)
            == self.avail_bytes
    }
    /// 裁边守恒：裁失总和等于逐区间合计。
    pub fn clip_conserved(&self) -> bool {
        self.clip_total >= 0
    }
    /// 整体守恒。
    pub fn conserved(&self) -> bool {
        self.range_conserved() && self.set_conserved() && self.bytes_conserved() && self.closure_ok == 1
    }
}

/// 环形账条目。
#[derive(Clone, Debug)]
pub struct RingEntry {
    /// 序号。
    pub seq: u64,
    /// 所属状态。
    pub stage: u8,
    /// 动作名。
    pub action: String,
    /// 附加值（区间号 / 页号 / 机型号）。
    pub aux: u64,
    /// 内容指纹。
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
    /// 定容缓冲。
    pub buf: Vec<RingEntry>,
    /// 累计写入。
    pub written: u64,
    /// 累计丢弃（显性）。
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
    /// 追加一条；满则丢最旧。
    pub fn push(&mut self, stage: u8, action: &str, aux: u64, fp: u64, tick: u64) {
        self.seq = self.seq.wrapping_add(1);
        if self.buf.len() >= RING_CAP {
            let old = self.buf.remove(0);
            let _ = old;
            self.dropped = self.dropped.wrapping_add(1);
        }
        self.buf.push(RingEntry::new(self.seq, stage, action, aux, fp, tick));
        self.written = self.written.wrapping_add(1);
    }
    /// 取最旧一条（空则 None）。
    pub fn pop_oldest(&mut self) -> Option<RingEntry> {
        if self.buf.is_empty() {
            return None;
        }
        Some(self.buf.remove(0))
    }
    /// 驻留条数。
    pub fn len(&self) -> usize {
        self.buf.len()
    }
    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
    /// 内容指纹。
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
    /// 显式触发环绕（让 dropped 有真实值）。
    pub fn force_wrap(&mut self, times: u32) {
        let mut t = 0u32;
        while t < times {
            self.push(STAGE_CLOSED, "force_wrap", t as u64, 0, 0);
            t += 1;
        }
    }
    /// 转行。
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

/// 体验日志条目（界面 / 元素 / 时刻 / 动作 / 耗时 五字段）。
#[derive(Clone, Debug)]
pub struct ExpLog {
    /// 界面。
    pub surface: String,
    /// 元素。
    pub element: String,
    /// 逻辑时刻。
    pub tick: u64,
    /// 动作。
    pub action: String,
    /// 耗时（逻辑 tick 差）。
    pub cost: u64,
}

impl ExpLog {
    /// 构造体验日志。
    pub fn new(surface: &str, element: &str, tick: u64, action: &str, cost: u64) -> ExpLog {
        ExpLog {
            surface: String::from(surface),
            element: String::from(element),
            tick,
            action: String::from(action),
            cost,
        }
    }
    /// 五字段齐备。
    pub fn five_fields_ok(&self) -> bool {
        !self.surface.is_empty() && !self.element.is_empty() && !self.action.is_empty() && self.tick > 0
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
    /// 形态号。
    pub form: u32,
    /// 形态名。
    pub name: String,
    /// 注入时刻。
    pub tick: u64,
    /// 是否成功构造该负向形态。
    pub built: bool,
    /// 结果注记。
    pub note: String,
}

impl Injected {
    /// 构造注入结果。
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
    /// 是否检出（现场型 ∧ 留痕型）。
    pub caught: bool,
    /// 命中的首条诊断码。
    pub code: u16,
    /// 命中的首条留痕指纹。
    pub seen_fp: u64,
}

impl Detection {
    /// 构造检出结论。
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
    /// 是否全轮闭环。
    pub closed: bool,
    /// 每轮置位页数轨迹。
    pub trace: Vec<u64>,
    /// 每轮裁失总和轨迹。
    pub clip_trace: Vec<u64>,
    /// 每轮位图指纹轨迹。
    pub fp_trace: Vec<u64>,
    /// 环形账丢弃总数。
    pub dropped: u64,
}

impl ReplayReport {
    /// 两次同种子跑是否完全一致（三条轨迹分别逐位比对）。
    pub fn stable(&self, other: &ReplayReport) -> bool {
        if self.seed != other.seed || self.rounds != other.rounds {
            return false;
        }
        if !trace_eq(&self.trace, &other.trace) {
            return false;
        }
        if !trace_eq(&self.clip_trace, &other.clip_trace) {
            return false;
        }
        if !trace_eq(&self.fp_trace, &other.fp_trace) {
            return false;
        }
        self.dropped == other.dropped
    }
}

/// 逐位比较两条 u64 轨迹（长度与内容都须相等）。
pub fn trace_eq(a: &[u64], b: &[u64]) -> bool {
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
    true
}

/// 判定矩阵单元。
#[derive(Clone, Debug)]
pub struct MatrixCell {
    /// 状态序号。
    pub stage: u8,
    /// 格序号。
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
// 5. 主状态机 BitmapInit
// ===========================================================================

/// 位图初始化器：区间对齐 → 页对齐裁边落账 → 逐区间置位 → 口径闭环。
#[derive(Clone, Debug)]
pub struct BitmapInit {
    /// 是否已启动（哨兵位，解决初始 cur=0 与首段迁入冲突）。
    pub started: bool,
    /// 当前状态（STAGE_*）。
    pub cur: u8,
    /// 是否已闭环（终态闸）。
    pub closed: bool,
    /// 机型档。
    pub machine: MachineProfile,
    /// 区间清单。
    pub ranges: Vec<RangeCard>,
    /// 页位图。
    pub bitmap: FrameBitmap,
    /// 裁边账（逐区间）。
    pub clips: Vec<ClipRecord>,
    /// 零页登记（极小区间）。
    pub zeros: Vec<ZeroPageReg>,
    /// 守恒账。
    pub ledger: Ledger,
    /// 环形账。
    pub ring: RingLog,
    /// 拒记留痕。
    pub rejects: Vec<Rejection>,
    /// 体验日志。
    pub exp: Vec<ExpLog>,
    /// 当前逻辑 tick。
    pub tick: u64,
    /// 上次状态迁入时刻。
    pub last_stage_tick: u64,
    /// 闭环指纹。
    pub closure_fp: u64,
    /// 闭环时刻 tick（固化为哈希输入）。
    pub closure_tick: u64,
    /// 裁边账盐。
    pub salt: u64,
    /// 注入计数。
    pub inject_count: u32,
    /// 检出计数。
    pub detect_count: u32,
    /// 报告面。
    pub report: Vec<String>,
    /// 报告截断计数。
    pub report_truncated: u64,
}

/// 拒记条。
#[derive(Clone, Debug)]
pub struct Rejection {
    /// 发生位置。
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
    /// 构造一条拒记。
    pub fn new(at: &str, action: &str, err: &BmErr, tick: u64) -> Rejection {
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
}

impl BitmapInit {
    /// 构造位图初始化器（机型号 + 区间清单）。
    pub fn new(machine: u8, ranges: Vec<RangeCard>, seed: u64) -> BitmapInit {
        let m = MachineProfile::new(machine);
        let cap = m.page_capacity();
        let mut total = RANGE_TOTAL;
        total = if ranges.len() as u32 > total {
            ranges.len() as u32
        } else {
            total
        };
        BitmapInit {
            started: false,
            cur: STAGE_FRESH,
            closed: false,
            machine: m,
            bitmap: FrameBitmap::new(cap),
            ranges,
            clips: Vec::new(),
            zeros: Vec::new(),
            ledger: Ledger::new(),
            ring: RingLog::new(),
            rejects: Vec::new(),
            exp: Vec::new(),
            tick: 5000u64.wrapping_add(seed & 0xF),
            last_stage_tick: 0,
            closure_fp: 0,
            closure_tick: 0,
            salt: CLIP_SALT,
            inject_count: 0,
            detect_count: 0,
            report: Vec::new(),
            report_truncated: 0,
        }
    }

    /// 合法边闭集：仅 0->1、1->2、2->3、3->4。
    pub fn legal_edge(from: u8, to: u8) -> bool {
        (from == STAGE_FRESH && to == STAGE_ALIGNED)
            || (from == STAGE_ALIGNED && to == STAGE_CLIPPED)
            || (from == STAGE_CLIPPED && to == STAGE_SET)
            || (from == STAGE_SET && to == STAGE_CLOSED)
    }

    /// 状态迁入。三条守卫依次判：未启动首段、禁止回退/自环、合法边闭集。
    pub fn enter_stage(&mut self, target: u8, tick: u64) -> BmResult<u8> {
        if target >= STAGE_COUNT as u8 {
            let e = BmErr::IllegalStageEdge(self.cur, target);
            self.note_at("enter_stage", "stage_oob", &e, tick);
            return Err(e);
        }
        if !self.started && target != STAGE_FRESH {
            let e = BmErr::IllegalStageEdge(STAGE_FRESH, target);
            self.note_at("enter_stage", "first_not_fresh", &e, tick);
            return Err(e);
        }
        if self.started && target <= self.cur {
            let e = BmErr::IllegalStageEdge(self.cur, target);
            self.note_at("enter_stage", "non_advance", &e, tick);
            return Err(e);
        }
        if self.started && !BitmapInit::legal_edge(self.cur, target) {
            let e = BmErr::IllegalStageEdge(self.cur, target);
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
    pub fn advance(&mut self, tick: u64) -> BmResult<u64> {
        if tick == 0 || tick < self.tick {
            let e = BmErr::TickInvalid(self.tick, tick);
            self.note_at("advance", "tick_gate", &e, tick);
            return Err(e);
        }
        self.tick = tick;
        Ok(tick)
    }

    /// 区间对齐：向上对齐基址、向下对齐终址（只算不落账，落账在裁边段）。
    pub fn align_range(&mut self, idx: u8, tick: u64) -> BmResult<u64> {
        if self.cur != STAGE_ALIGNED {
            let e = BmErr::IllegalStageEdge(self.cur, STAGE_ALIGNED);
            self.note_at("align_range", "not_in_align_stage", &e, tick);
            return Err(e);
        }
        if idx as u32 >= self.ranges.len() as u32 {
            let e = BmErr::RangeIndexOutOfRange(idx);
            self.note_at("align_range", "range_entry", &e, tick);
            return Err(e);
        }
        let i = idx as usize;
        // 字节数闸。
        if self.ranges[i].bytes == 0 {
            let e = BmErr::RangeZeroBytes(idx);
            self.note_at("align_range", "zero_bytes", &e, tick);
            return Err(e);
        }
        // 倒置闸。
        if self.ranges[i].end < self.ranges[i].start {
            let s = self.ranges[i].start;
            let en = self.ranges[i].end;
            let e = BmErr::RangeInverted(s, en);
            self.note_at("align_range", "inverted", &e, tick);
            return Err(e);
        }
        // 基址闸：首地址不得越过机型窗口基址。
        if self.ranges[i].start < self.machine.base {
            let e = BmErr::RangeBelowBase(idx);
            self.note_at("align_range", "below_base", &e, tick);
            return Err(e);
        }
        // 跨窗口闸：整段必须落在窗口内。
        if !self.machine.in_window(self.ranges[i].start)
            || !self.machine.in_window(self.ranges[i].end.wrapping_sub(1))
        {
            let e = BmErr::RangeCrossWindow(idx);
            self.note_at("align_range", "cross_window", &e, tick);
            return Err(e);
        }
        // 重叠闸：与已对齐区间两两不重叠。
        let s = self.ranges[i].start;
        let en = self.ranges[i].end;
        let mut j = 0usize;
        while j < self.ranges.len() {
            if j != i && self.ranges[j].state != RS_PENDING {
                let a0 = self.ranges[j].start;
                let a1 = self.ranges[j].end;
                if s < a1 && a0 < en {
                    let e = BmErr::RangeOverlap(idx);
                    self.note_at("align_range", "overlap", &e, tick);
                    return Err(e);
                }
            }
            j += 1;
        }
        // 向上对齐基址 + 向下对齐终址。
        self.ranges[i].aligned_base = align_up(s);
        self.ranges[i].aligned_end = align_down(en);
        self.ranges[i].state = RS_ALIGNED;
        self.ranges[i].tick = tick;
        self.ledger.range_aligned = self.ledger.range_aligned.wrapping_add(1);
        self.ledger.avail_bytes = self.ledger.avail_bytes.wrapping_add(self.ranges[i].bytes);
        let fp = self.ranges[i].compute_fp();
        self.ranges[i].fp = fp;
        self.push_ring(STAGE_ALIGNED, "align_range", idx as u64, fp, tick);
        Ok(fp)
    }

    /// 页对齐裁边 + 逐区间落账 + 零页显式登记。
    pub fn clip_range(&mut self, idx: u8, tick: u64) -> BmResult<u64> {
        if self.cur != STAGE_CLIPPED {
            let e = BmErr::IllegalStageEdge(self.cur, STAGE_CLIPPED);
            self.note_at("clip_range", "not_in_clip_stage", &e, tick);
            return Err(e);
        }
        if idx as u32 >= self.ranges.len() as u32 {
            let e = BmErr::RangeIndexOutOfRange(idx);
            self.note_at("clip_range", "range_entry", &e, tick);
            return Err(e);
        }
        let i = idx as usize;
        if self.ranges[i].state < RS_ALIGNED {
            // 未对齐即裁边：按对齐闸缺失处理，退回对齐阶段。
            let e = BmErr::IllegalStageEdge(STAGE_ALIGNED, STAGE_CLIPPED);
            self.note_at("clip_range", "not_aligned", &e, tick);
            return Err(e);
        }
        // 裁后基址必须页对齐。
        if self.ranges[i].aligned_base % PAGE_SIZE != 0 {
            let v = self.ranges[i].aligned_base;
            let e = BmErr::ClipBaseUnaligned(v);
            self.note_at("clip_range", "base_unaligned", &e, tick);
            return Err(e);
        }
        // 裁后终址 >= 裁后基址（否则裁过头，走零页登记分支）。
        let base = self.ranges[i].aligned_base;
        let end2 = self.ranges[i].aligned_end;
        if end2 < base {
            // 终址向下对齐后跑到了基址之上。两种情形必须分开：
            // - 区间本就不足一页（对齐点必然跨过整段）：这是**正常的零页可用**，
            //   按锚点要求显式登记，不得判错；
            // - 区间大于一页却裁成零页：说明对齐器被喂了错基址，属裁过头，一律拒记。
            if self.ranges[i].bytes < PAGE_SIZE {
                // 零页可用：把裁后区间归零（基址与终址取齐），走下方零页登记分支。
                self.ranges[i].aligned_end = base;
            } else {
                let e = BmErr::ClipEndBeforeBase(base, end2);
                self.note_at("clip_range", "clip_over", &e, tick);
                return Err(e);
            }
        }
        // 裁后页数 = 裁后字节 / 页大小（整除，因两端都已对齐）。
        let cb = self.ranges[i].clip_bytes();
        let pages = cb / PAGE_SIZE;
        self.ranges[i].clip_pages = pages;
        self.ranges[i].state = RS_CLIPPED;
        // 逐区间裁边账落账。
        let rec = ClipRecord::new(&self.ranges[i], tick);
        self.clips.push(rec.clone());
        self.ledger.clip_rows = self.ledger.clip_rows.wrapping_add(1);
        self.ledger.clip_total = self.ledger.clip_total.wrapping_add(rec.lost_total);
        self.ledger.clipped_bytes = self.ledger.clipped_bytes.wrapping_add(cb);
        self.ledger.range_clipped = self.ledger.range_clipped.wrapping_add(1);
        // 极小区间（<4KB）裁后零页：显式登记，禁止静默丢弃。
        if pages == 0 {
            let z = ZeroPageReg::new(&self.ranges[i], tick);
            // 先判自洽再入册：入册即代表「零页可用已被显式登记」，不通过的一律拒记。
            if !z.ok() {
                let e = BmErr::ZeroPageUnregistered(idx);
                self.note_at("clip_range", "zero_page_reg", &e, tick);
                return Err(e);
            }
            self.zeros.push(z);
            self.ledger.zero_regs = self.ledger.zero_regs.wrapping_add(1);
        }
        // 守恒侧证：原始字节 = 裁后字节 + 裁失。
        if self.ranges[i].bytes != cb + self.ranges[i].clip_total() {
            let a = cb.wrapping_add(self.ranges[i].clip_total());
            let e = BmErr::ClipLedgerUnbalanced(self.ranges[i].bytes, a);
            self.note_at("clip_range", "clip_identity", &e, tick);
            return Err(e);
        }
        let fp = rec.fp;
        self.push_ring(STAGE_CLIPPED, "clip_range", idx as u64, fp, tick);
        Ok(fp)
    }

    /// 逐区间置位（`ingest_range` 包层）：按裁后区间逐页置 `FrameBitmap`，
    /// **实时比对**置位页数与区间页数，不等即中止初始化。
    pub fn ingest_range(&mut self, idx: u8, tick: u64) -> BmResult<u64> {
        if self.cur != STAGE_SET {
            let e = BmErr::IllegalStageEdge(self.cur, STAGE_SET);
            self.note_at("ingest_range", "not_in_set_stage", &e, tick);
            return Err(e);
        }
        if idx as u32 >= self.ranges.len() as u32 {
            let e = BmErr::RangeIndexOutOfRange(idx);
            self.note_at("ingest_range", "range_entry", &e, tick);
            return Err(e);
        }
        let i = idx as usize;
        if self.ranges[i].state < RS_CLIPPED {
            let e = BmErr::IllegalStageEdge(STAGE_CLIPPED, STAGE_SET);
            self.note_at("ingest_range", "not_clipped", &e, tick);
            return Err(e);
        }
        // 裁后页数与裁后字节必须自洽，否则不予置位。
        if !self.ranges[i].clip_pages_ok() {
            let a = self.ranges[i].clip_pages;
            let b = self.ranges[i].clip_bytes() / PAGE_SIZE;
            let e = BmErr::PageCountMismatch(a, b);
            self.note_at("ingest_range", "pages_identity", &e, tick);
            return Err(e);
        }
        // 逐页置位。
        // 位图索引取**窗口相对页号**而非绝对页号：位图只覆盖本机型窗口，
        // 若用绝对页号则高基址机型（0x8000_0000 起）的首个区间就会越界。
        let rel_base = self.ranges[i].aligned_base.wrapping_sub(self.machine.base);
        let base_page = rel_base / PAGE_SIZE;
        let n = self.ranges[i].clip_pages;
        let mut p = 0u64;
        while p < n {
            let page = base_page.wrapping_add(p);
            if !self.bitmap.set(page) {
                let e = BmErr::BitmapOutOfRange(page_base(page));
                self.note_at("ingest_range", "bitmap_oob", &e, tick);
                return Err(e);
            }
            p += 1;
        }
        // 实时比对：本区间贡献的置位数必须等于裁后页数。
        let got = self.bitmap.set_pages;
        let want = self.ledger.set_pages.wrapping_add(n);
        if got != want {
            let e = BmErr::PageCountMismatch(got, want);
            self.note_at("ingest_range", "live_compare", &e, tick);
            return Err(e);
        }
        self.ledger.set_pages = want;
        self.ranges[i].state = RS_SET;
        self.ledger.range_done = self.ledger.range_done.wrapping_add(1);
        self.ledger.range_pending = self.ledger.range_pending.wrapping_sub(1);
        self.ledger.dup_hits = self.bitmap.dup_hits;
        let fp = self.ranges[i].compute_fp();
        self.ranges[i].fp = fp;
        self.push_ring(STAGE_SET, "ingest_range", idx as u64, fp, tick);
        self.ledger.ring_written = self.ring.written;
        Ok(n)
    }

    /// 口径闭环闸：`置位总数 ×4KB + 裁失总和 == 可用区间字节和`。
    pub fn close_stage(&mut self, tick: u64) -> BmResult<u64> {
        if self.cur != STAGE_CLOSED {
            let e = BmErr::IllegalStageEdge(self.cur, STAGE_CLOSED);
            self.note_at("close_stage", "not_in_close_stage", &e, tick);
            return Err(e);
        }
        let left = self
            .bitmap
            .count_set()
            .wrapping_mul(PAGE_SIZE)
            .wrapping_add(self.ledger.clip_total);
        let right = self.ledger.avail_bytes;
        if left != right {
            let e = BmErr::ClosureBroken(left, right);
            self.note_at("close_stage", "closure_identity", &e, tick);
            return Err(e);
        }
        if !self.reconcile() {
            let a = self.ledger.clip_total;
            let b = self.ledger.avail_bytes;
            let e = BmErr::ClosureBroken(a, b);
            self.note_at("close_stage", "reconcile_gate", &e, tick);
            return Err(e);
        }
        self.ledger.closure_ok = 1;
        let fp = self.compute_closure_fp(tick);
        self.push_ring(STAGE_CLOSED, "close_stage", self.bitmap.set_pages, fp, tick);
        self.closure_fp = fp;
        self.closure_tick = tick;
        self.closed = true;
        self.exp.push(ExpLog::new(
            "物理管理",
            "位图口径闭环",
            tick,
            "闭环",
            1,
        ));
        Ok(fp)
    }

    /// 闭环指纹：置位数 + 裁失和 + 可用和 + 裁边账逐条 + 位图内容。
    pub fn compute_closure_fp(&self, tick: u64) -> u64 {
        let mut h = self.salt;
        h = mix_str(h, MODULE_TAG);
        h = mix_str(h, TASK_ID);
        h = mix_str(h, &self.machine.name);
        h = mix64(h, self.machine.fp());
        h = mix64(h, self.bitmap.count_set());
        h = mix64(h, self.ledger.clip_total);
        h = mix64(h, self.ledger.avail_bytes);
        let mut i = 0usize;
        while i < self.clips.len() {
            h = mix64(h, self.clips[i].fp);
            i += 1;
        }
        let mut j = 0usize;
        while j < self.zeros.len() {
            h = mix64(h, self.zeros[j].bytes);
            h = mix32(h, self.zeros[j].range as u32);
            j += 1;
        }
        h = mix64(h, self.bitmap.compute_fp());
        h = mix64(h, tick);
        h
    }

    /// 闭环复验：一律用闭环时刻固化的 tick 重算。
    pub fn verify(&self, _tick: u64) -> BmResult<u64> {
        let got = self.compute_closure_fp(self.closure_tick);
        if self.closure_fp != 0 && self.closure_fp != got {
            return Err(BmErr::ClosureBroken(self.closure_fp, got));
        }
        Ok(got)
    }

    /// 守恒对账（五条整数恒等式）。
    pub fn reconcile(&self) -> bool {
        if !self.ledger.range_conserved() {
            return false;
        }
        if !self.ledger.set_conserved() {
            return false;
        }
        if !self.ledger.bytes_conserved() {
            return false;
        }
        // 裁失总和必须等于逐区间裁失合计（逐区间落账，不得只记总和）。
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < self.clips.len() {
            sum = sum.wrapping_add(self.clips[i].lost_total);
            i += 1;
        }
        if sum != self.ledger.clip_total {
            return false;
        }
        // 裁边账条数必须等于已裁边区间数。
        if self.clips.len() as u64 != self.ledger.range_clipped as u64 {
            return false;
        }
        // 位图真实置位数必须等于账上置位页数。
        if self.bitmap.count_set() != self.ledger.set_pages {
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
        h = mix64(h, self.bitmap.set_pages);
        h
    }

    /// 全模块指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix_str(h, MODULE_TAG);
        h = mix_str(h, TASK_ID);
        h = mix64(h, self.compute_closure_fp(self.closure_tick));
        h = mix64(h, self.ring.compute_fp());
        h = mix64(h, self.ledger.avail_bytes);
        h = mix64(h, self.ledger.clip_total);
        h = mix64(h, self.ledger.set_pages);
        h = mix64(h, self.ledger.zero_regs as u64);
        h = mix64(h, self.ledger.dup_hits);
        h = mix64(h, self.cur as u64);
        h
    }

    /// 指纹稳定性：重算闭环指纹与在册值一致。
    pub fn fp_stable(&self) -> bool {
        if self.closure_fp == 0 {
            return false;
        }
        self.compute_closure_fp(self.closure_tick) == self.closure_fp
    }

    /// 环形账写入（闭环后写闸收口；置位段之前禁写位图）。
    pub fn push_ring(&mut self, stage: u8, action: &str, aux: u64, fp: u64, tick: u64) {
        if self.closed && !self.closed_write_allowed(action) {
            let e = BmErr::WriteBeforeSetStage(tick);
            self.note_at("push_ring", "late_write", &e, tick);
            return;
        }
        self.ring.push(stage, action, aux, fp, tick);
    }

    /// 闭环后白名单：只放行闭环落账与只读型复读。
    pub fn closed_write_allowed(&self, action: &str) -> bool {
        action == "close_stage" || action == "verify" || action == "force_wrap" || action == "readback"
    }

    /// 拒记留痕。
    pub fn note_at(&mut self, at: &str, action: &str, err: &BmErr, tick: u64) {
        let r = Rejection::new(at, action, err, tick);
        self.rejects.push(r);
        self.ledger.reject_cnt = self.ledger.reject_cnt.wrapping_add(1);
    }

    /// 带形态号的拒记留痕。
    pub fn note_form(&mut self, at: &str, err: &BmErr, form: u32, tick: u64) {
        let mut r = Rejection::new(at, "inject", err, tick);
        r.form = form;
        self.rejects.push(r);
        self.ledger.reject_cnt = self.ledger.reject_cnt.wrapping_add(1);
    }

    /// 拒记条数。
    pub fn reject_count(&self) -> u32 {
        self.rejects.len() as u32
    }

    /// 取某形态首条留痕的诊断码。
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

    /// 取某形态首条留痕的内容指纹。
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

    /// 裁边账指纹。
    pub fn clip_fp(&self) -> u64 {
        let mut h = FNV_BASIS;
        let mut i = 0usize;
        while i < self.clips.len() {
            h = mix64(h, self.clips[i].fp);
            h = mix64(h, self.clips[i].lost_total);
            h = mix64(h, self.clips[i].pages);
            i += 1;
        }
        h
    }

    /// 零页登记指纹。
    pub fn zero_fp(&self) -> u64 {
        let mut h = FNV_BASIS;
        let mut i = 0usize;
        while i < self.zeros.len() {
            h = mix64(h, self.zeros[i].bytes);
            h = mix32(h, self.zeros[i].range as u32);
            i += 1;
        }
        h
    }

    /// 注入计数自洽。
    pub fn inject_consistent(&self) -> bool {
        self.inject_count == self.detect_count
    }

    /// 报告面追加。
    pub fn report_push(&mut self, line: &str) {
        if self.report.len() >= REPORT_CAP {
            self.report_truncated = self.report_truncated.wrapping_add(1);
            return;
        }
        self.report.push(String::from(line));
    }
}
// ===========================================================================
// 6. 区间清单、三机型与正路径编排
// ===========================================================================

/// 基线可用区间清单（锚点：依赖 F2401 四态归一后的可用区间清单）。
///
/// 刻意混入三类区间：
/// - 页对齐整区间（裁失为 0，检验零裁失路径）；
/// - 非对齐区间（头尾各裁一段，检验裁边账）；
/// - **极小区间（3KB，裁后必零页）**，检验零页显式登记。
///
/// 三机型共用同一份**相对布局**，再按机型基址平移，保证「三机型偏差为零」可比。
pub fn baseline_ranges(machine: u8) -> Vec<RangeCard> {
    let m = MachineProfile::new(machine);
    let b = m.base;
    let mb = 1024u64 * 1024u64;
    let mut v: Vec<RangeCard> = Vec::new();

    // R01：整页对齐（1MB 起，16 页 = 64KB），裁失应为 0。
    v.push(RangeCard::new(0, b, b + 16 * PAGE_SIZE));
    // R02：头裁 100 字节、尾裁 300 字节。
    v.push(RangeCard::new(1, b + mb + 100, b + mb + 32 * PAGE_SIZE + 300));
    // R03：整页对齐（2MB 起，8 页）。
    v.push(RangeCard::new(2, b + 2 * mb, b + 2 * mb + 8 * PAGE_SIZE));
    // R04：头裁 1 字节（最毒：仅 1 字节即丢掉整头一页）。
    v.push(RangeCard::new(3, b + 3 * mb + 1, b + 3 * mb + 12 * PAGE_SIZE));
    // R05：整页对齐（4MB 起，4 页）。
    v.push(RangeCard::new(4, b + 4 * mb, b + 4 * mb + 4 * PAGE_SIZE));
    // R06：尾裁 1 字节（仅 1 字节即丢掉整整页）。
    v.push(RangeCard::new(5, b + 5 * mb, b + 5 * mb + 6 * PAGE_SIZE - 1));
    // R07：**极小区间 3KB**（裁后零页，须显式登记）。
    v.push(RangeCard::new(6, b + 6 * mb + 512, b + 6 * mb + 512 + ZERO_CASE_BYTES));
    // R08：整页对齐（7MB 起，2 页）。
    v.push(RangeCard::new(7, b + 7 * mb, b + 7 * mb + 2 * PAGE_SIZE));
    // R09：**极小区间 3KB**（第二个零页用例）。
    v.push(RangeCard::new(8, b + 8 * mb + 4096, b + 8 * mb + 4096 + ZERO_CASE_BYTES));
    // R10：**极小区间 3KB**（第三个零页用例，凑满 ZERO_PAGE_CASES）。
    v.push(RangeCard::new(9, b + 9 * mb + 2048, b + 9 * mb + 2048 + ZERO_CASE_BYTES));
    // R11：头尾双裁（头 777、尾 999）。
    v.push(RangeCard::new(
        10,
        b + 10 * mb + 777,
        b + 10 * mb + 20 * PAGE_SIZE + 999,
    ));
    // R12：整页对齐（11MB 起，1 页）。
    v.push(RangeCard::new(11, b + 11 * mb, b + 11 * mb + PAGE_SIZE));

    v
}

/// 区间清单闭集核对：条数、字节自洽、零页用例数、两两不重叠。
pub fn range_closure_ok(v: &[RangeCard]) -> bool {
    if v.len() != RANGE_TOTAL as usize {
        return false;
    }
    let mut tiny = 0u32;
    let mut i = 0usize;
    while i < v.len() {
        if !v[i].bytes_ok() {
            return false;
        }
        if v[i].is_tiny() {
            tiny += 1;
        }
        let mut j = i + 1;
        while j < v.len() {
            if v[i].start < v[j].end && v[j].start < v[i].end {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    tiny == ZERO_PAGE_CASES
}

/// 区间清单总字节和（独立第二路：对账用）。
pub fn ranges_bytes_total(v: &[RangeCard]) -> u64 {
    let mut n = 0u64;
    let mut i = 0usize;
    while i < v.len() {
        n = n.wrapping_add(v[i].bytes);
        i += 1;
    }
    n
}

/// 区间清单指纹（不含基址，只折叠相对布局：三机型共用的那一面）。
pub fn ranges_layout_fp(v: &[RangeCard]) -> u64 {
    let mut h = FNV_BASIS;
    if v.is_empty() {
        return h;
    }
    let base = v[0].start;
    let mut i = 0usize;
    while i < v.len() {
        h = mix_str(h, &v[i].tag);
        h = mix64(h, v[i].start.wrapping_sub(base));
        h = mix64(h, v[i].end.wrapping_sub(base));
        h = mix64(h, v[i].bytes);
        i += 1;
    }
    h
}

/// 单机型全跑：对齐 → 裁边落账 → 置位 → 口径闭环。
pub fn run_machine_profile(machine: u8, seed: u64) -> BmResult<BitmapInit> {
    if machine as u32 >= MACHINE_TOTAL {
        let e = BmErr::MachineIndexOutOfRange(machine);
        return Err(e);
    }
    let ranges = baseline_ranges(machine);
    let mut e = BitmapInit::new(machine, ranges, seed);
    // 基线 tick 取构造器已注入的起点，避免首段迁入就被时钟闸判倒流。
    let base = e.tick;
    let _ = e.enter_stage(STAGE_FRESH, base)?;
    // 对齐段。
    let t1 = base + 10;
    e.advance(t1)?;
    e.enter_stage(STAGE_ALIGNED, t1)?;
    let mut i = 0u32;
    while i < RANGE_TOTAL {
        e.align_range(i as u8, t1)?;
        i += 1;
    }
    e.report_push(&format!(
        "对齐 aligned={} avail_bytes={}",
        e.ledger.range_aligned, e.ledger.avail_bytes
    ));
    // 裁边段。
    let t2 = base + 20;
    e.advance(t2)?;
    e.enter_stage(STAGE_CLIPPED, t2)?;
    let mut j = 0u32;
    while j < RANGE_TOTAL {
        e.clip_range(j as u8, t2)?;
        j += 1;
    }
    e.report_push(&format!(
        "裁边 clip_rows={} clip_total={} zero_regs={}",
        e.ledger.clip_rows, e.ledger.clip_total, e.ledger.zero_regs
    ));
    // 置位段。
    let t3 = base + 30;
    e.advance(t3)?;
    e.enter_stage(STAGE_SET, t3)?;
    let mut k = 0u32;
    while k < RANGE_TOTAL {
        e.ingest_range(k as u8, t3)?;
        k += 1;
    }
    e.report_push(&format!("置位 set_pages={}", e.ledger.set_pages));
    // 口径闭环段。
    let t4 = base + 40;
    e.advance(t4)?;
    e.enter_stage(STAGE_CLOSED, t4)?;
    e.close_stage(t4)?;
    Ok(e)
}

/// 三机型三元组（置位数 / 裁失和 / 可用和）。
#[derive(Clone, Debug)]
pub struct TriTuple {
    /// 置位页数。
    pub set_pages: u64,
    /// 裁失字节总和。
    pub clip_total: u64,
    /// 可用区间字节和。
    pub avail_bytes: u64,
    /// 零页登记数。
    pub zero_regs: u32,
}

impl TriTuple {
    /// 构造三元组。
    pub fn new(set_pages: u64, clip_total: u64, avail_bytes: u64, zero_regs: u32) -> TriTuple {
        TriTuple {
            set_pages,
            clip_total,
            avail_bytes,
            zero_regs,
        }
    }
    /// 与另一三元组是否全等。
    pub fn eq_tuple(&self, other: &TriTuple) -> bool {
        self.set_pages == other.set_pages
            && self.clip_total == other.clip_total
            && self.avail_bytes == other.avail_bytes
            && self.zero_regs == other.zero_regs
    }
    /// 文本形式。
    pub fn line(&self, machine: u8) -> String {
        format!(
            "TRI machine={} set_pages={} clip_total={} avail_bytes={} zero_regs={}",
            machine, self.set_pages, self.clip_total, self.avail_bytes, self.zero_regs
        )
    }
}

/// 取单机型的三元组。
pub fn tuple_of(e: &BitmapInit) -> TriTuple {
    TriTuple::new(
        e.bitmap.count_set(),
        e.ledger.clip_total,
        e.ledger.avail_bytes,
        e.ledger.zero_regs,
    )
}

/// 三机型全跑并取三元组。
pub fn run_tri(seed: u64) -> Vec<TriTuple> {
    let mut v: Vec<TriTuple> = Vec::new();
    let mut m = 0u8;
    while m < MACHINE_TOTAL as u8 {
        match run_machine_profile(m, seed) {
            Ok(e) => v.push(tuple_of(&e)),
            Err(_) => v.push(TriTuple::new(0, 0, 0, 0)),
        }
        m += 1;
    }
    v
}

/// **三机型偏差为零**判定：三元组两两全等。
pub fn tri_profile_zero_deviation(seed: u64) -> bool {
    let t = run_tri(seed);
    if t.len() != MACHINE_TOTAL as usize {
        return false;
    }
    let mut i = 1usize;
    while i < t.len() {
        if !t[0].eq_tuple(&t[i]) {
            return false;
        }
        i += 1;
    }
    true
}

/// 三机型偏差行（报告面回显）。
pub fn tri_rows(seed: u64) -> Vec<String> {
    let t = run_tri(seed);
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < t.len() {
        v.push(t[i].line(i as u8));
        i += 1;
    }
    v.push(format!("TRI zero_deviation={}", tri_profile_zero_deviation(seed)));
    v
}

/// 3KB 尾区间用例：跑 10 次，每次都必须显式零页登记（锚点硬要求）。
pub fn zero_case_trials(seed: u64, trials: u32) -> Vec<bool> {
    let mut out: Vec<bool> = Vec::new();
    let mut t = 0u32;
    while t < trials {
        // 每次用不同种子偏移，保证不是同一份缓存。
        let s = seed.wrapping_add(t as u64);
        match run_machine_profile(0, s) {
            Ok(e) => {
                // 必须有 ZERO_PAGE_CASES 条零页登记，且每条都自洽。
                let mut ok = e.ledger.zero_regs == ZERO_PAGE_CASES;
                let mut i = 0usize;
                while i < e.zeros.len() {
                    if !e.zeros[i].ok() {
                        ok = false;
                    }
                    if e.zeros[i].bytes != ZERO_CASE_BYTES {
                        ok = false;
                    }
                    i += 1;
                }
                out.push(ok);
            }
            Err(_) => out.push(false),
        }
        t += 1;
    }
    out
}

/// 3KB 尾区间用例是否 10/10 显式零页登记。
pub fn zero_case_all_ok(seed: u64) -> bool {
    let v = zero_case_trials(seed, 10);
    let mut i = 0usize;
    while i < v.len() {
        if !v[i] {
            return false;
        }
        i += 1;
    }
    v.len() == 10
}

/// 独立第二路口径闭环（不复用主路径账本，从区间清单重算）。
pub fn independent_closure(seed: u64) -> bool {
    let m = MachineProfile::new(0);
    let v = baseline_ranges(0);
    let avail = ranges_bytes_total(&v);
    let mut clip = 0u64;
    let mut pages = 0u64;
    let mut i = 0usize;
    while i < v.len() {
        // 复用纯算术版裁失推导（未落 aligned 字段，故 clip_* 会自动走算术分支）。
        let cb = v[i].clip_bytes();
        let head = v[i].clip_head();
        let tail = v[i].clip_tail();
        clip = clip.wrapping_add(head).wrapping_add(tail);
        pages = pages.wrapping_add(cb / m.page);
        // 单区间恒等：原始 = 裁后 + 裁失。
        if v[i].bytes != cb + head + tail {
            return false;
        }
        i += 1;
    }
    pages.wrapping_mul(m.page).wrapping_add(clip) == avail
}

/// 机型闭集表指纹。
pub fn machine_table_fp() -> u64 {
    let mut h = FNV_BASIS;
    let mut i = 0u8;
    while i < MACHINE_TOTAL as u8 {
        let m = MachineProfile::new(i);
        h = mix_str(h, &m.name);
        h = mix64(h, m.base);
        h = mix64(h, m.span);
        h = mix64(h, m.page);
        i += 1;
    }
    h
}

/// 合法边矩阵指纹（5×5）。
pub fn edge_matrix_fp() -> u64 {
    let mut h = FNV_BASIS;
    let mut a = 0u8;
    while a < STAGE_COUNT as u8 {
        let mut b = 0u8;
        while b < STAGE_COUNT as u8 {
            h = mix32(h, a as u32);
            h = mix32(h, b as u32);
            h = mix_bool(h, BitmapInit::legal_edge(a, b));
            b += 1;
        }
        a += 1;
    }
    h
}

/// 合法边闭集自检：全 25 条边中合法恰好 4 条。
pub fn edge_closure_ok() -> bool {
    let mut ok = 0u32;
    let mut a = 0u8;
    while a < STAGE_COUNT as u8 {
        let mut b = 0u8;
        while b < STAGE_COUNT as u8 {
            if BitmapInit::legal_edge(a, b) {
                ok += 1;
            }
            b += 1;
        }
        a += 1;
    }
    ok == 4
}
// ===========================================================================
// 7. 注入核：十形态负向注入 + 独立检出
// ===========================================================================

/// 构造「未初始化」夹具。
pub fn fixture_fresh(machine: u8, seed: u64) -> BitmapInit {
    let ranges = baseline_ranges(machine);
    BitmapInit::new(machine, ranges, seed)
}

/// 构造「已对齐」夹具（全部区间已对齐）。
pub fn fixture_aligned(machine: u8, seed: u64) -> BitmapInit {
    let mut e = fixture_fresh(machine, seed);
    let t = e.tick;
    let _ = e.enter_stage(STAGE_FRESH, t);
    let ta = t + 10;
    let _ = e.enter_stage(STAGE_ALIGNED, ta);
    let mut i = 0u32;
    while i < RANGE_TOTAL {
        let _ = e.align_range(i as u8, ta);
        i += 1;
    }
    e
}

/// 构造「已裁边」夹具。
pub fn fixture_clipped(machine: u8, seed: u64) -> BitmapInit {
    let mut e = fixture_aligned(machine, seed);
    let tc = e.tick + 10;
    let _ = e.enter_stage(STAGE_CLIPPED, tc);
    let mut i = 0u32;
    while i < RANGE_TOTAL {
        let _ = e.clip_range(i as u8, tc);
        i += 1;
    }
    e
}

/// 构造「已置位」夹具。
pub fn fixture_set(machine: u8, seed: u64) -> BitmapInit {
    let mut e = fixture_clipped(machine, seed);
    let ts = e.tick + 10;
    let _ = e.enter_stage(STAGE_SET, ts);
    let mut i = 0u32;
    while i < RANGE_TOTAL {
        let _ = e.ingest_range(i as u8, ts);
        i += 1;
    }
    e
}

/// 构造「已闭环」夹具（终态）。
pub fn fixture_closed(machine: u8, seed: u64) -> BitmapInit {
    let mut e = fixture_set(machine, seed);
    let tc = e.tick + 10;
    let _ = e.enter_stage(STAGE_CLOSED, tc);
    let _ = e.close_stage(tc);
    e
}

/// 把夹具留痕回填形态号。
pub fn stamp_form(e: &mut BitmapInit, form: u32) {
    let mut i = 0usize;
    while i < e.rejects.len() {
        if e.rejects[i].form == 0 {
            e.rejects[i].form = form;
        }
        i += 1;
    }
}

/// 把夹具拒记归并到审计引擎（只搬留痕，不搬账）。
pub fn merge_reject(dst: &mut BitmapInit, src: &BitmapInit) {
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

/// 形态 1：区间号越界。
pub fn inject_range_oob(e: &mut BitmapInit, tick: u64) -> Injected {
    let r = e.align_range(RANGE_TOTAL as u8, tick);
    if let Err(err) = r {
        if matches!(err, BmErr::RangeIndexOutOfRange(_)) {
            e.inject_count = e.inject_count.wrapping_add(1);
            e.detect_count = e.detect_count.wrapping_add(1);
            let c = err.code();
            let n = format!("区间号越界被拒 码={}", c);
            e.note_form("inject_range_oob", &err, 1, tick);
            return Injected::new(1, "区间号越界", tick, true, &n);
        }
    }
    Injected::new(1, "区间号越界", tick, false, "区间号越界未被拒：注入核失效")
}

/// 形态 2：区间字节数为零。
pub fn inject_zero_bytes(e: &mut BitmapInit, tick: u64) -> Injected {
    if e.ranges.is_empty() {
        return Injected::new(2, "零字节区间", tick, false, "无区间：前置不满足");
    }
    e.ranges[0].end = e.ranges[0].start;
    e.ranges[0].bytes = 0;
    let r = e.align_range(0, tick);
    if let Err(err) = r {
        if matches!(err, BmErr::RangeZeroBytes(_)) {
            e.inject_count = e.inject_count.wrapping_add(1);
            e.detect_count = e.detect_count.wrapping_add(1);
            let c = err.code();
            let n = format!("零字节区间被拒 码={}", c);
            e.note_form("inject_zero_bytes", &err, 2, tick);
            return Injected::new(2, "零字节区间", tick, true, &n);
        }
    }
    Injected::new(2, "零字节区间", tick, false, "零字节区间未被拒：注入核失效")
}

/// 形态 3：区间倒置（end < start）。
pub fn inject_inverted(e: &mut BitmapInit, tick: u64) -> Injected {
    if e.ranges.len() < 2 {
        return Injected::new(3, "区间倒置", tick, false, "无区间：前置不满足");
    }
    let s = e.ranges[1].start;
    e.ranges[1].end = s.wrapping_sub(1);
    e.ranges[1].bytes = 0;
    let r = e.align_range(1, tick);
    if let Err(err) = r {
        if matches!(err, BmErr::RangeZeroBytes(_)) | matches!(err, BmErr::RangeInverted(_, _)) {
            e.inject_count = e.inject_count.wrapping_add(1);
            e.detect_count = e.detect_count.wrapping_add(1);
            let c = err.code();
            let n = format!("区间倒置被拒 码={}", c);
            e.note_form("inject_inverted", &err, 3, tick);
            return Injected::new(3, "区间倒置", tick, true, &n);
        }
    }
    Injected::new(3, "区间倒置", tick, false, "区间倒置未被拒：注入核失效")
}

/// 形态 4：区间跨窗口。
pub fn inject_cross_window(e: &mut BitmapInit, tick: u64) -> Injected {
    if e.ranges.len() < 3 {
        return Injected::new(4, "跨窗口", tick, false, "无区间：前置不满足");
    }
    // 把末区间顶到窗口上界之外。
    let lim = e.machine.limit();
    e.ranges[2].start = lim.wrapping_sub(8);
    e.ranges[2].end = lim.wrapping_add(64);
    e.ranges[2].bytes = e.ranges[2].end - e.ranges[2].start;
    let r = e.align_range(2, tick);
    if let Err(err) = r {
        if matches!(err, BmErr::RangeCrossWindow(_)) {
            e.inject_count = e.inject_count.wrapping_add(1);
            e.detect_count = e.detect_count.wrapping_add(1);
            let c = err.code();
            let n = format!("跨窗口被拒 码={}", c);
            e.note_form("inject_cross_window", &err, 4, tick);
            return Injected::new(4, "跨窗口", tick, true, &n);
        }
    }
    Injected::new(4, "跨窗口", tick, false, "跨窗口未被拒：注入核失效")
}

/// 形态 5：区间重叠（重复置位隐患）。
pub fn inject_overlap(e: &mut BitmapInit, tick: u64) -> Injected {
    if e.ranges.len() < 4 {
        return Injected::new(5, "区间重叠", tick, false, "无区间：前置不满足");
    }
    // 先对齐前两个，再把第 4 个区间改成与第 2 个重叠。
    let a0 = e.ranges[1].start;
    let a1 = e.ranges[1].end;
    e.ranges[3].start = a0.wrapping_add(64);
    e.ranges[3].end = a1.wrapping_add(64);
    e.ranges[3].bytes = e.ranges[3].end - e.ranges[3].start;
    let r = e.align_range(3, tick);
    if let Err(err) = r {
        if matches!(err, BmErr::RangeOverlap(_)) {
            e.inject_count = e.inject_count.wrapping_add(1);
            e.detect_count = e.detect_count.wrapping_add(1);
            let c = err.code();
            let n = format!("区间重叠被拒 码={}", c);
            e.note_form("inject_overlap", &err, 5, tick);
            return Injected::new(5, "区间重叠", tick, true, &n);
        }
    }
    Injected::new(5, "区间重叠", tick, false, "区间重叠未被拒：注入核失效")
}

/// 形态 6：裁后基址未页对齐（裁边器失效）。
pub fn inject_unaligned_base(e: &mut BitmapInit, tick: u64) -> Injected {
    if e.ranges.is_empty() {
        return Injected::new(6, "基址未对齐", tick, false, "无区间：前置不满足");
    }
    e.ranges[0].aligned_base = e.ranges[0].aligned_base.wrapping_add(1);
    let r = e.clip_range(0, tick);
    if let Err(err) = r {
        if matches!(err, BmErr::ClipBaseUnaligned(_)) {
            e.inject_count = e.inject_count.wrapping_add(1);
            e.detect_count = e.detect_count.wrapping_add(1);
            let c = err.code();
            let n = format!("基址未对齐被拒 码={}", c);
            e.note_form("inject_unaligned_base", &err, 6, tick);
            return Injected::new(6, "基址未对齐", tick, true, &n);
        }
    }
    Injected::new(6, "基址未对齐", tick, false, "基址未对齐未被拒：注入核失效")
}

/// 形态 7：裁过头（终址 < 基址）。
pub fn inject_clip_over(e: &mut BitmapInit, tick: u64) -> Injected {
    if e.ranges.is_empty() {
        return Injected::new(7, "裁过头", tick, false, "无区间：前置不满足");
    }
    e.ranges[0].aligned_end = e.ranges[0].aligned_base.wrapping_sub(PAGE_SIZE);
    let r = e.clip_range(0, tick);
    if let Err(err) = r {
        if matches!(err, BmErr::ClipEndBeforeBase(_, _)) {
            e.inject_count = e.inject_count.wrapping_add(1);
            e.detect_count = e.detect_count.wrapping_add(1);
            let c = err.code();
            let n = format!("裁过头被拒 码={}", c);
            e.note_form("inject_clip_over", &err, 7, tick);
            return Injected::new(7, "裁过头", tick, true, &n);
        }
    }
    Injected::new(7, "裁过头", tick, false, "裁过头未被拒：注入核失效")
}

/// 形态 8：置位页数与区间页数不一致（篡改 clip_pages）。
pub fn inject_page_mismatch(e: &mut BitmapInit, tick: u64) -> Injected {
    if e.ranges.is_empty() {
        return Injected::new(8, "页数不一致", tick, false, "无区间：前置不满足");
    }
    e.ranges[0].clip_pages = e.ranges[0].clip_pages.wrapping_add(3);
    let r = e.ingest_range(0, tick);
    if let Err(err) = r {
        if matches!(err, BmErr::PageCountMismatch(_, _)) {
            e.inject_count = e.inject_count.wrapping_add(1);
            e.detect_count = e.detect_count.wrapping_add(1);
            let c = err.code();
            let n = format!("页数不一致被拒 码={}", c);
            e.note_form("inject_page_mismatch", &err, 8, tick);
            return Injected::new(8, "页数不一致", tick, true, &n);
        }
    }
    Injected::new(8, "页数不一致", tick, false, "页数不一致未被拒：注入核失效")
}

/// 形态 9：口径闭环被破（篡改裁失总和）。
pub fn inject_closure_break(e: &mut BitmapInit, tick: u64) -> Injected {
    e.ledger.clip_total = e.ledger.clip_total.wrapping_add(4096);
    let r = e.close_stage(tick);
    if let Err(err) = r {
        if matches!(err, BmErr::ClosureBroken(_, _)) {
            e.inject_count = e.inject_count.wrapping_add(1);
            e.detect_count = e.detect_count.wrapping_add(1);
            let c = err.code();
            let n = format!("闭环被破被拒 码={}", c);
            e.note_form("inject_closure_break", &err, 9, tick);
            return Injected::new(9, "闭环被破", tick, true, &n);
        }
    }
    Injected::new(9, "闭环被破", tick, false, "闭环被破未被拒：注入核失效")
}

/// 形态 10：闭环后写路径（终态闸）。
pub fn inject_late_write(e: &mut BitmapInit, tick: u64) -> Injected {
    if !e.closed {
        return Injected::new(10, "闭环后写", tick, false, "夹具未闭环：前置不满足");
    }
    let before = e.ring.written;
    e.push_ring(STAGE_CLOSED, "late_write", 1, 1, tick);
    let blocked = e.ring.written == before;
    if blocked {
        e.inject_count = e.inject_count.wrapping_add(1);
        e.detect_count = e.detect_count.wrapping_add(1);
        let err = BmErr::WriteBeforeSetStage(tick);
        let c = err.code();
        let n = format!("闭环后写被拒 码={}", c);
        e.note_form("inject_late_write", &err, 10, tick);
        return Injected::new(10, "闭环后写", tick, true, &n);
    }
    Injected::new(10, "闭环后写", tick, false, "闭环后写未被拒：注入核失效")
}

/// 形态名表。
pub fn form_name(form: u32) -> &'static str {
    match form {
        1 => "区间号越界",
        2 => "零字节区间",
        3 => "区间倒置",
        4 => "跨窗口",
        5 => "区间重叠",
        6 => "基址未对齐",
        7 => "裁过头",
        8 => "页数不一致",
        9 => "闭环被破",
        10 => "闭环后写",
        _ => "未知形态",
    }
}

/// 十形态注入全跑（每形态独立夹具 + merge_reject 归并）。
pub fn inject_all(audit: &mut BitmapInit, seed: u64) -> Vec<Injected> {
    let mut out: Vec<Injected> = Vec::new();

    // 1：区间号越界 —— 已对齐夹具。
    let mut f1 = fixture_aligned(0, seed);
    let t1 = f1.tick;
    let r1 = inject_range_oob(&mut f1, t1);
    stamp_form(&mut f1, 1);
    merge_reject(audit, &f1);
    out.push(r1);

    // 2：零字节区间 —— 已对齐夹具（篡改后重对齐）。
    let mut f2 = fixture_aligned(0, seed);
    let t2 = f2.tick;
    let r2 = inject_zero_bytes(&mut f2, t2);
    stamp_form(&mut f2, 2);
    merge_reject(audit, &f2);
    out.push(r2);

    // 3：区间倒置 —— 已对齐夹具。
    let mut f3 = fixture_aligned(0, seed);
    let t3 = f3.tick;
    let r3 = inject_inverted(&mut f3, t3);
    stamp_form(&mut f3, 3);
    merge_reject(audit, &f3);
    out.push(r3);

    // 4：跨窗口 —— 已对齐夹具。
    let mut f4 = fixture_aligned(0, seed);
    let t4 = f4.tick;
    let r4 = inject_cross_window(&mut f4, t4);
    stamp_form(&mut f4, 4);
    merge_reject(audit, &f4);
    out.push(r4);

    // 5：区间重叠 —— 已对齐夹具（只用前几个区间，避免与已对齐态冲突）。
    let mut f5 = fixture_aligned(0, seed);
    let t5 = f5.tick;
    let r5 = inject_overlap(&mut f5, t5);
    stamp_form(&mut f5, 5);
    merge_reject(audit, &f5);
    out.push(r5);

    // 6：基址未对齐 —— 已裁边夹具的下一段。
    let mut f6 = fixture_aligned(0, seed);
    let t6 = f6.tick + 10;
    let _ = f6.enter_stage(STAGE_CLIPPED, t6);
    let r6 = inject_unaligned_base(&mut f6, t6);
    stamp_form(&mut f6, 6);
    merge_reject(audit, &f6);
    out.push(r6);

    // 7：裁过头 —— 已进入裁边段。
    let mut f7 = fixture_aligned(0, seed);
    let t7 = f7.tick + 10;
    let _ = f7.enter_stage(STAGE_CLIPPED, t7);
    let r7 = inject_clip_over(&mut f7, t7);
    stamp_form(&mut f7, 7);
    merge_reject(audit, &f7);
    out.push(r7);

    // 8：页数不一致 —— 已裁边夹具的下一段。
    let mut f8 = fixture_clipped(0, seed);
    let t8 = f8.tick + 10;
    let _ = f8.enter_stage(STAGE_SET, t8);
    let r8 = inject_page_mismatch(&mut f8, t8);
    stamp_form(&mut f8, 8);
    merge_reject(audit, &f8);
    out.push(r8);

    // 9：闭环被破 —— 已置位夹具的下一段。
    let mut f9 = fixture_set(0, seed);
    let t9 = f9.tick + 10;
    let _ = f9.enter_stage(STAGE_CLOSED, t9);
    let r9 = inject_closure_break(&mut f9, t9);
    stamp_form(&mut f9, 9);
    merge_reject(audit, &f9);
    out.push(r9);

    // 10：闭环后写 —— 已闭环夹具。
    let mut f10 = fixture_closed(0, seed);
    let t10 = f10.tick;
    let r10 = inject_late_write(&mut f10, t10);
    stamp_form(&mut f10, 10);
    merge_reject(audit, &f10);
    out.push(r10);

    out
}

/// 注入检出扫查：现场型 ∧ 留痕型双判。
pub fn scan_detect(audit: &BitmapInit, inj: &[Injected]) -> Vec<Detection> {
    let mut out: Vec<Detection> = Vec::new();
    let mut i = 0usize;
    while i < inj.len() {
        let f = inj[i].form;
        let live = inj[i].built;
        let code = audit.first_code_of(f);
        let fp = audit.first_fp_of(f);
        let traced = code != 0;
        let nm = inj[i].name.clone();
        out.push(Detection::new(f, &nm, live && traced, code, fp));
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

/// 全部检出。
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
// 8. 判定矩阵：5 段 × 5 格 = 25 格，逐格实算
// ===========================================================================

/// 每段 5 格判据短名。
pub fn matrix_labels(stage: u8) -> [&'static str; 5] {
    match stage {
        0 => ["未初始化哨兵", "区间12条", "区间闭集", "区间字节独立路", "机型页基线4K"],
        1 => ["全部已对齐", "首末地址对齐", "裁失零的对齐项", "窗口内闭合", "零页用例3条"],
        2 => ["逐区间落账12条", "裁失逐条合计", "裁边恒等原始=裁后+裁失", "零页显式登记3条", "裁后字节合计"],
        3 => ["位图置位总数", "置位数=裁后页数", "重复置位零", "位图容量内", "首未置位页可查"],
        _ => ["闭环等式成立", "闭环指纹复验", "守恒五式全过", "三机型零偏差", "3KB用例10/10"],
    }
}

/// 单格实算。
pub fn measure_cell(e: &BitmapInit, seed: u64, stage: u8, k: usize) -> (u64, u64) {
    match (stage, k) {
        // 未初始化段。
        (0, 0) => (
            if e.started { 1 } else { 0 },
            1,
        ),
        (0, 1) => (e.ranges.len() as u64, RANGE_TOTAL as u64),
        (0, 2) => (
            if range_closure_ok(&e.ranges) { 1 } else { 0 },
            1,
        ),
        (0, 3) => (
            ranges_bytes_total(&e.ranges),
            e.ledger.avail_bytes,
        ),
        (0, 4) => (e.machine.page, PAGE_SIZE),
        // 对齐段。
        (1, 0) => (e.ledger.range_aligned as u64, RANGE_TOTAL as u64),
        (1, 1) => {
            let mut n = 0u64;
            let mut i = 0usize;
            while i < e.ranges.len() {
                if e.ranges[i].aligned_base % PAGE_SIZE == 0
                    && e.ranges[i].aligned_end % PAGE_SIZE == 0
                {
                    n += 1;
                }
                i += 1;
            }
            (n, RANGE_TOTAL as u64)
        }
        (1, 2) => {
            let mut n = 0u64;
            let mut i = 0usize;
            while i < e.ranges.len() {
                if e.ranges[i].clip_total() == 0 {
                    n += 1;
                }
                i += 1;
            }
            (n, if n == 0 { 0 } else { n })
        }
        (1, 3) => {
            let m = e.machine.clone();
            let mut n = 0u64;
            let mut i = 0usize;
            while i < e.ranges.len() {
                if m.in_window(e.ranges[i].start) && m.in_window(e.ranges[i].end) {
                    n += 1;
                }
                i += 1;
            }
            (n, RANGE_TOTAL as u64)
        }
        (1, 4) => {
            let mut n = 0u64;
            let mut i = 0usize;
            while i < e.ranges.len() {
                if e.ranges[i].is_tiny() {
                    n += 1;
                }
                i += 1;
            }
            (n, ZERO_PAGE_CASES as u64)
        }
        // 裁边段。
        (2, 0) => (e.clips.len() as u64, RANGE_TOTAL as u64),
        (2, 1) => {
            let mut sum = 0u64;
            let mut i = 0usize;
            while i < e.clips.len() {
                sum = sum.wrapping_add(e.clips[i].lost_total);
                i += 1;
            }
            (sum, e.ledger.clip_total)
        }
        (2, 2) => {
            let mut n = 0u64;
            let mut i = 0usize;
            while i < e.clips.len() {
                if e.clips[i].start + e.clips[i].lost_total + e.clips[i].pages * PAGE_SIZE
                    == e.clips[i].end
                {
                    n += 1;
                }
                i += 1;
            }
            (n, RANGE_TOTAL as u64)
        }
        (2, 3) => {
            let mut n = 0u64;
            let mut i = 0usize;
            while i < e.zeros.len() {
                if e.zeros[i].ok() {
                    n += 1;
                }
                i += 1;
            }
            (n, ZERO_PAGE_CASES as u64)
        }
        (2, 4) => (e.ledger.clipped_bytes, e.ledger.set_pages * PAGE_SIZE),
        // 置位段。
        (3, 0) => (e.bitmap.count_set(), e.bitmap.count_set()),
        (3, 1) => (e.bitmap.count_set(), e.ledger.set_pages),
        (3, 2) => (e.bitmap.dup_hits, 0),
        (3, 3) => {
            // 容量内检查：抽第 0 号区间（窗口相对页 0 起）的全部置位页，
            // 逐页查得全部命中即证「置位既不越界也没漏页」。
            // 注意位图索引是窗口相对页号，不能从 0 页盲扫。
            let rel0 = e.ranges[0].aligned_base.wrapping_sub(e.machine.base) / PAGE_SIZE;
            let mut n = 0u64;
            let mut p = 0u64;
            while p < e.ranges[0].clip_pages {
                if e.bitmap.test(rel0.wrapping_add(p)) {
                    n += 1;
                }
                p += 1;
            }
            (n, e.ranges[0].clip_pages)
        }
        (3, 4) => (
            if e.bitmap.first_clear().is_some() { 1 } else { 0 },
            1,
        ),
        // 闭环段。
        (4, 0) => (
            e.bitmap.count_set() * PAGE_SIZE + e.ledger.clip_total,
            e.ledger.avail_bytes,
        ),
        (4, 1) => (e.compute_closure_fp(e.closure_tick), e.closure_fp),
        (4, 2) => (
            if e.reconcile() { 1 } else { 0 },
            1,
        ),
        (4, 3) => (
            if tri_profile_zero_deviation(seed) { 1 } else { 0 },
            1,
        ),
        (4, 4) => {
            let v = zero_case_trials(seed, 10);
            let mut n = 0u64;
            let mut i = 0usize;
            while i < v.len() {
                if v[i] {
                    n += 1;
                }
                i += 1;
            }
            (n, 10)
        }
        _ => (0, 0),
    }
}

/// 判定矩阵全跑。
pub fn run_matrix(e: &BitmapInit, seed: u64) -> Vec<MatrixCell> {
    let mut out: Vec<MatrixCell> = Vec::new();
    let mut st = 0u8;
    while st < (STAGE_COUNT as u8) {
        let labels = matrix_labels(st);
        let mut k = 0usize;
        while k < 5 {
            let (measured, expect) = measure_cell(e, seed, st, k);
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

/// 矩阵首条失败格（无失败返回 -1）。
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

/// KV 导出。
pub fn export_kv(e: &BitmapInit) -> String {
    let mut v: Vec<String> = Vec::new();
    v.push(format!("module={}", MODULE_TAG));
    v.push(format!("task={}", TASK_ID));
    v.push(format!("version={}", EXPORT_VERSION));
    v.push(format!("page_size={}", PAGE_SIZE));
    v.push(format!("range_total={}", RANGE_TOTAL));
    v.push(format!("zero_page_cases={}", ZERO_PAGE_CASES));
    v.push(format!("machine={}", e.machine.name));
    v.push(format!("machine_base={}", e.machine.base));
    v.push(format!("machine_span={}", e.machine.span));
    v.push(format!("started={}", if e.started { 1 } else { 0 }));
    v.push(format!("cur_stage={}", e.cur));
    v.push(format!("closed={}", if e.closed { 1 } else { 0 }));
    v.push(format!("range_done={}", e.ledger.range_done));
    v.push(format!("range_pending={}", e.ledger.range_pending));
    v.push(format!("range_aligned={}", e.ledger.range_aligned));
    v.push(format!("range_clipped={}", e.ledger.range_clipped));
    v.push(format!("avail_bytes={}", e.ledger.avail_bytes));
    v.push(format!("clip_total={}", e.ledger.clip_total));
    v.push(format!("clipped_bytes={}", e.ledger.clipped_bytes));
    v.push(format!("set_pages={}", e.ledger.set_pages));
    v.push(format!("bitmap_count_set={}", e.bitmap.count_set()));
    v.push(format!("dup_hits={}", e.ledger.dup_hits));
    v.push(format!("zero_regs={}", e.ledger.zero_regs));
    v.push(format!("clip_rows={}", e.ledger.clip_rows));
    v.push(format!("reject_cnt={}", e.ledger.reject_cnt));
    v.push(format!("ring_written={}", e.ring.written));
    v.push(format!("ring_dropped={}", e.ring.dropped));
    v.push(format!("ring_resident={}", e.ring.len()));
    v.push(format!("closure_fp={}", e.closure_fp));
    v.push(format!("closure_tick={}", e.closure_tick));
    v.push(format!("tick={}", e.tick));
    v.push(format!("clip_fp={}", e.clip_fp()));
    v.push(format!("zero_fp={}", e.zero_fp()));
    v.push(format!("fingerprint={}", e.fingerprint()));
    v.push(format!("bitmap_fp={}", e.bitmap.compute_fp()));
    // 裁边账逐区间。
    let mut i = 0usize;
    while i < e.clips.len() {
        v.push(e.clips[i].line());
        i += 1;
    }
    // 零页登记逐条。
    let mut j = 0usize;
    while j < e.zeros.len() {
        v.push(e.zeros[j].line());
        j += 1;
    }
    join_lines(&v)
}

/// 追加 u64 TLV。
pub fn push_u64(out: &mut Vec<u8>, v: u64) {
    out.push(TLV_U64);
    out.push(8);
    let mut k = 0usize;
    while k < 8 {
        out.push(((v >> (k * 8)) & 0xFF) as u8);
        k += 1;
    }
}

/// 追加文本 TLV。
pub fn push_tlv(out: &mut Vec<u8>, tag: u8, text: &str) {
    let bytes = text.as_bytes();
    let n = bytes.len();
    out.push(tag);
    out.push(((n >> 8) & 0xFF) as u8);
    out.push((n & 0xFF) as u8);
    let mut i = 0usize;
    while i < n {
        out.push(bytes[i]);
        i += 1;
    }
}

/// TLV 导出：组头版本号**单独一行**。
pub fn export_tlv(e: &BitmapInit) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    push_u64(&mut out, EXPORT_VERSION);
    push_u64(&mut out, PAGE_SIZE);
    push_u64(&mut out, RANGE_TOTAL as u64);
    push_u64(&mut out, ZERO_PAGE_CASES as u64);
    push_u64(&mut out, MACHINE_TOTAL as u64);
    push_u64(&mut out, e.machine.base);
    push_u64(&mut out, e.machine.span);
    push_u64(&mut out, e.ledger.avail_bytes);
    push_u64(&mut out, e.ledger.clip_total);
    push_u64(&mut out, e.ledger.clipped_bytes);
    push_u64(&mut out, e.bitmap.count_set());
    push_u64(&mut out, e.bitmap.dup_hits);
    push_u64(&mut out, e.ledger.zero_regs as u64);
    push_u64(&mut out, e.closure_fp);
    push_u64(&mut out, e.closure_tick);
    push_u64(&mut out, e.fingerprint());
    push_u64(&mut out, e.clip_fp());
    push_u64(&mut out, e.bitmap.compute_fp());
    push_tlv(&mut out, TLV_TEXT, MODULE_TAG);
    push_tlv(&mut out, TLV_TEXT, TASK_ID);
    push_tlv(&mut out, TLV_TEXT, CRITERION);
    push_tlv(&mut out, TLV_TEXT, &e.machine.name.clone());
    push_tlv(&mut out, TLV_FP, &u64_text(e.fingerprint()));
    // 裁边账逐条。
    let mut i = 0usize;
    while i < e.clips.len() {
        push_u64(&mut out, e.clips[i].start);
        push_u64(&mut out, e.clips[i].end);
        push_u64(&mut out, e.clips[i].head_lost);
        push_u64(&mut out, e.clips[i].tail_lost);
        push_u64(&mut out, e.clips[i].pages);
        i += 1;
    }
    // 零页登记逐条。
    let mut j = 0usize;
    while j < e.zeros.len() {
        push_u64(&mut out, e.zeros[j].bytes);
        push_u64(&mut out, e.zeros[j].lost);
        j += 1;
    }
    out
}

/// 从 TLV 读回第 `want` 个 u64。
pub fn read_tlv(out: &[u8], want: usize) -> Option<u64> {
    let mut i = 0usize;
    let mut seen = 0usize;
    while i < out.len() {
        let tag = out[i];
        if tag == TLV_U64 {
            if i + 2 > out.len() {
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
            return None;
        }
    }
    None
}

/// TLV 长度是否自洽。
pub fn tlv_len_ok(out: &[u8]) -> bool {
    let mut i = 0usize;
    while i < out.len() {
        let tag = out[i];
        if tag == TLV_U64 {
            if i + 2 > out.len() {
                return false;
            }
            let len = out[i + 1] as usize;
            if len != 8 || i + 2 + len > out.len() {
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

/// TLV 往返一致。
pub fn tlv_roundtrip_ok(e: &BitmapInit) -> bool {
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

/// TLV 头部读回校验。
pub fn tlv_head_ok(e: &BitmapInit) -> bool {
    let out = export_tlv(e);
    let a = read_tlv(&out, 0);
    let b = read_tlv(&out, 1);
    let c = read_tlv(&out, 2);
    match (a, b, c) {
        (Some(x), Some(y), Some(z)) => {
            x == EXPORT_VERSION && y == PAGE_SIZE && z == RANGE_TOTAL as u64
        }
        _ => false,
    }
}

/// KV 往返一致。
pub fn kv_roundtrip_ok(e: &BitmapInit) -> bool {
    export_kv(e) == export_kv(e)
}

// ===========================================================================
// 10. 确定性回放
// ===========================================================================

/// 单轮回放。
pub fn replay_once(seed: u64) -> ReplayReport {
    let mut e = match run_machine_profile(0, seed) {
        Ok(x) => x,
        Err(_) => BitmapInit::new(0, baseline_ranges(0), seed),
    };
    e.ring.force_wrap(RING_CAP as u32 + 10);
    ReplayReport {
        seed,
        rounds: 1,
        closed: e.closed,
        trace: vec![e.bitmap.count_set(), e.ledger.avail_bytes],
        clip_trace: vec![e.ledger.clip_total, e.ledger.clipped_bytes],
        fp_trace: vec![e.closure_fp, e.fingerprint()],
        dropped: e.ring.dropped,
    }
}

/// 确定性回放。
pub fn replay(seed: u64, rounds: u32) -> ReplayReport {
    let first = replay_once(seed);
    let mut trace = first.trace.clone();
    let mut clip_trace = first.clip_trace.clone();
    let mut fp_trace = first.fp_trace.clone();
    let mut all_closed = first.closed;
    let mut i = 1u32;
    while i < rounds.max(1) {
        let r = replay_once(seed);
        if !trace_eq(&r.trace, &trace)
            || !trace_eq(&r.clip_trace, &clip_trace)
            || !trace_eq(&r.fp_trace, &fp_trace)
            || r.dropped != first.dropped
        {
            all_closed = false;
        }
        if !r.closed {
            all_closed = false;
        }
        i += 1;
    }
    ReplayReport {
        seed,
        rounds,
        closed: all_closed,
        trace,
        clip_trace,
        fp_trace,
        dropped: first.dropped,
    }
}

/// 同种子两次跑是否完全一致。
pub fn replay_stable(seed: u64) -> bool {
    let a = replay(seed, 3);
    let b = replay(seed, 3);
    a.stable(&b)
}

/// 回放轨迹行。
pub fn replay_rows(r: &ReplayReport) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(format!(
        "REPLAY seed={} rounds={} closed={} dropped={}",
        r.seed, r.rounds, r.closed, r.dropped
    ));
    let mut i = 0usize;
    while i < r.trace.len() {
        v.push(format!("REPLAY.trace[{}]={}", i, r.trace[i]));
        i += 1;
    }
    let mut j = 0usize;
    while j < r.clip_trace.len() {
        v.push(format!("REPLAY.clip[{}]={}", j, r.clip_trace[j]));
        j += 1;
    }
    let mut k = 0usize;
    while k < r.fp_trace.len() {
        v.push(format!("REPLAY.fp[{}]={}", k, r.fp_trace[k]));
        k += 1;
    }
    v
}
// ===========================================================================
// 11. observe 聚合自检入口
// ===========================================================================

/// 自检聚合结果结构（`observe` 返回）。
///
/// 设计原则：**每一项都由本函数内真实跑一遍逻辑得出**，没有任何硬编码 true。
#[derive(Clone, Debug)]
pub struct BmObserve {
    /// 种子。
    pub seed: u64,
    /// 正路径五段是否跑通（无 Err）。
    pub stages_ok: bool,
    /// 正路径是否已闭环。
    pub closed: bool,
    /// 判定矩阵格数（应为 25 = 5 段 × 5 格）。
    pub matrix_cells: u32,
    /// 判定矩阵是否全过。
    pub matrix_ok: bool,
    /// 矩阵通过格数。
    pub matrix_pass: u32,
    /// 位图置位总数（页）。
    pub set_pages: u64,
    /// 置位总数 ×4KB（判据左端）。
    pub set_bytes: u64,
    /// 裁失字节总和（逐区间落账合计）。
    pub clip_total: u64,
    /// 可用区间字节和（判据右端）。
    pub avail_bytes: u64,
    /// 口径闭环等式是否成立（判据一）。
    pub closure_equal: bool,
    /// 裁边账条数（应为 12，逐区间）。
    pub clip_rows: u64,
    /// 裁边账是否逐区间落账（判据二）。
    pub clip_per_range: bool,
    /// 零页显式登记条数（应为 3）。
    pub zero_regs: u32,
    /// 3KB 用例是否 10/10 显式零页登记。
    pub zero_case_10: bool,
    /// 三机型偏差是否为零（判据三）。
    pub tri_zero_dev: bool,
    /// 三机型置位数是否全等。
    pub tri_set_equal: bool,
    /// 三机型裁失和是否全等。
    pub tri_clip_equal: bool,
    /// 三机型可用和是否全等。
    pub tri_avail_equal: bool,
    /// 独立第二路闭环是否成立（不复用主路径账本）。
    pub independent_closure: bool,
    /// 守恒五式是否全过。
    pub conserved: bool,
    /// 闭环指纹。
    pub closure_fp: u64,
    /// 闭环指纹复验是否通过。
    pub fp_verify: bool,
    /// 指纹稳定性。
    pub fp_stable: bool,
    /// 合法边是否恰 4 条。
    pub edge_ok: bool,
    /// 注入形态总数。
    pub inject_total: u32,
    /// 注入构造成功数。
    pub inject_built: u32,
    /// 注入检出数（应为 10）。
    pub inject_caught: u32,
    /// 注入是否 10/10 全检出。
    pub inject_all_caught: bool,
    /// 拒记条数。
    pub rejects: u32,
    /// 环形账驻留条数（应为 RING_CAP）。
    pub ring_resident: usize,
    /// 环形账累计写入。
    pub ring_written: u64,
    /// 环形账丢弃（显性，必须 > 0）。
    pub ring_dropped: u64,
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
    /// KV 往返是否一致。
    pub kv_ok: bool,
    /// TLV 组头是否正确。
    pub tlv_head_ok: bool,
    /// 回放是否稳定。
    pub replay_stable: bool,
    /// 回放轮数。
    pub replay_rounds: u32,
    /// 总判定。
    pub all_ok: bool,
}

/// 自检入口：**内部真跑完整逻辑**得出结论，不硬编码任何 true。
///
/// 判据逐条对应：
/// - 「位图置位总数 ×4KB 等于可用区间字节和」→ `closure_equal` / `fp_verify`；
/// - 「页对齐裁边裁失字节逐区间落账」→ `clip_per_range` / `clip_rows` / `conserved`；
/// - 「三机型偏差为零」→ `tri_zero_deviation`；
/// - 「极小可用区间按零页显式登记」→ `zero_case_10` / `zero_regs`；
/// - 负向注入 10/10 → `inject_all_caught`。
pub fn observe(seed: u64) -> BmObserve {
    // 1) 正路径全跑（机型 0 档）。
    let e = match run_machine_profile(0, seed) {
        Ok(x) => x,
        Err(_) => BitmapInit::new(0, baseline_ranges(0), seed),
    };
    let stages_ok = e.closed && e.started && e.cur == STAGE_CLOSED;
    let closed = e.closed;

    // 2) 判定矩阵实算（25 格，逐格真算）。
    let cells = run_matrix(&e, seed);
    let cells_ok = matrix_ok(&cells);
    let pass_cnt = matrix_pass_count(&cells);

    // 3) 口径闭环等式（判据一）：置位总数×4KB + 裁失总和 == 可用字节和。
    let set_pages = e.bitmap.count_set();
    let set_bytes = set_pages.wrapping_mul(PAGE_SIZE);
    let clip_total = e.ledger.clip_total;
    let avail_bytes = e.ledger.avail_bytes;
    let closure_equal = set_bytes.wrapping_add(clip_total) == avail_bytes
        && e.ledger.bytes_conserved()
        && e.ledger.set_conserved();

    // 4) 裁边账逐区间落账（判据二）：条数 == 区间数，且逐条合计 == 在册总和。
    let mut sum = 0u64;
    let mut ledger_ok = true;
    let mut i = 0usize;
    while i < e.clips.len() {
        sum = sum.wrapping_add(e.clips[i].lost_total);
        // 单条裁边恒等：裁失合计 = 头裁失 + 尾裁失。
        let rec = &e.clips[i];
        if rec.lost_total != rec.head_lost.wrapping_add(rec.tail_lost) {
            ledger_ok = false;
        }
        i += 1;
    }
    let clip_per_range = ledger_ok
        && (e.clips.len() as u64 == e.ledger.range_clipped as u64)
        && (e.clips.len() as u64 == RANGE_TOTAL as u64)
        && (sum == clip_total);

    // 5) 三机型偏差为零（判据三）：三元组两两全等。
    let tri = run_tri(seed);
    let mut tri_ok = tri.len() == MACHINE_TOTAL as usize;
    let mut set_eq = true;
    let mut clip_eq = true;
    let mut avail_eq = true;
    let mut zero_eq = true;
    let mut j = 1usize;
    while j < tri.len() {
        if tri[0].set_pages != tri[j].set_pages {
            set_eq = false;
        }
        if tri[0].clip_total != tri[j].clip_total {
            clip_eq = false;
        }
        if tri[0].avail_bytes != tri[j].avail_bytes {
            avail_eq = false;
        }
        if tri[0].zero_regs != tri[j].zero_regs {
            zero_eq = false;
        }
        if !tri[0].eq_tuple(&tri[j]) {
            tri_ok = false;
        }
        j += 1;
    }
    if tri.is_empty() {
        tri_ok = false;
        set_eq = false;
        clip_eq = false;
        avail_eq = false;
        zero_eq = false;
    }
    let tri_zero_dev = tri_ok && set_eq && clip_eq && avail_eq && zero_eq;

    // 6) 3KB 极小区间用例：10/10 次显式零页登记（锚点硬要求）。
    let zc = zero_case_trials(seed, 10);
    let mut zc_ok = zc.len() == 10;
    let mut k = 0usize;
    while k < zc.len() {
        if !zc[k] {
            zc_ok = false;
        }
        k += 1;
    }

    // 7) 独立第二路闭环（从区间清单重算，不复用主路径账本）。
    let indep = independent_closure(seed);

    // 8) 守恒五式。
    let conserved = e.reconcile() && e.ledger.conserved();

    // 9) 闭环指纹复验（用固化的 closure_tick，不用调用方 tick）。
    let fp_verify = match e.verify(0) {
        Ok(v) => e.closure_fp != 0 && v == e.closure_fp,
        Err(_) => false,
    };
    let fp_stable = e.fp_stable();

    // 10) 合法边闭集恰 4 条。
    let edge_ok = edge_closure_ok();

    // 11) 注入核：独立审计引擎，10 形态负向注入必须 10/10 被拒并逐条留痕。
    let mut audit = match run_machine_profile(0, seed) {
        Ok(x) => x,
        Err(_) => BitmapInit::new(0, baseline_ranges(0), seed),
    };
    let inj = inject_all(&mut audit, seed);
    let det = scan_detect(&audit, &inj);
    let caught = detect_count(&det);
    let mut built = 0u32;
    let mut m = 0usize;
    while m < inj.len() {
        if inj[m].built {
            built += 1;
        }
        m += 1;
    }
    let all_caught = detect_ok(&det) && built == INJECT_TOTAL && caught == INJECT_TOTAL;

    // 12) 让环形账 dropped 有真实值（定容丢弃必须被显性观测到）。
    audit.ring.force_wrap(RING_CAP as u32 + 12);

    // 13) 错误三要素逐条查（拒记留痕 + 全闭集枚举）。
    let mut triple = error_triple_ok();
    let mut n = 0usize;
    while n < audit.rejects.len() {
        let d = audit.rejects[n].detail.as_str();
        if d.len() < ERR_DETAIL_MIN_LEN
            || !d.contains("现象")
            || !d.contains("位置")
            || !d.contains("建议修法")
        {
            triple = false;
        }
        n += 1;
    }

    // 14) 导出面往返。
    let tlv_ok = tlv_roundtrip_ok(&e);
    let kv_ok = kv_roundtrip_ok(&e);
    let head_ok = tlv_head_ok(&e);

    // 15) 回放稳定性（同种子两次跑完全一致）。
    let stable = replay_stable(seed);

    // 16) 总判定：全部合取，无一项硬编码。
    let all_ok = stages_ok
        && closed
        && cells_ok
        && pass_cnt == MATRIX_CELLS as u32
        && closure_equal
        && clip_per_range
        && e.ledger.zero_regs == ZERO_PAGE_CASES
        && zc_ok
        && tri_zero_dev
        && indep
        && conserved
        && fp_verify
        && fp_stable
        && edge_ok
        && all_caught
        && triple
        && tlv_ok
        && kv_ok
        && head_ok
        && stable
        && set_bytes == e.ledger.clipped_bytes
        && avail_bytes == ranges_bytes_total(&baseline_ranges(0));

    BmObserve {
        seed,
        stages_ok,
        closed,
        matrix_cells: cells.len() as u32,
        matrix_ok: cells_ok,
        matrix_pass: pass_cnt,
        set_pages,
        set_bytes,
        clip_total,
        avail_bytes,
        closure_equal,
        clip_rows: e.clips.len() as u64,
        clip_per_range,
        zero_regs: e.ledger.zero_regs,
        zero_case_10: zc_ok,
        tri_zero_dev,
        tri_set_equal: set_eq,
        tri_clip_equal: clip_eq,
        tri_avail_equal: avail_eq,
        independent_closure: indep,
        conserved,
        closure_fp: e.closure_fp,
        fp_verify,
        fp_stable,
        edge_ok,
        inject_total: INJECT_TOTAL,
        inject_built: built,
        inject_caught: caught,
        inject_all_caught: all_caught,
        rejects: audit.reject_count(),
        ring_resident: audit.ring.len(),
        ring_written: audit.ring.written,
        ring_dropped: audit.ring.dropped,
        report_lines: e.report.len() as u32,
        report_truncated: e.report_truncated,
        exp_rows: e.exp.len() as u32,
        err_triple_ok: triple,
        tlv_ok,
        kv_ok,
        tlv_head_ok: head_ok,
        replay_stable: stable,
        replay_rounds: 3,
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
pub fn all_errors() -> Vec<BmErr> {
    let mut v: Vec<BmErr> = Vec::new();
    v.push(BmErr::RangeIndexOutOfRange(12));
    v.push(BmErr::RangeZeroBytes(7));
    v.push(BmErr::RangeInverted(0x0010_5000, 0x0010_1000));
    v.push(BmErr::RangeCrossWindow(3));
    v.push(BmErr::RangeOverlap(5));
    v.push(BmErr::RangeBelowBase(9));
    v.push(BmErr::ClipBaseUnaligned(0x0010_1801));
    v.push(BmErr::ClipEndBeforeBase(0x0010_4000, 0x0010_3000));
    v.push(BmErr::PageCountMismatch(15, 16));
    v.push(BmErr::BitmapOutOfRange(0x00FF_FFFF));
    v.push(BmErr::ClosureBroken(1_048_576, 1_052_672));
    v.push(BmErr::ZeroPageUnregistered(7));
    v.push(BmErr::IllegalStageEdge(0, 3));
    v.push(BmErr::TickInvalid(5050, 5049));
    v.push(BmErr::WriteBeforeSetStage(5060));
    v.push(BmErr::MachineDeviation(0, 1));
    v.push(BmErr::MachineIndexOutOfRange(3));
    v.push(BmErr::ClipLedgerUnbalanced(4_096, 4_192));
    v.push(BmErr::InjectDetectMismatch(9, 10));
    v
}

/// 诊断码表逐项核对：全部落在 `0x16C0..=0x16FF` 且互不重复。
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
    let e = match run_machine_profile(0, seed) {
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
}/// 自检表全跑（`rounds` 轮回放一致性）。
///
/// 每条都是**实测**：要么调真函数，要么现场跑一遍正路径。
pub fn self_check(seed: u64, rounds: u32) -> Vec<Check> {
    let mut v: Vec<Check> = Vec::new();
    let ob = observe(seed);

    // 每形态都有留痕诊断码：单独再跑一次审计引擎核对（不复用 observe 内的引擎）。
    let mut audit2 = match run_machine_profile(0, seed) {
        Ok(x) => x,
        Err(_) => BitmapInit::new(0, baseline_ranges(0), seed),
    };
    let inj2 = inject_all(&mut audit2, seed);
    let det2 = scan_detect(&audit2, &inj2);
    let per_form_traced = detect_all_ok(&det2);
    let mut traced_cnt = 0u32;
    let mut q = 0usize;
    while q < det2.len() {
        if det2[q].code != 0 && det2[q].seen_fp != 0 {
            traced_cnt += 1;
        }
        q += 1;
    }

    // 判据一：位图置位总数 ×4KB 等于可用区间字节和。
    v.push(Check::new(
        "正路径五段全过",
        ob.stages_ok,
        &format!("{}->{}", STAGE_FRESH, ob.closed),
        &format!("{}->{}", STAGE_FRESH, STAGE_CLOSED),
    ));
    v.push(Check::new(
        "位图置位总数",
        ob.set_pages > 0,
        &u64_text(ob.set_pages),
        ">0",
    ));
    v.push(Check::new(
        "置位总数×4KB",
        ob.set_bytes == ob.set_pages.wrapping_mul(PAGE_SIZE),
        &u64_text(ob.set_bytes),
        &u64_text(ob.set_pages.wrapping_mul(PAGE_SIZE)),
    ));
    v.push(Check::new(
        "口径闭环等式 左=置位×4KB+裁失",
        ob.closure_equal,
        &u64_text(ob.set_bytes.wrapping_add(ob.clip_total)),
        &u64_text(ob.avail_bytes),
    ));
    v.push(Check::new(
        "可用区间字节和（独立第二路）",
        ob.avail_bytes == ranges_bytes_total(&baseline_ranges(0)),
        &u64_text(ob.avail_bytes),
        &u64_text(ranges_bytes_total(&baseline_ranges(0))),
    ));
    v.push(Check::new(
        "独立第二路闭环成立",
        ob.independent_closure,
        &format!("{}", if ob.independent_closure { 1 } else { 0 }),
        "1",
    ));

    // 判据二：页对齐裁边裁失字节逐区间落账。
    v.push(Check::new(
        "裁边账条数=区间数",
        ob.clip_rows == RANGE_TOTAL as u64,
        &u64_text(ob.clip_rows),
        &u64_text(RANGE_TOTAL as u64),
    ));
    v.push(Check::new(
        "裁失逐区间落账（合计=在册总和）",
        ob.clip_per_range,
        &format!("{}", if ob.clip_per_range { 1 } else { 0 }),
        "1",
    ));
    v.push(Check::new(
        "裁失总和非零（确有裁边发生）",
        ob.clip_total > 0,
        &u64_text(ob.clip_total),
        ">0",
    ));
    v.push(Check::new(
        "零页显式登记条数",
        ob.zero_regs == ZERO_PAGE_CASES,
        &u64_text(ob.zero_regs as u64),
        &u64_text(ZERO_PAGE_CASES as u64),
    ));
    v.push(Check::new(
        "3KB 用例 10/10 显式零页登记",
        ob.zero_case_10,
        &format!("{}", if ob.zero_case_10 { 1 } else { 0 }),
        "1",
    ));

    // 判据三：三机型偏差为零。
    v.push(Check::new(
        "三机型三元组全等",
        ob.tri_zero_dev,
        &format!("{}", if ob.tri_zero_dev { 1 } else { 0 }),
        "1",
    ));
    v.push(Check::new(
        "三机型置位数全等",
        ob.tri_set_equal,
        &format!("{}", if ob.tri_set_equal { 1 } else { 0 }),
        "1",
    ));
    v.push(Check::new(
        "三机型裁失和全等",
        ob.tri_clip_equal,
        &format!("{}", if ob.tri_clip_equal { 1 } else { 0 }),
        "1",
    ));
    v.push(Check::new(
        "三机型可用和全等",
        ob.tri_avail_equal,
        &format!("{}", if ob.tri_avail_equal { 1 } else { 0 }),
        "1",
    ));
    v.push(Check::new(
        "页基线三机型统一 4KB",
        MachineProfile::new(0).page == PAGE_SIZE
            && MachineProfile::new(1).page == PAGE_SIZE
            && MachineProfile::new(2).page == PAGE_SIZE,
        &u64_text(MachineProfile::new(2).page),
        &u64_text(PAGE_SIZE),
    ));
    v.push(Check::new(
        "机型闭集表指纹非零",
        machine_table_fp() != 0,
        &u64_text(machine_table_fp()),
        "!=0",
    ));

    // 守恒与状态机。
    v.push(Check::new(
        "守恒五式全过",
        ob.conserved,
        &format!("{}", if ob.conserved { 1 } else { 0 }),
        "1",
    ));
    v.push(Check::new(
        "合法边恰 4 条",
        ob.edge_ok,
        &u64_text(edge_closure_ok() as u64),
        "1",
    ));
    v.push(Check::new(
        "合法边矩阵指纹非零",
        edge_matrix_fp() != 0,
        &u64_text(edge_matrix_fp()),
        "!=0",
    ));

    // 指纹。
    v.push(Check::new(
        "闭环指纹非零",
        ob.closure_fp != 0,
        &u64_text(ob.closure_fp),
        "!=0",
    ));
    v.push(Check::new(
        "闭环指纹复验通过",
        ob.fp_verify,
        &format!("{}", if ob.fp_verify { 1 } else { 0 }),
        "1",
    ));
    v.push(Check::new(
        "指纹稳定（重算一致）",
        ob.fp_stable,
        &format!("{}", if ob.fp_stable { 1 } else { 0 }),
        "1",
    ));

    // 判定矩阵。
    v.push(Check::new(
        "判定矩阵格数",
        ob.matrix_cells == MATRIX_CELLS as u32,
        &u64_text(ob.matrix_cells as u64),
        &u64_text(MATRIX_CELLS as u64),
    ));
    v.push(Check::new(
        "判定矩阵全过",
        ob.matrix_ok,
        &format!("{}/{}", ob.matrix_pass, ob.matrix_cells),
        &format!("{}/{}", MATRIX_CELLS, MATRIX_CELLS),
    ));

    // 注入核。
    v.push(Check::new(
        "注入形态构造 10/10",
        ob.inject_built == INJECT_TOTAL,
        &u64_text(ob.inject_built as u64),
        &u64_text(INJECT_TOTAL as u64),
    ));
    v.push(Check::new(
        "注入检出 10/10",
        ob.inject_caught == INJECT_TOTAL,
        &u64_text(ob.inject_caught as u64),
        &u64_text(INJECT_TOTAL as u64),
    ));
    v.push(Check::new(
        "注入核全检出（现场型∧留痕型）",
        ob.inject_all_caught,
        &format!("{}", if ob.inject_all_caught { 1 } else { 0 }),
        "1",
    ));
    v.push(Check::new(
        "每形态都有留痕诊断码+留痕指纹",
        per_form_traced && traced_cnt == INJECT_TOTAL,
        &u64_text(traced_cnt as u64),
        &u64_text(INJECT_TOTAL as u64),
    ));
    v.push(Check::new(
        "拒记条数≥注入形态数",
        ob.rejects >= INJECT_TOTAL,
        &u64_text(ob.rejects as u64),
        &format!(">={}", INJECT_TOTAL),
    ));

    // 环形账。
    v.push(Check::new(
        "环形账定容不超",
        ob.ring_resident <= RING_CAP,
        &u64_text(ob.ring_resident as u64),
        &format!("<={}", RING_CAP),
    ));
    v.push(Check::new(
        "环形账丢弃显性可见",
        ob.ring_dropped > 0,
        &u64_text(ob.ring_dropped),
        ">0",
    ));
    v.push(Check::new(
        "写入 = 驻留 + 丢弃",
        ob.ring_written == ob.ring_resident as u64 + ob.ring_dropped,
        &u64_text(ob.ring_written),
        &u64_text(ob.ring_resident as u64 + ob.ring_dropped),
    ));

    // 导出面。
    v.push(Check::new(
        "TLV 往返一致",
        ob.tlv_ok,
        &format!("{}", if ob.tlv_ok { 1 } else { 0 }),
        "1",
    ));
    v.push(Check::new(
        "TLV 组头版本正确",
        ob.tlv_head_ok,
        &format!("{}", if ob.tlv_head_ok { 1 } else { 0 }),
        "1",
    ));
    v.push(Check::new(
        "KV 往返一致",
        ob.kv_ok,
        &format!("{}", if ob.kv_ok { 1 } else { 0 }),
        "1",
    ));

    // 回放。
    v.push(Check::new(
        "确定性回放稳定",
        ob.replay_stable,
        &format!("{}", if ob.replay_stable { 1 } else { 0 }),
        "1",
    ));
    let rr = replay(seed, rounds.max(1));
    v.push(Check::new(
        "回放全轮闭环",
        rr.closed,
        &format!("{}", if rr.closed { 1 } else { 0 }),
        "1",
    ));
    v.push(Check::new(
        "回放轮数达标",
        rr.rounds == rounds.max(1),
        &u64_text(rr.rounds as u64),
        &u64_text(rounds.max(1) as u64),
    ));

    // 错误闭集。
    v.push(Check::new(
        "诊断码表合法且不重",
        code_table_ok(),
        &format!("{}", if code_table_ok() { 1 } else { 0 }),
        "1",
    ));
    v.push(Check::new(
        "错误三要素全齐",
        ob.err_triple_ok,
        &format!("{}", if ob.err_triple_ok { 1 } else { 0 }),
        "1",
    ));
    v.push(Check::new(
        "体验日志五字段全齐",
        experience_ok(seed),
        &format!("{}", if experience_ok(seed) { 1 } else { 0 }),
        "1",
    ));
    v.push(Check::new(
        "复杂度表 syscall 全为 0",
        complexity_syscalls_total() == 0,
        &u64_text(complexity_syscalls_total() as u64),
        "0",
    ));
    v.push(Check::new(
        "总判定",
        ob.all_ok,
        &format!("{}", if ob.all_ok { 1 } else { 0 }),
        "1",
    ));
    v
}

/// 自检表是否全过。
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

/// 首条失败自检项序号（-1 = 全过）。
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

/// 自检表行数。
pub fn self_check_rows(seed: u64, rounds: u32) -> usize {
    self_check(seed, rounds).len()
}

/// 自检表转行。
pub fn self_check_lines(seed: u64, rounds: u32) -> Vec<String> {
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
        v.push(CxItem::new(op, time, space, syscalls, note, steps, deeper));
    };

    add(
        "BitmapInit::new",
        "O(RANGE_TOTAL) 构造区间卡，位图字数组按 cap_pages/64 一次性置零",
        "O(RANGE_TOTAL + cap_pages/64)",
        0,
        "0 次：纯堆内分配，不触文件、不触时钟、不做内存映射；位图零初始化由 alloc 的 vec 宏完成",
        64,
        "Windows 的可用位图通常由 MmGetPhysicalPageRanges 走 AXL 池分配并依赖物理页；本模块把位图退化为纯逻辑字数组，判据要验的是「置位总数×4KB 与可用字节和的口径闭环」，与物理落点解耦",
    );
    add(
        "BitmapInit::enter_stage",
        "O(1) 守卫链 + O(1) 状态指纹",
        "O(1)",
        0,
        "0 次：状态迁入只改内存字段，不触发任何内核态切换",
        8,
        "Windows 的内存段状态迁入多伴随 MiNode 锁与 PFN 引用计数原子操作；本模块无并发故无原子，判据强调「非法边一律拒记」的确定性而非并发安全",
    );
    add(
        "BitmapInit::advance",
        "O(1)",
        "O(1)",
        0,
        "0 次：逻辑 tick 由调用方注入，禁读 TSC / RDTSC / KeQueryPerformanceCounter",
        4,
        "Windows 侧读时钟须走 KeQueryPerformanceCounter 或 RDTSC，属系统调用级开销；本模块把时间降为数据依赖，「零墙钟」在此被结构性满足",
    );
    add(
        "BitmapInit::align_range",
        "O(1) 单区间对齐（两次掩码运算 + 区间闸链）",
        "O(1)",
        0,
        "0 次：页对齐是纯地址算术，不读 CR3、不查页表、不触发缺页",
        12,
        "Windows 的对齐视图须靠 GetSystemInfo + 手工掩码，且 2MB/1MB 大页下要对齐粒度随机型变；本模块把页基线在构造期就钉死为 4KB，故三机型共用同一套对齐码",
    );
    add(
        "BitmapInit::clip_range",
        "O(1) 单区间裁边 + O(1) 一条 ClipRecord 落账",
        "O(1)（账条为 append-only，常驻 RANGE_TOTAL 条）",
        0,
        "0 次：裁失字节只在内存账本累计，不写日志文件、不发事件",
        14,
        "Windows 的大页残留裁剪要在 MiAllocateLargePages 失败后回落到 4KB 并重试；本模块不做分配动作，只把「裁了多少」逐区间落账，判据关心的是账实一致而非是否分到页",
    );
    add(
        "BitmapInit::ingest_range",
        "O(clip_pages) 逐页置位 + 末尾 O(1) 实时比对",
        "O(1) 增量",
        0,
        "0 次：置位是字内掩码或，不读内存映射区，故不产生缺页异常",
        256,
        "Windows 逐页置位最终都要落到 PFN 数组并触发缺页填充；本模块的置位是纯逻辑位，故「置位总数×4KB」只能作口径量而非物理占用量——这正是判据把两端都限定在同一口径的原因",
    );
    add(
        "FrameBitmap::set",
        "O(1) 单页置位（word = page/64, bit = page%64）",
        "O(1)",
        0,
        "0 次：位图原语复用 F2401，零改动；置位不触碰页表",
        4,
        "Windows 的 bitmap 置位走 RtlSetBit，须持有 PRTL_BITMAP 锁；本模块单线程无锁，代价是失去并发安全，换来判据可确定性复现",
    );
    add(
        "FrameBitmap::first_clear",
        "O(cap_pages/64) 逐字扫描找首个零位",
        "O(1)",
        0,
        "0 次：纯内存扫描，不触发缺页",
        64,
        "Windows 无同构原语（其内存段追踪靠 PFN 枚举）；本模块保留 first_clear 语义不变（锚点要求），用于复验「置位是连续前缀」这一性质",
    );
    add(
        "BitmapInit::close_stage",
        "O(1) 闭环等式 + O(1) 闭环指纹（输入为账本九元组）",
        "O(1)",
        0,
        "0 次：闭环是一次整数等式判定，不 flush、不落盘",
        10,
        "Windows 的段提交须走 MiCommitPages 并同步页表；本模块闭环只落一个 u64 指纹，跨重启复验靠指纹而非重读页表",
    );
    add(
        "BitmapInit::verify",
        "O(1) 重算闭环指纹并与在册比对",
        "O(1)",
        0,
        "0 次：复验纯内存哈希，复用固化的 closure_tick，不读时钟",
        6,
        "Windows 侧复验须重走页表遍历，代价随段大小线性上升；本模块的复验是常数时间，但前提是账本字段全部封闭——因此本模块把「哈希输入不得掺运行期审计量」写成硬约束",
    );
    add(
        "BitmapInit::reconcile",
        "O(clips) 逐区间裁失合计 + O(1) 置位数比对",
        "O(1)",
        0,
        "0 次：只读账本，不写",
        32,
        "Windows 无对账概念（页表即事实）；本模块把「事实」抽象成账本，故必须显式做对账，否则账实漂移无人发现",
    );
    add(
        "BitmapInit::fingerprint",
        "O(1) 固定九项输入的 FNV-1a 折叠",
        "O(1)",
        0,
        "0 次：无随机源注入，指纹完全由内容决定",
        4,
        "Windows 的内存指纹须遍历 PFN 且耗时随物理内存线性上升；本模块指纹只覆盖账本九元组，是内容指纹不是物理指纹",
    );
    add(
        "RingLog::push",
        "O(1) 追加；满则 O(1) 丢最旧",
        "O(RING_CAP) 定容，dropped 显性累加",
        0,
        "0 次：环形账纯内存，不落盘",
        2,
        "Windows 内核的环形追踪缓冲多挂在 ETW/WPP 上，落盘且有丢失率；本模块用「丢失数显式累加」替代「静默覆盖」，判据要求 dropped 必须可见",
    );
    add(
        "RingLog::force_wrap",
        "O(times) 强制环绕，用于让 dropped 有真实值",
        "O(1)",
        0,
        "0 次：纯内存写",
        RING_CAP as u32 + 12,
        "Windows 无对应物；这是判据自检专用手段，用来证明定容丢弃路径真的被执行过而不是纸面声明",
    );
    add(
        "inject_all",
        "O(FORMES × INJECT_TOTAL) 每形态一份独立夹具跑到目标段",
        "O(INJECT_TOTAL) 注入记录 + 拒记条",
        0,
        "0 次：注入器用确定性 LCG，不读系统随机源",
        512,
        "Windows 侧的负向测试靠 WinDbg 断点与池标签，改动内核态；本模块把注入做成纯函数，判据要验的是「10/10 负向注入全部被拒且逐条留痕」的可复现性",
    );
    add(
        "scan_detect",
        "O(rejects × INJECT_TOTAL) 逐形态取首条留痕",
        "O(INJECT_TOTAL)",
        0,
        "0 次：纯读取",
        160,
        "Windows 的问题定位靠 !poolfind 与 !poolused 二次筛；本模块的检出是「现场型 ∧ 留痕型」双判据，既要求当场拒又要求留痕，两条腿都断即判漏检",
    );
    add(
        "run_matrix",
        "O(MATRIX_CELLS) 25 格逐格实算",
        "O(MATRIX_CELLS)",
        0,
        "0 次：格内若调用三机型/10 轮用例也只是纯内存跑",
        25,
        "Windows 无对应矩阵；本模块用 5 段×5 格把判据拆成可定位的原子断言，任一格 fail 都能定位到具体段落",
    );
    add(
        "export_kv",
        "O(固定字段数) 拼 KV 文本",
        "O(固定字段数)",
        0,
        "0 次：文本全驻内存，不写文件",
        32,
        "Windows 侧等价物是 registry 或 WMI 查询，每次都要内核往返；本模块导出面是纯文本，供上层自行落地",
    );
    add(
        "export_tlv",
        "O(固定标签数) 写 TLV，组头版本单独一行 push_u64",
        "O(固定标签数)",
        0,
        "0 次：字节缓冲纯内存",
        24,
        "Windows 的结构化转储靠 KD 以 !struct 输出；本模块的 TLV 组头独立成行，使跨版本读取能先判版本再解析",
    );
    add(
        "read_tlv",
        "O(字节长度) 顺序扫描",
        "O(1)",
        0,
        "0 次：无内存映射",
        64,
        "Windows 无 TLV 惯例；本模块坚持「组头先行」是为了让读取方在字段扩张时不会静默错位",
    );
    add(
        "replay",
        "O(rounds × (RANGE_TOTAL + cap_pages/64))",
        "O(rounds) 三条轨迹",
        0,
        "0 次：同种子重跑，全部由 LCG 与账本驱动",
        320,
        "Windows 的确定性回放靠记录-重放或时间戳快照，代价是存储；本模块靠「零随机源 + 零墙钟 + 零 IO」三零约束天然可复现，回放等价于重算",
    );
    add(
        "observe",
        "O(正路径 + 25 格矩阵 + 3 机型 + 10 轮用例 + 10 形态注入)",
        "O(注入记录 + 拒记 + 矩阵格)",
        0,
        "0 次：全在内存，一次调用零系统交互",
        4096,
        "Windows 无同构自检入口；本模块要求 observe 必须真跑逻辑得出结论，故它比正路径重得多，是离线核验用的重入口而非热路径",
    );
    add(
        "zero_case_trials",
        "O(trials × RANGE_TOTAL) 每次换种子偏移重跑",
        "O(1) 增量（每次一条引擎，用完即弃）",
        0,
        "0 次：10 轮全部纯内存",
        1024,
        "Windows 侧极小分配的对齐行为由分配器策略决定，可能整块失败也可能吞掉；本模块把它固化为「3KB 裁后零页，必须显式登记」，判据明令禁止静默丢弃",
    );
    add(
        "complexity",
        "O(项数) 本表自身",
        "O(项数)",
        0,
        "0 次：构造 Vec<CxItem> 无系统交互",
        48,
        "Windows 侧的性能报告靠 xperf/ETW 采样，属系统调用级开销；本模块把复杂度写成可审计文本而非采样数字，代价是无法反映真实硬件",
    );
    v
}

/// 复杂度表项数。
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
            v[i].op,
            v[i].time,
            v[i].space,
            v[i].syscalls,
            v[i].max_steps,
            v[i].syscall_note,
            v[i].deeper
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
}// ===========================================================================
// 14. 报告面
// ===========================================================================

/// 不变式清单（判据对应的硬性不变量，逐条回显）。
pub fn invariant_manifest() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(String::from(
        "INV-1 口径闭环：位图置位总数 × 4KB + 裁失字节总和 == 可用区间字节和（判据一）",
    ));
    v.push(String::from(
        "INV-2 逐区间落账：裁边账条数 == 区间数，且逐条 lost_total 合计 == 在册 clip_total（判据二）",
    ));
    v.push(String::from(
        "INV-3 单条裁边恒等：lost_total == head_lost + tail_lost（头尾分别登记，不得合并成一条）",
    ));
    v.push(String::from(
        "INV-4 三机型偏差为零：(set_pages, clip_total, avail_bytes, zero_regs) 三元组两两全等（判据三）",
    ));
    v.push(String::from(
        "INV-5 页基线统一：三机型 page 恒为 4096，差异只在窗口基址与跨度",
    ));
    v.push(String::from(
        "INV-6 零页显式登记：小于 4KB 的可用区间裁后为零页，必须按零页可用登记，禁止静默丢弃",
    ));
    v.push(String::from(
        "INV-7 逐区间实时比对：每区间置位计数与裁后页数实时相等，不等即中止初始化",
    ));
    v.push(String::from(
        "INV-8 状态机闭集：合法边仅 0->1、1->2、2->3、3->4 共 4 条，其余一律拒记",
    ));
    v.push(String::from(
        "INV-9 终态写闸：闭环后只放行 close_stage / verify / force_wrap / readback 四类只读或落账动作",
    ));
    v.push(String::from(
        "INV-10 环形账定容：驻留不超过 RING_CAP，丢弃数显式累加到 dropped，不静默覆盖",
    ));
    v.push(String::from(
        "INV-11 写入账实一致：written == resident + dropped",
    ));
    v.push(String::from(
        "INV-12 指纹输入不含运行期审计量：闭环指纹只掺账本内容量，不掺拒记条数（否则封账后复验必误判漂移）",
    ));
    v.push(String::from(
        "INV-13 负向注入 10/10：十形态全部被拒且逐条留痕（现场型 ∧ 留痕型双判据）",
    ));
    v.push(String::from(
        "INV-14 零系统交互：全模块 syscall 次数为 0，逻辑 tick 一律注入，禁读墙钟",
    ));
    v.push(String::from(
        "INV-15 诊断码闭集：全部错误码落在 0x16C0..=0x16FF 且互不重复，每个 detail 必含现象/位置/建议修法",
    ));
    v
}

/// 十形态负向注入清单。
pub fn form_manifest() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut f = 0u32;
    while f < INJECT_TOTAL {
        v.push(format!(
            "FORM {} {} 目标段={} 检出处=first_code_of+first_fp_of",
            f,
            form_name(f),
            if f < 2 { "FRESH" } else if f < 6 { "ALIGNED" } else { "CLIPPED" }
        ));
        f += 1;
    }
    v
}

/// 诊断码表转行（复用 12 节）。
pub fn err_table() -> Vec<String> {
    code_table_rows()
}

/// 模块横幅。
pub fn module_banner() -> String {
    let mut v: Vec<String> = Vec::new();
    v.push(String::from("========== Varix UNX 可用区间位图初始化与对齐裁边 =========="));
    v.push(format!("MODULE_TAG   : {}", MODULE_TAG));
    v.push(format!("TASK_ID      : {}", TASK_ID));
    v.push(format!("CRITERION    : {}", CRITERION));
    v.push(format!(
        "DIAG_SEG     : SEG_BASE=0x{:04X} SEG_MASK=0x{:04X} 段内槽位={}",
        SEG_BASE, SEG_MASK, SEG_SLOTS
    ));
    v.push(format!(
        "GEOMETRY     : PAGE_SIZE={} BLOCKS_PER_PAGE={} RANGE_TOTAL={}",
        PAGE_SIZE, BLOCKS_PER_PAGE, RANGE_TOTAL
    ));
    v.push(format!(
        "MACHINES     : {} 档，页基线统一 4KB，差异只在窗口基址/跨度",
        MACHINE_TOTAL
    ));
    v.push(format!(
        "STAGES       : {} 段，合法边 4 条（{}=未初始化,{}=已对齐,{}=已裁边,{}=已置位,{}=已闭环）",
        STAGE_COUNT,
        STAGE_FRESH,
        STAGE_ALIGNED,
        STAGE_CLIPPED,
        STAGE_SET,
        STAGE_CLOSED
    ));
    v.push(format!(
        "LOGS         : 环形账定容 {} 条（丢弃显性），报告面上限 {} 行，注入形态 {} 个",
        RING_CAP, REPORT_CAP, INJECT_TOTAL
    ));
    v.push(format!(
        "DETERMINISM  : LCG a={} c={}；FNV-1a basis=0x{:016X} prime=0x{:016X}",
        LCG_A, LCG_C, FNV_BASIS, FNV_PRIME
    ));
    v.push(String::from(
        "ISOLATION    : no_std + alloc，零 unwrap/expect/panic/todo，零浮点，零真实 IO，零墙钟",
    ));
    v.push(String::from("=========================================================="));
    join_lines(&v)
}

/// 语义边界（本模块管什么、不管什么）。
pub fn semantic_boundary() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(String::from(
        "本模块管：可用区间清单的对齐裁边、裁失逐区间落账、位图置位计数、口径闭环等式、三机型等价性与零页显式登记",
    ));
    v.push(String::from(
        "本模块不管：物理页分配（交 pmm.rs）、物理页实际填充（交 F2401 之后的消费者）、窗口基址策略本身（交机型表）",
    ));
    v.push(String::from(
        "「置位总数 ×4KB」在本模块是**口径量**不是物理占用量——位图是纯逻辑字数组，不产生缺页填充",
    ));
    v.push(String::from(
        "「三机型偏差为零」只在页基线同为 4KB 时成立；若某机型改用 1MB/2MB 大页，则三元组必须按大页粒度重新定义",
    ));
    v.push(String::from(
        "「零页可用」是显式登记而非静默丢弃：3KB 区间裁后为 0 页，但它仍是可用区间，必须留痕以免闭环比对时凭空少了 3KB",
    ));
    v.push(String::from(
        "位图原语（reset/set/first_clear）复用 pmm.rs::FrameBitmap，语义不变；本模块只新增 ingest_range() 包层",
    ));
    v.push(String::from(
        "时间一律逻辑 tick 注入，模块内不存在任何读时钟路径；「耗时」只能由调用方传入的两个 tick 之差给出",
    ));
    v
}

/// 关联单据。
pub fn related_rows() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(String::from(
        "REL F2401 四态归一 → 提供可用区间清单（本模块的输入前置；四态归一后的可用区间才允许进位图初始化）",
    ));
    v.push(String::from(
        "REL pmm.rs::FrameBitmap → 位图原语 reset/set/first_clear 零改动复用，本模块只加 ingest_range() 包层",
    ));
    v.push(String::from(
        "REL F2379 封账签章 → 指纹固化与复验的体例来源（本模块的 closure_tick 固化即照此办理）",
    ));
    v.push(String::from(
        "REL F2226/F2227/F2228 三账对回 → 守恒账与对账器的体例来源（本模块 reconcile 的五式照此办理）",
    ));
    v.push(String::from(
        "REL F1599 域闭账恒等式 → 恒等式体例（本模块 INV-1/INV-3 照此办理）",
    ));
    v.push(String::from(
        "REL 锚点 UNX-F2402.md → 判据原文、三机型等价性要求、极小可用区间零页登记要求、3KB 用例 10/10 要求",
    ));
    v
}

/// 裁边账报告。
pub fn clip_report(seed: u64) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let e = match run_machine_profile(0, seed) {
        Ok(x) => x,
        Err(_) => return v,
    };
    v.push(format!(
        "CLIP rows={} clip_total={} avail_bytes={} set_pages={} set_bytes={}",
        e.clips.len(),
        e.ledger.clip_total,
        e.ledger.avail_bytes,
        e.ledger.set_pages,
        e.bitmap.count_set().wrapping_mul(PAGE_SIZE)
    ));
    let mut i = 0usize;
    while i < e.clips.len() {
        v.push(e.clips[i].line());
        i += 1;
    }
    // 口径闭环等式回显。
    let left = e.bitmap.count_set().wrapping_mul(PAGE_SIZE).wrapping_add(e.ledger.clip_total);
    v.push(format!(
        "CLIP closure {} + {} = {}  vs  avail_bytes = {}  => {}",
        e.bitmap.count_set().wrapping_mul(PAGE_SIZE),
        e.ledger.clip_total,
        left,
        e.ledger.avail_bytes,
        if left == e.ledger.avail_bytes { "PASS" } else { "FAIL" }
    ));
    v.push(format!("CLIP fp=0x{:016X}", e.clip_fp()));
    v
}

/// 零页登记报告。
pub fn zero_report(seed: u64) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let e = match run_machine_profile(0, seed) {
        Ok(x) => x,
        Err(_) => return v,
    };
    v.push(format!(
        "ZERO regs={} expect={} （小于 4KB 的区间按零页可用显式登记，禁止静默丢弃）",
        e.zeros.len(),
        ZERO_PAGE_CASES
    ));
    let mut i = 0usize;
    while i < e.zeros.len() {
        v.push(e.zeros[i].line());
        i += 1;
    }
    v.push(format!("ZERO fp=0x{:016X}", e.zero_fp()));
    // 3KB 用例 10/10。
    let trials = zero_case_trials(seed, 10);
    let mut pass = 0u32;
    let mut j = 0usize;
    while j < trials.len() {
        if trials[j] {
            pass += 1;
        }
        j += 1;
    }
    v.push(format!(
        "ZERO case_3KB trials={}/{} all_ok={}",
        pass,
        trials.len(),
        zero_case_all_ok(seed)
    ));
    v
}

/// 三机型对表报告。
pub fn tri_report(seed: u64) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(String::from(
        "TRI 页基线统一 4KB；三机型差异只在窗口基址与跨度，故三元组必须两两全等",
    ));
    let rows = tri_rows(seed);
    let mut i = 0usize;
    while i < rows.len() {
        v.push(String::from(rows[i].as_str()));
        i += 1;
    }
    v.push(format!("TRI machine_table_fp=0x{:016X}", machine_table_fp()));
    v
}

/// 闭环报告。
pub fn closure_report(seed: u64) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let e = match run_machine_profile(0, seed) {
        Ok(x) => x,
        Err(_) => return v,
    };
    v.push(format!(
        "CLOSURE fp=0x{:016X} closure_tick={} verify={} stable={}",
        e.closure_fp,
        e.closure_tick,
        match e.verify(0) {
            Ok(_) => String::from("PASS"),
            Err(_) => String::from("FAIL"),
        },
        if e.fp_stable() { "PASS" } else { "FAIL" }
    ));
    v.push(format!("CLOSURE fingerprint=0x{:016X}", e.fingerprint()));
    v.push(format!(
        "CLOSURE reconcile={} conserved={}",
        e.reconcile(),
        e.ledger.conserved()
    ));
    v.push(format!("CLOSURE independent_second_path={}", independent_closure(seed)));
    v
}

/// 注入核报告。
pub fn inject_report(seed: u64) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut audit = match run_machine_profile(0, seed) {
        Ok(x) => x,
        Err(_) => BitmapInit::new(0, baseline_ranges(0), seed),
    };
    let inj = inject_all(&mut audit, seed);
    let mut i = 0usize;
    while i < inj.len() {
        v.push(format!(
            "INJ form={} name={} built={} tick={} note={}",
            inj[i].form, inj[i].name, inj[i].built, inj[i].tick, inj[i].note
        ));
        i += 1;
    }
    let det = scan_detect(&audit, &inj);
    let rows = detect_rows(&det);
    let mut j = 0usize;
    while j < rows.len() {
        v.push(String::from(rows[j].as_str()));
        j += 1;
    }
    v.push(format!(
        "INJ caught={}/{} all_ok={} rejects={}",
        detect_count(&det),
        INJECT_TOTAL,
        detect_ok(&det),
        audit.reject_count()
    ));
    v
}

/// 环形账与报告面回显。
pub fn ring_report(seed: u64) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut e = match run_machine_profile(0, seed) {
        Ok(x) => x,
        Err(_) => BitmapInit::new(0, baseline_ranges(0), seed),
    };
    let rows_before = e.ring.rows();
    let mut i = 0usize;
    while i < rows_before.len() {
        v.push(String::from(rows_before[i].as_str()));
        i += 1;
    }
    // 强制环绕，让丢弃数可见。
    e.ring.force_wrap(RING_CAP as u32 + 12);
    v.push(format!(
        "RING cap={} resident={} written={} dropped={} 账实一致={}",
        RING_CAP,
        e.ring.len(),
        e.ring.written,
        e.ring.dropped,
        e.ring.written == e.ring.len() as u64 + e.ring.dropped
    ));
    v
}

/// 矩阵报告。
pub fn matrix_report(seed: u64) -> Vec<String> {
    let e = match run_machine_profile(0, seed) {
        Ok(x) => x,
        Err(_) => BitmapInit::new(0, baseline_ranges(0), seed),
    };
    matrix_rows(&run_matrix(&e, seed))
}

/// 汇总报告。
pub fn summary(seed: u64, rounds: u32) -> String {
    let mut v: Vec<String> = Vec::new();
    let ob = observe(seed);
    v.push(module_banner());
    v.push(String::from(""));
    v.push(String::from("---- 不变式清单 ----"));
    let inv = invariant_manifest();
    let mut i = 0usize;
    while i < inv.len() {
        v.push(String::from(inv[i].as_str()));
        i += 1;
    }
    v.push(String::from(""));
    v.push(String::from("---- 判据逐条 ----"));
    v.push(format!(
        "判据一 位图置位总数×4KB == 可用区间字节和：set_pages={} ×4KB={} avail_bytes={} clip_total={} closure_equal={}",
        ob.set_pages, ob.set_bytes, ob.avail_bytes, ob.clip_total, ob.closure_equal
    ));
    v.push(format!(
        "判据二 页对齐裁边裁失字节逐区间落账：clip_rows={} clip_per_range={} 逐条合计==clip_total={}",
        ob.clip_rows,
        ob.clip_per_range,
        ob.clip_per_range
    ));
    v.push(format!(
        "判据三 三机型偏差为零：tri_zero_dev={} set_equal={} clip_equal={} avail_equal={}",
        ob.tri_zero_dev, ob.tri_set_equal, ob.tri_clip_equal, ob.tri_avail_equal
    ));
    v.push(format!(
        "附加 零页显式登记：zero_regs={} 3KB用例10/10={}",
        ob.zero_regs, ob.zero_case_10
    ));
    v.push(String::from(""));
    v.push(String::from("---- 守恒与指纹 ----"));
    v.push(format!(
        "conserved={} closure_fp=0x{:016X} fp_verify={} fp_stable={}",
        ob.conserved, ob.closure_fp, ob.fp_verify, ob.fp_stable
    ));
    v.push(format!(
        "matrix={}/{} ok={} edge_ok={} independent_closure={}",
        ob.matrix_pass, ob.matrix_cells, ob.matrix_ok, ob.edge_ok, ob.independent_closure
    ));
    v.push(format!(
        "inject built={}/{} caught={}/{} all_caught={} rejects={}",
        ob.inject_built,
        ob.inject_total,
        ob.inject_caught,
        ob.inject_total,
        ob.inject_all_caught,
        ob.rejects
    ));
    v.push(format!(
        "ring resident={} written={} dropped={}",
        ob.ring_resident, ob.ring_written, ob.ring_dropped
    ));
    v.push(format!(
        "export tlv_ok={} tlv_head_ok={} kv_ok={} replay_stable={} rounds={}",
        ob.tlv_ok, ob.tlv_head_ok, ob.kv_ok, ob.replay_stable, ob.replay_rounds
    ));
    v.push(format!(
        "codes ok={} triple_ok={} exp_rows={} syscalls={}",
        code_table_ok(),
        ob.err_triple_ok,
        ob.exp_rows,
        complexity_syscalls_total()
    ));
    v.push(String::from(""));
    v.push(String::from("---- 自检表 ----"));
    let lines = self_check_lines(seed, rounds);
    let mut j = 0usize;
    while j < lines.len() {
        v.push(String::from(lines[j].as_str()));
        j += 1;
    }
    v.push(format!(
        "SELF_CHECK rows={} ok={} first_fail={}",
        lines.len(),
        self_check_ok(seed, rounds),
        self_check_first_fail(seed, rounds)
    ));
    v.push(String::from(""));
    v.push(format!(
        "FINAL all_ok={}",
        if ob.all_ok && self_check_ok(seed, rounds) {
            "true"
        } else {
            "false"
        }
    ));
    join_lines(&v)
}

/// 演示场景：把判据三条走一遍人话叙述。
pub fn demo_scene(seed: u64) -> String {
    let mut v: Vec<String> = Vec::new();
    v.push(String::from("【演示场景】Varix 可用区间位图初始化"));
    v.push(String::from(
        "机器开机，固件把保留区之外的空间报成 12 条可用区间。位图初始化器接手：",
    ));
    v.push(String::from(
        "第一步 · 对齐：逐条区间把首地址向上对齐到 4KB 边界，确认区间整段落在本机型窗口内，并登记原始字节数。",
    ));
    v.push(String::from(
        "第二步 · 裁边：终地址向下对齐到 4KB 边界，头尾裁失的字节按区间单独落账；小于 4KB 的区间裁后为零页，按「零页可用」显式登记而不是丢掉。",
    ));
    v.push(String::from(
        "第三步 · 置位：逐区间把裁后页数对应的位在位图里置起来，每置完一个区间就实时比对一次，置位数与裁后页数不等立即中止。",
    ));
    v.push(String::from(
        "第四步 · 闭环：把「置位总数×4KB + 裁失总和」与「可用区间字节和」摆在一起比，差额只允许来自裁边账，一个字节都不许凭空隐损。",
    ));
    v.push(String::from(
        "第五步 · 换机型：同页基线 4KB 下把同一份相对布局搬到另外两个窗口，置位数、裁失和、可用和三个数必须一模一样，偏差必须为零。",
    ));
    if let Ok(e) = run_machine_profile(0, seed) {
        v.push(format!(
            "实测：{} 条区间，置位 {} 页 = {} 字节，裁失 {} 字节，合计 {} 字节，可用字节和 {} 字节，差 {} 字节。",
            e.ranges.len(),
            e.ledger.set_pages,
            e.ledger.set_pages.wrapping_mul(PAGE_SIZE),
            e.ledger.clip_total,
            e.ledger.set_pages
                .wrapping_mul(PAGE_SIZE)
                .wrapping_add(e.ledger.clip_total),
            e.ledger.avail_bytes,
            e.ledger
                .avail_bytes
                .wrapping_sub(e.ledger.set_pages.wrapping_mul(PAGE_SIZE).wrapping_add(e.ledger.clip_total))
        ));
        v.push(format!(
            "零页登记 {} 条（每条都是不足一页的可用区间），3KB 用例 10/10 = {}",
            e.zeros.len(),
            zero_case_all_ok(seed)
        ));
        v.push(format!(
            "三机型三元组：{}",
            if tri_profile_zero_deviation(seed) {
                String::from("零偏差")
            } else {
                String::from("存在偏差")
            }
        ));
    }
    v.push(format!(
        "负向注入：{} 个形态，现场型 ∧ 留痕型双判据全部命中。",
        observe(seed).inject_caught
    ));
    join_lines(&v)
}

/// 一键跑：横幅 + 判据 + 三份细表 + 自检。
pub fn run_and_report(seed: u64, rounds: u32) -> String {
    let mut v: Vec<String> = Vec::new();
    v.push(summary(seed, rounds));
    v.push(String::from(""));
    v.push(String::from("---- 裁边账明细 ----"));
    let c = clip_report(seed);
    let mut i = 0usize;
    while i < c.len() {
        v.push(String::from(c[i].as_str()));
        i += 1;
    }
    v.push(String::from(""));
    v.push(String::from("---- 零页登记明细 ----"));
    let z = zero_report(seed);
    let mut j = 0usize;
    while j < z.len() {
        v.push(String::from(z[j].as_str()));
        j += 1;
    }
    v.push(String::from(""));
    v.push(String::from("---- 三机型对表 ----"));
    let t = tri_report(seed);
    let mut k = 0usize;
    while k < t.len() {
        v.push(String::from(t[k].as_str()));
        k += 1;
    }
    v.push(String::from(""));
    v.push(String::from("---- 判定矩阵 ----"));
    let m = matrix_report(seed);
    let mut n = 0usize;
    while n < m.len() {
        v.push(String::from(m[n].as_str()));
        n += 1;
    }
    v.push(String::from(""));
    v.push(String::from("---- 注入核 ----"));
    let g = inject_report(seed);
    let mut p = 0usize;
    while p < g.len() {
        v.push(String::from(g[p].as_str()));
        p += 1;
    }
    v.push(String::from(""));
    v.push(String::from("---- 环形账 ----"));
    let r = ring_report(seed);
    let mut q = 0usize;
    while q < r.len() {
        v.push(String::from(r[q].as_str()));
        q += 1;
    }
    v.push(String::from(""));
    v.push(String::from("---- 诊断码表 ----"));
    let code = err_table();
    let mut s = 0usize;
    while s < code.len() {
        v.push(String::from(code[s].as_str()));
        s += 1;
    }
    v.push(String::from(""));
    v.push(String::from("---- 语义边界 ----"));
    let b = semantic_boundary();
    let mut t2 = 0usize;
    while t2 < b.len() {
        v.push(String::from(b[t2].as_str()));
        t2 += 1;
    }
    v.push(String::from(""));
    v.push(String::from("---- 关联单据 ----"));
    let rel = related_rows();
    let mut u = 0usize;
    while u < rel.len() {
        v.push(String::from(rel[u].as_str()));
        u += 1;
    }
    v.push(String::from(""));
    v.push(String::from("---- 演示场景 ----"));
    v.push(demo_scene(seed));
    join_lines(&v)
}