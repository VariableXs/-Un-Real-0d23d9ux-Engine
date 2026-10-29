//! UNX-F10837 exec 信号处置复位矩阵（0x1c120 段）
//!
//! 锚点：UNX-F10837-J1 复位对平——被捕获处置必复位为默认、SIG_IGN 处置
//! 保持、屏蔽账与挂起集跨 exec 存续三线全对平；注入坏实现（IGN 也被复位/
//! 挂起集清空）必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：exec 复位判定器（处置型对撞算 复位/保持）+ 快照状态机（exec 前
//! 后账对照）+ 全景扫描核（三处置×两账全扫）+ 注入核（两类坏实现必分歧）
//! + 正反双样本矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX 锚：SIGUSR1=10。
pub const SIGUSR1: u32 = 10;

/// 诊断段（0x1c120 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c120;
pub const EX_IGN_RESET: u32 = SEG_BASE + 0x01; // IGN 被误复位
pub const EX_CAUGHT_KEEP: u32 = SEG_BASE + 0x02; // 被捕获处置未复位
pub const EX_PEND_LOSS: u32 = SEG_BASE + 0x03; // 挂起集被清
pub const EX_MASK_LOSS: u32 = SEG_BASE + 0x04; // 屏蔽账丢失
pub const EX_SCAN_MISS: u32 = SEG_BASE + 0x05; // 全景扫描不齐
pub const EX_NOTE_SKEW: u32 = SEG_BASE + 0x06; // 注记错账

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        EX_IGN_RESET => "IGN 被误复位",
        EX_CAUGHT_KEEP => "被捕获处置未复位",
        EX_PEND_LOSS => "挂起集被清",
        EX_MASK_LOSS => "屏蔽账丢失",
        EX_SCAN_MISS => "全景扫描不齐",
        EX_NOTE_SKEW => "注记错账",
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

/// exec 前快照。
#[derive(Clone)]
pub struct PreExec {
    pub handlers: Vec<(u32, Disp)>,
    pub mask: u32,
    pub pending: u32,
    pub note: String,
}

impl PreExec {
    pub fn new() -> PreExec {
        PreExec { handlers: Vec::new(), mask: 0, pending: 0, note: "pre".to_string() }
    }

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
}

/// exec 复位判定核：被捕获处置 → 复位为默认；SIG_IGN → 保持；Dflt → 保持。
pub fn judge(d: &Disp) -> Disp {
    match d {
        Disp::Handler => Disp::Dflt,
        other => other.clone(),
    }
}

/// exec 后快照构造：逐号复位/保持，mask/pending 原样存续。
pub fn post_exec(pre: &PreExec) -> PreExec {
    let mut out = PreExec::new();
    out.note = "post".to_string();
    let mut i = 0usize;
    while i < pre.handlers.len() {
        let (sig, d) = &pre.handlers[i];
        out.install(*sig, judge(d));
        i += 1;
    }
    out.mask = pre.mask;
    out.pending = pre.pending;
    out
}

/// 坏实现 A：IGN 也被复位。
pub fn faulty_ign_reset(pre: &PreExec) -> PreExec {
    let mut out = PreExec::new();
    out.note = "post-bad".to_string();
    let mut i = 0usize;
    while i < pre.handlers.len() {
        let (sig, _d) = &pre.handlers[i];
        out.install(*sig, Disp::Dflt);
        i += 1;
    }
    out.mask = pre.mask;
    out.pending = pre.pending;
    out
}

/// 坏实现 B：挂起集被清。
pub fn faulty_pend_loss(pre: &PreExec) -> PreExec {
    let mut out = post_exec(pre);
    out.pending = 0;
    out
}

/// 注入核：两类坏实现必与权威分歧（检出）。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    let mut pre = PreExec::new();
    pre.install(17, Disp::Ign);
    pre.install(SIGUSR1, Disp::Handler);
    pre.mask = 0x40;
    pre.pending = 1 << 3;
    // 1) IGN 复位：权威 post 保持 Ign、坏实现复位 Dflt → 检出
    let good = post_exec(&pre);
    let bad = faulty_ign_reset(&pre);
    got[0] = good.lookup(17) != bad.lookup(17)
        && good.lookup(17) == Disp::Ign
        && bad.lookup(17) == Disp::Dflt;
    // 2) 挂起丢失：权威 post 存续、坏实现清零 → 检出
    let bad2 = faulty_pend_loss(&pre);
    got[1] = good.pending != bad2.pending && good.pending == (1 << 3) && bad2.pending == 0;
    got
}

/// 全景扫描核：三处置型 exec 后走向 + mask/pending 存续逐项对平。
pub fn scan_ok() -> bool {
    let mut ok = true;
    let disps = [Disp::Handler, Disp::Ign, Disp::Dflt];
    let mut i = 0usize;
    while i < disps.len() {
        if judge(&disps[i]) != expected_post(&disps[i]) {
            ok = false;
        }
        i += 1;
    }
    // mask/pending 存续
    let mut pre = PreExec::new();
    pre.mask = 0xFF;
    pre.pending = 0x0F0F;
    let post = post_exec(&pre);
    ok && post.mask == 0xFF && post.pending == 0x0F0F
}

/// 期望 exec 后处置。
pub fn expected_post(d: &Disp) -> Disp {
    match d {
        Disp::Handler => Disp::Dflt,
        other => other.clone(),
    }
}

/// 全序核：exec 前后账对照（Handler 复位、Ign/Dflt 保持、mask/pending 存续）。
pub fn seq_ok() -> bool {
    let mut pre = PreExec::new();
    pre.install(SIGUSR1, Disp::Handler);
    pre.install(17, Disp::Ign);
    pre.install(2, Disp::Dflt);
    pre.mask = 0x30;
    pre.pending = 1 << 7;
    let post = post_exec(&pre);
    post.lookup(SIGUSR1) == Disp::Dflt
        && post.lookup(17) == Disp::Ign
        && post.lookup(2) == Disp::Dflt
        && post.mask == 0x30
        && post.pending == (1 << 7)
        && post.note == "post"
}

/// 复位面注记（对账/落账用）。
pub fn reset_note(d: &Disp) -> String {
    match d {
        Disp::Handler => "reset:handler->default".to_string(),
        Disp::Ign => "keep:ign".to_string(),
        Disp::Dflt => "keep:default".to_string(),
    }
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 16;

/// 正反双样本矩阵：期望 exec 后处置。
pub fn sample_matrix() -> Vec<(Disp, Disp)> {
    let mut m: Vec<(Disp, Disp)> = Vec::new();
    m.push((Disp::Handler, Disp::Dflt));
    m.push((Disp::Ign, Disp::Ign));
    m.push((Disp::Dflt, Disp::Dflt));
    m
}

/// 矩阵对平：返回 (命中数, 总数, 容量合规)。
pub fn verify_matrix() -> (usize, usize, bool) {
    let m = sample_matrix();
    let mut ok = 0usize;
    let mut i = 0usize;
    while i < m.len() {
        let (pre_d, want) = &m[i];
        if &judge(pre_d) == want {
            ok += 1;
        }
        i += 1;
    }
    (ok, m.len(), m.len() <= MATRIX_CAP)
}

/// 判据（对应卡验收 J 判据）。
pub fn run_unx10837_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10837");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平
    s.add("J1-矩阵对平", hits == total && cap_ok, "三线复位样本全命中");

    // J2 正路径：被捕获处置复位为默认
    let j2 = judge(&Disp::Handler) == Disp::Dflt;
    s.add("J2-捕获复位", j2, "handler → default");

    // J3 正路径：SIG_IGN 保持
    let j3 = judge(&Disp::Ign) == Disp::Ign;
    s.add("J3-IGN保持", j3, "ignored 跨 exec");

    // J4 正路径：默认保持
    let j4 = judge(&Disp::Dflt) == Disp::Dflt;
    s.add("J4-默认保持", j4, "default 跨 exec");

    // J5 全序核：三处置+双账
    s.add("J5-全序核", seq_ok(), "复位/保持/存续全对");

    // J6 全景扫描核
    s.add("J6-全景扫描", scan_ok(), "三处置+双账逐项对平");

    // J7 注入1 IGN 复位检出
    s.add("J7-IGN检出", inj[0], "IGN 复位坏实现必分歧");

    // J8 注入2 挂起清空检出
    s.add("J8-挂起检出", inj[1], "pending 清空坏实现必分歧");

    // J9 屏蔽存续核
    let mut pre = PreExec::new();
    pre.mask = 0x1234;
    let post = post_exec(&pre);
    let j9 = post.mask == 0x1234;
    s.add("J9-屏蔽存续", j9, "mask 跨 exec 不变");

    // J10 字面锚：SIGUSR1=10
    let j10 = SIGUSR1 == 10 && diag_text(EX_IGN_RESET).len() > 0
        && diag_text(0xffff_ffff) == "未知诊断码";
    s.add("J10-字面锚对平", j10, "10 锚且语义完备");

    // J11 复位面注记闭集
    let a = reset_note(&Disp::Handler);
    let b = reset_note(&Disp::Ign);
    let c = reset_note(&Disp::Dflt);
    let j11 = a.contains("reset") && b.contains("keep") && c.contains("keep") && a != b;
    s.add("J11-注记互异", j11, "reset/keep 注记互异");

    // J12 post 账行数对账：行数不增不减
    let mut pre2 = PreExec::new();
    pre2.install(1, Disp::Handler);
    pre2.install(2, Disp::Ign);
    let post2 = post_exec(&pre2);
    let j12 = post2.handlers.len() == 2 && post2.handlers.len() == pre2.handlers.len();
    s.add("J12-行数对账", j12, "exec 不增删行");

    // J13 (dyn) post 账指纹落账
    let mut pre3 = PreExec::new();
    pre3.install(SIGUSR1, Disp::Handler);
    pre3.install(17, Disp::Ign);
    pre3.mask = 0x20;
    pre3.pending = 0x400;
    let post3 = post_exec(&pre3);
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(post3.mask);
    h = h.wrapping_mul(31).wrapping_add(post3.pending);
    h = h.wrapping_mul(31).wrapping_add(post3.handlers.len() as u32);
    for ch in post3.note.bytes() {
        h = h.wrapping_mul(31).wrapping_add(ch as u32);
    }
    s.add_dyn("J13-post账指纹", h != 0, &format!("fold={:#x}", h));

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
    fn unx10837_exec_flow() {
        let mut pre = PreExec::new();
        pre.install(SIGUSR1, Disp::Handler);
        pre.install(17, Disp::Ign);
        pre.mask = 0x44;
        pre.pending = 0x400;
        let post = post_exec(&pre);
        assert_eq!(post.lookup(SIGUSR1), Disp::Dflt);
        assert_eq!(post.lookup(17), Disp::Ign);
        assert_eq!(post.mask, 0x44);
        assert_eq!(post.pending, 0x400);
        let bad = faulty_ign_reset(&pre);
        assert_eq!(bad.lookup(17), Disp::Dflt);
        let inj = inject_two();
        assert!(inj.iter().all(|b| *b));
    }

    #[test]
    fn unx10837_checks_green() {
        let s = run_unx10837_checks();
        assert!(s.all_passed(), "UNX-F10837 判据未全绿");
    }
}
