//! UNX-F10813 EINTR 中断读写矩阵（0x1be20 段）
//!
//! 锚点：UNX-F10813-J1 EINTR 双判据对平——有数据则数据胜出（信号让路）、
//! 无数据且处理器返回（非 SA_RESTART）必 EINTR、SA_RESTART 必静默重启
//! 三线全对平；注入坏实现（有数据仍报 EINTR/重启位漏查）必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：信号中断面（挂起位 × 处置位 × 可用数据）+ 读写裁决器（数据
//! 优先 → 重启位 → EINTR 三级裁决）+ 中断循环状态机（重试推进）+
//! 注入核（两类坏实现必分歧）+ 正反双样本矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX 锚：EINTR=4。
pub const POSIX_EINTR: u32 = 4;

/// 诊断段（0x1be20 独占 0x20）。
pub const SEG_BASE: u32 = 0x1be20;
pub const IN_DATA_LOSS: u32 = SEG_BASE + 0x01; // 有数据仍报 EINTR
pub const IN_RESTART_MISS: u32 = SEG_BASE + 0x02; // SA_RESTART 漏查
pub const IN_NODATA_INTR: u32 = SEG_BASE + 0x03; // 无数据未中断却 EINTR
pub const IN_RETRY_LOSS: u32 = SEG_BASE + 0x04; // 重启循环漏重试
pub const IN_MASK_SKEW: u32 = SEG_BASE + 0x05; // 挂起位错账
pub const IN_READ_TWICE: u32 = SEG_BASE + 0x06; // 重启后重复消费

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        IN_DATA_LOSS => "有数据仍报 EINTR",
        IN_RESTART_MISS => "SA_RESTART 漏查",
        IN_NODATA_INTR => "无信号中断却报 EINTR",
        IN_RETRY_LOSS => "重启循环漏重试",
        IN_MASK_SKEW => "挂起位错账",
        IN_READ_TWICE => "重启后重复消费",
        _ => "未知诊断码",
    }
}

/// 中断后走向。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// 有数据：数据胜出，读走 n 字节。
    Data(usize),
    /// SA_RESTART：静默重启（调用方不可见）。
    Restart,
    /// 处理器返回且无重启位：EINTR。
    Interrupted,
}

/// 场景输入：可用数据 × 挂起信号 × 处置重启位。
#[derive(Clone)]
pub struct Scenario {
    pub avail: usize,
    pub sig_pending: bool,
    pub sa_restart: bool,
    /// 本次请求读长。
    pub want: usize,
}

/// 读写裁决核：数据优先 → 重启位 → EINTR。
pub fn judge(sc: &Scenario) -> Outcome {
    if sc.avail > 0 {
        // 数据胜出：信号不吞已就绪数据
        let n = if sc.avail < sc.want { sc.avail } else { sc.want };
        return Outcome::Data(n);
    }
    if !sc.sig_pending {
        // 无中断且无数据：非 EINTR 面（挂起等待，不产出错误）
        return Outcome::Restart;
    }
    if sc.sa_restart {
        Outcome::Restart
    } else {
        Outcome::Interrupted
    }
}

/// 中断循环状态机：EINTR 后重试，直至有数据；账面记录逐次走向。
pub struct InLoop {
    pub avail: usize,
    pub consumed: u32,
    pub attempts: u32,
    pub log: Vec<String>,
    pub note: String,
}

impl InLoop {
    pub fn new(avail: usize) -> InLoop {
        InLoop {
            avail,
            consumed: 0,
            attempts: 0,
            log: Vec::new(),
            note: "loop".to_string(),
        }
    }

    /// 一次尝试：按裁决推进；EINTR 不消费可重试，Restart 视作等待。
    pub fn step(&mut self, sc: &Scenario) -> Outcome {
        let out = judge(sc);
        self.attempts += 1;
        match &out {
            Outcome::Data(n) => {
                self.avail -= *n;
                self.consumed += *n as u32;
                self.log.push("data".to_string());
            }
            Outcome::Restart => {
                self.log.push("restart".to_string());
            }
            Outcome::Interrupted => {
                self.log.push("eintr".to_string());
            }
        }
        out
    }
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 16;

/// 正反双样本矩阵：期望走向。
pub fn sample_matrix() -> Vec<(Scenario, Outcome)> {
    let mut m: Vec<(Scenario, Outcome)> = Vec::new();
    // 正：有数据 + 挂起 → 数据胜出
    m.push((
        Scenario { avail: 64, sig_pending: true, sa_restart: false, want: 32 },
        Outcome::Data(32),
    ));
    // 正：无数据 + 挂起 + SA_RESTART → 静默重启
    m.push((
        Scenario { avail: 0, sig_pending: true, sa_restart: true, want: 32 },
        Outcome::Restart,
    ));
    // 反：无数据 + 挂起 + 无重启位 → EINTR
    m.push((
        Scenario { avail: 0, sig_pending: true, sa_restart: false, want: 32 },
        Outcome::Interrupted,
    ));
    // 边界：无信号 + 无数据 → 等待面（Restart 语义，不报 EINTR）
    m.push((
        Scenario { avail: 0, sig_pending: false, sa_restart: false, want: 16 },
        Outcome::Restart,
    ));
    // 边界：avail < want 部分读
    m.push((
        Scenario { avail: 5, sig_pending: false, sa_restart: false, want: 16 },
        Outcome::Data(5),
    ));
    // 边界：avail==want 整读
    m.push((
        Scenario { avail: 16, sig_pending: true, sa_restart: true, want: 16 },
        Outcome::Data(16),
    ));
    // 反：重启位挂起但数据先行到达（同场景 avail>0）→ 数据仍胜出
    m.push((
        Scenario { avail: 8, sig_pending: true, sa_restart: true, want: 24 },
        Outcome::Data(8),
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
        if &judge(sc) == want {
            ok += 1;
        }
        i += 1;
    }
    (ok, m.len(), m.len() <= MATRIX_CAP)
}

/// 坏实现 A：有数据仍报 EINTR（信号吞数据）。
pub fn faulty_swallow(sc: &Scenario) -> Outcome {
    if sc.sig_pending {
        return Outcome::Interrupted;
    }
    judge(sc)
}

/// 坏实现 B：重启位漏查（SA_RESTART 也报 EINTR）。
pub fn faulty_no_restart(sc: &Scenario) -> Outcome {
    if sc.sig_pending && sc.avail == 0 {
        return Outcome::Interrupted;
    }
    judge(sc)
}

/// 注入核：两类坏实现必与权威分歧（检出）。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    // 1) 有数据 + 挂起：权威 Data、坏实现 Interrupted → 检出
    let sc1 = Scenario { avail: 8, sig_pending: true, sa_restart: false, want: 8 };
    got[0] = judge(&sc1) != faulty_swallow(&sc1) && matches!(judge(&sc1), Outcome::Data(8));
    // 2) 无数据 + 挂起 + SA_RESTART：权威 Restart、坏实现 Interrupted → 检出
    let sc2 = Scenario { avail: 0, sig_pending: true, sa_restart: true, want: 8 };
    got[1] = judge(&sc2) != faulty_no_restart(&sc2) && judge(&sc2) == Outcome::Restart;
    got
}

/// 中断循环核：EINTR → 重试 → 数据到 → 消费对账（不重复消费、不漏重试）。
pub fn retry_cycle_ok() -> bool {
    let mut lp = InLoop::new(0);
    // 第 1 步：无数据 + 挂起（无重启位）→ EINTR
    let o1 = lp.step(&Scenario { avail: 0, sig_pending: true, sa_restart: false, want: 16 });
    // 第 2 步：仍无数据但 SA_RESTART → 静默重启
    let o2 = lp.step(&Scenario { avail: 0, sig_pending: true, sa_restart: true, want: 16 });
    // 第 3 步：数据到达（挂起仍在）→ 数据胜出
    let o3 = lp.step(&Scenario { avail: 12, sig_pending: true, sa_restart: false, want: 16 });
    let c1 = o1 == Outcome::Interrupted;
    let c2 = o2 == Outcome::Restart;
    let c3 = o3 == Outcome::Data(12);
    let acct = lp.avail == 0 && lp.consumed == 12 && lp.attempts == 3;
    c1 && c2 && c3 && acct && lp.log.len() == 3 && lp.log[0] == "eintr"
}

/// 中断循环二次消费核：数据到后消费一次即耗尽，重启不重复读。
pub fn no_double_read_ok() -> bool {
    let mut lp = InLoop::new(8);
    let _ = lp.step(&Scenario { avail: 8, sig_pending: false, sa_restart: false, want: 4 });
    let _ = lp.step(&Scenario { avail: 4, sig_pending: false, sa_restart: false, want: 8 });
    lp.consumed == 8 && lp.avail == 0 && lp.attempts == 2
}

/// EINTR 判据（对应卡验收 J 判据）。
pub fn run_unx10813_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10813");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平
    s.add("J1-矩阵对平", hits == total && cap_ok, "七线中断样本全命中");

    // J2 正路径：有数据信号让路
    let j2 = judge(&Scenario { avail: 64, sig_pending: true, sa_restart: false, want: 32 })
        == Outcome::Data(32);
    s.add("J2-数据胜出", j2, "挂起信号不吞就绪数据");

    // J3 反路径：无数据 + 处理器返回必 EINTR
    let j3 = judge(&Scenario { avail: 0, sig_pending: true, sa_restart: false, want: 8 })
        == Outcome::Interrupted;
    s.add("J3-处理器返回EINTR", j3, "非重启位必 4");

    // J4 SA_RESTART 静默重启
    let j4 = judge(&Scenario { avail: 0, sig_pending: true, sa_restart: true, want: 8 })
        == Outcome::Restart;
    s.add("J4-重启位静默", j4, "SA_RESTART 不出 EINTR");

    // J5 边界：无信号无数据等待面不报 EINTR
    let j5 = judge(&Scenario { avail: 0, sig_pending: false, sa_restart: false, want: 8 })
        == Outcome::Restart;
    s.add("J5-无信号不误报", j5, "等待面非中断面");

    // J6 边界：部分读 avail<want
    let j6 = judge(&Scenario { avail: 5, sig_pending: false, sa_restart: false, want: 16 })
        == Outcome::Data(5);
    s.add("J6-部分读", j6, "数据不足读尽即返");

    // J7 中断循环核：EINTR → 重试 → 数据 → 消费对账
    s.add("J7-循环消费核", retry_cycle_ok(), "三步走向与账面对平");

    // J8 注入1 数据吞没检出
    s.add("J8-吞没检出", inj[0], "有数据报 EINTR 坏实现必分歧");

    // J9 注入2 重启位漏查检出
    s.add("J9-漏重启检出", inj[1], "SA_RESTART 报 EINTR 必分歧");

    // J10 二次消费钳制
    s.add("J10-不重复消费", no_double_read_ok(), "消费账与可用账同步");

    // J11 字面锚：EINTR=4 且诊断表完备
    let j11 = POSIX_EINTR == 4 && diag_text(IN_RESTART_MISS).len() > 0
        && diag_text(0xffff_ffff) == "未知诊断码";
    s.add("J11-字面锚对平", j11, "EINTR 字面 4 且语义完备");

    // J12 走向闭集：数据/重启/中断三值不混
    let all = [
        judge(&Scenario { avail: 1, sig_pending: true, sa_restart: false, want: 1 }),
        judge(&Scenario { avail: 0, sig_pending: true, sa_restart: true, want: 1 }),
        judge(&Scenario { avail: 0, sig_pending: true, sa_restart: false, want: 1 }),
    ];
    let j12 = all[0] != all[1] && all[1] != all[2] && all[0] != all[2];
    s.add("J12-三值互异", j12, "三走向两两互异");

    // J13 (dyn) 循环账指纹落账
    let mut lp2 = InLoop::new(4);
    let _ = lp2.step(&Scenario { avail: 4, sig_pending: true, sa_restart: false, want: 4 });
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(lp2.attempts);
    h = h.wrapping_mul(31).wrapping_add(lp2.consumed);
    for row in &lp2.log {
        for c in row.bytes() {
            h = h.wrapping_mul(31).wrapping_add(c as u32);
        }
    }
    s.add_dyn("J13-循环账指纹", h != 0 && lp2.note == "loop", &format!("fold={:#x}", h));

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
    fn unx10813_intr_flow() {
        let mut lp = InLoop::new(0);
        assert_eq!(lp.step(&Scenario { avail: 0, sig_pending: true, sa_restart: false, want: 4 }), Outcome::Interrupted);
        assert_eq!(lp.step(&Scenario { avail: 0, sig_pending: true, sa_restart: true, want: 4 }), Outcome::Restart);
        assert_eq!(lp.step(&Scenario { avail: 4, sig_pending: true, sa_restart: false, want: 4 }), Outcome::Data(4));
        assert_eq!(lp.consumed, 4);
        assert_eq!(lp.avail, 0);
        let sc = Scenario { avail: 2, sig_pending: true, sa_restart: true, want: 9 };
        assert_eq!(judge(&sc), Outcome::Data(2));
        let inj = inject_two();
        assert!(inj.iter().all(|b| *b));
    }

    #[test]
    fn unx10813_checks_green() {
        let s = run_unx10813_checks();
        assert!(s.all_passed(), "UNX-F10813 判据未全绿");
    }
}
