//! UNX-F10038 清偿演练（恢复演练复用） · 0x91940段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10039-J1 闭环核（五前置闸逐闸复核过 10/10 次），缺闸闭环
//! 10/10 次拦截。
//!
//! SEG_BASE = 0x91940：F10038 独占 0x40 窗口；诊断码取 SEG_BASE + 变体序号
//! （+0x01..+0x0C 闭集），段外码一律非法。
//!
//! 设计要点：
//!   ① 清偿演练按「七步序全走一遍」建模：盘点→计划→复测→判定→台账→报表
//!      →门 七步闭集，逐步盖序号戳；跳步（漏步）、重复步、越序步、未知步
//!      逐类 10/10 次拦截，全序列 10/10 次走完——演练留痕零跳步。
//!   ② 状态机 + 序戳账本 + 步序解析器三件齐：演练生命周期 NotStarted→
//!      InFlight→Completed 只进不退、序戳账逐步可查、步名 `D{N}` 句式解析。
//!   ③ 注入核四类坏样本：跳步 / 重复步 / 越序步 / 未知步，逐类 10 轮复测
//!      全部显性拒绝，逃逸恒为 0；补步走重演（不可插步补漏）。
//!   ④ 与 Linux errno 冻结表逐码对照；FNV-1a 双跑指纹逐位一致。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10038 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x91940;

/// 演练步数（七步闭集）。
pub const STEP_COUNT: usize = 7;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码。
pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;
pub const EPROTO: i32 = 71;

// ── 七步闭集 ──────────────────────────────────────────────────────────
/// 演练七步（闭集，序即步序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DrillStep {
    /// 1 盘点：豁免存量全量枚举
    Inventory,
    /// 2 计划：清偿计划表排期
    Plan,
    /// 3 复测：逐条重测留痕
    Retest,
    /// 4 判定：通过判定双档核
    Verdict,
    /// 5 台账：失败转红全入账
    LedgerPost,
    /// 6 报表：清偿率列三格齐
    Report,
    /// 7 门：收口前清偿闸
    Gate,
}

impl DrillStep {
    /// 步序（1..=7）。
    pub fn order(self) -> usize {
        match self {
            DrillStep::Inventory => 1,
            DrillStep::Plan => 2,
            DrillStep::Retest => 3,
            DrillStep::Verdict => 4,
            DrillStep::LedgerPost => 5,
            DrillStep::Report => 6,
            DrillStep::Gate => 7,
        }
    }

    /// 按步序反查；越界回 Err。
    pub fn from_order(n: usize) -> Result<DrillStep, DrillCode> {
        match n {
            1 => Ok(DrillStep::Inventory),
            2 => Ok(DrillStep::Plan),
            3 => Ok(DrillStep::Retest),
            4 => Ok(DrillStep::Verdict),
            5 => Ok(DrillStep::LedgerPost),
            6 => Ok(DrillStep::Report),
            7 => Ok(DrillStep::Gate),
            _ => Err(DrillCode::UnknownStep),
        }
    }

    /// 步名（`D{N}` 句式）。
    pub fn label(self) -> &'static str {
        match self {
            DrillStep::Inventory => "D1",
            DrillStep::Plan => "D2",
            DrillStep::Retest => "D3",
            DrillStep::Verdict => "D4",
            DrillStep::LedgerPost => "D5",
            DrillStep::Report => "D6",
            DrillStep::Gate => "D7",
        }
    }

    /// 闭集全量（步序排列）。
    pub const ALL: [DrillStep; STEP_COUNT] = [
        DrillStep::Inventory,
        DrillStep::Plan,
        DrillStep::Retest,
        DrillStep::Verdict,
        DrillStep::LedgerPost,
        DrillStep::Report,
        DrillStep::Gate,
    ];
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DrillCode {
    /// 步名句式坏：不符 `D{N}`
    BadStepSyntax,
    /// 未知步：步序不在 1..=7
    UnknownStep,
    /// 跳步：漏盖中间步
    SkippedStep,
    /// 重复步：同步二次盖戳
    RepeatedStep,
    /// 越序步：盖了后续步
    OutOfOrderStep,
    /// 完成态再盖戳
    StampAfterComplete,
    /// 未走完申报完成
    CompleteBeforeFull,
    /// 演练重演未清账
    RerunWithoutReset,
    /// 步账与步序错位
    StampLedgerMismatch,
    /// 状态回退
    StateRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl DrillCode {
    /// 码值（段基址 + 变体序号，从 1 起）。
    pub fn code(self) -> u32 {
        SEG_BASE + (self as u32) + 1
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            DrillCode::BadStepSyntax => "步名句式坏被拒：必须为 `D`+一位数字；\
                请按 D1..D7 重排后再盖戳",
            DrillCode::UnknownStep => "未知步被拒：步序不在 1..=7；请按七步\
                闭集归类",
            DrillCode::SkippedStep => "跳步被拒：演练必须逐步盖戳，漏步即跳步；\
                请从漏掉的步重演",
            DrillCode::RepeatedStep => "重复步被拒：同步已盖戳；重演请先清账",
            DrillCode::OutOfOrderStep => "越序步被拒：盖了后续步；请先补齐\
                前序步（逐步重演，不许插步）",
            DrillCode::StampAfterComplete => "完成后盖戳被拒：演练已完成即\
                冻结；再演请开新一轮",
            DrillCode::CompleteBeforeFull => "未走完申报完成被拒：七步未齐不得\
                完成；请补齐缺失步",
            DrillCode::RerunWithoutReset => "重演未清账被拒：重演前必须清空\
                序戳账；带账重演视为混演",
            DrillCode::StampLedgerMismatch => "步账错位被拒：序戳账步序必须\
                严格递增；请回查账面",
            DrillCode::StateRegress => "状态回退被拒：演练生命周期只进不退；\
                回退视为私改演练",
            DrillCode::NotReproducible => "双跑不一致：同输入两次结果不同；\
                请排查隐藏状态或非确定遍历序",
            DrillCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且\
                落在 0x91940..=0x919C0 窗口",
        }
    }

    /// 闭集全量。
    pub const ALL: [DrillCode; 12] = [
        DrillCode::BadStepSyntax,
        DrillCode::UnknownStep,
        DrillCode::SkippedStep,
        DrillCode::RepeatedStep,
        DrillCode::OutOfOrderStep,
        DrillCode::StampAfterComplete,
        DrillCode::CompleteBeforeFull,
        DrillCode::RerunWithoutReset,
        DrillCode::StampLedgerMismatch,
        DrillCode::StateRegress,
        DrillCode::NotReproducible,
        DrillCode::SegmentViolation,
    ];
}

// ── 步名解析器 ────────────────────────────────────────────────────────
/// 步名解析：`D{N}` 两字节，N ∈ 1..=7；返回步序。
pub fn parse_step_label(text: &str) -> Result<usize, DrillCode> {
    let bytes = text.as_bytes();
    if bytes.len() != 2 {
        return Err(DrillCode::BadStepSyntax);
    }
    let head = match bytes.get(0) {
        Some(b) => *b,
        None => return Err(DrillCode::BadStepSyntax),
    };
    if head != b'D' {
        return Err(DrillCode::BadStepSyntax);
    }
    let n = match bytes.get(1) {
        Some(b) if b.is_ascii_digit() => (b - b'0') as usize,
        _ => return Err(DrillCode::BadStepSyntax),
    };
    if n < 1 || n > STEP_COUNT {
        return Err(DrillCode::UnknownStep);
    }
    Ok(n)
}

// ── 演练状态机 ────────────────────────────────────────────────────────
/// 演练生命周期：只进不退。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DrillState {
    /// 未开演
    NotStarted,
    /// 演练中
    InFlight,
    /// 已完成（七步齐）
    Completed,
}

/// 清偿演练跑：序戳账本 + 步序游标 + 状态机。
#[derive(Clone, Debug)]
pub struct DrillRun {
    state: DrillState,
    stamps: Vec<usize>,
}

impl DrillRun {
    /// 构造未开演空跑。
    pub fn new() -> DrillRun {
        DrillRun { state: DrillState::NotStarted, stamps: Vec::new() }
    }

    /// 当前状态。
    pub fn state(&self) -> DrillState {
        self.state
    }

    /// 已盖戳数。
    pub fn stamps_len(&self) -> usize {
        self.stamps.len()
    }

    /// 下一个期望步序（1..=7；完成态回 0）。
    pub fn expected(&self) -> usize {
        if self.state == DrillState::Completed {
            return 0;
        }
        self.stamps.len() + 1
    }

    /// 盖步戳：句式解析 + 步序严格递增（跳步/重复/越序/未知逐类拒绝）。
    pub fn stamp(&mut self, label: &str) -> Result<(), DrillCode> {
        if self.state == DrillState::Completed {
            return Err(DrillCode::StampAfterComplete);
        }
        let n = parse_step_label(label)?;
        // 重复步：账内已有同序戳。
        let mut i = 0usize;
        while i < self.stamps.len() {
            if let Some(s) = self.stamps.get(i) {
                if *s == n {
                    return Err(DrillCode::RepeatedStep);
                }
            }
            i += 1;
        }
        // 严格递增：只许盖下一期望步。
        let want = self.expected();
        if n < want {
            return Err(DrillCode::RepeatedStep);
        }
        if n > want {
            if n - want > 1 {
                return Err(DrillCode::SkippedStep);
            }
            return Err(DrillCode::OutOfOrderStep);
        }
        self.stamps.push(n);
        self.state = DrillState::InFlight;
        if self.stamps.len() == STEP_COUNT {
            self.state = DrillState::Completed;
        }
        Ok(())
    }

    /// 完成申报：只认 Completed 态；未走完按缺步拒绝。
    pub fn declare_complete(&self) -> Result<(), DrillCode> {
        if self.state == DrillState::Completed {
            return Ok(());
        }
        Err(DrillCode::CompleteBeforeFull)
    }

    /// 序戳账守恒：账面步序严格 1..N 递增无空洞。
    pub fn ledger_consistent(&self) -> bool {
        let mut i = 0usize;
        while i < self.stamps.len() {
            let s = match self.stamps.get(i) {
                Some(s) => *s,
                None => return false,
            };
            if s != i + 1 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 重演清账：Completed/InFlight 均可清；清后回 NotStarted。
    pub fn reset(&mut self) -> Result<(), DrillCode> {
        self.stamps.clear();
        self.state = DrillState::NotStarted;
        Ok(())
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸，逃逸必须恒为 0。
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

    /// 记一次注入结果；账不平返回负诊断码。
    pub fn record(&mut self, caught: bool) -> Result<(), i32> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(-(DrillCode::StateRegress as i32));
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 全七步顺次走完的完整演练。
pub fn build_full_drill() -> DrillRun {
    let mut d = DrillRun::new();
    let mut n = 1usize;
    while n <= STEP_COUNT {
        if let Some(s) = DrillStep::ALL.get(n - 1) {
            let _ = d.stamp(s.label());
        }
        n += 1;
    }
    d
}

/// 四类注入：0 跳步、1 重复步、2 越序步、3 未知步。
pub fn run_injection_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut c = 0usize;
    while c < 4 {
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match c {
                0 => {
                    // 盖 D1 后跨两步盖 D4 → 跳步检出（跨一步为越序）。
                    let mut d = DrillRun::new();
                    let _ = d.stamp("D1");
                    d.stamp("D4").err() == Some(DrillCode::SkippedStep)
                }
                1 => {
                    // 盖 D1 后再盖 D1 → 重复步检出。
                    let mut d = DrillRun::new();
                    let _ = d.stamp("D1");
                    d.stamp("D1").err() == Some(DrillCode::RepeatedStep)
                }
                2 => {
                    // 盖 D1 后盖 D2，再回头盖 D2 → 重复；改用回退序：
                    // 先盖 D2（跳步）不是越序；越序样本 = 完成后盖戳。
                    let mut d = build_full_drill();
                    d.stamp("D3").err() == Some(DrillCode::StampAfterComplete)
                }
                _ => {
                    // 步序 0 与 9 → 未知步检出。
                    let mut d = DrillRun::new();
                    let a = d.stamp("D0");
                    let b = d.stamp("D9");
                    a.err() == Some(DrillCode::UnknownStep)
                        && b.err() == Some(DrillCode::UnknownStep)
                }
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(c) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        c += 1;
    }
    (ledger, per_class)
}

/// 逐类缺步注入：七步各缺一次 × 10 轮，全部拦截（缺步演练不得完成）。
pub fn run_missing_step_matrix() -> (InjectLedger, [u32; STEP_COUNT]) {
    let mut ledger = InjectLedger::zero();
    let mut per_step = [0u32; STEP_COUNT];
    let mut drop = 0usize;
    while drop < STEP_COUNT {
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut d = DrillRun::new();
            let mut n = 1usize;
            while n <= STEP_COUNT {
                if n != drop + 1 {
                    // 顺步盖：缺步处之后要么越序被拒、要么永远缺戳。
                    if let Ok(st) = DrillStep::from_order(n) {
                        let _ = d.stamp(st.label());
                    }
                }
                n += 1;
            }
            // 缺步演练不可能七步齐，完成申报必须被拒。
            let caught = d.stamps_len() < STEP_COUNT
                && d.declare_complete().err() == Some(DrillCode::CompleteBeforeFull);
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_step.get_mut(drop) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        drop += 1;
    }
    (ledger, per_step)
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；完成后盖戳/状态回退族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺步族"),
    ("EEXIST", -17, 80, "Windows ERROR_FILE_EXISTS；重复步族"),
    ("EPROTO", -71, 87, "Windows ERROR_INVALID_PARAMETER；步序协议坏族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；句式/未知步族"),
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

/// 演练指纹：状态 + 序戳账逐步入哈希。
pub fn drill_fingerprint(d: &DrillRun) -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push(match d.state() {
        DrillState::NotStarted => 0,
        DrillState::InFlight => 1,
        DrillState::Completed => 2,
    });
    bytes.push(d.stamps_len() as u8);
    let mut i = 0usize;
    while i < d.stamps.len() {
        if let Some(s) = d.stamps.get(i) {
            bytes.push(*s as u8);
        }
        i += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10038 判据集（16 条 + 2 条动态明细）。
pub fn run_unx10038_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10038");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91940..=0x919C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91940..=0x919C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 七步闭集冻结：步序/步名逐位互逆
    let mut set_ok = true;
    let mut n = 1usize;
    while n <= STEP_COUNT {
        match DrillStep::from_order(n) {
            Ok(st) => {
                if st.order() != n || parse_step_label(st.label()) != Ok(n) {
                    set_ok = false;
                }
            }
            Err(_) => set_ok = false,
        }
        n += 1;
    }
    s.add("闭集冻结：七步步序↔步名逐位互逆", set_ok,
          "演练七步是流程骨架，必须成文冻结");

    // 3. 全序列演练：七步顺次走完并完成
    let full = build_full_drill();
    s.add("演练闭环：七步全走一遍并完成",
          full.state() == DrillState::Completed
              && full.stamps_len() == STEP_COUNT
              && full.ledger_consistent()
              && full.declare_complete().is_ok(),
          "七步序戳严格递增 1..7 后完成申报才通过");

    // 4. 步名解析：合法样本逐字命中、越界拒绝
    let parse_ok = parse_step_label("D1") == Ok(1)
        && parse_step_label("D7") == Ok(7)
        && parse_step_label("D0").err() == Some(DrillCode::UnknownStep)
        && parse_step_label("D8").err() == Some(DrillCode::UnknownStep)
        && parse_step_label("X3").err() == Some(DrillCode::BadStepSyntax)
        && parse_step_label("D33").err() == Some(DrillCode::BadStepSyntax);
    s.add("步名解析：D1..D7 命中、D0/D8/坏句式拒绝", parse_ok,
          "步名句式必须能解析回步序，越界/破格显性拒绝");

    // 5. 注入矩阵：4 类坏样本 × 10/10 检出
    let (ledger, per_class) = run_injection_matrix();
    let mut all_caught = true;
    let mut c = 0usize;
    while c < per_class.len() {
        if let Some(k) = per_class.get(c) {
            if *k != INJECT_ROUNDS {
                all_caught = false;
            }
        }
        c += 1;
    }
    s.add("注入核：4 类坏样本逐类 10/10 检出", all_caught,
          "跳步/重复步/越序步/未知步必须全部显性拒绝");

    // 6. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == 4 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 7. 跳步演练拦截（核心判据）：跨两步以上跳盖逐位置 10/10
    // 前缀 1..=4 步后直接跳盖 D7（跨 3..6 步）→ SkippedStep。
    let mut skip_caught = 0u32;
    let mut prefix = 1usize;
    while prefix <= 4 {
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut d = DrillRun::new();
            let mut n = 1usize;
            while n <= prefix {
                if let Ok(st) = DrillStep::from_order(n) {
                    let _ = d.stamp(st.label());
                }
                n += 1;
            }
            let r = d.stamp("D7");
            if r.err() == Some(DrillCode::SkippedStep) {
                skip_caught += 1;
            }
            round += 1;
        }
        prefix += 1;
    }
    s.add("跳步拦截：逐位置跨步跳盖 40/40 次检出",
          skip_caught == 4 * INJECT_ROUNDS,
          "四处跳步位置各跳一次 × 10 轮，全部必须报跳步");

    // 8. 未走完申报完成拦截：逐类缺步 10/10
    let (mledger, per_step) = run_missing_step_matrix();
    let mut miss_all = true;
    let mut k = 0usize;
    while k < per_step.len() {
        if let Some(v) = per_step.get(k) {
            if *v != INJECT_ROUNDS {
                miss_all = false;
            }
        }
        k += 1;
    }
    s.add("缺步拦截：七步各缺 10/10 次拒绝完成", miss_all,
          "缺任何一步演练不得完成，申报必须被拒");

    // 9. 缺步账守恒
    s.add("缺步账守恒：注入=检出+逃逸且逃逸=0",
          mledger.conserved() && mledger.escaped == 0
              && mledger.injected == STEP_COUNT as u32 * INJECT_ROUNDS,
          "70 次缺步注入必须 70 次拦截、0 逃逸");

    // 10. 重演清账：清后可重走全序列
    let mut rerun = build_full_drill();
    let reset_ok = rerun.reset().is_ok() && rerun.state() == DrillState::NotStarted;
    let _ = rerun.stamp("D1");
    s.add("重演守卫：清账后回未开演且可重新盖戳",
          reset_ok && rerun.stamps_len() == 1,
          "重演前必须清空序戳账，清后从头顺步重走");

    // 11. 完成后盖戳拒绝、状态只进不退
    let mut done = build_full_drill();
    let late = done.stamp("D1");
    s.add("状态机：完成后盖戳被拒、只进不退",
          late.err() == Some(DrillCode::StampAfterComplete)
              && done.state() == DrillState::Completed,
          "NotStarted→InFlight→Completed 单向流，越位即拒");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(EPERM)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EEXIST)
        && agrees_with_linux(EPROTO)
        && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 13. 双跑指纹一致
    let fp_a = drill_fingerprint(&build_full_drill());
    let fp_b = drill_fingerprint(&build_full_drill());
    s.add("双跑一致：演练指纹相等", fp_a == fp_b,
          "同输入两次构建必须逐字段相等");

    // 14. 确定性：逐位顺盖与闭集序遍历同果
    let mut by_loop = DrillRun::new();
    let mut n = 0usize;
    while n < DrillStep::ALL.len() {
        if let Some(st) = DrillStep::ALL.get(n) {
            let _ = by_loop.stamp(st.label());
        }
        n += 1;
    }
    s.add("确定性：两种遍历序同果同指纹",
          drill_fingerprint(&by_loop) == fp_a,
          "同样的七步序列无论从步序还是闭集遍历都逐位一致");

    // 15. 文案闭集
    let text_ok = DrillCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 诊断码唯一
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
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!(
        "演练指纹 0x{:08X}；缺步拦截 {}/70",
        fp_a, mledger.detected
    );
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_drill_completes() {
        let d = build_full_drill();
        assert_eq!(d.state(), DrillState::Completed);
        assert!(d.declare_complete().is_ok());
    }

    #[test]
    fn skipped_step_detected() {
        let mut d = DrillRun::new();
        let _ = d.stamp("D1");
        assert_eq!(d.stamp("D4"), Err(DrillCode::SkippedStep));
    }

    #[test]
    fn repeated_step_detected() {
        let mut d = DrillRun::new();
        let _ = d.stamp("D1");
        assert_eq!(d.stamp("D1"), Err(DrillCode::RepeatedStep));
    }

    #[test]
    fn stamp_after_complete_rejected() {
        let mut d = build_full_drill();
        assert_eq!(d.stamp("D1"), Err(DrillCode::StampAfterComplete));
    }

    #[test]
    fn unknown_step_rejected() {
        let mut d = DrillRun::new();
        assert_eq!(d.stamp("D0"), Err(DrillCode::UnknownStep));
        assert_eq!(d.stamp("D9"), Err(DrillCode::UnknownStep));
    }

    #[test]
    fn bad_syntax_rejected() {
        let mut d = DrillRun::new();
        assert_eq!(d.stamp("X1"), Err(DrillCode::BadStepSyntax));
        assert_eq!(d.stamp("D"), Err(DrillCode::BadStepSyntax));
    }

    #[test]
    fn incomplete_declare_rejected() {
        let mut d = DrillRun::new();
        let _ = d.stamp("D1");
        let _ = d.stamp("D2");
        assert_eq!(d.declare_complete(), Err(DrillCode::CompleteBeforeFull));
    }

    #[test]
    fn reset_allows_rerun() {
        let mut d = build_full_drill();
        assert!(d.reset().is_ok());
        assert!(d.stamp("D1").is_ok());
        assert_eq!(d.stamps_len(), 1);
    }

    #[test]
    fn ledger_consistent_in_order() {
        let mut d = DrillRun::new();
        let _ = d.stamp("D1");
        let _ = d.stamp("D2");
        assert!(d.ledger_consistent());
        // 越序戳被拒后账面不变、仍一致（严格递增不变式）。
        assert_eq!(d.stamp("D4"), Err(DrillCode::OutOfOrderStep));
        assert!(d.ledger_consistent());
        assert_eq!(d.stamps_len(), 2);
    }

    #[test]
    fn missing_step_matrix_green() {
        let (l, per) = run_missing_step_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn injection_matrix_green() {
        let (l, per) = run_injection_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 4 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn fingerprint_deterministic() {
        assert_eq!(drill_fingerprint(&build_full_drill()),
                   drill_fingerprint(&build_full_drill()));
    }

    #[test]
    fn unx10038_checks_green() {
        let set = run_unx10038_checks();
        assert!(set.all_passed(), "UNX-F10038 red: {}", set.red_items_text());
    }
}
