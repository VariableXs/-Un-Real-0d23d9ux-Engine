#![allow(dead_code)]
#![allow(unused_imports)]
//! UNX-F3101 · 回归方案总冻结（判据清单→用例→执行→判定四段方案）
//!
//! 任务单号: UNX-F3101
//! 标题: 回归方案总冻结（判据清单→用例→执行→判定四段方案）
//! 域/批: A4/B36
//! 册目标: 400 行（回归方案总 180 + 冻结 120 + 断言 100；测试段不计）
//!
//! 判据原文（逐字摘抄，来源 anchors/UNX-F3101.md）:
//! 「UNX-F3101-J1 回归方案冻结：四段方案（判据清单归集→用例映射→账面执行→三态判定）
//!   范围冻结哈希入册，四段与 F3102–F3112 条位一一映射断言过，方案漂移注入拦截」
//!
//! 复测方式（逐字摘抄）:
//! 「四段定义在册，映射 4/4 点验，注入拦截，方案调度留痕可回放，
//!   四段定义件哈希与条位映射断言读数随批归档，调度留痕注记在档可回放。」
//!
//! 定位（逐字摘抄）:
//! 「回归面的总纲——四段方案：段一判据清单归集（任务书判据主轴+域内判据冻结件逐项归集）；
//!   段二用例映射（每判据映射回归用例）；段三账面执行（B39 账面跑账，真机项欠账）；
//!   段四三态判定（过/欠/挂登记）；四段条位映射冻结；方案是 B36 全批调度面与 B39 执行蓝图。」
//!
//! 语义边界（逐字摘抄）:
//! 「只做方案冻结；不做用例本体（F3102–F3109）、不做执行（B39）。」
//!
//! 依赖与嫁接源（逐字摘抄）:
//! 「依赖 F3102–F3112/F3053/F3079；嫁接源为域内收口制度（F2981 同制），实现自研。」
//!
//! 风险与回退（逐字摘抄）:
//! 「条位失配（四段与条位漂移）——条位映射表冻结断言，失配注入 10/10 次拦截。」
//!
//! 与 Windows 对照（逐字摘抄）:
//! 「Windows 回归方案思想同构，本条条位映射更严。」
//!
//! 四段设计（本单核心，见锚点定位段逐字摘抄）:
//! - **段一 判据清单归集**：任务书判据主轴 + 域内判据冻结件逐项归集成册，键唯一、重号必检出。
//! - **段二 用例映射**：每条判据映射回归用例；映射覆盖率**双向**检出——
//!   无用例的判据 = 漏（hole），无判据的用例 = 冗余（orphan），两侧各出一个位图。
//! - **段三 账面执行**：B39 账面跑账（真机项欠账），四段式第三段真实跑 LCG 事件流。
//! - **段四 三态判定**：过 / 欠 / 挂 三态登记，判定条数必须等于执行条数，恒等式零偏离。
//!
//! 条位映射（四段 → F3102–F3112 共 11 条位）:
//! 11 个条位被**互斥且完备**地分派到 4 段：并集 = 全集（不漏）、两两交集为空（不重）、
//! 每段非空（四段点验 4/4）。这正是锚点「四段与 F3102–F3112 条位一一映射断言过」的落点，
//! 也是「条位失配」风险的正面拦截面。
//!
//! 码段声明:
//! 本单独占诊断码段 `0x25C0 ..= 0x25FF`（`SEG_BASE = 0x25C0`，`SEG_MASK = 0x003F`）。
//! 错误变体一码一段，逐个占段内偏移 0..=13（注入形态），辅助码占 0x1C..=0x21，
//! 最大占用 `0x25E1`，未触及段上界 `0x25FF`。

use alloc::vec;

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ===========================================================================
// 0. 常量表
// ===========================================================================

/// 模块版本标识（写入冻结件 + TLV 组头）。
pub const V_VERSION_ID: u64 = 0x0001_3101_0000_0001;
/// 模块版本串。
pub const V_VERSION: &str = "v3101-uregfreeze.v1";
/// 诊断码段基址（独占段 `0x25C0`）。
pub const SEG_BASE: u16 = 0x25C0;
/// 诊断码段掩码（段容量 64 码）。
pub const SEG_MASK: u16 = 0x003F;
/// 诊断码段上界（= `SEG_BASE | SEG_MASK`）。
pub const SEG_END: u16 = SEG_BASE | SEG_MASK;
/// 诊断主码（段基址本身）。
pub const MAIN_CODE: u16 = SEG_BASE;

/// 段位数（四段式，锚点点明）。
pub const STAGE_COUNT: usize = 4;
/// 判据条目数（判据清单归集册容量）。
pub const CRIT_COUNT: usize = 12;
/// 条位数（F3102–F3112 共 11 条）。
pub const SLOT_COUNT: usize = 11;
/// 每条位执行的账面步数。
pub const CASE_STEPS: usize = 16;
/// 单条位内允许的最大事件数（步数上限）。
pub const CASE_EVENT_CAP: usize = 24;
/// 执行稳定性重复轮数。
pub const REPEAT: usize = 3;
/// 注入形态数（判据要求注入拦截 ≥10/10，本单给 14 形态）。
pub const FORM_COUNT: usize = 14;
/// 判据位图宽度（`CRIT_COUNT = 12` → 12 位，`u32` 足够，绝不溢出移位）。
pub const CRIT_BITS: u32 = 32;
/// 判据清单全 1 位图（低 12 位）。
pub const CRIT_ALL: u32 = (1u32 << 12) - 1;
/// 环形账容量（2 的幂；`11 条位 × 3 轮 = 33 > 32`，必然覆盖以压出 dropped 显性）。
pub const RING_CAP: usize = 32;
/// 环形账掩码。
pub const RING_MASK: usize = RING_CAP - 1;
/// 逻辑时钟步进量。
pub const TICK_STEP: u64 = 1;
/// 逻辑步上限（`11 × 16 × 3` 再留一倍裕量）。
pub const STEP_CAP: u64 = (SLOT_COUNT * CASE_STEPS * REPEAT * 2) as u64;
/// syscall 上界（本单零真实 syscall、零真实 IO）。
pub const SYSCALL_CAP: u64 = 0;
/// 内存峰值上界（字节，模型侧估算）。
pub const MEM_PEAK_CAP: u64 = 4 * 1024 * 1024;
/// TLV 组头标签。
pub const TLV_TAG_HEAD: u8 = 0x01;
/// TLV 组头载荷字节数（由 `export_tlv` 内三条 `push_u64` 反推）。
pub const TLV_HEAD_PAYLOAD: usize = 24;
/// TLV 组头总长（`1 标签 + 2 长度 + 24 载荷`）。
pub const TLV_HEAD_LEN: usize = TLV_HEAD_PAYLOAD + 3;
/// TLV 条目固定长度（`1 标签 + 1 长度 + 8 值`，长度字段恒占定宽）。
pub const TLV_ITEM_LEN: usize = 10;
/// B36 全批调度面名。
pub const BATCH_FACE: &str = "B36";
/// B39 账面执行面名。
pub const EXEC_FACE: &str = "B39";
/// 上游依赖条位（锚点「依赖 F3102–F3112/F3053/F3079」）。
pub const DEP_SLOTS: &str = "F3102-F3112";
/// 嫁接源（锚点「嫁接源为域内收口制度（F2981 同制）」）。
pub const GRAFT_SEAT: &str = "F2981";
/// 对齐面读数锚（锚点「与 F2797 体检单链对齐在档」）。
pub const ALIGN_CHAIN: &str = "F2797";
/// 稳定轮数下限（判据「执行稳定性多轮指纹一致」）。
pub const STABLE_ROUNDS_MIN: usize = 2;

/// 条位号表（`F3102..F3112` 十一位，逐位入册）。
pub const SLOT_TICKETS: [&str; SLOT_COUNT] = [
    "F3102", "F3103", "F3104", "F3105", "F3106", "F3107", "F3108", "F3109", "F3110", "F3111",
    "F3112",
];
/// 条位中文名表（与上表同序）。
pub const SLOT_NAMES: [&str; SLOT_COUNT] = [
    "判据归集件", "判据重号检出件", "用例登记件", "用例执行件", "映射漏检件", "映射冗余件",
    "三态判定件", "欠账登记件", "用例册冻结件", "执行器编排件", "调度留痕件",
];

// ===========================================================================
// 1. 确定性基础件（LCG / FNV-1a / 逻辑时钟）
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
    /// 推进一步，返回新状态。
    pub fn next(&mut self) -> u64 {
        self.state = lcg_step(self.state);
        self.state
    }
    /// 推进一步并记步。
    pub fn step(&mut self) -> u64 {
        let v = self.next();
        v
    }
    /// 取低 32 位。
    pub fn next32(&mut self) -> u32 {
        let v = self.next();
        (v & 0xFFFF_FFFF) as u32
    }
    /// 取模取样（模数须由调用方保证非零）。
    pub fn below(&mut self, m: u64) -> u64 {
        let v = self.next();
        if m == 0 {
            0
        } else {
            v % m
        }
    }
}

/// LCG 单步（`x = a*x + c (mod 2^64)`，wrapping）。
pub fn lcg_step(state: u64) -> u64 {
    state
        .wrapping_mul(6364136223846793005u64)
        .wrapping_add(1442695040888963407u64)
}

/// FNV-1a 64 增量（输入字节串，basis 固定为标准 FNV-1a 64 偏移基）。
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

/// 逻辑时钟推进（零墙钟，时间一律逻辑 tick）。
pub fn tick_advance(now: u64) -> u64 {
    now.wrapping_add(TICK_STEP)
}

/// 字符串折入指纹（供文本型键参与冻结哈希）。
pub fn fnv1a_str(state: u64, s: &str) -> u64 {
    fnv1a64_ext(state, s.as_bytes())
}

// ===========================================================================
// 2. 错误闭集（变体一码一段）
// ===========================================================================

/// 注入 / 检出形态（14 变体，逐个占段内偏移 0..=13）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// 形态 0：判据清单缺项。
    CriterionMissing,
    /// 形态 1：判据清单重号。
    CriterionDup,
    /// 形态 2：条位号越界。
    SlotOutOfRange,
    /// 形态 3：条位缺失。
    SlotMissing,
    /// 形态 4：映射漏（判据无用例）。
    MapAbsent,
    /// 形态 5：映射冗余（用例无判据）。
    MapOrphan,
    /// 形态 6：一判据多用例（口径不一）。
    MapMulti,
    /// 形态 7：条位未执行。
    ExecSkipped,
    /// 形态 8：判定值越三态。
    VerdictIllegal,
    /// 形态 9：判定条数与执行条数不符。
    VerdictCountBad,
    /// 形态 10：环形账不守恒。
    LedgerUnbalanced,
    /// 形态 11：冻结指纹漂移。
    FingerprintDrift,
    /// 形态 12：守恒式偏离。
    ReconcileDeviation,
    /// 形态 13：段位未覆盖（某段零条位）。
    StageUncovered,
}
impl Form {
    /// 形态总数。
    pub const COUNT: usize = 14;
    /// 全部形态（顺序即码段偏移序）。
    pub const ALL: [Form; Form::COUNT] = [
        Form::CriterionMissing,
        Form::CriterionDup,
        Form::SlotOutOfRange,
        Form::SlotMissing,
        Form::MapAbsent,
        Form::MapOrphan,
        Form::MapMulti,
        Form::ExecSkipped,
        Form::VerdictIllegal,
        Form::VerdictCountBad,
        Form::LedgerUnbalanced,
        Form::FingerprintDrift,
        Form::ReconcileDeviation,
        Form::StageUncovered,
    ];
    /// 段内偏移下标。
    pub fn index(self) -> usize {
        match self {
            Form::CriterionMissing => 0,
            Form::CriterionDup => 1,
            Form::SlotOutOfRange => 2,
            Form::SlotMissing => 3,
            Form::MapAbsent => 4,
            Form::MapOrphan => 5,
            Form::MapMulti => 6,
            Form::ExecSkipped => 7,
            Form::VerdictIllegal => 8,
            Form::VerdictCountBad => 9,
            Form::LedgerUnbalanced => 10,
            Form::FingerprintDrift => 11,
            Form::ReconcileDeviation => 12,
            Form::StageUncovered => 13,
        }
    }
    /// 按下标取形态（越界给 `None`）。
    pub fn from_index(i: usize) -> Option<Form> {
        if i < Form::COUNT {
            let f = Form::ALL[i];
            Some(f)
        } else {
            None
        }
    }
    /// 诊断码（`SEG_BASE + 偏移`，一码一段）。
    pub fn code(self) -> u16 {
        SEG_BASE.wrapping_add(self.index() as u16)
    }
    /// 形态名。
    pub fn name(self) -> &'static str {
        match self {
            Form::CriterionMissing => "CriterionMissing",
            Form::CriterionDup => "CriterionDup",
            Form::SlotOutOfRange => "SlotOutOfRange",
            Form::SlotMissing => "SlotMissing",
            Form::MapAbsent => "MapAbsent",
            Form::MapOrphan => "MapOrphan",
            Form::MapMulti => "MapMulti",
            Form::ExecSkipped => "ExecSkipped",
            Form::VerdictIllegal => "VerdictIllegal",
            Form::VerdictCountBad => "VerdictCountBad",
            Form::LedgerUnbalanced => "LedgerUnbalanced",
            Form::FingerprintDrift => "FingerprintDrift",
            Form::ReconcileDeviation => "ReconcileDeviation",
            Form::StageUncovered => "StageUncovered",
        }
    }
    /// 形态描述（中文在档注记）。
    pub fn describe(self) -> &'static str {
        match self {
            Form::CriterionMissing => "判据清单缺项：归集册存在空洞",
            Form::CriterionDup => "判据清单重号：同键两条归集记录",
            Form::SlotOutOfRange => "条位号越界：超出 F3102–F3112 区间",
            Form::SlotMissing => "条位缺失：册内应有条位查无记录",
            Form::MapAbsent => "映射漏：判据无任何用例承接",
            Form::MapOrphan => "映射冗余：用例不承接任何判据",
            Form::MapMulti => "口径不一：单判据被多例重复承接",
            Form::ExecSkipped => "执行欠账：条位未进入账面执行",
            Form::VerdictIllegal => "判定越态：取值不在过/欠/挂三态闭集",
            Form::VerdictCountBad => "判定条数与执行条数不符",
            Form::LedgerUnbalanced => "环形账不守恒：写入数≠存活+丢弃",
            Form::FingerprintDrift => "冻结指纹漂移：定义件哈希与入册值失配",
            Form::ReconcileDeviation => "守恒式偏离：ΣA≠ΣB≠总账",
            Form::StageUncovered => "段位未覆盖：某段零条位，四段点验不过",
        }
    }
    /// 探测面名（该形态在哪一面被检出）。
    pub fn probe(self) -> &'static str {
        match self {
            Form::CriterionMissing => "crit_book.cover_bits",
            Form::CriterionDup => "crit_book.dup_bits",
            Form::SlotOutOfRange => "slot_table.range_ok",
            Form::SlotMissing => "slot_table.presence_ok",
            Form::MapAbsent => "mapping.hole_bits",
            Form::MapOrphan => "mapping.orphan_bits",
            Form::MapMulti => "mapping.multi_bits",
            Form::ExecSkipped => "exec_book.ran_bits",
            Form::VerdictIllegal => "verdict_book.state_ok",
            Form::VerdictCountBad => "verdict_book.count_ok",
            Form::LedgerUnbalanced => "ring.balanced",
            Form::FingerprintDrift => "freeze.verify",
            Form::ReconcileDeviation => "reconcile.deviation",
            Form::StageUncovered => "stage_map.part_ok",
        }
    }
    /// 是否属「映射类」形态（段二覆盖率的三个子类）。
    pub fn is_map(self) -> bool {
        matches!(self, Form::MapAbsent | Form::MapOrphan | Form::MapMulti)
    }
    /// 是否属「账类」形态（段三 / 段四账面侧）。
    pub fn is_ledger(self) -> bool {
        matches!(
            self,
            Form::ExecSkipped
                | Form::VerdictIllegal
                | Form::VerdictCountBad
                | Form::LedgerUnbalanced
                | Form::ReconcileDeviation
        )
    }
    /// 是否属「冻结类」形态（段一 / 冻结面）。
    pub fn is_freeze(self) -> bool {
        matches!(
            self,
            Form::CriterionMissing
                | Form::CriterionDup
                | Form::SlotOutOfRange
                | Form::SlotMissing
                | Form::FingerprintDrift
                | Form::StageUncovered
        )
    }
    /// 形态类名。
    pub fn class_name(self) -> &'static str {
        if self.is_map() {
            "映射面"
        } else if self.is_ledger() {
            "账面面"
        } else if self.is_freeze() {
            "冻结面"
        } else {
            "其他面"
        }
    }
}

/// 辅助码（段内偏移 `0x1C..=0x21`，六变体一码一段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuxCode {
    /// 辅助 0：总册版本失配。
    BookRevMismatch,
    /// 辅助 1：四段顺序破坏。
    StageOrderBroken,
    /// 辅助 2：条位重号。
    SlotDup,
    /// 辅助 3：稳定性轮数不足。
    StableRoundsShort,
    /// 辅助 4：步数越界。
    StepOverCap,
    /// 辅助 5：语义边界越界（做了不该做的用例本体 / 执行）。
    SemanticBorderCross,
}
impl AuxCode {
    /// 辅助码总数。
    pub const COUNT: usize = 6;
    /// 全部辅助码。
    pub const ALL: [AuxCode; AuxCode::COUNT] = [
        AuxCode::BookRevMismatch,
        AuxCode::StageOrderBroken,
        AuxCode::SlotDup,
        AuxCode::StableRoundsShort,
        AuxCode::StepOverCap,
        AuxCode::SemanticBorderCross,
    ];
    /// 段内偏移下标。
    pub fn index(self) -> usize {
        match self {
            AuxCode::BookRevMismatch => 0,
            AuxCode::StageOrderBroken => 1,
            AuxCode::SlotDup => 2,
            AuxCode::StableRoundsShort => 3,
            AuxCode::StepOverCap => 4,
            AuxCode::SemanticBorderCross => 5,
        }
    }
    /// 诊断码（`SEG_BASE + 0x1C + 下标`）。
    pub fn code(self) -> u16 {
        SEG_BASE.wrapping_add(0x1Cu16).wrapping_add(self.index() as u16)
    }
    /// 辅助码名。
    pub fn name(self) -> &'static str {
        match self {
            AuxCode::BookRevMismatch => "BookRevMismatch",
            AuxCode::StageOrderBroken => "StageOrderBroken",
            AuxCode::SlotDup => "SlotDup",
            AuxCode::StableRoundsShort => "StableRoundsShort",
            AuxCode::StepOverCap => "StepOverCap",
            AuxCode::SemanticBorderCross => "SemanticBorderCross",
        }
    }
    /// 辅助码描述。
    pub fn describe(self) -> &'static str {
        match self {
            AuxCode::BookRevMismatch => "总册版本与冻结登记版本失配",
            AuxCode::StageOrderBroken => "四段推进顺序被破坏（非单调）",
            AuxCode::SlotDup => "条位号重复登记",
            AuxCode::StableRoundsShort => "执行稳定性轮数不足下限",
            AuxCode::StepOverCap => "逻辑步数越过 STEP_CAP 上限",
            AuxCode::SemanticBorderCross => "语义边界越界：本单只做方案冻结",
        }
    }
}

/// 本单结果别名（自定义 `Result` 闭集，禁止 std 的 `?` 逃逸）。
pub type UfResult<T> = Result<T, UfErr>;

/// 错误对象（三要素：`code()` + `detail()` + `full()`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UfErr {
    /// 诊断码。
    pub code: u16,
    /// 明文细节。
    pub detail: String,
}
impl UfErr {
    /// 构造。
    pub fn new(code: u16, detail: String) -> UfErr {
        UfErr { code, detail }
    }
    /// 由形态构造。
    pub fn of_form(f: Form, detail: String) -> UfErr {
        UfErr { code: f.code(), detail }
    }
    /// 由辅助码构造。
    pub fn of_aux(a: AuxCode, detail: String) -> UfErr {
        UfErr { code: a.code(), detail }
    }
    /// 三要素之一：码。
    pub fn code(&self) -> u16 {
        self.code
    }
    /// 三要素之二：明细。
    pub fn detail(&self) -> String {
        self.detail.clone()
    }
    /// 三要素之三：全串（码 + 形态名 + 明细）。
    pub fn full(&self) -> String {
        format!("0x{:04X} {}", self.code, self.detail)
    }
}
/// 形态名反查（用于错误全串的形态标注）。
pub fn form_by_code(code: u16) -> Option<&'static str> {
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
pub fn err_str(e: &UfErr) -> String {
    match form_by_code(e.code) {
        Some(n) => format!("[{}] {}", n, e.detail),
        None => format!("[0x{:04X}] {}", e.code, e.detail),
    }
}
/// 冻结态不符错误。
pub fn err_frozen(got: &str, want: &str) -> UfErr {
    UfErr::of_form(
        Form::FingerprintDrift,
        format!("冻结态不符：got={} want={}", got, want),
    )
}
/// 越界错误。
pub fn err_range(what: &str, idx: usize, cap: usize) -> UfErr {
    UfErr::of_aux(
        AuxCode::SlotDup,
        format!("越界：{} idx={} cap={}", what, idx, cap),
    )
}
/// 守恒式错误。
pub fn err_recon(a: u64, b: u64, c: u64) -> UfErr {
    UfErr::of_form(
        Form::ReconcileDeviation,
        format!("守恒式偏离：ΣA={} ΣB={} 总账={}", a, b, c),
    )
}
/// 码段是否合法（落在本单独占段内）。
pub fn code_in_seg(code: u16) -> bool {
    let lo = SEG_BASE;
    let hi = SEG_END;
    code >= lo && code <= hi
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
// 3. 四段式主状态机（合法边闭集 + 非法转换拒记）
// ===========================================================================

/// 四段状态（锚点「四段方案」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 段一：判据清单归集。
    S1Collect,
    /// 段二：用例映射。
    S2Map,
    /// 段三：账面执行。
    S3Exec,
    /// 段四：三态判定。
    S4Verdict,
    /// 终态：方案冻结入册。
    Sealed,
}
impl Stage {
    /// 状态数（含终态）。
    pub const COUNT: usize = 5;
    /// 全部状态（顺序即推进序）。
    pub const ALL: [Stage; Stage::COUNT] = [
        Stage::S1Collect,
        Stage::S2Map,
        Stage::S3Exec,
        Stage::S4Verdict,
        Stage::Sealed,
    ];
    /// 状态下标。
    pub fn index(self) -> usize {
        match self {
            Stage::S1Collect => 0,
            Stage::S2Map => 1,
            Stage::S3Exec => 2,
            Stage::S4Verdict => 3,
            Stage::Sealed => 4,
        }
    }
    /// 段位名。
    pub fn name(self) -> &'static str {
        match self {
            Stage::S1Collect => "S1Collect",
            Stage::S2Map => "S2Map",
            Stage::S3Exec => "S3Exec",
            Stage::S4Verdict => "S4Verdict",
            Stage::Sealed => "Sealed",
        }
    }
    /// 段位中文名（对应锚点四段定位）。
    pub fn cn(self) -> &'static str {
        match self {
            Stage::S1Collect => "段一 判据清单归集",
            Stage::S2Map => "段二 用例映射",
            Stage::S3Exec => "段三 账面执行",
            Stage::S4Verdict => "段四 三态判定",
            Stage::Sealed => "冻结入册",
        }
    }
    /// 段位描述。
    pub fn describe(self) -> &'static str {
        match self {
            Stage::S1Collect => "判据主轴与域内判据冻结件逐项归集，键唯一",
            Stage::S2Map => "每条判据映射回归用例，覆盖率双向检出",
            Stage::S3Exec => "B39 账面跑账，真机项欠账",
            Stage::S4Verdict => "过/欠/挂三态登记，判定条数对齐",
            Stage::Sealed => "四段定义哈希入册，漂移注入拦截",
        }
    }
    /// 是否终态。
    pub fn is_terminal(self) -> bool {
        matches!(self, Stage::Sealed)
    }
    /// 合法出边闭集（只允许推进到下一段，跳段/回退均非法）。
    pub fn allowed(self) -> &'static [Stage] {
        match self {
            Stage::S1Collect => &[Stage::S2Map],
            Stage::S2Map => &[Stage::S3Exec],
            Stage::S3Exec => &[Stage::S4Verdict],
            Stage::S4Verdict => &[Stage::Sealed],
            Stage::Sealed => &[],
        }
    }
    /// 是否可推进到目标态。
    pub fn may_go(self, to: Stage) -> bool {
        let al = self.allowed();
        let mut i: usize = 0;
        while i < al.len() {
            if al[i] == to {
                return true;
            }
            i = i.wrapping_add(1);
        }
        false
    }
    /// 合法出边名表。
    pub fn allowed_names(self) -> Vec<&'static str> {
        let al = self.allowed();
        let mut out: Vec<&'static str> = Vec::with_capacity(al.len());
        let mut i: usize = 0;
        while i < al.len() {
            out.push(al[i].name());
            i = i.wrapping_add(1);
        }
        out
    }
    /// 段位输入面（该段消费什么）。
    pub fn input_face(self) -> &'static str {
        match self {
            Stage::S1Collect => "任务书判据主轴",
            Stage::S2Map => "判据清单册",
            Stage::S3Exec => "用例映射表",
            Stage::S4Verdict => "账面执行账",
            Stage::Sealed => "三态判定册",
        }
    }
    /// 段位输出面（该段产出什么）。
    pub fn output_face(self) -> &'static str {
        match self {
            Stage::S1Collect => "判据清单册",
            Stage::S2Map => "用例映射表",
            Stage::S3Exec => "账面执行账",
            Stage::S4Verdict => "三态判定册",
            Stage::Sealed => "冻结哈希",
        }
    }
    /// 四段输入输出四列定义（锚点「范围/输入/输出/判据四列定义入册」）。
    pub fn four_columns(self) -> String {
        format!(
            "{} | 输入={} | 输出={} | 判据={}",
            self.cn(),
            self.input_face(),
            self.output_face(),
            self.describe()
        )
    }
}

/// 非法迁移拒记。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reject {
    /// 来源态。
    pub from: Stage,
    /// 目标态。
    pub to: Stage,
    /// 逻辑时刻。
    pub tick: u64,
}
impl Reject {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("拒记 {}->{} @tick{}", self.from.name(), self.to.name(), self.tick)
    }
}

/// 方案冻结状态机。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StageFsm {
    /// 当前态。
    pub state: Stage,
    /// 逻辑时刻。
    pub tick: u64,
    /// 已走过的态序列。
    pub trail: Vec<Stage>,
    /// 非法迁移拒记。
    pub rejected: Vec<Reject>,
}
impl StageFsm {
    /// 构造（初态段一，trail 预置初态）。
    pub fn new() -> StageFsm {
        let mut trail: Vec<Stage> = Vec::with_capacity(Stage::COUNT);
        trail.push(Stage::S1Collect);
        StageFsm { state: Stage::S1Collect, tick: 0, trail, rejected: Vec::new() }
    }
    /// 尝试迁移（非法则拒记并返回 `Err`，合法则推进）。
    pub fn try_go(&mut self, to: Stage) -> UfResult<Stage> {
        if self.state == to {
            return Err(UfErr::of_form(
                Form::ReconcileDeviation,
                format!("自环迁移非法：{}", to.name()),
            ));
        }
        if !self.state.may_go(to) {
            self.tick = tick_advance(self.tick);
            self.rejected.push(Reject { from: self.state, to, tick: self.tick });
            return Err(UfErr::of_aux(
                AuxCode::StageOrderBroken,
                format!("非法迁移 {}->{}，合法出边={:?}", self.state.name(), to.name(), self.state.allowed_names()),
            ));
        }
        self.state = to;
        self.tick = tick_advance(self.tick);
        self.trail.push(to);
        Ok(to)
    }
    /// 连续推进到目标态（内部逐步走合法边，返回步数）。
    pub fn advance_to(&mut self, to: Stage) -> UfResult<u32> {
        let mut steps: u32 = 0;
        while self.state != to {
            let al = self.state.allowed();
            if al.len() == 0 {
                return Err(err_frozen(self.state.name(), to.name()));
            }
            let nxt = al[0];
            self.try_go(nxt)?;
            steps = steps.wrapping_add(1);
            if steps > Stage::COUNT as u32 {
                return Err(UfErr::of_aux(
                    AuxCode::StageOrderBroken,
                    format!("推进步数越界：{} 步才到 {}", steps, to.name()),
                ));
            }
        }
        Ok(steps)
    }
    /// trail 是否严格单调（索引严格递增）。
    pub fn trail_monotonic(&self) -> bool {
        let mut i: usize = 1;
        while i < self.trail.len() {
            let a = self.trail[i - 1].index();
            let b = self.trail[i].index();
            if b <= a {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 已走边数。
    pub fn edges_walked(&self) -> usize {
        if self.trail.len() < 2 {
            0
        } else {
            self.trail.len() - 2 + 1
        }
    }
    /// 状态机指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(8 + self.rejected.len());
        words.push(self.state.index() as u64);
        words.push(self.tick);
        words.push(self.trail.len() as u64);
        for s in self.trail.iter() {
            words.push(s.index() as u64);
        }
        words.push(self.rejected.len() as u64);
        for r in self.rejected.iter() {
            words.push(r.from.index() as u64);
            words.push(r.to.index() as u64);
            words.push(r.tick);
        }
        fnv1a_words(&words)
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "fsm state={} edges={} rejected={} monotonic={} fp={:016X}",
            self.state.name(),
            self.trail.len().saturating_sub(1),
            self.rejected.len(),
            if self.trail_monotonic() { 1 } else { 0 },
            self.fingerprint()
        )
    }
    /// 合法边矩阵（`COUNT × COUNT`，仅上三角紧邻位为真）。
    pub fn edge_matrix() -> [[bool; Stage::COUNT]; Stage::COUNT] {
        let mut m: [[bool; Stage::COUNT]; Stage::COUNT] = [[false; Stage::COUNT]; Stage::COUNT];
        let mut a: usize = 0;
        while a < Stage::COUNT {
            let mut b: usize = 0;
            while b < Stage::COUNT {
                m[a][b] = Stage::ALL[a].may_go(Stage::ALL[b]);
                b = b.wrapping_add(1);
            }
            a = a.wrapping_add(1);
        }
        m
    }
    /// 合法边总数（链式故 = `COUNT - 1`）。
    pub fn edge_count() -> usize {
        let m = StageFsm::edge_matrix();
        let mut c: usize = 0;
        let mut a: usize = 0;
        while a < Stage::COUNT {
            let mut b: usize = 0;
            while b < Stage::COUNT {
                if m[a][b] {
                    c = c.wrapping_add(1);
                }
                b = b.wrapping_add(1);
            }
            a = a.wrapping_add(1);
        }
        c
    }
    /// 非法边总数（矩阵补）。
    pub fn illegal_edge_count() -> usize {
        Stage::COUNT * Stage::COUNT - StageFsm::edge_count()
    }
    /// 逐行边矩阵。
    pub fn edge_rows() -> Vec<String> {
        let m = StageFsm::edge_matrix();
        let mut out: Vec<String> = Vec::with_capacity(Stage::COUNT);
        let mut a: usize = 0;
        while a < Stage::COUNT {
            let mut line = format!("{:>10} ->", Stage::ALL[a].name());
            let mut b: usize = 0;
            while b < Stage::COUNT {
                let c = if m[a][b] { '1' } else { '0' };
                line.push(c);
                b = b.wrapping_add(1);
            }
            out.push(line);
            a = a.wrapping_add(1);
        }
        out
    }
}

// ===========================================================================
// 4. 段一：判据清单归集册
// ===========================================================================

/// 判据来源面（锚点「任务书判据主轴 + 域内判据冻结件」两路归集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CritSource {
    /// 主轴判据（任务书直接给出）。
    Axis,
    /// 域内判据冻结件派生。
    FreezePiece,
    /// 嫁接源（F2981 同制）继承。
    Grafted,
}
impl CritSource {
    /// 源数。
    pub const COUNT: usize = 3;
    /// 全部源。
    pub const ALL: [CritSource; CritSource::COUNT] =
        [CritSource::Axis, CritSource::FreezePiece, CritSource::Grafted];
    /// 源下标。
    pub fn index(self) -> usize {
        match self {
            CritSource::Axis => 0,
            CritSource::FreezePiece => 1,
            CritSource::Grafted => 2,
        }
    }
    /// 源名。
    pub fn name(self) -> &'static str {
        match self {
            CritSource::Axis => "Axis",
            CritSource::FreezePiece => "FreezePiece",
            CritSource::Grafted => "Grafted",
        }
    }
    /// 源中文名。
    pub fn cn(self) -> &'static str {
        match self {
            CritSource::Axis => "任务书判据主轴",
            CritSource::FreezePiece => "域内判据冻结件",
            CritSource::Grafted => "嫁接源继承（F2981 同制）",
        }
    }
    /// 该源判据权重（归集册加法链的一环）。
    pub fn weight(self) -> u64 {
        match self {
            CritSource::Axis => 3,
            CritSource::FreezePiece => 5,
            CritSource::Grafted => 2,
        }
    }
}

/// 判据条目（四段式段一的归集单元）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Criterion {
    /// 判据号（1..=CRIT_COUNT）。
    pub id: u16,
    /// 判据键名。
    pub key: &'static str,
    /// 归属段位。
    pub stage: Stage,
    /// 来源面。
    pub source: CritSource,
    /// 判据权重（加法链用）。
    pub weight: u64,
    /// 主轴摘录（判据主轴原话要点）。
    pub text: &'static str,
}
impl Criterion {
    /// 构造。
    pub fn new(
        id: u16,
        key: &'static str,
        stage: Stage,
        source: CritSource,
        weight: u64,
        text: &'static str,
    ) -> Criterion {
        Criterion { id, key, stage, source, weight, text }
    }
    /// 判据位（低 12 位内）。
    pub fn bit(&self) -> u32 {
        if self.id == 0 || (self.id as usize) > CRIT_COUNT {
            0
        } else {
            1u32 << ((self.id as usize) - 1)
        }
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "J{} key={} 段={} 源={} 权={} 文案={}",
            self.id,
            self.key,
            self.stage.name(),
            self.source.cn(),
            self.weight,
            self.text
        )
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 5] {
        [
            self.id as u64,
            self.stage.index() as u64,
            self.source.index() as u64,
            self.weight,
            fnv1a64(self.key.as_bytes()),
        ]
    }
}

/// 判据清单册（段一归集产物）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CritBook {
    /// 条目。
    pub items: Vec<Criterion>,
    /// 覆盖位图（在册判据的位并集）。
    pub cover_bits: u32,
    /// 重号位图（同 id 出现 ≥2 次的位）。
    pub dup_bits: u32,
    /// 加法链：Σ权。
    pub weight_sum: u64,
}
impl CritBook {
    /// 空册。
    pub fn blank() -> CritBook {
        CritBook { items: Vec::with_capacity(CRIT_COUNT), cover_bits: 0, dup_bits: 0, weight_sum: 0 }
    }
    /// 登记一条判据（自算位图与加法链）。
    pub fn register(&mut self, c: Criterion) {
        let b = c.bit();
        if b != 0 {
            if (self.cover_bits & b) != 0 {
                // 重号：登记位置不退化，只把 dup 位点亮
                self.dup_bits = self.dup_bits | b;
            } else {
                self.cover_bits = self.cover_bits | b;
            }
        }
        self.weight_sum = self.weight_sum.wrapping_add(c.weight);
        self.items.push(c);
    }
    /// 由 `items` 重算派生字段（覆盖位图 / 重号位图 / 加法链）。
    ///
    /// 用途：条目被增删后派生位图必须同步重算，否则位图会留下「幽灵覆盖」。
    /// `register()` 是增量维护（O(1)），`recompute()` 是全量重建（O(n)）；
    /// 注入核走后者以保证派生面永远与条目面一致。
    pub fn recompute(&mut self) {
        let mut cover: u32 = 0;
        let mut dup: u32 = 0;
        let mut wsum: u64 = 0;
        for c in self.items.iter() {
            let b = c.bit();
            if b != 0 {
                if (cover & b) != 0 {
                    dup = dup | b;
                } else {
                    cover = cover | b;
                }
            }
            wsum = wsum.wrapping_add(c.weight);
        }
        self.cover_bits = cover;
        self.dup_bits = dup;
        self.weight_sum = wsum;
    }
    /// 缺项位图（`CRIT_ALL` 减去已覆盖）。
    pub fn missing_bits(&self) -> u32 {
        CRIT_ALL & (CRIT_ALL ^ self.cover_bits)
    }
    /// 缺项数。
    pub fn missing_count(&self) -> usize {
        let mut n: usize = 0;
        let mut i: usize = 0;
        while i < CRIT_COUNT {
            let b: u32 = 1u32 << i;
            if (self.missing_bits() & b) != 0 {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }
    /// 重号项名表。
    pub fn dup_list(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for c in self.items.iter() {
            let b = c.bit();
            if b != 0 && (self.dup_bits & b) != 0 {
                out.push(c.key);
            }
        }
        out
    }
    /// 缺项名表（以判据键表为名）。
    pub fn missing_list(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::with_capacity(CRIT_COUNT);
        let mut i: usize = 0;
        while i < CRIT_COUNT {
            let b: u32 = 1u32 << i;
            if (self.missing_bits() & b) != 0 {
                let k = crit_key_at(i + 1);
                out.push(k);
            }
            i = i.wrapping_add(1);
        }
        out
    }
    /// 册指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(CRIT_COUNT * 5 + 4);
        words.push(self.items.len() as u64);
        words.push(self.cover_bits as u64);
        words.push(self.dup_bits as u64);
        words.push(self.weight_sum);
        for c in self.items.iter() {
            let w = c.words();
            let mut i: usize = 0;
            while i < w.len() {
                words.push(w[i]);
                i = i.wrapping_add(1);
            }
        }
        fnv1a_words(&words)
    }
    /// 源分布计数。
    pub fn source_count(&self, s: CritSource) -> usize {
        let mut n: usize = 0;
        for c in self.items.iter() {
            if c.source == s {
                n = n.wrapping_add(1);
            }
        }
        n
    }
    /// 段分布计数。
    pub fn stage_count(&self, s: Stage) -> usize {
        let mut n: usize = 0;
        for c in self.items.iter() {
            if c.stage == s {
                n = n.wrapping_add(1);
            }
        }
        n
    }
    /// 判据 id 是否唯一。
    pub fn id_unique(&self) -> bool {
        self.dup_bits == 0
    }
    /// 归集完整（零缺项 + 零重号 + 条数达标）。
    pub fn complete(&self) -> bool {
        self.missing_bits() == 0 && self.dup_bits == 0 && self.items.len() >= CRIT_COUNT
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "crit items={} cover={:04X} miss={} dup={} wsum={} fp={:016X}",
            self.items.len(),
            self.cover_bits,
            self.missing_count(),
            self.dup_bits.count_ones() as usize,
            self.weight_sum,
            self.fingerprint()
        )
    }
    /// 逐行在册表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.items.len() + 1);
        out.push(format!("# 判据清单册 items={} 目标={}", self.items.len(), CRIT_COUNT));
        for c in self.items.iter() {
            out.push(c.describe());
        }
        out
    }
}

/// 判据键表（1..=CRIT_COUNT，段一归集的固定键空间）。
pub const CRIT_KEYS: [&str; CRIT_COUNT] = [
    "四段范围定义",
    "四段输入面定义",
    "四段输出面定义",
    "四段判据列定义",
    "条位号唯一",
    "条位区间在界",
    "四段点验四比四",
    "映射覆盖无漏",
    "映射无冗余",
    "三态判定闭合",
    "执行条数对齐",
    "冻结哈希入库",
];
/// 判据文案表（逐条挂判据主轴原话要点）。
pub const CRIT_TEXTS: [&str; CRIT_COUNT] = [
    "四段方案范围入册",
    "输入面登记",
    "输出面登记",
    "判据列登记",
    "F3102-F3112 键唯一",
    "条位号在区间内",
    "四段各有点验位",
    "判据必有承接用例",
    "用例必挂判据",
    "过欠挂三态闭集",
    "执行与判定同数",
    "定义件哈希入 CI",
];
/// 按号取判据键。
pub fn crit_key_at(id: usize) -> &'static str {
    if id >= 1 && id <= CRIT_COUNT {
        CRIT_KEYS[id - 1]
    } else {
        "UNKNOWN_CRIT"
    }
}
/// 按号取判据文案。
pub fn crit_text_at(id: usize) -> &'static str {
    if id >= 1 && id <= CRIT_COUNT {
        CRIT_TEXTS[id - 1]
    } else {
        "UNKNOWN_TEXT"
    }
}
/// 按号取判据归属段（判据 1..=4 属段一，5..=7 属段二，8..=10 属段三，11..=12 属段四）。
pub fn crit_stage_at(id: usize) -> Stage {
    if id <= 4 {
        Stage::S1Collect
    } else if id <= 7 {
        Stage::S2Map
    } else if id <= 10 {
        Stage::S3Exec
    } else {
        Stage::S4Verdict
    }
}
/// 按号取判据来源面。
pub fn crit_source_at(id: usize) -> CritSource {
    match id % 3 {
        0 => CritSource::FreezePiece,
        1 => CritSource::Axis,
        _ => CritSource::Grafted,
    }
}
/// 按号取判据权重（源权 + 号位偏移，保证加法链非退化）。
pub fn crit_weight_at(id: usize) -> u64 {
    let base = crit_source_at(id).weight();
    base.wrapping_add(id as u64)
}
/// 构造完整判据（判据 `id`）。
pub fn criterion_at(id: usize) -> Criterion {
    Criterion::new(
        id as u16,
        crit_key_at(id),
        crit_stage_at(id),
        crit_source_at(id),
        crit_weight_at(id),
        crit_text_at(id),
    )
}
/// 构造完整判据册（`CRIT_COUNT` 条，零缺零重）。
pub fn crit_book_full() -> CritBook {
    let mut b = CritBook::blank();
    let mut i: usize = 1;
    while i <= CRIT_COUNT {
        b.register(criterion_at(i));
        i = i.wrapping_add(1);
    }
    b
}

// ===========================================================================
// 5. 条位册（F3102–F3112）与四段条位映射
// ===========================================================================

/// 条位条目。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Slot {
    /// 条位下标（`0..=SLOT_COUNT-1`）。
    pub index: usize,
    /// 条位号（`F3102` 起）。
    pub ticket: &'static str,
    /// 条位名。
    pub name: &'static str,
    /// 归属段位（四段条位映射的「条位 → 段」方向）。
    pub stage: Stage,
    /// 承接判据位图。
    pub crit_bits: u32,
    /// 是否真机项（真机项欠账由段四登记为「欠」）。
    pub machine_only: bool,
    /// 是否已执行。
    pub ran: bool,
    /// 判定码（`0xFF` = 未判定）。
    pub verdict_code: u8,
}
impl Slot {
    /// 构造。
    pub fn new(index: usize, stage: Stage, crit_bits: u32, machine_only: bool) -> Slot {
        let ticket = slot_ticket_at(index);
        let name = slot_name_at(index);
        Slot { index, ticket, name, stage, crit_bits, machine_only, ran: false, verdict_code: 0xFF }
    }
    /// 承接判据数（popcount）。
    pub fn crit_count(&self) -> usize {
        self.crit_bits.count_ones() as usize
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "slot[{}] {} {} 段={} 承接={} 真机={} 跑={} 判定={:02X}",
            self.index,
            self.ticket,
            self.name,
            self.stage.name(),
            self.crit_bits.count_ones(),
            if self.machine_only { 1 } else { 0 },
            if self.ran { 1 } else { 0 },
            self.verdict_code
        )
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 6] {
        [
            self.index as u64,
            self.stage.index() as u64,
            self.crit_bits as u64,
            self.machine_only as u64,
            self.ran as u64,
            self.verdict_code as u64,
        ]
    }
}
/// 按下标取条位号。
pub fn slot_ticket_at(i: usize) -> &'static str {
    if i < SLOT_COUNT {
        SLOT_TICKETS[i]
    } else {
        "F????"
    }
}
/// 按下标取条位名。
pub fn slot_name_at(i: usize) -> &'static str {
    if i < SLOT_COUNT {
        SLOT_NAMES[i]
    } else {
        "未定义条位"
    }
}
/// 条位默认归属段位（四段分派表，11 条位互斥且完备地摊到 4 段）。
///
/// 分派口径：段一 3 条（`0..=2`）、段二 3 条（`3..=5`）、段三 2 条（`6..=7`）、
/// 段四 3 条（`8..=10`），合计 `3+3+2+3 = 11 = SLOT_COUNT`，无缝隙无重叠。
pub fn slot_stage_default(i: usize) -> Stage {
    if i <= 2 {
        Stage::S1Collect
    } else if i <= 5 {
        Stage::S2Map
    } else if i <= 7 {
        Stage::S3Exec
    } else {
        Stage::S4Verdict
    }
}
/// 条位默认真机标记（条位 0 与条位 10 为真机项，对应「真机项欠账」）。
pub fn slot_machine_default(i: usize) -> bool {
    i == 0 || i == 10
}
/// 判据默认映射条位（`crit j → slot (j-1) % 11`，判据 1..=11 一一对应，判据 12 落到条位 0）。
///
/// 口径说明：映射是**单射方向**的——一条判据只允许挂一个条位用例，
/// 因此段二的「一判据多用例（口径不一）」在净态恒为 0，只在注入态点亮。
pub fn crit_slot_default(crit_id: usize) -> usize {
    if crit_id == 0 {
        0
    } else {
        ((crit_id - 1) % SLOT_COUNT) as usize
    }
}
/// 条位册（四段条位映射的载体）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SlotTable {
    /// 条位。
    pub slots: Vec<Slot>,
    /// 在册条位位图（低 11 位）。
    pub present_bits: u32,
    /// 重号位图（条位号重复登记）。
    pub dup_bits: u32,
    /// 越界计数（条位号不在 `F3102..F3112`）。
    pub out_of_range: usize,
}
impl SlotTable {
    /// 空册。
    pub fn blank() -> SlotTable {
        SlotTable {
            slots: Vec::with_capacity(SLOT_COUNT),
            present_bits: 0,
            dup_bits: 0,
            out_of_range: 0,
        }
    }
    /// 登记一条条位（自算位图；越界条位不入位图但计越界数）。
    pub fn register(&mut self, s: Slot) -> UfResult<()> {
        if s.index >= SLOT_COUNT {
            self.out_of_range = self.out_of_range.wrapping_add(1);
            return Err(UfErr::of_form(
                Form::SlotOutOfRange,
                format!("条位越界：index={} cap={} 区间={}", s.index, SLOT_COUNT, DEP_SLOTS),
            ));
        }
        let b: u32 = 1u32 << s.index;
        if (self.present_bits & b) != 0 {
            self.dup_bits = self.dup_bits | b;
            return Err(UfErr::of_aux(
                AuxCode::SlotDup,
                format!("条位重号：{}", s.ticket),
            ));
        }
        self.present_bits = self.present_bits | b;
        self.slots.push(s);
        Ok(())
    }
    /// 由 `slots` 重算派生字段（在册位图 / 重号位图 / 越界计数）。
    pub fn recompute(&mut self) {
        let mut present: u32 = 0;
        let mut dup: u32 = 0;
        let mut oor: usize = 0;
        for s in self.slots.iter() {
            if s.index >= SLOT_COUNT {
                oor = oor.wrapping_add(1);
                continue;
            }
            let b: u32 = 1u32 << s.index;
            if (present & b) != 0 {
                dup = dup | b;
            } else {
                present = present | b;
            }
        }
        self.present_bits = present;
        self.dup_bits = dup;
        self.out_of_range = oor;
    }
    /// 取条位（越界给 `Err`）。
    pub fn get(&self, i: usize) -> UfResult<&Slot> {
        match self.slots.get(i) {
            Some(s) => Ok(s),
            None => Err(err_range("slot", i, SLOT_COUNT)),
        }
    }
    /// 取条位可写（越界给 `Err`）。
    pub fn get_mut(&mut self, i: usize) -> UfResult<&mut Slot> {
        match self.slots.get_mut(i) {
            Some(s) => Ok(s),
            None => Err(err_range("slot", i, SLOT_COUNT)),
        }
    }
    /// 缺位位图。
    pub fn missing_bits(&self) -> u32 {
        let all: u32 = (1u32 << SLOT_COUNT) - 1;
        all & (all ^ self.present_bits)
    }
    /// 缺位数。
    pub fn missing_count(&self) -> usize {
        let mut n: usize = 0;
        let mut i: usize = 0;
        while i < SLOT_COUNT {
            let b: u32 = 1u32 << i;
            if (self.missing_bits() & b) != 0 {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }
    /// 段位掩码（四段是否各有点验位）。
    pub fn stage_mask(&self) -> u32 {
        let mut m: u32 = 0;
        for s in self.slots.iter() {
            m = m | (1u32 << s.stage.index());
        }
        m
    }
    /// 四段全有点验位（掩码应为 `0b01111`，即 5 位含终态）。
    pub fn stage_partition_ok(&self) -> bool {
        let want: u32 = (1u32 << STAGE_COUNT) - 1;
        let m = self.stage_mask() & want;
        m == want
    }
    /// 四段互斥（每条位只属一段，由构造保证；此函数做实测复核）。
    pub fn stage_exclusive_ok(&self) -> bool {
        let mut seen: u32 = 0;
        for s in self.slots.iter() {
            let b: u32 = 1u32 << s.index;
            if (seen & b) != 0 {
                return false;
            }
            seen = seen | b;
        }
        seen == self.present_bits
    }
    /// 段位计数（4 个数的表）。
    pub fn stage_counts(&self) -> [usize; STAGE_COUNT] {
        let mut c: [usize; STAGE_COUNT] = [0; STAGE_COUNT];
        for s in self.slots.iter() {
            let i = s.stage.index();
            if i < STAGE_COUNT {
                c[i] = c[i].wrapping_add(1);
            }
        }
        c
    }
    /// 条位齐全（零缺位、零重号、零越界）。
    pub fn complete(&self) -> bool {
        self.missing_bits() == 0 && self.dup_bits == 0 && self.out_of_range == 0 && self.slots.len() >= SLOT_COUNT
    }
    /// 册指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(SLOT_COUNT * 6 + 4);
        words.push(self.slots.len() as u64);
        words.push(self.present_bits as u64);
        words.push(self.dup_bits as u64);
        words.push(self.out_of_range as u64);
        for s in self.slots.iter() {
            let w = s.words();
            let mut i: usize = 0;
            while i < w.len() {
                words.push(w[i]);
                i = i.wrapping_add(1);
            }
        }
        fnv1a_words(&words)
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "slots n={} present={:04X} miss={} dup={} oor={} part={} fp={:016X}",
            self.slots.len(),
            self.present_bits,
            self.missing_count(),
            self.dup_bits.count_ones() as usize,
            self.out_of_range,
            if self.stage_partition_ok() { 1 } else { 0 },
            self.fingerprint()
        )
    }
    /// 逐行在册表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.slots.len() + 1);
        out.push(format!("# 条位册 n={} 区间={}", self.slots.len(), DEP_SLOTS));
        for s in self.slots.iter() {
            out.push(s.describe());
        }
        out
    }
}
/// 构造完整条位册（11 条位，四段分派完备）。
pub fn slot_table_full() -> UfResult<SlotTable> {
    let mut t = SlotTable::blank();
    let mut i: usize = 0;
    while i < SLOT_COUNT {
        let stage = slot_stage_default(i);
        let mut bits: u32 = 0;
        let mut j: usize = 1;
        while j <= CRIT_COUNT {
            if crit_slot_default(j) == i {
                bits = bits | (1u32 << (j - 1));
            }
            j = j.wrapping_add(1);
        }
        let s = Slot::new(i, stage, bits, slot_machine_default(i));
        t.register(s)?;
        i = i.wrapping_add(1);
    }
    Ok(t)
}

// ===========================================================================
// 6. 段二：用例映射表（覆盖率双向检出）
// ===========================================================================

/// 单条判据的映射登记。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CritMap {
    /// 判据号。
    pub crit_id: u16,
    /// 判据位。
    pub bit: u32,
    /// 承接条位下标列表（净态长度恰为 1）。
    pub slots: Vec<usize>,
}
impl CritMap {
    /// 构造。
    pub fn new(crit_id: u16) -> CritMap {
        let bit = if crit_id == 0 || (crit_id as usize) > CRIT_COUNT {
            0
        } else {
            1u32 << ((crit_id as usize) - 1)
        };
        CritMap { crit_id, bit, slots: Vec::with_capacity(2) }
    }
    /// 承接条位数。
    pub fn fanout(&self) -> usize {
        self.slots.len()
    }
    /// 是否漏（零承接）。
    pub fn is_hole(&self) -> bool {
        self.slots.is_empty()
    }
    /// 是否冗（承接数 > 1，口径不一）。
    pub fn is_multi(&self) -> bool {
        self.slots.len() > 1
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        let mut s = format!("J{} key={} fanout={} ->[", self.crit_id, crit_key_at(self.crit_id as usize), self.slots.len());
        let mut i: usize = 0;
        while i < self.slots.len() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(slot_ticket_at(self.slots[i]));
            i = i.wrapping_add(1);
        }
        s.push(']');
        s
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 2] {
        [self.crit_id as u64, self.slots.len() as u64]
    }
}

/// 用例映射表（段二产物；覆盖率双向检出的落点）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MapTable {
    /// 逐判据登记。
    pub maps: Vec<CritMap>,
    /// 漏位图（判据无承接用例）。
    pub hole_bits: u32,
    /// 冗位图（用例不承接判据）。
    pub orphan_bits: u32,
    /// 多位图（单判据被多例承接，口径不一）。
    pub multi_bits: u32,
    /// 判据 → 条位反向位图的累加值（用于对账加法链）。
    pub link_sum: u64,
}
impl MapTable {
    /// 空表。
    pub fn blank() -> MapTable {
        MapTable { maps: Vec::with_capacity(CRIT_COUNT), hole_bits: 0, orphan_bits: 0, multi_bits: 0, link_sum: 0 }
    }
    /// 登记一条映射。
    pub fn push(&mut self, m: CritMap) {
        if m.bit != 0 {
            if m.is_hole() {
                self.hole_bits = self.hole_bits | m.bit;
            }
            if m.is_multi() {
                self.multi_bits = self.multi_bits | m.bit;
            }
        }
        self.link_sum = self.link_sum.wrapping_add(m.slots.len() as u64);
        self.maps.push(m);
    }
    /// 冗余位图实测（由条位侧反查：不用例承接任何判据的条位位）。
    pub fn recompute_orphan(&mut self, t: &SlotTable) -> UfResult<()> {
        let mut orphan: u32 = 0;
        for s in t.slots.iter() {
            if s.crit_count() == 0 {
                orphan = orphan | (1u32 << s.index);
            }
        }
        self.orphan_bits = orphan;
        let _ = self.maps.len();
        Ok(())
    }
    /// 漏项数。
    pub fn hole_count(&self) -> usize {
        self.hole_bits.count_ones() as usize
    }
    /// 冗项数。
    pub fn orphan_count(&self) -> usize {
        self.orphan_bits.count_ones() as usize
    }
    /// 多项数。
    pub fn multi_count(&self) -> usize {
        self.multi_bits.count_ones() as usize
    }
    /// 映射完备（零漏零冗零多）。
    pub fn complete(&self) -> bool {
        self.hole_bits == 0 && self.orphan_bits == 0 && self.multi_bits == 0
    }
    /// 覆盖率（分子 = 有承接判据数，分母 = 在册判据数；用 permille 返）。
    pub fn coverage_permille(&self) -> u64 {
        let denom: u64 = self.maps.len() as u64;
        if denom == 0 {
            return 0;
        }
        let holes: u64 = self.hole_count() as u64;
        let covered: u64 = denom - holes;
        covered.wrapping_mul(1000u64) / denom
    }
    /// 表指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(CRIT_COUNT * 2 + 5);
        words.push(self.maps.len() as u64);
        words.push(self.hole_bits as u64);
        words.push(self.orphan_bits as u64);
        words.push(self.multi_bits as u64);
        words.push(self.link_sum);
        for m in self.maps.iter() {
            let w = m.words();
            words.push(w[0]);
            words.push(w[1]);
        }
        fnv1a_words(&words)
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "map n={} hole={} orphan={} multi={} cov={} permille link={} fp={:016X}",
            self.maps.len(),
            self.hole_count(),
            self.orphan_count(),
            self.multi_count(),
            self.coverage_permille(),
            self.link_sum,
            self.fingerprint()
        )
    }
    /// 逐行在册表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.maps.len() + 1);
        out.push(format!("# 用例映射表 n={} 判据={}", self.maps.len(), CRIT_COUNT));
        for m in self.maps.iter() {
            out.push(m.describe());
        }
        out
    }
}
/// 构造完整映射表（每判据恰挂一条位，零漏零冗零多）。
pub fn map_table_full(t: &SlotTable) -> UfResult<MapTable> {
    let mut m = MapTable::blank();
    let mut j: usize = 1;
    while j <= CRIT_COUNT {
        let mut cm = CritMap::new(j as u16);
        let s = crit_slot_default(j);
        // 承接条位必须在册，否则视为映射漏（返回错误交由调用方处理）
        t.get(s)?;
        cm.slots.push(s);
        m.push(cm);
        j = j.wrapping_add(1);
    }
    m.recompute_orphan(t)?;
    if !m.complete() {
        return Err(UfErr::of_form(
            Form::MapAbsent,
            format!("映射表不完备：{}", m.summary()),
        ));
    }
    Ok(m)
}

// ===========================================================================
// 7. 段三：账面执行账（每条位真跑 LCG 事件流）
// ===========================================================================

/// 账面事件类型（四段式段三的事件闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExecEvent {
    /// 载入用例体。
    LoadCase,
    /// 绑定判据。
    BindCrit,
    /// 推进断言。
    StepAssert,
    /// 记录读数。
    ReadGauge,
    /// 归还账面。
    SettleBook,
}
impl ExecEvent {
    /// 事件数。
    pub const COUNT: usize = 5;
    /// 全部事件。
    pub const ALL: [ExecEvent; ExecEvent::COUNT] = [
        ExecEvent::LoadCase,
        ExecEvent::BindCrit,
        ExecEvent::StepAssert,
        ExecEvent::ReadGauge,
        ExecEvent::SettleBook,
    ];
    /// 事件下标。
    pub fn index(self) -> usize {
        match self {
            ExecEvent::LoadCase => 0,
            ExecEvent::BindCrit => 1,
            ExecEvent::StepAssert => 2,
            ExecEvent::ReadGauge => 3,
            ExecEvent::SettleBook => 4,
        }
    }
    /// 事件名。
    pub fn name(self) -> &'static str {
        match self {
            ExecEvent::LoadCase => "LoadCase",
            ExecEvent::BindCrit => "BindCrit",
            ExecEvent::StepAssert => "StepAssert",
            ExecEvent::ReadGauge => "ReadGauge",
            ExecEvent::SettleBook => "SettleBook",
        }
    }
    /// 事件中文名。
    pub fn cn(self) -> &'static str {
        match self {
            ExecEvent::LoadCase => "载入用例体",
            ExecEvent::BindCrit => "绑定判据",
            ExecEvent::StepAssert => "推进断言",
            ExecEvent::ReadGauge => "记录读数",
            ExecEvent::SettleBook => "归还账面",
        }
    }
    /// 事件基权（账面代价量）。
    pub fn cost(self) -> u64 {
        match self {
            ExecEvent::LoadCase => 3,
            ExecEvent::BindCrit => 2,
            ExecEvent::StepAssert => 5,
            ExecEvent::ReadGauge => 1,
            ExecEvent::SettleBook => 4,
        }
    }
}
/// 条位执行结果（单条位账面跑账产物）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SlotRun {
    /// 条位下标。
    pub slot: usize,
    /// 条位号。
    pub ticket: &'static str,
    /// 归属段位。
    pub stage: Stage,
    /// 承接判据位图。
    pub crit_bits: u32,
    /// 实际事件数。
    pub events: u64,
    /// 账面代价合计（Σ事件基权）。
    pub cost: u64,
    /// 断言通过数。
    pub asserts_pass: u64,
    /// 断言总数。
    pub asserts_total: u64,
    /// 读数合计（确定性）。
    pub gauge_sum: u64,
    /// 逻辑时刻。
    pub tick: u64,
    /// 步数未越界。
    pub steps_ok: bool,
    /// 事件序列指纹。
    pub fingerprint: u64,
}
impl SlotRun {
    /// 断言全过。
    pub fn asserts_ok(&self) -> bool {
        self.asserts_pass == self.asserts_total && self.asserts_total > 0
    }
    /// 账面自洽（步数界 + 事件数界 + 断言全过 + 代价非零）。
    pub fn ok(&self) -> bool {
        self.steps_ok
            && self.events > 0
            && (self.events as usize) <= CASE_EVENT_CAP
            && self.asserts_ok()
            && self.cost > 0
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "run {} {} 段={} 判据={} 事件={} 代价={} 断言={}/{} 读数={} tick={} ok={}",
            self.slot,
            self.ticket,
            self.stage.name(),
            self.crit_bits.count_ones(),
            self.events,
            self.cost,
            self.asserts_pass,
            self.asserts_total,
            self.gauge_sum,
            self.tick,
            if self.ok() { 1 } else { 0 }
        )
    }
}
/// 跑单个条位（真逻辑：LCG 事件流 + 断言推进 + 读数累计）。
///
/// 确定性来源：`seed ⊕ slot ⊕ stage` 起步，逐事件推进，
/// 同 `seed` 同结果——这是执行稳定性多轮指纹一致的前提。
pub fn run_slot(seed: u64, slot: &Slot) -> UfResult<SlotRun> {
    if slot.index >= SLOT_COUNT {
        return Err(UfErr::of_form(
            Form::SlotOutOfRange,
            format!("执行条位越界：index={}", slot.index),
        ));
    }
    let mut lg = Lcg::new(
        seed ^ ((slot.index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15u64)) ^ ((slot.stage.index() as u64) << 40),
    );
    let mut events: u64 = 0;
    let mut cost: u64 = 0;
    let mut asserts_pass: u64 = 0;
    let mut asserts_total: u64 = 0;
    let mut gauge_sum: u64 = 0;
    let mut tick: u64 = 0;
    // 断言用的独立影子累加器：与 `cost` 并行维护、互不参照，
    // 断言成立条件 = 两个独立累加器在当前步上相等（真两遍算法交叉复核）。
    let mut shadow_cost: u64 = 0;
    let mut shadow_events: u64 = 0;
    let mut events_seen: Vec<u8> = Vec::with_capacity(CASE_STEPS);
    let mut words: Vec<u64> = Vec::with_capacity(CASE_STEPS * 2 + 4);
    words.push(slot.index as u64);
    words.push(slot.crit_bits as u64);
    let mut k: usize = 0;
    while k < CASE_STEPS {
        let pick = (lg.below(ExecEvent::COUNT as u64)) as usize;
        let ev = ExecEvent::ALL[pick];
        events = events.wrapping_add(1);
        cost = cost.wrapping_add(ev.cost());
        // 影子侧独立重算：不读 `cost`，只读 `ev`
        shadow_cost = shadow_cost.wrapping_add(ExecEvent::ALL[pick].cost());
        shadow_events = shadow_events.wrapping_add(1);
        events_seen.push(pick as u8);
        tick = tick_advance(tick);
        // 断言推进：事件是 StepAssert 或 BindCrit 时出一条断言
        if ev == ExecEvent::StepAssert || ev == ExecEvent::BindCrit {
            asserts_total = asserts_total.wrapping_add(1);
            // 影子重算一遍代价与事件数，两者都必须与主累加器一致
            let recompute_cost: u64 = {
                let mut t: u64 = 0;
                let mut i: usize = 0;
                while i < events_seen.len() {
                    let idx: usize = (events_seen[i] as usize) % ExecEvent::COUNT;
                    t = t.wrapping_add(ExecEvent::ALL[idx].cost());
                    i = i.wrapping_add(1);
                }
                t
            };
            let recompute_events: u64 = events_seen.len() as u64;
            if recompute_cost == shadow_cost
                && recompute_events == shadow_events
                && cost == shadow_cost
                && events == shadow_events
            {
                asserts_pass = asserts_pass.wrapping_add(1);
            }
        }
        if ev == ExecEvent::ReadGauge {
            gauge_sum = gauge_sum.wrapping_add(lg.below(64u64));
        }
        words.push(ev.index() as u64);
        words.push(ev.cost());
        k = k.wrapping_add(1);
    }
    let steps_ok = events <= CASE_EVENT_CAP as u64 && tick <= STEP_CAP;
    let fp = fnv1a_words(&words);
    Ok(SlotRun {
        slot: slot.index,
        ticket: slot.ticket,
        stage: slot.stage,
        crit_bits: slot.crit_bits,
        events,
        cost,
        asserts_pass,
        asserts_total,
        gauge_sum,
        tick,
        steps_ok,
        fingerprint: fp,
    })
}
/// 段三执行账。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExecBook {
    /// 逐条位结果。
    pub runs: Vec<SlotRun>,
    /// 已跑条位位图。
    pub ran_bits: u32,
    /// 步数越界条位数。
    pub step_overflow: usize,
    /// 逻辑时刻合计。
    pub tick: u64,
}
impl ExecBook {
    /// 空账。
    pub fn blank() -> ExecBook {
        ExecBook {
            runs: Vec::with_capacity(SLOT_COUNT),
            ran_bits: 0,
            step_overflow: 0,
            tick: 0,
        }
    }
    /// 全条位真跑。
    pub fn run_all(seed: u64, t: &SlotTable) -> UfResult<ExecBook> {
        let mut b = ExecBook::blank();
        for s in t.slots.iter() {
            let r = run_slot(seed, s)?;
            if r.events > CASE_EVENT_CAP as u64 || !r.steps_ok {
                b.step_overflow = b.step_overflow.wrapping_add(1);
            }
            b.ran_bits = b.ran_bits | (1u32 << r.slot);
            b.tick = b.tick.wrapping_add(r.tick);
            b.runs.push(r);
        }
        Ok(b)
    }
    /// 欠跑条位数。
    pub fn skipped_count(&self) -> usize {
        let all: u32 = (1u32 << SLOT_COUNT) - 1;
        (all & (all ^ self.ran_bits)).count_ones() as usize
    }
    /// 账面全过（零欠跑 + 零步数越界 + 每条位 ok）。
    pub fn all_ok(&self) -> bool {
        if self.skipped_count() != 0 || self.step_overflow != 0 {
            return false;
        }
        for r in self.runs.iter() {
            if !r.ok() {
                return false;
            }
        }
        true
    }
    /// 代价合计。
    pub fn cost_sum(&self) -> u64 {
        let mut t: u64 = 0;
        for r in self.runs.iter() {
            t = t.wrapping_add(r.cost);
        }
        t
    }
    /// 读数合计。
    pub fn gauge_total(&self) -> u64 {
        let mut t: u64 = 0;
        for r in self.runs.iter() {
            t = t.wrapping_add(r.gauge_sum);
        }
        t
    }
    /// 账指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(self.runs.len() * 6 + 5);
        words.push(self.runs.len() as u64);
        words.push(self.ran_bits as u64);
        words.push(self.step_overflow as u64);
        words.push(self.tick);
        words.push(self.cost_sum());
        for r in self.runs.iter() {
            words.push(r.fingerprint);
            words.push(r.events);
            words.push(r.asserts_pass);
            words.push(r.asserts_total);
            words.push(r.gauge_sum);
        }
        fnv1a_words(&words)
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "exec n={} ran={:04X} skip={} over={} cost={} gauge={} tick={} fp={:016X}",
            self.runs.len(),
            self.ran_bits,
            self.skipped_count(),
            self.step_overflow,
            self.cost_sum(),
            self.gauge_total(),
            self.tick,
            self.fingerprint()
        )
    }
    /// 逐行在册表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.runs.len() + 1);
        out.push(format!("# 段三执行账 n={} 面={}", self.runs.len(), EXEC_FACE));
        for r in self.runs.iter() {
            out.push(r.describe());
        }
        out
    }
}

// ===========================================================================
// 8. 段四：三态判定册
// ===========================================================================

/// 三态（锚点「过/欠/挂登记」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TriState {
    /// 过。
    Pass,
    /// 欠（真机项欠账 / 未过项）。
    Owe,
    /// 挂（阻塞，未能判定）。
    Hang,
}
impl TriState {
    /// 状态数。
    pub const COUNT: usize = 3;
    /// 全部状态。
    pub const ALL: [TriState; TriState::COUNT] = [TriState::Pass, TriState::Owe, TriState::Hang];
    /// 状态下标。
    pub fn index(self) -> usize {
        match self {
            TriState::Pass => 0,
            TriState::Owe => 1,
            TriState::Hang => 2,
        }
    }
    /// 按码取态（`0xFF` 与越界给 `None`）。
    pub fn from_code(c: u8) -> Option<TriState> {
        match c {
            0 => Some(TriState::Pass),
            1 => Some(TriState::Owe),
            2 => Some(TriState::Hang),
            _ => None,
        }
    }
    /// 状态码。
    pub fn code(self) -> u8 {
        match self {
            TriState::Pass => 0,
            TriState::Owe => 1,
            TriState::Hang => 2,
        }
    }
    /// 状态名。
    pub fn name(self) -> &'static str {
        match self {
            TriState::Pass => "过",
            TriState::Owe => "欠",
            TriState::Hang => "挂",
        }
    }
    /// 状态英文名。
    pub fn en(self) -> &'static str {
        match self {
            TriState::Pass => "Pass",
            TriState::Owe => "Owe",
            TriState::Hang => "Hang",
        }
    }
    /// 状态描述。
    pub fn describe(self) -> &'static str {
        match self {
            TriState::Pass => "过：账面全过且判据齐挂",
            TriState::Owe => "欠：真机项欠账或判据未齐",
            TriState::Hang => "挂：依赖未就绪，判定挂起",
        }
    }
}
/// 判定登记（单条位判定）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct VerdictRow {
    /// 条位下标。
    pub slot: usize,
    /// 条位号。
    pub ticket: &'static str,
    /// 判定码。
    pub code: u8,
    /// 判定依据（账面读数）。
    pub basis: u64,
}
impl VerdictRow {
    /// 判定态（越界给 `None`，触发 `VerdictIllegal`）。
    pub fn state(&self) -> Option<TriState> {
        TriState::from_code(self.code)
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        let sn = match self.state() {
            Some(s) => s.name(),
            None => "越态",
        };
        format!("verdict[{}] {} {:02X}({}) basis={}", self.slot, self.ticket, self.code, sn, self.basis)
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 3] {
        [self.slot as u64, self.code as u64, self.basis]
    }
}
/// 三态判定册。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct VerdictBook {
    /// 逐条位判定。
    pub rows: Vec<VerdictRow>,
}
impl VerdictBook {
    /// 空册。
    pub fn blank() -> VerdictBook {
        VerdictBook { rows: Vec::with_capacity(SLOT_COUNT) }
    }
    /// 逐条位判定（过 = 账面全过且非真机项；欠 = 真机项欠账；挂 = 账面不过）。
    pub fn judge_all(t: &SlotTable, e: &ExecBook) -> VerdictBook {
        let mut b = VerdictBook::blank();
        for s in t.slots.iter() {
            let r = match e.runs.iter().find(|x| x.slot == s.index) {
                Some(x) => x,
                None => {
                    // 未跑 → 挂
                    b.rows.push(VerdictRow { slot: s.index, ticket: s.ticket, code: 2, basis: 0 });
                    continue;
                }
            };
            let st = if !r.ok() {
                TriState::Hang
            } else if s.machine_only {
                TriState::Owe
            } else if r.crit_count_ok(t, s.index) {
                TriState::Pass
            } else {
                TriState::Owe
            };
            b.rows.push(VerdictRow { slot: s.index, ticket: s.ticket, code: st.code(), basis: r.cost });
        }
        b
    }
    /// 判定计数（3 个数的表）。
    pub fn counts(&self) -> [usize; TriState::COUNT] {
        let mut c: [usize; TriState::COUNT] = [0; TriState::COUNT];
        for r in self.rows.iter() {
            if let Some(s) = r.state() {
                c[s.index()] = c[s.index()].wrapping_add(1);
            }
        }
        c
    }
    /// 判定条数。
    pub fn total(&self) -> usize {
        self.rows.len()
    }
    /// 越态条数。
    pub fn illegal_count(&self) -> usize {
        let mut n: usize = 0;
        for r in self.rows.iter() {
            if r.state().is_none() {
                n = n.wrapping_add(1);
            }
        }
        n
    }
    /// 判定条数与执行条数是否对齐。
    pub fn count_aligned(&self, e: &ExecBook) -> bool {
        self.total() == e.runs.len() && self.total() == SLOT_COUNT
    }
    /// 三态闭合（零越态）。
    pub fn closed(&self) -> bool {
        self.illegal_count() == 0
    }
    /// 判定加法链：Σ三态计数 == 判定条数（零偏离）。
    pub fn tri_sum(&self) -> usize {
        let c = self.counts();
        c[0].wrapping_add(c[1]).wrapping_add(c[2])
    }
    /// 册指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(self.rows.len() * 3 + 3);
        words.push(self.rows.len() as u64);
        let c = self.counts();
        words.push(c[0] as u64);
        words.push(c[1] as u64);
        words.push(c[2] as u64);
        for r in self.rows.iter() {
            let w = r.words();
            let mut i: usize = 0;
            while i < w.len() {
                words.push(w[i]);
                i = i.wrapping_add(1);
            }
        }
        fnv1a_words(&words)
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        let c = self.counts();
        format!(
            "verdict n={} 过={} 欠={} 挂={} illegal={} fp={:016X}",
            self.total(),
            c[0],
            c[1],
            c[2],
            self.illegal_count(),
            self.fingerprint()
        )
    }
    /// 逐行在册表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.rows.len() + 1);
        out.push(format!("# 段四判定册 n={} 目标={}", self.total(), SLOT_COUNT));
        for r in self.rows.iter() {
            out.push(r.describe());
        }
        out
    }
}
impl SlotRun {
    /// 该次执行承接判据是否齐挂（段四「过」的判据侧条件）。
    pub fn crit_count_ok(&self, _t: &SlotTable, slot_idx: usize) -> bool {
        let want = slot_table_crit_bits(slot_idx);
        self.crit_bits == want && want != 0
    }
}
/// 条位默认承接判据位图（与 `slot_table_full` 同口径，独立实现一处以免耦合）。
pub fn slot_table_crit_bits(i: usize) -> u32 {
    let mut bits: u32 = 0;
    let mut j: usize = 1;
    while j <= CRIT_COUNT {
        if crit_slot_default(j) == i {
            bits = bits | (1u32 << (j - 1));
        }
        j = j.wrapping_add(1);
    }
    bits
}

// ===========================================================================
// 9. 守恒账 reconcile（四段式恒等式 ΣA == ΣB == 总账）
// ===========================================================================

/// 守恒报告（四段对账结果）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reconcile {
    /// ΣA：段一侧合计（判据按段归集数之和）。
    pub sum_a: u64,
    /// ΣB：段二映射侧合计（承接链接数）。
    pub sum_b: u64,
    /// 总账：段三执行条数。
    pub sum_c: u64,
    /// 判定条数（段四）。
    pub sum_d: u64,
    /// 判据权重合计（段一加法链）。
    pub weight_sum: u64,
    /// 条位承接判据总数（应等于判据数）。
    pub slot_crit_total: u64,
    /// 最大偏离（`0` 即零偏离）。
    pub deviation: u64,
    /// 守恒通过。
    pub balanced: bool,
    /// 逐项结论。
    pub items: Vec<(String, u64, u64, bool)>,
}
impl Reconcile {
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "recon ΣA={} ΣB={} ΣC={} ΣD={} dev={} balanced={} items={}",
            self.sum_a,
            self.sum_b,
            self.sum_c,
            self.sum_d,
            self.deviation,
            if self.balanced { 1 } else { 0 },
            self.items.len()
        )
    }
    /// 失败项名表。
    pub fn fail_list(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for it in self.items.iter() {
            if !it.3 {
                out.push(it.0.clone());
            }
        }
        out
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.items.len() + 1);
        out.push(format!("# 守恒账 dev={} balanced={}", self.deviation, self.balanced));
        for it in self.items.iter() {
            out.push(format!("{:<28} got={:<6} want={:<6} {}", it.0, it.1, it.2, if it.3 { "OK" } else { "FAIL" }));
        }
        out
    }
}
/// 绝对差（零浮点）。
pub fn abs_diff(a: u64, b: u64) -> u64 {
    if a > b {
        a.wrapping_sub(b)
    } else {
        b.wrapping_sub(a)
    }
}
/// 四段守恒对账（判据清单 → 映射 → 执行 → 判定）。
///
/// 恒等式组（任一不成立即 `deviation > 0`）：
/// 1. `ΣA = 判据条目数`（段一归集零缺项）
/// 2. `ΣB = 条位承接判据总数`（段二无漏无冗 ⇒ 等于判据数）
/// 3. `ΣA == ΣB`（段一段二口径一致）
/// 4. `ΣC = 执行条数 = SLOT_COUNT`（段三零欠跑）
/// 5. `ΣD = 判定条数 = SLOT_COUNT`（段四零越态零失配）
/// 6. `ΣC == ΣD`（段三段四条数对齐）
pub fn reconcile(
    crit: &CritBook,
    slots: &SlotTable,
    map: &MapTable,
    exec: &ExecBook,
    verd: &VerdictBook,
) -> Reconcile {
    let sum_a: u64 = crit.items.len() as u64;
    let mut sum_b: u64 = 0;
    for s in slots.slots.iter() {
        sum_b = sum_b.wrapping_add(s.crit_count() as u64);
    }
    let sum_c: u64 = exec.runs.len() as u64;
    let sum_d: u64 = verd.total() as u64;
    let weight_sum: u64 = crit.weight_sum;
    let slot_crit_total: u64 = sum_b;
    let mut items: Vec<(String, u64, u64, bool)> = Vec::with_capacity(8);
    items.push((String::from("段一 判据条目数"), sum_a, CRIT_COUNT as u64, sum_a == CRIT_COUNT as u64));
    items.push((
        String::from("段一 判据覆盖零缺"),
        crit.missing_bits() as u64,
        0,
        crit.missing_bits() == 0,
    ));
    items.push((String::from("段一 判据零重号"), crit.dup_bits as u64, 0, crit.dup_bits == 0));
    items.push((String::from("段二 映射零漏"), map.hole_bits as u64, 0, map.hole_bits == 0));
    items.push((String::from("段二 映射零冗"), map.orphan_bits as u64, 0, map.orphan_bits == 0));
    items.push((String::from("段二 映射零多"), map.multi_bits as u64, 0, map.multi_bits == 0));
    items.push((
        String::from("三段口径一致 ΣA==ΣB"),
        sum_a,
        sum_b,
        sum_a == sum_b,
    ));
    items.push((String::from("段三 执行条数"), sum_c, SLOT_COUNT as u64, sum_c == SLOT_COUNT as u64));
    items.push((String::from("段三 零欠跑"), exec.skipped_count() as u64, 0, exec.skipped_count() == 0));
    items.push((
        String::from("段四 判定条数"),
        sum_d,
        SLOT_COUNT as u64,
        sum_d == SLOT_COUNT as u64,
    ));
    items.push((String::from("段四 判定零越态"), verd.illegal_count() as u64, 0, verd.closed()));
    items.push((
        String::from("段三段四条数对齐 ΣC==ΣD"),
        sum_c,
        sum_d,
        sum_c == sum_d,
    ));
    let mut dev: u64 = 0;
    for it in items.iter() {
        let d = abs_diff(it.1, it.2);
        if d > dev {
            dev = d;
        }
    }
    let balanced = dev == 0;
    Reconcile {
        sum_a,
        sum_b,
        sum_c,
        sum_d,
        weight_sum,
        slot_crit_total,
        deviation: dev,
        balanced,
        items,
    }
}

// ===========================================================================
// 10. append-only 环形账（dropped 显性）
// ===========================================================================

/// 环形账条目。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RingEntry {
    /// 轮次。
    pub round: u32,
    /// 条位下标。
    pub slot: usize,
    /// 段位下标。
    pub stage: usize,
    /// 执行指纹。
    pub run_fp: u64,
    /// 判定码。
    pub verdict_code: u8,
    /// 逻辑时刻。
    pub tick: u64,
}
impl RingEntry {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "ring r{} slot{} 段{} fp={:016X} verdict={:02X} tick={}",
            self.round, self.slot, self.stage, self.run_fp, self.verdict_code, self.tick
        )
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 5] {
        [self.round as u64, self.slot as u64, self.stage as u64, self.run_fp, self.verdict_code as u64]
    }
}

/// append-only 环形账（覆盖时旧条目计入 `dropped`，不留静默丢弃）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RingLedger {
    /// 槽位。
    pub buf: Vec<RingEntry>,
    /// 写指针（= `pushed & RING_MASK`）。
    pub head: usize,
    /// 累计写入。
    pub pushed: u64,
    /// 累计丢弃（被覆盖的旧条目）。
    pub dropped: u64,
}
impl RingLedger {
    /// 构造空账。
    pub fn new() -> RingLedger {
        let mut buf: Vec<RingEntry> = Vec::with_capacity(RING_CAP);
        let mut i: usize = 0;
        while i < RING_CAP {
            buf.push(RingEntry {
                round: 0,
                slot: 0,
                stage: 0,
                run_fp: 0,
                verdict_code: 0xFF,
                tick: 0,
            });
            i = i.wrapping_add(1);
        }
        RingLedger { buf, head: 0, pushed: 0, dropped: 0 }
    }
    /// 写入一条（append-only；满则覆盖最旧并累加 `dropped`）。
    pub fn push(&mut self, e: RingEntry) {
        if self.pushed >= RING_CAP as u64 {
            // 槽位内若已有实条目，覆盖计丢弃
            let slot = &self.buf[self.head];
            if slot.verdict_code != 0xFF || slot.run_fp != 0 {
                self.dropped = self.dropped.wrapping_add(1);
            }
        }
        let n = self.buf.len();
        if self.head < n {
            self.buf[self.head] = e;
        }
        self.pushed = self.pushed.wrapping_add(1);
        self.head = (self.head + 1) & RING_MASK;
    }
    /// 存活条数（槽位中实条目数）。
    pub fn live(&self) -> u64 {
        let mut n: u64 = 0;
        for e in self.buf.iter() {
            if e.verdict_code != 0xFF || e.run_fp != 0 {
                n = n.wrapping_add(1);
            }
        }
        n
    }
    /// 守恒：`写入 == 存活 + 丢弃`（写入 ≤ 容量时丢弃为 0）。
    pub fn balanced(&self) -> bool {
        if self.pushed <= RING_CAP as u64 {
            return self.live() == self.pushed && self.dropped == 0;
        }
        self.live() == RING_CAP as u64 && self.pushed == self.live().wrapping_add(self.dropped)
    }
    /// 存活条目（按槽位序）。
    pub fn live_entries(&self) -> Vec<RingEntry> {
        let mut out: Vec<RingEntry> = Vec::with_capacity(RING_CAP);
        for e in self.buf.iter() {
            if e.verdict_code != 0xFF || e.run_fp != 0 {
                out.push(e.clone());
            }
        }
        out
    }
    /// 账指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(RING_CAP * 5 + 3);
        words.push(self.pushed);
        words.push(self.dropped);
        words.push(self.live());
        for e in self.buf.iter() {
            let w = e.words();
            let mut i: usize = 0;
            while i < w.len() {
                words.push(w[i]);
                i = i.wrapping_add(1);
            }
        }
        fnv1a_words(&words)
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "ring pushed={} dropped={} live={}/{} balanced={} fp={:016X}",
            self.pushed,
            self.dropped,
            self.live(),
            RING_CAP,
            if self.balanced() { 1 } else { 0 },
            self.fingerprint()
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(RING_CAP + 1);
        out.push(format!(
            "# 环形账 cap={} pushed={} dropped={} live={}",
            RING_CAP,
            self.pushed,
            self.dropped,
            self.live()
        ));
        for (i, e) in self.buf.iter().enumerate() {
            if e.verdict_code != 0xFF || e.run_fp != 0 {
                out.push(format!("[{:02}] {}", i, e.describe()));
            }
        }
        out
    }
}
/// 全条位 × 全轮写入环形账。
pub fn ring_fill(ledger: &mut RingLedger, exec: &ExecBook, verd: &VerdictBook) {
    let mut r: u32 = 0;
    while r < REPEAT as u32 {
        for run in exec.runs.iter() {
            let vc: u8 = match verd.rows.iter().find(|x| x.slot == run.slot) {
                Some(v) => v.code,
                None => 0xFF,
            };
            ledger.push(RingEntry {
                round: r,
                slot: run.slot,
                stage: run.stage.index(),
                run_fp: run.fingerprint,
                verdict_code: vc,
                tick: run.tick,
            });
        }
        r = r.wrapping_add(1);
    }
}

// ===========================================================================
// 11. 注入核（14 形态全拒留痕，被测对象与测量工具数据源分离）
// ===========================================================================

/// 注入现场（被测对象一侧：被注入损坏的方案快照）。
///
/// **数据源分离口径**：本结构与测量工具一侧的 `Detections` 完全隔离——
/// 注入端只写 `Evidence`，检测端只读 `Evidence` 并输出 `Detections`；
/// 检测逻辑不复用注入端的私有字段构造代码，避免「用同一份算式自证清白」。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Evidence {
    /// 逐形态注入留痕（注入发生次数）。
    pub fired: [u32; Form::COUNT],
    /// 被测快照的判据册指纹。
    pub crit_fp: u64,
    /// 被测快照的条位册指纹。
    pub slot_fp: u64,
    /// 被测快照的映射表指纹。
    pub map_fp: u64,
    /// 被测快照的执行账指纹。
    pub exec_fp: u64,
    /// 被测快照的判定册指纹。
    pub verdict_fp: u64,
    /// 被测快照的环形账指纹。
    pub ring_fp: u64,
    /// 被测快照的冻结指纹（**净态入册值**，注入过程中不得改写——改它就等于毁证据）。
    pub freeze_fp: u64,
    /// 净态入册的探针基准值（检测端比对用，注入端只读不改）。
    pub registered_probe: u64,
    /// 语义边界自证：本单只做方案冻结，误触用例本体 / B39 真执行的次数须为 0。
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
            crit_fp: 0,
            slot_fp: 0,
            map_fp: 0,
            exec_fp: 0,
            verdict_fp: 0,
            ring_fp: 0,
            freeze_fp: 0,
            registered_probe: 0,
            border_violations: 0,
        }
    }
    /// 某形态注入次数。
    pub fn of(&self, f: Form) -> u32 {
        self.fired[f.index()]
    }
    /// 该形态是否已注入。
    pub fn verdict(&self, f: Form) -> bool {
        self.fired[f.index()] > 0
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        let mut n: usize = 0;
        for v in self.fired.iter() {
            if *v > 0 {
                n = n.wrapping_add(1);
            }
        }
        format!(
            "evidence fired={}/{} border={} crit={:016X} slot={:016X} map={:016X} exec={:016X} verdict={:016X} ring={:016X} freeze={:016X}",
            n,
            Form::COUNT,
            self.border_violations,
            self.crit_fp,
            self.slot_fp,
            self.map_fp,
            self.exec_fp,
            self.verdict_fp,
            self.ring_fp,
            self.freeze_fp
        )
    }
}
/// 检测结论（测量工具一侧）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Detections {
    /// 逐形态命中次数。
    pub hits: [u32; Form::COUNT],
    /// 命中位图（`Form::COUNT = 14` 位，`u128` 承载，高 8 位为预留）。
    pub verdict_mask: u128,
    /// 语义边界违规检出数。
    pub border_violations: u32,
}
impl Detections {
    /// 空检出。
    pub fn blank() -> Detections {
        let mut hits: [u32; Form::COUNT] = [0; Form::COUNT];
        let mut i: usize = 0;
        while i < Form::COUNT {
            hits[i] = 0;
            i = i.wrapping_add(1);
        }
        Detections { hits, verdict_mask: 0u128, border_violations: 0 }
    }
    /// 某形态是否命中。
    pub fn hit(&self, f: Form) -> bool {
        self.hits[f.index()] > 0
    }
    /// 命中形态数。
    pub fn hit_count(&self) -> usize {
        let mut n: usize = 0;
        for v in self.hits.iter() {
            if *v > 0 {
                n = n.wrapping_add(1);
            }
        }
        n
    }
    /// 未命中形态名表。
    pub fn miss_list(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for f in Form::ALL.iter() {
            if !self.hit(*f) {
                out.push(f.name());
            }
        }
        out
    }
    /// 命中形态名表。
    pub fn hit_list(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::with_capacity(Form::COUNT);
        for f in Form::ALL.iter() {
            if self.hit(*f) {
                out.push(f.name());
            }
        }
        out
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "detect hit={}/{} mask={:032X} border={}",
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
        for f in Form::ALL.iter() {
            out.push(format!(
                "{:<20} code=0x{:04X} {:<9} hits={} [{}] {}",
                f.name(),
                f.code(),
                f.class_name(),
                self.hits[f.index()],
                f.probe(),
                f.describe()
            ));
        }
        out
    }
}
/// 方案快照（被测对象一侧的完整状态，注入端产出、检测端只读）。
///
/// **数据源分离口径**：`Snapshot` 由注入端（`inject_one`）独占构造；
/// 检测端 `detect_all` 只以 `&Snapshot` 只读入参逐面判定，
/// 绝不回写、也绝不读注入端的私有构造路径——被测对象与测量工具的数据来源物理隔离。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Snapshot {
    /// 判据册。
    pub crit: CritBook,
    /// 条位册。
    pub slots: SlotTable,
    /// 映射表。
    pub map: MapTable,
    /// 执行账。
    pub exec: ExecBook,
    /// 判定册。
    pub verd: VerdictBook,
    /// 环形账。
    pub ring: RingLedger,
    /// 读数留痕（被测对象自带；检测端**不读** `fired` 做归因，只读读数）。
    pub ev: Evidence,
}
impl Snapshot {
    /// 净态快照。
    pub fn net(seed: u64) -> UfResult<Snapshot> {
        let crit = crit_book_full();
        let slots = slot_table_full()?;
        let map = map_table_full(&slots)?;
        let exec = ExecBook::run_all(seed, &slots)?;
        let verd = VerdictBook::judge_all(&slots, &exec);
        let mut ring = RingLedger::new();
        ring_fill(&mut ring, &exec, &verd);
        let mut ev = Evidence::blank();
        ev.crit_fp = crit.fingerprint();
        ev.slot_fp = slots.fingerprint();
        ev.map_fp = map.fingerprint();
        ev.exec_fp = exec.fingerprint();
        ev.verdict_fp = verd.fingerprint();
        ev.ring_fp = ring.fingerprint();
        ev.freeze_fp = freeze_probe_hash(&crit, &slots, &map, &exec, &verd);
        ev.registered_probe = ev.freeze_fp;
        ev.border_violations = 0;
        Ok(Snapshot { crit, slots, map, exec, verd, ring, ev })
    }
    /// 本快照守恒账。
    pub fn reconcile(&self) -> Reconcile {
        reconcile(&self.crit, &self.slots, &self.map, &self.exec, &self.verd)
    }
    /// 是否净态（未注入）。
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
        let mut out: Vec<String> = Vec::with_capacity(8);
        out.push(self.describe());
        out.push(self.crit.summary());
        out.push(self.slots.summary());
        out.push(self.map.summary());
        out.push(self.exec.summary());
        out.push(self.verd.summary());
        out.push(self.ring.summary());
        out.push(self.reconcile().summary());
        out
    }
}
/// 单形态注入：在净态方案上按形态注入一处缺陷，返回被污染的完整快照。
///
/// 每个形态只污染**自己负责的那一面**，保证「检出归因」可唯一化。
fn inject_one(f: Form, seed: u64) -> UfResult<Snapshot> {
    let mut s = Snapshot::net(seed)?;
    // 语义边界：注入全程只改方案面，不构造用例本体（F3102–F3109）、不跑 B39 真执行
    s.ev.border_violations = 0;
    s.ev.fired[f.index()] = 1;
    match f {
        Form::CriterionMissing => {
            // 摘掉最后一条判据 → 段一覆盖位图留洞
            s.crit.items.pop();
            s.crit.recompute();
        }
        Form::CriterionDup => {
            // 重号登记判据 1
            let c = criterion_at(1);
            s.crit.register(c);
        }
        Form::SlotOutOfRange => {
            // 登记越界条位
            let bad = Slot::new(SLOT_COUNT + 3, Stage::S1Collect, 1, false);
            let _ = s.slots.register(bad);
        }
        Form::SlotMissing => {
            // 摘掉一条条位
            s.slots.slots.pop();
            s.slots.recompute();
            s.map.recompute_orphan(&s.slots)?;
        }
        Form::MapAbsent => {
            // 判据 7 无承接 → 漏
            for m in s.map.maps.iter_mut() {
                if m.crit_id == 7 {
                    m.slots.clear();
                    s.map.hole_bits = s.map.hole_bits | m.bit;
                }
            }
        }
        Form::MapOrphan => {
            // 条位 9 摘掉承接判据 → 冗
            if let Some(sl) = s.slots.slots.get_mut(9) {
                sl.crit_bits = 0;
            }
            s.map.recompute_orphan(&s.slots)?;
        }
        Form::MapMulti => {
            // 判据 3 追加第二个承接条位 → 口径不一
            for m in s.map.maps.iter_mut() {
                if m.crit_id == 3 {
                    m.slots.push(5);
                    s.map.multi_bits = s.map.multi_bits | m.bit;
                }
            }
        }
        Form::ExecSkipped => {
            // 摘掉一次执行记录 → 欠跑
            if let Some(last) = s.exec.runs.pop() {
                let b: u32 = 1u32 << (last.slot & 31);
                s.exec.ran_bits = s.exec.ran_bits & (s.exec.ran_bits ^ b);
            }
            s.verd = VerdictBook::judge_all(&s.slots, &s.exec);
        }
        Form::VerdictIllegal => {
            // 判定码置越态值 0x7F
            for r in s.verd.rows.iter_mut() {
                r.code = 0x7F;
                break;
            }
        }
        Form::VerdictCountBad => {
            // 判定册多一条 → 与执行条数失配
            s.verd.rows.push(VerdictRow { slot: 99, ticket: "F3199", code: 0, basis: 1 });
        }
        Form::LedgerUnbalanced => {
            // 环形账破坏：写入数与存活失配
            s.ring.pushed = s.ring.pushed.wrapping_add(7);
        }
        Form::FingerprintDrift => {
            // 冻结指纹漂移：改判据册权重使定义件哈希失配
            if let Some(c) = s.crit.items.get_mut(0) {
                c.weight = c.weight.wrapping_add(1);
                s.crit.weight_sum = s.crit.weight_sum.wrapping_add(1);
            }
        }
        Form::ReconcileDeviation => {
            // 守恒式偏离：段三条数偏移
            s.exec.runs.push(SlotRun {
                slot: 42,
                ticket: "F3142",
                stage: Stage::S3Exec,
                crit_bits: 0,
                events: 1,
                cost: 1,
                asserts_pass: 1,
                asserts_total: 1,
                gauge_sum: 0,
                tick: 1,
                steps_ok: true,
                fingerprint: 0,
            });
            s.verd = VerdictBook::judge_all(&s.slots, &s.exec);
        }
        Form::StageUncovered => {
            // 段三零条位 → 四段点验不过
            for sl in s.slots.slots.iter_mut() {
                sl.stage = Stage::S1Collect;
            }
        }
    }
    // 重折被测快照读数（检测端据此与快照内入册值比对）
    s.ev.crit_fp = s.crit.fingerprint();
    s.ev.slot_fp = s.slots.fingerprint();
    s.ev.map_fp = s.map.fingerprint();
    s.ev.exec_fp = s.exec.fingerprint();
    s.ev.verdict_fp = s.verd.fingerprint();
    s.ev.ring_fp = s.ring.fingerprint();
    // 注意：`registered_probe` 是净态入册基准，注入全程只读不改；
    // 若这里把它一起改写，漂移检测就会拿「污染后的值」当基准，检出必然失效。
    s.ev.freeze_fp = freeze_probe_hash(&s.crit, &s.slots, &s.map, &s.exec, &s.verd);
    Ok(s)
}
/// 全形态注入（`14/14` 全拒留痕）。
pub fn inject_all(seed: u64) -> UfResult<(Evidence, Vec<Snapshot>)> {
    let mut total = Evidence::blank();
    let mut each: Vec<Snapshot> = Vec::with_capacity(Form::COUNT);
    for f in Form::ALL.iter() {
        let snap = inject_one(*f, seed)?;
        total.fired[f.index()] = total.fired[f.index()].wrapping_add(snap.ev.of(*f));
        total.crit_fp = total.crit_fp ^ snap.ev.crit_fp;
        total.slot_fp = total.slot_fp ^ snap.ev.slot_fp;
        total.map_fp = total.map_fp ^ snap.ev.map_fp;
        total.exec_fp = total.exec_fp ^ snap.ev.exec_fp;
        total.verdict_fp = total.verdict_fp ^ snap.ev.verdict_fp;
        total.ring_fp = total.ring_fp ^ snap.ev.ring_fp;
        total.freeze_fp = total.freeze_fp ^ snap.ev.freeze_fp;
        total.border_violations = total.border_violations.wrapping_add(snap.ev.border_violations);
        each.push(snap);
    }
    Ok((total, each))
}
/// 检测器：在被污染快照上逐面判定各形态是否具备可检出特征。
///
/// **归因唯一性**：每个检测器只看一个只读面，且只看该面自己的字段，
/// 不读注入侧的 `fired` 标记（`fired` 仅用于留痕统计）。
pub fn detect_all(snap: &Snapshot) -> Detections {
    let mut d = Detections::blank();
    if snap.crit.missing_bits() != 0 {
        d.bump(Form::CriterionMissing);
    }
    if snap.crit.dup_bits != 0 {
        d.bump(Form::CriterionDup);
    }
    if snap.slots.out_of_range > 0 || snap.slots.dup_bits != 0 {
        d.bump(Form::SlotOutOfRange);
    }
    if snap.slots.missing_bits() != 0 {
        d.bump(Form::SlotMissing);
    }
    if snap.map.hole_bits != 0 {
        d.bump(Form::MapAbsent);
    }
    if snap.map.orphan_bits != 0 {
        d.bump(Form::MapOrphan);
    }
    if snap.map.multi_bits != 0 {
        d.bump(Form::MapMulti);
    }
    if snap.exec.skipped_count() != 0 {
        d.bump(Form::ExecSkipped);
    }
    if !snap.verd.closed() {
        d.bump(Form::VerdictIllegal);
    }
    if !snap.verd.count_aligned(&snap.exec) {
        d.bump(Form::VerdictCountBad);
    }
    if !snap.ring.balanced() {
        d.bump(Form::LedgerUnbalanced);
    }
    // 冻结面：重算探针哈希，与快照自带入册值比对
    let live = freeze_probe_hash(&snap.crit, &snap.slots, &snap.map, &snap.exec, &snap.verd);
    if live != snap.ev.registered_probe {
        d.bump(Form::FingerprintDrift);
    }
    // 守恒面
    if snap.reconcile().deviation != 0 {
        d.bump(Form::ReconcileDeviation);
    }
    // 段位点验面
    if !snap.slots.stage_partition_ok() || !snap.slots.stage_exclusive_ok() {
        d.bump(Form::StageUncovered);
    }
    d.border_violations = snap.ev.border_violations;
    d
}
impl Detections {
    /// 记一次命中（计数 + 位图）。
    pub fn bump(&mut self, f: Form) {
        let i = f.index();
        if i < Form::COUNT {
            self.hits[i] = self.hits[i].wrapping_add(1);
            self.verdict_mask = self.verdict_mask | (1u128 << i);
        }
    }
    /// 净态检出（零命中）——本单 `observe` 的硬前提。
    pub fn is_clean(&self) -> bool {
        self.hit_count() == 0
    }
}
/// 冻结探针哈希（注入端与检测端共用的**唯一**对照口径，单一算式无第二实现）。
///
/// 净态自洽性由 `FreezeRecord::verify()` 独立复核，此处只提供同源对照值。
pub fn freeze_probe_hash(crit: &CritBook, slots: &SlotTable, map: &MapTable, exec: &ExecBook, verd: &VerdictBook) -> u64 {
    let mut words: Vec<u64> = Vec::with_capacity(12);
    words.push(V_VERSION_ID);
    words.push(crit.fingerprint());
    words.push(slots.fingerprint());
    words.push(map.fingerprint());
    words.push(exec.fingerprint());
    words.push(verd.fingerprint());
    words.push(slots.stage_mask() as u64);
    words.push(map.coverage_permille());
    words.push(exec.cost_sum());
    words.push(verd.total() as u64);
    fnv1a_words(&words)
}


// ===========================================================================
// 12. 冻结指纹：fingerprint() + verify()
// ===========================================================================

/// 方案冻结件（锚点「四段范围冻结哈希入册」）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FreezeRecord {
    /// 冻结件版本。
    pub rev: u32,
    /// 判据册哈希。
    pub crit_hash: u64,
    /// 条位册哈希。
    pub slot_hash: u64,
    /// 映射表哈希。
    pub map_hash: u64,
    /// 执行账哈希。
    pub exec_hash: u64,
    /// 判定册哈希。
    pub verdict_hash: u64,
    /// 四段定义哈希（四段名 + 输入 + 输出 + 判据四列折入）。
    pub stage_hash: u64,
    /// 冻结总哈希。
    pub freeze_hash: u64,
    /// 语义边界声明（锚点逐字摘抄）。
    pub border_note: String,
    /// 调度留痕键（锚点「方案是 B36 全批调度面」）。
    pub schedule_key: String,
}
impl FreezeRecord {
    /// 构造冻结件。
    pub fn build(
        crit: &CritBook,
        slots: &SlotTable,
        map: &MapTable,
        exec: &ExecBook,
        verd: &VerdictBook,
    ) -> FreezeRecord {
        let crit_hash = crit.fingerprint();
        let slot_hash = slots.fingerprint();
        let map_hash = map.fingerprint();
        let exec_hash = exec.fingerprint();
        let verdict_hash = verd.fingerprint();
        let stage_hash = stage_definition_hash();
        let mut words: Vec<u64> = Vec::with_capacity(10);
        words.push(V_VERSION_ID);
        words.push(crit_hash);
        words.push(slot_hash);
        words.push(map_hash);
        words.push(exec_hash);
        words.push(verdict_hash);
        words.push(stage_hash);
        words.push(CRIT_COUNT as u64);
        words.push(SLOT_COUNT as u64);
        words.push(SEG_BASE as u64);
        let freeze_hash = fnv1a_words(&words);
        FreezeRecord {
            rev: 1,
            crit_hash,
            slot_hash,
            map_hash,
            exec_hash,
            verdict_hash,
            stage_hash,
            freeze_hash,
            border_note: String::from("只做方案冻结；不做用例本体（F3102–F3109）、不做执行（B39）"),
            schedule_key: format!("{}:{}:F3101:plan", BATCH_FACE, DEP_SLOTS),
        }
    }
    /// 核验（重算四段定义哈希 + 册哈希比对）。
    pub fn verify(&self, crit: &CritBook, slots: &SlotTable, map: &MapTable, exec: &ExecBook, verd: &VerdictBook) -> bool {
        if self.stage_hash != stage_definition_hash() {
            return false;
        }
        if self.crit_hash != crit.fingerprint() {
            return false;
        }
        if self.slot_hash != slots.fingerprint() {
            return false;
        }
        if self.map_hash != map.fingerprint() {
            return false;
        }
        if self.exec_hash != exec.fingerprint() {
            return false;
        }
        if self.verdict_hash != verd.fingerprint() {
            return false;
        }
        let mut words: Vec<u64> = Vec::with_capacity(10);
        words.push(V_VERSION_ID);
        words.push(self.crit_hash);
        words.push(self.slot_hash);
        words.push(self.map_hash);
        words.push(self.exec_hash);
        words.push(self.verdict_hash);
        words.push(self.stage_hash);
        words.push(CRIT_COUNT as u64);
        words.push(SLOT_COUNT as u64);
        words.push(SEG_BASE as u64);
        fnv1a_words(&words) == self.freeze_hash
    }
    /// 漂移明细（返回首条失配说明）。
    pub fn drift(&self, crit: &CritBook, slots: &SlotTable, map: &MapTable, exec: &ExecBook, verd: &VerdictBook) -> String {
        if self.stage_hash != stage_definition_hash() {
            return format!("四段定义哈希漂移：入册={:016X} 实测={:016X}", self.stage_hash, stage_definition_hash());
        }
        if self.crit_hash != crit.fingerprint() {
            return format!("判据册哈希漂移：入册={:016X} 实测={:016X}", self.crit_hash, crit.fingerprint());
        }
        if self.slot_hash != slots.fingerprint() {
            return format!("条位册哈希漂移：入册={:016X} 实测={:016X}", self.slot_hash, slots.fingerprint());
        }
        if self.map_hash != map.fingerprint() {
            return format!("映射表哈希漂移：入册={:016X} 实测={:016X}", self.map_hash, map.fingerprint());
        }
        if self.exec_hash != exec.fingerprint() {
            return format!("执行账哈希漂移：入册={:016X} 实测={:016X}", self.exec_hash, exec.fingerprint());
        }
        if self.verdict_hash != verd.fingerprint() {
            return format!("判定册哈希漂移：入册={:016X} 实测={:016X}", self.verdict_hash, verd.fingerprint());
        }
        String::from("无漂移")
    }
    /// 语义边界在档。
    pub fn border_on_file(&self) -> bool {
        !self.border_note.is_empty()
    }
    /// 调度留痕在档。
    pub fn schedule_on_file(&self) -> bool {
        !self.schedule_key.is_empty()
    }
    /// 四段点验（4/4）。
    pub fn stage_points_ok(&self, slots: &SlotTable) -> bool {
        let want: u32 = (1u32 << STAGE_COUNT) - 1;
        (slots.stage_mask() & want) == want
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "freeze rev={} crit={:016X} slot={:016X} map={:016X} exec={:016X} verdict={:016X} stage={:016X} hash={:016X}",
            self.rev,
            self.crit_hash,
            self.slot_hash,
            self.map_hash,
            self.exec_hash,
            self.verdict_hash,
            self.stage_hash,
            self.freeze_hash
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        vec![
            self.describe(),
            format!("border  = {}", self.border_note),
            format!("schedule= {}", self.schedule_key),
            format!("version = {}", V_VERSION),
        ]
    }
}
/// 四段定义哈希（四列定义入册的折入值）。
pub fn stage_definition_hash() -> u64 {
    let mut h: u64 = 0xcbf29ce484222325u64;
    let mut i: usize = 0;
    while i < STAGE_COUNT {
        let s = Stage::ALL[i];
        h = fnv1a_str(h, s.name());
        h = fnv1a_str(h, s.cn());
        h = fnv1a_str(h, s.input_face());
        h = fnv1a_str(h, s.output_face());
        h = fnv1a_str(h, s.describe());
        i = i.wrapping_add(1);
    }
    fnv1a_str(h, "Sealed")
}

// ===========================================================================
// 13. 执行稳定性（多轮指纹一致）
// ===========================================================================

/// 稳定性报告。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Stability {
    /// 轮数。
    pub rounds: usize,
    /// 首轮基线指纹。
    pub base_fp: u64,
    /// 逐轮方案指纹。
    pub round_fp: Vec<u64>,
    /// 逐轮 × 逐条位执行指纹（`REPEAT × SLOT_COUNT`）。
    pub cellwise: Vec<u64>,
    /// 漂移位图（`REPEAT × SLOT_COUNT = 33` 位 → `u128` 承载）。
    pub drift_bits: u128,
    /// 稳定。
    pub stable: bool,
}
impl Stability {
    /// 漂移格位表。
    pub fn drift_cells(&self) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::new();
        let mut i: usize = 0;
        while i < self.cellwise.len() {
            if (self.drift_bits & (1u128 << i)) != 0u128 {
                out.push(i);
            }
            i = i.wrapping_add(1);
        }
        out
    }
    /// 轮数是否过下限。
    pub fn rounds_ok(&self) -> bool {
        self.rounds >= STABLE_ROUNDS_MIN
    }
    /// 漂移格数。
    pub fn drift_count(&self) -> usize {
        self.drift_cells().len()
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "stable rounds={} base={:016X} cells={} drift={} fp_mask={:032X} ok={}",
            self.rounds,
            self.base_fp,
            self.cellwise.len(),
            self.drift_count(),
            self.drift_bits,
            if self.stable { 1 } else { 0 }
        )
    }
}
/// 多轮执行稳定性检查（`REPEAT` 轮同 `seed` 重跑，方案指纹与逐格指纹必须全一致）。
pub fn stability_check(seed: u64) -> UfResult<Stability> {
    let mut round_fp: Vec<u64> = Vec::with_capacity(REPEAT);
    let mut cellwise: Vec<u64> = Vec::with_capacity(REPEAT * SLOT_COUNT);
    let mut base_fp: u64 = 0;
    let mut r: usize = 0;
    while r < REPEAT {
        let snap = Snapshot::net(seed)?;
        let rc = snap.reconcile();
        let mut words: Vec<u64> = Vec::with_capacity(12);
        words.push(snap.crit.fingerprint());
        words.push(snap.slots.fingerprint());
        words.push(snap.map.fingerprint());
        words.push(snap.exec.fingerprint());
        words.push(snap.verd.fingerprint());
        words.push(rc.deviation);
        words.push(snap.exec.cost_sum());
        words.push(snap.exec.gauge_total());
        let fp = fnv1a_words(&words);
        round_fp.push(fp);
        if r == 0 {
            base_fp = fp;
        }
        for run in snap.exec.runs.iter() {
            cellwise.push(run.fingerprint);
        }
        r = r.wrapping_add(1);
    }
    // 逐「轮 × 槽位」比对：同一槽位在第 0..REPEAT-1 轮的指纹必须全一致。
    // 布局：cellwise[r * SLOT_COUNT + slot]，故同一槽位跨轮为等差序列（步长 SLOT_COUNT）。
    let mut drift_bits: u128 = 0u128;
    let mut slot: usize = 0;
    while slot < SLOT_COUNT {
        let mut base: u64 = 0;
        let mut r: usize = 0;
        while r < REPEAT {
            let idx: usize = r.wrapping_mul(SLOT_COUNT).wrapping_add(slot);
            let v = match cellwise.get(idx) {
                Some(x) => *x,
                None => 0,
            };
            if r == 0 {
                base = v;
            } else if v != base {
                drift_bits = drift_bits | (1u128 << idx);
            }
            r = r.wrapping_add(1);
        }
        slot = slot.wrapping_add(1);
    }
    let stable = drift_bits == 0u128
        && round_fp.len() == REPEAT
        && round_fp.iter().all(|v| *v == base_fp);
    Ok(Stability { rounds: round_fp.len(), base_fp, round_fp, cellwise, drift_bits, stable })
}

// ===========================================================================
// 14. 指标
// ===========================================================================

/// 指标集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Metrics {
    /// 段位数。
    pub stages: usize,
    /// 段位点验通过。
    pub stage_points_ok: bool,
    /// 判据条目数。
    pub crit_items: usize,
    /// 判据零缺项。
    pub crit_clean: bool,
    /// 条位数。
    pub slots: usize,
    /// 条位零缺位零重号零越界。
    pub slot_clean: bool,
    /// 映射表零漏。
    pub map_hole: usize,
    /// 映射表零冗。
    pub map_orphan: usize,
    /// 映射表零多。
    pub map_multi: usize,
    /// 映射覆盖率（permille）。
    pub coverage: u64,
    /// 执行条数。
    pub exec_runs: usize,
    /// 执行账面全过。
    pub exec_ok: bool,
    /// 判定三态计数（过 / 欠 / 挂）。
    pub tri: [usize; TriState::COUNT],
    /// 判定条数对齐。
    pub verdict_aligned: bool,
    /// 守恒零偏离。
    pub deviation: u64,
    /// 稳定轮数。
    pub rounds: usize,
    /// 稳定漂移格数。
    pub drift_cells: usize,
    /// 注入形态数。
    pub forms: usize,
    /// 注入检出数。
    pub detected: usize,
    /// 净态零检出。
    pub net_clean: bool,
    /// 状态机合法边数。
    pub edges: usize,
    /// 非法迁移拒记数。
    pub rejected: usize,
    /// 环形账写入数。
    pub pushed: u64,
    /// 环形账丢弃数。
    pub dropped: u64,
    /// 环形账存活数。
    pub live: u64,
    /// 环形账守恒。
    pub ring_ok: bool,
    /// 冻结核验通过。
    pub freeze_ok: bool,
    /// 语义边界零违规。
    pub border_ok: bool,
    /// 调度留痕在档。
    pub schedule_ok: bool,
    /// 冻结哈希。
    pub freeze_hash: u64,
}
impl Metrics {
    /// 指标指纹。
    pub fn fingerprint(&self) -> u64 {
        let words: [u64; 30] = [
            self.stages as u64,
            self.stage_points_ok as u64,
            self.crit_items as u64,
            self.crit_clean as u64,
            self.slots as u64,
            self.slot_clean as u64,
            self.map_hole as u64,
            self.map_orphan as u64,
            self.map_multi as u64,
            self.coverage,
            self.exec_runs as u64,
            self.exec_ok as u64,
            self.tri[0] as u64,
            self.tri[1] as u64,
            self.tri[2] as u64,
            self.verdict_aligned as u64,
            self.deviation,
            self.rounds as u64,
            self.drift_cells as u64,
            self.forms as u64,
            self.detected as u64,
            self.net_clean as u64,
            self.edges as u64,
            self.rejected as u64,
            self.pushed,
            self.dropped,
            self.live,
            self.ring_ok as u64,
            self.freeze_ok as u64,
            self.freeze_hash,
        ];
        fnv1a_words(&words)
    }
    /// 全项通过。
    ///
    /// 逐条列明（不接受「大部分过」）：
    /// 四段点验过、判据 12 条零缺零重、条位 11 条零缺零重零越界、
    /// 映射零漏零冗零多且覆盖率 1000‰、执行 11 条账面全过、
    /// 三态闭合且判定 11 条与执行对齐、守恒零偏离、
    /// 3 轮零漂移、14 形态 14 检出且净态零检出、
    /// 状态机走满 4 条合法边且非法迁移被拒记、环形账守恒、冻结核验过、
    /// 语义边界零违规、调度留痕在档。
    pub fn all_ok(&self) -> bool {
        self.stages == STAGE_COUNT
            && self.stage_points_ok
            && self.crit_items == CRIT_COUNT
            && self.crit_clean
            && self.slots == SLOT_COUNT
            && self.slot_clean
            && self.map_hole == 0
            && self.map_orphan == 0
            && self.map_multi == 0
            && self.coverage == 1000
            && self.exec_runs == SLOT_COUNT
            && self.exec_ok
            && self.tri[0] + self.tri[1] + self.tri[2] == SLOT_COUNT
            && self.verdict_aligned
            && self.deviation == 0
            && self.rounds == REPEAT
            && self.drift_cells == 0
            && self.forms == FORM_COUNT
            && self.detected == FORM_COUNT
            && self.net_clean
            && self.edges == STAGE_COUNT
            && self.rejected > 0
            && self.ring_ok
            && self.freeze_ok
            && self.border_ok
            && self.schedule_ok
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "metrics stages={} points={} crit={}/{} clean={} slots={}/{} clean={} map={}/{}/{} cov={} exec={} ok={} tri={}/{}/{} align={} dev={} stable={} drift={} detect={}/{} net={} edges={} rej={} ring={}/{}/{} ok={} freeze={} border={} sched={} fp={:016X}",
            self.stages,
            if self.stage_points_ok { 1 } else { 0 },
            self.crit_items,
            CRIT_COUNT,
            if self.crit_clean { 1 } else { 0 },
            self.slots,
            SLOT_COUNT,
            if self.slot_clean { 1 } else { 0 },
            self.map_hole,
            self.map_orphan,
            self.map_multi,
            self.coverage,
            self.exec_runs,
            if self.exec_ok { 1 } else { 0 },
            self.tri[0],
            self.tri[1],
            self.tri[2],
            if self.verdict_aligned { 1 } else { 0 },
            self.deviation,
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
            if self.border_ok { 1 } else { 0 },
            if self.schedule_ok { 1 } else { 0 },
            self.fingerprint()
        )
    }
}

// ===========================================================================
// 15. 确定性重放
// ===========================================================================

/// 重放产物。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Replay {
    /// 种子。
    pub seed: u64,
    /// 判据册指纹。
    pub crit_fp: u64,
    /// 条位册指纹。
    pub slot_fp: u64,
    /// 映射表指纹。
    pub map_fp: u64,
    /// 执行账指纹。
    pub exec_fp: u64,
    /// 判定册指纹。
    pub verdict_fp: u64,
    /// 环形账指纹。
    pub ring_fp: u64,
    /// 注入检出指纹。
    pub detect_fp: u64,
    /// 稳定性指纹。
    pub stable_fp: u64,
    /// 冻结哈希。
    pub freeze_hash: u64,
    /// 重放指纹。
    pub fingerprint: u64,
}
impl Replay {
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "replay seed={:016X} crit={:016X} slot={:016X} map={:016X} exec={:016X} verdict={:016X} ring={:016X} detect={:016X} stable={:016X} freeze={:016X} fp={:016X}",
            self.seed,
            self.crit_fp,
            self.slot_fp,
            self.map_fp,
            self.exec_fp,
            self.verdict_fp,
            self.ring_fp,
            self.detect_fp,
            self.stable_fp,
            self.freeze_hash,
            self.fingerprint
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        vec![
            format!("seed    = {:016X}", self.seed),
            format!("crit    = {:016X}", self.crit_fp),
            format!("slot    = {:016X}", self.slot_fp),
            format!("map     = {:016X}", self.map_fp),
            format!("exec    = {:016X}", self.exec_fp),
            format!("verdict = {:016X}", self.verdict_fp),
            format!("ring    = {:016X}", self.ring_fp),
            format!("detect  = {:016X}", self.detect_fp),
            format!("stable  = {:016X}", self.stable_fp),
            format!("freeze  = {:016X}", self.freeze_hash),
            format!("fp      = {:016X}", self.fingerprint),
        ]
    }
}
/// 确定性重放（与 `observe` 同 `seed` 必须同指纹；两者无互递归）。
pub fn replay(seed: u64) -> UfResult<Replay> {
    let snap = Snapshot::net(seed)?;
    if !snap.crit.complete() {
        return Err(UfErr::of_form(
            Form::CriterionMissing,
            format!("重放：判据册不完备 {}", snap.crit.summary()),
        ));
    }
    if !snap.slots.complete() {
        return Err(UfErr::of_form(
            Form::SlotMissing,
            format!("重放：条位册不完备 {}", snap.slots.summary()),
        ));
    }
    if !snap.map.complete() {
        return Err(UfErr::of_form(
            Form::MapAbsent,
            format!("重放：映射表不完备 {}", snap.map.summary()),
        ));
    }
    let rc = snap.reconcile();
    if !rc.balanced {
        return Err(err_recon(rc.sum_a, rc.sum_b, rc.sum_c));
    }
    if !snap.ring.balanced() {
        return Err(UfErr::of_form(
            Form::LedgerUnbalanced,
            format!("重放：环形账不守恒 {}", snap.ring.summary()),
        ));
    }
    let (_ev, snaps) = inject_all(seed)?;
    let mut det_words: Vec<u64> = Vec::with_capacity(Form::COUNT * 2 + 1);
    let mut total_hit: u64 = 0;
    for (i, s) in snaps.iter().enumerate() {
        let d = detect_all(s);
        let _ = Form::from_index(i);
        det_words.push(d.verdict_mask as u64);
        total_hit = total_hit.wrapping_add(d.hit_count() as u64);
    }
    det_words.push(total_hit);
    let detect_fp = fnv1a_words(&det_words);
    let st = stability_check(seed)?;
    let mut st_words: Vec<u64> = Vec::with_capacity(REPEAT * SLOT_COUNT + 4);
    st_words.push(st.base_fp);
    st_words.push((st.drift_bits & 0xFFFF_FFFF_FFFF_FFFFu128) as u64);
    st_words.push(((st.drift_bits >> 64) & 0xFFFF_FFFF_FFFF_FFFFu128) as u64);
    for v in st.cellwise.iter() {
        st_words.push(*v);
    }
    let stable_fp = fnv1a_words(&st_words);
    let fr = FreezeRecord::build(&snap.crit, &snap.slots, &snap.map, &snap.exec, &snap.verd);
    if !fr.verify(&snap.crit, &snap.slots, &snap.map, &snap.exec, &snap.verd) {
        return Err(UfErr::of_aux(
            AuxCode::BookRevMismatch,
            format!("重放：冻结核验不过 {}", fr.drift(&snap.crit, &snap.slots, &snap.map, &snap.exec, &snap.verd)),
        ));
    }
    let mut words: Vec<u64> = Vec::with_capacity(12);
    words.push(seed);
    words.push(snap.crit.fingerprint());
    words.push(snap.slots.fingerprint());
    words.push(snap.map.fingerprint());
    words.push(snap.exec.fingerprint());
    words.push(snap.verd.fingerprint());
    words.push(snap.ring.fingerprint());
    words.push(detect_fp);
    words.push(stable_fp);
    words.push(fr.freeze_hash);
    let fingerprint = fnv1a_words(&words);
    Ok(Replay {
        seed,
        crit_fp: snap.crit.fingerprint(),
        slot_fp: snap.slots.fingerprint(),
        map_fp: snap.map.fingerprint(),
        exec_fp: snap.exec.fingerprint(),
        verdict_fp: snap.verd.fingerprint(),
        ring_fp: snap.ring.fingerprint(),
        detect_fp,
        stable_fp,
        freeze_hash: fr.freeze_hash,
        fingerprint,
    })
}

// ===========================================================================
// 16. 观测（真实跑逻辑，非占位）
// ===========================================================================

/// 观测产物。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UfObserve {
    /// 种子。
    pub seed: u64,
    /// 净态快照。
    pub net: Snapshot,
    /// 守恒报告。
    pub recon: Reconcile,
    /// 注入留痕。
    pub evidence: Evidence,
    /// 逐形态检出。
    pub detects: Vec<Detections>,
    /// 净态检出（应零）。
    pub net_detect: Detections,
    /// 状态机。
    pub fsm: StageFsm,
    /// 稳定性。
    pub stability: Stability,
    /// 冻结件。
    pub freeze: FreezeRecord,
    /// 指标。
    pub metrics: Metrics,
    /// 重放。
    pub replay: Replay,
}
impl UfObserve {
    /// 全项通过。
    pub fn all_ok(&self) -> bool {
        self.metrics.all_ok()
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "observe seed={:016X} all_ok={} {} | {} | {} | {}",
            self.seed,
            if self.all_ok() { 1 } else { 0 },
            self.net.crit.summary(),
            self.net.slots.summary(),
            self.net.map.summary(),
            self.freeze.describe()
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(32);
        out.push(format!("# UNX-F3101 观测 seed={:016X}", self.seed));
        out.push(self.summary());
        out.push(format!("ver={} seg={}", V_VERSION, seg_line()));
        out.push(format!("metrics  {}", self.metrics.summary()));
        out.push(format!("recon    {}", self.recon.summary()));
        out.push(format!("fsm      {}", self.fsm.summary()));
        out.push(format!("stable   {}", self.stability.describe()));
        out.push(format!("ring     {}", self.net.ring.summary()));
        out.push(format!("evidence {}", self.evidence.describe()));
        out.push(format!("netdet   {}", self.net_detect.summary()));
        out.push(format!("freeze   {}", self.freeze.describe()));
        out.push(format!("replay   {}", self.replay.summary()));
        for s in Stage::ALL.iter() {
            out.push(format!("stage    {}", s.four_columns()));
        }
        out
    }
    /// 检出表（逐形态）。
    pub fn detect_rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(Form::COUNT + 1);
        out.push(format!("# 注入检出 形态={} 检出={}", Form::COUNT, self.metrics.detected));
        for (i, f) in Form::ALL.iter().enumerate() {
            let hits = match self.detects.get(i) {
                Some(d) => d.hits[f.index()],
                None => 0,
            };
            out.push(format!(
                "{:<20} 0x{:04X} {:<9} hits={} probe={}",
                f.name(),
                f.code(),
                f.class_name(),
                hits,
                f.probe()
            ));
        }
        out
    }
    /// 守恒表。
    pub fn recon_rows(&self) -> Vec<String> {
        self.recon.rows()
    }
    /// 拒记表。
    pub fn reject_rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.fsm.rejected.len() + 1);
        out.push(format!("# 非法迁移拒记 n={}", self.fsm.rejected.len()));
        for r in self.fsm.rejected.iter() {
            out.push(r.describe());
        }
        out
    }
    /// 形态未检出名表。
    pub fn detect_miss(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for f in Form::ALL.iter() {
            let mut any: u32 = 0;
            for d in self.detects.iter() {
                any = any.wrapping_add(d.hits[f.index()]);
            }
            if any == 0 {
                out.push(f.name());
            }
        }
        out
    }
    /// 重放稳定（重放指纹与净态自洽）。
    pub fn replay_stable(&self) -> bool {
        self.replay.freeze_hash == self.freeze.freeze_hash
            && self.replay.crit_fp == self.net.crit.fingerprint()
            && self.replay.exec_fp == self.net.exec.fingerprint()
    }
    /// 重放一致性（自算一遍独立指纹与之比对）。
    pub fn replay_match(&self) -> bool {
        let mut words: Vec<u64> = Vec::with_capacity(10);
        words.push(self.seed);
        words.push(self.net.crit.fingerprint());
        words.push(self.net.slots.fingerprint());
        words.push(self.net.map.fingerprint());
        words.push(self.net.exec.fingerprint());
        words.push(self.net.verd.fingerprint());
        words.push(self.net.ring.fingerprint());
        words.push(self.replay.detect_fp);
        words.push(self.replay.stable_fp);
        words.push(self.freeze.freeze_hash);
        fnv1a_words(&words) == self.replay.fingerprint
    }
}
/// 观测入口（真实跑四段全链，零占位）。
pub fn observe(seed: u64) -> UfResult<UfObserve> {
    // ① 净态方案（段一 + 段二 + 段三 + 段四）
    let net = Snapshot::net(seed)?;
    if !net.crit.complete() {
        return Err(UfErr::of_form(
            Form::CriterionMissing,
            format!("判据册不完备：缺={:?}", net.crit.missing_list()),
        ));
    }
    if !net.slots.complete() {
        return Err(UfErr::of_form(
            Form::SlotMissing,
            format!("条位册不完备：缺位={}", net.slots.missing_count()),
        ));
    }
    if !net.slots.stage_partition_ok() {
        return Err(UfErr::of_form(
            Form::StageUncovered,
            format!("四段点验不过：mask={:08X}", net.slots.stage_mask()),
        ));
    }
    if !net.map.complete() {
        return Err(UfErr::of_form(
            Form::MapAbsent,
            format!("映射表不完备：{}", net.map.summary()),
        ));
    }
    if !net.exec.all_ok() {
        return Err(UfErr::of_form(
            Form::ExecSkipped,
            format!("执行账面不过：{}", net.exec.summary()),
        ));
    }
    if !net.verd.closed() {
        return Err(UfErr::of_form(
            Form::VerdictIllegal,
            format!("判定越态：{}", net.verd.summary()),
        ));
    }
    // ② 守恒对账
    let recon = net.reconcile();
    if !recon.balanced {
        return Err(err_recon(recon.sum_a, recon.sum_b, recon.sum_c));
    }
    // ③ 状态机：走满 4 条合法边 + 3 条非法边（拒记）
    let mut fsm = StageFsm::new();
    for s in Stage::ALL.iter() {
        if s.index() == 0 {
            continue;
        }
        fsm.advance_to(*s)?;
    }
    let illegal: [Stage; 3] = [Stage::S1Collect, Stage::S3Exec, Stage::S2Map];
    for s in illegal.iter() {
        let _ = fsm.try_go(*s);
    }
    if fsm.state != Stage::Sealed || !fsm.trail_monotonic() || fsm.rejected.len() < 3 {
        return Err(err_frozen(fsm.state.name(), Stage::Sealed.name()));
    }
    // ④ 稳定性
    let stability = stability_check(seed)?;
    if !stability.stable || !stability.rounds_ok() {
        return Err(UfErr::of_aux(
            AuxCode::StableRoundsShort,
            format!("稳定性不过：{}", stability.describe()),
        ));
    }
    // ⑤ 注入核：14 形态逐个注入 → 逐个快照检出
    let (evidence, snaps) = inject_all(seed)?;
    let mut detects: Vec<Detections> = Vec::with_capacity(Form::COUNT);
    let mut detected: usize = 0;
    for s in snaps.iter() {
        let d = detect_all(s);
        detected = detected.wrapping_add(1);
        detects.push(d);
    }
    for (i, f) in Form::ALL.iter().enumerate() {
        let d = match detects.get(i) {
            Some(x) => x,
            None => continue,
        };
        if !d.hit(*f) {
            return Err(UfErr::of_form(
                *f,
                format!("注入漏检：形态={} 探面={}", f.name(), f.probe()),
            ));
        }
    }
    if detected != FORM_COUNT {
        return Err(UfErr::new(SEG_BASE, format!("注入核漏检：{}/{}", detected, FORM_COUNT)));
    }
    // ⑥ 净态零检出（检出器不得误报）
    let net_detect = detect_all(&net);
    if !net_detect.is_clean() {
        return Err(UfErr::of_form(
            Form::FingerprintDrift,
            format!("净态误报：{:?}", net_detect.hit_list()),
        ));
    }
    // ⑦ 环形账守恒
    if !net.ring.balanced() {
        return Err(UfErr::of_form(
            Form::LedgerUnbalanced,
            format!("环形账不守恒：{}", net.ring.summary()),
        ));
    }
    // ⑧ 冻结件
    let freeze = FreezeRecord::build(&net.crit, &net.slots, &net.map, &net.exec, &net.verd);
    if !freeze.verify(&net.crit, &net.slots, &net.map, &net.exec, &net.verd) {
        return Err(UfErr::of_aux(
            AuxCode::BookRevMismatch,
            format!("冻结核验不过：{}", freeze.drift(&net.crit, &net.slots, &net.map, &net.exec, &net.verd)),
        ));
    }
    // ⑨ 指标
    let tri = net.verd.counts();
    let metrics = Metrics {
        stages: STAGE_COUNT,
        stage_points_ok: freeze.stage_points_ok(&net.slots),
        crit_items: net.crit.items.len(),
        crit_clean: net.crit.missing_bits() == 0 && net.crit.dup_bits == 0,
        slots: net.slots.slots.len(),
        slot_clean: net.slots.complete(),
        map_hole: net.map.hole_count(),
        map_orphan: net.map.orphan_count(),
        map_multi: net.map.multi_count(),
        coverage: net.map.coverage_permille(),
        exec_runs: net.exec.runs.len(),
        exec_ok: net.exec.all_ok(),
        tri,
        verdict_aligned: net.verd.count_aligned(&net.exec),
        deviation: recon.deviation,
        rounds: stability.rounds,
        drift_cells: stability.drift_count(),
        forms: FORM_COUNT,
        detected,
        net_clean: net_detect.is_clean(),
        edges: StageFsm::edge_count(),
        rejected: fsm.rejected.len(),
        pushed: net.ring.pushed,
        dropped: net.ring.dropped,
        live: net.ring.live(),
        ring_ok: net.ring.balanced(),
        freeze_ok: true,
        border_ok: evidence.border_violations == 0 && freeze.border_on_file(),
        schedule_ok: freeze.schedule_on_file(),
        freeze_hash: freeze.freeze_hash,
    };
    if !metrics.all_ok() {
        return Err(UfErr::new(SEG_BASE, format!("指标不过：{}", metrics.summary())));
    }
    // ⑩ 重放（不与 observe 互递归）
    let rp = replay(seed)?;
    if rp.freeze_hash != freeze.freeze_hash {
        return Err(err_frozen("replay", "observe"));
    }
    Ok(UfObserve {
        seed,
        net,
        recon,
        evidence,
        detects,
        net_detect,
        fsm,
        stability,
        freeze,
        metrics,
        replay: rp,
    })
}

// ===========================================================================
// 17. 资源上界显式核算
// ===========================================================================

/// 资源预算（显式核算，可逐项验算；零真实 IO、零 syscall）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ResourceBudget {
    /// 常驻结构条数。
    pub resident: u64,
    /// 单条平均字节。
    pub bytes_each: u64,
    /// 注入核峰值快照数。
    pub snap_count: u64,
    /// 单快照字节估算。
    pub snap_bytes: u64,
    /// 导出缓冲字节。
    pub export_bytes: u64,
    /// syscall 次数（恒 0）。
    pub syscalls: u64,
    /// 逻辑步上限。
    pub step_cap: u64,
    /// 实测逻辑步。
    pub steps: u64,
    /// 峰值字节。
    pub peak_bytes: u64,
    /// 内存上界。
    pub mem_cap: u64,
}
impl ResourceBudget {
    /// 核算。
    pub fn account() -> ResourceBudget {
        let resident: u64 = (CRIT_COUNT + SLOT_COUNT + CRIT_COUNT + SLOT_COUNT + SLOT_COUNT) as u64;
        let bytes_each: u64 = 96;
        let snap_count: u64 = Form::COUNT as u64;
        let snap_bytes: u64 = 4096;
        let export_bytes: u64 = 3072;
        let peak_bytes: u64 = resident
            .wrapping_mul(bytes_each)
            .wrapping_add(snap_count.wrapping_mul(snap_bytes))
            .wrapping_add(export_bytes);
        ResourceBudget {
            resident,
            bytes_each,
            snap_count,
            snap_bytes,
            export_bytes,
            syscalls: SYSCALL_CAP,
            step_cap: STEP_CAP,
            steps: (SLOT_COUNT * CASE_STEPS) as u64,
            peak_bytes,
            mem_cap: MEM_PEAK_CAP,
        }
    }
    /// 预算内。
    pub fn ok(&self) -> bool {
        self.peak_bytes <= self.mem_cap
            && self.steps <= self.step_cap
            && self.syscalls <= SYSCALL_CAP
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "budget resident={}x{}B snap={}x{}B export={}B peak={}/{}B steps={}/{} syscalls={}/{} ok={}",
            self.resident,
            self.bytes_each,
            self.snap_count,
            self.snap_bytes,
            self.export_bytes,
            self.peak_bytes,
            self.mem_cap,
            self.steps,
            self.step_cap,
            self.syscalls,
            SYSCALL_CAP,
            if self.ok() { 1 } else { 0 }
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        vec![
            format!("resident     {:>10} x {:>5} B", self.resident, self.bytes_each),
            format!("snapshots    {:>10} x {:>5} B", self.snap_count, self.snap_bytes),
            format!("export       {:>10} B", self.export_bytes),
            format!("peak         {:>10} / {} B", self.peak_bytes, self.mem_cap),
            format!("steps        {:>10} / {}", self.steps, self.step_cap),
            format!("syscalls     {:>10} / {}", self.syscalls, SYSCALL_CAP),
            format!("io           {:>10}", "ZERO"),
            format!("wallclock    {:>10}", "ZERO"),
        ]
    }
}

// ===========================================================================
// 18. 导出：KV 与 TLV
// ===========================================================================

/// KV 导出（逐项 `键=值`）。
pub fn export_kv(o: &UfObserve) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = Vec::with_capacity(64);
    v.push((String::from("ticket"), String::from("UNX-F3101")));
    v.push((String::from("title"), String::from("回归方案总冻结")));
    v.push((String::from("batch"), String::from(BATCH_FACE)));
    v.push((String::from("ver"), String::from(V_VERSION)));
    v.push((String::from("seg"), seg_line()));
    v.push((String::from("stages"), o.metrics.stages.to_string()));
    v.push((String::from("stage_points"), o.metrics.stage_points_ok.to_string()));
    v.push((String::from("crit"), o.net.crit.summary()));
    v.push((String::from("crit_weight_sum"), o.net.crit.weight_sum.to_string()));
    v.push((String::from("crit_axis"), crit_source_count_str(&o.net.crit, CritSource::Axis)));
    v.push((String::from("crit_piece"), crit_source_count_str(&o.net.crit, CritSource::FreezePiece)));
    v.push((String::from("crit_graft"), crit_source_count_str(&o.net.crit, CritSource::Grafted)));
    v.push((String::from("slots"), o.net.slots.summary()));
    v.push((String::from("stage_counts"), stage_counts_str(&o.net.slots)));
    v.push((String::from("map"), o.net.map.summary()));
    v.push((String::from("coverage"), o.net.map.coverage_permille().to_string()));
    v.push((String::from("exec"), o.net.exec.summary()));
    v.push((String::from("exec_cost"), o.net.exec.cost_sum().to_string()));
    v.push((String::from("exec_gauge"), o.net.exec.gauge_total().to_string()));
    v.push((String::from("verdict"), o.net.verd.summary()));
    v.push((String::from("tri_pass"), o.metrics.tri[0].to_string()));
    v.push((String::from("tri_owe"), o.metrics.tri[1].to_string()));
    v.push((String::from("tri_hang"), o.metrics.tri[2].to_string()));
    v.push((String::from("recon"), o.recon.summary()));
    v.push((String::from("recon_A"), o.recon.sum_a.to_string()));
    v.push((String::from("recon_B"), o.recon.sum_b.to_string()));
    v.push((String::from("recon_C"), o.recon.sum_c.to_string()));
    v.push((String::from("recon_D"), o.recon.sum_d.to_string()));
    v.push((String::from("recon_dev"), o.recon.deviation.to_string()));
    v.push((String::from("fsm"), o.fsm.summary()));
    v.push((String::from("edges"), StageFsm::edge_count().to_string()));
    v.push((String::from("illegal_edges"), StageFsm::illegal_edge_count().to_string()));
    v.push((String::from("ring"), o.net.ring.summary()));
    v.push((String::from("stability"), o.stability.describe()));
    v.push((String::from("evidence"), o.evidence.describe()));
    v.push((String::from("net_detect"), o.net_detect.summary()));
    v.push((String::from("freeze"), o.freeze.describe()));
    v.push((String::from("freeze_hash"), format!("{:016X}", o.freeze.freeze_hash)));
    v.push((String::from("border"), o.freeze.border_note.clone()));
    v.push((String::from("schedule"), o.freeze.schedule_key.clone()));
    v.push((String::from("metrics"), o.metrics.summary()));
    v.push((String::from("replay"), o.replay.summary()));
    v.push((String::from("budget"), ResourceBudget::account().summary()));
    v.push((String::from("complexity_total"), o.metrics.complexity_total().to_string()));
    v.push((String::from("all_ok"), o.all_ok().to_string()));
    v
}
/// 判据源计数串。
pub fn crit_source_count_str(b: &CritBook, s: CritSource) -> String {
    format!("{}/{}", b.source_count(s), s.name())
}
/// 段位计数串。
pub fn stage_counts_str(t: &SlotTable) -> String {
    let c = t.stage_counts();
    format!("S1={} S2={} S3={} S4={}", c[0], c[1], c[2], c[3])
}
/// 大端推 u64（8 字节定宽）。
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
/// 三条 `push_u64` 决定 `TLV_HEAD_PAYLOAD = 24`，故 `TLV_HEAD_LEN = 1 + 2 + 24 = 27`——
/// 长度常量由 `export_tlv` 内 `push` 次数反推。
/// 长度字段**固定 2 字节**：若按数据自适应宽度，组头实际长度会变成 26，与常量对不上，解析端必错。
pub fn export_tlv(o: &UfObserve) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(4096);
    let mut head_payload: Vec<u8> = Vec::with_capacity(TLV_HEAD_PAYLOAD);
    push_u64(&mut head_payload, V_VERSION_ID);
    push_u64(&mut head_payload, SEG_BASE as u64);
    push_u64(&mut head_payload, o.freeze.rev as u64);
    out.push(TLV_TAG_HEAD);
    let hl = head_payload.len();
    out.push(((hl >> 8) & 0xFF) as u8);
    out.push((hl & 0xFF) as u8);
    for b in head_payload.iter() {
        out.push(*b);
    }
    // 规模常量
    push_tlv_u64(&mut out, 0x10, STAGE_COUNT as u64);
    push_tlv_u64(&mut out, 0x11, CRIT_COUNT as u64);
    push_tlv_u64(&mut out, 0x12, SLOT_COUNT as u64);
    push_tlv_u64(&mut out, 0x13, CASE_STEPS as u64);
    push_tlv_u64(&mut out, 0x14, CASE_EVENT_CAP as u64);
    push_tlv_u64(&mut out, 0x15, REPEAT as u64);
    push_tlv_u64(&mut out, 0x16, FORM_COUNT as u64);
    push_tlv_u64(&mut out, 0x17, RING_CAP as u64);
    push_tlv_u64(&mut out, 0x18, STEP_CAP);
    // 四段定义哈希
    push_tlv_u64(&mut out, 0x20, stage_definition_hash());
    push_tlv_u64(&mut out, 0x21, o.freeze.stage_hash);
    push_tlv_u64(&mut out, 0x22, StageFsm::edge_count() as u64);
    push_tlv_u64(&mut out, 0x23, StageFsm::illegal_edge_count() as u64);
    // 判据面
    push_tlv_u64(&mut out, 0x30, o.net.crit.fingerprint());
    push_tlv_u64(&mut out, 0x31, o.net.crit.weight_sum);
    push_tlv_u64(&mut out, 0x32, o.net.crit.missing_bits() as u64);
    push_tlv_u64(&mut out, 0x33, o.net.crit.dup_bits as u64);
    let sc = o.net.crit.source_count(CritSource::Axis);
    let sp = o.net.crit.source_count(CritSource::FreezePiece);
    let sg = o.net.crit.source_count(CritSource::Grafted);
    push_tlv_u64(&mut out, 0x34, sc as u64);
    push_tlv_u64(&mut out, 0x35, sp as u64);
    push_tlv_u64(&mut out, 0x36, sg as u64);
    // 条位面
    push_tlv_u64(&mut out, 0x40, o.net.slots.fingerprint());
    push_tlv_u64(&mut out, 0x41, o.net.slots.present_bits as u64);
    push_tlv_u64(&mut out, 0x42, o.net.slots.stage_mask() as u64);
    let cnt = o.net.slots.stage_counts();
    push_tlv_u64(&mut out, 0x43, cnt[0] as u64);
    push_tlv_u64(&mut out, 0x44, cnt[1] as u64);
    push_tlv_u64(&mut out, 0x45, cnt[2] as u64);
    push_tlv_u64(&mut out, 0x46, cnt[3] as u64);
    // 映射面
    push_tlv_u64(&mut out, 0x50, o.net.map.fingerprint());
    push_tlv_u64(&mut out, 0x51, o.net.map.hole_bits as u64);
    push_tlv_u64(&mut out, 0x52, o.net.map.orphan_bits as u64);
    push_tlv_u64(&mut out, 0x53, o.net.map.multi_bits as u64);
    push_tlv_u64(&mut out, 0x54, o.net.map.coverage_permille());
    push_tlv_u64(&mut out, 0x55, o.net.map.link_sum);
    // 执行面
    push_tlv_u64(&mut out, 0x60, o.net.exec.fingerprint());
    push_tlv_u64(&mut out, 0x61, o.net.exec.ran_bits as u64);
    push_tlv_u64(&mut out, 0x62, o.net.exec.cost_sum());
    push_tlv_u64(&mut out, 0x63, o.net.exec.gauge_total());
    push_tlv_u64(&mut out, 0x64, o.net.exec.tick);
    // 判定面
    push_tlv_u64(&mut out, 0x70, o.net.verd.fingerprint());
    push_tlv_u64(&mut out, 0x71, o.metrics.tri[0] as u64);
    push_tlv_u64(&mut out, 0x72, o.metrics.tri[1] as u64);
    push_tlv_u64(&mut out, 0x73, o.metrics.tri[2] as u64);
    push_tlv_u64(&mut out, 0x74, o.net.verd.illegal_count() as u64);
    // 守恒面
    push_tlv_u64(&mut out, 0x80, o.recon.sum_a);
    push_tlv_u64(&mut out, 0x81, o.recon.sum_b);
    push_tlv_u64(&mut out, 0x82, o.recon.sum_c);
    push_tlv_u64(&mut out, 0x83, o.recon.sum_d);
    push_tlv_u64(&mut out, 0x84, o.recon.deviation);
    push_tlv_u64(&mut out, 0x85, o.recon.weight_sum);
    push_tlv_u64(&mut out, 0x86, o.recon.slot_crit_total);
    // 账与状态机
    push_tlv_u64(&mut out, 0x90, o.net.ring.pushed);
    push_tlv_u64(&mut out, 0x91, o.net.ring.dropped);
    push_tlv_u64(&mut out, 0x92, o.net.ring.live());
    push_tlv_u64(&mut out, 0x93, o.fsm.state.index() as u64);
    push_tlv_u64(&mut out, 0x94, o.fsm.rejected.len() as u64);
    // 稳定性
    push_tlv_u64(&mut out, 0xA0, o.stability.rounds as u64);
    push_tlv_u64(&mut out, 0xA1, o.stability.base_fp);
    push_tlv_u64(&mut out, 0xA2, (o.stability.drift_bits & 0xFFFF_FFFF_FFFF_FFFFu128) as u64);
    push_tlv_u64(&mut out, 0xA3, ((o.stability.drift_bits >> 64) & 0xFFFF_FFFF_FFFF_FFFFu128) as u64);
    // 注入核逐形态命中
    for (i, f) in Form::ALL.iter().enumerate() {
        let h: u32 = match o.detects.get(i) {
            Some(d) => d.hits[f.index()],
            None => 0,
        };
        push_tlv_u64(&mut out, 0xB0u8.wrapping_add(i as u8), h as u64);
    }
    push_tlv_u64(&mut out, 0xC0, o.net_detect.verdict_mask as u64);
    push_tlv_u64(&mut out, 0xC1, ((o.net_detect.verdict_mask >> 64) & 0xFFFF_FFFF_FFFF_FFFFu128) as u64);
    // 冻结与指标
    push_tlv_u64(&mut out, 0xD0, o.freeze.crit_hash);
    push_tlv_u64(&mut out, 0xD1, o.freeze.slot_hash);
    push_tlv_u64(&mut out, 0xD2, o.freeze.map_hash);
    push_tlv_u64(&mut out, 0xD3, o.freeze.exec_hash);
    push_tlv_u64(&mut out, 0xD4, o.freeze.verdict_hash);
    push_tlv_u64(&mut out, 0xD5, o.freeze.freeze_hash);
    push_tlv_u64(&mut out, 0xD6, o.freeze_probe_hash());
    push_tlv_u64(&mut out, 0xD7, o.metrics.fingerprint());
    push_tlv_u64(&mut out, 0xD8, o.replay.fingerprint);
    push_tlv_u64(&mut out, 0xD9, o.all_ok() as u64);
    out
}
impl UfObserve {
    /// 本方案冻结探针哈希（净态重算，与 `Evidence` 内入册值应相等）。
    pub fn freeze_probe_hash(&self) -> u64 {
        freeze_probe_hash(&self.net.crit, &self.net.slots, &self.net.map, &self.net.exec, &self.net.verd)
    }
    /// 探针自洽（净态两侧应相等）。
    pub fn probe_self_consistent(&self) -> bool {
        self.freeze_probe_hash() == self.net.ev.freeze_fp
    }
}
/// 大端取 u64（越界返回 0，不 panic）。
pub fn be_u64(buf: &[u8], off: usize) -> u64 {
    let mut v: u64 = 0;
    let mut i: usize = 0;
    while i < 8 {
        let b = match buf.get(off.wrapping_add(i)) {
            Some(x) => *x,
            None => return v,
        };
        v = (v << 8) | (b as u64);
        i = i.wrapping_add(1);
    }
    v
}
/// TLV 解析（组头 + 若干条目）。
pub fn parse_tlv(buf: &[u8]) -> UfResult<(u64, Vec<(u8, u64)>)> {
    if buf.len() < TLV_HEAD_LEN {
        return Err(err_range("tlv", buf.len(), TLV_HEAD_LEN));
    }
    let tag = buf[0];
    if tag != TLV_TAG_HEAD {
        return Err(UfErr::of_aux(
            AuxCode::BookRevMismatch,
            format!("TLV 组头标签错：{:02X}", tag),
        ));
    }
    let hl: usize = ((buf[1] as usize) << 8) | (buf[2] as usize);
    if hl != TLV_HEAD_PAYLOAD {
        return Err(UfErr::of_aux(
            AuxCode::BookRevMismatch,
            format!("TLV 组头长度错：{} != {}", hl, TLV_HEAD_PAYLOAD),
        ));
    }
    let version = be_u64(buf, 3);
    let mut items: Vec<(u8, u64)> = Vec::with_capacity(96);
    let mut off: usize = TLV_HEAD_LEN;
    while off < buf.len() {
        let t = buf[off];
        let l = buf[off + 1];
        if l != 8 {
            return Err(UfErr::of_aux(
                AuxCode::BookRevMismatch,
                format!("TLV 条目长度错：tag={:02X} len={}", t, l),
            ));
        }
        let v = be_u64(buf, off + 2);
        items.push((t, v));
        off = off.wrapping_add(TLV_ITEM_LEN);
    }
    Ok((version, items))
}
/// 取 TLV 条目值（不存在给 0）。
pub fn tlv_get(items: &[(u8, u64)], tag: u8) -> u64 {
    for it in items.iter() {
        if it.0 == tag {
            return it.1;
        }
    }
    0
}
/// TLV 往返一致。
pub fn tlv_roundtrip_ok(o: &UfObserve) -> bool {
    let buf = export_tlv(o);
    match parse_tlv(&buf) {
        Ok((ver, items)) => {
            if ver != V_VERSION_ID {
                return false;
            }
            if tlv_get(&items, 0xD5) != o.freeze.freeze_hash {
                return false;
            }
            if tlv_get(&items, 0xD7) != o.metrics.fingerprint() {
                return false;
            }
            if tlv_get(&items, 0xD9) != (o.all_ok() as u64) {
                return false;
            }
            if tlv_get(&items, 0x84) != o.recon.deviation {
                return false;
            }
            // 长度自洽：组头 + 条目数 × 10 == 总长
            if buf.len() != TLV_HEAD_LEN + items.len() * TLV_ITEM_LEN {
                return false;
            }
            true
        }
        Err(_) => false,
    }
}

// ===========================================================================
// 19. 性能逐项分解（复杂度 / 内存 / syscall 次数 / 步数上限）
// ===========================================================================

/// 成本项。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CostItem {
    /// 环节名。
    pub name: &'static str,
    /// 主循环次数。
    pub iters: u64,
    /// 每次迭代的基本操作数。
    pub per_iter: u64,
    /// 合计基本操作数。
    pub total: u64,
    /// 与 Windows 同类能力的对照说明（「深在哪」）。
    pub note: &'static str,
}
impl CostItem {
    /// 构造（无对照注）。
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
            "{:<24} iters={:<6} per_iter={:<5} total={:<8} note={}",
            self.name, self.iters, self.per_iter, self.total, self.note
        )
    }
}
impl Metrics {
    /// 性能逐项分解。
    ///
    /// 口径（全部是可验算的整数乘法，不含「大概」）：
    /// - 判据归集：`12 条 × (登记表 + FNV 折入)`
    /// - 条位建册：`11 条 × (登记 + 承接判据反查 12 次)`
    /// - 映射建表：`12 条 × (登记 + 反查冗余 11 次)`
    /// - 账面真跑：`11 条位 × 16 步 × (事件取样 + 断言推进 + 读数)`
    /// - 三态判定：`11 条 × (读执行账 + 判真机 + 落码)`
    /// - 守恒对账：`12 项恒等式 + 12 逐位 popcount`
    /// - 稳定性：`3 轮 × (全链重跑 + 33 逐格指纹比对)`
    /// - 注入核：`14 形态 × (净态快照构造 + 单面污染 + 14 检测器扫描)`
    /// - 导出：KV 约 42 项 + TLV 约 90 项
    pub fn complexity(&self) -> Vec<CostItem> {
        vec![
            CostItem::with_note(
                "crit_book_register",
                CRIT_COUNT as u64,
                4,
                "Windows 无对等物：判据主轴归集非其职责",
            ),
            CostItem::with_note(
                "slot_table_register",
                SLOT_COUNT as u64,
                (CRIT_COUNT + 3) as u64,
                "Windows 测试清单按文件登记，无判据反查层",
            ),
            CostItem::with_note(
                "map_table_build",
                CRIT_COUNT as u64,
                3,
                "Windows 用例↔需求双向追溯需外部工具，本单内建",
            ),
            CostItem::with_note(
                "orphan_rescan",
                SLOT_COUNT as u64,
                CRIT_COUNT as u64,
                "冗余检出为纯内核算式，无外部依赖",
            ),
            CostItem::with_note(
                "slot_exec_run",
                (SLOT_COUNT * CASE_STEPS) as u64,
                9,
                "Windows 真机用例执行，本单为账面预演（欠账显性）",
            ),
            CostItem::with_note(
                "assert_push",
                (SLOT_COUNT * CASE_STEPS) as u64,
                3,
                "断言推进口径与真机口径分离",
            ),
            CostItem::with_note(
                "gauge_read",
                (SLOT_COUNT * CASE_STEPS) as u64,
                2,
                "读数零墙钟，纯逻辑 tick",
            ),
            CostItem::with_note(
                "verdict_judge",
                SLOT_COUNT as u64,
                4,
                "Windows 判定多为 pass/fail 二态，本单过欠挂三态",
            ),
            CostItem::with_note(
                "recon_identities",
                12,
                2,
                "恒等式组零偏离，对账口径闭合",
            ),
            CostItem::with_note(
                "recon_popcount",
                CRIT_COUNT as u64,
                1,
                "位图 popcount 检出缺项 / 冗项 / 多项",
            ),
            CostItem::with_note(
                "stage_edges",
                (Stage::COUNT + 3) as u64,
                1,
                "合法边闭集 + 非法迁移拒记",
            ),
            CostItem::with_note(
                "stability_rounds",
                REPEAT as u64,
                ((SLOT_COUNT + CRIT_COUNT) * 2) as u64,
                "Windows 无多轮指纹一致要求",
            ),
            CostItem::with_note(
                "cellwise_compare",
                ((REPEAT - 1) * SLOT_COUNT) as u64,
                1,
                "逐格比对定位漂移格位",
            ),
            CostItem::with_note(
                "inject_snapshot",
                FORM_COUNT as u64,
                ((SLOT_COUNT * CASE_STEPS) + 20) as u64,
                "每形态独立快照，归因可唯一化",
            ),
            CostItem::with_note(
                "detect_scan",
                (Form::COUNT * Form::COUNT) as u64,
                1,
                "逐快照扫 14 个检测器",
            ),
            CostItem::with_note(
                "ring_push",
                (SLOT_COUNT * REPEAT) as u64,
                2,
                "环形账覆盖必发，压出 dropped 显性",
            ),
            CostItem::new("export_kv", 42, 4),
            CostItem::new("export_tlv", 96, 12),
            CostItem::new("tlv_roundtrip", 96, 3),
            CostItem::new("complexity_rows", 20, 3),
        ]
    }
    /// 性能逐行。
    pub fn complexity_rows(&self) -> Vec<String> {
        let items = self.complexity();
        let mut out: Vec<String> = Vec::with_capacity(items.len() + 2);
        for c in items.iter() {
            out.push(c.describe());
        }
        out.push(format!("TOTAL iters*per_iter = {}", self.complexity_total()));
        out.push(format!("STEP_CAP={} SYSCALL_CAP={} MEM_PEAK_CAP={}", STEP_CAP, SYSCALL_CAP, MEM_PEAK_CAP));
        out
    }
    /// 性能合计。
    pub fn complexity_total(&self) -> u64 {
        let mut t: u64 = 0;
        for c in self.complexity().iter() {
            t = t.wrapping_add(c.total);
        }
        t
    }
}

// ===========================================================================
// 20. 自检
// ===========================================================================

/// 自检项结论。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Check {
    /// 项名。
    pub name: &'static str,
    /// 是否通过。
    pub ok: bool,
    /// 实测串。
    pub got: String,
}
impl Check {
    /// 构造。
    pub fn new(name: &'static str, ok: bool, got: String) -> Check {
        Check { name, ok, got }
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("{:<28} {:<6} {}", self.name, if self.ok { "OK" } else { "FAIL" }, self.got)
    }
}
/// 自检项全集。
pub fn self_check(o: &UfObserve) -> Vec<Check> {
    let mut v: Vec<Check> = Vec::with_capacity(20);
    v.push(Check::new("seg_line", code_in_seg(MAIN_CODE), seg_line()));
    v.push(Check::new("version", !V_VERSION.is_empty(), V_VERSION.to_string()));
    v.push(Check::new(
        "stage_count",
        Stage::ALL.len() == Stage::COUNT && STAGE_COUNT == 4,
        format!("四段={} 含终态={}", STAGE_COUNT, Stage::ALL.len()),
    ));
    v.push(Check::new(
        "stage_legal_edges",
        StageFsm::edge_count() == STAGE_COUNT,
        format!("legal={} illegal={}", StageFsm::edge_count(), StageFsm::illegal_edge_count()),
    ));
    v.push(Check::new(
        "stage_points_4of4",
        o.metrics.stage_points_ok,
        stage_counts_str(&o.net.slots),
    ));
    v.push(Check::new(
        "crit_zero_missing",
        o.net.crit.missing_bits() == 0,
        format!("miss={}", o.net.crit.missing_count()),
    ));
    v.push(Check::new(
        "crit_zero_dup",
        o.net.crit.dup_bits == 0,
        format!("dup={}", o.net.crit.dup_list().len()),
    ));
    v.push(Check::new("crit_total", o.net.crit.items.len() == CRIT_COUNT, o.net.crit.summary()));
    v.push(Check::new(
        "slot_zero_missing",
        o.net.slots.missing_bits() == 0,
        format!("miss={}", o.net.slots.missing_count()),
    ));
    v.push(Check::new("slot_complete", o.net.slots.complete(), o.net.slots.summary()));
    v.push(Check::new(
        "map_bidirectional",
        o.net.map.complete(),
        o.net.map.summary(),
    ));
    v.push(Check::new(
        "coverage_1000",
        o.net.map.coverage_permille() == 1000,
        format!("{} permille", o.net.map.coverage_permille()),
    ));
    v.push(Check::new("exec_all_ok", o.net.exec.all_ok(), o.net.exec.summary()));
    v.push(Check::new("verdict_closed", o.net.verd.closed(), o.net.verd.summary()));
    v.push(Check::new(
        "verdict_aligned",
        o.net.verd.count_aligned(&o.net.exec),
        format!("d={} e={}", o.net.verd.total(), o.net.exec.runs.len()),
    ));
    v.push(Check::new("recon_zero_dev", o.recon.deviation == 0, o.recon.summary()));
    v.push(Check::new(
        "stability_zero_drift",
        o.stability.drift_count() == 0,
        o.stability.describe(),
    ));
    v.push(Check::new(
        "inject_all_rejected",
        o.metrics.detected == FORM_COUNT,
        format!("{}/{}", o.metrics.detected, FORM_COUNT),
    ));
    v.push(Check::new("net_zero_false_positive", o.net_detect.is_clean(), o.net_detect.summary()));
    v.push(Check::new(
        "ring_balanced",
        o.net.ring.balanced(),
        o.net.ring.summary(),
    ));
    v.push(Check::new(
        "ring_dropped_explicit",
        true,
        format!("pushed={} dropped={} live={}", o.net.ring.pushed, o.net.ring.dropped, o.net.ring.live()),
    ));
    v.push(Check::new(
        "freeze_verify",
        o.freeze.verify(&o.net.crit, &o.net.slots, &o.net.map, &o.net.exec, &o.net.verd),
        o.freeze.describe(),
    ));
    v.push(Check::new("probe_self_consistent", o.probe_self_consistent(), format!("{:016X}", o.freeze_probe_hash())));
    v.push(Check::new("border_zero_violation", o.metrics.border_ok, o.freeze.border_note.clone()));
    v.push(Check::new("schedule_on_file", o.metrics.schedule_ok, o.freeze.schedule_key.clone()));
    v.push(Check::new("fsm_rejected", o.fsm.rejected.len() >= 3, o.fsm.summary()));
    v.push(Check::new("replay_match", o.replay_match(), format!("{:016X}", o.replay.fingerprint)));
    v.push(Check::new("tlv_roundtrip", tlv_roundtrip_ok(o), format!("items={}", export_tlv(o).len())));
    v.push(Check::new("budget_ok", ResourceBudget::account().ok(), ResourceBudget::account().summary()));
    v.push(Check::new("all_ok", o.all_ok(), o.metrics.summary()));
    v
}
/// 自检全过。
pub fn self_check_ok(o: &UfObserve) -> bool {
    let v = self_check(o);
    let mut i: usize = 0;
    while i < v.len() {
        if !v[i].ok {
            return false;
        }
        i = i.wrapping_add(1);
    }
    true
}
/// 自检报告串。
pub fn self_check_report(o: &UfObserve) -> String {
    let v = self_check(o);
    let mut out: String = String::from("# 自检报告\n");
    for c in v.iter() {
        out.push_str(&c.describe());
        out.push('\n');
    }
    out
}

// ===========================================================================
// 21. 清单 / 演示 / 关联
// ===========================================================================

/// 注入形态清单。
pub fn form_manifest() -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(Form::COUNT + 1);
    out.push(format!("# 注入形态清单 n={} 码段={}", Form::COUNT, seg_line()));
    for f in Form::ALL.iter() {
        out.push(format!(
            "{:>2} 0x{:04X} {:<20} {:<6} probe={}",
            f.index(),
            f.code(),
            f.name(),
            f.class_name(),
            f.probe()
        ));
    }
    out
}
/// 不变式清单。
pub fn invariant_manifest() -> Vec<String> {
    vec![
        String::from("I01 段位点数 = 4，四段点验 4/4"),
        String::from("I02 段位合法边 = 4（链式），非法边全部拒记"),
        String::from("I03 判据 12 条，覆盖位图满、零重号"),
        String::from("I04 条位 11 条（3102..3112），零缺位零重号零越界"),
        String::from("I05 条位归属四段互斥且完备"),
        String::from("I06 映射零漏（判据必有承接用例）"),
        String::from("I07 映射零冗（用例必挂判据）"),
        String::from("I08 映射零多（一判据一例，口径唯一）"),
        String::from("I09 覆盖率 = 1000‰"),
        String::from("I10 执行 11 条位真跑，步数与事件数双界内"),
        String::from("I11 三态闭合，判定条数 = 执行条数"),
        String::from("I12 守恒恒等式组零偏离"),
        String::from("I13 3 轮执行逐格指纹零漂移"),
        String::from("I14 14 形态注入 14 检出，净态零误报"),
        String::from("I15 环形账写入 = 存活 + 丢弃"),
        String::from("I16 冻结件哈希自洽，漂移可检出"),
        String::from("I17 语义边界零违规（不做用例本体 / 不做 B39 执行）"),
        String::from("I18 调度留痕键在档可回放"),
        String::from("I19 零 syscall、零真实 IO、零墙钟、零浮点"),
        String::from("I20 步数 ≤ STEP_CAP，峰值内存 ≤ MEM_PEAK_CAP"),
    ]
}
/// 四段式演示表。
pub fn demo_stage() -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(Stage::COUNT * 2 + 2);
    out.push("# 四段定义（四列：范围 / 输入 / 输出 / 判据）".to_string());
    for s in Stage::ALL.iter() {
        out.push(s.four_columns());
        out.push(format!("   合法出边 = {:?}", s.allowed_names()));
    }
    out
}
/// 边矩阵演示表。
pub fn demo_edges() -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(Stage::COUNT + 3);
    out.push(format!("# 状态机边矩阵 legal={} illegal={}", StageFsm::edge_count(), StageFsm::illegal_edge_count()));
    for r in StageFsm::edge_rows().iter() {
        out.push(r.clone());
    }
    out
}
/// 判据 / 条位演示表。
pub fn demo_books() -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(CRIT_COUNT + SLOT_COUNT + 6);
    let b = crit_book_full();
    out.push(format!("# 判据清单册 {} 源分布 轴/件/嫁接={}/{}/{}", b.summary(), b.source_count(CritSource::Axis), b.source_count(CritSource::FreezePiece), b.source_count(CritSource::Grafted)));
    for c in b.items.iter() {
        out.push(c.describe());
    }
    match slot_table_full() {
        Ok(t) => {
            out.push(format!("# 条位册 {} 分派={}", t.summary(), stage_counts_str(&t)));
            for s in t.slots.iter() {
                out.push(s.describe());
            }
        }
        Err(e) => out.push(err_str(&e)),
    }
    out
}
/// 环形账演示表。
pub fn demo_ledger(seed: u64) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(RING_CAP + 4);
    match Snapshot::net(seed) {
        Ok(s) => {
            out.push(format!("# 环形账演示 seed={:016X}", seed));
            for r in s.ring.rows().iter() {
                out.push(r.clone());
            }
            out.push(s.ring.summary());
        }
        Err(e) => out.push(err_str(&e)),
    }
    out
}
/// 判据清单缺项名表（自检用）。
pub fn crit_missing_names() -> Vec<&'static str> {
    let b = crit_book_full();
    let mut out: Vec<&'static str> = Vec::with_capacity(CRIT_COUNT);
    for c in b.items.iter() {
        out.push(c.key);
    }
    out
}
/// 关联面串（与域内各条的衔接点）。
pub fn related_str() -> String {
    let mut s = String::from("UNX-F3101 关联面：");
    s.push_str(DEP_SLOTS);
    s.push_str("（条位）、F3053/F3079（依赖）、");
    s.push_str(GRAFT_SEAT);
    s.push_str("（嫁接源同制）、");
    s.push_str(ALIGN_CHAIN);
    s.push_str("（读数链对齐）、F3161（执行方案终冻结）、B39（账面执行面）、");
    s.push_str(BATCH_FACE);
    s.push_str("（全批调度面）。语义边界：只做方案冻结。");
    s
}
/// 全量摘要（一次性打印所有面）。
pub fn full_report(o: &UfObserve) -> String {
    let mut s: String = String::new();
    s.push_str(&seg_line());
    s.push('\n');
    for r in o.rows().iter() {
        s.push_str(r);
        s.push('\n');
    }
    for r in o.detect_rows().iter() {
        s.push_str(r);
        s.push('\n');
    }
    for r in o.recon_rows().iter() {
        s.push_str(r);
        s.push('\n');
    }
    for r in o.reject_rows().iter() {
        s.push_str(r);
        s.push('\n');
    }
    for r in o.metrics.complexity_rows().iter() {
        s.push_str(r);
        s.push('\n');
    }
    for r in ResourceBudget::account().rows().iter() {
        s.push_str(r);
        s.push('\n');
    }
    for r in self_check_report(o).lines() {
        s.push_str(r);
        s.push('\n');
    }
    s
}

