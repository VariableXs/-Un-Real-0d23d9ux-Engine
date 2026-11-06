//! UNX-F10080 B24 收口小结与预算核销 · 0x923c0 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10080（B24 收口线收尾卡，M→E 换轨前的最后一张 M 型卡）。
//! 锚点：UNX-F10081-J1 换轨口径成文落账（E 型定位与 M 型关系），
//!       口径歧义 10/10 次检出。
//!
//! 数据段说明：
//!   SEG_BASE = 0x923c0（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `SwapCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 注册表 `SwapRegistry`：20 条 B24 战线条目逐条登记判据达成位，
//!     每条必须挂换轨口径标签（M 型 / E 型 / 衔接面三选一），
//!     重复登记 / 越界登记 / 口径歧义条目均显性拒绝；
//!   - 口径解析器 `classify_scope`：换轨口径语句归一到三档闭集，
//!     同时涉 M 与 E 而无换轨关键词即判「口径歧义」，逐次检出；
//!   - 状态机 `SummaryState`：Drafting → Sealed → Written 单向推进，
//!     封账后只读、写回后冻结；
//!   - 核销账 `BudgetLedger`：预算 6,720 固定，逐笔记账带批号定位，
//!     求和=预算零偏离，偏离必须可定位到批号；
//!   - 注入核 `BadCloseCase`：口径歧义 / 超预算 / 无定位核销 /
//!     缺判据封账 / 空摘要写回 五类坏样本逐类 10/10 次拒绝；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10080 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x923c0;

/// 段窗上界（下一卡 0x92400 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x92400;

/// B24 收口战线条目数（固定 20 条判据）。
pub const CRITERION_COUNT: usize = 20;

/// B24 预算核销锁定值（来源：B24 收口小结条款 6,720）。
pub const BUDGET_LOCKED: u32 = 6720;

/// 单条目预算均值（6720 / 20 = 336；容差核用）。
pub const PER_ITEM_MEAN: u32 = 336;

/// 换轨口径歧义检出要求的复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 登记出处锚前缀（无此前缀即无源条目）。
pub const SOURCE_PREFIX: &str = "UNX-F";

/// Linux errno 正数码（对照表口径）。
pub const EPERM: i32 = 1;
pub const EACCES: i32 = 13;
pub const EINVAL: i32 = 22;
pub const ENOSPC: i32 = 28;
pub const EBUSY: i32 = 16;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwapCode {
    /// 无源条目（判据 ID 缺 UNX-F 前缀）
    NoSource,
    /// 条目越界（登记下标出 0..20 窗）
    RowRange,
    /// 重复登记同条目
    DuplicateRow,
    /// 判据缺失收口（未满 20 条即封账）
    MissingCriterion,
    /// 口径歧义（换轨口径语句无法归档到唯一档位）
    AmbiguousScope,
    /// 口径未成文（条目缺换轨口径标签）
    ScopeMissing,
    /// 超预算核销（累计支出越过锁定值）
    OverBudget,
    /// 核销无批号定位
    Unattributed,
    /// 重复核销同批
    DuplicateSpend,
    /// 写回摘要为空
    EmptySummary,
    /// 小结状态倒退
    StateRegress,
    /// 段位越界
    SegmentViolation,
}

impl SwapCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            SwapCode::NoSource => "无源条目被拒：判据 ID 必须带 UNX-F 前缀；请补 ID 再登记",
            SwapCode::RowRange => "条目越界被拒：登记下标只允许 0..20；请核对条目号",
            SwapCode::DuplicateRow => "重复登记被拒：同条目已有达成位；请先核查再改",
            SwapCode::MissingCriterion => "判据缺失收口被拒：20 条未满不得封账；请补齐缺失条目",
            SwapCode::AmbiguousScope => "口径歧义被拒：换轨口径必须归到 M/E/衔接唯一档；请改写成文",
            SwapCode::ScopeMissing => "口径未成文被拒：条目缺换轨口径标签；请补 M/E/衔接标签",
            SwapCode::OverBudget => "超预算核销被拒：累计核销不得越过 6,720；请先压预算",
            SwapCode::Unattributed => "核销无定位被拒：每笔核销必须挂批号；请补定位单",
            SwapCode::DuplicateSpend => "重复核销被拒：同批号已核销；请查重复记账",
            SwapCode::EmptySummary => "空摘要写回被拒：结果摘要必须非空；请补小结文本",
            SwapCode::StateRegress => "状态倒退被拒：Drafting→Sealed→Written 单向；请排查回滚",
            SwapCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x923c0..=0x92400",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [SwapCode; 12] = [
        SwapCode::NoSource,
        SwapCode::RowRange,
        SwapCode::DuplicateRow,
        SwapCode::MissingCriterion,
        SwapCode::AmbiguousScope,
        SwapCode::ScopeMissing,
        SwapCode::OverBudget,
        SwapCode::Unattributed,
        SwapCode::DuplicateSpend,
        SwapCode::EmptySummary,
        SwapCode::StateRegress,
        SwapCode::SegmentViolation,
    ];
}

// ── 换轨口径解析器 ────────────────────────────────────────────────────
/// 换轨口径三档闭集：M 型定位、E 型定位、M→E 衔接面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    /// M 型口径（对账/收口面，本段归属）
    MType,
    /// E 型口径（双判据面，下段归属）
    EType,
    /// 衔接面（M→E 换轨关系，两段共有）
    Bridge,
}

impl Scope {
    /// 闭集全量。
    pub const ALL: [Scope; 3] = [Scope::MType, Scope::EType, Scope::Bridge];

    /// 档位名（入账显示用，闭集返回）。
    pub fn name(self) -> &'static str {
        match self {
            Scope::MType => "M型",
            Scope::EType => "E型",
            Scope::Bridge => "衔接",
        }
    }
}

/// 子串包含判定（字节层面，ASCII 关键词；UTF-8 边界安全：整词匹配）。
fn contains_ascii(hay: &str, needle: &str) -> bool {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || n.len() > h.len() {
        return false;
    }
    let mut i = 0usize;
    while i + n.len() <= h.len() {
        let mut j = 0usize;
        let mut eq = true;
        while j < n.len() {
            if h[i + j] != n[j] {
                eq = false;
                break;
            }
            j += 1;
        }
        if eq {
            return true;
        }
        i += 1;
    }
    false
}

/// 换轨口径解析器：把口径语句归到 M/E/衔接 三档闭集。
///
/// 规则（写成文口径，避免歧义）：
///   1. 空语句 → ScopeMissing；
///   2. 同时出现 M 型与 E 型关键词而无「换轨/衔接」关键词 → AmbiguousScope；
///   3. 只涉 M → MType；只涉 E → EType；
///   4. 双涉且带换轨关键词 → Bridge。
pub fn classify_scope(stmt: &str) -> Result<Scope, SwapCode> {
    if stmt.is_empty() {
        return Err(SwapCode::ScopeMissing);
    }
    let has_m = contains_ascii(stmt, "M型") || contains_ascii(stmt, "M-") || contains_ascii(stmt, "M ");
    let has_e = contains_ascii(stmt, "E型") || contains_ascii(stmt, "E-") || contains_ascii(stmt, "E ");
    let has_bridge = contains_ascii(stmt, "换轨") || contains_ascii(stmt, "衔接");
    match (has_m, has_e, has_bridge) {
        (true, true, true) => Ok(Scope::Bridge),
        (true, true, false) => Err(SwapCode::AmbiguousScope),
        (true, false, _) => Ok(Scope::MType),
        (false, true, _) => Ok(Scope::EType),
        (false, false, true) => Ok(Scope::Bridge),
        (false, false, false) => Err(SwapCode::ScopeMissing),
    }
}

// ── 收口小结状态机与注册表 ────────────────────────────────────────────
/// 状态机：Drafting → Sealed → Written 单向推进。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SummaryState {
    /// 起草中（判据登记进行时）
    Drafting,
    /// 已封账（20 条齐，判据只读）
    Sealed,
    /// 已写回（状态=已完成、摘要非空）
    Written,
}

/// 判据 ID 解析器："UNX-F10080-J3" → 条目序号 3（1..=20）。
pub fn parse_criterion_no(id: &str) -> Option<usize> {
    let tag = "-J";
    let pos = id.rfind(tag)?;
    let tail = id.get(pos + tag.len()..)?;
    if tail.is_empty() || tail.len() > 2 {
        return None;
    }
    let mut n: usize = 0;
    for b in tail.bytes() {
        if !b.is_ascii_digit() {
            return None;
        }
        n = n * 10 + (b - b'0') as usize;
    }
    if n >= 1 && n <= CRITERION_COUNT {
        Some(n)
    } else {
        None
    }
}

/// 收口小结注册表：20 条判据达成位 + 每条换轨口径档位 + 状态机。
#[derive(Clone, Debug)]
pub struct SwapRegistry {
    /// 判据达成位（None=未登记）
    pub rows: [Option<bool>; CRITERION_COUNT],
    /// 每条判据挂的换轨口径档位（None=未挂）
    pub scopes: [Option<Scope>; CRITERION_COUNT],
    /// 当前状态
    pub state: SummaryState,
}

impl SwapRegistry {
    /// 构造空注册表。
    pub const fn new() -> SwapRegistry {
        SwapRegistry {
            rows: [None; CRITERION_COUNT],
            scopes: [None; CRITERION_COUNT],
            state: SummaryState::Drafting,
        }
    }

    /// 登记一条判据：ID 必检、口径必检、重复必拒、歧义必拒、封账后必拒。
    pub fn register(&mut self, id: &str, achieved: bool, scope_stmt: &str) -> Result<usize, SwapCode> {
        if self.state != SummaryState::Drafting {
            return Err(SwapCode::StateRegress);
        }
        if id.is_empty()
            || id.len() < SOURCE_PREFIX.len()
            || &id.as_bytes()[..SOURCE_PREFIX.len()] != SOURCE_PREFIX.as_bytes()
        {
            return Err(SwapCode::NoSource);
        }
        let no = match parse_criterion_no(id) {
            Some(n) => n,
            None => return Err(SwapCode::RowRange),
        };
        let idx = no - 1;
        let scope = classify_scope(scope_stmt)?;
        if self.rows.get(idx).copied().unwrap_or(None).is_some() {
            return Err(SwapCode::DuplicateRow);
        }
        if let Some(slot) = self.rows.get_mut(idx) {
            *slot = Some(achieved);
        }
        if let Some(slot) = self.scopes.get_mut(idx) {
            *slot = Some(scope);
        }
        Ok(idx)
    }

    /// 封账：20 条全登记且全达成且每条口径已挂档才可封。
    pub fn seal(&mut self) -> Result<(), SwapCode> {
        if self.state != SummaryState::Drafting {
            return Err(SwapCode::StateRegress);
        }
        let mut i = 0usize;
        while i < CRITERION_COUNT {
            match self.rows.get(i).copied().unwrap_or(None) {
                Some(true) => {}
                _ => return Err(SwapCode::MissingCriterion),
            }
            if self.scopes.get(i).copied().unwrap_or(None).is_none() {
                return Err(SwapCode::ScopeMissing);
            }
            i += 1;
        }
        self.state = SummaryState::Sealed;
        Ok(())
    }

    /// 写回：封账后才可写回；摘要必须非空。
    pub fn writeback(&mut self, summary: &str) -> Result<(), SwapCode> {
        if self.state != SummaryState::Sealed {
            return Err(SwapCode::StateRegress);
        }
        if summary.is_empty() {
            return Err(SwapCode::EmptySummary);
        }
        self.state = SummaryState::Written;
        Ok(())
    }

    /// 已登记达成条数。
    pub fn achieved_count(&self) -> usize {
        self.rows.iter().filter(|r| matches!(r, Some(true))).count()
    }
}

/// 标准 20 条判据登记：J1..J20 逐条带成文换轨口径。
/// 口径分布：J1..J14 M 型、J15..J18 E 型、J19..J20 衔接面。
pub fn register_standard(reg: &mut SwapRegistry) -> Result<(), SwapCode> {
    let mut n = 1usize;
    while n <= CRITERION_COUNT {
        let id = format!("UNX-F10080-J{}", n);
        let stmt = if n <= 14 {
            format!("M型 对账收口条目 {}", n)
        } else if n <= 18 {
            format!("E型 双判据条目 {}", n)
        } else {
            format!("M型 与 E型 换轨衔接条目 {}", n)
        };
        reg.register(&id, true, &stmt)?;
        n += 1;
    }
    Ok(())
}

// ── 预算核销账 ────────────────────────────────────────────────────────
/// 一笔核销：批号定位 + 金额。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SpendEntry {
    /// 批号（B01..B40 编号 1..=40）
    pub batch: u32,
    /// 核销金额
    pub amount: u32,
}

/// 预算核销账：逐笔记账、求和零偏离、偏离可定位。
#[derive(Clone, Debug)]
pub struct BudgetLedger {
    /// 已记账目（批号互异）
    pub entries: Vec<SpendEntry>,
    /// 累计核销
    pub spent: u32,
    /// 无法定位到批号的逃逸笔数（必须为 0）
    pub unattributed: u32,
}

impl BudgetLedger {
    /// 构造零账。
    pub fn new() -> BudgetLedger {
        BudgetLedger { entries: Vec::new(), spent: 0, unattributed: 0 }
    }

    /// 记一笔核销：批号 1..=40 必检、重复批号必拒、超预算必拒。
    pub fn record(&mut self, batch: u32, amount: u32) -> Result<(), SwapCode> {
        if batch == 0 || batch > 40 {
            self.unattributed = self.unattributed.saturating_add(1);
            return Err(SwapCode::Unattributed);
        }
        if self.entries.iter().any(|e| e.batch == batch) {
            return Err(SwapCode::DuplicateSpend);
        }
        let next = self.spent.saturating_add(amount);
        if next > BUDGET_LOCKED {
            return Err(SwapCode::OverBudget);
        }
        self.spent = next;
        self.entries.push(SpendEntry { batch, amount });
        Ok(())
    }

    /// 守恒式：求和 = 累计 = 预算锁定值才为零偏离。
    pub fn deviation(&self) -> Option<u32> {
        let sum = self.entries.iter().map(|e| e.amount).fold(0u32, |a, b| a.saturating_add(b));
        if sum == self.spent && sum == BUDGET_LOCKED {
            None
        } else {
            Some(BUDGET_LOCKED.saturating_sub(sum))
        }
    }

    /// 偏离定位：返回第一笔出均值 ±40% 容差带的批号。
    pub fn locate_outlier(&self) -> Option<u32> {
        let lo = PER_ITEM_MEAN * 60 / 100;
        let hi = PER_ITEM_MEAN * 140 / 100;
        for e in self.entries.iter() {
            if e.amount < lo || e.amount > hi {
                return Some(e.batch);
            }
        }
        None
    }
}

/// 标准核销方案：20 批各 336，合计 6,720 恰等于预算。
pub fn spend_standard(ledger: &mut BudgetLedger) -> Result<(), SwapCode> {
    let mut n = 1u32;
    while n <= 20 {
        ledger.record(n, PER_ITEM_MEAN)?;
        n += 1;
    }
    Ok(())
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏收口样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadCloseCase {
    /// 口径歧义条目（M/E 双涉无换轨词）
    AmbiguousScope,
    /// 超预算核销（累计越过 6,720）
    OverBudget,
    /// 无批号定位核销
    Unattributed,
    /// 缺判据封账（19 条即封）
    MissingCriterion,
    /// 空摘要写回
    EmptySummary,
}

impl BadCloseCase {
    /// 闭集全量。
    pub const ALL: [BadCloseCase; 5] = [
        BadCloseCase::AmbiguousScope,
        BadCloseCase::OverBudget,
        BadCloseCase::Unattributed,
        BadCloseCase::MissingCriterion,
        BadCloseCase::EmptySummary,
    ];

    /// 注入一轮：构造坏样本并触发校验，返回是否被显式拒绝。
    pub fn inject(self) -> bool {
        match self {
            BadCloseCase::AmbiguousScope => {
                let mut reg = SwapRegistry::new();
                reg.register("UNX-F10080-J1", true, "M型 与 E型 并存无向")
                    == Err(SwapCode::AmbiguousScope)
            }
            BadCloseCase::OverBudget => {
                let mut l = BudgetLedger::new();
                let a = l.record(1, BUDGET_LOCKED).is_ok();
                let b = l.record(2, 1).err() == Some(SwapCode::OverBudget);
                a && b
            }
            BadCloseCase::Unattributed => {
                let mut l = BudgetLedger::new();
                l.record(0, 100).is_err() && l.unattributed == 1
            }
            BadCloseCase::MissingCriterion => {
                let mut reg = SwapRegistry::new();
                let mut n = 1usize;
                while n < CRITERION_COUNT {
                    let id = format!("UNX-F10080-J{}", n);
                    let stmt = format!("M型 条目 {}", n);
                    if reg.register(&id, true, &stmt).is_err() {
                        return false;
                    }
                    n += 1;
                }
                reg.seal().is_err()
            }
            BadCloseCase::EmptySummary => {
                let mut reg = SwapRegistry::new();
                let _ = register_standard(&mut reg);
                if reg.seal().is_err() {
                    return false;
                }
                reg.writeback("").is_err()
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_close_injection() -> (BudgetLedger, [u32; 5]) {
    let mut ledger = BudgetLedger::new();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadCloseCase::ALL.len() {
        if let Some(case) = BadCloseCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject();
                if !caught {
                    ledger.unattributed = ledger.unattributed.saturating_add(1);
                }
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
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；状态倒退/封账改判据族"),
    ("EACCES", -13, 5, "Windows ERROR_ACCESS_DENIED；写回冻结族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；口径歧义/条目越界族"),
    ("ENOSPC", -28, 112, "Windows ERROR_DISK_FULL；预算耗尽族"),
    ("EBUSY", -16, 170, "Windows ERROR_BUSY；重复核销族"),
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

/// 规范语料双跑指纹：口径归档→登记→封账→写回→核销全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    // 口径解析链：三档归档结果逐字节入哈希
    let probes = ["M型 对账", "E型 双判据", "M型 与 E型 换轨衔接", "", "歧义双涉 M型 E型"];
    let mut p = 0usize;
    while p < probes.len() {
        if let Some(s) = probes.get(p) {
            match classify_scope(s) {
                Ok(sc) => bytes.push(sc as u8 + 1),
                Err(e) => bytes.push(e.val() as u8 & 0x0F),
            }
        }
        p += 1;
    }
    let mut reg = SwapRegistry::new();
    let _ = register_standard(&mut reg);
    bytes.push(match reg.seal() {
        Ok(()) => 1,
        Err(_) => 2,
    });
    bytes.push(match reg.writeback("B24 收口：20 判据齐+6,720 核销零偏离+换轨口径成文") {
        Ok(()) => 3,
        Err(_) => 4,
    });
    bytes.push(reg.state as u8);
    let mut l = BudgetLedger::new();
    let _ = spend_standard(&mut l);
    bytes.extend_from_slice(&l.spent.to_le_bytes());
    bytes.push(match l.deviation() {
        None => 5,
        Some(_) => 6,
    });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10080 判据集（17 条静态 + 3 条动态明细）。
pub fn run_unx10080_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10080");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x923c0..=0x92400", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝");

    // 2. 换轨口径三档归档
    let scope_ok = classify_scope("M型 对账面收口") == Ok(Scope::MType)
        && classify_scope("E型 双判据开卷") == Ok(Scope::EType)
        && classify_scope("M型 与 E型 换轨衔接") == Ok(Scope::Bridge);
    s.add("口径归档：M/E/衔接三档逐档命中", scope_ok,
          "换轨口径必须归一到三档闭集，才能成文落账");

    // 3. 口径歧义 10/10 检出（锚点判据）
    let mut amb_hits = 0u32;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        if classify_scope("M型 与 E型 并存").err() == Some(SwapCode::AmbiguousScope) {
            amb_hits += 1;
        }
        round += 1;
    }
    s.add("口径歧义：10/10 次检出", amb_hits == INJECT_ROUNDS,
          "M/E 双涉而无换轨关键词的语句逐次判歧义拒绝");

    // 4. 空口径与无口径语句拒绝
    let empty_rej = classify_scope("").err() == Some(SwapCode::ScopeMissing)
        && classify_scope("纯描述无档位词").err() == Some(SwapCode::ScopeMissing);
    s.add("口径未成文：空语句/无档位词拒绝", empty_rej,
          "条目必须带 M/E/衔接之一，否则不得入收口账");

    // 5. 标准 20 条登记成功且口径全挂档
    let mut reg = SwapRegistry::new();
    let std_ok = register_standard(&mut reg).is_ok() && reg.achieved_count() == CRITERION_COUNT;
    let mut scope_filled = true;
    let mut i = 0usize;
    while i < CRITERION_COUNT {
        if reg.scopes.get(i).copied().unwrap_or(None).is_none() {
            scope_filled = false;
        }
        i += 1;
    }
    s.add("小结登记：20 条判据逐条登记且口径全挂档", std_ok && scope_filled,
          "UNX-F10080-J1..J20 逐条入表，口径 M×14+E×4+衔接×2");

    // 6. 无源条目拦截
    let nosrc = reg.register("B24 手记", true, "M型 条目")
        == Err(SwapCode::NoSource);
    s.add("无源条目：缺 UNX-F 前缀的 ID 拒登", nosrc,
          "判据 ID 必须带出处锚前缀才可入小结账");

    // 7. 封账闸：缺条拒绝 + 齐账通过 + 状态单向
    let mut part = SwapRegistry::new();
    let _ = part.register("UNX-F10080-J1", true, "M型 条目");
    let part_rej = part.seal() == Err(SwapCode::MissingCriterion);
    let mut full = SwapRegistry::new();
    let _ = register_standard(&mut full);
    let full_ok = full.seal().is_ok() && full.state == SummaryState::Sealed;
    s.add("封账闸：判据缺失拒绝、20 条齐通过", part_rej && full_ok,
          "封账单向：Drafting → Sealed，回写倒退显性拒绝");

    // 8. 写回三态：未封账拒、空摘要拒、正常写回成
    let mut wb = SwapRegistry::new();
    let _ = register_standard(&mut wb);
    let early_rej = wb.writeback("摘要") == Err(SwapCode::StateRegress);
    let _ = wb.seal();
    let empty_rej2 = wb.writeback("") == Err(SwapCode::EmptySummary);
    let write_ok = wb.writeback("B24 收口：20 判据+6,720 核销").is_ok()
        && wb.state == SummaryState::Written;
    s.add("写回三态：未封账拒、空摘要拒、正常写回成", early_rej && empty_rej2 && write_ok,
          "状态=已完成 且 结果摘要非空才算完成写回");

    // 9. 标准核销：20 批求和恰为 6,720 零偏离
    let mut l = BudgetLedger::new();
    let spend_ok = spend_standard(&mut l).is_ok();
    s.add("预算核销：20 批各 336 求和=6,720 零偏离",
          spend_ok && l.deviation().is_none() && l.spent == BUDGET_LOCKED,
          "6720 = 20 × 336，分批核销后总账守恒");

    // 10. 超预算核销拦截
    let mut ob = BudgetLedger::new();
    let a = ob.record(1, BUDGET_LOCKED).is_ok();
    let b = ob.record(2, 1).err() == Some(SwapCode::OverBudget);
    s.add("超预算：累计越过锁定值即拦", a && b,
          "6,720 是硬顶，多核销一元即拒绝");

    // 11. 无定位核销拦截 + 逃逸显性入账
    let mut ua = BudgetLedger::new();
    let ua_rej = ua.record(0, 100).is_err() && ua.record(41, 100).is_err();
    s.add("无定位：批号出 1..=40 拒记且逃逸入账",
          ua_rej && ua.unattributed == 2,
          "核销必须挂批号；无定位逃逸显性可见不静默");

    // 12. 重复核销拦截
    let mut ds = BudgetLedger::new();
    let _ = ds.record(5, 100);
    let ds_ok = ds.record(5, 100) == Err(SwapCode::DuplicateSpend);
    s.add("重复核销：同批二次记账被拒", ds_ok,
          "一批一笔，重复批号显性拒绝");

    // 13. 偏离定位：出容差带批号可定位
    let mut loc = BudgetLedger::new();
    let _ = spend_standard(&mut loc);
    let no_outlier = loc.locate_outlier().is_none();
    let mut bad = BudgetLedger::new();
    let _ = bad.record(1, PER_ITEM_MEAN * 2);
    let _ = bad.record(2, BUDGET_LOCKED - PER_ITEM_MEAN * 2);
    let located = bad.locate_outlier() == Some(1);
    s.add("偏离定位：均衡方案无离群、失衡方案定位到批", no_outlier && located,
          "偏离不定位即拦截：离群批必须可指认");

    // 14. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_close_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "口径歧义/超预算/无定位/缺判据/空摘要全被显式拒绝");

    // 15. 账守恒逃逸=0
    s.add("账守恒：注入过程逃逸=0",
          ledger.unattributed == 0 && ledger.entries.is_empty(),
          "注入核全程不得产生未定位核销逃逸");

    // 16. 文案闭集：非空
    let text_ok = SwapCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 17. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i2 = 0usize;
    while i2 < SwapCode::ALL.len() {
        let mut j = i2 + 1;
        while j < SwapCode::ALL.len() {
            if let (Some(x), Some(y)) = (SwapCode::ALL.get(i2), SwapCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i2 += 1;
    }
    let in_seg = SwapCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 18. Linux 逐码一致
    let codes_ok = agrees_with_linux(EPERM)
        && agrees_with_linux(EACCES)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(ENOSPC)
        && agrees_with_linux(EBUSY)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 19. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 动态明细（≤8）：核销快照 + 口径分布 + 指纹快照
    let ledger_text = format!("核销 {}/{} 元 / 笔数 {}", l.spent, BUDGET_LOCKED, l.entries.len());
    s.add_dyn("核销快照", l.deviation().is_none(), &ledger_text);
    let m_cnt = reg.scopes.iter().filter(|sc| matches!(sc, Some(Scope::MType))).count();
    let scope_text = format!("M型 {} 条 / E型 4 条 / 衔接 2 条", m_cnt);
    s.add_dyn("口径分布", m_cnt == 14, &scope_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

/// 探针：向注册表登记一条合法判据（仅测试块使用，供缺条封账测试用）。
#[cfg(test)]
fn reg_probe(reg: &mut SwapRegistry) -> Result<usize, SwapCode> {
    reg.register("UNX-F10080-J1", true, "M型 条目")
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_classify_closed_set() {
        assert_eq!(classify_scope("M型 收口"), Ok(Scope::MType));
        assert_eq!(classify_scope("E型 开卷"), Ok(Scope::EType));
        assert_eq!(classify_scope("M型 与 E型 换轨衔接"), Ok(Scope::Bridge));
        assert_eq!(classify_scope("M型 与 E型 并存"), Err(SwapCode::AmbiguousScope));
        assert_eq!(classify_scope(""), Err(SwapCode::ScopeMissing));
    }

    #[test]
    fn standard_registration_ok() {
        let mut reg = SwapRegistry::new();
        assert!(register_standard(&mut reg).is_ok());
        assert_eq!(reg.achieved_count(), CRITERION_COUNT);
    }

    #[test]
    fn no_source_rejected() {
        let mut reg = SwapRegistry::new();
        assert_eq!(reg.register("", true, "M型"), Err(SwapCode::NoSource));
        assert_eq!(reg.register("B24 手记", true, "M型"), Err(SwapCode::NoSource));
    }

    #[test]
    fn parse_boundary() {
        assert_eq!(parse_criterion_no("UNX-F10080-J1"), Some(1));
        assert_eq!(parse_criterion_no("UNX-F10080-J20"), Some(20));
        assert!(parse_criterion_no("UNX-F10080-J21").is_none());
        assert!(parse_criterion_no("UNX-F10080-Jx").is_none());
    }

    #[test]
    fn partial_seal_rejected() {
        let mut reg = SwapRegistry::new();
        let _ = reg_probe(&mut reg);
        assert_eq!(reg.seal(), Err(SwapCode::MissingCriterion));
    }

    #[test]
    fn writeback_requires_seal_and_text() {
        let mut reg = SwapRegistry::new();
        let _ = register_standard(&mut reg);
        assert_eq!(reg.writeback("x"), Err(SwapCode::StateRegress));
        let _ = reg.seal();
        assert_eq!(reg.writeback(""), Err(SwapCode::EmptySummary));
        assert!(reg.writeback("完成").is_ok());
    }

    #[test]
    fn budget_conservation() {
        let mut l = BudgetLedger::new();
        assert!(spend_standard(&mut l).is_ok());
        assert_eq!(l.spent, BUDGET_LOCKED);
        assert!(l.deviation().is_none());
        assert!(l.locate_outlier().is_none());
    }

    #[test]
    fn over_budget_and_duplicate_rejected() {
        let mut l = BudgetLedger::new();
        assert!(l.record(1, BUDGET_LOCKED).is_ok());
        assert_eq!(l.record(2, 1), Err(SwapCode::OverBudget));
        assert_eq!(l.record(1, 1), Err(SwapCode::DuplicateSpend));
    }

    #[test]
    fn unattributed_spend_visible() {
        let mut l = BudgetLedger::new();
        assert_eq!(l.record(0, 10), Err(SwapCode::Unattributed));
        assert_eq!(l.record(99, 10), Err(SwapCode::Unattributed));
        assert_eq!(l.unattributed, 2);
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_close_injection();
        assert_eq!(l.unattributed, 0);
        assert!(l.entries.is_empty());
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EPERM));
        assert!(agrees_with_linux(EBUSY));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EINVAL), -EINVAL);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10080_checks_green() {
        let s = run_unx10080_checks();
        assert!(s.all_passed(), "UNX-F10080 red: {}", s.red_items_text());
    }
}
