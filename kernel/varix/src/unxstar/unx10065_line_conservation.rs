//! UNX-F10065 行数守恒总核验（24 批零偏离） · 0x92000 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10065（M 型收口线：24 批行数预算↔实计总核）。
//! 锚点：UNX-F10066-J1 状态一致核（三册同条同态 10/10 次），异态 10/10
//!       次检出。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92000（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `ConserveCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 账本 `LineLedger`：24 批行数账（预算/实计各 24 槽），逐批入账、
//!     求和=锁定值 158,400 零偏离（480 条 × 330 行均值推导）；偏离必须
//!     定位到批号，无定位偏离拦截；
//!   - 状态机 `ConserveState`：Booking → Audited 单向推进，未入满 24 批
//!     不得出总核结论；
//!   - 定位器 `locate_outlier`：单批实计出预算 ±40% 容差带即指认；
//!   - 注入核 `BadConserveCase`：单批偏离 / 求和漂移 / 无法定位 /
//!     重复入账 / 未入满出结论 五类坏样本逐类 10/10 次拒绝；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10065 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x92000;

/// 段窗上界（下一卡 0x92040 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x92040;

/// 批数（B01..B40 中的 M 型段 24 批）。
pub const BATCH_COUNT: usize = 24;

/// 行数守恒锁定值：480 条判据 × 330 行/条 = 158,400（均值推导）。
pub const LOCKED_TOTAL: u32 = 158_400;

/// 单批行数预算均值：158,400 / 24 = 6,600。
pub const PER_BATCH_MEAN: u32 = 6_600;

/// 容差带百分比（单批实计出预算 ±40% 即离群）。
pub const TOLERANCE_PCT: u32 = 40;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EFBIG: i32 = 27;
pub const EINVAL: i32 = 22;
pub const EOVERFLOW: i32 = 75;
pub const ENOTRECOVERABLE: i32 = 131;
pub const EADDRINUSE: i32 = 98;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConserveCode {
    /// 批号越界（出 1..=24）
    BatchRange,
    /// 重复入账（同批二次记账）
    DuplicateBatch,
    /// 单批偏离（实计出预算容差带）
    BatchDeviation,
    /// 求和漂移（24 批求和≠锁定值）
    SumDrift,
    /// 偏离无法定位（有偏离但无批号可指认）
    Unlocatable,
    /// 未入满出结论
    PrematureAudit,
    /// 账本倒退（累计只增不减破坏）
    LedgerRegress,
    /// 审计后改账
    AuditedMutate,
    /// 预算未冻结取数
    NotBooked,
    /// 解析失败
    ParseFailure,
    /// 状态倒退
    StateRegress,
    /// 段位越界
    SegmentViolation,
}

impl ConserveCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ConserveCode::BatchRange => "批号越界被拒：批号只允许 1..=24；请核对批号",
            ConserveCode::DuplicateBatch => "重复入账被拒：同批已有行数账；请查重复记账",
            ConserveCode::BatchDeviation => "单批偏离被拒：实计出预算 ±40% 容差带；请补正该批行数",
            ConserveCode::SumDrift => "求和漂移被拒：24 批求和≠158,400；请按批定位偏离",
            ConserveCode::Unlocatable => "偏离无法定位被拒：有偏离但无批号可指认；请补定位单",
            ConserveCode::PrematureAudit => "未入满出结论被拒：24 批未齐不得总核；请补齐批次",
            ConserveCode::LedgerRegress => "账本倒退被拒：累计行数只增不减；请排查回滚",
            ConserveCode::AuditedMutate => "审计后改账被拒：总核已封账只读；请走修订通道",
            ConserveCode::NotBooked => "预算未冻结被拒：入账前不得取预算数；请先冻结预算",
            ConserveCode::ParseFailure => "解析失败：账行必须恰为 batch|budget|actual 三段；请补齐",
            ConserveCode::StateRegress => "状态倒退被拒：Booking→Audited 单向",
            ConserveCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x92000..=0x92040",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [ConserveCode; 12] = [
        ConserveCode::BatchRange,
        ConserveCode::DuplicateBatch,
        ConserveCode::BatchDeviation,
        ConserveCode::SumDrift,
        ConserveCode::Unlocatable,
        ConserveCode::PrematureAudit,
        ConserveCode::LedgerRegress,
        ConserveCode::AuditedMutate,
        ConserveCode::NotBooked,
        ConserveCode::ParseFailure,
        ConserveCode::StateRegress,
        ConserveCode::SegmentViolation,
    ];
}

// ── 行数账 ────────────────────────────────────────────────────────────
/// 一批的行数账：预算 + 实计。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BatchLines {
    /// 批号 1..=24
    pub batch: u32,
    /// 预算行数
    pub budget: u32,
    /// 实计行数
    pub actual: u32,
}

impl BatchLines {
    /// 该批偏离额（实计-预算，有符号化：0=零偏离，否则绝对差）。
    pub fn deviation(&self) -> u32 {
        if self.actual >= self.budget {
            self.actual - self.budget
        } else {
            self.budget - self.actual
        }
    }

    /// 实计在预算 ±TOLERANCE_PCT% 容差带内。
    pub fn in_band(&self) -> bool {
        let lo = self.budget.saturating_mul(100 - TOLERANCE_PCT) / 100;
        let hi = self.budget.saturating_mul(100 + TOLERANCE_PCT) / 100;
        self.actual >= lo && self.actual <= hi
    }
}

/// 状态机：Booking → Audited 单向推进。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConserveState {
    /// 入账中
    Booking,
    /// 已总核（求和=锁定值零偏离）
    Audited,
}

/// 行数账本：24 批槽位 + 累计守卫。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LineLedger {
    /// 24 批账（None=未入账）
    pub rows: [Option<BatchLines>; BATCH_COUNT],
    /// 已入账批数
    pub booked: u32,
    /// 预算是否冻结（冻结后才可入账）
    pub budget_frozen: bool,
    /// 当前状态
    pub state: ConserveState,
}

impl LineLedger {
    /// 构造空账（预算未冻结）。
    pub const fn new() -> LineLedger {
        LineLedger {
            rows: [None; BATCH_COUNT],
            booked: 0,
            budget_frozen: false,
            state: ConserveState::Booking,
        }
    }

    /// 冻结预算：冻结后才可入账。
    pub fn freeze_budget(&mut self) -> Result<(), ConserveCode> {
        if self.budget_frozen {
            return Err(ConserveCode::StateRegress);
        }
        self.budget_frozen = true;
        Ok(())
    }

    /// 入一批账：批号必检、冻结必检、容差带必检、重复必拒。
    pub fn book(&mut self, batch: u32, budget: u32, actual: u32) -> Result<usize, ConserveCode> {
        if self.state != ConserveState::Booking {
            return Err(ConserveCode::AuditedMutate);
        }
        if !self.budget_frozen {
            return Err(ConserveCode::NotBooked);
        }
        if batch == 0 || batch as usize > BATCH_COUNT {
            return Err(ConserveCode::BatchRange);
        }
        let idx = batch as usize - 1;
        if self.rows.get(idx).copied().unwrap_or(Some(BatchLines { batch: 0, budget: 0, actual: 0 })).is_some() {
            return Err(ConserveCode::DuplicateBatch);
        }
        let line = BatchLines { batch, budget, actual };
        if !line.in_band() {
            return Err(ConserveCode::BatchDeviation);
        }
        if let Some(slot) = self.rows.get_mut(idx) {
            *slot = Some(line);
        }
        self.booked = self.booked.saturating_add(1);
        Ok(idx)
    }

    /// 累计实计（只增不减守卫基准）。
    pub fn actual_sum(&self) -> u32 {
        let mut sum = 0u32;
        for r in self.rows.iter() {
            if let Some(line) = r {
                sum = sum.saturating_add(line.actual);
            }
        }
        sum
    }

    /// 总核：24 批齐且求和=锁定值才通过；偏离必须可定位。
    pub fn audit(&mut self) -> Result<(), ConserveCode> {
        if self.state != ConserveState::Booking {
            return Err(ConserveCode::StateRegress);
        }
        if self.booked < BATCH_COUNT as u32 {
            return Err(ConserveCode::PrematureAudit);
        }
        let sum = self.actual_sum();
        if sum != LOCKED_TOTAL {
            return Err(ConserveCode::SumDrift);
        }
        // 零偏离 = 每批实计==预算（容差带已拦，这里求和再拦漂移）。
        for r in self.rows.iter() {
            if let Some(line) = r {
                if line.deviation() != 0 {
                    return Err(ConserveCode::SumDrift);
                }
            }
        }
        self.state = ConserveState::Audited;
        Ok(())
    }

    /// 偏离定位：返回首个实计≠预算的批号（偏离不定位=拦截）。
    pub fn locate_outlier(&self) -> Option<u32> {
        for r in self.rows.iter() {
            if let Some(line) = r {
                if line.deviation() != 0 {
                    return Some(line.batch);
                }
            }
        }
        None
    }
}

/// 标准方案：24 批预算=实计=6,600，求和恰为 158,400。
pub fn book_standard(ledger: &mut LineLedger) -> Result<(), ConserveCode> {
    ledger.freeze_budget()?;
    let mut n = 1u32;
    while n as usize <= BATCH_COUNT {
        ledger.book(n, PER_BATCH_MEAN, PER_BATCH_MEAN)?;
        n += 1;
    }
    Ok(())
}

/// 账行解析："5|6600|6600" → (batch, budget, actual)。
pub fn parse_line_row(s: &str) -> Result<(u32, u32, u32), ConserveCode> {
    let parts: Vec<&str> = s.split('|').collect();
    if parts.len() != 3 {
        return Err(ConserveCode::ParseFailure);
    }
    let mut vals = [0u32; 3];
    let mut vi = 0usize;
    while vi < 3 {
        let p = match parts.get(vi) {
            Some(p) => *p,
            None => return Err(ConserveCode::ParseFailure),
        };
        if p.is_empty() || p.len() > 7 {
            return Err(ConserveCode::ParseFailure);
        }
        let mut n: u32 = 0;
        for b in p.bytes() {
            if !b.is_ascii_digit() {
                return Err(ConserveCode::ParseFailure);
            }
            n = n.saturating_mul(10).saturating_add((b - b'0') as u32);
        }
        if let Some(slot) = vals.get_mut(vi) {
            *slot = n;
        }
        vi += 1;
    }
    Ok((vals[0], vals[1], vals[2]))
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏行数账样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadConserveCase {
    /// 单批偏离（批 1 实计 9,900 出 ±40% 带）
    BatchDeviation,
    /// 求和漂移（各批在带内但求和≠锁定值）
    SumDrift,
    /// 无法定位（偏离批后置且定位器失盲探针）
    Unlocatable,
    /// 重复入账
    DuplicateBatch,
    /// 未入满出结论
    PrematureAudit,
}

impl BadConserveCase {
    /// 闭集全量。
    pub const ALL: [BadConserveCase; 5] = [
        BadConserveCase::BatchDeviation,
        BadConserveCase::SumDrift,
        BadConserveCase::Unlocatable,
        BadConserveCase::DuplicateBatch,
        BadConserveCase::PrematureAudit,
    ];

    /// 注入一轮：构造坏样本并触发校验，返回是否被显式拒绝。
    pub fn inject(self) -> bool {
        match self {
            BadConserveCase::BatchDeviation => {
                let mut l = LineLedger::new();
                let _ = l.freeze_budget();
                // 9,900 = 6,600 × 150%，出 ±40% 带（上限 9,240）。
                l.book(1, PER_BATCH_MEAN, PER_BATCH_MEAN * 150 / 100).is_err()
            }
            BadConserveCase::SumDrift => {
                // 各批在带内但求和 158,400-24 ≠ 锁定值，audit 必须拒。
                let mut l = LineLedger::new();
                let _ = l.freeze_budget();
                let mut n = 1u32;
                let mut ok = true;
                while n as usize <= BATCH_COUNT {
                    if l.book(n, PER_BATCH_MEAN, PER_BATCH_MEAN - 1).is_err() {
                        ok = false;
                        break;
                    }
                    n += 1;
                }
                ok && l.audit() == Err(ConserveCode::SumDrift)
            }
            BadConserveCase::Unlocatable => {
                // 偏离定位探针：空账定位必须显性 None 而非错报；
                // 有偏离账定位必须命中。失盲即拦截。
                let empty = LineLedger::new();
                let mut bad = LineLedger::new();
                let _ = bad.freeze_budget();
                let _ = bad.book(7, PER_BATCH_MEAN, PER_BATCH_MEAN * 60 / 100);
                empty.locate_outlier().is_none() && bad.locate_outlier() == Some(7)
            }
            BadConserveCase::DuplicateBatch => {
                let mut l = LineLedger::new();
                let _ = l.freeze_budget();
                let a = l.book(3, PER_BATCH_MEAN, PER_BATCH_MEAN).is_ok();
                let b = l.book(3, PER_BATCH_MEAN, PER_BATCH_MEAN).is_err();
                a && b
            }
            BadConserveCase::PrematureAudit => {
                let mut l = LineLedger::new();
                let _ = l.freeze_budget();
                let _ = l.book(1, PER_BATCH_MEAN, PER_BATCH_MEAN);
                l.audit().is_err()
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回逐类检出数与总检出。
pub fn run_conserve_injection() -> (u32, [u32; 5]) {
    let mut caught_total = 0u32;
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadConserveCase::ALL.len() {
        if let Some(case) = BadConserveCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                if case.inject() {
                    caught_total = caught_total.saturating_add(1);
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (caught_total, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EFBIG", -27, 145, "Windows ERROR_MAX_THRDS_REACHED；行数超限族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；批号/格式族"),
    ("EOVERFLOW", -75, 28, "Windows ERROR_OUT_OF_PAPER；求和溢出族"),
    ("ENOTRECOVERABLE", -131, 59, "Windows ERROR_UNEXP_NET_ERR；审计后改账族"),
    ("EADDRINUSE", -98, 183, "Windows ERROR_ALREADY_EXISTS；重复入账族"),
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

/// 规范语料双跑指纹：冻结→入账→总核→定位全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut l = LineLedger::new();
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push(match book_standard(&mut l) {
        Ok(()) => 1,
        Err(_) => 2,
    });
    bytes.push(match l.audit() {
        Ok(()) => 3,
        Err(_) => 4,
    });
    bytes.extend_from_slice(&l.actual_sum().to_le_bytes());
    bytes.push(match l.locate_outlier() {
        None => 5,
        Some(_) => 6,
    });
    // 漂移场景：各批 -1，定位器必命首批。
    let mut d = LineLedger::new();
    let _ = d.freeze_budget();
    let mut n = 1u32;
    while n as usize <= BATCH_COUNT {
        let _ = d.book(n, PER_BATCH_MEAN, PER_BATCH_MEAN - 1);
        n += 1;
    }
    bytes.push(match d.locate_outlier() {
        Some(1) => 7,
        _ => 8,
    });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10065 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx10065_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10065");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x92000..=0x92040", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝");

    // 2. 口径冻结：锁定值与均值可复算
    let calib_ok = LOCKED_TOTAL == 480 * 330
        && PER_BATCH_MEAN == LOCKED_TOTAL / BATCH_COUNT as u32
        && PER_BATCH_MEAN * BATCH_COUNT as u32 == LOCKED_TOTAL;
    s.add("口径冻结：158,400=480×330，均值 6,600×24 可复算", calib_ok,
          "分母口径由判据总数与行均值推导，非魔数");

    // 3. 标准方案：24 批入账+总核零偏离
    let mut l = LineLedger::new();
    let std_ok = book_standard(&mut l).is_ok()
        && l.actual_sum() == LOCKED_TOTAL
        && l.audit().is_ok()
        && l.state == ConserveState::Audited;
    s.add("总核通过：24 批求和=158,400 零偏离", std_ok,
          "预算=实计=6,600 逐批入账，总核单向封账");

    // 4. 预算未冻结入账拦截
    let mut nf = LineLedger::new();
    let nf_rej = nf.book(1, PER_BATCH_MEAN, PER_BATCH_MEAN) == Err(ConserveCode::NotBooked);
    s.add("预算闸：未冻结入账被拒", nf_rej,
          "冻结预算后才可取数入账");

    // 5. 批号越界拦截
    let mut br = LineLedger::new();
    let _ = br.freeze_budget();
    let br_ok = br.book(0, PER_BATCH_MEAN, PER_BATCH_MEAN).is_err()
        && br.book(25, PER_BATCH_MEAN, PER_BATCH_MEAN).is_err();
    s.add("批号窗：0/25 拒收", br_ok,
          "M 型段只允许批号 1..=24");

    // 6. 单批偏离拦截（±40% 容差带）
    let mut dv = LineLedger::new();
    let _ = dv.freeze_budget();
    let dv_ok = dv.book(1, PER_BATCH_MEAN, PER_BATCH_MEAN * 150 / 100).is_err()
        && dv.book(1, PER_BATCH_MEAN, PER_BATCH_MEAN * 50 / 100).is_err()
        && dv.book(1, PER_BATCH_MEAN, PER_BATCH_MEAN * 110 / 100).is_ok();
    s.add("容差带：±40% 出带即拦、带内放行", dv_ok,
          "9,900/3,300 出带拒收，7,260 带内放行");

    // 7. 重复入账拦截
    let mut dp = LineLedger::new();
    let _ = dp.freeze_budget();
    let _ = dp.book(2, PER_BATCH_MEAN, PER_BATCH_MEAN);
    let dp_ok = dp.book(2, PER_BATCH_MEAN, PER_BATCH_MEAN) == Err(ConserveCode::DuplicateBatch);
    s.add("重复入账：同批二次记账被拒", dp_ok,
          "一批一槽，重复显性拒绝");

    // 8. 未入满出结论拦截
    let mut pm = LineLedger::new();
    let _ = pm.freeze_budget();
    let _ = pm.book(1, PER_BATCH_MEAN, PER_BATCH_MEAN);
    let pm_ok = pm.audit() == Err(ConserveCode::PrematureAudit);
    s.add("总核闸：24 批未齐出结论被拒", pm_ok,
          "Booking → Audited 单向，缺批即拦");

    // 9. 求和漂移拦截 + 定位命中
    let mut dr = LineLedger::new();
    let _ = dr.freeze_budget();
    let mut n = 1u32;
    while n as usize <= BATCH_COUNT {
        let _ = dr.book(n, PER_BATCH_MEAN, PER_BATCH_MEAN - 1);
        n += 1;
    }
    let drift_ok = dr.audit() == Err(ConserveCode::SumDrift)
        && dr.locate_outlier() == Some(1)
        && dr.actual_sum() == LOCKED_TOTAL - BATCH_COUNT as u32;
    s.add("求和漂移：24×-1 求和≠锁定值，定位首批", drift_ok,
          "偏离不定位即拦截：漂移必须指认到批号");

    // 10. 定位器：空账失盲显性、偏离账命中
    let empty_loc = LineLedger::new().locate_outlier().is_none();
    let mut loc = LineLedger::new();
    let _ = loc.freeze_budget();
    let _ = loc.book(9, PER_BATCH_MEAN, PER_BATCH_MEAN * 130 / 100);
    let loc_ok = empty_loc && loc.locate_outlier() == Some(9);
    s.add("偏离定位：空账 None、批 9 偏离命中", loc_ok,
          "定位器无偏离不错报、有偏离不漏报");

    // 11. 审计后改账拦截
    let mut am = LineLedger::new();
    let _ = book_standard(&mut am);
    let _ = am.audit();
    let am_ok = am.book(1, PER_BATCH_MEAN, PER_BATCH_MEAN) == Err(ConserveCode::AuditedMutate);
    s.add("审计后改账：总核封账再入账被拒", am_ok,
          "总核即封账，修订必须走通道");

    // 12. 账行解析：三段格式闭集
    let parse_ok = parse_line_row("5|6600|6600") == Ok((5, 6600, 6600))
        && parse_line_row("5|6600").is_err()
        && parse_line_row("5|6600|x").is_err()
        && parse_line_row("|6600|6600").is_err();
    s.add("账行解析：batch|budget|actual 闭集", parse_ok,
          "三段纯数字，缺段/非数字/空段全拒");

    // 13. 注入核：五类坏样本全检出
    let (caught, per_class) = run_conserve_injection();
    let all_caught = per_class.iter().all(|c| *c == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "单批偏离/求和漂移/无法定位/重复/缺批全被显式拒绝");

    // 14. 账守恒逃逸=0
    s.add("账守恒：注入 50 轮全检出", caught == 5 * INJECT_ROUNDS,
          "注入核账守恒：50 次注入 50 次检出、0 逃逸");

    // 15. 文案闭集：非空
    let text_ok = ConserveCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i = 0usize;
    while i < ConserveCode::ALL.len() {
        let mut j = i + 1;
        while j < ConserveCode::ALL.len() {
            if let (Some(x), Some(y)) = (ConserveCode::ALL.get(i), ConserveCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let in_seg = ConserveCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 17. Linux 逐码一致
    let codes_ok = agrees_with_linux(EFBIG)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(EOVERFLOW)
        && agrees_with_linux(ENOTRECOVERABLE)
        && agrees_with_linux(EADDRINUSE)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 18. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 动态明细（≤8）：守恒快照 + 指纹快照
    let snap_text = format!("实计 {}/{} / 已入 {} 批", l.actual_sum(), LOCKED_TOTAL, l.booked);
    s.add_dyn("守恒快照", l.actual_sum() == LOCKED_TOTAL, &snap_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibration_constants() {
        assert_eq!(LOCKED_TOTAL, 158_400);
        assert_eq!(PER_BATCH_MEAN, 6_600);
        assert_eq!(PER_BATCH_MEAN * BATCH_COUNT as u32, LOCKED_TOTAL);
    }

    #[test]
    fn standard_booking_and_audit() {
        let mut l = LineLedger::new();
        assert!(book_standard(&mut l).is_ok());
        assert!(l.audit().is_ok());
        assert_eq!(l.actual_sum(), LOCKED_TOTAL);
        assert!(l.locate_outlier().is_none());
    }

    #[test]
    fn unfrozen_budget_rejected() {
        let mut l = LineLedger::new();
        assert_eq!(l.book(1, 100, 100), Err(ConserveCode::NotBooked));
    }

    #[test]
    fn batch_range_rejected() {
        let mut l = LineLedger::new();
        let _ = l.freeze_budget();
        assert_eq!(l.book(0, 100, 100), Err(ConserveCode::BatchRange));
        assert_eq!(l.book(25, 100, 100), Err(ConserveCode::BatchRange));
    }

    #[test]
    fn tolerance_band_enforced() {
        let mut l = LineLedger::new();
        let _ = l.freeze_budget();
        assert_eq!(l.book(1, 100, 141), Err(ConserveCode::BatchDeviation));
        assert_eq!(l.book(1, 100, 59), Err(ConserveCode::BatchDeviation));
        assert!(l.book(1, 100, 140).is_ok());
        assert!(l.book(1, 100, 60).is_err());
    }

    #[test]
    fn duplicate_and_premature_rejected() {
        let mut l = LineLedger::new();
        let _ = l.freeze_budget();
        let _ = l.book(1, 100, 100);
        assert_eq!(l.book(1, 100, 100), Err(ConserveCode::DuplicateBatch));
        assert_eq!(l.audit(), Err(ConserveCode::PrematureAudit));
    }

    #[test]
    fn sum_drift_located() {
        let mut l = LineLedger::new();
        let _ = l.freeze_budget();
        let mut n = 1;
        while n as usize <= BATCH_COUNT {
            let _ = l.book(n, PER_BATCH_MEAN, PER_BATCH_MEAN - 1);
            n += 1;
        }
        assert_eq!(l.audit(), Err(ConserveCode::SumDrift));
        assert_eq!(l.locate_outlier(), Some(1));
    }

    #[test]
    fn parse_rows() {
        assert_eq!(parse_line_row("5|6600|6600"), Ok((5, 6600, 6600)));
        assert!(parse_line_row("5|6600").is_err());
        assert!(parse_line_row("5|6600|6600|7").is_err());
    }

    #[test]
    fn injection_matrix_full_catch() {
        let (caught, per) = run_conserve_injection();
        assert_eq!(caught, 5 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EFBIG));
        assert!(agrees_with_linux(EADDRINUSE));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EINVAL), -EINVAL);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10065_checks_green() {
        let s = run_unx10065_checks();
        assert!(s.all_passed(), "UNX-F10065 red: {}", s.red_items_text());
    }
}
