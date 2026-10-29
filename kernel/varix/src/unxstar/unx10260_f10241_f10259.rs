//! UNX-F10260 裁决轴闭账（F10241–F10259 对平+哈希冻结）（0x149C0 段）
//!
//! 锚点：UNX-F10260-J1 裁决轴闭账达成（十九项 F10241~F10259 逐项对平
//! 且闭账哈希冻结 10/10 次）；J2 漏项检出（缺席即拒闭账 10/10 次）；
//! J3 冻结不可改（闭账后改账拒收 + 哈希重验一致）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·裁决轴收口）。
//!
//! 结构：裁决轴目录（F10241~F10259 十九项真源）+ 对账核 + 闭账哈希
//! 冻结 + 注入核（漏项/错项/重项/冻结后改必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x149C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x149C0;
pub const CD_MISS: u32 = SEG_BASE + 0x01; // 漏项
pub const CD_OUT_AXIS: u32 = SEG_BASE + 0x02; // 轴外项
pub const CD_DUP: u32 = SEG_BASE + 0x03; // 重项
pub const CD_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后改
pub const CD_BAD_STATE: u32 = SEG_BASE + 0x05; // 坏状态

/// 裁决轴目录真源：F10241~F10259 十九项。
pub const AXIS: [u32; 19] = [
    10241, 10242, 10243, 10244, 10245, 10246, 10247, 10248, 10249, 10250,
    10251, 10252, 10253, 10254, 10255, 10256, 10257, 10258, 10259,
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
pub struct CloseRec {
    pub item: u32,
    pub state: u8,
    pub note: String,
}

impl CloseRec {
    pub fn ok(&self) -> bool {
        AXIS.contains(&self.item) && state_ok(self.state) && !self.note.is_empty()
    }
}

/// 闭账册。
pub struct AxisClose {
    rows: Vec<CloseRec>,
    frozen: bool,
    frozen_fp: u32,
}

impl AxisClose {
    pub fn new() -> AxisClose {
        AxisClose { rows: Vec::new(), frozen: false, frozen_fp: 0 }
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
        self.rows.push(CloseRec { item, state, note: note.to_string() });
        Ok(())
    }

    /// 全轴齐备且全平/锚定。
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

    /// 闭账哈希。
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

    /// 闭账冻结。
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
pub fn seed(b: &mut AxisClose) -> Result<(), &'static str> {
    for i in AXIS {
        b.put(i, S_FLAT, "裁决轴平账")?;
    }
    Ok(())
}

/// 注入核。
pub fn inject_fault(b: &mut AxisClose, code: u32, item: u32) -> Result<(), &'static str> {
    match code {
        CD_MISS => {
            for i in AXIS {
                if i != item {
                    b.put(i, S_FLAT, "铺")?;
                }
            }
            Ok(())
        }
        CD_OUT_AXIS => b.put(9999, S_FLAT, "轴外"),
        CD_DUP => {
            let _ = b.put(item, S_FLAT, "一");
            b.put(item, S_ANCHOR, "二")
        }
        CD_FROZEN => {
            let _ = seed(b);
            let _ = b.close();
            b.put(item, S_OPEN, "冻结后改")
        }
        CD_BAD_STATE => b.put(item, 9, "坏态"),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10260_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10260");

    // J1 十九项对平 + 闭账冻结 + 重验一致
    let j1 = {
        let mut b = AxisClose::new();
        let _ = seed(&mut b);
        match b.close() {
            Ok(fp) => fp != 0 && b.verify() == Ok(true) && b.is_frozen(),
            Err(_) => false,
        }
    };
    s.add("J1-闭账重验", j1, "19 项闭账哈希重验一致");

    // J2a 漏项拒闭账
    let j2a = {
        let mut b = AxisClose::new();
        let _ = inject_fault(&mut b, CD_MISS, 10255);
        b.close() == Err("missing_items") && b.missing() == vec![10255u32]
    };
    s.add("J2a-漏项拒闭", j2a, "缺 10255 精确拒");

    // J2b 轴外拒收
    let j2b = inject_fault(&mut AxisClose::new(), CD_OUT_AXIS, 0) == Err("axis_out");
    s.add("J2b-轴外拒", j2b, "9999 即拒");

    // J2c 重项拒收
    let j2c = inject_fault(&mut AxisClose::new(), CD_DUP, 10250) == Err("dup_item");
    s.add("J2c-重项拒", j2c, "同项二次即拒");

    // J3 冻结后改账拒收
    let j3 = inject_fault(&mut AxisClose::new(), CD_FROZEN, 10259) == Err("frozen");
    s.add("J3-冻结后拒", j3, "闭账后写必抓");

    // J4 哈希文本敏感：注释变哈希变
    let j4 = {
        let mut b1 = AxisClose::new();
        let _ = seed(&mut b1);
        let f1 = b1.fold_fp();
        let mut b2 = AxisClose::new();
        let mut once = true;
        for i in AXIS {
            let note = if i == 10248 && once {
                once = false;
                "不同注释"
            } else {
                "裁决轴平账"
            };
            let _ = b2.put(i, S_FLAT, note);
        }
        f1 != b2.fold_fp() && f1 != 0
    };
    s.add("J4-哈希敏感", j4, "文本变哈希必变");

    // J5 坏状态拒收
    let j5 = inject_fault(&mut AxisClose::new(), CD_BAD_STATE, 10249) == Err("bad_state");
    s.add("J5-坏态拒收", j5, "state=9 即拒");

    // J6 未冻 verify 拒
    let j6 = {
        let mut b = AxisClose::new();
        let _ = seed(&mut b);
        b.verify() == Err("not_frozen")
    };
    s.add("J6-未冻verify拒", j6, "未闭账不得重验");

    // J7 二次闭账拒
    let j7 = {
        let mut b = AxisClose::new();
        let _ = seed(&mut b);
        let _ = b.close();
        b.close() == Err("already")
    };
    s.add("J7-二次闭账拒", j7, "已冻结再闭即拒");

    // J8 open 项不闭
    let j8 = {
        let mut b = AxisClose::new();
        for i in AXIS {
            let st = if i == 10257 { S_OPEN } else { S_FLAT };
            let _ = b.put(i, st, "x");
        }
        !b.full()
    };
    s.add("J8-open不闭", j8, "open 项挡闭账");

    // J9 渐进补齐：逐项对平最后一项翻转
    let j9 = {
        let mut b = AxisClose::new();
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
    s.add("J9-渐进单翻", j9, "补齐末项单次翻转");

    // J10 十轮闭账回放：10 轮哈希稳定（确定性）
    let j10 = {
        let mk = || {
            let mut b = AxisClose::new();
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
    s.add("J10-十轮闭账", j10, "10/10 轮哈希重验一致");

    s
}
