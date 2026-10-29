//! UNX-F10842 对端关闭矩阵（master 关闭 slave 面）（0x1c1c0 段）
//!
//! 锚点：UNX-F10842-J1 主端关闭后从端读写面——挂断（hangup）锁存后
//! slave 读必 EIO 而非 EOF 0（流 EOF 与终端挂断分账），slave 写必 EIO
//! 而非 EPIPE；挂断前在途数据在主端存活期仍可读。正反双样本对账，
//! 注入核（EOF 冒账/EPIPE 冒账）必检出。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：关闭事件序列（主关/从读/从写）→ 挂断判定器（存活×操作×
//! 在途三输入）→ 事件账（seq 严格递增 + 挂断锁存）→ 关闭样本矩阵
//! → 注入核（两冒账坏实现必分歧）→ 序列回放（关前可读/关后 EIO）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1c1c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c1c0;
pub const PC_HANGUP_EIO_R: u32 = SEG_BASE + 0x01; // 挂断后从端读 EIO
pub const PC_HANGUP_EIO_W: u32 = SEG_BASE + 0x02; // 挂断后从端写 EIO
pub const PC_EOF_FACE: u32 = SEG_BASE + 0x03; // 存活期数据排空（EOF 面）
pub const PC_DATA_FACE: u32 = SEG_BASE + 0x04; // 存活期在途数据可读
pub const PC_BLOCK_FACE: u32 = SEG_BASE + 0x05; // 存活期无数据待读
pub const PC_SEQ_BAD: u32 = SEG_BASE + 0x06; // 事件序号回退

/// 诊断码语义文本。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        PC_HANGUP_EIO_R => "挂断后读 EIO",
        PC_HANGUP_EIO_W => "挂断后写 EIO",
        PC_EOF_FACE => "数据排空 EOF 面",
        PC_DATA_FACE => "在途数据可读",
        PC_BLOCK_FACE => "无数据待读",
        PC_SEQ_BAD => "事件序号回退",
        _ => "未知诊断码",
    }
}

/// 从端操作：读 / 写。
#[derive(Clone, PartialEq)]
pub enum Op {
    Read,
    Write,
}

/// 关闭场景：主端存活 × 操作 × 在途字节数（判定器三输入）。
#[derive(Clone)]
pub struct CloseReq {
    pub master_alive: bool,
    pub op: Op,
    pub pending: u32,
    pub want: u32,
}

/// 判定结果：Read 返回字节数，Write 全收；否则诊断码。
#[derive(Clone, PartialEq)]
pub enum Res {
    Got(u32),
    Block,
    Err(u32),
}

/// 挂断判定器：存活分账（挂断锁存覆盖 EOF 面），读写分码。
pub fn judge(req: &CloseReq) -> Res {
    if !req.master_alive {
        return match req.op {
            Op::Read => Res::Err(PC_HANGUP_EIO_R),
            Op::Write => Res::Err(PC_HANGUP_EIO_W),
        };
    }
    match req.op {
        Op::Read => {
            if req.pending > 0 {
                Res::Got(req.pending)
            } else {
                Res::Block
            }
        }
        Op::Write => Res::Got(req.pending),
    }
}

/// 关闭事件：主端关闭 / 从端读 / 从端写。
#[derive(Clone)]
pub struct Ev {
    pub seq: u64,
    pub kind: Op,
    pub is_close: bool,
    pub bytes: u32,
}

/// 事件账：seq 严格递增、挂断锁存、在途数据随读排空。
pub struct EvLedger {
    pub rows: Vec<Ev>,
    pub hangup: bool,
    pub drained: u32,
    pub last_seq: u64,
    pub note: String,
}

impl EvLedger {
    pub fn new() -> EvLedger {
        EvLedger {
            rows: Vec::new(),
            hangup: false,
            drained: 0,
            last_seq: 0,
            note: "open".to_string(),
        }
    }

    /// 入账：seq 回退拒绝（PC_SEQ_BAD）；主关事件锁存挂断。
    pub fn feed(&mut self, ev: Ev) -> Result<(), u32> {
        if !self.rows.is_empty() && ev.seq <= self.last_seq {
            return Err(PC_SEQ_BAD);
        }
        if ev.is_close {
            self.hangup = true;
            self.note = "hangup".to_string();
        }
        self.last_seq = ev.seq;
        if !ev.is_close && !self.hangup && ev.kind == Op::Read {
            self.drained += ev.bytes;
        }
        self.rows.push(ev);
        Ok(())
    }

    /// 序严格账：seq 单调递增核。
    pub fn seq_strict(&self) -> bool {
        let mut ok = true;
        let mut i = 1usize;
        while i < self.rows.len() {
            if self.rows[i].seq <= self.rows[i - 1].seq {
                ok = false;
            }
            i += 1;
        }
        ok
    }

    /// 回放核：挂断前读入账、挂断后读 EIO 账，两段可辨。
    pub fn replay_ok(&self) -> bool {
        let mut pre_read = 0usize;
        let mut post_op = 0usize;
        let mut i = 0usize;
        while i < self.rows.len() {
            if !self.rows[i].is_close && self.rows[i].kind == Op::Read {
                pre_read += 1;
            } else if self.hangup && self.rows[i].seq >= self.last_seq {
                post_op += 1;
            }
            i += 1;
        }
        pre_read > 0 && post_op > 0 && self.hangup
    }

    /// 账面一致：排空量不超过总在途。
    pub fn ledger_ok(&self, total_pending: u32) -> bool {
        self.drained <= total_pending && self.seq_strict()
    }
}

/// 样本矩阵容量。
pub const MATRIX_CAP: usize = 20;

/// 矩阵逐格判定：命中（与期望一致）计数。
pub fn verify_matrix(reqs: &[CloseReq]) -> (usize, usize, bool) {
    let mut hits = 0usize;
    let total = reqs.len();
    let mut i = 0usize;
    while i < total && i < MATRIX_CAP {
        let got = judge(&reqs[i]);
        let want: Res = if reqs[i].want == 0 {
            Res::Block
        } else if reqs[i].want == 1 {
            Res::Got(reqs[i].pending)
        } else {
            Res::Err(reqs[i].want)
        };
        if got == want {
            hits += 1;
        }
        i += 1;
    }
    (hits, total.min(MATRIX_CAP), hits == total.min(MATRIX_CAP))
}

/// 样本矩阵：存活/挂断 × 读/写 × 在途，8 行全分账。
pub fn sample_matrix() -> Vec<CloseReq> {
    let mut v: Vec<CloseReq> = Vec::new();
    // R0 存活+在途+读 → Got(n)
    v.push(CloseReq { master_alive: true, op: Op::Read, pending: 12, want: 1 });
    // R1 存活+无数据+读 → Block
    v.push(CloseReq { master_alive: true, op: Op::Read, pending: 0, want: 0 });
    // R2 存活+写 → Got(0)（写面恒全收）
    v.push(CloseReq { master_alive: true, op: Op::Write, pending: 5, want: 1 });
    // R3 挂断+读（即便有在途）→ EIO 读
    v.push(CloseReq { master_alive: false, op: Op::Read, pending: 7, want: PC_HANGUP_EIO_R });
    // R4 挂断+读（无数据）→ EIO 读
    v.push(CloseReq { master_alive: false, op: Op::Read, pending: 0, want: PC_HANGUP_EIO_R });
    // R5 挂断+写 → EIO 写
    v.push(CloseReq { master_alive: false, op: Op::Write, pending: 3, want: PC_HANGUP_EIO_W });
    // R6 存活+大块读 → Got(满)
    v.push(CloseReq { master_alive: true, op: Op::Read, pending: 4096, want: 1 });
    // R7 挂断+写（零长也 EIO）
    v.push(CloseReq { master_alive: false, op: Op::Write, pending: 0, want: PC_HANGUP_EIO_W });
    v
}

/// 注入 A：坏实现挂断后读返 EOF 面（Got(0) 而非 EIO）。
pub fn faulty_eof_instead(req: &CloseReq) -> Res {
    if !req.master_alive && req.op == Op::Read {
        Res::Got(0)
    } else {
        judge(req)
    }
}

/// 注入 B：坏实现挂断后写返 EPIPE 面（流管道码而非终端码）。
pub fn faulty_epipe_face(req: &CloseReq) -> Res {
    if !req.master_alive && req.op == Op::Write {
        Res::Err(PC_HANGUP_EIO_W.wrapping_sub(1))
    } else {
        judge(req)
    }
}

/// 注入核：两处冒账与判定器必分歧。
pub fn inject_two() -> [bool; 2] {
    let rd = CloseReq { master_alive: false, op: Op::Read, pending: 0, want: 0 };
    let wr = CloseReq { master_alive: false, op: Op::Write, pending: 1, want: 0 };
    let d1 = judge(&rd) != faulty_eof_instead(&rd);
    let d2 = judge(&wr) != faulty_epipe_face(&wr);
    [d1, d2]
}

/// 序列回放：关前读排空在途、主关锁存、关后读 EIO。
pub fn seq_replay_ok() -> bool {
    let mut lg = EvLedger::new();
    let r1 = lg.feed(Ev { seq: 1, kind: Op::Read, is_close: false, bytes: 10 });
    let r2 = lg.feed(Ev { seq: 2, kind: Op::Write, is_close: false, bytes: 0 });
    let r3 = lg.feed(Ev { seq: 3, kind: Op::Read, is_close: true, bytes: 0 });
    let r4 = lg.feed(Ev { seq: 4, kind: Op::Read, is_close: false, bytes: 4 });
    let r5 = lg.feed(Ev { seq: 3, kind: Op::Read, is_close: false, bytes: 0 });
    r1.is_ok() && r2.is_ok() && r3.is_ok() && r4.is_ok()
        && r5.err() == Some(PC_SEQ_BAD)
        && lg.hangup
        && lg.seq_strict()
        && lg.drained == 10
}

/// 挂断前在途核：存活期数据可读、排空后待读。
pub fn pending_face_ok() -> bool {
    let with = judge(&CloseReq { master_alive: true, op: Op::Read, pending: 9, want: 0 });
    let out = judge(&CloseReq { master_alive: true, op: Op::Read, pending: 0, want: 0 });
    with == Res::Got(9) && out == Res::Block
}

/// 判定注记：行语义渲染。
pub fn res_note(r: &Res) -> String {
    match r {
        Res::Got(n) => format!("got={}", n),
        Res::Block => diag_text(PC_BLOCK_FACE).to_string(),
        Res::Err(c) => diag_text(*c).to_string(),
    }
}

/// 域判据（对应卡验收 J 判据）。
pub fn run_unx10842_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10842");
    let reqs = sample_matrix();
    let (hits, total, cap_ok) = verify_matrix(&reqs);

    // J1 正路径：存活+在途+读 → Got
    s.add(
        "J1-存活在途读",
        judge(&CloseReq { master_alive: true, op: Op::Read, pending: 6, want: 0 }) == Res::Got(6),
        "在途数据全量可读",
    );

    // J2 边界：零长在途读 → Block 面
    let j2 = judge(&CloseReq { master_alive: true, op: Op::Read, pending: 0, want: 0 }) == Res::Block;
    s.add("J2-零长待读", j2, "排空即待读面");

    // J3 挂断读面：EIO 而非 EOF（有/无在途同码）
    let j3 = judge(&CloseReq { master_alive: false, op: Op::Read, pending: 8, want: 0 })
        == Res::Err(PC_HANGUP_EIO_R)
        && judge(&CloseReq { master_alive: false, op: Op::Read, pending: 0, want: 0 })
            == Res::Err(PC_HANGUP_EIO_R);
    s.add("J3-挂断读 EIO", j3, "挂断覆盖 EOF 面");

    // J4 挂断写面：EIO 而非 EPIPE
    s.add(
        "J4-挂断写 EIO",
        judge(&CloseReq { master_alive: false, op: Op::Write, pending: 2, want: 0 })
            == Res::Err(PC_HANGUP_EIO_W),
        "终端码非流码",
    );

    // J5 挂断前在途核
    s.add("J5-在途分账", pending_face_ok(), "Got(9)/Block 两面可辨");

    // J6 样本矩阵全命中
    s.add("J6-矩阵对平", cap_ok, "8 行挂断/存活全分账");

    // J7 注入1 EOF 冒账检出
    let inj = inject_two();
    s.add("J7-EOF 冒账检出", inj[0], "挂断读返 0 必分歧");

    // J8 注入2 EPIPE 冒账检出
    s.add("J8-EPIPE 冒账检出", inj[1], "挂断写错码必分歧");

    // J9 序列回放（seq 严格 + 回退拒绝 + 挂断锁存）
    s.add("J9-序列回放", seq_replay_ok(), "关前读/关后账/回退拒三可辨");

    // J10 账面一致：排空 ≤ 总在途
    let mut lg = EvLedger::new();
    let _ = lg.feed(Ev { seq: 1, kind: Op::Read, is_close: false, bytes: 5 });
    let _ = lg.feed(Ev { seq: 2, kind: Op::Read, is_close: true, bytes: 0 });
    s.add("J10-账面一致", lg.ledger_ok(5) && !lg.ledger_ok(3), "排空量封顶");

    // J11 诊断码闭集
    let j11 = diag_text(PC_HANGUP_EIO_R) == "挂断后读 EIO"
        && diag_text(PC_EOF_FACE) == "数据排空 EOF 面"
        && diag_text(0xdead) == "未知诊断码";
    s.add("J11-码文本闭集", j11, "六码文本可辨");

    // J12 码字面锚：读/写挂断码相隔一位
    let j12 = PC_HANGUP_EIO_W == PC_HANGUP_EIO_R + 1 && PC_SEQ_BAD == SEG_BASE + 0x06;
    s.add("J12-码字面锚", j12, "读写码序对");

    // J13 (dyn) 矩阵明细落账
    s.add_dyn("J13-矩阵明细", hits == total, &format!("hits={}/{} cap_ok={}", hits, total, cap_ok));

    // J14 (dyn) 回放注记落账
    let note = res_note(&judge(&CloseReq { master_alive: false, op: Op::Read, pending: 4, want: 0 }));
    s.add_dyn(
        "J14-回放注记",
        note.contains("EIO"),
        &format!("note={} drained={}", note, lg.drained),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10842_flow() {
        let mut lg = EvLedger::new();
        assert!(lg.feed(Ev { seq: 1, kind: Op::Read, is_close: false, bytes: 7 }).is_ok());
        assert!(lg.feed(Ev { seq: 1, kind: Op::Read, is_close: false, bytes: 1 }).err() == Some(PC_SEQ_BAD));
        assert!(lg.feed(Ev { seq: 2, kind: Op::Read, is_close: true, bytes: 0 }).is_ok());
        assert!(lg.hangup && lg.seq_strict());
        assert!(judge(&CloseReq { master_alive: false, op: Op::Read, pending: 1, want: 0 }) == Res::Err(PC_HANGUP_EIO_R));
        assert!(judge(&CloseReq { master_alive: false, op: Op::Write, pending: 0, want: 0 }) == Res::Err(PC_HANGUP_EIO_W));
        assert!(judge(&CloseReq { master_alive: true, op: Op::Read, pending: 3, want: 0 }) == Res::Got(3));
        assert!(judge(&CloseReq { master_alive: true, op: Op::Read, pending: 0, want: 0 }) == Res::Block);
    }

    #[test]
    fn unx10842_checks_green() {
        let s = run_unx10842_checks();
        assert!(s.all_passed(), "UNX-F10842 判据未全绿");
    }
}
