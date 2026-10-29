//! UNX-F10876 抽象命名空间错误矩阵（非法字符/占用）（0x1c600 段）
//! 锚点：UNX-F10876-J1 抽象名错误面（非法字符/重名占用/清理语义）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c600;
pub const E_EINVAL: u32 = SEG_BASE + 1;
pub const E_EADDRINUSE: u32 = SEG_BASE + 2;
pub const E_ENOENT: u32 = SEG_BASE + 3;
pub const E_EPERM: u32 = SEG_BASE + 4;
pub const E_OK: u32 = SEG_BASE + 5;
pub const E_SEALED: u32 = SEG_BASE + 6;

pub const NAME_MAX: usize = 108;

/// 抽象名登记册：不落文件系统，进程退出自动清理
pub struct AbstractNs {
    names: Vec<(String, i32)>,
    sealed: bool,
}

impl AbstractNs {
    pub fn new() -> Self {
        AbstractNs { names: Vec::new(), sealed: false }
    }

    /// 名合法校验：非空 / ≤108 / 不含 NUL / 不含路径分隔
    pub fn valid_name(name: &str) -> Result<(), u32> {
        if name.is_empty() {
            return Err(E_EINVAL);
        }
        if name.len() > NAME_MAX {
            return Err(E_EINVAL);
        }
        let b = name.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i] == 0 {
                return Err(E_EINVAL);
            }
            if b[i] == b'/' {
                return Err(E_EINVAL);
            }
            i += 1;
        }
        Ok(())
    }

    /// 绑定抽象名：owner 进程号记账
    pub fn bind(&mut self, name: &str, owner: i32) -> Result<(), u32> {
        if self.sealed {
            return Err(E_SEALED);
        }
        Self::valid_name(name)?;
        let mut i = 0;
        while i < self.names.len() {
            if self.names[i].0 == name {
                return Err(E_EADDRINUSE);
            }
            i += 1;
        }
        self.names.push((name.to_string(), owner));
        Ok(())
    }

    /// 进程退出自动清理：按 owner 批量摘除
    pub fn reap(&mut self, owner: i32) -> usize {
        let mut removed = 0;
        let mut i = 0;
        while i < self.names.len() {
            if self.names[i].1 == owner {
                self.names.remove(i);
                removed += 1;
            } else {
                i += 1;
            }
        }
        removed
    }

    pub fn lookup(&self, name: &str) -> Option<i32> {
        let mut i = 0;
        while i < self.names.len() {
            if self.names[i].0 == name {
                return Some(self.names[i].1);
            }
            i += 1;
        }
        None
    }

    pub fn count(&self) -> usize {
        self.names.len()
    }

    /// 显式解绑：非 owner 拒
    pub fn unbind(&mut self, name: &str, caller: i32) -> Result<(), u32> {
        if self.sealed {
            return Err(E_SEALED);
        }
        let mut i = 0;
        while i < self.names.len() {
            if self.names[i].0 == name {
                if self.names[i].1 != caller {
                    return Err(E_EPERM);
                }
                self.names.remove(i);
                return Ok(());
            }
            i += 1;
        }
        Err(E_ENOENT)
    }
}

pub fn run_unx10876_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10876");
    s.add("J1-段锚", SEG_BASE == 0x1c600, "抽象命名空间段基址 0x1c600");
    s.add("J2-名长锚", NAME_MAX == 108, "抽象名上限 108");
    s.add("J3-空名拒", AbstractNs::valid_name("") == Err(E_EINVAL), "空名 E_EINVAL");
    s.add("J4-超长拒", {
        let mut long = String::new();
        let mut n = 0;
        while n <= NAME_MAX {
            long.push('a');
            n += 1;
        }
        AbstractNs::valid_name(&long) == Err(E_EINVAL)
    }, "109 字节 E_EINVAL");
    s.add("J5-恰长过", {
        let mut long = String::new();
        let mut n = 0;
        while n < NAME_MAX {
            long.push('a');
            n += 1;
        }
        AbstractNs::valid_name(&long).is_ok()
    }, "108 字节 Ok");
    s.add("J6-路径分隔拒", AbstractNs::valid_name("a/b") == Err(E_EINVAL), "含斜杠 E_EINVAL");
    s.add("J7-合法名绑定", {
        let mut ns = AbstractNs::new();
        ns.bind("svc.api", 100).is_ok()
    }, "合法名绑 Ok");
    s.add("J8-重名占用", {
        let mut ns = AbstractNs::new();
        ns.bind("svc", 1).unwrap();
        ns.bind("svc", 2) == Err(E_EADDRINUSE)
    }, "重名 E_EADDRINUSE");
    s.add("J9-占用跨 owner", {
        let mut ns = AbstractNs::new();
        ns.bind("svc", 1).unwrap();
        ns.bind("svc", 999) == Err(E_EADDRINUSE)
    }, "异 owner 重名仍拒");
    s.add("J10-进程退出清理", {
        let mut ns = AbstractNs::new();
        ns.bind("a", 7).unwrap();
        ns.bind("b", 7).unwrap();
        ns.bind("c", 8).unwrap();
        ns.reap(7) == 2 && ns.count() == 1 && ns.lookup("a").is_none()
    }, "owner 7 退出摘 2 剩 1");
    s.add("J11-退出后名可复用", {
        let mut ns = AbstractNs::new();
        ns.bind("svc", 1).unwrap();
        ns.reap(1);
        ns.bind("svc", 2).is_ok()
    }, "清理后重绑 Ok");
    s.add("J12-查名", {
        let mut ns = AbstractNs::new();
        ns.bind("who", 42).unwrap();
        ns.lookup("who") == Some(42) && ns.lookup("nope").is_none()
    }, "查中 42 / 查空 None");
    s.add("J13-显式解绑 owner", {
        let mut ns = AbstractNs::new();
        ns.bind("svc", 5).unwrap();
        ns.unbind("svc", 5).is_ok() && ns.count() == 0
    }, "owner 解绑 Ok");
    s.add("J14-解绑非 owner 拒", {
        let mut ns = AbstractNs::new();
        ns.bind("svc", 5).unwrap();
        ns.unbind("svc", 6) == Err(E_EPERM)
    }, "异 owner 解绑 E_EPERM");
    s.add("J15-解绑未知名", {
        let mut ns = AbstractNs::new();
        ns.unbind("ghost", 1) == Err(E_ENOENT)
    }, "未知名 E_ENOENT");
    s.add_dyn("J16-非法名不入册", {
        let mut ns = AbstractNs::new();
        let r1 = ns.bind("", 1);
        let r2 = ns.bind("a/b", 1);
        r1.is_err() && r2.is_err() && ns.count() == 0
    }, "两非法名拒后册空 0");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_abstract_ns() {
        let mut ns = AbstractNs::new();
        ns.bind("app", 10).unwrap();
        assert_eq!(ns.bind("app", 11), Err(E_EADDRINUSE));
        assert_eq!(ns.reap(10), 1);
        assert!(ns.bind("app", 11).is_ok());
    }

    #[test]
    fn checks_green() {
        let s = run_unx10876_checks();
        assert!(s.all_passed(), "UNX-F10876 判据全绿");
    }
}
