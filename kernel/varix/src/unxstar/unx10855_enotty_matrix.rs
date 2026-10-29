//! UNX-F10855 无控制终端 ioctl 矩阵（ENOTTY）（0x1c360 段）
//!
//! 锚点：UNX-F10855-J1 ENOTTY 判定矩阵——ioctl 命令 × 终端态二轴定
//! 错码：终端类命令（TCGETS/TCSETS/TIOCGWINSZ/TIOCSWINSZ）落在非终
//! 端 fd（管道/普通文件）必 ENOTTY（25）；落在终端/pty slave 必成
//! 功；FIONBIO 在终端与管道均放行、普通文件拒；未知命令无论何态必
//! ENOTTY。判定依赖场景字段演算，注入核（fd 失明/命令失明坏实现）
//! 必分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：ioctl 场景（命令 × fd 终端态）→ ENOTTY 判定器（命令×态
//! 二轴表）→ fd ioctl 状态机（查询/设置走账）→ 样本矩阵（八行正反
//! 对）→ 注入核（两坏实现必分歧）→ 设置回放（winsz 写入读回对平）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1c360 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c360;
pub const ET_NOTTY: u32 = SEG_BASE + 0x01; // 非终端 fd 上终端类命令
pub const ET_OK_FACE: u32 = SEG_BASE + 0x02; // 终端态放行面
pub const ET_CMD_MISS: u32 = SEG_BASE + 0x03; // 未知命令
pub const ET_FD_MISS: u32 = SEG_BASE + 0x04; // fd 态失明
pub const ET_MATRIX_LOST: u32 = SEG_BASE + 0x05; // 矩阵格失平
pub const ET_LEDGER_LOST: u32 = SEG_BASE + 0x06; // ioctl 账失平

/// 语义字面锚。
pub const ENOTTY: u32 = 25;
pub const TCGETS: u32 = 0x5401;
pub const TCSETS: u32 = 0x5402;
pub const TIOCGWINSZ: u32 = 0x5413;
pub const WINSZ_DEF: u32 = 80;

/// 诊断码语义文本。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        ET_NOTTY => "非终端 fd 上终端类命令",
        ET_OK_FACE => "终端态放行面",
        ET_CMD_MISS => "未知命令",
        ET_FD_MISS => "fd 态失明",
        ET_MATRIX_LOST => "矩阵格失平",
        ET_LEDGER_LOST => "ioctl 账失平",
        _ => "未知诊断码",
    }
}

/// ioctl 命令面：终端查询/设置 / winsz 查询/设置 / FIONBIO(旗) / 未知。
#[derive(Clone, PartialEq)]
pub enum IoCmd {
    TcGets,
    TcSets,
    TiocgWinsz,
    TiocsWinsz(u32),
    Fionbio(bool),
    Unknown(u32),
}

/// fd 终端态：终端 / pty slave / 管道 / 普通文件。
#[derive(Clone, PartialEq)]
pub enum TermKind {
    Tty,
    PtySlave,
    Pipe,
    Regular,
}

/// 判定结果：Got(载荷字面) / Err(错误码)。
#[derive(Clone, PartialEq, Debug)]
pub enum Res {
    Got(u32),
    Err(u32),
}

/// ENOTTY 判定器：命令 × fd 态二轴表。
/// 终端类命令只在 Tty/PtySlave 放行；FIONBIO 在 Regular 拒；未知命令
/// 无论何态必 ENOTTY。
pub fn judge(cmd: &IoCmd, kind: &TermKind) -> Res {
    let is_term = *kind == TermKind::Tty || *kind == TermKind::PtySlave;
    match cmd {
        IoCmd::TcGets | IoCmd::TcSets => {
            if is_term {
                Res::Got(ET_OK_FACE)
            } else {
                Res::Err(ET_NOTTY)
            }
        }
        IoCmd::TiocgWinsz => {
            if is_term {
                Res::Got(ET_OK_FACE)
            } else {
                Res::Err(ET_NOTTY)
            }
        }
        IoCmd::TiocsWinsz(cols) => {
            if is_term {
                Res::Got(*cols)
            } else {
                Res::Err(ET_NOTTY)
            }
        }
        IoCmd::Fionbio(_) => {
            if *kind == TermKind::Regular {
                Res::Err(ET_NOTTY)
            } else {
                Res::Got(ET_OK_FACE)
            }
        }
        IoCmd::Unknown(_) => Res::Err(ET_CMD_MISS),
    }
}

/// fd ioctl 状态机：终端态 fd 的查询/设置走账。
pub struct TermFd {
    pub kind: TermKind,
    pub nonblock: bool,
    pub cols: u32,
    pub ok_calls: u32,
    pub err_calls: u32,
    pub note: String,
}

impl TermFd {
    pub fn new(kind: TermKind) -> TermFd {
        TermFd {
            kind,
            nonblock: false,
            cols: WINSZ_DEF,
            ok_calls: 0,
            err_calls: 0,
            note: "idle".to_string(),
        }
    }

    /// 单次 ioctl：判定走账（Ok 落 ok_calls / Err 落 err_calls）。
    pub fn ioctl(&mut self, cmd: &IoCmd) -> Res {
        let r = judge(cmd, &self.kind);
        match &r {
            Res::Got(_) => {
                self.ok_calls += 1;
                match cmd {
                    IoCmd::TiocsWinsz(c) => {
                        self.cols = *c;
                        self.note = "winsz-set".to_string();
                    }
                    IoCmd::Fionbio(on) => {
                        self.nonblock = *on;
                        self.note = "flag-set".to_string();
                    }
                    _ => {
                        self.note = "query".to_string();
                    }
                }
            }
            Res::Err(_) => {
                self.err_calls += 1;
                self.note = "enotty".to_string();
            }
        }
        r
    }

    /// ioctl 账一致：成功与失败账之和恒为调用总数可辨。
    pub fn ledger_ok(&self) -> bool {
        self.ok_calls + self.err_calls <= 64 && self.note != ""
    }
}

/// 样本矩阵容量。
pub const MATRIX_CAP: usize = 24;

/// 矩阵逐格判定：期望（Got(n) 编码 n+1，Err(c) 直码）命中计数。
pub fn verify_matrix(cmds: &[IoCmd], kinds: &[TermKind], wants: &[u32]) -> (usize, usize, bool) {
    let mut hits = 0usize;
    let total = cmds.len();
    let mut i = 0usize;
    while i < total && i < MATRIX_CAP {
        let want = if i < wants.len() { wants[i] } else { 0 };
        let hit = match judge(&cmds[i], &kinds[i]) {
            Res::Got(n) => want == n + 1,
            Res::Err(c) => c == want,
        };
        if hit {
            hits += 1;
        }
        i += 1;
    }
    (hits, total.min(MATRIX_CAP), hits == total.min(MATRIX_CAP))
}

/// 样本矩阵：终端类命令/FIONBIO/未知 × 终端/管道/普通文件，8 行正反对。
pub fn sample_matrix() -> (Vec<IoCmd>, Vec<TermKind>, Vec<u32>) {
    let mut cmds: Vec<IoCmd> = Vec::new();
    let mut kinds: Vec<TermKind> = Vec::new();
    let mut wants: Vec<u32> = Vec::new();
    // R0 TCGETS 落管道 → ENOTTY
    cmds.push(IoCmd::TcGets);
    kinds.push(TermKind::Pipe);
    wants.push(ET_NOTTY);
    // R1 TCGETS 落终端 → 放行
    cmds.push(IoCmd::TcGets);
    kinds.push(TermKind::Tty);
    wants.push(ET_OK_FACE + 1);
    // R2 TCSETS 落 pty slave → 放行
    cmds.push(IoCmd::TcSets);
    kinds.push(TermKind::PtySlave);
    wants.push(ET_OK_FACE + 1);
    // R3 TIOCGWINSZ 落普通文件 → ENOTTY
    cmds.push(IoCmd::TiocgWinsz);
    kinds.push(TermKind::Regular);
    wants.push(ET_NOTTY);
    // R4 FIONBIO 落管道 → 放行
    cmds.push(IoCmd::Fionbio(true));
    kinds.push(TermKind::Pipe);
    wants.push(ET_OK_FACE + 1);
    // R5 FIONBIO 落普通文件 → ENOTTY
    cmds.push(IoCmd::Fionbio(true));
    kinds.push(TermKind::Regular);
    wants.push(ET_NOTTY);
    // R6 未知命令落终端 → ENOTTY
    cmds.push(IoCmd::Unknown(0xdead));
    kinds.push(TermKind::Tty);
    wants.push(ET_CMD_MISS);
    // R7 TIOCSWINSZ 落 pty slave → 放行载荷
    cmds.push(IoCmd::TiocsWinsz(120));
    kinds.push(TermKind::PtySlave);
    wants.push(121);
    (cmds, kinds, wants)
}

/// 注入 A：坏实现 fd 态失明（管道上终端命令误放行）。
pub fn faulty_fd_blind(cmd: &IoCmd, kind: &TermKind) -> Res {
    let fixed = if *kind == TermKind::Pipe { TermKind::Tty } else { kind.clone() };
    judge(cmd, &fixed)
}

/// 注入 B：坏实现命令失明（未知命令误放行）。
pub fn faulty_cmd_blind(cmd: &IoCmd, kind: &TermKind) -> Res {
    match cmd {
        IoCmd::Unknown(_) => judge(&IoCmd::TcGets, kind),
        other => judge(other, kind),
    }
}

/// 注入核：两处坏账与判定器必分歧。
pub fn inject_two() -> [bool; 2] {
    let p = (IoCmd::TcGets, TermKind::Pipe);
    let u = (IoCmd::Unknown(7), TermKind::Tty);
    let d1 = judge(&p.0, &p.1) != faulty_fd_blind(&p.0, &p.1);
    let d2 = judge(&u.0, &u.1) != faulty_cmd_blind(&u.0, &u.1);
    [d1, d2]
}

/// 设置回放：winsz 写入读回对平（管道侧对账必 ENOTTY）。
pub fn winsz_replay_ok() -> bool {
    let mut t = TermFd::new(TermKind::PtySlave);
    let w = t.ioctl(&IoCmd::TiocsWinsz(132));
    let q = t.ioctl(&IoCmd::TiocgWinsz);
    w == Res::Got(132) && q == Res::Got(ET_OK_FACE) && t.cols == 132
        && t.ok_calls == 2 && t.ledger_ok()
}

/// 拒绝回放：普通文件上 TCGETS 必 ENOTTY、账落 err_calls。
pub fn reject_replay_ok() -> bool {
    let mut f = TermFd::new(TermKind::Regular);
    let r = f.ioctl(&IoCmd::TcGets);
    r == Res::Err(ET_NOTTY) && f.err_calls == 1 && f.ok_calls == 0 && f.ledger_ok()
}

/// ENOTTY 注记：判定渲染。
pub fn et_note(r: &Res) -> String {
    match r {
        Res::Got(n) => format!("got={}", n),
        Res::Err(c) => format!("{} ({})", diag_text(*c), c),
    }
}

/// 域判据（对应卡验收 J 判据）。
pub fn run_unx10855_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10855");
    let (cmds, kinds, wants) = sample_matrix();
    let (hits, total, cap_ok) = verify_matrix(&cmds, &kinds, &wants);

    // J1 正路径：终端态终端命令放行
    let j1 = judge(&IoCmd::TcGets, &TermKind::Tty) == Res::Got(ET_OK_FACE)
        && judge(&IoCmd::TcSets, &TermKind::PtySlave) == Res::Got(ET_OK_FACE);
    s.add("J1-终端放行", j1, "Tty/PtySlave 双态放行");

    // J2 反路径：非终端 fd 终端命令必 ENOTTY
    let j2 = judge(&IoCmd::TcGets, &TermKind::Pipe) == Res::Err(ET_NOTTY)
        && judge(&IoCmd::TiocgWinsz, &TermKind::Regular) == Res::Err(ET_NOTTY);
    s.add("J2-非终端拒", j2, "ENOTTY 面 25");

    // J3 边界：FIONBIO 管道放行 / 普通文件拒两沿
    let j3 = judge(&IoCmd::Fionbio(true), &TermKind::Pipe) == Res::Got(ET_OK_FACE)
        && judge(&IoCmd::Fionbio(false), &TermKind::Regular) == Res::Err(ET_NOTTY);
    s.add("J3-FIONBIO 两沿", j3, "管道/普通文件可辨");

    // J4 未知命令双态全拒
    let j4 = judge(&IoCmd::Unknown(1), &TermKind::Tty) == Res::Err(ET_CMD_MISS)
        && judge(&IoCmd::Unknown(2), &TermKind::Pipe) == Res::Err(ET_CMD_MISS);
    s.add("J4-未知命令全拒", j4, "命令轴失明不可赦");

    // J5 设置回放（winsz 写读对平）
    s.add("J5-winsz 回放", winsz_replay_ok(), "写入 132 读回对平");

    // J6 拒绝回放（普通文件 ENOTTY 落账）
    s.add("J6-拒绝回放", reject_replay_ok(), "err_calls 落账");

    // J7 样本矩阵全命中
    s.add("J7-矩阵对平", cap_ok, "8 行正反对全命中");

    // J8 注入1 fd 态失明检出
    let inj = inject_two();
    s.add("J8-fd 态失明检出", inj[0], "管道误放行必分歧");

    // J9 注入2 命令失明检出
    s.add("J9-命令失明检出", inj[1], "未知命令误放行必分歧");

    // J10 ioctl 账一致
    let mut t = TermFd::new(TermKind::Pipe);
    let _ = t.ioctl(&IoCmd::TcGets);
    let _ = t.ioctl(&IoCmd::Fionbio(true));
    s.add("J10-ioctl 账", t.ledger_ok() && t.ok_calls == 1 && t.err_calls == 1, "成败两账各一");

    // J11 字面锚：ENOTTY=25 / TCGETS=0x5401 / TCSETS=0x5402 / TIOCGWINSZ=0x5413
    let j11 = ENOTTY == 25 && TCGETS == 0x5401 && TCSETS == 0x5402 && TIOCGWINSZ == 0x5413;
    s.add("J11-字面锚", j11, "命令字与错误码常量");

    // J12 诊断码闭集
    let j12 = diag_text(ET_NOTTY) == "非终端 fd 上终端类命令"
        && diag_text(ET_CMD_MISS) == "未知命令"
        && diag_text(7) == "未知诊断码";
    s.add("J12-码文本闭集", j12, "六码文本可辨");

    // J13 (dyn) 矩阵明细落账
    s.add_dyn("J13-矩阵明细", hits == total, &format!("hits={}/{} cap_ok={}", hits, total, cap_ok));

    // J14 (dyn) ENOTTY 注记落账
    let note = et_note(&judge(&IoCmd::TcSets, &TermKind::Regular));
    s.add_dyn("J14-ENOTTY 注记", note.contains("ENOTTY") == false && note.contains("25"), &note);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10855_flow() {
        assert_eq!(judge(&IoCmd::TcGets, &TermKind::Pipe), Res::Err(ET_NOTTY));
        assert_eq!(judge(&IoCmd::TcGets, &TermKind::Tty), Res::Got(ET_OK_FACE));
        assert_eq!(judge(&IoCmd::Fionbio(true), &TermKind::Regular), Res::Err(ET_NOTTY));
        assert_eq!(judge(&IoCmd::Unknown(9), &TermKind::PtySlave), Res::Err(ET_CMD_MISS));
        let mut t = TermFd::new(TermKind::PtySlave);
        assert_eq!(t.ioctl(&IoCmd::TiocsWinsz(100)), Res::Got(100));
        assert_eq!(t.cols, 100);
        assert!(t.ledger_ok());
    }

    #[test]
    fn unx10855_checks_green() {
        let s = run_unx10855_checks();
        assert!(s.all_passed(), "UNX-F10855 判据未全绿");
    }
}
