//! UNX-F11514 cut 字节/字符/字段三切（-b/-c/-f）
//!
//! `cut` 的三把刀切的是三种**不同的坐标轴**，把它们的语义混起来是这一族最
//! 常见的错：
//!
//! - `-b`：字节偏移。切在多字节 UTF-8 序列中间时**允许切碎**——这是它的
//!   本义，二进制数据没有字符概念；
//! - `-c`：字符偏移。本实现把输入按「已解码的标量」交给它（每个单元是一个
//!   `u16` 码元），因此代理区码元一律报 `CT_ECHAR`，绝不在半个字符上切；
//! - `-f`：字段偏移。字段由 `-d` 分隔符划定，**没出现分隔符的行整行输出**，
//!   除非给了 `-s`（此时该行被丢弃）。这条「整行输出」是最容易被漏掉的。
//!
//! 位置表语法是共用的：`N`、`N-M`、`N-`、`-M`，逗号分隔。归一化规则写死：
//! 升序排列、相接或重叠的区间合并成一条、开区间（`N-`）吞掉其后的所有区间。
//! 于是 `1,3-5` 与 `3-5,1` 与 `1-5` 在账上是同一个选择器——账本记的是**归一
//! 后**的条数，而不是用户敲了几段。
//!
//! 构造闸门次序（`Opts::build`）：模式互斥（0 个 → `ENOMODE`，2 个以上 →
//! `EEXCL`）→ 分隔符（`-d` 只对 `-f` 有意义，多字节或零字节报 `EDELIM`）→
//! `-s` 只对 `-f` 有意义（否则 `ESFLAG`）。次序不能颠倒：`cut -b -d:` 同时犯了
//! 「-d 用错模式」和「没给 -f」，先报模式缺失对调用方更可操作。
//!
//! 区间闸门次序（`add_spec`）：数值合法性（0 不是位置、逆序、空段）→ 表满。
//! 先报 `ERANGE` 再报 `EFULL`：一段非法区间不该因为表恰好满了而被报成「表满」。
//!
//! 账本语义：十四格 = 八格成功（三轴各若干）+ 六格失败（八类域错里的六类各
//! 一格）。`audit` 认「八值六码」，`seal` 做 FNV-1a 折叠，`replay` 重建比对，
//! `freeze` 后登记一律拒绝。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf130;

/// 未指定 -b/-c/-f。
pub const CT_ENOMODE: u32 = SEG_BASE + 0x01;
/// 模式重复指定。
pub const CT_EEXCL: u32 = SEG_BASE + 0x02;
/// 分隔符非法或用在错误模式下。
pub const CT_EDELIM: u32 = SEG_BASE + 0x03;
/// -s 用在非字段模式。
pub const CT_ESFLAG: u32 = SEG_BASE + 0x04;
/// 区间非法。
pub const CT_ERANGE: u32 = SEG_BASE + 0x05;
/// 区间表满。
pub const CT_EFULL: u32 = SEG_BASE + 0x06;
/// 字符模式下出现非标量码元。
pub const CT_ECHAR: u32 = SEG_BASE + 0x07;
/// 模式与操作不匹配。
pub const CT_EMODE: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const CT_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const CT_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const CT_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const CT_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const CT_ENOBS: u32 = SEG_BASE + 0x0d;

/// 字节模式。
pub const MODE_BYTE: u8 = 1;
/// 字符模式。
pub const MODE_CHAR: u8 = 2;
/// 字段模式。
pub const MODE_FIELD: u8 = 3;

/// 位置表中归一化后的区间条数上限。
pub const CAP: usize = 8;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 单段数值上限（位置表不做无界增长）。
pub const POS_MAX: u32 = 1_000_000;

/// 操作码：构造。
pub const OP_BUILD: u8 = 1;
/// 操作码：区间归一。
pub const OP_SPEC: u8 = 2;
/// 操作码：按单元切。
pub const OP_UNITS: u8 = 3;
/// 操作码：按字段切。
pub const OP_FIELDS: u8 = 4;

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

/// 一段闭区间 `[lo, hi]`；`hi == 0` 表示「到行尾」的开区间。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    /// 起点（1 基）。
    pub lo: u32,
    /// 终点（1 基闭区间）；0 表示开放。
    pub hi: u32,
}

/// 选项构造器。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opts {
    b: bool,
    c: bool,
    f: bool,
    delim: u32,
    has_d: bool,
    suppress: bool,
    complement: bool,
}

impl Opts {
    /// 空选项。
    pub const fn new() -> Opts {
        Opts {
            b: false,
            c: false,
            f: false,
            delim: 0,
            has_d: false,
            suppress: false,
            complement: false,
        }
    }

    /// 选字节轴。
    pub fn bytes(self) -> Opts {
        let mut o = self;
        o.b = true;
        o
    }

    /// 选字符轴。
    pub fn chars(self) -> Opts {
        let mut o = self;
        o.c = true;
        o
    }

    /// 选字段轴。
    pub fn fields(self) -> Opts {
        let mut o = self;
        o.f = true;
        o
    }

    /// 指定分隔符（码点值）。
    pub fn delim(self, d: u32) -> Opts {
        let mut o = self;
        o.delim = d;
        o.has_d = true;
        o
    }

    /// 抑制无分隔符的行。
    pub fn suppress(self) -> Opts {
        let mut o = self;
        o.suppress = true;
        o
    }

    /// 取补集。
    pub fn complement(self) -> Opts {
        let mut o = self;
        o.complement = true;
        o
    }

    /// 定型并过闸。
    pub fn build(&self) -> Result<Cut, u32> {
        let mut modes = 0u8;
        if self.b {
            modes = modes.saturating_add(1);
        }
        if self.c {
            modes = modes.saturating_add(1);
        }
        if self.f {
            modes = modes.saturating_add(1);
        }
        if modes == 0 {
            return Err(CT_ENOMODE);
        }
        if modes > 1 {
            return Err(CT_EEXCL);
        }
        let mut cut = Cut::blank();
        cut.complement = self.complement;
        if self.f {
            let d = if self.has_d { self.delim } else { 0x09 };
            if d == 0 || d > 0xff {
                return Err(CT_EDELIM);
            }
            cut.mode = MODE_FIELD;
            cut.delim = d as u8;
            cut.suppress = self.suppress;
            return Ok(cut);
        }
        if self.has_d {
            return Err(CT_EDELIM);
        }
        if self.suppress {
            return Err(CT_ESFLAG);
        }
        cut.mode = if self.b { MODE_BYTE } else { MODE_CHAR };
        Ok(cut)
    }
}

/// 三轴之一的选择器。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cut {
    mode: u8,
    delim: u8,
    suppress: bool,
    complement: bool,
    spans: [Span; CAP],
    n: usize,
}

impl Cut {
    /// 空白选择器（仅供内部起手）。
    pub const fn blank() -> Cut {
        Cut {
            mode: 0,
            delim: 0,
            suppress: false,
            complement: false,
            spans: [Span { lo: 0, hi: 0 }; CAP],
            n: 0,
        }
    }

    /// 模式。
    pub fn mode(&self) -> u8 {
        self.mode
    }

    /// 分隔符字节。
    pub fn delim(&self) -> u8 {
        self.delim
    }

    /// 是否抑制无分隔符的行。
    pub fn suppresses(&self) -> bool {
        self.suppress
    }

    /// 是否取补集。
    pub fn complemented(&self) -> bool {
        self.complement
    }

    /// 归一化后的区间条数。
    pub fn span_count(&self) -> usize {
        self.n
    }

    /// 第 i 段区间。
    pub fn span_at(&self, i: usize) -> Option<Span> {
        if i >= self.n {
            return None;
        }
        self.spans.get(i).copied()
    }

    /// 位置表数值解析。
    fn num(s: &str) -> Result<u32, u32> {
        if s.is_empty() || s.len() > 9 {
            return Err(CT_ERANGE);
        }
        let b = s.as_bytes();
        let mut v: u32 = 0;
        let mut i = 0usize;
        while i < b.len() {
            let c = match b.get(i) {
                Some(x) => *x,
                None => return Err(CT_ERANGE),
            };
            if !c.is_ascii_digit() {
                return Err(CT_ERANGE);
            }
            v = match v.checked_mul(10) {
                Some(x) => x,
                None => return Err(CT_ERANGE),
            };
            v = match v.checked_add((c - b'0') as u32) {
                Some(x) => x,
                None => return Err(CT_ERANGE),
            };
            i += 1;
        }
        if v == 0 || v > POS_MAX {
            return Err(CT_ERANGE);
        }
        Ok(v)
    }

    /// 把一段区间并入表并归一化。
    pub fn add_range(&mut self, lo: u32, hi: u32) -> Result<usize, u32> {
        if lo == 0 || lo > POS_MAX {
            return Err(CT_ERANGE);
        }
        if hi != 0 {
            if hi > POS_MAX || hi < lo {
                return Err(CT_ERANGE);
            }
        }
        if self.n >= CAP {
            return Err(CT_EFULL);
        }
        if let Some(dst) = self.spans.get_mut(self.n) {
            *dst = Span { lo, hi };
        }
        self.n = self.n.saturating_add(1);
        self.normalize();
        Ok(self.n)
    }

    /// 归一化：按 lo 升序，相接/重叠/开放区间合并。
    fn normalize(&mut self) {
        let mut i = 1usize;
        while i < self.n {
            let mut j = i;
            while j > 0 {
                let a = match self.spans.get(j - 1) {
                    Some(v) => *v,
                    None => break,
                };
                let b = match self.spans.get(j) {
                    Some(v) => *v,
                    None => break,
                };
                if a.lo <= b.lo {
                    break;
                }
                if let Some(dst) = self.spans.get_mut(j - 1) {
                    *dst = b;
                }
                if let Some(dst) = self.spans.get_mut(j) {
                    *dst = a;
                }
                j -= 1;
            }
            i += 1;
        }
        let mut k = 0usize;
        while k + 1 < self.n {
            let a = match self.spans.get(k) {
                Some(v) => *v,
                None => return,
            };
            let b = match self.spans.get(k + 1) {
                Some(v) => *v,
                None => return,
            };
            let touches = a.hi == 0 || b.lo <= a.hi.saturating_add(1);
            if !touches {
                k += 1;
                continue;
            }
            let hi = if a.hi == 0 || b.hi == 0 {
                0
            } else if a.hi > b.hi {
                a.hi
            } else {
                b.hi
            };
            if let Some(dst) = self.spans.get_mut(k) {
                *dst = Span { lo: a.lo, hi };
            }
            let mut m = k + 1;
            while m + 1 < self.n {
                match self.spans.get(m + 1).copied() {
                    Some(next) => {
                        if let Some(dst) = self.spans.get_mut(m) {
                            *dst = next;
                        }
                    }
                    None => return,
                }
                m += 1;
            }
            self.n = self.n.saturating_sub(1);
        }
    }

    /// 解析位置表（`N` / `N-M` / `N-` / `-M`，逗号分隔）。
    pub fn add_spec(&mut self, spec: &str) -> Result<usize, u32> {
        if spec.is_empty() {
            return Err(CT_ERANGE);
        }
        let b = spec.as_bytes();
        let mut start = 0usize;
        let mut i = 0usize;
        let mut segs = 0usize;
        while i <= b.len() {
            let at_end = i == b.len();
            let is_comma = match b.get(i) {
                Some(c) => *c == b',',
                None => at_end,
            };
            if at_end || is_comma {
                let seg = match spec.get(start..i) {
                    Some(v) => v,
                    None => return Err(CT_ERANGE),
                };
                let (lo, hi) = Cut::parse_seg(seg)?;
                let _ = self.add_range(lo, hi)?;
                segs += 1;
                start = i.saturating_add(1);
            }
            i = i.saturating_add(1);
        }
        if segs == 0 {
            return Err(CT_ERANGE);
        }
        Ok(self.n)
    }

    /// 单段解析。
    fn parse_seg(seg: &str) -> Result<(u32, u32), u32> {
        if seg.is_empty() {
            return Err(CT_ERANGE);
        }
        let dash = seg.find('-');
        match dash {
            None => {
                let n = Cut::num(seg)?;
                Ok((n, n))
            }
            Some(0) => {
                let hi = Cut::num(match seg.get(1..) {
                    Some(v) => v,
                    None => return Err(CT_ERANGE),
                })?;
                Ok((1, hi))
            }
            Some(p) => {
                let lo = Cut::num(match seg.get(..p) {
                    Some(v) => v,
                    None => return Err(CT_ERANGE),
                })?;
                let tail = match seg.get(p + 1..) {
                    Some(v) => v,
                    None => return Err(CT_ERANGE),
                };
                if tail.is_empty() {
                    return Ok((lo, 0));
                }
                if tail.contains('-') {
                    return Err(CT_ERANGE);
                }
                let hi = Cut::num(tail)?;
                if hi < lo {
                    return Err(CT_ERANGE);
                }
                Ok((lo, hi))
            }
        }
    }

    /// 位置是否入选（已计补集）。
    pub fn picks(&self, pos: u32) -> bool {
        let mut hit = false;
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.spans.get(i) {
                if pos >= s.lo && (s.hi == 0 || pos <= s.hi) {
                    hit = true;
                    break;
                }
            }
            i += 1;
        }
        if self.complement {
            !hit
        } else {
            hit
        }
    }

    /// 按单元切（字节轴或字符轴），返回入选单元数。
    pub fn apply_units(&self, units: &[u16]) -> Result<u32, u32> {
        if self.mode == MODE_FIELD {
            return Err(CT_EMODE);
        }
        if self.mode != MODE_BYTE && self.mode != MODE_CHAR {
            return Err(CT_EMODE);
        }
        if self.mode == MODE_CHAR {
            let mut i = 0usize;
            while i < units.len() {
                let u = match units.get(i) {
                    Some(v) => *v,
                    None => break,
                };
                if u >= 0xd800 && u <= 0xdfff {
                    return Err(CT_ECHAR);
                }
                i += 1;
            }
        }
        let mut n = 0u32;
        let mut pos = 1u32;
        let mut i = 0usize;
        while i < units.len() {
            if self.picks(pos) {
                n = n.saturating_add(1);
            }
            pos = pos.saturating_add(1);
            i += 1;
        }
        Ok(n)
    }

    /// 按字段切，返回入选字段数；无分隔符的行按整行记一格或被抑制。
    pub fn apply_fields(&self, line: &[u8]) -> Result<u32, u32> {
        if self.mode != MODE_FIELD {
            return Err(CT_EMODE);
        }
        let mut nf = 1u32;
        let mut has = false;
        let mut i = 0usize;
        while i < line.len() {
            match line.get(i) {
                Some(b) => {
                    if *b == self.delim {
                        has = true;
                        nf = nf.saturating_add(1);
                    }
                }
                None => break,
            }
            i += 1;
        }
        if !has {
            return Ok(if self.suppress { 0 } else { 1 });
        }
        let mut sel = 0u32;
        let mut k = 1u32;
        while k <= nf {
            if self.picks(k) {
                sel = sel.saturating_add(1);
            }
            k = k.saturating_add(1);
        }
        Ok(sel)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.mode != MODE_BYTE && self.mode != MODE_CHAR && self.mode != MODE_FIELD {
            return false;
        }
        if self.mode == MODE_FIELD {
            if self.delim == 0 {
                return false;
            }
        } else if self.delim != 0 || self.suppress {
            return false;
        }
        if self.n > CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            let s = match self.spans.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if s.lo == 0 || (s.hi != 0 && s.hi < s.lo) {
                return false;
            }
            if i > 0 {
                match self.spans.get(i - 1) {
                    Some(p) => {
                        if p.hi == 0 {
                            return false;
                        }
                        if s.lo <= p.hi.saturating_add(1) {
                            return false;
                        }
                    }
                    None => return false,
                }
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
            return Err(CT_ESLOT);
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
            return Err(CT_EBAL);
        }
        const VALS: [u32; 8] = [4, 1, 2, 1, 2, 2, 0, 1];
        const CODES: [u32; 6] =
            [CT_ENOMODE, CT_ERANGE, CT_ERANGE, CT_EDELIM, CT_ESFLAG, CT_EFULL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(CT_EBAL),
            };
            if s.slot as usize != i {
                return Err(CT_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(CT_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(CT_EVALUE);
                        }
                    }
                    None => return Err(CT_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(CT_EVALUE);
                        }
                    }
                    None => return Err(CT_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(CT_EBAL);
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
            return Err(CT_EBAL);
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
            Err(CT_ESEAL)
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

/// 八个单元的行（字节轴与字符轴共用）。
const LINE: [u16; 8] = [10, 20, 30, 40, 50, 60, 70, 80];
/// 冒号分隔的三字段行。
const ROW: &[u8] = b"a:b:c";
/// 无分隔符行。
const PLAIN: &[u8] = b"abc";

/// 起手 + 位置表，返回选择器。
fn mk(opts: Opts, spec: &str) -> Result<Cut, u32> {
    let mut c = opts.build()?;
    let _ = c.add_spec(spec)?;
    Ok(c)
}

/// 十四格账本：三轴各切 → 补集 → 抑制 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(word_cell(
        0,
        OP_UNITS,
        mk(Opts::new().bytes(), "1,3-5").and_then(|c| c.apply_units(&LINE)),
    ));
    let _ = l.register(word_cell(
        1,
        OP_UNITS,
        mk(Opts::new().bytes(), "8-").and_then(|c| c.apply_units(&LINE)),
    ));
    let _ = l.register(word_cell(
        2,
        OP_UNITS,
        mk(Opts::new().bytes().complement(), "2-7").and_then(|c| c.apply_units(&LINE)),
    ));
    let _ = l.register(word_cell(
        3,
        OP_FIELDS,
        mk(Opts::new().fields().delim(b':' as u32), "2").and_then(|c| c.apply_fields(ROW)),
    ));
    let _ = l.register(word_cell(
        4,
        OP_FIELDS,
        mk(Opts::new().fields().delim(b':' as u32), "1,3").and_then(|c| c.apply_fields(ROW)),
    ));
    let _ = l.register(word_cell(
        5,
        OP_UNITS,
        mk(Opts::new().chars(), "2-3").and_then(|c| c.apply_units(&LINE)),
    ));
    let _ = l.register(word_cell(
        6,
        OP_FIELDS,
        mk(Opts::new().fields().delim(b':' as u32).suppress(), "2")
            .and_then(|c| c.apply_fields(PLAIN)),
    ));
    let _ = l.register(word_cell(
        7,
        OP_FIELDS,
        mk(Opts::new().fields().delim(b':' as u32), "2").and_then(|c| c.apply_fields(PLAIN)),
    ));

    let _ = l.register(word_cell(8, OP_BUILD, Opts::new().build().map(|_| 0)));
    let _ = l.register(word_cell(
        9,
        OP_SPEC,
        mk(Opts::new().bytes(), "0-2").map(|_| 0),
    ));
    let _ = l.register(word_cell(
        10,
        OP_SPEC,
        mk(Opts::new().bytes(), "4-2").map(|_| 0),
    ));
    let _ = l.register(word_cell(
        11,
        OP_BUILD,
        Opts::new().chars().delim(0x41).build().map(|_| 0),
    ));
    let _ = l.register(word_cell(
        12,
        OP_BUILD,
        Opts::new().bytes().suppress().build().map(|_| 0),
    ));
    let _ = l.register(word_cell(
        13,
        OP_SPEC,
        mk(Opts::new().bytes(), "1,3,5,7,9,11,13,15,17").map(|_| 0),
    ));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：字节轴与开区间。
    let b = match mk(Opts::new().bytes(), "8-") {
        Ok(v) => v,
        Err(_) => return out,
    };
    let open = match mk(Opts::new().bytes(), "2-4,6-") {
        Ok(v) => v,
        Err(_) => return out,
    };
    out[1] = b.apply_units(&LINE) == Ok(1)
        && b.picks(8)
        && !b.picks(7)
        && open.span_count() == 2
        && open.apply_units(&LINE) == Ok(6)
        && b.consistent()
        && open.consistent();

    // 二路：归一化与补集。
    let norm = match mk(Opts::new().bytes(), "3-5,1,2") {
        Ok(v) => v,
        Err(_) => return out,
    };
    let comp = match mk(Opts::new().bytes().complement(), "2-7") {
        Ok(v) => v,
        Err(_) => return out,
    };
    out[2] = norm.span_count() == 1
        && norm.span_at(0) == Some(Span { lo: 1, hi: 5 })
        && norm.apply_units(&LINE) == Ok(5)
        && comp.span_count() == 1
        && comp.apply_units(&LINE) == Ok(2)
        && comp.picks(1)
        && !comp.picks(2)
        && norm.consistent()
        && comp.consistent();

    // 三路：三轴的闸门与错配。
    let f = match mk(Opts::new().fields().delim(b':' as u32), "2,3") {
        Ok(v) => v,
        Err(_) => return out,
    };
    let ch = match mk(Opts::new().chars(), "1") {
        Ok(v) => v,
        Err(_) => return out,
    };
    out[3] = f.apply_fields(ROW) == Ok(2)
        && f.apply_fields(b":a:") == Ok(2)
        && f.apply_units(&LINE) == Err(CT_EMODE)
        && ch.apply_fields(ROW) == Err(CT_EMODE)
        && ch.apply_units(&[0xd800]) == Err(CT_ECHAR)
        && ch.apply_units(&[0x41]) == Ok(1)
        && Opts::new().build() == Err(CT_ENOMODE)
        && Opts::new().bytes().chars().build() == Err(CT_EEXCL)
        && Opts::new().fields().delim(0x1f600).build() == Err(CT_EDELIM)
        && f.consistent()
        && ch.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11514_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11514.cut_three_axes");
    let l = build_ledger();

    s.add_dyn(
        "CT1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "CT2-三轴互斥",
        Opts::new().build() == Err(CT_ENOMODE)
            && Opts::new().bytes().chars().build() == Err(CT_EEXCL)
            && Opts::new().fields().bytes().build() == Err(CT_EEXCL)
            && Opts::new().bytes().build().is_ok()
            && l.sample(8).map(|x| x.code) == Some(CT_ENOMODE),
        "static",
    );

    s.add(
        "CT3-字节轴与开区间",
        mk(Opts::new().bytes(), "1,3-5").map(|c| c.apply_units(&LINE)) == Ok(Ok(4))
            && mk(Opts::new().bytes(), "8-").map(|c| c.apply_units(&LINE)) == Ok(Ok(1))
            && mk(Opts::new().bytes(), "9-").map(|c| c.apply_units(&LINE)) == Ok(Ok(0))
            && l.sample(0).map(|x| x.value) == Some(4)
            && l.sample(1).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "CT4-字符轴拒非标量",
        {
            let ch = match mk(Opts::new().chars(), "1-2") {
                Ok(v) => v,
                Err(_) => return s,
            };
            ch.apply_units(&[0x41, 0x4e2d]) == Ok(2)
                && ch.apply_units(&[0x41, 0xd800]) == Err(CT_ECHAR)
                && ch.apply_units(&[0xdfff]) == Err(CT_ECHAR)
                && ch.apply_units(&[]) == Ok(0)
                && l.sample(5).map(|x| x.value) == Some(2)
        },
        "static",
    );

    s.add(
        "CT5-区间归一化",
        {
            let a = match mk(Opts::new().bytes(), "3-5,1,2") {
                Ok(v) => v,
                Err(_) => return s,
            };
            let b = match mk(Opts::new().bytes(), "1-3,3-5") {
                Ok(v) => v,
                Err(_) => return s,
            };
            let c = match mk(Opts::new().bytes(), "5,1-2,3-4") {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.span_count() == 1
                && a.span_at(0) == Some(Span { lo: 1, hi: 5 })
                && b.span_count() == 1
                && b.span_at(0) == Some(Span { lo: 1, hi: 5 })
                && c.span_count() == 1
                && a.consistent()
                && b.consistent()
                && c.consistent()
        },
        "static",
    );

    s.add(
        "CT6-区间合法性",
        mk(Opts::new().bytes(), "0").is_err()
            && mk(Opts::new().bytes(), "0-2") == Err(CT_ERANGE)
            && mk(Opts::new().bytes(), "4-2") == Err(CT_ERANGE)
            && mk(Opts::new().bytes(), "1,,2") == Err(CT_ERANGE)
            && mk(Opts::new().bytes(), "") == Err(CT_ERANGE)
            && mk(Opts::new().bytes(), "-0") == Err(CT_ERANGE)
            && mk(Opts::new().bytes(), "1-2-3") == Err(CT_ERANGE)
            && mk(Opts::new().bytes(), "a") == Err(CT_ERANGE)
            && l.sample(9).map(|x| x.code) == Some(CT_ERANGE)
            && l.sample(10).map(|x| x.code) == Some(CT_ERANGE),
        "static",
    );

    s.add(
        "CT7-区间表满八段",
        {
            let eight = mk(Opts::new().bytes(), "1,3,5,7,9,11,13,15");
            let nine = mk(Opts::new().bytes(), "1,3,5,7,9,11,13,15,17");
            matches!(eight, Ok(ref c) if c.span_count() == CAP)
                && nine == Err(CT_EFULL)
                && l.sample(13).map(|x| x.code) == Some(CT_EFULL)
        },
        "static",
    );

    s.add(
        "CT8-补集与相接",
        {
            let c = match mk(Opts::new().bytes().complement(), "2-7") {
                Ok(v) => v,
                Err(_) => return s,
            };
            c.apply_units(&LINE) == Ok(2)
                && c.picks(1)
                && !c.picks(2)
                && c.picks(8)
                && c.picks(9)
                && l.sample(2).map(|x| x.value) == Some(2)
        },
        "static",
    );

    s.add(
        "CT9-字段轴三切",
        {
            let two = match mk(Opts::new().fields().delim(b':' as u32), "2") {
                Ok(v) => v,
                Err(_) => return s,
            };
            let ends = match mk(Opts::new().fields().delim(b':' as u32), "1,3") {
                Ok(v) => v,
                Err(_) => return s,
            };
            two.apply_fields(ROW) == Ok(1)
                && ends.apply_fields(ROW) == Ok(2)
                && ends.apply_fields(b"a:b") == Ok(1)
                && ends.apply_fields(b"a:") == Ok(1)
                && two.delim() == b':'
                && l.sample(3).map(|x| x.value) == Some(1)
                && l.sample(4).map(|x| x.value) == Some(2)
        },
        "static",
    );

    s.add(
        "CT10-无分隔行整行输出",
        {
            let plain = match mk(Opts::new().fields().delim(b':' as u32), "2") {
                Ok(v) => v,
                Err(_) => return s,
            };
            let quiet = match mk(Opts::new().fields().delim(b':' as u32).suppress(), "2") {
                Ok(v) => v,
                Err(_) => return s,
            };
            plain.apply_fields(PLAIN) == Ok(1)
                && quiet.apply_fields(PLAIN) == Ok(0)
                && plain.apply_fields(b"") == Ok(1)
                && quiet.suppresses()
                && !plain.suppresses()
                && l.sample(6).map(|x| x.value) == Some(0)
                && l.sample(7).map(|x| x.value) == Some(1)
        },
        "static",
    );

    s.add(
        "CT11-分隔符门",
        Opts::new().fields().delim(0x1f600).build() == Err(CT_EDELIM)
            && Opts::new().fields().delim(0).build() == Err(CT_EDELIM)
            && Opts::new().bytes().delim(b':' as u32).build() == Err(CT_EDELIM)
            && Opts::new().chars().delim(b':' as u32).build() == Err(CT_EDELIM)
            && Opts::new().bytes().suppress().build() == Err(CT_ESFLAG)
            && Opts::new().fields().build().map(|c| c.delim()) == Ok(b'\t')
            && l.sample(11).map(|x| x.code) == Some(CT_EDELIM)
            && l.sample(12).map(|x| x.code) == Some(CT_ESFLAG),
        "static",
    );

    s.add(
        "CT12-模式错配与相接合并",
        {
            let f = match mk(Opts::new().fields().delim(b':' as u32), "1-2") {
                Ok(v) => v,
                Err(_) => return s,
            };
            let b = match mk(Opts::new().bytes(), "1-2") {
                Ok(v) => v,
                Err(_) => return s,
            };
            let joined = match mk(Opts::new().bytes(), "1-2,3-4") {
                Ok(v) => v,
                Err(_) => return s,
            };
            f.apply_units(&LINE) == Err(CT_EMODE)
                && b.apply_fields(ROW) == Err(CT_EMODE)
                && joined.span_count() == 1
                && joined.span_at(0) == Some(Span { lo: 1, hi: 4 })
                && mk(Opts::new().bytes(), "1-2,4-5").map(|c| c.span_count()) == Ok(2)
        },
        "static",
    );

    s.add(
        "CT13-冻结重放四注入",
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
        let s = run_unx11514_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn bytes_and_chars_differ() {
        let b = match mk(Opts::new().bytes(), "2") {
            Ok(v) => v,
            Err(_) => panic!("build"),
        };
        let c = match mk(Opts::new().chars(), "2") {
            Ok(v) => v,
            Err(_) => panic!("build"),
        };
        assert_eq!(b.apply_units(&LINE), Ok(1));
        assert_eq!(c.apply_units(&LINE), Ok(1));
        assert_eq!(c.apply_units(&[0x41, 0xdc00]), Err(CT_ECHAR));
        assert_eq!(b.apply_units(&[0x41, 0xdc00]), Ok(1));
    }

    #[test]
    fn ranges_normalize() {
        let c = match mk(Opts::new().bytes(), "4,1-3,5") {
            Ok(v) => v,
            Err(_) => panic!("build"),
        };
        assert_eq!(c.span_count(), 1);
        assert_eq!(c.span_at(0), Some(Span { lo: 1, hi: 5 }));
        assert_eq!(c.apply_units(&LINE), Ok(5));
        assert!(c.consistent());
    }

    #[test]
    fn open_range_is_unbounded() {
        let c = match mk(Opts::new().bytes(), "6-") {
            Ok(v) => v,
            Err(_) => panic!("build"),
        };
        assert_eq!(c.span_at(0), Some(Span { lo: 6, hi: 0 }));
        assert_eq!(c.apply_units(&LINE), Ok(3));
        assert!(c.picks(999));
    }

    #[test]
    fn field_mode_rules() {
        let f = match mk(Opts::new().fields().delim(b':' as u32), "2") {
            Ok(v) => v,
            Err(_) => panic!("build"),
        };
        assert_eq!(f.apply_fields(b"a:b:c"), Ok(1));
        assert_eq!(f.apply_fields(b"abc"), Ok(1));
        assert_eq!(f.apply_units(&LINE), Err(CT_EMODE));
        let q = match mk(Opts::new().fields().delim(b':' as u32).suppress(), "2") {
            Ok(v) => v,
            Err(_) => panic!("build"),
        };
        assert_eq!(q.apply_fields(b"abc"), Ok(0));
    }

    #[test]
    fn spec_rejects_bad_ranges() {
        assert_eq!(mk(Opts::new().bytes(), "0"), Err(CT_ERANGE));
        assert_eq!(mk(Opts::new().bytes(), "5-2"), Err(CT_ERANGE));
        assert_eq!(mk(Opts::new().bytes(), "1-2-3"), Err(CT_ERANGE));
        assert_eq!(
            mk(Opts::new().bytes(), "1,3,5,7,9,11,13,15,17"),
            Err(CT_EFULL)
        );
        assert_eq!(Opts::new().build(), Err(CT_ENOMODE));
        assert_eq!(Opts::new().bytes().chars().build(), Err(CT_EEXCL));
    }

    #[test]
    fn inject_four_is_all_true() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
