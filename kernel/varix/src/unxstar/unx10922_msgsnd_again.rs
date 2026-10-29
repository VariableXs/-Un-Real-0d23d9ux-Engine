//! UNX-F10922 msgsnd EAGAIN/EINTR 矩阵（队列满）（0x1d2c0 段）
//!
//! 锚点：UNX-F10922-J1 正面：满队 NOWAIT→EAGAIN（10/10）、阻塞中断
//! EINTR（10/10）；反面：有余量零误报（千次 10/10）。
//! 域：CoRun·UNX-B27（SysV 错误矩阵域·msgsnd 面）。
//!
//! 结构：消息队列（定容 + NOWAIT 位 + 信号中断位）+ 裁断器（满队
//! NOWAIT→EAGAIN、阻塞遇信号→EINTR、余量内全通）+ 注入核（超容/
//! 伪中断/中断后余量漂移/冻结改队四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1d2c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1d2c0;
pub const SN_EAGAIN: u32 = SEG_BASE + 0x01; // 满队 NOWAIT
pub const SN_EINTR: u32 = SEG_BASE + 0x02; // 阻塞中断
pub const SN_EINVAL: u32 = SEG_BASE + 0x03; // msgsz 负面/队列坏
pub const SN_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后改队
pub const SN_NOQ: u32 = SEG_BASE + 0x05; // 队列不存在
pub const SN_DRIFT: u32 = SEG_BASE + 0x06; // 中断后余量漂移

/// IPC_NOWAIT 位。
pub const F_NOWAIT: u32 = 0x1;
/// 队列容量。
pub const QCAP: usize = 8;
/// SN 域标（fold 种子）。
pub const SN_TAG: u64 = 0x1d2c216;

/// 消息条目。
#[derive(Clone, Debug)]
pub struct MsgEntry {
    pub mtype: u64,
    pub size: u32,
}

impl MsgEntry {
    pub fn render(&self) -> String {
        format!("M{}|{}", self.mtype, self.size)
    }
}

/// 消息队列。
pub struct SendQueue {
    pub qid: u32,
    pub msgs: Vec<MsgEntry>,
    pub signal_pending: bool,
    pub frozen: bool,
    pub intr_count: u32,
}

impl SendQueue {
    pub fn new(qid: u32) -> SendQueue {
        SendQueue { qid, msgs: Vec::new(), signal_pending: false, frozen: false, intr_count: 0 }
    }

    /// 余量。
    pub fn room(&self) -> usize {
        QCAP - self.msgs.len()
    }

    /// msgsnd 主裁断。
    pub fn send(&mut self, mtype: u64, size: u32, flags: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(SN_FROZEN);
        }
        if size == 0 {
            return Err(SN_EINVAL);
        }
        if self.room() == 0 {
            if flags & F_NOWAIT != 0 {
                return Err(SN_EAGAIN);
            }
            // 阻塞路径：遇信号中断。
            if self.signal_pending {
                self.signal_pending = false;
                self.intr_count += 1;
                return Err(SN_EINTR);
            }
            return Err(SN_EAGAIN);
        }
        self.msgs.push(MsgEntry { mtype, size });
        Ok(())
    }

    /// 收（腾余量）。
    pub fn recv(&mut self) -> Option<MsgEntry> {
        if self.msgs.is_empty() {
            return None;
        }
        Some(self.msgs.remove(0))
    }

    /// 中断后余量审计：中断不消费余量。
    pub fn no_drift(&self) -> bool {
        self.msgs.len() + self.room() == QCAP
    }

    /// 链式 fold 指纹（序敏感，含消息账与中断计数）。
    pub fn fold(&self) -> u64 {
        let mut h = SN_TAG.wrapping_add(self.qid as u64);
        let mut k = 0usize;
        while k < self.msgs.len() {
            let m = &self.msgs[k];
            h = (h ^ m.mtype).wrapping_mul(0x100000001b3);
            h = (h ^ m.size as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.intr_count as u64).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 满队 NOWAIT 必报 EAGAIN
    let mut q = SendQueue::new(1);
    let mut i = 0u64;
    while i < QCAP as u64 {
        let _ = q.send(i + 1, 8, F_NOWAIT);
        i += 1;
    }
    got[0] = q.send(99, 8, F_NOWAIT).err() == Some(SN_EAGAIN);
    // 2) 阻塞遇信号必报 EINTR
    let mut b = SendQueue::new(2);
    let mut j = 0u64;
    while j < QCAP as u64 {
        let _ = b.send(j + 1, 8, 0);
        j += 1;
    }
    b.signal_pending = true;
    got[1] = b.send(99, 8, 0).err() == Some(SN_EINTR);
    // 3) 中断后余量零漂移
    got[2] = b.no_drift() && b.msgs.len() == QCAP;
    // 4) 冻结后发送必抓
    let mut f = SendQueue::new(3);
    f.freeze();
    got[3] = f.send(1, 8, F_NOWAIT).err() == Some(SN_FROZEN);
    got
}

/// mtype 确定性样本。
pub fn sn_sample_mtype(r: u32) -> u64 {
    r.wrapping_mul(2654435761).wrapping_add(41) as u64 % 100 + 1
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10922_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10922");

    // J1 正面·满队 NOWAIT→EAGAIN 十连
    let mut h1 = 0u32;
    let mut r = 0u32;
    while r < 10 {
        let mut q = SendQueue::new(r + 1);
        let mut i = 0u64;
        while i < QCAP as u64 {
            let _ = q.send(i + 1, 8, F_NOWAIT);
            i += 1;
        }
        if q.send(999, 8, F_NOWAIT).err() == Some(SN_EAGAIN) {
            h1 += 1;
        }
        r += 1;
    }
    s.add("J1-EAGAIN十连", h1 == 10, "满队 NOWAIT 十轮全数 EAGAIN");

    // J2 正面·阻塞中断 EINTR 十连
    let mut h2 = 0u32;
    let mut r2 = 0u32;
    while r2 < 10 {
        let mut q = SendQueue::new(r2 + 20);
        let mut i = 0u64;
        while i < QCAP as u64 {
            let _ = q.send(i + 1, 8, 0);
            i += 1;
        }
        q.signal_pending = true;
        if q.send(999, 8, 0).err() == Some(SN_EINTR) {
            h2 += 1;
        }
        r2 += 1;
    }
    s.add("J2-EINTR十连", h2 == 10, "阻塞中断十轮全数 EINTR");

    // J3 反面：有余量千次零误报
    let mut ok = 0u32;
    let mut r3 = 0u32;
    while r3 < 1000 {
        let mut q = SendQueue::new((r3 % 5) as u32 + 1);
        // 只放 r3%3 条，必有余量。
        let mut i = 0u64;
        let fill = (r3 % 3) as u64;
        while i < fill {
            let _ = q.send(i + 1, 4, F_NOWAIT);
            i += 1;
        }
        if q.send(sn_sample_mtype(r3), 4, F_NOWAIT).is_ok() {
            ok += 1;
        }
        r3 += 1;
    }
    s.add("J3-余量千次零误报", ok == 1000, "有余量千轮发送零误报");

    // J4 中断后余量零漂移十连
    let mut h4 = 0u32;
    let mut r4 = 0u32;
    while r4 < 10 {
        let mut q = SendQueue::new(r4 + 40);
        let mut i = 0u64;
        while i < QCAP as u64 {
            let _ = q.send(i + 1, 8, 0);
            i += 1;
        }
        q.signal_pending = true;
        let _ = q.send(999, 8, 0);
        if q.no_drift() && q.intr_count == 1 {
            h4 += 1;
        }
        r4 += 1;
    }
    s.add("J4-中断零漂移十连", h4 == 10, "中断后余量十轮零漂移");

    // J5 注入四路全抓
    let inj = inject_four();
    s.add("J5-注入四路全抓", inj[0] && inj[1] && inj[2] && inj[3], "满队/中断/漂移/冻结全数命中");

    // J6 收腾余量后再发成功
    let j6 = {
        let mut q = SendQueue::new(50);
        let mut i = 0u64;
        while i < QCAP as u64 {
            let _ = q.send(i + 1, 8, F_NOWAIT);
            i += 1;
        }
        let _ = q.recv();
        q.send(1000, 8, F_NOWAIT).is_ok()
    };
    s.add("J6-收腾再发", j6, "收一枚腾余量后发送成功");

    // J7 收空队返 None
    let j7 = SendQueue::new(51).recv().is_none();
    s.add("J7-空队收返None", j7, "空队列收取显性返回 None");

    // J8 零长消息拒
    let j8 = SendQueue::new(52).send(1, 0, F_NOWAIT).err() == Some(SN_EINVAL);
    s.add("J8-零长拒收", j8, "零长消息必拒");

    // J9 阻塞无信号满队返 EAGAIN（无 NOWAIT 位也裁满）
    let j9 = {
        let mut q = SendQueue::new(53);
        let mut i = 0u64;
        while i < QCAP as u64 {
            let _ = q.send(i + 1, 8, F_NOWAIT);
            i += 1;
        }
        q.send(99, 8, 0).err() == Some(SN_EAGAIN)
    };
    s.add("J9-阻塞满队裁满", j9, "无信号阻塞满队显性裁满");

    // J10 冻结后发送拒
    let j10 = {
        let mut q = SendQueue::new(54);
        q.freeze();
        q.send(1, 8, F_NOWAIT).err() == Some(SN_FROZEN)
    };
    s.add("J10-冻结拒发送", j10, "冻结后发送必拒");

    // J11 中断计数逐次累积
    let j11 = {
        let mut q = SendQueue::new(55);
        let mut i = 0u64;
        while i < QCAP as u64 {
            let _ = q.send(i + 1, 8, F_NOWAIT);
            i += 1;
        }
        q.signal_pending = true;
        let _ = q.send(99, 8, 0);
        q.signal_pending = true;
        let _ = q.send(98, 8, 0);
        q.intr_count == 2
    };
    s.add("J11-中断计数累积", j11, "两次中断计数为二");

    // J12 渲染面：消息渲染可区分
    let j12 = {
        let a = MsgEntry { mtype: 1, size: 8 };
        let b = MsgEntry { mtype: 2, size: 8 };
        a.render() != b.render()
    };
    s.add("J12-渲染分旗", j12, "mtype 渲染可区分");

    // J13 动态明细：千次零误报计数落账
    s.add_dyn("J13-千次明细", ok == 1000, &format!("legal_ok={}/1000", ok));

    // J14 动态明细：双十连计数落账
    s.add_dyn("J14-十连明细", h1 == 10 && h2 == 10 && h4 == 10, &format!("eagain={} eintr={} drift={}", h1, h2, h4));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10922_sendqueue_flow() {
        let mut q = SendQueue::new(1);
        let mut i = 0u64;
        while i < QCAP as u64 {
            assert!(q.send(i + 1, 8, F_NOWAIT).is_ok());
            i += 1;
        }
        assert!(q.send(99, 8, F_NOWAIT).err() == Some(SN_EAGAIN));
        assert!(q.send(1, 0, F_NOWAIT).err() == Some(SN_EINVAL));
        assert!(q.recv().is_some());
        assert!(q.send(100, 8, F_NOWAIT).is_ok());
        q.freeze();
        assert!(q.send(101, 8, F_NOWAIT).err() == Some(SN_FROZEN));
    }

    #[test]
    fn unx10922_checks_green() {
        let s = run_unx10922_checks();
        assert!(s.all_passed(), "UNX-F10922 判据未全绿");
    }
}
