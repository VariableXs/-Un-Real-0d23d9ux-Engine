//! UNX-F11123 B03–B04 清账（信号族判据终重放）（0x1f5a3 段）
//!
//! 信号族占 B03 与 B04 两批，每批 20 条处置判据。每条记录信号号与处置
//! 动作（忽略/默认/接管），KILL 与 STOP 只允许默认处置，清账时逐条重放。

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址：F11123 独占 0x1f5a3。
pub const SEG_BASE: u32 = 0x1f5a3;

/// 批号越出 B03–B04。
pub const SC_BATCH: u32 = SEG_BASE + 0x01;
/// 信号号越出 1..=31。
pub const SC_SIGNO: u32 = SEG_BASE + 0x02;
/// 处置动作越出 1..=3。
pub const SC_ACT: u32 = SEG_BASE + 0x03;
/// 不可捕获信号被接管或忽略。
pub const SC_UNCATCH: u32 = SEG_BASE + 0x04;
/// 判据重复登记。
pub const SC_DUP: u32 = SEG_BASE + 0x05;
/// 批内判据不连续。
pub const SC_ORDER: u32 = SEG_BASE + 0x06;
/// 判据重复重放。
pub const SC_REPLAY: u32 = SEG_BASE + 0x07;
/// 清账账已冻结。
pub const SC_FROZEN: u32 = SEG_BASE + 0x08;

/// 首批号。
pub const BATCH_FIRST: usize = 3;
/// 末批号。
pub const BATCH_LAST: usize = 4;
/// 每批判据数。
pub const CRIT_PER_BATCH: usize = 20;
/// 判据总数。
pub const CRITS: usize = 40;
/// 信号号上限。
pub const SIGNO_MAX: usize = 31;
/// 处置：忽略。
pub const ACT_IGN: u8 = 1;
/// 处置：默认。
pub const ACT_DFL: u8 = 2;
/// 处置：接管。
pub const ACT_HAND: u8 = 3;
/// KILL 信号号。
pub const SIG_KILL: usize = 9;
/// STOP 信号号。
pub const SIG_STOP: usize = 19;
/// 折叠种子。
const SC_TAG: u64 = 0x0000_1f5_a3a3;
/// FNV-1a 素数。
const FP_STEP: u64 = 0x100000001b3;

/// 信号判据。
#[derive(Clone, Debug)]
pub struct SigCase {
    /// 批号（3 或 4）。
    pub batch: usize,
    /// 信号号。
    pub signo: usize,
    /// 处置动作。
    pub action: u8,
    /// 是否已终重放。
    pub replayed: bool,
}

impl SigCase {
    /// 信号是否不可捕获。
    pub fn uncatchable(&self) -> bool {
        self.signo == SIG_KILL || self.signo == SIG_STOP
    }

    /// 处置是否合法。
    pub fn legal(&self) -> bool {
        if self.uncatchable() {
            self.action == ACT_DFL
        } else {
            self.action >= ACT_IGN && self.action <= ACT_HAND
        }
    }

    /// 判据是否清账闭合。
    pub fn settled(&self) -> bool {
        self.legal() && self.replayed
    }

    /// 处置名。
    pub fn action_name(&self) -> String {
        let name = match self.action {
            ACT_IGN => "IGN",
            ACT_DFL => "DFL",
            ACT_HAND => "HAND",
            _ => "BAD",
        };
        String::from(name)
    }

    /// 判据渲染，形如 S3-01|SIG1|DFL|R。
    pub fn render(&self) -> String {
        let r = if self.replayed { "R" } else { "N" };
        format!("S{}-{:02}|SIG{}|{}|{}", self.batch, self.batch_no(), self.signo, self.action_name(), r)
    }

    /// 批内序号。
    pub fn batch_no(&self) -> usize {
        self.signo.saturating_sub(1).wrapping_rem(CRIT_PER_BATCH).wrapping_add(1)
    }
}

/// 信号族清账账。
#[derive(Clone, Debug)]
pub struct SignalClear {
    cases: Vec<SigCase>,
    frozen: bool,
    root: u64,
}

impl SignalClear {
    /// 建空账。
    pub fn new() -> Self {
        Self { cases: Vec::new(), frozen: false, root: 0 }
    }

    /// 定位判据下标。
    fn index_of(&self, batch: usize, signo: usize) -> usize {
        let mut idx = usize::MAX;
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].batch == batch && self.cases[k].signo == signo {
                idx = k;
            }
            k = k.wrapping_add(1);
        }
        idx
    }

    /// 登记一条判据：批、号、动作、不可捕获、重复、顺序、冻结七道闸。
    pub fn file(&mut self, batch: usize, signo: usize, action: u8) -> Result<(), u32> {
        if self.frozen {
            return Err(SC_FROZEN);
        }
        if batch < BATCH_FIRST || batch > BATCH_LAST {
            return Err(SC_BATCH);
        }
        if signo == 0 || signo > SIGNO_MAX {
            return Err(SC_SIGNO);
        }
        if action < ACT_IGN || action > ACT_HAND {
            return Err(SC_ACT);
        }
        if (signo == SIG_KILL || signo == SIG_STOP) && action != ACT_DFL {
            return Err(SC_UNCATCH);
        }
        if self.index_of(batch, signo) != usize::MAX {
            return Err(SC_DUP);
        }
        if self.cases.len() % CRIT_PER_BATCH != 0 {
            let cur_batch = self.cases.len() / CRIT_PER_BATCH + BATCH_FIRST;
            if batch != cur_batch {
                return Err(SC_ORDER);
            }
        } else {
            let want = self.cases.len() / CRIT_PER_BATCH + BATCH_FIRST;
            if batch != want {
                return Err(SC_ORDER);
            }
        }
        self.cases.push(SigCase { batch, signo, action, replayed: false });
        Ok(())
    }

    /// 终重放一条判据。
    pub fn replay(&mut self, batch: usize, signo: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(SC_FROZEN);
        }
        let idx = self.index_of(batch, signo);
        if idx == usize::MAX {
            return Err(SC_SIGNO);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            if !c.legal() {
                return Err(SC_UNCATCH);
            }
            if c.replayed {
                return Err(SC_REPLAY);
            }
            c.replayed = true;
            return Ok(());
        }
        Err(SC_SIGNO)
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

    /// 某批已重放条数。
    pub fn batch_replayed(&self, batch: usize) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].batch == batch && self.cases[k].replayed {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 接管处置条数。
    pub fn handled(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].action == ACT_HAND {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 两批是否全部闭合。
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
        self.batch_replayed(BATCH_FIRST) == CRIT_PER_BATCH
            && self.batch_replayed(BATCH_LAST) == CRIT_PER_BATCH
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
        let mut h = SC_TAG;
        let mut k = 0usize;
        while k < self.cases.len() {
            let c = &self.cases[k];
            h = (h ^ c.batch as u64).wrapping_mul(FP_STEP);
            h = (h ^ c.signo as u64).wrapping_mul(FP_STEP);
            h = (h ^ c.action as u64).wrapping_mul(FP_STEP);
            let b = if c.replayed { 1u64 } else { 2u64 };
            h = (h ^ b).wrapping_mul(FP_STEP);
            k = k.wrapping_add(1);
        }
        h
    }
}

/// 标准信号清账账：B03 覆盖 SIG1–SIG20，B04 覆盖 SIG12–SIG31。
pub fn build_signal() -> SignalClear {
    let mut book = SignalClear::new();
    let mut no = 0usize;
    while no < CRITS {
        let batch = BATCH_FIRST + no / CRIT_PER_BATCH;
        let signo = if batch == BATCH_FIRST {
            no % CRIT_PER_BATCH + 1
        } else {
            no % CRIT_PER_BATCH + 12
        };
        let action = if signo == SIG_KILL || signo == SIG_STOP {
            ACT_DFL
        } else if signo % 2 == 0 {
            ACT_HAND
        } else {
            ACT_IGN
        };
        let _ = book.file(batch, signo, action);
        no = no.wrapping_add(1);
    }
    let mut k = 0usize;
    while k < book.cases.len() {
        let batch = book.cases[k].batch;
        let signo = book.cases[k].signo;
        let _ = book.replay(batch, signo);
        k = k.wrapping_add(1);
    }
    book
}

/// 四路对抗：批号越界、信号号越界、接管不可捕获信号、重复登记。
pub fn inject_four() -> [bool; 4] {
    let mut a_book = SignalClear::new();
    let a = a_book.file(5, 1, ACT_DFL) == Err(SC_BATCH) && a_book.case_count() == 0;
    let mut b_book = SignalClear::new();
    let b = b_book.file(3, 32, ACT_DFL) == Err(SC_SIGNO) && b_book.case_count() == 0;
    let mut c_book = SignalClear::new();
    let c = c_book.file(3, SIG_KILL, ACT_HAND) == Err(SC_UNCATCH) && c_book.case_count() == 0;
    let mut d_book = SignalClear::new();
    let _ = d_book.file(3, 5, ACT_IGN);
    let d = d_book.file(3, 5, ACT_HAND) == Err(SC_DUP) && d_book.case_count() == 1;
    [a, b, c, d]
}

/// F11123 全部检查：信号族判据终重放与逐条闭合。
pub fn run_unx11123_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11123 B03-B04 清账");
    let book = build_signal();
    let four = inject_four();

    // J1 两批四十条在册。
    s.add(
        "J1-两批在册",
        book.case_count() == CRITS && CRITS == 2 * CRIT_PER_BATCH,
        "B03 与 B04 各 20 条处置判据，共 40 条在册",
    );
    // J2 终重放全量。
    s.add(
        "J2-终重放",
        book.replayed() == CRITS && book.batch_replayed(BATCH_FIRST) == 20,
        "四十条判据全部完成终重放，B03 恰好 20 条",
    );
    // J3 逐条闭合。
    s.add("J3-逐条闭合", book.closed(), "两批四十条判据全部合法且重放，清账账闭合");
    // J4 批域闸。
    let mut batch_probe = SignalClear::new();
    s.add(
        "J4-批域闸",
        batch_probe.file(2, 1, ACT_DFL) == Err(SC_BATCH)
            && batch_probe.file(5, 1, ACT_DFL) == Err(SC_BATCH),
        "批号 2 与 5 越出 B03–B04，登记报 SC_BATCH",
    );
    // J5 信号号闸。
    let mut sig_probe = SignalClear::new();
    s.add(
        "J5-信号号闸",
        sig_probe.file(3, 0, ACT_DFL) == Err(SC_SIGNO)
            && sig_probe.file(3, 32, ACT_DFL) == Err(SC_SIGNO),
        "信号号 0 与 32 越出 1..=31，登记报 SC_SIGNO",
    );
    // J6 动作闸。
    let mut act_probe = SignalClear::new();
    s.add(
        "J6-动作闸",
        act_probe.file(3, 1, 0) == Err(SC_ACT) && act_probe.file(3, 1, 4) == Err(SC_ACT),
        "处置动作 0 与 4 越出 1..=3，登记报 SC_ACT",
    );
    // J7 不可捕获闸。
    let mut kill_probe = SignalClear::new();
    s.add(
        "J7-不可捕获闸",
        kill_probe.file(3, SIG_KILL, ACT_HAND) == Err(SC_UNCATCH)
            && kill_probe.file(3, SIG_STOP, ACT_IGN) == Err(SC_UNCATCH)
            && kill_probe.file(3, SIG_KILL, ACT_DFL).is_ok(),
        "KILL 与 STOP 只接受默认处置，接管或忽略均报 SC_UNCATCH",
    );
    // J8 重号闸。
    let mut dup_probe = SignalClear::new();
    let _ = dup_probe.file(3, 7, ACT_IGN);
    s.add(
        "J8-重号闸",
        dup_probe.file(3, 7, ACT_HAND) == Err(SC_DUP) && dup_probe.case_count() == 1,
        "同批同信号号二次登记报 SC_DUP",
    );
    // J9 重复重放闸。
    let mut twice = SignalClear::new();
    let _ = twice.file(3, 3, ACT_IGN);
    let first_ok = twice.replay(3, 3).is_ok();
    s.add(
        "J9-重放闸",
        first_ok && twice.replay(3, 3) == Err(SC_REPLAY) && twice.replayed() == 1,
        "同一条判据二次终重放报 SC_REPLAY",
    );
    // J10 接管计数。
    s.add(
        "J10-接管计数",
        book.handled() > 0 && book.handled() < CRITS,
        "接管处置条数介于零与四十之间，忽略与默认各占其余部分",
    );
    // J11 冻结拒写。
    let mut sealed = build_signal();
    let root = sealed.freeze();
    s.add(
        "J11-冻结拒写",
        sealed.is_frozen()
            && sealed.file(3, 1, ACT_DFL) == Err(SC_FROZEN)
            && sealed.replay(3, 1) == Err(SC_FROZEN)
            && sealed.fold() == root,
        "冻结后登记与重放均报 SC_FROZEN，根保持不变",
    );
    // J12 四路对抗。
    s.add(
        "J12-四路对抗",
        four[0] && four[1] && four[2] && four[3],
        "批域、信号号、不可捕获、重号四路全部被拒",
    );
    // J13 判据渲染。
    let case = SigCase { batch: 3, signo: 1, action: ACT_DFL, replayed: true };
    s.add(
        "J13-判据渲染",
        case.render() == "S3-01|SIG1|DFL|R" && case.settled(),
        "B03 首条判据渲染为 S3-01|SIG1|DFL|R",
    );
    // J14 动态明细。
    s.add_dyn(
        "J14-清账明细",
        book.closed(),
        &format!(
            "条={} 重放={} 接管={} B04={}",
            book.case_count(),
            book.replayed(),
            book.handled(),
            book.batch_replayed(BATCH_LAST)
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11123_signal_flow() {
        let mut book = build_signal();
        assert_eq!(book.case_count(), CRITS);
        assert_eq!(book.replayed(), CRITS);
        assert!(book.closed());
        let root = book.freeze();
        assert_eq!(book.fold(), root);
        assert_eq!(book.replay(3, 1), Err(SC_FROZEN));
    }

    #[test]
    fn unx11123_checks_green() {
        let s = run_unx11123_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
