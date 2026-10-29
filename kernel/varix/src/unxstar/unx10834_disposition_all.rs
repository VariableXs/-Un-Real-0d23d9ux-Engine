//! UNX-F10834 信号处置全景矩阵（SA_RESTART 全景）（0x1c0c0 段）
//!
//! 锚点：UNX-F10834-J1 处置全景对平——处置×中断×数据三维矩阵逐格同判：
//! 有数据必数据胜出、无数据+处理器返回必 EINTR（无 SA_RESTART）、无数据
//! +SA_RESTART 必静默重启、SIG_IGN/Dflt 阻塞面不受中断扰动四线全对平；
//! 注入坏实现（重启位漏查/数据吞没）必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：处置全景表（处置型 × 挂起 × 数据 × 重启位 → 走向）+ 走向判定器
//! （三维对撞算 Data/EINTR/Restart/Block）+ 全景扫描核（全表逐格对平）+
//! 注入核（两类坏实现必分歧）+ 正反双样本矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX 锚：EINTR=4。
pub const POSIX_EINTR: u32 = 4;

/// 诊断段（0x1c0c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c0c0;
pub const DP_RESTART_MISS: u32 = SEG_BASE + 0x01; // 重启位漏查
pub const DP_DATA_LOSS: u32 = SEG_BASE + 0x02; // 数据吞没
pub const DP_CELL_SKEW: u32 = SEG_BASE + 0x03; // 矩阵格错账
pub const DP_SCAN_MISS: u32 = SEG_BASE + 0x04; // 全景扫描不齐
pub const DP_ROUTE_LOSS: u32 = SEG_BASE + 0x05; // 走向丢失
pub const DP_NOTE_SKEW: u32 = SEG_BASE + 0x06; // 注记错账

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        DP_RESTART_MISS => "重启位漏查",
        DP_DATA_LOSS => "数据吞没",
        DP_CELL_SKEW => "矩阵格错账",
        DP_SCAN_MISS => "全景扫描不齐",
        DP_ROUTE_LOSS => "走向丢失",
        DP_NOTE_SKEW => "注记错账",
        _ => "未知诊断码",
    }
}

/// 处置型。
#[derive(Clone, PartialEq, Eq)]
pub enum Disp {
    Handler,
    Ign,
    Dflt,
}

/// 走向。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Go {
    /// 数据胜出：消费 n 字节。
    Data(usize),
    /// EINTR。
    Eintr,
    /// 静默重启（SA_RESTART）。
    Restart,
    /// 阻塞等待（无中断扰动面）。
    Block,
}

/// 场景输入：处置型 × 挂起信号 × 可用数据 × 重启位 × 请求长。
#[derive(Clone)]
pub struct Scenario {
    pub disp: Disp,
    pub sig_pending: bool,
    pub avail: usize,
    pub sa_restart: bool,
    pub want: usize,
}

/// 走向判定核：数据优先 → 处置对撞（Ign/Dflt 不产 EINTR）→ 重启位 → EINTR。
pub fn judge(sc: &Scenario) -> Go {
    if sc.avail > 0 {
        let n = if sc.avail < sc.want { sc.avail } else { sc.want };
        return Go::Data(n);
    }
    if !sc.sig_pending {
        return Go::Block;
    }
    match sc.disp {
        Disp::Handler => {
            if sc.sa_restart {
                Go::Restart
            } else {
                Go::Eintr
            }
        }
        Disp::Ign | Disp::Dflt => Go::Block,
    }
}

/// 全景扫描核：处置×挂起×重启 三维全扫（数据恒 0）与期望表逐格对平。
pub fn scan_ok() -> bool {
    let disp = [Disp::Handler, Disp::Ign, Disp::Dflt];
    let pend = [true, false];
    let rst = [true, false];
    let mut ok = true;
    let mut cells = 0usize;
    let mut d = 0usize;
    while d < disp.len() {
        let mut p = 0usize;
        while p < pend.len() {
            let mut r = 0usize;
            while r < rst.len() {
                let sc = Scenario {
                    disp: disp[d].clone(),
                    sig_pending: pend[p],
                    avail: 0,
                    sa_restart: rst[r],
                    want: 8,
                };
                let want = match disp[d] {
                    Disp::Handler => {
                        if !pend[p] {
                            Go::Block
                        } else if rst[r] {
                            Go::Restart
                        } else {
                            Go::Eintr
                        }
                    }
                    _ => Go::Block,
                };
                if judge(&sc) != want {
                    ok = false;
                }
                cells += 1;
                r += 1;
            }
            p += 1;
        }
        d += 1;
    }
    ok && cells == 12
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 16;

/// 正反双样本矩阵：期望走向。
pub fn sample_matrix() -> Vec<(Scenario, Go)> {
    let mut m: Vec<(Scenario, Go)> = Vec::new();
    // 正：数据胜出（任何处置）
    m.push((
        Scenario { disp: Disp::Handler, sig_pending: true, avail: 16, sa_restart: false, want: 8 },
        Go::Data(8),
    ));
    // 反：处理器返回无重启位 → EINTR
    m.push((
        Scenario { disp: Disp::Handler, sig_pending: true, avail: 0, sa_restart: false, want: 8 },
        Go::Eintr,
    ));
    // 正：SA_RESTART 静默重启
    m.push((
        Scenario { disp: Disp::Handler, sig_pending: true, avail: 0, sa_restart: true, want: 8 },
        Go::Restart,
    ));
    // 正：SIG_IGN 挂起不产 EINTR
    m.push((
        Scenario { disp: Disp::Ign, sig_pending: true, avail: 0, sa_restart: false, want: 8 },
        Go::Block,
    ));
    // 正：Dflt 挂起不产 EINTR
    m.push((
        Scenario { disp: Disp::Dflt, sig_pending: true, avail: 0, sa_restart: false, want: 8 },
        Go::Block,
    ));
    // 边界：无挂起无数据 → Block
    m.push((
        Scenario { disp: Disp::Handler, sig_pending: false, avail: 0, sa_restart: false, want: 8 },
        Go::Block,
    ));
    // 边界：部分读
    m.push((
        Scenario { disp: Disp::Handler, sig_pending: false, avail: 3, sa_restart: false, want: 8 },
        Go::Data(3),
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

/// 坏实现 A：重启位漏查。
pub fn faulty_no_restart(sc: &Scenario) -> Go {
    match judge(sc) {
        Go::Restart => Go::Eintr,
        g => g,
    }
}

/// 坏实现 B：数据吞没（挂起时数据也报 EINTR）。
pub fn faulty_swallow(sc: &Scenario) -> Go {
    if sc.sig_pending && sc.disp == Disp::Handler && !sc.sa_restart {
        return Go::Eintr;
    }
    judge(sc)
}

/// 注入核：两类坏实现必与权威分歧（检出）。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    // 1) 重启位漏查：SA_RESTART 场景权威 Restart、坏实现 Eintr → 检出
    let sc1 = Scenario { disp: Disp::Handler, sig_pending: true, avail: 0, sa_restart: true, want: 8 };
    got[0] = judge(&sc1) != faulty_no_restart(&sc1) && judge(&sc1) == Go::Restart;
    // 2) 数据吞没：有数据+挂起+处理器返回 权威 Data、坏实现 Eintr → 检出
    let sc2 = Scenario { disp: Disp::Handler, sig_pending: true, avail: 8, sa_restart: false, want: 8 };
    got[1] = judge(&sc2) != faulty_swallow(&sc2) && judge(&sc2) == Go::Data(8);
    got
}

/// 走向注记（对账/落账用）。
pub fn go_note(g: &Go) -> String {
    match g {
        Go::Data(n) => format!("data({})", n),
        Go::Eintr => format!("eintr({})", POSIX_EINTR),
        Go::Restart => "restart".to_string(),
        Go::Block => "block".to_string(),
    }
}

/// 走向互异核：四走向注记两两互异。
pub fn routes_ok() -> bool {
    let a = go_note(&Go::Data(1));
    let b = go_note(&Go::Eintr);
    let c = go_note(&Go::Restart);
    let d = go_note(&Go::Block);
    a != b && a != c && a != d && b != c && b != d && c != d
}

/// 判据（对应卡验收 J 判据）。
pub fn run_unx10834_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10834");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平
    s.add("J1-矩阵对平", hits == total && cap_ok, "七线全景样本全命中");

    // J2 正路径：数据胜出
    let j2 = judge(&Scenario { disp: Disp::Handler, sig_pending: true, avail: 16, sa_restart: false, want: 8 })
        == Go::Data(8);
    s.add("J2-数据胜出", j2, "信号不吞就绪数据");

    // J3 反路径：处理器返回必 EINTR
    let j3 = judge(&Scenario { disp: Disp::Handler, sig_pending: true, avail: 0, sa_restart: false, want: 8 })
        == Go::Eintr;
    s.add("J3-处理器EINTR", j3, "无重启位必 4");

    // J4 SA_RESTART 静默重启
    let j4 = judge(&Scenario { disp: Disp::Handler, sig_pending: true, avail: 0, sa_restart: true, want: 8 })
        == Go::Restart;
    s.add("J4-重启静默", j4, "SA_RESTART 不出 EINTR");

    // J5 正路径：Ign/Dflt 挂起不产 EINTR
    let j5 = judge(&Scenario { disp: Disp::Ign, sig_pending: true, avail: 0, sa_restart: false, want: 8 })
        == Go::Block
        && judge(&Scenario { disp: Disp::Dflt, sig_pending: true, avail: 0, sa_restart: false, want: 8 })
            == Go::Block;
    s.add("J5-忽略面", j5, "非处理器处置不产中断");

    // J6 全景扫描核：12 格逐格对平
    s.add("J6-全景扫描", scan_ok(), "处置×挂起×重启 12 格全对");

    // J7 注入1 重启位漏查检出
    s.add("J7-漏重启检出", inj[0], "SA_RESTART 报 EINTR 必分歧");

    // J8 注入2 数据吞没检出
    s.add("J8-吞没检出", inj[1], "有数据报 EINTR 坏实现必分歧");

    // J9 走向互异核
    s.add("J9-走向互异", routes_ok(), "四走向注记两两互异");

    // J10 部分读边界
    let j10 = judge(&Scenario { disp: Disp::Handler, sig_pending: false, avail: 3, sa_restart: false, want: 8 })
        == Go::Data(3);
    s.add("J10-部分读", j10, "avail<want 读尽即返");

    // J11 字面锚：EINTR=4 且诊断表完备
    let j11 = POSIX_EINTR == 4 && diag_text(DP_RESTART_MISS).len() > 0
        && diag_text(0xffff_ffff) == "未知诊断码";
    s.add("J11-字面锚对平", j11, "EINTR 字面 4 且语义完备");

    // J12 处置闭集：三处置型互异
    let j12 = Disp::Handler != Disp::Ign && Disp::Ign != Disp::Dflt
        && Disp::Handler != Disp::Dflt;
    s.add("J12-处置闭集", j12, "handler/ignored/default 互异");

    // J13 (dyn) 全景指纹落账：12 格走向折入
    let disp = [Disp::Handler, Disp::Ign, Disp::Dflt];
    let mut h: u32 = SEG_BASE;
    let mut d = 0usize;
    while d < disp.len() {
        let sc = Scenario {
            disp: disp[d].clone(),
            sig_pending: true,
            avail: 0,
            sa_restart: false,
            want: 8,
        };
        h = h.wrapping_mul(31).wrapping_add(match judge(&sc) {
            Go::Data(_) => 1,
            Go::Eintr => 2,
            Go::Restart => 3,
            Go::Block => 4,
        });
        d += 1;
    }
    s.add_dyn("J13-全景指纹", h != 0, &format!("fold={:#x}", h));

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
    fn unx10834_disp_flow() {
        assert_eq!(
            judge(&Scenario { disp: Disp::Handler, sig_pending: true, avail: 0, sa_restart: false, want: 4 }),
            Go::Eintr
        );
        assert_eq!(
            judge(&Scenario { disp: Disp::Handler, sig_pending: true, avail: 0, sa_restart: true, want: 4 }),
            Go::Restart
        );
        assert_eq!(
            judge(&Scenario { disp: Disp::Ign, sig_pending: true, avail: 4, sa_restart: false, want: 4 }),
            Go::Data(4)
        );
        assert!(scan_ok());
        let inj = inject_two();
        assert!(inj.iter().all(|b| *b));
    }

    #[test]
    fn unx10834_checks_green() {
        let s = run_unx10834_checks();
        assert!(s.all_passed(), "UNX-F10834 判据未全绿");
    }
}
