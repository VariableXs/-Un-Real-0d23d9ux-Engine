//! UNX-F12127 UWOP_ALLOC_SMALL/ALLOC_LARGE 栈分配语义（0x1a200 段）
//!
//! 卡面：UWOP_ALLOC_SMALL（码 2，尺寸 = opinfo×8+8，8..=128）、
//! UWOP_ALLOC_LARGE（码 1，opinfo 0 → 下一槽 u16×8，opinfo 1 →
//! 下两槽 u32）；槽缺失/坏参/零尺寸/越限显性拒。
//! 域：CoRun·UNX-D1-B07。
//!
//! 判据原文：
//! - J1 SMALL 口径：尺寸 = opinfo×8+8（opinfo 0..=15 → 8..=128）。
//! - J2 LARGE 口径：opinfo 0 → slot×8；opinfo 1 → u32 直取。
//! - J3 LARGE 无槽拒：LARGE 后无操作数槽必拒。
//! - J4 LARGE 坏参拒：opinfo > 1 必拒。
//! - J5 零尺寸拒：LARGE 槽值为 0 必拒；LARGE u32 越界必拒。
//! - J6 印账冻结：seal 后登记必拒。
//! - J7 注入五况全抓。
//!
//! 结构：AlCode 分配码 + AlStream 码流（槽队列）+ RSP 增量核 +
//! 折叠指纹（211 进制卷积）+ 注入核（无槽/坏参/零槽/越限/印后登记）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1a200 独占 0x20）。
pub const SEG_BASE: u32 = 0x1a200;
pub const AL_NO_SLOT: u32 = SEG_BASE + 0x01; // LARGE 无槽
pub const AL_BAD_OP: u32 = SEG_BASE + 0x02; // LARGE 坏参
pub const AL_ZERO: u32 = SEG_BASE + 0x03; // 零尺寸
pub const AL_OVER: u32 = SEG_BASE + 0x04; // 尺寸越限
pub const AL_FROZEN: u32 = SEG_BASE + 0x05; // 印账后登记

/// 展开码常量。
pub const OP_ALLOC_SMALL: u8 = 2;
pub const OP_ALLOC_LARGE: u8 = 1;

/// LARGE 尺寸上限（模型口径 0x7FFF8）。
pub const AL_LARGE_CAP: u32 = 0x7FFF8;

/// 单码（含操作数槽队列视图）。
#[derive(Clone, Copy)]
pub struct AlCode {
    pub opcode: u8,
    pub opinfo: u8,
}

/// RSP 增量核：slots 为该码之后可用的操作数槽队列。
/// 返回 (增量, 消耗槽数)。
pub fn alloc_delta(c: AlCode, slots: &[u16]) -> Result<(u32, usize), u32> {
    match c.opcode {
        OP_ALLOC_SMALL => {
            if c.opinfo > 15 {
                return Err(AL_BAD_OP);
            }
            Ok((c.opinfo as u32 * 8 + 8, 0))
        }
        OP_ALLOC_LARGE => match c.opinfo {
            0 => {
                let s = slots.first().copied().ok_or(AL_NO_SLOT)?;
                if s == 0 {
                    return Err(AL_ZERO);
                }
                Ok((s as u32 * 8, 1))
            }
            1 => {
                let lo = slots.first().copied().ok_or(AL_NO_SLOT)?;
                let hi = slots.get(1).copied().ok_or(AL_NO_SLOT)?;
                let v = lo as u32 | (hi as u32) << 16;
                if v == 0 {
                    return Err(AL_ZERO);
                }
                if v > AL_LARGE_CAP {
                    return Err(AL_OVER);
                }
                Ok((v, 2))
            }
            _ => Err(AL_BAD_OP),
        },
        _ => Err(AL_BAD_OP), // 本卡闭集外码
    }
}

/// 码流册。
pub struct AlStream {
    pub codes: Vec<AlCode>,
    pub rsp_total: u32,
    pub sealed: bool,
}

impl AlStream {
    pub fn new() -> AlStream {
        AlStream { codes: Vec::new(), rsp_total: 0, sealed: false }
    }

    /// 逐码入流：slots 为紧跟该码的操作数槽。
    pub fn push(&mut self, c: AlCode, slots: &[u16]) -> Result<u32, u32> {
        if self.sealed {
            return Err(AL_FROZEN);
        }
        let (d, used) = alloc_delta(c, slots)?;
        if used != slots.len() {
            return Err(AL_NO_SLOT); // 多余槽按无槽面显性
        }
        self.codes.push(c);
        self.rsp_total += d;
        Ok(d)
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for c in &self.codes {
            h = h.wrapping_mul(211).wrapping_add(c.opcode as u32);
            h = h.wrapping_mul(211).wrapping_add(c.opinfo as u32);
        }
        h = h.wrapping_mul(211).wrapping_add(self.rsp_total);
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("codes={} rsp={}", self.codes.len(), self.rsp_total)
    }

    /// 诊断面：码渲染。
    pub fn line_of(&self, i: usize) -> String {
        match self.codes.get(i) {
            Some(c) => format!("A{:x}/{:x}", c.opcode, c.opinfo),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) LARGE 无槽必拒。
    got[0] = alloc_delta(AlCode { opcode: OP_ALLOC_LARGE, opinfo: 0 }, &[])
        .err() == Some(AL_NO_SLOT);
    // 2) LARGE 坏参必拒。
    got[1] = alloc_delta(AlCode { opcode: OP_ALLOC_LARGE, opinfo: 2 }, &[8])
        .err() == Some(AL_BAD_OP);
    // 3) LARGE 零槽必拒。
    got[2] = alloc_delta(AlCode { opcode: OP_ALLOC_LARGE, opinfo: 0 }, &[0])
        .err() == Some(AL_ZERO);
    // 4) LARGE u32 越限必拒。
    got[3] = alloc_delta(AlCode { opcode: OP_ALLOC_LARGE, opinfo: 1 }, &[0x0000, 0x0080])
        .err() == Some(AL_OVER);
    // 5) 印账后登记必抓。
    let mut st = AlStream::new();
    st.seal();
    got[4] = st.push(AlCode { opcode: OP_ALLOC_SMALL, opinfo: 0 }, &[])
        .err() == Some(AL_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12127_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12127");

    // J1 SMALL 口径：边界 0/15。
    let j1 = {
        let lo = alloc_delta(AlCode { opcode: OP_ALLOC_SMALL, opinfo: 0 }, &[]);
        let hi = alloc_delta(AlCode { opcode: OP_ALLOC_SMALL, opinfo: 15 }, &[]);
        lo == Ok((8, 0)) && hi == Ok((128, 0))
    };
    s.add("J1-SMALL口径", j1, "8..=128");

    // J2 SMALL 坏参拒。
    let j2 = {
        let bad = alloc_delta(AlCode { opcode: OP_ALLOC_SMALL, opinfo: 16 }, &[]);
        bad.err() == Some(AL_BAD_OP)
    };
    s.add("J2-SMALL坏参拒", j2, "opinfo>15 必拒");

    // J3 LARGE opinfo0 口径。
    let j3 = {
        let d = alloc_delta(AlCode { opcode: OP_ALLOC_LARGE, opinfo: 0 }, &[16]);
        d == Ok((128, 1))
    };
    s.add("J3-LARGE-u16", j3, "slot×8");

    // J4 LARGE opinfo1 口径。
    let j4 = {
        let d = alloc_delta(AlCode { opcode: OP_ALLOC_LARGE, opinfo: 1 }, &[0x00A0, 0x0000]);
        d == Ok((0xA0, 2))
    };
    s.add("J4-LARGE-u32", j4, "u32 直取");

    // J5 无槽拒。
    let j5 = {
        let d = alloc_delta(AlCode { opcode: OP_ALLOC_LARGE, opinfo: 1 }, &[0x10]);
        d.err() == Some(AL_NO_SLOT)
    };
    s.add("J5-无槽拒", j5, "u32 需双槽");

    // J6 零尺寸拒。
    let j6 = {
        let d = alloc_delta(AlCode { opcode: OP_ALLOC_LARGE, opinfo: 1 }, &[0, 0]);
        d.err() == Some(AL_ZERO)
    };
    s.add("J6-零尺寸拒", j6, "u32=0 必拒");

    // J7 越限拒。
    let j7 = {
        let d = alloc_delta(AlCode { opcode: OP_ALLOC_LARGE, opinfo: 1 }, &[0xFFFF, 0xFFFF]);
        d.err() == Some(AL_OVER)
    };
    s.add("J7-越限拒", j7, "超 0x7FFF8 必拒");

    // J8 码流入账。
    let j8 = {
        let mut st = AlStream::new();
        let _ = st.push(AlCode { opcode: OP_ALLOC_SMALL, opinfo: 3 }, &[]);
        let _ = st.push(AlCode { opcode: OP_ALLOC_LARGE, opinfo: 0 }, &[4]);
        st.rsp_total == 32 + 32 && st.summary().contains("codes=2")
    };
    s.add("J8-码流入账", j8, "32+32=64");

    // J9 多余槽显性。
    let j9 = {
        let mut st = AlStream::new();
        st.push(AlCode { opcode: OP_ALLOC_LARGE, opinfo: 0 }, &[4, 5])
            .err() == Some(AL_NO_SLOT)
    };
    s.add("J9-多余槽拒", j9, "消耗数与槽队列须等");

    // J10 印账冻结。
    let j10 = {
        let mut st = AlStream::new();
        let _ = st.push(AlCode { opcode: OP_ALLOC_SMALL, opinfo: 0 }, &[]);
        st.seal();
        st.push(AlCode { opcode: OP_ALLOC_SMALL, opinfo: 1 }, &[]).err() == Some(AL_FROZEN)
    };
    s.add("J10-印账冻结", j10, "seal 后必拒");

    // J11 指纹幂等。
    let j11 = {
        let mut st = AlStream::new();
        let _ = st.push(AlCode { opcode: OP_ALLOC_SMALL, opinfo: 0 }, &[]);
        st.fold() == st.fold()
    };
    s.add("J11-指纹幂等", j11, "同态同指纹");

    // J12 注入核 5/5。
    let got = inject_five();
    let j12 = got.iter().all(|x| *x);
    s.add_dyn("J12-注入全抓", j12, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut st = AlStream::new();
    let _ = st.push(AlCode { opcode: OP_ALLOC_SMALL, opinfo: 2 }, &[]);
    let _ = st.push(AlCode { opcode: OP_ALLOC_LARGE, opinfo: 1 }, &[0x0080, 0x0000]);

    s.add("摘要回读", st.summary().contains("rsp=152"), "24+128=152");
    s.add("渲染行", st.line_of(1).contains("A1/1"), "LARGE 行渲染");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12127_alloc_flow() {
        assert_eq!(alloc_delta(AlCode { opcode: OP_ALLOC_SMALL, opinfo: 5 }, &[]), Ok((48, 0)));
        assert_eq!(alloc_delta(AlCode { opcode: OP_ALLOC_LARGE, opinfo: 0 }, &[8]), Ok((64, 1)));
        assert!(alloc_delta(AlCode { opcode: OP_ALLOC_LARGE, opinfo: 0 }, &[]).is_err());
    }

    #[test]
    fn unx12127_checks_green() {
        let s = run_unx12127_checks();
        assert!(s.all_passed(), "UNX-F12127 判据未全绿");
    }

    #[test]
    fn stream_accumulate() {
        let mut st = AlStream::new();
        let _ = st.push(AlCode { opcode: OP_ALLOC_SMALL, opinfo: 0 }, &[]);
        let _ = st.push(AlCode { opcode: OP_ALLOC_SMALL, opinfo: 15 }, &[]);
        assert_eq!(st.rsp_total, 136);
    }
}
