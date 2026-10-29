//! UNX-F10276 压力豁免边界（压力性 SKIP 挂账规则）（0x16BC0 段）
//!
//! 锚点：UNX-F10276-J1 豁免边界达成（压力性 SKIP 挂账规则落账：可挂/
//! 不可挂/须披露三类 10/10 次精确分档）；J2 越界豁免检出（不可挂类挂
//! 账/披露缺失/冻结后挂 10/10 次检出）；J3 挂账指纹（同挂账集同指纹）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·压力演习）。
//!
//! 结构：挂账规则真源（判据类 → 可挂性三档）+ 挂账册（SKIP 条目四要
//! 素）+ 披露核 + 注入核（越界挂/缺披露/冻结必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x16BC0 独占 0x20）。
pub const SEG_BASE: u32 = 0x16BC0;
pub const SK_DENY: u32 = SEG_BASE + 0x01; // 不可挂类挂账
pub const SK_NO_DISC: u32 = SEG_BASE + 0x02; // 披露缺失
pub const SK_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后挂
pub const SK_BAD_CLASS: u32 = SEG_BASE + 0x04; // 坏判据类
pub const SK_EMPTY: u32 = SEG_BASE + 0x05; // 空注

/// 判据类闭集（压力演习面）。
pub const K_CORE: u8 = 0; // 核心功能判据
pub const K_PERF: u8 = 1; // 性能判据
pub const K_SOAK: u8 = 2; // soak 长稳判据
pub const K_AUX: u8 = 3; // 辅助判据
pub const CLASS_N: u8 = 4;

pub fn class_ok(c: u8) -> bool {
    c < CLASS_N
}

/// 挂账规则真源：判据类 → 可挂性。
/// 0=可挂（须披露） 1=不可挂 2=可挂免披露
pub fn skip_rule(cls: u8) -> Result<u8, &'static str> {
    match cls {
        K_CORE => Ok(1),      // 核心判据压力下不可 SKIP 挂账
        K_PERF => Ok(0),      // 性能判据可挂须披露
        K_SOAK => Ok(0),      // 长稳判据可挂须披露
        K_AUX => Ok(2),       // 辅助判据可挂免披露
        _ => Err("bad_class"),
    }
}

/// SKIP 挂账条目（四要素）。
#[derive(Clone, PartialEq)]
pub struct SkipRec {
    pub ck: u32,
    pub class: u8,
    pub pressure: u16, // 挂账时压力
    pub disc: String,  // 披露文本（可挂须非空）
}

impl SkipRec {
    /// 四要素自洽：披露规则按挂账规则真源。
    pub fn ok(&self) -> bool {
        if !class_ok(self.class) || self.ck == 0 || self.pressure > 1000 {
            return false;
        }
        match skip_rule(self.class) {
            Ok(0) => !self.disc.is_empty(),   // 可挂须披露
            Ok(1) => false,                   // 不可挂类不得挂账
            Ok(2) => true,                    // 免披露
            _ => false,
        }
    }
}

/// 挂账册。
pub struct SkipBook {
    rows: Vec<SkipRec>,
    frozen: bool,
}

impl SkipBook {
    pub fn new() -> SkipBook {
        SkipBook { rows: Vec::new(), frozen: false }
    }

    /// 挂账：规则校验（不可挂类/披露缺失）+ 重号/冻结拒收。
    pub fn hang(&mut self, ck: u32, class: u8, pressure: u16, disc: &str) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if ck == 0 {
            return Err("zero_ck");
        }
        if !class_ok(class) {
            return Err("bad_class");
        }
        if pressure > 1000 {
            return Err("bad_pressure");
        }
        if self.rows.iter().any(|r| r.ck == ck) {
            return Err("dup_ck");
        }
        match skip_rule(class) {
            Ok(1) => return Err("deny"),
            Ok(0) if disc.is_empty() => return Err("no_disc"),
            _ => {}
        }
        self.rows.push(SkipRec {
            ck,
            class,
            pressure,
            disc: disc.to_string(),
        });
        Ok(())
    }

    /// 披露完备：可挂须披露类条目披露全非空。
    pub fn disc_complete(&self) -> bool {
        self.rows.iter().all(|r| match skip_rule(r.class) {
            Ok(0) => !r.disc.is_empty(),
            _ => true,
        })
    }

    /// 挂账指纹。
    pub fn fp(&self) -> u32 {
        let mut h: u32 = 0x811c9dc5;
        let mut sorted: Vec<&SkipRec> = self.rows.iter().collect();
        sorted.sort_by_key(|r| r.ck);
        for r in sorted {
            for b in format!("{}|{}|{}", r.ck, r.class, r.pressure).as_bytes() {
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

/// 注入核。
pub fn inject_fault(b: &mut SkipBook, code: u32, ck: u32) -> Result<(), &'static str> {
    match code {
        SK_DENY => b.hang(ck, K_CORE, 900, "核心判据越界挂"),
        SK_NO_DISC => b.hang(ck, K_PERF, 800, ""),
        SK_FROZEN => {
            b.freeze();
            b.hang(ck, K_PERF, 800, "冻结后")
        }
        SK_BAD_CLASS => b.hang(ck, 9, 800, "x"),
        SK_EMPTY => b.hang(ck, K_PERF, 1001, "超压"),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10276_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10276");

    // J1 挂账规则真源：四类三档精确
    let j1 = skip_rule(K_CORE) == Ok(1)
        && skip_rule(K_PERF) == Ok(0)
        && skip_rule(K_SOAK) == Ok(0)
        && skip_rule(K_AUX) == Ok(2)
        && skip_rule(9).is_err();
    s.add("J1-规则真源", j1, "四类三档精确");

    // J2a 不可挂类拦截
    let j2a = inject_fault(&mut SkipBook::new(), SK_DENY, 10) == Err("deny");
    s.add("J2a-核心挂账拦", j2a, "核心判据挂账即拒");

    // J2b 披露缺失拦截
    let j2b = inject_fault(&mut SkipBook::new(), SK_NO_DISC, 11) == Err("no_disc");
    s.add("J2b-披露缺失拦", j2b, "可挂须披露类空披露即拒");

    // J2c 越压挂账拦截
    let j2c = inject_fault(&mut SkipBook::new(), SK_EMPTY, 12) == Err("bad_pressure");
    s.add("J2c-越压拦截", j2c, "压力 1001 即拒");

    // J3 合规挂账三类全通
    let j3 = {
        let mut b = SkipBook::new();
        let a = b.hang(1, K_PERF, 800, "压力性 SKIP：性能抖动");
        let bb = b.hang(2, K_SOAK, 850, "长稳扰动 SKIP");
        let c = b.hang(3, K_AUX, 900, "");
        a == Ok(()) && bb == Ok(()) && c == Ok(()) && b.disc_complete() && b.all_ok()
    };
    s.add("J3-合规三挂", j3, "性能/长稳/辅助全过");

    // J4 核心判据条目 ok 必假（不可挂类自洽）
    let j4 = {
        let bad = SkipRec { ck: 1, class: K_CORE, pressure: 900, disc: "x".to_string() };
        !bad.ok()
    };
    s.add("J4-核心负样本", j4, "核心类 ok 必假");

    // J5 挂账指纹：同集同指纹/压变指纹变
    let j5 = {
        let mk = |p: u16| {
            let mut b = SkipBook::new();
            let _ = b.hang(1, K_PERF, p, "n");
            b.fp()
        };
        let f1 = mk(800);
        let f2 = mk(800);
        let f3 = mk(850);
        f1 == f2 && f1 != f3 && f1 != 0
    };
    s.add("J5-挂账指纹", j5, "压变指纹必变");

    // J6 重号拒收
    let j6 = {
        let mut b = SkipBook::new();
        let a = b.hang(7, K_PERF, 800, "一");
        let bb = b.hang(7, K_PERF, 800, "二");
        a == Ok(()) && bb == Err("dup_ck")
    };
    s.add("J6-重号拒收", j6, "同判据二次即拒");

    // J7 坏类拒收
    let j7 = inject_fault(&mut SkipBook::new(), SK_BAD_CLASS, 13) == Err("bad_class");
    s.add("J7-坏类拒收", j7, "class=9 即拒");

    // J8 注入五路
    let codes = [SK_DENY, SK_NO_DISC, SK_FROZEN, SK_BAD_CLASS, SK_EMPTY];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut SkipBook::new(), **c, 14).is_err())
        .count();
    s.add("J8-注入五路", caught == 5, "五路注入全拒");

    // J9 冻结红线
    let j9 = {
        let mut b = SkipBook::new();
        inject_fault(&mut b, SK_FROZEN, 15);
        b.is_frozen() && b.hang(16, K_PERF, 800, "后") == Err("frozen")
    };
    s.add("J9-冻结拒挂", j9, "印后挂账必抓");

    // J10 十轮挂账回放：10 轮指纹一致（确定性）
    let j10 = {
        let mk = || {
            let mut b = SkipBook::new();
            let _ = b.hang(1, K_PERF, 800, "n1");
            let _ = b.hang(2, K_AUX, 850, "");
            let _ = b.hang(3, K_SOAK, 900, "n3");
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
    s.add("J10-十轮挂账", j10, "10/10 轮指纹一致");

    s
}
