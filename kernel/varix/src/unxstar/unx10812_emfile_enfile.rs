//! UNX-F10812 EMFILE/ENFILE 耗尽矩阵（进程/全系统两级）（0x1be00 段）
//!
//! 锚点：UNX-F10812-J1 两级配额对平——进程级配额满必 EMFILE 且先于全局
//! 检查、全局配额满必 ENFILE、双限皆空必放行且两级计数同步推进三线全
//! 对平；注入坏实现（检查次序颠倒/全局计数漏推进）必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：两级配额账（进程级 used/cap × 全局 used/cap）+ 获取判定器
//! （先进程后全局两级算 EMFILE/ENFILE/放行）+ 释放钳零 + 生命周期核
//! （open/close 计数对账）+ 注入核（两类坏实现必分歧）+ 正反双样本矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX 锚：EMFILE=24、ENFILE=23。
pub const POSIX_EMFILE: u32 = 24;
pub const POSIX_ENFILE: u32 = 23;

/// 诊断段（0x1be00 独占 0x20）。
pub const SEG_BASE: u32 = 0x1be00;
pub const EM_PROC_FIRST: u32 = SEG_BASE + 0x01; // 进程级检查缺失（先查全局）
pub const EM_GLOB_MISS: u32 = SEG_BASE + 0x02; // 全局计数漏推进
pub const EM_REL_OVER: u32 = SEG_BASE + 0x03; // 释放超发（计数下溢）
pub const EM_ALLOC_GHOST: u32 = SEG_BASE + 0x04; // 拒绝后误占配额
pub const EM_ORDER_SKEW: u32 = SEG_BASE + 0x05; // 两级判定次序错
pub const EM_PROBE_SKEW: u32 = SEG_BASE + 0x06; // 探针账与计数错账

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        EM_PROC_FIRST => "进程级检查缺失",
        EM_GLOB_MISS => "全局计数漏推进",
        EM_REL_OVER => "释放超发（下溢）",
        EM_ALLOC_GHOST => "拒绝后误占配额",
        EM_ORDER_SKEW => "两级判定次序错",
        EM_PROBE_SKEW => "探针账错账",
        _ => "未知诊断码",
    }
}

/// 场景输入：两级占用与两级上限。
#[derive(Clone)]
pub struct Scenario {
    pub proc_used: u32,
    pub proc_cap: u32,
    pub glob_used: u32,
    pub glob_cap: u32,
}

/// 获取判定核：进程级满 → EMFILE（先查）；全局级满 → ENFILE（后查）。
pub fn judge(sc: &Scenario) -> Result<(), u32> {
    if sc.proc_used >= sc.proc_cap {
        return Err(POSIX_EMFILE);
    }
    if sc.glob_used >= sc.glob_cap {
        return Err(POSIX_ENFILE);
    }
    Ok(())
}

/// 两级配额状态机：获取成功两级同步推进，拒绝不占位，释放钳零。
pub struct Quota {
    pub proc_used: u32,
    pub proc_cap: u32,
    pub glob_used: u32,
    pub glob_cap: u32,
    pub note: String,
}

impl Quota {
    pub fn new(proc_cap: u32, glob_cap: u32) -> Quota {
        Quota {
            proc_used: 0,
            proc_cap,
            glob_used: 0,
            glob_cap,
            note: "fresh".to_string(),
        }
    }

    /// 获取：按判定核推进，拒绝零占位。
    pub fn acquire(&mut self) -> Result<(), u32> {
        let sc = Scenario {
            proc_used: self.proc_used,
            proc_cap: self.proc_cap,
            glob_used: self.glob_used,
            glob_cap: self.glob_cap,
        };
        let out = judge(&sc);
        if out.is_ok() {
            self.proc_used += 1;
            self.glob_used += 1;
            self.note = "acquired".to_string();
        } else {
            self.note = "denied".to_string();
        }
        out
    }

    /// 释放：两级同步回退，下限钳零。
    pub fn release(&mut self) {
        if self.proc_used > 0 {
            self.proc_used -= 1;
        }
        if self.glob_used > 0 {
            self.glob_used -= 1;
        }
        self.note = "released".to_string();
    }
}

/// 坏实现 A：判定次序颠倒（先查全局）。
pub fn faulty_order(sc: &Scenario) -> Result<(), u32> {
    if sc.glob_used >= sc.glob_cap {
        return Err(POSIX_ENFILE);
    }
    if sc.proc_used >= sc.proc_cap {
        return Err(POSIX_EMFILE);
    }
    Ok(())
}

/// 坏实现 B：获取成功漏推全局计数（全局满判定永不触发）。
pub fn faulty_glob() -> bool {
    // 模拟：坏实现只看进程级（进程未满即放行）→ 全局满场景漏判
    let mut ok = true;
    let mut i = 0;
    while i < 4 {
        let sc = Scenario {
            proc_used: i as u32,
            proc_cap: 8,
            glob_used: i as u32,
            glob_cap: 2,
        };
        // i>=2 时全局满（cap=2），权威必 ENFILE
        if i >= 2 && judge(&sc).err() != Some(POSIX_ENFILE) {
            ok = false;
        }
        i += 1;
    }
    ok
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 16;

/// 正反双样本矩阵：期望 () 或期望诊断码。
pub fn sample_matrix() -> Vec<(Scenario, Option<u32>)> {
    let mut m: Vec<(Scenario, Option<u32>)> = Vec::new();
    // 正：双限皆空
    m.push((Scenario { proc_used: 0, proc_cap: 8, glob_used: 0, glob_cap: 64 }, None));
    // 正：进程级贴界内 1 格、全局同游
    m.push((Scenario { proc_used: 7, proc_cap: 8, glob_used: 63, glob_cap: 64 }, None));
    // 反：进程级满（全局尚空）→ EMFILE 优先
    m.push((Scenario { proc_used: 8, proc_cap: 8, glob_used: 0, glob_cap: 64 }, Some(POSIX_EMFILE)));
    // 反：全局满而进程级未满 → ENFILE
    m.push((Scenario { proc_used: 3, proc_cap: 8, glob_used: 64, glob_cap: 64 }, Some(POSIX_ENFILE)));
    // 边界：双限同时满 → EMFILE 先决
    m.push((Scenario { proc_used: 8, proc_cap: 8, glob_used: 64, glob_cap: 64 }, Some(POSIX_EMFILE)));
    // 边界：cap=0 进程级立即满
    m.push((Scenario { proc_used: 0, proc_cap: 0, glob_used: 0, glob_cap: 64 }, Some(POSIX_EMFILE)));
    // 边界：cap=0 全局立即满（进程级未满）
    m.push((Scenario { proc_used: 0, proc_cap: 4, glob_used: 0, glob_cap: 0 }, Some(POSIX_ENFILE)));
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

/// 生命周期核：open/close 计数对账（两级同步涨落、贴界拒绝、释放归零）。
pub fn lifecycle_ok() -> bool {
    let mut q = Quota::new(3, 5);
    let mut seq_ok = true;
    let mut i = 0;
    while i < 3 {
        if q.acquire().is_err() {
            seq_ok = false;
        }
        i += 1;
    }
    // 第 4 次：进程满 → EMFILE，不占位
    let d = q.acquire();
    let deny_ok = d.err() == Some(POSIX_EMFILE) && q.proc_used == 3 && q.glob_used == 3;
    // 释放 2 次 → 再取 2 次通
    q.release();
    q.release();
    let r1 = q.acquire();
    let r2 = q.acquire();
    let take_ok = r1.is_ok() && r2.is_ok() && q.proc_used == 3 && q.glob_used == 3;
    // 释放超发：连放 5 次，计数钳零不回绕
    let mut j = 0;
    while j < 5 {
        q.release();
        j += 1;
    }
    seq_ok && deny_ok && take_ok && q.proc_used == 0 && q.glob_used == 0
}

/// 次序注入核：进程满全局空的场景下，颠倒次序坏实现必与权威分歧。
pub fn order_inject_ok() -> bool {
    let sc = Scenario { proc_used: 8, proc_cap: 8, glob_used: 0, glob_cap: 64 };
    judge(&sc) != faulty_order(&sc) && judge(&sc).err() == Some(POSIX_EMFILE)
}

/// 探针账核：逐次 acquire 的先验场景与权威同判（重放对拍）。
pub fn probe_replay_ok() -> bool {
    let mut q = Quota::new(2, 3);
    let mut ok = true;
    let mut i = 0;
    while i < 4 {
        let sc = Scenario {
            proc_used: q.proc_used,
            proc_cap: q.proc_cap,
            glob_used: q.glob_used,
            glob_cap: q.glob_cap,
        };
        let want = judge(&sc);
        let got = q.acquire();
        if want != got {
            ok = false;
        }
        i += 1;
    }
    // 第 4 次必 ENFILE（全局 cap=3）
    ok && q.glob_used == 3 && q.note == "denied"
}

/// 注入核：两类坏实现必与权威分歧（检出）。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    // 1) 次序颠倒检出
    got[0] = order_inject_ok();
    // 2) 全局漏推进检出：坏实现模型下全局满场景永远不触发（探针对拍）
    let mut ok = true;
    let mut i = 0;
    while i < 4 {
        // 坏实现只看进程级：进程未满即放行（全局满仍放行）→ 权威拒
        let sc = Scenario { proc_used: i as u32, proc_cap: 8, glob_used: i as u32, glob_cap: 2 };
        let bad = if sc.proc_used >= sc.proc_cap { Err(POSIX_EMFILE) } else { Ok(()) };
        if judge(&sc) != bad && judge(&sc).err() == Some(POSIX_ENFILE) {
            // 权威 ENFILE、坏实现放行 → 分歧
        } else if i >= 2 {
            ok = false;
        }
        i += 1;
    }
    got[1] = ok && faulty_glob();
    got
}

/// EMFILE/ENFILE 判据（对应卡验收 J 判据）。
pub fn run_unx10812_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10812");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平
    s.add("J1-矩阵对平", hits == total && cap_ok, "七线配额样本全命中");

    // J2 正路径：双限皆空放行
    s.add("J2-双限空放行", judge(&Scenario { proc_used: 0, proc_cap: 8, glob_used: 0, glob_cap: 64 }).is_ok(), "两级未满即放行");

    // J3 反路径：进程级满必 EMFILE
    let j3 = judge(&Scenario { proc_used: 8, proc_cap: 8, glob_used: 0, glob_cap: 64 })
        .err() == Some(POSIX_EMFILE);
    s.add("J3-进程满EMFILE", j3, "proc_used>=proc_cap 必 24");

    // J4 反路径：全局满必 ENFILE
    let j4 = judge(&Scenario { proc_used: 3, proc_cap: 8, glob_used: 64, glob_cap: 64 })
        .err() == Some(POSIX_ENFILE);
    s.add("J4-全局满ENFILE", j4, "glob_used>=glob_cap 必 23");

    // J5 边界：双限同满 EMFILE 先决（次序语义）
    let j5 = judge(&Scenario { proc_used: 8, proc_cap: 8, glob_used: 64, glob_cap: 64 })
        .err() == Some(POSIX_EMFILE);
    s.add("J5-双满先EMFILE", j5, "先进程后全局");

    // J6 边界：cap=0 立即满
    let j6 = judge(&Scenario { proc_used: 0, proc_cap: 0, glob_used: 0, glob_cap: 4 })
        .err() == Some(POSIX_EMFILE)
        && judge(&Scenario { proc_used: 0, proc_cap: 4, glob_used: 0, glob_cap: 0 })
            .err() == Some(POSIX_ENFILE);
    s.add("J6-零额即满", j6, "cap=0 零额即拒");

    // J7 生命周期核：涨落对账
    s.add("J7-生命周期核", lifecycle_ok(), "贴界拒/释放归零全对");

    // J8 注入1 次序颠倒检出
    s.add("J8-次序检出", inj[0], "先全局后进程坏实现必分歧");

    // J9 注入2 全局漏推进检出
    s.add("J9-漏推检出", inj[1], "漏推全局计数坏实现必分歧");

    // J10 拒绝不占位：EMFILE 后计数不变
    let mut q = Quota::new(1, 8);
    let _ = q.acquire();
    let d = q.acquire();
    let j10 = d.err() == Some(POSIX_EMFILE) && q.proc_used == 1 && q.glob_used == 1;
    s.add("J10-拒绝零占位", j10, "拒绝后两级计数不变");

    // J11 探针重放对拍
    s.add("J11-探针对拍", probe_replay_ok(), "逐次先验与权威同判");

    // J12 字面锚：24/23 双锚且诊断表完备
    let j12 = POSIX_EMFILE == 24 && POSIX_ENFILE == 23
        && diag_text(EM_PROC_FIRST).len() > 0
        && diag_text(0xffff_ffff) == "未知诊断码";
    s.add("J12-字面锚对平", j12, "24/23 双锚且语义完备");

    // J13 (dyn) 配额账指纹落账
    let mut q2 = Quota::new(4, 8);
    let _ = q2.acquire();
    let _ = q2.acquire();
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(q2.proc_used);
    h = h.wrapping_mul(31).wrapping_add(q2.glob_used);
    h = h.wrapping_mul(31).wrapping_add(q2.proc_cap);
    for c in q2.note.bytes() {
        h = h.wrapping_mul(31).wrapping_add(c as u32);
    }
    s.add_dyn("J13-配额账指纹", h != 0, &format!("fold={:#x}", h));

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
    fn unx10812_quota_flow() {
        let mut q = Quota::new(2, 3);
        assert!(q.acquire().is_ok());
        assert!(q.acquire().is_ok());
        assert!(q.acquire().err() == Some(POSIX_EMFILE));
        q.release();
        assert!(q.acquire().is_ok());
        assert!(q.acquire().err() == Some(POSIX_ENFILE));
        assert!(q.proc_used == 2 && q.glob_used == 3);
        let sc = Scenario { proc_used: 9, proc_cap: 8, glob_used: 9, glob_cap: 8 };
        assert!(judge(&sc).err() == Some(POSIX_EMFILE));
    }

    #[test]
    fn unx10812_checks_green() {
        let s = run_unx10812_checks();
        assert!(s.all_passed(), "UNX-F10812 判据未全绿");
    }
}
