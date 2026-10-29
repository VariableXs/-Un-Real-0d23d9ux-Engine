//! UNX-F10893 地址文件 unlink 竞态错误矩阵（连接中 unlink）（0x1c820 段）
//! 锚点：UNX-F10893-J1 地址文件 unlink 竞态（连接中摘除/新 bind 覆盖面）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c820;
pub const E_ENOENT: u32 = SEG_BASE + 1;
pub const E_EADDRINUSE: u32 = SEG_BASE + 2;
pub const E_ECONNREFUSED: u32 = SEG_BASE + 3;
pub const E_EINVAL: u32 = SEG_BASE + 4;
pub const E_OK: u32 = SEG_BASE + 5;
pub const E_SEALED: u32 = SEG_BASE + 6;

/// 地址文件态：存在/已摘 + 连接计数
pub struct AddrLife {
    exists: bool,
    holder: Option<u32>,
    conns: usize,
    sealed: bool,
}

impl AddrLife {
    pub fn new(holder: u32) -> Self {
        AddrLife { exists: true, holder: Some(holder), conns: 0, sealed: false }
    }

    pub fn exists_of(&self) -> bool {
        self.exists
    }

    pub fn holder_of(&self) -> Option<u32> {
        self.holder
    }

    pub fn conns_of(&self) -> usize {
        self.conns
    }

    /// unlink：摘除路径但连接保持（内核语义：已连不断）
    pub fn unlink(&mut self, pid: u32) -> Result<bool, u32> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if !self.exists {
            return Err(E_ENOENT);
        }
        if self.holder != Some(pid) {
            return Err(E_EINVAL);
        }
        self.exists = false;
        Ok(self.conns > 0)
    }

    /// connect：路径在 → 连上并计数；路径摘 → ECONNREFUSED（新连）
    pub fn connect(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if !self.exists {
            return Err(E_ECONNREFUSED);
        }
        self.conns += 1;
        Ok(())
    }

    /// 新进程 bind 同路径：旧摘除后才可占
    pub fn rebind(&mut self, pid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.exists {
            return Err(E_EADDRINUSE);
        }
        self.exists = true;
        self.holder = Some(pid);
        self.conns = 0;
        Ok(())
    }

    /// 老连接面：摘除后既有连接读写不受扰
    pub fn legacy_alive(&self) -> bool {
        self.conns > 0
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

pub fn run_unx10893_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10893");
    s.add("J1-段锚", SEG_BASE == 0x1c820, "unlink 竞态段基址 0x1c820");
    s.add("J2-初态存在", {
        let a = AddrLife::new(1);
        a.exists_of() && a.holder_of() == Some(1) && a.conns_of() == 0
    }, "存在主 1 零连");
    s.add("J3-连上计数", {
        let mut a = AddrLife::new(1);
        a.connect().unwrap();
        a.conns_of() == 1
    }, "一连计 1");
    s.add("J4-主 unlink 成", {
        let mut a = AddrLife::new(1);
        a.unlink(1).is_ok() && !a.exists_of()
    }, "主摘路径假");
    s.add("J5-异主 unlink 拒", {
        let mut a = AddrLife::new(1);
        a.unlink(2) == Err(E_EINVAL)
    }, "异主 E_EINVAL");
    s.add("J6-二次 unlink 拒", {
        let mut a = AddrLife::new(1);
        a.unlink(1).unwrap();
        a.unlink(1) == Err(E_ENOENT)
    }, "已摘再摘 E_ENOENT");
    s.add("J7-摘后新连拒", {
        let mut a = AddrLife::new(1);
        a.unlink(1).unwrap();
        a.connect() == Err(E_ECONNREFUSED)
    }, "摘后 connect E_ECONNREFUSED");
    s.add("J8-摘后老连保持", {
        let mut a = AddrLife::new(1);
        a.connect().unwrap();
        a.unlink(1).unwrap();
        a.legacy_alive()
    }, "一老连摘后仍活");
    s.add("J9-摘后重绑成", {
        let mut a = AddrLife::new(1);
        a.unlink(1).unwrap();
        a.rebind(2).is_ok() && a.holder_of() == Some(2)
    }, "重绑新主 2");
    s.add("J10-未摘重绑拒", {
        let mut a = AddrLife::new(1);
        a.rebind(2) == Err(E_EADDRINUSE)
    }, "占用 E_EADDRINUSE");
    s.add("J11-重绑计数清零", {
        let mut a = AddrLife::new(1);
        a.connect().unwrap();
        a.unlink(1).unwrap();
        a.rebind(2).unwrap();
        a.conns_of() == 0 && a.exists_of()
    }, "重绑后连零存在真");
    s.add("J12-重绑后新连成", {
        let mut a = AddrLife::new(1);
        a.unlink(1).unwrap();
        a.rebind(2).unwrap();
        a.connect().is_ok() && a.conns_of() == 1
    }, "重绑后连上 1");
    s.add("J13-多连摘除账", {
        let mut a = AddrLife::new(1);
        a.connect().unwrap();
        a.connect().unwrap();
        a.connect().unwrap();
        let had = a.unlink(1).unwrap();
        had && a.conns_of() == 3
    }, "三连摘除返真连 3");
    s.add("J14-无连摘除返假", {
        let mut a = AddrLife::new(1);
        a.unlink(1).unwrap() == false
    }, "零连摘除返 false");
    s.add("J15-封账拒操作", {
        let mut a = AddrLife::new(1);
        a.seal();
        a.connect() == Err(E_SEALED) && a.unlink(1) == Err(E_SEALED)
    }, "封账双拒");
    s.add_dyn("J16-竞态全序账", {
        let mut a = AddrLife::new(1);
        let c1 = a.connect();
        let u = a.unlink(1);
        let c2 = a.connect();
        a.rebind(2).unwrap();
        let c3 = a.connect();
        c1.is_ok() && u.is_ok() && c2.is_err() && c3.is_ok()
    }, "连成-摘成-新连拒-重绑连成");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_unlink_race() {
        let mut a = AddrLife::new(1);
        a.connect().unwrap();
        a.unlink(1).unwrap();
        assert_eq!(a.connect(), Err(E_ECONNREFUSED));
        assert!(a.legacy_alive());
        a.rebind(2).unwrap();
        assert!(a.connect().is_ok());
    }

    #[test]
    fn checks_green() {
        let s = run_unx10893_checks();
        assert!(s.all_passed(), "UNX-F10893 判据全绿");
    }
}
