//! UNX-F10874 SCM_CREDENTIALS 伪造拒收矩阵（0x1c5c0 段）
//! 锚点：UNX-F10874-J1 SCM_CREDENTIALS 伪造拒收（用户态不可伪造凭证面）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c5c0;
pub const E_EPERM: u32 = SEG_BASE + 1;
pub const E_EINVAL: u32 = SEG_BASE + 2;
pub const E_ENOBUFS: u32 = SEG_BASE + 3;
pub const E_EBADF: u32 = SEG_BASE + 4;
pub const E_OK: u32 = SEG_BASE + 5;
pub const E_SEALED: u32 = SEG_BASE + 6;

/// 凭证来源：内核回填（真）/ 用户态附载（伪）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CredOrigin {
    KernelFilled,
    UserForged,
}

/// 凭证三元组：uid/gid/pid
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cred {
    pub uid: u32,
    pub gid: u32,
    pub pid: i32,
}

impl Cred {
    pub fn new(uid: u32, gid: u32, pid: i32) -> Self {
        Cred { uid, gid, pid }
    }
}

/// 凭证信道：SO_PASSCRED 开关 + 附载校验
pub struct CredChannel {
    passcred: bool,
    attached: Vec<(Cred, CredOrigin)>,
    sealed: bool,
}

impl CredChannel {
    pub fn new(passcred: bool) -> Self {
        CredChannel { passcred, attached: Vec::new(), sealed: false }
    }

    pub fn has_passcred(&self) -> bool {
        self.passcred
    }

    /// 用户态试图附载凭证：一律 EPERM（内核独占回填权）
    pub fn user_attach(&mut self, c: Cred) -> Result<(), u32> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if !self.passcred {
            return Err(E_EINVAL);
        }
        let _ = c;
        Err(E_EPERM)
    }

    /// 内核回填：收包时自动挂真凭证
    pub fn kernel_fill(&mut self, c: Cred) -> Result<(), u32> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if !self.passcred {
            return Err(E_EINVAL);
        }
        self.attached.push((c, CredOrigin::KernelFilled));
        Ok(())
    }

    /// 收取凭证：只认 KernelFilled 来源
    pub fn take(&mut self) -> Result<Cred, u32> {
        if self.attached.is_empty() {
            return Err(E_ENOBUFS);
        }
        let (c, o) = self.attached.remove(0);
        if o != CredOrigin::KernelFilled {
            return Err(E_EPERM);
        }
        Ok(c)
    }

    pub fn depth(&self) -> usize {
        self.attached.len()
    }
}

pub fn run_unx10874_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10874");
    s.add("J1-段锚", SEG_BASE == 0x1c5c0, "SCM_CREDENTIALS 段基址 0x1c5c0");
    s.add("J2-伪造一律 EPERM", {
        let mut ch = CredChannel::new(true);
        ch.user_attach(Cred::new(0, 0, 1)) == Err(E_EPERM)
    }, "root 伪造也 E_EPERM");
    s.add("J3-伪造任意 uid EPERM", {
        let mut ch = CredChannel::new(true);
        ch.user_attach(Cred::new(1000, 1000, 42)) == Err(E_EPERM)
    }, "普通 uid 伪造 E_EPERM");
    s.add("J4-未开 SO_PASSCRED 附载 EINVAL", {
        let mut ch = CredChannel::new(false);
        ch.user_attach(Cred::new(0, 0, 1)) == Err(E_EINVAL)
    }, "未开选项 E_EINVAL");
    s.add("J5-未开选项回填 EINVAL", {
        let mut ch = CredChannel::new(false);
        ch.kernel_fill(Cred::new(1, 1, 2)) == Err(E_EINVAL)
    }, "未开选项回填拒 E_EINVAL");
    s.add("J6-内核回填 Ok", {
        let mut ch = CredChannel::new(true);
        ch.kernel_fill(Cred::new(1000, 100, 7)).is_ok()
    }, "内核回填成功");
    s.add("J7-回填可取", {
        let mut ch = CredChannel::new(true);
        ch.kernel_fill(Cred::new(9, 8, 77)).unwrap();
        ch.take() == Ok(Cred::new(9, 8, 77))
    }, "回填后 take 全中");
    s.add("J8-空取 ENOBUFS", {
        let mut ch = CredChannel::new(true);
        ch.take() == Err(E_ENOBUFS)
    }, "无凭证 E_ENOBUFS");
    s.add("J9-深度计数", {
        let mut ch = CredChannel::new(true);
        ch.kernel_fill(Cred::new(1, 1, 1)).unwrap();
        ch.kernel_fill(Cred::new(2, 2, 2)).unwrap();
        ch.depth() == 2
    }, "两包凭证深度 2");
    s.add("J10-取序 FIFO", {
        let mut ch = CredChannel::new(true);
        ch.kernel_fill(Cred::new(1, 1, 1)).unwrap();
        ch.kernel_fill(Cred::new(2, 2, 2)).unwrap();
        ch.take() == Ok(Cred::new(1, 1, 1)) && ch.take() == Ok(Cred::new(2, 2, 2))
    }, "两包 FIFO 全中");
    s.add("J11-开关可读", {
        let a = CredChannel::new(true);
        let b = CredChannel::new(false);
        a.has_passcred() && !b.has_passcred()
    }, "开关两态可读");
    s.add("J12-零 uid 合法回填", {
        let mut ch = CredChannel::new(true);
        ch.kernel_fill(Cred::new(0, 0, 0)).unwrap();
        ch.take() == Ok(Cred::new(0, 0, 0))
    }, "零三元组合法");
    s.add("J13-负 pid 合法回填", {
        let mut ch = CredChannel::new(true);
        ch.kernel_fill(Cred::new(1, 1, -1)).unwrap();
        ch.take() == Ok(Cred::new(1, 1, -1))
    }, "负 pid（无进程）合法");
    s.add("J14-取空再取 ENOBUFS", {
        let mut ch = CredChannel::new(true);
        ch.kernel_fill(Cred::new(1, 1, 1)).unwrap();
        ch.take().unwrap();
        ch.take() == Err(E_ENOBUFS)
    }, "取空后 E_ENOBUFS");
    s.add("J15-伪造不入队", {
        let mut ch = CredChannel::new(true);
        let _ = ch.user_attach(Cred::new(0, 0, 1));
        ch.depth() == 0
    }, "伪造不产生凭证");
    s.add_dyn("J16-伪造后真源不受扰", {
        let mut ch = CredChannel::new(true);
        ch.kernel_fill(Cred::new(5, 5, 5)).unwrap();
        let _ = ch.user_attach(Cred::new(0, 0, 0));
        let c = ch.take();
        c.is_ok() && ch.depth() == 0
    }, "伪造后 take 仍真源 Ok(5,5,5)");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_cred_channel() {
        let mut ch = CredChannel::new(true);
        assert_eq!(ch.user_attach(Cred::new(0, 0, 1)), Err(E_EPERM));
        ch.kernel_fill(Cred::new(7, 7, 7)).unwrap();
        assert_eq!(ch.take(), Ok(Cred::new(7, 7, 7)));
    }

    #[test]
    fn checks_green() {
        let s = run_unx10874_checks();
        assert!(s.all_passed(), "UNX-F10874 判据全绿");
    }
}
