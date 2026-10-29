//! UNX-F56197 启动方差深化账件（13 子段方差矩阵）（0x618A0 段）
//!
//! 锚点：UNX-F56197-J1 方差深化账（13 子段方差矩阵——子段 × 样本序
//! 列 × 极差 × 在阈判定逐格落账）；J2 方差语义（矩阵行闭集 + 极差口
//! 径守恒 + 在阈判定三性质）；J3 方差指纹（矩阵折叠指纹）三判据全
//! 绿。
//! 域：CoRun·UNX-F2-B40（启动域·方差深化面）。
//!
//! 结构：13 子段矩阵（四段内 4+4+4+1 汇总位）+ 方差核（行闭集闸 +
//! 极差闸 + 在阈闸）+ 方差指纹 + 注入核（空序列/单点序列/未知子段/
//! 超阈判定错/印后写入必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x618A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x618A0;
pub const VD_EMPTY_SERIES: u32 = SEG_BASE + 0x01; // 空序列
pub const VD_SINGLE_POINT: u32 = SEG_BASE + 0x02; // 单点序列
pub const VD_UNK_CELL: u32 = SEG_BASE + 0x03; // 未知子段格
pub const VD_THRESH_MISS: u32 = SEG_BASE + 0x04; // 超阈判定错
pub const VD_FROZEN: u32 = SEG_BASE + 0x05; // 印后写入

/// 13 子段闭集（12 子段 + 1 全链汇总位）。
pub const CELL_COUNT: usize = 13;

pub const CELL_NAMES: [&str; CELL_COUNT] = [
    "fwpost", "load", "menu", "handoff", "init", "drv", "svc", "sched",
    "sesmgr", "compos", "logingui", "cred", "grand",
];

/// 单格方差账。
#[derive(Clone)]
pub struct VarCell {
    pub cell: usize,
    pub series: Vec<u64>,
}

impl VarCell {
    /// 极差 = max - min。
    pub fn range(&self) -> u64 {
        if self.series.len() < 2 {
            return 0;
        }
        let lo = *self.series.iter().min().unwrap();
        let hi = *self.series.iter().max().unwrap();
        hi - lo
    }

    /// 均值。
    pub fn mean(&self) -> u64 {
        if self.series.is_empty() {
            return 0;
        }
        self.series.iter().sum::<u64>() / self.series.len() as u64
    }

    /// 序列合格：≥2 点（单点序列无方差可言）。
    pub fn series_ok(&self) -> bool {
        self.series.len() >= 2
    }

    /// 在阈判定。
    pub fn within(&self, threshold: u64) -> bool {
        self.range() <= threshold
    }
}

/// 方差深化账（矩阵）。
pub struct VarMatrix {
    cells: Vec<VarCell>,
    threshold: u64,
    sealed: bool,
}

impl VarMatrix {
    pub fn new(threshold: u64) -> Self {
        VarMatrix {
            cells: Vec::new(),
            threshold,
            sealed: false,
        }
    }

    /// 写入一格：格闸 + 序列闸 + 印后拒（同格覆盖为重写，允许更新）。
    pub fn put(&mut self, cell: usize, series: Vec<u64>) -> Result<(), u32> {
        if self.sealed {
            return Err(VD_FROZEN);
        }
        if cell >= CELL_COUNT {
            return Err(VD_UNK_CELL);
        }
        if series.is_empty() {
            return Err(VD_EMPTY_SERIES);
        }
        if series.len() < 2 {
            return Err(VD_SINGLE_POINT);
        }
        if let Some(c) = self.cells.iter_mut().find(|c| c.cell == cell) {
            c.series = series;
            return Ok(());
        }
        self.cells.push(VarCell { cell, series });
        Ok(())
    }

    pub fn cell(&self, idx: usize) -> Option<&VarCell> {
        self.cells.iter().find(|c| c.cell == idx)
    }

    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// 覆盖核：13 格全写。
    pub fn covers_all(&self) -> bool {
        (0..CELL_COUNT).all(|c| self.cell(c).is_some())
    }

    /// 全格在阈。
    pub fn all_within(&self) -> bool {
        self.cells.iter().all(|c| c.within(self.threshold))
    }

    /// 最差格（极差最大）。
    pub fn worst_cell(&self) -> Option<usize> {
        self.cells
            .iter()
            .max_by_key(|c| c.range())
            .map(|c| c.cell)
    }

    /// 方差指纹。
    pub fn matrix_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for c in &self.cells {
            h = h.wrapping_mul(0x0100_0193) ^ c.cell as u32;
            for v in &c.series {
                h = h.wrapping_mul(0x0100_0193) ^ (*v as u32);
                h ^= (v >> 32) as u32;
            }
        }
        h = h.wrapping_mul(0x0100_0193) ^ self.threshold as u32;
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 封账印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || !self.covers_all() || !self.all_within() {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 渲染导出。
    pub fn render_matrix(&self) -> String {
        let mut out = String::new();
        for c in &self.cells {
            out.push_str(&format!(
                "{}|n={}|mean={}|range={}\n",
                CELL_NAMES[c.cell],
                c.series.len(),
                c.mean(),
                c.range()
            ));
        }
        if self.sealed {
            out.push_str("VARMAT-SEAL\n");
        }
        out
    }
}

/// 标准夹具：13 格全写（每格 3 点，全部在阈 40 内）。
fn standard_matrix() -> VarMatrix {
    let mut m = VarMatrix::new(40);
    // 12 子段：三样本微扰；grand 为四段和的三样本。
    let cells: [(usize, [u64; 3]); 13] = [
        (0, [150, 155, 148]),
        (1, [370, 375, 365]),
        (2, [240, 236, 244]),
        (3, [40, 42, 39]),
        (4, [300, 305, 295]),
        (5, [1000, 1008, 995]),
        (6, [400, 396, 405]),
        (7, [100, 102, 98]),
        (8, [400, 404, 395]),
        (9, [1100, 1105, 1095]),
        (10, [700, 704, 695]),
        (11, [400, 396, 404]),
        (12, [6000, 6008, 5990]),
    ];
    for (c, s) in cells {
        let _ = m.put(c, s.to_vec());
    }
    m
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：空序列
    let mut m = VarMatrix::new(40);
    if m.put(0, Vec::new()) == Err(VD_EMPTY_SERIES) {
        caught += 1;
    }
    // 注 2：单点序列
    let mut m = VarMatrix::new(40);
    if m.put(0, vec![100]) == Err(VD_SINGLE_POINT) {
        caught += 1;
    }
    // 注 3：未知格
    let mut m = VarMatrix::new(40);
    if m.put(13, vec![1, 2]) == Err(VD_UNK_CELL) {
        caught += 1;
    }
    // 注 4：超阈封印拒
    let mut m = VarMatrix::new(10);
    let _ = m.put(0, vec![100, 200]); // 极差 100 > 10
    if !m.seal() && !m.sealed() {
        caught += 1;
    }
    // 注 5：印后写入
    let mut m = standard_matrix();
    let _ = m.seal();
    if m.put(0, vec![1, 2]) == Err(VD_FROZEN) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 方差深化账；J2 方差语义；J3 方差指纹。
pub fn run_unx56197_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56197");
    // J1-1 13 格全写
    let m = standard_matrix();
    let ok = m.len() == CELL_COUNT && m.covers_all();
    s.add("56197-J1-13格全写", ok, if ok { "" } else { "矩阵不全" });
    // J1-2 渲染导出齐
    let mut m2 = standard_matrix();
    let _ = m2.seal();
    let text = m2.render_matrix();
    let ok = text.contains("fwpost|n=3|mean=151|range=7")
        && text.contains("grand|n=3|mean=5999|range=18")
        && text.contains("VARMAT-SEAL");
    s.add("56197-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 行闭集（13 格全在闭集）
    let ok = (0..CELL_COUNT).all(|c| m.cell(c).is_some());
    s.add("56197-J2-行闭集", ok, if ok { "" } else { "行越界" });
    // J2-2 极差口径守恒（grand: 6008-5990 = 18）
    let ok = m.cell(12).map(|c| c.range() == 18).unwrap_or(false);
    s.add("56197-J2-极差口径", ok, if ok { "" } else { "极差漂移" });
    // J2-3 全格在阈
    let ok = m.all_within();
    s.add("56197-J2-全格在阈", ok, if ok { "" } else { "超阈" });
    // J2-4 均值抽查（drv: (1000+1008+995)/3 = 1001）
    let ok = m.cell(5).map(|c| c.mean() == 1_001).unwrap_or(false);
    s.add("56197-J2-均值抽查", ok, if ok { "" } else { "均值漂移" });
    // J2-5 最差格 = grand（range 18 全矩阵最大）
    let ok = m.worst_cell() == Some(12);
    s.add("56197-J2-最差格", ok, if ok { "" } else { "最差格漂移" });
    // J2-6 单点序列拒
    let mut m3 = standard_matrix();
    let ok = m3.put(0, vec![100]) == Err(VD_SINGLE_POINT);
    s.add("56197-J2-单点拒", ok, if ok { "" } else { "单点闸失效" });
    // J3-1 矩阵指纹确定
    let ok = standard_matrix().matrix_fp() == standard_matrix().matrix_fp();
    s.add("56197-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 序列变签变
    let mut m4 = standard_matrix();
    let fp0 = m4.matrix_fp();
    if let Some(c) = m4.cells.first_mut() {
        c.series[0] += 1;
    }
    let ok = m4.matrix_fp() != fp0;
    s.add("56197-J3-序列变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 覆盖更新同格（幂等改写合法）
    let mut m5 = standard_matrix();
    let ok = m5.put(0, vec![150, 155, 148]).is_ok() && m5.len() == CELL_COUNT;
    s.add("56197-J3-同格覆盖", ok, if ok { "" } else { "覆盖漂移" });
    // J3-4 封账印
    let mut m6 = standard_matrix();
    let ok = m6.seal() && m6.sealed();
    s.add("56197-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56197-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56197_all_checks_green() {
        let set = run_unx56197_checks();
        assert!(set.all_passed(), "UNX-F56197 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56197_matrix_stable() {
        assert!(standard_matrix().all_within());
    }
}
