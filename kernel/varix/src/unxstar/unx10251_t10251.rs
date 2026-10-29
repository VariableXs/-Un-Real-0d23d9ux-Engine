//! UNX-F10251 双档分账披露（模拟/真机结果分账）（0x136A0 段）
//!
//! 锚点：UNX-F10251-J1 双档分账达成（模拟档/真机档结果分账登记且互
//! 不混账 10/10 次）；J2 混账检出（双档同判据结果矛盾未标注/单档冒充
//! 双档 10/10 次检出）；J3 分账合计口径（双档合计数可复算）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·认证面）。
//!
//! 结构：双档账（SIM/EMU 两档各自判据流水）+ 分账核（同判据双档对
//! 平）+ 矛盾单 + 合计口径核 + 注入核（混账/冒充/冻结后写必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x136A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x136A0;
pub const DU_CONFLICT: u32 = SEG_BASE + 0x01; // 双档矛盾
pub const DU_IMPERSONATE: u32 = SEG_BASE + 0x02; // 单档冒充双档
pub const DU_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后写
pub const DU_BAD_TIER: u32 = SEG_BASE + 0x04; // 坏档位
pub const DU_ZERO_CK: u32 = SEG_BASE + 0x05; // 零判据号

/// 档位闭集。
pub const T_SIM: u8 = 0; // 模拟档
pub const T_EMU: u8 = 1; // 真机档（宿主模拟）
pub const TIER_N: u8 = 2;

pub fn tier_ok(t: u8) -> bool {
    t < TIER_N
}

pub fn tier_name(t: u8) -> &'static str {
    match t {
        T_SIM => "sim",
        _ => "emu",
    }
}

/// 档内结果。
#[derive(Clone, PartialEq)]
pub struct TierRec {
    pub tier: u8,
    pub ck: u32,
    pub passed: bool,
    pub note: String,
}

impl TierRec {
    pub fn ok(&self) -> bool {
        tier_ok(self.tier) && self.ck != 0 && !self.note.is_empty()
    }
}

/// 双档账。
pub struct DualBook {
    rows: Vec<TierRec>,
    frozen: bool,
    conflicts: Vec<(u32, u8, u8)>, // (ck, sim, emu) 矛盾单
}

impl DualBook {
    pub fn new() -> DualBook {
        DualBook { rows: Vec::new(), frozen: false, conflicts: Vec::new() }
    }

    /// 登结果：同 (tier, ck) 重号拒收。
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
        self.rows.push(TierRec { tier, ck, passed, note: note.to_string() });
        Ok(())
    }

    /// 分账对平：双档同判据结果必须一致；不一致进矛盾单。
    /// 单档判据不进矛盾单（但披露时单档须显式标注——impersonate 检测）。
    pub fn crosscheck(&mut self) -> usize {
        self.conflicts.clear();
        let cks: Vec<u32> = {
            let mut v: Vec<u32> = self.rows.iter().map(|r| r.ck).collect();
            v.sort();
            v.dedup();
            v
        };
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

    /// 矛盾单回读（判据用）：与 crosscheck 记账同源。
    pub fn conflicts(&self) -> Vec<(u32, u8, u8)> {
        self.conflicts.clone()
    }

    /// 冒充检测：声称双档合计但某档缺席的判据数。
    pub fn impersonate_count(&self) -> usize {
        let cks: Vec<u32> = {
            let mut v: Vec<u32> = self.rows.iter().map(|r| r.ck).collect();
            v.sort();
            v.dedup();
            v
        };
        cks.iter()
            .filter(|ck| {
                let has_sim = self.rows.iter().any(|r| r.ck == **ck && r.tier == T_SIM);
                let has_emu = self.rows.iter().any(|r| r.ck == **ck && r.tier == T_EMU);
                has_sim != has_emu // 一档有一档无 = 冒充候选
            })
            .count()
    }

    /// 合计口径：档 × 通过数。
    pub fn tally(&self, tier: u8) -> (usize, usize) {
        let rows: Vec<&TierRec> = self.rows.iter().filter(|r| r.tier == tier).collect();
        let total = rows.len();
        let green = rows.iter().filter(|r| r.passed).count();
        (green, total)
    }

    /// 双档合计可复算：两档 tally 相加。
    pub fn total_tally(&self) -> (usize, usize) {
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
pub fn inject_fault(b: &mut DualBook, code: u32, ck: u32) -> Result<(), &'static str> {
    match code {
        DU_CONFLICT => {
            let _ = b.put(T_SIM, ck, true, "sim 过");
            b.put(T_EMU, ck, false, "emu 挂")
        }
        DU_IMPERSONATE => {
            // 只有 sim 档有、emu 档缺席
            b.put(T_SIM, ck, true, "单档冒充")
        }
        DU_FROZEN => {
            b.freeze();
            b.put(T_SIM, ck, true, "冻结后写")
        }
        DU_BAD_TIER => b.put(9, ck, true, "坏档"),
        DU_ZERO_CK => b.put(T_SIM, 0, true, "零号"),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10251_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10251");

    // J1 双档分账：sim/emu 各自 tally 独立可复算
    let j1 = {
        let mut b = DualBook::new();
        let _ = b.put(T_SIM, 1, true, "s1");
        let _ = b.put(T_SIM, 2, false, "s2");
        let _ = b.put(T_EMU, 1, true, "e1");
        let _ = b.put(T_EMU, 2, true, "e2");
        let _ = b.put(T_EMU, 3, false, "e3");
        let (gs, ts) = b.tally(T_SIM);
        let (ge, te) = b.tally(T_EMU);
        let (g, t) = b.total_tally();
        gs == 1 && ts == 2 && ge == 2 && te == 3 && g == 3 && t == 5
    };
    s.add("J1-双档分账", j1, "档内 tally 与合计口径一致");

    // J2a 双档矛盾检出
    let j2a = {
        let mut b = DualBook::new();
        let _ = inject_fault(&mut b, DU_CONFLICT, 10);
        b.crosscheck() == 1
            && b.conflicts() == vec![(10u32, 1u8, 0u8)]
    };
    s.add("J2a-矛盾检出", j2a, "sim 过 emu 挂精确入单");

    // J2b 单档冒充检出
    let j2b = {
        let mut b = DualBook::new();
        let _ = inject_fault(&mut b, DU_IMPERSONATE, 11);
        b.impersonate_count() == 1
    };
    s.add("J2b-冒充检出", j2b, "单档缺席判据计数");

    // J3 分账互不混账：档内重号拒收
    let j3 = {
        let mut b = DualBook::new();
        let a = b.put(T_SIM, 5, true, "一");
        let bb = b.put(T_SIM, 5, true, "二");
        a == Ok(()) && bb == Err("dup")
    };
    s.add("J3-档内重号拒", j3, "同档同号二次即拒");

    // J4 注入面五路
    let codes = [DU_CONFLICT, DU_IMPERSONATE, DU_FROZEN, DU_BAD_TIER, DU_ZERO_CK];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut DualBook::new(), **c, 20).is_err())
        .count();
    s.add("J4-注入五路", caught >= 2, "至少两路直接拒收");

    // J5 坏档拒收
    let j5 = inject_fault(&mut DualBook::new(), DU_BAD_TIER, 21) == Err("bad_tier");
    s.add("J5-坏档拒收", j5, "tier=9 即拒");

    // J6 零号拒收
    let j6 = inject_fault(&mut DualBook::new(), DU_ZERO_CK, 0) == Err("zero_ck");
    s.add("J6-零号拒收", j6, "ck=0 即拒");

    // J7 冻结红线
    let j7 = {
        let mut b = DualBook::new();
        inject_fault(&mut b, DU_FROZEN, 22);
        b.is_frozen() && b.put(T_SIM, 23, true, "后") == Err("frozen")
    };
    s.add("J7-冻结拒收", j7, "印后写必抓");

    // J8 混合册：三判据（一致/矛盾/单档）分账后三面清晰
    let j8 = {
        let mut b = DualBook::new();
        let _ = b.put(T_SIM, 30, true, "一致过");
        let _ = b.put(T_EMU, 30, true, "一致过");
        let _ = b.put(T_SIM, 31, true, "矛盾");
        let _ = b.put(T_EMU, 31, false, "矛盾");
        let _ = b.put(T_SIM, 32, false, "单档");
        let conflicts = b.crosscheck();
        conflicts == 1
            && b.conflict_len() == 1
            && b.impersonate_count() == 1
            && b.len() == 5
    };
    s.add("J8-混合三面", j8, "一致/矛盾/单档三面清晰");

    // J9 条目负样本：坏档/零号/空注 ok 必假
    let j9 = {
        let bad_t = TierRec { tier: 9, ck: 1, passed: true, note: "x".to_string() };
        let zero = TierRec { tier: T_SIM, ck: 0, passed: true, note: "x".to_string() };
        let empty = TierRec { tier: T_SIM, ck: 1, passed: true, note: String::new() };
        !bad_t.ok() && !zero.ok() && !empty.ok()
    };
    s.add("J9-负样本三假", j9, "三类坏条目 ok 必假");

    // J10 十轮分账回放：10 轮 tally 与矛盾面一致（确定性）
    let j10 = {
        let mut ok = 0;
        for r in 0..10u32 {
            let mut b = DualBook::new();
            let base = r * 10 + 1; // 零号 ck 被 zero_ck 拒收，基址 +1
            let _ = b.put(T_SIM, base, true, "s");
            let _ = b.put(T_EMU, base, true, "e");
            let _ = b.put(T_SIM, base + 1, true, "s");
            let _ = b.put(T_EMU, base + 1, false, "e");
            let c = b.crosscheck();
            let (g, t) = b.total_tally();
            if c == 1 && g == 3 && t == 4 {
                ok += 1;
            }
        }
        ok == 10
    };
    s.add("J10-十轮分账", j10, "10/10 轮口径一致");

    s
}
