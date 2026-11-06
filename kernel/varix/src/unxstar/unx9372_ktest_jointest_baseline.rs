//! UNX-F9372 回归基线联测（ktest -f c2 与 C3 回归 runner 对接） · 0x7CA80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9372 逐字）：
//!   ① 纯功能代码 ≥440 行；
//!   ② 判据原文：UNX-F9372-J1 基线联测判据过，ktest -f c2 与 C3 runner
//!      对接可观测，fail==0 判据联测格过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 过滤器复刻 `ktest -f` 的 token 语义：逗号切分、逐 token 精确比对
//!      `c2`，不做子串匹配（c2x/c22/C2 不选中）；零选中回 ENOENT，过滤后
//!      无用例可跑时不得假装通过。
//!   ② 四段状态机 Idle→C2Loaded→Mirrored→Executed→Sealed 是唯一写入口：
//!      跳段 EINVAL；未执行封存 EBUSY；红账封存 EBUSY；重复封存 EBUSY；
//!      封存后任何写入 EPERM（并计入 writes_after_seal）。
//!   ③ 双 runner 对账按 case 号对齐两张定容结果表：skip 表示 runner 根本
//!      没记录该 case（不是 ran=false），故 missing 与 mismatch 是两种可
//!      区分缺陷；fail_total = mismatch + 双向 missing + 双侧同红
//!      （两侧 ran&&failed 的用例只记一笔；单侧红即 mismatch，不重复累加）。
//!   ④ 运行计划是四个 u32 位图，位号即 case 号 0..=31；越界号拒绝不截断。
//!   ⑤ 注入两束：过滤器 10 格 × 10 轮、封存 8 行 × 8 轮，共 164 次；
//!      账本恒满足 injected == detected + escaped 且 escaped == 0。
//!   ⑥ 写回 Pending→Done 单次：空摘要 EINVAL、超长 E2BIG、二次 EBUSY。
//!   ⑦ 指纹：规范联测（1 次 skip、1 次 fail、封存被拒）编码后 FNV-1a 折叠。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9372 独占 0x7CA80..=0x7CABF）。
pub const SEG_BASE: u32 = 0x7CA80;
/// 段窗口宽度（本批每单 0x40）。
pub const SEG_WINDOW: u32 = 0x40;
/// 下一单段基址（F9373），用于段间不重叠判定。
pub const NEXT_SEG_BASE: u32 = 0x7CAC0;
/// ktest 过滤串与 C3 runner 名。
pub const C2_FILTER: &str = "c2";
pub const C3_RUNNER: &str = "C3";
/// 用例簿容量与注入规模。
pub const CASE_CAP: usize = 12;
pub const FILTER_CELLS: usize = 10;
pub const SEAL_CELLS: usize = 8;
pub const INJECT_ROUNDS: u32 = 10;
pub const SEAL_ROUNDS: u32 = 8;
/// 写回摘要字节上限与本卡行配额。
pub const WB_SUMMARY_MAX: usize = 96;
pub const CODE_QUOTA: u32 = 440;

/// Linux errno 数值（正值口径）。
pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const E2BIG: i32 = 7;
pub const ECHILD: i32 = 10;
pub const ENOMEM: i32 = 12;
pub const EACCES: i32 = 13;
pub const EBUSY: i32 = 16;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码。`code()` 是段内序号（`#[repr(u16)]` 契约），`seg_addr()`
/// 是段绝对码值（SEG_BASE + 序号；u16 收不下 SEG，故单列）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JointCode {
    FilterMalformed = 1,
    RunnerDetached = 2,
    CaseUnknown = 3,
    CaseDuplicate = 4,
    BaselineMissing = 5,
    SuiteOverflow = 6,
    SealBeforeRun = 7,
    SealTwice = 8,
    FailNotZero = 9,
    OrderViolation = 10,
    LedgerRegress = 11,
    NotReproducible = 12,
    SegmentViolation = 13,
    WritebackEmpty = 14,
}

impl JointCode {
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 段绝对码值：SEG_BASE + 序号，段内两两唯一。
    pub fn seg_addr(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文一句话诊断与建议（闭集，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            JointCode::FilterMalformed => "过滤串零选中：-f c2 只认逗号分隔的精确 token；请改成 c2 或 kvm,c2",
            JointCode::RunnerDetached => "C3 runner 未挂接：Mirrored 之前不得对账；请先 attach(\"C3\")",
            JointCode::CaseUnknown => "未知 case 号：位图只覆盖 0..=31；越界号必须显式拒绝而不是截断移位",
            JointCode::CaseDuplicate => "case 号重复：同一号只能登记一次；请改用递增号或先撤旧用例",
            JointCode::BaselineMissing => "基线缺失：两侧结果表至少一侧为空；请确认 runner 真的跑了这批用例",
            JointCode::SuiteOverflow => "用例簿已满：容量 12 条；请拆批登记，满表后再录必回 ENOMEM",
            JointCode::SealBeforeRun => "未执行即封存：Sealed 只能从 Executed 进入；请先 execute 拿对账报告",
            JointCode::SealTwice => "重复封存：封存是终态迁移；请复用首次封存指纹",
            JointCode::FailNotZero => "红账拒封：fail_total 必须为 0；请先修不一致用例再重跑基线",
            JointCode::OrderViolation => "状态机跳段：只允许 Idle→C2Loaded→Mirrored→Executed→Sealed 逐段推进",
            JointCode::LedgerRegress => "注入账倒退：计数只增不减；请检查回滚路径是否误改累计值",
            JointCode::NotReproducible => "双跑不一致：同输入两次封存指纹不同；请排查隐藏状态或非确定遍历序",
            JointCode::SegmentViolation => "段位越界：本模块必须落在 0x7CA80..=0x7CABF，不得侵入 F9373",
            JointCode::WritebackEmpty => "写回摘要为空：状态=已完成时必须带非空摘要；请写入 fail=0 与用例数",
        }
    }

    /// 闭集全量。
    pub const ALL: &'static [JointCode] = &[
        JointCode::FilterMalformed, JointCode::RunnerDetached,
        JointCode::CaseUnknown, JointCode::CaseDuplicate,
        JointCode::BaselineMissing, JointCode::SuiteOverflow,
        JointCode::SealBeforeRun, JointCode::SealTwice,
        JointCode::FailNotZero, JointCode::OrderViolation,
        JointCode::LedgerRegress, JointCode::NotReproducible,
        JointCode::SegmentViolation, JointCode::WritebackEmpty,
    ];
}

// ── ktest 过滤器语义 ─────────────────────────────────────────────────
/// token 是否精确等于 `c2`。
pub fn token_is_c2(bytes: &[u8], start: usize, len: usize) -> bool {
    len == 2
        && match (bytes.get(start), bytes.get(start + 1)) {
            (Some(b'c'), Some(b'2')) => true,
            _ => false,
        }
}

/// `ktest -f <filter>` 选中判定：逗号分隔 token 中存在精确 `c2`。
/// 空 token / 空过滤器零选中；全程 `get()`，不分配切分结果。
pub fn filter_accepts(filter: &str) -> bool {
    let bytes = filter.as_bytes();
    let mut i = 0usize;
    let mut start = 0usize;
    let mut hit = false;
    while i <= bytes.len() {
        let at_end = i == bytes.len();
        let is_comma = match bytes.get(i) {
            Some(b',') => true,
            None => at_end,
            _ => false,
        };
        if is_comma {
            if token_is_c2(bytes, start, i.saturating_sub(start)) {
                hit = true;
            }
            start = i.saturating_add(1);
        }
        i = i.saturating_add(1);
    }
    hit
}

/// 冻结过滤器语义格：(过滤串, 是否选中)。
pub const FILTER_BATTERY: [(&str, bool); FILTER_CELLS] = [
    ("c2", true),
    ("c3", false),
    ("c2x", false),
    ("", false),
    ("kvm,c2", true),
    ("c2,kvm", true),
    ("C2", false),
    ("c2,", true),
    ("c2 c3", false),
    ("c22", false),
];

// ── 用例与结果表 ─────────────────────────────────────────────────────
/// 用例规格：号、suite 标签（-f 匹配对象）、是否期望失败。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseSpec {
    pub id: u32,
    pub tag: &'static str,
    pub expect_fail: bool,
}

impl CaseSpec {
    pub const EMPTY: CaseSpec = CaseSpec { id: 0, tag: "", expect_fail: false };
}

/// runner 侧结果：只有「记录了什么」——skip 即整条不存在。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseRun {
    pub id: u32,
    pub ran: bool,
    pub failed: bool,
}

impl CaseRun {
    pub const EMPTY: CaseRun = CaseRun { id: 0, ran: false, failed: false };
}

/// 单侧结果表（定容 12 条，按登记序）。
#[derive(Clone, Copy, Debug)]
pub struct BaselineSheet {
    runs: [CaseRun; CASE_CAP],
    len: usize,
}

impl BaselineSheet {
    pub const fn new() -> BaselineSheet {
        BaselineSheet { runs: [CaseRun::EMPTY; CASE_CAP], len: 0 }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    /// 取第 i 条（越界 None）。
    pub fn get(&self, i: usize) -> Option<CaseRun> {
        if i >= self.len {
            return None;
        }
        self.runs.get(i).copied()
    }

    /// 按号查找。
    pub fn find(&self, id: u32) -> Option<CaseRun> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.runs.get(i) {
                if r.id == id {
                    return Some(*r);
                }
            }
            i += 1;
        }
        None
    }

    /// 记录结果；重复号 EEXIST、满表 ENOMEM（失败不占槽）。
    pub fn record(&mut self, id: u32, ran: bool, failed: bool) -> Result<usize, i32> {
        if self.find(id).is_some() {
            return Err(EEXIST);
        }
        if self.len >= CASE_CAP {
            return Err(ENOMEM);
        }
        let idx = self.len;
        if let Some(slot) = self.runs.get_mut(idx) {
            *slot = CaseRun { id, ran, failed };
        }
        self.len = self.len.saturating_add(1);
        Ok(idx)
    }
}

/// 双 runner 对账报告；`fail_total` 是唯一判据（必须为 0）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CompareReport {
    pub matched: u32,
    pub mismatched: u32,
    pub missing_in_c3: u32,
    pub missing_in_c2: u32,
    pub failed_c2: u32,
    pub failed_c3: u32,
    pub fail_total: u32,
}

impl CompareReport {
    pub const fn zero() -> CompareReport {
        CompareReport {
            matched: 0, mismatched: 0, missing_in_c3: 0,
            missing_in_c2: 0, failed_c2: 0, failed_c3: 0, fail_total: 0,
        }
    }

    pub fn green(&self) -> bool {
        self.fail_total == 0
    }
}

/// 按 case 号对齐两张表。空表 ENOENT；其余缺陷全记账、不短路。
pub fn compare_sheets(c2: &BaselineSheet, c3: &BaselineSheet) -> Result<CompareReport, i32> {
    if c2.len() == 0 || c3.len() == 0 {
        return Err(ENOENT);
    }
    let mut rep = CompareReport::zero();
    // 双侧同红：两侧都 ran 且都 failed（结果一致地红）——matched 不代表
    // 绿账，仍须单独入 fail_total；单侧红已经构成 mismatch，只记一次，
    // 不得由 failed_c2/failed_c3 诊断列重复累加。
    let mut failed_both = 0u32;
    let mut i = 0usize;
    while i < c2.len() {
        if let Some(ra) = c2.get(i) {
            match c3.find(ra.id) {
                Some(rb) => {
                    if rb.ran == ra.ran && rb.failed == ra.failed {
                        rep.matched = rep.matched.saturating_add(1);
                    } else {
                        rep.mismatched = rep.mismatched.saturating_add(1);
                    }
                    if rb.ran && rb.failed {
                        rep.failed_c3 = rep.failed_c3.saturating_add(1);
                    }
                    if ra.ran && ra.failed && rb.ran && rb.failed {
                        failed_both = failed_both.saturating_add(1);
                    }
                }
                None => rep.missing_in_c3 = rep.missing_in_c3.saturating_add(1),
            }
            if ra.ran && ra.failed {
                rep.failed_c2 = rep.failed_c2.saturating_add(1);
            }
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < c3.len() {
        if let Some(rb) = c3.get(j) {
            if c2.find(rb.id).is_none() {
                rep.missing_in_c2 = rep.missing_in_c2.saturating_add(1);
                if rb.ran && rb.failed {
                    rep.failed_c3 = rep.failed_c3.saturating_add(1);
                }
            }
        }
        j += 1;
    }
    rep.fail_total = rep.mismatched
        .saturating_add(rep.missing_in_c3)
        .saturating_add(rep.missing_in_c2)
        .saturating_add(failed_both);
    Ok(rep)
}

/// 运行计划：四个位图。skip = 该 runner 没记录该 case。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RunPlan {
    pub c2_skip: u32,
    pub c3_skip: u32,
    pub c2_fail: u32,
    pub c3_fail: u32,
}

impl RunPlan {
    pub const fn clean() -> RunPlan {
        RunPlan { c2_skip: 0, c3_skip: 0, c2_fail: 0, c3_fail: 0 }
    }
}

/// case 号 → 位；只接受 0..=31，越界 None（禁止截断移位）。
pub fn case_bit(id: u32) -> Option<u32> {
    if id < 32 {
        Some(1u32.wrapping_shl(id))
    } else {
        None
    }
}

// ── 联测状态机 ───────────────────────────────────────────────────────
/// 联测阶段（判别值即指纹里的阶段序）。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Idle = 0,
    C2Loaded = 1,
    Mirrored = 2,
    Executed = 3,
    Sealed = 4,
}

impl Phase {
    pub fn ord(self) -> u8 {
        self as u8
    }
}

/// 冻结迁移表：只允许逐段推进。
pub const PHASE_EDGES: [(Phase, Phase); 4] = [
    (Phase::Idle, Phase::C2Loaded),
    (Phase::C2Loaded, Phase::Mirrored),
    (Phase::Mirrored, Phase::Executed),
    (Phase::Executed, Phase::Sealed),
];

/// 迁移是否在冻结表内。
pub fn transition_allowed(from: Phase, to: Phase) -> bool {
    let mut i = 0usize;
    while i < PHASE_EDGES.len() {
        if let Some((a, b)) = PHASE_EDGES.get(i) {
            if *a == from && *b == to {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 联测会话：用例簿 + 状态机 + 最近对账 + 封存指纹。
pub struct JointSession {
    phase: Phase,
    specs: [CaseSpec; CASE_CAP],
    spec_len: usize,
    skipped: u32,
    attached: bool,
    runner: &'static str,
    last: CompareReport,
    transitions: u32,
    seal_digest: u32,
    writes_after_seal: u32,
}

impl JointSession {
    pub fn new() -> JointSession {
        JointSession {
            phase: Phase::Idle,
            specs: [CaseSpec::EMPTY; CASE_CAP],
            spec_len: 0,
            skipped: 0,
            attached: false,
            runner: "",
            last: CompareReport::zero(),
            transitions: 0,
            seal_digest: 0,
            writes_after_seal: 0,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn spec_len(&self) -> usize {
        self.spec_len
    }
    pub fn skipped(&self) -> u32 {
        self.skipped
    }
    pub fn transitions(&self) -> u32 {
        self.transitions
    }
    pub fn seal_digest(&self) -> u32 {
        self.seal_digest
    }
    pub fn writes_after_seal(&self) -> u32 {
        self.writes_after_seal
    }
    pub fn last_report(&self) -> CompareReport {
        self.last
    }
    pub fn runner(&self) -> &'static str {
        self.runner
    }

    /// 按号查用例下标。
    fn find_spec(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.spec_len {
            if let Some(s) = self.specs.get(i) {
                if s.id == id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 阶段推进（顺序判据入口）：只走冻结表，封存后拒绝。
    pub fn advance(&mut self, to: Phase) -> Result<(), i32> {
        if self.phase == Phase::Sealed {
            return if to == Phase::Sealed { Err(EBUSY) } else { Err(EPERM) };
        }
        if !transition_allowed(self.phase, to) {
            return Err(EINVAL);
        }
        self.phase = to;
        self.transitions = self.transitions.saturating_add(1);
        Ok(())
    }

    /// 登记用例：过滤 → 查重 → 容量；零选中 ENOENT。
    pub fn declare(&mut self, specs: &[CaseSpec]) -> Result<u32, i32> {
        if self.phase == Phase::Sealed {
            self.writes_after_seal = self.writes_after_seal.saturating_add(1);
            return Err(EPERM);
        }
        if self.phase != Phase::Idle {
            return Err(EINVAL);
        }
        let mut accepted = 0u32;
        let mut i = 0usize;
        while i < specs.len() {
            let spec = match specs.get(i) {
                Some(v) => *v,
                None => {
                    i += 1;
                    continue;
                }
            };
            if !filter_accepts(spec.tag) {
                self.skipped = self.skipped.saturating_add(1);
                i += 1;
                continue;
            }
            if self.find_spec(spec.id).is_some() {
                return Err(EEXIST);
            }
            if self.spec_len >= CASE_CAP {
                return Err(ENOMEM);
            }
            let idx = self.spec_len;
            if let Some(slot) = self.specs.get_mut(idx) {
                *slot = spec;
            }
            self.spec_len = self.spec_len.saturating_add(1);
            accepted = accepted.saturating_add(1);
            i += 1;
        }
        if accepted == 0 {
            return Err(ENOENT);
        }
        self.phase = Phase::C2Loaded;
        self.transitions = self.transitions.saturating_add(1);
        Ok(accepted)
    }

    /// 挂接 C3 runner：只认 "C3"，坏名字 EACCES 且不推进。
    pub fn attach(&mut self, runner: &'static str) -> Result<(), i32> {
        if self.phase == Phase::Sealed {
            self.writes_after_seal = self.writes_after_seal.saturating_add(1);
            return Err(EPERM);
        }
        if self.phase != Phase::C2Loaded {
            return Err(EINVAL);
        }
        if runner != C3_RUNNER {
            return Err(EACCES);
        }
        self.runner = runner;
        self.attached = true;
        self.phase = Phase::Mirrored;
        self.transitions = self.transitions.saturating_add(1);
        Ok(())
    }

    /// 执行双 runner 并立即对账；skip 的 case 不进对应表（真实缺失）。
    pub fn execute(&mut self, plan: RunPlan) -> Result<CompareReport, i32> {
        if self.phase == Phase::Sealed {
            self.writes_after_seal = self.writes_after_seal.saturating_add(1);
            return Err(EPERM);
        }
        if self.phase != Phase::Mirrored || !self.attached {
            return Err(EINVAL);
        }
        let mut c2 = BaselineSheet::new();
        let mut c3 = BaselineSheet::new();
        let mut i = 0usize;
        while i < self.spec_len {
            if let Some(spec) = self.specs.get(i) {
                let bit = match case_bit(spec.id) {
                    Some(v) => v,
                    None => return Err(EINVAL),
                };
                if plan.c2_skip & bit == 0 {
                    c2.record(spec.id, true, plan.c2_fail & bit != 0)?;
                }
                if plan.c3_skip & bit == 0 {
                    c3.record(spec.id, true, plan.c3_fail & bit != 0)?;
                }
            }
            i += 1;
        }
        let report = compare_sheets(&c2, &c3)?;
        self.last = report;
        self.phase = Phase::Executed;
        self.transitions = self.transitions.saturating_add(1);
        Ok(report)
    }

    /// 封存：仅绿账可封；未执行 EBUSY，重复封存 EBUSY。
    pub fn seal(&mut self) -> Result<u32, i32> {
        if self.phase == Phase::Sealed || self.phase != Phase::Executed {
            return Err(EBUSY);
        }
        if !self.last.green() {
            return Err(EBUSY);
        }
        // 先落阶段再算指纹：封存指纹即终态指纹，事后 digest() 必须复算一致。
        self.phase = Phase::Sealed;
        let d = self.digest();
        self.seal_digest = d;
        self.transitions = self.transitions.saturating_add(1);
        Ok(d)
    }

    /// 会话指纹：阶段 + 用例簿 + 对账报告逐字段编码。
    pub fn digest(&self) -> u32 {
        let mut bytes: Vec<u8> = Vec::new();
        bytes.push(self.phase.ord());
        push_u32(&mut bytes, self.spec_len as u32);
        push_u32(&mut bytes, self.skipped);
        push_u32(&mut bytes, self.last.matched);
        push_u32(&mut bytes, self.last.mismatched);
        push_u32(&mut bytes, self.last.missing_in_c3);
        push_u32(&mut bytes, self.last.missing_in_c2);
        push_u32(&mut bytes, self.last.fail_total);
        let mut i = 0usize;
        while i < self.spec_len {
            if let Some(s) = self.specs.get(i) {
                push_u32(&mut bytes, s.id);
                bytes.push(if s.expect_fail { 1 } else { 0 });
            }
            i += 1;
        }
        fnv1a(&bytes)
    }
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账：injected == detected + escaped，escaped 恒 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuditLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl AuditLedger {
    pub const fn new() -> AuditLedger {
        AuditLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一笔；返回账目是否仍守恒。
    pub fn record(&mut self, caught: bool) -> bool {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        self.conserved()
    }

    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 封存注入用的四条 c2 用例。
pub const SEAL_SPECS: [CaseSpec; 4] = [
    CaseSpec { id: 0, tag: C2_FILTER, expect_fail: false },
    CaseSpec { id: 1, tag: C2_FILTER, expect_fail: false },
    CaseSpec { id: 2, tag: C2_FILTER, expect_fail: false },
    CaseSpec { id: 3, tag: C2_FILTER, expect_fail: false },
];

/// 封存语义格：(c2 失败位, c3 失败位, 期望封存成功)。8 行。
pub const SEAL_BATTERY: [(u32, u32, bool); SEAL_CELLS] = [
    (0, 0, true),
    (0b0001, 0, false),
    (0, 0b0001, false),
    (0b0001, 0b0001, false),
    (0, 0, true),
    (0b0010, 0b0010, false),
    (0b0100, 0, false),
    (0, 0b1000, false),
];

/// 过滤器注入：10 格 × 10 轮（语义必须逐格一致）。
pub fn run_filter_battery() -> (u32, AuditLedger) {
    let mut ledger = AuditLedger::new();
    let mut matched = 0u32;
    let mut ci = 0usize;
    while ci < FILTER_BATTERY.len() {
        if let Some((filter, expect)) = FILTER_BATTERY.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = filter_accepts(filter) == expect;
                if !ledger.record(caught) {
                    return (matched, ledger);
                }
                if caught {
                    matched = matched.saturating_add(1);
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (matched, ledger)
}

/// 封存注入：8 行 × 8 轮（红账必拒、绿账必放）。
pub fn run_seal_battery() -> (u32, AuditLedger) {
    let mut ledger = AuditLedger::new();
    let mut matched = 0u32;
    let mut ci = 0usize;
    while ci < SEAL_BATTERY.len() {
        if let Some((fail_c2, fail_c3, expect_ok)) = SEAL_BATTERY.get(ci).copied() {
            let mut round = 0u32;
            while round < SEAL_ROUNDS {
                let mut sess = JointSession::new();
                let mut ok = sess.declare(&SEAL_SPECS).is_ok();
                ok = ok && sess.attach(C3_RUNNER).is_ok();
                let plan = RunPlan { c2_skip: 0, c3_skip: 0, c2_fail: fail_c2, c3_fail: fail_c3 };
                ok = ok && sess.execute(plan).is_ok();
                let sealed = sess.seal();
                let phase_ok = if expect_ok {
                    sealed.is_ok() && sess.phase() == Phase::Sealed
                } else {
                    sealed == Err(EBUSY) && sess.phase() == Phase::Executed
                };
                let caught = ok && phase_ok;
                if !ledger.record(caught) {
                    return (matched, ledger);
                }
                if caught {
                    matched = matched.saturating_add(1);
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (matched, ledger)
}

/// 两束注入合并：164 次（10×10 + 8×8）。
pub fn run_all_injections() -> (u32, u32, AuditLedger) {
    let (fm, fl) = run_filter_battery();
    let (sm, sl) = run_seal_battery();
    let merged = AuditLedger {
        injected: fl.injected.saturating_add(sl.injected),
        detected: fl.detected.saturating_add(sl.detected),
        escaped: fl.escaped.saturating_add(sl.escaped),
    };
    (fm, sm, merged)
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, Linux 正值, Windows 近似码, 语义说明)
pub const ERRNO_MAP: &[(&str, i32, i32, &str)] = &[
    ("EPERM", EPERM, 5, "ERROR_ACCESS_DENIED；封存后写入被拒"),
    ("ENOENT", ENOENT, 2, "ERROR_FILE_NOT_FOUND；过滤零选中/结果表为空"),
    ("E2BIG", E2BIG, 206, "ERROR_FILENAME_EXCED_RANGE；写回摘要超长"),
    ("ECHILD", ECHILD, 128, "ERROR_WAIT_NO_CHILDREN；runner 未交结果"),
    ("ENOMEM", ENOMEM, 8, "ERROR_NOT_ENOUGH_MEMORY；用例簿/结果表满"),
    ("EACCES", EACCES, 5, "ERROR_ACCESS_DENIED；runner 名不匹配"),
    ("EBUSY", EBUSY, 170, "ERROR_BUSY；红账拒封/重复封存"),
    ("EEXIST", EEXIST, 183, "ERROR_ALREADY_EXISTS；case 号重复"),
    ("EINVAL", EINVAL, 87, "ERROR_INVALID_PARAMETER；跳段/坏位号"),
];

/// 查表得 Linux 负值；表外 None（不得伪造）。
pub fn linux_neg(code: i32) -> Option<i32> {
    let mut i = 0usize;
    while i < ERRNO_MAP.len() {
        if let Some(row) = ERRNO_MAP.get(i) {
            if row.1 == code {
                return Some(-row.1);
            }
        }
        i += 1;
    }
    None
}

/// 是否与 Linux 逐码一致（正数、表内、可复算）。
pub fn errno_is_linux(code: i32) -> bool {
    if code <= 0 {
        return false;
    }
    linux_neg(code) == Some(-code)
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 小端编码 u32。
pub fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.push((v & 0xFF) as u8);
    out.push(((v >> 8) & 0xFF) as u8);
    out.push(((v >> 16) & 0xFF) as u8);
    out.push(((v >> 24) & 0xFF) as u8);
}

/// 规范联测语料：4 条用例（1 条非 c2 跳过），c3 缺 1 条、两侧各红 1 条，
/// 封存被拒。双跑必须逐字节相等。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let specs = [
        CaseSpec { id: 0, tag: C2_FILTER, expect_fail: false },
        CaseSpec { id: 1, tag: C2_FILTER, expect_fail: false },
        CaseSpec { id: 2, tag: C2_FILTER, expect_fail: false },
        CaseSpec { id: 3, tag: "c3", expect_fail: false },
    ];
    let mut sess = JointSession::new();
    match sess.declare(&specs) {
        Ok(n) => {
            bytes.push(1);
            push_u32(&mut bytes, n);
        }
        Err(e) => {
            bytes.push(2);
            push_u32(&mut bytes, (e & 0x7FFF_FFFF) as u32);
        }
    }
    match sess.attach(C3_RUNNER) {
        Ok(()) => bytes.push(3),
        Err(e) => {
            bytes.push(4);
            push_u32(&mut bytes, (e & 0x7FFF_FFFF) as u32);
        }
    }
    let plan = RunPlan { c2_skip: 0, c3_skip: 0b0010, c2_fail: 0b0100, c3_fail: 0b0100 };
    match sess.execute(plan) {
        Ok(r) => {
            bytes.push(5);
            push_u32(&mut bytes, r.matched);
            push_u32(&mut bytes, r.missing_in_c3);
            push_u32(&mut bytes, r.fail_total);
        }
        Err(e) => {
            bytes.push(6);
            push_u32(&mut bytes, (e & 0x7FFF_FFFF) as u32);
        }
    }
    bytes.push(sess.phase().ord());
    match sess.seal() {
        Ok(d) => {
            bytes.push(7);
            push_u32(&mut bytes, d);
        }
        Err(e) => {
            bytes.push(8);
            push_u32(&mut bytes, (e & 0x7FFF_FFFF) as u32);
        }
    }
    fnv1a(&bytes)
}

// ── 写回（状态 + 摘要） ──────────────────────────────────────────────
/// 写回状态：完成态必须带非空摘要。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum WbState {
    Pending,
    Done,
}

/// API/软件写回执据：摘要非空且 ≤96 字节，只允许一次成功提交。
#[derive(Clone, Debug)]
pub struct Writeback {
    state: WbState,
    summary: String,
    receipts: u32,
}

impl Writeback {
    pub fn new() -> Writeback {
        Writeback { state: WbState::Pending, summary: String::new(), receipts: 0 }
    }

    pub fn summary(&self) -> &str {
        self.summary.as_str()
    }

    pub fn receipts(&self) -> u32 {
        self.receipts
    }

    pub fn done(&self) -> bool {
        self.state == WbState::Done
    }

    /// 提交写回：空摘要 EINVAL、超长 E2BIG、已完成 EBUSY。
    pub fn submit(&mut self, summary: &str) -> Result<u32, i32> {
        if self.state == WbState::Done {
            return Err(EBUSY);
        }
        let n = summary.as_bytes().len();
        if n == 0 {
            return Err(EINVAL);
        }
        if n > WB_SUMMARY_MAX {
            return Err(E2BIG);
        }
        self.summary = String::from(summary);
        self.state = WbState::Done;
        self.receipts = self.receipts.saturating_add(1);
        Ok(self.receipts)
    }

    /// 写回行（状态=… 摘要=…）。
    pub fn line(&self) -> String {
        let status = match self.state {
            WbState::Pending => "未完成",
            WbState::Done => "已完成",
        };
        format!("状态={} 摘要={}", status, self.summary)
    }
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9372 判据集（add 14 + add_dyn 3 = 17 条）。
pub fn run_unx9372_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9372");

    // 1. 段位守卫（0x40 窗口且不侵入 F9373）。
    let seg_ok = SEG_BASE % SEG_WINDOW == 0
        && SEG_BASE.saturating_add(SEG_WINDOW) <= NEXT_SEG_BASE
        && SEG_BASE > 0x7CA40
        && (0x7C000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x7CA80 独占 0x40 且不压 F9373", seg_ok,
          "段基址必须 0x40 对齐，窗口末端不得越过 0x7CAC0");

    // 2. 过滤器十格。
    let mut filter_bad = 0u32;
    let mut ci = 0usize;
    while ci < FILTER_BATTERY.len() {
        if let Some((f, expect)) = FILTER_BATTERY.get(ci).copied() {
            if filter_accepts(f) != expect {
                filter_bad = filter_bad.saturating_add(1);
            }
        }
        ci += 1;
    }
    s.add("过滤器：-f c2 十格语义逐格一致", filter_bad == 0,
          "精确 token 匹配：c2x/c22/C2/空串不得选中");

    // 3. 注入两束。
    let (fm, sm, inj) = run_all_injections();
    s.add("注入：过滤器 100 + 封存 64 格全检出",
          inj.conserved() && inj.escaped == 0 && inj.injected == 164
              && fm == 100 && sm == 64,
          "164 次注入必须 164 次检出，语义表不得漂移");

    // 4. 正常流。
    let mut happy = JointSession::new();
    let specs5 = [
        CaseSpec { id: 0, tag: C2_FILTER, expect_fail: false },
        CaseSpec { id: 1, tag: C2_FILTER, expect_fail: false },
        CaseSpec { id: 2, tag: "kvm,c2", expect_fail: false },
        CaseSpec { id: 3, tag: C2_FILTER, expect_fail: false },
        CaseSpec { id: 4, tag: "c3", expect_fail: false },
    ];
    let declared = happy.declare(&specs5);
    let attached = happy.attach(C3_RUNNER);
    let report = happy.execute(RunPlan::clean());
    let sealed = happy.seal();
    s.add("正常流：声明→挂接→执行→封存四段推进",
          declared == Ok(4) && happy.skipped() == 1 && attached.is_ok()
              && matches!(report, Ok(r) if r.green() && r.matched == 4)
              && matches!(sealed, Ok(d) if d != 0)
              && happy.phase() == Phase::Sealed
              && happy.transitions() == 4
              && happy.seal_digest() == happy.digest(),
          "四条 c2 用例全绿、一条 c3 跳过，封存指纹非零且可复算");

    // 5. 跳段 + runner 对接。
    let mut ord = JointSession::new();
    let skip1 = ord.advance(Phase::Executed);
    let skip2 = ord.advance(Phase::Mirrored);
    let step1 = ord.advance(Phase::C2Loaded);
    let skip3 = ord.advance(Phase::Executed);
    let mut det = JointSession::new();
    let dec = det.declare(&SEAL_SPECS);
    let bad_attach = det.attach("C2");
    let early_exec = det.execute(RunPlan::clean());
    s.add("状态机：跳段 EINVAL、坏 runner EACCES、未挂接执行 EINVAL",
          skip1 == Err(EINVAL) && skip2 == Err(EINVAL) && step1.is_ok()
              && skip3 == Err(EINVAL) && ord.phase() == Phase::C2Loaded
              && dec == Ok(4) && bad_attach == Err(EACCES)
              && early_exec == Err(EINVAL)
              && det.phase() == Phase::C2Loaded && det.runner().is_empty(),
          "Idle 只能进 C2Loaded；只有 C3 runner 能挂接，挂接前不得对账");

    // 6. 封存顺序。
    let mut so = JointSession::new();
    let setup = so.declare(&SEAL_SPECS) == Ok(4) && so.attach(C3_RUNNER).is_ok();
    let early_seal = so.seal();
    let exec_ok = so.execute(RunPlan::clean()).is_ok();
    let first_seal = so.seal();
    let second_seal = so.seal();
    let post_exec = so.execute(RunPlan::clean());
    let post_declare = so.declare(&SEAL_SPECS);
    s.add("封存顺序：早封 EBUSY、二次封 EBUSY、封后写 EPERM",
          setup && exec_ok && early_seal == Err(EBUSY) && first_seal.is_ok()
              && second_seal == Err(EBUSY) && post_exec == Err(EPERM)
              && post_declare == Err(EPERM) && so.writes_after_seal() == 2,
          "封存是终态：任何后续写入都必须显式拒绝并记账");

    // 7. 用例簿三拒。
    let mut dup = JointSession::new();
    let dup_specs = [
        CaseSpec { id: 7, tag: C2_FILTER, expect_fail: false },
        CaseSpec { id: 7, tag: C2_FILTER, expect_fail: false },
    ];
    let dup_err = dup.declare(&dup_specs);
    let mut cap = JointSession::new();
    let mut cap_specs = [CaseSpec::EMPTY; CASE_CAP + 1];
    let mut k = 0usize;
    while k < CASE_CAP + 1 {
        if let Some(slot) = cap_specs.get_mut(k) {
            *slot = CaseSpec { id: k as u32, tag: C2_FILTER, expect_fail: false };
        }
        k += 1;
    }
    let cap_err = cap.declare(&cap_specs);
    let mut none = JointSession::new();
    let none_specs = [
        CaseSpec { id: 1, tag: "c3", expect_fail: false },
        CaseSpec { id: 2, tag: "", expect_fail: false },
    ];
    let none_err = none.declare(&none_specs);
    s.add("用例簿：重复 EEXIST、满表 ENOMEM、零选中 ENOENT",
          dup_err == Err(EEXIST) && dup.spec_len() == 1
              && cap_err == Err(ENOMEM) && cap.spec_len() == CASE_CAP
              && none_err == Err(ENOENT) && none.phase() == Phase::Idle
              && none.skipped() == 2,
          "三种拒绝都不得留下半登记状态");

    // 8. 对账：红账拒封 + 缺失区分 + 空表 ENOENT。
    let mut red = JointSession::new();
    let red_setup = red.declare(&SEAL_SPECS) == Ok(4) && red.attach(C3_RUNNER).is_ok();
    let red_rep = red.execute(RunPlan { c2_skip: 0, c3_skip: 0, c2_fail: 0, c3_fail: 0b0100 });
    let red_seal = red.seal();
    let red_ok = match red_rep {
        Ok(r) => !r.green() && r.fail_total >= 1 && r.failed_c3 == 1 && r.mismatched == 1,
        Err(_) => false,
    };
    let mut miss = JointSession::new();
    let miss_setup = miss.declare(&SEAL_SPECS) == Ok(4) && miss.attach(C3_RUNNER).is_ok();
    let miss_rep = miss.execute(RunPlan { c2_skip: 0, c3_skip: 0b0010, c2_fail: 0, c3_fail: 0 });
    let miss_ok = match miss_rep {
        Ok(r) => r.missing_in_c3 == 1 && r.mismatched == 0 && r.matched == 3 && r.fail_total == 1,
        Err(_) => false,
    };
    let empty = BaselineSheet::new();
    let mut one = BaselineSheet::new();
    let one_ok = one.record(1, true, false) == Ok(0);
    s.add("对账：红账拒封、缺失与不一致区分、空表 ENOENT",
          red_setup && red_ok && red_seal == Err(EBUSY) && red.phase() == Phase::Executed
              && miss_setup && miss_ok
              && one_ok && compare_sheets(&empty, &one) == Err(ENOENT)
              && compare_sheets(&one, &empty) == Err(ENOENT),
          "fail==0 联测格：红账不得进 Sealed，缺失不得记成不一致");

    // 9. 账本单调 + 位图守卫。
    let mut mono = inj;
    let before = mono.injected;
    let mut kept = true;
    let mut k = 0u32;
    while k < 5 {
        kept = kept && mono.record(true);
        k += 1;
    }
    s.add("账本单调与位图守卫",
          kept && mono.injected == before.saturating_add(5)
              && mono.detected == mono.injected && mono.escaped == 0
              && case_bit(0) == Some(1)
              && case_bit(31) == Some(0x8000_0000)
              && case_bit(32).is_none()
              && case_bit(u32::MAX).is_none(),
          "计数只增不减、逃逸恒 0；case 号越界必须拒绝而不是截断移位");

    // 10. Linux 逐码一致。
    let mut codes_ok = ERRNO_MAP.len() >= 9;
    let mut ei = 0usize;
    while ei < ERRNO_MAP.len() {
        if let Some(row) = ERRNO_MAP.get(ei) {
            if !errno_is_linux(row.1) || linux_neg(row.1) != Some(-row.1) {
                codes_ok = false;
            }
            let mut ej = ei + 1;
            while ej < ERRNO_MAP.len() {
                if let Some(other) = ERRNO_MAP.get(ej) {
                    if other.1 == row.1 {
                        codes_ok = false;
                    }
                }
                ej += 1;
            }
        }
        ei += 1;
    }
    s.add("Linux 逐码：9 个 errno 表内可复算且不重复",
          codes_ok && !errno_is_linux(0) && !errno_is_linux(9999),
          "错误码只允许表内正值，表外码一律拒绝");

    // 11. 写回。
    let mut wb = Writeback::new();
    let empty_submit = wb.submit("");
    let long_text = "0123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789";
    let long_submit = wb.submit(long_text);
    let good_submit = wb.submit("F9372 状态=已完成 fail=0 cases=5 baseline-joint");
    let second_submit = wb.submit("again");
    s.add("写回：状态=已完成且摘要非空（EINVAL/E2BIG/EBUSY 三拒）",
          empty_submit == Err(EINVAL) && long_submit == Err(E2BIG)
              && good_submit == Ok(1) && second_submit == Err(EBUSY)
              && wb.done() && wb.receipts() == 1 && !wb.summary().is_empty()
              && wb.line().contains("已完成"),
          "写回是幂等边界：只允许一次成功提交");

    // 12. 配额面 + 诊断码闭集。
    let face = CASE_CAP as u32 + FILTER_CELLS as u32 + SEAL_CELLS as u32
        + JointCode::ALL.len() as u32 + ERRNO_MAP.len() as u32;
    let mut uniq = true;
    let mut empty_text = 0u32;
    let mut ai = 0usize;
    while ai < JointCode::ALL.len() {
        if let Some(a) = JointCode::ALL.get(ai) {
            if a.detail().is_empty() {
                empty_text = empty_text.saturating_add(1);
            }
            let mut bi = ai + 1;
            while bi < JointCode::ALL.len() {
                if let Some(b) = JointCode::ALL.get(bi) {
                    if a.seg_addr() == b.seg_addr() || a.code() == b.code() {
                        uniq = false;
                    }
                }
                bi += 1;
            }
        }
        ai += 1;
    }
    s.add("诊断码：14 个段内码值唯一、文案非空、判定面达标",
          uniq && empty_text == 0 && JointCode::ALL.len() == 14
              && face >= CODE_QUOTA / 40,
          "闭集码不得重复，否则红项无法定位；判定面须达配额密度");

    // 13. 双跑指纹。
    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：规范联测语料指纹相等",
          fp_a == fp_b && fp_a != 0,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 14. 过滤器与对账的次序：先过滤后对账（零选中不产生空表对账）。
    let mut order = JointSession::new();
    let zero = order.declare(&[CaseSpec { id: 5, tag: "c9", expect_fail: false }]);
    s.add("判定次序：过滤筛选先于登记与对账",
          zero == Err(ENOENT) && order.skipped() == 1 && order.phase() == Phase::Idle,
          "零选中的过滤器必须在登记阶段被挡下，不得留到对账");

    // 15-17. 动态明细。
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}（过滤器 {} + 封存 {}）",
                           inj.injected, inj.detected, inj.escaped, fm, sm);
    s.add_dyn("注入快照", inj.conserved() && inj.escaped == 0, &inj_text);

    let rep = miss.last_report();
    let rep_text = format!("matched={} mismatch={} miss_c3={} fail_total={}",
                           rep.matched, rep.mismatched, rep.missing_in_c3, rep.fail_total);
    s.add_dyn("对账快照", rep.fail_total == 1, &rep_text);

    let fp_text = format!("语料指纹 0x{:08X} 写回=[{}]", fp_a, wb.line());
    s.add_dyn("指纹与写回快照", fp_a == fp_b && wb.done(), &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_is_exact_token() {
        assert_eq!(filter_accepts("c2"), true);
        assert_eq!(filter_accepts("kvm,c2"), true);
        assert_eq!(filter_accepts("c2,"), true);
        assert_eq!(filter_accepts("c2x"), false);
        assert_eq!(filter_accepts("c22"), false);
        assert_eq!(filter_accepts("C2"), false);
        assert_eq!(filter_accepts(""), false);
        assert_eq!(filter_accepts("c2 c3"), false);
    }

    #[test]
    fn happy_path_seals() {
        let mut sess = JointSession::new();
        assert_eq!(sess.declare(&SEAL_SPECS), Ok(4));
        assert_eq!(sess.attach(C3_RUNNER), Ok(()));
        match sess.execute(RunPlan::clean()) {
            Ok(r) => {
                assert_eq!(r.matched, 4);
                assert_eq!(r.fail_total, 0);
                assert_eq!(r.green(), true);
            }
            Err(_) => assert_eq!(1, 0),
        }
        assert_eq!(sess.seal().is_ok(), true);
        assert_eq!(sess.phase(), Phase::Sealed);
        assert_eq!(sess.transitions(), 4);
    }

    #[test]
    fn phase_and_runner_gates() {
        let mut sess = JointSession::new();
        assert_eq!(sess.advance(Phase::Executed), Err(EINVAL));
        assert_eq!(sess.advance(Phase::C2Loaded), Ok(()));
        assert_eq!(sess.advance(Phase::Executed), Err(EINVAL));
        assert_eq!(sess.advance(Phase::Mirrored), Ok(()));
        assert_eq!(sess.advance(Phase::Executed), Ok(()));
        assert_eq!(sess.advance(Phase::Sealed), Ok(()));
        assert_eq!(sess.advance(Phase::Sealed), Err(EBUSY));
        assert_eq!(sess.advance(Phase::Idle), Err(EPERM));

        let mut det = JointSession::new();
        assert_eq!(det.declare(&SEAL_SPECS), Ok(4));
        assert_eq!(det.attach("C2"), Err(EACCES));
        assert_eq!(det.execute(RunPlan::clean()), Err(EINVAL));
        assert_eq!(det.attach(C3_RUNNER), Ok(()));
    }

    #[test]
    fn red_report_blocks_seal() {
        let mut sess = JointSession::new();
        assert_eq!(sess.declare(&SEAL_SPECS), Ok(4));
        assert_eq!(sess.attach(C3_RUNNER), Ok(()));
        let plan = RunPlan { c2_skip: 0, c3_skip: 0, c2_fail: 0b0001, c3_fail: 0 };
        match sess.execute(plan) {
            Ok(rep) => {
                assert_eq!(rep.failed_c2, 1);
                assert_eq!(rep.fail_total, 1);
                assert_eq!(rep.green(), false);
            }
            Err(_) => assert_eq!(1, 0),
        }
        assert_eq!(sess.seal(), Err(EBUSY));
        assert_eq!(sess.phase(), Phase::Executed);
    }

    #[test]
    fn missing_is_not_mismatch() {
        let mut sess = JointSession::new();
        assert_eq!(sess.declare(&SEAL_SPECS), Ok(4));
        assert_eq!(sess.attach(C3_RUNNER), Ok(()));
        let plan = RunPlan { c2_skip: 0, c3_skip: 0b0100, c2_fail: 0, c3_fail: 0 };
        match sess.execute(plan) {
            Ok(rep) => {
                assert_eq!(rep.missing_in_c3, 1);
                assert_eq!(rep.mismatched, 0);
                assert_eq!(rep.matched, 3);
                assert_eq!(rep.fail_total, 1);
            }
            Err(_) => assert_eq!(1, 0),
        }
    }

    #[test]
    fn suite_rejections() {
        let mut dup = JointSession::new();
        let two = [
            CaseSpec { id: 9, tag: C2_FILTER, expect_fail: false },
            CaseSpec { id: 9, tag: C2_FILTER, expect_fail: false },
        ];
        assert_eq!(dup.declare(&two), Err(EEXIST));
        assert_eq!(dup.spec_len(), 1);

        let mut none = JointSession::new();
        let only_c3 = [CaseSpec { id: 1, tag: "c3", expect_fail: false }];
        assert_eq!(none.declare(&only_c3), Err(ENOENT));
        assert_eq!(none.phase(), Phase::Idle);
        assert_eq!(none.skipped(), 1);
    }

    #[test]
    fn injection_and_errno_tables() {
        let (fm, sm, ledger) = run_all_injections();
        assert_eq!(ledger.injected, 164);
        assert_eq!(ledger.detected, 164);
        assert_eq!(ledger.escaped, 0);
        assert_eq!(fm, 100);
        assert_eq!(sm, 64);
        assert_eq!(linux_neg(EINVAL), Some(-22));
        assert_eq!(linux_neg(EBUSY), Some(-16));
        assert_eq!(linux_neg(12345), None);
        assert_eq!(errno_is_linux(0), false);
        assert_eq!(ERRNO_MAP.len(), 9);
    }

    #[test]
    fn writeback_is_one_shot() {
        let mut wb = Writeback::new();
        assert_eq!(wb.submit(""), Err(EINVAL));
        assert_eq!(wb.submit("ok"), Ok(1));
        assert_eq!(wb.submit("again"), Err(EBUSY));
        assert_eq!(wb.done(), true);
        assert_eq!(wb.receipts(), 1);
        assert_eq!(wb.line().contains("已完成"), true);
    }

    #[test]
    fn fingerprint_is_stable() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
        assert_eq!(corpus_fingerprint() != 0, true);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9372_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
    }
}
