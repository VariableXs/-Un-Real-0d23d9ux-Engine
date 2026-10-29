//! UNX-F10836 fork 信号继承矩阵（0x1c100 段）
//!
//! 锚点：UNX-F10836-J1 继承对平——子进程继承处理器账与屏蔽账、SIG_IGN
//! 处置随继承、挂起集必清空三线全对平；注入坏实现（挂起集被复制/处理器
//! 账丢失）必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：进程信号快照（处理器账 × 屏蔽位 × SIG_IGN 集 × 挂起位集）+
//! fork 拷贝判定器（账三传一清）+ 继承状态机（子账独立推进）+ 重放核
//! （父子账互不串扰）+ 注入核（两类坏实现必分歧）+ 正反双样本矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX 锚：SIGUSR1=10。
pub const SIGUSR1: u32 = 10;

/// 诊断段（0x1c100 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c100;
pub const FK_PEND_COPIED: u32 = SEG_BASE + 0x01; // 挂起集被复制
pub const FK_HANDLER_LOSS: u32 = SEG_BASE + 0x02; // 处理器账丢失
pub const FK_MASK_LOSS: u32 = SEG_BASE + 0x03; // 屏蔽账丢失
pub const FK_IGN_MISS: u32 = SEG_BASE + 0x04; // SIG_IGN 集漏传
pub const FK_REPLAY_MISS: u32 = SEG_BASE + 0x05; // 重放不齐
pub const FK_NOTE_SKEW: u32 = SEG_BASE + 0x06; // 注记错账

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        FK_PEND_COPIED => "挂起集被复制",
        FK_HANDLER_LOSS => "处理器账丢失",
        FK_MASK_LOSS => "屏蔽账丢失",
        FK_IGN_MISS => "SIG_IGN 集漏传",
        FK_REPLAY_MISS => "重放不齐",
        FK_NOTE_SKEW => "注记错账",
        _ => "未知诊断码",
    }
}

/// 处置型。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Disp {
    Handler,
    Ign,
    Dflt,
}

/// 进程信号快照。
#[derive(Clone)]
pub struct SigSnap {
    pub handlers: Vec<(u32, Disp)>,
    pub mask: u32,
    pub pending: u32,
    pub note: String,
}

impl SigSnap {
    pub fn new() -> SigSnap {
        SigSnap { handlers: Vec::new(), mask: 0, pending: 0, note: "parent".to_string() }
    }

    /// 装处理器。
    pub fn install(&mut self, sig: u32, d: Disp) {
        let mut hit = false;
        let mut i = 0usize;
        while i < self.handlers.len() {
            if self.handlers[i].0 == sig {
                self.handlers[i].1 = d.clone();
                hit = true;
            }
            i += 1;
        }
        if !hit {
            self.handlers.push((sig, d));
        }
    }

    /// 查处置。
    pub fn lookup(&self, sig: u32) -> Disp {
        let mut i = 0usize;
        while i < self.handlers.len() {
            if self.handlers[i].0 == sig {
                return self.handlers[i].1.clone();
            }
            i += 1;
        }
        Disp::Dflt
    }

    /// 挂起信号入账。
    pub fn pend(&mut self, sig: u32) {
        self.pending |= 1 << sig;
    }
}

/// fork 拷贝判定核：处理器账/屏蔽账/SIG_IGN 随拷贝；挂起集清空。
pub fn fork_copy(p: &SigSnap) -> SigSnap {
    SigSnap {
        handlers: p.handlers.clone(),
        mask: p.mask,
        pending: 0,
        note: "child".to_string(),
    }
}

/// 坏实现 A：挂起集被复制。
pub fn faulty_pend_copy(p: &SigSnap) -> SigSnap {
    let mut c = fork_copy(p);
    c.pending = p.pending;
    c
}

/// 坏实现 B：处理器账丢失（子账清空）。
pub fn faulty_handler_loss(p: &SigSnap) -> SigSnap {
    SigSnap { handlers: Vec::new(), mask: p.mask, pending: 0, note: "child".to_string() }
}

/// 注入核：两类坏实现必与权威分歧（检出）。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    let mut p = SigSnap::new();
    p.install(SIGUSR1, Disp::Handler);
    p.mask = 1 << 5;
    p.pend(SIGUSR1);
    // 1) 挂起集复制：权威子账 pending==0、坏实现非 0 → 检出
    let good = fork_copy(&p);
    let bad = faulty_pend_copy(&p);
    got[0] = good.pending != bad.pending && good.pending == 0 && bad.pending == (1 << SIGUSR1);
    // 2) 处理器账丢失：权威子账含 USR1 行、坏实现空 → 检出
    let bad2 = faulty_handler_loss(&p);
    got[1] = good.lookup(SIGUSR1) != bad2.lookup(SIGUSR1)
        && good.lookup(SIGUSR1) == Disp::Handler
        && bad2.lookup(SIGUSR1) == Disp::Dflt;
    got
}

/// 继承全序核：父账（handler+mask+pending）→ fork → 子账三传一清；
/// 子账独立推进不回写父账。
pub fn inherit_seq_ok() -> bool {
    let mut p = SigSnap::new();
    p.install(SIGUSR1, Disp::Handler);
    p.install(2, Disp::Ign);
    p.mask = (1 << 5) | (1 << 6);
    p.pend(SIGUSR1);
    let mut c = fork_copy(&p);
    let pass = c.lookup(SIGUSR1) == Disp::Handler
        && c.lookup(2) == Disp::Ign
        && c.mask == p.mask
        && c.pending == 0
        && c.note == "child";
    // 子账独立推进：子装 USR2、加屏蔽；父账不变
    c.install(12, Disp::Handler);
    c.mask |= 1 << 12;
    c.pend(SIGUSR1);
    let indep = p.lookup(12) == Disp::Dflt && p.mask == ((1 << 5) | (1 << 6))
        && p.pending == (1 << SIGUSR1);
    pass && indep
}

/// SIG_IGN 继承核：忽略处置随拷贝（子进程同样自动收尸语义）。
pub fn ign_inherit_ok() -> bool {
    let mut p = SigSnap::new();
    p.install(17, Disp::Ign);
    let c = fork_copy(&p);
    c.lookup(17) == Disp::Ign
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 16;

/// 正反双样本矩阵：期望子账快照字段。
pub fn sample_matrix() -> Vec<(u32, Disp, u32, u32, bool)> {
    // (sig, disp, 父 mask, 父 pending, 期望子账保留该处置)
    let mut m: Vec<(u32, Disp, u32, u32, bool)> = Vec::new();
    m.push((SIGUSR1, Disp::Handler, 1 << 5, 0, true));
    m.push((17, Disp::Ign, 0, 1 << 17, true));
    m.push((2, Disp::Dflt, 1 << 2, 0, true));
    m
}

/// 矩阵对平：返回 (命中数, 总数, 容量合规)。
pub fn verify_matrix() -> (usize, usize, bool) {
    let m = sample_matrix();
    let mut ok = 0usize;
    let mut i = 0usize;
    while i < m.len() {
        let (sig, disp, mask, pending, want_keep) = &m[i];
        let mut p = SigSnap::new();
        p.install(*sig, disp.clone());
        p.mask = *mask;
        p.pending = *pending;
        let c = fork_copy(&p);
        let kept = c.lookup(*sig) == *disp && c.mask == *mask;
        if &kept == want_keep {
            ok += 1;
        }
        i += 1;
    }
    (ok, m.len(), m.len() <= MATRIX_CAP)
}

/// 继承面注记（对账/落账用）。
pub fn inherit_note(c: &SigSnap) -> String {
    format!("role={} mask={:#x} pend={:#x} rows={}", c.note, c.mask, c.pending, c.handlers.len())
}

/// 判据（对应卡验收 J 判据）。
pub fn run_unx10836_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10836");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平
    s.add("J1-矩阵对平", hits == total && cap_ok, "三线继承样本全命中");

    // J2 正路径：处理器账随拷贝
    let mut p = SigSnap::new();
    p.install(SIGUSR1, Disp::Handler);
    let c = fork_copy(&p);
    let j2 = c.lookup(SIGUSR1) == Disp::Handler;
    s.add("J2-处理器随拷", j2, "子账同处置");

    // J3 正路径：屏蔽账随拷贝
    let mut p2 = SigSnap::new();
    p2.mask = 0x30;
    let c2 = fork_copy(&p2);
    let j3 = c2.mask == 0x30;
    s.add("J3-屏蔽随拷", j3, "子账同掩码");

    // J4 正路径：挂起集清空
    let mut p3 = SigSnap::new();
    p3.pend(SIGUSR1);
    p3.pend(2);
    let c3 = fork_copy(&p3);
    let j4 = c3.pending == 0;
    s.add("J4-挂起清空", j4, "pending 不跨 fork");

    // J5 SIG_IGN 继承核
    s.add("J5-IGN继承", ign_inherit_ok(), "忽略处置随拷贝");

    // J6 继承全序核：三传一清 + 子账独立
    s.add("J6-全序核", inherit_seq_ok(), "传三清一且互不串扰");

    // J7 注入1 挂起复制检出
    s.add("J7-复制检出", inj[0], "挂起集复制坏实现必分歧");

    // J8 注入2 处理器丢失检出
    s.add("J8-丢失检出", inj[1], "处理器账清空坏实现必分歧");

    // J9 子账行数对账：拷贝行数==父账行数
    let mut p4 = SigSnap::new();
    p4.install(1, Disp::Handler);
    p4.install(2, Disp::Ign);
    p4.install(15, Disp::Handler);
    let c4 = fork_copy(&p4);
    let j9 = c4.handlers.len() == 3 && c4.handlers.len() == p4.handlers.len();
    s.add("J9-行数对账", j9, "三行账全传");

    // J10 字面锚：SIGUSR1=10
    let j10 = SIGUSR1 == 10 && diag_text(FK_PEND_COPIED).len() > 0
        && diag_text(0xffff_ffff) == "未知诊断码";
    s.add("J10-字面锚对平", j10, "10 锚且语义完备");

    // J11 继承面注记闭集
    let n_p = inherit_note(&p4);
    let n_c = inherit_note(&c4);
    let j11 = n_p.contains("role=parent") && n_c.contains("role=child") && n_p != n_c;
    s.add("J11-注记互异", j11, "parent/child 注记互异");

    // J12 挂起清空位级核：全位集拷贝后为 0
    let mut p5 = SigSnap::new();
    p5.pending = 0xFFFF_FFFF;
    let c5 = fork_copy(&p5);
    let j12 = c5.pending == 0 && p5.pending == 0xFFFF_FFFF;
    s.add("J12-全位清空", j12, "满挂起集子账全清");

    // J13 (dyn) 子账指纹落账
    let mut p6 = SigSnap::new();
    p6.install(SIGUSR1, Disp::Handler);
    p6.mask = 0x20;
    p6.pend(SIGUSR1);
    let c6 = fork_copy(&p6);
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(c6.mask);
    h = h.wrapping_mul(31).wrapping_add(c6.pending);
    h = h.wrapping_mul(31).wrapping_add(c6.handlers.len() as u32);
    for ch in c6.note.bytes() {
        h = h.wrapping_mul(31).wrapping_add(ch as u32);
    }
    s.add_dyn("J13-子账指纹", h != 0, &format!("fold={:#x}", h));

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
    fn unx10836_fork_flow() {
        let mut p = SigSnap::new();
        p.install(SIGUSR1, Disp::Handler);
        p.install(17, Disp::Ign);
        p.mask = 0x40;
        p.pend(SIGUSR1);
        let c = fork_copy(&p);
        assert_eq!(c.lookup(SIGUSR1), Disp::Handler);
        assert_eq!(c.lookup(17), Disp::Ign);
        assert_eq!(c.mask, 0x40);
        assert_eq!(c.pending, 0);
        let bad = faulty_pend_copy(&p);
        assert_eq!(bad.pending, 1 << SIGUSR1);
        let inj = inject_two();
        assert!(inj.iter().all(|b| *b));
    }

    #[test]
    fn unx10836_checks_green() {
        let s = run_unx10836_checks();
        assert!(s.all_passed(), "UNX-F10836 判据未全绿");
    }
}
