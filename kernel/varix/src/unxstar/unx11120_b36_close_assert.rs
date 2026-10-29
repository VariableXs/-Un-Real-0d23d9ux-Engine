//! UNX-F11120 B36 批收口（联签总对账与 I 型总收口锚）（0x1f5a0 段）
//!
//! B36 是联签总对账的末批，批内 19 条判据必须逐条重放全绿；同时行数账
//! 必须守恒：批前行数加本批行数等于批后行数，且批后累计到 216000 行。

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址：F11120 独占 0x1f5a0。
pub const SEG_BASE: u32 = 0x1f5a0;

/// 判据号越界（合法 1..=19）。
pub const BC_RANGE: u32 = SEG_BASE + 0x01;
/// 判据重复登记或重复重放。
pub const BC_DUP: u32 = SEG_BASE + 0x02;
/// 红判据不允许重放通过。
pub const BC_NOTGREEN: u32 = SEG_BASE + 0x03;
/// 重放顺序错乱。
pub const BC_ORDER: u32 = SEG_BASE + 0x04;
/// 行数不守恒。
pub const BC_CONSERVE: u32 = SEG_BASE + 0x05;
/// 判据条数不足。
pub const BC_THIN: u32 = SEG_BASE + 0x06;
/// 批账已冻结。
pub const BC_FROZEN: u32 = SEG_BASE + 0x07;
/// 容量溢出。
pub const BC_OVER: u32 = SEG_BASE + 0x08;

/// 批内判据数。
pub const BATCH_CRIT: usize = 19;
/// 本批行数。
pub const BATCH_LINES: usize = 6000;
/// 批前行数（35 批 × 6000）。
pub const BEFORE_LINES: usize = 210000;
/// 批后行数（36 批 × 6000）。
pub const AFTER_LINES: usize = 216000;
/// 容量。
pub const CAP: usize = 24;
/// 折叠种子。
const BC_TAG: u64 = 0x0000_1f5_a0a0;
/// FNV-1a 素数。
const FP_STEP: u64 = 0x100000001b3;

/// 单条批内判据。
#[derive(Clone, Debug)]
pub struct Verdict {
    /// 判据号（1..=19）。
    pub no: usize,
    /// 判据名。
    pub name: String,
    /// 首跑是否全绿。
    pub green: bool,
    /// 是否已重放。
    pub replayed: bool,
}

impl Verdict {
    /// 判据渲染，形如 J01|G|R。
    pub fn render(&self) -> String {
        let g = if self.green { "G" } else { "R" };
        let r = if self.replayed { "R" } else { "N" };
        format!("J{:02}|{}|{}", self.no, g, r)
    }

    /// 判据是否重放通过。
    pub fn passed(&self) -> bool {
        self.green && self.replayed
    }
}

/// B36 批收口账。
#[derive(Clone, Debug)]
pub struct BatchClose {
    verdicts: Vec<Verdict>,
    before: usize,
    batch: usize,
    after: usize,
    frozen: bool,
    root: u64,
}

impl BatchClose {
    /// 建空账。
    pub fn new() -> Self {
        Self {
            verdicts: Vec::new(),
            before: 0,
            batch: 0,
            after: 0,
            frozen: false,
            root: 0,
        }
    }

    /// 登记判据：号域、重复、容量、冻结四道闸。
    pub fn enroll(&mut self, no: usize, green: bool) -> Result<(), u32> {
        if self.frozen {
            return Err(BC_FROZEN);
        }
        if no == 0 || no > BATCH_CRIT {
            return Err(BC_RANGE);
        }
        if self.verdicts.len() >= CAP {
            return Err(BC_OVER);
        }
        let mut k = 0usize;
        while k < self.verdicts.len() {
            if self.verdicts[k].no == no {
                return Err(BC_DUP);
            }
            k = k.wrapping_add(1);
        }
        let name = format!("J{:02}", no);
        self.verdicts.push(Verdict { no, name, green, replayed: false });
        Ok(())
    }

    /// 顺序重放一条：序号必须紧接已重放条数。
    pub fn replay_seq(&mut self, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(BC_FROZEN);
        }
        if no == 0 || no > BATCH_CRIT {
            return Err(BC_RANGE);
        }
        if no != self.replayed_count().wrapping_add(1) {
            return Err(BC_ORDER);
        }
        let mut idx = usize::MAX;
        let mut k = 0usize;
        while k < self.verdicts.len() {
            if self.verdicts[k].no == no {
                idx = k;
            }
            k = k.wrapping_add(1);
        }
        if idx == usize::MAX {
            return Err(BC_RANGE);
        }
        if let Some(v) = self.verdicts.get_mut(idx) {
            if v.replayed {
                return Err(BC_DUP);
            }
            if !v.green {
                return Err(BC_NOTGREEN);
            }
            v.replayed = true;
        }
        Ok(())
    }

    /// 全批重放。
    pub fn sweep(&mut self) -> usize {
        let mut n = 0usize;
        let mut no = 1usize;
        while no <= self.verdicts.len() {
            if self.replay_seq(no).is_ok() {
                n = n.wrapping_add(1);
            }
            no = no.wrapping_add(1);
        }
        n
    }

    /// 判据条数。
    pub fn verdict_count(&self) -> usize {
        self.verdicts.len()
    }

    /// 重放条数。
    pub fn replayed_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.verdicts.len() {
            if self.verdicts[k].replayed {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 全绿条数。
    pub fn green_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.verdicts.len() {
            if self.verdicts[k].green {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 是否全部重放通过。
    pub fn sweep_green(&self) -> bool {
        self.verdicts.len() == BATCH_CRIT
            && self.green_count() == BATCH_CRIT
            && self.replayed_count() == BATCH_CRIT
    }

    /// 批收口：判据全绿重放后校验行数守恒。
    pub fn close(&mut self, before: usize, batch: usize, after: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(BC_FROZEN);
        }
        if self.verdicts.len() < BATCH_CRIT {
            return Err(BC_THIN);
        }
        if !self.sweep_green() {
            return Err(BC_NOTGREEN);
        }
        if before.wrapping_add(batch) != after
            || batch != BATCH_LINES
            || after != AFTER_LINES
        {
            return Err(BC_CONSERVE);
        }
        self.before = before;
        self.batch = batch;
        self.after = after;
        Ok(())
    }

    /// 行数是否守恒。
    pub fn conserved(&self) -> bool {
        self.before.wrapping_add(self.batch) == self.after
            && self.batch == BATCH_LINES
            && self.after == AFTER_LINES
            && self.after == 36 * BATCH_LINES
    }

    /// 批前、本批、批后三数。
    pub fn lines(&self) -> (usize, usize, usize) {
        (self.before, self.batch, self.after)
    }

    /// 冻结：返回折叠根。
    pub fn seal(&mut self) -> u64 {
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
        let mut h = BC_TAG;
        let mut k = 0usize;
        while k < self.verdicts.len() {
            let v = &self.verdicts[k];
            h = (h ^ v.no as u64).wrapping_mul(FP_STEP);
            let g = if v.green { 1u64 } else { 2u64 };
            h = (h ^ g).wrapping_mul(FP_STEP);
            let r = if v.replayed { 3u64 } else { 4u64 };
            h = (h ^ r).wrapping_mul(FP_STEP);
            k = k.wrapping_add(1);
        }
        h = (h ^ self.before as u64).wrapping_mul(FP_STEP);
        h = (h ^ self.batch as u64).wrapping_mul(FP_STEP);
        h = (h ^ self.after as u64).wrapping_mul(FP_STEP);
        h
    }
}

/// 标准收口账：19 条判据全绿重放并完成行数守恒。
pub fn build_close() -> BatchClose {
    let mut book = BatchClose::new();
    let mut no = 1usize;
    while no <= BATCH_CRIT {
        let _ = book.enroll(no, true);
        no = no.wrapping_add(1);
    }
    let _ = book.sweep();
    let _ = book.close(BEFORE_LINES, BATCH_LINES, AFTER_LINES);
    book
}

/// 四路对抗：号越界、重号、红判据重放、行数不守恒。
pub fn inject_four() -> [bool; 4] {
    let mut a_book = BatchClose::new();
    let a = a_book.enroll(0, true) == Err(BC_RANGE) && a_book.verdict_count() == 0;
    let mut b_book = BatchClose::new();
    let _ = b_book.enroll(1, true);
    let b = b_book.enroll(1, true) == Err(BC_DUP) && b_book.verdict_count() == 1;
    let mut c_book = BatchClose::new();
    let _ = c_book.enroll(1, false);
    let c = c_book.replay_seq(1) == Err(BC_NOTGREEN) && c_book.replayed_count() == 0;
    let mut d_book = BatchClose::new();
    let mut no = 1usize;
    while no <= BATCH_CRIT {
        let _ = d_book.enroll(no, true);
        no = no.wrapping_add(1);
    }
    let _ = d_book.sweep();
    let d = d_book.close(BEFORE_LINES, BATCH_LINES, AFTER_LINES + 1000) == Err(BC_CONSERVE)
        && !d_book.conserved();
    [a, b, c, d]
}

/// F11120 全部检查：19 条判据重放与行数守恒。
pub fn run_unx11120_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11120 B36 批收口");
    let book = build_close();
    let four = inject_four();

    // J1 十九判据在册。
    s.add(
        "J1-判据在册",
        book.verdict_count() == BATCH_CRIT && BATCH_CRIT == 19,
        "B36 批内 J01 至 J19 共 19 条判据全部在册",
    );
    // J2 全绿。
    s.add(
        "J2-判据全绿",
        book.green_count() == BATCH_CRIT,
        "19 条判据首跑全部为绿",
    );
    // J3 全量重放。
    s.add(
        "J3-全量重放",
        book.replayed_count() == BATCH_CRIT && book.sweep_green(),
        "19 条判据按序重放全部通过",
    );
    // J4 行数守恒。
    s.add(
        "J4-行数守恒",
        book.conserved() && book.lines() == (BEFORE_LINES, BATCH_LINES, AFTER_LINES),
        "批前 210000 加本批 6000 等于批后 216000",
    );
    // J5 域累计对平。
    s.add(
        "J5-域累计",
        AFTER_LINES == 36 * BATCH_LINES && book.lines().2 == AFTER_LINES,
        "批后累计等于 36 批各 6000 行，域账对平",
    );
    // J6 号域闸。
    let mut range_book = BatchClose::new();
    s.add(
        "J6-号域闸",
        range_book.enroll(0, true) == Err(BC_RANGE)
            && range_book.enroll(20, true) == Err(BC_RANGE),
        "判据号 0 与 20 越出 1..=19，登记报 BC_RANGE",
    );
    // J7 重号闸。
    let mut dup_book = BatchClose::new();
    let _ = dup_book.enroll(7, true);
    s.add(
        "J7-重号闸",
        dup_book.enroll(7, true) == Err(BC_DUP) && dup_book.verdict_count() == 1,
        "同一判据号二次登记报 BC_DUP",
    );
    // J8 红判据闸。
    let mut red_book = BatchClose::new();
    let _ = red_book.enroll(1, false);
    s.add(
        "J8-红判据闸",
        red_book.replay_seq(1) == Err(BC_NOTGREEN) && red_book.replayed_count() == 0,
        "红判据重放报 BC_NOTGREEN，不计入重放条数",
    );
    // J9 守恒闸。
    let mut off_book = build_close();
    s.add(
        "J9-守恒闸",
        off_book.close(BEFORE_LINES, BATCH_LINES, AFTER_LINES + 1) == Err(BC_CONSERVE)
            && off_book.conserved(),
        "批后行数被改坏时收口报 BC_CONSERVE，原守恒结论不被覆盖",
    );
    // J10 顺序闸。
    let mut order_book = BatchClose::new();
    let _ = order_book.enroll(1, true);
    let _ = order_book.enroll(2, true);
    s.add(
        "J10-顺序闸",
        order_book.replay_seq(2) == Err(BC_ORDER) && order_book.replayed_count() == 0,
        "首条未重放时直接重放第 2 条报 BC_ORDER",
    );
    // J11 条数不足闸。
    let mut thin_book = BatchClose::new();
    let mut no = 1usize;
    while no < BATCH_CRIT {
        let _ = thin_book.enroll(no, true);
        no = no.wrapping_add(1);
    }
    let _ = thin_book.sweep();
    s.add(
        "J11-条数闸",
        thin_book.close(BEFORE_LINES, BATCH_LINES, AFTER_LINES) == Err(BC_THIN),
        "仅登记 18 条判据时收口报 BC_THIN",
    );
    // J12 冻结拒写。
    let mut sealed = build_close();
    let root = sealed.seal();
    s.add(
        "J12-冻结拒写",
        sealed.is_frozen()
            && sealed.enroll(1, true) == Err(BC_FROZEN)
            && sealed.replay_seq(1) == Err(BC_FROZEN)
            && sealed.close(BEFORE_LINES, BATCH_LINES, AFTER_LINES) == Err(BC_FROZEN),
        "冻结后登记、重放、收口均报 BC_FROZEN",
    );
    // J13 冻结根。
    s.add(
        "J13-冻结根",
        sealed.fold() == root && root != BC_TAG,
        "冻结根等于冻结时刻的折叠值且不等于种子",
    );
    // J14 四路对抗。
    s.add(
        "J14-四路对抗",
        four[0] && four[1] && four[2] && four[3],
        "号越界、重号、红判据重放、行数不守恒四路全部被拒",
    );
    // J15 判据渲染。
    let v = Verdict {
        no: 1,
        name: String::from("J01"),
        green: true,
        replayed: true,
    };
    s.add(
        "J15-判据渲染",
        v.render() == "J01|G|R" && v.passed(),
        "首条判据渲染为 J01|G|R",
    );
    // J16 动态明细。
    s.add_dyn(
        "J16-收口明细",
        book.sweep_green() && book.conserved(),
        &format!(
            "判据={} 重放={} 绿={} 行={}/{}/{}",
            book.verdict_count(),
            book.replayed_count(),
            book.green_count(),
            book.lines().0,
            book.lines().1,
            book.lines().2
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11120_b36_close_flow() {
        let mut book = build_close();
        assert_eq!(book.verdict_count(), BATCH_CRIT);
        assert!(book.sweep_green());
        assert!(book.conserved());
        assert_eq!(book.lines(), (210000, 6000, 216000));
        let root = book.seal();
        assert_eq!(book.fold(), root);
        assert_eq!(book.replay_seq(1), Err(BC_FROZEN));
    }

    #[test]
    fn unx11120_checks_green() {
        let s = run_unx11120_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
