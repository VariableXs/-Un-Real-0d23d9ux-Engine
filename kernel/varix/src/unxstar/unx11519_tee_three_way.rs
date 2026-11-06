//! UNX-F11519 tee 三通分流（-a 追加）
//!
//! `tee` 是管道里的「三通」：它把标准输入的每一块**同时**复制到标准输出和若干
//! 个文件。真正值得建模的不是「复制」，而是复制过程里的三件事：
//!
//! 1. **打开时机决定内容**。不带 `-a` 时文件在**打开**那一刻被截断，此后每块
//!    顺序追加；带 `-a` 时保留原有内容，每块接在尾部。所以「偏移」不是写入时算
//!    的，而是打开时就定下的：本实现把 `add` 当作 open，`append` 决定是否把
//!    `used` 清零，写块只做累加；
//! 2. **故障是逐路隔离的**。某一路写失败（只读、已关闭、目录、空间不足）不该
//!    影响其它路——这正是 tee 与 `cat >` 的根本区别。因此每次写返回的是一份
//!    `Report`（几路成功、几路失败、首个错误码），而不是一个 `Err`。整体退出
//!    码是 `failed > 0`，失败路数不计入 `copies`；
//! 3. **短写要留下痕迹**。空间不足时 tee 会写出能写的那一段再报错，`used`
//!    顶到容量、`written` 只记实际落盘的字节。本实现按 `cap - used` 算余量，
//!    余量不足即短写并置 `TT_ENOSPC`。
//!
//! 闸门次序（`write`）：块长（`TT_ECHUNK`）→ 逐路（目录 → 只读 → 已关闭 →
//! 余量）→ 总预算（`TT_EBYTES`）。块长在最前：一块就超上限时逐路算余量毫无
//! 意义。
//!
//! 严格模式 `write_strict` 在宽容写之后把**首个**错误码上抛，用于「宁可整体
//! 失败」的调用方；两种模式共用同一套逐路逻辑，不存在两套计数。
//!
//! 账本语义：十四格 = 八格成功（复制计数、追加、截断、短写、隔离、关闭、目录、
//! 零路）+ 六格失败（选项、块长、表满、预算、严格短写、严格目录）。`audit` 认
//! 「八值六码」，`seal` 做 FNV-1a 折叠，`replay` 重建比对，`freeze` 后拒绝。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf180;

/// 选项非法。
pub const TT_EFLAG: u32 = SEG_BASE + 0x01;
/// 单块超过块上限。
pub const TT_ECHUNK: u32 = SEG_BASE + 0x02;
/// 输出文件表满。
pub const TT_EFULL: u32 = SEG_BASE + 0x03;
/// 复制总量越过预算。
pub const TT_EBYTES: u32 = SEG_BASE + 0x04;
/// 目标余量不足（短写）。
pub const TT_ENOSPC: u32 = SEG_BASE + 0x05;
/// 目标是目录。
pub const TT_EDIR: u32 = SEG_BASE + 0x06;
/// 目标只读。
pub const TT_EACCES: u32 = SEG_BASE + 0x07;
/// 目标已关闭。
pub const TT_ECLOSED: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const TT_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const TT_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const TT_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const TT_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const TT_ENOBS: u32 = SEG_BASE + 0x0d;

/// 可读可写。
pub const RW: u8 = 0;
/// 只读。
pub const RDONLY: u8 = 1;
/// 目录。
pub const DIR: u8 = 2;
/// 已关闭。
pub const CLOSED: u8 = 3;

/// 输出文件表容量（不含标准输出）。
pub const SINKS: usize = 3;
/// 单块字节上限。
pub const CHUNK_MAX: u32 = 32;
/// 复制总量预算。
pub const OUT_CAP: u32 = 512;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：打开。
pub const OP_OPEN: u8 = 1;
/// 操作码：写块。
pub const OP_WRITE: u8 = 2;
/// 操作码：严格写。
pub const OP_STRICT: u8 = 3;

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

/// 一路输出。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sink {
    used: u32,
    cap: u32,
    mode: u8,
}

impl Sink {
    /// 已落盘字节。
    pub fn used(&self) -> u32 {
        self.used
    }

    /// 容量。
    pub fn cap(&self) -> u32 {
        self.cap
    }

    /// 状态字。
    pub fn mode(&self) -> u8 {
        self.mode
    }

    /// 余量。
    pub fn room(&self) -> u32 {
        self.cap.saturating_sub(self.used)
    }
}

/// 一次写的结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    /// 完整收到的路数。
    pub copies: u32,
    /// 实际落盘字节（各路之和）。
    pub written: u32,
    /// 失败路数。
    pub failed: u32,
    /// 首个错误码；0 表示全好。
    pub err: u32,
}

impl Report {
    /// 退出码：有一路失败即 1。
    pub fn status(&self) -> u32 {
        if self.failed > 0 {
            1
        } else {
            0
        }
    }
}

/// 三通分流器。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tee {
    append: bool,
    sinks: [Sink; SINKS],
    n: usize,
    total: u32,
}

impl Tee {
    /// 解析选项串：只认 `a`，重复也合法；其它字符一律 `TT_EFLAG`。
    pub fn flags(args: &'static str) -> Result<Tee, u32> {
        let b = args.as_bytes();
        let mut append = false;
        let mut i = 0usize;
        while i < b.len() {
            match b.get(i) {
                Some(b'a') => append = true,
                Some(_) => return Err(TT_EFLAG),
                None => break,
            }
            i += 1;
        }
        Ok(Tee::new(append))
    }

    /// 直接给定追加标志。
    pub const fn new(append: bool) -> Tee {
        Tee {
            append,
            sinks: [Sink { used: 0, cap: 0, mode: RW }; SINKS],
            n: 0,
            total: 0,
        }
    }

    /// 是否追加。
    pub fn is_append(&self) -> bool {
        self.append
    }

    /// 打开一路：`append` 为假时清零已有内容。
    pub fn add(&mut self, cap: u32, used: u32, mode: u8) -> Result<usize, u32> {
        if self.n >= SINKS {
            return Err(TT_EFULL);
        }
        let start = if self.append { used } else { 0 };
        if let Some(dst) = self.sinks.get_mut(self.n) {
            *dst = Sink { used: start.min(cap), cap, mode };
        }
        let idx = self.n;
        self.n += 1;
        Ok(idx)
    }

    /// 路数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否零路。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 第 i 路。
    pub fn sink(&self, i: usize) -> Option<Sink> {
        if i >= self.n {
            return None;
        }
        self.sinks.get(i).copied()
    }

    /// 首路错误码（本实现只记首个）。
    fn note(r: &mut Report, code: u32) {
        r.failed = r.failed.saturating_add(1);
        if r.err == 0 {
            r.err = code;
        }
    }

    /// 宽容写：逐路隔离，返回报告而不上抛。
    pub fn write(&mut self, chunk: &[u8]) -> Result<Report, u32> {
        if chunk.len() as u32 > CHUNK_MAX {
            return Err(TT_ECHUNK);
        }
        let want = chunk.len() as u32;
        let mut r = Report { copies: 0, written: 0, failed: 0, err: 0 };
        let mut i = 0usize;
        while i < self.n {
            let idx = i;
            if let Some(s) = self.sinks.get_mut(idx) {
                match s.mode {
                    DIR => Tee::note(&mut r, TT_EDIR),
                    RDONLY => Tee::note(&mut r, TT_EACCES),
                    CLOSED => Tee::note(&mut r, TT_ECLOSED),
                    _ => {
                        let room = s.room();
                        if want <= room {
                            s.used = s.used.saturating_add(want);
                            r.copies = r.copies.saturating_add(1);
                            r.written = r.written.saturating_add(want);
                        } else {
                            s.used = s.cap;
                            r.written = r.written.saturating_add(room);
                            Tee::note(&mut r, TT_ENOSPC);
                        }
                    }
                }
            }
            i += 1;
        }
        self.total = self.total.saturating_add(r.written);
        if self.total > OUT_CAP {
            return Err(TT_EBYTES);
        }
        Ok(r)
    }

    /// 严格写：首路错误即上抛。
    pub fn write_strict(&mut self, chunk: &[u8]) -> Result<Report, u32> {
        let r = self.write(chunk)?;
        if r.err != 0 {
            return Err(r.err);
        }
        Ok(r)
    }

    /// 累计复制字节。
    pub fn total(&self) -> u32 {
        self.total
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.n > SINKS || self.total > OUT_CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            match self.sinks.get(i) {
                Some(s) => {
                    if s.mode > CLOSED || s.used > s.cap {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
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
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
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
            self.rejects = self.rejects.saturating_add(1);
            return Err(TT_ESLOT);
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

    /// 对账：八成功值 + 六失败码，且槽位连续。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(TT_EBAL);
        }
        const VALS: [u32; 8] = [6, 7, 3, 1, 2, 1, 1, 0];
        const CODES: [u32; 6] =
            [TT_EFLAG, TT_ECHUNK, TT_EFULL, TT_EBYTES, TT_ENOSPC, TT_EDIR];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(TT_EBAL),
            };
            if s.slot as usize != i {
                return Err(TT_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(TT_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(TT_EVALUE);
                        }
                    }
                    None => return Err(TT_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(TT_EVALUE);
                        }
                    }
                    None => return Err(TT_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(TT_EBAL);
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
            self.rejects = self.rejects.saturating_add(1);
            return Err(TT_EBAL);
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
            Err(TT_ESEAL)
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

/// 写块。
const CH: &[u8] = b"abc";

/// 起手：不带 `-a`。
fn mk() -> Tee {
    Tee::new(false)
}

/// 十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    // 0：两路各收三字节。
    let mut t0 = mk();
    let _ = t0.add(16, 0, RW);
    let _ = t0.add(16, 2, RW);
    let _ = l.register(word_cell(0, OP_WRITE, t0.write(CH).map(|r| r.written)));

    // 1：追加保留原偏移。
    let mut t1 = Tee::new(true);
    let _ = t1.add(16, 4, RW);
    let _ = l.register(word_cell(
        1,
        OP_WRITE,
        t1.write(CH).and_then(|_| match t1.sink(0) {
            Some(s) => Ok(s.used()),
            None => Err(TT_ENOBS),
        }),
    ));

    // 2：非追加在打开时截断。
    let mut t2 = mk();
    let _ = t2.add(16, 4, RW);
    let _ = l.register(word_cell(
        2,
        OP_WRITE,
        t2.write(CH).and_then(|_| match t2.sink(0) {
            Some(s) => Ok(s.used()),
            None => Err(TT_ENOBS),
        }),
    ));

    // 3：余量一字节，短写（追加模式保留原有 4 字节）。
    let mut t3 = Tee::new(true);
    let _ = t3.add(5, 4, RW);
    let _ = l.register(word_cell(3, OP_WRITE, t3.write(CH).map(|r| r.written)));

    // 4：只读一路不拖累另一路。
    let mut t4 = mk();
    let _ = t4.add(16, 0, RW);
    let _ = t4.add(16, 0, RDONLY);
    let _ = t4.add(16, 0, RW);
    let _ = l.register(word_cell(4, OP_WRITE, t4.write(CH).map(|r| r.copies)));

    // 5：关闭一路。
    let mut t5 = mk();
    let _ = t5.add(16, 0, RW);
    let _ = t5.add(16, 0, CLOSED);
    let _ = l.register(word_cell(5, OP_WRITE, t5.write(CH).map(|r| r.copies)));

    // 6：目录一路。
    let mut t6 = mk();
    let _ = t6.add(16, 0, DIR);
    let _ = l.register(word_cell(6, OP_WRITE, t6.write(CH).map(|r| r.failed)));

    // 7：零路只走标准输出。
    let mut t7 = mk();
    let _ = l.register(word_cell(7, OP_WRITE, t7.write(CH).map(|r| r.written)));

    // 8：选项非法。
    let _ = l.register(word_cell(8, OP_OPEN, Tee::flags("x").map(|t| t.len() as u32)));
    // 9：块超上限。
    let mut t9 = mk();
    let _ = t9.add(64, 0, RW);
    let big = [b'z'; 33];
    let _ = l.register(word_cell(9, OP_WRITE, t9.write(&big).map(|r| r.written)));
    // 10：第四路装不下。
    let mut ta = mk();
    let _ = ta.add(16, 0, RW);
    let _ = ta.add(16, 0, RW);
    let _ = ta.add(16, 0, RW);
    let _ = l.register(word_cell(10, OP_OPEN, ta.add(16, 0, RW).map(|i| i as u32)));
    // 11：预算顶穿。
    let mut tb = mk();
    let _ = tb.add(1000, 0, RW);
    let _ = tb.add(1000, 0, RW);
    let blk = [b'q'; 32];
    let mut step = 0u32;
    let mut last: Result<u32, u32> = Ok(0);
    while step < 9 {
        last = tb.write(&blk).map(|_| step);
        step += 1;
    }
    let _ = l.register(word_cell(11, OP_WRITE, last));
    // 12：严格模式上抛短写。
    let mut tc = Tee::new(true);
    let _ = tc.add(5, 4, RW);
    let _ = l.register(word_cell(12, OP_STRICT, tc.write_strict(CH).map(|r| r.written)));
    // 13：严格模式上抛目录。
    let mut td = mk();
    let _ = td.add(16, 0, DIR);
    let _ = l.register(word_cell(13, OP_STRICT, td.write_strict(CH).map(|r| r.written)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：三通计数。
    let mut t = Tee::new(true);
    let _ = t.add(16, 0, RW);
    let _ = t.add(16, 2, RW);
    let r = match t.write(b"abc") {
        Ok(v) => v,
        Err(_) => return out,
    };
    out[1] = r == Report { copies: 2, written: 6, failed: 0, err: 0 }
        && r.status() == 0
        && t.total() == 6
        && t.sink(0).map(|s| s.used()) == Some(3)
        && t.sink(1).map(|s| s.used()) == Some(5)
        && t.consistent();

    // 二路：追加与截断。
    let mut a = Tee::new(true);
    let _ = a.add(16, 4, RW);
    let mut n = mk();
    let _ = n.add(16, 4, RW);
    out[2] = a.is_append()
        && !n.is_append()
        && a.sink(0).map(|s| s.used()) == Some(4)
        && n.sink(0).map(|s| s.used()) == Some(0)
        && a.write(CH).is_ok()
        && n.write(CH).is_ok()
        && a.sink(0).map(|s| s.used()) == Some(7)
        && n.sink(0).map(|s| s.used()) == Some(3)
        && a.write(b"xx").is_ok()
        && a.sink(0).map(|s| s.used()) == Some(9);

    // 三路：闸门与隔离。
    let mut p = Tee::new(true);
    let _ = p.add(5, 4, RW);
    let short = p.write(CH);
    let mut q = mk();
    let _ = q.add(16, 0, RW);
    let _ = q.add(16, 0, RDONLY);
    let _ = q.add(16, 0, CLOSED);
    let mixed = q.write(CH);
    out[3] = short.map(|r| (r.written, r.err, r.status())) == Ok((1, TT_ENOSPC, 1))
        && mixed.map(|r| (r.copies, r.failed, r.err)) == Ok((1, 2, TT_EACCES))
        && p.write_strict(b"yy") == Err(TT_ENOSPC)
        && Tee::flags("x") == Err(TT_EFLAG)
        && {
            let mut z = mk();
            let big = [b'z'; 33];
            z.write(&big) == Err(TT_ECHUNK)
        };

    out
}

/// 十三项判据。
pub fn run_unx11519_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11519.tee_three_way");
    let l = build_ledger();

    s.add_dyn(
        "TT1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "TT2-三通复制计数",
        {
            let mut t = mk();
            let _ = t.add(16, 0, RW);
            let _ = t.add(16, 2, RW);
            matches!(t.write(b"abc"), Ok(ref r)
                if r.copies == 2 && r.written == 6 && r.failed == 0 && r.err == 0)
                && l.sample(0).map(|x| x.value) == Some(6)
        },
        "static",
    );

    s.add(
        "TT3-追加与截断",
        {
            let mut a = Tee::new(true);
            let _ = a.add(16, 4, RW);
            let mut n = mk();
            let _ = n.add(16, 4, RW);
            let before_a = a.sink(0).map(|x| x.used());
            let before_n = n.sink(0).map(|x| x.used());
            let _ = a.write(CH);
            let _ = n.write(CH);
            before_a == Some(4)
                && before_n == Some(0)
                && a.sink(0).map(|x| x.used()) == Some(7)
                && n.sink(0).map(|x| x.used()) == Some(3)
                && l.sample(1).map(|x| x.value) == Some(7)
                && l.sample(2).map(|x| x.value) == Some(3)
        },
        "static",
    );

    s.add(
        "TT4-短写报ENOSPC",
        {
            let mut t = Tee::new(true);
            let _ = t.add(5, 4, RW);
            matches!(t.write(CH), Ok(ref r)
                if r.written == 1 && r.copies == 0 && r.err == TT_ENOSPC && r.status() == 1)
                && t.sink(0).map(|x| x.used()) == Some(5)
                && t.sink(0).map(|x| x.room()) == Some(0)
                && l.sample(3).map(|x| x.value) == Some(1)
        },
        "static",
    );

    s.add(
        "TT5-失败逐路隔离",
        {
            let mut t = mk();
            let _ = t.add(16, 0, RW);
            let _ = t.add(16, 0, RDONLY);
            let _ = t.add(16, 0, RW);
            matches!(t.write(CH), Ok(ref r)
                if r.copies == 2 && r.failed == 1 && r.written == 6 && r.err == TT_EACCES)
                && t.sink(0).map(|x| x.used()) == Some(3)
                && t.sink(1).map(|x| x.used()) == Some(0)
                && t.sink(2).map(|x| x.used()) == Some(3)
                && l.sample(4).map(|x| x.value) == Some(2)
        },
        "static",
    );

    s.add(
        "TT6-目录与已关闭",
        {
            let mut d = mk();
            let _ = d.add(16, 0, DIR);
            let mut c = mk();
            let _ = c.add(16, 0, RW);
            let _ = c.add(16, 0, CLOSED);
            matches!(d.write(CH), Ok(ref r) if r.err == TT_EDIR && r.failed == 1)
                && matches!(c.write(CH), Ok(ref r) if r.err == TT_ECLOSED && r.copies == 1)
                && d.write_strict(CH) == Err(TT_EDIR)
                && l.sample(6).map(|x| x.value) == Some(1)
                && l.sample(13).map(|x| x.code) == Some(TT_EDIR)
        },
        "static",
    );

    s.add(
        "TT7-块上限先行",
        {
            let mut t = mk();
            let _ = t.add(64, 0, RW);
            let ok = [b'z'; 32];
            let big = [b'z'; 33];
            t.write(&ok).is_ok()
                && t.write(&big) == Err(TT_ECHUNK)
                && t.sink(0).map(|x| x.used()) == Some(32)
                && l.sample(9).map(|x| x.code) == Some(TT_ECHUNK)
        },
        "static",
    );

    s.add(
        "TT8-输出文件表满",
        {
            let mut t = mk();
            t.add(16, 0, RW).is_ok()
                && t.add(16, 0, RW).is_ok()
                && t.add(16, 0, RW).is_ok()
                && t.add(16, 0, RW) == Err(TT_EFULL)
                && t.len() == SINKS
                && l.sample(10).map(|x| x.code) == Some(TT_EFULL)
        },
        "static"
    );

    s.add(
        "TT9-复制总量预算",
        {
            let mut t = mk();
            let _ = t.add(1000, 0, RW);
            let _ = t.add(1000, 0, RW);
            let blk = [b'q'; 32];
            let mut i = 0u32;
            let mut last = Ok(Report { copies: 1, written: 32, failed: 0, err: 0 });
            while i < 9 {
                last = t.write(&blk);
                i += 1;
            }
            last == Err(TT_EBYTES)
                && t.total() > OUT_CAP
                && l.sample(11).map(|x| x.code) == Some(TT_EBYTES)
        },
        "static",
    );

    s.add(
        "TT10-严格模式上抛",
        {
            let mut t = Tee::new(true);
            let _ = t.add(5, 4, RW);
            let mut u = mk();
            let _ = u.add(16, 0, RW);
            let _ = u.add(16, 0, CLOSED);
            t.write_strict(CH) == Err(TT_ENOSPC)
                && u.write_strict(CH) == Err(TT_ECLOSED)
                && matches!(u.write(CH), Ok(ref r) if r.copies == 1)
                && l.sample(12).map(|x| x.code) == Some(TT_ENOSPC)
        },
        "static",
    );

    s.add(
        "TT11-零路只走标准输出",
        {
            let mut t = mk();
            matches!(t.write(CH), Ok(ref r)
                if r.copies == 0 && r.written == 0 && r.failed == 0 && r.status() == 0)
                && t.is_empty()
                && t.len() == 0
                && t.sink(0).is_none()
                && l.sample(7).map(|x| x.value) == Some(0)
        },
        "static",
    );

    s.add(
        "TT12-状态码聚合",
        {
            let mut a = mk();
            let _ = a.add(16, 0, RW);
            let mut b = mk();
            let _ = b.add(16, 0, RW);
            let _ = b.add(16, 0, CLOSED);
            matches!(a.write(CH), Ok(ref r) if r.status() == 0)
                && matches!(b.write(CH), Ok(ref r) if r.status() == 1)
                && Report { copies: 0, written: 0, failed: 0, err: 0 }.status() == 0
                && Report { copies: 0, written: 0, failed: 3, err: TT_EDIR }.status() == 1
        },
        "static",
    );

    s.add(
        "TT13-冻结重放四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two.is_err()
                && f.rejects() == 1
                && f.is_frozen()
                && f.root() != 0
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
        assert_eq!(l.rejects(), 0);
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11519_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn fan_out_counts() {
        let mut t = Tee::new(true);
        assert_eq!(t.add(16, 0, RW), Ok(0));
        assert_eq!(t.add(16, 2, RW), Ok(1));
        assert_eq!(
            t.write(b"abc"),
            Ok(Report { copies: 2, written: 6, failed: 0, err: 0 })
        );
        assert_eq!(t.sink(1).map(|s| s.used()), Some(5));
    }

    #[test]
    fn append_keeps_offset() {
        let mut a = Tee::new(true);
        let _ = a.add(16, 4, RW);
        let mut n = Tee::new(false);
        let _ = n.add(16, 4, RW);
        assert_eq!(a.sink(0).map(|s| s.used()), Some(4));
        assert_eq!(n.sink(0).map(|s| s.used()), Some(0));
        let _ = a.write(CH);
        let _ = n.write(CH);
        assert_eq!(a.sink(0).map(|s| s.used()), Some(7));
        assert_eq!(n.sink(0).map(|s| s.used()), Some(3));
    }

    #[test]
    fn short_write_reports() {
        let mut t = Tee::new(true);
        let _ = t.add(5, 4, RW);
        match t.write(CH) {
            Ok(r) => {
                assert_eq!(r.written, 1);
                assert_eq!(r.err, TT_ENOSPC);
                assert_eq!(r.status(), 1);
            }
            Err(_) => panic!("write"),
        }
        assert_eq!(t.write_strict(b"y"), Err(TT_ENOSPC));
    }

    #[test]
    fn gates() {
        let mut t = mk();
        let big = [b'z'; 33];
        assert_eq!(t.write(&big), Err(TT_ECHUNK));
        assert_eq!(Tee::flags("ax"), Err(TT_EFLAG));
        assert!(Tee::flags("a").map(|x| x.is_append()) == Ok(true));
        assert!(Tee::flags("").map(|x| x.is_append()) == Ok(false));
        let _ = t.add(16, 0, RW);
        let _ = t.add(16, 0, RW);
        let _ = t.add(16, 0, RW);
        assert_eq!(t.add(16, 0, RW), Err(TT_EFULL));
    }

    #[test]
    fn inject_four_is_all_true() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
