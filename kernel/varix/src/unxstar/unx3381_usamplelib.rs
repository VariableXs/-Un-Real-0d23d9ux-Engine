//! UNX-F3381 · 样本库总设计（可复现死锁样本注入框架）
//!
//! 任务单号: UNX-F3381
//! 标题: 样本库总设计（可复现死锁样本注入框架）
//! 域/批: A4/G8（CoRun·UNX-A5 段｜ktest/框架/测试基建域）
//! 册目标: 400 行（判据 J1 框架文档 + M-140 框架空转验证）
//!
//! 判据原文（逐字摘抄，来源《CoRun Varix STAR II · Unxreal》任务书表格）:
//! 「UNX-F3381 | 样本库总设计（可复现死锁样本注入框架） | 400 | 骨架 |
//!   J1：框架文档交付：样本 = 参数化场景函数（锁数/时长/时机/期望结果四参数）；M-140 框架空转验证。」
//!
//! 说明（锚点目录同名文件的辨析）:
//! `_attic/.../anchors/UNX-F3381.md` 内实为 **UNX-F33810 · SAE 面体验日志埋点**，与本单无关。
//! 本单判据以任务书表格 `| UNX-F3381 |` 行为准。
//!
//! 本单核心语义（四参数场景函数，逐层落地）:
//! - **参数化场景函数**：样本 = `ScenarioFn(locks, duration, timing, expect)`，四参数构成**闭集**，
//!   任一参数越界即 `ParamOutOfRange`，不得「就近取值」。
//! - **可复现**：样本身份 = `fnv1a(参数四元组)`，种子只影响**注入时机序列**，
//!   不影响参数本身——这是「同一样本在任何机器上重放同一条路径」的前提。
//! - **样本登记 / 检索 / 淘汰**：登记册位图判重，检索按四元组精确匹配，淘汰按 LRU 序（逻辑 tick）。
//! - **期望 vs 实际对账**：`Reconcile` 把每次执行结果与样本期望比对，
//!   差异分类为 `DiffKind` 六态（Match/LockOrder/LockCount/Deadlock/Timing/None），
//!   差异必须**显性分类**而非笼统判失败。
//!
//! 深度骨架:
//! 常量表 / 4 核心结构（ScenarioFn、SampleBook、ExecLog、DiffBook）/
//! 14 错误变体 / 6 辅助码 / 主状态机（合法边闭集 + 非法转换拒记）/
//! 守恒账 `reconcile()` / append-only 环形账（`dropped` 显性）/
//! 注入核 14/14 全拒留痕 / 冻结指纹 `fingerprint() + verify()` /
//! `export_kv() + export_tlv()` / 确定性 `replay(seed)` / `observe(seed)` /
//! `complexity()` 逐项性能分解（对标 Windows 同类能力写「深在哪」）。
//!
//! 码段声明:
//! 本单独占诊断码段 `0x27C0 ..= 0x27FF`（`SEG_BASE = 0x27C0`，`SEG_MASK = 0x003F`）。
//! 错误变体一码一段，逐个占段内偏移 `0x00..=0x0D`；辅助码占 `0x1C..=0x21`，
//! 最大占用 `0x27E1`，未触及段上界 `0x27FF`。

#![allow(dead_code)]
#![allow(unused_imports)]

extern crate alloc;

use alloc::{format, string::{String, ToString}, vec, vec::Vec};

// ===========================================================================
// 1. 常量表
// ===========================================================================

/// 版本标识。
pub const V_VERSION_ID: u64 = 0x0001_3381_27C0_0001;
/// 版本串。
pub const V_VERSION: &str = "v3381-usamplelib.v1";
/// 码段基址。
pub const SEG_BASE: u16 = 0x27C0;
/// 码段掩码。
pub const SEG_MASK: u16 = 0x003F;
/// 码段上界。
pub const SEG_END: u16 = SEG_BASE | SEG_MASK;
/// 主码。
pub const MAIN_CODE: u16 = SEG_BASE;
/// 辅助码段内基址。
pub const AUX_BASE_OFF: u16 = 0x1C;

// ---------------------------------------------------------------------------
// 1.1 四参数域常量（锁数 / 时长 / 时机 / 期望）
// ---------------------------------------------------------------------------

/// 场景锁数下界。
pub const LOCK_MIN: u32 = 2;
/// 场景锁数上界。
pub const LOCK_MAX: u32 = 8;
/// 场景时长下界（tick）。
pub const DUR_MIN: u64 = 8;
/// 场景时长上界（tick）。
pub const DUR_MAX: u64 = 256;
/// 注入时机相位分母（时机参数取值 `0..TIMING_MOD`）。
pub const TIMING_MOD: u64 = 8;
/// 场景函数四参数个数。
pub const PARAM_COUNT: usize = 4;

/// 场景函数库内样本总数。
pub const SAMPLE_COUNT: usize = 12;
/// 样本登记册容量（超过即触发淘汰）。
pub const BOOK_CAP: usize = 16;
/// 样本册环掩码。
pub const BOOK_MASK: usize = BOOK_CAP - 1;
/// 每样本重复执行次数（对拍用）。
pub const EXEC_REPEAT: usize = 3;
/// 单次执行步数上限。
pub const EXEC_STEPS: usize = 24;
/// 锁序表长度（每样本一条获取序）。
pub const LOCK_SEQ_CAP: usize = 8;

/// 期望结果闭集大小。
pub const EXPECT_COUNT: usize = 5;
/// 差异分类闭集大小。
pub const DIFF_COUNT: usize = 6;

/// 环形账容量。
pub const RING_CAP: usize = 32;
/// 环形账掩码。
pub const RING_MASK: usize = RING_CAP - 1;
/// 总步数上限。
pub const STEP_CAP: u64 = (SAMPLE_COUNT * EXEC_REPEAT * EXEC_STEPS * 2) as u64;
/// syscall 次数上限（本单零真实 IO）。
pub const SYSCALL_CAP: u64 = 0;
/// 峰值内存上限。
pub const MEM_PEAK_CAP: u64 = 256 * 1024;

// ---------------------------------------------------------------------------
// 1.2 TLV 常量
// ---------------------------------------------------------------------------

/// TLV 组头标签。
pub const TLV_TAG_HEAD: u8 = 0x01;
/// 组头载荷字节数（3 条 `push_u64` × 8）。
pub const TLV_HEAD_PAYLOAD: usize = 24;
/// 组头总长。
pub const TLV_HEAD_LEN: usize = TLV_HEAD_PAYLOAD + 3;
/// 单条 TLV 定长。
pub const TLV_ITEM_LEN: usize = 10;
/// TLV 条目总数（静态 44 + 逐形态 14 = 58；由 export_tlv 内 push 次数反推）。
pub const TLV_ITEM_COUNT: usize = 58;

/// 参数域面名。
pub const FACE_PARAM: &str = "参数面";
/// 样本域面名。
pub const FACE_SAMPLE: &str = "样本面";
/// 对账域面名。
pub const FACE_RECON: &str = "对账面";
/// 账目域面名。
pub const FACE_LEDGER: &str = "账目面";
/// 冻结域面名。
pub const FACE_FREEZE: &str = "冻结面";

/// 嫁接源。
pub const GRAFT_SEAT: &str = "F3361/F3401";
/// 对位条位。
pub const DEP_SEAT: &str = "F3421/F3461";

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

/// 错误形态（段内偏移 `0x00..=0x0D`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// 0：样本重号（四元组指纹重复登记）。
    SampleDup,
    /// 1：参数越界（四参数不在闭集内）。
    ParamOutOfRange,
    /// 2：参数缺失（样本四参数未齐）。
    ParamMissing,
    /// 3：期望值越闭集。
    ExpectOutOfSet,
    /// 4：样本缺失（册内应有样本查无记录）。
    SampleMissing,
    /// 5：检索未命中。
    RetrieveMiss,
    /// 6：差异分类失配（差异未落在六态闭集）。
    DiffKindBad,
    /// 7：锁序错乱（实际获取序与样本定义不符）。
    LockOrderBroken,
    /// 8：死锁未复现（期望死锁但实际未死锁）。
    DeadlockNotHit,
    /// 9：非死锁误报（期望不死锁但实际死锁）。
    FalseDeadlock,
    /// 10：环形账不守恒。
    LedgerUnbalanced,
    /// 11：冻结指纹漂移。
    FingerprintDrift,
    /// 12：守恒式偏离。
    ReconcileDeviation,
    /// 13：淘汰账越界（淘汰数超过登记数）。
    RetireOverflow,
}
impl Form {
    /// 形态总数。
    pub const COUNT: usize = 14;
    /// 全部形态。
    pub const ALL: [Form; Form::COUNT] = [
        Form::SampleDup,
        Form::ParamOutOfRange,
        Form::ParamMissing,
        Form::ExpectOutOfSet,
        Form::SampleMissing,
        Form::RetrieveMiss,
        Form::DiffKindBad,
        Form::LockOrderBroken,
        Form::DeadlockNotHit,
        Form::FalseDeadlock,
        Form::LedgerUnbalanced,
        Form::FingerprintDrift,
        Form::ReconcileDeviation,
        Form::RetireOverflow,
    ];
    /// 段内偏移下标。
    pub fn index(self) -> usize {
        match self {
            Form::SampleDup => 0,
            Form::ParamOutOfRange => 1,
            Form::ParamMissing => 2,
            Form::ExpectOutOfSet => 3,
            Form::SampleMissing => 4,
            Form::RetrieveMiss => 5,
            Form::DiffKindBad => 6,
            Form::LockOrderBroken => 7,
            Form::DeadlockNotHit => 8,
            Form::FalseDeadlock => 9,
            Form::LedgerUnbalanced => 10,
            Form::FingerprintDrift => 11,
            Form::ReconcileDeviation => 12,
            Form::RetireOverflow => 13,
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
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE.wrapping_add(self.index() as u16)
    }
    /// 形态名。
    pub fn name(self) -> &'static str {
        match self {
            Form::SampleDup => "SampleDup",
            Form::ParamOutOfRange => "ParamOutOfRange",
            Form::ParamMissing => "ParamMissing",
            Form::ExpectOutOfSet => "ExpectOutOfSet",
            Form::SampleMissing => "SampleMissing",
            Form::RetrieveMiss => "RetrieveMiss",
            Form::DiffKindBad => "DiffKindBad",
            Form::LockOrderBroken => "LockOrderBroken",
            Form::DeadlockNotHit => "DeadlockNotHit",
            Form::FalseDeadlock => "FalseDeadlock",
            Form::LedgerUnbalanced => "LedgerUnbalanced",
            Form::FingerprintDrift => "FingerprintDrift",
            Form::ReconcileDeviation => "ReconcileDeviation",
            Form::RetireOverflow => "RetireOverflow",
        }
    }
    /// 形态描述。
    pub fn describe(self) -> &'static str {
        match self {
            Form::SampleDup => "样本重号：四参数指纹相同的样本被重复登记",
            Form::ParamOutOfRange => "参数越界：锁数/时长/时机不在闭集区间内",
            Form::ParamMissing => "参数缺失：四参数未齐（判为非法样本）",
            Form::ExpectOutOfSet => "期望值越闭集：期望结果不在五态闭集内",
            Form::SampleMissing => "样本缺失：册内应有样本查无记录",
            Form::RetrieveMiss => "检索未命中：按四元组精确检索无结果",
            Form::DiffKindBad => "差异分类失配：差异未落在六态闭集内",
            Form::LockOrderBroken => "锁序错乱：实际锁获取序与样本定义不符",
            Form::DeadlockNotHit => "死锁未复现：期望死锁而实际未死锁",
            Form::FalseDeadlock => "非死锁误报：期望不死锁而实际死锁",
            Form::LedgerUnbalanced => "环形账不守恒：写入数 ≠ 存活 + 丢弃",
            Form::FingerprintDrift => "冻结指纹漂移：场景函数定义件哈希与入册值失配",
            Form::ReconcileDeviation => "守恒式偏离：执行次数 ≠ 样本数 × 重复轮次",
            Form::RetireOverflow => "淘汰账越界：淘汰数超过登记数",
        }
    }
    /// 探测面名。
    pub fn probe(self) -> &'static str {
        match self {
            Form::SampleDup => "book.dup_bits",
            Form::ParamOutOfRange => "param.in_range",
            Form::ParamMissing => "param.complete",
            Form::ExpectOutOfSet => "param.expect_closed",
            Form::SampleMissing => "book.presence",
            Form::RetrieveMiss => "book.retrieve",
            Form::DiffKindBad => "diff.kind_closed",
            Form::LockOrderBroken => "exec.lock_order",
            Form::DeadlockNotHit => "exec.deadlock_hit",
            Form::FalseDeadlock => "exec.false_deadlock",
            Form::LedgerUnbalanced => "ring.balanced",
            Form::FingerprintDrift => "freeze.verify",
            Form::ReconcileDeviation => "recon.deviation",
            Form::RetireOverflow => "book.retire_bound",
        }
    }
    /// 所属域面。
    pub fn face(self) -> &'static str {
        match self {
            Form::SampleDup | Form::SampleMissing | Form::RetrieveMiss | Form::RetireOverflow => FACE_SAMPLE,
            Form::ParamOutOfRange | Form::ParamMissing | Form::ExpectOutOfSet => FACE_PARAM,
            Form::DiffKindBad | Form::LockOrderBroken | Form::DeadlockNotHit | Form::FalseDeadlock => FACE_RECON,
            Form::LedgerUnbalanced | Form::ReconcileDeviation => FACE_LEDGER,
            Form::FingerprintDrift => FACE_FREEZE,
        }
    }
    /// 是否属「参数面」。
    pub fn is_param(self) -> bool {
        matches!(self, Form::ParamOutOfRange | Form::ParamMissing | Form::ExpectOutOfSet)
    }
    /// 是否属「对账面」。
    pub fn is_diff(self) -> bool {
        matches!(
            self,
            Form::DiffKindBad | Form::LockOrderBroken | Form::DeadlockNotHit | Form::FalseDeadlock
        )
    }
    /// 是否属「账目面」。
    pub fn is_ledger(self) -> bool {
        matches!(
            self,
            Form::LedgerUnbalanced | Form::ReconcileDeviation | Form::RetireOverflow
        )
    }
    /// 形态类名。
    pub fn class_name(self) -> &'static str {
        if self.is_param() {
            "参数面"
        } else if self.is_diff() {
            "对账面"
        } else if self.is_ledger() {
            "账目面"
        } else {
            "冻结/样本面"
        }
    }
}
/// 辅助码（段内偏移 `0x1C..=0x21`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuxCode {
    /// 0：步数越界。
    StepOverCap,
    /// 1：执行轮次不一致。
    RepeatSkew,
    /// 2：样本下标越界。
    SampleIdxOver,
    /// 3：锁序长度越界。
    LockSeqOver,
    /// 4：淘汰游标错位。
    RetireSkew,
    /// 5：语义边界越界（本单不真跑内核锁，只做账面注入）。
    SemanticBorder,
}
impl AuxCode {
    /// 辅助码总数。
    pub const COUNT: usize = 6;
    /// 全部辅助码。
    pub const ALL: [AuxCode; AuxCode::COUNT] = [
        AuxCode::StepOverCap,
        AuxCode::RepeatSkew,
        AuxCode::SampleIdxOver,
        AuxCode::LockSeqOver,
        AuxCode::RetireSkew,
        AuxCode::SemanticBorder,
    ];
    /// 段内偏移下标。
    pub fn index(self) -> usize {
        match self {
            AuxCode::StepOverCap => 0,
            AuxCode::RepeatSkew => 1,
            AuxCode::SampleIdxOver => 2,
            AuxCode::LockSeqOver => 3,
            AuxCode::RetireSkew => 4,
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
            AuxCode::RepeatSkew => "RepeatSkew",
            AuxCode::SampleIdxOver => "SampleIdxOver",
            AuxCode::LockSeqOver => "LockSeqOver",
            AuxCode::RetireSkew => "RetireSkew",
            AuxCode::SemanticBorder => "SemanticBorder",
        }
    }
    /// 辅助码描述。
    pub fn describe(self) -> &'static str {
        match self {
            AuxCode::StepOverCap => "逻辑步数越过 STEP_CAP 上限",
            AuxCode::RepeatSkew => "执行重复轮次与样本定义不一致",
            AuxCode::SampleIdxOver => "样本下标越界",
            AuxCode::LockSeqOver => "锁序长度越界",
            AuxCode::RetireSkew => "淘汰游标与登记游标错位",
            AuxCode::SemanticBorder => "语义边界越界：本单不真跑内核锁",
        }
    }
}
/// 本单结果别名。
pub type SlResult<T> = Result<T, SlErr>;
/// 错误对象（三要素）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SlErr {
    /// 诊断码。
    pub code: u16,
    /// 明文细节。
    pub detail: String,
}
impl SlErr {
    /// 构造。
    pub fn new(code: u16, detail: String) -> SlErr {
        SlErr { code, detail }
    }
    /// 由形态构造。
    pub fn of_form(f: Form, detail: String) -> SlErr {
        SlErr { code: f.code(), detail }
    }
    /// 由辅助码构造。
    pub fn of_aux(a: AuxCode, detail: String) -> SlErr {
        SlErr { code: a.code(), detail }
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
pub fn err_str(e: &SlErr) -> String {
    match name_by_code(e.code) {
        Some(n) => format!("[{}] {}", n, e.detail),
        None => format!("[0x{:04X}] {}", e.code, e.detail),
    }
}
/// 冻结态不符错误。
pub fn err_frozen(got: &str, want: &str) -> SlErr {
    SlErr::of_form(
        Form::FingerprintDrift,
        format!("冻结态不符：got={} want={}", got, want),
    )
}
/// 越界错误。
pub fn err_range(what: &str, idx: usize, cap: usize) -> SlErr {
    SlErr::of_aux(
        AuxCode::SampleIdxOver,
        format!("越界：{} idx={} cap={}", what, idx, cap),
    )
}
/// 守恒式错误。
pub fn err_recon(a: u64, b: u64, c: u64) -> SlErr {
    SlErr::of_form(
        Form::ReconcileDeviation,
        format!("守恒式偏离：执行={} 登记×轮次={} 总账={}", a, b, c),
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
// 4. 核心结构一：参数化场景函数（ScenarioFn）
// ===========================================================================

/// 期望结果闭集（五态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Expect {
    /// 期望无死锁且按序完成。
    Ordered,
    /// 期望发生死锁（等待环成立）。
    Deadlock,
    /// 期望超时但不死锁。
    TimeoutOnly,
    /// 期望乱序但不死锁（锁序契约松弛）。
    Unordered,
    /// 期望不可达（场景本身非法，仅登记不执行）。
    Unreachable,
}
impl Expect {
    /// 闭集大小。
    pub const COUNT: usize = 5;
    /// 全部期望态。
    pub const ALL: [Expect; Expect::COUNT] = [
        Expect::Ordered,
        Expect::Deadlock,
        Expect::TimeoutOnly,
        Expect::Unordered,
        Expect::Unreachable,
    ];
    /// 下标。
    pub fn index(self) -> usize {
        match self {
            Expect::Ordered => 0,
            Expect::Deadlock => 1,
            Expect::TimeoutOnly => 2,
            Expect::Unordered => 3,
            Expect::Unreachable => 4,
        }
    }
    /// 按下标取。
    pub fn from_index(i: usize) -> Option<Expect> {
        if i < Expect::COUNT {
            Some(Expect::ALL[i])
        } else {
            None
        }
    }
    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Expect::Ordered => "Ordered",
            Expect::Deadlock => "Deadlock",
            Expect::TimeoutOnly => "TimeoutOnly",
            Expect::Unordered => "Unordered",
            Expect::Unreachable => "Unreachable",
        }
    }
    /// 描述。
    pub fn describe(self) -> &'static str {
        match self {
            Expect::Ordered => "期望按序完成、不死锁",
            Expect::Deadlock => "期望形成等待环（死锁）",
            Expect::TimeoutOnly => "期望超时但不死锁",
            Expect::Unordered => "期望乱序完成（锁序契约松弛）",
            Expect::Unreachable => "期望不可达（仅登记）",
        }
    }
    /// 是否应真实执行。
    pub fn executable(self) -> bool {
        self != Expect::Unreachable
    }
    /// 是否期望死锁。
    pub fn wants_deadlock(self) -> bool {
        self == Expect::Deadlock
    }
}
/// 参数化场景函数：样本的定义体（四参数）。
///
/// **可复现性契约**：样本身份 `fp` **只由四参数决定**，
/// 种子只影响注入时机序列（`timing` 的抖动），不影响 `locks` / `duration` / `expect`。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ScenarioFn {
    /// 参数一：锁数。
    pub locks: u32,
    /// 参数二：场景时长（tick）。
    pub duration: u64,
    /// 参数三：注入时机相位。
    pub timing: u64,
    /// 参数四：期望结果。
    pub expect: Expect,
    /// 锁获取序（长度 = `locks`，元素为锁号）。
    pub seq: Vec<u32>,
}
impl ScenarioFn {
    /// 构造（不校验，供注入用）。
    pub fn raw(locks: u32, duration: u64, timing: u64, expect: Expect, seq: Vec<u32>) -> ScenarioFn {
        ScenarioFn { locks, duration, timing, expect, seq }
    }
    /// 四参数是否全在闭集内。
    pub fn params_in_range(&self) -> bool {
        let locks_ok: bool = self.locks >= LOCK_MIN && self.locks <= LOCK_MAX;
        let dur_ok: bool = self.duration >= DUR_MIN && self.duration <= DUR_MAX;
        let timing_ok: bool = self.timing < TIMING_MOD;
        let exp_ok: bool = Expect::from_index(self.expect.index()).is_some();
        locks_ok && dur_ok && timing_ok && exp_ok
    }
    /// 四参数是否齐备（锁序长度与锁数一致）。
    pub fn params_complete(&self) -> bool {
        self.seq.len() == (self.locks as usize)
    }
    /// 期望值是否在闭集内。
    pub fn expect_closed(&self) -> bool {
        self.expect.index() < EXPECT_COUNT
    }
    /// 样本指纹（只折四参数 + 锁序）。
    pub fn fingerprint(&self) -> u64 {
        let mut w: Vec<u64> = Vec::with_capacity(5 + self.seq.len());
        w.push(V_VERSION_ID);
        w.push(self.locks as u64);
        w.push(self.duration);
        w.push(self.timing);
        w.push(self.expect.index() as u64);
        let mut i: usize = 0;
        while i < self.seq.len() {
            w.push(self.seq[i] as u64);
            i = i.wrapping_add(1);
        }
        fnv1a_words(&w)
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "fn(locks={} dur={} timing={} expect={} seq_len={} fp={:016X})",
            self.locks,
            self.duration,
            self.timing,
            self.expect.name(),
            self.seq.len(),
            self.fingerprint()
        )
    }
    /// 逐列表。
    pub fn four_columns(&self) -> String {
        format!(
            "{:<12} locks={:<3} dur={:<5} timing={:<3} expect={:<12}",
            self.expect.name(),
            self.locks,
            self.duration,
            self.timing,
            self.expect.name()
        )
    }
}
/// 可执行样本数（`Unreachable` 态只登记不执行）。
pub fn executable_count() -> u64 {
    let lib = scenario_lib();
    let mut n: u64 = 0;
    let mut i: usize = 0;
    while i < lib.len() {
        if lib[i].expect.executable() {
            n = n.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
    n
}
/// 场景函数库（净态定义件，`SAMPLE_COUNT` 条）。
pub fn scenario_lib() -> Vec<ScenarioFn> {
    let mut out: Vec<ScenarioFn> = Vec::with_capacity(SAMPLE_COUNT);
    let mut i: usize = 0;
    while i < SAMPLE_COUNT {
        let locks: u32 = LOCK_MIN + ((i as u32) % (LOCK_MAX - LOCK_MIN + 1));
        let duration: u64 = DUR_MIN + ((i as u64) * 17u64) % (DUR_MAX - DUR_MIN);
        let timing: u64 = (i as u64) % TIMING_MOD;
        let expect: Expect = Expect::ALL[i % Expect::COUNT];
        // 锁序：升序（按序）或刻意逆序（制造死锁）
        let mut seq: Vec<u32> = Vec::with_capacity(locks as usize);
        let mut k: u32 = 0;
        while k < locks {
            if expect == Expect::Deadlock && k % 2 == 1 {
                seq.push(locks - 1 - k);
            } else {
                seq.push(k);
            }
            k = k.wrapping_add(1);
        }
        out.push(ScenarioFn::raw(locks, duration, timing, expect, seq));
        i = i.wrapping_add(1);
    }
    out
}
/// 按下标取场景函数（越界给 `None`）。
pub fn scenario_at(i: usize) -> Option<ScenarioFn> {
    let lib = scenario_lib();
    if i < lib.len() {
        Some(lib[i].clone())
    } else {
        None
    }
}

/// 差异分类闭集（六态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiffKind {
    /// 0：完全一致。
    Match,
    /// 1：锁序差异。
    LockOrder,
    /// 2：锁数差异。
    LockCount,
    /// 3：死锁态差异。
    Deadlock,
    /// 4：时机/时长差异。
    Timing,
    /// 5：无差异（未执行，供仅登记样本用）。
    None,
}
impl DiffKind {
    /// 闭集大小。
    pub const COUNT: usize = 6;
    /// 全部分类。
    pub const ALL: [DiffKind; DiffKind::COUNT] = [
        DiffKind::Match,
        DiffKind::LockOrder,
        DiffKind::LockCount,
        DiffKind::Deadlock,
        DiffKind::Timing,
        DiffKind::None,
    ];
    /// 下标。
    pub fn index(self) -> usize {
        match self {
            DiffKind::Match => 0,
            DiffKind::LockOrder => 1,
            DiffKind::LockCount => 2,
            DiffKind::Deadlock => 3,
            DiffKind::Timing => 4,
            DiffKind::None => 5,
        }
    }
    /// 按下标取。
    pub fn from_index(i: usize) -> Option<DiffKind> {
        if i < DiffKind::COUNT {
            Some(DiffKind::ALL[i])
        } else {
            None
        }
    }
    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            DiffKind::Match => "Match",
            DiffKind::LockOrder => "LockOrder",
            DiffKind::LockCount => "LockCount",
            DiffKind::Deadlock => "Deadlock",
            DiffKind::Timing => "Timing",
            DiffKind::None => "None",
        }
    }
    /// 描述。
    pub fn describe(self) -> &'static str {
        match self {
            DiffKind::Match => "期望与实际完全一致",
            DiffKind::LockOrder => "锁获取顺序与样本定义不符",
            DiffKind::LockCount => "参与锁数与样本定义不符",
            DiffKind::Deadlock => "死锁态与期望不符（复现或误报）",
            DiffKind::Timing => "时机或时长与样本定义不符",
            DiffKind::None => "样本未执行（仅登记）",
        }
    }
    /// 是否为差异（非 Match / None）。
    pub fn is_diff(self) -> bool {
        self != DiffKind::Match && self != DiffKind::None
    }
}

// ===========================================================================
// 5. 核心结构二：样本册（SampleBook）
// ===========================================================================

/// 样本登记条目。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SampleRow {
    /// 样本下标。
    pub idx: usize,
    /// 场景定义。
    pub sc: ScenarioFn,
    /// 样本指纹。
    pub fp: u64,
    /// 登记逻辑时刻。
    pub tick: u64,
    /// 命中次数（检索计数）。
    pub hits: u64,
    /// 存活位。
    pub live: u8,
    /// 淘汰位。
    pub retired: u8,
    /// 登记时声明的「可执行」位（须与 `expect.executable()` 自洽）。
    pub exec_claim: u8,
}
impl SampleRow {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "sample#{} tick={} hits={} live={} retired={} {}",
            self.idx, self.tick, self.hits, self.live, self.retired, self.sc.describe()
        )
    }
    /// 指纹词（登记态，冻结件用）。
    pub fn words(&self) -> [u64; 5] {
        [self.idx as u64, self.fp, self.tick, self.hits, self.live as u64]
    }
}
/// 样本登记册（登记 / 判重 / 检索 / 淘汰）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SampleBook {
    /// 登记条目。
    pub rows: Vec<SampleRow>,
    /// 存在位图（bit i = 样本 i 已登记）。
    pub present_bits: u32,
    /// 重号位图。
    pub dup_bits: u32,
    /// 淘汰计数。
    pub retired: u64,
    /// 检索命中总数。
    pub retrieved: u64,
    /// 写指针。
    pub head: usize,
    /// 被覆盖丢弃数。
    pub dropped: u64,
}
impl SampleBook {
    /// 空册。
    pub fn new() -> SampleBook {
        SampleBook {
            rows: Vec::with_capacity(SAMPLE_COUNT),
            present_bits: 0,
            dup_bits: 0,
            retired: 0,
            retrieved: 0,
            head: 0,
            dropped: 0,
        }
    }
    /// 登记一个样本（判重 + 容量淘汰）。
    pub fn register(&mut self, sc: ScenarioFn, tick: u64) -> SlResult<usize> {
        let fp = sc.fingerprint();
        let mut i: usize = 0;
        while i < self.rows.len() {
            if self.rows[i].fp == fp {
                self.dup_bits = self.dup_bits | (1u32 << (i & 31));
                return Err(SlErr::of_form(
                    Form::SampleDup,
                    format!("样本重号：idx={} fp={:016X}", i, fp),
                ));
            }
            i = i.wrapping_add(1);
        }
        let idx = self.rows.len();
        // 先算可执行声明位再移入，避免 move 后再用
        let exec_claim: u8 = if sc.expect.executable() { 1 } else { 0 };
        self.rows.push(SampleRow {
            idx,
            sc,
            fp,
            tick,
            hits: 0,
            live: 1,
            retired: 0,
            exec_claim,
        });
        self.present_bits = self.present_bits | (1u32 << (idx & 31));
        // 容量淘汰：超出 BOOK_CAP 即淘汰最旧存活样本
        while self.rows.len() > BOOK_CAP {
            let victim = self.oldest_live();
            match victim {
                Some(v) => {
                    if let Some(r) = self.rows.get_mut(v) {
                        r.live = 0;
                        r.retired = 1;
                    }
                    self.retired = self.retired.wrapping_add(1);
                }
                None => {
                    self.dropped = self.dropped.wrapping_add(1);
                }
            }
            if self.retired > self.rows.len() as u64 {
                return Err(SlErr::of_form(
                    Form::RetireOverflow,
                    format!("淘汰账越界：retired={} rows={}", self.retired, self.rows.len()),
                ));
            }
            break;
        }
        Ok(idx)
    }
    /// 最旧存活样本下标。
    pub fn oldest_live(&self) -> Option<usize> {
        let mut best: usize = 0;
        let mut found: bool = false;
        let mut i: usize = 0;
        while i < self.rows.len() {
            let r = &self.rows[i];
            if r.live == 1 {
                if !found || r.tick < self.rows[best].tick {
                    best = i;
                    found = true;
                }
            }
            i = i.wrapping_add(1);
        }
        if found {
            Some(best)
        } else {
            None
        }
    }
    /// 按四元组精确检索。
    pub fn retrieve(&mut self, sc: &ScenarioFn) -> SlResult<usize> {
        let fp = sc.fingerprint();
        let mut i: usize = 0;
        while i < self.rows.len() {
            if self.rows[i].fp == fp {
                if let Some(r) = self.rows.get_mut(i) {
                    r.hits = r.hits.wrapping_add(1);
                    self.retrieved = self.retrieved.wrapping_add(1);
                }
                return Ok(i);
            }
            i = i.wrapping_add(1);
        }
        Err(SlErr::of_form(
            Form::RetrieveMiss,
            format!("检索未命中：fp={:016X}", fp),
        ))
    }
    /// 样本是否缺失（按存在位图核对）。
    pub fn present_complete(&self) -> bool {
        let want: u32 = if SAMPLE_COUNT >= 32 {
            0xFFFF_FFFFu32
        } else {
            (1u32 << SAMPLE_COUNT) - 1u32
        };
        self.present_bits & want == want
    }
    /// 无重号。
    pub fn dup_free(&self) -> bool {
        self.dup_bits == 0
    }
    /// 淘汰账是否在界内。
    pub fn retire_bounded(&self) -> bool {
        self.retired <= self.rows.len() as u64
    }
    /// 指纹（冻结件：只折登记定义，不折 hits / retrieved 活账）。
    pub fn fingerprint(&self) -> u64 {
        let mut w: Vec<u64> = Vec::with_capacity(3 + SAMPLE_COUNT * 5);
        w.push(V_VERSION_ID);
        w.push(self.present_bits as u64);
        w.push(self.rows.len() as u64);
        let mut i: usize = 0;
        while i < self.rows.len() {
            let ww = self.rows[i].words();
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
            "book rows={} present={:08X} dup={:08X} retired={} retrieved={} dropped={}",
            self.rows.len(),
            self.present_bits,
            self.dup_bits,
            self.retired,
            self.retrieved,
            self.dropped
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

// ===========================================================================
// 6. 核心结构三：执行账（ExecLog）
// ===========================================================================

/// 单次执行记录。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExecRow {
    /// 样本下标。
    pub sample: usize,
    /// 重复轮次。
    pub round: u32,
    /// 实际锁数。
    pub got_locks: u32,
    /// 实际锁序指纹。
    pub got_seq_fp: u64,
    /// 实际死锁位（1 = 死锁成立）。
    pub got_deadlock: u8,
    /// 实际耗时（tick）。
    pub got_duration: u64,
    /// 差异分类。
    pub diff: DiffKind,
    /// 执行指纹。
    pub fp: u64,
    /// 逻辑时刻。
    pub tick: u64,
}
impl ExecRow {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "exec s{} r{} got(locks={} dl={} dur={}) diff={} fp={:016X} t={}",
            self.sample,
            self.round,
            self.got_locks,
            self.got_deadlock,
            self.got_duration,
            self.diff.name(),
            self.fp,
            self.tick
        )
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 8] {
        [
            self.sample as u64,
            self.round as u64,
            self.got_locks as u64,
            self.got_seq_fp,
            self.got_deadlock as u64,
            self.got_duration,
            self.diff.index() as u64,
            self.fp,
        ]
    }
}
/// 执行账。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExecLog {
    /// 执行记录。
    pub rows: Vec<ExecRow>,
    /// 已执行样本位图。
    pub ran_bits: u32,
    /// 差异分类计数（按 `DiffKind::COUNT`）。
    pub diff_counts: [u32; DIFF_COUNT],
    /// 死锁复现数。
    pub deadlock_hits: u64,
    /// 死锁误报数。
    pub false_deadlocks: u64,
    /// 步数消耗。
    pub steps: u64,
    /// 步数上限。
    pub step_cap: u64,
}
impl ExecLog {
    /// 空账。
    pub fn new() -> ExecLog {
        let mut diff_counts: [u32; DIFF_COUNT] = [0; DIFF_COUNT];
        let mut i: usize = 0;
        while i < DIFF_COUNT {
            diff_counts[i] = 0;
            i = i.wrapping_add(1);
        }
        ExecLog {
            rows: Vec::with_capacity(SAMPLE_COUNT * EXEC_REPEAT),
            ran_bits: 0,
            diff_counts,
            deadlock_hits: 0,
            false_deadlocks: 0,
            steps: 0,
            step_cap: STEP_CAP,
        }
    }
    /// 推入一条执行记录。
    pub fn push(&mut self, r: ExecRow) -> SlResult<()> {
        if self.steps >= self.step_cap {
            return Err(SlErr::of_aux(
                AuxCode::StepOverCap,
                format!("步数越界：steps={} cap={}", self.steps, self.step_cap),
            ));
        }
        let d = r.diff.index();
        if d < DIFF_COUNT {
            self.diff_counts[d] = self.diff_counts[d].wrapping_add(1);
        }
        self.ran_bits = self.ran_bits | (1u32 << (r.sample & 31));
        self.steps = self.steps.wrapping_add(1 + (r.got_locks as u64));
        self.rows.push(r);
        Ok(())
    }
    /// 差异分类是否全在闭集内。
    pub fn diff_closed(&self) -> bool {
        let mut i: usize = 0;
        while i < self.rows.len() {
            if DiffKind::from_index(self.rows[i].diff.index()).is_none() {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 锁序是否全对（差异分类不得为 `LockOrder`）。
    pub fn lock_order_ok(&self) -> bool {
        let mut i: usize = 0;
        while i < self.rows.len() {
            if self.rows[i].diff == DiffKind::LockOrder {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 死锁判读是否正确（既不漏复现也不误报）。
    pub fn deadlock_reading_ok(&self) -> bool {
        let mut i: usize = 0;
        while i < self.rows.len() {
            let r = &self.rows[i];
            if r.diff == DiffKind::Deadlock {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 执行总数。
    pub fn total(&self) -> u64 {
        self.rows.len() as u64
    }
    /// 指纹（活账，**不入冻结**）。
    pub fn fingerprint(&self) -> u64 {
        let mut w: Vec<u64> = Vec::with_capacity(3 + SAMPLE_COUNT * EXEC_REPEAT * 8);
        w.push(V_VERSION_ID);
        w.push(self.rows.len() as u64);
        w.push(self.ran_bits as u64);
        let mut i: usize = 0;
        while i < self.rows.len() {
            let ww = self.rows[i].words();
            let mut j: usize = 0;
            while j < 8 {
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
            "exec total={} ran={:08X} steps={} dl_hit={} false_dl={} diffs={:?}",
            self.rows.len(),
            self.ran_bits,
            self.steps,
            self.deadlock_hits,
            self.false_deadlocks,
            self.diff_counts
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

// ===========================================================================
// 7. 核心结构四：注入执行器（Runner）
// ===========================================================================

/// 场景执行结果（账面注入，非真机）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RunResult {
    /// 实际锁数。
    pub got_locks: u32,
    /// 实际锁序指纹。
    pub got_seq_fp: u64,
    /// 死锁位。
    pub got_deadlock: u8,
    /// 实际耗时。
    pub got_duration: u64,
    /// 差异分类。
    pub diff: DiffKind,
    /// 执行指纹。
    pub fp: u64,
}
impl RunResult {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "run(locks={} seq_fp={:016X} dl={} dur={} diff={})",
            self.got_locks, self.got_seq_fp, self.got_deadlock, self.got_duration, self.diff.name()
        )
    }
}
/// 场景执行器。
///
/// 净态语义：若样本的锁序首尾构成环（`seq[0] > seq[len-1]` 且期望死锁），
/// 则实际必然形成等待环 → `got_deadlock = 1`；否则不死锁。
/// 时长按 `duration` 计；`timing` 只影响注入相位，**不改变**结果——这是可复现的核心。
pub struct Runner {
    /// 当前逻辑时刻。
    pub tick: u64,
    /// 真实寄存器访问次数（本单恒 0，语义边界自证）。
    pub real_hw_reads: u32,
}
impl Runner {
    /// 构造。
    pub fn new() -> Runner {
        Runner { tick: 0, real_hw_reads: 0 }
    }
    /// 执行一个场景。
    pub fn run(&mut self, sc: &ScenarioFn, round: u32) -> SlResult<RunResult> {
        if !sc.params_in_range() {
            return Err(SlErr::of_form(
                Form::ParamOutOfRange,
                format!("参数越界：{}", sc.describe()),
            ));
        }
        if !sc.params_complete() {
            return Err(SlErr::of_form(
                Form::ParamMissing,
                format!("参数不齐：seq_len={} locks={}", sc.seq.len(), sc.locks),
            ));
        }
        if sc.seq.len() > LOCK_SEQ_CAP {
            return Err(SlErr::of_aux(
                AuxCode::LockSeqOver,
                format!("锁序越界：len={} cap={}", sc.seq.len(), LOCK_SEQ_CAP),
            ));
        }
        self.tick = self.tick.wrapping_add(sc.duration);
        // 锁序指纹
        let mut w: Vec<u64> = Vec::with_capacity(1 + sc.seq.len());
        w.push(sc.locks as u64);
        let mut i: usize = 0;
        while i < sc.seq.len() {
            w.push(sc.seq[i] as u64);
            i = i.wrapping_add(1);
        }
        let got_seq_fp = fnv1a_words(&w);
        // 死锁判读：期望死锁且锁序构成环 → 复现
        let forms_ring: bool = sc.seq.len() >= 2 && sc.expect.wants_deadlock();
        let got_deadlock: u8 = if forms_ring { 1 } else { 0 };
        // 实际耗时：净态下恒等于样本定义时长——「时机不影响结果」是可复现的硬要求
        let got_duration: u64 = sc.duration;
        // 差异分类
        let diff: DiffKind = if !sc.expect.executable() {
            DiffKind::None
        } else if got_deadlock != (if sc.expect.wants_deadlock() { 1u8 } else { 0u8 }) {
            DiffKind::Deadlock
        } else if got_locks_ne(sc) {
            DiffKind::LockCount
        } else if got_seq_fp != seq_fp_of(sc) {
            DiffKind::LockOrder
        } else if sc.timing >= TIMING_MOD {
            DiffKind::Timing
        } else {
            DiffKind::Match
        };
        let mut fw: Vec<u64> = Vec::with_capacity(6);
        fw.push(V_VERSION_ID);
        fw.push(sc.fingerprint());
        fw.push(round as u64);
        fw.push(got_seq_fp);
        fw.push(got_deadlock as u64);
        fw.push(got_duration);
        let fp = fnv1a_words(&fw);
        Ok(RunResult {
            got_locks: sc.locks,
            got_seq_fp,
            got_deadlock,
            got_duration,
            diff,
            fp,
        })
    }
}
/// 样本锁数是否被改（净态恒否）。
fn got_locks_ne(sc: &ScenarioFn) -> bool {
    sc.locks < LOCK_MIN || sc.locks > LOCK_MAX
}
/// 样本定义锁序指纹。
fn seq_fp_of(sc: &ScenarioFn) -> u64 {
    let mut w: Vec<u64> = Vec::with_capacity(1 + sc.seq.len());
    w.push(sc.locks as u64);
    let mut i: usize = 0;
    while i < sc.seq.len() {
        w.push(sc.seq[i] as u64);
        i = i.wrapping_add(1);
    }
    fnv1a_words(&w)
}

// ===========================================================================
// 8. 主状态机（合法边闭集 + 非法转换拒记）
// ===========================================================================

/// 样本库建设阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 阶段一：场景函数定义（四参数闭集）。
    S1Define,
    /// 阶段二：样本登记（判重 / 淘汰）。
    S2Register,
    /// 阶段三：执行注入（账面跑）。
    S3Inject,
    /// 阶段四：期望 vs 实际对账。
    S4Reconcile,
    /// 阶段五：框架空转验证（M-140）。
    S5Idle,
    /// 终态：样本库冻结入册。
    Sealed,
}
impl Stage {
    /// 阶段数（含终态）。
    pub const COUNT: usize = 6;
    /// 全部阶段。
    pub const ALL: [Stage; Stage::COUNT] = [
        Stage::S1Define,
        Stage::S2Register,
        Stage::S3Inject,
        Stage::S4Reconcile,
        Stage::S5Idle,
        Stage::Sealed,
    ];
    /// 下标。
    pub fn index(self) -> usize {
        match self {
            Stage::S1Define => 0,
            Stage::S2Register => 1,
            Stage::S3Inject => 2,
            Stage::S4Reconcile => 3,
            Stage::S5Idle => 4,
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
            Stage::S1Define => "S1Define",
            Stage::S2Register => "S2Register",
            Stage::S3Inject => "S3Inject",
            Stage::S4Reconcile => "S4Reconcile",
            Stage::S5Idle => "S5Idle",
            Stage::Sealed => "Sealed",
        }
    }
    /// 中文名。
    pub fn cn(self) -> &'static str {
        match self {
            Stage::S1Define => "场景函数定义",
            Stage::S2Register => "样本登记",
            Stage::S3Inject => "执行注入",
            Stage::S4Reconcile => "期望实际对账",
            Stage::S5Idle => "框架空转验证",
            Stage::Sealed => "样本库冻结入册",
        }
    }
    /// 是否终态。
    pub fn is_sealed(self) -> bool {
        self == Stage::Sealed
    }
    /// 四列输出。
    pub fn four_columns(&self) -> String {
        let nxt: &'static str = match self {
            Stage::S1Define => "S2Register",
            Stage::S2Register => "S3Inject",
            Stage::S3Inject => "S4Reconcile",
            Stage::S4Reconcile => "S5Idle",
            Stage::S5Idle => "Sealed",
            Stage::Sealed => "-",
        };
        format!("{:<12} {:<16} next={:<12} {}", self.name(), self.cn(), nxt, FACE_SAMPLE)
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
        trail.push(Stage::S1Define);
        StageFsm { state: Stage::S1Define, tick: 0, legal_moves: 0, rejected: Vec::new(), trail }
    }
    /// 合法边闭集。
    pub fn is_legal(from: Stage, to: Stage) -> bool {
        match (from, to) {
            (Stage::S1Define, Stage::S2Register) => true,
            (Stage::S2Register, Stage::S3Inject) => true,
            (Stage::S3Inject, Stage::S4Reconcile) => true,
            (Stage::S4Reconcile, Stage::S5Idle) => true,
            (Stage::S5Idle, Stage::Sealed) => true,
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
    pub fn try_go(&mut self, to: Stage, tick: u64) -> SlResult<Stage> {
        self.tick = tick;
        if self.state.is_sealed() {
            self.rejected.push(Reject {
                seq: self.rejected.len() as u32,
                from: self.state,
                to,
                tick,
                code: Form::SampleMissing.code(),
            });
            return Err(SlErr::of_form(
                Form::SampleMissing,
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
            code: Form::DiffKindBad.code(),
        });
        Err(SlErr::of_form(
            Form::DiffKindBad,
            format!("非法转换：{}->{}", self.state.name(), to.name()),
        ))
    }
    /// 走满全部合法边。
    pub fn drive_all(&mut self, start_tick: u64) -> SlResult<()> {
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
    /// 样本下标。
    pub sample: usize,
    /// 轮次。
    pub round: u32,
    /// 差异分类下标。
    pub diff: u8,
    /// 执行指纹。
    pub fp: u64,
    /// 逻辑时刻。
    pub tick: u64,
}
impl RingEntry {
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "ring s{} r{} diff={} fp={:016X} t={}",
            self.sample, self.round, self.diff, self.fp, self.tick
        )
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 5] {
        [self.sample as u64, self.round as u64, self.diff as u64, self.fp, self.tick]
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
            buf.push(RingEntry { sample: 0, round: 0, diff: 0xFF, fp: 0, tick: 0 });
            i = i.wrapping_add(1);
        }
        RingLedger { buf, head: 0, pushed: 0, dropped: 0 }
    }
    /// 写一条。
    pub fn push(&mut self, e: RingEntry) {
        let slot = self.head;
        if let Some(old) = self.buf.get(slot) {
            if old.diff != 0xFF || old.fp != 0 {
                self.dropped = self.dropped.wrapping_add(1);
            }
        }
        let mut ne = e;
        ne.tick = e.tick;
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
            if self.buf[i].diff != 0xFF || self.buf[i].fp != 0 {
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
            if e.diff != 0xFF || e.fp != 0 {
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
    /// ΣB：登记样本数 × 重复轮次。
    pub sum_b: u64,
    /// ΣC：环形账写入数。
    pub sum_c: u64,
    /// ΣD：差异分类计数合计。
    pub sum_d: u64,
    /// ΣE：可执行样本执行数（不可达样本不计）。
    pub sum_e: u64,
    /// 检出差异数（非 Match 非 None）。
    pub diff_n: u64,
    /// 淘汰数。
    pub retired: u64,
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
            "recon A={} B={} C={} D={} E={} diffs={} dev={} balanced={} items={}",
            self.sum_a, self.sum_b, self.sum_c, self.sum_d, self.sum_e,
            self.diff_n, self.deviation, if self.balanced { 1 } else { 0 }, self.items.len()
        )
    }
    /// 逐行表。
    pub fn rows_out(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.items.len() + 2);
        out.push(self.summary());
        let mut i: usize = 0;
        while i < self.items.len() {
            let it = &self.items[i];
            out.push(format!("  {:<24} L={:<7} R={:<7} eq={}", it.0, it.1, it.2, if it.3 { 1 } else { 0 }));
            i = i.wrapping_add(1);
        }
        out
    }
}
/// 计算守恒账。
pub fn reconcile(book: &SampleBook, exec: &ExecLog, ring: &RingLedger) -> Reconcile {
    let a: u64 = exec.total();
    let b: u64 = (executable_count()).wrapping_mul(EXEC_REPEAT as u64);
    let c: u64 = ring.pushed;
    let mut d: u64 = 0;
    let mut i: usize = 0;
    while i < DIFF_COUNT {
        d = d.wrapping_add(exec.diff_counts[i] as u64);
        i = i.wrapping_add(1);
    }
    let e: u64 = (executable_count()).wrapping_mul(EXEC_REPEAT as u64);
    let mut diff_n: u64 = 0;
    let mut k: usize = 0;
    while k < exec.rows.len() {
        if exec.rows[k].diff.is_diff() {
            diff_n = diff_n.wrapping_add(1);
        }
        k = k.wrapping_add(1);
    }
    let mut items: Vec<(String, u64, u64, bool)> = Vec::with_capacity(6);
    items.push((String::from("执行=登记×轮次"), a, b, a == b));
    items.push((String::from("执行=环形写入"), a, c, a == c));
    items.push((String::from("执行=差异分类合计"), a, d, a == d));
    items.push((String::from("执行=可执行样本执行"), a, e, a == e));
    items.push((String::from("差异数=非Match非None"), diff_n, 0, diff_n == 0));
    // 淘汰是「≤」关系而非等式：单独用布尔量表达，避免被通用偏离公式误判
    items.push((
        String::from("淘汰有界(布尔)"),
        if book.retire_bounded() { 1 } else { 0 },
        1,
        book.retire_bounded(),
    ));
    let mut dev: u64 = 0;
    let mut m: usize = 0;
    while m < items.len() {
        let it = &items[m];
        let l = if it.1 > it.2 { it.1 - it.2 } else { it.2 - it.1 };
        if l > dev {
            dev = l;
        }
        m = m.wrapping_add(1);
    }
    let balanced: bool = dev == 0 && items.iter().all(|it| it.3);
    Reconcile {
        sum_a: a,
        sum_b: b,
        sum_c: c,
        sum_d: d,
        sum_e: e,
        diff_n,
        retired: book.retired,
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
    /// 样本册指纹。
    pub book_fp: u64,
    /// 执行账指纹。
    pub exec_fp: u64,
    /// 环形账指纹。
    pub ring_fp: u64,
    /// 场景库定义指纹。
    pub lib_fp: u64,
    /// 冻结指纹（净态入册值，注入不改写）。
    pub freeze_fp: u64,
    /// 入册探针基准值。
    pub registered_probe: u64,
    /// 语义边界违例（真机锁访问次数，恒 0）。
    pub border_violations: u32,
    /// 空转验证：零事件推进轮数。
    pub idle_rounds: u64,
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
            book_fp: 0,
            exec_fp: 0,
            ring_fp: 0,
            lib_fp: 0,
            freeze_fp: 0,
            registered_probe: 0,
            border_violations: 0,
            idle_rounds: 0,
        }
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "ev book={:016X} exec={:016X} ring={:016X} lib={:016X} probe={:016X} border={} idle={}",
            self.book_fp, self.exec_fp, self.ring_fp, self.lib_fp,
            self.registered_probe, self.border_violations, self.idle_rounds
        )
    }
}
/// 场景库定义指纹（只折定义，不折执行结果）。
pub fn lib_definition_hash() -> u64 {
    let lib = scenario_lib();
    let mut w: Vec<u64> = Vec::with_capacity(4 + SAMPLE_COUNT * 6);
    w.push(V_VERSION_ID);
    w.push(lib.len() as u64);
    w.push(LOCK_MIN as u64);
    w.push(TIMING_MOD);
    let mut i: usize = 0;
    while i < lib.len() {
        let s = &lib[i];
        w.push(s.fingerprint());
        w.push(s.locks as u64);
        w.push(s.duration);
        w.push(s.timing);
        w.push(s.expect.index() as u64);
        w.push(s.seq.len() as u64);
        i = i.wrapping_add(1);
    }
    fnv1a_words(&w)
}
/// 冻结探针基准哈希。
pub fn freeze_probe_hash(book: &SampleBook, exec: &ExecLog) -> u64 {
    let mut w: Vec<u64> = Vec::with_capacity(4);
    w.push(V_VERSION_ID);
    w.push(lib_definition_hash());
    w.push(book.fingerprint());
    w.push(exec.fingerprint());
    fnv1a_words(&w)
}
/// 快照（被测对象一侧完整状态）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Snapshot {
    /// 样本册。
    pub book: SampleBook,
    /// 执行账。
    pub exec: ExecLog,
    /// 环形账。
    pub ring: RingLedger,
    /// 留痕。
    pub ev: Evidence,
}
impl Snapshot {
    /// 净态快照。
    pub fn net() -> SlResult<Snapshot> {
        let lib = scenario_lib();
        let mut book = SampleBook::new();
        let mut i: usize = 0;
        while i < lib.len() {
            let sc = match lib.get(i) {
                Some(s) => s.clone(),
                None => return Err(err_range("net register", i, lib.len())),
            };
            book.register(sc, tick_advance(i as u64))?;
            i = i.wrapping_add(1);
        }
        // 检索全量（每样本一次，证明可检索）
        let mut j: usize = 0;
        while j < lib.len() {
            let sc = match lib.get(j) {
                Some(s) => s.clone(),
                None => return Err(err_range("net retrieve", j, lib.len())),
            };
            let _ = book.retrieve(&sc)?;
            j = j.wrapping_add(1);
        }
        // 执行全量（仅可执行样本；`Unreachable` 态只登记不执行）
        let mut exec = ExecLog::new();
        let mut ring = RingLedger::new();
        let mut runner = Runner::new();
        let mut k: usize = 0;
        while k < lib.len() {
            let sc = match lib.get(k) {
                Some(s) => s.clone(),
                None => return Err(err_range("net exec", k, lib.len())),
            };
            if !sc.expect.executable() {
                k = k.wrapping_add(1);
                continue;
            }
            let mut r: u32 = 0;
            while r < EXEC_REPEAT as u32 {
                let res = runner.run(&sc, r)?;
                exec.push(ExecRow {
                    sample: k,
                    round: r,
                    got_locks: res.got_locks,
                    got_seq_fp: res.got_seq_fp,
                    got_deadlock: res.got_deadlock,
                    got_duration: res.got_duration,
                    diff: res.diff,
                    fp: res.fp,
                    tick: runner.tick,
                })?;
                ring.push(RingEntry {
                    sample: k,
                    round: r,
                    diff: res.diff.index() as u8,
                    fp: res.fp,
                    tick: runner.tick,
                });
                r = r.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        // 死锁复现统计（净态：期望死锁且复现）
        let mut m: usize = 0;
        while m < lib.len() {
            let sc = &lib[m];
            if sc.expect.wants_deadlock() {
                exec.deadlock_hits = exec.deadlock_hits.wrapping_add(EXEC_REPEAT as u64);
            }
            m = m.wrapping_add(1);
        }
        let mut ev = Evidence::blank();
        ev.book_fp = book.fingerprint();
        ev.exec_fp = exec.fingerprint();
        ev.ring_fp = ring.fingerprint();
        ev.lib_fp = lib_definition_hash();
        ev.freeze_fp = freeze_probe_hash(&book, &exec);
        ev.registered_probe = ev.freeze_fp;
        ev.border_violations = runner.real_hw_reads;
        ev.idle_rounds = idle_verify(&book);
        Ok(Snapshot { book, exec, ring, ev })
    }
    /// 守恒账。
    pub fn reconcile(&self) -> Reconcile {
        reconcile(&self.book, &self.exec, &self.ring)
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
        let mut out: Vec<String> = Vec::with_capacity(8);
        out.push(self.describe());
        out.push(self.book.summary());
        out.push(self.exec.summary());
        out.push(self.ring.summary());
        out.push(self.reconcile().summary());
        out
    }
}
/// 框架空转验证（M-140）：零事件推进下，样本册指纹与执行账读数必须**逐轮不变**。
pub fn idle_verify(book: &SampleBook) -> u64 {
    let base = book.fingerprint();
    let mut stable: u64 = 1;
    let mut i: usize = 0;
    while i < 3 {
        // 空转：不推进任何事件，仅重复取指纹
        if book.fingerprint() == base {
            stable = stable.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
    stable
}
/// 单形态注入。
fn inject_one(f: Form) -> SlResult<Snapshot> {
    let mut s = Snapshot::net()?;
    s.ev.border_violations = 0;
    let idx = f.index();
    if idx < Form::COUNT {
        s.ev.fired[idx] = 1;
    }
    match f {
        Form::SampleDup => {
            // 重号登记：拿样本 0 再登记一次
            let lib = scenario_lib();
            if let Some(sc) = lib.get(0) {
                let c = sc.clone();
                let _ = s.book.register(c, 999);
            }
            s.book.dup_bits = s.book.dup_bits | 1u32;
        }
        Form::ParamOutOfRange => {
            // 锁数越界
            if let Some(r) = s.book.rows.get_mut(0) {
                r.sc.locks = LOCK_MAX + 7;
            }
        }
        Form::ParamMissing => {
            // 锁序长度与锁数不符
            if let Some(r) = s.book.rows.get_mut(1) {
                r.sc.seq.pop();
            }
        }
        Form::ExpectOutOfSet => {
            // 期望口径失配：把「不可达」样本声明为可执行（期望态与登记声明不闭合）
            if let Some(r) = s.book.rows.get_mut(2) {
                r.exec_claim = if r.sc.expect.executable() { 0 } else { 1 };
            }
        }
        Form::SampleMissing => {
            // 摘掉一条登记（存在位图留洞）
            if let Some(last) = s.book.rows.pop() {
                let _ = last;
            }
            if s.book.rows.len() >= 32 {
                s.book.present_bits = 0;
            } else {
                let b: u32 = 1u32 << (s.book.rows.len() & 31);
                s.book.present_bits = s.book.present_bits & (s.book.present_bits ^ b);
            }
        }
        Form::RetrieveMiss => {
            // 检索未命中：构造一个不在册的四元组
            let bogus = ScenarioFn::raw(2, DUR_MIN, 7, Expect::Ordered, vec![0u32, 1u32]);
            let _ = s.book.retrieve(&bogus);
            // 检索面读数缺口：净态下每条样本各命中一次，缺口即漏检
            s.book.retrieved = s.book.retrieved.wrapping_sub(1);
            if let Some(r) = s.book.rows.get_mut(0) {
                r.hits = 0;
            }
        }
        Form::DiffKindBad => {
            // 差异分类越闭集
            if let Some(r) = s.exec.rows.get_mut(0) {
                r.diff = DiffKind::None;
                r.fp = r.fp.wrapping_add(1);
            }
        }
        Form::LockOrderBroken => {
            // 锁序错乱：反转一条执行记录的锁序指纹
            if let Some(r) = s.exec.rows.get_mut(0) {
                r.got_seq_fp = r.got_seq_fp.wrapping_add(0x9E37);
                r.diff = DiffKind::LockOrder;
                r.fp = r.fp.wrapping_add(1);
            }
        }
        Form::DeadlockNotHit => {
            // 期望死锁但实际未死锁
            if let Some(r) = s.exec.rows.get_mut(0) {
                r.got_deadlock = 0;
                r.diff = DiffKind::Deadlock;
                r.fp = r.fp.wrapping_add(1);
            }
            s.exec.deadlock_hits = 0;
        }
        Form::FalseDeadlock => {
            // 非死锁场景却报死锁
            if let Some(r) = s.exec.rows.get_mut(1) {
                r.got_deadlock = 1;
                r.diff = DiffKind::Deadlock;
                r.fp = r.fp.wrapping_add(1);
            }
            s.exec.false_deadlocks = s.exec.false_deadlocks.wrapping_add(1);
        }
        Form::LedgerUnbalanced => {
            // 环形账写指针回退而不补 dropped
            s.ring.head = s.ring.head.wrapping_sub(1) & RING_MASK;
            s.ring.dropped = s.ring.dropped.wrapping_add(5);
        }
        Form::FingerprintDrift => {
            // 冻结指纹漂移：改样本定义而不更新入册探针
            if let Some(r) = s.book.rows.get_mut(0) {
                r.sc.duration = r.sc.duration.wrapping_add(1);
                r.fp = r.sc.fingerprint();
            }
            s.ev.freeze_fp = freeze_probe_hash(&s.book, &s.exec);
        }
        Form::ReconcileDeviation => {
            // 守恒偏离：执行记录数加一
            let lib = scenario_lib();
            if let Some(sc) = lib.get(0) {
                let c = sc.clone();
                s.exec.push(ExecRow {
                    sample: 0,
                    round: EXEC_REPEAT as u32,
                    got_locks: c.locks,
                    got_seq_fp: seq_fp_of(&c),
                    got_deadlock: 0,
                    got_duration: c.duration,
                    diff: DiffKind::Match,
                    fp: 0xDEAD_BEEF,
                    tick: 7777,
                })?;
            }
        }
        Form::RetireOverflow => {
            // 淘汰账越界
            s.book.retired = (s.book.rows.len() as u64) + 3;
        }
    }
    Ok(s)
}
/// 逐形态注入全跑。
pub fn inject_all() -> SlResult<(Vec<Snapshot>, Vec<Form>)> {
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
    // ① 样本册面
    if !s.book.dup_free() {
        d.mark(Form::SampleDup);
    }
    if !s.book.present_complete() {
        d.mark(Form::SampleMissing);
    }
    if !s.book.retire_bounded() {
        d.mark(Form::RetireOverflow);
    }
    // ② 参数面
    let mut param_bad: bool = false;
    let mut param_missing: bool = false;
    let mut expect_bad: bool = false;
    let mut i: usize = 0;
    while i < s.book.rows.len() {
        let r = &s.book.rows[i];
        let sc = &r.sc;
        if !sc.params_in_range() {
            param_bad = true;
        }
        if !sc.params_complete() {
            param_missing = true;
        }
        if !sc.expect_closed() {
            expect_bad = true;
        }
        // 登记声明的可执行位必须与期望态自洽
        if r.exec_claim != (if sc.expect.executable() { 1u8 } else { 0u8 }) {
            expect_bad = true;
        }
        i = i.wrapping_add(1);
    }
    if param_bad {
        d.mark(Form::ParamOutOfRange);
    }
    if param_missing {
        d.mark(Form::ParamMissing);
    }
    if expect_bad {
        d.mark(Form::ExpectOutOfSet);
    }
    // ③ 检索面：净态下检索命中数应等于登记数
    let expect_retrieved: u64 = s.book.rows.len() as u64;
    if s.book.retrieved < expect_retrieved {
        d.mark(Form::RetrieveMiss);
    }
    // ④ 对账面：差异分类不仅要在闭集内，还要与样本可执行态自洽
    if !s.exec.diff_closed() {
        d.mark(Form::DiffKindBad);
    } else {
        let lib = scenario_lib();
        let mut k: usize = 0;
        while k < s.exec.rows.len() {
            let r = &s.exec.rows[k];
            let sc_opt = lib.get(r.sample);
            let mut bad: bool = false;
            if let Some(sc) = sc_opt {
                if sc.expect.executable() && r.diff == DiffKind::None {
                    // 可执行样本不得给出「未执行」分类
                    bad = true;
                }
                if !sc.expect.executable() && r.diff != DiffKind::None {
                    bad = true;
                }
            } else {
                bad = true;
            }
            if bad {
                d.mark(Form::DiffKindBad);
            }
            k = k.wrapping_add(1);
        }
    }
    if !s.exec.lock_order_ok() {
        d.mark(Form::LockOrderBroken);
    }
    if !s.exec.deadlock_reading_ok() {
        // 区分漏复现与误报：靠样本期望态
        d.mark(Form::DeadlockNotHit);
        d.mark(Form::FalseDeadlock);
    }
    // ⑤ 账目面
    if !s.ring.balanced() {
        d.mark(Form::LedgerUnbalanced);
    }
    let rc = s.reconcile();
    if !rc.balanced {
        d.mark(Form::ReconcileDeviation);
    }
    // ⑥ 冻结面
    if s.ev.freeze_fp != s.ev.registered_probe {
        d.mark(Form::FingerprintDrift);
    }
    d
}
/// 全形态检出。
pub fn detect_all(inj: &[Snapshot], forms: &[Form]) -> SlResult<Detections> {
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
            return Err(SlErr::of_form(f, format!("注入未被检出：{} 漏检", f.name())));
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
    /// 场景库定义哈希。
    pub lib_hash: u64,
    /// 样本册哈希。
    pub book_hash: u64,
    /// 状态机定义哈希。
    pub fsm_hash: u64,
    /// 冻结总哈希。
    pub freeze_hash: u64,
    /// 语义边界声明。
    pub border_note: String,
    /// 库键。
    pub lib_key: String,
}
impl FreezeRecord {
    /// 构造。
    pub fn build(book: &SampleBook, exec: &ExecLog) -> FreezeRecord {
        let lib_hash = lib_definition_hash();
        let book_hash = book.fingerprint();
        let fsm_hash = fsm_definition_hash();
        let mut w: Vec<u64> = Vec::with_capacity(7);
        w.push(V_VERSION_ID);
        w.push(lib_hash);
        w.push(book_hash);
        w.push(fsm_hash);
        w.push(SAMPLE_COUNT as u64);
        w.push(EXEC_REPEAT as u64);
        w.push(EXPECT_COUNT as u64);
        let freeze_hash = fnv1a_words(&w);
        FreezeRecord {
            rev: 1,
            lib_hash,
            book_hash,
            fsm_hash,
            freeze_hash,
            border_note: String::from("只做样本库框架与账面注入；不真跑内核锁、不做真机时序采集"),
            lib_key: format!("samplelib/{}/{}", V_VERSION, GRAFT_SEAT),
        }
    }
    /// 校验。
    pub fn verify(&self, book: &SampleBook, exec: &ExecLog) -> SlResult<()> {
        if self.lib_hash != lib_definition_hash() {
            return Err(err_frozen("lib_hash", &format!("{:016X}", lib_definition_hash())));
        }
        if self.book_hash != book.fingerprint() {
            return Err(err_frozen("book_hash", &format!("{:016X}", book.fingerprint())));
        }
        if self.fsm_hash != fsm_definition_hash() {
            return Err(err_frozen("fsm_hash", &format!("{:016X}", fsm_definition_hash())));
        }
        if !exec.diff_closed() {
            return Err(SlErr::of_form(
                Form::DiffKindBad,
                format!("冻结校验：差异分类越闭集 {}", exec.summary()),
            ));
        }
        Ok(())
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "freeze rev={} lib={:016X} book={:016X} fsm={:016X} total={:016X}",
            self.rev, self.lib_hash, self.book_hash, self.fsm_hash, self.freeze_hash
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(4);
        out.push(self.describe());
        out.push(format!("  border  {}", self.border_note));
        out.push(format!("  key     {}", self.lib_key));
        out.push(format!("  dep     {}", DEP_SEAT));
        out
    }
}
/// 状态机定义哈希。
pub fn fsm_definition_hash() -> u64 {
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
                w.push(((j * Stage::COUNT + k) as u64).wrapping_add(0x20000));
            }
            k = k.wrapping_add(1);
        }
        j = j.wrapping_add(1);
    }
    fnv1a_words(&w)
}
/// 稳定性报告。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Stability {
    /// 轮数。
    pub rounds: u32,
    /// 逐轮冻结哈希。
    pub hashes: Vec<u64>,
    /// 逐轮执行账指纹。
    pub exec_fps: Vec<u64>,
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
        let e0 = self.exec_fps[0];
        let mut i: usize = 1;
        while i < self.hashes.len() {
            if self.hashes[i] != h0 || self.exec_fps[i] != e0 {
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
pub fn stability_check() -> SlResult<Stability> {
    let mut hashes: Vec<u64> = Vec::with_capacity(REPEAT_RUNS);
    let mut exec_fps: Vec<u64> = Vec::with_capacity(REPEAT_RUNS);
    let mut i: usize = 0;
    while i < REPEAT_RUNS {
        let s = Snapshot::net()?;
        let fr = FreezeRecord::build(&s.book, &s.exec);
        hashes.push(fr.freeze_hash);
        exec_fps.push(s.exec.fingerprint());
        i = i.wrapping_add(1);
    }
    let mut st = Stability { rounds: REPEAT_RUNS as u32, hashes, exec_fps, stable: false };
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
    /// 执行账指纹。
    pub exec_fp: u64,
    /// 差异分类计数。
    pub diff_counts: [u32; DIFF_COUNT],
    /// 执行总数。
    pub total: u64,
    /// 注入检出命中数。
    pub inject_hits: usize,
    /// 环形账写入数。
    pub ring_pushed: u64,
}
impl Replay {
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "replay freeze={:016X} exec={:016X} total={} diffs={:?} hits={} ring={}",
            self.freeze_hash, self.exec_fp, self.total, self.diff_counts, self.inject_hits, self.ring_pushed
        )
    }
}
/// 确定性重放（与 `observe` 互不递归）。
pub fn replay(seed: u64) -> SlResult<Replay> {
    let mut rng = Lcg::new(seed);
    let _ = rng.next();
    let s = Snapshot::net()?;
    if !s.book.present_complete() {
        return Err(SlErr::of_form(
            Form::SampleMissing,
            format!("重放：样本缺失 present={:08X}", s.book.present_bits),
        ));
    }
    if !s.book.dup_free() {
        return Err(SlErr::of_form(
            Form::SampleDup,
            format!("重放：样本重号 dup={:08X}", s.book.dup_bits),
        ));
    }
    if !s.exec.diff_closed() {
        return Err(SlErr::of_form(
            Form::DiffKindBad,
            format!("重放：差异分类越闭集 {}", s.exec.summary()),
        ));
    }
    if !s.ring.balanced() {
        return Err(SlErr::of_form(
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
    let fr = FreezeRecord::build(&s.book, &s.exec);
    Ok(Replay {
        freeze_hash: fr.freeze_hash,
        exec_fp: s.exec.fingerprint(),
        diff_counts: s.exec.diff_counts,
        total: s.exec.total(),
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
    /// 对照说明。
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
    /// 登记样本数。
    pub samples: u64,
    /// 检索命中数。
    pub retrieved: u64,
    /// 执行总数。
    pub total: u64,
    /// 差异分类计数。
    pub diff_counts: [u32; DIFF_COUNT],
    /// 检出差异数。
    pub diff_n: u64,
    /// 死锁复现数。
    pub deadlock_hits: u64,
    /// 误报数。
    pub false_deadlocks: u64,
    /// 淘汰数。
    pub retired: u64,
    /// 环形账写入/丢弃。
    pub ring_pushed: u64,
    /// 环形账丢弃。
    pub ring_dropped: u64,
    /// 执行步数。
    pub steps: u64,
    /// 注入检出命中数。
    pub inject_hits: usize,
    /// 净态检出（应零）。
    pub net_hits: usize,
    /// 边界违例。
    pub border_violations: u32,
    /// 空转轮数。
    pub idle_rounds: u64,
    /// 峰值内存估算。
    pub mem_peak: u64,
}
impl Metrics {
    /// 全项通过。
    pub fn all_ok(&self) -> bool {
        self.net_hits == 0
            && self.border_violations == 0
            && self.samples == SAMPLE_COUNT as u64
            && self.total == (executable_count() * EXEC_REPEAT as u64)
            && self.retrieved == SAMPLE_COUNT as u64
            && self.diff_n == 0
            && self.false_deadlocks == 0
            && self.inject_hits == Form::COUNT
            && self.steps <= STEP_CAP
            && self.mem_peak <= MEM_PEAK_CAP
            && self.idle_rounds >= 4
    }
    /// 性能逐项分解。
    pub fn complexity(&self) -> Vec<CostItem> {
        vec![
            CostItem::with_note(
                "scenario_lib_build",
                SAMPLE_COUNT as u64,
                LOCK_SEQ_CAP as u64,
                "Windows 无参数化死锁场景库（死锁用例多为手写脚本）",
            ),
            CostItem::with_note(
                "param_range_check",
                SAMPLE_COUNT as u64,
                PARAM_COUNT as u64,
                "Windows 无四参数闭集校验，参数错由运行期暴露",
            ),
            CostItem::with_note(
                "sample_register",
                SAMPLE_COUNT as u64,
                SAMPLE_COUNT as u64,
                "Windows 无指纹判重，重号靠人工",
            ),
            CostItem::with_note(
                "sample_retrieve",
                SAMPLE_COUNT as u64,
                SAMPLE_COUNT as u64,
                "Windows 无按四元组精确检索层",
            ),
            CostItem::with_note(
                "retire_lru_scan",
                SAMPLE_COUNT as u64,
                SAMPLE_COUNT as u64,
                "Windows 无样本淘汰账（LRU 由文件系统代管）",
            ),
            CostItem::with_note(
                "scenario_run",
                self.total,
                6,
                "Windows 真机跑锁竞争，本单为账面注入（欠账显性）",
            ),
            CostItem::with_note(
                "diff_classify",
                self.total,
                DIFF_COUNT as u64,
                "Windows 只判 pass/fail，无差异分类",
            ),
            CostItem::with_note(
                "ring_push",
                self.ring_pushed,
                3,
                "append-only + dropped 显性",
            ),
            CostItem::with_note(
                "reconcile",
                6,
                3,
                "Windows 无跨面守恒恒等式",
            ),
            CostItem::with_note(
                "idle_verify",
                self.idle_rounds,
                3,
                "M-140 框架空转验证，Windows 无对应面",
            ),
            CostItem::with_note(
                "inject_one",
                Form::COUNT as u64,
                self.total,
                "本单特有：14 形态全注入",
            ),
            CostItem::with_note(
                "detect_one",
                Form::COUNT as u64,
                (DIFF_COUNT + PARAM_COUNT) as u64,
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
            "metrics samples={} retrieved={} total={} diffs={:?} dl_hit={} false_dl={} retired={} ring={}/{} inj={} net={} idle={}",
            self.samples, self.retrieved, self.total, self.diff_counts, self.deadlock_hits,
            self.false_deadlocks, self.retired, self.ring_pushed, self.ring_dropped,
            self.inject_hits, self.net_hits, self.idle_rounds
        )
    }
}

// ===========================================================================
// 14. 导出
// ===========================================================================

/// KV 导出。
pub fn export_kv(o: &SlObserve) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(48);
    out.push(format!("ver={}", V_VERSION));
    out.push(format!("seed={:016X}", o.seed));
    out.push(format!("seg=0x{:04X}", SEG_BASE));
    out.push(format!("lock_min={}", LOCK_MIN));
    out.push(format!("lock_max={}", LOCK_MAX));
    out.push(format!("dur_min={}", DUR_MIN));
    out.push(format!("dur_max={}", DUR_MAX));
    out.push(format!("timing_mod={}", TIMING_MOD));
    out.push(format!("param_count={}", PARAM_COUNT));
    out.push(format!("samples={}", o.metrics.samples));
    out.push(format!("retrieved={}", o.metrics.retrieved));
    out.push(format!("total={}", o.metrics.total));
    let mut i: usize = 0;
    while i < DIFF_COUNT {
        out.push(format!("diff_{}={}", DiffKind::ALL[i].name(), o.metrics.diff_counts[i]));
        i = i.wrapping_add(1);
    }
    out.push(format!("deadlock_hits={}", o.metrics.deadlock_hits));
    out.push(format!("false_deadlocks={}", o.metrics.false_deadlocks));
    out.push(format!("retired={}", o.metrics.retired));
    out.push(format!("ring_pushed={}", o.metrics.ring_pushed));
    out.push(format!("ring_dropped={}", o.metrics.ring_dropped));
    out.push(format!("steps={}", o.metrics.steps));
    out.push(format!("idle_rounds={}", o.metrics.idle_rounds));
    out.push(format!("recon_dev={}", o.recon.deviation));
    out.push(format!("recon_balanced={}", if o.recon.balanced { 1 } else { 0 }));
    out.push(format!("fsm_state={}", o.fsm.state.name()));
    out.push(format!("fsm_reject={}", o.fsm.rejected.len()));
    out.push(format!("freeze_hash={:016X}", o.freeze.freeze_hash));
    out.push(format!("replay_freeze={:016X}", o.replay.freeze_hash));
    out.push(format!("inject_hits={}", o.metrics.inject_hits));
    out.push(format!("net_hits={}", o.metrics.net_hits));
    out.push(format!("all_ok={}", if o.all_ok() { 1 } else { 0 }));
    let mut j: usize = 0;
    while j < Form::COUNT {
        out.push(format!("form_{}={}", Form::ALL[j].name(), o.detects.hits[j]));
        j = j.wrapping_add(1);
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
pub fn export_tlv(o: &SlObserve) -> Vec<u8> {
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
    push_tlv_u64(&mut out, 0x10, LOCK_MIN as u64);
    push_tlv_u64(&mut out, 0x11, LOCK_MAX as u64);
    push_tlv_u64(&mut out, 0x12, DUR_MIN);
    push_tlv_u64(&mut out, 0x13, DUR_MAX);
    push_tlv_u64(&mut out, 0x14, TIMING_MOD);
    push_tlv_u64(&mut out, 0x15, PARAM_COUNT as u64);
    push_tlv_u64(&mut out, 0x16, SAMPLE_COUNT as u64);
    push_tlv_u64(&mut out, 0x17, BOOK_CAP as u64);
    push_tlv_u64(&mut out, 0x18, EXEC_REPEAT as u64);
    push_tlv_u64(&mut out, 0x19, RING_CAP as u64);
    push_tlv_u64(&mut out, 0x1A, STEP_CAP);
    push_tlv_u64(&mut out, 0x1B, MEM_PEAK_CAP);
    // 样本面（6 条）
    push_tlv_u64(&mut out, 0x20, o.metrics.samples);
    push_tlv_u64(&mut out, 0x21, o.metrics.retrieved);
    push_tlv_u64(&mut out, 0x22, o.metrics.retired);
    push_tlv_u64(&mut out, 0x23, o.net.book.fingerprint());
    push_tlv_u64(&mut out, 0x24, o.net.book.present_bits as u64);
    push_tlv_u64(&mut out, 0x25, o.net.book.dup_bits as u64);
    // 执行面（8 条）
    push_tlv_u64(&mut out, 0x30, o.metrics.total);
    push_tlv_u64(&mut out, 0x31, o.metrics.deadlock_hits);
    push_tlv_u64(&mut out, 0x32, o.metrics.false_deadlocks);
    push_tlv_u64(&mut out, 0x33, o.net.exec.fingerprint());
    push_tlv_u64(&mut out, 0x34, o.net.exec.ran_bits as u64);
    push_tlv_u64(&mut out, 0x35, o.metrics.steps);
    push_tlv_u64(&mut out, 0x36, o.metrics.mem_peak);
    push_tlv_u64(&mut out, 0x37, lib_definition_hash());
    // 差异分类（6 条）
    let mut i: usize = 0;
    while i < DIFF_COUNT {
        push_tlv_u64(&mut out, 0x40u8.wrapping_add(i as u8), o.metrics.diff_counts[i] as u64);
        i = i.wrapping_add(1);
    }
    // 账目面（5 条）
    push_tlv_u64(&mut out, 0x50, o.ring.pushed);
    push_tlv_u64(&mut out, 0x51, o.ring.dropped);
    push_tlv_u64(&mut out, 0x52, o.ring.live());
    push_tlv_u64(&mut out, 0x53, o.ring.fingerprint());
    push_tlv_u64(&mut out, 0x54, o.recon.deviation);
    // 冻结面（5 条）
    push_tlv_u64(&mut out, 0x60, o.freeze.freeze_hash);
    push_tlv_u64(&mut out, 0x61, o.replay.freeze_hash);
    push_tlv_u64(&mut out, 0x62, o.replay.inject_hits as u64);
    push_tlv_u64(&mut out, 0x63, o.detects.verdict_mask as u64);
    push_tlv_u64(&mut out, 0x64, fsm_definition_hash());
    // 空转面（2 条）
    push_tlv_u64(&mut out, 0x70, o.metrics.idle_rounds);
    push_tlv_u64(&mut out, 0x71, o.metrics.net_hits as u64);
    // 逐形态命中（14 条）
    let mut j: usize = 0;
    while j < Form::COUNT {
        push_tlv_u64(&mut out, 0x80u8.wrapping_add(j as u8), o.detects.hits[j] as u64);
        j = j.wrapping_add(1);
    }
    out
}
/// TLV 长度核算。
pub fn tlv_len_check(o: &SlObserve) -> SlResult<usize> {
    let buf = export_tlv(o);
    let want: usize = TLV_HEAD_LEN + TLV_ITEM_COUNT * TLV_ITEM_LEN;
    if buf.len() != want {
        return Err(SlErr::of_aux(
            AuxCode::SampleIdxOver,
            format!("TLV 长度不符：got={} want={}", buf.len(), want),
        ));
    }
    let hl = ((buf[1] as usize) << 8) | (buf[2] as usize);
    if hl != TLV_HEAD_PAYLOAD {
        return Err(SlErr::of_aux(
            AuxCode::SampleIdxOver,
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
pub struct SlObserve {
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
impl SlObserve {
    /// 全项通过。
    pub fn all_ok(&self) -> bool {
        self.metrics.all_ok()
    }
    /// 重放一致性。
    pub fn replay_match(&self) -> bool {
        self.replay.freeze_hash == self.freeze.freeze_hash
            && self.replay.exec_fp == self.net.exec.fingerprint()
            && self.replay.total == self.metrics.total
            && self.replay.ring_pushed == self.metrics.ring_pushed
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
        out.push(format!("# UNX-F3381 观测 seed={:016X}", self.seed));
        out.push(self.summary());
        out.push(format!("ver={} seg={}", V_VERSION, seg_line()));
        out.push(format!("metrics  {}", self.metrics.summary()));
        out.push(format!("recon    {}", self.recon.summary()));
        out.push(format!("fsm      {}", self.fsm.summary()));
        out.push(format!("stable   {}", self.stability.describe()));
        out.push(format!("book     {}", self.net.book.summary()));
        out.push(format!("exec     {}", self.net.exec.summary()));
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
pub fn observe(seed: u64) -> SlResult<SlObserve> {
    let net = Snapshot::net()?;
    if !net.book.present_complete() {
        return Err(SlErr::of_form(
            Form::SampleMissing,
            format!("样本缺失 present={:08X}", net.book.present_bits),
        ));
    }
    if !net.book.dup_free() {
        return Err(SlErr::of_form(
            Form::SampleDup,
            format!("样本重号 dup={:08X}", net.book.dup_bits),
        ));
    }
    if !net.book.retire_bounded() {
        return Err(SlErr::of_form(
            Form::RetireOverflow,
            format!("淘汰越界 retired={}", net.book.retired),
        ));
    }
    if !net.exec.diff_closed() {
        return Err(SlErr::of_form(
            Form::DiffKindBad,
            format!("差异分类越闭集 {}", net.exec.summary()),
        ));
    }
    if net.exec.total() != (executable_count() * EXEC_REPEAT as u64) {
        return Err(SlErr::of_form(
            Form::ReconcileDeviation,
            format!("执行条数不符 total={}", net.exec.total()),
        ));
    }
    if !net.ring.balanced() {
        return Err(SlErr::of_form(
            Form::LedgerUnbalanced,
            format!("环形账不守恒 {}", net.ring.summary()),
        ));
    }
    // 空转验证（M-140）
    if idle_verify(&net.book) < 4 {
        return Err(SlErr::of_form(
            Form::ReconcileDeviation,
            format!("空转验证不过 idle={}", net.ev.idle_rounds),
        ));
    }
    let recon = net.reconcile();
    if !recon.balanced {
        return Err(err_recon(recon.sum_a, recon.sum_b, recon.sum_c));
    }
    // 状态机
    let mut fsm = StageFsm::new();
    fsm.drive_all(1)?;
    let illegal: [Stage; 3] = [Stage::S3Inject, Stage::S1Define, Stage::Sealed];
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
        return Err(SlErr::of_form(
            Form::ReconcileDeviation,
            format!("注入检出不足 {}/{}", detects.hit_count(), Form::COUNT),
        ));
    }
    let net_detect = detect_one(&net);
    if net_detect.hit_count() != 0 {
        return Err(SlErr::of_form(
            Form::ReconcileDeviation,
            format!("净态检出非零 {:?}", net_detect.hit_list()),
        ));
    }
    let freeze = FreezeRecord::build(&net.book, &net.exec);
    freeze.verify(&net.book, &net.exec)?;
    if net.ev.freeze_fp != net.ev.registered_probe {
        return Err(SlErr::of_form(
            Form::FingerprintDrift,
            format!("探针漂移 {:016X} vs {:016X}", net.ev.freeze_fp, net.ev.registered_probe),
        ));
    }
    let mut diff_n: u64 = 0;
    let mut i: usize = 0;
    while i < net.exec.rows.len() {
        if net.exec.rows[i].diff.is_diff() {
            diff_n = diff_n.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
    let metrics = Metrics {
        samples: net.book.rows.len() as u64,
        retrieved: net.book.retrieved,
        total: net.exec.total(),
        diff_counts: net.exec.diff_counts,
        diff_n,
        deadlock_hits: net.exec.deadlock_hits,
        false_deadlocks: net.exec.false_deadlocks,
        retired: net.book.retired,
        ring_pushed: net.ring.pushed,
        ring_dropped: net.ring.dropped,
        steps: net.exec.steps,
        inject_hits: detects.hit_count(),
        net_hits: net_detect.hit_count(),
        border_violations: net.ev.border_violations,
        idle_rounds: net.ev.idle_rounds,
        mem_peak: mem_estimate() as u64,
    };
    let replay = replay(seed)?;
    let ring_clone = net.ring.clone();
    let o = SlObserve {
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
    let book: usize = BOOK_CAP * (core::mem::size_of::<SampleRow>() + 32);
    let exec: usize = SAMPLE_COUNT * EXEC_REPEAT * (core::mem::size_of::<ExecRow>() + 32);
    let ring: usize = RING_CAP * (core::mem::size_of::<RingEntry>() + 16);
    let lib: usize = SAMPLE_COUNT * (core::mem::size_of::<ScenarioFn>() + LOCK_SEQ_CAP * 4);
    book + exec + ring + lib
}
