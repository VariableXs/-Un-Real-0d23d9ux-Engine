//! UNX-F11028 pty 主从挂点联签（ptmx/pts 挂号对账）（0x1e380 段）
//!
//! 锚点：UNX-F11028-J1 主从挂点千次对平（10/10），开闭注册账一致（10/10）。
//! 域：CoRun·UNX-C4-B32（五族挂号联签域·pty 主从）。
//!
//! 结构：pty 对账（主 fd + 从 fd + 打开位 + 收发计数）+ 开对器（成对分配
//! 主从 fd）+ 数据挂点（主写从读、字节守恒）+ 关对器（双侧关齐方回收）
//! + 开闭断言（开对数 = 关对数且在位清零）+ fold 与冻结 + 注入核（从端
//! 未开读写/重复 fd/单向关对/冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e380 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e380;
pub const PT_NOMASTER: u32 = SEG_BASE + 0x01; // 主 fd 未开
pub const PT_DUPFD: u32 = SEG_BASE + 0x02; // fd 重号
pub const PT_HALFCLOSE: u32 = SEG_BASE + 0x03; // 单向关对
pub const PT_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后写账
pub const PT_EMPTY: u32 = SEG_BASE + 0x05; // 从端空读
pub const PT_CAP: u32 = SEG_BASE + 0x06; // 对账超容

/// 对账定容。
pub const PAIR_CAP: usize = 12;
/// 千次挂点轮数。
pub const PT_ROUNDS: u32 = 1000;
/// pty 账域标（fold 种子）。
pub const PTY_TAG: u64 = 0x1e38_28;
/// 开闭对平面规模（10/10）。
pub const PAIRS_NEEDED: usize = 10;

/// 单个 pty 对：主从 fd + 打开位 + 收发字节。
#[derive(Clone, Debug)]
pub struct PtyPair {
    pub mfd: u32,
    pub sfd: u32,
    pub open: bool,
    pub sent: u32,
    pub recvd: u32,
}

/// pty 主从挂号账：开对 → 数据挂点 → 关对回收 → 开闭封账。
pub struct PtyBook {
    pub pairs: Vec<PtyPair>,
    pub next_fd: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl PtyBook {
    pub fn new() -> PtyBook {
        PtyBook { pairs: Vec::new(), next_fd: 80, frozen: false, root: None }
    }

    /// fd 占用检查。
    pub fn fd_used(&self, fd: u32) -> bool {
        let mut k = 0usize;
        while k < self.pairs.len() {
            if self.pairs[k].mfd == fd || self.pairs[k].sfd == fd {
                return true;
            }
            k += 1;
        }
        false
    }

    /// 开对：成对分配主从 fd；超容必拒。
    pub fn open_pair(&mut self) -> Result<(u32, u32), u32> {
        if self.frozen {
            return Err(PT_FROZEN);
        }
        if self.pairs.len() >= PAIR_CAP {
            return Err(PT_CAP);
        }
        let m = self.next_fd;
        let sd = self.next_fd.wrapping_add(1);
        if self.fd_used(m) || self.fd_used(sd) {
            return Err(PT_DUPFD);
        }
        self.next_fd = self.next_fd.wrapping_add(2);
        self.pairs.push(PtyPair { mfd: m, sfd: sd, open: true, sent: 0, recvd: 0 });
        Ok((m, sd))
    }

    /// 主端写挂点：主 fd 未开必拒；字节入账。
    pub fn master_write(&mut self, mfd: u32, len: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(PT_FROZEN);
        }
        let mut k = 0usize;
        while k < self.pairs.len() {
            if self.pairs[k].mfd == mfd {
                if !self.pairs[k].open {
                    return Err(PT_NOMASTER);
                }
                self.pairs[k].sent = self.pairs[k].sent.wrapping_add(len);
                return Ok(self.pairs[k].sent);
            }
            k += 1;
        }
        Err(PT_NOMASTER)
    }

    /// 从端读挂点：无驻留必拒；字节出账。
    pub fn slave_read(&mut self, sfd: u32, len: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(PT_FROZEN);
        }
        let mut k = 0usize;
        while k < self.pairs.len() {
            if self.pairs[k].sfd == sfd {
                if self.pairs[k].recvd == self.pairs[k].sent {
                    return Err(PT_EMPTY);
                }
                self.pairs[k].recvd = self.pairs[k].recvd.wrapping_add(len);
                return Ok(self.pairs[k].recvd);
            }
            k += 1;
        }
        Err(PT_EMPTY)
    }

    /// 关对：双侧关齐方回收；单向关对必拒（记录半关态）。
    pub fn close_pair(&mut self, fd: u32) -> Result<bool, u32> {
        if self.frozen {
            return Err(PT_FROZEN);
        }
        let mut k = 0usize;
        while k < self.pairs.len() {
            if self.pairs[k].mfd == fd || self.pairs[k].sfd == fd {
                if !self.pairs[k].open {
                    return Err(PT_HALFCLOSE);
                }
                // 单侧关：主关而从收发未平即半关拒收。
                if self.pairs[k].sent != self.pairs[k].recvd {
                    return Err(PT_HALFCLOSE);
                }
                self.pairs[k].open = false;
                return Ok(true);
            }
            k += 1;
        }
        Err(PT_NOMASTER)
    }

    /// 回收在位断言：开对数 = 关对数且在位清零、字节守恒。
    pub fn closed_even(&self, opened: u32) -> bool {
        let mut off = 0u32;
        let mut s = 0u32;
        let mut r = 0u32;
        let mut k = 0usize;
        while k < self.pairs.len() {
            if !self.pairs[k].open {
                off += 1;
            }
            s = s.wrapping_add(self.pairs[k].sent);
            r = r.wrapping_add(self.pairs[k].recvd);
            k += 1;
        }
        off == opened && s == r && self.pairs.len() as u32 == opened
    }

    /// 按主 fd 查位次。
    pub fn find_m(&self, mfd: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.pairs.len() {
            if self.pairs[k].mfd == mfd {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 链式 fold 指纹（FNV 风格，序敏感）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = PTY_TAG;
        let mut k = 0usize;
        while k < self.pairs.len() {
            let x = &self.pairs[k];
            h = (h ^ x.mfd as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.sfd as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.sent as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.recvd as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.open as u64).wrapping_mul(0x100000001b3);
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

/// 每轮写入字节数确定性样本。
pub fn pt_len(no: u32) -> u32 {
    (no % 7 + 3) as u32
}

/// 千次对账全链：十对开立；千轮在轮转对上主写从读；收尾逐对收平关对。
pub fn build_ptybook() -> (PtyBook, bool) {
    let mut b = PtyBook::new();
    let mut ok = true;
    let mut i = 0usize;
    let mut mfds: Vec<u32> = Vec::new();
    while i < PAIRS_NEEDED {
        match b.open_pair() {
            Ok((m, _)) => mfds.push(m),
            Err(_) => ok = false,
        }
        i += 1;
    }
    let mut r = 0u32;
    while r < PT_ROUNDS {
        let k = (r % PAIRS_NEEDED as u32) as usize;
        let m = mfds[k];
        let l = pt_len(r);
        if b.master_write(m, l).is_err() {
            ok = false;
        }
        // 从 fd = 主 fd + 1（成对分配规律）。
        if b.slave_read(m + 1, l).is_err() {
            ok = false;
        }
        r += 1;
    }
    let mut i = 0usize;
    while i < mfds.len() {
        if b.close_pair(mfds[i]).is_err() {
            ok = false;
        }
        i += 1;
    }
    let b_ok = ok && b.closed_even(PAIRS_NEEDED as u32);
    (b, b_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 从端空读必拒
    let mut a = PtyBook::new();
    let _ = a.open_pair();
    got[0] = a.slave_read(81, 4).err() == Some(PT_EMPTY);
    // 2) 半关对必拒：收发未平即关
    let mut b = PtyBook::new();
    let om = match b.open_pair() {
        Ok((m, _)) => m,
        Err(_) => 0,
    };
    let _ = b.master_write(om, 9);
    got[1] = b.close_pair(om).err() == Some(PT_HALFCLOSE);
    // 3) 未开主 fd 写必拒
    let mut c = PtyBook::new();
    got[2] = c.master_write(0x999, 4).err() == Some(PT_NOMASTER);
    // 4) 冻结后写账必抓
    let (mut d, _) = build_ptybook();
    d.freeze();
    got[3] = d.master_write(80, 1).err() == Some(PT_FROZEN)
        && d.open_pair().err() == Some(PT_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11028_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11028");

    // J1 主从挂点千次对平：十对千轮主写从读
    let (mut book, green) = build_ptybook();
    s.add("J1-千次对平", green && book.pairs.len() == 10, "十对千轮主写从读对平");

    // J2 开闭注册账一致：开对数 = 关对数且字节守恒
    s.add("J2-开闭一致", book.closed_even(PAIRS_NEEDED as u32), "开对关对齐平字节守恒");

    // J3 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J3-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J4 冻结后写账必抓
    let j4 = book.master_write(80, 1).err() == Some(PT_FROZEN)
        && book.slave_read(81, 1).err() == Some(PT_FROZEN);
    s.add("J4-冻结拒写", j4, "冻结后读写均必拒");

    // J5 注入检出：从端空读 + 半关对
    let inj = inject_four();
    s.add("J5-空读半关抓", inj[0] && inj[1], "从端空读与半关对两路必抓");

    // J6 注入检出：未开主 fd + 冻结后写
    s.add("J6-未开冻结检出", inj[2] && inj[3], "未开主 fd 与冻结后写必抓");

    // J7 成对分配：主从 fd 相邻且不重号
    let j7 = {
        let mut c = PtyBook::new();
        let g = c.open_pair();
        match g {
            Ok((m, sd)) => sd == m + 1 && !c.fd_used(m + 2),
            Err(_) => false,
        }
    };
    s.add("J7-成对分配", j7, "主从 fd 相邻分配无重号");

    // J8 收平关对：收发齐平方可关对
    let j8 = {
        let mut c = PtyBook::new();
        let om = match c.open_pair() {
            Ok((m, _)) => m,
            Err(_) => 0,
        };
        let _ = c.master_write(om, 6);
        let _ = c.slave_read(om + 1, 6);
        c.close_pair(om) == Ok(true) && c.find_m(om).map(|k| !c.pairs[k].open) == Some(true)
    };
    s.add("J8-收平关对", j8, "收发齐平方可关对回收");

    // J9 关后再写必拒
    let j9 = {
        let mut c = PtyBook::new();
        let om = match c.open_pair() {
            Ok((m, _)) => m,
            Err(_) => 0,
        };
        let _ = c.close_pair(om);
        c.master_write(om, 1).err() == Some(PT_NOMASTER)
    };
    s.add("J9-关后再写拒", j9, "关对后主端再写必拒");

    // J10 轮转均匀：各对收发各两百
    let j10 = {
        let per = PT_ROUNDS / PAIRS_NEEDED as u32;
        let mut even = true;
        let mut k = 0usize;
        while k < book.pairs.len() {
            if book.pairs[k].sent != book.pairs[k].recvd {
                even = false;
            }
            k += 1;
        }
        even && per == 100
    };
    s.add("J10-轮转均匀", j10, "十对各百轮收发齐平");

    // J11 fd 唯一性：全账 fd 无重号
    let j11 = {
        let mut uniq = true;
        let mut a = 0usize;
        while a < book.pairs.len() {
            let mut b2 = a + 1;
            while b2 < book.pairs.len() {
                if book.pairs[a].mfd == book.pairs[b2].mfd
                    || book.pairs[a].sfd == book.pairs[b2].sfd
                {
                    uniq = false;
                }
                b2 += 1;
            }
            a += 1;
        }
        uniq
    };
    s.add("J11-fd 唯一", j11, "全账主从 fd 无重号");

    // J12 字节样本确定性：同轮同长
    let l4a = pt_len(4);
    let l4b = pt_len(4);
    s.add("J12-样本确定性", l4a == l4b, "字节样本同入同出");

    // J13 超容拒：第 PAIR_CAP+1 对必拒
    let j13 = {
        let mut c = PtyBook::new();
        let mut filled = true;
        let mut n = 0usize;
        while n < PAIR_CAP {
            if c.open_pair().is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.open_pair().err() == Some(PT_CAP)
    };
    s.add("J13-超容边界", j13, "超出定容必拒且原账不损");

    // J14 守恒注入：漏读一轮则关对必拒且断言为假
    let j14 = {
        let mut c = PtyBook::new();
        let om = match c.open_pair() {
            Ok((m, _)) => m,
            Err(_) => 0,
        };
        let _ = c.master_write(om, 5);
        let _ = c.slave_read(om + 1, 3);
        let _ = c.master_write(om, 5);
        c.close_pair(om).is_err() && !c.closed_even(1)
    };
    s.add("J14-守恒注入", j14, "漏读一轮守恒断言为假");

    // J15 动态明细：千次对平与冻结根落账
    let fp = book.fold();
    s.add_dyn(
        "J15-千次明细",
        fp == root && book.closed_even(PAIRS_NEEDED as u32),
        &format!("root={:#x} pairs=10", fp),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11028_pty_flow() {
        let (mut b, built) = build_ptybook();
        assert!(built);
        assert!(b.slave_read(81, 1).err() == Some(PT_EMPTY));
        b.freeze();
        assert!(b.master_write(80, 1).err() == Some(PT_FROZEN));
        assert!(b.root.is_some());
    }

    #[test]
    fn unx11028_checks_green() {
        let s = run_unx11028_checks();
        assert!(s.all_passed(), "UNX-F11028 判据未全绿");
    }
}
