//! UNX-F11132 B16–B17 清账（socket 深水判据终重放）（0x1f5ac 段）
//!
//! 深水族占 B16 与 B17 两批。每条连接记录积压上限、在队字节与连接状态：
//! 在队不得超过上限，关闭前必须排空，只有已关闭连接才能进入终重放。

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址：F11132 独占 0x1f5ac。
pub const SEG_BASE: u32 = 0x1f5ac;

/// 批号越出 B16–B17。
pub const DS_BATCH: u32 = SEG_BASE + 0x01;
/// 判据号越出 1..=20。
pub const DS_NO: u32 = SEG_BASE + 0x02;
/// 连接状态不允许该动作。
pub const DS_STATE: u32 = SEG_BASE + 0x03;
/// 在队字节超过积压上限。
pub const DS_BACKLOG: u32 = SEG_BASE + 0x04;
/// 判据重复登记。
pub const DS_DUP: u32 = SEG_BASE + 0x05;
/// 判据号不连续。
pub const DS_ORDER: u32 = SEG_BASE + 0x06;
/// 判据重复重放。
pub const DS_REPLAY: u32 = SEG_BASE + 0x07;
/// 清账账已冻结。
pub const DS_FROZEN: u32 = SEG_BASE + 0x08;

/// 首批号。
pub const BATCH_FIRST: usize = 16;
/// 末批号。
pub const BATCH_LAST: usize = 17;
/// 每批判据数。
pub const CRIT_PER_BATCH: usize = 20;
/// 判据总数。
pub const CRITS: usize = 40;
/// 积压上限。
pub const BACKLOG: usize = 8;
/// 折叠种子。
const DS_TAG: u64 = 0x0000_1f5_acac;
/// FNV-1a 素数。
const FP_STEP: u64 = 0x100000001b3;

/// 深水连接判据。
#[derive(Clone, Debug)]
pub struct ConnCase {
    /// 批号（16 或 17）。
    pub batch: usize,
    /// 批内判据号。
    pub no: usize,
    /// 积压上限。
    pub backlog: usize,
    /// 在队字节。
    pub queued: usize,
    /// 是否已建立。
    pub established: bool,
    /// 是否已关闭。
    pub closed: bool,
    /// 是否已终重放。
    pub replayed: bool,
}

impl ConnCase {
    /// 是否已排空。
    pub fn drained(&self) -> bool {
        self.queued == 0
    }

    /// 水位是否安全：在队不超过上限。
    pub fn safe(&self) -> bool {
        self.queued <= self.backlog
    }

    /// 判据是否闭合。
    pub fn settled(&self) -> bool {
        self.closed && self.safe() && self.drained() && self.replayed
    }

    /// 状态名。
    pub fn state_name(&self) -> &'static str {
        if self.closed {
            "CLOSED"
        } else if self.established {
            "ESTABLISHED"
        } else {
            "INIT"
        }
    }

    /// 判据渲染，形如 D16-01|ESTABLISHED|0/8|R。
    pub fn render(&self) -> String {
        let r = if self.replayed { "R" } else { "N" };
        format!(
            "D{}-{:02}|{}|{}/{}|{}",
            self.batch,
            self.no,
            self.state_name(),
            self.queued,
            self.backlog,
            r
        )
    }
}

/// 深水族清账账。
#[derive(Clone, Debug)]
pub struct DeepClear {
    cases: Vec<ConnCase>,
    frozen: bool,
    root: u64,
}

impl DeepClear {
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

    /// 登记连接：批、号、重复、顺序、冻结五道闸。
    pub fn file(&mut self, batch: usize, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(DS_FROZEN);
        }
        if batch < BATCH_FIRST || batch > BATCH_LAST {
            return Err(DS_BATCH);
        }
        if no == 0 || no > CRIT_PER_BATCH {
            return Err(DS_NO);
        }
        if self.index_of(batch, no) != usize::MAX {
            return Err(DS_DUP);
        }
        let want = self.cases.len() / CRIT_PER_BATCH + BATCH_FIRST;
        if batch != want || self.cases.len() % CRIT_PER_BATCH != no.saturating_sub(1) {
            return Err(DS_ORDER);
        }
        self.cases.push(ConnCase {
            batch,
            no,
            backlog: BACKLOG,
            queued: 0,
            established: false,
            closed: false,
            replayed: false,
        });
        Ok(())
    }

    /// 建立连接。
    pub fn establish(&mut self, batch: usize, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(DS_FROZEN);
        }
        let idx = self.index_of(batch, no);
        if idx == usize::MAX {
            return Err(DS_NO);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            if c.closed {
                return Err(DS_STATE);
            }
            c.established = true;
            return Ok(());
        }
        Err(DS_NO)
    }

    /// 入队：不得超过积压上限。
    pub fn enqueue(&mut self, batch: usize, no: usize, bytes: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(DS_FROZEN);
        }
        let idx = self.index_of(batch, no);
        if idx == usize::MAX {
            return Err(DS_NO);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            if !c.established || c.closed {
                return Err(DS_STATE);
            }
            let next = c.queued.wrapping_add(bytes);
            if next > c.backlog {
                return Err(DS_BACKLOG);
            }
            c.queued = next;
            return Ok(());
        }
        Err(DS_NO)
    }

    /// 排空在队字节。
    pub fn drain(&mut self, batch: usize, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(DS_FROZEN);
        }
        let idx = self.index_of(batch, no);
        if idx == usize::MAX {
            return Err(DS_NO);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            if !c.established {
                return Err(DS_STATE);
            }
            c.queued = 0;
            return Ok(());
        }
        Err(DS_NO)
    }

    /// 关闭连接：必须已排空。
    pub fn close(&mut self, batch: usize, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(DS_FROZEN);
        }
        let idx = self.index_of(batch, no);
        if idx == usize::MAX {
            return Err(DS_NO);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            if !c.established || !c.drained() {
                return Err(DS_STATE);
            }
            c.closed = true;
            return Ok(());
        }
        Err(DS_NO)
    }

    /// 终重放：连接必须已关闭且排空。
    pub fn replay(&mut self, batch: usize, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(DS_FROZEN);
        }
        let idx = self.index_of(batch, no);
        if idx == usize::MAX {
            return Err(DS_NO);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            if !c.closed || !c.drained() {
                return Err(DS_STATE);
            }
            if c.replayed {
                return Err(DS_REPLAY);
            }
            c.replayed = true;
            return Ok(());
        }
        Err(DS_NO)
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

    /// 已建立连接数。
    pub fn established(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].established {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 已关闭连接数。
    pub fn closed_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].closed {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 仍有在队字节的连接数。
    pub fn backlogged(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.cases.len() {
            if !self.cases[k].drained() {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 连接账是否逐条闭合。
    pub fn conn_closed(&self) -> bool {
        if self.cases.len() != CRITS || self.closed_count() != CRITS || self.replayed() != CRITS {
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
        let mut h = DS_TAG;
        let mut k = 0usize;
        while k < self.cases.len() {
            let c = &self.cases[k];
            h = (h ^ c.batch as u64).wrapping_mul(FP_STEP);
            h = (h ^ c.no as u64).wrapping_mul(FP_STEP);
            h = (h ^ c.queued as u64).wrapping_mul(FP_STEP);
            let b = if c.closed { 1u64 } else { 2u64 };
            h = (h ^ b).wrapping_mul(FP_STEP);
            k = k.wrapping_add(1);
        }
        h
    }
}

/// 标准深水账：四十条连接依次建立、入队、排空、关闭并重放。
pub fn build_deep() -> DeepClear {
    let mut book = DeepClear::new();
    let mut batch = BATCH_FIRST;
    while batch <= BATCH_LAST {
        let mut no = 1usize;
        while no <= CRIT_PER_BATCH {
            let _ = book.file(batch, no);
            let _ = book.establish(batch, no);
            let _ = book.enqueue(batch, no, no % BACKLOG);
            let _ = book.drain(batch, no);
            let _ = book.close(batch, no);
            let _ = book.replay(batch, no);
            no = no.wrapping_add(1);
        }
        batch = batch.wrapping_add(1);
    }
    book
}

/// 四路对抗：批号越界、未建立即入队、超积压入队、未排空关闭。
pub fn inject_four() -> [bool; 4] {
    let mut a_book = DeepClear::new();
    let a = a_book.file(15, 1) == Err(DS_BATCH) && a_book.file(18, 1) == Err(DS_BATCH);
    let mut b_book = DeepClear::new();
    let _ = b_book.file(16, 1);
    let b = b_book.enqueue(16, 1, 1) == Err(DS_STATE);
    let mut c_book = DeepClear::new();
    let _ = c_book.file(16, 1);
    let _ = c_book.establish(16, 1);
    let c = c_book.enqueue(16, 1, BACKLOG + 1) == Err(DS_BACKLOG) && c_book.backlogged() == 0;
    let mut d_book = DeepClear::new();
    let _ = d_book.file(16, 1);
    let _ = d_book.establish(16, 1);
    let _ = d_book.enqueue(16, 1, 3);
    let d = d_book.close(16, 1) == Err(DS_STATE) && d_book.closed_count() == 0;
    [a, b, c, d]
}

/// F11132 全部检查：深水族判据终重放与连接账闭合。
pub fn run_unx11132_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11132 B16-B17 清账");
    let book = build_deep();
    let four = inject_four();

    // J1 两批四十条在册。
    s.add(
        "J1-两批在册",
        book.case_count() == CRITS && CRITS == 2 * CRIT_PER_BATCH,
        "B16 与 B17 各 20 条深水判据，共 40 条在册",
    );
    // J2 连接全部建立。
    s.add(
        "J2-全部建立",
        book.established() == CRITS,
        "四十条连接全部进入已建立状态",
    );
    // J3 连接全部关闭。
    s.add(
        "J3-全部关闭",
        book.closed_count() == CRITS && book.backlogged() == 0,
        "四十条连接全部关闭且无在队字节残留",
    );
    // J4 连接账闭合。
    s.add(
        "J4-连接账闭合",
        book.conn_closed() && book.replayed() == CRITS,
        "连接账逐条闭合，四十条判据全部终重放",
    );
    // J5 批域闸。
    let mut batch_probe = DeepClear::new();
    s.add(
        "J5-批域闸",
        batch_probe.file(15, 1) == Err(DS_BATCH) && batch_probe.file(18, 1) == Err(DS_BATCH),
        "批号 15 与 18 越出 B16–B17，登记报 DS_BATCH",
    );
    // J6 判据号闸。
    let mut no_probe = DeepClear::new();
    s.add(
        "J6-判据号闸",
        no_probe.file(16, 0) == Err(DS_NO) && no_probe.file(16, 21) == Err(DS_NO),
        "判据号 0 与 21 越出 1..=20，登记报 DS_NO",
    );
    // J7 建立前置闸。
    let mut state_probe = DeepClear::new();
    let _ = state_probe.file(16, 1);
    s.add(
        "J7-建立前置闸",
        state_probe.enqueue(16, 1, 1) == Err(DS_STATE) && state_probe.drain(16, 1) == Err(DS_STATE),
        "未建立连接就入队或排空报 DS_STATE",
    );
    // J8 积压闸。
    let mut backlog_probe = DeepClear::new();
    let _ = backlog_probe.file(16, 1);
    let _ = backlog_probe.establish(16, 1);
    s.add(
        "J8-积压闸",
        backlog_probe.enqueue(16, 1, BACKLOG + 1) == Err(DS_BACKLOG)
            && backlog_probe.backlogged() == 0,
        "单次入队超过积压上限报 DS_BACKLOG，在队保持为零",
    );
    // J9 排空闸。
    let mut close_probe = DeepClear::new();
    let _ = close_probe.file(16, 1);
    let _ = close_probe.establish(16, 1);
    let _ = close_probe.enqueue(16, 1, 3);
    s.add(
        "J9-排空闸",
        close_probe.close(16, 1) == Err(DS_STATE) && close_probe.closed_count() == 0,
        "尚有 3 字节在队时关闭连接报 DS_STATE",
    );
    // J10 终重放闸。
    let mut replay_probe = DeepClear::new();
    let _ = replay_probe.file(16, 1);
    let _ = replay_probe.establish(16, 1);
    let _ = replay_probe.drain(16, 1);
    s.add(
        "J10-终重放闸",
        replay_probe.replay(16, 1) == Err(DS_STATE) && replay_probe.replayed() == 0,
        "连接未关闭就终重放报 DS_STATE",
    );
    // J11 重号闸。
    let mut dup_probe = DeepClear::new();
    let _ = dup_probe.file(16, 1);
    s.add(
        "J11-重号闸",
        dup_probe.file(16, 1) == Err(DS_DUP) && dup_probe.case_count() == 1,
        "同一批同一判据号二次登记报 DS_DUP",
    );
    // J12 冻结拒写。
    let mut sealed = build_deep();
    let root = sealed.freeze();
    s.add(
        "J12-冻结拒写",
        sealed.is_frozen()
            && sealed.file(16, 1) == Err(DS_FROZEN)
            && sealed.establish(16, 1) == Err(DS_FROZEN)
            && sealed.enqueue(16, 1, 1) == Err(DS_FROZEN)
            && sealed.fold() == root,
        "冻结后登记、建立、入队均报 DS_FROZEN",
    );
    // J13 四路对抗。
    s.add(
        "J13-四路对抗",
        four[0] && four[1] && four[2] && four[3],
        "批域、建立前置、积压、未排空关闭四路全部被拒",
    );
    // J14 判据渲染。
    let case = ConnCase {
        batch: 16,
        no: 1,
        backlog: BACKLOG,
        queued: 0,
        established: true,
        closed: false,
        replayed: true,
    };
    s.add(
        "J14-判据渲染",
        case.render() == "D16-01|ESTABLISHED|0/8|R" && case.safe(),
        "已排空连接渲染为 D16-01|ESTABLISHED|0/8|R",
    );
    // J15 动态明细。
    s.add_dyn(
        "J15-清账明细",
        book.conn_closed(),
        &format!(
            "条={} 建立={} 关闭={} 在队={}",
            book.case_count(),
            book.established(),
            book.closed_count(),
            book.backlogged()
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11132_deep_flow() {
        let mut book = build_deep();
        assert_eq!(book.case_count(), CRITS);
        assert_eq!(book.established(), CRITS);
        assert_eq!(book.closed_count(), CRITS);
        assert!(book.conn_closed());
        let root = book.freeze();
        assert_eq!(book.fold(), root);
        assert_eq!(book.drain(16, 1), Err(DS_FROZEN));
    }

    #[test]
    fn unx11132_checks_green() {
        let s = run_unx11132_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
