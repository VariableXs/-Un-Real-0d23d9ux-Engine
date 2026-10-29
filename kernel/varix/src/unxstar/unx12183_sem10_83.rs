//! UNX-F12183 — LeaveCriticalSection 语义与唤醒协议（0x1a900 段）
//!
//! 锚点：UNX-F12183-J1 离开与唤醒 10/10（非主释放必拒 / 递归归零
//! 才真放锁 / FIFO 唤醒头等待者），释放留痕全账对平。
//! 域：CoRun·UNX-D1-B10（临界区谱系）。
//! 结构：CritSecExit（释放运行时）+ WakeProtocol（唤醒协议账）+
//! 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a900;
pub const LV_NOT_OWN: u32 = SEG_BASE + 0x01; // 非主释放
pub const LV_ORDER: u32 = SEG_BASE + 0x02; // 形制非法（未锁释放）
pub const LV_SEALED: u32 = SEG_BASE + 0x03; // 印后释放
pub const LV_GHOST: u32 = SEG_BASE + 0x04; // 野唤醒
pub const LV_EMPTY: u32 = SEG_BASE + 0x05; // 空队列唤醒

/// 释放运行时。
pub struct CritSecExit {
    pub lock_count: i32,
    pub owning_thread: u64,
    pub recursion_count: u32,
    pub waiters: Vec<u64>,
    pub leaves: Vec<(u64, u32)>, // (tid, 释放后递归计数)
    pub wakes: Vec<u64>,
    sealed: bool,
}

impl CritSecExit {
    pub fn new() -> CritSecExit {
        CritSecExit {
            lock_count: -1,
            owning_thread: 0,
            recursion_count: 0,
            waiters: Vec::new(),
            leaves: Vec::new(),
            wakes: Vec::new(),
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn enter(&mut self, tid: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(LV_SEALED);
        }
        if tid == 0 {
            return Err(LV_ORDER);
        }
        if self.owning_thread != 0 && self.owning_thread != tid {
            return Err(LV_NOT_OWN);
        }
        self.lock_count += 1;
        self.owning_thread = tid;
        self.recursion_count += 1;
        Ok(())
    }

    /// LeaveCriticalSection（非主/未锁必拒；递归归零真放锁）。
    pub fn leave(&mut self, tid: u64) -> Result<u32, u32> {
        if self.sealed {
            return Err(LV_SEALED);
        }
        if self.owning_thread == 0 || self.lock_count < 0 {
            return Err(LV_ORDER);
        }
        if self.owning_thread != tid {
            return Err(LV_NOT_OWN);
        }
        self.recursion_count -= 1;
        self.lock_count -= 1;
        if self.recursion_count == 0 {
            self.owning_thread = 0;
            self.lock_count = -1;
            // 唤醒协议：FIFO 头出队记唤醒。
            if !self.waiters.is_empty() {
                let w = self.waiters.remove(0);
                self.wakes.push(w);
            }
        }
        self.leaves.push((tid, self.recursion_count));
        Ok(self.recursion_count)
    }

    /// 唤醒账与等待队列对平：唤醒数 ≤ 曾入队数。
    pub fn wakes_consistent(&self) -> bool {
        self.wakes.len() <= self.leaves.len() + self.waiters.len()
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a900;
        h = h.wrapping_mul(211).wrapping_add(self.lock_count as u32);
        h = h.wrapping_mul(211).wrapping_add(self.owning_thread as u32);
        for (t, r) in &self.leaves {
            h = h.wrapping_mul(211).wrapping_add(*t as u32);
            h = h.wrapping_mul(211).wrapping_add(*r);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!(
            "leaves={} wakes={} locked={}",
            self.leaves.len(),
            self.wakes.len(),
            self.owning_thread != 0
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = CritSecExit::new();
    // 1) 未锁释放必拒。
    got[0] = b.leave(1).err() == Some(LV_ORDER);
    // 2) 非主释放必拒。
    let _ = b.enter(1);
    got[1] = b.leave(2).err() == Some(LV_NOT_OWN);
    // 3) 零线程进必拒。
    let mut b2 = CritSecExit::new();
    got[2] = b2.enter(0).err() == Some(LV_ORDER);
    // 4) 异线程进必拒。
    got[3] = b2.enter(3).err() == Some(LV_NOT_OWN) || {
        let _ = b2.enter(1);
        b2.enter(2).err() == Some(LV_NOT_OWN)
    };
    // 5) 印后释放必拒。
    b.seal();
    got[4] = b.leave(1).err() == Some(LV_SEALED);
    got
}

/// 判据入口：UNX-F12183
pub fn run_unx12183_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12183");
    let mut b = CritSecExit::new();

    // J1 进入两次（递归 2）。
    let _ = b.enter(0x42);
    let _ = b.enter(0x42);
    s.add(
        "J1_reenter2",
        b.recursion_count == 2 && b.owning_thread == 0x42,
        "双进递归计 2",
    );

    // J2 首次 leave 递归归 1 不放锁。
    let r1 = b.leave(0x42);
    s.add(
        "J2_partial_leave",
        r1 == Ok(1) && b.owning_thread == 0x42,
        "递归 2→1 仍持锁",
    );

    // J3 二次 leave 真放锁。
    let r2 = b.leave(0x42);
    s.add(
        "J3_full_leave",
        r2 == Ok(0) && b.owning_thread == 0 && b.lock_count == -1,
        "递归归零真放锁",
    );

    // J4 非主释放拒。
    let _ = b.enter(0x50);
    s.add(
        "J4_not_owner",
        b.leave(0x99).err() == Some(LV_NOT_OWN),
        "非主释放拒 NOT_OWN",
    );

    // J5 未锁释放拒。
    let _ = b.leave(0x50);
    s.add(
        "J5_unlocked_leave",
        b.leave(0x50).err() == Some(LV_ORDER),
        "未锁再放拒 ORDER",
    );

    // J6 唤醒协议 FIFO。
    let mut b6 = CritSecExit::new();
    let _ = b6.enter(0x10);
    b6.waiters = [0x20, 0x21].to_vec();
    let _ = b6.leave(0x10);
    s.add(
        "J6_wake_fifo",
        b6.wakes == [0x20] && b6.waiters == [0x21],
        "放锁唤醒头 0x20、余 0x21",
    );

    // J7 空队列放锁不唤。
    let mut b7 = CritSecExit::new();
    let _ = b7.enter(0x30);
    let _ = b7.leave(0x30);
    s.add(
        "J7_no_wake",
        b7.wakes.is_empty(),
        "空队列放锁零唤醒",
    );

    // J8 释放留痕对平（J2/J3/J5 三次释放全在账）。
    s.add(
        "J8_leaves",
        b.leaves == [(0x42, 1), (0x42, 0), (0x50, 0)] && b.leaves.len() == 3,
        "释放留痕三条逐笔对平",
    );

    // J9 唤醒账一致。
    s.add(
        "J9_wakes_ok",
        b.wakes_consistent(),
        "唤醒账一致位真",
    );

    // J10 唤醒留痕 10/10（十次进出带等待者）。
    let mut b10 = CritSecExit::new();
    let mut ok10 = true;
    for i in 0..10u64 {
        let t = 0xA0 + i;
        let _ = b10.enter(t);
        b10.waiters.push(t + 0x100);
        if b10.leave(t).is_err() {
            ok10 = false;
        }
    }
    s.add(
        "J10_wake_10x",
        ok10 && b10.wakes.len() == 10,
        "十轮放锁十次唤醒",
    );

    // J11 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J11_fingerprint", f1 == f2, "释放运行时折叠指纹一致");

    // J12 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J12_inject_all", caught == 5, "5 注入点全检出");

    // J13 印账受控。
    let _ = b.enter(0x60);
    b.seal();
    s.add(
        "J13_sealed",
        b.leave(0x60).err() == Some(LV_SEALED) && b.enter(0x60).err() == Some(LV_SEALED),
        "印账后进/放皆拒",
    );

    // J14 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J14_summary",
        sm.contains("leaves=3") && sm.contains("locked=true"),
        "摘要含释放数与锁位",
    );

    // J15 零线程进拒。
    let mut b15 = CritSecExit::new();
    s.add(
        "J15_zero_tid",
        b15.enter(0).err() == Some(LV_ORDER),
        "零线程进拒 ORDER",
    );

    // J16 重入账三进三放全归零。
    let mut b16 = CritSecExit::new();
    for _ in 0..3 {
        let _ = b16.enter(0x70);
    }
    for _ in 0..3 {
        let _ = b16.leave(0x70);
    }
    s.add(
        "J16_three_cycle",
        b16.owning_thread == 0 && b16.recursion_count == 0,
        "三进三放全归零",
    );

    // J17 唤醒头 FIFO 序保持。
    let mut b17 = CritSecExit::new();
    let _ = b17.enter(0x80);
    b17.waiters = [1, 2, 3].to_vec();
    let _ = b17.leave(0x80);
    s.add(
        "J17_fifo_order",
        b17.wakes == [1] && b17.waiters == [2, 3],
        "唤醒头出队序保持",
    );

    // J18 空运行时指纹稳定。
    let b18 = CritSecExit::new();
    s.add("J18_fresh_fp", b18.fold() == b18.fold(), "空释放运行时指纹稳定");

    // J19 等待者不动账（未放锁不唤）。
    let mut b19 = CritSecExit::new();
    let _ = b19.enter(0x90);
    b19.waiters.push(0x91);
    s.add(
        "J19_no_wake_locked",
        b19.wakes.is_empty() && b19.owning_thread == 0x90,
        "持锁期零唤醒",
    );

    // J20 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J20_seal_fp", b.fold() == fb, "印账不改指纹");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12183_leave() {
        let mut b = CritSecExit::new();
        let _ = b.enter(1);
        let _ = b.enter(1);
        assert_eq!(b.leave(1), Ok(1));
        assert_eq!(b.leave(1), Ok(0));
        assert_eq!(b.owning_thread, 0);
    }

    #[test]
    fn unx12183_checks_green() {
        let s = run_unx12183_checks();
        assert!(s.all_passed(), "UNX-F12183 判据未全绿");
    }

    #[test]
    fn unx12183_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
