//! UNX-F10803 EAGAIN 正反双判据矩阵（O_NONBLOCK）（0x1bce0 段）
//!
//! 锚点：UNX-F10803-J1 EAGAIN 双判据对平——非阻塞且缓冲不足必 EAGAIN
//! 且零消费、非阻塞有空间必整写放行、阻塞满缓冲必挂起不报 EAGAIN 三线
//! 对平；注入坏实现（EAGAIN 却部分消费/阻塞误报 EAGAIN/非阻塞假阻塞）
//! 必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：管道缓冲状态机（cap/used/非阻塞位，写推进、读排水推进）+
//! 写裁决器（空间×非阻塞位算 Done/Again/WouldBlock）+ 消费对账核（
//! 返回值=实进字节、EAGAIN 零消费）+ 注入核（三类坏实现必分歧）+
//! 正反双样本矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX 锚：EAGAIN errno 11。
pub const POSIX_EAGAIN: u32 = 11;

/// 诊断段（0x1bce0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1bce0;
pub const AG_PARTIAL_CONSUME: u32 = SEG_BASE + 0x01; // EAGAIN 却部分消费
pub const AG_BLOCK_MISREPORT: u32 = SEG_BASE + 0x02; // 阻塞态误报 EAGAIN
pub const AG_NONBLOCK_HANG: u32 = SEG_BASE + 0x03; // 非阻塞态假阻塞
pub const AG_ZLEN_MISROUTE: u32 = SEG_BASE + 0x04; // 零长写误报 EAGAIN
pub const AG_DRAIN_LOSS: u32 = SEG_BASE + 0x05; // 排水字节丢失
pub const AG_USED_OVER_CAP: u32 = SEG_BASE + 0x06; // used 越过 cap

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        AG_PARTIAL_CONSUME => "EAGAIN 却部分消费",
        AG_BLOCK_MISREPORT => "阻塞态误报 EAGAIN",
        AG_NONBLOCK_HANG => "非阻塞态假阻塞",
        AG_ZLEN_MISROUTE => "零长写误报 EAGAIN",
        AG_DRAIN_LOSS => "排水字节丢失",
        AG_USED_OVER_CAP => "used 越过 cap",
        _ => "未知诊断码",
    }
}

/// 写裁决输出。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum WriteOut {
    /// 整写成功，字节数。
    Done(usize),
    /// 非阻塞空间不足：EAGAIN，零消费。
    Again,
    /// 阻塞态空间不足：挂起等待。
    WouldBlock,
}

/// 场景输入：缓冲占用 × 写长 × 非阻塞位。
#[derive(Clone)]
pub struct Scenario {
    pub used: usize,
    pub cap: usize,
    pub nonblock: bool,
    pub write_len: usize,
}

/// 写裁决核：整写/EAGAIN/挂起三分，EAGAIN 必零消费。
pub fn judge(sc: &Scenario) -> (WriteOut, usize) {
    if sc.write_len == 0 {
        return (WriteOut::Done(0), 0);
    }
    if sc.used + sc.write_len <= sc.cap {
        return (WriteOut::Done(sc.write_len), sc.write_len);
    }
    if sc.nonblock {
        (WriteOut::Again, 0)
    } else {
        (WriteOut::WouldBlock, 0)
    }
}

/// 管道缓冲状态机：写推进、读排水、消费对账。
pub struct PipeBuf {
    pub cap: usize,
    pub used: usize,
    pub nonblock: bool,
    pub note: String,
}

impl PipeBuf {
    pub fn new(cap: usize, nonblock: bool) -> PipeBuf {
        PipeBuf {
            cap,
            used: 0,
            nonblock,
            note: if nonblock { "o_nonblock".to_string() } else { "blocking".to_string() },
        }
    }

    pub fn free(&self) -> usize {
        self.cap - self.used
    }

    /// 写入：按裁决推进 used（EAGAIN/WouldBlock 零消费）。
    pub fn write(&mut self, len: usize) -> WriteOut {
        let sc = Scenario { used: self.used, cap: self.cap, nonblock: self.nonblock, write_len: len };
        let (out, consumed) = judge(&sc);
        self.used += consumed;
        if self.used > self.cap {
            self.used = self.cap;
        }
        out
    }

    /// 读排水：至多 drain 字节，返回实排字节数。
    pub fn drain(&mut self, want: usize) -> usize {
        let got = if want > self.used { self.used } else { want };
        self.used -= got;
        got
    }
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 16;

/// 正反双样本矩阵：期望 (WriteOut, 消费字节)。
pub fn sample_matrix() -> Vec<(Scenario, (WriteOut, usize))> {
    let mut m: Vec<(Scenario, (WriteOut, usize))> = Vec::new();
    // 正：非阻塞有空间整写放行
    m.push((
        Scenario { used: 0, cap: 4096, nonblock: true, write_len: 512 },
        (WriteOut::Done(512), 512),
    ));
    m.push((
        Scenario { used: 3584, cap: 4096, nonblock: true, write_len: 512 },
        (WriteOut::Done(512), 512),
    ));
    // 边界：恰好填满
    m.push((
        Scenario { used: 4096 - 7, cap: 4096, nonblock: true, write_len: 7 },
        (WriteOut::Done(7), 7),
    ));
    // 正：零长写免检
    m.push((
        Scenario { used: 4096, cap: 4096, nonblock: true, write_len: 0 },
        (WriteOut::Done(0), 0),
    ));
    // 反：非阻塞空间不足必 EAGAIN 零消费（1 字节边界 / 超大块）
    m.push((
        Scenario { used: 4096, cap: 4096, nonblock: true, write_len: 1 },
        (WriteOut::Again, 0),
    ));
    m.push((
        Scenario { used: 4000, cap: 4096, nonblock: true, write_len: 512 },
        (WriteOut::Again, 0),
    ));
    // 正：阻塞态空间不足挂起（不是 EAGAIN）
    m.push((
        Scenario { used: 4096, cap: 4096, nonblock: false, write_len: 64 },
        (WriteOut::WouldBlock, 0),
    ));
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
        if got != *want {
            i += 1;
            continue;
        }
        ok += 1;
        i += 1;
    }
    (ok, m.len(), m.len() <= MATRIX_CAP)
}

/// 坏实现 A：EAGAIN 却部分消费。
pub fn faulty_partial(sc: &Scenario) -> (WriteOut, usize) {
    let base = judge(sc);
    match base.0 {
        WriteOut::Again => (WriteOut::Again, 1),
        _ => base,
    }
}

/// 坏实现 B：阻塞态误报 EAGAIN。
pub fn faulty_block_eagain(sc: &Scenario) -> (WriteOut, usize) {
    let base = judge(sc);
    match base.0 {
        WriteOut::WouldBlock => (WriteOut::Again, 0),
        _ => base,
    }
}

/// 坏实现 C：非阻塞态假阻塞（满缓冲仍挂起）。
pub fn faulty_nonblock_hang(sc: &Scenario) -> (WriteOut, usize) {
    let base = judge(sc);
    match base.0 {
        WriteOut::Again => (WriteOut::WouldBlock, 0),
        _ => base,
    }
}

/// 消费对账核：满缓冲非阻塞写后 used 不变，排水后整写可进。
pub fn consume_reconcile_ok() -> bool {
    let mut b = PipeBuf::new(64, true);
    let _ = b.write(64);
    let full_before = b.used;
    let w1 = b.write(16);
    let used_after = b.used;
    let drained = b.drain(64);
    let w2 = b.write(16);
    full_before == 64 && w1 == WriteOut::Again && used_after == 64 && drained == 64
        && w2 == WriteOut::Done(16) && b.used == 16
}

/// 混流对账核：阻塞/非阻塞两管交替写读，逐笔对平 used 账。
pub fn mixed_flow_ok() -> bool {
    let mut nb = PipeBuf::new(64, true);
    let mut blk = PipeBuf::new(64, false);
    // 混流序：nb 写满 → blk 写 40 → nb 再写必 EAGAIN → blk 再写挂起
    let s1 = nb.write(64);
    let s2 = blk.write(40);
    let s3 = nb.write(8);
    let s4 = blk.write(40);
    // 排水：nb 排 16 → 再写 16 可进；blk 排 8 → 再写 8 可进（裁决放行后记账）
    let d1 = nb.drain(16);
    let s5 = nb.write(16);
    let d2 = blk.drain(8);
    let s6 = blk.write(8);
    let s1_ok = s1 == WriteOut::Done(64) && nb.used == 64;
    let s2_ok = s2 == WriteOut::Done(40) && blk.used == 40;
    let s3_ok = s3 == WriteOut::Again && nb.used == 64;
    let s4_ok = s4 == WriteOut::WouldBlock && blk.used == 40;
    let s5_ok = d1 == 16 && s5 == WriteOut::Done(16) && nb.used == 64;
    let s6_ok = d2 == 8 && s6 == WriteOut::Done(8) && blk.used == 40;
    let total = nb.used + blk.used;
    s1_ok && s2_ok && s3_ok && s4_ok && s5_ok && s6_ok && total == 64 + 48
}

/// 场景语义注记（对账/落账用）。
pub fn scenario_note(sc: &Scenario) -> String {
    let mode = if sc.nonblock { "nonblock" } else { "blocking" };
    let (out, used) = judge(sc);
    let verdict = match out {
        WriteOut::Done(n) => format!("done({})", n),
        WriteOut::Again => "again".to_string(),
        WriteOut::WouldBlock => "wouldblock".to_string(),
    };
    format!("{} len={} cap={} -> {} consumed={}", mode, sc.write_len, sc.cap, verdict, used)
}

/// 注入核：三类坏实现必与权威分歧（检出）。
pub fn inject_three() -> [bool; 3] {
    let mut got = [false; 3];
    let full = Scenario { used: 4096, cap: 4096, nonblock: true, write_len: 8 };
    // 1) EAGAIN 部分消费坏实现 → 权威零消费 → 分歧检出
    got[0] = judge(&full) != faulty_partial(&full) && judge(&full) == (WriteOut::Again, 0);
    let blk = Scenario { used: 4096, cap: 4096, nonblock: false, write_len: 8 };
    // 2) 阻塞误报 EAGAIN 坏实现 → 权威 WouldBlock → 分歧检出
    got[1] = judge(&blk) != faulty_block_eagain(&blk) && judge(&blk).0 == WriteOut::WouldBlock;
    // 3) 非阻塞假阻塞坏实现 → 权威 Again → 分歧检出
    got[2] = judge(&full) != faulty_nonblock_hang(&full) && judge(&full).0 == WriteOut::Again;
    got
}

/// EAGAIN 矩阵判据（对应卡验收 J 判据）。
pub fn run_unx10803_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10803");
    let inj = inject_three();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平：正反样本全命中
    s.add("J1-矩阵对平", hits == total && cap_ok, "七线正反样本全命中");

    // J2 非阻塞有空间整写放行
    let j2 = judge(&Scenario { used: 0, cap: 4096, nonblock: true, write_len: 512 })
        == (WriteOut::Done(512), 512);
    s.add("J2-非阻塞整写", j2, "空间充足 Done(512) 消费 512");

    // J3 非阻塞满缓冲必 EAGAIN 且零消费
    let j3 = judge(&Scenario { used: 4096, cap: 4096, nonblock: true, write_len: 8 })
        == (WriteOut::Again, 0);
    s.add("J3-满缓冲EAGAIN", j3, "EAGAIN 零消费");

    // J4 阻塞态满缓冲挂起不报 EAGAIN
    let j4 = judge(&Scenario { used: 4096, cap: 4096, nonblock: false, write_len: 8 }).0
        == WriteOut::WouldBlock;
    s.add("J4-阻塞态挂起", j4, "阻塞态 WouldBlock");

    // J5 边界：恰好填满放行、超 1 字节必 EAGAIN
    let j5 = judge(&Scenario { used: 4095, cap: 4096, nonblock: true, write_len: 1 })
        == (WriteOut::Done(1), 1)
        && judge(&Scenario { used: 4096, cap: 4096, nonblock: true, write_len: 1 })
            == (WriteOut::Again, 0);
    s.add("J5-满界两侧", j5, "恰好填满/超一字节分界清晰");

    // J6 零长写免检：满缓冲上零长写 Done(0)
    let j6 = judge(&Scenario { used: 4096, cap: 4096, nonblock: true, write_len: 0 })
        == (WriteOut::Done(0), 0);
    s.add("J6-零长免检", j6, "零长写不查空间");

    // J7 消费对账：EAGAIN 零消费、排水后整写可进，且混流逐笔对平
    s.add("J7-消费对账核", consume_reconcile_ok() && mixed_flow_ok(), "used 账与返回值一致");

    // J8 注入1 EAGAIN 部分消费检出
    s.add("J8-部分消费检出", inj[0], "EAGAIN 消费 1 字节坏实现必分歧");

    // J9 注入2 阻塞误报 EAGAIN 检出
    s.add("J9-阻塞误报检出", inj[1], "阻塞态报 EAGAIN 必分歧");

    // J10 注入3 非阻塞假阻塞检出
    s.add("J10-假阻塞检出", inj[2], "非阻塞挂起坏实现必分歧");

    // J11 字面锚：EAGAIN errno=11 且诊断表完备
    let j11 = POSIX_EAGAIN == 11
        && diag_text(AG_PARTIAL_CONSUME).len() > 0
        && diag_text(0xffff_ffff) == "未知诊断码";
    s.add("J11-字面锚对平", j11, "EAGAIN 字面 11 且诊断表完备");

    // J12 used 钳制与排水记账：写超不越 cap、排水至多排尽不透支
    let mut b2 = PipeBuf::new(64, false);
    let _ = b2.write(64);
    let out = b2.write(64);
    let mut b4 = PipeBuf::new(128, true);
    let _ = b4.write(100);
    let d1 = b4.drain(30);
    let d2 = b4.drain(200);
    let j12 = b2.used == 64
        && out == WriteOut::WouldBlock
        && d1 == 30
        && d2 == 70
        && b4.used == 0;
    s.add("J12-钳制与排水", j12, "used 不越 cap 且排水记账不透支");

    // J13 (dyn) 状态机指纹落账
    let mut b3 = PipeBuf::new(256, true);
    let _ = b3.write(128);
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(b3.cap as u32);
    h = h.wrapping_mul(31).wrapping_add(b3.used as u32);
    h = h.wrapping_mul(31).wrapping_add(b3.nonblock as u32);
    for c in b3.note.bytes() {
        h = h.wrapping_mul(31).wrapping_add(c as u32);
    }
    s.add_dyn("J13-状态指纹", h != 0 && b3.used == 128, &format!("fold={:#x}", h));

    // J14 (dyn) 矩阵命中明细 + 场景注记对账
    let probe = Scenario { used: 4000, cap: 4096, nonblock: true, write_len: 512 };
    let note = scenario_note(&probe);
    s.add_dyn(
        "J14-命中明细",
        hits == total && note.contains("again") && note.contains("consumed=0"),
        &format!("hits={}/{} cap_ok={} note={}", hits, total, cap_ok, note),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10803_eagain_flow() {
        let mut b = PipeBuf::new(32, true);
        assert_eq!(b.write(20), WriteOut::Done(20));
        assert_eq!(b.write(20), WriteOut::Again);
        assert_eq!(b.used, 20);
        assert_eq!(b.drain(10), 10);
        assert_eq!(b.write(20), WriteOut::Done(20));
        assert_eq!(b.used, 30);
        assert_eq!(b.drain(100), 30);
        assert_eq!(b.used, 0);
        let sc = Scenario { used: 4, cap: 8, nonblock: true, write_len: 0 };
        assert_eq!(judge(&sc), (WriteOut::Done(0), 0));
    }

    #[test]
    fn unx10803_checks_green() {
        let s = run_unx10803_checks();
        assert!(s.all_passed(), "UNX-F10803 判据未全绿");
    }
}
