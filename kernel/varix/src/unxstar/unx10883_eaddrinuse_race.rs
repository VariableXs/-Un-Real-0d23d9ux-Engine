//! UNX-F10883 EADDRINUSE 竞态矩阵（bind 与 unlink 并发）（0x1c6e0 段）
//! 锚点：UNX-F10883-J1 EADDRINUSE 竞态（bind/unlink 并发三序判定）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c6e0;
pub const E_EADDRINUSE: u32 = SEG_BASE + 1;
pub const E_ENOENT: u32 = SEG_BASE + 2;
pub const E_EACCES: u32 = SEG_BASE + 3;
pub const E_EINVAL: u32 = SEG_BASE + 4;
pub const E_OK: u32 = SEG_BASE + 5;
pub const E_SEALED: u32 = SEG_BASE + 6;

/// 地址文件面：路径 → 占用者（内存账，不落盘）
pub struct AddrFile {
    bound: Vec<(String, u32)>,
    unlinked: Vec<String>,
    sealed: bool,
}

impl AddrFile {
    pub fn new() -> Self {
        AddrFile { bound: Vec::new(), unlinked: Vec::new(), sealed: false }
    }

    pub fn bound_count(&self) -> usize {
        self.bound.len()
    }

    pub fn holder_of(&self, path: &str) -> Option<u32> {
        let mut i = 0;
        while i < self.bound.len() {
            if self.bound[i].0 == path {
                return Some(self.bound[i].1);
            }
            i += 1;
        }
        None
    }

    /// bind：路径已在内存账 → EADDRINUSE；曾 unlink 且无新占用 → 可重绑
    pub fn bind(&mut self, path: &str, pid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if path.is_empty() {
            return Err(E_EINVAL);
        }
        if self.holder_of(path).is_some() {
            return Err(E_EADDRINUSE);
        }
        self.bound.push((path.to_string(), pid));
        Ok(())
    }

    /// unlink：先摘内存账再记 unlink 面
    pub fn unlink(&mut self, path: &str, pid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(E_SEALED);
        }
        let holder = match self.holder_of(path) {
            None => return Err(E_ENOENT),
            Some(h) => h,
        };
        if holder != pid {
            return Err(E_EACCES);
        }
        let mut i = 0;
        while i < self.bound.len() {
            if self.bound[i].0 == path {
                self.bound.remove(i);
                break;
            }
            i += 1;
        }
        self.unlinked.push(path.to_string());
        Ok(())
    }

    pub fn unlinked_times(&self, path: &str) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.unlinked.len() {
            if self.unlinked[i] == path {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 竞态序判定：bind 与 unlink 并发时的三种交错结果
    /// Order A: bind→unlink→bind' → 成功
    /// Order B: bind→bind'（无 unlink）→ EADDRINUSE
    /// Order C: unlink→bind' → 成功（先释放后占）
    pub fn race_verdict(order: usize, path: &str, p1: u32, p2: u32) -> Result<&'static str, u32> {
        let mut a = AddrFile::new();
        match order {
            0 => {
                a.bind(path, p1)?;
                a.unlink(path, p1)?;
                a.bind(path, p2)?;
                Ok("rebound")
            }
            1 => {
                a.bind(path, p1)?;
                a.bind(path, p2)?;
                Ok("inuse")
            }
            _ => {
                a.bind(path, p1)?;
                a.unlink(path, p1)?;
                a.bind(path, p2)?;
                a.unlink(path, p2)?;
                Ok("released")
            }
        }
    }
}

pub fn run_unx10883_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10883");
    s.add("J1-段锚", SEG_BASE == 0x1c6e0, "EADDRINUSE 竞态段基址 0x1c6e0");
    s.add("J2-顺序 A 重绑成", {
        AddrFile::race_verdict(0, "/tmp/s.sock", 1, 2) == Ok("rebound")
    }, "bind→unlink→bind' rebound");
    s.add("J3-顺序 B 占用拒", {
        AddrFile::race_verdict(1, "/tmp/s.sock", 1, 2) == Err(E_EADDRINUSE)
    }, "双 bind 无 unlink E_EADDRINUSE");
    s.add("J4-顺序 C 释放成", {
        AddrFile::race_verdict(2, "/tmp/s.sock", 1, 2) == Ok("released")
    }, "再绑再解 released");
    s.add("J5-重绑可成", {
        let mut a = AddrFile::new();
        a.bind("/s", 1).unwrap();
        a.unlink("/s", 1).unwrap();
        a.bind("/s", 2).is_ok() && a.holder_of("/s") == Some(2)
    }, "释放后新主 2");
    s.add("J6-占用态 holder", {
        let mut a = AddrFile::new();
        a.bind("/s", 9).unwrap();
        a.holder_of("/s") == Some(9)
    }, "holder 9");
    s.add("J7-未绑 unlink 拒", {
        let mut a = AddrFile::new();
        a.unlink("/ghost", 1) == Err(E_ENOENT)
    }, "未绑 E_ENOENT");
    s.add("J8-异主 unlink 拒", {
        let mut a = AddrFile::new();
        a.bind("/s", 1).unwrap();
        a.unlink("/s", 2) == Err(E_EACCES)
    }, "异主 E_EACCES");
    s.add("J9-空路径拒", {
        let mut a = AddrFile::new();
        a.bind("", 1) == Err(E_EINVAL)
    }, "空路径 E_EINVAL");
    s.add("J10-同主 unlink 成", {
        let mut a = AddrFile::new();
        a.bind("/s", 1).unwrap();
        a.unlink("/s", 1).is_ok() && a.bound_count() == 0
    }, "同主解绑册空");
    s.add("J11-unlink 计数", {
        let mut a = AddrFile::new();
        a.bind("/s", 1).unwrap();
        a.unlink("/s", 1).unwrap();
        a.bind("/s", 1).unwrap();
        a.unlink("/s", 1).unwrap();
        a.unlinked_times("/s") == 2
    }, "两次 unlink 计 2");
    s.add("J12-异路径互不扰", {
        let mut a = AddrFile::new();
        a.bind("/a", 1).unwrap();
        a.bind("/b", 1).unwrap();
        a.unlink("/a", 1).unwrap();
        a.holder_of("/b") == Some(1) && a.holder_of("/a").is_none()
    }, "/b 存 /a 摘");
    s.add("J13-占用重绑拒 holder 不变", {
        let mut a = AddrFile::new();
        a.bind("/s", 1).unwrap();
        a.bind("/s", 2).unwrap_err() == E_EADDRINUSE;
        a.holder_of("/s") == Some(1)
    }, "拒绑后原主 1 不变");
    s.add("J14-多路并存", {
        let mut a = AddrFile::new();
        a.bind("/x", 1).unwrap();
        a.bind("/y", 2).unwrap();
        a.bind("/z", 3).unwrap();
        a.bound_count() == 3
    }, "三路并存计 3");
    s.add("J15-释放后零态", {
        let mut a = AddrFile::new();
        a.bind("/s", 1).unwrap();
        a.unlink("/s", 1).unwrap();
        a.holder_of("/s").is_none() && a.bound_count() == 0
    }, "释放后册空");
    s.add_dyn("J16-竞态后 unlink 计数", {
        let mut a = AddrFile::new();
        a.bind("/s", 1).unwrap();
        a.unlink("/s", 1).unwrap();
        a.bind("/s", 2).unwrap();
        let h = a.holder_of("/s");
        h == Some(2) && a.unlinked_times("/s") == 1
    }, "竞态后 holder Some(2) unlink 计 1");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_addrinuse_race() {
        let mut a = AddrFile::new();
        a.bind("/s", 1).unwrap();
        assert_eq!(a.bind("/s", 2), Err(E_EADDRINUSE));
        a.unlink("/s", 1).unwrap();
        assert!(a.bind("/s", 2).is_ok());
    }

    #[test]
    fn checks_green() {
        let s = run_unx10883_checks();
        assert!(s.all_passed(), "UNX-F10883 判据全绿");
    }
}
