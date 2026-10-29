//! UNX-F11219 嫁接源版本钉定与跟随协议（0x1f819 段）
//!
//! 锚点：UNX-F11219-J1 四件嫁接源版本/哈希/许可三列登记入 AI-94 台账，
//! 跟随 SLA（重大变更 7 日内评估）协议落账。
//! 域：CoRun·UNX-C5-B01——钉定不是一次性动作：上游重大变更须在七日内
//! 完成评估并落结论，逾期或未评估的台账不得冻存。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f819;
pub const GS_EMPTY: u32 = SEG_BASE + 0x01; // 三列有空
pub const GS_DUP: u32 = SEG_BASE + 0x02; // 源重录
pub const GS_SLA: u32 = SEG_BASE + 0x03; // 超出跟随窗口
pub const GS_ORDER: u32 = SEG_BASE + 0x04; // 源数量不足
pub const GS_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后改写
pub const GS_OVER: u32 = SEG_BASE + 0x06; // 超容
pub const GS_EVAL: u32 = SEG_BASE + 0x07; // 未评估即冻存

/// 跟随 SLA 天数（重大变更评估窗口）。
pub const SLA_DAYS: u32 = 7;
/// 台账至少四件嫁接源。
pub const MIN_SOURCES: usize = 4;
pub const CAP: usize = 10;
pub const GR_TAG: u64 = 0x1f819d1;
pub const FP_STEP: u64 = 0x100000001b3;

/// 一件嫁接源的三列：版本、哈希、许可。
#[derive(Clone, Debug)]
pub struct Source {
    pub name: &'static str,
    pub ver: &'static str,
    pub hash: &'static str,
    pub lic: &'static str,
}

impl Source {
    pub fn complete(&self) -> bool {
        !self.name.is_empty() && !self.ver.is_empty() && !self.hash.is_empty() && !self.lic.is_empty()
    }

    pub fn hash_shaped(&self) -> bool {
        let b = self.hash.as_bytes();
        b.len() == 16 && b.iter().all(|c| c.is_ascii_hexdigit())
    }

    pub fn render(&self) -> String {
        format!("{} {} {} {}", self.name, self.ver, self.hash, self.lic)
    }
}

/// 跟随评估状态。
#[derive(Clone, Debug)]
pub struct Follow {
    pub days_since: u32,
    pub evaluated: bool,
    pub note: &'static str,
}

impl Follow {
    pub fn within_sla(&self) -> bool {
        self.days_since <= SLA_DAYS
    }

    pub fn verdict(&self) -> bool {
        self.within_sla() && self.evaluated
    }
}

/// AI-94 台账。
#[derive(Clone, Debug)]
pub struct GraftLedger {
    pub items: Vec<Source>,
    pub follow: Follow,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl GraftLedger {
    pub fn new() -> GraftLedger {
        GraftLedger {
            items: Vec::new(),
            follow: Follow { days_since: 0, evaluated: false, note: "probe" },
            frozen: false,
            root: None,
        }
    }

    /// 登一件源：冻结/空列/重录/超容四路拒。
    pub fn add(&mut self, s: Source) -> Result<(), u32> {
        if self.frozen {
            return Err(GS_FROZEN);
        }
        if !s.complete() {
            return Err(GS_EMPTY);
        }
        if self.items.iter().any(|x| x.name == s.name) {
            return Err(GS_DUP);
        }
        if self.items.len() >= CAP {
            return Err(GS_OVER);
        }
        self.items.push(s);
        Ok(())
    }

    /// 上游变更申报：按天数计进入评估窗口。
    pub fn note_change(&mut self, days: u32, note: &'static str) -> Result<(), u32> {
        if self.frozen {
            return Err(GS_FROZEN);
        }
        if days > SLA_DAYS {
            return Err(GS_SLA);
        }
        self.follow.days_since = days;
        self.follow.evaluated = false;
        self.follow.note = note;
        Ok(())
    }

    /// 评估落结论。
    pub fn evaluate(&mut self, ok: bool) -> Result<(), u32> {
        if self.frozen {
            return Err(GS_FROZEN);
        }
        if !self.follow.within_sla() {
            return Err(GS_SLA);
        }
        self.follow.evaluated = ok;
        Ok(())
    }

    /// 三列全形（哈希十六位）。
    pub fn all_shaped(&self) -> bool {
        self.items.len() >= MIN_SOURCES && self.items.iter().all(|s| s.complete() && s.hash_shaped())
    }

    /// 冻存：源数量、三列、跟随窗口与评估结论四门全过。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(GS_FROZEN);
        }
        if self.items.len() < MIN_SOURCES {
            return Err(GS_ORDER);
        }
        if !self.all_shaped() {
            return Err(GS_EMPTY);
        }
        if !self.follow.within_sla() {
            return Err(GS_SLA);
        }
        if !self.follow.evaluated {
            return Err(GS_EVAL);
        }
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        Ok(r)
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = GR_TAG;
        let mut k = 0usize;
        while k < self.items.len() {
            let s = &self.items[k];
            let mut f = 0usize;
            let fields = [s.name.as_bytes(), s.ver.as_bytes(), s.hash.as_bytes(), s.lic.as_bytes()];
            while f < 4 {
                let b = fields[f];
                let mut i = 0usize;
                while i < b.len() {
                    h = (h ^ b[i] as u64).wrapping_mul(FP_STEP);
                    i += 1;
                }
                h = (h ^ 0x7c).wrapping_mul(FP_STEP);
                f += 1;
            }
            k += 1;
        }
        h = (h ^ self.follow.days_since as u64).wrapping_mul(FP_STEP);
        h = (h ^ self.follow.evaluated as u64).wrapping_mul(FP_STEP);
        h
    }
}

/// 四件嫁接源：版本、十六位哈希、许可。
pub const SOURCES: [Source; 4] = [
    Source { name: "glibc", ver: "2.39", hash: "9f2c4a17b0d3e561", lic: "LGPL-2.1-or-later" },
    Source { name: "musl", ver: "1.2.5", hash: "4d7e21c9a8f035b6", lic: "MIT" },
    Source { name: "busybox", ver: "1.36.1", hash: "c1a9e40d7b2f8635", lic: "GPL-2.0-only" },
    Source { name: "linux-headers", ver: "6.6.30", hash: "b6f0d95e3a7c2148", lic: "GPL-2.0-only WITH Linux-syscall-note" },
];

/// 组一张窗口内已评估的台账。
pub fn build() -> GraftLedger {
    let mut g = GraftLedger::new();
    let mut k = 0usize;
    while k < SOURCES.len() {
        let _ = g.add(SOURCES[k].clone());
        k += 1;
    }
    let _ = g.note_change(3, "musl 1.2.5 -> 1.2.6 提案");
    let _ = g.evaluate(true);
    g
}

/// 四路对抗：空哈希、重录、逾期申报、未评估冻存。
pub fn inject_four(g: &GraftLedger) -> [bool; 4] {
    let mut out = [false; 4];
    let mut a = build();
    out[0] = a.add(Source { name: "x", ver: "1", hash: "", lic: "MIT" }) == Err(GS_EMPTY);
    out[1] = a.add(Source { name: "musl", ver: "1.2.6", hash: "0000000000000000", lic: "MIT" }) == Err(GS_DUP);
    let mut b = build();
    out[2] = b.note_change(9, "超出七日的申报") == Err(GS_SLA);
    let mut c = GraftLedger::new();
    let mut k = 0usize;
    while k < SOURCES.len() {
        let _ = c.add(SOURCES[k].clone());
        k += 1;
    }
    let _ = c.note_change(2, "待评估");
    out[3] = c.freeze() == Err(GS_EVAL);
    let _ = g.follow.days_since;
    out
}

/// 判据入口。
pub fn run_unx11219_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11219");
    let mut g = build();
    let root = g.freeze().unwrap_or(0);

    s.add("J1-四源齐", g.items.len() == MIN_SOURCES, "AI-94 台账恰登四件嫁接源");
    s.add("J2-三列全形", g.items.iter().all(|x| x.complete()), "每件源版本/哈希/许可三列均非空");
    s.add("J3-哈希范式", g.items.iter().all(|x| x.hash_shaped()), "哈希为十六位十六进制");
    s.add("J4-版本钉定", g.items.iter().all(|x| x.ver.contains('.')), "四件版本号均为主次结构");
    s.add("J5-许可合规", g.items.iter().all(|x| !x.lic.is_empty()), "许可列逐件非空且登记在册");
    s.add("J6-窗口内", g.follow.within_sla(), "距上游变更三日，未超七日窗口");
    s.add("J7-评估已落", g.follow.evaluated, "跟随评估已落结论");
    s.add("J8-冻存根", root != 0 && g.root == Some(root), "冻存产出非零根并留账");

    s.add("J9-冻后加拒", g.add(SOURCES[0].clone()) == Err(GS_FROZEN), "冻存后新增源拒为 GS_FROZEN");
    s.add("J10-冻后申报拒", g.note_change(1, "迟报") == Err(GS_FROZEN), "冻存后变更申报拒为 GS_FROZEN");
    s.add("J11-逾期申报拒", {
        let mut x = build();
        x.note_change(8, "九日") == Err(GS_SLA)
    }, "超过七日的变更申报拒为 GS_SLA");

    let mut un = GraftLedger::new();
    let mut k = 0usize;
    while k < SOURCES.len() {
        let _ = un.add(SOURCES[k].clone());
        k += 1;
    }
    let _ = un.note_change(4, "未评估");
    s.add("J12-未评估禁冻", un.freeze() == Err(GS_EVAL), "评估结论未落即冻存拒为 GS_EVAL");

    let four = inject_four(&g);
    s.add("J13-四路注入", four.iter().all(|x| *x), "空哈希/重录/逾期/未评估冻存四路全拒");
    s.add_dyn(
        "J14-台账明细",
        g.items.len() == 4 && g.items.iter().all(|x| x.hash.len() == 16),
        &format!("源 {} 件，窗口 {} 日，根 0x{:x}", g.items.len(), g.follow.days_since, root),
    );
    s.add_dyn(
        "J15-根稳定",
        root == g.fold(),
        &format!("冻存根 0x{:x} 与重折叠一致", g.fold()),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11219_graft_source_flow() {
        let mut g = build();
        assert_eq!(g.items.len(), 4);
        assert!(g.follow.verdict());
        let r = g.freeze().unwrap_or(0);
        assert!(r != 0);
        assert!(g.frozen);
        assert_eq!(g.add(SOURCES[0].clone()), Err(GS_FROZEN));
        let mut stale = build();
        assert_eq!(stale.note_change(8, "late"), Err(GS_SLA));
    }

    #[test]
    fn unx11219_checks_green() {
        let s = run_unx11219_checks();
        assert!(s.all_passed(), "UNX-F11219 判据未全绿");
    }
}
