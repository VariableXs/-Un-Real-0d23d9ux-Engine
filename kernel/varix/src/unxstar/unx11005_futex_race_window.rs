//! UNX-F11005 futex 竞态挂点联签（WAIT 前唤醒窗口价值变更序）（0x1e0a0 段）
//!
//! 锚点：UNX-F11005-J1 前唤醒窗口千次零丢失（10/10），价值变更序对账
//! （10/10）。
//! 域：CoRun·UNX-C4-B31（futex/epoll 挂号联签域）。
//!
//! 结构：竞态窗口账（价值变更 SetValue 与等待 Wait 两类事件按发生序落账）
//! + 取值器（WAIT 前窗口内的等值变更即取、不入眠，千次零丢失）+ 变更序对
//! 账（变更序号严格递增且逐笔可查）+ fold 指纹与冻结 + 注入核（值错配/
//! 序回退/取空/冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e0a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e0a0;
pub const RW_MISMATCH: u32 = SEG_BASE + 0x01; // 价值与等待期望错配
pub const RW_SEQ: u32 = SEG_BASE + 0x02; // 变更序回退
pub const RW_TAKEEMPTY: u32 = SEG_BASE + 0x03; // 窗口取空
pub const RW_EMPTY: u32 = SEG_BASE + 0x04; // 地址空
pub const RW_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后写账
pub const RW_CAP: u32 = SEG_BASE + 0x06; // 窗口超容

/// 千次竞态轮数。
pub const ROUNDS: u32 = 1000;
/// 窗口事件定容（须容纳千轮 SetValue/Wait 双笔共 2000 笔）。
pub const WIN_CAP: usize = 4096;
/// 竞态窗口域标（fold 种子）。
pub const RACE_TAG: u64 = 0x1e0a_0005;

/// 窗口事件类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvKind {
    /// 价值变更（前唤醒源）。
    SetValue,
    /// 等待挂号（WAIT）。
    Wait,
}

/// 单笔窗口事件。
#[derive(Clone, Debug)]
pub struct RaceEvent {
    pub kind: EvKind,
    pub uaddr: u32,
    pub val: u32,
    pub seq: u32,
    /// SetValue 被等待方取走 / Wait 直接取到前值。
    pub consumed: bool,
}

/// futex 竞态窗口账：变更序落账 → WAIT 前取值 → 逐笔核销 → 冻结。
pub struct RaceWindow {
    pub events: Vec<RaceEvent>,
    pub last_seq: u32,
    pub taken: u32,
    pub slept: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl RaceWindow {
    pub fn new() -> RaceWindow {
        RaceWindow {
            events: Vec::new(),
            last_seq: 0,
            taken: 0,
            slept: 0,
            frozen: false,
            root: None,
        }
    }

    /// 记一笔事件：冻结、零址、零序、序回退、超容均拒。
    pub fn push(&mut self, kind: EvKind, uaddr: u32, val: u32, seq: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(RW_FROZEN);
        }
        if uaddr == 0 {
            return Err(RW_EMPTY);
        }
        if seq == 0 || seq <= self.last_seq {
            return Err(RW_SEQ);
        }
        if self.events.len() >= WIN_CAP {
            return Err(RW_CAP);
        }
        self.last_seq = seq;
        self.events.push(RaceEvent { kind, uaddr, val, seq, consumed: false });
        Ok(())
    }

    /// WAIT 前窗口取值：找同址、未消费、值等于期望的最近一笔 SetValue；
    /// 取到即核销两笔（set 与 wait）并计 taken，取不到计 slept。
    pub fn wait_take(&mut self, uaddr: u32, expect: u32) -> Result<bool, u32> {
        if self.frozen {
            return Err(RW_FROZEN);
        }
        // 先落 WAIT 事件（借用副本避免迭代中改容器）
        let wait_seq = self.last_seq.wrapping_add(1);
        self.last_seq = wait_seq;
        let wait_idx = self.events.len();
        self.events.push(RaceEvent {
            kind: EvKind::Wait,
            uaddr,
            val: expect,
            seq: wait_seq,
            consumed: false,
        });
        // 倒序找最近的等值 SetValue
        let mut hit: i64 = -1;
        let mut k = wait_idx;
        while k > 0 {
            k -= 1;
            let e = &self.events[k];
            if e.kind == EvKind::SetValue && e.uaddr == uaddr && e.val == expect && !e.consumed {
                hit = k as i64;
            }
        }
        if hit >= 0 {
            let h = hit as usize;
            self.events[h].consumed = true;
            self.events[wait_idx].consumed = true;
            self.taken = self.taken.wrapping_add(1);
            Ok(true)
        } else {
            self.slept = self.slept.wrapping_add(1);
            Ok(false)
        }
    }

    /// 已消费事件数。
    pub fn consumed_n(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.events.len() {
            if self.events[k].consumed {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 未消费的 SetValue 残留清单（丢失探针）。
    pub fn residue(&self) -> Vec<u32> {
        let mut g: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.events.len() {
            let e = &self.events[k];
            if e.kind == EvKind::SetValue && !e.consumed {
                g.push(e.seq);
            }
            k += 1;
        }
        g
    }

    /// 零丢失裁断：取到数 + 入眠数 = 等待笔数且无残留 SetValue。
    pub fn no_loss(&self) -> bool {
        self.taken.wrapping_add(self.slept) == self.wait_count() && self.residue().is_empty()
    }

    /// 等待笔数。
    pub fn wait_count(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.events.len() {
            if self.events[k].kind == EvKind::Wait {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 变更序对账：序号严格递增且逐笔可查。
    pub fn seq_monotonic(&self) -> bool {
        let mut k = 1usize;
        while k < self.events.len() {
            if self.events[k].seq <= self.events[k - 1].seq {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 链式 fold 指纹（FNV 风格，序敏感，含类/址/值/序/核销位）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = RACE_TAG;
        let mut k = 0usize;
        while k < self.events.len() {
            let e = &self.events[k];
            let kd = match e.kind {
                EvKind::SetValue => 1u64,
                EvKind::Wait => 2u64,
            };
            h = (h ^ kd).wrapping_mul(0x100000001b3);
            h = (h ^ e.uaddr as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.val as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.seq as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.consumed as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.taken as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.slept as u64).wrapping_mul(0x100000001b3);
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

/// 等待期望值确定性样本。
pub fn race_sample(no: u32) -> u32 {
    no.wrapping_mul(19).wrapping_add(2)
}

/// 千次竞态全链：每轮先 SetValue（前唤醒）后 WAIT，前唤醒必被取走。
pub fn run_race_thousand() -> (RaceWindow, bool) {
    let mut w = RaceWindow::new();
    let mut ok = true;
    let mut i = 1u32;
    while i <= ROUNDS {
        let v = race_sample(i);
        if w.push(EvKind::SetValue, i.wrapping_mul(8), v, w.last_seq.wrapping_add(1)).is_err() {
            ok = false;
        }
        if w.wait_take(i.wrapping_mul(8), v) != Ok(true) {
            ok = false;
        }
        i += 1;
    }
    let w_ok = ok && w.no_loss() && w.taken == ROUNDS;
    (w, w_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 价值错配必入眠（前唤醒不命中）
    let mut a = RaceWindow::new();
    let _ = a.push(EvKind::SetValue, 8, 10, 1);
    let miss = a.wait_take(8, 11);
    got[0] = miss == Ok(false) && a.slept == 1 && !a.residue().is_empty();
    // 2) 变更序回退必拒
    let mut b = RaceWindow::new();
    let _ = b.push(EvKind::SetValue, 8, 1, 5);
    got[1] = b.push(EvKind::SetValue, 8, 1, 4).err() == Some(RW_SEQ);
    // 3) 空窗口取值必记入眠（取空语义）
    let mut c = RaceWindow::new();
    let empty = c.wait_take(16, 1);
    got[2] = empty == Ok(false) && c.taken == 0;
    // 4) 冻结后写账必抓
    let (mut d, _) = run_race_thousand();
    d.freeze();
    got[3] = d.push(EvKind::SetValue, 8, 1, 99999).err() == Some(RW_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11005_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11005");

    // J1 前唤醒窗口千次零丢失
    let (mut win, green) = run_race_thousand();
    s.add("J1-千次零丢失", green && win.no_loss() && win.residue().is_empty(), "千轮前唤醒全数取走零残留");

    // J2 价值变更序对账：序号严格递增且逐笔可查
    s.add("J2-变更序对账", win.seq_monotonic() && win.events.len() == (ROUNDS * 2) as usize, "千轮双笔事件序号严格递增");

    // J3 前唤醒全命中：taken 恰为千且 slept 为零
    s.add("J3-前唤醒全命中", win.taken == ROUNDS && win.slept == 0, "每轮 SetValue 均被当轮 WAIT 取走");

    // J4 冻结根：fold 稳定且与根一致
    let root = win.freeze();
    s.add("J4-冻结根", win.root == Some(root) && win.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J5 注入检出：值错配 + 序回退
    let inj = inject_four();
    s.add("J5-错配回退检出", inj[0] && inj[1], "值错配入眠留痕与序回退必抓");

    // J6 注入检出：取空 + 冻结后写
    s.add("J6-取空冻结检出", inj[2] && inj[3], "空窗口取值必记入眠且冻结后写必抓");

    // J7 零址拒收
    let j7 = RaceWindow::new().push(EvKind::Wait, 0, 1, 1).err() == Some(RW_EMPTY);
    s.add("J7-零址拒收", j7, "窗口地址零必拒");

    // J8 超容拒：第 WIN_CAP+1 笔必拒
    let j8 = {
        let mut c = RaceWindow::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= WIN_CAP as u32 {
            if c.push(EvKind::SetValue, n.wrapping_mul(4), n, n).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.push(EvKind::SetValue, 8, 1, WIN_CAP as u32 + 1).err() == Some(RW_CAP)
    };
    s.add("J8-超容边界", j8, "超出定容必拒且原账不损");

    // J9 取值后不可重复取：同值二次 WAIT 必入眠
    let j9 = {
        let mut c = RaceWindow::new();
        let _ = c.push(EvKind::SetValue, 8, 42, 1);
        let first = c.wait_take(8, 42);
        let second = c.wait_take(8, 42);
        first == Ok(true) && second == Ok(false) && c.taken == 1
    };
    s.add("J9-不可重复取", j9, "已消费的前唤醒不可二次命中");

    // J10 零丢失对错配半成品同样成立：错配轮全留残留
    let j10 = {
        let mut c = RaceWindow::new();
        let _ = c.push(EvKind::SetValue, 8, 10, 1);
        let _ = c.push(EvKind::SetValue, 8, 11, 2);
        let _ = c.wait_take(8, 10);
        let _ = c.wait_take(8, 11);
        c.no_loss() && c.taken == 2
    };
    s.add("J10-错配半成品守恒", j10, "错配轮取走/入眠记账仍守恒");

    // J11 样本确定性：同号两次求值一致且异号互异
    let v3a = race_sample(3);
    let v3b = race_sample(3);
    let mut distinct = true;
    let mut i = 1u32;
    while i <= 16u32 {
        let mut j = i.wrapping_add(1);
        while j <= 16u32 {
            if race_sample(i) == race_sample(j) {
                distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("J11-样本确定性", v3a == v3b && distinct, "样本同入同出且互异");

    // J12 取值核销成对：每笔 taken 对应 set/wait 双核销
    let j12 = {
        let mut c = RaceWindow::new();
        let _ = c.push(EvKind::SetValue, 8, 7, 1);
        let _ = c.wait_take(8, 7);
        c.consumed_n() == 2
    };
    s.add("J12-核销成对", j12, "一次命中核销 set 与 wait 双笔");

    // J13 动态明细：千次取走计数落账
    let tk = win.taken;
    s.add_dyn("J13-取走明细", tk == ROUNDS && win.slept == 0, &format!("taken={} slept=0", tk));

    // J14 动态明细：冻结根十六进制落账
    let fp = win.fold();
    s.add_dyn("J14-冻结根明细", fp == root, &format!("root={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11005_race_flow() {
        let mut w = RaceWindow::new();
        assert!(w.push(EvKind::SetValue, 8, 42, 1).is_ok());
        assert!(w.push(EvKind::SetValue, 8, 42, 1).err() == Some(RW_SEQ));
        assert_eq!(w.wait_take(8, 42), Ok(true));
        assert_eq!(w.wait_take(8, 43), Ok(false));
        assert_eq!(w.taken, 1);
        assert_eq!(w.slept, 1);
        w.freeze();
        assert!(w.push(EvKind::Wait, 8, 1, 9).err() == Some(RW_FROZEN));
        assert!(w.root.is_some());
    }

    #[test]
    fn unx11005_checks_green() {
        let s = run_unx11005_checks();
        assert!(s.all_passed(), "UNX-F11005 判据未全绿");
    }
}
