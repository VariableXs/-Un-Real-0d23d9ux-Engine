//! UNX-F10844 EIO 与 EOF 分账矩阵（读返 0 vs EIO）（0x1c200 段）
//!
//! 锚点：UNX-F10844-J1 读零与读错分账——管道面对端全关且数据排空后
//! 读返 0（EOF 数据尽头，非错误）；pty 从端面主端关闭后读必 EIO
//! （挂断错误，非数据尽头）；对端全关但在途数据未排空时先读数据后
//! 才见 EOF。两面账本独立计数，串账必检出（注入核双冒账必分歧）。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：读场景（面别 × 对端存活 × 在途）→ 分账判定器（面内三态：
//! Got/EOF/EIO）→ 读账本（eof/eio/字节三计数串账核）→ 分账样本矩阵
//! → 注入核（EOF 冒 EIO/EIO 冒 EOF 必分歧）→ 排空回放（先数据后 0）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1c200 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c200;
pub const EE_HANGUP_EIO: u32 = SEG_BASE + 0x01; // pty 挂断读 EIO
pub const EE_EOF_FACE: u32 = SEG_BASE + 0x02; // 管道排空读 0（EOF 面）
pub const EE_DATA_FACE: u32 = SEG_BASE + 0x03; // 在途数据面
pub const EE_BLOCK_FACE: u32 = SEG_BASE + 0x04; // 对端活但无数据
pub const EE_LEDGER_MIX: u32 = SEG_BASE + 0x05; // 两面账串账
pub const EE_DRAIN_ORDER: u32 = SEG_BASE + 0x06; // 排空次序账

/// 语义字面锚。
pub const EIO: u32 = 5;

/// 诊断码语义文本。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        EE_HANGUP_EIO => "pty 挂断读 EIO",
        EE_EOF_FACE => "管道排空 EOF 读 0",
        EE_DATA_FACE => "在途数据面",
        EE_BLOCK_FACE => "对端活无数据",
        EE_LEDGER_MIX => "两面账串账",
        EE_DRAIN_ORDER => "排空次序账",
        _ => "未知诊断码",
    }
}

/// 读面别：pty 从端 / 管道。
#[derive(Clone, PartialEq)]
pub enum Face {
    PtySlave,
    Pipe,
}

/// 读场景：面别 × 对端存活 × 在途字节（判定器三输入）。
#[derive(Clone)]
pub struct ReadReq {
    pub face: Face,
    pub peer_alive: bool,
    pub pending: u32,
}

/// 判定结果：Got(字节数)/Eof(0 面)/Err(码)。
#[derive(Clone, PartialEq, Debug)]
pub enum Res {
    Got(u32),
    Eof,
    Err(u32),
}

/// 分账判定器：pty 面挂断 EIO 压倒一切；管道面先数据后 EOF。
pub fn judge(req: &ReadReq) -> Res {
    match req.face {
        Face::PtySlave => {
            if !req.peer_alive {
                Res::Err(EE_HANGUP_EIO)
            } else if req.pending > 0 {
                Res::Got(req.pending)
            } else {
                Res::Err(EE_BLOCK_FACE)
            }
        }
        Face::Pipe => {
            if req.pending > 0 {
                Res::Got(req.pending)
            } else if req.peer_alive {
                Res::Err(EE_BLOCK_FACE)
            } else {
                Res::Eof
            }
        }
    }
}

/// 读账本：eof/eio/字节三计数，串账核。
pub struct ReadLedger {
    pub eof_hits: u32,
    pub eio_hits: u32,
    pub data_bytes: u32,
    pub note: String,
}

impl ReadLedger {
    pub fn new() -> ReadLedger {
        ReadLedger { eof_hits: 0, eio_hits: 0, data_bytes: 0, note: "clean".to_string() }
    }

    /// 落账一次读结果：Got/EOF/EIO 各归其账，绝不串。
    pub fn record(&mut self, r: &Res) -> Result<(), u32> {
        match r {
            Res::Got(n) => {
                self.data_bytes += *n;
                Ok(())
            }
            Res::Eof => {
                self.eof_hits += 1;
                Ok(())
            }
            Res::Err(EE_HANGUP_EIO) => {
                self.eio_hits += 1;
                Ok(())
            }
            Res::Err(EE_BLOCK_FACE) => Ok(()),
            Res::Err(_) => {
                self.note = "mixed".to_string();
                Err(EE_LEDGER_MIX)
            }
        }
    }

    /// 账面一致：两计数独立、字节账封闭。
    pub fn ledger_ok(&self) -> bool {
        self.note == "clean" && self.eio_hits <= 1 && self.eof_hits <= 1
    }

    /// 串账核：EOF 账与 EIO 账必须可辨（同账双清即串）。
    pub fn no_cross(&self) -> bool {
        !(self.eof_hits > 0 && self.eio_hits > 0)
    }
}

/// 样本矩阵容量。
pub const MATRIX_CAP: usize = 20;

/// 矩阵逐格判定：期望编码（1=Got/2=Eof/其余=Err 码）命中计数。
pub fn verify_matrix(reqs: &[ReadReq], wants: &[u32]) -> (usize, usize, bool) {
    let mut hits = 0usize;
    let total = reqs.len();
    let mut i = 0usize;
    while i < total && i < MATRIX_CAP {
        let got = judge(&reqs[i]);
        let want = if i < wants.len() { wants[i] } else { 0 };
        let hit = match (&got, want) {
            (Res::Got(_), 1) => true,
            (Res::Eof, 2) => true,
            (Res::Err(c), w) => *c == w,
            _ => false,
        };
        if hit {
            hits += 1;
        }
        i += 1;
    }
    (hits, total.min(MATRIX_CAP), hits == total.min(MATRIX_CAP))
}

/// 样本矩阵：两面对端活/死 × 在途 0/非 0，8 行全分账。
pub fn sample_matrix() -> (Vec<ReadReq>, Vec<u32>) {
    let mut reqs: Vec<ReadReq> = Vec::new();
    let mut wants: Vec<u32> = Vec::new();
    // R0 pty 挂断+在途（挂断压数据）→ EIO
    reqs.push(ReadReq { face: Face::PtySlave, peer_alive: false, pending: 9 });
    wants.push(EE_HANGUP_EIO);
    // R1 pty 挂断+无数据 → EIO
    reqs.push(ReadReq { face: Face::PtySlave, peer_alive: false, pending: 0 });
    wants.push(EE_HANGUP_EIO);
    // R2 pty 活+在途 → Got
    reqs.push(ReadReq { face: Face::PtySlave, peer_alive: true, pending: 4 });
    wants.push(1);
    // R3 pty 活+无数据 → Block
    reqs.push(ReadReq { face: Face::PtySlave, peer_alive: true, pending: 0 });
    wants.push(EE_BLOCK_FACE);
    // R4 管道全关+在途 → Got（先数据）
    reqs.push(ReadReq { face: Face::Pipe, peer_alive: false, pending: 6 });
    wants.push(1);
    // R5 管道全关+排空 → EOF
    reqs.push(ReadReq { face: Face::Pipe, peer_alive: false, pending: 0 });
    wants.push(2);
    // R6 管道活+在途 → Got
    reqs.push(ReadReq { face: Face::Pipe, peer_alive: true, pending: 3 });
    wants.push(1);
    // R7 管道活+排空 → Block
    reqs.push(ReadReq { face: Face::Pipe, peer_alive: true, pending: 0 });
    wants.push(EE_BLOCK_FACE);
    (reqs, wants)
}

/// 注入 A：坏实现 pty 挂断读返 EOF 面（Got(0)）。
pub fn faulty_eof_on_eio(req: &ReadReq) -> Res {
    if req.face == Face::PtySlave && !req.peer_alive {
        Res::Got(0)
    } else {
        judge(req)
    }
}

/// 注入 B：坏实现管道排空读返 EIO。
pub fn faulty_eio_on_eof(req: &ReadReq) -> Res {
    if req.face == Face::Pipe && !req.peer_alive && req.pending == 0 {
        Res::Err(EE_HANGUP_EIO)
    } else {
        judge(req)
    }
}

/// 注入核：两处冒账与判定器必分歧。
pub fn inject_two() -> [bool; 2] {
    let hang = ReadReq { face: Face::PtySlave, peer_alive: false, pending: 0 };
    let drain = ReadReq { face: Face::Pipe, peer_alive: false, pending: 0 };
    let d1 = judge(&hang) != faulty_eof_on_eio(&hang);
    let d2 = judge(&drain) != faulty_eio_on_eof(&drain);
    [d1, d2]
}

/// 排空回放：对端全关时先读尽在途数据、再读见 EOF，次序账对平。
pub fn drain_order_ok() -> bool {
    let mut lg = ReadLedger::new();
    let mut pending: u32 = 11;
    let mut saw_eof = false;
    let mut guard = 0u32;
    while guard < 16 {
        let req = ReadReq { face: Face::Pipe, peer_alive: false, pending };
        let step = judge(&req);
        let rec = lg.record(&step);
        if rec.is_err() {
            return false;
        }
        match step {
            Res::Got(n) => {
                if n == 0 || n > pending {
                    return false;
                }
                pending = 0;
            }
            Res::Eof => {
                saw_eof = true;
                break;
            }
            _ => break,
        }
        guard += 1;
    }
    saw_eof && lg.eof_hits == 1 && lg.eio_hits == 0 && lg.data_bytes == 11 && lg.no_cross()
}

/// pty 挂断回放：读一次即 EIO，账只入 EIO。
pub fn hangup_ledger_ok() -> bool {
    let mut lg = ReadLedger::new();
    let req = ReadReq { face: Face::PtySlave, peer_alive: false, pending: 5 };
    let r1 = lg.record(&judge(&req));
    let r2 = lg.record(&judge(&ReadReq { face: Face::PtySlave, peer_alive: false, pending: 0 }));
    r1.is_ok() && r2.is_ok() && lg.eio_hits == 2 && lg.eof_hits == 0 && lg.data_bytes == 0
}

/// 分账注记：读结果渲染。
pub fn res_note(r: &Res) -> String {
    match r {
        Res::Got(n) => format!("got={}", n),
        Res::Eof => diag_text(EE_EOF_FACE).to_string(),
        Res::Err(c) => format!("{} ({})", diag_text(*c), c),
    }
}

/// 域判据（对应卡验收 J 判据）。
pub fn run_unx10844_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10844");
    let (reqs, wants) = sample_matrix();
    let (hits, total, cap_ok) = verify_matrix(&reqs, &wants);

    // J1 EOF 面：管道全关+排空 → Eof（读返 0，非错误）
    let eof = judge(&ReadReq { face: Face::Pipe, peer_alive: false, pending: 0 });
    s.add("J1-EOF 面", eof == Res::Eof, "排空读 0 为数据尽头");

    // J2 EIO 面：pty 挂断 → EIO（即便有在途）
    let j2 = judge(&ReadReq { face: Face::PtySlave, peer_alive: false, pending: 7 })
        == Res::Err(EE_HANGUP_EIO)
        && judge(&ReadReq { face: Face::PtySlave, peer_alive: false, pending: 0 })
            == Res::Err(EE_HANGUP_EIO);
    s.add("J2-挂断 EIO", j2, "挂断压数据面");

    // J3 边界：管道全关恰一字节在途 → Got；恰零字节 → Eof
    let j3 = judge(&ReadReq { face: Face::Pipe, peer_alive: false, pending: 1 }) == Res::Got(1)
        && judge(&ReadReq { face: Face::Pipe, peer_alive: false, pending: 0 }) == Res::Eof;
    s.add("J3-一字节边界", j3, "1/0 字节两沿可辨");

    // J4 对端活无数据 → Block 面（非 EOF 非 EIO）
    s.add(
        "J4-待读面",
        judge(&ReadReq { face: Face::Pipe, peer_alive: true, pending: 0 }) != Res::Eof,
        "存活期非尽头",
    );

    // J5 排空回放（次序账）
    s.add("J5-排空回放", drain_order_ok(), "先数据后 EOF 字节账对平");

    // J6 pty 挂断账
    s.add("J6-挂断账", hangup_ledger_ok(), "两读两 EIO 零 EOF");

    // J7 样本矩阵全命中
    s.add("J7-矩阵对平", cap_ok, "8 行两面全分账");

    // J8 注入1 EIO 冒 EOF 检出
    let inj = inject_two();
    s.add("J8-EIO 冒 EOF 检出", inj[0], "挂断读返 0 必分歧");

    // J9 注入2 EOF 冒 EIO 检出
    s.add("J9-EOF 冒 EIO 检出", inj[1], "排空读错码必分歧");

    // J10 账本串账核：正常单面账 no_cross
    let mut lg = ReadLedger::new();
    let _ = lg.record(&Res::Eof);
    s.add("J10-单面账", lg.ledger_ok() && lg.no_cross(), "EOF 账独占");

    // J11 字面锚：EIO=5 / EOF 面 = 0 字节
    let j11 = EIO == 5 && EE_HANGUP_EIO == SEG_BASE + 0x01 && EE_EOF_FACE == SEG_BASE + 0x02;
    s.add("J11-字面锚", j11, "EIO 码序对");

    // J12 诊断码闭集
    let j12 = diag_text(EE_HANGUP_EIO) == "pty 挂断读 EIO"
        && diag_text(EE_EOF_FACE) == "管道排空 EOF 读 0"
        && diag_text(7) == "未知诊断码";
    s.add("J12-码文本闭集", j12, "六码文本可辨");

    // J13 (dyn) 矩阵明细落账
    s.add_dyn("J13-矩阵明细", hits == total, &format!("hits={}/{} cap_ok={}", hits, total, cap_ok));

    // J14 (dyn) 分账注记落账
    let note = res_note(&judge(&ReadReq { face: Face::PtySlave, peer_alive: false, pending: 3 }));
    s.add_dyn("J14-分账注记", note.contains("EIO"), &note);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10844_flow() {
        assert_eq!(judge(&ReadReq { face: Face::Pipe, peer_alive: false, pending: 0 }), Res::Eof);
        assert_eq!(judge(&ReadReq { face: Face::Pipe, peer_alive: false, pending: 4 }), Res::Got(4));
        assert_eq!(judge(&ReadReq { face: Face::PtySlave, peer_alive: false, pending: 4 }), Res::Err(EE_HANGUP_EIO));
        assert_eq!(judge(&ReadReq { face: Face::Pipe, peer_alive: true, pending: 0 }), Res::Err(EE_BLOCK_FACE));
        let mut lg = ReadLedger::new();
        assert!(lg.record(&Res::Got(6)).is_ok());
        assert!(lg.record(&Res::Eof).is_ok());
        assert!(lg.record(&Res::Err(EE_HANGUP_EIO)).is_ok());
        assert!(lg.record(&Res::Err(99)).err() == Some(EE_LEDGER_MIX));
        assert_eq!(lg.data_bytes, 6);
        assert!(!lg.no_cross());
    }

    #[test]
    fn unx10844_checks_green() {
        let s = run_unx10844_checks();
        assert!(s.all_passed(), "UNX-F10844 判据未全绿");
    }
}
