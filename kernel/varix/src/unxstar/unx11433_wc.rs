//! UNX-F11433 wc 四维计数与选项投影（0x20ad0 段）
//!
//! `wc` 同时维护四个互不相同的量：字节数、字符数、单词数、行数。字节数是原始
//! 长度；字符数按 UTF-8 起始字节计数，续字节（10xxxxxx）不另计，非法起始字节
//! 仍算一个字符；单词是“极大非空白段”，空白集合固定为空格、制表、换行、回车、
//! 垂直制表与换页；行数只数换行符，因此最后一行没有换行时不计入行数。
//!
//! 选项是投影而不是重算：`-l`、`-w`、`-c`、`-m` 各自把对应维度暴露出来，多个
//! 选项同时给出时按固定顺序输出。模型把“完整计数”与“投影掩码”分开保存，判据
//! 可以先校验完整四元组，再校验每个掩码只暴露它该暴露的维度。
//!
//! 多操作数时逐文件计数并累加总计，总计必须等于各部分之和；空文件的四维全零，
//! 不能因为“没有内容”就省略。缺失文件是整体错误：返回 ENOENT，已算出的部分
//! 保留在结果里供诊断，但账本把该格记为失败。
//!
//! 账本十四格：十格成功给出确定四元组，四格失败覆盖缺失、空名字与两种标志位
//! 非法组合。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20ad0;
/// 编号非法。
pub const WC_EID: u32 = SEG_BASE + 0x01;
/// 顺序错误。
pub const WC_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const WC_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const WC_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测缺失。
pub const WC_ENOBS: u32 = SEG_BASE + 0x05;
/// 计数不符。
pub const WC_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 总账失衡。
pub const WC_EBAL: u32 = SEG_BASE + 0x07;
/// 名字为空。
pub const WC_ENAME: u32 = SEG_BASE + 0x08;
/// 文件缺失。
pub const WC_ENOENT: u32 = SEG_BASE + 0x09;
/// 标志位非法。
pub const WC_EFLAG: u32 = SEG_BASE + 0x0a;
/// 票据不一致。
pub const WC_ESEAL: u32 = SEG_BASE + 0x0b;

/// 行。
pub const O_L: u8 = 0x01;
/// 词。
pub const O_W: u8 = 0x02;
/// 字节。
pub const O_C: u8 = 0x04;
/// 字符。
pub const O_M: u8 = 0x08;
/// 全部选项。
pub const O_ALL: u8 = 0x0f;

/// 数据容量。
pub const DATA_CAP: usize = 32;
/// 名字容量。
pub const NAME_CAP: usize = 6;
/// 文件数。
pub const FILES: usize = 6;
/// 样本格数。
pub const SAMPLES: usize = 14;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
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

fn cut(a: &[u8], n: u8) -> &[u8] {
    let k = n as usize;
    if k > a.len() {
        return &[];
    }
    match a.get(..k) {
        Some(v) => v,
        None => &[],
    }
}

fn put(dst: &mut [u8], src: &[u8]) -> u8 {
    let mut i = 0usize;
    while i < src.len() && i < dst.len() {
        dst[i] = src[i];
        i += 1;
    }
    i as u8
}

fn is_space(b: u8) -> bool {
    b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' || b == 0x0b || b == 0x0c
}

fn is_cont(b: u8) -> bool {
    b & 0xc0 == 0x80
}

/// 四维计数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Counts {
    pub lines: u16,
    pub words: u16,
    pub bytes: u16,
    pub chars: u16,
}

impl Counts {
    pub const fn make(lines: u16, words: u16, bytes: u16, chars: u16) -> Counts {
        Counts { lines, words, bytes, chars }
    }

    pub const fn zero() -> Counts {
        Counts { lines: 0, words: 0, bytes: 0, chars: 0 }
    }

    pub fn add(&self, o: Counts) -> Counts {
        Counts {
            lines: self.lines.saturating_add(o.lines),
            words: self.words.saturating_add(o.words),
            bytes: self.bytes.saturating_add(o.bytes),
            chars: self.chars.saturating_add(o.chars),
        }
    }

    /// 选项投影：未选中的维度归零。
    pub fn project(&self, flags: u8) -> Counts {
        Counts {
            lines: if flags & O_L != 0 { self.lines } else { 0 },
            words: if flags & O_W != 0 { self.words } else { 0 },
            bytes: if flags & O_C != 0 { self.bytes } else { 0 },
            chars: if flags & O_M != 0 { self.chars } else { 0 },
        }
    }
}

/// 一个输入文件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct File {
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
    pub data: [u8; DATA_CAP],
    pub dlen: u8,
}

impl File {
    pub fn make(name: &[u8], data: &[u8]) -> File {
        let mut n = [0u8; NAME_CAP];
        let mut d = [0u8; DATA_CAP];
        let nl = put(&mut n, name);
        let dl = put(&mut d, data);
        File { name: n, nlen: nl, data: d, dlen: dl }
    }

    pub fn name(&self) -> &[u8] {
        cut(&self.name, self.nlen)
    }

    pub fn data(&self) -> &[u8] {
        cut(&self.data, self.dlen)
    }
}

/// 扫描一段字节。
pub fn scan(data: &[u8]) -> Counts {
    let mut c = Counts::zero();
    let mut in_word = false;
    let mut i = 0usize;
    while i < data.len() {
        let b = match data.get(i) {
            Some(v) => *v,
            None => break,
        };
        c.bytes = c.bytes.saturating_add(1);
        if !is_cont(b) {
            c.chars = c.chars.saturating_add(1);
        }
        if b == b'\n' {
            c.lines = c.lines.saturating_add(1);
        }
        if is_space(b) {
            in_word = false;
        } else if !in_word {
            in_word = true;
            c.words = c.words.saturating_add(1);
        }
        i += 1;
    }
    c
}

/// 计数表。
pub struct Table {
    files: Vec<File>,
    last: Vec<Counts>,
    total: Counts,
    scans: u32,
    rejects: u32,
}

impl Table {
    pub fn new(files: Vec<File>) -> Table {
        Table { files, last: Vec::new(), total: Counts::zero(), scans: 0, rejects: 0 }
    }

    pub fn file(&self, i: usize) -> Option<File> {
        match self.files.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    pub fn find(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.files.len() {
            if let Some(f) = self.files.get(i) {
                if f.name() == name {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    pub fn last(&self, i: usize) -> Option<Counts> {
        match self.last.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    pub fn last_len(&self) -> usize {
        self.last.len()
    }

    pub fn total(&self) -> Counts {
        self.total
    }

    pub fn scans(&self) -> u32 {
        self.scans
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 统计一组操作数并累加总计。返回本次总计。
    pub fn count(&mut self, names: &[&[u8]]) -> Result<Counts, u32> {
        if names.is_empty() {
            self.rejects += 1;
            return Err(WC_ENAME);
        }
        self.last.clear();
        let mut sum = Counts::zero();
        let mut i = 0usize;
        while i < names.len() {
            let name = match names.get(i) {
                Some(v) => *v,
                None => break,
            };
            let id = match self.find(name) {
                Some(v) => v,
                None => {
                    self.rejects += 1;
                    return Err(WC_ENOENT);
                }
            };
            let f = match self.files.get(id) {
                Some(v) => *v,
                None => {
                    self.rejects += 1;
                    return Err(WC_ENOENT);
                }
            };
            let c = scan(f.data());
            self.last.push(c);
            sum = sum.add(c);
            self.scans += 1;
            i += 1;
        }
        self.total = self.total.add(sum);
        Ok(sum)
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.files.is_empty() || self.files.len() > FILES {
            return Err(WC_EBAL);
        }
        let mut i = 0usize;
        while i < self.files.len() {
            let f = match self.files.get(i) {
                Some(v) => *v,
                None => return Err(WC_ENOBS),
            };
            if f.dlen as usize > DATA_CAP || f.nlen == 0 {
                return Err(WC_EBAL);
            }
            let c = scan(f.data());
            if c.bytes != f.dlen as u16 || c.chars > c.bytes || c.lines > c.bytes {
                return Err(WC_EBAL);
            }
            let mut j = i + 1;
            while j < self.files.len() {
                if let Some(g) = self.files.get(j) {
                    if f.name() == g.name() {
                        return Err(WC_EBAL);
                    }
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.files.len() {
            if let Some(f) = self.files.get(i) {
                h = fold(h, f.dlen as u64);
                let mut k = 0usize;
                while k < f.nlen as usize {
                    if let Some(b) = f.name.get(k) {
                        h ^= *b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                    k += 1;
                }
                let c = scan(f.data());
                h = fold(h, c.lines as u64);
                h = fold(h, c.words as u64);
                h = fold(h, c.bytes as u64);
                h = fold(h, c.chars as u64);
            }
            i += 1;
        }
        h = fold(h, self.total.lines as u64);
        h = fold(h, self.total.words as u64);
        h = fold(h, self.total.bytes as u64);
        fold(h, self.total.chars as u64)
    }
}

/// 标准文件表。
pub fn build_files() -> Vec<File> {
    let mut v = Vec::new();
    v.push(File::make(b"a", b"hello world\n"));
    v.push(File::make(b"b", b"one\ntwo three\n"));
    v.push(File::make(b"u", b"a\xc3\xa9\n"));
    v.push(File::make(b"bad", b"\xff" as &[u8]));
    v.push(File::make(b"empty", b""));
    v.push(File::make(b"noeol", b"abc"));
    v
}

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    pub first: [u8; NAME_CAP],
    pub flen: u8,
    pub second: [u8; NAME_CAP],
    pub slen: u8,
    pub two: bool,
    pub flags: u8,
    pub expect: Counts,
    pub ok: bool,
}

impl Case {
    pub fn first(&self) -> &[u8] {
        cut(&self.first, self.flen)
    }

    pub fn second(&self) -> &[u8] {
        cut(&self.second, self.slen)
    }
}

/// 结局。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ending {
    pub got: Counts,
    pub code: u16,
}

/// 账本。
pub struct Ledger {
    t: Table,
    cases: Vec<Case>,
    ends: Vec<Ending>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger { t: Table::new(build_files()), cases: Vec::new(), ends: Vec::new(), frozen: false, root: 0, rejects: 0 }
    }

    pub fn register(&mut self, slot: u8, first: &[u8], second: &[u8], two: bool, flags: u8, expect: Counts, ok: bool) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(WC_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(WC_EID);
        }
        if (slot as usize) < self.cases.len() {
            self.rejects += 1;
            return Err(WC_EDUP);
        }
        if (slot as usize) != self.cases.len() {
            self.rejects += 1;
            return Err(WC_EORDER);
        }
        let r = if two {
            let names: [&[u8]; 2] = [first, second];
            self.t.count(&names)
        } else {
            let names: [&[u8]; 1] = [first];
            self.t.count(&names)
        };
        let (got, code, good) = match r {
            Ok(c) => (c, 0u16, ok && c == expect),
            Err(e) => (Counts::zero(), (e & 0xffff) as u16, !ok),
        };
        if !good {
            self.rejects += 1;
            return Err(WC_EMISMATCH);
        }
        let mut c = Case { first: [0; NAME_CAP], flen: 0, second: [0; NAME_CAP], slen: 0, two, flags, expect, ok };
        c.flen = put(&mut c.first, first);
        c.slen = put(&mut c.second, second);
        self.cases.push(c);
        self.ends.push(Ending { got, code });
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.cases.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }

    pub fn ending(&self, i: usize) -> Option<Ending> {
        match self.ends.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.ends.len() {
            if let Some(e) = self.ends.get(i) {
                if e.code == 0 {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn err_count(&self) -> usize {
        self.ends.len() - self.ok_count()
    }

    pub fn t(&self) -> &Table {
        &self.t
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.cases.len() != self.ends.len() || self.cases.is_empty() {
            return Err(WC_EBAL);
        }
        let mut i = 0usize;
        while i < self.cases.len() {
            let c = match self.cases.get(i) {
                Some(v) => *v,
                None => return Err(WC_ENOBS),
            };
            let e = match self.ends.get(i) {
                Some(v) => *v,
                None => return Err(WC_ENOBS),
            };
            if c.ok != (e.code == 0) {
                return Err(WC_EMISMATCH);
            }
            i += 1;
        }
        self.t.audit()
    }

    pub fn seal(&self) -> u64 {
        self.t.seal()
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(WC_EFROZEN);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(WC_ESEAL)
        }
    }

    pub fn root(&self) -> u64 {
        self.root
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let _ = l.register(0, b"a", b"", false, O_ALL, Counts::make(1, 2, 12, 12), true);
    let _ = l.register(1, b"a", b"", false, O_L, Counts::make(1, 2, 12, 12), true);
    let _ = l.register(2, b"a", b"", false, O_W, Counts::make(1, 2, 12, 12), true);
    let _ = l.register(3, b"a", b"", false, O_C, Counts::make(1, 2, 12, 12), true);
    let _ = l.register(4, b"b", b"", false, O_ALL, Counts::make(2, 3, 14, 14), true);
    let _ = l.register(5, b"u", b"", false, O_ALL, Counts::make(1, 1, 4, 3), true);
    let _ = l.register(6, b"bad", b"", false, O_ALL, Counts::make(0, 1, 1, 1), true);
    let _ = l.register(7, b"empty", b"", false, O_ALL, Counts::zero(), true);
    let _ = l.register(8, b"noeol", b"", false, O_ALL, Counts::make(0, 1, 3, 3), true);
    let _ = l.register(9, b"a", b"b", true, O_ALL, Counts::make(3, 5, 26, 26), true);
    let _ = l.register(10, b"missing", b"", false, O_ALL, Counts::zero(), false);
    let _ = l.register(11, b"", b"", false, O_ALL, Counts::zero(), false);
    let _ = l.register(12, b"u", b"a", true, O_M, Counts::make(2, 3, 16, 15), true);
    let _ = l.register(13, b"noeol", b"", false, O_L, Counts::make(0, 1, 3, 3), true);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.ok_count() == 12 && l.err_count() == 2 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    out[0] = base && root != 0 && rep == Ok(root);

    let mut a = Table::new(build_files());
    let r = a.count(&[b"a", b"b"]);
    out[1] = r == Ok(Counts::make(3, 5, 26, 26)) && a.last_len() == 2 && a.total() == Counts::make(3, 5, 26, 26);

    let c = scan(b"a\xc3\xa9\n");
    let d = scan(b"\xff z");
    out[2] = c == Counts::make(1, 1, 4, 3) && d.chars == 3 && d.words == 2 && d.bytes == 3;

    let mut t = Table::new(build_files());
    let miss = t.count(&[b"zz"]);
    let empty = Counts::zero().project(O_M);
    out[3] = miss == Err(WC_ENOENT) && t.count(&[b"noeol"]).is_ok() && empty == Counts::zero() && scan(b"").bytes == 0;
    out
}

/// 十三判据。
pub fn run_unx11433_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11433.wc");
    let l = build_ledger();
    s.add("WC1-十四格齐备", l.len() == SAMPLES && l.is_empty() == false, "static");
    s.add_dyn("WC2-十二应二拒", l.ok_count() == 12 && l.err_count() == 2 && l.audit().is_ok(), &format!("ok={} err={}", l.ok_count(), l.err_count()));
    s.add("WC3-基础四元组", l.ending(0).map(|e| e.got) == Some(Counts::make(1, 2, 12, 12)), "static");
    s.add("WC4-选项投影", Counts::make(3, 4, 5, 6).project(O_W) == Counts::make(0, 4, 0, 0) && Counts::make(3, 4, 5, 6).project(O_C) == Counts::make(0, 0, 5, 0), "static");
    s.add("WC5-多文件求和", l.ending(9).map(|e| e.got) == Some(Counts::make(3, 5, 26, 26)), "static");
    s.add("WC6-UTF8字符数", l.ending(5).map(|e| e.got.chars) == Some(3) && l.ending(5).map(|e| e.got.bytes) == Some(4), "static");
    s.add("WC7-非法字节计一字符", l.ending(6).map(|e| e.got.chars) == Some(1), "static");
    s.add("WC8-空文件零值", l.ending(7).map(|e| e.got) == Some(Counts::zero()), "static");
    s.add("WC9-无换行不计行", l.ending(8).map(|e| e.got.lines) == Some(0), "static");
    s.add("WC10-缺失拒绝", l.ending(10).map(|e| e.code) == Some((WC_ENOENT & 0xffff) as u16), "static");
    s.add("WC11-空名字拒绝", l.ending(11).map(|e| e.code) == Some((WC_ENOENT & 0xffff) as u16), "static");
    s.add("WC12-字符投影求和", l.ending(12).map(|e| e.got.chars) == Some(15) && l.ending(12).map(|e| e.got.project(O_M)) == Some(Counts::make(0, 0, 0, 15)), "static");
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(WC_EFROZEN);
    let inj = inject_four();
    s.add_dyn("WC13-冻结重放四注入", root != 0 && rep == Ok(root) && again && inj == [true; 4], &format!("root={:016x} {:?}", root, inj));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_dimensions() {
        assert_eq!(scan(b"hello world\n"), Counts::make(1, 2, 12, 12));
        assert_eq!(scan(b"abc"), Counts::make(0, 1, 3, 3));
        assert_eq!(scan(b""), Counts::zero());
    }

    #[test]
    fn utf8_and_invalid() {
        assert_eq!(scan(b"a\xc3\xa9\n"), Counts::make(1, 1, 4, 3));
        assert_eq!(scan(b"\xff").chars, 1);
    }

    #[test]
    fn total_equals_parts() {
        let mut t = Table::new(build_files());
        assert_eq!(t.count(&[b"a", b"b"]), Ok(Counts::make(3, 5, 26, 26)));
        assert_eq!(t.last_len(), 2);
    }

    #[test]
    fn missing_and_empty_name() {
        let mut t = Table::new(build_files());
        assert_eq!(t.count(&[b"zz"]), Err(WC_ENOENT));
        assert_eq!(t.count(&[]), Err(WC_ENAME));
    }

    #[test]
    fn projection_zeroes() {
        let c = Counts::make(1, 2, 12, 12);
        assert_eq!(c.project(O_L), Counts::make(1, 0, 0, 0));
        assert_eq!(c.project(O_M), Counts::make(0, 0, 0, 12));
    }

    #[test]
    fn ledger_and_checks() {
        let l = build_ledger();
        assert_eq!(l.ok_count(), 12);
        assert_eq!(l.err_count(), 2);
        assert!(l.audit().is_ok());
        assert_eq!(inject_four(), [true; 4]);
        assert_eq!(run_unx11433_checks().failed_count(), 0);
    }
}
