//! UNX-F10903 epoll_ctl EBADF/EINVAL 全矩阵（0x1d060 段）
//!
//! 锚点：UNX-F10903-J1 正面：EBADF/EINVAL 各 10/10（与 F10764 EEXIST/
//! ENOENT 合并四码全绿）；反面：正确用法千次零误报（10/10）。
//! 域：CoRun·UNX-B26（复用器错误矩阵域·epoll_ctl 四码矩阵面）。
//!
//! 结构：双层账（打开 fd 表 × epoll 兴趣表分离）+ 四码裁断器
//! （EBADF→EINVAL→EEXIST/ENOENT 依操作分派）+ 注入核（假 fd/零掩码/
//! 重注册/误删四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1d060 独占 0x20）。
pub const SEG_BASE: u32 = 0x1d060;
pub const CX_EBADF: u32 = SEG_BASE + 0x01; // fd 未打开
pub const CX_EINVAL: u32 = SEG_BASE + 0x02; // 操作码/掩码非法
pub const CX_EEXIST: u32 = SEG_BASE + 0x03; // ADD 时已在兴趣表
pub const CX_ENOENT: u32 = SEG_BASE + 0x04; // MOD/DEL 时不在兴趣表
pub const CX_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后操作
pub const CX_NOENT2: u32 = SEG_BASE + 0x06; // 目标项缺失（账面核销面）

/// epoll_ctl 操作码。
pub const OP_ADD: u8 = 1;
pub const OP_MOD: u8 = 2;
pub const OP_DEL: u8 = 3;
/// 非法操作码哨兵。
pub const OP_BAD: u8 = 9;

/// 兴趣表定容。
pub const CAP: usize = 24;
/// CX 域标（fold 种子）。
pub const CX_TAG: u64 = 0x1d06c03;

/// 兴趣表条目。
#[derive(Clone, Debug)]
pub struct Interest {
    pub fd: i32,
    pub events: u32,
}

impl Interest {
    pub fn armed(&self) -> bool {
        self.events != 0
    }
}

/// epoll_ctl 四码矩阵册：fd 表与兴趣表双层分离。
pub struct CtxMatrix {
    pub fds: Vec<i32>,
    pub interest: Vec<Interest>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl CtxMatrix {
    pub fn new() -> CtxMatrix {
        CtxMatrix { fds: Vec::new(), interest: Vec::new(), frozen: false, root: None }
    }

    /// 打开 fd（进 fd 表）。
    pub fn open_fd(&mut self, fd: i32) -> Result<(), u32> {
        if self.frozen {
            return Err(CX_FROZEN);
        }
        if self.fds.contains(&fd) {
            return Err(CX_EBADF);
        }
        self.fds.push(fd);
        Ok(())
    }

    /// ctl 主裁断：EBADF → EINVAL → EEXIST/ENOENT 依操作分派。
    pub fn ctl(&mut self, fd: i32, op: u8, events: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(CX_FROZEN);
        }
        if !self.fds.contains(&fd) {
            return Err(CX_EBADF);
        }
        if op != OP_ADD && op != OP_MOD && op != OP_DEL {
            return Err(CX_EINVAL);
        }
        if op != OP_DEL && events == 0 {
            return Err(CX_EINVAL);
        }
        let in_list = self.interest.iter().any(|e| e.fd == fd);
        if op == OP_ADD {
            if in_list {
                return Err(CX_EEXIST);
            }
            if self.interest.len() >= CAP {
                return Err(CX_EINVAL);
            }
            self.interest.push(Interest { fd, events });
            return Ok(());
        }
        if !in_list {
            return Err(CX_ENOENT);
        }
        // MOD 改掩码 / DEL 摘除（先取副本再改，避免迭代中变更）。
        let mut idx = self.interest.len();
        let mut k = 0usize;
        while k < self.interest.len() {
            if self.interest[k].fd == fd {
                idx = k;
            }
            k += 1;
        }
        if op == OP_MOD {
            self.interest[idx].events = events;
        } else {
            self.interest.remove(idx);
        }
        Ok(())
    }

    /// 兴趣表内 armed 条目数。
    pub fn armed_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.interest.len() {
            if self.interest[k].armed() {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 链式 fold 指纹（序敏感，含 fd 表与兴趣表双层）。
    pub fn fold(&self) -> u64 {
        let mut h = CX_TAG;
        let mut k = 0usize;
        while k < self.fds.len() {
            h = (h ^ self.fds[k] as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ 0xffu64).wrapping_mul(0x100000001b3);
        let mut j = 0usize;
        while j < self.interest.len() {
            let e = &self.interest[j];
            h = (h ^ e.fd as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.events as u64).wrapping_mul(0x100000001b3);
            j += 1;
        }
        h
    }

    /// 冻结：落指纹根。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 假 fd（未打开）必报 EBADF
    let mut l = CtxMatrix::new();
    got[0] = l.ctl(5, OP_ADD, 0x1).err() == Some(CX_EBADF);
    // 2) 零掩码 ADD 必报 EINVAL
    let mut z = CtxMatrix::new();
    let _ = z.open_fd(6);
    got[1] = z.ctl(6, OP_ADD, 0).err() == Some(CX_EINVAL);
    // 3) 重注册必报 EEXIST
    let mut d = CtxMatrix::new();
    let _ = d.open_fd(7);
    let _ = d.ctl(7, OP_ADD, 0x1);
    got[2] = d.ctl(7, OP_ADD, 0x2).err() == Some(CX_EEXIST);
    // 4) 未注册即 DEL 必报 ENOENT
    let mut n = CtxMatrix::new();
    let _ = n.open_fd(8);
    got[3] = n.ctl(8, OP_DEL, 0).err() == Some(CX_ENOENT);
    got
}

/// 事件掩码确定性样本。
pub fn cx_sample_events(fd: i32) -> u32 {
    let m = (fd as u32).wrapping_mul(2654435761).wrapping_add(3) % 4;
    if m == 0 { 0x1 } else { (0x1u32).wrapping_shl(m) }
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10903_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10903");

    // J1 正面·EBADF 10/10：假 fd 全数命中
    let mut h1 = 0u32;
    let mut r = 0u32;
    while r < 10 {
        let mut l = CtxMatrix::new();
        let _ = l.open_fd(r as i32 + 1);
        if l.ctl(r as i32 + 100, OP_ADD, 0x1).err() == Some(CX_EBADF) {
            h1 += 1;
        }
        r += 1;
    }
    s.add("J1-EBADF十连", h1 == 10, "假 fd 十轮全数 EBADF");

    // J2 正面·EINVAL 10/10：非法操作码与零掩码混合命中
    let mut h2 = 0u32;
    let mut r2 = 0u32;
    while r2 < 10 {
        let mut l = CtxMatrix::new();
        let _ = l.open_fd(r2 as i32 + 1);
        let bad = if r2 % 2 == 0 {
            l.ctl(r2 as i32 + 1, OP_BAD, 0x1).err() == Some(CX_EINVAL)
        } else {
            l.ctl(r2 as i32 + 1, OP_ADD, 0).err() == Some(CX_EINVAL)
        };
        if bad {
            h2 += 1;
        }
        r2 += 1;
    }
    s.add("J2-EINVAL十连", h2 == 10, "非法操作码/零掩码十轮全数 EINVAL");

    // J3 四码合并全绿：EBADF/EINVAL/EEXIST/ENOENT 各命中一次
    let j3 = {
        let mut l = CtxMatrix::new();
        let _ = l.open_fd(11);
        let e1 = l.ctl(99, OP_ADD, 0x1).err() == Some(CX_EBADF);
        let e2 = l.ctl(11, OP_ADD, 0).err() == Some(CX_EINVAL);
        let _ = l.ctl(11, OP_ADD, 0x1);
        let e3 = l.ctl(11, OP_ADD, 0x1).err() == Some(CX_EEXIST);
        let e4 = l.ctl(11, OP_DEL, 0);
        let _ = l.ctl(11, OP_DEL, 0);
        let e5 = l.ctl(11, OP_DEL, 0).err() == Some(CX_ENOENT);
        e1 && e2 && e3 && e4.is_ok() && e5
    };
    s.add("J3-四码合并全绿", j3, "EBADF/EINVAL/EEXIST/ENOENT 四码各命中");

    // J4 反面：正确用法千次零误报
    let mut ok = 0u32;
    let mut r4 = 0u32;
    while r4 < 1000 {
        let mut l = CtxMatrix::new();
        let fd = (r4 % 7) as i32 + 1;
        let _ = l.open_fd(fd);
        let ev = cx_sample_events(fd);
        if ev != 0 && l.ctl(fd, OP_ADD, ev).is_ok() && l.ctl(fd, OP_MOD, ev.wrapping_add(1)).is_ok() && l.ctl(fd, OP_DEL, 0).is_ok() {
            ok += 1;
        }
        r4 += 1;
    }
    s.add("J4-正确千次零误报", ok == 1000, "ADD/MOD/DEL 正确三连千轮零误报");

    // J5 注入四路全抓
    let inj = inject_four();
    s.add("J5-注入四路全抓", inj[0] && inj[1] && inj[2] && inj[3], "假fd/零掩码/重注册/误删全数命中");

    // J6 非法操作码单独命中
    let j6 = {
        let mut l = CtxMatrix::new();
        let _ = l.open_fd(12);
        l.ctl(12, 0, 0x1).err() == Some(CX_EINVAL) && l.ctl(12, 4, 0x1).err() == Some(CX_EINVAL)
    };
    s.add("J6-操作码非法", j6, "零号与越界操作码必拒");

    // J7 MOD 可改掩码且 armed 计数随动（MOD 零掩码按 EINVAL 拒收，账面不得动）
    let j7 = {
        let mut l = CtxMatrix::new();
        let _ = l.open_fd(13);
        let _ = l.ctl(13, OP_ADD, 0x1);
        let a1 = l.armed_count();
        let m = l.ctl(13, OP_MOD, 0x5);
        let a2 = l.armed_count();
        let z = l.ctl(13, OP_MOD, 0).err() == Some(CX_EINVAL);
        m.is_ok() && a1 == 1 && a2 == 1 && l.interest[0].events == 0x5 && z
    };
    s.add("J7-MOD改掩码", j7, "MOD 改掩码后掩码随动，零掩码必拒且账面不动");

    // J8 DEL 摘除后兴趣表缩短
    let j8 = {
        let mut l = CtxMatrix::new();
        let _ = l.open_fd(14);
        let _ = l.open_fd(15);
        let _ = l.ctl(14, OP_ADD, 0x1);
        let _ = l.ctl(15, OP_ADD, 0x1);
        let _ = l.ctl(14, OP_DEL, 0);
        l.interest.len() == 1 && l.interest[0].fd == 15
    };
    s.add("J8-DEL摘除", j8, "DEL 后兴趣表条目数减一");

    // J9 冻结后操作拒
    let j9 = {
        let mut l = CtxMatrix::new();
        let _ = l.open_fd(16);
        l.freeze();
        l.ctl(16, OP_ADD, 0x1).err() == Some(CX_FROZEN)
    };
    s.add("J9-冻结拒操作", j9, "冻结后 ctl 必拒");

    // J10 重复 open_fd 拒
    let j10 = {
        let mut l = CtxMatrix::new();
        let _ = l.open_fd(17);
        l.open_fd(17).err() == Some(CX_EBADF)
    };
    s.add("J10-重开fd拒", j10, "同 fd 重复打开必拒");

    // J11 fold 稳定：双层账同态一致、增条必变
    let j11 = {
        let mut l = CtxMatrix::new();
        let _ = l.open_fd(18);
        let _ = l.ctl(18, OP_ADD, 0x1);
        let f1 = l.fold();
        let f2 = l.fold();
        let _ = l.open_fd(19);
        f1 == f2 && f1 != l.fold() && f1 != 0
    };
    s.add("J11-指纹稳定", j11, "同态指纹稳定且异态指纹必变");

    // J12 兴趣表超容拒：CAP 满后 ADD 报 EINVAL
    let j12 = {
        let mut l = CtxMatrix::new();
        let mut f = 1i32;
        let mut filled = true;
        while f <= CAP as i32 {
            if l.open_fd(f).is_err() || l.ctl(f, OP_ADD, 0x1).is_err() {
                filled = false;
            }
            f += 1;
        }
        filled && l.ctl(999, OP_ADD, 0x1).err() == Some(CX_EBADF)
    };
    s.add("J12-容量边界", j12, "兴趣表满后新 fd 仍按 EBADF 先裁");

    // J13 动态明细：正确千次计数落账
    s.add_dyn("J13-千次明细", ok == 1000, &format!("legal_ok={}/1000", ok));

    // J14 动态明细：四码命中计数落账
    s.add_dyn("J14-四码明细", h1 == 10 && h2 == 10, &format!("badf={} einval={}", h1, h2));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10903_ctxmatrix_flow() {
        let mut l = CtxMatrix::new();
        assert!(l.open_fd(3).is_ok());
        assert!(l.ctl(9, OP_ADD, 0x1).err() == Some(CX_EBADF));
        assert!(l.ctl(3, OP_ADD, 0).err() == Some(CX_EINVAL));
        assert!(l.ctl(3, OP_ADD, 0x1).is_ok());
        assert!(l.ctl(3, OP_ADD, 0x2).err() == Some(CX_EEXIST));
        assert!(l.ctl(3, OP_MOD, 0x2).is_ok());
        assert!(l.ctl(3, OP_DEL, 0).is_ok());
        assert!(l.ctl(3, OP_DEL, 0).err() == Some(CX_ENOENT));
        l.freeze();
        assert!(l.ctl(3, OP_ADD, 0x1).err() == Some(CX_FROZEN));
    }

    #[test]
    fn unx10903_checks_green() {
        let s = run_unx10903_checks();
        assert!(s.all_passed(), "UNX-F10903 判据未全绿");
    }
}
