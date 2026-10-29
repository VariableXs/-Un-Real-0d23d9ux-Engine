//! UNX-F10399 域闭账中检（十八项对平）（0x1bc60 段）
//! 锚点：UNX-F10399-J1 域闭账中检（十八项逐项对平+单值裁决）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1bc60;
pub const EV_NOT_EIGHT: u32 = SEG_BASE + 1;
pub const EV_SEALED: u32 = SEG_BASE + 2;
pub const EV_ITEM_RED: u32 = SEG_BASE + 3;
pub const EV_TWICE: u32 = SEG_BASE + 4;
pub const EV_OUT_OF_ORDER: u32 = SEG_BASE + 5;
pub const EV_EMPTY: u32 = SEG_BASE + 6;

pub const MID_ANCHOR: usize = 18;

/// 中检项：序号 1..=18 + 对平布尔
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MidItem {
    pub no: usize,
    pub balanced: bool,
}

/// 十八项中检册：逐项登记 → 全对平才出 PASS
pub struct CloseMidcheck {
    items: Vec<MidItem>,
    sealed: bool,
    passed: Option<bool>,
}

impl CloseMidcheck {
    pub fn new() -> Self {
        CloseMidcheck { items: Vec::new(), sealed: false, passed: None }
    }

    pub fn put(&mut self, item: MidItem) -> Result<(), u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if self.items.len() >= MID_ANCHOR {
            return Err(EV_NOT_EIGHT);
        }
        if item.no == 0 || item.no > MID_ANCHOR {
            return Err(EV_OUT_OF_ORDER);
        }
        if let Some(last) = self.items.last() {
            if item.no != last.no + 1 {
                return Err(EV_OUT_OF_ORDER);
            }
        } else if item.no != 1 {
            return Err(EV_OUT_OF_ORDER);
        }
        self.items.push(item);
        Ok(())
    }

    pub fn count(&self) -> usize {
        self.items.len()
    }

    pub fn red_count(&self) -> usize {
        self.items.iter().filter(|i| !i.balanced).count()
    }

    /// 中检执行：十八项齐才检，任一红即 FAIL 单值
    pub fn execute(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if self.items.is_empty() {
            self.sealed = true;
            return Err(EV_EMPTY);
        }
        if self.items.len() != MID_ANCHOR {
            self.sealed = true;
            return Err(EV_NOT_EIGHT);
        }
        if self.red_count() > 0 {
            self.sealed = true;
            self.passed = Some(false);
            return Err(EV_ITEM_RED);
        }
        self.sealed = true;
        self.passed = Some(true);
        Ok(())
    }

    pub fn result_of(&self) -> Option<bool> {
        self.passed
    }

    pub fn red_nos(&self) -> Vec<usize> {
        let mut v = Vec::new();
        for i in self.items.iter() {
            if !i.balanced {
                v.push(i.no);
            }
        }
        v
    }

    pub fn progress(&self) -> u64 {
        (self.items.len() as u64) * 10_000 / (MID_ANCHOR as u64)
    }
}

/// 十八项全对平构造
fn full_mid() -> CloseMidcheck {
    let mut m = CloseMidcheck::new();
    let mut n = 1;
    while n <= MID_ANCHOR {
        m.put(MidItem { no: n, balanced: true }).unwrap();
        n += 1;
    }
    m
}

pub fn run_unx10399_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10399");
    s.add("J1-段锚", SEG_BASE == 0x1bc60, "域闭账中检段基址 0x1bc60");
    s.add("J2-项数锚", MID_ANCHOR == 18, "十八项锚");
    s.add("J3-新册零项", CloseMidcheck::new().count() == 0, "新册 0/18");
    s.add("J4-首项必 1", {
        let mut m = CloseMidcheck::new();
        m.put(MidItem { no: 2, balanced: true }).unwrap_err() == EV_OUT_OF_ORDER
    }, "首项写 2 EV_OUT_OF_ORDER");
    s.add("J5-序号单调", {
        let mut m = CloseMidcheck::new();
        m.put(MidItem { no: 1, balanced: true }).unwrap();
        m.put(MidItem { no: 3, balanced: true }).unwrap_err() == EV_OUT_OF_ORDER
    }, "1 后写 3 EV_OUT_OF_ORDER");
    s.add("J6-序号越界", {
        let mut m = CloseMidcheck::new();
        m.put(MidItem { no: 0, balanced: true }).unwrap_err() == EV_OUT_OF_ORDER
            && m.put(MidItem { no: 19, balanced: true }).unwrap_err() == EV_OUT_OF_ORDER
    }, "序 0/19 拒收");
    s.add("J7-满册十八项", full_mid().count() == 18, "十八项齐入册");
    s.add("J8-满册中检过", full_mid().execute().is_ok(), "十八项全对平过检");
    s.add("J9-进度满格", {
        let m = full_mid();
        m.progress() == 10_000
    }, "进度万分位 10,000");
    s.add("J10-进度半格", {
        let mut m = CloseMidcheck::new();
        let mut n = 1;
        while n <= 9 {
            m.put(MidItem { no: n, balanced: true }).unwrap();
            n += 1;
        }
        m.progress() == 5_000
    }, "9/18 进度 5,000");
    s.add("J11-红项中检拒", {
        let mut m = CloseMidcheck::new();
        let mut n = 1;
        while n <= MID_ANCHOR {
            let bal = n != 7;
            m.put(MidItem { no: n, balanced: bal }).unwrap();
            n += 1;
        }
        m.execute().unwrap_err() == EV_ITEM_RED
    }, "第 7 项红 EV_ITEM_RED");
    s.add("J12-红号可枚举", {
        let mut m = CloseMidcheck::new();
        m.put(MidItem { no: 1, balanced: false }).unwrap();
        m.put(MidItem { no: 2, balanced: true }).unwrap();
        m.put(MidItem { no: 3, balanced: false }).unwrap();
        m.red_nos() == vec![1, 3] && m.red_count() == 2
    }, "红号 [1,3] 计 2");
    s.add("J13-缺项中检拒", {
        let mut m = CloseMidcheck::new();
        let mut n = 1;
        while n <= 17 {
            m.put(MidItem { no: n, balanced: true }).unwrap();
            n += 1;
        }
        m.execute().unwrap_err() == EV_NOT_EIGHT
    }, "十七项 EV_NOT_EIGHT");
    s.add("J14-空册中检拒", {
        let mut m = CloseMidcheck::new();
        m.execute().unwrap_err() == EV_EMPTY
    }, "空册 EV_EMPTY");
    s.add("J15-中检封账", {
        let mut m = full_mid();
        m.execute().unwrap();
        m.put(MidItem { no: 19, balanced: true }).unwrap_err() == EV_SEALED
    }, "过检后入项 EV_SEALED");
    s.add("J16-败检结果存档", {
        let mut m = CloseMidcheck::new();
        let mut n = 1;
        while n <= MID_ANCHOR {
            let bal = n == 18;
            m.put(MidItem { no: n, balanced: bal }).unwrap();
            n += 1;
        }
        m.execute().unwrap_err() == EV_ITEM_RED;
        m.result_of() == Some(false) && m.red_nos() == vec![18]
    }, "败检 false 且红号 [18]");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_mid_eighteen() {
        let mut m = full_mid();
        assert!(m.execute().is_ok());
        assert_eq!(m.result_of(), Some(true));
    }

    #[test]
    fn checks_green() {
        let s = run_unx10399_checks();
        assert!(s.all_passed(), "UNX-F10399 判据全绿");
    }
}
