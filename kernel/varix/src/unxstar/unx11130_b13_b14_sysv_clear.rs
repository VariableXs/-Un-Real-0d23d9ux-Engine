//! UNX-F11130 B13–B14 清账（SysV shm/sem/msg 判据终重放）（0x1f5aa 段）
//!
//! SysV 族占 B13 与 B14 两批，覆盖共享内存、信号量与消息队列三型。每个
//! 对象走创建、获取、释放、删除四态生命周期，键在同型内唯一，终态必须删除。

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址：F11130 独占 0x1f5aa。
pub const SEG_BASE: u32 = 0x1f5aa;

/// 批号越出 B13–B14。
pub const KS_BATCH: u32 = SEG_BASE + 0x01;
/// 判据号越出 1..=20。
pub const KS_NO: u32 = SEG_BASE + 0x02;
/// 对象类型越出 1..=3。
pub const KS_KIND: u32 = SEG_BASE + 0x03;
/// 同型键重复。
pub const KS_KEY: u32 = SEG_BASE + 0x04;
/// 生命周期状态不允许该动作。
pub const KS_STATE: u32 = SEG_BASE + 0x05;
/// 判据号不连续。
pub const KS_ORDER: u32 = SEG_BASE + 0x06;
/// 判据重复重放。
pub const KS_REPLAY: u32 = SEG_BASE + 0x07;
/// 清账账已冻结。
pub const KS_FROZEN: u32 = SEG_BASE + 0x08;

/// 首批号。
pub const BATCH_FIRST: usize = 13;
/// 末批号。
pub const BATCH_LAST: usize = 14;
/// 每批判据数。
pub const CRIT_PER_BATCH: usize = 20;
/// 判据总数。
pub const CRITS: usize = 40;
/// 共享内存型。
pub const KIND_SHM: u8 = 1;
/// 信号量型。
pub const KIND_SEM: u8 = 2;
/// 消息队列型。
pub const KIND_MSG: u8 = 3;
/// 状态：已创建。
pub const ST_CREATED: u8 = 1;
/// 状态：已获取。
pub const ST_ACQUIRED: u8 = 2;
/// 状态：已释放。
pub const ST_RELEASED: u8 = 3;
/// 状态：已删除。
pub const ST_REMOVED: u8 = 4;
/// 折叠种子。
const KS_TAG: u64 = 0x0000_1f5_aaaa;
/// FNV-1a 素数。
const FP_STEP: u64 = 0x100000001b3;

/// SysV 生命周期判据。
#[derive(Clone, Debug)]
pub struct SysvCase {
    /// 批号（13 或 14）。
    pub batch: usize,
    /// 批内判据号。
    pub no: usize,
    /// 类型（1 shm / 2 sem / 3 msg）。
    pub kind: u8,
    /// 对象键。
    pub key: u32,
    /// 生命周期状态。
    pub state: u8,
    /// 是否已终重放。
    pub replayed: bool,
}

impl SysvCase {
    /// 类型名。
    pub fn kind_name(&self) -> String {
        let name = match self.kind {
            KIND_SHM => "SHM",
            KIND_SEM => "SEM",
            KIND_MSG => "MSG",
            _ => "BAD",
        };
        String::from(name)
    }

    /// 状态名。
    pub fn state_name(&self) -> String {
        let name = match self.state {
            ST_CREATED => "CREATED",
            ST_ACQUIRED => "ACQUIRED",
            ST_RELEASED => "RELEASED",
            ST_REMOVED => "REMOVED",
            _ => "BAD",
        };
        String::from(name)
    }

    /// 生命周期是否走完。
    pub fn retired(&self) -> bool {
        self.state == ST_REMOVED
    }

    /// 判据是否闭合。
    pub fn settled(&self) -> bool {
        self.retired() && self.replayed
    }

    /// 判据渲染，形如 V13-01|SHM|0x1001|REMOVED|R。
    pub fn render(&self) -> String {
        let r = if self.replayed { "R" } else { "N" };
        format!(
            "V{}-{:02}|{}|{:#06x}|{}|{}",
            self.batch, self.no, self.kind_name(), self.key, self.state_name(), r
        )
    }
}

/// SysV 族清账账。
#[derive(Clone, Debug)]
pub struct SysvClear {
    cases: Vec<SysvCase>,
    frozen: bool,
    root: u64,
}

impl SysvClear {
    /// 建空账。
    pub fn new() -> Self {
        Self { cases: Vec::new(), frozen: false, root: 0 }
    }

    /// 定位判据下标。
    fn index_of(&self, batch: usize, no: usize) -> usize {
        let mut idx = usize::MAX;
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].batch == batch && self.cases[k].no == no {
                idx = k;
            }
            k = k.wrapping_add(1);
        }
        idx
    }

    /// 登记对象：批、号、类型、键、顺序、冻结六道闸。
    pub fn file(&mut self, batch: usize, no: usize, kind: u8, key: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(KS_FROZEN);
        }
        if batch < BATCH_FIRST || batch > BATCH_LAST {
            return Err(KS_BATCH);
        }
        if no == 0 || no > CRIT_PER_BATCH {
            return Err(KS_NO);
        }
        if kind < KIND_SHM || kind > KIND_MSG {
            return Err(KS_KIND);
        }
        let want = self.cases.len() / CRIT_PER_BATCH + BATCH_FIRST;
        if batch != want || self.cases.len() % CRIT_PER_BATCH != no.saturating_sub(1) {
            return Err(KS_ORDER);
        }
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].kind == kind && self.cases[k].key == key {
                return Err(KS_KEY);
            }
            k = k.wrapping_add(1);
        }
        self.cases.push(SysvCase {
            batch,
            no,
            kind,
            key,
            state: ST_CREATED,
            replayed: false,
        });
        Ok(())
    }

    /// 状态推进：创建到获取到释放到删除，每次只前进一步。
    pub fn advance(&mut self, batch: usize, no: usize) -> Result<u8, u32> {
        if self.frozen {
            return Err(KS_FROZEN);
        }
        let idx = self.index_of(batch, no);
        if idx == usize::MAX {
            return Err(KS_NO);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            if c.state >= ST_REMOVED {
                return Err(KS_STATE);
            }
            c.state = c.state.wrapping_add(1);
            return Ok(c.state);
        }
        Err(KS_NO)
    }

    /// 终重放：对象必须已删除。
    pub fn replay(&mut self, batch: usize, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(KS_FROZEN);
        }
        let idx = self.index_of(batch, no);
        if idx == usize::MAX {
            return Err(KS_NO);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            if !c.retired() {
                return Err(KS_STATE);
            }
            if c.replayed {
                return Err(KS_REPLAY);
            }
            c.replayed = true;
            return Ok(());
        }
        Err(KS_NO)
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

    /// 某型对象数。
    pub fn in_kind(&self, kind: u8) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].kind == kind {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 未删除对象数。
    pub fn alive(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.cases.len() {
            if !self.cases[k].retired() {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 生命周期账是否闭合。
    pub fn lifecycle_closed(&self) -> bool {
        self.cases.len() == CRITS && self.alive() == 0 && self.replayed() == CRITS
    }

    /// 是否全部闭合。
    pub fn closed(&self) -> bool {
        if !self.lifecycle_closed() {
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
        let mut h = KS_TAG;
        let mut k = 0usize;
        while k < self.cases.len() {
            let c = &self.cases[k];
            h = (h ^ c.batch as u64).wrapping_mul(FP_STEP);
            h = (h ^ c.no as u64).wrapping_mul(FP_STEP);
            h = (h ^ c.kind as u64).wrapping_mul(FP_STEP);
            h = (h ^ c.key as u64).wrapping_mul(FP_STEP);
            h = (h ^ c.state as u64).wrapping_mul(FP_STEP);
            k = k.wrapping_add(1);
        }
        h
    }
}

/// 标准 SysV 账：四十个对象按型轮转，各自走完四态生命周期。
pub fn build_sysv() -> SysvClear {
    let mut book = SysvClear::new();
    let mut batch = BATCH_FIRST;
    while batch <= BATCH_LAST {
        let mut no = 1usize;
        while no <= CRIT_PER_BATCH {
            let slot = book.case_count() % 3;
            let kind = if slot == 0 {
                KIND_SHM
            } else if slot == 1 {
                KIND_SEM
            } else {
                KIND_MSG
            };
            let key = 0x1000u32.wrapping_add(book.case_count() as u32);
            let _ = book.file(batch, no, kind, key);
            let _ = book.advance(batch, no);
            let _ = book.advance(batch, no);
            let _ = book.advance(batch, no);
            let _ = book.replay(batch, no);
            no = no.wrapping_add(1);
        }
        batch = batch.wrapping_add(1);
    }
    book
}

/// 四路对抗：批号越界、类型越界、同型键重复、未删除即重放。
pub fn inject_four() -> [bool; 4] {
    let mut a_book = SysvClear::new();
    let a = a_book.file(12, 1, KIND_SHM, 0x2001) == Err(KS_BATCH)
        && a_book.file(15, 1, KIND_SHM, 0x2001) == Err(KS_BATCH);
    let mut b_book = SysvClear::new();
    let b = b_book.file(13, 1, 4, 0x2001) == Err(KS_KIND) && b_book.case_count() == 0;
    let mut c_book = SysvClear::new();
    let _ = c_book.file(13, 1, KIND_SHM, 0x2001);
    let c = c_book.file(13, 2, KIND_SHM, 0x2001) == Err(KS_KEY) && c_book.case_count() == 1;
    let mut d_book = SysvClear::new();
    let _ = d_book.file(13, 1, KIND_SEM, 0x2001);
    let d = d_book.replay(13, 1) == Err(KS_STATE) && d_book.alive() == 1;
    [a, b, c, d]
}

/// F11130 全部检查：SysV 族判据终重放与生命周期账闭合。
pub fn run_unx11130_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11130 B13-B14 清账");
    let book = build_sysv();
    let four = inject_four();

    // J1 两批四十条在册。
    s.add(
        "J1-两批在册",
        book.case_count() == CRITS && CRITS == 2 * CRIT_PER_BATCH,
        "B13 与 B14 各 20 个对象判据，共 40 条在册",
    );
    // J2 三型分布。
    s.add(
        "J2-三型分布",
        book.in_kind(KIND_SHM) > 0 && book.in_kind(KIND_SEM) > 0 && book.in_kind(KIND_MSG) > 0,
        "共享内存、信号量、消息队列三型各有对象在账",
    );
    // J3 生命周期闭合。
    s.add(
        "J3-生命周期",
        book.lifecycle_closed() && book.alive() == 0,
        "四十个对象全部走完四态并删除，生命周期账闭合",
    );
    // J4 四态推进。
    let mut step_book = SysvClear::new();
    let _ = step_book.file(13, 1, KIND_MSG, 0x3001);
    let s1 = step_book.advance(13, 1);
    let s2 = step_book.advance(13, 1);
    let s3 = step_book.advance(13, 1);
    s.add(
        "J4-四态推进",
        s1 == Ok(ST_ACQUIRED) && s2 == Ok(ST_RELEASED) && s3 == Ok(ST_REMOVED),
        "对象按获取、释放、删除三步步进，状态依次为 2、3、4",
    );
    // J5 批域闸。
    let mut batch_probe = SysvClear::new();
    s.add(
        "J5-批域闸",
        batch_probe.file(12, 1, KIND_SHM, 0x2001) == Err(KS_BATCH)
            && batch_probe.file(15, 1, KIND_SHM, 0x2001) == Err(KS_BATCH),
        "批号 12 与 15 越出 B13–B14，登记报 KS_BATCH",
    );
    // J6 类型闸。
    let mut kind_probe = SysvClear::new();
    s.add(
        "J6-类型闸",
        kind_probe.file(13, 1, 0, 0x2001) == Err(KS_KIND)
            && kind_probe.file(13, 1, 4, 0x2001) == Err(KS_KIND),
        "类型 0 与 4 越出 1..=3，登记报 KS_KIND",
    );
    // J7 键唯一闸。
    let mut key_probe = SysvClear::new();
    let _ = key_probe.file(13, 1, KIND_SHM, 0x2001);
    s.add(
        "J7-键唯一闸",
        key_probe.file(13, 2, KIND_SHM, 0x2001) == Err(KS_KEY)
            && key_probe.file(13, 2, KIND_SEM, 0x2001).is_ok(),
        "同型键重复报 KS_KEY，异型同键允许",
    );
    // J8 未删除闸。
    let mut alive_probe = SysvClear::new();
    let _ = alive_probe.file(13, 1, KIND_SEM, 0x2001);
    s.add(
        "J8-未删除闸",
        alive_probe.replay(13, 1) == Err(KS_STATE) && alive_probe.alive() == 1,
        "对象未删除即终重放报 KS_STATE",
    );
    // J9 越级推进闸。
    let mut over_probe = SysvClear::new();
    let _ = over_probe.file(13, 1, KIND_MSG, 0x2001);
    let _ = over_probe.advance(13, 1);
    let _ = over_probe.advance(13, 1);
    let _ = over_probe.advance(13, 1);
    s.add(
        "J9-越级推进闸",
        over_probe.advance(13, 1) == Err(KS_STATE),
        "已删除对象再次推进报 KS_STATE",
    );
    // J10 重复重放闸。
    let mut twice = SysvClear::new();
    let _ = twice.file(13, 1, KIND_SHM, 0x2001);
    let _ = twice.advance(13, 1);
    let _ = twice.advance(13, 1);
    let _ = twice.advance(13, 1);
    let first_ok = twice.replay(13, 1).is_ok();
    s.add(
        "J10-重放闸",
        first_ok && twice.replay(13, 1) == Err(KS_REPLAY) && twice.replayed() == 1,
        "同一对象二次终重放报 KS_REPLAY",
    );
    // J11 冻结拒写。
    let mut sealed = build_sysv();
    let root = sealed.freeze();
    s.add(
        "J11-冻结拒写",
        sealed.is_frozen()
            && sealed.file(13, 1, KIND_SHM, 0x9001) == Err(KS_FROZEN)
            && sealed.advance(13, 1) == Err(KS_FROZEN)
            && sealed.replay(13, 1) == Err(KS_FROZEN)
            && sealed.fold() == root,
        "冻结后登记、推进、重放均报 KS_FROZEN",
    );
    // J12 四路对抗。
    s.add(
        "J12-四路对抗",
        four[0] && four[1] && four[2] && four[3],
        "批域、类型、键重复、未删除四路全部被拒",
    );
    // J13 判据渲染。
    let case = SysvCase {
        batch: 13,
        no: 1,
        kind: KIND_SHM,
        key: 0x1001,
        state: ST_REMOVED,
        replayed: true,
    };
    s.add(
        "J13-判据渲染",
        case.render() == "V13-01|SHM|0x1001|REMOVED|R" && case.settled(),
        "首个共享内存对象渲染为 V13-01|SHM|0x1001|REMOVED|R",
    );
    // J14 动态明细。
    s.add_dyn(
        "J14-清账明细",
        book.closed(),
        &format!(
            "条={} 重放={} 存活={} 信号量={}",
            book.case_count(),
            book.replayed(),
            book.alive(),
            book.in_kind(KIND_SEM)
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11130_sysv_flow() {
        let mut book = build_sysv();
        assert_eq!(book.case_count(), CRITS);
        assert_eq!(book.replayed(), CRITS);
        assert_eq!(book.alive(), 0);
        assert!(book.lifecycle_closed());
        assert!(book.closed());
        let root = book.freeze();
        assert_eq!(book.fold(), root);
        assert_eq!(book.advance(13, 1), Err(KS_FROZEN));
    }

    #[test]
    fn unx11130_checks_green() {
        let s = run_unx11130_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
