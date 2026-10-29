//! UNX-F11118 I 型常态化协议（联测纳入回归）（0x1f598 段）
//!
//! I 型面转常态后按三段协议运转：前置段立账、执行段立账、收口段立账。
//! 每段四个步骤逐步核销，三段全闭后方可把联测清单纳入回归。

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址：F11118 独占 0x1f598。
pub const SEG_BASE: u32 = 0x1f598;

/// 协议段号越界。
pub const PR_SEG: u32 = SEG_BASE + 0x01;
/// 协议段重复开启。
pub const PR_DUP: u32 = SEG_BASE + 0x02;
/// 步骤号越界。
pub const PR_STEP: u32 = SEG_BASE + 0x03;
/// 协议段未开启。
pub const PR_OPEN: u32 = SEG_BASE + 0x04;
/// 段内步骤未走完，不能收口。
pub const PR_INCOMPLETE: u32 = SEG_BASE + 0x05;
/// 协议账已冻结。
pub const PR_FROZEN: u32 = SEG_BASE + 0x06;
/// 清单未闭合。
pub const PR_THIN: u32 = SEG_BASE + 0x07;
/// 容量溢出。
pub const PR_OVER: u32 = SEG_BASE + 0x08;

/// 协议段数。
pub const SEGMENTS: usize = 3;
/// 每段步骤数。
pub const STEPS_PER_SEG: usize = 4;
/// 步骤总数。
pub const STEPS_TOTAL: usize = 12;
/// 回归清单容量。
pub const REG_CAP: usize = 6;
/// 折叠种子。
const PR_TAG: u64 = 0x0000_1f5_9898;
/// FNV-1a 素数。
const FP_STEP: u64 = 0x100000001b3;

/// 协议段。
#[derive(Clone, Debug)]
pub struct ProtoSegment {
    /// 段号（1 前置 / 2 执行 / 3 收口）。
    pub no: u8,
    /// 段名。
    pub name: String,
    /// 段内步骤数。
    pub steps: usize,
    /// 已核销步骤数。
    pub done: usize,
    /// 是否已收口。
    pub closed: bool,
}

impl ProtoSegment {
    /// 段渲染，形如 S1|4/4|C。
    pub fn render(&self) -> String {
        let flag = if self.closed { "C" } else { "O" };
        format!("S{}|{}/{}|{}", self.no, self.done, self.steps, flag)
    }

    /// 段内步骤是否走完。
    pub fn complete(&self) -> bool {
        self.steps > 0 && self.done == self.steps
    }
}

/// 回归清单条目。
#[derive(Clone, Debug)]
pub struct RegEntry {
    /// 条目号（51 起）。
    pub id: usize,
    /// 联测标签。
    pub tag: String,
    /// 是否已纳入。
    pub enrolled: bool,
}

/// 协议总账。
#[derive(Clone, Debug)]
pub struct ProtoBook {
    segs: Vec<ProtoSegment>,
    regs: Vec<RegEntry>,
    frozen: bool,
    root: u64,
}

impl ProtoBook {
    /// 建空账。
    pub fn new() -> Self {
        Self { segs: Vec::new(), regs: Vec::new(), frozen: false, root: 0 }
    }

    /// 定位段下标。
    fn seg_index(&self, no: u8) -> usize {
        let mut idx = usize::MAX;
        let mut k = 0usize;
        while k < self.segs.len() {
            if self.segs[k].no == no {
                idx = k;
            }
            k = k.wrapping_add(1);
        }
        idx
    }

    /// 开启一段：段域、重复、顺序、容量、冻结五道闸。
    pub fn open_segment(&mut self, no: u8, name: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(PR_FROZEN);
        }
        if no == 0 || no as usize > SEGMENTS {
            return Err(PR_SEG);
        }
        if self.segs.len() >= SEGMENTS.saturating_add(1) {
            return Err(PR_OVER);
        }
        if self.seg_index(no) != usize::MAX {
            return Err(PR_DUP);
        }
        if self.segs.len().wrapping_add(1) != no as usize {
            return Err(PR_SEG);
        }
        let label = String::from(name);
        self.segs.push(ProtoSegment {
            no,
            name: label,
            steps: STEPS_PER_SEG,
            done: 0,
            closed: false,
        });
        Ok(())
    }

    /// 核销一步。
    pub fn record(&mut self, no: u8, step: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(PR_FROZEN);
        }
        if step == 0 || step > STEPS_PER_SEG {
            return Err(PR_STEP);
        }
        let idx = self.seg_index(no);
        if idx == usize::MAX {
            return Err(PR_OPEN);
        }
        if let Some(seg) = self.segs.get_mut(idx) {
            if seg.closed || seg.done >= seg.steps {
                return Err(PR_DUP);
            }
            seg.done = seg.done.wrapping_add(1);
            return Ok(());
        }
        Err(PR_OPEN)
    }

    /// 收口一段：步骤必须全走完。
    pub fn close_segment(&mut self, no: u8) -> Result<(), u32> {
        if self.frozen {
            return Err(PR_FROZEN);
        }
        let idx = self.seg_index(no);
        if idx == usize::MAX {
            return Err(PR_OPEN);
        }
        if let Some(seg) = self.segs.get_mut(idx) {
            if !seg.complete() {
                return Err(PR_INCOMPLETE);
            }
            seg.closed = true;
            return Ok(());
        }
        Err(PR_OPEN)
    }

    /// 已收口段数。
    pub fn closed_segments(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.segs.len() {
            if self.segs[k].closed {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 已核销步骤总数。
    pub fn steps_done(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.segs.len() {
            n = n.wrapping_add(self.segs[k].done);
            k = k.wrapping_add(1);
        }
        n
    }

    /// 三段是否立账完毕。
    pub fn accounted(&self) -> bool {
        self.segs.len() == SEGMENTS && self.closed_segments() == SEGMENTS
    }

    /// 清单是否闭合：三段收口且步骤全核销。
    pub fn checklist_closed(&self) -> bool {
        self.accounted() && self.steps_done() == STEPS_TOTAL
    }

    /// 纳入回归清单。
    pub fn enroll_reg(&mut self, id: usize, tag: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(PR_FROZEN);
        }
        if id < 51 {
            return Err(PR_STEP);
        }
        if self.regs.len() >= REG_CAP {
            return Err(PR_OVER);
        }
        let mut k = 0usize;
        while k < self.regs.len() {
            if self.regs[k].id == id {
                return Err(PR_DUP);
            }
            k = k.wrapping_add(1);
        }
        let label = String::from(tag);
        self.regs.push(RegEntry { id, tag: label, enrolled: false });
        Ok(())
    }

    /// 确认纳入。
    pub fn confirm_reg(&mut self, id: usize) -> Result<(), u32> {
        let mut idx = usize::MAX;
        let mut k = 0usize;
        while k < self.regs.len() {
            if self.regs[k].id == id {
                idx = k;
            }
            k = k.wrapping_add(1);
        }
        if idx == usize::MAX {
            return Err(PR_OPEN);
        }
        if let Some(reg) = self.regs.get_mut(idx) {
            reg.enrolled = true;
            return Ok(());
        }
        Err(PR_OPEN)
    }

    /// 回归条目数。
    pub fn reg_count(&self) -> usize {
        self.regs.len()
    }

    /// 已纳入条目数。
    pub fn reg_enrolled(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.regs.len() {
            if self.regs[k].enrolled {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
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
        let mut h = PR_TAG;
        let mut k = 0usize;
        while k < self.segs.len() {
            let g = &self.segs[k];
            h = (h ^ g.no as u64).wrapping_mul(FP_STEP);
            h = (h ^ g.done as u64).wrapping_mul(FP_STEP);
            let b = if g.closed { 1u64 } else { 2u64 };
            h = (h ^ b).wrapping_mul(FP_STEP);
            k = k.wrapping_add(1);
        }
        let mut j = 0usize;
        while j < self.regs.len() {
            let r = &self.regs[j];
            h = (h ^ r.id as u64).wrapping_mul(FP_STEP);
            let b = if r.enrolled { 3u64 } else { 4u64 };
            h = (h ^ b).wrapping_mul(FP_STEP);
            j = j.wrapping_add(1);
        }
        h
    }
}

/// 标准协议账：三段各四步核销并收口，三条联测纳入回归。
pub fn build_proto() -> ProtoBook {
    let mut book = ProtoBook::new();
    let _ = book.open_segment(1, "PRE");
    let _ = book.open_segment(2, "RUN");
    let _ = book.open_segment(3, "CLOSE");
    let mut seg = 1u8;
    while seg <= 3 {
        let mut step = 1usize;
        while step <= STEPS_PER_SEG {
            let _ = book.record(seg, step);
            step = step.wrapping_add(1);
        }
        let _ = book.close_segment(seg);
        seg = seg.wrapping_add(1);
    }
    let _ = book.enroll_reg(51, "LTP-I");
    let _ = book.enroll_reg(52, "MTX-I");
    let _ = book.enroll_reg(53, "DRL-I");
    let _ = book.confirm_reg(51);
    let _ = book.confirm_reg(52);
    let _ = book.confirm_reg(53);
    book
}

/// 四路对抗：段号越界、重段、步骤未走完收口、步骤越界。
pub fn inject_four() -> [bool; 4] {
    let mut a_book = ProtoBook::new();
    let a = a_book.open_segment(4, "BAD") == Err(PR_SEG) && a_book.closed_segments() == 0;
    let mut b_book = ProtoBook::new();
    let _ = b_book.open_segment(1, "PRE");
    let b = b_book.open_segment(1, "PRE") == Err(PR_DUP);
    let mut c_book = ProtoBook::new();
    let _ = c_book.open_segment(1, "PRE");
    let _ = c_book.record(1, 1);
    let c = c_book.close_segment(1) == Err(PR_INCOMPLETE) && c_book.closed_segments() == 0;
    let mut d_book = ProtoBook::new();
    let _ = d_book.open_segment(1, "PRE");
    let d = d_book.record(1, 5) == Err(PR_STEP) && d_book.record(2, 1) == Err(PR_OPEN);
    [a, b, c, d]
}

/// F11118 全部检查：三段逐项立账与清单闭合。
pub fn run_unx11118_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11118 I 型常态化协议");
    let book = build_proto();
    let four = inject_four();

    // J1 三段立账。
    s.add(
        "J1-三段立账",
        book.closed_segments() == SEGMENTS && book.accounted(),
        "前置段、执行段、收口段三段全部开启并收口",
    );
    // J2 十二步全核销。
    s.add(
        "J2-步骤核销",
        book.steps_done() == STEPS_TOTAL && book.steps_done() == SEGMENTS * STEPS_PER_SEG,
        "三段各四步，共 12 步全部核销",
    );
    // J3 清单闭合。
    s.add(
        "J3-清单闭合",
        book.checklist_closed(),
        "三段收口且步骤全核销，协议清单闭合",
    );
    // J4 联测纳入回归。
    s.add(
        "J4-回归纳入",
        book.reg_count() == 3 && book.reg_enrolled() == 3,
        "LTP-I、MTX-I、DRL-I 三条联测全部纳入回归清单",
    );
    // J5 段域闸。
    let mut seg_probe = ProtoBook::new();
    s.add(
        "J5-段域闸",
        seg_probe.open_segment(0, "ZERO") == Err(PR_SEG)
            && seg_probe.open_segment(4, "FOUR") == Err(PR_SEG),
        "段号 0 与 4 越出 1..=3，开启报 PR_SEG",
    );
    // J6 重段闸。
    let mut dup_probe = ProtoBook::new();
    let _ = dup_probe.open_segment(1, "PRE");
    s.add(
        "J6-重段闸",
        dup_probe.open_segment(1, "PRE") == Err(PR_DUP),
        "前置段二次开启报 PR_DUP",
    );
    // J7 顺序闸。
    let mut order_probe = ProtoBook::new();
    s.add(
        "J7-顺序闸",
        order_probe.open_segment(2, "RUN") == Err(PR_SEG) && order_probe.closed_segments() == 0,
        "未开前置段直接开启执行段报 PR_SEG",
    );
    // J8 收口闸。
    let mut half = ProtoBook::new();
    let _ = half.open_segment(1, "PRE");
    let _ = half.record(1, 1);
    s.add(
        "J8-收口闸",
        half.close_segment(1) == Err(PR_INCOMPLETE) && half.closed_segments() == 0,
        "仅核销 1 步时收口报 PR_INCOMPLETE",
    );
    // J9 步骤域闸。
    let mut step_probe = ProtoBook::new();
    let _ = step_probe.open_segment(1, "PRE");
    s.add(
        "J9-步骤域闸",
        step_probe.record(1, 0) == Err(PR_STEP)
            && step_probe.record(1, 5) == Err(PR_STEP)
            && step_probe.record(2, 1) == Err(PR_OPEN),
        "步骤号 0 与 5 越界报 PR_STEP，未开段核销报 PR_OPEN",
    );
    // J10 收口后拒再核销。
    let mut done_book = ProtoBook::new();
    let _ = done_book.open_segment(1, "PRE");
    let mut k = 1usize;
    while k <= STEPS_PER_SEG {
        let _ = done_book.record(1, k);
        k = k.wrapping_add(1);
    }
    let closed_ok = done_book.close_segment(1).is_ok();
    s.add(
        "J10-收口拒核销",
        closed_ok && done_book.record(1, 1) == Err(PR_DUP),
        "段收口后再次核销步骤报 PR_DUP",
    );
    // J11 冻结拒写。
    let mut sealed = build_proto();
    let root = sealed.seal();
    s.add(
        "J11-冻结拒写",
        sealed.is_frozen()
            && sealed.open_segment(1, "PRE") == Err(PR_FROZEN)
            && sealed.record(1, 1) == Err(PR_FROZEN)
            && sealed.close_segment(2) == Err(PR_FROZEN)
            && sealed.enroll_reg(54, "REG-I") == Err(PR_FROZEN),
        "冻结后开启、核销、收口、纳入回归均报 PR_FROZEN",
    );
    // J12 冻结根。
    s.add(
        "J12-冻结根",
        sealed.fold() == root && root != PR_TAG,
        "冻结根等于冻结时刻的折叠值且不等于种子",
    );
    // J13 四路对抗。
    s.add(
        "J13-四路对抗",
        four[0] && four[1] && four[2] && four[3],
        "段号越界、重段、未走完收口、步骤越界四路全部被拒",
    );
    // J14 段渲染。
    let seg = ProtoSegment {
        no: 2,
        name: String::from("RUN"),
        steps: STEPS_PER_SEG,
        done: STEPS_PER_SEG,
        closed: true,
    };
    s.add(
        "J14-段渲染",
        seg.render() == "S2|4/4|C" && seg.complete(),
        "执行段渲染为 S2|4/4|C",
    );
    // J15 动态明细。
    s.add_dyn(
        "J15-协议明细",
        book.checklist_closed(),
        &format!(
            "段={} 收口={} 步骤={}/{} 回归={}/{}",
            SEGMENTS,
            book.closed_segments(),
            book.steps_done(),
            STEPS_TOTAL,
            book.reg_enrolled(),
            book.reg_count()
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11118_proto_flow() {
        let mut book = build_proto();
        assert_eq!(book.closed_segments(), SEGMENTS);
        assert_eq!(book.steps_done(), STEPS_TOTAL);
        assert!(book.checklist_closed());
        assert_eq!(book.reg_enrolled(), 3);
        let root = book.seal();
        assert_eq!(book.fold(), root);
        assert_eq!(book.record(1, 1), Err(PR_FROZEN));
    }

    #[test]
    fn unx11118_checks_green() {
        let s = run_unx11118_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
