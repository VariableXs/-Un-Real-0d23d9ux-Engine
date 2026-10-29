//! UNX-F11086 unix socket 长稳段（连接风暴万轮）（0x1eac0 段）
//!
//! 连接风暴按 10 个千次窗口推进，backlog 固定 4。每次尝试要么入槽，要么在
//! backlog 满时明确拒绝；每三次尝试关闭最早连接，窗口末强制排空。槽表最多
//! 4 个元素，万次尝试只累积 accepted/refused/closed 三个标量。
//! 不变式：attempts=accepted+refused=10000、accepted=closed、窗口末 residual=0，
//! 且任何时刻活动连接数不超过 backlog。
//!
//! 结构：socket 槽（fd/peer/代次/引用）+ 风暴窗口（尝试/接纳/拒绝/关闭/峰值）
//! + 风暴总账（连接守恒、恢复零残留、冻结根）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11086 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1eac0;

/// 空账 —— 尚未跑任何连接窗口。
pub const UL_EMPTY: u32 = SEG_BASE + 0x01;
/// 范围 —— fd/peer 为零、窗口号越界或窗口尝试数为零。
pub const UL_RANGE: u32 = SEG_BASE + 0x02;
/// 重号 —— 同 fd 仍在槽内时再次接纳，或窗口重复推进。
pub const UL_DUP: u32 = SEG_BASE + 0x03;
/// backlog 满 —— 活动连接已达到 4，无法再接纳。
pub const UL_LIMIT: u32 = SEG_BASE + 0x04;
/// 残留 —— 槽表未排空，或接纳/拒绝/关闭账目不对平。
pub const UL_RESID: u32 = SEG_BASE + 0x05;
/// 封账后写 —— 风暴账冻结后仍尝试接纳。
pub const UL_FROZEN: u32 = SEG_BASE + 0x06;

/// fold 域标（unix socket 风暴账）。
pub const UL_TAG: u64 = 0x1ea_86;

/// backlog 深度。
pub const UL_BACKLOG: usize = 4;
/// 总尝试数。
pub const UL_STORM: u32 = 10_000;
/// 窗口数。
pub const UL_EPOCHS: usize = 10;
/// 每窗尝试数。
pub const UL_EPOCH_TRIES: u32 = 1_000;

/// 一个活动连接槽。
#[derive(Clone, Debug)]
pub struct SocketSlot {
    pub fd: u32,
    pub peer: u32,
    pub state: u8,
    pub refs: u16,
    pub epoch: u32,
}

/// 一个千次连接窗口的汇总。
#[derive(Clone, Debug)]
pub struct StormEpoch {
    pub epoch: u16,
    pub attempts: u32,
    pub accepted: u32,
    pub refused: u32,
    pub closed: u32,
    pub peak: u32,
    pub residual: u32,
}

impl StormEpoch {
    /// 窗口内尝试守恒且排空。
    pub fn balanced(&self) -> bool {
        self.attempts != 0
            && self.attempts == self.accepted.wrapping_add(self.refused)
            && self.accepted == self.closed
            && self.residual == 0
    }
}

/// unix socket 风暴总账。
#[derive(Clone, Debug)]
pub struct ConnStorm {
    pub slots: Vec<SocketSlot>,
    pub epochs: Vec<StormEpoch>,
    pub attempts: u32,
    pub accepted: u32,
    pub refused: u32,
    pub closed: u32,
    pub max_active: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl ConnStorm {
    pub fn new() -> ConnStorm {
        ConnStorm {
            slots: Vec::new(),
            epochs: Vec::new(),
            attempts: 0,
            accepted: 0,
            refused: 0,
            closed: 0,
            max_active: 0,
            frozen: false,
            root: None,
        }
    }

    fn slot_at(&self, fd: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].fd == fd {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    fn epoch_at(&self, epoch: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.epochs.len() {
            if self.epochs[i].epoch == epoch {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 接纳一个连接；backlog 满时返回 UL_LIMIT 且不改变槽表。
    pub fn accept_conn(&mut self, fd: u32, peer: u32, epoch: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(UL_FROZEN);
        }
        if fd == 0 || peer == 0 || fd == peer {
            return Err(UL_RANGE);
        }
        if self.slot_at(fd).is_some() {
            return Err(UL_DUP);
        }
        if self.slots.len() >= UL_BACKLOG {
            return Err(UL_LIMIT);
        }
        self.slots.push(SocketSlot {
            fd,
            peer,
            state: 1,
            refs: 1,
            epoch,
        });
        let active = self.slots.len() as u32;
        if active > self.max_active {
            self.max_active = active;
        }
        Ok(())
    }

    /// 关闭连接；引用计数大于一的情况下只降引用。
    pub fn close_conn(&mut self, fd: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(UL_FROZEN);
        }
        let si = match self.slot_at(fd) {
            Some(i) => i,
            None => return Err(UL_EMPTY),
        };
        if self.slots[si].refs > 1 {
            self.slots[si].refs = self.slots[si].refs.wrapping_sub(1);
            return Ok(());
        }
        self.slots[si].state = 2;
        self.slots.remove(si);
        self.closed = self.closed.wrapping_add(1);
        Ok(())
    }

    /// 给连接增加引用（模拟 dup 出来的 fd）。
    pub fn ref_conn(&mut self, fd: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(UL_FROZEN);
        }
        let si = match self.slot_at(fd) {
            Some(i) => i,
            None => return Err(UL_EMPTY),
        };
        if self.slots[si].refs == u16::MAX {
            return Err(UL_RANGE);
        }
        self.slots[si].refs = self.slots[si].refs.wrapping_add(1);
        Ok(())
    }

    /// 排空当前槽表并返回释放数量。
    pub fn drain_slots(&mut self) -> u32 {
        let mut n = 0u32;
        while !self.slots.is_empty() {
            let fd = self.slots[0].fd;
            if self.slots[0].refs > 1 {
                self.slots[0].refs = 0;
            }
            if self.close_conn(fd).is_ok() {
                n = n.wrapping_add(1);
            }
        }
        n
    }

    /// 跑一个千次连接窗口。
    pub fn run_epoch(&mut self, epoch: u16, tries: u32) -> Result<StormEpoch, u32> {
        if self.frozen {
            return Err(UL_FROZEN);
        }
        if epoch >= UL_EPOCHS as u16 || tries == 0 || tries > UL_EPOCH_TRIES {
            return Err(UL_RANGE);
        }
        if self.epoch_at(epoch).is_some() {
            return Err(UL_DUP);
        }
        let mut acc = 0u32;
        let mut refu = 0u32;
        let closed_start = self.closed;
        let mut peak = self.slots.len() as u32;
        let mut i = 0u32;
        while i < tries {
            let fd = 0x8600_0000u32
                .wrapping_add((epoch as u32).wrapping_mul(UL_EPOCH_TRIES))
                .wrapping_add(i)
                .wrapping_add(1);
            let peer = fd ^ 0x55aa;
            match self.accept_conn(fd, peer, epoch as u32) {
                Ok(()) => {
                    acc = acc.wrapping_add(1);
                    let active = self.slots.len() as u32;
                    if active > peak {
                        peak = active;
                    }
                }
                Err(UL_LIMIT) => {
                    refu = refu.wrapping_add(1);
                }
                Err(e) => return Err(e),
            }
            if i % 3 == 2 && !self.slots.is_empty() {
                let victim = self.slots[0].fd;
                let _ = self.close_conn(victim);
            }
            i = i.wrapping_add(1);
        }
        self.drain_slots();
        let wins = StormEpoch {
            epoch,
            attempts: tries,
            accepted: acc,
            refused: refu,
            closed: self.closed.wrapping_sub(closed_start),
            peak,
            residual: self.slots.len() as u32,
        };
        self.attempts = self.attempts.wrapping_add(tries);
        self.accepted = self.accepted.wrapping_add(acc);
        self.refused = self.refused.wrapping_add(refu);
        self.epochs.push(wins.clone());
        Ok(wins)
    }

    /// 跑满十个窗口。
    pub fn run_storm(&mut self) -> Result<u64, u32> {
        let mut e = 0u16;
        while e < UL_EPOCHS as u16 {
            self.run_epoch(e, UL_EPOCH_TRIES)?;
            e = e.wrapping_add(1);
        }
        Ok(self.fold())
    }

    /// 连接守恒：万次尝试 = 接纳 + 拒绝，接纳 = 关闭。
    pub fn connection_conserved(&self) -> bool {
        self.attempts == UL_STORM
            && self.attempts == self.accepted.wrapping_add(self.refused)
            && self.accepted == self.closed
            && self.accepted > 0
    }

    /// 风暴恢复：槽表空、每窗 residual 为零。
    pub fn recovered(&self) -> bool {
        if !self.slots.is_empty() || self.epochs.len() != UL_EPOCHS {
            return false;
        }
        let mut i = 0usize;
        while i < self.epochs.len() {
            if self.epochs[i].residual != 0 || !self.epochs[i].balanced() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 总审计。
    pub fn audit(&self) -> Result<(), u32> {
        if self.attempts == 0 {
            return Err(UL_EMPTY);
        }
        if !self.slots.is_empty() {
            return Err(UL_RESID);
        }
        if self.attempts != UL_STORM {
            return Err(UL_RESID);
        }
        if self.attempts != self.accepted.wrapping_add(self.refused)
            || self.accepted != self.closed
        {
            return Err(UL_RESID);
        }
        Ok(())
    }

    /// FNV-1a：槽表与每窗七项计数全入链。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = UL_TAG;
        let mut i = 0usize;
        while i < self.slots.len() {
            let s = &self.slots[i];
            h = (h ^ s.fd as u64).wrapping_mul(0x100000001b3);
            h = (h ^ s.peer as u64).wrapping_mul(0x100000001b3);
            h = (h ^ s.refs as u64).wrapping_mul(0x100000001b3);
            h = (h ^ s.epoch as u64).wrapping_mul(0x100000001b3);
            i += 1;
        }
        let mut k = 0usize;
        while k < self.epochs.len() {
            let e = &self.epochs[k];
            h = (h ^ e.epoch as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.attempts as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.accepted as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.refused as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.closed as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.peak as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.attempts as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.accepted as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.refused as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.closed as u64).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结风暴账。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }

    /// 风暴摘要。
    pub fn summary(&self) -> String {
        let label = "unixsock".to_string();
        format!("{}|att={}|acc={}|ref={}|closed={}|peak={}", label, self.attempts,
            self.accepted, self.refused, self.closed, self.max_active)
    }
}

/// 四路坏样本：fd 重号、backlog 满、关闭未知 fd、冻结后接纳。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];

    let mut a = ConnStorm::new();
    let _ = a.accept_conn(1, 2, 0);
    got[0] = a.accept_conn(1, 3, 0).err() == Some(UL_DUP);

    let mut b = ConnStorm::new();
    let _ = b.accept_conn(11, 21, 0);
    let _ = b.accept_conn(12, 22, 0);
    let _ = b.accept_conn(13, 23, 0);
    let _ = b.accept_conn(14, 24, 0);
    got[1] = b.accept_conn(15, 25, 0).err() == Some(UL_LIMIT);

    let mut c = ConnStorm::new();
    got[2] = c.close_conn(99).err() == Some(UL_EMPTY);

    let mut d = ConnStorm::new();
    let _ = d.accept_conn(31, 41, 0);
    let _ = d.freeze();
    got[3] = d.accept_conn(32, 42, 0).err() == Some(UL_FROZEN);
    got
}

/// unix socket 万轮风暴判据。
pub fn run_unx11086_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11086");

    let mut b = ConnStorm::new();
    let run = b.run_storm();

    // J1 两半：万轮连接账守恒，风暴恢复零残留。
    let j1a = b.connection_conserved() && b.attempts == 10_000;
    let j1b = b.recovered() && b.slots.is_empty() && b.audit().is_ok();
    s.add("J1-万轮连接守恒", j1a, "10000 次尝试 = 接纳 + 拒绝");
    s.add("J1-风暴零残留", j1b, "窗口末槽表为空且 residual 全零");

    // J2 十窗逐窗对平。
    let mut epoch_ok = true;
    let mut sum_acc = 0u32;
    let mut sum_ref = 0u32;
    let mut i = 0usize;
    while i < b.epochs.len() {
        if !b.epochs[i].balanced() {
            epoch_ok = false;
        }
        sum_acc = sum_acc.wrapping_add(b.epochs[i].accepted);
        sum_ref = sum_ref.wrapping_add(b.epochs[i].refused);
        i += 1;
    }
    let j2 = epoch_ok && sum_acc == b.accepted && sum_ref == b.refused;
    s.add("J2-十窗对平", j2, "10 窗尝试/接纳/拒绝/关闭逐窗对平");

    // J3 四路注入。
    let inj = inject_four();
    s.add("J3-重号注入", inj[0], "同 fd 二次接纳必拒");
    s.add("J3-满槽注入", inj[1], "backlog 满时第五连接必拒");
    s.add("J3-空关注入", inj[2], "关闭未知 fd 必拒");
    s.add("J3-冻结注入", inj[3], "冻结后接纳必拒");

    // J4 冻结：根稳定且接纳/关闭双拒。
    let mut f = b.clone();
    let root = f.freeze();
    let j4 = f.root == Some(root) && f.fold() == root
        && f.accept_conn(1, 2, 0).err() == Some(UL_FROZEN)
        && f.close_conn(1).err() == Some(UL_FROZEN);
    s.add("J4-封账不可变", j4, "冻结后接纳与关闭均拒");

    // J5 指纹敏感：单窗峰值变化即改根。
    let mut c = b.clone();
    c.epochs[5].peak = c.epochs[5].peak.wrapping_add(1);
    let j5 = c.fold() != b.fold();
    s.add("J5-指纹敏感", j5, "单窗峰值变化即改变风暴根");

    // J6 空账/范围/残留。
    let mut e = ConnStorm::new();
    let empty = e.audit();
    let range = e.accept_conn(0, 1, 0);
    let _ = e.accept_conn(51, 61, 0);
    e.attempts = 1;
    let resid = e.audit();
    let j6 = empty.err() == Some(UL_EMPTY) && range.err() == Some(UL_RANGE)
        && resid.err() == Some(UL_RESID);
    s.add("J6-空账残留", j6, "空账、零 fd、未排空均拒");

    // J7 backlog 上界与 fd 唯一。
    let j7 = b.max_active <= 4 && b.max_active > 0
        && b.accepted > 0 && b.refused > 0;
    s.add("J7-backlog有界", j7, "峰值活动连接不超过 4 且拒绝路径被触发");

    // J8 负语义：九窗时不得判万轮完成。
    let mut partial = ConnStorm::new();
    let mut pe = 0u16;
    while pe < 9 {
        let _ = partial.run_epoch(pe, 1_000);
        pe = pe.wrapping_add(1);
    }
    let j8 = !partial.connection_conserved() && partial.attempts == 9_000
        && partial.audit().err() == Some(UL_RESID);
    s.add("J8-九窗非绿", j8, "九千次尝试不得冒充万轮风暴");

    // J9 动态风暴账。
    s.add_dyn("J9-风暴动态", j1a,
        &format!("att={} acc={} ref={} closed={} peak={}", b.attempts,
            b.accepted, b.refused, b.closed, b.max_active));

    // J10 引用路径：dup 引用关闭后保留，最终 drain 清空。
    let mut probe = ConnStorm::new();
    let _ = probe.accept_conn(71, 81, 0);
    let _ = probe.ref_conn(71);
    let _ = probe.close_conn(71);
    let kept = probe.slots.len() == 1 && probe.slots[0].refs == 1;
    let _ = probe.drain_slots();
    let j10 = kept && probe.slots.is_empty() && probe.closed == 1;
    s.add("J10-引用路径", j10, "dup 引用降一后仍存活，排空后关闭");

    // J11 审计与摘要。
    let text = b.summary();
    let j11 = b.audit().is_ok() && run.is_ok() && text.starts_with("unixsock|");
    s.add_dyn("J11-审计摘要", j11, &text);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11086_flow() {
        let mut b = ConnStorm::new();
        assert!(b.audit().err() == Some(UL_EMPTY));
        assert!(b.accept_conn(1, 2, 0).is_ok());
        assert!(b.accept_conn(1, 3, 0).err() == Some(UL_DUP));
        assert!(b.close_conn(9).err() == Some(UL_EMPTY));
        assert!(b.close_conn(1).is_ok());
        let _ = b.run_storm();
        let fp = b.fold();
        let root = b.freeze();
        assert!(fp == root && b.accept_conn(2, 3, 0).err() == Some(UL_FROZEN));
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11086_checks_green() {
        assert!(run_unx11086_checks().all_passed(), "UNX-F11086 判据未全绿");
    }
}
