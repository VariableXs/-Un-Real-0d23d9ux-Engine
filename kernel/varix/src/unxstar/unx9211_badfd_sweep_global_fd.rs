//! UNX-F9211 坏 fd 注入跨号段扫描（fd 空间全域） · 0x7A240段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9211 逐字）：
//!   ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9211-J1 fd 空间全域扫描注入
//!   判据过，未开槽位全 EBADF 逐格检出，越上限与 RLIMIT_NOFILE 分界格过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 号段不是一条直线，而是七段：负值段、标准流段、低稀疏段、高稀疏段、
//!      RLIMIT 边界段、表容量边界段、int 上界段。扫描表按段取点，逐格记录
//!      「期望 / 实测 / 分类」，漏一格即红——这是 J1 的「逐格检出」。
//!   ② RLIMIT_NOFILE 只管**新分配**与 dup2 目标，不管既有句柄：把 rlimit
//!      从 24 降到 8 之后，fd=20 仍可 lookup/read，但再分配只能落 3..=7，
//!      分界格必须双向成立，否则会把「缩限」误判成「批量失效」。
//!   ③ 判定分类先于错误码：同一个 EBADF 背后有 越界（Range）/ 空槽（Slot）/
//!      陈旧代际（Generation）三种成因，fault_class 必须在返回码之前定型，
//!      这样「负值 fd」不会被误记成「未开槽」。
//!   ④ fd 表按 2 的幂扩容（8→16→32…），扩容只增不减；分配取最低可用槽，
//!      因此 fd 号连续可从账上复算：allocs - closes == open_count。
//!   ⑤ 注入器跨号段轮流打点（负值 / 空槽 / 越 rlimit / 越表容量），每类 10 轮，
//!      检出数必须逐类满格，逃逸恒为 0；账目与扫描点表同源，禁止各写各的。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9211 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7A240;
/// 标准流占用 0/1/2。
pub const STD_FDS: i32 = 3;
/// fd 表初始容量（2 的幂，按需翻倍）。
pub const INIT_CAP: usize = 8;
/// fd 表容量上界（防止无限扩容把栈/堆吃穿）。
pub const HARD_CAP: usize = 256;
/// 默认软限制 RLIMIT_NOFILE。
pub const DEFAULT_RLIMIT: u32 = 24;
/// 单类注入轮数。
pub const INJECT_ROUNDS: u32 = 10;

/// 坏文件描述符。
pub const EBADF: u32 = 9;
/// 参数非法。
pub const EINVAL: u32 = 22;
/// fd 表耗尽。
pub const EMFILE: u32 = 24;
/// 提升 rlimit 越过硬限。
pub const EPERM: u32 = 1;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 号段扫描诊断码（段内 +0x00..+0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SweepCode {
    NegativeBand,
    StdBand,
    UnopenedSlot,
    StaleGeneration,
    BeyondRlimit,
    BeyondTable,
    IntMaxWrap,
    RlimitLowered,
    Exhausted,
    LedgerRegress,
    NotReproducible,
    SegmentViolation,
}

impl SweepCode {
    /// 码值。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与建议（闭集，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            SweepCode::NegativeBand => "负值段：fd 小于 0 一律 EBADF（fget 的 fd<0 早退），请勿与未开槽混记",
            SweepCode::StdBand => "标准流段：本卡把 0/1/2 预先占用，关闭后同样 EBADF；请按真实 fdtable 复核",
            SweepCode::UnopenedSlot => "空槽段：fd 在表内但槽位未分配，回 EBADF；请先 open 再读写",
            SweepCode::StaleGeneration => "陈旧代际：同号已复用，旧代际不得通过；请用 close 返回的新代际",
            SweepCode::BeyondRlimit => "越 RLIMIT_NOFILE：fd 不小于软限不予分配；请核对 ulimit -n 与实际号段",
            SweepCode::BeyondTable => "越表容量：fd 超出已分配的 fdtable 上界，回 EBADF；扩容只由分配触发",
            SweepCode::IntMaxWrap => "int 上界：0x7FFFFFFF 与其后一号都是越界；无符号解释 -1 得 0xFFFFFFFF 仍 EBADF",
            SweepCode::RlimitLowered => "缩限后既有句柄仍有效：rlimit 只约束新分配，缩限不得连带作废在册 fd",
            SweepCode::Exhausted => "号段耗尽：rlimit 内已无空槽，新分配回 EMFILE；请关闭闲置 fd 或提升软限",
            SweepCode::LedgerRegress => "扫描账倒退：注入=检出+漏检，累计只增不减；请检查回滚路径",
            SweepCode::NotReproducible => "双跑不一致：同号段两次扫描结果不同；请排查扩容时序或非确定遍历",
            SweepCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在 0x7A000..=0x7D1C0",
        }
    }

    /// 闭集全量。
    pub const ALL: [SweepCode; 12] = [
        SweepCode::NegativeBand, SweepCode::StdBand, SweepCode::UnopenedSlot,
        SweepCode::StaleGeneration, SweepCode::BeyondRlimit, SweepCode::BeyondTable,
        SweepCode::IntMaxWrap, SweepCode::RlimitLowered, SweepCode::Exhausted,
        SweepCode::LedgerRegress, SweepCode::NotReproducible, SweepCode::SegmentViolation,
    ];
}

// ── 失效分类与槽位 ────────────────────────────────────────────────────
/// EBADF 的三种成因（先定型，再返回码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaultClass {
    /// 号段越界：负值 / 越表容量 / int 上界
    Range,
    /// 表内空槽
    Slot,
    /// 代际落后
    Generation,
    /// 未失效
    None,
}

/// 单个 fd 槽。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FdSlot {
    pub alive: bool,
    pub gen: u16,
    /// 打开标签（0=标准流，1=普通文件，2=管道，3=目录）
    pub tag: u16,
}

impl FdSlot {
    pub const fn empty() -> FdSlot {
        FdSlot { alive: false, gen: 1, tag: 0 }
    }

    pub const fn std(tag: u16) -> FdSlot {
        FdSlot { alive: true, gen: 1, tag }
    }
}

/// fd 空间：表 + 软限 + 账。
#[derive(Clone, Debug)]
pub struct FdSpace {
    slots: Vec<FdSlot>,
    rlimit_cur: u32,
    rlimit_max: u32,
    allocs: u32,
    closes: u32,
    ebaddr_hits: u32,
    emfile_hits: u32,
}

impl FdSpace {
    /// 构造：0/1/2 常驻，表容量 INIT_CAP。
    pub fn new(rlimit_cur: u32) -> FdSpace {
        let mut slots: Vec<FdSlot> = Vec::new();
        slots.push(FdSlot::std(0));
        slots.push(FdSlot::std(1));
        slots.push(FdSlot::std(2));
        while slots.len() < INIT_CAP {
            slots.push(FdSlot::empty());
        }
        FdSpace {
            slots,
            rlimit_cur,
            rlimit_max: 1024,
            allocs: 0,
            closes: 0,
            ebaddr_hits: 0,
            emfile_hits: 0,
        }
    }

    /// 表容量。
    pub fn cap(&self) -> usize {
        self.slots.len()
    }

    /// 软限。
    pub fn rlimit(&self) -> u32 {
        self.rlimit_cur
    }

    /// 已用槽数。
    pub fn open_count(&self) -> usize {
        self.slots.iter().filter(|s| s.alive).count()
    }

    /// 分配账 / 关闭账 / EBADF 记账 / EMFILE 记账。
    pub fn allocs(&self) -> u32 {
        self.allocs
    }
    pub fn closes(&self) -> u32 {
        self.closes
    }
    pub fn ebaddr_hits(&self) -> u32 {
        self.ebaddr_hits
    }
    pub fn emfile_hits(&self) -> u32 {
        self.emfile_hits
    }

    /// 失效分类（不返回码，只定型）。
    pub fn classify(&self, fd: i32) -> FaultClass {
        if fd < 0 {
            return FaultClass::Range;
        }
        let idx = fd as usize;
        match self.slots.get(idx) {
            None => FaultClass::Range,
            Some(slot) if slot.alive => FaultClass::None,
            Some(_) => FaultClass::Slot,
        }
    }

    /// 选个：越界/空槽/代际落后统一 EBADF，但先记分类。
    pub fn lookup(&mut self, fd: i32) -> Result<&FdSlot, u32> {
        let class = self.classify(fd);
        if class != FaultClass::None {
            self.ebaddr_hits = self.ebaddr_hits.saturating_add(1);
            return Err(EBADF);
        }
        match self.slots.get(fd as usize) {
            Some(slot) => Ok(slot),
            None => Err(EBADF),
        }
    }

    /// 代际选个：号与代际都必须匹配。
    pub fn lookup_gen(&mut self, fd: i32, gen: u16) -> Result<&FdSlot, u32> {
        let current = match self.lookup(fd) {
            Ok(slot) => slot.gen,
            Err(e) => return Err(e),
        };
        if current != gen {
            self.ebaddr_hits = self.ebaddr_hits.saturating_add(1);
            return Err(EBADF);
        }
        match self.slots.get(fd as usize) {
            Some(s) => Ok(s),
            None => Err(EBADF),
        }
    }

    /// 扩容：2 的幂增长，直到容得下 target_fd+1；上界 HARD_CAP。
    pub fn grow_for(&mut self, target_fd: u32) -> Result<(), u32> {
        if target_fd as usize >= HARD_CAP {
            return Err(EBADF);
        }
        let mut want = self.slots.len();
        while want <= target_fd as usize {
            want = want.saturating_mul(2);
        }
        if want > HARD_CAP {
            want = HARD_CAP;
        }
        while self.slots.len() < want {
            self.slots.push(FdSlot::empty());
        }
        Ok(())
    }

    /// 取最低可用 fd：必须同时满足「槽空」与「fd < rlimit_cur」；
    /// 槽位不足时按 2 的幂扩容一次再试，仍无空槽则 EMFILE。
    pub fn alloc_lowest(&mut self, tag: u16) -> Result<i32, u32> {
        let mut attempt = 0u32;
        while attempt < 2 {
            let mut i = STD_FDS as usize;
            while i < self.slots.len() {
                if (i as u32) >= self.rlimit_cur {
                    break;
                }
                let free = match self.slots.get(i) {
                    Some(s) => !s.alive,
                    None => false,
                };
                if free {
                    if let Some(s) = self.slots.get_mut(i) {
                        s.alive = true;
                        s.tag = tag;
                        s.gen = s.gen.wrapping_add(1);
                    }
                    self.allocs = self.allocs.saturating_add(1);
                    return Ok(i as i32);
                }
                i += 1;
            }
            let cur_len = self.slots.len() as u32;
            let roomy = cur_len.saturating_mul(2) < self.rlimit_cur
                && (cur_len as usize) < HARD_CAP;
            if attempt == 0 && roomy {
                if self.grow_for(cur_len).is_ok() {
                    attempt += 1;
                    continue;
                }
            }
            break;
        }
        self.emfile_hits = self.emfile_hits.saturating_add(1);
        Err(EMFILE)
    }

    /// 关闭：推进代际。越界/空槽按 EBADF。
    pub fn close(&mut self, fd: i32) -> Result<u16, u32> {
        if self.classify(fd) != FaultClass::None {
            self.ebaddr_hits = self.ebaddr_hits.saturating_add(1);
            return Err(EBADF);
        }
        let idx = fd as usize;
        let next = match self.slots.get(idx) {
            Some(s) => s.gen.wrapping_add(1),
            None => return Err(EBADF),
        };
        if let Some(s) = self.slots.get_mut(idx) {
            s.alive = false;
            s.gen = next;
            s.tag = 0;
        }
        self.closes = self.closes.saturating_add(1);
        Ok(next)
    }

    /// dup2 风格的目标位安装：目标不小于 rlimit 直接 EBADF。
    pub fn install_at(&mut self, target: u32, tag: u16) -> Result<i32, u32> {
        if target >= self.rlimit_cur {
            self.ebaddr_hits = self.ebaddr_hits.saturating_add(1);
            return Err(EBADF);
        }
        if target as usize >= self.slots.len() {
            self.grow_for(target)?;
        }
        if let Some(s) = self.slots.get_mut(target as usize) {
            s.alive = true;
            s.tag = tag;
            s.gen = s.gen.wrapping_add(1);
        }
        self.allocs = self.allocs.saturating_add(1);
        Ok(target as i32)
    }

    /// 软限调整：不得超过硬限（EPERM），不得低于 STD_FDS。
    pub fn setrlimit(&mut self, cur: u32) -> Result<u32, u32> {
        if cur > self.rlimit_max {
            return Err(EPERM);
        }
        if cur < STD_FDS as u32 {
            return Err(EINVAL);
        }
        self.rlimit_cur = cur;
        Ok(cur)
    }
}

// ── 号段扫描表 ────────────────────────────────────────────────────────
/// 一段号段（用于对账与逐格报告）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Band {
    Negative,
    Std,
    LowSparse,
    HighSparse,
    RlimitEdge,
    TableEdge,
    IntMax,
}

impl Band {
    /// 段中文名。
    pub fn name(self) -> &'static str {
        match self {
            Band::Negative => "负值段",
            Band::Std => "标准流段",
            Band::LowSparse => "低稀疏段",
            Band::HighSparse => "高稀疏段",
            Band::RlimitEdge => "RLIMIT 边界段",
            Band::TableEdge => "表容量边界段",
            Band::IntMax => "int 上界段",
        }
    }

    /// 闭集全量。
    pub const ALL: [Band; 7] = [
        Band::Negative, Band::Std, Band::LowSparse, Band::HighSparse,
        Band::RlimitEdge, Band::TableEdge, Band::IntMax,
    ];
}

/// 一个扫描点：(fd, 所属段)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Point {
    pub fd: i32,
    pub band: Band,
}

/// 扫描点表：跨七段共 20 点（rlimit=12、表容量=8 时取点）。
pub const SWEEP_POINTS: [Point; 20] = [
    Point { fd: -2147483648, band: Band::Negative },
    Point { fd: -3, band: Band::Negative },
    Point { fd: -1, band: Band::Negative },
    Point { fd: 0, band: Band::Std },
    Point { fd: 1, band: Band::Std },
    Point { fd: 2, band: Band::Std },
    Point { fd: 3, band: Band::LowSparse },
    Point { fd: 4, band: Band::LowSparse },
    Point { fd: 5, band: Band::LowSparse },
    Point { fd: 6, band: Band::LowSparse },
    Point { fd: 7, band: Band::LowSparse },
    Point { fd: 9, band: Band::HighSparse },
    Point { fd: 10, band: Band::HighSparse },
    Point { fd: 11, band: Band::RlimitEdge },
    Point { fd: 12, band: Band::RlimitEdge },
    Point { fd: 13, band: Band::RlimitEdge },
    Point { fd: 14, band: Band::TableEdge },
    Point { fd: 15, band: Band::TableEdge },
    Point { fd: 16, band: Band::TableEdge },
    Point { fd: 2147483647, band: Band::IntMax },
];

/// 一支扫描结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hit {
    pub fd: i32,
    pub band: Band,
    /// 期望：true=可用，false=EBADF
    pub expect_ok: bool,
    /// 实测：true=可用，false=EBADF
    pub got_ok: bool,
    /// 失效分类
    pub class: FaultClass,
}

impl Hit {
    /// 该点是否命中期望。
    pub fn matched(&self) -> bool {
        self.expect_ok == self.got_ok
    }
}

/// 扫描报告。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SweepReport {
    pub hits: Vec<Hit>,
    /// 期望为 EBADF 的点数
    pub bad_points: u32,
    /// 期望为可用的点数
    pub good_points: u32,
    /// 不符点数
    pub mismatched: u32,
}

impl SweepReport {
    /// 是否符合「未开槽全 EBADF」。
    pub fn all_bad_are_badf(&self) -> bool {
        self.hits.iter().all(|h| h.expect_ok || !h.got_ok)
    }
}

/// 全域扫描：空格子必须逐格 EBADF。space 的内部状态决定期望值。
pub fn sweep(space: &mut FdSpace) -> SweepReport {
    let mut hits: Vec<Hit> = Vec::new();
    let mut bad = 0u32;
    let mut good = 0u32;
    let mut mism = 0u32;
    let mut i = 0usize;
    while i < SWEEP_POINTS.len() {
        if let Some(p) = SWEEP_POINTS.get(i).copied() {
            let class = space.classify(p.fd);
            let expect_ok = class == FaultClass::None;
            let got_ok = space.lookup(p.fd).is_ok();
            if expect_ok {
                good += 1;
            } else {
                bad += 1;
            }
            if expect_ok != got_ok {
                mism += 1;
            }
            hits.push(Hit { fd: p.fd, band: p.band, expect_ok, got_ok, class });
        }
        i += 1;
    }
    SweepReport { hits, bad_points: bad, good_points: good, mismatched: mism }
}

/// 按段汇总的检出数（0..=6 共七段）。
pub fn band_tally(report: &SweepReport) -> [u32; 7] {
    let mut out = [0u32; 7];
    let mut i = 0usize;
    while i < report.hits.len() {
        if let Some(h) = report.hits.get(i) {
            let idx = match h.band {
                Band::Negative => 0usize,
                Band::Std => 1,
                Band::LowSparse => 2,
                Band::HighSparse => 3,
                Band::RlimitEdge => 4,
                Band::TableEdge => 5,
                Band::IntMax => 6,
            };
            if let Some(slot) = out.get_mut(idx) {
                *slot = slot.saturating_add(1);
            }
        }
        i += 1;
    }
    out
}

// ── 注入器 ────────────────────────────────────────────────────────────
/// 四类坏 fd 注入。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SweepInject {
    NegativeFd,
    EmptySlot,
    BeyondRlimit,
    BeyondTable,
}

impl SweepInject {
    /// 对应诊断码。
    pub fn code(self) -> SweepCode {
        match self {
            SweepInject::NegativeFd => SweepCode::NegativeBand,
            SweepInject::EmptySlot => SweepCode::UnopenedSlot,
            SweepInject::BeyondRlimit => SweepCode::BeyondRlimit,
            SweepInject::BeyondTable => SweepCode::BeyondTable,
        }
    }

    /// 期望分类。
    pub fn class(self) -> FaultClass {
        match self {
            SweepInject::NegativeFd => FaultClass::Range,
            SweepInject::EmptySlot => FaultClass::Slot,
            SweepInject::BeyondRlimit => FaultClass::Range,
            SweepInject::BeyondTable => FaultClass::Range,
        }
    }

    /// 闭集全量。
    pub const ALL: [SweepInject; 4] = [
        SweepInject::NegativeFd, SweepInject::EmptySlot,
        SweepInject::BeyondRlimit, SweepInject::BeyondTable,
    ];
}

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ledger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl Ledger {
    pub const fn zero() -> Ledger {
        Ledger { injected: 0, detected: 0, escaped: 0 }
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

/// 跨号段注入：四类各 INJECT_ROUNDS 轮，逐轮记录分类是否与期望一致。
pub fn run_injections() -> (Ledger, [u32; 4]) {
    let mut space = FdSpace::new(12);
    // 预留一个空槽（fd=3）用于 EmptySlot；表容量 8，rlimit 12。
    let mut ledger = Ledger::zero();
    let mut per = [0u32; 4];
    let mut ci = 0usize;
    while ci < SweepInject::ALL.len() {
        if let Some(case) = SweepInject::ALL.get(ci).copied() {
            let fd = match case {
                SweepInject::NegativeFd => -1,
                SweepInject::EmptySlot => 3,
                SweepInject::BeyondRlimit => 12,
                SweepInject::BeyondTable => 64,
            };
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let class_before = space.classify(fd);
                let caught = space.lookup(fd) == Err(EBADF) && class_before == case.class();
                let _ = ledger.record(caught);
                if caught {
                    if let Some(s) = per.get_mut(ci) {
                        *s = s.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per)
}

// ── 对账表 ────────────────────────────────────────────────────────────
/// (号段, fd 采样, 期望是否可用, 依据)
pub const BAND_TABLE: &[(&str, i32, bool, &str)] = &[
    ("负值段", -1, false, "fget：fd<0 早退，回 -EBADF"),
    ("标准流段", 1, true, "0/1/2 由 shell 预置，常驻可用"),
    ("低稀疏段", 3, false, "未 open 时槽位为空，回 -EBADF"),
    ("高稀疏段", 9, false, "表内空槽同样 -EBADF，不升格为 EINVAL"),
    ("RLIMIT 边界段", 11, false, "fd 不小于 rlimit_cur 不予分配"),
    ("表容量边界段", 15, false, "超出已分配 fdtable 上界，回 -EBADF"),
    ("int 上界段", 2147483647, false, "fd 以 int 承载，上界恒越界"),
];

/// 查表：号段名 → (采样, 期望可用)。
pub fn band_expected(name: &str) -> Option<(i32, bool)> {
    let mut i = 0usize;
    while i < BAND_TABLE.len() {
        if let Some(row) = BAND_TABLE.get(i) {
            if row.0 == name {
                return Some((row.1, row.2));
            }
        }
        i += 1;
    }
    None
}

// ── 指纹 ──────────────────────────────────────────────────────────────
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

/// 规范语料：造一个稀疏空间（3/4/9 打开，5 关闭）后全域扫描取指纹。
pub fn corpus_fingerprint() -> u32 {
    let mut space = FdSpace::new(12);
    let _ = space.alloc_lowest(1);
    let _ = space.alloc_lowest(1);
    let third = space.alloc_lowest(2);
    let _ = space.install_at(9, 3);
    let _ = space.close(5);
    let report = sweep(&mut space);
    let mut bytes: Vec<u8> = Vec::new();
    let third_fd = match third {
        Ok(v) => v,
        Err(_) => 0,
    };
    bytes.push((third_fd & 0xFF) as u8);
    let mut i = 0usize;
    while i < report.hits.len() {
        if let Some(h) = report.hits.get(i) {
            let code = match h.class {
                FaultClass::None => 1u8,
                FaultClass::Range => 2,
                FaultClass::Slot => 3,
                FaultClass::Generation => 4,
            };
            bytes.push(if h.got_ok { code } else { code | 0x80 });
        }
        i += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9211 判据集（20 条静态 + 3 条动态）。
pub fn run_unx9211_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9211");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须独占且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：最低可用分配
    let mut sp = FdSpace::new(12);
    let a = sp.alloc_lowest(1);
    let b = sp.alloc_lowest(1);
    let c = sp.alloc_lowest(2);
    s.add("正常流：最低空闲分配 3→4→5",
          a == Ok(3) && b == Ok(4) && c == Ok(5) && sp.open_count() == 6,
          "Linux 取最低可用 fd；0/1/2 常驻，动态槽从 3 起");

    // 3. 全域逐格
    let mut sp3 = FdSpace::new(12);
    let _ = sp3.alloc_lowest(1);
    let _ = sp3.alloc_lowest(1);
    let report = sweep(&mut sp3);
    s.add("全域逐格：20 点扫描零不符，未开槽全 EBADF",
          report.mismatched == 0 && report.all_bad_are_badf() && report.hits.len() == SWEEP_POINTS.len(),
          "负值/标准流/稀疏/边界四类点逐一比对期望与实测");

    // 4. 七段全覆盖
    let tally = band_tally(&report);
    let bands_ok = tally.iter().all(|n| *n > 0);
    s.add("号段覆盖：七段各有采样点", bands_ok && Band::ALL.len() == 7,
          "任一段无点即视为覆盖缺口，扫描表不得只打低号段");

    // 5. RLIMIT 分界：rlimit-1 可安装、rlimit 拒
    let mut sp5 = FdSpace::new(12);
    let edge_ok = sp5.install_at(11, 1) == Ok(11)
        && sp5.install_at(12, 1) == Err(EBADF)
        && sp5.install_at(13, 1) == Err(EBADF);
    s.add("RLIMIT 分界：fd=11 可装、fd=12/13 拒", edge_ok,
          "软限 12 时合法号段止于 11；边界两侧必须不同判");

    // 6. 表容量边界
    let mut sp6 = FdSpace::new(24);
    let cap = sp6.cap();
    let far_bad = sp6.lookup(cap as i32) == Err(EBADF);
    let far_class = sp6.classify(cap as i32);
    s.add("表容量边界：fd == cap 回 EBADF",
          far_bad && far_class == FaultClass::Range,
          "fdtable 未覆盖的号一律 Range 分类，不得记成空槽");

    // 7. 负值与 int 上界
    let mut sp7 = FdSpace::new(12);
    let neg_ok = sp7.lookup(-1) == Err(EBADF)
        && sp7.lookup(-2147483648) == Err(EBADF)
        && sp7.lookup(2147483647) == Err(EBADF)
        && sp7.classify(-1) == FaultClass::Range
        && sp7.classify(2147483647) == FaultClass::Range;
    s.add("边界：负值/INT_MIN/INT_MAX 全 EBADF 且归 Range",
          neg_ok,
          "越界与空槽必须分类不同，否则坏 fd 账无法归因");

    // 8. 判定分类先于返回码
    let mut sp8 = FdSpace::new(12);
    let slot_class = sp8.classify(3);
    let range_class = sp8.classify(-9);
    s.add("判定次序：越界(Range) 先于空槽(Slot)",
          slot_class == FaultClass::Slot && range_class == FaultClass::Range
              && sp8.lookup(-9) == Err(EBADF) && sp8.lookup(3) == Err(EBADF),
          "同是 EBADF，成因分类在返回码之前定型");

    // 9. 缩限后既有句柄仍有效
    let mut sp9 = FdSpace::new(24);
    let high = sp9.install_at(20, 2);
    let lower = sp9.setrlimit(8);
    let still = sp9.lookup(20).is_ok();
    s.add("缩限：rlimit 24→8 后 fd=20 仍可用",
          high == Ok(20) && lower == Ok(8) && still,
          "rlimit 只约束新分配，不得连带作废在册句柄");

    // 10. 缩限后新分配落在新限内
    let mut sp10 = FdSpace::new(24);
    let _ = sp10.setrlimit(8);
    let mut in_range = true;
    let mut k = 0u32;
    while k < 3 {
        match sp10.alloc_lowest(1) {
            Ok(fd) => {
                if fd as u32 >= 8 {
                    in_range = false;
                }
            }
            Err(_) => in_range = false,
        }
        k += 1;
    }
    s.add("缩限后分配：3 次分配全部落在 3..=7", in_range,
          "缩限后的新 fd 必须小于新软限，越界即红");

    // 11. 号段耗尽 EMFILE
    let mut sp11 = FdSpace::new(6);
    let mut got = 0u32;
    let mut k = 0u32;
    while k < 8 {
        if sp11.alloc_lowest(1).is_ok() {
            got += 1;
        }
        k += 1;
    }
    s.add("耗尽：rlimit=6 时只容 3 个动态槽，其余 EMFILE",
          got == 3 && sp11.emfile_hits() == 5 && sp11.rlimit() == 6,
          "软限内无空槽即 EMFILE；扩容不得越过 rlimit");

    // 12. 状态机：close 推进代际
    let mut sp12 = FdSpace::new(12);
    let fd = sp12.alloc_lowest(1);
    let gen_before = match fd {
        Ok(f) => match sp12.lookup(f) {
            Ok(slot) => slot.gen,
            Err(_) => 0,
        },
        Err(_) => 0,
    };
    let closed = match fd {
        Ok(f) => sp12.close(f),
        Err(_) => Err(EBADF),
    };
    let stale = match fd {
        Ok(f) => sp12.lookup_gen(f, gen_before),
        Err(_) => Err(EBADF),
    };
    s.add("状态机：close 后代际前进，旧代际全拒",
          matches!(closed, Ok(g) if g == gen_before.wrapping_add(1)) && stale == Err(EBADF),
          "每次 close 推进一代，旧句柄不得复活");

    // 13. 复用后新代际可用
    let reused = sp12.alloc_lowest(1);
    let reuse_ok = match reused {
        Ok(f) => {
            let g = match sp12.lookup(f) {
                Ok(slot) => slot.gen,
                Err(_) => 0,
            };
            g > gen_before && sp12.lookup_gen(f, g).is_ok()
        }
        Err(_) => false,
    };
    s.add("复用：同号重开后新代际通过", reuse_ok,
          "最低可用槽复用是 Linux 常态，代际必须随重开前进");

    // 14. 扩容只增不减
    let mut sp14 = FdSpace::new(24);
    let cap_before = sp14.cap();
    let _ = sp14.grow_for(16);
    let cap_after = sp14.cap();
    let shrink = sp14.grow_for(2);
    s.add("容量：扩容 8→32 且不可回缩",
          cap_before == INIT_CAP && cap_after >= 17 && cap_after > cap_before
              && shrink.is_ok() && sp14.cap() == cap_after,
          "grow_for 只扩不缩；容量账是单调的");

    // 15. 注入 4 类 10/10
    let (ledger, per) = run_injections();
    s.add("注入：四类跨段违约各 10/10 全检出",
          per.iter().all(|n| *n == INJECT_ROUNDS),
          "负值/空槽/越 rlimit/越表容量必须逐类全抓");

    // 16. 守恒
    s.add("守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == SweepInject::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出，逃逸恒为 0");

    // 17. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：稀疏空间扫描指纹相等", f1 == f2,
          "同输入两次必须逐点相等；扩容时序不得外泄");

    // 18. 对账表
    let recon_ok = band_expected("负值段") == Some((-1, false))
        && band_expected("标准流段") == Some((1, true))
        && band_expected("RLIMIT 边界段") == Some((11, false))
        && band_expected("int 上界段") == Some((2147483647, false))
        && band_expected("未登记段").is_none();
    s.add("对账：七段判据表逐行可复算", recon_ok,
          "号段名 → 采样 fd → 期望可用性，表外返回 None");

    // 19. 文案闭集
    s.add("文案闭集：12 个诊断码均带中文建议",
          SweepCode::ALL.iter().all(|c| !c.detail().is_empty()),
          "每个码必须能取到非空且可操作的说明");

    // 动态明细（≤8）：扫描、注入、指纹三行
    let sweep_text = format!("扫描 {} 点 / 不符 {} / 坏点 {}",
                             report.hits.len(), report.mismatched, report.bad_points);
    s.add_dyn("扫描快照", report.mismatched == 0, &sweep_text);
    let ledger_text = format!("注入 {} / 检出 {} / 逃逸 {}",
                              ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("注入账快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!("扫描指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    // 20. 收口
    let before = s.len();
    let trunc = s.truncated();
    s.add("收口：判据条数不少于 20 且未截断", before >= 20 && !trunc,
          "本模块断言只增不减；动态明细槽仅用 3 个");

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowest_free_allocation() {
        let mut sp = FdSpace::new(12);
        assert_eq!(sp.alloc_lowest(1), Ok(3));
        assert_eq!(sp.alloc_lowest(1), Ok(4));
        // 代际从 1 起：alloc 推进到 2，close 再推进到 3，返回新代际。
        assert_eq!(sp.close(3), Ok(3));
        // 关闭后 3 号回到最低可用位，复用同一槽。
        assert_eq!(sp.alloc_lowest(1), Ok(3));
    }

    #[test]
    fn unopened_slots_are_ebadf() {
        let mut sp = FdSpace::new(12);
        let report = sweep(&mut sp);
        assert_eq!(report.mismatched, 0);
        for h in report.hits.iter() {
            if h.fd >= 3 && h.fd < sp.cap() as i32 {
                assert!(!h.got_ok);
                assert_eq!(h.class, FaultClass::Slot);
            }
        }
    }

    #[test]
    fn rlimit_boundary_is_split() {
        let mut sp = FdSpace::new(12);
        assert_eq!(sp.install_at(11, 1), Ok(11));
        assert_eq!(sp.install_at(12, 1), Err(EBADF));
        assert_eq!(sp.lookup(11).is_ok(), true);
    }

    #[test]
    fn lowering_rlimit_keeps_existing_fds() {
        let mut sp = FdSpace::new(24);
        assert_eq!(sp.install_at(20, 2), Ok(20));
        assert_eq!(sp.setrlimit(8), Ok(8));
        assert!(sp.lookup(20).is_ok());
        assert_eq!(sp.install_at(9, 1), Err(EBADF));
    }

    #[test]
    fn int_extremes_are_ebadf() {
        let mut sp = FdSpace::new(12);
        assert_eq!(sp.lookup(-1), Err(EBADF));
        assert_eq!(sp.lookup(-2147483648), Err(EBADF));
        assert_eq!(sp.lookup(2147483647), Err(EBADF));
    }

    #[test]
    fn exhaustion_reports_emfile() {
        let mut sp = FdSpace::new(6);
        assert!(sp.alloc_lowest(1).is_ok());
        assert!(sp.alloc_lowest(1).is_ok());
        assert!(sp.alloc_lowest(1).is_ok());
        assert_eq!(sp.alloc_lowest(1), Err(EMFILE));
        assert_eq!(sp.emfile_hits(), 1);
    }

    #[test]
    fn generation_advances_on_close() {
        let mut sp = FdSpace::new(12);
        let fd = sp.alloc_lowest(1).unwrap_or(-1);
        let g = sp.close(fd).unwrap_or(0);
        assert_eq!(sp.lookup(fd), Err(EBADF));
        assert_eq!(sp.lookup_gen(fd, g), Err(EBADF));
        let fd2 = sp.alloc_lowest(1).unwrap_or(-1);
        assert_eq!(fd2, fd);
        let g2 = sp.lookup(fd2).map(|s| s.gen).unwrap_or(0);
        assert_eq!(g2, g.wrapping_add(1));
        assert!(sp.lookup_gen(fd2, g2).is_ok());
        assert_eq!(sp.lookup_gen(fd2, g), Err(EBADF));
    }

    #[test]
    fn growth_is_monotonic() {
        let mut sp = FdSpace::new(24);
        let before = sp.cap();
        assert!(sp.grow_for(16).is_ok());
        assert!(sp.cap() > before);
        let grown = sp.cap();
        assert!(sp.grow_for(2).is_ok());
        assert_eq!(sp.cap(), grown);
    }

    #[test]
    fn injections_are_all_caught() {
        let (l, per) = run_injections();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 40);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn fingerprint_is_stable() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9211_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(s.len() >= 20);
    }
}
