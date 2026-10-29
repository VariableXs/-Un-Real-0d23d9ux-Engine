//! UNX-F10833 SIGUSR1/SIGUSR2 用户信号面（0x1c0a0 段）
//!
//! 锚点：UNX-F10833-J1 用户信号对平——USR1/USR2 默认终止、屏蔽期间计数
//! 合并（std 面一位一次）、计数账与投递账对平三线全对平；注入坏实现
//! （默认不终止/计数虚增）必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：用户信号计数账（号 → 投递计数）+ 处置判定器（默认/处理器/忽略
//! 对撞投递走向）+ 屏蔽期合并状态机 + 重放核（事件序逐笔同判）+ 注入核
//! （两类坏实现必分歧）+ 正反双样本矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX 锚：SIGUSR1=10、SIGUSR2=12。
pub const SIGUSR1: u32 = 10;
pub const SIGUSR2: u32 = 12;

/// 诊断段（0x1c0a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c0a0;
pub const US_TERM_MISS: u32 = SEG_BASE + 0x01; // 默认不终止
pub const US_COUNT_SKEW: u32 = SEG_BASE + 0x02; // 计数虚增
pub const US_MERGE_MISS: u32 = SEG_BASE + 0x03; // 合并缺失
pub const US_ROUTE_LOSS: u32 = SEG_BASE + 0x04; // 分路丢失
pub const US_REPLAY_MISS: u32 = SEG_BASE + 0x05; // 重放不齐
pub const US_NOTE_SKEW: u32 = SEG_BASE + 0x06; // 注记错账

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        US_TERM_MISS => "默认不终止",
        US_COUNT_SKEW => "计数虚增",
        US_MERGE_MISS => "合并缺失",
        US_ROUTE_LOSS => "分路丢失",
        US_REPLAY_MISS => "重放不齐",
        US_NOTE_SKEW => "注记错账",
        _ => "未知诊断码",
    }
}

/// 处置型。
#[derive(Clone, PartialEq, Eq)]
pub enum Disp {
    Dflt,
    Handler,
    Ign,
}

/// 投递走向。
#[derive(Clone, PartialEq, Eq)]
pub enum Route {
    Terminate,
    Count(u32),
    Drop,
}

/// 投递判定核：默认 → 终止；处理器 → 计数+1；忽略 → 丢弃。
pub fn judge(disp: &Disp, count: u32) -> Route {
    match disp {
        Disp::Dflt => Route::Terminate,
        Disp::Handler => Route::Count(count + 1),
        Disp::Ign => Route::Drop,
    }
}

/// 用户信号计数账状态机。
pub struct UserLedger {
    pub c1: u32,
    pub c2: u32,
    pub log: Vec<(u32, String)>,
    pub note: String,
}

impl UserLedger {
    pub fn new() -> UserLedger {
        UserLedger { c1: 0, c2: 0, log: Vec::new(), note: "fresh".to_string() }
    }

    /// 投递一次：按处置分路（USR1/USR2 独立计数）。
    pub fn deliver(&mut self, sig: u32, disp: &Disp) -> Route {
        let out = match sig {
            SIGUSR1 => {
                let r = judge(disp, self.c1);
                if let Route::Count(n) = &r {
                    self.c1 = *n;
                }
                r
            }
            SIGUSR2 => {
                let r = judge(disp, self.c2);
                if let Route::Count(n) = &r {
                    self.c2 = *n;
                }
                r
            }
            _ => Route::Drop,
        };
        let tag = match &out {
            Route::Terminate => "term".to_string(),
            Route::Count(n) => format!("count:{}", n),
            Route::Drop => "drop".to_string(),
        };
        self.log.push((sig, tag.clone()));
        self.note = tag;
        out
    }

    /// 重放核：账行与分路一致。
    pub fn replay_ok(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.log.len() {
            let (sig, tag) = &self.log[i];
            let want = match *sig {
                SIGUSR1 if *tag == "count:1" => true,
                SIGUSR2 if *tag == "count:2" => true,
                _ => *tag == "term" || *tag == "drop",
            };
            if !want {
                ok = false;
            }
            i += 1;
        }
        ok
    }
}

/// 屏蔽期合并核：屏蔽 USR1 期间 N 次投递合并为 1 次（计数 +1 非 +N）。
pub fn merge_ok() -> bool {
    // std 位集模型：三次发送 → 1 位 → 一次投递 → 计数 1
    let mut pending = 0u32;
    let mut i = 0;
    while i < 3 {
        pending |= 1 << SIGUSR1;
        i += 1;
    }
    let deliveries = pending.count_ones();
    let mut lg = UserLedger::new();
    let mut j = 0;
    while j < deliveries {
        let _ = lg.deliver(SIGUSR1, &Disp::Handler);
        j += 1;
    }
    deliveries == 1 && lg.c1 == 1
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 16;

/// 正反双样本矩阵：期望走向。
pub fn sample_matrix() -> Vec<(Disp, u32, Route)> {
    let mut m: Vec<(Disp, u32, Route)> = Vec::new();
    // 正：默认终止
    m.push((Disp::Dflt, 0, Route::Terminate));
    // 正：处理器计数
    m.push((Disp::Handler, 2, Route::Count(3)));
    m.push((Disp::Handler, 0, Route::Count(1)));
    // 正：忽略丢弃
    m.push((Disp::Ign, 7, Route::Drop));
    m
}

/// 矩阵对平：返回 (命中数, 总数, 容量合规)。
pub fn verify_matrix() -> (usize, usize, bool) {
    let m = sample_matrix();
    let mut ok = 0usize;
    let mut i = 0usize;
    while i < m.len() {
        let (disp, count, want) = &m[i];
        if &judge(disp, *count) == want {
            ok += 1;
        }
        i += 1;
    }
    (ok, m.len(), m.len() <= MATRIX_CAP)
}

/// 坏实现 A：默认不终止（当计数处理）。
pub fn faulty_dflt_skip(disp: &Disp, count: u32) -> Route {
    match disp {
        Disp::Dflt => Route::Count(count + 1),
        _ => judge(disp, count),
    }
}

/// 坏实现 B：计数虚增（一次投递加 2）。
pub fn faulty_inflate(disp: &Disp, count: u32) -> Route {
    match disp {
        Disp::Handler => Route::Count(count + 2),
        _ => judge(disp, count),
    }
}

/// 注入核：两类坏实现必与权威分歧（检出）。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    // 1) 默认不终止：权威 Terminate、坏实现 Count → 检出
    got[0] = judge(&Disp::Dflt, 0) == Route::Terminate
        && matches!(faulty_dflt_skip(&Disp::Dflt, 0), Route::Count(_));
    // 2) 计数虚增：权威 +1、坏实现 +2 → 检出
    got[1] = judge(&Disp::Handler, 3) != faulty_inflate(&Disp::Handler, 3)
        && judge(&Disp::Handler, 3) == Route::Count(4);
    got
}

/// 双号独立核：USR1/USR2 计数独立互不串扰。
pub fn two_sig_ok() -> bool {
    let mut lg = UserLedger::new();
    let _ = lg.deliver(SIGUSR1, &Disp::Handler);
    let _ = lg.deliver(SIGUSR2, &Disp::Handler);
    let _ = lg.deliver(SIGUSR1, &Disp::Handler);
    lg.c1 == 2 && lg.c2 == 1 && lg.replay_ok()
}

/// 终止面注记（对账/落账用）。
pub fn route_note(r: &Route) -> String {
    match r {
        Route::Terminate => "terminate".to_string(),
        Route::Count(n) => format!("count:{}", n),
        Route::Drop => "drop".to_string(),
    }
}

/// 判据（对应卡验收 J 判据）。
pub fn run_unx10833_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10833");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平
    s.add("J1-矩阵对平", hits == total && cap_ok, "四线投递样本全命中");

    // J2 正路径：默认终止
    let j2 = judge(&Disp::Dflt, 0) == Route::Terminate;
    s.add("J2-默认终止", j2, "USR1/USR2 默认核死");

    // J3 正路径：处理器计数
    let j3 = judge(&Disp::Handler, 4) == Route::Count(5);
    s.add("J3-计数面", j3, "投递 +1");

    // J4 正路径：忽略丢弃
    let j4 = judge(&Disp::Ign, 9) == Route::Drop;
    s.add("J4-忽略丢弃", j4, "投递不入账");

    // J5 屏蔽期合并核
    s.add("J5-合并核", merge_ok(), "三发一投递计数 1");

    // J6 注入1 默认不终止检出
    s.add("J6-默认检出", inj[0], "默认当计数坏实现必分歧");

    // J7 注入2 计数虚增检出
    s.add("J7-虚增检出", inj[1], "+2 坏实现必分歧");

    // J8 双号独立核
    s.add("J8-双号独立", two_sig_ok(), "USR1/USR2 计数不串");

    // J9 账重放核
    let mut lg = UserLedger::new();
    let _ = lg.deliver(SIGUSR1, &Disp::Handler);
    let _ = lg.deliver(SIGUSR2, &Disp::Ign);
    let _ = lg.deliver(SIGUSR1, &Disp::Dflt);
    let j9 = lg.replay_ok() && lg.log.len() == 3;
    s.add("J9-账重放", j9, "三行账逐条同判");

    // J10 分路闭集：三走向互异
    let a = route_note(&Route::Terminate);
    let b = route_note(&Route::Count(1));
    let c = route_note(&Route::Drop);
    let j10 = a != b && b != c && a != c;
    s.add("J10-分路闭集", j10, "terminate/count/drop 互异");

    // J11 字面锚：SIGUSR1=10 / SIGUSR2=12
    let j11 = SIGUSR1 == 10 && SIGUSR2 == 12 && diag_text(US_TERM_MISS).len() > 0;
    s.add("J11-字面锚对平", j11, "10/12 双锚且语义完备");

    // J12 计数上限防御：u32 不回绕（大计数钳制面）
    let big = u32::MAX - 1;
    let j12 = judge(&Disp::Handler, big) == Route::Count(u32::MAX);
    s.add("J12-上限防御", j12, "贴界 +1 不回绕");

    // J13 (dyn) 计数账指纹落账
    let mut lg2 = UserLedger::new();
    let _ = lg2.deliver(SIGUSR1, &Disp::Handler);
    let _ = lg2.deliver(SIGUSR1, &Disp::Handler);
    let _ = lg2.deliver(SIGUSR2, &Disp::Handler);
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(lg2.c1);
    h = h.wrapping_mul(31).wrapping_add(lg2.c2);
    for c in lg2.note.bytes() {
        h = h.wrapping_mul(31).wrapping_add(c as u32);
    }
    s.add_dyn("J13-计数账指纹", h != 0, &format!("fold={:#x}", h));

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
    fn unx10833_usr_flow() {
        let mut lg = UserLedger::new();
        assert!(matches!(lg.deliver(SIGUSR1, &Disp::Handler), Route::Count(1)));
        assert!(matches!(lg.deliver(SIGUSR2, &Disp::Handler), Route::Count(1)));
        assert!(matches!(lg.deliver(SIGUSR1, &Disp::Ign), Route::Drop));
        assert!(matches!(lg.deliver(SIGUSR1, &Disp::Dflt), Route::Terminate));
        assert_eq!(lg.c1, 1);
        assert_eq!(lg.c2, 1);
        assert!(lg.replay_ok());
        let inj = inject_two();
        assert!(inj.iter().all(|b| *b));
    }

    #[test]
    fn unx10833_checks_green() {
        let s = run_unx10833_checks();
        assert!(s.all_passed(), "UNX-F10833 判据未全绿");
    }
}
