//! UNX-F11061 回归矩阵场搭建（五族矩阵账与执行序）（0x1e7a0 段）
//!
//! 回归矩阵场：五族（pipe/signal/pty/sysv/posix）× 六格（地基/机制/
//! 错误矩阵/联测/性能/常态）构成 30 格回归矩阵。逐格登记执行序
//! （序号单调），逐格重放裁断；守恒式：run == cells（全格执行），
//! 执行序严格递增（序账零倒置），族账 6/6 齐备。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11061 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1e000 + (11061 - 11000) * 0x20;

/// 矩阵空 —— 执行前必须先立格
pub const RM_EMPTY: u32 = SEG_BASE + 0x01;
/// 格号越界 —— 30 格编号 0..30
pub const RM_RANGE: u32 = SEG_BASE + 0x02;
/// 序号倒置 —— 执行序必须严格递增
pub const RM_ORDER: u32 = SEG_BASE + 0x03;
/// 重复执行 —— 一格只跑一次
pub const RM_DUP: u32 = SEG_BASE + 0x04;
/// 族号越界 —— 五族编号 0..5
pub const RM_BADFAM: u32 = SEG_BASE + 0x05;
/// 场已冻结 —— freeze 后禁止再执行
pub const RM_FROZEN: u32 = SEG_BASE + 0x06;

/// 族数：五族
pub const FAMILIES: usize = 5;
/// 每族格数
pub const CELLS_PER_FAM: usize = 6;
/// 矩阵总格数
pub const CELLS: usize = FAMILIES * CELLS_PER_FAM;
/// 回归矩阵场标签
pub const RMX_TAG: u64 = 0x726d_7872_6d78_304b;

/// 单个回归格
#[derive(Clone, Debug)]
pub struct RmCell {
    /// 格号（0x13000 段起编）
    pub cid: u32,
    /// 族号
    pub fam: u8,
    /// 格内维号 0..6
    pub dim: u8,
    /// 执行序号（0 = 未执行）
    pub order: u64,
    /// 裁断位
    pub verdict: bool,
}

/// 回归矩阵场总账
#[derive(Clone, Debug)]
pub struct RmArena {
    /// 30 格矩阵
    pub cells: Vec<RmCell>,
    /// 已执行格数
    pub run_n: usize,
    /// 最后一次执行序号
    pub last_order: u64,
    /// 冻结标记
    pub frozen: bool,
}

impl RmArena {
    /// 立场：30 格全部登记（未执行）
    pub fn new() -> RmArena {
        let mut ar = RmArena {
            cells: Vec::new(),
            run_n: 0, last_order: 0, frozen: false,
        };
        for fam in 0..FAMILIES {
            for dim in 0..CELLS_PER_FAM {
                let no = fam * CELLS_PER_FAM + dim;
                ar.cells.push(RmCell {
                    cid: 0x13000 + no as u32,
                    fam: fam as u8,
                    dim: dim as u8,
                    order: 0,
                    verdict: false,
                });
            }
        }
        ar
    }

    /// 执行一格：序号必须严格递增
    pub fn run_cell(&mut self, no: usize, verdict: bool) -> Result<u64, u32> {
        if self.frozen { return Err(RM_FROZEN); }
        if self.cells.is_empty() { return Err(RM_EMPTY); }
        if no >= self.cells.len() { return Err(RM_RANGE); }
        let c = &mut self.cells[no];
        if c.order != 0 { return Err(RM_DUP); }
        let order = self.last_order.wrapping_add(1);
        c.order = order;
        c.verdict = verdict;
        self.last_order = order;
        self.run_n += 1;
        Ok(order)
    }

    /// 全量执行：30 格依序全绿
    pub fn run_all(&mut self) -> Result<usize, u32> {
        for no in 0..self.cells.len() {
            self.run_cell(no, true)?;
        }
        Ok(self.run_n)
    }

    /// 按族统计已执行格数
    pub fn run_fam(&self, fam: u8) -> Result<usize, u32> {
        if fam as usize >= FAMILIES { return Err(RM_BADFAM); }
        Ok(self.cells.iter().filter(|c| c.fam == fam && c.order != 0).count())
    }

    /// 序账单调：已执行格的序号两两严格递增
    pub fn order_monotonic(&self) -> bool {
        let mut prev = 0u64;
        for c in &self.cells {
            if c.order != 0 {
                if c.order <= prev { return false; }
                prev = c.order;
            }
        }
        true
    }

    /// 全场裁断：30 格全执行、全绿、序单调
    pub fn all_green(&self) -> bool {
        self.run_n == CELLS
            && self.cells.iter().all(|c| c.verdict && c.order != 0)
            && self.order_monotonic()
            && self.last_order == CELLS as u64
    }

    /// FNV 折叠
    pub fn fold(&self) -> u64 {
        let mut h: u64 = RMX_TAG;
        for c in &self.cells {
            h = (h ^ (c.cid as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ c.order).wrapping_mul(0x100000001b3);
            h = (h ^ (c.verdict as u64)).wrapping_mul(0x100000001b3);
        }
        h
    }

    /// 冻结矩阵场
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 确定性样本：30 格依序全绿
pub fn sample_run() -> (RmArena, bool) {
    let mut ar = RmArena::new();
    let _ = ar.run_all();
    let ok = ar.all_green();
    ar.freeze();
    (ar, ok)
}

/// 注入路径：序倒置 / 重复执行 / 越界 / 冻结
pub fn inject_four() -> [bool; 4] {
    let mut ar = RmArena::new();
    // 注入 1：重复执行 → RM_DUP 拒止
    let _ = ar.run_cell(0, true);
    let i1 = matches!(ar.run_cell(0, true), Err(RM_DUP));
    // 注入 2：格号越界 → RM_RANGE 拒止
    let i2 = matches!(ar.run_cell(CELLS, true), Err(RM_RANGE));
    // 注入 3：冻结后执行 → RM_FROZEN 拒止
    ar.freeze();
    let i3 = matches!(ar.run_cell(1, true), Err(RM_FROZEN));
    // 注入 4：序账倒置检测 —— 手工写乱序后 order_monotonic 为假
    let mut ar2 = RmArena::new();
    ar2.cells[0].order = 5;
    ar2.cells[1].order = 3;
    let i4 = !ar2.order_monotonic();
    [i1, i2, i3, i4]
}

/// 判据检查：J1 五族矩阵账与执行序
pub fn run_unx11061_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11061");
    let (ar, ok) = sample_run();

    s.add("J1-全格执行", ok && ar.run_n == CELLS, "30 格回归矩阵全格执行");
    s.add("J1-全格裁断", ar.cells.iter().all(|c| c.verdict), "30 格裁断全绿");
    s.add("J1-序账单调", ar.order_monotonic() && ar.last_order == CELLS as u64, "执行序 1..30 严格递增零倒置");
    s.add("J2-族账齐备", (0..FAMILIES as u8).all(|f| ar.run_fam(f).unwrap_or(0) == CELLS_PER_FAM), "五族每族 6/6 格执行");
    s.add("J2-维账齐备", (0..CELLS_PER_FAM as u8).all(|d| ar.cells.iter().filter(|c| c.dim == d).count() == FAMILIES), "六维每维 5 格分布");
    s.add("J2-序号连续", ar.cells.iter().map(|c| c.order).fold(0u64, |a, b| a.wrapping_add(b)) == (1u64..=CELLS as u64).fold(0u64, |a, b| a.wrapping_add(b)), "序号 1..30 连续无跳格");
    s.add("J3-编号连续", ar.cells[0].cid == 0x13000 && ar.cells[CELLS - 1].cid == 0x1301D, "格号 0x13000 段起编连续");
    s.add("J3-族归属", ar.cells.iter().filter(|c| c.fam == 0).count() == CELLS_PER_FAM, "族归属标注准确每族 6 格");
    s.add("J3-未执行零序", RmArena::new().cells.iter().all(|c| c.order == 0), "立账时全部格零序待执行");
    s.add("J4-重复拒跑", inject_four()[0], "重复执行被 RM_DUP 拒止");
    s.add("J4-越界拒跑", inject_four()[1], "格号越界被 RM_RANGE 拒止");
    s.add("J5-冻结拒跑", inject_four()[2], "冻结后执行被 RM_FROZEN 拒止");
    s.add("J5-倒置必检", inject_four()[3], "乱序账 order_monotonic 即假");
    s.add("J6-空账拒跑", RmArena { cells: Vec::new(), run_n: 0, last_order: 0, frozen: false }.run_cell(0, true).is_err(), "空场执行被 RM_EMPTY 拒止");
    s.add("J6-指纹稳定", ar.frozen && ar.fold() == ar.fold(), "冻结后指纹重算稳定");

    // J8 族号越界：五族之外的族号查询必拒
    let bad_fam = ar.run_fam(FAMILIES as u8).err() == Some(RM_BADFAM);
    s.add("J8-族号越界拒", bad_fam, "族号 >= 5 查询被 RM_BADFAM 拒止");

    // J9 部分执行语义：三格执行后未全绿但序账仍单调
    let j9 = {
        let mut part = RmArena::new();
        let mut n = 0usize;
        let mut ran = true;
        while n < 3 {
            if part.run_cell(n, true).is_err() {
                ran = false;
            }
            n += 1;
        }
        ran
            && part.run_n == 3
            && part.last_order == 3
            && part.order_monotonic()
            && !part.all_green()
    };
    s.add("J9-部分执行语义", j9, "三格执行后未全绿但序账单调");

    // J10 指纹敏感：某格裁断翻转后全场指纹必变
    let j10 = {
        let (base, _) = sample_run();
        let mut flipped = RmArena::new();
        let _ = flipped.run_all();
        let root = flipped.fold();
        if let Some(c) = flipped.cells.get_mut(7) {
            c.verdict = false;
        }
        let after = flipped.fold();
        root != after && base.fold() == root
    };
    s.add("J10-指纹敏感", j10, "单格裁断翻转即改全场指纹");

    s.add_dyn(
        "J7-矩阵动态",
        ok,
        &format!("执行 {}/{}，末序 {}，fold={:#x}", ar.run_n, CELLS, ar.last_order, ar.fold()),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11061_reg_matrix_arena_flow() {
        let mut ar = RmArena::new();
        // 全量执行
        assert_eq!(ar.run_all().unwrap_or(0), CELLS);
        assert!(ar.all_green());
        assert_eq!(ar.last_order, CELLS as u64);
        // 族账抽查
        assert_eq!(ar.run_fam(4).unwrap_or(0), CELLS_PER_FAM);
        let root = ar.freeze();
        assert!(ar.frozen);
        assert_eq!(root, ar.fold());
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11061_checks_green() {
        let s = run_unx11061_checks();
        assert!(s.all_passed(), "UNX-F11061 判据未全绿");
    }

    #[test]
    fn unx11061_partial_run_semantics() {
        let mut ar = RmArena::new();
        // 前五格执行：未全绿但序账单调、族账可分族统计
        let mut n = 0usize;
        while n < 5 {
            assert!(ar.run_cell(n, true).is_ok());
            n += 1;
        }
        assert!(!ar.all_green());
        assert!(ar.order_monotonic());
        assert_eq!(ar.last_order, 5);
        assert_eq!(ar.run_fam(0).unwrap_or(0), 5);
        assert_eq!(ar.run_fam(1).unwrap_or(0), 0);
        // 冻结后继续执行必拒
        ar.freeze();
        assert!(ar.run_cell(5, true).err() == Some(RM_FROZEN));
    }
}
