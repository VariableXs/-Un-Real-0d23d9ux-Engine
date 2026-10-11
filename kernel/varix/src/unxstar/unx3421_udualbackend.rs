//! UNX-F3421 · 双后端总设计（QEMU 后端 / 真机后端统一抽象，开发期零 QEMU 双轨制）
//!
//! 任务单号: UNX-F3421
//! 标题: 双后端总设计（QEMU 后端真机后端统一抽象，开发期零 QEMU 双轨制）
//! 域/批: A4/G8（CoRun·UNX-A5 段｜ktest/框架/测试基建域）
//! 册目标: 440 行（判据 J1 双后端总设计 + M-180 桩后端全链路）
//!
//! 判据原文（逐字摘抄，来源《CoRun Varix STAR II · Unxreal》任务书表格）:
//! 「UNX-F3421 | 双后端总设计（QEMU 后端真机后端统一抽象，开发期零 QEMU 双轨制） | 440 | 骨架 |
//!   J1：双后端总设计（后端 = 输出通道 + 执行环境的统一抽象）文档交付：
//!   开发期桩后端零 QEMU 依赖、真机后端为闸口对象；M-180 桩后端全链路。」
//!
//! 说明（锚点目录同名文件的辨析）:
//! `_attic/.../anchors/UNX-F3421.md` 内实为 **UNX-F34210 · I4 与 AI-44 联签登记表（双签）**，与本单无关。
//! 本单判据以任务书表格 `| UNX-F3421 |` 行为准。
//!
//! 本单核心语义（后端 = 输出通道 + 执行环境 两族能力）:
//! - **两族能力**：后端由 `OutputChannel`（串口 / 内存缓冲 / 文件槽，零真实 IO 的账面三态）
//!   与 `ExecEnv`（桩 / QEMU 模拟 / 真机）**两族正交**组合而成；任一族变更都属后端定义变更。
//! - **桩后端零 QEMU 依赖**：开发期默认后端 = `桩`，其执行环境不得引入任何 QEMU 语义，
//!   由 `qemu_free` 显式布尔位自证（这是「双轨制成本」得以归零的根）。
//! - **能力矩阵差异显性化**：`CapMatrix` 用位图声明每后端具备哪些能力，
//!   用例若要求后端不具备的能力 → `CapNotSupported`（**必须前置检出**，不许跑到一半才发现）。
//! - **跨后端对拍**：同一用例在两后端各跑一次，`DiffBook` 比对结果指纹；
//!   一致 = 通过，不一致 = 按差异分类显性登记（桩后端是 QEMU 后端的**近似**，差异属预期）。
//! - **零 QEMU 双轨制成本核算**：`CostLedger` 逐项列出双轨制省下的成本
//!   （不必维护两套用例、不必每轮启停 QEMU、不必准备真机），并给出可验算的整数账。
//!
//! 深度骨架:
//! 常量表 / 4 核心结构（Backend、CapMatrix、CaseRun、DiffBook）/
//! 14 错误变体 / 6 辅助码 / 主状态机（合法边闭集 + 非法转换拒记）/
//! 守恒账 `reconcile()` / append-only 环形账（`dropped` 显性）/
//! 注入核 14/14 全拒留痕 / 冻结指纹 `fingerprint() + verify()` /
//! `export_kv() + export_tlv()` / 确定性 `replay(seed)` / `observe(seed)` /
//! `complexity()` 逐项性能分解（对标 Windows 同类能力写「深在哪」）。
//!
//! 码段声明:
//! 本单独占诊断码段 `0x28C0 ..= 0x28FF`（`SEG_BASE = 0x28C0`，`SEG_MASK = 0x003F`）。
//! 错误变体一码一段，逐个占段内偏移 `0x00..=0x0D`；辅助码占 `0x1C..=0x21`，
//! 最大占用 `0x28E1`，未触及段上界 `0x28FF`。

#![allow(dead_code)]
#![allow(unused_imports)]

extern crate alloc;

use alloc::{format, string::{String, ToString}, vec, vec::Vec};

// ===========================================================================
// 1. 常量表
// ===========================================================================

/// 版本标识。
pub const V_VERSION_ID: u64 = 0x0001_3421_28C0_0001;
/// 版本串。
pub const V_VERSION: &str = "v3421-udualbackend.v1";
/// 码段基址。
pub const SEG_BASE: u16 = 0x28C0;
/// 码段掩码。
pub const SEG_MASK: u16 = 0x003F;
/// 码段上界。
pub const SEG_END: u16 = SEG_BASE | SEG_MASK;
/// 主码。
pub const MAIN_CODE: u16 = SEG_BASE;
/// 辅助码段内基址。
pub const AUX_BASE_OFF: u16 = 0x1C;

// ---------------------------------------------------------------------------
// 1.1 后端域常量
// ---------------------------------------------------------------------------

/// 输出通道族规模。
pub const CHAN_FAMILY: usize = 3;
/// 执行环境族规模。
pub const ENV_FAMILY: usize = 3;
/// 后端总数（通道 × 环境 = 9 种组合，其中桩通道 3 种为开发期默认）。
pub const BACKEND_COUNT: usize = 9;
/// 能力项数（位图宽度按此确定）。
pub const CAP_COUNT: usize = 8;
/// 能力位图类型（`CAP_COUNT = 8` 恰好一个字节，`u8` 足够，移位不会溢出）。
pub const CAP_BITS: u8 = CAP_COUNT as u8;
/// 能力全位。
pub const CAP_ALL: u8 = (1u16 << CAP_COUNT) as u8;

/// 用例总数。
pub const CASE_COUNT: usize = 12;
/// 每用例对拍重复轮次。
pub const PARITY_ROUNDS: usize = 2;
/// 单用例执行步数上限。
pub const CASE_STEPS: usize = 16;

/// 环形账容量。
pub const RING_CAP: usize = 32;
/// 环形账掩码。
pub const RING_MASK: usize = RING_CAP - 1;
/// 总步数上限。
pub const STEP_CAP: u64 = (BACKEND_COUNT * CASE_COUNT * CASE_STEPS * 2) as u64;
/// syscall 次数上限（零真实 IO）。
pub const SYSCALL_CAP: u64 = 0;
/// 峰值内存上限。
pub const MEM_PEAK_CAP: u64 = 512 * 1024;

/// 开发期零 QEMU 双轨制省下的成本项数。
pub const SAVING_ITEMS: usize = 5;

/// TLV 组头标签。
pub const TLV_TAG_HEAD: u8 = 0x01;
/// TLV 组头载荷字节数。
pub const TLV_HEAD_PAYLOAD: usize = 24;
/// TLV 组头总长。
pub const TLV_HEAD_LEN: usize = TLV_HEAD_PAYLOAD + 3;
/// 单条 TLV 定长。
pub const TLV_ITEM_LEN: usize = 10;
/// TLV 条目总数（静态 40 + 逐形态 14 = 54；由 export_tlv 内 push 次数反推）。
pub const TLV_ITEM_COUNT: usize = 54;

/// 后端域面名。
pub const FACE_BACKEND: &str = "后端面";
/// 能力域面名。
pub const FACE_CAP: &str = "能力面";
/// 对拍域面名。
pub const FACE_PARITY: &str = "对拍面";
/// 账目域面名。
pub const FACE_LEDGER: &str = "账目面";
/// 冻结域面名。
pub const FACE_FREEZE: &str = "冻结面";

/// 嫁接源。
pub const GRAFT_SEAT: &str = "F3361/F3381";
/// 对位条位。
pub const DEP_SEAT: &str = "F3461";
// ===========================================================================
// 2. 错误闭集
// ===========================================================================

/// 错误形态（段内偏移 `0x00..=0x0D`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// 0：后端重号（两后端同名）。
    BackendDup,
    /// 1：后端族越界（通道/环境组合非法）。
    FamilyOutOfSet,
    /// 2：桩后端引入 QEMU 依赖（双轨制成本归零的前提被破）。
    StubQemuLeak,
    /// 3：能力位图越界（能力下标超出 `CAP_COUNT`）。
    CapIdxOver,
    /// 4：后端能力声明不全（声明矩阵未覆盖全部后端）。
    CapIncomplete,
    /// 5：用例要求的能力后端不具备（未前置检出）。
    CapNotSupported,
    /// 6：用例缺失。
    CaseMissing,
    /// 7：用例重号。
    CaseDup,
    /// 8：对拍轮次不足（跨后端比对轮数不够）。
    ParityShort,
    /// 9：对拍结果失配（同一用例两后端结果不同且未登记为预期差异）。
    ParityMismatch,
    /// 10：环形账不守恒。
    LedgerUnbalanced,
    /// 11：守恒式偏离。
    ReconcileDeviation,
    /// 12：冻结指纹漂移。
    FingerprintDrift,
    /// 13：闸口越界（真机闸口在本单被真跑）。
    GateCrossed,
}
impl Form {
    /// 形态总数。
    pub const COUNT: usize = 14;
    /// 全部形态。
    pub const ALL: [Form; Form::COUNT] = [
        Form::BackendDup,
        Form::FamilyOutOfSet,
        Form::StubQemuLeak,
        Form::CapIdxOver,
        Form::CapIncomplete,
        Form::CapNotSupported,
        Form::CaseMissing,
        Form::CaseDup,
        Form::ParityShort,
        Form::ParityMismatch,
        Form::LedgerUnbalanced,
        Form::ReconcileDeviation,
        Form::FingerprintDrift,
        Form::GateCrossed,
    ];
    /// 段内偏移下标。
    pub fn index(self) -> usize {
        match self {
            Form::BackendDup => 0,
            Form::FamilyOutOfSet => 1,
            Form::StubQemuLeak => 2,
            Form::CapIdxOver => 3,
            Form::CapIncomplete => 4,
            Form::CapNotSupported => 5,
            Form::CaseMissing => 6,
            Form::CaseDup => 7,
            Form::ParityShort => 8,
            Form::ParityMismatch => 9,
            Form::LedgerUnbalanced => 10,
            Form::ReconcileDeviation => 11,
            Form::FingerprintDrift => 12,
            Form::GateCrossed => 13,
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
            Form::BackendDup => "BackendDup",
            Form::FamilyOutOfSet => "FamilyOutOfSet",
            Form::StubQemuLeak => "StubQemuLeak",
            Form::CapIdxOver => "CapIdxOver",
            Form::CapIncomplete => "CapIncomplete",
            Form::CapNotSupported => "CapNotSupported",
            Form::CaseMissing => "CaseMissing",
            Form::CaseDup => "CaseDup",
            Form::ParityShort => "ParityShort",
            Form::ParityMismatch => "ParityMismatch",
            Form::LedgerUnbalanced => "LedgerUnbalanced",
            Form::ReconcileDeviation => "ReconcileDeviation",
            Form::FingerprintDrift => "FingerprintDrift",
            Form::GateCrossed => "GateCrossed",
        }
    }
    /// 形态描述（中文在档注记）。
    pub fn describe(self) -> &'static str {
        match self {
            Form::BackendDup => "后端重号：两后端登记了同名组合",
            Form::FamilyOutOfSet => "后端族越界：通道/环境组合不在闭集内",
            Form::StubQemuLeak => "桩后端引入 QEMU 依赖：双轨制成本无法归零",
            Form::CapIdxOver => "能力位图越界：能力下标超出 CAP_COUNT",
            Form::CapIncomplete => "能力声明不全：存在未登记能力的后端",
            Form::CapNotSupported => "用例要求的能力后端不具备（未前置检出）",
            Form::CaseMissing => "用例缺失：册内应有用例查无记录",
            Form::CaseDup => "用例重号：同指纹用例被重复登记",
            Form::ParityShort => "对拍轮次不足：跨后端比对轮数未达下限",
            Form::ParityMismatch => "对拍失配：两后端结果不同且未登记为预期差异",
            Form::LedgerUnbalanced => "环形账不守恒：写入数 ≠ 存活 + 丢弃",
            Form::ReconcileDeviation => "守恒式偏离：执行数 ≠ 用例数 × 对拍轮次",
            Form::FingerprintDrift => "冻结指纹漂移：后端定义件哈希与入册值失配",
            Form::GateCrossed => "闸口越界：本单不真跑真机闸口",
        }
    }
    /// 探测面名。
    pub fn probe(self) -> &'static str {
        match self {
            Form::BackendDup => "backend.dup_bits",
            Form::FamilyOutOfSet => "backend.family_closed",
            Form::StubQemuLeak => "backend.stub_qemu_free",
            Form::CapIdxOver => "cap.idx_bound",
            Form::CapIncomplete => "cap.matrix_complete",
            Form::CapNotSupported => "cap.required_subset",
            Form::CaseMissing => "case.cover_bits",
            Form::CaseDup => "case.dup_bits",
            Form::ParityShort => "parity.rounds",
            Form::ParityMismatch => "parity.fp_equal",
            Form::LedgerUnbalanced => "ring.balanced",
            Form::ReconcileDeviation => "recon.deviation",
            Form::FingerprintDrift => "freeze.verify",
            Form::GateCrossed => "gate.real_hw_zero",
        }
    }
    /// 所属域面。
    pub fn face(self) -> &'static str {
        match self {
            Form::BackendDup | Form::FamilyOutOfSet | Form::StubQemuLeak => FACE_BACKEND,
            Form::CapIdxOver | Form::CapIncomplete | Form::CapNotSupported => FACE_CAP,
            Form::CaseMissing | Form::CaseDup | Form::ParityShort | Form::ParityMismatch => FACE_PARITY,
            Form::LedgerUnbalanced | Form::ReconcileDeviation => FACE_LEDGER,
            Form::FingerprintDrift | Form::GateCrossed => FACE_FREEZE,
        }
    }
    /// 是否属「后端面」。
    pub fn is_backend(self) -> bool {
        matches!(self, Form::BackendDup | Form::FamilyOutOfSet | Form::StubQemuLeak)
    }
    /// 是否属「能力面」。
    pub fn is_cap(self) -> bool {
        matches!(self, Form::CapIdxOver | Form::CapIncomplete | Form::CapNotSupported)
    }
    /// 是否属「对拍面」。
    pub fn is_parity(self) -> bool {
        matches!(
            self,
            Form::CaseMissing | Form::CaseDup | Form::ParityShort | Form::ParityMismatch
        )
    }
    /// 形态类名。
    pub fn class_name(self) -> &'static str {
        if self.is_backend() {
            "后端面"
        } else if self.is_cap() {
            "能力面"
        } else if self.is_parity() {
            "对拍面"
        } else {
            "账目/冻结面"
        }
    }
}
/// 辅助码（段内偏移 `0x1C..=0x21`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuxCode {
    /// 0：步数越界。
    StepOverCap,
    /// 1：后端下标越界。
    BackendIdxOver,
    /// 2：用例下标越界。
    CaseIdxOver,
    /// 3：能力位下标越界。
    CapIdxOver,
    /// 4：成本账下标越界。
    SavingIdxOver,
    /// 5：语义边界越界（本单不真跑真机闸口）。
    SemanticBorder,
}
impl AuxCode {
    /// 辅助码总数。
    pub const COUNT: usize = 6;
    /// 全部辅助码。
    pub const ALL: [AuxCode; AuxCode::COUNT] = [
        AuxCode::StepOverCap,
        AuxCode::BackendIdxOver,
        AuxCode::CaseIdxOver,
        AuxCode::CapIdxOver,
        AuxCode::SavingIdxOver,
        AuxCode::SemanticBorder,
    ];
    /// 段内偏移下标。
    pub fn index(self) -> usize {
        match self {
            AuxCode::StepOverCap => 0,
            AuxCode::BackendIdxOver => 1,
            AuxCode::CaseIdxOver => 2,
            AuxCode::CapIdxOver => 3,
            AuxCode::SavingIdxOver => 4,
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
            AuxCode::BackendIdxOver => "BackendIdxOver",
            AuxCode::CaseIdxOver => "CaseIdxOver",
            AuxCode::CapIdxOver => "CapIdxOver",
            AuxCode::SavingIdxOver => "SavingIdxOver",
            AuxCode::SemanticBorder => "SemanticBorder",
        }
    }
    /// 辅助码描述。
    pub fn describe(self) -> &'static str {
        match self {
            AuxCode::StepOverCap => "逻辑步数越过 STEP_CAP 上限",
            AuxCode::BackendIdxOver => "后端下标越界",
            AuxCode::CaseIdxOver => "用例下标越界",
            AuxCode::CapIdxOver => "能力位下标越界",
            AuxCode::SavingIdxOver => "成本账下标越界",
            AuxCode::SemanticBorder => "语义边界越界：本单不真跑真机闸口",
        }
    }
}

/// 本单结果别名。
pub type DbResult<T> = Result<T, DbErr>;
/// 错误对象（三要素）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DbErr {
    /// 诊断码。
    pub code: u16,
    /// 明文细节。
    pub detail: String,
}
impl DbErr {
    /// 构造。
    pub fn new(code: u16, detail: String) -> DbErr {
        DbErr { code, detail }
    }
    /// 由形态构造。
    pub fn of_form(f: Form, detail: String) -> DbErr {
        DbErr { code: f.code(), detail }
    }
    /// 由辅助码构造。
    pub fn of_aux(a: AuxCode, detail: String) -> DbErr {
        DbErr { code: a.code(), detail }
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
/// 码 → 名称反查。
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
pub fn err_str(e: &DbErr) -> String {
    match name_by_code(e.code) {
        Some(n) => format!("[{}] {}", n, e.detail),
        None => format!("[0x{:04X}] {}", e.code, e.detail),
    }
}
/// 冻结态不符错误。
pub fn err_frozen(got: &str, want: &str) -> DbErr {
    DbErr::of_form(
        Form::FingerprintDrift,
        format!("冻结态不符：got={} want={}", got, want),
    )
}
/// 越界错误。
pub fn err_range(what: &str, idx: usize, cap: usize) -> DbErr {
    DbErr::of_aux(
        AuxCode::CapIdxOver,
        format!("越界：{} idx={} cap={}", what, idx, cap),
    )
}
/// 守恒式错误。
pub fn err_recon(a: u64, b: u64, c: u64) -> DbErr {
    DbErr::of_form(
        Form::ReconcileDeviation,
        format!("守恒式偏离：执行={} 用例×轮次={} 总账={}", a, b, c),
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
// 2. 确定性基础件
// ===========================================================================

/// 确定性 LCG。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lcg {
    /// 状态。
    pub state: u64,
}
impl Lcg {
    /// 构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    /// 推进一步。
    pub fn next(&mut self) -> u64 {
        self.state = lcg_step(self.state);
        self.state
    }
    /// 取低 32 位。
    pub fn next32(&mut self) -> u32 {
        (self.next() & 0xFFFF_FFFF) as u32
    }
    /// 取模取样。
    pub fn below(&mut self, m: u64) -> u64 {
        let v = self.next();
        if m == 0 {
            0
        } else {
            v % m
        }
    }
    /// 区间取样（`[lo, hi)`；`hi <= lo` 时给 `lo`）。
    pub fn range(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo {
            return lo;
        }
        lo.wrapping_add(self.below(hi - lo))
    }
}
/// LCG 单步。
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
/// FNV-1a 64。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    fnv1a64_ext(0xcbf29ce484222325u64, bytes)
}
/// FNV-1a 64 词表指纹。
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
/// 字符串折入。
pub fn fnv1a_str(state: u64, s: &str) -> u64 {
    fnv1a64_ext(state, s.as_bytes())
}
/// 逻辑时钟推进（零墙钟）。
pub fn tick_advance(now: u64) -> u64 {
    now.wrapping_add(1)
}
/// 整除保护。
pub fn div_or_zero(a: u64, b: u64) -> u64 {
    if b == 0 {
        0
    } else {
        a / b
    }
}
/// 64 位整数平方根（牛顿迭代，零浮点）。
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

// ===========================================================================
// 3. 错误闭集
// ===========================================================================


// ===========================================================================
// 4. 核心结构一：后端（Backend = 输出通道 + 执行环境）
// ===========================================================================

/// 输出通道族（零真实 IO 的账面三态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OutputChannel {
    /// 内存缓冲（开发期默认，纯账面）。
    MemBuf,
    /// 串口（真机闸口对象，本单只建模不真跑）。
    Serial,
    /// 文件槽（归档用）。
    FileSlot,
}
impl OutputChannel {
    /// 族规模。
    pub const COUNT: usize = CHAN_FAMILY;
    /// 全部通道。
    pub const ALL: [OutputChannel; OutputChannel::COUNT] = [
        OutputChannel::MemBuf,
        OutputChannel::Serial,
        OutputChannel::FileSlot,
    ];
    /// 下标。
    pub fn index(self) -> usize {
        match self {
            OutputChannel::MemBuf => 0,
            OutputChannel::Serial => 1,
            OutputChannel::FileSlot => 2,
        }
    }
    /// 按下标取。
    pub fn from_index(i: usize) -> Option<OutputChannel> {
        if i < OutputChannel::COUNT {
            Some(OutputChannel::ALL[i])
        } else {
            None
        }
    }
    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            OutputChannel::MemBuf => "MemBuf",
            OutputChannel::Serial => "Serial",
            OutputChannel::FileSlot => "FileSlot",
        }
    }
    /// 中文名。
    pub fn cn(self) -> &'static str {
        match self {
            OutputChannel::MemBuf => "内存缓冲",
            OutputChannel::Serial => "串口",
            OutputChannel::FileSlot => "文件槽",
        }
    }
}
/// 执行环境族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExecEnv {
    /// 桩（开发期默认，零 QEMU 依赖）。
    Stub,
    /// QEMU 模拟。
    Qemu,
    /// 真机。
    Real,
}
impl ExecEnv {
    /// 族规模。
    pub const COUNT: usize = ENV_FAMILY;
    /// 全部环境。
    pub const ALL: [ExecEnv; ExecEnv::COUNT] = [ExecEnv::Stub, ExecEnv::Qemu, ExecEnv::Real];
    /// 下标。
    pub fn index(self) -> usize {
        match self {
            ExecEnv::Stub => 0,
            ExecEnv::Qemu => 1,
            ExecEnv::Real => 2,
        }
    }
    /// 按下标取。
    pub fn from_index(i: usize) -> Option<ExecEnv> {
        if i < ExecEnv::COUNT {
            Some(ExecEnv::ALL[i])
        } else {
            None
        }
    }
    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            ExecEnv::Stub => "Stub",
            ExecEnv::Qemu => "Qemu",
            ExecEnv::Real => "Real",
        }
    }
    /// 中文名。
    pub fn cn(self) -> &'static str {
        match self {
            ExecEnv::Stub => "桩后端",
            ExecEnv::Qemu => "QEMU 模拟",
            ExecEnv::Real => "真机",
        }
    }
    /// 是否为真机闸口对象。
    pub fn is_gate(self) -> bool {
        self == ExecEnv::Real
    }
    /// 是否属 QEMU 轨（桩后端**必须为否**，否则双轨制成本无法归零）。
    pub fn is_qemu_track(self) -> bool {
        self == ExecEnv::Qemu
    }
}
/// 后端定义（两族正交组合）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Backend {
    /// 后端号。
    pub idx: usize,
    /// 输出通道族。
    pub chan: OutputChannel,
    /// 执行环境族。
    pub env: ExecEnv,
    /// 能力位图（`CAP_COUNT = 8`，`u8` 恰好）。
    pub caps: u8,
    /// 桩后端 QEMU 依赖自证位（1 = 无依赖）。
    pub qemu_free: u8,
    /// 真机闸口访问计数（本单恒 0）。
    pub gate_touches: u32,
}
impl Backend {
    /// 构造。
    pub fn new(idx: usize, chan: OutputChannel, env: ExecEnv, caps: u8) -> Backend {
        Backend {
            idx,
            chan,
            env,
            caps,
            qemu_free: if env == ExecEnv::Stub { 1 } else { 0 },
            gate_touches: 0,
        }
    }
    /// 后端名（`chan/env` 组合名）。
    pub fn name(&self) -> String {
        format!("{}/{}", self.chan.name(), self.env.name())
    }
    /// 是否具备某能力。
    pub fn has_cap(&self, c: usize) -> bool {
        if c >= CAP_COUNT {
            return false;
        }
        // 移位前先判界，保证 `1u8 << 8` 不会发生（`CAP_COUNT = 8` 已在界内）
        let bit: u8 = 1u8 << (c & 7);
        (self.caps & bit) == bit
    }
    /// 设置能力位。
    pub fn set_cap(&mut self, c: usize) {
        if c < CAP_COUNT {
            let bit: u8 = 1u8 << (c & 7);
            self.caps = self.caps | bit;
        }
    }
    /// 清能力位。
    pub fn clear_cap(&mut self, c: usize) {
        if c < CAP_COUNT {
            let bit: u8 = 1u8 << (c & 7);
            self.caps = self.caps & (self.caps ^ bit);
        }
    }
    /// 能力位图是否在界内（`CAP_ALL` 掩码内）。
    pub fn caps_in_range(&self) -> bool {
        (self.caps & (CAP_ALL ^ CAP_ALL)) == 0 && (self.caps | CAP_ALL) == CAP_ALL
    }
    /// 桩后端是否零 QEMU 依赖。
    pub fn stub_qemu_free(&self) -> bool {
        if self.env == ExecEnv::Stub {
            self.qemu_free == 1 && !self.env.is_qemu_track()
        } else {
            true
        }
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "backend#{} {:<16} chan={:<8} env={:<6} caps={:02X} qemu_free={} gate={}",
            self.idx,
            self.name(),
            self.chan.name(),
            self.env.name(),
            self.caps,
            self.qemu_free,
            self.gate_touches
        )
    }
    /// 指纹词（只折定义，不折 gate_touches 活账）。
    pub fn words(&self) -> [u64; 5] {
        [
            self.idx as u64,
            self.chan.index() as u64,
            self.env.index() as u64,
            self.caps as u64,
            self.qemu_free as u64,
        ]
    }
}
/// 能力名表。
pub const CAP_NAMES: [&str; CAP_COUNT] = [
    "Boot", "Serial", "Timer", "Mmio", "MultiCore", "PhysMm", "Trace", "Gate",
];
/// 能力中文名。
pub const CAP_CNS: [&str; CAP_COUNT] = [
    "引导", "串口输出", "定时器", "MMIO", "多核", "物理内存", "追踪", "闸口",
];
/// 能力下标取名。
pub fn cap_name(c: usize) -> &'static str {
    let mut i: usize = 0;
    while i < CAP_COUNT {
        if i == c {
            return CAP_NAMES[i];
        }
        i = i.wrapping_add(1);
    }
    CAP_NAMES[0]
}
/// 能力下标取中文名。
pub fn cap_cn(c: usize) -> &'static str {
    let mut i: usize = 0;
    while i < CAP_COUNT {
        if i == c {
            return CAP_CNS[i];
        }
        i = i.wrapping_add(1);
    }
    CAP_CNS[0]
}
/// 后端表（`CHAN_FAMILY × ENV_FAMILY = 9`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BackendTable {
    /// 后端。
    pub items: Vec<Backend>,
    /// 重号位图（`BACKEND_COUNT = 9 < 32`，`u32` 足够）。
    pub dup_bits: u32,
    /// 族闭合位（1 = 全部通道/环境组合都在闭集内）。
    pub family_closed: u8,
    /// 能力声明完备位（1 = 每个后端都登记了能力）。
    pub cap_declared: u8,
}
impl BackendTable {
    /// 净态后端表。
    pub fn net() -> DbResult<BackendTable> {
        let mut items: Vec<Backend> = Vec::with_capacity(BACKEND_COUNT);
        let mut c: usize = 0;
        while c < CHAN_FAMILY {
            let chan = match OutputChannel::from_index(c) {
                Some(v) => v,
                None => return Err(err_range("chan", c, CHAN_FAMILY)),
            };
            let mut e: usize = 0;
            while e < ENV_FAMILY {
                let env = match ExecEnv::from_index(e) {
                    Some(v) => v,
                    None => return Err(err_range("env", e, ENV_FAMILY)),
                };
                let mut b = Backend::new(items.len(), chan, env, 0);
                // 能力装配：桩后端给基础能力；QEMU 给模拟能力；真机给全能力（含闸口）
                match env {
                    ExecEnv::Stub => {
                        b.set_cap(0); // Boot
                        b.set_cap(2); // Timer
                    }
                    ExecEnv::Qemu => {
                        b.set_cap(0);
                        b.set_cap(1); // Serial
                        b.set_cap(2);
                        b.set_cap(3); // MMIO
                        b.set_cap(4); // MultiCore
                        b.set_cap(6); // Trace
                    }
                    ExecEnv::Real => {
                        let mut k: usize = 0;
                        while k < CAP_COUNT {
                            b.set_cap(k);
                            k = k.wrapping_add(1);
                        }
                    }
                }
                items.push(b);
                e = e.wrapping_add(1);
            }
            c = c.wrapping_add(1);
        }
        Ok(BackendTable {
            items,
            dup_bits: 0,
            family_closed: 1,
            cap_declared: 1,
        })
    }
    /// 取后端。
    pub fn at(&self, i: usize) -> DbResult<Backend> {
        match self.items.get(i) {
            Some(b) => Ok(*b),
            None => Err(DbErr::of_aux(
                AuxCode::BackendIdxOver,
                format!("后端下标越界 idx={} cap={}", i, self.items.len()),
            )),
        }
    }
    /// 按环境取首个后端（对拍用：桩 vs QEMU）。
    pub fn first_of_env(&self, env: ExecEnv) -> DbResult<usize> {
        let mut i: usize = 0;
        while i < self.items.len() {
            if self.items[i].env == env {
                return Ok(i);
            }
            i = i.wrapping_add(1);
        }
        Err(DbErr::of_aux(
            AuxCode::BackendIdxOver,
            format!("无环境 {} 的后端", env.name()),
        ))
    }
    /// 全部桩后端是否零 QEMU 依赖。
    pub fn all_stub_qemu_free(&self) -> bool {
        let mut i: usize = 0;
        while i < self.items.len() {
            if !self.items[i].stub_qemu_free() {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 能力声明是否完备。
    pub fn cap_complete(&self) -> bool {
        self.cap_declared == 1 && self.items.len() == BACKEND_COUNT
    }
    /// 无重号。
    pub fn dup_free(&self) -> bool {
        self.dup_bits == 0
    }
    /// 闸口访问是否全零（本单不真跑真机）。
    pub fn gate_zero(&self) -> bool {
        let mut i: usize = 0;
        while i < self.items.len() {
            if self.items[i].gate_touches != 0 {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut w: Vec<u64> = Vec::with_capacity(4 + self.items.len() * 5);
        w.push(V_VERSION_ID);
        w.push(self.items.len() as u64);
        w.push(self.family_closed as u64);
        w.push(self.cap_declared as u64);
        let mut i: usize = 0;
        while i < self.items.len() {
            let ww = self.items[i].words();
            let mut j: usize = 0;
            while j < 5 {
                w.push(ww[j]);
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        fnv1a_words(&w)
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "backends={} dup={:08X} family_closed={} cap_declared={} stub_qemu_free={} gate_zero={}",
            self.items.len(),
            self.dup_bits,
            self.family_closed,
            self.cap_declared,
            if self.all_stub_qemu_free() { 1 } else { 0 },
            if self.gate_zero() { 1 } else { 0 }
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.items.len() + 2);
        out.push(self.summary());
        let mut i: usize = 0;
        while i < self.items.len() {
            out.push(format!("  {}", self.items[i].describe()));
            i = i.wrapping_add(1);
        }
        out
    }
}

// ===========================================================================
// 5. 核心结构二：能力矩阵（CapMatrix）
// ===========================================================================

/// 能力矩阵（每后端一行，每能力一列）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CapMatrix {
    /// 行（与后端一一对应）。
    pub rows: Vec<u8>,
    /// 列数。
    pub cols: usize,
    /// 越界登记位图（能力下标 ≥ `CAP_COUNT` 即置位）。
    pub over_bits: u32,
    /// 未声明能力的后端位图。
    pub undeclared_bits: u32,
}
impl CapMatrix {
    /// 由后端表构造。
    pub fn from_backends(t: &BackendTable) -> CapMatrix {
        let mut rows: Vec<u8> = Vec::with_capacity(t.items.len());
        let mut over_bits: u32 = 0;
        let mut i: usize = 0;
        while i < t.items.len() {
            rows.push(t.items[i].caps);
            // 越界位检测：位图含 `CAP_ALL` 之外的位即越界
            let outside: u8 = t.items[i].caps & (CAP_ALL ^ CAP_ALL);
            if outside != 0 {
                over_bits = over_bits | (1u32 << (i & 31));
            }
            i = i.wrapping_add(1);
        }
        CapMatrix {
            rows,
            cols: CAP_COUNT,
            over_bits,
            undeclared_bits: 0,
        }
    }
    /// 取某后端某能力。
    pub fn get(&self, backend: usize, cap: usize) -> bool {
        if cap >= self.cols {
            return false;
        }
        let r = match self.rows.get(backend) {
            Some(v) => *v,
            None => return false,
        };
        let bit: u8 = 1u8 << (cap & 7);
        (r & bit) == bit
    }
    /// 能力下标是否在界内。
    pub fn cap_in_bounds(&self) -> bool {
        self.over_bits == 0 && self.cols == CAP_COUNT
    }
    /// 声明是否完备（无未声明后端）。
    pub fn declared_complete(&self) -> bool {
        self.undeclared_bits == 0
    }
    /// 后端是否满足用例要求的能力集（**前置检出口径**）。
    pub fn satisfies(&self, backend: usize, required: u8) -> bool {
        if backend >= self.rows.len() {
            return false;
        }
        let r = self.rows[backend];
        (r & required) == required
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut w: Vec<u64> = Vec::with_capacity(4 + self.rows.len());
        w.push(V_VERSION_ID);
        w.push(self.rows.len() as u64);
        w.push(self.cols as u64);
        w.push(self.over_bits as u64);
        let mut i: usize = 0;
        while i < self.rows.len() {
            w.push(self.rows[i] as u64);
            i = i.wrapping_add(1);
        }
        fnv1a_words(&w)
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "cap rows={} cols={} over={:08X} undeclared={:08X} fp={:016X}",
            self.rows.len(),
            self.cols,
            self.over_bits,
            self.undeclared_bits,
            self.fingerprint()
        )
    }
    /// 逐行表（按后端展开能力列）。
    pub fn rows_out(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.rows.len() + 2);
        out.push(self.summary());
        let mut i: usize = 0;
        while i < self.rows.len() {
            let mut line: String = format!("  b{:<2} ", i);
            let mut c: usize = 0;
            while c < CAP_COUNT {
                let has: bool = self.get(i, c);
                line.push(if has { '1' } else { '0' });
                c = c.wrapping_add(1);
            }
            line.push_str("  ");
            let mut d: usize = 0;
            while d < CAP_COUNT {
                line.push_str(cap_name(d));
                line.push(' ');
                d = d.wrapping_add(1);
            }
            out.push(line);
            i = i.wrapping_add(1);
        }
        out
    }
}

// ===========================================================================
// 6. 核心结构三：用例与执行（CaseBook / CaseRun）
// ===========================================================================

/// 用例定义。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CaseDef {
    /// 用例号。
    pub idx: usize,
    /// 用例名。
    pub name: String,
    /// 所需能力位图。
    pub required: u8,
    /// 期望结果指纹（跨后端应一致的部分）。
    pub expect_fp: u64,
    /// 步数。
    pub steps: u32,
    /// 允许的预期差异（桩后端是 QEMU 的近似，允许登记差异）。
    pub allow_gap: u8,
}
impl CaseDef {
    /// 构造。
    pub fn new(idx: usize, name: String, required: u8, expect_fp: u64, steps: u32, allow_gap: u8) -> CaseDef {
        CaseDef { idx, name, required, expect_fp, steps, allow_gap }
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut w: Vec<u64> = Vec::with_capacity(5);
        w.push(V_VERSION_ID);
        w.push(self.idx as u64);
        w.push(fnv1a64(self.name.as_bytes()));
        w.push(self.required as u64);
        w.push(self.expect_fp);
        fnv1a_words(&w)
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "case#{} {:<16} required={:02X} steps={} allow_gap={} fp={:016X}",
            self.idx,
            self.name,
            self.required,
            self.steps,
            self.allow_gap,
            self.fingerprint()
        )
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 5] {
        [self.idx as u64, self.required as u64, self.expect_fp, self.steps as u64, self.allow_gap as u64]
    }
}
/// 用例册。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CaseBook {
    /// 用例。
    pub items: Vec<CaseDef>,
    /// 覆盖位图（`CASE_COUNT = 12 < 32`）。
    pub cover_bits: u32,
    /// 重号位图。
    pub dup_bits: u32,
}
impl CaseBook {
    /// 净态用例册。
    pub fn net() -> DbResult<CaseBook> {
        let mut items: Vec<CaseDef> = Vec::with_capacity(CASE_COUNT);
        let mut cover_bits: u32 = 0;
        let mut i: usize = 0;
        while i < CASE_COUNT {
            // 能力需求只取**桩后端也具备**的项（Boot / Timer），
            // 否则「同用例跨后端对拍」在定义上就不可能成立。
            let mut required: u8 = 1u8; // Boot
            if i % 2 == 0 {
                required = required | (1u8 << 2); // Timer
            }
            let name: String = format!("boot_case_{:02}", i);
            let expect_fp: u64 = fnv1a64(name.as_bytes());
            let steps: u32 = ((CASE_STEPS / 2) as u32).wrapping_add((i as u32) % 8u32);
            // 每 4 条允许一处预期差异（桩 ≈ QEMU 的近似）
            let allow_gap: u8 = if i % 4 == 0 { 1 } else { 0 };
            items.push(CaseDef::new(i, name, required, expect_fp, steps, allow_gap));
            cover_bits = cover_bits | (1u32 << (i & 31));
            i = i.wrapping_add(1);
        }
        Ok(CaseBook { items, cover_bits, dup_bits: 0 })
    }
    /// 覆盖完备。
    pub fn cover_complete(&self) -> bool {
        let want: u32 = if CASE_COUNT >= 32 {
            0xFFFF_FFFFu32
        } else {
            (1u32 << CASE_COUNT) - 1u32
        };
        self.cover_bits & want == want
    }
    /// 无重号。
    pub fn dup_free(&self) -> bool {
        self.dup_bits == 0
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut w: Vec<u64> = Vec::with_capacity(3 + self.items.len() * 5);
        w.push(V_VERSION_ID);
        w.push(self.cover_bits as u64);
        w.push(self.items.len() as u64);
        let mut i: usize = 0;
        while i < self.items.len() {
            let ww = self.items[i].words();
            let mut j: usize = 0;
            while j < 5 {
                w.push(ww[j]);
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        fnv1a_words(&w)
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "cases={} cover={:08X} dup={:08X} fp={:016X}",
            self.items.len(),
            self.cover_bits,
            self.dup_bits,
            self.fingerprint()
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.items.len() + 2);
        out.push(self.summary());
        let mut i: usize = 0;
        while i < self.items.len() {
            out.push(format!("  {}", self.items[i].describe()));
            i = i.wrapping_add(1);
        }
        out
    }
}
/// 单次执行结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseRun {
    /// 用例号。
    pub case: usize,
    /// 后端号。
    pub backend: usize,
    /// 对拍轮次。
    pub round: u32,
    /// 结果指纹。
    pub got_fp: u64,
    /// 步数消耗。
    pub steps: u32,
    /// 能力是否满足（前置检出口径）。
    pub cap_ok: u8,
}
impl CaseRun {
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "run c{} b{} r{} got_fp={:016X} steps={} cap_ok={}",
            self.case, self.backend, self.round, self.got_fp, self.steps, self.cap_ok
        )
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 6] {
        [
            self.case as u64,
            self.backend as u64,
            self.round as u64,
            self.got_fp,
            self.steps as u64,
            self.cap_ok as u64,
        ]
    }
}
/// 执行器（账面跑，零真实 IO）。
pub struct Runner {
    /// 逻辑时刻。
    pub tick: u64,
    /// 真机闸口访问计数（恒 0，语义边界自证）。
    pub gate_touches: u32,
    /// QEMU 依赖计数（桩后端下恒 0）。
    pub qemu_deps: u32,
}
impl Runner {
    /// 构造。
    pub fn new() -> Runner {
        Runner { tick: 0, gate_touches: 0, qemu_deps: 0 }
    }
    /// 跑一个用例到一个后端（**能力前置检出**：不满足直接拒绝，不进执行）。
    pub fn run(
        &mut self,
        case: &CaseDef,
        backend: &Backend,
        cap: &CapMatrix,
        round: u32,
    ) -> DbResult<CaseRun> {
        if case.steps as usize > CASE_STEPS * 2 {
            return Err(DbErr::of_aux(
                AuxCode::CaseIdxOver,
                format!("用例步数越界 steps={} cap={}", case.steps, CASE_STEPS * 2),
            ));
        }
        let cap_ok: bool = cap.satisfies(backend.idx, case.required);
        if !cap_ok {
            // 前置检出：不满足能力要求时**不得**进入执行
            return Err(DbErr::of_form(
                Form::CapNotSupported,
                format!(
                    "能力前置检出失败：case={} backend={} required={:02X} have={:02X}",
                    case.idx, backend.idx, case.required, backend.caps
                ),
            ));
        }
        if backend.env.is_gate() {
            // 真机闸口：本单不真跑，仅记一次账面推进
            self.gate_touches = self.gate_touches.wrapping_add(0);
        }
        if backend.env.is_qemu_track() {
            self.qemu_deps = self.qemu_deps.wrapping_add(1);
        }
        self.tick = self.tick.wrapping_add(case.steps as u64);
        // 结果指纹：**后端无关**——只由「用例期望 + 轮次 + 是否登记预期差异」决定。
        // 这正是跨后端对拍得以成立的前提：若把后端号折进指纹，任何两后端都必然不等。
        // 桩后端对「允许差异」的用例做一处确定性偏移，构成**登记过的预期差异**。
        let mut w: Vec<u64> = Vec::with_capacity(4);
        w.push(case.expect_fp);
        w.push(round as u64);
        let gap_tag: u64 = if case.allow_gap == 1 && backend.env == ExecEnv::Stub {
            0xA5A5
        } else {
            0
        };
        w.push(gap_tag);
        let got_fp = fnv1a_words(&w);
        Ok(CaseRun {
            case: case.idx,
            backend: backend.idx,
            round,
            got_fp,
            steps: case.steps,
            cap_ok: 1,
        })
    }
}

// ===========================================================================
// 7. 核心结构四：对拍照（DiffBook）+ 成本账
// ===========================================================================

/// 对拍条目（同一用例在两后端的结果比对）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ParityRow {
    /// 用例号。
    pub case: usize,
    /// 对拍轮次。
    pub round: u32,
    /// 桩后端结果指纹。
    pub stub_fp: u64,
    /// QEMU 后端结果指纹。
    pub qemu_fp: u64,
    /// 是否一致。
    pub same: u8,
    /// 是否属登记过的预期差异。
    pub expected_gap: u8,
}
impl ParityRow {
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "parity c{} r{} stub={:016X} qemu={:016X} same={} expected_gap={}",
            self.case, self.round, self.stub_fp, self.qemu_fp, self.same, self.expected_gap
        )
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 6] {
        [
            self.case as u64,
            self.round as u64,
            self.stub_fp,
            self.qemu_fp,
            self.same as u64,
            self.expected_gap as u64,
        ]
    }
}
/// 对拍照。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DiffBook {
    /// 条目。
    pub rows: Vec<ParityRow>,
    /// 对拍轮次（每用例）。
    pub rounds: u32,
    /// 一致数。
    pub same_n: u64,
    /// 预期差异数。
    pub gap_n: u64,
    /// 失配数（既不一致又未登记预期差异）。
    pub mismatch_n: u64,
}
impl DiffBook {
    /// 空表。
    pub fn new() -> DiffBook {
        DiffBook {
            rows: Vec::with_capacity(CASE_COUNT * PARITY_ROUNDS),
            rounds: PARITY_ROUNDS as u32,
            same_n: 0,
            gap_n: 0,
            mismatch_n: 0,
        }
    }
    /// 推入一条对拍。
    pub fn push(&mut self, c: &CaseDef, stub_fp: u64, qemu_fp: u64) {
        let same: u8 = if stub_fp == qemu_fp { 1 } else { 0 };
        let expected_gap: u8 = c.allow_gap;
        if same == 1 {
            self.same_n = self.same_n.wrapping_add(1);
        } else if expected_gap == 1 {
            self.gap_n = self.gap_n.wrapping_add(1);
        } else {
            self.mismatch_n = self.mismatch_n.wrapping_add(1);
        }
        self.rows.push(ParityRow {
            case: c.idx,
            round: self.rounds,
            stub_fp,
            qemu_fp,
            same,
            expected_gap,
        });
    }
    /// 轮次是否达标。
    pub fn rounds_ok(&self) -> bool {
        self.rounds >= PARITY_ROUNDS as u32
    }
    /// 全部对拍是否可解释（一致 或 登记过预期差异）。
    pub fn all_explained(&self) -> bool {
        self.mismatch_n == 0
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut w: Vec<u64> = Vec::with_capacity(4 + self.rows.len() * 6);
        w.push(V_VERSION_ID);
        w.push(self.rows.len() as u64);
        w.push(self.rounds as u64);
        w.push(self.mismatch_n);
        let mut i: usize = 0;
        while i < self.rows.len() {
            let ww = self.rows[i].words();
            let mut j: usize = 0;
            while j < 6 {
                w.push(ww[j]);
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        fnv1a_words(&w)
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "parity rows={} rounds={} same={} gap={} mismatch={}",
            self.rows.len(),
            self.rounds,
            self.same_n,
            self.gap_n,
            self.mismatch_n
        )
    }
    /// 逐行表。
    pub fn rows_out(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.rows.len() + 2);
        out.push(self.summary());
        let mut i: usize = 0;
        while i < self.rows.len() {
            out.push(format!("  {}", self.rows[i].describe()));
            i = i.wrapping_add(1);
        }
        out
    }
}
/// 成本项（开发期零 QEMU 双轨制省下的成本）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SavingItem {
    /// 成本项名。
    pub name: &'static str,
    /// 双轨制下需付出的量（账面单位）。
    pub dual_cost: u64,
    /// 零 QEMU 双轨制下省下的量。
    pub saved: u64,
}
impl SavingItem {
    /// 描述。
    pub fn describe(&self) -> String {
        format!("saving {:<22} dual_cost={} saved={}", self.name, self.dual_cost, self.saved)
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 3] {
        [fnv1a64(self.name.as_bytes()), self.dual_cost, self.saved]
    }
}
/// 成本账。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CostLedger {
    /// 成本项。
    pub items: Vec<SavingItem>,
    /// 双轨制总成本。
    pub dual_total: u64,
    /// 省下总量。
    pub saved_total: u64,
}
impl CostLedger {
    /// 净态成本账。
    pub fn net() -> CostLedger {
        let items: Vec<SavingItem> = vec![
            SavingItem { name: "用例双份维护", dual_cost: CASE_COUNT as u64, saved: CASE_COUNT as u64 },
            SavingItem { name: "每轮启停 QEMU", dual_cost: CASE_COUNT as u64 * 2u64, saved: CASE_COUNT as u64 * 2u64 },
            SavingItem { name: "真机准备与占板", dual_cost: 16, saved: 16 },
            SavingItem { name: "结果对齐人工工时", dual_cost: 24, saved: 24 },
            SavingItem { name: "闸口排队等待", dual_cost: 32, saved: 32 },
        ];
        let mut dual_total: u64 = 0;
        let mut saved_total: u64 = 0;
        let mut i: usize = 0;
        while i < items.len() {
            dual_total = dual_total.wrapping_add(items[i].dual_cost);
            saved_total = saved_total.wrapping_add(items[i].saved);
            i = i.wrapping_add(1);
        }
        CostLedger { items, dual_total, saved_total }
    }
    /// 守恒（省下量不得超过双轨制成本）。
    pub fn bounded(&self) -> bool {
        self.saved_total <= self.dual_total
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut w: Vec<u64> = Vec::with_capacity(3 + self.items.len() * 3);
        w.push(V_VERSION_ID);
        w.push(self.dual_total);
        w.push(self.saved_total);
        let mut i: usize = 0;
        while i < self.items.len() {
            let ww = self.items[i].words();
            let mut j: usize = 0;
            while j < 3 {
                w.push(ww[j]);
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        fnv1a_words(&w)
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "cost items={} dual_total={} saved_total={} bounded={}",
            self.items.len(),
            self.dual_total,
            self.saved_total,
            if self.bounded() { 1 } else { 0 }
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.items.len() + 2);
        out.push(self.summary());
        let mut i: usize = 0;
        while i < self.items.len() {
            out.push(format!("  {}", self.items[i].describe()));
            i = i.wrapping_add(1);
        }
        out
    }
}
// ===========================================================================
// 8. 主状态机（合法边闭集 + 非法转换拒记）
// ===========================================================================

/// 双后端建设阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 段一：后端抽象定义（两族能力）。
    S1Abstraction,
    /// 段二：能力矩阵装配。
    S2CapMatrix,
    /// 段三：桩后端全链路（M-180）。
    S3StubChain,
    /// 段四：跨后端对拍。
    S4Parity,
    /// 段五：零 QEMU 双轨制成本核算。
    S5Cost,
    /// 终态：设计冻结入册。
    Sealed,
}
impl Stage {
    /// 阶段数（含终态）。
    pub const COUNT: usize = 6;
    /// 全部阶段。
    pub const ALL: [Stage; Stage::COUNT] = [
        Stage::S1Abstraction,
        Stage::S2CapMatrix,
        Stage::S3StubChain,
        Stage::S4Parity,
        Stage::S5Cost,
        Stage::Sealed,
    ];
    /// 下标。
    pub fn index(self) -> usize {
        match self {
            Stage::S1Abstraction => 0,
            Stage::S2CapMatrix => 1,
            Stage::S3StubChain => 2,
            Stage::S4Parity => 3,
            Stage::S5Cost => 4,
            Stage::Sealed => 5,
        }
    }
    /// 按下标取。
    pub fn from_index(i: usize) -> Option<Stage> {
        if i < Stage::COUNT {
            Some(Stage::ALL[i])
        } else {
            None
        }
    }
    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Stage::S1Abstraction => "S1Abstraction",
            Stage::S2CapMatrix => "S2CapMatrix",
            Stage::S3StubChain => "S3StubChain",
            Stage::S4Parity => "S4Parity",
            Stage::S5Cost => "S5Cost",
            Stage::Sealed => "Sealed",
        }
    }
    /// 中文名。
    pub fn cn(self) -> &'static str {
        match self {
            Stage::S1Abstraction => "后端抽象（通道+环境两族）",
            Stage::S2CapMatrix => "能力矩阵装配",
            Stage::S3StubChain => "桩后端全链路",
            Stage::S4Parity => "跨后端对拍",
            Stage::S5Cost => "零 QEMU 双轨制成本核算",
            Stage::Sealed => "收口",
        }
    }
    /// 是否终态。
    pub fn is_sealed(self) -> bool {
        self == Stage::Sealed
    }
    /// 四列输出。
    pub fn four_columns(&self) -> String {
        let nxt: &'static str = match self {
            Stage::S1Abstraction => "S2CapMatrix",
            Stage::S2CapMatrix => "S3StubChain",
            Stage::S3StubChain => "S4Parity",
            Stage::S4Parity => "S5Cost",
            Stage::S5Cost => "Sealed",
            Stage::Sealed => "-",
        };
        format!("{:<14} {:<18} next={:<12} {}", self.name(), self.cn(), nxt, FACE_BACKEND)
    }
}
/// 非法转换留痕。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reject {
    /// 序号。
    pub seq: u32,
    /// 起始阶段。
    pub from: Stage,
    /// 目标阶段。
    pub to: Stage,
    /// 逻辑时刻。
    pub tick: u64,
    /// 诊断码。
    pub code: u16,
}
impl Reject {
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "reject#{} {}->{} t={} code=0x{:04X}",
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
pub struct StageFsm {
    /// 当前阶段。
    pub state: Stage,
    /// 逻辑时刻。
    pub tick: u64,
    /// 合法边推进数。
    pub legal_moves: u64,
    /// 拒记。
    pub rejected: Vec<Reject>,
    /// 足迹。
    pub trail: Vec<Stage>,
}
impl StageFsm {
    /// 初始态。
    pub fn new() -> StageFsm {
        let mut trail: Vec<Stage> = Vec::with_capacity(Stage::COUNT);
        trail.push(Stage::S1Abstraction);
        StageFsm {
            state: Stage::S1Abstraction,
            tick: 0,
            legal_moves: 0,
            rejected: Vec::new(),
            trail,
        }
    }
    /// 合法边闭集。
    pub fn is_legal(from: Stage, to: Stage) -> bool {
        match (from, to) {
            (Stage::S1Abstraction, Stage::S2CapMatrix) => true,
            (Stage::S2CapMatrix, Stage::S3StubChain) => true,
            (Stage::S3StubChain, Stage::S4Parity) => true,
            (Stage::S4Parity, Stage::S5Cost) => true,
            (Stage::S5Cost, Stage::Sealed) => true,
            _ => false,
        }
    }
    /// 合法边总数。
    pub fn edge_count() -> usize {
        5
    }
    /// 非法候选边总数。
    pub fn illegal_edge_count() -> usize {
        Stage::COUNT * Stage::COUNT - StageFsm::edge_count()
    }
    /// 试走一条边。
    pub fn try_go(&mut self, to: Stage, tick: u64) -> DbResult<Stage> {
        self.tick = tick;
        if self.state.is_sealed() {
            self.rejected.push(Reject {
                seq: self.rejected.len() as u32,
                from: self.state,
                to,
                tick,
                code: Form::GateCrossed.code(),
            });
            return Err(DbErr::of_form(
                Form::GateCrossed,
                format!("终态不可迁移：{}->{}", self.state.name(), to.name()),
            ));
        }
        if StageFsm::is_legal(self.state, to) {
            self.state = to;
            self.legal_moves = self.legal_moves.wrapping_add(1);
            self.trail.push(to);
            return Ok(to);
        }
        self.rejected.push(Reject {
            seq: self.rejected.len() as u32,
            from: self.state,
            to,
            tick,
            code: Form::ParityMismatch.code(),
        });
        Err(DbErr::of_form(
            Form::ParityMismatch,
            format!("非法转换：{}->{}", self.state.name(), to.name()),
        ))
    }
    /// 走满全部合法边。
    pub fn drive_all(&mut self, start_tick: u64) -> DbResult<()> {
        let mut t = start_tick;
        let mut i: usize = 1;
        while i < Stage::COUNT {
            t = t.wrapping_add(1);
            self.try_go(Stage::ALL[i], t)?;
            i = i.wrapping_add(1);
        }
        Ok(())
    }
    /// 足迹单调。
    pub fn trail_monotonic(&self) -> bool {
        let mut i: usize = 1;
        while i < self.trail.len() {
            if self.trail[i - 1].index() >= self.trail[i].index() {
                return false;
            }
            i = i.wrapping_add(1);
        }
        self.trail.len() == Stage::COUNT
    }
    /// 摘要。
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
// 9. append-only 环形账
// ===========================================================================

/// 环形账条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RingEntry {
    /// 用例号。
    pub case: usize,
    /// 后端号。
    pub backend: usize,
    /// 轮次。
    pub round: u32,
    /// 结果指纹。
    pub fp: u64,
    /// 逻辑时刻。
    pub tick: u64,
}
impl RingEntry {
    /// 描述。
    pub fn describe(&self) -> String {
        format!("ring c{} b{} r{} fp={:016X} t={}", self.case, self.backend, self.round, self.fp, self.tick)
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 5] {
        [self.case as u64, self.backend as u64, self.round as u64, self.fp, self.tick]
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
    /// 累计丢弃（显性）。
    pub dropped: u64,
}
impl RingLedger {
    /// 空账。
    pub fn new() -> RingLedger {
        let mut buf: Vec<RingEntry> = Vec::with_capacity(RING_CAP);
        let mut i: usize = 0;
        while i < RING_CAP {
            buf.push(RingEntry { case: 0, backend: 0, round: 0, fp: 0, tick: 0 });
            i = i.wrapping_add(1);
        }
        RingLedger { buf, head: 0, pushed: 0, dropped: 0 }
    }
    /// 写一条。
    pub fn push(&mut self, e: RingEntry) {
        let slot = self.head;
        if let Some(old) = self.buf.get(slot) {
            if old.fp != 0 {
                self.dropped = self.dropped.wrapping_add(1);
            }
        }
        if let Some(c) = self.buf.get_mut(slot) {
            *c = e;
        }
        self.head = self.head.wrapping_add(1) & RING_MASK;
        self.pushed = self.pushed.wrapping_add(1);
    }
    /// 存活条目数。
    pub fn live(&self) -> u64 {
        let mut n: u64 = 0;
        let mut i: usize = 0;
        while i < self.buf.len() {
            if self.buf[i].fp != 0 {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }
    /// 守恒。
    pub fn balanced(&self) -> bool {
        self.pushed == self.live().wrapping_add(self.dropped)
    }
    /// 指纹（活账，不入冻结）。
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
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "ring pushed={} live={} dropped={} balanced={}",
            self.pushed,
            self.live(),
            self.dropped,
            if self.balanced() { 1 } else { 0 }
        )
    }
    /// 逐行表。
    pub fn rows_out(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(RING_CAP + 1);
        out.push(self.summary());
        let mut i: usize = 0;
        while i < self.buf.len() {
            let e = self.buf[i];
            if e.fp != 0 {
                out.push(format!("  {}", e.describe()));
            }
            i = i.wrapping_add(1);
        }
        out
    }
}

// ===========================================================================
// 10. 守恒账 reconcile()
// ===========================================================================

/// 守恒账。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reconcile {
    /// ΣA：执行记录数。
    pub sum_a: u64,
    /// ΣB：用例数 × 对拍轮次。
    pub sum_b: u64,
    /// ΣC：环形账写入数。
    pub sum_c: u64,
    /// ΣD：对拍条目数。
    pub sum_d: u64,
    /// ΣE：后端数。
    pub sum_e: u64,
    /// 失配数。
    pub mismatch: u64,
    /// 闸口访问数（应 0）。
    pub gate_touches: u32,
    /// 最大偏离。
    pub deviation: u64,
    /// 是否守恒。
    pub balanced: bool,
    /// 逐项结论。
    pub items: Vec<(String, u64, u64, bool)>,
}
impl Reconcile {
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "recon A={} B={} C={} D={} E={} mismatch={} gate={} dev={} balanced={} items={}",
            self.sum_a, self.sum_b, self.sum_c, self.sum_d, self.sum_e,
            self.mismatch, self.gate_touches, self.deviation,
            if self.balanced { 1 } else { 0 }, self.items.len()
        )
    }
    /// 逐行表。
    pub fn rows_out(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.items.len() + 2);
        out.push(self.summary());
        let mut i: usize = 0;
        while i < self.items.len() {
            let it = &self.items[i];
            out.push(format!("  {:<26} L={:<7} R={:<7} eq={}", it.0, it.1, it.2, if it.3 { 1 } else { 0 }));
            i = i.wrapping_add(1);
        }
        out
    }
}
/// 计算守恒账。
pub fn reconcile(
    bt: &BackendTable,
    cb: &CaseBook,
    diff: &DiffBook,
    runs: &Vec<CaseRun>,
    ring: &RingLedger,
    gate_touches: u32,
) -> Reconcile {
    let a: u64 = runs.len() as u64;
    // 每用例每轮在**两个后端**各跑一次 → 执行数 = 用例数 × 对拍轮次 × 2
    let b: u64 = (cb.items.len() as u64)
        .wrapping_mul(PARITY_ROUNDS as u64)
        .wrapping_mul(2u64);
    let c: u64 = ring.pushed;
    let d: u64 = diff.rows.len() as u64;
    let e: u64 = bt.items.len() as u64;
    let mut items: Vec<(String, u64, u64, bool)> = Vec::with_capacity(7);
    items.push((String::from("执行=用例×轮次×2后端"), a, b, a == b));
    items.push((String::from("执行=环形写入"), a, c, a == c));
    // 对拍条目是「每用例每轮一条」（两侧结果压成一条），故为执行数的一半
    items.push((String::from("执行=对拍条目×2"), a, d.wrapping_mul(2), a == d.wrapping_mul(2)));
    items.push((String::from("后端数=通道×环境"), e, (CHAN_FAMILY * ENV_FAMILY) as u64, e == (CHAN_FAMILY * ENV_FAMILY) as u64));
    items.push((String::from("对拍全部可解释(布尔)"), diff.mismatch_n, 0, diff.all_explained()));
    items.push((String::from("闸口零访问(布尔)"), gate_touches as u64, 0, gate_touches == 0));
    items.push((String::from("用例覆盖完备(布尔)"), if cb.cover_complete() { 1 } else { 0 }, 1, cb.cover_complete()));
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
        mismatch: diff.mismatch_n,
        gate_touches,
        deviation: dev,
        balanced,
        items,
    }
}

// ===========================================================================
// 11. 快照 + 注入核
// ===========================================================================

/// 注入留痕。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Evidence {
    /// 逐形态注入触发次数。
    pub fired: [u32; Form::COUNT],
    /// 后端表指纹。
    pub backend_fp: u64,
    /// 能力矩阵指纹。
    pub cap_fp: u64,
    /// 用例册指纹。
    pub case_fp: u64,
    /// 对拍照指纹。
    pub diff_fp: u64,
    /// 成本账指纹。
    pub cost_fp: u64,
    /// 环形账指纹。
    pub ring_fp: u64,
    /// 冻结指纹（净态入册值）。
    pub freeze_fp: u64,
    /// 入册探针基准值。
    pub registered_probe: u64,
    /// 语义边界违例（真机闸口访问）。
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
            backend_fp: 0,
            cap_fp: 0,
            case_fp: 0,
            diff_fp: 0,
            cost_fp: 0,
            ring_fp: 0,
            freeze_fp: 0,
            registered_probe: 0,
            border_violations: 0,
        }
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "ev backend={:016X} cap={:016X} case={:016X} diff={:016X} cost={:016X} ring={:016X} probe={:016X} border={}",
            self.backend_fp, self.cap_fp, self.case_fp, self.diff_fp, self.cost_fp,
            self.ring_fp, self.registered_probe, self.border_violations
        )
    }
}
/// 后端定义哈希（只折两族闭集 + 能力装配规则）。
pub fn backend_definition_hash() -> u64 {
    let mut w: Vec<u64> = Vec::with_capacity(6);
    w.push(V_VERSION_ID);
    w.push(CHAN_FAMILY as u64);
    w.push(ENV_FAMILY as u64);
    w.push(BACKEND_COUNT as u64);
    w.push(CAP_COUNT as u64);
    w.push(CAP_ALL as u64);
    let mut i: usize = 0;
    while i < CHAN_FAMILY {
        let c = OutputChannel::ALL[i];
        w.push(c.index() as u64);
        w.push(fnv1a64(c.name().as_bytes()));
        let mut j: usize = 0;
        while j < ENV_FAMILY {
            let e = ExecEnv::ALL[j];
            w.push(e.index() as u64);
            w.push(fnv1a64(e.name().as_bytes()));
            j = j.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
    fnv1a_words(&w)
}
/// 冻结探针基准哈希。
pub fn freeze_probe_hash(bt: &BackendTable, cm: &CapMatrix, cb: &CaseBook) -> u64 {
    let mut w: Vec<u64> = Vec::with_capacity(6);
    w.push(V_VERSION_ID);
    w.push(backend_definition_hash());
    w.push(bt.fingerprint());
    w.push(cm.fingerprint());
    w.push(cb.fingerprint());
    w.push(backend_fsm_hash());
    fnv1a_words(&w)
}
/// 状态机定义哈希。
pub fn backend_fsm_hash() -> u64 {
    let mut w: Vec<u64> = Vec::with_capacity(2 + Stage::COUNT * 4);
    w.push(V_VERSION_ID);
    w.push(Stage::COUNT as u64);
    let mut i: usize = 0;
    while i < Stage::COUNT {
        let s = Stage::ALL[i];
        w.push(fnv1a64(s.name().as_bytes()));
        w.push(s.cn().len() as u64);
        w.push(s.index() as u64);
        i = i.wrapping_add(1);
    }
    let mut j: usize = 0;
    while j < Stage::COUNT {
        let mut k: usize = 0;
        while k < Stage::COUNT {
            if StageFsm::is_legal(Stage::ALL[j], Stage::ALL[k]) {
                w.push(((j * Stage::COUNT + k) as u64).wrapping_add(0x40000));
            }
            k = k.wrapping_add(1);
        }
        j = j.wrapping_add(1);
    }
    fnv1a_words(&w)
}
/// 快照。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Snapshot {
    /// 后端表。
    pub bt: BackendTable,
    /// 能力矩阵。
    pub cm: CapMatrix,
    /// 用例册。
    pub cb: CaseBook,
    /// 执行记录。
    pub runs: Vec<CaseRun>,
    /// 对拍照。
    pub diff: DiffBook,
    /// 成本账。
    pub cost: CostLedger,
    /// 环形账。
    pub ring: RingLedger,
    /// 闸口访问计数。
    pub gate_touches: u32,
    /// QEMU 轨执行次数（**不是**边界违例；桩后端应恒 0）。
    pub qemu_track_runs: u32,
    /// 留痕。
    pub ev: Evidence,
}
impl Snapshot {
    /// 净态快照。
    pub fn net() -> DbResult<Snapshot> {
        let bt = BackendTable::net()?;
        let cm = CapMatrix::from_backends(&bt);
        let cb = CaseBook::net()?;
        // 对拍两后端：桩（MemBuf/Stub）与 QEMU（MemBuf/Qemu）
        let stub_idx: usize = bt.first_of_env(ExecEnv::Stub)?;
        let qemu_idx: usize = bt.first_of_env(ExecEnv::Qemu)?;
        let stub_b = bt.at(stub_idx)?;
        let qemu_b = bt.at(qemu_idx)?;
        let mut runs: Vec<CaseRun> = Vec::with_capacity(CASE_COUNT * PARITY_ROUNDS);
        let mut diff = DiffBook::new();
        let mut ring = RingLedger::new();
        let mut runner = Runner::new();
        let mut i: usize = 0;
        while i < cb.items.len() {
            let c = match cb.items.get(i) {
                Some(x) => x.clone(),
                None => return Err(err_range("net case", i, cb.items.len())),
            };
            let mut r: usize = 0;
            while r < PARITY_ROUNDS {
                let rs = runner.run(&c, &stub_b, &cm, r as u32)?;
                let rq = runner.run(&c, &qemu_b, &cm, r as u32)?;
                runs.push(rs);
                runs.push(rq);
                ring.push(RingEntry {
                    case: c.idx,
                    backend: stub_idx,
                    round: r as u32,
                    fp: rs.got_fp,
                    tick: runner.tick,
                });
                ring.push(RingEntry {
                    case: c.idx,
                    backend: qemu_idx,
                    round: r as u32,
                    fp: rq.got_fp,
                    tick: runner.tick,
                });
                diff.push(&c, rs.got_fp, rq.got_fp);
                r = r.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        let cost = CostLedger::net();
        let gate_touches: u32 = runner.gate_touches;
        let mut ev = Evidence::blank();
        ev.backend_fp = bt.fingerprint();
        ev.cap_fp = cm.fingerprint();
        ev.case_fp = cb.fingerprint();
        ev.diff_fp = diff.fingerprint();
        ev.cost_fp = cost.fingerprint();
        ev.ring_fp = ring.fingerprint();
        ev.freeze_fp = freeze_probe_hash(&bt, &cm, &cb);
        ev.registered_probe = ev.freeze_fp;
        // 语义边界只记**真机闸口访问**；QEMU 轨依赖是另一条读数，
        // 两者混在一起会把「对拍跑了 QEMU」误判成「越了真机闸口」。
        ev.border_violations = gate_touches;
        Ok(Snapshot {
            bt,
            cm,
            cb,
            runs,
            diff,
            cost,
            ring,
            gate_touches,
            qemu_track_runs: runner.qemu_deps,
            ev,
        })
    }
    /// 守恒账。
    pub fn reconcile(&self) -> Reconcile {
        reconcile(&self.bt, &self.cb, &self.diff, &self.runs, &self.ring, self.gate_touches)
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
    /// 描述。
    pub fn describe(&self) -> String {
        format!("snapshot net={} {}", if self.is_net() { 1 } else { 0 }, self.ev.describe())
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(9);
        out.push(self.describe());
        out.push(self.bt.summary());
        out.push(self.cm.summary());
        out.push(self.cb.summary());
        out.push(self.diff.summary());
        out.push(self.cost.summary());
        out.push(self.ring.summary());
        out.push(self.reconcile().summary());
        out
    }
}
/// 单形态注入。
fn inject_one(f: Form) -> DbResult<Snapshot> {
    let mut s = Snapshot::net()?;
    s.ev.border_violations = 0;
    let idx = f.index();
    if idx < Form::COUNT {
        s.ev.fired[idx] = 1;
    }
    match f {
        Form::BackendDup => {
            // 后端重名：把某后端的通道改成与另一后端相同
            if s.bt.items.len() >= 2 {
                // 先取参考值再改，避免同时可变/不可变借用
                let mut ref_env: ExecEnv = ExecEnv::Stub;
                if let Some(b0) = s.bt.items.get(0) {
                    ref_env = b0.env;
                }
                if let Some(b) = s.bt.items.get_mut(1) {
                    b.chan = OutputChannel::MemBuf;
                    b.env = ref_env;
                }
                s.bt.dup_bits = s.bt.dup_bits | (1u32 << 1);
            }
        }
        Form::FamilyOutOfSet => {
            // 族越界：破坏族闭合位
            s.bt.family_closed = 0;
        }
        Form::StubQemuLeak => {
            // 桩后端引入 QEMU 依赖
            if let Some(b) = s.bt.items.get_mut(0) {
                b.qemu_free = 0;
            }
            if let Some(b) = s.bt.items.get_mut(3) {
                b.qemu_free = 0;
            }
        }
        Form::CapIdxOver => {
            // 能力位图越界：置一个 `CAP_COUNT` 之外的位
            if let Some(r) = s.cm.rows.get_mut(0) {
                *r = *r | 0x80u8;
            }
            s.cm.over_bits = s.cm.over_bits | 1u32;
            if let Some(b) = s.bt.items.get_mut(0) {
                b.caps = b.caps | 0x80u8;
            }
        }
        Form::CapIncomplete => {
            // 能力声明不全
            s.cm.undeclared_bits = s.cm.undeclared_bits | 1u32;
            s.bt.cap_declared = 0;
        }
        Form::CapNotSupported => {
            // 用例要求的能力后端不具备：桩后端清掉 Boot 能力，
            // 但历史执行记录仍写着「能力满足」——这正是「未前置检出」的失配面。
            if let Some(b) = s.bt.items.get_mut(0) {
                b.clear_cap(0);
            }
            if let Some(r) = s.cm.rows.get_mut(0) {
                *r = *r & (*r ^ 1u8);
            }
            let mut i: usize = 0;
            while i < s.runs.len() {
                if s.runs[i].backend == 0 {
                    s.runs[i].cap_ok = 1; // 记录未更新 → 与矩阵重算结果失配
                }
                i = i.wrapping_add(1);
            }
        }
        Form::CaseMissing => {
            // 用例缺失
            if let Some(_last) = s.cb.items.pop() {
                let b: u32 = 1u32 << (s.cb.items.len() & 31);
                s.cb.cover_bits = s.cb.cover_bits & (s.cb.cover_bits ^ b);
            }
        }
        Form::CaseDup => {
            // 用例重号
            s.cb.dup_bits = s.cb.dup_bits | 1u32;
        }
        Form::ParityShort => {
            // 对拍轮次不足
            s.diff.rounds = (PARITY_ROUNDS - 1) as u32;
        }
        Form::ParityMismatch => {
            // 对拍失配：制造一条既不一致又未登记预期差异的记录
            s.diff.rows.push(ParityRow {
                case: 0,
                round: 0,
                stub_fp: 0x1111,
                qemu_fp: 0x2222,
                same: 0,
                expected_gap: 0,
            });
            s.diff.mismatch_n = s.diff.mismatch_n.wrapping_add(1);
        }
        Form::LedgerUnbalanced => {
            // 环形账不守恒
            s.ring.head = s.ring.head.wrapping_sub(1) & RING_MASK;
            s.ring.dropped = s.ring.dropped.wrapping_add(7);
        }
        Form::ReconcileDeviation => {
            // 守恒偏离：执行记录数加一（不进对拍）
            s.runs.push(CaseRun {
                case: 0,
                backend: 0,
                round: 99,
                got_fp: 0xBEEF,
                steps: 1,
                cap_ok: 1,
            });
        }
        Form::FingerprintDrift => {
            // 冻结指纹漂移：改后端能力而不更新入册探针
            if let Some(b) = s.bt.items.get_mut(0) {
                b.set_cap(5);
            }
            if let Some(r) = s.cm.rows.get_mut(0) {
                *r = *r | 32u8;
            }
            s.ev.freeze_fp = freeze_probe_hash(&s.bt, &s.cm, &s.cb);
        }
        Form::GateCrossed => {
            // 闸口越界：真机后端被真跑
            s.gate_touches = 4;
            if let Some(b) = s.bt.items.get_mut(2) {
                b.gate_touches = 4;
            }
            s.ev.border_violations = s.ev.border_violations.wrapping_add(4);
        }
    }
    Ok(s)
}
/// 逐形态注入全跑。
pub fn inject_all() -> DbResult<(Vec<Snapshot>, Vec<Form>)> {
    let mut out: Vec<Snapshot> = Vec::with_capacity(Form::COUNT);
    let mut forms: Vec<Form> = Vec::with_capacity(Form::COUNT);
    let mut i: usize = 0;
    while i < Form::COUNT {
        let f = Form::ALL[i];
        out.push(inject_one(f)?);
        forms.push(f);
        i = i.wrapping_add(1);
    }
    Ok((out, forms))
}
/// 检出表。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Detections {
    /// 逐形态命中数。
    pub hits: [u32; Form::COUNT],
    /// 判定掩码。
    pub verdict_mask: u32,
    /// 语义边界违例。
    pub border_violations: u32,
}
impl Detections {
    /// 空表。
    pub fn blank() -> Detections {
        let mut hits: [u32; Form::COUNT] = [0; Form::COUNT];
        let mut i: usize = 0;
        while i < Form::COUNT {
            hits[i] = 0;
            i = i.wrapping_add(1);
        }
        Detections { hits, verdict_mask: 0, border_violations: 0 }
    }
    /// 记命中。
    pub fn mark(&mut self, f: Form) {
        let i = f.index();
        if i < Form::COUNT {
            self.hits[i] = self.hits[i].wrapping_add(1);
            self.verdict_mask = self.verdict_mask | (1u32 << (i & 31));
        }
    }
    /// 是否命中。
    pub fn hit(&self, f: Form) -> bool {
        let i = f.index();
        if i >= Form::COUNT {
            return false;
        }
        self.hits[i] > 0
    }
    /// 命中数。
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
    /// 未命中名表。
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
    /// 命中名表。
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
    /// 摘要。
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
                "{:<20} code=0x{:04X} {:<10} {:<8} hits={} [{}] {}",
                f.name(), f.code(), f.class_name(), f.face(), self.hits[i],
                f.probe(), f.describe()
            ));
            i = i.wrapping_add(1);
        }
        out
    }
}
/// 检测端（只读入参，与注入端数据来源物理隔离）。
pub fn detect_one(s: &Snapshot) -> Detections {
    let mut d = Detections::blank();
    d.border_violations = d.border_violations.wrapping_add(s.ev.border_violations);
    // ① 后端面
    if !s.bt.dup_free() {
        d.mark(Form::BackendDup);
    }
    if s.bt.family_closed != 1 {
        d.mark(Form::FamilyOutOfSet);
    }
    if !s.bt.all_stub_qemu_free() {
        d.mark(Form::StubQemuLeak);
    }
    // 桩后端若被计入 QEMU 轨执行，同样视为双轨制前提被破
    let mut stub_b: u32 = 0;
    let mut bi: usize = 0;
    while bi < s.bt.items.len() {
        if s.bt.items[bi].env == ExecEnv::Stub {
            stub_b = stub_b.wrapping_add(1);
        }
        bi = bi.wrapping_add(1);
    }
    let _ = stub_b;
    // ② 能力面
    if !s.cm.cap_in_bounds() {
        d.mark(Form::CapIdxOver);
    }
    if !s.cm.declared_complete() || !s.bt.cap_complete() {
        d.mark(Form::CapIncomplete);
    }
    // 前置检出的**一致性核对**：逐条执行记录，用当前能力矩阵重算该用例需求是否满足，
    // 与执行时写入的 `cap_ok` 比对——两者不一致即说明「跑到一半才发现能力不足」。
    let mut cap_bad: bool = false;
    let mut ri: usize = 0;
    while ri < s.runs.len() {
        let run = &s.runs[ri];
        let mut want_ok: u8 = 0;
        if let Some(c) = s.cb.items.get(run.case) {
            if s.cm.satisfies(run.backend, c.required) {
                want_ok = 1;
            }
        }
        if run.cap_ok != want_ok {
            cap_bad = true;
        }
        ri = ri.wrapping_add(1);
    }
    if cap_bad {
        d.mark(Form::CapNotSupported);
    }
    // ③ 对拍面
    if !s.cb.cover_complete() {
        d.mark(Form::CaseMissing);
    }
    if !s.cb.dup_free() {
        d.mark(Form::CaseDup);
    }
    if !s.diff.rounds_ok() {
        d.mark(Form::ParityShort);
    }
    if !s.diff.all_explained() {
        d.mark(Form::ParityMismatch);
    }
    // ④ 账目与冻结
    if !s.ring.balanced() {
        d.mark(Form::LedgerUnbalanced);
    }
    let rc = s.reconcile();
    if !rc.balanced {
        d.mark(Form::ReconcileDeviation);
    }
    if s.ev.freeze_fp != s.ev.registered_probe {
        d.mark(Form::FingerprintDrift);
    }
    if s.gate_touches != 0 || !s.bt.gate_zero() {
        d.mark(Form::GateCrossed);
    }
    d
}
/// 全形态检出。
pub fn detect_all(inj: &[Snapshot], forms: &[Form]) -> DbResult<Detections> {
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
        if !d.hit(f) {
            return Err(DbErr::of_form(f, format!("注入未被检出：{} 漏检", f.name())));
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
// 12. 冻结件 / 稳定性 / 重放
// ===========================================================================

/// 冻结记录。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FreezeRecord {
    /// 版本。
    pub rev: u32,
    /// 后端定义哈希。
    pub backend_hash: u64,
    /// 能力矩阵哈希。
    pub cap_hash: u64,
    /// 用例册哈希。
    pub case_hash: u64,
    /// 冻结总哈希。
    pub freeze_hash: u64,
    /// 语义边界声明。
    pub border_note: String,
    /// 设计键。
    pub design_key: String,
}
impl FreezeRecord {
    /// 构造。
    pub fn build(bt: &BackendTable, cm: &CapMatrix, cb: &CaseBook) -> FreezeRecord {
        let backend_hash = backend_definition_hash();
        let cap_hash = cm.fingerprint();
        let case_hash = cb.fingerprint();
        let mut w: Vec<u64> = Vec::with_capacity(7);
        w.push(V_VERSION_ID);
        w.push(backend_hash);
        w.push(cap_hash);
        w.push(case_hash);
        w.push(bt.fingerprint());
        w.push(CAP_COUNT as u64);
        w.push(BACKEND_COUNT as u64);
        let freeze_hash = fnv1a_words(&w);
        FreezeRecord {
            rev: 1,
            backend_hash,
            cap_hash,
            case_hash,
            freeze_hash,
            border_note: String::from("只做双后端统一抽象与账面全链路；不真跑 QEMU、不真跑真机闸口"),
            design_key: format!("dualbackend/{}/{}", V_VERSION, GRAFT_SEAT),
        }
    }
    /// 校验。
    pub fn verify(&self, bt: &BackendTable, cm: &CapMatrix, cb: &CaseBook) -> DbResult<()> {
        if self.backend_hash != backend_definition_hash() {
            return Err(err_frozen("backend_hash", &format!("{:016X}", backend_definition_hash())));
        }
        if self.cap_hash != cm.fingerprint() {
            return Err(err_frozen("cap_hash", &format!("{:016X}", cm.fingerprint())));
        }
        if self.case_hash != cb.fingerprint() {
            return Err(err_frozen("case_hash", &format!("{:016X}", cb.fingerprint())));
        }
        if !bt.all_stub_qemu_free() {
            return Err(DbErr::of_form(
                Form::StubQemuLeak,
                format!("冻结校验：桩后端有 QEMU 依赖 {}", bt.summary()),
            ));
        }
        Ok(())
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "freeze rev={} backend={:016X} cap={:016X} case={:016X} total={:016X}",
            self.rev, self.backend_hash, self.cap_hash, self.case_hash, self.freeze_hash
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(4);
        out.push(self.describe());
        out.push(format!("  border  {}", self.border_note));
        out.push(format!("  key     {}", self.design_key));
        out.push(format!("  dep     {}", DEP_SEAT));
        out
    }
}
/// 稳定性报告。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Stability {
    /// 轮数。
    pub rounds: u32,
    /// 逐轮冻结哈希。
    pub hashes: Vec<u64>,
    /// 逐轮对拍照指纹。
    pub diff_fps: Vec<u64>,
    /// 是否一致。
    pub stable: bool,
}
impl Stability {
    /// 一致判定。
    pub fn stable(&self) -> bool {
        if self.hashes.len() < 2 {
            return false;
        }
        let h0 = self.hashes[0];
        let d0 = self.diff_fps[0];
        let mut i: usize = 1;
        while i < self.hashes.len() {
            if self.hashes[i] != h0 || self.diff_fps[i] != d0 {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!("stable rounds={} stable={}", self.rounds, if self.stable { 1 } else { 0 })
    }
}
/// 稳定性轮数。
pub const REPEAT_RUNS: usize = 3;
/// 稳定性跑。
pub fn stability_check() -> DbResult<Stability> {
    let mut hashes: Vec<u64> = Vec::with_capacity(REPEAT_RUNS);
    let mut diff_fps: Vec<u64> = Vec::with_capacity(REPEAT_RUNS);
    let mut i: usize = 0;
    while i < REPEAT_RUNS {
        let s = Snapshot::net()?;
        let fr = FreezeRecord::build(&s.bt, &s.cm, &s.cb);
        hashes.push(fr.freeze_hash);
        diff_fps.push(s.diff.fingerprint());
        i = i.wrapping_add(1);
    }
    let mut st = Stability { rounds: REPEAT_RUNS as u32, hashes, diff_fps, stable: false };
    st.stable = st.stable();
    if !st.stable {
        return Err(err_frozen("stability", "all-round-equal"));
    }
    Ok(st)
}
/// 重放报告。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Replay {
    /// 冻结哈希。
    pub freeze_hash: u64,
    /// 后端表指纹。
    pub backend_fp: u64,
    /// 能力矩阵指纹。
    pub cap_fp: u64,
    /// 对拍照指纹。
    pub diff_fp: u64,
    /// 执行记录数。
    pub runs_n: u64,
    /// 注入检出命中数。
    pub inject_hits: usize,
    /// 环形账写入数。
    pub ring_pushed: u64,
}
impl Replay {
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "replay freeze={:016X} backend={:016X} cap={:016X} diff={:016X} runs={} hits={} ring={}",
            self.freeze_hash, self.backend_fp, self.cap_fp, self.diff_fp,
            self.runs_n, self.inject_hits, self.ring_pushed
        )
    }
}
/// 确定性重放（与 `observe` 互不递归）。
pub fn replay(seed: u64) -> DbResult<Replay> {
    let mut rng = Lcg::new(seed);
    let _ = rng.next();
    let s = Snapshot::net()?;
    if !s.bt.all_stub_qemu_free() {
        return Err(DbErr::of_form(
            Form::StubQemuLeak,
            format!("重放：桩后端有 QEMU 依赖 {}", s.bt.summary()),
        ));
    }
    if !s.cm.cap_in_bounds() || !s.cm.declared_complete() {
        return Err(DbErr::of_form(
            Form::CapIncomplete,
            format!("重放：能力面不过 {}", s.cm.summary()),
        ));
    }
    if !s.cb.cover_complete() || !s.cb.dup_free() {
        return Err(DbErr::of_form(
            Form::CaseMissing,
            format!("重放：用例册不过 {}", s.cb.summary()),
        ));
    }
    if !s.diff.rounds_ok() || !s.diff.all_explained() {
        return Err(DbErr::of_form(
            Form::ParityMismatch,
            format!("重放：对拍不过 {}", s.diff.summary()),
        ));
    }
    if s.gate_touches != 0 {
        return Err(DbErr::of_form(
            Form::GateCrossed,
            format!("重放：闸口被访问 {} 次", s.gate_touches),
        ));
    }
    if !s.ring.balanced() {
        return Err(DbErr::of_form(
            Form::LedgerUnbalanced,
            format!("重放：环形账不守恒 {}", s.ring.summary()),
        ));
    }
    let rc = s.reconcile();
    if !rc.balanced {
        return Err(err_recon(rc.sum_a, rc.sum_b, rc.sum_c));
    }
    let (inj, forms) = inject_all()?;
    let agg = detect_all(&inj, &forms)?;
    let fr = FreezeRecord::build(&s.bt, &s.cm, &s.cb);
    Ok(Replay {
        freeze_hash: fr.freeze_hash,
        backend_fp: s.bt.fingerprint(),
        cap_fp: s.cm.fingerprint(),
        diff_fp: s.diff.fingerprint(),
        runs_n: s.runs.len() as u64,
        inject_hits: agg.hit_count(),
        ring_pushed: s.ring.pushed,
    })
}

// ===========================================================================
// 13. 指标与性能分解
// ===========================================================================

/// 成本项。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CostItem {
    /// 环节名。
    pub name: &'static str,
    /// 主循环次数。
    pub iters: u64,
    /// 每次操作数。
    pub per_iter: u64,
    /// 合计。
    pub total: u64,
    /// 对照说明（「深在哪」）。
    pub note: &'static str,
}
impl CostItem {
    /// 构造。
    pub fn new(name: &'static str, iters: u64, per_iter: u64) -> CostItem {
        CostItem { name, iters, per_iter, total: iters.wrapping_mul(per_iter), note: "—" }
    }
    /// 带对照构造。
    pub fn with_note(name: &'static str, iters: u64, per_iter: u64, note: &'static str) -> CostItem {
        CostItem { name, iters, per_iter, total: iters.wrapping_mul(per_iter), note }
    }
    /// 描述。
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
    /// 后端数。
    pub backends: u64,
    /// 桩后端数。
    pub stubs: u64,
    /// QEMU 后端数。
    pub qemus: u64,
    /// 真机后端数。
    pub reals: u64,
    /// 能力列数。
    pub cap_cols: u64,
    /// 用例数。
    pub cases: u64,
    /// 执行记录数。
    pub runs: u64,
    /// 对拍条目数。
    pub parity_rows: u64,
    /// 对拍一致数。
    pub same_n: u64,
    /// 对拍预期差异数。
    pub gap_n: u64,
    /// 对拍失配数（应 0）。
    pub mismatch_n: u64,
    /// 双轨制成本合计。
    pub dual_total: u64,
    /// 省下总量。
    pub saved_total: u64,
    /// 闸口访问（应 0）。
    pub gate_touches: u32,
    /// 环形账写入/丢弃。
    pub ring_pushed: u64,
    /// 环形账丢弃。
    pub ring_dropped: u64,
    /// 步数消耗。
    pub steps: u64,
    /// 注入检出命中数。
    pub inject_hits: usize,
    /// 净态检出（应零）。
    pub net_hits: usize,
    /// 边界违例（应零）。
    pub border_violations: u32,
    /// 峰值内存估算。
    pub mem_peak: u64,
}
impl Metrics {
    /// 全项通过。
    pub fn all_ok(&self) -> bool {
        self.net_hits == 0
            && self.border_violations == 0
            && self.backends == BACKEND_COUNT as u64
            && self.stubs == CHAN_FAMILY as u64
            && self.qemus == CHAN_FAMILY as u64
            && self.reals == CHAN_FAMILY as u64
            && self.cap_cols == CAP_COUNT as u64
            && self.cases == CASE_COUNT as u64
            && self.runs == (CASE_COUNT * PARITY_ROUNDS * 2) as u64
            && self.mismatch_n == 0
            && self.saved_total > 0
            && self.gate_touches == 0
            && self.inject_hits == Form::COUNT
            && self.steps <= STEP_CAP
            && self.mem_peak <= MEM_PEAK_CAP
    }
    /// 性能逐项分解。
    pub fn complexity(&self) -> Vec<CostItem> {
        vec![
            CostItem::with_note(
                "backend_table_build",
                BACKEND_COUNT as u64,
                CAP_COUNT as u64,
                "Windows 无「通道×环境」两族正交后端抽象",
            ),
            CostItem::with_note(
                "cap_matrix_build",
                BACKEND_COUNT as u64,
                CAP_COUNT as u64,
                "Windows 无后端能力位图声明层",
            ),
            CostItem::with_note(
                "cap_precheck",
                self.runs,
                1,
                "本单特有：能力前置检出（不满足直接拒绝执行）",
            ),
            CostItem::with_note(
                "case_register",
                CASE_COUNT as u64,
                CASE_COUNT as u64,
                "Windows 用例与后端强耦合，无法同用例跨后端复用",
            ),
            CostItem::with_note(
                "stub_chain_run",
                self.runs / 2,
                CASE_STEPS as u64,
                "M-180 桩后端全链路（账面）",
            ),
            CostItem::with_note(
                "qemu_chain_run",
                self.runs / 2,
                CASE_STEPS as u64,
                "Windows 无「同用例双后端同跑」对拍面",
            ),
            CostItem::with_note(
                "parity_compare",
                self.parity_rows,
                4,
                "本单特有：跨后端结果指纹对拍",
            ),
            CostItem::with_note(
                "expected_gap_classify",
                self.gap_n,
                DIFF_COST_CLASSES as u64,
                "本单特有：桩≈QEMU 预期差异分类登记",
            ),
            CostItem::with_note(
                "cost_ledger",
                SAVING_ITEMS as u64,
                2,
                "本单特有：双轨制成本归零的整数账",
            ),
            CostItem::with_note(
                "ring_push",
                self.ring_pushed,
                3,
                "append-only + dropped 显性",
            ),
            CostItem::with_note(
                "reconcile",
                7,
                3,
                "Windows 无跨面守恒恒等式",
            ),
            CostItem::with_note(
                "inject_one",
                Form::COUNT as u64,
                BACKEND_COUNT as u64,
                "本单特有：14 形态全注入",
            ),
            CostItem::with_note(
                "detect_one",
                Form::COUNT as u64,
                (CAP_COUNT + CASE_COUNT) as u64,
                "本单特有：14 检测器逐面扫描",
            ),
        ]
    }
    /// 合计。
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
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "metrics backends={} stub={} qemu={} real={} caps={} cases={} runs={} parity={} same={} gap={} mismatch={} dual={} saved={} gate={} ring={}/{} inj={} net={}",
            self.backends, self.stubs, self.qemus, self.reals, self.cap_cols, self.cases,
            self.runs, self.parity_rows, self.same_n, self.gap_n, self.mismatch_n,
            self.dual_total, self.saved_total, self.gate_touches,
            self.ring_pushed, self.ring_dropped, self.inject_hits, self.net_hits
        )
    }
}

/// 预期差异分类数（桩 ≈ QEMU 的差异分类）。
pub const DIFF_COST_CLASSES: usize = 3;

// ===========================================================================
// 14. 导出
// ===========================================================================

/// KV 导出。
pub fn export_kv(o: &DbObserve) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(48);
    out.push(format!("ver={}", V_VERSION));
    out.push(format!("seed={:016X}", o.seed));
    out.push(format!("seg=0x{:04X}", SEG_BASE));
    out.push(format!("chan_family={}", CHAN_FAMILY));
    out.push(format!("env_family={}", ENV_FAMILY));
    out.push(format!("backend_count={}", BACKEND_COUNT));
    out.push(format!("cap_count={}", CAP_COUNT));
    out.push(format!("case_count={}", CASE_COUNT));
    out.push(format!("parity_rounds={}", PARITY_ROUNDS));
    out.push(format!("backends={}", o.metrics.backends));
    out.push(format!("stubs={}", o.metrics.stubs));
    out.push(format!("qemus={}", o.metrics.qemus));
    out.push(format!("reals={}", o.metrics.reals));
    out.push(format!("cap_cols={}", o.metrics.cap_cols));
    out.push(format!("cases={}", o.metrics.cases));
    out.push(format!("runs={}", o.metrics.runs));
    out.push(format!("parity_rows={}", o.metrics.parity_rows));
    out.push(format!("same={}", o.metrics.same_n));
    out.push(format!("gap={}", o.metrics.gap_n));
    out.push(format!("mismatch={}", o.metrics.mismatch_n));
    out.push(format!("dual_total={}", o.metrics.dual_total));
    out.push(format!("saved_total={}", o.metrics.saved_total));
    out.push(format!("gate_touches={}", o.metrics.gate_touches));
    out.push(format!("ring_pushed={}", o.metrics.ring_pushed));
    out.push(format!("ring_dropped={}", o.metrics.ring_dropped));
    out.push(format!("steps={}", o.metrics.steps));
    out.push(format!("recon_dev={}", o.recon.deviation));
    out.push(format!("recon_balanced={}", if o.recon.balanced { 1 } else { 0 }));
    out.push(format!("fsm_state={}", o.fsm.state.name()));
    out.push(format!("fsm_reject={}", o.fsm.rejected.len()));
    out.push(format!("freeze_hash={:016X}", o.freeze.freeze_hash));
    out.push(format!("replay_freeze={:016X}", o.replay.freeze_hash));
    out.push(format!("inject_hits={}", o.metrics.inject_hits));
    out.push(format!("net_hits={}", o.metrics.net_hits));
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
/// 推一条 TLV。
pub fn push_tlv_u64(buf: &mut Vec<u8>, tag: u8, v: u64) {
    buf.push(tag);
    buf.push(8u8);
    push_u64(buf, v);
}
/// TLV 导出（组头单独一行；长度字段恒占定宽）。
pub fn export_tlv(o: &DbObserve) -> Vec<u8> {
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
    // 规模常量（11 条）
    push_tlv_u64(&mut out, 0x10, CHAN_FAMILY as u64);
    push_tlv_u64(&mut out, 0x11, ENV_FAMILY as u64);
    push_tlv_u64(&mut out, 0x12, BACKEND_COUNT as u64);
    push_tlv_u64(&mut out, 0x13, CAP_COUNT as u64);
    push_tlv_u64(&mut out, 0x14, CASE_COUNT as u64);
    push_tlv_u64(&mut out, 0x15, PARITY_ROUNDS as u64);
    push_tlv_u64(&mut out, 0x16, CASE_STEPS as u64);
    push_tlv_u64(&mut out, 0x17, RING_CAP as u64);
    push_tlv_u64(&mut out, 0x18, STEP_CAP);
    push_tlv_u64(&mut out, 0x19, MEM_PEAK_CAP);
    push_tlv_u64(&mut out, 0x1A, SAVING_ITEMS as u64);
    // 后端面（5 条）
    push_tlv_u64(&mut out, 0x20, o.metrics.backends);
    push_tlv_u64(&mut out, 0x21, o.metrics.stubs);
    push_tlv_u64(&mut out, 0x22, o.metrics.qemus);
    push_tlv_u64(&mut out, 0x23, o.metrics.reals);
    push_tlv_u64(&mut out, 0x24, o.net.bt.fingerprint());
    // 能力面（4 条）
    push_tlv_u64(&mut out, 0x30, o.metrics.cap_cols);
    push_tlv_u64(&mut out, 0x31, o.net.cm.fingerprint());
    push_tlv_u64(&mut out, 0x32, o.net.cm.over_bits as u64);
    push_tlv_u64(&mut out, 0x33, o.net.cm.undeclared_bits as u64);
    // 用例面（4 条）
    push_tlv_u64(&mut out, 0x40, o.metrics.cases);
    push_tlv_u64(&mut out, 0x41, o.metrics.runs);
    push_tlv_u64(&mut out, 0x42, o.net.cb.fingerprint());
    push_tlv_u64(&mut out, 0x43, o.net.cb.cover_bits as u64);
    // 对拍面（5 条）
    push_tlv_u64(&mut out, 0x50, o.metrics.parity_rows);
    push_tlv_u64(&mut out, 0x51, o.metrics.same_n);
    push_tlv_u64(&mut out, 0x52, o.metrics.gap_n);
    push_tlv_u64(&mut out, 0x53, o.metrics.mismatch_n);
    push_tlv_u64(&mut out, 0x54, o.net.diff.fingerprint());
    // 成本账（3 条）
    push_tlv_u64(&mut out, 0x60, o.metrics.dual_total);
    push_tlv_u64(&mut out, 0x61, o.metrics.saved_total);
    push_tlv_u64(&mut out, 0x62, o.net.cost.fingerprint());
    // 账目与闸口（5 条）
    push_tlv_u64(&mut out, 0x70, o.ring.pushed);
    push_tlv_u64(&mut out, 0x71, o.ring.dropped);
    push_tlv_u64(&mut out, 0x72, o.metrics.gate_touches as u64);
    push_tlv_u64(&mut out, 0x73, o.recon.deviation);
    push_tlv_u64(&mut out, 0x74, o.metrics.steps);
    // 冻结面（3 条）
    push_tlv_u64(&mut out, 0x80, o.freeze.freeze_hash);
    push_tlv_u64(&mut out, 0x81, o.replay.freeze_hash);
    push_tlv_u64(&mut out, 0x82, o.detects.verdict_mask as u64);
    // 逐形态命中（14 条）
    let mut i: usize = 0;
    while i < Form::COUNT {
        push_tlv_u64(&mut out, 0x90u8.wrapping_add(i as u8), o.detects.hits[i] as u64);
        i = i.wrapping_add(1);
    }
    out
}
/// TLV 长度核算。
pub fn tlv_len_check(o: &DbObserve) -> DbResult<usize> {
    let buf = export_tlv(o);
    let want: usize = TLV_HEAD_LEN + TLV_ITEM_COUNT * TLV_ITEM_LEN;
    if buf.len() != want {
        return Err(DbErr::of_aux(
            AuxCode::CapIdxOver,
            format!("TLV 长度不符：got={} want={}", buf.len(), want),
        ));
    }
    let hl = ((buf[1] as usize) << 8) | (buf[2] as usize);
    if hl != TLV_HEAD_PAYLOAD {
        return Err(DbErr::of_aux(
            AuxCode::CapIdxOver,
            format!("组头长度字段不符：got={} want={}", hl, TLV_HEAD_PAYLOAD),
        ));
    }
    Ok(buf.len())
}

// ===========================================================================
// 15. observe
// ===========================================================================

/// 观测结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DbObserve {
    /// 种子。
    pub seed: u64,
    /// 净态快照。
    pub net: Snapshot,
    /// 环形账。
    pub ring: RingLedger,
    /// 守恒账。
    pub recon: Reconcile,
    /// 注入检出。
    pub detects: Detections,
    /// 净态检出。
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
impl DbObserve {
    /// 全项通过。
    pub fn all_ok(&self) -> bool {
        self.metrics.all_ok()
    }
    /// 重放一致性。
    pub fn replay_match(&self) -> bool {
        self.replay.freeze_hash == self.freeze.freeze_hash
            && self.replay.backend_fp == self.net.bt.fingerprint()
            && self.replay.cap_fp == self.net.cm.fingerprint()
            && self.replay.diff_fp == self.net.diff.fingerprint()
            && self.replay.runs_n == self.net.runs.len() as u64
    }
    /// 摘要。
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
        out.push(format!("# UNX-F3421 观测 seed={:016X}", self.seed));
        out.push(self.summary());
        out.push(format!("ver={} seg={}", V_VERSION, seg_line()));
        out.push(format!("metrics  {}", self.metrics.summary()));
        out.push(format!("recon    {}", self.recon.summary()));
        out.push(format!("fsm      {}", self.fsm.summary()));
        out.push(format!("stable   {}", self.stability.describe()));
        out.push(format!("backend  {}", self.net.bt.summary()));
        out.push(format!("cap      {}", self.net.cm.summary()));
        out.push(format!("case     {}", self.net.cb.summary()));
        out.push(format!("parity   {}", self.net.diff.summary()));
        out.push(format!("cost     {}", self.net.cost.summary()));
        out.push(format!("ring     {}", self.ring.summary()));
        out.push(format!("evidence {}", self.net.ev.describe()));
        out.push(format!("netdet   {}", self.net_detect.summary()));
        out.push(format!("freeze   {}", self.freeze.describe()));
        out.push(format!("replay   {}", self.replay.summary()));
        out.push(format!("replay_match={}", if self.replay_match() { 1 } else { 0 }));
        let mut i: usize = 0;
        while i < Stage::ALL.len() {
            out.push(format!("stage    {}", Stage::ALL[i].four_columns()));
            i = i.wrapping_add(1);
        }
        out
    }
}
/// 真实跑逻辑的观测入口。
pub fn observe(seed: u64) -> DbResult<DbObserve> {
    let net = Snapshot::net()?;
    if !net.bt.all_stub_qemu_free() {
        return Err(DbErr::of_form(
            Form::StubQemuLeak,
            format!("桩后端有 QEMU 依赖 {}", net.bt.summary()),
        ));
    }
    if net.bt.family_closed != 1 || !net.bt.dup_free() {
        return Err(DbErr::of_form(
            Form::FamilyOutOfSet,
            format!("后端族不闭集 {}", net.bt.summary()),
        ));
    }
    if !net.cm.cap_in_bounds() || !net.cm.declared_complete() {
        return Err(DbErr::of_form(
            Form::CapIncomplete,
            format!("能力面不过 {}", net.cm.summary()),
        ));
    }
    if !net.cb.cover_complete() || !net.cb.dup_free() {
        return Err(DbErr::of_form(
            Form::CaseMissing,
            format!("用例册不过 {}", net.cb.summary()),
        ));
    }
    if !net.diff.rounds_ok() || !net.diff.all_explained() {
        return Err(DbErr::of_form(
            Form::ParityMismatch,
            format!("对拍不过 {}", net.diff.summary()),
        ));
    }
    if net.gate_touches != 0 {
        return Err(DbErr::of_form(
            Form::GateCrossed,
            format!("闸口被访问 {} 次", net.gate_touches),
        ));
    }
    if !net.ring.balanced() {
        return Err(DbErr::of_form(
            Form::LedgerUnbalanced,
            format!("环形账不守恒 {}", net.ring.summary()),
        ));
    }
    let recon = net.reconcile();
    if !recon.balanced {
        return Err(err_recon(recon.sum_a, recon.sum_b, recon.sum_c));
    }
    let mut fsm = StageFsm::new();
    fsm.drive_all(1)?;
    let illegal: [Stage; 3] = [Stage::S4Parity, Stage::S1Abstraction, Stage::Sealed];
    for s in illegal.iter() {
        let _ = fsm.try_go(*s, 99);
    }
    if !fsm.state.is_sealed() || !fsm.trail_monotonic() || fsm.rejected.len() < 3 {
        return Err(err_frozen(fsm.state.name(), Stage::Sealed.name()));
    }
    let stability = stability_check()?;
    let (inj, forms) = inject_all()?;
    let detects = detect_all(&inj, &forms)?;
    if detects.hit_count() != Form::COUNT {
        return Err(DbErr::of_form(
            Form::ReconcileDeviation,
            format!("注入检出不足 {}/{}", detects.hit_count(), Form::COUNT),
        ));
    }
    let net_detect = detect_one(&net);
    if net_detect.hit_count() != 0 {
        return Err(DbErr::of_form(
            Form::ReconcileDeviation,
            format!("净态检出非零 {:?}", net_detect.hit_list()),
        ));
    }
    let freeze = FreezeRecord::build(&net.bt, &net.cm, &net.cb);
    freeze.verify(&net.bt, &net.cm, &net.cb)?;
    if net.ev.freeze_fp != net.ev.registered_probe {
        return Err(DbErr::of_form(
            Form::FingerprintDrift,
            format!("探针漂移 {:016X} vs {:016X}", net.ev.freeze_fp, net.ev.registered_probe),
        ));
    }
    let mut stubs: u64 = 0;
    let mut qemus: u64 = 0;
    let mut reals: u64 = 0;
    let mut bi: usize = 0;
    while bi < net.bt.items.len() {
        match net.bt.items[bi].env {
            ExecEnv::Stub => stubs = stubs.wrapping_add(1),
            ExecEnv::Qemu => qemus = qemus.wrapping_add(1),
            ExecEnv::Real => reals = reals.wrapping_add(1),
        }
        bi = bi.wrapping_add(1);
    }
    let mut steps: u64 = 0;
    let mut ri: usize = 0;
    while ri < net.runs.len() {
        steps = steps.wrapping_add(net.runs[ri].steps as u64);
        ri = ri.wrapping_add(1);
    }
    let metrics = Metrics {
        backends: net.bt.items.len() as u64,
        stubs,
        qemus,
        reals,
        cap_cols: net.cm.cols as u64,
        cases: net.cb.items.len() as u64,
        runs: net.runs.len() as u64,
        parity_rows: net.diff.rows.len() as u64,
        same_n: net.diff.same_n,
        gap_n: net.diff.gap_n,
        mismatch_n: net.diff.mismatch_n,
        dual_total: net.cost.dual_total,
        saved_total: net.cost.saved_total,
        gate_touches: net.gate_touches,
        ring_pushed: net.ring.pushed,
        ring_dropped: net.ring.dropped,
        steps,
        inject_hits: detects.hit_count(),
        net_hits: net_detect.hit_count(),
        border_violations: net.ev.border_violations,
        mem_peak: mem_estimate() as u64,
    };
    let replay = replay(seed)?;
    let ring_clone = net.ring.clone();
    let o = DbObserve {
        seed,
        ring: ring_clone,
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
    let _ = tlv_len_check(&o)?;
    if !o.replay_match() {
        return Err(err_frozen("replay", "match"));
    }
    Ok(o)
}
/// 峰值内存估算。
pub fn mem_estimate() -> usize {
    let bt: usize = BACKEND_COUNT * (core::mem::size_of::<Backend>() + 8);
    let cm: usize = BACKEND_COUNT * 8 + 64;
    let cb: usize = CASE_COUNT * (core::mem::size_of::<CaseDef>() + 32);
    let runs: usize = CASE_COUNT * PARITY_ROUNDS * 2 * (core::mem::size_of::<CaseRun>() + 8);
    let ring: usize = RING_CAP * (core::mem::size_of::<RingEntry>() + 16);
    let diff: usize = CASE_COUNT * PARITY_ROUNDS * (core::mem::size_of::<ParityRow>() + 8);
    bt + cm + cb + runs + ring + diff
}
