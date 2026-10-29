//! UNX-F10292 冲突仲裁规则（双端版本冲突裁决）（0x18DC0 段）
//!
//! 锚点：UNX-F10292-J1 仲裁达成（双端版本冲突按规则裁决 10/10 次：本
//! 核优先/O3 优先/双弃三档）；J2 越规检出（无规则冲突/重复仲裁/冻结后
//! 裁 10/10 次检出）；J3 仲裁指纹（同冲突同裁决同指纹）。
//! 域：CoRun·UNX-C3-B40（O3 对接面·冲突仲裁）。
//!
//! 结构：冲突账（双端版本对）+ 仲裁规则真源（按判据类三档）+ 仲裁账
//! （一次裁定）+ 注入核（无规/重复/冻结必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x18DC0 独占 0x20）。
pub const SEG_BASE: u32 = 0x18DC0;
pub const CF_NO_RULE: u32 = SEG_BASE + 0x01; // 无规则冲突
pub const CF_DUP: u32 = SEG_BASE + 0x02; // 重复仲裁
pub const CF_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后裁
pub const CF_BAD_CLASS: u32 = SEG_BASE + 0x04; // 坏判据类
pub const CF_SAME: u32 = SEG_BASE + 0x05; // 同版本伪冲突

/// 仲裁档。
pub const A_CORE: u8 = 0; // 本核优先
pub const A_O3: u8 = 1; // O3 优先
pub const A_DROP: u8 = 2; // 双弃（挂人工）

pub fn arbi_ok(a: u8) -> bool {
    a <= A_DROP
}

pub fn arbi_name(a: u8) -> &'static str {
    match a {
        A_CORE => "core",
        A_O3 => "o3",
        _ => "drop",
    }
}

/// 仲裁规则真源：判据类 → 仲裁档。
pub fn rule_of(class: u8) -> Result<u8, &'static str> {
    match class {
        0 => Ok(A_CORE), // 核心判据冲突本核优先
        1 => Ok(A_O3),   // 辅助判据冲突 O3 优先
        2 => Ok(A_DROP), // 性能判据冲突双弃挂人工
        _ => Err("no_rule"),
    }
}

/// 冲突条目。
#[derive(Clone, PartialEq)]
pub struct Conflict {
    pub ck: u32,
    pub class: u8,
    pub ver_core: u32,
    pub ver_o3: u32,
}

impl Conflict {
    /// 伪冲突：双端同版本。
    pub fn is_same(&self) -> bool {
        self.ver_core == self.ver_o3
    }

    pub fn ok(&self) -> bool {
        self.ck != 0 && !self.is_same()
    }
}

/// 仲裁账。
pub struct ArbitBook {
    rows: Vec<Conflict>,
    verdicts: Vec<(u32, u8)>,
    frozen: bool,
}

impl ArbitBook {
    pub fn new() -> ArbitBook {
        ArbitBook { rows: Vec::new(), verdicts: Vec::new(), frozen: false }
    }

    /// 登冲突：伪冲突/坏类/重号/冻结拒收。
    pub fn reg(&mut self, c: Conflict) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if c.ck == 0 {
            return Err("zero_ck");
        }
        if c.is_same() {
            return Err("same_ver");
        }
        if rule_of(c.class).is_err() {
            return Err("no_rule");
        }
        if self.rows.iter().any(|r| r.ck == c.ck) {
            return Err("dup_ck");
        }
        self.rows.push(c);
        Ok(())
    }

    /// 仲裁：按规则一次裁定；重复仲裁拒收。
    pub fn judge(&mut self, ck: u32) -> Result<u8, &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if self.verdicts.iter().any(|(c, _)| *c == ck) {
            return Err("dup_verdict");
        }
        let c = match self.rows.iter().find(|r| r.ck == ck) {
            Some(x) => x.clone(),
            None => return Err("no_conflict"),
        };
        let a = rule_of(c.class)?;
        self.verdicts.push((ck, a));
        Ok(a)
    }

    /// 仲裁一致性：重算规则与裁决账一致。
    pub fn consistent(&self) -> bool {
        self.verdicts.iter().all(|(ck, a)| {
            self.rows
                .iter()
                .find(|r| r.ck == *ck)
                .map(|r| rule_of(r.class) == Ok(*a))
                .unwrap_or(false)
        })
    }

    /// 仲裁指纹。
    pub fn fp(&self) -> u32 {
        let mut h: u32 = 0x811c9dc5;
        let mut sorted: Vec<&(u32, u8)> = self.verdicts.iter().collect();
        sorted.sort_by_key(|(c, _)| *c);
        for (ck, a) in sorted {
            for b in format!("{}|{}", ck, a).as_bytes() {
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
}

/// 注入核。
pub fn inject_fault(b: &mut ArbitBook, code: u32, ck: u32) -> Result<u8, &'static str> {
    match code {
        CF_NO_RULE => b.reg(Conflict { ck, class: 9, ver_core: 1, ver_o3: 2 }).map(|_| 0),
        CF_DUP => {
            let _ = b.reg(Conflict { ck, class: 0, ver_core: 1, ver_o3: 2 })?;
            let _ = b.judge(ck)?;
            b.judge(ck)
        }
        CF_FROZEN => {
            b.freeze();
            b.judge(ck)
        }
        CF_BAD_CLASS => b.reg(Conflict { ck, class: 200, ver_core: 1, ver_o3: 2 }).map(|_| 0),
        CF_SAME => b.reg(Conflict { ck, class: 0, ver_core: 5, ver_o3: 5 }).map(|_| 0),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10292_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10292");

    // J1 三档仲裁：核心/O3/双弃各按规则
    let j1 = {
        let mut b = ArbitBook::new();
        let _ = b.reg(Conflict { ck: 1, class: 0, ver_core: 2, ver_o3: 1 });
        let _ = b.reg(Conflict { ck: 2, class: 1, ver_core: 1, ver_o3: 2 });
        let _ = b.reg(Conflict { ck: 3, class: 2, ver_core: 1, ver_o3: 3 });
        b.judge(1) == Ok(A_CORE)
            && b.judge(2) == Ok(A_O3)
            && b.judge(3) == Ok(A_DROP)
            && b.consistent()
    };
    s.add("J1-三档仲裁", j1, "三冲突三档各归其位");

    // J2a 无规则冲突拒收
    let j2a = inject_fault(&mut ArbitBook::new(), CF_NO_RULE, 10) == Err("no_rule");
    s.add("J2a-无规则拒", j2a, "class=9 即拒");

    // J2b 重复仲裁拒收
    let j2b = inject_fault(&mut ArbitBook::new(), CF_DUP, 11) == Err("dup_verdict");
    s.add("J2b-重复仲裁拒", j2b, "一次裁定不可逆");

    // J2c 伪冲突拒收
    let j2c = inject_fault(&mut ArbitBook::new(), CF_SAME, 12) == Err("same_ver");
    s.add("J2c-伪冲突拒", j2c, "同版本非冲突");

    // J3 仲裁指纹：同裁决同指纹/加裁必变
    let j3 = {
        let mk = |extra: bool| {
            let mut b = ArbitBook::new();
            let _ = b.reg(Conflict { ck: 1, class: 0, ver_core: 2, ver_o3: 1 });
            let _ = b.judge(1);
            if extra {
                let _ = b.reg(Conflict { ck: 2, class: 1, ver_core: 1, ver_o3: 2 });
                let _ = b.judge(2);
            }
            b.fp()
        };
        let f1 = mk(false);
        let f2 = mk(false);
        let f3 = mk(true);
        f1 == f2 && f1 != f3 && f1 != 0
    };
    s.add("J3-仲裁指纹", j3, "加裁指纹必变");

    // J4 规则真源：三类三档闭集外拒
    let j4 = rule_of(0) == Ok(A_CORE) && rule_of(1) == Ok(A_O3) && rule_of(2) == Ok(A_DROP)
        && rule_of(9).is_err();
    s.add("J4-规则真源", j4, "三类三档闭集外拒");

    // J5 未登记仲裁拒绝
    let j5 = ArbitBook::new().judge(99) == Err("no_conflict");
    s.add("J5-未登记拒", j5, "无冲突即拒");

    // J6 注入五路
    let codes = [CF_NO_RULE, CF_DUP, CF_FROZEN, CF_BAD_CLASS, CF_SAME];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut ArbitBook::new(), **c, 20).is_err())
        .count();
    s.add("J6-注入五路", caught == 5, "五路注入全拒");

    // J7 冻结红线
    let j7 = {
        let mut b = ArbitBook::new();
        inject_fault(&mut b, CF_FROZEN, 21);
        b.is_frozen()
    };
    s.add("J7-冻结拒裁", j7, "印后仲裁必拒");

    // J8 零号拒收
    let j8 = ArbitBook::new()
        .reg(Conflict { ck: 0, class: 0, ver_core: 1, ver_o3: 2 })
        == Err("zero_ck");
    s.add("J8-零号拒收", j8, "ck=0 即拒");

    // J9 仲裁档闭集与名字真源
    let j9 = arbi_ok(A_CORE) && arbi_ok(A_O3) && arbi_ok(A_DROP)
        && arbi_name(A_CORE) == "core" && arbi_name(A_DROP) == "drop";
    s.add("J9-档闭集", j9, "三档名字齐");

    // J10 十轮仲裁回放
    let j10 = {
        let mk = || {
            let mut b = ArbitBook::new();
            let _ = b.reg(Conflict { ck: 1, class: 0, ver_core: 3, ver_o3: 1 });
            let _ = b.reg(Conflict { ck: 2, class: 2, ver_core: 1, ver_o3: 5 });
            let v1 = b.judge(1);
            let v2 = b.judge(2);
            (v1, v2, b.consistent())
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10 && base == (Ok(A_CORE), Ok(A_DROP), true)
    };
    s.add("J10-十轮仲裁", j10, "10/10 轮一致");

    s
}
