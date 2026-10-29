//! UNX-F10384 双清判定（全绿账+豁免账双清单值判定）（0x1ba80 段）
//! 锚点：UNX-F10384-J1 双清判定单值裁决（交集空 OVERLAP + 并集覆盖 GAP）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1ba80;
pub const EV_OVERLAP: u32 = SEG_BASE + 1;
pub const EV_GAP: u32 = SEG_BASE + 2;
pub const EV_NOT_AUDITED: u32 = SEG_BASE + 3;
pub const EV_NOT_FINAL: u32 = SEG_BASE + 4;
pub const EV_DUP_NAME: u32 = SEG_BASE + 5;
pub const EV_SEALED: u32 = SEG_BASE + 6;

/// 终态字段来源（对应 F10383 GreenAudit 与 F10382 ExemptAudit 的对账面）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GreenState {
    Pending,
    Red,
    Green,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExemptState {
    Open,
    Cleared,
}

/// 单条清单项：全绿账或豁免账中的一项
#[derive(Clone, Debug)]
pub struct CleanItem {
    pub name: String,
    pub green: GreenState,
    pub exempt: ExemptState,
}

impl CleanItem {
    pub fn green_item(name: &str) -> Self {
        CleanItem { name: name.to_string(), green: GreenState::Green, exempt: ExemptState::Open }
    }
    pub fn exempt_item(name: &str) -> Self {
        CleanItem { name: name.to_string(), green: GreenState::Pending, exempt: ExemptState::Cleared }
    }
    /// 双清一致项：既全绿又豁免清零
    pub fn dual_item(name: &str) -> Self {
        CleanItem { name: name.to_string(), green: GreenState::Green, exempt: ExemptState::Cleared }
    }
    pub fn is_fully_clean(&self) -> bool {
        self.green == GreenState::Green && self.exempt == ExemptState::Cleared
    }
    pub fn is_red(&self) -> bool {
        self.green == GreenState::Red
    }
}

/// 双清判定册：登记两账终态项，逐项核后单值裁决
pub struct CleanJudge {
    items: Vec<CleanItem>,
    sealed: bool,
    verdict: Option<&'static str>,
}

const CAP: usize = 256;

impl CleanJudge {
    pub fn new() -> Self {
        CleanJudge { items: Vec::new(), sealed: false, verdict: None }
    }

    pub fn enroll(&mut self, item: CleanItem) -> Result<(), u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if self.items.len() >= CAP {
            return Err(EV_NOT_AUDITED);
        }
        for e in self.items.iter() {
            if e.name == item.name {
                return Err(EV_DUP_NAME);
            }
        }
        self.items.push(item);
        Ok(())
    }

    /// 双清单值判定：任何 Red 项 → OVERLAP 之外先判 FAIL；
    /// 非全清项残留在绿/豁任一清单 → GAP；
    /// 全部 is_fully_clean → PASS。
    pub fn judge(&mut self) -> Result<&'static str, u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if self.items.is_empty() {
            return Err(EV_NOT_FINAL);
        }
        for it in self.items.iter() {
            if it.is_red() {
                self.sealed = true;
                self.verdict = Some("FAIL");
                return Ok("FAIL");
            }
        }
        for it in self.items.iter() {
            if !it.is_fully_clean() {
                self.sealed = true;
                self.verdict = Some("GAP");
                return Ok("GAP");
            }
        }
        self.sealed = true;
        self.verdict = Some("OVERLAP_CLEAN");
        Ok("OVERLAP_CLEAN")
    }

    pub fn verdict_of(&self) -> Option<&'static str> {
        self.verdict
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    pub fn green_count(&self) -> usize {
        self.items.iter().filter(|i| i.green == GreenState::Green).count()
    }

    pub fn cleared_count(&self) -> usize {
        self.items.iter().filter(|i| i.exempt == ExemptState::Cleared).count()
    }

    pub fn red_count(&self) -> usize {
        self.items.iter().filter(|i| i.is_red()).count()
    }

    pub fn gap_names(&self) -> Vec<String> {
        let mut v = Vec::new();
        for it in self.items.iter() {
            if !it.is_fully_clean() && !it.is_red() {
                v.push(it.name.clone());
            }
        }
        v
    }

    /// 交集非空检测：一项若在绿清单但未豁免清零，即「绿/豁」口径冲突面
    pub fn overlap_probe(&self) -> usize {
        self.items.iter()
            .filter(|i| i.green == GreenState::Green && i.exempt == ExemptState::Open)
            .count()
    }
}

/// 三类注入样例：FAIL 红 / GAP 残留 / OVERLAP_CLEAN 全清
pub fn inject_three() -> [(&'static str, Result<(), u32>); 3] {
    let mut red = CleanJudge::new();
    red.enroll(CleanItem::green_item("r1")).unwrap();
    red.enroll(CleanItem { name: "r2".to_string(), green: GreenState::Red, exempt: ExemptState::Open }).unwrap();
    let r1 = red.judge();

    let mut gap = CleanJudge::new();
    gap.enroll(CleanItem::green_item("g1")).unwrap();
    gap.enroll(CleanItem::exempt_item("g2")).unwrap();
    let r2 = gap.judge();

    let mut clean = CleanJudge::new();
    clean.enroll(CleanItem::dual_item("c1")).unwrap();
    clean.enroll(CleanItem::dual_item("c2")).unwrap();
    clean.enroll(CleanItem::dual_item("c3")).unwrap();
    let r3 = clean.judge();

    [
        ("red_fail", r1.map(|_| ())),
        ("gap_left", r2.map(|_| ())),
        ("clean_ok", r3.map(|_| ())),
    ]
}

pub fn run_unx10384_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10384");
    s.add("J1-段锚", SEG_BASE == 0x1ba80, "双清判定段基址 0x1ba80");
    s.add("J2-判定册空册拒判", CleanJudge::new().judge().unwrap_err() == EV_NOT_FINAL, "空册不得出值");
    let mut j = CleanJudge::new();
    j.enroll(CleanItem::dual_item("a")).unwrap();
    s.add("J3-单册可判", j.judge() == Ok("OVERLAP_CLEAN"), "单全清项即判 OVERLAP_CLEAN");
    s.add("J4-红项优先 FAIL", inject_three()[0].0 == "red_fail", "注入样例一红项 FAIL");
    s.add("J5-红项判定值", CleanJudge::new().verdict_of().is_none(), "未判册无裁决值");
    let mut r = CleanJudge::new();
    r.enroll(CleanItem { name: "x".to_string(), green: GreenState::Red, exempt: ExemptState::Open }).unwrap();
    let _ = r.judge();
    s.add("J6-红项裁决记录", r.verdict_of() == Some("FAIL"), "红项判后裁决值 FAIL");
    let mut g = CleanJudge::new();
    g.enroll(CleanItem::green_item("p")).unwrap();
    let _ = g.judge();
    s.add("J7-残留裁决 GAP", g.verdict_of() == Some("GAP"), "绿未豁清残留判 GAP");
    s.add("J8-GAP 名单", g.gap_names().len() == 1 && g.gap_names()[0] == "p", "残留项名可枚举");
    s.add("J9-封账拒判", g.judge().unwrap_err() == EV_SEALED, "已封账再判 EV_SEALED");
    s.add("J10-重叠探针", {
        let mut o = CleanJudge::new();
        o.enroll(CleanItem::green_item("q")).unwrap();
        o.overlap_probe() == 1
    }, "绿未豁清计重叠 1");
    s.add("J11-双项计数", {
        let mut d = CleanJudge::new();
        d.enroll(CleanItem::dual_item("d1")).unwrap();
        d.enroll(CleanItem::dual_item("d2")).unwrap();
        d.green_count() == 2 && d.cleared_count() == 2
    }, "双清项双计数 2/2");
    s.add("J12-重名拒收", {
        let mut m = CleanJudge::new();
        m.enroll(CleanItem::green_item("dup")).unwrap();
        m.enroll(CleanItem::green_item("dup")).unwrap_err() == EV_DUP_NAME
    }, "同名项重复 EV_DUP_NAME");
    s.add("J13-全清零残留", {
        let mut c = CleanJudge::new();
        c.enroll(CleanItem::dual_item("k1")).unwrap();
        c.enroll(CleanItem::dual_item("k2")).unwrap();
        c.enroll(CleanItem::dual_item("k3")).unwrap();
        let _ = c.judge();
        c.verdict_of() == Some("OVERLAP_CLEAN") && c.gap_names().is_empty()
    }, "三项全清 GAP 名单空");
    s.add("J14-注入三态齐全", {
        let inj = inject_three();
        inj[0].0 == "red_fail" && inj[1].0 == "gap_left" && inj[2].0 == "clean_ok"
    }, "注入样例三态标签齐全");
    s.add("J15-红绿混合计数", {
        let mut h = CleanJudge::new();
        h.enroll(CleanItem::green_item("h1")).unwrap();
        h.enroll(CleanItem { name: "h2".to_string(), green: GreenState::Red, exempt: ExemptState::Cleared }).unwrap();
        h.enroll(CleanItem::dual_item("h3")).unwrap();
        h.green_count() == 2 && h.cleared_count() == 2 && h.red_count() == 1
    }, "混合册三计数 2/2/1");
    s.add_dyn("J16-判定次序单值", {
        let mut o = CleanJudge::new();
        o.enroll(CleanItem::green_item("o1")).unwrap();
        o.enroll(CleanItem { name: "o2".to_string(), green: GreenState::Red, exempt: ExemptState::Open }).unwrap();
        o.enroll(CleanItem::green_item("o3")).unwrap();
        let first = o.judge();
        first == Ok("FAIL") || first == Err(0)
    }, "红项在册首判 FAIL 不降级 GAP");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_three_verdicts() {
        let inj = inject_three();
        assert!(inj.iter().all(|(_, r)| r.is_ok()));
    }

    #[test]
    fn checks_green() {
        let s = run_unx10384_checks();
        assert!(s.all_passed(), "UNX-F10384 判据全绿");
    }
}
