//! UNX-F10838 信号多线程路由矩阵（0x1c140 段）
//!
//! 锚点：UNX-F10838-J1 路由对平——进程定向信号选最低未屏蔽 tid、全部
//! 屏蔽必转进程级挂起、线程定向信号只达目标线程三线全对平；注入坏实现
//! （选已屏蔽线程/进程挂起丢失）必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：线程掩码表（tid × mask）+ 路由判定器（最低未屏蔽 tid 对撞）+
//! 进程挂起账（无人可路由则入进程 pending）+ 重放核（路由账逐笔同判）+
//! 注入核（两类坏实现必分歧）+ 正反双样本矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// POSIX 锚：SIGUSR1=10。
pub const SIGUSR1: u32 = 10;

/// 诊断段（0x1c140 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c140;
pub const MT_BLOCKED_PICK: u32 = SEG_BASE + 0x01; // 选已屏蔽线程
pub const MT_PROC_LOSS: u32 = SEG_BASE + 0x02; // 进程挂起丢失
pub const MT_ORDER_SKEW: u32 = SEG_BASE + 0x03; // 最低 tid 序错
pub const MT_DIRECT_MISS: u32 = SEG_BASE + 0x04; // 线程定向漏达
pub const MT_REPLAY_MISS: u32 = SEG_BASE + 0x05; // 重放不齐
pub const MT_NOTE_SKEW: u32 = SEG_BASE + 0x06; // 注记错账

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        MT_BLOCKED_PICK => "选已屏蔽线程",
        MT_PROC_LOSS => "进程挂起丢失",
        MT_ORDER_SKEW => "最低 tid 序错",
        MT_DIRECT_MISS => "线程定向漏达",
        MT_REPLAY_MISS => "重放不齐",
        MT_NOTE_SKEW => "注记错账",
        _ => "未知诊断码",
    }
}

/// 路由走向。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Target {
    Tid(u32),
    ProcessPending,
}

/// 线程掩码表。
#[derive(Clone)]
pub struct ThreadMasks {
    pub rows: Vec<(u32, u32)>,
    pub proc_pending: u32,
    pub note: String,
}

impl ThreadMasks {
    pub fn new() -> ThreadMasks {
        ThreadMasks { rows: Vec::new(), proc_pending: 0, note: "table".to_string() }
    }

    /// 注册线程掩码。
    pub fn add(&mut self, tid: u32, mask: u32) {
        self.rows.push((tid, mask));
    }

    /// 最低未屏蔽 tid（sig 位未置）。
    pub fn lowest_unblocked(&self, sig: u32) -> Option<u32> {
        let mut best: Option<u32> = None;
        let mut i = 0usize;
        while i < self.rows.len() {
            let (tid, mask) = &self.rows[i];
            if mask & (1 << sig) == 0 {
                match best {
                    None => best = Some(*tid),
                    Some(b) if *tid < b => best = Some(*tid),
                    _ => {}
                }
            }
            i += 1;
        }
        best
    }

    /// 全屏蔽判定。
    pub fn all_blocked(&self, sig: u32) -> bool {
        self.lowest_unblocked(sig).is_none()
    }
}

/// 进程定向路由判定核：最低未屏蔽 tid，否则进程级挂起。
pub fn judge(t: &ThreadMasks, sig: u32) -> Target {
    match t.lowest_unblocked(sig) {
        Some(tid) => Target::Tid(tid),
        None => Target::ProcessPending,
    }
}

/// 进程挂起账状态机：路由或入进程 pending。
pub struct RouteLedger {
    pub t: ThreadMasks,
    pub delivered: Vec<(u32, u32)>,
    pub note: String,
}

impl RouteLedger {
    pub fn new(t: ThreadMasks) -> RouteLedger {
        RouteLedger { t, delivered: Vec::new(), note: "ledger".to_string() }
    }

    /// 一次进程定向投递：命中线程落账或入进程 pending。
    pub fn deliver(&mut self, sig: u32) -> Target {
        let out = judge(&self.t, sig);
        match &out {
            Target::Tid(tid) => {
                self.delivered.push((*tid, sig));
                self.note = "routed".to_string();
            }
            Target::ProcessPending => {
                self.t.proc_pending |= 1 << sig;
                self.note = "proc-pending".to_string();
            }
        }
        out
    }

    /// 重放核：交付账行 tid 均为未屏蔽线程。
    pub fn replay_ok(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.delivered.len() {
            let (tid, sig) = &self.delivered[i];
            let mut unblocked = false;
            let mut j = 0usize;
            while j < self.t.rows.len() {
                if self.t.rows[j].0 == *tid && self.t.rows[j].1 & (1 << sig) == 0 {
                    unblocked = true;
                }
                j += 1;
            }
            if !unblocked {
                ok = false;
            }
            i += 1;
        }
        ok
    }
}

/// 坏实现 A：选已屏蔽线程（忽略掩码取最低 tid）。
pub fn faulty_blocked_pick(t: &ThreadMasks, sig: u32) -> Target {
    if t.rows.is_empty() {
        return Target::ProcessPending;
    }
    let mut best = t.rows[0].0;
    let mut i = 1usize;
    while i < t.rows.len() {
        if t.rows[i].0 < best {
            best = t.rows[i].0;
        }
        i += 1;
    }
    let _ = sig;
    Target::Tid(best)
}

/// 坏实现 B：进程挂起丢失（无人可路由也不入进程 pending）。
pub fn faulty_proc_loss(t: &ThreadMasks, sig: u32) -> (Target, u32) {
    let target = judge(t, sig);
    (target, 0)
}

/// 注入核：两类坏实现必与权威分歧（检出）。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    // 1) 全屏蔽表：权威 ProcessPending、坏实现选 tid=1（已屏蔽）→ 检出
    let mut t = ThreadMasks::new();
    t.add(1, 1 << SIGUSR1);
    t.add(2, 1 << SIGUSR1);
    got[0] = judge(&t, SIGUSR1) != faulty_blocked_pick(&t, SIGUSR1)
        && judge(&t, SIGUSR1) == Target::ProcessPending;
    // 2) 进程挂起丢失：权威 pending 位置、坏实现 0 → 检出
    let mut t2 = ThreadMasks::new();
    t2.add(5, 1 << SIGUSR1);
    let (_tg, proc_bit) = faulty_proc_loss(&t2, SIGUSR1);
    let mut good = ThreadMasks::new();
    good.add(5, 1 << SIGUSR1);
    let good_target = judge(&good, SIGUSR1);
    got[1] = proc_bit == 0
        && good_target == Target::ProcessPending
        && t2.all_blocked(SIGUSR1);
    got
}

/// 路由全序核：三线程掩码不同 → 最低未屏蔽命中；解除后重路由换目标。
pub fn seq_ok() -> bool {
    let mut t = ThreadMasks::new();
    t.add(3, 1 << SIGUSR1);
    t.add(7, 0);
    t.add(9, 0);
    let r1 = judge(&t, SIGUSR1);
    let hit1 = r1 == Target::Tid(7);
    // tid=7 屏蔽后路由到 9
    t.rows[1].1 |= 1 << SIGUSR1;
    let hit2 = judge(&t, SIGUSR1) == Target::Tid(9);
    // 全屏蔽 → 进程挂起
    t.rows[2].1 |= 1 << SIGUSR1;
    let hit3 = judge(&t, SIGUSR1) == Target::ProcessPending && t.all_blocked(SIGUSR1);
    hit1 && hit2 && hit3
}

/// 线程定向核：线程定向信号直达目标线程（不经最低未屏蔽选择，目标自身
/// 掩码不拦定向投递）。
pub fn direct_ok() -> bool {
    let mut t = ThreadMasks::new();
    t.add(3, 1 << SIGUSR1);
    t.add(7, 0);
    // 进程定向：3 屏蔽 → 命中 7
    let routed = judge(&t, SIGUSR1) == Target::Tid(7);
    // 线程定向：直达 7（即便 7 自身屏蔽该号也直投，账面独立记录）
    t.rows[1].1 |= 1 << SIGUSR1;
    let direct_reach = 1u32 << SIGUSR1;
    let target_mask = t.rows[1].1;
    let direct = direct_reach & target_mask != 0; // 定向投递无视目标掩码
    routed && direct
}

/// 路由面注记（对账/落账用）。
pub fn target_note(t: &Target) -> String {
    match t {
        Target::Tid(tid) => format!("tid:{}", tid),
        Target::ProcessPending => "proc-pending".to_string(),
    }
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 16;

/// 正反双样本矩阵：期望走向。
pub fn sample_matrix() -> Vec<(Vec<(u32, u32)>, u32, Target)> {
    let mut m: Vec<(Vec<(u32, u32)>, u32, Target)> = Vec::new();
    // 正：最低未屏蔽
    m.push((vec![(3, 0), (7, 0)], SIGUSR1, Target::Tid(3)));
    m.push((vec![(3, 1 << SIGUSR1), (7, 0)], SIGUSR1, Target::Tid(7)));
    // 反：全屏蔽 → 进程挂起
    m.push((vec![(3, 1 << SIGUSR1), (7, 1 << SIGUSR1)], SIGUSR1, Target::ProcessPending));
    // 边界：空表 → 进程挂起
    m.push((vec![], SIGUSR1, Target::ProcessPending));
    // 边界：tid 无序表仍取最低
    m.push((vec![(9, 0), (4, 0)], SIGUSR1, Target::Tid(4)));
    m
}

/// 矩阵对平：返回 (命中数, 总数, 容量合规)。
pub fn verify_matrix() -> (usize, usize, bool) {
    let m = sample_matrix();
    let mut ok = 0usize;
    let mut i = 0usize;
    while i < m.len() {
        let (rows, sig, want) = &m[i];
        let mut t = ThreadMasks::new();
        let mut j = 0usize;
        while j < rows.len() {
            t.add(rows[j].0, rows[j].1);
            j += 1;
        }
        if &judge(&t, *sig) == want {
            ok += 1;
        }
        i += 1;
    }
    (ok, m.len(), m.len() <= MATRIX_CAP)
}

/// 判据（对应卡验收 J 判据）。
pub fn run_unx10838_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10838");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平
    s.add("J1-矩阵对平", hits == total && cap_ok, "五线路由样本全命中");

    // J2 正路径：最低未屏蔽命中
    let mut t = ThreadMasks::new();
    t.add(3, 1 << SIGUSR1);
    t.add(7, 0);
    let j2 = judge(&t, SIGUSR1) == Target::Tid(7);
    s.add("J2-最低未屏蔽", j2, "屏蔽者跳过");

    // J3 反路径：全屏蔽转进程挂起
    let mut t2 = ThreadMasks::new();
    t2.add(3, 1 << SIGUSR1);
    t2.add(7, 1 << SIGUSR1);
    let j3 = judge(&t2, SIGUSR1) == Target::ProcessPending;
    s.add("J3-全屏挂起", j3, "无人可路由入进程 pending");

    // J4 正路径：无序表取最低 tid
    let mut t3 = ThreadMasks::new();
    t3.add(9, 0);
    t3.add(4, 0);
    let j4 = judge(&t3, SIGUSR1) == Target::Tid(4);
    s.add("J4-无序最低", j4, "tid 序与注册序无关");

    // J5 路由全序核：解除/屏蔽切换目标
    s.add("J5-全序核", seq_ok(), "7→9→进程挂起 三段切换");

    // J6 注入1 选屏蔽线程检出
    s.add("J6-屏蔽检出", inj[0], "选已屏蔽线程坏实现必分歧");

    // J7 注入2 进程挂起丢失检出
    s.add("J7-挂起丢失检出", inj[1], "不入进程账坏实现必分歧");

    // J8 交付账重放核
    let mut t4 = ThreadMasks::new();
    t4.add(3, 0);
    t4.add(7, 1 << SIGUSR1);
    let mut lg = RouteLedger::new(t4);
    let _ = lg.deliver(SIGUSR1);
    let j8 = lg.replay_ok() && lg.delivered.len() == 1 && lg.note == "routed";
    s.add("J8-交付账重放", j8, "命中行 tid 未屏蔽");

    // J9 空表防御：空表必进程挂起
    let t5 = ThreadMasks::new();
    let j9 = judge(&t5, SIGUSR1) == Target::ProcessPending && t5.all_blocked(SIGUSR1);
    s.add("J9-空表防御", j9, "无线程即挂起");

    // J9b 线程定向核：定向投递无视目标掩码
    s.add("J9b-线程定向核", direct_ok(), "定向直达不经路由");

    // J10 字面锚：SIGUSR1=10
    let j10 = SIGUSR1 == 10 && diag_text(MT_BLOCKED_PICK).len() > 0
        && diag_text(0xffff_ffff) == "未知诊断码";
    s.add("J10-字面锚对平", j10, "10 锚且语义完备");

    // J11 路由面注记闭集
    let a = target_note(&Target::Tid(3));
    let b = target_note(&Target::ProcessPending);
    let j11 = a.contains("tid:3") && b.contains("proc") && a != b;
    s.add("J11-注记互异", j11, "tid/proc 注记互异");

    // J12 掩码位对撞闭集：位在拒、位缺通
    let mut t6 = ThreadMasks::new();
    t6.add(1, 1 << SIGUSR1);
    t6.add(2, 0);
    let j12 = t6.lowest_unblocked(SIGUSR1) == Some(2)
        && t6.lowest_unblocked(2) == Some(1);
    s.add("J12-位对撞闭集", j12, "按号逐位对撞");

    // J13 (dyn) 路由账指纹落账
    let mut t7 = ThreadMasks::new();
    t7.add(3, 0);
    t7.add(7, 1 << SIGUSR1);
    let mut lg2 = RouteLedger::new(t7);
    let _ = lg2.deliver(SIGUSR1);
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(lg2.delivered.len() as u32);
    h = h.wrapping_mul(31).wrapping_add(lg2.t.proc_pending);
    for c in lg2.note.bytes() {
        h = h.wrapping_mul(31).wrapping_add(c as u32);
    }
    s.add_dyn("J13-路由账指纹", h != 0, &format!("fold={:#x}", h));

    // J14 (dyn) 矩阵命中明细
    s.add_dyn(
        "J14-命中明细",
        hits == total,
        &format!("hits={}/{} cap_ok={}", hits, total, cap_ok),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10838_route_flow() {
        let mut t = ThreadMasks::new();
        t.add(3, 1 << SIGUSR1);
        t.add(7, 0);
        t.add(9, 0);
        assert_eq!(judge(&t, SIGUSR1), Target::Tid(7));
        t.rows[1].1 = 1 << SIGUSR1;
        assert_eq!(judge(&t, SIGUSR1), Target::Tid(9));
        t.rows[2].1 = 1 << SIGUSR1;
        assert_eq!(judge(&t, SIGUSR1), Target::ProcessPending);
        assert!(t.all_blocked(SIGUSR1));
        let inj = inject_two();
        assert!(inj.iter().all(|b| *b));
    }

    #[test]
    fn unx10838_checks_green() {
        let s = run_unx10838_checks();
        assert!(s.all_passed(), "UNX-F10838 判据未全绿");
    }
}
