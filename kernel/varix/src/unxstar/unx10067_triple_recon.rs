//! UNX-F10067 台账↔两册三方对账（总纲/批册/深化册） · 0x92080 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10067（M 型收口线：三方逐格对账与差异账）。
//! 锚点：UNX-F10068-J1 红账总核对齐（全账逐条在案 10/10 次），账外红
//!       10/10 次检出。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92080（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `ReconCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 对账网 `ReconGrid`：台账/批册/深化册三方 × 24 批 × (条数,行数)
//!     逐格承载；对账后封网只读（ReconState 单向）；
//!   - 差异账 `DiffLedger`：每处差异必须入账（批号+指标定位）；
//!     账满差异 = 差异不记 → reconcile 显性 Err 拦截，逃逸入账；
//!   - 解析器 `parse_recon_row`："5|20|6600" → (批,条,行) 三段闭集；
//!   - 定位器 `first_diff`：逐批逐指标扫描，首个差异批号可指认；
//!   - 注入核 `BadReconCase`：单侧改格 / 批号缺位 / 差异不记 /
//!     封网改格 / 越界批号 五类坏样本逐类 10/10 次拒绝；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10067 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x92080;

/// 段窗上界（下一卡 0x920c0 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x920c0;

/// 对账批数（M 型段 24 批）。
pub const BATCH_COUNT: usize = 24;

/// 每批条数口径（20 条/批）。
pub const ITEMS_PER_BATCH: u32 = 20;

/// 每批行数口径（20 条 × 330 行）。
pub const LINES_PER_BATCH: u32 = 6_600;

/// 差异账容量（24 批 × 2 指标上界）。
pub const DIFF_CAP: usize = 48;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EINVAL: i32 = 22;
pub const EBUSY: i32 = 16;
pub const ENOENT: i32 = 2;
pub const EIO: i32 = 5;
pub const ENOSPC: i32 = 28;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReconCode {
    /// 批号越界（出 1..=24）
    BatchRange,
    /// 封网改格（对账后网格只读）
    SealedMutate,
    /// 批号缺位（某方格值为零口径）
    BatchMissing,
    /// 差异不记（差异账满，reconcile 显性拦截）
    DiffNotRecorded,
    /// 差异账倒退（只增不减破坏）
    LedgerRegress,
    /// 对账状态倒退
    StateRegress,
    /// 格值口径非法（条数/行数为零或超口径上界）
    CellRange,
    /// 差异逃逸（账面漏计差异）
    DiffEscape,
    /// 重复入账（同批同指标差异重复记）
    DuplicateDiff,
    /// 解析失败
    ParseFailure,
    /// 未对账出结论
    NotReconciled,
    /// 段位越界
    SegmentViolation,
}

impl ReconCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ReconCode::BatchRange => "批号越界被拒：批号只允许 1..=24；请核对批号",
            ReconCode::SealedMutate => "封网改格被拒：对账后网格只读；请走修订通道",
            ReconCode::BatchMissing => "批号缺位被拒：三方须同批在格；请补缺位批",
            ReconCode::DiffNotRecorded => "差异不记被拒：差异账满不得继续对账；请扩账或修格",
            ReconCode::LedgerRegress => "差异账倒退被拒：条目只增不减；请排查回滚",
            ReconCode::StateRegress => "状态倒退被拒：Open→Reconciled 单向",
            ReconCode::CellRange => "格值口径被拒：条数 1..=40、行数 1..=26400；请核口径",
            ReconCode::DiffEscape => "差异逃逸：账面漏计差异；请补记差异明细",
            ReconCode::DuplicateDiff => "重复入账被拒：同批同指标差异已记；请查重复",
            ReconCode::ParseFailure => "解析失败：格行必须恰为 batch|items|lines 三段",
            ReconCode::NotReconciled => "未对账出结论被拒：先 reconcile 再取差异结论",
            ReconCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x92080..=0x920c0",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [ReconCode; 12] = [
        ReconCode::BatchRange,
        ReconCode::SealedMutate,
        ReconCode::BatchMissing,
        ReconCode::DiffNotRecorded,
        ReconCode::LedgerRegress,
        ReconCode::StateRegress,
        ReconCode::CellRange,
        ReconCode::DiffEscape,
        ReconCode::DuplicateDiff,
        ReconCode::ParseFailure,
        ReconCode::NotReconciled,
        ReconCode::SegmentViolation,
    ];
}

// ── 格与解析器 ────────────────────────────────────────────────────────
/// 一方一格：条数 + 行数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    /// 条数
    pub items: u32,
    /// 行数
    pub lines: u32,
}

/// 格值口径：条数 1..=40、行数 1..=26400（40 条 × 660 上界余量）。
pub fn cell_in_range(c: &Cell) -> bool {
    c.items >= 1 && c.items <= 40 && c.lines >= 1 && c.lines <= 26_400
}

/// 格行解析："5|20|6600" → (批号, Cell)。
pub fn parse_recon_row(s: &str) -> Result<(u32, Cell), ReconCode> {
    let parts: Vec<&str> = s.split('|').collect();
    if parts.len() != 3 {
        return Err(ReconCode::ParseFailure);
    }
    let mut vals = [0u32; 3];
    let mut vi = 0usize;
    while vi < 3 {
        let p = match parts.get(vi) {
            Some(p) => *p,
            None => return Err(ReconCode::ParseFailure),
        };
        if p.is_empty() || p.len() > 6 {
            return Err(ReconCode::ParseFailure);
        }
        let mut n: u32 = 0;
        for b in p.bytes() {
            if !b.is_ascii_digit() {
                return Err(ReconCode::ParseFailure);
            }
            n = n.saturating_mul(10).saturating_add((b - b'0') as u32);
        }
        if let Some(slot) = vals.get_mut(vi) {
            *slot = n;
        }
        vi += 1;
    }
    Ok((vals[0], Cell { items: vals[1], lines: vals[2] }))
}

// ── 差异账 ────────────────────────────────────────────────────────────
/// 差异指标（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Metric {
    /// 条数
    Items,
    /// 行数
    Lines,
}

impl Metric {
    /// 指标下标。
    pub fn index(self) -> usize {
        match self {
            Metric::Items => 0,
            Metric::Lines => 1,
        }
    }
}

/// 一处差异：批号 + 指标 + 三方值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DiffEntry {
    /// 批号 1..=24
    pub batch: u32,
    /// 差异指标
    pub metric: Metric,
}

/// 差异账：逐差异入账；账满即逃逸显性计数并拒绝。
#[derive(Clone, Debug)]
pub struct DiffLedger {
    /// 已记差异
    pub entries: Vec<DiffEntry>,
    /// 账容量
    pub cap: usize,
    /// 逃逸数（差异不记时显性累加）
    pub escapes: u32,
}

impl DiffLedger {
    /// 构造账（默认容量 DIFF_CAP）。
    pub fn new() -> DiffLedger {
        DiffLedger { entries: Vec::new(), cap: DIFF_CAP, escapes: 0 }
    }

    /// 记一处差异：重复必拒、账满即逃逸并拒。
    pub fn record(&mut self, e: DiffEntry) -> Result<(), ReconCode> {
        if self.entries.iter().any(|x| *x == e) {
            return Err(ReconCode::DuplicateDiff);
        }
        if self.entries.len() >= self.cap {
            self.escapes = self.escapes.saturating_add(1);
            return Err(ReconCode::DiffNotRecorded);
        }
        self.entries.push(e);
        Ok(())
    }
}

// ── 对账网 ────────────────────────────────────────────────────────────
/// 对账状态：Open → Reconciled 单向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReconState {
    /// 开网（可改格）
    Open,
    /// 已对账（封网只读）
    Reconciled,
}

/// 三方对账网：台账/批册/深化册 × 24 批。
#[derive(Clone, Debug)]
pub struct ReconGrid {
    /// 台账方
    pub ledger: [Option<Cell>; BATCH_COUNT],
    /// 批册方
    pub batch_book: [Option<Cell>; BATCH_COUNT],
    /// 深化册方
    pub deep_book: [Option<Cell>; BATCH_COUNT],
    /// 状态
    pub state: ReconState,
}

impl ReconGrid {
    /// 构造开网（三方全空）。
    pub const fn new() -> ReconGrid {
        ReconGrid {
            ledger: [None; BATCH_COUNT],
            batch_book: [None; BATCH_COUNT],
            deep_book: [None; BATCH_COUNT],
            state: ReconState::Open,
        }
    }

    /// 三方同批置同格（标准格）。
    pub fn set_uniform(&mut self, batch: u32, c: Cell) -> Result<(), ReconCode> {
        if self.state != ReconState::Open {
            return Err(ReconCode::SealedMutate);
        }
        if batch == 0 || batch as usize > BATCH_COUNT {
            return Err(ReconCode::BatchRange);
        }
        if !cell_in_range(&c) {
            return Err(ReconCode::CellRange);
        }
        let idx = batch as usize - 1;
        if let Some(slot) = self.ledger.get_mut(idx) {
            *slot = Some(c);
        }
        if let Some(slot) = self.batch_book.get_mut(idx) {
            *slot = Some(c);
        }
        if let Some(slot) = self.deep_book.get_mut(idx) {
            *slot = Some(c);
        }
        Ok(())
    }

    /// 单方置格（供注入与单侧改格）。
    pub fn set_side(
        &mut self,
        side: usize,
        batch: u32,
        c: Option<Cell>,
    ) -> Result<(), ReconCode> {
        if self.state != ReconState::Open {
            return Err(ReconCode::SealedMutate);
        }
        if batch == 0 || batch as usize > BATCH_COUNT {
            return Err(ReconCode::BatchRange);
        }
        if let Some(cc) = c {
            if !cell_in_range(&cc) {
                return Err(ReconCode::CellRange);
            }
        }
        let idx = batch as usize - 1;
        let target = match side {
            0 => &mut self.ledger,
            1 => &mut self.batch_book,
            2 => &mut self.deep_book,
            _ => return Err(ReconCode::BatchRange),
        };
        if let Some(slot) = target.get_mut(idx) {
            *slot = c;
        }
        Ok(())
    }

    /// 首差异定位：逐批逐指标扫描三方格值；None 格与有值格同视为差异。
    pub fn first_diff(&self) -> Option<DiffEntry> {
        let mut b = 1u32;
        while b as usize <= BATCH_COUNT {
            let idx = b as usize - 1;
            let tl = self.ledger.get(idx).copied().unwrap_or(None);
            let bb = self.batch_book.get(idx).copied().unwrap_or(None);
            let db = self.deep_book.get(idx).copied().unwrap_or(None);
            if tl.is_none() || bb.is_none() || db.is_none() {
                return Some(DiffEntry { batch: b, metric: Metric::Items });
            }
            let a = tl.unwrap_or(Cell { items: 0, lines: 0 });
            let x = bb.unwrap_or(Cell { items: 0, lines: 0 });
            let y = db.unwrap_or(Cell { items: 0, lines: 0 });
            if a.items != x.items || a.items != y.items {
                return Some(DiffEntry { batch: b, metric: Metric::Items });
            }
            if a.lines != x.lines || a.lines != y.lines {
                return Some(DiffEntry { batch: b, metric: Metric::Lines });
            }
            b += 1;
        }
        None
    }

    /// 差异总数（逐批逐指标全扫描）。
    pub fn diff_count(&self) -> usize {
        let mut n = 0usize;
        let mut b = 1u32;
        while b as usize <= BATCH_COUNT {
            let idx = b as usize - 1;
            let tl = self.ledger.get(idx).copied().unwrap_or(None);
            let bb = self.batch_book.get(idx).copied().unwrap_or(None);
            let db = self.deep_book.get(idx).copied().unwrap_or(None);
            match (tl, bb, db) {
                (Some(a), Some(x), Some(y)) => {
                    if a.items != x.items || a.items != y.items {
                        n += 1;
                    }
                    if a.lines != x.lines || a.lines != y.lines {
                        n += 1;
                    }
                }
                _ => n += 1,
            }
            b += 1;
        }
        n
    }

    /// 三方对账：逐差异入账；任何差异无法入账即显性拒绝（差异不记=拦截）。
    pub fn reconcile(&mut self, ledger: &mut DiffLedger) -> Result<usize, ReconCode> {
        if self.state != ReconState::Open {
            return Err(ReconCode::StateRegress);
        }
        let total = self.diff_count();
        let mut b = 1u32;
        while b as usize <= BATCH_COUNT {
            let idx = b as usize - 1;
            let tl = self.ledger.get(idx).copied().unwrap_or(None);
            let bb = self.batch_book.get(idx).copied().unwrap_or(None);
            let db = self.deep_book.get(idx).copied().unwrap_or(None);
            match (tl, bb, db) {
                (Some(a), Some(x), Some(y)) => {
                    if a.items != x.items || a.items != y.items {
                        ledger.record(DiffEntry { batch: b, metric: Metric::Items })?;
                    }
                    if a.lines != x.lines || a.lines != y.lines {
                        ledger.record(DiffEntry { batch: b, metric: Metric::Lines })?;
                    }
                }
                _ => {
                    ledger.record(DiffEntry { batch: b, metric: Metric::Items })?;
                }
            }
            b += 1;
        }
        self.state = ReconState::Reconciled;
        Ok(total)
    }

    /// 取差异结论：未对账拒绝。
    pub fn conclusion(&self) -> Result<usize, ReconCode> {
        if self.state != ReconState::Reconciled {
            return Err(ReconCode::NotReconciled);
        }
        Ok(self.diff_count())
    }
}

/// 标准对账网：24 批三方同格（20 条 / 6600 行）。
pub fn grid_standard() -> Result<ReconGrid, ReconCode> {
    let mut g = ReconGrid::new();
    let mut b = 1u32;
    while b as usize <= BATCH_COUNT {
        g.set_uniform(b, Cell { items: ITEMS_PER_BATCH, lines: LINES_PER_BATCH })?;
        b += 1;
    }
    Ok(g)
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏对账样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadReconCase {
    /// 单侧改格（深化册批 5 行数改动）
    SingleSide,
    /// 批号缺位（批 7 批册缺格）
    BatchMissing,
    /// 差异不记（差异超账容量，reconcile 显性拦截）
    DiffNotRecorded,
    /// 封网改格
    SealedMutate,
    /// 越界批号
    BatchRange,
}

impl BadReconCase {
    /// 闭集全量。
    pub const ALL: [BadReconCase; 5] = [
        BadReconCase::SingleSide,
        BadReconCase::BatchMissing,
        BadReconCase::DiffNotRecorded,
        BadReconCase::SealedMutate,
        BadReconCase::BatchRange,
    ];

    /// 注入一轮：构造坏样本并触发校验，返回是否被显式拒绝。
    pub fn inject(self) -> bool {
        match self {
            BadReconCase::SingleSide => {
                let mut g = match grid_standard() {
                    Ok(g) => g,
                    Err(_) => return false,
                };
                if g.set_side(2, 5, Some(Cell { items: ITEMS_PER_BATCH, lines: 6_000 })).is_err() {
                    return false;
                }
                let mut dl = DiffLedger::new();
                match g.reconcile(&mut dl) {
                    Ok(1) => g.first_diff() == Some(DiffEntry { batch: 5, metric: Metric::Lines })
                        && dl.entries.len() == 1,
                    _ => false,
                }
            }
            BadReconCase::BatchMissing => {
                let mut g = match grid_standard() {
                    Ok(g) => g,
                    Err(_) => return false,
                };
                if g.set_side(1, 7, None).is_err() {
                    return false;
                }
                let mut dl = DiffLedger::new();
                match g.reconcile(&mut dl) {
                    Ok(1) => dl.entries.first().copied()
                        == Some(DiffEntry { batch: 7, metric: Metric::Items }),
                    _ => false,
                }
            }
            BadReconCase::DiffNotRecorded => {
                // 20 批 × 2 指标 = 40 处差异 > 账容量 48？取 25 批侧不现实，
                // 改用小账：cap=3 的账承受 4 处差异，必触发 DiffNotRecorded。
                let mut g = match grid_standard() {
                    Ok(g) => g,
                    Err(_) => return false,
                };
                let mut b = 1u32;
                while b <= 2 {
                    if g.set_side(2, b, Some(Cell { items: 10, lines: 1_000 })).is_err() {
                        return false;
                    }
                    b += 1;
                }
                let mut small = DiffLedger { entries: Vec::new(), cap: 3, escapes: 0 };
                let rejected = g.reconcile(&mut small) == Err(ReconCode::DiffNotRecorded);
                // 2 批 × 2 指标 = 4 差异 > cap 3：前 3 笔入账，第 4 笔逃逸。
                rejected && small.escapes == 1 && small.entries.len() == 3
            }
            BadReconCase::SealedMutate => {
                let mut g = match grid_standard() {
                    Ok(g) => g,
                    Err(_) => return false,
                };
                let mut dl = DiffLedger::new();
                if g.reconcile(&mut dl).is_err() {
                    return false;
                }
                g.set_uniform(1, Cell { items: 20, lines: 6_600 }).is_err()
                    && g.set_side(0, 1, None).is_err()
            }
            BadReconCase::BatchRange => {
                let mut g = ReconGrid::new();
                g.set_uniform(0, Cell { items: 20, lines: 6_600 }).is_err()
                    && g.set_uniform(25, Cell { items: 20, lines: 6_600 }).is_err()
                    && g.set_side(0, 0, None).is_err()
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_grid_injection() -> (DiffLedger, [u32; 5]) {
    let mut ledger = DiffLedger::new();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadReconCase::ALL.len() {
        if let Some(case) = BadReconCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject();
                if !caught {
                    ledger.escapes = ledger.escapes.saturating_add(1);
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
    ("EBUSY", -16, 170, "Windows ERROR_RESOURCE_BUSY；封网改格族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；批号缺位族"),
    ("EIO", -5, 1117, "Windows ERROR_IO_DEVICE；三方不一致族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；口径/格式族"),
    ("ENOSPC", -28, 112, "Windows ERROR_DISK_FULL；差异账满族"),
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

/// 规范语料双跑指纹：标准网→对账→单侧差异→差异账全链入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut g = match grid_standard() {
        Ok(g) => g,
        Err(_) => ReconGrid::new(),
    };
    let mut dl = DiffLedger::new();
    bytes.push(match g.reconcile(&mut dl) {
        Ok(0) => 1,
        _ => 2,
    });
    bytes.push(match g.conclusion() {
        Ok(0) => 3,
        _ => 4,
    });
    // 单侧差异场景：批 5 深化册行数 6000。
    let mut g2 = match grid_standard() {
        Ok(g) => g,
        Err(_) => ReconGrid::new(),
    };
    let _ = g2.set_side(2, 5, Some(Cell { items: ITEMS_PER_BATCH, lines: 6_000 }));
    let mut dl2 = DiffLedger::new();
    bytes.push(match g2.reconcile(&mut dl2) {
        Ok(1) => 5,
        _ => 6,
    });
    bytes.extend_from_slice(&(dl2.entries.len() as u32).to_le_bytes());
    bytes.push(match g2.first_diff() {
        Some(d) => d.batch as u8,
        None => 0,
    });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10067 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx10067_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10067");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x92080..=0x920c0", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝");

    // 2. 标准网：24 批三方同格对账零差异
    let mut g = match grid_standard() {
        Ok(g) => g,
        Err(_) => ReconGrid::new(),
    };
    let mut dl = DiffLedger::new();
    let std_ok = g.reconcile(&mut dl) == Ok(0)
        && g.conclusion() == Ok(0)
        && g.state == ReconState::Reconciled;
    s.add("三方对账：24 批逐格零差异、结论=0", std_ok,
          "台账/批册/深化册三方同格（20 条/6600 行）");

    // 3. 格行解析器
    let parse_ok = parse_recon_row("5|20|6600") == Ok((5, Cell { items: 20, lines: 6600 }))
        && parse_recon_row("5|20").is_err()
        && parse_recon_row("5|20|x").is_err()
        && parse_recon_row("|20|6600").is_err();
    s.add("格行解析：batch|items|lines 三段闭集", parse_ok,
          "缺段/非数字/空段全拒");

    // 4. 格值口径守卫
    let mut cr = ReconGrid::new();
    let cr_ok = cr.set_uniform(1, Cell { items: 0, lines: 6_600 }).is_err()
        && cr.set_uniform(1, Cell { items: 20, lines: 0 }).is_err()
        && cr.set_uniform(1, Cell { items: 41, lines: 6_600 }).is_err()
        && cr.set_uniform(1, Cell { items: 20, lines: 6_600 }).is_ok();
    s.add("口径窗：条数 1..=40、行数 1..=26400", cr_ok,
          "零口径/超上界格值显性拒绝");

    // 5. 越界批号拦截
    let mut br = ReconGrid::new();
    let br_ok = br.set_uniform(0, Cell { items: 20, lines: 6_600 }).is_err()
        && br.set_uniform(25, Cell { items: 20, lines: 6_600 }).is_err();
    s.add("批号窗：0/25 拒收", br_ok,
          "M 型段只允许批号 1..=24");

    // 6. 单侧改格：差异定位到批+指标
    let mut sg = match grid_standard() {
        Ok(g) => g,
        Err(_) => ReconGrid::new(),
    };
    let _ = sg.set_side(2, 5, Some(Cell { items: ITEMS_PER_BATCH, lines: 6_000 }));
    let mut sg_dl = DiffLedger::new();
    let sg_ok = sg.reconcile(&mut sg_dl) == Ok(1)
        && sg.first_diff() == Some(DiffEntry { batch: 5, metric: Metric::Lines })
        && sg_dl.entries.len() == 1;
    s.add("单侧改格：1 处差异入账并定位（批5·行数）", sg_ok,
          "差异必须带批号+指标定位单");

    // 7. 批号缺位：缺格即差异
    let mut mg = match grid_standard() {
        Ok(g) => g,
        Err(_) => ReconGrid::new(),
    };
    let _ = mg.set_side(1, 7, None);
    let mut mg_dl = DiffLedger::new();
    let mg_ok = mg.reconcile(&mut mg_dl) == Ok(1)
        && mg_dl.entries.first().copied() == Some(DiffEntry { batch: 7, metric: Metric::Items });
    s.add("批号缺位：缺格计入差异并定位批 7", mg_ok,
          "三方须同批在格，缺位显性报差");

    // 8. 差异不记：账满即显性拦截且逃逸入账
    let mut fg = match grid_standard() {
        Ok(g) => g,
        Err(_) => ReconGrid::new(),
    };
    let _ = fg.set_side(2, 1, Some(Cell { items: 10, lines: 1_000 }));
    let _ = fg.set_side(2, 2, Some(Cell { items: 10, lines: 1_000 }));
    let mut small = DiffLedger { entries: Vec::new(), cap: 3, escapes: 0 };
    let fg_ok = fg.reconcile(&mut small) == Err(ReconCode::DiffNotRecorded)
        && small.escapes == 1
        && small.entries.len() == 3;
    s.add("差异不记：账满差异显性 Err 拦截+逃逸入账", fg_ok,
          "4 差异对 cap3 账：3 入账 1 逃逸，绝不静默");

    // 9. 差异账重复入账拦截
    let mut dd = DiffLedger::new();
    let _ = dd.record(DiffEntry { batch: 3, metric: Metric::Items });
    let dd_ok = dd.record(DiffEntry { batch: 3, metric: Metric::Items })
        == Err(ReconCode::DuplicateDiff);
    s.add("重复入账：同批同指标差异二次记被拒", dd_ok,
          "一差异一账位");

    // 10. 封网改格拦截 + 未对账出结论拦截
    let mut sm = match grid_standard() {
        Ok(g) => g,
        Err(_) => ReconGrid::new(),
    };
    let mut sm_dl = DiffLedger::new();
    let _ = sm.reconcile(&mut sm_dl);
    let sealed_ok = sm.set_uniform(1, Cell { items: 20, lines: 6_600 }).is_err()
        && sm.set_side(0, 1, None).is_err();
    let fresh = ReconGrid::new();
    let nr_ok = fresh.conclusion().is_err();
    s.add("状态闸：封网改格拒、未对账出结论拒", sealed_ok && nr_ok,
          "Open → Reconciled 单向，结论只在封网后出");

    // 11. 对账幂等：同网两次对账差异结论一致
    let g_a = grid_standard();
    let g_b = grid_standard();
    let mut dl_a = DiffLedger::new();
    let mut dl_b = DiffLedger::new();
    let idem = match (g_a, g_b) {
        (Ok(mut a), Ok(mut b)) => {
            let ra = a.reconcile(&mut dl_a);
            let rb = b.reconcile(&mut dl_b);
            ra == rb && a.conclusion() == b.conclusion()
        }
        _ => false,
    };
    s.add("对账幂等：同网两次结论逐位一致", idem,
          "对账算法可复算：同输入同差异集");

    // 12. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_grid_injection();
    let all_caught = per_class.iter().all(|c| *c == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "单侧/缺位/差异不记/封网改格/越界全被显式拒绝");

    // 13. 账守恒逃逸=0
    s.add("账守恒：注入 50 轮逃逸=0",
          ledger.escapes == 0 && ledger.entries.is_empty(),
          "注入核全程不得产生差异账逃逸");

    // 14. 文案闭集：非空
    let text_ok = ReconCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 15. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i = 0usize;
    while i < ReconCode::ALL.len() {
        let mut j = i + 1;
        while j < ReconCode::ALL.len() {
            if let (Some(x), Some(y)) = (ReconCode::ALL.get(i), ReconCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let in_seg = ReconCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 16. Linux 逐码一致
    let codes_ok = agrees_with_linux(EBUSY)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EIO)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(ENOSPC)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 17. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 动态明细（≤8）：对账快照 + 指纹快照
    let snap_text = format!("差异 {}/48 / 结论 {:?}", dl.entries.len(), g.conclusion());
    s.add_dyn("对账快照", g.conclusion() == Ok(0), &snap_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_grid_zero_diff() {
        let mut g = grid_standard().unwrap_or(ReconGrid::new());
        let mut dl = DiffLedger::new();
        assert_eq!(g.reconcile(&mut dl), Ok(0));
        assert_eq!(g.conclusion(), Ok(0));
    }

    #[test]
    fn parse_rows() {
        assert_eq!(parse_recon_row("5|20|6600"), Ok((5, Cell { items: 20, lines: 6600 })));
        assert!(parse_recon_row("5|20").is_err());
        assert!(parse_recon_row("a|20|6600").is_err());
    }

    #[test]
    fn cell_range_guard() {
        assert!(!cell_in_range(&Cell { items: 0, lines: 100 }));
        assert!(!cell_in_range(&Cell { items: 20, lines: 0 }));
        assert!(cell_in_range(&Cell { items: 20, lines: 6_600 }));
    }

    #[test]
    fn single_side_located() {
        let mut g = grid_standard().unwrap_or(ReconGrid::new());
        let _ = g.set_side(2, 5, Some(Cell { items: 20, lines: 6_000 }));
        let mut dl = DiffLedger::new();
        assert_eq!(g.reconcile(&mut dl), Ok(1));
        assert_eq!(g.first_diff(), Some(DiffEntry { batch: 5, metric: Metric::Lines }));
    }

    #[test]
    fn missing_batch_detected() {
        let mut g = grid_standard().unwrap_or(ReconGrid::new());
        let _ = g.set_side(1, 7, None);
        let mut dl = DiffLedger::new();
        assert_eq!(g.reconcile(&mut dl), Ok(1));
        assert_eq!(dl.entries.first().copied(), Some(DiffEntry { batch: 7, metric: Metric::Items }));
    }

    #[test]
    fn full_diff_ledger_rejected() {
        let mut g = grid_standard().unwrap_or(ReconGrid::new());
        let _ = g.set_side(2, 1, Some(Cell { items: 10, lines: 1_000 }));
        let _ = g.set_side(2, 2, Some(Cell { items: 10, lines: 1_000 }));
        let mut small = DiffLedger { entries: Vec::new(), cap: 3, escapes: 0 };
        assert_eq!(g.reconcile(&mut small), Err(ReconCode::DiffNotRecorded));
        assert_eq!(small.escapes, 1);
    }

    #[test]
    fn sealed_grid_readonly() {
        let mut g = grid_standard().unwrap_or(ReconGrid::new());
        let mut dl = DiffLedger::new();
        let _ = g.reconcile(&mut dl);
        assert!(g.set_uniform(1, Cell { items: 20, lines: 6_600 }).is_err());
        assert_eq!(ReconGrid::new().conclusion(), Err(ReconCode::NotReconciled));
    }

    #[test]
    fn duplicate_diff_rejected() {
        let mut dl = DiffLedger::new();
        let _ = dl.record(DiffEntry { batch: 3, metric: Metric::Items });
        assert_eq!(dl.record(DiffEntry { batch: 3, metric: Metric::Items }),
                   Err(ReconCode::DuplicateDiff));
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_grid_injection();
        assert_eq!(l.escapes, 0);
        assert!(l.entries.is_empty());
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EBUSY));
        assert!(agrees_with_linux(ENOSPC));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EINVAL), -EINVAL);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10067_checks_green() {
        let s = run_unx10067_checks();
        assert!(s.all_passed(), "UNX-F10067 red: {}", s.red_items_text());
    }
}
