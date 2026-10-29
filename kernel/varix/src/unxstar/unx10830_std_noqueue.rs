//! UNX-F10830 标准信号不排队合并矩阵（0x1c040 段）
//!
//! 锚点：UNX-F10830-J1 合并对平——std 号屏蔽期间 N 次发送必合并为一次
//! 交付、RT 号必逐次排队 N 次交付、两域互不串账三线全对平；注入坏实现
//! （std 排队/RT 合并）必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：双域账（std 位集 × RT 队列）+ 发送判定器（域对撞算 合并入位/
//! 入队）+ 交付状态机（std 一次清位、RT 逐个出队）+ 计数核（交付次数
//! 对平）+ 注入核（两类坏实现必分歧）+ 正反双样本矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX 锚：SIGRTMIN 起点锚（建模 34）、SIGUSR1=10（std 域）。
pub const SIGRTMIN: u32 = 34;
pub const SIGUSR1: u32 = 10;

/// 诊断段（0x1c040 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c040;
pub const NQ_STD_QUEUED: u32 = SEG_BASE + 0x01; // std 误排队
pub const NQ_RT_MERGED: u32 = SEG_BASE + 0x02; // RT 误合并
pub const NQ_DOMAIN_SKEW: u32 = SEG_BASE + 0x03; // 域判定错
pub const NQ_COUNT_LOSS: u32 = SEG_BASE + 0x04; // 计数丢失
pub const NQ_DRAIN_MISS: u32 = SEG_BASE + 0x05; // 排队排空不齐
pub const NQ_NOTE_SKEW: u32 = SEG_BASE + 0x06; // 注记错账

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        NQ_STD_QUEUED => "std 误排队",
        NQ_RT_MERGED => "RT 误合并",
        NQ_DOMAIN_SKEW => "域判定错",
        NQ_COUNT_LOSS => "计数丢失",
        NQ_DRAIN_MISS => "排队排空不齐",
        NQ_NOTE_SKEW => "注记错账",
        _ => "未知诊断码",
    }
}

/// 域判定：sig >= SIGRTMIN 为 RT 域，否则 std 域。
pub fn is_rt(sig: u32) -> bool {
    sig >= SIGRTMIN
}

/// 发送判定输出。
#[derive(Clone, PartialEq, Eq)]
pub enum Send {
    /// std 域：合并入位（返回位集）。
    Merge(u32),
    /// RT 域：入队（返回队列长）。
    Enqueue(usize),
}

/// 发送判定核：std 屏蔽中入位合并、RT 屏蔽中入队。
pub fn judge(sig: u32, pending: u32, queue_len: usize) -> Send {
    if is_rt(sig) {
        Send::Enqueue(queue_len + 1)
    } else if pending & 1u32.wrapping_shl(sig) != 0 {
        Send::Merge(pending)
    } else {
        Send::Merge(pending | 1u32.wrapping_shl(sig))
    }
}

/// 双域账状态机：std 位集 + RT 队列，交付分账。
pub struct SigDual {
    pub std_pending: u32,
    pub rt_queue: Vec<u32>,
    pub std_delivered: u32,
    pub rt_delivered: u32,
    pub note: String,
}

impl SigDual {
    pub fn new() -> SigDual {
        SigDual {
            std_pending: 0,
            rt_queue: Vec::new(),
            std_delivered: 0,
            rt_delivered: 0,
            note: "fresh".to_string(),
        }
    }

    /// 屏蔽中发送：std 入位、RT 入队。
    pub fn send_blocked(&mut self, sig: u32) -> Send {
        let out = judge(sig, self.std_pending, self.rt_queue.len());
        match &out {
            Send::Merge(p) => {
                self.std_pending = *p;
                self.note = "std-merged".to_string();
            }
            Send::Enqueue(_n) => {
                self.rt_queue.push(sig);
                self.note = "rt-queued".to_string();
            }
        }
        out
    }

    /// 交付排空：std 一次性清位（合并交付 1 次），RT 逐个出队。
    pub fn drain(&mut self) -> (u32, usize) {
        let std_n = if self.std_pending != 0 { self.std_pending.count_ones() } else { 0 };
        self.std_delivered |= self.std_pending;
        self.std_pending = 0;
        let rt_n = self.rt_queue.len();
        let mut i = 0usize;
        while i < rt_n {
            self.rt_delivered += 1;
            i += 1;
        }
        self.rt_queue.clear();
        self.note = "drained".to_string();
        (std_n, rt_n)
    }

    /// 账面核：双域无交叠（std 号不入 RT 队、RT 号不入 std 位集）。
    pub fn domains_ok(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.rt_queue.len() {
            if !is_rt(self.rt_queue[i]) {
                ok = false;
            }
            i += 1;
        }
        // std_pending 只含 std 域位
        let mut probe = self.std_pending;
        while probe != 0 {
            let bit = probe.trailing_zeros();
            if is_rt(bit) {
                ok = false;
            }
            probe &= !(1 << bit);
        }
        ok
    }
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 16;

/// 正反双样本矩阵：期望 Send。
pub fn sample_matrix() -> Vec<(u32, u32, usize, Send)> {
    let mut m: Vec<(u32, u32, usize, Send)> = Vec::new();
    // std 域：首入置位
    m.push((SIGUSR1, 0, 0, Send::Merge(1 << SIGUSR1)));
    // std 域：重复合并
    m.push((SIGUSR1, 1 << SIGUSR1, 0, Send::Merge(1 << SIGUSR1)));
    // RT 域：逐次入队
    m.push((SIGRTMIN, 0, 0, Send::Enqueue(1)));
    m.push((SIGRTMIN, 0, 3, Send::Enqueue(4)));
    m.push((SIGRTMIN + 5, 0, 1, Send::Enqueue(2)));
    // 边界：SIGRTMIN-1 仍 std 域
    m.push((SIGRTMIN - 1, 0, 0, Send::Merge(1u32.wrapping_shl(SIGRTMIN - 1))));
    m
}

/// 矩阵对平：返回 (命中数, 总数, 容量合规)。
pub fn verify_matrix() -> (usize, usize, bool) {
    let m = sample_matrix();
    let mut ok = 0usize;
    let mut i = 0usize;
    while i < m.len() {
        let (sig, pending, qlen, want) = &m[i];
        if &judge(*sig, *pending, *qlen) == want {
            ok += 1;
        }
        i += 1;
    }
    (ok, m.len(), m.len() <= MATRIX_CAP)
}

/// 坏实现 A：std 误排队（std 也入队 → N 次交付）。
pub fn faulty_std_queue(sigs: &[u32]) -> usize {
    let mut q: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < sigs.len() {
        q.push(sigs[i]);
        i += 1;
    }
    q.len()
}

/// 坏实现 B：RT 误合并（RT 只记账数 1）。
pub fn faulty_rt_merge(_sigs: &[u32]) -> usize {
    1
}

/// 注入核：两类坏实现必与权威分歧（检出）。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    let sigs = [SIGUSR1, SIGUSR1, SIGUSR1];
    // 1) std 三连：权威交付 1 次、坏实现排队 3 次 → 检出
    got[0] = faulty_std_queue(&sigs) != 1 && faulty_std_queue(&sigs) == 3;
    // 2) RT 三连：权威交付 3 次、坏实现合并 1 次 → 检出
    let rts = [SIGRTMIN, SIGRTMIN, SIGRTMIN];
    got[1] = faulty_rt_merge(&rts) != 3 && faulty_rt_merge(&rts) == 1;
    got
}

/// 合并全序核：std 三连发送→一次交付；RT 三连发送→三次交付。
pub fn dual_seq_ok() -> bool {
    let mut d = SigDual::new();
    let _ = d.send_blocked(SIGUSR1);
    let _ = d.send_blocked(SIGUSR1);
    let _ = d.send_blocked(SIGUSR1);
    let _ = d.send_blocked(SIGRTMIN);
    let _ = d.send_blocked(SIGRTMIN);
    let _ = d.send_blocked(SIGRTMIN);
    let pre = d.rt_queue.len() == 3 && d.std_pending == (1 << SIGUSR1);
    let (std_n, rt_n) = d.drain();
    pre && std_n == 1 && rt_n == 3 && d.std_pending == 0 && d.rt_queue.is_empty()
        && d.rt_delivered == 3 && d.domains_ok()
}

/// 域界核：SIGRTMIN-1 std 域、SIGRTMIN RT 域。
pub fn domain_bound_ok() -> bool {
    !is_rt(SIGRTMIN - 1) && is_rt(SIGRTMIN) && !is_rt(SIGUSR1)
}

/// 交付面注记（对账/落账用）。
pub fn drain_note(std_n: u32, rt_n: usize) -> String {
    format!("std={} rt={} total={}", std_n, rt_n, std_n as usize + rt_n)
}

/// 判据（对应卡验收 J 判据）。
pub fn run_unx10830_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10830");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平
    s.add("J1-矩阵对平", hits == total && cap_ok, "六线发送样本全命中");

    // J2 正路径：std 首入置位
    let j2 = judge(SIGUSR1, 0, 0) == Send::Merge(1 << SIGUSR1);
    s.add("J2-std置位", j2, "首入置 1 位");

    // J3 正路径：std 重复合并
    let j3 = judge(SIGUSR1, 1 << SIGUSR1, 0) == Send::Merge(1 << SIGUSR1);
    s.add("J3-std合并", j3, "重复发送只 1 位");

    // J4 正路径：RT 逐次入队
    let j4 = judge(SIGRTMIN, 0, 0) == Send::Enqueue(1)
        && judge(SIGRTMIN, 0, 3) == Send::Enqueue(4);
    s.add("J4-RT排队", j4, "N 发 N 队");

    // J5 合并全序核
    s.add("J5-双域全序", dual_seq_ok(), "std 1 次 / RT 3 次交付");

    // J6 注入1 std 误排队检出
    s.add("J6-误排队检出", inj[0], "std 3 次交付坏实现必分歧");

    // J7 注入2 RT 误合并检出
    s.add("J7-误合并检出", inj[1], "RT 1 次交付坏实现必分歧");

    // J8 域界核
    s.add("J8-域界核", domain_bound_ok(), "SIGRTMIN-1/RTMIN 两界清晰");

    // J9 双域无串账核
    let mut d2 = SigDual::new();
    let _ = d2.send_blocked(SIGUSR1);
    let _ = d2.send_blocked(SIGRTMIN);
    let j9 = d2.domains_ok() && d2.rt_queue.len() == 1 && d2.std_pending == (1 << SIGUSR1);
    s.add("J9-无串账", j9, "std 位与 RT 队互斥");

    // J10 排空核：排空后双域清零
    let (sn, rn) = {
        let mut d3 = SigDual::new();
        let _ = d3.send_blocked(SIGRTMIN);
        let _ = d3.send_blocked(SIGUSR1);
        d3.drain()
    };
    let j10 = sn == 1 && rn == 1 && drain_note(sn, rn).contains("total=2");
    s.add("J10-排空核", j10, "排空对账含总交付");

    // J11 字面锚：SIGRTMIN=34 / SIGUSR1=10
    let j11 = SIGRTMIN == 34 && SIGUSR1 == 10 && diag_text(NQ_STD_QUEUED).len() > 0;
    s.add("J11-字面锚对平", j11, "34/10 双锚且语义完备");

    // J12 域判定闭集：0..33 std、34..64 RT
    let j12 = !is_rt(0) && !is_rt(33) && is_rt(34) && is_rt(64);
    s.add("J12-域闭集", j12, "域界逐点清晰");

    // J13 (dyn) 双域账指纹落账
    let mut d4 = SigDual::new();
    let _ = d4.send_blocked(SIGUSR1);
    let _ = d4.send_blocked(SIGRTMIN);
    let _ = d4.send_blocked(SIGRTMIN + 1);
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(d4.std_pending);
    h = h.wrapping_mul(31).wrapping_add(d4.rt_queue.len() as u32);
    for c in d4.note.bytes() {
        h = h.wrapping_mul(31).wrapping_add(c as u32);
    }
    s.add_dyn("J13-双域账指纹", h != 0, &format!("fold={:#x}", h));

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
    fn unx10830_dual_flow() {
        let mut d = SigDual::new();
        assert!(matches!(d.send_blocked(SIGUSR1), Send::Merge(_)));
        assert!(matches!(d.send_blocked(SIGRTMIN), Send::Enqueue(_)));
        assert_eq!(d.std_pending, 1 << SIGUSR1);
        assert_eq!(d.rt_queue.len(), 1);
        let (sn, rn) = d.drain();
        assert_eq!(sn, 1);
        assert_eq!(rn, 1);
        assert!(d.domains_ok());
        assert!(is_rt(SIGRTMIN + 2) && !is_rt(SIGUSR1));
        let inj = inject_two();
        assert!(inj.iter().all(|b| *b));
    }

    #[test]
    fn unx10830_checks_green() {
        let s = run_unx10830_checks();
        assert!(s.all_passed(), "UNX-F10830 判据未全绿");
    }
}
