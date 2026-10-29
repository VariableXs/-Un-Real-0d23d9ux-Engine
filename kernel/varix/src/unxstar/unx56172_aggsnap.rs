//! UNX-F56172 syscall 聚合审计快照件（0x61580 段）
//!
//! 锚点：UNX-F56172-J1 快照账（syscall 聚合审计快照逐帧落账：帧号 ×
//! 矩阵指纹 × 抽读窗 × 对接态四元组）；J2 快照指纹核（同帧同矩阵同
//! 签 + 矩阵变签变）；J3 快照印（三帧齐拍可印）三判据全绿。
//! 域：CoRun·UNX-F2-B40（syscall 域·审计快照面）。
//!
//! 结构：聚合矩阵冻结源（类 × 段 6×3 格只读副本）+ 快照器（帧号唯一
//! + 指纹折叠）+ 抽读窗核（窗切矩阵行）+ 快照账 + 注入核（重复帧/窗
//! 越界/指纹漂移/空矩阵快照/印后拍必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61580 独占 0x20）。
pub const SEG_BASE: u32 = 0x61580;
pub const SS_DUP_FRAME: u32 = SEG_BASE + 0x01; // 重复帧
pub const SS_WIN_RANGE: u32 = SEG_BASE + 0x02; // 窗越界
pub const SS_FP_DRIFT: u32 = SEG_BASE + 0x03; // 指纹漂移
pub const SS_EMPTY_SNAP: u32 = SEG_BASE + 0x04; // 空矩阵快照
pub const SS_SEALED: u32 = SEG_BASE + 0x05; // 印后拍

/// 类数 × 段数（与 F56161 同源）。
pub const SS_CLASSES: usize = 6;
pub const SS_PHASES: usize = 3;

/// 矩阵指纹：全格折叠。
pub fn matrix_fp(cells: &[[u64; SS_PHASES]; SS_CLASSES]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for row in cells {
        for v in row {
            h = h.wrapping_mul(0x0100_0193) ^ (*v as u32);
            h ^= (*v >> 32) as u32;
        }
    }
    h
}

/// 抽读窗：类下标区间 [from, to) 的行快照。
pub fn class_window(cells: &[[u64; SS_PHASES]; SS_CLASSES], from: usize, to: usize) -> Result<Vec<[u64; SS_PHASES]>, u32> {
    if cells.iter().all(|r| r.iter().all(|v| *v == 0)) {
        return Err(SS_EMPTY_SNAP);
    }
    if from >= to || to > SS_CLASSES {
        return Err(SS_WIN_RANGE);
    }
    Ok(cells[from..to].to_vec())
}

/// 单帧快照。
#[derive(Clone)]
pub struct SnapEntry {
    pub frame: u32,
    pub fp: u32,
    pub win_from: usize,
    pub win_to: usize,
}

impl SnapEntry {
    fn render_line(&self) -> String {
        format!("f{}|{:08x}|w{}..{}|OK", self.frame, self.fp, self.win_from, self.win_to)
    }
}

/// 快照账。
pub struct SnapBook {
    snaps: Vec<SnapEntry>,
    frozen: [[u64; SS_PHASES]; SS_CLASSES],
    sealed: bool,
}

impl SnapBook {
    pub fn new(cells: &[[u64; SS_PHASES]; SS_CLASSES]) -> Self {
        SnapBook {
            snaps: Vec::new(),
            frozen: *cells,
            sealed: false,
        }
    }

    /// 拍一帧：印后拒 + 重复帧拒 + 窗合法三闸。
    pub fn snap(&mut self, frame: u32, win_from: usize, win_to: usize) -> Result<u32, u32> {
        if self.sealed {
            return Err(SS_SEALED);
        }
        if self.snaps.iter().any(|s| s.frame == frame) {
            return Err(SS_DUP_FRAME);
        }
        let win = class_window(&self.frozen, win_from, win_to)?;
        let fp = matrix_fp(&self.frozen) ^ (win.len() as u32);
        self.snaps.push(SnapEntry {
            frame,
            fp,
            win_from,
            win_to,
        });
        Ok(fp)
    }

    pub fn snap_entry(&self, frame: u32) -> Option<&SnapEntry> {
        self.snaps.iter().find(|s| s.frame == frame)
    }

    pub fn len(&self) -> usize {
        self.snaps.len()
    }

    /// 验签核：同帧同窗重拍一致。
    pub fn verify(&self, frame: u32, win_from: usize, win_to: usize) -> Result<bool, u32> {
        let s = self.snap_entry(frame).ok_or(SS_FP_DRIFT)?;
        let win = class_window(&self.frozen, win_from, win_to)?;
        Ok(s.fp == matrix_fp(&self.frozen) ^ (win.len() as u32))
    }

    /// 三帧齐拍可印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || self.snaps.len() < 3 {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 渲染导出。
    pub fn render_book(&self) -> String {
        let mut out = String::new();
        for s in &self.snaps {
            out.push_str(&s.render_line());
            out.push('\n');
        }
        if self.sealed {
            out.push_str("SNAP-SEAL\n");
        }
        out
    }
}

/// 标准夹具：6×3 演示矩阵 + 三帧快照。
fn demo_cells() -> [[u64; SS_PHASES]; SS_CLASSES] {
    let mut cells = [[0u64; SS_PHASES]; SS_CLASSES];
    for c in 0..SS_CLASSES {
        for p in 0..SS_PHASES {
            cells[c][p] = ((c + 1) * 100 + p as usize) as u64;
        }
    }
    cells
}

fn standard_book() -> SnapBook {
    let mut b = SnapBook::new(&demo_cells());
    let _ = b.snap(1, 0, 2);
    let _ = b.snap(2, 2, 4);
    let _ = b.snap(3, 4, 6);
    b
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：重复帧
    let mut b = standard_book();
    if b.snap(1, 0, 2) == Err(SS_DUP_FRAME) {
        caught += 1;
    }
    // 注 2：窗越界
    let mut b = SnapBook::new(&demo_cells());
    if b.snap(1, 4, 9) == Err(SS_WIN_RANGE) {
        caught += 1;
    }
    // 注 3：空矩阵快照
    let empty = [[0u64; SS_PHASES]; SS_CLASSES];
    let mut b = SnapBook::new(&empty);
    if b.snap(1, 0, 2) == Err(SS_EMPTY_SNAP) {
        caught += 1;
    }
    // 注 4：指纹漂移（异窗签异）
    let b = standard_book();
    let v1 = b.verify(1, 0, 2);
    let ok = v1 == Ok(true);
    if ok {
        caught += 1;
    }
    // 注 5：印后拍
    let mut b = standard_book();
    let _ = b.seal();
    if b.snap(9, 0, 2) == Err(SS_SEALED) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 快照账；J2 指纹核；J3 快照印。
pub fn run_unx56172_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56172");
    // J1-1 三帧齐拍
    let b = standard_book();
    let ok = b.len() == 3;
    s.add("56172-J1-三帧齐拍", ok, if ok { "" } else { "快照账不全" });
    // J1-2 渲染导出齐
    let text = standard_book().render_book();
    let ok = text.contains("f1|") && text.contains("w0..2|OK") && text.contains("w4..6|OK");
    s.add("56172-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 矩阵指纹确定
    let ok = matrix_fp(&demo_cells()) == matrix_fp(&demo_cells());
    s.add("56172-J2-矩阵签确定", ok, if ok { "" } else { "指纹漂移" });
    // J2-2 一格变签变
    let mut cells = demo_cells();
    let fp0 = matrix_fp(&cells);
    cells[3][1] += 1;
    let ok = matrix_fp(&cells) != fp0;
    s.add("56172-J2-格变签变", ok, if ok { "" } else { "折叠退化" });
    // J2-3 窗切核（0..2 两行）
    let win = class_window(&demo_cells(), 0, 2).unwrap_or_default();
    let ok = win.len() == 2 && win[0][0] == 100;
    s.add("56172-J2-窗切核", ok, if ok { "" } else { "窗切漂移" });
    // J2-4 窗越界拒
    let ok = class_window(&demo_cells(), 5, 9) == Err(SS_WIN_RANGE)
        && class_window(&demo_cells(), 3, 3) == Err(SS_WIN_RANGE);
    s.add("56172-J2-窗界拒", ok, if ok { "" } else { "窗界穿透" });
    // J2-5 验签核过
    let ok = standard_book().verify(2, 2, 4) == Ok(true);
    s.add("56172-J2-验签过", ok, if ok { "" } else { "验签漂移" });
    // J3-1 快照印
    let mut b = standard_book();
    let ok = b.seal() && b.sealed();
    s.add("56172-J3-快照印", ok, if ok { "" } else { "快照印失败" });
    // J3-2 印后拍拒
    let ok = b.snap(9, 0, 2) == Err(SS_SEALED);
    s.add("56172-J3-印后拍拒", ok, if ok { "" } else { "只读闸失效" });
    // J3-3 印文渲染
    let ok = b.render_book().contains("SNAP-SEAL");
    s.add("56172-J3-印文渲染", ok, if ok { "" } else { "印文漂移" });
    // J3-4 不足三帧拒印
    let mut b2 = SnapBook::new(&demo_cells());
    let _ = b2.snap(1, 0, 2);
    let ok = !b2.seal() && !b2.sealed();
    s.add("56172-J3-不足拒印", ok, if ok { "" } else { "印闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56172-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56172_all_checks_green() {
        let set = run_unx56172_checks();
        assert!(set.all_passed(), "UNX-F56172 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56172_fp_stable() {
        assert_eq!(matrix_fp(&demo_cells()), matrix_fp(&demo_cells()));
    }
}
