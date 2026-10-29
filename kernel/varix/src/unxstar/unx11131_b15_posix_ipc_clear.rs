//! UNX-F11131 B15 清账（POSIX IPC 判据终重放）（0x1f5ab 段）
//!
//! POSIX IPC 批 B15 共 20 条判据，命名信号量与消息队列混在同一命名空间。
//! 名字必须以斜杠开头且全场唯一；每条判据打开、关闭、取消链接后终重放。

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址：F11131 独占 0x1f5ab。
pub const SEG_BASE: u32 = 0x1f5ab;

/// 判据号越出 1..=20。
pub const PX_NO: u32 = SEG_BASE + 0x01;
/// IPC 名不合法。
pub const PX_NAME: u32 = SEG_BASE + 0x02;
/// 类型越出 1..=2。
pub const PX_KIND: u32 = SEG_BASE + 0x03;
/// 名字在混合场中重复。
pub const PX_DUP: u32 = SEG_BASE + 0x04;
/// 未打开就关闭。
pub const PX_OPEN: u32 = SEG_BASE + 0x05;
/// 判据号不连续。
pub const PX_ORDER: u32 = SEG_BASE + 0x06;
/// 未取消链接或引用未归零。
pub const PX_UNLINK: u32 = SEG_BASE + 0x07;
/// 清账账已冻结。
pub const PX_FROZEN: u32 = SEG_BASE + 0x08;

/// 判据总数。
pub const CRITS: usize = 20;
/// 命名信号量。
pub const KIND_SEM: u8 = 1;
/// 消息队列。
pub const KIND_MQ: u8 = 2;
/// 名字最小长度。
pub const NAME_MIN: usize = 2;
/// 折叠种子。
const PX_TAG: u64 = 0x0000_1f5_abab;
/// FNV-1a 素数。
const FP_STEP: u64 = 0x100000001b3;

/// POSIX IPC 判据。
#[derive(Clone, Debug)]
pub struct PosixCase {
    /// 判据号（1..=20）。
    pub no: usize,
    /// 类型（1 命名信号量 / 2 消息队列）。
    pub kind: u8,
    /// IPC 名（斜杠开头）。
    pub name: String,
    /// 打开次数。
    pub opens: usize,
    /// 关闭次数。
    pub closes: usize,
    /// 是否已取消链接。
    pub unlinked: bool,
    /// 是否已终重放。
    pub replayed: bool,
}

impl PosixCase {
    /// 名字是否合法：斜杠开头且长度达标。
    pub fn name_ok(&self) -> bool {
        if self.name.len() < NAME_MIN {
            return false;
        }
        let bytes = self.name.as_bytes();
        if let Some(first) = bytes.first() {
            return *first == b'/';
        }
        false
    }

    /// 引用是否归零。
    pub fn refs_zero(&self) -> bool {
        self.opens > 0 && self.opens == self.closes
    }

    /// 判据是否闭合。
    pub fn settled(&self) -> bool {
        self.name_ok() && self.refs_zero() && self.unlinked && self.replayed
    }

    /// 类型名。
    pub fn kind_name(&self) -> String {
        let name = match self.kind {
            KIND_SEM => "SEM",
            KIND_MQ => "MQ",
            _ => "BAD",
        };
        String::from(name)
    }

    /// 判据渲染，形如 X05|MQ|/vx05|1/1|U|R。
    pub fn render(&self) -> String {
        let u = if self.unlinked { "U" } else { "K" };
        let r = if self.replayed { "R" } else { "N" };
        format!(
            "X{:02}|{}|{}|{}/{}|{}|{}",
            self.no,
            self.kind_name(),
            self.name,
            self.opens,
            self.closes,
            u,
            r
        )
    }
}

/// POSIX IPC 混合场清账账。
#[derive(Clone, Debug)]
pub struct PosixClear {
    cases: Vec<PosixCase>,
    frozen: bool,
    root: u64,
}

impl PosixClear {
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

    /// 登记判据：号、类型、名字、重名、顺序、冻结六道闸。
    pub fn file(&mut self, no: usize, kind: u8, name: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(PX_FROZEN);
        }
        if no == 0 || no > CRITS {
            return Err(PX_NO);
        }
        if kind < KIND_SEM || kind > KIND_MQ {
            return Err(PX_KIND);
        }
        if !name.starts_with('/') || name.len() < NAME_MIN {
            return Err(PX_NAME);
        }
        if self.index_of(no) != usize::MAX {
            return Err(PX_DUP);
        }
        if self.cases.len().wrapping_add(1) != no {
            return Err(PX_ORDER);
        }
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].name == name {
                return Err(PX_DUP);
            }
            k = k.wrapping_add(1);
        }
        let label = String::from(name);
        self.cases.push(PosixCase {
            no,
            kind,
            name: label,
            opens: 0,
            closes: 0,
            unlinked: false,
            replayed: false,
        });
        Ok(())
    }

    /// 打开一次。
    pub fn open(&mut self, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(PX_FROZEN);
        }
        let idx = self.index_of(no);
        if idx == usize::MAX {
            return Err(PX_NO);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            c.opens = c.opens.wrapping_add(1);
            return Ok(());
        }
        Err(PX_NO)
    }

    /// 关闭一次：引用必须为正。
    pub fn close(&mut self, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(PX_FROZEN);
        }
        let idx = self.index_of(no);
        if idx == usize::MAX {
            return Err(PX_NO);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            if c.closes >= c.opens {
                return Err(PX_OPEN);
            }
            c.closes = c.closes.wrapping_add(1);
            return Ok(());
        }
        Err(PX_NO)
    }

    /// 取消链接：允许在打开状态下进行。
    pub fn unlink(&mut self, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(PX_FROZEN);
        }
        let idx = self.index_of(no);
        if idx == usize::MAX {
            return Err(PX_NO);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            c.unlinked = true;
            return Ok(());
        }
        Err(PX_NO)
    }

    /// 终重放：名字合法、引用归零、已取消链接。
    pub fn replay(&mut self, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(PX_FROZEN);
        }
        let idx = self.index_of(no);
        if idx == usize::MAX {
            return Err(PX_NO);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            if !c.name_ok() {
                return Err(PX_NAME);
            }
            if !c.refs_zero() || !c.unlinked {
                return Err(PX_UNLINK);
            }
            if c.replayed {
                return Err(PX_DUP);
            }
            c.replayed = true;
            return Ok(());
        }
        Err(PX_NO)
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

    /// 未取消链接条数。
    pub fn linked(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.cases.len() {
            if !self.cases[k].unlinked {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 混合场是否闭合。
    pub fn field_closed(&self) -> bool {
        if self.cases.len() != CRITS || self.replayed() != CRITS || self.linked() != 0 {
            return false;
        }
        let mut k = 0usize;
        while k < self.cases.len() {
            if !self.cases[k].settled() {
                return false;
            }
            k = k.wrapping_add(1);
        }
        self.in_kind(KIND_SEM) > 0 && self.in_kind(KIND_MQ) > 0
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
        let mut h = PX_TAG;
        let mut k = 0usize;
        while k < self.cases.len() {
            let c = &self.cases[k];
            h = (h ^ c.no as u64).wrapping_mul(FP_STEP);
            h = (h ^ c.kind as u64).wrapping_mul(FP_STEP);
            h = (h ^ c.opens as u64).wrapping_mul(FP_STEP);
            h = (h ^ c.closes as u64).wrapping_mul(FP_STEP);
            let u = if c.unlinked { 1u64 } else { 2u64 };
            h = (h ^ u).wrapping_mul(FP_STEP);
            k = k.wrapping_add(1);
        }
        h
    }
}

/// 标准 POSIX 混合场：信号量与消息队列交替，逐条打开关闭取消链接。
pub fn build_posix() -> PosixClear {
    let mut book = PosixClear::new();
    let mut no = 1usize;
    while no <= CRITS {
        let kind = if no % 2 == 0 { KIND_MQ } else { KIND_SEM };
        let name = format!("/vx{:02}", no);
        let _ = book.file(no, kind, &name);
        let _ = book.open(no);
        let _ = book.close(no);
        let _ = book.unlink(no);
        let _ = book.replay(no);
        no = no.wrapping_add(1);
    }
    book
}

/// 四路对抗：判据号越界、名字不合法、重名、未打开就关闭。
pub fn inject_four() -> [bool; 4] {
    let mut a_book = PosixClear::new();
    let a = a_book.file(0, KIND_SEM, "/vx00") == Err(PX_NO)
        && a_book.file(21, KIND_SEM, "/vx21") == Err(PX_NO);
    let mut b_book = PosixClear::new();
    let b = b_book.file(1, KIND_SEM, "vx01") == Err(PX_NAME) && b_book.case_count() == 0;
    let mut c_book = PosixClear::new();
    let _ = c_book.file(1, KIND_SEM, "/vx01");
    let c = c_book.file(2, KIND_MQ, "/vx01") == Err(PX_DUP) && c_book.case_count() == 1;
    let mut d_book = PosixClear::new();
    let _ = d_book.file(1, KIND_MQ, "/vx01");
    let d = d_book.close(1) == Err(PX_OPEN);
    [a, b, c, d]
}

/// F11131 全部检查：POSIX 族判据终重放与混合场账闭合。
pub fn run_unx11131_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11131 B15 清账");
    let book = build_posix();
    let four = inject_four();

    // J1 二十条在册。
    s.add(
        "J1-判据在册",
        book.case_count() == CRITS && CRITS == 20,
        "B15 批 20 条 POSIX IPC 判据全部在册",
    );
    // J2 混合类型。
    s.add(
        "J2-混合类型",
        book.in_kind(KIND_SEM) > 0 && book.in_kind(KIND_MQ) > 0,
        "命名信号量与消息队列在同一混合场中各有判据",
    );
    // J3 终重放全量。
    s.add("J3-终重放", book.replayed() == CRITS, "二十条判据全部完成终重放");
    // J4 混合场闭合。
    s.add(
        "J4-混合场闭合",
        book.field_closed() && book.linked() == 0,
        "所有名字已取消链接且引用归零，混合场账闭合",
    );
    // J5 判据号闸。
    let mut no_probe = PosixClear::new();
    s.add(
        "J5-判据号闸",
        no_probe.file(0, KIND_SEM, "/vx00") == Err(PX_NO)
            && no_probe.file(21, KIND_SEM, "/vx21") == Err(PX_NO),
        "判据号 0 与 21 越出 1..=20，登记报 PX_NO",
    );
    // J6 名字闸。
    let mut name_probe = PosixClear::new();
    s.add(
        "J6-名字闸",
        name_probe.file(1, KIND_SEM, "vx01") == Err(PX_NAME)
            && name_probe.file(2, KIND_MQ, "/") == Err(PX_NAME),
        "缺斜杠或过短的名字登记报 PX_NAME",
    );
    // J7 重名闸。
    let mut dup_probe = PosixClear::new();
    let _ = dup_probe.file(1, KIND_SEM, "/vx01");
    s.add(
        "J7-重名闸",
        dup_probe.file(2, KIND_MQ, "/vx01") == Err(PX_DUP) && dup_probe.case_count() == 1,
        "同名对象跨类型重复登记报 PX_DUP",
    );
    // J8 未打开关闭闸。
    let mut open_probe = PosixClear::new();
    let _ = open_probe.file(1, KIND_MQ, "/vx01");
    s.add(
        "J8-未打开关闭闸",
        open_probe.close(1) == Err(PX_OPEN),
        "引用为零时关闭报 PX_OPEN",
    );
    // J9 引用未归零闸。
    let mut ref_probe = PosixClear::new();
    let _ = ref_probe.file(1, KIND_SEM, "/vx01");
    let _ = ref_probe.open(1);
    let _ = ref_probe.open(1);
    let _ = ref_probe.close(1);
    let _ = ref_probe.unlink(1);
    s.add(
        "J9-引用归零闸",
        ref_probe.replay(1) == Err(PX_UNLINK) && ref_probe.replayed() == 0,
        "打开两次只关闭一次时终重放报 PX_UNLINK",
    );
    // J10 未取消链接闸。
    let mut link_probe = PosixClear::new();
    let _ = link_probe.file(1, KIND_MQ, "/vx01");
    let _ = link_probe.open(1);
    let _ = link_probe.close(1);
    s.add(
        "J10-链接闸",
        link_probe.replay(1) == Err(PX_UNLINK) && link_probe.linked() == 1,
        "未取消链接时终重放报 PX_UNLINK，链接计数为 1",
    );
    // J11 冻结拒写。
    let mut sealed = build_posix();
    let root = sealed.freeze();
    s.add(
        "J11-冻结拒写",
        sealed.is_frozen()
            && sealed.file(1, KIND_SEM, "/zz01") == Err(PX_FROZEN)
            && sealed.open(1) == Err(PX_FROZEN)
            && sealed.close(1) == Err(PX_FROZEN)
            && sealed.unlink(1) == Err(PX_FROZEN)
            && sealed.fold() == root,
        "冻结后登记、打开、关闭、取消链接均报 PX_FROZEN",
    );
    // J12 四路对抗。
    s.add(
        "J12-四路对抗",
        four[0] && four[1] && four[2] && four[3],
        "判据号、名字、重名、未打开关闭四路全部被拒",
    );
    // J13 判据渲染。
    let case = PosixCase {
        no: 5,
        kind: KIND_MQ,
        name: String::from("/vx05"),
        opens: 1,
        closes: 1,
        unlinked: true,
        replayed: true,
    };
    s.add(
        "J13-判据渲染",
        case.render() == "X05|MQ|/vx05|1/1|U|R" && case.settled(),
        "第 5 条消息队列判据渲染为 X05|MQ|/vx05|1/1|U|R",
    );
    // J14 动态明细。
    s.add_dyn(
        "J14-清账明细",
        book.field_closed(),
        &format!(
            "条={} 重放={} 信号量={} 队列={} 未链接={}",
            book.case_count(),
            book.replayed(),
            book.in_kind(KIND_SEM),
            book.in_kind(KIND_MQ),
            book.linked()
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11131_posix_flow() {
        let mut book = build_posix();
        assert_eq!(book.case_count(), CRITS);
        assert_eq!(book.replayed(), CRITS);
        assert_eq!(book.linked(), 0);
        assert!(book.field_closed());
        let root = book.freeze();
        assert_eq!(book.fold(), root);
        assert_eq!(book.unlink(1), Err(PX_FROZEN));
    }

    #[test]
    fn unx11131_checks_green() {
        let s = run_unx11131_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
