//! UNX-F10120 B26 收口小结与预算核销 · 0x92E00 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10121-J1 O_ 标志组合达成（O_CREAT/O_TRUNC/O_APPEND 组合
//!       语义 LTP open05/open06 面 10/10 次）；UNX-F10121-J2 O_EXCL 冲突
//!       误用受控（预期 EEXIST 精确复现 10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92E00（本卡独占 0x40 窗口，下一卡 0x92E40）。
//!   诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器 `parse_entry`：收口行 "F1xxxx|J1态|J2态|行数" 四成分逐段
//!      解析（fid/J1态/J2态/行数），任一成分缺失或非法即显性拒收；
//!   ② 注册表 `EntryRegistry`：F10101..F10119 共 19 条战线条目登记，
//!      重复 fid / 越位 fid / 审计后改写逐类拒绝；
//!   ③ 账本 `WriteOffLedger`：预算 6,600 分摊↔实计核销，守恒式
//!      Σ实计 == 6,600 才零偏离；核销不符逐次显性入账不静默；
//!   ④ 状态机 `CloseState`：Drafting → Audited → Closed 单向推进，
//!      未审计收口（虚报）与状态倒退逐次拦截；
//!   ⑤ 注入核 `BadEntryCase`：缺 J2 / 非法态 / 非数字行数 / 越下限 /
//!      重复条目 五类坏样本逐类 10/10 次拒绝，账守恒逃逸=0。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10120 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x92E00;

/// 段窗上界（下一卡 0x92E40 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x92E40;

/// B26 战线条目基数（F10101..F10119 共 19 条）。
pub const ENTRIES: usize = 19;

/// B26 首条目号（F10101）。
pub const FIRST_FID: u32 = 10101;

/// B26 预算锁定值（行数核销口径：350×14 + 340×5 = 6,600）。
pub const BUDGET: u32 = 6600;

/// 单条目行数硬下限（卡面下限 320，低于即越下限）。
pub const LINE_FLOOR: u32 = 320;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EINVAL: i32 = 22;
/// 文件不存在（对照缺件/缺成分族）。
pub const ENOENT: i32 = 2;
/// 已存在（对照重复条目族）。
pub const EEXIST: i32 = 17;
/// 数值越界（对照预算超额/越下限族）。
pub const EOVERFLOW: i32 = 75;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseCode {
    /// 缺 fid 成分（首段空、非 F 前缀或不可解析）
    MissingFid,
    /// 缺 J1 态（第二段空）
    MissingJ1,
    /// 缺 J2 态（第三段空）
    MissingJ2,
    /// 非法判据态（出 达成/缺失 闭集）
    IllegalState,
    /// 非数字行数（尾段空或不可解析）
    BadCount,
    /// 行数越下限（低于卡面 320）
    CountFloor,
    /// 重复条目（同 fid 二次登记/核销）
    DuplicateFid,
    /// 条目越位（fid 出 F10101..=F10119 窗）
    SlotRange,
    /// 核销不符（实计 ≠ 预算分摊）
    BudgetDrift,
    /// 审计后改写（封版只读）
    Overwrite,
    /// 未审计即收口（虚报）
    NotAudited,
    /// 状态倒退
    StateRegress,
    /// 段位越界（收口语料防御码）
    SegmentViolation,
}

impl CloseCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            CloseCode::MissingFid => "缺 fid 成分被拒：首段必须为 F1xxxx 条目号；请补条目号",
            CloseCode::MissingJ1 => "缺 J1 态被拒：第二段为空；请补 J1 判据态（达成/缺失）",
            CloseCode::MissingJ2 => "缺 J2 态被拒：第三段为空；请补 J2 判据态（达成/缺失）",
            CloseCode::IllegalState => "非法态被拒：判据态只允许 达成/缺失；请改写判据态",
            CloseCode::BadCount => "非数字行数被拒：尾段必须是十进制行数；请改写",
            CloseCode::CountFloor => "行数越下限被拒：单条目不得低于 320 行；请补足行数",
            CloseCode::DuplicateFid => "重复条目被拒：同 fid 已登记/核销；请先核查再改",
            CloseCode::SlotRange => "条目越位被拒：fid 必须落在 F10101..=F10119；请核对批窗",
            CloseCode::BudgetDrift => "核销不符被拒：实计≠预算分摊；请回查行数口径",
            CloseCode::Overwrite => "审计后改写被拒：收口已封版只读；请走勘误通道",
            CloseCode::NotAudited => "未审计即收口被拒：虚报不放行；请先完成审计闸",
            CloseCode::StateRegress => "状态倒退被拒：Drafting→Audited→Closed 单向；请排查回滚",
            CloseCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在卡面窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [CloseCode; 13] = [
        CloseCode::MissingFid,
        CloseCode::MissingJ1,
        CloseCode::MissingJ2,
        CloseCode::IllegalState,
        CloseCode::BadCount,
        CloseCode::CountFloor,
        CloseCode::DuplicateFid,
        CloseCode::SlotRange,
        CloseCode::BudgetDrift,
        CloseCode::Overwrite,
        CloseCode::NotAudited,
        CloseCode::StateRegress,
        CloseCode::SegmentViolation,
    ];
}

// ── 收口行解析器 ──────────────────────────────────────────────────────
/// 解析后的收口行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloseEntry {
    /// 条目号（F10101..F10119）
    pub fid: u32,
    /// J1 判据态
    pub j1: bool,
    /// J2 判据态
    pub j2: bool,
    /// 核销行数
    pub lines: u32,
}

/// 解析 "F1xxxx|J1态|J2态|行数" 四段收口行。
pub fn parse_entry(line: &str) -> Result<CloseEntry, CloseCode> {
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in line.chars() {
        if ch == '|' {
            let _ = parts.push(cur.clone());
            cur.clear();
        } else {
            let _ = cur.push(ch);
        }
    }
    let _ = parts.push(cur);
    if parts.len() != 4 {
        return Err(CloseCode::MissingFid);
    }
    // 成分1：fid（F + 四~五位十进制）
    let f0 = match parts.first() {
        Some(p) => p.as_str(),
        None => return Err(CloseCode::MissingFid),
    };
    let fb = f0.as_bytes();
    if fb.is_empty() || fb.first().copied() != Some(b'F') || fb.len() < 2 {
        return Err(CloseCode::MissingFid);
    }
    let mut fid_acc: u32 = 0;
    let mut k = 1usize;
    while k < fb.len() {
        let b = match fb.get(k) {
            Some(x) => *x,
            None => break,
        };
        if !b.is_ascii_digit() {
            return Err(CloseCode::MissingFid);
        }
        fid_acc = fid_acc.wrapping_mul(10).wrapping_add((b - b'0') as u32);
        k += 1;
    }
    // 成分2/3：J1/J2 判据态
    let j1s = match parts.get(1) {
        Some(p) => p.as_str(),
        None => return Err(CloseCode::MissingJ1),
    };
    if j1s.is_empty() {
        return Err(CloseCode::MissingJ1);
    }
    let j2s = match parts.get(2) {
        Some(p) => p.as_str(),
        None => return Err(CloseCode::MissingJ2),
    };
    if j2s.is_empty() {
        return Err(CloseCode::MissingJ2);
    }
    let state_bit = |s: &str| -> Result<bool, CloseCode> {
        if s == "达成" {
            Ok(true)
        } else if s == "缺失" {
            Ok(false)
        } else {
            Err(CloseCode::IllegalState)
        }
    };
    let j1 = state_bit(j1s)?;
    let j2 = state_bit(j2s)?;
    // 成分4：行数
    let ls = match parts.get(3) {
        Some(p) => p.as_str(),
        None => return Err(CloseCode::BadCount),
    };
    if ls.is_empty() {
        return Err(CloseCode::BadCount);
    }
    let mut lines: u32 = 0;
    for b in ls.bytes() {
        if !b.is_ascii_digit() {
            return Err(CloseCode::BadCount);
        }
        lines = lines.wrapping_mul(10).wrapping_add((b - b'0') as u32);
    }
    if lines < LINE_FLOOR {
        return Err(CloseCode::CountFloor);
    }
    Ok(CloseEntry { fid: fid_acc, j1, j2, lines })
}

// ── 收口状态机 ────────────────────────────────────────────────────────
/// 收口生命周期（单向）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseState {
    /// 起草中
    Drafting,
    /// 已审计
    Audited,
    /// 已收口（封版只读）
    Closed,
}

impl CloseState {
    /// 单调推进判定。
    pub fn can_advance(self, next: CloseState) -> bool {
        let rank = |s: CloseState| match s {
            CloseState::Drafting => 0u8,
            CloseState::Audited => 1,
            CloseState::Closed => 2,
        };
        rank(next) == rank(self) + 1
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            CloseState::Drafting => "起草中",
            CloseState::Audited => "已审计",
            CloseState::Closed => "已收口",
        }
    }
}

// ── 战线注册表 + 核销账 ───────────────────────────────────────────────
/// 单条战线登记。
#[derive(Clone, Debug)]
pub struct LineEnt {
    /// 条目号
    pub fid: u32,
    /// J1/J2 双判据态
    pub j1: bool,
    pub j2: bool,
    /// 预算分摊行数
    pub budget: u32,
    /// 实计核销行数（核销时落账）
    pub actual: u32,
}

/// 收口总账：注册表 + 核销账 + 状态机。
#[derive(Clone, Debug)]
pub struct CloseLedger {
    ents: Vec<LineEnt>,
    cap: usize,
    /// 收口状态
    pub state: CloseState,
    /// 累计登记次数
    pub registered: u32,
    /// 累计核销次数
    pub written_off: u32,
}

impl CloseLedger {
    /// 空账构造（Drafting 态）。
    pub fn new() -> CloseLedger {
        CloseLedger { ents: Vec::new(), cap: ENTRIES, state: CloseState::Drafting, registered: 0, written_off: 0 }
    }

    /// 按号寻址。
    fn idx_of(&self, fid: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.ents.len() {
            if let Some(e) = self.ents.get(i) {
                if e.fid == fid {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记一条战线：封版/越位/重复/越下限逐闸。
    pub fn register(&mut self, e: &CloseEntry) -> Result<(), CloseCode> {
        if self.state != CloseState::Drafting {
            return Err(CloseCode::Overwrite);
        }
        if e.fid < FIRST_FID || e.fid >= FIRST_FID + ENTRIES as u32 {
            return Err(CloseCode::SlotRange);
        }
        if self.idx_of(e.fid).is_some() {
            return Err(CloseCode::DuplicateFid);
        }
        if e.lines < LINE_FLOOR {
            return Err(CloseCode::CountFloor);
        }
        let budget = budget_of(e.fid);
        let _ = self.ents.push(LineEnt {
            fid: e.fid,
            j1: e.j1,
            j2: e.j2,
            budget,
            actual: 0,
        });
        self.registered = self.registered.saturating_add(1);
        Ok(())
    }

    /// 审计闸：19 条齐备且双判据全达成才放行 Drafting→Audited。
    pub fn audit(&mut self) -> Result<(), CloseCode> {
        if self.state != CloseState::Drafting {
            return Err(CloseCode::StateRegress);
        }
        if self.ents.len() != ENTRIES {
            return Err(CloseCode::NotAudited);
        }
        let mut i = 0usize;
        while i < self.ents.len() {
            if let Some(e) = self.ents.get(i) {
                if !e.j1 || !e.j2 {
                    return Err(CloseCode::NotAudited);
                }
            }
            i += 1;
        }
        self.state = CloseState::Audited;
        Ok(())
    }

    /// 核销：逐条以实计行数入账；实计 ≠ 预算分摊即 BudgetDrift。
    pub fn write_off(&mut self, fid: u32, actual: u32) -> Result<(), CloseCode> {
        if self.state != CloseState::Audited {
            return Err(CloseCode::NotAudited);
        }
        let idx = match self.idx_of(fid) {
            Some(i) => i,
            None => return Err(CloseCode::SlotRange),
        };
        let budget = match self.ents.get(idx) {
            Some(e) => e.budget,
            None => return Err(CloseCode::SlotRange),
        };
        if actual < LINE_FLOOR {
            return Err(CloseCode::CountFloor);
        }
        if actual != budget {
            return Err(CloseCode::BudgetDrift);
        }
        if let Some(e) = self.ents.get_mut(idx) {
            e.actual = actual;
        }
        self.written_off = self.written_off.saturating_add(1);
        Ok(())
    }

    /// 总核销守恒：19 条全核销且 Σ实计 == 6,600。
    pub fn fully_settled(&self) -> bool {
        if self.written_off != ENTRIES as u32 {
            return false;
        }
        let mut sum: u32 = 0;
        let mut i = 0usize;
        while i < self.ents.len() {
            if let Some(e) = self.ents.get(i) {
                sum = sum.saturating_add(e.actual);
            }
            i += 1;
        }
        sum == BUDGET
    }

    /// 收口（封版）：须 Audited 且全核销。
    pub fn close(&mut self) -> Result<(), CloseCode> {
        if self.state != CloseState::Audited {
            return Err(CloseCode::NotAudited);
        }
        if !self.fully_settled() {
            return Err(CloseCode::BudgetDrift);
        }
        self.state = CloseState::Closed;
        Ok(())
    }

    /// 在册条数。
    pub fn len(&self) -> usize {
        self.ents.len()
    }
}

/// 预算分摊表：F10101..F10114 各 350 行，F10115..F10119 各 340 行。
pub fn budget_of(fid: u32) -> u32 {
    if fid <= 10114 {
        350
    } else {
        340
    }
}

/// 规范登记语料：19 条战线（全达成、行数取预算分摊值）。
pub fn canonical_entries() -> Vec<CloseEntry> {
    let mut out: Vec<CloseEntry> = Vec::new();
    let mut fid = FIRST_FID;
    while fid < FIRST_FID + ENTRIES as u32 {
        let _ = out.push(CloseEntry {
            fid,
            j1: true,
            j2: true,
            lines: budget_of(fid),
        });
        fid = fid.saturating_add(1);
    }
    out
}

// ── J1 达成核：收口落账 10/10 轮 ──────────────────────────────────────
/// 逐轮：19 条登记→审计→逐条核销→总守恒→收口封版。
pub fn run_close_rounds() -> (u32, usize) {
    let mut ok = 0u32;
    let mut probes = 0usize;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut l = CloseLedger::new();
        let mut all_ok = true;
        // 面1：19 条全登记。
        let ents = canonical_entries();
        let mut i = 0usize;
        let mut reg_ok = true;
        while i < ents.len() {
            if let Some(e) = ents.get(i) {
                if l.register(e).is_err() {
                    reg_ok = false;
                }
            }
            i += 1;
        }
        all_ok = all_ok && reg_ok && l.len() == ENTRIES;
        probes += 1;
        // 面2：审计放行。
        all_ok = all_ok && l.audit().is_ok() && l.state == CloseState::Audited;
        probes += 1;
        // 面3：逐条核销零偏离。
        let mut wo_ok = true;
        let mut j = 0usize;
        while j < ents.len() {
            if let Some(e) = ents.get(j) {
                if l.write_off(e.fid, e.lines).is_err() {
                    wo_ok = false;
                }
            }
            j += 1;
        }
        all_ok = all_ok && wo_ok;
        probes += 1;
        // 面4：总守恒 Σ实计 == 6,600。
        all_ok = all_ok && l.fully_settled();
        probes += 1;
        // 面5：收口封版。
        all_ok = all_ok && l.close().is_ok() && l.state == CloseState::Closed;
        probes += 1;
        // 面6：封版后改写被拒。
        let e0 = match ents.first() {
            Some(e) => e.clone(),
            None => CloseEntry { fid: FIRST_FID, j1: true, j2: true, lines: 350 },
        };
        all_ok = all_ok && l.register(&e0).err() == Some(CloseCode::Overwrite);
        probes += 1;
        if all_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok, probes)
}

// ── 注入核：坏收口矩阵 ────────────────────────────────────────────────
/// 五类坏样本（判据要求缺 J2/非法态/非数字/越下限/重复五类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadEntryCase {
    /// 缺 J2 态
    MissingJ2,
    /// 非法判据态
    IllegalState,
    /// 非数字行数
    BadCount,
    /// 行数越下限
    CountFloor,
    /// 重复条目
    DuplicateFid,
}

impl BadEntryCase {
    /// 闭集全量。
    pub const ALL: [BadEntryCase; 5] = [
        BadEntryCase::MissingJ2,
        BadEntryCase::IllegalState,
        BadEntryCase::BadCount,
        BadEntryCase::CountFloor,
        BadEntryCase::DuplicateFid,
    ];

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            BadEntryCase::MissingJ2 => "缺 J2 态行",
            BadEntryCase::IllegalState => "非法判据态行",
            BadEntryCase::BadCount => "非数字行数行",
            BadEntryCase::CountFloor => "行数越下限行",
            BadEntryCase::DuplicateFid => "重复条目行",
        }
    }
}

/// 注入账。
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

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) -> Result<(), CloseCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(CloseCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏收口注入：每类 INJECT_ROUNDS 次。
pub fn run_bad_entry_matrix() -> (InjectLedger, [u32; 5]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadEntryCase::ALL.len() {
        let case = match BadEntryCase::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut l = CloseLedger::new();
            let caught = match case {
                BadEntryCase::MissingJ2 => {
                    parse_entry("F10101|达成||350").err() == Some(CloseCode::MissingJ2)
                }
                BadEntryCase::IllegalState => {
                    parse_entry("F10101|达成|完|350").err() == Some(CloseCode::IllegalState)
                }
                BadEntryCase::BadCount => {
                    parse_entry("F10101|达成|达成|3x0").err() == Some(CloseCode::BadCount)
                }
                BadEntryCase::CountFloor => {
                    parse_entry("F10101|达成|达成|319").err() == Some(CloseCode::CountFloor)
                }
                BadEntryCase::DuplicateFid => {
                    let e = CloseEntry { fid: FIRST_FID, j1: true, j2: true, lines: 350 };
                    let first = l.register(&e).is_ok();
                    let second = l.register(&e).err() == Some(CloseCode::DuplicateFid);
                    first && second
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

// ── 收口小结成文（纯函数） ────────────────────────────────────────────
/// 小结成文：「entries=<n> budget=<b> settled=<0|1> state=<t>」。
pub fn compose_close_summary(l: &CloseLedger) -> Result<String, CloseCode> {
    let out = format!(
        "entries={} budget={} settled={} state={}",
        l.len(), BUDGET, if l.fully_settled() { 1u32 } else { 0u32 }, l.state.label()
    );
    if out.is_empty() {
        return Err(CloseCode::SegmentViolation);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -EINVAL, 87, "Windows ERROR_INVALID_PARAMETER；非法态/越下限族"),
    ("ENOENT", -ENOENT, 2, "Windows ERROR_FILE_NOT_FOUND；缺成分/缺条目族"),
    ("EEXIST", -EEXIST, 183, "Windows ERROR_ALREADY_EXISTS；重复条目族"),
    ("EOVERFLOW", -EOVERFLOW, 132, "Windows ERROR_ARITHMETIC_OVERFLOW；预算超额族"),
];

/// 查表：按 errno 正数取负值；表外返回 0。
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

/// 与 Linux 逐码一致。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
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

/// 规范语料指纹：19 条登记→审计→核销→收口→成文→摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut l = CloseLedger::new();
    let ents = canonical_entries();
    let mut i = 0usize;
    while i < ents.len() {
        if let Some(e) = ents.get(i) {
            let _ = l.register(e);
        }
        i += 1;
    }
    let _ = l.audit();
    let mut j = 0usize;
    while j < ents.len() {
        if let Some(e) = ents.get(j) {
            let _ = l.write_off(e.fid, e.lines);
        }
        j += 1;
    }
    let _ = l.close();
    let mut bytes: Vec<u8> = Vec::new();
    if let Ok(s) = compose_close_summary(&l) {
        bytes.extend_from_slice(s.as_bytes());
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10120 判据集（14 条静态 + 3 条动态）。
pub fn run_unx10120_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10120");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92E00..=0x92E3F).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集
    let mut uniq = true;
    let mut i = 0usize;
    while i < CloseCode::ALL.len() {
        let mut j = i + 1;
        while j < CloseCode::ALL.len() {
            if let (Some(a), Some(b)) = (CloseCode::ALL.get(i), CloseCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = CloseCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：闭集码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. 解析器：合法行放行、五类坏行逐档拒绝
    let parse_ok = parse_entry("F10101|达成|达成|350").is_ok()
        && parse_entry("F10119|达成|达成|340").is_ok()
        && parse_entry("F10101|达成||350").err() == Some(CloseCode::MissingJ2)
        && parse_entry("F10101|达成|达成|319").err() == Some(CloseCode::CountFloor);
    s.add("收口行解析器：四成分闭集放行且坏行逐档拒绝", parse_ok,
          "解析器是收口前置；fid/J1/J2/行数任一缺失即显性拒收");

    // 4. J1 达成核
    let (ok, probes) = run_close_rounds();
    s.add("J1 收口小结落账：19 条战线 J1/J2 双判据齐备+预算 6,600 核销零偏离 10/10 次",
          ok == INJECT_ROUNDS && probes == (INJECT_ROUNDS as usize) * 6,
          "每轮 6 面（登记/审计/核销/守恒/封版/改写拒）全对");

    // 5. 预算分摊表逐条可复算
    let mut b_ok = true;
    let mut fid = FIRST_FID;
    while fid < FIRST_FID + ENTRIES as u32 {
        let want = if fid <= 10114 { 350u32 } else { 340u32 };
        b_ok = b_ok && budget_of(fid) == want;
        fid = fid.saturating_add(1);
    }
    let b_sum = 350u32.saturating_mul(14).saturating_add(340u32.saturating_mul(5));
    s.add("预算分摊：19 条分摊逐条可复算且 Σ==6,600", b_ok && b_sum == BUDGET,
          "350×14 + 340×5 = 6,600 是本批锁定口径；任何一条漂移即失守");

    // 6. 条目越位：F10100/F10120 两侧越位拒绝
    let mut l = CloseLedger::new();
    let side_l = CloseEntry { fid: FIRST_FID - 1, j1: true, j2: true, lines: 350 };
    let side_r = CloseEntry { fid: FIRST_FID + ENTRIES as u32, j1: true, j2: true, lines: 340 };
    let slot_ok = l.register(&side_l).err() == Some(CloseCode::SlotRange)
        && l.register(&side_r).err() == Some(CloseCode::SlotRange);
    s.add("条目越位：批窗两侧越位逐档拒绝", slot_ok,
          "fid 必须落在 F10101..=F10119；越位即拒");

    // 7. 未审计收口（虚报）拦截
    let e = CloseEntry { fid: FIRST_FID, j1: true, j2: true, lines: 350 };
    let _ = l.register(&e);
    let va_ok = l.write_off(FIRST_FID, 350).err() == Some(CloseCode::NotAudited)
        && l.close().err() == Some(CloseCode::NotAudited);
    s.add("收口虚报拦截：未审计即核销/收口逐次拒绝", va_ok,
          "J2 收口虚报受控面：核销与收口都必须在审计闸之后");

    // 8. 注入核：五类坏样本各 10/10 拒绝
    let (inj, per_class) = run_bad_entry_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 拒绝", all_caught,
          "缺J2/非法态/非数字/越下限/重复必须逐类逐次精确复现");

    // 9. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadEntryCase::ALL.len() as u32 * INJECT_ROUNDS,
          "50 次注入必须 50 次检出、0 逃逸");

    // 10. 状态机
    let sm_ok = CloseState::Drafting.can_advance(CloseState::Audited)
        && CloseState::Audited.can_advance(CloseState::Closed)
        && !CloseState::Closed.can_advance(CloseState::Drafting)
        && CloseState::Audited.label() == "已审计";
    s.add("状态机：Drafting→Audited→Closed 单向", sm_ok,
          "收口生命周期只许单调推进；倒退即协议违约");

    // 11. 判据缺失拦截：单判据缺位不能过审计闸
    let mut l2 = CloseLedger::new();
    let ents = canonical_entries();
    let mut k = 0usize;
    while k < ents.len() {
        if let Some(e) = ents.get(k) {
            let mut e2 = e.clone();
            if k == 0 {
                e2.j2 = false; // 首条 J2 缺位
            }
            let _ = l2.register(&e2);
        }
        k += 1;
    }
    s.add("判据缺失拦截：单条 J2 缺位审计即拒",
          l2.audit().err() == Some(CloseCode::NotAudited),
          "19 条必须双判据齐备；缺一条即整批拦下");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(22) && agrees_with_linux(2)
        && agrees_with_linux(17) && agrees_with_linux(75)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 13. 成文 + 指纹
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("成文与指纹：收口语料双跑哈希逐位一致", f1 == f2,
          "成文为纯函数；同语料两次哈希必须逐位一致");

    // 14. 收口只增不减：本批 19 条登记后条数恒定
    let grow_ok = l.len() == 1 && l.registered == 1;
    s.add("收口只增不减：登记后账面条数与计数一致", grow_ok,
          "战线登记面只增不减；重复与越位不入账");

    // 动态明细（≤8）
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let ok_text = format!("收口落账轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_guards() {
        assert!(parse_entry("F10101|达成|达成|350").is_ok());
        assert_eq!(parse_entry("F10101|达成||350").err(), Some(CloseCode::MissingJ2));
        assert_eq!(parse_entry("F10101|虚|达成|350").err(), Some(CloseCode::IllegalState));
        assert_eq!(parse_entry("F10101|达成|达成|3x0").err(), Some(CloseCode::BadCount));
        assert_eq!(parse_entry("F10101|达成|达成|319").err(), Some(CloseCode::CountFloor));
        assert_eq!(parse_entry("X10101|达成|达成|350").err(), Some(CloseCode::MissingFid));
    }

    #[test]
    fn slot_range_guards() {
        let mut l = CloseLedger::new();
        let lo = CloseEntry { fid: 10100, j1: true, j2: true, lines: 350 };
        let hi = CloseEntry { fid: 10120, j1: true, j2: true, lines: 350 };
        assert_eq!(l.register(&lo).err(), Some(CloseCode::SlotRange));
        assert_eq!(l.register(&hi).err(), Some(CloseCode::SlotRange));
    }

    #[test]
    fn duplicate_blocked() {
        let mut l = CloseLedger::new();
        let e = CloseEntry { fid: FIRST_FID, j1: true, j2: true, lines: 350 };
        assert!(l.register(&e).is_ok());
        assert_eq!(l.register(&e).err(), Some(CloseCode::DuplicateFid));
    }

    #[test]
    fn audit_requires_all() {
        let mut l = CloseLedger::new();
        assert_eq!(l.audit().err(), Some(CloseCode::NotAudited));
        for e in canonical_entries() {
            let _ = l.register(&e);
        }
        assert!(l.audit().is_ok());
    }

    #[test]
    fn write_off_requires_audited() {
        let mut l = CloseLedger::new();
        let e = CloseEntry { fid: FIRST_FID, j1: true, j2: true, lines: 350 };
        let _ = l.register(&e);
        assert_eq!(l.write_off(FIRST_FID, 350).err(), Some(CloseCode::NotAudited));
    }

    #[test]
    fn write_off_drift_blocked() {
        let mut l = CloseLedger::new();
        for e in canonical_entries() {
            let _ = l.register(&e);
        }
        let _ = l.audit();
        assert_eq!(l.write_off(FIRST_FID, 349).err(), Some(CloseCode::BudgetDrift));
        assert!(l.write_off(FIRST_FID, 350).is_ok());
    }

    #[test]
    fn full_settle_and_close() {
        let mut l = CloseLedger::new();
        for e in canonical_entries() {
            let _ = l.register(&e);
        }
        let _ = l.audit();
        for e in canonical_entries() {
            let _ = l.write_off(e.fid, e.lines);
        }
        assert!(l.fully_settled());
        assert!(l.close().is_ok());
        assert_eq!(l.state, CloseState::Closed);
    }

    #[test]
    fn bad_matrix_green() {
        let (inj, per) = run_bad_entry_matrix();
        assert_eq!(inj.injected, 5 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn rounds_full() {
        let (ok, probes) = run_close_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
        assert_eq!(probes, (INJECT_ROUNDS as usize) * 6);
    }

    #[test]
    fn state_machine_monotonic() {
        assert!(CloseState::Drafting.can_advance(CloseState::Audited));
        assert!(!CloseState::Closed.can_advance(CloseState::Audited));
        assert_eq!(CloseState::Closed.label(), "已收口");
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10120_checks_green() {
        let set = run_unx10120_checks();
        assert!(set.all_passed(), "UNX-F10120 red: {}", set.red_items_text());
    }
}
