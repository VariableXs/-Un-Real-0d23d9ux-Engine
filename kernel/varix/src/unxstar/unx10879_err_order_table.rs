//! UNX-F10879 unix socket 错误判定总序表（冻结件）（0x1c660 段）
//! 锚点：UNX-F10879-J1 错误判定总序表（十二级序+冻结校验和）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c660;
pub const EV_ORDER_DRIFT: u32 = SEG_BASE + 1;
pub const EV_SEALED: u32 = SEG_BASE + 2;
pub const EV_UNKNOWN_ERR: u32 = SEG_BASE + 3;
pub const EV_FROZEN: u32 = SEG_BASE + 4;
pub const EV_EMPTY: u32 = SEG_BASE + 5;
pub const EV_TWICE: u32 = SEG_BASE + 6;

pub const ORDER_ANCHOR: usize = 12;

const FNV_SEED: u32 = 0x811c_9dc5;

fn fnv(data: &[u8]) -> u32 {
    let mut h = FNV_SEED;
    let mut i = 0;
    while i < data.len() {
        h ^= data[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 十二级错误判定总序（冻结语义）：序号 → 错误名
pub fn order_table() -> [&'static str; ORDER_ANCHOR] {
    [
        "EBADF",
        "ENOTCONN",
        "EISCONN",
        "EALREADY",
        "EINPROGRESS",
        "ECONNREFUSED",
        "ECONNRESET",
        "EPROTOTYPE",
        "EADDRINUSE",
        "EMSGSIZE",
        "EAGAIN",
        "EPIPE",
    ]
}

/// 序表冻结器：登记序 → 校验和冻结 → 查序
pub struct OrderFreeze {
    entries: Vec<&'static str>,
    checksum: Option<u32>,
}

impl OrderFreeze {
    pub fn new() -> Self {
        OrderFreeze { entries: Vec::new(), checksum: None }
    }

    pub fn build(&mut self, names: &[&'static str]) -> Result<(), u32> {
        if self.checksum.is_some() {
            return Err(EV_FROZEN);
        }
        if names.is_empty() {
            return Err(EV_EMPTY);
        }
        if self.entries.len() + names.len() > ORDER_ANCHOR {
            return Err(EV_ORDER_DRIFT);
        }
        let mut i = 0;
        while i < names.len() {
            self.entries.push(names[i]);
            i += 1;
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 冻结：满十二级才可，冻结后校验和入档
    pub fn freeze(&mut self) -> Result<u32, u32> {
        if self.checksum.is_some() {
            return Err(EV_SEALED);
        }
        if self.entries.len() != ORDER_ANCHOR {
            return Err(EV_ORDER_DRIFT);
        }
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0;
        while i < self.entries.len() {
            let b = self.entries[i].as_bytes();
            let mut k = 0;
            while k < b.len() {
                buf.push(b[k]);
                k += 1;
            }
            i += 1;
        }
        let c = fnv(&buf);
        self.checksum = Some(c);
        Ok(c)
    }

    pub fn checksum_of(&self) -> Option<u32> {
        self.checksum
    }

    /// 查序：错误名 → 序号（1 起）
    pub fn rank_of(&self, name: &str) -> Result<usize, u32> {
        if self.checksum.is_none() {
            return Err(EV_FROZEN);
        }
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i] == name {
                return Ok(i + 1);
            }
            i += 1;
        }
        Err(EV_UNKNOWN_ERR)
    }

    /// 判定优先级：低序号先判
    pub fn first_of(&self, a: &str, b: &str) -> Result<&'static str, u32> {
        let ra = self.rank_of(a)?;
        let rb = self.rank_of(b)?;
        if ra <= rb {
            Ok(self.entries[ra - 1])
        } else {
            Ok(self.entries[rb - 1])
        }
    }
}

pub fn run_unx10879_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10879");
    s.add("J1-段锚", SEG_BASE == 0x1c660, "总序表段基址 0x1c660");
    s.add("J2-级数锚", ORDER_ANCHOR == 12, "十二级锚");
    s.add("J3-内置表满级", order_table().len() == 12, "内置序表 12 级");
    s.add("J4-首级 EBADF", order_table()[0] == "EBADF", "首级恒 EBADF");
    s.add("J5-尾级 EPIPE", order_table()[11] == "EPIPE", "尾级恒 EPIPE");
    s.add("J6-建表冻结", {
        let mut f = OrderFreeze::new();
        let t = order_table();
        f.build(&t).unwrap();
        f.freeze().is_ok()
    }, "十二级建表冻结成功");
    s.add("J7-校验和确定", {
        let mut a = OrderFreeze::new();
        let mut b = OrderFreeze::new();
        let t = order_table();
        a.build(&t).unwrap();
        b.build(&t).unwrap();
        a.freeze().unwrap() == b.freeze().unwrap()
    }, "同表两次冻结同和");
    s.add("J8-缺级拒冻", {
        let mut f = OrderFreeze::new();
        let t = order_table();
        f.build(&t[..11]).unwrap();
        f.freeze() == Err(EV_ORDER_DRIFT)
    }, "十一级 EV_ORDER_DRIFT");
    s.add("J9-查序中值", {
        let mut f = OrderFreeze::new();
        let t = order_table();
        f.build(&t).unwrap();
        f.freeze().unwrap();
        f.rank_of("EAGAIN") == Ok(11) && f.rank_of("EBADF") == Ok(1)
    }, "EAGAIN=11 EBADF=1");
    s.add("J10-查未知错", {
        let mut f = OrderFreeze::new();
        let t = order_table();
        f.build(&t).unwrap();
        f.freeze().unwrap();
        f.rank_of("ENOENT") == Err(EV_UNKNOWN_ERR)
    }, "表外错 EV_UNKNOWN_ERR");
    s.add("J11-未冻查序拒", {
        let mut f = OrderFreeze::new();
        let t = order_table();
        f.build(&t).unwrap();
        f.rank_of("EBADF") == Err(EV_FROZEN)
    }, "未冻结查序 EV_FROZEN");
    s.add("J12-优先级对", {
        let mut f = OrderFreeze::new();
        let t = order_table();
        f.build(&t).unwrap();
        f.freeze().unwrap();
        f.first_of("EAGAIN", "EBADF") == Ok("EBADF")
            && f.first_of("EPIPE", "EAGAIN") == Ok("EAGAIN")
    }, "低序号优先两对全中");
    s.add("J13-二次冻结拒", {
        let mut f = OrderFreeze::new();
        let t = order_table();
        f.build(&t).unwrap();
        f.freeze().unwrap();
        f.freeze() == Err(EV_SEALED)
    }, "二次冻结 EV_SEALED");
    s.add("J14-冻后拒建", {
        let mut f = OrderFreeze::new();
        let t = order_table();
        f.build(&t).unwrap();
        f.freeze().unwrap();
        f.build(&t) == Err(EV_FROZEN)
    }, "冻后建表 EV_FROZEN");
    s.add("J15-空建表拒", {
        let mut f = OrderFreeze::new();
        f.build(&[]) == Err(EV_EMPTY)
    }, "空表 EV_EMPTY");
    s.add("J16-十二级查序全账", {
        let mut f = OrderFreeze::new();
        let t = order_table();
        f.build(&t).unwrap();
        f.freeze().unwrap();
        let mut acc = 0;
        let mut n = 1;
        while n <= ORDER_ANCHOR {
            acc += f.rank_of(t[n - 1]).unwrap();
            n += 1;
        }
        acc == 78
    }, "1..=12 求和 78");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_order_freeze() {
        let mut f = OrderFreeze::new();
        let t = order_table();
        f.build(&t).unwrap();
        f.freeze().unwrap();
        assert_eq!(f.rank_of("EISCONN"), Ok(3));
        assert_eq!(f.first_of("EPIPE", "EBADF"), Ok("EBADF"));
    }

    #[test]
    fn checks_green() {
        let s = run_unx10879_checks();
        assert!(s.all_passed(), "UNX-F10879 判据全绿");
    }
}
