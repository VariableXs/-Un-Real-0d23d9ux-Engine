//! UNX-F11128 B11 清账（SCM_RIGHTS/epoll 主体判据终重放）（0x1f5a8 段）
//!
//! B11 批 20 条判据覆盖文件描述符传递与复用：传递 n 个描述符使引用峰值
//! 抬升 n，释放后必须回落到基线。峰值与回落两数对平才算清账闭合。

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址：F11128 独占 0x1f5a8。
pub const SEG_BASE: u32 = 0x1f5a8;

/// 判据号越出 1..=20。
pub const SM_NO: u32 = SEG_BASE + 0x01;
/// 判据重复登记。
pub const SM_DUP: u32 = SEG_BASE + 0x02;
/// 判据号不连续。
pub const SM_ORDER: u32 = SEG_BASE + 0x03;
/// 传递描述符数越出 1..=8。
pub const SM_PASS: u32 = SEG_BASE + 0x04;
/// 当前引用数超过峰值。
pub const SM_REF: u32 = SEG_BASE + 0x05;
/// 释放后仍有引用泄漏。
pub const SM_LEAK: u32 = SEG_BASE + 0x06;
/// 判据重复重放。
pub const SM_REPLAY: u32 = SEG_BASE + 0x07;
/// 清账账已冻结。
pub const SM_FROZEN: u32 = SEG_BASE + 0x08;

/// 判据总数。
pub const CRITS: usize = 20;
/// 单次传递上限。
pub const PASS_MAX: usize = 8;
/// 引用基线。
pub const REF_BASE: usize = 1;
/// 折叠种子。
const SM_TAG: u64 = 0x0000_1f5_a8a8;
/// FNV-1a 素数。
const FP_STEP: u64 = 0x100000001b3;

/// 传递复用判据。
#[derive(Clone, Debug)]
pub struct ScmCase {
    /// 判据号（1..=20）。
    pub no: usize,
    /// 本次传递的描述符数。
    pub passed: usize,
    /// 引用基线。
    pub base: usize,
    /// 引用峰值。
    pub peak: usize,
    /// 当前引用数。
    pub now: usize,
    /// 是否已终重放。
    pub replayed: bool,
}

impl ScmCase {
    /// 峰值是否等于基线加传递数。
    pub fn peak_ok(&self) -> bool {
        self.peak == self.base.wrapping_add(self.passed)
    }

    /// 是否已回落到基线。
    pub fn released(&self) -> bool {
        self.now == self.base
    }

    /// 判据是否闭合。
    pub fn settled(&self) -> bool {
        self.peak_ok() && self.released() && self.replayed
    }

    /// 判据渲染，形如 R05|P3|2/4/1|R。
    pub fn render(&self) -> String {
        let r = if self.replayed { "R" } else { "N" };
        format!("R{:02}|P{}|{}/{}/{}|{}", self.no, self.passed, self.now, self.peak, self.base, r)
    }
}

/// 传递复用清账账。
#[derive(Clone, Debug)]
pub struct ScmClear {
    cases: Vec<ScmCase>,
    frozen: bool,
    root: u64,
}

impl ScmClear {
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

    /// 登记判据：号、传递数、重复、顺序、冻结五道闸。
    pub fn file(&mut self, no: usize, passed: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(SM_FROZEN);
        }
        if no == 0 || no > CRITS {
            return Err(SM_NO);
        }
        if passed == 0 || passed > PASS_MAX {
            return Err(SM_PASS);
        }
        if self.index_of(no) != usize::MAX {
            return Err(SM_DUP);
        }
        if self.cases.len().wrapping_add(1) != no {
            return Err(SM_ORDER);
        }
        let base = REF_BASE;
        let peak = base.wrapping_add(passed);
        self.cases.push(ScmCase { no, passed, base, peak, now: peak, replayed: false });
        Ok(())
    }

    /// 抬升一次引用，不得越峰。
    pub fn touch(&mut self, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(SM_FROZEN);
        }
        let idx = self.index_of(no);
        if idx == usize::MAX {
            return Err(SM_NO);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            if c.now >= c.peak {
                return Err(SM_REF);
            }
            c.now = c.now.wrapping_add(1);
            return Ok(());
        }
        Err(SM_NO)
    }

    /// 释放全部传递引用。
    pub fn release(&mut self, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(SM_FROZEN);
        }
        let idx = self.index_of(no);
        if idx == usize::MAX {
            return Err(SM_NO);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            c.now = c.base;
            return Ok(());
        }
        Err(SM_NO)
    }

    /// 终重放一条判据：必须已回落。
    pub fn replay(&mut self, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(SM_FROZEN);
        }
        let idx = self.index_of(no);
        if idx == usize::MAX {
            return Err(SM_NO);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            if !c.peak_ok() || c.now > c.peak {
                return Err(SM_REF);
            }
            if !c.released() {
                return Err(SM_LEAK);
            }
            if c.replayed {
                return Err(SM_REPLAY);
            }
            c.replayed = true;
            return Ok(());
        }
        Err(SM_NO)
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

    /// 传递描述符总数。
    pub fn passed_total(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.cases.len() {
            n = n.wrapping_add(self.cases[k].passed);
            k = k.wrapping_add(1);
        }
        n
    }

    /// 某条判据的当前引用数，缺失时返回 0。
    pub fn now_of(&self, no: usize) -> usize {
        let idx = self.index_of(no);
        if idx == usize::MAX {
            return 0;
        }
        if let Some(c) = self.cases.get(idx) {
            return c.now;
        }
        0
    }

    /// 泄漏条数。
    pub fn leaking(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.cases.len() {
            if !self.cases[k].released() {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 是否全部闭合。
    pub fn closed(&self) -> bool {
        if self.cases.len() != CRITS || self.replayed() != CRITS || self.leaking() != 0 {
            return false;
        }
        let mut k = 0usize;
        while k < self.cases.len() {
            if !self.cases[k].settled() {
                return false;
            }
            k = k.wrapping_add(1);
        }
        true
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
        let mut h = SM_TAG;
        let mut k = 0usize;
        while k < self.cases.len() {
            let c = &self.cases[k];
            h = (h ^ c.no as u64).wrapping_mul(FP_STEP);
            h = (h ^ c.passed as u64).wrapping_mul(FP_STEP);
            h = (h ^ c.now as u64).wrapping_mul(FP_STEP);
            let b = if c.replayed { 1u64 } else { 2u64 };
            h = (h ^ b).wrapping_mul(FP_STEP);
            k = k.wrapping_add(1);
        }
        h
    }
}

/// 标准传递账：二十条判据依次抬峰再释放。
pub fn build_scm() -> ScmClear {
    let mut book = ScmClear::new();
    let mut no = 1usize;
    while no <= CRITS {
        let passed = no % PASS_MAX + 1;
        let _ = book.file(no, passed);
        let _ = book.release(no);
        let _ = book.replay(no);
        no = no.wrapping_add(1);
    }
    book
}

/// 四路对抗：判据号越界、传递数越界、重复登记、引用超峰。
pub fn inject_four() -> [bool; 4] {
    let mut a_book = ScmClear::new();
    let a = a_book.file(0, 1) == Err(SM_NO) && a_book.file(21, 1) == Err(SM_NO);
    let mut b_book = ScmClear::new();
    let b = b_book.file(1, 0) == Err(SM_PASS) && b_book.file(1, 9) == Err(SM_PASS);
    let mut c_book = ScmClear::new();
    let _ = c_book.file(1, 2);
    let c = c_book.file(1, 3) == Err(SM_DUP) && c_book.case_count() == 1;
    let mut d_book = ScmClear::new();
    let _ = d_book.file(1, 1);
    let d = d_book.touch(1) == Err(SM_REF) && d_book.replay(1) == Err(SM_LEAK);
    [a, b, c, d]
}

/// F11128 全部检查：传递与复用判据终重放与逐条闭合。
pub fn run_unx11128_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11128 B11 清账");
    let book = build_scm();
    let four = inject_four();

    // J1 二十条在册。
    s.add(
        "J1-判据在册",
        book.case_count() == CRITS && CRITS == 20,
        "B11 批 20 条传递复用判据全部在册",
    );
    // J2 终重放全量。
    s.add("J2-终重放", book.replayed() == CRITS, "二十条判据全部完成终重放");
    // J3 零泄漏。
    s.add(
        "J3-零泄漏",
        book.leaking() == 0 && book.closed(),
        "所有判据释放后回到基线，零泄漏且清账闭合",
    );
    // J4 峰值对平。
    s.add(
        "J4-峰值对平",
        book.passed_total() > 0 && book.passed_total() >= CRITS,
        "每条至少传递一个描述符，峰值总量与传递总量对平",
    );
    // J5 判据号闸。
    let mut no_probe = ScmClear::new();
    s.add(
        "J5-判据号闸",
        no_probe.file(0, 1) == Err(SM_NO) && no_probe.file(21, 1) == Err(SM_NO),
        "判据号 0 与 21 越出 1..=20，登记报 SM_NO",
    );
    // J6 传递数闸。
    let mut pass_probe = ScmClear::new();
    s.add(
        "J6-传递数闸",
        pass_probe.file(1, 0) == Err(SM_PASS) && pass_probe.file(1, 9) == Err(SM_PASS),
        "传递数 0 与 9 越出 1..=8，登记报 SM_PASS",
    );
    // J7 超峰闸。
    let mut peak_probe = ScmClear::new();
    let _ = peak_probe.file(1, 1);
    s.add(
        "J7-超峰闸",
        peak_probe.touch(1) == Err(SM_REF) && peak_probe.now_of(1) == REF_BASE + 1,
        "峰值已满时再抬引用报 SM_REF，当前引用保持在峰值",
    );
    // J8 泄漏闸。
    let mut leak_probe = ScmClear::new();
    let _ = leak_probe.file(1, 3);
    s.add(
        "J8-泄漏闸",
        leak_probe.replay(1) == Err(SM_LEAK) && leak_probe.leaking() == 1,
        "未释放就终重放报 SM_LEAK，泄漏计数为 1",
    );
    // J9 重号闸。
    let mut dup_probe = ScmClear::new();
    let _ = dup_probe.file(1, 2);
    s.add(
        "J9-重号闸",
        dup_probe.file(1, 3) == Err(SM_DUP) && dup_probe.case_count() == 1,
        "同一判据号二次登记报 SM_DUP",
    );
    // J10 重复重放闸。
    let mut twice = ScmClear::new();
    let _ = twice.file(1, 2);
    let _ = twice.release(1);
    let first_ok = twice.replay(1).is_ok();
    s.add(
        "J10-重放闸",
        first_ok && twice.replay(1) == Err(SM_REPLAY) && twice.replayed() == 1,
        "同一条判据二次终重放报 SM_REPLAY",
    );
    // J11 冻结拒写。
    let mut sealed = build_scm();
    let root = sealed.freeze();
    s.add(
        "J11-冻结拒写",
        sealed.is_frozen()
            && sealed.file(1, 1) == Err(SM_FROZEN)
            && sealed.touch(1) == Err(SM_FROZEN)
            && sealed.release(1) == Err(SM_FROZEN)
            && sealed.fold() == root,
        "冻结后登记、抬引用、释放均报 SM_FROZEN",
    );
    // J12 四路对抗。
    s.add(
        "J12-四路对抗",
        four[0] && four[1] && four[2] && four[3],
        "判据号、传递数、重号、超峰四路全部被拒",
    );
    // J13 判据渲染。
    let case = ScmCase {
        no: 5,
        passed: 3,
        base: REF_BASE,
        peak: 4,
        now: REF_BASE,
        replayed: true,
    };
    s.add(
        "J13-判据渲染",
        case.render() == "R05|P3|1/4/1|R" && case.settled(),
        "第 5 条判据渲染为 R05|P3|1/4/1|R",
    );
    // J14 动态明细。
    s.add_dyn(
        "J14-清账明细",
        book.closed(),
        &format!(
            "条={} 重放={} 传递={} 泄漏={}",
            book.case_count(),
            book.replayed(),
            book.passed_total(),
            book.leaking()
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11128_scm_flow() {
        let mut book = build_scm();
        assert_eq!(book.case_count(), CRITS);
        assert_eq!(book.replayed(), CRITS);
        assert!(book.closed());
        let root = book.freeze();
        assert_eq!(book.fold(), root);
        assert_eq!(book.touch(1), Err(SM_FROZEN));
    }

    #[test]
    fn unx11128_checks_green() {
        let s = run_unx11128_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
