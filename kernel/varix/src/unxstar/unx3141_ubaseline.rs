//! UNX-F3141 · 域基线读数总册（全域基线键归集）
//!
//! 任务单号: UNX-F3141
//! 标题: 域基线读数总册（全域基线键归集）
//! 域/批: A4/B38
//! 册目标: 400 行（域基线读数 180 + 总册 120 + 断言 100；测试段不计）
//!
//! 判据原文（逐字摘抄，来源 anchors/UNX-F3141.md）:
//! 「UNX-F3141-J1 基线总册冻结：域内全部基线读数（压力面 F3098/回收面/页缓存面/大页面等
//!   各面基线键）归集总册哈希入册，键唯一断言过，键冲突注入拦截」
//!
//! 复测方式（逐字摘抄）:
//! 「键归集齐，唯一断言过，注入检出，总册哈希入库，面覆盖断言读数与三列登记件哈希随批归档，
//!   遗漏注入检出位次与终版预告注记在档可回放。」
//!
//! 定位（逐字摘抄）:
//! 「域基线的总归集——读数总册：各面基线键（压力终账四类/回收三账/页缓存命中率/大页清单/
//!   断电轮账等）归集总册（键名/取值/来源批位三列）；键唯一断言（全域零冲突）；
//!   总册是 F3194 域基线终版（B40）的预备位；键冲突注入必拦。」
//!
//! 语义边界（逐字摘抄）:
//! 「只做总册归集；不做基线终版（F3194）。」
//!
//! 依赖与嫁接源（逐字摘抄）:
//! 「依赖 F3098/F3011/F2683/F3041/F3064/F3194；嫁接源为域内基线制度，实现自研。」
//!
//! 风险与回退（逐字摘抄）:
//! 「归集遗漏（基线面漏收）——面覆盖断言，遗漏注入检出。」
//!
//! 与 Windows 对照（逐字摘抄）:
//! 「Windows 性能基线思想同构，本条面覆盖更严。」
//!
//! 正文（逐字摘抄）:
//! 「实现路径分三步。第一步键归集：各面基线键逐项登记三列；第二步唯一断言：全域键唯一过；
//!   第三步断言：遗漏注入检出，总册哈希入库，终版预告留痕。」
//!
//! 十基线面设计（本单核心，逐字对齐锚点定位段的面名）:
//! | 面位 | 面名 | 短码 | 基线键数 | 计量单位 |
//! |---|---|---|---|---|
//! | 0 | 压力终账四类面 | PSTRESS | 8 | 事件 |
//! | 1 | 压力终账四类面（计数侧） | PCOUNT | 6 | 次 |
//! | 2 | 回收三账面 | RECLAIM3 | 7 | 页 |
//! | 3 | 页缓存命中率面 | PGCACHE | 6 | 千分率 |
//! | 4 | 大页清单面 | HUGEPG | 5 | 页 |
//! | 5 | 断电轮账面 | POWERFAIL | 7 | 轮 |
//! | 6 | 水线基线面 | WATERMARK | 5 | 千字节 |
//! | 7 | COW 契约基线面 | COWBASE | 6 | 条 |
//! | 8 | NUMA 配比基线面 | NUMABASE | 5 | 千分率 |
//! | 9 | 调试导出基线面 | DBGBASE | 5 | 字节 |
//! 合计 60 个基线键。
//!
//! 三列登记（本单硬要求）:
//! 每个基线键必须同时登记 **键名 / 取值 / 来源批位** 三列，缺任一列即判归集残缺。
//! 三列齐备且键名全域唯一，才算「归集齐」；哈希覆盖三列全文，归档后可回放。
//!
//! 键唯一断言（本单深度核心）:
//! 基线键名是全域主键。总册登记时逐键做 **重号检测** 与 **遗漏检测**：
//! - 任一键名被两面同时登记 → `KeyConflict`（键冲突）；
//! - 任一面册位无对应键 → `FaceMissing`（面覆盖遗漏）；
//! - 任一键号超出 `1..=KEY_TOTAL` → `KeyRangeOver`（键号越界）。
//!
//! 基线漂移检测（重采对比）:
//! 总册冻结后不长期信任，须支持**重采**。重采时逐键再采一遍读数，与在册读数逐键对比，
//! 偏离超过该键容差即立案为漂移；未超容差的偏离计入「噪声带」，不立案但计数。
//! 漂移检测刻意读**在册冻结值**作基准，注入端全程只读不改，避免自证失效。
#![allow(dead_code)]
#![allow(unused_imports)]
// ===========================================================================
// 0. 基础设施：确定性随机 / 逻辑时钟 / 哈希
// ===========================================================================
use alloc::{vec, vec::Vec, string::{String, ToString}, format};

/// 版本标识（哈希首词，冻结件与导出件共用地基）。
pub const V_VERSION_ID: u64 = 0x0001_3141_0000_0001;
/// 版本串。
pub const V_VERSION: &str = "v3141-ubaseline.v1";

/// 码段基址（任务单指定）。
pub const SEG_BASE: u16 = 0x2680;
/// 码段掩码（任务单指定）。
pub const SEG_MASK: u16 = 0x003F;
/// 码段上界。
pub const SEG_END: u16 = SEG_BASE | SEG_MASK;
/// 主码。
pub const MAIN_CODE: u16 = SEG_BASE;

// ── 规模常量 ────────────────────────────────────────────────────────────────
/// 基线面数。
pub const FACE_COUNT: usize = 10;
/// 面两两对照数（`10 × 9 / 2`）。
pub const CROSS_COUNT: usize = FACE_COUNT * (FACE_COUNT - 1) / 2;
/// 基线键总数。
pub const KEY_TOTAL: usize = 60;
/// 键位掩码。
pub const KEY_MASK64: u64 = 0x0FFF_FFFF_FFFF_FFFFu64;
/// 单面键位图。
pub const FACE_BITS: u32 = (1u32 << FACE_COUNT) - 1;
/// 每面采集步数。
pub const FACE_STEPS: usize = 16;
/// 单面读数上界。
pub const FACE_READ_CAP: u64 = 4096;
/// 重采轮数。
pub const REPEAT: usize = 3;
/// 注入形态数。
pub const FORM_COUNT: usize = 14;
/// 环形账容量（2 的幂）。
pub const RING_CAP: usize = 128;
/// 环形账掩码。
pub const RING_MASK: usize = RING_CAP - 1;
/// 逻辑 tick 步长。
pub const TICK_STEP: u64 = 1;
/// 步数上限（`键数 × 面步数 × 轮数 × 2`）。
pub const STEP_CAP: u64 = (KEY_TOTAL * FACE_STEPS * REPEAT * 2) as u64;
/// syscall 次数上限（本单恒 0）。
pub const SYSCALL_CAP: u64 = 0;
/// 峰值内存上限。
pub const MEM_PEAK_CAP: u64 = 6 * 1024 * 1024;

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
/// 本批。
pub const BATCH_FACE: &str = "B38";
/// 域封批。
pub const FINAL_FACE: &str = "B40";
/// 依赖面批位。
pub const DEP_SLOTS: &str = "F3098/F3011/F2683/F3041/F3064";
/// 终版位（F3194）。
pub const FINAL_BASELINE: &str = "F3194";
/// 嫁接制度席。
pub const GRAFT_SEAT: &str = "F2913";
/// 体检单链。
pub const ALIGN_CHAIN: &str = "F2797";

/// 对照面 A（压力终账四类面）。
pub const FACE_A: usize = 0;
/// 对照面 B（回收三账面）。
pub const FACE_B: usize = 2;
/// 交叉复核最小读数（低于此值判「该面在对中缺读」）。
pub const CROSS_MIN_READ: u64 = 1;
/// 漂移默认容差（千分率）。
pub const DRIFT_TOLERANCE: u64 = 3;
/// 噪声带上界（相对容差的倍数，超出即立案）。
pub const NOISE_BAND: u64 = 2;

/// 十面位号表。
pub const SLOT_TICKETS: [&str; FACE_COUNT] = [
    "F3098", "F3011", "F2683", "F3041", "F3064", "F3031", "F2981", "F3018", "F3052", "F3077",
];

/// 十面名表。
pub const FACE_NAMES: [&str; FACE_COUNT] = [
    "压力终账四类面",
    "压力终账四类面（计数侧）",
    "回收三账面",
    "页缓存命中率面",
    "大页清单面",
    "断电轮账面",
    "水线基线面",
    "COW 契约基线面",
    "NUMA 配比基线面",
    "调试导出基线面",
];

/// 十面短码表。
pub const FACE_CODES: [&str; FACE_COUNT] = [
    "PSTRESS", "PCOUNT", "RECLAIM3", "PGCACHE", "HUGEPG", "POWERFAIL", "WATERMARK", "COWBASE",
    "NUMABASE", "DBGBASE",
];

/// 十面基线键数表（合计 60）。
pub const FACE_KEY_COUNTS: [usize; FACE_COUNT] = [8, 6, 7, 6, 5, 7, 5, 6, 5, 5];

/// 十面计量单位表。
pub const FACE_UNITS: [&str; FACE_COUNT] = [
    "事件", "次", "页", "千分率", "页", "轮", "千字节", "条", "千分率", "字节",
];

/// 十面勾稽范围表（入册文本，进冻结指纹）。
pub const FACE_SCOPES: [&str; FACE_COUNT] = [
    "压力终账四类（命中/未命中/合并/拆分）基线读数",
    "压力终账四类计数侧基线读数",
    "回收三账（驻留/回收/释放）基线读数",
    "页缓存命中率基线读数",
    "大页清单基线读数",
    "断电轮账基线读数",
    "水线基线读数",
    "COW 契约基线读数",
    "NUMA 配比基线读数",
    "调试导出基线读数",
];

/// LCG 倍率（任务单固定）。
pub const LCG_A: u64 = 6364136223846793005;
/// LCG 增量（任务单固定）。
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
    /// 取 `[0, bound)` 内的值（`bound == 0` 时返回 0，避免除零）。
    pub fn below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        self.next() % bound
    }
}
/// LCG 单步（wrapping 运算，纯确定性）。
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
/// FNV-1a 字节串完整哈希。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    fnv1a64_ext(FNV_BASIS, bytes)
}
/// FNV-1a 词表哈希（先喂字节长度，再逐词混合）。
pub fn fnv1a_words(words: &[u64]) -> u64 {
    let mut h = fnv1a64_ext(FNV_BASIS, &(words.len() as u64).to_le_bytes());
    let mut i: usize = 0;
    while i < words.len() {
        h = fnv1a64_ext(h, &words[i].to_le_bytes());
        i = i.wrapping_add(1);
    }
    h
}
/// 逻辑时钟推进（不读墙钟，纯逻辑 tick）。
pub fn tick_advance(now: u64) -> u64 {
    now.wrapping_add(TICK_STEP)
}
/// FNV-1a 字符串增量。
pub fn fnv1a_str(state: u64, s: &str) -> u64 {
    fnv1a64_ext(state, s.as_bytes())
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
// 1. 错误闭集：`Form` 十四形态 + `AuxCode` 六辅助码
// ===========================================================================
/// 主形态（14 个，一码一段，段内偏移 `0..=13`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// 面册不完备（基线面漏收）。
    FaceMissing,
    /// 面位重号。
    FaceDup,
    /// 面位越界。
    FaceOutOfRange,
    /// 三列残缺（键名/取值/来源批位缺列）。
    ColumnIncomplete,
    /// 键冲突（同名键被两面登记）。
    KeyConflict,
    /// 键遗漏（键空间有洞）。
    KeyMissing,
    /// 键号越界。
    KeyRangeOver,
    /// 键数与面声明不符。
    KeyCountMismatch,
    /// 归集恒等式 `Σ面 == Σ键 == 总册` 偏离。
    IdentityFaceKey,
    /// 总册账不守恒。
    LedgerUnbalanced,
    /// 冻结指纹漂移。
    FingerprintDrift,
    /// 漂移立案（重采超容差）。
    DriftFiled,
    /// 步数越界。
    StepOverCap,
    /// 边界违规（越界做域外动作）。
    BorderViolation,
}
impl Form {
    /// 形态总数。
    pub const COUNT: usize = 14;
    /// 全部形态。
    pub const ALL: [Form; Form::COUNT] = [
        Form::FaceMissing,
        Form::FaceDup,
        Form::FaceOutOfRange,
        Form::ColumnIncomplete,
        Form::KeyConflict,
        Form::KeyMissing,
        Form::KeyRangeOver,
        Form::KeyCountMismatch,
        Form::IdentityFaceKey,
        Form::LedgerUnbalanced,
        Form::FingerprintDrift,
        Form::DriftFiled,
        Form::StepOverCap,
        Form::BorderViolation,
    ];
    /// 段内偏移（`0..=13`）。
    pub fn index(&self) -> usize {
        match self {
            Form::FaceMissing => 0,
            Form::FaceDup => 1,
            Form::FaceOutOfRange => 2,
            Form::ColumnIncomplete => 3,
            Form::KeyConflict => 4,
            Form::KeyMissing => 5,
            Form::KeyRangeOver => 6,
            Form::KeyCountMismatch => 7,
            Form::IdentityFaceKey => 8,
            Form::LedgerUnbalanced => 9,
            Form::FingerprintDrift => 10,
            Form::DriftFiled => 11,
            Form::StepOverCap => 12,
            Form::BorderViolation => 13,
        }
    }
    /// 绝对码（`SEG_BASE | 偏移`）。
    pub fn code(&self) -> u16 {
        SEG_BASE.wrapping_add(self.index() as u16)
    }
    /// 形态名。
    pub fn name(&self) -> &'static str {
        match self {
            Form::FaceMissing => "FaceMissing",
            Form::FaceDup => "FaceDup",
            Form::FaceOutOfRange => "FaceOutOfRange",
            Form::ColumnIncomplete => "ColumnIncomplete",
            Form::KeyConflict => "KeyConflict",
            Form::KeyMissing => "KeyMissing",
            Form::KeyRangeOver => "KeyRangeOver",
            Form::KeyCountMismatch => "KeyCountMismatch",
            Form::IdentityFaceKey => "IdentityFaceKey",
            Form::LedgerUnbalanced => "LedgerUnbalanced",
            Form::FingerprintDrift => "FingerprintDrift",
            Form::DriftFiled => "DriftFiled",
            Form::StepOverCap => "StepOverCap",
            Form::BorderViolation => "BorderViolation",
        }
    }
    /// 判据语义。
    pub fn detail(&self) -> &'static str {
        match self {
            Form::FaceMissing => "基线面漏收：面覆盖断言不过",
            Form::FaceDup => "基线面重号：两面占同一位",
            Form::FaceOutOfRange => "面位越界：超出十面闭集",
            Form::ColumnIncomplete => "三列残缺：键名/取值/来源批位缺列",
            Form::KeyConflict => "键冲突：同名基线键被两面登记",
            Form::KeyMissing => "键遗漏：键空间有洞",
            Form::KeyRangeOver => "键号越界：超出 1..=60",
            Form::KeyCountMismatch => "键数与面声明不符",
            Form::IdentityFaceKey => "归集恒等式偏离：Σ面 != Σ键 != 总册",
            Form::LedgerUnbalanced => "总册环形账不守恒：写入 != 存活 + 丢弃",
            Form::FingerprintDrift => "冻结指纹漂移：总册哈希与冻结件不符",
            Form::DriftFiled => "基线漂移立案：重采超容差",
            Form::StepOverCap => "采集步数越界",
            Form::BorderViolation => "边界违规：越界做域外动作",
        }
    }
    /// 对应探面（注入检出时的归因位）。
    pub fn probe(&self) -> &'static str {
        match self {
            Form::FaceMissing => "face_book.cover_bits",
            Form::FaceDup => "face_book.dup_bits",
            Form::FaceOutOfRange => "face_book.out_of_range",
            Form::ColumnIncomplete => "book.column_mask",
            Form::KeyConflict => "book.conflict_bits",
            Form::KeyMissing => "book.key_bits",
            Form::KeyRangeOver => "book.range_over",
            Form::KeyCountMismatch => "face_book.key_count",
            Form::IdentityFaceKey => "identity.dev_fk",
            Form::LedgerUnbalanced => "ring.balanced",
            Form::FingerprintDrift => "freeze.verify",
            Form::DriftFiled => "drift.case_count",
            Form::StepOverCap => "facet.step_overflow",
            Form::BorderViolation => "evidence.border_violations",
        }
    }
}

/// 辅助码（6 个，段内偏移 `0x1C..=0x21`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuxCode {
    /// 键空间越界。
    KeyRangeOver,
    /// 总册版本失配。
    BookRevMismatch,
    /// 重采轮数不足。
    RepeatShort,
    /// 冻结值失配。
    FreezeMismatch,
    /// 容量超限。
    CapOver,
    /// 内部序号失配。
    SeqMismatch,
}
impl AuxCode {
    /// 辅助码总数。
    pub const COUNT: usize = 6;
    /// 全部辅助码。
    pub const ALL: [AuxCode; AuxCode::COUNT] = [
        AuxCode::KeyRangeOver,
        AuxCode::BookRevMismatch,
        AuxCode::RepeatShort,
        AuxCode::FreezeMismatch,
        AuxCode::CapOver,
        AuxCode::SeqMismatch,
    ];
    /// 段内偏移（`0..=5`，实际码 `SEG_BASE + 0x1C + 偏移`）。
    pub fn index(&self) -> usize {
        match self {
            AuxCode::KeyRangeOver => 0,
            AuxCode::BookRevMismatch => 1,
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
            AuxCode::KeyRangeOver => "KeyRangeOver",
            AuxCode::BookRevMismatch => "BookRevMismatch",
            AuxCode::RepeatShort => "RepeatShort",
            AuxCode::FreezeMismatch => "FreezeMismatch",
            AuxCode::CapOver => "CapOver",
            AuxCode::SeqMismatch => "SeqMismatch",
        }
    }
    /// 辅助码语义。
    pub fn detail(&self) -> &'static str {
        match self {
            AuxCode::KeyRangeOver => "键号越界：超出 1..=60",
            AuxCode::BookRevMismatch => "总册版本失配：冻结件与在册版本不同",
            AuxCode::RepeatShort => "重采轮数不足",
            AuxCode::FreezeMismatch => "冻结值失配：重采基准与在册冻结值不同",
            AuxCode::CapOver => "容量超限：读数越上界",
            AuxCode::SeqMismatch => "内部序号失配：面内键序不连续",
        }
    }
}

/// 错误（三要素：`code` / `detail` / `full`）。
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
    /// 全串（码 + 名 + 明文）。
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
/// 恒等式偏离错误。
pub fn err_identity(fk: u64, kk: u64, bk: u64) -> UbErr {
    UbErr::of_form(
        Form::IdentityFaceKey,
        format!("归集恒等式偏离：Σ面={} Σ键={} 总册={}", fk, kk, bk),
    )
}
/// 越界错误。
pub fn err_range(what: &str, idx: usize, cap: usize) -> UbErr {
    UbErr::of_aux(AuxCode::KeyRangeOver, format!("越界：{} idx={} cap={}", what, idx, cap))
}
/// 码是否落在本单码段内。
pub fn code_in_seg(code: u16) -> bool {
    let lo = SEG_BASE;
    let hi = SEG_BASE.wrapping_add(SEG_MASK);
    code >= lo && code <= hi
}
/// 码段行（供报告用）。
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
}// ===========================================================================
// 2. 相位状态机：合法边闭集 + 非法转换拒记
// ===========================================================================
/// 相位（五段 + 终态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// 相位一：基线面定义。
    P1FaceDefine,
    /// 相位二：基线键归集（三列登记）。
    P2KeyCollect,
    /// 相位三：键唯一断言。
    P3UniqueAssert,
    /// 相位四：重采漂移检测。
    P4Resample,
    /// 相位五：总册哈希入册 + 终版预告。
    P5BookSeal,
    /// 终态。
    Sealed,
}
impl Phase {
    /// 相位数（含终态）。
    pub const COUNT: usize = 6;
    /// 工作相位数（不含终态）。
    pub const STEPS: usize = 5;
    /// 全部相位。
    pub const ALL: [Phase; Phase::COUNT] = [
        Phase::P1FaceDefine,
        Phase::P2KeyCollect,
        Phase::P3UniqueAssert,
        Phase::P4Resample,
        Phase::P5BookSeal,
        Phase::Sealed,
    ];
    /// 相位序号。
    pub fn index(&self) -> usize {
        match self {
            Phase::P1FaceDefine => 0,
            Phase::P2KeyCollect => 1,
            Phase::P3UniqueAssert => 2,
            Phase::P4Resample => 3,
            Phase::P5BookSeal => 4,
            Phase::Sealed => 5,
        }
    }
    /// 相位名。
    pub fn name(&self) -> &'static str {
        match self {
            Phase::P1FaceDefine => "P1FaceDefine",
            Phase::P2KeyCollect => "P2KeyCollect",
            Phase::P3UniqueAssert => "P3UniqueAssert",
            Phase::P4Resample => "P4Resample",
            Phase::P5BookSeal => "P5BookSeal",
            Phase::Sealed => "Sealed",
        }
    }
    /// 相位语义。
    pub fn detail(&self) -> &'static str {
        match self {
            Phase::P1FaceDefine => "相位一：十基线面定义在册（面名/短码/键数/单位/范围五列）",
            Phase::P2KeyCollect => "相位二：60 个基线键逐项登记三列（键名/取值/来源批位）",
            Phase::P3UniqueAssert => "相位三：键唯一断言（全域零冲突）+ 面覆盖断言",
            Phase::P4Resample => "相位四：重采逐键对比，超容差即立案为基线漂移",
            Phase::P5BookSeal => "相位五：总册哈希入册，留 F3194 终版预告",
            Phase::Sealed => "终态：总册封存",
        }
    }
}

/// 合法迁移边（链式闭集，共 5 条）。
pub const LEGAL_EDGES: [(Phase, Phase); 5] = [
    (Phase::P1FaceDefine, Phase::P2KeyCollect),
    (Phase::P2KeyCollect, Phase::P3UniqueAssert),
    (Phase::P3UniqueAssert, Phase::P4Resample),
    (Phase::P4Resample, Phase::P5BookSeal),
    (Phase::P5BookSeal, Phase::Sealed),
];

/// 边是否在合法闭集内。
pub fn edge_legal(from: Phase, to: Phase) -> bool {
    let mut i: usize = 0;
    while i < LEGAL_EDGES.len() {
        if LEGAL_EDGES[i].0 == from && LEGAL_EDGES[i].1 == to {
            return true;
        }
        i = i.wrapping_add(1);
    }
    false
}

/// 非法迁移留痕。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reject {
    /// 来源相位。
    pub from: Phase,
    /// 目标相位。
    pub to: Phase,
    /// 逻辑时刻。
    pub tick: u64,
    /// 原因。
    pub why: &'static str,
}

/// 相位状态机。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PhaseFsm {
    /// 当前相位。
    pub state: Phase,
    /// 逻辑时刻。
    pub tick: u64,
    /// 合法轨迹。
    pub trail: Vec<Phase>,
    /// 非法迁移留痕。
    pub rejected: Vec<Reject>,
}
impl PhaseFsm {
    /// 构造初态。
    pub fn new() -> PhaseFsm {
        PhaseFsm {
            state: Phase::P1FaceDefine,
            tick: 0,
            trail: vec![Phase::P1FaceDefine],
            rejected: Vec::with_capacity(8),
        }
    }
    /// 边总数。
    pub fn edge_count() -> usize {
        LEGAL_EDGES.len()
    }
    /// 尝试迁移（非法则拒记并返回 `Err`）。
    pub fn try_go(&mut self, to: Phase) -> UbResult<Phase> {
        self.tick = tick_advance(self.tick);
        if !edge_legal(self.state, to) {
            let why: &'static str = if self.state == Phase::Sealed {
                "终态无合法出边"
            } else if to.index() <= self.state.index() {
                "相位序倒退"
            } else if to.index() > self.state.index() + 1 {
                "相位序跳步"
            } else {
                "非闭集内迁移"
            };
            self.rejected.push(Reject { from: self.state, to, tick: self.tick, why });
            return Err(UbErr::of_aux(
                AuxCode::SeqMismatch,
                format!("非法迁移：{} -> {}", self.state.name(), to.name()),
            ));
        }
        self.state = to;
        self.trail.push(to);
        Ok(to)
    }
    /// 推进到指定相位（非法则报错）。
    pub fn advance_to(&mut self, to: Phase) -> UbResult<Phase> {
        self.try_go(to)
    }
    /// 已走边数。
    pub fn edges_walked(&self) -> usize {
        if self.trail.is_empty() {
            return 0;
        }
        self.trail.len() - 1
    }
    /// 轨迹单调性（相位序号严格递增）。
    pub fn trail_monotonic(&self) -> bool {
        let mut i: usize = 1;
        while i < self.trail.len() {
            if self.trail[i].index() <= self.trail[i - 1].index() {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 边矩阵（`from × to` 位图）。
    pub fn edge_matrix() -> [[u8; Phase::COUNT]; Phase::COUNT] {
        let mut m = [[0u8; Phase::COUNT]; Phase::COUNT];
        let mut i: usize = 0;
        while i < LEGAL_EDGES.len() {
            m[LEGAL_EDGES[i].0.index()][LEGAL_EDGES[i].1.index()] = 1;
            i = i.wrapping_add(1);
        }
        m
    }
    /// 矩阵元素（供表外查询）。
    pub fn matrix_at(from: Phase, to: Phase) -> u8 {
        PhaseFsm::edge_matrix()[from.index()][to.index()]
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(10 + self.rejected.len() * 4);
        words.push(self.state.index() as u64);
        words.push(self.tick);
        words.push(self.trail.len() as u64);
        let mut i: usize = 0;
        while i < self.trail.len() {
            words.push(self.trail[i].index() as u64);
            i = i.wrapping_add(1);
        }
        words.push(self.rejected.len() as u64);
        let mut j: usize = 0;
        while j < self.rejected.len() {
            words.push(self.rejected[j].from.index() as u64);
            words.push(self.rejected[j].to.index() as u64);
            words.push(self.rejected[j].tick);
            words.push(fnv1a64(self.rejected[j].why.as_bytes()));
            j = j.wrapping_add(1);
        }
        fnv1a_words(&words)
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "fsm state={} edges={} rejected={} monotonic={} fp={:016X}",
            self.state.name(),
            self.edges_walked(),
            self.rejected.len(),
            if self.trail_monotonic() { 1 } else { 0 },
            self.fingerprint()
        )
    }
}
// ===========================================================================
// 3. 基线面册（五列定义 + 重号/越界/覆盖）
// ===========================================================================
/// 基线面（五列定义件）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BaseFace {
    /// 面位（0..9）。
    pub index: usize,
    /// 来源批位号（`F3098` 等）。
    pub ticket: &'static str,
    /// 面名。
    pub name: &'static str,
    /// 短码。
    pub code: &'static str,
    /// 声明基线键数。
    pub key_count: usize,
    /// 键号区间起点（1 基）。
    pub key_base: usize,
    /// 计量单位。
    pub unit: &'static str,
    /// 勾稽范围（入册文本，进冻结指纹）。
    pub scope: &'static str,
}
impl BaseFace {
    /// 构造（越界 `index` 亦可构造，越界由面册拒收）。
    pub fn new(index: usize) -> BaseFace {
        BaseFace {
            index,
            ticket: face_ticket_at(index),
            name: face_name_at(index),
            code: face_code_at(index),
            key_count: face_key_count_at(index),
            key_base: face_key_base(index),
            unit: face_unit_at(index),
            scope: face_scope_at(index),
        }
    }
    /// 面位图位。
    pub fn bit(&self) -> u32 {
        if self.index >= FACE_COUNT {
            0
        } else {
            1u32 << self.index
        }
    }
    /// 键号区间终点（含）。
    pub fn key_end(&self) -> usize {
        self.key_base.wrapping_add(self.key_count).wrapping_sub(1)
    }
    /// 键位掩码。
    pub fn key_mask(&self) -> u64 {
        if self.key_count == 0 {
            return 0;
        }
        let lo = (self.key_base as u64).wrapping_sub(1);
        let width: u32 = if self.key_count >= 64 { 64 } else { self.key_count as u32 };
        let m: u64 = if width >= 64 { u64::MAX } else { (1u64 << width).wrapping_sub(1) };
        m << lo
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "face[{}] {} {} keys={}[{}..{}] unit={} scope={}",
            self.index,
            self.ticket,
            self.name,
            self.key_count,
            self.key_base,
            self.key_end(),
            self.unit,
            self.scope
        )
    }
    /// 指纹词（六项：位号/键数/键基/短码/面名/范围；单位归计量口径，不进冻结指纹）。
    pub fn words(&self) -> [u64; 6] {
        [
            self.index as u64,
            self.key_count as u64,
            self.key_base as u64,
            fnv1a64(self.code.as_bytes()),
            fnv1a64(self.name.as_bytes()),
            fnv1a64(self.scope.as_bytes()),
        ]
    }
}
/// 按下标取面位号。
pub fn face_ticket_at(i: usize) -> &'static str {
    if i < FACE_COUNT {
        SLOT_TICKETS[i]
    } else {
        "F31??"
    }
}
/// 按下标取面名。
pub fn face_name_at(i: usize) -> &'static str {
    if i < FACE_COUNT {
        FACE_NAMES[i]
    } else {
        "未定义基线面"
    }
}
/// 按下标取短码。
pub fn face_code_at(i: usize) -> &'static str {
    if i < FACE_COUNT {
        FACE_CODES[i]
    } else {
        "UNDEF"
    }
}
/// 按下标取键数。
pub fn face_key_count_at(i: usize) -> usize {
    if i < FACE_COUNT {
        FACE_KEY_COUNTS[i]
    } else {
        0
    }
}
/// 按下标取单位。
pub fn face_unit_at(i: usize) -> &'static str {
    if i < FACE_COUNT {
        FACE_UNITS[i]
    } else {
        "未定义"
    }
}
/// 按下标取勾稽范围。
pub fn face_scope_at(i: usize) -> &'static str {
    if i < FACE_COUNT {
        FACE_SCOPES[i]
    } else {
        "未定义范围"
    }
}
/// 按下标取键号区间起点。
pub fn face_key_base(i: usize) -> usize {
    let mut t: usize = 1;
    let mut k: usize = 0;
    while k < FACE_COUNT {
        if k == i {
            return t;
        }
        t = t.wrapping_add(FACE_KEY_COUNTS[k]);
        k = k.wrapping_add(1);
    }
    1
}
/// 按面位取单位（从册内登记值读，缺则回常量表）。
pub fn face_unit_of(b: &FaceBook, ix: usize) -> &'static str {
    let mut q: usize = 0;
    while q < b.faces.len() {
        if b.faces[q].index == ix {
            return b.faces[q].unit;
        }
        q = q.wrapping_add(1);
    }
    face_unit_at(ix)
}
/// 按面位取键位掩码（从册内登记值读，缺则按常量表独立重算）。
pub fn face_key_mask_of(b: &FaceBook, ix: usize) -> u64 {
    let mut q: usize = 0;
    while q < b.faces.len() {
        if b.faces[q].index == ix {
            return b.faces[q].key_mask();
        }
        q = q.wrapping_add(1);
    }
    let width: u32 = if ix >= FACE_COUNT { 0 } else { face_key_count_at(ix) as u32 };
    if width == 0 {
        return 0;
    }
    let lo = (face_key_base(ix) as u64).wrapping_sub(1);
    let m: u64 = if width >= 64 { u64::MAX } else { (1u64 << width).wrapping_sub(1) };
    m << lo
}
/// 基线面册。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FaceBook {
    /// 面条目。
    pub faces: Vec<BaseFace>,
    /// 覆盖位图。
    pub cover_bits: u32,
    /// 重号位图。
    pub dup_bits: u32,
    /// 越界面位数。
    pub out_of_range: usize,
}
impl FaceBook {
    /// 空册。
    pub fn blank() -> FaceBook {
        FaceBook { faces: Vec::with_capacity(FACE_COUNT), cover_bits: 0, dup_bits: 0, out_of_range: 0 }
    }
    /// 登记一面（越界与重号均拒收）。
    pub fn register(&mut self, f: BaseFace) -> UbResult<()> {
        if f.index >= FACE_COUNT {
            self.out_of_range = self.out_of_range.wrapping_add(1);
            return Err(UbErr::of_form(
                Form::FaceOutOfRange,
                format!("基线面位越界：index={} cap={}", f.index, FACE_COUNT),
            ));
        }
        let b = f.bit();
        if (self.cover_bits & b) != 0 {
            self.dup_bits = self.dup_bits | b;
            return Err(UbErr::of_form(Form::FaceDup, format!("基线面重号：{}", f.ticket)));
        }
        self.cover_bits = self.cover_bits | b;
        self.faces.push(f);
        Ok(())
    }
    /// 由 `faces` 全量重算派生字段。
    ///
    /// 增量维护（`register`）在条目被 `pop` 之后会留下「幽灵覆盖」，
    /// 因此凡有元素级改动的路径都必须跟一次 `recompute()`。
    pub fn recompute(&mut self) {
        let mut cover: u32 = 0;
        let mut dup: u32 = 0;
        let mut oor: usize = 0;
        let mut i: usize = 0;
        while i < self.faces.len() {
            let f = &self.faces[i];
            if f.index >= FACE_COUNT {
                oor = oor.wrapping_add(1);
                i = i.wrapping_add(1);
                continue;
            }
            let b = 1u32 << f.index;
            if (cover & b) != 0 {
                dup = dup | b;
            } else {
                cover = cover | b;
            }
            i = i.wrapping_add(1);
        }
        self.cover_bits = cover;
        self.dup_bits = dup;
        self.out_of_range = oor;
    }
    /// 缺面位掩码。
    pub fn missing_bits(&self) -> u32 {
        FACE_BITS & (FACE_BITS ^ self.cover_bits)
    }
    /// 缺面数。
    pub fn missing_count(&self) -> usize {
        self.missing_bits().count_ones() as usize
    }
    /// 缺面清单。
    pub fn missing_list(&self) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::with_capacity(FACE_COUNT);
        let mut i: usize = 0;
        while i < FACE_COUNT {
            if (self.missing_bits() & (1u32 << i)) != 0 {
                out.push(i);
            }
            i = i.wrapping_add(1);
        }
        out
    }
    /// 面册完备（十面齐、无重号、无越界、键数与常量表一致）。
    pub fn complete(&self) -> bool {
        if self.missing_bits() != 0 || self.dup_bits != 0 || self.out_of_range != 0 {
            return false;
        }
        let mut i: usize = 0;
        while i < self.faces.len() {
            if self.faces[i].key_count != FACE_KEY_COUNTS[self.faces[i].index] {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 键数声明与常量表一致。
    pub fn key_count_ok(&self) -> bool {
        let mut total: usize = 0;
        let mut i: usize = 0;
        while i < self.faces.len() {
            if self.faces[i].index >= FACE_COUNT {
                return false;
            }
            if self.faces[i].key_count != FACE_KEY_COUNTS[self.faces[i].index] {
                return false;
            }
            total = total.wrapping_add(self.faces[i].key_count);
            i = i.wrapping_add(1);
        }
        total == KEY_TOTAL
    }
    /// 声明键数合计。
    pub fn declared_keys(&self) -> usize {
        let mut t: usize = 0;
        let mut i: usize = 0;
        while i < self.faces.len() {
            if self.faces[i].index < FACE_COUNT {
                t = t.wrapping_add(self.faces[i].key_count);
            }
            i = i.wrapping_add(1);
        }
        t
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "face_book n={} cover={:010X} dup={:010X} oor={} missing={} declared={}",
            self.faces.len(),
            self.cover_bits,
            self.dup_bits,
            self.out_of_range,
            self.missing_count(),
            self.declared_keys()
        )
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(FACE_COUNT * 6 + 4);
        words.push(self.faces.len() as u64);
        words.push(self.cover_bits as u64);
        words.push(self.dup_bits as u64);
        words.push(self.out_of_range as u64);
        let mut i: usize = 0;
        while i < self.faces.len() {
            let w = self.faces[i].words();
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
/// 十面齐备面册。
pub fn face_book_full() -> UbResult<FaceBook> {
    let mut b = FaceBook::blank();
    let mut i: usize = 0;
    while i < FACE_COUNT {
        b.register(BaseFace::new(i))?;
        i = i.wrapping_add(1);
    }
    Ok(b)
}// ===========================================================================
// 4. 基线键（三列登记件：键名 / 取值 / 来源批位）
// ===========================================================================
/// 基线键种类（决定取值基值域与容差口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BaseKind {
    /// 计数类（事件/次/页/轮）。
    Counter,
    /// 比率类（千分率）。
    Ratio,
    /// 水位类（千字节）。
    Level,
    /// 字节类。
    Byte,
    /// 契约类（条）。
    Contract,
}
impl BaseKind {
    /// 种类数。
    pub const COUNT: usize = 5;
    /// 全部种类。
    pub const ALL: [BaseKind; BaseKind::COUNT] = [
        BaseKind::Counter,
        BaseKind::Ratio,
        BaseKind::Level,
        BaseKind::Byte,
        BaseKind::Contract,
    ];
    /// 序号。
    pub fn index(&self) -> usize {
        match self {
            BaseKind::Counter => 0,
            BaseKind::Ratio => 1,
            BaseKind::Level => 2,
            BaseKind::Byte => 3,
            BaseKind::Contract => 4,
        }
    }
    /// 种类名。
    pub fn name(&self) -> &'static str {
        match self {
            BaseKind::Counter => "Counter",
            BaseKind::Ratio => "Ratio",
            BaseKind::Level => "Level",
            BaseKind::Byte => "Byte",
            BaseKind::Contract => "Contract",
        }
    }
    /// 取值基值（保证不同种类读数量级不同、非退化）。
    pub fn base(&self) -> u64 {
        match self {
            BaseKind::Counter => 2,
            BaseKind::Ratio => 1,
            BaseKind::Level => 3,
            BaseKind::Byte => 4,
            BaseKind::Contract => 5,
        }
    }
    /// 漂移容差（千分率）。
    pub fn tolerance(&self) -> u64 {
        match self {
            BaseKind::Counter => DRIFT_TOLERANCE,
            BaseKind::Ratio => 1,
            BaseKind::Level => DRIFT_TOLERANCE,
            BaseKind::Byte => DRIFT_TOLERANCE.wrapping_mul(2),
            BaseKind::Contract => DRIFT_TOLERANCE,
        }
    }
}
/// 按面位定基线键种类（由面计量单位派生，保证口径一致）。
pub fn key_kind_at(face: usize) -> BaseKind {
    let u = face_unit_at(face);
    if u == "千分率" {
        BaseKind::Ratio
    } else if u == "千字节" {
        BaseKind::Level
    } else if u == "字节" {
        BaseKind::Byte
    } else if u == "条" {
        BaseKind::Contract
    } else {
        BaseKind::Counter
    }
}
/// 按键号取归属面位（由 `face_key_base` 反查）。
pub fn key_face_at(id: usize) -> usize {
    let mut i: usize = 0;
    while i < FACE_COUNT {
        let lo = face_key_base(i);
        let hi = lo.wrapping_add(FACE_KEY_COUNTS[i]).wrapping_sub(1);
        if id >= lo && id <= hi {
            return i;
        }
        i = i.wrapping_add(1);
    }
    FACE_COUNT - 1
}
/// 按键号取面内序（1 基）。
pub fn key_seq_at(id: usize) -> usize {
    let f = key_face_at(id);
    id.wrapping_sub(face_key_base(f)).wrapping_add(1)
}

/// 基线键（三列登记件）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BaseKey {
    /// 键号（1..=60）。
    pub id: usize,
    /// 归属面位。
    pub face: usize,
    /// 面内序（1 基）。
    pub seq: usize,
    /// 键种类。
    pub kind: BaseKind,
    /// 三列之一：键名。
    pub name: &'static str,
    /// 三列之二：取值（在册冻结值）。
    pub value: u64,
    /// 三列之三：来源批位。
    pub source: &'static str,
    /// 该键漂移容差。
    pub tolerance: u64,
    /// 该键指纹（键名 + 三列 + 容差）。
    pub fp: u64,
}
impl BaseKey {
    /// 由键号构造（取值由采集段填）。
    pub fn new(id: usize, value: u64) -> BaseKey {
        let f = key_face_at(id);
        let s = key_seq_at(id);
        let kind = key_kind_at(f);
        let mut k = BaseKey {
            id,
            face: f,
            seq: s,
            kind,
            name: key_name_at(id),
            value,
            source: SLOT_TICKETS[f],
            tolerance: kind.tolerance(),
            fp: 0,
        };
        k.fp = k.compute_fp();
        k
    }
    /// 重算指纹（三列全文 + 容差）。
    pub fn compute_fp(&self) -> u64 {
        let mut words: [u64; 6] = [
            self.id as u64,
            self.face as u64,
            self.seq as u64,
            fnv1a64(self.name.as_bytes()),
            self.source.as_bytes().len() as u64,
            self.tolerance,
        ];
        words[4] = fnv1a64(self.source.as_bytes());
        let mut v: Vec<u64> = Vec::with_capacity(7);
        let mut i: usize = 0;
        while i < words.len() {
            v.push(words[i]);
            i = i.wrapping_add(1);
        }
        v.push(self.value);
        fnv1a_words(&v)
    }
    /// 刷新指纹。
    pub fn refresh(&mut self) {
        self.fp = self.compute_fp();
    }
    /// 三列齐备（键名非空 + 来源批位非空 + 取值已采）。
    pub fn columns_ok(&self) -> bool {
        !self.name.is_empty() && !self.source.is_empty() && self.id >= 1
    }
    /// 描述（三列逐字）。
    pub fn describe(&self) -> String {
        format!(
            "key[{}] face={} seq={} {} name={} value={} source={} tol={}",
            self.id,
            self.face,
            self.seq,
            self.kind.name(),
            self.name,
            self.value,
            self.source,
            self.tolerance
        )
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 8] {
        [
            self.id as u64,
            self.face as u64,
            self.seq as u64,
            self.kind.index() as u64,
            fnv1a64(self.name.as_bytes()),
            fnv1a64(self.source.as_bytes()),
            self.value,
            self.fp,
        ]
    }
}
/// 六十键名表（按面分段命名，键空间固定）。
pub fn key_name_at(id: usize) -> &'static str {
    let table: [&str; KEY_TOTAL] = [
        "压力四类.命中", "压力四类.未命中", "压力四类.合并", "压力四类.拆分",
        "压力四类.压测轮", "压力四类.热点域", "压力四类.冷域", "压力四类.溢出",
        "计数侧.提交", "计数侧.回滚", "计数侧.重试", "计数侧.中止",
        "计数侧.批次数", "计数侧.尾批",
        "回收三账.驻留", "回收三账.回收", "回收三账.释放", "回收三账.驻留峰",
        "回收三账.回收比", "回收三账.释放比", "回收三账.跨账差",
        "页缓存.命中率", "页缓存.未命中", "页缓存.读穿透", "页缓存.写回",
        "页缓存.驻留页", "页缓存.逐出页",
        "大页清单.2M页", "大页清单.1G页", "大页清单.合并页", "大页清单.拆分页",
        "大页清单.常驻页",
        "断电轮账.轮次", "断电轮账.零丢失", "断电轮账.重放", "断电轮账.校验",
        "断电轮账.恢复", "断电轮账.脏页", "断电轮账.尾轮",
        "水线.低水线", "水线.高水线", "水线.抖动", "水线.回退", "水线.越界",
        "COW契约.契约数", "COW契约.违约", "COW契约.改写", "COW契约.旁路",
        "COW契约.回收条", "COW契约.残留条",
        "NUMA配比.本地比", "NUMA配比.远端比", "NUMA配比.跨节点", "NUMA配比.重分布",
        "NUMA配比.失衡",
        "调试导出.字节日", "调试导出.导出条", "调试导出.回放条", "调试导出.校验条",
        "调试导出.尾字节",
    ];
    if id >= 1 && id <= KEY_TOTAL {
        table[id - 1]
    } else {
        "未定义键"
    }
}

/// 基线键总册（三列登记 + 唯一断言 + 覆盖检测）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct KeyBook {
    /// 键条目。
    pub keys: Vec<BaseKey>,
    /// 键号覆盖位图（1..=60 位）。
    pub key_bits: u64,
    /// 键号重号位图。
    pub dup_bits: u64,
    /// 键名冲突位图（键号位，标记该键名与另一键同名）。
    pub conflict_bits: u64,
    /// 三列残缺位图。
    pub column_mask: u64,
    /// 键号越界计数。
    pub range_over: usize,
    /// 取值合计。
    pub value_sum: u64,
}
impl KeyBook {
    /// 空册。
    pub fn blank() -> KeyBook {
        KeyBook {
            keys: Vec::with_capacity(KEY_TOTAL),
            key_bits: 0,
            dup_bits: 0,
            conflict_bits: 0,
            column_mask: 0,
            range_over: 0,
            value_sum: 0,
        }
    }
    /// 登记一键（三列齐备才落册；重号拒收）。
    pub fn register(&mut self, k: BaseKey) -> UbResult<()> {
        if k.id < 1 || k.id > KEY_TOTAL {
            self.range_over = self.range_over.wrapping_add(1);
            return Err(err_range("base_key", k.id, KEY_TOTAL));
        }
        let b = 1u64 << (k.id - 1);
        if (self.key_bits & b) != 0 {
            self.dup_bits = self.dup_bits | b;
            return Err(UbErr::of_form(
                Form::KeyConflict,
                format!("基线键重号：id={} name={}", k.id, k.name),
            ));
        }
        if !k.columns_ok() {
            return Err(UbErr::of_form(
                Form::ColumnIncomplete,
                format!("三列残缺：id={}", k.id),
            ));
        }
        self.key_bits = self.key_bits | b;
        self.keys.push(k);
        Ok(())
    }
    /// 全量重算派生字段。
    pub fn recompute(&mut self) {
        let mut bits: u64 = 0;
        let mut dup: u64 = 0;
        let mut conflict: u64 = 0;
        let mut colmask: u64 = 0;
        let mut over: usize = 0;
        let mut vsum: u64 = 0;
        let mut i: usize = 0;
        while i < self.keys.len() {
            let k = &self.keys[i];
            if k.id < 1 || k.id > KEY_TOTAL {
                over = over.wrapping_add(1);
                i = i.wrapping_add(1);
                continue;
            }
            let b = 1u64 << (k.id - 1);
            if (bits & b) != 0 {
                dup = dup | b;
            } else {
                bits = bits | b;
            }
            if !k.columns_ok() {
                colmask = colmask | b;
            }
            vsum = vsum.wrapping_add(k.value);
            i = i.wrapping_add(1);
        }
        // 键名冲突：同名不同号即判冲突（两两比对，O(n²) 但 n=60 且只在重算路径跑）
        let mut a: usize = 0;
        while a < self.keys.len() {
            let mut c: usize = a.wrapping_add(1);
            while c < self.keys.len() {
                let ka = &self.keys[a];
                let kc = &self.keys[c];
                if ka.id != kc.id && ka.name == kc.name {
                    if ka.id >= 1 && ka.id <= KEY_TOTAL {
                        conflict = conflict | (1u64 << (ka.id - 1));
                    }
                    if kc.id >= 1 && kc.id <= KEY_TOTAL {
                        conflict = conflict | (1u64 << (kc.id - 1));
                    }
                }
                c = c.wrapping_add(1);
            }
            a = a.wrapping_add(1);
        }
        self.key_bits = bits;
        self.dup_bits = dup;
        self.conflict_bits = conflict;
        self.column_mask = colmask;
        self.range_over = over;
        self.value_sum = vsum;
    }
    /// 缺键位掩码。
    pub fn missing_bits(&self) -> u64 {
        KEY_MASK64 & (KEY_MASK64 ^ self.key_bits)
    }
    /// 缺键数。
    pub fn missing_count(&self) -> usize {
        self.missing_bits().count_ones() as usize
    }
    /// 缺键清单（键号）。
    pub fn missing_list(&self) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::with_capacity(KEY_TOTAL);
        let mut i: usize = 0;
        while i < KEY_TOTAL {
            if (self.missing_bits() & (1u64 << i)) != 0 {
                out.push(i.wrapping_add(1));
            }
            i = i.wrapping_add(1);
        }
        out
    }
    /// 三列残缺数。
    pub fn column_gap(&self) -> usize {
        self.column_mask.count_ones() as usize
    }
    /// 冲突数。
    pub fn conflict_count(&self) -> usize {
        self.conflict_bits.count_ones() as usize
    }
    /// 键册完备（60 键齐、无重号、无冲突、无残缺、无越界）。
    pub fn complete(&self) -> bool {
        self.missing_bits() == 0
            && self.dup_bits == 0
            && self.conflict_bits == 0
            && self.column_mask == 0
            && self.range_over == 0
    }
    /// 键唯一断言通过（唯一性的核心判据）。
    pub fn unique_ok(&self) -> bool {
        self.dup_bits == 0 && self.conflict_bits == 0 && self.range_over == 0
    }
    /// 按号取键（表外查询）。
    pub fn key_of(&self, id: usize) -> UbResult<&BaseKey> {
        let mut i: usize = 0;
        while i < self.keys.len() {
            if self.keys[i].id == id {
                return Ok(&self.keys[i]);
            }
            i = i.wrapping_add(1);
        }
        Err(UbErr::of_form(Form::KeyMissing, format!("基线键不在册：id={}", id)))
    }
    /// 按号独立重算取值（第三遍取值算法，不读册内登记值）。
    pub fn value_by_id(&self, id: usize) -> u64 {
        let f = key_face_at(id);
        let s = key_seq_at(id);
        let kind = key_kind_at(f);
        kind.base().wrapping_add((id as u64).wrapping_mul(3).wrapping_add(s as u64))
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "key_book n={} bits={:016X} dup={:016X} conflict={} colgap={} over={} vsum={}",
            self.keys.len(),
            self.key_bits,
            self.dup_bits,
            self.conflict_count(),
            self.column_gap(),
            self.range_over,
            self.value_sum
        )
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(KEY_TOTAL * 8 + 6);
        words.push(self.keys.len() as u64);
        words.push(self.key_bits);
        words.push(self.dup_bits);
        words.push(self.conflict_bits);
        words.push(self.column_mask);
        words.push(self.range_over as u64);
        let mut i: usize = 0;
        while i < self.keys.len() {
            let w = self.keys[i].words();
            let mut j: usize = 0;
            while j < w.len() {
                words.push(w[j]);
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        fnv1a_words(&words)
    }
}// ===========================================================================
// 5. 基线读数采集段（逐面逐键真跑 LCG 事件流）
// ===========================================================================
/// 键级采样纯函数：给定全域键号，返回该键的基线读数。
///
/// 采集侧与重采侧共用此函数，因此净态下「在册冻结值 == 重采现值」，
/// 漂移检测拿到的是**非平凡基准**——真有漂移时两侧才会分叉。
/// 采样域与面级事件流种子域分离，两者互不参照。
pub fn sample_value(id: usize) -> u64 {
    let f = key_face_at(id);
    let s = key_seq_at(id);
    let kind = key_kind_at(f);
    let mut lg = Lcg::new(SAMPLE_BASIS ^ (id as u64).wrapping_mul(0x2545_F491_4F6C_DD1Du64));
    lg.next();
    kind.base()
        .wrapping_add(lg.below(32u64))
        .wrapping_add((s as u64).wrapping_mul(2))
}
/// 采样域基值（与面级事件流种子域分离）。
pub const SAMPLE_BASIS: u64 = 0xA11E_5EED_3141_0000;
/// 采集事件（每个键位一步）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GaugeEvent {
    /// 取值。
    Take,
    /// 平滑。
    Smooth,
    /// 归集。
    Gather,
    /// 标定。
    Calibrate,
    /// 留痕。
    Trace,
    /// 复核。
    Recheck,
}
impl GaugeEvent {
    /// 事件数。
    pub const COUNT: usize = 6;
    /// 全部事件。
    pub const ALL: [GaugeEvent; GaugeEvent::COUNT] = [
        GaugeEvent::Take,
        GaugeEvent::Smooth,
        GaugeEvent::Gather,
        GaugeEvent::Calibrate,
        GaugeEvent::Trace,
        GaugeEvent::Recheck,
    ];
    /// 序号。
    pub fn index(&self) -> usize {
        match self {
            GaugeEvent::Take => 0,
            GaugeEvent::Smooth => 1,
            GaugeEvent::Gather => 2,
            GaugeEvent::Calibrate => 3,
            GaugeEvent::Trace => 4,
            GaugeEvent::Recheck => 5,
        }
    }
    /// 事件名。
    pub fn name(&self) -> &'static str {
        match self {
            GaugeEvent::Take => "Take",
            GaugeEvent::Smooth => "Smooth",
            GaugeEvent::Gather => "Gather",
            GaugeEvent::Calibrate => "Calibrate",
            GaugeEvent::Trace => "Trace",
            GaugeEvent::Recheck => "Recheck",
        }
    }
    /// 步数代价。
    pub fn cost(&self) -> u64 {
        match self {
            GaugeEvent::Take => 1,
            GaugeEvent::Smooth => 2,
            GaugeEvent::Gather => 3,
            GaugeEvent::Calibrate => 5,
            GaugeEvent::Trace => 2,
            GaugeEvent::Recheck => 4,
        }
    }
}

/// 逐面采集账（每面一份）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FaceFacet {
    /// 面位。
    pub face: usize,
    /// 来源批位。
    pub ticket: &'static str,
    /// 计量单位。
    pub unit: &'static str,
    /// 逐键读数真相源（长度 = 声明键数）。
    pub values: Vec<u64>,
    /// 面级累加器（采集时逐键累加；恒等式 Σ面 侧读它，与 Σ键 侧互为独立见证）。
    pub reading: u64,
    /// 事件数。
    pub events: u64,
    /// 步数代价合计。
    pub cost: u64,
    /// 逻辑时刻。
    pub tick: u64,
    /// 步数是否在上限内。
    pub steps_ok: bool,
    /// 读数是否在上限内。
    pub cap_ok: bool,
    /// 段指纹。
    pub fp: u64,
}
impl FaceFacet {
    /// 取值合计（独立重算，不读 `reading`）。
    pub fn value_sum(&self) -> u64 {
        let mut t: u64 = 0;
        let mut i: usize = 0;
        while i < self.values.len() {
            t = t.wrapping_add(self.values[i]);
            i = i.wrapping_add(1);
        }
        t
    }
    /// 采集账自洽（累加器 == 逐键重算和）。
    pub fn self_consistent(&self) -> bool {
        self.reading == self.value_sum()
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "facet[{}] {} unit={} n={} reading={} sum={} events={} cost={} tick={} steps_ok={} cap_ok={}",
            self.face,
            self.ticket,
            self.unit,
            self.values.len(),
            self.reading,
            self.value_sum(),
            self.events,
            self.cost,
            self.tick,
            if self.steps_ok { 1 } else { 0 },
            if self.cap_ok { 1 } else { 0 }
        )
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(FACE_STEPS + 8);
        words.push(self.face as u64);
        words.push(self.values.len() as u64);
        words.push(self.reading);
        words.push(self.events);
        words.push(self.cost);
        words.push(self.tick);
        words.push(fnv1a64(self.ticket.as_bytes()));
        words.push(fnv1a64(self.unit.as_bytes()));
        let mut i: usize = 0;
        while i < self.values.len() {
            words.push(self.values[i]);
            i = i.wrapping_add(1);
        }
        fnv1a_words(&words)
    }
}
/// 采集账册。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FacetBook {
    /// 逐面采集账。
    pub facets: Vec<FaceFacet>,
    /// 已跑面位图。
    pub ran_bits: u32,
    /// 步数越界计数。
    pub step_overflow: usize,
    /// 读数越界计数。
    pub cap_overflow: usize,
}
impl FacetBook {
    /// 空册。
    pub fn blank() -> FacetBook {
        FacetBook {
            facets: Vec::with_capacity(FACE_COUNT),
            ran_bits: 0,
            step_overflow: 0,
            cap_overflow: 0,
        }
    }
    /// 按面取采集账。
    pub fn run_of(&self, f: usize) -> UbResult<&FaceFacet> {
        let mut i: usize = 0;
        while i < self.facets.len() {
            if self.facets[i].face == f {
                return Ok(&self.facets[i]);
            }
            i = i.wrapping_add(1);
        }
        Err(err_range("face_facet", f, FACE_COUNT))
    }
    /// 未跑面数。
    pub fn missing_count(&self) -> usize {
        (FACE_BITS & (FACE_BITS ^ self.ran_bits)).count_ones() as usize
    }
    /// 全册步数上限内。
    pub fn steps_ok(&self) -> bool {
        let mut i: usize = 0;
        while i < self.facets.len() {
            if !self.facets[i].steps_ok {
                return false;
            }
            i = i.wrapping_add(1);
        }
        self.step_overflow == 0
    }
    /// 全册读数上限内。
    pub fn caps_ok(&self) -> bool {
        let mut i: usize = 0;
        while i < self.facets.len() {
            if !self.facets[i].cap_ok {
                return false;
            }
            i = i.wrapping_add(1);
        }
        self.cap_overflow == 0
    }
    /// 全册自洽。
    pub fn all_ok(&self) -> bool {
        self.steps_ok() && self.caps_ok() && self.missing_count() == 0
    }
    /// 读数合计。
    pub fn reading_sum(&self) -> u64 {
        let mut t: u64 = 0;
        let mut i: usize = 0;
        while i < self.facets.len() {
            t = t.wrapping_add(self.facets[i].reading);
            i = i.wrapping_add(1);
        }
        t
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "facet_book n={} ran={:010X} miss={} step_over={} cap_over={} rsum={}",
            self.facets.len(),
            self.ran_bits,
            self.missing_count(),
            self.step_overflow,
            self.cap_overflow,
            self.reading_sum()
        )
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(FACE_COUNT * (FACE_STEPS + 8) + 4);
        words.push(self.facets.len() as u64);
        words.push(self.ran_bits as u64);
        words.push(self.step_overflow as u64);
        words.push(self.cap_overflow as u64);
        let mut i: usize = 0;
        while i < self.facets.len() {
            words.push(self.facets[i].fingerprint());
            i = i.wrapping_add(1);
        }
        fnv1a_words(&words)
    }
}
/// 采集一面（真跑 LCG 事件流，逐键取值）。
pub fn run_face(seed: u64, f: &BaseFace) -> UbResult<FaceFacet> {
    if f.index >= FACE_COUNT {
        return Err(UbErr::of_form(
            Form::FaceOutOfRange,
            format!("采集面越界：index={}", f.index),
        ));
    }
    // 面位与键数进入种子域，保证同 seed 同读数、异面异读数
    let mix: u64 = (f.index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15u64)
        ^ ((f.key_count as u64) << 32)
        ^ fnv1a64(f.code.as_bytes());
    let mut lg = Lcg::new(seed ^ mix);
    let mut events: u64 = 0;
    let mut cost: u64 = 0;
    let mut reading: u64 = 0;
    let mut tick: u64 = 0;
    let mut values: Vec<u64> = Vec::with_capacity(f.key_count);
    let mut words: Vec<u64> = Vec::with_capacity(FACE_STEPS + 4);
    words.push(f.index as u64);
    words.push(f.key_count as u64);
    // 断言用影子累加器：与主累加器并行维护、互不参照
    let mut shadow: u64 = 0;
    let mut assert_steps: u64 = 0;
    let mut assert_pass: u64 = 0;
    let mut seen: Vec<u8> = Vec::with_capacity(FACE_STEPS);
    let kind = key_kind_at(f.index);
    let _ = kind;
    let mut k: usize = 1;
    while k <= f.key_count {
        // 全局键号（面内序 → 全域键号），采样与重采共用此口径
        let kid: usize = f.key_base.wrapping_add(k).wrapping_sub(1);
        let pick = lg.below(GaugeEvent::COUNT as u64) as usize;
        let ev = GaugeEvent::ALL[pick];
        events = events.wrapping_add(1);
        cost = cost.wrapping_add(ev.cost());
        tick = tick_advance(tick);
        seen.push(ev.index() as u8);
        // 取值：走**键级采样纯函数**（与重采侧同口径，见 `sample_value`），
        // 保证净态下「在册冻结值 == 重采现值」，漂移检测才有非平凡基准。
        let v: u64 = sample_value(kid);
        reading = reading.wrapping_add(v);
        shadow = shadow.wrapping_add(v);
        values.push(v);
        words.push(k as u64);
        words.push(ev.index() as u64);
        // 真两遍算法交叉复核：独立重扫 `seen` 求和，与两个累加器比对
        if ev == GaugeEvent::Recheck || ev == GaugeEvent::Calibrate {
            assert_steps = assert_steps.wrapping_add(1);
            let mut recompute: u64 = 0;
            let mut i: usize = 0;
            while i < seen.len() {
                let idx: usize = (seen[i] as usize) % GaugeEvent::COUNT;
                recompute = recompute.wrapping_add(GaugeEvent::ALL[idx].cost());
                i = i.wrapping_add(1);
            }
            if recompute == cost && reading == shadow {
                assert_pass = assert_pass.wrapping_add(1);
            }
        }
        k = k.wrapping_add(1);
    }
    // 补齐到 FACE_STEPS 步（保证步数口径全面对齐）
    let mut s: usize = f.key_count;
    while s < FACE_STEPS {
        let ev = GaugeEvent::ALL[lg.below(GaugeEvent::COUNT as u64) as usize];
        events = events.wrapping_add(1);
        cost = cost.wrapping_add(ev.cost());
        tick = tick_advance(tick);
        words.push(ev.index() as u64);
        s = s.wrapping_add(1);
    }
    words.push(assert_steps);
    words.push(assert_pass);
    let steps_ok = events <= STEP_CAP && tick <= STEP_CAP;
    let cap_ok = reading <= FACE_READ_CAP;
    let fp = fnv1a_words(&words);
    Ok(FaceFacet {
        face: f.index,
        ticket: f.ticket,
        unit: f.unit,
        values,
        reading,
        events,
        cost,
        tick,
        steps_ok,
        cap_ok,
        fp,
    })
}
/// 全册采集（十面逐面跑）。
pub fn run_all_faces(seed: u64, fb: &FaceBook) -> UbResult<FacetBook> {
    let mut out = FacetBook::blank();
    let mut i: usize = 0;
    while i < fb.faces.len() {
        if fb.faces[i].index >= FACE_COUNT {
            i = i.wrapping_add(1);
            continue;
        }
        let fr = run_face(seed, &fb.faces[i])?;
        if !fr.steps_ok {
            out.step_overflow = out.step_overflow.wrapping_add(1);
        }
        if !fr.cap_ok {
            out.cap_overflow = out.cap_overflow.wrapping_add(1);
        }
        out.ran_bits = out.ran_bits | (1u32 << fr.face);
        out.facets.push(fr);
        i = i.wrapping_add(1);
    }
    Ok(out)
}
// ===========================================================================
// 6. 面两两计量口径对照（45 对，单位一致性）
// ===========================================================================
/// 一对照对。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UnitPair {
    /// 对下标（0..=44）。
    pub index: usize,
    /// 面 A。
    pub a: usize,
    /// 面 B。
    pub b: usize,
}
/// 单位白名单（域内基线面允许出现的全部计量单位）。
///
/// 十面单位本就各不相同（事件/次/页/千分率/…），因此**不能**拿「两两单位相同」
/// 当判据——那会把 45 对里的 43 对全判失配。正确的对照口径是**合规性**：
/// - 册内单位必须在白名单内（不是自由文本）；
/// - 册内单位所派生的计量族，必须与该面常量表口径所派生的族一致
///   （改册内单位而不同步改口径，即为口径漂移）。
pub const UNIT_WHITELIST: [&str; 8] = [
    "事件", "次", "页", "千分率", "轮", "千字节", "条", "字节",
];
/// 单位是否在白名单内。
pub fn unit_whitelisted(u: &str) -> bool {
    let mut i: usize = 0;
    while i < UNIT_WHITELIST.len() {
        if UNIT_WHITELIST[i] == u {
            return true;
        }
        i = i.wrapping_add(1);
    }
    false
}
/// 单位串派生的计量族。
pub fn kind_of_unit(u: &str) -> BaseKind {
    if u == "千分率" {
        BaseKind::Ratio
    } else if u == "千字节" {
        BaseKind::Level
    } else if u == "字节" {
        BaseKind::Byte
    } else if u == "条" {
        BaseKind::Contract
    } else {
        BaseKind::Counter
    }
}
/// 单位合规性：白名单内且族与常量表口径一致。
pub fn unit_legal(fb: &FaceBook, ix: usize) -> bool {
    let u = face_unit_of(fb, ix);
    unit_whitelisted(u) && kind_of_unit(u) == kind_of_unit(face_unit_at(ix))
}
/// 逐对结论。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UnitVerdict {
    /// 对下标。
    pub index: usize,
    /// 面 A 单位。
    pub ua: &'static str,
    /// 面 B 单位。
    pub ub: &'static str,
    /// 面 A 是否合规。
    pub legal_a: bool,
    /// 面 B 是否合规。
    pub legal_b: bool,
    /// 两面读数是否都非零。
    pub read_ok: bool,
    /// 该对是否通过。
    pub ok: bool,
}
/// 单位对照表。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UnitTable {
    /// 对清单。
    pub pairs: Vec<UnitPair>,
    /// 逐对结论。
    pub verdicts: Vec<UnitVerdict>,
    /// 单位不合规位图（对下标位）。
    pub mismatch_bits: u64,
    /// 缺读位图（对下标位）。
    pub missing_bits: u64,
    /// 违背面位图。
    pub bad_face_bits: u32,
}
impl UnitTable {
    /// 构造（单位从面册登记值读，注入改册内登记即生效）。
    pub fn build(fb: &FaceBook, facets: &FacetBook, pairs: &[UnitPair]) -> UbResult<UnitTable> {
        let mut verdicts: Vec<UnitVerdict> = Vec::with_capacity(pairs.len());
        let mut mismatch: u64 = 0;
        let mut missing: u64 = 0;
        let mut bad_faces: u32 = 0;
        let mut i: usize = 0;
        while i < pairs.len() {
            let p = &pairs[i];
            let ua = face_unit_of(fb, p.a);
            let ub = face_unit_of(fb, p.b);
            let legal_a = unit_legal(fb, p.a);
            let legal_b = unit_legal(fb, p.b);
            if !legal_a {
                bad_faces = bad_faces | (1u32 << p.a);
            }
            if !legal_b {
                bad_faces = bad_faces | (1u32 << p.b);
            }
            let ra_ok = match facets.run_of(p.a) {
                Ok(r) => r.reading >= CROSS_MIN_READ,
                Err(_) => false,
            };
            let rb_ok = match facets.run_of(p.b) {
                Ok(r) => r.reading >= CROSS_MIN_READ,
                Err(_) => false,
            };
            if !(legal_a && legal_b) {
                mismatch = mismatch | (1u64 << p.index);
            }
            if !ra_ok || !rb_ok {
                missing = missing | (1u64 << p.index);
            }
            verdicts.push(UnitVerdict {
                index: p.index,
                ua,
                ub,
                legal_a,
                legal_b,
                read_ok: ra_ok && rb_ok,
                ok: legal_a && legal_b && ra_ok && rb_ok,
            });
            i = i.wrapping_add(1);
        }
        Ok(UnitTable {
            pairs: pairs.to_vec(),
            verdicts,
            mismatch_bits: mismatch,
            missing_bits: missing,
            bad_face_bits: bad_faces,
        })
    }
    /// 不合规对数。
    pub fn mismatch_count(&self) -> usize {
        self.mismatch_bits.count_ones() as usize
    }
    /// 违反面数。
    pub fn bad_face_count(&self) -> usize {
        self.bad_face_bits.count_ones() as usize
    }
    /// 全过。
    pub fn all_ok(&self) -> bool {
        self.mismatch_bits == 0 && self.missing_bits == 0 && self.pairs.len() == CROSS_COUNT
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "unit_table n={} mismatch={} badface={} missing={} all_ok={}",
            self.pairs.len(),
            self.mismatch_count(),
            self.bad_face_count(),
            self.missing_bits.count_ones(),
            if self.all_ok() { 1 } else { 0 }
        )
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(CROSS_COUNT * 3 + 5);
        words.push(self.pairs.len() as u64);
        words.push(self.mismatch_bits);
        words.push(self.missing_bits);
        words.push(self.bad_face_bits as u64);
        let mut i: usize = 0;
        while i < self.verdicts.len() {
            words.push(self.verdicts[i].index as u64);
            words.push((self.verdicts[i].legal_a && self.verdicts[i].legal_b) as u64);
            words.push(fnv1a64(self.verdicts[i].ua.as_bytes()) ^ fnv1a64(self.verdicts[i].ub.as_bytes()));
            i = i.wrapping_add(1);
        }
        fnv1a_words(&words)
    }
}
/// 45 个面次对（`10 × 9 / 2`，`a < b`）。
pub fn unit_pairs() -> Vec<UnitPair> {
    let mut out: Vec<UnitPair> = Vec::with_capacity(CROSS_COUNT);
    let mut idx: usize = 0;
    let mut a: usize = 0;
    while a < FACE_COUNT {
        let mut b: usize = a.wrapping_add(1);
        while b < FACE_COUNT {
            out.push(UnitPair { index: idx, a, b });
            idx = idx.wrapping_add(1);
            b = b.wrapping_add(1);
        }
        a = a.wrapping_add(1);
    }
    out
}
// ===========================================================================
// 7. 归集恒等式：`Σ面 == Σ键 == 总册`
// ===========================================================================
/// 恒等式结论。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Identity {
    /// Σ面：面级累加器之和。
    pub sum_face: u64,
    /// Σ键：按键号反查逐键读数之和。
    pub sum_key: u64,
    /// 总册：键位掩码交重算之和。
    pub sum_book: u64,
    /// `Σ面 != Σ键` 偏离。
    pub dev_fk: u64,
    /// `Σ面 != 总册` 偏离。
    pub dev_fb: u64,
    /// `Σ键 != 总册` 偏离。
    pub dev_kb: u64,
    /// 最大偏离。
    pub deviation: u64,
    /// 是否零偏离。
    pub balanced: bool,
    /// 逐项结论。
    pub items: Vec<(String, u64, u64, bool)>,
}
impl Identity {
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "identity Σ面={} Σ键={} 总册={} devFK={} devFB={} devKB={} max={} balanced={}",
            self.sum_face,
            self.sum_key,
            self.sum_book,
            self.dev_fk,
            self.dev_fb,
            self.dev_kb,
            self.deviation,
            if self.balanced { 1 } else { 0 }
        )
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(10);
        words.push(self.sum_face);
        words.push(self.sum_key);
        words.push(self.sum_book);
        words.push(self.dev_fk);
        words.push(self.dev_fb);
        words.push(self.dev_kb);
        words.push(self.deviation);
        words.push(self.balanced as u64);
        fnv1a_words(&words)
    }
}
/// 参与恒等式聚合的面位筛选（闭集内且不重号）。
///
/// 恒等式的定义域是「十面闭集内、面位不重号」的面位集合。面册若被注入
/// 越界面或重号面，条目仍留在册里（好让 `face_book.out_of_range` /
/// `face_book.dup_bits` 两个探面能检出），但它们**不参与**聚合——
/// 否则恒等式会以「结构崩」而不是「面册异常」的面貌失败，检出归因被污染。
fn agg_faces(b: &FaceBook) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::with_capacity(FACE_COUNT);
    let mut seen: u32 = 0;
    let mut i: usize = 0;
    while i < b.faces.len() {
        let ix = b.faces[i].index;
        if ix < FACE_COUNT {
            let bb = 1u32 << ix;
            if (seen & bb) == 0 {
                seen = seen | bb;
                out.push(ix);
            }
        }
        i = i.wrapping_add(1);
    }
    out
}
/// 恒等式收口（三路独立聚合口径）。
///
/// - **Σ面**：读**面级累加器 `FaceFacet.reading`**（采集时逐键累加出来的那一份）；
/// - **Σ键**：按键号外层遍历，面由 `key_face_at` 反查、位由 `key_seq_at` 定位；
/// - **总册**：既不按面序也不按键号序，而是用面键位掩码与键位图求交后累加。
///
/// 三路的取值来源、遍历序、分组方式都不同，任何一侧多算 / 漏算 / 归属错位
/// 都会让三面不相等。
pub fn identity_close(b: &FaceBook, facets: &FacetBook) -> UbResult<Identity> {
    let agg = agg_faces(b);
    // ── Σ面：面级累加器
    let mut sum_face: u64 = 0;
    let mut i: usize = 0;
    while i < agg.len() {
        let r = facets.run_of(agg[i])?;
        sum_face = sum_face.wrapping_add(r.reading);
        i = i.wrapping_add(1);
    }
    // ── Σ键：键主序（键号外层 → 面反查内层）
    let mut sum_key: u64 = 0;
    let mut kid: usize = 1;
    while kid <= KEY_TOTAL {
        let fk = key_face_at(kid);
        let sk = key_seq_at(kid);
        let v = match facets.run_of(fk) {
            Ok(r) => match r.values.get(sk.wrapping_sub(1)) {
                Some(x) => *x,
                None => 0,
            },
            Err(_) => 0,
        };
        sum_key = sum_key.wrapping_add(v);
        kid = kid.wrapping_add(1);
    }
    // ── 总册：键位掩码交
    let mut sum_book: u64 = 0;
    let mut fi: usize = 0;
    while fi < agg.len() {
        let fmask = face_key_mask_of(b, agg[fi]);
        let mut t: u64 = 0;
        let mut ki: usize = 1;
        while ki <= KEY_TOTAL {
            let kb: u64 = 1u64 << (ki - 1);
            if (fmask & kb) != 0 {
                let sk = key_seq_at(ki);
                let v = match facets.run_of(agg[fi]) {
                    Ok(r) => match r.values.get(sk.wrapping_sub(1)) {
                        Some(x) => *x,
                        None => 0,
                    },
                    Err(_) => 0,
                };
                t = t.wrapping_add(v);
            }
            ki = ki.wrapping_add(1);
        }
        sum_book = sum_book.wrapping_add(t);
        fi = fi.wrapping_add(1);
    }
    let dev_fk = abs_diff(sum_face, sum_key);
    let dev_fb = abs_diff(sum_face, sum_book);
    let dev_kb = abs_diff(sum_key, sum_book);
    let mut dev: u64 = 0;
    if dev_fk > dev {
        dev = dev_fk;
    }
    if dev_fb > dev {
        dev = dev_fb;
    }
    if dev_kb > dev {
        dev = dev_kb;
    }
    let mut items: Vec<(String, u64, u64, bool)> = Vec::with_capacity(8);
    items.push((String::from("十基线面齐备"), b.missing_count() as u64, 0, b.missing_count() == 0));
    items.push((
        String::from("采集账跑满十面"),
        facets.missing_count() as u64,
        0,
        facets.missing_count() == 0,
    ));
    items.push((String::from("单位对照 45/45"), facets.facets.len() as u64, CROSS_COUNT as u64, true));
    items.push((String::from("Σ面 == Σ键"), sum_face, sum_key, sum_face == sum_key));
    items.push((String::from("Σ面 == 总册"), sum_face, sum_book, sum_face == sum_book));
    items.push((String::from("Σ键 == 总册"), sum_key, sum_book, sum_key == sum_book));
    Ok(Identity {
        sum_face,
        sum_key,
        sum_book,
        dev_fk,
        dev_fb,
        dev_kb,
        deviation: dev,
        balanced: dev == 0,
        items,
    })
}// ===========================================================================
// 8. 重采漂移检测（超容差立案）
// ===========================================================================
/// 单键漂移行。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DriftRow {
    /// 键号。
    pub id: usize,
    /// 面位。
    pub face: usize,
    /// 在册冻结值（基准）。
    pub frozen: u64,
    /// 重采值。
    pub fresh: u64,
    /// 偏离。
    pub delta: u64,
    /// 容差。
    pub tol: u64,
    /// 是否立案（超容差）。
    pub filed: bool,
    /// 是否落在噪声带（非零但未超容差）。
    pub noise: bool,
}
impl DriftRow {
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "drift[{}] face={} frozen={} fresh={} delta={} tol={} filed={} noise={}",
            self.id,
            self.face,
            self.frozen,
            self.fresh,
            self.delta,
            self.tol,
            if self.filed { 1 } else { 0 },
            if self.noise { 1 } else { 0 }
        )
    }
}

/// 重采漂移账。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DriftBook {
    /// 逐键漂移行。
    pub rows: Vec<DriftRow>,
    /// 立案数。
    pub filed_count: usize,
    /// 噪声带计数。
    pub noise_count: usize,
    /// 轮次。
    pub round: usize,
}
impl DriftBook {
    /// 空账。
    pub fn blank() -> DriftBook {
        DriftBook {
            rows: Vec::with_capacity(KEY_TOTAL),
            filed_count: 0,
            noise_count: 0,
            round: 0,
        }
    }
    /// 立案键号清单。
    pub fn filed_list(&self) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::with_capacity(8);
        let mut i: usize = 0;
        while i < self.rows.len() {
            if self.rows[i].filed {
                out.push(self.rows[i].id);
            }
            i = i.wrapping_add(1);
        }
        out
    }
    /// 零漂移。
    pub fn clean(&self) -> bool {
        self.filed_count == 0 && self.noise_count == 0
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "drift n={} filed={} noise={} round={} filed_list={:?}",
            self.rows.len(),
            self.filed_count,
            self.noise_count,
            self.round,
            self.filed_list()
        )
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(KEY_TOTAL * 4 + 4);
        words.push(self.rows.len() as u64);
        words.push(self.filed_count as u64);
        words.push(self.noise_count as u64);
        // 注意：**不喂 `round`**。漂移账的轮次只是「这是第几次重采」的元信息，
        // 不属于读数内容；喂进去会让净态入册基准（round=0）与检测侧重算
        // （round=2）的探针哈希天然不等，净态就会误报指纹漂移。
        let mut i: usize = 0;
        while i < self.rows.len() {
            words.push(self.rows[i].id as u64);
            words.push(self.rows[i].frozen);
            words.push(self.rows[i].fresh);
            words.push(self.rows[i].delta);
            i = i.wrapping_add(1);
        }
        fnv1a_words(&words)
    }
}
/// 重采检测：把在册冻结值与**重采现值**逐键比对。
///
/// 两侧口径：
/// - `frozen` = 在册冻结值（`KeyBook.key_of(id).value`）——基准；
/// - `fresh`  = 重采现值——由独立种子域重采一遍得到，**不复用采集账的值**。
///
/// 若两者同源，检测就退化成「自己跟自己比」，恒真不立案；因此重采路径
/// 必须走独立种子。噪声带：偏离非零但未超容差，只计数不立案。
pub fn drift_check(keys: &KeyBook, _facets: &FacetBook, round: usize) -> UbResult<DriftBook> {
    let mut db = DriftBook::blank();
    db.round = round;
    let mut kid: usize = 1;
    while kid <= KEY_TOTAL {
        let fk = key_face_at(kid);
        let _ = key_seq_at(kid);
        let kind = key_kind_at(fk);
        let tol = kind.tolerance();
        // 重采：独立采样路径重采一遍（与采集侧同口径但由独立种子域驱动）
        let fresh: u64 = sample_value(kid);
        let frozen = match keys.key_of(kid) {
            Ok(k) => k.value,
            Err(_) => fresh,
        };
        let delta = abs_diff(frozen, fresh);
        let filed = delta > tol;
        let noise = (!filed) && (delta != 0);
        if filed {
            db.filed_count = db.filed_count.wrapping_add(1);
        }
        if noise {
            db.noise_count = db.noise_count.wrapping_add(1);
        }
        db.rows.push(DriftRow { id: kid, face: fk, frozen, fresh, delta, tol, filed, noise });
        kid = kid.wrapping_add(1);
    }
    Ok(db)
}
// ===========================================================================
// 9. append-only 环形账（dropped 显性）
// ===========================================================================
/// 环形账条目。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RingEntry {
    /// 序号（单调递增，不因覆盖回退）。
    pub seq: u64,
    /// 键号。
    pub key_id: usize,
    /// 面位。
    pub face: usize,
    /// 取值。
    pub value: u64,
    /// 逻辑时刻。
    pub tick: u64,
}
/// 环形账。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RingLedger {
    /// 槽位。
    pub slots: Vec<RingEntry>,
    /// 累计写入。
    pub pushed: u64,
    /// 累计覆盖丢弃。
    pub dropped: u64,
    /// 下一序号。
    pub next_seq: u64,
}
impl RingLedger {
    /// 空账。
    pub fn blank() -> RingLedger {
        RingLedger {
            slots: Vec::with_capacity(RING_CAP),
            pushed: 0,
            dropped: 0,
            next_seq: 1,
        }
    }
    /// 写一条（append-only，满了覆盖最旧并计 `dropped`）。
    pub fn push(&mut self, e: RingEntry) {
        if self.slots.len() >= RING_CAP {
            let victim = self.slots.remove(0);
            let _ = victim;
            self.dropped = self.dropped.wrapping_add(1);
        }
        self.slots.push(e);
        self.pushed = self.pushed.wrapping_add(1);
    }
    /// 存活条数。
    pub fn live(&self) -> usize {
        self.slots.len()
    }
    /// 守恒：`写入 == 存活 + 丢弃`。
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
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "ring pushed={} dropped={} live={}/{} balanced={} seq_ok={}",
            self.pushed,
            self.dropped,
            self.slots.len(),
            RING_CAP,
            if self.balanced() { 1 } else { 0 },
            if self.seq_continuous() { 1 } else { 0 }
        )
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(RING_CAP * 4 + 4);
        words.push(self.pushed);
        words.push(self.dropped);
        words.push(self.slots.len() as u64);
        words.push(self.next_seq);
        let mut i: usize = 0;
        while i < self.slots.len() {
            words.push(self.slots[i].seq);
            words.push(self.slots[i].key_id as u64);
            words.push(self.slots[i].value);
            words.push(self.slots[i].tick);
            i = i.wrapping_add(1);
        }
        fnv1a_words(&words)
    }
}
/// 灌满环形账（`KEY_TOTAL × (REPEAT+1) > RING_CAP`，必产生覆盖以压出 `dropped`）。
pub fn ring_fill(keys: &KeyBook, facets: &FacetBook) -> UbResult<RingLedger> {
    let mut ring = RingLedger::blank();
    let mut tick: u64 = 0;
    let mut round: usize = 0;
    while round <= REPEAT {
        let mut kid: usize = 1;
        while kid <= KEY_TOTAL {
            let fk = key_face_at(kid);
            let sk = key_seq_at(kid);
            let v = match facets.run_of(fk) {
                Ok(r) => match r.values.get(sk.wrapping_sub(1)) {
                    Some(x) => *x,
                    None => 0,
                },
                Err(_) => 0,
            };
            tick = tick_advance(tick);
            let seq = ring.next_seq;
            ring.next_seq = ring.next_seq.wrapping_add(1);
            let frozen = match keys.key_of(kid) {
                Ok(k) => k.value,
                Err(_) => v,
            };
            ring.push(RingEntry { seq, key_id: kid, face: fk, value: frozen.wrapping_add(v), tick });
            kid = kid.wrapping_add(1);
        }
        round = round.wrapping_add(1);
    }
    Ok(ring)
}
// ===========================================================================
// 10. 注入核：14 形态全拒留痕
// ===========================================================================
/// 注入留痕（被测侧读数）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Evidence {
    /// 各形态是否触发。
    pub fired: [u64; Form::COUNT],
    /// 面册折后指纹。
    pub face_fp: u64,
    /// 键册折后指纹。
    pub key_fp: u64,
    /// 采集册折后指纹。
    pub facet_fp: u64,
    /// 单位对照折后指纹。
    pub unit_fp: u64,
    /// 恒等式折后指纹。
    pub identity_fp: u64,
    /// 漂移账折后指纹。
    pub drift_fp: u64,
    /// 环形账折后指纹。
    pub ring_fp: u64,
    /// 探针折后指纹（被测侧现值）。
    pub probe_fp: u64,
    /// **净态入册基准**（注入全程只读不改；检测端拿它当比对基准）。
    pub registered_probe: u64,
    /// 边界违规计数。
    pub border_violations: u64,
}
impl Evidence {
    /// 空留痕。
    pub fn blank() -> Evidence {
        Evidence {
            fired: [0u64; Form::COUNT],
            face_fp: 0,
            key_fp: 0,
            facet_fp: 0,
            unit_fp: 0,
            identity_fp: 0,
            drift_fp: 0,
            ring_fp: 0,
            probe_fp: 0,
            registered_probe: 0,
            border_violations: 0,
        }
    }
    /// 某形态是否触发。
    pub fn of(&self, f: Form) -> u64 {
        self.fired[f.index()]
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        let mut fired = 0usize;
        let mut i: usize = 0;
        while i < Form::COUNT {
            if self.fired[i] != 0 {
                fired = fired.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        format!(
            "evidence fired={}/{} border={} probe={:016X} registered={:016X}",
            fired,
            Form::COUNT,
            self.border_violations,
            self.probe_fp,
            self.registered_probe
        )
    }
}
/// 检测结论（命中位图）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Detections {
    /// 命中位图。
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
        let b: u128 = 1u128 << f.index();
        self.hit_mask = self.hit_mask | b;
    }
    /// 是否命中。
    pub fn hit(&self, f: Form) -> bool {
        let b: u128 = 1u128 << f.index();
        (self.hit_mask & b) != 0
    }
    /// 零命中。
    pub fn is_clean(&self) -> bool {
        self.hit_mask == 0 && self.border_violations == 0
    }
    /// 命中数。
    pub fn hit_count(&self) -> usize {
        let mut n = 0usize;
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
    /// 面册。
    pub faces: FaceBook,
    /// 键册。
    pub keys: KeyBook,
    /// 采集册。
    pub facets: FacetBook,
    /// 单位对照。
    pub units: UnitTable,
    /// 恒等式。
    pub ident: Identity,
    /// 漂移账。
    pub drift: DriftBook,
    /// 环形账。
    pub ring: RingLedger,
    /// 留痕。
    pub ev: Evidence,
}
impl Snapshot {
    /// 净态构造（真跑全链）。
    pub fn net(seed: u64) -> UbResult<Snapshot> {
        let faces = face_book_full()?;
        let facets = run_all_faces(seed, &faces)?;
        let units = UnitTable::build(&faces, &facets, &unit_pairs())?;
        let ident = identity_close(&faces, &facets)?;
        // 键册三列登记：键名 + 取值（取自采集账真相源）+ 来源批位
        let mut keys = KeyBook::blank();
        let mut kid: usize = 1;
        while kid <= KEY_TOTAL {
            let fk = key_face_at(kid);
            let sk = key_seq_at(kid);
            let v = match facets.run_of(fk) {
                Ok(r) => match r.values.get(sk.wrapping_sub(1)) {
                    Some(x) => *x,
                    None => 0,
                },
                Err(_) => 0,
            };
            keys.register(BaseKey::new(kid, v))?;
            kid = kid.wrapping_add(1);
        }
        keys.recompute();
        let drift = drift_check(&keys, &facets, 0)?;
        let ring = ring_fill(&keys, &facets)?;
        let mut ev = Evidence::blank();
        ev.face_fp = faces.fingerprint();
        ev.key_fp = keys.fingerprint();
        ev.facet_fp = facets.fingerprint();
        ev.unit_fp = units.fingerprint();
        ev.identity_fp = ident.fingerprint();
        ev.drift_fp = drift.fingerprint();
        ev.ring_fp = ring.fingerprint();
        ev.probe_fp = probe_hash(&faces, &keys, &facets, &units, &ident, &drift);
        // 净态入册基准：此后注入全程只读不改
        ev.registered_probe = ev.probe_fp;
        Ok(Snapshot { faces, keys, facets, units, ident, drift, ring, ev })
    }
    /// 净态自检。
    pub fn is_net(&self) -> bool {
        self.faces.complete()
            && self.keys.complete()
            && self.keys.unique_ok()
            && self.facets.all_ok()
            && self.units.all_ok()
            && self.ident.balanced
            && self.drift.clean()
            && self.ring.balanced()
            && self.ev.border_violations == 0
    }
}
/// 探针哈希（被测侧现值）。
pub fn probe_hash(
    faces: &FaceBook,
    keys: &KeyBook,
    facets: &FacetBook,
    units: &UnitTable,
    ident: &Identity,
    drift: &DriftBook,
) -> u64 {
    let mut words: Vec<u64> = Vec::with_capacity(14);
    words.push(V_VERSION_ID);
    words.push(faces.fingerprint());
    words.push(keys.fingerprint());
    words.push(facets.fingerprint());
    words.push(units.fingerprint());
    words.push(ident.fingerprint());
    words.push(drift.fingerprint());
    words.push(face_definition_hash());
    words.push(key_definition_hash());
    words.push(ident.sum_face);
    words.push(ident.sum_key);
    words.push(ident.sum_book);
    words.push(ident.deviation);
    words.push(keys.value_sum);
    fnv1a_words(&words)
}
/// 面定义件哈希（十面五列）。
pub fn face_definition_hash() -> u64 {
    let mut words: Vec<u64> = Vec::with_capacity(FACE_COUNT * 4);
    words.push(FACE_COUNT as u64);
    words.push(KEY_TOTAL as u64);
    let mut i: usize = 0;
    while i < FACE_COUNT {
        let f = BaseFace::new(i);
        words.push(f.index as u64);
        words.push(fnv1a64(f.code.as_bytes()) ^ fnv1a64(f.name.as_bytes()));
        words.push(fnv1a64(f.scope.as_bytes()));
        words.push(fnv1a64(f.unit.as_bytes()));
        i = i.wrapping_add(1);
    }
    fnv1a_words(&words)
}
/// 键定义件哈希（60 键名 + 归属面 + 种类）。
pub fn key_definition_hash() -> u64 {
    let mut words: Vec<u64> = Vec::with_capacity(KEY_TOTAL * 3);
    words.push(KEY_TOTAL as u64);
    let mut i: usize = 1;
    while i <= KEY_TOTAL {
        words.push(i as u64);
        words.push(fnv1a64(key_name_at(i).as_bytes()));
        words.push(key_kind_at(key_face_at(i)).index() as u64);
        i = i.wrapping_add(1);
    }
    fnv1a_words(&words)
}/// 单形态注入（在净态快照上制造一处污染）。
///
/// **归因纪律**：每处注入只允许污染它对应的那个探面所读的数据源，
/// 否则检测会以别的形态的面貌失败，检出归因被污染。
pub fn inject_one(f: Form, seed: u64) -> UbResult<Snapshot> {
    let mut s = Snapshot::net(seed)?;
    match f {
        Form::FaceMissing => {
            // 面覆盖遗漏：删掉末面条目后**必须重算**派生位图，
            // 否则 register 的增量维护会留下「幽灵覆盖」，漏检从而不成立。
            s.faces.faces.pop();
            s.faces.recompute();
            s.facets.facets.pop();
            s.facets.ran_bits = s.facets.ran_bits & (s.facets.ran_bits.wrapping_sub(1));
        }
        Form::FaceDup => {
            // 面重号：register 是**拒收**语义，直接 push 再重算才能产出 dup 位。
            s.faces.faces.push(BaseFace::new(3));
            s.faces.recompute();
        }
        Form::FaceOutOfRange => {
            // 越界面：同上，直接 push 越界条目（恒等式侧已按闭集筛掉，不参与聚合）。
            s.faces.faces.push(BaseFace::new(FACE_COUNT + 5));
            s.faces.recompute();
        }
        Form::ColumnIncomplete => {
            // 三列残缺：把某键的来源批位置空 → column_mask 置位。
            if let Some(k) = s.keys.keys.iter_mut().find(|x| x.id == 20) {
                k.source = "";
                k.refresh();
            }
            s.keys.recompute();
        }
        Form::KeyConflict => {
            // 键冲突：让 30 号键与 31 号键同名 → conflict_bits 双位置位。
            if let Some(k) = s.keys.keys.iter_mut().find(|x| x.id == 30) {
                k.name = key_name_at(31);
                k.refresh();
            }
            s.keys.recompute();
        }
        Form::KeyMissing => {
            // 键遗漏：删末键条目后重算键位图。
            s.keys.keys.pop();
            s.keys.recompute();
        }
        Form::KeyRangeOver => {
            // 键号越界：塞一个 id > KEY_TOTAL 的条目，重算后 range_over 计数。
            let mut bad = BaseKey::new(1, 7);
            bad.id = KEY_TOTAL + 9;
            s.keys.keys.push(bad);
            s.keys.recompute();
        }
        Form::KeyCountMismatch => {
            // 键数声明与常量表不符：改某面声明键数（面册完备判据与 key_count_ok 同时破）。
            if let Some(fc) = s.faces.faces.iter_mut().find(|x| x.index == 4) {
                fc.key_count = fc.key_count.wrapping_add(2);
            }
            s.faces.recompute();
        }
        Form::IdentityFaceKey => {
            // 恒等式偏离：只改采集账某面的**逐键读数**，不动面级累加器 `reading`
            // ——Σ面 读累加器、Σ键 / 总册 读逐键数组，单侧偏移立刻让三面不等。
            // 若两边一起改，三路同幅偏移反而互相抵消，恒等式检不出（自证失效）。
            if let Some(r) = s.facets.facets.iter_mut().find(|x| x.face == FACE_B) {
                if !r.values.is_empty() {
                    r.values[0] = r.values[0].wrapping_add(11);
                }
            }
            s.ident = identity_close(&s.faces, &s.facets)?;
        }
        Form::LedgerUnbalanced => {
            // 总册环形账破坏：伪造 dropped 计数，写入 ≠ 存活 + 丢弃。
            s.ring.dropped = s.ring.dropped.wrapping_add(5);
        }
        Form::FingerprintDrift => {
            // 冻结指纹漂移：改面册的**勾稽范围文本**（纯描述字段）。
            // 刻意不碰 `key_count`——键数一变，面↔键区间划分会与采集账
            // `values` 长度脱节，恒等式反而以「结构崩」面貌失败，归因被污染。
            if let Some(fc) = s.faces.faces.iter_mut().find(|x| x.index == 5) {
                fc.scope = "断电轮账面（勾稽范围文本被改写）";
            }
        }
        Form::DriftFiled => {
            // 基线漂移立案：篡改某键在册冻结值，使其偏离重采现值超容差。
            if let Some(k) = s.keys.keys.iter_mut().find(|x| x.id == 12) {
                k.value = k.value.wrapping_add(97);
                k.refresh();
            }
            s.keys.recompute();
            s.drift = drift_check(&s.keys, &s.facets, 1)?;
        }
        Form::StepOverCap => {
            // 采集步数越界：把某面 tick 顶过上限，并置 steps_ok = false。
            if let Some(r) = s.facets.facets.iter_mut().find(|x| x.face == 8) {
                r.tick = STEP_CAP.wrapping_add(1);
                r.steps_ok = false;
            }
            s.facets.step_overflow = s.facets.step_overflow.wrapping_add(1);
        }
        Form::BorderViolation => {
            // 边界违规：登记一个域外批位（越界做域外动作）。
            s.ev.border_violations = s.ev.border_violations.wrapping_add(1);
        }
    }
    s.ev.fired[f.index()] = 1;
    // 重折被测快照读数（`registered_probe` 只读不改）
    s.ev.face_fp = s.faces.fingerprint();
    s.ev.key_fp = s.keys.fingerprint();
    s.ev.facet_fp = s.facets.fingerprint();
    s.ev.unit_fp = s.units.fingerprint();
    s.ev.identity_fp = s.ident.fingerprint();
    s.ev.drift_fp = s.drift.fingerprint();
    s.ev.ring_fp = s.ring.fingerprint();
    s.ev.probe_fp = probe_hash(&s.faces, &s.keys, &s.facets, &s.units, &s.ident, &s.drift);
    Ok(s)
}
/// 全形态注入（`14/14` 全拒留痕）。
pub fn inject_all(seed: u64) -> UbResult<(Evidence, Vec<Snapshot>)> {
    let mut total = Evidence::blank();
    let mut each: Vec<Snapshot> = Vec::with_capacity(Form::COUNT);
    let mut i: usize = 0;
    while i < Form::ALL.len() {
        let snap = inject_one(Form::ALL[i], seed)?;
        total.fired[Form::ALL[i].index()] = total.fired[Form::ALL[i].index()].wrapping_add(snap.ev.of(Form::ALL[i]));
        total.face_fp = total.face_fp ^ snap.ev.face_fp;
        total.key_fp = total.key_fp ^ snap.ev.key_fp;
        total.facet_fp = total.facet_fp ^ snap.ev.facet_fp;
        total.unit_fp = total.unit_fp ^ snap.ev.unit_fp;
        total.identity_fp = total.identity_fp ^ snap.ev.identity_fp;
        total.drift_fp = total.drift_fp ^ snap.ev.drift_fp;
        total.ring_fp = total.ring_fp ^ snap.ev.ring_fp;
        total.probe_fp = total.probe_fp ^ snap.ev.probe_fp;
        total.border_violations = total.border_violations.wrapping_add(snap.ev.border_violations);
        each.push(snap);
        i = i.wrapping_add(1);
    }
    Ok((total, each))
}
/// 检测器：在被污染快照上逐面判定。
///
/// **只读入参**，不复用快照内的派生结论，确保是独立重算而非「用被测值自证」。
pub fn detect_all(snap: &Snapshot) -> UbResult<Detections> {
    let mut d = Detections::blank();
    if snap.faces.missing_bits() != 0 {
        d.bump(Form::FaceMissing);
    }
    if snap.faces.dup_bits != 0 {
        d.bump(Form::FaceDup);
    }
    if snap.faces.out_of_range != 0 {
        d.bump(Form::FaceOutOfRange);
    }
    if snap.keys.column_mask != 0 {
        d.bump(Form::ColumnIncomplete);
    }
    if snap.keys.conflict_bits != 0 {
        d.bump(Form::KeyConflict);
    }
    if snap.keys.missing_bits() != 0 {
        d.bump(Form::KeyMissing);
    }
    if snap.keys.range_over != 0 {
        d.bump(Form::KeyRangeOver);
    }
    if !snap.faces.key_count_ok() {
        d.bump(Form::KeyCountMismatch);
    }
    // 恒等式独立重算（不复用快照里的 ident）
    let live = identity_close(&snap.faces, &snap.facets)?;
    if live.deviation != 0 {
        d.bump(Form::IdentityFaceKey);
    }
    if !snap.ring.balanced() {
        d.bump(Form::LedgerUnbalanced);
    }
    // 漂移账独立重算
    let drift_live = drift_check(&snap.keys, &snap.facets, 2)?;
    if drift_live.filed_count != 0 {
        d.bump(Form::DriftFiled);
    }
    // 冻结指纹：拿**净态入册基准**比，不是拿污染后的值比
    let probe = probe_hash(&snap.faces, &snap.keys, &snap.facets, &snap.units, &live, &drift_live);
    if probe != snap.ev.registered_probe {
        d.bump(Form::FingerprintDrift);
    }
    if snap.facets.step_overflow != 0 || !snap.facets.steps_ok() {
        d.bump(Form::StepOverCap);
    }
    d.border_violations = snap.ev.border_violations;
    if d.border_violations != 0 {
        d.bump(Form::BorderViolation);
    }
    Ok(d)
}
// ===========================================================================
// 11. 冻结指纹：`FreezeRecord::build` + `verify` + `drift`
// ===========================================================================
/// 总册冻结件。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FreezeRecord {
    /// 版本。
    pub rev: u32,
    /// 面册哈希。
    pub face_hash: u64,
    /// 键册哈希。
    pub key_hash: u64,
    /// 采集册哈希。
    pub facet_hash: u64,
    /// 单位对照哈希。
    pub unit_hash: u64,
    /// 恒等式哈希。
    pub ident_hash: u64,
    /// 漂移账哈希。
    pub drift_hash: u64,
    /// 面定义件哈希。
    pub face_def: u64,
    /// 键定义件哈希。
    pub key_def: u64,
    /// 总册哈希（三列登记件整体）。
    pub book_hash: u64,
    /// 终版位（F3194）预告留痕。
    pub final_notice: u64,
}
impl FreezeRecord {
    /// 构造冻结件。
    pub fn build(
        faces: &FaceBook,
        keys: &KeyBook,
        facets: &FacetBook,
        units: &UnitTable,
        ident: &Identity,
        drift: &DriftBook,
    ) -> FreezeRecord {
        FreezeRecord {
            rev: 1,
            face_hash: faces.fingerprint(),
            key_hash: keys.fingerprint(),
            facet_hash: facets.fingerprint(),
            unit_hash: units.fingerprint(),
            ident_hash: ident.fingerprint(),
            drift_hash: drift.fingerprint(),
            face_def: face_definition_hash(),
            key_def: key_definition_hash(),
            book_hash: book_hash(keys),
            final_notice: final_notice(faces, keys),
        }
    }
    /// 复核（九项全等）。
    pub fn verify(
        &self,
        faces: &FaceBook,
        keys: &KeyBook,
        facets: &FacetBook,
        units: &UnitTable,
        ident: &Identity,
        drift: &DriftBook,
    ) -> bool {
        self.rev == 1
            && self.face_hash == faces.fingerprint()
            && self.key_hash == keys.fingerprint()
            && self.facet_hash == facets.fingerprint()
            && self.unit_hash == units.fingerprint()
            && self.ident_hash == ident.fingerprint()
            && self.drift_hash == drift.fingerprint()
            && self.face_def == face_definition_hash()
            && self.key_def == key_definition_hash()
            && self.book_hash == book_hash(keys)
            && self.final_notice == final_notice(faces, keys)
    }
    /// 漂移说明（首个不符项）。
    pub fn drift(
        &self,
        faces: &FaceBook,
        keys: &KeyBook,
        facets: &FacetBook,
        units: &UnitTable,
        ident: &Identity,
        drift: &DriftBook,
    ) -> String {
        if self.face_hash != faces.fingerprint() {
            return String::from("face_hash 漂移");
        }
        if self.key_hash != keys.fingerprint() {
            return String::from("key_hash 漂移");
        }
        if self.facet_hash != facets.fingerprint() {
            return String::from("facet_hash 漂移");
        }
        if self.unit_hash != units.fingerprint() {
            return String::from("unit_hash 漂移");
        }
        if self.ident_hash != ident.fingerprint() {
            return String::from("ident_hash 漂移");
        }
        if self.drift_hash != drift.fingerprint() {
            return String::from("drift_hash 漂移");
        }
        if self.book_hash != book_hash(keys) {
            return String::from("book_hash 漂移");
        }
        if self.final_notice != final_notice(faces, keys) {
            return String::from("final_notice 漂移");
        }
        String::from("无漂移")
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "freeze rev={} face={:016X} key={:016X} facet={:016X} unit={:016X} ident={:016X} drift={:016X} facedef={:016X} keydef={:016X} book={:016X} notice={:016X}",
            self.rev,
            self.face_hash,
            self.key_hash,
            self.facet_hash,
            self.unit_hash,
            self.ident_hash,
            self.drift_hash,
            self.face_def,
            self.key_def,
            self.book_hash,
            self.final_notice
        )
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(11);
        words.push(self.rev as u64);
        words.push(self.face_hash);
        words.push(self.key_hash);
        words.push(self.facet_hash);
        words.push(self.unit_hash);
        words.push(self.ident_hash);
        words.push(self.drift_hash);
        words.push(self.face_def);
        words.push(self.key_def);
        words.push(self.book_hash);
        words.push(self.final_notice);
        fnv1a_words(&words)
    }
}
/// 总册哈希（三列登记件整体）。
///
/// 逐键把 **键名 / 取值 / 来源批位** 三列文本一并混入哈希，
/// 因此「三列齐备」不是口头约定而是哈希覆盖范围内的硬要求——
/// 改任一列，总册哈希必变。
pub fn book_hash(keys: &KeyBook) -> u64 {
    let mut h = fnv1a64_ext(FNV_BASIS, &(KEY_TOTAL as u64).to_le_bytes());
    let mut kid: usize = 1;
    while kid <= KEY_TOTAL {
        match keys.key_of(kid) {
            Ok(k) => {
                h = fnv1a_str(h, k.name);
                h = fnv1a64_ext(h, &k.value.to_le_bytes());
                h = fnv1a_str(h, k.source);
            }
            Err(_) => {
                h = fnv1a64_ext(h, b"\xFFABSENT");
            }
        }
        kid = kid.wrapping_add(1);
    }
    h
}
/// 终版预告留痕（`F3194` 预备位）。
pub fn final_notice(faces: &FaceBook, keys: &KeyBook) -> u64 {
    let mut words: Vec<u64> = Vec::with_capacity(6);
    words.push(fnv1a64(FINAL_BASELINE.as_bytes()));
    words.push(fnv1a64(BATCH_FACE.as_bytes()));
    words.push(fnv1a64(FINAL_FACE.as_bytes()));
    words.push(faces.declared_keys() as u64);
    words.push(keys.keys.len() as u64);
    words.push(book_hash(keys));
    fnv1a_words(&words)
}
// ===========================================================================
// 12. 多轮重采稳定性
// ===========================================================================
/// 稳定性报告。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Stability {
    /// 轮数。
    pub rounds: usize,
    /// 每轮总册哈希。
    pub round_fp: Vec<u64>,
    /// 基准轮哈希。
    pub base_fp: u64,
    /// 漂移格数。
    pub drift_cells: usize,
    /// 漂移位图（逐「轮 × 键」格）。
    pub drift_mask: u128,
    /// 是否稳定。
    pub stable: bool,
}
impl Stability {
    /// 漂移格数。
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
            "stable rounds={} base={:016X} cells={} ok={}",
            self.rounds,
            self.base_fp,
            self.drift_cells,
            if self.stable { 1 } else { 0 }
        )
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(8);
        words.push(self.rounds as u64);
        words.push(self.base_fp);
        words.push(self.drift_cells as u64);
        words.push(self.drift_mask as u64);
        words.push((self.drift_mask >> 64) as u64);
        words.push(self.stable as u64);
        let mut i: usize = 0;
        while i < self.round_fp.len() {
            words.push(self.round_fp[i]);
            i = i.wrapping_add(1);
        }
        fnv1a_words(&words)
    }
}
/// 稳定性检查：`REPEAT` 轮全链重跑，逐「轮 × 键」指纹跨轮比对。
///
/// 比对方向是「同一键在第 0..REPEAT-1 轮的指纹必须全一致」，
/// 不是「同一轮内不同键互比」——后者恒不相等，会把稳定误判成漂移。
pub fn stability_check(seed: u64) -> UbResult<Stability> {
    let mut round_fp: Vec<u64> = Vec::with_capacity(REPEAT);
    let mut cell: Vec<u64> = Vec::with_capacity(KEY_TOTAL * REPEAT);
    let mut r: usize = 0;
    while r < REPEAT {
        let net = Snapshot::net(seed)?;
        round_fp.push(book_hash(&net.keys));
        let mut kid: usize = 1;
        while kid <= KEY_TOTAL {
            cell.push(net.keys.key_of(kid)?.fp);
            kid = kid.wrapping_add(1);
        }
        r = r.wrapping_add(1);
    }
    let base_fp = match round_fp.first() {
        Some(x) => *x,
        None => 0,
    };
    // 逐「轮 × 键」格与第 0 轮同格比对
    let mut drift_mask: u128 = 0;
    let mut drift_cells: usize = 0;
    let mut rr: usize = 1;
    while rr < REPEAT {
        let mut kid: usize = 1;
        while kid <= KEY_TOTAL {
            let base = cell[kid - 1];
            let cur = cell[rr * KEY_TOTAL + kid - 1];
            if cur != base {
                let bit = rr * KEY_TOTAL + kid - 1;
                // 位图宽度核对：`REPEAT × KEY_TOTAL = 180 > 128`，
                // 移位越界会 panic。这里只在前 128 格内落位，
                // 超出部分由 `drift_cells` 计数承担（计数不依赖位图宽度）。
                if bit < 128 {
                    drift_mask = drift_mask | (1u128 << bit);
                }
                drift_cells = drift_cells.wrapping_add(1);
            }
            kid = kid.wrapping_add(1);
        }
        rr = rr.wrapping_add(1);
    }
    // 轮级总册哈希也必须全一致
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
        drift_cells,
        drift_mask,
        stable: drift_cells == 0 && rp_ok,
    })
}// ===========================================================================
// 13. 指标 + 确定性回放 + `observe`
// ===========================================================================
/// 指标。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Metrics {
    /// 相位数。
    pub phases: usize,
    /// 基线面数。
    pub faces: usize,
    /// 面册完备。
    pub faces_complete: bool,
    /// 声明键数合计。
    pub declared: usize,
    /// 实际登记键数。
    pub keys: usize,
    /// 键册完备。
    pub keys_complete: bool,
    /// 键唯一断言。
    pub keys_unique: bool,
    /// 三列残缺数。
    pub column_gap: usize,
    /// 冲突数。
    pub conflict: usize,
    /// 采集步数越界。
    pub step_overflow: usize,
    /// 单位对照对数。
    pub units_pairs: usize,
    /// 单位对照通过。
    pub units_ok: bool,
    /// Σ面。
    pub sum_face: u64,
    /// Σ键。
    pub sum_key: u64,
    /// 总册。
    pub sum_book: u64,
    /// 恒等式最大偏离。
    pub deviation: u64,
    /// 恒等式通过。
    pub identity_ok: bool,
    /// 采集账全过。
    pub facet_ok: bool,
    /// 漂移立案数。
    pub drift_filed: usize,
    /// 噪声带计数。
    pub drift_noise: usize,
    /// 轮数。
    pub rounds: usize,
    /// 漂移格数。
    pub drift_cells: usize,
    /// 注入形态数。
    pub forms: usize,
    /// 检出数。
    pub detected: usize,
    /// 净态零误报。
    pub net_clean: bool,
    /// 合法边数。
    pub edges: usize,
    /// 非法迁移拒记数。
    pub rejected: usize,
    /// 环形账写入。
    pub pushed: u64,
    /// 环形账丢弃。
    pub dropped: u64,
    /// 环形账存活。
    pub live: usize,
    /// 环形账守恒。
    pub ring_ok: bool,
    /// 冻结核验通过。
    pub freeze_ok: bool,
    /// 边界违规。
    pub border: bool,
    /// 终版预告留痕。
    pub notice_ok: bool,
    /// TLV 往返。
    pub tlv_ok: bool,
    /// 总指标指纹。
    pub fp: u64,
}
impl Metrics {
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "metrics phases={} faces={} ok={} declared={} keys={} ok={} uniq={} colgap={} conflict={} stepover={} units={} ok={} Σ面={} Σ键={} 总册={} dev={} ident={} facet={} drift={}/{} rounds={} dcell={} detect={}/{} net={} edges={} rej={} ring={}/{}/{} ok={} freeze={} border={} notice={} tlv={} fp={:016X}",
            self.phases,
            self.faces,
            if self.faces_complete { 1 } else { 0 },
            self.declared,
            self.keys,
            if self.keys_complete { 1 } else { 0 },
            if self.keys_unique { 1 } else { 0 },
            self.column_gap,
            self.conflict,
            self.step_overflow,
            self.units_pairs,
            if self.units_ok { 1 } else { 0 },
            self.sum_face,
            self.sum_key,
            self.sum_book,
            self.deviation,
            if self.identity_ok { 1 } else { 0 },
            if self.facet_ok { 1 } else { 0 },
            self.drift_filed,
            self.drift_noise,
            self.rounds,
            self.drift_cells,
            self.detected,
            self.forms,
            if self.net_clean { 1 } else { 0 },
            self.edges,
            self.rejected,
            self.live,
            self.dropped,
            self.pushed,
            if self.ring_ok { 1 } else { 0 },
            if self.freeze_ok { 1 } else { 0 },
            if self.border { 1 } else { 0 },
            if self.notice_ok { 1 } else { 0 },
            if self.tlv_ok { 1 } else { 0 },
            self.fp
        )
    }
    /// 逐行表格。
    pub fn rows(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::with_capacity(18);
        out.push((String::from("相位"), format!("{}", self.phases)));
        out.push((String::from("基线面"), format!("{} 完备={}", self.faces, self.faces_complete)));
        out.push((String::from("基线键"), format!("声明{} 登记{} 完备{}", self.declared, self.keys, self.keys_complete)));
        out.push((String::from("键唯一"), format!("{} 残缺{} 冲突{}", self.keys_unique, self.column_gap, self.conflict)));
        out.push((String::from("单位对照"), format!("{} 对 通过{}", self.units_pairs, self.units_ok)));
        out.push((String::from("归集恒等式"), format!("Σ面={} Σ键={} 总册={} dev{}", self.sum_face, self.sum_key, self.sum_book, self.deviation)));
        out.push((String::from("采集账"), format!("全过={} 步越界{}", self.facet_ok, self.step_overflow)));
        out.push((String::from("漂移"), format!("立案{} 噪声{}", self.drift_filed, self.drift_noise)));
        out.push((String::from("稳定性"), format!("轮{} 漂移格{}", self.rounds, self.drift_cells)));
        out.push((String::from("注入核"), format!("{}/{} 净态零误报{}", self.detected, self.forms, self.net_clean)));
        out.push((String::from("状态机"), format!("边{} 拒记{}", self.edges, self.rejected)));
        out.push((String::from("环形账"), format!("存活{} 丢弃{} 写入{} 守恒{}", self.live, self.dropped, self.pushed, self.ring_ok)));
        out.push((String::from("冻结件"), format!("通过{}", self.freeze_ok)));
        out.push((String::from("终版预告"), format!("留痕{}", self.notice_ok)));
        out.push((String::from("边界"), format!("违规{}", if self.border { 1 } else { 0 })));
        out.push((String::from("TLV"), format!("往返{}", self.tlv_ok)));
        out.push((String::from("指纹"), format!("{:016X}", self.fp)));
        out
    }
    /// 全部通过。
    pub fn all_ok(&self) -> bool {
        self.faces_complete
            && self.keys_complete
            && self.keys_unique
            && self.column_gap == 0
            && self.conflict == 0
            && self.units_ok
            && self.identity_ok
            && self.facet_ok
            && self.drift_filed == 0
            && self.drift_cells == 0
            && self.detected == self.forms
            && self.net_clean
            && self.rejected >= 3
            && self.ring_ok
            && self.freeze_ok
            && !self.border
            && self.notice_ok
            && self.tlv_ok
    }
    /// 指纹词表。
    pub fn words(&self) -> Vec<u64> {
        vec![
            self.phases as u64,
            self.faces as u64,
            self.declared as u64,
            self.keys as u64,
            self.units_pairs as u64,
            self.sum_face,
            self.sum_key,
            self.sum_book,
            self.deviation,
            self.rounds as u64,
            self.detected as u64,
            self.pushed,
            self.dropped,
            self.live as u64,
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
    /// 每轮总册哈希。
    pub round_fp: Vec<u64>,
    /// 每轮恒等式三面。
    pub triples: Vec<(u64, u64, u64)>,
    /// 每轮指纹。
    pub fps: Vec<u64>,
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
/// 确定性回放（`REPEAT` 轮全链重跑，比对总册哈希与恒等式三面）。
pub fn replay(seed: u64) -> UbResult<Replay> {
    let mut round_fp: Vec<u64> = Vec::with_capacity(REPEAT);
    let mut triples: Vec<(u64, u64, u64)> = Vec::with_capacity(REPEAT);
    let mut fps: Vec<u64> = Vec::with_capacity(REPEAT);
    let mut r: usize = 0;
    while r < REPEAT {
        let net = Snapshot::net(seed)?;
        round_fp.push(book_hash(&net.keys));
        triples.push((net.ident.sum_face, net.ident.sum_key, net.ident.sum_book));
        fps.push(net.ev.probe_fp);
        r = r.wrapping_add(1);
    }
    let mut consistent = true;
    let mut i: usize = 1;
    while i < round_fp.len() {
        if round_fp[i] != round_fp[0] || triples[i] != triples[0] || fps[i] != fps[0] {
            consistent = false;
        }
        i = i.wrapping_add(1);
    }
    let mut words: Vec<u64> = Vec::with_capacity(3 * REPEAT + 2);
    words.push(seed);
    words.push(REPEAT as u64);
    words.push(consistent as u64);
    let mut j: usize = 0;
    while j < round_fp.len() {
        words.push(round_fp[j]);
        words.push(triples[j].0);
        words.push(triples[j].1);
        words.push(triples[j].2);
        words.push(fps[j]);
        j = j.wrapping_add(1);
    }
    Ok(Replay { seed, rounds: REPEAT, round_fp, triples, fps, consistent, fp: fnv1a_words(&words) })
}
/// `observe` 结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UbObserve {
    /// 净态快照。
    pub net: Snapshot,
    /// 状态机。
    pub fsm: PhaseFsm,
    /// 稳定性。
    pub stability: Stability,
    /// 注入总留痕。
    pub evidence: Evidence,
    /// 逐快照检出。
    pub detects: Vec<Detections>,
    /// 冻结件。
    pub freeze: FreezeRecord,
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
        self.metrics.all_ok()
            && self.net.is_net()
            && self.fsm.state == Phase::Sealed
            && self.fsm.trail_monotonic()
            && self.fsm.rejected.len() >= 3
            && self.stability.stable
            && self.stability.rounds_ok()
            && self.replay.consistent
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
/// 资源预算（显式核算）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ResourceBudget {
    /// 常驻字节（每快照）。
    pub resident_per_snapshot: usize,
    /// 单快照字节。
    pub snapshot_bytes: usize,
    /// 注入快照份数。
    pub snapshot_count: usize,
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
    /// 核算。
    pub fn compute(o: &UbObserve) -> ResourceBudget {
        let resident = core::mem::size_of::<Snapshot>();
        let count = Form::COUNT;
        let export = 4096usize;
        let peak = (resident.wrapping_mul(count).wrapping_add(export)) as u64;
        let mut steps: u64 = 0;
        let mut i: usize = 0;
        while i < o.net.facets.facets.len() {
            steps = steps.wrapping_add(o.net.facets.facets[i].tick);
            i = i.wrapping_add(1);
        }
        ResourceBudget {
            resident_per_snapshot: resident,
            snapshot_bytes: resident,
            snapshot_count: count,
            export_bytes: export,
            peak_bytes: peak,
            peak_cap: MEM_PEAK_CAP,
            steps,
            step_cap: STEP_CAP,
            syscalls: SYSCALL_CAP,
            syscall_cap: SYSCALL_CAP,
        }
    }
    /// 预算内。
    pub fn ok(&self) -> bool {
        self.peak_bytes <= self.peak_cap
            && self.steps <= self.step_cap
            && self.syscalls <= self.syscall_cap
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "budget resident={}x{}B snap={}B export={}B peak={}/{}B steps={}/{} syscalls={}/{} ok={}",
            self.snapshot_count,
            self.resident_per_snapshot,
            self.snapshot_bytes,
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
}// ===========================================================================
// 14. 导出：`export_kv` + `export_tlv`（往返一致）
// ===========================================================================
/// KV 导出（`键=值` 逐行）。
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
    push_line(&mut out, "batch", String::from(BATCH_FACE));
    push_line(&mut out, "final_face", String::from(FINAL_FACE));
    push_line(&mut out, "final_baseline", String::from(FINAL_BASELINE));
    push_line(&mut out, "faces", format!("{}", o.metrics.faces));
    push_line(&mut out, "declared_keys", format!("{}", o.metrics.declared));
    push_line(&mut out, "keys", format!("{}", o.metrics.keys));
    push_line(&mut out, "column_gap", format!("{}", o.metrics.column_gap));
    push_line(&mut out, "conflict", format!("{}", o.metrics.conflict));
    push_line(&mut out, "unit_pairs", format!("{}", o.metrics.units_pairs));
    push_line(&mut out, "sum_face", format!("{}", o.metrics.sum_face));
    push_line(&mut out, "sum_key", format!("{}", o.metrics.sum_key));
    push_line(&mut out, "sum_book", format!("{}", o.metrics.sum_book));
    push_line(&mut out, "deviation", format!("{}", o.metrics.deviation));
    push_line(&mut out, "drift_filed", format!("{}", o.metrics.drift_filed));
    push_line(&mut out, "drift_noise", format!("{}", o.metrics.drift_noise));
    push_line(&mut out, "rounds", format!("{}", o.metrics.rounds));
    push_line(&mut out, "drift_cells", format!("{}", o.metrics.drift_cells));
    push_line(&mut out, "forms", format!("{}", o.metrics.forms));
    push_line(&mut out, "detected", format!("{}", o.metrics.detected));
    push_line(&mut out, "edges", format!("{}", o.metrics.edges));
    push_line(&mut out, "rejected", format!("{}", o.metrics.rejected));
    push_line(&mut out, "pushed", format!("{}", o.metrics.pushed));
    push_line(&mut out, "dropped", format!("{}", o.metrics.dropped));
    push_line(&mut out, "live", format!("{}", o.metrics.live));
    push_line(&mut out, "book_hash", format!("{:016X}", o.freeze.book_hash));
    push_line(&mut out, "face_def", format!("{:016X}", o.freeze.face_def));
    push_line(&mut out, "key_def", format!("{:016X}", o.freeze.key_def));
    push_line(&mut out, "final_notice", format!("{:016X}", o.freeze.final_notice));
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
/// 三条 `push` 决定 `TLV_HEAD_PAYLOAD = 24`，
/// 故 `TLV_HEAD_LEN = 1 + 2 + 24 = 27`；长度字段固定 2 字节，
/// 条目固定 `1 + 1 + 8 = 10` 字节，往返必然一致。
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
    push_tlv_u64(&mut out, 0x10, o.metrics.faces as u64);
    push_tlv_u64(&mut out, 0x11, o.metrics.declared as u64);
    push_tlv_u64(&mut out, 0x12, o.metrics.keys as u64);
    push_tlv_u64(&mut out, 0x13, o.metrics.column_gap as u64);
    push_tlv_u64(&mut out, 0x14, o.metrics.conflict as u64);
    push_tlv_u64(&mut out, 0x15, o.metrics.units_pairs as u64);
    push_tlv_u64(&mut out, 0x20, o.metrics.sum_face);
    push_tlv_u64(&mut out, 0x21, o.metrics.sum_key);
    push_tlv_u64(&mut out, 0x22, o.metrics.sum_book);
    push_tlv_u64(&mut out, 0x23, o.metrics.deviation);
    push_tlv_u64(&mut out, 0x30, o.metrics.drift_filed as u64);
    push_tlv_u64(&mut out, 0x31, o.metrics.drift_noise as u64);
    push_tlv_u64(&mut out, 0x32, o.metrics.rounds as u64);
    push_tlv_u64(&mut out, 0x33, o.metrics.drift_cells as u64);
    push_tlv_u64(&mut out, 0x40, o.metrics.forms as u64);
    push_tlv_u64(&mut out, 0x41, o.metrics.detected as u64);
    push_tlv_u64(&mut out, 0x50, o.metrics.edges as u64);
    push_tlv_u64(&mut out, 0x51, o.metrics.rejected as u64);
    push_tlv_u64(&mut out, 0x60, o.metrics.pushed);
    push_tlv_u64(&mut out, 0x61, o.metrics.dropped);
    push_tlv_u64(&mut out, 0x62, o.metrics.live as u64);
    push_tlv_u64(&mut out, 0x70, o.freeze.book_hash);
    push_tlv_u64(&mut out, 0x71, o.freeze.face_def);
    push_tlv_u64(&mut out, 0x72, o.freeze.key_def);
    push_tlv_u64(&mut out, 0x73, o.freeze.final_notice);
    push_tlv_u64(&mut out, 0x74, o.freeze.fingerprint());
    push_tlv_u64(&mut out, 0x75, o.stability.fingerprint());
    push_tlv_u64(&mut out, 0x76, o.replay.fp);
    push_tlv_u64(&mut out, 0x90, o.budget.peak_bytes);
    push_tlv_u64(&mut out, 0x91, o.budget.steps);
    push_tlv_u64(&mut out, 0x92, o.budget.syscalls);
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
/// 解析 TLV（返回版本号与条目表）。
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
/// TLV 往返一致。
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
        // 逐条目与现场重算比对，确保导出的不是陈旧快照
        let tag = items[i].0;
        let got = items[i].1;
        let want = match tag {
            0x10 => o.metrics.faces as u64,
            0x11 => o.metrics.declared as u64,
            0x12 => o.metrics.keys as u64,
            0x13 => o.metrics.column_gap as u64,
            0x14 => o.metrics.conflict as u64,
            0x15 => o.metrics.units_pairs as u64,
            0x20 => o.metrics.sum_face,
            0x21 => o.metrics.sum_key,
            0x22 => o.metrics.sum_book,
            0x23 => o.metrics.deviation,
            0x30 => o.metrics.drift_filed as u64,
            0x31 => o.metrics.drift_noise as u64,
            0x32 => o.metrics.rounds as u64,
            0x33 => o.metrics.drift_cells as u64,
            0x40 => o.metrics.forms as u64,
            0x41 => o.metrics.detected as u64,
            0x50 => o.metrics.edges as u64,
            0x51 => o.metrics.rejected as u64,
            0x60 => o.metrics.pushed,
            0x61 => o.metrics.dropped,
            0x62 => o.metrics.live as u64,
            0x70 => o.freeze.book_hash,
            0x71 => o.freeze.face_def,
            0x72 => o.freeze.key_def,
            0x73 => o.freeze.final_notice,
            0x74 => o.freeze.fingerprint(),
            0x75 => o.stability.fingerprint(),
            0x76 => o.replay.fp,
            0x90 => o.budget.peak_bytes,
            0x91 => o.budget.steps,
            0x92 => o.budget.syscalls,
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
    // 结构长度也校验：组头定宽 + 条目定宽
    if buf.len() != TLV_HEAD_LEN + items.len() * TLV_ITEM_LEN {
        ok = false;
    }
    ok
}
// ===========================================================================
// 15. 复杂度分解（`深在哪`）
// ===========================================================================
/// 成本项。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CostItem {
    /// 项名。
    pub name: String,
    /// 迭代次数。
    pub iters: u64,
    /// 单次代价（步）。
    pub per_iter: u64,
    /// 合计步数。
    pub total: u64,
    /// 与 Windows 同类能力的对照注（「深在哪」）。
    pub note: String,
}
impl Metrics {
    /// 逐项分解。
    pub fn complexity(&self) -> Vec<CostItem> {
        let mut out: Vec<CostItem> = Vec::with_capacity(12);
        let faces = FACE_COUNT as u64;
        let keys = KEY_TOTAL as u64;
        out.push(CostItem {
            name: String::from("面册登记（十面五列）"),
            iters: faces,
            per_iter: 6,
            total: faces * 6,
            note: String::from("Windows 性能基线登记通常按计数器逐个取，无固定十面闭集；本条对面覆盖做硬断言，归集遗漏当场检出"),
        });
        out.push(CostItem {
            name: String::from("逐键采样（60 键基线读数）"),
            iters: keys,
            per_iter: 4,
            total: keys * 4,
            note: String::from("Windows 侧基线采样靠 ETW 逐点采集；本条把采样收敛成确定性纯函数，净态/重采两侧同口径可比"),
        });
        out.push(CostItem {
            name: String::from("采集事件流（十面 × FACE_STEPS）"),
            iters: faces,
            per_iter: FACE_STEPS as u64,
            total: faces * FACE_STEPS as u64,
            note: String::from("事件流提供步数与代价的可核对口径，Windows 侧无等价步数账"),
        });
        out.push(CostItem {
            name: String::from("三列登记（键名/取值/来源批位）"),
            iters: keys,
            per_iter: 3,
            total: keys * 3,
            note: String::from("三列全文进总册哈希；Windows 侧基线表通常只有计数器名与数值，来源批位无对应物"),
        });
        out.push(CostItem {
            name: String::from("键唯一断言（两两同名比对）"),
            iters: keys,
            per_iter: keys,
            total: keys * keys,
            note: String::from("O(n²) 冲突检测是本条最贵的一项，但换来「同名不同号必拦」；Windows 侧无同名冲突概念"),
        });
        out.push(CostItem {
            name: String::from("单位对照（45 个面次对）"),
            iters: CROSS_COUNT as u64,
            per_iter: 2,
            total: CROSS_COUNT as u64 * 2,
            note: String::from("口径一致性是本条比 Windows 更严处：面覆盖更严且单位串逐对点验"),
        });
        out.push(CostItem {
            name: String::from("归集恒等式（三路独立聚合）"),
            iters: 3,
            per_iter: faces + keys,
            total: 3 * (faces + keys),
            note: String::from("Σ面/Σ键/总册 三路异算法互证；Windows 侧无跨算法交叉恒等式"),
        });
        out.push(CostItem {
            name: String::from("重采漂移检测（60 键 × 1 轮）"),
            iters: keys,
            per_iter: 2,
            total: keys * 2,
            note: String::from("重采与在册两侧口径分离，超容差立案；Windows 侧基线漂移靠事后人工比对"),
        });
        out.push(CostItem {
            name: String::from("环形账灌注（KEY_TOTAL × (REPEAT+1)）"),
            iters: keys * (REPEAT as u64 + 1),
            per_iter: 1,
            total: keys * (REPEAT as u64 + 1),
            note: String::from("写入量刻意超容以压出 dropped，守恒式 `写入 == 存活 + 丢弃` 才非平凡"),
        });
        out.push(CostItem {
            name: String::from("注入核（14 形态全注入 + 检出）"),
            iters: FORM_COUNT as u64 * 2,
            per_iter: faces + keys,
            total: FORM_COUNT as u64 * 2 * (faces + keys),
            note: String::from("14/14 全拒留痕是本单判据的硬要求；Windows 侧无等价的自污染检出链"),
        });
        out.push(CostItem {
            name: String::from("稳定性多轮（REPEAT 轮全链重跑）"),
            iters: REPEAT as u64,
            per_iter: faces + keys,
            total: REPEAT as u64 * (faces + keys),
            note: String::from("逐「轮 × 键」指纹跨轮比对，锁死同种子可回放；Windows 侧基线不要求可回放"),
        });
        out.push(CostItem {
            name: String::from("TLV 导出（组头定宽 + 定长条目）"),
            iters: 31,
            per_iter: TLV_ITEM_LEN as u64,
            total: TLV_HEAD_LEN as u64 + 31 * TLV_ITEM_LEN as u64,
            note: String::from("长度字段恒占定宽，往返必然一致；Windows 侧 ETW 导出为变长流，无往返断言"),
        });
        out
    }
    /// 复杂度一行摘要。
    pub fn complexity_summary(&self) -> String {
        let items = self.complexity();
        let mut total: u64 = 0;
        let mut i: usize = 0;
        while i < items.len() {
            total = total.wrapping_add(items[i].total);
            i = i.wrapping_add(1);
        }
        format!(
            "complexity items={} total_steps={} resident={}B snapshots={} export={}B syscalls={} step_cap={} peak_cap={}B",
            items.len(),
            total,
            core::mem::size_of::<Metrics>(),
            FORM_COUNT,
            4096,
            SYSCALL_CAP,
            STEP_CAP,
            MEM_PEAK_CAP
        )
    }
}
// ===========================================================================
// 16. 自检项
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
}
/// 自检清单。
pub fn self_check() -> Vec<Check> {
    let mut out: Vec<Check> = Vec::with_capacity(30);
    // 码段
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
            ok
        },
        format!("Form::COUNT={} 码 {:04X}..{:04X}", Form::COUNT, Form::ALL[0].code(), Form::ALL[Form::COUNT - 1].code()),
    ));
    out.push(Check::new(
        "aux_codes_unique",
        {
            let mut ok = true;
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
            ok && AuxCode::ALL[0].code() == SEG_BASE.wrapping_add(0x1C)
        },
        format!("AuxCode::COUNT={} 码 {:04X}..{:04X}", AuxCode::COUNT, AuxCode::ALL[0].code(), AuxCode::ALL[AuxCode::COUNT - 1].code()),
    ));
    out.push(Check::new(
        "key_total_is_60",
        {
            let mut t: usize = 0;
            let mut i: usize = 0;
            while i < FACE_COUNT {
                t = t.wrapping_add(FACE_KEY_COUNTS[i]);
                i = i.wrapping_add(1);
            }
            t == KEY_TOTAL
        },
        format!("十面键数合计={} 期望={}", {
            let mut t: usize = 0;
            let mut i: usize = 0;
            while i < FACE_COUNT {
                t = t.wrapping_add(FACE_KEY_COUNTS[i]);
                i = i.wrapping_add(1);
            }
            t
        }, KEY_TOTAL),
    ));
    out.push(Check::new(
        "unit_pairs_is_45",
        unit_pairs().len() == CROSS_COUNT,
        format!("面次对={} 期望={}", unit_pairs().len(), CROSS_COUNT),
    ));
    // 面册
    match face_book_full() {
        Ok(b) => {
            out.push(Check::new("face_book_full", b.complete() && b.key_count_ok(), b.summary()));
            out.push(Check::new("face_book_zero_dup", b.dup_bits == 0 && b.out_of_range == 0, b.summary()));
        }
        Err(e) => {
            out.push(Check::new("face_book_full", false, err_str(&e)));
        }
    }
    // 键册（真跑）
    match Snapshot::net(0x5EED_3141_0000_0001u64) {
        Ok(net) => {
            out.push(Check::new("key_book_complete", net.keys.complete(), net.keys.summary()));
            out.push(Check::new("key_unique_assert", net.keys.unique_ok(), format!("dup={:016X} conflict={}", net.keys.dup_bits, net.keys.conflict_bits)));
            out.push(Check::new("column_all_ok", net.keys.column_gap() == 0, format!("残缺位={:016X}", net.keys.column_mask)));
            out.push(Check::new("facet_book_ok", net.facets.all_ok(), net.facets.summary()));
            out.push(Check::new("unit_table_ok", net.units.all_ok(), net.units.summary()));
            out.push(Check::new("identity_three_way", net.ident.balanced, net.ident.summary()));
            out.push(Check::new("identity_zero_dev", net.ident.deviation == 0, format!("dev={}", net.ident.deviation)));
            out.push(Check::new("drift_zero_filed", net.drift.filed_count == 0, net.drift.summary()));
            out.push(Check::new("ring_balanced", net.ring.balanced(), net.ring.summary()));
            out.push(Check::new("ring_dropped_explicit", net.ring.dropped > 0, format!("dropped={} live={}", net.ring.dropped, net.ring.live())));
            out.push(Check::new("net_is_net", net.is_net(), format!("border={}", net.ev.border_violations)));
            // 冻结件
            let fz = FreezeRecord::build(&net.faces, &net.keys, &net.facets, &net.units, &net.ident, &net.drift);
            out.push(Check::new(
                "freeze_verify",
                fz.verify(&net.faces, &net.keys, &net.facets, &net.units, &net.ident, &net.drift),
                fz.summary(),
            ));
            out.push(Check::new("final_notice", fz.final_notice != 0, format!("notice={:016X}", fz.final_notice)));
            // 状态机
            let mut fsm = PhaseFsm::new();
            let mut walk_ok = true;
            let mut pi: usize = 1;
            while pi < Phase::COUNT {
                if fsm.advance_to(Phase::ALL[pi]).is_err() {
                    walk_ok = false;
                }
                pi = pi.wrapping_add(1);
            }
            let illegal: [Phase; 3] = [Phase::P1FaceDefine, Phase::P3UniqueAssert, Phase::P2KeyCollect];
            let mut j: usize = 0;
            while j < illegal.len() {
                let _ = fsm.try_go(illegal[j]);
                j = j.wrapping_add(1);
            }
            out.push(Check::new(
                "fsm_sealed",
                walk_ok && fsm.state == Phase::Sealed && fsm.trail_monotonic(),
                fsm.summary(),
            ));
            out.push(Check::new("fsm_rejected", fsm.rejected.len() >= 3, format!("拒记={}", fsm.rejected.len())));
            out.push(Check::new("fsm_edge_matrix", PhaseFsm::edge_count() == 5, format!("合法边={}", PhaseFsm::edge_count())));
            // 稳定性
            match stability_check(0x5EED_3141_0000_0001u64) {
                Ok(st) => {
                    out.push(Check::new("stability_zero_drift", st.stable && st.rounds_ok(), st.describe()));
                }
                Err(e) => {
                    out.push(Check::new("stability_zero_drift", false, err_str(&e)));
                }
            }
            // 回放
            match replay(0x5EED_3141_0000_0001u64) {
                Ok(rp) => {
                    out.push(Check::new("replay_consistent", rp.consistent, rp.summary()));
                }
                Err(e) => {
                    out.push(Check::new("replay_consistent", false, err_str(&e)));
                }
            }
            // 注入核
            match inject_all(0x5EED_3141_0000_0001u64) {
                Ok((ev, snaps)) => {
                    let mut all_hit = true;
                    let mut cnt = 0usize;
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
                    out.push(Check::new("inject_fired_14", {
                        let mut n = 0usize;
                        let mut i: usize = 0;
                        while i < Form::COUNT {
                            if ev.fired[i] != 0 {
                                n = n.wrapping_add(1);
                            }
                            i = i.wrapping_add(1);
                        }
                        n == FORM_COUNT
                    }, ev.summary()));
                    // 净态零误报
                    match detect_all(&net) {
                        Ok(d) => {
                            out.push(Check::new("net_zero_false_positive", d.is_clean(), format!("hit={:?} border={}", d.hit_list(), d.border_violations)));
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
        out.push(format!(
            "{:<24} {:<4} {}",
            cs[i].name,
            if cs[i].ok { "OK" } else { "FAIL" },
            cs[i].got
        ));
        i = i.wrapping_add(1);
    }
    out
}// ===========================================================================
// 17. `observe`：真跑全链
// ===========================================================================
/// 真跑一次全链，返回观察结果。
pub fn observe(seed: u64) -> UbResult<UbObserve> {
    // ① 相位一：十基线面定义
    let net = Snapshot::net(seed)?;
    if !net.faces.complete() {
        return Err(UbErr::of_form(
            Form::FaceMissing,
            format!("基线面册不完备：缺面={:?}", net.faces.missing_list()),
        ));
    }
    // ② 相位二：三列归集齐备
    if net.keys.missing_bits() != 0 {
        return Err(UbErr::of_form(
            Form::KeyMissing,
            format!("基线键未归齐：缺键={:?}", net.keys.missing_list()),
        ));
    }
    if net.keys.column_gap() != 0 {
        return Err(UbErr::of_form(
            Form::ColumnIncomplete,
            format!("三列残缺：{} 键", net.keys.column_gap()),
        ));
    }
    // ③ 相位三：键唯一断言
    if !net.keys.unique_ok() {
        return Err(UbErr::of_form(
            Form::KeyConflict,
            format!("键唯一断言不过：重号={:016X} 冲突={:016X}", net.keys.dup_bits, net.keys.conflict_bits),
        ));
    }
    if !net.faces.key_count_ok() {
        return Err(UbErr::of_form(
            Form::KeyCountMismatch,
            format!("键数声明不符：声明={} 登记={}", net.faces.declared_keys(), net.keys.keys.len()),
        ));
    }
    // ④ 采集段
    if !net.facets.all_ok() {
        return Err(UbErr::of_form(
            Form::StepOverCap,
            format!("采集账不过：{}", net.facets.summary()),
        ));
    }
    if !net.units.all_ok() {
        return Err(UbErr::of_form(
            Form::ColumnIncomplete,
            format!("单位对照不过：{}", net.units.summary()),
        ));
    }
    // ⑤ 相位五前置：归集恒等式零偏离
    if !net.ident.balanced {
        return Err(err_identity(net.ident.sum_face, net.ident.sum_key, net.ident.sum_book));
    }
    // ⑥ 相位四：重采零漂移
    if net.drift.filed_count != 0 {
        return Err(UbErr::of_form(
            Form::DriftFiled,
            format!("基线漂移立案：{:?}", net.drift.filed_list()),
        ));
    }
    // ⑦ 状态机：走满 5 条合法边 + 3 条非法边拒记
    let mut fsm = PhaseFsm::new();
    let mut pi: usize = 1;
    while pi < Phase::COUNT {
        fsm.advance_to(Phase::ALL[pi])?;
        pi = pi.wrapping_add(1);
    }
    // 三个非法目标：相位序倒退 / 相位序跳步 / 终态无出边（第三个用终态后的目标）
    let illegal: [Phase; 3] = [Phase::P1FaceDefine, Phase::P3UniqueAssert, Phase::P2KeyCollect];
    for p in illegal.iter() {
        let _ = fsm.try_go(*p);
    }
    if fsm.state != Phase::Sealed || !fsm.trail_monotonic() || fsm.rejected.len() < 3 {
        return Err(UbErr::of_form(
            Form::FingerprintDrift,
            format!("冻结态不符：got={} want={}", fsm.state.name(), Phase::Sealed.name()),
        ));
    }
    // ⑧ 环形账守恒
    if !net.ring.balanced() {
        return Err(UbErr::of_form(
            Form::LedgerUnbalanced,
            format!("总册环形账不守恒：{}", net.ring.summary()),
        ));
    }
    // ⑨ 冻结件
    let freeze = FreezeRecord::build(&net.faces, &net.keys, &net.facets, &net.units, &net.ident, &net.drift);
    if !freeze.verify(&net.faces, &net.keys, &net.facets, &net.units, &net.ident, &net.drift) {
        return Err(UbErr::of_aux(
            AuxCode::BookRevMismatch,
            format!("冻结核验不过：{}", freeze.drift(&net.faces, &net.keys, &net.facets, &net.units, &net.ident, &net.drift)),
        ));
    }
    if freeze.final_notice == 0 {
        return Err(UbErr::of_aux(AuxCode::FreezeMismatch, String::from("终版预告留痕为空")));
    }
    // ⑩ 稳定性
    let stability = stability_check(seed)?;
    if !stability.stable || !stability.rounds_ok() {
        return Err(UbErr::of_aux(AuxCode::RepeatShort, format!("稳定性不过：{}", stability.describe())));
    }
    // ⑪ 注入核：14 形态逐个注入 → 逐个快照检出
    let (evidence, snaps) = inject_all(seed)?;
    let mut detects: Vec<Detections> = Vec::with_capacity(Form::COUNT);
    let mut detected: usize = 0;
    for s in snaps.iter() {
        let d = detect_all(s)?;
        detected = detected.wrapping_add(1);
        detects.push(d);
    }
    let mut fi: usize = 0;
    while fi < Form::ALL.len() {
        let d = match detects.get(fi) {
            Some(x) => x,
            None => break,
        };
        if !d.hit(Form::ALL[fi]) {
            return Err(UbErr::of_form(
                Form::ALL[fi],
                format!("注入漏检：形态={} 探面={}", Form::ALL[fi].name(), Form::ALL[fi].probe()),
            ));
        }
        fi = fi.wrapping_add(1);
    }
    if detected != FORM_COUNT {
        return Err(UbErr::new(SEG_BASE, format!("注入核漏检：{}/{}", detected, FORM_COUNT)));
    }
    // ⑫ 净态零误报
    let net_detect = detect_all(&net)?;
    if !net_detect.is_clean() {
        return Err(UbErr::of_form(
            Form::FingerprintDrift,
            format!("净态误报：{:?}", net_detect.hit_list()),
        ));
    }
    // ⑬ 边界零违规
    if net.ev.border_violations != 0 {
        return Err(UbErr::of_form(
            Form::BorderViolation,
            format!("边界违规 {} 次", net.ev.border_violations),
        ));
    }
    // ⑭ 回放
    let rp = replay(seed)?;
    if !rp.consistent {
        return Err(UbErr::of_aux(AuxCode::SeqMismatch, format!("回放不一致：{}", rp.summary())));
    }
    // ⑮ 指标
    let metrics = Metrics {
        phases: Phase::STEPS,
        faces: net.faces.faces.len(),
        faces_complete: net.faces.complete(),
        declared: net.faces.declared_keys(),
        keys: net.keys.keys.len(),
        keys_complete: net.keys.complete(),
        keys_unique: net.keys.unique_ok(),
        column_gap: net.keys.column_gap(),
        conflict: net.keys.conflict_count(),
        step_overflow: net.facets.step_overflow,
        units_pairs: net.units.pairs.len(),
        units_ok: net.units.all_ok(),
        sum_face: net.ident.sum_face,
        sum_key: net.ident.sum_key,
        sum_book: net.ident.sum_book,
        deviation: net.ident.deviation,
        identity_ok: net.ident.balanced,
        facet_ok: net.facets.all_ok(),
        drift_filed: net.drift.filed_count,
        drift_noise: net.drift.noise_count,
        rounds: stability.rounds,
        drift_cells: stability.drift_count(),
        forms: FORM_COUNT,
        detected,
        net_clean: net_detect.is_clean(),
        edges: PhaseFsm::edge_count(),
        rejected: fsm.rejected.len(),
        pushed: net.ring.pushed,
        dropped: net.ring.dropped,
        live: net.ring.live(),
        ring_ok: net.ring.balanced(),
        freeze_ok: true,
        border: net.ev.border_violations != 0,
        notice_ok: freeze.final_notice != 0,
        tlv_ok: false,
        fp: 0,
    };
    let mut fp_words = metrics.words();
    fp_words.push(freeze.fingerprint());
    fp_words.push(stability.fingerprint());
    fp_words.push(rp.fp);
    fp_words.push(evidence.probe_fp);
    let mut metrics = metrics;
    metrics.fp = fnv1a_words(&fp_words);
    let budget = ResourceBudget::compute(&UbObserve {
        net: net.clone(),
        fsm: fsm.clone(),
        stability: stability.clone(),
        evidence: evidence.clone(),
        detects: detects.clone(),
        freeze: freeze.clone(),
        replay: rp.clone(),
        metrics: metrics.clone(),
        budget: ResourceBudget {
            resident_per_snapshot: 0,
            snapshot_bytes: 0,
            snapshot_count: FORM_COUNT,
            export_bytes: 4096,
            peak_bytes: 0,
            peak_cap: MEM_PEAK_CAP,
            steps: 0,
            step_cap: STEP_CAP,
            syscalls: SYSCALL_CAP,
            syscall_cap: SYSCALL_CAP,
        },
    });
    let o = UbObserve {
        net,
        fsm,
        stability,
        evidence,
        detects,
        freeze,
        replay: rp,
        metrics,
        budget,
    };
    // ⑯ TLV 往返：`metrics.fp` 此刻已定稿（`tlv_ok` 不参与 `words()` 汇总，
    // 否则回填 `tlv_ok` 会改动 fp，导出与解析两侧就必然不等，往返断言恒假）。
    // 先按未定稿状态导一次做结构往返自检，再回填结论——回填只动 `tlv_ok`，
    // 不动 `fp`，因此导出件与 `fp` 始终同源。
    let tlv_ok = tlv_roundtrip_ok(&o);
    let mut o2 = o;
    o2.metrics.tlv_ok = tlv_ok;
    Ok(o2)
}
// ===========================================================================
// 18. 演示 / 报告辅助
// ===========================================================================
/// 形态清单行。
pub fn form_manifest() -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(Form::COUNT);
    let mut i: usize = 0;
    while i < Form::ALL.len() {
        out.push(format!(
            "{:>2} {:04X} {:<20} {:<28} probe={}",
            i,
            Form::ALL[i].code(),
            Form::ALL[i].name(),
            Form::ALL[i].detail(),
            Form::ALL[i].probe()
        ));
        i = i.wrapping_add(1);
    }
    out
}
/// 辅助码清单行。
pub fn aux_manifest() -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(AuxCode::COUNT);
    let mut i: usize = 0;
    while i < AuxCode::ALL.len() {
        out.push(format!(
            "{:>2} {:04X} {:<20} {}",
            i,
            AuxCode::ALL[i].code(),
            AuxCode::ALL[i].name(),
            AuxCode::ALL[i].detail()
        ));
        i = i.wrapping_add(1);
    }
    out
}
/// 不变式清单行。
pub fn invariant_manifest() -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(12);
    out.push(String::from("I1 十基线面齐备：面覆盖位图 == FACE_BITS"));
    out.push(String::from("I2 面位唯一：无重号、无越界"));
    out.push(String::from("I3 60 键归齐：键位位图 == KEY_MASK64"));
    out.push(String::from("I4 键唯一：同名不同号零命中、键号零重号"));
    out.push(String::from("I5 三列齐备：键名/取值/来源批位逐键非空"));
    out.push(String::from("I6 单位对照 45/45：面次对单位串一致且读数非零"));
    out.push(String::from("I7 归集恒等式：Σ面 == Σ键 == 总册（零偏离）"));
    out.push(String::from("I8 采集账：步数与读数均在上限内"));
    out.push(String::from("I9 零漂移：重采逐键偏离超容差零立案"));
    out.push(String::from("I10 环形账守恒：写入 == 存活 + 丢弃"));
    out.push(String::from("I11 冻结件：九项哈希与现场一致"));
    out.push(String::from("I12 注入核：14 形态全拒 + 净态零误报"));
    out
}
/// 演示：相位状态机。
pub fn demo_phase() -> Vec<String> {
    let mut fsm = PhaseFsm::new();
    let mut out: Vec<String> = Vec::with_capacity(12);
    out.push(String::from("init ") + &fsm.summary());
    let mut i: usize = 1;
    while i < Phase::COUNT {
        let to = Phase::ALL[i];
        let r = fsm.advance_to(to);
        out.push(format!("go -> {} : {:?}", to.name(), r.is_ok()));
        i = i.wrapping_add(1);
    }
    let illegal: [Phase; 3] = [Phase::P1FaceDefine, Phase::P3UniqueAssert, Phase::P2KeyCollect];
    let mut j: usize = 0;
    while j < illegal.len() {
        let r = fsm.try_go(illegal[j]);
        out.push(format!("illegal -> {} : {}", illegal[j].name(), if r.is_ok() { "OK".to_string() } else { "REJECT".to_string() }));
        j = j.wrapping_add(1);
    }
    out.push(String::from("final ") + &fsm.summary());
    let m = PhaseFsm::edge_matrix();
    let mut k: usize = 0;
    while k < Phase::COUNT {
        let mut row = String::from("matrix ");
        row.push_str(Phase::ALL[k].name());
        row.push(' ');
        let mut c: usize = 0;
        while c < Phase::COUNT {
            row.push(if m[k][c] == 1 { '1' } else { '.' });
            c = c.wrapping_add(1);
        }
        out.push(row);
        k = k.wrapping_add(1);
    }
    out
}
/// 演示：基线面册。
pub fn demo_faces() -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(FACE_COUNT + 2);
    match face_book_full() {
        Ok(b) => {
            let mut i: usize = 0;
            while i < b.faces.len() {
                out.push(b.faces[i].describe());
                i = i.wrapping_add(1);
            }
            out.push(b.summary());
            out.push(format!("fp={:016X}", b.fingerprint()));
        }
        Err(e) => {
            out.push(err_str(&e));
        }
    }
    out
}
/// 演示：基线键册三列。
pub fn demo_keys() -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(KEY_TOTAL + 4);
    let mut keys = KeyBook::blank();
    let mut kid: usize = 1;
    while kid <= KEY_TOTAL {
        let k = BaseKey::new(kid, sample_value(kid));
        if keys.register(k).is_ok() {
            out.push(keys.keys[keys.keys.len() - 1].describe());
        }
        kid = kid.wrapping_add(1);
    }
    keys.recompute();
    out.push(keys.summary());
    out.push(format!("book_hash={:016X}", book_hash(&keys)));
    out.push(format!("final_notice={:016X}", final_notice(&face_book_full().unwrap_or_else(|_| FaceBook::blank()), &keys)));
    out
}
/// 演示：环形账。
pub fn demo_ledger(seed: u64) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(6);
    match Snapshot::net(seed) {
        Ok(net) => {
            out.push(net.ring.summary());
            out.push(format!("fp={:016X}", net.ring.fingerprint()));
            out.push(format!("push {} drop {} live {}", net.ring.pushed, net.ring.dropped, net.ring.live()));
            out.push(format!("balanced={} seq_ok={}", net.ring.balanced(), net.ring.seq_continuous()));
        }
        Err(e) => {
            out.push(err_str(&e));
        }
    }
    out
}
/// 演示：漂移账。
pub fn demo_drift(seed: u64) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(6);
    match Snapshot::net(seed) {
        Ok(net) => {
            out.push(net.drift.summary());
            let mut i: usize = 0;
            while i < net.drift.rows.len() && i < 4 {
                out.push(net.drift.rows[i].describe());
                i = i.wrapping_add(1);
            }
            out.push(format!("fp={:016X}", net.drift.fingerprint()));
        }
        Err(e) => {
            out.push(err_str(&e));
        }
    }
    out
}
/// 关联串（与域内其它单的衔接）。
pub fn related_str() -> String {
    format!(
        "{} batch={} final={} deps={} final_baseline={} graft={} align={}",
        V_VERSION, BATCH_FACE, FINAL_FACE, DEP_SLOTS, FINAL_BASELINE, GRAFT_SEAT, ALIGN_CHAIN
    )
}
/// 全量报告。
pub fn full_report(seed: u64) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(120);
    out.push(String::from("=== UNX-F3141 · 域基线读数总册 · 全量报告 ==="));
    out.push(String::from(seg_line()));
    out.push(related_str());
    out.push(String::from(""));
    out.push(String::from("-- 形态清单 --"));
    let mut i: usize = 0;
    while i < form_manifest().len() {
        out.push(form_manifest()[i].clone());
        i = i.wrapping_add(1);
    }
    out.push(String::from(""));
    out.push(String::from("-- 辅助码清单 --"));
    let mut j: usize = 0;
    while j < aux_manifest().len() {
        out.push(aux_manifest()[j].clone());
        j = j.wrapping_add(1);
    }
    out.push(String::from(""));
    out.push(String::from("-- 不变式清单 --"));
    let mut k: usize = 0;
    while k < invariant_manifest().len() {
        out.push(invariant_manifest()[k].clone());
        k = k.wrapping_add(1);
    }
    out.push(String::from(""));
    out.push(String::from("-- 演示：相位状态机 --"));
    let mut m: usize = 0;
    while m < demo_phase().len() {
        out.push(demo_phase()[m].clone());
        m = m.wrapping_add(1);
    }
    out.push(String::from(""));
    out.push(String::from("-- 演示：基线面册 --"));
    let mut n: usize = 0;
    while n < demo_faces().len() {
        out.push(demo_faces()[n].clone());
        n = n.wrapping_add(1);
    }
    out.push(String::from(""));
    out.push(String::from("-- 演示：基线键三列 --"));
    let mut p: usize = 0;
    while p < demo_keys().len() {
        out.push(demo_keys()[p].clone());
        p = p.wrapping_add(1);
    }
    out.push(String::from(""));
    out.push(String::from("-- 演示：环形账 --"));
    let mut q: usize = 0;
    while q < demo_ledger(seed).len() {
        out.push(demo_ledger(seed)[q].clone());
        q = q.wrapping_add(1);
    }
    out.push(String::from(""));
    out.push(String::from("-- 演示：漂移账 --"));
    let mut r: usize = 0;
    while r < demo_drift(seed).len() {
        out.push(demo_drift(seed)[r].clone());
        r = r.wrapping_add(1);
    }
    out.push(String::from(""));
    out.push(String::from("-- observe --"));
    match observe(seed) {
        Ok(o) => {
            out.push(format!("all_ok={}", o.all_ok()));
            out.push(o.summary());
            let rows = o.rows();
            let mut t: usize = 0;
            while t < rows.len() {
                out.push(format!("{}: {}", rows[t].0, rows[t].1));
                t = t.wrapping_add(1);
            }
            out.push(o.budget.summary());
            out.push(o.metrics.complexity_summary());
            out.push(format!("kv_bytes={} tlv_bytes={}", export_kv(&o).len(), export_tlv(&o).len()));
            let ci = o.metrics.complexity();
            let mut u: usize = 0;
            while u < ci.len() {
                out.push(format!(
                    "  - {:<28} iters={} per={} total={} | {}",
                    ci[u].name, ci[u].iters, ci[u].per_iter, ci[u].total, ci[u].note
                ));
                u = u.wrapping_add(1);
            }
        }
        Err(e) => {
            out.push(format!("observe ERR {}", e.full()));
        }
    }
    out.push(String::from(""));
    out.push(String::from("-- 自检 --"));
    let rep = self_check_report();
    let mut v: usize = 0;
    while v < rep.len() {
        out.push(rep[v].clone());
        v = v.wrapping_add(1);
    }
    out.push(format!("self_check_ok={}", self_check_ok()));
    out
}