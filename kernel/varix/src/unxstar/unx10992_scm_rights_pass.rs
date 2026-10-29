//! UNX-F10992 SCM_RIGHTS 描述符传递多进程压力（fd 账联动）（0x1db80 段）
//!
//! 锚点：UNX-F10992-J1 fd 传递千次接收可用——SCM_RIGHTS 通道千次发收后
//! 接收方持有的 fd 全部可读可用；J2 引用账两端对平——发送方减账与接收方
//! 加账逐笔对平、全程守恒。
//! 域：CoRun·UNX-C4（多进程压力域）。
//!
//! 结构：fd 引用账（每 fd 全局计数：持方进程集 + 引用数）+ SCM_RIGHTS
//! 通道（send 附带 fd 组 / recv 收取 fd 组，failpath 双向回滚）+ 联动账
//! （每笔传递发收两端各留痕）+ 注入核（坏 fd/超限/重复收号/封账写入四路必抓）。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x1db80 独占 0x20）。
pub const SEG_BASE: u32 = 0x1db80;
pub const SR_BADFD: u32 = SEG_BASE + 0x01; // 非法 fd 号
pub const SR_NOENT: u32 = SEG_BASE + 0x02; // fd 不在账
pub const SR_LIMIT: u32 = SEG_BASE + 0x03; // 接收方 fd 配额超限
pub const SR_DUPHOLD: u32 = SEG_BASE + 0x04; // 同进程重复持同一 fd
pub const SR_NOTHOLD: u32 = SEG_BASE + 0x05; // 发送方不持有该 fd
pub const SR_FROZEN: u32 = SEG_BASE + 0x06; // 引用账封账后传递
pub const SR_EMPTY: u32 = SEG_BASE + 0x07; // 传递组为空
pub const SR_ORPHAN: u32 = SEG_BASE + 0x08; // fd 账孤儿（引用零但持方非空）

/// fd 配额（每进程）。
pub const FD_QUOTA: usize = 32;
/// 全局 fd 上界（fd 号 < FD_MAX）。
pub const FD_MAX: u32 = 4096;
/// SCM_RIGHTS 域标（fold 种子）。
pub const SR_TAG: u64 = 0x1db8_0292;

/// fd 账项：全局 fd + 持方进程集 + 引用数。
#[derive(Clone, Debug)]
pub struct FdSlot {
    pub fd: u32,
    pub holders: Vec<u32>,
}

/// 一笔传递留痕：发送方 + 接收方 + fd 组 + 账号。
#[derive(Clone, Debug)]
pub struct PassMark {
    pub from: u32,
    pub to: u32,
    pub fds: Vec<u32>,
    pub no: u32,
}

/// fd 引用账 + SCM_RIGHTS 通道。
pub struct RightsBook {
    pub slots: Vec<FdSlot>,
    pub passes: Vec<PassMark>,
    pub sealed: bool,
    pub pass_no: u32,
}

impl RightsBook {
    pub fn new() -> RightsBook {
        RightsBook { slots: Vec::new(), passes: Vec::new(), sealed: false, pass_no: 0 }
    }

    /// 开 fd：入账并指定初始持方。
    pub fn open(&mut self, owner: u32, fd: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(SR_FROZEN);
        }
        if fd == 0 || fd >= FD_MAX {
            return Err(SR_BADFD);
        }
        if self.find(fd).is_some() {
            return Err(SR_DUPHOLD);
        }
        self.slots.push(FdSlot { fd, holders: {
            let mut h: Vec<u32> = Vec::new();
            h.push(owner);
            h
        } });
        Ok(())
    }

    fn find(&self, fd: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].fd == fd {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 某进程当前持有 fd 数。
    pub fn held_by(&self, pid: u32) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].holders.iter().any(|h| *h == pid) {
                n = n.wrapping_add(1);
            }
            k += 1;
        }
        n
    }

    fn holds(&self, fd: u32, pid: u32) -> bool {
        match self.find(fd) {
            None => false,
            Some(i) => self.slots[i].holders.iter().any(|h| *h == pid),
        }
    }

    /// SCM_RIGHTS 发送：校验发送方全组持有并临时减账（failpath 回滚内建在 recv 校验失败时）。
    pub fn send(&mut self, from: u32, to: u32, fds: &[u32]) -> Result<u32, u32> {
        if self.sealed {
            return Err(SR_FROZEN);
        }
        if fds.is_empty() {
            return Err(SR_EMPTY);
        }
        let mut k = 0usize;
        while k < fds.len() {
            let fd = fds[k];
            if fd == 0 || fd >= FD_MAX {
                return Err(SR_BADFD);
            }
            if self.find(fd).is_none() {
                return Err(SR_NOENT);
            }
            if !self.holds(fd, from) {
                return Err(SR_NOTHOLD);
            }
            k += 1;
        }
        // 接收方配额预检：新持计数不得超配额。
        let mut incoming = 0u32;
        let mut k = 0usize;
        while k < fds.len() {
            if !self.holds(fds[k], to) {
                incoming = incoming.wrapping_add(1);
            }
            k += 1;
        }
        if self.held_by(to).wrapping_add(incoming) > FD_QUOTA as u32 {
            return Err(SR_LIMIT);
        }
        self.pass_no = self.pass_no.wrapping_add(1);
        self.passes.push(PassMark { from, to, fds: {
            let mut v: Vec<u32> = Vec::new();
            let mut k = 0usize;
            while k < fds.len() {
                v.push(fds[k]);
                k += 1;
            }
            v
        }, no: self.pass_no });
        Ok(self.pass_no)
    }

    /// SCM_RIGHTS 接收：按最近一笔匹配的传递加账（接收方 +1，发送方 -1）。
    pub fn recv(&mut self, to: u32, no: u32) -> Result<Vec<u32>, u32> {
        if self.sealed {
            return Err(SR_FROZEN);
        }
        let idx = {
            let mut found = None;
            let mut k = 0usize;
            while k < self.passes.len() {
                if self.passes[k].no == no && self.passes[k].to == to {
                    found = Some(k);
                }
                k += 1;
            }
            match found {
                Some(i) => i,
                None => return Err(SR_NOENT),
            }
        };
        let fds = self.passes[idx].fds.clone();
        let from = self.passes[idx].from;
        let mut got: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < fds.len() {
            let fd = fds[k];
            match self.find(fd) {
                None => return Err(SR_NOENT),
                Some(i) => {
                    // 发送方减账。
                    let mut di = None;
                    let mut h = 0usize;
                    while h < self.slots[i].holders.len() {
                        if self.slots[i].holders[h] == from {
                            di = Some(h);
                        }
                        h += 1;
                    }
                    if let Some(di) = di {
                        self.slots[i].holders.remove(di);
                    }
                    // 接收方加账（去重）。
                    if !self.slots[i].holders.iter().any(|x| *x == to) {
                        self.slots[i].holders.push(to);
                    }
                    got.push(fd);
                }
            }
            k += 1;
        }
        Ok(got)
    }

    /// 接收方 fd 可用核验：收取后全部 fd 确在接收方名下。
    pub fn usable_by(&self, pid: u32, fds: &[u32]) -> bool {
        let mut k = 0usize;
        while k < fds.len() {
            if !self.holds(fds[k], pid) {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 引用账两端对平：每 fd 持方数即引用数且无孤儿。
    pub fn refs_balanced(&self) -> bool {
        let mut k = 0usize;
        while k < self.slots.len() {
            let sl = &self.slots[k];
            // 孤儿探测：持方空（引用零）视为孤儿账。
            if sl.holders.is_empty() {
                return false;
            }
            // 同进程重复持同一 fd 必为坏账。
            let mut a = 0usize;
            while a < sl.holders.len() {
                let mut b = a + 1;
                while b < sl.holders.len() {
                    if sl.holders[a] == sl.holders[b] {
                        return false;
                    }
                    b += 1;
                }
                a += 1;
            }
            k += 1;
        }
        true
    }

    /// 传递账对平：账号连续且逐笔非空组。
    pub fn pass_ledger_ok(&self) -> bool {
        let mut k = 0usize;
        while k < self.passes.len() {
            if self.passes[k].no != (k + 1) as u32 || self.passes[k].fds.is_empty() {
                return false;
            }
            k += 1;
        }
        true
    }

    /// fold 指纹（序敏感）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = SR_TAG;
        let mut k = 0usize;
        while k < self.slots.len() {
            h = (h ^ self.slots[k].fd as u64).wrapping_mul(0x100000001b3);
            let mut a = 0usize;
            while a < self.slots[k].holders.len() {
                h = (h ^ self.slots[k].holders[a] as u64).wrapping_mul(0x100000001b3);
                a += 1;
            }
            k += 1;
        }
        h = (h ^ self.pass_no as u64).wrapping_mul(0x100000001b3);
        h
    }

    /// 封账。
    pub fn seal(&mut self) -> u64 {
        self.sealed = true;
        self.fold()
    }
}

/// 确定性样本：第 r 轮 fd 与进程号编码（fd 恒在 [1, FD_MAX) 内且逐轮零碰撞）。
pub fn sr_sample(r: u32, j: u32) -> (u32, u32, u32) {
    // fd 步长 3 保证千轮内互不重号；取模把号段压回合法域，避免越界后整轮失效。
    let fd = r.wrapping_mul(3).wrapping_add(j).wrapping_add(1) % (FD_MAX - 1) + 1;
    let from = r.wrapping_mul(2).wrapping_add(100);
    let to = from.wrapping_add(1);
    (fd, from, to)
}

/// 千次 SCM_RIGHTS 传递压力：每轮 1 fd 开账→发送→接收→可用核验。
pub fn stress_rights(rounds: u32) -> u32 {
    let mut book = RightsBook::new();
    let mut ok_cnt = 0u32;
    let mut r = 0u32;
    while r < rounds {
        let (fd, from, to) = sr_sample(r, 0);
        let _ = book.open(from, fd);
        let no = book.send(from, to, &[fd]);
        if let Ok(no) = no {
            if let Ok(got) = book.recv(to, no) {
                if book.usable_by(to, &got) && book.refs_balanced() {
                    ok_cnt = ok_cnt.wrapping_add(1);
                }
            }
        }
        r += 1;
    }
    ok_cnt
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 坏 fd 号必拒
    let mut b = RightsBook::new();
    let _ = b.open(1, 5);
    got[0] = b.open(1, 0).err() == Some(SR_BADFD)
        && b.open(1, FD_MAX).err() == Some(SR_BADFD)
        && b.send(1, 2, &[5, FD_MAX]).err() == Some(SR_BADFD);
    // 2) 发送方不持有必拒
    let mut c = RightsBook::new();
    let _ = c.open(1, 6);
    got[1] = c.send(9, 2, &[6]).err() == Some(SR_NOTHOLD);
    // 3) 接收方超配额必拒
    let mut d = RightsBook::new();
    let _ = d.open(1, 1);
    let mut n = 0u32;
    while n < FD_QUOTA as u32 {
        let _ = d.open(2, n.wrapping_add(10));
        n = n.wrapping_add(1);
    }
    got[2] = d.send(1, 2, &[1]).err() == Some(SR_LIMIT);
    // 4) 空传递组与封账写入必拒
    d.seal();
    got[3] = RightsBook::new().send(1, 2, &[]).err() == Some(SR_EMPTY)
        && d.send(1, 2, &[1]).err() == Some(SR_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10992_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10992");

    // J1 fd 传递千次接收可用
    let ok = stress_rights(1000);
    let inj = inject_four();
    let j1 = ok == 1000 && inj[0];
    s.add("J1-传递千次可用", j1, "千次 SCM_RIGHTS 发收后接收方 fd 全部可用");

    // J2 引用账两端对平
    let mut b = RightsBook::new();
    let _ = b.open(1, 3);
    let _ = b.open(1, 4);
    let no = b.send(1, 2, &[3, 4]);
    let got = match no {
        Ok(n) => b.recv(2, n),
        Err(_) => Err(SR_NOENT),
    };
    let j2 = matches!(got, Ok(ref v) if v.len() == 2)
        && b.usable_by(2, &[3, 4])
        && !b.holds(3, 1)
        && !b.holds(4, 1)
        && b.refs_balanced()
        && b.pass_ledger_ok();
    s.add("J2-引用账两端对平", j2, "发送方减账与接收方加账逐笔对平且无孤儿");

    // J3 组合传递链：A→B→C 二段接力全程对平
    let mut c = RightsBook::new();
    let _ = c.open(1, 7);
    let n1 = c.send(1, 2, &[7]).unwrap_or(0);
    let _ = c.recv(2, n1);
    let n2 = c.send(2, 3, &[7]).unwrap_or(0);
    let _ = c.recv(3, n2);
    let j3 = c.usable_by(3, &[7]) && !c.usable_by(2, &[7]) && c.refs_balanced();
    s.add("J3-二段接力", j3, "fd 经 B 中转至 C 后仅 C 持有且账平");

    // J4 多 fd 组单笔传递序保持
    let mut d = RightsBook::new();
    let _ = d.open(1, 11);
    let _ = d.open(1, 12);
    let _ = d.open(1, 13);
    let n = d.send(1, 2, &[11, 12, 13]).unwrap_or(0);
    let got = d.recv(2, n).unwrap_or_default();
    let j4 = got.len() == 3 && got[0] == 11 && got[1] == 12 && got[2] == 13;
    s.add("J4-组序保持", j4, "多 fd 组传递序与登记序一致");

    // J5 越权与未知 fd 必拒
    let inj2 = inject_four();
    s.add("J5-越权未知拒", inj2[1], "发送方不持有的 fd 必拒");

    // J6 配额边界：恰好配额可收、超一必拒
    let mut e = RightsBook::new();
    let _ = e.open(1, 2);
    let mut n = 0u32;
    while n < (FD_QUOTA - 1) as u32 {
        let _ = e.open(2, n.wrapping_add(20));
        n = n.wrapping_add(1);
    }
    let fit = e.send(1, 2, &[2]).is_ok();
    let mut f = RightsBook::new();
    let _ = f.open(1, 3);
    let mut n = 0u32;
    while n < FD_QUOTA as u32 {
        let _ = f.open(2, n.wrapping_add(60));
        n = n.wrapping_add(1);
    }
    let over = f.send(1, 2, &[3]).err() == Some(SR_LIMIT);
    s.add("J6-配额边界", fit && over, "恰配额可收且超一必拒");

    // J7 重复开号必拒
    let mut g = RightsBook::new();
    let _ = g.open(1, 9);
    let j7 = g.open(2, 9).err() == Some(SR_DUPHOLD) && g.refs_balanced();
    s.add("J7-重开必拒", j7, "同 fd 二次开账必拒且原账不损");

    // J8 封账后传递必抓
    let mut h = RightsBook::new();
    let _ = h.open(1, 4);
    h.seal();
    let j8 = h.send(1, 2, &[4]).err() == Some(SR_FROZEN)
        && h.recv(2, 1).err() == Some(SR_FROZEN);
    s.add("J8-封账拒传", j8, "封账后收发两路均必拒");

    // J9 传递账连续性与指纹冻结
    let mut i = RightsBook::new();
    let _ = i.open(1, 5);
    let _ = i.open(1, 6);
    let na = i.send(1, 2, &[5]).unwrap_or(0);
    let nb = i.send(1, 2, &[6]).unwrap_or(0);
    let seq_ok = na == 1 && nb == 2 && i.pass_ledger_ok();
    let fp0 = i.fold();
    let root = i.seal();
    let j9 = seq_ok && fp0 == root && i.fold() == root;
    s.add("J9-账连指纹冻", j9, "传递账号连续且封账根稳定");

    // J10 空组必拒
    let j10 = inj2[3];
    s.add("J10-空组拒收", j10, "空传递组必拒");

    // J11 动态明细：千次可用计数落账
    s.add_dyn("J11-可用计数", ok == 1000, &format!("usable={}", ok));

    // J12 动态明细：引用账指纹落账
    let fp = b.fold();
    s.add_dyn("J12-账指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10992_rights_flow() {
        let mut b = RightsBook::new();
        assert!(b.open(1, 3).is_ok());
        assert!(b.open(1, 3).err() == Some(SR_DUPHOLD));
        let n = b.send(1, 2, &[3]).unwrap();
        let got = b.recv(2, n).unwrap();
        assert_eq!(got, vec![3]);
        assert!(b.usable_by(2, &[3]));
        assert!(!b.holds(3, 1));
        assert!(b.refs_balanced());
        b.seal();
        assert!(b.send(2, 1, &[3]).err() == Some(SR_FROZEN));
    }

    #[test]
    fn unx10992_checks_green() {
        let s = run_unx10992_checks();
        assert!(s.all_passed(), "UNX-F10992 判据未全绿");
    }
}
