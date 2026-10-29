//! UNX-F10871 EINTR 收发中断矩阵（0x1c560 段）
//! 锚点：UNX-F10871-J1 EINTR 正反双判据（阻塞收发被信号中断+重启语义）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c560;
pub const E_EINTR: u32 = SEG_BASE + 1;
pub const E_EAGAIN: u32 = SEG_BASE + 2;
pub const E_EBADF: u32 = SEG_BASE + 3;
pub const E_EPIPE: u32 = SEG_BASE + 4;
pub const E_OK: u32 = SEG_BASE + 5;
pub const E_SEALED: u32 = SEG_BASE + 6;

/// 中断策略：Restart（SA_RESTART）/ Return（默认）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RestartPolicy {
    Restart,
    Return,
}

/// 阻塞收发模拟器：pending 信号中断面
pub struct IntrSock {
    open: bool,
    peer_readable: bool,
    pending_signal: bool,
    policy: RestartPolicy,
    intr_hits: usize,
}

impl IntrSock {
    pub fn new(policy: RestartPolicy) -> Self {
        IntrSock {
            open: true,
            peer_readable: false,
            pending_signal: false,
            policy,
            intr_hits: 0,
        }
    }

    pub fn set_signal(&mut self, on: bool) {
        self.pending_signal = on;
    }

    pub fn set_peer_readable(&mut self, on: bool) {
        self.peer_readable = on;
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn intr_hits(&self) -> usize {
        self.intr_hits
    }

    /// 阻塞 recv：有信号且 policy=Return → EINTR；Restart → 自动重试
    pub fn blocking_recv(&mut self) -> Result<&'static str, u32> {
        loop {
            if !self.open {
                return Err(E_EBADF);
            }
            if self.pending_signal {
                self.intr_hits += 1;
                self.pending_signal = false;
                if self.policy == RestartPolicy::Return {
                    return Err(E_EINTR);
                }
                continue;
            }
            if self.peer_readable {
                return Ok("data");
            }
            return Err(E_EAGAIN);
        }
    }
}

pub fn run_unx10871_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10871");
    s.add("J1-段锚", SEG_BASE == 0x1c560, "EINTR 段基址 0x1c560");
    s.add("J2-Return 中断报 EINTR", {
        let mut k = IntrSock::new(RestartPolicy::Return);
        k.set_signal(true);
        k.blocking_recv() == Err(E_EINTR)
    }, "Return 策略 E_EINTR");
    s.add("J3-Restart 自动重试", {
        let mut k = IntrSock::new(RestartPolicy::Restart);
        k.set_signal(true);
        k.set_peer_readable(true);
        k.blocking_recv() == Ok("data")
    }, "Restart 策略重试后取到 data");
    s.add("J4-中断计数", {
        let mut k = IntrSock::new(RestartPolicy::Restart);
        k.set_signal(true);
        k.set_peer_readable(true);
        let _ = k.blocking_recv();
        k.intr_hits() == 1
    }, "重试路径命中计数 1");
    s.add("J5-信号清除", {
        let mut k = IntrSock::new(RestartPolicy::Return);
        k.set_signal(true);
        let _ = k.blocking_recv();
        k.blocking_recv() == Err(E_EAGAIN)
    }, "EINTR 后信号已清再收 E_EAGAIN");
    s.add("J6-无信号直取", {
        let mut k = IntrSock::new(RestartPolicy::Return);
        k.set_peer_readable(true);
        k.blocking_recv() == Ok("data")
    }, "无中断直接 data");
    s.add("J7-无信号无数据 EAGAIN", {
        let mut k = IntrSock::new(RestartPolicy::Return);
        k.blocking_recv() == Err(E_EAGAIN)
    }, "静默面 E_EAGAIN");
    s.add("J8-关闭后 EBADF", {
        let mut k = IntrSock::new(RestartPolicy::Return);
        k.close();
        k.blocking_recv() == Err(E_EBADF)
    }, "关闭后 E_EBADF");
    s.add("J9-EBADF 先于中断", {
        let mut k = IntrSock::new(RestartPolicy::Return);
        k.set_signal(true);
        k.close();
        k.blocking_recv() == Err(E_EBADF)
    }, "关闭+信号 EBADF 优先");
    s.add("J10-策略枚举互异", RestartPolicy::Restart != RestartPolicy::Return, "两策略互异");
    s.add("J11-连发双信号单次命中", {
        let mut k = IntrSock::new(RestartPolicy::Return);
        k.set_signal(true);
        let _ = k.blocking_recv();
        k.set_signal(true);
        let _ = k.blocking_recv();
        k.intr_hits() == 2
    }, "两轮信号命中 2");
    s.add("J12-Restart 多信号重试", {
        let mut k = IntrSock::new(RestartPolicy::Restart);
        k.set_signal(true);
        k.blocking_recv() == Err(E_EAGAIN)
    }, "Restart 无数据重试后 E_EAGAIN");
    s.add("J13-中断计数跨策略", {
        let mut a = IntrSock::new(RestartPolicy::Return);
        a.set_signal(true);
        let _ = a.blocking_recv();
        let mut b = IntrSock::new(RestartPolicy::Restart);
        b.set_signal(true);
        b.set_peer_readable(true);
        let _ = b.blocking_recv();
        a.intr_hits() == 1 && b.intr_hits() == 1
    }, "两策略各命中 1");
    s.add("J14-恢复取数", {
        let mut k = IntrSock::new(RestartPolicy::Return);
        k.set_signal(true);
        let _ = k.blocking_recv();
        k.set_peer_readable(true);
        k.blocking_recv() == Ok("data")
    }, "中断后来数取到 data");
    s.add("J15-信号置位幂等", {
        let mut k = IntrSock::new(RestartPolicy::Return);
        k.set_signal(true);
        k.set_signal(true);
        k.blocking_recv() == Err(E_EINTR)
    }, "重复置位仍单次中断");
    s.add_dyn("J16-策略切换账", {
        let mut k = IntrSock::new(RestartPolicy::Restart);
        k.set_signal(true);
        k.set_peer_readable(true);
        let r1 = k.blocking_recv();
        k.set_signal(true);
        k.policy = RestartPolicy::Return;
        let r2 = k.blocking_recv();
        r1.is_ok() && r2.is_err()
    }, "Restart Ok → Return EINTR 切换");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_eintr_policies() {
        let mut r = IntrSock::new(RestartPolicy::Return);
        r.set_signal(true);
        assert_eq!(r.blocking_recv(), Err(E_EINTR));
        let mut t = IntrSock::new(RestartPolicy::Restart);
        t.set_signal(true);
        t.set_peer_readable(true);
        assert_eq!(t.blocking_recv(), Ok("data"));
    }

    #[test]
    fn checks_green() {
        let s = run_unx10871_checks();
        assert!(s.all_passed(), "UNX-F10871 判据全绿");
    }
}
