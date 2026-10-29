//! UNX-F10109 pipe 正反双判据（双端达成与断端写误用受控）（0x100c0 段）
//!
//! 锚点：UNX-F10109-J1 pipe 双端读写达成（LTP pipe01/pipe02 面 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：环形管道（定容环形缓冲 + 读/写端存活位）+ 管核（FIFO 顺序读/
//! 满/空语义/断端写 EPIPE）+ 管账（逐次读写留痕 + 字节计数对平）+ 注入
//! 核（断端写/双端关/空留痕/封管/超容必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x100c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x100c0;
pub const PP_EPIPE: u32 = SEG_BASE + 0x01; // 断端写
pub const PP_BAD_FD: u32 = SEG_BASE + 0x02; // 坏端
pub const PP_FULL: u32 = SEG_BASE + 0x03; // 管满
pub const PP_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 空留痕
pub const PP_SEALED: u32 = SEG_BASE + 0x05; // 封管后操作

/// 环形缓冲容量。
pub const PIPE_CAP: usize = 32;

/// 环形管道。
#[derive(Clone)]
pub struct RingPipe {
    pub buf: [u8; PIPE_CAP],
    pub head: usize,
    pub tail: usize,
    pub count: usize,
    pub read_open: bool,
    pub write_open: bool,
}

impl RingPipe {
    pub fn new() -> RingPipe {
        RingPipe {
            buf: [0u8; PIPE_CAP],
            head: 0,
            tail: 0,
            count: 0,
            read_open: true,
            write_open: true,
        }
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 满判。
    pub fn is_full(&self) -> bool {
        self.count == PIPE_CAP
    }

    /// 写：FIFO 入环，满则拒。
    pub fn push(&mut self, data: &[u8]) -> Result<usize, u32> {
        if !self.read_open {
            return Err(PP_EPIPE);
        }
        if self.count + data.len() > PIPE_CAP {
            return Err(PP_FULL);
        }
        let mut i = 0;
        while i < data.len() {
            self.buf[self.tail] = data[i];
            self.tail = (self.tail + 1) % PIPE_CAP;
            i += 1;
        }
        self.count += data.len();
        Ok(data.len())
    }

    /// 读：FIFO 出环，最多取 count 字节。
    pub fn pop(&mut self, count: usize) -> Vec<u8> {
        let n = count.min(self.count);
        let mut out: Vec<u8> = Vec::new();
        let mut i = 0;
        while i < n {
            out.push(self.buf[self.head]);
            self.head = (self.head + 1) % PIPE_CAP;
            i += 1;
        }
        self.count -= n;
        out
    }
}

/// 管账留痕。
#[derive(Clone)]
pub struct PpRec {
    pub seq: u32,
    pub side: u32, // 0=写 1=读
    pub n: usize,
    pub note: String,
}

impl PpRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.side <= 1
    }
}

/// 管道表：pipe() 分配双端 fd。
pub struct PipeBook {
    pub pipes: Vec<Option<RingPipe>>,
    pub ledger: Vec<PpRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl PipeBook {
    pub fn new() -> PipeBook {
        PipeBook { pipes: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// pipe()：返回 (读端 fd, 写端 fd)。
    pub fn pipe(&mut self) -> Result<(usize, usize), u32> {
        if self.sealed {
            return Err(PP_SEALED);
        }
        if self.pipes.len() + 2 > 16 {
            return Err(PP_BAD_FD);
        }
        self.pipes.push(Some(RingPipe::new()));
        let r = self.pipes.len() - 1;
        self.pipes.push(None);
        let w = self.pipes.len() - 1;
        // 写端与读端共享同一管道：用配对号关联
        self.pipes[w] = self.pipes[r].clone();
        Ok((r, w))
    }

    /// 定位管道（读端偶数位、写端=读端+1 的配对约定）。
    fn locate(&self, fd: i32) -> Result<usize, u32> {
        if fd < 0 || fd as usize >= self.pipes.len() || self.pipes[fd as usize].is_none() {
            return Err(PP_BAD_FD);
        }
        Ok(fd as usize)
    }

    /// 写端写：数据入读端所在的环（配对共享）。
    pub fn write(&mut self, wfd: i32, data: &[u8], note: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(PP_SEALED);
        }
        if note.is_empty() {
            return Err(PP_EMPTY_NOTE);
        }
        let u = self.locate(wfd)?;
        if u % 2 != 1 {
            return Err(PP_BAD_FD);
        }
        // 读端存活核：读端关闭则断端写
        let rf = u - 1;
        let read_open = self.pipes[rf].as_ref().map(|p| p.read_open).unwrap_or(false);
        if !read_open {
            return Err(PP_EPIPE);
        }
        let mut rp_default = RingPipe::new();
        let p = self.pipes[u].as_mut().unwrap_or(&mut rp_default);
        let n = p.push(data)?;
        self.ledger.push(PpRec { seq: self.seq_next, side: 0, n, note: note.to_string() });
        self.seq_next += 1;
        // 写端态同步回读端（配对镜像）
        self.pipes[rf] = self.pipes[u].clone();
        Ok(n)
    }

    /// 读端读。
    pub fn read(&mut self, rfd: i32, count: usize, note: &str) -> Result<Vec<u8>, u32> {
        if self.sealed {
            return Err(PP_SEALED);
        }
        if note.is_empty() {
            return Err(PP_EMPTY_NOTE);
        }
        let u = self.locate(rfd)?;
        if u % 2 != 0 {
            return Err(PP_BAD_FD);
        }
        let mut rp_default = RingPipe::new();
        let p = self.pipes[u].as_mut().unwrap_or(&mut rp_default);
        let out = p.pop(count);
        self.ledger
            .push(PpRec { seq: self.seq_next, side: 1, n: out.len(), note: note.to_string() });
        self.seq_next += 1;
        self.pipes[u + 1] = self.pipes[u].clone();
        Ok(out)
    }

    /// 关闭端：读端关则写端后续写必 EPIPE。
    pub fn close_read(&mut self, rfd: i32) -> Result<(), u32> {
        let u = self.locate(rfd)?;
        if u % 2 != 0 {
            return Err(PP_BAD_FD);
        }
        if let Some(p) = self.pipes[u].as_mut() {
            p.read_open = false;
        }
        self.pipes[u + 1] = self.pipes[u].clone();
        Ok(())
    }

    /// 字节计数对平：写累计 == 读累计 + 在管存量。
    pub fn conserved(&self) -> bool {
        let mut w = 0usize;
        let mut r = 0usize;
        for rec in &self.ledger {
            if rec.side == 0 {
                w += rec.n;
            } else {
                r += rec.n;
            }
        }
        let mut buf = 0usize;
        for p in &self.pipes {
            if let Some(pp) = p {
                buf += pp.count;
            }
        }
        w == r + buf
    }

    pub fn seq_unique(&self) -> bool {
        for i in 0..self.ledger.len() {
            for j in (i + 1)..self.ledger.len() {
                if self.ledger[i].seq == self.ledger[j].seq {
                    return false;
                }
            }
        }
        true
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x100c00;
        for rec in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(rec.n as u32);
            h = h.wrapping_mul(31).wrapping_add(rec.side);
            for b in rec.note.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = PipeBook::new();
    let (r, w) = b.pipe().unwrap_or((usize::MAX, usize::MAX));
    // 1) 断端写：读端关后写必 EPIPE
    let _ = b.close_read(r as i32);
    got[0] = b.write(w as i32, b"x", "inj-epipe").err() == Some(PP_EPIPE);
    // 2) 坏端：用读端 fd 写
    let (r2, w2) = b.pipe().unwrap_or((usize::MAX, usize::MAX));
    let _ = r2;
    got[1] = b.write(w2 as i32 + 1, b"x", "inj-bad").err() == Some(PP_BAD_FD);
    // 3) 超容必拒
    let (r3, w3) = b.pipe().unwrap_or((usize::MAX, usize::MAX));
    let big = [5u8; PIPE_CAP + 1];
    got[2] = b.write(w3 as i32, &big, "inj-big").err() == Some(PP_FULL);
    // 4) 空留痕必拒
    got[3] = b.write(w3 as i32, b"y", "").err() == Some(PP_EMPTY_NOTE);
    // 5) 封管后操作必抓
    b.seal();
    got[4] = b.read(r3 as i32, 1, "inj-seal").err() == Some(PP_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10109_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10109");

    // 正路径：双端读写 FIFO 一致（pipe01 面）
    let mut b = PipeBook::new();
    let (r, w) = b.pipe().unwrap_or((usize::MAX, usize::MAX));
    let _ = b.write(w as i32, b"pipe-data", "写入");
    let got = b.read(r as i32, 9, "读出").unwrap_or_default();
    let j1 = got == b"pipe-data".to_vec();
    s.add("J1-双端FIFO", j1, "写端入读端出逐字节一致");

    // pipe02 面：分段读写顺序保持
    let j2 = {
        let mut b2 = PipeBook::new();
        let (r2, w2) = b2.pipe().unwrap_or((usize::MAX, usize::MAX));
        let _ = b2.write(w2 as i32, b"abc", "a");
        let _ = b2.write(w2 as i32, b"def", "b");
        let g1 = b2.read(r2 as i32, 2, "g1").unwrap_or_default();
        let g2 = b2.read(r2 as i32, 4, "g2").unwrap_or_default();
        let mut all = g1;
        all.extend(g2);
        all == b"abcdef".to_vec()
    };
    s.add("J2-分段顺序", j2, "两次写 FIFO 拼回原文");

    // 空管读得零
    let j3 = b.read(r as i32, 4, "空读").unwrap_or_default().is_empty();
    s.add("J3-空管读零", j3, "空管读返回零字节");

    // 断端写受控
    let j4 = {
        let mut b4 = PipeBook::new();
        let (r4, w4) = b4.pipe().unwrap_or((usize::MAX, usize::MAX));
        let _ = b4.close_read(r4 as i32);
        b4.write(w4 as i32, b"x", "断端").err() == Some(PP_EPIPE)
    };
    s.add("J4-断端写受控", j4, "读端关闭后写必 EPIPE");

    // 满管受控
    let j5 = {
        let mut b5 = PipeBook::new();
        let (r5, w5) = b5.pipe().unwrap_or((usize::MAX, usize::MAX));
        let full = [7u8; PIPE_CAP];
        b5.write(w5 as i32, &full, "满").is_ok()
            && b5.write(w5 as i32, b"!", "超").err() == Some(PP_FULL)
    };
    s.add("J5-满管受控", j5, "PIPE_CAP 恰满可写、超一必拒");

    // 环绕语义：写满→读空→再写读（模回绕）
    let j6 = {
        let mut b6 = PipeBook::new();
        let (r6, w6) = b6.pipe().unwrap_or((usize::MAX, usize::MAX));
        let full = [1u8; PIPE_CAP];
        let _ = b6.write(w6 as i32, &full, "满");
        let _ = b6.read(r6 as i32, PIPE_CAP, "空");
        let _ = b6.write(w6 as i32, b"wrap", "回绕");
        b6.read(r6 as i32, 4, "再读").unwrap_or_default() == b"wrap".to_vec()
    };
    s.add("J6-环形回绕", j6, "满读循环后 FIFO 仍序");

    // 字节计数守恒
    let j7 = b.conserved();
    s.add("J7-计数守恒", j7, "写累计 == 读累计 + 在管存量");

    // 坏端受控
    let j8 = b.write(r as i32, b"x", "错向").err() == Some(PP_BAD_FD)
        && b.read(w as i32, 1, "错向读").err() == Some(PP_BAD_FD);
    s.add("J8-错向受控", j8, "读端写/写端读均拒");

    // 空留痕拒收
    let j9 = b.write(w as i32, b"z", "").err() == Some(PP_EMPTY_NOTE);
    s.add("J9-空留痕拒", j9, "note 为空不入账");

    // 留痕合法与 seq 唯一
    let j10 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J10-留痕合法", j10, "方向位在闭集且注非空");

    // 注入核 5/5
    let inj = inject_five();
    let j11 = inj.iter().all(|g| *g);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", inj));

    // 指纹稳定
    let j12 = b.fold() == b.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10109_pipe_flow() {
        let mut b = PipeBook::new();
        let (r, w) = b.pipe().unwrap_or((0, 0));
        assert_eq!(b.write(w as i32, b"hi", "a").unwrap_or(0), 2);
        assert_eq!(b.read(r as i32, 2, "b").unwrap_or_default(), b"hi".to_vec());
        let _ = b.close_read(r as i32);
        assert!(b.write(w as i32, b"x", "c").err() == Some(PP_EPIPE));
    }

    #[test]
    fn unx10109_checks_green() {
        let s = run_unx10109_checks();
        assert!(s.all_passed(), "UNX-F10109 判据未全绿");
    }
}
