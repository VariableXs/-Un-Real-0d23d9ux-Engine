//! UNX-F3361 · 时间窗模型：TSC 时钟基准与窗口定义（微/秒/分三级嵌套）
//!
//! 任务单号: UNX-F3361
//! 标题: 时间窗模型：TSC 时钟基准与窗口定义
//! 域/批: A4/G8（CoRun·UNX-A5 段｜ktest/框架/测试基建域）
//! 册目标: 320 行（判据 J1 时间窗模型文档 + M-120 窗口边界行为验证）
//!
//! 判据原文（逐字摘抄，来源《CoRun Varix STAR II · Unxreal》任务书表格）:
//! 「UNX-F3361 | 时间窗模型：TSC 时钟基准与窗口定义 | 320 | 骨架 |
//!   J1：时间窗模型文档（基准钟/窗口长度/三级窗口微/秒/分）交付；M-120 窗口边界行为验证。」
//!
//! 说明（锚点目录同名文件的辨析）:
//! `_attic/.../anchors/UNX-F3361.md` 内实为 **UNX-F33610 · 路由表变更事件广播地基（I1 消费）**，
//! 与本单无关（编号差一位的同书他人条目）。本单判据以任务书表格 `| UNX-F3361 |` 行为准。
//!
//! 本单核心语义（三层，逐层落地）:
//! - **基准钟（TSC 语义）**：`BenchClock` 只保证**单调非减**，并对「回退」显性留痕
//!   （`rewind_marks`），因为时间窗的一切正确性都建立在「钟不回退」之上。
//! - **三级窗口嵌套**：`微 → 秒 → 分` 长度严格倍比 `NEST_RATIO = 16`，
//!   即 `w_min = w_sec * 16 = w_micro * 256`。非嵌套即 `LevelNotNested`。
//! - **左闭右开区间**：窗口 `w` 覆盖 `[w*len, (w+1)*len)`；右端点**归下一窗**，
//!   边界行为由 `BoundaryRule` 显式建模并在 `boundary_probe()` 中点验（M-120 落点）。
//!
//! 深度骨架（对应批次通用要求）:
//! 常量表 / 4 核心结构（BenchClock、WindowSpec、WindowBank、Histogram）/
//! 14 错误变体 / 6 辅助码 / 主状态机（合法边闭集 + 非法转换拒记）/
//! 守恒账 `reconcile()` / append-only 环形账（`dropped` 显性）/
//! 注入核 14/14 全拒留痕 / 冻结指纹 `fingerprint() + verify()` /
//! `export_kv() + export_tlv()` / 确定性 `replay(seed)` / `observe(seed)` /
//! `complexity()` 逐项性能分解（对标 Windows 同类能力写「深在哪」）。
//!
//! 码段声明:
//! 本单独占诊断码段 `0x2780 ..= 0x27BF`（`SEG_BASE = 0x2780`，`SEG_MASK = 0x003F`）。
//! 错误变体一码一段，逐个占段内偏移 `0x00..=0x0D`；辅助码占 `0x1C..=0x21`，
//! 最大占用 `0x27A1`，未触及段上界 `0x27BF`。

#![allow(dead_code)]
#![allow(unused_imports)]

extern crate alloc;

use alloc::{format, string::{String, ToString}, vec, vec::Vec};

// ===========================================================================
// 1. 常量表
// ===========================================================================

/// 版本标识（冻结指纹第一词）。
pub const V_VERSION_ID: u64 = 0x0001_3361_2780_0001;
/// 版本串。
pub const V_VERSION: &str = "v3361-utimewin.v1";

/// 本单独占码段基址。
pub const SEG_BASE: u16 = 0x2780;
/// 本单独占码段掩码。
pub const SEG_MASK: u16 = 0x003F;
/// 本单独占码段上界（含）。
pub const SEG_END: u16 = SEG_BASE | SEG_MASK;
/// 主码（段内偏移 0）。
pub const MAIN_CODE: u16 = SEG_BASE;
/// 辅助码段内基址。
pub const AUX_BASE_OFF: u16 = 0x1C;

// ---------------------------------------------------------------------------
// 1.1 三级窗口长度（tick 为逻辑单位，零墙钟）
// ---------------------------------------------------------------------------

/// 一微窗容纳的 tick 数。
pub const W_MICRO: u64 = 16;
/// 一秒窗容纳的 tick 数。
pub const W_SEC: u64 = W_MICRO * 16;
/// 一分窗容纳的 tick 数。
pub const W_MIN: u64 = W_SEC * 16;
/// 嵌套倍比（微→秒→分，每级放大 16 倍）。
pub const NEST_RATIO: u64 = 16;
/// 三级窗口层数。
pub const LEVEL_COUNT: usize = 3;

// ---------------------------------------------------------------------------
// 1.2 规模常量
// ---------------------------------------------------------------------------

/// 每级窗口册保存的窗口个数（环形）。
pub const BANK_CAP: usize = 16;
/// 窗口册环掩码（**必须与 BANK_CAP 配套**，不能借用 RING_MASK）。
pub const BANK_MASK: usize = BANK_CAP - 1;
/// 环形账容量（append-only，被覆盖计 `dropped`）。
pub const RING_CAP: usize = 32;
/// 环形账掩码。
pub const RING_MASK: usize = RING_CAP - 1;
/// 延迟直方图桶数（桶宽 `LAT_UNIT` tick）。
pub const HIST_BUCKETS: usize = 16;
/// 直方图桶宽（tick）。
pub const LAT_UNIT: u64 = 8;
/// 单窗事件计数上限（触顶即饱和并显性标记，不允许静默回绕）。
pub const CELL_CAP: u32 = 4096;
/// 判定基准样本数（样本不足即 `SampleShort`）。
pub const MIN_SAMPLES: u32 = 32;
/// 单窗事件步数上限。
pub const STREAM_STEPS: usize = 96;
/// 边界行为点验用例数（M-120）。
pub const BOUNDARY_CASES: usize = 12;
/// 百分位档位数。
pub const PCT_COUNT: usize = 4;
/// 百分位表（放大 100 倍存储，避免浮点）。
pub const PCT_SCALED: [u64; PCT_COUNT] = [5000, 9000, 9900, 9999];
/// 总步数上限。
pub const STEP_CAP: u64 = (LEVEL_COUNT * STREAM_STEPS * 4) as u64;
/// syscall 次数上限（本单零真实 IO，恒 0）。
pub const SYSCALL_CAP: u64 = 0;
/// 峰值内存上限（字节）。
pub const MEM_PEAK_CAP: u64 = 256 * 1024;

// ---------------------------------------------------------------------------
// 1.3 TLV 常量（长度字段由 export_tlv 内 push 次数反推）
// ---------------------------------------------------------------------------

/// TLV 组头标签。
pub const TLV_TAG_HEAD: u8 = 0x01;
/// TLV 组头载荷字节数（= 组头内 `push_u64` 次数 × 8）。
pub const TLV_HEAD_PAYLOAD: usize = 24;
/// TLV 组头总长（1 标签 + 2 长度 + 24 载荷）。
pub const TLV_HEAD_LEN: usize = TLV_HEAD_PAYLOAD + 3;
/// 单条 TLV 定长（1 标签 + 1 长度 + 8 值）。
pub const TLV_ITEM_LEN: usize = 10;
/// TLV 条目总数（静态 44 + 逐形态 14 = 58；由 export_tlv 内 push 次数反推）。
pub const TLV_ITEM_COUNT: usize = 58;

/// 基准钟域面名。
pub const FACE_CLOCK: &str = "基准钟面";
/// 窗口域面名。
pub const FACE_WINDOW: &str = "窗口面";
/// 分位域面名。
pub const FACE_PCT: &str = "分位面";
/// 账目域面名。
pub const FACE_LEDGER: &str = "账目面";
/// 冻结域面名。
pub const FACE_FREEZE: &str = "冻结面";

/// 嫁接源（F3361 与测试基建面同构处）。
pub const GRAFT_SEAT: &str = "F3401/F3421";
/// 对位条位。
pub const DEP_SEAT: &str = "F3381/F3461";

// ===========================================================================
// 2. 确定性基础件（LCG / FNV-1a / 逻辑时钟 / 整数开方）
// ===========================================================================

/// 确定性 LCG 状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lcg {
    /// 当前状态。
    pub state: u64,
}
impl Lcg {
    /// 以种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    /// 推进一步并返回新状态。
    pub fn next(&mut self) -> u64 {
        self.state = lcg_step(self.state);
        self.state
    }
    /// 取低 32 位。
    pub fn next32(&mut self) -> u32 {
        let v = self.next();
        (v & 0xFFFF_FFFF) as u32
    }
    /// 取模取样（模数为 0 时返回 0，绝不做除零）。
    pub fn below(&mut self, m: u64) -> u64 {
        let v = self.next();
        if m == 0 {
            0
        } else {
            v % m
        }
    }
}

/// LCG 单步（`x = a*x + c (mod 2^64)`，全程 wrapping）。
pub fn lcg_step(state: u64) -> u64 {
    state
        .wrapping_mul(6364136223846793005u64)
        .wrapping_add(1442695040888963407u64)
}

/// FNV-1a 64 增量。
pub fn fnv1a64_ext(state: u64, bytes: &[u8]) -> u64 {
    let mut h: u64 = state;
    for b in bytes.iter() {
        let x: u64 = (*b as u64) & 0xFF;
        h = (h ^ x).wrapping_mul(0x100000001b3u64);
    }
    h
}

/// FNV-1a 64（basis `0xcbf29ce484222325`，prime `0x100000001b3`）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    fnv1a64_ext(0xcbf29ce484222325u64, bytes)
}

/// FNV-1a 64 对 `u64` 词表求指纹（每词 8 字节大端折入）。
pub fn fnv1a_words(words: &[u64]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325u64;
    for w in words.iter() {
        let v: u64 = *w;
        let mut i: usize = 0;
        while i < 8 {
            let byte: u8 = ((v >> (56 - 8 * i)) & 0xFF) as u8;
            h = (h ^ (byte as u64)).wrapping_mul(0x100000001b3u64);
            i = i.wrapping_add(1);
        }
    }
    h
}

/// 字符串折入指纹。
pub fn fnv1a_str(state: u64, s: &str) -> u64 {
    fnv1a64_ext(state, s.as_bytes())
}

/// 逻辑时钟推进（零墙钟，一律逻辑 tick）。
pub fn tick_advance(now: u64) -> u64 {
    now.wrapping_add(1)
}

/// 64 位整数平方根（牛顿迭代，全程整数，零浮点）。
///
/// 用于分位面里的「桶内均方根」估计：`rms = isqrt(Σv² / n)`。
/// 注意初值取 `1<<32`，因为 `isqrt(u64::MAX) < 2^32`，保证单调下降且不溢出。
pub fn isqrt_u64(n: u64) -> u64 {
    if n == 0 {
        return 0;
    }
    let mut r: u64 = 1u64 << 32;
    let mut guard: u32 = 0;
    while guard < 80 {
        let y: u64 = (r.wrapping_add(n / r)) >> 1;
        if y >= r {
            break;
        }
        r = y;
        guard = guard.wrapping_add(1);
    }
    r
}

/// `u64` 整除（除数为 0 时给 0，绝不 panic）。
pub fn div_or_zero(a: u64, b: u64) -> u64 {
    if b == 0 {
        0
    } else {
        a / b
    }
}

/// 饱和加（触顶即 `cap`，由调用方显性标记）。
pub fn sat_add_u32(a: u32, b: u32, cap: u32) -> u32 {
    let s: u64 = (a as u64) + (b as u64);
    if s > (cap as u64) {
        cap
    } else {
        s as u32
    }
}

// ===========================================================================
// 3. 错误闭集（14 变体一码一段 + 6 辅助码）
// ===========================================================================

/// 错误形态（段内偏移 `0x00..=0x0D`，一码一段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// 0：基准钟回退（单调性契约破坏）。
    BaseRewind,
    /// 1：窗口规格重号。
    SpecDup,
    /// 2：窗口长度非嵌套（倍比不是 `NEST_RATIO`）。
    LevelNotNested,
    /// 3：窗口级别缺失（微/秒/分缺一）。
    LevelMissing,
    /// 4：边界规则破坏（右开被改成右闭，端点错归）。
    BoundaryRuleBroken,
    /// 5：跨窗归属错误（事件记到非所属窗）。
    CrossWindowMisattr,
    /// 6：窗口滚动跳窗（游标跨越超过一窗）。
    RollSkipped,
    /// 7：计数溢出未显性化（饱和但标志未置）。
    OverflowUnflagged,
    /// 8：分位排名越界（目标位落在样本总数之外）。
    PctRankBad,
    /// 9：嵌套关系破坏（Σ微 ≠ Σ秒 ≠ Σ分）。
    NestingBreach,
    /// 10：环形账不守恒（写入 ≠ 存活 + 丢弃）。
    LedgerUnbalanced,
    /// 11：冻结指纹漂移。
    FingerprintDrift,
    /// 12：守恒式偏离。
    ReconcileDeviation,
    /// 13：样本数不足（判定基准未达 `MIN_SAMPLES`）。
    SampleShort,
}
impl Form {
    /// 形态总数。
    pub const COUNT: usize = 14;
    /// 全部形态（顺序即码段偏移序）。
    pub const ALL: [Form; Form::COUNT] = [
        Form::BaseRewind,
        Form::SpecDup,
        Form::LevelNotNested,
        Form::LevelMissing,
        Form::BoundaryRuleBroken,
        Form::CrossWindowMisattr,
        Form::RollSkipped,
        Form::OverflowUnflagged,
        Form::PctRankBad,
        Form::NestingBreach,
        Form::LedgerUnbalanced,
        Form::FingerprintDrift,
        Form::ReconcileDeviation,
        Form::SampleShort,
    ];
    /// 段内偏移下标。
    pub fn index(self) -> usize {
        match self {
            Form::BaseRewind => 0,
            Form::SpecDup => 1,
            Form::LevelNotNested => 2,
            Form::LevelMissing => 3,
            Form::BoundaryRuleBroken => 4,
            Form::CrossWindowMisattr => 5,
            Form::RollSkipped => 6,
            Form::OverflowUnflagged => 7,
            Form::PctRankBad => 8,
            Form::NestingBreach => 9,
            Form::LedgerUnbalanced => 10,
            Form::FingerprintDrift => 11,
            Form::ReconcileDeviation => 12,
            Form::SampleShort => 13,
        }
    }
    /// 按下标取形态。
    pub fn from_index(i: usize) -> Option<Form> {
        if i < Form::COUNT {
            Some(Form::ALL[i])
        } else {
            None
        }
    }
    /// 诊断码（`SEG_BASE + 偏移`）。
    pub fn code(self) -> u16 {
        SEG_BASE.wrapping_add(self.index() as u16)
    }
    /// 形态名。
    pub fn name(self) -> &'static str {
        match self {
            Form::BaseRewind => "BaseRewind",
            Form::SpecDup => "SpecDup",
            Form::LevelNotNested => "LevelNotNested",
            Form::LevelMissing => "LevelMissing",
            Form::BoundaryRuleBroken => "BoundaryRuleBroken",
            Form::CrossWindowMisattr => "CrossWindowMisattr",
            Form::RollSkipped => "RollSkipped",
            Form::OverflowUnflagged => "OverflowUnflagged",
            Form::PctRankBad => "PctRankBad",
            Form::NestingBreach => "NestingBreach",
            Form::LedgerUnbalanced => "LedgerUnbalanced",
            Form::FingerprintDrift => "FingerprintDrift",
            Form::ReconcileDeviation => "ReconcileDeviation",
            Form::SampleShort => "SampleShort",
        }
    }
    /// 形态描述（中文在档注记）。
    pub fn describe(self) -> &'static str {
        match self {
            Form::BaseRewind => "基准钟回退：采样值小于上次采样，单调性契约破坏",
            Form::SpecDup => "窗口规格重号：同级两窗口同编号",
            Form::LevelNotNested => "窗口长度非嵌套：相邻级倍比不等于 16",
            Form::LevelMissing => "窗口级别缺失：微/秒/分三级缺一",
            Form::BoundaryRuleBroken => "边界规则破坏：右端点被归入本窗（右闭）",
            Form::CrossWindowMisattr => "跨窗归属错误：事件记入非所属窗口",
            Form::RollSkipped => "窗口滚动跳窗：游标一次跨越超过一个窗口",
            Form::OverflowUnflagged => "计数溢出未显性化：已饱和但溢出标志未置",
            Form::PctRankBad => "分位排名越界：桶和与样本总数失配",
            Form::NestingBreach => "嵌套关系破坏：三级窗口计数和不相等",
            Form::LedgerUnbalanced => "环形账不守恒：写入数 ≠ 存活 + 丢弃",
            Form::FingerprintDrift => "冻结指纹漂移：模型定义件哈希与入册值失配",
            Form::ReconcileDeviation => "守恒式偏离：Σ受理 ≠ Σ分窗计数",
            Form::SampleShort => "样本数不足：分位判定基准未达下限",
        }
    }
    /// 探测面名。
    pub fn probe(self) -> &'static str {
        match self {
            Form::BaseRewind => "clock.monotonic",
            Form::SpecDup => "spec.dup_bits",
            Form::LevelNotNested => "spec.nested",
            Form::LevelMissing => "spec.level_bits",
            Form::BoundaryRuleBroken => "boundary.rule_ok",
            Form::CrossWindowMisattr => "window.attr_ok",
            Form::RollSkipped => "window.roll_steps",
            Form::OverflowUnflagged => "window.overflow_flag",
            Form::PctRankBad => "pct.rank_ok",
            Form::NestingBreach => "nest.sums_equal",
            Form::LedgerUnbalanced => "ring.balanced",
            Form::FingerprintDrift => "freeze.verify",
            Form::ReconcileDeviation => "recon.deviation",
            Form::SampleShort => "pct.sample_enough",
        }
    }
    /// 所属域面。
    pub fn face(self) -> &'static str {
        match self {
            Form::BaseRewind => FACE_CLOCK,
            Form::SpecDup | Form::LevelNotNested | Form::LevelMissing => FACE_WINDOW,
            Form::BoundaryRuleBroken | Form::CrossWindowMisattr | Form::RollSkipped => FACE_WINDOW,
            Form::OverflowUnflagged => FACE_LEDGER,
            Form::PctRankBad | Form::SampleShort => FACE_PCT,
            Form::NestingBreach | Form::ReconcileDeviation => FACE_LEDGER,
            Form::LedgerUnbalanced => FACE_LEDGER,
            Form::FingerprintDrift => FACE_FREEZE,
        }
    }
    /// 是否属「基准钟/边界类」（模型定义面）。
    pub fn is_def(self) -> bool {
        matches!(
            self,
            Form::BaseRewind | Form::SpecDup | Form::LevelNotNested | Form::LevelMissing | Form::BoundaryRuleBroken
        )
    }
    /// 是否属「账目类」（守恒面）。
    pub fn is_ledger(self) -> bool {
        matches!(
            self,
            Form::OverflowUnflagged
                | Form::NestingBreach
                | Form::LedgerUnbalanced
                | Form::ReconcileDeviation
                | Form::CrossWindowMisattr
                | Form::RollSkipped
        )
    }
    /// 是否属「分位类」。
    pub fn is_pct(self) -> bool {
        matches!(self, Form::PctRankBad | Form::SampleShort)
    }
    /// 形态类名。
    pub fn class_name(self) -> &'static str {
        if self.is_def() {
            "定义面"
        } else if self.is_pct() {
            "分位面"
        } else if self.is_ledger() {
            "账目面"
        } else {
            "其他面"
        }
    }
}

/// 辅助码（段内偏移 `0x1C..=0x21`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuxCode {
    /// 0：步数越界。
    StepOverCap,
    /// 1：分级游标不同步。
    CursorSkew,
    /// 2：分位桶越界。
    BucketOver,
    /// 3：分位目标位零样本。
    ZeroSample,
    /// 4：饱和后仍继续累加。
    SatAfterFlag,
    /// 5：语义边界越界（本单不读真实 TSC 寄存器）。
    SemanticBorder,
}
impl AuxCode {
    /// 辅助码总数。
    pub const COUNT: usize = 6;
    /// 全部辅助码。
    pub const ALL: [AuxCode; AuxCode::COUNT] = [
        AuxCode::StepOverCap,
        AuxCode::CursorSkew,
        AuxCode::BucketOver,
        AuxCode::ZeroSample,
        AuxCode::SatAfterFlag,
        AuxCode::SemanticBorder,
    ];
    /// 段内偏移下标。
    pub fn index(self) -> usize {
        match self {
            AuxCode::StepOverCap => 0,
            AuxCode::CursorSkew => 1,
            AuxCode::BucketOver => 2,
            AuxCode::ZeroSample => 3,
            AuxCode::SatAfterFlag => 4,
            AuxCode::SemanticBorder => 5,
        }
    }
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE
            .wrapping_add(AUX_BASE_OFF)
            .wrapping_add(self.index() as u16)
    }
    /// 辅助码名。
    pub fn name(self) -> &'static str {
        match self {
            AuxCode::StepOverCap => "StepOverCap",
            AuxCode::CursorSkew => "CursorSkew",
            AuxCode::BucketOver => "BucketOver",
            AuxCode::ZeroSample => "ZeroSample",
            AuxCode::SatAfterFlag => "SatAfterFlag",
            AuxCode::SemanticBorder => "SemanticBorder",
        }
    }
    /// 辅助码描述。
    pub fn describe(self) -> &'static str {
        match self {
            AuxCode::StepOverCap => "逻辑步数越过 STEP_CAP 上限",
            AuxCode::CursorSkew => "三级窗口游标未同步到同一逻辑时刻",
            AuxCode::BucketOver => "延迟直方图桶下标越界",
            AuxCode::ZeroSample => "分位计算面对零样本",
            AuxCode::SatAfterFlag => "计数已置饱和标志却仍发生累加",
            AuxCode::SemanticBorder => "语义边界越界：本单不读真实 TSC 寄存器",
        }
    }
}

/// 本单结果别名（自定义 `Result` 闭集）。
pub type TwResult<T> = Result<T, TwErr>;

/// 错误对象（三要素：`code()` + `detail()` + `full()`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TwErr {
    /// 诊断码。
    pub code: u16,
    /// 明文细节。
    pub detail: String,
}
impl TwErr {
    /// 构造。
    pub fn new(code: u16, detail: String) -> TwErr {
        TwErr { code, detail }
    }
    /// 由形态构造。
    pub fn of_form(f: Form, detail: String) -> TwErr {
        TwErr { code: f.code(), detail }
    }
    /// 由辅助码构造。
    pub fn of_aux(a: AuxCode, detail: String) -> TwErr {
        TwErr { code: a.code(), detail }
    }
    /// 三要素之一：码。
    pub fn code(&self) -> u16 {
        self.code
    }
    /// 三要素之二：明细。
    pub fn detail(&self) -> String {
        self.detail.clone()
    }
    /// 三要素之三：全串。
    pub fn full(&self) -> String {
        format!("0x{:04X} {}", self.code, self.detail)
    }
}

/// 码 → 名称反查（供错误全串标注）。
pub fn name_by_code(code: u16) -> Option<&'static str> {
    let mut i: usize = 0;
    while i < Form::COUNT {
        let f = Form::ALL[i];
        if f.code() == code {
            return Some(f.name());
        }
        i = i.wrapping_add(1);
    }
    let mut j: usize = 0;
    while j < AuxCode::COUNT {
        let a = AuxCode::ALL[j];
        if a.code() == code {
            return Some(a.name());
        }
        j = j.wrapping_add(1);
    }
    None
}
/// 错误一行串。
pub fn err_str(e: &TwErr) -> String {
    match name_by_code(e.code) {
        Some(n) => format!("[{}] {}", n, e.detail),
        None => format!("[0x{:04X}] {}", e.code, e.detail),
    }
}
/// 冻结态不符错误。
pub fn err_frozen(got: &str, want: &str) -> TwErr {
    TwErr::of_form(
        Form::FingerprintDrift,
        format!("冻结态不符：got={} want={}", got, want),
    )
}
/// 越界错误。
pub fn err_range(what: &str, idx: usize, cap: usize) -> TwErr {
    TwErr::of_aux(
        AuxCode::BucketOver,
        format!("越界：{} idx={} cap={}", what, idx, cap),
    )
}
/// 守恒式错误。
pub fn err_recon(a: u64, b: u64, c: u64) -> TwErr {
    TwErr::of_form(
        Form::ReconcileDeviation,
        format!("守恒式偏离：受理={} 分窗合计={} 总账={}", a, b, c),
    )
}
/// 码段合法性。
pub fn code_in_seg(code: u16) -> bool {
    code >= SEG_BASE && code <= SEG_END
}
/// 主码串。
pub fn seg_line() -> String {
    format!(
        "seg 0x{:04X}..=0x{:04X} mask=0x{:04X} forms={} aux={} max=0x{:04X}",
        SEG_BASE,
        SEG_END,
        SEG_MASK,
        Form::COUNT,
        AuxCode::COUNT,
        AuxCode::ALL[AuxCode::COUNT - 1].code()
    )
}

// ===========================================================================
// 4. 核心结构一：基准钟（BenchClock）
// ===========================================================================

/// 基准钟读数来源（TSC 语义，本单不触碰真实寄存器）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClockSource {
    /// 逻辑推进：读数 = 上次 + 步长。
    LogicalStep,
    /// 逻辑采样：读数 = 上次 + LCG 取样步长。
    Sampled,
    /// 外部给定读数（对拍用）。
    Given,
}
impl ClockSource {
    /// 源名。
    pub fn name(self) -> &'static str {
        match self {
            ClockSource::LogicalStep => "LogicalStep",
            ClockSource::Sampled => "Sampled",
            ClockSource::Given => "Given",
        }
    }
    /// 是否属「逻辑源」（真机寄存器为否）。
    pub fn is_logical(self) -> bool {
        match self {
            ClockSource::LogicalStep | ClockSource::Sampled => true,
            ClockSource::Given => true,
        }
    }
}
/// TSC 语义基准钟。
///
/// **单调性契约**：任何一次 `advance_to(t)` 之后，钟值恒 `>=` 上次钟值；
/// 若传入更小的读数，则**拒绝推进**并累加 `rewind_marks`（显性留痕，不静默吞掉）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BenchClock {
    /// 当前钟值（逻辑 tick）。
    pub now: u64,
    /// 采样次数。
    pub samples: u64,
    /// 步长总和（可用于核对增量链）。
    pub delta_sum: u64,
    /// 回退拦截次数。
    pub rewind_marks: u64,
    /// 读数来源。
    pub src: ClockSource,
    /// 已消费游标（供窗口层核对钟未被重复消费）。
    pub consumed_to: u64,
}
impl BenchClock {
    /// 以初值构造。
    pub fn new(now: u64, src: ClockSource) -> BenchClock {
        BenchClock {
            now,
            samples: 0,
            delta_sum: 0,
            rewind_marks: 0,
            src,
            consumed_to: now,
        }
    }
    /// 推进到 `t`（单调契约核心）。
    pub fn advance_to(&mut self, t: u64) -> TwResult<u64> {
        if t < self.now {
            self.rewind_marks = self.rewind_marks.wrapping_add(1);
            return Err(TwErr::of_form(
                Form::BaseRewind,
                format!("基准钟回退：t={} now={}", t, self.now),
            ));
        }
        self.delta_sum = self.delta_sum.wrapping_add(t - self.now);
        self.now = t;
        self.samples = self.samples.wrapping_add(1);
        Ok(self.now)
    }
    /// 推进一个固定步长。
    pub fn advance(&mut self, step: u64) -> TwResult<u64> {
        let t = self.now.wrapping_add(step);
        self.advance_to(t)
    }
    /// 标记消费位置（窗口层每收一个样本调用一次）。
    pub fn consume(&mut self, t: u64) -> TwResult<()> {
        if t < self.consumed_to {
            return Err(TwErr::of_aux(
                AuxCode::CursorSkew,
                format!("消费游标回退：t={} consumed={}", t, self.consumed_to),
            ));
        }
        self.consumed_to = t;
        Ok(())
    }
    /// 单调性是否成立。
    pub fn monotonic(&self) -> bool {
        if self.rewind_marks != 0 {
            return false;
        }
        if self.samples == 0 {
            return true;
        }
        self.now >= self.delta_sum
    }
    /// 指纹（只折来源与契约字段）。
    pub fn fingerprint(&self) -> u64 {
        let w: [u64; 4] = [
            V_VERSION_ID,
            self.src.name().len() as u64,
            self.samples,
            self.rewind_marks,
        ];
        fnv1a_words(&w)
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "clock now={} samples={} rewind={} delta_sum={} src={}",
            self.now, self.samples, self.rewind_marks, self.delta_sum, self.src.name()
        )
    }
}

// ===========================================================================
// 5. 核心结构二：窗口级别与规格（WindowSpec）
// ===========================================================================

/// 窗口级别（微/秒/分）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    /// 微窗。
    Micro,
    /// 秒窗。
    Sec,
    /// 分窗。
    Min,
}
impl Level {
    /// 级别总数。
    pub const COUNT: usize = 3;
    /// 全部级别（由细到粗，索引即层级）。
    pub const ALL: [Level; Level::COUNT] = [Level::Micro, Level::Sec, Level::Min];
    /// 层级下标。
    pub fn index(self) -> usize {
        match self {
            Level::Micro => 0,
            Level::Sec => 1,
            Level::Min => 2,
        }
    }
    /// 按下标取级别。
    pub fn from_index(i: usize) -> Option<Level> {
        if i < Level::COUNT {
            Some(Level::ALL[i])
        } else {
            None
        }
    }
    /// 级别名。
    pub fn name(self) -> &'static str {
        match self {
            Level::Micro => "Micro",
            Level::Sec => "Sec",
            Level::Min => "Min",
        }
    }
    /// 中文名。
    pub fn cn(self) -> &'static str {
        match self {
            Level::Micro => "微窗",
            Level::Sec => "秒窗",
            Level::Min => "分窗",
        }
    }
    /// 窗口长度（tick）。
    pub fn len_ticks(self) -> u64 {
        match self {
            Level::Micro => W_MICRO,
            Level::Sec => W_SEC,
            Level::Min => W_MIN,
        }
    }
    /// 相邻层级。
    pub fn next(self) -> Option<Level> {
        Level::from_index(self.index() + 1)
    }
    /// 上一层级。
    pub fn prev(self) -> Option<Level> {
        if self.index() == 0 {
            None
        } else {
            Level::from_index(self.index() - 1)
        }
    }
    /// 该级每窗包含的细一级窗数。
    pub fn sub_windows(self) -> u64 {
        match self {
            Level::Micro => 1,
            Level::Sec => NEST_RATIO,
            Level::Min => NEST_RATIO * NEST_RATIO,
        }
    }
}
/// 窗口边界规则（左闭右开）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BoundaryRule {
    /// 左闭右开 `[id*len, (id+1)*len)`：右端点归下一窗（标准）。
    HalfOpen,
    /// 左闭右闭 `[id*len, (id+1)*len]`：右端点归本窗（错误语义，仅注入用）。
    HalfClosed,
}
impl BoundaryRule {
    /// 规则名。
    pub fn name(self) -> &'static str {
        match self {
            BoundaryRule::HalfOpen => "HalfOpen",
            BoundaryRule::HalfClosed => "HalfClosed",
        }
    }
    /// 描述。
    pub fn describe(self) -> &'static str {
        match self {
            BoundaryRule::HalfOpen => "左闭右开：端点属于下一窗口（标准语义）",
            BoundaryRule::HalfClosed => "左闭右闭：端点属于本窗口（边界错归）",
        }
    }
    /// 判定某 tick 归属的窗口号。
    pub fn window_of(self, tick: u64, len: u64) -> u64 {
        if len == 0 {
            0
        } else if self == BoundaryRule::HalfOpen {
            div_or_zero(tick, len)
        } else {
            let r = div_or_zero(tick, len);
            let rem = tick % len;
            if rem == 0 && r > 0 {
                r.wrapping_sub(1)
            } else {
                r
            }
        }
    }
}
/// 窗口规格（一级一规格的冻结定义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WindowSpec {
    /// 级别。
    pub level: Level,
    /// 窗口长度（tick）。
    pub len: u64,
    /// 边界规则。
    pub rule: BoundaryRule,
    /// 是否启用滚动。
    pub rolling: bool,
}
impl WindowSpec {
    /// 构造规格。
    pub fn new(level: Level, len: u64, rule: BoundaryRule, rolling: bool) -> WindowSpec {
        WindowSpec { level, len, rule, rolling }
    }
    /// 指纹词（冻结件只折规格，不折游标）。
    pub fn words(&self) -> [u64; 4] {
        [
            self.level.index() as u64,
            self.len,
            self.rule.name().len() as u64,
            if self.rolling { 1 } else { 0 },
        ]
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "{:<6} len={:<5} rule={:<12} rolling={}",
            self.level.cn(),
            self.len,
            self.rule.name(),
            if self.rolling { 1 } else { 0 }
        )
    }
}
/// 默认规格（净态定义件：微 16 / 秒 256 / 分 4096，左闭右开，全滚动）。
pub fn default_specs() -> [WindowSpec; LEVEL_COUNT] {
    [
        WindowSpec::new(Level::Micro, W_MICRO, BoundaryRule::HalfOpen, true),
        WindowSpec::new(Level::Sec, W_SEC, BoundaryRule::HalfOpen, true),
        WindowSpec::new(Level::Min, W_MIN, BoundaryRule::HalfOpen, true),
    ]
}
/// 规格册（三级窗口定义 + 嵌套校验 + 重号位图）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SpecBook {
    /// 规格表（按级别下标索引）。
    pub specs: [WindowSpec; LEVEL_COUNT],
    /// 级别存在位图（bit i = 级别 i 已登记）。
    pub level_bits: u8,
    /// 窗口号重号位图（对每级独立检查，取并集）。
    pub dup_bits: u8,
    /// 嵌套校验结果。
    pub nested_ok: bool,
    /// 边界规则统一位（true = 全级同一规则）。
    pub rule_uniform: bool,
}
impl SpecBook {
    /// 由规格表构造并自检。
    pub fn build(specs: [WindowSpec; LEVEL_COUNT]) -> TwResult<SpecBook> {
        let mut level_bits: u8 = 0;
        let mut dup_bits: u8 = 0;
        let mut i: usize = 0;
        while i < LEVEL_COUNT {
            let sp = specs[i];
            level_bits = level_bits | (1u8 << sp.level.index());
            // 重号检查：同一级别不得出现两条规格
            let mut j: usize = i + 1;
            while j < LEVEL_COUNT {
                if specs[j].level == sp.level {
                    dup_bits = dup_bits | (1u8 << sp.level.index());
                }
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        // 嵌套校验：len[k+1] == len[k] * NEST_RATIO
        let mut nested_ok: bool = true;
        let mut k: usize = 0;
        while k + 1 < LEVEL_COUNT {
            let a = specs[k].len;
            let b = specs[k + 1].len;
            if b != a.wrapping_mul(NEST_RATIO) {
                nested_ok = false;
            }
            k = k.wrapping_add(1);
        }
        let mut rule_uniform: bool = true;
        let mut m: usize = 1;
        while m < LEVEL_COUNT {
            if specs[m].rule != specs[0].rule {
                rule_uniform = false;
            }
            m = m.wrapping_add(1);
        }
        Ok(SpecBook {
            specs,
            level_bits,
            dup_bits,
            nested_ok,
            rule_uniform,
        })
    }
    /// 净态规格册。
    pub fn net() -> TwResult<SpecBook> {
        SpecBook::build(default_specs())
    }
    /// 取某级规格。
    pub fn spec_at(&self, l: Level) -> TwResult<WindowSpec> {
        let i = l.index();
        if i >= LEVEL_COUNT {
            return Err(err_range("spec_at", i, LEVEL_COUNT));
        }
        Ok(self.specs[i])
    }
    /// 三级是否齐备。
    pub fn complete(&self) -> bool {
        let want: u8 = (1u8 << LEVEL_COUNT) - 1u8;
        self.level_bits == want
    }
    /// 无重号。
    pub fn dup_free(&self) -> bool {
        self.dup_bits == 0
    }
    /// 指纹（冻结件：只折规格定义）。
    pub fn fingerprint(&self) -> u64 {
        let mut w: Vec<u64> = Vec::with_capacity(2 + LEVEL_COUNT * 4);
        w.push(V_VERSION_ID);
        w.push(self.level_bits as u64);
        let mut i: usize = 0;
        while i < LEVEL_COUNT {
            let ww = self.specs[i].words();
            let mut j: usize = 0;
            while j < 4 {
                w.push(ww[j]);
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        fnv1a_words(&w)
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "spec levels={:08b} dup={:08b} nested={} uniform={} fp={:016X}",
            self.level_bits,
            self.dup_bits,
            if self.nested_ok { 1 } else { 0 },
            if self.rule_uniform { 1 } else { 0 },
            self.fingerprint()
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(LEVEL_COUNT + 2);
        out.push(self.summary());
        let mut i: usize = 0;
        while i < LEVEL_COUNT {
            out.push(format!("  spec {}", self.specs[i].describe()));
            i = i.wrapping_add(1);
        }
        out
    }
}

// ===========================================================================
// 6. 核心结构三：窗口册（WindowBank）
// ===========================================================================

/// 单窗计数格。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    /// 窗口号。
    pub id: u64,
    /// 事件计数（饱和于 `CELL_CAP`）。
    pub count: u32,
    /// 饱和标志（1 = 已触顶）。
    pub sat: u8,
    /// 有效位（1 = 已真实写入过）。
    pub used: u8,
}
impl Cell {
    /// 空格。
    pub fn empty() -> Cell {
        Cell { id: 0, count: 0, sat: 0, used: 0 }
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("win#{} count={} sat={} used={}", self.id, self.count, self.sat, self.used)
    }
}
/// 单级窗口册（环形保存 `BANK_CAP` 个窗口的计数格）。
///
/// 滚动语义：游标从 `cursor` 单调前进；一次前进超过一格即记 `roll_skips`（跳窗显性化）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WindowBank {
    /// 所属级别。
    pub level: Level,
    /// 窗口长度。
    pub len: u64,
    /// 边界规则。
    pub rule: BoundaryRule,
    /// 环形格。
    pub cells: Vec<Cell>,
    /// 当前游标窗口号。
    pub cursor: u64,
    /// 当前格下标。
    pub cur_slot: usize,
    /// 已滚动步数（正常单格步）。
    pub roll_steps: u64,
    /// 跳窗次数。
    pub roll_skips: u64,
    /// 被覆盖丢弃的格数。
    pub dropped: u64,
    /// 饱和标志出现次数。
    pub sat_marks: u64,
    /// 归属校验失败计数。
    pub attr_faults: u64,
}
impl WindowBank {
    /// 构造空册。
    pub fn new(level: Level, spec: WindowSpec) -> WindowBank {
        let mut cells: Vec<Cell> = Vec::with_capacity(BANK_CAP);
        let mut i: usize = 0;
        while i < BANK_CAP {
            cells.push(Cell::empty());
            i = i.wrapping_add(1);
        }
        WindowBank {
            level,
            len: spec.len,
            rule: spec.rule,
            cells,
            cursor: 0,
            cur_slot: 0,
            roll_steps: 0,
            roll_skips: 0,
            dropped: 0,
            sat_marks: 0,
            attr_faults: 0,
        }
    }
    /// 该 tick 应归属的窗口号。
    pub fn window_of(&self, tick: u64) -> u64 {
        self.rule.window_of(tick, self.len)
    }
    /// 滚动到 `target` 窗（返回实际滚动步数；`0` 表示已在该窗）。
    pub fn roll_to(&mut self, target: u64) -> TwResult<u64> {
        if target < self.cursor {
            return Err(TwErr::of_form(
                Form::BaseRewind,
                format!("{} 窗口游标回退：target={} cursor={}", self.level.name(), target, self.cursor),
            ));
        }
        let steps: u64 = target - self.cursor;
        if steps == 0 {
            return Ok(0);
        }
        if steps > 1 {
            self.roll_skips = self.roll_skips.wrapping_add(steps - 1);
            self.dropped = self.dropped.wrapping_add(steps - 1);
        }
        self.cursor = target;
        self.cur_slot = self.cur_slot.wrapping_add(steps as usize) & BANK_MASK;
        self.roll_steps = self.roll_steps.wrapping_add(steps);
        Ok(steps)
    }
    /// 计数 +1（饱和显性）。
    pub fn bump(&mut self) -> TwResult<()> {
        let slot = self.cur_slot;
        if let Some(c) = self.cells.get_mut(slot) {
            if c.sat == 1 {
                return Err(TwErr::of_aux(
                    AuxCode::SatAfterFlag,
                    format!("{} 窗口#{} 已饱和仍累加", self.level.name(), self.cursor),
                ));
            }
            let before = c.count;
            let after = sat_add_u32(before, 1, CELL_CAP);
            c.count = after;
            c.id = self.cursor;
            c.used = 1;
            if after >= CELL_CAP {
                c.sat = 1;
                self.sat_marks = self.sat_marks.wrapping_add(1);
            }
            return Ok(());
        }
        Err(err_range("bump", slot, self.cells.len()))
    }
    /// 记一次归属校验失败。
    pub fn mark_attr_fault(&mut self) {
        self.attr_faults = self.attr_faults.wrapping_add(1);
    }
    /// 取某格。
    pub fn cell_at(&self, slot: usize) -> TwResult<Cell> {
        match self.cells.get(slot) {
            Some(c) => Ok(*c),
            None => Err(err_range("cell_at", slot, self.cells.len())),
        }
    }
    /// 存活格计数和。
    pub fn sum_live(&self) -> u64 {
        let mut s: u64 = 0;
        let mut i: usize = 0;
        while i < self.cells.len() {
            let c = self.cells[i];
            if c.used == 1 {
                s = s.wrapping_add(c.count as u64);
            }
            i = i.wrapping_add(1);
        }
        s
    }
    /// 存活格数。
    pub fn live_cells(&self) -> u64 {
        let mut s: u64 = 0;
        let mut i: usize = 0;
        while i < self.cells.len() {
            if self.cells[i].used == 1 {
                s = s.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        s
    }
    /// 已用格不得为空窗（有窗号必有事件）。
    pub fn cover_ok(&self) -> bool {
        let mut i: usize = 0;
        while i < self.cells.len() {
            let c = self.cells[i];
            if c.used == 1 && c.count == 0 {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 饱和标志与计数一致性。
    pub fn sat_consistent(&self) -> bool {
        let mut i: usize = 0;
        while i < self.cells.len() {
            let c = self.cells[i];
            if c.sat == 1 && c.count < CELL_CAP {
                return false;
            }
            if c.sat == 0 && c.count >= CELL_CAP {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 指纹（只折定义字段，**不折游标与计数**）。
    pub fn fingerprint(&self) -> u64 {
        let w: [u64; 4] = [
            V_VERSION_ID,
            self.level.index() as u64,
            self.len,
            self.rule.name().len() as u64,
        ];
        fnv1a_words(&w)
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "{:<6} sum_live={} live={} cursor={} roll={} skips={} drop={} sat={} attr_err={}",
            self.level.cn(),
            self.sum_live(),
            self.live_cells(),
            self.cursor,
            self.roll_steps,
            self.roll_skips,
            self.dropped,
            self.sat_marks,
            self.attr_faults
        )
    }
}

// ===========================================================================
// 7. 核心结构四：延迟直方图与整数分位（Histogram）
// ===========================================================================

/// 延迟直方图（`HIST_BUCKETS` 桶，桶宽 `LAT_UNIT` tick）。
///
/// 分位全整数实现：`target = ceil(n * pct / 10000)`，定位累计桶，
/// 桶内按整数线性插值，并用 `isqrt` 给出均方根——**零浮点**。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Histogram {
    /// 桶计数。
    pub buckets: [u32; HIST_BUCKETS],
    /// 样本总数。
    pub samples: u32,
    /// 延迟平方和（用于 rms）。
    pub sq_sum: u64,
    /// 延迟总和。
    pub sum: u64,
    /// 最大延迟（tick）。
    pub max: u64,
    /// 最小延迟（tick）。
    pub min: u64,
}
impl Histogram {
    /// 空直方图。
    pub fn new() -> Histogram {
        Histogram {
            buckets: [0u32; HIST_BUCKETS],
            samples: 0,
            sq_sum: 0,
            sum: 0,
            max: 0,
            min: u64::MAX,
        }
    }
    /// 记一个延迟样本。
    pub fn record(&mut self, lat: u64) -> TwResult<()> {
        let b: usize = (lat / LAT_UNIT) as usize;
        if b >= HIST_BUCKETS {
            return Err(TwErr::of_aux(
                AuxCode::BucketOver,
                format!("桶越界：lat={} b={} cap={}", lat, b, HIST_BUCKETS),
            ));
        }
        let cur = self.buckets[b];
        self.buckets[b] = cur.wrapping_add(1);
        self.samples = self.samples.wrapping_add(1);
        self.sq_sum = self.sq_sum.wrapping_add(lat.saturating_mul(lat));
        self.sum = self.sum.wrapping_add(lat);
        if lat > self.max {
            self.max = lat;
        }
        if lat < self.min {
            self.min = lat;
        }
        Ok(())
    }
    /// 桶和（自洽性核对用）。
    pub fn bucket_sum(&self) -> u64 {
        let mut s: u64 = 0;
        let mut i: usize = 0;
        while i < HIST_BUCKETS {
            s = s.wrapping_add(self.buckets[i] as u64);
            i = i.wrapping_add(1);
        }
        s
    }
    /// 平均延迟（整数微刻度，×1000）。
    pub fn mean_scaled(&self) -> u64 {
        if self.samples == 0 {
            return 0;
        }
        let n = self.samples as u64;
        div_or_zero(self.sum.saturating_mul(1000), n)
    }
    /// 均方根（整数 `isqrt`）。
    pub fn rms(&self) -> u64 {
        if self.samples == 0 {
            return 0;
        }
        let n = self.samples as u64;
        isqrt_u64(div_or_zero(self.sq_sum, n))
    }
    /// 分位数（`pct` 为放大 100 倍的值，如 `9900` 表 99%）。
    pub fn percentile(&self, pct: u64) -> TwResult<u64> {
        if self.samples == 0 {
            return Err(TwErr::of_aux(AuxCode::ZeroSample, String::from("分位计算面对零样本")));
        }
        let n = self.samples as u64;
        // target = ceil(n * pct / 10000)
        let target: u64 = div_or_zero(n.saturating_mul(pct) + 9999, 10000);
        if target == 0 || target > n {
            return Err(TwErr::of_form(
                Form::PctRankBad,
                format!("分位排名越界：target={} n={} pct={}", target, n, pct),
            ));
        }
        let mut cum: u64 = 0;
        let mut i: usize = 0;
        while i < HIST_BUCKETS {
            let c = self.buckets[i] as u64;
            if cum.saturating_add(c) >= target {
                let before = cum;
                let in_bucket: u64 = target - before;
                if c == 0 {
                    return Ok((i as u64) * LAT_UNIT);
                }
                let frac: u64 = div_or_zero(in_bucket.saturating_mul(LAT_UNIT), c);
                return Ok((i as u64).saturating_mul(LAT_UNIT).wrapping_add(frac));
            }
            cum = cum.wrapping_add(c);
            i = i.wrapping_add(1);
        }
        Ok(((HIST_BUCKETS - 1) as u64) * LAT_UNIT)
    }
    /// 样本数是否达判定基准。
    pub fn enough(&self) -> bool {
        self.samples >= MIN_SAMPLES
    }
    /// 分位全表。
    pub fn pct_table(&self) -> TwResult<Vec<u64>> {
        let mut out: Vec<u64> = Vec::with_capacity(PCT_COUNT);
        let mut i: usize = 0;
        while i < PCT_COUNT {
            out.push(self.percentile(PCT_SCALED[i])?);
            i = i.wrapping_add(1);
        }
        Ok(out)
    }
    /// 指纹（只折桶定义 + 样本数）。
    pub fn fingerprint(&self) -> u64 {
        let w: [u64; 5] = [
            V_VERSION_ID,
            HIST_BUCKETS as u64,
            LAT_UNIT,
            self.samples as u64,
            div_or_zero(self.sum, self.samples as u64),
        ];
        fnv1a_words(&w)
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "hist n={} bucket_sum={} sum={} mean={} rms={} max={} min={}",
            self.samples,
            self.bucket_sum(),
            self.sum,
            self.mean_scaled(),
            self.rms(),
            self.max,
            if self.min == u64::MAX { 0 } else { self.min }
        )
    }
}

// ===========================================================================
// 8. 主状态机（合法边闭集 + 非法转换拒记）
// ===========================================================================

/// 时间窗建模阶段（锚点「基准钟/窗口长度/三级窗口」三段式）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// 阶段一：基准钟立基准（选源、定初值、验证单调）。
    P1Clock,
    /// 阶段二：窗口长度与三级嵌套定义。
    P2Spec,
    /// 阶段三：边界行为点验（M-120）。
    P3Boundary,
    /// 阶段四：事件入窗与滚动。
    P4Ingest,
    /// 阶段五：分位统计。
    P5Percentile,
    /// 终态：模型冻结入册。
    Sealed,
}
impl Phase {
    /// 阶段数（含终态）。
    pub const COUNT: usize = 6;
    /// 全部阶段。
    pub const ALL: [Phase; Phase::COUNT] = [
        Phase::P1Clock,
        Phase::P2Spec,
        Phase::P3Boundary,
        Phase::P4Ingest,
        Phase::P5Percentile,
        Phase::Sealed,
    ];
    /// 阶段下标。
    pub fn index(self) -> usize {
        match self {
            Phase::P1Clock => 0,
            Phase::P2Spec => 1,
            Phase::P3Boundary => 2,
            Phase::P4Ingest => 3,
            Phase::P5Percentile => 4,
            Phase::Sealed => 5,
        }
    }
    /// 按下标取阶段。
    pub fn from_index(i: usize) -> Option<Phase> {
        if i < Phase::COUNT {
            Some(Phase::ALL[i])
        } else {
            None
        }
    }
    /// 阶段名。
    pub fn name(self) -> &'static str {
        match self {
            Phase::P1Clock => "P1Clock",
            Phase::P2Spec => "P2Spec",
            Phase::P3Boundary => "P3Boundary",
            Phase::P4Ingest => "P4Ingest",
            Phase::P5Percentile => "P5Percentile",
            Phase::Sealed => "Sealed",
        }
    }
    /// 中文名。
    pub fn cn(self) -> &'static str {
        match self {
            Phase::P1Clock => "基准钟立基准",
            Phase::P2Spec => "窗口长度与三级嵌套定义",
            Phase::P3Boundary => "边界行为点验",
            Phase::P4Ingest => "事件入窗与滚动",
            Phase::P5Percentile => "分位统计",
            Phase::Sealed => "模型冻结入册",
        }
    }
    /// 是否终态。
    pub fn is_sealed(self) -> bool {
        self == Phase::Sealed
    }
    /// 四列输出（阶段名/中文/下一步/判据面）。
    pub fn four_columns(&self) -> String {
        let nxt: &'static str = match self {
            Phase::P1Clock => "P2Spec",
            Phase::P2Spec => "P3Boundary",
            Phase::P3Boundary => "P4Ingest",
            Phase::P4Ingest => "P5Percentile",
            Phase::P5Percentile => "Sealed",
            Phase::Sealed => "-",
        };
        format!("{:<14} {:<16} next={:<14} {}", self.name(), self.cn(), nxt, FACE_WINDOW)
    }
}

/// 非法转换留痕（append-only 语义，不覆盖）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reject {
    /// 序号。
    pub seq: u32,
    /// 起始阶段。
    pub from: Phase,
    /// 目标阶段。
    pub to: Phase,
    /// 逻辑时刻。
    pub tick: u64,
    /// 拒绝理由码（诊断码）。
    pub code: u16,
}
impl Reject {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "reject#{} {}->{} tick={} code=0x{:04X}",
            self.seq,
            self.from.name(),
            self.to.name(),
            self.tick,
            self.code
        )
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 4] {
        [self.from.index() as u64, self.to.index() as u64, self.tick, self.code as u64]
    }
}
/// 主状态机。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PhaseFsm {
    /// 当前阶段。
    pub state: Phase,
    /// 逻辑时刻。
    pub tick: u64,
    /// 合法边推进计数。
    pub legal_moves: u64,
    /// 非法转换拒记。
    pub rejected: Vec<Reject>,
    /// 阶段足迹（单调）。
    pub trail: Vec<Phase>,
}
impl PhaseFsm {
    /// 初始态。
    pub fn new() -> PhaseFsm {
        let mut trail: Vec<Phase> = Vec::with_capacity(Phase::COUNT);
        trail.push(Phase::P1Clock);
        PhaseFsm { state: Phase::P1Clock, tick: 0, legal_moves: 0, rejected: Vec::new(), trail }
    }
    /// 合法边闭集（只允许「下一阶段」；其余全为非法）。
    pub fn is_legal(from: Phase, to: Phase) -> bool {
        match (from, to) {
            (Phase::P1Clock, Phase::P2Spec) => true,
            (Phase::P2Spec, Phase::P3Boundary) => true,
            (Phase::P3Boundary, Phase::P4Ingest) => true,
            (Phase::P4Ingest, Phase::P5Percentile) => true,
            (Phase::P5Percentile, Phase::Sealed) => true,
            _ => false,
        }
    }
    /// 合法边总数（点验用）。
    pub fn edge_count() -> usize {
        5
    }
    /// 非法候选边总数（全序对数减合法边数）。
    pub fn illegal_edge_count() -> usize {
        Phase::COUNT * Phase::COUNT - PhaseFsm::edge_count()
    }
    /// 试走一条边（非法则拒记，不改状态）。
    pub fn try_go(&mut self, to: Phase, tick: u64) -> TwResult<Phase> {
        self.tick = tick;
        if self.state.is_sealed() {
            let r = Reject {
                seq: self.rejected.len() as u32,
                from: self.state,
                to,
                tick,
                code: Form::BaseRewind.code(),
            };
            self.rejected.push(r);
            return Err(TwErr::of_form(
                Form::BaseRewind,
                format!("终态不可再迁移：{}->{}", self.state.name(), to.name()),
            ));
        }
        if PhaseFsm::is_legal(self.state, to) {
            self.state = to;
            self.legal_moves = self.legal_moves.wrapping_add(1);
            self.trail.push(to);
            return Ok(to);
        }
        let r = Reject {
            seq: self.rejected.len() as u32,
            from: self.state,
            to,
            tick,
            code: Form::CrossWindowMisattr.code(),
        };
        self.rejected.push(r);
        Err(TwErr::of_form(
            Form::CrossWindowMisattr,
            format!("非法转换：{}->{}", self.state.name(), to.name()),
        ))
    }
    /// 走满全部合法边。
    pub fn drive_all(&mut self, start_tick: u64) -> TwResult<()> {
        let mut t = start_tick;
        let mut i: usize = 1;
        while i < Phase::COUNT {
            let p = Phase::ALL[i];
            t = t.wrapping_add(1);
            self.try_go(p, t)?;
            i = i.wrapping_add(1);
        }
        Ok(())
    }
    /// 足迹单调（长度等于阶段数且下标严格递增）。
    pub fn trail_monotonic(&self) -> bool {
        let mut i: usize = 1;
        while i < self.trail.len() {
            if self.trail[i - 1].index() >= self.trail[i].index() {
                return false;
            }
            i = i.wrapping_add(1);
        }
        self.trail.len() == Phase::COUNT
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "fsm state={} legal={} reject={} trail={}",
            self.state.name(),
            self.legal_moves,
            self.rejected.len(),
            self.trail.len()
        )
    }
}

// ===========================================================================
// 9. 边界行为点验（M-120 落点）
// ===========================================================================

/// 边界点验用例。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BoundaryCase {
    /// 用例名。
    pub name: &'static str,
    /// 相对窗起点的偏移（tick）。
    pub offset: u64,
    /// 期望归属窗号（相对窗起点窗为 0）。
    pub expect_win: u64,
    /// 期望是否落在「窗口右端点」。
    pub expect_endpoint: bool,
}
impl BoundaryCase {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "case {:<16} off={:<4} expect_win={} endpoint={}",
            self.name, self.offset, self.expect_win, if self.expect_endpoint { 1 } else { 0 }
        )
    }
}
/// 边界点验表（`BOUNDARY_CASES` 条）。
///
/// 覆盖：窗首、窗中、窗尾前一 tick、**正窗尾（关键）**、跨两窗、
/// 零偏移、极大偏移、单窗全宽等。
pub fn boundary_cases() -> [BoundaryCase; BOUNDARY_CASES] {
    [
        BoundaryCase { name: "zero", offset: 0, expect_win: 0, expect_endpoint: false },
        BoundaryCase { name: "mid", offset: 8, expect_win: 0, expect_endpoint: false },
        BoundaryCase { name: "last_in", offset: 15, expect_win: 0, expect_endpoint: false },
        BoundaryCase { name: "right_end", offset: 16, expect_win: 1, expect_endpoint: true },
        BoundaryCase { name: "next_mid", offset: 24, expect_win: 1, expect_endpoint: false },
        BoundaryCase { name: "two_win", offset: 32, expect_win: 2, expect_endpoint: true },
        BoundaryCase { name: "off_1", offset: 1, expect_win: 0, expect_endpoint: false },
        BoundaryCase { name: "off_15", offset: 15, expect_win: 0, expect_endpoint: false },
        BoundaryCase { name: "off_17", offset: 17, expect_win: 1, expect_endpoint: false },
        BoundaryCase { name: "off_31", offset: 31, expect_win: 1, expect_endpoint: false },
        BoundaryCase { name: "off_33", offset: 33, expect_win: 2, expect_endpoint: false },
        BoundaryCase { name: "far", offset: 4095, expect_win: 255, expect_endpoint: false },
    ]
}
/// 单条边界点验执行结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BoundaryResult {
    /// 用例下标。
    pub idx: usize,
    /// 规则实际给出的窗号。
    pub got_win: u64,
    /// 期望窗号。
    pub want_win: u64,
    /// 是否通过。
    pub ok: bool,
}
impl BoundaryResult {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "bc#{} got={} want={} ok={}",
            self.idx, self.got_win, self.want_win, if self.ok { 1 } else { 0 }
        )
    }
}
/// 边界点验结果册。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BoundaryBook {
    /// 逐用例结果。
    pub rows: Vec<BoundaryResult>,
    /// 通过数。
    pub pass: u32,
    /// 失败数。
    pub fail: u32,
    /// 使用的规则名长度（供冻结折入）。
    pub rule_len: u64,
}
impl BoundaryBook {
    /// 对某规则跑全量点验。
    pub fn probe(rule: BoundaryRule, len: u64) -> BoundaryBook {
        let cases = boundary_cases();
        let mut rows: Vec<BoundaryResult> = Vec::with_capacity(BOUNDARY_CASES);
        let mut pass: u32 = 0;
        let mut fail: u32 = 0;
        let mut i: usize = 0;
        while i < cases.len() {
            let c = cases[i];
            let got: u64 = rule.window_of(c.offset, len);
            let ok: bool = got == c.expect_win;
            if ok {
                pass = pass.wrapping_add(1);
            } else {
                fail = fail.wrapping_add(1);
            }
            rows.push(BoundaryResult { idx: i, got_win: got, want_win: c.expect_win, ok });
            i = i.wrapping_add(1);
        }
        BoundaryBook { rows, pass, fail, rule_len: rule.name().len() as u64 }
    }
    /// 全通过。
    pub fn all_ok(&self) -> bool {
        self.fail == 0 && self.pass == BOUNDARY_CASES as u32
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!("boundary pass={} fail={} of {}", self.pass, self.fail, BOUNDARY_CASES)
    }
    /// 逐行表。
    pub fn rows_out(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(BOUNDARY_CASES + 1);
        out.push(self.summary());
        let cases = boundary_cases();
        let mut i: usize = 0;
        while i < self.rows.len() {
            out.push(format!("  {}", self.rows[i].describe()));
            out.push(format!("      expect {}", cases[i].describe()));
            i = i.wrapping_add(1);
        }
        out
    }
}

// ===========================================================================
// 10. 事件流与入窗（EventStream）
// ===========================================================================

/// 事件种类（模拟被测系统在不同逻辑时刻产生的一次时间窗事件）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventKind {
    /// 采样事件。
    Sample,
    /// 滚动事件。
    Roll,
    /// 溢出探针事件。
    OverflowProbe,
    /// 乱序事件（故意越窗，仅用于注入）。
    Disorder,
}
impl EventKind {
    /// 种类总数。
    pub const COUNT: usize = 4;
    /// 全部种类。
    pub const ALL: [EventKind; EventKind::COUNT] = [
        EventKind::Sample,
        EventKind::Roll,
        EventKind::OverflowProbe,
        EventKind::Disorder,
    ];
    /// 下标。
    pub fn index(self) -> usize {
        match self {
            EventKind::Sample => 0,
            EventKind::Roll => 1,
            EventKind::OverflowProbe => 2,
            EventKind::Disorder => 3,
        }
    }
    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            EventKind::Sample => "Sample",
            EventKind::Roll => "Roll",
            EventKind::OverflowProbe => "OverflowProbe",
            EventKind::Disorder => "Disorder",
        }
    }
    /// 描述。
    pub fn describe(self) -> &'static str {
        match self {
            EventKind::Sample => "采样事件：正常入窗",
            EventKind::Roll => "滚动事件：推进窗口游标",
            EventKind::OverflowProbe => "溢出探针：连续灌入以触顶单窗计数",
            EventKind::Disorder => "乱序事件：时刻回退（仅注入用）",
        }
    }
}
/// 事件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Event {
    /// 逻辑时刻（tick）。
    pub tsc: u64,
    /// 种类。
    pub kind: EventKind,
    /// 携带延迟（tick），入直方图。
    pub latency: u64,
}
impl Event {
    /// 构造事件。
    pub fn new(tsc: u64, kind: EventKind, latency: u64) -> Event {
        Event { tsc, kind, latency }
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 3] {
        [self.tsc, self.kind.index() as u64, self.latency]
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("ev t={} kind={} lat={}", self.tsc, self.kind.name(), self.latency)
    }
}
/// 入窗统计。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IngestStat {
    /// 受理事件数。
    pub accepted: u64,
    /// 拒收事件数（回退 / 步数越界）。
    pub rejected: u64,
    /// 归属校验通过数。
    pub attr_ok: u64,
    /// 归属校验失败数。
    pub attr_bad: u64,
    /// 直方图收样数。
    pub hist_n: u32,
    /// 逻辑步数消耗。
    pub steps: u64,
}
impl IngestStat {
    /// 空统计。
    pub fn blank() -> IngestStat {
        IngestStat { accepted: 0, rejected: 0, attr_ok: 0, attr_bad: 0, hist_n: 0, steps: 0 }
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "ingest acc={} rej={} attr_ok={} attr_bad={} hist={} steps={}",
            self.accepted, self.rejected, self.attr_ok, self.attr_bad, self.hist_n, self.steps
        )
    }
    /// 指纹词（活账，仅摘要用）。
    pub fn words(&self) -> [u64; 6] {
        [self.accepted, self.rejected, self.attr_ok, self.attr_bad, self.hist_n as u64, self.steps]
    }
}
/// 事件流（三级窗口联合入窗器）。
///
/// 入窗顺序（关键）：先由基准钟推进，再对**三级窗口逐级滚动并计数**，
/// 每级都必须 `bump()` 一次——由此天然满足 `Σ微 = Σ秒 = Σ分 = 受理数` 的嵌套恒等式。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EventStream {
    /// 基准钟。
    pub clock: BenchClock,
    /// 三级窗口册。
    pub banks: Vec<WindowBank>,
    /// 直方图。
    pub hist: Histogram,
    /// 入窗统计。
    pub stat: IngestStat,
    /// 步数上限（构造期固化，防注入越界）。
    pub step_cap: u64,
    /// 当前 tick（用于 ClockSource::Given 对拍）。
    pub given: u64,
}
impl EventStream {
    /// 构造（三级规格）。
    pub fn new(specs: &SpecBook) -> TwResult<EventStream> {
        let mut banks: Vec<WindowBank> = Vec::with_capacity(LEVEL_COUNT);
        let mut i: usize = 0;
        while i < LEVEL_COUNT {
            let l = Level::ALL[i];
            let sp = specs.spec_at(l)?;
            banks.push(WindowBank::new(l, sp));
            i = i.wrapping_add(1);
        }
        Ok(EventStream {
            clock: BenchClock::new(0, ClockSource::LogicalStep),
            banks,
            hist: Histogram::new(),
            stat: IngestStat::blank(),
            step_cap: STEP_CAP,
            given: 0,
        })
    }
    /// 单级册索引取。
    pub fn bank_at(&self, l: Level) -> TwResult<&WindowBank> {
        let i = l.index();
        match self.banks.get(i) {
            Some(b) => Ok(b),
            None => Err(err_range("bank_at", i, self.banks.len())),
        }
    }
    /// 单级册索引取（可变）。
    pub fn bank_mut(&mut self, l: Level) -> TwResult<&mut WindowBank> {
        let i = l.index();
        match self.banks.get_mut(i) {
            Some(b) => Ok(b),
            None => Err(err_range("bank_mut", i, LEVEL_COUNT)),
        }
    }
    /// 三级计数和（各自分别求和）。
    pub fn level_sums(&self) -> [u64; LEVEL_COUNT] {
        let mut out: [u64; LEVEL_COUNT] = [0u64; LEVEL_COUNT];
        let mut i: usize = 0;
        while i < self.banks.len() {
            out[i] = self.banks[i].sum_live();
            i = i.wrapping_add(1);
        }
        out
    }
    /// 三级窗口号（应指向同一逻辑时刻）。
    pub fn cursors(&self) -> [u64; LEVEL_COUNT] {
        let mut out: [u64; LEVEL_COUNT] = [0u64; LEVEL_COUNT];
        let mut i: usize = 0;
        while i < self.banks.len() {
            out[i] = self.banks[i].cursor;
            i = i.wrapping_add(1);
        }
        out
    }
    /// 吞一个事件（返回是否受理）。
    pub fn feed(&mut self, ev: Event) -> TwResult<bool> {
        // 步数上限检查
        if self.stat.steps >= self.step_cap {
            self.stat.rejected = self.stat.rejected.wrapping_add(1);
            return Err(TwErr::of_aux(
                AuxCode::StepOverCap,
                format!("步数越界：steps={} cap={}", self.stat.steps, self.step_cap),
            ));
        }
        // ① 基准钟单调推进（回退即拒）
        if ev.tsc < self.clock.now {
            self.stat.rejected = self.stat.rejected.wrapping_add(1);
            self.clock.rewind_marks = self.clock.rewind_marks.wrapping_add(1);
            return Err(TwErr::of_form(
                Form::BaseRewind,
                format!("事件回退：ev={} now={}", ev.tsc, self.clock.now),
            ));
        }
        self.clock.advance_to(ev.tsc)?;
        self.clock.consume(ev.tsc)?;
        // ② 三级窗口逐级滚动 + 计数
        let mut attr_ok: bool = true;
        let mut i: usize = 0;
        while i < LEVEL_COUNT {
            let want: u64 = {
                let b = self.banks[i].rule.window_of(ev.tsc, self.banks[i].len);
                b
            };
            let cur: u64 = self.banks[i].cursor;
            if want < cur {
                self.banks[i].mark_attr_fault();
                attr_ok = false;
            } else {
                // 逐窗滚动：缺几格补几格（补格计入 dropped）
                // 逐窗补齐：每轮目标都取当前游标 +1，否则 roll_to 会拿到陈旧目标而空转
                while self.banks[i].cursor < want {
                    let next_cur: u64 = self.banks[i].cursor.wrapping_add(1);
                    let steps = self.banks[i].roll_to(next_cur)?;
                    let _ = steps;
                    self.stat.steps = self.stat.steps.wrapping_add(1);
                }
                self.banks[i].bump()?;
                self.stat.steps = self.stat.steps.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        if attr_ok {
            self.stat.attr_ok = self.stat.attr_ok.wrapping_add(1);
        } else {
            self.stat.attr_bad = self.stat.attr_bad.wrapping_add(1);
        }
        // ③ 直方图收样
        self.hist.record(ev.latency)?;
        self.stat.hist_n = self.stat.hist_n.wrapping_add(1);
        // ④ 统计
        self.stat.accepted = self.stat.accepted.wrapping_add(1);
        self.stat.steps = self.stat.steps.wrapping_add(1);
        self.given = ev.tsc;
        Ok(true)
    }
    /// 生成净态事件序列（确定性 LCG 驱动，步数受 `STREAM_STEPS` 约束）。
    pub fn gen_net(seed: u64, n: usize) -> TwResult<Vec<Event>> {
        let mut rng = Lcg::new(seed);
        let mut out: Vec<Event> = Vec::with_capacity(n);
        let mut t: u64 = 0;
        let mut i: usize = 0;
        while i < n {
            // 步长落在 [1, 2*W_MICRO)，保证既有窗内也有跨窗
            let step: u64 = 1u64.wrapping_add(rng.below(W_MICRO * 2));
            t = t.wrapping_add(step);
            let lat: u64 = rng.below((HIST_BUCKETS as u64) * LAT_UNIT);
            let kind: EventKind = if i % 8 == 7 {
                EventKind::Roll
            } else {
                EventKind::Sample
            };
            out.push(Event::new(t, kind, lat));
            i = i.wrapping_add(1);
        }
        Ok(out)
    }
    /// 跑完整流。
    pub fn run(&mut self, evs: &[Event]) -> TwResult<()> {
        let mut i: usize = 0;
        while i < evs.len() {
            let _ = self.feed(*evs.get(i).unwrap_or(&Event::new(0, EventKind::Sample, 0)));
            i = i.wrapping_add(1);
        }
        Ok(())
    }
    /// 嵌套恒等是否成立。
    pub fn nested_ok(&self) -> bool {
        let s = self.level_sums();
        s[0] == s[1] && s[1] == s[2]
    }
    /// 游标是否同步（各级 `cursor*len` 指向同一逻辑时刻的同一窗边界）。
    pub fn cursor_sync(&self) -> bool {
        let mut i: usize = 0;
        while i < LEVEL_COUNT {
            let b = &self.banks[i];
            let left: u64 = b.cursor.wrapping_mul(b.len);
            let right: u64 = b.cursor.wrapping_add(1).wrapping_mul(b.len);
            if left < right {
                // 恒真，仅为可读性保留：窗口区间非退化
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        let s = self.level_sums();
        format!(
            "stream Σ微={} Σ秒={} Σ分={} nested={} {} | {}",
            s[0], s[1], s[2], if self.nested_ok() { 1 } else { 0 },
            self.stat.summary(), self.hist.summary()
        )
    }
}

// ===========================================================================
// 11. append-only 环形账（dropped 显性）
// ===========================================================================

/// 环形账条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RingEntry {
    /// 序号。
    pub seq: u64,
    /// 逻辑时刻。
    pub tick: u64,
    /// 事件种类下标。
    pub kind: u8,
    /// 归属窗口号（微窗）。
    pub win: u64,
    /// 事件指纹。
    pub ev_fp: u64,
}
impl RingEntry {
    /// 描述。
    pub fn describe(&self) -> String {
        format!("ring#{} t={} kind={} win={} fp={:016X}", self.seq, self.tick, self.kind, self.win, self.ev_fp)
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 5] {
        [self.seq, self.tick, self.kind as u64, self.win, self.ev_fp]
    }
}
/// append-only 环形账。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RingLedger {
    /// 缓冲。
    pub buf: Vec<RingEntry>,
    /// 写指针。
    pub head: usize,
    /// 累计写入。
    pub pushed: u64,
    /// 累计丢弃（被覆盖的旧条目，**显性**）。
    pub dropped: u64,
}
impl RingLedger {
    /// 构造空账。
    pub fn new() -> RingLedger {
        let mut buf: Vec<RingEntry> = Vec::with_capacity(RING_CAP);
        let mut i: usize = 0;
        while i < RING_CAP {
            buf.push(RingEntry { seq: 0, tick: 0, kind: 0xFF, win: 0, ev_fp: 0 });
            i = i.wrapping_add(1);
        }
        RingLedger { buf, head: 0, pushed: 0, dropped: 0 }
    }
    /// 写一条（覆盖最旧时累加 `dropped`）。
    pub fn push(&mut self, e: RingEntry) {
        let slot = self.head;
        if let Some(old) = self.buf.get(slot) {
            if old.kind != 0xFF || old.ev_fp != 0 {
                self.dropped = self.dropped.wrapping_add(1);
            }
        }
        let mut ne = e;
        ne.seq = self.pushed;
        if let Some(c) = self.buf.get_mut(slot) {
            *c = ne;
        }
        self.head = self.head.wrapping_add(1) & RING_MASK;
        self.pushed = self.pushed.wrapping_add(1);
    }
    /// 存活条目数。
    pub fn live(&self) -> u64 {
        let mut n: u64 = 0;
        let mut i: usize = 0;
        while i < self.buf.len() {
            if self.buf[i].kind != 0xFF || self.buf[i].ev_fp != 0 {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }
    /// 守恒：`pushed == live + dropped`。
    pub fn balanced(&self) -> bool {
        self.pushed == self.live().wrapping_add(self.dropped)
    }
    /// 指纹（活账——**绝不进冻结**）。
    pub fn fingerprint(&self) -> u64 {
        let mut w: Vec<u64> = Vec::with_capacity(3 + RING_CAP * 5);
        w.push(V_VERSION_ID);
        w.push(self.pushed);
        w.push(self.dropped);
        let mut i: usize = 0;
        while i < self.buf.len() {
            let ww = self.buf[i].words();
            let mut j: usize = 0;
            while j < 5 {
                w.push(ww[j]);
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        fnv1a_words(&w)
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "ring pushed={} live={} dropped={} balanced={} cap={}",
            self.pushed,
            self.live(),
            self.dropped,
            if self.balanced() { 1 } else { 0 },
            RING_CAP
        )
    }
    /// 逐行表。
    pub fn rows_out(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(RING_CAP + 1);
        out.push(self.summary());
        let mut i: usize = 0;
        while i < self.buf.len() {
            let e = self.buf[i];
            if e.kind != 0xFF || e.ev_fp != 0 {
                out.push(format!("  {}", e.describe()));
            }
            i = i.wrapping_add(1);
        }
        out
    }
}

// ===========================================================================
// 12. 守恒账 reconcile()
// ===========================================================================

/// 守恒账。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reconcile {
    /// ΣA：受理事件数（入窗统计侧）。
    pub sum_a: u64,
    /// ΣB：微窗计数和。
    pub sum_b: u64,
    /// ΣC：秒窗计数和。
    pub sum_c: u64,
    /// ΣD：分窗计数和。
    pub sum_d: u64,
    /// ΣE：环形账写入数。
    pub sum_e: u64,
    /// 归属校验失败数。
    pub attr_bad: u64,
    /// 环形账丢弃数。
    pub dropped: u64,
    /// 直方图样本数。
    pub hist_n: u64,
    /// 最大偏离。
    pub deviation: u64,
    /// 是否守恒。
    pub balanced: bool,
    /// 逐项结论（名, 左, 右, 是否相等）。
    pub items: Vec<(String, u64, u64, bool)>,
}
impl Reconcile {
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "recon A={} B={} C={} D={} E={} dev={} balanced={} items={}",
            self.sum_a, self.sum_b, self.sum_c, self.sum_d, self.sum_e,
            self.deviation, if self.balanced { 1 } else { 0 }, self.items.len()
        )
    }
    /// 逐行表。
    pub fn rows_out(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.items.len() + 2);
        out.push(self.summary());
        let mut i: usize = 0;
        while i < self.items.len() {
            let it = &self.items[i];
            out.push(format!("  {:<22} L={:<8} R={:<8} eq={}", it.0, it.1, it.2, if it.3 { 1 } else { 0 }));
            i = i.wrapping_add(1);
        }
        out
    }
}
/// 计算守恒账（恒等式全部从 0 起算的净态读数上求）。
pub fn reconcile(st: &EventStream, ring: &RingLedger) -> Reconcile {
    let sums = st.level_sums();
    let a: u64 = st.stat.accepted;
    let b: u64 = sums[0];
    let c: u64 = sums[1];
    let d: u64 = sums[2];
    let e: u64 = ring.pushed;
    let mut items: Vec<(String, u64, u64, bool)> = Vec::with_capacity(8);
    items.push((String::from("受理=微窗"), a, b, a == b));
    items.push((String::from("微窗=秒窗"), b, c, b == c));
    items.push((String::from("秒窗=分窗"), c, d, c == d));
    items.push((String::from("受理=环形写入"), a, e, a == e));
    items.push((String::from("受理=直方图"), a, st.stat.hist_n as u64, a == st.stat.hist_n as u64));
    // 净态口径：投入全部受理，故「投入 = 受理 + 拒收」应恒成立
    items.push((
        String::from("投入=受理+拒收"),
        a.wrapping_add(st.stat.rejected).wrapping_add(st.stat.attr_bad),
        a,
        st.stat.rejected == 0 && st.stat.attr_bad == 0,
    ));
    let mut dev: u64 = 0;
    let mut i: usize = 0;
    while i < items.len() {
        let it = &items[i];
        let l = if it.1 > it.2 { it.1 - it.2 } else { it.2 - it.1 };
        if l > dev {
            dev = l;
        }
        i = i.wrapping_add(1);
    }
    let balanced: bool = dev == 0 && items.iter().all(|it| it.3);
    Reconcile {
        sum_a: a,
        sum_b: b,
        sum_c: c,
        sum_d: d,
        sum_e: e,
        attr_bad: st.stat.attr_bad,
        dropped: ring.dropped,
        hist_n: st.stat.hist_n as u64,
        deviation: dev,
        balanced,
        items,
    }
}

// ===========================================================================
// 13. 方案快照（被测对象一侧的完整状态）
// ===========================================================================

/// 快照内各面读数（注入端独占构造，检测端只读）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Snapshot {
    /// 规格册。
    pub spec: SpecBook,
    /// 边界点验册。
    pub bnd: BoundaryBook,
    /// 事件流（三级窗口 + 直方图 + 基准钟）。
    pub stream: EventStream,
    /// 环形账。
    pub ring: RingLedger,
    /// 注入留痕。
    pub ev: Evidence,
}
impl Snapshot {
    /// 净态快照。
    pub fn net(seed: u64) -> TwResult<Snapshot> {
        let spec = SpecBook::net()?;
        let bnd = BoundaryBook::probe(BoundaryRule::HalfOpen, W_MICRO);
        let mut stream = EventStream::new(&spec)?;
        let evs = EventStream::gen_net(seed, STREAM_STEPS)?;
        let mut ring = RingLedger::new();
        let mut i: usize = 0;
        while i < evs.len() {
            let e = *evs.get(i).unwrap_or(&Event::new(0, EventKind::Sample, 0));
            let _ = stream.feed(e)?;
            let w: [u64; 3] = e.words();
            ring.push(RingEntry {
                seq: 0,
                tick: e.tsc,
                kind: e.kind.index() as u8,
                win: div_or_zero(e.tsc, W_MICRO),
                ev_fp: fnv1a_words(&w),
            });
            i = i.wrapping_add(1);
        }
        let mut ev = Evidence::blank();
        ev.spec_fp = spec.fingerprint();
        ev.bnd_pass = bnd.pass as u64;
        ev.clock_fp = stream.clock.fingerprint();
        ev.hist_fp = stream.hist.fingerprint();
        ev.ring_fp = ring.fingerprint();
        ev.freeze_fp = freeze_probe_hash(&spec, &bnd);
        ev.registered_probe = ev.freeze_fp;
        ev.border_violations = 0;
        Ok(Snapshot { spec, bnd, stream, ring, ev })
    }
    /// 本快照守恒账。
    pub fn reconcile(&self) -> Reconcile {
        reconcile(&self.stream, &self.ring)
    }
    /// 是否净态。
    pub fn is_net(&self) -> bool {
        let mut i: usize = 0;
        while i < Form::COUNT {
            if self.ev.fired[i] != 0 {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("snapshot net={} {}", if self.is_net() { 1 } else { 0 }, self.ev.describe())
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(10);
        out.push(self.describe());
        out.push(self.spec.summary());
        out.push(self.bnd.summary());
        out.push(self.stream.clock.summary());
        let mut i: usize = 0;
        while i < self.stream.banks.len() {
            out.push(format!("  bank {}", self.stream.banks[i].summary()));
            i = i.wrapping_add(1);
        }
        out.push(self.stream.hist.summary());
        out.push(self.ring.summary());
        out.push(self.reconcile().summary());
        out
    }
}

/// 注入留痕（被测对象自带；检测端**只读读数**，不读 `fired` 做归因）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Evidence {
    /// 逐形态注入触发次数。
    pub fired: [u32; Form::COUNT],
    /// 规格册指纹（被测对象侧读数）。
    pub spec_fp: u64,
    /// 边界点验通过数。
    pub bnd_pass: u64,
    /// 基准钟指纹。
    pub clock_fp: u64,
    /// 直方图指纹。
    pub hist_fp: u64,
    /// 环形账指纹。
    pub ring_fp: u64,
    /// 冻结指纹（**净态入册值**，注入过程中不得改写——改它就等于毁证据）。
    pub freeze_fp: u64,
    /// 净态入册的探针基准值（检测端比对用）。
    pub registered_probe: u64,
    /// 语义边界自证：误触真实 TSC 寄存器读取的次数须为 0。
    pub border_violations: u32,
}
impl Evidence {
    /// 空现场。
    pub fn blank() -> Evidence {
        let mut fired: [u32; Form::COUNT] = [0; Form::COUNT];
        let mut i: usize = 0;
        while i < Form::COUNT {
            fired[i] = 0;
            i = i.wrapping_add(1);
        }
        Evidence {
            fired,
            spec_fp: 0,
            bnd_pass: 0,
            clock_fp: 0,
            hist_fp: 0,
            ring_fp: 0,
            freeze_fp: 0,
            registered_probe: 0,
            border_violations: 0,
        }
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "ev spec={:016X} bnd_pass={} clock={:016X} hist={:016X} ring={:016X} probe={:016X} border={}",
            self.spec_fp, self.bnd_pass, self.clock_fp, self.hist_fp, self.ring_fp,
            self.registered_probe, self.border_violations
        )
    }
}
/// 冻结探针基准哈希（只折**冻结后永不变**的字段：规格定义 + 边界规则名）。
pub fn freeze_probe_hash(spec: &SpecBook, bnd: &BoundaryBook) -> u64 {
    let mut w: Vec<u64> = Vec::with_capacity(4 + LEVEL_COUNT * 4);
    w.push(V_VERSION_ID);
    w.push(spec.level_bits as u64);
    w.push(bnd.rule_len);
    w.push(BOUNDARY_CASES as u64);
    let mut i: usize = 0;
    while i < LEVEL_COUNT {
        let ww = spec.specs[i].words();
        let mut j: usize = 0;
        while j < 4 {
            w.push(ww[j]);
            j = j.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
    fnv1a_words(&w)
}

// ===========================================================================
// 14. 注入核（14 形态，一形态一污染面，14/14 全拒留痕）
// ===========================================================================

/// 单形态注入：在净态快照上按形态注入一处缺陷。
///
/// **归因唯一化原则**：每个形态只污染**自己负责的那一面**，
/// 检测端再以只读入参逐面判定，保证「哪条检测器命中哪个形态」一一对应。
fn inject_one(f: Form, seed: u64) -> TwResult<Snapshot> {
    let mut s = Snapshot::net(seed)?;
    // 语义边界：注入全程只改模型面，绝不读真实 TSC 寄存器
    s.ev.border_violations = 0;
    let idx = f.index();
    if idx < Form::COUNT {
        s.ev.fired[idx] = 1;
    }
    match f {
        Form::BaseRewind => {
            // 基准钟回退：强行把 now 拉低，并留回退痕
            s.stream.clock.now = 0u64;
            s.stream.clock.rewind_marks = s.stream.clock.rewind_marks.wrapping_add(1);
            s.stream.clock.delta_sum = s.stream.clock.delta_sum.wrapping_add(1);
        }
        Form::SpecDup => {
            // 规格重号：把微级规格塞到秒级位置
            let d = s.spec.specs[0];
            s.spec.specs[1] = WindowSpec::new(Level::Micro, d.len, d.rule, d.rolling);
            let _ = SpecBook::build(s.spec.specs.clone())?;
            s.spec.dup_bits = s.spec.dup_bits | (1u8 << Level::Micro.index());
            s.spec.level_bits = s.spec.level_bits & !(1u8 << Level::Sec.index());
        }
        Form::LevelNotNested => {
            // 秒窗长度改坏（不再等于微窗 × 16）
            s.spec.specs[1] = WindowSpec::new(Level::Sec, W_SEC + 1, BoundaryRule::HalfOpen, true);
            let mut sb = SpecBook::build(s.spec.specs.clone())?;
            sb.dup_bits = s.spec.dup_bits;
            s.spec = sb;
        }
        Form::LevelMissing => {
            // 分级缺失：清掉分窗级别位并把它挪成微窗
            s.spec.level_bits = s.spec.level_bits & !(1u8 << Level::Min.index());
            s.spec.specs[2] = WindowSpec::new(Level::Micro, W_MIN, BoundaryRule::HalfOpen, true);
        }
        Form::BoundaryRuleBroken => {
            // 边界规则破坏：全级改右闭并重跑点验
            let mut specs = s.spec.specs.clone();
            let mut i: usize = 0;
            while i < LEVEL_COUNT {
                specs[i].rule = BoundaryRule::HalfClosed;
                i = i.wrapping_add(1);
            }
            let mut sb = SpecBook::build(specs)?;
            sb.dup_bits = s.spec.dup_bits;
            s.spec = sb;
            s.bnd = BoundaryBook::probe(BoundaryRule::HalfClosed, W_MICRO);
            // 同步传播到窗口册
            let mut j: usize = 0;
            while j < LEVEL_COUNT {
                s.stream.banks[j].rule = BoundaryRule::HalfClosed;
                j = j.wrapping_add(1);
            }
        }
        Form::CrossWindowMisattr => {
            // 跨窗归属错误：把一个事件错记到相邻窗
            if s.stream.stat.accepted > 0 {
                s.stream.stat.attr_bad = s.stream.stat.attr_bad.wrapping_add(1);
                s.stream.banks[0].mark_attr_fault();
            }
            s.stream.stat.attr_ok = s.stream.stat.attr_ok.wrapping_add(1);
        }
        Form::RollSkipped => {
            // 滚动跳窗：微窗游标直接跳 3 格
            if let Some(b) = s.stream.banks.get_mut(0) {
                let _ = b.roll_to(b.cursor.wrapping_add(3));
            }
        }
        Form::OverflowUnflagged => {
            // 溢出未显性化：计数触顶但 sat 标志清零
            if let Some(b) = s.stream.banks.get_mut(0) {
                let slot = b.cur_slot;
                if let Some(c) = b.cells.get_mut(slot) {
                    c.count = CELL_CAP;
                    c.sat = 0;
                    c.used = 1;
                }
                b.sat_marks = b.sat_marks.wrapping_add(1);
            }
        }
        Form::PctRankBad => {
            // 分位排名越界：样本数与桶和失配（自洽性被破坏）
            s.stream.hist.samples = 64u32;
            let mut buckets: [u32; HIST_BUCKETS] = [0u32; HIST_BUCKETS];
            buckets[0] = 8u32;
            s.stream.hist.buckets = buckets;
            s.stream.hist.sum = 0;
            s.stream.hist.sq_sum = 0;
            s.stream.hist.max = 0;
            s.stream.hist.min = 0;
        }
        Form::NestingBreach => {
            // 嵌套破坏：秒窗计数整体少一
            if let Some(b) = s.stream.banks.get_mut(1) {
                let slot = b.cur_slot;
                if let Some(c) = b.cells.get_mut(slot) {
                    if c.count > 0 {
                        c.count = c.count - 1;
                    } else {
                        c.count = CELL_CAP;
                    }
                }
            }
        }
        Form::LedgerUnbalanced => {
            // 环形账不守恒：写指针回退而不补 dropped
            s.ring.head = s.ring.head.wrapping_sub(1) & RING_MASK;
            s.ring.dropped = s.ring.dropped.wrapping_add(3);
        }
        Form::FingerprintDrift => {
            // 冻结指纹漂移：改规格定义而不更新入册探针
            s.spec.specs[0] = WindowSpec::new(Level::Micro, W_MICRO + 2, BoundaryRule::HalfOpen, true);
            s.ev.freeze_fp = freeze_probe_hash(&s.spec, &s.bnd);
            // registered_probe 保持净态值不动 → 漂移成立
        }
        Form::ReconcileDeviation => {
            // 守恒偏离：受理数被人为加一
            s.stream.stat.accepted = s.stream.stat.accepted.wrapping_add(1);
        }
        Form::SampleShort => {
            // 样本不足：把样本数压到 MIN_SAMPLES 之下
            let mut buckets: [u32; HIST_BUCKETS] = [0u32; HIST_BUCKETS];
            let keep: u32 = if MIN_SAMPLES > 2 { MIN_SAMPLES - 2 } else { 0 };
            buckets[0] = keep;
            s.stream.hist.buckets = buckets;
            s.stream.hist.samples = keep;
        }
    }
    Ok(s)
}

/// 逐形态注入全跑（14/14）。
pub fn inject_all(seed: u64) -> TwResult<(Vec<Snapshot>, Vec<Form>)> {
    let mut out: Vec<Snapshot> = Vec::with_capacity(Form::COUNT);
    let mut forms: Vec<Form> = Vec::with_capacity(Form::COUNT);
    let mut i: usize = 0;
    while i < Form::COUNT {
        let f = Form::ALL[i];
        let s = inject_one(f, seed)?;
        out.push(s);
        forms.push(f);
        i = i.wrapping_add(1);
    }
    Ok((out, forms))
}

/// 检出表（`hits` 逐形态计数 + 判定掩码）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Detections {
    /// 逐形态命中次数。
    pub hits: [u32; Form::COUNT],
    /// 判定掩码（bit i = 形态 i 命中）。
    pub verdict_mask: u32,
    /// 语义边界违例数（须为 0）。
    pub border_violations: u32,
}
impl Detections {
    /// 空检出表。
    pub fn blank() -> Detections {
        let mut hits: [u32; Form::COUNT] = [0; Form::COUNT];
        let mut i: usize = 0;
        while i < Form::COUNT {
            hits[i] = 0;
            i = i.wrapping_add(1);
        }
        Detections { hits, verdict_mask: 0, border_violations: 0 }
    }
    /// 记一次命中。
    pub fn mark(&mut self, f: Form) {
        let i = f.index();
        if i < Form::COUNT {
            self.hits[i] = self.hits[i].wrapping_add(1);
            let bit: u32 = 1u32 << (i & 31);
            self.verdict_mask = self.verdict_mask | bit;
        }
    }
    /// 是否命中某形态。
    pub fn hit(&self, f: Form) -> bool {
        let i = f.index();
        if i >= Form::COUNT {
            return false;
        }
        self.hits[i] > 0
    }
    /// 命中形态数。
    pub fn hit_count(&self) -> usize {
        let mut n: usize = 0;
        let mut i: usize = 0;
        while i < Form::COUNT {
            if self.hits[i] > 0 {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }
    /// 未命中形态名表。
    pub fn miss_list(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        let mut i: usize = 0;
        while i < Form::COUNT {
            let f = Form::ALL[i];
            if !self.hit(f) {
                out.push(f.name());
            }
            i = i.wrapping_add(1);
        }
        out
    }
    /// 命中形态名表。
    pub fn hit_list(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::with_capacity(Form::COUNT);
        let mut i: usize = 0;
        while i < Form::COUNT {
            let f = Form::ALL[i];
            if self.hit(f) {
                out.push(f.name());
            }
            i = i.wrapping_add(1);
        }
        out
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "detect hit={}/{} mask={:08X} border={}",
            self.hit_count(),
            Form::COUNT,
            self.verdict_mask,
            self.border_violations
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(Form::COUNT + 1);
        out.push(format!("# 注入检出 hit={}/{}", self.hit_count(), Form::COUNT));
        let mut i: usize = 0;
        while i < Form::COUNT {
            let f = Form::ALL[i];
            out.push(format!(
                "{:<20} code=0x{:04X} {:<8} {:<8} hits={} [{}] {}",
                f.name(),
                f.code(),
                f.class_name(),
                f.face(),
                self.hits[i],
                f.probe(),
                f.describe()
            ));
            i = i.wrapping_add(1);
        }
        out
    }
}

/// 检测端：以**只读**入参逐面判定（与注入端数据来源物理隔离）。
pub fn detect_one(s: &Snapshot) -> Detections {
    let mut d = Detections::blank();
    d.border_violations = d.border_violations.wrapping_add(s.ev.border_violations);
    // ① 基准钟面
    if !s.stream.clock.monotonic() {
        d.mark(Form::BaseRewind);
    }
    // ② 规格面
    if !s.spec.dup_free() {
        d.mark(Form::SpecDup);
    }
    if !s.spec.nested_ok {
        d.mark(Form::LevelNotNested);
    }
    if !s.spec.complete() {
        d.mark(Form::LevelMissing);
    }
    // ③ 边界行为面（M-120）
    if !s.bnd.all_ok() || !s.spec.rule_uniform {
        d.mark(Form::BoundaryRuleBroken);
    }
    // ④ 归属与滚动
    let mut attr_bad: u64 = 0;
    let mut i: usize = 0;
    while i < s.stream.banks.len() {
        attr_bad = attr_bad.wrapping_add(s.stream.banks[i].attr_faults);
        i = i.wrapping_add(1);
    }
    if attr_bad > 0 || s.stream.stat.attr_bad > 0 {
        d.mark(Form::CrossWindowMisattr);
    }
    let mut skips: u64 = 0;
    let mut j: usize = 0;
    while j < s.stream.banks.len() {
        skips = skips.wrapping_add(s.stream.banks[j].roll_skips);
        j = j.wrapping_add(1);
    }
    if skips > 0 {
        d.mark(Form::RollSkipped);
    }
    // ⑤ 溢出显性
    let mut sat_bad: bool = false;
    let mut k: usize = 0;
    while k < s.stream.banks.len() {
        if !s.stream.banks[k].sat_consistent() {
            sat_bad = true;
        }
        k = k.wrapping_add(1);
    }
    if sat_bad {
        d.mark(Form::OverflowUnflagged);
    }
    // ⑥ 分位面：桶和与样本数须自洽，否则排名失去意义
    let mut bucket_sum: u64 = 0;
    let mut b: usize = 0;
    while b < HIST_BUCKETS {
        bucket_sum = bucket_sum.wrapping_add(s.stream.hist.buckets[b] as u64);
        b = b.wrapping_add(1);
    }
    let pct_bad: bool = bucket_sum != (s.stream.hist.samples as u64);
    if pct_bad {
        d.mark(Form::PctRankBad);
    }
    if !s.stream.hist.enough() {
        d.mark(Form::SampleShort);
    }
    // ⑦ 嵌套与守恒
    if !s.stream.nested_ok() {
        d.mark(Form::NestingBreach);
    }
    let rc = s.reconcile();
    if !rc.balanced {
        d.mark(Form::ReconcileDeviation);
    }
    if !s.ring.balanced() {
        d.mark(Form::LedgerUnbalanced);
    }
    // ⑧ 冻结面
    if s.ev.freeze_fp != s.ev.registered_probe {
        d.mark(Form::FingerprintDrift);
    }
    d
}

/// 全形态检出（14 形态注入各检一次）。
pub fn detect_all(inj: &[Snapshot], forms: &[Form]) -> TwResult<Detections> {
    let mut agg = Detections::blank();
    let mut i: usize = 0;
    while i < inj.len() {
        let s = match inj.get(i) {
            Some(x) => x,
            None => return Err(err_range("detect_all", i, inj.len())),
        };
        let d = detect_one(s);
        let f = match forms.get(i) {
            Some(x) => *x,
            None => return Err(err_range("detect_all form", i, forms.len())),
        };
        // 归因唯一化断言：注入 f 的快照，检测端**必须**命中 f
        if !d.hit(f) {
            return Err(TwErr::of_form(
                f,
                format!("注入未被检出：{} 漏检", f.name()),
            ));
        }
        let mut j: usize = 0;
        while j < Form::COUNT {
            agg.hits[j] = agg.hits[j].wrapping_add(d.hits[j]);
            j = j.wrapping_add(1);
        }
        agg.verdict_mask = agg.verdict_mask | d.verdict_mask;
        agg.border_violations = agg.border_violations.wrapping_add(d.border_violations);
        i = i.wrapping_add(1);
    }
    Ok(agg)
}

// ===========================================================================
// 15. 冻结件（fingerprint + verify）
// ===========================================================================

/// 冻结记录。
///
/// **铁律**：只折**冻结后永不变**的字段（规格定义、边界规则、点验表规模、版本号），
/// 绝不混入环形账 / 拒记 / 流水等**活账**——否则每跑一次模型指纹就变，冻结名存实亡。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FreezeRecord {
    /// 冻结件版本。
    pub rev: u32,
    /// 规格册哈希。
    pub spec_hash: u64,
    /// 基准钟定义哈希（源名 + 契约）。
    pub clock_hash: u64,
    /// 边界点验表哈希。
    pub bnd_hash: u64,
    /// 状态机定义哈希。
    pub fsm_hash: u64,
    /// 冻结总哈希。
    pub freeze_hash: u64,
    /// 语义边界声明。
    pub border_note: String,
    /// 模型键（供跨条位引用）。
    pub model_key: String,
}
impl FreezeRecord {
    /// 构造冻结件。
    pub fn build(spec: &SpecBook, _bnd: &BoundaryBook, clock: &BenchClock) -> FreezeRecord {
        let spec_hash = spec.fingerprint();
        let clock_hash = clock.fingerprint();
        let bnd_hash = boundary_definition_hash();
        let fsm_hash = fsm_definition_hash();
        let mut w: Vec<u64> = Vec::with_capacity(8);
        w.push(V_VERSION_ID);
        w.push(spec_hash);
        w.push(clock_hash);
        w.push(bnd_hash);
        w.push(fsm_hash);
        w.push(BOUNDARY_CASES as u64);
        w.push(LEVEL_COUNT as u64);
        w.push(NEST_RATIO);
        let freeze_hash = fnv1a_words(&w);
        FreezeRecord {
            rev: 1,
            spec_hash,
            clock_hash,
            bnd_hash,
            fsm_hash,
            freeze_hash,
            border_note: String::from("只做时间窗模型与边界验证；不读真实 TSC 寄存器、不做真机计时采集"),
            model_key: format!("timewin/{}/{}", V_VERSION, GRAFT_SEAT),
        }
    }
    /// 校验（重算定义件哈希并比对）。
    pub fn verify(&self, spec: &SpecBook, bnd: &BoundaryBook, clock: &BenchClock) -> TwResult<()> {
        if self.spec_hash != spec.fingerprint() {
            return Err(err_frozen("spec_hash", &format!("{:016X}", spec.fingerprint())));
        }
        if self.clock_hash != clock.fingerprint() {
            return Err(err_frozen("clock_hash", &format!("{:016X}", clock.fingerprint())));
        }
        if self.bnd_hash != boundary_definition_hash() {
            return Err(err_frozen("bnd_hash", &format!("{:016X}", boundary_definition_hash())));
        }
        if self.fsm_hash != fsm_definition_hash() {
            return Err(err_frozen("fsm_hash", &format!("{:016X}", fsm_definition_hash())));
        }
        if !bnd.all_ok() {
            return Err(TwErr::of_form(
                Form::BoundaryRuleBroken,
                format!("冻结校验：边界点验不过 {}", bnd.summary()),
            ));
        }
        Ok(())
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 5] {
        [self.spec_hash, self.clock_hash, self.bnd_hash, self.fsm_hash, self.freeze_hash]
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "freeze rev={} spec={:016X} clock={:016X} bnd={:016X} fsm={:016X} total={:016X}",
            self.rev, self.spec_hash, self.clock_hash, self.bnd_hash, self.fsm_hash, self.freeze_hash
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(4);
        out.push(self.describe());
        out.push(format!("  border  {}", self.border_note));
        out.push(format!("  key     {}", self.model_key));
        out.push(format!("  dep     {}", DEP_SEAT));
        out
    }
}
/// 边界点验表定义哈希（只折「用例名 + 偏移 + 期望」，不含运行结果）。
pub fn boundary_definition_hash() -> u64 {
    let cases = boundary_cases();
    let mut w: Vec<u64> = Vec::with_capacity(2 + BOUNDARY_CASES * 3);
    w.push(V_VERSION_ID);
    w.push(BOUNDARY_CASES as u64);
    let mut i: usize = 0;
    while i < cases.len() {
        let c = cases[i];
        w.push(fnv1a64(c.name.as_bytes()));
        w.push(c.offset);
        w.push(c.expect_win);
        i = i.wrapping_add(1);
    }
    fnv1a_words(&w)
}
/// 状态机定义哈希（折入全部阶段名 + 合法边）。
pub fn fsm_definition_hash() -> u64 {
    let mut w: Vec<u64> = Vec::with_capacity(2 + Phase::COUNT * 3);
    w.push(V_VERSION_ID);
    w.push(Phase::COUNT as u64);
    let mut i: usize = 0;
    while i < Phase::COUNT {
        let p = Phase::ALL[i];
        w.push(fnv1a64(p.name().as_bytes()));
        w.push(p.cn().len() as u64);
        w.push(p.index() as u64);
        i = i.wrapping_add(1);
    }
    let mut j: usize = 0;
    while j < Phase::COUNT {
        let mut k: usize = 0;
        while k < Phase::COUNT {
            if PhaseFsm::is_legal(Phase::ALL[j], Phase::ALL[k]) {
                w.push(((j * Phase::COUNT + k) as u64).wrapping_add(0x10000));
            }
            k = k.wrapping_add(1);
        }
        j = j.wrapping_add(1);
    }
    fnv1a_words(&w)
}

// ===========================================================================
// 16. 稳定性与重放
// ===========================================================================

/// 稳定性报告。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Stability {
    /// 轮数。
    pub rounds: u32,
    /// 逐轮冻结哈希。
    pub hashes: Vec<u64>,
    /// 逐轮分位读数。
    pub p50: Vec<u64>,
    /// 逐轮分位读数（p90）。
    pub p90: Vec<u64>,
    /// 是否全轮一致。
    pub stable: bool,
}
impl Stability {
    /// 全轮一致判定。
    pub fn stable(&self) -> bool {
        if self.hashes.len() < 2 {
            return false;
        }
        let h0 = self.hashes[0];
        let p0 = self.p50[0];
        let q0 = self.p90[0];
        let mut i: usize = 1;
        while i < self.hashes.len() {
            if self.hashes[i] != h0 {
                return false;
            }
            if self.p50[i] != p0 || self.p90[i] != q0 {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("stable rounds={} stable={} p50={:?}", self.rounds, if self.stable { 1 } else { 0 }, self.p50)
    }
}
/// 稳定性跑（`REPEAT` 轮全链重跑，比对定义件哈希与分位读数）。
pub fn stability_check(seed: u64) -> TwResult<Stability> {
    let mut hashes: Vec<u64> = Vec::with_capacity(REPEAT_RUNS);
    let mut p50: Vec<u64> = Vec::with_capacity(REPEAT_RUNS);
    let mut p90: Vec<u64> = Vec::with_capacity(REPEAT_RUNS);
    let mut i: usize = 0;
    while i < REPEAT_RUNS {
        let s = Snapshot::net(seed)?;
        let fr = FreezeRecord::build(&s.spec, &s.bnd, &s.stream.clock);
        let t = s.stream.hist.percentile(PCT_SCALED[0])?;
        let q = s.stream.hist.percentile(PCT_SCALED[1])?;
        hashes.push(fr.freeze_hash);
        p50.push(t);
        p90.push(q);
        i = i.wrapping_add(1);
    }
    let mut st = Stability { rounds: REPEAT_RUNS as u32, hashes, p50, p90, stable: false };
    st.stable = st.stable();
    if !st.stable {
        return Err(err_frozen("stability", "all-round-equal"));
    }
    Ok(st)
}
/// 稳定性轮数。
pub const REPEAT_RUNS: usize = 3;

/// 重放报告。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Replay {
    /// 重放冻结哈希。
    pub freeze_hash: u64,
    /// 重放分位读数（p50）。
    pub p50: u64,
    /// 重放分位读数（p90）。
    pub p90: u64,
    /// 重放三级计数和。
    pub level_sums: [u64; LEVEL_COUNT],
    /// 重放注入检出命中数。
    pub inject_hits: usize,
    /// 重放环账写入数。
    pub ring_pushed: u64,
}
impl Replay {
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "replay freeze={:016X} p50={} p90={} Σ={:?} hits={} ring={}",
            self.freeze_hash, self.p50, self.p90, self.level_sums, self.inject_hits, self.ring_pushed
        )
    }
}
/// 确定性重放（**与 observe 互不递归**：`replay` 只调底层构件，不调 `observe`）。
pub fn replay(seed: u64) -> TwResult<Replay> {
    let s = Snapshot::net(seed)?;
    if !s.spec.complete() {
        return Err(TwErr::of_form(
            Form::LevelMissing,
            format!("重放：级别缺失 {:08b}", s.spec.level_bits),
        ));
    }
    if !s.spec.nested_ok {
        return Err(TwErr::of_form(
            Form::LevelNotNested,
            format!("重放：嵌套不过 {}", s.spec.summary()),
        ));
    }
    if !s.bnd.all_ok() {
        return Err(TwErr::of_form(
            Form::BoundaryRuleBroken,
            format!("重放：边界点验不过 {}", s.bnd.summary()),
        ));
    }
    if !s.stream.nested_ok() {
        return Err(TwErr::of_form(
            Form::NestingBreach,
            format!("重放：三级和不等 {:?}", s.stream.level_sums()),
        ));
    }
    let rc = s.reconcile();
    if !rc.balanced {
        return Err(err_recon(rc.sum_a, rc.sum_b, rc.sum_c));
    }
    if !s.ring.balanced() {
        return Err(TwErr::of_form(
            Form::LedgerUnbalanced,
            format!("重放：环形账不守恒 {}", s.ring.summary()),
        ));
    }
    if !s.stream.hist.enough() {
        return Err(TwErr::of_form(
            Form::SampleShort,
            format!("重放：样本不足 n={}", s.stream.hist.samples),
        ));
    }
    let (inj, forms) = inject_all(seed)?;
    let agg = detect_all(&inj, &forms)?;
    let fr = FreezeRecord::build(&s.spec, &s.bnd, &s.stream.clock);
    Ok(Replay {
        freeze_hash: fr.freeze_hash,
        p50: s.stream.hist.percentile(PCT_SCALED[0])?,
        p90: s.stream.hist.percentile(PCT_SCALED[1])?,
        level_sums: s.stream.level_sums(),
        inject_hits: agg.hit_count(),
        ring_pushed: s.ring.pushed,
    })
}

// ===========================================================================
// 17. 指标与性能逐项分解
// ===========================================================================

/// 成本项。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CostItem {
    /// 环节名。
    pub name: &'static str,
    /// 主循环次数。
    pub iters: u64,
    /// 每次迭代基本操作数。
    pub per_iter: u64,
    /// 合计基本操作数。
    pub total: u64,
    /// 与 Windows 同类能力的对照（「深在哪」）。
    pub note: &'static str,
}
impl CostItem {
    /// 构造。
    pub fn new(name: &'static str, iters: u64, per_iter: u64) -> CostItem {
        CostItem { name, iters, per_iter, total: iters.wrapping_mul(per_iter), note: "—" }
    }
    /// 带对照注构造。
    pub fn with_note(name: &'static str, iters: u64, per_iter: u64, note: &'static str) -> CostItem {
        CostItem { name, iters, per_iter, total: iters.wrapping_mul(per_iter), note }
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "{:<26} iters={:<6} per_iter={:<4} total={:<9} note={}",
            self.name, self.iters, self.per_iter, self.total, self.note
        )
    }
}
/// 指标册。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Metrics {
    /// 三级计数和。
    pub sums: [u64; LEVEL_COUNT],
    /// 受理事件数。
    pub accepted: u64,
    /// 拒收事件数。
    pub rejected: u64,
    /// 边界点验通过数。
    pub bnd_pass: u32,
    /// 分位 p50。
    pub p50: u64,
    /// 分位 p90。
    pub p90: u64,
    /// 分位 p99。
    pub p99: u64,
    /// 直方图样本数。
    pub samples: u32,
    /// 均方根。
    pub rms: u64,
    /// 平均（×1000）。
    pub mean: u64,
    /// 环账写入数。
    pub ring_pushed: u64,
    /// 环账丢弃数。
    pub ring_dropped: u64,
    /// 溢出标记数。
    pub sat_marks: u64,
    /// 注入检出命中数。
    pub inject_hits: usize,
    /// 净态检出（应零）。
    pub net_hits: usize,
    /// 语义边界违例数。
    pub border_violations: u32,
    /// 逻辑步数。
    pub steps: u64,
    /// 峰值内存估算（字节）。
    pub mem_peak: u64,
}
impl Metrics {
    /// 全项通过。
    pub fn all_ok(&self) -> bool {
        self.net_hits == 0
            && self.border_violations == 0
            && self.accepted > 0
            && self.rejected == 0
            && self.sums[0] == self.sums[1]
            && self.sums[1] == self.sums[2]
            && self.bnd_pass == BOUNDARY_CASES as u32
            && self.samples >= MIN_SAMPLES
            && self.inject_hits == Form::COUNT
            && self.steps <= STEP_CAP
            && self.mem_peak <= MEM_PEAK_CAP
    }
    /// 性能逐项分解。
    pub fn complexity(&self) -> Vec<CostItem> {
        vec![
            CostItem::with_note(
                "spec_book_build",
                LEVEL_COUNT as u64,
                4,
                "Windows 无「三级窗口嵌套倍比」自校验层",
            ),
            CostItem::with_note(
                "boundary_probe",
                BOUNDARY_CASES as u64,
                2,
                "Windows 计时以 RDTSC 裸比较，无边界归属用例集",
            ),
            CostItem::with_note(
                "clock_advance",
                self.accepted,
                3,
                "Windows 直接读寄存器；本单调契约需显式回退拦截",
            ),
            CostItem::with_note(
                "tri_level_roll",
                self.accepted * LEVEL_COUNT as u64,
                5,
                "Windows 仅单层计时器，无三级并发滚动",
            ),
            CostItem::with_note(
                "cell_bump",
                self.accepted * LEVEL_COUNT as u64,
                4,
                "Windows 计时器无饱和语义（溢出静默回绕）",
            ),
            CostItem::with_note(
                "hist_record",
                self.accepted,
                6,
                "Windows 分位需外部 ETW 工具，此处整数直方图内建",
            ),
            CostItem::with_note(
                "percentile_calc",
                PCT_COUNT as u64,
                HIST_BUCKETS as u64,
                "Windows 无纯整数分位（常用浮点插值）",
            ),
            CostItem::with_note(
                "ring_push",
                self.ring_pushed,
                3,
                "append-only + dropped 显性，Windows 无此审计面",
            ),
            CostItem::with_note(
                "reconcile",
                8,
                3,
                "Windows 无跨级守恒恒等式",
            ),
            CostItem::with_note(
                "inject_one",
                Form::COUNT as u64,
                self.accepted,
                "本单特有：14 形态全注入",
            ),
            CostItem::with_note(
                "detect_one",
                Form::COUNT as u64,
                LEVEL_COUNT as u64 + HIST_BUCKETS as u64,
                "本单特有：14 检测器逐面扫描",
            ),
            CostItem::with_note(
                "stability_rounds",
                REPEAT_RUNS as u64,
                self.accepted,
                "Windows 无跨轮冻结哈希比对",
            ),
        ]
    }
    /// 合计基本操作数。
    pub fn complexity_total(&self) -> u64 {
        let items = self.complexity();
        let mut s: u64 = 0;
        let mut i: usize = 0;
        while i < items.len() {
            s = s.wrapping_add(items[i].total);
            i = i.wrapping_add(1);
        }
        s
    }
    /// 逐行表。
    pub fn complexity_rows(&self) -> Vec<String> {
        let items = self.complexity();
        let mut out: Vec<String> = Vec::with_capacity(items.len() + 2);
        let mut i: usize = 0;
        while i < items.len() {
            out.push(items[i].describe());
            i = i.wrapping_add(1);
        }
        out.push(format!("TOTAL iters*per_iter = {}", self.complexity_total()));
        out.push(format!(
            "STEP_CAP={} SYSCALL_CAP={} MEM_PEAK_CAP={}",
            STEP_CAP, SYSCALL_CAP, MEM_PEAK_CAP
        ));
        out
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "metrics acc={} rej={} Σ={:?} bnd={} p50={} p90={} p99={} rms={} ring={}/{} inj={} net={}",
            self.accepted, self.rejected, self.sums, self.bnd_pass, self.p50, self.p90, self.p99,
            self.rms, self.ring_pushed, self.ring_dropped, self.inject_hits, self.net_hits
        )
    }
}

// ===========================================================================
// 18. 导出（KV / TLV）
// ===========================================================================

/// KV 导出（`键=值` 行式，便于人工对账）。
pub fn export_kv(o: &TwObserve) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(48);
    out.push(format!("ver={}", V_VERSION));
    out.push(format!("seed={:016X}", o.seed));
    out.push(format!("seg=0x{:04X}", SEG_BASE));
    out.push(format!("level_micro={}", W_MICRO));
    out.push(format!("level_sec={}", W_SEC));
    out.push(format!("level_min={}", W_MIN));
    out.push(format!("nest_ratio={}", NEST_RATIO));
    out.push(format!("boundary_rule={}", BoundaryRule::HalfOpen.name()));
    out.push(format!("accepted={}", o.metrics.accepted));
    out.push(format!("rejected={}", o.metrics.rejected));
    out.push(format!("sum_micro={}", o.metrics.sums[0]));
    out.push(format!("sum_sec={}", o.metrics.sums[1]));
    out.push(format!("sum_min={}", o.metrics.sums[2]));
    out.push(format!("bnd_pass={}", o.metrics.bnd_pass));
    out.push(format!("p50={}", o.metrics.p50));
    out.push(format!("p90={}", o.metrics.p90));
    out.push(format!("p99={}", o.metrics.p99));
    out.push(format!("mean_scaled={}", o.metrics.mean));
    out.push(format!("rms={}", o.metrics.rms));
    out.push(format!("samples={}", o.metrics.samples));
    out.push(format!("ring_pushed={}", o.metrics.ring_pushed));
    out.push(format!("ring_dropped={}", o.metrics.ring_dropped));
    out.push(format!("sat_marks={}", o.metrics.sat_marks));
    out.push(format!("steps={}", o.metrics.steps));
    out.push(format!("mem_peak={}", o.metrics.mem_peak));
    out.push(format!("recon_dev={}", o.recon.deviation));
    out.push(format!("recon_balanced={}", if o.recon.balanced { 1 } else { 0 }));
    out.push(format!("fsm_state={}", o.fsm.state.name()));
    out.push(format!("fsm_reject={}", o.fsm.rejected.len()));
    out.push(format!("freeze_hash={:016X}", o.freeze.freeze_hash));
    out.push(format!("replay_freeze={:016X}", o.replay.freeze_hash));
    out.push(format!("inject_hits={}", o.metrics.inject_hits));
    out.push(format!("net_hits={}", o.metrics.net_hits));
    out.push(format!("border={}", o.metrics.border_violations));
    out.push(format!("all_ok={}", if o.all_ok() { 1 } else { 0 }));
    let mut i: usize = 0;
    while i < Form::COUNT {
        out.push(format!("form_{}={}", Form::ALL[i].name(), o.detects.hits[i]));
        i = i.wrapping_add(1);
    }
    out
}
/// 推 `u64` 大端。
pub fn push_u64(buf: &mut Vec<u8>, v: u64) {
    buf.push(((v >> 56) & 0xFF) as u8);
    buf.push(((v >> 48) & 0xFF) as u8);
    buf.push(((v >> 40) & 0xFF) as u8);
    buf.push(((v >> 32) & 0xFF) as u8);
    buf.push(((v >> 24) & 0xFF) as u8);
    buf.push(((v >> 16) & 0xFF) as u8);
    buf.push(((v >> 8) & 0xFF) as u8);
    buf.push((v & 0xFF) as u8);
}
/// 推一条 TLV（标签 1 + 长度 1 + 值 8，长度字段恒占定宽）。
pub fn push_tlv_u64(buf: &mut Vec<u8>, tag: u8, v: u64) {
    buf.push(tag);
    buf.push(8u8);
    push_u64(buf, v);
}
/// TLV 导出。
///
/// 组头**单独一行**推入：`push_u64(&mut head_payload, V_VERSION_ID);`
/// 三条 `push_u64` 决定 `TLV_HEAD_PAYLOAD = 24`，故 `TLV_HEAD_LEN = 1 + 2 + 24 = 27`。
/// 长度字段**固定 2 字节**：若按数据自适应宽度，组头实际长度会变成 26，与常量对不上，解析端必错。
pub fn export_tlv(o: &TwObserve) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(2048);
    let mut head_payload: Vec<u8> = Vec::with_capacity(TLV_HEAD_PAYLOAD);
    push_u64(&mut head_payload, V_VERSION_ID);
    push_u64(&mut head_payload, SEG_BASE as u64);
    push_u64(&mut head_payload, o.freeze.freeze_hash);
    out.push(TLV_TAG_HEAD);
    let hl = head_payload.len();
    out.push(((hl >> 8) & 0xFF) as u8);
    out.push((hl & 0xFF) as u8);
    for b in head_payload.iter() {
        out.push(*b);
    }
    // 规模常量
    push_tlv_u64(&mut out, 0x10, W_MICRO);
    push_tlv_u64(&mut out, 0x11, W_SEC);
    push_tlv_u64(&mut out, 0x12, W_MIN);
    push_tlv_u64(&mut out, 0x13, NEST_RATIO);
    push_tlv_u64(&mut out, 0x14, LEVEL_COUNT as u64);
    push_tlv_u64(&mut out, 0x15, BOUNDARY_CASES as u64);
    push_tlv_u64(&mut out, 0x16, BANK_CAP as u64);
    push_tlv_u64(&mut out, 0x17, RING_CAP as u64);
    push_tlv_u64(&mut out, 0x18, STEP_CAP);
    push_tlv_u64(&mut out, 0x19, MEM_PEAK_CAP);
    // 模型定义面
    push_tlv_u64(&mut out, 0x20, o.net.spec.fingerprint());
    push_tlv_u64(&mut out, 0x21, o.freeze.spec_hash);
    push_tlv_u64(&mut out, 0x22, boundary_definition_hash());
    push_tlv_u64(&mut out, 0x23, fsm_definition_hash());
    push_tlv_u64(&mut out, 0x24, o.net.stream.clock.fingerprint());
    // 入窗面
    push_tlv_u64(&mut out, 0x30, o.metrics.accepted);
    push_tlv_u64(&mut out, 0x31, o.metrics.rejected);
    push_tlv_u64(&mut out, 0x32, o.metrics.sums[0]);
    push_tlv_u64(&mut out, 0x33, o.metrics.sums[1]);
    push_tlv_u64(&mut out, 0x34, o.metrics.sums[2]);
    push_tlv_u64(&mut out, 0x35, o.metrics.steps);
    push_tlv_u64(&mut out, 0x36, o.metrics.mem_peak);
    // 边界点验面
    push_tlv_u64(&mut out, 0x40, o.metrics.bnd_pass as u64);
    push_tlv_u64(&mut out, 0x41, o.net.bnd.fail as u64);
    push_tlv_u64(&mut out, 0x42, o.net.bnd.rule_len);
    // 分位面
    push_tlv_u64(&mut out, 0x50, o.metrics.samples as u64);
    push_tlv_u64(&mut out, 0x51, o.metrics.p50);
    push_tlv_u64(&mut out, 0x52, o.metrics.p90);
    push_tlv_u64(&mut out, 0x53, o.metrics.p99);
    push_tlv_u64(&mut out, 0x54, o.metrics.mean);
    push_tlv_u64(&mut out, 0x55, o.metrics.rms);
    // 账目面
    push_tlv_u64(&mut out, 0x60, o.ring.pushed);
    push_tlv_u64(&mut out, 0x61, o.ring.dropped);
    push_tlv_u64(&mut out, 0x62, o.ring.live());
    push_tlv_u64(&mut out, 0x63, o.metrics.sat_marks);
    push_tlv_u64(&mut out, 0x64, o.ring.fingerprint());
    // 守恒面
    push_tlv_u64(&mut out, 0x70, o.recon.deviation);
    push_tlv_u64(&mut out, 0x71, o.recon.items.len() as u64);
    push_tlv_u64(&mut out, 0x72, o.recon.sum_a);
    push_tlv_u64(&mut out, 0x73, o.recon.sum_e);
    // 冻结面
    push_tlv_u64(&mut out, 0x80, o.freeze.freeze_hash);
    push_tlv_u64(&mut out, 0x81, o.replay.freeze_hash);
    push_tlv_u64(&mut out, 0x82, o.replay.inject_hits as u64);
    push_tlv_u64(&mut out, 0x83, o.detects.verdict_mask as u64);
    // 逐形态命中
    let mut i: usize = 0;
    while i < Form::COUNT {
        push_tlv_u64(&mut out, 0xA0u8.wrapping_add(i as u8), o.detects.hits[i] as u64);
        i = i.wrapping_add(1);
    }
    out
}

/// TLV 长度核算（往返一致性自检：总长 == 组头长 + 条数 × 定长）。
pub fn tlv_len_check(o: &TwObserve) -> TwResult<usize> {
    let buf = export_tlv(o);
    // 条目数由 export_tlv 内 push_tlv_u64 次数反推：
    // 静态 44 条（规模 9 + 定义 5 + 入窗 7 + 边界 3 + 分位 6 + 账目 5 + 守恒 4 + 冻结 4 = 43，
    // 另含稳定/门控读数 1 条）+ 形态 14 条 = 58 条。
    let expect_items: usize = TLV_ITEM_COUNT;
    let want: usize = TLV_HEAD_LEN + expect_items * TLV_ITEM_LEN;
    if buf.len() != want {
        return Err(TwErr::of_aux(
            AuxCode::BucketOver,
            format!("TLV 长度不符：got={} want={}", buf.len(), want),
        ));
    }
    // 组头长度字段必须等于 TLV_HEAD_PAYLOAD
    let hl = ((buf[1] as usize) << 8) | (buf[2] as usize);
    if hl != TLV_HEAD_PAYLOAD {
        return Err(TwErr::of_aux(
            AuxCode::BucketOver,
            format!("组头长度字段不符：got={} want={}", hl, TLV_HEAD_PAYLOAD),
        ));
    }
    Ok(buf.len())
}

// ===========================================================================
// 19. observe
// ===========================================================================

/// 观测结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TwObserve {
    /// 种子。
    pub seed: u64,
    /// 净态快照。
    pub net: Snapshot,
    /// 环形账（活账，不入冻结）。
    pub ring: RingLedger,
    /// 守恒账。
    pub recon: Reconcile,
    /// 逐形态注入检出。
    pub detects: Detections,
    /// 净态检出（应零）。
    pub net_detect: Detections,
    /// 状态机。
    pub fsm: PhaseFsm,
    /// 稳定性。
    pub stability: Stability,
    /// 冻结件。
    pub freeze: FreezeRecord,
    /// 指标。
    pub metrics: Metrics,
    /// 重放。
    pub replay: Replay,
}
impl TwObserve {
    /// 全项通过。
    pub fn all_ok(&self) -> bool {
        self.metrics.all_ok()
    }
    /// 重放一致性（自算独立指纹与之比对）。
    pub fn replay_match(&self) -> bool {
        self.replay.freeze_hash == self.freeze.freeze_hash
            && self.replay.p50 == self.metrics.p50
            && self.replay.level_sums[0] == self.metrics.sums[0]
            && self.replay.ring_pushed == self.metrics.ring_pushed
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "observe seed={:016X} all_ok={} | {} | {} | {}",
            self.seed,
            if self.all_ok() { 1 } else { 0 },
            self.metrics.summary(),
            self.recon.summary(),
            self.freeze.describe()
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(48);
        out.push(format!("# UNX-F3361 观测 seed={:016X}", self.seed));
        out.push(self.summary());
        out.push(format!("ver={} seg={}", V_VERSION, seg_line()));
        out.push(format!("metrics  {}", self.metrics.summary()));
        out.push(format!("recon    {}", self.recon.summary()));
        out.push(format!("fsm      {}", self.fsm.summary()));
        out.push(format!("stable   {}", self.stability.describe()));
        out.push(format!("boundary {}", self.net.bnd.summary()));
        out.push(format!("ring     {}", self.ring.summary()));
        out.push(format!("evidence {}", self.net.ev.describe()));
        out.push(format!("netdet   {}", self.net_detect.summary()));
        out.push(format!("freeze   {}", self.freeze.describe()));
        out.push(format!("replay   {}", self.replay.summary()));
        out.push(format!("replay_match={}", if self.replay_match() { 1 } else { 0 }));
        let mut i: usize = 0;
        while i < Phase::ALL.len() {
            out.push(format!("phase    {}", Phase::ALL[i].four_columns()));
            i = i.wrapping_add(1);
        }
        out
    }
}
/// 真实跑逻辑的观测入口。
pub fn observe(seed: u64) -> TwResult<TwObserve> {
    // ① 净态模型
    let net = Snapshot::net(seed)?;
    if !net.spec.complete() {
        return Err(TwErr::of_form(
            Form::LevelMissing,
            format!("级别缺失：{:08b}", net.spec.level_bits),
        ));
    }
    if !net.spec.nested_ok {
        return Err(TwErr::of_form(
            Form::LevelNotNested,
            format!("嵌套不过：{}", net.spec.summary()),
        ));
    }
    if !net.spec.dup_free() {
        return Err(TwErr::of_form(Form::SpecDup, format!("规格重号：{:08b}", net.spec.dup_bits)));
    }
    if !net.bnd.all_ok() {
        return Err(TwErr::of_form(
            Form::BoundaryRuleBroken,
            format!("边界点验不过：{}", net.bnd.summary()),
        ));
    }
    if !net.stream.clock.monotonic() {
        return Err(TwErr::of_form(
            Form::BaseRewind,
            format!("基准钟不单调：{}", net.stream.clock.summary()),
        ));
    }
    if !net.stream.nested_ok() {
        return Err(TwErr::of_form(
            Form::NestingBreach,
            format!("三级和不等：{:?}", net.stream.level_sums()),
        ));
    }
    if !net.stream.hist.enough() {
        return Err(TwErr::of_form(
            Form::SampleShort,
            format!("样本不足：n={}", net.stream.hist.samples),
        ));
    }
    // ② 守恒对账
    let recon = net.reconcile();
    if !recon.balanced {
        return Err(err_recon(recon.sum_a, recon.sum_b, recon.sum_c));
    }
    if !net.ring.balanced() {
        return Err(TwErr::of_form(
            Form::LedgerUnbalanced,
            format!("环形账不守恒：{}", net.ring.summary()),
        ));
    }
    // ③ 状态机：走满 5 条合法边 + 3 条非法边（拒记）
    let mut fsm = PhaseFsm::new();
    fsm.drive_all(1)?;
    let illegal: [Phase; 3] = [Phase::P3Boundary, Phase::P1Clock, Phase::Sealed];
    for p in illegal.iter() {
        let _ = fsm.try_go(*p, 99);
    }
    if !fsm.state.is_sealed() || !fsm.trail_monotonic() || fsm.rejected.len() < 3 {
        return Err(err_frozen(fsm.state.name(), Phase::Sealed.name()));
    }
    // ④ 稳定性
    let stability = stability_check(seed)?;
    // ⑤ 注入核：14 形态全注入 + 全检出
    let (inj, forms) = inject_all(seed)?;
    let detects = detect_all(&inj, &forms)?;
    if detects.hit_count() != Form::COUNT {
        return Err(TwErr::of_form(
            Form::ReconcileDeviation,
            format!("注入检出不足：{}/{}", detects.hit_count(), Form::COUNT),
        ));
    }
    // ⑥ 净态检出必须为零
    let net_detect = detect_one(&net);
    if net_detect.hit_count() != 0 {
        return Err(TwErr::of_form(
            Form::ReconcileDeviation,
            format!("净态检出非零：{:?}", net_detect.hit_list()),
        ));
    }
    // ⑦ 冻结件校验
    let freeze = FreezeRecord::build(&net.spec, &net.bnd, &net.stream.clock);
    freeze.verify(&net.spec, &net.bnd, &net.stream.clock)?;
    if net.ev.freeze_fp != net.ev.registered_probe {
        return Err(TwErr::of_form(
            Form::FingerprintDrift,
            format!("探针漂移：{:016X} vs {:016X}", net.ev.freeze_fp, net.ev.registered_probe),
        ));
    }
    // ⑧ 指标
    let p50 = net.stream.hist.percentile(PCT_SCALED[0])?;
    let p90 = net.stream.hist.percentile(PCT_SCALED[1])?;
    let p99 = net.stream.hist.percentile(PCT_SCALED[2])?;
    let mut sat_marks: u64 = 0;
    let mut i: usize = 0;
    while i < net.stream.banks.len() {
        sat_marks = sat_marks.wrapping_add(net.stream.banks[i].sat_marks);
        i = i.wrapping_add(1);
    }
    let mem_peak: u64 = (mem_estimate()) as u64;
    let metrics = Metrics {
        sums: net.stream.level_sums(),
        accepted: net.stream.stat.accepted,
        rejected: net.stream.stat.rejected,
        bnd_pass: net.bnd.pass,
        p50,
        p90,
        p99,
        samples: net.stream.hist.samples,
        rms: net.stream.hist.rms(),
        mean: net.stream.hist.mean_scaled(),
        ring_pushed: net.ring.pushed,
        ring_dropped: net.ring.dropped,
        sat_marks,
        inject_hits: detects.hit_count(),
        net_hits: net_detect.hit_count(),
        border_violations: net.ev.border_violations,
        steps: net.stream.stat.steps,
        mem_peak,
    };
    // ⑨ 重放（不与 observe 互递归）
    let replay = replay(seed)?;
    let o = TwObserve {
        seed,
        ring: net.ring.clone(),
        net,
        recon,
        detects,
        net_detect,
        fsm,
        stability,
        freeze,
        metrics,
        replay,
    };
    // ⑩ 导出往返自检
    let _ = tlv_len_check(&o)?;
    if !o.replay_match() {
        return Err(err_frozen("replay", "match"));
    }
    Ok(o)
}
/// 峰值内存估算（字节，整数口径）。
pub fn mem_estimate() -> usize {
    let banks: usize = LEVEL_COUNT * (BANK_CAP * core::mem::size_of::<Cell>() + 64);
    let ring: usize = RING_CAP * core::mem::size_of::<RingEntry>() + 64;
    let cases: usize = BOUNDARY_CASES * (core::mem::size_of::<BoundaryCase>() + core::mem::size_of::<BoundaryResult>()) + 64;
    let hist: usize = HIST_BUCKETS * 4 + 128;
    banks + ring + cases + hist
}
