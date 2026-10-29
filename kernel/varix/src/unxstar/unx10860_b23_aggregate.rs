//! UNX-F10860 pty/termios 错误矩阵段总收口（B23 断言聚合）（0x1c400 段）
//!
//! 锚点：UNX-F10860-J1 B23 聚合收口——段内六面基元判据（零头部分
//! 交付 / 满态 EAGAIN / 非终端 ENOTTY / slave 全关 EIO / 死管道
//! EPIPE / CAS 丢更新）各自独立复演必全真；聚合器账面（子判据计数
//! × 全真 × 封顶）必对平；任一子判据假或计数越界必聚合败。判定依
//! 赖场景字段演算，注入核（假真吞并/计数漂移坏实现）必分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：六面基元判定器（本地复演）→ B23 聚合器（add/verdict 走账）
//! → 样本矩阵（六面 × 正反格）→ 注入核（两坏实现必分歧）→ 总收口
//! 回放（六面全真聚合绿）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1c400 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c400;
pub const AG_ALL_TRUE: u32 = SEG_BASE + 0x01; // 六面全真
pub const AG_FALSE_MIX: u32 = SEG_BASE + 0x02; // 假真混入
pub const AG_COUNT_DRIFT: u32 = SEG_BASE + 0x03; // 计数漂移
pub const AG_CAP_OVER: u32 = SEG_BASE + 0x04; // 越封顶
pub const AG_TABLE_LOST: u32 = SEG_BASE + 0x05; // 基元表失平
pub const AG_LEDGER_LOST: u32 = SEG_BASE + 0x06; // 聚合账失平

/// 语义字面锚（六码沿用段字面）。
pub const EAGAIN: u32 = 11;
pub const EIO: u32 = 5;
pub const ENOTTY: u32 = 25;
pub const EPIPE: u32 = 32;

/// 诊断码语义文本。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        AG_ALL_TRUE => "六面全真",
        AG_FALSE_MIX => "假真混入",
        AG_COUNT_DRIFT => "计数漂移",
        AG_CAP_OVER => "越封顶",
        AG_TABLE_LOST => "基元表失平",
        AG_LEDGER_LOST => "聚合账失平",
        _ => "未知诊断码",
    }
}

/// 六面基元判定器（本地复演，语义与段内各卡一致）。

/// P1 零头部分交付：min(free,want)（want=0 恒 0）。
pub fn p1_partial(want: u32, free: u32) -> u32 {
    if want < free {
        want
    } else {
        free
    }
}

/// P2 满态 EAGAIN：free=0 且 nonblock → 真。
pub fn p2_full_eagain(nonblock: bool, free: u32) -> bool {
    nonblock && free == 0
}

/// P3 非终端 ENOTTY：终端命令落非终端 fd → ENOTTY 码。
pub fn p3_enotty(is_term: bool) -> u32 {
    if is_term {
        0
    } else {
        ENOTTY
    }
}

/// P4 slave 全关 EIO：Gone 态无残余读 → EIO 码。
pub fn p4_eio(gone: bool, avail: u32) -> u32 {
    if gone && avail == 0 {
        EIO
    } else {
        0
    }
}

/// P5 死管道 EPIPE：对端已关且写正量 → EPIPE 码（零写放行）。
pub fn p5_epipe(peer_open: bool, want: u32) -> u32 {
    if !peer_open && want > 0 {
        EPIPE
    } else {
        0
    }
}

/// P6 CAS 丢更新：版本失配 → 真。
pub fn p6_cas_lost(expect: u32, current: u32) -> bool {
    expect != current
}

/// 聚合子判据项：名 × 真值。
#[derive(Clone)]
pub struct AggItem {
    pub name: String,
    pub ok: bool,
}

/// B23 聚合器：add 走账 / verdict 对平（全真 × 计数在窗内）。
pub struct Aggregator {
    pub items: Vec<AggItem>,
    pub cap: usize,
    pub note: String,
}

impl Aggregator {
    pub fn new(cap: usize) -> Aggregator {
        Aggregator { items: Vec::new(), cap, note: "empty".to_string() }
    }

    /// 添加子判据：越封顶拒收（返回假且不落账）。
    pub fn add(&mut self, name: &str, ok: bool) -> bool {
        if self.items.len() >= self.cap {
            self.note = "over-cap".to_string();
            return false;
        }
        self.items.push(AggItem { name: name.to_string(), ok });
        self.note = "added".to_string();
        true
    }

    /// 聚合判定：非空、未越顶、全真。
    pub fn verdict(&self) -> bool {
        if self.items.is_empty() || self.items.len() > self.cap {
            return false;
        }
        let mut all = true;
        let mut i = 0usize;
        while i < self.items.len() {
            if !self.items[i].ok {
                all = false;
            }
            i += 1;
        }
        all
    }

    /// 账面统计：真项数 / 总项数。
    pub fn tally(&self) -> (usize, usize) {
        let mut t = 0usize;
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].ok {
                t += 1;
            }
            i += 1;
        }
        (t, self.items.len())
    }
}

/// 六面全真样本（B23 正收口）。
pub fn six_faces_ok() -> [bool; 6] {
    [
        p1_partial(100, 40) == 40,
        p2_full_eagain(true, 0) && !p2_full_eagain(false, 0),
        p3_enotty(false) == ENOTTY && p3_enotty(true) == 0,
        p4_eio(true, 0) == EIO && p4_eio(true, 9) == 0,
        p5_epipe(false, 4) == EPIPE && p5_epipe(false, 0) == 0,
        p6_cas_lost(0, 1) && !p6_cas_lost(1, 1),
    ]
}

/// 注入 A：坏实现假真吞并（假项误记真）。
pub fn faulty_absorb(ok: bool) -> bool {
    true
}

/// 注入 B：坏实现计数漂移（越顶仍收账）。
pub fn faulty_over_cap(a: &Aggregator) -> bool {
    let mut b = Aggregator::new(a.cap);
    b.items = a.items.clone();
    b.items.push(AggItem { name: "ghost".to_string(), ok: true });
    b.items.len() > a.cap
}

/// 注入核：两处坏账与聚合器必分歧。
pub fn inject_two() -> [bool; 2] {
    let d1 = faulty_absorb(false) != false;
    let mut a = Aggregator::new(2);
    let _ = a.add("x", true);
    let _ = a.add("y", true);
    let d2 = a.add("z", true) != faulty_over_cap(&a);
    [d1, d2]
}

/// 总收口回放：六面入账全真 → verdict 绿、账面 (6,6)。
pub fn b23_replay_ok() -> bool {
    let faces = six_faces_ok();
    let mut ag = Aggregator::new(8);
    let _ = ag.add("P1-零头", faces[0]);
    let _ = ag.add("P2-EAGAIN", faces[1]);
    let _ = ag.add("P3-ENOTTY", faces[2]);
    let _ = ag.add("P4-EIO", faces[3]);
    let _ = ag.add("P5-EPIPE", faces[4]);
    let _ = ag.add("P6-CAS", faces[5]);
    ag.verdict() && ag.tally() == (6, 6) && six_faces_ok() == [true, true, true, true, true, true]
}

/// 败面回放：任一假项 → verdict 败、账面 (5,6)。
pub fn b23_fail_replay_ok() -> bool {
    let faces = six_faces_ok();
    let mut ag = Aggregator::new(8);
    let _ = ag.add("P1-零头", faces[0]);
    let _ = ag.add("P2-EAGAIN", faces[1]);
    let _ = ag.add("P3-ENOTTY", faces[2]);
    let _ = ag.add("P4-EIO", faces[3]);
    let _ = ag.add("P5-EPIPE", false); // 注入一假
    let _ = ag.add("P6-CAS", faces[5]);
    !ag.verdict() && ag.tally() == (5, 6)
}

/// 聚合注记：账面渲染。
pub fn ag_note(ag: &Aggregator) -> String {
    let (t, n) = ag.tally();
    if ag.verdict() {
        format!("{} ({}/{})", diag_text(AG_ALL_TRUE), t, n)
    } else {
        format!("{} ({}/{})", diag_text(AG_FALSE_MIX), t, n)
    }
}

/// 域判据（对应卡验收 J 判据）。
pub fn run_unx10860_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10860");
    let faces = six_faces_ok();

    // J1 基元 P1 零头部分交付
    let j1 = p1_partial(100, 40) == 40 && p1_partial(10, 40) == 10 && p1_partial(0, 0) == 0;
    s.add("J1-P1 零头", j1, "min(free,want) 复演");

    // J2 基元 P2 满态 EAGAIN 正反
    let j2 = p2_full_eagain(true, 0) && !p2_full_eagain(false, 0) && !p2_full_eagain(true, 8);
    s.add("J2-P2 EAGAIN", j2, "满态/有空间两沿");

    // J3 基元 P3 ENOTTY 正反
    let j3 = p3_enotty(false) == ENOTTY && p3_enotty(true) == 0;
    s.add("J3-P3 ENOTTY", j3, "非终端/终端两沿");

    // J4 基元 P4 EIO 正反（残余前置）
    let j4 = p4_eio(true, 0) == EIO && p4_eio(true, 9) == 0 && p4_eio(false, 0) == 0;
    s.add("J4-P4 EIO", j4, "Gone 无残余才 EIO");

    // J5 基元 P5 EPIPE 正反（零写放行）
    let j5 = p5_epipe(false, 4) == EPIPE && p5_epipe(false, 0) == 0 && p5_epipe(true, 4) == 0;
    s.add("J5-P5 EPIPE", j5, "死管道正量才 EPIPE");

    // J6 基元 P6 CAS 丢更新
    let j6 = p6_cas_lost(0, 1) && !p6_cas_lost(1, 1);
    s.add("J6-P6 CAS", j6, "失配才丢更新");

    // J7 六面全真样本
    s.add("J7-六面全真", faces == [true, true, true, true, true, true], "段内基元全绿");

    // J8 总收口回放
    s.add("J8-B23 收口", b23_replay_ok(), "聚合 verdict 绿");

    // J9 败面回放
    s.add("J9-败面回放", b23_fail_replay_ok(), "一假必聚合败");

    // J10 注入1 假真吞并检出
    let inj = inject_two();
    s.add("J10-假真吞并检出", inj[0], "假项误真必分歧");

    // J11 注入2 计数漂移检出
    s.add("J11-计数漂移检出", inj[1], "越顶收账必分歧");

    // J12 封顶边界：cap=6 收满后拒收
    let mut ag6 = Aggregator::new(6);
    let mut i = 0usize;
    while i < 6 {
        let _ = ag6.add(&format!("p{}", i), true);
        i += 1;
    }
    let over = ag6.add("p6", true);
    s.add("J12-封顶两沿", ag6.verdict() && !over, "满窗全真/拒收第 7 项");

    // J13 字面锚：EAGAIN=11 / EIO=5 / ENOTTY=25 / EPIPE=32
    let j13 = EAGAIN == 11 && EIO == 5 && ENOTTY == 25 && EPIPE == 32;
    s.add("J13-字面锚", j13, "四码段字面沿用");

    // J14 诊断码闭集
    let j14 = diag_text(AG_ALL_TRUE) == "六面全真"
        && diag_text(AG_CAP_OVER) == "越封顶"
        && diag_text(9) == "未知诊断码";
    s.add("J14-码文本闭集", j14, "六码文本可辨");

    // J15 (dyn) 聚合注记落账
    let mut agd = Aggregator::new(8);
    let fs = six_faces_ok();
    let _ = agd.add("P1", fs[0]);
    let _ = agd.add("P2", fs[1]);
    let _ = agd.add("P3", fs[2]);
    let _ = agd.add("P4", fs[3]);
    let _ = agd.add("P5", fs[4]);
    let _ = agd.add("P6", fs[5]);
    let note = ag_note(&agd);
    s.add_dyn("J15-聚合注记", note.contains("6/6"), &note);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10860_flow() {
        assert_eq!(p1_partial(100, 40), 40);
        assert!(p2_full_eagain(true, 0));
        assert_eq!(p3_enotty(false), ENOTTY);
        assert_eq!(p4_eio(true, 0), EIO);
        assert_eq!(p5_epipe(false, 4), EPIPE);
        assert!(p6_cas_lost(0, 1));
        let mut ag = Aggregator::new(8);
        let _ = ag.add("a", true);
        let _ = ag.add("b", false);
        assert!(!ag.verdict());
        assert_eq!(ag.tally(), (1, 2));
    }

    #[test]
    fn unx10860_checks_green() {
        let s = run_unx10860_checks();
        assert!(s.all_passed(), "UNX-F10860 判据未全绿");
    }
}
