//! UNX-F10280 压力轴闭账（F10261–F10279 对平+哈希冻结）（0x17440 段）
//!
//! 锚点：UNX-F10280-J1 压力轴闭账达成（十九项 F10261~F10279 逐项对平
//! 且闭账哈希冻结 10/10 次）；J2 漏项检出（缺席即拒闭账 10/10 次）；
//! J3 冻结不可改（闭账后改账拒收 + 哈希重验一致）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·压力轴收口）。
//!
//! 结构：压力轴目录（F10261~F10279 十九项真源）+ 对账核 + 闭账冻结 +
//! 注入核（漏项/轴外/重项/冻结后改必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x17440 独占 0x20）。
pub const SEG_BASE: u32 = 0x17440;
pub const PC_MISS: u32 = SEG_BASE + 0x01; // 漏项
pub const PC_OUT_AXIS: u32 = SEG_BASE + 0x02; // 轴外项
pub const PC_DUP: u32 = SEG_BASE + 0x03; // 重项
pub const PC_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后改
pub const PC_BAD_STATE: u32 = SEG_BASE + 0x05; // 坏状态

/// 压力轴目录真源：F10261~F10279 十九项。
pub const AXIS: [u32; 19] = [
    10261, 10262, 10263, 10264, 10265, 10266, 10267, 10268, 10269, 10270,
    10271, 10272, 10273, 10274, 10275, 10276, 10277, 10278, 10279,
];

/// 项状态。
pub const S_OPEN: u8 = 0;
pub const S_FLAT: u8 = 1;
pub const S_ANCHOR: u8 = 2;

pub fn state_ok(v: u8) -> bool {
    v <= S_ANCHOR
}

/// 轴内条目。
#[derive(Clone, PartialEq)]
pub struct PressCloseRec {
    pub item: u32,
    pub state: u8,
    pub note: String,
}

impl PressCloseRec {
    pub fn ok(&self) -> bool {
        AXIS.contains(&self.item) && state_ok(self.state) && !self.note.is_empty()
    }
}

/// 闭账册。
pub struct PressClose {
    rows: Vec<PressCloseRec>,
    frozen: bool,
    frozen_fp: u32,
}

impl PressClose {
    pub fn new() -> PressClose {
        PressClose { rows: Vec::new(), frozen: false, frozen_fp: 0 }
    }

    pub fn put(&mut self, item: u32, state: u8, note: &str) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if !AXIS.contains(&item) {
            return Err("axis_out");
        }
        if !state_ok(state) {
            return Err("bad_state");
        }
        if note.is_empty() {
            return Err("empty_note");
        }
        if self.rows.iter().any(|r| r.item == item) {
            return Err("dup_item");
        }
        self.rows.push(PressCloseRec { item, state, note: note.to_string() });
        Ok(())
    }

    pub fn full(&self) -> bool {
        AXIS.iter().all(|i| {
            self.rows
                .iter()
                .find(|r| r.item == *i)
                .map(|r| r.state >= S_FLAT)
                .unwrap_or(false)
        })
    }

    pub fn missing(&self) -> Vec<u32> {
        AXIS.iter()
            .filter(|i| !self.rows.iter().any(|r| r.item == **i))
            .cloned()
            .collect()
    }

    pub fn fold_fp(&self) -> u32 {
        let mut h: u32 = 0x811c9dc5;
        for i in AXIS {
            let seg = match self.rows.iter().find(|r| r.item == i) {
                Some(r) => format!("{}|{}|{}", i, r.state, r.note),
                None => format!("{}|absent", i),
            };
            for b in seg.as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(0x01000193);
            }
        }
        h
    }

    pub fn close(&mut self) -> Result<u32, &'static str> {
        if self.frozen {
            return Err("already");
        }
        if !self.full() {
            return Err("missing_items");
        }
        self.frozen_fp = self.fold_fp();
        self.frozen = true;
        Ok(self.frozen_fp)
    }

    pub fn verify(&self) -> Result<bool, &'static str> {
        if !self.frozen {
            return Err("not_frozen");
        }
        Ok(self.fold_fp() == self.frozen_fp)
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn all_ok(&self) -> bool {
        self.rows.iter().all(|r| r.ok())
    }
}

/// 铺十九项全平基线。
pub fn seed(b: &mut PressClose) -> Result<(), &'static str> {
    for i in AXIS {
        b.put(i, S_FLAT, "压力轴平账")?;
    }
    Ok(())
}

/// 注入核。
pub fn inject_fault(b: &mut PressClose, code: u32, item: u32) -> Result<(), &'static str> {
    match code {
        PC_MISS => {
            for i in AXIS {
                if i != item {
                    b.put(i, S_FLAT, "铺")?;
                }
            }
            Ok(())
        }
        PC_OUT_AXIS => b.put(9999, S_FLAT, "轴外"),
        PC_DUP => {
            let _ = b.put(item, S_FLAT, "一");
            b.put(item, S_ANCHOR, "二")
        }
        PC_FROZEN => {
            let _ = seed(b);
            let _ = b.close();
            b.put(item, S_OPEN, "冻结后改")
        }
        PC_BAD_STATE => b.put(item, 9, "坏态"),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10280_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10280");

    // J1 十九项闭账 + 重验一致
    let j1 = {
        let mut b = PressClose::new();
        let _ = seed(&mut b);
        match b.close() {
            Ok(fp) => fp != 0 && b.verify() == Ok(true),
            Err(_) => false,
        }
    };
    s.add("J1-闭账重验", j1, "19 项闭账哈希重验一致");

    // J2a 漏项拒闭
    let j2a = {
        let mut b = PressClose::new();
        let _ = inject_fault(&mut b, PC_MISS, 10275);
        b.close() == Err("missing_items") && b.missing() == vec![10275u32]
    };
    s.add("J2a-漏项拒闭", j2a, "缺 10275 精确拒");

    // J2b 轴外拒收
    let j2b = inject_fault(&mut PressClose::new(), PC_OUT_AXIS, 0) == Err("axis_out");
    s.add("J2b-轴外拒收", j2b, "9999 即拒");

    // J2c 重项拒收
    let j2c = inject_fault(&mut PressClose::new(), PC_DUP, 10270) == Err("dup_item");
    s.add("J2c-重项拒收", j2c, "同项二次即拒");

    // J3 冻结后改拒收
    let j3 = inject_fault(&mut PressClose::new(), PC_FROZEN, 10279) == Err("frozen");
    s.add("J3-冻结后拒", j3, "闭账后写必抓");

    // J4 哈希文本敏感
    let j4 = {
        let mut b1 = PressClose::new();
        let _ = seed(&mut b1);
        let f1 = b1.fold_fp();
        let mut b2 = PressClose::new();
        let mut once = true;
        for i in AXIS {
            let note = if i == 10268 && once {
                once = false;
                "异注"
            } else {
                "压力轴平账"
            };
            let _ = b2.put(i, S_FLAT, note);
        }
        f1 != b2.fold_fp() && f1 != 0
    };
    s.add("J4-哈希敏感", j4, "文本变哈希必变");

    // J5 坏状态拒收
    let j5 = inject_fault(&mut PressClose::new(), PC_BAD_STATE, 10269) == Err("bad_state");
    s.add("J5-坏态拒收", j5, "state=9 即拒");

    // J6 未冻 verify 拒
    let j6 = {
        let mut b = PressClose::new();
        let _ = seed(&mut b);
        b.verify() == Err("not_frozen")
    };
    s.add("J6-未冻verify拒", j6, "未闭账不得重验");

    // J7 二次闭账拒
    let j7 = {
        let mut b = PressClose::new();
        let _ = seed(&mut b);
        let _ = b.close();
        b.close() == Err("already")
    };
    s.add("J7-二次闭账拒", j7, "已冻结再闭即拒");

    // J8 open 项不闭
    let j8 = {
        let mut b = PressClose::new();
        for i in AXIS {
            let st = if i == 10277 { S_OPEN } else { S_FLAT };
            let _ = b.put(i, st, "x");
        }
        !b.full()
    };
    s.add("J8-open不闭", j8, "open 项挡闭账");

    // J9 渐进补齐单翻转
    let j9 = {
        let mut b = PressClose::new();
        let mut flips = 0;
        let mut prev = false;
        for i in AXIS {
            let _ = b.put(i, S_FLAT, "逐项");
            let now = b.full();
            if now && !prev {
                flips += 1;
            }
            prev = now;
        }
        flips == 1 && prev
    };
    s.add("J9-渐进单翻", j9, "末项补齐单次翻转");

    // J10 十轮闭账回放
    let j10 = {
        let mk = || {
            let mut b = PressClose::new();
            let _ = seed(&mut b);
            let fp = b.close().unwrap();
            (fp, b.verify())
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10 && base.1 == Ok(true)
    };
    s.add("J10-十轮闭账", j10, "10/10 轮重验一致");

    s
}
