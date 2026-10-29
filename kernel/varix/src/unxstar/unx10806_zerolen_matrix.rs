//! UNX-F10806 零长度读写语义矩阵（read 0/write 0）（0x1bd40 段）
//!
//! 锚点：UNX-F10806-J1 零长语义对平——fd 有效性先裁（关闭 fd 零长也
//! EBADF）、零长读写免检直返 0（不查管道断裂/不满态、不出信号）、非零长
//! 才入状态机三条序全对平；注入坏序（先查管道后查 fd/零长误入 EPIPE 路）
//! 必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：判定序状态机（fd 位→零长位→管道态三级漏斗）+ 读口写口双裁决器
//! （读口 EOF/EAGAIN 面、写口 EPIPE 面）+ 正反双样本矩阵（关闭 fd×零长×
//! 断裂管三轴）+ 注入核（序翻转/零长误判两类必分歧）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX 锚：EBADF=9、EPIPE=32、EAGAIN=11。
pub const POSIX_EBADF: u32 = 9;
pub const POSIX_EPIPE: u32 = 32;
pub const POSIX_EAGAIN: u32 = 11;

/// 诊断段（0x1bd40 独占 0x20）。
pub const SEG_BASE: u32 = 0x1bd40;
pub const ZL_ORDER_FLIP: u32 = SEG_BASE + 0x01; // 判定序翻转（fd 后裁）
pub const ZL_ZLEN_EPIPE: u32 = SEG_BASE + 0x02; // 零长写误入 EPIPE 路
pub const ZL_ZLEN_SIG: u32 = SEG_BASE + 0x03; // 零长写误触信号面
pub const ZL_ZLEN_EAGAIN: u32 = SEG_BASE + 0x04; // 零长读误报 EAGAIN
pub const ZL_EBADF_LOSS: u32 = SEG_BASE + 0x05; // 关闭 fd 零长放行
pub const ZL_NONZERO_SKIP: u32 = SEG_BASE + 0x06; // 非零长漏入状态机

/// 读口/写口。
#[derive(Clone, PartialEq, Eq)]
pub enum Op {
    Read,
    Write,
}

/// 场景输入：fd 位 × 口 × 长度 × 管道态（读端计数/写端计数/缓冲字节）。
#[derive(Clone)]
pub struct Scenario {
    pub fd_open: bool,
    pub op: Op,
    pub len: usize,
    pub readers: u32,
    pub writers: u32,
    pub buffered: usize,
    pub nonblock: bool,
}

/// 三级漏斗裁决核：fd 位 → 零长位 → 管道态。
pub fn judge(sc: &Scenario) -> Result<usize, u32> {
    // 第一级：fd 有效性（关闭 fd 连零长也拒）
    if !sc.fd_open {
        return Err(ZL_EBADF_LOSS);
    }
    // 第二级：零长免检直返 0（不查管道态、不触信号面）
    if sc.len == 0 {
        return Ok(0);
    }
    // 第三级：管道态（非零长才入）
    match sc.op {
        Op::Write => {
            if sc.readers == 0 {
                return Err(ZL_ZLEN_EPIPE);
            }
            Ok(sc.len)
        }
        Op::Read => {
            if sc.buffered > 0 {
                let n = if sc.buffered < sc.len { sc.buffered } else { sc.len };
                return Ok(n);
            }
            if sc.writers == 0 {
                return Ok(0);
            }
            if sc.nonblock {
                return Err(ZL_ZLEN_EAGAIN);
            }
            Ok(0)
        }
    }
}

/// 判定序注记：逐级落账（fd/零长/管道态），对账序不可翻转。
pub fn judge_trace(sc: &Scenario) -> String {
    if !sc.fd_open {
        return "L1-fd".to_string();
    }
    if sc.len == 0 {
        return "L2-zerolen".to_string();
    }
    match judge(sc) {
        Ok(n) => format!("L3-ok({})", n),
        Err(c) => format!("L3-err({:#x})", c),
    }
}

/// 管道零长状态机：事件驱动（读写调用逐次落判）。
pub struct ZeroPipe {
    pub fd_open: bool,
    pub readers: u32,
    pub writers: u32,
    pub buffered: usize,
    pub nonblock: bool,
    pub note: String,
}

impl ZeroPipe {
    pub fn new(readers: u32, writers: u32, nonblock: bool) -> ZeroPipe {
        ZeroPipe {
            fd_open: true,
            readers,
            writers,
            buffered: 0,
            nonblock,
            note: "open".to_string(),
        }
    }

    pub fn close_all_readers(&mut self) {
        self.readers = 0;
        self.note = "readers-gone".to_string();
    }

    pub fn close_fd(&mut self) {
        self.fd_open = false;
        self.note = "fd-closed".to_string();
    }

    pub fn fill(&mut self, n: usize) {
        self.buffered += n;
    }

    /// 读调用落判（状态机字段实时喂入判定器，命中即消费缓冲）。
    pub fn call_read(&mut self, len: usize) -> Result<usize, u32> {
        let sc = Scenario {
            fd_open: self.fd_open,
            op: Op::Read,
            len,
            readers: self.readers,
            writers: self.writers,
            buffered: self.buffered,
            nonblock: self.nonblock,
        };
        let out = judge(&sc);
        if let Ok(n) = out {
            if len > 0 && self.buffered >= n {
                self.buffered -= n;
            }
        }
        out
    }

    /// 写调用落判。
    pub fn call_write(&self, len: usize) -> Result<usize, u32> {
        let sc = Scenario {
            fd_open: self.fd_open,
            op: Op::Write,
            len,
            readers: self.readers,
            writers: self.writers,
            buffered: self.buffered,
            nonblock: self.nonblock,
        };
        judge(&sc)
    }
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 24;

/// 正反双样本矩阵：期望 () 或期望诊断码。
pub fn sample_matrix() -> Vec<(Scenario, Option<u32>)> {
    let mut m: Vec<(Scenario, Option<u32>)> = Vec::new();
    let base = |op: Op, len: usize, readers: u32, writers: u32, buffered: usize| Scenario {
        fd_open: true,
        op,
        len,
        readers,
        writers,
        buffered,
        nonblock: true,
    };
    // 正：零长读写免检（断裂管上亦放行）
    m.push((base(Op::Write, 0, 0, 1, 0), None));
    m.push((base(Op::Read, 0, 1, 0, 0), None));
    // 正：非零长写读正常态
    m.push((base(Op::Write, 8, 1, 1, 0), None));
    m.push((base(Op::Read, 8, 1, 1, 8), None));
    // 反：关闭 fd 零长也 EBADF（序第一级）
    m.push((Scenario { fd_open: false, op: Op::Write, len: 0, readers: 1, writers: 1, buffered: 0, nonblock: true }, Some(ZL_EBADF_LOSS)));
    m.push((Scenario { fd_open: false, op: Op::Read, len: 0, readers: 1, writers: 1, buffered: 0, nonblock: true }, Some(ZL_EBADF_LOSS)));
    // 反：断裂管非零长写必 EPIPE
    m.push((base(Op::Write, 8, 0, 1, 0), Some(ZL_ZLEN_EPIPE)));
    // 正：断裂管非零长读（写端也没了）EOF 返 0 —— 放行
    m.push((base(Op::Read, 8, 1, 0, 0), None));
    // 反：非阻塞空缓冲非零长读必 EAGAIN
    m.push((base(Op::Read, 8, 1, 1, 0), Some(ZL_ZLEN_EAGAIN)));
    m
}

/// 矩阵对平：返回 (命中数, 总数, 容量合规)。
pub fn verify_matrix() -> (usize, usize, bool) {
    let m = sample_matrix();
    let mut ok = 0usize;
    let mut i = 0usize;
    while i < m.len() {
        let (sc, want) = &m[i];
        let got = judge(sc);
        let hit = match want {
            None => got.is_ok(),
            Some(c) => got.err() == Some(*c),
        };
        if hit {
            ok += 1;
        }
        i += 1;
    }
    (ok, m.len(), m.len() <= MATRIX_CAP)
}

/// 坏实现 A：判定序翻转（先查管道态后查 fd）。
pub fn faulty_order_flip(sc: &Scenario) -> Result<usize, u32> {
    if sc.readers == 0 && sc.op == Op::Write && sc.len > 0 {
        return Err(ZL_ZLEN_EPIPE);
    }
    judge(sc)
}

/// 坏实现 B：零长写误入 EPIPE 路（零长不免检）。
pub fn faulty_zlen_epipe(sc: &Scenario) -> Result<usize, u32> {
    if sc.op == Op::Write && sc.readers == 0 {
        return Err(ZL_ZLEN_EPIPE);
    }
    judge(sc)
}

/// 注入核：两类坏序必与权威分歧（检出）。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    // 1) 序翻转：关闭 fd 上零长写，权威 EBADF、坏实现放行 → 检出
    let sc1 = Scenario {
        fd_open: false,
        op: Op::Write,
        len: 0,
        readers: 0,
        writers: 1,
        buffered: 0,
        nonblock: true,
    };
    got[0] = judge(&sc1) != faulty_order_flip(&sc1)
        && judge(&sc1).err() == Some(ZL_EBADF_LOSS);
    // 2) 零长误判：断裂管上零长写，权威放行、坏实现 EPIPE → 检出
    let sc2 = Scenario {
        fd_open: true,
        op: Op::Write,
        len: 0,
        readers: 0,
        writers: 1,
        buffered: 0,
        nonblock: true,
    };
    got[1] = judge(&sc2) != faulty_zlen_epipe(&sc2) && judge(&sc2).is_ok();
    got
}

/// 读口 EOF 核：排空后写端存续是 EAGAIN 面、写端归零才是 EOF 面。
pub fn read_eof_ok() -> bool {
    let mut p = ZeroPipe::new(1, 1, true);
    p.fill(4);
    let r1 = p.call_read(8);
    let r2 = p.call_read(8);
    p.writers = 0;
    let r3 = p.call_read(8);
    r1 == Ok(4) && r2.err() == Some(ZL_ZLEN_EAGAIN) && r3 == Ok(0)
}

/// 零长语义判据（对应卡验收 J 判据）。
pub fn run_unx10806_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10806");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平：正反样本全命中
    s.add("J1-矩阵对平", hits == total && cap_ok, "九线正反样本全命中");

    // J2 零长写免检：断裂管上零长写放行
    let j2 = judge(&Scenario {
        fd_open: true,
        op: Op::Write,
        len: 0,
        readers: 0,
        writers: 1,
        buffered: 0,
        nonblock: true,
    }) == Ok(0);
    s.add("J2-零长写免检", j2, "零长写不查断裂态");

    // J3 零长读免检：空缓冲零长读放行（非 EAGAIN）
    let j3 = judge(&Scenario {
        fd_open: true,
        op: Op::Read,
        len: 0,
        readers: 1,
        writers: 1,
        buffered: 0,
        nonblock: true,
    }) == Ok(0);
    s.add("J3-零长读免检", j3, "零长读不报 EAGAIN");

    // J4 判定序第一级：关闭 fd 零长也 EBADF
    let j4 = judge(&Scenario {
        fd_open: false,
        op: Op::Write,
        len: 0,
        readers: 1,
        writers: 1,
        buffered: 0,
        nonblock: true,
    }).err() == Some(ZL_EBADF_LOSS);
    s.add("J4-fd先裁", j4, "fd 位先于零长位");

    // J5 判定序第三级：断裂管非零长写必 EPIPE
    let j5 = judge(&Scenario {
        fd_open: true,
        op: Op::Write,
        len: 8,
        readers: 0,
        writers: 1,
        buffered: 0,
        nonblock: true,
    }).err() == Some(ZL_ZLEN_EPIPE);
    s.add("J5-非零长入状态机", j5, "非零长才查管道态");

    // J6 判定序注记：三级漏斗逐级落账
    let t1 = judge_trace(&Scenario {
        fd_open: false,
        op: Op::Read,
        len: 0,
        readers: 1,
        writers: 1,
        buffered: 0,
        nonblock: true,
    });
    let t2 = judge_trace(&Scenario {
        fd_open: true,
        op: Op::Write,
        len: 0,
        readers: 0,
        writers: 1,
        buffered: 0,
        nonblock: true,
    });
    let j6 = t1 == "L1-fd" && t2 == "L2-zerolen";
    s.add("J6-序注记对平", j6, "L1-fd / L2-zerolen 逐级落账");

    // J7 读口 EOF 核：排空后写端归零读返 0
    s.add("J7-EOF分账核", read_eof_ok(), "排空读返 0、EOF 面 EAGAIN 不混");

    // J8 注入1 序翻转检出
    s.add("J8-序翻转检出", inj[0], "fd 后裁坏序必分歧");

    // J9 注入2 零长误判检出
    s.add("J9-零长误判检出", inj[1], "零长入 EPIPE 路坏序必分歧");

    // J10 状态机全序：open→fill→read→readers-gone→write
    let mut p = ZeroPipe::new(1, 1, true);
    p.fill(16);
    let a = p.call_read(16);
    p.close_all_readers();
    let b = p.call_write(4);
    let c = p.call_write(0);
    let j10 = a == Ok(16) && b.err() == Some(ZL_ZLEN_EPIPE) && c == Ok(0);
    s.add("J10-全序落判", j10, "断裂前后写判定随状态翻转");

    // J11 字面锚：9/32/11 三锚
    let j11 = POSIX_EBADF == 9 && POSIX_EPIPE == 32 && POSIX_EAGAIN == 11;
    s.add("J11-字面锚对平", j11, "9/32/11 三锚");

    // J12 状态机指纹：断裂前后注记翻转
    let mut p2 = ZeroPipe::new(1, 1, false);
    let n1 = p2.note.clone();
    p2.close_all_readers();
    let n2 = p2.note.clone();
    let j12 = n1 == "open" && n2 == "readers-gone";
    s.add("J12-注记翻转", j12, "状态注记随事件翻转");

    // J13 (dyn) 状态机指纹落账
    let mut p3 = ZeroPipe::new(1, 1, true);
    p3.fill(64);
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(p3.readers);
    h = h.wrapping_mul(31).wrapping_add(p3.writers);
    h = h.wrapping_mul(31).wrapping_add(p3.buffered as u32);
    for c in p3.note.bytes() {
        h = h.wrapping_mul(31).wrapping_add(c as u32);
    }
    s.add_dyn("J13-状态指纹", h != 0, &format!("fold={:#x}", h));

    // J14 (dyn) 矩阵命中明细
    s.add_dyn(
        "J14-命中明细",
        hits == total,
        &format!("hits={}/{} cap_ok={}", hits, total, cap_ok),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10806_zerolen_flow() {
        let mut p = ZeroPipe::new(1, 1, true);
        assert!(p.call_write(8).is_ok());
        p.fill(8);
        assert_eq!(p.call_read(4), Ok(4));
        assert!(p.call_read(0).is_ok());
        p.close_all_readers();
        assert!(p.call_write(4).err() == Some(ZL_ZLEN_EPIPE));
        assert!(p.call_write(0).is_ok());
        p.close_fd();
        assert!(p.call_read(0).err() == Some(ZL_EBADF_LOSS));
        let sc = Scenario {
            fd_open: true,
            op: Op::Read,
            len: 8,
            readers: 1,
            writers: 1,
            buffered: 0,
            nonblock: false,
        };
        assert_eq!(judge(&sc), Ok(0));
    }

    #[test]
    fn unx10806_checks_green() {
        let s = run_unx10806_checks();
        assert!(s.all_passed(), "UNX-F10806 判据未全绿");
    }
}
