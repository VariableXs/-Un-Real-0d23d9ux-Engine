//! UNX-F11125 B07 清账（行规程与录制基线终重放）（0x1f5a5 段）
//!
//! 行规程批 B07 共 20 条判据，分属三套基线：规范、原始、半原始。每条判据
//! 记录基线与观测到的回显/规范行为，终重放时与基线期望逐条对照。

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址：F11125 独占 0x1f5a5。
pub const SEG_BASE: u32 = 0x1f5a5;

/// 判据号越出 1..=20。
pub const LC_NO: u32 = SEG_BASE + 0x01;
/// 基线号越出 1..=3。
pub const LC_BASE: u32 = SEG_BASE + 0x02;
/// 判据重复登记。
pub const LC_DUP: u32 = SEG_BASE + 0x03;
/// 判据号不连续。
pub const LC_ORDER: u32 = SEG_BASE + 0x04;
/// 观测行为偏离基线期望。
pub const LC_DRIFT: u32 = SEG_BASE + 0x05;
/// 判据重复重放。
pub const LC_REPLAY: u32 = SEG_BASE + 0x06;
/// 清账账已冻结。
pub const LC_FROZEN: u32 = SEG_BASE + 0x07;
/// 判据条数不足。
pub const LC_THIN: u32 = SEG_BASE + 0x08;

/// 判据总数。
pub const CRITS: usize = 20;
/// 基线数。
pub const BASELINES: usize = 3;
/// 规范基线号。
pub const BASE_CANON: u8 = 1;
/// 原始基线号。
pub const BASE_RAW: u8 = 2;
/// 半原始基线号。
pub const BASE_CBREAK: u8 = 3;
/// 折叠种子。
const LC_TAG: u64 = 0x0000_1f5_a5a5;
/// FNV-1a 素数。
const FP_STEP: u64 = 0x100000001b3;

/// 行规程判据。
#[derive(Clone, Debug)]
pub struct LineCase {
    /// 判据号（1..=20）。
    pub no: usize,
    /// 基线号。
    pub baseline: u8,
    /// 观测到的回显行为。
    pub echo: bool,
    /// 观测到的规范行为。
    pub canon: bool,
    /// 是否已终重放。
    pub replayed: bool,
}

impl LineCase {
    /// 基线期望：返回（回显，规范）。
    pub fn expected(baseline: u8) -> (bool, bool) {
        match baseline {
            BASE_CANON => (true, true),
            BASE_RAW => (false, false),
            BASE_CBREAK => (true, false),
            _ => (false, false),
        }
    }

    /// 观测是否与基线期望一致。
    pub fn matched(&self) -> bool {
        let (e, c) = Self::expected(self.baseline);
        self.echo == e && self.canon == c
    }

    /// 判据是否闭合。
    pub fn settled(&self) -> bool {
        self.matched() && self.replayed
    }

    /// 基线名。
    pub fn base_name(&self) -> String {
        let name = match self.baseline {
            BASE_CANON => "CANON",
            BASE_RAW => "RAW",
            BASE_CBREAK => "CBREAK",
            _ => "BAD",
        };
        String::from(name)
    }

    /// 判据渲染，形如 L07|RAW|E0C0|R。
    pub fn render(&self) -> String {
        let e = if self.echo { 1 } else { 0 };
        let c = if self.canon { 1 } else { 0 };
        let r = if self.replayed { "R" } else { "N" };
        format!("L{:02}|{}|E{}C{}|{}", self.no, self.base_name(), e, c, r)
    }
}

/// 行规程清账账。
#[derive(Clone, Debug)]
pub struct LineClear {
    cases: Vec<LineCase>,
    frozen: bool,
    root: u64,
}

impl LineClear {
    /// 建空账。
    pub fn new() -> Self {
        Self { cases: Vec::new(), frozen: false, root: 0 }
    }

    /// 定位判据下标。
    fn index_of(&self, no: usize) -> usize {
        let mut idx = usize::MAX;
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].no == no {
                idx = k;
            }
            k = k.wrapping_add(1);
        }
        idx
    }

    /// 登记判据：号、基线、重复、顺序、容量、冻结六道闸。
    pub fn file(&mut self, no: usize, baseline: u8, echo: bool, canon: bool) -> Result<(), u32> {
        if self.frozen {
            return Err(LC_FROZEN);
        }
        if no == 0 || no > CRITS {
            return Err(LC_NO);
        }
        if baseline < BASE_CANON || baseline > BASE_CBREAK {
            return Err(LC_BASE);
        }
        if self.index_of(no) != usize::MAX {
            return Err(LC_DUP);
        }
        if self.cases.len().wrapping_add(1) != no {
            return Err(LC_ORDER);
        }
        self.cases.push(LineCase { no, baseline, echo, canon, replayed: false });
        Ok(())
    }

    /// 终重放一条判据：观测必须与基线一致。
    pub fn replay(&mut self, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(LC_FROZEN);
        }
        let idx = self.index_of(no);
        if idx == usize::MAX {
            return Err(LC_NO);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            if !c.matched() {
                return Err(LC_DRIFT);
            }
            if c.replayed {
                return Err(LC_REPLAY);
            }
            c.replayed = true;
            return Ok(());
        }
        Err(LC_NO)
    }

    /// 判据条数。
    pub fn case_count(&self) -> usize {
        self.cases.len()
    }

    /// 已重放条数。
    pub fn replayed(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].replayed {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 某基线判据数。
    pub fn in_baseline(&self, baseline: u8) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].baseline == baseline {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 某基线是否闭合。
    pub fn baseline_closed(&self, baseline: u8) -> bool {
        if self.in_baseline(baseline) == 0 {
            return false;
        }
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].baseline == baseline && !self.cases[k].settled() {
                return false;
            }
            k = k.wrapping_add(1);
        }
        true
    }

    /// 三基线是否全部闭合。
    pub fn baselines_closed(&self) -> bool {
        self.baseline_closed(BASE_CANON)
            && self.baseline_closed(BASE_RAW)
            && self.baseline_closed(BASE_CBREAK)
    }

    /// 是否全部闭合。
    pub fn closed(&self) -> bool {
        self.cases.len() == CRITS && self.replayed() == CRITS && self.baselines_closed()
    }

    /// 冻结：返回折叠根。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.root = self.fold();
        self.root
    }

    /// 已冻结否。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// FNV-1a 折叠。
    pub fn fold(&self) -> u64 {
        let mut h = LC_TAG;
        let mut k = 0usize;
        while k < self.cases.len() {
            let c = &self.cases[k];
            h = (h ^ c.no as u64).wrapping_mul(FP_STEP);
            h = (h ^ c.baseline as u64).wrapping_mul(FP_STEP);
            let e = if c.echo { 1u64 } else { 2u64 };
            h = (h ^ e).wrapping_mul(FP_STEP);
            let n = if c.canon { 3u64 } else { 4u64 };
            h = (h ^ n).wrapping_mul(FP_STEP);
            k = k.wrapping_add(1);
        }
        h
    }
}

/// 标准行规程清账账：二十条判据按三套基线轮转并全部重放。
pub fn build_line() -> LineClear {
    let mut book = LineClear::new();
    let mut no = 1usize;
    while no <= CRITS {
        let slot = no.saturating_sub(1) % BASELINES;
        let baseline = if slot == 0 {
            BASE_CANON
        } else if slot == 1 {
            BASE_RAW
        } else {
            BASE_CBREAK
        };
        let (echo, canon) = LineCase::expected(baseline);
        let _ = book.file(no, baseline, echo, canon);
        let _ = book.replay(no);
        no = no.wrapping_add(1);
    }
    book
}

/// 四路对抗：判据号越界、基线越界、重复登记、观测漂移。
pub fn inject_four() -> [bool; 4] {
    let mut a_book = LineClear::new();
    let a = a_book.file(0, BASE_CANON, true, true) == Err(LC_NO)
        && a_book.file(21, BASE_CANON, true, true) == Err(LC_NO);
    let mut b_book = LineClear::new();
    let b = b_book.file(1, 4, true, true) == Err(LC_BASE) && b_book.case_count() == 0;
    let mut c_book = LineClear::new();
    let _ = c_book.file(1, BASE_CANON, true, true);
    let c = c_book.file(1, BASE_RAW, false, false) == Err(LC_DUP) && c_book.case_count() == 1;
    let mut d_book = LineClear::new();
    let _ = d_book.file(1, BASE_RAW, true, false);
    let d = d_book.replay(1) == Err(LC_DRIFT) && d_book.replayed() == 0;
    [a, b, c, d]
}

/// F11125 全部检查：行规程判据终重放与三基线对照。
pub fn run_unx11125_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11125 B07 清账");
    let book = build_line();
    let four = inject_four();

    // J1 二十条在册。
    s.add(
        "J1-判据在册",
        book.case_count() == CRITS && CRITS == 20,
        "B07 行规程批 20 条判据全部在册",
    );
    // J2 终重放全量。
    s.add("J2-终重放", book.replayed() == CRITS, "二十条判据全部完成终重放");
    // J3 三基线对照。
    s.add(
        "J3-三基线对照",
        book.baselines_closed() && book.in_baseline(BASE_CANON) > 0
            && book.in_baseline(BASE_RAW) > 0
            && book.in_baseline(BASE_CBREAK) > 0,
        "规范、原始、半原始三套基线各有用例且逐条对照闭合",
    );
    // J4 基线期望。
    s.add(
        "J4-基线期望",
        LineCase::expected(BASE_CANON) == (true, true)
            && LineCase::expected(BASE_RAW) == (false, false)
            && LineCase::expected(BASE_CBREAK) == (true, false),
        "规范回显且规范，原始不回显不规范，半原始回显不规范",
    );
    // J5 判据号闸。
    let mut no_probe = LineClear::new();
    s.add(
        "J5-判据号闸",
        no_probe.file(0, BASE_CANON, true, true) == Err(LC_NO)
            && no_probe.file(21, BASE_CANON, true, true) == Err(LC_NO),
        "判据号 0 与 21 越出 1..=20，登记报 LC_NO",
    );
    // J6 基线闸。
    let mut base_probe = LineClear::new();
    s.add(
        "J6-基线闸",
        base_probe.file(1, 0, false, false) == Err(LC_BASE)
            && base_probe.file(1, 4, false, false) == Err(LC_BASE),
        "基线号 0 与 4 越出 1..=3，登记报 LC_BASE",
    );
    // J7 漂移闸。
    let mut drift = LineClear::new();
    let _ = drift.file(1, BASE_CANON, false, true);
    s.add(
        "J7-漂移闸",
        drift.replay(1) == Err(LC_DRIFT) && drift.replayed() == 0,
        "规范基线观测到不回显时终重放报 LC_DRIFT",
    );
    // J8 重号闸。
    let mut dup_probe = LineClear::new();
    let _ = dup_probe.file(1, BASE_CANON, true, true);
    s.add(
        "J8-重号闸",
        dup_probe.file(1, BASE_RAW, false, false) == Err(LC_DUP),
        "同一判据号二次登记报 LC_DUP",
    );
    // J9 重复重放闸。
    let mut twice = LineClear::new();
    let _ = twice.file(1, BASE_RAW, false, false);
    let first_ok = twice.replay(1).is_ok();
    s.add(
        "J9-重放闸",
        first_ok && twice.replay(1) == Err(LC_REPLAY) && twice.replayed() == 1,
        "同一条判据二次终重放报 LC_REPLAY",
    );
    // J10 闭合断言。
    s.add("J10-闭合", book.closed(), "二十条判据逐条闭合，B07 清账账闭合");
    // J11 冻结拒写。
    let mut sealed = build_line();
    let root = sealed.freeze();
    s.add(
        "J11-冻结拒写",
        sealed.is_frozen()
            && sealed.file(1, BASE_CANON, true, true) == Err(LC_FROZEN)
            && sealed.replay(1) == Err(LC_FROZEN)
            && sealed.fold() == root,
        "冻结后登记与重放均报 LC_FROZEN，根保持不变",
    );
    // J12 四路对抗。
    s.add(
        "J12-四路对抗",
        four[0] && four[1] && four[2] && four[3],
        "判据号、基线、重号、漂移四路全部被拒",
    );
    // J13 判据渲染。
    let case = LineCase {
        no: 7,
        baseline: BASE_CBREAK,
        echo: true,
        canon: false,
        replayed: true,
    };
    s.add(
        "J13-判据渲染",
        case.render() == "L07|CBREAK|E1C0|R" && case.matched(),
        "半原始第 7 条判据渲染为 L07|CBREAK|E1C0|R",
    );
    // J14 动态明细。
    s.add_dyn(
        "J14-清账明细",
        book.closed(),
        &format!(
            "条={} 重放={} 规范={} 原始={}",
            book.case_count(),
            book.replayed(),
            book.in_baseline(BASE_CANON),
            book.in_baseline(BASE_RAW)
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11125_line_flow() {
        let mut book = build_line();
        assert_eq!(book.case_count(), CRITS);
        assert_eq!(book.replayed(), CRITS);
        assert!(book.baselines_closed());
        assert!(book.closed());
        let root = book.freeze();
        assert_eq!(book.fold(), root);
        assert_eq!(book.replay(1), Err(LC_FROZEN));
    }

    #[test]
    fn unx11125_checks_green() {
        let s = run_unx11125_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
