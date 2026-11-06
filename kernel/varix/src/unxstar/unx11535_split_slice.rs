//! UNX-F11535 split 切分（-b/-l/-d）
//!
//! `split` 把一个字节流按固定大小切成若干块，块名由后缀决定。两个切分轴互斥：
//! `-b N` 按字节数切，`-l N` 按行数切。二者同时给出或都不给出都属于参数冲突，
//! 必须在打开输入之前拒绝——否则实现只能靠「默认按行」之类的隐式约定，调用方
//! 永远不知道实际走的是哪条路径。
//!
//! 按字节切分是无状态的：每块恰好 N 字节，最后一块可以不足 N。按行切分则以
//! 换行符为界：一行包含它的换行符，最后一行即使没有换行符也算一行；块边界只能
//! 落在行边界上，因此最后一块的长度与输入尾部是否带换行直接相关。这个差异是
//! split 语义的核心，不能用「先按字节切再修」来近似。
//!
//! 后缀有两套字母表。默认是字母后缀：`aa`、`ab`……`az`、`ba`，按 26 进制从右
//! 向左进位；`-d` 切换为数字后缀，按 10 进制左补零。后缀宽度 `-a` 固定，宽度
//! 决定容量：数字宽度 w 只能容纳 10^w 个块，字母宽度 w 只能容纳 26^w 个块。
//! 超出容量是硬错误（`SPL_ESUF`），绝不回绕——回绕会让两个不同块同名，破坏
//! 「块名到内容」的单射。
//!
//! 输入与块数都有上限：输入超过 `INMAX` 拒绝，块数超过 `CHUNKMAX` 拒绝。上限
//! 存在的意义是让切分计划可以放在定容数组里，不需要堆；拒绝而不是截断，保证
//! 调用方看到的是「没做成」而不是「做了一半」。
//!
//! 状态机只有一次机会：`Plan::run` 计算块边界，重复调用返回 `SPL_ESTATE`；
//! `Plan::render` 必须携带与配置一致的模式码，否则 `SPL_EMODE`。
//!
//! 闸门次序：
//!
//! - `Cfg::flags`：字节/行标志冲突；
//! - `Cfg::flags`：块大小为零；
//! - `Cfg::flags`：后缀宽度越界；
//! - `Plan::open`：输入超限 → 非法字节；
//! - `Plan::run`：块数超限 → 重复运行；
//! - `Plan::suffix`：后缀容量溢出；
//! - `Plan::render`：模式不符 → 输出超限。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf280;

/// 切分轴冲突。
pub const SPL_EFLAG: u32 = SEG_BASE + 0x01;
/// 块大小为零。
pub const SPL_EZERO: u32 = SEG_BASE + 0x02;
/// 后缀宽度越界。
pub const SPL_EWIDTH: u32 = SEG_BASE + 0x03;
/// 输入、块数或输出超限。
pub const SPL_ESIZE: u32 = SEG_BASE + 0x04;
/// 非法字节。
pub const SPL_EBYTE: u32 = SEG_BASE + 0x05;
/// 后缀容量溢出。
pub const SPL_ESUF: u32 = SEG_BASE + 0x06;
/// 状态非法。
pub const SPL_ESTATE: u32 = SEG_BASE + 0x07;
/// 模式不符。
pub const SPL_EMODE: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const SPL_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const SPL_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const SPL_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const SPL_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const SPL_ENOBS: u32 = SEG_BASE + 0x0d;

/// 输入字节上限。
pub const INMAX: usize = 512;
/// 块数上限。
pub const CHUNKMAX: usize = 64;
/// 后缀宽度上限。
pub const SUFMAX: u32 = 8;
/// 输出字节上限。
pub const OUTMAX: usize = 768;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 模式：按字节。
pub const MODE_BYTE: u8 = 1;
/// 模式：按行。
pub const MODE_LINE: u8 = 2;

/// 操作码：切分。
pub const OP_SPLIT: u8 = 1;
/// 操作码：块长。
pub const OP_CHUNK: u8 = 2;
/// 操作码：后缀。
pub const OP_SUFFIX: u8 = 3;
/// 操作码：渲染。
pub const OP_RENDER: u8 = 4;

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

/// 切分参数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cfg {
    mode: u8,
    size: u32,
    width: u32,
    numeric: bool,
}

impl Cfg {
    /// 由两个互斥标志构造。
    pub fn flags(byte: bool, line: bool, size: u32, width: u32, numeric: bool) -> Result<Cfg, u32> {
        if byte == line {
            return Err(SPL_EFLAG);
        }
        if size == 0 {
            return Err(SPL_EZERO);
        }
        if width == 0 || width > SUFMAX {
            return Err(SPL_EWIDTH);
        }
        let mode = if byte { MODE_BYTE } else { MODE_LINE };
        Ok(Cfg { mode, size, width, numeric })
    }

    /// 模式。
    pub fn mode(&self) -> u8 {
        self.mode
    }

    /// 块大小（字节数或行数）。
    pub fn size(&self) -> u32 {
        self.size
    }

    /// 后缀宽度。
    pub fn width(&self) -> u32 {
        self.width
    }

    /// 是否数字后缀。
    pub fn numeric(&self) -> bool {
        self.numeric
    }
}

/// 输入字节是否允许（可见 ASCII 与换行）。
fn allowed(b: u8) -> bool {
    b == b'\n' || b == 0x20 || (0x21..=0x7e).contains(&b)
}

/// 校验输入。
fn validate(data: &[u8]) -> Result<(), u32> {
    if data.len() > INMAX {
        return Err(SPL_ESIZE);
    }
    let mut i = 0usize;
    while i < data.len() {
        match data.get(i) {
            Some(b) => {
                if !allowed(*b) {
                    return Err(SPL_EBYTE);
                }
            }
            None => return Err(SPL_EBYTE),
        }
        i += 1;
    }
    Ok(())
}

/// 后缀容量：numeric 为 10^w，否则 26^w。
fn suffix_cap(width: u32, numeric: bool) -> u64 {
    let base = if numeric { 10u64 } else { 26u64 };
    let mut cap = 1u64;
    let mut i = 0u32;
    while i < width {
        cap = cap.saturating_mul(base);
        i += 1;
    }
    cap
}

/// 生成后缀。`index` 从零开始。
pub fn suffix_at(index: u32, width: u32, numeric: bool) -> Result<String, u32> {
    if width == 0 || width > SUFMAX {
        return Err(SPL_EWIDTH);
    }
    let cap = suffix_cap(width, numeric);
    if (index as u64) >= cap {
        return Err(SPL_ESUF);
    }
    let base = if numeric { 10u32 } else { 26u32 };
    let mut buf = [b'0'; SUFMAX as usize];
    let mut n = index;
    let mut pos = width as usize;
    while pos > 0 {
        pos -= 1;
        let d = n % base;
        n /= base;
        let ch = if numeric { b'0' + (d as u8) } else { b'a' + (d as u8) };
        if let Some(cell) = buf.get_mut(pos) {
            *cell = ch;
        }
    }
    let mut out = String::new();
    let mut i = 0usize;
    while i < width as usize {
        if let Some(b) = buf.get(i) {
            out.push(*b as char);
        }
        i += 1;
    }
    Ok(out)
}

/// 切分计划（定容）。
#[derive(Clone, Copy, Debug)]
pub struct Plan<'a> {
    cfg: Cfg,
    data: &'a [u8],
    starts: [usize; CHUNKMAX],
    ends: [usize; CHUNKMAX],
    n: usize,
    ran: bool,
}

impl<'a> Plan<'a> {
    /// 打开输入。
    pub fn open(cfg: Cfg, data: &'a [u8]) -> Result<Plan<'a>, u32> {
        validate(data)?;
        Ok(Plan {
            cfg,
            data,
            starts: [0usize; CHUNKMAX],
            ends: [0usize; CHUNKMAX],
            n: 0,
            ran: false,
        })
    }

    /// 推进一个块边界。
    fn push(&mut self, start: usize, end: usize) -> Result<(), u32> {
        if self.n >= CHUNKMAX {
            return Err(SPL_ESIZE);
        }
        if let Some(cell) = self.starts.get_mut(self.n) {
            *cell = start;
        }
        if let Some(cell) = self.ends.get_mut(self.n) {
            *cell = end;
        }
        self.n += 1;
        Ok(())
    }

    /// 计算块边界（只允许一次）。
    pub fn run(&mut self) -> Result<usize, u32> {
        if self.ran {
            return Err(SPL_ESTATE);
        }
        let len = self.data.len();
        if len == 0 {
            self.ran = true;
            return Ok(0);
        }
        let step = self.cfg.size as usize;
        if self.cfg.mode == MODE_BYTE {
            let mut start = 0usize;
            while start < len {
                let end = if start.saturating_add(step) < len {
                    start.saturating_add(step)
                } else {
                    len
                };
                self.push(start, end)?;
                start = end;
            }
        } else {
            let mut start = 0usize;
            let mut lines = 0u32;
            let mut i = 0usize;
            while i < len {
                let b = match self.data.get(i) {
                    Some(v) => *v,
                    None => break,
                };
                let at_end = b == b'\n' || i.saturating_add(1) == len;
                if at_end {
                    lines = lines.saturating_add(1);
                    if lines == self.cfg.size {
                        let end = i.saturating_add(1);
                        self.push(start, end)?;
                        start = end;
                        lines = 0;
                    }
                }
                i += 1;
            }
            if start < len {
                self.push(start, len)?;
            }
        }
        self.ran = true;
        Ok(self.n)
    }

    /// 块数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否无块。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 第 i 块字节长度。
    pub fn chunk_len(&self, i: usize) -> Option<usize> {
        let s = self.starts.get(i)?;
        let e = self.ends.get(i)?;
        Some(e.saturating_sub(*s))
    }

    /// 第 i 块内容。
    pub fn chunk(&self, i: usize) -> Option<&'a [u8]> {
        let s = *self.starts.get(i)?;
        let e = *self.ends.get(i)?;
        self.data.get(s..e)
    }

    /// 第 i 块后缀。
    pub fn suffix(&self, i: usize) -> Result<String, u32> {
        if i >= self.n {
            return Err(SPL_ESUF);
        }
        suffix_at(i as u32, self.cfg.width, self.cfg.numeric)
    }

    /// 按模式渲染（块间以 `|` 相连，供自检观察）。
    pub fn render(&self, mode: u8) -> Result<String, u32> {
        if mode != self.cfg.mode {
            return Err(SPL_EMODE);
        }
        let mut out = String::new();
        let mut i = 0usize;
        while i < self.n {
            if i > 0 {
                out.push('|');
            }
            match self.chunk(i) {
                Some(c) => {
                    let mut k = 0usize;
                    while k < c.len() {
                        if let Some(b) = c.get(k) {
                            out.push(*b as char);
                        }
                        k += 1;
                    }
                }
                None => return Err(SPL_ESTATE),
            }
            if out.len() > OUTMAX {
                return Err(SPL_ESIZE);
            }
            i += 1;
        }
        Ok(out)
    }

    /// 是否已运行。
    pub fn ran(&self) -> bool {
        self.ran
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

/// `Result<usize, u32>` → 样本。
fn count_cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<String, u32>` → 样本（值为字节长度）。
fn text_cell(slot: u8, op: u8, r: Result<String, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v.len() as u32 },
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
            return Err(SPL_ESLOT);
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
            return Err(SPL_EBAL);
        }
        const VALS: [u32; 8] = [3, 2, 2, 1, 2, 3, 9, 4];
        const CODES: [u32; 6] = [
            SPL_EFLAG,
            SPL_EZERO,
            SPL_EWIDTH,
            SPL_ESIZE,
            SPL_EBYTE,
            SPL_ESUF,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(SPL_EBAL),
            };
            if s.slot as usize != i {
                return Err(SPL_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(SPL_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(SPL_EVALUE);
                        }
                    }
                    None => return Err(SPL_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(SPL_EVALUE);
                        }
                    }
                    None => return Err(SPL_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(SPL_EBAL);
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
            return Err(SPL_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let r = fresh.seal();
        if r == self.seal() {
            Ok(r)
        } else {
            Err(SPL_ESEAL)
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

/// 十四格账本：两种切分轴、尾块、后缀与六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let data: [u8; 10] = *b"abcdefghij";
    let c0 = match Cfg::flags(true, false, 4, 2, false) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let mut p0 = match Plan::open(c0, &data) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(count_cell(0, OP_SPLIT, p0.run()));

    let _ = l.register(count_cell(1, OP_CHUNK, Ok(p0.chunk_len(2).unwrap_or(0))));

    let lines: [u8; 6] = *b"a\nb\nc\n";
    let c1 = match Cfg::flags(false, true, 2, 2, false) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let mut p1 = match Plan::open(c1, &lines) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(count_cell(2, OP_SPLIT, p1.run()));

    let tail: [u8; 5] = *b"a\nb\nc";
    let c2 = match Cfg::flags(false, true, 2, 2, false) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let mut p2 = match Plan::open(c2, &tail) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = p2.run();
    let _ = l.register(count_cell(3, OP_CHUNK, Ok(p2.chunk_len(1).unwrap_or(0))));

    let _ = l.register(text_cell(4, OP_SUFFIX, suffix_at(1, 2, false)));
    let _ = l.register(text_cell(5, OP_SUFFIX, suffix_at(10, 3, true)));

    let body: [u8; 7] = *b"abcdefg";
    let c3 = match Cfg::flags(true, false, 3, 2, true) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let mut p3 = match Plan::open(c3, &body) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = p3.run();
    let _ = l.register(text_cell(6, OP_RENDER, p3.render(MODE_BYTE)));

    let _ = l.register(count_cell(7, OP_CHUNK, Ok(p0.chunk_len(0).unwrap_or(0))));

    let _ = l.register(word_cell(8, OP_SPLIT, Cfg::flags(false, false, 4, 2, false).map(|c| c.size())));

    let _ = l.register(word_cell(9, OP_SPLIT, Cfg::flags(true, false, 0, 2, false).map(|c| c.size())));

    let _ = l.register(word_cell(10, OP_SPLIT, Cfg::flags(true, false, 4, SUFMAX + 1, false).map(|c| c.size())));

    let huge = [b'a'; INMAX + 1];
    let c9 = match Cfg::flags(true, false, 4, 2, false) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(count_cell(11, OP_SPLIT, Plan::open(c9, &huge).map(|p| p.len())));

    let bad: [u8; 2] = [b'a', 0x80];
    let _ = l.register(count_cell(12, OP_SPLIT, Plan::open(c9, &bad).map(|p| p.len())));

    let _ = l.register(text_cell(13, OP_SUFFIX, suffix_at(26, 1, false)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：按字节切分与尾块。
    let data: [u8; 10] = *b"abcdefghij";
    let c = match Cfg::flags(true, false, 4, 2, false) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let mut p = match Plan::open(c, &data) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let n = match p.run() {
        Ok(v) => v,
        Err(_) => return out,
    };
    out[1] = n == 3
        && p.chunk_len(0) == Some(4)
        && p.chunk_len(1) == Some(4)
        && p.chunk_len(2) == Some(2)
        && p.chunk(1).map(|c| c == b"efgh") == Some(true);

    // 二路：按行切分、末行无换行与后缀次序。
    let lines: [u8; 6] = *b"a\nb\nc\n";
    let cl = match Cfg::flags(false, true, 2, 2, false) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let mut pl = match Plan::open(cl, &lines) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let nl = match pl.run() {
        Ok(v) => v,
        Err(_) => return out,
    };
    let twice = pl.run() == Err(SPL_ESTATE);
    let tail: [u8; 5] = *b"a\nb\nc";
    let mut pt = match Plan::open(cl, &tail) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let _ = pt.run();
    let s0 = match suffix_at(0, 2, false) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let s1 = match suffix_at(1, 2, false) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let s25 = match suffix_at(25, 2, false) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let s26 = match suffix_at(26, 2, false) {
        Ok(v) => v,
        Err(_) => return out,
    };
    out[2] = nl == 2
        && pl.chunk_len(1) == Some(2)
        && pt.chunk_len(1) == Some(1)
        && twice
        && s0.as_bytes() == b"aa"
        && s1.as_bytes() == b"ab"
        && s25.as_bytes() == b"az"
        && s26.as_bytes() == b"ba"
        && suffix_at(10, 3, true).map(|s| s.as_bytes() == b"010") == Ok(true);

    // 三路：六类拒绝与冻结重放。
    let conflict = matches!(Cfg::flags(false, false, 4, 2, false), Err(SPL_EFLAG));
    let both = matches!(Cfg::flags(true, true, 4, 2, false), Err(SPL_EFLAG));
    let zero = matches!(Cfg::flags(true, false, 0, 2, false), Err(SPL_EZERO));
    let width = matches!(Cfg::flags(true, false, 4, 0, false), Err(SPL_EWIDTH))
        && matches!(Cfg::flags(true, false, 4, SUFMAX + 1, false), Err(SPL_EWIDTH));
    let huge = [b'a'; INMAX + 1];
    let c9 = match Cfg::flags(true, false, 4, 2, false) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let size = matches!(Plan::open(c9, &huge), Err(SPL_ESIZE));
    let bad: [u8; 2] = [b'a', 0x80];
    let byte = matches!(Plan::open(c9, &bad), Err(SPL_EBYTE));
    let suf = suffix_at(26, 1, false) == Err(SPL_ESUF)
        && suffix_at(10, 1, true) == Err(SPL_ESUF)
        && suffix_at(9, 1, true).is_ok();

    let mut led = build_ledger();
    let one_freeze = led.freeze();
    let rep = led.replay();
    let two_freeze = led.freeze();
    let reg = led.register(Sample::blank());
    out[3] = conflict
        && both
        && zero
        && width
        && size
        && byte
        && suf
        && one_freeze.is_ok()
        && rep == one_freeze
        && two_freeze == Err(SPL_EBAL)
        && reg == Err(SPL_ESLOT)
        && led.is_frozen()
        && led.rejects() == 2
        && led.replay() == Err(SPL_ESEAL);

    out
}

/// 十三项判据。
pub fn run_unx11535_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11535.split_slice");

    s.add(
        "SPL01-按字节切分",
        {
            let data: [u8; 10] = *b"abcdefghij";
            let c = match Cfg::flags(true, false, 4, 2, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut p = match Plan::open(c, &data) {
                Ok(v) => v,
                Err(_) => return s,
            };
            p.run() == Ok(3)
                && p.chunk_len(0) == Some(4)
                && p.chunk_len(1) == Some(4)
                && p.chunk_len(2) == Some(2)
        },
        "static",
    );

    s.add(
        "SPL02-尾块与渲染",
        {
            let data: [u8; 7] = *b"abcdefg";
            let c = match Cfg::flags(true, false, 3, 2, true) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut p = match Plan::open(c, &data) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = p.run();
            match p.render(MODE_BYTE) {
                Ok(v) => v.as_bytes() == b"abc|def|g",
                Err(_) => false,
            }
        },
        "static",
    );

    s.add(
        "SPL03-按行切分",
        {
            let data: [u8; 6] = *b"a\nb\nc\n";
            let c = match Cfg::flags(false, true, 2, 2, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut p = match Plan::open(c, &data) {
                Ok(v) => v,
                Err(_) => return s,
            };
            p.run() == Ok(2)
                && p.chunk(0).map(|c| c == b"a\nb\n") == Some(true)
                && p.chunk(1).map(|c| c == b"c\n") == Some(true)
        },
        "static",
    );

    s.add(
        "SPL04-末行无换行",
        {
            let data: [u8; 5] = *b"a\nb\nc";
            let c = match Cfg::flags(false, true, 2, 2, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut p = match Plan::open(c, &data) {
                Ok(v) => v,
                Err(_) => return s,
            };
            p.run() == Ok(2)
                && p.chunk_len(1) == Some(1)
                && p.chunk(1).map(|c| c == b"c") == Some(true)
        },
        "static",
    );

    s.add(
        "SPL05-字母后缀进位",
        {
            let a = suffix_at(0, 2, false);
            let b = suffix_at(1, 2, false);
            let z = suffix_at(25, 2, false);
            let c = suffix_at(26, 2, false);
            match (a, b, z, c) {
                (Ok(a), Ok(b), Ok(z), Ok(c)) => {
                    a.as_bytes() == b"aa"
                        && b.as_bytes() == b"ab"
                        && z.as_bytes() == b"az"
                        && c.as_bytes() == b"ba"
                }
                _ => false,
            }
        },
        "static",
    );

    s.add(
        "SPL06-数字后缀补零",
        {
            let a = suffix_at(0, 3, true);
            let b = suffix_at(10, 3, true);
            let c = suffix_at(999, 3, true);
            match (a, b, c) {
                (Ok(a), Ok(b), Ok(c)) => {
                    a.as_bytes() == b"000"
                        && b.as_bytes() == b"010"
                        && c.as_bytes() == b"999"
                }
                _ => false,
            }
        },
        "static",
    );

    s.add(
        "SPL07-空输入与单块",
        {
            let empty: [u8; 0] = [];
            let one: [u8; 3] = *b"abc";
            let c = match Cfg::flags(true, false, 4, 2, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut pe = match Plan::open(c, &empty) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut po = match Plan::open(c, &one) {
                Ok(v) => v,
                Err(_) => return s,
            };
            pe.run() == Ok(0)
                && pe.is_empty()
                && po.run() == Ok(1)
                && po.chunk_len(0) == Some(3)
                && !po.is_empty()
        },
        "static",
    );

    s.add(
        "SPL08-参数闸门",
        {
            matches!(Cfg::flags(false, false, 4, 2, false), Err(SPL_EFLAG))
                && matches!(Cfg::flags(true, true, 4, 2, false), Err(SPL_EFLAG))
                && matches!(Cfg::flags(true, false, 0, 2, false), Err(SPL_EZERO))
                && matches!(Cfg::flags(false, true, 1, 0, false), Err(SPL_EWIDTH))
                && matches!(Cfg::flags(false, true, 1, SUFMAX + 1, false), Err(SPL_EWIDTH))
                && Cfg::flags(true, false, 1, SUFMAX, true).is_ok()
        },
        "static",
    );

    s.add(
        "SPL09-非法字节与超长",
        {
            let huge = [b'a'; INMAX + 1];
            let bad: [u8; 3] = [b'a', 0x00, b'b'];
            let high: [u8; 2] = [b'a', 0x80];
            let c = match Cfg::flags(true, false, 4, 2, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            matches!(Plan::open(c, &huge), Err(SPL_ESIZE))
                && matches!(Plan::open(c, &bad), Err(SPL_EBYTE))
                && matches!(Plan::open(c, &high), Err(SPL_EBYTE))
        },
        "static",
    );

    s.add(
        "SPL10-后缀溢出与状态",
        {
            let data: [u8; 10] = *b"abcdefghij";
            let c = match Cfg::flags(true, false, 4, 1, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut p = match Plan::open(c, &data) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let first = p.run() == Ok(3);
            let twice = p.run() == Err(SPL_ESTATE);
            let bad_mode = p.render(MODE_LINE) == Err(SPL_EMODE);
            let suf = suffix_at(26, 1, false) == Err(SPL_ESUF)
                && suffix_at(10, 1, true) == Err(SPL_ESUF)
                && suffix_at(25, 1, false).is_ok();
            first && twice && bad_mode && suf && p.ran()
        },
        "static",
    );

    s.add(
        "SPL11-账本审计",
        {
            let l = build_ledger();
            l.len() == SAMPLES
                && l.ok_count() == 8
                && l.err_count() == 6
                && l.audit().is_ok()
                && l.rejects() == 0
                && l.sample(0).map(|x| x.value) == Some(3)
                && l.sample(13).map(|x| x.code) == Some(SPL_ESUF)
        },
        "static",
    );

    s.add(
        "SPL12-冻结与重放",
        {
            let mut l = build_ledger();
            let a = l.freeze();
            let b = l.freeze();
            let c = l.register(Sample::blank());
            let sealed = l.seal();
            l.is_frozen()
                && a.is_ok()
                && b == Err(SPL_EBAL)
                && c == Err(SPL_ESLOT)
                && l.rejects() == 2
                && l.replay() == Err(SPL_ESEAL)
                && sealed != 0
        },
        "static",
    );

    s.add(
        "SPL13-冻结重放四注入",
        inject_four() == [true; 4] && build_ledger().audit().is_ok(),
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(byte: bool, size: u32, width: u32) -> Cfg {
        match Cfg::flags(byte, !byte, size, width, false) {
            Ok(v) => v,
            Err(_) => Cfg::flags(true, false, 4, 2, false).ok().unwrap(),
        }
    }

    #[test]
    fn byte_split() {
        let data: [u8; 10] = *b"abcdefghij";
        let mut p = Plan::open(cfg(true, 4, 2), &data).ok().unwrap();
        assert_eq!(p.run(), Ok(3));
        assert_eq!(p.chunk_len(0), Some(4));
        assert_eq!(p.chunk_len(2), Some(2));
        assert_eq!(p.chunk(1).map(|c| c == b"efgh"), Some(true));
    }

    #[test]
    fn line_split() {
        let data: [u8; 6] = *b"a\nb\nc\n";
        let mut p = Plan::open(cfg(false, 2, 2), &data).ok().unwrap();
        assert_eq!(p.run(), Ok(2));
        assert_eq!(p.chunk(0).map(|c| c == b"a\nb\n"), Some(true));
        let tail: [u8; 5] = *b"a\nb\nc";
        let mut q = Plan::open(cfg(false, 2, 2), &tail).ok().unwrap();
        assert_eq!(q.run(), Ok(2));
        assert_eq!(q.chunk_len(1), Some(1));
    }

    #[test]
    fn suffixes() {
        assert_eq!(suffix_at(1, 2, false).map(|s| s.len()), Ok(2));
        assert_eq!(suffix_at(10, 3, true).map(|s| s.len()), Ok(3));
        assert_eq!(suffix_at(26, 1, false), Err(SPL_ESUF));
        assert_eq!(suffix_at(10, 1, true), Err(SPL_ESUF));
        assert_eq!(suffix_at(9, 1, true).map(|s| s.len()), Ok(1));
    }

    #[test]
    fn gates() {
        assert_eq!(Cfg::flags(false, false, 4, 2, false), Err(SPL_EFLAG));
        assert_eq!(Cfg::flags(true, true, 4, 2, false), Err(SPL_EFLAG));
        assert_eq!(Cfg::flags(true, false, 0, 2, false), Err(SPL_EZERO));
        assert_eq!(Cfg::flags(true, false, 4, 0, false), Err(SPL_EWIDTH));
    }

    #[test]
    fn rejections() {
        let huge = [b'a'; INMAX + 1];
        let bad: [u8; 2] = [b'a', 0x80];
        let c = cfg(true, 4, 2);
        assert!(matches!(Plan::open(c, &huge), Err(SPL_ESIZE)));
        assert!(matches!(Plan::open(c, &bad), Err(SPL_EBYTE)));
    }

    #[test]
    fn state_and_render() {
        let data: [u8; 7] = *b"abcdefg";
        let mut p = Plan::open(cfg(true, 3, 2), &data).ok().unwrap();
        assert_eq!(p.run(), Ok(3));
        assert_eq!(p.run(), Err(SPL_ESTATE));
        assert_eq!(p.render(MODE_LINE), Err(SPL_EMODE));
        match p.render(MODE_BYTE) {
            Ok(v) => assert!(v.as_bytes() == b"abc|def|g"),
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn ledger_cycle() {
        let mut l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        let r = match l.freeze() {
            Ok(v) => v,
            Err(_) => 0,
        };
        assert!(r != 0);
        assert_eq!(l.replay(), Ok(r));
        assert_eq!(l.freeze(), Err(SPL_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(SPL_ESLOT));
        assert_eq!(l.rejects(), 2);
        assert_eq!(l.replay(), Err(SPL_ESEAL));
        assert_eq!(inject_four(), [true; 4]);
    }
}
