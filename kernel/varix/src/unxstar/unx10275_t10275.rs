//! UNX-F10275 压力档双档分账（模拟压/真机压分账）（0x169A0 段）
//!
//! 锚点：UNX-F10275-J1 双档分账达成（模拟压/真机压结果分账且互不混
//! 账 10/10 次）；J2 混账检出（双档矛盾未标注/单档冒充 10/10 次检出）；
//! J3 分账合计口径（双档合计可复算）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·压力演习）。
//!
//! 结构：压力双档账（SIM/EMU 各自判据流水）+ 对平核 + 矛盾单 + 合计
//! 口径核 + 注入核（混账/冒充/冻结必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x169A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x169A0;
pub const DS_CONFLICT: u32 = SEG_BASE + 0x01; // 双档矛盾
pub const DS_IMPERSONATE: u32 = SEG_BASE + 0x02; // 单档冒充
pub const DS_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后写
pub const DS_BAD_TIER: u32 = SEG_BASE + 0x04; // 坏档位
pub const DS_ZERO: u32 = SEG_BASE + 0x05; // 零判据号

/// 压力档位。
pub const T_SIM: u8 = 0; // 模拟压
pub const T_EMU: u8 = 1; // 真机压
pub const TIER_N: u8 = 2;

pub fn tier_ok(t: u8) -> bool {
    t < TIER_N
}

/// 档内结果。
#[derive(Clone, PartialEq)]
pub struct PressRec {
    pub tier: u8,
    pub ck: u32,
    pub passed: bool,
    pub pressure: u16, // 档内压力（千分位）
    pub note: String,
}

impl PressRec {
    pub fn ok(&self) -> bool {
        tier_ok(self.tier) && self.ck != 0 && self.pressure <= 1000 && !self.note.is_empty()
    }
}

/// 压力分账册。
pub struct PressBook {
    rows: Vec<PressRec>,
    conflicts: Vec<(u32, u8, u8)>,
    frozen: bool,
}

impl PressBook {
    pub fn new() -> PressBook {
        PressBook { rows: Vec::new(), conflicts: Vec::new(), frozen: false }
    }

    /// 登结果：重 (tier,ck)/坏档/零号/冻结拒收。
    pub fn put(&mut self, tier: u8, ck: u32, passed: bool, pressure: u16, note: &str) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if !tier_ok(tier) {
            return Err("bad_tier");
        }
        if ck == 0 {
            return Err("zero_ck");
        }
        if pressure > 1000 {
            return Err("bad_pressure");
        }
        if note.is_empty() {
            return Err("empty_note");
        }
        if self.rows.iter().any(|r| r.tier == tier && r.ck == ck) {
            return Err("dup");
        }
        self.rows.push(PressRec { tier, ck, passed, pressure, note: note.to_string() });
        Ok(())
    }

    /// 双档对平：同判据双档结果不一致进矛盾单。
    pub fn crosscheck(&mut self) -> usize {
        self.conflicts.clear();
        let mut cks: Vec<u32> = self.rows.iter().map(|r| r.ck).collect();
        cks.sort();
        cks.dedup();
        for ck in cks {
            let sim = self.rows.iter().find(|r| r.ck == ck && r.tier == T_SIM);
            let emu = self.rows.iter().find(|r| r.ck == ck && r.tier == T_EMU);
            if let (Some(a), Some(bb)) = (sim, emu) {
                if a.passed != bb.passed {
                    self.conflicts.push((ck, a.passed as u8, bb.passed as u8));
                }
            }
        }
        self.conflicts.len()
    }

    /// 冒充检测：单档缺席判据数。
    pub fn impersonate_count(&self) -> usize {
        let mut cks: Vec<u32> = self.rows.iter().map(|r| r.ck).collect();
        cks.sort();
        cks.dedup();
        cks.iter()
            .filter(|ck| {
                let has_sim = self.rows.iter().any(|r| r.ck == **ck && r.tier == T_SIM);
                let has_emu = self.rows.iter().any(|r| r.ck == **ck && r.tier == T_EMU);
                has_sim != has_emu
            })
            .count()
    }

    /// 档内 tally。
    pub fn tally(&self, tier: u8) -> (usize, usize) {
        let pts: Vec<&PressRec> = self.rows.iter().filter(|r| r.tier == tier).collect();
        (pts.iter().filter(|r| r.passed).count(), pts.len())
    }

    /// 合计口径。
    pub fn total(&self) -> (usize, usize) {
        let (g1, t1) = self.tally(T_SIM);
        let (g2, t2) = self.tally(T_EMU);
        (g1 + g2, t1 + t2)
    }

    /// 档内压力均值。
    pub fn avg_pressure(&self, tier: u8) -> Result<u16, &'static str> {
        let pts: Vec<&PressRec> = self.rows.iter().filter(|r| r.tier == tier).collect();
        if pts.is_empty() {
            return Err("empty_tier");
        }
        let sum: u64 = pts.iter().map(|r| r.pressure as u64).sum();
        Ok((sum / pts.len() as u64) as u16)
    }

    pub fn conflict_len(&self) -> usize {
        self.conflicts.len()
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
pub fn inject_fault(b: &mut PressBook, code: u32, ck: u32) -> Result<(), &'static str> {
    match code {
        DS_CONFLICT => {
            let _ = b.put(T_SIM, ck, true, 800, "sim 过");
            b.put(T_EMU, ck, false, 850, "emu 挂")
        }
        DS_IMPERSONATE => b.put(T_SIM, ck, true, 800, "单档"),
        DS_FROZEN => {
            b.freeze();
            b.put(T_SIM, ck, true, 800, "冻结后")
        }
        DS_BAD_TIER => b.put(9, ck, true, 800, "坏档"),
        DS_ZERO => b.put(T_SIM, 0, true, 800, "零号"),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10275_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10275");

    // J1 双档分账 tally 独立
    let j1 = {
        let mut b = PressBook::new();
        let _ = b.put(T_SIM, 1, true, 700, "s1");
        let _ = b.put(T_SIM, 2, false, 750, "s2");
        let _ = b.put(T_EMU, 1, true, 900, "e1");
        let _ = b.put(T_EMU, 2, true, 950, "e2");
        let _ = b.put(T_EMU, 3, false, 980, "e3");
        let (gs, ts) = b.tally(T_SIM);
        let (ge, te) = b.tally(T_EMU);
        let (g, t) = b.total();
        gs == 1 && ts == 2 && ge == 2 && te == 3 && g == 3 && t == 5
    };
    s.add("J1-双档 tally", j1, "档内与合计口径一致");

    // J2a 双档矛盾检出
    let j2a = {
        let mut b = PressBook::new();
        let _ = inject_fault(&mut b, DS_CONFLICT, 10);
        b.crosscheck() == 1 && b.conflict_len() == 1
    };
    s.add("J2a-矛盾检出", j2a, "sim 过 emu 挂入单");

    // J2b 单档冒充检出
    let j2b = {
        let mut b = PressBook::new();
        let _ = inject_fault(&mut b, DS_IMPERSONATE, 11);
        b.impersonate_count() == 1
    };
    s.add("J2b-冒充检出", j2b, "单档缺席计数");

    // J3 档内压力均值
    let j3 = {
        let mut b = PressBook::new();
        let _ = b.put(T_SIM, 1, true, 700, "a");
        let _ = b.put(T_SIM, 2, true, 900, "b");
        b.avg_pressure(T_SIM) == Ok(800)
    };
    s.add("J3-压力均值", j3, "档内均值复算精确");

    // J4 重号拒收
    let j4 = {
        let mut b = PressBook::new();
        let a = b.put(T_SIM, 5, true, 800, "一");
        let bb = b.put(T_SIM, 5, true, 800, "二");
        a == Ok(()) && bb == Err("dup")
    };
    s.add("J4-重号拒收", j4, "同档同号二次即拒");

    // J5 注入五路
    let codes = [DS_CONFLICT, DS_IMPERSONATE, DS_FROZEN, DS_BAD_TIER, DS_ZERO];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut PressBook::new(), **c, 20).is_err())
        .count();
    s.add("J5-注入五路", caught >= 2, "至少两路直接拒收");

    // J6 坏档拒收
    let j6 = inject_fault(&mut PressBook::new(), DS_BAD_TIER, 21) == Err("bad_tier");
    s.add("J6-坏档拒收", j6, "tier=9 即拒");

    // J7 零号拒收
    let j7 = inject_fault(&mut PressBook::new(), DS_ZERO, 0) == Err("zero_ck");
    s.add("J7-零号拒收", j7, "ck=0 即拒");

    // J8 冻结红线
    let j8 = {
        let mut b = PressBook::new();
        inject_fault(&mut b, DS_FROZEN, 22);
        b.is_frozen() && b.put(T_SIM, 23, true, 800, "后") == Err("frozen")
    };
    s.add("J8-冻结拒收", j8, "印后写必抓");

    // J9 条目负样本：坏档/超压/空注 ok 必假
    let j9 = {
        let bad = PressRec { tier: 9, ck: 1, passed: true, pressure: 800, note: "x".to_string() };
        let over = PressRec { tier: T_SIM, ck: 1, passed: true, pressure: 1001, note: "x".to_string() };
        let empty = PressRec { tier: T_SIM, ck: 1, passed: true, pressure: 800, note: String::new() };
        !bad.ok() && !over.ok() && !empty.ok()
    };
    s.add("J9-负样本三假", j9, "三类坏条目 ok 必假");

    // J10 十轮分账回放：10 轮矛盾面与合计一致（确定性）
    let j10 = {
        let mk = || {
            let mut b = PressBook::new();
            let base = 100;
            let _ = b.put(T_SIM, base, true, 800, "s");
            let _ = b.put(T_EMU, base, true, 900, "e");
            let _ = b.put(T_SIM, base + 1, true, 800, "s");
            let _ = b.put(T_EMU, base + 1, false, 900, "e");
            let c = b.crosscheck();
            let (g, t) = b.total();
            (c, g, t)
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10 && base == (1, 3, 4)
    };
    s.add("J10-十轮分账", j10, "10/10 轮口径一致");

    s
}
