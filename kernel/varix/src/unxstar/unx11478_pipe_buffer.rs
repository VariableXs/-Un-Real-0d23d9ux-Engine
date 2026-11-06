//! UNX-F11478 管道缓冲与原子写（0x20db0 段）
//!
//! 管道是一个固定容量环形缓冲区。不超过 `PIPE_BUF` 的写入必须原子：要么
//! 全部进入缓冲区，要么在空间不足时返回 `EAGAIN`，绝不部分写入；超过
//! `PIPE_BUF` 的写入允许部分成功，返回实际写入字节数，调用方必须循环。
//! 本模块用 `Vec<u8>` 的前端出队模拟 FIFO，块大小固定为 1024，`PIPE_BUF`
//! 为 128，所有判断都以“剩余空间”为准。
//!
//! 读端关闭后写返回 `EPIPE`（真实内核还会发 SIGPIPE）；写端关闭且缓冲区
//! 为空时读返回 0 表示 EOF，但缓冲区里残留的数据仍可读完。零长度读写是
//! 合法的空操作，不改变任何状态，也不触发 EOF 或 EPIPE。
//!
//! 不变量：`len == buf.len() <= cap`；读出的字节数不超过请求与现有长度；
//! 原子写只在 `free >= n` 时成功；`eof` 仅在读端确实为空且写端已关闭时为真。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20db0;
pub const PP_EID: u32 = SEG_BASE + 0x01;
pub const PP_EORDER: u32 = SEG_BASE + 0x02;
pub const PP_EDUP: u32 = SEG_BASE + 0x03;
pub const PP_EFROZEN: u32 = SEG_BASE + 0x04;
pub const PP_ENOBS: u32 = SEG_BASE + 0x05;
pub const PP_EMISMATCH: u32 = SEG_BASE + 0x06;
pub const PP_EBAL: u32 = SEG_BASE + 0x07;
pub const PP_ESEAL: u32 = SEG_BASE + 0x08;
pub const PP_EAGAIN: u32 = SEG_BASE + 0x09;
pub const PP_EPIPE: u32 = SEG_BASE + 0x0a;
pub const PP_EINVAL: u32 = SEG_BASE + 0x0b;
pub const PP_ENOSPC: u32 = SEG_BASE + 0x0c;
pub const PP_EBADF: u32 = SEG_BASE + 0x0d;
pub const PP_EOVERFLOW: u32 = SEG_BASE + 0x0e;

pub const CAP: usize = 1024;
pub const PIPE_BUF: usize = 128;
pub const SAMPLES: usize = 14;

pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

fn fold(h: u64, v: u64) -> u64 {
    let mut h = h;
    let mut i = 0u32;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xff;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 管道。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pipe {
    pub buf: Vec<u8>,
    pub cap: usize,
    pub readers: u16,
    pub writers: u16,
    pub nonblock: bool,
}

/// 结果快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Out {
    pub n: u32,
    pub len: u32,
    pub eof: bool,
}

impl Pipe {
    pub fn len(&self) -> usize {
        self.buf.len()
    }
    pub fn free(&self) -> usize {
        self.cap.saturating_sub(self.buf.len())
    }
    pub fn write(&mut self, n: usize) -> Result<Out, u32> {
        if self.readers == 0 {
            return Err(PP_EPIPE);
        }
        if n == 0 {
            return Ok(Out { n: 0, len: self.len() as u32, eof: false });
        }
        let free = self.free();
        if n <= PIPE_BUF {
            if free < n {
                return Err(PP_EAGAIN);
            }
            let mut k = 0usize;
            while k < n {
                self.buf.push(0xa5);
                k += 1;
            }
            Ok(Out { n: n as u32, len: self.len() as u32, eof: false })
        } else {
            let k = if n < free { n } else { free };
            if k == 0 {
                return Err(PP_EAGAIN);
            }
            let mut i = 0usize;
            while i < k {
                self.buf.push(0xa5);
                i += 1;
            }
            Ok(Out { n: k as u32, len: self.len() as u32, eof: false })
        }
    }
    pub fn read(&mut self, n: usize) -> Result<Out, u32> {
        if n == 0 {
            return Ok(Out { n: 0, len: self.len() as u32, eof: false });
        }
        if self.buf.is_empty() {
            if self.writers == 0 {
                return Ok(Out { n: 0, len: 0, eof: true });
            }
            return Err(PP_EAGAIN);
        }
        let k = if n < self.buf.len() { n } else { self.buf.len() };
        let mut i = 0usize;
        while i < k {
            let _ = self.buf.remove(0);
            i += 1;
        }
        Ok(Out { n: k as u32, len: self.len() as u32, eof: false })
    }
    pub fn audit(&self) -> Result<(), u32> {
        if self.buf.len() > self.cap {
            return Err(PP_EBAL);
        }
        if self.cap == 0 || self.cap > CAP {
            return Err(PP_EBAL);
        }
        if self.buf.len() > 0 && self.readers == 0 {
            return Err(PP_EBAL);
        }
        Ok(())
    }
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.buf.len() {
            if let Some(&b) = self.buf.get(i) {
                h = fold(h, b as u64);
            }
            i += 1;
        }
        h = fold(h, self.cap as u64);
        h = fold(h, self.readers as u64);
        h = fold(h, self.writers as u64);
        fold(h, u64::from(self.nonblock))
    }
}

/// 基础管道：100 字节已缓冲，读写端各一个。
pub fn base_pipe() -> Pipe {
    let mut buf = Vec::new();
    let mut i = 0usize;
    while i < 100 {
        buf.push((i % 251 + 1) as u8);
        i += 1;
    }
    Pipe { buf, cap: CAP, readers: 1, writers: 1, nonblock: true }
}

/// 场景执行。
pub fn run_scen(id: u8) -> Result<Out, u32> {
    let mut p = base_pipe();
    match id {
        0 => p.write(100),
        1 => p.write(1000),
        2 => p.write(PIPE_BUF),
        3 => {
            p.buf = alloc_vec(PIPE_BUF + 0, 1000);
            p.write(PIPE_BUF)
        }
        4 => {
            p.buf = alloc_vec(0, 1000);
            p.write(PIPE_BUF + 1)
        }
        5 => p.read(50),
        6 => p.read(200),
        7 => {
            p.buf.clear();
            p.read(200)
        }
        8 => {
            p.buf.clear();
            p.writers = 0;
            p.read(200)
        }
        9 => {
            p.readers = 0;
            p.write(10)
        }
        10 => p.read(0),
        11 => p.write(0),
        12 => {
            let _ = p.read(100)?;
            p.writers = 0;
            p.read(100)
        }
        13 => {
            p.writers = 0;
            p.read(200)
        }
        _ => Err(PP_EID),
    }
}

fn alloc_vec(seed: usize, n: usize) -> Vec<u8> {
    let mut v = Vec::new();
    let mut i = 0usize;
    while i < n {
        v.push(((seed + i) % 251 + 1) as u8);
        i += 1;
    }
    v
}

/// 样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub scen: u8,
    pub expect_ok: bool,
    pub n: u32,
    pub len: u32,
    pub eof: bool,
    pub code: u16,
}

/// 结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    pub n: u32,
    pub len: u32,
    pub eof: bool,
    pub code: u16,
}

/// 账本。
pub struct Ledger {
    samples: Vec<Sample>,
    outcomes: Vec<Outcome>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger {
            samples: Vec::new(),
            outcomes: Vec::new(),
            frozen: false,
            root: 0,
            rejects: 0,
        }
    }
    pub fn register(&mut self, slot: u8, scen: u8, ok: bool, n: u32, len: u32, eof: bool, code: u16) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(PP_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(PP_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(PP_EDUP);
        }
        if (slot as usize) != self.samples.len() {
            self.rejects += 1;
            return Err(PP_EORDER);
        }
        let s = Sample { scen, expect_ok: ok, n, len, eof, code };
        let out = match run_scen(scen) {
            Ok(o) => {
                if !ok || o.n != n || o.len != len || o.eof != eof {
                    return Err(PP_EMISMATCH);
                }
                Outcome { ok: true, n: o.n, len: o.len, eof: o.eof, code: 0 }
            }
            Err(e) => {
                let low = (e & 0xffff) as u16;
                if ok || low != code {
                    return Err(PP_EMISMATCH);
                }
                Outcome { ok: false, n: 0, len: 0, eof: false, code: low }
            }
        };
        self.samples.push(s);
        self.outcomes.push(out);
        Ok(())
    }
    pub fn len(&self) -> usize {
        self.samples.len()
    }
    pub fn sample(&self, i: usize) -> Option<Sample> {
        self.samples.get(i).copied()
    }
    pub fn outcome(&self, i: usize) -> Option<Outcome> {
        self.outcomes.get(i).copied()
    }
    pub fn ok_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.outcomes.len() {
            if let Some(o) = self.outcomes.get(i) {
                if o.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
    pub fn err_count(&self) -> usize {
        self.outcomes.len().saturating_sub(self.ok_count())
    }
    pub fn audit(&self) -> Result<(), u32> {
        if self.samples.len() != self.outcomes.len() || self.samples.is_empty() {
            return Err(PP_EBAL);
        }
        let mut i = 0;
        while i < self.samples.len() {
            let s = match self.samples.get(i) {
                Some(v) => *v,
                None => return Err(PP_ENOBS),
            };
            let o = match self.outcomes.get(i) {
                Some(v) => *v,
                None => return Err(PP_ENOBS),
            };
            if s.expect_ok != o.ok {
                return Err(PP_EMISMATCH);
            }
            match run_scen(s.scen) {
                Ok(v) => {
                    if !o.ok || v.n != o.n || v.len != o.len || v.eof != o.eof {
                        return Err(PP_EMISMATCH);
                    }
                    if v.len > CAP as u32 || v.eof && v.n != 0 {
                        return Err(PP_EBAL);
                    }
                }
                Err(e) => {
                    let low = (e & 0xffff) as u16;
                    if o.ok || low != o.code {
                        return Err(PP_EMISMATCH);
                    }
                }
            }
            i += 1;
        }
        Ok(())
    }
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(PP_EFROZEN);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0;
        while i < self.outcomes.len() {
            if let Some(o) = self.outcomes.get(i) {
                h = fold(h, u64::from(o.ok));
                h = fold(h, o.n as u64);
                h = fold(h, o.len as u64);
                h = fold(h, u64::from(o.eof));
                h = fold(h, o.code as u64);
            }
            if let Some(s) = self.samples.get(i) {
                h = fold(h, s.scen as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }
    pub fn root(&self) -> u64 {
        self.root
    }
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() && fresh.ok_count() == self.ok_count() && fresh.err_count() == self.err_count() {
            Ok(fresh.seal())
        } else {
            Err(PP_ESEAL)
        }
    }
}

/// 十四行账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let eagain = (PP_EAGAIN & 0xffff) as u16;
    let epipe = (PP_EPIPE & 0xffff) as u16;
    let _ = l.register(0, 0, true, 100, 200, false, 0);
    let _ = l.register(1, 1, true, 924, 1024, false, 0);
    let _ = l.register(2, 2, true, 128, 228, false, 0);
    let _ = l.register(3, 3, false, 0, 0, false, eagain);
    let _ = l.register(4, 4, true, 24, 1024, false, 0);
    let _ = l.register(5, 5, true, 50, 50, false, 0);
    let _ = l.register(6, 6, true, 100, 0, false, 0);
    let _ = l.register(7, 7, false, 0, 0, false, eagain);
    let _ = l.register(8, 8, true, 0, 0, true, 0);
    let _ = l.register(9, 9, false, 0, 0, false, epipe);
    let _ = l.register(10, 10, true, 0, 100, false, 0);
    let _ = l.register(11, 11, true, 0, 100, false, 0);
    let _ = l.register(12, 12, true, 0, 0, true, 0);
    let _ = l.register(13, 13, true, 100, 0, false, 0);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.ok_count() == 11 && l.err_count() == 3 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let twice = l.freeze() == Err(PP_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && twice;

    let mut p = base_pipe();
    let r = p.read(10);
    let first_after = p.buf.first().copied();
    let w = p.write(20);
    out[1] = r.map(|o| (o.n, o.len)) == Ok((10, 90))
        && first_after == Some(11)
        && w.map(|o| (o.n, o.len)) == Ok((20, 110))
        && p.audit().is_ok();

    let mut q = base_pipe();
    q.buf = alloc_vec(0, CAP);
    let atomic = q.write(PIPE_BUF);
    let large = q.write(PIPE_BUF + 1);
    let _ = q.read(1);
    let partial = q.write(PIPE_BUF + 1);
    out[2] = atomic == Err(PP_EAGAIN)
        && large == Err(PP_EAGAIN)
        && partial.map(|o| o.n) == Ok(1)
        && q.audit().is_ok();

    let mut e = base_pipe();
    e.readers = 0;
    let broken = e.write(1);
    let mut z = Pipe { buf: Vec::new(), cap: CAP, readers: 1, writers: 0, nonblock: true };
    let eof = z.read(10);
    let zero = z.read(0);
    let mut f = base_pipe();
    let fill = f.write(CAP);
    out[3] = broken == Err(PP_EPIPE)
        && eof.map(|o| (o.n, o.eof)) == Ok((0, true))
        && zero.map(|o| o.n) == Ok(0)
        && fill.map(|o| o.len) == Ok(CAP as u32);
    out
}

/// 十三项判据。
pub fn run_unx11478_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11478_pipe_buffer");
    let l = build_ledger();
    s.add("PP1-十四行齐备", l.len() == SAMPLES && l.sample(0).map(|x| x.scen) == Some(0), "static");
    s.add_dyn(
        "PP2-十一应三拒",
        l.ok_count() == 11 && l.err_count() == 3 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );
    s.add(
        "PP3-写入追加",
        {
            let mut p = base_pipe();
            p.write(100).map(|o| (o.n, o.len)) == Ok((100, 200))
        },
        "static",
    );
    s.add(
        "PP4-大块部分写",
        {
            let mut p = base_pipe();
            p.write(1000).map(|o| (o.n, o.len)) == Ok((924, 1024))
        },
        "static",
    );
    s.add(
        "PP5-原子边界",
        {
            let mut a = base_pipe();
            a.buf = alloc_vec(0, 1000);
            let denied = a.write(PIPE_BUF) == Err(PP_EAGAIN);
            let partial = a.write(PIPE_BUF + 1).map(|o| o.n) == Ok(24);
            denied && partial
        },
        "static",
    );
    s.add(
        "PP6-读取 FIFO",
        {
            let mut p = base_pipe();
            let r = p.read(10);
            r.map(|o| o.n) == Ok(10) && p.buf.first().copied() == Some(11) && p.len() == 90
        },
        "static",
    );
    s.add(
        "PP7-EAGAIN 与 EOF 区分",
        {
            let mut a = base_pipe();
            a.buf.clear();
            let again = a.read(10) == Err(PP_EAGAIN);
            a.writers = 0;
            let eof = a.read(10).map(|o| (o.n, o.eof)) == Ok((0, true));
            again && eof
        },
        "static",
    );
    s.add(
        "PP8-读端关闭 EPIPE",
        {
            let mut p = base_pipe();
            p.readers = 0;
            p.write(1) == Err(PP_EPIPE)
        },
        "static",
    );
    s.add(
        "PP9-零长度空操作",
        {
            let mut p = base_pipe();
            p.write(0).map(|o| o.len) == Ok(100) && p.read(0).map(|o| o.n) == Ok(0)
        },
        "static",
    );
    s.add(
        "PP10-关闭写端读完再 EOF",
        {
            let mut p = base_pipe();
            p.writers = 0;
            let first = p.read(200).map(|o| (o.n, o.eof)) == Ok((100, false));
            let second = p.read(200).map(|o| (o.n, o.eof)) == Ok((0, true));
            first && second
        },
        "static",
    );
    s.add(
        "PP11-容量钳制",
        {
            let mut p = base_pipe();
            let r = p.write(10000);
            r.map(|o| o.len) == Ok(CAP as u32) && p.len() == CAP && p.audit().is_ok()
        },
        "static",
    );
    s.add(
        "PP12-独立审计",
        {
            let p = base_pipe();
            p.audit().is_ok() && p.seal() != 0 && p.seal() == base_pipe().seal()
        },
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(PP_EFROZEN);
    s.add_dyn(
        "PP13-冻结重放与四路",
        root != 0 && rep == Ok(root) && again && inject_four() == [true; 4],
        &format!("root={:016x}", root),
    );
    s
}

/// 摘要。
pub fn summary(l: &Ledger) -> alloc::string::String {
    format!("ok={} err={} rows={}", l.ok_count(), l.err_count(), l.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_append() {
        let mut p = base_pipe();
        assert_eq!(p.write(100).map(|o| o.len), Ok(200));
    }

    #[test]
    fn large_write_is_partial() {
        let mut p = base_pipe();
        assert_eq!(p.write(1000).map(|o| o.n), Ok(924));
    }

    #[test]
    fn atomic_write_needs_full_space() {
        let mut p = base_pipe();
        p.buf = alloc_vec(0, 1000);
        assert_eq!(p.write(PIPE_BUF), Err(PP_EAGAIN));
        assert_eq!(p.write(PIPE_BUF + 1).map(|o| o.n), Ok(24));
    }

    #[test]
    fn eof_requires_writer_close() {
        let mut p = base_pipe();
        p.buf.clear();
        assert_eq!(p.read(10), Err(PP_EAGAIN));
        p.writers = 0;
        assert_eq!(p.read(10).map(|o| o.eof), Ok(true));
    }

    #[test]
    fn read_end_closed_is_epipe() {
        let mut p = base_pipe();
        p.readers = 0;
        assert_eq!(p.write(1), Err(PP_EPIPE));
    }

    #[test]
    fn reads_follow_fifo_order() {
        let mut p = base_pipe();
        assert_eq!(p.read(10).map(|o| o.n), Ok(10));
        assert_eq!(p.buf.first().copied(), Some(11));
    }

    #[test]
    fn ledger_is_consistent() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 11);
        assert_eq!(l.err_count(), 3);
        assert!(l.audit().is_ok());
        let s = run_unx11478_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
    }
}
