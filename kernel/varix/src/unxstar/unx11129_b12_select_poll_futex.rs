//! UNX-F11129 B12 清账（select/poll/futex 判据终重放）（0x1f5a9 段）
//!
//! B12 批 20 条判据覆盖多路适配与同步：就绪位必须是关注位的子集，futex
//! 等待者唤醒数必须等于登记数。两条不变量逐条重放，清账账才闭合。

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址：F11129 独占 0x1f5a9。
pub const SEG_BASE: u32 = 0x1f5a9;

/// 判据号越出 1..=20。
pub const MS_NO: u32 = SEG_BASE + 0x01;
/// 判据重复登记。
pub const MS_DUP: u32 = SEG_BASE + 0x02;
/// 判据号不连续。
pub const MS_ORDER: u32 = SEG_BASE + 0x03;
/// 就绪位超出关注位。
pub const MS_BITS: u32 = SEG_BASE + 0x04;
/// 等待者未全部唤醒。
pub const MS_WAIT: u32 = SEG_BASE + 0x05;
/// 判据重复重放。
pub const MS_REPLAY: u32 = SEG_BASE + 0x06;
/// 清账账已冻结。
pub const MS_FROZEN: u32 = SEG_BASE + 0x07;
/// 判据条数不足。
pub const MS_THIN: u32 = SEG_BASE + 0x08;

/// 判据总数。
pub const CRITS: usize = 20;
/// 关注位上限。
pub const BITS_MAX: u8 = 0x0f;
/// 等待者上限。
pub const WAIT_MAX: usize = 6;
/// 折叠种子。
const MS_TAG: u64 = 0x0000_1f5_a9a9;
/// FNV-1a 素数。
const FP_STEP: u64 = 0x100000001b3;

/// 适配与同步判据。
#[derive(Clone, Debug)]
pub struct MuxCase {
    /// 判据号（1..=20）。
    pub no: usize,
    /// 关注位掩码。
    pub interest: u8,
    /// 就绪位掩码。
    pub ready: u8,
    /// futex 等待者数。
    pub waiters: usize,
    /// 已唤醒数。
    pub woken: usize,
    /// 是否已终重放。
    pub replayed: bool,
}

impl MuxCase {
    /// 就绪位是否为关注位子集。
    pub fn bits_ok(&self) -> bool {
        (self.ready & !self.interest) == 0
    }

    /// 等待者是否全部唤醒。
    pub fn woken_ok(&self) -> bool {
        self.woken == self.waiters
    }

    /// 判据是否闭合。
    pub fn settled(&self) -> bool {
        self.bits_ok() && self.woken_ok() && self.replayed
    }

    /// 就绪位数。
    pub fn ready_bits(&self) -> usize {
        let mut m = self.ready;
        let mut n = 0usize;
        while m != 0 {
            n = n.wrapping_add((m & 1) as usize);
            m >>= 1;
        }
        n
    }

    /// 判据渲染，形如 M12|I0b1011|R0b0010|3/3|R。
    pub fn render(&self) -> String {
        let r = if self.replayed { "R" } else { "N" };
        format!(
            "M{:02}|I{:04b}|R{:04b}|{}/{}|{}",
            self.no, self.interest, self.ready, self.woken, self.waiters, r
        )
    }
}

/// 适配与同步清账账。
#[derive(Clone, Debug)]
pub struct MuxClear {
    cases: Vec<MuxCase>,
    frozen: bool,
    root: u64,
}

impl MuxClear {
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

    /// 登记判据：号、位域、等待数、重复、顺序、冻结六道闸。
    pub fn file(&mut self, no: usize, interest: u8, ready: u8) -> Result<(), u32> {
        if self.frozen {
            return Err(MS_FROZEN);
        }
        if no == 0 || no > CRITS {
            return Err(MS_NO);
        }
        if interest == 0 || interest > BITS_MAX || ready > BITS_MAX {
            return Err(MS_BITS);
        }
        if (ready & !interest) != 0 {
            return Err(MS_BITS);
        }
        if self.index_of(no) != usize::MAX {
            return Err(MS_DUP);
        }
        if self.cases.len().wrapping_add(1) != no {
            return Err(MS_ORDER);
        }
        let waiters = no % WAIT_MAX + 1;
        self.cases.push(MuxCase {
            no,
            interest,
            ready,
            waiters,
            woken: 0,
            replayed: false,
        });
        Ok(())
    }

    /// 唤醒全部等待者。
    pub fn wake(&mut self, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(MS_FROZEN);
        }
        let idx = self.index_of(no);
        if idx == usize::MAX {
            return Err(MS_NO);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            c.woken = c.waiters;
            return Ok(());
        }
        Err(MS_NO)
    }

    /// 终重放一条判据：位域合法且等待者走完。
    pub fn replay(&mut self, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(MS_FROZEN);
        }
        let idx = self.index_of(no);
        if idx == usize::MAX {
            return Err(MS_NO);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            if !c.bits_ok() {
                return Err(MS_BITS);
            }
            if !c.woken_ok() {
                return Err(MS_WAIT);
            }
            if c.replayed {
                return Err(MS_REPLAY);
            }
            c.replayed = true;
            return Ok(());
        }
        Err(MS_NO)
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

    /// 未唤醒判据数。
    pub fn pending(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.cases.len() {
            if !self.cases[k].woken_ok() {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 总等待者数。
    pub fn waiters_total(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.cases.len() {
            n = n.wrapping_add(self.cases[k].waiters);
            k = k.wrapping_add(1);
        }
        n
    }

    /// 是否全部闭合。
    pub fn closed(&self) -> bool {
        if self.cases.len() != CRITS || self.replayed() != CRITS || self.pending() != 0 {
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
        let mut h = MS_TAG;
        let mut k = 0usize;
        while k < self.cases.len() {
            let c = &self.cases[k];
            h = (h ^ c.no as u64).wrapping_mul(FP_STEP);
            h = (h ^ c.interest as u64).wrapping_mul(FP_STEP);
            h = (h ^ c.ready as u64).wrapping_mul(FP_STEP);
            h = (h ^ c.woken as u64).wrapping_mul(FP_STEP);
            k = k.wrapping_add(1);
        }
        h
    }
}

/// 标准适配账：二十条判据登记后逐条唤醒并重放。
pub fn build_mux() -> MuxClear {
    let mut book = MuxClear::new();
    let mut no = 1usize;
    while no <= CRITS {
        let interest = (no as u8 % BITS_MAX) | 0x01;
        let ready = interest & 0x05;
        let _ = book.file(no, interest, ready);
        let _ = book.wake(no);
        let _ = book.replay(no);
        no = no.wrapping_add(1);
    }
    book
}

/// 四路对抗：判据号越界、就绪位越出关注位、重复登记、未唤醒重放。
pub fn inject_four() -> [bool; 4] {
    let mut a_book = MuxClear::new();
    let a = a_book.file(0, 0x03, 0x01) == Err(MS_NO) && a_book.file(21, 0x03, 0x01) == Err(MS_NO);
    let mut b_book = MuxClear::new();
    let b = b_book.file(1, 0x02, 0x05) == Err(MS_BITS) && b_book.case_count() == 0;
    let mut c_book = MuxClear::new();
    let _ = c_book.file(1, 0x03, 0x01);
    let c = c_book.file(1, 0x03, 0x01) == Err(MS_DUP) && c_book.case_count() == 1;
    let mut d_book = MuxClear::new();
    let _ = d_book.file(1, 0x03, 0x01);
    let d = d_book.replay(1) == Err(MS_WAIT) && d_book.pending() == 1;
    [a, b, c, d]
}

/// F11129 全部检查：适配与同步判据终重放与逐条闭合。
pub fn run_unx11129_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11129 B12 清账");
    let book = build_mux();
    let four = inject_four();

    // J1 二十条在册。
    s.add(
        "J1-判据在册",
        book.case_count() == CRITS && CRITS == 20,
        "B12 批 20 条适配与同步判据全部在册",
    );
    // J2 终重放全量。
    s.add("J2-终重放", book.replayed() == CRITS, "二十条判据全部完成终重放");
    // J3 零等待。
    s.add(
        "J3-零等待",
        book.pending() == 0 && book.closed(),
        "所有 futex 等待者均已唤醒，零在途且清账闭合",
    );
    // J4 等待总量。
    s.add(
        "J4-等待总量",
        book.waiters_total() >= CRITS && book.waiters_total() <= CRITS * WAIT_MAX,
        "等待者总量落在 20 至 120 之间，与每条 1 至 6 个等待者相符",
    );
    // J5 判据号闸。
    let mut no_probe = MuxClear::new();
    s.add(
        "J5-判据号闸",
        no_probe.file(0, 0x03, 0x01) == Err(MS_NO)
            && no_probe.file(21, 0x03, 0x01) == Err(MS_NO),
        "判据号 0 与 21 越出 1..=20，登记报 MS_NO",
    );
    // J6 位域闸。
    let mut bits_probe = MuxClear::new();
    s.add(
        "J6-位域闸",
        bits_probe.file(1, 0x02, 0x05) == Err(MS_BITS)
            && bits_probe.file(1, 0x00, 0x00) == Err(MS_BITS),
        "就绪位越出关注位或关注位为零时登记报 MS_BITS",
    );
    // J7 未唤醒闸。
    let mut wait_probe = MuxClear::new();
    let _ = wait_probe.file(1, 0x03, 0x01);
    s.add(
        "J7-未唤醒闸",
        wait_probe.replay(1) == Err(MS_WAIT) && wait_probe.pending() == 1,
        "等待者未唤醒就重放报 MS_WAIT",
    );
    // J8 唤醒后闭合。
    let mut wake_probe = MuxClear::new();
    let _ = wake_probe.file(1, 0x03, 0x01);
    let _ = wake_probe.wake(1);
    s.add(
        "J8-唤醒闭合",
        wake_probe.pending() == 0 && wake_probe.replay(1).is_ok() && wake_probe.replayed() == 1,
        "唤醒全部等待者后判据可重放并计入闭合",
    );
    // J9 重号闸。
    let mut dup_probe = MuxClear::new();
    let _ = dup_probe.file(1, 0x03, 0x01);
    s.add(
        "J9-重号闸",
        dup_probe.file(1, 0x03, 0x01) == Err(MS_DUP) && dup_probe.case_count() == 1,
        "同一判据号二次登记报 MS_DUP",
    );
    // J10 重复重放闸。
    let mut twice = MuxClear::new();
    let _ = twice.file(1, 0x03, 0x01);
    let _ = twice.wake(1);
    let first_ok = twice.replay(1).is_ok();
    s.add(
        "J10-重放闸",
        first_ok && twice.replay(1) == Err(MS_REPLAY) && twice.replayed() == 1,
        "同一条判据二次终重放报 MS_REPLAY",
    );
    // J11 冻结拒写。
    let mut sealed = build_mux();
    let root = sealed.freeze();
    s.add(
        "J11-冻结拒写",
        sealed.is_frozen()
            && sealed.file(1, 0x03, 0x01) == Err(MS_FROZEN)
            && sealed.wake(1) == Err(MS_FROZEN)
            && sealed.fold() == root,
        "冻结后登记与唤醒均报 MS_FROZEN",
    );
    // J12 四路对抗。
    s.add(
        "J12-四路对抗",
        four[0] && four[1] && four[2] && four[3],
        "判据号、位域、重号、未唤醒四路全部被拒",
    );
    // J13 判据渲染。
    let case = MuxCase {
        no: 12,
        interest: 0x0b,
        ready: 0x02,
        waiters: 3,
        woken: 3,
        replayed: true,
    };
    s.add(
        "J13-判据渲染",
        case.render() == "M12|I1011|R0010|3/3|R" && case.settled(),
        "第 12 条判据渲染为 M12|I1011|R0010|3/3|R",
    );
    // J14 动态明细。
    s.add_dyn(
        "J14-清账明细",
        book.closed(),
        &format!(
            "条={} 重放={} 等待={} 在途={}",
            book.case_count(),
            book.replayed(),
            book.waiters_total(),
            book.pending()
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11129_mux_flow() {
        let mut book = build_mux();
        assert_eq!(book.case_count(), CRITS);
        assert_eq!(book.replayed(), CRITS);
        assert!(book.closed());
        let root = book.freeze();
        assert_eq!(book.fold(), root);
        assert_eq!(book.wake(1), Err(MS_FROZEN));
    }

    #[test]
    fn unx11129_checks_green() {
        let s = run_unx11129_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
