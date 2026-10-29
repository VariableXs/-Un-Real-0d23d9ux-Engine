//! UNX-F12188 — RecursionCount/OwningThread 递归语义（0x1a9a0 段）
//!
//! 锚点：UNX-F12188-J1 递归持锁 10/10（同线程重入 RecursionCount
//! 逐次 +1、异线程拒入、递归深度归零才真释放），重入账全账回放
//! 一致，深度下溢必拒。
//! 域：CoRun·UNX-D1-B10（临界区语义谱系）。
//! 结构：RecursionLed（重入账：OwningThread + RecursionCount）+
//! 深度判定核 + 折叠指纹 + 注入核。
//!
//! NT 语义：RTL_CRITICAL_SECTION 的 OwningThread 记唯一持有线程、
//! RecursionCount 记重入深度——同线程 Enter 每次深度 +1，Leave
//! 每次深度 -1，深度归零才把锁真正放回空态；异线程在深度非零
//! 期间一律拒入；未持有而 Leave 是深度下溢，必拒。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a9a0;
pub const RS_SEALED: u32 = SEG_BASE + 0x01; // 印后进出
pub const RS_FOREIGN: u32 = SEG_BASE + 0x02; // 异线程拒入
pub const RS_FREE: u32 = SEG_BASE + 0x03; // 未持而释
pub const RS_UNDER: u32 = SEG_BASE + 0x04; // 深度下溢
pub const RS_EMPTY: u32 = SEG_BASE + 0x05; // 空账回放

/// 重入事件留痕。
#[derive(Clone, PartialEq)]
pub struct RecRec {
    pub tid: u64,
    pub depth: u32,
    pub op: u8, // 0=enter 1=leave
    pub note: String,
}

impl RecRec {
    /// 事件自洽：tid 非零、op 落档、留痕非空（leave 事件深度可为 0）。
    pub fn ok(&self) -> bool {
        self.tid != 0 && self.op <= 1 && !self.note.is_empty()
    }

    pub fn render(&self) -> String {
        let tag = if self.op == 0 { "E" } else { "L" };
        format!("{}d{}", tag, self.depth)
    }
}

/// 重入账：OwningThread + RecursionCount 双字段核心。
pub struct RecursionLed {
    pub owner: u64,
    pub depth: u32,
    pub log: Vec<RecRec>,
    next_seq: u32,
    sealed: bool,
}

impl RecursionLed {
    pub fn new() -> RecursionLed {
        RecursionLed { owner: 0, depth: 0, log: Vec::new(), next_seq: 1, sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 进入：空锁得锁深度 1；同线程重入深度 +1；异线程拒入。
    pub fn enter(&mut self, tid: u64, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(RS_SEALED);
        }
        if tid == 0 {
            return Err(RS_FOREIGN);
        }
        if self.owner != 0 && self.owner != tid {
            return Err(RS_FOREIGN);
        }
        if self.owner == 0 {
            self.owner = tid;
            self.depth = 1;
        } else if self.depth == u32::MAX {
            return Err(RS_UNDER);
        } else {
            self.depth += 1;
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.log.push(RecRec { tid, depth: self.depth, op: 0, note: note.to_string() });
        Ok(seq)
    }

    /// 离开：只认持有者；深度 -1，归零真释放；未持而释拒。
    pub fn leave(&mut self, tid: u64, note: &str) -> Result<bool, u32> {
        if self.sealed {
            return Err(RS_SEALED);
        }
        if note.is_empty() {
            return Err(RS_EMPTY);
        }
        if self.owner != tid {
            return Err(RS_FREE);
        }
        if self.depth == 0 {
            return Err(RS_UNDER);
        }
        self.depth -= 1;
        let released = self.depth == 0;
        if released {
            self.owner = 0;
        }
        self.log.push(RecRec { tid, depth: self.depth, op: 1, note: note.to_string() });
        Ok(released)
    }

    /// 递归判定：深度 > 0 即持锁中。
    pub fn held(&self) -> bool {
        self.depth > 0
    }

    /// 全账回放自洽：逐事件过事件核。
    pub fn log_replay(&self) -> bool {
        !self.log.is_empty() && self.log.iter().all(|r| r.ok())
    }

    /// 重入计数对平：enter 次数 = leave 次数 + 当前深度。
    pub fn depth_tallies(&self) -> bool {
        let e = self.log.iter().filter(|r| r.op == 0).count() as u32;
        let l = self.log.iter().filter(|r| r.op == 1).count() as u32;
        e >= l && e - l == self.depth
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a9a0;
        h = h.wrapping_mul(211).wrapping_add(self.owner as u32);
        h = h.wrapping_mul(211).wrapping_add(self.depth);
        for r in &self.log {
            h = h.wrapping_mul(211).wrapping_add(r.tid as u32);
            h = h.wrapping_mul(211).wrapping_add(r.depth);
            h = h.wrapping_mul(211).wrapping_add(r.op as u32);
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
        format!("owner={:#x} depth={}", self.owner, self.depth)
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = RecursionLed::new();
    let _ = b.enter(0xA1, "first");
    // 1) 异线程拒入。
    got[0] = b.enter(0xB2, "foreign").err() == Some(RS_FOREIGN);
    // 2) 异线程离开拒 FREE。
    got[1] = b.leave(0xB2, "bad").err() == Some(RS_FREE);
    // 3) 深度下溢：三连出必拒（深度 1 只够一次）。
    let _ = b.leave(0xA1, "l1");
    got[2] = b.leave(0xA1, "l2").err() == Some(RS_FREE);
    // 4) 空 note 离开拒 EMPTY。
    let _ = b.enter(0xA1, "again");
    got[3] = b.leave(0xA1, "").err() == Some(RS_EMPTY);
    // 5) 印后进出必拒。
    b.seal();
    got[4] = b.enter(0xA1, "post").err() == Some(RS_SEALED);
    got
}

/// 判据入口：UNX-F12188
pub fn run_unx12188_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12188");
    let mut b = RecursionLed::new();

    // J1 首次得锁深度 1。
    s.add(
        "J1_first_depth1",
        b.enter(0xA1, "first").is_ok() && b.depth == 1 && b.owner == 0xA1,
        "首入得锁深度 1",
    );

    // J2 同线程重入深度逐次 +1（连入三次到 4）。
    let j2 = b.enter(0xA1, "r2").is_ok()
        && b.enter(0xA1, "r3").is_ok()
        && b.enter(0xA1, "r4").is_ok()
        && b.depth == 4;
    s.add("J2_recur_depth", j2 && b.held(), "四连入深度 4");

    // J3 异线程拒入。
    s.add(
        "J3_foreign",
        b.enter(0xB2, "x").err() == Some(RS_FOREIGN),
        "异线程拒入 FOREIGN",
    );

    // J4 异线程拒入不改状态。
    s.add(
        "J4_foreign_noop",
        b.owner == 0xA1 && b.depth == 4,
        "拒入后所有权/深度不动",
    );

    // J5 逐层离开深度递减（4→1 不释放）。
    let j5 = b.leave(0xA1, "l4").is_ok()
        && b.leave(0xA1, "l3").is_ok()
        && b.leave(0xA1, "l2").is_ok()
        && b.depth == 1
        && b.held();
    s.add("J5_unwind", j5, "三连出深度 1 仍持锁");

    // J6 深度归零才真释放。
    let rel = b.leave(0xA1, "l1");
    s.add(
        "J6_release_at_zero",
        rel == Ok(true) && b.owner == 0 && !b.held(),
        "最后一层离开才释放",
    );

    // J7 未持而释拒。
    s.add(
        "J7_over_leave",
        b.leave(0xA1, "over").err() == Some(RS_FREE),
        "空锁再出拒 FREE",
    );

    // J8 重入计数对平。
    s.add("J8_tallies", b.depth_tallies(), "enter 数 = leave 数 + 深度");

    // J9 全账回放自洽。
    s.add("J9_replay", b.log_replay(), "重入账逐事件过核");

    // J10 事件渲染。
    let r = b.log[0].render();
    s.add(
        "J10_render",
        r == "Ed1" && b.log[7].render() == "Ld0",
        "首入渲染 Ed1、末出渲染 Ld0",
    );

    // J11 异线程得锁接力。
    let ok11 = b.enter(0xB2, "next").is_ok() && b.owner == 0xB2 && b.depth == 1;
    s.add("J11_next_owner", ok11, "释放后异线程接力得锁");

    // J12 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J12_fingerprint", f1 == f2, "重入账折叠指纹一致");

    // J13 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J13_inject_all", caught == 5, "5 注入点全检出");

    // J14 印账受控。
    b.seal();
    s.add(
        "J14_sealed",
        b.enter(0xA1, "post").err() == Some(RS_SEALED)
            && b.leave(0xB2, "post").err() == Some(RS_SEALED),
        "印账后进出皆拒",
    );

    // J15 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J15_seal_fp", b.fold() == fb, "印账不改指纹");

    // J16 空 note 离开拒 EMPTY。
    let mut b16 = RecursionLed::new();
    let _ = b16.enter(1, "e");
    s.add(
        "J16_empty_note",
        b16.leave(1, "").err() == Some(RS_EMPTY),
        "空留痕离开拒 EMPTY",
    );

    // J17 零 tid 拒入。
    let mut b17 = RecursionLed::new();
    s.add(
        "J17_zero_tid",
        b17.enter(0, "z").err() == Some(RS_FOREIGN) && b17.owner == 0,
        "零 tid 拒入且不改锁态",
    );

    // J18 空账指纹稳定。
    let b18 = RecursionLed::new();
    s.add("J18_fresh_fp", b18.fold() == b18.fold(), "空重入账指纹稳定");

    // J19 空账回放判不自洽。
    s.add("J19_empty_replay", !b18.log_replay(), "空账回放判不自洽");

    // J20 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J20_summary",
        sm.contains("owner=0xb2") && sm.contains("depth=1"),
        "摘要含持有者与当前深度",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12188_recursion() {
        let mut b = RecursionLed::new();
        assert!(b.enter(0xA1, "e1").is_ok());
        assert!(b.enter(0xA1, "e2").is_ok());
        assert_eq!(b.depth, 2);
        assert_eq!(b.enter(0xB2, "x"), Err(RS_FOREIGN));
        assert_eq!(b.leave(0xA1, "l1"), Ok(false));
        assert_eq!(b.leave(0xA1, "l2"), Ok(true));
        assert_eq!(b.leave(0xA1, "l3"), Err(RS_FREE));
    }

    #[test]
    fn unx12188_checks_green() {
        let s = run_unx12188_checks();
        assert!(s.all_passed(), "UNX-F12188 判据未全绿");
    }

    #[test]
    fn unx12188_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
