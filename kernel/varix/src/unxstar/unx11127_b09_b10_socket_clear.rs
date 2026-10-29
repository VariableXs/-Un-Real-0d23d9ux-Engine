//! UNX-F11127 B09–B10 清账（unix socket 主体判据终重放）（0x1f5a7 段）
//!
//! unix socket 主体占 B09 与 B10 两批，每批 20 条判据，按流式、数据报、
//! 顺序包三型分账。未绑定不得连接，三型账逐条闭合后主体清账才成立。

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址：F11127 独占 0x1f5a7。
pub const SEG_BASE: u32 = 0x1f5a7;

/// 批号越出 B09–B10。
pub const SK_BATCH: u32 = SEG_BASE + 0x01;
/// 判据号越出 1..=20。
pub const SK_NO: u32 = SEG_BASE + 0x02;
/// socket 类型越出 1..=3。
pub const SK_KIND: u32 = SEG_BASE + 0x03;
/// 判据重复登记。
pub const SK_DUP: u32 = SEG_BASE + 0x04;
/// 判据不连续。
pub const SK_ORDER: u32 = SEG_BASE + 0x05;
/// 未绑定即连接。
pub const SK_CONN: u32 = SEG_BASE + 0x06;
/// 判据重复重放。
pub const SK_REPLAY: u32 = SEG_BASE + 0x07;
/// 清账账已冻结。
pub const SK_FROZEN: u32 = SEG_BASE + 0x08;

/// 首批号。
pub const BATCH_FIRST: usize = 9;
/// 末批号。
pub const BATCH_LAST: usize = 10;
/// 每批判据数。
pub const CRIT_PER_BATCH: usize = 20;
/// 判据总数。
pub const CRITS: usize = 40;
/// 流式类型。
pub const KIND_STREAM: u8 = 1;
/// 数据报类型。
pub const KIND_DGRAM: u8 = 2;
/// 顺序包类型。
pub const KIND_SEQPACKET: u8 = 3;
/// 类型数。
pub const KINDS: usize = 3;
/// 折叠种子。
const SK_TAG: u64 = 0x0000_1f5_a7a7;
/// FNV-1a 素数。
const FP_STEP: u64 = 0x100000001b3;

/// socket 主体判据。
#[derive(Clone, Debug)]
pub struct SockCase {
    /// 批号（9 或 10）。
    pub batch: usize,
    /// 批内判据号。
    pub no: usize,
    /// 类型（1 流式 / 2 数据报 / 3 顺序包）。
    pub kind: u8,
    /// 是否已绑定。
    pub bound: bool,
    /// 是否已连接。
    pub connected: bool,
    /// 是否已终重放。
    pub replayed: bool,
}

impl SockCase {
    /// 连接前置：已连接必须已绑定。
    pub fn consistent(&self) -> bool {
        !self.connected || self.bound
    }

    /// 判据是否闭合。
    pub fn settled(&self) -> bool {
        self.consistent() && self.bound && self.replayed
    }

    /// 类型名。
    pub fn kind_name(&self) -> String {
        let name = match self.kind {
            KIND_STREAM => "STREAM",
            KIND_DGRAM => "DGRAM",
            KIND_SEQPACKET => "SEQPACKET",
            _ => "BAD",
        };
        String::from(name)
    }

    /// 判据渲染，形如 K9-03|STREAM|B1C1|R。
    pub fn render(&self) -> String {
        let b = if self.bound { 1 } else { 0 };
        let c = if self.connected { 1 } else { 0 };
        let r = if self.replayed { "R" } else { "N" };
        format!("K{}-{:02}|{}|B{}C{}|{}", self.batch, self.no, self.kind_name(), b, c, r)
    }
}

/// socket 主体清账账。
#[derive(Clone, Debug)]
pub struct SockClear {
    cases: Vec<SockCase>,
    frozen: bool,
    root: u64,
}

impl SockClear {
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

    /// 登记判据：批、号、类型、重复、顺序、冻结六道闸。
    pub fn file(&mut self, batch: usize, no: usize, kind: u8) -> Result<(), u32> {
        if self.frozen {
            return Err(SK_FROZEN);
        }
        if batch < BATCH_FIRST || batch > BATCH_LAST {
            return Err(SK_BATCH);
        }
        if no == 0 || no > CRIT_PER_BATCH {
            return Err(SK_NO);
        }
        if kind < KIND_STREAM || kind > KIND_SEQPACKET {
            return Err(SK_KIND);
        }
        if self.index_of(batch, no) != usize::MAX {
            return Err(SK_DUP);
        }
        let want = self.cases.len() / CRIT_PER_BATCH + BATCH_FIRST;
        if batch != want || self.cases.len() % CRIT_PER_BATCH != no.saturating_sub(1) {
            return Err(SK_ORDER);
        }
        self.cases.push(SockCase {
            batch,
            no,
            kind,
            bound: false,
            connected: false,
            replayed: false,
        });
        Ok(())
    }

    /// 绑定。
    pub fn bind(&mut self, batch: usize, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(SK_FROZEN);
        }
        let idx = self.index_of(batch, no);
        if idx == usize::MAX {
            return Err(SK_NO);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            c.bound = true;
            return Ok(());
        }
        Err(SK_NO)
    }

    /// 连接：必须已绑定。
    pub fn connect(&mut self, batch: usize, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(SK_FROZEN);
        }
        let idx = self.index_of(batch, no);
        if idx == usize::MAX {
            return Err(SK_NO);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            if !c.bound {
                return Err(SK_CONN);
            }
            c.connected = true;
            return Ok(());
        }
        Err(SK_NO)
    }

    /// 终重放一条判据。
    pub fn replay(&mut self, batch: usize, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(SK_FROZEN);
        }
        let idx = self.index_of(batch, no);
        if idx == usize::MAX {
            return Err(SK_NO);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            if !c.consistent() || !c.bound {
                return Err(SK_CONN);
            }
            if c.replayed {
                return Err(SK_REPLAY);
            }
            c.replayed = true;
            return Ok(());
        }
        Err(SK_NO)
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

    /// 某型判据数。
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

    /// 某型是否逐条闭合。
    pub fn kind_closed(&self, kind: u8) -> bool {
        if self.in_kind(kind) == 0 {
            return false;
        }
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].kind == kind && !self.cases[k].settled() {
                return false;
            }
            k = k.wrapping_add(1);
        }
        true
    }

    /// 三型账是否全部闭合。
    pub fn tri_closed(&self) -> bool {
        self.kind_closed(KIND_STREAM)
            && self.kind_closed(KIND_DGRAM)
            && self.kind_closed(KIND_SEQPACKET)
    }

    /// 是否全部闭合。
    pub fn closed(&self) -> bool {
        self.cases.len() == CRITS && self.replayed() == CRITS && self.tri_closed()
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
        let mut h = SK_TAG;
        let mut k = 0usize;
        while k < self.cases.len() {
            let c = &self.cases[k];
            h = (h ^ c.batch as u64).wrapping_mul(FP_STEP);
            h = (h ^ c.no as u64).wrapping_mul(FP_STEP);
            h = (h ^ c.kind as u64).wrapping_mul(FP_STEP);
            let b = if c.replayed { 1u64 } else { 2u64 };
            h = (h ^ b).wrapping_mul(FP_STEP);
            k = k.wrapping_add(1);
        }
        h
    }
}

/// 标准 socket 清账账：四十条判据按三型轮转，绑定后流式与顺序包连接。
pub fn build_sock() -> SockClear {
    let mut book = SockClear::new();
    let mut batch = BATCH_FIRST;
    while batch <= BATCH_LAST {
        let mut no = 1usize;
        while no <= CRIT_PER_BATCH {
            let slot = book.case_count() % KINDS;
            let kind = if slot == 0 {
                KIND_STREAM
            } else if slot == 1 {
                KIND_DGRAM
            } else {
                KIND_SEQPACKET
            };
            let _ = book.file(batch, no, kind);
            let _ = book.bind(batch, no);
            if kind != KIND_DGRAM {
                let _ = book.connect(batch, no);
            }
            let _ = book.replay(batch, no);
            no = no.wrapping_add(1);
        }
        batch = batch.wrapping_add(1);
    }
    book
}

/// 四路对抗：批号越界、判据号越界、类型越界、未绑定即连接。
pub fn inject_four() -> [bool; 4] {
    let mut a_book = SockClear::new();
    let a = a_book.file(8, 1, KIND_STREAM) == Err(SK_BATCH)
        && a_book.file(11, 1, KIND_STREAM) == Err(SK_BATCH);
    let mut b_book = SockClear::new();
    let b = b_book.file(9, 21, KIND_STREAM) == Err(SK_NO) && b_book.case_count() == 0;
    let mut c_book = SockClear::new();
    let c = c_book.file(9, 1, 4) == Err(SK_KIND) && c_book.case_count() == 0;
    let mut d_book = SockClear::new();
    let _ = d_book.file(9, 1, KIND_STREAM);
    let d = d_book.connect(9, 1) == Err(SK_CONN) && d_book.replay(9, 1) == Err(SK_CONN);
    [a, b, c, d]
}

/// F11127 全部检查：socket 族判据终重放与三型账闭合。
pub fn run_unx11127_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11127 B09-B10 清账");
    let book = build_sock();
    let four = inject_four();

    // J1 两批四十条在册。
    s.add(
        "J1-两批在册",
        book.case_count() == CRITS && CRITS == 2 * CRIT_PER_BATCH,
        "B09 与 B10 各 20 条判据，共 40 条在册",
    );
    // J2 终重放全量。
    s.add("J2-终重放", book.replayed() == CRITS, "四十条判据全部完成终重放");
    // J3 三型分账。
    s.add(
        "J3-三型分账",
        book.in_kind(KIND_STREAM) > 0
            && book.in_kind(KIND_DGRAM) > 0
            && book.in_kind(KIND_SEQPACKET) > 0,
        "流式、数据报、顺序包三型各有判据在账",
    );
    // J4 三型闭合。
    s.add(
        "J4-三型闭合",
        book.tri_closed()
            && book.kind_closed(KIND_STREAM)
            && book.kind_closed(KIND_DGRAM)
            && book.kind_closed(KIND_SEQPACKET),
        "三型账逐条闭合，无悬空判据",
    );
    // J5 批域闸。
    let mut batch_probe = SockClear::new();
    s.add(
        "J5-批域闸",
        batch_probe.file(8, 1, KIND_STREAM) == Err(SK_BATCH)
            && batch_probe.file(11, 1, KIND_STREAM) == Err(SK_BATCH),
        "批号 8 与 11 越出 B09–B10，登记报 SK_BATCH",
    );
    // J6 判据号闸。
    let mut no_probe = SockClear::new();
    s.add(
        "J6-判据号闸",
        no_probe.file(9, 0, KIND_STREAM) == Err(SK_NO)
            && no_probe.file(9, 21, KIND_STREAM) == Err(SK_NO),
        "判据号 0 与 21 越出 1..=20，登记报 SK_NO",
    );
    // J7 类型闸。
    let mut kind_probe = SockClear::new();
    s.add(
        "J7-类型闸",
        kind_probe.file(9, 1, 0) == Err(SK_KIND) && kind_probe.file(9, 1, 4) == Err(SK_KIND),
        "类型 0 与 4 越出 1..=3，登记报 SK_KIND",
    );
    // J8 连接前置闸。
    let mut conn_probe = SockClear::new();
    let _ = conn_probe.file(9, 1, KIND_STREAM);
    s.add(
        "J8-连接前置闸",
        conn_probe.connect(9, 1) == Err(SK_CONN) && conn_probe.replay(9, 1) == Err(SK_CONN),
        "未绑定即连接报 SK_CONN，未绑定判据也不得重放",
    );
    // J9 重号闸。
    let mut dup_probe = SockClear::new();
    let _ = dup_probe.file(9, 1, KIND_DGRAM);
    s.add(
        "J9-重号闸",
        dup_probe.file(9, 1, KIND_STREAM) == Err(SK_DUP) && dup_probe.case_count() == 1,
        "同一判据二次登记报 SK_DUP",
    );
    // J10 重复重放闸。
    let mut twice = SockClear::new();
    let _ = twice.file(9, 1, KIND_DGRAM);
    let _ = twice.bind(9, 1);
    let first_ok = twice.replay(9, 1).is_ok();
    s.add(
        "J10-重放闸",
        first_ok && twice.replay(9, 1) == Err(SK_REPLAY) && twice.replayed() == 1,
        "同一条判据二次终重放报 SK_REPLAY",
    );
    // J11 冻结拒写。
    let mut sealed = build_sock();
    let root = sealed.freeze();
    s.add(
        "J11-冻结拒写",
        sealed.is_frozen()
            && sealed.file(9, 1, KIND_STREAM) == Err(SK_FROZEN)
            && sealed.bind(9, 1) == Err(SK_FROZEN)
            && sealed.connect(9, 1) == Err(SK_FROZEN)
            && sealed.fold() == root,
        "冻结后登记、绑定、连接均报 SK_FROZEN",
    );
    // J12 四路对抗。
    s.add(
        "J12-四路对抗",
        four[0] && four[1] && four[2] && four[3],
        "批域、判据号、类型、未绑定连接四路全部被拒",
    );
    // J13 判据渲染。
    let case = SockCase {
        batch: 9,
        no: 3,
        kind: KIND_STREAM,
        bound: true,
        connected: true,
        replayed: true,
    };
    s.add(
        "J13-判据渲染",
        case.render() == "K9-03|STREAM|B1C1|R" && case.settled(),
        "B09 第 3 条流式判据渲染为 K9-03|STREAM|B1C1|R",
    );
    // J14 动态明细。
    s.add_dyn(
        "J14-清账明细",
        book.closed(),
        &format!(
            "条={} 重放={} 流={} 报={} 序={}",
            book.case_count(),
            book.replayed(),
            book.in_kind(KIND_STREAM),
            book.in_kind(KIND_DGRAM),
            book.in_kind(KIND_SEQPACKET)
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11127_sock_flow() {
        let mut book = build_sock();
        assert_eq!(book.case_count(), CRITS);
        assert_eq!(book.replayed(), CRITS);
        assert!(book.tri_closed());
        assert!(book.closed());
        let root = book.freeze();
        assert_eq!(book.fold(), root);
        assert_eq!(book.connect(9, 1), Err(SK_FROZEN));
    }

    #[test]
    fn unx11127_checks_green() {
        let s = run_unx11127_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
