//! UNX-F24154 reservation 锁与显存互斥纪律·子功能14（优先级继承账）（0x41520 段）
//!
//! 锚点：UNX-F24154-J1 继承账（优先级抬升与归还逐条落账）；J2 继承
//! 语义（高等待抬低 + 释放归还 + 非传递闸三性质）；J3 继承指纹（账
//! 折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（优先级继承面）。
//!
//! 结构：任务优先级表 + 继承抬升核 + 归还核 + 账本 + 折叠指纹 + 注
//! 入核（降抬/野任务/重抬/印后/伪归还必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41520 独占 0x20）。
pub const SEG_BASE: u32 = 0x41520;
pub const PI_UNK_TASK: u32 = SEG_BASE + 0x01; // 野任务
pub const PI_NO_BOOST: u32 = SEG_BASE + 0x02; // 无需抬升
pub const PI_FAKE_RETURN: u32 = SEG_BASE + 0x03; // 伪归还
pub const PI_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const PI_DUP_BOOST: u32 = SEG_BASE + 0x05; // 重复抬升

/// 任务条目。
#[derive(Clone)]
pub struct PiTask {
    pub name: String,
    pub base_prio: u8, // 数字越小优先级越高
    pub eff_prio: u8,
}

/// 继承账。
#[derive(Clone)]
pub struct PiRec {
    pub seq: u32,
    pub low: String,
    pub high: String,
    pub boosted_to: u8,
    pub returned: bool,
}

/// 继承簿。
pub struct PiBook {
    pub tasks: Vec<PiTask>,
    pub ledger: Vec<PiRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl PiBook {
    pub fn new() -> PiBook {
        PiBook { tasks: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn declare(&mut self, name: &str, prio: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(PI_FROZEN);
        }
        if name.is_empty() || self.tasks.iter().any(|t| t.name == name) {
            return Err(PI_UNK_TASK);
        }
        self.tasks.push(PiTask { name: name.to_string(), base_prio: prio, eff_prio: prio });
        Ok(())
    }

    fn prio_of(&self, name: &str) -> u8 {
        self.tasks.iter().find(|t| t.name == name).map(|t| t.eff_prio).unwrap_or(u8::MAX)
    }

    /// 高优先级等低优先级持锁：抬升 low 的有效优先级到 high 的。
    pub fn boost(&mut self, low: &str, high: &str) -> Result<u8, u32> {
        if self.sealed {
            return Err(PI_FROZEN);
        }
        let hp = self.prio_of(high);
        let lp = self.prio_of(low);
        if hp == u8::MAX || lp == u8::MAX {
            return Err(PI_UNK_TASK);
        }
        if hp >= lp {
            return Err(PI_NO_BOOST); // 高者不更高：无需抬升
        }
        if self.ledger.iter().any(|r| r.low == low && !r.returned) {
            return Err(PI_DUP_BOOST);
        }
        if let Some(t) = self.tasks.iter_mut().find(|t| t.name == low) {
            t.eff_prio = hp;
        }
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(PiRec { seq, low: low.to_string(), high: high.to_string(), boosted_to: hp, returned: false });
        Ok(hp)
    }

    /// 释放归还：eff 恢复 base。
    pub fn hand_back(&mut self, low: &str) -> Result<u8, u32> {
        if self.sealed {
            return Err(PI_FROZEN);
        }
        let rec = match self.ledger.iter_mut().find(|r| r.low == low && !r.returned) {
            Some(r) => r,
            None => return Err(PI_FAKE_RETURN),
        };
        rec.returned = true;
        let base = self.tasks.iter().find(|t| t.name == low).map(|t| t.base_prio).unwrap_or(u8::MAX);
        if let Some(t) = self.tasks.iter_mut().find(|t| t.name == low) {
            t.eff_prio = base;
        }
        Ok(base)
    }

    pub fn boosted_active(&self) -> usize {
        self.ledger.iter().filter(|r| !r.returned).count()
    }

    pub fn seq_unique(&self) -> bool {
        for i in 0..self.ledger.len() {
            for j in (i + 1)..self.ledger.len() {
                if self.ledger[i].seq == self.ledger[j].seq {
                    return false;
                }
            }
        }
        true
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41520;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.boosted_to as u32);
            h = h.wrapping_mul(31).wrapping_add(r.returned as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：三任务 prio 1/3/5，一次抬升未归还。
pub fn mk_book() -> PiBook {
    let mut b = PiBook::new();
    let _ = b.declare("high", 1);
    let _ = b.declare("mid", 3);
    let _ = b.declare("low", 5);
    let _ = b.boost("low", "high"); // low eff→1
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 野任务
    got[0] = b.boost("ghost", "high").err() == Some(PI_UNK_TASK);
    // 2) 无需抬升（high 不更高）
    got[1] = b.boost("high", "low").err() == Some(PI_NO_BOOST);
    // 3) 重复抬升（未归还再抬）
    got[2] = b.boost("low", "mid").err() == Some(PI_DUP_BOOST);
    // 4) 伪归还
    let mut b2 = PiBook::new();
    let _ = b2.declare("a", 1);
    got[3] = b2.hand_back("a").err() == Some(PI_FAKE_RETURN);
    // 5) 印后操作必抓
    b.seal();
    got[4] = b.boost("mid", "high").err() == Some(PI_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24154_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24154");
    let mut b = mk_book();

    // J1 任务账齐
    let j1 = b.tasks.len() == 3;
    s.add("J1-任务账齐", j1, "三任务全落表");

    // J2 抬升生效
    let j2 = b.prio_of("low") == 1;
    s.add("J2-抬升生效", j2, "low eff→1");

    // J3 抬升不越 base 高位
    let j3 = {
        let t = b.tasks.iter().find(|t| t.name == "low").unwrap();
        t.base_prio == 5 && t.eff_prio <= t.base_prio
    };
    s.add("J3-不越base", j3, "eff ≤ base（数值小=高）");

    // J4 无需抬升拒
    let j4 = b.boost("high", "low").err() == Some(PI_NO_BOOST);
    s.add("J4-无需抬升拒", j4, "高者不更高必拒");

    // J5 重复抬升拒
    let j5 = b.boost("low", "mid").err() == Some(PI_DUP_BOOST);
    s.add("J5-重复抬升拒", j5, "未归还再抬必拒");

    // J6 归还恢复 base
    let j6 = {
        b.hand_back("low").is_ok() && b.prio_of("low") == 5
    };
    s.add("J6-归还恢复", j6, "归还后 eff=base");

    // J7 伪归还拒
    let j7 = b.hand_back("low").err() == Some(PI_FAKE_RETURN);
    s.add("J7-伪归还拒", j7, "无在抬记录必拒");

    // J8 野任务拒
    let j8 = b.boost("ghost", "high").err() == Some(PI_UNK_TASK);
    s.add("J8-野任务拒", j8, "未声明任务必拒");

    // J9 重名声明拒
    let j9 = b.declare("high", 2).err() == Some(PI_UNK_TASK);
    s.add("J9-重名拒", j9, "任务重名必拒");

    // J10 归还后可再抬
    let j10 = {
        let mut b2 = mk_book();
        let _ = b2.hand_back("low");
        b2.boost("low", "high").is_ok()
    };
    s.add("J10-归还再抬", j10, "归还后重新可抬");

    // J11 在抬计数
    let j11 = b.boosted_active() == 0;
    s.add("J11-在抬计数", j11, "归还后在抬归零");

    // J12 指纹稳定 + seq 唯一
    let j12 = b.fold() == b.fold() && b.seq_unique();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致且 seq 唯一");

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
    fn unx24154_pi_flow() {
        let mut b = PiBook::new();
        let _ = b.declare("h", 1);
        let _ = b.declare("l", 5);
        assert_eq!(b.boost("l", "h"), Ok(1));
        assert_eq!(b.boost("l", "h"), Err(PI_DUP_BOOST));
        assert_eq!(b.hand_back("l"), Ok(5));
        assert_eq!(b.hand_back("l"), Err(PI_FAKE_RETURN));
        assert_eq!(b.boost("h", "l"), Err(PI_NO_BOOST));
        b.seal();
        assert_eq!(b.boost("l", "h"), Err(PI_FROZEN));
    }

    #[test]
    fn unx24154_checks_green() {
        let s = run_unx24154_checks();
        assert!(s.all_passed(), "UNX-F24154 判据未全绿");
    }
}
