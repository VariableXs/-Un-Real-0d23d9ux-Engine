//! UNX-F11014 close/资源回收挂点对账（注销与挂点清扫账）（0x1e1c0 段）
//!
//! 锚点：UNX-F11014-J1 close 注销千次零残留（10/10），回收账逐件核销
//! （10/10）。
//! 域：CoRun·UNX-C4-B31（futex/epoll 挂号联签域）。
//!
//! 结构：资源账（fd 挂点逐件登记：fd + 挂点类 + 引用数）+ 注销器（close
//! 逐件翻核销位、引用归零）+ 清扫账（逐件核销后零残留断言）+ fold 指纹
//! 与冻结 + 注入核（重复注销/注销不存在件/带引用强收/冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e1c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e1c0;
pub const RC_RECLOSE: u32 = SEG_BASE + 0x01; // 重复注销
pub const RC_NOFD: u32 = SEG_BASE + 0x02; // 注销不存在件
pub const RC_REFCNT: u32 = SEG_BASE + 0x03; // 带引用强收
pub const RC_RESIDUE: u32 = SEG_BASE + 0x04; // 清扫后残留
pub const RC_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后写账
pub const RC_CAP: u32 = SEG_BASE + 0x06; // 资源表超容

/// 千次注销轮数。
pub const ROUNDS: u32 = 1000;
/// 资源表定容。
pub const RES_CAP: usize = 128;
/// 回收账域标（fold 种子）。
pub const RC_TAG: u64 = 0x1e1c_0e;

/// 单条资源件：fd + 挂点类 + 引用数 + 核销位。
#[derive(Clone, Debug)]
pub struct ResItem {
    pub fd: u32,
    pub kind: u32,
    pub refs: u32,
    pub recycled: bool,
}

impl ResItem {
    pub fn render(&self) -> String {
        let flag = if self.recycled { "R" } else { "L" };
        format!("fd{}|{}|r{}|{}", self.fd, flag, self.refs, self.kind)
    }
}

/// close 资源回收账：登记 → 引用挂账 → 注销核销 → 清扫 → 冻结。
pub struct RecycleLedger {
    pub items: Vec<ResItem>,
    pub close_count: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl RecycleLedger {
    pub fn new() -> RecycleLedger {
        RecycleLedger { items: Vec::new(), close_count: 0, frozen: false, root: None }
    }

    /// 登记资源件：冻结、零 fd、活账重号、超容均拒；已核销件可复用重登。
    pub fn enroll(&mut self, fd: u32, kind: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(RC_FROZEN);
        }
        if fd == 0 {
            return Err(RC_NOFD);
        }
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].fd == fd {
                if !self.items[k].recycled {
                    return Err(RC_RECLOSE);
                }
                // 复用重登：复位引用与核销位
                self.items[k].kind = kind;
                self.items[k].refs = 0;
                self.items[k].recycled = false;
                return Ok(());
            }
            k += 1;
        }
        if self.items.len() >= RES_CAP {
            return Err(RC_CAP);
        }
        self.items.push(ResItem { fd, kind, refs: 0, recycled: false });
        Ok(())
    }

    /// 引用挂账：核销件不得再挂引用。
    pub fn ref_up(&mut self, fd: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(RC_FROZEN);
        }
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].fd == fd {
                if self.items[k].recycled {
                    return Err(RC_RESIDUE);
                }
                self.items[k].refs = self.items[k].refs.wrapping_add(1);
                return Ok(self.items[k].refs);
            }
            k += 1;
        }
        Err(RC_NOFD)
    }

    /// 引用减账：归零后才可注销。
    pub fn ref_down(&mut self, fd: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(RC_FROZEN);
        }
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].fd == fd {
                if self.items[k].refs == 0 {
                    return Err(RC_REFCNT);
                }
                self.items[k].refs -= 1;
                return Ok(self.items[k].refs);
            }
            k += 1;
        }
        Err(RC_NOFD)
    }

    /// close 注销：引用未清必拒（带引用强收），重复注销必拒。
    pub fn close(&mut self, fd: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(RC_FROZEN);
        }
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].fd == fd {
                if self.items[k].recycled {
                    return Err(RC_RECLOSE);
                }
                if self.items[k].refs != 0 {
                    return Err(RC_REFCNT);
                }
                self.items[k].recycled = true;
                self.close_count = self.close_count.wrapping_add(1);
                return Ok(());
            }
            k += 1;
        }
        Err(RC_NOFD)
    }

    /// 残留清单：未核销件 fd（清扫面）。
    pub fn residue(&self) -> Vec<u32> {
        let mut g: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.items.len() {
            if !self.items[k].recycled {
                g.push(self.items[k].fd);
            }
            k += 1;
        }
        g
    }

    /// 零残留断言：全部核销且引用全零。
    pub fn swept(&self) -> bool {
        self.residue().is_empty() && {
            let mut k = 0usize;
            while k < self.items.len() {
                if self.items[k].refs != 0 {
                    return false;
                }
                k += 1;
            }
            true
        }
    }

    /// 链式 fold 指纹（FNV 风格，序敏感，含 fd/类/引用/核销位）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = RC_TAG;
        let mut k = 0usize;
        while k < self.items.len() {
            let e = &self.items[k];
            h = (h ^ e.fd as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.kind as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.refs as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.recycled as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.close_count as u64).wrapping_mul(0x100000001b3);
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

/// fd 与类确定性样本。
pub fn rc_fd(no: u32) -> u32 {
    no.wrapping_mul(7).wrapping_add(2)
}

pub fn rc_kind(no: u32) -> u32 {
    no % 4
}

/// 千次注销全链：千轮内循环登记/注销同一 fd 池（计数跨轮累计）。
pub fn run_close_thousand() -> (RecycleLedger, bool) {
    let mut l = RecycleLedger::new();
    let mut ok = true;
    let mut r = 0u32;
    while r < ROUNDS {
        let fd = rc_fd(r % 10 + 1);
        // 轮内：登记 → 引用挂/减 → 注销 → 再登记供下轮（表容量轮转重用）
        if l.items.len() >= RES_CAP {
            l.items.clear();
        }
        if l.enroll(fd, rc_kind(r)).is_err() {
            ok = false;
        }
        let _ = l.ref_up(fd);
        let _ = l.ref_down(fd);
        if l.close(fd).is_err() {
            ok = false;
        }
        r += 1;
    }
    let l_ok = ok && l.close_count == ROUNDS;
    (l, l_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 重复注销必拒
    let mut a = RecycleLedger::new();
    let _ = a.enroll(3, 1);
    let _ = a.close(3);
    got[0] = a.close(3).err() == Some(RC_RECLOSE);
    // 2) 注销不存在件必拒
    let mut b = RecycleLedger::new();
    got[1] = b.close(999).err() == Some(RC_NOFD);
    // 3) 带引用强收必拒
    let mut c = RecycleLedger::new();
    let _ = c.enroll(3, 1);
    let _ = c.ref_up(3);
    got[2] = c.close(3).err() == Some(RC_REFCNT) && !c.items[0].recycled;
    // 4) 冻结后写账必抓
    let (mut d, _) = run_close_thousand();
    d.freeze();
    got[3] = d.enroll(1, 1).err() == Some(RC_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11014_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11014");

    // J1 close 注销千次零残留：千次注销计数闭合
    let (mut book, green) = run_close_thousand();
    s.add("J1-千次注销零残留", green && book.close_count == ROUNDS, "千轮登记/注销循环计数零漂");

    // J2 回收账逐件核销：表内核销位与残留面一致
    let rec_n = book.items.iter().filter(|i| i.recycled).count();
    s.add("J2-逐件核销", rec_n == book.items.len() && book.swept(), "表内逐件核销且引用全零");

    // J3 清扫断言：swept 为真且 residue 清空
    s.add("J3-清扫断言", book.swept() && book.residue().is_empty(), "清扫后零残留成立");

    // J4 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J4-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J5 注入检出：重复注销 + 注销不存在件
    let inj = inject_four();
    s.add("J5-重注关无检出", inj[0] && inj[1], "重复注销与关不存在两路必抓");

    // J6 注入检出：带引用强收 + 冻结后写
    s.add("J6-强收冻结检出", inj[2] && inj[3], "带引用强收必拒且冻结后写必抓");

    // J7 引用纪律：挂/减归零后可注销
    let j7 = {
        let mut c = RecycleLedger::new();
        let _ = c.enroll(3, 1);
        let _ = c.ref_up(3);
        let _ = c.ref_up(3);
        let n2 = c.ref_down(3);
        let n1 = c.ref_down(3);
        let zero = c.close(3);
        n2 == Ok(1) && n1 == Ok(0) && zero.is_ok()
    };
    s.add("J7-引用纪律", j7, "引用归零后注销放行");

    // J8 引用减越界必拒
    let j8 = {
        let mut c = RecycleLedger::new();
        let _ = c.enroll(3, 1);
        c.ref_down(3).err() == Some(RC_REFCNT)
    };
    s.add("J8-减越界拒", j8, "零引用再减必拒");

    // J9 核销件不得再挂引用
    let j9 = {
        let mut c = RecycleLedger::new();
        let _ = c.enroll(3, 1);
        let _ = c.close(3);
        c.ref_up(3).err() == Some(RC_RESIDUE)
    };
    s.add("J9-死件拒挂引用", j9, "核销后挂引用必拒");

    // J10 零 fd 拒与重号拒
    let j10 = RecycleLedger::new().enroll(0, 1).err() == Some(RC_NOFD) && {
        let mut c = RecycleLedger::new();
        let _ = c.enroll(3, 1);
        c.enroll(3, 2).err() == Some(RC_RECLOSE)
    };
    s.add("J10-零 fd 重号拒", j10, "零 fd 与重号均必拒");

    // J11 超容拒：第 RES_CAP+1 件必拒
    let j11 = {
        let mut c = RecycleLedger::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= RES_CAP as u32 {
            if c.enroll(n.wrapping_add(1), n % 4).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.enroll(99999, 1).err() == Some(RC_CAP)
    };
    s.add("J11-超容边界", j11, "超出定容必拒且原账不损");

    // J12 渲染面：核销与在册标记可区分
    let j12 = {
        let a = ResItem { fd: 5, kind: 2, refs: 0, recycled: true };
        let b = ResItem { fd: 5, kind: 2, refs: 0, recycled: false };
        a.render() != b.render() && a.render().contains('R') && b.render().contains('L')
    };
    s.add("J12-渲染分旗", j12, "核销/在册渲染可区分");

    // J13 部分清扫：核销半数残留恰半数
    let j13 = {
        let mut c = RecycleLedger::new();
        let mut i = 1u32;
        while i <= 6u32 {
            let _ = c.enroll(rc_fd(i), rc_kind(i));
            i += 1;
        }
        let mut i = 1u32;
        while i <= 3u32 {
            let _ = c.close(rc_fd(i));
            i += 1;
        }
        c.residue().len() == 3 && !c.swept()
    };
    s.add("J13-部分清扫", j13, "核销半数残留恰半数且清扫为假");

    // J14 动态明细：千次注销计数落账
    let cc = book.close_count;
    s.add_dyn("J14-注销计数明细", cc == ROUNDS, &format!("close_count={}", cc));

    // J15 动态明细：冻结根十六进制落账
    let fp = book.fold();
    s.add_dyn("J15-冻结根明细", fp == root, &format!("root={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11014_recycle_flow() {
        let mut l = RecycleLedger::new();
        assert!(l.enroll(0, 1).err() == Some(RC_NOFD));
        assert!(l.enroll(3, 1).is_ok());
        assert!(l.enroll(3, 2).err() == Some(RC_RECLOSE));
        assert!(l.ref_up(3).is_ok());
        assert!(l.close(3).err() == Some(RC_REFCNT));
        assert!(l.ref_down(3).is_ok());
        assert!(l.close(3).is_ok());
        assert!(l.close(3).err() == Some(RC_RECLOSE));
        l.freeze();
        assert!(l.enroll(4, 1).err() == Some(RC_FROZEN));
        assert!(l.root.is_some());
    }

    #[test]
    fn unx11014_checks_green() {
        let s = run_unx11014_checks();
        assert!(s.all_passed(), "UNX-F11014 判据未全绿");
    }
}
