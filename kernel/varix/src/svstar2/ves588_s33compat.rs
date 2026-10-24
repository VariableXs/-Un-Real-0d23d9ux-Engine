//! VE-F4354 · 一致性兼容矩阵（U08 测试与资产组）
//! 职责：一致性兼容矩阵——矩阵四维（U 域工具 × 平台版本 × 设备分档 ×
//! 第三方版本）+ 矩阵维护（每发版全格实测——声明兼容不算数）+ 缺口处置
//! （不支持格显式标注 + 替代路径给出）+ 缺格补测排程阻断发版 + 误标支持
//! 回滚标注 + 读屏环境维（域本色）。
//! 段位：0x43E0（续表 idx588）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x43E0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CmCode {
    /// 缺格（补测排程阻断发版）。
    CellMissing = SEG_BASE,
    /// 误标支持（实测推翻——回滚标注）。
    FalseSupport = SEG_BASE + 0x01,
    /// 替代路径失效（重设计）。
    AltPathDead = SEG_BASE + 0x02,
}

/// 矩阵四维坐标。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Cell {
    pub tool: u8,
    pub platform: u8,
    pub tier: u8,
    pub third_ver: u8,
}

/// 格实测结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellResult {
    pub cell: Cell,
    /// 实测通过（声明兼容不算数——实测优先）。
    pub tested: bool,
    pub passed: bool,
    /// 不支持格的替代路径（0 = 无）。
    pub alt_path: u32,
    /// 读屏环境维实测（域色——矩阵含读屏维）。
    pub sr_tested: bool,
}

/// 兼容矩阵台。
#[derive(Default)]
pub struct CompatMatrix {
    pub cells: Vec<CellResult>,
    /// 期望全格数（四维笛卡尔积）。
    pub expect_cells: u32,
    pub blocks: u32,
    pub rollbacks: u32,
}

impl CompatMatrix {
    pub fn new(expect: u32) -> Self {
        CompatMatrix { expect_cells: expect, ..Default::default() }
    }

    fn same_cell(a: &Cell, b: &Cell) -> bool {
        a.tool == b.tool && a.platform == b.platform && a.tier == b.tier && a.third_ver == b.third_ver
    }

    /// 格实测登记（覆盖式）。
    pub fn report(&mut self, r: CellResult) {
        if let Some(x) = self.cells.iter_mut().find(|x| Self::same_cell(&x.cell, &r.cell)) {
            *x = r;
        } else {
            self.cells.push(r);
        }
    }

    /// 缺格检测：未实测格清单（阻断发版——返回缺格数）。
    pub fn missing(&mut self) -> usize {
        let got = self.cells.iter().filter(|c| c.tested).count() as u32;
        let miss = self.expect_cells.saturating_sub(got) as usize;
        if miss > 0 {
            self.blocks += 1;
        }
        miss
    }

    /// 发版闸：全格实测无缺才放行。
    pub fn release_gate(&mut self) -> bool {
        self.missing() == 0
    }

    /// 误标回滚：声明支持但实测失败 → 标注回滚（返回受影响格数）。
    pub fn rollback_false_support(&mut self, cell: Cell, real_pass: bool) -> Result<bool, CmCode> {
        let c = self.cells.iter_mut().find(|c| Self::same_cell(&c.cell, &cell)).ok_or(CmCode::CellMissing)?;
        if c.passed && !real_pass {
            c.passed = false;
            self.rollbacks += 1;
            Ok(true)
        } else {
            c.passed = real_pass;
            Ok(false)
        }
    }

    /// 不支持格替代路径强制：失败格必须给替代路径（缺失 = 重设计拒绝）。
    pub fn alt_required(&self, cell: Cell) -> Result<(), CmCode> {
        let c = self.cells.iter().find(|c| Self::same_cell(&c.cell, &cell)).ok_or(CmCode::CellMissing)?;
        if !c.passed && c.alt_path == 0 {
            return Err(CmCode::AltPathDead);
        }
        Ok(())
    }

    /// 读屏环境维断言（域色）：所有失败格的替代路径须含读屏方案（sr_tested）。
    pub fn sr_coverage(&self) -> bool {
        self.cells.iter().filter(|c| c.tested && !c.passed).all(|c| c.sr_tested || c.alt_path > 0)
    }

    /// 支持率 bp。
    pub fn support_bp(&self) -> u32 {
        if self.cells.is_empty() {
            return 0;
        }
        let p = self.cells.iter().filter(|c| c.passed).count() as u64;
        (p * 10000 / self.cells.len() as u64) as u32
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves588_checks() -> CheckSet {
    let mut set = CheckSet::new("ves588");
    let mut cm = CompatMatrix::new(8);

    // C588-01 全格实测登记（8 格：2 工具 × 2 平台 × 2 档简化维）。
    for t in 0..2u8 {
        for p in 0..2u8 {
            for tier in 0..2u8 {
                let _ = cm.report(CellResult {
                    cell: Cell { tool: t, platform: p, tier, third_ver: 1 },
                    tested: true, passed: true, alt_path: 0, sr_tested: true,
                });
            }
        }
    }
    chk(&mut set, "C588-01", cm.missing() == 0 && cm.release_gate(), "全格实测齐");

    // C588-02 缺格阻断发版（撤一格实测态）。
    cm.cells[3].tested = false;
    chk(&mut set, "C588-02", cm.missing() == 1 && !cm.release_gate() && cm.blocks == 1, "缺格阻断");

    // C588-03 补测恢复放行。
    cm.cells[3].tested = true;
    chk(&mut set, "C588-03", cm.release_gate(), "补测放行");

    // C588-04 声明兼容不算数（实测优先语义：未实测格不计支持）。
    let mut cm2 = CompatMatrix::new(4);
    chk(&mut set, "C588-04", cm2.missing() == 4 && cm2.support_bp() == 0, "空格不算支持");

    // C588-05 误标支持回滚标注。
    cm.cells[0].passed = true;
    let rb = cm.rollback_false_support(cm.cells[0].cell, false);
    chk(&mut set, "C588-05", rb == Ok(true) && !cm.cells[0].passed && cm.rollbacks == 1, "误标回滚");

    // C588-06 实测一致不回滚（零误翻）。
    let rb2 = cm.rollback_false_support(cm.cells[1].cell, true);
    chk(&mut set, "C588-06", rb2 == Ok(false) && cm.rollbacks == 1, "一致不回滚");

    // C588-07 失败格替代路径强制（缺失 = 重设计拒）。
    cm.cells[0].alt_path = 0;
    chk(&mut set, "C588-07", cm.alt_required(cm.cells[0].cell) == Err(CmCode::AltPathDead), "替代路径强制");

    // C588-08 给出替代路径后解除。
    cm.cells[0].alt_path = 77;
    chk(&mut set, "C588-08", cm.alt_required(cm.cells[0].cell).is_ok(), "替代路径解除");

    // C588-09 读屏环境维断言（域色）：失败格含读屏方案。
    cm.cells[0].sr_tested = true;
    chk(&mut set, "C588-09", cm.sr_coverage(), "读屏维覆盖");

    // C588-10 未登记格回滚拒。
    chk(&mut set, "C588-10", cm.rollback_false_support(Cell { tool: 9, platform: 9, tier: 9, third_ver: 9 }, false)
        == Err(CmCode::CellMissing), "未登记格拒");

    // C588-11 支持率核算（7/8 通过）。
    chk(&mut set, "C588-11", cm.support_bp() == 8750, "支持率 87.5%");

    // C588-12 空矩阵发版闸恒假（不臆测）。
    let mut cm3 = CompatMatrix::new(1);
    chk(&mut set, "C588-12", !cm3.release_gate(), "空矩阵不放行");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    fn res(tool: u8, passed: bool) -> CellResult {
        CellResult { cell: Cell { tool, platform: 1, tier: 1, third_ver: 1 }, tested: true, passed, alt_path: if passed { 0 } else { 5 }, sr_tested: !passed }
    }

    #[test]
    fn missing_blocks() {
        let mut cm = CompatMatrix::new(3);
        cm.report(res(1, true));
        assert_eq!(cm.missing(), 2);
        cm.report(res(2, true));
        cm.report(res(3, true));
        assert!(cm.release_gate());
    }

    #[test]
    fn rollback_marks() {
        let mut cm = CompatMatrix::new(1);
        cm.report(res(1, true));
        assert_eq!(cm.rollback_false_support(cm.cells[0].cell, false), Ok(true));
        assert!(!cm.cells[0].passed);
        assert_eq!(cm.support_bp(), 0);
    }
}
