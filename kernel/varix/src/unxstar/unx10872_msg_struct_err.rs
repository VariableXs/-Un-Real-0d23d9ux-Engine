//! UNX-F10872 msg 结构错误矩阵（msg_iov/msg_control 面 EINVAL/EFAULT）（0x1c580 段）
//! 锚点：UNX-F10872-J1 msg 结构错误（iov 非法/control 非法双口判定）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c580;
pub const E_EINVAL: u32 = SEG_BASE + 1;
pub const E_EFAULT: u32 = SEG_BASE + 2;
pub const E_EMSGSIZE: u32 = SEG_BASE + 3;
pub const E_EBADF: u32 = SEG_BASE + 4;
pub const E_OK: u32 = SEG_BASE + 5;
pub const E_SEALED: u32 = SEG_BASE + 6;

pub const IOV_MAX: usize = 1024;

/// 单个 iovec：长度面模拟（0 长或超总限即非法）
#[derive(Clone, Copy, Debug)]
pub struct IoVec {
    pub len: usize,
    pub bad_ptr: bool,
}

impl IoVec {
    pub fn ok(len: usize) -> Self {
        IoVec { len, bad_ptr: false }
    }
    pub fn fault(len: usize) -> Self {
        IoVec { len, bad_ptr: true }
    }
}

/// msghdr 结构：iov 集合 + control 字节
pub struct MsgHdr {
    pub iov: Vec<IoVec>,
    pub control: Vec<u8>,
    pub controllen_declared: usize,
}

impl MsgHdr {
    pub fn plain(lens: &[usize]) -> Self {
        let mut iov = Vec::new();
        for l in lens.iter() {
            iov.push(IoVec::ok(*l));
        }
        MsgHdr { iov, control: Vec::new(), controllen_declared: 0 }
    }
    pub fn total(&self) -> usize {
        let mut acc = 0;
        let mut i = 0;
        while i < self.iov.len() {
            acc += self.iov[i].len;
            i += 1;
        }
        acc
    }
}

/// sendmsg 结构校验器
pub struct MsgValidator {
    open: bool,
    size_limit: usize,
}

impl MsgValidator {
    pub fn new(size_limit: usize) -> Self {
        MsgValidator { open: true, size_limit }
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    /// 校验序：fd → iov 空数（EINVAL）→ iov 数超限（EINVAL）→
    /// 坏指针（EFAULT）→ control 声明超实（EINVAL）→ 总量超限（EMSGSIZE）
    pub fn sendmsg(&mut self, msg: &MsgHdr) -> Result<usize, u32> {
        if !self.open {
            return Err(E_EBADF);
        }
        if msg.iov.is_empty() {
            return Err(E_EINVAL);
        }
        if msg.iov.len() > IOV_MAX {
            return Err(E_EINVAL);
        }
        let mut i = 0;
        while i < msg.iov.len() {
            if msg.iov[i].bad_ptr {
                return Err(E_EFAULT);
            }
            i += 1;
        }
        if msg.controllen_declared > msg.control.len() {
            return Err(E_EINVAL);
        }
        let total = msg.total();
        if total > self.size_limit {
            return Err(E_EMSGSIZE);
        }
        Ok(total)
    }
}

pub fn run_unx10872_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10872");
    s.add("J1-段锚", SEG_BASE == 0x1c580, "msg 结构段基址 0x1c580");
    s.add("J2-iov 上限锚", IOV_MAX == 1024, "IOV_MAX 1024");
    s.add("J3-空 iov 拒", {
        let mut v = MsgValidator::new(4096);
        v.sendmsg(&MsgHdr::plain(&[])) == Err(E_EINVAL)
    }, "空 iov E_EINVAL");
    s.add("J4-单段过", {
        let mut v = MsgValidator::new(4096);
        v.sendmsg(&MsgHdr::plain(&[10])) == Ok(10)
    }, "单段 10 字节 Ok(10)");
    s.add("J5-多段合计", {
        let mut v = MsgValidator::new(4096);
        v.sendmsg(&MsgHdr::plain(&[10, 20, 30])) == Ok(60)
    }, "三段合计 60");
    s.add("J6-iov 数超限", {
        let mut v = MsgValidator::new(4096);
        let mut lens = Vec::new();
        let mut n = 0;
        while n <= IOV_MAX {
            lens.push(1);
            n += 1;
        }
        v.sendmsg(&MsgHdr::plain(&lens)) == Err(E_EINVAL)
    }, "1025 段 E_EINVAL");
    s.add("J7-iov 数恰上限", {
        let mut v = MsgValidator::new(4096);
        let mut lens = Vec::new();
        let mut n = 0;
        while n < IOV_MAX {
            lens.push(1);
            n += 1;
        }
        v.sendmsg(&MsgHdr::plain(&lens)).is_ok()
    }, "1024 段 Ok");
    s.add("J8-坏指针 EFAULT", {
        let mut v = MsgValidator::new(4096);
        let mut m = MsgHdr::plain(&[8, 8]);
        m.iov[1] = IoVec::fault(8);
        v.sendmsg(&m) == Err(E_EFAULT)
    }, "第二段坏指针 E_EFAULT");
    s.add("J9-EFAULT 先于总量", {
        let mut v = MsgValidator::new(4);
        let mut m = MsgHdr::plain(&[100]);
        m.iov[0] = IoVec::fault(100);
        v.sendmsg(&m) == Err(E_EFAULT)
    }, "坏指针优先于 EMSGSIZE");
    s.add("J10-control 声明超实", {
        let mut v = MsgValidator::new(4096);
        let mut m = MsgHdr::plain(&[4]);
        m.controllen_declared = 8;
        v.sendmsg(&m) == Err(E_EINVAL)
    }, "声明 8 实 0 E_EINVAL");
    s.add("J11-control 声明如实过", {
        let mut v = MsgValidator::new(4096);
        let mut m = MsgHdr::plain(&[4]);
        m.control = vec![1, 2, 3, 4];
        m.controllen_declared = 4;
        v.sendmsg(&m).is_ok()
    }, "声明 4 实 4 Ok");
    s.add("J12-总量超限", {
        let mut v = MsgValidator::new(16);
        v.sendmsg(&MsgHdr::plain(&[8, 9, 8])) == Err(E_EMSGSIZE)
    }, "25>16 E_EMSGSIZE");
    s.add("J13-总量恰限", {
        let mut v = MsgValidator::new(16);
        v.sendmsg(&MsgHdr::plain(&[8, 8])) == Ok(16)
    }, "16=16 Ok");
    s.add("J14-关闭后 EBADF", {
        let mut v = MsgValidator::new(4096);
        v.close();
        v.sendmsg(&MsgHdr::plain(&[1])) == Err(E_EBADF)
    }, "关闭 E_EBADF");
    s.add("J15-EBADF 先于空 iov", {
        let mut v = MsgValidator::new(4096);
        v.close();
        v.sendmsg(&MsgHdr::plain(&[])) == Err(E_EBADF)
    }, "关闭+空 iov EBADF 优先");
    s.add_dyn("J16-零长段合法", {
        let mut v = MsgValidator::new(4096);
        let r = v.sendmsg(&MsgHdr::plain(&[0, 6, 0]));
        r == Ok(6)
    }, "零长段夹段 Ok(6)");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_msg_struct() {
        let mut v = MsgValidator::new(4096);
        assert_eq!(v.sendmsg(&MsgHdr::plain(&[1, 2, 3])), Ok(6));
        assert_eq!(v.sendmsg(&MsgHdr::plain(&[])), Err(E_EINVAL));
    }

    #[test]
    fn checks_green() {
        let s = run_unx10872_checks();
        assert!(s.all_passed(), "UNX-F10872 判据全绿");
    }
}
