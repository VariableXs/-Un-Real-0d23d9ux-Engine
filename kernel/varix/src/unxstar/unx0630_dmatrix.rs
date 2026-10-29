//! unx0630_dmatrix —— UNX-F0630 域演练总矩阵（全批演练聚合）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0630 · 域演练总矩阵（全批演练聚合） · 目标 ≥220 行
//! 判据原文摘录：演练矩阵账齐（全域注入类条目 ≥40 归集——类/批/
//!   判据三列），矩阵行数=归集数。
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 三列定表：演练类（闭集 5 类）/ 来源批（B13..B31）/ 判据号
//!      三列，一条目一行，行号连续零跳号。
//!   2. 归集下限：注入类条目 ≥40（判据原文），少于 40 不许封矩阵。
//!   3. 矩阵行数 = 归集数恒等式：矩阵行数必须与归集账行数逐次相等，
//!      两账脱节即红。
//!   4. 类闭集：miss注入/重放注入/漂移注入/截断注入/乱序注入 五类
//!      之外拒收。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!      确定性。
//!
//! 诊断段声明：SEG 0x5080 段位本模块独占。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x5080 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x5080;
/// 诊断段顶。
pub const SEG_TOP: u16 = 0x508F;

/// XcCode：演练类闭集外。
pub const XC_BAD_KIND: u16 = SEG_BASE + 1; // 0x5081 五类之外
/// XcCode：来源批越界。
pub const XC_BAD_BATCH: u16 = SEG_BASE + 2; // 0x5082 批号 0..19 闭集外
/// XcCode：判据号为零。
pub const XC_BAD_CRIT: u16 = SEG_BASE + 3; // 0x5083 判据号必须非零
/// XcCode：归集不足。
pub const XC_COLLECT_SHORT: u16 = SEG_BASE + 4; // 0x5084 <40 条
/// XcCode：行号跳号。
pub const XC_ROW_GAP: u16 = SEG_BASE + 5; // 0x5085 行号不连续
/// XcCode：矩阵行数 ≠ 归集数。
pub const XC_ROW_MISMATCH: u16 = SEG_BASE + 6; // 0x5086 两账脱节

/// 注入类条目下限（判据原文 ≥40）。
pub const COLLECT_MIN: usize = 40;
/// 来源批数（B13..B31）。
pub const BATCH_N: usize = 19;

/// 演练类闭集（五类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrillKind {
    /// 缺失注入。
    Miss = 0,
    /// 重放注入。
    Replay = 1,
    /// 漂移注入。
    Drift = 2,
    /// 截断注入。
    Trunc = 3,
    /// 乱序注入。
    Reorder = 4,
}

impl DrillKind {
    /// 类名。
    pub fn name(self) -> &'static str {
        match self {
            DrillKind::Miss => "缺失注入",
            DrillKind::Replay => "重放注入",
            DrillKind::Drift => "漂移注入",
            DrillKind::Trunc => "截断注入",
            DrillKind::Reorder => "乱序注入",
        }
    }
}

/// 矩阵一行（三列：类/批/判据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrillRow {
    /// 行号（0 起连续）。
    pub row_no: usize,
    /// 演练类。
    pub kind: DrillKind,
    /// 来源批号（0..19）。
    pub batch: usize,
    /// 判据号（非零）。
    pub crit_id: u16,
}

/// 演练总矩阵。
#[derive(Clone, Debug, Default)]
pub struct DrillMatrix {
    /// 矩阵行。
    rows: Vec<DrillRow>,
    /// 已封矩阵位。
    sealed: bool,
}

impl DrillMatrix {
    /// 空矩阵。
    pub fn new() -> Self {
        DrillMatrix {
            rows: Vec::new(),
            sealed: false,
        }
    }

    /// 当前行数。
    pub fn row_n(&self) -> usize {
        self.rows.len()
    }

    /// 归集一条（类闭集 + 批闭集 + 判据非零 + 行号连续 + 封后拒写）。
    pub fn collect(&mut self, kind: DrillKind, batch: usize, crit_id: u16) -> Result<usize, u16> {
        if self.sealed {
            return Err(XC_ROW_MISMATCH);
        }
        if kind as usize > DrillKind::Reorder as usize {
            return Err(XC_BAD_KIND);
        }
        if batch >= BATCH_N {
            return Err(XC_BAD_BATCH);
        }
        if crit_id == 0 {
            return Err(XC_BAD_CRIT);
        }
        let row_no = self.rows.len();
        self.rows.push(DrillRow {
            row_no,
            kind,
            batch,
            crit_id,
        });
        Ok(row_no)
    }

    /// 行号连续零跳号核。
    pub fn rows_continuous(&self) -> Result<usize, u16> {
        for (i, r) in self.rows.iter().enumerate() {
            if r.row_no != i {
                return Err(XC_ROW_GAP);
            }
        }
        Ok(self.rows.len())
    }

    /// 归集数（= 矩阵行数，恒等面）。
    pub fn collected_n(&self) -> usize {
        self.rows.len()
    }

    /// 矩阵行数 = 归集数核（两账同源，此核防未来分账漂移）。
    pub fn row_eq_collect(&self) -> Result<usize, u16> {
        if self.row_n() != self.collected_n() {
            return Err(XC_ROW_MISMATCH);
        }
        self.rows_continuous()?;
        Ok(self.row_n())
    }

    /// 归集下限核：≥40。
    pub fn collect_min_ok(&self) -> Result<usize, u16> {
        if self.row_n() < COLLECT_MIN {
            return Err(XC_COLLECT_SHORT);
        }
        Ok(self.row_n())
    }

    /// 封矩阵：下限 + 连续 + 两账一致三前置全过才封；封后只读。
    pub fn seal(&mut self) -> Result<usize, u16> {
        if self.sealed {
            return Err(XC_ROW_MISMATCH);
        }
        let n = self.collect_min_ok()?;
        self.row_eq_collect()?;
        self.sealed = true;
        Ok(n)
    }

    /// 类分布（五类计数，确定性序）。
    pub fn kind_hist(&self) -> [usize; 5] {
        let mut h = [0usize; 5];
        for r in &self.rows {
            h[r.kind as usize] += 1;
        }
        h
    }

    /// 人话报告。
    pub fn report(&self) -> String {
        let h = self.kind_hist();
        let mut s = alloc::format!(
            "演练总矩阵 · 行数 {} · 封矩阵={} · 类分布 [{},{},{},{},{}]",
            self.row_n(),
            self.sealed,
            h[0],
            h[1],
            h[2],
            h[3],
            h[4]
        );
        for r in self.rows.iter().take(8) {
            s.push_str(&alloc::format!(
                "\n  行{} {} 批{} 判据F0{}",
                r.row_no, r.kind.name(), r.batch, r.crit_id
            ));
        }
        s
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in self.report().as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

/// 标准矩阵构造器：42 行（≥40 下限），行 i：类 = i%5 映射五类，
/// 批 = i%19，判据号 = 300 + i（确定性）。
pub fn build_standard_matrix() -> Result<DrillMatrix, u16> {
    let mut m = DrillMatrix::new();
    let kinds = [
        DrillKind::Miss,
        DrillKind::Replay,
        DrillKind::Drift,
        DrillKind::Trunc,
        DrillKind::Reorder,
    ];
    for i in 0..42 {
        m.collect(kinds[i % 5], i % BATCH_N, 300 + i as u16)?;
    }
    m.seal()?;
    Ok(m)
}

/// 自例程：主路径（42 行→封矩阵→分布核）+ 错误路径（类外/批外/零判据/不足/跳号/封后写）。
pub fn drill() -> Result<u32, u16> {
    // 主路径。
    let m = build_standard_matrix()?;
    if m.row_eq_collect()? != 42 {
        return Err(XC_ROW_MISMATCH);
    }
    if m.collect_min_ok()? < COLLECT_MIN {
        return Err(XC_COLLECT_SHORT);
    }
    let h = m.kind_hist();
    // 42 行五类分布 = [9,9,9,8,8]（i%5 循环，确定性）。
    if h != [9, 9, 9, 8, 8] {
        return Err(XC_BAD_KIND);
    }
    if m.fingerprint() == 0 {
        return Err(XC_ROW_MISMATCH);
    }

    // 错误路径 1：判据零值（类闭集外的构造面在本 no_std 工位不可达，
    // 以"判据号为零"作为同位注入红检出点）。
    let mut m1 = DrillMatrix::new();
    if m1.collect(DrillKind::Miss, 0, 0) != Err(XC_BAD_CRIT) {
        return Err(XC_BAD_CRIT);
    }
    // 错误路径 2：来源批越界。
    let mut m2 = DrillMatrix::new();
    if m2.collect(DrillKind::Miss, BATCH_N, 1) != Err(XC_BAD_BATCH) {
        return Err(XC_BAD_BATCH);
    }
    // 错误路径 3：归集不足封矩阵。
    let mut m3 = DrillMatrix::new();
    for i in 0..COLLECT_MIN - 1 {
        m3.collect(DrillKind::Miss, i % BATCH_N, 300 + i as u16)?;
    }
    if m3.seal() != Err(XC_COLLECT_SHORT) {
        return Err(XC_COLLECT_SHORT);
    }
    // 错误路径 4：行号跳号（手工篡改行号）。
    let mut m4 = build_standard_matrix()?;
    m4.rows[3].row_no = 99;
    if m4.rows_continuous() != Err(XC_ROW_GAP) {
        return Err(XC_ROW_GAP);
    }
    if m4.row_eq_collect() != Err(XC_ROW_GAP) {
        return Err(XC_ROW_GAP);
    }
    // 错误路径 5：封后写拦截。
    let mut m5 = build_standard_matrix()?;
    if m5.collect(DrillKind::Miss, 0, 1) != Err(XC_ROW_MISMATCH) {
        return Err(XC_ROW_MISMATCH);
    }
    Ok(0)
}
