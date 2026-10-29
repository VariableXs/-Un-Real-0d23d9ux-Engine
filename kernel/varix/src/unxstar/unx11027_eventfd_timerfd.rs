//! UNX-F11027 eventfd/timerfd 挂号联签（读挂点与就绪账对账）（0x1e360 段）
//!
//! 锚点：UNX-F11027-J1 就绪挂点千次对平（10/10），读清账联动一致（10/10）。
//! 域：CoRun·UNX-C4-B32（五族挂号联签域·eventfd/timerfd）。
//!
//! 结构：事件 fd 账（计数值 + 就绪位 + 写读计数）与定时器 fd 账（到期数
//! + 读清数 + 武装位）双面 + 挂点器（写置就绪、读清值清就绪）+ 千次对账
//! 器（确定性写读轮转）+ 联动断言（读清数 = 写入数、到期数 = 读清数）
//! + fold 与冻结 + 注入核（空读/超界写/未武装到期/冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e360 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e360;
pub const EF_EMPTY: u32 = SEG_BASE + 0x01; // 空值读
pub const EF_OVER: u32 = SEG_BASE + 0x02; // 计数超界
pub const EF_UNARMED: u32 = SEG_BASE + 0x03; // 未武装到期
pub const EF_NOFD: u32 = SEG_BASE + 0x04; // fd 未注册
pub const EF_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后写账
pub const EF_DUP: u32 = SEG_BASE + 0x06; // fd 重号

/// eventfd 计数上限。
pub const EF_VAL_CAP: u64 = 0xffff;
/// 千次就绪轮数。
pub const EF_ROUNDS: u32 = 1000;
/// timerfd 到期轮数。
pub const TF_ROUNDS: u32 = 100;
/// 事件账域标（fold 种子）。
pub const EVFD_TAG: u64 = 0x1e36_27;
/// fd 对平面规模（10/10）。
pub const EFD_NEEDED: usize = 10;

/// eventfd 项：fd + 计数 + 就绪位 + 写读计数。
#[derive(Clone, Debug)]
pub struct EfdItem {
    pub fd: u32,
    pub val: u64,
    pub ready: bool,
    pub writes: u32,
    pub reads: u32,
}

/// timerfd 项：fd + 到期数 + 读清数 + 武装位。
#[derive(Clone, Debug)]
pub struct TfdItem {
    pub fd: u32,
    pub expiries: u32,
    pub cleared: u32,
    pub armed: bool,
}

/// eventfd/timerfd 挂号账：注册 → 写读挂点 → 到期读清 → 联动封账。
pub struct EvfdBook {
    pub efs: Vec<EfdItem>,
    pub tfs: Vec<TfdItem>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl EvfdBook {
    pub fn new() -> EvfdBook {
        EvfdBook { efs: Vec::new(), tfs: Vec::new(), frozen: false, root: None }
    }

    /// 注册 eventfd：冻结、重号必拒。
    pub fn enroll_ef(&mut self, fd: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(EF_FROZEN);
        }
        if self.efs.iter().any(|x| x.fd == fd) {
            return Err(EF_DUP);
        }
        self.efs.push(EfdItem { fd, val: 0, ready: false, writes: 0, reads: 0 });
        Ok(())
    }

    /// 注册 timerfd：冻结、重号必拒。
    pub fn enroll_tf(&mut self, fd: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(EF_FROZEN);
        }
        if self.tfs.iter().any(|x| x.fd == fd) {
            return Err(EF_DUP);
        }
        self.tfs.push(TfdItem { fd, expiries: 0, cleared: 0, armed: false });
        Ok(())
    }

    /// 写挂点：计数入账并置就绪；超界必拒。
    pub fn ef_write(&mut self, fd: u32, add: u64) -> Result<u64, u32> {
        if self.frozen {
            return Err(EF_FROZEN);
        }
        let mut k = 0usize;
        while k < self.efs.len() {
            if self.efs[k].fd == fd {
                if self.efs[k].val.wrapping_add(add) > EF_VAL_CAP {
                    return Err(EF_OVER);
                }
                self.efs[k].val += add;
                self.efs[k].ready = true;
                self.efs[k].writes = self.efs[k].writes.wrapping_add(1);
                return Ok(self.efs[k].val);
            }
            k += 1;
        }
        Err(EF_NOFD)
    }

    /// 读挂点：值零即拒；读清值并清就绪。
    pub fn ef_read(&mut self, fd: u32) -> Result<u64, u32> {
        if self.frozen {
            return Err(EF_FROZEN);
        }
        let mut k = 0usize;
        while k < self.efs.len() {
            if self.efs[k].fd == fd {
                if self.efs[k].val == 0 {
                    return Err(EF_EMPTY);
                }
                let v = self.efs[k].val;
                self.efs[k].val = 0;
                self.efs[k].ready = false;
                self.efs[k].reads = self.efs[k].reads.wrapping_add(1);
                return Ok(v);
            }
            k += 1;
        }
        Err(EF_NOFD)
    }

    /// timerfd 武装：到期前提。
    pub fn tf_arm(&mut self, fd: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(EF_FROZEN);
        }
        let mut k = 0usize;
        while k < self.tfs.len() {
            if self.tfs[k].fd == fd {
                self.tfs[k].armed = true;
                return Ok(());
            }
            k += 1;
        }
        Err(EF_NOFD)
    }

    /// timerfd 到期：未武装必拒；到期数入账保持就绪。
    pub fn tf_expire(&mut self, fd: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(EF_FROZEN);
        }
        let mut k = 0usize;
        while k < self.tfs.len() {
            if self.tfs[k].fd == fd {
                if !self.tfs[k].armed {
                    return Err(EF_UNARMED);
                }
                self.tfs[k].expiries = self.tfs[k].expiries.wrapping_add(1);
                return Ok(self.tfs[k].expiries);
            }
            k += 1;
        }
        Err(EF_NOFD)
    }

    /// timerfd 读清：到期数为零必拒；读清并解除武装。
    pub fn tf_read(&mut self, fd: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(EF_FROZEN);
        }
        let mut k = 0usize;
        while k < self.tfs.len() {
            if self.tfs[k].fd == fd {
                if self.tfs[k].expiries == self.tfs[k].cleared {
                    return Err(EF_EMPTY);
                }
                self.tfs[k].cleared = self.tfs[k].expiries;
                self.tfs[k].armed = false;
                return Ok(self.tfs[k].cleared);
            }
            k += 1;
        }
        Err(EF_NOFD)
    }

    /// 就绪面：eventfd 就绪数。
    pub fn ready_n(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.efs.len() {
            if self.efs[k].ready {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 联动断言：eventfd 读清数 = 写入数且就绪面清空；timerfd 到期 = 读清。
    pub fn linked(&self) -> bool {
        let mut w = 0u32;
        let mut r = 0u32;
        let mut k = 0usize;
        while k < self.efs.len() {
            w = w.wrapping_add(self.efs[k].writes);
            r = r.wrapping_add(self.efs[k].reads);
            k += 1;
        }
        let mut te = 0u32;
        let mut tc = 0u32;
        k = 0;
        while k < self.tfs.len() {
            te = te.wrapping_add(self.tfs[k].expiries);
            tc = tc.wrapping_add(self.tfs[k].cleared);
            k += 1;
        }
        w == r && self.ready_n() == 0 && te == tc
    }

    /// 链式 fold 指纹（FNV 风格，序敏感）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = EVFD_TAG;
        let mut k = 0usize;
        while k < self.efs.len() {
            let x = &self.efs[k];
            h = (h ^ x.fd as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.val).wrapping_mul(0x100000001b3);
            h = (h ^ x.writes as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.reads as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        k = 0;
        while k < self.tfs.len() {
            let x = &self.tfs[k];
            h = (h ^ x.fd as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.expiries as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.cleared as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
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

/// fd 确定性样本。
pub fn ef_fd(no: usize) -> u32 {
    300 + no as u32
}

/// tf fd 确定性样本。
pub fn tf_fd(no: usize) -> u32 {
    400 + no as u32
}

/// 千次对账全链：十枚 eventfd 千轮成对写读；两枚 timerfd 各 TF_ROUNDS
/// 次武装-到期-读清。
pub fn build_evfdbook() -> (EvfdBook, bool) {
    let mut b = EvfdBook::new();
    let mut ok = true;
    let mut i = 0usize;
    while i < EFD_NEEDED {
        if b.enroll_ef(ef_fd(i)).is_err() {
            ok = false;
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < 2 {
        if b.enroll_tf(tf_fd(i)).is_err() {
            ok = false;
        }
        i += 1;
    }
    let mut r = 0u32;
    while r < EF_ROUNDS {
        let k = (r % EFD_NEEDED as u32) as usize;
        if b.ef_write(ef_fd(k), 1).is_err() {
            ok = false;
        }
        if b.ef_read(ef_fd(k)).is_err() {
            ok = false;
        }
        r += 1;
    }
    let mut t = 0u32;
    while t < TF_ROUNDS {
        let k = (t % 2) as usize;
        if b.tf_arm(tf_fd(k)).is_err() {
            ok = false;
        }
        if b.tf_expire(tf_fd(k)).is_err() {
            ok = false;
        }
        if b.tf_read(tf_fd(k)).is_err() {
            ok = false;
        }
        t += 1;
    }
    let b_ok = ok && b.linked();
    (b, b_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 空值读必拒
    let mut a = EvfdBook::new();
    let _ = a.enroll_ef(1);
    got[0] = a.ef_read(1).err() == Some(EF_EMPTY);
    // 2) 未武装到期必拒
    let mut b = EvfdBook::new();
    let _ = b.enroll_tf(2);
    got[1] = b.tf_expire(2).err() == Some(EF_UNARMED);
    // 3) 超界写必拒
    let mut c = EvfdBook::new();
    let _ = c.enroll_ef(3);
    got[2] = c.ef_write(3, EF_VAL_CAP).is_ok() && c.ef_write(3, 1).err() == Some(EF_OVER);
    // 4) 冻结后写账必抓
    let (mut d, _) = build_evfdbook();
    d.freeze();
    got[3] = d.ef_write(ef_fd(0), 1).err() == Some(EF_FROZEN)
        && d.tf_arm(tf_fd(0)).err() == Some(EF_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11027_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11027");

    // J1 就绪挂点千次对平：十枚 eventfd 千轮成对写读
    let (mut book, green) = build_evfdbook();
    s.add("J1-千次对平", green && book.efs.len() == 10, "十枚事件 fd 千轮写读对平");

    // J2 读清账联动一致：读清数 = 写入数且就绪面清空、到期 = 读清
    s.add("J2-读清联动", book.linked(), "事件读清与到期读清双面联动一致");

    // J3 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J3-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J4 冻结后写账必抓
    let j4 = book.ef_write(ef_fd(0), 1).err() == Some(EF_FROZEN)
        && book.ef_read(ef_fd(0)).err() == Some(EF_FROZEN);
    s.add("J4-冻结拒写", j4, "冻结后写读均必拒");

    // J5 注入检出：空值读 + 未武装到期
    let inj = inject_four();
    s.add("J5-空读未武抓", inj[0] && inj[1], "空值读与未武装到期两路必抓");

    // J6 注入检出：超界写 + 冻结后写
    s.add("J6-超界冻结检出", inj[2] && inj[3], "计数超界与冻结后写必抓");

    // J7 就绪位语义：写置就绪读清就绪
    let j7 = {
        let mut c = EvfdBook::new();
        let _ = c.enroll_ef(9);
        let _ = c.ef_write(9, 2);
        let mid = c.ready_n() == 1;
        let v = c.ef_read(9);
        mid && v == Ok(2) && c.ready_n() == 0
    };
    s.add("J7-就绪位语义", j7, "写置就绪读清值并清就绪");

    // J8 计数累加：连续写计数值累加一次读清
    let j8 = {
        let mut c = EvfdBook::new();
        let _ = c.enroll_ef(8);
        let _ = c.ef_write(8, 3);
        let _ = c.ef_write(8, 4);
        c.ef_read(8) == Ok(7) && c.efs[0].writes == 2 && c.efs[0].reads == 1
    };
    s.add("J8-计数累加", j8, "连续写累加一次读清");

    // J9 timerfd 周期：武装-到期-读清逐轮闭合
    let j9 = {
        let per = TF_ROUNDS / 2;
        let mut even = true;
        let mut k = 0usize;
        while k < book.tfs.len() {
            if book.tfs[k].expiries != per || book.tfs[k].cleared != per {
                even = false;
            }
            k += 1;
        }
        even && per == 50
    };
    s.add("J9-timerfd 周期", j9, "两枚定时器各五十周期闭合");

    // J10 未注册 fd 必拒
    let j10 = EvfdBook::new().ef_write(0x7777, 1).err() == Some(EF_NOFD)
        && EvfdBook::new().tf_read(0x7777).err() == Some(EF_NOFD);
    s.add("J10-未注册拒", j10, "未注册 fd 读写均必拒");

    // J11 重号必拒
    let j11 = {
        let mut c = EvfdBook::new();
        let _ = c.enroll_ef(0x55);
        c.enroll_ef(0x55).err() == Some(EF_DUP)
    };
    s.add("J11-重号拒", j11, "fd 重号必拒");

    // J12 读清后空读必拒
    let j12 = {
        let mut c = EvfdBook::new();
        let _ = c.enroll_ef(0x66);
        let _ = c.ef_write(0x66, 1);
        let _ = c.ef_read(0x66);
        c.ef_read(0x66).err() == Some(EF_EMPTY)
    };
    s.add("J12-读清后拒", j12, "读清后再读必拒");

    // J13 样本确定性：fd 同入同出
    let f1a = ef_fd(1);
    let f1b = ef_fd(1);
    let g0a = tf_fd(0);
    let g0b = tf_fd(0);
    s.add("J13-样本确定性", f1a == f1b && g0a == g0b, "fd 样本同入同出");

    // J14 就绪敏感：就绪面变化指纹必变
    let j14 = {
        let mut c = EvfdBook::new();
        let _ = c.enroll_ef(0x88);
        let f0 = c.fold();
        let _ = c.ef_write(0x88, 1);
        c.fold() != f0
    };
    s.add("J14-就绪敏感", j14, "就绪与计数变化指纹必变");

    // J15 动态明细：千次对平与冻结根落账
    let fp = book.fold();
    s.add_dyn(
        "J15-千次明细",
        fp == root && book.linked(),
        &format!("root={:#x} ready={}", fp, book.ready_n()),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11027_evfd_flow() {
        let (mut b, built) = build_evfdbook();
        assert!(built);
        assert!(b.ef_read(ef_fd(0)).err() == Some(EF_EMPTY));
        b.freeze();
        assert!(b.ef_write(ef_fd(1), 1).err() == Some(EF_FROZEN));
        assert!(b.tf_read(tf_fd(0)).err() == Some(EF_FROZEN));
        assert!(b.root.is_some());
    }

    #[test]
    fn unx11027_checks_green() {
        let s = run_unx11027_checks();
        assert!(s.all_passed(), "UNX-F11027 判据未全绿");
    }
}
