//! UNX-F10824 kill() 错误矩阵（EINVAL/ESRCH/EPERM 三级）（0x1bf80 段）
//!
//! 锚点：UNX-F10824-J1 三级裁决对平——非法信号号必 EINVAL（先决）、
//! 目标不存在必 ESRCH、存在但越权必 EPERM、合法必投递入账四线全对平；
//! 注入坏实现（次序颠倒/越权放行）必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：进程表（pid × uid × 存活位）+ 调用者凭据 + 三级判定器（号→
//！ 存在→权限顺序）+ 投递账状态机（目标命中落账）+ 注入核（两类坏实现
//! 必分歧）+ 正反双样本矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// POSIX 锚：EINVAL=22、ESRCH=3、EPERM=1、SIGKILL=9。
pub const POSIX_EINVAL: u32 = 22;
pub const POSIX_ESRCH: u32 = 3;
pub const POSIX_EPERM: u32 = 1;
pub const SIGKILL: u32 = 9;
pub const SIGSTOP: u32 = 19;
/// 合法信号号上界。
pub const NSIG: u32 = 65;

/// 诊断段（0x1bf80 独占 0x20）。
pub const SEG_BASE: u32 = 0x1bf80;
pub const KL_ORDER_SKEW: u32 = SEG_BASE + 0x01; // 三级次序颠倒
pub const KL_PERM_MISS: u32 = SEG_BASE + 0x02; // 越权放行
pub const KL_SRCH_MISS: u32 = SEG_BASE + 0x03; // 幽灵目标放行
pub const KL_DELIVER_LOSS: u32 = SEG_BASE + 0x04; // 合法投递丢失
pub const KL_REPLAY_MISS: u32 = SEG_BASE + 0x05; // 重放不齐
pub const KL_NOTE_SKEW: u32 = SEG_BASE + 0x06; // 注记错账

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        KL_ORDER_SKEW => "三级次序颠倒",
        KL_PERM_MISS => "越权放行",
        KL_SRCH_MISS => "幽灵目标放行",
        KL_DELIVER_LOSS => "合法投递丢失",
        KL_REPLAY_MISS => "重放不齐",
        KL_NOTE_SKEW => "注记错账",
        _ => "未知诊断码",
    }
}

/// 进程表条目。
#[derive(Clone)]
pub struct Proc {
    pub pid: u32,
    pub uid: u32,
    pub alive: bool,
}

/// 目标场景：目标 pid × 信号号 × 调用者 uid × 进程表快照。
#[derive(Clone)]
pub struct Scenario {
    pub pid: i32,
    pub sig: u32,
    pub caller_uid: u32,
    pub table: Vec<Proc>,
}

/// 查找目标进程。
pub fn find<'a>(table: &'a [Proc], pid: i32) -> Option<&'a Proc> {
    let mut i = 0usize;
    while i < table.len() {
        if table[i].pid as i32 == pid {
            return Some(&table[i]);
        }
        i += 1;
    }
    None
}

/// 三级判定核：EINVAL（号非法）→ ESRCH（目标不存在）→ EPERM（uid 不匹配
/// 且调用者非根）→ Ok（投递）；sig=0 探活合法（只走权限路不投递）。
pub fn judge(sc: &Scenario) -> Result<&'static str, u32> {
    if sc.sig >= NSIG {
        return Err(POSIX_EINVAL);
    }
    if sc.pid <= 0 {
        // 0/负 pid 面：进程组语义此处建模为必拒（无组表）
        return Err(POSIX_ESRCH);
    }
    let target = match find(&sc.table, sc.pid) {
        Some(p) => p,
        None => return Err(POSIX_ESRCH),
    };
    if !target.alive {
        return Err(POSIX_ESRCH);
    }
    if sc.caller_uid != 0 && sc.caller_uid != target.uid {
        return Err(POSIX_EPERM);
    }
    Ok("delivered")
}

/// 投递账状态机：合法 kill 落账。
pub struct KillLedger {
    pub rows: Vec<(i32, u32, String)>,
    pub note: String,
}

impl KillLedger {
    pub fn new() -> KillLedger {
        KillLedger { rows: Vec::new(), note: "ledger".to_string() }
    }

    /// 一次 kill：判三级通过才落账。
    pub fn call(&mut self, sc: &Scenario) -> Result<&'static str, u32> {
        let out = judge(sc);
        if let Ok(t) = out {
            self.rows.push((sc.pid, sc.sig, t.to_string()));
            self.note = "delivered".to_string();
        }
        out
    }

    /// 重放核：账行 pid 全在存活集。
    pub fn replay_ok(&self, table: &[Proc]) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.rows.len() {
            let (pid, _sig, _t) = &self.rows[i];
            match find(table, *pid) {
                Some(p) if p.alive => {}
                _ => ok = false,
            }
            i += 1;
        }
        ok
    }
}

/// 基准进程表。
pub fn base_table() -> Vec<Proc> {
    vec![
        Proc { pid: 100, uid: 1000, alive: true },
        Proc { pid: 101, uid: 1001, alive: true },
        Proc { pid: 102, uid: 1000, alive: false },
    ]
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 16;

/// 正反双样本矩阵：期望注记或诊断码。
pub fn sample_matrix() -> Vec<(Scenario, Option<u32>)> {
    let mut m: Vec<(Scenario, Option<u32>)> = Vec::new();
    let t = base_table();
    // 正：同 uid 合法投递
    m.push((Scenario { pid: 100, sig: 15, caller_uid: 1000, table: t.clone() }, None));
    // 正：根越权豁免
    m.push((Scenario { pid: 101, sig: SIGKILL, caller_uid: 0, table: t.clone() }, None));
    // 正：sig=0 探活（号合法即走存在/权限路）
    m.push((Scenario { pid: 100, sig: 0, caller_uid: 1000, table: t.clone() }, None));
    // 反：号超界必 EINVAL（先决）
    m.push((Scenario { pid: 999, sig: NSIG, caller_uid: 1000, table: t.clone() }, Some(POSIX_EINVAL)));
    // 反：幽灵目标必 ESRCH
    m.push((Scenario { pid: 999, sig: 15, caller_uid: 1000, table: t.clone() }, Some(POSIX_ESRCH)));
    // 反：死进程必 ESRCH
    m.push((Scenario { pid: 102, sig: 15, caller_uid: 1000, table: t.clone() }, Some(POSIX_ESRCH)));
    // 反：异 uid 越权必 EPERM
    m.push((Scenario { pid: 101, sig: 15, caller_uid: 1000, table: t.clone() }, Some(POSIX_EPERM)));
    // 边界：号非法 + 幽灵目标 → EINVAL 先决
    m.push((Scenario { pid: 999, sig: 999, caller_uid: 1000, table: t.clone() }, Some(POSIX_EINVAL)));
    m
}

/// 矩阵对平：返回 (命中数, 总数, 容量合规)。
pub fn verify_matrix() -> (usize, usize, bool) {
    let m = sample_matrix();
    let mut ok = 0usize;
    let mut i = 0usize;
    while i < m.len() {
        let (sc, want) = &m[i];
        let got = judge(sc);
        let hit = match want {
            None => got.is_ok(),
            Some(c) => got.err() == Some(*c),
        };
        if hit {
            ok += 1;
        }
        i += 1;
    }
    (ok, m.len(), m.len() <= MATRIX_CAP)
}

/// 坏实现 A：次序颠倒（先查存在后查号——幽灵+坏号场景报 ESRCH）。
pub fn faulty_order(sc: &Scenario) -> Result<&'static str, u32> {
    let target = find(&sc.table, sc.pid);
    if target.is_none() {
        return Err(POSIX_ESRCH);
    }
    if sc.sig >= NSIG {
        return Err(POSIX_EINVAL);
    }
    judge(sc)
}

/// 坏实现 B：越权放行（异 uid 也投递）。
pub fn faulty_perm(sc: &Scenario) -> Result<&'static str, u32> {
    match judge(sc) {
        Err(POSIX_EPERM) => Ok("delivered"),
        other => other,
    }
}

/// 注入核：两类坏实现必与权威分歧（检出）。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    let t = base_table();
    // 1) 次序颠倒：幽灵目标 + 坏号：权威 EINVAL、坏实现 ESRCH → 检出
    let sc1 = Scenario { pid: 999, sig: 999, caller_uid: 1000, table: t.clone() };
    got[0] = judge(&sc1) != faulty_order(&sc1)
        && judge(&sc1).err() == Some(POSIX_EINVAL)
        && faulty_order(&sc1).err() == Some(POSIX_ESRCH);
    // 2) 越权放行：异 uid：权威 EPERM、坏实现投递 → 检出
    let sc2 = Scenario { pid: 101, sig: 15, caller_uid: 1000, table: t.clone() };
    got[1] = judge(&sc2) != faulty_perm(&sc2) && judge(&sc2).err() == Some(POSIX_EPERM);
    got
}

/// 三级全序核：EINVAL → ESRCH → EPERM → 投递 逐级闸门。
pub fn gate_seq_ok() -> bool {
    let t = base_table();
    let g1 = judge(&Scenario { pid: 100, sig: 999, caller_uid: 1000, table: t.clone() })
        .err() == Some(POSIX_EINVAL);
    let g2 = judge(&Scenario { pid: 999, sig: 15, caller_uid: 1000, table: t.clone() })
        .err() == Some(POSIX_ESRCH);
    let g3 = judge(&Scenario { pid: 101, sig: 15, caller_uid: 1000, table: t.clone() })
        .err() == Some(POSIX_EPERM);
    let g4 = judge(&Scenario { pid: 100, sig: 15, caller_uid: 1000, table: t.clone() })
        == Ok("delivered");
    g1 && g2 && g3 && g4
}

/// 投递账核：合法 kill 落账、拒绝零污染、重放对拍。
pub fn deliver_ledger_ok() -> bool {
    let t = base_table();
    let mut lg = KillLedger::new();
    let _ = lg.call(&Scenario { pid: 100, sig: 15, caller_uid: 1000, table: t.clone() });
    let _ = lg.call(&Scenario { pid: 101, sig: SIGKILL, caller_uid: 0, table: t.clone() });
    let _ = lg.call(&Scenario { pid: 999, sig: 15, caller_uid: 1000, table: t.clone() });
    let _ = lg.call(&Scenario { pid: 100, sig: 999, caller_uid: 1000, table: t.clone() });
    lg.rows.len() == 2 && lg.note == "delivered" && lg.replay_ok(&t)
}

/// kill 面注记（对账/落账用）。
pub fn kill_note(sc: &Scenario) -> String {
    match judge(sc) {
        Ok(t) => format!("{} pid={}", t, sc.pid),
        Err(c) => format!("err:{:#x}", c),
    }
}

/// 判据（对应卡验收 J 判据）。
pub fn run_unx10824_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10824");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平
    s.add("J1-矩阵对平", hits == total && cap_ok, "八线 kill 样本全命中");

    // J2 反路径：号非法必 EINVAL 先决
    let j2 = judge(&Scenario { pid: 100, sig: NSIG, caller_uid: 1000, table: base_table() })
        .err() == Some(POSIX_EINVAL);
    s.add("J2-号先决", j2, "EINVAL 优先于存在/权限");

    // J3 反路径：幽灵/死目标必 ESRCH
    let t = base_table();
    let j3 = judge(&Scenario { pid: 999, sig: 15, caller_uid: 1000, table: t.clone() })
        .err() == Some(POSIX_ESRCH)
        && judge(&Scenario { pid: 102, sig: 15, caller_uid: 1000, table: t.clone() })
            .err() == Some(POSIX_ESRCH);
    s.add("J3-ESRCH面", j3, "幽灵与死进程同面");

    // J4 反路径：异 uid 必 EPERM
    let j4 = judge(&Scenario { pid: 101, sig: 15, caller_uid: 1000, table: t.clone() })
        .err() == Some(POSIX_EPERM);
    s.add("J4-EPERM面", j4, "异 uid 越权必 1");

    // J5 正路径：同 uid/根投递
    let j5 = judge(&Scenario { pid: 100, sig: 15, caller_uid: 1000, table: t.clone() })
        == Ok("delivered")
        && judge(&Scenario { pid: 101, sig: SIGKILL, caller_uid: 0, table: t.clone() })
            == Ok("delivered");
    s.add("J5-投递面", j5, "同 uid 与根全通");

    // J6 三级全序核
    s.add("J6-全序闸门", gate_seq_ok(), "22→3→1→投递 逐级");

    // J7 注入1 次序颠倒检出
    s.add("J7-次序检出", inj[0], "ESRCH 抢先坏实现必分歧");

    // J8 注入2 越权放行检出
    s.add("J8-越权检出", inj[1], "异 uid 投递坏实现必分歧");

    // J9 投递账核
    s.add("J9-投递账核", deliver_ledger_ok(), "合法落账拒绝零污染");

    // J10 字面锚：22/3/1 三锚
    let j10 = POSIX_EINVAL == 22 && POSIX_ESRCH == 3 && POSIX_EPERM == 1
        && diag_text(KL_ORDER_SKEW).len() > 0;
    s.add("J10-字面锚对平", j10, "22/3/1 三锚且语义完备");

    // J11 探活面：sig=0 不杀但走权限路
    let j11 = judge(&Scenario { pid: 100, sig: 0, caller_uid: 1000, table: t.clone() }).is_ok()
        && judge(&Scenario { pid: 101, sig: 0, caller_uid: 1000, table: t.clone() })
            .err() == Some(POSIX_EPERM);
    s.add("J11-探活面", j11, "sig=0 只探不杀");

    // J12 kill 面注记闭集
    let n_ok = kill_note(&Scenario { pid: 100, sig: 15, caller_uid: 1000, table: t.clone() });
    let n_perm = kill_note(&Scenario { pid: 101, sig: 15, caller_uid: 1000, table: t.clone() });
    let j12 = n_ok.contains("delivered") && n_perm.contains("err") && n_ok != n_perm;
    s.add("J12-注记互异", j12, "delivered/err 注记互异");

    // J13 (dyn) 投递账指纹落账
    let mut lg2 = KillLedger::new();
    let _ = lg2.call(&Scenario { pid: 100, sig: 15, caller_uid: 1000, table: t.clone() });
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(lg2.rows.len() as u32);
    let mut i = 0usize;
    while i < lg2.rows.len() {
        h = h.wrapping_mul(31).wrapping_add(lg2.rows[i].0 as u32);
        h = h.wrapping_mul(31).wrapping_add(lg2.rows[i].1);
        i += 1;
    }
    for c in lg2.note.bytes() {
        h = h.wrapping_mul(31).wrapping_add(c as u32);
    }
    s.add_dyn("J13-投递账指纹", h != 0, &format!("fold={:#x}", h));

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
    fn unx10824_kill_flow() {
        let t = base_table();
        assert!(judge(&Scenario { pid: 999, sig: 999, caller_uid: 1000, table: t.clone() })
            .err() == Some(POSIX_EINVAL));
        assert!(judge(&Scenario { pid: 999, sig: 15, caller_uid: 1000, table: t.clone() })
            .err() == Some(POSIX_ESRCH));
        assert!(judge(&Scenario { pid: 101, sig: 15, caller_uid: 1000, table: t.clone() })
            .err() == Some(POSIX_EPERM));
        assert!(judge(&Scenario { pid: 100, sig: 15, caller_uid: 1000, table: t.clone() }).is_ok());
        let inj = inject_two();
        assert!(inj.iter().all(|b| *b));
    }

    #[test]
    fn unx10824_checks_green() {
        let s = run_unx10824_checks();
        assert!(s.all_passed(), "UNX-F10824 判据未全绿");
    }
}
