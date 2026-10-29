//! UNX-F10802 SIGPIPE 三态矩阵（缺省/忽略/屏蔽）（0x1bcc0 段）
//!
//! 锚点：UNX-F10802-J1 SIGPIPE 三态对平——写断裂管道时缺省处置必出信号、
//! SIG_IGN 必静默、屏蔽必挂起三线裁决各归其位，三态下 errno 一律 EPIPE，
//! 读端存续三态皆无信号；注入坏处置器（屏蔽仍出信号/忽略仍投递/吞 EPIPE）
//! 必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：信号面状态机（处置三态×挂起位×投递事件）+ 写裁决器（读端计数
//! ×处置态算 SigOut/EPIPE）+ 挂起集进出核（屏蔽写入挂起、解除屏蔽即
//! 投递清位）+ 注入核（三态误裁三类必分歧）+ 正反双样本矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX 锚：SIGPIPE 信号号 13、EPIPE errno 32。
pub const POSIX_SIGPIPE: u32 = 13;
pub const POSIX_EPIPE: u32 = 32;

/// 诊断段（0x1bcc0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1bcc0;
pub const SP_BLOCKED_RAISED: u32 = SEG_BASE + 0x01; // 屏蔽态却出信号
pub const SP_IGNORED_DELIVERED: u32 = SEG_BASE + 0x02; // 忽略态却投递
pub const SP_SWALLOW_EPIPE: u32 = SEG_BASE + 0x03; // 断裂写吞掉 EPIPE
pub const SP_LIVE_SIG: u32 = SEG_BASE + 0x04; // 读端存续误出信号
pub const SP_PENDING_STUCK: u32 = SEG_BASE + 0x05; // 解除屏蔽后挂起位不清
pub const SP_DEFAULT_NOACT: u32 = SEG_BASE + 0x06; // 缺省态不出信号

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        SP_BLOCKED_RAISED => "屏蔽态却出信号",
        SP_IGNORED_DELIVERED => "忽略态却投递",
        SP_SWALLOW_EPIPE => "断裂写吞掉 EPIPE",
        SP_LIVE_SIG => "读端存续误出信号",
        SP_PENDING_STUCK => "解除屏蔽后挂起位不清",
        SP_DEFAULT_NOACT => "缺省态不出信号",
        _ => "未知诊断码",
    }
}

/// SIGPIPE 处置三态。
#[derive(Clone, PartialEq, Eq)]
pub enum Disp {
    /// 缺省（终止进程）。
    Dflt,
    /// SIG_IGN（忽略）。
    Ign,
    /// 被进程掩码屏蔽。
    Blocked,
}

/// 信号裁决输出。
#[derive(Clone, PartialEq, Eq)]
pub enum SigOut {
    /// 缺省态：信号触发（进程按缺省处置终止）。
    Raised,
    /// 屏蔽态：信号入挂起集。
    Pended,
    /// 忽略态：信号静默丢弃。
    Silent,
    /// 读端存续：无信号。
    None,
}

/// 场景输入：读端计数 × 处置态 × 写长。
#[derive(Clone)]
pub struct Scenario {
    pub readers: u32,
    pub disp: Disp,
    pub write_len: usize,
}

/// 写裁决核：断裂判定（读端计数归零且非零长写）与信号面三态裁决。
pub fn judge(sc: &Scenario) -> (SigOut, Result<usize, u32>) {
    if sc.write_len == 0 {
        return (SigOut::None, Ok(0));
    }
    if sc.readers > 0 {
        return (SigOut::None, Ok(sc.write_len));
    }
    let sig = match sc.disp {
        Disp::Dflt => SigOut::Raised,
        Disp::Ign => SigOut::Silent,
        Disp::Blocked => SigOut::Pended,
    };
    (sig, Err(POSIX_EPIPE))
}

/// 信号面状态机：处置态可改写，屏蔽期间断裂信号入挂起位，解除即投递。
pub struct SigFace {
    pub disp: Disp,
    pub pending: bool,
    pub delivered: u32,
    pub note: String,
}

impl SigFace {
    pub fn new() -> SigFace {
        SigFace { disp: Disp::Dflt, pending: false, delivered: 0, note: "init".to_string() }
    }

    /// 改写处置态（ign/block 互斥生效）。
    pub fn set_disp(&mut self, d: Disp) {
        self.note = match &d {
            Disp::Dflt => "disp=dflt".to_string(),
            Disp::Ign => "disp=ign".to_string(),
            Disp::Blocked => "disp=blocked".to_string(),
        };
        self.disp = d;
    }

    /// 断裂写落判：按当前处置态出信号/挂起/静默，errno 恒 EPIPE。
    pub fn on_broken_write(&mut self, len: usize) -> Result<usize, u32> {
        if len == 0 {
            return Ok(0);
        }
        match self.disp {
            Disp::Dflt => {
                self.pending = false;
                Err(POSIX_EPIPE)
            }
            Disp::Ign => {
                self.pending = false;
                Err(POSIX_EPIPE)
            }
            Disp::Blocked => {
                self.pending = true;
                Err(POSIX_EPIPE)
            }
        }
    }

    /// 解除屏蔽：挂起位清零并投递一次。
    pub fn unblock(&mut self) -> bool {
        if self.pending {
            self.pending = false;
            self.delivered += 1;
            true
        } else {
            false
        }
    }
}

/// 三态矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 16;

/// 正反双样本矩阵：期望 (SigOut, errno)。
pub fn sample_matrix() -> Vec<(Scenario, (SigOut, Option<u32>))> {
    let mut m: Vec<(Scenario, (SigOut, Option<u32>))> = Vec::new();
    // 正：读端存续三态皆无信号且放行
    m.push((Scenario { readers: 1, disp: Disp::Dflt, write_len: 8 }, (SigOut::None, None)));
    m.push((Scenario { readers: 2, disp: Disp::Ign, write_len: 8 }, (SigOut::None, None)));
    m.push((Scenario { readers: 3, disp: Disp::Blocked, write_len: 8 }, (SigOut::None, None)));
    // 正：零长写免检
    m.push((Scenario { readers: 0, disp: Disp::Dflt, write_len: 0 }, (SigOut::None, None)));
    // 反：断裂写三态 errno 恒 EPIPE，信号面各归其位
    m.push((
        Scenario { readers: 0, disp: Disp::Dflt, write_len: 8 },
        (SigOut::Raised, Some(POSIX_EPIPE)),
    ));
    m.push((
        Scenario { readers: 0, disp: Disp::Ign, write_len: 8 },
        (SigOut::Silent, Some(POSIX_EPIPE)),
    ));
    m.push((
        Scenario { readers: 0, disp: Disp::Blocked, write_len: 8 },
        (SigOut::Pended, Some(POSIX_EPIPE)),
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
        let (sig, ret) = judge(sc);
        let hit = sig == want.0
            && match want.1 {
                None => ret.is_ok(),
                Some(c) => ret.err() == Some(c),
            };
        if hit {
            ok += 1;
        }
        i += 1;
    }
    (ok, m.len(), m.len() <= MATRIX_CAP)
}

/// 坏实现 A：屏蔽态仍出信号。
pub fn faulty_block_raise(sc: &Scenario) -> SigOut {
    if sc.readers == 0 && sc.write_len > 0 {
        SigOut::Raised
    } else {
        SigOut::None
    }
}

/// 坏实现 B：忽略态仍投递信号。
pub fn faulty_ignore_deliver(sc: &Scenario) -> SigOut {
    if sc.readers == 0 && sc.write_len > 0 {
        SigOut::Pended
    } else {
        SigOut::None
    }
}

/// 坏实现 C：忽略态吞掉 EPIPE（写假成功）。
pub fn faulty_swallow(sc: &Scenario) -> Result<usize, u32> {
    if sc.disp == Disp::Ign && sc.readers == 0 {
        Ok(sc.write_len)
    } else {
        judge(sc).1
    }
}

/// 挂起集进出核：屏蔽断裂写 → 挂起位置位 → 解除即投递清位。
pub fn pending_cycle_ok() -> bool {
    let mut f = SigFace::new();
    f.set_disp(Disp::Blocked);
    let r1 = f.on_broken_write(8);
    let p1 = f.pending;
    let delivered = f.unblock();
    r1.err() == Some(POSIX_EPIPE) && p1 && delivered && !f.pending && f.delivered == 1
}

/// 注入核：三类坏实现必与权威分歧（检出）。
pub fn inject_three() -> [bool; 3] {
    let mut got = [false; 3];
    let sc = Scenario { readers: 0, disp: Disp::Blocked, write_len: 8 };
    // 1) 屏蔽态出信号坏实现 → 权威裁 Pended → 分歧检出
    got[0] = judge(&sc).0 != faulty_block_raise(&sc) && judge(&sc).0 == SigOut::Pended;
    let sc2 = Scenario { readers: 0, disp: Disp::Ign, write_len: 8 };
    // 2) 忽略态投递坏实现 → 权威裁 Silent → 分歧检出
    got[1] = judge(&sc2).0 != faulty_ignore_deliver(&sc2) && judge(&sc2).0 == SigOut::Silent;
    // 3) 忽略态吞 EPIPE 坏实现 → 权威判 EPIPE → 分歧检出
    got[2] = judge(&sc2).1 != faulty_swallow(&sc2) && judge(&sc2).1.err() == Some(POSIX_EPIPE);
    got
}

/// 状态机折叠指纹（处置态+挂起位+投递数）。
pub fn fold_face(f: &SigFace) -> u32 {
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(match f.disp {
        Disp::Dflt => 1,
        Disp::Ign => 2,
        Disp::Blocked => 3,
    });
    h = h.wrapping_mul(31).wrapping_add(f.pending as u32);
    h = h.wrapping_mul(31).wrapping_add(f.delivered);
    for b in f.note.bytes() {
        h = h.wrapping_mul(31).wrapping_add(b as u32);
    }
    h
}

/// SIGPIPE 三态矩阵判据（对应卡验收 J 判据）。
pub fn run_unx10802_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10802");
    let inj = inject_three();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平：三态正反样本全命中
    s.add("J1-三态矩阵对平", hits == total && cap_ok, "缺省/忽略/屏蔽三线全命中");

    // J2 缺省态：断裂写必出信号且 errno=EPIPE
    let sc_d = Scenario { readers: 0, disp: Disp::Dflt, write_len: 8 };
    let j2 = judge(&sc_d).0 == SigOut::Raised && judge(&sc_d).1.err() == Some(POSIX_EPIPE);
    s.add("J2-缺省出信号", j2, "缺省处置断裂写 Raised+EPIPE");

    // J3 忽略态：静默但 errno=EPIPE
    let sc_i = Scenario { readers: 0, disp: Disp::Ign, write_len: 8 };
    let j3 = judge(&sc_i).0 == SigOut::Silent && judge(&sc_i).1.err() == Some(POSIX_EPIPE);
    s.add("J3-忽略静默断写", j3, "SIG_IGN 断裂写无信号仍 EPIPE");

    // J4 屏蔽态：信号入挂起集且 errno=EPIPE
    let sc_b = Scenario { readers: 0, disp: Disp::Blocked, write_len: 8 };
    let j4 = judge(&sc_b).0 == SigOut::Pended && judge(&sc_b).1.err() == Some(POSIX_EPIPE);
    s.add("J4-屏蔽入挂起", j4, "屏蔽处置断裂写 Pended+EPIPE");

    // J5 读端存续：三态皆无信号且放行
    let j5 = judge(&Scenario { readers: 1, disp: Disp::Dflt, write_len: 8 }).1.is_ok()
        && judge(&Scenario { readers: 2, disp: Disp::Ign, write_len: 8 }).1.is_ok()
        && judge(&Scenario { readers: 3, disp: Disp::Blocked, write_len: 8 }).1.is_ok();
    s.add("J5-存续皆放行", j5, "读端存续无信号");

    // J6 挂起集进出：屏蔽写挂起、解除投递清位
    s.add("J6-挂起进出核", pending_cycle_ok(), "屏蔽置位→解除投递清位");

    // J7 零长写免检（缺省态断裂管上）
    let j7 = judge(&Scenario { readers: 0, disp: Disp::Dflt, write_len: 0 }).1.is_ok();
    s.add("J7-零长免检", j7, "零长写不触信号面");

    // J8 注入1 屏蔽态出信号检出
    s.add("J8-屏蔽出信号检出", inj[0], "坏实现屏蔽态 Raised 必分歧");

    // J9 注入2 忽略态投递检出
    s.add("J9-忽略投递检出", inj[1], "坏实现忽略态 Pended 必分歧");

    // J10 注入3 吞 EPIPE 检出
    s.add("J10-吞EPIPE检出", inj[2], "忽略态假成功必分歧");

    // J11 字面锚：SIGPIPE=13 / EPIPE=32 且诊断表完备
    let j11 = POSIX_SIGPIPE == 13
        && POSIX_EPIPE == 32
        && diag_text(SP_BLOCKED_RAISED).len() > 0
        && diag_text(0xffff_ffff) == "未知诊断码";
    s.add("J11-字面锚对平", j11, "13/32 双锚且诊断表完备");

    // J12 处置态改写往返：Dflt→Ign→Blocked 状态机记账
    let mut f = SigFace::new();
    f.set_disp(Disp::Ign);
    let n1 = f.note.clone();
    f.set_disp(Disp::Blocked);
    let n2 = f.note.clone();
    let j12 = n1 == "disp=ign" && n2 == "disp=blocked";
    s.add("J12-处置态改写", j12, "三态切换记账");

    // J13 (dyn) 状态机指纹落账
    let fp = fold_face(&f);
    s.add_dyn("J13-状态机指纹", fp != 0, &format!("fold={:#x}", fp));

    // J14 (dyn) 挂起投递计数明细
    let mut f2 = SigFace::new();
    f2.set_disp(Disp::Blocked);
    let _ = f2.on_broken_write(4);
    let _ = f2.on_broken_write(4);
    let d1 = f2.unblock();
    let d2 = f2.unblock();
    s.add_dyn(
        "J14-挂起计数",
        d1 && !d2 && f2.delivered == 1,
        &format!("first={} second={} delivered={}", d1, d2, f2.delivered),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10802_sigpipe_flow() {
        let mut f = SigFace::new();
        f.set_disp(Disp::Blocked);
        assert!(f.on_broken_write(8).err() == Some(POSIX_EPIPE));
        assert!(f.pending);
        f.set_disp(Disp::Ign);
        assert!(f.on_broken_write(8).err() == Some(POSIX_EPIPE));
        assert!(!f.pending);
        let sc = Scenario { readers: 0, disp: Disp::Dflt, write_len: 16 };
        let (sig, ret) = judge(&sc);
        assert!(sig == SigOut::Raised);
        assert!(ret.err() == Some(POSIX_EPIPE));
        assert!(pending_cycle_ok());
    }

    #[test]
    fn unx10802_checks_green() {
        let s = run_unx10802_checks();
        assert!(s.all_passed(), "UNX-F10802 判据未全绿");
    }
}
