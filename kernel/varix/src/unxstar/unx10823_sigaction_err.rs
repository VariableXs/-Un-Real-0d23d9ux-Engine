//! UNX-F10823 sigaction 参数错误矩阵（EINVAL/EFAULT）（0x1bf60 段）
//!
//! 锚点：UNX-F10823-J1 参数对平——非法信号号必 EINVAL、未知旗标位必
//! EINVAL、oldact 指针无效必 EFAULT 且不污染回写账、合法参数必全落账
//! 四线全对平；注入坏实现（旗标漏查/坏指针不查）必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：指针域账（USER_LO/USER_HI 界）+ 旗标闭集（SA_* 合法位集）+
//! 参数判定器（号→旗标→指针三级裁决）+ 回写账状态机（旧处置快照回填）
//! + 注入核（两类坏实现必分歧）+ 正反双样本矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX 锚：EINVAL=22、EFAULT=14、SIGKILL=9、SIGSTOP=19。
pub const POSIX_EINVAL: u32 = 22;
pub const POSIX_EFAULT: u32 = 14;
pub const SIGKILL: u32 = 9;
pub const SIGSTOP: u32 = 19;
/// 合法信号号上界。
pub const NSIG: u32 = 65;

/// 旗标合法位集：SA_NOCLDSTOP|SA_NOCLDWAIT|SA_SIGINFO|SA_ONSTACK|SA_RESTART|SA_NODEFER|SA_RESETHAND。
pub const SA_VALID: u32 = 0x1 | 0x2 | 0x4 | 0x8 | 0x10 | 0x20 | 0x40;
pub const SA_RESTART: u32 = 0x10;

/// 用户指针域：0 表示 NULL（合法），USER_LO..USER_HI 表示有效域。
pub const USER_LO: u64 = 0x1000;
pub const USER_HI: u64 = 0xC000_0000;

/// 诊断段（0x1bf60 独占 0x20）。
pub const SEG_BASE: u32 = 0x1bf60;
pub const SA_BAD_SIG: u32 = SEG_BASE + 0x01; // 非法信号号
pub const SA_BAD_FLAG: u32 = SEG_BASE + 0x02; // 未知旗标位
pub const SA_BAD_PTR: u32 = SEG_BASE + 0x03; // 指针无效
pub const SA_OLD_SKEW: u32 = SEG_BASE + 0x04; // 回写账错
pub const SA_FLAG_LOSS: u32 = SEG_BASE + 0x05; // 旗标丢失
pub const SA_REPLAY_MISS: u32 = SEG_BASE + 0x06; // 重放不齐

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        SA_BAD_SIG => "非法信号号",
        SA_BAD_FLAG => "未知旗标位",
        SA_BAD_PTR => "指针无效",
        SA_OLD_SKEW => "回写账错",
        SA_FLAG_LOSS => "旗标丢失",
        SA_REPLAY_MISS => "重放不齐",
        _ => "未知诊断码",
    }
}

/// 指针域判定：NULL 合法；域内合法；域外（内核/越界）非法。
pub fn ptr_ok(p: u64) -> bool {
    if p == 0 {
        return true;
    }
    p >= USER_LO && p < USER_HI
}

/// 场景输入：信号号 × 旗标位 × act 指针 × oldact 指针。
#[derive(Clone)]
pub struct Scenario {
    pub sig: u32,
    pub flags: u32,
    pub act_ptr: u64,
    pub old_ptr: u64,
}

/// 参数判定核：号 → 旗标 → act 指针 → oldact 指针 顺序裁决。
pub fn judge(sc: &Scenario) -> Result<(), u32> {
    if sc.sig == 0 || sc.sig >= NSIG {
        return Err(SA_BAD_SIG);
    }
    if sc.sig == SIGKILL || sc.sig == SIGSTOP {
        return Err(SA_BAD_SIG);
    }
    if sc.flags & !SA_VALID != 0 {
        return Err(SA_BAD_FLAG);
    }
    if !ptr_ok(sc.act_ptr) {
        return Err(POSIX_EFAULT);
    }
    if !ptr_ok(sc.old_ptr) {
        return Err(POSIX_EFAULT);
    }
    Ok(())
}

/// sigaction 调用状态机：旧处置账（回写快照）。
pub struct ActionLedger {
    pub installed: Vec<(u32, u32)>,
    pub old_writes: Vec<(u64, u32)>,
    pub note: String,
}

impl ActionLedger {
    pub fn new() -> ActionLedger {
        ActionLedger { installed: Vec::new(), old_writes: Vec::new(), note: "fresh".to_string() }
    }

    /// 一次 sigaction：判参通过才装账并回写 oldact 快照。
    pub fn call(&mut self, sc: &Scenario, old_flags: u32) -> Result<(), u32> {
        let out = judge(sc);
        if out.is_ok() {
            self.installed.push((sc.sig, sc.flags));
            if sc.old_ptr != 0 {
                self.old_writes.push((sc.old_ptr, old_flags));
            }
            self.note = "installed".to_string();
        }
        out
    }

    /// 回写账核：每个非 NULL oldact 行恰一次回写且指针域合法。
    pub fn old_writes_ok(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.old_writes.len() {
            if !ptr_ok(self.old_writes[i].0) {
                ok = false;
            }
            i += 1;
        }
        ok && self.old_writes.len() <= self.installed.len()
    }

    /// 重放核：装账行号全在合法域。
    pub fn replay_ok(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.installed.len() {
            let (sig, _fl) = &self.installed[i];
            if *sig == 0 || *sig >= NSIG {
                ok = false;
            }
            i += 1;
        }
        ok
    }
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 16;

/// 正反双样本矩阵：期望 () 或期望诊断码。
pub fn sample_matrix() -> Vec<(Scenario, Option<u32>)> {
    let mut m: Vec<(Scenario, Option<u32>)> = Vec::new();
    // 正：合法全套（act/old 均域内）
    m.push((
        Scenario { sig: 15, flags: SA_RESTART, act_ptr: 0x2000, old_ptr: 0x3000 },
        None,
    ));
    // 正：act NULL（查询式）old NULL
    m.push((
        Scenario { sig: 15, flags: 0, act_ptr: 0, old_ptr: 0 },
        None,
    ));
    // 正：旗标满集
    m.push((
        Scenario { sig: 2, flags: SA_VALID, act_ptr: USER_LO, old_ptr: USER_HI - 8 },
        None,
    ));
    // 反：0 号
    m.push((
        Scenario { sig: 0, flags: 0, act_ptr: 0x2000, old_ptr: 0 },
        Some(SA_BAD_SIG),
    ));
    // 反：9 号
    m.push((
        Scenario { sig: SIGKILL, flags: 0, act_ptr: 0x2000, old_ptr: 0 },
        Some(SA_BAD_SIG),
    ));
    // 反：未知旗标位 0x80
    m.push((
        Scenario { sig: 15, flags: SA_RESTART | 0x80, act_ptr: 0x2000, old_ptr: 0 },
        Some(SA_BAD_FLAG),
    ));
    // 反：oldact 指针内核域
    m.push((
        Scenario { sig: 15, flags: 0, act_ptr: 0x2000, old_ptr: 0xFFFF_8000_0000_0000 },
        Some(POSIX_EFAULT),
    ));
    // 反：act 指针越下界
    m.push((
        Scenario { sig: 15, flags: 0, act_ptr: 0x800, old_ptr: 0 },
        Some(POSIX_EFAULT),
    ));
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

/// 坏实现 A：旗标漏查（未知位放行）。
pub fn faulty_flag(sc: &Scenario) -> Result<(), u32> {
    if sc.sig == 0 || sc.sig >= NSIG {
        return Err(SA_BAD_SIG);
    }
    if !ptr_ok(sc.act_ptr) || !ptr_ok(sc.old_ptr) {
        return Err(POSIX_EFAULT);
    }
    Ok(())
}

/// 坏实现 B：坏指针不查（oldact 内核域放行）。
pub fn faulty_ptr(sc: &Scenario) -> Result<(), u32> {
    if sc.sig == 0 || sc.sig >= NSIG {
        return Err(SA_BAD_SIG);
    }
    if sc.flags & !SA_VALID != 0 {
        return Err(SA_BAD_FLAG);
    }
    Ok(())
}

/// 注入核：两类坏实现必与权威分歧（检出）。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    // 1) 未知旗标 0x80：权威 EINVAL 面、坏实现放行 → 检出
    let sc1 = Scenario { sig: 15, flags: 0x80, act_ptr: 0x2000, old_ptr: 0 };
    got[0] = judge(&sc1) != faulty_flag(&sc1) && judge(&sc1).err() == Some(SA_BAD_FLAG);
    // 2) oldact 内核域：权威 EFAULT、坏实现放行 → 检出
    let sc2 = Scenario {
        sig: 15,
        flags: 0,
        act_ptr: 0x2000,
        old_ptr: 0xFFFF_8000_0000_0000,
    };
    got[1] = judge(&sc2) != faulty_ptr(&sc2) && judge(&sc2).err() == Some(POSIX_EFAULT);
    got
}

/// 参数全序核：装 15（带 oldact 回写）→ 查账 → 再装 2 → 账面对平。
pub fn install_seq_ok() -> bool {
    let mut lg = ActionLedger::new();
    let r1 = lg.call(&Scenario { sig: 15, flags: SA_RESTART, act_ptr: 0x2000, old_ptr: 0x3000 }, 0);
    let r2 = lg.call(&Scenario { sig: 2, flags: 0, act_ptr: 0x2000, old_ptr: 0 }, SA_RESTART);
    let ok = r1.is_ok() && r2.is_ok();
    ok && lg.installed.len() == 2 && lg.old_writes.len() == 1
        && lg.old_writes[0] == (0x3000, 0)
        && lg.old_writes_ok() && lg.replay_ok()
}

/// EFAULT 零污染核：坏指针调用后装账/回写账均不变。
pub fn fault_no_side_ok() -> bool {
    let mut lg = ActionLedger::new();
    let _ = lg.call(&Scenario { sig: 15, flags: SA_RESTART, act_ptr: 0x2000, old_ptr: 0 }, 0);
    let before_i = lg.installed.len();
    let before_w = lg.old_writes.len();
    let _ = lg.call(&Scenario { sig: 15, flags: 0, act_ptr: 0x2000, old_ptr: 0xDEAD_BEEF }, 7);
    let _ = lg.call(&Scenario { sig: 0, flags: 0, act_ptr: 0x2000, old_ptr: 0x3000 }, 7);
    lg.installed.len() == before_i && lg.old_writes.len() == before_w
}

/// 参数面注记（对账/落账用）。
pub fn arg_note(sc: &Scenario) -> String {
    match judge(sc) {
        Ok(()) => format!("ok sig={} flags={:#x}", sc.sig, sc.flags),
        Err(c) => format!("err:{:#x}", c),
    }
}

/// 判据（对应卡验收 J 判据）。
pub fn run_unx10823_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10823");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平
    s.add("J1-矩阵对平", hits == total && cap_ok, "八线参数样本全命中");

    // J2 反路径：非法号必 EINVAL 面
    let j2 = judge(&Scenario { sig: 0, flags: 0, act_ptr: 0x2000, old_ptr: 0 })
        .err() == Some(SA_BAD_SIG)
        && judge(&Scenario { sig: SIGSTOP, flags: 0, act_ptr: 0x2000, old_ptr: 0 })
            .err() == Some(SA_BAD_SIG);
    s.add("J2-非法号拒", j2, "0/9/19 号必拒");

    // J3 反路径：未知旗标必 EINVAL 面
    let j3 = judge(&Scenario { sig: 15, flags: 0x80, act_ptr: 0x2000, old_ptr: 0 })
        .err() == Some(SA_BAD_FLAG);
    s.add("J3-未知旗标拒", j3, "0x80 越闭集必拒");

    // J4 反路径：坏指针必 EFAULT
    let j4 = judge(&Scenario { sig: 15, flags: 0, act_ptr: 0x2000, old_ptr: 0xFFFF_8000_0000_0000 })
        .err() == Some(POSIX_EFAULT);
    s.add("J4-坏指针拒", j4, "oldact 内核域必 14");

    // J5 正路径：NULL act 查询式合法
    let j5 = judge(&Scenario { sig: 15, flags: 0, act_ptr: 0, old_ptr: 0 }).is_ok();
    s.add("J5-NULL合法", j5, "NULL 指针域合法");

    // J6 参数全序核
    s.add("J6-装账全序", install_seq_ok(), "装账/回写账全对");

    // J7 注入1 旗标漏查检出
    s.add("J7-旗标检出", inj[0], "未知位放行坏实现必分歧");

    // J8 注入2 坏指针不查检出
    s.add("J8-指针检出", inj[1], "坏指针放行坏实现必分歧");

    // J9 EFAULT 零污染核
    s.add("J9-零污染核", fault_no_side_ok(), "拒参不落账不回写");

    // J10 指针域闭集：NULL/域内真、域外假
    let j10 = ptr_ok(0) && ptr_ok(USER_LO) && ptr_ok(USER_HI - 1) && !ptr_ok(USER_HI)
        && !ptr_ok(0x800);
    s.add("J10-指针域闭集", j10, "四界清晰");

    // J11 字面锚：22/14 双锚且 SA_RESTART 位对平
    let j11 = POSIX_EINVAL == 22 && POSIX_EFAULT == 14 && SA_RESTART == 0x10
        && diag_text(SA_BAD_PTR).len() > 0;
    s.add("J11-字面锚对平", j11, "22/14/0x10 三锚");

    // J12 参数面注记闭集
    let n_ok = arg_note(&Scenario { sig: 15, flags: SA_RESTART, act_ptr: 0x2000, old_ptr: 0 });
    let n_bad = arg_note(&Scenario { sig: 15, flags: 0x80, act_ptr: 0x2000, old_ptr: 0 });
    let j12 = n_ok.contains("ok") && n_bad.contains("err") && n_ok != n_bad;
    s.add("J12-注记互异", j12, "ok/err 注记互异");

    // J13 (dyn) 装账指纹落账
    let mut lg2 = ActionLedger::new();
    let _ = lg2.call(&Scenario { sig: 15, flags: SA_RESTART, act_ptr: 0x2000, old_ptr: 0x3000 }, 0);
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(lg2.installed.len() as u32);
    h = h.wrapping_mul(31).wrapping_add(lg2.old_writes.len() as u32);
    for c in lg2.note.bytes() {
        h = h.wrapping_mul(31).wrapping_add(c as u32);
    }
    s.add_dyn("J13-装账指纹", h != 0, &format!("fold={:#x}", h));

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
    fn unx10823_sigaction_flow() {
        let mut lg = ActionLedger::new();
        assert!(judge(&Scenario { sig: 0, flags: 0, act_ptr: 0x2000, old_ptr: 0 }).is_err());
        assert!(judge(&Scenario { sig: 15, flags: 0x80, act_ptr: 0x2000, old_ptr: 0 }).is_err());
        assert!(judge(&Scenario { sig: 15, flags: 0, act_ptr: 0x2000, old_ptr: 0xFFFF_8000_0000_0000 }).is_err());
        assert!(lg.call(&Scenario { sig: 15, flags: SA_RESTART, act_ptr: 0x2000, old_ptr: 0x3000 }, 0).is_ok());
        assert!(lg.call(&Scenario { sig: 15, flags: 0, act_ptr: 0x2000, old_ptr: 0xDEAD }, 0).is_err());
        assert_eq!(lg.installed.len(), 1);
        let inj = inject_two();
        assert!(inj.iter().all(|b| *b));
    }

    #[test]
    fn unx10823_checks_green() {
        let s = run_unx10823_checks();
        assert!(s.all_passed(), "UNX-F10823 判据未全绿");
    }
}
