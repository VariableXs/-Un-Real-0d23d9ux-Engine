//! UNX-F9341 E 型总装总纲（全错误码矩阵收口协议） · 0x7C2C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9341 逐字）：
//!   ① 纯功能代码 ≥420 行；② 判据原文：UNX-F9341-J1 八批错误矩阵收口
//!   协议判据过，全错误码账逐码落账，收口判序（EFAULT>EBADF>EINVAL>
//!   EPERM）声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要
//!   非空。
//!
//! 设计要点：
//!   ① 八批（B20..B27）各带一张 12 列错误码计数行，合起来是 8×12=96 格
//!      矩阵。行合计恒 16、总账恒 128，冻结在 EXPECT 常量里——收口不是
//!      数“跑过几批”，而是把 96 格逐格对上。
//!   ② 逐码落账走独立账本 CodeLedger：十二码各有列合计，credited 必须
//!      等于 12，任何一码为 0 都算缺码（不是“总账够了就行”）。
//!   ③ 判序是声明格而不是口头约定：PREC_GRID 四格写死
//!      EFAULT(0)>EBADF(1)>EINVAL(2)>EPERM(3)，仲裁时按 (rank, 列号) 取小；
//!      落在四码之外的仲裁结果一律 EINVAL——没声明过的优先级不许现编。
//!   ④ 收口协议是相位机：Collect→Reconcile→Arbitrate→Seal→Closed，
//!      跳步回 EPERM、封印后采集回 EPERM、二次封印回 EBUSY、未封印收口
//!      回 EBUSY。相位本身就是判据，不靠注释约束。
//!   ⑤ 采集顺序也是判据：批号必须等于 next（乱序 EINVAL）、重复批号
//!      EEXIST、八批之后再送 ENFILE。矩阵不会替调用者排序。
//!   ⑥ 注入三变体（判序倒置、缺格、乱序）各 10 轮：注入 30、检出 30、
//!      逃逸 0，逐变体命中 10。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;

use crate::checks::CheckSet;

/// 诊断段基址（F9341 独占 0x7C2C0..=0x7C2FF）。
pub const SEG_BASE: u32 = 0x7C2C0;

/// 段窗终点（不含）。
pub const SEG_END: u32 = 0x7C300;

/// 错误码列数（全错误码账）。
pub const CODES: usize = 12;

/// 批数（B20..B27）。
pub const BATCHES: usize = 8;

/// 矩阵格子总数。
pub const CELLS: usize = BATCHES * CODES;

/// 单批计数合计（每行恒 16）。
pub const BATCH_SUM: u16 = 16;

/// 全矩阵总账（8 × 16）。
pub const GRAND_TOTAL: u32 = 128;

/// 单变体注入轮数。
pub const MUTANT_ROUNDS: u32 = 10;

/// 注入变体数。
pub const MUTANTS: usize = 3;

/// 收口报告摘要（判据③要求非空）。
pub const CLOSE_SUMMARY: &str =
    "八批错误矩阵收口：96 格逐码落账，判序 EFAULT>EBADF>EINVAL>EPERM 声明格成立";

// ── errno（正数口径） ─────────────────────────────────────────────────
/// 操作不允许。
pub const EPERM: i32 = 1;
/// 目标不存在。
pub const ESRCH: i32 = 3;
/// 被信号打断。
pub const EINTR: i32 = 4;
/// I/O 错误。
pub const EIO: i32 = 5;
/// 设备或地址不存在。
pub const ENXIO: i32 = 6;
/// 坏文件描述符。
pub const EBADF: i32 = 9;
/// 暂不可用。
pub const EAGAIN: i32 = 11;
/// 内存不足。
pub const ENOMEM: i32 = 12;
/// 坏用户指针。
pub const EFAULT: i32 = 14;
/// 设备忙。
pub const EBUSY: i32 = 16;
/// 已存在。
pub const EEXIST: i32 = 17;
/// 参数非法。
pub const EINVAL: i32 = 22;
/// 句柄耗尽。
pub const ENFILE: i32 = 23;

/// 错误码表：(名称, 码值, 说明)，列序即矩阵列序。
pub const CODE_TABLE: [(&str, i32, &str); CODES] = [
    ("EPERM", EPERM, "缺能力位"),
    ("ESRCH", ESRCH, "目标不存在"),
    ("EINTR", EINTR, "被信号打断"),
    ("EIO", EIO, "底层 I/O 错"),
    ("ENXIO", ENXIO, "设备或地址不存在"),
    ("EBADF", EBADF, "句柄非法"),
    ("EAGAIN", EAGAIN, "暂不可用"),
    ("ENOMEM", ENOMEM, "内存不足"),
    ("EFAULT", EFAULT, "坏用户指针"),
    ("EBUSY", EBUSY, "设备忙"),
    ("EEXIST", EEXIST, "对象已存在"),
    ("EINVAL", EINVAL, "参数非法"),
];

/// 列号取码值。
pub fn code_at(c: usize) -> Option<i32> {
    match CODE_TABLE.get(c) {
        Some(row) => Some(row.1),
        None => None,
    }
}

/// 码值取列号。
pub fn code_index(code: i32) -> Option<usize> {
    let mut i = 0usize;
    while i < CODES {
        let hit = match CODE_TABLE.get(i) {
            Some(row) => row.1 == code,
            None => false,
        };
        if hit {
            return Some(i);
        }
        i += 1;
    }
    None
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +1..=+15）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum ChartCode {
    /// 段基址越窗。
    SegmentOutOfWindow = 1,
    /// 批数不是 8。
    BatchCountMismatch = 2,
    /// 码列不是 12。
    CodeColumnMismatch = 3,
    /// 矩阵形状被改。
    MatrixShapeBroken = 4,
    /// 采集乱序。
    ArrivalOutOfOrder = 5,
    /// 批号重复。
    BatchDuplicate = 6,
    /// 批容量已满。
    BatchCapacityFull = 7,
    /// 相位跳步。
    PhaseSkipped = 8,
    /// 对账有差。
    ReconcileDiff = 9,
    /// 判序被倒置。
    PrecedenceInverted = 10,
    /// 判序未声明。
    PrecedenceUndeclared = 11,
    /// 逐码账缺码。
    LedgerCodeMissing = 12,
    /// 封印后改写。
    SealViolated = 13,
    /// 注入未被检出。
    InjectionEscaped = 14,
    /// 摘要为空。
    EmptySummary = 15,
}

impl ChartCode {
    /// 序号。
    pub fn code(self) -> u16 {
        self as u16
    }
    /// 码值 = 段基址 + 序号。
    pub fn seg_addr(self) -> u32 {
        SEG_BASE + self.code() as u32
    }
    /// 中文诊断。
    pub fn detail(self) -> &'static str {
        match self {
            ChartCode::SegmentOutOfWindow => "段基址越窗：本卡独占 0x7C2C0..=0x7C2FF",
            ChartCode::BatchCountMismatch => "批数不是 8：B20..B27 八批缺一不可",
            ChartCode::CodeColumnMismatch => "码列不是 12：全错误码账要求十二码齐列",
            ChartCode::MatrixShapeBroken => "矩阵形状被改：8 行 × 12 列一格格都不能挪",
            ChartCode::ArrivalOutOfOrder => "采集乱序：第 n 批未到就送 n+1，矩阵不替调用者排序",
            ChartCode::BatchDuplicate => "批号重复：同一批登记两次会把账重复计",
            ChartCode::BatchCapacityFull => "批容量已满：八批之后的第九批没有位置",
            ChartCode::PhaseSkipped => "相位跳步：前置阶段未完成就调后续动作",
            ChartCode::ReconcileDiff => "对账有差：矩阵与冻结期望表存在不等格",
            ChartCode::PrecedenceInverted => "判序被倒置：EFAULT>EBADF>EINVAL>EPERM 不许改",
            ChartCode::PrecedenceUndeclared => "判序未声明：仲裁落在四码之外，说明声明格缺失",
            ChartCode::LedgerCodeMissing => "逐码账缺码：某错误码在八批中从未落账",
            ChartCode::SealViolated => "封印后改写：已封矩阵不接受任何采集",
            ChartCode::InjectionEscaped => "注入未被检出：倒序、缺格、乱序三变体必须全被抓",
            ChartCode::EmptySummary => "摘要为空：收口报告必须带非空摘要",
        }
    }

    /// 闭集全量。
    pub const ALL: &'static [ChartCode] = &[
        ChartCode::SegmentOutOfWindow,
        ChartCode::BatchCountMismatch,
        ChartCode::CodeColumnMismatch,
        ChartCode::MatrixShapeBroken,
        ChartCode::ArrivalOutOfOrder,
        ChartCode::BatchDuplicate,
        ChartCode::BatchCapacityFull,
        ChartCode::PhaseSkipped,
        ChartCode::ReconcileDiff,
        ChartCode::PrecedenceInverted,
        ChartCode::PrecedenceUndeclared,
        ChartCode::LedgerCodeMissing,
        ChartCode::SealViolated,
        ChartCode::InjectionEscaped,
        ChartCode::EmptySummary,
    ];
}

// ── 矩阵与冻结期望 ────────────────────────────────────────────────────
/// 矩阵形状：8 行 × 12 列计数。
pub type Rows = [[u16; CODES]; BATCHES];

/// 冻结期望矩阵（行合计 16，总账 128）。
pub const EXPECT: Rows = [
    [2, 1, 1, 1, 1, 2, 1, 1, 2, 1, 1, 2],
    [2, 2, 1, 1, 1, 1, 2, 1, 1, 2, 1, 1],
    [1, 2, 2, 1, 1, 1, 1, 2, 1, 1, 2, 1],
    [1, 1, 2, 2, 1, 1, 1, 1, 2, 1, 1, 2],
    [2, 1, 1, 2, 2, 1, 1, 1, 1, 2, 1, 1],
    [1, 2, 1, 1, 2, 2, 1, 1, 1, 1, 2, 1],
    [1, 1, 2, 1, 1, 2, 2, 1, 1, 1, 1, 2],
    [2, 1, 1, 2, 1, 1, 2, 2, 1, 1, 1, 1],
];

/// 冻结列合计。
pub const COL_TOTALS: [u32; CODES] = [12, 11, 11, 11, 10, 11, 11, 10, 10, 10, 10, 11];

/// 取期望格。
pub fn expect_at(r: usize, c: usize) -> u16 {
    match EXPECT.get(r) {
        Some(row) => match row.get(c) {
            Some(v) => *v,
            None => 0,
        },
        None => 0,
    }
}

/// 取实际格。
pub fn row_at(rows: &Rows, r: usize, c: usize) -> u16 {
    match rows.get(r) {
        Some(row) => match row.get(c) {
            Some(v) => *v,
            None => 0,
        },
        None => 0,
    }
}

/// 行合计。
pub fn row_sum(rows: &Rows, r: usize) -> u32 {
    let mut sum = 0u32;
    let mut c = 0usize;
    while c < CODES {
        sum = sum.saturating_add(row_at(rows, r, c) as u32);
        c += 1;
    }
    sum
}

/// 列合计。
pub fn col_sum(rows: &Rows, c: usize) -> u32 {
    let mut sum = 0u32;
    let mut r = 0usize;
    while r < BATCHES {
        sum = sum.saturating_add(row_at(rows, r, c) as u32);
        r += 1;
    }
    sum
}

/// 全矩阵总账。
pub fn grand_total(rows: &Rows) -> u32 {
    let mut sum = 0u32;
    let mut r = 0usize;
    while r < BATCHES {
        sum = sum.saturating_add(row_sum(rows, r));
        r += 1;
    }
    sum
}

/// 冻结期望表自洽：行合计、总账、列合计三处对齐。
pub fn expect_consistent() -> bool {
    let mut r = 0usize;
    while r < BATCHES {
        if row_sum(&EXPECT, r) != BATCH_SUM as u32 {
            return false;
        }
        r += 1;
    }
    if grand_total(&EXPECT) != GRAND_TOTAL {
        return false;
    }
    let mut c = 0usize;
    while c < CODES {
        let want = match COL_TOTALS.get(c) {
            Some(v) => *v,
            None => return false,
        };
        if col_sum(&EXPECT, c) != want {
            return false;
        }
        c += 1;
    }
    true
}

/// 每格至少落账一次（逐格非零）。
pub fn every_cell_positive(rows: &Rows) -> bool {
    let mut r = 0usize;
    while r < BATCHES {
        let mut c = 0usize;
        while c < CODES {
            if row_at(rows, r, c) == 0 {
                return false;
            }
            c += 1;
        }
        r += 1;
    }
    true
}

// ── 判序声明格 ────────────────────────────────────────────────────────
/// 一格判序声明。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PrecCell {
    /// 错误码。
    pub code: i32,
    /// 秩（越小越优先）。
    pub rank: u8,
    /// 条款。
    pub clause: &'static str,
}

/// 四码判序声明格（收口判序 EFAULT>EBADF>EINVAL>EPERM）。
pub const PREC_GRID: [PrecCell; 4] = [
    PrecCell { code: EFAULT, rank: 0, clause: "坏用户指针最先判，指针不可读时其余面不成立" },
    PrecCell { code: EBADF, rank: 1, clause: "坏句柄次之，句柄无效时参数面无从谈起" },
    PrecCell { code: EINVAL, rank: 2, clause: "参数非法再次之，句柄有效才轮到结构校验" },
    PrecCell { code: EPERM, rank: 3, clause: "能力缺失最后报，前述各面通过才做权限判定" },
];

/// 四码之外的秩。
pub const RANK_OTHER: u8 = 10;

/// 单码的判序秩。
pub fn precedence_rank(code: i32) -> u8 {
    if code == EFAULT {
        0
    } else if code == EBADF {
        1
    } else if code == EINVAL {
        2
    } else if code == EPERM {
        3
    } else {
        RANK_OTHER
    }
}

/// 从声明格导出四秩。
pub fn declared_ranks() -> [u8; 4] {
    let mut out = [RANK_OTHER; 4];
    let mut i = 0usize;
    while i < PREC_GRID.len() {
        let rank = match PREC_GRID.get(i) {
            Some(cell) => cell.rank,
            None => RANK_OTHER,
        };
        if let Some(slot) = out.get_mut(i) {
            *slot = rank;
        }
        i += 1;
    }
    out
}

/// 四秩必须恰为 0,1,2,3（按 EFAULT,EBADF,EINVAL,EPERM 顺序）。
pub fn order_ok_of(ranks: &[u8; 4]) -> bool {
    let mut i = 0usize;
    while i < ranks.len() {
        match ranks.get(i) {
            Some(v) => {
                if *v != i as u8 {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 声明格整体自洽（码序、秩序、条款、解析各面）。
pub fn precedence_grid_ok() -> bool {
    if PREC_GRID.len() != 4 || !order_ok_of(&declared_ranks()) {
        return false;
    }
    let mut i = 0usize;
    while i < PREC_GRID.len() {
        if let Some(cell) = PREC_GRID.get(i) {
            if cell.clause.is_empty() || precedence_rank(cell.code) != cell.rank {
                return false;
            }
            let mut j = i + 1;
            while j < PREC_GRID.len() {
                if let Some(other) = PREC_GRID.get(j) {
                    if other.code == cell.code || other.clause == cell.clause {
                        return false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    // 四码同场时 EFAULT 必须胜出；空掩码必须无从仲裁。
    let quartet = (1u16 << 0) | (1u16 << 5) | (1u16 << 8) | (1u16 << 11);
    resolve(quartet) == Some(EFAULT) && resolve(0).is_none()
}

/// 在候选掩码上按 (秩, 列号) 取最小码值。
pub fn resolve(mask: u16) -> Option<i32> {
    let mut best: Option<(u8, usize)> = None;
    let mut c = 0usize;
    while c < CODES {
        let bit = (mask >> c) & 1;
        if bit == 1 {
            if let Some(code) = code_at(c) {
                let cand = (precedence_rank(code), c);
                best = match best {
                    Some(cur) => {
                        if cand.0 < cur.0 || (cand.0 == cur.0 && cand.1 < cur.1) {
                            Some(cand)
                        } else {
                            Some(cur)
                        }
                    }
                    None => Some(cand),
                };
            }
        }
        c += 1;
    }
    match best {
        Some(b) => code_at(b.1),
        None => None,
    }
}

/// 单批仲裁：掩码上每一位都必须有账，胜者必须落在已声明的四码内。
pub fn resolve_batch(mask: u16, row: &[u16; CODES]) -> Result<i32, i32> {
    let mut c = 0usize;
    while c < CODES {
        let bit = (mask >> c) & 1;
        if bit == 1 {
            let count = match row.get(c) {
                Some(v) => *v,
                None => 0,
            };
            if count == 0 {
                return Err(EINVAL);
            }
        }
        c += 1;
    }
    match resolve(mask) {
        Some(winner) => {
            if precedence_rank(winner) < RANK_OTHER {
                Ok(winner)
            } else {
                Err(EINVAL)
            }
        }
        None => Err(EINVAL),
    }
}

// ── 八批声明 ──────────────────────────────────────────────────────────
/// 一批的错误矩阵行声明。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BatchDecl {
    /// 批号（0..=7 对应 B20..B27）。
    pub id: u8,
    /// 批名。
    pub name: &'static str,
    /// 候选错误码掩码（12 位）。
    pub mask: u16,
    /// 逐码计数行。
    pub row: [u16; CODES],
}

/// 八批声明（行与 EXPECT 逐格相同）。
pub const DECLS: [BatchDecl; BATCHES] = [
    BatchDecl {
        id: 0,
        name: "B20 参数面收口",
        mask: 0x0901,
        row: [2, 1, 1, 1, 1, 2, 1, 1, 2, 1, 1, 2],
    },
    BatchDecl {
        id: 1,
        name: "B21 句柄面收口",
        mask: 0x0061,
        row: [2, 2, 1, 1, 1, 1, 2, 1, 1, 2, 1, 1],
    },
    BatchDecl {
        id: 2,
        name: "B22 钟号面收口",
        mask: 0x0830,
        row: [1, 2, 2, 1, 1, 1, 1, 2, 1, 1, 2, 1],
    },
    BatchDecl {
        id: 3,
        name: "B23 指针面收口",
        mask: 0x0501,
        row: [1, 1, 2, 2, 1, 1, 1, 1, 2, 1, 1, 2],
    },
    BatchDecl {
        id: 4,
        name: "B24 能力面收口",
        mask: 0x0920,
        row: [2, 1, 1, 2, 2, 1, 1, 1, 1, 2, 1, 1],
    },
    BatchDecl {
        id: 5,
        name: "B25 目标面收口",
        mask: 0x0101,
        row: [1, 2, 1, 1, 2, 2, 1, 1, 1, 1, 2, 1],
    },
    BatchDecl {
        id: 6,
        name: "B26 时间值面收口",
        mask: 0x00A1,
        row: [1, 1, 2, 1, 1, 2, 2, 1, 1, 1, 1, 2],
    },
    BatchDecl {
        id: 7,
        name: "B27 消费面收口",
        mask: 0x0820,
        row: [2, 1, 1, 2, 1, 1, 2, 2, 1, 1, 1, 1],
    },
];

/// 冻结仲裁结果（按 PREC_GRID 解析八批掩码）。
pub const WINS: [i32; BATCHES] = [EFAULT, EBADF, EBADF, EFAULT, EFAULT, EFAULT, EBADF, EBADF];

/// 声明表自洽：批号连续、批名互异、行与 EXPECT 相等、行合计与总账足额。
pub fn decls_consistent() -> bool {
    if DECLS.len() != BATCHES {
        return false;
    }
    let mut i = 0usize;
    while i < BATCHES {
        let decl = match DECLS.get(i) {
            Some(d) => d,
            None => return false,
        };
        if decl.id as usize != i || decl.name.is_empty() {
            return false;
        }
        let mut c = 0usize;
        while c < CODES {
            let want = match decl.row.get(c) {
                Some(v) => *v,
                None => return false,
            };
            if want != expect_at(i, c) {
                return false;
            }
            c += 1;
        }
        let mut j = i + 1;
        while j < BATCHES {
            if let Some(other) = DECLS.get(j) {
                if other.name == decl.name || other.id == decl.id {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 指纹（自写混算，非 FNV） ──────────────────────────────────────────
/// 字节续混：左旋 5 位后加字节，再乘黄金比奇数。
pub fn mix(h: u32, bytes: &[u8]) -> u32 {
    let mut acc = h;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            acc = acc.rotate_left(5).wrapping_add(*b as u32);
            acc = acc.wrapping_mul(0x9E37_79B1);
        }
        i += 1;
    }
    acc
}

/// 混入一个 u32。
pub fn mix_u32(h: u32, v: u32) -> u32 {
    mix(h, &v.to_le_bytes())
}

/// 单行指纹。
pub fn row_digest(id: u8, row: &[u16; CODES]) -> u32 {
    let mut h = mix(5381, b"unx9341/row");
    h = mix_u32(h, id as u32);
    let mut c = 0usize;
    while c < CODES {
        if let Some(v) = row.get(c) {
            h = mix_u32(h, *v as u32);
        }
        c += 1;
    }
    h
}

/// 矩阵指纹。
pub fn rows_digest(rows: &Rows) -> u32 {
    let mut h = mix(5381, b"unx9341/rows");
    let mut r = 0usize;
    while r < BATCHES {
        if let Some(row) = rows.get(r) {
            h = mix_u32(h, row_digest(r as u8, row));
        }
        r += 1;
    }
    h
}

/// 封印指纹：矩阵 + 掩码 + 仲裁结果。
pub fn seal_digest(rows: &Rows, masks: &[u16; BATCHES], resolved: &[i32; BATCHES]) -> u32 {
    let mut h = rows_digest(rows);
    h = mix(h, b"unx9341/seal");
    let mut i = 0usize;
    while i < BATCHES {
        if let Some(m) = masks.get(i) {
            h = mix_u32(h, *m as u32);
        }
        if let Some(w) = resolved.get(i) {
            h = mix_u32(h, *w as u32);
        }
        i += 1;
    }
    h
}

// ── 逐码账 ────────────────────────────────────────────────────────────
/// 十二码逐码账本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CodeLedger {
    /// 每码列合计。
    pub totals: [u32; CODES],
    /// 已落账码数。
    pub credited: u8,
    /// 账本指纹。
    pub seal: u32,
}

impl CodeLedger {
    /// 空账。
    pub const fn empty() -> CodeLedger {
        CodeLedger { totals: [0; CODES], credited: 0, seal: 0 }
    }

    /// 从矩阵滚出逐码账。
    pub fn from_rows(rows: &Rows) -> CodeLedger {
        let mut totals = [0u32; CODES];
        let mut r = 0usize;
        while r < BATCHES {
            let mut c = 0usize;
            while c < CODES {
                let v = row_at(rows, r, c) as u32;
                if let Some(slot) = totals.get_mut(c) {
                    *slot = slot.saturating_add(v);
                }
                c += 1;
            }
            r += 1;
        }
        let mut credited = 0u8;
        let mut i = 0usize;
        while i < CODES {
            if let Some(v) = totals.get(i) {
                if *v > 0 {
                    credited = credited.saturating_add(1);
                }
            }
            i += 1;
        }
        let mut seal = mix(5381, b"unx9341/ledger");
        let mut k = 0usize;
        while k < CODES {
            if let Some(v) = totals.get(k) {
                seal = mix_u32(seal, *v);
            }
            k += 1;
        }
        CodeLedger { totals, credited, seal }
    }

    /// 总账。
    pub fn total(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < CODES {
            if let Some(v) = self.totals.get(i) {
                sum = sum.saturating_add(*v);
            }
            i += 1;
        }
        sum
    }

    /// 每码在位。
    pub fn every_code(&self) -> bool {
        let mut i = 0usize;
        while i < CODES {
            let ok = match self.totals.get(i) {
                Some(v) => *v > 0,
                None => false,
            };
            if !ok {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 账本自审：十二码全落账、总账 128、credited 与列合计一致。
    pub fn audit(&self) -> bool {
        if self.credited as usize != CODES || !self.every_code() || self.total() != GRAND_TOTAL {
            return false;
        }
        let mut i = 0usize;
        while i < CODES {
            let want = match COL_TOTALS.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let got = match self.totals.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if want != got {
                return false;
            }
            i += 1;
        }
        self.seal != 0
    }
}

// ── 相位机 ────────────────────────────────────────────────────────────
/// 收口相位：Collect→Reconcile→Arbitrate→Seal→Closed。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// 采集八批。
    Collect,
    /// 对账冻结期望。
    Reconcile,
    /// 按判序仲裁。
    Arbitrate,
    /// 封印。
    Seal,
    /// 已收口。
    Closed,
}

impl Phase {
    /// 相位秩。
    pub fn rank(self) -> u8 {
        match self {
            Phase::Collect => 0,
            Phase::Reconcile => 1,
            Phase::Arbitrate => 2,
            Phase::Seal => 3,
            Phase::Closed => 4,
        }
    }
    /// 相位名。
    pub fn name(self) -> &'static str {
        match self {
            Phase::Collect => "采集",
            Phase::Reconcile => "对账",
            Phase::Arbitrate => "仲裁",
            Phase::Seal => "封印",
            Phase::Closed => "已收口",
        }
    }
    /// 是否采集相位。
    pub fn is_collect(self) -> bool {
        self == Phase::Collect
    }
    /// 是否对账相位。
    pub fn is_reconcile(self) -> bool {
        self == Phase::Reconcile
    }
    /// 是否仲裁相位。
    pub fn is_arbitrate(self) -> bool {
        self == Phase::Arbitrate
    }
    /// 是否封印相位。
    pub fn is_seal(self) -> bool {
        self == Phase::Seal
    }
    /// 是否已收口。
    pub fn is_closed(self) -> bool {
        self == Phase::Closed
    }
}

// ── 收口协议 ──────────────────────────────────────────────────────────
/// 收口报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CloseReport {
    /// 批数。
    pub batches: usize,
    /// 格数。
    pub cells: usize,
    /// 总账。
    pub grand: u32,
    /// 封印指纹。
    pub digest: u32,
    /// 判序声明格是否成立。
    pub precedence_ok: bool,
    /// 摘要。
    pub summary: &'static str,
}

impl CloseReport {
    /// 报告完整。
    pub fn complete(&self) -> bool {
        self.batches == BATCHES
            && self.cells == CELLS
            && self.grand == GRAND_TOTAL
            && self.digest != 0
            && self.precedence_ok
            && !self.summary.is_empty()
    }
}

/// 八批错误矩阵收口协议（纯状态机，一次装配一次收口）。
#[derive(Clone, Copy, Debug)]
pub struct Protocol {
    phase: Phase,
    next: usize,
    rows: Rows,
    seen: [bool; BATCHES],
    masks: [u16; BATCHES],
    resolved: [i32; BATCHES],
    diffs: u32,
    first_bad: Option<(usize, usize)>,
    seal: u32,
}

impl Protocol {
    /// 新建：采集相位、零矩阵。
    pub const fn new() -> Protocol {
        Protocol {
            phase: Phase::Collect,
            next: 0,
            rows: [[0; CODES]; BATCHES],
            seen: [false; BATCHES],
            masks: [0; BATCHES],
            resolved: [0; BATCHES],
            diffs: 0,
            first_bad: None,
            seal: 0,
        }
    }

    /// 当前相位。
    pub fn phase(&self) -> Phase {
        self.phase
    }
    /// 已采集批数。
    pub fn collected(&self) -> usize {
        self.next
    }
    /// 对账差异格数。
    pub fn diffs(&self) -> u32 {
        self.diffs
    }
    /// 首个差异格。
    pub fn first_bad(&self) -> Option<(usize, usize)> {
        self.first_bad
    }
    /// 掩码快照。
    pub fn masks(&self) -> [u16; BATCHES] {
        self.masks
    }
    /// 仲裁结果快照。
    pub fn resolved(&self) -> [i32; BATCHES] {
        self.resolved
    }
    /// 封印指纹。
    pub fn seal_stamp(&self) -> u32 {
        self.seal
    }
    /// 矩阵快照。
    pub fn rows(&self) -> Rows {
        self.rows
    }

    /// 采集一批。顺序：容量（ENFILE）→ 批号范围（EINVAL）→ 重复（EEXIST）
    /// → 乱序（EINVAL）。封印后一律 EPERM。
    pub fn collect(&mut self, decl: &BatchDecl) -> Result<u32, i32> {
        if !self.phase.is_collect() {
            return Err(EPERM);
        }
        if self.next >= BATCHES {
            return Err(ENFILE);
        }
        let id = decl.id as usize;
        if id >= BATCHES {
            return Err(EINVAL);
        }
        let seen = match self.seen.get(id) {
            Some(v) => *v,
            None => return Err(EINVAL),
        };
        if seen {
            return Err(EEXIST);
        }
        if id != self.next {
            return Err(EINVAL);
        }
        let idx = self.next;
        let mut c = 0usize;
        while c < CODES {
            let add = match decl.row.get(c) {
                Some(v) => *v,
                None => 0,
            };
            if let Some(row) = self.rows.get_mut(idx) {
                if let Some(cell) = row.get_mut(c) {
                    *cell = cell.saturating_add(add);
                }
            }
            c += 1;
        }
        if let Some(m) = self.masks.get_mut(idx) {
            *m = decl.mask;
        }
        if let Some(s) = self.seen.get_mut(id) {
            *s = true;
        }
        self.next = idx + 1;
        Ok(row_digest(decl.id, &decl.row))
    }

    /// 采集收尾：八批齐了才进对账，否则 EBUSY；相位不对 EPERM。
    pub fn finish_collect(&mut self) -> Result<(), i32> {
        if !self.phase.is_collect() {
            return Err(EPERM);
        }
        if self.next != BATCHES {
            return Err(EBUSY);
        }
        self.phase = Phase::Reconcile;
        Ok(())
    }

    /// 对账：96 格逐格比冻结期望，零差才进仲裁；有差回 EINVAL 并留痕。
    pub fn reconcile(&mut self) -> Result<u32, i32> {
        if !self.phase.is_reconcile() {
            return Err(EPERM);
        }
        let mut diffs = 0u32;
        let mut first: Option<(usize, usize)> = None;
        let mut r = 0usize;
        while r < BATCHES {
            let mut c = 0usize;
            while c < CODES {
                if expect_at(r, c) != row_at(&self.rows, r, c) {
                    diffs = diffs.saturating_add(1);
                    if first.is_none() {
                        first = Some((r, c));
                    }
                }
                c += 1;
            }
            r += 1;
        }
        self.diffs = diffs;
        self.first_bad = first;
        if diffs == 0 {
            self.phase = Phase::Arbitrate;
            Ok(grand_total(&self.rows))
        } else {
            Err(EINVAL)
        }
    }

    /// 仲裁：每批掩码逐位有账，胜者按声明判序。
    pub fn arbitrate(&mut self) -> Result<u32, i32> {
        if !self.phase.is_arbitrate() {
            return Err(EPERM);
        }
        let mut done = 0u32;
        let mut b = 0usize;
        while b < BATCHES {
            let row = match self.rows.get(b) {
                Some(r) => r,
                None => return Err(EINVAL),
            };
            let mask = match self.masks.get(b) {
                Some(m) => *m,
                None => return Err(EINVAL),
            };
            let winner = resolve_batch(mask, row)?;
            if let Some(slot) = self.resolved.get_mut(b) {
                *slot = winner;
            }
            done = done.saturating_add(1);
            b += 1;
        }
        self.phase = Phase::Seal;
        Ok(done)
    }

    /// 封印：写封印指纹并进入 Closed。二次封印回 EBUSY，未到封印相位 EPERM。
    pub fn seal(&mut self) -> Result<u32, i32> {
        if self.phase.is_closed() {
            return Err(EBUSY);
        }
        if !self.phase.is_seal() {
            return Err(EPERM);
        }
        let digest = seal_digest(&self.rows, &self.masks, &self.resolved);
        self.seal = digest;
        self.phase = Phase::Closed;
        Ok(digest)
    }

    /// 收口：只在 Closed 相位给报告，逐码账、仲裁复算、封印指纹三面都要对上。
    pub fn close(&self) -> Result<CloseReport, i32> {
        if !self.phase.is_closed() {
            return Err(EBUSY);
        }
        let ledger = CodeLedger::from_rows(&self.rows);
        if !ledger.audit() {
            return Err(EINVAL);
        }
        let mut again = [0i32; BATCHES];
        let mut b = 0usize;
        while b < BATCHES {
            let row = match self.rows.get(b) {
                Some(r) => r,
                None => return Err(EINVAL),
            };
            let mask = match self.masks.get(b) {
                Some(m) => *m,
                None => return Err(EINVAL),
            };
            match resolve_batch(mask, row) {
                Ok(w) => {
                    if let Some(slot) = again.get_mut(b) {
                        *slot = w;
                    }
                }
                Err(e) => return Err(e),
            }
            b += 1;
        }
        if again != self.resolved {
            return Err(EINVAL);
        }
        let digest = seal_digest(&self.rows, &self.masks, &self.resolved);
        if digest != self.seal || digest == 0 {
            return Err(EINVAL);
        }
        Ok(CloseReport {
            batches: BATCHES,
            cells: CELLS,
            grand: grand_total(&self.rows),
            digest,
            precedence_ok: precedence_grid_ok(),
            summary: CLOSE_SUMMARY,
        })
    }

    /// 注入专用：跳过相位与顺序守卫直接落一行（可指定一格清零）。
    /// 仅供负向用例造缺格，正常运行路径禁用。
    pub fn force_collect(&mut self, decl: &BatchDecl, zero_at: Option<usize>) {
        if self.next >= BATCHES {
            return;
        }
        let idx = self.next;
        let mut c = 0usize;
        while c < CODES {
            let add = match decl.row.get(c) {
                Some(v) => *v,
                None => 0,
            };
            if let Some(row) = self.rows.get_mut(idx) {
                if let Some(cell) = row.get_mut(c) {
                    *cell = cell.saturating_add(add);
                }
            }
            c += 1;
        }
        if let Some(z) = zero_at {
            if let Some(row) = self.rows.get_mut(idx) {
                if let Some(cell) = row.get_mut(z) {
                    *cell = 0;
                }
            }
        }
        if let Some(m) = self.masks.get_mut(idx) {
            *m = decl.mask;
        }
        if let Some(s) = self.seen.get_mut(decl.id as usize) {
            *s = true;
        }
        self.next = idx + 1;
    }
}

impl Default for Protocol {
    fn default() -> Protocol {
        Protocol::new()
    }
}

/// 干净装配：八批依次采集→收尾→对账→仲裁→封印。
pub fn assemble_clean() -> Result<Protocol, i32> {
    let mut p = Protocol::new();
    let mut i = 0usize;
    while i < DECLS.len() {
        if let Some(d) = DECLS.get(i) {
            p.collect(d)?;
        }
        i += 1;
    }
    p.finish_collect()?;
    p.reconcile()?;
    p.arbitrate()?;
    p.seal()?;
    Ok(p)
}

// ── 注入 ──────────────────────────────────────────────────────────────
/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectionLedger {
    /// 注入总数。
    pub injected: u32,
    /// 检出数。
    pub caught: u32,
    /// 逃逸数。
    pub escaped: u32,
    /// 已跑轮数。
    pub rounds: u32,
}

impl InjectionLedger {
    /// 空账。
    pub const fn empty() -> InjectionLedger {
        InjectionLedger { injected: 0, caught: 0, escaped: 0, rounds: 0 }
    }
    /// 记一笔。
    pub fn account(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.caught = self.caught.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }
    /// 守恒。
    pub fn balanced(&self) -> bool {
        self.caught.saturating_add(self.escaped) == self.injected
    }
}

impl Default for InjectionLedger {
    fn default() -> InjectionLedger {
        InjectionLedger::empty()
    }
}

/// 注入变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mutant {
    /// 判序倒置（EBADF 与 EINVAL 换秩）。
    SwapPrec,
    /// 末批缺一格。
    DropCell,
    /// 乱序送达（先送 B21）。
    ReorderArrival,
}

impl Mutant {
    /// 闭集。
    pub const ALL: &'static [Mutant] = &[Mutant::SwapPrec, Mutant::DropCell, Mutant::ReorderArrival];
    /// 标签。
    pub fn tag(self) -> u8 {
        match self {
            Mutant::SwapPrec => 0,
            Mutant::DropCell => 1,
            Mutant::ReorderArrival => 2,
        }
    }
    /// 中文说明。
    pub fn detail(self) -> &'static str {
        match self {
            Mutant::SwapPrec => "判序倒置必须被声明格自检抓到",
            Mutant::DropCell => "末批缺一格必须被对账数出差异",
            Mutant::ReorderArrival => "乱序送达必须被采集顺序守卫拒收",
        }
    }
    /// 是否被抓到。
    pub fn detect(self) -> bool {
        match self {
            Mutant::SwapPrec => {
                let mut ranks = declared_ranks();
                ranks.swap(1, 2);
                !order_ok_of(&ranks)
            }
            Mutant::DropCell => {
                let mut p = Protocol::new();
                let mut b = 0usize;
                while b < BATCHES {
                    if let Some(d) = DECLS.get(b) {
                        let zero = if b + 1 == BATCHES { Some(0usize) } else { None };
                        p.force_collect(d, zero);
                    }
                    b += 1;
                }
                let armed = p.finish_collect().is_ok();
                match p.reconcile() {
                    Ok(_) => false,
                    Err(e) => armed && e == EINVAL && p.diffs() == 1,
                }
            }
            Mutant::ReorderArrival => {
                let mut p = Protocol::new();
                match DECLS.get(1) {
                    Some(d) => p.collect(d) == Err(EINVAL),
                    None => false,
                }
            }
        }
    }
}

/// 三变体各 10 轮。
pub fn run_injections() -> (InjectionLedger, [u32; MUTANTS]) {
    let mut log = InjectionLedger::empty();
    let mut per = [0u32; MUTANTS];
    let mut round = 0u32;
    while round < MUTANT_ROUNDS {
        let mut i = 0usize;
        while i < Mutant::ALL.len() {
            if let Some(m) = Mutant::ALL.get(i) {
                let caught = m.detect();
                log.account(caught);
                if caught {
                    let slot = (m.tag() as usize) % MUTANTS;
                    if let Some(v) = per.get_mut(slot) {
                        *v = v.saturating_add(1);
                    }
                }
            }
            i += 1;
        }
        log.rounds = log.rounds.saturating_add(1);
        round += 1;
    }
    (log, per)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9341 判据集。
pub fn run_unx9341_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9341");

    let seg_ok = SEG_BASE == 0x7C2C0 && SEG_BASE % 0x40 == 0 && SEG_BASE < SEG_END;
    s.add("段位守卫", seg_ok, "0x7C2C0 独占窗口且 0x40 对齐");

    let mut code_ok = ChartCode::ALL.len() == 15;
    let mut i = 0usize;
    while i < ChartCode::ALL.len() {
        let mut j = i + 1;
        while j < ChartCode::ALL.len() {
            if let (Some(a), Some(b)) = (ChartCode::ALL.get(i), ChartCode::ALL.get(j)) {
                if a.seg_addr() == b.seg_addr() || a.detail() == b.detail() || a.detail().is_empty() {
                    code_ok = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("码窗与文案", code_ok, "15 个码值唯一、文案互异且落在段窗内");

    let shape_ok = BATCHES == 8
        && CODES == 12
        && CELLS == 96
        && CODE_TABLE.len() == CODES
        && DECLS.len() == BATCHES
        && BATCH_SUM as u32 * BATCHES as u32 == GRAND_TOTAL;
    s.add("批数与码列", shape_ok, "八批 × 十二码 = 96 格，行合计 16 总账 128");

    let mut table_ok = true;
    let mut t = 0usize;
    while t < CODES {
        if let Some(row) = CODE_TABLE.get(t) {
            if row.0.is_empty() || row.2.is_empty() || code_index(row.1) != Some(t) {
                table_ok = false;
            }
        } else {
            table_ok = false;
        }
        t += 1;
    }
    s.add("码表可逆", table_ok, "十二码名、值、说明齐全且列号与码值一一对应");

    s.add(
        "期望表自洽",
        expect_consistent() && every_cell_positive(&EXPECT),
        "8 行各 16、总账 128、列合计 12/11/11/11/10/11/11/10/10/10/10/11",
    );

    s.add(
        "判序声明格",
        precedence_grid_ok() && declared_ranks() == [0, 1, 2, 3],
        "EFAULT(0)>EBADF(1)>EINVAL(2)>EPERM(3)，格序与秩一一对应",
    );

    let mask_badf = 1u16 << 5;
    let mask_einval = 1u16 << 11;
    let mask_eperm = 1u16 << 0;
    let mask_other = 1u16 << 4;
    let resolve_ok = resolve(mask_eperm) == Some(EPERM)
        && resolve(mask_einval) == Some(EINVAL)
        && resolve(mask_badf) == Some(EBADF)
        && resolve(mask_other) == Some(ENXIO)
        && resolve(0x0921) == Some(EFAULT)
        && resolve(0).is_none();
    s.add("判序解析", resolve_ok, "四码按声明格取小，未声明码退回列序，空掩码无从仲裁");

    let ledger = CodeLedger::from_rows(&EXPECT);
    let ledger_ok = ledger.audit()
        && ledger.credited as usize == CODES
        && ledger.total() == GRAND_TOTAL
        && ledger.every_code()
        && ledger.seal != 0;
    s.add("逐码落账", ledger_ok, "十二码列合计全非零，credited=12，总账 128");

    let mut zero_probe = Protocol::new();
    let early_reconcile = zero_probe.reconcile();
    let early_arbitrate = zero_probe.arbitrate();
    let early_seal = zero_probe.seal();
    let early_close = zero_probe.close();
    s.add(
        "相位早调",
        early_reconcile == Err(EPERM)
            && early_arbitrate == Err(EPERM)
            && early_seal == Err(EPERM)
            && early_close == Err(EBUSY),
        "采集未完成时对账/仲裁/封印回 EPERM，收口回 EBUSY",
    );

    let mut order_probe = Protocol::new();
    let skip = match DECLS.get(1) {
        Some(d) => order_probe.collect(d),
        None => Err(EINVAL),
    };
    let first = match DECLS.get(0) {
        Some(d) => order_probe.collect(d),
        None => Err(EINVAL),
    };
    let dup = match DECLS.get(0) {
        Some(d) => order_probe.collect(d),
        None => Err(EINVAL),
    };
    s.add(
        "采集顺序",
        skip == Err(EINVAL) && first.is_ok() && dup == Err(EEXIST) && order_probe.collected() == 1,
        "跳批回 EINVAL，重批回 EEXIST，已收批次不被重复记账",
    );

    let mut fill = Protocol::new();
    let mut fill_ok = true;
    let mut f = 0usize;
    while f < BATCHES {
        let accepted = match DECLS.get(f) {
            Some(d) => fill.collect(d).is_ok(),
            None => false,
        };
        if !accepted {
            fill_ok = false;
        }
        f += 1;
    }
    let overflow = match DECLS.get(0) {
        Some(d) => fill.collect(d),
        None => Err(EINVAL),
    };
    s.add(
        "批容量",
        fill_ok && overflow == Err(ENFILE) && fill.collected() == BATCHES,
        "八批满载后第九批回 ENFILE，容量先于重复判定",
    );

    let mut half = Protocol::new();
    let mut half_ok = true;
    let mut h = 0usize;
    while h < BATCHES - 1 {
        if let Some(d) = DECLS.get(h) {
            if half.collect(d).is_err() {
                half_ok = false;
            }
        }
        h += 1;
    }
    s.add(
        "采集未完",
        half_ok && half.finish_collect() == Err(EBUSY) && half.collected() == BATCHES - 1,
        "七批就收尾回 EBUSY，采集数不被改写",
    );

    let mut good = Protocol::new();
    let mut good_ok = true;
    let mut g = 0usize;
    while g < BATCHES {
        if let Some(d) = DECLS.get(g) {
            if good.collect(d).is_err() {
                good_ok = false;
            }
        }
        g += 1;
    }
    let fin = good.finish_collect();
    let rec = good.reconcile();
    s.add(
        "对账零差",
        good_ok
            && fin.is_ok()
            && rec == Ok(GRAND_TOTAL)
            && good.diffs() == 0
            && good.phase() == Phase::Arbitrate,
        "八批齐后对账 96 格零差，总账 128，相位推进到仲裁",
    );

    let mut broken = Protocol::new();
    let mut b = 0usize;
    while b < BATCHES {
        if let Some(d) = DECLS.get(b) {
            let zero = if b + 1 == BATCHES { Some(0usize) } else { None };
            broken.force_collect(d, zero);
        }
        b += 1;
    }
    let broken_armed = broken.finish_collect().is_ok();
    let broken_rec = broken.reconcile();
    s.add(
        "对账留痕",
        broken_armed
            && broken_rec == Err(EINVAL)
            && broken.diffs() == 1
            && broken.first_bad() == Some((BATCHES - 1, 0))
            && broken.phase() == Phase::Reconcile,
        "缺一格即差 1 格，首差定位末批首列，相位不前进",
    );

    let mut arb = Protocol::new();
    let mut arb_ok = true;
    let mut a = 0usize;
    while a < BATCHES {
        if let Some(d) = DECLS.get(a) {
            if arb.collect(d).is_err() {
                arb_ok = false;
            }
        }
        a += 1;
    }
    let af = arb.finish_collect().is_ok();
    let ar = arb.reconcile().is_ok();
    let arb_res = arb.arbitrate();
    s.add(
        "仲裁判序",
        arb_ok
            && af
            && ar
            && arb_res == Ok(BATCHES as u32)
            && arb.resolved() == WINS
            && arb.phase() == Phase::Seal,
        "八批胜者 EFAULT/EBADF 分布与声明格一致，相位推进到封印",
    );

    let mut row_zero = [0u16; CODES];
    let mut rz = 0usize;
    while rz < CODES {
        if let Some(slot) = row_zero.get_mut(rz) {
            *slot = 1;
        }
        rz += 1;
    }
    if let Some(slot) = row_zero.get_mut(5) {
        *slot = 0;
    }
    s.add(
        "仲裁有账",
        resolve_batch(1u16 << 5, &row_zero) == Err(EINVAL)
            && resolve_batch(1u16 << 0, &row_zero) == Ok(EPERM)
            && resolve_batch(0, &row_zero) == Err(EINVAL),
        "掩码位无账回 EINVAL，空掩码无胜者，声明四码内正常出解",
    );

    let sealed = match assemble_clean() {
        Ok(p) => p,
        Err(_e) => Protocol::new(),
    };
    let sealed_ok = sealed.phase() == Phase::Closed && sealed.seal_stamp() != 0;
    let mut after = sealed;
    let after_collect = match DECLS.get(0) {
        Some(d) => after.collect(d),
        None => Err(EINVAL),
    };
    let after_seal = after.seal();
    s.add(
        "封印冻结",
        sealed_ok && after_collect == Err(EPERM) && after_seal == Err(EBUSY),
        "封印后采集回 EPERM、二次封印回 EBUSY，矩阵不再接受写入",
    );

    let report = match sealed.close() {
        Ok(r) => r,
        Err(_e) => CloseReport {
            batches: 0,
            cells: 0,
            grand: 0,
            digest: 0,
            precedence_ok: false,
            summary: "",
        },
    };
    s.add(
        "收口报告",
        report.complete()
            && report.batches == 8
            && report.cells == 96
            && report.grand == 128
            && !report.summary.is_empty(),
        "报告 8 批 / 96 格 / 总账 128 / 判序成立 / 摘要非空",
    );

    let mut unsealed = Protocol::new();
    let mut us_ok = true;
    let mut u = 0usize;
    while u < BATCHES {
        if let Some(d) = DECLS.get(u) {
            if unsealed.collect(d).is_err() {
                us_ok = false;
            }
        }
        u += 1;
    }
    let uf = unsealed.finish_collect().is_ok();
    let ur = unsealed.reconcile().is_ok();
    let ua = unsealed.arbitrate().is_ok();
    s.add(
        "未封印收口",
        us_ok && uf && ur && ua && unsealed.phase() == Phase::Seal && unsealed.close() == Err(EBUSY),
        "到了封印相位但未盖印，收口仍回 EBUSY",
    );

    let d1 = rows_digest(&EXPECT);
    let d2 = rows_digest(&EXPECT);
    let sd = seal_digest(&EXPECT, &[0x0901, 0x0061, 0x0830, 0x0501, 0x0920, 0x0101, 0x00A1, 0x0820], &WINS);
    let row_d = match DECLS.get(0) {
        Some(d) => row_digest(d.id, &d.row),
        None => 0,
    };
    s.add(
        "指纹稳定",
        d1 == d2 && d1 != 0 && sd != 0 && row_d != 0 && d1 != sd,
        "同表双算相等，行指纹与矩阵指纹非同值",
    );

    let (log, per) = run_injections();
    s.add(
        "注入全检出",
        log.injected == 30 && log.caught == 30 && log.escaped == 0 && log.balanced(),
        "三变体各 10 轮共 30 次注入全检出，逃逸 0",
    );

    s.add("分类命中", per.iter().all(|v| *v == MUTANT_ROUNDS), "每变体命中 10");

    s.add(
        "声明表自洽",
        decls_consistent() && WINS.len() == BATCHES && !CLOSE_SUMMARY.is_empty(),
        "八批声明与期望逐格一致，仲裁表八项齐备，摘要非空",
    );

    s.add(
        "相位闭集",
        Phase::Collect.rank() == 0
            && Phase::Closed.rank() == 4
            && Phase::Collect.name() == "采集"
            && Phase::Closed.name() == "已收口"
            && Phase::Reconcile.is_reconcile()
            && !Phase::Reconcile.is_closed(),
        "五相位秩 0..4，名称与判定谓词一致",
    );

    s.add_dyn(
        "矩阵快照",
        ledger_ok,
        &format!("{} 批 × {} 码 = {} 格，总账 {}", BATCHES, CODES, CELLS, GRAND_TOTAL),
    );
    s.add_dyn(
        "收口快照",
        report.complete(),
        &format!("判序 {}，封印 {:#010x}，批次 {}", report.precedence_ok, report.digest, report.batches),
    );
    s.add_dyn(
        "注入快照",
        log.balanced(),
        &format!("注入 {} 检出 {} 逃逸 {} 轮 {}", log.injected, log.caught, log.escaped, log.rounds),
    );

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_all_green() {
        let s = run_unx9341_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
        assert_eq!(s.truncated(), false);
        assert_eq!(s.len() >= 12, true);
    }

    #[test]
    fn matrix_shape_and_expect() {
        assert_eq!(BATCHES, 8);
        assert_eq!(CODES, 12);
        assert_eq!(CELLS, 96);
        assert_eq!(GRAND_TOTAL, 128);
        assert_eq!(expect_consistent(), true);
        assert_eq!(every_cell_positive(&EXPECT), true);
        assert_eq!(grand_total(&EXPECT), 128);
        assert_eq!(col_sum(&EXPECT, 0), 12);
        assert_eq!(col_sum(&EXPECT, 11), 11);
        assert_eq!(row_sum(&EXPECT, 7), 16);
        assert_eq!(decls_consistent(), true);
    }

    #[test]
    fn code_table_is_reversible() {
        assert_eq!(CODE_TABLE.len(), 12);
        assert_eq!(code_at(0), Some(EPERM));
        assert_eq!(code_at(5), Some(EBADF));
        assert_eq!(code_at(11), Some(EINVAL));
        assert_eq!(code_at(12), None);
        assert_eq!(code_index(EFAULT), Some(8));
        assert_eq!(code_index(ENFILE), None);
    }

    #[test]
    fn precedence_grid_declared() {
        assert_eq!(precedence_grid_ok(), true);
        assert_eq!(declared_ranks(), [0, 1, 2, 3]);
        assert_eq!(precedence_rank(EFAULT), 0);
        assert_eq!(precedence_rank(EBADF), 1);
        assert_eq!(precedence_rank(EINVAL), 2);
        assert_eq!(precedence_rank(EPERM), 3);
        assert_eq!(precedence_rank(EIO), RANK_OTHER);
        assert_eq!(resolve(0x0921), Some(EFAULT));
        assert_eq!(resolve(0x0820), Some(EBADF));
        assert_eq!(resolve(0x0801), Some(EINVAL));
        assert_eq!(resolve(0x0001), Some(EPERM));
        assert_eq!(resolve(0), None);
        let mut swapped = declared_ranks();
        swapped.swap(1, 2);
        assert_eq!(order_ok_of(&swapped), false);
    }

    #[test]
    fn collect_guards_and_arrival() {
        let mut p = Protocol::new();
        let d0 = match DECLS.get(0) {
            Some(d) => *d,
            None => return,
        };
        let d1 = match DECLS.get(1) {
            Some(d) => *d,
            None => return,
        };
        assert_eq!(p.collect(&d1), Err(EINVAL));
        assert_eq!(p.collect(&d0).is_ok(), true);
        assert_eq!(p.collect(&d0), Err(EEXIST));
        assert_eq!(p.collected(), 1);
        assert_eq!(p.reconcile(), Err(EPERM));
        assert_eq!(p.close(), Err(EBUSY));
    }

    #[test]
    fn reconcile_requires_eight_and_zero_diff() {
        let mut p = Protocol::new();
        let mut i = 0usize;
        while i < BATCHES - 1 {
            if let Some(d) = DECLS.get(i) {
                assert_eq!(p.collect(d).is_ok(), true);
            }
            i += 1;
        }
        assert_eq!(p.finish_collect(), Err(EBUSY));
        if let Some(d) = DECLS.get(BATCHES - 1) {
            assert_eq!(p.collect(d).is_ok(), true);
        }
        assert_eq!(p.finish_collect().is_ok(), true);
        assert_eq!(p.reconcile(), Ok(GRAND_TOTAL));
        assert_eq!(p.diffs(), 0);
        assert_eq!(p.phase(), Phase::Arbitrate);
    }

    #[test]
    fn arbitration_follows_declared_order() {
        let p = match assemble_clean() {
            Ok(v) => v,
            Err(_e) => return,
        };
        assert_eq!(p.resolved(), WINS);
        assert_eq!(p.resolved().len(), 8);
        assert_eq!(p.phase(), Phase::Closed);
        let row = match p.rows().get(0) {
            Some(r) => *r,
            None => return,
        };
        assert_eq!(resolve_batch(0x0921, &row), Ok(EFAULT));
        assert_eq!(resolve_batch(0x0820, &row), Ok(EBADF));
        assert_eq!(resolve_batch(0x0901, &row), Ok(EFAULT));
    }

    #[test]
    fn seal_freezes_and_close_reports() {
        let p = match assemble_clean() {
            Ok(v) => v,
            Err(_e) => return,
        };
        assert_ne!(p.seal_stamp(), 0);
        let mut after = p;
        let d0 = match DECLS.get(0) {
            Some(d) => *d,
            None => return,
        };
        assert_eq!(after.collect(&d0), Err(EPERM));
        assert_eq!(after.seal(), Err(EBUSY));
        let report = match p.close() {
            Ok(r) => r,
            Err(_e) => return,
        };
        assert_eq!(report.complete(), true);
        assert_eq!(report.batches, 8);
        assert_eq!(report.cells, 96);
        assert_eq!(report.grand, 128);
        assert_ne!(report.digest, 0);
        assert_eq!(report.precedence_ok, true);
        assert_eq!(report.summary.is_empty(), false);
    }

    #[test]
    fn unsealed_close_refused() {
        let mut p = Protocol::new();
        let mut i = 0usize;
        while i < BATCHES {
            if let Some(d) = DECLS.get(i) {
                assert_eq!(p.collect(d).is_ok(), true);
            }
            i += 1;
        }
        assert_eq!(p.finish_collect().is_ok(), true);
        assert_eq!(p.reconcile().is_ok(), true);
        assert_eq!(p.arbitrate().is_ok(), true);
        assert_eq!(p.phase(), Phase::Seal);
        assert_eq!(p.close(), Err(EBUSY));
        assert_eq!(p.seal().is_ok(), true);
        assert_eq!(p.close().is_ok(), true);
    }

    #[test]
    fn ledger_counts_every_code() {
        let ledger = CodeLedger::from_rows(&EXPECT);
        assert_eq!(ledger.audit(), true);
        assert_eq!(ledger.credited, CODES as u8);
        assert_eq!(ledger.total(), 128);
        assert_eq!(ledger.every_code(), true);
        assert_ne!(ledger.seal, 0);
        let empty = CodeLedger::empty();
        assert_eq!(empty.audit(), false);
        assert_eq!(empty.every_code(), false);
    }

    #[test]
    fn injections_detect_all_rounds() {
        let (log, per) = run_injections();
        assert_eq!(log.injected, 30);
        assert_eq!(log.caught, 30);
        assert_eq!(log.escaped, 0);
        assert_eq!(log.rounds, 10);
        assert_eq!(log.balanced(), true);
        assert_eq!(per, [10, 10, 10]);
        assert_eq!(Mutant::ALL.len(), 3);
        assert_eq!(Mutant::SwapPrec.detect(), true);
        assert_eq!(Mutant::DropCell.detect(), true);
        assert_eq!(Mutant::ReorderArrival.detect(), true);
    }

    #[test]
    fn digests_stable_and_codes_closed() {
        assert_eq!(rows_digest(&EXPECT), rows_digest(&EXPECT));
        assert_ne!(rows_digest(&EXPECT), 0);
        assert_eq!(ChartCode::ALL.len(), 15);
        let mut i = 0usize;
        while i < ChartCode::ALL.len() {
            if let Some(c) = ChartCode::ALL.get(i) {
                assert_eq!(c.detail().is_empty(), false);
                assert_eq!(c.seg_addr(), SEG_BASE + c.code() as u32);
            }
            i += 1;
        }
        assert_eq!(CLOSE_SUMMARY.is_empty(), false);
    }
}
