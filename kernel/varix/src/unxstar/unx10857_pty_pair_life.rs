//! UNX-F10857 pty pair 生命周期矩阵（slave 关 master 面）（0x1c3a0 段）
//!
//! 锚点：UNX-F10857-J1 pty 主从生命周期矩阵——master 面随 slave 引
//! 用计数三态演进：Unborn（slave 未开）读必 EAGAIN/阻塞、写仍可缓
//! 冲交付；Live（slave 引用>0）读写正常；Gone（slave 全关）读必先
//! 排干残余（Got(avail)）后必 EIO（5，非 EOF 冒账）、写必 EIO。判定
//! 依赖场景字段演算，注入核（EOF 冒账/死写放行坏实现）必分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：生命周期场景（refs × op × avail × 旗）→ 三态判定器（排干
//! 前置）→ 主从状态机（open/close 引用账走账）→ 样本矩阵（八行正反
//! 对）→ 注入核（两坏实现必分歧）→ 关闭回放（排干→EIO 双沿对平）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1c3a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c3a0;
pub const PL_EIO: u32 = SEG_BASE + 0x01; // slave 全关后 EIO
pub const PL_DRAIN: u32 = SEG_BASE + 0x02; // 残余排干面
pub const PL_EAGAIN: u32 = SEG_BASE + 0x03; // Unborn 非阻塞读 EAGAIN
pub const PL_BLOCK_FACE: u32 = SEG_BASE + 0x04; // Unborn 阻塞读面
pub const PL_EOF_GHOST: u32 = SEG_BASE + 0x05; // EOF 冒账
pub const PL_LEDGER_LOST: u32 = SEG_BASE + 0x06; // 引用账失平

/// 语义字面锚。
pub const EAGAIN: u32 = 11;
pub const EIO: u32 = 5;
pub const BUF_CAP: u32 = 4096;

/// 诊断码语义文本。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        PL_EIO => "slave 全关后 EIO",
        PL_DRAIN => "残余排干面",
        PL_EAGAIN => "Unborn 非阻塞读 EAGAIN",
        PL_BLOCK_FACE => "Unborn 阻塞读面",
        PL_EOF_GHOST => "EOF 冒账",
        PL_LEDGER_LOST => "引用账失平",
        _ => "未知诊断码",
    }
}

/// 主从生命周期三态：Unborn（slave 未开）/ Live（引用>0）/ Gone（全关）。
#[derive(Clone, PartialEq, Debug)]
pub enum Life {
    Unborn,
    Live,
    Gone,
}

/// 主面操作：读 / 写(want)。
#[derive(Clone, PartialEq)]
pub enum MOp {
    Read,
    Write(u32),
}

/// 主面场景：生命周期 × 操作 × 残余 × 非阻塞旗（判定器四输入）。
#[derive(Clone)]
pub struct MReq {
    pub life: Life,
    pub op: MOp,
    pub avail: u32,
    pub nonblock: bool,
}

/// 判定结果：Got(交付字节数) / Block / Err(错误码)。
#[derive(Clone, PartialEq, Debug)]
pub enum Res {
    Got(u32),
    Block,
    Err(u32),
}

/// 三态判定器：Unborn 读看旗、写放行缓冲；Live 正常；Gone 读先排干
/// 残余后 EIO、写必 EIO。
pub fn judge(req: &MReq) -> Res {
    match req.life {
        Life::Unborn => match &req.op {
            MOp::Read => {
                if req.nonblock {
                    Res::Err(PL_EAGAIN)
                } else {
                    Res::Block
                }
            }
            MOp::Write(want) => Res::Got(*want),
        },
        Life::Live => match &req.op {
            MOp::Read => {
                if req.avail > 0 {
                    Res::Got(req.avail)
                } else if req.nonblock {
                    Res::Err(PL_EAGAIN)
                } else {
                    Res::Block
                }
            }
            MOp::Write(want) => Res::Got(*want),
        },
        Life::Gone => match &req.op {
            MOp::Read => {
                if req.avail > 0 {
                    Res::Got(req.avail)
                } else {
                    Res::Err(PL_EIO)
                }
            }
            MOp::Write(_) => Res::Err(PL_EIO),
        },
    }
}

/// 主从状态机：master 侧引用账与残余水位账。
pub struct MasterPty {
    pub life: Life,
    pub refs: u32,
    pub avail: u32,
    pub nonblock: bool,
    pub eio_count: u32,
    pub note: String,
}

impl MasterPty {
    pub fn new() -> MasterPty {
        MasterPty {
            life: Life::Unborn,
            refs: 0,
            avail: 0,
            nonblock: true,
            eio_count: 0,
            note: "unborn".to_string(),
        }
    }

    /// slave 打开：引用账入账，Unborn→Live。
    pub fn slave_open(&mut self) {
        self.refs += 1;
        self.life = Life::Live;
        self.note = "live".to_string();
    }

    /// slave 关闭：引用账出账，引用归零则 Live→Gone。
    pub fn slave_close(&mut self) {
        if self.refs > 0 {
            self.refs -= 1;
        }
        if self.refs == 0 {
            self.life = Life::Gone;
            self.note = "gone".to_string();
        }
    }

    /// 写 master：Live/Unborn 走缓冲水位账，Gone 落 EIO 账。
    pub fn write(&mut self, want: u32) -> Res {
        let req = MReq { life: self.life.clone(), op: MOp::Write(want), avail: self.avail, nonblock: self.nonblock };
        match judge(&req) {
            Res::Got(n) => {
                self.avail += n;
                Res::Got(n)
            }
            Res::Err(c) => {
                self.eio_count += 1;
                Res::Err(c)
            }
            other => other,
        }
    }

    /// 读 master：Live 排水位，Gone 先排残余后 EIO。
    pub fn read(&mut self) -> Res {
        let req = MReq { life: self.life.clone(), op: MOp::Read, avail: self.avail, nonblock: self.nonblock };
        match judge(&req) {
            Res::Got(n) => {
                self.avail -= n;
                if self.life == Life::Gone {
                    self.note = "drain".to_string();
                }
                Res::Got(n)
            }
            Res::Err(c) => {
                if c == PL_EIO {
                    self.eio_count += 1;
                }
                Res::Err(c)
            }
            other => other,
        }
    }

    /// 引用账一致：Gone 必 refs==0，Live 必 refs>0。
    pub fn ledger_ok(&self) -> bool {
        match self.life {
            Life::Gone => self.refs == 0,
            Life::Live => self.refs > 0,
            Life::Unborn => self.refs == 0,
        }
    }
}

/// 样本矩阵容量。
pub const MATRIX_CAP: usize = 24;

/// 矩阵逐格判定：期望（Got(n) 编码 n+1，Block 0，Err(c) 直码）命中计数。
pub fn verify_matrix(reqs: &[MReq], wants: &[u32]) -> (usize, usize, bool) {
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

/// 样本矩阵：三态 × 读/写 × 有/无残余，8 行正反对。
pub fn sample_matrix() -> (Vec<MReq>, Vec<u32>) {
    let mut reqs: Vec<MReq> = Vec::new();
    let mut wants: Vec<u32> = Vec::new();
    // R0 Unborn 旗位读 → EAGAIN
    reqs.push(MReq { life: Life::Unborn, op: MOp::Read, avail: 0, nonblock: true });
    wants.push(PL_EAGAIN);
    // R1 Unborn 旗清读 → Block
    reqs.push(MReq { life: Life::Unborn, op: MOp::Read, avail: 0, nonblock: false });
    wants.push(0);
    // R2 Unborn 写 → 缓冲放行
    reqs.push(MReq { life: Life::Unborn, op: MOp::Write(8), avail: 0, nonblock: true });
    wants.push(9);
    // R3 Live 有残余读 → Got
    reqs.push(MReq { life: Life::Live, op: MOp::Read, avail: 7, nonblock: true });
    wants.push(8);
    // R4 Gone 无残余读 → EIO
    reqs.push(MReq { life: Life::Gone, op: MOp::Read, avail: 0, nonblock: true });
    wants.push(PL_EIO);
    // R5 Gone 有残余读 → 排干面
    reqs.push(MReq { life: Life::Gone, op: MOp::Read, avail: 12, nonblock: true });
    wants.push(13);
    // R6 Gone 写 → EIO
    reqs.push(MReq { life: Life::Gone, op: MOp::Write(4), avail: 0, nonblock: true });
    wants.push(PL_EIO);
    // R7 Live 写 → 放行
    reqs.push(MReq { life: Life::Live, op: MOp::Write(4), avail: 0, nonblock: true });
    wants.push(5);
    (reqs, wants)
}

/// 注入 A：坏实现 EOF 冒账（Gone 读误报 Got(0)）。
pub fn faulty_eof_ghost(req: &MReq) -> Res {
    if req.life == Life::Gone && req.op == MOp::Read && req.avail == 0 {
        Res::Got(0)
    } else {
        judge(req)
    }
}

/// 注入 B：坏实现死写放行（Gone 写误交付）。
pub fn faulty_dead_write(req: &MReq) -> Res {
    if req.life == Life::Gone {
        if let MOp::Write(w) = &req.op {
            return Res::Got(*w);
        }
    }
    judge(req)
}

/// 注入核：两处坏账与判定器必分歧。
pub fn inject_two() -> [bool; 2] {
    let rd = MReq { life: Life::Gone, op: MOp::Read, avail: 0, nonblock: true };
    let wr = MReq { life: Life::Gone, op: MOp::Write(8), avail: 0, nonblock: true };
    let d1 = judge(&rd) != faulty_eof_ghost(&rd);
    let d2 = judge(&wr) != faulty_dead_write(&wr);
    [d1, d2]
}

/// 关闭回放：写 9 字节 → slave 全关 → 先排干 9 → 再读 EIO。
pub fn close_replay_ok() -> bool {
    let mut m = MasterPty::new();
    let w = m.write(9);
    m.slave_open();
    m.slave_close();
    let d = m.read();
    let e = m.read();
    w == Res::Got(9) && d == Res::Got(9) && e == Res::Err(PL_EIO)
        && m.avail == 0 && m.eio_count == 1 && m.ledger_ok()
}

/// 引用回放：双引用关一次仍 Live、再关转 Gone。
pub fn refs_replay_ok() -> bool {
    let mut m = MasterPty::new();
    m.slave_open();
    m.slave_open();
    m.slave_close();
    let mid = m.life == Life::Live && m.refs == 1;
    m.slave_close();
    mid && m.life == Life::Gone && m.refs == 0 && m.ledger_ok()
}

/// 生命周期注记：判定渲染。
pub fn pl_note(r: &Res) -> String {
    match r {
        Res::Got(n) => format!("got={}", n),
        Res::Block => diag_text(PL_BLOCK_FACE).to_string(),
        Res::Err(c) => format!("{} ({})", diag_text(*c), c),
    }
}

/// 域判据（对应卡验收 J 判据）。
pub fn run_unx10857_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10857");
    let (reqs, wants) = sample_matrix();
    let (hits, total, cap_ok) = verify_matrix(&reqs, &wants);

    // J1 正路径：Gone 读 EIO / 写 EIO
    let j1 = judge(&MReq { life: Life::Gone, op: MOp::Read, avail: 0, nonblock: true })
        == Res::Err(PL_EIO)
        && judge(&MReq { life: Life::Gone, op: MOp::Write(4), avail: 0, nonblock: true })
            == Res::Err(PL_EIO);
    s.add("J1-死面对", j1, "EIO 面 5");

    // J2 排干前置：Gone 有残余读必 Got
    let j2 = judge(&MReq { life: Life::Gone, op: MOp::Read, avail: 12, nonblock: true })
        == Res::Got(12);
    s.add("J2-排干前置", j2, "残余先交付后 EIO");

    // J3 边界：Unborn 读双层（旗位 EAGAIN / 旗清 Block）
    let j3 = judge(&MReq { life: Life::Unborn, op: MOp::Read, avail: 0, nonblock: true })
        == Res::Err(PL_EAGAIN)
        && judge(&MReq { life: Life::Unborn, op: MOp::Read, avail: 0, nonblock: false })
            == Res::Block;
    s.add("J3-Unborn 双层", j3, "旗面可辨");

    // J4 旗不压交付：Live 有残余读旗位/旗清同量
    let j4 = judge(&MReq { life: Life::Live, op: MOp::Read, avail: 7, nonblock: true })
        == Res::Got(7)
        && judge(&MReq { life: Life::Live, op: MOp::Read, avail: 7, nonblock: false })
            == Res::Got(7);
    s.add("J4-旗不压水", j4, "交付量与旗无关");

    // J5 关闭回放（排干→EIO 双沿）
    s.add("J5-关闭回放", close_replay_ok(), "排干 9 后 EIO 落账");

    // J6 引用回放（双关转 Gone）
    s.add("J6-引用回放", refs_replay_ok(), "refs 账三态演进");

    // J7 样本矩阵全命中
    s.add("J7-矩阵对平", cap_ok, "8 行正反对全命中");

    // J8 注入1 EOF 冒账检出
    let inj = inject_two();
    s.add("J8-EOF 冒账检出", inj[0], "Gone 读误 0 必分歧");

    // J9 注入2 死写放行检出
    s.add("J9-死写放行检出", inj[1], "Gone 写误交付必分歧");

    // J10 水位账一致
    let mut m = MasterPty::new();
    m.slave_open();
    let _ = m.write(30);
    let _ = m.read();
    s.add("J10-水位账", m.ledger_ok() && m.avail == 0 && m.eio_count == 0, "Live 读写回平");

    // J11 字面锚：EAGAIN=11 / EIO=5 / BUF_CAP=4096
    let j11 = EAGAIN == 11 && EIO == 5 && BUF_CAP == 4096;
    s.add("J11-字面锚", j11, "错误码与容量常量");

    // J12 诊断码闭集
    let j12 = diag_text(PL_EIO) == "slave 全关后 EIO"
        && diag_text(PL_DRAIN) == "残余排干面"
        && diag_text(9) == "未知诊断码";
    s.add("J12-码文本闭集", j12, "六码文本可辨");

    // J13 (dyn) 矩阵明细落账
    s.add_dyn("J13-矩阵明细", hits == total, &format!("hits={}/{} cap_ok={}", hits, total, cap_ok));

    // J14 (dyn) 生命周期注记落账
    let note = pl_note(&judge(&MReq { life: Life::Gone, op: MOp::Read, avail: 0, nonblock: true }));
    s.add_dyn("J14-生命周期注记", note.contains("EIO"), &note);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10857_flow() {
        assert_eq!(judge(&MReq { life: Life::Gone, op: MOp::Read, avail: 0, nonblock: true }), Res::Err(PL_EIO));
        assert_eq!(judge(&MReq { life: Life::Gone, op: MOp::Read, avail: 12, nonblock: true }), Res::Got(12));
        assert_eq!(judge(&MReq { life: Life::Gone, op: MOp::Write(4), avail: 0, nonblock: true }), Res::Err(PL_EIO));
        assert_eq!(judge(&MReq { life: Life::Unborn, op: MOp::Read, avail: 0, nonblock: false }), Res::Block);
        let mut m = MasterPty::new();
        m.slave_open();
        m.slave_open();
        m.slave_close();
        assert_eq!(m.life, Life::Live);
        m.slave_close();
        assert_eq!(m.life, Life::Gone);
        assert!(m.ledger_ok());
    }

    #[test]
    fn unx10857_checks_green() {
        let s = run_unx10857_checks();
        assert!(s.all_passed(), "UNX-F10857 判据未全绿");
    }
}
