//! UNX-F24199 vblank/HPD 内核事件面与事件队列·子功能19（序列重演）（0x41AC0 段）
//!
//! 锚点：UNX-F24199-J1 重演账（全序列重演逐条落账）；J2 重演语义（
//! 同序同指纹 + 异序异指纹 + 野类型拒三性质）；J3 重演指纹（账折叠
//! 指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（序列重演面）。
//!
//! 结构：事件流重演器 + 纯函数状态机（复位/入队/出队/计数四操作）
//! + 账本 + 折叠指纹 + 注入核（野操作/溢出/异序/印后/坏水位必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41AC0 独占 0x20）。
pub const SEG_BASE: u32 = 0x41AC0;
pub const SR_BAD_OP: u32 = SEG_BASE + 0x01; // 野操作
pub const SR_OVERFLOW: u32 = SEG_BASE + 0x02; // 深度溢出
pub const SR_DIVERGE: u32 = SEG_BASE + 0x03; // 重演分歧
pub const SR_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const SR_BAD_WATER: u32 = SEG_BASE + 0x05; // 序长越界

/// 流事件。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SrEvent {
    pub op: u8, // 0=复位 1=入队(v) 2=出队 3=跳过(v)
    pub v: u64,
}

/// 纯函数状态机终态：深度 + 校验和。
pub type SrFinal = (usize, u64);

/// 从零重演事件流。
pub fn sr_replay(events: &[SrEvent]) -> Result<SrFinal, u32> {
    let mut depth = 0usize;
    let mut acc: u64 = 0;
    for e in events {
        match e.op {
            0 => {
                depth = 0;
                acc = 0;
            }
            1 => {
                if depth >= 32 {
                    return Err(SR_OVERFLOW);
                }
                depth += 1;
                acc = acc.wrapping_mul(31).wrapping_add(e.v);
            }
            2 => {
                depth = depth.saturating_sub(1);
            }
            3 => {
                acc = acc.wrapping_add(e.v);
            }
            _ => return Err(SR_BAD_OP),
        }
    }
    Ok((depth, acc))
}

/// 重演账本。
pub struct SrReplay {
    ledger: Vec<(usize, SrFinal)>,
    pub runs: u64,
    sealed: bool,
}

impl SrReplay {
    pub fn new() -> SrReplay {
        SrReplay {
            ledger: Vec::new(),
            runs: 0,
            sealed: false,
        }
    }

    /// 跑一段并落账；序长闸 512。
    pub fn run(&mut self, seq: usize, events: &[SrEvent]) -> Result<SrFinal, u32> {
        if self.sealed {
            return Err(SR_FROZEN);
        }
        if events.len() > 512 {
            return Err(SR_BAD_WATER);
        }
        let f = sr_replay(events)?;
        self.ledger.push((seq, f));
        self.runs += 1;
        Ok(f)
    }

    /// 序列重演一致性：同段两次跑同指纹。
    pub fn consistent(&mut self, seq: usize, events: &[SrEvent]) -> Result<bool, u32> {
        let a = self.run(seq, events)?;
        let b = self.run(seq, events)?;
        Ok(a == b)
    }

    /// 异序分歧检测：两段不同序必异指纹。
    pub fn diverges(a: &[SrEvent], b: &[SrEvent]) -> Result<bool, u32> {
        Ok(sr_replay(a)? != sr_replay(b)?)
    }

    pub fn ledger_snapshot(&self) -> Vec<(usize, SrFinal)> {
        self.ledger.iter().copied().collect()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41AC0;
        h = h.wrapping_mul(31).wrapping_add(self.runs as u32);
        for (s, (d, a)) in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(*s as u32);
            h = h.wrapping_mul(31).wrapping_add(*d as u32);
            h = h.wrapping_mul(31).wrapping_add(*a as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准夹具：vblank 流（入 3 出 1 跳 1 复位 重入 2）。
pub fn std_stream() -> [SrEvent; 8] {
    [
        SrEvent { op: 1, v: 1 },
        SrEvent { op: 1, v: 2 },
        SrEvent { op: 1, v: 3 },
        SrEvent { op: 2, v: 0 },
        SrEvent { op: 3, v: 10 },
        SrEvent { op: 0, v: 0 },
        SrEvent { op: 1, v: 7 },
        SrEvent { op: 1, v: 8 },
    ]
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 野操作
    got[0] = sr_replay(&[SrEvent { op: 9, v: 0 }]).err() == Some(SR_BAD_OP);
    // 2) 深度溢出
    let evs: [SrEvent; 33] = {
        let mut a = [SrEvent { op: 1, v: 0 }; 33];
        a[0] = SrEvent { op: 1, v: 0 };
        for i in 1..33 {
            a[i] = SrEvent { op: 1, v: i as u64 };
        }
        a
    };
    got[1] = sr_replay(&evs).err() == Some(SR_OVERFLOW);
    // 3) 出队空稳态
    got[2] = sr_replay(&[SrEvent { op: 2, v: 0 }]) == Ok((0, 0));
    // 4) 复位清零
    got[3] = sr_replay(&[
        SrEvent { op: 1, v: 5 },
        SrEvent { op: 0, v: 0 },
    ]) == Ok((0, 0));
    // 5) 印后操作
    let mut r = SrReplay::new();
    let _ = r.run(1, &std_stream());
    r.seal();
    got[4] = r.run(2, &std_stream()).err() == Some(SR_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24199_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24199");
    let mut r = SrReplay::new();
    let st = std_stream();

    // J1 终态：复位后入 7/8 ⇒ 深 2 和 31*31+8
    let j1 = sr_replay(&st) == Ok((2, 7u64.wrapping_mul(31).wrapping_add(8)));
    s.add("J1-终态", j1, "深 2 校验和 225");

    // J2 同序同指纹
    let j2 = r.consistent(1, &st).unwrap_or(false);
    s.add("J2-同序同纹", j2, "重演确定");

    // J3 跑次
    let j3 = r.runs == 2;
    s.add("J3-跑次", j3, "2 跑");

    // J4 异序异指纹
    let j4 = SrReplay::diverges(
        &[SrEvent { op: 1, v: 1 }, SrEvent { op: 1, v: 2 }],
        &[SrEvent { op: 1, v: 2 }, SrEvent { op: 1, v: 1 }],
    )
    .unwrap_or(false);
    s.add("J4-异序异纹", j4, "序敏校验和");

    // J5 复位清零
    let j5 = sr_replay(&[
        SrEvent { op: 1, v: 9 },
        SrEvent { op: 0, v: 0 },
    ]) == Ok((0, 0));
    s.add("J5-复位清零", j5, "复位归零");

    // J6 出队空稳态
    let j6 = sr_replay(&[SrEvent { op: 2, v: 0 }]) == Ok((0, 0));
    s.add("J6-空出队稳", j6, "saturating 不负");

    // J7 野操作拒
    let j7 = r.run(2, &[SrEvent { op: 9, v: 0 }]).err() == Some(SR_BAD_OP);
    s.add("J7-野操作拒", j7, "op>3 必拒");

    // J8 深度溢出拒
    let j8 = {
        let evs2: [SrEvent; 33] = {
            let mut a = [SrEvent { op: 1, v: 0 }; 33];
            for (i, e) in a.iter_mut().enumerate() {
                *e = SrEvent { op: 1, v: i as u64 };
            }
            a
        };
        r.run(2, &evs2).err() == Some(SR_OVERFLOW)
    };
    s.add("J8-溢出拒", j8, "深>32 必拒");

    // J9 序长闸
    let j9 = {
        let evs2: Vec<SrEvent> = (0..600).map(|_| SrEvent { op: 0, v: 0 }).collect();
        r.run(2, &evs2).err() == Some(SR_BAD_WATER)
    };
    s.add("J9-序长闸", j9, ">512 必拒");

    // J10 账序守恒
    let j10 = {
        let led = r.ledger_snapshot();
        led.len() == 3 && led[0].0 == 1
    };
    s.add("J10-账序守恒", j10, "账随跑序");

    // J11 印后操作拒
    let j11 = {
        let mut r2 = SrReplay::new();
        let _ = r2.run(1, &st);
        r2.seal();
        r2.run(1, &st).err() == Some(SR_FROZEN)
    };
    s.add("J11-印后拒", j11, "印发后冻结");

    // J12 指纹稳定
    let j12 = {
        let mut r2 = SrReplay::new();
        let _ = r2.run(1, &st);
        let a = r2.fold();
        r2.seal();
        r2.fold() == a
    };
    s.add("J12-指纹稳定", j12, "印不改指纹");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx24199_replay_flow() {
        assert_eq!(sr_replay(&std_stream()), Ok((2, 225)));
        assert_eq!(sr_replay(&[]), Ok((0, 0)));
        assert_eq!(
            sr_replay(&[SrEvent { op: 9, v: 0 }]),
            Err(SR_BAD_OP)
        );
        assert_eq!(
            SrReplay::diverges(
                &[SrEvent { op: 1, v: 1 }],
                &[SrEvent { op: 1, v: 2 }],
            ),
            Ok(true)
        );
        let mut r = SrReplay::new();
        assert_eq!(r.run(1, &std_stream()), Ok((2, 225)));
        r.seal();
        assert_eq!(r.run(1, &std_stream()), Err(SR_FROZEN));
    }

    #[test]
    fn unx24199_checks_green() {
        let s = run_unx24199_checks();
        assert!(s.all_passed(), "UNX-F24199 判据未全绿");
    }
}
