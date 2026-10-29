//! UNX-F10831 信号处理器内动作异步安全面（safe 契约清单）（0x1c060 段）
//!
//! 锚点：UNX-F10831-J1 安全面对平——safe 契约清单内动作必放行、清单外
//! 动作（分配/锁/stdio 面）必检出、注入违例必与权威分歧三线全对平；
//! 注入坏实现（违例放行/清单误收）必分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：safe 契约清单（动作名闭集）+ 动作判定器（清单对撞算 safe/违规）
//! + 处理器账（逐动作落账重放）+ 违例分类核（分配面/锁面/stdio 面）+
//! 注入核（两类坏实现必分歧）+ 正反双样本矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1c060 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c060;
pub const AS_UNSAFE_PASS: u32 = SEG_BASE + 0x01; // 违例放行
pub const AS_SAFE_REJECT: u32 = SEG_BASE + 0x02; // 清单内动作误拒
pub const AS_CLASS_MISS: u32 = SEG_BASE + 0x03; // 违例分类缺失
pub const AS_REPLAY_MISS: u32 = SEG_BASE + 0x04; // 重放不齐
pub const AS_LIST_SKEW: u32 = SEG_BASE + 0x05; // 清单错账
pub const AS_NOTE_SKEW: u32 = SEG_BASE + 0x06; // 注记错账

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        AS_UNSAFE_PASS => "违例放行",
        AS_SAFE_REJECT => "清单内动作误拒",
        AS_CLASS_MISS => "违例分类缺失",
        AS_REPLAY_MISS => "重放不齐",
        AS_LIST_SKEW => "清单错账",
        AS_NOTE_SKEW => "注记错账",
        _ => "未知诊断码",
    }
}

/// 违例类别。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Class {
    Safe,
    Alloc,
    Lock,
    Stdio,
    Unknown,
}

/// safe 契约清单（异步信号安全动作闭集）。
pub const SAFE_LIST: [&str; 10] = [
    "write",
    "_exit",
    "sigaction",
    "sigprocmask",
    "kill",
    "raise",
    "alarm",
    "time",
    "unlink",
    "open",
];

/// 清单内判定。
pub fn in_safe_list(action: &str) -> bool {
    let mut hit = false;
    let mut i = 0usize;
    while i < SAFE_LIST.len() {
        if SAFE_LIST[i] == action {
            hit = true;
        }
        i += 1;
    }
    hit
}

/// 违例分类核：清单外动作按名字面分类（分配/锁/stdio/未知）。
pub fn classify(action: &str) -> Class {
    if in_safe_list(action) {
        return Class::Safe;
    }
    if action == "malloc" || action == "free" || action == "realloc" {
        return Class::Alloc;
    }
    if action == "lock" || action == "mutex" || action == "sem_wait" {
        return Class::Lock;
    }
    if action == "printf" || action == "fprintf" || action == "puts" {
        return Class::Stdio;
    }
    Class::Unknown
}

/// 动作判定核：safe 放行、违例拒（分配/锁/stdio 三面携 AS_UNSAFE_PASS，
/// 未知动作携 AS_CLASS_MISS）。
pub fn judge(action: &str) -> Result<Class, u32> {
    match classify(action) {
        Class::Safe => Ok(Class::Safe),
        Class::Unknown => Err(AS_CLASS_MISS),
        Class::Alloc | Class::Lock | Class::Stdio => Err(AS_UNSAFE_PASS),
    }
}

/// 处理器账：逐动作落账重放。
pub struct HandlerLedger {
    pub rows: Vec<(String, String)>,
    pub note: String,
}

impl HandlerLedger {
    pub fn new() -> HandlerLedger {
        HandlerLedger { rows: Vec::new(), note: "ledger".to_string() }
    }

    /// 一次动作：判定入账。
    pub fn act(&mut self, action: &str) -> Result<Class, u32> {
        let out = judge(action);
        let tag = match &out {
            Ok(_) => "safe".to_string(),
            Err(c) => format!("err:{:#x}", c),
        };
        self.rows.push((action.to_string(), tag));
        out
    }

    /// 重放核：逐行与权威同判。
    pub fn replay_ok(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.rows.len() {
            let (action, tag) = &self.rows[i];
            let want = match judge(action) {
                Ok(_) => "safe".to_string(),
                Err(c) => format!("err:{:#x}", c),
            };
            if *tag != want {
                ok = false;
            }
            i += 1;
        }
        ok
    }

    /// 清洁核：账内无违例行（全 safe 处理器）。
    pub fn clean(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].1 != "safe" {
                ok = false;
            }
            i += 1;
        }
        ok
    }
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 16;

/// 正反双样本矩阵：期望 Result。
pub fn sample_matrix() -> Vec<(&'static str, bool)> {
    let mut m: Vec<(&'static str, bool)> = Vec::new();
    // 正：清单内动作
    m.push(("write", true));
    m.push(("_exit", true));
    m.push(("kill", true));
    m.push(("sigprocmask", true));
    m.push(("open", true));
    // 反：分配面
    m.push(("malloc", false));
    // 反：锁面
    m.push(("mutex", false));
    // 反：stdio 面
    m.push(("printf", false));
    m
}

/// 矩阵对平：返回 (命中数, 总数, 容量合规)。
pub fn verify_matrix() -> (usize, usize, bool) {
    let m = sample_matrix();
    let mut ok = 0usize;
    let mut i = 0usize;
    while i < m.len() {
        let (action, want_safe) = &m[i];
        if &judge(action).is_ok() == want_safe {
            ok += 1;
        }
        i += 1;
    }
    (ok, m.len(), m.len() <= MATRIX_CAP)
}

/// 坏实现 A：违例放行（清单外也放行）。
pub fn faulty_pass_all(_action: &str) -> Result<Class, u32> {
    Ok(Class::Safe)
}

/// 坏实现 B：清单误收（malloc 也算 safe）。
pub fn faulty_bad_list(action: &str) -> Result<Class, u32> {
    if action == "malloc" || in_safe_list(action) {
        return Ok(Class::Safe);
    }
    judge(action)
}

/// 注入核：两类坏实现必与权威分歧（检出）。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    // 1) 违例放行：权威拒、坏实现放行 → 检出
    got[0] = judge("printf") != faulty_pass_all("printf") && judge("printf").is_err();
    // 2) 清单误收：权威拒 malloc、坏实现放行 → 检出
    got[1] = judge("malloc") != faulty_bad_list("malloc") && judge("malloc").is_err();
    got
}

/// safe 契约全序核：五动作 safe 账重放全绿。
pub fn safe_seq_ok() -> bool {
    let mut lg = HandlerLedger::new();
    let _ = lg.act("write");
    let _ = lg.act("kill");
    let _ = lg.act("sigaction");
    let _ = lg.act("raise");
    let _ = lg.act("_exit");
    lg.rows.len() == 5 && lg.clean() && lg.replay_ok()
}

/// 违例分类核：三类违例各归类、未知动作归 AS_CLASS_MISS。
pub fn class_matrix_ok() -> bool {
    classify("malloc") == Class::Alloc
        && classify("sem_wait") == Class::Lock
        && classify("puts") == Class::Stdio
        && classify("frobnicate") == Class::Unknown
        && judge("frobnicate").err() == Some(AS_CLASS_MISS)
}

/// 混合账核：safe+违例混合账行数与重放一致。
pub fn mixed_ok() -> bool {
    let mut lg = HandlerLedger::new();
    let _ = lg.act("write");
    let _ = lg.act("malloc");
    let _ = lg.act("printf");
    let _ = lg.act("kill");
    lg.rows.len() == 4 && !lg.clean() && lg.replay_ok() && lg.rows[1].1.starts_with("err:")
}

/// 动作面注记（对账/落账用）。
pub fn action_note(action: &str) -> String {
    match judge(action) {
        Ok(_) => format!("safe:{}", action),
        Err(c) => format!("err:{:#x}:{}", c, action),
    }
}

/// 判据（对应卡验收 J 判据）。
pub fn run_unx10831_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10831");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平
    s.add("J1-矩阵对平", hits == total && cap_ok, "八线动作样本全命中");

    // J2 正路径：write 放行
    let j2 = judge("write") == Ok(Class::Safe);
    s.add("J2-write放行", j2, "write 在 safe 清单");

    // J3 正路径：_exit 放行
    let j3 = judge("_exit") == Ok(Class::Safe);
    s.add("J3-exit放行", j3, "_exit 在 safe 清单");

    // J4 反路径：malloc 拒（分配面）
    let j4 = judge("malloc").is_err() && classify("malloc") == Class::Alloc;
    s.add("J4-malloc拒", j4, "分配面违例");

    // J5 反路径：printf 拒（stdio 面）
    let j5 = judge("printf").is_err() && classify("printf") == Class::Stdio;
    s.add("J5-printf拒", j5, "stdio 面违例");

    // J6 反路径：mutex 拒（锁面）
    let j6 = judge("mutex").is_err() && classify("mutex") == Class::Lock;
    s.add("J6-mutex拒", j6, "锁面违例");

    // J7 注入1 违例放行检出
    s.add("J7-放行检出", inj[0], "printf 放行坏实现必分歧");

    // J8 注入2 清单误收检出
    s.add("J8-误收检出", inj[1], "malloc 入清单坏实现必分歧");

    // J9 safe 全序核
    s.add("J9-safe全序", safe_seq_ok(), "五动作账全绿重放");

    // J10 违例分类核
    s.add("J10-分类核", class_matrix_ok(), "三类违例+未知归类全对");

    // J11 混合账核
    s.add("J11-混合账核", mixed_ok(), "safe/违例混合重放一致");

    // J12 清单闭集核：SAFE_LIST 十项皆 safe
    let mut all_safe = true;
    let mut i = 0usize;
    while i < SAFE_LIST.len() {
        if judge(SAFE_LIST[i]).is_err() {
            all_safe = false;
        }
        i += 1;
    }
    let j12 = all_safe && SAFE_LIST.len() == 10 && diag_text(AS_SAFE_REJECT).len() > 0;
    s.add("J12-清单闭集", j12, "十项清单自洽");

    // J13 (dyn) 处理器账指纹落账
    let mut lg2 = HandlerLedger::new();
    let _ = lg2.act("write");
    let _ = lg2.act("malloc");
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(lg2.rows.len() as u32);
    let mut i2 = 0usize;
    while i2 < lg2.rows.len() {
        for c in lg2.rows[i2].0.bytes() {
            h = h.wrapping_mul(31).wrapping_add(c as u32);
        }
        i2 += 1;
    }
    for c in lg2.note.bytes() {
        h = h.wrapping_mul(31).wrapping_add(c as u32);
    }
    s.add_dyn("J13-处理器账指纹", h != 0, &format!("fold={:#x}", h));

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
    fn unx10831_safe_flow() {
        assert!(judge("write").is_ok());
        assert!(judge("malloc").is_err());
        assert_eq!(classify("realloc"), Class::Alloc);
        assert_eq!(classify("fprintf"), Class::Stdio);
        assert_eq!(classify("lock"), Class::Lock);
        let mut lg = HandlerLedger::new();
        assert!(lg.act("kill").is_ok());
        assert!(lg.act("puts").is_err());
        assert!(lg.replay_ok() && !lg.clean());
        let inj = inject_two();
        assert!(inj.iter().all(|b| *b));
    }

    #[test]
    fn unx10831_checks_green() {
        let s = run_unx10831_checks();
        assert!(s.all_passed(), "UNX-F10831 判据未全绿");
    }
}
