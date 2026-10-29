//! UNX-F10821 SIGKILL/SIGSTOP 不可捕信号面（0x1bf20 段）
//!
//! 锚点：UNX-F10821-J1 不可捕对平——对 9/19 号装处理器或忽略必 EINVAL、
//! 屏蔽位寄存必被丢弃、9/19 一经投递必生效（终止/暂停不可拦截）三线全
//! 对平；注入坏实现（可捕化/屏蔽穿透）必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：处置账（信号号 × 处置型）+ 处置判定器（不可捕号对撞处置型算
//! EINVAL/接受）+ 屏蔽位账（9/19 位寄存即丢）+ 投递状态机（终态不可拦）
//! + 注入核（两类坏实现必分歧）+ 正反双样本矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX 锚：SIGKILL=9、SIGSTOP=19、EINVAL=22。
pub const SIGKILL: u32 = 9;
pub const SIGSTOP: u32 = 19;
pub const POSIX_EINVAL: u32 = 22;

/// 诊断段（0x1bf20 独占 0x20）。
pub const SEG_BASE: u32 = 0x1bf20;
pub const UK_CATCHABLE: u32 = SEG_BASE + 0x01; // 不可捕信号被装处理器
pub const UK_MASKED: u32 = SEG_BASE + 0x02; // 屏蔽位穿透
pub const UK_DELIVER_MISS: u32 = SEG_BASE + 0x03; // 投递被拦截
pub const UK_REG_SKEW: u32 = SEG_BASE + 0x04; // 处置账错
pub const UK_MASK_LOSS: u32 = SEG_BASE + 0x05; // 屏蔽账丢失
pub const UK_NOTE_SKEW: u32 = SEG_BASE + 0x06; // 终态注记错

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        UK_CATCHABLE => "不可捕信号被装处理器",
        UK_MASKED => "9/19 屏蔽位穿透",
        UK_DELIVER_MISS => "投递被拦截",
        UK_REG_SKEW => "处置账错",
        UK_MASK_LOSS => "屏蔽账丢失",
        UK_NOTE_SKEW => "终态注记错",
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

/// 是否不可捕号。
pub fn uncatchable(sig: u32) -> bool {
    sig == SIGKILL || sig == SIGSTOP
}

/// 场景输入：信号号 × 处置型 × 屏蔽位集。
#[derive(Clone)]
pub struct Scenario {
    pub sig: u32,
    pub disp: Disp,
    pub mask: u32,
}

/// 处置判定核：9/19 装 Handler/Ign 必 EINVAL；Dflt 接受。
pub fn judge(sc: &Scenario) -> Result<&'static str, u32> {
    if uncatchable(sc.sig) && sc.disp != Disp::Dflt {
        return Err(POSIX_EINVAL);
    }
    match sc.disp {
        Disp::Dflt => Ok("default"),
        Disp::Handler => Ok("handler"),
        Disp::Ign => Ok("ignored"),
    }
}

/// 屏蔽位账：9/19 位寄存即丢（返回清洗后的位集）。
pub fn mask_clean(mask: u32) -> u32 {
    mask & !(1u32 << SIGKILL) & !(1u32 << SIGSTOP)
}

/// 进程投递状态机：终态枚举（Alive/Stopped/Killed），9/19 不可拦。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum State {
    Alive,
    Stopped,
    Killed,
}

/// 投递核：按信号号推进状态，屏蔽位不影响 9/19 生效。
pub fn deliver(cur: &State, sig: u32, _mask: u32) -> State {
    match sig {
        SIGKILL => State::Killed,
        SIGSTOP => State::Stopped,
        _ => cur.clone(),
    }
}

/// 处置账：逐次装处置，落账重放。
pub struct DispLedger {
    pub rows: Vec<(u32, Disp, String)>,
    pub note: String,
}

impl DispLedger {
    pub fn new() -> DispLedger {
        DispLedger { rows: Vec::new(), note: "ledger".to_string() }
    }

    /// 装处置：裁定入账。
    pub fn install(&mut self, sc: &Scenario) -> Result<&'static str, u32> {
        let out = judge(sc);
        let tag = match &out {
            Ok(t) => t.to_string(),
            Err(c) => format!("err:{:#x}", c),
        };
        self.rows.push((sc.sig, sc.disp.clone(), tag));
        out
    }

    /// 重放核：逐行与权威同判。
    pub fn replay_ok(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.rows.len() {
            let (sig, disp, tag) = &self.rows[i];
            let sc = Scenario { sig: *sig, disp: disp.clone(), mask: 0 };
            let want = match judge(&sc) {
                Ok(t) => t.to_string(),
                Err(c) => format!("err:{:#x}", c),
            };
            if *tag != want {
                ok = false;
            }
            i += 1;
        }
        ok
    }

    /// 不可捕拒装核：账内 9/19 无 handler/ignored 行。
    pub fn uncatchable_clean(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.rows.len() {
            let (sig, disp, tag) = &self.rows[i];
            if uncatchable(*sig) && *disp != Disp::Dflt && !tag.starts_with("err:") {
                ok = false;
            }
            i += 1;
        }
        ok
    }
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 16;

/// 正反双样本矩阵：期望注记或 EINVAL。
pub fn sample_matrix() -> Vec<(Scenario, Option<u32>)> {
    let mut m: Vec<(Scenario, Option<u32>)> = Vec::new();
    // 正：普通信号装处理器/忽略接受
    m.push((Scenario { sig: 15, disp: Disp::Handler, mask: 0 }, None));
    m.push((Scenario { sig: 1, disp: Disp::Ign, mask: 0 }, None));
    // 正：9/19 默认处置接受
    m.push((Scenario { sig: SIGKILL, disp: Disp::Dflt, mask: 0 }, None));
    m.push((Scenario { sig: SIGSTOP, disp: Disp::Dflt, mask: 0 }, None));
    // 反：9 装 Handler 必 EINVAL
    m.push((Scenario { sig: SIGKILL, disp: Disp::Handler, mask: 0 }, Some(POSIX_EINVAL)));
    // 反：19 忽略必 EINVAL
    m.push((Scenario { sig: SIGSTOP, disp: Disp::Ign, mask: 0 }, Some(POSIX_EINVAL)));
    // 边界：屏蔽位含 9/19（清洗后不拒默认处置）
    m.push((Scenario { sig: SIGKILL, disp: Disp::Dflt, mask: (1 << SIGKILL) | (1 << SIGSTOP) }, None));
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

/// 坏实现 A：9 可捕化（装处理器放行）。
pub fn faulty_catchable(sc: &Scenario) -> Result<&'static str, u32> {
    if sc.disp == Disp::Handler {
        return Ok("handler");
    }
    judge(sc)
}

/// 坏实现 B：屏蔽位穿透（9/19 位也寄存）。
pub fn faulty_mask_pass(mask: u32) -> u32 {
    mask
}

/// 注入核：两类坏实现必与权威分歧（检出）。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    // 1) 9 装 Handler：权威 EINVAL、坏实现放行 → 检出
    let sc1 = Scenario { sig: SIGKILL, disp: Disp::Handler, mask: 0 };
    got[0] = judge(&sc1) != faulty_catchable(&sc1) && judge(&sc1).err() == Some(POSIX_EINVAL);
    // 2) 屏蔽位清洗：权威丢 9/19 位、坏实现保留 → 检出
    let raw = (1u32 << SIGKILL) | (1u32 << 15);
    got[1] = mask_clean(raw) != faulty_mask_pass(raw)
        && mask_clean(raw) == (1u32 << 15)
        && uncatchable(SIGKILL)
        && uncatchable(SIGSTOP);
    got
}

/// 投递全序核：Alive → SIGSTOP 暂停 → SIGKILL 必死（屏蔽位无效）。
pub fn deliver_seq_ok() -> bool {
    let st0 = State::Alive;
    let st1 = deliver(&st0, SIGSTOP, 1 << SIGSTOP);
    let st2 = deliver(&st1, 15, 0);
    let st3 = deliver(&st2, SIGKILL, 1 << SIGKILL);
    st0 == State::Alive && st1 == State::Stopped && st2 == State::Stopped
        && st3 == State::Killed
}

/// 终态注记（对账/落账用）。
pub fn state_note(st: &State) -> String {
    match st {
        State::Alive => "alive".to_string(),
        State::Stopped => "stopped".to_string(),
        State::Killed => format!("killed sig={}", SIGKILL),
    }
}

/// 判据（对应卡验收 J 判据）。
pub fn run_unx10821_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10821");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平
    s.add("J1-矩阵对平", hits == total && cap_ok, "七线处置样本全命中");

    // J2 反路径：9 装 Handler 必 EINVAL
    let j2 = judge(&Scenario { sig: SIGKILL, disp: Disp::Handler, mask: 0 })
        .err() == Some(POSIX_EINVAL);
    s.add("J2-KILL拒捕", j2, "SIGKILL 不可装处理器");

    // J3 反路径：19 忽略必 EINVAL
    let j3 = judge(&Scenario { sig: SIGSTOP, disp: Disp::Ign, mask: 0 })
        .err() == Some(POSIX_EINVAL);
    s.add("J3-STOP拒忽略", j3, "SIGSTOP 不可忽略");

    // J4 正路径：9/19 默认处置接受
    let j4 = judge(&Scenario { sig: SIGKILL, disp: Disp::Dflt, mask: 0 }).is_ok()
        && judge(&Scenario { sig: SIGSTOP, disp: Disp::Dflt, mask: 0 }).is_ok();
    s.add("J4-默认接受", j4, "默认处置可装");

    // J5 屏蔽位清洗：9/19 位寄存即丢
    let raw = (1u32 << SIGKILL) | (1u32 << SIGSTOP) | (1u32 << 2);
    let j5 = mask_clean(raw) == (1u32 << 2);
    s.add("J5-屏蔽清洗", j5, "9/19 位不入账");

    // J6 投递全序核：暂停→普通让路→必死
    s.add("J6-投递全序", deliver_seq_ok(), "9/19 一经投递必生效");

    // J7 注入1 可捕化检出
    s.add("J7-可捕化检出", inj[0], "装处理器放行坏实现必分歧");

    // J8 注入2 屏蔽穿透检出
    s.add("J8-穿透检出", inj[1], "屏蔽位保留坏实现必分歧");

    // J9 处置账重放核
    let mut lg = DispLedger::new();
    let _ = lg.install(&Scenario { sig: 15, disp: Disp::Handler, mask: 0 });
    let _ = lg.install(&Scenario { sig: SIGKILL, disp: Disp::Ign, mask: 0 });
    let _ = lg.install(&Scenario { sig: 1, disp: Disp::Ign, mask: 0 });
    let j9 = lg.replay_ok() && lg.uncatchable_clean() && lg.rows.len() == 3;
    s.add("J9-处置账重放", j9, "三行账逐条同判且不可捕行全拒");

    // J10 字面锚：9/19/22 三锚
    let j10 = SIGKILL == 9 && SIGSTOP == 19 && POSIX_EINVAL == 22;
    s.add("J10-字面锚对平", j10, "9/19/22 三锚");

    // J11 不可捕判定闭集：9/19 真、15/2 假
    let j11 = uncatchable(9) && uncatchable(19) && !uncatchable(15) && !uncatchable(2);
    s.add("J11-不可捕闭集", j11, "仅 9/19 不可捕");

    // J12 终态注记闭集
    let n1 = state_note(&State::Alive);
    let n2 = state_note(&State::Stopped);
    let n3 = state_note(&State::Killed);
    let j12 = n1.contains("alive") && n2.contains("stopped") && n3.contains("sig=9")
        && diag_text(UK_CATCHABLE).len() > 0;
    s.add("J12-终态注记", j12, "三态注记互异且语义完备");

    // J13 (dyn) 处置账指纹落账
    let mut lg2 = DispLedger::new();
    let _ = lg2.install(&Scenario { sig: 15, disp: Disp::Handler, mask: 0 });
    let _ = lg2.install(&Scenario { sig: 2, disp: Disp::Dflt, mask: 0 });
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(lg2.rows.len() as u32);
    let mut i = 0usize;
    while i < lg2.rows.len() {
        h = h.wrapping_mul(31).wrapping_add(lg2.rows[i].0);
        i += 1;
    }
    for c in lg2.note.bytes() {
        h = h.wrapping_mul(31).wrapping_add(c as u32);
    }
    s.add_dyn("J13-处置账指纹", h != 0, &format!("fold={:#x}", h));

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
    fn unx10821_uncatchable_flow() {
        assert!(judge(&Scenario { sig: SIGKILL, disp: Disp::Handler, mask: 0 })
            .err() == Some(POSIX_EINVAL));
        assert!(judge(&Scenario { sig: SIGSTOP, disp: Disp::Ign, mask: 0 })
            .err() == Some(POSIX_EINVAL));
        assert!(judge(&Scenario { sig: 15, disp: Disp::Handler, mask: 0 }).is_ok());
        assert_eq!(mask_clean(0b1111), 0b0000);
        let st = deliver(&State::Alive, SIGKILL, 1 << SIGKILL);
        assert_eq!(st, State::Killed);
        let inj = inject_two();
        assert!(inj.iter().all(|b| *b));
    }

    #[test]
    fn unx10821_checks_green() {
        let s = run_unx10821_checks();
        assert!(s.all_passed(), "UNX-F10821 判据未全绿");
    }
}
