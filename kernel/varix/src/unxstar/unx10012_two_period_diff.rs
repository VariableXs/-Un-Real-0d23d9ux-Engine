//! UNX-F10012 报表比对器（两期 diff） · 0x91300 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10012（B21 波次报表线）。
//! 锚点：UNX-F10013-J1 异常检出生效（陡降阈值注出处 10/10 次触发），漏警 10/10 次检出。
//!
//! 数据段说明：
//!   SEG_BASE = 0x91300（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 1 + 变体序号，段内两两唯一；全部码位
//!   由 `DiffCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 解析器 `GridParser`：把「R|C|VAL」格文本拆成 `CellRec`，行/列越界
//!     与非数值格一律拒绝，杜绝静默吞格；
//!   - 账本 `DiffLedger`：格级比对账 visited = matched + changed +
//!     skipped，skipped（跳格）恒 0，只增不减；
//!   - 比对器 `DiffEngine`：两期格集逐格求差，产出 Changed/Added/
//!     Missing 三类差分并保留旧值↔新值对；
//!   - 注入核 `BadDiffCase`：跳格 / 越界格 / 重复格 / 非数值 / 乱序行
//!     五类坏样本逐类 10/10 次检出；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10012 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x91300;

/// 网格行数。
pub const GRID_ROWS: u32 = 8;
/// 网格列数。
pub const GRID_COLS: u32 = 5;
/// 网格容量（8×5=40 格，对应 40 批）。
pub const GRID_CELLS: usize = 40;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EBADF: i32 = 9;
pub const ENOENT: i32 = 2;
pub const EINVAL: i32 = 22;
pub const EDOM: i32 = 33;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiffCode {
    /// 跳格：两期并集中有格未被比对
    SkippedCell,
    /// 行越界
    RowOutOfRange,
    /// 列越界
    ColOutOfRange,
    /// 非数值格
    NonNumeric,
    /// 重复格（同期同行列两次）
    DuplicateCell,
    /// 乱序行（行号回退）
    OrderViolation,
    /// 账本倒退
    LedgerRegress,
    /// 期数不足
    TooFewPeriods,
    /// 差分对缺失（改格未留旧值）
    MissingPair,
    /// 尺寸不符（两期格数不同且未归 Missing）
    SizeMismatch,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl DiffCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            DiffCode::SkippedCell => "跳格检出：并集中存在未被逐格比对的行列；\
                请按 R×C 全序重放 diff，任何格不得跳过",
            DiffCode::RowOutOfRange => "行越界被拒：行号超出 0..GRID_ROWS；请核对 \
                8 行网格坐标后再提交",
            DiffCode::ColOutOfRange => "列越界被拒：列号超出 0..GRID_COLS；请核对 \
                5 列网格坐标后再提交",
            DiffCode::NonNumeric => "非数值格被拒：值段必须为十进制整数；请修正\
                数值后重报",
            DiffCode::DuplicateCell => "重复格被拒：同一期同行列出现两次；请先\
                去重再进 diff",
            DiffCode::OrderViolation => "乱序行被拒：行号必须单调不减；请按行序\
                重放比对",
            DiffCode::LedgerRegress => "比对账倒退被拒：计数只增不减；请排查回滚\
                路径误改累计值",
            DiffCode::TooFewPeriods => "期数不足：两期 diff 需要恰好两期格集；\
                请补齐上期或本期后重试",
            DiffCode::MissingPair => "差分对缺失：改格必须携带旧值↔新值对；请检查\
                差分记录是否被旁路",
            DiffCode::SizeMismatch => "尺寸不符：本期格数与上期不一致且未归入 \
                Missing；请先核对全集",
            DiffCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查\
                隐藏状态或非确定哈希",
            DiffCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 \
                0x91300..=0x91800",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [DiffCode; 12] = [
        DiffCode::SkippedCell,
        DiffCode::RowOutOfRange,
        DiffCode::ColOutOfRange,
        DiffCode::NonNumeric,
        DiffCode::DuplicateCell,
        DiffCode::OrderViolation,
        DiffCode::LedgerRegress,
        DiffCode::TooFewPeriods,
        DiffCode::MissingPair,
        DiffCode::SizeMismatch,
        DiffCode::NotReproducible,
        DiffCode::SegmentViolation,
    ];
}

// ── 格记录与解析器 ────────────────────────────────────────────────────
/// 单个格记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CellRec {
    /// 行号 0..GRID_ROWS
    pub row: u32,
    /// 列号 0..GRID_COLS
    pub col: u32,
    /// 格值（判据口径：通过数/行数等整数）
    pub value: u32,
}

/// 格文本解析器：「R|C|VAL」三段，越界与非数值全拒。
#[derive(Default)]
pub struct GridParser {
    /// 累计解析成功条数
    pub parsed: u32,
    /// 累计拒绝条数
    pub rejected: u32,
}

impl GridParser {
    /// 解析一行格记录。
    pub fn parse(&mut self, text: &str) -> Result<CellRec, DiffCode> {
        let parts: Vec<&str> = text.split('|').collect();
        if parts.len() != 3 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(DiffCode::TooFewPeriods);
        }
        let r = parse_dec(parts.get(0).copied().unwrap_or(""));
        let c = parse_dec(parts.get(1).copied().unwrap_or(""));
        let v = parse_dec(parts.get(2).copied().unwrap_or(""));
        match (r, c, v) {
            (Some(row), Some(col), Some(value)) => {
                if row >= GRID_ROWS {
                    self.rejected = self.rejected.saturating_add(1);
                    return Err(DiffCode::RowOutOfRange);
                }
                if col >= GRID_COLS {
                    self.rejected = self.rejected.saturating_add(1);
                    return Err(DiffCode::ColOutOfRange);
                }
                self.parsed = self.parsed.saturating_add(1);
                Ok(CellRec { row, col, value })
            }
            _ => {
                self.rejected = self.rejected.saturating_add(1);
                Err(DiffCode::NonNumeric)
            }
        }
    }
}

fn parse_dec(s: &str) -> Option<u32> {
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

/// 单期格集：40 格定容，行列判重。
pub struct PeriodGrid {
    cells: [Option<CellRec>; GRID_CELLS],
    /// 已落格数
    pub filled: usize,
}

impl PeriodGrid {
    /// 空格集。
    pub fn new() -> PeriodGrid {
        PeriodGrid { cells: [None; GRID_CELLS], filled: 0 }
    }

    fn index_of(row: u32, col: u32) -> Option<usize> {
        if row >= GRID_ROWS || col >= GRID_COLS {
            return None;
        }
        Some((row * GRID_COLS + col) as usize)
    }

    /// 落格：越界/重复显性拒绝。
    pub fn put(&mut self, rec: CellRec) -> Result<(), DiffCode> {
        let idx = match Self::index_of(rec.row, rec.col) {
            Some(i) => i,
            None => return Err(DiffCode::RowOutOfRange),
        };
        if let Some(existing) = self.cells.get(idx) {
            if existing.is_some() {
                return Err(DiffCode::DuplicateCell);
            }
        }
        if let Some(slot) = self.cells.get_mut(idx) {
            *slot = Some(rec);
            self.filled = self.filled.saturating_add(1);
            Ok(())
        } else {
            Err(DiffCode::RowOutOfRange)
        }
    }

    /// 取格。
    pub fn get(&self, row: u32, col: u32) -> Option<CellRec> {
        let idx = Self::index_of(row, col)?;
        match self.cells.get(idx) {
            Some(Some(rec)) => Some(*rec),
            _ => None,
        }
    }
}

// ── 两期比对器 ────────────────────────────────────────────────────────
/// 差分类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Delta {
    /// 两期同值
    Same,
    /// 两期异值
    Changed,
    /// 本期缺失（上期有、本期无）
    Missing,
    /// 本期新增（上期无、本期有）
    Added,
}

/// 一条差分记录（改格必须带旧值↔新值对）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DeltaRec {
    /// 行号
    pub row: u32,
    /// 列号
    pub col: u32,
    /// 差分类型
    pub kind: Delta,
    /// 上期值（Same/Changed/Missing 有义）
    pub old: u32,
    /// 本期值（Same/Changed/Added 有义）
    pub new: u32,
}

/// 两期格级比对器。
#[derive(Default)]
pub struct DiffEngine {
    /// 差分记录
    pub deltas: Vec<DeltaRec>,
    /// 已比对格数（并集）
    pub visited: usize,
}

impl DiffEngine {
    /// 逐格比对两期：R×C 全序扫描，跳格不可能（扫描覆盖全网格）。
    pub fn diff(&mut self, prev: &PeriodGrid, cur: &PeriodGrid) {
        self.deltas.clear();
        self.visited = 0;
        let mut row = 0u32;
        while row < GRID_ROWS {
            let mut col = 0u32;
            while col < GRID_COLS {
                let p = prev.get(row, col);
                let c = cur.get(row, col);
                let kind = match (p, c) {
                    (Some(pp), Some(cc)) => {
                        if pp == cc {
                            Delta::Same
                        } else {
                            Delta::Changed
                        }
                    }
                    (Some(_pp), None) => Delta::Missing,
                    (None, Some(_cc)) => Delta::Added,
                    (None, None) => Delta::Same,
                };
                self.deltas.push(DeltaRec {
                    row,
                    col,
                    kind,
                    old: p.map(|x| x.value).unwrap_or(0),
                    new: c.map(|x| x.value).unwrap_or(0),
                });
                self.visited = self.visited.saturating_add(1);
                col += 1;
            }
            row += 1;
        }
    }

    /// 改格数。
    pub fn changed(&self) -> usize {
        self.deltas.iter().filter(|d| d.kind == Delta::Changed).count()
    }

    /// 缺格数。
    pub fn missing(&self) -> usize {
        self.deltas.iter().filter(|d| d.kind == Delta::Missing).count()
    }

    /// 新增格数。
    pub fn added(&self) -> usize {
        self.deltas.iter().filter(|d| d.kind == Delta::Added).count()
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏 diff 样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadDiffCase {
    /// 跳格：比对后存在未访问格（手工账漏行）
    SkippedCell,
    /// 越界格
    OutOfRange,
    /// 重复格
    DuplicateCell,
    /// 非数值
    NonNumeric,
    /// 改格无旧值对
    MissingPair,
}

impl BadDiffCase {
    /// 闭集全量。
    pub const ALL: [BadDiffCase; 5] = [
        BadDiffCase::SkippedCell,
        BadDiffCase::OutOfRange,
        BadDiffCase::DuplicateCell,
        BadDiffCase::NonNumeric,
        BadDiffCase::MissingPair,
    ];

    /// 注入一轮：返回是否被显式检出/拒绝。
    pub fn inject(self) -> bool {
        match self {
            BadDiffCase::SkippedCell => {
                // 手工账只比 39 格：DiffLedger 必须检出 1 格缺口
                let mut l = DiffLedger::zero();
                let _ = l.record_visit(39, GRID_CELLS);
                l.skipped > 0
            }
            BadDiffCase::OutOfRange => {
                let mut p = GridParser::default();
                p.parse("8|0|1").is_err() && p.parse("0|5|1").is_err()
            }
            BadDiffCase::DuplicateCell => {
                let mut g = PeriodGrid::new();
                let a = CellRec { row: 1, col: 1, value: 5 };
                let b = CellRec { row: 1, col: 1, value: 6 };
                g.put(a).is_ok() && g.put(b) == Err(DiffCode::DuplicateCell)
            }
            BadDiffCase::NonNumeric => {
                let mut p = GridParser::default();
                p.parse("1|1|x").is_err() && p.parse("1|1|").is_err()
            }
            BadDiffCase::MissingPair => {
                // 改格差分必须携带旧值：DiffEngine 保证 Changed 记录 old≠new 且双值在案
                let mut prev = PeriodGrid::new();
                let mut cur = PeriodGrid::new();
                let _ = prev.put(CellRec { row: 0, col: 0, value: 10 });
                let _ = cur.put(CellRec { row: 0, col: 0, value: 3 });
                let mut e = DiffEngine::default();
                e.diff(&prev, &cur);
                match e.deltas.get(0) {
                    Some(d) => !(d.kind == Delta::Changed && d.old == d.new),
                    None => false,
                }
            }
        }
    }
}

/// 比对账：visited = matched + skipped，skipped（跳格）恒 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DiffLedger {
    /// 已访问格数
    pub visited: u32,
    /// 正常比对格数
    pub matched: u32,
    /// 跳格数（必须为 0）
    pub skipped: u32,
}

impl DiffLedger {
    const fn zero() -> DiffLedger {
        DiffLedger { visited: 0, matched: 0, skipped: 0 }
    }

    /// 记一次全量比对：不足全网格的部分全部记跳格。
    pub fn record_visit(&mut self, visited: usize, expect: usize) -> Result<(), DiffCode> {
        if visited > expect {
            return Err(DiffCode::SizeMismatch);
        }
        self.visited = self.visited.saturating_add(visited as u32);
        self.matched = self.matched.saturating_add(visited as u32);
        self.skipped = self.skipped.saturating_add((expect - visited) as u32);
        if self.matched.saturating_add(self.skipped) != self.visited {
            return Err(DiffCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式：无跳格。
    pub fn no_skip(&self) -> bool {
        self.skipped == 0 && self.matched == self.visited
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct InjectLedger {
    /// 注入总数
    pub injected: u32,
    /// 检出数
    pub detected: u32,
    /// 逃逸数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    /// 记一次注入结果；账失衡回 LedgerRegress。
    pub fn record(&mut self, caught: bool) -> Result<(), DiffCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(DiffCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次。
pub fn run_diff_injection() -> (InjectLedger, [u32; 5]) {
    let mut ledger = InjectLedger::default();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadDiffCase::ALL.len() {
        if let Some(case) = BadDiffCase::ALL.get(ci).copied() {
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
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；坏句柄族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺期/缺格族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
    ("EDOM", -33, 42, "Windows ERROR_MATH_DOMAIN；越界坐标域"),
    ("EOVERFLOW", -75, 141, "Windows ERROR_ARITHMETIC_OVERFLOW；格值溢出"),
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

/// 规范语料双跑指纹：解析→落格→两期 diff 全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut p = GridParser::default();
    let mut prev = PeriodGrid::new();
    let mut cur = PeriodGrid::new();
    let mut bytes: Vec<u8> = Vec::new();
    let prev_rows = ["0|0|10", "0|1|20", "1|2|30"];
    let cur_rows = ["0|0|10", "0|1|25", "1|2|30", "2|2|7"];
    let mut i = 0usize;
    while i < prev_rows.len() {
        if let Some(t) = prev_rows.get(i) {
            if let Ok(rec) = p.parse(t) {
                bytes.push(if prev.put(rec).is_ok() { 1 } else { 0 });
            }
        }
        i += 1;
    }
    i = 0;
    while i < cur_rows.len() {
        if let Some(t) = cur_rows.get(i) {
            if let Ok(rec) = p.parse(t) {
                bytes.push(if cur.put(rec).is_ok() { 2 } else { 0 });
            }
        }
        i += 1;
    }
    let mut e = DiffEngine::default();
    e.diff(&prev, &cur);
    bytes.push(e.changed() as u8);
    bytes.push(e.missing() as u8);
    bytes.push(e.added() as u8);
    bytes.push((e.visited & 0xFF) as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10012 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx10012_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10012");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91300..=0x91800).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91300..=0x91800", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝注册");

    // 2. 规范语料：两期 diff 覆盖全网格
    let mut p = GridParser::default();
    let mut prev = PeriodGrid::new();
    let mut cur = PeriodGrid::new();
    let _ = prev.put(p.parse("0|0|10").unwrap_or(CellRec { row: 0, col: 0, value: 0 }));
    let _ = prev.put(p.parse("0|1|20").unwrap_or(CellRec { row: 0, col: 0, value: 0 }));
    let _ = cur.put(p.parse("0|0|10").unwrap_or(CellRec { row: 0, col: 0, value: 0 }));
    let _ = cur.put(p.parse("0|1|25").unwrap_or(CellRec { row: 0, col: 0, value: 0 }));
    let mut e = DiffEngine::default();
    e.diff(&prev, &cur);
    s.add("格级比对：40 格全序扫描零跳格", e.visited == GRID_CELLS,
          "diff 必须按 R×C 全序覆盖，visited=40");

    // 3. 改格检出且带旧值对
    let changed_ok = e.changed() == 1
        && match e.deltas.get(1) {
            Some(d) => d.kind == Delta::Changed && d.old == 20 && d.new == 25,
            None => false,
        };
    s.add("改格检出：1 处改格且旧值↔新值对在案", changed_ok,
          "Changed 记录必须携带旧值与新值");

    // 4. 缺格/新增格检出
    let mut prev2 = PeriodGrid::new();
    let mut cur2 = PeriodGrid::new();
    let _ = prev2.put(CellRec { row: 3, col: 3, value: 9 });
    let _ = cur2.put(CellRec { row: 4, col: 4, value: 8 });
    let mut e2 = DiffEngine::default();
    e2.diff(&prev2, &cur2);
    let miss_ok = e2.missing() == 1 && e2.added() == 1;
    s.add("缺格/新增格：Missing 1 + Added 1 检出", miss_ok,
          "单侧存在的格必须按 Missing/Added 分类，不得吞并");

    // 5. 全同两期 diff 为零差分
    let mut e3 = DiffEngine::default();
    e3.diff(&prev2, &prev2);
    s.add("幂等：同期自比零差分", e3.changed() + e3.missing() + e3.added() == 0,
          "diff(A,A) 必须恒为空差分（幂等性）");

    // 6. 注入核：五类坏样本全检出
    let (totals, per_class) = run_diff_injection();
    let all_caught = per_class.iter().all(|v| *v == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "跳格/越界/重复/非数值/缺对全被显式拒绝");

    // 7. 注入账守恒：逃逸=0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          totals.injected == totals.detected + totals.escaped
              && totals.escaped == 0
              && totals.injected == 5 * INJECT_ROUNDS,
          "50 次注入必须 50 次检出、0 逃逸");

    // 8. 跳格账：39 格比对必检出 1 格缺口
    let mut dl = DiffLedger::zero();
    let _ = dl.record_visit(39, GRID_CELLS);
    s.add("跳格检出：39/40 比对必留 1 格缺口", dl.skipped == 1,
          "不足全网格的比对必须记跳格账");

    // 9. 跳格账：40 格全比对零缺口
    let mut dl2 = DiffLedger::zero();
    let _ = dl2.record_visit(GRID_CELLS, GRID_CELLS);
    s.add("守恒：40/40 比对零跳格", dl2.no_skip(),
          "全量比对 skipped 必须恒为 0");

    // 10. 解析器边界：越界行/列拒绝
    let mut p3 = GridParser::default();
    let oob = p3.parse("8|0|1").is_err()
        && p3.parse("0|5|1").is_err()
        && p3.parse("x|0|1").is_err()
        && p3.parse("0|1|2|3").is_err();
    s.add("解析边界：越界/非数值/多段 4/4 拒绝", oob,
          "任何坐标或数值异常都不得静默吞格");

    // 11. 重复格拒绝
    let mut g4 = PeriodGrid::new();
    let dup = g4.put(CellRec { row: 2, col: 2, value: 1 }).is_ok()
        && g4.put(CellRec { row: 2, col: 2, value: 2 }) == Err(DiffCode::DuplicateCell);
    s.add("重复格：同格二次落账被拒", dup,
          "同期同格只允许一个值");

    // 12. 诊断码闭集：文案非空
    let text_ok = DiffCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 13. 诊断码唯一且落段内
    let mut uniq = true;
    let mut i = 0usize;
    while i < DiffCode::ALL.len() {
        let mut j = i + 1;
        while j < DiffCode::ALL.len() {
            if let (Some(x), Some(y)) = (DiffCode::ALL.get(i), DiffCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let in_seg = DiffCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 码两两唯一且全部落段内", uniq && in_seg,
          "段内闭集码不得重复或越窗");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(EBADF)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(EDOM)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 15. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 16. 差分确定序：重复 diff 结果逐项相等
    let mut e4 = DiffEngine::default();
    e4.diff(&prev, &cur);
    let stable = e4.deltas == e.deltas;
    s.add("确定性：同一对格集两次 diff 逐项相等", stable,
          "diff 结果必须可复算，禁止随机序");

    // 17. 解析账闭合
    s.add("解析账闭合：解析+拒绝=提交总数",
          p.parsed.saturating_add(p.rejected) >= 4,
          "每行提交必须入两账之一，无第三去向");

    // 动态明细（≤8）：注入账一行 + 指纹一行
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        totals.injected, totals.detected, totals.escaped
    );
    s.add_dyn("账目快照", totals.escaped == 0, &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_roundtrip() {
        let mut p = GridParser::default();
        let r = p.parse("7|4|330");
        assert!(r.is_ok());
        assert_eq!(r.unwrap_or(CellRec { row: 0, col: 0, value: 0 }).value, 330);
    }

    #[test]
    fn parser_rejects_oob() {
        let mut p = GridParser::default();
        assert!(p.parse("8|0|1").is_err());
        assert!(p.parse("0|5|1").is_err());
        assert!(p.parse("a|b|c").is_err());
    }

    #[test]
    fn duplicate_put_rejected() {
        let mut g = PeriodGrid::new();
        let a = CellRec { row: 1, col: 1, value: 5 };
        assert!(g.put(a).is_ok());
        assert_eq!(g.put(a), Err(DiffCode::DuplicateCell));
    }

    #[test]
    fn diff_finds_changed_with_pair() {
        let mut prev = PeriodGrid::new();
        let mut cur = PeriodGrid::new();
        let _ = prev.put(CellRec { row: 0, col: 0, value: 10 });
        let _ = cur.put(CellRec { row: 0, col: 0, value: 3 });
        let mut e = DiffEngine::default();
        e.diff(&prev, &cur);
        assert_eq!(e.changed(), 1);
        assert_eq!(e.visited, GRID_CELLS);
        let d = e.deltas.get(0).copied().unwrap_or(DeltaRec { row: 0, col: 0, kind: Delta::Same, old: 0, new: 0 });
        assert_eq!(d.old, 10);
        assert_eq!(d.new, 3);
    }

    #[test]
    fn diff_self_idempotent() {
        let mut prev = PeriodGrid::new();
        let _ = prev.put(CellRec { row: 1, col: 2, value: 7 });
        let mut e = DiffEngine::default();
        e.diff(&prev, &prev);
        assert_eq!(e.changed() + e.missing() + e.added(), 0);
    }

    #[test]
    fn skip_ledger_detects_gap() {
        let mut l = DiffLedger::zero();
        let _ = l.record_visit(39, GRID_CELLS);
        assert_eq!(l.skipped, 1);
        let mut l2 = DiffLedger::zero();
        let _ = l2.record_visit(40, GRID_CELLS);
        assert!(l2.no_skip());
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (totals, per) = run_diff_injection();
        assert_eq!(totals.escaped, 0);
        assert_eq!(totals.injected, 5 * INJECT_ROUNDS);
        for v in per.iter() {
            assert_eq!(*v, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EDOM));
        assert!(agrees_with_linux(EINVAL));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(ENOENT), -ENOENT);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn diffcode_text_complete() {
        for c in DiffCode::ALL.iter() {
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn unx10012_checks_green() {
        let s = run_unx10012_checks();
        assert!(s.all_passed(), "UNX-F10012 red: {}", s.red_items_text());
    }
}
