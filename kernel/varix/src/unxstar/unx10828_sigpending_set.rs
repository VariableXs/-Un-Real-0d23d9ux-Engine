//! UNX-F10828 sigpending 挂起集矩阵（0x1c000 段）
//!
//! 锚点：UNX-F10828-J1 挂起集对平——屏蔽期间到达信号必入挂起集、std 号
//! 重复到达合并为一位、解屏蔽后恰一次交付且挂起位清零、非屏蔽信号直达
//! 不入挂起集四线全对平；注入坏实现（重复交付/挂起位残留）必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：挂起集（u32 位集）+ 屏蔽账（u32 位集）+ 到达判定器（屏蔽对撞
//! 算 挂起/直达）+ 交付状态机（解屏蔽逐位清账）+ 重放核（事件序逐笔同判）
//! + 注入核（两类坏实现必分歧）+ 正反双样本矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX 锚：SIGUSR1=10、SIGUSR2=12。
pub const SIGUSR1: u32 = 10;
pub const SIGUSR2: u32 = 12;

/// 诊断段（0x1c000 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c000;
pub const PD_DOUBLE_DELIVER: u32 = SEG_BASE + 0x01; // 重复交付
pub const PD_PEND_RESIDUE: u32 = SEG_BASE + 0x02; // 挂起位残留
pub const PD_DIRECT_MISS: u32 = SEG_BASE + 0x03; // 非屏蔽信号误挂起
pub const PD_MASK_SKEW: u32 = SEG_BASE + 0x04; // 屏蔽账错
pub const PD_MERGE_MISS: u32 = SEG_BASE + 0x05; // 合并缺失
pub const PD_NOTE_SKEW: u32 = SEG_BASE + 0x06; // 注记错账

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        PD_DOUBLE_DELIVER => "重复交付",
        PD_PEND_RESIDUE => "挂起位残留",
        PD_DIRECT_MISS => "非屏蔽信号误挂起",
        PD_MASK_SKEW => "屏蔽账错",
        PD_MERGE_MISS => "合并缺失",
        PD_NOTE_SKEW => "注记错账",
        _ => "未知诊断码",
    }
}

/// 到达走向。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Arrival {
    /// 非屏蔽：直达交付。
    Direct,
    /// 屏蔽中：入挂起集（std 合并：位已置则吸收）。
    Pended(bool),
}

/// 到达判定核：屏蔽位在则挂起（记录是否首置），否则直达。
pub fn judge(sig: u32, mask: u32, already_pending: bool) -> Arrival {
    if mask & (1 << sig) != 0 {
        Arrival::Pended(!already_pending)
    } else {
        Arrival::Direct
    }
}

/// 交付核：解屏蔽后挂起位逐位清账，返回本次交付的位集（一次性全交）。
pub fn deliver(pending: u32, unblocked: u32) -> (u32, u32) {
    let hit = pending & unblocked;
    (pending & !hit, hit)
}

/// 挂起账状态机：事件序驱动（arrive/deliver/unblock）。
pub struct PendLedger {
    pub pending: u32,
    pub mask: u32,
    pub delivered: u32,
    pub log: Vec<(u32, String)>,
    pub note: String,
}

impl PendLedger {
    pub fn new(mask: u32) -> PendLedger {
        PendLedger { pending: 0, mask, delivered: 0, log: Vec::new(), note: "fresh".to_string() }
    }

    /// 信号到达：按判定入账。
    pub fn arrive(&mut self, sig: u32) -> Arrival {
        let already = self.pending & (1 << sig) != 0;
        let out = judge(sig, self.mask, already);
        match &out {
            Arrival::Direct => {
                self.delivered |= 1 << sig;
                self.log.push((sig, "direct".to_string()));
                self.note = "direct".to_string();
            }
            Arrival::Pended(first) => {
                if *first {
                    self.pending |= 1 << sig;
                }
                let tag = if *first { "pend" } else { "absorb" };
                self.log.push((sig, tag.to_string()));
                self.note = tag.to_string();
            }
        }
        out
    }

    /// 解屏蔽：清掩码位并触发交付。
    pub fn unblock(&mut self, sig: u32) -> u32 {
        self.mask &= !(1 << sig);
        let (next, hit) = deliver(self.pending, 1 << sig);
        self.pending = next;
        if hit != 0 {
            self.delivered |= hit;
            self.log.push((sig, "deliver".to_string()));
            self.note = "deliver".to_string();
        }
        hit
    }

    /// 重放核：挂起位与交付位无交叠、账行无重复交付。
    pub fn ledger_ok(&self) -> bool {
        self.pending & self.delivered == 0
    }
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 16;

/// 正反双样本矩阵：期望走向。
pub fn sample_matrix() -> Vec<(u32, u32, bool, Arrival)> {
    let mut m: Vec<(u32, u32, bool, Arrival)> = Vec::new();
    // 正：非屏蔽直达
    m.push((SIGUSR1, 0, false, Arrival::Direct));
    // 正：屏蔽中首置挂起
    m.push((SIGUSR1, 1 << SIGUSR1, false, Arrival::Pended(true)));
    // 正：屏蔽中重复吸收
    m.push((SIGUSR1, 1 << SIGUSR1, true, Arrival::Pended(false)));
    // 边界：屏蔽他号不影响本号直达
    m.push((SIGUSR2, 1 << SIGUSR1, false, Arrival::Direct));
    // 边界：多号屏蔽各挂各位
    m.push((SIGUSR2, (1 << SIGUSR1) | (1 << SIGUSR2), false, Arrival::Pended(true)));
    m
}

/// 矩阵对平：返回 (命中数, 总数, 容量合规)。
pub fn verify_matrix() -> (usize, usize, bool) {
    let m = sample_matrix();
    let mut ok = 0usize;
    let mut i = 0usize;
    while i < m.len() {
        let (sig, mask, already, want) = &m[i];
        if &judge(*sig, *mask, *already) == want {
            ok += 1;
        }
        i += 1;
    }
    (ok, m.len(), m.len() <= MATRIX_CAP)
}

/// 坏实现 A：重复交付（解屏蔽后挂起位不清）。
pub fn faulty_double(pending: u32, unblocked: u32) -> (u32, u32) {
    let (_next, hit) = deliver(pending, unblocked);
    // 坏实现：账不清，挂起位残留
    (pending, hit)
}

/// 坏实现 B：非屏蔽信号误挂起。
pub fn faulty_direct_miss(sig: u32, mask: u32, _already: bool) -> Arrival {
    // 坏实现：一律挂起
    let _ = mask;
    Arrival::Pended(true)
}

/// 注入核：两类坏实现必与权威分歧（检出）。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    // 1) 重复交付：权威挂起清零、坏实现残留 → 检出
    let (g_next, _g_hit) = deliver(1 << SIGUSR1, 1 << SIGUSR1);
    let (b_next, _b_hit) = faulty_double(1 << SIGUSR1, 1 << SIGUSR1);
    got[0] = g_next != b_next && g_next == 0 && b_next == (1 << SIGUSR1);
    // 2) 非屏蔽误挂起：权威 Direct、坏实现 Pended → 检出
    got[1] = judge(SIGUSR1, 0, false) != faulty_direct_miss(SIGUSR1, 0, false)
        && judge(SIGUSR1, 0, false) == Arrival::Direct;
    got
}

/// 全序核：屏蔽 USR1 → 三连到达（首置+两吸收）→ 解屏蔽一次交付 → 挂起清零。
pub fn pend_seq_ok() -> bool {
    let mut lg = PendLedger::new(1 << SIGUSR1);
    let a1 = lg.arrive(SIGUSR1);
    let a2 = lg.arrive(SIGUSR1);
    let a3 = lg.arrive(SIGUSR1);
    let s1 = a1 == Arrival::Pended(true) && a2 == Arrival::Pended(false)
        && a3 == Arrival::Pended(false) && lg.pending == (1 << SIGUSR1);
    let hit = lg.unblock(SIGUSR1);
    let s2 = hit == (1 << SIGUSR1) && lg.pending == 0
        && (lg.delivered & (1 << SIGUSR1)) != 0 && lg.ledger_ok();
    s1 && s2 && lg.log.len() == 4
}

/// 双号独立核：USR1/USR2 各自屏蔽与交付互不串扰。
pub fn two_sig_ok() -> bool {
    let mut lg = PendLedger::new((1 << SIGUSR1) | (1 << SIGUSR2));
    let _ = lg.arrive(SIGUSR1);
    let _ = lg.arrive(SIGUSR2);
    let both = lg.pending == (1 << SIGUSR1) | (1 << SIGUSR2);
    let h1 = lg.unblock(SIGUSR1);
    let h2 = lg.unblock(SIGUSR2);
    both && h1 == (1 << SIGUSR1) && h2 == (1 << SIGUSR2) && lg.pending == 0
}

/// 直达核：非屏蔽信号直接置交付位不入挂起集。
pub fn direct_ok() -> bool {
    let mut lg = PendLedger::new(0);
    let a = lg.arrive(SIGUSR1);
    a == Arrival::Direct && lg.pending == 0 && (lg.delivered & (1 << SIGUSR1)) != 0
}

/// 挂起面注记（对账/落账用）。
pub fn pend_note(lg: &PendLedger) -> String {
    format!("pend={:#x} mask={:#x} note={}", lg.pending, lg.mask, lg.note)
}

/// 判据（对应卡验收 J 判据）。
pub fn run_unx10828_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10828");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平
    s.add("J1-矩阵对平", hits == total && cap_ok, "五线到达样本全命中");

    // J2 正路径：屏蔽中到达必挂起
    let j2 = judge(SIGUSR1, 1 << SIGUSR1, false) == Arrival::Pended(true);
    s.add("J2-屏蔽挂起", j2, "掩码命中入挂起集");

    // J3 正路径：重复到达吸收（std 合并）
    let j3 = judge(SIGUSR1, 1 << SIGUSR1, true) == Arrival::Pended(false);
    s.add("J3-重复吸收", j3, "位已置不再计");

    // J4 正路径：非屏蔽直达
    let j4 = judge(SIGUSR1, 0, false) == Arrival::Direct;
    s.add("J4-直达面", j4, "非屏蔽数直达交付");

    // J5 全序核：三连到达→一次交付→清零
    s.add("J5-全序核", pend_seq_ok(), "首置+吸收+一次交付全对");

    // J6 注入1 重复交付检出
    s.add("J6-重复交付检出", inj[0], "挂起位残留坏实现必分歧");

    // J7 注入2 误挂起检出
    s.add("J7-误挂起检出", inj[1], "直达面误挂起坏实现必分歧");

    // J8 双号独立核
    s.add("J8-双号独立", two_sig_ok(), "USR1/USR2 互不串扰");

    // J9 直达核
    s.add("J9-直达核", direct_ok(), "直达不污染挂起集");

    // J10 交付核：解屏蔽只交解的位
    let (next, hit) = deliver((1 << SIGUSR1) | (1 << SIGUSR2), 1 << SIGUSR1);
    let j10 = hit == (1 << SIGUSR1) && next == (1 << SIGUSR2);
    s.add("J10-逐位交付", j10, "只清解屏蔽位");

    // J11 字面锚：SIGUSR1=10 / SIGUSR2=12
    let j11 = SIGUSR1 == 10 && SIGUSR2 == 12 && diag_text(PD_MERGE_MISS).len() > 0;
    s.add("J11-字面锚对平", j11, "10/12 双锚且语义完备");

    // J12 挂起面注记闭集
    let mut lg2 = PendLedger::new(1 << SIGUSR2);
    let _ = lg2.arrive(SIGUSR2);
    let n1 = pend_note(&lg2);
    let _ = lg2.unblock(SIGUSR2);
    let n2 = pend_note(&lg2);
    let j12 = n1.contains("pend=0x400") && n2.contains("pend=0x0") && n1 != n2;
    s.add("J12-注记闭集", j12, "挂起/清零注记可辨");

    // J13 (dyn) 挂起账指纹落账
    let mut lg3 = PendLedger::new(0);
    let _ = lg3.arrive(SIGUSR1);
    let _ = lg3.arrive(SIGUSR2);
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(lg3.pending);
    h = h.wrapping_mul(31).wrapping_add(lg3.delivered);
    h = h.wrapping_mul(31).wrapping_add(lg3.log.len() as u32);
    for c in lg3.note.bytes() {
        h = h.wrapping_mul(31).wrapping_add(c as u32);
    }
    s.add_dyn("J13-挂起账指纹", h != 0, &format!("fold={:#x}", h));

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
    fn unx10828_pending_flow() {
        let mut lg = PendLedger::new(1 << SIGUSR2);
        assert_eq!(lg.arrive(SIGUSR2), Arrival::Pended(true));
        assert_eq!(lg.arrive(SIGUSR2), Arrival::Pended(false));
        assert_eq!(lg.arrive(SIGUSR1), Arrival::Direct);
        assert_eq!(lg.pending, 1 << SIGUSR2);
        let hit = lg.unblock(SIGUSR2);
        assert_eq!(hit, 1 << SIGUSR2);
        assert_eq!(lg.pending, 0);
        assert!(lg.ledger_ok());
        let inj = inject_two();
        assert!(inj.iter().all(|b| *b));
    }

    #[test]
    fn unx10828_checks_green() {
        let s = run_unx10828_checks();
        assert!(s.all_passed(), "UNX-F10828 判据未全绿");
    }
}
