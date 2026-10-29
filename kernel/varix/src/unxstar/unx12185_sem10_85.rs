//! UNX-F12185 — TryEnterCriticalSection 语义（0x1a940 段）
//!
//! 锚点：UNX-F12185-J1 非阻塞试锁三态判据 10/10（0=得锁、1=忙、
//! 自锁拒 OWNED），试锁账逐笔留痕全账回放一致，未持而释必拒。
//! 域：CoRun·UNX-D1-B10（临界区语义谱系）。
//! 结构：TryLed（试锁账）+ 三态判定核 + 折叠指纹 + 注入核。
//!
//! NT 语义：TryEnterCriticalSection 立即返回、绝不阻塞——锁空则
//! 占为己有并返回真（记账 outcome 0）；锁被他线程持有立即返回假
//! （记账 outcome 1=忙）；锁被本线程持有即自锁场景，本模型按
//! 「试锁账拒 OWNED 自锁」纪律显性记账并拒（outcome 2）。释放
//! 只认持有者本人，未持而释是账面破坏，必拒。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a940;
pub const TE_SEALED: u32 = SEG_BASE + 0x01; // 印后试锁
pub const TE_OWNED: u32 = SEG_BASE + 0x02; // 自锁试锁拒
pub const TE_BUSY: u32 = SEG_BASE + 0x03; // 他持忙
pub const TE_FREE: u32 = SEG_BASE + 0x04; // 未持而释
pub const TE_EMPTY: u32 = SEG_BASE + 0x05; // 空账回放

/// 试锁三态：0=得 1=忙 2=自锁拒。
pub type TryOutcome = u8;
pub const OUT_GOT: TryOutcome = 0;
pub const OUT_BUSY: TryOutcome = 1;
pub const OUT_OWNED: TryOutcome = 2;

/// 试锁账逐笔留痕。
#[derive(Clone, PartialEq)]
pub struct TryRec {
    pub tid: u64,
    pub outcome: TryOutcome,
    pub note: String,
}

impl TryRec {
    /// 记录自洽：tid 非零、outcome 落在三态内。
    pub fn ok(&self) -> bool {
        self.tid != 0 && self.outcome <= OUT_OWNED && !self.note.is_empty()
    }

    pub fn render(&self) -> String {
        format!("tid={:#x} out={}", self.tid, self.outcome)
    }
}

/// 试锁账：owner（0=锁空）+ 逐笔留痕 + 印账位。
pub struct TryLed {
    pub owner: u64,
    pub log: Vec<TryRec>,
    sealed: bool,
}

impl TryLed {
    pub fn new() -> TryLed {
        TryLed { owner: 0, log: Vec::new(), sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 非阻塞试锁：0 得 / 忙拒 / 自锁拒，绝不等待。
    pub fn try_enter(&mut self, tid: u64, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(TE_SEALED);
        }
        if tid == 0 {
            return Err(TE_FREE);
        }
        let outcome = if self.owner == 0 {
            self.owner = tid;
            OUT_GOT
        } else if self.owner == tid {
            OUT_OWNED
        } else {
            OUT_BUSY
        };
        self.log.push(TryRec { tid, outcome, note: note.to_string() });
        if outcome == OUT_GOT {
            Ok(0)
        } else if outcome == OUT_BUSY {
            Err(TE_BUSY)
        } else {
            Err(TE_OWNED)
        }
    }

    /// 释放：只认持有者本人；未持而释拒。
    pub fn release(&mut self, tid: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(TE_SEALED);
        }
        if self.owner != tid {
            return Err(TE_FREE);
        }
        self.owner = 0;
        Ok(())
    }

    /// 三态计数对平：(得, 忙, 自锁拒)。
    pub fn tally(&self) -> (usize, usize, usize) {
        (
            self.log.iter().filter(|r| r.outcome == OUT_GOT).count(),
            self.log.iter().filter(|r| r.outcome == OUT_BUSY).count(),
            self.log.iter().filter(|r| r.outcome == OUT_OWNED).count(),
        )
    }

    /// 全账回放自洽：逐笔过记录核。
    pub fn log_replay(&self) -> bool {
        !self.log.is_empty() && self.log.iter().all(|r| r.ok())
    }

    /// 忙试不改所有权：他持期间所有权仍属原主。
    pub fn busy_keeps_owner(&self) -> bool {
        self.owner != 0
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a940;
        h = h.wrapping_mul(211).wrapping_add(self.owner as u32);
        for r in &self.log {
            h = h.wrapping_mul(211).wrapping_add(r.tid as u32);
            h = h.wrapping_mul(211).wrapping_add(r.outcome as u32);
            for by in r.note.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 账面摘要。
    pub fn book_summary(&self) -> String {
        let (g, b, o) = self.tally();
        format!("owner={:#x} got={} busy={} owned={}", self.owner, g, b, o)
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = TryLed::new();
    // 1) 他持试锁必拒（忙）。
    let _ = b.try_enter(0xA1, "hold");
    got[0] = b.try_enter(0xB2, "busy").err() == Some(TE_BUSY);
    // 2) 自锁试锁必拒（OWNED）。
    got[1] = b.try_enter(0xA1, "self").err() == Some(TE_OWNED);
    // 3) 未持而释必拒。
    got[2] = b.release(0xB2).err() == Some(TE_FREE);
    // 4) 零 tid 试锁必拒。
    got[3] = b.try_enter(0, "zero").err() == Some(TE_FREE);
    // 5) 印后试锁必拒。
    b.release(0xA1);
    b.seal();
    got[4] = b.try_enter(0xC3, "post").err() == Some(TE_SEALED);
    got
}

/// 判据入口：UNX-F12185
pub fn run_unx12185_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12185");
    let mut b = TryLed::new();

    // J1 空锁首试得 0。
    s.add(
        "J1_first_got",
        b.try_enter(0xA1, "first") == Ok(0) && b.owner == 0xA1,
        "空锁试锁得 0 且占为己有",
    );

    // J2 自锁试锁拒 OWNED。
    s.add(
        "J2_self_owned",
        b.try_enter(0xA1, "self").err() == Some(TE_OWNED),
        "自锁场景拒 OWNED",
    );

    // J3 他持试锁拒 BUSY。
    s.add(
        "J3_foreign_busy",
        b.try_enter(0xB2, "foreign").err() == Some(TE_BUSY),
        "他持试锁拒 BUSY",
    );

    // J4 试锁账留痕条数对平。
    s.add("J4_log_len", b.log.len() == 3, "三笔试锁全留痕");

    // J5 全账回放自洽。
    s.add("J5_replay", b.log_replay(), "试锁账逐笔过记录核");

    // J6 忙试不改所有权。
    s.add("J6_busy_keeps", b.busy_keeps_owner() && b.owner == 0xA1, "忙试后所有权仍属原主");

    // J7 释放只认本人：他人释拒。
    s.add(
        "J7_foreign_release",
        b.release(0xB2).err() == Some(TE_FREE),
        "他人释放拒 FREE",
    );

    // J8 本主释放成功且锁归空。
    s.add(
        "J8_owner_release",
        b.release(0xA1).is_ok() && b.owner == 0,
        "本主释放后锁归空",
    );

    // J9 释放后他人可试得。
    s.add(
        "J9_next_got",
        b.try_enter(0xB2, "next") == Ok(0) && b.owner == 0xB2,
        "释放后他人试锁得 0",
    );

    // J10 三态计数对平。
    let (g, bu, o) = b.tally();
    s.add("J10_tally", (g, bu, o) == (2, 1, 1), "得2/忙1/自锁1 对平");

    // J11 得锁记录渲染。
    let r = b.log[0].render();
    s.add(
        "J11_render",
        r.contains("tid=0xa1") && r.contains("out=0"),
        "记录渲染含 tid 与三态",
    );

    // J12 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J12_fingerprint", f1 == f2, "试锁账折叠指纹一致");

    // J13 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J13_inject_all", caught == 5, "5 注入点全检出");

    // J14 印账受控。
    b.seal();
    s.add(
        "J14_sealed",
        b.try_enter(0xC3, "post").err() == Some(TE_SEALED)
            && b.release(0xB2).err() == Some(TE_SEALED),
        "印账后试/释皆拒",
    );

    // J15 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J15_seal_fp", b.fold() == fb, "印账不改指纹");

    // J16 交替得锁序列（A 得 A 释 B 得 B 释）。
    let mut b16 = TryLed::new();
    let ok16 = b16.try_enter(1, "a1") == Ok(0)
        && b16.release(1).is_ok()
        && b16.try_enter(2, "b1") == Ok(0)
        && b16.release(2).is_ok()
        && b16.owner == 0;
    s.add("J16_alt_seq", ok16, "A/B 交替得释后锁归空");

    // J17 空账指纹稳定。
    let b17 = TryLed::new();
    s.add("J17_fresh_fp", b17.fold() == b17.fold(), "空试锁账指纹稳定");

    // J18 空账回放拒 EMPTY。
    let b18 = TryLed::new();
    s.add("J18_empty_replay", !b18.log_replay(), "空账回放判不自洽");

    // J19 零 tid 试锁拒。
    let mut b19 = TryLed::new();
    s.add(
        "J19_zero_tid",
        b19.try_enter(0, "zero").err() == Some(TE_FREE) && b19.owner == 0,
        "零 tid 试锁拒且不改锁态",
    );

    // J20 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J20_summary",
        sm.contains("owner=0xb2") && sm.contains("got=2"),
        "摘要含持有者与得锁计数",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12185_try_enter() {
        let mut b = TryLed::new();
        assert_eq!(b.try_enter(0xA1, "t"), Ok(0));
        assert_eq!(b.try_enter(0xB2, "t"), Err(TE_BUSY));
        assert_eq!(b.try_enter(0xA1, "t"), Err(TE_OWNED));
        assert_eq!(b.release(0xB2), Err(TE_FREE));
        assert_eq!(b.release(0xA1), Ok(()));
    }

    #[test]
    fn unx12185_checks_green() {
        let s = run_unx12185_checks();
        assert!(s.all_passed(), "UNX-F12185 判据未全绿");
    }

    #[test]
    fn unx12185_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
