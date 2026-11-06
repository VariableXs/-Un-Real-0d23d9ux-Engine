//! UNX-F11536 csplit 上下文切分
//!
//! `csplit` 不按固定大小切分，而是按「上下文」切：行号模式在指定行之前断开，
//! 字面量模式在第一条包含该字面量的行之前断开。每条模式还可以带一个以行为单位
//! 的偏移量，把断点向前或向后挪动若干行；重复次数则让同一条模式连续生效多次。
//!
//! 断点是一个单调递增的行号序列。序列以 0 开头，以总行数结尾，中间每个断点都
//! 必须严格大于前一个、严格小于等于总行数——等于总行数只允许出现在末尾。任何
//! 重复断点、倒退断点、越过末尾的断点都是错误（`CSI_EOFF`），因为段区间一旦
//! 出现空集或负长度，后续所有段的编号都会漂移。
//!
//! 行号模式是绝对定位：`2` 表示在第 2 行之前断开；重复时每次把目标再推进
//! 「行号步长」那么多行：`1{3}` 依次在 1、2、3 行前断开。字面量模式是相对定位：
//! 从上次命中的下一行继续找，找到后断点落在命中行的行首，再叠加偏移。重复次数
//! 用尽之前如果找不到匹配，就是 `CSI_ENOMATCH`，不做「找不到就切到末尾」的
//! 静默兜底。
//!
//! 参数闸门先于数据闸门：非法模式（空字面量、未知种类）不必等到读入数据就能
//! 拒绝；行号为零、重复为零、偏移越界同理。数据侧只检查两件事：总字节数上限
//! 与模式数量上限。两者都用 `CSI_EFULL` 表达，因为它们的后果相同——计划放不进
//! 定容表。
//!
//! 输出有两种观察模式：`MODE_LINE` 打印断点行号序列，`MODE_LIT` 打印各段字节
//! 长度。模式码不匹配返回 `CSI_EMODE`；`run` 只允许一次，第二次返回
//! `CSI_ESTATE`。这两种闸门存在的目的是让自检能区分「算错了」与「调用顺序错了」。
//!
//! 闸门次序：
//!
//! - `Pat::line` / `Pat::lit`：构造不校验；
//! - `Csplit::open`：字节上限 → 模式数上限 → 种类/行号 → 重复 → 偏移 → 字面量；
//! - `Csplit::run`：重复运行 → 未命中 → 断点越界；
//! - `Csplit::render`：模式不符。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf290;

/// 非法模式。
pub const CSI_EPAT: u32 = SEG_BASE + 0x01;
/// 行号非法。
pub const CSI_ELINE: u32 = SEG_BASE + 0x02;
/// 重复次数非法。
pub const CSI_EREP: u32 = SEG_BASE + 0x03;
/// 字面量未命中。
pub const CSI_ENOMATCH: u32 = SEG_BASE + 0x04;
/// 断点或偏移越界。
pub const CSI_EOFF: u32 = SEG_BASE + 0x05;
/// 数据或模式表已满。
pub const CSI_EFULL: u32 = SEG_BASE + 0x06;
/// 状态非法。
pub const CSI_ESTATE: u32 = SEG_BASE + 0x07;
/// 模式不符。
pub const CSI_EMODE: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const CSI_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const CSI_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const CSI_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const CSI_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const CSI_ENOBS: u32 = SEG_BASE + 0x0d;

/// 输入字节上限。
pub const INMAX: usize = 512;
/// 模式数量上限。
pub const PATMAX: usize = 8;
/// 行数上限。
pub const LINEMAX: usize = 64;
/// 重复次数上限。
pub const REPMAX: u32 = 8;
/// 偏移绝对值上限。
pub const OFFMAX: i32 = 16;
/// 段数上限。
pub const SECMAX: usize = 32;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 模式：行号。
pub const KIND_LINE: u8 = 1;
/// 模式：字面量。
pub const KIND_LIT: u8 = 2;
/// 观察模式：断点行号。
pub const MODE_LINE: u8 = 1;
/// 观察模式：段长。
pub const MODE_LIT: u8 = 2;

/// 操作码：切分。
pub const OP_CSPLIT: u8 = 1;
/// 操作码：段长。
pub const OP_SEC: u8 = 2;
/// 操作码：断点。
pub const OP_BOUND: u8 = 3;

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

/// 一条上下文模式。
#[derive(Clone, Copy, Debug)]
pub struct Pat<'a> {
    kind: u8,
    num: u32,
    off: i32,
    rep: u32,
    lit: &'a [u8],
}

impl<'a> Pat<'a> {
    /// 行号模式。
    pub fn line(num: u32, off: i32, rep: u32) -> Pat<'a> {
        Pat { kind: KIND_LINE, num, off, rep, lit: &[] }
    }

    /// 字面量模式。
    pub fn lit(lit: &'a [u8], off: i32, rep: u32) -> Pat<'a> {
        Pat { kind: KIND_LIT, num: 0, off, rep, lit }
    }
}

/// 字节是否可打印。
fn printable(b: u8) -> bool {
    b == 0x20 || (0x21..=0x7e).contains(&b)
}

/// 校验字面量。
fn check_lit(lit: &[u8]) -> Result<(), u32> {
    if lit.is_empty() {
        return Err(CSI_EPAT);
    }
    let mut i = 0usize;
    while i < lit.len() {
        match lit.get(i) {
            Some(b) => {
                if !printable(*b) {
                    return Err(CSI_EPAT);
                }
            }
            None => return Err(CSI_EPAT),
        }
        i += 1;
    }
    Ok(())
}

/// 子串包含。
fn contains(hay: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || needle.len() > hay.len() {
        return false;
    }
    let mut i = 0usize;
    while i + needle.len() <= hay.len() {
        let mut k = 0usize;
        let mut hit = true;
        while k < needle.len() {
            let a = match hay.get(i + k) {
                Some(v) => *v,
                None => {
                    hit = false;
                    break;
                }
            };
            let b = match needle.get(k) {
                Some(v) => *v,
                None => {
                    hit = false;
                    break;
                }
            };
            if a != b {
                hit = false;
                break;
            }
            k += 1;
        }
        if hit {
            return true;
        }
        i += 1;
    }
    false
}

/// 十进制压入。
fn push_dec(out: &mut String, mut v: u32) {
    let mut buf = [0u8; 10];
    let mut n = 0usize;
    if v == 0 {
        out.push('0');
        return;
    }
    while v > 0 {
        let d = (v % 10) as u8;
        if let Some(cell) = buf.get_mut(n) {
            *cell = b'0' + d;
        }
        n += 1;
        v /= 10;
    }
    while n > 0 {
        n -= 1;
        if let Some(b) = buf.get(n) {
            out.push(*b as char);
        }
    }
}

/// 上下文切分计划。
#[derive(Clone, Copy, Debug)]
pub struct Csplit<'a> {
    data: &'a [u8],
    pats: [Pat<'a>; PATMAX],
    np: usize,
    starts: [usize; LINEMAX],
    lines: usize,
    bounds: [usize; SECMAX],
    nb: usize,
    ran: bool,
}

impl<'a> Csplit<'a> {
    /// 打开数据与模式表。
    pub fn open(data: &'a [u8], pats: &[Pat<'a>]) -> Result<Csplit<'a>, u32> {
        if data.len() > INMAX {
            return Err(CSI_EFULL);
        }
        if pats.len() > PATMAX {
            return Err(CSI_EFULL);
        }
        let mut tab: [Pat<'a>; PATMAX] = [Pat::line(0, 0, 0); PATMAX];
        let mut i = 0usize;
        while i < pats.len() {
            let p = match pats.get(i) {
                Some(v) => *v,
                None => return Err(CSI_EFULL),
            };
            if p.rep == 0 || p.rep > REPMAX {
                return Err(CSI_EREP);
            }
            if p.off < -OFFMAX || p.off > OFFMAX {
                return Err(CSI_EOFF);
            }
            if p.kind == KIND_LINE {
                if p.num == 0 || p.num as usize > LINEMAX {
                    return Err(CSI_ELINE);
                }
            } else if p.kind == KIND_LIT {
                check_lit(p.lit)?;
            } else {
                return Err(CSI_EPAT);
            }
            if let Some(dst) = tab.get_mut(i) {
                *dst = p;
            }
            i += 1;
        }
        let mut starts = [0usize; LINEMAX];
        let mut lines = 0usize;
        if !data.is_empty() {
            lines = 1;
            let mut k = 0usize;
            while k < data.len() {
                match data.get(k) {
                    Some(b) if *b == b'\n' => {
                        if k + 1 < data.len() {
                            if lines >= LINEMAX {
                                return Err(CSI_EFULL);
                            }
                            if let Some(cell) = starts.get_mut(lines) {
                                *cell = k + 1;
                            }
                            lines += 1;
                        }
                    }
                    Some(_) => {}
                    None => break,
                }
                k += 1;
            }
        }
        Ok(Csplit { data, pats: tab, np: pats.len(), starts, lines, bounds: [0usize; SECMAX], nb: 1, ran: false })
    }

    /// 行数。
    pub fn lines(&self) -> usize {
        self.lines
    }

    /// 某个行号对应的字节偏移。
    fn line_off(&self, k: usize) -> usize {
        if k < self.lines {
            match self.starts.get(k) {
                Some(v) => *v,
                None => self.data.len(),
            }
        } else {
            self.data.len()
        }
    }

    /// 行切片。
    fn line_slice(&self, k: usize) -> Option<&'a [u8]> {
        if k >= self.lines {
            return None;
        }
        let s = self.line_off(k);
        let e = self.line_off(k + 1);
        self.data.get(s..e)
    }

    /// 追加一个断点。
    fn bound(&mut self, target: i64) -> Result<(), u32> {
        if target < 0 || target as usize > self.lines {
            return Err(CSI_EOFF);
        }
        let t = target as usize;
        let last = match self.bounds.get(self.nb.saturating_sub(1)) {
            Some(v) => *v,
            None => 0,
        };
        if t <= last || self.nb >= SECMAX {
            return Err(CSI_EOFF);
        }
        if let Some(cell) = self.bounds.get_mut(self.nb) {
            *cell = t;
        }
        self.nb += 1;
        Ok(())
    }

    /// 从第 `from` 行开始找字面量。
    fn find_from(&self, lit: &[u8], from: usize) -> Option<usize> {
        let mut i = from;
        while i < self.lines {
            if let Some(line) = self.line_slice(i) {
                if contains(line, lit) {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 执行切分（只允许一次）。
    pub fn run(&mut self) -> Result<usize, u32> {
        if self.ran {
            return Err(CSI_ESTATE);
        }
        let mut pi = 0usize;
        while pi < self.np {
            let p = match self.pats.get(pi) {
                Some(v) => *v,
                None => return Err(CSI_EFULL),
            };
            if p.kind == KIND_LINE {
                let base = p.num as i64 + p.off as i64;
                let mut k = 0u32;
                while k < p.rep {
                    let target = base + (k as i64).saturating_mul(p.num as i64);
                    self.bound(target)?;
                    k += 1;
                }
            } else {
                let mut cursor = 0usize;
                let mut k = 0u32;
                while k < p.rep {
                    let hit = match self.find_from(p.lit, cursor) {
                        Some(v) => v,
                        None => return Err(CSI_ENOMATCH),
                    };
                    let target = hit as i64 + p.off as i64;
                    self.bound(target)?;
                    cursor = hit.saturating_add(1);
                    k += 1;
                }
            }
            pi += 1;
        }
        if self.lines > 0 {
            let last = match self.bounds.get(self.nb.saturating_sub(1)) {
                Some(v) => *v,
                None => 0,
            };
            if last < self.lines {
                if self.nb < SECMAX {
                    if let Some(cell) = self.bounds.get_mut(self.nb) {
                        *cell = self.lines;
                    }
                    self.nb += 1;
                } else {
                    return Err(CSI_EFULL);
                }
            }
        }
        self.ran = true;
        Ok(self.nb.saturating_sub(1))
    }

    /// 段数。
    pub fn len(&self) -> usize {
        self.nb.saturating_sub(1)
    }

    /// 是否无段。
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 第 i 个断点（行号）。
    pub fn boundary(&self, i: usize) -> Option<usize> {
        if i >= self.nb {
            return None;
        }
        self.bounds.get(i).copied()
    }

    /// 第 i 段字节。
    pub fn section(&self, i: usize) -> Option<&'a [u8]> {
        let a = *self.bounds.get(i)?;
        let b = *self.bounds.get(i + 1)?;
        let s = self.line_off(a);
        let e = self.line_off(b);
        self.data.get(s..e)
    }

    /// 第 i 段长度。
    pub fn section_len(&self, i: usize) -> Option<usize> {
        self.section(i).map(|v| v.len())
    }

    /// 按观察模式渲染。
    pub fn render(&self, mode: u8) -> Result<String, u32> {
        let mut out = String::new();
        if mode == MODE_LINE {
            let mut i = 0usize;
            while i < self.nb {
                if i > 0 {
                    out.push(',');
                }
                if let Some(b) = self.bounds.get(i) {
                    push_dec(&mut out, *b as u32);
                }
                i += 1;
            }
            return Ok(out);
        }
        if mode == MODE_LIT {
            let mut i = 0usize;
            while i + 1 < self.nb {
                if i > 0 {
                    out.push('|');
                }
                match self.section_len(i) {
                    Some(n) => push_dec(&mut out, n as u32),
                    None => return Err(CSI_EOFF),
                }
                i += 1;
            }
            return Ok(out);
        }
        Err(CSI_EMODE)
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
            return Err(CSI_ESLOT);
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
            return Err(CSI_EBAL);
        }
        const VALS: [u32; 8] = [2, 3, 2, 2, 4, 12, 2, 11];
        const CODES: [u32; 6] = [
            CSI_ELINE,
            CSI_EREP,
            CSI_ENOMATCH,
            CSI_EOFF,
            CSI_EFULL,
            CSI_EPAT,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(CSI_EBAL),
            };
            if s.slot as usize != i {
                return Err(CSI_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(CSI_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(CSI_EVALUE);
                        }
                    }
                    None => return Err(CSI_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(CSI_EVALUE);
                        }
                    }
                    None => return Err(CSI_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(CSI_EBAL);
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
            return Err(CSI_EBAL);
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
            Err(CSI_ESEAL)
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

/// 十四格账本：行号、字面量、偏移、重复与六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let data: [u8; 23] = *b"alpha\nbeta\ngamma\ndelta\n";

    let p0 = [Pat::line(2, 0, 1)];
    let mut c0 = match Csplit::open(&data, &p0) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(count_cell(0, OP_CSPLIT, c0.run()));

    let p1 = [Pat::line(2, 0, 1), Pat::line(3, 0, 1)];
    let mut c1 = match Csplit::open(&data, &p1) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(count_cell(1, OP_CSPLIT, c1.run()));

    let p2 = [Pat::lit(b"gamma", 0, 1)];
    let mut c2 = match Csplit::open(&data, &p2) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(count_cell(2, OP_CSPLIT, c2.run()));

    let p3 = [Pat::lit(b"ta", 0, 1)];
    let mut c3 = match Csplit::open(&data, &p3) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(count_cell(3, OP_CSPLIT, c3.run()));

    let p4 = [Pat::line(1, 0, 3)];
    let mut c4 = match Csplit::open(&data, &p4) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(count_cell(4, OP_CSPLIT, c4.run()));

    let _ = l.register(count_cell(5, OP_SEC, c0.section_len(1).ok_or(CSI_EOFF)));

    let p6 = [Pat::line(2, 1, 1)];
    let mut c6 = match Csplit::open(&data, &p6) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(count_cell(6, OP_CSPLIT, c6.run()));

    let p7 = [Pat::lit(b"ta", 1, 1)];
    let mut c7 = match Csplit::open(&data, &p7) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(count_cell(7, OP_SEC, {
        let _ = c7.run();
        c7.section_len(0).ok_or(CSI_EOFF)
    }));

    let p8 = [Pat::line(0, 0, 1)];
    let _ = l.register(count_cell(8, OP_CSPLIT, Csplit::open(&data, &p8).map(|c| c.lines())));

    let p9 = [Pat::line(1, 0, 0)];
    let _ = l.register(count_cell(9, OP_CSPLIT, Csplit::open(&data, &p9).map(|c| c.lines())));

    let p10 = [Pat::lit(b"zeta", 0, 1)];
    let _ = l.register(count_cell(10, OP_CSPLIT, {
        match Csplit::open(&data, &p10) {
            Ok(mut c) => c.run(),
            Err(e) => Err(e),
        }
    }));

    let p11 = [Pat::line(2, OFFMAX + 1, 1)];
    let _ = l.register(count_cell(11, OP_CSPLIT, Csplit::open(&data, &p11).map(|c| c.lines())));

    let huge = [b'a'; INMAX + 1];
    let p12 = [Pat::line(1, 0, 1)];
    let _ = l.register(count_cell(12, OP_CSPLIT, Csplit::open(&huge, &p12).map(|c| c.lines())));

    let p13 = [Pat::lit(b"", 0, 1)];
    let _ = l.register(count_cell(13, OP_CSPLIT, Csplit::open(&data, &p13).map(|c| c.lines())));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：行号切分与段内容。
    let data: [u8; 23] = *b"alpha\nbeta\ngamma\ndelta\n";
    let p = [Pat::line(2, 0, 1)];
    let mut c = match Csplit::open(&data, &p) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let n = match c.run() {
        Ok(v) => v,
        Err(_) => return out,
    };
    out[1] = n == 2
        && c.len() == 2
        && c.boundary(0) == Some(0)
        && c.boundary(1) == Some(2)
        && c.boundary(2) == Some(4)
        && c.section(0).map(|s| s == b"alpha\nbeta\n") == Some(true)
        && c.section_len(1) == Some(12);

    // 二路：字面量、偏移、重复与渲染往返。
    let p2 = [Pat::lit(b"ta", 0, 1)];
    let mut c2 = match Csplit::open(&data, &p2) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let n2 = match c2.run() {
        Ok(v) => v,
        Err(_) => return out,
    };
    let twice = c2.run() == Err(CSI_ESTATE);
    let p3 = [Pat::line(1, 0, 3)];
    let mut c3 = match Csplit::open(&data, &p3) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let n3 = match c3.run() {
        Ok(v) => v,
        Err(_) => return out,
    };
    let off = [Pat::line(2, 1, 1)];
    let mut co = match Csplit::open(&data, &off) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let _ = co.run();
    out[2] = n2 == 2
        && twice
        && n3 == 4
        && c3.boundary(3) == Some(3)
        && co.boundary(1) == Some(3)
        && c2.render(MODE_LINE).map(|s| s.as_bytes() == b"0,1,4") == Ok(true)
        && c2.render(MODE_LIT).map(|s| s.as_bytes() == b"6|17") == Ok(true)
        && c2.render(9) == Err(CSI_EMODE);

    // 三路：六类拒绝与冻结重放。
    let pzero = [Pat::line(0, 0, 1)];
    let line = matches!(Csplit::open(&data, &pzero), Err(CSI_ELINE));
    let prep = [Pat::line(1, 0, 0)];
    let erep = matches!(Csplit::open(&data, &prep), Err(CSI_EREP));
    let pnone = [Pat::lit(b"zeta", 0, 1)];
    let nomatch = match Csplit::open(&data, &pnone) {
        Ok(mut c) => c.run() == Err(CSI_ENOMATCH),
        Err(_) => false,
    };
    let poff = [Pat::line(2, OFFMAX + 1, 1)];
    let off = matches!(Csplit::open(&data, &poff), Err(CSI_EOFF));
    let huge = [b'a'; INMAX + 1];
    let pl = [Pat::line(1, 0, 1)];
    let full = matches!(Csplit::open(&huge, &pl), Err(CSI_EFULL));
    let pempty = [Pat::lit(b"", 0, 1)];
    let pat = matches!(Csplit::open(&data, &pempty), Err(CSI_EPAT));

    let mut led = build_ledger();
    let one_freeze = led.freeze();
    let rp = led.replay();
    let two_freeze = led.freeze();
    let reg = led.register(Sample::blank());
    out[3] = line
        && erep
        && nomatch
        && off
        && full
        && pat
        && one_freeze.is_ok()
        && rp == one_freeze
        && two_freeze == Err(CSI_EBAL)
        && reg == Err(CSI_ESLOT)
        && led.is_frozen()
        && led.rejects() == 2
        && led.replay() == Err(CSI_ESEAL);

    out
}

/// 十三项判据。
pub fn run_unx11536_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11536.csplit_context");

    s.add(
        "CSI01-行号切分",
        {
            let data: [u8; 23] = *b"alpha\nbeta\ngamma\ndelta\n";
            let p = [Pat::line(2, 0, 1)];
            let mut c = match Csplit::open(&data, &p) {
                Ok(v) => v,
                Err(_) => return s,
            };
            c.run() == Ok(2)
                && c.boundary(0) == Some(0)
                && c.boundary(1) == Some(2)
                && c.boundary(2) == Some(4)
        },
        "static",
    );

    s.add(
        "CSI02-多行断点",
        {
            let data: [u8; 23] = *b"alpha\nbeta\ngamma\ndelta\n";
            let p = [Pat::line(2, 0, 1), Pat::line(3, 0, 1)];
            let mut c = match Csplit::open(&data, &p) {
                Ok(v) => v,
                Err(_) => return s,
            };
            c.run() == Ok(3)
                && c.section(1).map(|x| x == b"gamma\n") == Some(true)
                && c.section(2).map(|x| x == b"delta\n") == Some(true)
        },
        "static",
    );

    s.add(
        "CSI03-字面量切分",
        {
            let data: [u8; 23] = *b"alpha\nbeta\ngamma\ndelta\n";
            let p = [Pat::lit(b"gamma", 0, 1)];
            let mut c = match Csplit::open(&data, &p) {
                Ok(v) => v,
                Err(_) => return s,
            };
            c.run() == Ok(2)
                && c.boundary(1) == Some(2)
                && c.section(1).map(|x| x == b"gamma\ndelta\n") == Some(true)
        },
        "static",
    );

    s.add(
        "CSI04-子串命中",
        {
            let data: [u8; 23] = *b"alpha\nbeta\ngamma\ndelta\n";
            let p = [Pat::lit(b"ta", 0, 1)];
            let mut c = match Csplit::open(&data, &p) {
                Ok(v) => v,
                Err(_) => return s,
            };
            c.run() == Ok(2)
                && c.boundary(1) == Some(1)
                && c.section(0).map(|x| x == b"alpha\n") == Some(true)
        },
        "static",
    );

    s.add(
        "CSI05-重复次数",
        {
            let data: [u8; 23] = *b"alpha\nbeta\ngamma\ndelta\n";
            let p = [Pat::line(1, 0, 3)];
            let mut c = match Csplit::open(&data, &p) {
                Ok(v) => v,
                Err(_) => return s,
            };
            c.run() == Ok(4)
                && c.boundary(1) == Some(1)
                && c.boundary(2) == Some(2)
                && c.boundary(3) == Some(3)
                && c.section_len(0) == Some(6)
        },
        "static",
    );

    s.add(
        "CSI06-偏移修正",
        {
            let data: [u8; 23] = *b"alpha\nbeta\ngamma\ndelta\n";
            let p = [Pat::line(2, 1, 1)];
            let mut c = match Csplit::open(&data, &p) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let a = c.run() == Ok(2);
            let q = [Pat::lit(b"ta", 1, 1)];
            let mut d = match Csplit::open(&data, &q) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a && c.boundary(1) == Some(3) && d.run() == Ok(2) && d.boundary(1) == Some(2)
        },
        "static",
    );

    s.add(
        "CSI07-段长与渲染",
        {
            let data: [u8; 23] = *b"alpha\nbeta\ngamma\ndelta\n";
            let p = [Pat::line(2, 0, 1)];
            let mut c = match Csplit::open(&data, &p) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = c.run();
            let a = c.render(MODE_LINE);
            let b = c.render(MODE_LIT);
            match (a, b) {
                (Ok(a), Ok(b)) => {
                    a.as_bytes() == b"0,2,4"
                        && b.as_bytes() == b"11|12"
                        && c.section_len(0) == Some(11)
                        && c.section_len(1) == Some(12)
                }
                _ => false,
            }
        },
        "static",
    );

    s.add(
        "CSI08-参数闸门",
        {
            let data: [u8; 23] = *b"alpha\nbeta\ngamma\ndelta\n";
            matches!(Csplit::open(&data, &[Pat::line(0, 0, 1)]), Err(CSI_ELINE))
                && matches!(Csplit::open(&data, &[Pat::line(1, 0, 0)]), Err(CSI_EREP))
                && matches!(Csplit::open(&data, &[Pat::line(1, 0, REPMAX + 1)]), Err(CSI_EREP))
                && matches!(Csplit::open(&data, &[Pat::line(1, OFFMAX + 1, 1)]), Err(CSI_EOFF))
                && matches!(Csplit::open(&data, &[Pat::lit(b"", 0, 1)]), Err(CSI_EPAT))
        },
        "static",
    );

    s.add(
        "CSI09-未命中拒绝",
        {
            let data: [u8; 23] = *b"alpha\nbeta\ngamma\ndelta\n";
            let p = [Pat::lit(b"zeta", 0, 1)];
            let mut c = match Csplit::open(&data, &p) {
                Ok(v) => v,
                Err(_) => return s,
            };
            c.run() == Err(CSI_ENOMATCH) && !c.ran()
        },
        "static",
    );

    s.add(
        "CSI10-重复运行与模式",
        {
            let data: [u8; 23] = *b"alpha\nbeta\ngamma\ndelta\n";
            let p = [Pat::line(2, 0, 1)];
            let mut c = match Csplit::open(&data, &p) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let first = c.run() == Ok(2);
            let twice = c.run() == Err(CSI_ESTATE);
            let bad = c.render(9) == Err(CSI_EMODE);
            first && twice && bad && c.ran() && !c.is_empty()
        },
        "static",
    );

    s.add(
        "CSI11-账本审计",
        {
            let l = build_ledger();
            l.len() == SAMPLES
                && l.ok_count() == 8
                && l.err_count() == 6
                && l.audit().is_ok()
                && l.rejects() == 0
                && l.sample(0).map(|x| x.value) == Some(2)
                && l.sample(5).map(|x| x.value) == Some(12)
        },
        "static",
    );

    s.add(
        "CSI12-冻结与重放",
        {
            let mut l = build_ledger();
            let a = l.freeze();
            let b = l.freeze();
            let c = l.register(Sample::blank());
            let sealed = l.seal();
            l.is_frozen()
                && a.is_ok()
                && b == Err(CSI_EBAL)
                && c == Err(CSI_ESLOT)
                && l.rejects() == 2
                && l.replay() == Err(CSI_ESEAL)
                && sealed != 0
        },
        "static",
    );

    s.add(
        "CSI13-冻结重放四注入",
        inject_four() == [true; 4] && build_ledger().audit().is_ok(),
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const DATA: [u8; 23] = *b"alpha\nbeta\ngamma\ndelta\n";

    fn open(p: &[Pat<'_>]) -> Csplit<'_> {
        match Csplit::open(&DATA, p) {
            Ok(v) => v,
            Err(_) => Csplit::open(&DATA, &[Pat::line(1, 0, 1)]).ok().unwrap(),
        }
    }

    #[test]
    fn line_mode() {
        let mut c = open(&[Pat::line(2, 0, 1)]);
        assert_eq!(c.run(), Ok(2));
        assert_eq!(c.boundary(1), Some(2));
        assert_eq!(c.section_len(1), Some(12));
    }

    #[test]
    fn lit_mode() {
        let mut c = open(&[Pat::lit(b"gamma", 0, 1)]);
        assert_eq!(c.run(), Ok(2));
        assert_eq!(c.boundary(1), Some(2));
    }

    #[test]
    fn repeat_and_offset() {
        let mut c = open(&[Pat::line(1, 0, 3)]);
        assert_eq!(c.run(), Ok(4));
        assert_eq!(c.boundary(3), Some(3));
        let mut d = open(&[Pat::line(2, 1, 1)]);
        assert_eq!(d.run(), Ok(3));
        assert_eq!(d.boundary(1), Some(3));
    }

    #[test]
    fn render_modes() {
        let mut c = open(&[Pat::line(2, 0, 1)]);
        assert_eq!(c.run(), Ok(2));
        assert_eq!(c.render(MODE_LINE).map(|v| v.len()), Ok(5));
        assert_eq!(c.render(MODE_LIT).map(|v| v.len()), Ok(5));
        assert_eq!(c.render(9), Err(CSI_EMODE));
    }

    #[test]
    fn gates() {
        assert!(matches!(Csplit::open(&DATA, &[Pat::line(0, 0, 1)]), Err(CSI_ELINE)));
        assert!(matches!(Csplit::open(&DATA, &[Pat::line(1, 0, 0)]), Err(CSI_EREP)));
        assert!(matches!(Csplit::open(&DATA, &[Pat::lit(b"", 0, 1)]), Err(CSI_EPAT)));
        assert!(matches!(
            Csplit::open(&DATA, &[Pat::line(1, OFFMAX + 1, 1)]),
            Err(CSI_EOFF)
        ));
    }

    #[test]
    fn miss_and_state() {
        let mut c = open(&[Pat::lit(b"zeta", 0, 1)]);
        assert_eq!(c.run(), Err(CSI_ENOMATCH));
        let mut d = open(&[Pat::line(2, 0, 1)]);
        assert_eq!(d.run(), Ok(2));
        assert_eq!(d.run(), Err(CSI_ESTATE));
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
        assert_eq!(l.freeze(), Err(CSI_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(CSI_ESLOT));
        assert_eq!(l.rejects(), 2);
        assert_eq!(l.replay(), Err(CSI_ESEAL));
        assert_eq!(inject_four(), [true; 4]);
    }
}
