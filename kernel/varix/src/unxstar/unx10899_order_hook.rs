//! UNX-F10899 错误二段判定序挂接核销（F10879 序表）（0x1c8e0 段）
//! 锚点：UNX-F10899-J1 二段判定序挂接核销（序表挂接+核销账闭环）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c8e0;
pub const EV_NOT_HOOKED: u32 = SEG_BASE + 1;
pub const EV_ORDER_DRIFT: u32 = SEG_BASE + 2;
pub const EV_TWICE: u32 = SEG_BASE + 3;
pub const EV_SEALED: u32 = SEG_BASE + 4;
pub const EV_UNKNOWN: u32 = SEG_BASE + 5;
pub const EV_NOT_CLEARED: u32 = SEG_BASE + 6;

pub const HOOK_ANCHOR: usize = 12;

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

/// 十二级序表（与 F10879 冻结件同源）
pub fn hook_table() -> [&'static str; HOOK_ANCHOR] {
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

/// 挂接核销器：逐级挂接 → 全挂核销 → 单值闭环
pub struct OrderHook {
    hooked: Vec<&'static str>,
    cleared: Vec<&'static str>,
    sealed: bool,
    stamp: Option<u32>,
}

impl OrderHook {
    pub fn new() -> Self {
        OrderHook { hooked: Vec::new(), cleared: Vec::new(), sealed: false, stamp: None }
    }

    /// 挂接一级：须按序表次序
    pub fn hook(&mut self, name: &'static str) -> Result<(), u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if self.hooked.len() >= HOOK_ANCHOR {
            return Err(EV_ORDER_DRIFT);
        }
        let t = hook_table();
        let expect = t[self.hooked.len()];
        if name != expect {
            return Err(EV_ORDER_DRIFT);
        }
        let mut i = 0;
        while i < self.hooked.len() {
            if self.hooked[i] == name {
                return Err(EV_TWICE);
            }
            i += 1;
        }
        self.hooked.push(name);
        Ok(())
    }

    pub fn hooked_count(&self) -> usize {
        self.hooked.len()
    }

    /// 核销一级：须先挂接
    pub fn clear(&mut self, name: &'static str) -> Result<(), u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        let mut hit = false;
        let mut i = 0;
        while i < self.hooked.len() {
            if self.hooked[i] == name {
                hit = true;
                break;
            }
            i += 1;
        }
        if !hit {
            return Err(EV_NOT_HOOKED);
        }
        let mut j = 0;
        while j < self.cleared.len() {
            if self.cleared[j] == name {
                return Err(EV_TWICE);
            }
            j += 1;
        }
        self.cleared.push(name);
        Ok(())
    }

    pub fn cleared_count(&self) -> usize {
        self.cleared.len()
    }

    /// 全挂全核 → 印章闭环
    pub fn certify(&mut self) -> Result<u32, u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if self.hooked.len() != HOOK_ANCHOR {
            return Err(EV_NOT_HOOKED);
        }
        if self.cleared.len() != HOOK_ANCHOR {
            return Err(EV_NOT_CLEARED);
        }
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0;
        while i < self.cleared.len() {
            let b = self.cleared[i].as_bytes();
            let mut k = 0;
            while k < b.len() {
                buf.push(b[k]);
                k += 1;
            }
            i += 1;
        }
        let st = fnv(&buf);
        self.stamp = Some(st);
        self.sealed = true;
        Ok(st)
    }

    pub fn stamp_of(&self) -> Option<u32> {
        self.stamp
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }
}

pub fn run_unx10899_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10899");
    s.add("J1-段锚", SEG_BASE == 0x1c8e0, "二段核销段基址 0x1c8e0");
    s.add("J2-级数锚", HOOK_ANCHOR == 12, "十二级锚");
    s.add("J3-序表与 F10879 同源", {
        let a = hook_table();
        a[0] == "EBADF" && a[11] == "EPIPE" && a.len() == 12
    }, "首尾级一致");
    s.add("J4-乱序挂接拒", {
        let mut h = OrderHook::new();
        h.hook("EAGAIN") == Err(EV_ORDER_DRIFT)
    }, "首位非 EBADF EV_ORDER_DRIFT");
    s.add("J5-依序挂接过", {
        let mut h = OrderHook::new();
        let t = hook_table();
        let mut ok = true;
        let mut n = 0;
        while n < 6 {
            ok = ok && h.hook(t[n]).is_ok();
            n += 1;
        }
        ok && h.hooked_count() == 6
    }, "前六级依序挂 6");
    s.add("J6-重复挂接拒", {
        let mut h = OrderHook::new();
        h.hook("EBADF").unwrap();
        h.hook("EBADF") == Err(EV_ORDER_DRIFT)
    }, "二次挂首级位错拒");
    s.add("J7-未挂核销拒", {
        let mut h = OrderHook::new();
        h.clear("EPIPE") == Err(EV_NOT_HOOKED)
    }, "未挂核销 EV_NOT_HOOKED");
    s.add("J8-挂后核销过", {
        let mut h = OrderHook::new();
        h.hook("EBADF").unwrap();
        h.clear("EBADF").is_ok() && h.cleared_count() == 1
    }, "挂一核一");
    s.add("J9-重复核销拒", {
        let mut h = OrderHook::new();
        h.hook("EBADF").unwrap();
        h.clear("EBADF").unwrap();
        h.clear("EBADF") == Err(EV_TWICE)
    }, "二次核销 EV_TWICE");
    s.add("J10-全挂全核闭环", {
        let mut h = OrderHook::new();
        let t = hook_table();
        let mut n = 0;
        while n < HOOK_ANCHOR {
            h.hook(t[n]).unwrap();
            n += 1;
        }
        n = 0;
        while n < HOOK_ANCHOR {
            h.clear(t[n]).unwrap();
            n += 1;
        }
        h.certify().is_ok()
    }, "十二挂十二核闭环 Ok");
    s.add("J11-缺核拒闭环", {
        let mut h = OrderHook::new();
        let t = hook_table();
        let mut n = 0;
        while n < HOOK_ANCHOR {
            h.hook(t[n]).unwrap();
            n += 1;
        }
        n = 0;
        while n < 11 {
            h.clear(t[n]).unwrap();
            n += 1;
        }
        h.certify() == Err(EV_NOT_CLEARED)
    }, "十一核 EV_NOT_CLEARED");
    s.add("J12-缺挂拒闭环", {
        let mut h = OrderHook::new();
        let t = hook_table();
        let mut n = 0;
        while n < 11 {
            h.hook(t[n]).unwrap();
            n += 1;
        }
        h.certify() == Err(EV_NOT_HOOKED)
    }, "十一挂 EV_NOT_HOOKED");
    s.add("J13-印章确定", {
        let mut a = OrderHook::new();
        let mut b = OrderHook::new();
        let t = hook_table();
        let mut n = 0;
        while n < HOOK_ANCHOR {
            a.hook(t[n]).unwrap();
            b.hook(t[n]).unwrap();
            n += 1;
        }
        n = 0;
        while n < HOOK_ANCHOR {
            a.clear(t[n]).unwrap();
            b.clear(t[n]).unwrap();
            n += 1;
        }
        a.certify().unwrap() == b.certify().unwrap()
    }, "同账两次闭环同印");
    s.add("J14-闭环封账", {
        let mut h = OrderHook::new();
        let t = hook_table();
        let mut n = 0;
        while n < HOOK_ANCHOR {
            h.hook(t[n]).unwrap();
            n += 1;
        }
        n = 0;
        while n < HOOK_ANCHOR {
            h.clear(t[n]).unwrap();
            n += 1;
        }
        h.certify().unwrap();
        h.hook("EBADF") == Err(EV_SEALED) && h.is_sealed()
    }, "闭环后挂接 EV_SEALED");
    s.add("J15-印章可回读", {
        let mut h = OrderHook::new();
        let t = hook_table();
        let mut n = 0;
        while n < HOOK_ANCHOR {
            h.hook(t[n]).unwrap();
            n += 1;
        }
        n = 0;
        while n < HOOK_ANCHOR {
            h.clear(t[n]).unwrap();
            n += 1;
        }
        let st = h.certify().unwrap();
        h.stamp_of() == Some(st)
    }, "印章回读全中");
    s.add_dyn("J16-挂核交错账", {
        let mut h = OrderHook::new();
        let t = hook_table();
        let r1 = h.clear(t[0]);
        h.hook(t[0]).unwrap();
        let r2 = h.clear(t[0]);
        r1.is_err() && r2.is_ok() && h.cleared_count() == 1
    }, "未挂核拒→挂→核过计 1");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_order_hook() {
        let mut h = OrderHook::new();
        let t = hook_table();
        for n in 0..HOOK_ANCHOR {
            h.hook(t[n]).unwrap();
        }
        for n in 0..HOOK_ANCHOR {
            h.clear(t[n]).unwrap();
        }
        assert!(h.certify().is_ok());
    }

    #[test]
    fn checks_green() {
        let s = run_unx10899_checks();
        assert!(s.all_passed(), "UNX-F10899 判据全绿");
    }
}
