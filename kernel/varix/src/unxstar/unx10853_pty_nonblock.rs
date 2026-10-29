//! UNX-F10853 pty O_NONBLOCK 读写矩阵（EAGAIN 正反）（0x1c320 段）
//!
//! 锚点：UNX-F10853-J1 非阻塞读写正反矩阵——O_NONBLOCK 置位后无数据
//! 读必 EAGAIN（11，非 EOF 0 面）、缓冲不足写必 EAGAIN；同状态下
//! 清位后必转阻塞面（正反两沿：同态换旗换面）；有数据/有空间时旗标
//! 不改变交付量。判定依赖场景字段演算，注入核（EOF 冒账/旗标失明）
//! 必分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：读写场景（nonblock × op × 现量 × 空间）→ 正反判定器（旗标
//! × 水位四分支）→ 非阻塞状态机（F_SETFL 翻旗/读写走账）→ 正反样本
//! 矩阵（同态双旗封顶）→ 注入核（两坏实现必分歧）→ 翻旗回放
//! （同态 EAGAIN↔Block 双沿对平）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1c320 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c320;
pub const NB_READ_EAGAIN: u32 = SEG_BASE + 0x01; // 非阻塞无数据读 EAGAIN
pub const NB_WRITE_EAGAIN: u32 = SEG_BASE + 0x02; // 非阻塞无空间写 EAGAIN
pub const NB_BLOCK_FACE: u32 = SEG_BASE + 0x03; // 阻塞面
pub const NB_DATA_FACE: u32 = SEG_BASE + 0x04; // 数据交付面
pub const NB_FLAG_MISS: u32 = SEG_BASE + 0x05; // 旗标账丢失
pub const NB_LEDGER_LOST: u32 = SEG_BASE + 0x06; // 水位账失平

/// 语义字面锚。
pub const EAGAIN: u32 = 11;
pub const O_NONBLOCK: u32 = 0o4000;
pub const BUF_CAP: u32 = 4096;

/// 诊断码语义文本。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        NB_READ_EAGAIN => "非阻塞读 EAGAIN",
        NB_WRITE_EAGAIN => "非阻塞写 EAGAIN",
        NB_BLOCK_FACE => "阻塞面",
        NB_DATA_FACE => "数据交付面",
        NB_FLAG_MISS => "旗标账丢失",
        NB_LEDGER_LOST => "水位账失平",
        _ => "未知诊断码",
    }
}

/// 读写操作：读 / 写(want)。
#[derive(Clone, PartialEq)]
pub enum NbOp {
    Read,
    Write(u32),
}

/// 读写场景：旗标 × 操作 × 现量 × 空闲空间（判定器四输入）。
#[derive(Clone)]
pub struct NbReq {
    pub nonblock: bool,
    pub op: NbOp,
    pub avail: u32,
    pub free: u32,
}

/// 判定结果：Got(交付字节数) / Block（阻塞面） / Err(EAGAIN)。
#[derive(Clone, PartialEq, Debug)]
pub enum Res {
    Got(u32),
    Block,
    Err(u32),
}

/// 正反判定器：有水走水，无水看旗（旗位 EAGAIN / 旗清 Block）。
pub fn judge(req: &NbReq) -> Res {
    match &req.op {
        NbOp::Read => {
            if req.avail > 0 {
                Res::Got(req.avail)
            } else if req.nonblock {
                Res::Err(NB_READ_EAGAIN)
            } else {
                Res::Block
            }
        }
        NbOp::Write(want) => {
            if req.free >= *want {
                Res::Got(*want)
            } else if req.nonblock {
                Res::Err(NB_WRITE_EAGAIN)
            } else {
                Res::Block
            }
        }
    }
}

/// 非阻塞状态机：旗标（F_SETFL 语义）、读写水位账。
pub struct NbPty {
    pub nonblock: bool,
    pub avail: u32,
    pub free: u32,
    pub reads: u32,
    pub writes: u32,
    pub setfl_count: u32,
    pub note: String,
}

impl NbPty {
    pub fn new(avail: u32, free: u32) -> NbPty {
        NbPty {
            nonblock: false,
            avail,
            free,
            reads: 0,
            writes: 0,
            setfl_count: 0,
            note: "blocking".to_string(),
        }
    }

    /// F_SETFL：置/清 O_NONBLOCK，旗标账落笔。
    pub fn setfl(&mut self, on: bool) {
        self.nonblock = on;
        self.setfl_count += 1;
        self.note = if on { "nonblock".to_string() } else { "blocking".to_string() };
    }

    /// 读：水位账随交付回退。
    pub fn read(&mut self) -> Res {
        let req = NbReq { nonblock: self.nonblock, op: NbOp::Read, avail: self.avail, free: self.free };
        match judge(&req) {
            Res::Got(n) => {
                self.avail -= n;
                self.free += n;
                self.reads += 1;
                self.note = "read".to_string();
                Res::Got(n)
            }
            other => {
                self.reads += 1;
                other
            }
        }
    }

    /// 写：水位账随写入推进。
    pub fn write(&mut self, want: u32) -> Res {
        let req = NbReq { nonblock: self.nonblock, op: NbOp::Write(want), avail: self.avail, free: self.free };
        match judge(&req) {
            Res::Got(n) => {
                self.free -= n;
                self.avail += n;
                self.writes += 1;
                self.note = "written".to_string();
                Res::Got(n)
            }
            other => {
                self.writes += 1;
                other
            }
        }
    }

    /// 水位账一致：avail+free 恒等于 BUF_CAP。
    pub fn ledger_ok(&self) -> bool {
        self.avail + self.free == BUF_CAP
    }
}

/// 样本矩阵容量。
pub const MATRIX_CAP: usize = 24;

/// 矩阵逐格判定：期望（Got(n) 编码 n+1，Block 0，Err(c) 直码）命中计数。
pub fn verify_matrix(reqs: &[NbReq], wants: &[u32]) -> (usize, usize, bool) {
    let mut hits = 0usize;
    let total = reqs.len();
    let mut i = 0usize;
    while i < total && i < MATRIX_CAP {
        let want = if i < wants.len() { wants[i] } else { 0 };
        let hit = match judge(&reqs[i]) {
            Res::Got(n) => want == n + 1,
            Res::Block => want == 0,
            Res::Err(c) => c == want,
        };
        if hit {
            hits += 1;
        }
        i += 1;
    }
    (hits, total.min(MATRIX_CAP), hits == total.min(MATRIX_CAP))
}

/// 样本矩阵：读/写 × 旗位/旗清 × 有水/无水，8 行正反对。
pub fn sample_matrix() -> (Vec<NbReq>, Vec<u32>) {
    let mut reqs: Vec<NbReq> = Vec::new();
    let mut wants: Vec<u32> = Vec::new();
    // R0 旗位+无数据读 → EAGAIN
    reqs.push(NbReq { nonblock: true, op: NbOp::Read, avail: 0, free: BUF_CAP });
    wants.push(NB_READ_EAGAIN);
    // R1 旗清+无数据读 → Block
    reqs.push(NbReq { nonblock: false, op: NbOp::Read, avail: 0, free: BUF_CAP });
    wants.push(0);
    // R2 旗位+有数据读 → Got（旗不压水）
    reqs.push(NbReq { nonblock: true, op: NbOp::Read, avail: 7, free: BUF_CAP - 7 });
    wants.push(8);
    // R3 旗清+有数据读 → Got
    reqs.push(NbReq { nonblock: false, op: NbOp::Read, avail: 7, free: BUF_CAP - 7 });
    wants.push(8);
    // R4 旗位+无空间写 → EAGAIN
    reqs.push(NbReq { nonblock: true, op: NbOp::Write(16), avail: BUF_CAP, free: 0 });
    wants.push(NB_WRITE_EAGAIN);
    // R5 旗清+无空间写 → Block
    reqs.push(NbReq { nonblock: false, op: NbOp::Write(16), avail: BUF_CAP, free: 0 });
    wants.push(0);
    // R6 旗位+有空间写 → Got
    reqs.push(NbReq { nonblock: true, op: NbOp::Write(16), avail: 100, free: BUF_CAP - 100 });
    wants.push(17);
    // R7 边界：恰满空间写 → Got
    reqs.push(NbReq { nonblock: true, op: NbOp::Write(16), avail: BUF_CAP - 16, free: 16 });
    wants.push(17);
    (reqs, wants)
}

/// 注入 A：坏实现非阻塞读返 EOF 面（Got(0) 而非 EAGAIN）。
pub fn faulty_nb_zero(req: &NbReq) -> Res {
    if req.nonblock && req.op == NbOp::Read && req.avail == 0 {
        Res::Got(0)
    } else {
        judge(req)
    }
}

/// 注入 B：坏实现旗标失明（非阻塞仍报阻塞面）。
pub fn faulty_flag_ignore(req: &NbReq) -> Res {
    let mut fixed = req.clone();
    fixed.nonblock = false;
    judge(&fixed)
}

/// 注入核：两处坏账与判定器必分歧。
pub fn inject_two() -> [bool; 2] {
    let rd = NbReq { nonblock: true, op: NbOp::Read, avail: 0, free: BUF_CAP };
    let wr = NbReq { nonblock: true, op: NbOp::Write(8), avail: BUF_CAP, free: 0 };
    let d1 = judge(&rd) != faulty_nb_zero(&rd);
    let d2 = judge(&wr) != faulty_flag_ignore(&wr);
    [d1, d2]
}

/// 翻旗回放：同态下旗位 EAGAIN ↔ 旗清 Block 双沿，旗标账落笔。
pub fn toggle_replay_ok() -> bool {
    let mut p = NbPty::new(0, BUF_CAP);
    p.setfl(true);
    let on = p.read();
    p.setfl(false);
    let off = p.read();
    on == Res::Err(NB_READ_EAGAIN) && off == Res::Block
        && p.setfl_count == 2 && p.nonblock == false && p.ledger_ok()
}

/// 写面回放：满态旗位 EAGAIN、腾空后放行。
pub fn write_replay_ok() -> bool {
    let mut p = NbPty::new(BUF_CAP, 0);
    p.setfl(true);
    let full = p.write(8);
    let _ = p.read();
    let freed = p.write(8);
    full == Res::Err(NB_WRITE_EAGAIN) && freed == Res::Got(8)
        && p.avail == BUF_CAP - 8 && p.ledger_ok()
}

/// 正反注记：判定渲染。
pub fn nb_note(r: &Res) -> String {
    match r {
        Res::Got(n) => format!("got={}", n),
        Res::Block => diag_text(NB_BLOCK_FACE).to_string(),
        Res::Err(c) => format!("{} ({})", diag_text(*c), c),
    }
}

/// 域判据（对应卡验收 J 判据）。
pub fn run_unx10853_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10853");
    let (reqs, wants) = sample_matrix();
    let (hits, total, cap_ok) = verify_matrix(&reqs, &wants);

    // J1 正路径：旗位无数据读 EAGAIN
    let j1 = judge(&NbReq { nonblock: true, op: NbOp::Read, avail: 0, free: BUF_CAP })
        == Res::Err(NB_READ_EAGAIN);
    s.add("J1-读 EAGAIN", j1, "非阻塞无数据即回");

    // J2 反路径：同态旗清转 Block
    let j2 = judge(&NbReq { nonblock: false, op: NbOp::Read, avail: 0, free: BUF_CAP })
        == Res::Block;
    s.add("J2-同态转阻塞", j2, "旗清 Block 面");

    // J3 边界：写恰满空间放行 / 恰缺一 EAGAIN
    let j3 = judge(&NbReq { nonblock: true, op: NbOp::Write(16), avail: 0, free: 16 })
        == Res::Got(16)
        && judge(&NbReq { nonblock: true, op: NbOp::Write(16), avail: 0, free: 15 })
            == Res::Err(NB_WRITE_EAGAIN);
    s.add("J3-写边界", j3, "16/15 两沿可辨");

    // J4 旗不压水：有数据读旗位/旗清同交付
    let j4 = judge(&NbReq { nonblock: true, op: NbOp::Read, avail: 7, free: BUF_CAP - 7 })
        == Res::Got(7)
        && judge(&NbReq { nonblock: false, op: NbOp::Read, avail: 7, free: BUF_CAP - 7 })
            == Res::Got(7);
    s.add("J4-旗不压水", j4, "交付量与旗无关");

    // J5 翻旗回放
    s.add("J5-翻旗回放", toggle_replay_ok(), "EAGAIN↔Block 双沿对平");

    // J6 写面回放（满态→腾空）
    s.add("J6-写面回放", write_replay_ok(), "满态 EAGAIN/腾空放行");

    // J7 样本矩阵全命中
    s.add("J7-矩阵对平", cap_ok, "8 行正反对全命中");

    // J8 注入1 EOF 冒账检出
    let inj = inject_two();
    s.add("J8-EOF 冒账检出", inj[0], "EAGAIN 误 0 必分歧");

    // J9 注入2 旗标失明检出
    s.add("J9-旗标失明检出", inj[1], "旗位误报阻塞必分歧");

    // J10 水位账一致
    let mut p = NbPty::new(100, BUF_CAP - 100);
    let _ = p.read();
    s.add("J10-水位账", p.ledger_ok() && p.avail == 0 && p.free == BUF_CAP, "读后水位回平");

    // J11 字面锚：EAGAIN=11 / O_NONBLOCK=0o4000 / BUF_CAP=4096
    let j11 = EAGAIN == 11 && O_NONBLOCK == 0o4000 && BUF_CAP == 4096;
    s.add("J11-字面锚", j11, "旗标与容量常量");

    // J12 诊断码闭集
    let j12 = diag_text(NB_READ_EAGAIN) == "非阻塞读 EAGAIN"
        && diag_text(NB_BLOCK_FACE) == "阻塞面"
        && diag_text(9) == "未知诊断码";
    s.add("J12-码文本闭集", j12, "六码文本可辨");

    // J13 (dyn) 矩阵明细落账
    s.add_dyn("J13-矩阵明细", hits == total, &format!("hits={}/{} cap_ok={}", hits, total, cap_ok));

    // J14 (dyn) 正反注记落账
    let note = nb_note(&judge(&NbReq { nonblock: true, op: NbOp::Write(4), avail: BUF_CAP, free: 0 }));
    s.add_dyn("J14-正反注记", note.contains("EAGAIN"), &note);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10853_flow() {
        let mut p = NbPty::new(0, BUF_CAP);
        p.setfl(true);
        assert_eq!(p.read(), Res::Err(NB_READ_EAGAIN));
        p.setfl(false);
        assert_eq!(p.read(), Res::Block);
        p.setfl(true);
        assert_eq!(p.write(BUF_CAP + 1), Res::Err(NB_WRITE_EAGAIN));
        p.setfl(false);
        p.avail = 9;
        p.free = BUF_CAP - 9;
        assert_eq!(p.read(), Res::Got(9));
        assert!(p.ledger_ok());
        assert_eq!(judge(&NbReq { nonblock: true, op: NbOp::Write(4), avail: 0, free: 4 }), Res::Got(4));
    }

    #[test]
    fn unx10853_checks_green() {
        let s = run_unx10853_checks();
        assert!(s.all_passed(), "UNX-F10853 判据未全绿");
    }
}
