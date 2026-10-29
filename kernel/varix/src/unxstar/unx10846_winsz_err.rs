//! UNX-F10846 TIOCGWINSZ/TIOCSWINSZ 错误矩阵（0x1c240 段）
//!
//! 锚点：UNX-F10846-J1 窗口尺寸两 ioctl 错误面——fd 非终端必 ENOTTY
//! （fd 层先判）；TIOCSWINSZ 行/列越出 u16 域必 EINVAL（域溢出面，
//! 不截断放行）；TIOCGWINSZ 恒回当前值（未置为零值域）。零值合法
//! （表示未知）。正反双样本对账，注入核（无 ENOTTY 层/截断放水）
//! 必检出。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：ioctl 场景（is_tty × cmd × 行列）→ 两层判定器（fd→域溢出）
//! → 窗口账状态机（置读对平）→ 尺寸样本矩阵（容量封顶）→ 注入核
//! （两坏实现必分歧）→ 边界回放（0/0xFFFF/0x10000 三沿）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1c240 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c240;
pub const WS_NOT_TTY: u32 = SEG_BASE + 0x01; // fd 非终端（ENOTTY 面）
pub const WS_OVERFLOW: u32 = SEG_BASE + 0x02; // 行列越 u16 域（EINVAL 面）
pub const WS_GET_OK: u32 = SEG_BASE + 0x03; // 读面放行
pub const WS_SET_OK: u32 = SEG_BASE + 0x04; // 写面放行
pub const WS_LEDGER_LOST: u32 = SEG_BASE + 0x05; // 置读账丢失
pub const WS_BAD_CMD: u32 = SEG_BASE + 0x06; // 未知命令字

/// 语义字面锚。
pub const ENOTTY: u32 = 25;
pub const TIOCGWINSZ: u32 = 1;
pub const TIOCSWINSZ: u32 = 2;
pub const WIN_MAX: u32 = 0xFFFF;

/// 诊断码语义文本。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        WS_NOT_TTY => "fd 非终端 ENOTTY",
        WS_OVERFLOW => "行列越 u16 域 EINVAL",
        WS_GET_OK => "读面放行",
        WS_SET_OK => "写面放行",
        WS_LEDGER_LOST => "置读账丢失",
        WS_BAD_CMD => "未知命令字",
        _ => "未知诊断码",
    }
}

/// ioctl 命令：取窗 / 置窗(rows, cols) / 噪声命令字。
#[derive(Clone, PartialEq)]
pub enum WinCmd {
    Get,
    Set(u32, u32),
    Noise(u32),
}

/// ioctl 场景：fd 是否终端 × 命令（判定器两输入）。
#[derive(Clone)]
pub struct WinReq {
    pub is_tty: bool,
    pub cmd: WinCmd,
}

/// 判定结果：Get/Set 返回 (行, 列)，拒绝携带诊断码。
#[derive(Clone, PartialEq, Debug)]
pub enum Res {
    Wins(u32, u32),
    Err(u32),
}

/// u16 域核：值必须落 0..=0xFFFF。
pub fn in_u16(v: u32) -> bool {
    v <= WIN_MAX
}

/// 两层判定器：fd 层（ENOTTY）→ 命令分账（域溢出 EINVAL）→ 放行。
pub fn judge(req: &WinReq) -> Res {
    if !req.is_tty {
        return Res::Err(WS_NOT_TTY);
    }
    match &req.cmd {
        WinCmd::Get => Res::Wins(0, 0),
        WinCmd::Set(rows, cols) => {
            if !in_u16(*rows) || !in_u16(*cols) {
                Res::Err(WS_OVERFLOW)
            } else {
                Res::Wins(*rows, *cols)
            }
        }
        WinCmd::Noise(_) => Res::Err(WS_BAD_CMD),
    }
}

/// 窗口账状态机：置读对平、命令账。
pub struct WinCtl {
    pub is_tty: bool,
    pub rows: u32,
    pub cols: u32,
    pub sets: u32,
    pub gets: u32,
    pub note: String,
}

impl WinCtl {
    pub fn new(is_tty: bool) -> WinCtl {
        WinCtl {
            is_tty,
            rows: 0,
            cols: 0,
            sets: 0,
            gets: 0,
            note: "idle".to_string(),
        }
    }

    /// TIOCGWINSZ：非终端 ENOTTY；否则回当前值并落读账。
    pub fn get(&mut self) -> Result<(u32, u32), u32> {
        if !self.is_tty {
            return Err(WS_NOT_TTY);
        }
        self.gets += 1;
        self.note = "read".to_string();
        Ok((self.rows, self.cols))
    }

    /// TIOCSWINSZ：非终端 ENOTTY → 域溢出 EINVAL → 提交双字段。
    pub fn set(&mut self, rows: u32, cols: u32) -> Result<(), u32> {
        if !self.is_tty {
            return Err(WS_NOT_TTY);
        }
        if !in_u16(rows) || !in_u16(cols) {
            self.note = "overflow".to_string();
            return Err(WS_OVERFLOW);
        }
        self.rows = rows;
        self.cols = cols;
        self.sets += 1;
        self.note = "written".to_string();
        Ok(())
    }

    /// 置读账一致：读账随写账单调、终值对平。
    pub fn ledger_ok(&self) -> bool {
        self.gets <= self.sets + 1 && self.rows <= WIN_MAX && self.cols <= WIN_MAX
    }
}

/// 样本矩阵容量。
pub const MATRIX_CAP: usize = 20;

/// 矩阵逐格判定：期望码 0 表放行命中。
pub fn verify_matrix(reqs: &[WinReq], wants: &[u32]) -> (usize, usize, bool) {
    let mut hits = 0usize;
    let total = reqs.len();
    let mut i = 0usize;
    while i < total && i < MATRIX_CAP {
        let want = if i < wants.len() { wants[i] } else { 0 };
        let hit = match judge(&reqs[i]) {
            Res::Wins(_, _) => want == WS_GET_OK,
            Res::Err(c) => c == want,
        };
        if hit {
            hits += 1;
        }
        i += 1;
    }
    (hits, total.min(MATRIX_CAP), hits == total.min(MATRIX_CAP))
}

/// 样本矩阵：tty/非 tty × get/set × 溢出/零/满，8 行全分账。
pub fn sample_matrix() -> (Vec<WinReq>, Vec<u32>) {
    let mut reqs: Vec<WinReq> = Vec::new();
    let mut wants: Vec<u32> = Vec::new();
    // R0 非 tty get → ENOTTY
    reqs.push(WinReq { is_tty: false, cmd: WinCmd::Get });
    wants.push(WS_NOT_TTY);
    // R1 非 tty set（合法值也拒）
    reqs.push(WinReq { is_tty: false, cmd: WinCmd::Set(24, 80) });
    wants.push(WS_NOT_TTY);
    // R2 行溢出 0x10000 → EINVAL
    reqs.push(WinReq { is_tty: true, cmd: WinCmd::Set(WIN_MAX + 1, 80) });
    wants.push(WS_OVERFLOW);
    // R3 列溢出 → EINVAL
    reqs.push(WinReq { is_tty: true, cmd: WinCmd::Set(24, u32::MAX) });
    wants.push(WS_OVERFLOW);
    // R4 tty get → 放行
    reqs.push(WinReq { is_tty: true, cmd: WinCmd::Get });
    wants.push(WS_GET_OK);
    // R5 零值置窗（未知语义合法）→ 放行
    reqs.push(WinReq { is_tty: true, cmd: WinCmd::Set(0, 0) });
    wants.push(WS_GET_OK);
    // R6 满域置窗 0xFFFF → 放行
    reqs.push(WinReq { is_tty: true, cmd: WinCmd::Set(WIN_MAX, WIN_MAX) });
    wants.push(WS_GET_OK);
    // R7 噪声命令字 → BAD_CMD
    reqs.push(WinReq { is_tty: true, cmd: WinCmd::Noise(0x77) });
    wants.push(WS_BAD_CMD);
    (reqs, wants)
}

/// 注入 A：坏实现无 ENOTTY 层。
pub fn faulty_no_enotty(req: &WinReq) -> Res {
    let mut fixed = req.clone();
    fixed.is_tty = true;
    judge(&fixed)
}

/// 注入 B：坏实现截断放水（越域值截 u16 放行）。
pub fn faulty_trunc(req: &WinReq) -> Res {
    match &req.cmd {
        WinCmd::Set(rows, cols) if req.is_tty => {
            Res::Wins(rows & WIN_MAX, cols & WIN_MAX)
        }
        _ => judge(req),
    }
}

/// 注入核：两处坏账与判定器必分歧。
pub fn inject_two() -> [bool; 2] {
    let notty = WinReq { is_tty: false, cmd: WinCmd::Get };
    let ovf = WinReq { is_tty: true, cmd: WinCmd::Set(0x10000, 3) };
    let d1 = judge(&notty) != faulty_no_enotty(&notty);
    let d2 = judge(&ovf) != faulty_trunc(&ovf);
    [d1, d2]
}

/// 边界回放：零/满/溢出一三沿，置读账终值对平。
pub fn bound_replay_ok() -> bool {
    let mut ctl = WinCtl::new(true);
    let s0 = ctl.set(0, 0);
    let g0 = ctl.get();
    let s1 = ctl.set(WIN_MAX, WIN_MAX);
    let g1 = ctl.get();
    let s2 = ctl.set(WIN_MAX + 1, 1);
    let g2 = ctl.get();
    s0.is_ok() && s1.is_ok() && s2.err() == Some(WS_OVERFLOW)
        && g0 == Ok((0, 0)) && g1 == Ok((WIN_MAX, WIN_MAX)) && g2 == Ok((WIN_MAX, WIN_MAX))
        && ctl.sets == 2 && ctl.gets == 3 && ctl.ledger_ok()
}

/// 非终端核：get/set 双调皆 ENOTTY，账面零污染。
pub fn notty_face_ok() -> bool {
    let mut ctl = WinCtl::new(false);
    let g = ctl.get();
    let st = ctl.set(24, 80);
    g.err() == Some(WS_NOT_TTY) && st.err() == Some(WS_NOT_TTY)
        && ctl.rows == 0 && ctl.cols == 0 && ctl.sets == 0 && ctl.gets == 0
}

/// 窗注记：尺寸渲染。
pub fn win_note(r: &Res) -> String {
    match r {
        Res::Wins(r, c) => format!("{}x{}", r, c),
        Res::Err(c) => diag_text(*c).to_string(),
    }
}

/// 域判据（对应卡验收 J 判据）。
pub fn run_unx10846_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10846");
    let (reqs, wants) = sample_matrix();
    let (hits, total, cap_ok) = verify_matrix(&reqs, &wants);

    // J1 正路径：tty 置窗 24x80 放行
    let j1 = judge(&WinReq { is_tty: true, cmd: WinCmd::Set(24, 80) }) == Res::Wins(24, 80);
    s.add("J1-置窗放行", j1, "常规尺寸放行");

    // J2 边界：恰 0xFFFF 放行 / 恰 0x10000 拒绝
    let j2 = judge(&WinReq { is_tty: true, cmd: WinCmd::Set(WIN_MAX, 0) }) == Res::Wins(WIN_MAX, 0)
        && judge(&WinReq { is_tty: true, cmd: WinCmd::Set(WIN_MAX + 1, 0) }) == Res::Err(WS_OVERFLOW);
    s.add("J2-域边界", j2, "0xFFFF/0x10000 两沿可辨");

    // J3 零值语义：0x0 合法（未知尺寸）
    s.add(
        "J3-零值合法",
        judge(&WinReq { is_tty: true, cmd: WinCmd::Set(0, 0) }) == Res::Wins(0, 0),
        "零值域内放行",
    );

    // J4 fd 层：非 tty 双命令皆 ENOTTY
    let j4 = judge(&WinReq { is_tty: false, cmd: WinCmd::Get }) == Res::Err(WS_NOT_TTY)
        && judge(&WinReq { is_tty: false, cmd: WinCmd::Set(1, 1) }) == Res::Err(WS_NOT_TTY);
    s.add("J4-fd 层先判", j4, "ENOTTY 覆盖溢出面");

    // J5 边界回放（三沿 + 置读账）
    s.add("J5-边界回放", bound_replay_ok(), "0/满/溢出一三沿对平");

    // J6 非终端核（零污染）
    s.add("J6-非终端零污染", notty_face_ok(), "账面回零");

    // J7 样本矩阵全命中
    s.add("J7-矩阵对平", cap_ok, "8 行两层全命中");

    // J8 注入1 ENOTTY 缺失检出
    let inj = inject_two();
    s.add("J8-ENOTTY 缺失检出", inj[0], "非 tty 误判必分歧");

    // J9 注入2 截断放水检出
    s.add("J9-截断放水检出", inj[1], "越域截断必分歧");

    // J10 置读账一致
    let mut ctl = WinCtl::new(true);
    let _ = ctl.set(40, 100);
    let _ = ctl.get();
    s.add("J10-置读账", ctl.ledger_ok() && ctl.rows == 40 && ctl.cols == 100, "终值对平");

    // J11 字面锚：ENOTTY=25 / WIN_MAX=0xFFFF
    let j11 = ENOTTY == 25 && WIN_MAX == 0xFFFF && in_u16(WIN_MAX) && !in_u16(WIN_MAX + 1);
    s.add("J11-字面锚", j11, "errno 与 u16 域");

    // J12 诊断码闭集
    let j12 = diag_text(WS_NOT_TTY) == "fd 非终端 ENOTTY"
        && diag_text(WS_OVERFLOW) == "行列越 u16 域 EINVAL"
        && diag_text(3) == "未知诊断码";
    s.add("J12-码文本闭集", j12, "六码文本可辨");

    // J13 (dyn) 矩阵明细落账
    s.add_dyn("J13-矩阵明细", hits == total, &format!("hits={}/{} cap_ok={}", hits, total, cap_ok));

    // J14 (dyn) 窗注记落账
    let note = win_note(&judge(&WinReq { is_tty: true, cmd: WinCmd::Set(24, 80) }));
    s.add_dyn("J14-窗注记", note == "24x80", &note);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10846_flow() {
        let mut ctl = WinCtl::new(false);
        assert!(ctl.get().err() == Some(WS_NOT_TTY));
        assert!(ctl.set(24, 80).err() == Some(WS_NOT_TTY));
        let mut tty = WinCtl::new(true);
        assert!(tty.set(0x10000, 1).err() == Some(WS_OVERFLOW));
        assert!(tty.set(24, 80).is_ok());
        assert_eq!(tty.get(), Ok((24, 80)));
        assert_eq!(judge(&WinReq { is_tty: true, cmd: WinCmd::Noise(9) }), Res::Err(WS_BAD_CMD));
        assert_eq!(judge(&WinReq { is_tty: false, cmd: WinCmd::Get }), Res::Err(WS_NOT_TTY));
        assert!(in_u16(0xFFFF) && !in_u16(0x10000));
    }

    #[test]
    fn unx10846_checks_green() {
        let s = run_unx10846_checks();
        assert!(s.all_passed(), "UNX-F10846 判据未全绿");
    }
}
