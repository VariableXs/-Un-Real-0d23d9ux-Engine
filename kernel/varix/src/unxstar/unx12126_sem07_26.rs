//! UNX-F12126 UWOP_PUSH_NONVOL/PUSH_MACHFRAME 展开码语义（0x1a1e0 段）
//!
//! 卡面：推栈族展开码——UWOP_PUSH_NONVOL（码 0，opinfo=寄存器 0..15，
//! RSP+8）、UWOP_PUSH_MACHFRAME（码 10，opinfo 0/1，机器帧 40/48 字节，
//! 至多一次且必须首码）。
//! 域：CoRun·UNX-D1-B07。
//!
//! 判据原文：
//! - J1 寄存器闭集：PUSH_NONVOL 的 opinfo ≤ 15，越码必拒。
//! - J2 机器帧口径：opinfo 0/1 对应 RSP+40/+48。
//! - J3 机器帧唯一：MACHFRAME 二次出现必拒。
//! - J4 机器帧首码：MACHFRAME 必须是第一个码，违者必拒。
//! - J5 坏机器帧参：MACHFRAME 的 opinfo > 1 必拒。
//! - J6 印账冻结：seal 后登记必拒。
//! - J7 注入五况全抓。
//!
//! 结构：PwCode 推栈码 + PwBook 码流册（含 RSP 增量模拟）+ 折叠指纹
//! （211 进制卷积）+ 注入核（越寄存器/双机器帧/机器帧非首/坏参/印后登记）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1a1e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1a1e0;
pub const PW_BAD_REG: u32 = SEG_BASE + 0x01; // 寄存器越闭集
pub const PW_MACH_TWICE: u32 = SEG_BASE + 0x02; // 机器帧双现
pub const PW_MACH_NOT_FIRST: u32 = SEG_BASE + 0x03; // 机器帧非首
pub const PW_BAD_MACH_OP: u32 = SEG_BASE + 0x04; // 坏机器帧参
pub const PW_FROZEN: u32 = SEG_BASE + 0x05; // 印账后登记

/// 展开码。
pub const OP_PUSH_NONVOL: u8 = 0;
pub const OP_PUSH_MACHFRAME: u8 = 10;

/// 单码。
#[derive(Clone, Copy)]
pub struct PwCode {
    pub opcode: u8,
    pub opinfo: u8,
}

impl PwCode {
    /// RSP 增量（推栈使 RSP 减，模型记绝对增量字节数）。
    pub fn rsp_delta(&self) -> Result<u32, u32> {
        match self.opcode {
            OP_PUSH_NONVOL => {
                if self.opinfo > 15 {
                    return Err(PW_BAD_REG);
                }
                Ok(8)
            }
            OP_PUSH_MACHFRAME => match self.opinfo {
                0 => Ok(40),
                1 => Ok(48),
                _ => Err(PW_BAD_MACH_OP),
            },
            _ => Err(PW_BAD_REG), // 本卡闭集外码按寄存器面拒
        }
    }
    pub fn render(&self) -> String {
        format!("P{:x}/{:x}", self.opcode, self.opinfo)
    }
}

/// 码流册。
pub struct PwBook {
    pub codes: Vec<PwCode>,
    pub rsp_total: u32,
    pub sealed: bool,
}

impl PwBook {
    pub fn new() -> PwBook {
        PwBook { codes: Vec::new(), rsp_total: 0, sealed: false }
    }

    /// 逐码入流：寄存器闭集 → 机器帧唯一/首码 → 增量累计。
    pub fn push_code(&mut self, c: PwCode) -> Result<u32, u32> {
        if self.sealed {
            return Err(PW_FROZEN);
        }
        let d = c.rsp_delta()?;
        if c.opcode == OP_PUSH_MACHFRAME {
            if self.codes.iter().any(|x| x.opcode == OP_PUSH_MACHFRAME) {
                return Err(PW_MACH_TWICE);
            }
            if !self.codes.is_empty() {
                return Err(PW_MACH_NOT_FIRST);
            }
        }
        self.codes.push(c);
        self.rsp_total += d;
        Ok(d)
    }

    /// 机器帧出现次数。
    pub fn mach_count(&self) -> usize {
        self.codes.iter().filter(|c| c.opcode == OP_PUSH_MACHFRAME).count()
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

    /// 诊断面：渲染行。
    pub fn line_of(&self, i: usize) -> String {
        match self.codes.get(i) {
            Some(c) => c.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = PwBook::new();
    // 1) 寄存器越闭集必拒。
    got[0] = b.push_code(PwCode { opcode: OP_PUSH_NONVOL, opinfo: 16 })
        .err() == Some(PW_BAD_REG);
    // 2) 机器帧双现必拒。
    let _ = b.push_code(PwCode { opcode: OP_PUSH_MACHFRAME, opinfo: 0 });
    got[1] = b.push_code(PwCode { opcode: OP_PUSH_MACHFRAME, opinfo: 0 })
        .err() == Some(PW_MACH_TWICE);
    // 3) 机器帧非首必拒。
    let mut b2 = PwBook::new();
    let _ = b2.push_code(PwCode { opcode: OP_PUSH_NONVOL, opinfo: 5 });
    got[2] = b2.push_code(PwCode { opcode: OP_PUSH_MACHFRAME, opinfo: 0 })
        .err() == Some(PW_MACH_NOT_FIRST);
    // 4) 坏机器帧参必拒。
    let mut b3 = PwBook::new();
    got[3] = b3.push_code(PwCode { opcode: OP_PUSH_MACHFRAME, opinfo: 2 })
        .err() == Some(PW_BAD_MACH_OP);
    // 5) 印账后登记必抓。
    b.seal();
    got[4] = b.push_code(PwCode { opcode: OP_PUSH_NONVOL, opinfo: 1 })
        .err() == Some(PW_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12126_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12126");

    // J1 寄存器闭集。
    let j1 = {
        let ok = PwCode { opcode: OP_PUSH_NONVOL, opinfo: 15 };
        let bad = PwCode { opcode: OP_PUSH_NONVOL, opinfo: 16 };
        ok.rsp_delta() == Ok(8) && bad.rsp_delta().err() == Some(PW_BAD_REG)
    };
    s.add("J1-寄存器闭集", j1, "15 过 16 拒");

    // J2 机器帧口径 40/48。
    let j2 = {
        let m0 = PwCode { opcode: OP_PUSH_MACHFRAME, opinfo: 0 };
        let m1 = PwCode { opcode: OP_PUSH_MACHFRAME, opinfo: 1 };
        m0.rsp_delta() == Ok(40) && m1.rsp_delta() == Ok(48)
    };
    s.add("J2-机器帧口径", j2, "opinfo 0/1 → 40/48");

    // J3 坏机器帧参拒。
    let j3 = {
        let m = PwCode { opcode: OP_PUSH_MACHFRAME, opinfo: 3 };
        m.rsp_delta().err() == Some(PW_BAD_MACH_OP)
    };
    s.add("J3-坏参拒", j3, "opinfo>1 必拒");

    // J4 机器帧首码放行且唯一。
    let j4 = {
        let mut b = PwBook::new();
        let _ = b.push_code(PwCode { opcode: OP_PUSH_MACHFRAME, opinfo: 1 });
        let _ = b.push_code(PwCode { opcode: OP_PUSH_NONVOL, opinfo: 4 });
        b.mach_count() == 1 && b.rsp_total == 56
    };
    s.add("J4-首码唯一", j4, "48+8=56");

    // J5 机器帧非首拒。
    let j5 = {
        let mut b = PwBook::new();
        let _ = b.push_code(PwCode { opcode: OP_PUSH_NONVOL, opinfo: 0 });
        b.push_code(PwCode { opcode: OP_PUSH_MACHFRAME, opinfo: 0 })
            .err() == Some(PW_MACH_NOT_FIRST)
    };
    s.add("J5-非首拒", j5, "机器帧必须首码");

    // J6 机器帧双现拒。
    let j6 = {
        let mut b = PwBook::new();
        let _ = b.push_code(PwCode { opcode: OP_PUSH_MACHFRAME, opinfo: 0 });
        b.push_code(PwCode { opcode: OP_PUSH_MACHFRAME, opinfo: 1 })
            .err() == Some(PW_MACH_TWICE)
    };
    s.add("J6-双现拒", j6, "二次机器帧必拒");

    // J7 多推累计。
    let j7 = {
        let mut b = PwBook::new();
        let _ = b.push_code(PwCode { opcode: OP_PUSH_NONVOL, opinfo: 5 });
        let _ = b.push_code(PwCode { opcode: OP_PUSH_NONVOL, opinfo: 6 });
        b.rsp_total == 16 && b.summary().contains("rsp=16")
    };
    s.add("J7-多推累计", j7, "2×8=16");

    // J8 渲染行。
    let j8 = {
        let mut b = PwBook::new();
        let _ = b.push_code(PwCode { opcode: OP_PUSH_NONVOL, opinfo: 12 });
        b.line_of(0).contains("P0/c") && b.line_of(1) == "missing"
    };
    s.add("J8-渲染行", j8, "码行含 opinfo 十六进制");

    // J9 印账冻结。
    let j9 = {
        let mut b = PwBook::new();
        b.seal();
        b.push_code(PwCode { opcode: OP_PUSH_NONVOL, opinfo: 0 })
            .err() == Some(PW_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后必拒");

    // J10 指纹幂等。
    let j10 = {
        let mut b = PwBook::new();
        let _ = b.push_code(PwCode { opcode: OP_PUSH_NONVOL, opinfo: 0 });
        b.fold() == b.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = PwBook::new();
    let _ = b.push_code(PwCode { opcode: OP_PUSH_MACHFRAME, opinfo: 0 });
    let _ = b.push_code(PwCode { opcode: OP_PUSH_NONVOL, opinfo: 3 });
    let _ = b.push_code(PwCode { opcode: OP_PUSH_NONVOL, opinfo: 6 });

    s.add("摘要回读", b.summary().contains("codes=3 rsp=56"), "40+8+8=56");
    s.add("指纹非零", b.fold() != 0, "指纹有效");

    // J12 寄存器闭集边界：0 与 15 放行、16 拒。
    let j12 = PwCode { opcode: OP_PUSH_NONVOL, opinfo: 0 }.rsp_delta() == Ok(8)
        && PwCode { opcode: OP_PUSH_NONVOL, opinfo: 15 }.rsp_delta() == Ok(8)
        && PwCode { opcode: OP_PUSH_NONVOL, opinfo: 16 }.rsp_delta().err() == Some(PW_BAD_REG);
    s.add("J12-寄存器闭集", j12, "0..15 闭、16 拒");

    // J13 opinfo=1 机器帧 48。
    let j13 = PwCode { opcode: OP_PUSH_MACHFRAME, opinfo: 1 }.rsp_delta() == Ok(48);
    s.add("J13-机器帧48", j13, "错误码叠加 8 字节");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12126_push_flow() {
        let mut b = PwBook::new();
        assert!(b.push_code(PwCode { opcode: OP_PUSH_MACHFRAME, opinfo: 0 }).is_ok());
        assert_eq!(
            b.push_code(PwCode { opcode: OP_PUSH_MACHFRAME, opinfo: 0 }),
            Err(PW_MACH_TWICE)
        );
        assert!(b.push_code(PwCode { opcode: OP_PUSH_NONVOL, opinfo: 15 }).is_ok());
    }

    #[test]
    fn unx12126_checks_green() {
        let s = run_unx12126_checks();
        assert!(s.all_passed(), "UNX-F12126 判据未全绿");
    }

    #[test]
    fn bad_reg_guard() {
        let c = PwCode { opcode: OP_PUSH_NONVOL, opinfo: 200 };
        assert_eq!(c.rsp_delta(), Err(PW_BAD_REG));
    }
}
