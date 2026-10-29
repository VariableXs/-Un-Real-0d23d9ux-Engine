//! UNX-F10822 signal() 注册错误矩阵（EINVAL 非法信号号）（0x1bf40 段）
//!
//! 锚点：UNX-F10822-J1 注册对平——0 号必 EINVAL、超界（>NSIG 上界）必
//! EINVAL、9/19 号注册非默认必 EINVAL、合法号注册必落账可查三线全对平；
//! 注入坏实现（0 号放行/超界截断）必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：信号注册表（号 → 处置型闭集）+ 注册判定器（号域对撞算 EINVAL/
//! 落账）+ 注册状态机（装/查/还原默认）+ 重放核（逐笔先验同判）+ 注入核
//! （两类坏实现必分歧）+ 正反双样本矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX 锚：EINVAL=22、SIGKILL=9、SIGSTOP=19。
pub const POSIX_EINVAL: u32 = 22;
pub const SIGKILL: u32 = 9;
pub const SIGSTOP: u32 = 19;
/// 信号号上界（1..NSIG-1 合法域）。
pub const NSIG: u32 = 65;

/// 诊断段（0x1bf40 独占 0x20）。
pub const SEG_BASE: u32 = 0x1bf40;
pub const SR_ZERO_SIG: u32 = SEG_BASE + 0x01; // 0 号注册
pub const SR_OVER_SIG: u32 = SEG_BASE + 0x02; // 超界注册
pub const SR_UNCATCH_REG: u32 = SEG_BASE + 0x03; // 9/19 非默认注册
pub const SR_TABLE_LOSS: u32 = SEG_BASE + 0x04; // 注册表丢行
pub const SR_REPLAY_MISS: u32 = SEG_BASE + 0x05; // 重放不齐
pub const SR_NOTE_SKEW: u32 = SEG_BASE + 0x06; // 注记错账

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        SR_ZERO_SIG => "0 号注册",
        SR_OVER_SIG => "超界注册",
        SR_UNCATCH_REG => "9/19 非默认注册",
        SR_TABLE_LOSS => "注册表丢行",
        SR_REPLAY_MISS => "重放不齐",
        SR_NOTE_SKEW => "注记错账",
        _ => "未知诊断码",
    }
}

/// 处置型。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Disp {
    Dflt,
    Handler,
    Ign,
}

/// 场景输入：信号号 × 处置型。
#[derive(Clone)]
pub struct Scenario {
    pub sig: u32,
    pub disp: Disp,
}

/// 注册判定核：0 号/超界必 EINVAL；9/19 非默认必 EINVAL；合法落账。
pub fn judge(sc: &Scenario) -> Result<&'static str, u32> {
    if sc.sig == 0 || sc.sig >= NSIG {
        return Err(POSIX_EINVAL);
    }
    if (sc.sig == SIGKILL || sc.sig == SIGSTOP) && sc.disp != Disp::Dflt {
        return Err(POSIX_EINVAL);
    }
    match sc.disp {
        Disp::Dflt => Ok("default"),
        Disp::Handler => Ok("handler"),
        Disp::Ign => Ok("ignored"),
    }
}

/// 信号注册表状态机：装/查/还原默认。
pub struct SigTable {
    pub slots: Vec<(u32, Disp)>,
    pub note: String,
}

impl SigTable {
    pub fn new() -> SigTable {
        SigTable { slots: Vec::new(), note: "fresh".to_string() }
    }

    /// 注册：裁定通过才落表（覆盖同号旧处置）。
    pub fn install(&mut self, sc: &Scenario) -> Result<&'static str, u32> {
        let out = judge(sc);
        if let Ok(tag) = out {
            let mut hit = false;
            let mut i = 0usize;
            while i < self.slots.len() {
                if self.slots[i].0 == sc.sig {
                    self.slots[i].1 = sc.disp.clone();
                    hit = true;
                }
                i += 1;
            }
            if !hit {
                self.slots.push((sc.sig, sc.disp.clone()));
            }
            self.note = "installed".to_string();
        }
        out
    }

    /// 查处置：未装行为默认。
    pub fn lookup(&self, sig: u32) -> Disp {
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].0 == sig {
                return self.slots[i].1.clone();
            }
            i += 1;
        }
        Disp::Dflt
    }

    /// 还原默认。
    pub fn reset(&mut self, sig: u32) {
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].0 == sig {
                self.slots[i].1 = Disp::Dflt;
                self.note = "reset".to_string();
                return;
            }
            i += 1;
        }
    }

    /// 表账核：表内行号全在合法域且 9/19 行（若有）必默认。
    pub fn table_ok(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.slots.len() {
            let (sig, disp) = &self.slots[i];
            if *sig == 0 || *sig >= NSIG {
                ok = false;
            }
            if (*sig == SIGKILL || *sig == SIGSTOP) && *disp != Disp::Dflt {
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
    // 正：合法号注册
    m.push((Scenario { sig: 15, disp: Disp::Handler }, None));
    m.push((Scenario { sig: 1, disp: Disp::Ign }, None));
    // 正：上界内末号 NSIG-1
    m.push((Scenario { sig: NSIG - 1, disp: Disp::Handler }, None));
    // 正：9/19 默认
    m.push((Scenario { sig: SIGKILL, disp: Disp::Dflt }, None));
    m.push((Scenario { sig: SIGSTOP, disp: Disp::Dflt }, None));
    // 反：0 号
    m.push((Scenario { sig: 0, disp: Disp::Handler }, Some(POSIX_EINVAL)));
    // 反：超界 NSIG
    m.push((Scenario { sig: NSIG, disp: Disp::Handler }, Some(POSIX_EINVAL)));
    // 反：9 装 Handler
    m.push((Scenario { sig: SIGKILL, disp: Disp::Handler }, Some(POSIX_EINVAL)));
    // 反：19 忽略
    m.push((Scenario { sig: SIGSTOP, disp: Disp::Ign }, Some(POSIX_EINVAL)));
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

/// 坏实现 A：0 号放行。
pub fn faulty_zero(sc: &Scenario) -> Result<&'static str, u32> {
    if sc.sig == 0 {
        return Ok("default");
    }
    judge(sc)
}

/// 坏实现 B：超界截断（号对 NSIG 取模后注册）。
pub fn faulty_trunc(sc: &Scenario) -> Result<&'static str, u32> {
    let s2 = Scenario { sig: sc.sig % NSIG, disp: sc.disp.clone() };
    judge(&s2)
}

/// 注入核：两类坏实现必与权威分歧（检出）。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    // 1) 0 号：权威 EINVAL、坏实现放行 → 检出
    let sc1 = Scenario { sig: 0, disp: Disp::Handler };
    got[0] = judge(&sc1) != faulty_zero(&sc1) && judge(&sc1).err() == Some(POSIX_EINVAL);
    // 2) 超界 NSIG+3=68 → 68%65=3：权威 EINVAL、坏实现按 3 号放行 → 检出
    let sc2 = Scenario { sig: NSIG + 3, disp: Disp::Handler };
    got[1] = judge(&sc2) != faulty_trunc(&sc2) && judge(&sc2).err() == Some(POSIX_EINVAL);
    got
}

/// 注册全序核：装 15/1 → 覆盖 15 → 查账 → 还原 → 查默认。
pub fn install_seq_ok() -> bool {
    let mut t = SigTable::new();
    let r1 = t.install(&Scenario { sig: 15, disp: Disp::Handler });
    let r2 = t.install(&Scenario { sig: 1, disp: Disp::Ign });
    let r3 = t.install(&Scenario { sig: 15, disp: Disp::Ign });
    let ok1 = r1.is_ok() && r2.is_ok() && r3.is_ok();
    let lookup = t.lookup(15) == Disp::Ign && t.lookup(1) == Disp::Ign
        && t.lookup(2) == Disp::Dflt && t.slots.len() == 2;
    t.reset(15);
    let after = t.lookup(15) == Disp::Dflt && t.note == "reset";
    ok1 && lookup && after && t.table_ok()
}

/// 注入拒绝不落表核：EINVAL 行不入表。
pub fn reject_no_row_ok() -> bool {
    let mut t = SigTable::new();
    let _ = t.install(&Scenario { sig: 0, disp: Disp::Handler });
    let _ = t.install(&Scenario { sig: NSIG, disp: Disp::Handler });
    let _ = t.install(&Scenario { sig: SIGKILL, disp: Disp::Handler });
    t.slots.is_empty() && t.note == "fresh"
}

/// 注册面注记（对账/落账用）。
pub fn disp_note(d: &Disp) -> String {
    match d {
        Disp::Dflt => "default".to_string(),
        Disp::Handler => format!("handler(nsig={})", NSIG),
        Disp::Ign => "ignored".to_string(),
    }
}

/// 判据（对应卡验收 J 判据）。
pub fn run_unx10822_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10822");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平
    s.add("J1-矩阵对平", hits == total && cap_ok, "九线注册样本全命中");

    // J2 反路径：0 号必 EINVAL
    let j2 = judge(&Scenario { sig: 0, disp: Disp::Handler }).err() == Some(POSIX_EINVAL);
    s.add("J2-零号拒", j2, "sig=0 必 22");

    // J3 反路径：超界必 EINVAL
    let j3 = judge(&Scenario { sig: NSIG, disp: Disp::Handler }).err() == Some(POSIX_EINVAL)
        && judge(&Scenario { sig: 1000, disp: Disp::Ign }).err() == Some(POSIX_EINVAL);
    s.add("J3-超界拒", j3, "sig>=NSIG 必 22");

    // J4 反路径：9/19 非默认必 EINVAL
    let j4 = judge(&Scenario { sig: SIGKILL, disp: Disp::Handler }).err() == Some(POSIX_EINVAL)
        && judge(&Scenario { sig: SIGSTOP, disp: Disp::Ign }).err() == Some(POSIX_EINVAL);
    s.add("J4-不可捕拒", j4, "9/19 仅默认可装");

    // J5 正路径：合法号落账
    let j5 = judge(&Scenario { sig: 15, disp: Disp::Handler }).is_ok()
        && judge(&Scenario { sig: NSIG - 1, disp: Disp::Ign }).is_ok();
    s.add("J5-合法落账", j5, "1..NSIG-1 全域可装");

    // J6 注册全序核
    s.add("J6-注册全序", install_seq_ok(), "覆盖/查询/还原全对");

    // J7 注入1 零号放行检出
    s.add("J7-零号检出", inj[0], "0 号放行坏实现必分歧");

    // J8 注入2 超界截断检出
    s.add("J8-截断检出", inj[1], "取模截断坏实现必分歧");

    // J9 拒绝不落表核
    s.add("J9-拒零落表", reject_no_row_ok(), "EINVAL 行不入表");

    // J10 字面锚：EINVAL=22 / NSIG=65
    let j10 = POSIX_EINVAL == 22 && NSIG == 65 && diag_text(SR_ZERO_SIG).len() > 0;
    s.add("J10-字面锚对平", j10, "22/65 双锚且语义完备");

    // J11 处置闭集：三型互异
    let a = disp_note(&Disp::Dflt);
    let b = disp_note(&Disp::Handler);
    let c = disp_note(&Disp::Ign);
    let j11 = a != b && b != c && a != c;
    s.add("J11-三型互异", j11, "default/handler/ignored 注记互异");

    // J12 表账核（覆盖后行数恒定）
    let mut t2 = SigTable::new();
    let _ = t2.install(&Scenario { sig: 15, disp: Disp::Handler });
    let _ = t2.install(&Scenario { sig: 15, disp: Disp::Handler });
    let j12 = t2.slots.len() == 1 && t2.table_ok();
    s.add("J12-覆盖不增行", j12, "同号覆盖不增行");

    // J13 (dyn) 注册表指纹落账
    let mut t3 = SigTable::new();
    let _ = t3.install(&Scenario { sig: 15, disp: Disp::Handler });
    let _ = t3.install(&Scenario { sig: 1, disp: Disp::Ign });
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(t3.slots.len() as u32);
    let mut i = 0usize;
    while i < t3.slots.len() {
        h = h.wrapping_mul(31).wrapping_add(t3.slots[i].0);
        h = h.wrapping_mul(31).wrapping_add(match &t3.slots[i].1 {
            Disp::Dflt => 0,
            Disp::Handler => 1,
            Disp::Ign => 2,
        });
        i += 1;
    }
    for ch in t3.note.bytes() {
        h = h.wrapping_mul(31).wrapping_add(ch as u32);
    }
    s.add_dyn("J13-注册表指纹", h != 0, &format!("fold={:#x}", h));

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
    fn unx10822_register_flow() {
        let mut t = SigTable::new();
        assert!(t.install(&Scenario { sig: 0, disp: Disp::Handler }).err() == Some(POSIX_EINVAL));
        assert!(t.install(&Scenario { sig: NSIG, disp: Disp::Handler }).err() == Some(POSIX_EINVAL));
        assert!(t.install(&Scenario { sig: SIGKILL, disp: Disp::Handler }).err() == Some(POSIX_EINVAL));
        assert!(t.install(&Scenario { sig: 15, disp: Disp::Handler }).is_ok());
        assert_eq!(t.lookup(15), Disp::Handler);
        assert_eq!(t.lookup(3), Disp::Dflt);
        let inj = inject_two();
        assert!(inj.iter().all(|b| *b));
    }

    #[test]
    fn unx10822_checks_green() {
        let s = run_unx10822_checks();
        assert!(s.all_passed(), "UNX-F10822 判据未全绿");
    }
}
