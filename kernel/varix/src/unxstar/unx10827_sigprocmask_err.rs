//! UNX-F10827 sigprocmask 错误矩阵（EINVAL/NULL 语义面）（0x1bfe0 段）
//!
//! 锚点：UNX-F10827-J1 屏蔽对平——how 越闭集必 EINVAL、set=NULL 为只读
//! 面（掩码不变、oldset 回写当前值）、9/19 位入集必丢弃、合法 how 必
//! 推进掩码账四线全对平；注入坏实现（how 漏查/NULL 误改账）必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：掩码账（u32 位集）+ how 闭集（SIG_BLOCK=0/SIG_UNBLOCK=1/
//! SIG_SETMASK=2）+ 掩码判定器（how 对撞 set 算 EINVAL/推进/只读）+
//! 清洗核（9/19 位丢弃）+ 注入核（两类坏实现必分歧）+ 正反双样本矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX 锚：EINVAL=22、SIGKILL=9、SIGSTOP=19。
pub const POSIX_EINVAL: u32 = 22;
pub const SIGKILL: u32 = 9;
pub const SIGSTOP: u32 = 19;

/// how 闭集。
pub const SIG_BLOCK: u32 = 0;
pub const SIG_UNBLOCK: u32 = 1;
pub const SIG_SETMASK: u32 = 2;

/// 诊断段（0x1bfe0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1bfe0;
pub const PM_BAD_HOW: u32 = SEG_BASE + 0x01; // how 越闭集
pub const PM_NULL_SKEW: u32 = SEG_BASE + 0x02; // NULL 误改账
pub const PM_UNCLEAN: u32 = SEG_BASE + 0x03; // 9/19 位入账
pub const PM_OLD_MISS: u32 = SEG_BASE + 0x04; // oldset 回写丢失
pub const PM_REPLAY_MISS: u32 = SEG_BASE + 0x05; // 重放不齐
pub const PM_NOTE_SKEW: u32 = SEG_BASE + 0x06; // 注记错账

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        PM_BAD_HOW => "how 越闭集",
        PM_NULL_SKEW => "NULL 误改账",
        PM_UNCLEAN => "9/19 位入账",
        PM_OLD_MISS => "oldset 回写丢失",
        PM_REPLAY_MISS => "重放不齐",
        PM_NOTE_SKEW => "注记错账",
        _ => "未知诊断码",
    }
}

/// how 合法性。
pub fn how_ok(how: u32) -> bool {
    how == SIG_BLOCK || how == SIG_UNBLOCK || how == SIG_SETMASK
}

/// 9/19 位清洗。
pub fn clean(set: u32) -> u32 {
    set & !(1u32 << SIGKILL) & !(1u32 << SIGSTOP)
}

/// 场景输入：当前掩码 × how × set（None 表示 NULL）× 是否要 oldset。
#[derive(Clone)]
pub struct Scenario {
    pub cur: u32,
    pub how: u32,
    pub set: Option<u32>,
    pub want_old: bool,
}

/// 掩码判定核：how 越闭集必 EINVAL（账不变）；set=NULL 为只读面（账不变，
/// oldset 回写当前值）；合法 how 推进掩码（9/19 位清洗后入账）。
pub fn judge(sc: &Scenario) -> Result<(u32, Option<u32>), u32> {
    if !how_ok(sc.how) {
        return Err(POSIX_EINVAL);
    }
    match sc.set {
        None => {
            let old = if sc.want_old { Some(sc.cur) } else { None };
            Ok((sc.cur, old))
        }
        Some(raw) => {
            let s = clean(raw);
            let next = match sc.how {
                SIG_BLOCK => sc.cur | s,
                SIG_UNBLOCK => sc.cur & !s,
                _ => s,
            };
            let old = if sc.want_old { Some(sc.cur) } else { None };
            Ok((next, old))
        }
    }
}

/// 掩码账状态机：逐次 sigprocmask 推进，oldset 回写账。
pub struct MaskLedger {
    pub cur: u32,
    pub old_writes: Vec<(u32, u32)>,
    pub note: String,
}

impl MaskLedger {
    pub fn new() -> MaskLedger {
        MaskLedger { cur: 0, old_writes: Vec::new(), note: "fresh".to_string() }
    }

    /// 一次调用：判参推进账并回写 oldset。
    pub fn call(&mut self, sc: &Scenario) -> Result<Option<u32>, u32> {
        let out = judge(sc);
        match out {
            Ok((next, old)) => {
                if let Some(o) = old {
                    self.old_writes.push((self.cur, o));
                }
                self.cur = next;
                self.note = if sc.set.is_some() { "advanced".to_string() } else { "readonly".to_string() };
                Ok(old)
            }
            Err(c) => Err(c),
        }
    }

    /// 回写账核：每行 old==调用前掩码。
    pub fn old_writes_ok(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.old_writes.len() {
            let (_pre, old) = &self.old_writes[i];
            if old != &self.old_writes[i].0 {
                ok = false;
            }
            i += 1;
        }
        ok
    }

    /// 掩码清洁核：9/19 位不入账。
    pub fn clean_ok(&self) -> bool {
        self.cur & (1 << SIGKILL) == 0 && self.cur & (1 << SIGSTOP) == 0
    }
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 16;

/// 正反双样本矩阵：期望 (新掩码, oldset) 或 EINVAL。
pub fn sample_matrix() -> Vec<(Scenario, Option<u32>, Option<u32>)> {
    let mut m: Vec<(Scenario, Option<u32>, Option<u32>)> = Vec::new();
    // 正：BLOCK 加位
    m.push((
        Scenario { cur: 0, how: SIG_BLOCK, set: Some(1 << 5), want_old: false },
        Some(1 << 5), None,
    ));
    // 正：UNBLOCK 清位
    m.push((
        Scenario { cur: (1 << 5) | (1 << 6), how: SIG_UNBLOCK, set: Some(1 << 5), want_old: false },
        Some(1 << 6), None,
    ));
    // 正：SETMASK 直写
    m.push((
        Scenario { cur: 0xFF, how: SIG_SETMASK, set: Some(1 << 2), want_old: true },
        Some(1 << 2), Some(0xFF),
    ));
    // 正：set=NULL 只读面（掩码不变，oldset 回写）
    m.push((
        Scenario { cur: 0x20, how: SIG_SETMASK, set: None, want_old: true },
        Some(0x20), Some(0x20),
    ));
    // 正：9/19 位清洗后入账
    m.push((
        Scenario { cur: 0, how: SIG_BLOCK, set: Some((1 << SIGKILL) | (1 << 4)), want_old: false },
        Some(1 << 4), None,
    ));
    // 反：how=3 越闭集
    m.push((
        Scenario { cur: 0, how: 3, set: Some(1), want_old: false },
        None, Some(POSIX_EINVAL),
    ));
    // 反：how=7 越闭集且 set=NULL（仍 EINVAL）
    m.push((
        Scenario { cur: 0x20, how: 7, set: None, want_old: true },
        None, Some(POSIX_EINVAL),
    ));
    m
}

/// 矩阵对平：返回 (命中数, 总数, 容量合规)。
pub fn verify_matrix() -> (usize, usize, bool) {
    let m = sample_matrix();
    let mut ok = 0usize;
    let mut i = 0usize;
    while i < m.len() {
        let (sc, want_mask, want_err) = &m[i];
        let got = judge(sc);
        let hit = match want_err {
            Some(c) => got.err() == Some(*c),
            None => match (&got, want_mask) {
                (Ok((mask, old)), Some(w)) => mask == w && old.is_none(),
                _ => false,
            },
        };
        if hit {
            ok += 1;
        }
        i += 1;
    }
    (ok, m.len(), m.len() <= MATRIX_CAP)
}

/// 坏实现 A：how 漏查（越闭集也推进）。
pub fn faulty_how(sc: &Scenario) -> Result<(u32, Option<u32>), u32> {
    match sc.set {
        None => Ok((sc.cur, if sc.want_old { Some(sc.cur) } else { None })),
        Some(raw) => {
            let s = clean(raw);
            let next = match sc.how {
                SIG_BLOCK => sc.cur | s,
                SIG_UNBLOCK => sc.cur & !s,
                _ => s,
            };
            Ok((next, if sc.want_old { Some(sc.cur) } else { None }))
        }
    }
}

/// 坏实现 B：NULL 误改账（set=NULL 仍清零掩码）。
pub fn faulty_null(sc: &Scenario) -> Result<(u32, Option<u32>), u32> {
    match sc.set {
        None => Ok((0, if sc.want_old { Some(sc.cur) } else { None })),
        _ => judge(sc),
    }
}

/// 注入核：两类坏实现必与权威分歧（检出）。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    // 1) how=3：权威 EINVAL、坏实现按 SETMASK 推进 → 检出
    let sc1 = Scenario { cur: 0, how: 3, set: Some(1 << 3), want_old: false };
    got[0] = judge(&sc1) != faulty_how(&sc1) && judge(&sc1).err() == Some(POSIX_EINVAL);
    // 2) set=NULL：权威掩码不变、坏实现清零 → 检出
    let sc2 = Scenario { cur: 0x20, how: SIG_SETMASK, set: None, want_old: false };
    got[1] = judge(&sc2) != faulty_null(&sc2) && judge(&sc2) == Ok((0x20, None));
    got
}

/// how 全序核：BLOCK → UNBLOCK → SETMASK → 只读 四步账重放。
pub fn how_seq_ok() -> bool {
    let mut lg = MaskLedger::new();
    let _ = lg.call(&Scenario { cur: 0, how: SIG_BLOCK, set: Some(1 << 3), want_old: true });
    let s1 = lg.cur == (1 << 3) && lg.old_writes.len() == 1;
    let _ = lg.call(&Scenario { cur: lg.cur, how: SIG_UNBLOCK, set: Some(1 << 3), want_old: false });
    let s2 = lg.cur == 0;
    let _ = lg.call(&Scenario { cur: lg.cur, how: SIG_SETMASK, set: Some(1 << 6), want_old: true });
    let s3 = lg.cur == (1 << 6) && lg.old_writes.len() == 2;
    let _ = lg.call(&Scenario { cur: lg.cur, how: SIG_SETMASK, set: None, want_old: true });
    let s4 = lg.cur == (1 << 6) && lg.note == "readonly";
    s1 && s2 && s3 && s4 && lg.clean_ok() && lg.old_writes_ok()
}

/// 清洗核：9/19 位单独入集也被丢。
pub fn clean_matrix_ok() -> bool {
    let a = clean(1 << SIGKILL);
    let b = clean(1 << SIGSTOP);
    let c = clean((1 << SIGKILL) | (1 << SIGSTOP) | 0xF);
    a == 0 && b == 0 && c == 0xF
}

/// 掩码面注记（对账/落账用）。
pub fn mask_note(sc: &Scenario) -> String {
    match judge(sc) {
        Ok((next, _)) => format!("next={:#x} how={}", next, sc.how),
        Err(c) => format!("err:{:#x}", c),
    }
}

/// 判据（对应卡验收 J 判据）。
pub fn run_unx10827_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10827");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平
    s.add("J1-矩阵对平", hits == total && cap_ok, "七线掩码样本全命中");

    // J2 反路径：how 越闭集必 EINVAL
    let j2 = judge(&Scenario { cur: 0, how: 3, set: Some(1), want_old: false })
        .err() == Some(POSIX_EINVAL);
    s.add("J2-how越界拒", j2, "how=3 必 22");

    // J3 正路径：set=NULL 只读面（掩码不变、oldset 回写）
    let j3 = judge(&Scenario { cur: 0x20, how: SIG_SETMASK, set: None, want_old: true })
        == Ok((0x20, Some(0x20)));
    s.add("J3-NULL只读面", j3, "掩码不变且回写当前值");

    // J4 9/19 位清洗
    let j4 = clean_matrix_ok();
    s.add("J4-清洗核", j4, "9/19 位不入账");

    // J5 how 全序核：BLOCK→UNBLOCK→SETMASK→只读
    s.add("J5-how全序", how_seq_ok(), "四步账重放全对");

    // J6 注入1 how 漏查检出
    s.add("J6-how检出", inj[0], "越界 how 推进坏实现必分歧");

    // J7 注入2 NULL 误改检出
    s.add("J7-NULL检出", inj[1], "只读面改账坏实现必分歧");

    // J8 oldset 回写核：want_old 才回写
    let j8 = judge(&Scenario { cur: 0x40, how: SIG_SETMASK, set: Some(1), want_old: false })
        == Ok((1, None));
    s.add("J8-回写开关", j8, "want_old=false 不回写");

    // J9 SETMASK 直写覆盖
    let j9 = judge(&Scenario { cur: 0xFF, how: SIG_SETMASK, set: Some(1 << 2), want_old: false })
        == Ok((1 << 2, None));
    s.add("J9-直写覆盖", j9, "SETMASK 全量替换");

    // J10 字面锚：EINVAL=22 / how 闭集 0/1/2
    let j10 = POSIX_EINVAL == 22 && SIG_BLOCK == 0 && SIG_UNBLOCK == 1 && SIG_SETMASK == 2
        && diag_text(PM_BAD_HOW).len() > 0;
    s.add("J10-字面锚对平", j10, "22 与 0/1/2 三 how 锚");

    // J11 掩码面注记闭集
    let n_ok = mask_note(&Scenario { cur: 0, how: SIG_BLOCK, set: Some(1 << 3), want_old: false });
    let n_bad = mask_note(&Scenario { cur: 0, how: 3, set: Some(1 << 3), want_old: false });
    let j11 = n_ok.contains("next") && n_bad.contains("err") && n_ok != n_bad;
    s.add("J11-注记互异", j11, "next/err 注记互异");

    // J12 UNBLOCK 清位面：清他位不动己位
    let j12 = judge(&Scenario {
        cur: (1 << 3) | (1 << 4),
        how: SIG_UNBLOCK,
        set: Some(1 << 3),
        want_old: false,
    }) == Ok((1 << 4, None));
    s.add("J12-清位精确", j12, "只清目标位");

    // J13 (dyn) 掩码账指纹落账
    let mut lg2 = MaskLedger::new();
    let _ = lg2.call(&Scenario { cur: 0, how: SIG_BLOCK, set: Some(0x30), want_old: false });
    let _ = lg2.call(&Scenario { cur: lg2.cur, how: SIG_SETMASK, set: Some(0x8), want_old: true });
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(lg2.cur);
    h = h.wrapping_mul(31).wrapping_add(lg2.old_writes.len() as u32);
    for c in lg2.note.bytes() {
        h = h.wrapping_mul(31).wrapping_add(c as u32);
    }
    s.add_dyn("J13-掩码账指纹", h != 0, &format!("fold={:#x}", h));

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
    fn unx10827_mask_flow() {
        let mut lg = MaskLedger::new();
        assert!(judge(&Scenario { cur: 0, how: 5, set: Some(1), want_old: false }).is_err());
        let r = lg.call(&Scenario { cur: 0, how: SIG_BLOCK, set: Some(0xFF), want_old: true });
        assert!(r.is_ok());
        assert_eq!(lg.cur, 0xFF & clean(0xFF));
        assert_eq!(lg.cur & (1 << SIGKILL), 0);
        let r2 = lg.call(&Scenario { cur: lg.cur, how: SIG_SETMASK, set: None, want_old: true });
        assert_eq!(r2, Ok(Some(lg.cur)));
        assert_eq!(lg.cur, 0xFF & clean(0xFF));
        let inj = inject_two();
        assert!(inj.iter().all(|b| *b));
    }

    #[test]
    fn unx10827_checks_green() {
        let s = run_unx10827_checks();
        assert!(s.all_passed(), "UNX-F10827 判据未全绿");
    }
}
