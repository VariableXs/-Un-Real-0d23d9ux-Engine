//! UNX-F50034 布局向量账（编排用例集+校验和）（0x43060 段）
//!
//! 锚点：UNX-F50034-J1 用例可复跑；判据锚定 §AI-63 §5。
//!
//! 数据段：`SEG_BASE = 0x43060`，诊断码闭集 `0x43060 + 1..=0x43060 + 11`。
//!
//! 结构清单：
//! - 网格求解器：`CaseSpec → Vec<Cell>`。列/行、媒体、边距、缝、填充序（行优先 /
//!   列优先）决定每格的 (col,row,x,y,w,h)；几何与填充序解耦，序只改**序列**；
//! - 用例账：`CaseLedger` 顺序追加 `VectorCase { spec, expected }`，维护链
//!   `chain_n = step(chain_{n-1}, digest_n)`。链对顺序敏感，对集合不敏感；
//! - 复跑：`replay()` 用同一 `place()` 重算每例摘要，逐例比对 expected，
//!   再从 `CHAIN_START` 重折链与账内链对齐。三类失败各自成码：
//!   用例参数非法（place 错误）、期望摘要不符（DigestMismatch）、链断（ChainBroken）；
//! - 夹具导入：`import()` 允许把外部期望摘要直接入账（离线基准集的形态），
//!   replay 是唯一的验收口——导入不意味着可信；
//! - 注入核：`injection_core` 五类坏账（重例、坏网格、坏期望、坏链、容量满）。
//!
//! 为什么「可复跑」要落到摘要 + 链两层：单例摘要只能证明这一例的几何没变；
//! 链还能证明**用例集合与顺序**没变。换序、删例、插例都会让链漂移，
//! 这正是编排用例集在回归里最容易被悄悄改动的地方。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段基址（0x43060 段，独占 0x20）。
pub const SEG_BASE: u32 = 0x43060;

/// 用例账容量上限：24 例覆盖列数/行数/媒体/序四维的等价类，再多就是重复。
pub const MAX_CASES: usize = 24;
/// 单例格数上限：64 格（8×8）已覆盖 N-up 与拼版常规上限。
pub const MAX_CELLS: u16 = 64;
/// 媒体边长上限（pt）：1e6 足够海报级幅面。
pub const MAX_MEDIA: f32 = 1.0e6;
/// 几何容差（pt）。
pub const GEOM_EPS: f32 = 1.0e-3;
/// 链起点盐（与摘要盐分离，避免「空链 == 空摘要」）。
pub const CHAIN_START: u64 = 0x4306_0000_C0DE_5EED;

const E_EMPTY: u16 = 1;
const E_FULL: u16 = 2;
const E_DUP: u16 = 3;
const E_GRID: u16 = 4;
const E_MEDIA: u16 = 5;
const E_MARGIN: u16 = 6;
const E_GUTTER: u16 = 7;
const E_NOROOM: u16 = 8;
const E_DIGEST: u16 = 9;
const E_CHAIN: u16 = 10;
const E_RANGE: u16 = 11;

/// 布局向量域错误十一型，`code()` 恒为 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VecError {
    Empty,
    Full,
    Duplicate,
    BadGrid,
    BadMedia,
    BadMargin,
    BadGutter,
    NoRoom,
    DigestMismatch,
    ChainBroken,
    Range,
}

impl VecError {
    pub fn code(self) -> u32 {
        let n = match self {
            VecError::Empty => E_EMPTY,
            VecError::Full => E_FULL,
            VecError::Duplicate => E_DUP,
            VecError::BadGrid => E_GRID,
            VecError::BadMedia => E_MEDIA,
            VecError::BadMargin => E_MARGIN,
            VecError::BadGutter => E_GUTTER,
            VecError::NoRoom => E_NOROOM,
            VecError::DigestMismatch => E_DIGEST,
            VecError::ChainBroken => E_CHAIN,
            VecError::Range => E_RANGE,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            VecError::Empty => "case ledger empty",
            VecError::Full => "case ledger full",
            VecError::Duplicate => "case already recorded",
            VecError::BadGrid => "column or row count zero",
            VecError::BadMedia => "media size non-finite or out of range",
            VecError::BadMargin => "margin non-finite or negative",
            VecError::BadGutter => "gutter non-finite or negative",
            VecError::NoRoom => "margins/gutters leave no printable cell",
            VecError::DigestMismatch => "case digest differs from expected",
            VecError::ChainBroken => "ledger chain does not recompute",
            VecError::Range => "argument outside accepted range",
        }
    }
}

/// 填充序：同一组格子，两种遍历序列。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillOrder {
    RowMajor,
    ColumnMajor,
}

impl FillOrder {
    pub fn code(self) -> u8 {
        match self {
            FillOrder::RowMajor => 0,
            FillOrder::ColumnMajor => 1,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FillOrder::RowMajor => "row-major",
            FillOrder::ColumnMajor => "column-major",
        }
    }
}

/// 一个排版格（pt，原点左上，y 向下）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub col: u8,
    pub row: u8,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Cell {
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn within(&self, media_w: f32, media_h: f32, eps: f32) -> bool {
        self.w >= 0.0
            && self.h >= 0.0
            && self.x >= -eps
            && self.y >= -eps
            && self.right() <= media_w + eps
            && self.bottom() <= media_h + eps
    }

    /// 严格相交：重叠宽高都大于 eps，贴边不算（缝为 0 时格子必然贴边）。
    pub fn intersects(&self, o: &Cell, eps: f32) -> bool {
        let ow = minf(self.right(), o.right()) - maxf(self.x, o.x);
        let oh = minf(self.bottom(), o.bottom()) - maxf(self.y, o.y);
        ow > eps && oh > eps
    }
}

fn minf(a: f32, b: f32) -> f32 {
    if a < b {
        a
    } else {
        b
    }
}

fn maxf(a: f32, b: f32) -> f32 {
    if a > b {
        a
    } else {
        b
    }
}

/// 一条用例的版面规格（Copy，便于账内按值保存与逐例重放）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaseSpec {
    pub cols: u8,
    pub rows: u8,
    pub media_w: f32,
    pub media_h: f32,
    pub margin: f32,
    pub gutter: f32,
    pub order: FillOrder,
}

impl CaseSpec {
    pub const fn new(cols: u8, rows: u8, media_w: f32, media_h: f32) -> CaseSpec {
        CaseSpec {
            cols,
            rows,
            media_w,
            media_h,
            margin: 0.0,
            gutter: 0.0,
            order: FillOrder::RowMajor,
        }
    }
}

/// 求解网格：校验 → 逐格定位 → 按填充序输出。几何与序解耦。
pub fn place(spec: &CaseSpec) -> Result<Vec<Cell>, VecError> {
    if spec.cols == 0 || spec.rows == 0 {
        return Err(VecError::BadGrid);
    }
    let count = spec.cols as u32 * spec.rows as u32;
    if count > MAX_CELLS as u32 {
        return Err(VecError::Range);
    }
    if !spec.media_w.is_finite() || !spec.media_h.is_finite() {
        return Err(VecError::BadMedia);
    }
    if spec.media_w <= 0.0 || spec.media_h <= 0.0 || spec.media_w > MAX_MEDIA || spec.media_h > MAX_MEDIA
    {
        return Err(VecError::BadMedia);
    }
    if !spec.margin.is_finite() || spec.margin < 0.0 {
        return Err(VecError::BadMargin);
    }
    if !spec.gutter.is_finite() || spec.gutter < 0.0 {
        return Err(VecError::BadGutter);
    }
    let usable_w = spec.media_w - 2.0 * spec.margin;
    let usable_h = spec.media_h - 2.0 * spec.margin;
    if usable_w <= 0.0 || usable_h <= 0.0 {
        return Err(VecError::NoRoom);
    }
    let cell_w = (usable_w - spec.gutter * (spec.cols as f32 - 1.0)) / spec.cols as f32;
    let cell_h = (usable_h - spec.gutter * (spec.rows as f32 - 1.0)) / spec.rows as f32;
    if !cell_w.is_finite() || !cell_h.is_finite() || cell_w <= 0.0 || cell_h <= 0.0 {
        return Err(VecError::NoRoom);
    }

    let mut cells = Vec::with_capacity(count as usize);
    let mut n = 0u32;
    while n < count {
        let (col, row) = match spec.order {
            FillOrder::RowMajor => (n % spec.cols as u32, n / spec.cols as u32),
            FillOrder::ColumnMajor => (n / spec.rows as u32, n % spec.rows as u32),
        };
        cells.push(Cell {
            col: col as u8,
            row: row as u8,
            x: spec.margin + col as f32 * (cell_w + spec.gutter),
            y: spec.margin + row as f32 * (cell_h + spec.gutter),
            w: cell_w,
            h: cell_h,
        });
        n += 1;
    }
    Ok(cells)
}

const FNV_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

fn fnv_u32(mut h: u64, v: u32) -> u64 {
    let mut k = 0u32;
    while k < 4 {
        h ^= ((v >> (k * 8)) & 0xFF) as u64;
        h = h.wrapping_mul(FNV_PRIME);
        k += 1;
    }
    h
}

fn fnv_f32(h: u64, v: f32) -> u64 {
    fnv_u32(h, v.to_bits())
}

/// 单例摘要：规格字段 + 填充序 + 每格 (col,row,x,y,w,h) 的比特序列。
pub fn case_digest(spec: &CaseSpec, cells: &[Cell]) -> u64 {
    let mut h = FNV_OFFSET;
    h = fnv_u32(h, spec.cols as u32);
    h = fnv_u32(h, spec.rows as u32);
    h = fnv_f32(h, spec.media_w);
    h = fnv_f32(h, spec.media_h);
    h = fnv_f32(h, spec.margin);
    h = fnv_f32(h, spec.gutter);
    h = fnv_u32(h, spec.order.code() as u32);
    h = fnv_u32(h, cells.len() as u32);
    let mut i = 0usize;
    while i < cells.len() {
        if let Some(c) = cells.get(i) {
            h = fnv_u32(h, c.col as u32);
            h = fnv_u32(h, c.row as u32);
            h = fnv_f32(h, c.x);
            h = fnv_f32(h, c.y);
            h = fnv_f32(h, c.w);
            h = fnv_f32(h, c.h);
        }
        i += 1;
    }
    h
}

/// 链步进：先旋转摘要再乘，保证换序必然改变链（摘要相同也不会抵消）。
pub fn chain_step(chain: u64, digest: u64) -> u64 {
    let mixed = chain ^ digest.rotate_left(17);
    let scaled = mixed.wrapping_mul(FNV_PRIME);
    scaled ^ (digest >> 29)
}

/// 账内一例：规格 + 期望摘要（record 写入自算值，import 写入外部值）。
#[derive(Clone, Debug, PartialEq)]
pub struct VectorCase {
    pub spec: CaseSpec,
    pub expected: u64,
}

/// 复跑结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplayReport {
    pub cases: usize,
    pub matched: usize,
    pub checksum: u64,
}

/// 布局向量账（顺序追加，链单调）。
pub struct CaseLedger {
    cases: Vec<VectorCase>,
    chain: u64,
    capacity: usize,
}

impl CaseLedger {
    pub fn new(capacity: usize) -> Result<CaseLedger, VecError> {
        if capacity == 0 || capacity > MAX_CASES {
            return Err(VecError::Range);
        }
        Ok(CaseLedger {
            cases: Vec::with_capacity(capacity),
            chain: CHAIN_START,
            capacity,
        })
    }

    pub fn len(&self) -> usize {
        self.cases.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }

    pub fn checksum(&self) -> u64 {
        self.chain
    }

    pub fn cases(&self) -> &[VectorCase] {
        &self.cases
    }

    fn append(&mut self, spec: CaseSpec, expected: u64, digest: u64) -> Result<(u64, u64), VecError> {
        let mut i = 0usize;
        while i < self.cases.len() {
            if let Some(c) = self.cases.get(i) {
                if c.spec == spec {
                    return Err(VecError::Duplicate);
                }
            }
            i += 1;
        }
        if self.cases.len() >= self.capacity {
            return Err(VecError::Full);
        }
        self.cases.push(VectorCase { spec, expected });
        self.chain = chain_step(self.chain, digest);
        Ok((digest, self.chain))
    }

    /// 记录一例：期望摘要由求解结果自算（正常入账路径）。
    pub fn record(&mut self, spec: CaseSpec) -> Result<(u64, u64), VecError> {
        let cells = place(&spec)?;
        let digest = case_digest(&spec, &cells);
        self.append(spec, digest, digest)
    }

    /// 导入一例：期望摘要来自外部夹具；replay 才判定它是否可复跑。
    pub fn import(&mut self, spec: CaseSpec, expected: u64) -> Result<(u64, u64), VecError> {
        let cells = place(&spec)?;
        let digest = case_digest(&spec, &cells);
        self.append(spec, expected, digest)
    }

    /// 复跑全账：重算每例摘要 + 重折链。任一例摘要不符或链不符都返回具体码。
    pub fn replay(&self) -> Result<ReplayReport, VecError> {
        if self.cases.is_empty() {
            return Err(VecError::Empty);
        }
        let mut chain = CHAIN_START;
        let mut matched = 0usize;
        let mut i = 0usize;
        while i < self.cases.len() {
            if let Some(c) = self.cases.get(i) {
                let cells = place(&c.spec)?;
                let digest = case_digest(&c.spec, &cells);
                if digest != c.expected {
                    return Err(VecError::DigestMismatch);
                }
                matched += 1;
                chain = chain_step(chain, digest);
            }
            i += 1;
        }
        if chain != self.chain {
            return Err(VecError::ChainBroken);
        }
        Ok(ReplayReport {
            cases: self.cases.len(),
            matched,
            checksum: chain,
        })
    }

    /// 用外部保存的校验和复验整账（回归夹具的形态）。
    pub fn verify_chain(&self, expected_chain: u64) -> Result<usize, VecError> {
        let report = self.replay()?;
        if report.checksum != expected_chain {
            return Err(VecError::ChainBroken);
        }
        Ok(report.matched)
    }
}

fn sample_specs() -> [CaseSpec; 6] {
    let mut a = CaseSpec::new(1, 1, 595.0, 842.0);
    a.margin = 0.0;
    a.gutter = 0.0;
    let mut b = CaseSpec::new(2, 2, 595.0, 842.0);
    b.margin = 20.0;
    b.gutter = 8.0;
    let mut c = CaseSpec::new(3, 2, 420.0, 595.0);
    c.margin = 10.0;
    c.gutter = 5.0;
    c.order = FillOrder::ColumnMajor;
    let mut d = CaseSpec::new(4, 3, 595.0, 842.0);
    d.margin = 12.0;
    d.gutter = 4.0;
    let mut e = CaseSpec::new(2, 1, 210.0, 297.0);
    e.margin = 5.0;
    e.gutter = 2.0;
    e.order = FillOrder::ColumnMajor;
    let mut f = CaseSpec::new(1, 2, 841.0, 1189.0);
    f.margin = 30.0;
    f.gutter = 0.0;
    [a, b, c, d, e, f]
}

fn ledger_with(capacity: usize) -> CaseLedger {
    match CaseLedger::new(capacity) {
        Ok(l) => l,
        // 容量参数由本模块常量约束，失败分支只可能是调用方笔误；
        // 仍构造同形账（空表 + 原容量），让后续判据按真实语义变红而非 panic。
        Err(_) => CaseLedger {
            cases: Vec::new(),
            chain: CHAIN_START,
            capacity,
        },
    }
}

fn sample_ledger() -> Result<CaseLedger, VecError> {
    let mut ledger = CaseLedger::new(8)?;
    let specs = sample_specs();
    let mut i = 0usize;
    while i < specs.len() {
        if let Some(sp) = specs.get(i) {
            let _ = ledger.record(*sp)?;
        }
        i += 1;
    }
    Ok(ledger)
}

/// 注入核：五类坏账必须分别落到 Duplicate / BadGrid / DigestMismatch / ChainBroken / Full。
pub fn injection_core() -> (usize, usize) {
    let total = 5usize;
    let mut caught = 0usize;
    let dot = CaseSpec::new(2, 2, 595.0, 842.0);

    let mut dup = match CaseLedger::new(4) {
        Ok(l) => l,
        Err(_) => return (0, total),
    };
    if dup.record(dot).is_ok() && dup.record(dot).err() == Some(VecError::Duplicate) {
        caught += 1;
    }

    let mut grid = match CaseLedger::new(4) {
        Ok(l) => l,
        Err(_) => return (caught, total),
    };
    let zero_cols = CaseSpec::new(0, 4, 595.0, 842.0);
    if grid.record(zero_cols).err() == Some(VecError::BadGrid) {
        caught += 1;
    }

    let mut tampered = match CaseLedger::new(4) {
        Ok(l) => l,
        Err(_) => return (caught, total),
    };
    let cells = match place(&dot) {
        Ok(c) => c,
        Err(_) => return (caught, total),
    };
    let real = case_digest(&dot, &cells);
    if tampered.import(dot, real ^ 1).is_ok()
        && tampered.replay().err() == Some(VecError::DigestMismatch)
    {
        caught += 1;
    }

    let broken = match sample_ledger() {
        Ok(l) => l,
        Err(_) => return (caught, total),
    };
    let chain = broken.checksum();
    if broken.verify_chain(chain ^ 1).err() == Some(VecError::ChainBroken) {
        caught += 1;
    }

    let mut full = match CaseLedger::new(2) {
        Ok(l) => l,
        Err(_) => return (caught, total),
    };
    let first = CaseSpec::new(1, 1, 595.0, 842.0);
    let second = CaseSpec::new(2, 1, 595.0, 842.0);
    let third = CaseSpec::new(3, 1, 595.0, 842.0);
    if full.record(first).is_ok()
        && full.record(second).is_ok()
        && full.record(third).err() == Some(VecError::Full)
    {
        caught += 1;
    }
    (caught, total)
}

/// 布局向量域自检（J1：用例可复跑 + 链顺序敏感 + 坏账必拒）。
pub fn run_unx50034_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50034");

    // J1-1 六例入账后可复跑：matched 全中，链与账内链一致。
    let (ok, det) = match sample_ledger() {
        Ok(l) => match l.replay() {
            Ok(r) => (
                r.cases == 6 && r.matched == 6 && r.checksum == l.checksum(),
                format!("cases={} matched={} cs={:#x}", r.cases, r.matched, r.checksum),
            ),
            Err(e) => (false, format!("replay={}", e.code())),
        },
        Err(e) => (false, format!("build={}", e.code())),
    };
    s.add_dyn("50034-J1-用例可复跑", ok, &det);

    // J1-2 复跑确定：两次 replay 的校验和与匹配数逐位一致。
    let twice = match sample_ledger() {
        Ok(l) => match (l.replay(), l.replay()) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        },
        Err(_) => false,
    };
    s.add("50034-J1-复跑确定", twice, if twice { "" } else { "两次复跑不一致" });

    // J1-3 链可复算：正确校验和放行、改一位即 ChainBroken。
    let chain_ok = match sample_ledger() {
        Ok(l) => {
            let good = l.verify_chain(l.checksum()).ok() == Some(6);
            let bad = l.verify_chain(l.checksum() ^ 1).err() == Some(VecError::ChainBroken);
            (good && bad, good, bad)
        }
        Err(_) => (false, false, false),
    };
    let det = format!("good={} broken={}", chain_ok.1, chain_ok.2);
    s.add_dyn("50034-J1-链可复算", chain_ok.0, &det);

    // J1-4 顺序敏感：同三例正序 / 倒序入账，校验和必不同。
    let a = CaseSpec::new(1, 1, 595.0, 842.0);
    let b = CaseSpec::new(2, 2, 595.0, 842.0);
    let c = CaseSpec::new(3, 2, 420.0, 595.0);
    let mut fwd = ledger_with(4);
    let mut rev = ledger_with(4);
    let _ = fwd.record(a);
    let _ = fwd.record(b);
    let _ = fwd.record(c);
    let _ = rev.record(c);
    let _ = rev.record(b);
    let _ = rev.record(a);
    let order_sensitive = fwd.checksum() != rev.checksum() && fwd.len() == rev.len();
    let det = format!("fwd={:#x} rev={:#x}", fwd.checksum(), rev.checksum());
    s.add_dyn("50034-J1-顺序敏感", order_sensitive, &det);

    // J1-5 重复用例拒。
    let mut dup = ledger_with(4);
    let first = dup.record(a);
    let again = dup.record(a).err() == Some(VecError::Duplicate);
    let ok = first.is_ok() && again && dup.len() == 1;
    s.add("50034-J1-重复用例拒", ok, if ok { "" } else { "重例未拒" });

    // J1-6 坏期望检出：import 错误摘要后 replay 必 DigestMismatch。
    let tamper = match CaseLedger::new(4) {
        Ok(mut l) => match place(&a) {
            Ok(cells) => {
                let real = case_digest(&a, &cells);
                let imported = l.import(a, real ^ 1).is_ok();
                (imported && l.replay().err() == Some(VecError::DigestMismatch), imported)
            }
            Err(_) => (false, false),
        },
        Err(_) => (false, false),
    };
    let det = format!("imported={}", tamper.1);
    s.add_dyn("50034-J1-坏期望检出", tamper.0, &det);

    // J1-7 空账拒。
    let empty = match CaseLedger::new(2) {
        Ok(l) => l.replay().err() == Some(VecError::Empty),
        Err(_) => false,
    };
    s.add("50034-J1-空账拒", empty, if empty { "" } else { "空账未拒" });

    // J1-8 容量满拒：容量 2 第 3 例 Full。
    let mut cap = ledger_with(2);
    let one = cap.record(a).is_ok();
    let two = cap.record(b).is_ok();
    let three = cap.record(c).err() == Some(VecError::Full);
    let ok = one && two && three;
    let det = format!("full={}", three);
    s.add_dyn("50034-J1-容量满拒", ok, &det);

    // J1-9 格数守恒与在纸内：4×3 共 12 格，逐格在媒体内。
    let mut grid = CaseSpec::new(4, 3, 595.0, 842.0);
    grid.margin = 12.0;
    grid.gutter = 4.0;
    let cells = match place(&grid) {
        Ok(v) => v,
        // 失败时空表入判据：`cells.len() == 12` 自然变红，不掩盖问题。
        Err(_) => Vec::new(),
    };
    let mut inside = 0usize;
    let mut i = 0usize;
    while i < cells.len() {
        if let Some(c) = cells.get(i) {
            if c.within(595.0, 842.0, GEOM_EPS) {
                inside += 1;
            }
        }
        i += 1;
    }
    let ok = cells.len() == 12 && inside == 12;
    let det = format!("cells={} inside={}", cells.len(), inside);
    s.add_dyn("50034-J1-格数守恒", ok, &det);

    // J1-10 格互不重叠（12 格 66 对）。
    let mut hits = 0usize;
    let mut x = 0usize;
    while x < cells.len() {
        if let Some(ca) = cells.get(x) {
            let mut y = x + 1;
            while y < cells.len() {
                if let Some(cb) = cells.get(y) {
                    if ca.intersects(cb, GEOM_EPS) {
                        hits += 1;
                    }
                }
                y += 1;
            }
        }
        x += 1;
    }
    let ok = hits == 0;
    let det = format!("overlap_pairs={}", hits);
    s.add_dyn("50034-J1-格不重叠", ok, &det);

    // J1-11 行序/列序同集异序：格集合相同、序列不同（摘要必不同）。
    let mut rm = grid;
    rm.order = FillOrder::RowMajor;
    let mut cm = grid;
    cm.order = FillOrder::ColumnMajor;
    let set_ok = match (place(&rm), place(&cm)) {
        (Ok(rc), Ok(cc)) => {
            if rc.len() != cc.len() {
                false
            } else {
                let mut matched = 0usize;
                let mut p = 0usize;
                while p < rc.len() {
                    if let Some(ra) = rc.get(p) {
                        let mut q = 0usize;
                        while q < cc.len() {
                            if let Some(cb) = cc.get(q) {
                                if *ra == *cb {
                                    matched += 1;
                                    break;
                                }
                            }
                            q += 1;
                        }
                    }
                    p += 1;
                }
                matched == rc.len()
            }
        }
        _ => false,
    };
    let digest_diff = match (place(&rm), place(&cm)) {
        (Ok(rc), Ok(cc)) => case_digest(&rm, &rc) != case_digest(&cm, &cc),
        _ => false,
    };
    let ok = set_ok && digest_diff;
    let det = format!("set_equal={} digest_diff={}", set_ok, digest_diff);
    s.add_dyn("50034-J1-行序列序", ok, &det);

    // J1-12 非法必拒：零列/零行、坏媒体、坏缝、无空间、超格数。
    let mut bad = 0usize;
    let mut zero = grid;
    zero.cols = 0;
    if place(&zero).err() == Some(VecError::BadGrid) {
        bad += 1;
    }
    zero = grid;
    zero.rows = 0;
    if place(&zero).err() == Some(VecError::BadGrid) {
        bad += 1;
    }
    let mut media = grid;
    media.media_w = f32::NAN;
    if place(&media).err() == Some(VecError::BadMedia) {
        bad += 1;
    }
    let mut gutter = grid;
    gutter.gutter = f32::NAN;
    if place(&gutter).err() == Some(VecError::BadGutter) {
        bad += 1;
    }
    let mut room = grid;
    room.margin = 400.0;
    if place(&room).err() == Some(VecError::NoRoom) {
        bad += 1;
    }
    let mut many = grid;
    many.cols = 16;
    many.rows = 8;
    if place(&many).err() == Some(VecError::Range) {
        bad += 1;
    }
    let ok = bad == 6;
    let det = format!("rejected={}/6", bad);
    s.add_dyn("50034-J1-非法必拒", ok, &det);

    // J1-13 摘要幂等：同一 spec 两次求解 + 两次摘要逐位一致。
    let idem = match (place(&grid), place(&grid)) {
        (Ok(c1), Ok(c2)) => {
            c1 == c2 && case_digest(&grid, &c1) == case_digest(&grid, &c2)
        }
        _ => false,
    };
    s.add("50034-J1-摘要幂等", idem, if idem { "" } else { "摘要不幂等" });

    // J1-14 五类注入全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50034-J1-注入五类", ok, &det);

    // J1-15 错误码闭集 0x43060+1..=0x43060+11。
    let all = [
        VecError::Empty,
        VecError::Full,
        VecError::Duplicate,
        VecError::BadGrid,
        VecError::BadMedia,
        VecError::BadMargin,
        VecError::BadGutter,
        VecError::NoRoom,
        VecError::DigestMismatch,
        VecError::ChainBroken,
        VecError::Range,
    ];
    let mut closed = 0usize;
    let mut k = 0usize;
    while k < all.len() {
        if let Some(e) = all.get(k) {
            let code = e.code();
            if code > SEG_BASE && code <= SEG_BASE + 11 && !e.message().is_empty() {
                closed += 1;
            }
        }
        k += 1;
    }
    let ok = closed == all.len();
    let det = format!("closed={}/{}", closed, all.len());
    s.add_dyn("50034-J1-码集闭包", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50034_checks_green() {
        let set = run_unx50034_checks();
        assert!(set.all_passed(), "UNX-F50034 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50034_replay_matches_checksum() {
        let l = match sample_ledger() {
            Ok(v) => v,
            Err(e) => panic!("build {}", e.code()),
        };
        let r = match l.replay() {
            Ok(v) => v,
            Err(_) => panic!("replay"),
        };
        assert_eq!(r.cases, 6);
        assert_eq!(r.matched, 6);
        assert_eq!(r.checksum, l.checksum());
    }

    #[test]
    fn unx50034_chain_order_sensitive() {
        let a = CaseSpec::new(1, 1, 595.0, 842.0);
        let b = CaseSpec::new(2, 2, 595.0, 842.0);
        let mut x = match CaseLedger::new(4) {
            Ok(v) => v,
            Err(_) => panic!("ledger"),
        };
        let mut y = match CaseLedger::new(4) {
            Ok(v) => v,
            Err(_) => panic!("ledger"),
        };
        let _ = x.record(a);
        let _ = x.record(b);
        let _ = y.record(b);
        let _ = y.record(a);
        assert!(x.checksum() != y.checksum());
    }
}
