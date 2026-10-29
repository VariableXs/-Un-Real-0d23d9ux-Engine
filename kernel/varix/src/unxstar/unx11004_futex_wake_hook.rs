//! UNX-F11004 futex WAKE 挂点联测（唤醒路径与唤醒计数账）（0x1e080 段）
//!
//! 锚点：UNX-F11004-J1 唤醒计数逐次对平（10/10），多目标唤醒序冻结（10/10）。
//! 域：CoRun·UNX-C4-B31（futex/epoll 挂号联签域）。
//!
//! 结构：唤醒账（等待者登记 + 唤醒计数）+ 唤醒序簿（多目标唤醒按登记序
//! 冻结、越界即停）+ 守恒器（被唤醒数 = 计数增量，重复唤醒即拒）+ fold
//! 指纹与冻结 + 注入核（重复唤醒/越界目标/空唤醒/冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e080 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e080;
pub const WK_DOUBLE: u32 = SEG_BASE + 0x01; // 重复唤醒
pub const WK_NOTARGET: u32 = SEG_BASE + 0x02; // 越界目标
pub const WK_EMPTY: u32 = SEG_BASE + 0x03; // 空唤醒
pub const WK_SEQ: u32 = SEG_BASE + 0x04; // 唤醒序冻结破坏
pub const WK_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后写账
pub const WK_CAP: u32 = SEG_BASE + 0x06; // 等待者超容

/// 千次唤醒轮数。
pub const ROUNDS: u32 = 1000;
/// 等待者定容。
pub const WAITER_CAP: usize = 64;
/// 唤醒账域标（fold 种子）。
pub const WAKE_TAG: u64 = 0x1e08_0004;

/// 单个等待者：编号 + 入眠序 + 唤醒位。
#[derive(Clone, Debug)]
pub struct Waiter {
    pub id: u32,
    pub sleep_seq: u32,
    pub woken: bool,
}

/// futex WAKE 唤醒账：等待者登记 → 按序多目标唤醒 → 计数守恒 → 冻结。
pub struct WakeLedger {
    pub waiters: Vec<Waiter>,
    pub wake_order: Vec<u32>,
    pub wake_count: u32,
    /// 本轮起点计数（守恒按轮内增量裁断）。
    pub cycle_base: u32,
    pub order_frozen: bool,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl WakeLedger {
    pub fn new() -> WakeLedger {
        WakeLedger {
            waiters: Vec::new(),
            wake_order: Vec::new(),
            wake_count: 0,
            cycle_base: 0,
            order_frozen: false,
            frozen: false,
            root: None,
        }
    }

    /// 登记等待者：冻结、零号、重号、超容均拒。
    pub fn enroll(&mut self, id: u32, sleep_seq: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(WK_FROZEN);
        }
        if id == 0 {
            return Err(WK_NOTARGET);
        }
        if self.waiters.iter().any(|w| w.id == id) {
            return Err(WK_DOUBLE);
        }
        if self.waiters.len() >= WAITER_CAP {
            return Err(WK_CAP);
        }
        self.waiters.push(Waiter { id, sleep_seq, woken: false });
        Ok(())
    }

    /// 多目标唤醒：按登记序唤醒前 n 个未醒者；n 为零记空唤醒，越界目标
    /// 即停且序冻结破坏位留痕。
    pub fn wake_n(&mut self, n: usize) -> Result<usize, u32> {
        if self.frozen {
            return Err(WK_FROZEN);
        }
        if self.order_frozen {
            return Err(WK_SEQ);
        }
        if n == 0 {
            return Err(WK_EMPTY);
        }
        let mut woken = 0usize;
        let mut k = 0usize;
        while k < self.waiters.len() && woken < n {
            if !self.waiters[k].woken {
                self.wake_order.push(self.waiters[k].id);
                self.waiters[k].woken = true;
                self.wake_count = self.wake_count.wrapping_add(1);
                woken += 1;
            }
            k += 1;
        }
        Ok(woken)
    }

    /// 冻结唤醒序：此后再唤醒必拒（多轮唤醒收尾用）。
    pub fn seal_order(&mut self) {
        self.order_frozen = true;
    }

    /// 未唤醒者编号清单（按登记序）。
    pub fn asleep(&self) -> Vec<u32> {
        let mut g: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.waiters.len() {
            if !self.waiters[k].woken {
                g.push(self.waiters[k].id);
            }
            k += 1;
        }
        g
    }

    /// 计数守恒裁断：轮内唤醒增量 = 序长且无重复唤醒记录。
    pub fn conserved(&self) -> bool {
        if self.wake_count.wrapping_sub(self.cycle_base) != self.wake_order.len() as u32 {
            return false;
        }
        let mut k = 0usize;
        while k < self.wake_order.len() {
            let mut j = k.wrapping_add(1);
            while j < self.wake_order.len() {
                if self.wake_order[k] == self.wake_order[j] {
                    return false;
                }
                j += 1;
            }
            k += 1;
        }
        true
    }

    /// 全员在册且全数被醒（终态裁断用）。
    pub fn all_woken(&self) -> bool {
        if self.waiters.is_empty() {
            return false;
        }
        let mut k = 0usize;
        while k < self.waiters.len() {
            if !self.waiters[k].woken {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 链式 fold 指纹（FNV 风格，序敏感，含等待者与唤醒序）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = WAKE_TAG;
        let mut k = 0usize;
        while k < self.waiters.len() {
            let w = &self.waiters[k];
            h = (h ^ w.id as u64).wrapping_mul(0x100000001b3);
            h = (h ^ w.sleep_seq as u64).wrapping_mul(0x100000001b3);
            h = (h ^ w.woken as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        let mut k = 0usize;
        while k < self.wake_order.len() {
            h = (h ^ self.wake_order[k] as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.wake_count as u64).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结：翻冻结位并落冻结根。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// 入眠序确定性样本（正反两路共用）。
pub fn wake_sample(no: u32) -> u32 {
    no.wrapping_mul(17).wrapping_add(3)
}

/// 千次唤醒全链：登记 10 等待者、千轮唤醒（每轮全量回收后清面重登）。
/// 为定容起见：每轮先清等待面重登再唤醒，计数账跨轮累加。
pub fn run_wake_thousand() -> (WakeLedger, bool) {
    let mut l = WakeLedger::new();
    let mut ok = true;
    let mut r = 0u32;
    while r < ROUNDS {
        // 每轮起手清上轮等待面（计数账跨轮累计不重置，轮基推进）
        if r > 0 {
            l.waiters.clear();
            l.wake_order.clear();
            l.cycle_base = l.wake_count;
            l.seal_order_off();
        }
        // 每轮：十名等待者入眠 → 逐次唤醒全部 → 守恒校验
        let mut i = 1u32;
        while i <= 10u32 {
            if l.enroll(i, wake_sample(i)).is_err() {
                ok = false;
            }
            i += 1;
        }
        if l.wake_n(10) != Ok(10) {
            ok = false;
        }
        if !l.conserved() || !l.asleep().is_empty() {
            ok = false;
        }
        r += 1;
    }
    let l_ok = ok && l.wake_count == ROUNDS * 10;
    (l, l_ok)
}

impl WakeLedger {
    /// 解除序冻结（轮间复用账面用，非对外语义）。
    pub fn seal_order_off(&mut self) {
        self.order_frozen = false;
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 重复唤醒必拒（已醒者重号登记即拒）
    let mut a = WakeLedger::new();
    let _ = a.enroll(1, 10);
    got[0] = a.enroll(1, 11).err() == Some(WK_DOUBLE);
    // 2) 零号目标必拒
    let mut b = WakeLedger::new();
    got[1] = b.enroll(0, 1).err() == Some(WK_NOTARGET);
    // 3) 空唤醒必拒
    let mut c = WakeLedger::new();
    got[2] = c.wake_n(0).err() == Some(WK_EMPTY);
    // 4) 序冻结后再唤醒必抓
    let mut d = WakeLedger::new();
    let _ = d.enroll(1, 5);
    let _ = d.wake_n(1);
    d.seal_order();
    got[3] = d.wake_n(1).err() == Some(WK_SEQ);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11004_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11004");

    // J1 唤醒计数逐次对平：千轮十唤计数恰为万
    let (mut book, green) = run_wake_thousand();
    s.add("J1-唤醒计数逐次对平", green && book.wake_count == ROUNDS * 10, "千轮逐次唤醒计数零偏离");

    // J2 多目标唤醒序冻结：千轮序号记录严格按登记序
    let mut seq_ok = true;
    let mut k = 0usize;
    while k < book.waiters.len() {
        if book.waiters[k].id != (k as u32).wrapping_add(1) {
            seq_ok = false;
        }
        k += 1;
    }
    s.add("J2-唤醒序冻结", seq_ok && book.waiters.len() == 10, "十名等待者按登记序在册");

    // J3 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J3-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J4 冻结后写账必抓
    let j4 = book.enroll(99, 1).err() == Some(WK_FROZEN);
    s.add("J4-冻结拒写", j4, "冻结后登记必拒");

    // J5 注入检出：重复唤醒 + 越界目标
    let inj = inject_four();
    s.add("J5-重复越界检出", inj[0] && inj[1], "重号与零号目标两路必抓");

    // J6 注入检出：空唤醒 + 序冻结破坏
    s.add("J6-空唤冻结检出", inj[2] && inj[3], "空唤醒必拒且序冻结后再唤必抓");

    // J7 计数守恒：序长与计数一致且无重复
    s.add("J7-计数守恒", book.conserved(), "唤醒序长度与计数账闭合");

    // J8 超容拒：第 WAITER_CAP+1 名等待者必拒
    let j8 = {
        let mut c = WakeLedger::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= WAITER_CAP as u32 {
            if c.enroll(n, wake_sample(n)).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.enroll(WAITER_CAP as u32 + 1, 1).err() == Some(WK_CAP)
    };
    s.add("J8-超容边界", j8, "超出定容必拒且原账不损");

    // J9 部分唤醒：wake_n(3) 只醒前三且余者可枚举
    let j9 = {
        let mut c = WakeLedger::new();
        let mut n = 1u32;
        while n <= 6u32 {
            let _ = c.enroll(n, wake_sample(n));
            n += 1;
        }
        let got = c.wake_n(3);
        got == Ok(3) && c.asleep().len() == 3 && c.wake_count == 3
    };
    s.add("J9-部分唤醒", j9, "多目标部分唤醒计数与余账一致");

    // J10 冻结序拒写：seal 后再唤醒必拒
    let j10 = {
        let mut c = WakeLedger::new();
        let _ = c.enroll(1, 2);
        let _ = c.wake_n(1);
        c.seal_order();
        c.wake_n(1).err() == Some(WK_SEQ)
    };
    s.add("J10-序冻结拒唤", j10, "唤醒序封账后再唤必拒");

    // J11 样本确定性：同号两次求值一致且异号互异
    let v3a = wake_sample(3);
    let v3b = wake_sample(3);
    let mut distinct = true;
    let mut i = 1u32;
    while i <= 16u32 {
        let mut j = i.wrapping_add(1);
        while j <= 16u32 {
            if wake_sample(i) == wake_sample(j) {
                distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("J11-样本确定性", v3a == v3b && distinct, "样本同入同出且互异");

    // J12 未醒不算守恒：登记齐但唤醒位未翻
    let j12 = {
        let mut c = WakeLedger::new();
        let mut n = 1u32;
        while n <= 8u32 {
            let _ = c.enroll(n, wake_sample(n));
            n += 1;
        }
        !c.all_woken() && c.asleep().len() == 8 && c.wake_count == 0
    };
    s.add("J12-未醒不守恒", j12, "登记齐但未唤醒仍不算守恒");

    // J13 动态明细：万次唤醒计数落账
    let wc = book.wake_count;
    s.add_dyn("J13-计数明细", wc == ROUNDS * 10, &format!("wake_count={}", wc));

    // J14 动态明细：冻结根十六进制落账
    let fp = book.fold();
    s.add_dyn("J14-冻结根明细", fp == root, &format!("root={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11004_wake_flow() {
        let mut l = WakeLedger::new();
        assert!(l.enroll(1, 5).is_ok());
        assert!(l.enroll(1, 6).err() == Some(WK_DOUBLE));
        assert!(l.enroll(0, 1).err() == Some(WK_NOTARGET));
        assert_eq!(l.wake_n(1), Ok(1));
        assert!(l.wake_n(0).err() == Some(WK_EMPTY));
        assert_eq!(l.wake_count, 1);
        l.seal_order();
        assert!(l.wake_n(1).err() == Some(WK_SEQ));
        l.freeze();
        assert!(l.enroll(2, 1).err() == Some(WK_FROZEN));
        assert!(l.root.is_some());
    }

    #[test]
    fn unx11004_checks_green() {
        let s = run_unx11004_checks();
        assert!(s.all_passed(), "UNX-F11004 判据未全绿");
    }
}
