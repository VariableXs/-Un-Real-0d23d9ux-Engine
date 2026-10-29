//! UNX-F10264 压力用例集选点（LTP 压力族映射）（0x15240 段）
//!
//! 锚点：UNX-F10264-J1 用例选点达成（LTP 压力族 → 本核压力用例映射
//! ≥10 族且族内选点齐备 10/10 次）；J2 选点越界检出（族外选点/重复
//! 选点/映射漂移 10/10 次检出）；J3 映射指纹（同映射同指纹）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·压力演习）。
//!
//! 结构：LTP 族目录（真源）+ 映射册（族 → 用例号集）+ 选点核（族内
//! 校验）+ 映射指纹 + 注入核（族外/重复/漂移必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x15240 独占 0x20）。
pub const SEG_BASE: u32 = 0x15240;
pub const LT_OUT_FAMILY: u32 = SEG_BASE + 0x01; // 族外选点
pub const LT_DUP: u32 = SEG_BASE + 0x02; // 重复选点
pub const LT_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后改
pub const LT_BAD_FAMILY: u32 = SEG_BASE + 0x04; // 坏族名
pub const LT_EMPTY_PICK: u32 = SEG_BASE + 0x05; // 空选点

/// LTP 压力族目录（真源）：族号 → 族名。
pub const FAMILIES: [(u8, &str); 10] = [
    (1, "sched"),
    (2, "mem"),
    (3, "fs_inod"),
    (4, "pipe"),
    (5, "sem"),
    (6, "shm"),
    (7, "fork"),
    (8, "mm_core"),
    (9, "pty"),
    (10, "tcp_multi"),
];

pub fn family_ok(f: u8) -> bool {
    FAMILIES.iter().any(|(id, _)| *id == f)
}

/// 族容量（族内用例号上限，选点校验依据）。
pub fn family_cap(f: u8) -> u32 {
    match f {
        1 => 20,
        2 => 40,
        3 => 30,
        4 => 25,
        5 => 15,
        6 => 15,
        7 => 35,
        8 => 30,
        9 => 12,
        10 => 20,
        _ => 0,
    }
}

/// 映射条目：族 → 选点用例号集。
#[derive(Clone, PartialEq)]
pub struct FamilyMap {
    pub family: u8,
    pub picks: Vec<u32>, // 1..=cap
}

impl FamilyMap {
    pub fn ok(&self) -> bool {
        family_ok(self.family)
            && !self.picks.is_empty()
            && self.picks.iter().all(|p| (1..=family_cap(self.family)).contains(p))
    }
}

/// 映射册。
pub struct MapBook {
    rows: Vec<FamilyMap>,
    frozen: bool,
}

impl MapBook {
    pub fn new() -> MapBook {
        MapBook { rows: Vec::new(), frozen: false }
    }

    /// 登映射：坏族/空选点/重族/冻结后写拒收。
    pub fn map(&mut self, family: u8, picks: Vec<u32>) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if !family_ok(family) {
            return Err("bad_family");
        }
        if picks.is_empty() {
            return Err("empty_pick");
        }
        if self.rows.iter().any(|r| r.family == family) {
            return Err("dup_family");
        }
        // 族内校验 + 族内去重
        let cap = family_cap(family);
        for p in &picks {
            if *p == 0 || *p > cap {
                return Err("out_family");
            }
        }
        let mut sorted = picks.clone();
        sorted.sort();
        sorted.dedup();
        if sorted.len() != picks.len() {
            return Err("dup_pick");
        }
        self.rows.push(FamilyMap { family, picks: sorted });
        Ok(())
    }

    /// 十族齐备。
    pub fn full(&self) -> bool {
        FAMILIES.iter().all(|(f, _)| self.rows.iter().any(|r| r.family == *f))
    }

    pub fn missing(&self) -> Vec<u8> {
        FAMILIES
            .iter()
            .filter(|(f, _)| !self.rows.iter().any(|r| r.family == *f))
            .map(|(f, _)| *f)
            .collect()
    }

    /// 选点总数。
    pub fn pick_total(&self) -> usize {
        self.rows.iter().map(|r| r.picks.len()).sum()
    }

    /// 映射指纹：族号 × 选点集折叠。
    pub fn fp(&self) -> u32 {
        let mut h: u32 = 0x811c9dc5;
        let mut sorted: Vec<&FamilyMap> = self.rows.iter().collect();
        sorted.sort_by_key(|r| r.family);
        for r in sorted {
            h ^= r.family as u32;
            h = h.wrapping_mul(0x01000193);
            for p in &r.picks {
                for b in p.to_le_bytes() {
                    h ^= b as u32;
                    h = h.wrapping_mul(0x01000193);
                }
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

/// 铺十族映射基线（每族选 3 点）。
pub fn seed(b: &mut MapBook) -> Result<(), &'static str> {
    for (f, _) in FAMILIES {
        let cap = family_cap(f);
        b.map(f, vec![1, cap / 2, cap])?;
    }
    Ok(())
}

/// 注入核。
pub fn inject_fault(b: &mut MapBook, code: u32, family: u8) -> Result<(), &'static str> {
    match code {
        LT_OUT_FAMILY => b.map(family, vec![1, family_cap(family) + 1]),
        LT_DUP => {
            let _ = b.map(family, vec![1]);
            b.map(family, vec![2])
        }
        LT_FROZEN => {
            b.freeze();
            b.map(family, vec![1])
        }
        LT_BAD_FAMILY => b.map(200, vec![1]),
        LT_EMPTY_PICK => b.map(family, vec![]),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10264_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10264");

    // J1 十族映射齐备且选点全合法
    let j1 = {
        let mut b = MapBook::new();
        let _ = seed(&mut b);
        b.full() && b.len() == 10 && b.all_ok() && b.pick_total() == 30
    };
    s.add("J1-十族三十点", j1, "10 族×3 点全合法");

    // J2a 族外选点拒收（超容量）
    let j2a = inject_fault(&mut MapBook::new(), LT_OUT_FAMILY, 1) == Err("out_family");
    s.add("J2a-族外拒收", j2a, "超 cap 即拒");

    // J2b 重复族拒收
    let j2b = inject_fault(&mut MapBook::new(), LT_DUP, 2) == Err("dup_family");
    s.add("J2b-重族拒收", j2b, "同族二次即拒");

    // J2c 族内重复选点拒收
    let j2c = MapBook::new().map(3, vec![1, 1, 2]) == Err("dup_pick");
    s.add("J2c-重选点拒", j2c, "族内重号即拒");

    // J3 映射指纹：同映射同指纹/选点变指纹变
    let j3 = {
        let mut b1 = MapBook::new();
        let _ = seed(&mut b1);
        let f1 = b1.fp();
        let mut b2 = MapBook::new();
        for (f, _) in FAMILIES {
            let cap = family_cap(f);
            let picks = if f == 4 { vec![1, 2, cap] } else { vec![1, cap / 2, cap] };
            let _ = b2.map(f, picks);
        }
        let f2 = b2.fp();
        f1 == b1.fp() && f1 != f2 && f1 != 0
    };
    s.add("J3-映射指纹", j3, "选点变指纹必变");

    // J4 零号选点拒收
    let j4 = MapBook::new().map(5, vec![0, 1]) == Err("out_family");
    s.add("J4-零号选点拒", j4, "0 即拒");

    // J5 注入五路
    let codes = [LT_OUT_FAMILY, LT_DUP, LT_FROZEN, LT_BAD_FAMILY, LT_EMPTY_PICK];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut MapBook::new(), **c, 6).is_err())
        .count();
    s.add("J5-注入五路", caught == 5, "五路注入全拒");

    // J6 坏族拒收
    let j6 = inject_fault(&mut MapBook::new(), LT_BAD_FAMILY, 0) == Err("bad_family");
    s.add("J6-坏族拒收", j6, "family=200 即拒");

    // J7 冻结红线
    let j7 = {
        let mut b = MapBook::new();
        inject_fault(&mut b, LT_FROZEN, 7);
        b.is_frozen() && b.map(8, vec![1]) == Err("frozen")
    };
    s.add("J7-冻结拒收", j7, "印后映射必抓");

    // J8 空选点拒收
    let j8 = inject_fault(&mut MapBook::new(), LT_EMPTY_PICK, 9) == Err("empty_pick");
    s.add("J8-空选点拒", j8, "空集即拒");

    // J9 族容量真源：十族容量全为正
    let j9 = FAMILIES.iter().all(|(f, _)| family_cap(*f) > 0) && family_cap(200) == 0;
    s.add("J9-容量真源", j9, "十族容量全正");

    // J10 十轮映射回放：10 轮指纹一致（确定性）
    let j10 = {
        let mk = || {
            let mut b = MapBook::new();
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
    s.add("J10-十轮映射", j10, "10/10 轮指纹一致");

    s
}
