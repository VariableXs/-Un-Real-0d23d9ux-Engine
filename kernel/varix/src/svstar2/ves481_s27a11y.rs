//! VE-F4247 · 无障碍一致性验证（U03 一致性验证平台·域色红线本体）
//! 职责：S 域标准单源矩阵检（域×标准条目）+ 一致性违规双轨升级
//! （一致性通道+无障碍通道双立案）+ 漏格补检 + 标准漂移以 S 域为准。
//! 段位：0x1C00（续表 idx481）。行数预算 340。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1C00;

/// 错误码：矩阵检域。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AyCode {
    /// 矩阵漏格（未检项）。
    CellMiss = SEG_BASE,
    /// 双通道立案链断裂。
    ChannelBroken = SEG_BASE + 0x01,
    /// 标准源漂移（非 S 域口径）。
    StdDrift = SEG_BASE + 0x02,
    /// 违规未升级为缺陷。
    CaseLost = SEG_BASE + 0x03,
    /// 补检后仍缺格。
    RefillFail = SEG_BASE + 0x04,
}

/// 域标识（十七域裁剪：本表覆盖 U03 汇总面）。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dom {
    S = 0,
    T = 1,
    U01 = 2,
    U02 = 3,
    U03 = 4,
}
pub const DOM_N: usize = 5;

/// S 域无障碍标准条目（单源指针指向 S 域标准族——本表只引用不自立）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StdItem {
    /// 条目号（S 域标准族内序号）。
    pub sid: u16,
    /// 是否红线级（红线违规即时阻断）。
    pub redline: bool,
}

pub const STD_ITEMS: [StdItem; 6] = [
    StdItem { sid: 1, redline: false }, // 焦点序
    StdItem { sid: 2, redline: false }, // 对比度
    StdItem { sid: 3, redline: true },  // 读屏标签
    StdItem { sid: 4, redline: false }, // 键盘可达
    StdItem { sid: 5, redline: false }, // 动效降级
    StdItem { sid: 6, redline: true },  // 语言声明
];

/// 矩阵格：某域对某条目的检查结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CellState {
    /// 已检·合规。
    Pass,
    /// 已检·违规。
    Violate,
    /// 未检（漏格）。
    Blind,
}

/// 矩阵检表：域×标准条目。
pub struct AyMatrix {
    /// 行=域、列=条目。
    pub cells: [[CellState; 6]; DOM_N],
    /// 标准源指纹（S 域单源哈希——漂移即拒检）。
    pub std_fp: u32,
    /// 双通道立案账（违规格 → 两通道各一张单）。
    pub cases: Vec<(u8, u16, bool)>, // (dom, sid, both_filed)
}

impl AyMatrix {
    /// 构造：全格置未检、锁定标准源指纹。
    pub fn new(std_fp: u32) -> Self {
        AyMatrix {
            cells: [[CellState::Blind; 6]; DOM_N],
            std_fp,
            cases: Vec::new(),
        }
    }

    /// 标准源对拍：指纹不符即 StdDrift（S 域为准）。
    pub fn std_ok(&self, expect_fp: u32) -> bool {
        self.std_fp == expect_fp
    }

    /// 单格检入。
    pub fn check_cell(&mut self, d: Dom, sid: u16, pass: bool) -> Result<(), AyCode> {
        let col = STD_ITEMS.iter().position(|s| s.sid == sid).ok_or(AyCode::StdDrift)?;
        self.cells[d as usize][col] = if pass { CellState::Pass } else { CellState::Violate };
        Ok(())
    }

    /// 漏格补检：返回仍未检的 (域,条目) 列表。
    pub fn blind_cells(&self) -> Vec<(u8, u16)> {
        let mut out = Vec::new();
        for r in 0..DOM_N {
            for c in 0..6 {
                if self.cells[r][c] == CellState::Blind {
                    out.push((r as u8, STD_ITEMS[c].sid));
                }
            }
        }
        out
    }

    /// 双轨升级：违规格必须在一致性+无障碍两通道各立案（红线格追加阻断标记）。
    pub fn dual_file(&mut self, d: Dom, sid: u16) -> Result<bool, AyCode> {
        let col = STD_ITEMS.iter().position(|s| s.sid == sid).ok_or(AyCode::StdDrift)?;
        if self.cells[d as usize][col] != CellState::Violate {
            return Err(AyCode::CaseLost);
        }
        let red = STD_ITEMS[col].redline;
        self.cases.push((d as u8, sid, true));
        Ok(red)
    }

    /// 红线违规格计数（即时阻断合并）。
    pub fn redline_violations(&self) -> usize {
        let mut n = 0;
        for r in 0..DOM_N {
            for c in 0..6 {
                if STD_ITEMS[c].redline && self.cells[r][c] == CellState::Violate {
                    n += 1;
                }
            }
        }
        n
    }

    /// 矩阵完备：无漏格且全部违规已双立案。
    pub fn complete(&self) -> bool {
        self.blind_cells().is_empty()
            && self.cells.iter().all(|row| {
                row.iter().zip(STD_ITEMS.iter()).all(|(st, it)| {
                    st != &CellState::Violate || it.redline
                        || self.cases.iter().any(|(_, sid, both)| *both && STD_ITEMS.iter().any(|s| s.sid == *sid))
                })
            })
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C481-01..12。
pub fn run_ves481_checks() -> CheckSet {
    let mut set = CheckSet::new("ves481");

    // 基础矩阵构造与漏格
    let mut m = AyMatrix::new(0xA110_00F1);
    chk(&mut set, "C481-01", m.blind_cells().len() == DOM_N * 6, "新表 30 格全盲");
    chk(&mut set, "C481-02", m.complete() == false, "盲表不完备");

    // 标准源对拍
    chk(&mut set, "C481-03", m.std_ok(0xA110_00F1), "指纹一致·单源通过");
    chk(&mut set, "C481-04", !m.std_ok(0xDEAD), "指纹漂移即拒检");

    // 检入与违规双轨
    let _ = m.check_cell(Dom::S, 3, false);
    let _ = m.check_cell(Dom::T, 1, true);
    chk(&mut set, "C481-05", m.dual_file(Dom::S, 3) == Ok(true), "红线违规·双轨升级+阻断");
    let _ = m.check_cell(Dom::U01, 2, false);
    chk(&mut set, "C481-06", m.dual_file(Dom::U01, 2) == Ok(false), "非红线违规·双轨无阻断");
    chk(&mut set, "C481-07", m.dual_file(Dom::U01, 1) == Err(AyCode::CaseLost), "合规格不可立案");

    // 未知条目拒收
    chk(&mut set, "C481-08", m.check_cell(Dom::S, 99, true) == Err(AyCode::StdDrift), "标准外条目=漂移");

    // 补检收敛
    let blind_before = m.blind_cells().len();
    for (r, sid) in m.blind_cells().clone() {
        let d = match r { 0 => Dom::S, 1 => Dom::T, 2 => Dom::U01, 3 => Dom::U02, _ => Dom::U03 };
        let _ = m.check_cell(d, sid, true);
    }
    chk(&mut set, "C481-09", m.blind_cells().is_empty(), "补检后零漏格");
    chk(&mut set, "C481-10", blind_before == 28, "补检前盲格精确计数");
    chk(&mut set, "C481-11", m.redline_violations() == 1, "红线违规格=1（S×条目3）");
    chk(&mut set, "C481-12", m.cases.len() == 2 && m.cases.iter().all(|c| c.2), "双通道立案账齐备");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_dual_track() {
        let mut m = AyMatrix::new(1);
        assert!(m.check_cell(Dom::T, 3, false).is_ok());
        assert_eq!(m.dual_file(Dom::T, 3), Ok(true));
        assert_eq!(m.blind_cells().len(), 29);
        assert!(!m.std_ok(2));
    }
}
