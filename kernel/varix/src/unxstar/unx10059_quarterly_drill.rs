//! UNX-F10059 回归演练（季度大回归）· 0x91EC0 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10060-J1 收口小结落账（20 条判据达成+预算 6,620 核销零偏离），核销不符 10/10 次检出
//!
//! 设计要点：
//!   ① 剧本闭集 6 步有序：冻结基线 → 全量回归 → 分型定级 → 升级处置 →
//!      报表成文 → 归档留痕；演练必须全走（全走留痕 10/10 次），跳一步
//!      → StepSkip 逐次拦截（10/10）。
//!   ② 演练状态机：done_steps 只进不退，期望步 = done+1；期望之前的步
//!      → DupStep（重复），期望之后的步 → StepSkip（跳步）；六步走满后
//!      再走 → DrillClosed（终态拒绝）。
//!   ③ 演练留痕：每步执行必须留 TraceRecord（步号+轮次），留痕数 ==
//!      done_steps 且步号 1..=6 严格递增（留痕断裂即失效）。
//!   ④ 季度登记表：形制「YYYYQn」（如 2026Q3），同季度重复演练 →
//!      DupQuarter 逐次拒绝；留痕账守恒：records == enrollments。
//!   ⑤ 注入核：跳步 / 逆序 / 未知步 / 重复步四类坏样本逐类 10/10 次拒绝，
//!     注入账守恒逃逸 = 0；errno 对照表 + FNV-1a 双跑指纹。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10059 独占 0x40 窗口，下一卡 0x91F00）；诊断码闭集
/// SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
pub const SEG_BASE: u32 = 0x91EC0;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 剧本步数：季度大回归 6 步闭集。
pub const PLAYBOOK_STEPS: u32 = 6;

/// 登记表容量上限。
pub const LEDGER_CAP: usize = 64;

/// 季度编码下限（year×10+n 的合法下界：20001 = 2000Q1）。
pub const QUARTER_MIN: u32 = 2000 * 10 + 1;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DrillCode {
    /// 跳步（越过期望步）
    StepSkip,
    /// 逆序/重复步（期望之前的步）
    DupStep,
    /// 未知步（步号越过剧本闭集）
    UnknownStep,
    /// 终态后再走步
    DrillClosed,
    /// 同季度重复演练
    DupQuarter,
    /// 季度形制非法
    BadQuarter,
    /// 空季度 token
    EmptyQuarter,
    /// 季度越界（年份或季度号超出闭集）
    QuarterRange,
    /// 留痕账失守恒（records ≠ done_steps）
    TraceRegress,
    /// 登记表容量满
    LedgerFull,
    /// 空成文
    EmptySummary,
    /// 段位越界
    SegmentViolation,
}

impl DrillCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            DrillCode::StepSkip => "跳步被拒：大回归剧本必须按步全走；\
                请回到期望步继续演练，不许跳过任何一步",
            DrillCode::DupStep => "重复步被拒：同一演练里每步只允许走一次；\
                请核对当前期望步再执行",
            DrillCode::UnknownStep => "未知步被拒：步号必须落在剧本 6 步闭集内；\
                请按 步号 1..=6 执行",
            DrillCode::DrillClosed => "终态后再走被拒：六步已走满演练关闭；\
                请换新季度开新演练",
            DrillCode::DupQuarter => "同季度重复演练被拒：同一季度只允许一次大回归演练；\
                请换新季度",
            DrillCode::BadQuarter => "季度形制非法被拒：必须为 YYYYQn 形制\
                （如 2026Q3）；请核对年份数位与 Q 记号",
            DrillCode::EmptyQuarter => "空季度被拒：季度 token 不能为空串",
            DrillCode::QuarterRange => "季度越界被拒：季度号只认 Q1..=Q4，\
                年份不得早于 2000",
            DrillCode::TraceRegress => "留痕账失守恒：留痕数必须等于已走步数；\
                请排查绕过留痕的走步路径",
            DrillCode::LedgerFull => "登记表容量满被拒：已达上限；请先归档旧季度",
            DrillCode::EmptySummary => "空成文被拒：演练小结不得为空串；请检查留痕账",
            DrillCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [DrillCode; 12] = [
        DrillCode::StepSkip,
        DrillCode::DupStep,
        DrillCode::UnknownStep,
        DrillCode::DrillClosed,
        DrillCode::DupQuarter,
        DrillCode::BadQuarter,
        DrillCode::EmptyQuarter,
        DrillCode::QuarterRange,
        DrillCode::TraceRegress,
        DrillCode::LedgerFull,
        DrillCode::EmptySummary,
        DrillCode::SegmentViolation,
    ];
}

// ── 剧本步闭集 ────────────────────────────────────────────────────────
/// 大回归剧本 6 步闭集（步号 1..=6 严格有序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DrillStep {
    /// 1 冻结基线
    Freeze,
    /// 2 全量回归
    FullRun,
    /// 3 分型定级
    Triage,
    /// 4 升级处置
    Escalate,
    /// 5 报表成文
    Report,
    /// 6 归档留痕
    Archive,
}

impl DrillStep {
    /// 步号（1 起算）。
    pub fn id(self) -> u32 {
        match self {
            DrillStep::Freeze => 1,
            DrillStep::FullRun => 2,
            DrillStep::Triage => 3,
            DrillStep::Escalate => 4,
            DrillStep::Report => 5,
            DrillStep::Archive => 6,
        }
    }

    /// 中文标签（成文用）。
    pub fn label(self) -> &'static str {
        match self {
            DrillStep::Freeze => "冻结基线",
            DrillStep::FullRun => "全量回归",
            DrillStep::Triage => "分型定级",
            DrillStep::Escalate => "升级处置",
            DrillStep::Report => "报表成文",
            DrillStep::Archive => "归档留痕",
        }
    }

    /// 由步号反查剧本步（越界 None）。
    pub fn of_id(id: u32) -> Option<DrillStep> {
        match id {
            1 => Some(DrillStep::Freeze),
            2 => Some(DrillStep::FullRun),
            3 => Some(DrillStep::Triage),
            4 => Some(DrillStep::Escalate),
            5 => Some(DrillStep::Report),
            6 => Some(DrillStep::Archive),
            _ => None,
        }
    }

    /// 闭集全量（按步号有序）。
    pub const ALL: [DrillStep; 6] = [
        DrillStep::Freeze,
        DrillStep::FullRun,
        DrillStep::Triage,
        DrillStep::Escalate,
        DrillStep::Report,
        DrillStep::Archive,
    ];
}

// ── 季度解析器 ────────────────────────────────────────────────────────
/// 季度解析：形制「YYYYQn」（2026Q3）→ year×10+n 编码；
/// 形制非法 BadQuarter、空串 EmptyQuarter、季度号或年份越界 QuarterRange。
pub fn parse_quarter(token: &str) -> Result<u32, DrillCode> {
    if token.is_empty() {
        return Err(DrillCode::EmptyQuarter);
    }
    let bytes = token.as_bytes();
    if bytes.len() != 6 {
        return Err(DrillCode::BadQuarter);
    }
    let mut year = 0u32;
    let mut i = 0usize;
    while i < 4 {
        let b = match bytes.get(i) {
            Some(b) => *b,
            None => return Err(DrillCode::BadQuarter),
        };
        if b < b'0' || b > b'9' {
            return Err(DrillCode::BadQuarter);
        }
        year = match year.checked_mul(10).and_then(|x| x.checked_add((b - b'0') as u32)) {
            Some(x) => x,
            None => return Err(DrillCode::QuarterRange),
        };
        i += 1;
    }
    let q_mark = match bytes.get(4) {
        Some(b) => *b,
        None => return Err(DrillCode::BadQuarter),
    };
    let q_digit = match bytes.get(5) {
        Some(b) => *b,
        None => return Err(DrillCode::BadQuarter),
    };
    if q_mark != b'Q' {
        return Err(DrillCode::BadQuarter);
    }
    if q_digit < b'1' || q_digit > b'4' {
        return Err(DrillCode::QuarterRange);
    }
    if year < 2000 {
        return Err(DrillCode::QuarterRange);
    }
    Ok(year * 10 + (q_digit - b'0') as u32)
}

// ── 演练留痕 ──────────────────────────────────────────────────────────
/// 演练留痕：步号 + 执行轮次。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TraceRecord {
    /// 步号（1..=6）
    pub step: u32,
    /// 执行轮次
    pub at_round: u32,
}

impl TraceRecord {
    /// 留痕自检：步号落在闭集内且轮次非零。
    pub fn sound(&self) -> bool {
        self.step >= 1 && self.step <= PLAYBOOK_STEPS && self.at_round > 0
    }
}

/// 演练状态机：期望步 = done+1，六步走满进入终态 DrillClosed。
#[derive(Clone, Debug)]
pub struct DrillRunner {
    done: u32,
    trace: Vec<TraceRecord>,
    cap: usize,
}

impl DrillRunner {
    /// 新演练（零步起算）。
    pub fn new() -> DrillRunner {
        DrillRunner { done: 0, trace: Vec::new(), cap: LEDGER_CAP }
    }

    /// 已走步数。
    pub fn done(&self) -> u32 {
        self.done
    }

    /// 期望步（done+1，走满后为 PLAYBOOK_STEPS+1）。
    pub fn expected(&self) -> u32 {
        self.done.saturating_add(1)
    }

    /// 是否已走满。
    pub fn is_complete(&self) -> bool {
        self.done >= PLAYBOOK_STEPS
    }

    /// 执行步：必须恰为期望步；期望之后 StepSkip、之前 DupStep、
    /// 闭集外 UnknownStep、走满后 DrillClosed。任何拒绝不留痕。
    pub fn do_step(&mut self, step: u32, at_round: u32) -> Result<u32, DrillCode> {
        if self.is_complete() {
            return Err(DrillCode::DrillClosed);
        }
        if DrillStep::of_id(step).is_none() {
            return Err(DrillCode::UnknownStep);
        }
        if at_round == 0 {
            return Err(DrillCode::SegmentViolation);
        }
        let expect = self.expected();
        if step > expect {
            return Err(DrillCode::StepSkip);
        }
        if step < expect {
            return Err(DrillCode::DupStep);
        }
        if self.trace.len() >= self.cap {
            return Err(DrillCode::LedgerFull);
        }
        self.trace.push(TraceRecord { step, at_round });
        self.done = self.done.saturating_add(1);
        Ok(self.done)
    }

    /// 留痕清单（只读）。
    pub fn records(&self) -> &[TraceRecord] {
        &self.trace
    }

    /// 留痕守恒：留痕数 == done 且步号严格递增 1..=n、全自洽。
    pub fn conserved(&self) -> bool {
        if self.trace.len() != self.done as usize {
            return false;
        }
        let mut i = 0usize;
        while i < self.trace.len() {
            match self.trace.get(i) {
                Some(r) => {
                    if !r.sound() || r.step != (i + 1) as u32 {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }
}

// ── 季度登记表 ────────────────────────────────────────────────────────
/// 季度登记表：同季度只允许一次大回归演练。
#[derive(Clone, Debug)]
pub struct DrillRegistry {
    quarters: Vec<u32>,
    cap: usize,
}

impl DrillRegistry {
    /// 空登记表构造。
    pub fn new() -> DrillRegistry {
        DrillRegistry { quarters: Vec::new(), cap: LEDGER_CAP }
    }

    fn find(&self, quarter: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.quarters.len() {
            if let Some(q) = self.quarters.get(i) {
                if *q == quarter {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记季度：重复 DupQuarter、满册 LedgerFull、编码越界 QuarterRange。
    pub fn enroll(&mut self, quarter: u32) -> Result<(), DrillCode> {
        if quarter < QUARTER_MIN || quarter % 10 == 0 || quarter % 10 > 4 {
            return Err(DrillCode::QuarterRange);
        }
        if self.find(quarter).is_some() {
            return Err(DrillCode::DupQuarter);
        }
        if self.quarters.len() >= self.cap {
            return Err(DrillCode::LedgerFull);
        }
        self.quarters.push(quarter);
        Ok(())
    }

    /// 已登记季度数。
    pub fn len(&self) -> usize {
        self.quarters.len()
    }

    /// 守恒：全部登记项编码合法。
    pub fn conserved(&self) -> bool {
        let mut i = 0usize;
        while i < self.quarters.len() {
            match self.quarters.get(i) {
                Some(q) => {
                    if *q < QUARTER_MIN || *q % 10 == 0 || *q % 10 > 4 {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }
}

// ── 演练成文（纯函数） ────────────────────────────────────────────────
/// 演练成文：「<季度>:冻结基线→…→归档留痕=<n>/6」，六步全走才允许成文；
/// 失守恒先拒。
pub fn compose_drill_summary(
    runner: &DrillRunner,
    quarter: u32,
) -> Result<String, DrillCode> {
    if !runner.conserved() {
        return Err(DrillCode::TraceRegress);
    }
    if !runner.is_complete() {
        return Err(DrillCode::StepSkip);
    }
    let year = quarter / 10;
    let q = quarter % 10;
    let mut out = format!("{}Q{}:", year, q);
    let mut i = 0usize;
    while i < DrillStep::ALL.len() {
        if let Some(st) = DrillStep::ALL.get(i) {
            if i > 0 {
                out.push('→');
            }
            out.push_str(st.label());
        }
        i += 1;
    }
    let tail = format!("={}/6", runner.done());
    out.push_str(&tail);
    if out.is_empty() {
        return Err(DrillCode::EmptySummary);
    }
    Ok(out)
}

// ── 注入核：坏演练样本矩阵 ────────────────────────────────────────────
/// 四类坏演练样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadDrill {
    /// 跳步：从第 1 步直接跳第 3 步
    StepSkip,
    /// 逆序：期望 2 却走 1
    ReversedStep,
    /// 未知步：步号 7
    UnknownStep,
    /// 重复步：重复走第 1 步
    DupStep,
}

impl BadDrill {
    /// 闭集全量。
    pub const ALL: [BadDrill; 4] = [
        BadDrill::StepSkip,
        BadDrill::ReversedStep,
        BadDrill::UnknownStep,
        BadDrill::DupStep,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadDrill::StepSkip => "跳步",
            BadDrill::ReversedStep => "逆序",
            BadDrill::UnknownStep => "未知步",
            BadDrill::DupStep => "重复步",
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；账失守恒返回 TraceRegress。
    pub fn record(&mut self, caught: bool) -> Result<(), DrillCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(DrillCode::TraceRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏演练注入矩阵：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
pub fn run_bad_drill_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadDrill::ALL.len() {
        let case = match BadDrill::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadDrill::StepSkip => {
                    let mut d = DrillRunner::new();
                    let _ = d.do_step(1, 1);
                    d.do_step(3, 2).err() == Some(DrillCode::StepSkip)
                }
                BadDrill::ReversedStep => {
                    let mut d = DrillRunner::new();
                    let _ = d.do_step(1, 1);
                    let _ = d.do_step(2, 2);
                    d.do_step(1, 3).err() == Some(DrillCode::DupStep)
                }
                BadDrill::UnknownStep => {
                    let mut d = DrillRunner::new();
                    d.do_step(7, 1).err() == Some(DrillCode::UnknownStep)
                }
                BadDrill::DupStep => {
                    let mut d = DrillRunner::new();
                    let _ = d.do_step(1, 1);
                    d.do_step(1, 2).err() == Some(DrillCode::DupStep)
                }
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏步号/坏季度族"),
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；跳步/终态续走族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；同季度重复族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；剧本步缺失族"),
    ("ENOSPC", -28, 112, "Windows ERROR_DISK_FULL；登记表满族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
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

/// 规范语料双跑指纹：2026Q3 登记六步全走 → 成文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut reg = DrillRegistry::new();
    let _ = reg.enroll(20263);
    let mut d = DrillRunner::new();
    let mut k = 1u32;
    while k <= PLAYBOOK_STEPS {
        let _ = d.do_step(k, k);
        k += 1;
    }
    let bytes = match compose_drill_summary(&d, 20263) {
        Ok(s) => s.into_bytes(),
        Err(_) => Vec::new(),
    };
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10059 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10059_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10059");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91EC0..=0x91EFF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠");

    // 2. 剧本闭集：6 步有序且标签非空
    let play_ok = PLAYBOOK_STEPS == 6
        && DrillStep::ALL.iter().enumerate().all(|(i, st)| st.id() == (i + 1) as u32)
        && DrillStep::ALL.iter().all(|st| !st.label().is_empty())
        && DrillStep::of_id(7).is_none();
    s.add("剧本闭集：6 步步号连续有序且标签非空", play_ok,
          "剧本步号必须 1..=6 严格有序，闭集外步号显性拒绝");

    // 3. 季度解析器：2026Q3 解析 + 非法 10/10 拒绝
    let parse_ok = parse_quarter("2026Q3") == Ok(20263)
        && parse_quarter("2024Q1") == Ok(20241);
    let mut amb_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        if parse_quarter("2026Q5").is_ok()
            || parse_quarter("2026q3").is_ok()
            || parse_quarter("26Q3").is_ok()
            || parse_quarter("").is_ok()
            || parse_quarter("2026X3").is_ok()
        {
            amb_ok = false;
        }
        r += 1;
    }
    s.add("季度解析器：YYYYQn 解析正确且歧义 10/10 拒绝", parse_ok && amb_ok,
          "季度形制必须严格校验，越界季度逐次显性拒绝");

    // 4. 演练留痕全走：六步全走 10/10 次留痕 6/6
    let mut full_ok = true;
    let mut rr = 0u32;
    while rr < INJECT_ROUNDS {
        let mut d = DrillRunner::new();
        let mut k = 1u32;
        let mut ok = true;
        while k <= PLAYBOOK_STEPS {
            if d.do_step(k, k).is_err() {
                ok = false;
            }
            k += 1;
        }
        if !ok || !d.is_complete() || d.records().len() != PLAYBOOK_STEPS as usize
            || !d.conserved()
        {
            full_ok = false;
        }
        rr += 1;
    }
    s.add("演练留痕：六步全走 10/10 次留痕 6/6", full_ok,
          "大回归剧本必须逐步执行并逐步留痕，留痕断裂即失效");

    // 5. 跳步拦截 10/10
    let mut skip_ok = true;
    let mut rr = 0u32;
    while rr < INJECT_ROUNDS {
        let mut d = DrillRunner::new();
        let _ = d.do_step(1, 1);
        if d.do_step(3, 2).err() != Some(DrillCode::StepSkip) || d.done() != 1 {
            skip_ok = false;
        }
        rr += 1;
    }
    s.add("跳步拦截：越过期望步 10/10 次拦截", skip_ok,
          "跳步样本必须逐次显性拒绝，不许静默放行");

    // 6. 逆序拦截 10/10
    let mut rev_ok = true;
    let mut rr = 0u32;
    while rr < INJECT_ROUNDS {
        let mut d = DrillRunner::new();
        let _ = d.do_step(1, 1);
        let _ = d.do_step(2, 2);
        if d.do_step(1, 3).err() != Some(DrillCode::DupStep) || d.done() != 2 {
            rev_ok = false;
        }
        rr += 1;
    }
    s.add("逆序拦截：回走已完成步 10/10 次拦截", rev_ok,
          "状态机 done 只进不退，回走企图逐次拦截");

    // 7. 注入核：四类坏演练各 10/10 拒绝
    let (inj, per_class) = run_bad_drill_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏演练样本各 10/10 拒绝", all_caught,
          "跳步/逆序/未知步/重复步必须逐类逐次拒绝");

    // 8. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadDrill::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 9. 季度登记：登记 + 重复拒绝
    let mut reg = DrillRegistry::new();
    let _ = reg.enroll(20263);
    let reg_ok = reg.len() == 1
        && reg.conserved()
        && reg.enroll(20263).err() == Some(DrillCode::DupQuarter);
    s.add("季度登记：2026Q3 建档且同季度重复拒绝", reg_ok,
          "同一季度只允许一次大回归演练，重复登记逐次拒绝");

    // 10. 留痕账守恒：records == done_steps
    let mut d = DrillRunner::new();
    let _ = d.do_step(1, 1);
    let _ = d.do_step(2, 2);
    let _ = d.do_step(3, 3);
    s.add("留痕守恒：留痕数==已走步数且步号连续",
          d.conserved() && d.done() == 3 && d.records().len() == 3,
          "留痕账必须逐笔对得上，绕过留痕的走步即失败");

    // 11. 拒绝不留痕：坏步不改留痕账
    let before = d.records().len();
    let _ = d.do_step(5, 4);
    let _ = d.do_step(1, 4);
    let _ = d.do_step(9, 4);
    s.add("拒绝不留痕：跳步/回走/未知步均不改账", d.records().len() == before && before == 3,
          "被拒走步不得改动留痕账，任何拒绝路径零副作用");

    // 12. 终态拦截：走满后再走步拒绝
    let mut full = DrillRunner::new();
    let mut k = 1u32;
    while k <= PLAYBOOK_STEPS {
        let _ = full.do_step(k, k);
        k += 1;
    }
    s.add("终态拦截：六步走满后再走判 DrillClosed",
          full.do_step(1, 7).err() == Some(DrillCode::DrillClosed),
          "演练关闭后不得续走，换季度开新演练");

    // 13. 成文非空且含六步标记 + 双跑一致
    let sum = compose_drill_summary(&full, 20263);
    let text_ok = match &sum {
        Ok(t) => t.contains("2026Q3:")
            && t.contains("冻结基线")
            && t.contains("归档留痕")
            && t.contains("=6/6"),
        Err(_) => false,
    };
    s.add("演练成文：六步标记齐全且两次成文逐字节一致",
          text_ok && sum == compose_drill_summary(&full, 20263),
          "成文必须携带 季度前缀/六步链/完成度 且为纯函数");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(22) && agrees_with_linux(1) && agrees_with_linux(17)
        && agrees_with_linux(2) && agrees_with_linux(28)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 15. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < DrillCode::ALL.len() {
        let mut j = i + 1;
        while j < DrillCode::ALL.len() {
            if let (Some(a), Some(b)) = (DrillCode::ALL.get(i), DrillCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = DrillCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 16. 双跑指纹一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑指纹：规范语料两次哈希相等", f1 == f2,
          "同输入两次必须逐位一致，隐藏状态即失败");

    // 动态明细（≤8）：注入账快照 + 指纹 + 成文长度
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let sum_len = match &sum {
        Ok(t) => t.len(),
        Err(_) => 0,
    };
    let sum_text = format!("演练小结字节数 {}", sum_len);
    s.add_dyn("成文快照", sum_len > 0, &sum_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarter_parser_closed_set() {
        assert_eq!(parse_quarter("2026Q3"), Ok(20263));
        assert!(parse_quarter("2026Q5").is_err());
        assert!(parse_quarter("2026q3").is_err());
        assert_eq!(parse_quarter("").err(), Some(DrillCode::EmptyQuarter));
        assert_eq!(parse_quarter("19XXQ1").err(), Some(DrillCode::BadQuarter));
    }

    #[test]
    fn playbook_ordered() {
        let mut expect = 1u32;
        for st in DrillStep::ALL.iter() {
            assert_eq!(st.id(), expect);
            expect += 1;
        }
    }

    #[test]
    fn full_walk_six_steps() {
        let mut d = DrillRunner::new();
        let mut k = 1u32;
        while k <= PLAYBOOK_STEPS {
            assert_eq!(d.do_step(k, k).ok(), Some(k));
            k += 1;
        }
        assert!(d.is_complete());
        assert!(d.conserved());
    }

    #[test]
    fn step_skip_rejected() {
        let mut d = DrillRunner::new();
        let _ = d.do_step(1, 1);
        assert_eq!(d.do_step(3, 2).err(), Some(DrillCode::StepSkip));
        assert_eq!(d.done(), 1);
    }

    #[test]
    fn dup_step_rejected() {
        let mut d = DrillRunner::new();
        let _ = d.do_step(1, 1);
        assert_eq!(d.do_step(1, 2).err(), Some(DrillCode::DupStep));
    }

    #[test]
    fn unknown_step_rejected() {
        let mut d = DrillRunner::new();
        assert_eq!(d.do_step(7, 1).err(), Some(DrillCode::UnknownStep));
        assert_eq!(d.do_step(0, 1).err(), Some(DrillCode::UnknownStep));
    }

    #[test]
    fn dup_quarter_rejected() {
        let mut reg = DrillRegistry::new();
        assert!(reg.enroll(20251).is_ok());
        assert_eq!(reg.enroll(20251).err(), Some(DrillCode::DupQuarter));
        assert!(reg.conserved());
    }

    #[test]
    fn bad_drill_matrix_green() {
        let (inj, per) = run_bad_drill_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(17));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(22), -22);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10059_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }

    #[test]
    fn unx10059_checks_green() {
        let set = run_unx10059_checks();
        assert!(set.all_passed(), "UNX-F10059 red: {}", set.red_items_text());
    }
}
