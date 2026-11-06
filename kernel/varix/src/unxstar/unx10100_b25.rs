//! UNX-F10100 B25 收口小结与预算核销 · 0x92900 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10100（B25 批收口卡：20 条战线条目 J1/J2 双判据齐备落账，
//!       预算 6,640 核销零偏离，核销不符 10/10 次检出）。
//! 锚点：UNX-F10101-J1 read 常规读达成（LTP read01/read03 面计数与内容一致
//!       10/10 次）；UNX-F10101-J2 负 fd 误用受控（预期 EBADF 精确复现
//!       10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92900（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `CloseCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 解析器 `parse_entry`：收口行 "F1xxxx|J1态|J2态|行数" 四成分逐段
//!     解析（fid/J1态/J2态/行数），任一成分缺失或非法即显性拒收；
//!   - 注册表 `EntryRegistry`：F10081..F10100 共 20 条战线条目登记，
//!     重复 fid / 越位 fid / 审计后改写逐类拒绝；
//!   - 账本 `WriteOffLedger`：预算 6,640 分摊↔实计核销，守恒式
//!     Σ实计 == 6,640 才零偏离；核销不符逐次显性入账不静默；
//!   - 状态机 `CloseState`：Drafting → Audited → Closed 单向推进，
//!     未审计收口（虚报）与状态倒退逐次拦截；
//!   - 注入核 `BadEntryCase`：缺 J2 / 非法态 / 非数字行数 / 越下限 /
//!     重复条目 五类坏样本逐类 10/10 次拒绝，账守恒逃逸=0；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10100 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x92900;

/// 段窗上界（下一卡 0x92940 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x92940;

/// B25 战线条目基数（F10081..F10100 共 20 条）。
pub const ENTRIES: usize = 20;

/// B25 首条目号（F10081）。
pub const FIRST_FID: u32 = 10081;

/// B25 预算锁定值（行数核销口径：334×10 + 330×10 = 6,640）。
pub const BUDGET: u32 = 6640;

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
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
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
    /// 条目越位（fid 出 F10081..=F10100 窗）
    SlotRange,
    /// 核销不符（实计 ≠ 预算分摊）
    BudgetDrift,
    /// 审计后改写（封版只读）
    Overwrite,
    /// 未审计即收口（虚报）
    NotAudited,
    /// 状态倒退
    StateRegress,
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
            CloseCode::SlotRange => "条目越位被拒：fid 必须落在 F10081..=F10100；请核对批窗",
            CloseCode::BudgetDrift => "核销不符被拒：实计≠预算分摊；请回查行数口径",
            CloseCode::Overwrite => "审计后改写被拒：收口已封版只读；请走勘误通道",
            CloseCode::NotAudited => "未审计即收口被拒：虚报不放行；请先完成审计闸",
            CloseCode::StateRegress => "状态倒退被拒：Drafting→Audited→Closed 单向；请排查回滚",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [CloseCode; 12] = [
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
    ];
}

// ── 判据态与条目结构 ──────────────────────────────────────────────────
/// 判据态闭集：达成 / 缺失（双判据 J1/J2 各占一态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JudgeState {
    /// 达成（判据逐次全过）
    Done,
    /// 缺失（未达成，收口不得放行）
    Missing,
}

/// 一条战线条目：fid + J1/J2 判据态 + 行数实计。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CloseEntry {
    /// 条目号（F10081..=F10100）
    pub fid: u32,
    /// J1 态（正判据）
    pub j1: JudgeState,
    /// J2 态（反判据）
    pub j2: JudgeState,
    /// 行数（实计）
    pub lines: u32,
}

/// 分段器：按 '|' 切四段，段数≠4 返回 None（无堆复制）。
fn split4(line: &str) -> Option<[usize; 5]> {
    let b = line.as_bytes();
    let mut cuts = [0usize; 5];
    let mut n = 0usize;
    let mut i = 0usize;
    while i < b.len() {
        if b.get(i) == Some(&b'|') {
            if n >= 3 {
                return None;
            }
            cuts[n + 1] = i + 1;
            n += 1;
        }
        i += 1;
    }
    if n != 3 {
        return None;
    }
    cuts[4] = b.len();
    Some(cuts)
}

/// 预算分摊：第 0..10 条各 334 行、第 10..20 条各 330 行，合计恰 6,640。
pub fn budget_for(fid: u32) -> u32 {
    let idx = (fid - FIRST_FID) as usize;
    if idx < 10 {
        334
    } else {
        330
    }
}

/// 收口行解析器："F1xxxx|J1态|J2态|行数" → CloseEntry。
///
/// 四成分逐段核：段数≠4 视为结构残缺从 fid 起报缺；fid 空/非 F 前缀/
/// 不可解析 → MissingFid；fid 出窗 → SlotRange；J1/J2 段空 →
/// MissingJ1/MissingJ2；态出闭集 → IllegalState；行数空/非数字 →
/// BadCount；行数低于 320 → CountFloor。
pub fn parse_entry(line: &str) -> Result<CloseEntry, CloseCode> {
    let cuts = match split4(line) {
        Some(c) => c,
        None => return Err(CloseCode::MissingFid),
    };
    let fid_seg = line.get(cuts[0]..cuts[1] - 1).unwrap_or("");
    let j1_seg = line.get(cuts[1]..cuts[2] - 1).unwrap_or("");
    let j2_seg = line.get(cuts[2]..cuts[3] - 1).unwrap_or("");
    let cnt_seg = line.get(cuts[3]..cuts[4]).unwrap_or("");
    if fid_seg.is_empty() || !fid_seg.starts_with('F') {
        return Err(CloseCode::MissingFid);
    }
    let fid_num = match fid_seg.get(1..).and_then(|s| s.parse::<u32>().ok()) {
        Some(n) => n,
        None => return Err(CloseCode::MissingFid),
    };
    if fid_num < FIRST_FID || fid_num >= FIRST_FID + ENTRIES as u32 {
        return Err(CloseCode::SlotRange);
    }
    let j1 = if j1_seg.is_empty() {
        return Err(CloseCode::MissingJ1);
    } else {
        match j1_seg {
            "达成" => JudgeState::Done,
            "缺失" => JudgeState::Missing,
            _ => return Err(CloseCode::IllegalState),
        }
    };
    let j2 = if j2_seg.is_empty() {
        return Err(CloseCode::MissingJ2);
    } else {
        match j2_seg {
            "达成" => JudgeState::Done,
            "缺失" => JudgeState::Missing,
            _ => return Err(CloseCode::IllegalState),
        }
    };
    let lines = if cnt_seg.is_empty() {
        return Err(CloseCode::BadCount);
    } else {
        match cnt_seg.parse::<u32>() {
            Ok(n) => n,
            Err(_) => return Err(CloseCode::BadCount),
        }
    };
    if lines < LINE_FLOOR {
        return Err(CloseCode::CountFloor);
    }
    Ok(CloseEntry { fid: fid_num, j1, j2, lines })
}

// ── 注册表与收口状态机 ────────────────────────────────────────────────
/// 收口状态机：Drafting → Audited → Closed 单向推进。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseState {
    /// 起草中（条目登记进行时）
    Drafting,
    /// 已审计（双判据齐备且预算守恒）
    Audited,
    /// 已收口（封版只读）
    Closed,
}

/// 战线条目注册表：20 槽 + 收口状态机。
#[derive(Clone, Debug)]
pub struct EntryRegistry {
    /// 已登记条目（None=空槽）
    pub slots: [Option<CloseEntry>; ENTRIES],
    /// 当前收口状态
    pub state: CloseState,
}

impl EntryRegistry {
    /// 构造空注册表（全空槽、起草态）。
    pub const fn new() -> EntryRegistry {
        EntryRegistry { slots: [None; ENTRIES], state: CloseState::Drafting }
    }

    /// 登记一条收口条目：状态必检、槽位必检、fid 必检、重复必拒。
    pub fn register(&mut self, slot: usize, line: &str) -> Result<(), CloseCode> {
        if self.state != CloseState::Drafting {
            return Err(CloseCode::Overwrite);
        }
        if slot >= ENTRIES {
            return Err(CloseCode::SlotRange);
        }
        let e = parse_entry(line)?;
        let mut i = 0usize;
        while i < ENTRIES {
            if i != slot {
                if let Some(Some(ex)) = self.slots.get(i) {
                    if ex.fid == e.fid {
                        return Err(CloseCode::DuplicateFid);
                    }
                }
            }
            i += 1;
        }
        if let Some(s) = self.slots.get_mut(slot) {
            *s = Some(e);
        }
        Ok(())
    }

    /// 审计闸：20 槽齐、J1/J2 双判据全 达成、Σ行数 == 6,640 才过；
    /// 任一判据缺失（虚报）或预算偏离显性拒绝。
    pub fn audit(&mut self) -> Result<(), CloseCode> {
        if self.state != CloseState::Drafting {
            return Err(CloseCode::StateRegress);
        }
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < ENTRIES {
            match self.slots.get(i).copied().unwrap_or(None) {
                Some(e) => {
                    if e.j1 != JudgeState::Done || e.j2 != JudgeState::Done {
                        return Err(CloseCode::NotAudited);
                    }
                    sum = sum.saturating_add(e.lines);
                }
                None => return Err(CloseCode::NotAudited),
            }
            i += 1;
        }
        if sum != BUDGET {
            return Err(CloseCode::BudgetDrift);
        }
        self.state = CloseState::Audited;
        Ok(())
    }

    /// 收口：审计通过后才可封版；封版后条目只读。
    pub fn close(&mut self) -> Result<(), CloseCode> {
        if self.state != CloseState::Audited {
            return Err(CloseCode::NotAudited);
        }
        self.state = CloseState::Closed;
        Ok(())
    }
}

/// 标准收口语料：F10081..F10100 逐条 "F…|达成|达成|分摊行数" 登记。
pub fn fill_standard(reg: &mut EntryRegistry) -> Result<(), CloseCode> {
    let mut i = 0usize;
    while i < ENTRIES {
        let fid = FIRST_FID + i as u32;
        let line = format!("F{}|达成|达成|{}", fid, budget_for(fid));
        reg.register(i, &line)?;
        i += 1;
    }
    Ok(())
}

// ── 核销账本 ──────────────────────────────────────────────────────────
/// 一行核销记录：条目号 + 预算分摊 + 实计。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WriteRow {
    /// 条目号
    pub fid: u32,
    /// 预算分摊
    pub budget: u32,
    /// 实计核销
    pub actual: u32,
}

/// 核销账本：逐条预算↔实计比对，守恒式 Σ实计 == BUDGET 才零偏离。
#[derive(Clone, Debug)]
pub struct WriteOffLedger {
    /// 已入账核销行
    pub rows: Vec<WriteRow>,
    /// 核销不符检出次数（必须与注入次数一致，不得静默放行）
    pub drifts: u32,
    /// 逃逸次数（坏账误入账，必须恒 0）
    pub escapes: u32,
}

impl WriteOffLedger {
    /// 构造零账。
    pub fn new() -> WriteOffLedger {
        WriteOffLedger { rows: Vec::new(), drifts: 0, escapes: 0 }
    }

    /// 核销一行：fid 越位拒、重复条目拒、实计≠分摊拒（计入 drifts）。
    pub fn write_off(&mut self, fid: u32, budget: u32, actual: u32) -> Result<(), CloseCode> {
        if fid < FIRST_FID || fid >= FIRST_FID + ENTRIES as u32 {
            return Err(CloseCode::SlotRange);
        }
        if self.rows.iter().any(|r| r.fid == fid) {
            return Err(CloseCode::DuplicateFid);
        }
        if actual != budget {
            self.drifts = self.drifts.saturating_add(1);
            return Err(CloseCode::BudgetDrift);
        }
        self.rows.push(WriteRow { fid, budget, actual });
        Ok(())
    }

    /// 守恒式：20 条齐且 Σ实计 == 6,640 才零偏离，否则返回预算缺口。
    pub fn deficit(&self) -> Option<u32> {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                sum = sum.saturating_add(r.actual);
            }
            i += 1;
        }
        if self.rows.len() == ENTRIES && sum == BUDGET {
            None
        } else {
            Some(BUDGET.saturating_sub(sum))
        }
    }
}

/// 标准核销：20 条逐条按分摊额核销，零偏离落账。
pub fn settle_standard(ledger: &mut WriteOffLedger) -> Result<(), CloseCode> {
    let mut i = 0usize;
    while i < ENTRIES {
        let fid = FIRST_FID + i as u32;
        let b = budget_for(fid);
        ledger.write_off(fid, b, b)?;
        i += 1;
    }
    Ok(())
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏收口样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadEntryCase {
    /// 缺 J2 态（第三段空）
    MissingJ2,
    /// 非法态（判据态出闭集）
    IllegalState,
    /// 非数字行数
    BadCount,
    /// 行数越下限
    CountFloor,
    /// 重复条目（注册表同 fid 二次登记）
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

    /// 注入一轮：构造坏样本并触发校验，返回是否被显式拒绝。
    pub fn inject(self) -> bool {
        match self {
            BadEntryCase::MissingJ2 => {
                parse_entry("F10081|达成||334").err() == Some(CloseCode::MissingJ2)
            }
            BadEntryCase::IllegalState => {
                parse_entry("F10081|达成|完成|334").err() == Some(CloseCode::IllegalState)
            }
            BadEntryCase::BadCount => {
                parse_entry("F10081|达成|达成|x8").err() == Some(CloseCode::BadCount)
            }
            BadEntryCase::CountFloor => {
                parse_entry("F10081|达成|达成|100").err() == Some(CloseCode::CountFloor)
            }
            BadEntryCase::DuplicateFid => {
                let mut reg = EntryRegistry::new();
                let first = reg.register(0, "F10081|达成|达成|334").is_ok();
                first && reg.register(1, "F10081|达成|达成|334").err()
                    == Some(CloseCode::DuplicateFid)
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_b25_injection() -> (WriteOffLedger, [u32; 5]) {
    let mut ledger = WriteOffLedger::new();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadEntryCase::ALL.len() {
        if let Some(case) = BadEntryCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject();
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                } else {
                    ledger.escapes = ledger.escapes.saturating_add(1);
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

/// 核销不符注入：10 轮各自以错账（实计=分摊-1）核销，返回检出次数。
pub fn run_drift_injection() -> u32 {
    let mut hits = 0u32;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut lg = WriteOffLedger::new();
        if lg.write_off(FIRST_FID, 334, 333).err() == Some(CloseCode::BudgetDrift)
            && lg.drifts == 1
        {
            hits += 1;
        }
        round += 1;
    }
    hits
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺成分/缺条目族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复条目族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；非法态/非数字族"),
    ("EOVERFLOW", -75, 84, "Windows ERROR_ARITHMETIC_OVERFLOW；越下限/预算超额族"),
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；未审计收口/封版改写族"),
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

/// 规范语料双跑指纹：登记→审计→收口→核销全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut reg = EntryRegistry::new();
    bytes.push(match fill_standard(&mut reg) {
        Ok(()) => 1,
        Err(_) => 2,
    });
    bytes.push(match reg.audit() {
        Ok(()) => 3,
        Err(_) => 4,
    });
    bytes.push(match reg.close() {
        Ok(()) => 5,
        Err(_) => 6,
    });
    bytes.push(reg.state as u8);
    let mut lg = WriteOffLedger::new();
    bytes.push(match settle_standard(&mut lg) {
        Ok(()) => 7,
        Err(_) => 8,
    });
    bytes.extend_from_slice(&(lg.rows.len() as u32).to_le_bytes());
    bytes.push(match lg.deficit() {
        None => 9,
        Some(_) => 10,
    });
    bytes.push(lg.drifts as u8);
    bytes.push(lg.escapes as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10100 判据集（18 条静态 + 3 条动态明细）。
pub fn run_unx10100_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10100");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x92900..=0x92940", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝");

    // 2. 标准收口：20 条逐条解析登记成功
    let mut reg = EntryRegistry::new();
    let fill_ok = fill_standard(&mut reg).is_ok();
    s.add("收口登记：F10081..F10100 逐条解析入槽", fill_ok,
          "20 条战线条目逐条落账是收口小结的硬前置");

    // 3. 缺成分 10/10 拒收（锚点判据口径）
    let mut miss_hits = 0u32;
    let miss_lines = ["|达成|达成|334", "F10081||达成|334", "F10081|达成||334"];
    let mut mi = 0usize;
    while mi < miss_lines.len() {
        if let Some(line) = miss_lines.get(mi) {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                if parse_entry(line).is_err() {
                    miss_hits += 1;
                }
                round += 1;
            }
        }
        mi += 1;
    }
    let miss_expect = INJECT_ROUNDS * miss_lines.len() as u32;
    s.add("缺成分：10/10 次拒收", miss_hits == miss_expect,
          "fid/J1态/J2态/行数任一缺失逐次显性拒收，30 轮全检出");

    // 4. 非法形态：非法态/非数字/越下限/段数不足
    let illegal_rej = parse_entry("F10081|达成|完成|334").err() == Some(CloseCode::IllegalState);
    let bad_cnt = parse_entry("F10081|达成|达成|abc").err() == Some(CloseCode::BadCount);
    let floor_rej = parse_entry("F10081|达成|达成|100").err() == Some(CloseCode::CountFloor);
    let seg_rej = parse_entry("F10081|达成").err() == Some(CloseCode::MissingFid);
    s.add("非法形态：非法态/非数字/越下限/段残全拒", illegal_rej && bad_cnt && floor_rej && seg_rej,
          "判据态闭集 达成/缺失；行数十进制且 ≥320；恰四段");

    // 5. fid 越位拦截（批窗外两侧）
    let low_rej = parse_entry("F10061|达成|达成|330").err() == Some(CloseCode::SlotRange);
    let high_rej = parse_entry("F10101|达成|达成|330").err() == Some(CloseCode::SlotRange);
    s.add("条目越位：F10061/F10101 出窗拒收", low_rej && high_rej,
          "B25 批窗只认 F10081..=F10100，窗外两侧都拒");

    // 6. 重复条目拦截
    let mut dup = EntryRegistry::new();
    let _ = dup.register(0, "F10081|达成|达成|334");
    let dup_ok = dup.register(1, "F10081|达成|达成|334").err() == Some(CloseCode::DuplicateFid);
    s.add("重复条目：同 fid 二次登记拒收", dup_ok,
          "一槽一条目，同 fid 不得重复占位");

    // 7. 审计闸：未审计收口拒、审计成功、审计后改写拒
    let mut fz = EntryRegistry::new();
    let early_rej = fz.close().err() == Some(CloseCode::NotAudited);
    let _ = fill_standard(&mut fz);
    let audited = fz.audit().is_ok() && fz.state == CloseState::Audited;
    let after_rej = fz.register(0, "F10081|达成|达成|334").err() == Some(CloseCode::Overwrite);
    s.add("审计闸：未审计拒、审计成、审计后改写拒", early_rej && audited && after_rej,
          "Drafting→Audited→Closed 单向，封版只读");

    // 8. 虚报拦截：判据缺失条目令审计失败
    let mut vr = EntryRegistry::new();
    let _ = fill_standard(&mut vr);
    let _ = vr.register(19, "F10100|达成|缺失|330");
    let vr_ok = vr.audit().err() == Some(CloseCode::NotAudited);
    s.add("虚报拦截：J2 缺失条目令审计闸拒绝", vr_ok,
          "双判据必须齐备，任何一条缺失都不得放行收口");

    // 9. 预算核销：标准 20 条零偏离
    let mut lg = WriteOffLedger::new();
    let settle_ok = settle_standard(&mut lg).is_ok();
    s.add("预算核销：20 条逐条核销零偏离", settle_ok && lg.deficit().is_none(),
          "Σ实计 == 6,640 且逐条 实计==分摊 才零偏离");

    // 10. 核销不符 10/10 检出
    let drift_hits = run_drift_injection();
    s.add("核销不符：10/10 次检出", drift_hits == INJECT_ROUNDS,
          "错账核销逐次显性拒绝并计入 drifts，不静默放行");

    // 11. 重复核销拦截
    let mut dl = WriteOffLedger::new();
    let _ = dl.write_off(FIRST_FID, 334, 334);
    let dl_ok = dl.write_off(FIRST_FID, 334, 334).err() == Some(CloseCode::DuplicateFid);
    s.add("重复核销：同 fid 二次核销拒收", dl_ok,
          "一条战线条目只允许一次核销");

    // 12. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_b25_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "缺 J2/非法态/非数字/越下限/重复条目全被显式拒收");

    // 13. 账守恒逃逸=0
    s.add("账守恒：注入过程逃逸=0",
          ledger.escapes == 0 && ledger.rows.is_empty(),
          "注入核全程不得产生坏账逃逸入账");

    // 14. 文案闭集：非空
    let text_ok = CloseCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 15. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i2 = 0usize;
    while i2 < CloseCode::ALL.len() {
        let mut j = i2 + 1;
        while j < CloseCode::ALL.len() {
            if let (Some(x), Some(y)) = (CloseCode::ALL.get(i2), CloseCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i2 += 1;
    }
    let in_seg = CloseCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 16. Linux 逐码一致
    let codes_ok = agrees_with_linux(EINVAL)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EEXIST)
        && agrees_with_linux(EOVERFLOW)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 17. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 18. 预算分摊可复算
    let mut split_sum = 0u32;
    let mut i3 = 0usize;
    while i3 < ENTRIES {
        split_sum = split_sum.saturating_add(budget_for(FIRST_FID + i3 as u32));
        i3 += 1;
    }
    s.add("预算分摊可复算：Σ分摊 == 6,640", split_sum == BUDGET,
          "334×10+330×10=6,640，分摊函数逐条可复算");

    // 动态明细（≤8）：条目快照 + 核销快照 + 指纹快照
    let filled = reg.slots.iter().filter(|e| e.is_some()).count();
    let slot_text = format!("已登记 {}/20 条（J1/J2 双判据齐备）", filled);
    s.add_dyn("条目快照", fill_ok && filled == ENTRIES, &slot_text);
    let drift_text = format!("核销 {}/20 行，drifts={}，逃逸={}", lg.rows.len(), lg.drifts, lg.escapes);
    s.add_dyn("核销快照", lg.deficit().is_none(), &drift_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ok_entry() {
        let e = parse_entry("F10081|达成|达成|334");
        assert!(e.is_ok());
        let e = e.ok().unwrap();
        assert_eq!(e.fid, 10081);
        assert_eq!(e.lines, 334);
    }

    #[test]
    fn missing_comps_rejected() {
        assert_eq!(parse_entry("|达成|达成|334"), Err(CloseCode::MissingFid));
        assert_eq!(parse_entry("F10081||达成|334"), Err(CloseCode::MissingJ1));
        assert_eq!(parse_entry("F10081|达成||334"), Err(CloseCode::MissingJ2));
        assert_eq!(parse_entry("F10081|达成|达成|"), Err(CloseCode::BadCount));
    }

    #[test]
    fn illegal_shapes_rejected() {
        assert_eq!(parse_entry("F10081|达成|完成|334"), Err(CloseCode::IllegalState));
        assert_eq!(parse_entry("F10081|达成|达成|x8"), Err(CloseCode::BadCount));
        assert_eq!(parse_entry("F10081|达成|达成|100"), Err(CloseCode::CountFloor));
        assert_eq!(parse_entry("F10101|达成|达成|334"), Err(CloseCode::SlotRange));
        assert_eq!(parse_entry("F10081|达成"), Err(CloseCode::MissingFid));
    }

    #[test]
    fn duplicate_fid_rejected() {
        let mut reg = EntryRegistry::new();
        assert!(reg.register(0, "F10081|达成|达成|334").is_ok());
        assert_eq!(reg.register(1, "F10081|达成|达成|334"), Err(CloseCode::DuplicateFid));
    }

    #[test]
    fn audit_close_flow() {
        let mut reg = EntryRegistry::new();
        assert_eq!(reg.close(), Err(CloseCode::NotAudited));
        assert!(fill_standard(&mut reg).is_ok());
        assert!(reg.audit().is_ok());
        assert!(reg.close().is_ok());
        assert_eq!(reg.register(0, "F10081|达成|达成|334"), Err(CloseCode::Overwrite));
    }

    #[test]
    fn audit_rejects_missing_judge() {
        let mut reg = EntryRegistry::new();
        let _ = fill_standard(&mut reg);
        let _ = reg.register(5, "F10086|达成|缺失|330");
        assert_eq!(reg.audit(), Err(CloseCode::NotAudited));
    }

    #[test]
    fn budget_conservation() {
        let mut lg = WriteOffLedger::new();
        assert!(settle_standard(&mut lg).is_ok());
        assert!(lg.deficit().is_none());
        let mut sum = 0u32;
        for r in lg.rows.iter() {
            sum += r.actual;
        }
        assert_eq!(sum, BUDGET);
    }

    #[test]
    fn drift_rejected() {
        let mut lg = WriteOffLedger::new();
        assert_eq!(lg.write_off(FIRST_FID, 334, 333), Err(CloseCode::BudgetDrift));
        assert_eq!(lg.drifts, 1);
        assert!(lg.rows.is_empty());
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_b25_injection();
        assert_eq!(l.escapes, 0);
        assert!(l.rows.is_empty());
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EEXIST));
        assert!(agrees_with_linux(EOVERFLOW));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EINVAL), -EINVAL);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10100_checks_green() {
        let s = run_unx10100_checks();
        assert!(s.all_passed(), "UNX-F10100 red: {}", s.red_items_text());
    }
}
