//! UNX-F10040 B22 收口小结与预算核销 · 0x91a00 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10040（B22 收口批）。
//! 锚点：UNX-F10041-J1 常态化口径成文落账（日/批/波三档节奏），口径歧义 10/10 次检出。
//!
//! 数据段说明：
//!   SEG_BASE = 0x91a00（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `ClosingCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 解析器 `SummaryParser`：把「judge|pass|lines|tag」小结行逐字段拆成
//!     `SummaryEntry`，四字段缺一即 ParseFailure，杜绝静默吞字段；
//!   - 状态机 `ClosingState`：Drafting → Summarized → Reconciled → Sealed
//!     单向推进，密封后任何再录入显性拒绝；
//!   - 账本 `ReconLedger`：注入 = 检出 + 逃逸，逃逸恒 0，只增不减；
//!   - 节奏口径表 `RhythmTier`：日/批/波三档口径成文落账，歧义（空文/同文）
//!     逐次检出（对齐锚点判据）；
//!   - 注入核 `BadSummaryCase`：缺判据条目 / 虚报达成 / 预算偏离 / 重复条目 /
//!     密封后改写 五类坏样本逐类 10/10 次拒绝；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10040 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x91a00;

/// 段窗上界（下一卡 0x91a40 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x91a40;

/// B22 预算行数（判据 UNX-F10040-J1：预算 6,580 核销零偏离）。
pub const BUDGET_LINES: u32 = 6580;

/// 收口判据条数（20 条判据达成）。
pub const ENTRY_COUNT: usize = 20;

/// 单条判据行数（6580 / 20 = 329，整除无余）。
pub const LINES_PER_ENTRY: u32 = 329;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 小结行字段数（judge|pass|lines|tag）。
pub const SUMMARY_FIELDS: usize = 4;

/// Linux errno 正数码（对照表口径）。
pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const EACCES: i32 = 13;
pub const EINVAL: i32 = 22;
pub const EOVERFLOW: i32 = 75;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClosingCode {
    /// 判据条目缺失（不足 20 条即核销）
    MissingEntry,
    /// 虚报达成（达成位=1 但无行数支撑）
    FakeClaim,
    /// 预算核销偏离（实计 ≠ 预算）
    BudgetDeviation,
    /// 重复判据条目
    DuplicateEntry,
    /// 越段核销（核销码不在段窗）
    OutOfSegRecon,
    /// 状态倒退（未核销即密封）
    StateRegress,
    /// 密封后再录入
    FrozenSeal,
    /// 解析失败
    ParseFailure,
    /// 节奏口径歧义（空文或同文）
    RhythmAmbiguity,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl ClosingCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ClosingCode::MissingEntry => "缺判据条目被拒：收口小结必须 20 条齐备；请补齐缺失判据再核销",
            ClosingCode::FakeClaim => "虚报达成被拒：达成位=1 但行数=0 无支撑；请先落实行数再标达成",
            ClosingCode::BudgetDeviation => "预算偏离被拒：实计行数与预算 6,580 不等；请逐条核对行数后重报",
            ClosingCode::DuplicateEntry => "重复条目被拒：同判据号已录入；请核对判据号后去重",
            ClosingCode::OutOfSegRecon => "越段核销被拒：核销码必须落在 0x91a00..=0x91a40；请核对口段位",
            ClosingCode::StateRegress => "状态倒退被拒：未到 Reconciled 不得密封；请先完成核销",
            ClosingCode::FrozenSeal => "密封后改写被拒：Sealed 态封账只读；请走勘误通道而非直改",
            ClosingCode::ParseFailure => "解析失败：小结行必须恰为四段 judge|pass|lines|tag；请补齐字段再提交",
            ClosingCode::RhythmAmbiguity => "口径歧义被拒：两档口径为空或逐字相同；请逐档独立成文",
            ClosingCode::LedgerRegress => "注入账倒退被拒：计数只增不减；请排查回滚路径误改累计值",
            ClosingCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏状态或非确定哈希",
            ClosingCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x91a00..=0x91a40",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [ClosingCode; 12] = [
        ClosingCode::MissingEntry,
        ClosingCode::FakeClaim,
        ClosingCode::BudgetDeviation,
        ClosingCode::DuplicateEntry,
        ClosingCode::OutOfSegRecon,
        ClosingCode::StateRegress,
        ClosingCode::FrozenSeal,
        ClosingCode::ParseFailure,
        ClosingCode::RhythmAmbiguity,
        ClosingCode::LedgerRegress,
        ClosingCode::NotReproducible,
        ClosingCode::SegmentViolation,
    ];
}

// ── 节奏口径表（常态化成文落账的载体） ────────────────────────────────
/// 节奏三档（日/批/波，对齐锚点判据）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RhythmTier {
    /// 日档（每日快跑）
    Daily,
    /// 批档（每批收口）
    PerBatch,
    /// 波档（每波出账）
    PerWave,
}

impl RhythmTier {
    /// 闭集全量。
    pub const ALL: [RhythmTier; 3] =
        [RhythmTier::Daily, RhythmTier::PerBatch, RhythmTier::PerWave];

    /// 档名。
    pub fn name(self) -> &'static str {
        match self {
            RhythmTier::Daily => "日档",
            RhythmTier::PerBatch => "批档",
            RhythmTier::PerWave => "波档",
        }
    }

    /// 成文口径（每档独立成文，来源 B22 收口小结常态化条款）。
    pub fn written_clause(self) -> &'static str {
        match self {
            RhythmTier::Daily => "日档口径：每日全量快跑一轮，红灯当日入 F9747 红账",
            RhythmTier::PerBatch => "批档口径：每批收口前全量复跑一次，红灯阻断收口闸",
            RhythmTier::PerWave => "波档口径：每波聚合报表出账前抽样五条对账留痕",
        }
    }
}

/// 口径歧义判定：两档口径任一为空或逐字相同即歧义。
pub fn clause_ambiguous(a: &str, b: &str) -> bool {
    a.is_empty() || b.is_empty() || a == b
}

/// 三档成文口径落账核：全部非空且两两不同。
pub fn rhythm_clauses_consistent() -> bool {
    let c = RhythmTier::ALL;
    let mut i = 0usize;
    while i < c.len() {
        let ci = match c.get(i) {
            Some(t) => t.written_clause(),
            None => return false,
        };
        if ci.is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < c.len() {
            if let Some(tj) = c.get(j) {
                if clause_ambiguous(ci, tj.written_clause()) {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 口径歧义探针：对坏文本逐轮检测，返回检出次数。
pub fn probe_clause_ambiguity(rounds: u32) -> u32 {
    let daily = RhythmTier::Daily.written_clause();
    let bads: [&str; 2] = ["", daily];
    let mut hit = 0u32;
    let mut r = 0u32;
    while r < rounds {
        let mut k = 0usize;
        while k < bads.len() {
            if let Some(b) = bads.get(k) {
                if clause_ambiguous(*b, daily) {
                    hit = hit.saturating_add(1);
                }
            }
            k += 1;
        }
        r += 1;
    }
    hit
}

// ── 小结解析器 ────────────────────────────────────────────────────────
/// 解析出的小结条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SummaryEntry {
    /// 判据号（1..=20）
    pub judge: u8,
    /// 达成位
    pub passed: bool,
    /// 声明行数
    pub lines: u32,
}

/// 小结文本解析器：字段数恰为 4，数值段须可解析，缺一即 ParseFailure。
#[derive(Default)]
pub struct SummaryParser {
    /// 累计成功解析条数
    pub parsed: u32,
    /// 累计拒绝条数
    pub rejected: u32,
}

impl SummaryParser {
    /// 解析一行小结；失败原因由 ClosingCode 表达。
    pub fn parse(&mut self, text: &str) -> Result<SummaryEntry, ClosingCode> {
        let parts: Vec<&str> = text.split('|').collect();
        if parts.len() != SUMMARY_FIELDS {
            self.rejected = self.rejected.saturating_add(1);
            return Err(ClosingCode::ParseFailure);
        }
        let judge = parse_u8(parts.get(0).copied().unwrap_or(""));
        let passed = match parts.get(1).copied().unwrap_or("") {
            "0" => Some(false),
            "1" => Some(true),
            _ => None,
        };
        let lines = parse_u32_dec(parts.get(2).copied().unwrap_or(""));
        match (judge, passed, lines) {
            (Some(j), Some(p), Some(l)) => {
                self.parsed = self.parsed.saturating_add(1);
                Ok(SummaryEntry { judge: j, passed: p, lines: l })
            }
            _ => {
                self.rejected = self.rejected.saturating_add(1);
                Err(ClosingCode::ParseFailure)
            }
        }
    }
}

fn parse_u8(s: &str) -> Option<u8> {
    let v = parse_u32_dec(s)?;
    if v <= 255 {
        Some(v as u8)
    } else {
        None
    }
}

fn parse_u32_dec(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut acc: u32 = 0;
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes.get(i)?;
        if !b.is_ascii_digit() {
            return None;
        }
        acc = acc.checked_mul(10)?.checked_add((*b - b'0') as u32)?;
        i += 1;
    }
    Some(acc)
}

// ── 收口状态机与核销账 ────────────────────────────────────────────────
/// 收口状态机：单向 Drafting → Summarized → Reconciled → Sealed。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClosingState {
    /// 起草中（条目录入进行时）
    Drafting,
    /// 20 条齐备（可核销）
    Summarized,
    /// 核销通过（预算零偏离）
    Reconciled,
    /// 已密封（封账只读）
    Sealed,
}

/// 收口器：维护 20 条判据条目与行数合计。
#[derive(Clone, Copy, Debug)]
pub struct ClosingBox {
    /// 当前状态
    pub state: ClosingState,
    /// 判据号占用表
    pub have: [bool; ENTRY_COUNT],
    /// 已录条数
    pub entries: u32,
    /// 达成条数
    pub claimed_pass: u32,
    /// 行数合计
    pub lines_sum: u32,
}

impl ClosingBox {
    /// 构造空收口器。
    pub const fn new() -> ClosingBox {
        ClosingBox {
            state: ClosingState::Drafting,
            have: [false; ENTRY_COUNT],
            entries: 0,
            claimed_pass: 0,
            lines_sum: 0,
        }
    }

    /// 录入一条小结：判据号 1..=20、不得重复、虚报（达成无行数）显性拒绝。
    pub fn enter(&mut self, e: &SummaryEntry) -> Result<ClosingState, ClosingCode> {
        if self.state == ClosingState::Sealed {
            return Err(ClosingCode::FrozenSeal);
        }
        if e.judge == 0 || (e.judge as usize) > ENTRY_COUNT {
            return Err(ClosingCode::MissingEntry);
        }
        let idx = e.judge as usize - 1;
        if *self.have.get(idx).unwrap_or(&true) {
            return Err(ClosingCode::DuplicateEntry);
        }
        if e.passed && e.lines == 0 {
            return Err(ClosingCode::FakeClaim);
        }
        if let Some(slot) = self.have.get_mut(idx) {
            *slot = true;
        }
        self.entries = self.entries.saturating_add(1);
        if e.passed {
            self.claimed_pass = self.claimed_pass.saturating_add(1);
        }
        self.lines_sum = self.lines_sum.saturating_add(e.lines);
        if self.entries == ENTRY_COUNT as u32 {
            self.state = ClosingState::Summarized;
        }
        Ok(self.state)
    }

    /// 核销：20 条齐备、全达成、行数和等于预算才通过。
    pub fn reconcile(&mut self, budget: u32) -> Result<u32, ClosingCode> {
        if self.entries != ENTRY_COUNT as u32 {
            return Err(ClosingCode::MissingEntry);
        }
        if self.claimed_pass != ENTRY_COUNT as u32 {
            return Err(ClosingCode::FakeClaim);
        }
        if self.lines_sum != budget {
            return Err(ClosingCode::BudgetDeviation);
        }
        self.state = ClosingState::Reconciled;
        Ok(self.lines_sum)
    }

    /// 密封：核销通过才可密封；密封后封账只读。
    pub fn seal(&mut self) -> Result<u32, ClosingCode> {
        if self.state != ClosingState::Reconciled {
            return Err(ClosingCode::StateRegress);
        }
        self.state = ClosingState::Sealed;
        Ok(self.lines_sum)
    }
}

/// 核销段码校验：必须落在本卡段窗内。
pub fn recon_seg_ok(code: u32) -> Result<(), ClosingCode> {
    if code >= SEG_BASE && code < SEG_END {
        Ok(())
    } else {
        Err(ClosingCode::OutOfSegRecon)
    }
}

/// 以标准语料（20 条 × 329 行）灌满收口器。
pub fn fill_standard(bx: &mut ClosingBox) -> Result<(), ClosingCode> {
    let mut j = 1u8;
    while (j as usize) <= ENTRY_COUNT {
        let e = SummaryEntry { judge: j, passed: true, lines: LINES_PER_ENTRY };
        bx.enter(&e)?;
        j += 1;
    }
    Ok(())
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏小结样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadSummaryCase {
    /// 19 条即核销
    MissingEntry,
    /// 虚报达成（行数=0）
    FakeClaim,
    /// 预算偏离（实计 ≠ 6580）
    BudgetDeviation,
    /// 同判据号重录
    DuplicateEntry,
    /// 密封后再录入
    FrozenSeal,
}

impl BadSummaryCase {
    /// 闭集全量。
    pub const ALL: [BadSummaryCase; 5] = [
        BadSummaryCase::MissingEntry,
        BadSummaryCase::FakeClaim,
        BadSummaryCase::BudgetDeviation,
        BadSummaryCase::DuplicateEntry,
        BadSummaryCase::FrozenSeal,
    ];

    /// 注入一轮：构造坏样本并触发收口器，返回是否被显式拒绝。
    pub fn inject(self) -> bool {
        match self {
            BadSummaryCase::MissingEntry => {
                let mut bx = ClosingBox::new();
                let mut j = 1u8;
                while (j as usize) < ENTRY_COUNT {
                    let e = SummaryEntry { judge: j, passed: true, lines: LINES_PER_ENTRY };
                    if bx.enter(&e).is_err() {
                        return false;
                    }
                    j += 1;
                }
                bx.reconcile(BUDGET_LINES).is_err()
            }
            BadSummaryCase::FakeClaim => {
                let mut bx = ClosingBox::new();
                bx.enter(&SummaryEntry { judge: 1, passed: true, lines: 0 }).is_err()
            }
            BadSummaryCase::BudgetDeviation => {
                let mut bx = ClosingBox::new();
                if fill_standard(&mut bx).is_err() {
                    return false;
                }
                bx.reconcile(BUDGET_LINES + 1).is_err()
            }
            BadSummaryCase::DuplicateEntry => {
                let mut bx = ClosingBox::new();
                let e = SummaryEntry { judge: 1, passed: true, lines: LINES_PER_ENTRY };
                if bx.enter(&e).is_err() {
                    return false;
                }
                bx.enter(&e).is_err()
            }
            BadSummaryCase::FrozenSeal => {
                let mut bx = ClosingBox::new();
                if fill_standard(&mut bx).is_err() {
                    return false;
                }
                if bx.reconcile(BUDGET_LINES).is_err() {
                    return false;
                }
                if bx.seal().is_err() {
                    return false;
                }
                bx.enter(&SummaryEntry { judge: 1, passed: true, lines: LINES_PER_ENTRY }).is_err()
            }
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReconLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl ReconLedger {
    const fn zero() -> ReconLedger {
        ReconLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；账失衡回 LedgerRegress。
    pub fn record(&mut self, caught: bool) -> Result<(), ClosingCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(ClosingCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_closing_injection() -> (ReconLedger, [u32; 5]) {
    let mut ledger = ReconLedger::zero();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadSummaryCase::ALL.len() {
        if let Some(case) = BadSummaryCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject();
                let _ = ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；密封后改写族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺条目/缺判据族"),
    ("EACCES", -13, 5, "Windows ERROR_ACCESS_DENIED；封账只读族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏小结行族"),
    ("EOVERFLOW", -75, 84, "Windows ERROR_ARITHMETIC_OVERFLOW；行数合计溢出族"),
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

/// 规范语料双跑指纹：解析→录入→核销→密封全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut p = SummaryParser::default();
    let mut bx = ClosingBox::new();
    let mut bytes: Vec<u8> = Vec::new();
    let texts = [
        "1|1|329|pass",
        "2|1|329|pass",
        "3|1|329|pass",
        "bad|1|329|x",
    ];
    let mut i = 0usize;
    while i < texts.len() {
        if let Some(t) = texts.get(i) {
            let r = p.parse(t);
            bytes.push(if r.is_ok() { 1 } else { 0 });
            if let Ok(e) = r {
                bytes.push(match bx.enter(&e) {
                    Ok(_) => 2,
                    Err(_) => 3,
                });
            }
        }
        i += 1;
    }
    // 以标准语料补满并核销密封，密封值入哈希。
    let mut j = 4u8;
    while (j as usize) <= ENTRY_COUNT {
        let e = SummaryEntry { judge: j, passed: true, lines: LINES_PER_ENTRY };
        let _ = bx.enter(&e);
        j += 1;
    }
    let recon = bx.reconcile(BUDGET_LINES);
    bytes.push(match recon {
        Ok(v) => (v & 0xFF) as u8,
        Err(_) => 0,
    });
    let _ = bx.seal();
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10040 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx10040_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10040");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91a00..=0x91a40", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝注册");

    // 2. 解析器：合法四段记录可解析
    let mut p = SummaryParser::default();
    let ok_parse = p.parse("7|1|329|pass").is_ok();
    s.add("解析器：合法四段小结行 1/1 解析成功", ok_parse,
          "judge|pass|lines|tag 四段齐备即入账");

    // 3. 解析器：缺段/坏位/坏数字全拒
    let bad = p.parse("7|1").is_err()
        && p.parse("7|2|329|x").is_err()
        && p.parse("7|1|abc|x").is_err()
        && p.parse("|1|329|x").is_err();
    s.add("解析器：缺段/坏达成位/坏数字 4/4 拒绝", bad,
          "任何字段缺失或不可解析都不得静默吞掉");

    // 4. 状态机：20 条齐备推进到 Summarized，核销到 Reconciled
    let mut bx = ClosingBox::new();
    let st = fill_standard(&mut bx);
    let sum_ok = st.is_ok() && bx.state == ClosingState::Summarized;
    let rec = bx.reconcile(BUDGET_LINES);
    s.add("状态机：20 条录入→Summarized→核销通过", sum_ok && rec == Ok(BUDGET_LINES),
          "条目齐备且行数和等于预算 6,580 才可核销");

    // 5. 状态机：密封后封账
    let sealed = bx.seal();
    s.add("状态机：核销后密封成功且封账只读", sealed == Ok(BUDGET_LINES)
        && bx.state == ClosingState::Sealed,
          "密封返回行数合计并进入只读态");

    // 6. 密封后再录入拒绝
    let rewrite = bx.enter(&SummaryEntry { judge: 1, passed: true, lines: LINES_PER_ENTRY })
        == Err(ClosingCode::FrozenSeal);
    s.add("状态机：Sealed 态再录入被拒", rewrite,
          "封账后任何改写都显性拒绝，走勘误通道");

    // 7. 虚报达成检出
    let mut fake = ClosingBox::new();
    let fake_caught = fake.enter(&SummaryEntry { judge: 1, passed: true, lines: 0 })
        == Err(ClosingCode::FakeClaim);
    s.add("边界：达成位=1 行数=0 虚报被拒", fake_caught,
          "达成必须有行数支撑，杜绝零行虚报");

    // 8. 预算偏离检出 + 越段核销拦截
    let mut dev = ClosingBox::new();
    let _ = fill_standard(&mut dev);
    let dev_caught = dev.reconcile(BUDGET_LINES + 1).is_err();
    let seg_caught = recon_seg_ok(SEG_END + 0x40).is_err() && recon_seg_ok(SEG_BASE).is_ok();
    s.add("边界：预算偏离与越段核销双双拦截", dev_caught && seg_caught,
          "实计≠预算或核销码出段窗都不得放行");

    // 9. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_closing_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "缺条目/虚报/预算偏离/重复/密封后改写全被显式拒绝");

    // 10. 注入账守恒：逃逸=0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == 5 * INJECT_ROUNDS,
          "50 次注入必须 50 次检出、0 逃逸");

    // 11. 文案闭集：非空
    let text_ok = ClosingCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 12. 诊断码唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < ClosingCode::ALL.len() {
        let mut j = i + 1;
        while j < ClosingCode::ALL.len() {
            if let (Some(x), Some(y)) = (ClosingCode::ALL.get(i), ClosingCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 13. 码值落在段窗口
    let in_seg = ClosingCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：全部落段内 +0x01..+0x0C", in_seg,
          "码值不得越过闭集窗口");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(EPERM)
        && agrees_with_linux(EACCES)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(EOVERFLOW)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 15. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 16. 节奏口径成文落账：三档非空且两两不同（口径歧义 10/10 检出）
    let probe = probe_clause_ambiguity(INJECT_ROUNDS);
    s.add("节奏口径：三档成文落账且歧义 20/20 检出",
          rhythm_clauses_consistent() && probe == 2 * INJECT_ROUNDS,
          "日/批/波三档逐档成文；空文与同文每轮都被检出");

    // 17. 解析账闭合
    let mut p2 = SummaryParser::default();
    let _ = p2.parse("1|1|329|pass");
    let _ = p2.parse("bad");
    let _ = p2.parse("2|1|329|pass");
    s.add("解析账闭合：解析+拒绝=提交总数",
          p2.parsed.saturating_add(p2.rejected) == 3,
          "每行提交必须入两账之一，无第三去向");

    // 动态明细（≤8）：注入账一行 + 指纹一行
    let ledger_text = format!("注入 {} / 检出 {} / 逃逸 {}", ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_accepts_wellformed() {
        let mut p = SummaryParser::default();
        let r = p.parse("11|1|329|pass");
        assert!(r.is_ok());
        assert_eq!(r.unwrap_or(SummaryEntry { judge: 0, passed: false, lines: 0 }).judge, 11);
    }

    #[test]
    fn parser_rejects_malformed() {
        let mut p = SummaryParser::default();
        assert!(p.parse("7|1").is_err());
        assert!(p.parse("7|9|329|x").is_err());
        assert!(p.parse("x|1|329|x").is_err());
    }

    #[test]
    fn fake_claim_rejected() {
        let mut bx = ClosingBox::new();
        assert_eq!(bx.enter(&SummaryEntry { judge: 1, passed: true, lines: 0 }),
                   Err(ClosingCode::FakeClaim));
    }

    #[test]
    fn duplicate_entry_rejected() {
        let mut bx = ClosingBox::new();
        let e = SummaryEntry { judge: 2, passed: true, lines: LINES_PER_ENTRY };
        assert!(bx.enter(&e).is_ok());
        assert_eq!(bx.enter(&e), Err(ClosingCode::DuplicateEntry));
    }

    #[test]
    fn reconcile_requires_full_entries() {
        let mut bx = ClosingBox::new();
        assert_eq!(bx.reconcile(BUDGET_LINES), Err(ClosingCode::MissingEntry));
    }

    #[test]
    fn seal_requires_reconciled() {
        let mut bx = ClosingBox::new();
        assert_eq!(bx.seal(), Err(ClosingCode::StateRegress));
    }

    #[test]
    fn frozen_box_is_readonly() {
        let mut bx = ClosingBox::new();
        let _ = fill_standard(&mut bx);
        let _ = bx.reconcile(BUDGET_LINES);
        assert!(bx.seal().is_ok());
        assert_eq!(bx.enter(&SummaryEntry { judge: 3, passed: true, lines: 1 }),
                   Err(ClosingCode::FrozenSeal));
    }

    #[test]
    fn out_of_seg_recon_rejected() {
        assert!(recon_seg_ok(SEG_BASE).is_ok());
        assert_eq!(recon_seg_ok(SEG_END), Err(ClosingCode::OutOfSegRecon));
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_closing_injection();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 5 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn rhythm_clauses_are_distinct() {
        assert!(rhythm_clauses_consistent());
        assert!(clause_ambiguous("", "x"));
        assert!(clause_ambiguous(RhythmTier::Daily.written_clause(),
                                 RhythmTier::Daily.written_clause()));
        assert!(!clause_ambiguous(RhythmTier::Daily.written_clause(),
                                  RhythmTier::PerWave.written_clause()));
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EOVERFLOW));
        assert!(agrees_with_linux(EACCES));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EINVAL), -EINVAL);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10040_checks_green() {
        let s = run_unx10040_checks();
        assert!(s.all_passed(), "UNX-F10040 red: {}", s.red_items_text());
    }
}
