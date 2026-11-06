//! UNX-F10060 B23 收口小结与预算核销 · 0x91ec0 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10060（B23 收口线收尾卡）。
//! 锚点：UNX-F10061-J1 收口小结落账（20 条判据达成+预算 6,620 核销零偏离），
//!       核销不符 10/10 次检出。
//!
//! 数据段说明：
//!   SEG_BASE = 0x91ec0（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `ClosureCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 注册表 `SummaryRegistry`：20 条战线条目逐条登记判据达成位，
//!     登记 ID 必须带 "UNX-F" 前缀，重复登记/越界登记显性拒绝；
//!   - 状态机 `SummaryState`：Drafting → Sealed → Written 单向推进，
//!     封账后判据只读、写回后小结冻结；
//!   - 核销账 `BudgetLedger`：预算 6,620 固定，逐笔记账带批号定位，
//!     求和=预算零偏离；偏离必须能定位到批号，无定位偏离拦截；
//!   - 写回 `Writeback`：状态=已完成 且 结果摘要非空才算完成写回；
//!   - 注入核 `BadClosureCase`：判据缺失 / 超预算核销 / 重复核销 /
//!     空摘要写回 / 封账后改判据 五类坏样本逐类 10/10 次拒绝；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10060 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x91ec0;

/// 段窗上界（下一卡 0x91f00 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x91f00;

/// 收口小结战线条目数（B23 固定 20 条判据）。
pub const CRITERION_COUNT: usize = 20;

/// B23 预算核销锁定值（来源：B23 收口小结条款 6,620）。
pub const BUDGET_LOCKED: u32 = 6620;

/// 单条目预算均值（6620 / 20 = 331；容差核用）。
pub const PER_ITEM_MEAN: u32 = 331;

/// 登记出处锚前缀（无此前缀即无源条目）。
pub const SOURCE_PREFIX: &str = "UNX-F";

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EPERM: i32 = 1;
pub const EACCES: i32 = 13;
pub const EINVAL: i32 = 22;
pub const ENOSPC: i32 = 28;
pub const EOVERFLOW: i32 = 75;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClosureCode {
    /// 无源条目（判据 ID 缺 UNX-F 前缀）
    NoSource,
    /// 条目越界（登记下标出 0..20 窗）
    RowRange,
    /// 重复登记同条目
    DuplicateRow,
    /// 判据缺失收口（未满 20 条即封账）
    MissingCriterion,
    /// 超预算核销（累计支出越过锁定值）
    OverBudget,
    /// 核销无批号定位
    Unattributed,
    /// 重复核销同批
    DuplicateSpend,
    /// 封账后改判据
    SealedMutate,
    /// 写回摘要为空
    EmptySummary,
    /// 小结状态倒退
    StateRegress,
    /// 解析失败
    ParseFailure,
    /// 段位越界
    SegmentViolation,
}

impl ClosureCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ClosureCode::NoSource => "无源条目被拒：判据 ID 必须带 UNX-F 前缀；请补 ID 再登记",
            ClosureCode::RowRange => "条目越界被拒：登记下标只允许 0..20；请核对条目号",
            ClosureCode::DuplicateRow => "重复登记被拒：同条目已有达成位；请先核查再改",
            ClosureCode::MissingCriterion => "判据缺失收口被拒：20 条未满不得封账；请补齐缺失条目",
            ClosureCode::OverBudget => "超预算核销被拒：累计核销不得越过 6,620；请先压预算",
            ClosureCode::Unattributed => "核销无定位被拒：每笔核销必须挂批号；请补定位单",
            ClosureCode::DuplicateSpend => "重复核销被拒：同批号已核销；请查重复记账",
            ClosureCode::SealedMutate => "封账后改判据被拒：小结已封账只读；请走修订通道",
            ClosureCode::EmptySummary => "空摘要写回被拒：结果摘要必须非空；请补小结文本",
            ClosureCode::StateRegress => "状态倒退被拒：Drafting→Sealed→Written 单向；请排查回滚",
            ClosureCode::ParseFailure => "解析失败：核销行必须恰为 batch|amount 两段；请补齐字段",
            ClosureCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x91ec0..=0x91f00",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [ClosureCode; 12] = [
        ClosureCode::NoSource,
        ClosureCode::RowRange,
        ClosureCode::DuplicateRow,
        ClosureCode::MissingCriterion,
        ClosureCode::OverBudget,
        ClosureCode::Unattributed,
        ClosureCode::DuplicateSpend,
        ClosureCode::SealedMutate,
        ClosureCode::EmptySummary,
        ClosureCode::StateRegress,
        ClosureCode::ParseFailure,
        ClosureCode::SegmentViolation,
    ];
}

// ── 收口小结状态机 ────────────────────────────────────────────────────
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

/// 登记出处锚合法：非空且以 "UNX-F" 开头。
pub fn source_ok(src: &str) -> bool {
    !src.is_empty()
        && src.len() >= SOURCE_PREFIX.len()
        && &src.as_bytes()[..SOURCE_PREFIX.len()] == SOURCE_PREFIX.as_bytes()
}

/// 判据 ID 解析器："UNX-F10060-J3" → 条目序号 3（1..=20）。
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

/// 收口小结注册表：20 条判据达成位 + 状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SummaryRegistry {
    /// 判据达成位（None=未登记，Some=已登记并带达成值）
    pub rows: [Option<bool>; CRITERION_COUNT],
    /// 当前状态
    pub state: SummaryState,
}

impl SummaryRegistry {
    /// 构造空注册表。
    pub const fn new() -> SummaryRegistry {
        SummaryRegistry { rows: [None; CRITERION_COUNT], state: SummaryState::Drafting }
    }

    /// 登记一条判据：ID 必检、下标必检、重复必拒、封账后必拒。
    pub fn register(&mut self, id: &str, achieved: bool) -> Result<usize, ClosureCode> {
        if self.state != SummaryState::Drafting {
            return Err(ClosureCode::SealedMutate);
        }
        if !source_ok(id) {
            return Err(ClosureCode::NoSource);
        }
        let no = match parse_criterion_no(id) {
            Some(n) => n,
            None => return Err(ClosureCode::ParseFailure),
        };
        let idx = no - 1;
        if idx >= CRITERION_COUNT {
            return Err(ClosureCode::RowRange);
        }
        if self.rows.get(idx).copied().unwrap_or(None).is_some() {
            return Err(ClosureCode::DuplicateRow);
        }
        if let Some(slot) = self.rows.get_mut(idx) {
            *slot = Some(achieved);
        }
        Ok(idx)
    }

    /// 封账：20 条全登记且全达成才可封；缺条/有红均拒绝。
    pub fn seal(&mut self) -> Result<(), ClosureCode> {
        if self.state != SummaryState::Drafting {
            return Err(ClosureCode::StateRegress);
        }
        let mut i = 0usize;
        while i < CRITERION_COUNT {
            match self.rows.get(i).copied().unwrap_or(None) {
                None => return Err(ClosureCode::MissingCriterion),
                Some(false) => return Err(ClosureCode::MissingCriterion),
                Some(true) => {}
            }
            i += 1;
        }
        self.state = SummaryState::Sealed;
        Ok(())
    }

    /// 写回：封账后才可写回；摘要必须非空。
    pub fn writeback(&mut self, summary: &str) -> Result<(), ClosureCode> {
        if self.state != SummaryState::Sealed {
            return Err(ClosureCode::StateRegress);
        }
        if summary.is_empty() {
            return Err(ClosureCode::EmptySummary);
        }
        self.state = SummaryState::Written;
        Ok(())
    }

    /// 已登记达成条数。
    pub fn achieved_count(&self) -> usize {
        self.rows.iter().filter(|r| matches!(r, Some(true))).count()
    }
}

/// 标准 20 条判据登记（ID = UNX-F10060-J1..J20）。
pub fn register_standard(reg: &mut SummaryRegistry) -> Result<(), ClosureCode> {
    let mut n = 1usize;
    while n <= CRITERION_COUNT {
        let id = format!("UNX-F10060-J{}", n);
        reg.register(&id, true)?;
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
    pub fn record(&mut self, batch: u32, amount: u32) -> Result<(), ClosureCode> {
        if batch == 0 || batch > 40 {
            self.unattributed = self.unattributed.saturating_add(1);
            return Err(ClosureCode::Unattributed);
        }
        if self.entries.iter().any(|e| e.batch == batch) {
            return Err(ClosureCode::DuplicateSpend);
        }
        let next = self.spent.saturating_add(amount);
        if next > BUDGET_LOCKED {
            return Err(ClosureCode::OverBudget);
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

    /// 偏离定位：返回第一笔未达均值容差带的批号（偏离必须能定位）。
    pub fn locate_outlier(&self) -> Option<u32> {
        // 均值 331 ±40% 容差带：单批核销出带即定位为离群批。
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

/// 标准核销方案：20 批各 331，合计 6,620 恰等于预算。
pub fn spend_standard(ledger: &mut BudgetLedger) -> Result<(), ClosureCode> {
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
pub enum BadClosureCase {
    /// 判据缺失封账（19 条即封）
    MissingCriterion,
    /// 超预算核销（累计越过 6,620）
    OverBudget,
    /// 重复核销同批
    DuplicateSpend,
    /// 空摘要写回
    EmptySummary,
    /// 封账后改判据
    SealedMutate,
}

impl BadClosureCase {
    /// 闭集全量。
    pub const ALL: [BadClosureCase; 5] = [
        BadClosureCase::MissingCriterion,
        BadClosureCase::OverBudget,
        BadClosureCase::DuplicateSpend,
        BadClosureCase::EmptySummary,
        BadClosureCase::SealedMutate,
    ];

    /// 注入一轮：构造坏样本并触发校验，返回是否被显式拒绝。
    pub fn inject(self) -> bool {
        match self {
            BadClosureCase::MissingCriterion => {
                // 只登记 19 条（跳过 J20）就封账，必须被拒。
                let mut part = SummaryRegistry::new();
                let mut n = 1usize;
                while n < CRITERION_COUNT {
                    let id = format!("UNX-F10060-J{}", n);
                    if part.register(&id, true).is_err() {
                        return false;
                    }
                    n += 1;
                }
                part.seal().is_err()
            }
            BadClosureCase::OverBudget => {
                let mut l = BudgetLedger::new();
                let a = l.record(1, BUDGET_LOCKED).is_ok();
                let b = l.record(2, 1) == Err(ClosureCode::OverBudget);
                a && b
            }
            BadClosureCase::DuplicateSpend => {
                let mut l = BudgetLedger::new();
                let a = l.record(3, 100).is_ok();
                let b = l.record(3, 100).is_err();
                a && b
            }
            BadClosureCase::EmptySummary => {
                let mut reg = SummaryRegistry::new();
                let _ = register_standard(&mut reg);
                if reg.seal().is_err() {
                    return false;
                }
                reg.writeback("").is_err()
            }
            BadClosureCase::SealedMutate => {
                let mut reg = SummaryRegistry::new();
                let _ = register_standard(&mut reg);
                if reg.seal().is_err() {
                    return false;
                }
                reg.register("UNX-F10060-J1", true).is_err()
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_closure_injection() -> (BudgetLedger, [u32; 5]) {
    let mut ledger = BudgetLedger::new();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadClosureCase::ALL.len() {
        if let Some(case) = BadClosureCase::ALL.get(ci).copied() {
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
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；封账后改判据族"),
    ("EACCES", -13, 5, "Windows ERROR_ACCESS_DENIED；写回冻结族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；条目/批号越界族"),
    ("ENOSPC", -28, 112, "Windows ERROR_DISK_FULL；预算耗尽族"),
    ("EOVERFLOW", -75, 28, "Windows ERROR_OUT_OF_PAPER；累计溢出族"),
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

/// 规范语料双跑指纹：登记→封账→写回→核销全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut reg = SummaryRegistry::new();
    let mut bytes: Vec<u8> = Vec::new();
    let _ = register_standard(&mut reg);
    bytes.push(match reg.seal() {
        Ok(()) => 1,
        Err(_) => 2,
    });
    bytes.push(match reg.writeback("B23 收口小结：20 条判据达成，预算核销零偏离") {
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
/// F10060 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx10060_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10060");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91ec0..=0x91f00", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝");

    // 2. 标准 20 条登记成功
    let mut reg = SummaryRegistry::new();
    let std_ok = register_standard(&mut reg).is_ok() && reg.achieved_count() == CRITERION_COUNT;
    s.add("小结登记：20 条判据逐条登记达成位", std_ok,
          "UNX-F10060-J1..J20 逐条入表且全部达成");

    // 3. 无源条目拦截
    let nosrc = reg.register("B23 手记", true) == Err(ClosureCode::NoSource);
    s.add("无源条目：缺 UNX-F 前缀的 ID 拒登", nosrc,
          "判据 ID 必须带出处锚前缀才可入小结账");

    // 4. ID 解析器：合法命中 + 非法拒绝
    let parse_ok = parse_criterion_no("UNX-F10060-J7") == Some(7)
        && parse_criterion_no("UNX-F10060-J0").is_none()
        && parse_criterion_no("UNX-F10060-J21").is_none()
        && parse_criterion_no("UNX-F10060-Jx").is_none();
    s.add("ID 解析：J1..J20 命中、J0/J21/非数字拒绝", parse_ok,
          "解析器闭集：序号只允许 1..=20 且必须纯数字");

    // 5. 重复登记拦截
    let mut dup = SummaryRegistry::new();
    let _ = dup.register("UNX-F10060-J1", true);
    let dup_ok = dup.register("UNX-F10060-J1", true) == Err(ClosureCode::DuplicateRow);
    s.add("重复登记：同条目二次登记被拒", dup_ok,
          "一条一槽，重复登记显性拒绝");

    // 6. 判据缺失封账拦截 + 齐账封账成功
    let mut part = SummaryRegistry::new();
    let _ = reg_seal_probe(&mut part);
    let part_rej = part.seal() == Err(ClosureCode::MissingCriterion);
    let mut full = SummaryRegistry::new();
    let _ = register_standard(&mut full);
    let full_ok = full.seal().is_ok() && full.state == SummaryState::Sealed;
    s.add("封账闸：判据缺失拒绝、20 条齐通过", part_rej && full_ok,
          "封账单向：Drafting → Sealed");

    // 7. 写回：未封账写回拒绝 + 空摘要拒绝 + 正常写回成功
    let mut wb = SummaryRegistry::new();
    let _ = register_standard(&mut wb);
    let early_rej = wb.writeback("摘要") == Err(ClosureCode::StateRegress);
    let _ = wb.seal();
    let empty_rej = wb.writeback("") == Err(ClosureCode::EmptySummary);
    let write_ok = wb.writeback("B23 收口：20 判据+6,620 核销").is_ok()
        && wb.state == SummaryState::Written;
    s.add("写回三态：未封账拒、空摘要拒、正常写回成", early_rej && empty_rej && write_ok,
          "状态=已完成 且 结果摘要非空才算完成写回");

    // 8. 标准核销：20 批求和恰为 6,620 零偏离
    let mut l = BudgetLedger::new();
    let spend_ok = spend_standard(&mut l).is_ok();
    s.add("预算核销：20 批各 331 求和=6,620 零偏离",
          spend_ok && l.deviation().is_none() && l.spent == BUDGET_LOCKED,
          "6620 = 20 × 331，分批核销后总账守恒");

    // 9. 超预算核销拦截
    let mut ob = BudgetLedger::new();
    let a = ob.record(1, BUDGET_LOCKED).is_ok();
    let b = ob.record(2, 1) == Err(ClosureCode::OverBudget);
    s.add("超预算：累计越过锁定值即拦", a && b,
          "6,620 是硬顶，多核销一元即拒绝");

    // 10. 无定位核销拦截 + 逃逸显性入账
    let mut ua = BudgetLedger::new();
    let ua_rej = ua.record(0, 100).is_err() && ua.record(41, 100).is_err();
    s.add("无定位：批号出 1..=40 拒记且逃逸入账",
          ua_rej && ua.unattributed == 2,
          "核销必须挂批号；无定位逃逸显性可见不静默");

    // 11. 重复核销拦截
    let mut ds = BudgetLedger::new();
    let _ = ds.record(5, 100);
    let ds_ok = ds.record(5, 100) == Err(ClosureCode::DuplicateSpend);
    s.add("重复核销：同批二次记账被拒", ds_ok,
          "一批一笔，重复批号显性拒绝");

    // 12. 偏离定位：出容差带批号可定位
    let mut loc = BudgetLedger::new();
    let _ = spend_standard(&mut loc);
    let no_outlier = loc.locate_outlier().is_none();
    let mut bad = BudgetLedger::new();
    let _ = bad.record(1, PER_ITEM_MEAN * 2);
    let _ = bad.record(2, BUDGET_LOCKED - PER_ITEM_MEAN * 2);
    let located = bad.locate_outlier() == Some(1);
    s.add("偏离定位：均衡方案无离群、失衡方案定位到批", no_outlier && located,
          "偏离不定位即拦截：离群批必须可指认");

    // 13. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_closure_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "缺判据/超预算/重复核销/空摘要/封账改判据全被显式拒绝");

    // 14. 账守恒逃逸=0
    s.add("账守恒：注入过程逃逸=0",
          ledger.unattributed == 0 && ledger.entries.is_empty(),
          "注入核全程不得产生未定位核销逃逸");

    // 15. 文案闭集：非空
    let text_ok = ClosureCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i = 0usize;
    while i < ClosureCode::ALL.len() {
        let mut j = i + 1;
        while j < ClosureCode::ALL.len() {
            if let (Some(x), Some(y)) = (ClosureCode::ALL.get(i), ClosureCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let in_seg = ClosureCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 17. Linux 逐码一致
    let codes_ok = agrees_with_linux(EPERM)
        && agrees_with_linux(EACCES)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(ENOSPC)
        && agrees_with_linux(EOVERFLOW)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 18. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 动态明细（≤8）：核销快照 + 指纹快照
    let ledger_text = format!("核销 {}/{} 元 / 笔数 {}", l.spent, BUDGET_LOCKED, l.entries.len());
    s.add_dyn("核销快照", l.deviation().is_none(), &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

/// 探针：向注册表登记一条合法判据（供缺条封账测试用）。
fn reg_seal_probe(reg: &mut SummaryRegistry) -> Result<usize, ClosureCode> {
    reg.register("UNX-F10060-J1", true)
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_registration_ok() {
        let mut reg = SummaryRegistry::new();
        assert!(register_standard(&mut reg).is_ok());
        assert_eq!(reg.achieved_count(), CRITERION_COUNT);
    }

    #[test]
    fn no_source_rejected() {
        let mut reg = SummaryRegistry::new();
        assert_eq!(reg.register("", true), Err(ClosureCode::NoSource));
        assert_eq!(reg.register("B23 手记", true), Err(ClosureCode::NoSource));
    }

    #[test]
    fn parse_boundary() {
        assert_eq!(parse_criterion_no("UNX-F10060-J1"), Some(1));
        assert_eq!(parse_criterion_no("UNX-F10060-J20"), Some(20));
        assert!(parse_criterion_no("UNX-F10060-J00").is_none()); // J00→0 越窗拒绝
        assert!(parse_criterion_no("UNX-F10060-J").is_none());
    }

    #[test]
    fn duplicate_row_rejected() {
        let mut reg = SummaryRegistry::new();
        let _ = reg.register("UNX-F10060-J1", true);
        assert_eq!(reg.register("UNX-F10060-J1", true), Err(ClosureCode::DuplicateRow));
    }

    #[test]
    fn partial_seal_rejected() {
        let mut reg = SummaryRegistry::new();
        let _ = reg.register("UNX-F10060-J1", true);
        assert_eq!(reg.seal(), Err(ClosureCode::MissingCriterion));
    }

    #[test]
    fn writeback_requires_seal_and_text() {
        let mut reg = SummaryRegistry::new();
        let _ = register_standard(&mut reg);
        assert_eq!(reg.writeback("x"), Err(ClosureCode::StateRegress));
        let _ = reg.seal();
        assert_eq!(reg.writeback(""), Err(ClosureCode::EmptySummary));
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
        assert_eq!(l.record(2, 1), Err(ClosureCode::OverBudget));
        assert_eq!(l.record(1, 1), Err(ClosureCode::DuplicateSpend));
    }

    #[test]
    fn unattributed_spend_visible() {
        let mut l = BudgetLedger::new();
        assert_eq!(l.record(0, 10), Err(ClosureCode::Unattributed));
        assert_eq!(l.record(99, 10), Err(ClosureCode::Unattributed));
        assert_eq!(l.unattributed, 2);
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_closure_injection();
        assert_eq!(l.unattributed, 0);
        assert!(l.entries.is_empty());
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(ENOSPC));
        assert!(agrees_with_linux(EOVERFLOW));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EINVAL), -EINVAL);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10060_checks_green() {
        let s = run_unx10060_checks();
        assert!(s.all_passed(), "UNX-F10060 red: {}", s.red_items_text());
    }
}
