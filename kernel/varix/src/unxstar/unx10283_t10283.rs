//! UNX-F10283 判据库移交清单（判据全量条目清点）（0x17AA0 段）
//!
//! 锚点：UNX-F10283-J1 移交清单达成（判据全量条目清点：条目数/分类
//! 分布/锚点齐备 10/10 次）；J2 清点异常检出（条目缺锚/分类越闭集/重
//! 条目 10/10 次检出）；J3 清单指纹（同清单同指纹）。
//! 域：CoRun·UNX-C3-B40（O3 对接面·移交清单）。
//!
//! 结构：清单册（判据条目五字段）+ 清点核（计数/分布/锚点齐备）+ 异
//! 常检出核 + 注入核（缺锚/越类/重条目必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x17AA0 独占 0x20）。
pub const SEG_BASE: u32 = 0x17AA0;
pub const HV_NO_ANCHOR: u32 = SEG_BASE + 0x01; // 缺锚
pub const HV_BAD_CLASS: u32 = SEG_BASE + 0x02; // 分类越闭集
pub const HV_DUP: u32 = SEG_BASE + 0x03; // 重条目
pub const HV_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后改
pub const HV_EMPTY: u32 = SEG_BASE + 0x05; // 空清单

/// 判据分类闭集。
pub const C_FUNC: u8 = 0; // 功能判据
pub const C_BOUND: u8 = 1; // 边界判据
pub const C_INJECT: u8 = 2; // 注入判据
pub const C_PERF: u8 = 3; // 性能判据
pub const CLASS_N: u8 = 4;

pub fn class_ok(c: u8) -> bool {
    c < CLASS_N
}

/// 判据条目。
#[derive(Clone, PartialEq)]
pub struct CkItem {
    pub ck: u32,
    pub class: u8,
    pub anchor: u32, // 判据锚点（SEG 段位）
    pub lines: u32,
    pub note: String,
}

impl CkItem {
    pub fn ok(&self) -> bool {
        self.ck != 0 && class_ok(self.class) && self.anchor != 0 && !self.note.is_empty()
    }
}

/// 移交清单册。
pub struct HandoverBook {
    rows: Vec<CkItem>,
    frozen: bool,
}

impl HandoverBook {
    pub fn new() -> HandoverBook {
        HandoverBook { rows: Vec::new(), frozen: false }
    }

    /// 登条目：重号/越类/缺锚/冻结拒收。
    pub fn add(&mut self, it: CkItem) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if it.ck == 0 {
            return Err("zero_ck");
        }
        if !class_ok(it.class) {
            return Err("bad_class");
        }
        if it.anchor == 0 {
            return Err("no_anchor");
        }
        if it.note.is_empty() {
            return Err("empty_note");
        }
        if self.rows.iter().any(|r| r.ck == it.ck) {
            return Err("dup_ck");
        }
        self.rows.push(it);
        Ok(())
    }

    /// 全量清点：总数 + 分类分布四桶。
    pub fn tally(&self) -> ([usize; CLASS_N as usize], usize) {
        let mut dist = [0usize; CLASS_N as usize];
        for r in &self.rows {
            dist[r.class as usize] += 1;
        }
        (dist, self.rows.len())
    }

    /// 清单齐备：四分类每类至少一条。
    pub fn classes_full(&self) -> bool {
        (0..CLASS_N).all(|c| self.rows.iter().any(|r| r.class == c))
    }

    /// 缺锚清单（防御性复扫）。
    pub fn anchorless(&self) -> Vec<u32> {
        self.rows.iter().filter(|r| r.anchor == 0).map(|r| r.ck).collect()
    }

    /// 清单指纹。
    pub fn fp(&self) -> u32 {
        let mut h: u32 = 0x811c9dc5;
        let mut sorted: Vec<&CkItem> = self.rows.iter().collect();
        sorted.sort_by_key(|r| r.ck);
        for r in sorted {
            for b in format!("{}|{}|{}|{}", r.ck, r.class, r.anchor, r.lines).as_bytes() {
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

/// 铺四分类基线（每类两条）。
pub fn seed(b: &mut HandoverBook) -> Result<(), &'static str> {
    let seeds: [(u32, u8, u32); 8] = [
        (1, C_FUNC, 0xFFA0), (2, C_FUNC, 0x10220),
        (3, C_BOUND, 0x11280), (4, C_BOUND, 0x114A0),
        (5, C_INJECT, 0x116C0), (6, C_INJECT, 0x118E0),
        (7, C_PERF, 0x11B00), (8, C_PERF, 0x11D20),
    ];
    for (ck, cls, seg) in seeds {
        b.add(CkItem { ck, class: cls, anchor: seg, lines: 320, note: "n".to_string() })?;
    }
    Ok(())
}

/// 注入核。
pub fn inject_fault(b: &mut HandoverBook, code: u32, ck: u32) -> Result<(), &'static str> {
    match code {
        HV_NO_ANCHOR => b.add(CkItem { ck, class: C_FUNC, anchor: 0, lines: 300, note: "缺锚".to_string() }),
        HV_BAD_CLASS => b.add(CkItem { ck, class: 9, anchor: 1, lines: 300, note: "越类".to_string() }),
        HV_DUP => {
            let _ = b.add(CkItem { ck, class: C_FUNC, anchor: 1, lines: 300, note: "一".to_string() });
            b.add(CkItem { ck, class: C_FUNC, anchor: 2, lines: 300, note: "二".to_string() })
        }
        HV_FROZEN => {
            b.freeze();
            b.add(CkItem { ck, class: C_FUNC, anchor: 1, lines: 300, note: "冻结后".to_string() })
        }
        HV_EMPTY => b.add(CkItem { ck, class: C_FUNC, anchor: 1, lines: 300, note: String::new() }),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10283_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10283");

    // J1 全量清点：八条四类分布均衡
    let j1 = {
        let mut b = HandoverBook::new();
        let _ = seed(&mut b);
        let (dist, total) = b.tally();
        total == 8 && dist == [2, 2, 2, 2] && b.classes_full()
    };
    s.add("J1-八条四类", j1, "清点分布均衡");

    // J2a 缺锚检出
    let j2a = {
        let mut b = HandoverBook::new();
        let _ = inject_fault(&mut b, HV_NO_ANCHOR, 20);
        b.anchorless().is_empty() // add 已拒 → 防御性复扫零
            && b.len() == 0
    };
    s.add("J2a-缺锚拒收", j2a, "缺锚条目入册即拒");

    // J2b 越类拒收
    let j2b = inject_fault(&mut HandoverBook::new(), HV_BAD_CLASS, 21) == Err("bad_class");
    s.add("J2b-越类拒收", j2b, "class=9 即拒");

    // J2c 重条目拒收
    let j2c = inject_fault(&mut HandoverBook::new(), HV_DUP, 22) == Err("dup_ck");
    s.add("J2c-重条目拒", j2c, "同号二次即拒");

    // J3 清单指纹：同清单同指纹/条目变必变
    let j3 = {
        let mk = |alt: bool| {
            let mut b = HandoverBook::new();
            let _ = seed(&mut b);
            if alt {
                let _ = b.add(CkItem { ck: 99, class: C_FUNC, anchor: 5, lines: 100, note: "x".to_string() });
            }
            b.fp()
        };
        let f1 = mk(false);
        let f2 = mk(false);
        let f3 = mk(true);
        f1 == f2 && f1 != f3 && f1 != 0
    };
    s.add("J3-清单指纹", j3, "加条指纹必变");

    // J4 缺锚防御复扫：合法清单零缺锚
    let j4 = {
        let mut b = HandoverBook::new();
        let _ = seed(&mut b);
        b.anchorless().is_empty() && b.all_ok()
    };
    s.add("J4-防御复扫", j4, "合法清单零缺锚");

    // J5 空注拒收
    let j5 = inject_fault(&mut HandoverBook::new(), HV_EMPTY, 23) == Err("empty_note");
    s.add("J5-空注拒收", j5, "空注即拒");

    // J6 注入五路
    let codes = [HV_NO_ANCHOR, HV_BAD_CLASS, HV_DUP, HV_FROZEN, HV_EMPTY];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut HandoverBook::new(), **c, 24).is_err())
        .count();
    s.add("J6-注入五路", caught == 5, "五路注入全拒");

    // J7 冻结红线
    let j7 = {
        let mut b = HandoverBook::new();
        inject_fault(&mut b, HV_FROZEN, 25);
        b.is_frozen() && b.add(CkItem { ck: 26, class: C_FUNC, anchor: 1, lines: 1, note: "x".to_string() }) == Err("frozen")
    };
    s.add("J7-冻结拒收", j7, "印后登记必抓");

    // J8 零号拒收
    let j8 = HandoverBook::new()
        .add(CkItem { ck: 0, class: C_FUNC, anchor: 1, lines: 1, note: "x".to_string() })
        == Err("zero_ck");
    s.add("J8-零号拒收", j8, "ck=0 即拒");

    // J9 行数账：清单行数合计可复算
    let j9 = {
        let mut b = HandoverBook::new();
        let _ = seed(&mut b);
        let sum: u32 = b.rows.iter().map(|r| r.lines).sum();
        sum == 8 * 320
    };
    s.add("J9-行数合计", j9, "2560 行合计精确");

    // J10 十轮清点回放
    let j10 = {
        let mk = || {
            let mut b = HandoverBook::new();
            let _ = seed(&mut b);
            b.fp()
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
    s.add("J10-十轮清点", j10, "10/10 轮指纹一致");

    s
}
