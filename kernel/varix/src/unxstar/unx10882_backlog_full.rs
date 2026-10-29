//! UNX-F10882 backlog 满矩阵（连接队列溢出）（0x1c6c0 段）
//! 锚点：UNX-F10882-J1 backlog 满正反双判据（溢出丢弃/临界可入）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c6c0;
pub const E_ECONNREFUSED: u32 = SEG_BASE + 1;
pub const E_EAGAIN: u32 = SEG_BASE + 2;
pub const E_EINVAL: u32 = SEG_BASE + 3;
pub const E_EBADF: u32 = SEG_BASE + 4;
pub const E_OK: u32 = SEG_BASE + 5;
pub const E_SEALED: u32 = SEG_BASE + 6;

pub const BACKLOG_MAX: usize = 128;

/// 监听队列：容量 + 溢出策略（拒 / 计丢）
pub struct Backlog {
    cap: usize,
    queued: usize,
    dropped: usize,
    accepted: usize,
    open: bool,
    listening: bool,
}

impl Backlog {
    pub fn new(cap: usize) -> Self {
        Backlog { cap, queued: 0, dropped: 0, accepted: 0, open: true, listening: true }
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn stop_listen(&mut self) {
        self.listening = false;
    }

    pub fn depth(&self) -> usize {
        self.queued
    }

    pub fn dropped_count(&self) -> usize {
        self.dropped
    }

    pub fn accepted_count(&self) -> usize {
        self.accepted
    }

    /// 入队：满 → 拒（模拟 SYN 丢弃）并计丢
    pub fn enqueue(&mut self) -> Result<(), u32> {
        if !self.open {
            return Err(E_EBADF);
        }
        if !self.listening {
            return Err(E_EINVAL);
        }
        if self.queued >= self.cap {
            self.dropped += 1;
            return Err(E_ECONNREFUSED);
        }
        self.queued += 1;
        Ok(())
    }

    /// accept 出队
    pub fn dequeue(&mut self) -> Result<(), u32> {
        if !self.open {
            return Err(E_EBADF);
        }
        if !self.listening {
            return Err(E_EINVAL);
        }
        if self.queued == 0 {
            return Err(E_EAGAIN);
        }
        self.queued -= 1;
        self.accepted += 1;
        Ok(())
    }

    /// 临界判定：cap-1 可入 / cap 拒
    pub fn critical_ok(&self) -> bool {
        self.queued < self.cap
    }
}

pub fn run_unx10882_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10882");
    s.add("J1-段锚", SEG_BASE == 0x1c6c0, "backlog 满段基址 0x1c6c0");
    s.add("J2-容量锚", BACKLOG_MAX == 128, "backlog 上限 128");
    s.add("J3-空队可入", {
        let mut b = Backlog::new(4);
        b.enqueue().is_ok() && b.depth() == 1
    }, "首入队 depth 1");
    s.add("J4-临界可入", {
        let mut b = Backlog::new(2);
        b.enqueue().unwrap();
        b.enqueue().is_ok() && b.critical_ok() == false
    }, "满前临界入队 Ok 且满后 critical 假");
    s.add("J5-溢出拒并计丢", {
        let mut b = Backlog::new(1);
        b.enqueue().unwrap();
        b.enqueue() == Err(E_ECONNREFUSED) && b.dropped_count() == 1
    }, "溢出 E_ECONNREFUSED 丢 1");
    s.add("J6-溢出多次累计", {
        let mut b = Backlog::new(1);
        b.enqueue().unwrap();
        let _ = b.enqueue();
        let _ = b.enqueue();
        let _ = b.enqueue();
        b.dropped_count() == 3 && b.depth() == 1
    }, "三丢计 3 队深 1");
    s.add("J7-出队腾位", {
        let mut b = Backlog::new(1);
        b.enqueue().unwrap();
        b.dequeue().unwrap();
        b.enqueue().is_ok()
    }, "出一进一恢复");
    s.add("J8-空队出 EAGAIN", {
        let mut b = Backlog::new(4);
        b.dequeue() == Err(E_EAGAIN)
    }, "空队 E_EAGAIN");
    s.add("J9-accept 计数", {
        let mut b = Backlog::new(4);
        b.enqueue().unwrap();
        b.enqueue().unwrap();
        b.dequeue().unwrap();
        b.dequeue().unwrap();
        b.accepted_count() == 2
    }, "两出 accepted 2");
    s.add("J10-停听拒入", {
        let mut b = Backlog::new(4);
        b.stop_listen();
        b.enqueue() == Err(E_EINVAL)
    }, "停听 E_EINVAL");
    s.add("J11-停听拒出", {
        let mut b = Backlog::new(4);
        b.enqueue().unwrap();
        b.stop_listen();
        b.dequeue() == Err(E_EINVAL)
    }, "停听出队 E_EINVAL");
    s.add("J12-关闭优先", {
        let mut b = Backlog::new(4);
        b.close();
        b.enqueue() == Err(E_EBADF) && b.dequeue() == Err(E_EBADF)
    }, "关后出入皆 E_EBADF");
    s.add("J13-容量大队列", {
        let mut b = Backlog::new(BACKLOG_MAX);
        let mut ok = true;
        let mut n = 0;
        while n < BACKLOG_MAX {
            ok = ok && b.enqueue().is_ok();
            n += 1;
        }
        ok && b.enqueue() == Err(E_ECONNREFUSED)
    }, "128 入满第 129 拒");
    s.add("J14-大队列全清", {
        let mut b = Backlog::new(BACKLOG_MAX);
        let mut n = 0;
        while n < BACKLOG_MAX {
            b.enqueue().unwrap();
            n += 1;
        }
        n = 0;
        let mut ok = true;
        while n < BACKLOG_MAX {
            ok = ok && b.dequeue().is_ok();
            n += 1;
        }
        ok && b.depth() == 0 && b.dropped_count() == 0
    }, "128 全清零丢");
    s.add("J15-小容量账", {
        let mut b = Backlog::new(3);
        b.enqueue().unwrap();
        b.enqueue().unwrap();
        b.enqueue().unwrap();
        let _ = b.enqueue();
        let r = (b.depth(), b.dropped_count(), b.critical_ok());
        r == (3, 1, false)
    }, "(3,1,false) 小容量账");
    s.add_dyn("J16-出后丢账不减", {
        let mut b = Backlog::new(2);
        b.enqueue().unwrap();
        b.enqueue().unwrap();
        let _ = b.enqueue();
        b.dequeue().unwrap();
        let d1 = b.dropped_count();
        b.enqueue().unwrap();
        d1 == 1 && b.dropped_count() == 1
    }, "丢账 1 出队后补入丢账仍 1");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_backlog_full() {
        let mut b = Backlog::new(2);
        b.enqueue().unwrap();
        b.enqueue().unwrap();
        assert_eq!(b.enqueue(), Err(E_ECONNREFUSED));
        assert_eq!(b.dequeue(), Ok(()));
        assert!(b.enqueue().is_ok());
    }

    #[test]
    fn checks_green() {
        let s = run_unx10882_checks();
        assert!(s.all_passed(), "UNX-F10882 判据全绿");
    }
}
