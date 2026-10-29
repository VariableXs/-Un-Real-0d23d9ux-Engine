//! UNX-F11135 B20 清账（M 型总收口判据终重放）（0x1f230 段）
//!
//! 纯功能模型：M 型总收口由两本账合拢而成。
//!
//! 其一为「交互矩阵账」：4 行 × 5 列共 20 格，行内结论只允许通过（1）
//! 与豁免（3），失败（2）一格即整行不成账；每行至多一格豁免、至少四格
//! 通过。豁免不是免检，必须挂一枚已登记的范围勘误注记（注记号 > 0），
//! 无注记的豁免当场拒止——这是清账段最容易放水的一处，故设硬门。
//!
//! 其二为「总收口判据账」：M1~M10 十条判据，每条终重放十轮，十轮全过
//! 才许收口；十轮之内出现一次失败，该判据整条不成账，重放不得续记。
//!
//! 总收口 = 矩阵四行闭合 ∧ 十条判据全闭合 ∧ 二十格无缺账。两本账都
//! 闭合后封盘，指纹并入基址与两账分指纹。
//!
//! 拒止面：判据号越界、格号越界、结论越界、重复登记、格序倒挂、行内
//! 不满足收口、重放未完即收口、豁免无注记、总收口账不平、封盘后动账。

use crate::alloc::format;
use crate::checks::CheckSet;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f230;
/// 判据号越界
pub const MT_CRIT: u32 = SEG_BASE + 0x01;
/// 格号越界
pub const MT_CELL: u32 = SEG_BASE + 0x02;
/// 结论越界
pub const MT_VERDICT: u32 = SEG_BASE + 0x03;
/// 重复登记或重复重放
pub const MT_DUP: u32 = SEG_BASE + 0x04;
/// 格序倒挂（格号与行列不对应）
pub const MT_ORDER: u32 = SEG_BASE + 0x05;
/// 行内结论不满足收口
pub const MT_ROW: u32 = SEG_BASE + 0x06;
/// 重放未满即收口
pub const MT_REPLAY: u32 = SEG_BASE + 0x07;
/// 豁免无注记或豁免超量
pub const MT_EXEMPT: u32 = SEG_BASE + 0x08;
/// 总收口账不平
pub const MT_LEDGER: u32 = SEG_BASE + 0x09;
/// 账已封盘
pub const MT_FROZEN: u32 = SEG_BASE + 0x0a;

/// 矩阵行数。
pub const ROWS: usize = 4;
/// 矩阵列数。
pub const COLS: usize = 5;
/// 矩阵格数。
pub const CELLS: usize = ROWS * COLS;
/// 总收口判据条数。
pub const CRITS: usize = 10;
/// 每条判据的终重放轮数。
pub const SLOTS: usize = 10;
/// 判据重放总轮数。
pub const REPLAYS: usize = CRITS * SLOTS;
/// 每行至少通过格数。
pub const ROW_MIN_PASS: usize = 4;
/// 每行至多豁免格数。
pub const EXEMPT_MAX: usize = 1;
/// 结论：通过。
pub const V_PASS: u8 = 1;
/// 结论：失败。
pub const V_FAIL: u8 = 2;
/// 结论：豁免。
pub const V_EXEMPT: u8 = 3;

/// 行名。
pub const ROW_NAMES: [&str; ROWS] = ["调用族交互", "状态迁移交互", "错误码交互", "账务交互"];
/// 列名。
pub const COL_NAMES: [&str; COLS] = ["登记", "观测", "对平", "注记", "收口"];
/// 判据名。
pub const CRIT_NAMES: [&str; CRITS] = [
    "M1-矩阵格齐",
    "M2-行内零失败",
    "M3-豁免有注",
    "M4-重放十轮",
    "M5-判据零失败",
    "M6-列账齐五",
    "M7-权重合计",
    "M8-账平 20/20",
    "M9-封盘前置",
    "M10-终重放",
];
/// 每条判据的权重（合计恰 100）。
pub const CRIT_WEIGHTS: [u32; CRITS] = [10, 10, 10, 10, 10, 10, 10, 10, 10, 10];
/// 权重合计。
pub const WEIGHT_TOTAL: u32 = 100;

/// 基线矩阵结论表：第 2 行第 3 列豁免（挂注记 0x2301），其余通过。
pub const BASE_VERDICTS: [u8; CELLS] = [
    V_PASS, V_PASS, V_PASS, V_PASS, V_PASS, // 行 0
    V_PASS, V_PASS, V_PASS, V_PASS, V_PASS, // 行 1
    V_PASS, V_PASS, V_EXEMPT, V_PASS, V_PASS, // 行 2
    V_PASS, V_PASS, V_PASS, V_PASS, V_PASS, // 行 3
];
/// 豁免注记号（0 表示无注记）。
pub const BASE_NOTE: u32 = 0x2301;
/// 豁免所在格号（1 起，行 2 列 2 => 2*5+2+1 = 13）。
pub const EXEMPT_CELL: usize = 13;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 格号（1 起）：行优先。
pub fn cell_no(row: usize, col: usize) -> usize {
    row.wrapping_mul(COLS).wrapping_add(col).wrapping_add(1)
}

/// 格号所在行。
pub fn cell_row(no: usize) -> Option<usize> {
    if no == 0 || no > CELLS {
        return None;
    }
    Some((no - 1) / COLS)
}

/// 格号所在列。
pub fn cell_col(no: usize) -> Option<usize> {
    if no == 0 || no > CELLS {
        return None;
    }
    Some((no - 1) % COLS)
}

/// 结论名。
pub fn verdict_name(v: u8) -> &'static str {
    match v {
        V_PASS => "通过",
        V_FAIL => "失败",
        V_EXEMPT => "豁免",
        _ => "越界",
    }
}

/// 结论是否在闭集内。
pub fn verdict_ok(v: u8) -> bool {
    v == V_PASS || v == V_FAIL || v == V_EXEMPT
}

/// 判据权重。
pub fn weight_of(crit: usize) -> u32 {
    match CRIT_WEIGHTS.get(crit) {
        Some(w) => *w,
        None => 0,
    }
}

/// 交互矩阵的一格。
#[derive(Clone, Copy, Debug)]
pub struct Cell {
    /// 格号（1..=20）。
    pub no: usize,
    /// 行号。
    pub row: usize,
    /// 列号。
    pub col: usize,
    /// 结论。
    pub verdict: u8,
    /// 豁免注记号（0 = 无注记）。
    pub note: u32,
    live: bool,
}

impl Cell {
    /// 空槽。
    pub const fn empty(no: usize) -> Cell {
        Cell {
            no,
            row: 0,
            col: 0,
            verdict: 0,
            note: 0,
            live: false,
        }
    }

    /// 是否已登记。
    pub fn live(&self) -> bool {
        self.live
    }

    /// 是否通过。
    pub fn is_pass(&self) -> bool {
        self.live && self.verdict == V_PASS
    }

    /// 是否失败。
    pub fn is_fail(&self) -> bool {
        self.live && self.verdict == V_FAIL
    }

    /// 是否豁免。
    pub fn is_exempt(&self) -> bool {
        self.live && self.verdict == V_EXEMPT
    }

    /// 豁免是否有注记背书。
    pub fn noted(&self) -> bool {
        self.is_exempt() && self.note != 0
    }

    /// 是否计入通过数（豁免需有注记才抵一格通过）。
    pub fn counts_pass(&self) -> bool {
        self.is_pass() || self.noted()
    }

    /// 格指纹。
    pub fn digest(&self) -> u64 {
        let mut h = FNV_OFF ^ (self.no as u64);
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.verdict as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.note as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= if self.live { 1 } else { 0 };
        h
    }
}

/// 总收口判据（十轮终重放）。
#[derive(Clone, Copy, Debug)]
pub struct Crit {
    /// 判据号（1..=10）。
    pub no: usize,
    replayed: usize,
    passes: usize,
    fails: usize,
    closed: bool,
    slots: [u8; SLOTS],
}

impl Crit {
    /// 新建空判据。
    pub const fn new(no: usize) -> Crit {
        Crit {
            no,
            replayed: 0,
            passes: 0,
            fails: 0,
            closed: false,
            slots: [0u8; SLOTS],
        }
    }

    /// 单轮重放。只收通过/失败两种结论；已收口或已满十轮即拒止。
    pub fn replay(&mut self, verdict: u8) -> Result<(), u32> {
        if self.closed || self.replayed >= SLOTS {
            return Err(MT_DUP);
        }
        if verdict != V_PASS && verdict != V_FAIL {
            return Err(MT_VERDICT);
        }
        if let Some(slot) = self.slots.get_mut(self.replayed) {
            *slot = verdict;
        }
        self.replayed = self.replayed.wrapping_add(1);
        if verdict == V_PASS {
            self.passes = self.passes.wrapping_add(1);
        } else {
            self.fails = self.fails.wrapping_add(1);
        }
        Ok(())
    }

    /// 收口：十轮走满且零失败。
    pub fn close(&mut self) -> Result<(), u32> {
        if self.closed {
            return Ok(());
        }
        if self.replayed != SLOTS || self.fails != 0 || self.passes != SLOTS {
            return Err(MT_REPLAY);
        }
        self.closed = true;
        Ok(())
    }

    /// 已重放轮数。
    pub fn replayed(&self) -> usize {
        self.replayed
    }

    /// 通过轮数。
    pub fn passes(&self) -> usize {
        self.passes
    }

    /// 失败轮数。
    pub fn fails(&self) -> usize {
        self.fails
    }

    /// 是否收口。
    pub fn closed(&self) -> bool {
        self.closed
    }

    /// 重放轨迹指纹。
    pub fn digest(&self) -> u64 {
        let mut h = FNV_OFF ^ (self.no as u64);
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.replayed as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.fails as u64;
        h = h.wrapping_mul(FNV_PRIME);
        let mut i = 0usize;
        while i < SLOTS {
            if let Some(v) = self.slots.get(i) {
                h ^= *v as u64;
                h = h.wrapping_mul(FNV_PRIME);
            }
            i = i.wrapping_add(1);
        }
        h
    }
}

/// 交互矩阵账（4 行 × 5 列）。
#[derive(Clone, Debug)]
pub struct Matrix {
    cells: [Cell; CELLS],
    cursor: usize,
    rows_closed: [bool; ROWS],
}

impl Matrix {
    /// 新建二十个空槽。
    pub fn new() -> Matrix {
        let mut cells = [Cell::empty(1); CELLS];
        let mut i = 0usize;
        while i < CELLS {
            if let Some(c) = cells.get_mut(i) {
                *c = Cell::empty(i.wrapping_add(1));
            }
            i = i.wrapping_add(1);
        }
        Matrix {
            cells,
            cursor: 0,
            rows_closed: [false; ROWS],
        }
    }

    /// 登记一格。格号必须紧接游标（行优先单调），豁免必须挂注记。
    pub fn register(&mut self, no: usize, verdict: u8, note: u32) -> Result<(), u32> {
        if no == 0 || no > CELLS {
            return Err(MT_CELL);
        }
        if no != self.cursor.wrapping_add(1) {
            return Err(MT_ORDER);
        }
        if !verdict_ok(verdict) {
            return Err(MT_VERDICT);
        }
        if verdict == V_EXEMPT && note == 0 {
            return Err(MT_EXEMPT);
        }
        let row = match cell_row(no) {
            Some(r) => r,
            None => return Err(MT_CELL),
        };
        let col = match cell_col(no) {
            Some(c) => c,
            None => return Err(MT_CELL),
        };
        if let Some(slot) = self.cells.get_mut(no - 1) {
            *slot = Cell {
                no,
                row,
                col,
                verdict,
                note,
                live: true,
            };
        }
        self.cursor = no;
        Ok(())
    }

    /// 取一格。
    pub fn cell(&self, no: usize) -> Option<Cell> {
        if no == 0 || no > CELLS {
            return None;
        }
        self.cells.get(no - 1).copied()
    }

    /// 行内四类计数：[通过, 豁免, 失败, 在册]。
    pub fn row_counts(&self, row: usize) -> [usize; 4] {
        let mut out = [0usize; 4];
        let mut i = 0usize;
        while i < CELLS {
            if let Some(c) = self.cells.get(i) {
                if c.live && c.row == row {
                    match c.verdict {
                        V_PASS => out[0] = out[0].wrapping_add(1),
                        V_EXEMPT => out[1] = out[1].wrapping_add(1),
                        V_FAIL => out[2] = out[2].wrapping_add(1),
                        _ => {}
                    }
                    out[3] = out[3].wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        out
    }

    /// 行是否满足收口条件（在册五格、零失败、豁免不超一、通过不少于四）。
    pub fn row_ok(&self, row: usize) -> bool {
        if row >= ROWS {
            return false;
        }
        let c = self.row_counts(row);
        c[3] == COLS && c[2] == 0 && c[1] <= EXEMPT_MAX && c[0] >= ROW_MIN_PASS
    }

    /// 行收口。条件不满足按原因分码：缺格账不平、失败/通过不足落 MT_ROW、
    /// 豁免超量落 MT_EXEMPT。
    pub fn close_row(&mut self, row: usize) -> Result<(), u32> {
        if row >= ROWS {
            return Err(MT_CELL);
        }
        if let Some(done) = self.rows_closed.get(row) {
            if *done {
                return Ok(());
            }
        }
        let c = self.row_counts(row);
        if c[3] != COLS {
            return Err(MT_LEDGER);
        }
        if c[2] != 0 {
            return Err(MT_ROW);
        }
        if c[1] > EXEMPT_MAX {
            return Err(MT_EXEMPT);
        }
        if c[0] < ROW_MIN_PASS {
            return Err(MT_ROW);
        }
        if let Some(slot) = self.rows_closed.get_mut(row) {
            *slot = true;
        }
        Ok(())
    }

    /// 行是否已收口。
    pub fn row_closed(&self, row: usize) -> bool {
        match self.rows_closed.get(row) {
            Some(v) => *v,
            None => false,
        }
    }

    /// 已收口行数。
    pub fn closed_rows(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < ROWS {
            if self.row_closed(i) {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 在册格数。
    pub fn live_cells(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < CELLS {
            if let Some(c) = self.cells.get(i) {
                if c.live {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 结论直方图：[通过, 失败, 豁免]。
    pub fn histogram(&self) -> [usize; 3] {
        let mut h = [0usize; 3];
        let mut i = 0usize;
        while i < CELLS {
            if let Some(c) = self.cells.get(i) {
                match c.verdict {
                    V_PASS => h[0] = h[0].wrapping_add(1),
                    V_FAIL => h[1] = h[1].wrapping_add(1),
                    V_EXEMPT => h[2] = h[2].wrapping_add(1),
                    _ => {}
                }
            }
            i = i.wrapping_add(1);
        }
        h
    }

    /// 列账：[每列在册格数]。
    pub fn col_counts(&self) -> [usize; COLS] {
        let mut out = [0usize; COLS];
        let mut i = 0usize;
        while i < CELLS {
            if let Some(c) = self.cells.get(i) {
                if c.live {
                    if let Some(slot) = out.get_mut(c.col) {
                        *slot = slot.wrapping_add(1);
                    }
                }
            }
            i = i.wrapping_add(1);
        }
        out
    }

    /// 矩阵账指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF ^ (self.cursor as u64);
        h = h.wrapping_mul(FNV_PRIME);
        let mut i = 0usize;
        while i < CELLS {
            if let Some(c) = self.cells.get(i) {
                h ^= c.digest();
                h = h.wrapping_mul(FNV_PRIME);
            }
            i = i.wrapping_add(1);
        }
        i = 0usize;
        while i < ROWS {
            if let Some(done) = self.rows_closed.get(i) {
                h ^= if *done { 1 } else { 0 };
                h = h.wrapping_mul(FNV_PRIME);
            }
            i = i.wrapping_add(1);
        }
        h
    }
}

/// M 型总收口总账：矩阵账 + 判据账。
#[derive(Clone, Debug)]
pub struct MTypeLedger {
    /// 段内基址。
    pub base: u32,
    matrix: Matrix,
    crits: [Crit; CRITS],
    rejects: u32,
    frozen: bool,
}

impl MTypeLedger {
    /// 新建空账。
    pub fn new(base: u32) -> MTypeLedger {
        let mut crits = [Crit::new(1); CRITS];
        let mut i = 0usize;
        while i < CRITS {
            if let Some(c) = crits.get_mut(i) {
                *c = Crit::new(i.wrapping_add(1));
            }
            i = i.wrapping_add(1);
        }
        MTypeLedger {
            base,
            matrix: Matrix::new(),
            crits,
            rejects: 0,
            frozen: false,
        }
    }

    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 登记矩阵格。
    pub fn register_cell(&mut self, no: usize, verdict: u8, note: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(MT_FROZEN));
        }
        match self.matrix.register(no, verdict, note) {
            Ok(()) => Ok(()),
            Err(e) => Err(self.fail(e)),
        }
    }

    /// 判据单轮重放。
    pub fn replay_crit(&mut self, no: usize, verdict: u8) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(MT_FROZEN));
        }
        if no == 0 || no > CRITS {
            return Err(self.fail(MT_CRIT));
        }
        match self.crits.get_mut(no - 1) {
            Some(c) => match c.replay(verdict) {
                Ok(()) => Ok(()),
                Err(e) => Err(self.fail(e)),
            },
            None => Err(self.fail(MT_CRIT)),
        }
    }

    /// 判据收口。
    pub fn close_crit(&mut self, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(MT_FROZEN));
        }
        if no == 0 || no > CRITS {
            return Err(self.fail(MT_CRIT));
        }
        match self.crits.get_mut(no - 1) {
            Some(c) => match c.close() {
                Ok(()) => Ok(()),
                Err(e) => Err(self.fail(e)),
            },
            None => Err(self.fail(MT_CRIT)),
        }
    }

    /// 行收口。
    pub fn close_row(&mut self, row: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(MT_FROZEN));
        }
        match self.matrix.close_row(row) {
            Ok(()) => Ok(()),
            Err(e) => Err(self.fail(e)),
        }
    }

    /// 矩阵只读。
    pub fn matrix(&self) -> &Matrix {
        &self.matrix
    }

    /// 判据只读。
    pub fn crit(&self, no: usize) -> Option<Crit> {
        if no == 0 || no > CRITS {
            return None;
        }
        self.crits.get(no - 1).copied()
    }

    /// 已收口判据数。
    pub fn closed_crits(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < CRITS {
            if let Some(c) = self.crits.get(i) {
                if c.closed() {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 判据重放总轮数。
    pub fn total_replays(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < CRITS {
            if let Some(c) = self.crits.get(i) {
                n = n.wrapping_add(c.replayed());
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 判据失败总轮数。
    pub fn total_fails(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < CRITS {
            if let Some(c) = self.crits.get(i) {
                n = n.wrapping_add(c.fails());
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 已收口判据的权重合计。
    pub fn weighted_score(&self) -> u32 {
        let mut w = 0u32;
        let mut i = 0usize;
        while i < CRITS {
            if let Some(c) = self.crits.get(i) {
                if c.closed() {
                    w = w.wrapping_add(weight_of(i));
                }
            }
            i = i.wrapping_add(1);
        }
        w
    }

    /// 拒止计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否封盘。
    pub fn frozen(&self) -> bool {
        self.frozen
    }

    /// 总账指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        for b in self.base.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        h ^= if self.frozen { 1 } else { 0 };
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.rejects as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.matrix.fold();
        h = h.wrapping_mul(FNV_PRIME);
        let mut i = 0usize;
        while i < CRITS {
            if let Some(c) = self.crits.get(i) {
                h ^= c.digest();
                h = h.wrapping_mul(FNV_PRIME);
            }
            i = i.wrapping_add(1);
        }
        h
    }

    /// 封盘，幂等。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Ok(self.fold());
        }
        self.frozen = true;
        Ok(self.fold())
    }

    /// 终重放：四行闭合、二十格在册、十条判据闭合、权重满百，然后封盘。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if self.matrix.live_cells() != CELLS {
            return Err(self.fail(MT_LEDGER));
        }
        if self.matrix.closed_rows() != ROWS {
            return Err(self.fail(MT_LEDGER));
        }
        if self.closed_crits() != CRITS {
            return Err(self.fail(MT_REPLAY));
        }
        if self.total_fails() != 0 {
            return Err(self.fail(MT_REPLAY));
        }
        if self.weighted_score() != WEIGHT_TOTAL {
            return Err(self.fail(MT_LEDGER));
        }
        self.freeze()
    }
}

/// 搭建 M 型总收口账：二十格、四行、十判据十轮，最后终重放。
pub fn build_mtype(base: u32) -> (MTypeLedger, Result<u64, u32>) {
    let mut g = MTypeLedger::new(base);
    let mut no = 1usize;
    while no <= CELLS {
        let v = match BASE_VERDICTS.get(no.wrapping_sub(1)) {
            Some(x) => *x,
            None => V_PASS,
        };
        let note = if v == V_EXEMPT { BASE_NOTE } else { 0 };
        if g.register_cell(no, v, note).is_err() {
            return (g, Err(MT_LEDGER));
        }
        no = no.wrapping_add(1);
    }
    let mut r = 0usize;
    while r < ROWS {
        if g.close_row(r).is_err() {
            return (g, Err(MT_ROW));
        }
        r = r.wrapping_add(1);
    }
    let mut c = 1usize;
    while c <= CRITS {
        let mut k = 0usize;
        while k < SLOTS {
            if g.replay_crit(c, V_PASS).is_err() {
                return (g, Err(MT_REPLAY));
            }
            k = k.wrapping_add(1);
        }
        if g.close_crit(c).is_err() {
            return (g, Err(MT_REPLAY));
        }
        c = c.wrapping_add(1);
    }
    let out = g.replay();
    (g, out)
}

/// 四路注入：格号越界、豁免无注记、行内失败、十轮未满即收口。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    // ① 格号越界落 MT_CELL。
    let mut a = MTypeLedger::new(0x3500);
    out[0] = a.register_cell(CELLS + 1, V_PASS, 0).err() == Some(MT_CELL) && a.rejects() == 1;

    // ② 豁免未挂注记落 MT_EXEMPT。
    let mut b = MTypeLedger::new(0x3501);
    out[1] = b.register_cell(1, V_EXEMPT, 0).err() == Some(MT_EXEMPT) && b.rejects() == 1;

    // ③ 行内一格失败落 MT_ROW。
    let mut c = MTypeLedger::new(0x3502);
    let mut i = 1usize;
    let mut ok3 = true;
    while i <= COLS {
        let v = if i == 3 { V_FAIL } else { V_PASS };
        if c.register_cell(i, v, 0).is_err() {
            ok3 = false;
        }
        i = i.wrapping_add(1);
    }
    out[2] = ok3 && c.close_row(0).err() == Some(MT_ROW) && c.rejects() == 1;

    // ④ 判据只重放九轮就收口落 MT_REPLAY。
    let mut d = MTypeLedger::new(0x3503);
    let mut k = 0usize;
    let mut ok4 = true;
    while k < SLOTS - 1 {
        if d.replay_crit(1, V_PASS).is_err() {
            ok4 = false;
        }
        k = k.wrapping_add(1);
    }
    out[3] = ok4 && d.close_crit(1).err() == Some(MT_REPLAY) && d.rejects() == 1;

    out
}

/// 判据集：B1~B13。
pub fn run_unx11135_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11135.b20_mtype_close");
    let (g, r) = build_mtype(0x1135);

    // B1 常量面。
    let mut wsum = 0u32;
    let mut iw = 0usize;
    while iw < CRITS {
        wsum = wsum.wrapping_add(weight_of(iw));
        iw = iw.wrapping_add(1);
    }
    let c1 = SEG_BASE == 0x1f230
        && ROWS == 4
        && COLS == 5
        && CELLS == 20
        && CRITS == 10
        && SLOTS == 10
        && REPLAYS == 100
        && WEIGHT_TOTAL == 100
        && wsum == WEIGHT_TOTAL
        && BASE_VERDICTS.len() == CELLS
        && CRIT_NAMES.len() == CRITS
        && ROW_NAMES.len() == ROWS
        && COL_NAMES.len() == COLS;
    s.add("B1-常量面", c1, "4×5=20 格、10 判据×10 轮、权重百");

    // B2 矩阵在册 20/20，五列各 4 格。
    let cols = g.matrix().col_counts();
    let mut c2 = g.matrix().live_cells() == CELLS;
    let mut ic = 0usize;
    while ic < COLS {
        match cols.get(ic) {
            Some(v) => c2 = c2 && *v == ROWS,
            None => c2 = false,
        }
        ic = ic.wrapping_add(1);
    }
    s.add_dyn(
        "B2-格账 20/20",
        c2,
        &format!("live={}/{} cols={:?}", g.matrix().live_cells(), CELLS, cols),
    );

    // B3 行闭合 4/4，逐行零失败。
    let mut c3 = g.matrix().closed_rows() == ROWS;
    let mut ir = 0usize;
    while ir < ROWS {
        let rc = g.matrix().row_counts(ir);
        c3 = c3 && rc[2] == 0 && rc[3] == COLS && g.matrix().row_closed(ir);
        ir = ir.wrapping_add(1);
    }
    s.add_dyn(
        "B3-行闭合 4/4",
        c3,
        &format!("rows={}/{} ", g.matrix().closed_rows(), ROWS),
    );

    // B4 判据闭合 10/10、重放百轮、零失败。
    s.add_dyn(
        "B4-判据 10/10",
        g.closed_crits() == CRITS && g.total_replays() == REPLAYS && g.total_fails() == 0,
        &format!(
            "closed={}/{} replays={}/{} fails={}",
            g.closed_crits(),
            CRITS,
            g.total_replays(),
            REPLAYS,
            g.total_fails()
        ),
    );

    // B5 直方图 19 通过 / 0 失败 / 1 豁免，且豁免挂注记。
    let h = g.matrix().histogram();
    let ex = g.matrix().cell(EXEMPT_CELL);
    let c5 = h[0] == 19 && h[1] == 0 && h[2] == 1 && match ex {
        Some(c) => c.is_exempt() && c.noted() && c.note == BASE_NOTE,
        None => false,
    };
    s.add("B5-结论直方图", c5, "19 通过 / 0 失败 / 1 豁免（注记在册）");

    // B6 权重满百。
    s.add_dyn(
        "B6-权重合计",
        g.weighted_score() == WEIGHT_TOTAL,
        &format!("weight={}/{}", g.weighted_score(), WEIGHT_TOTAL),
    );

    // B7 总收口成立并封盘。
    s.add("B7-总收口成立", r.is_ok() && g.frozen(), "终重放通过并封盘");

    // B8 重放幂等。
    let mut c8 = g.clone();
    let before = c8.fold();
    let again = c8.replay();
    s.add(
        "B8-重放幂等",
        again.is_ok() && c8.fold() == before && c8.rejects() == g.rejects(),
        "二次重放同指纹、零新增拒止",
    );

    // B9 封盘幂等。
    let mut c9 = MTypeLedger::new(0x1135);
    let f1 = c9.freeze();
    let f2 = c9.freeze();
    s.add("B9-封盘幂等", f1.is_ok() && f1 == f2, "两次封盘同指纹");

    // B10 拒止优先序：封盘先于格号校验。
    let mut c10 = g.clone();
    let z1 = c10.register_cell(CELLS + 1, V_PASS, 0).err();
    let mut c10b = MTypeLedger::new(0x1136);
    let z2 = c10b.register_cell(CELLS + 1, V_PASS, 0).err();
    s.add(
        "B10-拒止优先序",
        z1 == Some(MT_FROZEN) && z2 == Some(MT_CELL),
        "封盘优先于格号；未封盘则 MT_CELL",
    );

    // B11 四路注入。
    let inj = inject_four();
    s.add(
        "B11-四路注入",
        inj[0] && inj[1] && inj[2] && inj[3],
        "格号/注记/行失败/轮次四路各自发火",
    );

    // B12 指纹灵敏度。
    let (g2, _) = build_mtype(0x1137);
    let (g3, _) = build_mtype(0x1135);
    s.add(
        "B12-指纹灵敏度",
        g2.fold() != g.fold() && g3.fold() == g.fold(),
        "异基址相异、同基址相同",
    );

    // B13 逐例归零。
    s.add(
        "B13-逐例归零",
        g.rejects() == 0
            && g.matrix().live_cells() == CELLS
            && g.matrix().closed_rows() == ROWS
            && g.closed_crits() == CRITS,
        "rejects=0，20 格 4 行 10 判据全闭",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11135_flow() {
        let (g, r) = build_mtype(0x4000);
        assert!(r.is_ok());
        assert!(g.frozen());
        assert_eq!(g.matrix().live_cells(), CELLS);
        assert_eq!(g.matrix().closed_rows(), ROWS);
        assert_eq!(g.closed_crits(), CRITS);
        assert_eq!(g.total_replays(), REPLAYS);
        assert_eq!(g.total_fails(), 0);
        assert_eq!(g.weighted_score(), WEIGHT_TOTAL);
        assert_eq!(g.rejects(), 0);
        let h = g.matrix().histogram();
        assert_eq!(h, [19, 0, 1]);
        // 豁免格必须挂注记，且行 2 仍满足四格通过。
        let ex = g.matrix().cell(EXEMPT_CELL);
        assert!(ex.is_some());
        if let Some(c) = ex {
            assert!(c.noted());
            assert_eq!(c.row, 2);
            assert_eq!(c.col, 2);
        }
        assert!(g.matrix().row_ok(2));
    }

    #[test]
    fn f11135_gates() {
        let mut g = MTypeLedger::new(0x4001);
        assert!(g.register_cell(1, V_PASS, 0).is_ok());
        assert_eq!(g.register_cell(1, V_PASS, 0).err(), Some(MT_ORDER));
        assert_eq!(g.register_cell(3, V_PASS, 0).err(), Some(MT_ORDER));
        assert_eq!(g.replay_crit(11, V_PASS).err(), Some(MT_CRIT));
        assert_eq!(g.replay_crit(1, V_EXEMPT).err(), Some(MT_VERDICT));
        assert_eq!(g.close_crit(1).err(), Some(MT_REPLAY));
        assert_eq!(g.close_row(0).err(), Some(MT_LEDGER));
        assert_eq!(g.rejects(), 6);
        assert!(g.freeze().is_ok());
        assert_eq!(g.register_cell(2, V_PASS, 0).err(), Some(MT_FROZEN));
        assert_eq!(g.rejects(), 7);
        // 封盘后重放账不齐，落 MT_LEDGER（格未满）。
        assert_eq!(g.replay().err(), Some(MT_LEDGER));
        assert_eq!(g.rejects(), 8);
    }

    #[test]
    fn f11135_rules() {
        assert_eq!(cell_no(0, 0), 1);
        assert_eq!(cell_no(2, 2), 13);
        assert_eq!(cell_row(20), Some(3));
        assert_eq!(cell_col(20), Some(4));
        assert_eq!(cell_row(0), None);
        assert_eq!(cell_col(21), None);
        assert_eq!(verdict_name(V_EXEMPT), "豁免");
        assert!(verdict_ok(V_FAIL));
        assert!(!verdict_ok(0));
        assert!(!verdict_ok(4));
        assert_eq!(weight_of(0), 10);
        assert_eq!(weight_of(CRITS), 0);

        // 十轮全过方可收口；一轮失败即锁死。
        let mut c = Crit::new(1);
        let mut i = 0usize;
        while i < SLOTS {
            assert!(c.replay(V_PASS).is_ok());
            i += 1;
        }
        assert_eq!(c.replay(V_PASS).err(), Some(MT_DUP));
        assert!(c.close().is_ok());
        assert_eq!(c.replay(V_PASS).err(), Some(MT_DUP));

        let mut d = Crit::new(2);
        assert!(d.replay(V_FAIL).is_ok());
        let mut k = 1usize;
        while k < SLOTS {
            assert!(d.replay(V_PASS).is_ok());
            k += 1;
        }
        assert_eq!(d.close().err(), Some(MT_REPLAY));
        assert_eq!(d.fails(), 1);
    }

    #[test]
    fn f11135_row_precedence() {
        // 两个豁免：先落 MT_EXEMPT（豁免超量），而非通过不足。
        let mut m = Matrix::new();
        let verdicts: [u8; 5] = [V_PASS, V_PASS, V_PASS, V_EXEMPT, V_EXEMPT];
        let mut i = 0usize;
        while i < 5 {
            let v = match verdicts.get(i) {
                Some(x) => *x,
                None => V_PASS,
            };
            assert!(m.register(i + 1, v, 0x77).is_ok());
            i += 1;
        }
        assert_eq!(m.close_row(0).err(), Some(MT_EXEMPT));
        // 一格失败优先于豁免计数。
        let mut n = Matrix::new();
        let v2: [u8; 5] = [V_PASS, V_PASS, V_FAIL, V_EXEMPT, V_EXEMPT];
        let mut k = 0usize;
        while k < 5 {
            let v = match v2.get(k) {
                Some(x) => *x,
                None => V_PASS,
            };
            assert!(n.register(k + 1, v, 0x78).is_ok());
            k += 1;
        }
        assert_eq!(n.close_row(0).err(), Some(MT_ROW));
    }

    #[test]
    fn f11135_injection() {
        assert_eq!(inject_four(), [true, true, true, true]);
    }

    #[test]
    fn f11135_checks_green() {
        let cs = run_unx11135_checks();
        assert!(cs.all_passed(), "checks red in unx11135");
    }
}
