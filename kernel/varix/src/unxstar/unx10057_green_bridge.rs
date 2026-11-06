//! UNX-F10057 回归与全绿口径衔接 · 0x91E40 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10058-J1 熔断生效（连红 K 次即停批 10/10 次），失灵 10/10 次检出
//!
//! 设计要点：
//!   ① 双口径闭集：常态绿（RoutineGreen）与收口全量（CloseoutFull）
//!      是两条独立口径，桥规则「常态绿不替代收口全量」：只交常态绿
//!      就申请封桥 → CaliberSwap 逐次拦截（10/10）。
//!   ② 桥相位状态机：Idle → RoutinePassed → CloseoutPassed → Sealed，
//!      秩序只进不退；收口全量必须先过常态档（乱序 → OutOfOrder），
//!      重复提交同一档 → DupStage。
//!   ③ 覆盖注册表：收口全量必须覆盖全部 16 段回归矩阵（covered ==
//!      COVERAGE_TOTAL），缺段 → MissingCoverage 逐次拦截（10/10）；
//!      常态档允许子集但不许越界（CoverageOverflow）。
//!   ④ 衔接账本守恒：submissions == 常态提交数 + 收口提交数，且每次
//!      提交留痕（阶段/口径/覆盖数）；拒绝路径不改账（拒绝不留痕）。
//!   ⑤ 注入核四类坏样本逐类 10/10 次拒绝，注入账守恒逃逸 = 0；
//!     errno 对照表 + FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10057 独占 0x40 窗口，下一卡 0x91E80）；诊断码闭集
/// SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
pub const SEG_BASE: u32 = 0x91E40;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 收口全量覆盖段数：回归矩阵 16 段全覆盖才算收口全量。
pub const COVERAGE_TOTAL: u32 = 16;

/// 衔接账本容量上限。
pub const LEDGER_CAP: usize = 64;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BridgeCode {
    /// 口径互替：常态绿顶替收口全量
    CaliberSwap,
    /// 收口缺段：覆盖数不足全量
    MissingCoverage,
    /// 覆盖越界：覆盖数越过总段数
    CoverageOverflow,
    /// 乱序提交：收口先于常态档
    OutOfOrder,
    /// 重复提交同一档
    DupStage,
    /// 相位倒退（状态机秩序被破坏）
    PhaseRegress,
    /// 未知口径 token
    UnknownCaliber,
    /// 空口径 token
    EmptyCaliber,
    /// 衔接账失守恒
    LedgerRegress,
    /// 账本容量满
    LedgerFull,
    /// 空成文
    EmptySummary,
    /// 段位越界
    SegmentViolation,
}

impl BridgeCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            BridgeCode::CaliberSwap => "口径互替被拒：常态绿不替代收口全量；\
                请先跑齐 16 段收口全量再用收口口径封桥",
            BridgeCode::MissingCoverage => "收口缺段被拒：覆盖数必须等于全量段数；\
                请补齐缺席段再提交收口",
            BridgeCode::CoverageOverflow => "覆盖越界被拒：覆盖数不得越过总段数；请复核覆盖清单",
            BridgeCode::OutOfOrder => "乱序提交被拒：收口全量必须先过常态档；\
                请先提交常态绿再提交收口",
            BridgeCode::DupStage => "重复提交被拒：同一档只允许提交一次；\
                请核对桥相位再提交",
            BridgeCode::PhaseRegress => "相位倒退被拒：桥相位只进不退；请排查乱序调用",
            BridgeCode::UnknownCaliber => "未知口径 token 被拒：口径只认 routine|closeout \
                及其单字母缩写；请改用闭集内写法",
            BridgeCode::EmptyCaliber => "空口径 token 被拒：口径不能为空串",
            BridgeCode::LedgerRegress => "衔接账失守恒：submissions 必须等于 \
                常态+收口提交数之和；请排查绕账路径",
            BridgeCode::LedgerFull => "账本容量满被拒：已达上限；请先封桥归档",
            BridgeCode::EmptySummary => "空成文被拒：衔接小结不得为空串；请检查账本",
            BridgeCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [BridgeCode; 12] = [
        BridgeCode::CaliberSwap,
        BridgeCode::MissingCoverage,
        BridgeCode::CoverageOverflow,
        BridgeCode::OutOfOrder,
        BridgeCode::DupStage,
        BridgeCode::PhaseRegress,
        BridgeCode::UnknownCaliber,
        BridgeCode::EmptyCaliber,
        BridgeCode::LedgerRegress,
        BridgeCode::LedgerFull,
        BridgeCode::EmptySummary,
        BridgeCode::SegmentViolation,
    ];
}

// ── 口径闭集与解析器 ──────────────────────────────────────────────────
/// 双口径闭集：常态绿 / 收口全量。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Caliber {
    /// 常态绿：周期回归的常态挡板口径（允许子集覆盖）
    RoutineGreen,
    /// 收口全量：16 段矩阵全覆盖的收口口径
    CloseoutFull,
}

impl Caliber {
    /// 中文标签（成文用）。
    pub fn label(self) -> &'static str {
        match self {
            Caliber::RoutineGreen => "常态绿",
            Caliber::CloseoutFull => "收口全量",
        }
    }

    /// 长名（解析往返用）。
    pub fn tag(self) -> &'static str {
        match self {
            Caliber::RoutineGreen => "routine",
            Caliber::CloseoutFull => "closeout",
        }
    }

    /// 闭集全量。
    pub const ALL: [Caliber; 2] = [Caliber::RoutineGreen, Caliber::CloseoutFull];
}

/// 口径解析：长短名双形制（routine|r / closeout|c），其余 UnknownCaliber，
/// 空串 EmptyCaliber。
pub fn parse_caliber(tag: &str) -> Result<Caliber, BridgeCode> {
    if tag.is_empty() {
        return Err(BridgeCode::EmptyCaliber);
    }
    match tag {
        "routine" | "r" => Ok(Caliber::RoutineGreen),
        "closeout" | "c" => Ok(Caliber::CloseoutFull),
        _ => Err(BridgeCode::UnknownCaliber),
    }
}

// ── 桥相位状态机 ──────────────────────────────────────────────────────
/// 桥相位：Idle → RoutinePassed → CloseoutPassed → Sealed，只进不退。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BridgePhase {
    /// 未提交任何口径
    Idle,
    /// 常态绿已过
    RoutinePassed,
    /// 收口全量已过
    CloseoutPassed,
    /// 已封桥
    Sealed,
}

impl BridgePhase {
    /// 相位秩（只进不退校验用）。
    pub fn rank(self) -> u8 {
        match self {
            BridgePhase::Idle => 0,
            BridgePhase::RoutinePassed => 1,
            BridgePhase::CloseoutPassed => 2,
            BridgePhase::Sealed => 3,
        }
    }

    /// 中文标签（成文用）。
    pub fn label(self) -> &'static str {
        match self {
            BridgePhase::Idle => "待常态",
            BridgePhase::RoutinePassed => "常态已过",
            BridgePhase::CloseoutPassed => "收口已过",
            BridgePhase::Sealed => "已封桥",
        }
    }

    /// 闭集全量。
    pub const ALL: [BridgePhase; 4] = [
        BridgePhase::Idle,
        BridgePhase::RoutinePassed,
        BridgePhase::CloseoutPassed,
        BridgePhase::Sealed,
    ];
}

// ── 提交留痕 ──────────────────────────────────────────────────────────
/// 提交留痕：口径 + 覆盖数 + 提交前相位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SubmitRecord {
    /// 提交口径
    pub caliber: Caliber,
    /// 覆盖段数
    pub covered: u32,
    /// 提交前相位
    pub from_phase: BridgePhase,
}

impl SubmitRecord {
    /// 留痕自检：覆盖数非零且不越界。
    pub fn sound(&self) -> bool {
        self.covered > 0 && self.covered <= COVERAGE_TOTAL
    }
}

/// 衔接账本：常态档与收口档分别提交，封桥只认收口全量。
#[derive(Clone, Debug)]
pub struct BridgeLedger {
    phase: BridgePhase,
    records: Vec<SubmitRecord>,
    cap: usize,
    routine_subs: u32,
    closeout_subs: u32,
}

impl BridgeLedger {
    /// 空账构造（Idle 相位）。
    pub fn new() -> BridgeLedger {
        BridgeLedger {
            phase: BridgePhase::Idle,
            records: Vec::new(),
            cap: LEDGER_CAP,
            routine_subs: 0,
            closeout_subs: 0,
        }
    }

    /// 当前桥相位。
    pub fn phase(&self) -> BridgePhase {
        self.phase
    }

    /// 提交常态绿：仅 Idle 相位允许（DupStage/乱序拒绝）；覆盖允许子集
    /// 但不得越界；提交后相位 → RoutinePassed。
    pub fn submit_routine(&mut self, covered: u32) -> Result<BridgePhase, BridgeCode> {
        if covered == 0 || covered > COVERAGE_TOTAL {
            return Err(BridgeCode::CoverageOverflow);
        }
        if self.phase != BridgePhase::Idle {
            return Err(BridgeCode::DupStage);
        }
        if self.records.len() >= self.cap {
            return Err(BridgeCode::LedgerFull);
        }
        self.records.push(SubmitRecord { caliber: Caliber::RoutineGreen, covered, from_phase: BridgePhase::Idle });
        self.routine_subs = self.routine_subs.saturating_add(1);
        self.phase = BridgePhase::RoutinePassed;
        Ok(self.phase)
    }

    /// 提交收口全量：仅 RoutinePassed 相位允许（Idle 乱序、重复档拒绝）；
    /// 覆盖必须等于 COVERAGE_TOTAL（缺段 MissingCoverage）；提交后相位
    /// → CloseoutPassed。
    pub fn submit_closeout(&mut self, covered: u32) -> Result<BridgePhase, BridgeCode> {
        if covered > COVERAGE_TOTAL {
            return Err(BridgeCode::CoverageOverflow);
        }
        if covered < COVERAGE_TOTAL {
            return Err(BridgeCode::MissingCoverage);
        }
        if self.phase == BridgePhase::Idle {
            return Err(BridgeCode::OutOfOrder);
        }
        if self.phase.rank() >= BridgePhase::CloseoutPassed.rank() {
            return Err(BridgeCode::DupStage);
        }
        if self.records.len() >= self.cap {
            return Err(BridgeCode::LedgerFull);
        }
        self.records.push(SubmitRecord {
            caliber: Caliber::CloseoutFull,
            covered,
            from_phase: BridgePhase::RoutinePassed,
        });
        self.closeout_subs = self.closeout_subs.saturating_add(1);
        self.phase = BridgePhase::CloseoutPassed;
        Ok(self.phase)
    }

    /// 封桥：只认收口全量口径（常态绿顶替 → CaliberSwap）；相位必须
    /// 已到 CloseoutPassed（缺档 → OutOfOrder）。
    pub fn seal(&mut self, caliber: Caliber) -> Result<(), BridgeCode> {
        if caliber != Caliber::CloseoutFull {
            return Err(BridgeCode::CaliberSwap);
        }
        if self.phase.rank() < BridgePhase::CloseoutPassed.rank() {
            return Err(BridgeCode::OutOfOrder);
        }
        if self.phase == BridgePhase::Sealed {
            return Err(BridgeCode::PhaseRegress);
        }
        self.phase = BridgePhase::Sealed;
        Ok(())
    }

    /// 提交留痕清单（只读）。
    pub fn records(&self) -> &[SubmitRecord] {
        &self.records
    }

    /// 守恒：submissions == 常态 + 收口，且留痕全自洽。
    pub fn conserved(&self) -> bool {
        let sum = self.routine_subs.saturating_add(self.closeout_subs);
        if sum != self.records.len() as u32 {
            return false;
        }
        let mut i = 0usize;
        while i < self.records.len() {
            match self.records.get(i) {
                Some(r) => {
                    if !r.sound() {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 已提交笔数。
    pub fn len(&self) -> usize {
        self.records.len()
    }
}

// ── 衔接成文（纯函数） ────────────────────────────────────────────────
/// 衔接成文：「桥=<相位>;常态=<n>;收口=<m>;覆盖=<total>」；失守恒先拒。
pub fn compose_bridge_summary(l: &BridgeLedger) -> Result<String, BridgeCode> {
    if !l.conserved() {
        return Err(BridgeCode::LedgerRegress);
    }
    let out = format!(
        "桥={};常态={};收口={};覆盖={}",
        l.phase().label(),
        l.routine_subs,
        l.closeout_subs,
        COVERAGE_TOTAL
    );
    if out.is_empty() {
        return Err(BridgeCode::EmptySummary);
    }
    Ok(out)
}

// ── 注入核：坏衔接样本矩阵 ────────────────────────────────────────────
/// 四类坏衔接样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadBridge {
    /// 口径互替：只交常态绿就封桥
    CaliberSwap,
    /// 收口缺段
    MissingCoverage,
    /// 收口先于常态档
    OutOfOrder,
    /// 常态档重复提交
    DupStage,
}

impl BadBridge {
    /// 闭集全量。
    pub const ALL: [BadBridge; 4] = [
        BadBridge::CaliberSwap,
        BadBridge::MissingCoverage,
        BadBridge::OutOfOrder,
        BadBridge::DupStage,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadBridge::CaliberSwap => "口径互替",
            BadBridge::MissingCoverage => "收口缺段",
            BadBridge::OutOfOrder => "乱序提交",
            BadBridge::DupStage => "重复提交",
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

    /// 记一次注入结果；账失守恒返回 LedgerRegress。
    pub fn record(&mut self, caught: bool) -> Result<(), BridgeCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(BridgeCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏衔接注入矩阵：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
pub fn run_bad_bridge_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadBridge::ALL.len() {
        let case = match BadBridge::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadBridge::CaliberSwap => {
                    let mut l = BridgeLedger::new();
                    let _ = l.submit_routine(COVERAGE_TOTAL / 2);
                    // 常态绿顶替收口全量封桥
                    l.seal(Caliber::RoutineGreen).err() == Some(BridgeCode::CaliberSwap)
                        && l.phase() != BridgePhase::Sealed
                }
                BadBridge::MissingCoverage => {
                    let mut l = BridgeLedger::new();
                    let _ = l.submit_routine(COVERAGE_TOTAL / 2);
                    l.submit_closeout(COVERAGE_TOTAL - 1).err() == Some(BridgeCode::MissingCoverage)
                }
                BadBridge::OutOfOrder => {
                    let mut l = BridgeLedger::new();
                    l.submit_closeout(COVERAGE_TOTAL).err() == Some(BridgeCode::OutOfOrder)
                }
                BadBridge::DupStage => {
                    let mut l = BridgeLedger::new();
                    let _ = l.submit_routine(COVERAGE_TOTAL / 2);
                    l.submit_routine(COVERAGE_TOTAL / 2).err() == Some(BridgeCode::DupStage)
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
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；口径互替/乱序族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏覆盖/坏口径族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复提交族"),
    ("ENODATA", -61, 232, "Windows ERROR_NO_DATA；收口缺段族"),
    ("ENOSPC", -28, 112, "Windows ERROR_DISK_FULL；账本容量满族"),
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

/// 规范语料双跑指纹：常态提交 → 收口全量提交 → 封桥 → 成文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut l = BridgeLedger::new();
    let _ = l.submit_routine(COVERAGE_TOTAL / 2);
    let _ = l.submit_closeout(COVERAGE_TOTAL);
    let _ = l.seal(Caliber::CloseoutFull);
    let bytes = match compose_bridge_summary(&l) {
        Ok(s) => s.into_bytes(),
        Err(_) => Vec::new(),
    };
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10057 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10057_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10057");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91E40..=0x91E7F).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠");

    // 2. 口径解析器：双形制全通过
    let parse_ok = parse_caliber("routine") == Ok(Caliber::RoutineGreen)
        && parse_caliber("r") == Ok(Caliber::RoutineGreen)
        && parse_caliber("closeout") == Ok(Caliber::CloseoutFull)
        && parse_caliber("c") == Ok(Caliber::CloseoutFull);
    s.add("口径解析器：routine|r / closeout|c 双形制全通过", parse_ok,
          "口径闭集双形制必须全可解析，闭集外显性拒绝");

    // 3. 歧义口径注入 10/10 拒绝
    let mut amb_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        if parse_caliber("green").is_ok()
            || parse_caliber("").is_ok()
            || parse_caliber("ROUTINE").is_ok()
            || parse_caliber("closeout ").is_ok()
        {
            amb_ok = false;
        }
        r += 1;
    }
    s.add("歧义口径注入：闭集外/空串 10/10 拒绝", amb_ok,
          "口径歧义必须逐次显性拒绝，不许静默归并");

    // 4. 相位状态机：全链路 10/10 次秩序推进
    let mut chain_ok = true;
    let mut rr = 0u32;
    while rr < INJECT_ROUNDS {
        let mut l = BridgeLedger::new();
        let _ = l.submit_routine(COVERAGE_TOTAL / 2);
        let p1 = l.phase();
        let _ = l.submit_closeout(COVERAGE_TOTAL);
        let p2 = l.phase();
        let _ = l.seal(Caliber::CloseoutFull);
        let p3 = l.phase();
        if p1 != BridgePhase::RoutinePassed
            || p2 != BridgePhase::CloseoutPassed
            || p3 != BridgePhase::Sealed
        {
            chain_ok = false;
        }
        rr += 1;
    }
    s.add("桥相位状态机：全链路 10/10 次秩序推进", chain_ok,
          "Idle→RoutinePassed→CloseoutPassed→Sealed 必须逐次按序到达");

    // 5. 常态绿不替代收口：只交常态就封桥 10/10 次拦截
    let mut swap_ok = true;
    let mut rr = 0u32;
    while rr < INJECT_ROUNDS {
        let mut l = BridgeLedger::new();
        let _ = l.submit_routine(COVERAGE_TOTAL);
        if l.seal(Caliber::RoutineGreen).err() != Some(BridgeCode::CaliberSwap)
            || l.phase() == BridgePhase::Sealed
        {
            swap_ok = false;
        }
        rr += 1;
    }
    s.add("口径衔接核：常态绿替代收口 10/10 次拦截", swap_ok,
          "即便常态全绿，收口全量档位缺失也必须拦截封桥");

    // 6. 收口缺段拦截：覆盖不足 10/10 次拒绝
    let mut cov_ok = true;
    let mut rr = 0u32;
    while rr < INJECT_ROUNDS {
        let mut l = BridgeLedger::new();
        let _ = l.submit_routine(COVERAGE_TOTAL / 2);
        if l.submit_closeout(COVERAGE_TOTAL - 1).err() != Some(BridgeCode::MissingCoverage)
            || l.submit_closeout(COVERAGE_TOTAL / 2).err() != Some(BridgeCode::MissingCoverage)
        {
            cov_ok = false;
        }
        rr += 1;
    }
    s.add("收口缺段：覆盖不足 10/10 次拦截", cov_ok,
          "收口全量必须 16 段全覆盖，缺一段即拒绝");

    // 7. 注入核：四类坏衔接各 10/10 拒绝
    let (inj, per_class) = run_bad_bridge_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏衔接样本各 10/10 拒绝", all_caught,
          "口径互替/缺段/乱序/重复提交必须逐类逐次拒绝");

    // 8. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadBridge::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 9. 衔接账守恒：submissions == 常态+收口 且留痕自洽
    let mut l = BridgeLedger::new();
    let _ = l.submit_routine(COVERAGE_TOTAL / 2);
    let _ = l.submit_closeout(COVERAGE_TOTAL);
    let _ = l.seal(Caliber::CloseoutFull);
    let rec_ok = l.conserved() && l.len() == 2
        && l.records().iter().all(|r| r.sound());
    s.add("衔接守恒：提交数==常态+收口且留痕全自洽", rec_ok,
          "每次提交必须留痕（口径/覆盖/前相位），绕账路径即失败");

    // 10. 相位秩序：乱序收口/重复档拒绝且不留痕
    let mut l2 = BridgeLedger::new();
    let oo = l2.submit_closeout(COVERAGE_TOTAL).err() == Some(BridgeCode::OutOfOrder);
    let _ = l2.submit_routine(COVERAGE_TOTAL / 2);
    let dup = l2.submit_routine(COVERAGE_TOTAL / 2).err() == Some(BridgeCode::DupStage)
        && l2.len() == 1;
    s.add("相位秩序：乱序/重复拒绝且账不受扰", oo && dup,
          "被拒提交不得进入留痕账，桥相位不受扰动");

    // 11. 覆盖越界拒绝
    let mut l3 = BridgeLedger::new();
    s.add("覆盖越界：覆盖数越过总段数拒绝",
          l3.submit_routine(COVERAGE_TOTAL + 1).err() == Some(BridgeCode::CoverageOverflow)
              && l3.len() == 0,
          "覆盖清单必须落在 1..=16 段内，越界显性拒绝");

    // 12. 封桥后再封桥拒绝（幂等保护）
    let reseal = l.seal(Caliber::CloseoutFull).err() == Some(BridgeCode::PhaseRegress);
    s.add("重复封桥：Sealed 后再封桥判 PhaseRegress", reseal,
          "封桥一次定案，重复封桥显性拒绝");

    // 13. 成文非空且含标记 + 双跑一致
    let sum = compose_bridge_summary(&l);
    let text_ok = match &sum {
        Ok(t) => t.contains("桥=已封桥") && t.contains("覆盖=16"),
        Err(_) => false,
    };
    s.add("衔接成文：标记齐全且两次成文逐字节一致",
          text_ok && sum == compose_bridge_summary(&l),
          "成文必须携带 桥/常态/收口/覆盖 标记且为纯函数");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(1) && agrees_with_linux(22) && agrees_with_linux(17)
        && agrees_with_linux(61) && agrees_with_linux(28)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 15. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < BridgeCode::ALL.len() {
        let mut j = i + 1;
        while j < BridgeCode::ALL.len() {
            if let (Some(a), Some(b)) = (BridgeCode::ALL.get(i), BridgeCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = BridgeCode::ALL.iter().all(|c| !c.detail().is_empty());
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
    let sum_text = format!("衔接小结字节数 {}", sum_len);
    s.add_dyn("成文快照", sum_len > 0, &sum_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_closed_set() {
        assert_eq!(parse_caliber("routine"), Ok(Caliber::RoutineGreen));
        assert_eq!(parse_caliber("c"), Ok(Caliber::CloseoutFull));
        assert!(parse_caliber("green").is_err());
        assert_eq!(parse_caliber("").err(), Some(BridgeCode::EmptyCaliber));
    }

    #[test]
    fn full_bridge_chain() {
        let mut l = BridgeLedger::new();
        assert_eq!(l.submit_routine(8).ok(), Some(BridgePhase::RoutinePassed));
        assert_eq!(l.submit_closeout(16).ok(), Some(BridgePhase::CloseoutPassed));
        assert!(l.seal(Caliber::CloseoutFull).is_ok());
        assert_eq!(l.phase(), BridgePhase::Sealed);
        assert!(l.conserved());
    }

    #[test]
    fn routine_cannot_substitute_closeout() {
        let mut l = BridgeLedger::new();
        let _ = l.submit_routine(16);
        assert_eq!(l.seal(Caliber::RoutineGreen).err(), Some(BridgeCode::CaliberSwap));
    }

    #[test]
    fn closeout_requires_full_coverage() {
        let mut l = BridgeLedger::new();
        let _ = l.submit_routine(8);
        assert_eq!(l.submit_closeout(15).err(), Some(BridgeCode::MissingCoverage));
        assert!(l.submit_closeout(16).is_ok());
    }

    #[test]
    fn bad_bridge_matrix_green() {
        let (inj, per) = run_bad_bridge_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn rejected_without_trace() {
        let mut l = BridgeLedger::new();
        let _ = l.submit_routine(8);
        let before = l.len();
        let _ = l.submit_routine(8);
        let _ = l.submit_closeout(15);
        assert_eq!(l.len(), before);
    }

    #[test]
    fn summary_contains_marks() {
        let mut l = BridgeLedger::new();
        let _ = l.submit_routine(8);
        let _ = l.submit_closeout(16);
        let _ = l.seal(Caliber::CloseoutFull);
        let s = compose_bridge_summary(&l).unwrap_or_default();
        assert!(s.contains("桥=已封桥") && s.contains("收口=1"));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(61));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(22), -22);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10057_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }

    #[test]
    fn unx10057_checks_green() {
        let set = run_unx10057_checks();
        assert!(set.all_passed(), "UNX-F10057 red: {}", set.red_items_text());
    }
}
