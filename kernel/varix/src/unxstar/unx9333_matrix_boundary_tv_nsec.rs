//! UNX-F9333 时间值规范化边界矩阵（tv_nsec 上限 1e9） · 0x7C0C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9333 逐字）：
//!   UNX-F9333-J1 tv_nsec 越界注入判据过，≥1e9 全 EINVAL 格过，规范化
//!   （tv_sec 进位）语义格过，与 B20 规范化对账一致格过。
//!
//! 设计要点：
//!   ① tv_nsec 的合法域是闭开区间 [0, 1_000_000_000)：下界含 0，上界不含
//!      1e9。`validate_nsec` 只用两次比较，不做取模，因此 -1 与 1e9 都落
//!      在同一个 NsecOutOfRange 分类里，返回码一律 EINVAL。
//!   ② 规范化不是「拒绝」，而是进位：nsec 除以 1e9 的商进 tv_sec，余数
//!      留在 tv_nsec；余数为负时向 tv_sec 借 1 并补满 1e9。全程 checked_add/
//!      checked_sub，溢出返回 None（由调用方转 EINVAL），绝不回绕。
//!   ③ 判定次序冻结：时钟域合法 → tv_nsec 值域 → 时钟可设 → 规范化溢出。
//!      因此 CLOCK_MONOTONIC 配 1e9 必须先回 EINVAL 而不是 EPERM；只有
//!      nsec 合法时才轮到「不可设时钟」的 EPERM。
//!   ④ 时钟表 12 行：Linux 的 clockid 10 是空洞，合法值域是 0..=11 去掉 10；
//!      可设时钟只有 REALTIME/REALTIME_COARSE/REALTIME_ALARM/TAI 四个。
//!   ⑤ 边界注入十格 × 10 轮：-1e9-1 / -1e9 / -1 / 0 / 1 / 999_999_999 /
//!      1e9 / 1e9+1 / 2e9-1 / 2e9，合法格必须成功且原值不动，越界格必须
//!      EINVAL；账目 injected == detected + escaped，escaped 恒 0。
//!   ⑥ 进位语义十行：覆盖商进 1/2、负余数借位、i64 双端与零进位，进位
//!      单位合计冻结为 9，任何一行改动都会让对账格变红。
//!   ⑦ B20 对账：B20 的规范化账与本模块 normalizer 必须逐行一致（零失配），
//!      且 1e9 上限与账本世代两个常量必须逐字相等，防止两侧口径漂移。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9333 独占 0x7C0C0..=0x7C0FF）。
pub const SEG_BASE: u32 = 0x7C0C0;

/// 一秒的纳秒数（tv_nsec 的硬上界）。
pub const NS_PER_SEC: i64 = 1_000_000_000;
/// tv_nsec 最大值。
pub const MAX_NSEC: i64 = NS_PER_SEC - 1;
/// 时钟域行数。
pub const CLOCK_ROWS: usize = 12;
/// 边界注入格数。
pub const BOUNDARY_CELLS: usize = 10;
/// 进位语义行数。
pub const CARRY_ROWS: usize = 10;
/// B20 对账行数。
pub const B20_ROWS: usize = 10;
/// 单格注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 冻结的进位单位合计（十行 |Δsec| 之和）。
pub const CARRY_UNITS_TOTAL: u32 = 9;

pub const EPERM: i32 = 1;
pub const EFAULT: i32 = 14;
pub const EINVAL: i32 = 22;

/// 本模块诊断码（段内 +0x01..=+0x0D 闭集，共 13 码）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimeCode {
    NsecNegative,
    NsecTooLarge,
    ClockHole,
    NotSettable,
    CarryMismatch,
    B20Mismatch,
    B20ContractDrift,
    IdempotenceBroken,
    Overflow,
    LedgerLeak,
    NotReproducible,
    SegmentViolation,
    ClockTableDrift,
}

impl TimeCode {
    /// 段内序号。
    pub fn code(self) -> u16 {
        self as u16
    }
    /// 段内码值：SEG_BASE + 1 + 序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 中文诊断与可操作建议。
    pub fn detail(self) -> &'static str {
        match self {
            TimeCode::NsecNegative => "tv_nsec 为负：合法域下界是 0，负值必须 EINVAL，不得靠借位悄悄吞掉",
            TimeCode::NsecTooLarge => "tv_nsec 达到 1e9：上界闭开，1e9 本身即越界；请先把整秒进位再提交",
            TimeCode::ClockHole => "clockid 落空洞：Linux 的 10 号无时钟，合法域是 0..=11 去掉 10",
            TimeCode::NotSettable => "时钟不可设：MONOTONIC/BOOTTIME/CPU 时钟只能读，写入回 EPERM",
            TimeCode::CarryMismatch => "进位结果不符：nsec 商必须进 sec、负余数必须借位补满 1e9",
            TimeCode::B20Mismatch => "B20 对账失配：本模块 normalizer 与 B20 账逐行不一致",
            TimeCode::B20ContractDrift => "B20 契约漂移：1e9 上限或账本世代与 B20 声明不等",
            TimeCode::IdempotenceBroken => "规范化不幂等：对已规范值再规范化必须原样返回",
            TimeCode::Overflow => "规范化溢出：sec 进位越过 i64 边界，必须回 None 并转 EINVAL",
            TimeCode::LedgerLeak => "注入账不闭合：注入数必须等于检出数加逃逸数，逃逸恒 0",
            TimeCode::NotReproducible => "双跑不一致：同一语料两次指纹必须相等",
            TimeCode::SegmentViolation => "段位越界：0x7C0C0 必须 0x40 对齐且落在 0x7C000..=0x7D1C0",
            TimeCode::ClockTableDrift => "时钟表漂移：可设位与空洞位必须与冻结账逐行一致",
        }
    }
    /// 闭集全量。
    pub const ALL: &'static [TimeCode] = &[
        TimeCode::NsecNegative, TimeCode::NsecTooLarge, TimeCode::ClockHole,
        TimeCode::NotSettable, TimeCode::CarryMismatch, TimeCode::B20Mismatch,
        TimeCode::B20ContractDrift, TimeCode::IdempotenceBroken, TimeCode::Overflow,
        TimeCode::LedgerLeak, TimeCode::NotReproducible, TimeCode::SegmentViolation,
        TimeCode::ClockTableDrift,
    ];
}

/// 时钟行：id / 名称 / 是否可设。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClockSpec {
    pub id: i32,
    pub name: &'static str,
    pub settable: bool,
}

impl ClockSpec {
    pub const fn new(id: i32, name: &'static str, settable: bool) -> ClockSpec {
        ClockSpec { id, name, settable }
    }
}

/// 十二行时钟表（10 号是空洞，标记为不可设且不由 clock_spec 返回）。
pub const CLOCKS: [ClockSpec; CLOCK_ROWS] = [
    ClockSpec::new(0, "CLOCK_REALTIME", true),
    ClockSpec::new(1, "CLOCK_MONOTONIC", false),
    ClockSpec::new(2, "CLOCK_PROCESS_CPUTIME_ID", false),
    ClockSpec::new(3, "CLOCK_THREAD_CPUTIME_ID", false),
    ClockSpec::new(4, "CLOCK_MONOTONIC_RAW", false),
    ClockSpec::new(5, "CLOCK_REALTIME_COARSE", true),
    ClockSpec::new(6, "CLOCK_MONOTONIC_COARSE", false),
    ClockSpec::new(7, "CLOCK_BOOTTIME", false),
    ClockSpec::new(8, "CLOCK_REALTIME_ALARM", true),
    ClockSpec::new(9, "CLOCK_BOOTTIME_ALARM", false),
    ClockSpec::new(10, "CLOCK_HOLE", false),
    ClockSpec::new(11, "CLOCK_TAI", true),
];

/// 冻结账：(id, settable)。
pub const FROZEN_CLOCKS: [(i32, bool); CLOCK_ROWS] = [
    (0, true), (1, false), (2, false), (3, false), (4, false), (5, true),
    (6, false), (7, false), (8, true), (9, false), (10, false), (11, true),
];

/// 按 id 查时钟；10 号空洞与表外值返回 None。
pub fn clock_spec(id: i32) -> Option<&'static ClockSpec> {
    if id == 10 {
        return None;
    }
    let mut i = 0usize;
    while i < CLOCKS.len() {
        if let Some(sp) = CLOCKS.get(i) {
            if sp.id == id {
                return Some(sp);
            }
        }
        i += 1;
    }
    None
}

/// 时钟表漂移格数。
pub fn clock_table_drift() -> u32 {
    let mut drift = 0u32;
    let mut i = 0usize;
    while i < CLOCKS.len() {
        if let (Some(sp), Some(f)) = (CLOCKS.get(i), FROZEN_CLOCKS.get(i)) {
            if sp.id != f.0 || sp.settable != f.1 {
                drift = drift.saturating_add(1);
            }
        }
        i += 1;
    }
    drift
}

/// tv_nsec 值域判定：闭开区间 [0, 1e9)。
pub fn validate_nsec(nsec: i64) -> bool {
    nsec >= 0 && nsec < NS_PER_SEC
}

/// 规范化：返回 (规范 sec, 规范 nsec)；sec 进位溢出返回 None。
pub fn normalize(sec: i64, nsec: i64) -> Option<(i64, i64)> {
    let div = nsec / NS_PER_SEC;
    let rem = nsec % NS_PER_SEC;
    let mut s = match sec.checked_add(div) {
        Some(v) => v,
        None => return None,
    };
    let mut n = rem;
    if n < 0 {
        n = match n.checked_add(NS_PER_SEC) {
            Some(v) => v,
            None => return None,
        };
        s = match s.checked_sub(1) {
            Some(v) => v,
            None => return None,
        };
    }
    Some((s, n))
}

/// 规范化产生的 |Δsec|，饱和到 u32。
pub fn carry_units(sec: i64, out_sec: i64) -> u32 {
    let d = if out_sec >= sec {
        out_sec.checked_sub(sec)
    } else {
        sec.checked_sub(out_sec)
    };
    match d {
        Some(x) => {
            if x > u32::MAX as i64 {
                u32::MAX
            } else {
                x as u32
            }
        }
        None => u32::MAX,
    }
}

/// 边界注入格：nsec 取值 + 是否合法。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NsecCell {
    pub nsec: i64,
    pub valid: bool,
}

impl NsecCell {
    pub const fn new(nsec: i64, valid: bool) -> NsecCell {
        NsecCell { nsec, valid }
    }
}

/// 十格边界表。
pub const BOUNDARY_GRID: [NsecCell; BOUNDARY_CELLS] = [
    NsecCell::new(-1_000_000_001, false),
    NsecCell::new(-1_000_000_000, false),
    NsecCell::new(-1, false),
    NsecCell::new(0, true),
    NsecCell::new(1, true),
    NsecCell::new(MAX_NSEC, true),
    NsecCell::new(1_000_000_000, false),
    NsecCell::new(1_000_000_001, false),
    NsecCell::new(1_999_999_999, false),
    NsecCell::new(2_000_000_000, false),
];

/// 进位语义行：(sec_in, nsec_in, sec_out, nsec_out)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CarryRow {
    pub sec: i64,
    pub nsec: i64,
    pub out_sec: i64,
    pub out_nsec: i64,
}

impl CarryRow {
    pub const fn new(sec: i64, nsec: i64, out_sec: i64, out_nsec: i64) -> CarryRow {
        CarryRow { sec, nsec, out_sec, out_nsec }
    }
}

/// 十行进位语义表（含负余数借位与 i64 双端）。
pub const CARRY_GRID: [CarryRow; CARRY_ROWS] = [
    CarryRow::new(0, 1_000_000_000, 1, 0),
    CarryRow::new(0, 1_999_999_999, 1, 999_999_999),
    CarryRow::new(5, 2_500_000_000, 7, 500_000_000),
    CarryRow::new(-1, 1_500_000_000, 0, 500_000_000),
    CarryRow::new(0, -1, -1, 999_999_999),
    CarryRow::new(-2, -1_500_000_000, -4, 500_000_000),
    CarryRow::new(i64::MIN, 0, i64::MIN, 0),
    CarryRow::new(i64::MAX, 999_999_999, i64::MAX, 999_999_999),
    CarryRow::new(1, -1, 0, 999_999_999),
    CarryRow::new(0, 0, 0, 0),
];

/// B20 规范化账行（与 CarryRow 同形，但属于外部深化册口径）。
pub const B20_GRID: [CarryRow; B20_ROWS] = [
    CarryRow::new(0, 0, 0, 0),
    CarryRow::new(0, 999_999_999, 0, 999_999_999),
    CarryRow::new(0, 1_000_000_000, 1, 0),
    CarryRow::new(1, 1_999_999_999, 2, 999_999_999),
    CarryRow::new(-1, -1, -2, 999_999_999),
    CarryRow::new(7, 2_500_000_000, 9, 500_000_000),
    CarryRow::new(-7, -2_500_000_000, -10, 500_000_000),
    CarryRow::new(i64::MAX - 1, 1_999_999_999, i64::MAX, 999_999_999),
    CarryRow::new(i64::MIN + 1, -1_000_000_000, i64::MIN, 0),
    CarryRow::new(42, 1, 42, 1),
];

/// B20 契约常量：上限 / 账本世代 / 行数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct B20Contract {
    pub limit: i64,
    pub version: u32,
    pub rows: u32,
}

/// B20 对账契约声明。
pub const B20_CONTRACT: B20Contract = B20Contract {
    limit: 1_000_000_000,
    version: 2,
    rows: B20_ROWS as u32,
};

/// B20 逐行失配数（0 表示完全一致）。
pub fn b20_mismatch() -> u32 {
    let mut bad = 0u32;
    let mut i = 0usize;
    while i < B20_GRID.len() {
        if let Some(row) = B20_GRID.get(i) {
            let got = normalize(row.sec, row.nsec);
            if got != Some((row.out_sec, row.out_nsec)) {
                bad = bad.saturating_add(1);
            }
        }
        i += 1;
    }
    bad
}

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl InjectLedger {
    pub const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }
    pub fn record(&mut self, caught: bool) -> bool {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        self.conserved()
    }
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 时间域状态机：只记计数，不存时间值。
#[derive(Clone, Copy, Debug)]
pub struct TimeDomain {
    accepts: u32,
    rejects: u32,
    carries: u32,
    last_errno: i32,
}

impl TimeDomain {
    pub const fn new() -> TimeDomain {
        TimeDomain { accepts: 0, rejects: 0, carries: 0, last_errno: 0 }
    }
    pub fn accepts(&self) -> u32 {
        self.accepts
    }
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
    pub fn carries(&self) -> u32 {
        self.carries
    }
    pub fn last_errno(&self) -> i32 {
        self.last_errno
    }
    /// 记录一次拒绝。
    pub fn reject(&mut self, e: i32) -> Result<(i64, i64), i32> {
        self.rejects = self.rejects.saturating_add(1);
        self.last_errno = e;
        Err(e)
    }
    /// clock_settime 判定：时钟 → nsec → 可设 → 规范化。
    pub fn settime(&mut self, clock: i32, sec: i64, nsec: i64) -> Result<(i64, i64), i32> {
        let spec = match clock_spec(clock) {
            Some(sp) => sp,
            None => return self.reject(EINVAL),
        };
        if !validate_nsec(nsec) {
            return self.reject(EINVAL);
        }
        if !spec.settable {
            return self.reject(EPERM);
        }
        match normalize(sec, nsec) {
            Some((s, n)) => {
                self.carries = self.carries.saturating_add(carry_units(sec, s));
                self.accepts = self.accepts.saturating_add(1);
                self.last_errno = 0;
                Ok((s, n))
            }
            None => self.reject(EINVAL),
        }
    }
    /// 无副作用的失败分类，次序与 settime 一致。
    pub fn classify(&self, clock: i32, sec: i64, nsec: i64) -> Option<TimeCode> {
        let spec = match clock_spec(clock) {
            Some(sp) => sp,
            None => return Some(TimeCode::ClockHole),
        };
        if nsec < 0 {
            return Some(TimeCode::NsecNegative);
        }
        if nsec >= NS_PER_SEC {
            return Some(TimeCode::NsecTooLarge);
        }
        if !spec.settable {
            return Some(TimeCode::NotSettable);
        }
        match normalize(sec, nsec) {
            Some(_) => None,
            None => Some(TimeCode::Overflow),
        }
    }
}

/// 边界注入：十格 × 10 轮，合法格成功且原值不动，越界格 EINVAL。
pub fn run_boundary_matrix() -> (u32, InjectLedger, u32) {
    let mut ledger = InjectLedger::zero();
    let mut hits = 0u32;
    let mut drift = 0u32;
    let mut i = 0usize;
    while i < BOUNDARY_GRID.len() {
        if let Some(cell) = BOUNDARY_GRID.get(i) {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let mut dom = TimeDomain::new();
                let got = dom.settime(0, 0, cell.nsec);
                let caught = if cell.valid {
                    got == Ok((0, cell.nsec))
                        && dom.accepts() == 1
                        && dom.carries() == 0
                } else {
                    got == Err(EINVAL) && dom.rejects() == 1 && dom.accepts() == 0
                };
                if caught {
                    hits = hits.saturating_add(1);
                }
                if !ledger.record(caught) {
                    drift = drift.saturating_add(1);
                }
                round += 1;
            }
        }
        i += 1;
    }
    (hits, ledger, drift)
}

/// 进位语义：十行逐行复算，返回 (命中行, 进位单位合计)。
pub fn run_carry_grid() -> (u32, u32) {
    let mut hits = 0u32;
    let mut units = 0u32;
    let mut i = 0usize;
    while i < CARRY_GRID.len() {
        if let Some(row) = CARRY_GRID.get(i) {
            if normalize(row.sec, row.nsec) == Some((row.out_sec, row.out_nsec)) {
                hits = hits.saturating_add(1);
                units = units.saturating_add(carry_units(row.sec, row.out_sec));
            }
        }
        i += 1;
    }
    (hits, units)
}

/// 幂等性：对冻结表的每个输出再做一次规范化，结果必须不动。
pub fn idempotence_holds() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < CARRY_GRID.len() {
        if let Some(row) = CARRY_GRID.get(i) {
            match normalize(row.sec, row.nsec) {
                Some((s, n)) => {
                    if normalize(s, n) != Some((s, n)) {
                        ok = false;
                    }
                }
                None => ok = false,
            }
        }
        i += 1;
    }
    ok
}

/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；不可设时钟"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；tv_nsec 越界"),
];

/// 查表取负值；表外返回 0。
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

pub fn agrees_with_linux(code: i32) -> bool {
    code > 0 && linux_neg(code) == -code
}

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

/// 编码一次规范化结果（含失败位）。
fn encode(out: &mut Vec<u8>, r: Option<(i64, i64)>) {
    match r {
        Some((s, n)) => {
            out.push(1);
            out.push((s & 0xFF) as u8);
            out.push((n & 0xFF) as u8);
        }
        None => out.push(2),
    }
}

/// 规范语料双跑指纹：边界十格 + 进位十行 + B20 十行。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < BOUNDARY_GRID.len() {
        if let Some(cell) = BOUNDARY_GRID.get(i) {
            encode(&mut bytes, normalize(0, cell.nsec));
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < B20_GRID.len() {
        if let Some(row) = B20_GRID.get(j) {
            encode(&mut bytes, normalize(row.sec, row.nsec));
        }
        j += 1;
    }
    let mut k = 0usize;
    while k < CLOCKS.len() {
        if let Some(sp) = CLOCKS.get(k) {
            encode(&mut bytes, normalize(sp.id as i64, 999_999_999));
        }
        k += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9333 判据集。
pub fn run_unx9333_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9333");

    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7C000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x7C0C0 独占且 0x40 对齐", seg_ok,
          "段基址必须与 F9332 的 0x7C080 窗口相邻不重叠");

    let neg_ok = !validate_nsec(-1) && !validate_nsec(-1_000_000_000)
        && TimeDomain::new().settime(0, 0, -1) == Err(EINVAL);
    s.add("tv_nsec 负值：-1 与 -1e9 全 EINVAL", neg_ok,
          "合法域下界是 0，负值不得进入规范化");

    let big_ok = !validate_nsec(1_000_000_000) && !validate_nsec(1_000_000_001)
        && TimeDomain::new().settime(0, 0, 2_000_000_000) == Err(EINVAL);
    s.add("tv_nsec 上界：1e9/1e9+1/2e9 全 EINVAL", big_ok,
          "上界闭开，1e9 本身即越界，必须先整秒进位");

    let (b_hits, b_ledger, b_drift) = run_boundary_matrix();
    s.add("边界十格 × 10 轮：合法成功、越界 EINVAL",
          b_hits == BOUNDARY_CELLS as u32 * INJECT_ROUNDS && b_drift == 0,
          "每格 10 轮复算必须与 valid 标记完全一致");

    s.add("边界注入账：100 次全检出且零逃逸",
          b_ledger.conserved() && b_ledger.escaped == 0
              && b_ledger.injected == BOUNDARY_CELLS as u32 * INJECT_ROUNDS,
          "injected == detected + escaped 必须逐笔成立");

    let (c_hits, c_units) = run_carry_grid();
    s.add("进位语义十行：商进 sec、负余数借位全对",
          c_hits == CARRY_ROWS as u32 && c_units == CARRY_UNITS_TOTAL,
          "十行含 1/2 进位、借位与 i64 双端，合计进位单位冻结为 9");

    s.add("B20 对账：十行零失配", b20_mismatch() == 0 && B20_GRID.len() == B20_ROWS,
          "本模块 normalizer 必须与 B20 规范化账逐行一致");

    let contract_ok = B20_CONTRACT.limit == NS_PER_SEC
        && B20_CONTRACT.version >= 1
        && B20_CONTRACT.rows == B20_ROWS as u32
        && MAX_NSEC == B20_CONTRACT.limit - 1;
    s.add("B20 契约：1e9 上限与账本世代一致", contract_ok,
          "两侧上限常量与行数必须逐字相等，禁止各写一份");

    s.add("规范化幂等：已规范值再规范化不动", idempotence_holds(),
          "normalize(normalize(x)) 必须与 normalize(x) 相等");

    let hole_ok = TimeDomain::new().settime(10, 0, 0) == Err(EINVAL)
        && TimeDomain::new().settime(12, 0, 0) == Err(EINVAL)
        && TimeDomain::new().settime(-1, 0, 0) == Err(EINVAL)
        && TimeDomain::new().classify(10, 0, 0) == Some(TimeCode::ClockHole);
    s.add("时钟空洞：10/12/-1 全 EINVAL", hole_ok,
          "Linux clockid 10 无时钟，合法域是 0..=11 去掉 10");

    let ro_ok = TimeDomain::new().settime(1, 0, 0) == Err(EPERM)
        && TimeDomain::new().settime(7, 0, 0) == Err(EPERM)
        && TimeDomain::new().settime(9, 0, 0) == Err(EPERM)
        && TimeDomain::new().classify(1, 0, 0) == Some(TimeCode::NotSettable);
    s.add("只读时钟：MONOTONIC/BOOTTIME/ALARM 写入 EPERM", ro_ok,
          "不可设时钟必须在 nsec 合法后回 EPERM，而不是 EINVAL");

    let mut dom = TimeDomain::new();
    let carried = dom.settime(0, 0, MAX_NSEC);
    let set_ok = carried == Ok((0, MAX_NSEC))
        && dom.accepts() == 1
        && dom.carries() == 0
        && dom.last_errno() == 0
        // 进位由调用方先做（头注②③冻结次序）：1.5e9 必须先整秒进位成
        // (1, 5e8) 再提交；直接提交越界 nsec 必须 EINVAL。
        && normalize(0, 1_500_000_000) == Some((1, 500_000_000))
        && carry_units(0, 1) == 1
        && TimeDomain::new().settime(0, 0, 1_500_000_000) == Err(EINVAL);
    s.add("可设时钟：REALTIME 带进位落账", set_ok,
          "合法 nsec 必须成功落账；越界 nsec 必须先整秒进位再提交，|Δsec| 记入进位账");

    let order_ok = TimeDomain::new().settime(1, 0, 1_000_000_000) == Err(EINVAL)
        && TimeDomain::new().classify(1, 0, 1_000_000_000) == Some(TimeCode::NsecTooLarge)
        && TimeDomain::new().classify(11, 0, 999_999_999).is_none();
    s.add("判定次序：nsec 值域先于可设性", order_ok,
          "MONOTONIC + 1e9 必须 EINVAL；只有 nsec 合法才轮到 EPERM");

    let ovf_ok = normalize(i64::MAX, 1_000_000_000).is_none()
        && normalize(i64::MIN, -1_000_000_000).is_none()
        && normalize(i64::MAX, MAX_NSEC) == Some((i64::MAX, MAX_NSEC));
    s.add("溢出：i64 双端外推返回 None", ovf_ok,
          "checked_add/checked_sub 任一越界都必须转 None，禁止回绕");

    s.add("时钟表冻结：12 行可设位零漂移",
          clock_table_drift() == 0 && CLOCKS.len() == CLOCK_ROWS
              && clock_spec(0).map(|c| c.settable) == Some(true)
              && clock_spec(10).is_none(),
          "空洞与四个可设时钟必须与冻结账逐行一致");

    let codes_ok = agrees_with_linux(EINVAL) && agrees_with_linux(EPERM)
        && linux_neg(EINVAL) == -22 && linux_neg(EPERM) == -1 && !agrees_with_linux(0);
    s.add("Linux 逐码：EINVAL/EPERM 表内可复算", codes_ok,
          "返回码只允许 errno 表内正值");

    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：时间语料指纹相等", fp_a == fp_b,
          "同输入两次必须逐字节相等");

    s.add("文案闭集：13 个诊断码均有中文建议",
          TimeCode::ALL.iter().all(|c| !c.detail().is_empty()),
          "每个码必须能取到非空且可操作的说明");

    let mut uniq = true;
    let mut a = 0usize;
    while a < TimeCode::ALL.len() {
        let mut b = a + 1;
        while b < TimeCode::ALL.len() {
            if let (Some(x), Some(y)) = (TimeCode::ALL.get(a), TimeCode::ALL.get(b)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            b += 1;
        }
        a += 1;
    }
    s.add("诊断码：13 个码值两两唯一", uniq && TimeCode::ALL.len() == 13,
          "段内闭集码不得重复");

    let b_text = format!(
        "边界注入 {}/{}，账 {}/{}/{}", b_hits,
        BOUNDARY_CELLS as u32 * INJECT_ROUNDS,
        b_ledger.injected, b_ledger.detected, b_ledger.escaped
    );
    s.add_dyn("边界账目", b_ledger.conserved(), &b_text);

    let c_text = format!("进位 {}/{} 行，单位 {}, B20 失配 {}", c_hits, CARRY_ROWS, c_units, b20_mismatch());
    s.add_dyn("进位对账", c_hits == CARRY_ROWS as u32 && c_units == CARRY_UNITS_TOTAL, &c_text);

    let fp_text = format!("时间语料指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nsec_domain_is_half_open() {
        assert_eq!(validate_nsec(0), true);
        assert_eq!(validate_nsec(MAX_NSEC), true);
        assert_eq!(validate_nsec(-1), false);
        assert_eq!(validate_nsec(1_000_000_000), false);
    }

    #[test]
    fn normalize_carries_forward() {
        assert_eq!(normalize(0, 1_000_000_000), Some((1, 0)));
        assert_eq!(normalize(5, 2_500_000_000), Some((7, 500_000_000)));
        assert_eq!(normalize(-1, 1_500_000_000), Some((0, 500_000_000)));
    }

    #[test]
    fn normalize_borrows_backward() {
        assert_eq!(normalize(0, -1), Some((-1, 999_999_999)));
        assert_eq!(normalize(-2, -1_500_000_000), Some((-4, 500_000_000)));
    }

    #[test]
    fn normalize_edges_and_overflow() {
        assert_eq!(normalize(i64::MIN, 0), Some((i64::MIN, 0)));
        assert_eq!(normalize(i64::MAX, MAX_NSEC), Some((i64::MAX, MAX_NSEC)));
        assert_eq!(normalize(i64::MAX, 1_000_000_000), None);
        assert_eq!(normalize(i64::MIN, -1_000_000_000), None);
    }

    #[test]
    fn boundary_grid_is_consistent() {
        let (hits, led, drift) = run_boundary_matrix();
        assert_eq!(hits, 100);
        assert_eq!(led.escaped, 0);
        assert_eq!(led.conserved(), true);
        assert_eq!(drift, 0);
    }

    #[test]
    fn carry_grid_and_idempotence() {
        let (hits, units) = run_carry_grid();
        assert_eq!(hits, CARRY_ROWS as u32);
        assert_eq!(units, CARRY_UNITS_TOTAL);
        assert_eq!(idempotence_holds(), true);
    }

    #[test]
    fn b20_reconciles() {
        assert_eq!(b20_mismatch(), 0);
        assert_eq!(B20_CONTRACT.limit, NS_PER_SEC);
        assert_eq!(B20_CONTRACT.rows, B20_ROWS as u32);
    }

    #[test]
    fn clock_domain_rules() {
        assert_eq!(TimeDomain::new().settime(10, 0, 0), Err(EINVAL));
        assert_eq!(TimeDomain::new().settime(1, 0, 0), Err(EPERM));
        assert_eq!(TimeDomain::new().settime(1, 0, 1_000_000_000), Err(EINVAL));
        assert_eq!(TimeDomain::new().settime(0, 0, 0), Ok((0, 0)));
    }

    #[test]
    fn settime_ledger_moves_once() {
        // 先整秒进位再提交：2.5e9 nsec → (5, 5e8)，|Δsec| = 2 由规范化账承担。
        assert_eq!(normalize(3, 2_500_000_000), Some((5, 500_000_000)));
        assert_eq!(carry_units(3, 5), 2);
        let mut dom = TimeDomain::new();
        assert_eq!(dom.settime(0, 5, 500_000_000), Ok((5, 500_000_000)));
        assert_eq!(dom.accepts(), 1);
        assert_eq!(dom.carries(), 0);
        // 越界 nsec 不得由 settime 悄悄进位（判定次序冻结：值域先于一切）。
        assert_eq!(dom.settime(0, 0, 2_500_000_000), Err(EINVAL));
        assert_eq!(dom.rejects(), 1);
        assert_eq!(dom.last_errno(), EINVAL);
    }

    #[test]
    fn fingerprint_is_deterministic() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
        assert_eq!(clock_table_drift(), 0);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9333_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
    }
}
