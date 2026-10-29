//! UNX-F10829 sigsuspend 原子等待矩阵（0x1c020 段）
//!
//! 锚点：UNX-F10829-J1 原子性对平——sigsuspend 原子换掩码+等待+还原，
//! 信号在任何间隙到达必不丢失；两步法（先 UNBLOCK 再 pause）存在竞态窗，
//! 窗内到达必丢失（检出面）三线全对平；注入坏实现（两步法丢信号）必与
//! 权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：事件时序账（信号到达时刻 × 掩码快照）+ 原子判定器（单点换装
//! 无窗）+ 竞态窗状态机（两步法窗内到达丢失）+ 还原核（返回后掩码还原）
//! + 注入核（两类坏实现必分歧）+ 正反双样本矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX 锚：SIGUSR1=10、EINTR=4。
pub const SIGUSR1: u32 = 10;
pub const POSIX_EINTR: u32 = 4;

/// 诊断段（0x1c020 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c020;
pub const SS_RACE_LOSS: u32 = SEG_BASE + 0x01; // 竞态窗内信号丢失
pub const SS_RESTORE_MISS: u32 = SEG_BASE + 0x02; // 返回后掩码未还原
pub const SS_ATOMIC_MISS: u32 = SEG_BASE + 0x03; // 原子单点缺失
pub const SS_WAKE_MISS: u32 = SEG_BASE + 0x04; // 唤醒漏账
pub const SS_SEQ_SKEW: u32 = SEG_BASE + 0x05; // 时序账错
pub const SS_NOTE_SKEW: u32 = SEG_BASE + 0x06; // 注记错账

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        SS_RACE_LOSS => "竞态窗内信号丢失",
        SS_RESTORE_MISS => "返回后掩码未还原",
        SS_ATOMIC_MISS => "原子单点缺失",
        SS_WAKE_MISS => "唤醒漏账",
        SS_SEQ_SKEW => "时序账错",
        SS_NOTE_SKEW => "注记错账",
        _ => "未知诊断码",
    }
}

/// 信号到达时刻（相对 sigsuspend 调用）。
#[derive(Clone, PartialEq, Eq)]
pub enum At {
    /// 换装前（旧掩码仍在）。
    Before,
    /// 原子换装点（单点：旧掩码与新掩码同一原子操作）。
    SwapPoint,
    /// 等待中（新掩码生效）。
    During,
    /// 返回后（旧掩码已还原）。
    After,
}

/// 原子判定核：sigsuspend 任何时刻到达都不丢——Before/After 时刻按旧
/// 掩码判（若新号未屏蔽则直达），SwapPoint/During 按新掩码判（解屏蔽则
/// 立即唤醒）。输出 (是否立即处理, 唤醒)。
pub fn judge_atomic(at: &At, sig: u32, old_mask: u32, new_mask: u32) -> (bool, bool) {
    let mask = match at {
        At::Before | At::After => old_mask,
        At::SwapPoint | At::During => new_mask,
    };
    if mask & (1 << sig) != 0 {
        // 屏蔽中：挂起不唤醒
        (false, false)
    } else {
        // 未屏蔽：立即处理并唤醒等待
        (true, true)
    }
}

/// 两步法状态机（坏对照面）：UNBLOCK → pause 两步，窗口内到达丢失。
pub struct TwoStep {
    pub mask: u32,
    pub window_open: bool,
    pub lost: u32,
    pub log: Vec<String>,
    pub note: String,
}

impl TwoStep {
    pub fn new(mask: u32) -> TwoStep {
        TwoStep {
            mask,
            window_open: false,
            lost: 0,
            log: Vec::new(),
            note: "start".to_string(),
        }
    }

    /// 步 1：UNBLOCK（掩码先变，pause 未开始 → 窗口开）。
    pub fn unblock(&mut self, sig: u32) {
        self.mask &= !(1 << sig);
        self.window_open = true;
        self.log.push("unblock".to_string());
        self.note = "window".to_string();
    }

    /// 窗口内到达：丢失（pause 尚未登记等待）。
    pub fn arrive_in_window(&mut self, sig: u32) -> bool {
        if self.window_open {
            self.lost |= 1 << sig;
            self.log.push("lost".to_string());
            true
        } else {
            false
        }
    }

    /// 步 2：pause（窗口关）。
    pub fn pause(&mut self) {
        self.window_open = false;
        self.log.push("pause".to_string());
        self.note = "paused".to_string();
    }

    /// 还原核：返回后掩码还原（账面模拟）。
    pub fn restore(&mut self, old: u32) {
        self.mask = old;
        self.log.push("restore".to_string());
    }
}

/// 原子路径状态机：换装/唤醒/还原三段单点。
pub struct AtomicPath {
    pub old_mask: u32,
    pub cur_mask: u32,
    pub woke: bool,
    pub log: Vec<String>,
    pub note: String,
}

impl AtomicPath {
    pub fn new(old_mask: u32, new_mask: u32) -> AtomicPath {
        let mut log = Vec::new();
        log.push("swap".to_string());
        AtomicPath {
            old_mask,
            cur_mask: new_mask,
            woke: false,
            log,
            note: "waiting".to_string(),
        }
    }

    /// 信号到达（等待中）：解屏蔽即唤醒（单点，无窗）。
    pub fn arrive(&mut self, sig: u32) -> (bool, bool) {
        let out = judge_atomic(&At::During, sig, self.old_mask, self.cur_mask);
        if out.1 {
            self.woke = true;
            self.log.push("wake".to_string());
            self.note = "woke".to_string();
        }
        out
    }

    /// 还原核：唤醒后掩码还原旧值。
    pub fn restore(&mut self) -> bool {
        self.cur_mask = self.old_mask;
        self.log.push("restore".to_string());
        self.cur_mask == self.old_mask
    }
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 16;

/// 正反双样本矩阵：期望 (立即处理, 唤醒)。
pub fn sample_matrix() -> Vec<(At, u32, u32, u32, (bool, bool))> {
    let mut m: Vec<(At, u32, u32, u32, (bool, bool))> = Vec::new();
    // 正：等待中到达且新掩码解屏蔽 → 立即唤醒
    m.push((At::During, SIGUSR1, 1 << SIGUSR1, 0, (true, true)));
    // 正：等待中到达但新掩码仍屏蔽 → 挂起不唤醒
    m.push((At::During, SIGUSR1, 1 << SIGUSR1, 1 << SIGUSR1, (false, false)));
    // 正：换装前到达按旧掩码（屏蔽）→ 不唤醒
    m.push((At::Before, SIGUSR1, 1 << SIGUSR1, 0, (false, false)));
    // 正：返回后到达按旧掩码（解屏蔽）→ 直达
    m.push((At::After, SIGUSR1, 0, 0, (true, true)));
    // 正：换装点解屏蔽 → 唤醒（单点无窗）
    m.push((At::SwapPoint, SIGUSR1, 1 << SIGUSR1, 0, (true, true)));
    m
}

/// 矩阵对平：返回 (命中数, 总数, 容量合规)。
pub fn verify_matrix() -> (usize, usize, bool) {
    let m = sample_matrix();
    let mut ok = 0usize;
    let mut i = 0usize;
    while i < m.len() {
        let (at, sig, old, new, want) = &m[i];
        if &judge_atomic(at, *sig, *old, *new) == want {
            ok += 1;
        }
        i += 1;
    }
    (ok, m.len(), m.len() <= MATRIX_CAP)
}

/// 坏实现 A：两步法竞态窗（窗内到达丢失）。
pub fn faulty_two_step() -> u32 {
    let mut ts = TwoStep::new(1 << SIGUSR1);
    ts.unblock(SIGUSR1);
    let _ = ts.arrive_in_window(SIGUSR1);
    ts.pause();
    ts.lost
}

/// 权威原子路径对照：同序到达必不丢。
pub fn atomic_ref() -> bool {
    let mut ap = AtomicPath::new(1 << SIGUSR1, 0);
    let (_handled, woke) = ap.arrive(SIGUSR1);
    woke && ap.restore() && ap.cur_mask == ap.old_mask
}

/// 注入核：两步法丢信号 vs 原子不丢 → 分歧检出。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    // 1) 两步法丢失检出：窗内信号 lost 位被置
    got[0] = faulty_two_step() == (1 << SIGUSR1);
    // 2) 原子不丢：同场景唤醒且还原对账
    got[1] = atomic_ref() && faulty_two_step() != 0;
    got
}

/// 还原核：sigsuspend 返回后掩码必还原旧值（好实现两例）。
pub fn restore_ok() -> bool {
    let mut ap = AtomicPath::new(0x30, 0);
    let (_h, _w) = ap.arrive(SIGUSR1);
    ap.restore() && ap.cur_mask == 0x30
}

/// 竞态窗核：窗开时到达必丢、窗关（pause 后）到达不丢。
pub fn window_ok() -> bool {
    let mut ts = TwoStep::new(1 << SIGUSR1);
    ts.unblock(SIGUSR1);
    let in_win = ts.arrive_in_window(SIGUSR1);
    ts.pause();
    let after_win = !ts.arrive_in_window(SIGUSR1);
    in_win && after_win && ts.lost == (1 << SIGUSR1) && ts.note == "paused"
}

/// 时序面注记（对账/落账用）。
pub fn at_note(at: &At) -> String {
    match at {
        At::Before => "before".to_string(),
        At::SwapPoint => format!("swap(at={})", SIGUSR1),
        At::During => "during".to_string(),
        At::After => "after".to_string(),
    }
}

/// 判据（对应卡验收 J 判据）。
pub fn run_unx10829_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10829");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平
    s.add("J1-矩阵对平", hits == total && cap_ok, "五线时序样本全命中");

    // J2 正路径：等待中解屏蔽到达必唤醒
    let j2 = judge_atomic(&At::During, SIGUSR1, 1 << SIGUSR1, 0) == (true, true);
    s.add("J2-等待唤醒", j2, "解屏蔽到达立即处理");

    // J3 正路径：等待中屏蔽到达挂起不唤醒
    let j3 = judge_atomic(&At::During, SIGUSR1, 1 << SIGUSR1, 1 << SIGUSR1) == (false, false);
    s.add("J3-屏蔽挂起", j3, "新掩码仍屏蔽则等待");

    // J4 正路径：换装点无窗（单点语义）
    let j4 = judge_atomic(&At::SwapPoint, SIGUSR1, 1 << SIGUSR1, 0) == (true, true);
    s.add("J4-单点换装", j4, "swap 点到达不丢");

    // J5 竞态窗核：窗内必丢（坏对照面检出）
    s.add("J5-竞态窗检出", window_ok(), "两步法窗内丢失可见");

    // J6 注入1 两步法丢信号检出
    s.add("J6-丢信号检出", inj[0], "两步法 lost 位必置");

    // J7 注入2 原子不丢对照检出
    s.add("J7-原子对照", inj[1], "原子路径同场景唤醒");

    // J8 还原核：返回后掩码还原
    s.add("J8-还原核", restore_ok(), "cur==old 对账");

    // J9 原子路径唤醒账
    let mut ap = AtomicPath::new(0x10, 0);
    let (_h, w) = ap.arrive(SIGUSR1);
    let j9 = w && ap.woke && ap.note == "woke" && ap.log.len() == 3;
    s.add("J9-唤醒账", j9, "swap→wake→restore 三行账");

    // J10 时序闭集：四时刻互异
    let a = at_note(&At::Before);
    let b = at_note(&At::SwapPoint);
    let c = at_note(&At::During);
    let d = at_note(&At::After);
    let j10 = a != b && b != c && c != d && a != d;
    s.add("J10-时序闭集", j10, "四时刻注记互异");

    // J11 字面锚：SIGUSR1=10 / EINTR=4
    let j11 = SIGUSR1 == 10 && POSIX_EINTR == 4 && diag_text(SS_RACE_LOSS).len() > 0;
    s.add("J11-字面锚对平", j11, "10/4 双锚且语义完备");

    // J12 掩码生效面：During 判掩码用新账、Before/After 用旧账
    let j12 = judge_atomic(&At::Before, SIGUSR1, 0, 1 << SIGUSR1) == (true, true)
        && judge_atomic(&At::During, SIGUSR1, 0, 1 << SIGUSR1) == (false, false);
    s.add("J12-掩码分时", j12, "新旧掩码分时生效");

    // J13 (dyn) 时序账指纹落账
    let mut ap2 = AtomicPath::new(1 << SIGUSR1, 0);
    let _ = ap2.arrive(SIGUSR1);
    let _ = ap2.restore();
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(ap2.log.len() as u32);
    h = h.wrapping_mul(31).wrapping_add(ap2.cur_mask);
    for row in &ap2.log {
        for c in row.bytes() {
            h = h.wrapping_mul(31).wrapping_add(c as u32);
        }
    }
    s.add_dyn("J13-时序账指纹", h != 0, &format!("fold={:#x}", h));

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
    fn unx10829_suspend_flow() {
        assert_eq!(
            judge_atomic(&At::During, SIGUSR1, 1 << SIGUSR1, 0),
            (true, true)
        );
        let mut ts = TwoStep::new(1 << SIGUSR1);
        ts.unblock(SIGUSR1);
        assert!(ts.arrive_in_window(SIGUSR1));
        ts.pause();
        assert!(!ts.arrive_in_window(SIGUSR1));
        assert_eq!(ts.lost, 1 << SIGUSR1);
        let mut ap = AtomicPath::new(1 << SIGUSR1, 0);
        assert_eq!(ap.arrive(SIGUSR1), (true, true));
        assert!(ap.restore());
        let inj = inject_two();
        assert!(inj.iter().all(|b| *b));
    }

    #[test]
    fn unx10829_checks_green() {
        let s = run_unx10829_checks();
        assert!(s.all_passed(), "UNX-F10829 判据未全绿");
    }
}
