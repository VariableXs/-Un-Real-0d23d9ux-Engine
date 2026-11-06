//! UNX-F11525 od 八进制转储（0xf1e0 段）
//!
//! `od` 把一段字节按「地址列 + 若干个定宽字」排成行。地址列基数、字类型、
//! 字宽三者互相独立，任意组合都必须自洽：地址基数只影响地址怎么打印，类型
//! 只影响字怎么解释，宽度只影响一行的字节数。把它们揉成一个「模式」会让
//! `-A x -t o2` 这类组合无从表达。
//!
//! 本模块的行模型：
//!
//! - 每行最多 `WPC` 个字，字宽 `1/2/4`，因此一行最多 16 字节；
//! - 尾部不足一个字宽时，本行只记完整字，剩余字节数记入「短尾」，不补零
//!   也不丢弃 —— 丢弃会让转储看起来比输入短；
//! - 相邻两行内容完全相同时，若开启重复压缩，只保留一行并用 `*` 表示省略，
//!   被省略的行不占行表槽位，但仍在统计里计数。
//!
//! 闸门次序：
//!
//! - `open`：上限为零 → 宽度非法 → 地址基数非法 → 类型非法；
//! - `dump`：跳过量越界 → 行表满；
//! - `render`：行号越界 → 该行尚未生成。
//!
//! 字按小端拼装：`w = b0 | b1 << 8 | b2 << 16 | b3 << 24`。这与 `od -t x2`
//! 在 x86 上的默认行为一致；大端目标若要不同的拼装方式，应当换一个类型码，
//! 而不是在渲染阶段改字节序。

use crate::alloc::{format, string::String};
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf1e0;

/// 参数非法。
pub const OD_EINVAL: u32 = SEG_BASE + 0x01;
/// 上限为零或跳过量越界。
pub const OD_ERANGE: u32 = SEG_BASE + 0x02;
/// 地址基数非法。
pub const OD_EADDR: u32 = SEG_BASE + 0x03;
/// 字类型非法。
pub const OD_ETYPE: u32 = SEG_BASE + 0x04;
/// 字宽非法。
pub const OD_EWIDTH: u32 = SEG_BASE + 0x05;
/// 行表满。
pub const OD_EFULL: u32 = SEG_BASE + 0x06;
/// 行号越界或行未生成。
pub const OD_ENOROW: u32 = SEG_BASE + 0x07;
/// 重复压缩状态非法。
pub const OD_EDUP: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const OD_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const OD_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const OD_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const OD_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const OD_ENOBS: u32 = SEG_BASE + 0x0d;

/// 行表容量。
pub const CAP: usize = 8;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 每行字槽数。
pub const WPC: usize = 4;

/// 地址基数：八进制。
pub const A_OCT: u8 = 1;
/// 地址基数：十进制。
pub const A_DEC: u8 = 2;
/// 地址基数：十六进制。
pub const A_HEX: u8 = 3;
/// 地址基数：不打印。
pub const A_NONE: u8 = 4;

/// 字类型：八进制。
pub const T_OCT: u8 = 1;
/// 字类型：十六进制。
pub const T_HEX: u8 = 2;
/// 字类型：字符。
pub const T_CHAR: u8 = 3;

/// 操作码：转储。
pub const OP_DUMP: u8 = 1;
/// 操作码：渲染。
pub const OP_RENDER: u8 = 2;
/// 操作码：拼字。
pub const OP_WORD: u8 = 3;
/// 操作码：统计。
pub const OP_STAT: u8 = 4;

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

/// 一行转储。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    /// 行首地址。
    pub off: u32,
    /// 本行字数。
    pub n: u8,
    /// 字值（小端拼装）。
    pub w: [u32; WPC],
}

/// 转储器。
#[derive(Clone, Debug)]
pub struct Od {
    addr: u8,
    kind: u8,
    width: u8,
    limit: usize,
    skip: usize,
    rows: [Row; CAP],
    n: usize,
    bytes: u32,
    words: u32,
    short: u32,
    folded: u32,
}

impl Od {
    /// 开户。
    pub fn open(addr: u8, kind: u8, width: u8, skip: u32, limit: u32) -> Result<Od, u32> {
        if limit == 0 {
            return Err(OD_ERANGE);
        }
        if width != 1 && width != 2 && width != 4 {
            return Err(OD_EWIDTH);
        }
        if addr < A_OCT || addr > A_NONE {
            return Err(OD_EADDR);
        }
        if kind < T_OCT || kind > T_CHAR {
            return Err(OD_ETYPE);
        }
        Ok(Od {
            addr,
            kind,
            width,
            limit: limit as usize,
            skip: skip as usize,
            rows: [Row { off: 0, n: 0, w: [0; WPC] }; CAP],
            n: 0,
            bytes: 0,
            words: 0,
            short: 0,
            folded: 0,
        })
    }

    /// 地址基数。
    pub fn addr_base(&self) -> u8 {
        self.addr
    }

    /// 字类型。
    pub fn kind(&self) -> u8 {
        self.kind
    }

    /// 字宽。
    pub fn width(&self) -> u8 {
        self.width
    }

    /// 一行字节数。
    pub fn line_bytes(&self) -> usize {
        WPC * self.width as usize
    }

    /// 已生成行数。
    pub fn rows(&self) -> usize {
        self.n
    }

    /// 已统计字节数。
    pub fn bytes(&self) -> u32 {
        self.bytes
    }

    /// 已统计字数。
    pub fn words(&self) -> u32 {
        self.words
    }

    /// 短尾行数。
    pub fn short_rows(&self) -> u32 {
        self.short
    }

    /// 被压缩掉的行数。
    pub fn folded_rows(&self) -> u32 {
        self.folded
    }

    /// 第 i 行。
    pub fn row(&self, i: usize) -> Option<Row> {
        if i >= self.n {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 拼字：把 `width` 个字节按小端拼成一个字。
    pub fn pack(bytes: &[u8], at: usize, width: u8) -> Result<u32, u32> {
        if width != 1 && width != 2 && width != 4 {
            return Err(OD_EWIDTH);
        }
        let mut v = 0u32;
        let mut i = 0usize;
        while i < width as usize {
            let b = match bytes.get(at + i) {
                Some(x) => *x,
                None => return Err(OD_ERANGE),
            };
            v |= (b as u32) << (8 * i as u32);
            i += 1;
        }
        Ok(v)
    }

    /// 执行转储，返回行数。
    pub fn dump(&mut self, data: &[u8], fold_dups: bool) -> Result<usize, u32> {
        if self.skip > data.len() {
            return Err(OD_ERANGE);
        }
        let body = match data.get(self.skip..) {
            Some(v) => v,
            None => return Err(OD_ERANGE),
        };
        let take = if body.len() > self.limit { self.limit } else { body.len() };
        if fold_dups && self.n == 0 && self.folded != 0 {
            return Err(OD_EDUP);
        }
        let w = self.width as usize;
        let mut pos = 0usize;
        while pos < take {
            let remain = take - pos;
            let full = remain / w;
            let cnt = if full > WPC { WPC } else { full };
            if cnt == 0 {
                break;
            }
            if self.n >= CAP {
                return Err(OD_EFULL);
            }
            let mut row = Row { off: (self.skip + pos) as u32, n: cnt as u8, w: [0; WPC] };
            let mut k = 0usize;
            while k < cnt {
                let v = Od::pack(body, pos + k * w, self.width)?;
                if let Some(dst) = row.w.get_mut(k) {
                    *dst = v;
                }
                k += 1;
            }
            let dup = match self.rows.get(self.n.wrapping_sub(1)) {
                Some(prev) if self.n > 0 => {
                    prev.n == row.n && prev.w == row.w
                }
                _ => false,
            };
            if fold_dups && dup {
                self.folded = self.folded.saturating_add(1);
            } else {
                if let Some(dst) = self.rows.get_mut(self.n) {
                    *dst = row;
                }
                self.n += 1;
            }
            if cnt < WPC {
                self.short = self.short.saturating_add(1);
            }
            self.words = self.words.saturating_add(cnt as u32);
            self.bytes = self.bytes.saturating_add((cnt * w) as u32);
            pos += cnt * w;
        }
        Ok(self.n)
    }

    /// 渲染第 i 行。
    pub fn render(&self, i: usize) -> Result<String, u32> {
        let r = match self.row(i) {
            Some(v) => v,
            None => return Err(OD_ENOROW),
        };
        let head = match self.addr {
            A_OCT => format!("{:07o}", r.off),
            A_DEC => format!("{:07}", r.off),
            A_HEX => format!("{:07x}", r.off),
            A_NONE => String::new(),
            _ => return Err(OD_EADDR),
        };
        let mut out = head;
        let mut k = 0usize;
        while k < r.n as usize {
            let v = match r.w.get(k) {
                Some(x) => *x,
                None => return Err(OD_EBAL),
            };
            let piece = match self.kind {
                T_OCT => match self.width {
                    1 => format!(" {:03o}", v & 0xff),
                    2 => format!(" {:06o}", v & 0xffff),
                    _ => format!(" {:011o}", v),
                },
                T_HEX => match self.width {
                    1 => format!(" {:02x}", v & 0xff),
                    2 => format!(" {:04x}", v & 0xffff),
                    _ => format!(" {:08x}", v),
                },
                T_CHAR => {
                    let b = (v & 0xff) as u8;
                    if b >= 0x20 && b < 0x7f {
                        format!("   {}", b as char)
                    } else {
                        format!("  \\{:03o}", b)
                    }
                }
                _ => return Err(OD_ETYPE),
            };
            out.push_str(&piece);
            k += 1;
        }
        if r.n == 0 {
            return Err(OD_EBAL);
        }
        Ok(out)
    }

    /// 行首地址。
    pub fn offset_of(&self, i: usize) -> Result<u32, u32> {
        match self.row(i) {
            Some(r) => Ok(r.off),
            None => Err(OD_ENOROW),
        }
    }

    /// 第 i 行第 k 个字。
    pub fn word_at(&self, i: usize, k: usize) -> Result<u32, u32> {
        match self.row(i) {
            Some(r) => {
                if k >= r.n as usize {
                    return Err(OD_ENOBS);
                }
                match r.w.get(k) {
                    Some(v) => Ok(*v),
                    None => Err(OD_ENOBS),
                }
            }
            None => Err(OD_ENOROW),
        }
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
            return Err(OD_ESLOT);
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
            return Err(OD_EBAL);
        }
        const VALS: [u32; 8] = [3, 65, 2, 8, 1, 2, 6, 16];
        const CODES: [u32; 6] = [OD_EWIDTH, OD_EADDR, OD_ETYPE, OD_EFULL, OD_ERANGE, OD_ENOROW];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(OD_EBAL),
            };
            if s.slot as usize != i {
                return Err(OD_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(OD_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(OD_EVALUE);
                        }
                    }
                    None => return Err(OD_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(OD_EVALUE);
                        }
                    }
                    None => return Err(OD_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(OD_EBAL);
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
            return Err(OD_EBAL);
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
            Err(OD_ESEAL)
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

/// 十四格账本：三种宽度、跳过与上限、渲染与六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let data = b"ABCDEFGHIJKL";

    let mut d1 = match Od::open(A_OCT, T_OCT, 1, 0, 64) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(count_cell(0, OP_DUMP, d1.dump(data, false)));
    let _ = l.register(word_cell(1, OP_WORD, d1.word_at(0, 0)));

    let mut d2 = match Od::open(A_OCT, T_OCT, 1, 4, 64) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(count_cell(2, OP_DUMP, d2.dump(data, false)));
    let _ = l.register(word_cell(3, OP_STAT, Ok(d2.bytes())));

    let mut d3 = match Od::open(A_OCT, T_OCT, 1, 0, 4) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(count_cell(4, OP_DUMP, d3.dump(data, false)));

    let mut d4 = match Od::open(A_OCT, T_OCT, 2, 0, 64) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(count_cell(5, OP_DUMP, d4.dump(data, false)));
    let _ = l.register(word_cell(6, OP_STAT, Ok(d4.words())));

    let mut d5 = match Od::open(A_OCT, T_OCT, 4, 0, 64) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = d5.dump(data, false);
    let _ = l.register(word_cell(7, OP_STAT, Ok(d5.line_bytes() as u32)));

    let width_bad = match Od::open(A_OCT, T_OCT, 3, 0, 8) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(Sample { slot: 8, op: OP_DUMP, ok: false, code: width_bad, value: 0 });

    let addr_bad = match Od::open(9, T_OCT, 1, 0, 8) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(Sample { slot: 9, op: OP_DUMP, ok: false, code: addr_bad, value: 0 });

    let kind_bad = match Od::open(A_OCT, 9, 1, 0, 8) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(Sample { slot: 10, op: OP_DUMP, ok: false, code: kind_bad, value: 0 });

    let mut big = match Od::open(A_OCT, T_OCT, 1, 0, 64) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let full = match big.dump(b"0123456789abcdefghijklmnopqrstuvwxyz", false) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(Sample { slot: 11, op: OP_DUMP, ok: false, code: full, value: 0 });

    let mut far = match Od::open(A_OCT, T_OCT, 1, 5, 8) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let range = match far.dump(b"abc", false) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(Sample { slot: 12, op: OP_DUMP, ok: false, code: range, value: 0 });

    let mut empty = match Od::open(A_OCT, T_OCT, 1, 0, 8) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = empty.dump(b"abc", false);
    let no_row = match empty.offset_of(3) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(Sample { slot: 13, op: OP_RENDER, ok: false, code: no_row, value: 0 });

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：默认转储的行数与字。
    let mut a = match Od::open(A_OCT, T_OCT, 1, 0, 64) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let rows = a.dump(b"ABCDEFGHIJKL", false) == Ok(3);
    let r0 = a.row(0) == Some(Row {
        off: 0,
        n: 4,
        w: [0o101, 0o102, 0o103, 0o104],
    });
    let r2 = a.row(2) == Some(Row {
        off: 8,
        n: 4,
        w: [0o111, 0o112, 0o113, 0o114],
    });
    let none3 = a.row(3).is_none();
    let off = a.offset_of(2) == Ok(8);
    out[1] = rows && r0 && r2 && none3 && off && a.bytes() == 12 && a.words() == 12;

    // 二路：宽度 2/4 的拼装与短尾。
    let mut b = match Od::open(A_HEX, T_HEX, 2, 0, 64) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let b_rows = b.dump(b"ABCDEFGHIJKL", false) == Ok(2);
    let bw = b.word_at(0, 0) == Ok(0x4241) && b.word_at(0, 1) == Ok(0x4443);
    let short2 = b.short_rows() == 1 && b.words() == 6;
    let render = b.render(1) == Ok(String::from("0000008 4a49 4c4b"));

    let mut c = match Od::open(A_DEC, T_CHAR, 1, 0, 64) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let _ = c.dump(b"A\x01", false);
    let cr = c.render(0) == Ok(String::from("0000000   A  \\001"));
    out[2] = b_rows && bw && short2 && render && cr;

    // 三路：跳过、上限、重复压缩与拒绝闸门。
    let skip_hi = Od::open(A_OCT, T_OCT, 1, 5, 8)
        .and_then(|mut v| v.dump(b"abc", false))
        == Err(OD_ERANGE);
    let width = matches!(Od::open(A_OCT, T_OCT, 3, 0, 8), Err(OD_EWIDTH));
    let addr = matches!(Od::open(0, T_OCT, 1, 0, 8), Err(OD_EADDR));
    let kind = matches!(Od::open(A_OCT, 0, 1, 0, 8), Err(OD_ETYPE));
    let zero = matches!(Od::open(A_OCT, T_OCT, 1, 0, 0), Err(OD_ERANGE));

    let mut f = match Od::open(A_OCT, T_OCT, 1, 0, 64) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let _ = f.dump(b"AAAAAAAA", true);
    let folded = f.rows() == 1 && f.folded_rows() == 1;

    let mut led = build_ledger();
    let one = led.freeze();
    let rep = led.replay();
    let two = led.freeze();
    let reg = led.register(Sample::blank());
    out[3] = skip_hi
        && width
        && addr
        && kind
        && zero
        && folded
        && one.is_ok()
        && rep == one
        && two.is_err()
        && reg == Err(OD_ESLOT)
        && led.is_frozen()
        && led.rejects() == 2;

    out
}

/// 十三项判据。
pub fn run_unx11525_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11525.od_dump");

    s.add(
        "OD01-默认八进制转储",
        {
            let mut good = false;
            if let Ok(mut o) = Od::open(A_OCT, T_OCT, 1, 0, 64) {
                good = o.dump(b"ABCDEFGHIJKL", false) == Ok(3)
                    && o.row(0)
                        == Some(Row { off: 0, n: 4, w: [0o101, 0o102, 0o103, 0o104] })
                    && o.row(2)
                        == Some(Row { off: 8, n: 4, w: [0o111, 0o112, 0o113, 0o114] })
                    && o.bytes() == 12
                    && o.words() == 12;
            }
            good
        },
        "static",
    );

    s.add(
        "OD02-地址基数三档",
        {
            let mut good = false;
            if let Ok(mut a) = Od::open(A_OCT, T_OCT, 1, 0, 8) {
                let _ = a.dump(b"ABCD", false);
                if let Ok(mut b) = Od::open(A_DEC, T_OCT, 1, 0, 8) {
                    let _ = b.dump(b"ABCD", false);
                    if let Ok(mut c) = Od::open(A_HEX, T_OCT, 1, 0, 8) {
                        let _ = c.dump(b"ABCD", false);
                        if let Ok(mut n) = Od::open(A_NONE, T_OCT, 1, 0, 8) {
                            let _ = n.dump(b"ABCD", false);
                            good = a.render(0)
                                == Ok(String::from("0000000 101 102 103 104"))
                                && b.render(0) == Ok(String::from("0000000 101 102 103 104"))
                                && c.render(0) == Ok(String::from("0000000 101 102 103 104"))
                                && n.render(0) == Ok(String::from(" 101 102 103 104"));
                        }
                    }
                }
            }
            good
        },
        "static",
    );

    s.add(
        "OD03-十六进制字类型",
        {
            let mut good = false;
            if let Ok(mut o) = Od::open(A_HEX, T_HEX, 2, 0, 8) {
                good = o.dump(b"ABCD", false) == Ok(1)
                    && o.word_at(0, 0) == Ok(0x4241)
                    && o.word_at(0, 1) == Ok(0x4443)
                    && o.render(0) == Ok(String::from("0000000 4241 4443"));
            }
            good
        },
        "static",
    );

    s.add(
        "OD04-字符类型与转义",
        {
            let mut good = false;
            if let Ok(mut o) = Od::open(A_DEC, T_CHAR, 1, 0, 8) {
                good = o.dump(b"A\x01", false) == Ok(1)
                    && o.render(0) == Ok(String::from("0000000   A  \\001"))
                    && Od::pack(b"A", 0, 1) == Ok(65);
            }
            good
        },
        "static",
    );

    s.add(
        "OD05-宽度与拼装",
        {
            Od::pack(b"ABCD", 0, 4) == Ok(0x4443_4241)
                && Od::pack(b"ABCD", 2, 2) == Ok(0x4443)
                && Od::pack(b"ABCD", 0, 3) == Err(OD_EWIDTH)
                && Od::pack(b"AB", 1, 2) == Err(OD_ERANGE)
        },
        "static",
    );

    s.add(
        "OD06-上限截断",
        {
            let mut good = false;
            if let Ok(mut o) = Od::open(A_OCT, T_OCT, 1, 0, 4) {
                good = o.dump(b"ABCDEFGH", false) == Ok(1)
                    && o.bytes() == 4
                    && o.folded_rows() == 0;
            }
            good
        },
        "static",
    );

    s.add(
        "OD07-跳过与越界",
        {
            let mut good = false;
            if let Ok(mut o) = Od::open(A_OCT, T_OCT, 1, 4, 64) {
                good = o.dump(b"ABCDEFGHIJKL", false) == Ok(2)
                    && o.offset_of(0) == Ok(4)
                    && o.bytes() == 8;
            }
            good
                && Od::open(A_OCT, T_OCT, 1, 5, 8)
                    .and_then(|mut v| v.dump(b"abc", false))
                    == Err(OD_ERANGE)
        },
        "static",
    );

    s.add(
        "OD08-短尾与行表满",
        {
            let mut good = false;
            if let Ok(mut o) = Od::open(A_OCT, T_OCT, 2, 0, 64) {
                good = o.dump(b"ABCDEFGHIJKL", false) == Ok(2)
                    && o.short_rows() == 1
                    && o.words() == 6
                    && o.row(1).map(|r| r.n) == Some(2);
            }
            let mut full = false;
            if let Ok(mut b) = Od::open(A_OCT, T_OCT, 1, 0, 64) {
                full = b.dump(b"0123456789abcdefghijklmnopqrstuvwxyz", false) == Err(OD_EFULL)
                    && b.rows() == CAP;
            }
            good && full
        },
        "static",
    );

    s.add(
        "OD09-账本审计",
        {
            let l = build_ledger();
            l.len() == SAMPLES
                && l.ok_count() == 8
                && l.err_count() == 6
                && l.audit().is_ok()
                && l.rejects() == 0
        },
        "static",
    );

    s.add(
        "OD10-账本错误行码序",
        {
            let l = build_ledger();
            let mut got = [0u32; 6];
            let mut ci = 0usize;
            let mut i = 0usize;
            let mut good = true;
            while i < l.len() {
                match l.sample(i) {
                    Some(smp) => {
                        if !smp.ok {
                            if let Some(dst) = got.get_mut(ci) {
                                *dst = smp.code;
                            }
                            ci += 1;
                        }
                    }
                    None => good = false,
                }
                i += 1;
            }
            good
                && ci == 6
                && got == [OD_EWIDTH, OD_EADDR, OD_ETYPE, OD_EFULL, OD_ERANGE, OD_ENOROW]
        },
        "static",
    );

    s.add(
        "OD11-冻结与拒绝计数",
        {
            let mut l = build_ledger();
            let a = l.freeze();
            let b = l.freeze();
            let c = l.register(Sample::blank());
            a.is_ok() && b == Err(OD_EBAL) && c == Err(OD_ESLOT) && l.rejects() == 2
        },
        "static",
    );

    s.add(
        "OD12-重放指纹一致",
        {
            let l = build_ledger();
            let a = l.seal();
            let mut f = build_ledger();
            let b = f.freeze();
            a != 0 && l.replay() == Ok(a) && b == Ok(a) && f.root() == a
        },
        "static",
    );

    s.add(
        "OD13-冻结重放四注入",
        {
            let inj = inject_four();
            inj == [true; 4] && build_ledger().audit().is_ok()
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
        assert_eq!(l.sample(0).map(|s| s.slot), Some(0));
        assert_eq!(l.sample(SAMPLES), None);
    }

    #[test]
    fn default_dump_shape() {
        let mut o = Od::open(A_OCT, T_OCT, 1, 0, 64).ok().unwrap();
        assert_eq!(o.dump(b"ABCDEFGHIJKL", false), Ok(3));
        assert_eq!(o.row(0), Some(Row { off: 0, n: 4, w: [0o101, 0o102, 0o103, 0o104] }));
        assert_eq!(o.row(2), Some(Row { off: 8, n: 4, w: [0o111, 0o112, 0o113, 0o114] }));
        assert_eq!(o.row(3), None);
        assert_eq!(o.bytes(), 12);
    }

    #[test]
    fn address_rendering() {
        let mut oct = Od::open(A_OCT, T_OCT, 1, 0, 8).ok().unwrap();
        assert_eq!(oct.dump(b"ABCD", false), Ok(1));
        assert_eq!(oct.render(0), Ok(String::from("0000000 101 102 103 104")));
        let mut hex = Od::open(A_HEX, T_HEX, 2, 0, 8).ok().unwrap();
        assert_eq!(hex.dump(b"ABCD", false), Ok(1));
        assert_eq!(hex.render(0), Ok(String::from("0000000 4241 4443")));
        let mut none = Od::open(A_NONE, T_OCT, 1, 0, 8).ok().unwrap();
        assert_eq!(none.dump(b"AB", false), Ok(1));
        assert_eq!(none.render(0), Ok(String::from(" 101 102")));
    }

    #[test]
    fn char_type_escapes() {
        let mut o = Od::open(A_DEC, T_CHAR, 1, 0, 8).ok().unwrap();
        assert_eq!(o.dump(b"A\x01", false), Ok(1));
        assert_eq!(o.render(0), Ok(String::from("0000000   A  \\001")));
    }

    #[test]
    fn width_packing_and_short_tail() {
        assert_eq!(Od::pack(b"ABCD", 0, 4), Ok(0x4443_4241));
        assert_eq!(Od::pack(b"ABCD", 2, 2), Ok(0x4443));
        assert_eq!(Od::pack(b"ABCD", 0, 3), Err(OD_EWIDTH));
        assert_eq!(Od::pack(b"AB", 1, 2), Err(OD_ERANGE));
        let mut o = Od::open(A_OCT, T_OCT, 2, 0, 64).ok().unwrap();
        assert_eq!(o.dump(b"ABCDEFGHIJKL", false), Ok(2));
        assert_eq!(o.short_rows(), 1);
        assert_eq!(o.words(), 6);
    }

    #[test]
    fn skip_limit_and_gates() {
        let mut o = Od::open(A_OCT, T_OCT, 1, 4, 64).ok().unwrap();
        assert_eq!(o.dump(b"ABCDEFGHIJKL", false), Ok(2));
        assert_eq!(o.offset_of(0), Ok(4));
        assert_eq!(
            Od::open(A_OCT, T_OCT, 1, 5, 8).and_then(|mut v| v.dump(b"abc", false)),
            Err(OD_ERANGE)
        );
        assert_eq!(Od::open(A_OCT, T_OCT, 3, 0, 8).err(), Some(OD_EWIDTH));
        assert_eq!(Od::open(0, T_OCT, 1, 0, 8).err(), Some(OD_EADDR));
        assert_eq!(Od::open(A_OCT, 0, 1, 0, 8).err(), Some(OD_ETYPE));
        assert_eq!(Od::open(A_OCT, T_OCT, 1, 0, 0).err(), Some(OD_ERANGE));
    }

    #[test]
    fn row_table_full() {
        let mut o = Od::open(A_OCT, T_OCT, 1, 0, 64).ok().unwrap();
        assert_eq!(o.dump(b"0123456789abcdefghijklmnopqrstuvwxyz", false), Err(OD_EFULL));
        assert_eq!(o.rows(), CAP);
        assert_eq!(o.offset_of(CAP), Err(OD_ENOROW));
    }

    #[test]
    fn freeze_and_replay() {
        let mut l = build_ledger();
        let r = l.freeze().ok().unwrap();
        assert_eq!(l.replay(), Ok(r));
        assert_eq!(l.freeze(), Err(OD_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(OD_ESLOT));
        assert_eq!(l.rejects(), 2);
        assert_eq!(l.replay(), Err(OD_ESEAL));
    }
}
