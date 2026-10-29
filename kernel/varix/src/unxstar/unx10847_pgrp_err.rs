//! UNX-F10847 TIOCGPGRP/TIOCSPGRP 错误矩阵（前景组）（0x1c260 段）
//!
//! 锚点：UNX-F10847-J1 前台进程组两 ioctl 错误面——fd 非终端必 ENOTTY
//! （fd 层先判）；TIOCSPGRP 置 pgrp<=0 必 EINVAL（域层）；pgrp 不在
//! 该终端会话内必 EPERM（会话层）；置后 TIOCGPGRP 恒见同值。正反双
//! 样本对账，注入核（无 ENOTTY 层/零值放水）必检出。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：ioctl 场景（is_tty × cmd × pgrp × 会话成员）→ 三层判定器
//! （fd→域→会话）→ 终端会话账状态机（前景组置换/读账）→ 组样本矩阵
//! → 注入核（两坏实现必分歧）→ 置读回放（合法置换序列对平）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1c260 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c260;
pub const PG_NOT_TTY: u32 = SEG_BASE + 0x01; // fd 非终端（ENOTTY 面）
pub const PG_BAD_PGRP: u32 = SEG_BASE + 0x02; // pgrp<=0（EINVAL 面）
pub const PG_FOREIGN: u32 = SEG_BASE + 0x03; // pgrp 不在会话（EPERM 面）
pub const PG_GET_OK: u32 = SEG_BASE + 0x04; // 读面放行
pub const PG_SET_OK: u32 = SEG_BASE + 0x05; // 写面放行
pub const PG_LEDGER_LOST: u32 = SEG_BASE + 0x06; // 置读账丢失

/// 语义字面锚。
pub const ENOTTY: u32 = 25;
pub const TIOCGPGRP: u32 = 1;
pub const TIOCSPGRP: u32 = 2;

/// 诊断码语义文本。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        PG_NOT_TTY => "fd 非终端 ENOTTY",
        PG_BAD_PGRP => "pgrp 非正 EINVAL",
        PG_FOREIGN => "pgrp 不在会话 EPERM",
        PG_GET_OK => "读面放行",
        PG_SET_OK => "写面放行",
        PG_LEDGER_LOST => "置读账丢失",
        _ => "未知诊断码",
    }
}

/// ioctl 命令：取前景组 / 置前景组(pgrp)。
#[derive(Clone, PartialEq)]
pub enum PgrpCmd {
    Get,
    Set(i64),
}

/// ioctl 场景：fd 是否终端 × 命令 × 会话成员表（判定器三输入）。
#[derive(Clone)]
pub struct PgrpReq {
    pub is_tty: bool,
    pub cmd: PgrpCmd,
    pub members: Vec<u32>,
}

/// 判定结果：Get/Set 返回前景组，拒绝携带诊断码。
#[derive(Clone, PartialEq, Debug)]
pub enum Res {
    Pgrp(u32),
    Err(u32),
}

/// 会话成员核：pgrp 必须落成员表。
pub fn in_session(pgrp: u32, members: &[u32]) -> bool {
    let mut hit = false;
    let mut i = 0usize;
    while i < members.len() {
        if members[i] == pgrp {
            hit = true;
        }
        i += 1;
    }
    hit
}

/// 三层判定器：fd 层（ENOTTY）→ 域层（pgrp<=0 EINVAL）→ 会话层
/// （EPERM）→ 放行。全部分支依赖场景字段计算。
pub fn judge(req: &PgrpReq) -> Res {
    if !req.is_tty {
        return Res::Err(PG_NOT_TTY);
    }
    match &req.cmd {
        PgrpCmd::Get => {
            if req.members.is_empty() {
                Res::Pgrp(0)
            } else {
                Res::Pgrp(req.members[0])
            }
        }
        PgrpCmd::Set(p) => {
            if *p <= 0 {
                Res::Err(PG_BAD_PGRP)
            } else if !in_session(*p as u32, &req.members) {
                Res::Err(PG_FOREIGN)
            } else {
                Res::Pgrp(*p as u32)
            }
        }
    }
}

/// 终端会话账状态机：前景组、成员表、置读账。
pub struct TtySession {
    pub is_tty: bool,
    pub fg: u32,
    pub members: Vec<u32>,
    pub sets: u32,
    pub gets: u32,
    pub note: String,
}

impl TtySession {
    pub fn new(is_tty: bool, members: Vec<u32>) -> TtySession {
        TtySession {
            is_tty,
            fg: 0,
            members,
            sets: 0,
            gets: 0,
            note: "idle".to_string(),
        }
    }

    /// TIOCGPGRP：非终端 ENOTTY；否则回前景组并落读账。
    pub fn get_fg(&mut self) -> Result<u32, u32> {
        if !self.is_tty {
            return Err(PG_NOT_TTY);
        }
        self.gets += 1;
        self.note = "read".to_string();
        Ok(self.fg)
    }

    /// TIOCSPGRP：非终端 ENOTTY → 非正 EINVAL → 会话外 EPERM → 提交。
    pub fn set_fg(&mut self, pgrp: u32) -> Result<(), u32> {
        if !self.is_tty {
            return Err(PG_NOT_TTY);
        }
        if pgrp == 0 {
            self.note = "bad-pgrp".to_string();
            return Err(PG_BAD_PGRP);
        }
        if !in_session(pgrp, &self.members) {
            self.note = "foreign".to_string();
            return Err(PG_FOREIGN);
        }
        self.fg = pgrp;
        self.sets += 1;
        self.note = "swapped".to_string();
        Ok(())
    }

    /// 置读账一致：读账随写账单调，前景组恒落成员表。
    pub fn ledger_ok(&self) -> bool {
        self.gets <= self.sets + 1 && (self.fg == 0 || in_session(self.fg, &self.members))
    }
}

/// 样本矩阵容量。
pub const MATRIX_CAP: usize = 20;

/// 矩阵逐格判定：命中计数（期望码 0 表放行）。
pub fn verify_matrix(reqs: &[PgrpReq], wants: &[u32]) -> (usize, usize, bool) {
    let mut hits = 0usize;
    let total = reqs.len();
    let mut i = 0usize;
    while i < total && i < MATRIX_CAP {
        let want = if i < wants.len() { wants[i] } else { 0 };
        let hit = match judge(&reqs[i]) {
            Res::Pgrp(_) => want == PG_GET_OK,
            Res::Err(c) => c == want,
        };
        if hit {
            hits += 1;
        }
        i += 1;
    }
    (hits, total.min(MATRIX_CAP), hits == total.min(MATRIX_CAP))
}

/// 样本矩阵：tty/非 tty × get/set × 域/会话三沿，8 行全分账。
pub fn sample_matrix() -> (Vec<PgrpReq>, Vec<u32>) {
    let mut reqs: Vec<PgrpReq> = Vec::new();
    let mut wants: Vec<u32> = Vec::new();
    let mem = || {
        let mut v: Vec<u32> = Vec::new();
        v.push(100);
        v.push(200);
        v.push(300);
        v
    };
    // R0 非 tty get → ENOTTY
    reqs.push(PgrpReq { is_tty: false, cmd: PgrpCmd::Get, members: mem() });
    wants.push(PG_NOT_TTY);
    // R1 非 tty set（组内合法值也拒）
    reqs.push(PgrpReq { is_tty: false, cmd: PgrpCmd::Set(200), members: mem() });
    wants.push(PG_NOT_TTY);
    // R2 pgrp=0 → EINVAL
    reqs.push(PgrpReq { is_tty: true, cmd: PgrpCmd::Set(0), members: mem() });
    wants.push(PG_BAD_PGRP);
    // R3 pgrp=-5 → EINVAL
    reqs.push(PgrpReq { is_tty: true, cmd: PgrpCmd::Set(-5), members: mem() });
    wants.push(PG_BAD_PGRP);
    // R4 组外 999 → EPERM
    reqs.push(PgrpReq { is_tty: true, cmd: PgrpCmd::Set(999), members: mem() });
    wants.push(PG_FOREIGN);
    // R5 组内置 300 → 放行
    reqs.push(PgrpReq { is_tty: true, cmd: PgrpCmd::Set(300), members: mem() });
    wants.push(PG_GET_OK);
    // R6 get 空会话 → 前景 0
    reqs.push(PgrpReq { is_tty: true, cmd: PgrpCmd::Get, members: Vec::new() });
    wants.push(PG_GET_OK);
    // R7 get 有会话 → 首成员面
    reqs.push(PgrpReq { is_tty: true, cmd: PgrpCmd::Get, members: mem() });
    wants.push(PG_GET_OK);
    (reqs, wants)
}

/// 注入 A：坏实现无 ENOTTY 层。
pub fn faulty_no_enotty(req: &PgrpReq) -> Res {
    let mut fixed = req.clone();
    fixed.is_tty = true;
    judge(&fixed)
}

/// 注入 B：坏实现零值/负值放水（pgrp<=0 亦提交）。
pub fn faulty_zero_ok(req: &PgrpReq) -> Res {
    match &req.cmd {
        PgrpCmd::Set(p) if req.is_tty && *p <= 0 => Res::Pgrp((*p).unsigned_abs() as u32),
        _ => judge(req),
    }
}

/// 注入核：两处坏账与判定器必分歧。
pub fn inject_two() -> [bool; 2] {
    let mut mem: Vec<u32> = Vec::new();
    mem.push(100);
    mem.push(200);
    let notty = PgrpReq { is_tty: false, cmd: PgrpCmd::Get, members: mem.clone() };
    let zero = PgrpReq { is_tty: true, cmd: PgrpCmd::Set(0), members: mem };
    let d1 = judge(&notty) != faulty_no_enotty(&notty);
    let d2 = judge(&zero) != faulty_zero_ok(&zero);
    [d1, d2]
}

/// 置读回放：合法置换序列 get/set 交替，终值对平。
pub fn swap_replay_ok() -> bool {
    let mut mem: Vec<u32> = Vec::new();
    mem.push(10);
    mem.push(20);
    let mut ts = TtySession::new(true, mem);
    let s1 = ts.set_fg(10);
    let g1 = ts.get_fg();
    let s2 = ts.set_fg(20);
    let g2 = ts.get_fg();
    let bad = ts.set_fg(999);
    let g3 = ts.get_fg();
    s1.is_ok() && s2.is_ok() && bad.err() == Some(PG_FOREIGN)
        && g1 == Ok(10) && g2 == Ok(20) && g3 == Ok(20)
        && ts.sets == 2 && ts.gets == 3 && ts.ledger_ok()
}

/// 非终端核：get/set 双调皆 ENOTTY，账面零污染。
pub fn notty_face_ok() -> bool {
    let mut mem: Vec<u32> = Vec::new();
    mem.push(7);
    let mut ts = TtySession::new(false, mem);
    let g = ts.get_fg();
    let st = ts.set_fg(7);
    g.err() == Some(PG_NOT_TTY) && st.err() == Some(PG_NOT_TTY)
        && ts.fg == 0 && ts.sets == 0 && ts.gets == 0 && ts.ledger_ok()
}

/// 前景组注记：判定渲染。
pub fn pgrp_note(r: &Res) -> String {
    match r {
        Res::Pgrp(p) => format!("pgrp={}", p),
        Res::Err(c) => diag_text(*c).to_string(),
    }
}

/// 域判据（对应卡验收 J 判据）。
pub fn run_unx10847_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10847");
    let (reqs, wants) = sample_matrix();
    let (hits, total, cap_ok) = verify_matrix(&reqs, &wants);

    // J1 正路径：组内置 200 放行
    let mut mem: Vec<u32> = Vec::new();
    mem.push(100);
    mem.push(200);
    let j1 = judge(&PgrpReq { is_tty: true, cmd: PgrpCmd::Set(200), members: mem.clone() })
        == Res::Pgrp(200);
    s.add("J1-组内置入", j1, "会话内 pgrp 放行");

    // J2 域边界：pgrp=1 放行 / pgrp=0 拒绝
    let j2 = judge(&PgrpReq { is_tty: true, cmd: PgrpCmd::Set(1), members: mem.clone() })
        == Res::Pgrp(1)
        && judge(&PgrpReq { is_tty: true, cmd: PgrpCmd::Set(0), members: mem.clone() })
            == Res::Err(PG_BAD_PGRP);
    s.add("J2-零域边界", j2, "1/0 两沿可辨");

    // J3 会话层：组外 999 必 EPERM 面
    let j3 = judge(&PgrpReq { is_tty: true, cmd: PgrpCmd::Set(999), members: mem.clone() })
        == Res::Err(PG_FOREIGN);
    s.add("J3-组外 EPERM", j3, "会话外进程组必拒");

    // J4 fd 层：非 tty 双命令皆 ENOTTY
    let j4 = judge(&PgrpReq { is_tty: false, cmd: PgrpCmd::Get, members: mem.clone() })
        == Res::Err(PG_NOT_TTY)
        && judge(&PgrpReq { is_tty: false, cmd: PgrpCmd::Set(100), members: mem.clone() })
            == Res::Err(PG_NOT_TTY);
    s.add("J4-fd 层先判", j4, "ENOTTY 覆盖两会话面");

    // J5 置读回放
    s.add("J5-置读回放", swap_replay_ok(), "10→20 置换与读账对平");

    // J6 非终端核（零污染）
    s.add("J6-非终端零污染", notty_face_ok(), "账面回零");

    // J7 样本矩阵全命中
    s.add("J7-矩阵对平", cap_ok, "8 行三层全命中");

    // J8 注入1 ENOTTY 缺失检出
    let inj = inject_two();
    s.add("J8-ENOTTY 缺失检出", inj[0], "非 tty 误判必分歧");

    // J9 注入2 零值放水检出
    s.add("J9-零值放水检出", inj[1], "pgrp=0 误放必分歧");

    // J10 置读账一致
    let mut mem2: Vec<u32> = Vec::new();
    mem2.push(50);
    let mut ts = TtySession::new(true, mem2);
    let _ = ts.set_fg(50);
    let _ = ts.get_fg();
    s.add("J10-置读账", ts.ledger_ok() && ts.fg == 50, "前景组终值对平");

    // J11 字面锚：ENOTTY=25 / 成员表可辨
    let j11 = ENOTTY == 25 && in_session(200, &mem) && !in_session(999, &mem);
    s.add("J11-字面锚", j11, "errno 与成员核");

    // J12 诊断码闭集
    let j12 = diag_text(PG_NOT_TTY) == "fd 非终端 ENOTTY"
        && diag_text(PG_FOREIGN) == "pgrp 不在会话 EPERM"
        && diag_text(9) == "未知诊断码";
    s.add("J12-码文本闭集", j12, "六码文本可辨");

    // J13 (dyn) 矩阵明细落账
    s.add_dyn("J13-矩阵明细", hits == total, &format!("hits={}/{} cap_ok={}", hits, total, cap_ok));

    // J14 (dyn) 前景组注记落账
    let note = pgrp_note(&judge(&PgrpReq { is_tty: true, cmd: PgrpCmd::Set(300), members: mem }));
    s.add_dyn("J14-前景注记", note == "pgrp=300", &note);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10847_flow() {
        let mut mem: Vec<u32> = Vec::new();
        mem.push(100);
        mem.push(200);
        let mut ts = TtySession::new(false, mem.clone());
        assert!(ts.get_fg().err() == Some(PG_NOT_TTY));
        assert!(ts.set_fg(100).err() == Some(PG_NOT_TTY));
        let mut tty = TtySession::new(true, mem);
        assert!(tty.set_fg(0).err() == Some(PG_BAD_PGRP));
        assert!(tty.set_fg(999).err() == Some(PG_FOREIGN));
        assert!(tty.set_fg(200).is_ok());
        assert_eq!(tty.get_fg(), Ok(200));
        assert_eq!(judge(&PgrpReq { is_tty: true, cmd: PgrpCmd::Set(-3), members: Vec::new() }), Res::Err(PG_BAD_PGRP));
        assert!(in_session(200, &tty.members) && !in_session(999, &tty.members));
    }

    #[test]
    fn unx10847_checks_green() {
        let s = run_unx10847_checks();
        assert!(s.all_passed(), "UNX-F10847 判据未全绿");
    }
}
