//! UNX-F11133 B18 清账（SOL_SOCKET 选项族判据终重放）（0x1f5ad 段）
//!
//! SOL_SOCKET 选项批 B18 共 20 条判据，覆盖复用地址、发送缓冲、接收超时
//! 与逗留四类选项。设置值经内核钳位后回读，回读必须等于钳位期望。

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址：F11133 独占 0x1f5ad。
pub const SEG_BASE: u32 = 0x1f5ad;

/// 判据号越出 1..=20。
pub const SO_NO: u32 = SEG_BASE + 0x01;
/// 选项号越出 1..=4。
pub const SO_OPT: u32 = SEG_BASE + 0x02;
/// 判据重复登记。
pub const SO_DUP: u32 = SEG_BASE + 0x03;
/// 判据号不连续。
pub const SO_ORDER: u32 = SEG_BASE + 0x04;
/// 回读值与钳位期望不符。
pub const SO_CLAMP: u32 = SEG_BASE + 0x05;
/// 判据重复重放。
pub const SO_REPLAY: u32 = SEG_BASE + 0x06;
/// 清账账已冻结。
pub const SO_FROZEN: u32 = SEG_BASE + 0x07;
/// 判据条数不足。
pub const SO_THIN: u32 = SEG_BASE + 0x08;

/// 判据总数。
pub const CRITS: usize = 20;
/// 复用地址选项。
pub const OPT_REUSE: u8 = 1;
/// 发送缓冲选项。
pub const OPT_SNDBUF: u8 = 2;
/// 接收超时选项。
pub const OPT_RCVTIMEO: u8 = 3;
/// 逗留选项。
pub const OPT_LINGER: u8 = 4;
/// 发送缓冲下限。
pub const BUF_MIN: u32 = 2048;
/// 发送缓冲上限。
pub const BUF_MAX: u32 = 65536;
/// 超时微秒上限。
pub const USEC_MAX: u32 = 999999;
/// 逗留秒上限。
pub const LINGER_MAX: u32 = 60;
/// 折叠种子。
const SO_TAG: u64 = 0x0000_1f5_adad;
/// FNV-1a 素数。
const FP_STEP: u64 = 0x100000001b3;

/// SOL_SOCKET 选项判据。
#[derive(Clone, Debug)]
pub struct OptCase {
    /// 判据号（1..=20）。
    pub no: usize,
    /// 选项号。
    pub opt: u8,
    /// 设置值。
    pub set: u32,
    /// 回读值。
    pub got: u32,
    /// 是否已终重放。
    pub replayed: bool,
}

impl OptCase {
    /// 按选项规则计算钳位期望。
    pub fn expected(opt: u8, set: u32) -> u32 {
        match opt {
            OPT_REUSE => {
                if set == 0 {
                    0
                } else {
                    1
                }
            }
            OPT_SNDBUF => {
                if set < BUF_MIN {
                    BUF_MIN
                } else if set > BUF_MAX {
                    BUF_MAX
                } else {
                    set
                }
            }
            OPT_RCVTIMEO => {
                if set > USEC_MAX {
                    USEC_MAX
                } else {
                    set
                }
            }
            OPT_LINGER => {
                if set > LINGER_MAX {
                    LINGER_MAX
                } else {
                    set
                }
            }
            _ => 0,
        }
    }

    /// 回读是否符合钳位期望。
    pub fn clamp_ok(&self) -> bool {
        self.got == Self::expected(self.opt, self.set)
    }

    /// 判据是否闭合。
    pub fn settled(&self) -> bool {
        self.clamp_ok() && self.replayed
    }

    /// 选项名。
    pub fn opt_name(&self) -> &'static str {
        match self.opt {
            OPT_REUSE => "SO_REUSEADDR",
            OPT_SNDBUF => "SO_SNDBUF",
            OPT_RCVTIMEO => "SO_RCVTIMEO",
            OPT_LINGER => "SO_LINGER",
            _ => "BAD",
        }
    }

    /// 判据渲染，形如 O18-01|SO_SNDBUF|1000>2048|R。
    pub fn render(&self) -> String {
        let r = if self.replayed { "R" } else { "N" };
        format!(
            "O{:02}-{:02}|{}|{}>{}|{}",
            self.no, self.opt, self.opt_name(), self.set, self.got, r
        )
    }
}

/// 选项族清账账。
#[derive(Clone, Debug)]
pub struct OptClear {
    cases: Vec<OptCase>,
    frozen: bool,
    root: u64,
}

impl OptClear {
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

    /// 登记判据：号、选项、重复、顺序、冻结五道闸。
    pub fn file(&mut self, no: usize, opt: u8, set: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(SO_FROZEN);
        }
        if no == 0 || no > CRITS {
            return Err(SO_NO);
        }
        if opt < OPT_REUSE || opt > OPT_LINGER {
            return Err(SO_OPT);
        }
        if self.index_of(no) != usize::MAX {
            return Err(SO_DUP);
        }
        if self.cases.len().wrapping_add(1) != no {
            return Err(SO_ORDER);
        }
        let got = OptCase::expected(opt, set);
        self.cases.push(OptCase { no, opt, set, got, replayed: false });
        Ok(())
    }

    /// 篡改回读值，用于对抗校验。
    pub fn tamper(&mut self, no: usize) -> Result<(), u32> {
        let idx = self.index_of(no);
        if idx == usize::MAX {
            return Err(SO_NO);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            c.got = c.got.wrapping_add(1);
            return Ok(());
        }
        Err(SO_NO)
    }

    /// 终重放：回读必须等于钳位期望。
    pub fn replay(&mut self, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(SO_FROZEN);
        }
        let idx = self.index_of(no);
        if idx == usize::MAX {
            return Err(SO_NO);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            if !c.clamp_ok() {
                return Err(SO_CLAMP);
            }
            if c.replayed {
                return Err(SO_REPLAY);
            }
            c.replayed = true;
            return Ok(());
        }
        Err(SO_NO)
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

    /// 某选项判据数。
    pub fn in_opt(&self, opt: u8) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].opt == opt {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 被钳位的判据数。
    pub fn clamped(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].got != self.cases[k].set {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 选项账是否逐条闭合。
    pub fn closed(&self) -> bool {
        if self.cases.len() != CRITS || self.replayed() != CRITS {
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
        let mut h = SO_TAG;
        let mut k = 0usize;
        while k < self.cases.len() {
            let c = &self.cases[k];
            h = (h ^ c.no as u64).wrapping_mul(FP_STEP);
            h = (h ^ c.opt as u64).wrapping_mul(FP_STEP);
            h = (h ^ c.set as u64).wrapping_mul(FP_STEP);
            h = (h ^ c.got as u64).wrapping_mul(FP_STEP);
            k = k.wrapping_add(1);
        }
        h
    }
}

/// 标准选项账：二十条判据按四类选项轮转并逐条重放。
pub fn build_opt() -> OptClear {
    let mut book = OptClear::new();
    let mut no = 1usize;
    while no <= CRITS {
        let slot = no.saturating_sub(1) % 4;
        let opt = if slot == 0 {
            OPT_REUSE
        } else if slot == 1 {
            OPT_SNDBUF
        } else if slot == 2 {
            OPT_RCVTIMEO
        } else {
            OPT_LINGER
        };
        let set = match opt {
            OPT_REUSE => (no % 2) as u32,
            OPT_SNDBUF => (no as u32).wrapping_mul(1000),
            OPT_RCVTIMEO => 500000u32.wrapping_add(no as u32),
            _ => (no as u32).wrapping_mul(10),
        };
        let _ = book.file(no, opt, set);
        let _ = book.replay(no);
        no = no.wrapping_add(1);
    }
    book
}

/// 四路对抗：判据号越界、选项号越界、重复登记、回读篡改。
pub fn inject_four() -> [bool; 4] {
    let mut a_book = OptClear::new();
    let a = a_book.file(0, OPT_REUSE, 1) == Err(SO_NO) && a_book.file(21, OPT_REUSE, 1) == Err(SO_NO);
    let mut b_book = OptClear::new();
    let b = b_book.file(1, 5, 1) == Err(SO_OPT) && b_book.case_count() == 0;
    let mut c_book = OptClear::new();
    let _ = c_book.file(1, OPT_REUSE, 1);
    let c = c_book.file(1, OPT_LINGER, 1) == Err(SO_DUP) && c_book.case_count() == 1;
    let mut d_book = OptClear::new();
    let _ = d_book.file(1, OPT_SNDBUF, 1000);
    let _ = d_book.tamper(1);
    let d = d_book.replay(1) == Err(SO_CLAMP) && d_book.replayed() == 0;
    [a, b, c, d]
}

/// F11133 全部检查：选项族判据终重放与选项账闭合。
pub fn run_unx11133_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11133 B18 清账");
    let book = build_opt();
    let four = inject_four();

    // J1 二十条在册。
    s.add(
        "J1-判据在册",
        book.case_count() == CRITS && CRITS == 20,
        "B18 批 20 条选项判据全部在册",
    );
    // J2 终重放全量。
    s.add("J2-终重放", book.replayed() == CRITS, "二十条判据全部完成终重放");
    // J3 选项账闭合。
    s.add("J3-选项账闭合", book.closed(), "四类选项判据逐条闭合，选项账闭合");
    // J4 四类覆盖。
    s.add(
        "J4-四类覆盖",
        book.in_opt(OPT_REUSE) > 0
            && book.in_opt(OPT_SNDBUF) > 0
            && book.in_opt(OPT_RCVTIMEO) > 0
            && book.in_opt(OPT_LINGER) > 0,
        "复用地址、发送缓冲、接收超时、逗留四类选项各有判据",
    );
    // J5 复用钳位。
    s.add(
        "J5-复用钳位",
        OptCase::expected(OPT_REUSE, 0) == 0 && OptCase::expected(OPT_REUSE, 7) == 1,
        "复用地址选项把非零值钳位为 1，零保持为零",
    );
    // J6 发送缓冲钳位。
    s.add(
        "J6-缓冲钳位",
        OptCase::expected(OPT_SNDBUF, 1000) == BUF_MIN
            && OptCase::expected(OPT_SNDBUF, 100000) == BUF_MAX
            && OptCase::expected(OPT_SNDBUF, 4096) == 4096,
        "发送缓冲低于 2048 抬到下限、高于 65536 压到上限、区间内保持原值",
    );
    // J7 超时钳位。
    s.add(
        "J7-超时钳位",
        OptCase::expected(OPT_RCVTIMEO, 1500000) == USEC_MAX
            && OptCase::expected(OPT_RCVTIMEO, 250000) == 250000,
        "接收超时微秒超过 999999 时压到上限",
    );
    // J8 逗留钳位。
    s.add(
        "J8-逗留钳位",
        OptCase::expected(OPT_LINGER, 90) == LINGER_MAX && book.clamped() > 0,
        "逗留秒数超过 60 时压到上限，二十条中确有被钳位者",
    );
    // J9 判据号闸。
    let mut no_probe = OptClear::new();
    s.add(
        "J9-判据号闸",
        no_probe.file(0, OPT_REUSE, 1) == Err(SO_NO)
            && no_probe.file(21, OPT_REUSE, 1) == Err(SO_NO),
        "判据号 0 与 21 越出 1..=20，登记报 SO_NO",
    );
    // J10 选项号闸。
    let mut opt_probe = OptClear::new();
    s.add(
        "J10-选项号闸",
        opt_probe.file(1, 0, 1) == Err(SO_OPT) && opt_probe.file(1, 5, 1) == Err(SO_OPT),
        "选项号 0 与 5 越出 1..=4，登记报 SO_OPT",
    );
    // J11 钳位闸。
    let mut clamp_probe = OptClear::new();
    let _ = clamp_probe.file(1, OPT_SNDBUF, 1000);
    let _ = clamp_probe.tamper(1);
    s.add(
        "J11-钳位闸",
        clamp_probe.replay(1) == Err(SO_CLAMP) && clamp_probe.replayed() == 0,
        "回读值被篡改后终重放报 SO_CLAMP",
    );
    // J12 冻结拒写。
    let mut sealed = build_opt();
    let root = sealed.freeze();
    s.add(
        "J12-冻结拒写",
        sealed.is_frozen()
            && sealed.file(1, OPT_REUSE, 1) == Err(SO_FROZEN)
            && sealed.replay(1) == Err(SO_FROZEN)
            && sealed.fold() == root,
        "冻结后登记与重放均报 SO_FROZEN",
    );
    // J13 四路对抗。
    s.add(
        "J13-四路对抗",
        four[0] && four[1] && four[2] && four[3],
        "判据号、选项号、重号、回读篡改四路全部被拒",
    );
    // J14 判据渲染。
    let case = OptCase { no: 18, opt: OPT_SNDBUF, set: 1000, got: BUF_MIN, replayed: true };
    s.add(
        "J14-判据渲染",
        case.render() == "O18-02|SO_SNDBUF|1000>2048|R" && case.clamp_ok(),
        "第 18 条发送缓冲判据渲染为 O18-02|SO_SNDBUF|1000>2048|R",
    );
    // J15 动态明细。
    s.add_dyn(
        "J15-清账明细",
        book.closed(),
        &format!(
            "条={} 重放={} 钳位={} 缓冲={}",
            book.case_count(),
            book.replayed(),
            book.clamped(),
            book.in_opt(OPT_SNDBUF)
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11133_opt_flow() {
        let mut book = build_opt();
        assert_eq!(book.case_count(), CRITS);
        assert_eq!(book.replayed(), CRITS);
        assert!(book.closed());
        assert!(book.clamped() > 0);
        let root = book.freeze();
        assert_eq!(book.fold(), root);
        assert_eq!(book.replay(1), Err(SO_FROZEN));
    }

    #[test]
    fn unx11133_checks_green() {
        let s = run_unx11133_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
