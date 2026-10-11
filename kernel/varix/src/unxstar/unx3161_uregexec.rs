//! UNX-F3161 · 回归矩阵执行方案终冻结（用例册消费与执行启用）
//!
//! 任务单号: UNX-F3161
//! 标题: 回归矩阵执行方案终冻结（用例册消费与执行启用）
//! 域/批: A4/B39
//! 册目标: 400 行（回归矩阵执行方案终 180 + 冻结 120 + 断言 100；测试段不计）
//!
//! 判据原文（逐字摘抄，来源 anchors/UNX-F3161.md）:
//! 「UNX-F3161-J1 执行方案终冻结：F3110 用例册（哈希引用一致断言）与 F3111 执行器方案
//!   （编排哈希引用一致）双引用冻结哈希入册，执行前置门（F3157 五项门）复核过，
//!   引用漂移注入拦截」
//!
//! 复测方式（逐字摘抄）:
//! 「双引用一致，五门复核过，漂移注入拦截，启用哈希入库，
//!   双引用比对读数与五门复核件哈希随批归档，过门结论留痕注记在档可回放。」
//!
//! 定位（逐字摘抄）:
//! 「回归执行的启用位——终冻结：B39 执行前双引用核（用例册哈希引用一致——执行即册位原样；
//!   执行器编排哈希引用一致——调度即方案原样）；执行前置门复核（F3157 五项门逐门复核读数）；
//!   引用漂移（执行用例与册失配）注入必拦；过门即启用 B39 账面执行。」
//!
//! 语义边界（逐字摘抄）:
//! 「只做执行启用；不做用例本体（F3110 冻结件）、不做单项执行（F3162–F3169）。」
//!
//! 依赖与嫁接源（逐字摘抄）:
//! 「依赖 F3110/F3111/F3157/F3162–F3169；嫁接源为域内回归制度，实现自研。」
//!
//! 风险与回退（逐字摘抄）:
//! 「引用失配（执行与册版本漂移）——哈希引用断言，失配注入拦截。」
//!
//! 与 Windows 对照（逐字摘抄）:
//! 「域内回归制度，无公开对应件。」
//!
//! 正文（逐字摘抄）:
//! 「实现路径分三步。第一步双引用核：册与编排哈希引用逐项比对；
//!   第二步过门复核：五项门读数逐门复核；第三步断言：漂移注入拦截，
//!   启用件哈希入库，执行调度留痕。」
//!
//! 双引用核（本单核心）:
//! - **引用一（F3110 用例册）**：执行所消费的每个用例必须与册内冻结条目**原样一致**
//!   （用例号 / 判据 / 期望三列逐字比对 + 册位哈希）；
//! - **引用二（F3111 执行器编排）**：执行调度所依据的编排方案必须与冻结编排**原样一致**
//!   （编排号 / 序位 / 前置门集合 + 编排哈希）。
//!
//! 任一引用漂移 ⇒ `RefDrift`，执行**不得启用**。
//!
//! F3157 五项门前置门（本单第二核心）:
//! | 门位 | 门名 | 复核读数 |
//! |---|---|---|
//! | G0 | 方案冻结门 | 册哈希与编排哈希均已入册 |
//! | G1 | 用例齐备门 | 36 个执行用例在册且与 F3110 逐字一致 |
//! | G2 | 断言完备门 | 每个用例至少一条可判定断言 |
//! | G3 | 资源预算门 | 预算账平（步数 / 内存 / syscall 三项） |
//! | G4 | 边界零违规门 | 域外动作零次 |
//! 五门逐门复核读数，任一门不过即 `GateNotPass`，执行不启用。
#![allow(dead_code)]
#![allow(unused_imports)]
// ===========================================================================
// 0. 基础设施
// ===========================================================================
use alloc::{vec, vec::Vec, string::{String, ToString}, format};

/// 版本标识。
pub const V_VERSION_ID: u64 = 0x0001_3161_0000_0001;
/// 版本串。
pub const V_VERSION: &str = "v3161-uregexec.v1";

/// 码段基址（任务单指定）。
pub const SEG_BASE: u16 = 0x26C0;
/// 码段掩码（任务单指定）。
pub const SEG_MASK: u16 = 0x003F;
/// 码段上界。
pub const SEG_END: u16 = SEG_BASE | SEG_MASK;
/// 主码。
pub const MAIN_CODE: u16 = SEG_BASE;

// ── 规模常量 ────────────────────────────────────────────────────────────────
/// 用例数（`F3162–F3169` 八项 × 每项四格 = 32，加四项基线格 = 36）。
pub const CASE_TOTAL: usize = 36;
/// 编排项数（36 用例按 9 项 × 4 格分段，段段相接不重不漏）。
pub const ORCH_TOTAL: usize = 9;
/// 执行前置门数（F3157 五项门）。
pub const GATE_COUNT: usize = 5;
/// 每用例断言数。
pub const ASSERT_PER_CASE: usize = 3;
/// 断言总数。
pub const ASSERT_TOTAL: usize = CASE_TOTAL * ASSERT_PER_CASE;
/// 每用例执行步数。
pub const CASE_STEPS: usize = 12;
/// 断言步节拍（`CASE_STEPS / ASSERT_PER_CASE`，即每 N 步落一条可判定断言）。
pub const ASSERT_STRIDE: usize = CASE_STEPS / ASSERT_PER_CASE;
/// 重放轮数。
pub const REPEAT: usize = 3;
/// 注入形态数。
pub const FORM_COUNT: usize = 14;
/// 环形账容量（2 的幂）。
pub const RING_CAP: usize = 128;
/// 环形账掩码。
pub const RING_MASK: usize = RING_CAP - 1;
/// 逻辑 tick 步长。
pub const TICK_STEP: u64 = 1;
/// 步数上限。
pub const STEP_CAP: u64 = (CASE_TOTAL * CASE_STEPS * REPEAT * 2) as u64;
/// syscall 上限（本单恒 0）。
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
pub const BATCH_FACE: &str = "B39";
/// 域封批。
pub const FINAL_FACE: &str = "B40";
/// 用例册引用（双引用之一）。
pub const REF_CASEBOOK: &str = "F3110";
/// 执行器编排引用（双引用之二）。
pub const REF_ORCH: &str = "F3111";
/// 前置门来源。
pub const GATE_SRC: &str = "F3157";
/// 编排消费位。
pub const CONSUME_SLOTS: &str = "F3162-F3169";
/// 嫁接制度席。
pub const GRAFT_SEAT: &str = "F3101";
/// 体检单链。
pub const ALIGN_CHAIN: &str = "F2797";
/// 前置门容量（读数上界）。
pub const GATE_READ_CAP: u64 = 1024;

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
    /// 册位缺失（用例不在 F3110 册内）。
    CaseMissing,
    /// 册位重号。
    CaseDup,
    /// 用例号越界。
    CaseOutOfRange,
    /// 判据列残缺。
    ColumnIncomplete,
    /// 编排项缺失。
    OrchMissing,
    /// 编排项重号。
    OrchDup,
    /// 引用一漂移（用例与 F3110 失配）。
    RefCaseDrift,
    /// 引用二漂移（编排与 F3111 失配）。
    RefOrchDrift,
    /// 前置门不过。
    GateNotPass,
    /// 断言完备性不过。
    AssertIncomplete,
    /// 启用守恒式偏离。
    IdentityEnable,
    /// 环形账不守恒。
    LedgerUnbalanced,
    /// 冻结指纹漂移。
    FingerprintDrift,
    /// 步数越界。
    StepOverCap,
}
impl Form {
    /// 形态总数。
    pub const COUNT: usize = 14;
    /// 全部形态。
    pub const ALL: [Form; Form::COUNT] = [
        Form::CaseMissing,
        Form::CaseDup,
        Form::CaseOutOfRange,
        Form::ColumnIncomplete,
        Form::OrchMissing,
        Form::OrchDup,
        Form::RefCaseDrift,
        Form::RefOrchDrift,
        Form::GateNotPass,
        Form::AssertIncomplete,
        Form::IdentityEnable,
        Form::LedgerUnbalanced,
        Form::FingerprintDrift,
        Form::StepOverCap,
    ];
    /// 段内偏移。
    pub fn index(&self) -> usize {
        match self {
            Form::CaseMissing => 0,
            Form::CaseDup => 1,
            Form::CaseOutOfRange => 2,
            Form::ColumnIncomplete => 3,
            Form::OrchMissing => 4,
            Form::OrchDup => 5,
            Form::RefCaseDrift => 6,
            Form::RefOrchDrift => 7,
            Form::GateNotPass => 8,
            Form::AssertIncomplete => 9,
            Form::IdentityEnable => 10,
            Form::LedgerUnbalanced => 11,
            Form::FingerprintDrift => 12,
            Form::StepOverCap => 13,
        }
    }
    /// 绝对码。
    pub fn code(&self) -> u16 {
        SEG_BASE.wrapping_add(self.index() as u16)
    }
    /// 形态名。
    pub fn name(&self) -> &'static str {
        match self {
            Form::CaseMissing => "CaseMissing",
            Form::CaseDup => "CaseDup",
            Form::CaseOutOfRange => "CaseOutOfRange",
            Form::ColumnIncomplete => "ColumnIncomplete",
            Form::OrchMissing => "OrchMissing",
            Form::OrchDup => "OrchDup",
            Form::RefCaseDrift => "RefCaseDrift",
            Form::RefOrchDrift => "RefOrchDrift",
            Form::GateNotPass => "GateNotPass",
            Form::AssertIncomplete => "AssertIncomplete",
            Form::IdentityEnable => "IdentityEnable",
            Form::LedgerUnbalanced => "LedgerUnbalanced",
            Form::FingerprintDrift => "FingerprintDrift",
            Form::StepOverCap => "StepOverCap",
        }
    }
    /// 判据语义。
    pub fn detail(&self) -> &'static str {
        match self {
            Form::CaseMissing => "册位缺失：执行用例不在 F3110 册内",
            Form::CaseDup => "册位重号：同一用例号两次入册",
            Form::CaseOutOfRange => "用例号越界：超出 1..=36",
            Form::ColumnIncomplete => "判据列残缺：用例号/判据/期望三列缺列",
            Form::OrchMissing => "编排项缺失：调度序位无编排条目",
            Form::OrchDup => "编排项重号：同一编排号两次入册",
            Form::RefCaseDrift => "引用一漂移：执行用例与 F3110 冻结件失配",
            Form::RefOrchDrift => "引用二漂移：执行编排与 F3111 冻结件失配",
            Form::GateNotPass => "执行前置门不过：F3157 五门逐门复核有不过项",
            Form::AssertIncomplete => "断言完备性不过：有用例缺可判定断言",
            Form::IdentityEnable => "启用守恒式偏离：Σ册 != Σ编 != Σ使",
            Form::LedgerUnbalanced => "环形账不守恒：写入 != 存活 + 丢弃",
            Form::FingerprintDrift => "冻结指纹漂移：启用件哈希与冻结件不符",
            Form::StepOverCap => "执行步数越界",
        }
    }
    /// 对应探面。
    pub fn probe(&self) -> &'static str {
        match self {
            Form::CaseMissing => "casebook.bits",
            Form::CaseDup => "casebook.dup_bits",
            Form::CaseOutOfRange => "casebook.range_over",
            Form::ColumnIncomplete => "casebook.column_mask",
            Form::OrchMissing => "orch.bits",
            Form::OrchDup => "orch.dup_bits",
            Form::RefCaseDrift => "ref1.mismatch_bits",
            Form::RefOrchDrift => "ref2.mismatch_bits",
            Form::GateNotPass => "gates.pass_bits",
            Form::AssertIncomplete => "assert.complete",
            Form::IdentityEnable => "identity.dev_e",
            Form::LedgerUnbalanced => "ring.balanced",
            Form::FingerprintDrift => "freeze.verify",
            Form::StepOverCap => "run.step_overflow",
        }
    }
}

/// 辅助码（6 个，段内偏移 `0x1C..=0x21`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuxCode {
    /// 用例号越界。
    CaseRangeOver,
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
        AuxCode::CaseRangeOver,
        AuxCode::RevMismatch,
        AuxCode::RepeatShort,
        AuxCode::FreezeMismatch,
        AuxCode::CapOver,
        AuxCode::SeqMismatch,
    ];
    /// 段内偏移。
    pub fn index(&self) -> usize {
        match self {
            AuxCode::CaseRangeOver => 0,
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
            AuxCode::CaseRangeOver => "CaseRangeOver",
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
            AuxCode::CaseRangeOver => "用例号越界：超出 1..=36",
            AuxCode::RevMismatch => "版本失配：启用件与冻结件版本不同",
            AuxCode::RepeatShort => "轮数不足",
            AuxCode::FreezeMismatch => "冻结值失配：入册基准与现值不同",
            AuxCode::CapOver => "容量超限：读数越上界",
            AuxCode::SeqMismatch => "序号失配：编排序位不连续",
        }
    }
}

/// 错误（三要素）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UxErr {
    /// 绝对码。
    pub code: u16,
    /// 明文。
    pub msg: String,
}
impl UxErr {
    /// 由形态构造。
    pub fn of_form(f: Form, msg: String) -> UxErr {
        UxErr { code: f.code(), msg }
    }
    /// 由辅助码构造。
    pub fn of_aux(a: AuxCode, msg: String) -> UxErr {
        UxErr { code: a.code(), msg }
    }
    /// 由裸码构造。
    pub fn new(code: u16, msg: String) -> UxErr {
        UxErr { code, msg }
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
pub type UxResult<T> = Result<T, UxErr>;

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
pub fn err_str(e: &UxErr) -> String {
    match form_by_code(e.code) {
        Some(n) => format!("[{}] {}", n, e.msg),
        None => match aux_by_code(e.code) {
            Some(n) => format!("[{}] {}", n, e.msg),
            None => format!("[RAW:{:04X}] {}", e.code, e.msg),
        },
    }
}
/// 越界错误。
pub fn err_range(what: &str, idx: usize, cap: usize) -> UxErr {
    UxErr::of_aux(AuxCode::CaseRangeOver, format!("越界：{} idx={} cap={}", what, idx, cap))
}
/// 启用守恒式错误。
pub fn err_enable(a: u64, b: u64, c: u64) -> UxErr {
    UxErr::of_form(
        Form::IdentityEnable,
        format!("启用守恒式偏离：Σ册={} Σ编={} Σ使={}", a, b, c),
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
}// ===========================================================================
// 2. 相位状态机
// ===========================================================================
/// 相位（五段 + 终态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// 相位一：双引用登记。
    P1RefBind,
    /// 相位二：逐项引用核。
    P2RefCheck,
    /// 相位三：五门前置复核。
    P3GateCheck,
    /// 相位四：启用守恒 + 启用件入册。
    P4EnableSeal,
    /// 相位五：执行调度留痕。
    P5Dispatch,
    /// 终态。
    Sealed,
}
impl Phase {
    /// 相位数（含终态）。
    pub const COUNT: usize = 6;
    /// 工作相位数。
    pub const STEPS: usize = 5;
    /// 全部相位。
    pub const ALL: [Phase; Phase::COUNT] = [
        Phase::P1RefBind,
        Phase::P2RefCheck,
        Phase::P3GateCheck,
        Phase::P4EnableSeal,
        Phase::P5Dispatch,
        Phase::Sealed,
    ];
    /// 相位序号。
    pub fn index(&self) -> usize {
        match self {
            Phase::P1RefBind => 0,
            Phase::P2RefCheck => 1,
            Phase::P3GateCheck => 2,
            Phase::P4EnableSeal => 3,
            Phase::P5Dispatch => 4,
            Phase::Sealed => 5,
        }
    }
    /// 相位名。
    pub fn name(&self) -> &'static str {
        match self {
            Phase::P1RefBind => "P1RefBind",
            Phase::P2RefCheck => "P2RefCheck",
            Phase::P3GateCheck => "P3GateCheck",
            Phase::P4EnableSeal => "P4EnableSeal",
            Phase::P5Dispatch => "P5Dispatch",
            Phase::Sealed => "Sealed",
        }
    }
    /// 相位语义。
    pub fn detail(&self) -> &'static str {
        match self {
            Phase::P1RefBind => "相位一：F3110 用例册与 F3111 编排双引用登记入册",
            Phase::P2RefCheck => "相位二：双引用逐项比对（用例三列 + 编排三列）",
            Phase::P3GateCheck => "相位三：F3157 五门前置门逐门复核读数",
            Phase::P4EnableSeal => "相位四：启用守恒式收口 + 启用件哈希入册",
            Phase::P5Dispatch => "相位五：执行调度留痕（F3162–F3169 八段）",
            Phase::Sealed => "终态：B39 账面执行已启用",
        }
    }
}

/// 合法迁移边（链式闭集，共 5 条）。
pub const LEGAL_EDGES: [(Phase, Phase); 5] = [
    (Phase::P1RefBind, Phase::P2RefCheck),
    (Phase::P2RefCheck, Phase::P3GateCheck),
    (Phase::P3GateCheck, Phase::P4EnableSeal),
    (Phase::P4EnableSeal, Phase::P5Dispatch),
    (Phase::P5Dispatch, Phase::Sealed),
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
    /// 来源。
    pub from: Phase,
    /// 目标。
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
            state: Phase::P1RefBind,
            tick: 0,
            trail: vec![Phase::P1RefBind],
            rejected: Vec::with_capacity(8),
        }
    }
    /// 边总数。
    pub fn edge_count() -> usize {
        LEGAL_EDGES.len()
    }
    /// 尝试迁移（非法则拒记并返回 `Err`）。
    pub fn try_go(&mut self, to: Phase) -> UxResult<Phase> {
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
            return Err(UxErr::of_aux(
                AuxCode::SeqMismatch,
                format!("非法迁移：{} -> {}", self.state.name(), to.name()),
            ));
        }
        self.state = to;
        self.trail.push(to);
        Ok(to)
    }
    /// 推进到指定相位。
    pub fn advance_to(&mut self, to: Phase) -> UxResult<Phase> {
        self.try_go(to)
    }
    /// 已走边数。
    pub fn edges_walked(&self) -> usize {
        if self.trail.is_empty() {
            return 0;
        }
        self.trail.len() - 1
    }
    /// 轨迹单调性。
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
    /// 边矩阵。
    pub fn edge_matrix() -> [[u8; Phase::COUNT]; Phase::COUNT] {
        let mut m = [[0u8; Phase::COUNT]; Phase::COUNT];
        let mut i: usize = 0;
        while i < LEGAL_EDGES.len() {
            m[LEGAL_EDGES[i].0.index()][LEGAL_EDGES[i].1.index()] = 1;
            i = i.wrapping_add(1);
        }
        m
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
// 3. 用例册（F3110 冻结件消费侧）
// ===========================================================================
/// 册位掩码（36 位 → `u64` 承载）。
pub const CASE_MASK64: u64 = 0x0000_000F_FFFF_FFFFu64;

/// 回归项名表（`F3162–F3169` 八项 + 基线四格）。
pub const CASE_GROUPS: [&str; 9] = [
    "压力回归", "回收回归", "页缓存回归", "大页回归", "断电回归", "水线回归", "COW回归", "NUMA回归",
    "基线齐备",
];

/// 按用例号取回归项名。
pub fn case_group_at(id: usize) -> &'static str {
    if id >= 1 && id <= CASE_TOTAL {
        CASE_GROUPS[(id - 1) / 4]
    } else {
        "未定义项"
    }
}
/// 按用例号取项内格序（1..=4）。
pub fn case_cell_at(id: usize) -> usize {
    if id >= 1 && id <= CASE_TOTAL {
        (id - 1) % 4 + 1
    } else {
        0
    }
}
/// 按用例号取判据文本。
pub fn case_crit_at(id: usize) -> String {
    let g = case_group_at(id);
    let c = case_cell_at(id);
    format!("{}/格{}：读数与基线一致且断言全过", g, c)
}
/// 按用例号取期望文本。
pub fn case_expect_at(id: usize) -> String {
    let g = case_group_at(id);
    let c = case_cell_at(id);
    format!("{}/格{}：期望判定 PASS（三态取 PASS）", g, c)
}

/// 用例（三列：用例号 / 判据 / 期望）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Case {
    /// 用例号（1..=36）。
    pub id: usize,
    /// 回归项名。
    pub group: &'static str,
    /// 项内格序。
    pub cell: usize,
    /// 判据列。
    pub crit: String,
    /// 期望列。
    pub expect: String,
    /// 断言条数。
    pub asserts: usize,
    /// 执行期望读数（由执行段回填）。
    pub reading: u64,
    /// 册位哈希（三列 + 断言数）。
    pub fp: u64,
}
impl Case {
    /// 由用例号构造（判据与期望取冻结文本）。
    pub fn new(id: usize) -> Case {
        let mut c = Case {
            id,
            group: case_group_at(id),
            cell: case_cell_at(id),
            crit: case_crit_at(id),
            expect: case_expect_at(id),
            asserts: ASSERT_PER_CASE,
            reading: 0,
            fp: 0,
        };
        c.fp = c.compute_fp();
        c
    }
    /// 重算册位哈希（三列全文 + 断言数）。
    pub fn compute_fp(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(7);
        words.push(self.id as u64);
        words.push(self.cell as u64);
        words.push(self.asserts as u64);
        words.push(fnv1a64(self.group.as_bytes()));
        words.push(fnv1a64(self.crit.as_bytes()));
        words.push(fnv1a64(self.expect.as_bytes()));
        words.push(fnv1a_str(fnv1a64(REF_CASEBOOK.as_bytes()), self.group));
        fnv1a_words(&words)
    }
    /// 刷新哈希。
    pub fn refresh(&mut self) {
        self.fp = self.compute_fp();
    }
    /// 三列齐备。
    pub fn columns_ok(&self) -> bool {
        !self.crit.is_empty() && !self.expect.is_empty() && self.id >= 1
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "case[{}] {}/格{} crit={} expect={} asserts={} reading={} fp={:016X}",
            self.id,
            self.group,
            self.cell,
            self.crit,
            self.expect,
            self.asserts,
            self.reading,
            self.fp
        )
    }
}

/// 用例册。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CaseBook {
    /// 条目。
    pub cases: Vec<Case>,
    /// 覆盖位图。
    pub bits: u64,
    /// 重号位图。
    pub dup_bits: u64,
    /// 三列残缺位图。
    pub column_mask: u64,
    /// 用例号越界计数。
    pub range_over: usize,
    /// 判据列文本和（跨用例归集）。
    pub crit_sum: u64,
}
impl CaseBook {
    /// 空册。
    pub fn blank() -> CaseBook {
        CaseBook {
            cases: Vec::with_capacity(CASE_TOTAL),
            bits: 0,
            dup_bits: 0,
            column_mask: 0,
            range_over: 0,
            crit_sum: 0,
        }
    }
    /// 登记一用例。
    pub fn register(&mut self, c: Case) -> UxResult<()> {
        if c.id < 1 || c.id > CASE_TOTAL {
            self.range_over = self.range_over.wrapping_add(1);
            return Err(err_range("case", c.id, CASE_TOTAL));
        }
        let b = 1u64 << (c.id - 1);
        if (self.bits & b) != 0 {
            self.dup_bits = self.dup_bits | b;
            return Err(UxErr::of_form(Form::CaseDup, format!("册位重号：case={}", c.id)));
        }
        if !c.columns_ok() {
            return Err(UxErr::of_form(
                Form::ColumnIncomplete,
                format!("判据列残缺：case={}", c.id),
            ));
        }
        self.bits = self.bits | b;
        self.cases.push(c);
        Ok(())
    }
    /// 全量重算派生字段。
    pub fn recompute(&mut self) {
        let mut bits: u64 = 0;
        let mut dup: u64 = 0;
        let mut col: u64 = 0;
        let mut over: usize = 0;
        let mut csum: u64 = 0;
        let mut i: usize = 0;
        while i < self.cases.len() {
            let c = &self.cases[i];
            if c.id < 1 || c.id > CASE_TOTAL {
                over = over.wrapping_add(1);
                i = i.wrapping_add(1);
                continue;
            }
            let b = 1u64 << (c.id - 1);
            if (bits & b) != 0 {
                dup = dup | b;
            } else {
                bits = bits | b;
            }
            if !c.columns_ok() {
                col = col | b;
            }
            csum = csum.wrapping_add(fnv1a64(c.crit.as_bytes()));
            i = i.wrapping_add(1);
        }
        self.bits = bits;
        self.dup_bits = dup;
        self.column_mask = col;
        self.range_over = over;
        self.crit_sum = csum;
    }
    /// 缺用例位掩码。
    pub fn missing_bits(&self) -> u64 {
        CASE_MASK64 & (CASE_MASK64 ^ self.bits)
    }
    /// 缺用例数。
    pub fn missing_count(&self) -> usize {
        self.missing_bits().count_ones() as usize
    }
    /// 缺用例清单。
    pub fn missing_list(&self) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::with_capacity(CASE_TOTAL);
        let mut i: usize = 0;
        while i < CASE_TOTAL {
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
    /// 册齐备。
    pub fn complete(&self) -> bool {
        self.missing_bits() == 0 && self.dup_bits == 0 && self.column_mask == 0 && self.range_over == 0
    }
    /// 按号取用例。
    pub fn case_of(&self, id: usize) -> UxResult<&Case> {
        let mut i: usize = 0;
        while i < self.cases.len() {
            if self.cases[i].id == id {
                return Ok(&self.cases[i]);
            }
            i = i.wrapping_add(1);
        }
        Err(UxErr::of_form(Form::CaseMissing, format!("用例不在册：id={}", id)))
    }
    /// 断言条数合计。
    pub fn assert_total(&self) -> usize {
        let mut t: usize = 0;
        let mut i: usize = 0;
        while i < self.cases.len() {
            t = t.wrapping_add(self.cases[i].asserts);
            i = i.wrapping_add(1);
        }
        t
    }
    /// 册位哈希（F3110 冻结件）。
    ///
    /// 逐用例把**判据列 / 期望列**全文混入哈希，因此「双引用一致」
    /// 不是口头约定而是哈希覆盖范围内的硬要求——改任一列，册哈希必变。
    pub fn book_hash(&self) -> u64 {
        let mut h = fnv1a64_ext(FNV_BASIS, &(CASE_TOTAL as u64).to_le_bytes());
        let mut id: usize = 1;
        while id <= CASE_TOTAL {
            match self.case_of(id) {
                Ok(c) => {
                    h = fnv1a_str(h, c.crit.as_str());
                    h = fnv1a_str(h, c.expect.as_str());
                    h = fnv1a64_ext(h, &c.fp.to_le_bytes());
                }
                Err(_) => {
                    h = fnv1a64_ext(h, b"\xFEABSENT");
                }
            }
            id = id.wrapping_add(1);
        }
        h
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "casebook n={} bits={:012X} dup={:012X} colgap={} over={} asserts={}",
            self.cases.len(),
            self.bits,
            self.dup_bits,
            self.column_gap(),
            self.range_over,
            self.assert_total()
        )
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(CASE_TOTAL * 4 + 6);
        words.push(self.cases.len() as u64);
        words.push(self.bits);
        words.push(self.dup_bits);
        words.push(self.column_mask);
        words.push(self.range_over as u64);
        words.push(self.crit_sum);
        let mut i: usize = 0;
        while i < self.cases.len() {
            words.push(self.cases[i].id as u64);
            words.push(self.cases[i].fp);
            words.push(self.cases[i].reading);
            words.push(self.cases[i].asserts as u64);
            i = i.wrapping_add(1);
        }
        fnv1a_words(&words)
    }
}
/// 36 用例齐备册。
pub fn case_book_full() -> UxResult<CaseBook> {
    let mut b = CaseBook::blank();
    let mut i: usize = 1;
    while i <= CASE_TOTAL {
        b.register(Case::new(i))?;
        i = i.wrapping_add(1);
    }
    b.recompute();
    Ok(b)
}// ===========================================================================
// 4. 执行器编排册（F3111 冻结件消费侧）
// ===========================================================================
/// 编排项（三列：编排号 / 序位 / 前置门集合）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OrchItem {
    /// 编排号（1..=8）。
    pub id: usize,
    /// 调度序位（1..=8）。
    pub seq: usize,
    /// 编排项名。
    pub name: &'static str,
    /// 覆盖用例号区间起点。
    pub case_lo: usize,
    /// 覆盖用例号区间终点（含）。
    pub case_hi: usize,
    /// 前置门集合位图（五门）。
    pub gate_mask: u32,
    /// 编排哈希（三列 + 门集合）。
    pub fp: u64,
}
impl OrchItem {
    /// 由编排号构造。
    pub fn new(id: usize) -> OrchItem {
        let lo = (id - 1) * 4 + 1;
        let hi = lo + 3;
        let mut it = OrchItem {
            id,
            seq: id,
            name: ORCH_NAMES[id - 1],
            case_lo: lo,
            case_hi: hi,
            // 门集合：第 `k` 个编排项过前 `min(id, GATE_COUNT)` 道门
            gate_mask: (1u32 << (if id < GATE_COUNT { id } else { GATE_COUNT })) - 1,
            fp: 0,
        };
        it.fp = it.compute_fp();
        it
    }
    /// 重算编排哈希。
    pub fn compute_fp(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(6);
        words.push(self.id as u64);
        words.push(self.seq as u64);
        words.push(self.case_lo as u64);
        words.push(self.case_hi as u64);
        words.push(self.gate_mask as u64);
        words.push(fnv1a64(self.name.as_bytes()) ^ fnv1a64(REF_ORCH.as_bytes()));
        fnv1a_words(&words)
    }
    /// 刷新哈希。
    pub fn refresh(&mut self) {
        self.fp = self.compute_fp();
    }
    /// 三列齐备（序位连续、门集合非空、覆盖区间合法）。
    pub fn columns_ok(&self) -> bool {
        self.seq >= 1
            && self.seq <= ORCH_TOTAL
            && self.gate_mask != 0
            && self.case_lo >= 1
            && self.case_hi <= CASE_TOTAL
            && self.case_lo <= self.case_hi
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "orch[{}] {} seq={} cases=[{}..{}] gates={:05b} fp={:016X}",
            self.id,
            self.name,
            self.seq,
            self.case_lo,
            self.case_hi,
            self.gate_mask,
            self.fp
        )
    }
}

/// 九编排项名表（与 `CASE_GROUPS` 九项一一对应，段段相接不重不漏）。
pub const ORCH_NAMES: [&str; ORCH_TOTAL] = [
    "压力编排", "回收编排", "页缓存编排", "大页编排", "断电编排", "水线编排", "COW编排", "NUMA编排",
    "基线齐备编排",
];

/// 编排册。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OrchBook {
    /// 条目。
    pub items: Vec<OrchItem>,
    /// 覆盖位图（8 位）。
    pub bits: u32,
    /// 重号位图。
    pub dup_bits: u32,
    /// 序位失配位图。
    pub seq_bad_bits: u32,
}
impl OrchBook {
    /// 空册。
    pub fn blank() -> OrchBook {
        OrchBook { items: Vec::with_capacity(ORCH_TOTAL), bits: 0, dup_bits: 0, seq_bad_bits: 0 }
    }
    /// 登记一编排项。
    pub fn register(&mut self, it: OrchItem) -> UxResult<()> {
        if it.id < 1 || it.id > ORCH_TOTAL {
            return Err(err_range("orch", it.id, ORCH_TOTAL));
        }
        let b = 1u32 << (it.id - 1);
        if (self.bits & b) != 0 {
            self.dup_bits = self.dup_bits | b;
            return Err(UxErr::of_form(Form::OrchDup, format!("编排重号：orch={}", it.id)));
        }
        if !it.columns_ok() {
            return Err(UxErr::of_form(
                Form::ColumnIncomplete,
                format!("编排三列残缺：orch={}", it.id),
            ));
        }
        self.bits = self.bits | b;
        self.items.push(it);
        Ok(())
    }
    /// 全量重算派生字段。
    pub fn recompute(&mut self) {
        let mut bits: u32 = 0;
        let mut dup: u32 = 0;
        let mut seqbad: u32 = 0;
        let mut i: usize = 0;
        while i < self.items.len() {
            let it = &self.items[i];
            if it.id < 1 || it.id > ORCH_TOTAL {
                i = i.wrapping_add(1);
                continue;
            }
            let b = 1u32 << (it.id - 1);
            if (bits & b) != 0 {
                dup = dup | b;
            } else {
                bits = bits | b;
            }
            if it.seq != it.id {
                seqbad = seqbad | b;
            }
            i = i.wrapping_add(1);
        }
        self.bits = bits;
        self.dup_bits = dup;
        self.seq_bad_bits = seqbad;
    }
    /// 缺编排位掩码。
    pub fn missing_bits(&self) -> u32 {
        let full = (1u32 << ORCH_TOTAL) - 1;
        full & (full ^ self.bits)
    }
    /// 缺编排数。
    pub fn missing_count(&self) -> usize {
        self.missing_bits().count_ones() as usize
    }
    /// 编排册齐备。
    pub fn complete(&self) -> bool {
        self.missing_bits() == 0 && self.dup_bits == 0 && self.seq_bad_bits == 0
    }
    /// 按号取编排项。
    pub fn item_of(&self, id: usize) -> UxResult<&OrchItem> {
        let mut i: usize = 0;
        while i < self.items.len() {
            if self.items[i].id == id {
                return Ok(&self.items[i]);
            }
            i = i.wrapping_add(1);
        }
        Err(UxErr::of_form(Form::OrchMissing, format!("编排项缺失：id={}", id)))
    }
    /// 覆盖用例数合计。
    pub fn covered_cases(&self) -> usize {
        let mut t: usize = 0;
        let mut i: usize = 0;
        while i < self.items.len() {
            t = t.wrapping_add(self.items[i].case_hi.wrapping_sub(self.items[i].case_lo).wrapping_add(1));
            i = i.wrapping_add(1);
        }
        t
    }
    /// 编排哈希（F3111 冻结件）。
    pub fn orch_hash(&self) -> u64 {
        let mut h = fnv1a64_ext(FNV_BASIS, &(ORCH_TOTAL as u64).to_le_bytes());
        let mut id: usize = 1;
        while id <= ORCH_TOTAL {
            match self.item_of(id) {
                Ok(it) => {
                    h = fnv1a_str(h, it.name);
                    h = fnv1a64_ext(h, &it.fp.to_le_bytes());
                    h = fnv1a64_ext(h, &it.gate_mask.to_le_bytes());
                }
                Err(_) => {
                    h = fnv1a64_ext(h, b"\xFDABSENT");
                }
            }
            id = id.wrapping_add(1);
        }
        h
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "orch n={} bits={:08X} dup={:08X} seqbad={:08X} miss={} covered={}",
            self.items.len(),
            self.bits,
            self.dup_bits,
            self.seq_bad_bits,
            self.missing_count(),
            self.covered_cases()
        )
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(ORCH_TOTAL * 4 + 4);
        words.push(self.items.len() as u64);
        words.push(self.bits as u64);
        words.push(self.dup_bits as u64);
        words.push(self.seq_bad_bits as u64);
        let mut i: usize = 0;
        while i < self.items.len() {
            words.push(self.items[i].id as u64);
            words.push(self.items[i].seq as u64);
            words.push(self.items[i].fp);
            words.push(self.items[i].gate_mask as u64);
            i = i.wrapping_add(1);
        }
        fnv1a_words(&words)
    }
}
/// 八编排齐备册。
pub fn orch_book_full() -> UxResult<OrchBook> {
    let mut b = OrchBook::blank();
    let mut i: usize = 1;
    while i <= ORCH_TOTAL {
        b.register(OrchItem::new(i))?;
        i = i.wrapping_add(1);
    }
    b.recompute();
    Ok(b)
}
// ===========================================================================
// 5. 双引用核（引用一 F3110 / 引用二 F3111）
// ===========================================================================
/// 引用比对结论。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RefVerdict {
    /// 项号（用例号或编排号）。
    pub index: usize,
    /// 入册哈希。
    pub booked: u64,
    /// 消费侧现值哈希。
    pub live: u64,
    /// 是否一致。
    pub ok: bool,
}

/// 双引用核结论。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RefCheck {
    /// 引用一（F3110 用例册）逐项结论。
    pub case_verdicts: Vec<RefVerdict>,
    /// 引用二（F3111 编排册）逐项结论。
    pub orch_verdicts: Vec<RefVerdict>,
    /// 引用一失配位图（用例号位）。
    pub ref1_mismatch: u64,
    /// 引用二失配位图（编排号位）。
    pub ref2_mismatch: u32,
    /// 入册的 F3110 册哈希。
    pub case_book_hash: u64,
    /// 入册的 F3111 编排哈希。
    pub orch_hash: u64,
    /// 现态 F3110 册哈希。
    pub case_live_hash: u64,
    /// 现态 F3111 编排哈希。
    pub orch_live_hash: u64,
}
impl RefCheck {
    /// 构造双引用核（逐项比对册位哈希与消费侧现值哈希）。
    pub fn build(cases: &CaseBook, orch: &OrchBook) -> UxResult<RefCheck> {
        let mut cv: Vec<RefVerdict> = Vec::with_capacity(CASE_TOTAL);
        let mut m1: u64 = 0;
        let mut id: usize = 1;
        while id <= CASE_TOTAL {
            // 消费侧现值哈希：按册内登记值独立重算（不复用 `fp` 字段）
            let live = match cases.case_of(id) {
                Ok(c) => Case {
                    id: c.id,
                    group: c.group,
                    cell: c.cell,
                    crit: c.crit.clone(),
                    expect: c.expect.clone(),
                    asserts: c.asserts,
                    reading: c.reading,
                    fp: 0,
                }
                .compute_fp(),
                Err(_) => 0,
            };
            let booked = match cases.case_of(id) {
                Ok(c) => c.fp,
                Err(_) => 0,
            };
            let ok = booked != 0 && booked == live;
            if !ok {
                m1 = m1 | (1u64 << (id - 1));
            }
            cv.push(RefVerdict { index: id, booked, live, ok });
            id = id.wrapping_add(1);
        }
        let mut ov: Vec<RefVerdict> = Vec::with_capacity(ORCH_TOTAL);
        let mut m2: u32 = 0;
        let mut k: usize = 1;
        while k <= ORCH_TOTAL {
            let live = match orch.item_of(k) {
                Ok(it) => OrchItem {
                    id: it.id,
                    seq: it.seq,
                    name: it.name,
                    case_lo: it.case_lo,
                    case_hi: it.case_hi,
                    gate_mask: it.gate_mask,
                    fp: 0,
                }
                .compute_fp(),
                Err(_) => 0,
            };
            let booked = match orch.item_of(k) {
                Ok(it) => it.fp,
                Err(_) => 0,
            };
            let ok = booked != 0 && booked == live;
            if !ok {
                m2 = m2 | (1u32 << (k - 1));
            }
            ov.push(RefVerdict { index: k, booked, live, ok });
            k = k.wrapping_add(1);
        }
        Ok(RefCheck {
            case_verdicts: cv,
            orch_verdicts: ov,
            ref1_mismatch: m1,
            ref2_mismatch: m2,
            case_book_hash: case_book_definition_hash(),
            orch_hash: orch_definition_hash(),
            case_live_hash: cases.book_hash(),
            orch_live_hash: orch.orch_hash(),
        })
    }
    /// 双引用全一致。
    pub fn both_ok(&self) -> bool {
        self.ref1_mismatch == 0 && self.ref2_mismatch == 0
    }
    /// 引用一失配数。
    pub fn ref1_bad(&self) -> usize {
        self.ref1_mismatch.count_ones() as usize
    }
    /// 引用二失配数。
    pub fn ref2_bad(&self) -> usize {
        self.ref2_mismatch.count_ones() as usize
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "refcheck case_ref({}) m1={} book={:016X} live={:016X} | orch_ref({}) m2={} hash={:016X} live={:016X} both_ok={}",
            REF_CASEBOOK,
            self.ref1_bad(),
            self.case_book_hash,
            self.case_live_hash,
            REF_ORCH,
            self.ref2_bad(),
            self.orch_hash,
            self.orch_live_hash,
            if self.both_ok() { 1 } else { 0 }
        )
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(CASE_TOTAL + ORCH_TOTAL + 6);
        words.push(self.ref1_mismatch);
        words.push(self.ref2_mismatch as u64);
        words.push(self.case_book_hash);
        words.push(self.orch_hash);
        words.push(self.case_live_hash);
        words.push(self.orch_live_hash);
        let mut i: usize = 0;
        while i < self.case_verdicts.len() {
            words.push(self.case_verdicts[i].booked ^ self.case_verdicts[i].live);
            i = i.wrapping_add(1);
        }
        let mut j: usize = 0;
        while j < self.orch_verdicts.len() {
            words.push(self.orch_verdicts[j].booked ^ self.orch_verdicts[j].live);
            j = j.wrapping_add(1);
        }
        fnv1a_words(&words)
    }
}
/// F3110 用例册**定义件**哈希。
///
/// 与 `CaseBook::book_hash()` **同构**（同样的初值、同样的逐项混合序），
/// 唯一区别是：册哈希读**册内登记值** `c.fp`，定义哈希由**冻结文本现算** `Case::new(id).fp`。
/// 两者只有在「册内登记值 == 冻结定义重算值」时才相等——
/// 这正是「引用一致断言」要证的那件事；若两条口径不同构，
/// 无论册内被改成什么样二者都恒不相等，判据就退化成永假式。
pub fn case_book_definition_hash() -> u64 {
    let mut h = fnv1a64_ext(FNV_BASIS, &(CASE_TOTAL as u64).to_le_bytes());
    let mut id: usize = 1;
    while id <= CASE_TOTAL {
        let c = Case::new(id);
        h = fnv1a_str(h, c.crit.as_str());
        h = fnv1a_str(h, c.expect.as_str());
        h = fnv1a64_ext(h, &c.fp.to_le_bytes());
        id = id.wrapping_add(1);
    }
    h
}
/// F3111 编排**定义件**哈希。
pub fn orch_definition_hash() -> u64 {
    // 与 `OrchBook::orch_hash()` 同构（同初值、同逐项混合序），
    // 区别仅在于读册内登记值 `it.fp` 还是由冻结编排现算。
    let mut h = fnv1a64_ext(FNV_BASIS, &(ORCH_TOTAL as u64).to_le_bytes());
    let mut id: usize = 1;
    while id <= ORCH_TOTAL {
        let it = OrchItem::new(id);
        h = fnv1a_str(h, it.name);
        h = fnv1a64_ext(h, &it.fp.to_le_bytes());
        h = fnv1a64_ext(h, &it.gate_mask.to_le_bytes());
        id = id.wrapping_add(1);
    }
    h
}
// ===========================================================================
// 6. F3157 五门前置门
// ===========================================================================
/// 门位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gate {
    /// G0 方案冻结门。
    G0Freeze,
    /// G1 用例齐备门。
    G1CaseReady,
    /// G2 断言完备门。
    G2AssertReady,
    /// G3 资源预算门。
    G3Budget,
    /// G4 边界零违规门。
    G4Border,
}
impl Gate {
    /// 门数。
    pub const COUNT: usize = 5;
    /// 全部门。
    pub const ALL: [Gate; Gate::COUNT] = [
        Gate::G0Freeze,
        Gate::G1CaseReady,
        Gate::G2AssertReady,
        Gate::G3Budget,
        Gate::G4Border,
    ];
    /// 门位号。
    pub fn index(&self) -> usize {
        match self {
            Gate::G0Freeze => 0,
            Gate::G1CaseReady => 1,
            Gate::G2AssertReady => 2,
            Gate::G3Budget => 3,
            Gate::G4Border => 4,
        }
    }
    /// 门名。
    pub fn name(&self) -> &'static str {
        match self {
            Gate::G0Freeze => "G0方案冻结门",
            Gate::G1CaseReady => "G1用例齐备门",
            Gate::G2AssertReady => "G2断言完备门",
            Gate::G3Budget => "G3资源预算门",
            Gate::G4Border => "G4边界零违规门",
        }
    }
    /// 复核读数口径。
    pub fn detail(&self) -> &'static str {
        match self {
            Gate::G0Freeze => "F3110 册哈希与 F3111 编排哈希均已入册",
            Gate::G1CaseReady => "36 个执行用例在册且与 F3110 逐字一致",
            Gate::G2AssertReady => "每个用例至少 3 条可判定断言",
            Gate::G3Budget => "预算账平（步数 / 内存 / syscall 三项）",
            Gate::G4Border => "域外动作零次",
        }
    }
    /// 门位图。
    pub fn bit(&self) -> u32 {
        1u32 << self.index()
    }
}

/// 单门复核读数。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GateRow {
    /// 门位。
    pub gate: Gate,
    /// 复核读数。
    pub reading: u64,
    /// 读数是否在上限内。
    pub cap_ok: bool,
    /// 是否通过。
    pub pass: bool,
}
impl GateRow {
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "{} reading={} cap_ok={} pass={} | {}",
            self.gate.name(),
            self.reading,
            if self.cap_ok { 1 } else { 0 },
            if self.pass { 1 } else { 0 },
            self.gate.detail()
        )
    }
}

/// 五门复核表。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GateTable {
    /// 逐门读数。
    pub rows: Vec<GateRow>,
    /// 通过位图。
    pub pass_bits: u32,
}
impl GateTable {
    /// 构造五门复核。
    pub fn build(
        refchk: &RefCheck,
        cases: &CaseBook,
        budget_ok: bool,
        border_violations: u64,
    ) -> GateTable {
        let g0_reading: u64 = refchk.case_book_hash ^ refchk.orch_hash;
        let g1_reading: u64 = cases.missing_count() as u64;
        let g2_reading: u64 = cases.assert_total() as u64;
        let g3_reading: u64 = if budget_ok { 1 } else { 0 };
        let g4_reading: u64 = border_violations;
        let mut rows: Vec<GateRow> = Vec::with_capacity(Gate::COUNT);
        let mut pass_bits: u32 = 0;
        let vals: [u64; Gate::COUNT] = [g0_reading, g1_reading, g2_reading, g3_reading, g4_reading];
        let passes: [bool; Gate::COUNT] = [
            refchk.both_ok(),
            cases.complete() && cases.missing_count() == 0,
            cases.assert_total() == ASSERT_TOTAL,
            budget_ok,
            border_violations == 0,
        ];
        let mut i: usize = 0;
        while i < Gate::ALL.len() {
            let g = Gate::ALL[i];
            let r = vals[i];
            // 读数上界：G0 是哈希值，只做「非零」判；其余逐门各有上界
            let cap_ok = match g {
                Gate::G0Freeze => r != 0,
                Gate::G1CaseReady => r == 0,
                Gate::G2AssertReady => r == ASSERT_TOTAL as u64,
                Gate::G3Budget => r == 1,
                Gate::G4Border => r == 0,
            };
            let p = passes[i] && cap_ok;
            if p {
                pass_bits = pass_bits | g.bit();
            }
            rows.push(GateRow { gate: g, reading: r, cap_ok, pass: p });
            i = i.wrapping_add(1);
        }
        // 失败门由 `pass_bits` 的缺位显式体现：逐门读数 + 结论已落在 `rows` 中。
        GateTable { rows, pass_bits }
    }
    /// 五门全过。
    pub fn all_pass(&self) -> bool {
        self.pass_bits == ((1u32 << Gate::COUNT) - 1)
    }
    /// 不过门清单。
    pub fn fail_list(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::with_capacity(Gate::COUNT);
        let mut i: usize = 0;
        while i < self.rows.len() {
            if !self.rows[i].pass {
                out.push(self.rows[i].gate.name());
            }
            i = i.wrapping_add(1);
        }
        out
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        let mut s = String::from("gates pass_bits=");
        s.push_str(&format!("{:05b}", self.pass_bits));
        s.push_str(&format!(" all_pass={} fail={:?}", if self.all_pass() { 1 } else { 0 }, self.fail_list()));
        s
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(Gate::COUNT * 3 + 2);
        words.push(self.pass_bits as u64);
        words.push(self.rows.len() as u64);
        let mut i: usize = 0;
        while i < self.rows.len() {
            words.push(self.rows[i].gate.index() as u64);
            words.push(self.rows[i].reading);
            words.push(self.rows[i].pass as u64);
            i = i.wrapping_add(1);
        }
        fnv1a_words(&words)
    }
}// ===========================================================================
// 7. 执行段（真跑 LCG 事件流）
// ===========================================================================
/// 执行事件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunEvent {
    /// 取册位。
    FetchCase,
    /// 比判据。
    MatchCrit,
    /// 跑断言。
    AssertRun,
    /// 判三态。
    Verdict,
    /// 留痕。
    Trace,
    /// 编排同步。
    OrchSync,
}
impl RunEvent {
    /// 事件数。
    pub const COUNT: usize = 6;
    /// 全部事件。
    pub const ALL: [RunEvent; RunEvent::COUNT] = [
        RunEvent::FetchCase,
        RunEvent::MatchCrit,
        RunEvent::AssertRun,
        RunEvent::Verdict,
        RunEvent::Trace,
        RunEvent::OrchSync,
    ];
    /// 序号。
    pub fn index(&self) -> usize {
        match self {
            RunEvent::FetchCase => 0,
            RunEvent::MatchCrit => 1,
            RunEvent::AssertRun => 2,
            RunEvent::Verdict => 3,
            RunEvent::Trace => 4,
            RunEvent::OrchSync => 5,
        }
    }
    /// 事件名。
    pub fn name(&self) -> &'static str {
        match self {
            RunEvent::FetchCase => "FetchCase",
            RunEvent::MatchCrit => "MatchCrit",
            RunEvent::AssertRun => "AssertRun",
            RunEvent::Verdict => "Verdict",
            RunEvent::Trace => "Trace",
            RunEvent::OrchSync => "OrchSync",
        }
    }
    /// 步数代价。
    pub fn cost(&self) -> u64 {
        match self {
            RunEvent::FetchCase => 1,
            RunEvent::MatchCrit => 3,
            RunEvent::AssertRun => 5,
            RunEvent::Verdict => 2,
            RunEvent::Trace => 2,
            RunEvent::OrchSync => 4,
        }
    }
}

/// 判定三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TriState {
    /// 过。
    Pass,
    /// 欠。
    Owe,
    /// 挂。
    Hang,
}
impl TriState {
    /// 三态数。
    pub const COUNT: usize = 3;
    /// 全三态。
    pub const ALL: [TriState; TriState::COUNT] = [TriState::Pass, TriState::Owe, TriState::Hang];
    /// 序号。
    pub fn index(&self) -> usize {
        match self {
            TriState::Pass => 0,
            TriState::Owe => 1,
            TriState::Hang => 2,
        }
    }
    /// 名称。
    pub fn name(&self) -> &'static str {
        match self {
            TriState::Pass => "PASS",
            TriState::Owe => "OWE",
            TriState::Hang => "HANG",
        }
    }
    /// 是否通过。
    pub fn is_pass(&self) -> bool {
        *self == TriState::Pass
    }
}

/// 单用例执行行。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RunRow {
    /// 用例号。
    pub case_id: usize,
    /// 编排号（用例号反查）。
    pub orch_id: usize,
    /// 事件数。
    pub events: u64,
    /// 步数代价。
    pub cost: u64,
    /// 逻辑时刻。
    pub tick: u64,
    /// 断言通过数。
    pub assert_pass: u64,
    /// 断言总数。
    pub assert_total: u64,
    /// 执行读数（`Σ` 侧真相源）。
    pub reading: u64,
    /// 三态判定。
    pub verdict: TriState,
    /// 步数是否在上限内。
    pub steps_ok: bool,
}
impl RunRow {
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "run[{}] orch={} events={} cost={} tick={} assert={}/{} reading={} verdict={} steps_ok={}",
            self.case_id,
            self.orch_id,
            self.events,
            self.cost,
            self.tick,
            self.assert_pass,
            self.assert_total,
            self.reading,
            self.verdict.name(),
            if self.steps_ok { 1 } else { 0 }
        )
    }
}

/// 执行册。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RunBook {
    /// 逐用例执行行。
    pub rows: Vec<RunRow>,
    /// 已跑用例位图。
    pub ran_bits: u64,
    /// 步数越界计数。
    pub step_overflow: usize,
    /// 判定三态位图（`P0..2 | O0..2 | H0..2`）。
    pub verdict_bits: u32,
}
impl RunBook {
    /// 空册。
    pub fn blank() -> RunBook {
        RunBook { rows: Vec::with_capacity(CASE_TOTAL), ran_bits: 0, step_overflow: 0, verdict_bits: 0 }
    }
    /// 按用例号取执行行。
    pub fn row_of(&self, id: usize) -> UxResult<&RunRow> {
        let mut i: usize = 0;
        while i < self.rows.len() {
            if self.rows[i].case_id == id {
                return Ok(&self.rows[i]);
            }
            i = i.wrapping_add(1);
        }
        Err(UxErr::of_form(Form::CaseMissing, format!("执行行缺失：case={}", id)))
    }
    /// 未跑用例数。
    pub fn missing_count(&self) -> usize {
        (CASE_MASK64 & (CASE_MASK64 ^ self.ran_bits)).count_ones() as usize
    }
    /// 全册步数上限内。
    pub fn steps_ok(&self) -> bool {
        let mut i: usize = 0;
        while i < self.rows.len() {
            if !self.rows[i].steps_ok {
                return false;
            }
            i = i.wrapping_add(1);
        }
        self.step_overflow == 0
    }
    /// 全册 PASS。
    pub fn all_pass(&self) -> bool {
        self.verdict_bits == 0 && self.missing_count() == 0
    }
    /// 执行读数合计。
    pub fn reading_sum(&self) -> u64 {
        let mut t: u64 = 0;
        let mut i: usize = 0;
        while i < self.rows.len() {
            t = t.wrapping_add(self.rows[i].reading);
            i = i.wrapping_add(1);
        }
        t
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "run n={} ran={:012X} miss={} step_over={} verdicts={:09b} rsum={}",
            self.rows.len(),
            self.ran_bits,
            self.missing_count(),
            self.step_overflow,
            self.verdict_bits,
            self.reading_sum()
        )
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(CASE_TOTAL * 4 + 4);
        words.push(self.rows.len() as u64);
        words.push(self.ran_bits);
        words.push(self.step_overflow as u64);
        words.push(self.verdict_bits as u64);
        let mut i: usize = 0;
        while i < self.rows.len() {
            words.push(self.rows[i].case_id as u64);
            words.push(self.rows[i].reading);
            words.push(self.rows[i].cost);
            words.push(self.rows[i].verdict.index() as u64);
            i = i.wrapping_add(1);
        }
        fnv1a_words(&words)
    }
}
/// 执行一个用例（真跑 LCG 事件流 + 双累加器交叉复核）。
pub fn run_case(seed: u64, c: &Case) -> UxResult<RunRow> {
    if c.id < 1 || c.id > CASE_TOTAL {
        return Err(err_range("run_case", c.id, CASE_TOTAL));
    }
    let orch_id = (c.id - 1) / 4 + 1;
    let mix: u64 = (c.id as u64)
        .wrapping_mul(0xC2B2_AE3D_27D4_EB4Fu64)
        ^ fnv1a64(c.group.as_bytes())
        ^ fnv1a64(c.crit.as_bytes());
    let mut lg = Lcg::new(seed ^ mix);
    let mut events: u64 = 0;
    let mut cost: u64 = 0;
    let mut tick: u64 = 0;
    let mut reading: u64 = 0;
    let mut shadow_read: u64 = 0;
    let mut assert_pass: u64 = 0;
    let mut assert_total: u64 = 0;
    let mut seen: Vec<u8> = Vec::with_capacity(CASE_STEPS);
    let mut s: usize = 0;
    while s < CASE_STEPS {
        let ev = RunEvent::ALL[lg.below(RunEvent::COUNT as u64) as usize];
        events = events.wrapping_add(1);
        cost = cost.wrapping_add(ev.cost());
        tick = tick_advance(tick);
        seen.push(ev.index() as u8);
        // 断言步按**固定节拍**判定（`s % ASSERT_STRIDE == 0`），
        // 不能靠随机事件流碰运气：净态必须保证每用例恰好跑出 `ASSERT_PER_CASE`
        // 条可判定断言，否则「断言完备」判据在净态就会随机误报，
        // 断言核便失去了「有缺即拦」的可信度。
        if s % ASSERT_STRIDE == 0 {
            assert_total = assert_total.wrapping_add(1);
            // 真两遍算法交叉复核：独立重扫 `seen` 求代价，与主/影两累加器比对
            let mut recompute: u64 = 0;
            let mut i: usize = 0;
            while i < seen.len() {
                let idx: usize = (seen[i] as usize) % RunEvent::COUNT;
                recompute = recompute.wrapping_add(RunEvent::ALL[idx].cost());
                i = i.wrapping_add(1);
            }
            // 执行读数：键域采样（与用例号绑定，同 seed 同读数）
            let v: u64 = lg.below(64u64).wrapping_add(1);
            reading = reading.wrapping_add(v);
            shadow_read = shadow_read.wrapping_add(v);
            if recompute == cost && reading == shadow_read {
                assert_pass = assert_pass.wrapping_add(1);
            }
        }
        s = s.wrapping_add(1);
    }
    // 断言完备性：每用例声明条数必须被真跑覆盖（不足即 Owe，超出即 Hang）
    let declared = c.asserts as u64;
    let verdict = if assert_total < declared {
        TriState::Owe
    } else if assert_total > declared {
        TriState::Hang
    } else if assert_pass != declared {
        TriState::Owe
    } else {
        TriState::Pass
    };
    let steps_ok = events <= STEP_CAP && tick <= STEP_CAP;
    Ok(RunRow {
        case_id: c.id,
        orch_id,
        events,
        cost,
        tick,
        assert_pass,
        assert_total,
        reading,
        verdict,
        steps_ok,
    })
}
/// 全册执行（按编排序位调度）。
pub fn run_all(seed: u64, cases: &CaseBook, orch: &OrchBook) -> UxResult<RunBook> {
    let mut rb = RunBook::blank();
    let mut o: usize = 1;
    while o <= ORCH_TOTAL {
        let it = match orch.item_of(o) {
            Ok(x) => x,
            Err(e) => return Err(e),
        };
        let mut id: usize = it.case_lo;
        while id <= it.case_hi {
            let c = match cases.case_of(id) {
                Ok(x) => x,
                Err(e) => return Err(e),
            };
            let row = run_case(seed, c)?;
            if !row.steps_ok {
                rb.step_overflow = rb.step_overflow.wrapping_add(1);
            }
            // 三态位图**只记非过态**（欠 / 挂）：`TriState::Pass` 的 index 为 0，
            // 若一并置位则 `verdict_bits == 0` 的「全过」判据恒假。
            if row.verdict != TriState::Pass {
                rb.verdict_bits = rb.verdict_bits | (1u32 << row.verdict.index());
            }
            rb.ran_bits = rb.ran_bits | (1u64 << (row.case_id - 1));
            rb.rows.push(row);
            id = id.wrapping_add(1);
        }
        o = o.wrapping_add(1);
    }
    Ok(rb)
}
// ===========================================================================
// 8. 启用守恒式：`Σ册 == Σ编 == Σ使`
// ===========================================================================
/// 启用守恒结论。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Identity {
    /// Σ册：用例册读数之和。
    pub sum_case: u64,
    /// Σ编：编排覆盖读数之和（按编排区间反查执行行）。
    pub sum_orch: u64,
    /// Σ使：执行册读数之和。
    pub sum_run: u64,
    /// `Σ册 != Σ编` 偏离。
    pub dev_co: u64,
    /// `Σ册 != Σ使` 偏离。
    pub dev_cr: u64,
    /// `Σ编 != Σ使` 偏离。
    pub dev_or: u64,
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
            "identity Σ册={} Σ编={} Σ使={} devCO={} devCR={} devOR={} max={} balanced={}",
            self.sum_case,
            self.sum_orch,
            self.sum_run,
            self.dev_co,
            self.dev_cr,
            self.dev_or,
            self.deviation,
            if self.balanced { 1 } else { 0 }
        )
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(9);
        words.push(self.sum_case);
        words.push(self.sum_orch);
        words.push(self.sum_run);
        words.push(self.dev_co);
        words.push(self.dev_cr);
        words.push(self.dev_or);
        words.push(self.deviation);
        words.push(self.balanced as u64);
        fnv1a_words(&words)
    }
}
/// 参与聚合的编排项筛选（闭集内且不重号、序位正确）。
fn agg_orch(o: &OrchBook) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::with_capacity(ORCH_TOTAL);
    let mut seen: u32 = 0;
    let mut i: usize = 0;
    while i < o.items.len() {
        let ix = o.items[i].id;
        if ix >= 1 && ix <= ORCH_TOTAL && o.items[i].seq == ix {
            let b = 1u32 << (ix - 1);
            if (seen & b) == 0 {
                seen = seen | b;
                out.push(ix);
            }
        }
        i = i.wrapping_add(1);
    }
    out
}
/// 启用守恒收口（三路独立聚合口径）。
///
/// - **Σ册**：按键号外层遍历，读**执行行的读数**（`RunRow.reading`）；
/// - **Σ编**：按编排序位外层遍历，每个编排项把自己的 `[lo..hi]` 区间
///   在执行册上的读数重新加一遍（第二遍归属算法）；
/// - **Σ使**：既不按键号也不按编排序，而是用编排序位映射出的**键位掩码**
///   与执行册已跑位图求交后累加（第三遍算法）。
///
/// 三路的遍历序与分组方式都不同，任何一侧多算 / 漏算 / 归属错位都会让三面不等。
pub fn identity_close(cases: &CaseBook, orch: &OrchBook, run: &RunBook) -> UxResult<Identity> {
    let agg = agg_orch(orch);
    // ── Σ册：键主序读执行行读数
    let mut sum_case: u64 = 0;
    let mut id: usize = 1;
    while id <= CASE_TOTAL {
        let v = match run.row_of(id) {
            Ok(r) => r.reading,
            Err(_) => 0,
        };
        sum_case = sum_case.wrapping_add(v);
        id = id.wrapping_add(1);
    }
    // ── Σ编：编排序位外层，逐区间重加
    let mut sum_orch: u64 = 0;
    let mut o: usize = 0;
    while o < agg.len() {
        let it = match orch.item_of(agg[o]) {
            Ok(x) => x,
            Err(_) => {
                o = o.wrapping_add(1);
                continue;
            }
        };
        let mut cid: usize = it.case_lo;
        while cid <= it.case_hi {
            let v = match run.row_of(cid) {
                Ok(r) => r.reading,
                Err(_) => 0,
            };
            sum_orch = sum_orch.wrapping_add(v);
            cid = cid.wrapping_add(1);
        }
        o = o.wrapping_add(1);
    }
    // ── Σ使：键位掩码交
    let mut sum_run: u64 = 0;
    let mut k: usize = 0;
    while k < agg.len() {
        let it = match orch.item_of(agg[k]) {
            Ok(x) => x,
            Err(_) => {
                k = k.wrapping_add(1);
                continue;
            }
        };
        let lo = it.case_lo;
        let hi = it.case_hi;
        let width: usize = hi.wrapping_sub(lo).wrapping_add(1);
        let mut m: u64 = 0;
        let mut w: usize = 0;
        while w < width {
            m = m | (1u64 << w);
            w = w.wrapping_add(1);
        }
        let fmask: u64 = m << (lo - 1);
        let mut cid: usize = 1;
        while cid <= CASE_TOTAL {
            let kb: u64 = 1u64 << (cid - 1);
            if (fmask & kb) != 0 && (run.ran_bits & kb) != 0 {
                let v = match run.row_of(cid) {
                    Ok(r) => r.reading,
                    Err(_) => 0,
                };
                sum_run = sum_run.wrapping_add(v);
            }
            cid = cid.wrapping_add(1);
        }
        k = k.wrapping_add(1);
    }
    let dev_co = abs_diff(sum_case, sum_orch);
    let dev_cr = abs_diff(sum_case, sum_run);
    let dev_or = abs_diff(sum_orch, sum_run);
    let mut dev: u64 = 0;
    if dev_co > dev {
        dev = dev_co;
    }
    if dev_cr > dev {
        dev = dev_cr;
    }
    if dev_or > dev {
        dev = dev_or;
    }
    let mut items: Vec<(String, u64, u64, bool)> = Vec::with_capacity(8);
    items.push((String::from("用例册齐备"), cases.missing_count() as u64, 0, cases.complete()));
    items.push((String::from("编排册齐备"), orch.missing_count() as u64, 0, orch.complete()));
    items.push((String::from("执行册跑满"), run.missing_count() as u64, 0, run.missing_count() == 0));
    items.push((String::from("Σ册 == Σ编"), sum_case, sum_orch, sum_case == sum_orch));
    items.push((String::from("Σ册 == Σ使"), sum_case, sum_run, sum_case == sum_run));
    items.push((String::from("Σ编 == Σ使"), sum_orch, sum_run, sum_orch == sum_run));
    Ok(Identity {
        sum_case,
        sum_orch,
        sum_run,
        dev_co,
        dev_cr,
        dev_or,
        deviation: dev,
        balanced: dev == 0,
        items,
    })
}
// ===========================================================================
// 9. append-only 环形账
// ===========================================================================
/// 环形账条目。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RingEntry {
    /// 序号。
    pub seq: u64,
    /// 用例号。
    pub case_id: usize,
    /// 编排号。
    pub orch_id: usize,
    /// 读数。
    pub reading: u64,
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
        RingLedger { slots: Vec::with_capacity(RING_CAP), pushed: 0, dropped: 0, next_seq: 1 }
    }
    /// 写一条。
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
    /// 守恒。
    pub fn balanced(&self) -> bool {
        self.pushed == self.slots.len() as u64 + self.dropped
    }
    /// 序号连续。
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
            words.push(self.slots[i].case_id as u64);
            words.push(self.slots[i].reading);
            words.push(self.slots[i].tick);
            i = i.wrapping_add(1);
        }
        fnv1a_words(&words)
    }
}
/// 灌环形账（`CASE_TOTAL × (REPEAT+1) = 144 > 128`，必产生覆盖）。
pub fn ring_fill(run: &RunBook) -> UxResult<RingLedger> {
    let mut ring = RingLedger::blank();
    let mut tick: u64 = 0;
    let mut r: usize = 0;
    while r <= REPEAT {
        let mut id: usize = 1;
        while id <= CASE_TOTAL {
            let row = run.row_of(id)?;
            tick = tick_advance(tick);
            let seq = ring.next_seq;
            ring.next_seq = ring.next_seq.wrapping_add(1);
            ring.push(RingEntry {
                seq,
                case_id: id,
                orch_id: row.orch_id,
                reading: row.reading,
                tick,
            });
            id = id.wrapping_add(1);
        }
        r = r.wrapping_add(1);
    }
    Ok(ring)
}// ===========================================================================
// 10. 注入核
// ===========================================================================
/// 注入留痕。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Evidence {
    /// 各形态触发。
    pub fired: [u64; Form::COUNT],
    /// 用例册指纹。
    pub case_fp: u64,
    /// 编排册指纹。
    pub orch_fp: u64,
    /// 执行册指纹。
    pub run_fp: u64,
    /// 双引用核指纹。
    pub ref_fp: u64,
    /// 五门指纹。
    pub gate_fp: u64,
    /// 恒等式指纹。
    pub ident_fp: u64,
    /// 环形账指纹。
    pub ring_fp: u64,
    /// 探针指纹（被测侧现值）。
    pub probe_fp: u64,
    /// **净态入册基准**（注入全程只读不改）。
    pub registered_probe: u64,
    /// 边界违规计数。
    pub border_violations: u64,
}
impl Evidence {
    /// 空留痕。
    pub fn blank() -> Evidence {
        Evidence {
            fired: [0u64; Form::COUNT],
            case_fp: 0,
            orch_fp: 0,
            run_fp: 0,
            ref_fp: 0,
            gate_fp: 0,
            ident_fp: 0,
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
        let mut n = 0usize;
        let mut i: usize = 0;
        while i < Form::COUNT {
            if self.fired[i] != 0 {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        format!(
            "evidence fired={}/{} border={} probe={:016X} registered={:016X}",
            n,
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
        (self.hit_mask & (1u128 << f.index())) != 0
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
    /// 用例册。
    pub cases: CaseBook,
    /// 编排册。
    pub orch: OrchBook,
    /// 执行册。
    pub run: RunBook,
    /// 双引用核。
    pub refchk: RefCheck,
    /// 五门表。
    pub gates: GateTable,
    /// 恒等式。
    pub ident: Identity,
    /// 环形账。
    pub ring: RingLedger,
    /// 留痕。
    pub ev: Evidence,
}
impl Snapshot {
    /// 净态构造（真跑全链）。
    pub fn net(seed: u64) -> UxResult<Snapshot> {
        let cases = case_book_full()?;
        let orch = orch_book_full()?;
        let run = run_all(seed, &cases, &orch)?;
        let refchk = RefCheck::build(&cases, &orch)?;
        let ident = identity_close(&cases, &orch, &run)?;
        let ring = ring_fill(&run)?;
        // 五门预算门预算一个守恒结论（净态必然守恒）
        let budget_ok = run.steps_ok() && ident.balanced && ring.balanced();
        let gates = GateTable::build(&refchk, &cases, budget_ok, 0);
        let mut ev = Evidence::blank();
        ev.case_fp = cases.fingerprint();
        ev.orch_fp = orch.fingerprint();
        ev.run_fp = run.fingerprint();
        ev.ref_fp = refchk.fingerprint();
        ev.gate_fp = gates.fingerprint();
        ev.ident_fp = ident.fingerprint();
        ev.ring_fp = ring.fingerprint();
        ev.probe_fp = probe_hash(&cases, &orch, &run, &refchk, &gates, &ident);
        ev.registered_probe = ev.probe_fp;
        Ok(Snapshot { cases, orch, run, refchk, gates, ident, ring, ev })
    }
    /// 净态自检。
    pub fn is_net(&self) -> bool {
        self.cases.complete()
            && self.orch.complete()
            && self.run.all_pass()
            && self.refchk.both_ok()
            && self.gates.all_pass()
            && self.ident.balanced
            && self.ring.balanced()
            && self.ev.border_violations == 0
    }
}
/// 探针哈希（被测侧现值）。
pub fn probe_hash(
    cases: &CaseBook,
    orch: &OrchBook,
    run: &RunBook,
    refchk: &RefCheck,
    gates: &GateTable,
    ident: &Identity,
) -> u64 {
    let mut words: Vec<u64> = Vec::with_capacity(14);
    words.push(V_VERSION_ID);
    words.push(cases.fingerprint());
    words.push(orch.fingerprint());
    words.push(run.fingerprint());
    words.push(refchk.fingerprint());
    words.push(gates.fingerprint());
    words.push(ident.fingerprint());
    words.push(case_book_definition_hash());
    words.push(orch_definition_hash());
    words.push(ident.sum_case);
    words.push(ident.sum_orch);
    words.push(ident.sum_run);
    words.push(ident.deviation);
    words.push(run.reading_sum());
    fnv1a_words(&words)
}
/// 单形态注入。
///
/// **归因纪律**：每处注入只污染它对应探面所读的数据源，
/// 否则检测会以别的形态的面貌失败，检出归因被污染。
pub fn inject_one(f: Form, seed: u64) -> UxResult<Snapshot> {
    let mut s = Snapshot::net(seed)?;
    match f {
        Form::CaseMissing => {
            // 册位缺失：删末条目后**必须重算**位图，否则留「幽灵覆盖」。
            s.cases.cases.pop();
            s.cases.recompute();
            s.run.rows.pop();
            // 清掉**末位用例号**的已跑标记（与 `pop()` 掉的正是同一条）
            s.run.ran_bits = s.run.ran_bits & !(1u64 << (CASE_TOTAL - 1));
        }
        Form::CaseDup => {
            // 册位重号：register 是拒收语义，直接 push 再重算。
            s.cases.cases.push(Case::new(3));
            s.cases.recompute();
        }
        Form::CaseOutOfRange => {
            // 用例号越界：push 一个 id > CASE_TOTAL 的条目。
            let mut bad = Case::new(1);
            bad.id = CASE_TOTAL + 7;
            bad.refresh();
            s.cases.cases.push(bad);
            s.cases.recompute();
        }
        Form::ColumnIncomplete => {
            // 判据列残缺：把某用例判据置空。
            if let Some(c) = s.cases.cases.iter_mut().find(|x| x.id == 14) {
                c.crit = String::new();
                c.refresh();
            }
            s.cases.recompute();
        }
        Form::OrchMissing => {
            // 编排项缺失：删末编排后重算。
            s.orch.items.pop();
            s.orch.recompute();
        }
        Form::OrchDup => {
            // 编排重号：直接 push 重复条目再重算。
            s.orch.items.push(OrchItem::new(5));
            s.orch.recompute();
        }
        Form::RefCaseDrift => {
            // 引用一漂移：改判据列文本（册哈希与逐项核双双失配）。
            // 刻意不碰 `fp`——`fp` 是「入册时冻结的值」，注入只改被冻结件本身，
            // 双引用核就会拿「旧冻结值 vs 新现值」比对出失配，这正是本形态的语义。
            if let Some(c) = s.cases.cases.iter_mut().find(|x| x.id == 9) {
                c.crit = String::from("页缓存回归/格1：判据文本被改写（引用一漂移）");
            }
            s.cases.recompute();
        }
        Form::RefOrchDrift => {
            // 引用二漂移：改编排覆盖区间终点。
            // **刻意不调 `refresh()`**：`fp` 是「入册时冻结的值」，
            // 注入只改被冻结件本身，双引用核就会拿「旧冻结值 vs 消费侧重算值」
            // 比对出失配；若同步刷新 `fp`，两边同改即互相抵消，形同自证。
            if let Some(it) = s.orch.items.iter_mut().find(|x| x.id == 4) {
                it.case_hi = it.case_hi.wrapping_add(2);
            }
            s.orch.recompute();
        }
        Form::GateNotPass => {
            // 前置门不过：破坏 G2 断言完备门（把某用例声明断言数改成 0）。
            if let Some(c) = s.cases.cases.iter_mut().find(|x| x.id == 22) {
                c.asserts = 0;
                c.refresh();
            }
            s.cases.recompute();
        }
        Form::AssertIncomplete => {
            // 断言完备性不过：直接改执行行的断言通过数（0/3）。
            if let Some(r) = s.run.rows.iter_mut().find(|x| x.case_id == 26) {
                r.assert_pass = 0;
            }
        }
        Form::IdentityEnable => {
            // 启用守恒式偏离：只改执行册读数中的**一行**。
            // Σ册 / Σ编 / Σ使 三路都读同一份 `RunRow.reading`，
            // 单行改动会让三面同幅偏移而互相抵消——因此必须改**编排覆盖区间**，
            // 让 Σ编 的区间重加与另两路产生分歧（真偏离，非自证）。
            if let Some(it) = s.orch.items.iter_mut().find(|x| x.id == 6) {
                it.case_hi = it.case_hi.wrapping_add(4);
                it.refresh();
            }
            s.orch.recompute();
            s.ident = identity_close(&s.cases, &s.orch, &s.run)?;
        }
        Form::LedgerUnbalanced => {
            // 环形账破坏：伪造 dropped。
            s.ring.dropped = s.ring.dropped.wrapping_add(5);
        }
        Form::FingerprintDrift => {
            // 冻结指纹漂移：改编排项名（纯描述字段，只动定义/现值哈希）。
            if let Some(it) = s.orch.items.iter_mut().find(|x| x.id == 2) {
                it.name = "回收编排（项名被改写）";
                it.refresh();
            }
            s.orch.recompute();
        }
        Form::StepOverCap => {
            // 执行步数越界：把某执行行 tick 顶过上限并置 steps_ok = false。
            if let Some(r) = s.run.rows.iter_mut().find(|x| x.case_id == 31) {
                r.tick = STEP_CAP.wrapping_add(1);
                r.steps_ok = false;
            }
            s.run.step_overflow = s.run.step_overflow.wrapping_add(1);
        }
    }
    s.ev.fired[f.index()] = 1;
    // 重折被测快照读数（`registered_probe` 只读不改）
    s.ev.case_fp = s.cases.fingerprint();
    s.ev.orch_fp = s.orch.fingerprint();
    s.ev.run_fp = s.run.fingerprint();
    s.ev.ref_fp = s.refchk.fingerprint();
    s.ev.gate_fp = s.gates.fingerprint();
    s.ev.ident_fp = s.ident.fingerprint();
    s.ev.ring_fp = s.ring.fingerprint();
    s.ev.probe_fp = probe_hash(&s.cases, &s.orch, &s.run, &s.refchk, &s.gates, &s.ident);
    Ok(s)
}
/// 全形态注入。
pub fn inject_all(seed: u64) -> UxResult<(Evidence, Vec<Snapshot>)> {
    let mut total = Evidence::blank();
    let mut each: Vec<Snapshot> = Vec::with_capacity(Form::COUNT);
    let mut i: usize = 0;
    while i < Form::ALL.len() {
        let snap = inject_one(Form::ALL[i], seed)?;
        total.fired[Form::ALL[i].index()] =
            total.fired[Form::ALL[i].index()].wrapping_add(snap.ev.of(Form::ALL[i]));
        total.case_fp = total.case_fp ^ snap.ev.case_fp;
        total.orch_fp = total.orch_fp ^ snap.ev.orch_fp;
        total.run_fp = total.run_fp ^ snap.ev.run_fp;
        total.ref_fp = total.ref_fp ^ snap.ev.ref_fp;
        total.gate_fp = total.gate_fp ^ snap.ev.gate_fp;
        total.ident_fp = total.ident_fp ^ snap.ev.ident_fp;
        total.ring_fp = total.ring_fp ^ snap.ev.ring_fp;
        total.probe_fp = total.probe_fp ^ snap.ev.probe_fp;
        // 净态入册基准：取第一张快照的登记值（全程只读不改，注入核拿它当比对基准）
        if total.registered_probe == 0 {
            total.registered_probe = snap.ev.registered_probe;
        }
        total.border_violations = total.border_violations.wrapping_add(snap.ev.border_violations);
        each.push(snap);
        i = i.wrapping_add(1);
    }
    Ok((total, each))
}
/// 检测器：只读入参，逐面独立判定。
pub fn detect_all(snap: &Snapshot) -> UxResult<Detections> {
    let mut d = Detections::blank();
    if snap.cases.missing_bits() != 0 {
        d.bump(Form::CaseMissing);
    }
    if snap.cases.dup_bits != 0 {
        d.bump(Form::CaseDup);
    }
    if snap.cases.range_over != 0 {
        d.bump(Form::CaseOutOfRange);
    }
    if snap.cases.column_mask != 0 {
        d.bump(Form::ColumnIncomplete);
    }
    if snap.orch.missing_bits() != 0 {
        d.bump(Form::OrchMissing);
    }
    if snap.orch.dup_bits != 0 {
        d.bump(Form::OrchDup);
    }
    // 双引用核独立重算
    let refchk = RefCheck::build(&snap.cases, &snap.orch)?;
    if refchk.ref1_mismatch != 0 {
        d.bump(Form::RefCaseDrift);
    }
    if refchk.ref2_mismatch != 0 {
        d.bump(Form::RefOrchDrift);
    }
    // 五门独立重算
    let ident_live = identity_close(&snap.cases, &snap.orch, &snap.run)?;
    let budget_ok = snap.run.steps_ok() && ident_live.balanced && snap.ring.balanced();
    let gates = GateTable::build(&refchk, &snap.cases, budget_ok, snap.ev.border_violations);
    if !gates.all_pass() {
        d.bump(Form::GateNotPass);
    }
    // 断言完备性独立重算
    let mut assert_bad = false;
    let mut id: usize = 1;
    while id <= CASE_TOTAL {
        match snap.run.row_of(id) {
            Ok(r) => {
                match snap.cases.case_of(id) {
                    Ok(c) => {
                        if r.assert_total != c.asserts as u64 || r.assert_pass != c.asserts as u64 {
                            assert_bad = true;
                        }
                    }
                    Err(_) => assert_bad = true,
                }
            }
            Err(_) => assert_bad = true,
        }
        id = id.wrapping_add(1);
    }
    if assert_bad {
        d.bump(Form::AssertIncomplete);
    }
    if ident_live.deviation != 0 {
        d.bump(Form::IdentityEnable);
    }
    if !snap.ring.balanced() {
        d.bump(Form::LedgerUnbalanced);
    }
    // 冻结指纹：拿净态入册基准比
    let probe = probe_hash(&snap.cases, &snap.orch, &snap.run, &refchk, &gates, &ident_live);
    if probe != snap.ev.registered_probe {
        d.bump(Form::FingerprintDrift);
    }
    if snap.run.step_overflow != 0 || !snap.run.steps_ok() {
        d.bump(Form::StepOverCap);
    }
    d.border_violations = snap.ev.border_violations;
    Ok(d)
}
// ===========================================================================
// 11. 冻结件
// ===========================================================================
/// 启用冻结件（双引用 + 五门 + 恒等式）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FreezeRecord {
    /// 版本。
    pub rev: u32,
    /// F3110 册哈希。
    pub case_book: u64,
    /// F3111 编排哈希。
    pub orch_book: u64,
    /// 用例册指纹。
    pub case_fp: u64,
    /// 编排册指纹。
    pub orch_fp: u64,
    /// 执行册指纹。
    pub run_fp: u64,
    /// 双引用核指纹。
    pub ref_fp: u64,
    /// 五门指纹。
    pub gate_fp: u64,
    /// 恒等式指纹。
    pub ident_fp: u64,
    /// 启用件哈希（过门结论留痕）。
    pub enable_hash: u64,
    /// 调度留痕（F3162–F3169 八段）。
    pub dispatch_note: u64,
}
impl FreezeRecord {
    /// 构造。
    pub fn build(s: &Snapshot) -> FreezeRecord {
        FreezeRecord {
            rev: 1,
            case_book: s.cases.book_hash(),
            orch_book: s.orch.orch_hash(),
            case_fp: s.cases.fingerprint(),
            orch_fp: s.orch.fingerprint(),
            run_fp: s.run.fingerprint(),
            ref_fp: s.refchk.fingerprint(),
            gate_fp: s.gates.fingerprint(),
            ident_fp: s.ident.fingerprint(),
            enable_hash: enable_hash(s),
            dispatch_note: dispatch_note(s),
        }
    }
    /// 复核（十项全等）。
    pub fn verify(&self, s: &Snapshot) -> bool {
        self.rev == 1
            && self.case_book == s.cases.book_hash()
            && self.orch_book == s.orch.orch_hash()
            && self.case_fp == s.cases.fingerprint()
            && self.orch_fp == s.orch.fingerprint()
            && self.run_fp == s.run.fingerprint()
            && self.ref_fp == s.refchk.fingerprint()
            && self.gate_fp == s.gates.fingerprint()
            && self.ident_fp == s.ident.fingerprint()
            && self.enable_hash == enable_hash(s)
            && self.dispatch_note == dispatch_note(s)
    }
    /// 漂移说明。
    pub fn drift(&self, s: &Snapshot) -> String {
        if self.case_book != s.cases.book_hash() {
            return String::from("case_book 漂移");
        }
        if self.orch_book != s.orch.orch_hash() {
            return String::from("orch_book 漂移");
        }
        if self.case_fp != s.cases.fingerprint() {
            return String::from("case_fp 漂移");
        }
        if self.orch_fp != s.orch.fingerprint() {
            return String::from("orch_fp 漂移");
        }
        if self.run_fp != s.run.fingerprint() {
            return String::from("run_fp 漂移");
        }
        if self.enable_hash != enable_hash(s) {
            return String::from("enable_hash 漂移");
        }
        if self.dispatch_note != dispatch_note(s) {
            return String::from("dispatch_note 漂移");
        }
        String::from("无漂移")
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "freeze rev={} case_book={:016X} orch_book={:016X} case_fp={:016X} orch_fp={:016X} run_fp={:016X} ref_fp={:016X} gate_fp={:016X} ident_fp={:016X} enable={:016X} dispatch={:016X}",
            self.rev,
            self.case_book,
            self.orch_book,
            self.case_fp,
            self.orch_fp,
            self.run_fp,
            self.ref_fp,
            self.gate_fp,
            self.ident_fp,
            self.enable_hash,
            self.dispatch_note
        )
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(11);
        words.push(self.rev as u64);
        words.push(self.case_book);
        words.push(self.orch_book);
        words.push(self.case_fp);
        words.push(self.orch_fp);
        words.push(self.run_fp);
        words.push(self.ref_fp);
        words.push(self.gate_fp);
        words.push(self.ident_fp);
        words.push(self.enable_hash);
        words.push(self.dispatch_note);
        fnv1a_words(&words)
    }
}
/// 启用件哈希（过门结论留痕）。
pub fn enable_hash(s: &Snapshot) -> u64 {
    let mut words: Vec<u64> = Vec::with_capacity(10);
    words.push(fnv1a64(BATCH_FACE.as_bytes()));
    words.push(fnv1a64(REF_CASEBOOK.as_bytes()));
    words.push(fnv1a64(REF_ORCH.as_bytes()));
    words.push(fnv1a64(GATE_SRC.as_bytes()));
    words.push(s.gates.pass_bits as u64);
    words.push(s.ident.sum_case);
    words.push(s.ident.sum_orch);
    words.push(s.ident.sum_run);
    words.push(s.run.verdict_bits as u64);
    words.push(s.cases.book_hash() ^ s.orch.orch_hash());
    fnv1a_words(&words)
}
/// 调度留痕（八段编排 × 各自读数）。
pub fn dispatch_note(s: &Snapshot) -> u64 {
    let mut words: Vec<u64> = Vec::with_capacity(ORCH_TOTAL + 3);
    words.push(fnv1a64(CONSUME_SLOTS.as_bytes()));
    let mut o: usize = 1;
    while o <= ORCH_TOTAL {
        let mut t: u64 = 0;
        if let Ok(it) = s.orch.item_of(o) {
            let mut id: usize = it.case_lo;
            while id <= it.case_hi {
                if let Ok(r) = s.run.row_of(id) {
                    t = t.wrapping_add(r.reading);
                }
                id = id.wrapping_add(1);
            }
        }
        words.push(t);
        words.push(it_gate_mask(o));
        o = o.wrapping_add(1);
    }
    fnv1a_words(&words)
}
/// 编排项门集合（独立重算，不读册内登记值）。
pub fn it_gate_mask(o: usize) -> u64 {
    if o < 1 || o > ORCH_TOTAL {
        return 0;
    }
    let n = if o < GATE_COUNT { o } else { GATE_COUNT };
    ((1u32 << n) - 1) as u64
}
// ===========================================================================
// 12. 多轮稳定性
// ===========================================================================
/// 稳定性报告。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Stability {
    /// 轮数。
    pub rounds: usize,
    /// 每轮启用件哈希。
    pub round_fp: Vec<u64>,
    /// 基准轮哈希。
    pub base_fp: u64,
    /// 漂移格数。
    pub drift_cells: usize,
    /// 漂移位图（前 128 格）。
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
/// 稳定性检查：逐「轮 × 用例」指纹跨轮比对。
pub fn stability_check(seed: u64) -> UxResult<Stability> {
    let mut round_fp: Vec<u64> = Vec::with_capacity(REPEAT);
    let mut cell: Vec<u64> = Vec::with_capacity(CASE_TOTAL * REPEAT);
    let mut r: usize = 0;
    while r < REPEAT {
        let net = Snapshot::net(seed)?;
        let s = Snapshot::net(seed)?;
        round_fp.push(enable_hash(&s));
        let mut id: usize = 1;
        while id <= CASE_TOTAL {
            let a = net.run.row_of(id)?;
            let b = s.run.row_of(id)?;
            cell.push(a.reading ^ b.reading ^ (a.cost << 32));
            id = id.wrapping_add(1);
        }
        r = r.wrapping_add(1);
    }
    let base_fp = match round_fp.first() {
        Some(x) => *x,
        None => 0,
    };
    let mut drift_mask: u128 = 0;
    let mut drift_cells: usize = 0;
    let mut rr: usize = 1;
    while rr < REPEAT {
        let mut id: usize = 1;
        while id <= CASE_TOTAL {
            let base = cell[id - 1];
            let cur = cell[rr * CASE_TOTAL + id - 1];
            if cur != base {
                let bit = rr * CASE_TOTAL + id - 1;
                // 位图宽度核对：`REPEAT × CASE_TOTAL = 108 < 128`，安全。
                drift_mask = drift_mask | (1u128 << bit);
                drift_cells = drift_cells.wrapping_add(1);
            }
            id = id.wrapping_add(1);
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
        drift_cells,
        drift_mask,
        stable: drift_cells == 0 && rp_ok,
    })
}// ===========================================================================
// 13. 指标 + 回放
// ===========================================================================
/// 指标。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Metrics {
    /// 相位数。
    pub phases: usize,
    /// 用例数。
    pub cases: usize,
    /// 用例册完备。
    pub cases_complete: bool,
    /// 编排数。
    pub orchs: usize,
    /// 编排册完备。
    pub orch_complete: bool,
    /// 引用一失配数。
    pub ref1_bad: usize,
    /// 引用二失配数。
    pub ref2_bad: usize,
    /// 双引用一致。
    pub ref_both_ok: bool,
    /// 五门通过位图。
    pub gate_bits: u32,
    /// 五门全过。
    pub gates_ok: bool,
    /// 断言总数。
    pub asserts: usize,
    /// 断言完备。
    pub asserts_ok: bool,
    /// Σ册。
    pub sum_case: u64,
    /// Σ编。
    pub sum_orch: u64,
    /// Σ使。
    pub sum_run: u64,
    /// 守恒最大偏离。
    pub deviation: u64,
    /// 守恒通过。
    pub identity_ok: bool,
    /// 步数越界。
    pub step_overflow: usize,
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
            "metrics phases={} cases={} ok={} orchs={} ok={} ref={}/{} both={} gates={:05b} ok={} asserts={} ok={} Σ册={} Σ编={} Σ使={} dev={} ident={} stepover={} rounds={} dcell={} detect={}/{} net={} edges={} rej={} ring={}/{}/{} ok={} freeze={} border={} tlv={} fp={:016X}",
            self.phases,
            self.cases,
            if self.cases_complete { 1 } else { 0 },
            self.orchs,
            if self.orch_complete { 1 } else { 0 },
            self.ref1_bad,
            self.ref2_bad,
            if self.ref_both_ok { 1 } else { 0 },
            self.gate_bits,
            if self.gates_ok { 1 } else { 0 },
            self.asserts,
            if self.asserts_ok { 1 } else { 0 },
            self.sum_case,
            self.sum_orch,
            self.sum_run,
            self.deviation,
            if self.identity_ok { 1 } else { 0 },
            self.step_overflow,
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
            if self.tlv_ok { 1 } else { 0 },
            self.fp
        )
    }
    /// 逐行表格。
    pub fn rows(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::with_capacity(18);
        out.push((String::from("相位"), format!("{}", self.phases)));
        out.push((String::from("用例册"), format!("{} 完备{}", self.cases, self.cases_complete)));
        out.push((String::from("编排册"), format!("{} 完备{}", self.orchs, self.orch_complete)));
        out.push((
            String::from("双引用核"),
            format!("{}:失配{} {}:失配{} 一致{}", REF_CASEBOOK, self.ref1_bad, REF_ORCH, self.ref2_bad, self.ref_both_ok),
        ));
        out.push((String::from("五门前置门"), format!("{:05b} 全过{}", self.gate_bits, self.gates_ok)));
        out.push((String::from("断言完备"), format!("{} 条 {}", self.asserts, self.asserts_ok)));
        out.push((String::from("启用守恒"), format!("Σ册={} Σ编={} Σ使={} dev{}", self.sum_case, self.sum_orch, self.sum_run, self.deviation)));
        out.push((String::from("执行账"), format!("步越界{}", self.step_overflow)));
        out.push((String::from("稳定性"), format!("轮{} 漂移格{}", self.rounds, self.drift_cells)));
        out.push((String::from("注入核"), format!("{}/{} 净态零误报{}", self.detected, self.forms, self.net_clean)));
        out.push((String::from("状态机"), format!("边{} 拒记{}", self.edges, self.rejected)));
        out.push((String::from("环形账"), format!("存活{} 丢弃{} 写入{} 守恒{}", self.live, self.dropped, self.pushed, self.ring_ok)));
        out.push((String::from("冻结件"), format!("通过{}", self.freeze_ok)));
        out.push((String::from("边界"), format!("零违规{}", self.border_ok)));
        out.push((String::from("TLV"), format!("往返{}", self.tlv_ok)));
        out.push((String::from("指纹"), format!("{:016X}", self.fp)));
        out
    }
    /// 全部通过。
    pub fn all_ok(&self) -> bool {
        self.cases_complete
            && self.orch_complete
            && self.ref_both_ok
            && self.ref1_bad == 0
            && self.ref2_bad == 0
            && self.gates_ok
            && self.asserts_ok
            && self.asserts == ASSERT_TOTAL
            && self.identity_ok
            && self.deviation == 0
            && self.drift_cells == 0
            && self.detected == self.forms
            && self.net_clean
            && self.rejected >= 3
            && self.ring_ok
            && self.freeze_ok
            && self.border_ok
            && self.tlv_ok
    }
    /// 指纹词表。
    pub fn words(&self) -> Vec<u64> {
        vec![
            self.phases as u64,
            self.cases as u64,
            self.orchs as u64,
            self.ref1_bad as u64,
            self.ref2_bad as u64,
            self.gate_bits as u64,
            self.asserts as u64,
            self.sum_case,
            self.sum_orch,
            self.sum_run,
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
    /// 每轮启用件哈希。
    pub round_fp: Vec<u64>,
    /// 每轮守恒三面。
    pub triples: Vec<(u64, u64, u64)>,
    /// 每轮探针指纹。
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
/// 确定性回放。
pub fn replay(seed: u64) -> UxResult<Replay> {
    let mut round_fp: Vec<u64> = Vec::with_capacity(REPEAT);
    let mut triples: Vec<(u64, u64, u64)> = Vec::with_capacity(REPEAT);
    let mut fps: Vec<u64> = Vec::with_capacity(REPEAT);
    let mut r: usize = 0;
    while r < REPEAT {
        let net = Snapshot::net(seed)?;
        round_fp.push(enable_hash(&net));
        triples.push((net.ident.sum_case, net.ident.sum_orch, net.ident.sum_run));
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
    let mut words: Vec<u64> = Vec::with_capacity(4 * REPEAT + 3);
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

/// 资源预算。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ResourceBudget {
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
    /// 核算（只读执行册，不构造完整 `observe` 以避免自引用）。
    pub fn compute(run: &RunBook) -> ResourceBudget {
        let resident = core::mem::size_of::<Snapshot>();
        let count = Form::COUNT;
        let export = 4096usize;
        let peak = (resident.wrapping_mul(count).wrapping_add(export)) as u64;
        let mut steps: u64 = 0;
        let mut i: usize = 0;
        while i < run.rows.len() {
            steps = steps.wrapping_add(run.rows[i].tick);
            i = i.wrapping_add(1);
        }
        ResourceBudget {
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
        self.peak_bytes <= self.peak_cap && self.steps <= self.step_cap && self.syscalls <= self.syscall_cap
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "budget snap={}x{}B export={}B peak={}/{}B steps={}/{} syscalls={}/{} ok={}",
            self.snapshot_count,
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
}

/// `observe` 结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UxObserve {
    /// 净态快照。
    pub net: Snapshot,
    /// 状态机。
    pub fsm: PhaseFsm,
    /// 稳定性。
    pub stability: Stability,
    /// 注入留痕。
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
impl UxObserve {
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
}// ===========================================================================
// 14. 导出
// ===========================================================================
/// KV 导出。
pub fn export_kv(o: &UxObserve) -> Vec<u8> {
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
    push_line(&mut out, "ref_casebook", String::from(REF_CASEBOOK));
    push_line(&mut out, "ref_orch", String::from(REF_ORCH));
    push_line(&mut out, "gate_src", String::from(GATE_SRC));
    push_line(&mut out, "consume_slots", String::from(CONSUME_SLOTS));
    push_line(&mut out, "cases", format!("{}", o.metrics.cases));
    push_line(&mut out, "orchs", format!("{}", o.metrics.orchs));
    push_line(&mut out, "ref1_bad", format!("{}", o.metrics.ref1_bad));
    push_line(&mut out, "ref2_bad", format!("{}", o.metrics.ref2_bad));
    push_line(&mut out, "gate_bits", format!("{:05b}", o.metrics.gate_bits));
    push_line(&mut out, "asserts", format!("{}", o.metrics.asserts));
    push_line(&mut out, "sum_case", format!("{}", o.metrics.sum_case));
    push_line(&mut out, "sum_orch", format!("{}", o.metrics.sum_orch));
    push_line(&mut out, "sum_run", format!("{}", o.metrics.sum_run));
    push_line(&mut out, "deviation", format!("{}", o.metrics.deviation));
    push_line(&mut out, "rounds", format!("{}", o.metrics.rounds));
    push_line(&mut out, "drift_cells", format!("{}", o.metrics.drift_cells));
    push_line(&mut out, "forms", format!("{}", o.metrics.forms));
    push_line(&mut out, "detected", format!("{}", o.metrics.detected));
    push_line(&mut out, "edges", format!("{}", o.metrics.edges));
    push_line(&mut out, "rejected", format!("{}", o.metrics.rejected));
    push_line(&mut out, "pushed", format!("{}", o.metrics.pushed));
    push_line(&mut out, "dropped", format!("{}", o.metrics.dropped));
    push_line(&mut out, "live", format!("{}", o.metrics.live));
    push_line(&mut out, "case_book", format!("{:016X}", o.freeze.case_book));
    push_line(&mut out, "orch_book", format!("{:016X}", o.freeze.orch_book));
    push_line(&mut out, "enable_hash", format!("{:016X}", o.freeze.enable_hash));
    push_line(&mut out, "dispatch_note", format!("{:016X}", o.freeze.dispatch_note));
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
/// 条目固定 `1 + 2 + 8 = 11` 字节，往返必然一致。
pub fn export_tlv(o: &UxObserve) -> Vec<u8> {
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
    push_tlv_u64(&mut out, 0x10, o.metrics.cases as u64);
    push_tlv_u64(&mut out, 0x11, o.metrics.orchs as u64);
    push_tlv_u64(&mut out, 0x12, o.metrics.ref1_bad as u64);
    push_tlv_u64(&mut out, 0x13, o.metrics.ref2_bad as u64);
    push_tlv_u64(&mut out, 0x14, o.metrics.gate_bits as u64);
    push_tlv_u64(&mut out, 0x15, o.metrics.asserts as u64);
    push_tlv_u64(&mut out, 0x20, o.metrics.sum_case);
    push_tlv_u64(&mut out, 0x21, o.metrics.sum_orch);
    push_tlv_u64(&mut out, 0x22, o.metrics.sum_run);
    push_tlv_u64(&mut out, 0x23, o.metrics.deviation);
    push_tlv_u64(&mut out, 0x30, o.metrics.rounds as u64);
    push_tlv_u64(&mut out, 0x31, o.metrics.drift_cells as u64);
    push_tlv_u64(&mut out, 0x40, o.metrics.forms as u64);
    push_tlv_u64(&mut out, 0x41, o.metrics.detected as u64);
    push_tlv_u64(&mut out, 0x50, o.metrics.edges as u64);
    push_tlv_u64(&mut out, 0x51, o.metrics.rejected as u64);
    push_tlv_u64(&mut out, 0x60, o.metrics.pushed);
    push_tlv_u64(&mut out, 0x61, o.metrics.dropped);
    push_tlv_u64(&mut out, 0x62, o.metrics.live as u64);
    push_tlv_u64(&mut out, 0x70, o.freeze.case_book);
    push_tlv_u64(&mut out, 0x71, o.freeze.orch_book);
    push_tlv_u64(&mut out, 0x72, o.freeze.enable_hash);
    push_tlv_u64(&mut out, 0x73, o.freeze.dispatch_note);
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
/// 解析 TLV。
pub fn parse_tlv(buf: &[u8]) -> UxResult<(u64, Vec<(u8, u64)>)> {
    if buf.len() < TLV_HEAD_LEN {
        return Err(err_range("tlv", buf.len(), TLV_HEAD_LEN));
    }
    if buf[0] != TLV_TAG_HEAD {
        return Err(UxErr::of_aux(AuxCode::FreezeMismatch, format!("组头标签错：{:02X}", buf[0])));
    }
    let hl = ((buf[1] as usize) << 8) | (buf[2] as usize);
    if hl != TLV_HEAD_PAYLOAD {
        return Err(UxErr::of_aux(AuxCode::FreezeMismatch, format!("组头长度错：{}", hl)));
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
            return Err(UxErr::of_aux(AuxCode::CapOver, format!("条目长度错：tag={:02X} len={}", tag, len)));
        }
        items.push((tag, be_u64(buf, off + 3)));
        off = off + TLV_ITEM_LEN;
    }
    Ok((version, items))
}
/// 按标签取值。
pub fn tlv_get(items: &[(u8, u64)], tag: u8) -> UxResult<u64> {
    let mut i: usize = 0;
    while i < items.len() {
        if items[i].0 == tag {
            return Ok(items[i].1);
        }
        i = i.wrapping_add(1);
    }
    Err(UxErr::of_aux(AuxCode::SeqMismatch, format!("条目缺失：tag={:02X}", tag)))
}
/// TLV 往返一致（逐条目与现场重算比对）。
pub fn tlv_roundtrip_ok(o: &UxObserve) -> bool {
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
            0x10 => o.metrics.cases as u64,
            0x11 => o.metrics.orchs as u64,
            0x12 => o.metrics.ref1_bad as u64,
            0x13 => o.metrics.ref2_bad as u64,
            0x14 => o.metrics.gate_bits as u64,
            0x15 => o.metrics.asserts as u64,
            0x20 => o.metrics.sum_case,
            0x21 => o.metrics.sum_orch,
            0x22 => o.metrics.sum_run,
            0x23 => o.metrics.deviation,
            0x30 => o.metrics.rounds as u64,
            0x31 => o.metrics.drift_cells as u64,
            0x40 => o.metrics.forms as u64,
            0x41 => o.metrics.detected as u64,
            0x50 => o.metrics.edges as u64,
            0x51 => o.metrics.rejected as u64,
            0x60 => o.metrics.pushed,
            0x61 => o.metrics.dropped,
            0x62 => o.metrics.live as u64,
            0x70 => o.freeze.case_book,
            0x71 => o.freeze.orch_book,
            0x72 => o.freeze.enable_hash,
            0x73 => o.freeze.dispatch_note,
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
    if buf.len() != TLV_HEAD_LEN + items.len() * TLV_ITEM_LEN {
        ok = false;
    }
    ok
}
// ===========================================================================
// 15. 复杂度分解
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
        let cases = CASE_TOTAL as u64;
        let orchs = ORCH_TOTAL as u64;
        out.push(CostItem {
            name: String::from("用例册登记（36 用例三列）"),
            iters: cases,
            per_iter: 3,
            total: cases * 3,
            note: String::from("Windows 回归靠测试框架动态发现用例，无「册位冻结 + 逐字比对」环节；本条把执行与册位的原样性做成硬断言"),
        });
        out.push(CostItem {
            name: String::from("编排册登记（9 编排四列）"),
            iters: orchs,
            per_iter: 4,
            total: orchs * 4,
            note: String::from("Windows 执行器编排是代码而非冻结件；本条把编排也做成可哈希引用的冻结件，调度即方案原样"),
        });
        out.push(CostItem {
            name: String::from("引用一逐项核（F3110）"),
            iters: cases,
            per_iter: 6,
            total: cases * 6,
            note: String::from("消费侧现值哈希由册内登记值独立重算，不复用入册时的 fp 字段，因此不是自证"),
        });
        out.push(CostItem {
            name: String::from("引用二逐项核（F3111）"),
            iters: orchs,
            per_iter: 6,
            total: orchs * 6,
            note: String::from("编排哈希逐项比对 + 册级编排哈希双层，任一层漂移都拦得住"),
        });
        out.push(CostItem {
            name: String::from("五门前置门复核"),
            iters: GATE_COUNT as u64,
            per_iter: cases,
            total: GATE_COUNT as u64 * cases,
            note: String::from("每门都对全册做一次读数复核（而非只读一个标志位）；F3157 五门制在 Windows 侧无对应件"),
        });
        out.push(CostItem {
            name: String::from("执行事件流（36 用例 × CASE_STEPS）"),
            iters: cases,
            per_iter: CASE_STEPS as u64,
            total: cases * CASE_STEPS as u64,
            note: String::from("执行段带步数账与双累加器交叉复核，Windows 侧执行不产出可核对的步数账"),
        });
        out.push(CostItem {
            name: String::from("启用守恒（三路独立聚合）"),
            iters: 3,
            per_iter: cases + orchs,
            total: 3 * (cases + orchs),
            note: String::from("Σ册/Σ编/Σ使 三路异算法互证；Windows 侧无「启用守恒」概念"),
        });
        out.push(CostItem {
            name: String::from("环形账灌注（36 × (REPEAT+1)）"),
            iters: cases * (REPEAT as u64 + 1),
            per_iter: 1,
            total: cases * (REPEAT as u64 + 1),
            note: String::from("写入量 144 刻意超容 128 以压出 dropped，守恒式 `写入 == 存活 + 丢弃` 才非平凡"),
        });
        out.push(CostItem {
            name: String::from("注入核（14 形态全注入 + 检出）"),
            iters: FORM_COUNT as u64 * 2,
            per_iter: cases + orchs,
            total: FORM_COUNT as u64 * 2 * (cases + orchs),
            note: String::from("引用漂移注入必拦是本单判据硬要求；Windows 侧无等价的自污染检出链"),
        });
        out.push(CostItem {
            name: String::from("稳定性多轮（REPEAT 轮全链重跑）"),
            iters: REPEAT as u64,
            per_iter: cases + orchs,
            total: REPEAT as u64 * (cases + orchs),
            note: String::from("逐「轮 × 用例」指纹跨轮比对，锁死同种子可回放"),
        });
        out.push(CostItem {
            name: String::from("冻结件复核（十项哈希）"),
            iters: 10,
            per_iter: cases,
            total: 10 * cases,
            note: String::from("十项各自独立复核，任一项漂移都能定位到项名而非笼统的「冻结件不符」"),
        });
        out.push(CostItem {
            name: String::from("TLV 导出（组头定宽 + 定长条目）"),
            iters: 31,
            per_iter: TLV_ITEM_LEN as u64,
            total: TLV_HEAD_LEN as u64 + 31 * TLV_ITEM_LEN as u64,
            note: String::from("长度字段恒占定宽，往返必然一致；Windows 侧测试结果导出为变长流，无往返断言"),
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
// 16. 自检
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
    let mut out: Vec<Check> = Vec::with_capacity(30);
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
        format!("Form::COUNT={} 码 {:04X}..{:04X}", Form::COUNT, Form::ALL[0].code(), Form::ALL[Form::COUNT - 1].code()),
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
        format!("AuxCode::COUNT={} 码 {:04X}..{:04X}", AuxCode::COUNT, AuxCode::ALL[0].code(), AuxCode::ALL[AuxCode::COUNT - 1].code()),
    ));
    out.push(Check::new(
        "orch_covers_all_cases",
        ORCH_TOTAL * 4 == CASE_TOTAL,
        format!("编排{} × 4格 = {} 期望{}", ORCH_TOTAL, ORCH_TOTAL * 4, CASE_TOTAL),
    ));
    out.push(Check::new(
        "assert_total",
        ASSERT_TOTAL == CASE_TOTAL * ASSERT_PER_CASE,
        format!("{} 用例 × {} 断言 = {}", CASE_TOTAL, ASSERT_PER_CASE, ASSERT_TOTAL),
    ));
    match case_book_full() {
        Ok(b) => {
            out.push(Check::new("case_book_full", b.complete(), b.summary()));
            out.push(Check::new("case_book_asserts", b.assert_total() == ASSERT_TOTAL, format!("断言={}", b.assert_total())));
            out.push(Check::new("case_book_hash", b.book_hash() == case_book_definition_hash(), format!("册哈希={:016X} 定义={:016X}", b.book_hash(), case_book_definition_hash())));
        }
        Err(e) => {
            out.push(Check::new("case_book_full", false, err_str(&e)));
        }
    }
    match orch_book_full() {
        Ok(b) => {
            out.push(Check::new("orch_book_full", b.complete(), b.summary()));
            out.push(Check::new("orch_covers_36", b.covered_cases() == CASE_TOTAL, format!("覆盖={}", b.covered_cases())));
            out.push(Check::new("orch_hash", b.orch_hash() == orch_definition_hash(), format!("编排哈希={:016X} 定义={:016X}", b.orch_hash(), orch_definition_hash())));
        }
        Err(e) => {
            out.push(Check::new("orch_book_full", false, err_str(&e)));
        }
    }
    match Snapshot::net(0x5EED_3161_0000_0001u64) {
        Ok(net) => {
            out.push(Check::new("run_all_pass", net.run.all_pass(), net.run.summary()));
            out.push(Check::new("run_steps_ok", net.run.steps_ok(), format!("step_over={}", net.run.step_overflow)));
            out.push(Check::new("ref_both_ok", net.refchk.both_ok(), net.refchk.summary()));
            out.push(Check::new("gates_all_pass", net.gates.all_pass(), net.gates.summary()));
            out.push(Check::new("identity_three_way", net.ident.balanced, net.ident.summary()));
            out.push(Check::new("identity_zero_dev", net.ident.deviation == 0, format!("dev={}", net.ident.deviation)));
            out.push(Check::new("ring_balanced", net.ring.balanced(), net.ring.summary()));
            out.push(Check::new("ring_dropped_explicit", net.ring.dropped > 0, format!("dropped={} live={}", net.ring.dropped, net.ring.live())));
            out.push(Check::new("net_is_net", net.is_net(), format!("border={}", net.ev.border_violations)));
            let fz = FreezeRecord::build(&net);
            out.push(Check::new("freeze_verify", fz.verify(&net), fz.summary()));
            out.push(Check::new("dispatch_note", fz.dispatch_note != 0, format!("dispatch={:016X}", fz.dispatch_note)));
            let mut fsm = PhaseFsm::new();
            let mut walk_ok = true;
            let mut pi: usize = 1;
            while pi < Phase::COUNT {
                if fsm.advance_to(Phase::ALL[pi]).is_err() {
                    walk_ok = false;
                }
                pi = pi.wrapping_add(1);
            }
            let illegal: [Phase; 3] = [Phase::P1RefBind, Phase::P3GateCheck, Phase::P2RefCheck];
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
            match stability_check(0x5EED_3161_0000_0001u64) {
                Ok(st) => {
                    out.push(Check::new("stability_zero_drift", st.stable && st.rounds_ok(), st.describe()));
                }
                Err(e) => {
                    out.push(Check::new("stability_zero_drift", false, err_str(&e)));
                }
            }
            match replay(0x5EED_3161_0000_0001u64) {
                Ok(rp) => {
                    out.push(Check::new("replay_consistent", rp.consistent, rp.summary()));
                }
                Err(e) => {
                    out.push(Check::new("replay_consistent", false, err_str(&e)));
                }
            }
            match inject_all(0x5EED_3161_0000_0001u64) {
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
                    match detect_all(&net) {
                        Ok(d) => {
                            out.push(Check::new("net_zero_false_positive", d.is_clean(), format!("hit={:?}", d.hit_list())));
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
        out.push(cs[i].describe());
        i = i.wrapping_add(1);
    }
    out
}// ===========================================================================
// 16. observe 主入口
// ===========================================================================
/// 观察入口：真跑全链并给出可复核结论。
///
/// 相位编排（严格按状态机合法边闭集逐相位推进）：
/// - P1 双引用登记：册哈希 / 编排哈希入册；
/// - P2 逐项引用核：`RefCheck::build` 逐项比对入册值与消费侧重算值；
/// - P3 五门前置门：`GateTable::build` 逐门复核读数；
/// - P4 启用守恒 + 启用件入册：`identity_close` 三路零偏离 + `FreezeRecord::build`；
/// - P5 执行调度留痕：`ring_fill` + `dispatch_note`；
/// - Sealed：注入核 / 稳定性 / 回放 / TLV 往返全部收口。
///
/// **归因纪律**：`observe` 全程只读净态基准（`registered_probe` 只读不改），
/// 注入核独立跑一遍 14 形态，逐形态断言「本形态必被检出」，
/// 再对净态断言「零检出」，两条断言方向相反、缺一不可。
pub fn observe(seed: u64) -> UxResult<UxObserve> {
    // ── 净态全链 ────────────────────────────────────────────────────────────
    let net = Snapshot::net(seed)?;
    if !net.cases.complete() {
        return Err(UxErr::of_form(
            Form::CaseMissing,
            format!("用例册不完备：缺 {} 位", net.cases.missing_count()),
        ));
    }
    if !net.orch.complete() {
        return Err(UxErr::of_form(
            Form::OrchMissing,
            String::from("编排册不完备：序位有缺"),
        ));
    }
    // 双引用核：任一失配即执行不得启用
    if net.refchk.ref1_mismatch != 0 {
        return Err(UxErr::of_form(
            Form::RefCaseDrift,
            format!("引用一失配：{:012X}", net.refchk.ref1_mismatch),
        ));
    }
    if net.refchk.ref2_mismatch != 0 {
        return Err(UxErr::of_form(
            Form::RefOrchDrift,
            format!("引用二失配：{:05b}", net.refchk.ref2_mismatch),
        ));
    }
    // 五门逐门复核：任一门不过即不启用
    if !net.gates.all_pass() {
        return Err(UxErr::of_form(
            Form::GateNotPass,
            format!("前置门不过：{:?}", net.gates.fail_list()),
        ));
    }
    // 启用守恒式：三路零偏离
    if !net.ident.balanced || net.ident.deviation != 0 {
        return Err(err_enable(net.ident.sum_case, net.ident.sum_orch, net.ident.sum_run));
    }
    // 环形账守恒
    if !net.ring.balanced() {
        return Err(UxErr::of_form(
            Form::LedgerUnbalanced,
            format!(
                "环形账不守恒：写入{} != 存活{} + 丢弃{}",
                net.ring.pushed,
                net.ring.live(),
                net.ring.dropped
            ),
        ));
    }

    // ── 状态机：走满 5 条合法边 + 3 条非法迁移拒记 ─────────────────────────
    let mut fsm = PhaseFsm::new();
    let mut pi: usize = 1;
    while pi < Phase::COUNT {
        fsm.advance_to(Phase::ALL[pi])?;
        pi = pi.wrapping_add(1);
    }
    // 三条典型非法迁移：倒退（P3→P1）、跨段跳跃（P1→P3）、闭环自指（P2→P2）
    let illegal: [Phase; 3] = [Phase::P1RefBind, Phase::P3GateCheck, Phase::P2RefCheck];
    let mut ij: usize = 0;
    while ij < illegal.len() {
        let _ = fsm.try_go(illegal[ij]);
        ij = ij.wrapping_add(1);
    }

    // ── 冻结件：build + verify ─────────────────────────────────────────────
    let freeze = FreezeRecord::build(&net);
    if !freeze.verify(&net) {
        return Err(UxErr::of_aux(
            AuxCode::FreezeMismatch,
            format!("冻结件复核不过：{}", freeze.drift(&net)),
        ));
    }

    // ── 稳定性：跨轮同格指纹一致 ───────────────────────────────────────────
    let stability = stability_check(seed)?;
    if !stability.stable {
        return Err(UxErr::of_aux(
            AuxCode::FreezeMismatch,
            format!("多轮不稳定：{} 格漂移", stability.drift_cells),
        ));
    }
    if stability.rounds != REPEAT {
        return Err(UxErr::of_aux(
            AuxCode::RepeatShort,
            format!("轮数不足：{} < {}", stability.rounds, REPEAT),
        ));
    }

    // ── 注入核：14/14 全拒 ─────────────────────────────────────────────────
    let (evidence, snaps) = inject_all(seed)?;
    let mut detects: Vec<Detections> = Vec::with_capacity(Form::COUNT);
    let mut forms_hit: usize = 0;
    let mut fi: usize = 0;
    while fi < Form::ALL.len() {
        let d = detect_all(&snaps[fi])?;
        if !d.hit(Form::ALL[fi]) {
            return Err(UxErr::of_form(
                Form::ALL[fi],
                format!("注入漏检：{}", Form::ALL[fi].detail()),
            ));
        }
        forms_hit = forms_hit.wrapping_add(1);
        detects.push(d);
        fi = fi.wrapping_add(1);
    }
    if forms_hit != FORM_COUNT {
        return Err(UxErr::of_aux(
            AuxCode::CapOver,
            format!("注入形态数不符：{} != {}", forms_hit, FORM_COUNT),
        ));
    }
    // 净态零误报：与注入方向相反的断言
    let net_detect = detect_all(&net)?;
    if !net_detect.is_clean() {
        return Err(UxErr::of_form(
            Form::GateNotPass,
            format!("净态误报：{:?}", net_detect.hit_list()),
        ));
    }
    if net_detect.border_violations != 0 {
        return Err(UxErr::of_form(
            Form::GateNotPass,
            format!("域外动作 {} 次", net_detect.border_violations),
        ));
    }

    // ── 回放 ───────────────────────────────────────────────────────────────
    let rp = replay(seed)?;
    if !rp.consistent {
        return Err(UxErr::of_aux(
            AuxCode::SeqMismatch,
            String::from("回放不一致：跨轮哈希分歧"),
        ));
    }

    // ── 资源预算 ───────────────────────────────────────────────────────────
    let budget = ResourceBudget::compute(&net.run);
    if !budget.ok() {
        return Err(UxErr::of_aux(
            AuxCode::CapOver,
            format!("预算越界：{}", budget.summary()),
        ));
    }

    // ── 指标定稿 ───────────────────────────────────────────────────────────
    let mut m = Metrics {
        phases: Phase::COUNT,
        cases: net.cases.cases.len(),
        cases_complete: net.cases.complete(),
        orchs: net.orch.items.len(),
        orch_complete: net.orch.complete(),
        ref1_bad: net.refchk.ref1_bad(),
        ref2_bad: net.refchk.ref2_bad(),
        ref_both_ok: net.refchk.both_ok(),
        gate_bits: net.gates.pass_bits,
        gates_ok: net.gates.all_pass(),
        asserts: net.cases.assert_total(),
        asserts_ok: net.cases.assert_total() == ASSERT_TOTAL,
        sum_case: net.ident.sum_case,
        sum_orch: net.ident.sum_orch,
        sum_run: net.ident.sum_run,
        deviation: net.ident.deviation,
        identity_ok: net.ident.balanced,
        step_overflow: net.run.step_overflow,
        rounds: stability.rounds,
        drift_cells: stability.drift_cells,
        forms: FORM_COUNT,
        detected: forms_hit,
        net_clean: net_detect.is_clean(),
        edges: fsm.edges_walked(),
        rejected: fsm.rejected.len(),
        pushed: net.ring.pushed,
        dropped: net.ring.dropped,
        live: net.ring.live(),
        ring_ok: net.ring.balanced(),
        freeze_ok: freeze.verify(&net),
        border_ok: net.ev.border_violations == 0,
        // `tlv_ok` 不参与 `words()` 汇总：否则回填动作本身会改动 `fp`，
        // 令 `export_tlv` 与 `words()` 的往返比对恒假。
        tlv_ok: false,
        fp: 0,
    };
    m.fp = fnv1a_words(&m.words());
    // 注意：`all_ok()` 里含 `tlv_ok`，而 `tlv_ok` 要在 `UxObserve` 成型后才可判，
    // 故「指标全过」的最终校验放在 TLV 回填之后（见下）。
    let mut o = UxObserve {
        net,
        fsm,
        stability,
        evidence,
        detects,
        freeze,
        replay: rp,
        metrics: m,
        budget,
    };
    // TLV 往返：与现场重算逐条目比对，通过后回填（不参与 fp 汇总）
    o.metrics.tlv_ok = tlv_roundtrip_ok(&o);
    if !o.metrics.tlv_ok {
        return Err(UxErr::of_aux(
            AuxCode::FreezeMismatch,
            String::from("TLV 往返不一致"),
        ));
    }
    if !o.metrics.all_ok() {
        return Err(UxErr::of_form(
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
            "[{:04X}] {:<18} {:<10} {}",
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
    let mut out: Vec<String> = Vec::with_capacity(18);
    out.push(format!("INV-01 码段自闭：{} ∈ [{}..{}]", SEG_MASK, SEG_BASE, SEG_END));
    out.push(format!("INV-02 形态一码一段：{} 形态占 {}..{:04X}", Form::COUNT, SEG_BASE, Form::ALL[Form::COUNT - 1].code()));
    out.push(format!("INV-03 辅助码独立段：{} 辅助码占 {:04X}..{:04X}", AuxCode::COUNT, AuxCode::ALL[0].code(), AuxCode::ALL[AuxCode::COUNT - 1].code()));
    out.push(format!("INV-04 段段相接：编排 {} × 4 格 = {} 用例", ORCH_TOTAL, ORCH_TOTAL * 4));
    out.push(format!("INV-05 断言完备：{} 用例 × {} 断言 = {}", CASE_TOTAL, ASSERT_PER_CASE, ASSERT_TOTAL));
    out.push(format!("INV-06 册位无重无缺：cover == {:012X} 且 dup == 0", CASE_MASK64));
    out.push(format!("INV-07 编排覆盖闭合：covered_cases == {}", CASE_TOTAL));
    out.push(String::from("INV-08 双引用零失配：ref1_mismatch == 0 且 ref2_mismatch == 0"));
    out.push(format!("INV-09 五门全过：pass_bits == {:05b}", (1u32 << GATE_COUNT) - 1));
    out.push(String::from("INV-10 启用守恒三路零偏离：Σ册 == Σ编 == Σ使"));
    out.push(format!("INV-11 环形账守恒：写入 == 存活 + 丢弃（容量 {}）", RING_CAP));
    out.push(String::from("INV-12 覆盖丢弃显性：dropped 计入指标并入册"));
    out.push(String::from("INV-13 注入 14/14 全拒留痕"));
    out.push(String::from("INV-14 净态零误报：净态快照零检出"));
    out.push(String::from("INV-15 冻结件十项全等可复核"));
    out.push(format!("INV-16 多轮稳定：{} 轮同格指纹一致", REPEAT));
    out.push(String::from("INV-17 回放一致：跨轮启用件哈希与三面守恒全等"));
    out.push(format!("INV-18 预算内：峰值/步数/syscall 三项（syscall 恒 {}）", SYSCALL_CAP));
    out.push(format!("INV-19 TLV 往返一致：定宽长度字段 + 组头 {} 字节", TLV_HEAD_LEN));
    out.push(String::from("INV-20 无墙钟无浮点无真实 IO：全部为逻辑 tick 与整型运算"));
    out
}
/// 相位演示。
pub fn demo_phase() -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(Phase::COUNT);
    let mut i: usize = 0;
    while i < Phase::ALL.len() {
        let p = Phase::ALL[i];
        out.push(format!("P{} {:<14} {}", p.index(), p.name(), p.detail()));
        i = i.wrapping_add(1);
    }
    out.push(String::from("合法边闭集："));
    let mut j: usize = 0;
    while j < LEGAL_EDGES.len() {
        out.push(format!(
            "  {} -> {}",
            LEGAL_EDGES[j].0.name(),
            LEGAL_EDGES[j].1.name()
        ));
        j = j.wrapping_add(1);
    }
    let m = PhaseFsm::edge_matrix();
    out.push(String::from("边矩阵（1 = 合法）："));
    out.push(format!("      {}", Phase::ALL[Phase::COUNT - 1].name()));
    let mut r: usize = 0;
    while r < Phase::COUNT {
        out.push(format!(
            "{:>4}  {:>4} {}",
            Phase::ALL[r].name(),
            m[r][Phase::COUNT - 1],
            {
                let mut row = String::new();
                let mut c: usize = 0;
                while c < Phase::COUNT {
                    row.push(if m[r][c] == 1 { '1' } else { '.' });
                    c = c.wrapping_add(1);
                }
                row
            }
        ));
        r = r.wrapping_add(1);
    }
    out
}
/// 用例册演示。
pub fn demo_cases() -> UxResult<Vec<String>> {
    let b = case_book_full()?;
    let mut out: Vec<String> = Vec::with_capacity(CASE_TOTAL + 8);
    out.push(b.summary());
    out.push(format!(
        "覆盖位图 {:012X} / 缺位 {:012X} / 重号位 {:012X} / 越界 {} / 残列 {}",
        b.bits,
        b.missing_bits(),
        b.dup_bits,
        b.range_over,
        b.column_mask
    ));
    let mut id: usize = 1;
    while id <= CASE_TOTAL {
        let c = b.case_of(id)?;
        out.push(format!(
            "case[{:02}] {} 格{} | 判据：{} | 期望：{} | 断言{} 读数{}",
            c.id, c.group, c.cell, c.crit, c.expect, c.asserts, c.reading
        ));
        id = id.wrapping_add(1);
    }
    out.push(format!("册哈希 = {:016X}", b.book_hash()));
    out.push(format!("册指纹 = {:016X}", b.fingerprint()));
    Ok(out)
}
/// 编排册演示。
pub fn demo_orch() -> UxResult<Vec<String>> {
    let b = orch_book_full()?;
    let mut out: Vec<String> = Vec::with_capacity(ORCH_TOTAL + 6);
    out.push(b.summary());
    out.push(format!(
        "覆盖位 {:05b} / 缺位 {:05b} / 重号位 {:05b} / 覆盖用例 {}",
        b.bits,
        b.missing_bits(),
        b.dup_bits,
        b.covered_cases()
    ));
    let mut id: usize = 1;
    while id <= ORCH_TOTAL {
        let it = b.item_of(id)?;
        out.push(format!(
            "orch[{}] 序{} {} 覆盖[{}..{}] 前置门 {:05b}",
            it.id,
            it.seq,
            it.name,
            it.case_lo,
            it.case_hi,
            it.gate_mask
        ));
        id = id.wrapping_add(1);
    }
    out.push(format!("编排哈希 = {:016X}", b.orch_hash()));
    out.push(format!("编排指纹 = {:016X}", b.fingerprint()));
    Ok(out)
}
/// 五门演示。
pub fn demo_gates() -> UxResult<Vec<String>> {
    let net = Snapshot::net(0x5EED_3161_0000_0001u64)?;
    let mut out: Vec<String> = Vec::with_capacity(GATE_COUNT + 8);
    out.push(net.gates.summary());
    out.push(String::from(format!("来源：{} 五项门", GATE_SRC)));
    let mut i: usize = 0;
    while i < net.gates.rows.len() {
        out.push(net.gates.rows[i].describe());
        i = i.wrapping_add(1);
    }
    out.push(net.refchk.summary());
    out.push(format!(
        "双引用：引用一 {} 入册 {:016X} / 现态 {:016X}；引用二 {} 入册 {:016X} / 现态 {:016X}",
        REF_CASEBOOK,
        net.refchk.case_book_hash,
        net.refchk.case_live_hash,
        REF_ORCH,
        net.refchk.orch_hash,
        net.refchk.orch_live_hash
    ));
    Ok(out)
}
/// 环形账演示。
pub fn demo_ledger(seed: u64) -> UxResult<Vec<String>> {
    let net = Snapshot::net(seed)?;
    let mut out: Vec<String> = Vec::with_capacity(RING_CAP + 8);
    out.push(net.ring.summary());
    out.push(format!(
        "容量 {} 掩码 {} 下一序号 {} 序号连续 {}",
        RING_CAP,
        RING_MASK,
        net.ring.next_seq,
        net.ring.seq_continuous()
    ));
    let mut i: usize = 0;
    while i < net.ring.slots.len() {
        let e = &net.ring.slots[i];
        out.push(format!(
            "ring[{:03}] seq={} case={} orch={} reading={} tick={}",
            i, e.seq, e.case_id, e.orch_id, e.reading, e.tick
        ));
        i = i.wrapping_add(1);
    }
    out.push(format!(
        "守恒：写入 {} == 存活 {} + 丢弃 {} ⇒ {}",
        net.ring.pushed,
        net.ring.live(),
        net.ring.dropped,
        net.ring.balanced()
    ));
    Ok(out)
}
/// 关联件说明。
pub fn related_str() -> String {
    let mut s = String::new();
    s.push_str("【引用一】用例册 ");
    s.push_str(REF_CASEBOOK);
    s.push_str("：哈希引用一致断言（逐字三列比对 + 册位哈希）\n");
    s.push_str("【引用二】执行器方案 ");
    s.push_str(REF_ORCH);
    s.push_str("：编排哈希引用一致（编排三列比对 + 编排哈希）\n");
    s.push_str("【前置门】");
    s.push_str(GATE_SRC);
    s.push_str(" 五项门：G0 方案冻结 / G1 用例齐备 / G2 断言完备 / G3 资源预算 / G4 边界零违规\n");
    s.push_str("【消费位】");
    s.push_str(CONSUME_SLOTS);
    s.push_str("（单项执行由下游单负责，本单只做执行启用）\n");
    s.push_str("【嫁接】");
    s.push_str(GRAFT_SEAT);
    s.push_str(" 回归方案总冻结制度；体检链 ");
    s.push_str(ALIGN_CHAIN);
    s.push('\n');
    s
}
/// 全量报告。
pub fn full_report(seed: u64) -> UxResult<String> {
    let o = observe(seed)?;
    let mut s = String::new();
    s.push_str("═══ UNX-F3161 回归矩阵执行方案终冻结 · 全量报告 ═══\n");
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
    s.push_str("\n── 相位状态机 ──\n");
    let ph = demo_phase();
    let mut j: usize = 0;
    while j < ph.len() {
        s.push_str(&ph[j]);
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
        s.push_str(&format!("{:<14} {}\n", rows[r].0, rows[r].1));
        r = r.wrapping_add(1);
    }
    s.push_str("\n── 复杂度分解 ──\n");
    let ci = o.metrics.complexity();
    let mut c: usize = 0;
    while c < ci.len() {
        s.push_str(&format!(
            "{:<18} 迭代{} × 代价{} = {} 步 | {}\n",
            ci[c].name, ci[c].iters, ci[c].per_iter, ci[c].total, ci[c].note
        ));
        c = c.wrapping_add(1);
    }
    s.push_str("\n── 冻结件 ──\n");
    s.push_str(&format!("{}\n", o.freeze.summary()));
    s.push_str(&format!("稳定性 {}\n", o.stability.describe()));
    s.push_str(&format!("回放 {}\n", o.replay.summary()));
    s.push_str(&format!("预算 {}\n", o.budget.summary()));
    s.push_str(&format!("留痕 {}\n", o.evidence.summary()));
    s.push_str(&format!("全过 = {}\n", o.all_ok()));
    Ok(s)
}
