//! UNX-F10240 归因轴闭账（F10221–F10239 总对账冻结）（0x11F40 段）
//!
//! 锚点：UNX-F10240-J1 归因轴闭账达成（F10221–F10239 十九项逐项对平
//! 且闭账哈希冻结 10/10 次）；J2 漏项检出（十九项缺席其一即拒闭账
//! 10/10 次检出）；J3 冻结不可改（闭账后改账拒收，哈希重验一致）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·归因轴收口）。
//!
//! 结构：归因轴目录（F10221~F10239 十九项号真源）+ 逐项对账核（轴内
//! 项状态集）+ 闭账哈希（全轴折叠冻结）+ 注入核（漏项/错项/冻结后改
//! 必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x11F40 独占 0x20）。
pub const SEG_BASE: u32 = 0x11F40;
pub const CL_MISS_ITEM: u32 = SEG_BASE + 0x01; // 漏项
pub const CL_BAD_ITEM: u32 = SEG_BASE + 0x02; // 错项（轴外）
pub const CL_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后改
pub const CL_FP_DRIFT: u32 = SEG_BASE + 0x04; // 哈希漂移
pub const CL_DUP_ITEM: u32 = SEG_BASE + 0x05; // 重项

/// 归因轴目录真源：F10221~F10239 十九项。
pub const AXIS: [u32; 19] = [
    10221, 10222, 10223, 10224, 10225, 10226, 10227, 10228, 10229, 10230, 10231, 10232,
    10233, 10234, 10235, 10236, 10237, 10238, 10239,
];

/// 项状态：在管/平账/闭账锚定。
pub const S_OPEN: u8 = 0;
pub const S_FLAT: u8 = 1;
pub const S_ANCHOR: u8 = 2;

pub fn state_ok(s: u8) -> bool {
    s <= S_ANCHOR
}

/// 轴内条目。
#[derive(Clone, PartialEq)]
pub struct AxisRec {
    pub item: u32,
    pub state: u8,
    pub note: String,
}

impl AxisRec {
    pub fn ok(&self) -> bool {
        AXIS.contains(&self.item) && state_ok(self.state) && !self.note.is_empty()
    }
}

/// 闭账册。
pub struct CloseBook {
    rows: Vec<AxisRec>,
    frozen: bool,
    frozen_fp: u32,
}

impl CloseBook {
    pub fn new() -> CloseBook {
        CloseBook { rows: Vec::new(), frozen: false, frozen_fp: 0 }
    }

    /// 登项：轴外/重项/冻结后写拒收。
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
        self.rows.push(AxisRec { item, state, note: note.to_string() });
        Ok(())
    }

    /// 全轴齐备：十九项全部登记且全平/锚定。
    pub fn full(&self) -> bool {
        AXIS.iter().all(|i| {
            self.rows
                .iter()
                .find(|r| r.item == *i)
                .map(|r| r.state >= S_FLAT)
                .unwrap_or(false)
        })
    }

    /// 漏项清单。
    pub fn missing(&self) -> Vec<u32> {
        AXIS.iter()
            .filter(|i| !self.rows.iter().any(|r| r.item == **i))
            .cloned()
            .collect()
    }

    /// 闭账哈希：轴序 × 状态 × 注释折叠（FNV-1a 32）。
    pub fn fold_fp(&self) -> u32 {
        let mut h: u32 = 0x811c9dc5;
        for i in AXIS {
            let b = match self.rows.iter().find(|r| r.item == i) {
                Some(r) => format!("{}|{}|{}", i, r.state, r.note),
                None => format!("{}|absent", i),
            };
            for byte in b.as_bytes() {
                h ^= *byte as u32;
                h = h.wrapping_mul(0x01000193);
            }
        }
        h
    }

    /// 闭账：全轴齐备才允许，冻结并落哈希。
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

    /// 冻结后哈希重验。
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
pub fn seed_full(b: &mut CloseBook) -> Result<(), &'static str> {
    for i in AXIS {
        b.put(i, S_FLAT, "轴内平账")?;
    }
    Ok(())
}

/// 注入核。
pub fn inject_fault(b: &mut CloseBook, code: u32, item: u32) -> Result<(), &'static str> {
    match code {
        CL_MISS_ITEM => {
            // 登 10221~10238（十八项），漏 10239
            for i in AXIS {
                if i != 10239 {
                    b.put(i, S_FLAT, "铺")?;
                }
            }
            Ok(())
        }
        CL_BAD_ITEM => b.put(item, S_FLAT, "轴外项"),
        CL_FROZEN => {
            let _ = seed_full(b);
            let _ = b.close();
            b.put(item, S_OPEN, "冻结后改")
        }
        CL_FP_DRIFT => {
            let _ = seed_full(b);
            b.put(item, S_FLAT, "哈希漂移样本") // 重项必拒
        }
        CL_DUP_ITEM => {
            let _ = b.put(item, S_FLAT, "一");
            b.put(item, S_ANCHOR, "二")
        }
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10240_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10240");

    // J1 十九项对平 + 闭账冻结 + 哈希重验一致
    let j1 = {
        let mut b = CloseBook::new();
        let _ = seed_full(&mut b);
        match b.close() {
            Ok(fp) => fp != 0 && b.is_frozen() && b.verify() == Ok(true),
            Err(_) => false,
        }
    };
    s.add("J1-闭账冻结", j1, "19 项平账闭账哈希重验一致");

    // J2a 漏项拒闭账
    let j2a = {
        let mut b = CloseBook::new();
        let _ = inject_fault(&mut b, CL_MISS_ITEM, 0);
        b.close() == Err("missing_items") && b.missing() == vec![10239u32]
    };
    s.add("J2a-漏项拒闭", j2a, "缺 10239 精确报漏");

    // J2b 轴外项拒收
    let j2b = {
        let mut b = CloseBook::new();
        inject_fault(&mut b, CL_BAD_ITEM, 9999) == Err("axis_out")
    };
    s.add("J2b-轴外拒收", j2b, "非十九项即拒");

    // J2c 冻结后改账拒收
    let j2c = {
        let mut b = CloseBook::new();
        inject_fault(&mut b, CL_FROZEN, 10239) == Err("frozen")
    };
    s.add("J2c-冻结后拒", j2c, "闭账后写必抓");

    // J3 重项拒收
    let j3 = inject_fault(&mut CloseBook::new(), CL_DUP_ITEM, 10225) == Err("dup_item");
    s.add("J3-重项拒收", j3, "同项二次登记即拒");

    // J4 哈希漂移：注释变更 → 哈希必变（同项数不同文本）
    let j4 = {
        let mut b1 = CloseBook::new();
        let _ = seed_full(&mut b1);
        let fp1 = b1.fold_fp();
        let mut b2 = CloseBook::new();
        let mut changed = true;
        for i in AXIS {
            let note = if i == 10230 && changed {
                changed = false;
                "注释不同"
            } else {
                "轴内平账"
            };
            let _ = b2.put(i, S_FLAT, note);
        }
        let fp2 = b2.fold_fp();
        fp1 != 0 && fp1 != fp2
    };
    s.add("J4-哈希漂移", j4, "文本变哈希必变");

    // J5 闭账前 verify 拒绝
    let j5 = {
        let mut b = CloseBook::new();
        let _ = seed_full(&mut b);
        b.verify() == Err("not_frozen")
    };
    s.add("J5-未冻verify拒", j5, "未闭账不得重验");

    // J6 二次闭账拒收
    let j6 = {
        let mut b = CloseBook::new();
        let _ = seed_full(&mut b);
        let _ = b.close();
        b.close() == Err("already")
    };
    s.add("J6-二次闭账拒", j6, "已冻结再闭即拒");

    // J7 状态合法性：open 项不得闭账
    let j7 = {
        let mut b = CloseBook::new();
        let _ = seed_full(&mut b);
        // 把 10231 状态换成 OPEN：重项拒收 → 先清后写不可行，直接验证 full 假
        let mut b2 = CloseBook::new();
        for i in AXIS {
            let st = if i == 10231 { S_OPEN } else { S_FLAT };
            let _ = b2.put(i, st, "x");
        }
        !b2.full()
    };
    s.add("J7-open不闭", j7, "open 项挡闭账");

    // J8 条目 ok 性质：轴外/坏态/空注全假
    let j8 = {
        let out = AxisRec { item: 10200, state: S_FLAT, note: "x".to_string() };
        let bad = AxisRec { item: 10221, state: 9, note: "x".to_string() };
        let empty = AxisRec { item: 10221, state: S_FLAT, note: String::new() };
        !out.ok() && !bad.ok() && !empty.ok()
    };
    s.add("J8-负样本三假", j8, "轴外/坏态/空注 ok 必假");

    // J9 十轮闭账回放：全平基线 10 轮闭账哈希各自稳定（确定性）
    let j9 = {
        let mut ok = 0;
        for _ in 0..10 {
            let mut b = CloseBook::new();
            let _ = seed_full(&mut b);
            let fp1 = b.close().unwrap();
            if b.verify() == Ok(true) && fp1 == b.fold_fp() {
                ok += 1;
            }
        }
        ok == 10
    };
    s.add("J9-十轮闭账", j9, "10/10 轮闭账重验一致");

    // J10 全册合法率：seed 后 all_ok 全真
    let j10 = {
        let mut b = CloseBook::new();
        let _ = seed_full(&mut b);
        b.len() == 19 && b.all_ok()
    };
    s.add("J10-全册合法", j10, "19 项全合法");

    s
}
