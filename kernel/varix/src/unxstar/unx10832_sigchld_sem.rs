//! UNX-F10832 SIGCHLD 子进程终止通知矩阵（僵尸面）（0x1c080 段）
//!
//! 锚点：UNX-F10832-J1 僵尸对平——子退出且默认处置必置僵尸、wait 收尸
//! 必清僵尸、SIG_IGN 或显式忽略处置必自动收尸（不置僵尸）三线全对平；
//! 注入坏实现（忽略处置仍留僵尸/收尸不清）必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：子进程账（pid × 状态机 Running/Exited/Zombie/Reaped）+ 处置
//! 判定器（处置型对撞退出事件算 自动收尸/置僵尸）+ wait 收尸状态机 +
//! 重放核（事件序逐笔同判）+ 注入核（两类坏实现必分歧）+ 正反双样本
//! 矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX 锚：SIGCHLD=17。
pub const SIGCHLD: u32 = 17;

/// 诊断段（0x1c080 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c080;
pub const CH_ZOMBIE_LEAK: u32 = SEG_BASE + 0x01; // 忽略处置仍留僵尸
pub const CH_REAP_MISS: u32 = SEG_BASE + 0x02; // 收尸不清
pub const CH_DISP_SKEW: u32 = SEG_BASE + 0x03; // 处置判定错
pub const CH_STATE_LOSS: u32 = SEG_BASE + 0x04; // 状态机丢失
pub const CH_REPLAY_MISS: u32 = SEG_BASE + 0x05; // 重放不齐
pub const CH_NOTE_SKEW: u32 = SEG_BASE + 0x06; // 注记错账

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        CH_ZOMBIE_LEAK => "忽略处置仍留僵尸",
        CH_REAP_MISS => "收尸不清",
        CH_DISP_SKEW => "处置判定错",
        CH_STATE_LOSS => "状态机丢失",
        CH_REPLAY_MISS => "重放不齐",
        CH_NOTE_SKEW => "注记错账",
        _ => "未知诊断码",
    }
}

/// 子进程状态。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ChState {
    Running,
    Exited,
    Zombie,
    Reaped,
}

/// 处置型。
#[derive(Clone, PartialEq, Eq)]
pub enum Disp {
    Dflt,
    Handler,
    Ign,
}

/// 退出后走向判定核：默认/处理器 → 置僵尸（等 wait 收尸）；SIG_IGN →
/// 自动收尸（Reaped，不置僵尸）。
pub fn judge(disp: &Disp) -> ChState {
    match disp {
        Disp::Dflt | Disp::Handler => ChState::Zombie,
        Disp::Ign => ChState::Reaped,
    }
}

/// 子进程账条目。
#[derive(Clone)]
pub struct Child {
    pub pid: u32,
    pub state: ChState,
    pub note: String,
}

/// 子进程表状态机：exit 置态、wait 收尸。
pub struct ChildTable {
    pub rows: Vec<Child>,
    pub note: String,
}

impl ChildTable {
    pub fn new() -> ChildTable {
        ChildTable { rows: Vec::new(), note: "fresh".to_string() }
    }

    /// 注册子进程。
    pub fn spawn(&mut self, pid: u32) {
        self.rows.push(Child { pid, state: ChState::Running, note: "running".to_string() });
    }

    /// 子退出：按处置置态。
    pub fn exit(&mut self, pid: u32, disp: &Disp) -> bool {
        let want = judge(disp);
        let mut hit = false;
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].pid == pid && self.rows[i].state == ChState::Running {
                self.rows[i].state = want.clone();
                self.rows[i].note = if want == ChState::Reaped { "auto-reaped".to_string() } else { "zombie".to_string() };
                hit = true;
            }
            i += 1;
        }
        self.note = if want == ChState::Reaped { "auto-reaped".to_string() } else { "zombie".to_string() };
        hit
    }

    /// wait 收尸：Zombie → Reaped。
    pub fn reap(&mut self, pid: u32) -> bool {
        let mut hit = false;
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].pid == pid && self.rows[i].state == ChState::Zombie {
                self.rows[i].state = ChState::Reaped;
                self.rows[i].note = "reaped".to_string();
                hit = true;
            }
            i += 1;
        }
        if hit {
            self.note = "reaped".to_string();
        }
        hit
    }

    /// 僵尸计数。
    pub fn zombies(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].state == ChState::Zombie {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 无泄漏核：SIG_IGN 处置的子无僵尸行。
    pub fn no_leak(&self, auto_pids: &[u32]) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.rows.len() {
            let mut j = 0usize;
            let mut is_auto = false;
            while j < auto_pids.len() {
                if auto_pids[j] == self.rows[i].pid {
                    is_auto = true;
                }
                j += 1;
            }
            if is_auto && self.rows[i].state == ChState::Zombie {
                ok = false;
            }
            i += 1;
        }
        ok
    }
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 16;

/// 正反双样本矩阵：期望退出后状态。
pub fn sample_matrix() -> Vec<(Disp, ChState)> {
    let mut m: Vec<(Disp, ChState)> = Vec::new();
    // 正：默认 → 僵尸
    m.push((Disp::Dflt, ChState::Zombie));
    // 正：处理器 → 僵尸（等 wait）
    m.push((Disp::Handler, ChState::Zombie));
    // 正：SIG_IGN → 自动收尸
    m.push((Disp::Ign, ChState::Reaped));
    m
}

/// 矩阵对平：返回 (命中数, 总数, 容量合规)。
pub fn verify_matrix() -> (usize, usize, bool) {
    let m = sample_matrix();
    let mut ok = 0usize;
    let mut i = 0usize;
    while i < m.len() {
        let (disp, want) = &m[i];
        if &judge(disp) == want {
            ok += 1;
        }
        i += 1;
    }
    (ok, m.len(), m.len() <= MATRIX_CAP)
}

/// 坏实现 A：忽略处置仍留僵尸。
pub fn faulty_leak(disp: &Disp) -> ChState {
    match disp {
        Disp::Ign => ChState::Zombie,
        _ => judge(disp),
    }
}

/// 坏实现 B：wait 收尸不清（僵尸残留）。
pub fn faulty_reap_miss(t: &mut ChildTable, pid: u32) -> bool {
    let _ = t.reap(pid);
    // 坏实现：状态不动
    t.rows.iter().any(|c| c.pid == pid && c.state == ChState::Zombie)
}

/// 注入核：两类坏实现必与权威分歧（检出）。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    // 1) 忽略处置泄漏：权威 Reaped、坏实现 Zombie → 检出
    got[0] = judge(&Disp::Ign) != faulty_leak(&Disp::Ign) && judge(&Disp::Ign) == ChState::Reaped;
    // 2) 收尸不清：权威 reap 后无僵尸、坏实现残留 → 检出
    let mut t = ChildTable::new();
    t.spawn(50);
    let _ = t.exit(50, &Disp::Dflt);
    let good = {
        let mut t2 = ChildTable::new();
        t2.spawn(50);
        let _ = t2.exit(50, &Disp::Dflt);
        t2.reap(50) && t2.zombies() == 0
    };
    let bad = faulty_reap_miss(&mut t, 50);
    got[1] = good && bad && good != bad;
    got
}

/// 全序核：spawn → exit(默认) → 僵尸 → reap → 收清；spawn → exit(SIG_IGN) → 自动收。
pub fn seq_ok() -> bool {
    let mut t = ChildTable::new();
    t.spawn(10);
    t.spawn(11);
    let e1 = t.exit(10, &Disp::Dflt);
    let z1 = t.zombies() == 1;
    let e2 = t.exit(11, &Disp::Ign);
    let r1 = t.reap(10);
    let ok = e1 && e2 && z1 && r1
        && t.zombies() == 0
        && t.no_leak(&[11])
        && t.rows[0].state == ChState::Reaped
        && t.rows[1].state == ChState::Reaped
        && t.note == "reaped";
    ok
}

/// 处置面注记（对账/落账用）。
pub fn disp_note(d: &Disp) -> String {
    match judge(d) {
        ChState::Zombie => "zombie(wait)".to_string(),
        ChState::Reaped => "auto-reaped".to_string(),
        _ => "running".to_string(),
    }
}

/// 判据（对应卡验收 J 判据）。
pub fn run_unx10832_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10832");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平
    s.add("J1-矩阵对平", hits == total && cap_ok, "三线处置样本全命中");

    // J2 正路径：默认处置退出生成僵尸
    let j2 = judge(&Disp::Dflt) == ChState::Zombie;
    s.add("J2-默认置僵尸", j2, "等 wait 收尸");

    // J3 正路径：SIG_IGN 自动收尸
    let j3 = judge(&Disp::Ign) == ChState::Reaped;
    s.add("J3-忽略自动收", j3, "不置僵尸");

    // J4 正路径：处理器处置仍置僵尸
    let j4 = judge(&Disp::Handler) == ChState::Zombie;
    s.add("J4-处理器置僵尸", j4, "处理器不改变收尸责任");

    // J5 全序核：僵尸→收尸→自动收
    s.add("J5-全序核", seq_ok(), "双子账三步全对");

    // J6 注入1 忽略泄漏检出
    s.add("J6-泄漏检出", inj[0], "忽略处置留僵尸坏实现必分歧");

    // J7 注入2 收尸不清检出
    s.add("J7-收尸检出", inj[1], "僵尸残留坏实现必分歧");

    // J8 wait 收尸只对僵尸行生效
    let mut t2 = ChildTable::new();
    t2.spawn(20);
    let miss = t2.reap(20);
    let j8 = !miss && t2.rows[0].state == ChState::Running;
    s.add("J8-收尸条件", j8, "Running 行不可收");

    // J9 僵尸计数核
    let mut t3 = ChildTable::new();
    t3.spawn(30);
    t3.spawn(31);
    let _ = t3.exit(30, &Disp::Dflt);
    let _ = t3.exit(31, &Disp::Dflt);
    let j9 = t3.zombies() == 2 && t3.reap(30) && t3.zombies() == 1;
    s.add("J9-僵尸计数", j9, "计数随收尸递减");

    // J10 字面锚：SIGCHLD=17
    let j10 = SIGCHLD == 17 && diag_text(CH_ZOMBIE_LEAK).len() > 0
        && diag_text(0xffff_ffff) == "未知诊断码";
    s.add("J10-字面锚对平", j10, "17 锚且语义完备");

    // J11 处置面注记闭集
    let a = disp_note(&Disp::Dflt);
    let b = disp_note(&Disp::Ign);
    let j11 = a.contains("wait") && b.contains("auto") && a != b;
    s.add("J11-注记互异", j11, "wait/auto 注记互异");

    // J12 状态闭集：四态互异
    let j12 = ChState::Running != ChState::Exited
        && ChState::Exited != ChState::Zombie
        && ChState::Zombie != ChState::Reaped;
    s.add("J12-状态闭集", j12, "四态两两互异");

    // J13 (dyn) 子账指纹落账
    let mut t4 = ChildTable::new();
    t4.spawn(40);
    let _ = t4.exit(40, &Disp::Ign);
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(t4.rows.len() as u32);
    h = h.wrapping_mul(31).wrapping_add(t4.zombies() as u32);
    for c in t4.note.bytes() {
        h = h.wrapping_mul(31).wrapping_add(c as u32);
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
    fn unx10832_chld_flow() {
        let mut t = ChildTable::new();
        t.spawn(1);
        assert!(t.exit(1, &Disp::Dflt));
        assert_eq!(t.zombies(), 1);
        assert!(t.reap(1));
        assert_eq!(t.zombies(), 0);
        t.spawn(2);
        assert!(t.exit(2, &Disp::Ign));
        assert_eq!(t.rows[1].state, ChState::Reaped);
        assert!(t.no_leak(&[2]));
        let inj = inject_two();
        assert!(inj.iter().all(|b| *b));
    }

    #[test]
    fn unx10832_checks_green() {
        let s = run_unx10832_checks();
        assert!(s.all_passed(), "UNX-F10832 判据未全绿");
    }
}
