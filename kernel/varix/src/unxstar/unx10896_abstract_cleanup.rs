//! UNX-F10896 抽象名进程退出自动清理矩阵（0x1c880 段）
//! 锚点：UNX-F10896-J1 抽象名退出清理（进程终结批量摘名+引用失活面）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c880;
pub const E_ENOENT: u32 = SEG_BASE + 1;
pub const E_EADDRINUSE: u32 = SEG_BASE + 2;
pub const E_EINVAL: u32 = SEG_BASE + 3;
pub const E_OK: u32 = SEG_BASE + 4;
pub const E_SEALED: u32 = SEG_BASE + 5;

/// 注册名：名字 + 属主进程
#[derive(Clone, Debug, PartialEq)]
pub struct RegName {
    pub name: String,
    pub owner: i32,
}

/// 抽象名登记器 + 退出清理面
pub struct ExitCleaner {
    reg: Vec<RegName>,
    reaped_pids: Vec<i32>,
    sealed: bool,
}

impl ExitCleaner {
    pub fn new() -> Self {
        ExitCleaner { reg: Vec::new(), reaped_pids: Vec::new(), sealed: false }
    }

    pub fn count(&self) -> usize {
        self.reg.len()
    }

    pub fn register(&mut self, name: &str, pid: i32) -> Result<(), u32> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if name.is_empty() {
            return Err(E_EINVAL);
        }
        let mut i = 0;
        while i < self.reg.len() {
            if self.reg[i].name == name {
                return Err(E_EADDRINUSE);
            }
            i += 1;
        }
        self.reg.push(RegName { name: name.to_string(), owner: pid });
        Ok(())
    }

    pub fn lookup(&self, name: &str) -> Option<i32> {
        let mut i = 0;
        while i < self.reg.len() {
            if self.reg[i].name == name {
                return Some(self.reg[i].owner);
            }
            i += 1;
        }
        None
    }

    /// 进程退出：摘其全部名并记 reaped
    pub fn reap(&mut self, pid: i32) -> Result<usize, u32> {
        if self.sealed {
            return Err(E_SEALED);
        }
        let mut removed = 0;
        let mut i = 0;
        while i < self.reg.len() {
            if self.reg[i].owner == pid {
                self.reg.remove(i);
                removed += 1;
            } else {
                i += 1;
            }
        }
        self.reaped_pids.push(pid);
        Ok(removed)
    }

    pub fn reaped_count(&self) -> usize {
        self.reaped_pids.len()
    }

    /// 二次 reap 允许（幂等，摘 0）——内核 wait 循环语义
    pub fn reap_idempotent(&mut self, pid: i32) -> Result<usize, u32> {
        self.reap(pid)
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 名失效探针：属主退出后 lookup 即空
    pub fn dead_probe(&self, name: &str, dead_pid: i32) -> bool {
        match self.lookup(name) {
            None => true,
            Some(o) => o != dead_pid,
        }
    }
}

pub fn run_unx10896_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10896");
    s.add("J1-段锚", SEG_BASE == 0x1c880, "退出清理段基址 0x1c880");
    s.add("J2-注册可查", {
        let mut c = ExitCleaner::new();
        c.register("svc", 100).unwrap();
        c.lookup("svc") == Some(100)
    }, "svc 属主 100");
    s.add("J3-重名拒", {
        let mut c = ExitCleaner::new();
        c.register("svc", 1).unwrap();
        c.register("svc", 2) == Err(E_EADDRINUSE)
    }, "重名 E_EADDRINUSE");
    s.add("J4-空名拒", {
        let mut c = ExitCleaner::new();
        c.register("", 1) == Err(E_EINVAL)
    }, "空名 E_EINVAL");
    s.add("J5-多名同主", {
        let mut c = ExitCleaner::new();
        c.register("a", 5).unwrap();
        c.register("b", 5).unwrap();
        c.register("c", 5).unwrap();
        c.count() == 3
    }, "三名同主计 3");
    s.add("J6-退出批量摘", {
        let mut c = ExitCleaner::new();
        c.register("a", 5).unwrap();
        c.register("b", 5).unwrap();
        c.register("c", 6).unwrap();
        c.reap(5) == Ok(2) && c.count() == 1 && c.lookup("a").is_none()
    }, "主 5 摘 2 余 1");
    s.add("J7-退出后名可复用", {
        let mut c = ExitCleaner::new();
        c.register("svc", 1).unwrap();
        c.reap(1).unwrap();
        c.register("svc", 2).is_ok() && c.lookup("svc") == Some(2)
    }, "摘后新主 2");
    s.add("J8-reap 记账", {
        let mut c = ExitCleaner::new();
        c.register("a", 1).unwrap();
        c.reap(1).unwrap();
        c.reaped_count() == 1
    }, "reap 计 1");
    s.add("J9-二次 reap 幂等", {
        let mut c = ExitCleaner::new();
        c.register("a", 1).unwrap();
        c.reap(1).unwrap();
        c.reap_idempotent(1) == Ok(0)
    }, "二次 reap 摘 0");
    s.add("J10-二次 reap 记账", {
        let mut c = ExitCleaner::new();
        c.register("a", 1).unwrap();
        c.reap(1).unwrap();
        c.reap_idempotent(1).unwrap();
        c.reaped_count() == 2
    }, "双 reap 计 2");
    s.add("J11-死主探针真", {
        let mut c = ExitCleaner::new();
        c.register("n", 9).unwrap();
        c.reap(9).unwrap();
        c.dead_probe("n", 9)
    }, "死后探针真");
    s.add("J12-活主探针假", {
        let mut c = ExitCleaner::new();
        c.register("n", 9).unwrap();
        !c.dead_probe("n", 9)
    }, "活主探针假");
    s.add("J13-异主名不受扰", {
        let mut c = ExitCleaner::new();
        c.register("a", 1).unwrap();
        c.register("b", 2).unwrap();
        c.reap(1).unwrap();
        c.lookup("b") == Some(2)
    }, "b 仍属主 2");
    s.add("J14-未注册查空", {
        let c = ExitCleaner::new();
        c.lookup("ghost").is_none()
    }, "未注册查空");
    s.add("J15-封账拒注册", {
        let mut c = ExitCleaner::new();
        c.seal();
        c.register("late", 1) == Err(E_SEALED)
    }, "封账 E_SEALED");
    s.add_dyn("J16-全清账", {
        let mut c = ExitCleaner::new();
        c.register("a", 1).unwrap();
        c.register("b", 1).unwrap();
        c.register("c", 2).unwrap();
        let r1 = c.reap(1);
        let r2 = c.reap(2);
        r1.is_ok() && r2.is_ok() && c.count() == 0
    }, "摘 2-摘 1-册空 0");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_exit_clean() {
        let mut c = ExitCleaner::new();
        c.register("a", 7).unwrap();
        c.register("b", 7).unwrap();
        assert_eq!(c.reap(7), Ok(2));
        assert!(c.lookup("a").is_none());
        assert!(c.register("a", 8).is_ok());
    }

    #[test]
    fn checks_green() {
        let s = run_unx10896_checks();
        assert!(s.all_passed(), "UNX-F10896 判据全绿");
    }
}
