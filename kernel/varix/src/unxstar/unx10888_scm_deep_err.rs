//! UNX-F10888 SCM_RIGHTS 深水错误矩阵（环/重复 fd）（0x1c780 段）
//! 锚点：UNX-F10888-J1 SCM_RIGHTS 深水面（自指环/重复传递/跨层嵌套）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c780;
pub const E_ELOOP: u32 = SEG_BASE + 1;
pub const E_EBADF: u32 = SEG_BASE + 2;
pub const E_EINVAL: u32 = SEG_BASE + 3;
pub const E_ENFILE: u32 = SEG_BASE + 4;
pub const E_OK: u32 = SEG_BASE + 5;
pub const E_SEALED: u32 = SEG_BASE + 6;

pub const NEST_MAX: usize = 4;

/// 深水传删面：嵌套深度 + 传参 fd 账
pub struct DeepRights {
    nest_depth: usize,
    in_flight: Vec<i32>,
    sealed: bool,
}

impl DeepRights {
    pub fn new() -> Self {
        DeepRights { nest_depth: 0, in_flight: Vec::new(), sealed: false }
    }

    pub fn depth(&self) -> usize {
        self.nest_depth
    }

    pub fn in_flight(&self) -> usize {
        self.in_flight.len()
    }

    /// 层内携带 fd：同报内重复 → EINVAL（内核语义：重复 fd 同一报拒绝）
    pub fn carry(&mut self, fds: &[i32]) -> Result<(), u32> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if fds.is_empty() {
            return Err(E_EINVAL);
        }
        let mut i = 0;
        while i < fds.len() {
            if fds[i] < 0 {
                return Err(E_EBADF);
            }
            let mut j = i + 1;
            while j < fds.len() {
                if fds[i] == fds[j] {
                    return Err(E_EINVAL);
                }
                j += 1;
            }
            i += 1;
        }
        let mut i = 0;
        while i < fds.len() {
            self.in_flight.push(fds[i]);
            i += 1;
        }
        Ok(())
    }

    /// 嵌套进层：超 NEST_MAX → ELOOP
    pub fn enter_nest(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.nest_depth >= NEST_MAX {
            return Err(E_ELOOP);
        }
        self.nest_depth += 1;
        Ok(())
    }

    pub fn exit_nest(&mut self) -> Result<(), u32> {
        if self.nest_depth == 0 {
            return Err(E_EINVAL);
        }
        self.nest_depth -= 1;
        Ok(())
    }

    /// 自指环检测：携带清单含当前嵌套层正在传递的 fd（记 0 号哨兵）→ ELOOP
    pub fn self_ref_probe(&self, fd: i32) -> bool {
        let mut i = 0;
        while i < self.in_flight.len() {
            if self.in_flight[i] == fd {
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn drain(&mut self) -> Result<usize, u32> {
        if self.in_flight.is_empty() {
            return Err(E_EINVAL);
        }
        let n = self.in_flight.len();
        self.in_flight.clear();
        self.sealed = true;
        Ok(n)
    }
}

pub fn run_unx10888_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10888");
    s.add("J1-段锚", SEG_BASE == 0x1c780, "SCM 深水段基址 0x1c780");
    s.add("J2-嵌套上限锚", NEST_MAX == 4, "嵌套上限 4 层");
    s.add("J3-同报重复 fd 拒", {
        let mut d = DeepRights::new();
        d.carry(&[3, 3]) == Err(E_EINVAL)
    }, "重复 3 E_EINVAL");
    s.add("J4-异 fd 携带过", {
        let mut d = DeepRights::new();
        d.carry(&[3, 4, 5]).is_ok() && d.in_flight() == 3
    }, "三异 fd 在途 3");
    s.add("J5-负 fd 拒", {
        let mut d = DeepRights::new();
        d.carry(&[-1]) == Err(E_EBADF)
    }, "负 fd E_EBADF");
    s.add("J6-空携带拒", {
        let mut d = DeepRights::new();
        d.carry(&[]) == Err(E_EINVAL)
    }, "空清单 E_EINVAL");
    s.add("J7-嵌套逐层过", {
        let mut d = DeepRights::new();
        d.enter_nest().unwrap();
        d.enter_nest().unwrap();
        d.enter_nest().unwrap();
        d.enter_nest().unwrap();
        d.depth() == 4
    }, "四层全入 depth 4");
    s.add("J8-超层 ELOOP", {
        let mut d = DeepRights::new();
        let mut n = 0;
        while n < NEST_MAX {
            d.enter_nest().unwrap();
            n += 1;
        }
        d.enter_nest() == Err(E_ELOOP)
    }, "第五层 E_ELOOP");
    s.add("J9-出层回退", {
        let mut d = DeepRights::new();
        d.enter_nest().unwrap();
        d.enter_nest().unwrap();
        d.exit_nest().unwrap();
        d.depth() == 1
    }, "两进一出 depth 1");
    s.add("J10-零层出层拒", {
        let mut d = DeepRights::new();
        d.exit_nest() == Err(E_EINVAL)
    }, "零层出 EV/E_EINVAL");
    s.add("J11-自指探测中", {
        let mut d = DeepRights::new();
        d.carry(&[7]).unwrap();
        d.self_ref_probe(7)
    }, "在途 fd 7 自指真");
    s.add("J12-自指探测假", {
        let mut d = DeepRights::new();
        d.carry(&[7]).unwrap();
        !d.self_ref_probe(9)
    }, "异 fd 9 自指假");
    s.add("J13-跨报重携放行", {
        let mut d = DeepRights::new();
        d.carry(&[7]).unwrap();
        d.carry(&[7]).is_ok() && d.in_flight() == 2
    }, "跨报同 fd 放行（环语义只在嵌套层）");
    s.add("J14-排空封账", {
        let mut d = DeepRights::new();
        d.carry(&[1, 2]).unwrap();
        d.drain() == Ok(2) && d.in_flight() == 0
    }, "排空 2 且清零");
    s.add("J15-空排拒", {
        let mut d = DeepRights::new();
        d.drain() == Err(E_EINVAL)
    }, "空排 E_EINVAL");
    s.add("J16-排空后携拒", {
        let mut d = DeepRights::new();
        d.carry(&[1]).unwrap();
        d.drain().unwrap();
        d.carry(&[2]) == Err(E_SEALED)
    }, "排空封账后携 EV/E_SEALED");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_deep_rights() {
        let mut d = DeepRights::new();
        assert!(d.carry(&[3, 4]).is_ok());
        assert_eq!(d.carry(&[4, 4]), Err(E_EINVAL));
        d.enter_nest().unwrap();
        assert_eq!(d.depth(), 1);
    }

    #[test]
    fn checks_green() {
        let s = run_unx10888_checks();
        assert!(s.all_passed(), "UNX-F10888 判据全绿");
    }
}
