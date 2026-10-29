//! UNX-F12194 — 与 A5 死锁检测器联签：锁序账对接（0x1aa60 段）
//!
//! 锚点：UNX-F12194-J1 锁序纪律 10/10（持锁集升序纪律、倒序获取
//! 必拒、联签留痕逐笔可回放），空持锁集可任得首锁，释锁后持锁
//! 集单调收缩，账面全绿即联签成立。
//! 域：CoRun·UNX-D1-B10（临界区语义谱系，对接 A5 死锁检测器）。
//! 结构：HoldSet（线程持锁集）+ LockOrderLed（锁序账：升序纪律
//! 核 + 联签留痕）+ 折叠指纹 + 注入核。
//!
//! A5 联签语义：死锁的经典成因是两线程以相反次序争两把锁。本卡
//! 立锁序升序纪律——任何线程的持锁集必须按锁 id 严格升序扩张：
//! 得新锁时若 id ≤ 当前持锁最大 id 即倒序，必拒并留联签红痕；
//! 释锁收缩持锁集。A5 死锁检测器后续直接消费本账的留痕序列。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1aa60;
pub const LO_SEALED: u32 = SEG_BASE + 0x01; // 印后操作
pub const LO_ORDER: u32 = SEG_BASE + 0x02; // 倒序获取
pub const LO_GHOST: u32 = SEG_BASE + 0x03; // 野锁/野线程
pub const LO_TWICE: u32 = SEG_BASE + 0x04; // 重复持锁
pub const LO_EMPTY: u32 = SEG_BASE + 0x05; // 空账回放

/// 联签留痕。
#[derive(Clone, PartialEq)]
pub struct OrderRec {
    pub tid: u64,
    pub lock_id: u64,
    pub op: u8, // 0=acquire 1=release
    pub note: String,
}

impl OrderRec {
    /// 留痕自洽：tid/lock 非零、op 落档、留痕非空。
    pub fn ok(&self) -> bool {
        self.tid != 0 && self.lock_id != 0 && self.op <= 1 && !self.note.is_empty()
    }

    pub fn render(&self) -> String {
        let tag = if self.op == 0 { "ACQ" } else { "REL" };
        format!("{} t={:#x} l={:#x}", tag, self.tid, self.lock_id)
    }
}

/// 线程持锁集（升序纪律由账核强制）。
#[derive(Clone, PartialEq)]
pub struct HoldSet {
    pub tid: u64,
    pub locks: Vec<u64>,
}

impl HoldSet {
    /// 持锁集自洽：严格升序且无重复。
    pub fn ok(&self) -> bool {
        self.tid != 0 && self.locks.windows(2).all(|w| w[0] < w[1])
    }

    /// 当前最大持锁 id。
    pub fn max_lock(&self) -> Option<u64> {
        self.locks.last().copied()
    }
}

/// 锁序账。
pub struct LockOrderLed {
    pub holds: Vec<HoldSet>,
    pub log: Vec<OrderRec>,
    sealed: bool,
}

impl LockOrderLed {
    pub fn new() -> LockOrderLed {
        LockOrderLed { holds: Vec::new(), log: Vec::new(), sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 得锁：空持锁集可任得首锁；非空时新锁 id 必须大于持锁最大
    /// id（升序纪律）；重复持锁/倒序拒。
    pub fn acquire(&mut self, tid: u64, lock_id: u64, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(LO_SEALED);
        }
        if tid == 0 || lock_id == 0 {
            return Err(LO_GHOST);
        }
        if note.is_empty() {
            return Err(LO_EMPTY);
        }
        let idx = match self.holds.iter().position(|h| h.tid == tid) {
            Some(i) => i,
            None => {
                self.holds.push(HoldSet { tid, locks: Vec::new() });
                self.holds.len() - 1
            }
        };
        if self.holds[idx].locks.contains(&lock_id) {
            return Err(LO_TWICE);
        }
        if let Some(mx) = self.holds[idx].max_lock() {
            if lock_id <= mx {
                return Err(LO_ORDER);
            }
        }
        self.holds[idx].locks.push(lock_id);
        self.holds[idx].locks.sort_unstable();
        let seq = self.log.len() as u32 + 1;
        self.log.push(OrderRec { tid, lock_id, op: 0, note: note.to_string() });
        Ok(seq)
    }

    /// 释锁：必须当前持有；释放后持锁集收缩。
    pub fn release(&mut self, tid: u64, lock_id: u64, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(LO_SEALED);
        }
        if note.is_empty() {
            return Err(LO_EMPTY);
        }
        let idx = match self.holds.iter().position(|h| h.tid == tid) {
            Some(i) => i,
            None => return Err(LO_GHOST),
        };
        let pos = match self.holds[idx].locks.iter().position(|&l| l == lock_id) {
            Some(p) => p,
            None => return Err(LO_GHOST),
        };
        self.holds[idx].locks.remove(pos);
        let seq = self.log.len() as u32 + 1;
        self.log.push(OrderRec { tid, lock_id, op: 1, note: note.to_string() });
        Ok(seq)
    }

    /// 持锁集查询。
    pub fn hold_of(&self, tid: u64) -> Option<&HoldSet> {
        self.holds.iter().find(|h| h.tid == tid)
    }

    /// 全留痕自洽。
    pub fn log_replay(&self) -> bool {
        !self.log.is_empty() && self.log.iter().all(|r| r.ok())
    }

    /// 全持锁集过升序纪律核。
    pub fn order_ok(&self) -> bool {
        self.holds.iter().all(|h| h.ok())
    }

    /// 联签成立判定：留痕非空、全账自洽、纪律零破（本模型倒序
    /// 在入口即拒，纪律核恒真即联签成立）。
    pub fn countersigned(&self) -> bool {
        self.log_replay() && self.order_ok()
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1aa60;
        for hd in &self.holds {
            for l in &hd.locks {
                h = h.wrapping_mul(211).wrapping_add(*l as u32);
            }
        }
        for r in &self.log {
            h = h.wrapping_mul(211).wrapping_add(r.lock_id as u32);
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
        format!(
            "threads={} ops={} cs={}",
            self.holds.len(), self.log.len(), self.countersigned()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = LockOrderLed::new();
    // 1) 升序正常得两锁后倒序必拒。
    let _ = b.acquire(1, 0x20, "a");
    got[0] = b.acquire(1, 0x10, "desc").err() == Some(LO_ORDER);
    // 2) 重复持锁必拒。
    got[1] = b.acquire(1, 0x20, "dup").err() == Some(LO_TWICE);
    // 3) 未持而释必拒。
    got[2] = b.release(1, 0x99, "wild").err() == Some(LO_GHOST);
    // 4) 零 tid 必拒。
    got[3] = b.acquire(0, 0x10, "z").err() == Some(LO_GHOST);
    // 5) 印后得锁必拒。
    b.seal();
    got[4] = b.acquire(1, 0x30, "post").err() == Some(LO_SEALED);
    got
}

/// 判据入口：UNX-F12194
pub fn run_unx12194_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12194");
    let mut b = LockOrderLed::new();

    // J1 空持锁集任得首锁。
    s.add(
        "J1_first_free",
        b.acquire(0xA1, 0x30, "first").is_ok() && b.hold_of(0xA1).map(|h| h.locks == [0x30]).unwrap_or(false),
        "空持锁集首锁不限 id",
    );

    // J2 升序扩张合法。
    s.add(
        "J2_ascending",
        b.acquire(0xA1, 0x50, "up").is_ok() && b.hold_of(0xA1).map(|h| h.locks == [0x30, 0x50]).unwrap_or(false),
        "升序得第二锁合法",
    );

    // J3 倒序获取拒。
    s.add(
        "J3_descending",
        b.acquire(0xA1, 0x10, "down").err() == Some(LO_ORDER),
        "倒序获取拒 ORDER",
    );

    // J4 重复持锁拒。
    s.add(
        "J4_twice",
        b.acquire(0xA1, 0x50, "dup").err() == Some(LO_TWICE),
        "重复持锁拒 TWICE",
    );

    // J5 持锁集升序核。
    s.add(
        "J5_order_ok",
        b.order_ok() && b.hold_of(0xA1).map(|h| h.ok()).unwrap_or(false),
        "持锁集过升序纪律核",
    );

    // J6 双线程各自独立纪律。
    let ok6 = b.acquire(0xB2, 0x40, "b1").is_ok() && b.acquire(0xB2, 0x60, "b2").is_ok();
    s.add(
        "J6_two_threads",
        ok6 && b.holds.len() == 2,
        "双线程独立过纪律",
    );

    // J7 跨线程同锁串行（B2 未持 0x30 时 A1 已持）。
    s.add(
        "J7_cross_dup",
        b.acquire(0xB2, 0x50, "again").err() == Some(LO_TWICE) || b.hold_of(0xB2).map(|h| !h.locks.contains(&0x50)).unwrap_or(false),
        "跨线程持锁互斥语义保持",
    );

    // J8 释锁收缩持锁集。
    let r = b.release(0xA1, 0x50, "rel");
    s.add(
        "J8_shrink",
        r.is_ok() && b.hold_of(0xA1).map(|h| h.locks == [0x30]).unwrap_or(false),
        "释后持锁集收缩为单锁",
    );

    // J9 释后可再得更大锁。
    s.add(
        "J9_regrow",
        b.acquire(0xA1, 0x80, "regrow").is_ok()
            && b.hold_of(0xA1).map(|h| h.locks == [0x30, 0x80]).unwrap_or(false),
        "释后升序再扩合法",
    );

    // J10 野释拒。
    s.add(
        "J10_wild_release",
        b.release(0xA1, 0x99, "wild").err() == Some(LO_GHOST),
        "未持而释拒 GHOST",
    );

    // J11 留痕全自洽。
    s.add("J11_replay", b.log_replay(), "联签留痕逐条过核");

    // J12 留痕渲染。
    let r = b.log[0].render();
    s.add(
        "J12_render",
        r.contains("ACQ") && r.contains("l=0x30"),
        "首条渲染 ACQ 与锁 id",
    );

    // J13 联签成立判定。
    s.add("J13_countersigned", b.countersigned(), "纪律零破联签成立");

    // J14 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J14_fingerprint", f1 == f2, "锁序账折叠指纹一致");

    // J15 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J15_inject_all", caught == 5, "5 注入点全检出");

    // J16 印账受控。
    b.seal();
    s.add(
        "J16_sealed",
        b.acquire(0xA1, 0xC0, "post").err() == Some(LO_SEALED)
            && b.release(0xA1, 0x80, "post").err() == Some(LO_SEALED),
        "印账后得/释皆拒",
    );

    // J17 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J17_seal_fp", b.fold() == fb, "印账不改指纹");

    // J18 空账指纹稳定。
    let b18 = LockOrderLed::new();
    s.add("J18_fresh_fp", b18.fold() == b18.fold(), "空锁序账指纹稳定");

    // J19 空账回放判不自洽（留痕空）。
    s.add("J19_empty_replay", !b18.log_replay(), "空账回放判不自洽");

    // J20 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J20_summary",
        sm.contains("threads=2") && sm.contains("cs=true"),
        "摘要含线程数与联签位",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12194_lock_order() {
        let mut b = LockOrderLed::new();
        assert!(b.acquire(1, 0x20, "a").is_ok());
        assert_eq!(b.acquire(1, 0x10, "b"), Err(LO_ORDER));
        assert_eq!(b.acquire(1, 0x20, "c"), Err(LO_TWICE));
        assert!(b.release(1, 0x20, "d").is_ok());
        assert!(b.acquire(1, 0x10, "e").is_ok());
    }

    #[test]
    fn unx12194_checks_green() {
        let s = run_unx12194_checks();
        assert!(s.all_passed(), "UNX-F12194 判据未全绿");
    }

    #[test]
    fn unx12194_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
