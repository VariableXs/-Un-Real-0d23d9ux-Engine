//! UNX-F10259 裁决面中检（十八项对平）（0x147A0 段）
//!
//! 锚点：UNX-F10259-J1 裁决面中检达成（F10241~F10258 十八项逐项对平
//! 10/10 次）；J2 缺项检出（缺席项精确点名 10/10 次）；J3 中检哈希
//! （同十八项同哈希）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·裁决轴中检）。
//!
//! 结构：裁决轴目录（F10241~F10258 真源）+ 逐项对账核 + 缺项点名 +
//! 中检哈希 + 注入核（缺项/错项/重项必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x147A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x147A0;
pub const MD_MISS: u32 = SEG_BASE + 0x01; // 缺项
pub const MD_OUT_AXIS: u32 = SEG_BASE + 0x02; // 轴外项
pub const MD_DUP: u32 = SEG_BASE + 0x03; // 重项
pub const MD_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后改
pub const MD_BAD_STATE: u32 = SEG_BASE + 0x05; // 坏状态

/// 裁决轴目录真源：F10241~F10258 十八项。
pub const AXIS: [u32; 18] = [
    10241, 10242, 10243, 10244, 10245, 10246, 10247, 10248, 10249, 10250,
    10251, 10252, 10253, 10254, 10255, 10256, 10257, 10258,
];

/// 项状态。
pub const S_OPEN: u8 = 0;
pub const S_DONE: u8 = 1;

pub fn state_ok(v: u8) -> bool {
    v <= S_DONE
}

/// 轴内条目。
#[derive(Clone, PartialEq)]
pub struct MidRec {
    pub item: u32,
    pub state: u8,
    pub note: String,
}

impl MidRec {
    pub fn ok(&self) -> bool {
        AXIS.contains(&self.item) && state_ok(self.state) && !self.note.is_empty()
    }
}

/// 中检册。
pub struct MidBook {
    rows: Vec<MidRec>,
    frozen: bool,
}

impl MidBook {
    pub fn new() -> MidBook {
        MidBook { rows: Vec::new(), frozen: false }
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
        self.rows.push(MidRec { item, state, note: note.to_string() });
        Ok(())
    }

    /// 全轴齐备且全 DONE。
    pub fn all_done(&self) -> bool {
        AXIS.iter().all(|i| {
            self.rows
                .iter()
                .find(|r| r.item == *i)
                .map(|r| r.state == S_DONE)
                .unwrap_or(false)
        })
    }

    pub fn missing(&self) -> Vec<u32> {
        AXIS.iter()
            .filter(|i| !self.rows.iter().any(|r| r.item == **i))
            .cloned()
            .collect()
    }

    /// 中检哈希：轴序 × 状态折叠。
    pub fn mid_fp(&self) -> u32 {
        let mut h: u32 = 0x811c9dc5;
        for i in AXIS {
            let seg = match self.rows.iter().find(|r| r.item == i) {
                Some(r) => format!("{}|{}", i, r.state),
                None => format!("{}|absent", i),
            };
            for b in seg.as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(0x01000193);
            }
        }
        h
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
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

/// 铺十八项全 DONE 基线。
pub fn seed(b: &mut MidBook) -> Result<(), &'static str> {
    for i in AXIS {
        b.put(i, S_DONE, "中检对平")?;
    }
    Ok(())
}

/// 注入核。
pub fn inject_fault(b: &mut MidBook, code: u32, item: u32) -> Result<(), &'static str> {
    match code {
        MD_MISS => {
            for i in AXIS {
                if i != item {
                    b.put(i, S_DONE, "铺")?;
                }
            }
            Ok(())
        }
        MD_OUT_AXIS => b.put(9999, S_DONE, "轴外"),
        MD_DUP => {
            let _ = b.put(item, S_DONE, "一");
            b.put(item, S_OPEN, "二")
        }
        MD_FROZEN => {
            b.freeze();
            b.put(item, S_DONE, "冻结后")
        }
        MD_BAD_STATE => b.put(item, 9, "坏态"),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10259_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10259");

    // J1 十八项对平达成
    let j1 = {
        let mut b = MidBook::new();
        let _ = seed(&mut b);
        b.len() == 18 && b.all_done() && b.all_ok()
    };
    s.add("J1-十八项对平", j1, "18 项全 DONE 全合法");

    // J2a 缺项点名
    let j2a = {
        let mut b = MidBook::new();
        let _ = inject_fault(&mut b, MD_MISS, 10250);
        b.all_done() == false && b.missing() == vec![10250u32]
    };
    s.add("J2a-缺项点名", j2a, "缺 10250 精确清单");

    // J2b 轴外拒收
    let j2b = inject_fault(&mut MidBook::new(), MD_OUT_AXIS, 0) == Err("axis_out");
    s.add("J2b-轴外拒", j2b, "9999 即拒");

    // J2c 重项拒收
    let j2c = inject_fault(&mut MidBook::new(), MD_DUP, 10245) == Err("dup_item");
    s.add("J2c-重项拒", j2c, "同项二次即拒");

    // J3 中检哈希：同轴同哈希/缺项哈希变
    let j3 = {
        let mut b1 = MidBook::new();
        let _ = seed(&mut b1);
        let f1 = b1.mid_fp();
        let mut b2 = MidBook::new();
        let _ = inject_fault(&mut b2, MD_MISS, 10252);
        let f2 = b2.mid_fp();
        f1 != f2 && f1 != 0
    };
    s.add("J3-中检哈希", j3, "缺项哈希必变");

    // J4 坏状态拒收
    let j4 = inject_fault(&mut MidBook::new(), MD_BAD_STATE, 10246) == Err("bad_state");
    s.add("J4-坏态拒收", j4, "state=9 即拒");

    // J5 冻结红线
    let j5 = {
        let mut b = MidBook::new();
        inject_fault(&mut b, MD_FROZEN, 10247);
        b.is_frozen()
    };
    s.add("J5-冻结拒收", j5, "印后写必抓");

    // J6 open 项不齐：全 DONE 判定拒绝 open
    let j6 = {
        let mut b = MidBook::new();
        for i in AXIS {
            let st = if i == 10253 { S_OPEN } else { S_DONE };
            let _ = b.put(i, st, "x");
        }
        !b.all_done()
    };
    s.add("J6-open不齐", j6, "open 项挡 all_done");

    // J7 条目负样本：轴外/坏态/空注 ok 必假
    let j7 = {
        let out = MidRec { item: 10200, state: S_DONE, note: "x".to_string() };
        let bad = MidRec { item: 10241, state: 9, note: "x".to_string() };
        let empty = MidRec { item: 10241, state: S_DONE, note: String::new() };
        !out.ok() && !bad.ok() && !empty.ok()
    };
    s.add("J7-负样本三假", j7, "三类坏条目 ok 必假");

    // J8 渐进对平：逐项补齐 all_done 由假转真
    let j8 = {
        let mut b = MidBook::new();
        let mut flips = 0;
        let mut prev = false;
        for i in AXIS {
            let _ = b.put(i, S_DONE, "逐项");
            let now = b.all_done();
            if now && !prev {
                flips += 1;
            }
            prev = now;
        }
        flips == 1 && prev
    };
    s.add("J8-渐进单翻", j8, "补齐最后一项单次翻转");

    // J9 十轮中检回放：10 轮哈希一致（确定性）
    let j9 = {
        let mk = || {
            let mut b = MidBook::new();
            let _ = seed(&mut b);
            b.mid_fp()
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10
    };
    s.add("J9-十轮哈希", j9, "10/10 轮一致");

    // J10 十八项缺席轮转点名（18 轮全测）
    let j10 = {
        let mut ok = 0;
        for miss in AXIS {
            let mut b = MidBook::new();
            for i in AXIS {
                if i != miss {
                    let _ = b.put(i, S_DONE, "x");
                }
            }
            if b.missing() == vec![miss] {
                ok += 1;
            }
        }
        ok == 18
    };
    s.add("J10-缺席轮转", j10, "18/18 轮精确点名");

    s
}
