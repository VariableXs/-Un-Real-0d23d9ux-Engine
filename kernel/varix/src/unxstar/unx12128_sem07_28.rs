//! UNX-F12128 UWOP_SAVE_NONVOL/SAVE_NONVOL_FAR 语义（0x1a220 段）
//!
//! 卡面：GP 寄存器入栈保存族——SAVE_NONVOL（码 4，偏移 = 槽×8）、
//! SAVE_NONVOL_FAR（码 5，偏移 = 双槽 u32 不乘）；寄存器闭集、
//! 偏移对齐、槽缺失/零偏移显性拒。
//! 域：CoRun·UNX-D1-B07。
//!
//! 判据原文：
//! - J1 寄存器闭集：opinfo ≤ 15，越码必拒。
//! - J2 NONVOL 口径：偏移 = 槽 × 8。
//! - J3 FAR 口径：偏移 = 双槽 u32，不乘 8。
//! - J4 偏移对齐：NONVOL 偏移须 8 对齐（槽制保证）；FAR 偏移须 8 对齐。
//! - J5 槽缺失拒 / 零偏移拒。
//! - J6 印账冻结：seal 后登记必拒。
//! - J7 注入五况全抓。
//!
//! 结构：SvCode 保存码 + 偏移核 + SvStream 码流账 + 折叠指纹
//! （211 进制卷积）+ 注入核（越寄存器/坏对齐/无槽/零偏移/印后登记）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1a220 独占 0x20）。
pub const SEG_BASE: u32 = 0x1a220;
pub const SV_BAD_REG: u32 = SEG_BASE + 0x01; // 寄存器越闭集
pub const SV_NO_SLOT: u32 = SEG_BASE + 0x02; // 槽缺失
pub const SV_ZERO_OFF: u32 = SEG_BASE + 0x03; // 零偏移
pub const SV_UNALIGNED: u32 = SEG_BASE + 0x04; // 坏对齐
pub const SV_FROZEN: u32 = SEG_BASE + 0x05; // 印账后登记

/// 展开码常量。
pub const OP_SAVE_NONVOL: u8 = 4;
pub const OP_SAVE_NONVOL_FAR: u8 = 5;

/// 偏移核：返回 (寄存器, 偏移, 消耗槽数)。
pub fn save_offset(opcode: u8, opinfo: u8, slots: &[u16]) -> Result<(u8, u32, usize), u32> {
    if opinfo > 15 {
        return Err(SV_BAD_REG);
    }
    match opcode {
        OP_SAVE_NONVOL => {
            let s = slots.first().copied().ok_or(SV_NO_SLOT)?;
            if s == 0 {
                return Err(SV_ZERO_OFF);
            }
            Ok((opinfo, s as u32 * 8, 1))
        }
        OP_SAVE_NONVOL_FAR => {
            let lo = slots.first().copied().ok_or(SV_NO_SLOT)?;
            let hi = slots.get(1).copied().ok_or(SV_NO_SLOT)?;
            let off = lo as u32 | (hi as u32) << 16;
            if off == 0 {
                return Err(SV_ZERO_OFF);
            }
            if off % 8 != 0 {
                return Err(SV_UNALIGNED);
            }
            Ok((opinfo, off, 2))
        }
        _ => Err(SV_BAD_REG), // 本卡闭集外码
    }
}

/// 码流账。
pub struct SvStream {
    pub saves: Vec<(u8, u32)>, // (寄存器, 偏移)
    pub slots_used: usize,
    pub sealed: bool,
}

impl SvStream {
    pub fn new() -> SvStream {
        SvStream { saves: Vec::new(), slots_used: 0, sealed: false }
    }

    /// 逐码入流。
    pub fn push(&mut self, opcode: u8, opinfo: u8, slots: &[u16]) -> Result<(u8, u32), u32> {
        if self.sealed {
            return Err(SV_FROZEN);
        }
        let (reg, off, used) = save_offset(opcode, opinfo, slots)?;
        if used != slots.len() {
            return Err(SV_NO_SLOT);
        }
        self.saves.push((reg, off));
        self.slots_used += used;
        Ok((reg, off))
    }

    /// 保存对数。
    pub fn count(&self) -> usize {
        self.saves.len()
    }

    /// 偏移最大值。
    pub fn max_off(&self) -> u32 {
        self.saves.iter().map(|s| s.1).max().unwrap_or(0)
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for s in &self.saves {
            h = h.wrapping_mul(211).wrapping_add(s.0 as u32);
            h = h.wrapping_mul(211).wrapping_add(s.1);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("saves={} slots={}", self.count(), self.slots_used)
    }

    /// 诊断面：渲染行。
    pub fn line_of(&self, i: usize) -> String {
        match self.saves.get(i) {
            Some(s) => format!("R{}@{:x}", s.0, s.1),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 寄存器越闭集必拒。
    got[0] = save_offset(OP_SAVE_NONVOL, 16, &[4]).err() == Some(SV_BAD_REG);
    // 2) 无槽必拒。
    got[1] = save_offset(OP_SAVE_NONVOL, 3, &[]).err() == Some(SV_NO_SLOT);
    // 3) 零偏移必拒。
    got[2] = save_offset(OP_SAVE_NONVOL, 3, &[0]).err() == Some(SV_ZERO_OFF);
    // 4) FAR 坏对齐必拒。
    got[3] = save_offset(OP_SAVE_NONVOL_FAR, 3, &[0x0C, 0x00]).err() == Some(SV_UNALIGNED);
    // 5) 印账后登记必抓。
    let mut st = SvStream::new();
    st.seal();
    got[4] = st.push(OP_SAVE_NONVOL, 3, &[4]).err() == Some(SV_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12128_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12128");

    // J1 寄存器闭集。
    let j1 = {
        let ok = save_offset(OP_SAVE_NONVOL, 15, &[4]);
        let bad = save_offset(OP_SAVE_NONVOL, 16, &[4]);
        ok.is_ok() && bad.err() == Some(SV_BAD_REG)
    };
    s.add("J1-寄存器闭集", j1, "15 过 16 拒");

    // J2 NONVOL 口径。
    let j2 = save_offset(OP_SAVE_NONVOL, 5, &[8]) == Ok((5, 64, 1));
    s.add("J2-NONVOL口径", j2, "槽×8");

    // J3 FAR 口径。
    let j3 = {
        let d = save_offset(OP_SAVE_NONVOL_FAR, 3, &[0x28, 0x00]);
        d == Ok((3, 0x28, 2))
    };
    s.add("J3-FAR口径", j3, "u32 直取不乘");

    // J4 FAR 坏对齐拒。
    let j4 = {
        let d = save_offset(OP_SAVE_NONVOL_FAR, 3, &[3, 0x00]);
        d.err() == Some(SV_UNALIGNED)
    };
    s.add("J4-FAR对齐拒", j4, "非 8 对齐必拒");

    // J5 无槽拒。
    let j5 = save_offset(OP_SAVE_NONVOL_FAR, 3, &[4]).err() == Some(SV_NO_SLOT);
    s.add("J5-无槽拒", j5, "FAR 需双槽");

    // J6 零偏移拒。
    let j6 = {
        let d = save_offset(OP_SAVE_NONVOL_FAR, 3, &[0, 0]);
        d.err() == Some(SV_ZERO_OFF)
    };
    s.add("J6-零偏移拒", j6, "u32=0 必拒");

    // J7 闭集外码拒。
    let j7 = save_offset(9, 3, &[4]).err() == Some(SV_BAD_REG);
    s.add("J7-闭集外拒", j7, "非 4/5 码必拒");

    // J8 码流入账。
    let j8 = {
        let mut st = SvStream::new();
        let _ = st.push(OP_SAVE_NONVOL, 3, &[4]);
        let _ = st.push(OP_SAVE_NONVOL_FAR, 6, &[0x40, 0x00]);
        st.count() == 2 && st.slots_used == 3
    };
    s.add("J8-码流入账", j8, "双保存三槽");

    // J9 偏移最大值。
    let j9 = {
        let mut st = SvStream::new();
        let _ = st.push(OP_SAVE_NONVOL, 3, &[2]);
        let _ = st.push(OP_SAVE_NONVOL, 4, &[6]);
        st.max_off() == 48
    };
    s.add("J9-偏移最大", j9, "max 48");

    // J10 渲染行。
    let j10 = {
        let mut st = SvStream::new();
        let _ = st.push(OP_SAVE_NONVOL, 12, &[4]);
        st.line_of(0).contains("R12@20") && st.line_of(1) == "missing"
    };
    s.add("J10-渲染行", j10, "寄存器@偏移口径");

    // J11 印账冻结。
    let j11 = {
        let mut st = SvStream::new();
        let _ = st.push(OP_SAVE_NONVOL, 3, &[4]);
        st.seal();
        st.push(OP_SAVE_NONVOL, 4, &[4]).err() == Some(SV_FROZEN)
    };
    s.add("J11-印账冻结", j11, "seal 后必拒");

    // J12 指纹幂等。
    let j12 = {
        let mut st = SvStream::new();
        let _ = st.push(OP_SAVE_NONVOL, 3, &[4]);
        st.fold() == st.fold()
    };
    s.add("J12-指纹幂等", j12, "同态同指纹");

    // J13 注入核 5/5。
    let got = inject_five();
    let j13 = got.iter().all(|x| *x);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut st = SvStream::new();
    let _ = st.push(OP_SAVE_NONVOL, 3, &[3]);
    let _ = st.push(OP_SAVE_NONVOL, 6, &[5]);

    s.add("摘要回读", st.summary().contains("saves=2 slots=2"), "双保存");
    s.add("指纹非零", st.fold() != 0, "指纹有效");

    // J14 FAR 偏移不乘 8 且 8 对齐。
    let j14 = save_offset(OP_SAVE_NONVOL_FAR, 2, &[0x20, 0x00]) == Ok((2, 0x20, 2))
        && save_offset(OP_SAVE_NONVOL_FAR, 2, &[0x1C, 0x00]).is_err();
    s.add("J14-FAR口径", j14, "原始值偏移、8 对齐");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12128_save_flow() {
        assert_eq!(save_offset(OP_SAVE_NONVOL, 4, &[10]), Ok((4, 80, 1)));
        assert_eq!(save_offset(OP_SAVE_NONVOL_FAR, 4, &[0x10, 0x00]), Ok((4, 0x10, 2)));
        assert!(save_offset(OP_SAVE_NONVOL, 4, &[]).is_err());
    }

    #[test]
    fn unx12128_checks_green() {
        let s = run_unx12128_checks();
        assert!(s.all_passed(), "UNX-F12128 判据未全绿");
    }

    #[test]
    fn stream_max_off() {
        let mut st = SvStream::new();
        let _ = st.push(OP_SAVE_NONVOL_FAR, 0, &[0x100, 0x00]);
        assert_eq!(st.max_off(), 0x100);
    }
}
