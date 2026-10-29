//! UNX-F10862 ECONNRESET 矩阵（对端异常关闭）（0x1c440 段）
//!
//! 锚点：UNX-F10862-J1 RST 矩阵——对端异常关闭（RST）后：首次读必
//! ECONNRESET（104，非 EOF 冒账）；后续读转 EOF 面（Got(0)，RST 已
//! 通报不再重复）；首次写必 ECONNRESET、后续写转 EPIPE 纪（此处记
//! 拒写账）；正常连接面读写均交付。判定依赖场景字段演算，注入核
//! （RST 冒 EOF/复位失明坏实现）必分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：复位场景（RST × 次数 × op）→ 复位判定器（首次/后续双沿）
//! → 套接字状态机（reset/读写走账）→ 样本矩阵（八行正反对）→ 注入
//! 核（两坏实现必分歧）→ 复位回放（RST 通报→EOF 双沿对平）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1c440 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c440;
pub const RS_RESET_READ: u32 = SEG_BASE + 0x01; // RST 后首读
pub const RS_EOF_AFTER: u32 = SEG_BASE + 0x02; // RST 通报后 EOF 面
pub const RS_RESET_WRITE: u32 = SEG_BASE + 0x03; // RST 后首写
pub const RS_WRITE_DENY: u32 = SEG_BASE + 0x04; // RST 后续写拒写面
pub const RS_RST_GHOST: u32 = SEG_BASE + 0x05; // RST 冒 EOF
pub const RS_LEDGER_LOST: u32 = SEG_BASE + 0x06; // 复位账失平

/// 语义字面锚。
pub const ECONNRESET: u32 = 104;
pub const EPIPE: u32 = 32;

/// 诊断码语义文本。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        RS_RESET_READ => "RST 后首读",
        RS_EOF_AFTER => "RST 通报后 EOF 面",
        RS_RESET_WRITE => "RST 后首写",
        RS_WRITE_DENY => "RST 后续写拒写面",
        RS_RST_GHOST => "RST 冒 EOF",
        RS_LEDGER_LOST => "复位账失平",
        _ => "未知诊断码",
    }
}

/// 复位场景：对端 RST × 读次数 × 写次数（判定器三输入）。
#[derive(Clone)]
pub struct RsReq {
    pub rst: bool,
    pub reads: u32,
    pub writes: u32,
}

/// 判定结果：Got(0 为 EOF 面) / Err(码)。
#[derive(Clone, PartialEq, Debug)]
pub enum Res {
    Got(u32),
    Err(u32),
}

/// 复位判定器：非 RST 态读/写均交付；RST 态首读 ECONNRESET、后续
/// 读 Got(0)；RST 态首写 ECONNRESET、后续写 EPIPE 纪。
pub fn judge_read(req: &RsReq) -> Res {
    if !req.rst {
        return Res::Got(1);
    }
    if req.reads == 0 {
        Res::Err(RS_RESET_READ)
    } else {
        Res::Got(0)
    }
}

pub fn judge_write(req: &RsReq) -> Res {
    if !req.rst {
        return Res::Got(1);
    }
    if req.writes == 0 {
        Res::Err(RS_RESET_WRITE)
    } else {
        Res::Err(EPIPE)
    }
}

/// 套接字状态机：RST 旗、读写次数账。
pub struct ResetSock {
    pub rst: bool,
    pub reads: u32,
    pub writes: u32,
    pub reset_events: u32,
    pub note: String,
}

impl ResetSock {
    pub fn new() -> ResetSock {
        ResetSock { rst: false, reads: 0, writes: 0, reset_events: 0, note: "live".to_string() }
    }

    /// 对端异常关闭：置 RST 旗（重复复位不重复计账）。
    pub fn peer_reset(&mut self) {
        if !self.rst {
            self.rst = true;
            self.reset_events += 1;
            self.note = "rst".to_string();
        }
    }

    /// 读：判定走账。
    pub fn read(&mut self) -> Res {
        let req = RsReq { rst: self.rst, reads: self.reads, writes: self.writes };
        self.reads += 1;
        let r = judge_read(&req);
        if r == Res::Got(0) {
            self.note = "eof-after-rst".to_string();
        }
        r
    }

    /// 写：判定走账。
    pub fn write(&mut self) -> Res {
        let req = RsReq { rst: self.rst, reads: self.reads, writes: self.writes };
        self.writes += 1;
        judge_write(&req)
    }

    /// 复位账一致：reset_events ≤ 1 且 RST 态必有读写账可辨。
    pub fn ledger_ok(&self) -> bool {
        self.reset_events <= 1 && (!self.rst || self.note != "")
    }
}

/// 样本矩阵容量。
pub const MATRIX_CAP: usize = 24;

/// 矩阵逐格判定（读轴）：期望（Got(n) 编码 n+1，Err(c) 直码）命中计数。
pub fn verify_matrix(reqs: &[RsReq], wants: &[u32]) -> (usize, usize, bool) {
    let mut hits = 0usize;
    let total = reqs.len();
    let mut i = 0usize;
    while i < total && i < MATRIX_CAP {
        let want = if i < wants.len() { wants[i] } else { 0 };
        let hit = match judge_read(&reqs[i]) {
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

/// 样本矩阵：RST/正常 × 首读/次读，8 行正反对（读轴）。
pub fn sample_matrix() -> (Vec<RsReq>, Vec<u32>) {
    let mut reqs: Vec<RsReq> = Vec::new();
    let mut wants: Vec<u32> = Vec::new();
    // R0 RST 首读 → ECONNRESET
    reqs.push(RsReq { rst: true, reads: 0, writes: 0 });
    wants.push(RS_RESET_READ);
    // R1 RST 次读 → EOF 面
    reqs.push(RsReq { rst: true, reads: 1, writes: 0 });
    wants.push(1);
    // R2 正常首读 → 交付
    reqs.push(RsReq { rst: false, reads: 0, writes: 0 });
    wants.push(2);
    // R3 正常次读 → 交付
    reqs.push(RsReq { rst: false, reads: 1, writes: 3 });
    wants.push(2);
    // R4 RST 后第二次读 → EOF 面（通报一次后恒 Got(0)）
    reqs.push(RsReq { rst: true, reads: 2, writes: 0 });
    wants.push(1);
    // R5 RST 后多读 → EOF 面
    reqs.push(RsReq { rst: true, reads: 5, writes: 0 });
    wants.push(1);
    // R6 正常多次读 → 交付
    reqs.push(RsReq { rst: false, reads: 9, writes: 0 });
    wants.push(2);
    // R7 复核格：RST 后多读转 EOF 面（首读误期 EOF 由注入核把守）
    reqs.push(RsReq { rst: true, reads: 3, writes: 9 });
    wants.push(1);
    (reqs, wants)
}

/// 注入 A：坏实现 RST 冒 EOF（首读误 Got(0)）。
pub fn faulty_rst_ghost(req: &RsReq) -> Res {
    if req.rst {
        Res::Got(0)
    } else {
        judge_read(req)
    }
}

/// 注入 B：坏实现复位失明（RST 态误按正常交付）。
pub fn faulty_rst_blind(req: &RsReq) -> Res {
    let mut fixed = req.clone();
    fixed.rst = false;
    judge_read(&fixed)
}

/// 注入核：两处坏账与判定器必分歧。
pub fn inject_two() -> [bool; 2] {
    let first = RsReq { rst: true, reads: 0, writes: 0 };
    let d1 = judge_read(&first) != faulty_rst_ghost(&first);
    let d2 = judge_read(&first) != faulty_rst_blind(&first);
    [d1, d2]
}

/// 复位回放：RST → 首读 ECONNRESET → 次读 EOF → 首写 ECONNRESET
/// → 次写 EPIPE，账面各一。
pub fn rst_replay_ok() -> bool {
    let mut sock = ResetSock::new();
    sock.peer_reset();
    let r1 = sock.read();
    let r2 = sock.read();
    let w1 = sock.write();
    let w2 = sock.write();
    r1 == Res::Err(RS_RESET_READ) && r2 == Res::Got(0)
        && w1 == Res::Err(RS_RESET_WRITE) && w2 == Res::Err(EPIPE)
        && sock.reset_events == 1 && sock.ledger_ok()
}

/// 正常回放：无 RST 读写全交付、复位事件零。
pub fn live_replay_ok() -> bool {
    let mut sock = ResetSock::new();
    let r = sock.read();
    let w = sock.write();
    r == Res::Got(1) && w == Res::Got(1) && sock.reset_events == 0
        && sock.reads == 1 && sock.writes == 1 && sock.ledger_ok()
}

/// 复位注记：判定渲染。
pub fn rs_note(r: &Res) -> String {
    match r {
        Res::Got(0) => diag_text(RS_EOF_AFTER).to_string(),
        Res::Got(n) => format!("got={}", n),
        Res::Err(c) => format!("{} ({})", diag_text(*c), c),
    }
}

/// 域判据（对应卡验收 J 判据）。
pub fn run_unx10862_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10862");
    let (reqs, wants) = sample_matrix();
    let (hits, total, cap_ok) = verify_matrix(&reqs, &wants);

    // J1 正路径：RST 首读 ECONNRESET
    let j1 = judge_read(&RsReq { rst: true, reads: 0, writes: 0 }) == Res::Err(RS_RESET_READ);
    s.add("J1-RST 首读", j1, "ECONNRESET 104");

    // J2 反路径：RST 次读转 EOF 面
    let j2 = judge_read(&RsReq { rst: true, reads: 1, writes: 0 }) == Res::Got(0);
    s.add("J2-通报后 EOF", j2, "Got(0) 面");

    // J3 写轴双沿：首写 ECONNRESET / 次写 EPIPE
    let j3 = judge_write(&RsReq { rst: true, reads: 0, writes: 0 }) == Res::Err(RS_RESET_WRITE)
        && judge_write(&RsReq { rst: true, reads: 0, writes: 1 }) == Res::Err(EPIPE);
    s.add("J3-写轴双沿", j3, "104→32 次序可辨");

    // J4 正常面不误伤：无 RST 读写全交付
    let j4 = judge_read(&RsReq { rst: false, reads: 0, writes: 0 }) == Res::Got(1)
        && judge_write(&RsReq { rst: false, reads: 0, writes: 0 }) == Res::Got(1);
    s.add("J4-正常面", j4, "RST 不越权");

    // J5 复位回放（104→EOF→104→32 次序）
    s.add("J5-复位回放", rst_replay_ok(), "四步次序账对平");

    // J6 正常回放
    s.add("J6-正常回放", live_replay_ok(), "复位事件零");

    // J7 样本矩阵全命中
    s.add("J7-矩阵对平", cap_ok, "8 行正反对全命中");

    // J8 注入1 RST 冒 EOF 检出
    let inj = inject_two();
    s.add("J8-RST 冒 EOF 检出", inj[0], "首读误 0 必分歧");

    // J9 注入2 复位失明检出
    s.add("J9-复位失明检出", inj[1], "RST 误交付必分歧");

    // J10 复位账一致（重复复位不重复计）
    let mut sock = ResetSock::new();
    sock.peer_reset();
    sock.peer_reset();
    s.add("J10-复位账", sock.ledger_ok() && sock.reset_events == 1, "RST 幂等");

    // J11 字面锚：ECONNRESET=104 / EPIPE=32
    let j11 = ECONNRESET == 104 && EPIPE == 32;
    s.add("J11-字面锚", j11, "两码常量");

    // J12 诊断码闭集
    let j12 = diag_text(RS_RESET_READ) == "RST 后首读"
        && diag_text(RS_EOF_AFTER) == "RST 通报后 EOF 面"
        && diag_text(9) == "未知诊断码";
    s.add("J12-码文本闭集", j12, "六码文本可辨");

    // J13 (dyn) 矩阵明细落账
    s.add_dyn("J13-矩阵明细", hits == total, &format!("hits={}/{} cap_ok={}", hits, total, cap_ok));

    // J14 (dyn) 复位注记落账
    let note = rs_note(&judge_read(&RsReq { rst: true, reads: 0, writes: 0 }));
    s.add_dyn("J14-复位注记", note.contains("104"), &note);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10862_flow() {
        assert_eq!(judge_read(&RsReq { rst: true, reads: 0, writes: 0 }), Res::Err(RS_RESET_READ));
        assert_eq!(judge_read(&RsReq { rst: true, reads: 1, writes: 0 }), Res::Got(0));
        assert_eq!(judge_write(&RsReq { rst: true, reads: 0, writes: 0 }), Res::Err(RS_RESET_WRITE));
        assert_eq!(judge_write(&RsReq { rst: true, reads: 0, writes: 1 }), Res::Err(EPIPE));
        let mut sock = ResetSock::new();
        sock.peer_reset();
        assert_eq!(sock.read(), Res::Err(RS_RESET_READ));
        assert_eq!(sock.read(), Res::Got(0));
        assert!(sock.ledger_ok());
    }

    #[test]
    fn unx10862_checks_green() {
        let s = run_unx10862_checks();
        assert!(s.all_passed(), "UNX-F10862 判据未全绿");
    }
}
