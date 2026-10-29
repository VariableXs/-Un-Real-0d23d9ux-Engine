//! UNX-F12209 异常注入矩阵：三类异常 × 嵌套深度（SEG 0x64120..0x64140 段窗）
//!
//! 卡面：异常注入矩阵核账——三类异常码 × 五档嵌套深度的全格注入、
//! 单元去重、矩阵完备核账；越格与重复注入拒止全链过闸。
//! 域：CoRun·UNX-D1-B11。
//!
//! 判据原文：
//! - J1 三类异常码：AV/除零/断点三码合法，他码必拒 INJ_BAD_CLASS。
//! - J2 深度档位：0..=4 五档合法，>4 必拒 INJ_DEPTH。
//! - J3 全格注入：3×5=15 格全部可注入。
//! - J4 单元去重：同 (code,depth) 二次注入必拒 INJ_DUP。
//! - J5 结果确定：同格重算结果一致（确定性映射）。
//! - J6 矩阵完备：15 格齐备后 matrix_full 为真。
//! - J7 空格查询拒止：未注入格查询必拒 INJ_NO_CELL。
//! - J8 结果口径：处理器帧号 = 码低位 + 深度。
//! - J9 印账冻结：seal 后注入/查询必拒 INJ_FROZEN。
//! - J10 指纹幂等：同态同指纹。
//! - J11 摘要口径：格数回读。
//! - J12 注入五况全抓。
//!
//! 结构：InjectMatrix 注入矩阵（格账/操作账）+ 注入核（inject）+
//! 查询核（cell_of）+ 完备核（matrix_full）+ 折叠指纹 + 注入核
//! （坏类/越深/重复格/空格查询/印后操作）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x64120 独占 0x20 窗）。
pub const SEG_BASE: u32 = 0x64120;
pub const INJ_BAD_CLASS: u32 = SEG_BASE + 0x01; // 未知异常类
pub const INJ_DEPTH: u32 = SEG_BASE + 0x02; // 深度越档
pub const INJ_DUP: u32 = SEG_BASE + 0x03; // 重复格注入
pub const INJ_NO_CELL: u32 = SEG_BASE + 0x04; // 空格查询
pub const INJ_FROZEN: u32 = SEG_BASE + 0x05; // 印账后操作

/// 三类异常码。
pub const XC_AV: u32 = 0xC000_0005;
pub const XC_DIV0: u32 = 0xC000_0094;
pub const XC_BP: u32 = 0x8000_0003;

/// 深度档位上限（0..=4 五档）。
pub const MAX_DEPTH: u32 = 4;

fn known_class(c: u32) -> bool {
    c == XC_AV || c == XC_DIV0 || c == XC_BP
}

/// 矩阵格：(异常码, 深度) → 处理器帧号。
#[derive(Clone, Copy)]
pub struct Cell {
    pub code: u32,
    pub depth: u32,
    pub handler_frame: u32,
}

/// 注入矩阵。
pub struct InjectMatrix {
    pub cells: Vec<Cell>,
    pub ops: Vec<String>,
    pub sealed: bool,
}

impl InjectMatrix {
    pub fn new() -> InjectMatrix {
        InjectMatrix { cells: Vec::new(), ops: Vec::new(), sealed: false }
    }

    /// 确定性结果映射：处理器帧号 = 码低位 + 深度。
    pub fn result_of(code: u32, depth: u32) -> u32 {
        (code & 0xFF) + depth
    }

    /// 注入核。
    pub fn inject(&mut self, code: u32, depth: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(INJ_FROZEN);
        }
        if !known_class(code) {
            return Err(INJ_BAD_CLASS);
        }
        if depth > MAX_DEPTH {
            return Err(INJ_DEPTH);
        }
        if self.cells.iter().any(|c| c.code == code && c.depth == depth) {
            return Err(INJ_DUP);
        }
        let hf = Self::result_of(code, depth);
        self.cells.push(Cell { code, depth, handler_frame: hf });
        self.ops.push(format!("inj{:x}d{}", code, depth));
        Ok(hf)
    }

    /// 查询核：格在则返回帧号，空格必拒。
    pub fn cell_of(&self, code: u32, depth: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(INJ_FROZEN);
        }
        match self.cells.iter().find(|c| c.code == code && c.depth == depth) {
            Some(c) => Ok(c.handler_frame),
            None => Err(INJ_NO_CELL),
        }
    }

    /// 全格注入（3 类 × 5 档）。
    pub fn fill_all(&mut self) -> Result<usize, u32> {
        for code in [XC_AV, XC_DIV0, XC_BP] {
            for depth in 0..=MAX_DEPTH {
                self.inject(code, depth)?;
            }
        }
        Ok(self.cells.len())
    }

    /// 完备核：15 格齐备。
    pub fn matrix_full(&self) -> bool {
        self.cells.len() == 15
            && [XC_AV, XC_DIV0, XC_BP].iter().all(|code| {
                (0..=MAX_DEPTH).all(|d| self.cell_of(*code, d).is_ok())
            })
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for c in &self.cells {
            h = h.wrapping_mul(211).wrapping_add(c.code);
            h = h.wrapping_mul(211).wrapping_add(c.depth);
            h = h.wrapping_mul(211).wrapping_add(c.handler_frame);
        }
        for o in &self.ops {
            for by in o.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("cells={}", self.cells.len())
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 坏类必拒。
    got[0] = InjectMatrix::new().inject(0xE000_0001, 0).err() == Some(INJ_BAD_CLASS);
    // 2) 越档必拒。
    got[1] = InjectMatrix::new().inject(XC_AV, 5).err() == Some(INJ_DEPTH);
    // 3) 重复格必拒。
    let mut m = InjectMatrix::new();
    let _ = m.inject(XC_AV, 2);
    got[2] = m.inject(XC_AV, 2).err() == Some(INJ_DUP);
    // 4) 空格查询必拒。
    got[3] = InjectMatrix::new().cell_of(XC_DIV0, 0).err() == Some(INJ_NO_CELL);
    // 5) 印账后操作必抓。
    let mut f = InjectMatrix::new();
    f.seal();
    got[4] = f.inject(XC_AV, 0).err() == Some(INJ_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12209_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12209");

    // J1 三类异常码。
    let j1 = {
        let mut a = InjectMatrix::new();
        let c1 = a.inject(XC_AV, 0);
        let mut b = InjectMatrix::new();
        let c2 = b.inject(XC_DIV0, 0);
        let mut c = InjectMatrix::new();
        let c3 = c.inject(XC_BP, 0);
        c1.is_ok() && c2.is_ok() && c3.is_ok()
    };
    s.add("J1-三类异常码", j1, "AV/除零/断点可注入");

    // J2 深度档位。
    let j2 = {
        let mut m = InjectMatrix::new();
        let ok0 = m.inject(XC_AV, 0).is_ok();
        let mut m2 = InjectMatrix::new();
        let ok4 = m2.inject(XC_AV, 4).is_ok();
        let bad = InjectMatrix::new().inject(XC_AV, 5).err() == Some(INJ_DEPTH);
        ok0 && ok4 && bad
    };
    s.add("J2-深度档位", j2, "0..4 合法、5 拒");

    // J3 全格注入。
    let j3 = {
        let mut m = InjectMatrix::new();
        m.fill_all() == Ok(15)
    };
    s.add("J3-全格注入", j3, "15 格全数注入");

    // J4 单元去重。
    let j4 = {
        let mut m = InjectMatrix::new();
        let _ = m.inject(XC_DIV0, 3);
        m.inject(XC_DIV0, 3).err() == Some(INJ_DUP)
    };
    s.add("J4-单元去重", j4, "同格二次必拒");

    // J5 结果确定。
    let j5 = InjectMatrix::result_of(XC_AV, 2) == InjectMatrix::result_of(XC_AV, 2)
        && InjectMatrix::result_of(XC_BP, 0) == 0x03
        && InjectMatrix::result_of(XC_AV, 1) == 0x06;
    s.add("J5-结果确定", j5, "码低位+深度 确定映射");

    // J6 矩阵完备。
    let j6 = {
        let mut m = InjectMatrix::new();
        let _ = m.fill_all();
        m.matrix_full()
    };
    s.add("J6-矩阵完备", j6, "15 格齐备且全可查");

    // J7 空格查询拒止。
    let j7 = {
        let mut m = InjectMatrix::new();
        let _ = m.inject(XC_AV, 0);
        m.cell_of(XC_DIV0, 0).err() == Some(INJ_NO_CELL)
    };
    s.add("J7-空格查询拒止", j7, "未注入格必拒");

    // J8 结果口径回查。
    let j8 = {
        let mut m = InjectMatrix::new();
        let v = m.inject(XC_DIV0, 4).unwrap_or(0);
        v == 0x98 && m.cell_of(XC_DIV0, 4) == Ok(0x98)
    };
    s.add("J8-结果口径", j8, "注入值与查询值一致");

    // J9 印账冻结。
    let j9 = {
        let mut m = InjectMatrix::new();
        let _ = m.inject(XC_AV, 0);
        m.seal();
        m.inject(XC_AV, 1).err() == Some(INJ_FROZEN)
            && m.cell_of(XC_AV, 0).err() == Some(INJ_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后注入/查询皆拒");

    // J10 指纹幂等。
    let j10 = {
        let mut m = InjectMatrix::new();
        let _ = m.inject(XC_AV, 0);
        m.fold() == m.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 摘要口径。
    let j11 = {
        let mut m = InjectMatrix::new();
        let _ = m.fill_all();
        m.summary() == "cells=15"
    };
    s.add("J11-摘要口径", j11, "格数回读");

    // J12 异常码口径。
    let j12 = XC_AV == 0xC000_0005 && XC_DIV0 == 0xC000_0094 && XC_BP == 0x8000_0003;
    s.add("J12-异常码口径", j12, "三类异常码常量");

    let got = inject_five();
    s.add_dyn("J13-注入全抓", got.iter().all(|x| *x), &format!("inj5={:?}", got));
    s.add("J14-指纹非零", {
        let mut m = InjectMatrix::new();
        let _ = m.inject(XC_BP, 0);
        m.fold() != 0
    }, "指纹有效");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12209_matrix_flow() {
        let mut m = InjectMatrix::new();
        assert_eq!(m.inject(XC_AV, 0), Ok(0x05));
        assert_eq!(m.inject(XC_AV, 0), Err(INJ_DUP));
        assert_eq!(m.inject(0x1234, 0), Err(INJ_BAD_CLASS));
        assert_eq!(m.cell_of(XC_DIV0, 0), Err(INJ_NO_CELL));
    }

    #[test]
    fn unx12209_checks_green() {
        let s = run_unx12209_checks();
        assert!(s.all_passed(), "UNX-F12209 判据未全绿");
    }

    #[test]
    fn unx12209_inject_all_caught() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "注入五况未全抓: {:?}", got);
    }
}
