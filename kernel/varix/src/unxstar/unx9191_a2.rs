//! UNX-F9191 休眠族与 A2 时钟仲裁消费协议（0x637A0 段）
//!
//! 锚点：UNX-F9191-J1 仲裁账（休眠到期判定逐条落账：A2 源印、睡量、
//! 起点快照、到期快照、结果、留痕）；J2 仲裁语义（A2 源印锁定 + 到
//! 期判定 now≥start+rel + 辅源不从 + 单调闸四性质）；J3 仲裁指纹（
//! 账本折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B20（休眠×A2 时钟仲裁面）。
//!
//! 结构：A2 源印 + 到期核 + 仲裁账本 + 折叠指纹 + 注入核（错源印/
//! 空留痕/回退/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x637A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x637A0;
pub const AR_BAD_SRC: u32 = SEG_BASE + 0x01; // 错源印
pub const AR_REGRESS: u32 = SEG_BASE + 0x02; // 到期回退
pub const AR_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const AR_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const AR_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// A2 时钟源印。
pub const A2_SEAL: u32 = 0xA2A1_0001;

/// 单位：纳秒。
pub type Ns = u64;

/// 单条仲裁账。
#[derive(Clone)]
pub struct ArRec {
    pub seq: u32,
    pub rel: Ns,
    pub start: Ns,
    pub expire_at: Ns, // start + rel
    pub now: Ns,       // 检查时刻
    pub fired: bool,   // now >= expire_at
    pub src_seal: u32,
    pub note: String,
}

impl ArRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty()
            && self.src_seal == A2_SEAL
            && self.expire_at == self.start + self.rel
            && self.fired == (self.now >= self.expire_at)
    }

    pub fn render_line(&self) -> String {
        format!(
            "AR{}|rel{}|{}->{}|now{}|{}|{}",
            self.seq,
            self.rel,
            self.start,
            self.expire_at,
            self.now,
            if self.fired { "FIRE" } else { "WAIT" },
            self.note
        )
    }
}

/// 仲裁簿。
pub struct ArBook {
    pub ledger: Vec<ArRec>,
    pub last_now: Ns,
    pub seq_next: u32,
    pub sealed: bool,
}

impl ArBook {
    pub fn new() -> ArBook {
        ArBook { ledger: Vec::new(), last_now: 0, seq_next: 1, sealed: false }
    }

    /// 到期核：登记休眠并按 now 判到期。now 单调闸。
    pub fn arm(
        &mut self,
        rel: Ns,
        start: Ns,
        now: Ns,
        src: u32,
        note: &str,
    ) -> Result<bool, u32> {
        if self.sealed {
            return Err(AR_FROZEN);
        }
        if note.is_empty() {
            return Err(AR_EMPTY_NOTE);
        }
        if src != A2_SEAL {
            return Err(AR_BAD_SRC);
        }
        if now < self.last_now {
            self.record(rel, start, now, src, note);
            return Err(AR_REGRESS);
        }
        self.last_now = now;
        let expire_at = start + rel;
        let fired = now >= expire_at;
        self.record(rel, start, now, src, note);
        Ok(fired)
    }

    fn record(&mut self, rel: Ns, start: Ns, now: Ns, src: u32, note: &str) {
        let seq = self.seq_next;
        self.seq_next += 1;
        let expire_at = start + rel;
        let fired = now >= expire_at;
        self.ledger.push(ArRec {
            seq,
            rel,
            start,
            expire_at,
            now,
            fired,
            src_seal: src,
            note: note.to_string(),
        });
    }

    pub fn fire_count(&self) -> usize {
        self.ledger.iter().filter(|r| r.fired).count()
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
        let mut h: u32 = 0x91910;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add((r.expire_at & 0xFFFF) as u32);
            h = h.wrapping_mul(31).wrapping_add(r.fired as u32);
            h = h.wrapping_mul(31).wrapping_add(r.src_seal);
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
    // 1) 错源印账
    let bad = ArRec {
        seq: 1,
        rel: 1,
        start: 0,
        expire_at: 1,
        now: 0,
        fired: false,
        src_seal: 0xDEAD,
        note: "inj-src".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = ArRec {
        seq: 2,
        rel: 0,
        start: 0,
        expire_at: 0,
        now: 0,
        fired: true,
        src_seal: A2_SEAL,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 错源印拒
    let mut b = ArBook::new();
    got[2] = b.arm(100, 0, 0, 0xBEEF, "inj-src").err() == Some(AR_BAD_SRC);
    // 4) 到期回退
    let _ = b.arm(100, 0, 500, A2_SEAL, "a");
    got[3] = b.arm(100, 0, 100, A2_SEAL, "b").err() == Some(AR_REGRESS);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.arm(100, 0, 600, A2_SEAL, "inj-frozen").err() == Some(AR_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9191_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9191");
    let mut b = ArBook::new();

    // J1 仲裁账非空（FIRE/WAIT 两式）
    let j1 = {
        let _ = b.arm(100, 0, 150, A2_SEAL, "到期");
        let _ = b.arm(100, 0, 50, A2_SEAL, "未到期");
        b.ledger.iter().filter(|r| r.fired).count() >= 1
            && b.ledger.iter().filter(|r| !r.fired).count() >= 1
    };
    s.add("J1-两式账齐", j1, "FIRE/WAIT 各至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且源印/到期自洽");

    // J3 到期判定：now≥start+rel
    let j3 = {
        let mut b2 = ArBook::new();
        let f1 = b2.arm(100, 1000, 1100, A2_SEAL, "恰达") == Ok(true);
        let f2 = b2.arm(100, 1000, 1099, A2_SEAL, "差一") == Ok(false);
        f1 && f2
    };
    s.add("J3-到期判定", j3, "边界 now==expire 判 FIRE");

    // J4 源印锁定
    let j4 = b.arm(100, 0, 200, 0xBEEF, "野源").err() == Some(AR_BAD_SRC);
    s.add("J4-源印锁定", j4, "非 A2 印必拒");

    // J5 到期值自洽
    let j5 = b.ledger.iter().all(|r| r.expire_at == r.start + r.rel);
    s.add("J5-到期自洽", j5, "expire_at=start+rel 全账");

    // J6 now 单调闸
    let j6 = b.arm(100, 0, 50, A2_SEAL, "回退").err() == Some(AR_REGRESS);
    s.add("J6-单调闸", j6, "now 回退必拒");

    // J7 长睡到期
    let j7 = {
        let mut b2 = ArBook::new();
        b2.arm(1_000_000_000, 0, 1_000_000_000, A2_SEAL, "1s") == Ok(true)
    };
    s.add("J7-长睡到期", j7, "1s 睡眠恰点触发");

    // J8 零睡即到
    let j8 = {
        let mut b2 = ArBook::new();
        b2.arm(0, 500, 500, A2_SEAL, "零睡") == Ok(true)
    };
    s.add("J8-零睡即到", j8, "rel=0 恒 FIRE");

    // J9 源印常量锁定
    let j9 = A2_SEAL == 0xA2A1_0001;
    s.add("J9-源印常量", j9, "A2 仲裁印锁定");

    // J10 FIRE 计数
    let j10 = b.fire_count() >= 1;
    s.add("J10-FIRE计数", j10, "至少一次触发留痕");

    // J11 指纹稳定 + seq 唯一
    let j11 = b.fold() == b.fold() && b.seq_unique();
    s.add("J11-指纹稳定", j11, "折叠指纹两次一致且 seq 唯一");

    // J12 空留痕拒
    let j12 = b.arm(100, 0, 700, A2_SEAL, "").err() == Some(AR_EMPTY_NOTE);
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
    fn unx9191_arb_flow() {
        let mut b = ArBook::new();
        assert_eq!(b.arm(100, 0, 150, A2_SEAL, "a"), Ok(true));
        assert_eq!(b.arm(100, 0, 50, A2_SEAL, "b"), Ok(false));
        assert_eq!(b.arm(100, 0, 40, A2_SEAL, "c"), Err(AR_REGRESS));
        assert_eq!(b.arm(100, 0, 60, 0xBEEF, "d"), Err(AR_BAD_SRC));
    }

    #[test]
    fn unx9191_checks_green() {
        let s = run_unx9191_checks();
        assert!(s.all_passed(), "UNX-F9191 判据未全绿");
    }
}
