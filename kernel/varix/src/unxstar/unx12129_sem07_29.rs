//! UNX-F12129 UWOP_SAVE_XMM128/SAVE_XMM128_FAR 语义（0x1a240 段）
//!
//! 卡面：XMM 寄存器保存族——SAVE_XMM128（码 8，仅 XMM8..15，
//! 偏移 = 槽×16）、SAVE_XMM128_FAR（码 9，偏移 = 双槽 u32，
//! 16 对齐）；低号 XMM 拒、偏移 16 对齐。
//! 域：CoRun·UNX-D1-B07。
//!
//! 判据原文：
//! - J1 寄存器闭集：opinfo 仅 8..15（XMM 编号口径），低号必拒。
//! - J2 XMM128 口径：偏移 = 槽 × 16。
//! - J3 FAR 口径：偏移 = 双槽 u32，16 对齐。
//! - J4 槽缺失拒 / 零偏移拒。
//! - J5 印账冻结：seal 后登记必拒。
//! - J6 注入五况全抓。
//!
//! 结构：XaCode 保存码 + 偏移核 + XaStream 码流账 + 折叠指纹
//! （211 进制卷积）+ 注入核（低号/坏对齐/无槽/零偏移/印后登记）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1a240 独占 0x20）。
pub const SEG_BASE: u32 = 0x1a240;
pub const XA_BAD_REG: u32 = SEG_BASE + 0x01; // 低号 XMM
pub const XA_NO_SLOT: u32 = SEG_BASE + 0x02; // 槽缺失
pub const XA_ZERO_OFF: u32 = SEG_BASE + 0x03; // 零偏移
pub const XA_UNALIGNED: u32 = SEG_BASE + 0x04; // 坏对齐
pub const XA_FROZEN: u32 = SEG_BASE + 0x05; // 印账后登记

/// 展开码常量。
pub const OP_SAVE_XMM128: u8 = 8;
pub const OP_SAVE_XMM128_FAR: u8 = 9;

/// 偏移核：返回 (XMM 号, 偏移, 消耗槽数)。
pub fn xmm_save_offset(opcode: u8, opinfo: u8, slots: &[u16]) -> Result<(u8, u32, usize), u32> {
    if opinfo < 8 || opinfo > 15 {
        return Err(XA_BAD_REG);
    }
    match opcode {
        OP_SAVE_XMM128 => {
            let s = slots.first().copied().ok_or(XA_NO_SLOT)?;
            if s == 0 {
                return Err(XA_ZERO_OFF);
            }
            Ok((opinfo, s as u32 * 16, 1))
        }
        OP_SAVE_XMM128_FAR => {
            let lo = slots.first().copied().ok_or(XA_NO_SLOT)?;
            let hi = slots.get(1).copied().ok_or(XA_NO_SLOT)?;
            let off = lo as u32 | (hi as u32) << 16;
            if off == 0 {
                return Err(XA_ZERO_OFF);
            }
            if off % 16 != 0 {
                return Err(XA_UNALIGNED);
            }
            Ok((opinfo, off, 2))
        }
        _ => Err(XA_BAD_REG),
    }
}

/// 码流账。
pub struct XaStream {
    pub saves: Vec<(u8, u32)>,
    pub slots_used: usize,
    pub sealed: bool,
}

impl XaStream {
    pub fn new() -> XaStream {
        XaStream { saves: Vec::new(), slots_used: 0, sealed: false }
    }

    /// 逐码入流。
    pub fn push(&mut self, opcode: u8, opinfo: u8, slots: &[u16]) -> Result<(u8, u32), u32> {
        if self.sealed {
            return Err(XA_FROZEN);
        }
        let (reg, off, used) = xmm_save_offset(opcode, opinfo, slots)?;
        if used != slots.len() {
            return Err(XA_NO_SLOT);
        }
        self.saves.push((reg, off));
        self.slots_used += used;
        Ok((reg, off))
    }

    /// 保存对数。
    pub fn count(&self) -> usize {
        self.saves.len()
    }

    /// 偏移最小值。
    pub fn min_off(&self) -> u32 {
        self.saves.iter().map(|s| s.1).min().unwrap_or(0)
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
            Some(s) => format!("X{}@{:x}", s.0, s.1),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 低号 XMM 必拒。
    got[0] = xmm_save_offset(OP_SAVE_XMM128, 3, &[4]).err() == Some(XA_BAD_REG);
    // 2) 无槽必拒。
    got[1] = xmm_save_offset(OP_SAVE_XMM128, 10, &[]).err() == Some(XA_NO_SLOT);
    // 3) 零偏移必拒。
    got[2] = xmm_save_offset(OP_SAVE_XMM128, 10, &[0]).err() == Some(XA_ZERO_OFF);
    // 4) FAR 坏对齐必拒（8 对齐非 16）。
    got[3] = xmm_save_offset(OP_SAVE_XMM128_FAR, 10, &[0x08, 0x00])
        .err() == Some(XA_UNALIGNED);
    // 5) 印账后登记必抓。
    let mut st = XaStream::new();
    st.seal();
    got[4] = st.push(OP_SAVE_XMM128, 10, &[4]).err() == Some(XA_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12129_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12129");

    // J1 寄存器闭集：7 拒 8 过 15 过 16 拒。
    let j1 = xmm_save_offset(OP_SAVE_XMM128, 7, &[4]).err() == Some(XA_BAD_REG)
        && xmm_save_offset(OP_SAVE_XMM128, 8, &[4]).is_ok()
        && xmm_save_offset(OP_SAVE_XMM128, 15, &[4]).is_ok()
        && xmm_save_offset(OP_SAVE_XMM128, 16, &[4]).err() == Some(XA_BAD_REG);
    s.add("J1-寄存器闭集", j1, "8..15 四界");

    // J2 XMM128 口径。
    let j2 = xmm_save_offset(OP_SAVE_XMM128, 8, &[4]) == Ok((8, 64, 1));
    s.add("J2-XMM128口径", j2, "槽×16");

    // J3 FAR 口径。
    let j3 = {
        let d = xmm_save_offset(OP_SAVE_XMM128_FAR, 12, &[0x30, 0x00]);
        d == Ok((12, 0x30, 2))
    };
    s.add("J3-FAR口径", j3, "u32 直取");

    // J4 FAR 坏对齐拒（8 对齐非 16）。
    let j4 = {
        let d = xmm_save_offset(OP_SAVE_XMM128_FAR, 12, &[0x08, 0x00]);
        d.err() == Some(XA_UNALIGNED)
    };
    s.add("J4-16对齐拒", j4, "8 对齐亦不足");

    // J5 无槽拒。
    let j5 = xmm_save_offset(OP_SAVE_XMM128_FAR, 10, &[4]).err() == Some(XA_NO_SLOT);
    s.add("J5-无槽拒", j5, "FAR 需双槽");

    // J6 零偏移拒。
    let j6 = {
        let d = xmm_save_offset(OP_SAVE_XMM128_FAR, 10, &[0, 0]);
        d.err() == Some(XA_ZERO_OFF)
    };
    s.add("J6-零偏移拒", j6, "u32=0 必拒");

    // J7 闭集外码拒。
    let j7 = xmm_save_offset(4, 10, &[4]).err() == Some(XA_BAD_REG);
    s.add("J7-闭集外拒", j7, "非 8/9 码必拒");

    // J8 码流入账。
    let j8 = {
        let mut st = XaStream::new();
        let _ = st.push(OP_SAVE_XMM128, 8, &[3]);
        let _ = st.push(OP_SAVE_XMM128_FAR, 15, &[0x20, 0x00]);
        st.count() == 2 && st.slots_used == 3
    };
    s.add("J8-码流入账", j8, "双保存三槽");

    // J9 偏移最小值。
    let j9 = {
        let mut st = XaStream::new();
        let _ = st.push(OP_SAVE_XMM128, 10, &[4]);
        let _ = st.push(OP_SAVE_XMM128, 11, &[8]);
        st.min_off() == 64
    };
    s.add("J9-偏移最小", j9, "min 64");

    // J10 渲染行。
    let j10 = {
        let mut st = XaStream::new();
        let _ = st.push(OP_SAVE_XMM128, 15, &[4]);
        st.line_of(0).contains("X15@40") && st.line_of(1) == "missing"
    };
    s.add("J10-渲染行", j10, "XMM@偏移口径");

    // J11 印账冻结。
    let j11 = {
        let mut st = XaStream::new();
        let _ = st.push(OP_SAVE_XMM128, 10, &[4]);
        st.seal();
        st.push(OP_SAVE_XMM128, 11, &[4]).err() == Some(XA_FROZEN)
    };
    s.add("J11-印账冻结", j11, "seal 后必拒");

    // J12 指纹幂等。
    let j12 = {
        let mut st = XaStream::new();
        let _ = st.push(OP_SAVE_XMM128, 10, &[4]);
        st.fold() == st.fold()
    };
    s.add("J12-指纹幂等", j12, "同态同指纹");

    // J13 注入核 5/5。
    let got = inject_five();
    let j13 = got.iter().all(|x| *x);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut st = XaStream::new();
    let _ = st.push(OP_SAVE_XMM128, 8, &[4]);
    let _ = st.push(OP_SAVE_XMM128, 9, &[6]);

    s.add("摘要回读", st.summary().contains("saves=2 slots=2"), "双保存");
    s.add("指纹非零", st.fold() != 0, "指纹有效");

    // J14 XMM 闭集两端：8 放行、15 放行、16 拒、7 拒。
    let j14 = xmm_save_offset(OP_SAVE_XMM128, 8, &[2]).is_ok()
        && xmm_save_offset(OP_SAVE_XMM128, 15, &[2]).is_ok()
        && xmm_save_offset(OP_SAVE_XMM128, 16, &[2]).is_err()
        && xmm_save_offset(OP_SAVE_XMM128, 7, &[2]).is_err();
    s.add("J14-闭集两端", j14, "8..15 闭区间");

    // J15 FAR 对齐倍数：16/32/48 过、24 拒。
    let j15 = xmm_save_offset(OP_SAVE_XMM128_FAR, 8, &[0x10, 0x00]).is_ok()
        && xmm_save_offset(OP_SAVE_XMM128_FAR, 8, &[0x20, 0x00]).is_ok()
        && xmm_save_offset(OP_SAVE_XMM128_FAR, 8, &[0x18, 0x00]).is_err();
    s.add("J15-FAR对齐族", j15, "16 的倍数闭集");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12129_xmm_flow() {
        assert_eq!(xmm_save_offset(OP_SAVE_XMM128, 8, &[2]), Ok((8, 32, 1)));
        assert!(xmm_save_offset(OP_SAVE_XMM128, 7, &[2]).is_err());
        assert!(xmm_save_offset(OP_SAVE_XMM128_FAR, 8, &[8, 0]).is_err());
    }

    #[test]
    fn unx12129_checks_green() {
        let s = run_unx12129_checks();
        assert!(s.all_passed(), "UNX-F12129 判据未全绿");
    }

    #[test]
    fn far_16_align() {
        assert!(xmm_save_offset(OP_SAVE_XMM128_FAR, 8, &[0x10, 0x00]).is_ok());
        assert!(xmm_save_offset(OP_SAVE_XMM128_FAR, 8, &[0x0C, 0x00]).is_err());
    }
}
