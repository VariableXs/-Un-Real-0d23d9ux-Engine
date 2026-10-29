//! UNX-F10332 释放后回归保护（释放项入回归集）（0x1b400 段）
//!
//! 锚点：UNX-F10332-J1 释放项回归集——闸开后每个释放项按名取基线指纹
//! 入回归集，回放时重算指纹与基线比对，指纹漂移即列漂移单，全部回放
//! 零漂移且集齐锚才算回归保护就位。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：回归集（项=项号+释放要素号+项名+基线指纹 64 位）+ 回放核
//! （未入集回放即拒、重放即拒）+ 漂移核（重算指纹异于基线即列漂移）+
//! 注入核（零号/重项/漂移名/未集回放四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b400 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b400;
pub const GR_ZERO: u32 = SEG_BASE + 0x01; // 项号零
pub const GR_DUP: u32 = SEG_BASE + 0x02; // 项号重号
pub const GR_NORID: u32 = SEG_BASE + 0x03; // 未知项
pub const GR_REREPLAY: u32 = SEG_BASE + 0x04; // 已放再放
pub const GR_CAP: u32 = SEG_BASE + 0x05; // 集超容
pub const GR_ENO: u32 = SEG_BASE + 0x06; // 释放要素号越界

/// 回归集锚（八项）。
pub const ITEMS_NEEDED: usize = 8;
/// 集定容。
pub const CAP: usize = 12;

/// FNV-1a 64 位名指纹（基线与回放共用）。
pub fn name_digest(name: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let bytes = name.as_bytes();
    let mut k = 0usize;
    while k < bytes.len() {
        h ^= bytes[k] as u64;
        h = h.wrapping_mul(0x100000001b3);
        k += 1;
    }
    h
}

/// 回归项。
#[derive(Clone)]
pub struct GateRegItem {
    pub rid: u32,
    pub eno: u32,
    pub name: String,
    pub baseline: u64,
    pub replayed: bool,
    pub last: u64,
}

impl GateRegItem {
    /// 漂移：已回放且末次指纹异于基线。
    pub fn drifted(&self) -> bool {
        self.replayed && self.last != self.baseline
    }
}

/// 释放后回归集。
pub struct GateRegression {
    pub items: Vec<GateRegItem>,
    pub armed: bool,
}

impl GateRegression {
    pub fn new() -> GateRegression {
        GateRegression { items: Vec::new(), armed: false }
    }

    fn pos(&self, rid: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].rid == rid {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 入集：项号唯一、项名不空、释放要素号 1..=5。
    pub fn enroll(&mut self, rid: u32, eno: u32, name: &str) -> Result<(), u32> {
        if self.armed {
            return Err(GR_REREPLAY);
        }
        if rid == 0 {
            return Err(GR_ZERO);
        }
        if self.pos(rid).is_some() {
            return Err(GR_DUP);
        }
        if eno == 0 || eno > 5 {
            return Err(GR_ENO);
        }
        if name.is_empty() {
            return Err(GR_ZERO);
        }
        if self.items.len() >= CAP {
            return Err(GR_CAP);
        }
        self.items.push(GateRegItem {
            rid,
            eno,
            name: name.to_string(),
            baseline: name_digest(name),
            replayed: false,
            last: 0,
        });
        Ok(())
    }

    /// 回放一项：重算指纹与基线比对。
    pub fn replay(&mut self, rid: u32, got_name: &str) -> Result<bool, u32> {
        match self.pos(rid) {
            None => Err(GR_NORID),
            Some(p) => {
                if self.items[p].replayed {
                    return Err(GR_REREPLAY);
                }
                let d = name_digest(got_name);
                self.items[p].last = d;
                self.items[p].replayed = true;
                Ok(d == self.items[p].baseline)
            }
        }
    }

    /// 漂移项清单。
    pub fn drift_list(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].drifted() {
                v.push(self.items[k].rid);
            }
            k += 1;
        }
        v
    }

    /// 未回放清单。
    pub fn unplayed(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.items.len() {
            if !self.items[k].replayed {
                v.push(self.items[k].rid);
            }
            k += 1;
        }
        v
    }

    /// 回归保护就位：项数达锚、全回放、零漂移。
    pub fn guarded(&self) -> bool {
        self.items.len() >= ITEMS_NEEDED
            && self.unplayed().is_empty()
            && self.drift_list().is_empty()
    }

    pub fn arm(&mut self) {
        self.armed = true;
    }

    /// 回归集指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0x1b4001b40;
        h = h.wrapping_mul(0x100000001b3).wrapping_add(self.items.len() as u64);
        let mut k = 0usize;
        while k < self.items.len() {
            let it = &self.items[k];
            h = h.wrapping_mul(0x100000001b3).wrapping_add(it.rid as u64);
            h = h.wrapping_mul(0x100000001b3).wrapping_add(it.eno as u64);
            h = h.wrapping_mul(0x100000001b3).wrapping_add(it.baseline);
            h = h.wrapping_mul(0x100000001b3).wrapping_add(it.last);
            k += 1;
        }
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut g = GateRegression::new();
    // 1) 零号与坏要素号必拒
    got[0] = g.enroll(0, 1, "零项").err() == Some(GR_ZERO)
        && g.enroll(1, 6, "越号").err() == Some(GR_ENO);
    // 2) 重项必拒
    let _ = g.enroll(1, 1, "甲项");
    got[1] = g.enroll(1, 2, "重项").err() == Some(GR_DUP);
    // 3) 未知项回放必拒
    got[2] = g.replay(99, "x").err() == Some(GR_NORID);
    // 4) 漂移名必列漂移
    let _ = g.enroll(2, 3, "乙项原名");
    let drift = g.replay(2, "乙项改后");
    got[3] = drift == Ok(false) && g.drift_list() == [2u32];
    got
}

/// 批内判据。
pub fn run_unx10332_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10332");

    // J1 正路径：八释放项入集同位回放零漂移
    let mut g = GateRegression::new();
    let mut ok = true;
    let mut n = 1u32;
    while n <= ITEMS_NEEDED as u32 {
        let nm = format!("释放项{}", n);
        if g.enroll(n, ((n - 1) % 5) + 1, &nm).is_err() {
            ok = false;
        }
        n += 1;
    }
    n = 1;
    while n <= ITEMS_NEEDED as u32 {
        let nm = format!("释放项{}", n);
        match g.replay(n, &nm) {
            Ok(true) => {}
            _ => ok = false,
        }
        n += 1;
    }
    s.add(
        "J1-释放项回归集",
        ok && g.guarded() && g.drift_list().is_empty(),
        "八释放项入集同位回放零漂移全就位",
    );

    // J2 基线指纹与名指纹一致
    let j2 = g.items[0].baseline == name_digest("释放项1");
    s.add("J2-基线对名", j2, "基线指纹即名指纹");

    // J3 注入检出：零号坏号 + 重项
    let inj = inject_four();
    s.add("J3-零号重项检出", inj[0] && inj[1], "零项号与坏要素号及重项必拒");

    // J4 注入检出：未知回放 + 漂移入单
    s.add("J4-未知漂移检出", inj[2] && inj[3], "未知项回放必拒且漂移必列单");

    // J5 重放必拒
    let j5 = {
        let mut t = GateRegression::new();
        let _ = t.enroll(1, 1, "重放项");
        let _ = t.replay(1, "重放项");
        t.replay(1, "重放项").err() == Some(GR_REREPLAY)
    };
    s.add("J5-重放拒收", j5, "一项只许回放一次");

    // J6 缺项不成保护
    let j6 = {
        let mut t = GateRegression::new();
        let mut q = 1u32;
        while q < ITEMS_NEEDED as u32 {
            let nm = format!("短集{}", q);
            let _ = t.enroll(q, 1, &nm);
            let _ = t.replay(q, &nm);
            q += 1;
        }
        !t.guarded()
    };
    s.add("J6-短集不过", j6, "项数不达锚即不成保护");

    // J7 未回放不成保护
    let j7 = {
        let mut t = GateRegression::new();
        let mut q = 1u32;
        while q <= ITEMS_NEEDED as u32 {
            let nm = format!("未放{}", q);
            let _ = t.enroll(q, 1, &nm);
            q += 1;
        }
        !t.guarded() && t.unplayed().len() == ITEMS_NEEDED
    };
    s.add("J7-未放不成", j7, "存在未回放项即不成保护");

    // J8 空项名必拒
    let j8 = GateRegression::new().enroll(1, 1, "").err() == Some(GR_ZERO);
    s.add("J8-空名拒收", j8, "项名空必拒");

    // J9 容量边界
    let j9 = {
        let mut t = GateRegression::new();
        let mut q = 1u32;
        let mut filled = true;
        while q <= CAP as u32 {
            let nm = format!("容项{}", q);
            if t.enroll(q, 1, &nm).is_err() {
                filled = false;
            }
            q += 1;
        }
        filled && t.enroll(999, 1, "超").err() == Some(GR_CAP)
    };
    s.add("J9-容量边界", j9, "超容必拒且原集不损");

    // J10 指纹确定性：同名同指纹异名异指纹
    let j10 = name_digest("同") == name_digest("同") && name_digest("甲") != name_digest("乙");
    s.add("J10-指纹确定", j10, "名指纹同名必等异名必异");

    // J11 集指纹随回放推进且终态稳定
    let j11 = {
        let mut t = GateRegression::new();
        let _ = t.enroll(1, 1, "指项");
        let f0 = t.fold();
        let _ = t.replay(1, "指项");
        let f1 = t.fold();
        f0 != f1 && t.fold() == f1
    };
    s.add("J11-指纹推进", j11, "回放一步指纹一步且终态稳定");

    // J12 动态明细：漂移计数落账
    let dl = g.drift_list().len();
    s.add_dyn("J12-漂移计数", dl == 0, &format!("drift={}", dl));

    // J13 动态明细：回归集指纹落账
    let fp = g.fold();
    s.add_dyn("J13-回归指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10332_gate_regression_flow() {
        let mut g = GateRegression::new();
        assert!(g.enroll(0, 1, "零").err() == Some(GR_ZERO));
        assert!(g.enroll(1, 0, "零号").err() == Some(GR_ENO));
        assert!(g.enroll(1, 6, "越").err() == Some(GR_ENO));
        assert!(g.enroll(1, 1, "").err() == Some(GR_ZERO));
        assert!(g.enroll(1, 1, "甲").is_ok());
        assert!(g.enroll(1, 1, "重").err() == Some(GR_DUP));
        assert!(g.replay(99, "x").err() == Some(GR_NORID));
        assert!(g.replay(1, "甲").unwrap_or(false));
        assert!(g.replay(1, "甲").err() == Some(GR_REREPLAY));
        assert!(g.items[0].baseline == name_digest("甲"));
        assert!(!g.items[0].drifted());
        assert!(!g.guarded());
        g.arm();
        assert!(g.enroll(2, 1, "乙").err() == Some(GR_REREPLAY));
    }

    #[test]
    fn unx10332_checks_green() {
        let s = run_unx10332_checks();
        assert!(s.all_passed(), "UNX-F10332 判据未全绿");
    }
}
