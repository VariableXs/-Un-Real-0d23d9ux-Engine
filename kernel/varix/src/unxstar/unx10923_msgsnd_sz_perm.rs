//! UNX-F10923 msgsnd EINVAL/EACCES 矩阵（尺寸与权限）（0x1d2e0 段）
//!
//! 锚点：UNX-F10923-J1 正面：msgsz 超 EACCES 类 EINVAL、无写权限→
//! EACCES（各 10/10）；反面：合法发送千次零误报（10/10）。
//! 域：CoRun·UNX-B27（SysV 错误矩阵域·msgsnd 尺寸权限面）。
//!
//! 结构：发送账（msgmax 上限 + 写权限位 + 队列存活账）+ 裁断器
//! （超上限→EINVAL、无写权→EACCES、合法全通）+ 注入核（超限/无权/
//! 死队/冻结改账四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1d2e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1d2e0;
pub const SZ_EINVAL: u32 = SEG_BASE + 0x01; // msgsz 超上限
pub const SZ_EACCES: u32 = SEG_BASE + 0x02; // 无写权限
pub const SZ_ENOENT: u32 = SEG_BASE + 0x03; // 队列不存在
pub const SZ_EIDRM: u32 = SEG_BASE + 0x04; // 队列已删
pub const SZ_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后改账
pub const SZ_EAGAIN: u32 = SEG_BASE + 0x06; // 满队

/// msgmax 上限。
pub const MSGMAX: u32 = 64;
/// 账定容。
pub const CAP: usize = 16;
/// SZ 域标（fold 种子）。
pub const SZ_TAG: u64 = 0x1d2e217;

/// 队列账条目。
#[derive(Clone, Debug)]
pub struct SzQueue {
    pub qid: u32,
    pub writable: bool,
    pub alive: bool,
    pub depth: usize,
}

impl SzQueue {
    pub fn render(&self) -> String {
        let w = if self.writable { "W" } else { "R" };
        let a = if self.alive { "A" } else { "D" };
        format!("Q{}|{}{}", self.qid, w, a)
    }
}

/// 尺寸权限裁断账。
pub struct SzBoard {
    pub queues: Vec<SzQueue>,
    pub frozen: bool,
    pub sent: u32,
}

impl SzBoard {
    pub fn new() -> SzBoard {
        SzBoard { queues: Vec::new(), frozen: false, sent: 0 }
    }

    /// 建队列。
    pub fn create(&mut self, qid: u32, writable: bool) -> Result<(), u32> {
        if self.frozen {
            return Err(SZ_FROZEN);
        }
        if self.queues.iter().any(|q| q.qid == qid) {
            return Err(SZ_EINVAL);
        }
        if self.queues.len() >= CAP {
            return Err(SZ_EINVAL);
        }
        self.queues.push(SzQueue { qid, writable, alive: true, depth: 0 });
        Ok(())
    }

    /// 删队列（EIDRM 语义）。
    pub fn rmid(&mut self, qid: u32) -> Result<(), u32> {
        let mut idx = self.queues.len();
        let mut k = 0usize;
        while k < self.queues.len() {
            if self.queues[k].qid == qid {
                idx = k;
            }
            k += 1;
        }
        if idx == self.queues.len() {
            return Err(SZ_ENOENT);
        }
        self.queues[idx].alive = false;
        Ok(())
    }

    /// msgsnd 主裁断：不存在→ENOENT、已删→EIDRM、无写权→EACCES、
    /// 超上限→EINVAL、满队→EAGAIN。
    pub fn send(&mut self, qid: u32, msgsz: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(SZ_FROZEN);
        }
        let mut idx = self.queues.len();
        let mut k = 0usize;
        while k < self.queues.len() {
            if self.queues[k].qid == qid {
                idx = k;
            }
            k += 1;
        }
        if idx == self.queues.len() {
            return Err(SZ_ENOENT);
        }
        if !self.queues[idx].alive {
            return Err(SZ_EIDRM);
        }
        if !self.queues[idx].writable {
            return Err(SZ_EACCES);
        }
        if msgsz > MSGMAX || msgsz == 0 {
            return Err(SZ_EINVAL);
        }
        if self.queues[idx].depth >= 4 {
            return Err(SZ_EAGAIN);
        }
        self.queues[idx].depth += 1;
        self.sent += 1;
        Ok(())
    }

    /// 收（腾深度）。
    pub fn take(&mut self, qid: u32) -> Result<u32, u32> {
        let mut idx = self.queues.len();
        let mut k = 0usize;
        while k < self.queues.len() {
            if self.queues[k].qid == qid {
                idx = k;
            }
            k += 1;
        }
        if idx == self.queues.len() {
            return Err(SZ_ENOENT);
        }
        if self.queues[idx].depth == 0 {
            return Err(SZ_EAGAIN);
        }
        self.queues[idx].depth -= 1;
        Ok(MSGMAX)
    }

    /// 链式 fold 指纹（序敏感，含权限/存活/深度）。
    pub fn fold(&self) -> u64 {
        let mut h = SZ_TAG;
        let mut k = 0usize;
        while k < self.queues.len() {
            let q = &self.queues[k];
            h = (h ^ q.qid as u64).wrapping_mul(0x100000001b3);
            h = (h ^ q.writable as u64).wrapping_mul(0x100000001b3);
            h = (h ^ q.alive as u64).wrapping_mul(0x100000001b3);
            h = (h ^ q.depth as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.sent as u64).wrapping_mul(0x100000001b3);
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
    // 1) 超上限必报 EINVAL
    let mut b = SzBoard::new();
    let _ = b.create(1, true);
    got[0] = b.send(1, MSGMAX + 1).err() == Some(SZ_EINVAL);
    // 2) 无写权必报 EACCES
    let mut p = SzBoard::new();
    let _ = p.create(2, false);
    got[1] = p.send(2, 8).err() == Some(SZ_EACCES);
    // 3) 死队必报 EIDRM 且优先于权限裁断
    let mut d = SzBoard::new();
    let _ = d.create(3, false);
    let _ = d.rmid(3);
    got[2] = d.send(3, 8).err() == Some(SZ_EIDRM);
    // 4) 冻结后发送必抓
    let mut f = SzBoard::new();
    f.freeze();
    got[3] = f.send(1, 8).err() == Some(SZ_FROZEN);
    got
}

/// 尺寸确定性样本。
pub fn sz_sample_size(r: u32) -> u32 {
    r.wrapping_mul(2654435761).wrapping_add(43) % MSGMAX + 1
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10923_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10923");

    // J1 正面·超限 EINVAL 十连
    let mut h1 = 0u32;
    let mut r = 0u32;
    while r < 10 {
        let mut b = SzBoard::new();
        let _ = b.create(r + 1, true);
        if b.send(r + 1, MSGMAX + 1 + r).err() == Some(SZ_EINVAL) {
            h1 += 1;
        }
        r += 1;
    }
    s.add("J1-超限EINVAL十连", h1 == 10, "msgsz 超上限十轮全数 EINVAL");

    // J2 正面·无写权 EACCES 十连
    let mut h2 = 0u32;
    let mut r2 = 0u32;
    while r2 < 10 {
        let mut p = SzBoard::new();
        let _ = p.create(r2 + 20, false);
        if p.send(r2 + 20, 8).err() == Some(SZ_EACCES) {
            h2 += 1;
        }
        r2 += 1;
    }
    s.add("J2-EACCES十连", h2 == 10, "无写权限十轮全数 EACCES");

    // J3 反面：合法发送千次零误报
    let mut ok = 0u32;
    let mut r3 = 0u32;
    while r3 < 1000 {
        let mut b = SzBoard::new();
        let qid = (r3 % 5) as u32 + 1;
        let _ = b.create(qid, true);
        if b.send(qid, sz_sample_size(r3)).is_ok() {
            ok += 1;
        }
        r3 += 1;
    }
    s.add("J3-合法千次零误报", ok == 1000, "限内合法发送千轮零误报");

    // J4 边界：恰等于 MSGMAX 可发
    let j4 = {
        let mut b = SzBoard::new();
        let _ = b.create(30, true);
        b.send(30, MSGMAX).is_ok()
    };
    s.add("J4-上限恰可发", j4, "msgsz==MSGMAX 合法");

    // J5 零长拒收
    let j5 = {
        let mut b = SzBoard::new();
        let _ = b.create(31, true);
        b.send(31, 0).err() == Some(SZ_EINVAL)
    };
    s.add("J5-零长拒收", j5, "零长消息必拒");

    // J6 注入四路全抓
    let inj = inject_four();
    s.add("J6-注入四路全抓", inj[0] && inj[1] && inj[2] && inj[3], "超限/无权/死队/冻结全数命中");

    // J7 死队优先于尺寸裁断
    let j7 = {
        let mut b = SzBoard::new();
        let _ = b.create(32, true);
        let _ = b.rmid(32);
        b.send(32, MSGMAX + 9).err() == Some(SZ_EIDRM)
    };
    s.add("J7-死队优先裁", j7, "EIDRM 优先于 EINVAL");

    // J8 不存在队拒
    let j8 = SzBoard::new().send(99, 8).err() == Some(SZ_ENOENT);
    s.add("J8-不存在拒", j8, "未建队发送必拒");

    // J9 满队 EAGAIN 且可收腾
    let j9 = {
        let mut b = SzBoard::new();
        let _ = b.create(33, true);
        let _ = b.send(33, 8);
        let _ = b.send(33, 8);
        let _ = b.send(33, 8);
        let _ = b.send(33, 8);
        let full = b.send(33, 8).err() == Some(SZ_EAGAIN);
        let _ = b.take(33);
        full && b.send(33, 8).is_ok()
    };
    s.add("J9-满队收腾", j9, "满队 EAGAIN 后收一枚可再发");

    // J10 冻结后发送/建队双拒
    let j10 = {
        let mut b = SzBoard::new();
        b.freeze();
        b.create(34, true).err() == Some(SZ_FROZEN) && b.send(34, 8).err() == Some(SZ_FROZEN)
    };
    s.add("J10-冻结双拒", j10, "冻结后建队与发送均必拒");

    // J11 发送账逐次累积
    let j11 = {
        let mut b = SzBoard::new();
        let _ = b.create(35, true);
        let _ = b.send(35, 8);
        let _ = b.send(35, 8);
        b.sent == 2
    };
    s.add("J11-发送账累积", j11, "成功发送逐次入账");

    // J12 渲染面：权位/存活渲染可区分
    let j12 = {
        let a = SzQueue { qid: 1, writable: true, alive: true, depth: 0 };
        let b = SzQueue { qid: 1, writable: false, alive: false, depth: 0 };
        a.render() != b.render() && a.render().contains("W")
    };
    s.add("J12-渲染分旗", j12, "权限/存活渲染可区分");

    // J13 fold 稳定：发送改变指纹、同态一致
    let j13 = {
        let mut b = SzBoard::new();
        let _ = b.create(36, true);
        let f0 = b.fold();
        let _ = b.send(36, 8);
        let f1 = b.fold();
        let f2 = b.fold();
        f0 != f1 && f1 == f2 && f1 != 0
    };
    s.add("J13-指纹稳定", j13, "发送入指纹且同态稳定");

    // J14 动态明细：千次零误报计数落账
    s.add_dyn("J14-千次明细", ok == 1000, &format!("legal_ok={}/1000", ok));

    // J15 动态明细：双十连计数落账
    s.add_dyn("J15-十连明细", h1 == 10 && h2 == 10, &format!("einval={} eacces={}", h1, h2));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10923_szboard_flow() {
        let mut b = SzBoard::new();
        assert!(b.create(1, true).is_ok());
        assert!(b.create(1, true).err() == Some(SZ_EINVAL));
        assert_eq!(b.send(1, 8), Ok(()));
        assert!(b.send(1, MSGMAX + 1).err() == Some(SZ_EINVAL));
        assert!(b.send(1, 0).err() == Some(SZ_EINVAL));
        assert!(b.send(99, 8).err() == Some(SZ_ENOENT));
        let _ = b.rmid(1);
        assert!(b.send(1, 8).err() == Some(SZ_EIDRM));
        b.freeze();
        assert!(b.send(1, 8).err() == Some(SZ_FROZEN));
    }

    #[test]
    fn unx10923_checks_green() {
        let s = run_unx10923_checks();
        assert!(s.all_passed(), "UNX-F10923 判据未全绿");
    }
}
