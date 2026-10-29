//! UNX-F10294 移交面双档标记（模拟/真机判据分档移交）（0x19200 段）
//!
//! 锚点：UNX-F10294-J1 双档标记达成（模拟/真机判据分档移交且互不混
//! 档 10/10 次）；J2 混档检出（双档矛盾未标注/单档冒充双档 10/10 次
//! 检出）；J3 分档合计口径（双档合计可复算）。
//! 域：CoRun·UNX-C3-B40（O3 对接面·分档移交）。
//!
//! 结构：分档账（SIM/EMU 各自判据集）+ 对平核 + 矛盾单 + 合计口径 +
//! 注入核（混档/冒充/冻结必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19200 独占 0x20）。
pub const SEG_BASE: u32 = 0x19200;
pub const HT_CONFLICT: u32 = SEG_BASE + 0x01; // 双档矛盾
pub const HT_IMPERSONATE: u32 = SEG_BASE + 0x02; // 单档冒充
pub const HT_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后写
pub const HT_BAD_TIER: u32 = SEG_BASE + 0x04; // 坏档位
pub const HT_ZERO: u32 = SEG_BASE + 0x05; // 零判据号

/// 档位闭集。
pub const T_SIM: u8 = 0;
pub const T_EMU: u8 = 1;
pub const TIER_N: u8 = 2;

pub fn tier_ok(t: u8) -> bool {
    t < TIER_N
}

/// 档内判据。
#[derive(Clone, PartialEq)]
pub struct HandItem {
    pub tier: u8,
    pub ck: u32,
    pub passed: bool,
    pub note: String,
}

impl HandItem {
    pub fn ok(&self) -> bool {
        tier_ok(self.tier) && self.ck != 0 && !self.note.is_empty()
    }
}

/// 分档账。
pub struct TierBook {
    rows: Vec<HandItem>,
    conflicts: Vec<(u32, u8, u8)>,
    frozen: bool,
}

impl TierBook {
    pub fn new() -> TierBook {
        TierBook { rows: Vec::new(), conflicts: Vec::new(), frozen: false }
    }

    /// 登条目：重 (tier,ck)/坏档/零号/冻结拒收。
    pub fn put(&mut self, tier: u8, ck: u32, passed: bool, note: &str) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if !tier_ok(tier) {
            return Err("bad_tier");
        }
        if ck == 0 {
            return Err("zero_ck");
        }
        if note.is_empty() {
            return Err("empty_note");
        }
        if self.rows.iter().any(|r| r.tier == tier && r.ck == ck) {
            return Err("dup");
        }
        self.rows.push(HandItem { tier, ck, passed, note: note.to_string() });
        Ok(())
    }

    /// 双档对平：矛盾进单。
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

    /// 冒充计数：单档缺席判据。
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
        let pts: Vec<&HandItem> = self.rows.iter().filter(|r| r.tier == tier).collect();
        (pts.iter().filter(|r| r.passed).count(), pts.len())
    }

    /// 合计。
    pub fn total(&self) -> (usize, usize) {
        let (g1, t1) = self.tally(T_SIM);
        let (g2, t2) = self.tally(T_EMU);
        (g1 + g2, t1 + t2)
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
pub fn inject_fault(b: &mut TierBook, code: u32, ck: u32) -> Result<(), &'static str> {
    match code {
        HT_CONFLICT => {
            let _ = b.put(T_SIM, ck, true, "sim 过");
            b.put(T_EMU, ck, false, "emu 挂")
        }
        HT_IMPERSONATE => b.put(T_SIM, ck, true, "单档"),
        HT_FROZEN => {
            b.freeze();
            b.put(T_SIM, ck, true, "冻结后")
        }
        HT_BAD_TIER => b.put(9, ck, true, "坏档"),
        HT_ZERO => b.put(T_SIM, 0, true, "零号"),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10294_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10294");

    // J1 双档分账 tally 独立
    let j1 = {
        let mut b = TierBook::new();
        let _ = b.put(T_SIM, 1, true, "s1");
        let _ = b.put(T_SIM, 2, false, "s2");
        let _ = b.put(T_EMU, 1, true, "e1");
        let _ = b.put(T_EMU, 2, true, "e2");
        let _ = b.put(T_EMU, 3, false, "e3");
        let (gs, ts) = b.tally(T_SIM);
        let (ge, te) = b.tally(T_EMU);
        let (g, t) = b.total();
        gs == 1 && ts == 2 && ge == 2 && te == 3 && g == 3 && t == 5
    };
    s.add("J1-双档 tally", j1, "档内与合计口径一致");

    // J2a 双档矛盾检出
    let j2a = {
        let mut b = TierBook::new();
        let _ = inject_fault(&mut b, HT_CONFLICT, 10);
        b.crosscheck() == 1 && b.conflict_len() == 1
    };
    s.add("J2a-矛盾检出", j2a, "sim 过 emu 挂入单");

    // J2b 单档冒充检出
    let j2b = {
        let mut b = TierBook::new();
        let _ = inject_fault(&mut b, HT_IMPERSONATE, 11);
        b.impersonate_count() == 1
    };
    s.add("J2b-冒充检出", j2b, "单档缺席计数");

    // J3 混合三面清晰
    let j3 = {
        let mut b = TierBook::new();
        let _ = b.put(T_SIM, 30, true, "一致");
        let _ = b.put(T_EMU, 30, true, "一致");
        let _ = b.put(T_SIM, 31, true, "矛盾");
        let _ = b.put(T_EMU, 31, false, "矛盾");
        let _ = b.put(T_SIM, 32, false, "单档");
        let c = b.crosscheck();
        c == 1 && b.impersonate_count() == 1 && b.len() == 5
    };
    s.add("J3-混合三面", j3, "一致/矛盾/单档三面");

    // J4 重号拒收
    let j4 = {
        let mut b = TierBook::new();
        let a = b.put(T_SIM, 5, true, "一");
        let bb = b.put(T_SIM, 5, true, "二");
        a == Ok(()) && bb == Err("dup")
    };
    s.add("J4-重号拒收", j4, "同档同号二次即拒");

    // J5 注入五路
    let codes = [HT_CONFLICT, HT_IMPERSONATE, HT_FROZEN, HT_BAD_TIER, HT_ZERO];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut TierBook::new(), **c, 20).is_err())
        .count();
    s.add("J5-注入五路", caught >= 2, "至少两路直接拒收");

    // J6 坏档拒收
    let j6 = inject_fault(&mut TierBook::new(), HT_BAD_TIER, 21) == Err("bad_tier");
    s.add("J6-坏档拒收", j6, "tier=9 即拒");

    // J7 零号拒收
    let j7 = inject_fault(&mut TierBook::new(), HT_ZERO, 0) == Err("zero_ck");
    s.add("J7-零号拒收", j7, "ck=0 即拒");

    // J8 冻结红线
    let j8 = {
        let mut b = TierBook::new();
        inject_fault(&mut b, HT_FROZEN, 22);
        b.is_frozen() && b.put(T_SIM, 23, true, "后") == Err("frozen")
    };
    s.add("J8-冻结拒收", j8, "印后写必抓");

    // J9 条目负样本三假
    let j9 = {
        let bad = HandItem { tier: 9, ck: 1, passed: true, note: "x".to_string() };
        let zero = HandItem { tier: T_SIM, ck: 0, passed: true, note: "x".to_string() };
        let empty = HandItem { tier: T_SIM, ck: 1, passed: true, note: String::new() };
        !bad.ok() && !zero.ok() && !empty.ok()
    };
    s.add("J9-负样本三假", j9, "三类坏条目 ok 必假");

    // J10 十轮分账回放
    let j10 = {
        let mk = || {
            let mut b = TierBook::new();
            let _ = b.put(T_SIM, 100, true, "s");
            let _ = b.put(T_EMU, 100, true, "e");
            let _ = b.put(T_SIM, 101, true, "s");
            let _ = b.put(T_EMU, 101, false, "e");
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
