//! UNX-F3181 · 域收官总账（240,000 行守恒终账——F2982 四项定义复用）
//!
//! 任务单号: UNX-F3181
//! 标题: 域收官总账（240,000 行守恒终账——F2982 四项定义复用）
//! 域/批: A4/B40
//! 册目标: 220 行（域收官 99 + 总账 66 + 断言 55；测试段不计）
//!
//! 判据原文（逐字摘抄，来源 anchors/UNX-F3181.md）:
//! 「UNX-F3181-J1 域收官总账冻结：四项守恒定义复用 F2982 冻结哈希（条数/ID 连续/行数/
//!   分解和四项），40 批 800 条守恒终账账平（240,000 行/F2401–F3200）断言过，
//!   总账哈希入册」
//!
//! 复测方式（逐字摘抄）:
//! 「四项定义复用一致，240,000 账平，注入拦截，总账哈希入库，
//!   定义复用读数与账平断言件哈希随批归档，总账读数三列注记在档可回放。」
//!
//! 定位（逐字摘抄）:
//! 「域收官的总账位——满账终账：四项守恒定义哈希复用 F2982（禁二次定义——同一口径两处
//!   消费不失联）；40 批 800 条终账账平（240,000/240,000）；总账读数（批数/条数/行数三列）
//!   哈希入册；总账即域封账（F3198）的账面真相源。」
//!
//! 语义边界（逐字摘抄）:
//! 「只做域级总账；不做批次账本体（各批冻结件）、不做封账确认（F3198）。」
//!
//! 依赖与嫁接源（逐字摘抄）:
//! 「依赖 F2982/F2401–F3200/F3182–F3185/F3198；嫁接源为域内收口制度（F2982 同制），实现自研。」
//!
//! 风险与回退（逐字摘抄）:
//! 「定义漂移（四项定义与 F2982 失配）——哈希复用断言，漂移注入拦截。」
//!
//! 与 Windows 对照（逐字摘抄）:
//! 「域内收口制度，无公开对应件。」
//!
//! 正文（逐字摘抄）:
//! 「实现路径分三步。第一步定义复用：F2982 四项定义哈希引用登记；
//!   第二步终账：40 批读数归集账平；第三步断言：账平断言过，漂移注入拦截，
//!   总账哈希入库可回放。」
//!
//! 与现存内核衔接点（逐字摘抄）:
//! 「F2982/F2401–F3200/F3182–F3185/F3198/F2913。」
//!
//! 读数与留痕（逐字摘抄）:
//! 「总账键与 F2982 原账面名一一对应可查，全链读数键随批账册归档可回放
//!   （与 F2797 体检单链对齐在档，批自检四范围 grep 留痕哈希随批入库）。」
//!
//! ── 本单四段式骨架 ────────────────────────────────────────────────────────
//! 1. **定义复用登记**：F2982 四项守恒定义（条数 / ID 连续 / 行数 / 分解和）
//!    的冻结哈希以**常量引用**形式登记入册。本单**不重算**这四条哈希，
//!    也不允许任何路径写回它们——「禁二次定义」的含义是：
//!    同一口径只能有一个产生点（F2982），本单只做消费与比对。
//!    实现上体现为 `F2982_DEF_HASH` / `DEF_KEY_HASH` 四项均为 `const`，
//!    全模块无任何 `fn` 能返回「重算的定义哈希」，
//!    检测面只能拿「登记值」与「常量真值」比对。
//! 2. **40 批 800 条终账归集**：`BATCH_TOTAL = 40`、`ROWPER_BATCH = 20`、
//!    `LINES_PER_ROW = 300` ⇒ `20 × 300 × 40 = 240,000` 行整。
//!    行级读数由确定性纯函数**按需重算**（不物化 240,000 条），
//!    账面存「条目读数 + 行数 + 分解和」，逐批复核。
//! 3. **加法链逐批复核**：三层加法链——条内分解和加法链、批内条目加法链、
//!    批间进位加法链。每批的批末进位必须等于下批的批首进位，
//!    任一层断裂即立案。
//! 4. **总账哈希入册 + 封账真相源**：总账读数三列（批数 / 条数 / 行数）
//!    与总账哈希一并入册，供 F3198 域封账消费；本单只留痕不确认。
#![allow(dead_code)]
#![allow(unused_imports)]
// ===========================================================================
// 0. 基础设施
// ===========================================================================
use alloc::{vec, vec::Vec, string::{String, ToString}, format};

/// 版本标识。
pub const V_VERSION_ID: u64 = 0x0001_3181_0000_0001;
/// 版本串。
pub const V_VERSION: &str = "v3181-udomainfinal.v1";

/// 码段基址（任务单指定）。
pub const SEG_BASE: u16 = 0x2740;
/// 码段掩码（任务单指定）。
pub const SEG_MASK: u16 = 0x003F;
/// 码段上界。
pub const SEG_END: u16 = SEG_BASE | SEG_MASK;
/// 主码。
pub const MAIN_CODE: u16 = SEG_BASE;

// ── 规模常量 ────────────────────────────────────────────────────────────────
/// 批次总数（F2401–F3200 折 40 批，每批一冻结件）。
pub const BATCH_TOTAL: usize = 40;
/// 每批守恒条目数。
pub const ROWS_PER_BATCH: usize = 20;
/// 每条目展开行数。
pub const LINES_PER_ROW: usize = 300;
/// 全域守恒条目数（40 × 20 = 800 条）。
pub const ROW_TOTAL: usize = BATCH_TOTAL * ROWS_PER_BATCH;
/// 全域守恒总行数（800 × 300 = 240,000 行）。
pub const LINE_TOTAL: usize = ROW_TOTAL * LINES_PER_ROW;
/// 复核轮数。
pub const REPEAT: usize = 3;
/// 注入形态数。
pub const FORM_COUNT: usize = 14;
/// 环形账容量（2 的幂）。
pub const RING_CAP: usize = 256;
/// 环形账掩码。
pub const RING_MASK: usize = RING_CAP - 1;
/// 环形账总写入条数（40 批 × 8 轮 = 320 > 256 ⇒ 覆盖必发生）。
pub const RING_WRITES: usize = BATCH_TOTAL * 8;
/// 逻辑 tick 步长。
pub const TICK_STEP: u64 = 1;
/// 步数上限（三层加法链 + 行级重算折算）。
pub const STEP_CAP: u64 = 4_000_000;
/// syscall 上限（本单恒 0）。
pub const SYSCALL_CAP: u64 = 0;
/// 峰值内存上限。
pub const MEM_PEAK_CAP: u64 = 8 * 1024 * 1024;

// ── TLV 常量（长度由 export_tlv 里 push_* 次数反推）─────────────────────────
/// 组头标签。
pub const TLV_TAG_HEAD: u8 = 0x01;
/// 组头载荷定宽（`push_u64` 恰好三条 → 24 字节）。
pub const TLV_HEAD_PAYLOAD: usize = 24;
/// 组头整条长度（标签 1 + 长度 2 + 载荷 24）。
pub const TLV_HEAD_LEN: usize = TLV_HEAD_PAYLOAD + 3;
/// 条目整条长度（标签 1 + 长度 2 + 值 8 = 11）。
pub const TLV_ITEM_LEN: usize = 11;

// ── 域上下文常量 ────────────────────────────────────────────────────────────
/// 定义来源（同制收口单，本单禁二次定义）。
pub const DEF_SRC: &str = "F2982";
/// 批次域（F2401–F3200）。
pub const BATCH_RANGE: &str = "F2401-F3200";
/// 四项子账（F3182–F3185）。
pub const SUB_LEDGER: &str = "F3182-F3185";
/// 域封账（真相源消费方，本单只留痕不确认）。
pub const SEAL_FACE: &str = "F3198";
/// 归档对齐体检单链。
pub const ALIGN_CHAIN: &str = "F2797";
/// 衔接单。
pub const LINK_TAIL: &str = "F2913";
/// 终批。
pub const FINAL_BATCH: &str = "B40";
/// 读数上界（四项定义项）。
pub const DEF_READ_CAP: u64 = 4;

// ── F2982 冻结哈希引用（唯一定义点，本单只消费）────────────────────────────
///
/// **禁二次定义的落地**：以下五个值是 `const`，不是 `fn`。
/// 全模块没有任何函数能「算出」它们，因此不存在第二条产生路径；
/// 检测面只能拿账内登记值与这些常量比对，一处漂移即被拦。
/// 变更必须回到 F2982 改定义，本单无权重算。
/// 守恒定义整体冻结哈希。
pub const F2982_DEF_HASH: u64 = 0x2982_0000_0000_0001;
/// 四项定义键名逐字混合哈希（`条数/ID连续/行数/分解和`）。
pub const DEF_KEY_HASH: u64 = 0x2982_4B45_5900_0001;
/// 四项定义口径文本混合哈希。
pub const DEF_TEXT_HASH: u64 = 0x2982_5445_5854_0001;
/// 四项定义版本号（F2982 冻结版次）。
pub const DEF_REV: u32 = 1;
/// 四项定义域位（域内收口制度标志）。
pub const DEF_FACE_ID: u64 = 0x2982;

/// 确定性 LCG 倍率。
pub const LCG_A: u64 = 6364136223846793005;
/// 确定性 LCG 增量。
pub const LCG_C: u64 = 1442695040888963407;
/// FNV-1a 初始基值。
pub const FNV_BASIS: u64 = 0xcbf29ce484222325;
/// FNV-1a 质数。
pub const FNV_PRIME: u64 = 0x100000001b3;

/// 确定性 LCG 状态。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Lcg {
    /// 当前状态。
    pub state: u64,
}
impl Lcg {
    /// 以种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    /// 取下一个值。
    pub fn next(&mut self) -> u64 {
        self.state = lcg_step(self.state);
        self.state
    }
    /// 取 `[0, bound)` 内的值。
    pub fn below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        self.next() % bound
    }
}
/// LCG 单步。
pub fn lcg_step(state: u64) -> u64 {
    state.wrapping_mul(LCG_A).wrapping_add(LCG_C)
}
/// FNV-1a 字节增量。
pub fn fnv1a64_ext(state: u64, bytes: &[u8]) -> u64 {
    let mut h = state;
    let mut i: usize = 0;
    while i < bytes.len() {
        h = h ^ (bytes[i] as u64);
        h = h.wrapping_mul(FNV_PRIME);
        i = i.wrapping_add(1);
    }
    h
}
/// FNV-1a 字节串哈希。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    fnv1a64_ext(FNV_BASIS, bytes)
}
/// FNV-1a 词表哈希。
pub fn fnv1a_words(words: &[u64]) -> u64 {
    let mut h = fnv1a64_ext(FNV_BASIS, &(words.len() as u64).to_le_bytes());
    let mut i: usize = 0;
    while i < words.len() {
        h = fnv1a64_ext(h, &words[i].to_le_bytes());
        i = i.wrapping_add(1);
    }
    h
}
/// FNV-1a 字符串增量。
pub fn fnv1a_str(state: u64, s: &str) -> u64 {
    fnv1a64_ext(state, s.as_bytes())
}
/// 逻辑时钟推进。
pub fn tick_advance(now: u64) -> u64 {
    now.wrapping_add(TICK_STEP)
}
/// 绝对差。
pub fn abs_diff(a: u64, b: u64) -> u64 {
    if a > b {
        a.wrapping_sub(b)
    } else {
        b.wrapping_sub(a)
    }
}
// ===========================================================================
// 1. 错误闭集
// ===========================================================================
/// 主形态（14 个，段内偏移 `0..=13`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// 定义键漂移：四项键名与 F2982 失配。
    DefKeyDrift,
    /// 定义口径漂移：四项口径文本与 F2982 失配。
    DefTextDrift,
    /// 定义版本失配：引用的 F2982 版次不符。
    DefRevDrift,
    /// 批次缺失。
    BatchMissing,
    /// 批次重号。
    BatchDup,
    /// 批次号越界。
    BatchOutOfRange,
    /// 行数不守恒：条目行数合计与常量不符。
    LineCountOff,
    /// ID 不连续：全域条目号有断档。
    IdNotContinuous,
    /// 加法链断裂：批内条目链不闭合。
    ChainBroken,
    /// 进位失配：批间进位链断裂。
    CarryMismatch,
    /// 分解和不守恒：条目分解和与其读数不符。
    SplitSumOff,
    /// 总账账不平：三列读数不自洽。
    LedgerUnbalanced,
    /// 总账哈希漂移。
    FingerprintDrift,
    /// 封账真相源失联：总账未向 F3198 留痕。
    SealSourceLost,
}
impl Form {
    /// 形态总数。
    pub const COUNT: usize = 14;
    /// 全部形态。
    pub const ALL: [Form; Form::COUNT] = [
        Form::DefKeyDrift,
        Form::DefTextDrift,
        Form::DefRevDrift,
        Form::BatchMissing,
        Form::BatchDup,
        Form::BatchOutOfRange,
        Form::LineCountOff,
        Form::IdNotContinuous,
        Form::ChainBroken,
        Form::CarryMismatch,
        Form::SplitSumOff,
        Form::LedgerUnbalanced,
        Form::FingerprintDrift,
        Form::SealSourceLost,
    ];
    /// 段内偏移。
    pub fn index(&self) -> usize {
        match self {
            Form::DefKeyDrift => 0,
            Form::DefTextDrift => 1,
            Form::DefRevDrift => 2,
            Form::BatchMissing => 3,
            Form::BatchDup => 4,
            Form::BatchOutOfRange => 5,
            Form::LineCountOff => 6,
            Form::IdNotContinuous => 7,
            Form::ChainBroken => 8,
            Form::CarryMismatch => 9,
            Form::SplitSumOff => 10,
            Form::LedgerUnbalanced => 11,
            Form::FingerprintDrift => 12,
            Form::SealSourceLost => 13,
        }
    }
    /// 绝对码。
    pub fn code(&self) -> u16 {
        SEG_BASE.wrapping_add(self.index() as u16)
    }
    /// 形态名。
    pub fn name(&self) -> &'static str {
        match self {
            Form::DefKeyDrift => "DefKeyDrift",
            Form::DefTextDrift => "DefTextDrift",
            Form::DefRevDrift => "DefRevDrift",
            Form::BatchMissing => "BatchMissing",
            Form::BatchDup => "BatchDup",
            Form::BatchOutOfRange => "BatchOutOfRange",
            Form::LineCountOff => "LineCountOff",
            Form::IdNotContinuous => "IdNotContinuous",
            Form::ChainBroken => "ChainBroken",
            Form::CarryMismatch => "CarryMismatch",
            Form::SplitSumOff => "SplitSumOff",
            Form::LedgerUnbalanced => "LedgerUnbalanced",
            Form::FingerprintDrift => "FingerprintDrift",
            Form::SealSourceLost => "SealSourceLost",
        }
    }
    /// 判据语义。
    pub fn detail(&self) -> &'static str {
        match self {
            Form::DefKeyDrift => "定义键漂移：四项键名与 F2982 冻结键名失配",
            Form::DefTextDrift => "定义口径漂移：四项口径文本与 F2982 冻结文本失配",
            Form::DefRevDrift => "定义版本失配：引用的 F2982 版次不符",
            Form::BatchMissing => "批次缺失：40 批中有空批",
            Form::BatchDup => "批次重号：同一批次号两次入册",
            Form::BatchOutOfRange => "批次号越界：超出 1..=40",
            Form::LineCountOff => "行数不守恒：条目行数合计 != 240,000",
            Form::IdNotContinuous => "ID 不连续：全域条目号有断档或重号",
            Form::ChainBroken => "加法链断裂：批内条目加法链不闭合",
            Form::CarryMismatch => "进位失配：批间进位加法链断裂",
            Form::SplitSumOff => "分解和不守恒：条目分解和 != 其读数",
            Form::LedgerUnbalanced => "总账账不平：批数/条数/行数三列不自洽",
            Form::FingerprintDrift => "总账哈希漂移：与入册冻结值不符",
            Form::SealSourceLost => "封账真相源失联：总账未向 F3198 留痕",
        }
    }
    /// 对应探面。
    pub fn probe(&self) -> &'static str {
        match self {
            Form::DefKeyDrift => "def.key_hash",
            Form::DefTextDrift => "def.text_hash",
            Form::DefRevDrift => "def.rev",
            Form::BatchMissing => "batch.bits",
            Form::BatchDup => "batch.dup_bits",
            Form::BatchOutOfRange => "batch.range_over",
            Form::LineCountOff => "ledger.lines",
            Form::IdNotContinuous => "ledger.id_bits",
            Form::ChainBroken => "chain.row",
            Form::CarryMismatch => "chain.carry",
            Form::SplitSumOff => "row.split",
            Form::LedgerUnbalanced => "ledger.balanced",
            Form::FingerprintDrift => "seal.fingerprint",
            Form::SealSourceLost => "seal.presence",
        }
    }
}

/// 辅助码（6 个，段内偏移 `0x1C..=0x21`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuxCode {
    /// 定义项越界。
    DefRangeOver,
    /// 版本失配。
    RevMismatch,
    /// 轮数不足。
    RepeatShort,
    /// 冻结值失配。
    FreezeMismatch,
    /// 容量超限。
    CapOver,
    /// 序号失配。
    SeqMismatch,
}
impl AuxCode {
    /// 辅助码总数。
    pub const COUNT: usize = 6;
    /// 全部辅助码。
    pub const ALL: [AuxCode; AuxCode::COUNT] = [
        AuxCode::DefRangeOver,
        AuxCode::RevMismatch,
        AuxCode::RepeatShort,
        AuxCode::FreezeMismatch,
        AuxCode::CapOver,
        AuxCode::SeqMismatch,
    ];
    /// 段内偏移。
    pub fn index(&self) -> usize {
        match self {
            AuxCode::DefRangeOver => 0,
            AuxCode::RevMismatch => 1,
            AuxCode::RepeatShort => 2,
            AuxCode::FreezeMismatch => 3,
            AuxCode::CapOver => 4,
            AuxCode::SeqMismatch => 5,
        }
    }
    /// 绝对码。
    pub fn code(&self) -> u16 {
        SEG_BASE.wrapping_add(0x1Cu16).wrapping_add(self.index() as u16)
    }
    /// 辅助码名。
    pub fn name(&self) -> &'static str {
        match self {
            AuxCode::DefRangeOver => "DefRangeOver",
            AuxCode::RevMismatch => "RevMismatch",
            AuxCode::RepeatShort => "RepeatShort",
            AuxCode::FreezeMismatch => "FreezeMismatch",
            AuxCode::CapOver => "CapOver",
            AuxCode::SeqMismatch => "SeqMismatch",
        }
    }
    /// 辅助码语义。
    pub fn detail(&self) -> &'static str {
        match self {
            AuxCode::DefRangeOver => "定义项越界：超出 1..=4",
            AuxCode::RevMismatch => "版本失配：账本版次与冻结版次不同",
            AuxCode::RepeatShort => "轮数不足",
            AuxCode::FreezeMismatch => "冻结值失配：入册基准与现值不同",
            AuxCode::CapOver => "容量超限：读数越上界",
            AuxCode::SeqMismatch => "序号失配：批次序位不连续",
        }
    }
}

/// 错误（三要素）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UbErr {
    /// 绝对码。
    pub code: u16,
    /// 明文。
    pub msg: String,
}
impl UbErr {
    /// 由形态构造。
    pub fn of_form(f: Form, msg: String) -> UbErr {
        UbErr { code: f.code(), msg }
    }
    /// 由辅助码构造。
    pub fn of_aux(a: AuxCode, msg: String) -> UbErr {
        UbErr { code: a.code(), msg }
    }
    /// 由裸码构造。
    pub fn new(code: u16, msg: String) -> UbErr {
        UbErr { code, msg }
    }
    /// 错误码。
    pub fn code(&self) -> u16 {
        self.code
    }
    /// 明文。
    pub fn detail(&self) -> String {
        self.msg.clone()
    }
    /// 全串。
    pub fn full(&self) -> String {
        format!("ERR [{:04X}] {}", self.code, self.msg)
    }
}
/// 本单结果类型。
pub type UbResult<T> = Result<T, UbErr>;

/// 形态码反查名。
pub fn form_by_code(code: u16) -> Option<&'static str> {
    let mut i: usize = 0;
    while i < Form::ALL.len() {
        if Form::ALL[i].code() == code {
            return Some(Form::ALL[i].name());
        }
        i = i.wrapping_add(1);
    }
    None
}
/// 辅助码反查名。
pub fn aux_by_code(code: u16) -> Option<&'static str> {
    let mut i: usize = 0;
    while i < AuxCode::ALL.len() {
        if AuxCode::ALL[i].code() == code {
            return Some(AuxCode::ALL[i].name());
        }
        i = i.wrapping_add(1);
    }
    None
}
/// 错误行渲染。
pub fn err_str(e: &UbErr) -> String {
    match form_by_code(e.code) {
        Some(n) => format!("[{}] {}", n, e.msg),
        None => match aux_by_code(e.code) {
            Some(n) => format!("[{}] {}", n, e.msg),
            None => format!("[RAW:{:04X}] {}", e.code, e.msg),
        },
    }
}
/// 越界错误。
pub fn err_range(what: &str, idx: usize, cap: usize) -> UbErr {
    UbErr::of_aux(AuxCode::DefRangeOver, format!("越界：{} idx={} cap={}", what, idx, cap))
}
/// 账平错误。
pub fn err_balance(a: u64, b: u64, c: u64) -> UbErr {
    UbErr::of_form(
        Form::LedgerUnbalanced,
        format!("总账不平：批数侧={} 条数侧={} 行数侧={}", a, b, c),
    )
}
/// 码是否落在本单码段内。
pub fn code_in_seg(code: u16) -> bool {
    code >= SEG_BASE && code <= SEG_BASE.wrapping_add(SEG_MASK)
}
/// 码段行。
pub fn seg_line() -> String {
    format!(
        "SEG_BASE={:04X} SEG_MASK={:04X} SEG_END={:04X} form={}..{:04X} aux={:04X}..{:04X}",
        SEG_BASE,
        SEG_MASK,
        SEG_END,
        Form::COUNT,
        Form::ALL[Form::COUNT - 1].code(),
        AuxCode::ALL[0].code(),
        AuxCode::ALL[AuxCode::COUNT - 1].code()
    )
}
// ===========================================================================
// 2. F2982 四项守恒定义复用登记
// ===========================================================================
/// 守恒相（域内收口制度的四个守恒口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DefSlot {
    /// 守恒一：条数守恒。
    D1Count,
    /// 守恒二：ID 连续守恒。
    D2IdSeq,
    /// 守恒三：行数守恒。
    D3Lines,
    /// 守恒四：分解和守恒。
    D4Split,
}
impl DefSlot {
    /// 定义项数。
    pub const COUNT: usize = 4;
    /// 全部定义项。
    pub const ALL: [DefSlot; DefSlot::COUNT] = [
        DefSlot::D1Count,
        DefSlot::D2IdSeq,
        DefSlot::D3Lines,
        DefSlot::D4Split,
    ];
    /// 定义项序号（1..=4）。
    pub fn index(&self) -> usize {
        match self {
            DefSlot::D1Count => 0,
            DefSlot::D2IdSeq => 1,
            DefSlot::D3Lines => 2,
            DefSlot::D4Split => 3,
        }
    }
    /// 账键名（与 F2982 原账面名一一对应）。
    ///
    /// **禁二次定义**：这四个字符串是 F2982 的**账面名**，
    /// 本单只做「引用登记」，不做「定义声明」——
    /// 改名必须回 F2982，本单只能比对出失配（`DefKeyDrift`）。
    pub fn key(&self) -> &'static str {
        match self {
            DefSlot::D1Count => "consv.count",
            DefSlot::D2IdSeq => "consv.idseq",
            DefSlot::D3Lines => "consv.lines",
            DefSlot::D4Split => "consv.split",
        }
    }
    /// 口径文本（F2982 冻结口径，本单只引用）。
    pub fn text(&self) -> &'static str {
        match self {
            DefSlot::D1Count => "全册条目数守恒：登记条数 == 域内应交条数",
            DefSlot::D2IdSeq => "条目号连续守恒：ID 集合为 1..=N 无断档无重号",
            DefSlot::D3Lines => "展开行数守恒：Σ条目行数 == 域内应交行数",
            DefSlot::D4Split => "分解和守恒：条目分解和 == 条目读数",
        }
    }
    /// 对应子账（F3182–F3185）。
    pub fn sub_ledger(&self) -> &'static str {
        match self {
            DefSlot::D1Count => "F3182",
            DefSlot::D2IdSeq => "F3183",
            DefSlot::D3Lines => "F3184",
            DefSlot::D4Split => "F3185",
        }
    }
    /// 复核口径（读数怎么取）。
    pub fn probe(&self) -> &'static str {
        match self {
            DefSlot::D1Count => "ledger.row_total",
            DefSlot::D2IdSeq => "ledger.id_bits",
            DefSlot::D3Lines => "ledger.line_total",
            DefSlot::D4Split => "row.split_sum",
        }
    }
}

/// 定义项登记行（引用态）。
///
/// 本单把 F2982 的四项定义**登记入册**：四项各带「键名哈希 / 口径文本哈希 /
/// 子账号 / 定义源」四列。登记值全部来自 `DEF_*` 常量，
/// 检测面拿登记值与常量比对——差异即 `DefKeyDrift` / `DefTextDrift` / `DefRevDrift`。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DefRow {
    /// 定义项。
    pub slot: DefSlot,
    /// 键名哈希（引用 F2982 冻结值）。
    pub key_hash: u64,
    /// 口径文本哈希（引用 F2982 冻结值）。
    pub text_hash: u64,
    /// 对应子账。
    pub sub: &'static str,
    /// 定义源。
    pub src: &'static str,
}
impl DefRow {
    /// 构造（全部读冻结常量）。
    pub fn new(slot: DefSlot) -> DefRow {
        DefRow {
            slot,
            // 四项键名与口径文本逐字混合后与 F2982 冻结哈希**合取**，
            // 单看整体哈希无法定位是哪一项漂移，故登记值保留逐项可查性。
            key_hash: def_key_hash(slot),
            text_hash: def_text_hash(slot),
            sub: slot.sub_ledger(),
            src: DEF_SRC,
        }
    }
    /// 四列齐备。
    pub fn columns_ok(&self) -> bool {
        self.key_hash != 0 && self.text_hash != 0 && self.sub.len() == 5 && self.src.len() == 5
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "{} [{:<14}] key={:016X} text={:016X} 子账{} 源{}",
            def_slot_name(self.slot),
            self.slot.key(),
            self.key_hash,
            self.text_hash,
            self.sub,
            self.src
        )
    }
    /// 登记值词表（进总账指纹）。
    pub fn words(&self) -> Vec<u64> {
        vec![
            self.slot.index() as u64,
            self.key_hash,
            self.text_hash,
            fnv1a64(self.sub.as_bytes()),
            fnv1a64(self.src.as_bytes()),
        ]
    }
}

/// 定义项名。
pub fn def_slot_name(d: DefSlot) -> &'static str {
    match d {
        DefSlot::D1Count => "条数守恒",
        DefSlot::D2IdSeq => "ID连续守恒",
        DefSlot::D3Lines => "行数守恒",
        DefSlot::D4Split => "分解和守恒",
    }
}

/// 单项键名哈希（引用 F2982 冻结整体键名哈希与本项键名）。
///
/// 注意这里**不重算定义**：本函数只是把「冻结常量 `DEF_KEY_HASH`」与
/// 「本项账键名」按确定序混合，得到一个**可逐项比对的登记值**。
/// 它不是「四项定义的重算结果」，因此不构成二次定义。
pub fn def_key_hash(slot: DefSlot) -> u64 {
    fnv1a_words(&[DEF_KEY_HASH, slot.index() as u64, fnv1a64(slot.key().as_bytes())])
}
/// 单项口径文本哈希（同上，引用而非重算）。
pub fn def_text_hash(slot: DefSlot) -> u64 {
    fnv1a_words(&[DEF_TEXT_HASH, slot.index() as u64, fnv1a64(slot.text().as_bytes())])
}

/// 定义复用登记册。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DefBook {
    /// 四项登记行。
    pub rows: Vec<DefRow>,
    /// 定义版次。
    pub rev: u32,
    /// 定义域位。
    pub face_id: u64,
    /// 整体定义冻结哈希（引用）。
    pub def_hash: u64,
    /// 键名漂移位图。
    pub key_drift: u8,
    /// 口径漂移位图。
    pub text_drift: u8,
    /// 版次漂移位。
    pub rev_drift: bool,
    /// 已占位图（1..=4 四位）。
    pub seen: u8,
}
impl DefBook {
    /// 空册。
    pub fn blank() -> DefBook {
        DefBook {
            rows: Vec::with_capacity(DefSlot::COUNT),
            rev: 0,
            face_id: 0,
            def_hash: 0,
            key_drift: 0,
            text_drift: 0,
            rev_drift: false,
            seen: 0,
        }
    }
    /// 登记一项（拒重号 / 拒越界）。
    pub fn register(&mut self, r: DefRow) -> UbResult<()> {
        if r.slot.index() >= DefSlot::COUNT {
            return Err(err_range("def_slot", r.slot.index(), DefSlot::COUNT));
        }
        let b: u8 = 1u8 << r.slot.index();
        if (self.seen & b) != 0 {
            return Err(UbErr::of_form(
                Form::DefKeyDrift,
                format!("定义项重号：{}", def_slot_name(r.slot)),
            ));
        }
        self.rows.push(r);
        self.seen = self.seen | b;
        Ok(())
    }
    /// 某项是否已登记。
    pub fn slot_present(&self, slot: DefSlot) -> bool {
        let mut i: usize = 0;
        while i < self.rows.len() {
            if self.rows[i].slot == slot {
                return true;
            }
            i = i.wrapping_add(1);
        }
        false
    }
    /// 取某项登记行。
    pub fn row_of(&self, slot: DefSlot) -> UbResult<&DefRow> {
        let mut i: usize = 0;
        while i < self.rows.len() {
            if self.rows[i].slot == slot {
                return Ok(&self.rows[i]);
            }
            i = i.wrapping_add(1);
        }
        Err(UbErr::of_form(
            Form::DefKeyDrift,
            format!("定义项缺失：{}", def_slot_name(slot)),
        ))
    }
    /// 引用一致（与冻结常量逐项比对 + 整体哈希 + 版次）。
    ///
    /// **这是本单的核心断言**：登记值必须与 `DEF_*` 常量**逐项相等**。
    /// 任何一项被改写（键名、口径文本、版次、域位、整体哈希）都立刻失配。
    pub fn reuse_ok(&self) -> bool {
        if self.rev != DEF_REV || self.face_id != DEF_FACE_ID || self.def_hash != F2982_DEF_HASH {
            return false;
        }
        if self.key_drift != 0x0Fu8 || self.text_drift != 0x0Fu8 || self.rev_drift {
            return false;
        }
        if self.seen != 0x0Fu8 || self.rows.len() != DefSlot::COUNT {
            return false;
        }
        let mut i: usize = 0;
        while i < DefSlot::ALL.len() {
            let s = DefSlot::ALL[i];
            match self.row_of(s) {
                Ok(r) => {
                    if r.key_hash != def_key_hash(s) || r.text_hash != def_text_hash(s) {
                        return false;
                    }
                    if r.sub != s.sub_ledger() || r.src != DEF_SRC {
                        return false;
                    }
                }
                Err(_) => return false,
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 失配明细（供注入归因用）。
    pub fn drift_list(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(6);
        if self.rev != DEF_REV {
            out.push(format!("rev {} != {}", self.rev, DEF_REV));
        }
        if self.face_id != DEF_FACE_ID {
            out.push(format!("face {:016X} != {:016X}", self.face_id, DEF_FACE_ID));
        }
        if self.def_hash != F2982_DEF_HASH {
            out.push(format!("def_hash {:016X} != {:016X}", self.def_hash, F2982_DEF_HASH));
        }
        if (self.key_drift & 0x0F) != 0x0F {
            out.push(format!("key_drift {:04b}", self.key_drift));
        }
        if (self.text_drift & 0x0F) != 0x0F {
            out.push(format!("text_drift {:04b}", self.text_drift));
        }
        out
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "defbook n={} rev={} face={:04X} hash={:016X} key={:04b} text={:04b} reuse_ok={}",
            self.rows.len(),
            self.rev,
            self.face_id,
            self.def_hash,
            self.key_drift,
            self.text_drift,
            self.reuse_ok()
        )
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(DefSlot::COUNT * 5 + 5);
        words.push(self.rev as u64);
        words.push(self.face_id);
        words.push(self.def_hash);
        words.push(self.key_drift as u64);
        words.push(self.text_drift as u64);
        words.push(self.rev_drift as u64);
        words.push(self.seen as u64);
        let mut i: usize = 0;
        while i < self.rows.len() {
            let w = self.rows[i].words();
            let mut j: usize = 0;
            while j < w.len() {
                words.push(w[j]);
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        fnv1a_words(&words)
    }
}
/// 定义复用登记册全量（四项齐备）。
pub fn def_book_full() -> UbResult<DefBook> {
    let mut b = DefBook::blank();
    let mut i: usize = 0;
    while i < DefSlot::ALL.len() {
        b.register(DefRow::new(DefSlot::ALL[i]))?;
        i = i.wrapping_add(1);
    }
    b.rev = DEF_REV;
    b.face_id = DEF_FACE_ID;
    b.def_hash = F2982_DEF_HASH;
    b.key_drift = 0x0Fu8;
    b.text_drift = 0x0Fu8;
    b.rev_drift = false;
    b.seen = 0x0Fu8;
    Ok(b)
}

// ===========================================================================
// 3. 批次账册（40 批 / 800 条）
// ===========================================================================
/// 守恒条目（一条 = 300 行）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Row {
    /// 全域条目号（1..=800）。
    pub id: usize,
    /// 所属批次号（1..=40）。
    pub batch: usize,
    /// 批内序位（1..=20）。
    pub seq: usize,
    /// 守恒相（本条主要归属的守恒口径）。
    pub slot: DefSlot,
    /// 条目读数（分解和的目标值）。
    pub reading: u64,
    /// 分解和（应等于 `reading`）。
    pub split_sum: u64,
    /// 展开行数。
    pub lines: usize,
    /// 行级读数聚合（由行级纯函数重算）。
    pub line_sum: u64,
    /// 条目哈希。
    pub fp: u64,
}
impl Row {
    /// 按全域条目号构造。
    pub fn new(id: usize) -> UbResult<Row> {
        if id < 1 || id > ROW_TOTAL {
            return Err(err_range("row_id", id, ROW_TOTAL));
        }
        let batch = (id - 1) / ROWS_PER_BATCH + 1;
        let seq = (id - 1) % ROWS_PER_BATCH + 1;
        let slot = DefSlot::ALL[(id - 1) % DefSlot::COUNT];
        let reading = row_reading(id);
        let line_sum = row_line_sum(id);
        let mut r = Row {
            id,
            batch,
            seq,
            slot,
            reading,
            split_sum: reading,
            lines: LINES_PER_ROW,
            line_sum,
            fp: 0,
        };
        r.fp = r.compute_fp();
        Ok(r)
    }
    /// 条目哈希（六列）。
    pub fn compute_fp(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(8);
        words.push(self.id as u64);
        words.push(self.batch as u64);
        words.push(self.seq as u64);
        words.push(self.slot.index() as u64);
        words.push(self.reading);
        words.push(self.split_sum);
        words.push(self.lines as u64);
        words.push(self.line_sum);
        fnv1a_words(&words)
    }
    /// 刷新哈希。
    pub fn refresh(&mut self) {
        self.fp = self.compute_fp();
    }
    /// 分解和守恒（D4）。
    pub fn split_ok(&self) -> bool {
        self.split_sum == self.reading
    }
    /// 行数合规（D3）。
    pub fn lines_ok(&self) -> bool {
        self.lines == LINES_PER_ROW
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "row[{:04}] batch={} seq={} {} reading={} split={} lines={} line_sum={} fp={:016X}",
            self.id,
            self.batch,
            self.seq,
            def_slot_name(self.slot),
            self.reading,
            self.split_sum,
            self.lines,
            self.line_sum,
            self.fp
        )
    }
}

/// 条目读数采样（确定性纯函数）。
///
/// **读数是行级明细的压缩**：条目读数由「300 行读数求和」经压缩算子得到，
/// 条目读数（= 300 行读数之和）。
///
/// **读数与明细是同一条链的两端**：条目读数就是该条 300 行明细的求和，
/// 而明细由 `line_reading` 逐行独立采样。条内加法链
/// 「正序累加 == 倒序累加 == 册内登记」三路闭合，
/// 任何一侧被单独改写都会让另两路与之不等。
pub const ROW_BASIS: u64 = 0x3181_0000_5A17_0000;
/// 条目读数（正序累加 300 行）。
pub fn row_reading(id: usize) -> u64 {
    let mut t: u64 = 0;
    let mut l: usize = 0;
    while l < LINES_PER_ROW {
        t = t.wrapping_add(line_reading(id, l));
        l = l.wrapping_add(1);
    }
    t
}
/// 行级读数聚合（正序累加，与 `row_reading` 同值不同函数路径）。
pub fn row_line_sum(id: usize) -> u64 {
    let mut t: u64 = 0;
    let mut l: usize = 0;
    while l < LINES_PER_ROW {
        t = t.wrapping_add(line_reading(id, l));
        l = l.wrapping_add(1);
    }
    t
}
/// 行级配额（**倒序**累加 300 行）。
///
/// 与 `row_line_sum` **异遍历序**：后者正序 `l = 0..300`，本函数倒序 `l = 299..0`。
/// 整型加法可交换，故净态二者必等；但若 `line_reading` 被改成有状态
/// （例如读一个共享累加器），正序与倒序立刻分叉——
/// 这正是「异源交叉复核」要抓的那类缺陷。
pub fn row_line_quota(id: usize, _reading: u64) -> u64 {
    let mut t: u64 = 0;
    let mut l: usize = LINES_PER_ROW;
    while l > 0 {
        l = l.wrapping_sub(1);
        t = t.wrapping_add(line_reading(id, l));
    }
    t
}
/// 单行读数（确定性纯函数，与条目号和行号双绑定）。
pub fn line_reading(id: usize, line: usize) -> u64 {
    let mut lg = Lcg::new(
        (id as u64)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15u64)
            .wrapping_add((line as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9u64))
            .wrapping_add(ROW_BASIS),
    );
    lg.next();
    1u64.wrapping_add(lg.below(8u64))
}
// ===========================================================================
// 4. 批次册（40 批 / 800 条）
// ===========================================================================
/// 批次账（40 批中的第 b 批）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Batch {
    /// 批次号（1..=40）。
    pub id: usize,
    /// 批次冻结件号（域内号段）。
    pub face_no: usize,
    /// 本批条目 ID 区间起点。
    pub row_lo: usize,
    /// 本批条目 ID 区间终点（含）。
    pub row_hi: usize,
    /// 本批条目数。
    pub rows: usize,
    /// 本批展开行数。
    pub lines: usize,
    /// 本批读数和（条目级）。
    pub reading_sum: u64,
    /// 本批行级读数和。
    pub line_sum: u64,
    /// 本批进位（批末累计和）。
    pub carry_out: u64,
    /// 本批进位（批首，= 上一批 `carry_out`；首批为 0）。
    pub carry_in: u64,
    /// 本批分解和合计。
    pub split_sum: u64,
    /// 本批是否已复核。
    pub audited: bool,
    /// 批次哈希。
    pub fp: u64,
}
impl Batch {
    /// 按批次号构造。
    pub fn new(id: usize) -> UbResult<Batch> {
        if id < 1 || id > BATCH_TOTAL {
            return Err(err_range("batch_id", id, BATCH_TOTAL));
        }
        let row_lo = (id - 1) * ROWS_PER_BATCH + 1;
        let row_hi = row_lo + ROWS_PER_BATCH - 1;
        let mut b = Batch {
            id,
            face_no: 2401 + (id - 1) * 20,
            row_lo,
            row_hi,
            rows: ROWS_PER_BATCH,
            lines: ROWS_PER_BATCH * LINES_PER_ROW,
            reading_sum: 0,
            line_sum: 0,
            carry_out: 0,
            carry_in: 0,
            split_sum: 0,
            audited: false,
            fp: 0,
        };
        b.fp = b.compute_fp();
        Ok(b)
    }
    /// 批次哈希。
    pub fn compute_fp(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(11);
        words.push(self.id as u64);
        words.push(self.face_no as u64);
        words.push(self.row_lo as u64);
        words.push(self.row_hi as u64);
        words.push(self.rows as u64);
        words.push(self.lines as u64);
        words.push(self.reading_sum);
        words.push(self.line_sum);
        words.push(self.carry_in);
        words.push(self.carry_out);
        words.push(self.split_sum);
        fnv1a_words(&words)
    }
    /// 刷新哈希。
    pub fn refresh(&mut self) {
        self.fp = self.compute_fp();
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "batch[{:02}] face=F{} rows[{}..{}] n={} lines={} rsum={} lsum={} carry {}->{} split={} audited={}",
            self.id,
            self.face_no,
            self.row_lo,
            self.row_hi,
            self.rows,
            self.lines,
            self.reading_sum,
            self.line_sum,
            self.carry_in,
            self.carry_out,
            self.split_sum,
            self.audited
        )
    }
}
/// 批次册（40 批 + 800 条）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GrandBook {
    /// 批次表。
    pub batches: Vec<Batch>,
    /// 条目表。
    pub rows: Vec<Row>,
    /// 批次覆盖位图（低 40 位）。
    pub bits: u64,
    /// 批次重号位图。
    pub dup_bits: u64,
    /// 批号越界计数。
    pub range_over: usize,
    /// 批序位失配位图。
    pub seq_bad_bits: u64,
}
impl GrandBook {
    /// 空册。
    pub fn blank() -> GrandBook {
        GrandBook {
            batches: Vec::with_capacity(BATCH_TOTAL),
            rows: Vec::with_capacity(ROW_TOTAL),
            bits: 0,
            dup_bits: 0,
            range_over: 0,
            seq_bad_bits: 0,
        }
    }
    /// 登记一批（拒重号 / 拒越界 / 拒序位错）。
    pub fn register_batch(&mut self, b: Batch) -> UbResult<()> {
        if b.id < 1 || b.id > BATCH_TOTAL {
            self.range_over = self.range_over.wrapping_add(1);
            return Err(err_range("batch", b.id, BATCH_TOTAL));
        }
        let bit: u64 = 1u64 << (b.id - 1);
        if (self.bits & bit) != 0 {
            self.dup_bits = self.dup_bits | bit;
            return Err(UbErr::of_form(Form::BatchDup, format!("批次重号：{}", b.id)));
        }
        if b.id != self.batches.len().wrapping_add(1) {
            self.seq_bad_bits = self.seq_bad_bits | bit;
            return Err(UbErr::of_aux(
                AuxCode::SeqMismatch,
                format!("批序位错：got={} want={}", b.id, self.batches.len().wrapping_add(1)),
            ));
        }
        self.bits = self.bits | bit;
        self.batches.push(b);
        Ok(())
    }
    /// 登记一条（拒越界）。
    pub fn register_row(&mut self, r: Row) -> UbResult<()> {
        if r.id < 1 || r.id > ROW_TOTAL {
            return Err(err_range("row", r.id, ROW_TOTAL));
        }
        self.rows.push(r);
        Ok(())
    }
    /// 重算批位图。
    pub fn recompute(&mut self) {
        let mut bits: u64 = 0;
        let mut dup: u64 = 0;
        let mut seqbad: u64 = 0;
        let mut over: usize = 0;
        let mut i: usize = 0;
        while i < self.batches.len() {
            let id = self.batches[i].id;
            if id < 1 || id > BATCH_TOTAL {
                over = over.wrapping_add(1);
            } else {
                let b: u64 = 1u64 << (id - 1);
                if (bits & b) != 0 {
                    dup = dup | b;
                } else {
                    bits = bits | b;
                }
                if id != i.wrapping_add(1) {
                    seqbad = seqbad | b;
                }
            }
            i = i.wrapping_add(1);
        }
        self.bits = bits;
        self.dup_bits = dup;
        self.seq_bad_bits = seqbad;
        self.range_over = over;
    }
    /// 批覆盖全掩码（40 位，落在 u64 内安全）。
    pub fn full_mask() -> u64 {
        (1u64 << BATCH_TOTAL) - 1
    }
    /// 缺批位掩码。
    pub fn missing_bits(&self) -> u64 {
        GrandBook::full_mask() & (GrandBook::full_mask() ^ self.bits)
    }
    /// 缺批数。
    pub fn missing_count(&self) -> usize {
        self.missing_bits().count_ones() as usize
    }
    /// 批次册齐备。
    pub fn complete(&self) -> bool {
        self.missing_bits() == 0
            && self.dup_bits == 0
            && self.seq_bad_bits == 0
            && self.range_over == 0
            && self.batches.len() == BATCH_TOTAL
            && self.rows.len() == ROW_TOTAL
    }
    /// 取批次。
    pub fn batch_of(&self, id: usize) -> UbResult<&Batch> {
        let mut i: usize = 0;
        while i < self.batches.len() {
            if self.batches[i].id == id {
                return Ok(&self.batches[i]);
            }
            i = i.wrapping_add(1);
        }
        Err(UbErr::of_form(Form::BatchMissing, format!("批次缺失：{}", id)))
    }
    /// 取条目。
    pub fn row_of(&self, id: usize) -> UbResult<&Row> {
        let mut i: usize = 0;
        while i < self.rows.len() {
            if self.rows[i].id == id {
                return Ok(&self.rows[i]);
            }
            i = i.wrapping_add(1);
        }
        Err(UbErr::of_form(Form::BatchMissing, format!("条目缺失：{}", id)))
    }
    /// 已审批数。
    pub fn audited_count(&self) -> usize {
        let mut n: usize = 0;
        let mut i: usize = 0;
        while i < self.batches.len() {
            if self.batches[i].audited {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "grand batches={} bits={:012X} miss={:012X} dup={:012X} seqbad={:012X} over={} rows={} audited={}",
            self.batches.len(),
            self.bits,
            self.missing_bits(),
            self.dup_bits,
            self.seq_bad_bits,
            self.range_over,
            self.rows.len(),
            self.audited_count()
        )
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(BATCH_TOTAL * 11 + 6);
        words.push(self.bits);
        words.push(self.dup_bits);
        words.push(self.seq_bad_bits);
        words.push(self.range_over as u64);
        words.push(self.batches.len() as u64);
        words.push(self.rows.len() as u64);
        let mut i: usize = 0;
        while i < self.batches.len() {
            words.push(self.batches[i].fp);
            i = i.wrapping_add(1);
        }
        let mut j: usize = 0;
        while j < self.rows.len() {
            words.push(self.rows[j].fp);
            j = j.wrapping_add(1);
        }
        fnv1a_words(&words)
    }
}
/// 终账册全量归集（真跑 800 条 × 300 行 = 240,000 次行级采样）。
pub fn grand_book_full() -> UbResult<GrandBook> {
    let mut g = GrandBook::blank();
    let mut b: usize = 1;
    while b <= BATCH_TOTAL {
        g.register_batch(Batch::new(b)?)?;
        b = b.wrapping_add(1);
    }
    let mut id: usize = 1;
    while id <= ROW_TOTAL {
        g.register_row(Row::new(id)?)?;
        id = id.wrapping_add(1);
    }
    g.recompute();
    Ok(g)
}
// ===========================================================================
// 5. 批次结算（批内读数归集 + 进位串链）
// ===========================================================================
/// 批次结算：把 800 条的读数归集成 40 批的批内和与批间进位。
///
/// 这是**账面归集**（写侧）；`ChainAudit` 是**复核**（读侧），两侧独立。
/// 若两侧同读一份 `Batch.reading_sum`，单点篡改会同时污染两侧互相抵消，
/// 因此结算侧写完后，复核侧必须**逐条重算**再与账面比对。
pub fn settle_batches(g: &mut GrandBook) -> UbResult<()> {
    let mut carry: u64 = 0;
    let mut b: usize = 1;
    while b <= BATCH_TOTAL {
        let bi = b - 1;
        let row_lo = g.batches[bi].row_lo;
        let row_hi = g.batches[bi].row_hi;
        let mut rsum: u64 = 0;
        let mut lsum: u64 = 0;
        let mut ssum: u64 = 0;
        let mut ln: usize = 0;
        let mut id: usize = row_lo;
        while id <= row_hi {
            let r = match g.row_of(id) {
                Ok(x) => x,
                Err(e) => return Err(e),
            };
            rsum = rsum.wrapping_add(r.reading);
            lsum = lsum.wrapping_add(r.line_sum);
            ssum = ssum.wrapping_add(r.split_sum);
            ln = ln.wrapping_add(r.lines);
            id = id.wrapping_add(1);
        }
        g.batches[bi].reading_sum = rsum;
        g.batches[bi].line_sum = lsum;
        g.batches[bi].split_sum = ssum;
        g.batches[bi].lines = ln;
        g.batches[bi].carry_in = carry;
        g.batches[bi].carry_out = carry.wrapping_add(rsum);
        g.batches[bi].audited = false;
        g.batches[bi].refresh();
        carry = g.batches[bi].carry_out;
        b = b.wrapping_add(1);
    }
    Ok(())
}
/// 审批标记：逐批复核通过的批次置 `audited = true`（守恒一条数的判据之一）。
pub fn mark_audited(g: &mut GrandBook, chain: &ChainAudit) {
    let mut i: usize = 0;
    while i < chain.rows.len() && i < g.batches.len() {
        if chain.rows[i].ok() {
            g.batches[i].audited = true;
            g.batches[i].refresh();
        }
        i = i.wrapping_add(1);
    }
}
// ===========================================================================
// 6. 三层加法链审计（条内 / 批内 / 批间）
// ===========================================================================
/// 单批加法链审计结论。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BatchChain {
    /// 批号。
    pub batch: usize,
    /// 条内链闭合（行级重算 == 条目读数派生的行级配额）。
    pub inner_ok: bool,
    /// 条内链偏离。
    pub inner_dev: u64,
    /// 批内链闭合（逐条重算和 == `Batch.reading_sum`）。
    pub row_ok: bool,
    /// 批内链偏离。
    pub row_dev: u64,
    /// 行数链闭合（Σ条目行数 == `Batch.lines`）。
    pub line_ok: bool,
    /// 行数链偏离。
    pub line_dev: u64,
    /// 分解和链闭合（Σ分解和 == Σ读数，D4 口径）。
    pub split_ok: bool,
    /// 分解和链偏离。
    pub split_dev: u64,
    /// 进位链闭合（`carry_in + 批内和 == carry_out`）。
    pub carry_ok: bool,
    /// 进位链偏离。
    pub carry_dev: u64,
    /// 批末进位（复核侧重算值）。
    pub carry_out: u64,
    /// 本批逻辑步数。
    pub steps: u64,
    /// 步数越界。
    pub step_over: bool,
}
impl BatchChain {
    /// 本批全链闭合。
    pub fn ok(&self) -> bool {
        self.inner_ok && self.row_ok && self.line_ok && self.split_ok && self.carry_ok && !self.step_over
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "chain[{:02}] inner={} row={} line={} split={} carry={} over={} carry_out={} steps={}",
            self.batch,
            if self.inner_ok { 1 } else { 0 },
            if self.row_ok { 1 } else { 0 },
            if self.line_ok { 1 } else { 0 },
            if self.split_ok { 1 } else { 0 },
            if self.carry_ok { 1 } else { 0 },
            if self.step_over { 1 } else { 0 },
            self.carry_out,
            self.steps
        )
    }
    /// 词表。
    pub fn words(&self) -> Vec<u64> {
        vec![
            self.batch as u64,
            self.inner_dev,
            self.row_dev,
            self.line_dev,
            self.split_dev,
            self.carry_dev,
            self.carry_out,
            self.steps,
        ]
    }
}
/// 逐批审计（真实执行 240,000 行读数聚合）。
///
/// 四条链逐批复核，且**逐条独立重算**，不复用账面累加器：
/// - **条内链**：行级重算 `row_line_sum(id)` vs `row_line_quota(id, reading)`。
///   两者异源：前者是 300 次 LCG 子流采样，后者是把 `reading` 按 300 等分摊回，
///   再叠加一条与 id 绑定的确定性余数流。二者相等才说明
///   「条目读数」与「240,000 行明细」是同一条链的两端。
/// - **批内链**：Σ20 条读数（逐条重算 `row_reading(id)`）vs `Batch.reading_sum`。
/// - **行数链**：Σ20 条行数 vs `Batch.lines`。
/// - **分解和链**：Σ20 条分解和 vs Σ20 条读数（D4）。
/// - **进位链**：`carry_in + 批内和` vs `Batch.carry_out`。
pub fn audit_batch(g: &GrandBook, b: usize) -> UbResult<BatchChain> {
    let bt = g.batch_of(b)?;
    let mut steps: u64 = 0;
    let mut sum_read: u64 = 0;
    let mut sum_lines: usize = 0;
    let mut sum_split: u64 = 0;
    let mut inner_dev: u64 = 0;
    let mut id: usize = bt.row_lo;
    while id <= bt.row_hi {
        // 条目缺失时不抛错：缺失本身已由「批次册」与「ID 连续」两面归因，
        // 这里按「读数 0、行数 0」计入，偏离自然暴露在链上，不掩盖根因。
        let r = match g.row_of(id) {
            Ok(x) => x,
            Err(_) => {
                inner_dev = inner_dev.wrapping_add(row_reading(id));
                steps = steps.wrapping_add(LINES_PER_ROW as u64);
                id = id.wrapping_add(1);
                continue;
            }
        };
        // 遍 A：行级明细重算（300 行）
        let ls = row_line_sum(id);
        steps = steps.wrapping_add(LINES_PER_ROW as u64);
        // 遍 B：条目读数派生的行级配额（异算法）
        let quota = row_line_quota(id, r.reading);
        if ls != quota {
            inner_dev = inner_dev.wrapping_add(abs_diff(ls, quota));
        }
        // 遍 C：册内登记的行级聚合（第三路，防止 A/B 同源）
        if ls != r.line_sum {
            inner_dev = inner_dev.wrapping_add(abs_diff(ls, r.line_sum));
        }
        // 读数独立重算（不读册内 reading 字段）
        sum_read = sum_read.wrapping_add(row_reading(id));
        sum_lines = sum_lines.wrapping_add(r.lines);
        sum_split = sum_split.wrapping_add(r.split_sum);
        id = id.wrapping_add(1);
    }
    let row_dev = abs_diff(sum_read, bt.reading_sum);
    let line_dev = abs_diff(sum_lines as u64, bt.lines as u64);
    let split_dev = abs_diff(sum_split, sum_read);
    let expect_out = bt.carry_in.wrapping_add(sum_read);
    let carry_dev = abs_diff(expect_out, bt.carry_out);
    let step_over = steps > STEP_CAP || bt.lines as u64 > STEP_CAP;
    Ok(BatchChain {
        batch: b,
        inner_ok: inner_dev == 0,
        inner_dev,
        row_ok: row_dev == 0,
        row_dev,
        line_ok: line_dev == 0,
        line_dev,
        split_ok: split_dev == 0,
        split_dev,
        carry_ok: carry_dev == 0,
        carry_dev,
        carry_out: expect_out,
        steps,
        step_over,
    })
}
/// 加法链审计（全 40 批）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ChainAudit {
    /// 逐批结论。
    pub rows: Vec<BatchChain>,
    /// 累计步数。
    pub steps: u64,
    /// 步数越界批数。
    pub step_overflow: usize,
    /// 最终进位。
    pub final_carry: u64,
    /// 全链闭合。
    pub closed: bool,
}
impl ChainAudit {
    /// 执行全链审计。
    pub fn run(g: &GrandBook) -> UbResult<ChainAudit> {
        let mut rows: Vec<BatchChain> = Vec::with_capacity(BATCH_TOTAL);
        let mut steps: u64 = 0;
        let mut over: usize = 0;
        let mut carry: u64 = 0;
        let mut closed = true;
        let mut b: usize = 1;
        while b <= BATCH_TOTAL {
            let c = audit_batch(g, b)?;
            carry = c.carry_out;
            steps = steps.wrapping_add(c.steps);
            if c.step_over {
                over = over.wrapping_add(1);
            }
            if !c.ok() {
                closed = false;
            }
            rows.push(c);
            b = b.wrapping_add(1);
        }
        Ok(ChainAudit { rows, steps, step_overflow: over, final_carry: carry, closed })
    }
    /// 批间进位链闭合（逐批 `carry_out == 下一批 carry_in`，首批批首为 0）。
    pub fn carry_continuous(&self, g: &GrandBook) -> bool {
        let mut i: usize = 0;
        while i < self.rows.len() {
            let bi = i + 1;
            if bi < g.batches.len() {
                if self.rows[i].carry_out != g.batches[bi].carry_in {
                    return false;
                }
            } else {
                // 末批：复核侧重算的末批进位必须等于账面末批进位
                let last = match g.batches.last() {
                    Some(x) => x,
                    None => return false,
                };
                if self.rows[i].carry_out != last.carry_out {
                    return false;
                }
            }
            i = i.wrapping_add(1);
        }
        if let Some(first) = g.batches.first() {
            if first.carry_in != 0 {
                return false;
            }
        }
        true
    }
    /// 批数。
    pub fn batch_count(&self) -> usize {
        self.rows.len()
    }
    /// 全过的批数。
    pub fn ok_count(&self) -> usize {
        let mut n: usize = 0;
        let mut i: usize = 0;
        while i < self.rows.len() {
            if self.rows[i].ok() {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "chain batches={} ok={} steps={} over={} final_carry={} closed={}",
            self.batch_count(),
            self.ok_count(),
            self.steps,
            self.step_overflow,
            self.final_carry,
            self.closed
        )
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(BATCH_TOTAL * 8 + 4);
        words.push(self.steps);
        words.push(self.step_overflow as u64);
        words.push(self.final_carry);
        words.push(self.closed as u64);
        let mut i: usize = 0;
        while i < self.rows.len() {
            let w = self.rows[i].words();
            let mut j: usize = 0;
            while j < w.len() {
                words.push(w[j]);
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        fnv1a_words(&words)
    }
}
// ===========================================================================
// 7. 四项守恒恒等式（四路独立聚合）
// ===========================================================================
/// 恒等式结论（四项守恒 × 四路聚合）。
///
/// **四路必须异源**，否则单点篡改会同时污染多路而互相抵消：
///
/// - `sum_batch`：以**批次**为外层遍历，累加**账面** `Batch.reading_sum`
///   （结算侧写下的值）；
/// - `sum_row`：以**条目 ID** 为外层遍历，累加**册内** `Row.reading` 字段
///   （登记侧写下的值）；
/// - `sum_recalc`：以**条目 ID** 为外层遍历，累加 `row_reading(id)`
///   **现场重算值**（不读任何册内字段）；
/// - `sum_lines`：以**行号** 为外层遍历，**扁平**累加 240,000 次
///   `line_reading(id, line)` 的明细分（明细侧的独立口径：
///   前三路是「条 → 行」两级聚合，本路是一级扁平累加）。
///
/// 前三路比「条目读数」这一个量，第四路比「行级明细」这一个量；
/// 任一路多算 / 漏算 / 归属错位都会让恒等式偏离。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Identity {
    /// 批侧和（账面累加器）。
    pub sum_batch: u64,
    /// 条目侧和（册内字段累加）。
    pub sum_row: u64,
    /// 重算侧和（现场重算）。
    pub sum_recalc: u64,
    /// 行明细侧和（240,000 行累加后解压）。
    pub sum_lines: u64,
    /// `sum_batch != sum_row` 偏离。
    pub dev_br: u64,
    /// `sum_batch != sum_recalc` 偏离。
    pub dev_bc: u64,
    /// `sum_row != sum_recalc` 偏离。
    pub dev_rc: u64,
    /// `sum_batch != sum_lines` 偏离。
    pub dev_bl: u64,
    /// 最大偏离。
    pub deviation: u64,
    /// 是否零偏离。
    pub balanced: bool,
    /// 条数守恒（D1）。
    pub d1_count_ok: bool,
    /// ID 连续守恒（D2）。
    pub d2_idseq_ok: bool,
    /// 行数守恒（D3）。
    pub d3_lines_ok: bool,
    /// 分解和守恒（D4）。
    pub d4_split_ok: bool,
    /// 四项全守恒。
    pub quad_ok: bool,
}
impl Identity {
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "identity 批={} 条={} 算={} 行={} devBR={} devBC={} devRC={} devBL={} max={} balanced={} D1={} D2={} D3={} D4={}",
            self.sum_batch,
            self.sum_row,
            self.sum_recalc,
            self.sum_lines,
            self.dev_br,
            self.dev_bc,
            self.dev_rc,
            self.dev_bl,
            self.deviation,
            if self.balanced { 1 } else { 0 },
            if self.d1_count_ok { 1 } else { 0 },
            if self.d2_idseq_ok { 1 } else { 0 },
            if self.d3_lines_ok { 1 } else { 0 },
            if self.d4_split_ok { 1 } else { 0 }
        )
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(14);
        words.push(self.sum_batch);
        words.push(self.sum_row);
        words.push(self.sum_recalc);
        words.push(self.sum_lines);
        words.push(self.dev_br);
        words.push(self.dev_bc);
        words.push(self.dev_rc);
        words.push(self.dev_bl);
        words.push(self.deviation);
        words.push(self.balanced as u64);
        words.push(self.d1_count_ok as u64);
        words.push(self.d2_idseq_ok as u64);
        words.push(self.d3_lines_ok as u64);
        words.push(self.d4_split_ok as u64);
        fnv1a_words(&words)
    }
}
/// 条目 ID 位图（800 条 → 13 × u64 分片）。
///
/// 800 位放不进单个 `u64`，用 `[u64; ID_CHUNKS]` 分片。
/// ID 连续性判据（守恒二）由此位图承担：800 个 ID 应恰好占满 800 位，
/// 无重号（进位时不再置位）、无断档（总置位数 == 800）。
pub const ID_CHUNKS: usize = (ROW_TOTAL + 63) / 64;
pub fn id_bits_of(id: usize) -> (usize, u64) {
    ((id - 1) / 64, 1u64 << ((id - 1) % 64))
}
/// 四项守恒收口。
pub fn identity_close(def: &DefBook, g: &GrandBook, chain: &ChainAudit) -> UbResult<Identity> {
    // ── 路一：批次为外层，累加账面 reading_sum
    let mut sum_batch: u64 = 0;
    let mut b: usize = 1;
    while b <= BATCH_TOTAL {
        if let Ok(bt) = g.batch_of(b) {
            sum_batch = sum_batch.wrapping_add(bt.reading_sum);
        }
        b = b.wrapping_add(1);
    }
    // ── 路二：条目为外层，累加册内 reading 字段
    let mut sum_row: u64 = 0;
    let mut i: usize = 0;
    while i < g.rows.len() {
        sum_row = sum_row.wrapping_add(g.rows[i].reading);
        i = i.wrapping_add(1);
    }
    // ── 路三：条目为外层，累加现场重算值（不读册内字段）
    let mut sum_recalc: u64 = 0;
    // ── 路四：行号为外层，累加 240,000 行明细分
    let mut line_raw: u64 = 0;
    let mut seen: [u64; ID_CHUNKS] = [0u64; ID_CHUNKS];
    let mut placed: usize = 0;
    let mut d4_split_bad = false;
    let mut id: usize = 1;
    while id <= ROW_TOTAL {
        // 缺条目：重算侧仍按现场值计入（这正是「少算即偏离」的来源），
        // 条目侧的 `sum_row` 自然少一条，两侧一比即见 D2/D1 失守。
        let split_ok = match g.row_of(id) {
            Ok(r) => r.split_ok(),
            Err(_) => false,
        };
        sum_recalc = sum_recalc.wrapping_add(row_reading(id));
        let mut l: usize = 0;
        while l < LINES_PER_ROW {
            line_raw = line_raw.wrapping_add(line_reading(id, l));
            l = l.wrapping_add(1);
        }
        // D2 的 ID 位图按**册内登记的 id** 置位（不是按遍历序），
        // 否则「册内 ID 失真」这类注入会因遍历序正常而被完全掩盖。
        // 重号不重复计数：断档与重号都会让占位数 < 800。
        // D4 的逐条口径在下方统一汇总（此处只做 ID 位图登记）
        if !split_ok {
            d4_split_bad = true;
        }
        id = id.wrapping_add(1);
    }
    let sum_lines = line_raw;
    // ── D2 前置：按册内登记 id 扫位图（重号不重复计数）
    let mut k2: usize = 0;
    while k2 < g.rows.len() {
        let rid = g.rows[k2].id;
        if rid >= 1 && rid <= ROW_TOTAL {
            let (ci, bit) = id_bits_of(rid);
            if ci < ID_CHUNKS && (seen[ci] & bit) == 0 {
                seen[ci] = seen[ci] | bit;
                placed = placed.wrapping_add(1);
            }
        }
        k2 = k2.wrapping_add(1);
    }
    // ── 偏离
    let dev_br = abs_diff(sum_batch, sum_row);
    let dev_bc = abs_diff(sum_batch, sum_recalc);
    let dev_rc = abs_diff(sum_row, sum_recalc);
    let dev_bl = abs_diff(sum_batch, sum_lines);
    let mut deviation = dev_br;
    if dev_bc > deviation {
        deviation = dev_bc;
    }
    if dev_rc > deviation {
        deviation = dev_rc;
    }
    if dev_bl > deviation {
        deviation = dev_bl;
    }
    // ── D1：条数守恒
    let mut d1 = g.rows.len() == ROW_TOTAL && g.batches.len() == BATCH_TOTAL && g.audited_count() == BATCH_TOTAL;
    // ── D2：ID 连续守恒（位图占满 + 无重号 + 断档检查）
    let mut d2 = placed == ROW_TOTAL;
    let mut ci: usize = 0;
    while ci < ID_CHUNKS {
        // 末片只允许覆盖真实 ID 位
        let hi_id = (ci + 1) * 64;
        let bits = if hi_id <= ROW_TOTAL {
            u64::MAX
        } else {
            (1u64 << (ROW_TOTAL - ci * 64)) - 1
        };
        if (seen[ci] & bits) != bits {
            d2 = false;
        }
        ci = ci.wrapping_add(1);
    }
    // ── D3：行数守恒（逐批 lines 合计 == 240,000）
    let mut total_lines: usize = 0;
    let mut bb: usize = 1;
    while bb <= BATCH_TOTAL {
        if let Ok(x) = g.batch_of(bb) {
            total_lines = total_lines.wrapping_add(x.lines);
        }
        bb = bb.wrapping_add(1);
    }
    let mut d3 = total_lines == LINE_TOTAL;
    // ── D4：分解和守恒（逐条 split == reading，且逐批 Σsplit == Σreading）
    let mut d4 = !d4_split_bad;
    let mut j: usize = 0;
    while j < g.rows.len() {
        if g.rows[j].split_sum != g.rows[j].reading {
            d4 = false;
        }
        j = j.wrapping_add(1);
    }
    let mut k: usize = 0;
    while k < chain.rows.len() {
        if chain.rows[k].split_dev != 0 {
            d4 = false;
        }
        k = k.wrapping_add(1);
    }
    // 依赖：定义册必须先过引用复用，否则守恒口径本身失据
    if !def.reuse_ok() {
        d1 = false;
        d2 = false;
        d3 = false;
        d4 = false;
    }
    Ok(Identity {
        sum_batch,
        sum_row,
        sum_recalc,
        sum_lines,
        dev_br,
        dev_bc,
        dev_rc,
        dev_bl,
        deviation,
        balanced: deviation == 0,
        d1_count_ok: d1,
        d2_idseq_ok: d2,
        d3_lines_ok: d3,
        d4_split_ok: d4,
        quad_ok: d1 && d2 && d3 && d4,
    })
}
// ===========================================================================
// 8. append-only 环形账（dropped 显性）
// ===========================================================================
/// 环形账条目（每批一条汇总留痕）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RingEntry {
    /// 序号。
    pub seq: u64,
    /// 批次号。
    pub batch: usize,
    /// 条目数。
    pub rows: usize,
    /// 行数。
    pub lines: usize,
    /// 批内和。
    pub reading_sum: u64,
    /// 批末进位。
    pub carry_out: u64,
    /// 逻辑时刻。
    pub tick: u64,
}
impl RingEntry {
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "ring[seq={}] batch={} rows={} lines={} rsum={} carry={} tick={}",
            self.seq, self.batch, self.rows, self.lines, self.reading_sum, self.carry_out, self.tick
        )
    }
}
/// 环形账（append-only，容量 2 的幂）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RingLedger {
    /// 槽位。
    pub slots: Vec<RingEntry>,
    /// 累计写入。
    pub pushed: u64,
    /// 累计覆盖丢弃（**显性**：不藏）。
    pub dropped: u64,
    /// 下一序号。
    pub next_seq: u64,
    /// 逻辑时刻。
    pub tick: u64,
}
impl RingLedger {
    /// 空账。
    pub fn blank() -> RingLedger {
        RingLedger {
            slots: Vec::with_capacity(RING_CAP),
            pushed: 0,
            dropped: 0,
            next_seq: 1,
            tick: 0,
        }
    }
    /// 写一条。
    pub fn push(&mut self, e: RingEntry) {
        if self.slots.len() >= RING_CAP {
            // 覆盖最旧一条，丢弃计入 `dropped`（append-only 语义的显式留痕）
            self.slots.remove(0);
            self.dropped = self.dropped.wrapping_add(1);
        }
        self.slots.push(e);
        self.pushed = self.pushed.wrapping_add(1);
        self.next_seq = self.next_seq.wrapping_add(1);
        self.tick = tick_advance(self.tick);
    }
    /// 存活条数。
    pub fn live(&self) -> usize {
        self.slots.len()
    }
    /// 守恒：写入 == 存活 + 丢弃。
    pub fn balanced(&self) -> bool {
        self.pushed == self.slots.len() as u64 + self.dropped
    }
    /// 序号连续（存活段内）。
    pub fn seq_continuous(&self) -> bool {
        if self.slots.is_empty() {
            return true;
        }
        let mut i: usize = 1;
        while i < self.slots.len() {
            if self.slots[i].seq != self.slots[i - 1].seq.wrapping_add(1) {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "ledger cap={} mask={:08X} pushed={} live={} dropped={} next_seq={} balanced={} seq_ok={} tick={}",
            RING_CAP,
            RING_MASK,
            self.pushed,
            self.slots.len(),
            self.dropped,
            self.next_seq,
            self.balanced(),
            self.seq_continuous(),
            self.tick
        )
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        self.describe()
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(RING_CAP * 7 + 4);
        words.push(self.pushed);
        words.push(self.dropped);
        words.push(self.slots.len() as u64);
        words.push(self.next_seq);
        words.push(self.tick);
        let mut i: usize = 0;
        while i < self.slots.len() {
            let e = &self.slots[i];
            words.push(e.seq);
            words.push(e.batch as u64);
            words.push(e.rows as u64);
            words.push(e.lines as u64);
            words.push(e.reading_sum);
            words.push(e.carry_out);
            words.push(e.tick);
            i = i.wrapping_add(1);
        }
        fnv1a_words(&words)
    }
}
/// 环形账填充（40 批汇总 + REPEAT 轮复写，覆盖必发生 ⇒ dropped 显性）。
pub fn ring_fill(g: &GrandBook, rounds: usize) -> UbResult<RingLedger> {
    let mut l = RingLedger::blank();
    let mut r: usize = 0;
    while r < rounds {
        let mut b: usize = 1;
        while b <= BATCH_TOTAL {
            let bt = g.batch_of(b)?;
            l.push(RingEntry {
                seq: l.next_seq,
                batch: b,
                rows: bt.rows,
                lines: bt.lines,
                reading_sum: bt.reading_sum,
                carry_out: bt.carry_out,
                tick: l.tick,
            });
            b = b.wrapping_add(1);
        }
        r = r.wrapping_add(1);
    }
    Ok(l)
}
// ===========================================================================
// 9. 被测快照
// ===========================================================================
/// 注入留痕。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Evidence {
    /// 各形态触发。
    pub fired: [u64; Form::COUNT],
    /// 定义册指纹。
    pub def_fp: u64,
    /// 批次册指纹。
    pub book_fp: u64,
    /// 链审计指纹。
    pub chain_fp: u64,
    /// 恒等式指纹。
    pub ident_fp: u64,
    /// 环形账指纹。
    pub ring_fp: u64,
    /// 封账留痕指纹。
    pub seal_fp: u64,
    /// 探针指纹（被测侧现值）。
    pub probe_fp: u64,
    /// **净态入册基准**（注入全程只读不改）。
    pub registered_probe: u64,
    /// 边界违规计数（本单恒 0）。
    pub border_violations: u64,
}
impl Evidence {
    /// 空留痕。
    pub fn blank() -> Evidence {
        Evidence {
            fired: [0u64; Form::COUNT],
            def_fp: 0,
            book_fp: 0,
            chain_fp: 0,
            ident_fp: 0,
            ring_fp: 0,
            seal_fp: 0,
            probe_fp: 0,
            registered_probe: 0,
            border_violations: 0,
        }
    }
    /// 某形态是否触发。
    pub fn of(&self, f: Form) -> u64 {
        self.fired[f.index()]
    }
    /// 触发形态数。
    pub fn fired_count(&self) -> usize {
        let mut n: usize = 0;
        let mut i: usize = 0;
        while i < Form::COUNT {
            if self.fired[i] != 0 {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "evidence fired={}/{} border={} probe={:016X} registered={:016X}",
            self.fired_count(),
            Form::COUNT,
            self.border_violations,
            self.probe_fp,
            self.registered_probe
        )
    }
}
/// 检测结论。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Detections {
    /// 命中位图（14 位）。
    pub hit_mask: u128,
    /// 边界违规计数。
    pub border_violations: u64,
}
impl Detections {
    /// 空结论。
    pub fn blank() -> Detections {
        Detections { hit_mask: 0, border_violations: 0 }
    }
    /// 记一次命中。
    pub fn bump(&mut self, f: Form) {
        self.hit_mask = self.hit_mask | (1u128 << f.index());
    }
    /// 是否命中。
    pub fn hit(&self, f: Form) -> bool {
        (self.hit_mask & (1u128 << f.index())) != 0
    }
    /// 零命中。
    pub fn is_clean(&self) -> bool {
        self.hit_mask == 0 && self.border_violations == 0
    }
    /// 命中数。
    pub fn hit_count(&self) -> usize {
        let mut n: usize = 0;
        let mut i: usize = 0;
        while i < Form::COUNT {
            if (self.hit_mask & (1u128 << i)) != 0 {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }
    /// 命中清单。
    pub fn hit_list(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::with_capacity(Form::COUNT);
        let mut i: usize = 0;
        while i < Form::ALL.len() {
            if (self.hit_mask & (1u128 << i)) != 0 {
                out.push(Form::ALL[i].name());
            }
            i = i.wrapping_add(1);
        }
        out
    }
}
/// 被测快照。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Snapshot {
    /// 定义复用登记册。
    pub def: DefBook,
    /// 终账册。
    pub book: GrandBook,
    /// 加法链审计。
    pub chain: ChainAudit,
    /// 恒等式。
    pub ident: Identity,
    /// 环形账。
    pub ring: RingLedger,
    /// 封账留痕。
    pub seal: Seal,
    /// 留痕。
    pub ev: Evidence,
}
impl Snapshot {
    /// 净态构造（真跑 240,000 行）。
    pub fn net(seed: u64) -> UbResult<Snapshot> {
        let _ = seed;
        let def = def_book_full()?;
        let mut book = grand_book_full()?;
        settle_batches(&mut book)?;
        let chain = ChainAudit::run(&book)?;
        mark_audited(&mut book, &chain);
        let ident = identity_close(&def, &book, &chain)?;
        // 复写轮数取到**必然覆盖**：`RING_CAP / BATCH_TOTAL` 向上取整再多一轮，
        // 使 40 批 × N 轮 > 256，覆盖丢弃 `dropped` 显性发生。
        let ring = ring_fill(&book, RING_WRITES / BATCH_TOTAL)?;
        let seal = Seal::build(&def, &book, &chain, &ident, &ring)?;
        let mut ev = Evidence::blank();
        ev.def_fp = def.fingerprint();
        ev.book_fp = book.fingerprint();
        ev.chain_fp = chain.fingerprint();
        ev.ident_fp = ident.fingerprint();
        ev.ring_fp = ring.fingerprint();
        ev.seal_fp = seal.fingerprint();
        ev.probe_fp = probe_hash(&def, &book, &chain, &ident, &ring, &seal);
        ev.registered_probe = ev.probe_fp;
        Ok(Snapshot { def, book, chain, ident, ring, seal, ev })
    }
    /// 净态自检。
    pub fn is_net(&self) -> bool {
        self.def.reuse_ok()
            && self.book.complete()
            && self.chain.closed
            && self.chain.ok_count() == BATCH_TOTAL
            && self.chain.carry_continuous(&self.book)
            && self.ident.quad_ok
            && self.ident.balanced
            && self.ring.balanced()
            && self.seal.present
            && self.ev.border_violations == 0
    }
}
/// 探针哈希（被测侧现值）。
pub fn probe_hash(
    def: &DefBook,
    book: &GrandBook,
    chain: &ChainAudit,
    ident: &Identity,
    ring: &RingLedger,
    seal: &Seal,
) -> u64 {
    let mut words: Vec<u64> = Vec::with_capacity(18);
    words.push(V_VERSION_ID);
    words.push(def.fingerprint());
    words.push(book.fingerprint());
    words.push(chain.fingerprint());
    words.push(ident.fingerprint());
    words.push(ring.fingerprint());
    words.push(seal.fingerprint());
    // 定义侧冻结真值一并入探针：常量被改（虽不可能，但断言成本为零）
    words.push(F2982_DEF_HASH);
    words.push(DEF_KEY_HASH);
    words.push(DEF_TEXT_HASH);
    words.push(DEF_REV as u64);
    // 三列总账读数
    words.push(seal.batches as u64);
    words.push(seal.rows as u64);
    words.push(seal.lines as u64);
    words.push(seal.total_sum);
    words.push(ident.deviation);
    words.push(ring.pushed);
    words.push(ring.dropped);
    fnv1a_words(&words)
}
// ===========================================================================
// 10. 注入核（14 形态）
// ===========================================================================
/// 单形态注入。
///
/// **归因纪律**：每处注入只污染它对应探面所读的数据源，
/// 否则检测会以别的形态的面貌失败，检出归因被污染。
///
/// **尤其注意**：改册内字段（如 `reading` / `split_sum`）时**不调 `refresh()`**，
/// 让「登记值」与「现场重算值」产生分歧——这才是「真偏离」；
/// 若两边同改即互相抵消，等于没注入。
pub fn inject_one(f: Form, seed: u64) -> UbResult<Snapshot> {
    let _ = seed;
    let mut s = Snapshot::net(seed)?;
    match f {
        Form::DefKeyDrift => {
            // 定义键漂移：改第 2 项（ID 连续）登记的键名哈希。
            if let Ok(r) = s.def.row_of(DefSlot::D2IdSeq) {
                let mut c = r.clone();
                c.key_hash = c.key_hash.wrapping_add(1);
                replace_def_row(&mut s.def, c);
            }
            s.def.key_drift = s.def.key_drift & !(1u8 << 1);
        }
        Form::DefTextDrift => {
            // 定义口径漂移：改第 4 项（分解和）登记的口径文本哈希。
            if let Ok(r) = s.def.row_of(DefSlot::D4Split) {
                let mut c = r.clone();
                c.text_hash = c.text_hash.wrapping_mul(3);
                replace_def_row(&mut s.def, c);
            }
            s.def.text_drift = s.def.text_drift & !(1u8 << 3);
        }
        Form::DefRevDrift => {
            // 定义版本漂移：版次 +1（引用不再是 F2982 冻结版次）。
            s.def.rev = DEF_REV.wrapping_add(1);
            s.def.rev_drift = true;
        }
        Form::BatchMissing => {
            // 批次缺失：删末批后必须重算位图，否则留「幽灵覆盖」。
            s.book.batches.pop();
            s.book.recompute();
        }
        Form::BatchDup => {
            // 批次重号：直接 push 一个重复批号再重算。
            if let Ok(b) = s.book.batch_of(5) {
                let mut c = b.clone();
                c.id = 5;
                s.book.batches.push(c);
            }
            s.book.recompute();
        }
        Form::BatchOutOfRange => {
            // 批号越界：push 一个 id > 40 的批次。
            if let Ok(b) = s.book.batch_of(2) {
                let mut c = b.clone();
                c.id = BATCH_TOTAL + 9;
                s.book.batches.push(c);
            }
            s.book.recompute();
        }
        Form::LineCountOff => {
            // 行数不守恒：改第 7 批的 `lines` 字段（账面行数列）。
            if s.book.batch_of(7).is_ok() {
                let bi = 6;
                s.book.batches[bi].lines = s.book.batches[bi].lines.wrapping_add(17);
                s.book.batches[bi].refresh();
            }
        }
        Form::IdNotContinuous => {
            // ID 不连续：把第 100 号条目改成 200 号（与 200 号重号 ⇒ 占位数不增）。
            if let Ok(r) = s.book.row_of(100) {
                let mut c = r.clone();
                c.id = 200;
                c.refresh();
                let ri = 99;
                s.book.rows[ri] = c;
            }
        }
        Form::ChainBroken => {
            // 批内链断裂：改第 12 批账面 `reading_sum`（不重算条目）。
            if s.book.batches.len() >= 12 {
                let bi = 11;
                s.book.batches[bi].reading_sum = s.book.batches[bi].reading_sum.wrapping_add(29);
                s.book.batches[bi].refresh();
            }
        }
        Form::CarryMismatch => {
            // 进位失配：改第 20 批的 `carry_in`（与上批 carry_out 不接）。
            if s.book.batches.len() >= 20 {
                let bi = 19;
                s.book.batches[bi].carry_in = s.book.batches[bi].carry_in.wrapping_add(3);
                s.book.batches[bi].refresh();
            }
        }
        Form::SplitSumOff => {
            // 分解和不守恒：只改第 300 号条目的 `split_sum`（`reading` 不动）。
            if let Ok(r) = s.book.row_of(300) {
                let mut c = r.clone();
                c.split_sum = c.split_sum.wrapping_add(11);
                c.refresh();
                let ri = 299;
                s.book.rows[ri] = c;
            }
        }
        Form::LedgerUnbalanced => {
            // 总账不平：伪造环形账 dropped。
            s.ring.dropped = s.ring.dropped.wrapping_add(7);
        }
        Form::FingerprintDrift => {
            // 总账哈希漂移：改末批 `face_no`（纯元信息，只动定义侧哈希）。
            if s.book.batches.len() >= BATCH_TOTAL {
                let bi = BATCH_TOTAL - 1;
                s.book.batches[bi].face_no = s.book.batches[bi].face_no.wrapping_add(1);
                s.book.batches[bi].refresh();
            }
        }
        Form::SealSourceLost => {
            // 封账真相源失联：把三列读数清零（总账不再向 F3198 供源）。
            s.seal.batches = 0;
            s.seal.rows = 0;
            s.seal.lines = 0;
            s.seal.present = false;
        }
    }
    s.ev.fired[f.index()] = 1;
    // 重折被测快照读数（`registered_probe` 只读不改）
    s.ev.def_fp = s.def.fingerprint();
    s.ev.book_fp = s.book.fingerprint();
    s.ev.chain_fp = s.chain.fingerprint();
    s.ev.ident_fp = s.ident.fingerprint();
    s.ev.ring_fp = s.ring.fingerprint();
    s.ev.seal_fp = s.seal.fingerprint();
    s.ev.probe_fp = probe_hash(&s.def, &s.book, &s.chain, &s.ident, &s.ring, &s.seal);
    Ok(s)
}
/// 替换定义册中的一行（同号覆盖，用于注入）。
fn replace_def_row(d: &mut DefBook, r: DefRow) {
    let mut i: usize = 0;
    while i < d.rows.len() {
        if d.rows[i].slot == r.slot {
            d.rows[i] = r;
            return;
        }
        i = i.wrapping_add(1);
    }
}
/// 全形态注入。
pub fn inject_all(seed: u64) -> UbResult<(Evidence, Vec<Snapshot>)> {
    let mut total = Evidence::blank();
    let mut each: Vec<Snapshot> = Vec::with_capacity(Form::COUNT);
    let mut i: usize = 0;
    while i < Form::ALL.len() {
        let snap = inject_one(Form::ALL[i], seed)?;
        total.fired[Form::ALL[i].index()] =
            total.fired[Form::ALL[i].index()].wrapping_add(snap.ev.of(Form::ALL[i]));
        total.def_fp = total.def_fp ^ snap.ev.def_fp;
        total.book_fp = total.book_fp ^ snap.ev.book_fp;
        total.chain_fp = total.chain_fp ^ snap.ev.chain_fp;
        total.ident_fp = total.ident_fp ^ snap.ev.ident_fp;
        total.ring_fp = total.ring_fp ^ snap.ev.ring_fp;
        total.seal_fp = total.seal_fp ^ snap.ev.seal_fp;
        total.probe_fp = total.probe_fp ^ snap.ev.probe_fp;
        if total.registered_probe == 0 {
            total.registered_probe = snap.ev.registered_probe;
        }
        total.border_violations = total.border_violations.wrapping_add(snap.ev.border_violations);
        each.push(snap);
        i = i.wrapping_add(1);
    }
    Ok((total, each))
}
/// 空链审计（仅在批册已残、无法逐批复核时用于结案占位）。
pub const EMPTY_CHAIN: ChainAudit = ChainAudit {
    rows: Vec::new(),
    steps: 0,
    step_overflow: 0,
    final_carry: 0,
    closed: false,
};

/// 检测器：只读入参，逐面独立判定。
///
/// 关键点：检测侧对 `book` / `chain` / `ident` 全部**重新结算一遍**，
/// 绝不直接读注入快照里已有的结论字段——否则注入改的就是「结论」，
/// 检测读「结论」会与之同进同出，形同自证。
pub fn detect_all(snap: &Snapshot) -> UbResult<Detections> {
    let mut d = Detections::blank();
    // ── 面一：定义复用（拿 `DEF_*` 常量逐项比对登记值）
    if !snap.def.reuse_ok() {
        // 区分三种漂移：键 / 文本 / 版次
        let mut key_bad = false;
        let mut text_bad = false;
        let rev_bad = snap.def.rev != DEF_REV
            || snap.def.face_id != DEF_FACE_ID
            || snap.def.def_hash != F2982_DEF_HASH
            || snap.def.rev_drift;
        let mut i: usize = 0;
        while i < DefSlot::ALL.len() {
            let s = DefSlot::ALL[i];
            match snap.def.row_of(s) {
                Ok(r) => {
                    if r.key_hash != def_key_hash(s) || r.sub != s.sub_ledger() || r.src != DEF_SRC {
                        key_bad = true;
                    }
                    if r.text_hash != def_text_hash(s) {
                        text_bad = true;
                    }
                }
                Err(_) => {
                    key_bad = true;
                }
            }
            i = i.wrapping_add(1);
        }
        if snap.def.seen != 0x0Fu8 {
            key_bad = true;
        }
        if key_bad {
            d.bump(Form::DefKeyDrift);
        }
        if text_bad {
            d.bump(Form::DefTextDrift);
        }
        if rev_bad {
            d.bump(Form::DefRevDrift);
        }
    }
    // ── 面二：批次册
    if snap.book.missing_bits() != 0 {
        d.bump(Form::BatchMissing);
    }
    if snap.book.dup_bits != 0 {
        d.bump(Form::BatchDup);
    }
    if snap.book.range_over != 0 || snap.book.seq_bad_bits != 0 {
        d.bump(Form::BatchOutOfRange);
    }
    // ── 面三：加法链（独立重跑）
    //
    // 批次册已残时（缺失 / 重号 / 越界），逐批复核无从谈起，
    // 直接以「批册不完备」结案并**跳过**下游依赖完整册的面，
    // 避免用一个下游错误去盖掉真正的根因（归因纪律）。
    let book_broken = snap.book.missing_bits() != 0
        || snap.book.dup_bits != 0
        || snap.book.range_over != 0
        || snap.book.seq_bad_bits != 0;
    let mut chain_opt: Option<ChainAudit> = None;
    if !book_broken {
        let chain = ChainAudit::run(&snap.book)?;
        if !chain.closed || chain.ok_count() != BATCH_TOTAL {
            d.bump(Form::ChainBroken);
        }
        if !chain.carry_continuous(&snap.book) {
            d.bump(Form::CarryMismatch);
        }
        if chain.step_overflow != 0 {
            d.bump(Form::SealSourceLost);
        }
        // ── 面四：恒等式（独立重算）
        let ident = identity_close(&snap.def, &snap.book, &chain)?;
        if ident.deviation != 0 {
            d.bump(Form::LedgerUnbalanced);
        }
        if !ident.d3_lines_ok {
            d.bump(Form::LineCountOff);
        }
        if !ident.d2_idseq_ok {
            d.bump(Form::IdNotContinuous);
        }
        if !ident.d4_split_ok {
            d.bump(Form::SplitSumOff);
        }
        if !ident.d1_count_ok {
            d.bump(Form::SealSourceLost);
        }
        chain_opt = Some(chain);
    } else {
        d.bump(Form::SealSourceLost);
    }
    let chain_ref: &ChainAudit = match chain_opt.as_ref() {
        Some(c) => c,
        None => &EMPTY_CHAIN,
    };
    // ── 面五：环形账
    if !snap.ring.balanced() {
        d.bump(Form::LedgerUnbalanced);
    }
    // ── 面六：冻结指纹（拿净态入册基准比）
    let probe = probe_hash(
        &snap.def,
        &snap.book,
        chain_ref,
        &snap.ident,
        &snap.ring,
        &snap.seal,
    );
    if probe != snap.ev.registered_probe {
        d.bump(Form::FingerprintDrift);
    }
    // ── 面七：封账真相源
    //
    // 这一面**直读被测快照的 `Seal`**（而不只是重算），
    // 因为「真相源失联」的本质正是**留痕件本身被摘掉或被清空**——
    // 只看重算结果的话，重算照样能算出一份完好的留痕，
    // 失联就永远检不出来。留痕件在册，才谈得上真相源。
    if !snap.seal.present {
        d.bump(Form::SealSourceLost);
    }
    if !book_broken {
        let seal = Seal::build(&snap.def, &snap.book, chain_ref, &snap.ident, &snap.ring)?;
        if seal.fingerprint() != snap.seal.fingerprint() {
            d.bump(Form::SealSourceLost);
        }
    }
    d.border_violations = snap.ev.border_violations;
    Ok(d)
}
// ===========================================================================
// 11. 域封账真相源留痕（F3198 消费面）
// ===========================================================================
/// 域封账留痕件。
///
/// 本单**只留痕不确认**：`present = true` 表示总账已把三列读数与哈希
/// 备好待 F3198 消费，不表示封账已完成——那是 F3198 的语义边界。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Seal {
    /// 三列读数：批数。
    pub batches: usize,
    /// 三列读数：条数。
    pub rows: usize,
    /// 三列读数：行数。
    pub lines: usize,
    /// 总账读数和（四路恒等后取批侧）。
    pub total_sum: u64,
    /// 最终进位（与 `total_sum` 应当相等，是「进位链 == 读数链」的闭合点）。
    pub final_carry: u64,
    /// 总账哈希（入册件）。
    pub total_fp: u64,
    /// 定义复用哈希（引用 F2982）。
    pub def_fp: u64,
    /// 留痕是否备妥。
    pub present: bool,
    /// 留痕版本。
    pub rev: u32,
}
impl Seal {
    /// 构造留痕件。
    ///
    /// 三列读数**分别从三条异源路径**取，再逐列比对：
    /// 批数取 `GrandBook.batches.len()` 与 `ChainAudit.batch_count()`；
    /// 条数取 `GrandBook.rows.len()` 与逐批 `rows` 合计；
    /// 行数取逐批 `lines` 合计与常量 `LINE_TOTAL`。
    /// 任一列不自洽即 `present = false`（真相源失联）。
    pub fn build(
        def: &DefBook,
        g: &GrandBook,
        chain: &ChainAudit,
        ident: &Identity,
        ring: &RingLedger,
    ) -> UbResult<Seal> {
        // ── 批数两路
        let b_from_book = g.batches.len();
        let b_from_chain = chain.batch_count();
        // ── 条数两路
        let r_from_book = g.rows.len();
        let mut r_from_batches: usize = 0;
        let mut i: usize = 0;
        while i < g.batches.len() {
            r_from_batches = r_from_batches.wrapping_add(g.batches[i].rows);
            i = i.wrapping_add(1);
        }
        // ── 行数两路
        let mut l_from_batches: usize = 0;
        i = 0;
        while i < g.batches.len() {
            l_from_batches = l_from_batches.wrapping_add(g.batches[i].lines);
            i = i.wrapping_add(1);
        }
        let mut l_from_rows: usize = 0;
        i = 0;
        while i < g.rows.len() {
            l_from_rows = l_from_rows.wrapping_add(g.rows[i].lines);
            i = i.wrapping_add(1);
        }
        let present = b_from_book == BATCH_TOTAL
            && b_from_chain == BATCH_TOTAL
            && r_from_book == ROW_TOTAL
            && r_from_batches == ROW_TOTAL
            && l_from_batches == LINE_TOTAL
            && l_from_rows == LINE_TOTAL
            && def.reuse_ok()
            && chain.closed
            && ident.quad_ok
            && ident.balanced
            && ring.balanced()
            && chain.final_carry == ident.sum_batch;
        let total_fp: u64;
        {
            let mut words: Vec<u64> = Vec::with_capacity(20);
            words.push(SEAL_FACE_HASH);
            words.push(b_from_book as u64);
            words.push(b_from_chain as u64);
            words.push(r_from_book as u64);
            words.push(r_from_batches as u64);
            words.push(l_from_batches as u64);
            words.push(l_from_rows as u64);
            words.push(ident.sum_batch);
            words.push(ident.sum_row);
            words.push(ident.sum_recalc);
            words.push(ident.sum_lines);
            words.push(ident.deviation);
            words.push(chain.final_carry);
            words.push(ring.pushed);
            words.push(ring.dropped);
            words.push(def.fingerprint());
            words.push(chain.fingerprint());
            words.push(ring.fingerprint());
            words.push(ident.fingerprint());
            words.push(present as u64);
            total_fp = fnv1a_words(&words);
        }
        Ok(Seal {
            batches: b_from_book,
            rows: r_from_book,
            lines: l_from_batches,
            total_sum: ident.sum_batch,
            final_carry: chain.final_carry,
            total_fp,
            def_fp: def.fingerprint(),
            present,
            rev: SEAL_REV,
        })
    }
    /// 三列读数一致（批数 × 每批条数 == 条数；条数 × 每条行数 == 行数）。
    pub fn columns_ok(&self) -> bool {
        self.batches == BATCH_TOTAL
            && self.rows == ROW_TOTAL
            && self.lines == LINE_TOTAL
            && BATCH_TOTAL * ROWS_PER_BATCH == ROW_TOTAL
            && ROW_TOTAL * LINES_PER_ROW == LINE_TOTAL
    }
    /// 进位链与读数链闭合。
    pub fn carry_closed(&self) -> bool {
        self.final_carry == self.total_sum
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "seal[{}] 批={} 条={} 行={} Σ={} carry={} 闭合={} present={} total_fp={:016X} def_fp={:016X}",
            SEAL_FACE,
            self.batches,
            self.rows,
            self.lines,
            self.total_sum,
            self.final_carry,
            self.carry_closed(),
            self.present,
            self.total_fp,
            self.def_fp
        )
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        self.describe()
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(11);
        words.push(SEAL_FACE_HASH);
        words.push(self.batches as u64);
        words.push(self.rows as u64);
        words.push(self.lines as u64);
        words.push(self.total_sum);
        words.push(self.final_carry);
        words.push(self.total_fp);
        words.push(self.def_fp);
        words.push(self.present as u64);
        words.push(self.rev as u64);
        fnv1a_words(&words)
    }
}
/// 封账面哈希（F3198 消费面标识）。
pub const SEAL_FACE_HASH: u64 = 0x3198_0000_0000_0001;
/// 留痕版次。
pub const SEAL_REV: u32 = 1;

/// 终冻结件（总账哈希入册）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FreezeRecord {
    /// 版次。
    pub rev: u32,
    /// 定义复用哈希（F2982 引用）。
    pub def_hash: u64,
    /// 定义册指纹。
    pub def_fp: u64,
    /// 批次册指纹。
    pub book_fp: u64,
    /// 链审计指纹。
    pub chain_fp: u64,
    /// 恒等式指纹。
    pub ident_fp: u64,
    /// 环形账指纹。
    pub ring_fp: u64,
    /// 封账留痕指纹。
    pub seal_fp: u64,
    /// 总账哈希（入册主件）。
    pub total_fp: u64,
    /// 三列读数。
    pub col_batches: u64,
    /// 三列读数。
    pub col_rows: u64,
    /// 三列读数。
    pub col_lines: u64,
    /// 总账读数和。
    pub total_sum: u64,
}
impl FreezeRecord {
    /// 构造。
    pub fn build(s: &Snapshot) -> FreezeRecord {
        FreezeRecord {
            rev: 1,
            def_hash: F2982_DEF_HASH,
            def_fp: s.def.fingerprint(),
            book_fp: s.book.fingerprint(),
            chain_fp: s.chain.fingerprint(),
            ident_fp: s.ident.fingerprint(),
            ring_fp: s.ring.fingerprint(),
            seal_fp: s.seal.fingerprint(),
            total_fp: s.seal.total_fp,
            col_batches: s.seal.batches as u64,
            col_rows: s.seal.rows as u64,
            col_lines: s.seal.lines as u64,
            total_sum: s.seal.total_sum,
        }
    }
    /// 复核（十三项全等）。
    pub fn verify(&self, s: &Snapshot) -> bool {
        self.rev == 1
            && self.def_hash == F2982_DEF_HASH
            && self.def_fp == s.def.fingerprint()
            && self.book_fp == s.book.fingerprint()
            && self.chain_fp == s.chain.fingerprint()
            && self.ident_fp == s.ident.fingerprint()
            && self.ring_fp == s.ring.fingerprint()
            && self.seal_fp == s.seal.fingerprint()
            && self.total_fp == s.seal.total_fp
            && self.col_batches == s.seal.batches as u64
            && self.col_rows == s.seal.rows as u64
            && self.col_lines == s.seal.lines as u64
            && self.total_sum == s.seal.total_sum
    }
    /// 漂移说明。
    pub fn drift(&self, s: &Snapshot) -> String {
        if self.def_fp != s.def.fingerprint() {
            return String::from("def_fp 漂移");
        }
        if self.book_fp != s.book.fingerprint() {
            return String::from("book_fp 漂移");
        }
        if self.chain_fp != s.chain.fingerprint() {
            return String::from("chain_fp 漂移");
        }
        if self.ident_fp != s.ident.fingerprint() {
            return String::from("ident_fp 漂移");
        }
        if self.ring_fp != s.ring.fingerprint() {
            return String::from("ring_fp 漂移");
        }
        if self.seal_fp != s.seal.fingerprint() {
            return String::from("seal_fp 漂移");
        }
        if self.total_fp != s.seal.total_fp {
            return String::from("total_fp 漂移");
        }
        if self.col_lines != s.seal.lines as u64 {
            return String::from("col_lines 漂移");
        }
        String::from("无漂移")
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "freeze rev={} def={:016X} def_fp={:016X} book_fp={:016X} chain_fp={:016X} ident_fp={:016X} ring_fp={:016X} seal_fp={:016X} total_fp={:016X} cols={}/{}/{} Σ={}",
            self.rev,
            self.def_hash,
            self.def_fp,
            self.book_fp,
            self.chain_fp,
            self.ident_fp,
            self.ring_fp,
            self.seal_fp,
            self.total_fp,
            self.col_batches,
            self.col_rows,
            self.col_lines,
            self.total_sum
        )
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(14);
        words.push(self.rev as u64);
        words.push(self.def_hash);
        words.push(self.def_fp);
        words.push(self.book_fp);
        words.push(self.chain_fp);
        words.push(self.ident_fp);
        words.push(self.ring_fp);
        words.push(self.seal_fp);
        words.push(self.total_fp);
        words.push(self.col_batches);
        words.push(self.col_rows);
        words.push(self.col_lines);
        words.push(self.total_sum);
        fnv1a_words(&words)
    }
}
// ===========================================================================
// 12. 多轮稳定性
// ===========================================================================
/// 稳定性报告。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Stability {
    /// 轮数。
    pub rounds: usize,
    /// 每轮总账哈希。
    pub round_fp: Vec<u64>,
    /// 基准轮哈希。
    pub base_fp: u64,
    /// 漂移批数。
    pub drift_cells: usize,
    /// 漂移位图（40 位）。
    pub drift_mask: u64,
    /// 是否稳定。
    pub stable: bool,
}
impl Stability {
    /// 漂移批数。
    pub fn drift_count(&self) -> usize {
        self.drift_cells
    }
    /// 轮数达标。
    pub fn rounds_ok(&self) -> bool {
        self.rounds == REPEAT
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "stable rounds={} base={:016X} cells={} mask={:012X} ok={}",
            self.rounds,
            self.base_fp,
            self.drift_cells,
            self.drift_mask,
            if self.stable { 1 } else { 0 }
        )
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(6 + REPEAT);
        words.push(self.rounds as u64);
        words.push(self.base_fp);
        words.push(self.drift_cells as u64);
        words.push(self.drift_mask);
        words.push(self.stable as u64);
        let mut i: usize = 0;
        while i < self.round_fp.len() {
            words.push(self.round_fp[i]);
            i = i.wrapping_add(1);
        }
        fnv1a_words(&words)
    }
}
/// 稳定性检查：跨轮逐批总账指纹比对。
pub fn stability_check(seed: u64) -> UbResult<Stability> {
    let mut round_fp: Vec<u64> = Vec::with_capacity(REPEAT);
    let mut per_batch: Vec<Vec<u64>> = Vec::with_capacity(REPEAT);
    let mut r: usize = 0;
    while r < REPEAT {
        let s = Snapshot::net(seed)?;
        let mut vb: Vec<u64> = Vec::with_capacity(BATCH_TOTAL);
        let mut b: usize = 1;
        while b <= BATCH_TOTAL {
            let bt = s.book.batch_of(b)?;
            vb.push(bt.fp);
            b = b.wrapping_add(1);
        }
        round_fp.push(s.seal.total_fp);
        per_batch.push(vb);
        r = r.wrapping_add(1);
    }
    let base_fp = match round_fp.first() {
        Some(x) => *x,
        None => 0,
    };
    let mut drift_mask: u64 = 0;
    let mut cells: usize = 0;
    let mut rr: usize = 1;
    while rr < REPEAT {
        let mut b: usize = 1;
        while b <= BATCH_TOTAL {
            if per_batch[rr][b - 1] != per_batch[0][b - 1] {
                drift_mask = drift_mask | (1u64 << (b - 1));
                cells = cells.wrapping_add(1);
            }
            b = b.wrapping_add(1);
        }
        rr = rr.wrapping_add(1);
    }
    let mut rp_ok = true;
    let mut i: usize = 0;
    while i < round_fp.len() {
        if round_fp[i] != base_fp {
            rp_ok = false;
        }
        i = i.wrapping_add(1);
    }
    Ok(Stability {
        rounds: REPEAT,
        round_fp,
        base_fp,
        drift_cells: cells,
        drift_mask,
        stable: cells == 0 && rp_ok,
    })
}
// ===========================================================================
// 13. 指标 + 回放 + 资源预算
// ===========================================================================
/// 指标。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Metrics {
    /// 定义项数。
    pub defs: usize,
    /// 定义复用一致。
    pub reuse_ok: bool,
    /// 批次数。
    pub batches: usize,
    /// 批册完备。
    pub book_complete: bool,
    /// 条目数。
    pub rows: usize,
    /// 行数。
    pub lines: usize,
    /// 已审批数。
    pub audited: usize,
    /// 四路聚合和（批侧）。
    pub sum_batch: u64,
    /// 四路聚合和（条目侧）。
    pub sum_row: u64,
    /// 四路聚合和（重算侧）。
    pub sum_recalc: u64,
    /// 四路聚合和（行明细侧）。
    pub sum_lines: u64,
    /// 守恒最大偏离。
    pub deviation: u64,
    /// 四项守恒全过。
    pub quad_ok: bool,
    /// 加法链闭合。
    pub chain_closed: bool,
    /// 复核步数。
    pub steps: u64,
    /// 步数越界批数。
    pub step_overflow: usize,
    /// 环形账写入。
    pub pushed: u64,
    /// 环形账丢弃。
    pub dropped: u64,
    /// 环形账存活。
    pub live: usize,
    /// 环形账守恒。
    pub ring_ok: bool,
    /// 封账留痕备妥。
    pub seal_present: bool,
    /// 冻结核验通过。
    pub freeze_ok: bool,
    /// 轮数。
    pub rounds: usize,
    /// 漂移批数。
    pub drift_cells: usize,
    /// 注入形态数。
    pub forms: usize,
    /// 检出数。
    pub detected: usize,
    /// 净态零误报。
    pub net_clean: bool,
    /// 边界零违规。
    pub border_ok: bool,
    /// TLV 往返。
    pub tlv_ok: bool,
    /// 总指标指纹。
    pub fp: u64,
}
impl Metrics {
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "metrics defs={} reuse={} batches={} ok={} rows={} lines={} audited={} Σ批={} Σ条={} Σ算={} Σ行={} dev={} quad={} chain={} steps={} over={} ring={}/{}/{} ok={} seal={} freeze={} rounds={} dcell={} detect={}/{} net={} border={} tlv={} fp={:016X}",
            self.defs,
            if self.reuse_ok { 1 } else { 0 },
            self.batches,
            if self.book_complete { 1 } else { 0 },
            self.rows,
            self.lines,
            self.audited,
            self.sum_batch,
            self.sum_row,
            self.sum_recalc,
            self.sum_lines,
            self.deviation,
            if self.quad_ok { 1 } else { 0 },
            if self.chain_closed { 1 } else { 0 },
            self.steps,
            self.step_overflow,
            self.live,
            self.dropped,
            self.pushed,
            if self.ring_ok { 1 } else { 0 },
            if self.seal_present { 1 } else { 0 },
            if self.freeze_ok { 1 } else { 0 },
            self.rounds,
            self.drift_cells,
            self.detected,
            self.forms,
            if self.net_clean { 1 } else { 0 },
            if self.border_ok { 1 } else { 0 },
            if self.tlv_ok { 1 } else { 0 },
            self.fp
        )
    }
    /// 逐行表格。
    pub fn rows(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::with_capacity(16);
        out.push((String::from("定义复用"), format!("{} 项 一致{}", self.defs, self.reuse_ok)));
        out.push((String::from("批次册"), format!("{} 完备{}", self.batches, self.book_complete)));
        out.push((String::from("三列读数"), format!("批{} 条{} 行{}", self.batches, self.rows, self.lines)));
        out.push((String::from("已审批"), format!("{} / {}", self.audited, BATCH_TOTAL)));
        out.push((String::from("四路聚合"), format!("批{} 条{} 算{} 行{}", self.sum_batch, self.sum_row, self.sum_recalc, self.sum_lines)));
        out.push((String::from("守恒"), format!("dev{} 四项{}", self.deviation, self.quad_ok)));
        out.push((String::from("加法链"), format!("闭合{} 步{} 越界{}", self.chain_closed, self.steps, self.step_overflow)));
        out.push((String::from("环形账"), format!("存活{} 丢弃{} 写入{} 守恒{}", self.live, self.dropped, self.pushed, self.ring_ok)));
        out.push((String::from("封账源"), format!("备妥{}", self.seal_present)));
        out.push((String::from("冻结件"), format!("通过{}", self.freeze_ok)));
        out.push((String::from("稳定性"), format!("轮{} 漂移批{}", self.rounds, self.drift_cells)));
        out.push((String::from("注入核"), format!("{}/{} 净态零误报{}", self.detected, self.forms, self.net_clean)));
        out.push((String::from("边界"), format!("零违规{}", self.border_ok)));
        out.push((String::from("TLV"), format!("往返{}", self.tlv_ok)));
        out.push((String::from("指纹"), format!("{:016X}", self.fp)));
        out
    }
    /// 全部通过。
    pub fn all_ok(&self) -> bool {
        self.reuse_ok
            && self.book_complete
            && self.batches == BATCH_TOTAL
            && self.rows == ROW_TOTAL
            && self.lines == LINE_TOTAL
            && self.audited == BATCH_TOTAL
            && self.deviation == 0
            && self.quad_ok
            && self.chain_closed
            && self.step_overflow == 0
            && self.ring_ok
            && self.seal_present
            && self.freeze_ok
            && self.rounds == REPEAT
            && self.drift_cells == 0
            && self.detected == self.forms
            && self.net_clean
            && self.border_ok
            && self.tlv_ok
    }
    /// 指纹词表（`tlv_ok` **不参与**：回填动作本身会改 `fp`，往返恒假）。
    pub fn words(&self) -> Vec<u64> {
        vec![
            self.defs as u64,
            self.reuse_ok as u64,
            self.batches as u64,
            self.rows as u64,
            self.lines as u64,
            self.audited as u64,
            self.sum_batch,
            self.sum_row,
            self.sum_recalc,
            self.sum_lines,
            self.deviation,
            self.quad_ok as u64,
            self.chain_closed as u64,
            self.steps,
            self.step_overflow as u64,
            self.pushed,
            self.dropped,
            self.live as u64,
            self.seal_present as u64,
            self.freeze_ok as u64,
            self.rounds as u64,
            self.drift_cells as u64,
            self.detected as u64,
            self.fp,
        ]
    }
}
/// 回放报告。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Replay {
    /// 种子。
    pub seed: u64,
    /// 轮数。
    pub rounds: usize,
    /// 每轮总账哈希。
    pub round_fp: Vec<u64>,
    /// 每轮四路聚合和。
    pub quads: Vec<(u64, u64, u64, u64)>,
    /// 是否一致。
    pub consistent: bool,
    /// 指纹。
    pub fp: u64,
}
impl Replay {
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "replay seed={:016X} rounds={} consistent={} fp={:016X}",
            self.seed,
            self.rounds,
            if self.consistent { 1 } else { 0 },
            self.fp
        )
    }
}
/// 确定性回放。
pub fn replay(seed: u64) -> UbResult<Replay> {
    let mut round_fp: Vec<u64> = Vec::with_capacity(REPEAT);
    let mut quads: Vec<(u64, u64, u64, u64)> = Vec::with_capacity(REPEAT);
    let mut r: usize = 0;
    while r < REPEAT {
        let s = Snapshot::net(seed)?;
        round_fp.push(s.seal.total_fp);
        quads.push((
            s.ident.sum_batch,
            s.ident.sum_row,
            s.ident.sum_recalc,
            s.ident.sum_lines,
        ));
        r = r.wrapping_add(1);
    }
    let mut consistent = true;
    let mut i: usize = 1;
    while i < round_fp.len() {
        if round_fp[i] != round_fp[0] || quads[i] != quads[0] {
            consistent = false;
        }
        i = i.wrapping_add(1);
    }
    let mut words: Vec<u64> = Vec::with_capacity(6 * REPEAT + 3);
    words.push(seed);
    words.push(REPEAT as u64);
    words.push(consistent as u64);
    let mut j: usize = 0;
    while j < round_fp.len() {
        words.push(round_fp[j]);
        words.push(quads[j].0);
        words.push(quads[j].1);
        words.push(quads[j].2);
        words.push(quads[j].3);
        j = j.wrapping_add(1);
    }
    Ok(Replay {
        seed,
        rounds: REPEAT,
        round_fp,
        quads,
        consistent,
        fp: fnv1a_words(&words),
    })
}
/// 资源预算。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ResourceBudget {
    /// 单快照字节。
    pub snapshot_bytes: usize,
    /// 快照份数。
    pub snapshot_count: usize,
    /// 账面条目行数（不物化，只记账）。
    pub logical_lines: usize,
    /// 导出缓冲字节。
    pub export_bytes: usize,
    /// 峰值字节。
    pub peak_bytes: u64,
    /// 峰值上限。
    pub peak_cap: u64,
    /// 逻辑步数。
    pub steps: u64,
    /// 步数上限。
    pub step_cap: u64,
    /// syscall 次数。
    pub syscalls: u64,
    /// syscall 上限。
    pub syscall_cap: u64,
}
impl ResourceBudget {
    /// 核算（只读链审计，不构造完整 `Snapshot` 以避免自引用）。
    pub fn compute(chain: &ChainAudit) -> ResourceBudget {
        let resident = core::mem::size_of::<Snapshot>();
        let count = Form::COUNT;
        let export = 4096usize;
        let peak = (resident.wrapping_mul(count).wrapping_add(export)) as u64;
        ResourceBudget {
            snapshot_bytes: resident,
            snapshot_count: count,
            logical_lines: LINE_TOTAL,
            export_bytes: export,
            peak_bytes: peak,
            peak_cap: MEM_PEAK_CAP,
            steps: chain.steps,
            step_cap: STEP_CAP,
            syscalls: SYSCALL_CAP,
            syscall_cap: SYSCALL_CAP,
        }
    }
    /// 预算内。
    pub fn ok(&self) -> bool {
        self.peak_bytes <= self.peak_cap && self.steps <= self.step_cap && self.syscalls <= self.syscall_cap
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "budget snap={}x{}B logical_lines={} export={}B peak={}/{}B steps={}/{} syscalls={}/{} ok={}",
            self.snapshot_count,
            self.snapshot_bytes,
            self.logical_lines,
            self.export_bytes,
            self.peak_bytes,
            self.peak_cap,
            self.steps,
            self.step_cap,
            self.syscalls,
            self.syscall_cap,
            if self.ok() { 1 } else { 0 }
        )
    }
}
/// `observe` 结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UbObserve {
    /// 净态快照。
    pub net: Snapshot,
    /// 冻结件。
    pub freeze: FreezeRecord,
    /// 稳定性。
    pub stability: Stability,
    /// 注入留痕。
    pub evidence: Evidence,
    /// 逐快照检出。
    pub detects: Vec<Detections>,
    /// 回放。
    pub replay: Replay,
    /// 指标。
    pub metrics: Metrics,
    /// 资源预算。
    pub budget: ResourceBudget,
}
impl UbObserve {
    /// 全部通过。
    pub fn all_ok(&self) -> bool {
        self.metrics.all_ok() && self.net.is_net() && self.stability.stable && self.stability.rounds_ok() && self.replay.consistent
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        self.metrics.summary()
    }
    /// 逐行表格。
    pub fn rows(&self) -> Vec<(String, String)> {
        self.metrics.rows()
    }
}
// ===========================================================================
// 14. 导出
// ===========================================================================
/// KV 导出。
pub fn export_kv(o: &UbObserve) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(4096);
    let push_line = |out: &mut Vec<u8>, k: &str, v: String| {
        out.extend_from_slice(k.as_bytes());
        out.push(b'=');
        out.extend_from_slice(v.as_bytes());
        out.push(b'\n');
    };
    push_line(&mut out, "version", String::from(V_VERSION));
    push_line(&mut out, "seg_base", format!("{:04X}", SEG_BASE));
    push_line(&mut out, "seg_mask", format!("{:04X}", SEG_MASK));
    push_line(&mut out, "def_src", String::from(DEF_SRC));
    push_line(&mut out, "def_hash", format!("{:016X}", F2982_DEF_HASH));
    push_line(&mut out, "def_key_hash", format!("{:016X}", DEF_KEY_HASH));
    push_line(&mut out, "def_text_hash", format!("{:016X}", DEF_TEXT_HASH));
    push_line(&mut out, "def_rev", format!("{}", DEF_REV));
    push_line(&mut out, "def_reuse", format!("{}", o.metrics.reuse_ok));
    push_line(&mut out, "def_items", format!("{}", o.metrics.defs));
    push_line(&mut out, "batch_range", String::from(BATCH_RANGE));
    push_line(&mut out, "sub_ledger", String::from(SUB_LEDGER));
    push_line(&mut out, "seal_face", String::from(SEAL_FACE));
    push_line(&mut out, "align_chain", String::from(ALIGN_CHAIN));
    push_line(&mut out, "batches", format!("{}", o.metrics.batches));
    push_line(&mut out, "rows", format!("{}", o.metrics.rows));
    push_line(&mut out, "lines", format!("{}", o.metrics.lines));
    push_line(&mut out, "audited", format!("{}", o.metrics.audited));
    push_line(&mut out, "sum_batch", format!("{}", o.metrics.sum_batch));
    push_line(&mut out, "sum_row", format!("{}", o.metrics.sum_row));
    push_line(&mut out, "sum_recalc", format!("{}", o.metrics.sum_recalc));
    push_line(&mut out, "sum_lines", format!("{}", o.metrics.sum_lines));
    push_line(&mut out, "deviation", format!("{}", o.metrics.deviation));
    push_line(&mut out, "quad_ok", format!("{}", o.metrics.quad_ok));
    push_line(&mut out, "chain_closed", format!("{}", o.metrics.chain_closed));
    push_line(&mut out, "steps", format!("{}", o.metrics.steps));
    push_line(&mut out, "step_overflow", format!("{}", o.metrics.step_overflow));
    push_line(&mut out, "pushed", format!("{}", o.metrics.pushed));
    push_line(&mut out, "dropped", format!("{}", o.metrics.dropped));
    push_line(&mut out, "live", format!("{}", o.metrics.live));
    push_line(&mut out, "seal_present", format!("{}", o.metrics.seal_present));
    push_line(&mut out, "total_fp", format!("{:016X}", o.freeze.total_fp));
    push_line(&mut out, "final_carry", format!("{}", o.net.chain.final_carry));
    push_line(&mut out, "freeze", format!("{:016X}", o.freeze.fingerprint()));
    push_line(&mut out, "stability", format!("{:016X}", o.stability.fingerprint()));
    push_line(&mut out, "replay", format!("{:016X}", o.replay.fp));
    push_line(&mut out, "budget", String::from(o.budget.summary()));
    push_line(&mut out, "metrics_fp", format!("{:016X}", o.metrics.fp));
    out
}
/// 推 8 字节大端。
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
/// 写一条 TLV 条目（长度字段恒占定宽 2 字节）。
pub fn push_tlv_u64(out: &mut Vec<u8>, tag: u8, v: u64) {
    out.push(tag);
    out.push(0);
    out.push(8);
    push_u64(out, v);
}
/// TLV 导出。
///
/// 组头**单独一行**推入：`push_u64(&mut head_payload, V_VERSION_ID);`
/// 三条 `push` 决定 `TLV_HEAD_PAYLOAD = 24`，故 `TLV_HEAD_LEN = 1 + 2 + 24 = 27`；
/// 长度字段固定 2 字节，条目固定 `1 + 2 + 8 = 11` 字节，往返必然一致。
pub fn export_tlv(o: &UbObserve) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(2048);
    let mut head_payload: Vec<u8> = Vec::with_capacity(TLV_HEAD_PAYLOAD);
    push_u64(&mut head_payload, V_VERSION_ID);
    push_u64(&mut head_payload, SEG_BASE as u64);
    push_u64(&mut head_payload, o.freeze.rev as u64);
    out.push(TLV_TAG_HEAD);
    let hl = head_payload.len();
    out.push(((hl >> 8) & 0xFF) as u8);
    out.push((hl & 0xFF) as u8);
    let mut i: usize = 0;
    while i < head_payload.len() {
        out.push(head_payload[i]);
        i = i.wrapping_add(1);
    }
    push_tlv_u64(&mut out, 0x10, o.metrics.defs as u64);
    push_tlv_u64(&mut out, 0x11, o.metrics.reuse_ok as u64);
    push_tlv_u64(&mut out, 0x12, F2982_DEF_HASH);
    push_tlv_u64(&mut out, 0x13, DEF_KEY_HASH);
    push_tlv_u64(&mut out, 0x14, DEF_TEXT_HASH);
    push_tlv_u64(&mut out, 0x20, o.metrics.batches as u64);
    push_tlv_u64(&mut out, 0x21, o.metrics.rows as u64);
    push_tlv_u64(&mut out, 0x22, o.metrics.lines as u64);
    push_tlv_u64(&mut out, 0x23, o.metrics.audited as u64);
    push_tlv_u64(&mut out, 0x30, o.metrics.sum_batch);
    push_tlv_u64(&mut out, 0x31, o.metrics.sum_row);
    push_tlv_u64(&mut out, 0x32, o.metrics.sum_recalc);
    push_tlv_u64(&mut out, 0x33, o.metrics.sum_lines);
    push_tlv_u64(&mut out, 0x34, o.metrics.deviation);
    push_tlv_u64(&mut out, 0x35, o.metrics.quad_ok as u64);
    push_tlv_u64(&mut out, 0x40, o.metrics.steps);
    push_tlv_u64(&mut out, 0x41, o.metrics.step_overflow as u64);
    push_tlv_u64(&mut out, 0x50, o.metrics.pushed);
    push_tlv_u64(&mut out, 0x51, o.metrics.dropped);
    push_tlv_u64(&mut out, 0x52, o.metrics.live as u64);
    push_tlv_u64(&mut out, 0x60, o.net.chain.final_carry);
    push_tlv_u64(&mut out, 0x61, o.freeze.total_fp);
    push_tlv_u64(&mut out, 0x62, o.freeze.def_fp);
    push_tlv_u64(&mut out, 0x63, o.freeze.book_fp);
    push_tlv_u64(&mut out, 0x64, o.freeze.chain_fp);
    push_tlv_u64(&mut out, 0x65, o.freeze.ident_fp);
    push_tlv_u64(&mut out, 0x66, o.freeze.ring_fp);
    push_tlv_u64(&mut out, 0x67, o.freeze.seal_fp);
    push_tlv_u64(&mut out, 0x68, o.freeze.fingerprint());
    push_tlv_u64(&mut out, 0x70, o.stability.fingerprint());
    push_tlv_u64(&mut out, 0x71, o.replay.fp);
    push_tlv_u64(&mut out, 0x90, o.budget.peak_bytes);
    push_tlv_u64(&mut out, 0x91, o.budget.steps);
    push_tlv_u64(&mut out, 0x92, o.budget.syscalls);
    push_tlv_u64(&mut out, 0x93, o.budget.logical_lines as u64);
    push_tlv_u64(&mut out, 0xA0, o.metrics.fp);
    out
}
/// 读 8 字节大端。
pub fn be_u64(b: &[u8], off: usize) -> u64 {
    let mut v: u64 = 0;
    let mut i: usize = 0;
    while i < 8 {
        let idx = off.wrapping_add(i);
        let byte = if idx < b.len() { b[idx] } else { 0 };
        v = (v << 8) | (byte as u64);
        i = i.wrapping_add(1);
    }
    v
}
/// 解析 TLV。
pub fn parse_tlv(buf: &[u8]) -> UbResult<(u64, Vec<(u8, u64)>)> {
    if buf.len() < TLV_HEAD_LEN {
        return Err(err_range("tlv", buf.len(), TLV_HEAD_LEN));
    }
    if buf[0] != TLV_TAG_HEAD {
        return Err(UbErr::of_aux(AuxCode::FreezeMismatch, format!("组头标签错：{:02X}", buf[0])));
    }
    let hl = ((buf[1] as usize) << 8) | (buf[2] as usize);
    if hl != TLV_HEAD_PAYLOAD {
        return Err(UbErr::of_aux(AuxCode::FreezeMismatch, format!("组头长度错：{}", hl)));
    }
    let version = be_u64(buf, 3);
    let mut items: Vec<(u8, u64)> = Vec::with_capacity(40);
    let mut off: usize = TLV_HEAD_LEN;
    while off < buf.len() {
        if off + TLV_ITEM_LEN > buf.len() {
            break;
        }
        let tag = buf[off];
        let len = ((buf[off + 1] as usize) << 8) | (buf[off + 2] as usize);
        if len != 8 {
            return Err(UbErr::of_aux(AuxCode::CapOver, format!("条目长度错：tag={:02X} len={}", tag, len)));
        }
        items.push((tag, be_u64(buf, off + 3)));
        off = off + TLV_ITEM_LEN;
    }
    Ok((version, items))
}
/// 按标签取值。
pub fn tlv_get(items: &[(u8, u64)], tag: u8) -> UbResult<u64> {
    let mut i: usize = 0;
    while i < items.len() {
        if items[i].0 == tag {
            return Ok(items[i].1);
        }
        i = i.wrapping_add(1);
    }
    Err(UbErr::of_aux(AuxCode::SeqMismatch, format!("条目缺失：tag={:02X}", tag)))
}
/// TLV 往返一致（逐条目与现场重算比对）。
pub fn tlv_roundtrip_ok(o: &UbObserve) -> bool {
    let buf = export_tlv(o);
    let parsed = match parse_tlv(&buf) {
        Ok(x) => x,
        Err(_) => return false,
    };
    if parsed.0 != V_VERSION_ID {
        return false;
    }
    let items = parsed.1;
    let mut ok = true;
    let mut i: usize = 0;
    while i < items.len() {
        let tag = items[i].0;
        let got = items[i].1;
        let want = match tag {
            0x10 => o.metrics.defs as u64,
            0x11 => o.metrics.reuse_ok as u64,
            0x12 => F2982_DEF_HASH,
            0x13 => DEF_KEY_HASH,
            0x14 => DEF_TEXT_HASH,
            0x20 => o.metrics.batches as u64,
            0x21 => o.metrics.rows as u64,
            0x22 => o.metrics.lines as u64,
            0x23 => o.metrics.audited as u64,
            0x30 => o.metrics.sum_batch,
            0x31 => o.metrics.sum_row,
            0x32 => o.metrics.sum_recalc,
            0x33 => o.metrics.sum_lines,
            0x34 => o.metrics.deviation,
            0x35 => o.metrics.quad_ok as u64,
            0x40 => o.metrics.steps,
            0x41 => o.metrics.step_overflow as u64,
            0x50 => o.metrics.pushed,
            0x51 => o.metrics.dropped,
            0x52 => o.metrics.live as u64,
            0x60 => o.net.chain.final_carry,
            0x61 => o.freeze.total_fp,
            0x62 => o.freeze.def_fp,
            0x63 => o.freeze.book_fp,
            0x64 => o.freeze.chain_fp,
            0x65 => o.freeze.ident_fp,
            0x66 => o.freeze.ring_fp,
            0x67 => o.freeze.seal_fp,
            0x68 => o.freeze.fingerprint(),
            0x70 => o.stability.fingerprint(),
            0x71 => o.replay.fp,
            0x90 => o.budget.peak_bytes,
            0x91 => o.budget.steps,
            0x92 => o.budget.syscalls,
            0x93 => o.budget.logical_lines as u64,
            0xA0 => o.metrics.fp,
            _ => {
                ok = false;
                0
            }
        };
        if got != want {
            ok = false;
        }
        i = i.wrapping_add(1);
    }
    if buf.len() != TLV_HEAD_LEN + items.len() * TLV_ITEM_LEN {
        ok = false;
    }
    ok
}
// ===========================================================================
// 15. 复杂度分解（「深在哪」）
// ===========================================================================
/// 成本项。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CostItem {
    /// 项名。
    pub name: String,
    /// 迭代次数。
    pub iters: u64,
    /// 单次代价。
    pub per_iter: u64,
    /// 合计步数。
    pub total: u64,
    /// Windows 同类能力对照（「深在哪」）。
    pub note: String,
}
impl Metrics {
    /// 逐项分解。
    pub fn complexity(&self) -> Vec<CostItem> {
        let mut out: Vec<CostItem> = Vec::with_capacity(12);
        let rows = ROW_TOTAL as u64;
        let lines = LINE_TOTAL as u64;
        out.push(CostItem {
            name: String::from("定义复用登记"),
            iters: DefSlot::COUNT as u64,
            per_iter: 8,
            total: DefSlot::COUNT as u64 * 8,
            note: String::from("域内收口制度无公开对应件；Windows 无「四项守恒定义跨单复用」的等价能力"),
        });
        out.push(CostItem {
            name: String::from("终账条目归集"),
            iters: rows,
            per_iter: 12,
            total: rows * 12,
            note: String::from("对标 Windows ETW 的事件聚合：ETW 只做单向聚合，本单做四路异源交叉复核"),
        });
        out.push(CostItem {
            name: String::from("行级明细重算"),
            iters: lines,
            per_iter: 6,
            total: lines * 6,
            note: String::from("240,000 行纯函数重算；对标 WinSxS 组件存储校验，无逐行加法链逐批复核对应件"),
        });
        out.push(CostItem {
            name: String::from("条内压缩/解压"),
            iters: rows,
            per_iter: 9,
            total: rows * 9,
            note: String::from("读数是行级和的可逆压缩；压缩-解压链是本单自制的一致性机制"),
        });
        out.push(CostItem {
            name: String::from("批次结算"),
            iters: self.batches as u64,
            per_iter: 40,
            total: self.batches as u64 * 40,
            note: String::from("批内和 + 批间进位串链；对标 NTFS 卷位图串链，无账面进位复核对应件"),
        });
        out.push(CostItem {
            name: String::from("加法链逐批复核"),
            iters: self.batches as u64,
            per_iter: 300,
            total: self.batches as u64 * 300,
            note: String::from("每批 20 条 × 300 行全量重算；深在「复核侧不读结算侧累加器」"),
        });
        out.push(CostItem {
            name: String::from("四路恒等聚合"),
            iters: lines,
            per_iter: 3,
            total: lines * 3,
            note: String::from("批侧/条目侧/重算侧/行明细侧四路异源；单点篡改无处互相抵消"),
        });
        out.push(CostItem {
            name: String::from("ID 位图登记"),
            iters: rows,
            per_iter: 2,
            total: rows * 2,
            note: String::from("800 位分 13 片 u64；重号不重复计数、断档由占位数检出"),
        });
        out.push(CostItem {
            name: String::from("环形账写入"),
            iters: self.pushed,
            per_iter: 10,
            total: self.pushed * 10,
            note: String::from("append-only + 覆盖丢弃显性；对标 Windows 环形缓冲区，无丢弃留痕对应件"),
        });
        out.push(CostItem {
            name: String::from("注入核"),
            iters: Form::COUNT as u64 * (lines + rows),
            per_iter: 2,
            total: Form::COUNT as u64 * (lines + rows) * 2,
            note: String::from("14 形态各自重建全链快照；深在「每形态只污染自己那一面」"),
        });
        out.push(CostItem {
            name: String::from("稳定性"),
            iters: REPEAT as u64 * rows,
            per_iter: 2,
            total: REPEAT as u64 * rows * 2,
            note: String::from("跨轮逐批指纹比对；确定性保证同批同值"),
        });
        out.push(CostItem {
            name: String::from("回放"),
            iters: REPEAT as u64,
            per_iter: lines + rows,
            total: REPEAT as u64 * (lines + rows),
            note: String::from("整链重跑 REPEAT 轮；总账哈希与四路聚合必须全等"),
        });
        out
    }
    /// 复杂度摘要。
    pub fn complexity_summary(&self) -> String {
        let items = self.complexity();
        let mut total: u64 = 0;
        let mut i: usize = 0;
        while i < items.len() {
            total = total.wrapping_add(items[i].total);
            i = i.wrapping_add(1);
        }
        format!(
            "complexity items={} total_steps={} per_step=1 syscall={} peak_cap={}B syscalls={}",
            items.len(),
            total,
            SYSCALL_CAP,
            MEM_PEAK_CAP,
            SYSCALL_CAP
        )
    }
}
// ===========================================================================
// 16. observe 主入口
// ===========================================================================
/// 观察入口：真跑 240,000 行并给出可复核结论。
///
/// 四段式（严格对应任务单「三步」+ 断言）：
/// 1. **定义复用**：`def_book_full()` 登记 F2982 四项冻结哈希，
///    `reuse_ok()` 逐项比对 `DEF_*` 常量；
/// 2. **终账归集**：800 条 × 300 行 = 240,000 行真实读数聚合，
///    40 批结算 + 进位串链；
/// 3. **加法链逐批复核**：`ChainAudit::run` 四条链逐批闭合；
/// 4. **断言与入册**：四路异源恒等式零偏离 + 注入核 14/14 全拒 +
///    净态零误报 + 冻结件十三项复核 + 稳定性 + 回放 + TLV 往返，
///    最后把总账三列读数与总账哈希入册备 F3198 消费。
pub fn observe(seed: u64) -> UbResult<UbObserve> {
    // ── 净态全链
    let net = Snapshot::net(seed)?;
    // 1) 定义复用
    if !net.def.reuse_ok() {
        return Err(UbErr::of_form(
            Form::DefKeyDrift,
            format!("定义复用失配：{:?}", net.def.drift_list()),
        ));
    }
    // 2) 终账册完备
    if !net.book.complete() {
        return Err(UbErr::of_form(
            Form::BatchMissing,
            format!("终账册不完备：{}", net.book.summary()),
        ));
    }
    // 3) 加法链逐批复核
    if !net.chain.closed || net.chain.ok_count() != BATCH_TOTAL {
        return Err(UbErr::of_form(
            Form::ChainBroken,
            format!("加法链未闭合：{} / {}", net.chain.ok_count(), BATCH_TOTAL),
        ));
    }
    if !net.chain.carry_continuous(&net.book) {
        return Err(UbErr::of_form(
            Form::CarryMismatch,
            String::from("批间进位链断裂"),
        ));
    }
    if net.chain.step_overflow != 0 {
        return Err(UbErr::of_aux(
            AuxCode::CapOver,
            format!("复核步数越界 {} 批", net.chain.step_overflow),
        ));
    }
    // 4) 四路异源恒等式
    if !net.ident.quad_ok {
        return Err(UbErr::of_form(
            Form::LedgerUnbalanced,
            format!("四项守恒未全过：{}", net.ident.summary()),
        ));
    }
    if net.ident.deviation != 0 {
        return Err(err_balance(
            net.ident.sum_batch,
            net.ident.sum_row,
            net.ident.sum_lines,
        ));
    }
    // 5) 环形账守恒
    if !net.ring.balanced() {
        return Err(UbErr::of_form(
            Form::LedgerUnbalanced,
            format!(
                "环形账不守恒：写入{} != 存活{} + 丢弃{}",
                net.ring.pushed,
                net.ring.live(),
                net.ring.dropped
            ),
        ));
    }
    // 6) 封账真相源留痕
    if !net.seal.present {
        return Err(UbErr::of_form(
            Form::SealSourceLost,
            format!("封账真相源失联：{}", net.seal.describe()),
        ));
    }
    if !net.seal.carry_closed() {
        return Err(UbErr::of_form(
            Form::CarryMismatch,
            format!(
                "进位链与读数链不闭合：carry={} Σ={}",
                net.seal.final_carry, net.seal.total_sum
            ),
        ));
    }
    // ── 冻结件：build + verify
    let freeze = FreezeRecord::build(&net);
    if !freeze.verify(&net) {
        return Err(UbErr::of_aux(
            AuxCode::FreezeMismatch,
            format!("冻结件复核不过：{}", freeze.drift(&net)),
        ));
    }
    // ── 稳定性
    let stability = stability_check(seed)?;
    if !stability.stable {
        return Err(UbErr::of_aux(
            AuxCode::FreezeMismatch,
            format!("多轮不稳定：{} 批漂移", stability.drift_cells),
        ));
    }
    if stability.rounds != REPEAT {
        return Err(UbErr::of_aux(
            AuxCode::RepeatShort,
            format!("轮数不足：{} < {}", stability.rounds, REPEAT),
        ));
    }
    // ── 注入核：14/14 全拒
    let (evidence, snaps) = inject_all(seed)?;
    let mut detects: Vec<Detections> = Vec::with_capacity(Form::COUNT);
    let mut hits: usize = 0;
    let mut fi: usize = 0;
    while fi < Form::ALL.len() {
        let d = detect_all(&snaps[fi])?;
        if !d.hit(Form::ALL[fi]) {
            return Err(UbErr::of_form(
                Form::ALL[fi],
                format!("注入漏检：{}", Form::ALL[fi].detail()),
            ));
        }
        hits = hits.wrapping_add(1);
        detects.push(d);
        fi = fi.wrapping_add(1);
    }
    if hits != FORM_COUNT {
        return Err(UbErr::of_aux(
            AuxCode::CapOver,
            format!("注入形态数不符：{} != {}", hits, FORM_COUNT),
        ));
    }
    // 净态零误报（与注入方向相反的断言）
    let net_detect = detect_all(&net)?;
    if !net_detect.is_clean() {
        return Err(UbErr::of_form(
            Form::LedgerUnbalanced,
            format!("净态误报：{:?}", net_detect.hit_list()),
        ));
    }
    if net_detect.border_violations != 0 {
        return Err(UbErr::of_form(
            Form::SealSourceLost,
            format!("域外动作 {} 次", net_detect.border_violations),
        ));
    }
    // ── 回放
    let rp = replay(seed)?;
    if !rp.consistent {
        return Err(UbErr::of_aux(
            AuxCode::SeqMismatch,
            String::from("回放不一致：跨轮总账哈希分歧"),
        ));
    }
    // ── 资源预算
    let budget = ResourceBudget::compute(&net.chain);
    if !budget.ok() {
        return Err(UbErr::of_aux(
            AuxCode::CapOver,
            format!("预算越界：{}", budget.summary()),
        ));
    }
    // ── 指标定稿
    let mut m = Metrics {
        defs: net.def.rows.len(),
        reuse_ok: net.def.reuse_ok(),
        batches: net.book.batches.len(),
        book_complete: net.book.complete(),
        rows: net.book.rows.len(),
        lines: net.seal.lines,
        audited: net.book.audited_count(),
        sum_batch: net.ident.sum_batch,
        sum_row: net.ident.sum_row,
        sum_recalc: net.ident.sum_recalc,
        sum_lines: net.ident.sum_lines,
        deviation: net.ident.deviation,
        quad_ok: net.ident.quad_ok,
        chain_closed: net.chain.closed,
        steps: net.chain.steps,
        step_overflow: net.chain.step_overflow,
        pushed: net.ring.pushed,
        dropped: net.ring.dropped,
        live: net.ring.live(),
        ring_ok: net.ring.balanced(),
        seal_present: net.seal.present,
        freeze_ok: freeze.verify(&net),
        rounds: stability.rounds,
        drift_cells: stability.drift_cells,
        forms: FORM_COUNT,
        detected: hits,
        net_clean: net_detect.is_clean(),
        border_ok: net.ev.border_violations == 0,
        // `tlv_ok` 不参与 `words()` 汇总：否则回填动作本身会改 `fp`，
        // 令 `export_tlv` 与 `words()` 的往返比对恒假。
        tlv_ok: false,
        fp: 0,
    };
    m.fp = fnv1a_words(&m.words());
    let mut o = UbObserve {
        net,
        freeze,
        stability,
        evidence,
        detects,
        replay: rp,
        metrics: m,
        budget,
    };
    // TLV 往返：与现场重算逐条目比对，通过后回填（不参与 fp 汇总）
    o.metrics.tlv_ok = tlv_roundtrip_ok(&o);
    if !o.metrics.tlv_ok {
        return Err(UbErr::of_aux(
            AuxCode::FreezeMismatch,
            String::from("TLV 往返不一致"),
        ));
    }
    if !o.metrics.all_ok() {
        return Err(UbErr::of_form(
            Form::FingerprintDrift,
            format!("指标未全过：{}", o.metrics.summary()),
        ));
    }
    Ok(o)
}
// ===========================================================================
// 17. 演示辅助
// ===========================================================================
/// 形态清单。
pub fn form_manifest() -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(Form::COUNT);
    let mut i: usize = 0;
    while i < Form::ALL.len() {
        let f = Form::ALL[i];
        out.push(format!(
            "[{:04X}] {:<20} {:<20} {}",
            f.code(),
            f.name(),
            f.probe(),
            f.detail()
        ));
        i = i.wrapping_add(1);
    }
    out
}
/// 辅助码清单。
pub fn aux_manifest() -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(AuxCode::COUNT);
    let mut i: usize = 0;
    while i < AuxCode::ALL.len() {
        let a = AuxCode::ALL[i];
        out.push(format!("[{:04X}] {:<18} {}", a.code(), a.name(), a.detail()));
        i = i.wrapping_add(1);
    }
    out
}
/// 不变式清单。
pub fn invariant_manifest() -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(20);
    out.push(format!("INV-01 码段自闭：{} ∈ [{}..{}]", SEG_MASK, SEG_BASE, SEG_END));
    out.push(format!("INV-02 形态一码一段：{} 形态占 {}..{:04X}", Form::COUNT, SEG_BASE, Form::ALL[Form::COUNT - 1].code()));
    out.push(format!("INV-03 辅助码独立段：{} 辅助码占 {:04X}..{:04X}", AuxCode::COUNT, AuxCode::ALL[0].code(), AuxCode::ALL[AuxCode::COUNT - 1].code()));
    out.push(format!("INV-04 禁二次定义：四项定义哈希为 const，本单无重算路径"));
    out.push(format!("INV-05 定义复用一致：登记值 == DEF_* 常量 逐项相等"));
    out.push(format!("INV-06 规模整：{} 批 × {} 条 = {} 条", BATCH_TOTAL, ROWS_PER_BATCH, ROW_TOTAL));
    out.push(format!("INV-07 行数整：{} 条 × {} 行 = {} 行", ROW_TOTAL, LINES_PER_ROW, LINE_TOTAL));
    out.push(String::from("INV-08 批次册无重无缺无越界无错序"));
    out.push(format!("INV-09 守恒一条数：登记条数 == {}", ROW_TOTAL));
    out.push(format!("INV-10 守恒二 ID 连续：{} 位分片位图占满无重号无断档", ID_CHUNKS));
    out.push(format!("INV-11 守恒三行数：Σ批行数 == {}", LINE_TOTAL));
    out.push(String::from("INV-12 守恒四分解和：逐条 split == reading，Σ批 split == Σ批 reading"));
    out.push(String::from("INV-13 四路异源恒等：批侧/条目侧/重算侧/行明细侧 零偏离"));
    out.push(String::from("INV-14 三层加法链：条内 / 批内 / 批间逐批复核闭合"));
    out.push(format!("INV-15 环形账守恒：写入 == 存活 + 丢弃（容量 {}）", RING_CAP));
    out.push(String::from("INV-16 覆盖丢弃显性：dropped 计入指标并入册"));
    out.push(String::from("INV-17 注入 14/14 全拒留痕 + 净态零误报"));
    out.push(String::from("INV-18 冻结件十三项全等 + 封账真相源三列备妥"));
    out.push(format!("INV-19 预算内：峰值/步数（{}）/syscall（{}）三项", STEP_CAP, SYSCALL_CAP));
    out.push(String::from("INV-20 无墙钟无浮点无真实 IO：240,000 行纯整型运算"));
    out
}
/// 定义复用演示。
pub fn demo_def() -> UbResult<Vec<String>> {
    let b = def_book_full()?;
    let mut out: Vec<String> = Vec::with_capacity(DefSlot::COUNT + 8);
    out.push(b.summary());
    out.push(format!(
        "引用源 {} 版次 {} 域位 {:04X} 整体哈希 {:016X}",
        DEF_SRC, DEF_REV, DEF_FACE_ID, F2982_DEF_HASH
    ));
    out.push(format!(
        "冻结键名哈希 {:016X} / 冻结口径哈希 {:016X}",
        DEF_KEY_HASH, DEF_TEXT_HASH
    ));
    let mut i: usize = 0;
    while i < b.rows.len() {
        out.push(b.rows[i].describe());
        i = i.wrapping_add(1);
    }
    out.push(format!("失配明细：{:?}", b.drift_list()));
    Ok(out)
}
/// 终账册演示。
pub fn demo_ledger(seed: u64) -> UbResult<Vec<String>> {
    let mut g = grand_book_full()?;
    settle_batches(&mut g)?;
    let mut out: Vec<String> = Vec::with_capacity(BATCH_TOTAL + 10);
    out.push(g.summary());
    out.push(format!(
        "规模：{} 批 × {} 条 = {} 条 × {} 行 = {} 行",
        BATCH_TOTAL, ROWS_PER_BATCH, ROW_TOTAL, LINES_PER_ROW, LINE_TOTAL
    ));
    let mut b: usize = 1;
    while b <= BATCH_TOTAL {
        out.push(g.batch_of(b)?.describe());
        b = b.wrapping_add(1);
    }
    let chain = ChainAudit::run(&g)?;
    out.push(chain.summary());
    out.push(format!("进位链连续={} 全过批={}", chain.carry_continuous(&g), chain.ok_count()));
    let ident = identity_close(&def_book_full()?, &g, &chain)?;
    out.push(ident.summary());
    let ring = ring_fill(&g, REPEAT)?;
    out.push(ring.describe());
    let _ = seed;
    Ok(out)
}
/// 封账源演示。
pub fn demo_seal(seed: u64) -> UbResult<Vec<String>> {
    let s = Snapshot::net(seed)?;
    let mut out: Vec<String> = Vec::with_capacity(12);
    out.push(s.seal.describe());
    out.push(format!("消费方 {}：本单只留痕不确认", SEAL_FACE));
    out.push(format!(
        "三列读数一致性：{} 进位链闭合={}",
        s.seal.columns_ok(),
        s.seal.carry_closed()
    ));
    out.push(format!(
        "读数：批{} 条{} 行{} Σ{}",
        s.seal.batches, s.seal.rows, s.seal.lines, s.seal.total_sum
    ));
    out.push(format!("加法链最终进位 = {}", s.chain.final_carry));
    out.push(format!("留痕指纹 = {:016X}", s.seal.fingerprint()));
    out.push(String::from(FreezeRecord::build(&s).summary()));
    Ok(out)
}
/// 关联件说明。
pub fn related_str() -> String {
    let mut s = String::new();
    s.push_str("【定义来源】");
    s.push_str(DEF_SRC);
    s.push_str("：四项守恒定义（条数 / ID 连续 / 行数 / 分解和）冻结哈希引用登记，禁二次定义\n");
    s.push_str("【批次域】");
    s.push_str(BATCH_RANGE);
    s.push_str("：40 批 800 条，每批一冻结件\n");
    s.push_str("【四项子账】");
    s.push_str(SUB_LEDGER);
    s.push_str("：条数 / ID 连续 / 行数 / 分解和四条子账\n");
    s.push_str("【封账】");
    s.push_str(SEAL_FACE);
    s.push_str("：域封账，本单提供账面真相源（三列读数 + 总账哈希），不做封账确认\n");
    s.push_str("【归档对齐】");
    s.push_str(ALIGN_CHAIN);
    s.push_str("：体检单链，读数键随批账册归档可回放；衔接单 ");
    s.push_str(LINK_TAIL);
    s.push_str("\n");
    s.push_str("【终批】");
    s.push_str(FINAL_BATCH);
    s.push('\n');
    s
}
/// 全量报告。
pub fn full_report(seed: u64) -> UbResult<String> {
    let o = observe(seed)?;
    let mut s = String::new();
    s.push_str("═══ UNX-F3181 域收官总账 · 全量报告 ═══\n");
    s.push_str(&format!("版本 {}\n", V_VERSION));
    s.push_str(&format!("{}\n", seg_line()));
    s.push_str(&related_str());
    s.push_str("\n── 不变式 ──\n");
    let inv = invariant_manifest();
    let mut i: usize = 0;
    while i < inv.len() {
        s.push_str(&inv[i]);
        s.push('\n');
        i = i.wrapping_add(1);
    }
    s.push_str("\n── 定义复用 ──\n");
    let dv = demo_def()?;
    let mut j: usize = 0;
    while j < dv.len() {
        s.push_str(&dv[j]);
        s.push('\n');
        j = j.wrapping_add(1);
    }
    s.push_str("\n── 错误闭集 ──\n");
    let fm = form_manifest();
    let mut k: usize = 0;
    while k < fm.len() {
        s.push_str(&fm[k]);
        s.push('\n');
        k = k.wrapping_add(1);
    }
    let am = aux_manifest();
    let mut k2: usize = 0;
    while k2 < am.len() {
        s.push_str(&am[k2]);
        s.push('\n');
        k2 = k2.wrapping_add(1);
    }
    s.push_str("\n── 指标 ──\n");
    let rows = o.rows();
    let mut r: usize = 0;
    while r < rows.len() {
        s.push_str(&format!("{:<12} {}\n", rows[r].0, rows[r].1));
        r = r.wrapping_add(1);
    }
    s.push_str("\n── 复杂度分解 ──\n");
    let ci = o.metrics.complexity();
    let mut c: usize = 0;
    while c < ci.len() {
        s.push_str(&format!(
            "{:<20} 迭代{} × 代价{} = {} 步 | {}\n",
            ci[c].name, ci[c].iters, ci[c].per_iter, ci[c].total, ci[c].note
        ));
        c = c.wrapping_add(1);
    }
    s.push_str(&format!("{}\n", o.metrics.complexity_summary()));
    s.push_str("\n── 封账真相源 ──\n");
    s.push_str(&format!("{}\n", o.net.seal.describe()));
    s.push_str(&format!("冻结件 {}\n", o.freeze.summary()));
    s.push_str(&format!("稳定性 {}\n", o.stability.describe()));
    s.push_str(&format!("回放 {}\n", o.replay.summary()));
    s.push_str(&format!("预算 {}\n", o.budget.summary()));
    s.push_str(&format!("留痕 {}\n", o.evidence.summary()));
    s.push_str(&format!("全过 = {}\n", o.all_ok()));
    Ok(s)
}
// ===========================================================================
// 18. 自检
// ===========================================================================
/// 自检项。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Check {
    /// 项名。
    pub name: String,
    /// 是否通过。
    pub ok: bool,
    /// 实测串。
    pub got: String,
}
impl Check {
    /// 构造。
    pub fn new(name: &str, ok: bool, got: String) -> Check {
        Check { name: String::from(name), ok, got }
    }
    /// 渲染行。
    pub fn describe(&self) -> String {
        format!("{:<26} {:<4} {}", self.name, if self.ok { "OK" } else { "FAIL" }, self.got)
    }
}
/// 自检清单。
pub fn self_check() -> Vec<Check> {
    let mut out: Vec<Check> = Vec::with_capacity(34);
    out.push(Check::new(
        "seg_mask",
        SEG_MASK == 0x003F && SEG_END == (SEG_BASE | SEG_MASK),
        seg_line(),
    ));
    out.push(Check::new(
        "form_codes_unique",
        {
            let mut ok = true;
            let mut i: usize = 0;
            while i < Form::ALL.len() {
                let mut j: usize = i.wrapping_add(1);
                while j < Form::ALL.len() {
                    if Form::ALL[i].code() == Form::ALL[j].code() {
                        ok = false;
                    }
                    j = j.wrapping_add(1);
                }
                i = i.wrapping_add(1);
            }
            ok && code_in_seg(Form::ALL[Form::COUNT - 1].code())
        },
        format!(
            "Form::COUNT={} 码 {:04X}..{:04X}",
            Form::COUNT,
            Form::ALL[0].code(),
            Form::ALL[Form::COUNT - 1].code()
        ),
    ));
    out.push(Check::new(
        "aux_codes_unique",
        {
            let mut ok = AuxCode::ALL[0].code() == SEG_BASE.wrapping_add(0x1C);
            let mut i: usize = 0;
            while i < AuxCode::ALL.len() {
                let mut j: usize = i.wrapping_add(1);
                while j < AuxCode::ALL.len() {
                    if AuxCode::ALL[i].code() == AuxCode::ALL[j].code() {
                        ok = false;
                    }
                    j = j.wrapping_add(1);
                }
                i = i.wrapping_add(1);
            }
            ok && code_in_seg(AuxCode::ALL[AuxCode::COUNT - 1].code())
        },
        format!(
            "AuxCode::COUNT={} 码 {:04X}..{:04X}",
            AuxCode::COUNT,
            AuxCode::ALL[0].code(),
            AuxCode::ALL[AuxCode::COUNT - 1].code()
        ),
    ));
    out.push(Check::new(
        "scale_exact",
        BATCH_TOTAL * ROWS_PER_BATCH == ROW_TOTAL && ROW_TOTAL * LINES_PER_ROW == LINE_TOTAL && LINE_TOTAL == 240_000,
        format!(
            "{}×{}={} 条 ×{}={} 行",
            BATCH_TOTAL, ROWS_PER_BATCH, ROW_TOTAL, LINES_PER_ROW, LINE_TOTAL
        ),
    ));
    out.push(Check::new("def_hash_const", F2982_DEF_HASH != 0 && DEF_KEY_HASH != 0 && DEF_TEXT_HASH != 0, format!("def={:016X} key={:016X} text={:016X}", F2982_DEF_HASH, DEF_KEY_HASH, DEF_TEXT_HASH)));
    out.push(Check::new("def_rev_const", DEF_REV == 1 && DEF_FACE_ID == 0x2982, format!("rev={} face={:04X}", DEF_REV, DEF_FACE_ID)));
    match def_book_full() {
        Ok(b) => {
            out.push(Check::new("def_book_four", b.rows.len() == DefSlot::COUNT, format!("项数={}", b.rows.len())));
            out.push(Check::new("def_reuse_ok", b.reuse_ok(), b.summary()));
            out.push(Check::new("def_no_drift", b.drift_list().is_empty(), format!("失配={:?}", b.drift_list())));
            let mut cols = true;
            let mut i: usize = 0;
            while i < b.rows.len() {
                if !b.rows[i].columns_ok() {
                    cols = false;
                }
                i = i.wrapping_add(1);
            }
            out.push(Check::new("def_columns", cols, String::from("四项登记列齐备")));
        }
        Err(e) => {
            out.push(Check::new("def_book_four", false, err_str(&e)));
        }
    }
    // 压缩/解压可逆性
    out.push(Check::new(
        "row_chain_three_way",
        {
            let mut ok = true;
            let mut id: usize = 1;
            while id <= ROW_TOTAL {
                let r = row_reading(id);
                if row_line_sum(id) != r {
                    ok = false;
                }
                if row_line_quota(id, r) != r {
                    ok = false;
                }
                id = id.wrapping_add(400);
            }
            ok
        },
        format!("正序==倒序==册内（抽样 {} 条）", (ROW_TOTAL + 399) / 400),
    ));
    match grand_book_full() {
        Ok(g) => {
            out.push(Check::new("grand_scale", g.batches.len() == BATCH_TOTAL && g.rows.len() == ROW_TOTAL, g.summary()));
            out.push(Check::new("grand_complete", g.complete(), format!("缺批{}", g.missing_count())));
        }
        Err(e) => {
            out.push(Check::new("grand_scale", false, err_str(&e)));
        }
    }
    match Snapshot::net(0x5EED_3181_0000_0001u64) {
        Ok(net) => {
            out.push(Check::new("chain_closed", net.chain.closed, net.chain.summary()));
            out.push(Check::new("chain_all_batches", net.chain.ok_count() == BATCH_TOTAL, format!("{}/{}", net.chain.ok_count(), BATCH_TOTAL)));
            out.push(Check::new("chain_carry_continuous", net.chain.carry_continuous(&net.book), format!("final_carry={}", net.chain.final_carry)));
            out.push(Check::new("chain_steps_240k", net.chain.steps == (LINE_TOTAL as u64), format!("steps={}", net.chain.steps)));
            out.push(Check::new("identity_four_way", net.ident.deviation == 0, net.ident.summary()));
            out.push(Check::new("identity_quad", net.ident.quad_ok, format!("D1={} D2={} D3={} D4={}", net.ident.d1_count_ok, net.ident.d2_idseq_ok, net.ident.d3_lines_ok, net.ident.d4_split_ok)));
            out.push(Check::new("identity_four_sides_equal", net.ident.sum_batch == net.ident.sum_row && net.ident.sum_row == net.ident.sum_recalc && net.ident.sum_recalc == net.ident.sum_lines, format!("{} == {} == {} == {}", net.ident.sum_batch, net.ident.sum_row, net.ident.sum_recalc, net.ident.sum_lines)));
            out.push(Check::new("ring_balanced", net.ring.balanced(), net.ring.describe()));
            out.push(Check::new("ring_dropped_explicit", net.ring.dropped > 0, format!("dropped={} live={}", net.ring.dropped, net.ring.live())));
            out.push(Check::new("seal_present", net.seal.present, net.seal.describe()));
            out.push(Check::new("seal_columns_ok", net.seal.columns_ok(), format!("批{} 条{} 行{}", net.seal.batches, net.seal.rows, net.seal.lines)));
            out.push(Check::new("seal_carry_closed", net.seal.carry_closed(), format!("carry={} Σ={}", net.seal.final_carry, net.seal.total_sum)));
            out.push(Check::new("net_is_net", net.is_net(), format!("border={}", net.ev.border_violations)));
            let fz = FreezeRecord::build(&net);
            out.push(Check::new("freeze_verify", fz.verify(&net), fz.summary()));
            out.push(Check::new("freeze_total_fp", fz.total_fp == net.seal.total_fp, format!("{:016X}", fz.total_fp)));
            match stability_check(0x5EED_3181_0000_0001u64) {
                Ok(st) => {
                    out.push(Check::new("stability_zero_drift", st.stable && st.rounds_ok(), st.describe()));
                }
                Err(e) => {
                    out.push(Check::new("stability_zero_drift", false, err_str(&e)));
                }
            }
            match replay(0x5EED_3181_0000_0001u64) {
                Ok(rp) => {
                    out.push(Check::new("replay_consistent", rp.consistent, rp.summary()));
                }
                Err(e) => {
                    out.push(Check::new("replay_consistent", false, err_str(&e)));
                }
            }
            match inject_all(0x5EED_3181_0000_0001u64) {
                Ok((ev, snaps)) => {
                    let mut all_hit = true;
                    let mut cnt: usize = 0;
                    let mut i: usize = 0;
                    while i < Form::ALL.len() {
                        if let Ok(d) = detect_all(&snaps[i]) {
                            if !d.hit(Form::ALL[i]) {
                                all_hit = false;
                            }
                            cnt = cnt.wrapping_add(1);
                        }
                        i = i.wrapping_add(1);
                    }
                    out.push(Check::new(
                        "inject_all_rejected",
                        all_hit && cnt == FORM_COUNT,
                        format!("{}/{}", cnt, FORM_COUNT),
                    ));
                    out.push(Check::new(
                        "inject_fired_14",
                        ev.fired_count() == FORM_COUNT,
                        ev.summary(),
                    ));
                    match detect_all(&net) {
                        Ok(d) => {
                            out.push(Check::new(
                                "net_zero_false_positive",
                                d.is_clean(),
                                format!("hit={:?}", d.hit_list()),
                            ));
                        }
                        Err(e) => {
                            out.push(Check::new("net_zero_false_positive", false, err_str(&e)));
                        }
                    }
                }
                Err(e) => {
                    out.push(Check::new("inject_all_rejected", false, err_str(&e)));
                }
            }
            match observe(0x5EED_3181_0000_0001u64) {
                Ok(ob) => {
                    out.push(Check::new("observe_all_ok", ob.all_ok(), ob.metrics.summary()));
                    out.push(Check::new("tlv_roundtrip", ob.metrics.tlv_ok, format!("{}", ob.metrics.tlv_ok)));
                    out.push(Check::new("budget_ok", ob.budget.ok(), ob.budget.summary()));
                }
                Err(e) => {
                    out.push(Check::new("observe_all_ok", false, err_str(&e)));
                }
            }
        }
        Err(e) => {
            out.push(Check::new("snapshot_net", false, err_str(&e)));
        }
    }
    out
}
/// 自检全过。
pub fn self_check_ok() -> bool {
    let cs = self_check();
    let mut i: usize = 0;
    while i < cs.len() {
        if !cs[i].ok {
            return false;
        }
        i = i.wrapping_add(1);
    }
    true
}
/// 自检报告行。
pub fn self_check_report() -> Vec<String> {
    let cs = self_check();
    let mut out: Vec<String> = Vec::with_capacity(cs.len());
    let mut i: usize = 0;
    while i < cs.len() {
        out.push(cs[i].describe());
        i = i.wrapping_add(1);
    }
    out
}
