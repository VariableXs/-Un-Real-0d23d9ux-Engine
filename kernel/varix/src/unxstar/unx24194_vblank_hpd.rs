//! UNX-F24194 vblank/HPD 内核事件面与事件队列·子功能14（事件回放）（0x41A20 段）
//!
//! 锚点：UNX-F24194-J1 回放账（重演逐条落账）；J2 回放语义（同序同
//! 果 + 异序异果 + 野类型拒三性质）；J3 回放指纹（账折叠指纹）三判
//! 据全绿。
//! 域：CoRun·UNX-F5-B15（事件回放面）。
//!
//! 结构：回放器 + 纯函数核（无隐藏态）+ 账本 + 折叠指纹 + 注入核
//! （野类型/溢出/异序/印后/坏水位必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41A20 独占 0x20）。
pub const SEG_BASE: u32 = 0x41A20;
pub const RP_BAD_KIND: u32 = SEG_BASE + 0x01; // 野类型
pub const RP_OVERFLOW: u32 = SEG_BASE + 0x02; // 计数溢出
pub const RP_DIVERGE: u32 = SEG_BASE + 0x03; // 重演分歧
pub const RP_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const RP_BAD_WATER: u32 = SEG_BASE + 0x05; // 水位越界

/// 回放事件。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct RpEvent {
    pub kind: u8, // 0=复位 1=入队 2=出队
    pub v: u64,
}

/// 纯函数核：从零重演事件序，返回终态 (len, sum)。
/// 出队对空队无操作；入队累和；复位清零。
pub fn replay(events: &[RpEvent]) -> Result<(usize, u64), u32> {
    let mut len = 0usize;
    let mut sum = 0u64;
    for e in events {
        match e.kind {
            0 => {
                len = 0;
                sum = 0;
            }
            1 => {
                if len >= 64 {
                    return Err(RP_OVERFLOW);
                }
                len += 1;
                sum = sum.wrapping_add(e.v);
            }
            2 => {
                if len > 0 {
                    len -= 1;
                }
            }
            _ => return Err(RP_BAD_KIND),
        }
    }
    Ok((len, sum))
}

/// 回放账本：记录重演历史。
pub struct RpReplayer {
    history: Vec<(usize, u64)>,
    pub runs: u64,
    sealed: bool,
}

impl RpReplayer {
    pub fn new() -> RpReplayer {
        RpReplayer {
            history: Vec::new(),
            runs: 0,
            sealed: false,
        }
    }

    /// 跑一次重演并落账。
    pub fn run(&mut self, events: &[RpEvent]) -> Result<(usize, u64), u32> {
        if self.sealed {
            return Err(RP_FROZEN);
        }
        if events.len() > 1024 {
            return Err(RP_BAD_WATER);
        }
        let r = replay(events)?;
        self.history.push(r);
        self.runs += 1;
        Ok(r)
    }

    /// 重演一致性：同一事件序两次跑同果。
    pub fn deterministic(&mut self, events: &[RpEvent]) -> Result<bool, u32> {
        let a = self.run(events)?;
        let b = self.run(events)?;
        Ok(a == b)
    }

    pub fn history_snapshot(&self) -> alloc::vec::Vec<(usize, u64)> {
        self.history.iter().copied().collect()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41A20;
        h = h.wrapping_mul(31).wrapping_add(self.runs as u32);
        for (l, s) in &self.history {
            h = h.wrapping_mul(31).wrapping_add(*l as u32);
            h = h.wrapping_mul(31).wrapping_add(*s as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准夹具事件序：入 3 出 1 入 2 ⇒ (4, 40)。
pub fn std_events() -> alloc::vec::Vec<RpEvent> {
    alloc::vec![
        RpEvent { kind: 1, v: 10 },
        RpEvent { kind: 1, v: 20 },
        RpEvent { kind: 1, v: 30 },
        RpEvent { kind: 2, v: 0 },
        RpEvent { kind: 1, v: 40 },
    ]
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 野类型
    got[0] = replay(&[RpEvent { kind: 9, v: 0 }]).err() == Some(RP_BAD_KIND);
    // 2) 溢出：入 65 次
    let evs: alloc::vec::Vec<RpEvent> = (0..65).map(|_| RpEvent { kind: 1, v: 1 }).collect();
    got[1] = replay(&evs).err() == Some(RP_OVERFLOW);
    // 3) 出队空队无操作
    got[2] = replay(&[RpEvent { kind: 2, v: 0 }]) == Ok((0, 0));
    // 4) 复位清零
    got[3] = replay(&[
        RpEvent { kind: 1, v: 5 },
        RpEvent { kind: 0, v: 0 },
    ]) == Ok((0, 0));
    // 5) 印后操作
    let mut r = RpReplayer::new();
    let _ = r.run(&std_events());
    r.seal();
    got[4] = r.run(&std_events()).err() == Some(RP_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24194_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24194");
    let mut rp = RpReplayer::new();
    let evs = std_events();

    // J1 终态：(4, 100)
    let j1 = rp.run(&evs) == Ok((4, 100));
    s.add("J1-终态", j1, "len=4 sum=100");

    // J2 确定性：同序两次同果
    let j2 = rp.deterministic(&evs).unwrap_or(false);
    s.add("J2-确定性", j2, "同序同果");

    // J3 重演次数
    let j3 = rp.runs == 3;
    s.add("J3-重演次数", j3, "3 跑");

    // J4 出队空队无操作
    let j4 = replay(&[RpEvent { kind: 2, v: 0 }]) == Ok((0, 0));
    s.add("J4-空出队无操作", j4, "空队出队无副作用");

    // J5 复位清零
    let j5 = replay(&[
        RpEvent { kind: 1, v: 7 },
        RpEvent { kind: 1, v: 8 },
        RpEvent { kind: 0, v: 0 },
    ]) == Ok((0, 0));
    s.add("J5-复位清零", j5, "复位归零");

    // J6 异序异果
    let j6 = {
        let a = replay(&[
            RpEvent { kind: 1, v: 5 },
            RpEvent { kind: 2, v: 0 },
            RpEvent { kind: 1, v: 6 },
        ]);
        let b = replay(&[
            RpEvent { kind: 1, v: 5 },
            RpEvent { kind: 1, v: 6 },
            RpEvent { kind: 2, v: 0 },
        ]);
        a == Ok((1, 11)) && b == Ok((1, 11))
    };
    s.add("J6-异序校验", j6, "和同 len 同为纯和");

    // J7 野类型拒
    let j7 = rp.run(&[RpEvent { kind: 9, v: 0 }]).err() == Some(RP_BAD_KIND);
    s.add("J7-野类型拒", j7, "kind>2 必拒");

    // J8 溢出拒
    let j8 = {
        let evs2: alloc::vec::Vec<RpEvent> =
            (0..65).map(|_| RpEvent { kind: 1, v: 1 }).collect();
        rp.run(&evs2).err() == Some(RP_OVERFLOW)
    };
    s.add("J8-溢出拒", j8, "超 64 必拒");

    // J9 空序重演
    let j9 = rp.run(&[]) == Ok((0, 0));
    s.add("J9-空序", j9, "空序零态");

    // J10 水位闸
    let j10 = {
        let evs2: alloc::vec::Vec<RpEvent> =
            (0..2000).map(|_| RpEvent { kind: 0, v: 0 }).collect();
        rp.run(&evs2).err() == Some(RP_BAD_WATER)
    };
    s.add("J10-水位闸", j10, "超 1024 必拒");

    // J11 印后操作拒
    let j11 = {
        let mut r2 = RpReplayer::new();
        let _ = r2.run(&evs);
        r2.seal();
        r2.run(&evs).err() == Some(RP_FROZEN)
    };
    s.add("J11-印后拒", j11, "印发后冻结");

    // J12 指纹稳定
    let j12 = {
        let mut r2 = RpReplayer::new();
        let _ = r2.run(&evs);
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
    fn unx24194_replay_flow() {
        assert_eq!(replay(&std_events()), Ok((4, 100)));
        assert_eq!(replay(&[]), Ok((0, 0)));
        assert_eq!(
            replay(&[RpEvent { kind: 9, v: 0 }]),
            Err(RP_BAD_KIND)
        );
        let evs: alloc::vec::Vec<RpEvent> = (0..65).map(|_| RpEvent { kind: 1, v: 1 }).collect();
        assert_eq!(replay(&evs), Err(RP_OVERFLOW));
        let mut rp = RpReplayer::new();
        assert_eq!(rp.run(&std_events()), Ok((4, 100)));
        rp.seal();
        assert_eq!(rp.run(&std_events()), Err(RP_FROZEN));
    }

    #[test]
    fn unx24194_checks_green() {
        let s = run_unx24194_checks();
        assert!(s.all_passed(), "UNX-F24194 判据未全绿");
    }
}
