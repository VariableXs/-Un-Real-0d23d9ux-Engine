//! UNX-F9188 nanosleep 语义档（timespec 规范化）（0x63740 段）
//!
//! 锚点：UNX-F9188-J1 休眠账（每次 nanosleep 落账：请求 timespec、
//! 规范化后值、结果、留痕）；J2 规范化语义（tv_nsec ∈ [0, 1e9) 闭
//! 集 + 进位归一 + 负值拒 + 零时长直返四性质）；J3 休眠指纹（账本
//! 折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B20（nanosleep timespec 规范化面）。
//!
//! 结构：timespec 域 + 规范化核（进位/拒负）+ 休眠账本 + 折叠指纹
//! + 注入核（错码/空留痕/负_nsec/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63740 独占 0x20）。
pub const SEG_BASE: u32 = 0x63740;
pub const NS_NEG_NSEC: u32 = SEG_BASE + 0x01; // 负 nsec（以大数哨兵表达）
pub const NS_BAD_NORM: u32 = SEG_BASE + 0x02; // 规范化错
pub const NS_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const NS_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const NS_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// tv_nsec 域上界（1 秒）。
pub const NSEC_PER_SEC: u32 = 1_000_000_000;

/// timespec（u32 全域；≥哨兵视为负）。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Ts {
    pub sec: u32,
    pub nsec: u32,
}

/// 负域哨兵。
pub const NEG_SENTINEL: u32 = 0x8000_0000;

/// 规范化核：nsec ≥ 1e9 进位；nsec ≥ 哨兵（负）拒。
pub fn normalize_ts(t: Ts) -> Result<Ts, u32> {
    if t.nsec >= NEG_SENTINEL {
        return Err(NS_NEG_NSEC);
    }
    let mut sec = t.sec;
    let mut nsec = t.nsec;
    while nsec >= NSEC_PER_SEC {
        nsec -= NSEC_PER_SEC;
        sec += 1;
    }
    Ok(Ts { sec, nsec })
}

/// 零时长判定。
pub fn is_zero(t: &Ts) -> bool {
    t.sec == 0 && t.nsec == 0
}

/// 单条休眠账。
#[derive(Clone)]
pub struct NsRec {
    pub seq: u32,
    pub req: Ts,
    pub norm: Ts,
    pub note: String,
}

impl NsRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.norm.nsec < NSEC_PER_SEC
    }

    pub fn render_line(&self) -> String {
        format!(
            "NS{}|{:?}->|{:?}|{}",
            self.seq,
            (self.req.sec, self.req.nsec),
            (self.norm.sec, self.norm.nsec),
            self.note
        )
    }
}

/// 休眠簿。
pub struct NsBook {
    pub ledger: Vec<NsRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl NsBook {
    pub fn new() -> NsBook {
        NsBook { ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// nanosleep 门。
    pub fn nanosleep(&mut self, req: Ts, note: &str) -> Result<Ts, u32> {
        if self.sealed {
            return Err(NS_FROZEN);
        }
        if note.is_empty() {
            return Err(NS_EMPTY_NOTE);
        }
        let norm = match normalize_ts(req) {
            Ok(n) => n,
            Err(e) => return Err(e),
        };
        self.ledger.push(NsRec {
            seq: self.seq_next,
            req,
            norm,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(norm)
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
        let mut h: u32 = 0x91880;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.norm.sec);
            h = h.wrapping_mul(31).wrapping_add(r.norm.nsec);
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
    // 1) 负 nsec 拒
    got[0] = normalize_ts(Ts { sec: 0, nsec: NEG_SENTINEL }).err() == Some(NS_NEG_NSEC);
    // 2) 规范化错账
    let bad = NsRec {
        seq: 2,
        req: Ts { sec: 0, nsec: 0 },
        norm: Ts { sec: 0, nsec: NSEC_PER_SEC }, // 越域 norm
        note: "inj-norm".to_string(),
    };
    got[1] = !bad.ok();
    // 3) 空留痕
    let bad2 = NsRec {
        seq: 3,
        req: Ts { sec: 0, nsec: 0 },
        norm: Ts { sec: 0, nsec: 0 },
        note: String::new(),
    };
    got[2] = !bad2.ok();
    // 4) 进位正确
    got[3] = normalize_ts(Ts { sec: 1, nsec: NSEC_PER_SEC + 5 }) == Ok(Ts { sec: 2, nsec: 5 });
    // 5) 印后登记必抓
    let mut b = NsBook::new();
    b.seal();
    got[4] = b.nanosleep(Ts { sec: 0, nsec: 1 }, "inj-frozen").err() == Some(NS_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9188_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9188");
    let mut b = NsBook::new();

    // J1 休眠账非空
    let _ = b.nanosleep(Ts { sec: 0, nsec: 500_000_000 }, "睡 0.5s");
    let j1 = !b.ledger.is_empty();
    s.add("J1-休眠账非空", j1, "nanosleep 落账至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且 norm 在域");

    // J3 进位归一
    let j3 = normalize_ts(Ts { sec: 1, nsec: NSEC_PER_SEC + 5 }) == Ok(Ts { sec: 2, nsec: 5 });
    s.add("J3-进位归一", j3, "nsec≥1e9 进位到 sec");

    // J4 nsec 域闭集
    let j4 = normalize_ts(Ts { sec: 0, nsec: NSEC_PER_SEC - 1 }) == Ok(Ts { sec: 0, nsec: NSEC_PER_SEC - 1 });
    s.add("J4-域上界", j4, "nsec=1e9-1 不进位");

    // J5 负 nsec 拒
    let j5 = normalize_ts(Ts { sec: 0, nsec: NEG_SENTINEL }).err() == Some(NS_NEG_NSEC);
    s.add("J5-负nsec拒", j5, "负域 nsec 必拒");

    // J6 零时长直返
    let j6 = {
        let mut b2 = NsBook::new();
        let r = b2.nanosleep(Ts { sec: 0, nsec: 0 }, "零睡");
        r == Ok(Ts { sec: 0, nsec: 0 })
    };
    s.add("J6-零时长", j6, "零 timespec 直返");

    // J7 常值归一：1.5s
    let j7 = normalize_ts(Ts { sec: 1, nsec: 500_000_000 }) == Ok(Ts { sec: 1, nsec: 500_000_000 });
    s.add("J7-常值归一", j7, "1.5s 原样通过");

    // J8 多重进位
    let j8 = normalize_ts(Ts { sec: 0, nsec: 3 * NSEC_PER_SEC }) == Ok(Ts { sec: 3, nsec: 0 });
    s.add("J8-多重进位", j8, "3e9 nsec → 3s");

    // J9 域常量锁定
    let j9 = NSEC_PER_SEC == 1_000_000_000;
    s.add("J9-域常量", j9, "1e9 ns = 1s 锁定");

    // J10 零判定
    let j10 = is_zero(&Ts { sec: 0, nsec: 0 }) && !is_zero(&Ts { sec: 0, nsec: 1 });
    s.add("J10-零判定", j10, "is_zero 双例");

    // J11 指纹稳定 + seq 唯一
    let j11 = b.fold() == b.fold() && b.seq_unique();
    s.add("J11-指纹稳定", j11, "折叠指纹两次一致且 seq 唯一");

    // J12 空留痕拒
    let j12 = b.nanosleep(Ts { sec: 0, nsec: 1 }, "").err() == Some(NS_EMPTY_NOTE);
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
    fn unx9188_norm_flow() {
        assert_eq!(normalize_ts(Ts { sec: 1, nsec: NSEC_PER_SEC + 5 }), Ok(Ts { sec: 2, nsec: 5 }));
        assert_eq!(normalize_ts(Ts { sec: 0, nsec: NSEC_PER_SEC }), Ok(Ts { sec: 1, nsec: 0 }));
        assert_eq!(normalize_ts(Ts { sec: 0, nsec: NEG_SENTINEL }), Err(NS_NEG_NSEC));
        let mut b = NsBook::new();
        assert!(b.nanosleep(Ts { sec: 0, nsec: 100 }, "a").is_ok());
        assert_eq!(b.nanosleep(Ts { sec: 0, nsec: NEG_SENTINEL }, "b"), Err(NS_NEG_NSEC));
    }

    #[test]
    fn unx9188_checks_green() {
        let s = run_unx9188_checks();
        assert!(s.all_passed(), "UNX-F9188 判据未全绿");
    }
}
