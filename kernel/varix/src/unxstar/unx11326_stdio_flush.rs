//! UNX-F11326 退出时 stdio 缓冲刷写（0xea60 段）
//!
//! 进程咽气之前，libc 会把所有还开着、还有未落盘字节的流按文件描述符次序刷一遍。
//! 三件事必须钉死：刷写顺序可复现；某个流坏掉只记一次错、不能中断其余流；退出之后
//! 再写任何流都算延迟写入，必须被拒。
//!
//! 本模块把「流」压成四档状态：挂起字节数、缓冲模式、是否坏了、是否已关。刷写只清
//! 挂起字节，不碰模式；关流会先刷后关；退出刷写只做一次。
//!
//! 闸门次序（`write`）：非法 fd → 已关闭 → 单流缓冲上限 → 已退出。把「已退出」放在
//! 上限之后，是为了让「写入量本身越界」这一个错误面永远先响，便于账本逐格复算。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xea60;

/// 非法 fd。
pub const FL_EBADFD: u32 = SEG_BASE + 0x01;
/// 该流没有待刷字节。
pub const FL_EEMPTY: u32 = SEG_BASE + 0x02;
/// 流已关闭。
pub const FL_ECLOSED: u32 = SEG_BASE + 0x03;
/// 流已损坏，刷写失败但其余流继续。
pub const FL_EBROKEN: u32 = SEG_BASE + 0x04;
/// 单流缓冲上限。
pub const FL_EBUF: u32 = SEG_BASE + 0x05;
/// 退出刷写已做过一次。
pub const FL_ERUN: u32 = SEG_BASE + 0x06;
/// 退出后仍试图写入。
pub const FL_EEXIT: u32 = SEG_BASE + 0x07;
/// 流表满。
pub const FL_EFULL: u32 = SEG_BASE + 0x08;
/// fd 重复。
pub const FL_EDUP: u32 = SEG_BASE + 0x09;
/// 账本形状不符。
pub const FL_EBAL: u32 = SEG_BASE + 0x0a;
/// 槽位乱序。
pub const FL_EORDER: u32 = SEG_BASE + 0x0b;
/// 观测值与复算不符。
pub const FL_EMISMATCH: u32 = SEG_BASE + 0x0c;
/// 冻结后重放不一致。
pub const FL_ESEAL: u32 = SEG_BASE + 0x0d;

/// 流数。
pub const STREAMS: usize = 4;
/// 单流缓冲上限。
pub const LIMIT: u32 = 256;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 缓冲模式：全缓冲。
pub const MODE_FULL: u8 = 0;
/// 缓冲模式：行缓冲。
pub const MODE_LINE: u8 = 1;
/// 缓冲模式：无缓冲。
pub const MODE_NONE: u8 = 2;

/// 操作码：写入。
pub const OP_WRITE: u8 = 1;
/// 操作码：单流刷写。
pub const OP_FLUSH1: u8 = 2;
/// 操作码：全刷。
pub const OP_FLUSHALL: u8 = 3;
/// 操作码：关流 / 退出刷写。
pub const OP_CLOSE: u8 = 4;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个 64 位字。
pub fn fold(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut n = v;
    let mut i = 0u32;
    while i < 8 {
        x ^= n & 0xff;
        x = x.wrapping_mul(FNV_PRIME);
        n >>= 8;
        i += 1;
    }
    x
}

/// 一条流。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stream {
    /// 文件描述符。
    pub fd: u8,
    /// 待刷字节。
    pub pending: u32,
    /// 缓冲模式。
    pub mode: u8,
    /// 是否损坏。
    pub broken: bool,
    /// 是否已关。
    pub closed: bool,
}

impl Stream {
    /// 空流。
    pub const fn blank() -> Stream {
        Stream { fd: 0, pending: 0, mode: MODE_FULL, broken: false, closed: false }
    }
}

/// 流表。
#[derive(Clone, Copy, Debug)]
pub struct Stdio {
    s: [Stream; STREAMS],
    n: usize,
    flushes: u32,
    bytes_out: u32,
    errors: u32,
    exited: bool,
}

impl Stdio {
    /// 标准三流（0/1/2）就位。
    pub const fn new() -> Stdio {
        let mut s = [Stream::blank(); STREAMS];
        s[0] = Stream { fd: 0, pending: 0, mode: MODE_LINE, broken: false, closed: false };
        s[1] = Stream { fd: 1, pending: 0, mode: MODE_LINE, broken: false, closed: false };
        s[2] = Stream { fd: 2, pending: 0, mode: MODE_NONE, broken: false, closed: false };
        Stdio { s, n: 3, flushes: 0, bytes_out: 0, errors: 0, exited: false }
    }

    /// 流数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 全刷次数。
    pub fn flushes(&self) -> u32 {
        self.flushes
    }

    /// 累计落盘字节。
    pub fn bytes_out(&self) -> u32 {
        self.bytes_out
    }

    /// 损坏流造成的错误次数。
    pub fn errors(&self) -> u32 {
        self.errors
    }

    /// 是否已退出刷写。
    pub fn is_exited(&self) -> bool {
        self.exited
    }

    /// 第 i 条流。
    pub fn at(&self, i: usize) -> Option<Stream> {
        if i >= self.n {
            return None;
        }
        self.s.get(i).copied()
    }

    /// 按下标找 fd。
    fn index_of(&self, fd: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            if let Some(x) = self.s.get(i) {
                if x.fd == fd {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 待刷总字节。
    pub fn pending_total(&self) -> u32 {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(x) = self.s.get(i) {
                t = t.saturating_add(x.pending);
            }
            i += 1;
        }
        t
    }

    /// 登记一条新流（fd 必须未被占用）。
    pub fn open(&mut self, fd: u8, mode: u8) -> Result<usize, u32> {
        if fd > 15 {
            return Err(FL_EBADFD);
        }
        if mode > MODE_NONE {
            return Err(FL_EBADFD);
        }
        if self.index_of(fd).is_some() {
            return Err(FL_EDUP);
        }
        if self.n >= STREAMS {
            return Err(FL_EFULL);
        }
        if let Some(dst) = self.s.get_mut(self.n) {
            *dst = Stream { fd, pending: 0, mode, broken: false, closed: false };
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 写入若干字节（进入缓冲，不落盘）。
    pub fn write(&mut self, fd: u8, bytes: u32) -> Result<u32, u32> {
        let i = match self.index_of(fd) {
            Some(v) => v,
            None => return Err(FL_EBADFD),
        };
        let closed = match self.s.get(i) {
            Some(x) => x.closed,
            None => return Err(FL_EBADFD),
        };
        if closed {
            return Err(FL_ECLOSED);
        }
        let pending = match self.s.get(i) {
            Some(x) => x.pending,
            None => return Err(FL_EBADFD),
        };
        let next = match pending.checked_add(bytes) {
            Some(v) => v,
            None => return Err(FL_EBUF),
        };
        if next > LIMIT {
            return Err(FL_EBUF);
        }
        if self.exited {
            return Err(FL_EEXIT);
        }
        match self.s.get_mut(i) {
            Some(x) => x.pending = next,
            None => return Err(FL_EBADFD),
        }
        Ok(next)
    }

    /// 把一条流刷下去；损坏流只记错、返回 EBROKEN。
    pub fn flush_fd(&mut self, fd: u8) -> Result<u32, u32> {
        let i = match self.index_of(fd) {
            Some(v) => v,
            None => return Err(FL_EBADFD),
        };
        let st = match self.s.get(i) {
            Some(x) => *x,
            None => return Err(FL_EBADFD),
        };
        if st.closed {
            return Err(FL_ECLOSED);
        }
        if st.pending == 0 {
            return Err(FL_EEMPTY);
        }
        if st.broken {
            self.errors += 1;
            return Err(FL_EBROKEN);
        }
        match self.s.get_mut(i) {
            Some(x) => x.pending = 0,
            None => return Err(FL_EBADFD),
        }
        self.bytes_out = self.bytes_out.saturating_add(st.pending);
        self.flushes += 1;
        Ok(st.pending)
    }

    /// 按 fd 升序全刷一遍，坏流跳过不中断。
    pub fn flush_all(&mut self) -> Result<u32, u32> {
        let mut total = 0u32;
        let mut fd = 0u8;
        while fd < 16 {
            if let Some(i) = self.index_of(fd) {
                let st = match self.s.get(i) {
                    Some(x) => *x,
                    None => return Err(FL_EBADFD),
                };
                if !st.closed && st.pending > 0 {
                    if st.broken {
                        self.errors += 1;
                    } else {
                        match self.s.get_mut(i) {
                            Some(x) => x.pending = 0,
                            None => return Err(FL_EBADFD),
                        }
                        self.bytes_out = self.bytes_out.saturating_add(st.pending);
                        self.flushes += 1;
                        total = total.saturating_add(st.pending);
                    }
                }
            }
            fd += 1;
        }
        Ok(total)
    }

    /// 关闭一条流：先刷后关。返回刷掉的字节。
    pub fn close(&mut self, fd: u8) -> Result<u32, u32> {
        let i = match self.index_of(fd) {
            Some(v) => v,
            None => return Err(FL_EBADFD),
        };
        let st = match self.s.get(i) {
            Some(x) => *x,
            None => return Err(FL_EBADFD),
        };
        if st.closed {
            return Err(FL_ECLOSED);
        }
        let flushed = if st.pending > 0 && !st.broken {
            self.bytes_out = self.bytes_out.saturating_add(st.pending);
            self.flushes += 1;
            st.pending
        } else {
            0
        };
        match self.s.get_mut(i) {
            Some(x) => {
                x.pending = 0;
                x.closed = true;
            }
            None => return Err(FL_EBADFD),
        }
        Ok(flushed)
    }

    /// 退出刷写：只此一次。返回本次落盘字节。
    pub fn exit_flush(&mut self) -> Result<u32, u32> {
        if self.exited {
            return Err(FL_ERUN);
        }
        let n = self.flush_all()?;
        self.exited = true;
        Ok(n)
    }

    /// 待刷字节快照（用于断言刷写后为零）。
    pub fn pending_at(&self, fd: u8) -> Option<u32> {
        match self.index_of(fd) {
            Some(i) => self.s.get(i).map(|x| x.pending),
            None => None,
        }
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold(h, self.n as u64);
        let mut i = 0usize;
        while i < self.n {
            if let Some(x) = self.s.get(i) {
                h = fold(h, x.fd as u64);
                h = fold(h, x.pending as u64);
                h = fold(h, x.mode as u64);
                h = fold(h, x.broken as u64);
                h = fold(h, x.closed as u64);
            }
            i += 1;
        }
        h = fold(h, self.flushes as u64);
        h = fold(h, self.bytes_out as u64);
        h = fold(h, self.errors as u64);
        fold(h, self.exited as u64)
    }
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽位。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 是否成功。
    pub ok: bool,
    /// 错误码。
    pub code: u32,
    /// 观测值。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
    }
}

/// `Result<u32, u32>` → 样本。
fn cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 样本账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    root: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记一格。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(FL_EORDER);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                if s.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(FL_EBAL);
        }
        const VALS: [u32; 8] = [100, 50, 10, 50, 5, 115, 0, 0];
        const CODES: [u32; 6] = [FL_EBADFD, FL_EEMPTY, FL_ECLOSED, FL_EBROKEN, FL_EBUF, FL_ERUN];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(FL_EBAL),
            };
            if s.slot as usize != i {
                return Err(FL_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(FL_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(FL_EMISMATCH);
                        }
                    }
                    None => return Err(FL_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(FL_EMISMATCH);
                        }
                    }
                    None => return Err(FL_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(FL_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold(h, s.slot as u64);
                h = fold(h, s.op as u64);
                h = fold(h, s.ok as u64);
                h = fold(h, s.code as u64);
                h = fold(h, s.value as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(FL_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(FL_ESEAL)
        }
    }

    /// 冻结指纹。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 十四格账本：三流写满、单刷、全刷、关流、退出刷写各走一遍。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut io = Stdio::new();

    let _ = l.register(cell(0, OP_WRITE, io.write(1, 100)));
    let _ = l.register(cell(1, OP_WRITE, io.write(2, 50)));
    let _ = l.register(cell(2, OP_WRITE, io.write(0, 10)));
    let _ = l.register(cell(3, OP_FLUSH1, io.flush_fd(2)));
    let _ = l.register(cell(4, OP_WRITE, io.write(2, 5)));
    let _ = l.register(cell(5, OP_FLUSHALL, io.flush_all()));
    let _ = l.register(cell(6, OP_CLOSE, io.close(1)));
    let _ = l.register(cell(7, OP_FLUSHALL, io.exit_flush()));

    let _ = l.register(cell(8, OP_WRITE, io.write(9, 1)));
    let _ = l.register(cell(9, OP_FLUSH1, io.flush_fd(2)));
    let _ = l.register(cell(10, OP_WRITE, io.write(1, 1)));

    // 第 11 格：坏流刷写只记错、不落盘。
    let mut broken = Stdio::new();
    let _ = broken.write(1, 33);
    if let Some(x) = broken.s.get_mut(1) {
        x.broken = true;
    }
    let _ = l.register(cell(11, OP_FLUSH1, broken.flush_fd(1)));

    let _ = l.register(cell(12, OP_WRITE, io.write(0, LIMIT + 1)));
    let _ = l.register(cell(13, OP_FLUSHALL, io.exit_flush()));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：刷写按 fd 升序，且坏流不中断其余流。
    let mut a = Stdio::new();
    let _ = a.write(2, 7);
    let _ = a.write(0, 11);
    let _ = a.write(1, 13);
    if let Some(x) = a.s.get_mut(1) {
        x.broken = true;
    }
    let total = a.flush_all();
    out[1] = total == Ok(18)
        && a.errors() == 1
        && a.pending_at(0) == Some(0)
        && a.pending_at(1) == Some(13)
        && a.pending_at(2) == Some(0)
        && a.bytes_out() == 18;

    // 二路：关流先刷后关，再写必拒。
    let mut b = Stdio::new();
    let _ = b.write(1, 40);
    let closed = b.close(1);
    let again = b.write(1, 1);
    out[2] = closed == Ok(40) && again == Err(FL_ECLOSED) && b.pending_at(1) == Some(0);

    // 三路：退出刷写只做一次；退出后写入被拒。
    let mut c = Stdio::new();
    let _ = c.write(0, 12);
    let first = c.exit_flush();
    let second = c.exit_flush();
    let late = c.write(0, 1);
    out[3] = first == Ok(12) && second == Err(FL_ERUN) && late == Err(FL_EEXIT) && c.is_exited();

    out
}

/// 十三项判据。
pub fn run_unx11326_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11326.stdio_flush");
    let l = build_ledger();

    s.add_dyn(
        "FL1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "FL2-写入累加",
        l.sample(0).map(|x| x.value) == Some(100) && l.sample(1).map(|x| x.value) == Some(50),
        "static",
    );

    s.add(
        "FL3-单流刷写清零",
        l.sample(3).map(|x| x.value) == Some(50),
        "static",
    );

    s.add(
        "FL4-全刷合计三流",
        l.sample(5).map(|x| x.value) == Some(115),
        "static",
    );

    s.add(
        "FL5-关空流为零",
        l.sample(6).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "FL6-退出刷写一次到位",
        l.sample(7).map(|x| x.value) == Some(0) && l.sample(13).map(|x| x.code) == Some(FL_ERUN),
        "static",
    );

    s.add(
        "FL7-非法 fd 与空刷",
        l.sample(8).map(|x| x.code) == Some(FL_EBADFD)
            && l.sample(9).map(|x| x.code) == Some(FL_EEMPTY),
        "static",
    );

    s.add(
        "FL8-关流后写入被拒",
        l.sample(10).map(|x| x.code) == Some(FL_ECLOSED),
        "static",
    );

    s.add(
        "FL9-坏流记错不落盘",
        l.sample(11).map(|x| x.code) == Some(FL_EBROKEN),
        "static",
    );

    s.add(
        "FL10-单流上限",
        l.sample(12).map(|x| x.code) == Some(FL_EBUF),
        "static",
    );

    s.add(
        "FL11-顺序刷写与坏流隔离",
        {
            let mut a = Stdio::new();
            let _ = a.write(0, 1);
            let _ = a.write(1, 2);
            let _ = a.write(2, 3);
            if let Some(x) = a.s.get_mut(1) {
                x.broken = true;
            }
            let t = a.flush_all();
            t == Ok(4) && a.errors() == 1 && a.pending_total() == 2
        },
        "static",
    );

    s.add(
        "FL12-退出后写入保留挂起数",
        {
            let mut c = Stdio::new();
            let _ = c.write(2, 6);
            let f = c.exit_flush();
            let late = c.write(2, 1);
            f == Ok(6) && late == Err(FL_EEXIT) && c.pending_at(2) == Some(0)
        },
        "static",
    );

    s.add(
        "FL13-冻结重放四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two.is_err()
                && f.is_frozen()
                && f.root() != 0
                && f.rejects() == 1
                && inj == [true; 4]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11326_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn flush_clears_pending() {
        let mut a = Stdio::new();
        assert_eq!(a.write(1, 20), Ok(20));
        assert_eq!(a.flush_fd(1), Ok(20));
        assert_eq!(a.pending_at(1), Some(0));
        assert_eq!(a.flush_fd(1), Err(FL_EEMPTY));
        assert_eq!(a.bytes_out(), 20);
    }

    #[test]
    fn broken_stream_does_not_stop_others() {
        let mut a = Stdio::new();
        let _ = a.write(0, 1);
        let _ = a.write(1, 2);
        if let Some(x) = a.s.get_mut(1) {
            x.broken = true;
        }
        assert_eq!(a.flush_all(), Ok(1));
        assert_eq!(a.errors(), 1);
        assert_eq!(a.pending_at(1), Some(2));
    }

    #[test]
    fn close_flushes_then_locks() {
        let mut a = Stdio::new();
        let _ = a.write(2, 9);
        assert_eq!(a.close(2), Ok(9));
        assert_eq!(a.write(2, 1), Err(FL_ECLOSED));
        assert_eq!(a.flush_fd(2), Err(FL_ECLOSED));
    }

    #[test]
    fn exit_flush_is_single_shot() {
        let mut a = Stdio::new();
        let _ = a.write(0, 4);
        assert_eq!(a.exit_flush(), Ok(4));
        assert_eq!(a.exit_flush(), Err(FL_ERUN));
        assert_eq!(a.write(0, 1), Err(FL_EEXIT));
    }

    #[test]
    fn buffer_limit_is_exact() {
        let mut a = Stdio::new();
        assert_eq!(a.write(0, LIMIT), Ok(LIMIT));
        assert_eq!(a.write(0, 1), Err(FL_EBUF));
        assert_eq!(a.pending_at(0), Some(LIMIT));
    }

    #[test]
    fn open_rejects_duplicates() {
        let mut a = Stdio::new();
        assert_eq!(a.open(1, MODE_FULL), Err(FL_EDUP));
        assert_eq!(a.open(7, MODE_FULL), Ok(4));
        assert_eq!(a.open(8, MODE_FULL), Err(FL_EFULL));
    }
}
