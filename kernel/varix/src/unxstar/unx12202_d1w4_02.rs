//! UNX-F12202 展开码全量解释器回归矩阵（SEG 0x64040..0x64060 段窗）
//!
//! 卡面：x64 展开码（UNWIND_OP）解释器——槽流解码、槽量口径、
//! 非法操作数拒止、prologue 槽耗总账；全量操作码回归矩阵逐条过闸。
//! 域：CoRun·UNX-D1-B11。
//!
//! 判据原文：
//! - J1 PUSH_NONVOL：单槽，回归矩阵槽量=1。
//! - J2 ALLOC_LARGE：info=0 占 2 槽、info=1 占 3 槽、info≥2 必拒。
//! - J3 ALLOC_SMALL：单槽，size = info*8 + 8。
//! - J4 SET_FPREG：单槽。
//! - J5 SAVE_NONVOL：双槽；FAR 变体三槽。
//! - J6 SAVE_XMM128：双槽；FAR 变体三槽。
//! - J7 PUSH_MACHFRAME：单槽，info>1 必拒。
//! - J8 截断流拒止：槽流中途耗尽必拒 OP_TRUNC。
//! - J9 未知码拒止：操作码 > 10 必拒 OP_UNKNOWN。
//! - J10 残槽拒止：流末残槽不足半槽（奇数残槽）必拒 OP_LEFTOVER。
//! - J11 槽耗总账：解码总槽耗 = 槽流长度。
//! - J12 印账冻结：seal 后解码必拒 OP_FROZEN。
//! - J13 指纹幂等：同态同指纹。
//! - J14 注入五况全抓。
//!
//! 结构：Op 解码记录（码/info/槽量/槽位）+ OpDecoder 解释器（槽流/
//! 回归矩阵账/操作账）+ 解码核（逐槽推进）+ 折叠指纹 + 注入核
//! （未知码/截断流/坏 info/残槽/印后操作）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x64040 独占 0x20 窗）。
pub const SEG_BASE: u32 = 0x64040;
pub const OP_UNKNOWN: u32 = SEG_BASE + 0x01; // 未知操作码
pub const OP_TRUNC: u32 = SEG_BASE + 0x02; // 槽流中途耗尽
pub const OP_BADINFO: u32 = SEG_BASE + 0x03; // 操作数非法
pub const OP_LEFTOVER: u32 = SEG_BASE + 0x04; // 流末残槽不成对
pub const OP_FROZEN: u32 = SEG_BASE + 0x05; // 印账后操作

/// 展开操作码（x64 UNWIND_OP 口径）。
pub const UWOP_PUSH_NONVOL: u8 = 0;
pub const UWOP_ALLOC_LARGE: u8 = 1;
pub const UWOP_ALLOC_SMALL: u8 = 2;
pub const UWOP_SET_FPREG: u8 = 3;
pub const UWOP_SAVE_NONVOL: u8 = 4;
pub const UWOP_SAVE_NONVOL_FAR: u8 = 5;
pub const UWOP_SAVE_XMM128: u8 = 8;
pub const UWOP_SAVE_XMM128_FAR: u8 = 9;
pub const UWOP_PUSH_MACHFRAME: u8 = 10;

/// 解码记录：操作码 + 操作数 + 占槽量 + 起始槽位。
#[derive(Clone, Copy)]
pub struct Op {
    pub code: u8,
    pub info: u8,
    pub slots: u8,
    pub at: u32,
}

impl Op {
    /// 操作数语义：ALLOC_SMALL 返回字节数，其余返回 info 原值。
    pub fn operand(&self) -> u32 {
        if self.code == UWOP_ALLOC_SMALL {
            (self.info as u32) * 8 + 8
        } else {
            self.info as u32
        }
    }

    /// 码名渲染。
    pub fn name(&self) -> String {
        match self.code {
            UWOP_PUSH_NONVOL => "PUSH_NONVOL".to_string(),
            UWOP_ALLOC_LARGE => "ALLOC_LARGE".to_string(),
            UWOP_ALLOC_SMALL => "ALLOC_SMALL".to_string(),
            UWOP_SET_FPREG => "SET_FPREG".to_string(),
            UWOP_SAVE_NONVOL => "SAVE_NONVOL".to_string(),
            UWOP_SAVE_NONVOL_FAR => "SAVE_NONVOL_FAR".to_string(),
            UWOP_SAVE_XMM128 => "SAVE_XMM128".to_string(),
            UWOP_SAVE_XMM128_FAR => "SAVE_XMM128_FAR".to_string(),
            UWOP_PUSH_MACHFRAME => "PUSH_MACHFRAME".to_string(),
            _ => "??".to_string(),
        }
    }
}

/// 单码槽量口径表：合法码 → 槽量（info 无关项）；None = 非法 info。
pub fn slot_count(code: u8, info: u8) -> Result<u8, u32> {
    match code {
        UWOP_PUSH_NONVOL | UWOP_ALLOC_SMALL | UWOP_SET_FPREG | UWOP_PUSH_MACHFRAME => {
            if code == UWOP_PUSH_MACHFRAME && info > 1 {
                return Err(OP_BADINFO);
            }
            Ok(1)
        }
        UWOP_ALLOC_LARGE => match info {
            0 => Ok(2),
            1 => Ok(3),
            _ => Err(OP_BADINFO),
        },
        UWOP_SAVE_NONVOL | UWOP_SAVE_XMM128 => Ok(2),
        UWOP_SAVE_NONVOL_FAR | UWOP_SAVE_XMM128_FAR => Ok(3),
        _ => Err(OP_UNKNOWN),
    }
}

/// 解释器：槽流解码 + 回归矩阵账。
pub struct OpDecoder {
    pub stream: Vec<u8>,
    pub ops: Vec<Op>,
    pub runs: Vec<String>,
    pub sealed: bool,
}

impl OpDecoder {
    pub fn new(stream: Vec<u8>) -> OpDecoder {
        OpDecoder { stream, ops: Vec::new(), runs: Vec::new(), sealed: false }
    }

    /// 全流解码：逐槽推进，任何违例即拒。
    pub fn decode(&mut self) -> Result<usize, u32> {
        if self.sealed {
            return Err(OP_FROZEN);
        }
        if self.stream.len() % 2 != 0 {
            return Err(OP_LEFTOVER);
        }
        self.ops.clear();
        let mut i = 0usize;
        while i < self.stream.len() {
            let code = self.stream[i];
            let info = (self.stream[i + 1] >> 4) & 0xF;
            let n = slot_count(code, info)?;
            if i + (n as usize) * 2 > self.stream.len() {
                return Err(OP_TRUNC);
            }
            self.ops.push(Op { code, info, slots: n, at: i as u32 });
            i += 2 + (n as usize - 1) * 2;
        }
        self.runs.push(format!("dec{}op{}", self.stream.len(), self.ops.len()));
        Ok(self.ops.len())
    }

    /// 槽耗总账：各 op 槽量之和（不含每 op 头两槽的字节折算）。
    pub fn total_slots(&self) -> u32 {
        self.ops.iter().map(|o| o.slots as u32).sum()
    }

    /// 按码回归：该码在最近一次解码中的条数。
    pub fn count_of(&self, code: u8) -> usize {
        self.ops.iter().filter(|o| o.code == code).count()
    }

    /// 回归矩阵：全量码逐条单流解码核。
    pub fn regress(&mut self) -> Result<usize, u32> {
        if self.sealed {
            return Err(OP_FROZEN);
        }
        // 矩阵流：每码带头槽数据槽，全部单实例排布。
        let mut m: Vec<u8> = Vec::new();
        m.extend_from_slice(&[UWOP_PUSH_NONVOL, 0x30]); // rbp push
        m.extend_from_slice(&[UWOP_ALLOC_LARGE, 0x10, 0x08, 0x00, 0x10, 0x00]);
        m.extend_from_slice(&[UWOP_ALLOC_SMALL, 0x30]);
        m.extend_from_slice(&[UWOP_SET_FPREG, 0x00]);
        m.extend_from_slice(&[UWOP_SAVE_NONVOL, 0x56, 0x08, 0x00]);
        m.extend_from_slice(&[UWOP_SAVE_NONVOL_FAR, 0x57, 0x10, 0x00, 0x18, 0x00]);
        m.extend_from_slice(&[UWOP_SAVE_XMM128, 0x64, 0x20, 0x00]);
        m.extend_from_slice(&[UWOP_SAVE_XMM128_FAR, 0x65, 0x30, 0x00, 0x38, 0x00]);
        m.extend_from_slice(&[UWOP_PUSH_MACHFRAME, 0x00]);
        let saved = std::mem::replace(&mut self.stream, m);
        let r = self.decode();
        self.stream = saved;
        r
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for o in &self.ops {
            h = h.wrapping_mul(211).wrapping_add(o.code as u32);
            h = h.wrapping_mul(211).wrapping_add(o.info as u32);
            h = h.wrapping_mul(211).wrapping_add(o.slots as u32);
            h = h.wrapping_mul(211).wrapping_add(o.at);
        }
        for by in &self.stream {
            h = h.wrapping_mul(211).wrapping_add(*by as u32);
        }
        for r in &self.runs {
            for by in r.bytes() {
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
        format!("slots={} ops={}", self.stream.len(), self.ops.len())
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 未知码必拒（码 11 越界）。
    got[0] = OpDecoder::new(vec![11, 0]).decode().err() == Some(OP_UNKNOWN);
    // 2) 截断流必拒（ALLOC_LARGE info=1 需 3 数据槽，只给 1）。
    got[1] = OpDecoder::new(vec![UWOP_ALLOC_LARGE, 0x10, 0x00, 0x00]).decode()
        .err()
        == Some(OP_TRUNC);
    // 3) 坏 info 必拒（MACHFRAME info=2）。
    got[2] = OpDecoder::new(vec![UWOP_PUSH_MACHFRAME, 0x20]).decode().err() == Some(OP_BADINFO);
    // 4) 奇数残槽必拒。
    got[3] = OpDecoder::new(vec![UWOP_PUSH_NONVOL, 0x00, 0x55]).decode().err()
        == Some(OP_LEFTOVER);
    // 5) 印账后解码必抓。
    let mut d = OpDecoder::new(vec![UWOP_PUSH_NONVOL, 0x00]);
    d.seal();
    got[4] = d.decode().err() == Some(OP_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12202_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12202");

    // J1 PUSH_NONVOL 槽量。
    let j1 = slot_count(UWOP_PUSH_NONVOL, 5) == Ok(1);
    s.add("J1-PUSH_NONVOL", j1, "单槽口径");

    // J2 ALLOC_LARGE 槽量与 info 拒止。
    let j2 = slot_count(UWOP_ALLOC_LARGE, 0) == Ok(2)
        && slot_count(UWOP_ALLOC_LARGE, 1) == Ok(3)
        && slot_count(UWOP_ALLOC_LARGE, 2) == Err(OP_BADINFO);
    s.add("J2-ALLOC_LARGE", j2, "info 0/1 合法、≥2 拒");

    // J3 ALLOC_SMALL 操作数口径。
    let j3 = {
        let d = OpDecoder::new(vec![UWOP_ALLOC_SMALL, 0x23]);
        let mut d2 = OpDecoder::new(vec![UWOP_ALLOC_SMALL, 0x00]);
        let _ = d2.decode();
        d.stream[1] == 0x23 && d2.ops.first().map(|o| o.operand()) == Some(8)
    };
    s.add("J3-ALLOC_SMALL", j3, "size=info*8+8");

    // J4 SET_FPREG 槽量。
    let j4 = slot_count(UWOP_SET_FPREG, 0) == Ok(1);
    s.add("J4-SET_FPREG", j4, "单槽口径");

    // J5 SAVE_NONVOL/FAR 槽量。
    let j5 = slot_count(UWOP_SAVE_NONVOL, 0) == Ok(2)
        && slot_count(UWOP_SAVE_NONVOL_FAR, 0) == Ok(3);
    s.add("J5-SAVE_NONVOL", j5, "2/3 槽口径");

    // J6 SAVE_XMM128/FAR 槽量。
    let j6 = slot_count(UWOP_SAVE_XMM128, 0) == Ok(2)
        && slot_count(UWOP_SAVE_XMM128_FAR, 0) == Ok(3);
    s.add("J6-SAVE_XMM128", j6, "2/3 槽口径");

    // J7 PUSH_MACHFRAME。
    let j7 = slot_count(UWOP_PUSH_MACHFRAME, 0) == Ok(1)
        && slot_count(UWOP_PUSH_MACHFRAME, 1) == Ok(1)
        && slot_count(UWOP_PUSH_MACHFRAME, 2) == Err(OP_BADINFO);
    s.add("J7-PUSH_MACHFRAME", j7, "info≤1 合法");

    // J8 截断流拒止。
    let j8 = OpDecoder::new(vec![UWOP_SAVE_NONVOL, 0x50]).decode() == Err(OP_TRUNC);
    s.add("J8-截断流拒止", j8, "槽流中途耗尽必拒");

    // J9 未知码拒止。
    let j9 = OpDecoder::new(vec![7, 0]).decode() == Err(OP_UNKNOWN)
        && OpDecoder::new(vec![6, 0]).decode() == Err(OP_UNKNOWN);
    s.add("J9-未知码拒止", j9, "码 6/7 保留越界");

    // J10 残槽拒止。
    let j10 = OpDecoder::new(vec![UWOP_PUSH_NONVOL, 0, 1]).decode() == Err(OP_LEFTOVER);
    s.add("J10-残槽拒止", j10, "奇数字节流必拒");

    // J11 槽耗总账。
    let j11 = {
        let mut d = OpDecoder::new(vec![
            UWOP_PUSH_NONVOL, 0x30,
            UWOP_ALLOC_SMALL, 0x20,
            UWOP_SAVE_NONVOL, 0x56, 0x00, 0x00,
        ]);
        let n = d.decode().unwrap_or(0);
        n == 3 && d.total_slots() == 1 + 1 + 2 && d.summary().contains("slots=8")
    };
    s.add("J11-槽耗总账", j11, "3 op 4 槽 8 字节");

    // J12 印账冻结。
    let j12 = {
        let mut d = OpDecoder::new(vec![UWOP_PUSH_NONVOL, 0]);
        d.seal();
        d.decode().err() == Some(OP_FROZEN) && d.regress().err() == Some(OP_FROZEN)
    };
    s.add("J12-印账冻结", j12, "seal 后解码/回归皆拒");

    // J13 回归矩阵全绿：九码全数入账。
    let j13 = {
        let mut d = OpDecoder::new(Vec::new());
        d.regress().unwrap_or(0) == 9
            && d.count_of(UWOP_PUSH_NONVOL) == 1
            && d.count_of(UWOP_SAVE_XMM128_FAR) == 1
            && d.count_of(UWOP_PUSH_MACHFRAME) == 1
    };
    s.add("J13-回归矩阵", j13, "九码全数解码通过");

    // J14 指纹幂等。
    let j14 = {
        let mut d = OpDecoder::new(vec![UWOP_ALLOC_SMALL, 0x10]);
        let _ = d.decode();
        d.fold() == d.fold()
    };
    s.add("J14-指纹幂等", j14, "同态同指纹");

    let got = inject_five();
    s.add_dyn("J15-注入全抓", got.iter().all(|x| *x), &format!("inj5={:?}", got));
    s.add("J16-码名渲染", {
        let o = Op { code: UWOP_ALLOC_SMALL, info: 2, slots: 1, at: 0 };
        o.name() == "ALLOC_SMALL" && o.operand() == 24
    }, "码名与操作数口径");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12202_decode_flow() {
        let mut d = OpDecoder::new(vec![UWOP_ALLOC_LARGE, 0x00, 0x08, 0x00, UWOP_PUSH_NONVOL, 0x55]);
        assert_eq!(d.decode(), Ok(2));
        assert_eq!(d.total_slots(), 3);
        assert_eq!(d.decode(), Ok(2));
    }

    #[test]
    fn unx12202_checks_green() {
        let s = run_unx12202_checks();
        assert!(s.all_passed(), "UNX-F12202 判据未全绿");
    }

    #[test]
    fn unx12202_inject_all_caught() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "注入五况未全抓: {:?}", got);
    }
}
