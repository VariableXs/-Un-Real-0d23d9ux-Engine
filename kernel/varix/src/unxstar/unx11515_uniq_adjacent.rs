//! UNX-F11515 uniq 相邻去重与 -c/-d/-u
//!
//! `uniq` 的名字骗人：它不做全局去重，只合并**相邻**的重复行。`a b a` 三行进
//! 去，三行出来——想真正去重必须先 `sort`。这条「只比邻居」的纪律，正是它能在
//! 流式管道里常驻的原因：内存里永远只需要上一行的键。
//!
//! 键的构造有三级闸门：
//!
//! 1. `-f N` 先跳过前 N 个空白分隔字段（空白串视为一个分隔，行尾字段不足时
//!    键退化为空串）；
//! 2. `-w N` 再截前 N 个字节。`-w 0` 是**合法且极端**的：键恒空，于是所有行
//!    归成一组——这不是边界错误，而是「比较零个字符」的字面结果；
//! 3. `-i` 最后折叠大小写。折叠只在 ASCII 定义域内成立，遇到 `>= 0x80` 的
//!    字节直接报 `UQ_ECHAR`：宁可拒，不可默默按原字节比（那会让 `-i` 在非
//!    英文数据上静默失效）。
//!
//! 键缓冲是定长 64 字节。未给 `-w` 的行如果键超宽，报 `UQ_EKEY` 而不是截断：
//! 截断会让「键相同」变成假相等，这是去重工具最不该有的静默行为。给了 `-w`
//! 就按用户声明的宽度截，那是显式授权。
//!
//! `-d` 与 `-u` 在本实现里互斥（`UQ_EEXCL`）。GNU 允许两者同给并静默输出空，
//! 但「空输出」是最难排查的一种结果；拒掉它，调用方立刻知道自己想要的是
//! 「全部」而不是「既重复又唯一」。
//!
//! 计数输出（`-c`）的格式固定为 `%7d` 加一个空格再跟整行：每条 8 + 行宽 + 1
//! 字节。账本按这个公式记账，输出总量越过预算报 `UQ_EBYTES`。
//!
//! 账本语义：十四格 = 八格成功（推送三次、行数、两种筛选、计数输出、组数）+
//! 六格失败（八类域错里的六类各一格）。`audit` 认「八值六码」，`seal` 做
//! FNV-1a 折叠，`replay` 重建比对，`freeze` 后登记一律拒绝。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf140;

/// -d 与 -u 同给。
pub const UQ_EEXCL: u32 = SEG_BASE + 0x01;
/// -w 超过键缓冲。
pub const UQ_EWIDTH: u32 = SEG_BASE + 0x02;
/// -f 超过字段上限。
pub const UQ_EFIELDS: u32 = SEG_BASE + 0x03;
/// 行键超宽。
pub const UQ_EKEY: u32 = SEG_BASE + 0x04;
/// 折叠遇到非 ASCII 字节。
pub const UQ_ECHAR: u32 = SEG_BASE + 0x05;
/// 组表满。
pub const UQ_EFULL: u32 = SEG_BASE + 0x06;
/// 输出越过字节预算。
pub const UQ_EBYTES: u32 = SEG_BASE + 0x07;
/// 空输入上查末组。
pub const UQ_ENOLINE: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const UQ_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const UQ_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const UQ_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const UQ_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const UQ_ENOBS: u32 = SEG_BASE + 0x0d;

/// 组表容量。
pub const CAP: usize = 8;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 键缓冲容量。
pub const KEYMAX: usize = 64;
/// `-f` 可跳过的字段数上限。
pub const FIELD_MAX: u32 = 8;
/// 单次输出的字节预算。
pub const OUT_CAP: u32 = 512;
/// 计数前缀宽度（`%7d` 加一个空格）。
pub const COUNT_W: u32 = 8;
/// 未指定 `-w`。
pub const UNLIMITED: u32 = 0xffff_ffff;

/// 操作码：构造。
pub const OP_BUILD: u8 = 1;
/// 操作码：推送一行。
pub const OP_PUSH: u8 = 2;
/// 操作码：读数。
pub const OP_READ: u8 = 3;
/// 操作码：输出记账。
pub const OP_EMIT: u8 = 4;

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

/// 空白字节（空格或制表符）。
fn is_blank(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

/// 选项构造器。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opts {
    icase: bool,
    width: u32,
    skip: u32,
    dup: bool,
    uniq: bool,
    count: bool,
}

impl Opts {
    /// 空选项。
    pub const fn new() -> Opts {
        Opts { icase: false, width: UNLIMITED, skip: 0, dup: false, uniq: false, count: false }
    }

    /// `-i`。
    pub fn icase(self) -> Opts {
        let mut o = self;
        o.icase = true;
        o
    }

    /// `-w N`。
    pub fn width(self, w: u32) -> Opts {
        let mut o = self;
        o.width = w;
        o
    }

    /// `-f N`。
    pub fn skip(self, n: u32) -> Opts {
        let mut o = self;
        o.skip = n;
        o
    }

    /// `-d`。
    pub fn dup_only(self) -> Opts {
        let mut o = self;
        o.dup = true;
        o
    }

    /// `-u`。
    pub fn uniq_only(self) -> Opts {
        let mut o = self;
        o.uniq = true;
        o
    }

    /// `-c`。
    pub fn counted(self) -> Opts {
        let mut o = self;
        o.count = true;
        o
    }

    /// 定型并过闸。
    pub fn build(&self) -> Result<Proc, u32> {
        if self.dup && self.uniq {
            return Err(UQ_EEXCL);
        }
        if self.width != UNLIMITED && self.width > KEYMAX as u32 {
            return Err(UQ_EWIDTH);
        }
        if self.skip > FIELD_MAX {
            return Err(UQ_EFIELDS);
        }
        Ok(Proc {
            icase: self.icase,
            width: self.width,
            skip: self.skip,
            dup: self.dup,
            uniq: self.uniq,
            count: self.count,
        })
    }
}

/// 去重参数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Proc {
    icase: bool,
    width: u32,
    skip: u32,
    dup: bool,
    uniq: bool,
    count: bool,
}

impl Proc {
    /// 缺省参数。
    pub const fn plain() -> Proc {
        Proc { icase: false, width: UNLIMITED, skip: 0, dup: false, uniq: false, count: false }
    }

    /// 是否折叠大小写。
    pub fn icase(&self) -> bool {
        self.icase
    }

    /// 比较宽度。
    pub fn width(&self) -> u32 {
        self.width
    }

    /// 跳过的字段数。
    pub fn skip(&self) -> u32 {
        self.skip
    }

    /// 是否只出重复组。
    pub fn dup_only(&self) -> bool {
        self.dup
    }

    /// 是否只出唯一组。
    pub fn uniq_only(&self) -> bool {
        self.uniq
    }

    /// 是否带计数前缀。
    pub fn counted(&self) -> bool {
        self.count
    }

    /// 某组是否入选。
    pub fn wanted(&self, n: u32) -> bool {
        if self.dup {
            n > 1
        } else if self.uniq {
            n == 1
        } else {
            true
        }
    }

    /// 构造一行比较键。
    pub fn key(&self, line: &[u8]) -> Result<([u8; KEYMAX], usize), u32> {
        let mut out = [0u8; KEYMAX];
        let mut i = 0usize;
        let mut f = 0u32;
        while f < self.skip {
            while i < line.len() {
                match line.get(i) {
                    Some(b) => {
                        if is_blank(*b) {
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    None => break,
                }
            }
            let mut moved = false;
            while i < line.len() {
                match line.get(i) {
                    Some(b) => {
                        if !is_blank(*b) {
                            i += 1;
                            moved = true;
                        } else {
                            break;
                        }
                    }
                    None => break,
                }
            }
            if !moved {
                break;
            }
            f = f.saturating_add(1);
        }
        while i < line.len() {
            match line.get(i) {
                Some(b) => {
                    if is_blank(*b) {
                        i += 1;
                    } else {
                        break;
                    }
                }
                None => break,
            }
        }
        let mut n = 0usize;
        while i < line.len() {
            if self.width != UNLIMITED && (n as u32) >= self.width {
                break;
            }
            if n >= KEYMAX {
                return Err(UQ_EKEY);
            }
            let b = match line.get(i) {
                Some(v) => *v,
                None => break,
            };
            let c = if self.icase {
                if b >= 0x80 {
                    return Err(UQ_ECHAR);
                }
                b.to_ascii_lowercase()
            } else {
                b
            };
            if let Some(dst) = out.get_mut(n) {
                *dst = c;
            }
            n += 1;
            i += 1;
        }
        if i < line.len() && self.width == UNLIMITED {
            return Err(UQ_EKEY);
        }
        Ok((out, n))
    }
}

/// 一组的账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Group {
    /// 组内行数。
    pub n: u32,
    /// 组代表行的字节长度。
    pub out_len: u32,
}

/// 一趟去重。
#[derive(Clone, Copy, Debug)]
pub struct Run {
    p: Proc,
    groups: [Group; CAP],
    n: usize,
    lines: u32,
    last: [u8; KEYMAX],
    last_len: usize,
}

impl Run {
    /// 起手。
    pub const fn new(p: Proc) -> Run {
        Run { p, groups: [Group { n: 0, out_len: 0 }; CAP], n: 0, lines: 0, last: [0; KEYMAX], last_len: 0 }
    }

    /// 参数。
    pub fn proc(&self) -> Proc {
        self.p
    }

    /// 已并入的行数。
    pub fn lines(&self) -> u32 {
        self.lines
    }

    /// 组数。
    pub fn groups(&self) -> usize {
        self.n
    }

    /// 第 i 组。
    pub fn group_at(&self, i: usize) -> Option<Group> {
        if i >= self.n {
            return None;
        }
        self.groups.get(i).copied()
    }

    /// 末组；空输入报无行。
    pub fn last(&self) -> Result<Group, u32> {
        if self.n == 0 {
            return Err(UQ_ENOLINE);
        }
        match self.groups.get(self.n - 1) {
            Some(g) => Ok(*g),
            None => Err(UQ_ENOLINE),
        }
    }

    /// 相邻合并：键等于末组键就并入，否则开新组。
    pub fn push(&mut self, line: &[u8]) -> Result<u32, u32> {
        let (key, klen) = self.p.key(line)?;
        let out_len = if line.len() > 0xffff { 0xffff } else { line.len() as u32 };
        if self.n > 0 && klen == self.last_len {
            let mut same = true;
            let mut i = 0usize;
            while i < klen {
                let a = key.get(i).copied();
                let b = self.last.get(i).copied();
                if a != b {
                    same = false;
                    break;
                }
                i += 1;
            }
            if same {
                if let Some(g) = self.groups.get_mut(self.n - 1) {
                    g.n = g.n.saturating_add(1);
                    self.lines = self.lines.saturating_add(1);
                    return Ok(g.n);
                }
            }
        }
        if self.n >= CAP {
            return Err(UQ_EFULL);
        }
        if let Some(dst) = self.groups.get_mut(self.n) {
            *dst = Group { n: 1, out_len };
        }
        self.last = key;
        self.last_len = klen;
        self.n += 1;
        self.lines = self.lines.saturating_add(1);
        Ok(1)
    }

    /// 输出记账：返回字节数（`-c` 时含 8 字节前缀）。
    pub fn emit(&self) -> Result<u32, u32> {
        let mut bytes = 0u32;
        let mut i = 0usize;
        while i < self.n {
            let g = match self.groups.get(i) {
                Some(v) => *v,
                None => break,
            };
            if self.p.wanted(g.n) {
                let per = if self.p.count {
                    COUNT_W.saturating_add(g.out_len).saturating_add(1)
                } else {
                    g.out_len.saturating_add(1)
                };
                bytes = bytes.saturating_add(per);
            }
            i += 1;
        }
        if bytes > OUT_CAP {
            return Err(UQ_EBYTES);
        }
        Ok(bytes)
    }

    /// 输出行数。
    pub fn emit_lines(&self) -> u32 {
        let mut k = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(g) = self.groups.get(i) {
                if self.p.wanted(g.n) {
                    k = k.saturating_add(1);
                }
            }
            i += 1;
        }
        k
    }

    /// 重复组数。
    pub fn dups(&self) -> u32 {
        let mut k = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(g) = self.groups.get(i) {
                if g.n > 1 {
                    k = k.saturating_add(1);
                }
            }
            i += 1;
        }
        k
    }

    /// 唯一组数。
    pub fn uniques(&self) -> u32 {
        self.n as u32 - self.dups()
    }

    /// 不变式：组计数之和等于读入行数。
    pub fn check(&self) -> Result<u32, u32> {
        if self.n > CAP {
            return Err(UQ_EFULL);
        }
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.n {
            match self.groups.get(i) {
                Some(g) => {
                    if g.n == 0 {
                        return Err(UQ_ENOLINE);
                    }
                    sum = sum.saturating_add(g.n);
                }
                None => return Err(UQ_ENOLINE),
            }
            i += 1;
        }
        if sum != self.lines {
            return Err(UQ_EKEY);
        }
        Ok(self.n as u32)
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
            return Err(UQ_ESLOT);
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
            return Err(UQ_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 1, 3, 4, 2, 20, 2];
        const CODES: [u32; 6] =
            [UQ_EEXCL, UQ_EWIDTH, UQ_EFIELDS, UQ_EKEY, UQ_ECHAR, UQ_EFULL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(UQ_EBAL),
            };
            if s.slot as usize != i {
                return Err(UQ_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(UQ_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(UQ_EVALUE);
                        }
                    }
                    None => return Err(UQ_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(UQ_EVALUE);
                        }
                    }
                    None => return Err(UQ_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(UQ_EBAL);
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
            return Err(UQ_EBAL);
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
            Err(UQ_ESEAL)
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

/// 超宽行（65 字节）。
const WIDE: [u8; 65] = [b'z'; 65];
/// 九行互不相同的输入。
const NINE: [&[u8]; 9] = [
    b"a", b"b", b"c", b"d", b"e", b"f", b"g", b"h", b"i",
];
/// 非 ASCII 字节（`é` 的前导字节）。
const NONASCII: &[u8] = b"\xc3\xa9";

/// 起手。
fn mk(opts: Opts) -> Result<Proc, u32> {
    opts.build()
}

/// 十四格账本：三推 → 行数 → 两种筛选 → 计数 → 组数 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let plain = match mk(Opts::new()) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let mut r = Run::new(plain);
    let _ = l.register(word_cell(0, OP_PUSH, r.push(b"a")));
    let _ = l.register(word_cell(1, OP_PUSH, r.push(b"a")));
    let _ = l.register(word_cell(2, OP_PUSH, r.push(b"b")));
    let _ = l.register(word_cell(3, OP_READ, Ok(r.lines())));
    let _ = l.register(word_cell(4, OP_EMIT, r.emit()));

    let dup = match mk(Opts::new().dup_only()) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let mut dr = Run::new(dup);
    let _ = dr.push(b"a");
    let _ = dr.push(b"a");
    let _ = dr.push(b"b");
    let _ = l.register(word_cell(5, OP_EMIT, dr.emit()));

    let counted = match mk(Opts::new().counted()) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let mut cr = Run::new(counted);
    let _ = cr.push(b"a");
    let _ = cr.push(b"a");
    let _ = cr.push(b"b");
    let _ = l.register(word_cell(6, OP_EMIT, cr.emit()));
    let _ = l.register(word_cell(7, OP_READ, Ok(r.groups() as u32)));

    let _ = l.register(word_cell(
        8,
        OP_BUILD,
        mk(Opts::new().dup_only().uniq_only()).map(|_| 0),
    ));
    let _ = l.register(word_cell(
        9,
        OP_BUILD,
        mk(Opts::new().width(KEYMAX as u32 + 1)).map(|_| 0),
    ));
    let _ = l.register(word_cell(
        10,
        OP_BUILD,
        mk(Opts::new().skip(FIELD_MAX + 1)).map(|_| 0),
    ));
    let _ = l.register(word_cell(11, OP_PUSH, r.push(&WIDE)));

    let icase = match mk(Opts::new().icase()) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let mut ir = Run::new(icase);
    let _ = l.register(word_cell(12, OP_PUSH, ir.push(NONASCII)));

    let mut nine = Run::new(plain);
    let mut e = 0u32;
    let mut i = 0usize;
    while i < NINE.len() {
        if let Some(w) = NINE.get(i) {
            match nine.push(w) {
                Ok(_) => {}
                Err(c) => {
                    e = c;
                    break;
                }
            }
        }
        i += 1;
    }
    let _ = l.register(word_cell(13, OP_PUSH, Err::<u32, u32>(e)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：只合并相邻行。
    let plain = match mk(Opts::new()) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let mut a = Run::new(plain);
    let _ = a.push(b"a");
    let _ = a.push(b"b");
    let _ = a.push(b"a");
    out[1] = a.groups() == 3
        && a.lines() == 3
        && a.dups() == 0
        && a.uniques() == 3
        && a.check() == Ok(3)
        && a.last().map(|g| g.n) == Ok(1)
        && Run::new(plain).last() == Err(UQ_ENOLINE);

    // 二路：三条键规则各自的归组。
    let mut ic = Run::new(match mk(Opts::new().icase()) {
        Ok(v) => v,
        Err(_) => return out,
    });
    let _ = ic.push(b"Abc");
    let _ = ic.push(b"aBC");
    let mut w2 = Run::new(match mk(Opts::new().width(2)) {
        Ok(v) => v,
        Err(_) => return out,
    });
    let _ = w2.push(b"xy1");
    let _ = w2.push(b"xy2");
    let mut w0 = Run::new(match mk(Opts::new().width(0)) {
        Ok(v) => v,
        Err(_) => return out,
    });
    let _ = w0.push(b"p");
    let _ = w0.push(b"q");
    let mut sk = Run::new(match mk(Opts::new().skip(1)) {
        Ok(v) => v,
        Err(_) => return out,
    });
    let _ = sk.push(b"f a");
    let _ = sk.push(b"g a");
    out[2] = ic.groups() == 1
        && ic.last().map(|g| g.n) == Ok(2)
        && w2.groups() == 1
        && w0.groups() == 1
        && w0.last().map(|g| g.n) == Ok(2)
        && sk.groups() == 1
        && sk.check() == Ok(1);

    // 三路：八类域错的七类。
    let excl = mk(Opts::new().dup_only().uniq_only()) == Err(UQ_EEXCL);
    let wide = mk(Opts::new().width(KEYMAX as u32 + 1)) == Err(UQ_EWIDTH);
    let fields = mk(Opts::new().skip(FIELD_MAX + 1)) == Err(UQ_EFIELDS);
    let key = {
        let mut x = Run::new(plain);
        x.push(&WIDE) == Err(UQ_EKEY)
    };
    let chr = {
        let mut x = Run::new(match mk(Opts::new().icase()) {
            Ok(v) => v,
            Err(_) => return out,
        });
        x.push(NONASCII) == Err(UQ_ECHAR)
            && x.push(b"\xc3") == Err(UQ_ECHAR)
    };
    let full = {
        let mut x = Run::new(plain);
        let mut i = 0usize;
        let mut code = 0u32;
        while i < NINE.len() {
            if let Some(w) = NINE.get(i) {
                match x.push(w) {
                    Ok(_) => {}
                    Err(c) => {
                        code = c;
                        break;
                    }
                }
            }
            i += 1;
        }
        code == UQ_EFULL && x.groups() == CAP
    };
    let bytes = {
        let c = match mk(Opts::new().counted()) {
            Ok(v) => v,
            Err(_) => return out,
        };
        let mut x = Run::new(c);
        let mut i = 0usize;
        while i < CAP {
            if let Some(w) = MATRIX.get(i) {
                let _ = x.push(w);
            }
            i += 1;
        }
        x.emit() == Err(UQ_EBYTES) && x.emit_lines() == CAP as u32
    };
    out[3] = excl && wide && fields && key && chr && full && bytes;

    out
}

/// 八个 64 字节的互不相同行（用于顶到输出预算）。
const MATRIX: [&[u8]; 8] = [
    b"0000000000000000000000000000000000000000000000000000000000000000",
    b"1111111111111111111111111111111111111111111111111111111111111111",
    b"2222222222222222222222222222222222222222222222222222222222222222",
    b"3333333333333333333333333333333333333333333333333333333333333333",
    b"4444444444444444444444444444444444444444444444444444444444444444",
    b"5555555555555555555555555555555555555555555555555555555555555555",
    b"6666666666666666666666666666666666666666666666666666666666666666",
    b"7777777777777777777777777777777777777777777777777777777777777777",
];

/// 十三项判据。
pub fn run_unx11515_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11515.uniq_adjacent");
    let l = build_ledger();

    s.add_dyn(
        "UQ1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "UQ2-只合并相邻行",
        {
            let mut r = Run::new(match mk(Opts::new()) {
                Ok(v) => v,
                Err(_) => return s,
            });
            let _ = r.push(b"a");
            let _ = r.push(b"b");
            let _ = r.push(b"a");
            r.groups() == 3 && r.lines() == 3 && r.check() == Ok(3) && l.sample(3).map(|x| x.value) == Some(3)
        },
        "static",
    );

    s.add(
        "UQ3-相邻计数累加",
        {
            let mut r = Run::new(match mk(Opts::new()) {
                Ok(v) => v,
                Err(_) => return s,
            });
            let a = r.push(b"a");
            let b = r.push(b"a");
            let c = r.push(b"a");
            let d = r.push(b"b");
            a == Ok(1)
                && b == Ok(2)
                && c == Ok(3)
                && d == Ok(1)
                && r.last().map(|g| g.n) == Ok(1)
                && r.group_at(0).map(|g| g.n) == Some(3)
                && l.sample(1).map(|x| x.value) == Some(2)
        },
        "static",
    );

    s.add(
        "UQ4-计数前缀八字节",
        {
            let mut r = Run::new(match mk(Opts::new().counted()) {
                Ok(v) => v,
                Err(_) => return s,
            });
            let _ = r.push(b"a");
            let _ = r.push(b"a");
            let _ = r.push(b"b");
            r.emit() == Ok(20)
                && r.emit_lines() == 2
                && COUNT_W == 8
                && l.sample(6).map(|x| x.value) == Some(20)
        },
        "static",
    );

    s.add(
        "UQ5-三选一互斥",
        Opts::new().dup_only().uniq_only().build() == Err(UQ_EEXCL)
            && Opts::new().dup_only().build().is_ok()
            && Opts::new().uniq_only().build().is_ok()
            && l.sample(8).map(|x| x.code) == Some(UQ_EEXCL),
        "static",
    );

    s.add(
        "UQ6-d与u筛选",
        {
            let mut r = Run::new(match mk(Opts::new()) {
                Ok(v) => v,
                Err(_) => return s,
            });
            let _ = r.push(b"a");
            let _ = r.push(b"a");
            let _ = r.push(b"b");
            let _ = r.push(b"c");
            let _ = r.push(b"c");
            let _ = r.push(b"c");
            r.dups() == 2
                && r.uniques() == 1
                && r.emit() == Ok(6)
                && l.sample(5).map(|x| x.value) == Some(2)
        },
        "static",
    );

    s.add(
        "UQ7-折叠只在ASCII",
        {
            let mut r = Run::new(match mk(Opts::new().icase()) {
                Ok(v) => v,
                Err(_) => return s,
            });
            let _ = r.push(b"Abc");
            let _ = r.push(b"aBC");
            r.groups() == 1
                && r.last().map(|g| g.n) == Ok(2)
                && r.emit() == Ok(4)
                && r.check() == Ok(1)
                && l.sample(12).map(|x| x.code) == Some(UQ_ECHAR)
        },
        "static",
    );

    s.add(
        "UQ8-宽度截断与零宽",
        {
            let mut two = Run::new(match mk(Opts::new().width(2)) {
                Ok(v) => v,
                Err(_) => return s,
            });
            let _ = two.push(b"xy1");
            let _ = two.push(b"xy2");
            let mut zero = Run::new(match mk(Opts::new().width(0)) {
                Ok(v) => v,
                Err(_) => return s,
            });
            let _ = zero.push(b"a");
            let _ = zero.push(b"b");
            let _ = zero.push(b"c");
            two.groups() == 1
                && zero.groups() == 1
                && zero.last().map(|g| g.n) == Ok(3)
                && zero.check() == Ok(1)
        },
        "static",
    );

    s.add(
        "UQ9-跳过前n字段",
        {
            let mut f = Run::new(match mk(Opts::new().skip(1)) {
                Ok(v) => v,
                Err(_) => return s,
            });
            let _ = f.push(b"f a");
            let _ = f.push(b"g a");
            let mut g = Run::new(match mk(Opts::new().skip(2)) {
                Ok(v) => v,
                Err(_) => return s,
            });
            let _ = g.push(b"x y z");
            let _ = g.push(b"p q z");
            f.groups() == 1 && g.groups() == 1 && f.check() == Ok(1)
        },
        "static",
    );

    s.add(
        "UQ10-键宽上限",
        {
            let mut r = Run::new(match mk(Opts::new()) {
                Ok(v) => v,
                Err(_) => return s,
            });
            let wide = r.push(&WIDE) == Err(UQ_EKEY);
            let mut ok = Run::new(match mk(Opts::new().width(KEYMAX as u32)) {
                Ok(v) => v,
                Err(_) => return s,
            });
            wide
                && ok.push(&WIDE) == Ok(1)
                && ok.groups() == 1
                && l.sample(11).map(|x| x.code) == Some(UQ_EKEY)
        },
        "static",
    );

    s.add(
        "UQ11-组表满八组",
        {
            let mut r = Run::new(match mk(Opts::new()) {
                Ok(v) => v,
                Err(_) => return s,
            });
            let mut i = 0usize;
            let mut last = Ok(0);
            while i < NINE.len() {
                if let Some(w) = NINE.get(i) {
                    last = r.push(w);
                }
                i += 1;
            }
            last == Err(UQ_EFULL) && r.groups() == CAP && l.sample(13).map(|x| x.code) == Some(UQ_EFULL)
        },
        "static",
    );

    s.add(
        "UQ12-输出预算与空输入",
        {
            let mut r = Run::new(match mk(Opts::new().counted()) {
                Ok(v) => v,
                Err(_) => return s,
            });
            let mut i = 0usize;
            while i < CAP {
                if let Some(w) = MATRIX.get(i) {
                    let _ = r.push(w);
                }
                i += 1;
            }
            let over = r.emit() == Err(UQ_EBYTES);
            let mut small = Run::new(match mk(Opts::new()) {
                Ok(v) => v,
                Err(_) => return s,
            });
            let _ = small.push(b"a");
            over
                && r.emit_lines() == CAP as u32
                && Run::new(match mk(Opts::new()) {
                    Ok(v) => v,
                    Err(_) => return s,
                })
                .last()
                == Err(UQ_ENOLINE)
                && small.emit() == Ok(2)
                && Run::new(match mk(Opts::new()) {
                    Ok(v) => v,
                    Err(_) => return s,
                })
                .emit()
                == Ok(0)
        },
        "static",
    );

    s.add(
        "UQ13-冻结重放四注入",
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

    fn run(opts: Opts) -> Run {
        match opts.build() {
            Ok(p) => Run::new(p),
            Err(_) => panic!("build"),
        }
    }

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
        let s = run_unx11515_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn adjacent_only() {
        let mut r = run(Opts::new());
        assert_eq!(r.push(b"a"), Ok(1));
        assert_eq!(r.push(b"b"), Ok(1));
        assert_eq!(r.push(b"a"), Ok(1));
        assert_eq!(r.groups(), 3);
        assert_eq!(r.check(), Ok(3));
    }

    #[test]
    fn counts_and_bytes() {
        let mut r = run(Opts::new().counted());
        assert_eq!(r.push(b"a"), Ok(1));
        assert_eq!(r.push(b"a"), Ok(2));
        assert_eq!(r.push(b"b"), Ok(1));
        assert_eq!(r.emit(), Ok(20));
        assert_eq!(r.emit_lines(), 2);
        assert_eq!(r.dups(), 1);
        assert_eq!(r.uniques(), 1);
    }

    #[test]
    fn key_rules() {
        let two = run(Opts::new().width(2));
        let mut a = two;
        assert_eq!(a.push(b"xy1"), Ok(1));
        assert_eq!(a.push(b"xy2"), Ok(2));
        let mut z = run(Opts::new().width(0));
        assert_eq!(z.push(b"a"), Ok(1));
        assert_eq!(z.push(b"b"), Ok(2));
        let mut f = run(Opts::new().skip(1));
        assert_eq!(f.push(b"f a"), Ok(1));
        assert_eq!(f.push(b"g a"), Ok(2));
    }

    #[test]
    fn icase_is_ascii_only() {
        let mut r = run(Opts::new().icase());
        assert_eq!(r.push(b"Abc"), Ok(1));
        assert_eq!(r.push(b"aBC"), Ok(2));
        assert_eq!(r.push(b"\xc3\xa9"), Err(UQ_ECHAR));
    }

    #[test]
    fn capacity_and_budget() {
        let mut r = run(Opts::new());
        let mut last = Ok(0);
        let mut i = 0usize;
        while i < NINE.len() {
            if let Some(w) = NINE.get(i) {
                last = r.push(w);
            }
            i += 1;
        }
        assert_eq!(last, Err(UQ_EFULL));
        assert_eq!(r.groups(), CAP);
        let mut c = run(Opts::new().counted());
        let mut k = 0usize;
        while k < CAP {
            if let Some(w) = MATRIX.get(k) {
                let _ = c.push(w);
            }
            k += 1;
        }
        assert_eq!(c.emit(), Err(UQ_EBYTES));
    }

    #[test]
    fn inject_four_is_all_true() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
