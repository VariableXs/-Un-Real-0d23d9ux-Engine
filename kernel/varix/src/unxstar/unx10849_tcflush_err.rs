//! UNX-F10849 tcflush/tcdrain/tcflow 错误矩阵（0x1c2a0 段）
//!
//! 锚点：UNX-F10849-J1 线路控制三调用错误面——fd 非终端必 ENOTTY
//! （fd 层先判）；tcflush 选择子越出 {TCIFLUSH,TCOFLUSH,TCIOFLUSH}
//! 必 EINVAL；tcflow 动作越出 {TCOOFF,TCOON,TCIOFF,TCION} 必 EINVAL；
//! tcdrain 无选择子（排空输出）。正反双样本对账，注入核（无 ENOTTY
//! 层/选择子放水）必检出。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：调用场景（is_tty × 调用 × 选择子）→ 两层判定器（fd→选择子域）
//! → 线路控制状态机（入/出队列清账 + 流控旗标 + 排空账）→ 选择子样本
//! 矩阵 → 注入核（两坏实现必分歧）→ 清空回放（三选择子清账对平）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1c2a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c2a0;
pub const FL_NOT_TTY: u32 = SEG_BASE + 0x01; // fd 非终端（ENOTTY 面）
pub const FL_BAD_SEL: u32 = SEG_BASE + 0x02; // flush 选择子越界（EINVAL 面）
pub const FL_BAD_ACT: u32 = SEG_BASE + 0x03; // flow 动作越界（EINVAL 面）
pub const FL_OK_FACE: u32 = SEG_BASE + 0x04; // 放行面
pub const FL_DRAIN_LOST: u32 = SEG_BASE + 0x05; // 排空账丢失
pub const FL_FLOW_STUCK: u32 = SEG_BASE + 0x06; // 流控旗标失账

/// 语义字面锚。
pub const ENOTTY: u32 = 25;
pub const TCIFLUSH: u32 = 0;
pub const TCOFLUSH: u32 = 1;
pub const TCIOFLUSH: u32 = 2;
pub const TCOOFF: u32 = 0;
pub const TCOON: u32 = 1;
pub const TCIOFF: u32 = 2;
pub const TCION: u32 = 3;

/// 诊断码语义文本。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        FL_NOT_TTY => "fd 非终端 ENOTTY",
        FL_BAD_SEL => "flush 选择子越界 EINVAL",
        FL_BAD_ACT => "flow 动作越界 EINVAL",
        FL_OK_FACE => "放行面",
        FL_DRAIN_LOST => "排空账丢失",
        FL_FLOW_STUCK => "流控旗标失账",
        _ => "未知诊断码",
    }
}

/// 线路控制调用：清队列(sel) / 排空 / 流控(act)。
#[derive(Clone, PartialEq)]
pub enum LineCall {
    Flush(u32),
    Drain,
    Flow(u32),
}

/// 调用场景：fd 是否终端 × 调用（判定器两输入）。
#[derive(Clone)]
pub struct LineReq {
    pub is_tty: bool,
    pub call: LineCall,
}

/// 判定结果：放行携带语义面，拒绝携带诊断码。
#[derive(Clone, PartialEq, Debug)]
pub enum Res {
    Ok(&'static str),
    Err(u32),
}

/// 选择子域核：flush 0..=2 / flow 0..=3。
pub fn sel_valid(sel: u32) -> bool {
    sel == TCIFLUSH || sel == TCOFLUSH || sel == TCIOFLUSH
}

pub fn act_valid(act: u32) -> bool {
    act == TCOOFF || act == TCOON || act == TCIOFF || act == TCION
}

/// 两层判定器：fd 层（ENOTTY）→ 选择子/动作域（EINVAL）→ 放行。
pub fn judge(req: &LineReq) -> Res {
    if !req.is_tty {
        return Res::Err(FL_NOT_TTY);
    }
    match &req.call {
        LineCall::Flush(sel) => {
            if !sel_valid(*sel) {
                Res::Err(FL_BAD_SEL)
            } else if *sel == TCIFLUSH {
                Res::Ok("flush-in")
            } else if *sel == TCOFLUSH {
                Res::Ok("flush-out")
            } else {
                Res::Ok("flush-both")
            }
        }
        LineCall::Drain => Res::Ok("drain"),
        LineCall::Flow(act) => {
            if !act_valid(*act) {
                Res::Err(FL_BAD_ACT)
            } else if *act == TCOOFF {
                Res::Ok("out-stop")
            } else if *act == TCOON {
                Res::Ok("out-go")
            } else if *act == TCIOFF {
                Res::Ok("in-stop")
            } else {
                Res::Ok("in-go")
            }
        }
    }
}

/// 线路控制状态机：入/出队列、流控旗标、调用账。
pub struct LineCtl {
    pub is_tty: bool,
    pub in_q: u32,
    pub out_q: u32,
    pub out_stopped: bool,
    pub in_stopped: bool,
    pub calls: u32,
    pub note: String,
}

impl LineCtl {
    pub fn new(is_tty: bool, in_q: u32, out_q: u32) -> LineCtl {
        LineCtl {
            is_tty,
            in_q,
            out_q,
            out_stopped: false,
            in_stopped: false,
            calls: 0,
            note: "live".to_string(),
        }
    }

    /// tcflush：非终端 ENOTTY → 选择子 EINVAL → 按选择子清账。
    pub fn flush(&mut self, sel: u32) -> Result<&'static str, u32> {
        self.calls += 1;
        if !self.is_tty {
            return Err(FL_NOT_TTY);
        }
        if !sel_valid(sel) {
            self.note = "bad-sel".to_string();
            return Err(FL_BAD_SEL);
        }
        if sel == TCIFLUSH {
            self.in_q = 0;
        } else if sel == TCOFLUSH {
            self.out_q = 0;
        } else {
            self.in_q = 0;
            self.out_q = 0;
        }
        self.note = "flushed".to_string();
        Ok("flushed")
    }

    /// tcdrain：非终端 ENOTTY；输出排空（停止旗标下不排）。
    pub fn drain(&mut self) -> Result<u32, u32> {
        self.calls += 1;
        if !self.is_tty {
            return Err(FL_NOT_TTY);
        }
        if self.out_stopped {
            self.note = "out-stopped".to_string();
            return Ok(self.out_q);
        }
        let drained = self.out_q;
        self.out_q = 0;
        self.note = "drained".to_string();
        Ok(drained)
    }

    /// tcflow：非终端 ENOTTY → 动作 EINVAL → 流控旗标翻转。
    pub fn flow(&mut self, act: u32) -> Result<&'static str, u32> {
        self.calls += 1;
        if !self.is_tty {
            return Err(FL_NOT_TTY);
        }
        if !act_valid(act) {
            self.note = "bad-act".to_string();
            return Err(FL_BAD_ACT);
        }
        match act {
            TCOOFF => self.out_stopped = true,
            TCOON => self.out_stopped = false,
            TCIOFF => self.in_stopped = true,
            _ => self.in_stopped = false,
        }
        self.note = "flowed".to_string();
        Ok("flowed")
    }

    /// 线路账一致：队列恒有限、旗标与注记可辨。
    pub fn ledger_ok(&self) -> bool {
        self.note != "bad-sel" && self.note != "bad-act" && self.out_q <= 4096
    }
}

/// 样本矩阵容量。
pub const MATRIX_CAP: usize = 24;

/// 矩阵逐格判定：命中计数（期望码 0 表放行）。
pub fn verify_matrix(reqs: &[LineReq], wants: &[u32]) -> (usize, usize, bool) {
    let mut hits = 0usize;
    let total = reqs.len();
    let mut i = 0usize;
    while i < total && i < MATRIX_CAP {
        let want = if i < wants.len() { wants[i] } else { 0 };
        let hit = match judge(&reqs[i]) {
            Res::Ok(_) => want == FL_OK_FACE,
            Res::Err(c) => c == want,
        };
        if hit {
            hits += 1;
        }
        i += 1;
    }
    (hits, total.min(MATRIX_CAP), hits == total.min(MATRIX_CAP))
}

/// 样本矩阵：tty/非 tty × 三调用 × 域边界，10 行全分账。
pub fn sample_matrix() -> (Vec<LineReq>, Vec<u32>) {
    let mut reqs: Vec<LineReq> = Vec::new();
    let mut wants: Vec<u32> = Vec::new();
    // R0 非 tty flush → ENOTTY
    reqs.push(LineReq { is_tty: false, call: LineCall::Flush(TCIFLUSH) });
    wants.push(FL_NOT_TTY);
    // R1 非 tty drain → ENOTTY
    reqs.push(LineReq { is_tty: false, call: LineCall::Drain });
    wants.push(FL_NOT_TTY);
    // R2 非 tty flow → ENOTTY
    reqs.push(LineReq { is_tty: false, call: LineCall::Flow(TCOON) });
    wants.push(FL_NOT_TTY);
    // R3 flush 选择子 3 → EINVAL
    reqs.push(LineReq { is_tty: true, call: LineCall::Flush(3) });
    wants.push(FL_BAD_SEL);
    // R4 flush 选择子 u32::MAX → EINVAL
    reqs.push(LineReq { is_tty: true, call: LineCall::Flush(u32::MAX) });
    wants.push(FL_BAD_SEL);
    // R5 flow 动作 4 → EINVAL
    reqs.push(LineReq { is_tty: true, call: LineCall::Flow(4) });
    wants.push(FL_BAD_ACT);
    // R6 flush TCIOFLUSH → 放行
    reqs.push(LineReq { is_tty: true, call: LineCall::Flush(TCIOFLUSH) });
    wants.push(FL_OK_FACE);
    // R7 drain → 放行
    reqs.push(LineReq { is_tty: true, call: LineCall::Drain });
    wants.push(FL_OK_FACE);
    // R8 flow TCION → 放行
    reqs.push(LineReq { is_tty: true, call: LineCall::Flow(TCION) });
    wants.push(FL_OK_FACE);
    // R9 flow TCOOFF → 放行
    reqs.push(LineReq { is_tty: true, call: LineCall::Flow(TCOOFF) });
    wants.push(FL_OK_FACE);
    (reqs, wants)
}

/// 注入 A：坏实现无 ENOTTY 层。
pub fn faulty_no_enotty(req: &LineReq) -> Res {
    let mut fixed = req.clone();
    fixed.is_tty = true;
    judge(&fixed)
}

/// 注入 B：坏实现选择子放水（任意值亦清账放行）。
pub fn faulty_sel_loose(req: &LineReq) -> Res {
    match &req.call {
        LineCall::Flush(_sel) if req.is_tty => Res::Ok("flush-both"),
        _ => judge(req),
    }
}

/// 注入核：两处坏账与判定器必分歧。
pub fn inject_two() -> [bool; 2] {
    let notty = LineReq { is_tty: false, call: LineCall::Flush(TCIFLUSH) };
    let badsel = LineReq { is_tty: true, call: LineCall::Flush(9) };
    let d1 = judge(&notty) != faulty_no_enotty(&notty);
    let d2 = judge(&badsel) != faulty_sel_loose(&badsel);
    [d1, d2]
}

/// 清空回放：三选择子清账对平（入清/出清/坏选择子）。
pub fn flush_replay_ok() -> bool {
    let mut lc = LineCtl::new(true, 7, 9);
    let f1 = lc.flush(TCIFLUSH);
    let step1 = lc.in_q == 0 && lc.out_q == 9;
    let f2 = lc.flush(TCOFLUSH);
    let step2 = lc.in_q == 0 && lc.out_q == 0;
    let clean = lc.ledger_ok();
    let f3 = lc.flush(9);
    f1.is_ok() && step1 && f2.is_ok() && step2 && clean
        && f3.err() == Some(FL_BAD_SEL) && lc.calls == 3
}

/// 流控回放：出停→排空挂起→出启→排空落地。
pub fn flow_replay_ok() -> bool {
    let mut lc = LineCtl::new(true, 0, 5);
    let f1 = lc.flow(TCOOFF);
    let held = lc.drain();
    let f2 = lc.flow(TCOON);
    let released = lc.drain();
    f1.is_ok() && lc.out_stopped && held == Ok(5)
        && f2.is_ok() && !lc.out_stopped && released == Ok(5)
        && lc.in_stopped == false && lc.ledger_ok()
}

/// 非终端核：三调用皆 ENOTTY，账面零污染。
pub fn notty_face_ok() -> bool {
    let mut lc = LineCtl::new(false, 3, 4);
    let f = lc.flush(TCIOFLUSH);
    let d = lc.drain();
    let w = lc.flow(TCOON);
    f.err() == Some(FL_NOT_TTY) && d.err() == Some(FL_NOT_TTY) && w.err() == Some(FL_NOT_TTY)
        && lc.in_q == 3 && lc.out_q == 4 && lc.calls == 3
}

/// 调用注记：判定渲染。
pub fn line_note(r: &Res) -> String {
    match r {
        Res::Ok(face) => (*face).to_string(),
        Res::Err(c) => diag_text(*c).to_string(),
    }
}

/// 域判据（对应卡验收 J 判据）。
pub fn run_unx10849_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10849");
    let (reqs, wants) = sample_matrix();
    let (hits, total, cap_ok) = verify_matrix(&reqs, &wants);

    // J1 正路径：TCIOFLUSH 双清放行
    let j1 = judge(&LineReq { is_tty: true, call: LineCall::Flush(TCIOFLUSH) })
        == Res::Ok("flush-both");
    s.add("J1-双清放行", j1, "TCIOFLUSH 语义面");

    // J2 选择子边界：2 放行 / 3 拒绝
    let j2 = judge(&LineReq { is_tty: true, call: LineCall::Flush(TCIOFLUSH) }) != Res::Err(FL_BAD_SEL)
        && judge(&LineReq { is_tty: true, call: LineCall::Flush(3) }) == Res::Err(FL_BAD_SEL);
    s.add("J2-选择子边界", j2, "2/3 两沿可辨");

    // J3 动作边界：3 放行 / 4 拒绝
    let j3 = judge(&LineReq { is_tty: true, call: LineCall::Flow(TCION) }) == Res::Ok("in-go")
        && judge(&LineReq { is_tty: true, call: LineCall::Flow(4) }) == Res::Err(FL_BAD_ACT);
    s.add("J3-动作边界", j3, "3/4 两沿可辨");

    // J4 fd 层：三调用非 tty 皆 ENOTTY
    let j4 = judge(&LineReq { is_tty: false, call: LineCall::Flush(TCIOFLUSH) })
        == Res::Err(FL_NOT_TTY)
        && judge(&LineReq { is_tty: false, call: LineCall::Flow(TCOON) })
            == Res::Err(FL_NOT_TTY)
        && judge(&LineReq { is_tty: false, call: LineCall::Drain }) == Res::Err(FL_NOT_TTY);
    s.add("J4-fd 层先判", j4, "ENOTTY 覆盖三调用");

    // J5 清空回放（三选择子）
    s.add("J5-清空回放", flush_replay_ok(), "入清/出清/坏选择子对平");

    // J6 流控回放（停-排空挂起-启-排空落地）
    s.add("J6-流控回放", flow_replay_ok(), "旗标与排空账对平");

    // J7 非终端核（零污染）
    s.add("J7-非终端零污染", notty_face_ok(), "账面回零");

    // J8 样本矩阵全命中
    s.add("J8-矩阵对平", cap_ok, "10 行两层全命中");

    // J9 注入1 ENOTTY 缺失检出
    let inj = inject_two();
    s.add("J9-ENOTTY 缺失检出", inj[0], "非 tty 误判必分歧");

    // J10 注入2 选择子放水检出
    s.add("J10-选择子放水检出", inj[1], "坏选择子误放必分歧");

    // J11 字面锚：TCIFLUSH=0/TCOFLUSH=1/TCIOFLUSH=2 与 TCOON=1/TCION=3
    let j11 = TCIFLUSH == 0 && TCOFLUSH == 1 && TCIOFLUSH == 2
        && TCOON == 1 && TCION == 3 && sel_valid(TCIOFLUSH) && act_valid(TCION);
    s.add("J11-字面锚", j11, "选择子域常量对");

    // J12 诊断码闭集
    let j12 = diag_text(FL_BAD_SEL) == "flush 选择子越界 EINVAL"
        && diag_text(FL_BAD_ACT) == "flow 动作越界 EINVAL"
        && diag_text(6) == "未知诊断码";
    s.add("J12-码文本闭集", j12, "六码文本可辨");

    // J13 (dyn) 矩阵明细落账
    s.add_dyn("J13-矩阵明细", hits == total, &format!("hits={}/{} cap_ok={}", hits, total, cap_ok));

    // J14 (dyn) 调用注记落账
    let note = line_note(&judge(&LineReq { is_tty: true, call: LineCall::Flush(TCIFLUSH) }));
    s.add_dyn("J14-调用注记", note == "flush-in", &note);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10849_flow() {
        let mut lc = LineCtl::new(false, 3, 4);
        assert!(lc.flush(TCIFLUSH).err() == Some(FL_NOT_TTY));
        assert!(lc.drain().err() == Some(FL_NOT_TTY));
        assert!(lc.flow(TCOOFF).err() == Some(FL_NOT_TTY));
        let mut tty = LineCtl::new(true, 5, 6);
        assert!(tty.flush(9).err() == Some(FL_BAD_SEL));
        assert!(tty.flush(TCIFLUSH).is_ok());
        assert_eq!(tty.in_q, 0);
        assert!(tty.flow(7).err() == Some(FL_BAD_ACT));
        assert!(tty.flow(TCOOFF).is_ok());
        assert!(tty.out_stopped);
        assert_eq!(judge(&LineReq { is_tty: true, call: LineCall::Flow(9) }), Res::Err(FL_BAD_ACT));
    }

    #[test]
    fn unx10849_checks_green() {
        let s = run_unx10849_checks();
        assert!(s.all_passed(), "UNX-F10849 判据未全绿");
    }
}
