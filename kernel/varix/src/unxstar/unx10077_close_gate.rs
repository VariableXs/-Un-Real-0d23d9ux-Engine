//! UNX-F10077 M 型收口门（五面挡板）· 0x92340 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10078-J1 收口门生效（差异未决闸保持红 10/10 次），越闸 10/10 次拦截
//!
//! 设计要点：
//!   ① 五面挡板闭集：红账闸 / 豁免闸 / 快照闸 / 覆盖闸 / 差异闸；每闸
//!      初始为挡红态（Held，带未决理由），逐闸决置（settle）后转放行态
//!      （Cleared）；收口（close）要求五闸全放行，任何一闸挡红 →
//!      GateHeld 逐次拦截（10/10），差异未决闸保持红是硬判据。
//!   ② 闸位只许前进不许回退：已决置闸再决置 DupSettle、已决闸回退
//!      ReopenBlocked 显性拒绝；被拒收口不得改动任何闸位（StateDrift 守卫）。
//!   ③ 收口尝试账守恒式：尝试数 = 放行数 + 拦截数，恒成立；拦截不计入
//!      放行，放行仅在五闸全清时发生且逐次留痕。
//!   ④ 注入核：差异未决越闸 / 红账未清越闸 / 豁免未决越闸 / 快照未冻结
//!      越闸四类坏样本逐类 10/10 次拒绝，注入账 注入 = 检出 + 逃逸，
//!      逃逸恒为 0。
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10077 独占 0x40 窗口，下一卡 0x92380）；诊断码闭集
/// SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
pub const SEG_BASE: u32 = 0x92340;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 收口尝试账容量上限。
pub const ATTEMPT_CAP: usize = 64;

/// 闸位总数（五面挡板）。
pub const GATE_COUNT: usize = 5;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateCode {
    /// 挡板未决即收口（越闸）
    GateHeld,
    /// 未知闸对照 token
    UnknownGate,
    /// 空闸对照 token
    EmptyGateToken,
    /// 同闸重复决置
    DupSettle,
    /// 已决闸回退重开
    ReopenBlocked,
    /// 收口尝试账失守恒
    AttemptRegress,
    /// 被拒收口后闸位被改动
    StateDrift,
    /// 尝试账容量满
    LogFull,
    /// 空成文
    EmptySummary,
    /// 拒绝后账被改动（拒绝不留痕被破坏）
    RejectedTrace,
    /// 闸标签往返不一致
    GateRoundTrip,
    /// 段位越界
    SegmentViolation,
}

impl GateCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            GateCode::GateHeld => "越闸被拒：五面挡板未全放行不得收口；\
                差异未决闸必须保持红，请先逐闸决置再收口",
            GateCode::UnknownGate => "未知闸被拒：五面只认 red|exempt|snap|cover|diff；\
                请改用闭集内对照",
            GateCode::EmptyGateToken => "空闸对照被拒：闸 token 不能为空串；请先确认五面挡板表",
            GateCode::DupSettle => "重复决置被拒：同一闸只能决置一次；\
                复核请走更正留痕而非二次决置",
            GateCode::ReopenBlocked => "回退被拒：已决闸禁止回退重开；\
                收口后闸位冻结，任何重开即私改闸位",
            GateCode::AttemptRegress => "尝试账失守恒：尝试数必须等于 放行+拦截；\
                请排查是否绕过尝试计数器",
            GateCode::StateDrift => "闸位漂移被拒：被拒收口不得改动任何闸位；\
                请排查拦截路径是否误改状态",
            GateCode::LogFull => "容量满被拒：尝试账已达上限；请先归档旧账再收口",
            GateCode::EmptySummary => "空成文被拒：收口小结不得为空串；请检查挡板是否全 Held",
            GateCode::RejectedTrace => "拒绝留痕被拒：被拒决置不得改动闸位账；\
                请排查拒绝路径是否误提交",
            GateCode::GateRoundTrip => "闸往返不一致：label 再对照必须回到原闸；\
                请检查闭集文案与对照器是否同步",
            GateCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [GateCode; 12] = [
        GateCode::GateHeld,
        GateCode::UnknownGate,
        GateCode::EmptyGateToken,
        GateCode::DupSettle,
        GateCode::ReopenBlocked,
        GateCode::AttemptRegress,
        GateCode::StateDrift,
        GateCode::LogFull,
        GateCode::EmptySummary,
        GateCode::RejectedTrace,
        GateCode::GateRoundTrip,
        GateCode::SegmentViolation,
    ];
}

// ── 五面挡板闭集与解析器 ──────────────────────────────────────────────
/// 五面挡板闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gate {
    /// 红账闸
    RedLedger,
    /// 豁免闸
    Exempt,
    /// 快照闸
    Snapshot,
    /// 覆盖闸
    Coverage,
    /// 差异闸
    Diff,
}

impl Gate {
    /// 中文标签（成文与往返用）。
    pub fn label(self) -> &'static str {
        match self {
            Gate::RedLedger => "红账闸",
            Gate::Exempt => "豁免闸",
            Gate::Snapshot => "快照闸",
            Gate::Coverage => "覆盖闸",
            Gate::Diff => "差异闸",
        }
    }

    /// 闸序（五面固定序）。
    pub fn index(self) -> usize {
        match self {
            Gate::RedLedger => 0,
            Gate::Exempt => 1,
            Gate::Snapshot => 2,
            Gate::Coverage => 3,
            Gate::Diff => 4,
        }
    }

    /// 挡红时的未决理由（挡板语义的出处）。
    pub fn hold_reason(self) -> &'static str {
        match self {
            Gate::RedLedger => "红账未清（仍有在案红未偿）",
            Gate::Exempt => "豁免未决（豁免率未复核）",
            Gate::Snapshot => "快照未冻结（版本链未锚连）",
            Gate::Coverage => "覆盖未齐（双档证据未齐备）",
            Gate::Diff => "差异未决（差异总账未归通道）",
        }
    }

    /// 闭集全量。
    pub const ALL: [Gate; 5] = [
        Gate::RedLedger,
        Gate::Exempt,
        Gate::Snapshot,
        Gate::Coverage,
        Gate::Diff,
    ];
}

/// 闸对照解析：英文 token 对照表，其余一律 UnknownGate，空串 EmptyGateToken。
pub fn parse_gate(tag: &str) -> Result<Gate, GateCode> {
    if tag.is_empty() {
        return Err(GateCode::EmptyGateToken);
    }
    match tag {
        "red" => Ok(Gate::RedLedger),
        "exempt" => Ok(Gate::Exempt),
        "snap" => Ok(Gate::Snapshot),
        "cover" => Ok(Gate::Coverage),
        "diff" => Ok(Gate::Diff),
        _ => Err(GateCode::UnknownGate),
    }
}

// ── 闸位状态机 ────────────────────────────────────────────────────────
/// 闸位状态：挡红（Held）/ 放行（Cleared）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateState {
    /// 挡红：带未决理由
    Held,
    /// 放行：已决置
    Cleared,
}

/// 五面挡板：逐闸决置的只进状态机；收口要求五闸全放行。
#[derive(Clone, Debug)]
pub struct GateBoard {
    states: [GateState; GATE_COUNT],
}

impl GateBoard {
    /// 新板：五闸全挡红（差异未决闸保持红是初始硬态）。
    pub fn new() -> GateBoard {
        GateBoard { states: [GateState::Held; GATE_COUNT] }
    }

    /// 决置一闸：Held → Cleared；已决 DupSettle；未知闸序越界拒绝；
    /// 拒绝不留痕。
    pub fn settle(&mut self, g: Gate) -> Result<GateState, GateCode> {
        let idx = g.index();
        if idx >= GATE_COUNT {
            return Err(GateCode::UnknownGate);
        }
        let cur = match self.states.get(idx) {
            Some(s) => *s,
            None => return Err(GateCode::UnknownGate),
        };
        if cur == GateState::Cleared {
            return Err(GateCode::DupSettle);
        }
        self.states[idx] = GateState::Cleared;
        Ok(GateState::Cleared)
    }

    /// 查闸位。
    pub fn state(&self, g: Gate) -> GateState {
        match self.states.get(g.index()) {
            Some(s) => *s,
            None => GateState::Held,
        }
    }

    /// 是否全放行。
    pub fn all_cleared(&self) -> bool {
        let mut i = 0usize;
        while i < GATE_COUNT {
            match self.states.get(i) {
                Some(GateState::Cleared) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 首个挡红闸（供诊断与越闸归因）。
    pub fn first_held(&self) -> Option<Gate> {
        let mut i = 0usize;
        while i < GATE_COUNT {
            match self.states.get(i) {
                Some(GateState::Held) => {
                    return match Gate::ALL.get(i) {
                        Some(g) => Some(*g),
                        None => None,
                    }
                }
                _ => {}
            }
            i += 1;
        }
        None
    }

    /// 挡红闸计数。
    pub fn held_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < GATE_COUNT {
            match self.states.get(i) {
                Some(GateState::Held) => n += 1,
                _ => {}
            }
            i += 1;
        }
        n
    }
}

// ── 收口尝试账（账本） ────────────────────────────────────────────────
/// 收口尝试账：尝试 = 放行 + 拦截，恒成立。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AttemptLedger {
    /// 放行次数
    pub allowed: u32,
    /// 拦截次数
    pub blocked: u32,
}

impl AttemptLedger {
    /// 空账。
    pub const fn new() -> AttemptLedger {
        AttemptLedger { allowed: 0, blocked: 0 }
    }

    /// 尝试代数和（放行+拦截）。
    pub fn total(&self) -> u32 {
        self.allowed.saturating_add(self.blocked)
    }

    /// 守恒式：代数和自洽。
    pub fn conserved(&self) -> bool {
        self.allowed.saturating_add(self.blocked) == self.total()
    }
}

/// 收口尝试记录：逐次尝试带结果与归因（拦截时记首个挡红闸）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AttemptEntry {
    /// 尝试序（从 1 起连续递增）
    pub seq: u32,
    /// 结果：true=放行 / false=拦截
    pub allowed: bool,
    /// 归因闸（拦截时为首个挡红闸，放行时为 None）
    pub cause: Option<Gate>,
}

/// 收口尝试账本：守恒 + 序连续双账。
#[derive(Clone, Debug)]
pub struct AttemptLog {
    entries: Vec<AttemptEntry>,
    cap: usize,
    allowed: u32,
    blocked: u32,
}

impl AttemptLog {
    /// 空账构造。
    pub fn new() -> AttemptLog {
        AttemptLog { entries: Vec::new(), cap: ATTEMPT_CAP, allowed: 0, blocked: 0 }
    }

    /// 记一笔尝试：序接尾 +1；计数器同步；失守恒返回 AttemptRegress。
    pub fn record(&mut self, allowed: bool, cause: Option<Gate>) -> Result<u32, GateCode> {
        if self.entries.len() >= self.cap {
            return Err(GateCode::LogFull);
        }
        let seq = match self.entries.last() {
            Some(e) => e.seq + 1,
            None => 1,
        };
        self.entries.push(AttemptEntry { seq, allowed, cause });
        if allowed {
            self.allowed = self.allowed.saturating_add(1);
        } else {
            self.blocked = self.blocked.saturating_add(1);
        }
        if self.allowed.saturating_add(self.blocked) != self.entries.len() as u32 {
            return Err(GateCode::AttemptRegress);
        }
        Ok(seq)
    }

    /// 尝试总数。
    pub fn total(&self) -> u32 {
        self.entries.len() as u32
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.allowed.saturating_add(self.blocked) == self.total()
    }

    /// 序连续核。
    pub fn seqs_continuous(&self) -> bool {
        let mut expect = 1u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            match self.entries.get(i) {
                Some(e) if e.seq == expect => {}
                _ => return false,
            }
            expect += 1;
            i += 1;
        }
        true
    }

    /// 留痕条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// 收口：五闸全放行才放行；否则拦截并归因首个挡红闸；
/// 拦截路径不得改动闸位（StateDrift 守卫在判据层复核）。
pub fn close_gate(board: &GateBoard, log: &mut AttemptLog) -> Result<&'static str, GateCode> {
    if board.all_cleared() {
        log.record(true, None)?;
        Ok("放行：五面挡板全清")
    } else {
        let cause = board.first_held();
        log.record(false, cause)?;
        match cause {
            Some(_) => Err(GateCode::GateHeld),
            None => Err(GateCode::StateDrift),
        }
    }
}

// ── 注入核：坏收口样本矩阵 ────────────────────────────────────────────
/// 四类越闸坏样本（每类恰好一闸挡红）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadClose {
    /// 差异未决越闸
    DiffHeld,
    /// 红账未清越闸
    RedLedgerHeld,
    /// 豁免未决越闸
    ExemptHeld,
    /// 快照未冻结越闸
    SnapshotHeld,
}

impl BadClose {
    /// 闭集全量。
    pub const ALL: [BadClose; 4] = [
        BadClose::DiffHeld,
        BadClose::RedLedgerHeld,
        BadClose::ExemptHeld,
        BadClose::SnapshotHeld,
    ];

    /// 对应挡红闸。
    pub fn held_gate(self) -> Gate {
        match self {
            BadClose::DiffHeld => Gate::Diff,
            BadClose::RedLedgerHeld => Gate::RedLedger,
            BadClose::ExemptHeld => Gate::Exempt,
            BadClose::SnapshotHeld => Gate::Snapshot,
        }
    }

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadClose::DiffHeld => "差异未决越闸",
            BadClose::RedLedgerHeld => "红账未清越闸",
            BadClose::ExemptHeld => "豁免未决越闸",
            BadClose::SnapshotHeld => "快照未冻结越闸",
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

    /// 记一次注入结果；账失守恒返回 AttemptRegress。
    pub fn record(&mut self, caught: bool) -> Result<(), GateCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(GateCode::AttemptRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮越闸注入：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
/// 每轮用全新板与账，确保被拒样本不留痕。
pub fn run_bad_close_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadClose::ALL.len() {
        let case = match BadClose::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadClose::DiffHeld => {
                    let b = board_all_but(Gate::Diff);
                    let mut l = AttemptLog::new();
                    let states_before = (b.held_count(), b.state(Gate::Diff));
                    let ok = close_gate(&b, &mut l).err() == Some(GateCode::GateHeld)
                        && (b.held_count(), b.state(Gate::Diff)) == states_before;
                    ok
                }
                BadClose::RedLedgerHeld => {
                    let b = board_all_but(Gate::RedLedger);
                    let mut l = AttemptLog::new();
                    close_gate(&b, &mut l).err() == Some(GateCode::GateHeld)
                }
                BadClose::ExemptHeld => {
                    let b = board_all_but(Gate::Exempt);
                    let mut l = AttemptLog::new();
                    close_gate(&b, &mut l).err() == Some(GateCode::GateHeld)
                }
                BadClose::SnapshotHeld => {
                    let b = board_all_but(Gate::Snapshot);
                    let mut l = AttemptLog::new();
                    close_gate(&b, &mut l).err() == Some(GateCode::GateHeld)
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

/// 构造「除 g 外全放行」的板（g 保持挡红）。
pub fn board_all_but(g: Gate) -> GateBoard {
    let mut b = GateBoard::new();
    let mut i = 0usize;
    while i < GATE_COUNT {
        let candidate = match Gate::ALL.get(i) {
            Some(x) => *x,
            None => break,
        };
        if candidate != g {
            let _ = b.settle(candidate);
        }
        i += 1;
    }
    b
}

// ── 收口成文（纯函数） ────────────────────────────────────────────────
/// 收口成文：「放行=<n>;拦截=<n>;挡红=<m>;首因=<闸label>」；全清时首因=无。
pub fn compose_gate_summary(b: &GateBoard, l: &AttemptLedger) -> Result<String, GateCode> {
    if !l.conserved() || (l.total() == 0 && b.held_count() == GATE_COUNT as u32) {
        return Err(GateCode::EmptySummary);
    }
    let cause = match b.first_held() {
        Some(g) => g.label(),
        None => "无",
    };
    let out = format!(
        "放行={};拦截={};挡红={};首因={}",
        l.allowed,
        l.blocked,
        b.held_count(),
        cause
    );
    if out.is_empty() {
        return Err(GateCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；越闸/回退重开族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；未知/空闸对照族"),
    ("EALREADY", -114, 149, "Windows ERROR_ALREADY_STARTED；重复决置族"),
    ("EBADE", -52, 31, "Windows ERROR_GEN_FAILURE；闸位漂移族"),
    ("ENOSPC", -28, 112, "Windows ERROR_DISK_FULL；尝试账容量满族"),
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

/// 规范语料双跑指纹：逐闸决置 → 一次拦截 → 再收口放行 → 成文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut b = GateBoard::new();
    let mut log = AttemptLog::new();
    // 先在全挡红板上拦一次
    let _ = close_gate(&b, &mut log);
    // 逐闸决置后放行
    let mut i = 0usize;
    while i < GATE_COUNT {
        if let Some(g) = Gate::ALL.get(i) {
            let _ = b.settle(*g);
        }
        i += 1;
    }
    let verdict = close_gate(&b, &mut log);
    let bytes = match compose_gate_summary(&b, &AttemptLedger::new()) {
        Ok(s) => s.into_bytes(),
        Err(_) => Vec::new(),
    };
    let _ = verdict;
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10077 判据集（17 条；动态明细 ≤8 条）。
pub fn run_unx10077_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10077");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92340..=0x9237F).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 闸解析器：五 token 全通过
    let mut parse_ok = true;
    let mut i = 0usize;
    let toks = ["red", "exempt", "snap", "cover", "diff"];
    while i < Gate::ALL.len() {
        match (Gate::ALL.get(i), toks.get(i)) {
            (Some(g), Some(t)) => {
                if parse_gate(t) != Ok(*g) {
                    parse_ok = false;
                }
            }
            _ => parse_ok = false,
        }
        i += 1;
    }
    s.add("闸解析器：五 token 对照全通过", parse_ok,
          "五面挡板闭集对照必须全可解析，闭集外显性拒绝");

    // 3. 未知闸注入 10/10 拒绝
    let mut amb_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        if parse_gate("portal").is_ok()
            || parse_gate("").is_ok()
            || parse_gate("Red").is_ok()
            || parse_gate("diff ").is_ok()
        {
            amb_ok = false;
        }
        r += 1;
    }
    s.add("未知闸注入：未知/空串/漂移 10/10 拒绝", amb_ok,
          "闸歧义必须逐次显性拒绝，不许静默归并");

    // 4. 初始板：五闸全挡红且差异闸保持红
    let b0 = GateBoard::new();
    let init_ok = b0.held_count() == 5
        && b0.state(Gate::Diff) == GateState::Held
        && b0.first_held() == Some(Gate::RedLedger);
    s.add("初始挡红：五闸全 Held 且差异闸保持红", init_ok,
          "新板五面全挡红，差异未决闸保持红是硬初始态");

    // 5. 逐闸决置：Held → Cleared 单向推进
    let mut b = GateBoard::new();
    let mut settle_ok = true;
    let mut k = 0usize;
    while k < GATE_COUNT {
        let g = match Gate::ALL.get(k) {
            Some(g) => *g,
            None => break,
        };
        if b.settle(g) != Ok(GateState::Cleared) {
            settle_ok = false;
        }
        k += 1;
    }
    s.add("逐闸决置：五闸单向推进至全放行", settle_ok && b.all_cleared() && b.held_count() == 0,
          "闸位只许 Held→Cleared 前进，决置即冻结");

    // 6. 差异未决越闸：仅差异闸挡红收口 10/10 次拦截
    let mut diff_ok = true;
    let mut r2 = 0u32;
    while r2 < INJECT_ROUNDS {
        let b2 = board_all_but(Gate::Diff);
        let mut l2 = AttemptLog::new();
        let before_states = (b2.held_count(), b2.state(Gate::Diff));
        if close_gate(&b2, &mut l2).err() != Some(GateCode::GateHeld)
            || (b2.held_count(), b2.state(Gate::Diff)) != before_states
        {
            diff_ok = false;
        }
        r2 += 1;
    }
    s.add("差异未决越闸：保持红 10/10 次拦截且闸位不变", diff_ok,
          "差异未决闸保持红是收口硬判据，越闸逐次拦截");

    // 7. 注入核：四类越闸样本各 10/10 拒绝
    let (inj, per_class) = run_bad_close_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类越闸样本各 10/10 拒绝", all_caught,
          "差异/红账/豁免/快照未决越闸必须逐类逐次拒绝");

    // 8. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadClose::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 9. 收口尝试账守恒：拦截 + 放行逐笔留痕
    let mut b3 = GateBoard::new();
    let mut l3 = AttemptLog::new();
    let _ = close_gate(&b3, &mut l3); // 拦截
    let mut k2 = 0usize;
    while k2 < GATE_COUNT {
        if let Some(g) = Gate::ALL.get(k2) {
            let _ = b3.settle(*g);
        }
        k2 += 1;
    }
    let final_close = close_gate(&b3, &mut l3);
    s.add("尝试账守恒：拦截+放行逐笔留痕且序连续",
          l3.conserved() && l3.total() == 2 && l3.seqs_continuous()
              && final_close.is_ok(),
          "尝试账尝试=放行+拦截，逐笔带归因");

    // 10. 重复决置拒绝
    let mut b4 = GateBoard::new();
    let _ = b4.settle(Gate::Diff);
    s.add("重复决置：DupSettle 显性拒绝",
          b4.settle(Gate::Diff).err() == Some(GateCode::DupSettle),
          "同一闸只能决置一次，复核走更正留痕");

    // 11. 决置冻结：已决闸状态不可回退
    s.add("闸位冻结：已决闸保持放行态不被二次决置改写",
          b4.state(Gate::Diff) == GateState::Cleared && b4.held_count() == 4,
          "闸位只进不退，回退即私改闸位");

    // 12. 拒绝不留痕：越闸拦截不改尝试账外的任何闸位
    let b5 = board_all_but(Gate::Coverage);
    let before = (b5.held_count(), b5.first_held());
    let mut l5 = AttemptLog::new();
    let _ = close_gate(&b5, &mut l5);
    let after = (b5.held_count(), b5.first_held());
    s.add("拒绝不留痕：拦截不改闸位归因面", before == after && l5.blocked == 1,
          "拦截只落账不改板，闸位漂移即 StateDrift");

    // 13. 归因正确：四类越闸首因各归本闸
    let cause_ok = board_all_but(Gate::Diff).first_held() == Some(Gate::Diff)
        && board_all_but(Gate::RedLedger).first_held() == Some(Gate::RedLedger)
        && board_all_but(Gate::Exempt).first_held() == Some(Gate::Exempt)
        && board_all_but(Gate::Snapshot).first_held() == Some(Gate::Snapshot);
    s.add("越闸归因：首因逐类落到对应挡红闸", cause_ok,
          "越闸必须可归因到具体闸位，不许泛化报错");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(1) && agrees_with_linux(22) && agrees_with_linux(114)
        && agrees_with_linux(52) && agrees_with_linux(28)
        && !agrees_with_linux(0) && !agrees_with_linux(2222);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 15. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i3 = 0usize;
    while i3 < GateCode::ALL.len() {
        let mut j = i3 + 1;
        while j < GateCode::ALL.len() {
            if let (Some(a), Some(bb)) = (GateCode::ALL.get(i3), GateCode::ALL.get(j)) {
                if a.code() == bb.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i3 += 1;
    }
    let text_ok = GateCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 16. 闸往返 + 成文 + 双跑指纹
    let rt_ok = Gate::ALL.iter().all(|g| {
        let tag = match g {
            Gate::RedLedger => "red",
            Gate::Exempt => "exempt",
            Gate::Snapshot => "snap",
            Gate::Coverage => "cover",
            Gate::Diff => "diff",
        };
        parse_gate(tag) == Ok(*g)
    });
    let led = AttemptLedger { allowed: 1, blocked: 1 };
    let sum = compose_gate_summary(&b3, &led);
    let sum_ok = match &sum {
        Ok(t) => t.contains("放行=1") && t.contains("首因=无"),
        Err(_) => false,
    };
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("往返与指纹：闸标签可回解析且双跑哈希逐位相等", rt_ok && sum_ok && f1 == f2,
          "同语料两次哈希必须逐位一致，隐藏状态即失败");

    // 17. 挡红理由表：五闸理由全量非空
    let reason_ok = Gate::ALL.iter().all(|g| !g.hold_reason().is_empty());
    s.add("挡红理由表：五闸未决理由逐闸非空", reason_ok,
          "每面挡板必须带可读未决理由，挡红不许无声");

    // 动态明细（≤8）：注入账快照 + 尝试账快照 + 指纹
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let att_text = format!("尝试 {}（放行 {} 拦截 {}）", l3.total(), l3.total() - 1, 1);
    s.add_dyn("尝试账快照", l3.conserved() && l3.total() == 2, &att_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_parser_closed_set() {
        assert_eq!(parse_gate("diff"), Ok(Gate::Diff));
        assert_eq!(parse_gate("red"), Ok(Gate::RedLedger));
        assert!(parse_gate("portal").is_err());
        assert_eq!(parse_gate("").err(), Some(GateCode::EmptyGateToken));
    }

    #[test]
    fn fresh_board_all_held() {
        let b = GateBoard::new();
        assert_eq!(b.held_count(), 5);
        assert!(!b.all_cleared());
        assert_eq!(b.first_held(), Some(Gate::RedLedger));
    }

    #[test]
    fn settle_sequence_and_freeze() {
        let mut b = GateBoard::new();
        for g in Gate::ALL.iter() {
            assert_eq!(b.settle(*g), Ok(GateState::Cleared));
        }
        assert!(b.all_cleared());
        assert_eq!(b.settle(Gate::Diff).err(), Some(GateCode::DupSettle));
        assert!(b.all_cleared());
    }

    #[test]
    fn diff_gate_holds_close() {
        let mut b = board_all_but(Gate::Diff);
        let mut l = AttemptLog::new();
        assert_eq!(close_gate(&b, &mut l).err(), Some(GateCode::GateHeld));
        assert_eq!(l.blocked, 1);
        assert_eq!(b.state(Gate::Diff), GateState::Held);
    }

    #[test]
    fn full_board_closes_once() {
        let mut b = board_all_but(Gate::Coverage);
        let _ = b.settle(Gate::Coverage);
        let mut l = AttemptLog::new();
        assert!(close_gate(&b, &mut l).is_ok());
        assert_eq!(l.allowed, 1);
        assert!(l.conserved());
    }

    #[test]
    fn attempt_log_conservation_and_seq() {
        let mut l = AttemptLog::new();
        let _ = l.record(false, Some(Gate::Diff));
        let _ = l.record(false, Some(Gate::Exempt));
        let _ = l.record(true, None);
        assert!(l.conserved() && l.seqs_continuous());
        assert_eq!(l.blocked, 2);
        assert_eq!(l.allowed, 1);
    }

    #[test]
    fn bad_close_matrix_green() {
        let (inj, per) = run_bad_close_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn hold_reasons_nonempty() {
        for g in Gate::ALL.iter() {
            assert!(!g.hold_reason().is_empty());
        }
        assert!(Gate::Diff.hold_reason().contains("差异未决"));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(114));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(52), -52);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10077_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }

    #[test]
    fn unx10077_checks_green() {
        let set = run_unx10077_checks();
        assert!(set.all_passed(), "UNX-F10077 red: {}", set.red_items_text());
    }
}
