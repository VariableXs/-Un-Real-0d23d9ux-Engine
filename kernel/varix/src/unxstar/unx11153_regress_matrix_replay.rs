//! UNX-F11153 B34 清账（回归矩阵判据终重放）（0x1f350 段）
//!
//! 纯功能模型：B34 批以回归矩阵落账——五条用例 × 四个目标平台共二十格，格值
//! 分绿/红/跳三值。终重放要求矩阵五行齐、二十格全绿、红格与跳格皆为零；矩阵
//! 终值冻结后 B34 不再补格。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f350;
/// 用例号越界（只认 0..5）
pub const RM_ENO: u32 = SEG_BASE + 0x01;
/// 用例重记
pub const RM_EDUP: u32 = SEG_BASE + 0x02;
/// 用例序倒挂
pub const RM_EORDER: u32 = SEG_BASE + 0x03;
/// 格值越出三值（形状违规）
pub const RM_ECELL: u32 = SEG_BASE + 0x04;
/// 五行未齐即清账
pub const RM_ESHORT: u32 = SEG_BASE + 0x05;
/// 矩阵存在红格
pub const RM_ERED: u32 = SEG_BASE + 0x06;
/// 矩阵存在跳格
pub const RM_ESKIP: u32 = SEG_BASE + 0x07;
/// 终值已锁定
pub const RM_EFROZEN: u32 = SEG_BASE + 0x08;

/// 用例行数。
pub const ROWS: usize = 5;
/// 目标平台列数。
pub const TARGETS: usize = 4;
/// 矩阵格数。
pub const CELLS: usize = ROWS * TARGETS;
/// 格值：绿。
pub const CELL_GREEN: u8 = 0;
/// 格值：红。
pub const CELL_RED: u8 = 1;
/// 格值：跳。
pub const CELL_SKIP: u8 = 2;
/// 格值域宽度。
pub const CELL_VALUES: u8 = 3;
/// 目标平台名。
pub const TARGET_NAMES: [&str; TARGETS] = ["x86_64", "aarch64", "riscv64", "arm32"];

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一行回归结果（四目标）。
#[derive(Clone, Copy, Debug)]
pub struct MatrixRow {
    pub case: u8,
    pub cells: [u8; TARGETS],
}

impl MatrixRow {
    /// 本行是否四格全绿。
    pub fn all_green(&self) -> bool {
        let mut i = 0usize;
        while i < TARGETS {
            if self.cells[i] != CELL_GREEN {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
}

/// B34 回归矩阵账。
#[derive(Clone, Debug)]
pub struct MatrixLedger {
    rows: Vec<MatrixRow>,
    settled: Option<u64>,
    frozen: bool,
    pub accepts: u64,
    pub rejects: u64,
}

impl MatrixLedger {
    /// 空账。
    pub fn new() -> MatrixLedger {
        MatrixLedger {
            rows: Vec::new(),
            settled: None,
            frozen: false,
            accepts: 0,
            rejects: 0,
        }
    }

    /// 已落行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 取某用例行。
    pub fn row(&self, case: usize) -> Option<MatrixRow> {
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].case as usize == case {
                return Some(self.rows[i]);
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 落一行：冻结 → 越界 → 重记 → 倒序 → 格值形状。
    ///
    /// 重记先于倒序：同一用例重落只报 RM_EDUP；格值越界单独报 RM_ECELL。
    pub fn record(&mut self, case: usize, cells: [u8; TARGETS]) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(RM_EFROZEN);
        }
        if case >= ROWS {
            return self.fail(RM_ENO);
        }
        if self.row(case).is_some() {
            return self.fail(RM_EDUP);
        }
        if let Some(last) = self.rows.last() {
            if case <= last.case as usize {
                return self.fail(RM_EORDER);
            }
        }
        let mut i = 0usize;
        while i < TARGETS {
            if cells[i] >= CELL_VALUES {
                return self.fail(RM_ECELL);
            }
            i = i.wrapping_add(1);
        }
        self.rows.push(MatrixRow {
            case: case as u8,
            cells,
        });
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> Result<u64, u32> {
        self.rejects = self.rejects.wrapping_add(1);
        Err(code)
    }

    /// 格值计数。
    pub fn count_cell(&self, value: u8) -> u32 {
        let mut n = 0u32;
        let mut r = 0usize;
        while r < self.rows.len() {
            let mut c = 0usize;
            while c < TARGETS {
                if self.rows[r].cells[c] == value {
                    n = n.wrapping_add(1);
                }
                c = c.wrapping_add(1);
            }
            r = r.wrapping_add(1);
        }
        n
    }

    /// 已落格数。
    pub fn filled_cells(&self) -> u32 {
        (self.rows.len() as u32).wrapping_mul(TARGETS as u32)
    }

    /// 缺行表。
    pub fn gaps(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut c = 0usize;
        while c < ROWS {
            if self.row(c).is_none() {
                out.push(c as u32);
            }
            c = c.wrapping_add(1);
        }
        out
    }

    /// 行序严格递增。
    pub fn order_strict(&self) -> bool {
        let mut i = 1usize;
        while i < self.rows.len() {
            if self.rows[i].case <= self.rows[i - 1].case {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 是否每行全绿。
    pub fn all_rows_green(&self) -> bool {
        let mut i = 0usize;
        while i < self.rows.len() {
            if !self.rows[i].all_green() {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 矩阵指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut r = 0usize;
        while r < self.rows.len() {
            h = (h ^ self.rows[r].case as u64).wrapping_mul(FNV_PRIME);
            let mut c = 0usize;
            while c < TARGETS {
                h = (h ^ self.rows[r].cells[c] as u64).wrapping_mul(FNV_PRIME);
                c = c.wrapping_add(1);
            }
            r = r.wrapping_add(1);
        }
        h = (h ^ self.rows.len() as u64).wrapping_mul(FNV_PRIME);
        h
    }

    /// 终清账：五行齐 → 无红格 → 无跳格 → 二十格落满。
    pub fn settle(&mut self) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(RM_EFROZEN);
        }
        if self.rows.len() != ROWS || !self.gaps().is_empty() {
            return self.fail(RM_ESHORT);
        }
        if self.count_cell(CELL_RED) != 0 {
            return self.fail(RM_ERED);
        }
        if self.count_cell(CELL_SKIP) != 0 {
            return self.fail(RM_ESKIP);
        }
        if self.filled_cells() != CELLS as u32 {
            return self.fail(RM_ESHORT);
        }
        let v = self.fold();
        self.settled = Some(v);
        Ok(v)
    }

    /// 冻结：锁当前值，幂等。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.settled;
        }
        if self.settled.is_none() {
            self.settled = Some(self.fold());
        }
        self.frozen = true;
        self.settled
    }

    /// 终值。
    pub fn settled_value(&self) -> Option<u64> {
        self.settled
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 建账：五行 × 四目标全绿，再清账。
pub fn build_b34() -> (MatrixLedger, bool) {
    let mut l = MatrixLedger::new();
    let mut c = 0usize;
    while c < ROWS {
        if l.record(c, [CELL_GREEN; TARGETS]).is_err() {
            return (l, false);
        }
        c = c.wrapping_add(1);
    }
    let ok = l.settle().is_ok();
    (l, ok)
}

/// 四类注入：用例越界 / 重记与倒序 / 格值越界 / 终账后再落。
pub fn inject_four() -> [bool; 4] {
    // ① 用例号越界
    let mut a0 = MatrixLedger::new();
    let r0 = a0.record(ROWS, [CELL_GREEN; TARGETS]).err() == Some(RM_ENO)
        && a0
            .record(ROWS.wrapping_add(3), [CELL_GREEN; TARGETS])
            .err()
            == Some(RM_ENO)
        && a0.is_empty();

    // ② 重记与倒序
    let mut a1 = MatrixLedger::new();
    let s0 = a1.record(0, [CELL_GREEN; TARGETS]).is_ok();
    let s1 = a1.record(1, [CELL_GREEN; TARGETS]).is_ok();
    let dup = s0 && s1 && a1.record(0, [CELL_GREEN; TARGETS]).err() == Some(RM_EDUP);
    let mut a2 = MatrixLedger::new();
    let _ = a2.record(3, [CELL_GREEN; TARGETS]);
    let order = a2.record(2, [CELL_GREEN; TARGETS]).err() == Some(RM_EORDER);
    let r1 = dup && order;

    // ③ 格值越界
    let mut a3 = MatrixLedger::new();
    let r2 = a3
        .record(0, [CELL_GREEN, CELL_GREEN, 3, CELL_GREEN])
        .err()
        == Some(RM_ECELL)
        && a3.record(0, [9, 9, 9, 9]).err() == Some(RM_ECELL)
        && a3.is_empty();

    // ④ 终账后再落 / 再清账
    let (mut a4, ok4) = build_b34();
    let locked = a4.freeze().is_some();
    let r3 = ok4
        && locked
        && a4.record(0, [CELL_GREEN; TARGETS]).err() == Some(RM_EFROZEN)
        && a4.settle().err() == Some(RM_EFROZEN);

    [r0, r1, r2, r3]
}

/// 判据：UNX-F11153 B34 回归矩阵判据终重放。
pub fn run_unx11153_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11153");
    let (mut l, ok) = build_b34();

    // X1 五行齐备
    s.add_dyn(
        "X1-五行齐备",
        ok && l.len() == ROWS && l.gaps().is_empty(),
        &format!("rows={}/{} gaps={}", l.len(), ROWS, l.gaps().len()),
    );

    // X2 二十格落满
    s.add_dyn(
        "X2-二十格落满",
        l.filled_cells() == CELLS as u32,
        &format!("cells={}/{}", l.filled_cells(), CELLS),
    );

    // X3 全绿
    s.add_dyn(
        "X3-矩阵全绿",
        l.all_rows_green() && l.count_cell(CELL_GREEN) == CELLS as u32,
        &format!("green={}/{}", l.count_cell(CELL_GREEN), CELLS),
    );

    // X4 零红格
    s.add("X4-零红格", l.count_cell(CELL_RED) == 0, "回归矩阵不得留红格");

    // X5 零跳格
    s.add("X5-零跳格", l.count_cell(CELL_SKIP) == 0, "回归矩阵不得留跳格");

    // X6 行序严格
    s.add("X6-行序严格", l.order_strict(), "五行按 0..4 递增落账");

    // X7 终值即指纹
    let root = l.settled_value();
    s.add("X7-终值即指纹", root == Some(l.fold()), "矩阵清单值等于逐格指纹");

    // X8 冻结幂等
    let f1 = l.freeze();
    let f2 = l.freeze();
    s.add("X8-冻结幂等", f1.is_some() && f1 == f2, "二次冻结返回同一单值");

    // X9 冻结后拒落拒清
    s.add(
        "X9-冻结后拒落拒清",
        l.is_frozen()
            && l.record(0, [CELL_GREEN; TARGETS]).err() == Some(RM_EFROZEN)
            && l.settle().err() == Some(RM_EFROZEN),
        "冻结后 record/settle 都落 RM_EFROZEN",
    );

    // X10 用例越界拒止
    let mut g = MatrixLedger::new();
    s.add(
        "X10-用例越界拒止",
        g.record(ROWS, [CELL_GREEN; TARGETS]).err() == Some(RM_ENO)
            && g.record(ROWS.wrapping_add(2), [CELL_GREEN; TARGETS])
                .err()
                == Some(RM_ENO)
            && g.is_empty(),
        "0..5 之外的用例号一律 RM_ENO",
    );

    // X11 重记先于倒序
    let mut d = MatrixLedger::new();
    let _ = d.record(0, [CELL_GREEN; TARGETS]);
    let _ = d.record(1, [CELL_GREEN; TARGETS]);
    let mut o = MatrixLedger::new();
    let _ = o.record(3, [CELL_GREEN; TARGETS]);
    s.add(
        "X11-重记先于倒序",
        d.record(0, [CELL_GREEN; TARGETS]).err() == Some(RM_EDUP)
            && o.record(2, [CELL_GREEN; TARGETS]).err() == Some(RM_EORDER),
        "同用例行重落报 EDUP，真倒序才报 EORDER",
    );

    // X12 格值越界拒止
    let mut sh = MatrixLedger::new();
    s.add(
        "X12-格值越界拒止",
        sh.record(0, [CELL_GREEN, 3, CELL_GREEN, CELL_GREEN])
            .err()
            == Some(RM_ECELL)
            && sh.record(0, [9, 9, 9, 9]).err() == Some(RM_ECELL)
            && sh.is_empty(),
        "格值 ≥3 一律 RM_ECELL",
    );

    // X13 短账 / 红格 / 跳格 与注入
    let mut q1 = MatrixLedger::new();
    let mut c = 0usize;
    while c < ROWS - 1 {
        let _ = q1.record(c, [CELL_GREEN; TARGETS]);
        c = c.wrapping_add(1);
    }
    let mut q2 = MatrixLedger::new();
    let mut c = 0usize;
    while c < ROWS {
        let cells = if c == 2 {
            [CELL_GREEN, CELL_RED, CELL_GREEN, CELL_GREEN]
        } else {
            [CELL_GREEN; TARGETS]
        };
        let _ = q2.record(c, cells);
        c = c.wrapping_add(1);
    }
    let mut q3 = MatrixLedger::new();
    let mut c = 0usize;
    while c < ROWS {
        let cells = if c == 4 {
            [CELL_SKIP, CELL_GREEN, CELL_GREEN, CELL_GREEN]
        } else {
            [CELL_GREEN; TARGETS]
        };
        let _ = q3.record(c, cells);
        c = c.wrapping_add(1);
    }
    let hits = inject_four();
    let all = hits[0] && hits[1] && hits[2] && hits[3];
    let (mut fresh, ok2) = build_b34();
    let root2 = fresh.freeze().unwrap_or(0);
    s.add_dyn(
        "X13-三拒与注入四拒",
        q1.settle().err() == Some(RM_ESHORT)
            && q2.settle().err() == Some(RM_ERED)
            && q3.settle().err() == Some(RM_ESKIP)
            && all
            && ok2
            && fresh.accepts == ROWS as u64
            && fresh.rejects == 0
            && root2 != 0,
        &format!(
            "inject={}{}{}{} acc={} rej={} root={:#x}",
            hits[0] as u8, hits[1] as u8, hits[2] as u8, hits[3] as u8,
            fresh.accepts, fresh.rejects, root2
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11153_flow() {
        let (mut l, ok) = build_b34();
        assert!(ok);
        assert_eq!(l.len(), ROWS);
        assert_eq!(l.filled_cells(), CELLS as u32);
        assert!(l.all_rows_green());
        assert!(l.order_strict());
        assert_eq!(l.count_cell(CELL_RED), 0);
        let root = l.settled_value();
        assert_eq!(root, Some(l.fold()));
        assert_eq!(l.freeze(), root);
        assert_eq!(l.freeze(), root);
        assert!(l.is_frozen());
        assert_eq!(l.record(0, [CELL_GREEN; TARGETS]), Err(RM_EFROZEN));
        assert_eq!(l.settle(), Err(RM_EFROZEN));
    }

    #[test]
    fn f11153_gates() {
        let mut l = MatrixLedger::new();
        assert!(l.record(0, [CELL_GREEN; TARGETS]).is_ok());
        assert!(l.record(1, [CELL_GREEN; TARGETS]).is_ok());
        assert_eq!((l.accepts, l.rejects), (2, 0));
        assert_eq!(l.record(ROWS, [CELL_GREEN; TARGETS]), Err(RM_ENO));
        assert_eq!(l.record(0, [CELL_GREEN; TARGETS]), Err(RM_EDUP));
        assert_eq!(l.record(1, [9, 9, 9, 9]), Err(RM_EDUP));
        assert_eq!((l.accepts, l.rejects), (2, 3));

        let mut sh = MatrixLedger::new();
        assert_eq!(
            sh.record(0, [CELL_GREEN, 3, CELL_GREEN, CELL_GREEN]),
            Err(RM_ECELL)
        );
        assert_eq!(sh.record(0, [9, 9, 9, 9]), Err(RM_ECELL));
        assert_eq!((sh.accepts, sh.rejects), (0, 2));

        let mut o = MatrixLedger::new();
        let _ = o.record(3, [CELL_GREEN; TARGETS]);
        assert_eq!(o.record(2, [CELL_GREEN; TARGETS]), Err(RM_EORDER));
        assert_eq!((o.accepts, o.rejects), (1, 1));

        let mut q = MatrixLedger::new();
        let mut c = 0usize;
        while c < ROWS - 1 {
            let _ = q.record(c, [CELL_GREEN; TARGETS]);
            c = c.wrapping_add(1);
        }
        assert_eq!(q.settle(), Err(RM_ESHORT));
        assert_eq!((q.accepts, q.rejects), (4, 1));

        let mut r = MatrixLedger::new();
        let mut c = 0usize;
        while c < ROWS {
            let cells = if c == 1 {
                [CELL_RED, CELL_GREEN, CELL_GREEN, CELL_GREEN]
            } else {
                [CELL_GREEN; TARGETS]
            };
            let _ = r.record(c, cells);
            c = c.wrapping_add(1);
        }
        assert_eq!(r.settle(), Err(RM_ERED));
        assert_eq!((r.accepts, r.rejects), (5, 1));

        let mut k = MatrixLedger::new();
        let mut c = 0usize;
        while c < ROWS {
            let cells = if c == 4 {
                [CELL_SKIP, CELL_GREEN, CELL_GREEN, CELL_GREEN]
            } else {
                [CELL_GREEN; TARGETS]
            };
            let _ = k.record(c, cells);
            c = c.wrapping_add(1);
        }
        assert_eq!(k.settle(), Err(RM_ESKIP));
        assert_eq!((k.accepts, k.rejects), (5, 1));

        let (mut d, ok) = build_b34();
        assert!(ok);
        assert!(d.freeze().is_some());
        assert_eq!(d.record(0, [CELL_GREEN; TARGETS]), Err(RM_EFROZEN));
        assert_eq!(d.settle(), Err(RM_EFROZEN));
        assert_eq!((d.accepts, d.rejects), (5, 2));
    }

    #[test]
    fn f11153_checks_green() {
        assert!(run_unx11153_checks().all_passed());
    }
}
