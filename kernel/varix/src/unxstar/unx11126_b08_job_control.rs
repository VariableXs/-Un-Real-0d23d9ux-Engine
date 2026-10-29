//! UNX-F11126 B08 清账（作业控制判据终重放）（0x1f5a6 段）
//!
//! 作业控制批 B08 共 20 条判据。每个作业归属一个会话，同一会话同时至多
//! 一个前台作业；前台作业不得处于停止态。终重放逐条核对这两条铁律。

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址：F11126 独占 0x1f5a6。
pub const SEG_BASE: u32 = 0x1f5a6;

/// 作业号越出 1..=20。
pub const JC_NO: u32 = SEG_BASE + 0x01;
/// 会话号越出 1..=2。
pub const JC_SID: u32 = SEG_BASE + 0x02;
/// 作业重复登记。
pub const JC_DUP: u32 = SEG_BASE + 0x03;
/// 作业号不连续。
pub const JC_ORDER: u32 = SEG_BASE + 0x04;
/// 同会话出现第二个前台作业。
pub const JC_FG: u32 = SEG_BASE + 0x05;
/// 前台作业被停止。
pub const JC_STOP: u32 = SEG_BASE + 0x06;
/// 作业重复重放。
pub const JC_REPLAY: u32 = SEG_BASE + 0x07;
/// 清账账已冻结。
pub const JC_FROZEN: u32 = SEG_BASE + 0x08;

/// 判据总数。
pub const CRITS: usize = 20;
/// 会话数。
pub const SESSIONS: usize = 2;
/// 每会话作业数。
pub const JOBS_PER_SESSION: usize = 10;
/// 折叠种子。
const JC_TAG: u64 = 0x0000_1f5_a6a6;
/// FNV-1a 素数。
const FP_STEP: u64 = 0x100000001b3;

/// 作业判据。
#[derive(Clone, Debug)]
pub struct JobCase {
    /// 作业号（1..=20）。
    pub no: usize,
    /// 进程组号（等于作业号）。
    pub pgid: usize,
    /// 会话号（1 或 2）。
    pub sid: usize,
    /// 是否前台作业。
    pub fg: bool,
    /// 是否已停止。
    pub stopped: bool,
    /// 是否已终重放。
    pub replayed: bool,
}

impl JobCase {
    /// 作业状态是否自洽：前台不得停止。
    pub fn consistent(&self) -> bool {
        !(self.fg && self.stopped)
    }

    /// 判据是否闭合。
    pub fn settled(&self) -> bool {
        self.consistent() && self.replayed
    }

    /// 状态名。
    pub fn state_name(&self) -> String {
        if self.stopped {
            String::from("STOP")
        } else if self.fg {
            String::from("FG")
        } else {
            String::from("BG")
        }
    }

    /// 判据渲染，形如 J08|S1|FG|R。
    pub fn render(&self) -> String {
        let r = if self.replayed { "R" } else { "N" };
        format!("J{:02}|S{}|{}|{}", self.no, self.sid, self.state_name(), r)
    }
}

/// 作业控制清账账。
#[derive(Clone, Debug)]
pub struct JobClear {
    cases: Vec<JobCase>,
    frozen: bool,
    root: u64,
}

impl JobClear {
    /// 建空账。
    pub fn new() -> Self {
        Self { cases: Vec::new(), frozen: false, root: 0 }
    }

    /// 定位作业下标。
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

    /// 登记作业：号、会话、重复、顺序、冻结五道闸。
    pub fn file(&mut self, no: usize, sid: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(JC_FROZEN);
        }
        if no == 0 || no > CRITS {
            return Err(JC_NO);
        }
        if sid == 0 || sid > SESSIONS {
            return Err(JC_SID);
        }
        if self.index_of(no) != usize::MAX {
            return Err(JC_DUP);
        }
        if self.cases.len().wrapping_add(1) != no {
            return Err(JC_ORDER);
        }
        self.cases.push(JobCase {
            no,
            pgid: no,
            sid,
            fg: false,
            stopped: false,
            replayed: false,
        });
        Ok(())
    }

    /// 置为前台：同会话已有前台作业则拒绝。
    pub fn foreground(&mut self, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(JC_FROZEN);
        }
        let idx = self.index_of(no);
        if idx == usize::MAX {
            return Err(JC_NO);
        }
        let mut sid = 0usize;
        if let Some(c) = self.cases.get(idx) {
            sid = c.sid;
        }
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].sid == sid && self.cases[k].fg && self.cases[k].no != no {
                return Err(JC_FG);
            }
            k = k.wrapping_add(1);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            if c.stopped {
                return Err(JC_STOP);
            }
            c.fg = true;
            return Ok(());
        }
        Err(JC_NO)
    }

    /// 置为停止：前台作业必须先后台化。
    pub fn stop(&mut self, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(JC_FROZEN);
        }
        let idx = self.index_of(no);
        if idx == usize::MAX {
            return Err(JC_NO);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            if c.fg {
                return Err(JC_STOP);
            }
            c.stopped = true;
            return Ok(());
        }
        Err(JC_NO)
    }

    /// 后台化。
    pub fn background(&mut self, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(JC_FROZEN);
        }
        let idx = self.index_of(no);
        if idx == usize::MAX {
            return Err(JC_NO);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            c.fg = false;
            return Ok(());
        }
        Err(JC_NO)
    }

    /// 终重放一条作业。
    pub fn replay(&mut self, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(JC_FROZEN);
        }
        let idx = self.index_of(no);
        if idx == usize::MAX {
            return Err(JC_NO);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            if !c.consistent() {
                return Err(JC_STOP);
            }
            if c.replayed {
                return Err(JC_REPLAY);
            }
            c.replayed = true;
            return Ok(());
        }
        Err(JC_NO)
    }

    /// 作业数。
    pub fn case_count(&self) -> usize {
        self.cases.len()
    }

    /// 已重放数。
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

    /// 某会话前台作业数。
    pub fn fg_count(&self, sid: usize) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].sid == sid && self.cases[k].fg {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 停止作业数。
    pub fn stopped_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].stopped {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 是否全部闭合：两会话各一个前台，其余停止，逐条重放。
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
        self.fg_count(1) == 1 && self.fg_count(2) == 1 && self.stopped_count() == CRITS - 2
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
        let mut h = JC_TAG;
        let mut k = 0usize;
        while k < self.cases.len() {
            let c = &self.cases[k];
            h = (h ^ c.no as u64).wrapping_mul(FP_STEP);
            h = (h ^ c.sid as u64).wrapping_mul(FP_STEP);
            let f = if c.fg { 1u64 } else { 2u64 };
            h = (h ^ f).wrapping_mul(FP_STEP);
            let s = if c.stopped { 3u64 } else { 4u64 };
            h = (h ^ s).wrapping_mul(FP_STEP);
            k = k.wrapping_add(1);
        }
        h
    }
}

/// 标准作业账：两会话各一个前台作业，其余作业停止。
pub fn build_job() -> JobClear {
    let mut book = JobClear::new();
    let mut no = 1usize;
    while no <= CRITS {
        let sid = (no.saturating_sub(1)) / JOBS_PER_SESSION + 1;
        let _ = book.file(no, sid);
        no = no.wrapping_add(1);
    }
    let _ = book.foreground(1);
    let _ = book.foreground(11);
    let mut k = 1usize;
    while k <= CRITS {
        if k != 1 && k != 11 {
            let _ = book.stop(k);
        }
        let _ = book.replay(k);
        k = k.wrapping_add(1);
    }
    book
}

/// 四路对抗：作业号越界、会话号越界、重复登记、双前台。
pub fn inject_four() -> [bool; 4] {
    let mut a_book = JobClear::new();
    let a = a_book.file(0, 1) == Err(JC_NO) && a_book.file(21, 1) == Err(JC_NO);
    let mut b_book = JobClear::new();
    let b = b_book.file(1, 3) == Err(JC_SID) && b_book.case_count() == 0;
    let mut c_book = JobClear::new();
    let _ = c_book.file(1, 1);
    let c = c_book.file(1, 1) == Err(JC_DUP) && c_book.case_count() == 1;
    let mut d_book = JobClear::new();
    let _ = d_book.file(1, 1);
    let _ = d_book.file(2, 1);
    let _ = d_book.foreground(1);
    let d = d_book.foreground(2) == Err(JC_FG) && d_book.fg_count(1) == 1;
    [a, b, c, d]
}

/// F11126 全部检查：作业族判据终重放与清账闭合。
pub fn run_unx11126_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11126 B08 清账");
    let book = build_job();
    let four = inject_four();

    // J1 二十作业在册。
    s.add(
        "J1-作业在册",
        book.case_count() == CRITS && CRITS == SESSIONS * JOBS_PER_SESSION,
        "两个会话各 10 个作业，共 20 条判据在册",
    );
    // J2 终重放全量。
    s.add("J2-终重放", book.replayed() == CRITS, "二十条作业判据全部完成终重放");
    // J3 前台唯一。
    s.add(
        "J3-前台唯一",
        book.fg_count(1) == 1 && book.fg_count(2) == 1,
        "每个会话恰好一个前台作业，两个会话各一",
    );
    // J4 停止计数。
    s.add(
        "J4-停止计数",
        book.stopped_count() == CRITS - 2,
        "除两个前台作业外其余 18 个作业全部处于停止态",
    );
    // J5 闭合断言。
    s.add("J5-闭合", book.closed(), "作业判据逐条闭合，B08 清账账闭合");
    // J6 作业号闸。
    let mut no_probe = JobClear::new();
    s.add(
        "J6-作业号闸",
        no_probe.file(0, 1) == Err(JC_NO) && no_probe.file(21, 1) == Err(JC_NO),
        "作业号 0 与 21 越出 1..=20，登记报 JC_NO",
    );
    // J7 会话号闸。
    let mut sid_probe = JobClear::new();
    s.add(
        "J7-会话号闸",
        sid_probe.file(1, 0) == Err(JC_SID) && sid_probe.file(1, 3) == Err(JC_SID),
        "会话号 0 与 3 越出 1..=2，登记报 JC_SID",
    );
    // J8 双前台闸。
    let mut fg_probe = JobClear::new();
    let _ = fg_probe.file(1, 1);
    let _ = fg_probe.file(2, 1);
    let _ = fg_probe.foreground(1);
    s.add(
        "J8-双前台闸",
        fg_probe.foreground(2) == Err(JC_FG) && fg_probe.fg_count(1) == 1,
        "同会话第二个前台作业报 JC_FG",
    );
    // J9 前台停止闸。
    let mut stop_probe = JobClear::new();
    let _ = stop_probe.file(1, 1);
    let _ = stop_probe.foreground(1);
    s.add(
        "J9-前台停止闸",
        stop_probe.stop(1) == Err(JC_STOP) && stop_probe.stopped_count() == 0,
        "前台作业直接停止报 JC_STOP",
    );
    // J10 重号闸。
    let mut dup_probe = JobClear::new();
    let _ = dup_probe.file(1, 1);
    s.add(
        "J10-重号闸",
        dup_probe.file(1, 1) == Err(JC_DUP) && dup_probe.case_count() == 1,
        "同一作业号二次登记报 JC_DUP",
    );
    // J11 冻结拒写。
    let mut sealed = build_job();
    let root = sealed.freeze();
    s.add(
        "J11-冻结拒写",
        sealed.is_frozen()
            && sealed.file(1, 1) == Err(JC_FROZEN)
            && sealed.foreground(1) == Err(JC_FROZEN)
            && sealed.stop(1) == Err(JC_FROZEN)
            && sealed.fold() == root,
        "冻结后登记、前台化、停止均报 JC_FROZEN",
    );
    // J12 四路对抗。
    s.add(
        "J12-四路对抗",
        four[0] && four[1] && four[2] && four[3],
        "作业号、会话号、重号、双前台四路全部被拒",
    );
    // J13 判据渲染。
    let case = JobCase {
        no: 8,
        pgid: 8,
        sid: 1,
        fg: true,
        stopped: false,
        replayed: true,
    };
    s.add(
        "J13-判据渲染",
        case.render() == "J08|S1|FG|R" && case.consistent(),
        "第 8 个作业渲染为 J08|S1|FG|R",
    );
    // J14 动态明细。
    s.add_dyn(
        "J14-清账明细",
        book.closed(),
        &format!(
            "条={} 重放={} 前台1={} 停止={}",
            book.case_count(),
            book.replayed(),
            book.fg_count(1),
            book.stopped_count()
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11126_job_flow() {
        let mut book = build_job();
        assert_eq!(book.case_count(), CRITS);
        assert_eq!(book.fg_count(1), 1);
        assert!(book.closed());
        let root = book.freeze();
        assert_eq!(book.fold(), root);
        assert_eq!(book.stop(1), Err(JC_FROZEN));
    }

    #[test]
    fn unx11126_checks_green() {
        let s = run_unx11126_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
