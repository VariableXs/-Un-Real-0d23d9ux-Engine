//! UNX-F11117 联签总对账重放（六批判据全量重放）（0x1f597 段）
//!
//! 联签总对账的六批 B31–B36 必须逐批重放：每批按同一判据种子重算指纹，
//! 重放指纹与首跑指纹逐批一致，才算重放通过。任何一批篡改或漏放都拒绝。

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址：F11117 独占 0x1f597。
pub const SEG_BASE: u32 = 0x1f597;

/// 批号越出 B31–B36。
pub const RP_RANGE: u32 = SEG_BASE + 0x01;
/// 批号不连续。
pub const RP_ORDER: u32 = SEG_BASE + 0x02;
/// 批号重复登记。
pub const RP_DUP: u32 = SEG_BASE + 0x03;
/// 重放行不存在。
pub const RP_NOROW: u32 = SEG_BASE + 0x04;
/// 同一批重复重放。
pub const RP_TWICE: u32 = SEG_BASE + 0x05;
/// 重放指纹与首跑不一致。
pub const RP_MISMATCH: u32 = SEG_BASE + 0x06;
/// 重放账已冻结。
pub const RP_FROZEN: u32 = SEG_BASE + 0x07;
/// 容量溢出。
pub const RP_OVER: u32 = SEG_BASE + 0x08;

/// 首批号。
pub const BATCH_FIRST: usize = 31;
/// 末批号。
pub const BATCH_LAST: usize = 36;
/// 批数。
pub const BATCHES: usize = 6;
/// 每批判据数。
pub const CRIT_PER_BATCH: usize = 20;
/// 容量。
pub const CAP: usize = 8;
/// 折叠种子。
const RP_TAG: u64 = 0x0000_1f5_9797;
/// FNV-1a 素数。
const FP_STEP: u64 = 0x100000001b3;
/// 判据种子基址。
const SEED_BASE: u64 = 0x5250_0000;

/// 单批重放行。
#[derive(Clone, Debug)]
pub struct ReplayRow {
    /// 批号。
    pub batch: usize,
    /// 判据数。
    pub criteria: usize,
    /// 判据种子。
    pub seed: u64,
    /// 首跑指纹。
    pub first: u64,
    /// 重放指纹（未重放为空）。
    pub replay: Option<u64>,
}

impl ReplayRow {
    /// 首跑与重放是否一致（未重放视为不一致）。
    pub fn matched(&self) -> bool {
        match self.replay {
            Some(v) => v == self.first,
            None => false,
        }
    }

    /// 行渲染，形如 B31|20|MATCH 或 B31|20|PENDING。
    pub fn render(&self) -> String {
        let flag = if self.matched() { "MATCH" } else { "PENDING" };
        format!("B{}|{}|{}", self.batch, self.criteria, flag)
    }
}

/// 按批号与判据序号重算判据指纹。
pub fn replay_fp(batch: usize, criteria: usize, seed: u64) -> u64 {
    let mut h = seed;
    let mut k = 1usize;
    while k <= criteria {
        h = (h ^ batch as u64).wrapping_mul(FP_STEP);
        h = (h ^ k as u64).wrapping_mul(FP_STEP);
        k = k.wrapping_add(1);
    }
    h
}

/// 联签重放总账。
#[derive(Clone, Debug)]
pub struct ReplayBook {
    rows: Vec<ReplayRow>,
    frozen: bool,
    root: u64,
}

impl ReplayBook {
    /// 建空账。
    pub fn new() -> Self {
        Self { rows: Vec::new(), frozen: false, root: 0 }
    }

    /// 登记一批首跑指纹：域、顺序、重复、容量、冻结五道闸。
    pub fn enroll(&mut self, batch: usize, criteria: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(RP_FROZEN);
        }
        if batch < BATCH_FIRST || batch > BATCH_LAST {
            return Err(RP_RANGE);
        }
        if criteria != CRIT_PER_BATCH {
            return Err(RP_ORDER);
        }
        if self.rows.len() >= CAP {
            return Err(RP_OVER);
        }
        let mut k = 0usize;
        while k < self.rows.len() {
            if self.rows[k].batch == batch {
                return Err(RP_DUP);
            }
            k = k.wrapping_add(1);
        }
        if !self.rows.is_empty() {
            let mut tail = 0usize;
            let mut j = 0usize;
            while j < self.rows.len() {
                if self.rows[j].batch > tail {
                    tail = self.rows[j].batch;
                }
                j = j.wrapping_add(1);
            }
            if batch != tail.wrapping_add(1) {
                return Err(RP_ORDER);
            }
        } else if batch != BATCH_FIRST {
            return Err(RP_ORDER);
        }
        let seed = SEED_BASE.wrapping_add(batch as u64);
        let first = replay_fp(batch, criteria, seed);
        self.rows.push(ReplayRow { batch, criteria, seed, first, replay: None });
        Ok(())
    }

    /// 定位批行下标。
    fn index_of(&self, batch: usize) -> usize {
        let mut idx = usize::MAX;
        let mut k = 0usize;
        while k < self.rows.len() {
            if self.rows[k].batch == batch {
                idx = k;
            }
            k = k.wrapping_add(1);
        }
        idx
    }

    /// 重放一批：行必须存在且未重放。
    pub fn replay(&mut self, batch: usize) -> Result<u64, u32> {
        if self.frozen {
            return Err(RP_FROZEN);
        }
        let idx = self.index_of(batch);
        if idx == usize::MAX {
            return Err(RP_NOROW);
        }
        let mut out = 0u64;
        if let Some(row) = self.rows.get_mut(idx) {
            if row.replay.is_some() {
                return Err(RP_TWICE);
            }
            let fp = replay_fp(row.batch, row.criteria, row.seed);
            row.replay = Some(fp);
            out = fp;
        }
        Ok(out)
    }

    /// 重放全部在册批次，返回重放批数。
    pub fn replay_all(&mut self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.rows.len() {
            let batch = self.rows[k].batch;
            let pending = self.rows[k].replay.is_none();
            if pending && self.replay(batch).is_ok() {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 篡改首跑指纹，用于对抗校验。
    pub fn tamper(&mut self, batch: usize) -> Result<(), u32> {
        let idx = self.index_of(batch);
        if idx == usize::MAX {
            return Err(RP_NOROW);
        }
        if let Some(row) = self.rows.get_mut(idx) {
            row.first = row.first.wrapping_add(1);
            return Ok(());
        }
        Err(RP_NOROW)
    }

    /// 已重放批数。
    pub fn replayed(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.rows.len() {
            if self.rows[k].replay.is_some() {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 一致批数。
    pub fn matched(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.rows.len() {
            if self.rows[k].matched() {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 行数。
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 全量重放是否逐批对平。
    pub fn consistent(&self) -> bool {
        self.rows.len() == BATCHES && self.replayed() == BATCHES && self.matched() == BATCHES
    }

    /// 重放收官：六批齐、全重放、逐批一致。
    pub fn signoff(&self) -> Result<(), u32> {
        if self.rows.len() < BATCHES {
            return Err(RP_ORDER);
        }
        if self.replayed() != BATCHES {
            return Err(RP_NOROW);
        }
        if self.matched() != BATCHES || !self.consistent() {
            return Err(RP_MISMATCH);
        }
        Ok(())
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
        let mut h = RP_TAG;
        let mut k = 0usize;
        while k < self.rows.len() {
            let r = &self.rows[k];
            h = (h ^ r.batch as u64).wrapping_mul(FP_STEP);
            h = (h ^ r.first).wrapping_mul(FP_STEP);
            let v = match r.replay {
                Some(x) => x,
                None => 0,
            };
            h = (h ^ v).wrapping_mul(FP_STEP);
            k = k.wrapping_add(1);
        }
        h
    }
}

/// 标准重放账：B31 至 B36 登记后全量重放。
pub fn build_replay() -> ReplayBook {
    let mut book = ReplayBook::new();
    let mut b = BATCH_FIRST;
    while b <= BATCH_LAST {
        let _ = book.enroll(b, CRIT_PER_BATCH);
        b = b.wrapping_add(1);
    }
    let _ = book.replay_all();
    book
}

/// 四路对抗：越域批、断链批、重复重放、篡改首跑。
pub fn inject_four() -> [bool; 4] {
    let mut a_book = ReplayBook::new();
    let a = a_book.enroll(30, CRIT_PER_BATCH) == Err(RP_RANGE) && a_book.row_count() == 0;
    let mut b_book = ReplayBook::new();
    let _ = b_book.enroll(31, CRIT_PER_BATCH);
    let b = b_book.enroll(33, CRIT_PER_BATCH) == Err(RP_ORDER) && b_book.row_count() == 1;
    let mut c_book = ReplayBook::new();
    let _ = c_book.enroll(31, CRIT_PER_BATCH);
    let first_ok = c_book.replay(31).is_ok();
    let c = first_ok && c_book.replay(31) == Err(RP_TWICE) && c_book.replayed() == 1;
    let mut d_book = build_replay();
    let _ = d_book.tamper(33);
    let d = d_book.signoff() == Err(RP_MISMATCH) && d_book.matched() == BATCHES - 1;
    [a, b, c, d]
}

/// F11117 全部检查：六批重放逐批对平与首跑一致。
pub fn run_unx11117_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11117 联签总账重放");
    let book = build_replay();
    let four = inject_four();

    // J1 六批在册。
    s.add(
        "J1-六批在册",
        book.row_count() == BATCHES,
        "B31 至 B36 六批全部登记在册",
    );
    // J2 全量重放。
    let mut fresh = ReplayBook::new();
    let mut b = BATCH_FIRST;
    while b <= BATCH_LAST {
        let _ = fresh.enroll(b, CRIT_PER_BATCH);
        b = b.wrapping_add(1);
    }
    s.add(
        "J2-全量重放",
        fresh.replay_all() == BATCHES && fresh.replayed() == BATCHES,
        "新建六批账一次全量重放，重放批数达到 6",
    );
    // J3 逐批一致。
    s.add(
        "J3-逐批一致",
        book.matched() == BATCHES && book.consistent() && book.signoff().is_ok(),
        "六批重放指纹与首跑逐批一致，联签收官通过",
    );
    // J4 越域闸。
    let mut range_book = ReplayBook::new();
    s.add(
        "J4-越域闸",
        range_book.enroll(30, CRIT_PER_BATCH) == Err(RP_RANGE)
            && range_book.enroll(37, CRIT_PER_BATCH) == Err(RP_RANGE),
        "批号 30 与 37 越出 B31–B36，登记报 RP_RANGE",
    );
    // J5 断链闸。
    let mut order_book = ReplayBook::new();
    let _ = order_book.enroll(31, CRIT_PER_BATCH);
    s.add(
        "J5-断链闸",
        order_book.enroll(33, CRIT_PER_BATCH) == Err(RP_ORDER) && order_book.row_count() == 1,
        "B31 后直接登记 B33 报 RP_ORDER",
    );
    // J6 重复重放闸。
    let mut twice = ReplayBook::new();
    let _ = twice.enroll(31, CRIT_PER_BATCH);
    let ok_once = twice.replay(31).is_ok();
    s.add(
        "J6-重复重放",
        ok_once && twice.replay(31) == Err(RP_TWICE) && twice.replayed() == 1,
        "同一批二次重放报 RP_TWICE",
    );
    // J7 无行重放闸。
    let mut empty = ReplayBook::new();
    s.add(
        "J7-无行重放",
        empty.replay(35) == Err(RP_NOROW),
        "账内无 B35 行时重放报 RP_NOROW",
    );
    // J8 篡改检出。
    let mut tampered = build_replay();
    let _ = tampered.tamper(33);
    s.add(
        "J8-篡改检出",
        tampered.signoff() == Err(RP_MISMATCH) && tampered.matched() == BATCHES - 1,
        "篡改 B33 首跑指纹后仅 5 批一致，收官报 RP_MISMATCH",
    );
    // J9 判据数闸。
    let mut crit_probe = ReplayBook::new();
    s.add(
        "J9-判据数闸",
        crit_probe.enroll(31, 19) == Err(RP_ORDER) && crit_probe.row_count() == 0,
        "每批判据数不等于 20 时登记报 RP_ORDER",
    );
    // J10 指纹可复算。
    let mut calc = ReplayBook::new();
    let _ = calc.enroll(31, CRIT_PER_BATCH);
    let got = calc.replay(31);
    let want = replay_fp(31, CRIT_PER_BATCH, SEED_BASE.wrapping_add(31));
    s.add(
        "J10-指纹复算",
        got == Ok(want) && want != 0,
        "B31 重放指纹等于按批号与判据数复算的指纹",
    );
    // J11 冻结拒写。
    let mut sealed = build_replay();
    let root = sealed.seal();
    s.add(
        "J11-冻结拒写",
        sealed.is_frozen()
            && sealed.replay(31) == Err(RP_FROZEN)
            && sealed.enroll(31, CRIT_PER_BATCH) == Err(RP_FROZEN),
        "冻结后重放与登记均报 RP_FROZEN",
    );
    // J12 冻结根。
    s.add(
        "J12-冻结根",
        sealed.fold() == root && root != RP_TAG,
        "冻结根等于冻结时刻的折叠值且不等于种子",
    );
    // J13 四路对抗。
    s.add(
        "J13-四路对抗",
        four[0] && four[1] && four[2] && four[3],
        "越域批、断链批、重复重放、篡改首跑四路全部被拒",
    );
    // J14 行渲染。
    let row = ReplayRow {
        batch: 31,
        criteria: CRIT_PER_BATCH,
        seed: SEED_BASE.wrapping_add(31),
        first: 7,
        replay: Some(7),
    };
    s.add(
        "J14-行渲染",
        row.render() == "B31|20|MATCH" && row.matched(),
        "一致行渲染为 B31|20|MATCH",
    );
    // J15 动态明细。
    s.add_dyn(
        "J15-重放明细",
        book.consistent(),
        &format!(
            "批={} 重放={} 一致={} 末批={}",
            book.row_count(),
            book.replayed(),
            book.matched(),
            BATCH_LAST
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11117_replay_flow() {
        let mut book = build_replay();
        assert_eq!(book.row_count(), BATCHES);
        assert_eq!(book.replayed(), BATCHES);
        assert_eq!(book.matched(), BATCHES);
        assert!(book.consistent());
        let root = book.seal();
        assert_eq!(book.fold(), root);
        assert_eq!(book.replay(31), Err(RP_FROZEN));
    }

    #[test]
    fn unx11117_checks_green() {
        let s = run_unx11117_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
