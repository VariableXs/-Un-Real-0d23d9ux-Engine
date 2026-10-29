//! UNX-F11088 信号长稳段（万次风暴与合并账）（0x1eb00 段）
//!
//! 标准信号只有一位 pending：同一信号在未投递前再次 raise 只会合并，不会
//! 排队第二份。风暴按 10 个千次窗口推进，每窗故意在信号 1 上制造“先布防、
//! 再重复 raise、随后解防投递”的合并段；窗口末统一 drain，保证所有 pending
//! 都有归宿。不变式：sent = delivered + coalesced = 10000，slot 侧投递/合并
//! 计数与全局账逐项对平，窗口末 pending=0。
//!
//! 结构：信号槽（pending/blocked/投递/合并）+ 风暴窗口（发送/投递/合并/峰值）
//! + 信号总账（万次守恒、合并语义、冻结根）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11088 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1eb00;

/// 空账 —— 尚未跑任何信号窗口。
pub const SG_EMPTY: u32 = SEG_BASE + 0x01;
/// 范围 —— 信号号不在 1..=8、窗口或发送数越界。
pub const SG_RANGE: u32 = SEG_BASE + 0x02;
/// 重号 —— 同一信号重复注册，或窗口重复推进。
pub const SG_DUP: u32 = SEG_BASE + 0x03;
/// 丢失 —— sent 不等于 delivered+coalesced，或窗口末仍有 pending。
pub const SG_LOST: u32 = SEG_BASE + 0x04;
/// 合并账错 —— slot 合并计数与全局合并计数不相等。
pub const SG_MERGE: u32 = SEG_BASE + 0x05;
/// 封账后写 —— 总账冻结后仍 raise/deliver。
pub const SG_FROZEN: u32 = SEG_BASE + 0x06;

/// fold 域标（信号风暴账）。
pub const SG_TAG: u64 = 0x1ea_88;

/// 信号数。
pub const SG_SIGNALS: usize = 8;
/// 窗口数。
pub const SG_WINDOWS: usize = 10;
/// 每窗发送数。
pub const SG_PER_WINDOW: u32 = 1_000;
/// 万次发送总数。
pub const SG_SENT: u32 = 10_000;

/// 一个标准信号槽。
#[derive(Clone, Debug)]
pub struct SignalSlot {
    pub signo: u8,
    pub pending: bool,
    pub blocked: bool,
    pub delivered: u32,
    pub coalesced: u32,
    pub generation: u16,
}

impl SignalSlot {
    /// 当前是否还有未投递的 pending 位。
    pub fn live(&self) -> bool {
        self.pending
    }
}

/// 一个千次发送窗口。
#[derive(Clone, Debug)]
pub struct SignalWindow {
    pub epoch: u16,
    pub sent: u32,
    pub delivered: u32,
    pub coalesced: u32,
    pub max_pending: u8,
    pub blocked_rounds: u32,
}

impl SignalWindow {
    /// 窗口守恒：发送 = 投递 + 合并。
    pub fn balanced(&self) -> bool {
        self.sent != 0
            && self.sent == self.delivered.wrapping_add(self.coalesced)
    }
}

/// 信号风暴总账。
#[derive(Clone, Debug)]
pub struct SignalStorm {
    pub slots: Vec<SignalSlot>,
    pub windows: Vec<SignalWindow>,
    pub sent: u64,
    pub delivered: u64,
    pub coalesced: u64,
    pub drained: u32,
    pub blocked_rounds: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl SignalStorm {
    pub fn new() -> SignalStorm {
        SignalStorm {
            slots: Vec::new(),
            windows: Vec::new(),
            sent: 0,
            delivered: 0,
            coalesced: 0,
            drained: 0,
            blocked_rounds: 0,
            frozen: false,
            root: None,
        }
    }

    fn slot_at(&self, signo: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].signo == signo {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    fn window_at(&self, epoch: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.windows.len() {
            if self.windows[i].epoch == epoch {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 注册标准信号。
    pub fn register(&mut self, signo: u8) -> Result<(), u32> {
        if self.frozen {
            return Err(SG_FROZEN);
        }
        if signo == 0 || signo > SG_SIGNALS as u8 {
            return Err(SG_RANGE);
        }
        if self.slot_at(signo).is_some() {
            return Err(SG_DUP);
        }
        if self.slots.len() >= SG_SIGNALS {
            return Err(SG_RANGE);
        }
        self.slots.push(SignalSlot {
            signo,
            pending: false,
            blocked: false,
            delivered: 0,
            coalesced: 0,
            generation: signo as u16,
        });
        Ok(())
    }

    /// 屏蔽信号。
    pub fn block(&mut self, signo: u8) -> Result<(), u32> {
        if self.frozen {
            return Err(SG_FROZEN);
        }
        let si = match self.slot_at(signo) {
            Some(i) => i,
            None => return Err(SG_RANGE),
        };
        self.slots[si].blocked = true;
        Ok(())
    }

    /// 解除屏蔽。
    pub fn unblock(&mut self, signo: u8) -> Result<(), u32> {
        if self.frozen {
            return Err(SG_FROZEN);
        }
        let si = match self.slot_at(signo) {
            Some(i) => i,
            None => return Err(SG_RANGE),
        };
        self.slots[si].blocked = false;
        Ok(())
    }

    /// 投递一个信号；返回 true 表示发生合并（pending 位已存在）。
    pub fn raise(&mut self, signo: u8) -> Result<bool, u32> {
        if self.frozen {
            return Err(SG_FROZEN);
        }
        let si = match self.slot_at(signo) {
            Some(i) => i,
            None => return Err(SG_RANGE),
        };
        if self.slots[si].pending {
            self.slots[si].coalesced = self.slots[si].coalesced.wrapping_add(1);
            return Ok(true);
        }
        self.slots[si].pending = true;
        Ok(false)
    }

    /// 投递 pending；没有 pending 或仍被屏蔽时拒绝。
    pub fn deliver(&mut self, signo: u8) -> Result<(), u32> {
        if self.frozen {
            return Err(SG_FROZEN);
        }
        let si = match self.slot_at(signo) {
            Some(i) => i,
            None => return Err(SG_RANGE),
        };
        if self.slots[si].blocked || !self.slots[si].pending {
            return Err(SG_EMPTY);
        }
        self.slots[si].pending = false;
        self.slots[si].delivered = self.slots[si].delivered.wrapping_add(1);
        Ok(())
    }

    /// 当前 pending 槽数。
    pub fn pending_count(&self) -> u8 {
        let mut n = 0u8;
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].pending {
                n = n.wrapping_add(1);
            }
            i += 1;
        }
        n
    }

    /// 窗口末排空：解除全部屏蔽并投递所有 pending。
    pub fn drain_all(&mut self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.slots.len() {
            self.slots[i].blocked = false;
            if self.slots[i].pending {
                self.slots[i].pending = false;
                self.slots[i].delivered = self.slots[i].delivered.wrapping_add(1);
                n = n.wrapping_add(1);
            }
            i += 1;
        }
        n
    }

    /// 一个千次发送窗口。block 段固定在每 32 次发送的首位，第 9 次重复 raise。
    pub fn run_window(&mut self, epoch: u16, tries: u32) -> Result<SignalWindow, u32> {
        if self.frozen {
            return Err(SG_FROZEN);
        }
        if epoch >= SG_WINDOWS as u16 || tries == 0 || tries > SG_PER_WINDOW {
            return Err(SG_RANGE);
        }
        if self.window_at(epoch).is_some() {
            return Err(SG_DUP);
        }
        if self.slots.len() != SG_SIGNALS {
            return Err(SG_EMPTY);
        }
        let mut sent = 0u32;
        let mut del = 0u32;
        let mut coal = 0u32;
        let mut blocked = 0u32;
        let mut max_pending = 0u8;
        let mut i = 0u32;
        while i < tries {
            let signo = (i % 8) as u8 + 1;
            if i % 32 == 0 {
                let _ = self.block(signo);
                blocked = blocked.wrapping_add(1);
            }
            match self.raise(signo) {
                Ok(true) => coal = coal.wrapping_add(1),
                Ok(false) => {}
                Err(e) => return Err(e),
            }
            sent = sent.wrapping_add(1);
            if i % 32 == 8 {
                let _ = self.unblock(signo);
                if self.deliver(signo).is_ok() {
                    del = del.wrapping_add(1);
                }
            } else if i % 2 == 0 {
                if self.deliver(signo).is_ok() {
                    del = del.wrapping_add(1);
                }
            }
            let pending = self.pending_count();
            if pending > max_pending {
                max_pending = pending;
            }
            i = i.wrapping_add(1);
        }
        let drained = self.drain_all();
        del = del.wrapping_add(drained);
        self.sent = self.sent.wrapping_add(sent as u64);
        self.delivered = self.delivered.wrapping_add(del as u64);
        self.coalesced = self.coalesced.wrapping_add(coal as u64);
        self.drained = self.drained.wrapping_add(drained);
        self.blocked_rounds = self.blocked_rounds.wrapping_add(blocked);
        let rec = SignalWindow {
            epoch,
            sent,
            delivered: del,
            coalesced: coal,
            max_pending,
            blocked_rounds: blocked,
        };
        self.windows.push(rec.clone());
        Ok(rec)
    }

    /// 跑满十个窗口。
    pub fn run_all(&mut self) -> Result<u64, u32> {
        let mut e = 0u16;
        while e < SG_WINDOWS as u16 {
            self.run_window(e, SG_PER_WINDOW)?;
            e = e.wrapping_add(1);
        }
        Ok(self.fold())
    }

    /// slot 投递数总和。
    pub fn slot_delivered_sum(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.slots.len() {
            n = n.wrapping_add(self.slots[i].delivered);
            i += 1;
        }
        n
    }

    /// slot 合并数总和。
    pub fn slot_coalesced_sum(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.slots.len() {
            n = n.wrapping_add(self.slots[i].coalesced);
            i += 1;
        }
        n
    }

    /// 万次风暴守恒。
    pub fn storm_conserved(&self) -> bool {
        self.sent == SG_SENT as u64
            && self.delivered.wrapping_add(self.coalesced) == self.sent
            && self.pending_count() == 0
            && self.slot_delivered_sum() as u64 == self.delivered
            && self.windows.len() == SG_WINDOWS
    }

    /// 合并语义：每个窗口都有合并，slot 合并和等于全局合并。
    pub fn merge_semantics_ok(&self) -> bool {
        if self.coalesced == 0 || self.coalesced >= self.sent {
            return false;
        }
        if self.slot_coalesced_sum() as u64 != self.coalesced {
            return false;
        }
        let mut i = 0usize;
        while i < self.windows.len() {
            if self.windows[i].coalesced == 0 || !self.windows[i].balanced() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 总审计。
    pub fn audit(&self) -> Result<(), u32> {
        if self.sent == 0 {
            return Err(SG_EMPTY);
        }
        if self.pending_count() != 0 || self.sent != SG_SENT as u64 {
            return Err(SG_LOST);
        }
        if self.delivered.wrapping_add(self.coalesced) != self.sent {
            return Err(SG_LOST);
        }
        if self.slot_coalesced_sum() as u64 != self.coalesced
            || self.slot_delivered_sum() as u64 != self.delivered
        {
            return Err(SG_MERGE);
        }
        Ok(())
    }

    /// FNV-1a：槽位与每窗六项计数全入链。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = SG_TAG;
        let mut i = 0usize;
        while i < self.slots.len() {
            let s = &self.slots[i];
            h = (h ^ s.signo as u64).wrapping_mul(0x100000001b3);
            h = (h ^ if s.pending { 1 } else { 0 }).wrapping_mul(0x100000001b3);
            h = (h ^ if s.blocked { 1 } else { 0 }).wrapping_mul(0x100000001b3);
            h = (h ^ s.delivered as u64).wrapping_mul(0x100000001b3);
            h = (h ^ s.coalesced as u64).wrapping_mul(0x100000001b3);
            i += 1;
        }
        let mut k = 0usize;
        while k < self.windows.len() {
            let w = &self.windows[k];
            h = (h ^ w.epoch as u64).wrapping_mul(0x100000001b3);
            h = (h ^ w.sent as u64).wrapping_mul(0x100000001b3);
            h = (h ^ w.delivered as u64).wrapping_mul(0x100000001b3);
            h = (h ^ w.coalesced as u64).wrapping_mul(0x100000001b3);
            h = (h ^ w.blocked_rounds as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.sent).wrapping_mul(0x100000001b3);
        h = (h ^ self.delivered).wrapping_mul(0x100000001b3);
        h = (h ^ self.coalesced).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结总账。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }

    /// 风暴摘要。
    pub fn summary(&self) -> String {
        let label = "signal".to_string();
        format!("{}|sent={}|del={}|coal={}|drain={}|blocked={}", label, self.sent,
            self.delivered, self.coalesced, self.drained, self.blocked_rounds)
    }
}

/// 四路坏样本：信号重号、信号号越界、空 pending 投递、冻结后 raise。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];

    let mut a = SignalStorm::new();
    let _ = a.register(1);
    got[0] = a.register(1).err() == Some(SG_DUP);

    let mut b = SignalStorm::new();
    got[1] = b.register(9).err() == Some(SG_RANGE);

    let mut c = SignalStorm::new();
    let _ = c.register(2);
    got[2] = c.deliver(2).err() == Some(SG_EMPTY);

    let mut d = SignalStorm::new();
    let _ = d.register(3);
    let _ = d.freeze();
    got[3] = d.raise(3).err() == Some(SG_FROZEN);
    got
}

/// 信号万次风暴判据。
pub fn run_unx11088_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11088");

    let mut b = SignalStorm::new();
    let mut signo = 1u8;
    while signo <= SG_SIGNALS as u8 {
        let _ = b.register(signo);
        signo = signo.wrapping_add(1);
    }
    let run = b.run_all();

    // J1 两半：万次风暴账守恒，合并语义万次零漂。
    let j1a = b.storm_conserved() && b.sent == 10_000;
    let j1b = b.merge_semantics_ok() && b.coalesced > 0
        && b.delivered.wrapping_add(b.coalesced) == b.sent;
    s.add("J1-万次风暴守恒", j1a, "sent 等于 delivered+coalesced 且 pending 清零");
    s.add("J1-合并语义零漂", j1b, "每窗均有合并且槽账与总账对平");

    // J2 十窗逐窗对平且合并累计一致。
    let mut sum_del = 0u64;
    let mut sum_coal = 0u64;
    let mut win_ok = true;
    let mut i = 0usize;
    while i < b.windows.len() {
        if !b.windows[i].balanced() {
            win_ok = false;
        }
        sum_del = sum_del.wrapping_add(b.windows[i].delivered as u64);
        sum_coal = sum_coal.wrapping_add(b.windows[i].coalesced as u64);
        i += 1;
    }
    let j2 = win_ok && sum_del == b.delivered && sum_coal == b.coalesced;
    s.add("J2-十窗总账", j2, "10 窗发送/投递/合并与总账对平");

    // J3 四路注入。
    let inj = inject_four();
    s.add("J3-重号注入", inj[0], "同信号二次注册必拒");
    s.add("J3-越界注入", inj[1], "信号 9 越界必拒");
    s.add("J3-空投注入", inj[2], "无 pending 投递必拒");
    s.add("J3-冻结注入", inj[3], "冻结后 raise 必拒");

    // J4 冻结：根稳定且 raise/deliver 双拒。
    let mut f = b.clone();
    let root = f.freeze();
    let j4 = f.root == Some(root) && f.fold() == root
        && f.raise(1).err() == Some(SG_FROZEN)
        && f.deliver(1).err() == Some(SG_FROZEN);
    s.add("J4-封账不可变", j4, "冻结后 raise/deliver 均拒");

    // J5 指纹敏感：单槽合并数变化即改根。
    let mut c = b.clone();
    c.slots[3].coalesced = c.slots[3].coalesced.wrapping_add(1);
    let j5 = c.fold() != b.fold();
    s.add("J5-指纹敏感", j5, "单槽合并数变化即改变信号根");

    // J6 空账/范围/丢账。
    let mut e = SignalStorm::new();
    let empty = e.audit();
    let range = e.register(0);
    let _ = e.register(4);
    let _ = e.raise(4);
    e.sent = 1;
    let lost = e.audit();
    let j6 = empty.err() == Some(SG_EMPTY) && range.err() == Some(SG_RANGE)
        && lost.err() == Some(SG_LOST);
    s.add("J6-空账丢账", j6, "空账、零信号、pending 未清均拒");

    // J7 唯一有序：8 个信号号严格递增，窗口号 0..9。
    let mut ordered = true;
    let mut k = 1usize;
    while k < b.slots.len() {
        if b.slots[k].signo <= b.slots[k - 1].signo {
            ordered = false;
        }
        k += 1;
    }
    let mut w = 0usize;
    while w < b.windows.len() {
        if b.windows[w].epoch as usize != w {
            ordered = false;
        }
        w += 1;
    }
    s.add("J7-信号有序", ordered, "8 信号与 10 窗口编号严格递增");

    // J8 负语义：九窗时不得判万次全绿。
    let mut partial = SignalStorm::new();
    let mut ps = 1u8;
    while ps <= 8 {
        let _ = partial.register(ps);
        ps = ps.wrapping_add(1);
    }
    let mut pe = 0u16;
    while pe < 9 {
        let _ = partial.run_window(pe, 1_000);
        pe = pe.wrapping_add(1);
    }
    let j8 = !partial.storm_conserved() && partial.sent == 9_000
        && partial.audit().err() == Some(SG_LOST);
    s.add("J8-九窗非绿", j8, "九千次发送不得冒充万次风暴");

    // J9 动态风暴账。
    s.add_dyn("J9-风暴动态", j1a,
        &format!("sent={} del={} coal={} drain={} blocked={}", b.sent,
            b.delivered, b.coalesced, b.drained, b.blocked_rounds));

    // J10 合并账：至少一次合并，且 blocked 段确实发生。
    let j10 = b.coalesced > 0 && b.blocked_rounds > 0
        && b.slot_coalesced_sum() as u64 == b.coalesced;
    s.add("J10-合并路径", j10, "布防窗口产生真实 pending 合并");

    // J11 审计与摘要。
    let text = b.summary();
    let j11 = b.audit().is_ok() && run.is_ok() && text.starts_with("signal|");
    s.add_dyn("J11-审计摘要", j11, &text);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11088_flow() {
        let mut probe = SignalStorm::new();
        assert!(probe.audit().err() == Some(SG_EMPTY));
        assert!(probe.register(1).is_ok());
        assert!(probe.register(1).err() == Some(SG_DUP));
        assert!(probe.deliver(1).err() == Some(SG_EMPTY));
        assert!(probe.raise(1).is_ok());
        assert!(probe.raise(1).ok() == Some(true));
        assert!(probe.deliver(1).is_ok());

        let mut b = SignalStorm::new();
        let mut s = 1u8;
        while s <= 8 {
            let _ = b.register(s);
            s = s.wrapping_add(1);
        }
        let _ = b.run_all();
        let fp = b.fold();
        let root = b.freeze();
        assert!(fp == root && b.raise(1).err() == Some(SG_FROZEN));
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11088_checks_green() {
        assert!(run_unx11088_checks().all_passed(), "UNX-F11088 判据未全绿");
    }
}
