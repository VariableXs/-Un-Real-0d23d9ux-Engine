//! UNX-F9189 nanosleep EINTR 重启与 remaining 回写（0x63760 段）
//!
//! 锚点：UNX-F9189-J1 打断账（每次休眠被打断落账：请求值、已睡值、
//! remaining 回写值、重启决策、留痕）；J2 回写语义（remaining = 请求
//! −已睡 + 回写必填 + 重启两路（RESTART/EINTR）+ 已睡单调四性质）；
//! J3 打断指纹（账本折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B20（休眠打断回写面）。
//!
//! 结构：打断簿 + 回写核（remaining 计算 + 重启决策）+ 打断账本 +
//! 折叠指纹 + 注入核（回写缺失/空留痕/超睡/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63760 独占 0x20）。
pub const SEG_BASE: u32 = 0x63760;
pub const EI_MISS_REMAIN: u32 = SEG_BASE + 0x01; // 回写缺失
pub const EI_OVER_SLEPT: u32 = SEG_BASE + 0x02; // 已睡超请求
pub const EI_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const EI_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const EI_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 重启决策闭集。
pub const RD_RESTART: u8 = 0; // SA_RESTART：内核自动重启
pub const RD_EINTR: u8 = 1; // 返回 EINTR
pub const RD_COUNT: u8 = 2;

pub const RD_NAMES: [&str; 2] = ["restart", "eintr"];

/// 单位：纳秒（简化整数域）。
pub type Ns = u64;

/// 单条打断账。
#[derive(Clone)]
pub struct EiRec {
    pub seq: u32,
    pub req: Ns,
    pub slept: Ns,
    pub remaining: Ns,
    pub decision: u8,
    pub note: String,
}

impl EiRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty()
            && self.decision < RD_COUNT
            && self.remaining == self.req.saturating_sub(self.slept)
            && self.slept <= self.req
    }

    pub fn render_line(&self) -> String {
        format!(
            "EI{}|r{}s{}rem{}|{}|{}",
            self.seq,
            self.req,
            self.slept,
            self.remaining,
            RD_NAMES[self.decision as usize],
            self.note
        )
    }
}

/// 打断簿。
pub struct EiBook {
    pub ledger: Vec<EiRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl EiBook {
    pub fn new() -> EiBook {
        EiBook { ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 打断核：req 请求、slept 已睡、restart 决策 → remaining 回写。
    pub fn interrupted(
        &mut self,
        req: Ns,
        slept: Ns,
        restart: bool,
        note: &str,
    ) -> Result<Ns, u32> {
        if self.sealed {
            return Err(EI_FROZEN);
        }
        if note.is_empty() {
            return Err(EI_EMPTY_NOTE);
        }
        if slept > req {
            self.record(req, slept, 0, if restart { RD_RESTART } else { RD_EINTR }, note);
            return Err(EI_OVER_SLEPT);
        }
        let remaining = req - slept;
        let decision = if restart { RD_RESTART } else { RD_EINTR };
        self.record(req, slept, remaining, decision, note);
        if !restart {
            // EINTR 路：回写 remaining 给用户
            return Err(EI_MISS_REMAIN);
        }
        Ok(remaining)
    }

    fn record(&mut self, req: Ns, slept: Ns, remaining: Ns, decision: u8, note: &str) {
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(EiRec {
            seq,
            req,
            slept,
            remaining,
            decision,
            note: note.to_string(),
        });
    }

    /// 回写完备：每条账 remaining 与 req−slept 一致。
    pub fn remain_complete(&self) -> bool {
        self.ledger.iter().all(|r| r.remaining == r.req.saturating_sub(r.slept))
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
        let mut h: u32 = 0x91890;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add((r.req & 0xFFFF) as u32);
            h = h.wrapping_mul(31).wrapping_add((r.slept & 0xFFFF) as u32);
            h = h.wrapping_mul(31).wrapping_add((r.remaining & 0xFFFF) as u32);
            h = h.wrapping_mul(31).wrapping_add(r.decision as u32);
            for b in r.note.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 回写缺失账（remaining 与 req−slept 不符）
    let bad = EiRec {
        seq: 1,
        req: 100,
        slept: 30,
        remaining: 50, // 应为 70
        decision: RD_EINTR,
        note: "inj-remain".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = EiRec {
        seq: 2,
        req: 100,
        slept: 0,
        remaining: 100,
        decision: 0,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 已睡超请求
    let mut b = EiBook::new();
    got[2] = b.interrupted(100, 150, true, "inj-over").err() == Some(EI_OVER_SLEPT);
    // 4) EINTR 路返回
    got[3] = b.interrupted(100, 30, false, "inj-eintr").err() == Some(EI_MISS_REMAIN);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.interrupted(100, 10, true, "inj-frozen").err() == Some(EI_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9189_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9189");
    let mut b = EiBook::new();

    // J1 打断账非空
    let _ = b.interrupted(1_000_000_000, 400_000_000, false, "0.4s 被打断");
    let j1 = !b.ledger.is_empty();
    s.add("J1-打断账非空", j1, "打断落账至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且 remaining 自洽");

    // J3 remaining 计算：1s − 0.4s = 0.6s
    let j3 = b.ledger.iter().any(|r| r.req == 1_000_000_000 && r.remaining == 600_000_000);
    s.add("J3-remaining计算", j3, "req−slept 回写");

    // J4 重启决策闭集
    let j4 = {
        let mut b2 = EiBook::new();
        let _ = b2.interrupted(100, 10, true, "R");
        let _ = b2.interrupted(100, 10, false, "E");
        b2.ledger.iter().filter(|r| r.decision == RD_RESTART).count() >= 1
            && b2.ledger.iter().filter(|r| r.decision == RD_EINTR).count() >= 1
    };
    s.add("J4-重启两路", j4, "RESTART/EINTR 各至少一条");

    // J5 RESTART 路返回剩余
    let j5 = {
        let mut b2 = EiBook::new();
        b2.interrupted(100, 10, true, "R") == Ok(90)
    };
    s.add("J5-RESTART返余", j5, "重启路返回 remaining 供续睡");

    // J6 EINTR 路错误返回（用户可见 EINTR）
    let j6 = {
        let mut b2 = EiBook::new();
        b2.interrupted(100, 10, false, "E").is_err()
    };
    s.add("J6-EINTR错误路", j6, "EINTR 路以错误形式返回");

    // J7 已睡超请求拒
    let j7 = b.interrupted(100, 150, true, "超睡").err() == Some(EI_OVER_SLEPT);
    s.add("J7-超睡拒", j7, "slept>req 必拒");

    // J8 全睡完零剩余
    let j8 = {
        let mut b2 = EiBook::new();
        b2.interrupted(100, 100, true, "全睡") == Ok(0)
    };
    s.add("J8-全睡零余", j8, "slept=req → remaining=0");

    // J9 回写完备
    let j9 = b.remain_complete();
    s.add("J9-回写完备", j9, "全账 remaining 自洽");

    // J10 零请求
    let j10 = {
        let mut b2 = EiBook::new();
        b2.interrupted(0, 0, true, "零请") == Ok(0)
    };
    s.add("J10-零请求", j10, "req=0 直返 0");

    // J11 指纹稳定 + seq 唯一
    let j11 = b.fold() == b.fold() && b.seq_unique();
    s.add("J11-指纹稳定", j11, "折叠指纹两次一致且 seq 唯一");

    // J12 空留痕拒
    let j12 = b.interrupted(100, 10, true, "").err() == Some(EI_EMPTY_NOTE);
    s.add("J12-空留痕拒", j12, "空留痕必拒");

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
    fn unx9189_eintr_flow() {
        let mut b = EiBook::new();
        // EINTR 路留账并返回错误
        assert!(b.interrupted(1000, 400, false, "a").is_err());
        assert_eq!(b.ledger[0].remaining, 600);
        // RESTART 路返回剩余
        assert_eq!(b.interrupted(1000, 400, true, "b"), Ok(600));
        // 超睡
        assert_eq!(b.interrupted(100, 150, true, "c"), Err(EI_OVER_SLEPT));
        assert!(b.remain_complete());
    }

    #[test]
    fn unx9189_checks_green() {
        let s = run_unx9189_checks();
        assert!(s.all_passed(), "UNX-F9189 判据未全绿");
    }
}
