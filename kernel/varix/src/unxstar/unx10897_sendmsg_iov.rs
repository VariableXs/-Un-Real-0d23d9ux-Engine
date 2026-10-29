//! UNX-F10897 sendmsg iov 散布错误矩阵（部分 iov 非法）（0x1c8a0 段）
//! 锚点：UNX-F10897-J1 sendmsg iov 散布（前段已拷后段非法/全或无账）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c8a0;
pub const E_EFAULT: u32 = SEG_BASE + 1;
pub const E_EINVAL: u32 = SEG_BASE + 2;
pub const E_EMSGSIZE: u32 = SEG_BASE + 3;
pub const E_EBADF: u32 = SEG_BASE + 4;
pub const E_OK: u32 = SEG_BASE + 5;
pub const E_SEALED: u32 = SEG_BASE + 6;

/// 散布段：长度 + 指针健康
#[derive(Clone, Copy, Debug)]
pub struct ScatterSeg {
    pub len: usize,
    pub bad: bool,
}

impl ScatterSeg {
    pub fn good(len: usize) -> Self {
        ScatterSeg { len, bad: false }
    }
    pub fn bad_seg(len: usize) -> Self {
        ScatterSeg { len, bad: true }
    }
}

/// 散布发送器：语义=「全或无」——任一段非法整报拒收
pub struct ScatterSend {
    open: bool,
    total_cap: usize,
    sent: usize,
}

impl ScatterSend {
    pub fn new(total_cap: usize) -> Self {
        ScatterSend { open: true, total_cap, sent: 0 }
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn sent_of(&self) -> usize {
        self.sent
    }

    /// sendmsg：任一段非法 → EFAULT 整拒；全健康 → 合计入账
    pub fn sendmsg(&mut self, segs: &[ScatterSeg]) -> Result<usize, u32> {
        if !self.open {
            return Err(E_EBADF);
        }
        if segs.is_empty() {
            return Err(E_EINVAL);
        }
        let mut total = 0;
        let mut i = 0;
        while i < segs.len() {
            if segs[i].bad {
                return Err(E_EFAULT);
            }
            total += segs[i].len;
            i += 1;
        }
        if total > self.total_cap {
            return Err(E_EMSGSIZE);
        }
        self.sent += total;
        Ok(total)
    }

    /// 前缀健康预检：首 k 段全健康
    pub fn prefix_ok(segs: &[ScatterSeg], k: usize) -> bool {
        let mut i = 0;
        while i < k && i < segs.len() {
            if segs[i].bad {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_unx10897_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10897");
    s.add("J1-段锚", SEG_BASE == 0x1c8a0, "iov 散布段基址 0x1c8a0");
    s.add("J2-全健康入账", {
        let mut g = ScatterSend::new(64);
        g.sendmsg(&[ScatterSeg::good(4), ScatterSeg::good(6)]) == Ok(10)
    }, "4+6=10 Ok");
    s.add("J3-尾段非法整拒", {
        let mut g = ScatterSend::new(64);
        g.sendmsg(&[ScatterSeg::good(4), ScatterSeg::bad_seg(6)]) == Err(E_EFAULT)
    }, "尾段坏 E_EFAULT");
    s.add("J4-首段非法整拒", {
        let mut g = ScatterSend::new(64);
        g.sendmsg(&[ScatterSeg::bad_seg(4), ScatterSeg::good(6)]) == Err(E_EFAULT)
    }, "首段坏 E_EFAULT");
    s.add("J5-全或无不半拷", {
        let mut g = ScatterSend::new(64);
        let _ = g.sendmsg(&[ScatterSeg::good(8), ScatterSeg::bad_seg(2)]);
        g.sent_of() == 0
    }, "拒收后 sent 0");
    s.add("J6-成功入账", {
        let mut g = ScatterSend::new(64);
        g.sendmsg(&[ScatterSeg::good(8), ScatterSeg::good(8)]).unwrap();
        g.sent_of() == 16
    }, "两报累计 16");
    s.add("J7-空 iov 拒", {
        let mut g = ScatterSend::new(64);
        g.sendmsg(&[]) == Err(E_EINVAL)
    }, "空 E_EINVAL");
    s.add("J8-总量超限拒", {
        let mut g = ScatterSend::new(8);
        g.sendmsg(&[ScatterSeg::good(5), ScatterSeg::good(5)]) == Err(E_EMSGSIZE)
    }, "10>8 E_EMSGSIZE");
    s.add("J9-总量恰限过", {
        let mut g = ScatterSend::new(8);
        g.sendmsg(&[ScatterSeg::good(5), ScatterSeg::good(3)]) == Ok(8)
    }, "8=8 Ok");
    s.add("J10-EFAULT 先于 EMSGSIZE", {
        let mut g = ScatterSend::new(2);
        g.sendmsg(&[ScatterSeg::good(9), ScatterSeg::bad_seg(9)]) == Err(E_EFAULT)
    }, "坏+超 EFAULT 优先");
    s.add("J11-零长段合法", {
        let mut g = ScatterSend::new(64);
        g.sendmsg(&[ScatterSeg::good(0), ScatterSeg::good(5), ScatterSeg::good(0)]) == Ok(5)
    }, "零长夹段 Ok(5)");
    s.add("J12-关闭后拒", {
        let mut g = ScatterSend::new(64);
        g.close();
        g.sendmsg(&[ScatterSeg::good(1)]) == Err(E_EBADF)
    }, "关后 E_EBADF");
    s.add("J13-EBADF 先于空 iov", {
        let mut g = ScatterSend::new(64);
        g.close();
        g.sendmsg(&[]) == Err(E_EBADF)
    }, "关+空 EBADF 优先");
    s.add("J14-前缀健康探针", {
        let segs = [ScatterSeg::good(1), ScatterSeg::good(2), ScatterSeg::bad_seg(3)];
        ScatterSend::prefix_ok(&segs, 2) && !ScatterSend::prefix_ok(&segs, 3)
    }, "前 2 健康前 3 假");
    s.add("J15-单段零长报过", {
        let mut g = ScatterSend::new(64);
        g.sendmsg(&[ScatterSeg::good(0)]) == Ok(0) && g.sent_of() == 0
    }, "单零段 Ok(0)");
    s.add_dyn("J16-多段非法位置账", {
        let mut g = ScatterSend::new(64);
        let r1 = g.sendmsg(&[ScatterSeg::good(2), ScatterSeg::bad_seg(2)]);
        let r2 = g.sendmsg(&[ScatterSeg::good(2)]);
        r1.is_err() && r2 == Ok(2) && g.sent_of() == 2
    }, "EFAULT→Ok(2)→sent 2 账");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_scatter_iov() {
        let mut g = ScatterSend::new(64);
        assert_eq!(g.sendmsg(&[ScatterSeg::good(3), ScatterSeg::good(4)]), Ok(7));
        assert_eq!(g.sendmsg(&[ScatterSeg::good(1), ScatterSeg::bad_seg(1)]), Err(E_EFAULT));
        assert_eq!(g.sent_of(), 7);
    }

    #[test]
    fn checks_green() {
        let s = run_unx10897_checks();
        assert!(s.all_passed(), "UNX-F10897 判据全绿");
    }
}
