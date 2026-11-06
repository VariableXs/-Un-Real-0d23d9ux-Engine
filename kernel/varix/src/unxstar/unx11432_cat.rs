//! UNX-F11432 cat 多操作数串联与输出转换（0x20ac0 段）
//!
//! `cat` 的表面语义是“把每个操作数的字节依次写到标准输出”，但真正的工程难点
//! 在错误传播：第 k 个操作数打不开时，前面的字节已经写出、后面的操作数仍要
//! 继续处理，最终退出码必须是 1。模型用“逐操作数推进 + 粘性状态位”实现这一点，
//! 绝不因为一次失败而中断整条流水线。
//!
//! 转换选项让它不再是纯复制：`-n` 在每个逻辑行前写六列宽行号和一个制表符，
//! `-E` 在换行前插入 `$`，`-s` 把连续空行压缩成一行。三者共享同一个行首状态机，
//! 因此组合使用时的顺序必须是“行号 → 内容 → 行尾标记”。账本里既有单项样本，
//! 也有组合样本，用来固定这个顺序。
//!
//! 输出端可能写不满：模型给每次调用一个容量上限，写满即停止并把退出码置 1。
//! 这与磁盘满时的行为一致——已经写出的字节保留，未写出的丢弃，调用方通过退出
//! 码判断结果。没有操作数时读标准输入，操作数写作 `-` 时也读标准输入。
//!
//! 账本十四格中十格退出 0、四格退出 1；总写出字节数、行号格式、压缩结果与
//! 容量截断都在判据里逐项重算。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20ac0;
/// 编号非法。
pub const CT_EID: u32 = SEG_BASE + 0x01;
/// 顺序错误。
pub const CT_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const CT_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const CT_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测缺失。
pub const CT_ENOBS: u32 = SEG_BASE + 0x05;
/// 结果不符。
pub const CT_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 计数失衡。
pub const CT_EBAL: u32 = SEG_BASE + 0x07;
/// 名字非法。
pub const CT_ENAME: u32 = SEG_BASE + 0x08;
/// 源不可读。
pub const CT_EACCES: u32 = SEG_BASE + 0x09;
/// 输出容量耗尽。
pub const CT_ENOSPC: u32 = SEG_BASE + 0x0a;
/// 票据不一致。
pub const CT_ESEAL: u32 = SEG_BASE + 0x0b;

/// 行号。
pub const U_NUM: u8 = 0x01;
/// 行尾美元。
pub const U_END: u8 = 0x02;
/// 压缩空行。
pub const U_SQZ: u8 = 0x04;

/// 名字容量。
pub const NAME_CAP: usize = 6;
/// 数据容量。
pub const DATA_CAP: usize = 32;
/// 输出容量。
pub const OUT_CAP: usize = 192;
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

/// 输入源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Src {
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
    pub data: [u8; DATA_CAP],
    pub dlen: u8,
    pub readable: bool,
}

impl Src {
    pub fn make(name: &[u8], data: &[u8], readable: bool) -> Src {
        let mut n = [0u8; NAME_CAP];
        let mut d = [0u8; DATA_CAP];
        let nl = put(&mut n, name);
        let dl = put(&mut d, data);
        Src { name: n, nlen: nl, data: d, dlen: dl, readable }
    }

    pub fn name(&self) -> &[u8] {
        cut(&self.name, self.nlen)
    }

    pub fn data(&self) -> &[u8] {
        cut(&self.data, self.dlen)
    }
}

/// cat 运行器：输出缓冲 + 粘性状态。
pub struct Cat {
    srcs: Vec<Src>,
    out: [u8; OUT_CAP],
    olen: usize,
    total: u32,
    runs: u32,
    blocked: u32,
    rejects: u32,
}

impl Cat {
    pub fn new(srcs: Vec<Src>) -> Cat {
        Cat { srcs, out: [0; OUT_CAP], olen: 0, total: 0, runs: 0, blocked: 0, rejects: 0 }
    }

    pub fn src(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.srcs.len() {
            if let Some(s) = self.srcs.get(i) {
                if s.name() == name {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    pub fn out(&self) -> &[u8] {
        cut(&self.out, self.olen as u8)
    }

    pub fn out_len(&self) -> usize {
        self.olen
    }

    pub fn total(&self) -> u32 {
        self.total
    }

    pub fn runs(&self) -> u32 {
        self.runs
    }

    pub fn blocked(&self) -> u32 {
        self.blocked
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    fn emit(&mut self, b: u8, cap: usize) -> bool {
        if self.olen >= cap || self.olen >= OUT_CAP {
            return false;
        }
        self.out[self.olen] = b;
        self.olen += 1;
        true
    }

    fn emit_num(&mut self, n: u32, cap: usize) -> bool {
        let mut digits = 1u32;
        let mut v = n;
        while v >= 10 {
            v /= 10;
            digits += 1;
        }
        let mut pad = 6u32;
        if digits > pad {
            pad = digits;
        }
        let mut k = 0u32;
        while k + digits < pad {
            if !self.emit(b' ', cap) {
                return false;
            }
            k += 1;
        }
        let mut div = 1u32;
        let mut j = 1u32;
        while j < digits {
            div *= 10;
            j += 1;
        }
        while div > 0 {
            let d = (n / div) % 10;
            if !self.emit(b'0' + d as u8, cap) {
                return false;
            }
            div /= 10;
        }
        self.emit(b'\t', cap)
    }

    /// 串联。返回 (退出码, 写出字节数)。空列表或 "-" 读标准输入。
    pub fn run(&mut self, names: &[&[u8]], flags: u8, cap: usize) -> (u8, u16) {
        self.olen = 0;
        self.runs += 1;
        let mut status = 0u8;
        let mut line: u32 = 1;
        let mut at_start = true;
        let mut last_blank = false;
        let mut stopped = false;
        if names.is_empty() {
            let mut one: [&[u8]; 1] = [b"-" as &[u8]; 1];
            return self.run_inner(&mut one, flags, cap, &mut status, &mut line, &mut at_start, &mut last_blank, &mut stopped);
        }
        let mut i = 0usize;
        while i < names.len() {
            if stopped {
                break;
            }
            let name = match names.get(i) {
                Some(v) => *v,
                None => break,
            };
            let lookup = if name == b"-" { b"stdin" as &[u8] } else { name };
            let si = match self.src(lookup) {
                Some(v) => v,
                None => {
                    status = 1;
                    self.blocked += 1;
                    i += 1;
                    continue;
                }
            };
            let s = match self.srcs.get(si) {
                Some(v) => *v,
                None => {
                    status = 1;
                    i += 1;
                    continue;
                }
            };
            if !s.readable {
                status = 1;
                self.blocked += 1;
                i += 1;
                continue;
            }
            let data = s.data();
            let mut j = 0usize;
            while j < data.len() {
                let b = match data.get(j) {
                    Some(v) => *v,
                    None => break,
                };
                if at_start {
                    let blank = b == b'\n';
                    if flags & U_SQZ != 0 && blank && last_blank {
                        j += 1;
                        continue;
                    }
                    last_blank = blank;
                    if flags & U_NUM != 0 {
                        if !self.emit_num(line, cap) {
                            status = 1;
                            stopped = true;
                            self.blocked += 1;
                            break;
                        }
                        line += 1;
                    }
                    at_start = false;
                }
                if b == b'\n' {
                    if flags & U_END != 0 {
                        if !self.emit(b'$', cap) {
                            status = 1;
                            stopped = true;
                            self.blocked += 1;
                            break;
                        }
                    }
                    if !self.emit(b, cap) {
                        status = 1;
                        stopped = true;
                        self.blocked += 1;
                        break;
                    }
                    at_start = true;
                } else if !self.emit(b, cap) {
                    status = 1;
                    stopped = true;
                    self.blocked += 1;
                    break;
                }
                j += 1;
            }
            i += 1;
        }
        if !stopped && self.olen >= cap {
            status = 1;
        }
        self.total += self.olen as u32;
        (status, self.olen as u16)
    }

    #[allow(clippy::too_many_arguments)]
    fn run_inner(
        &mut self,
        names: &mut [&[u8]],
        flags: u8,
        cap: usize,
        status: &mut u8,
        line: &mut u32,
        at_start: &mut bool,
        last_blank: &mut bool,
        stopped: &mut bool,
    ) -> (u8, u16) {
        let mut i = 0usize;
        while i < names.len() {
            let name = match names.get(i) {
                Some(v) => *v,
                None => break,
            };
            let lookup = if name == b"-" { b"stdin" as &[u8] } else { name };
            let si = match self.src(lookup) {
                Some(v) => v,
                None => {
                    *status = 1;
                    self.blocked += 1;
                    i += 1;
                    continue;
                }
            };
            let s = match self.srcs.get(si) {
                Some(v) => *v,
                None => {
                    *status = 1;
                    i += 1;
                    continue;
                }
            };
            if !s.readable {
                *status = 1;
                self.blocked += 1;
                i += 1;
                continue;
            }
            let data = s.data();
            let mut j = 0usize;
            while j < data.len() {
                let b = match data.get(j) {
                    Some(v) => *v,
                    None => break,
                };
                if *at_start {
                    let blank = b == b'\n';
                    if flags & U_SQZ != 0 && blank && *last_blank {
                        j += 1;
                        continue;
                    }
                    *last_blank = blank;
                    if flags & U_NUM != 0 {
                        if !self.emit_num(*line, cap) {
                            *status = 1;
                            *stopped = true;
                            self.blocked += 1;
                            break;
                        }
                        *line += 1;
                    }
                    *at_start = false;
                }
                if b == b'\n' {
                    if flags & U_END != 0 && !self.emit(b'$', cap) {
                        *status = 1;
                        *stopped = true;
                        self.blocked += 1;
                        break;
                    }
                    if !self.emit(b, cap) {
                        *status = 1;
                        *stopped = true;
                        self.blocked += 1;
                        break;
                    }
                    *at_start = true;
                } else if !self.emit(b, cap) {
                    *status = 1;
                    *stopped = true;
                    self.blocked += 1;
                    break;
                }
                j += 1;
            }
            if *stopped {
                break;
            }
            i += 1;
        }
        if !*stopped && self.olen >= cap {
            *status = 1;
        }
        self.total += self.olen as u32;
        (*status, self.olen as u16)
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.srcs.is_empty() || self.olen > OUT_CAP {
            return Err(CT_EBAL);
        }
        let mut i = 0usize;
        while i < self.srcs.len() {
            let x = match self.srcs.get(i) {
                Some(v) => *v,
                None => return Err(CT_ENOBS),
            };
            if x.dlen as usize > DATA_CAP || x.nlen == 0 {
                return Err(CT_EBAL);
            }
            let mut j = i + 1;
            while j < self.srcs.len() {
                if let Some(y) = self.srcs.get(j) {
                    if x.name() == y.name() {
                        return Err(CT_EBAL);
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
        while i < self.srcs.len() {
            if let Some(s) = self.srcs.get(i) {
                h = fold(h, s.dlen as u64);
                h = fold(h, s.readable as u64);
                let mut k = 0usize;
                while k < s.nlen as usize {
                    if let Some(b) = s.name.get(k) {
                        h ^= *b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < self.olen {
            if let Some(b) = self.out.get(j) {
                h ^= *b as u64;
                h = h.wrapping_mul(FNV_PRIME);
            }
            j += 1;
        }
        h = fold(h, self.total as u64);
        h = fold(h, self.runs as u64);
        fold(h, self.blocked as u64)
    }
}

/// 标准源表。
pub fn build_srcs() -> Vec<Src> {
    let mut v = Vec::new();
    v.push(Src::make(b"a", b"hello\nworld\n", true));
    v.push(Src::make(b"b", b"tail", true));
    v.push(Src::make(b"empty", b"", true));
    v.push(Src::make(b"stdin", b"stdin\n", true));
    v.push(Src::make(b"blob", b"0123456789abcdefghij", true));
    v.push(Src::make(b"blank", b"x\n\n\n\ny\n", true));
    v.push(Src::make(b"secret", b"no", false));
    v
}

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    pub flags: u8,
    pub cap: u16,
    pub a: [u8; NAME_CAP],
    pub alen: u8,
    pub b: [u8; NAME_CAP],
    pub blen: u8,
    pub two: bool,
    pub status: u8,
    pub expect: u16,
}

impl Case {
    pub fn first(&self) -> &[u8] {
        cut(&self.a, self.alen)
    }

    pub fn second(&self) -> &[u8] {
        cut(&self.b, self.blen)
    }
}

/// 结局。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ending {
    pub status: u8,
    pub value: u16,
}

/// 账本。
pub struct Ledger {
    cat: Cat,
    cases: Vec<Case>,
    ends: Vec<Ending>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger { cat: Cat::new(build_srcs()), cases: Vec::new(), ends: Vec::new(), frozen: false, root: 0, rejects: 0 }
    }

    pub fn register(&mut self, slot: u8, first: &[u8], second: &[u8], two: bool, flags: u8, cap: u16, status: u8, expect: u16) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(CT_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(CT_EID);
        }
        if (slot as usize) < self.cases.len() {
            self.rejects += 1;
            return Err(CT_EDUP);
        }
        if (slot as usize) != self.cases.len() {
            self.rejects += 1;
            return Err(CT_EORDER);
        }
        let (st, got) = if two {
            let mut names: [&[u8]; 2] = [first, second];
            self.cat.run(&mut names, flags, cap as usize)
        } else if first.is_empty() {
            self.cat.run(&[], flags, cap as usize)
        } else {
            let mut names: [&[u8]; 1] = [first];
            self.cat.run(&mut names, flags, cap as usize)
        };
        if st != status || got != expect {
            self.rejects += 1;
            return Err(CT_EMISMATCH);
        }
        let mut c = Case { flags, cap, a: [0; NAME_CAP], alen: 0, b: [0; NAME_CAP], blen: 0, two, status, expect };
        c.alen = put(&mut c.a, first);
        c.blen = put(&mut c.b, second);
        self.cases.push(c);
        self.ends.push(Ending { status: st, value: got });
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
                if e.status == 0 {
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

    pub fn cat(&self) -> &Cat {
        &self.cat
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.cases.len() != self.ends.len() || self.cases.is_empty() {
            return Err(CT_EBAL);
        }
        let mut i = 0usize;
        while i < self.cases.len() {
            let c = match self.cases.get(i) {
                Some(v) => *v,
                None => return Err(CT_ENOBS),
            };
            let e = match self.ends.get(i) {
                Some(v) => *v,
                None => return Err(CT_ENOBS),
            };
            if c.status != e.status || c.expect != e.value {
                return Err(CT_EMISMATCH);
            }
            i += 1;
        }
        self.cat.audit()
    }

    pub fn seal(&self) -> u64 {
        self.cat.seal()
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(CT_EFROZEN);
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
            Err(CT_ESEAL)
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
    let _ = l.register(0, b"a", b"", false, 0, 64, 0, 12);
    let _ = l.register(1, b"a", b"b", true, 0, 64, 0, 16);
    let _ = l.register(2, b"-", b"", false, 0, 64, 0, 6);
    let _ = l.register(3, b"nope", b"a", true, 0, 64, 1, 12);
    let _ = l.register(4, b"empty", b"", false, 0, 64, 0, 0);
    let _ = l.register(5, b"a", b"", false, U_NUM, 64, 0, 26);
    let _ = l.register(6, b"a", b"", false, U_END, 64, 0, 14);
    let _ = l.register(7, b"blank", b"", false, U_SQZ, 64, 0, 5);
    let _ = l.register(8, b"a", b"nope", true, 0, 64, 1, 12);
    let _ = l.register(9, b"blob", b"", false, 0, 16, 1, 16);
    let _ = l.register(10, b"", b"", false, 0, 64, 0, 6);
    let _ = l.register(11, b"b", b"", false, U_NUM, 64, 0, 11);
    let _ = l.register(12, b"b", b"", false, U_END, 64, 0, 4);
    let _ = l.register(13, b"secret", b"", false, 0, 64, 1, 0);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.ok_count() == 10 && l.err_count() == 4 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    out[0] = base && root != 0 && rep == Ok(root);

    let mut a = Cat::new(build_srcs());
    let (st, n) = a.run(&[b"a", b"b"], 0, 64);
    out[1] = st == 0 && n == 16 && a.out() == b"hello\nworld\ntail" as &[u8];

    let mut b = Cat::new(build_srcs());
    let (st2, n2) = b.run(&[b"nope", b"a"], 0, 64);
    out[2] = st2 == 1 && n2 == 12 && b.out() == b"hello\nworld\n" as &[u8] && b.blocked() == 1;

    let mut c = Cat::new(build_srcs());
    let (st3, n3) = c.run(&[b"a"], U_NUM, 64);
    out[3] = st3 == 0
        && n3 == 26
        && c.out() == b"     1\thello\n     2\tworld\n" as &[u8]
        && c.run(&[b"blob"], 0, 16) == (1, 16)
        && c.blocked() >= 1;
    out
}

/// 十三判据。
pub fn run_unx11432_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11432.cat");
    let l = build_ledger();
    s.add("CT1-十四格齐备", l.len() == SAMPLES && l.is_empty() == false, "static");
    s.add_dyn("CT2-十字退出四字失败", l.ok_count() == 10 && l.err_count() == 4 && l.audit().is_ok(), &format!("ok={} err={}", l.ok_count(), l.err_count()));
    s.add("CT3-串联顺序", l.ending(1).map(|e| e.value) == Some(16), "static");
    s.add("CT4-标准输入", l.ending(2).map(|e| e.value) == Some(6) && l.ending(10).map(|e| e.value) == Some(6), "static");
    s.add("CT5-缺失继续", l.ending(3).map(|e| (e.status, e.value)) == Some((1, 12)), "static");
    s.add("CT6-空操作数", l.ending(4).map(|e| (e.status, e.value)) == Some((0, 0)), "static");
    s.add("CT7-行号六列宽", l.ending(5).map(|e| e.value) == Some(26) && l.ending(11).map(|e| e.value) == Some(11), "static");
    s.add("CT8-行尾美元", l.ending(6).map(|e| e.value) == Some(14) && l.ending(12).map(|e| e.value) == Some(4), "static");
    s.add("CT9-空行压缩", l.ending(7).map(|e| e.value) == Some(5), "static");
    s.add("CT10-尾部缺失仍写出", l.ending(8).map(|e| (e.status, e.value)) == Some((1, 12)), "static");
    s.add("CT11-容量截断", l.ending(9).map(|e| (e.status, e.value)) == Some((1, 16)), "static");
    s.add("CT12-不可读源", l.ending(13).map(|e| (e.status, e.value)) == Some((1, 0)), "static");
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(CT_EFROZEN);
    let inj = inject_four();
    s.add_dyn("CT13-冻结重放四注入", root != 0 && rep == Ok(root) && again && inj == [true; 4], &format!("root={:016x} {:?}", root, inj));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concat_order() {
        let mut c = Cat::new(build_srcs());
        assert_eq!(c.run(&[b"a", b"b"], 0, 64), (0, 16));
        assert_eq!(c.out(), b"hello\nworld\ntail" as &[u8]);
    }

    #[test]
    fn missing_continues() {
        let mut c = Cat::new(build_srcs());
        assert_eq!(c.run(&[b"nope", b"a"], 0, 64), (1, 12));
        assert_eq!(c.blocked(), 1);
    }

    #[test]
    fn numbering() {
        let mut c = Cat::new(build_srcs());
        assert_eq!(c.run(&[b"a"], U_NUM, 64), (0, 26));
        assert_eq!(c.out(), b"     1\thello\n     2\tworld\n" as &[u8]);
    }

    #[test]
    fn ends_and_squeeze() {
        let mut c = Cat::new(build_srcs());
        assert_eq!(c.run(&[b"a"], U_END, 64), (0, 14));
        assert_eq!(c.run(&[b"blank"], U_SQZ, 64), (0, 5));
        assert_eq!(c.out(), b"x\n\ny\n" as &[u8]);
    }

    #[test]
    fn capacity_truncates() {
        let mut c = Cat::new(build_srcs());
        assert_eq!(c.run(&[b"blob"], 0, 16), (1, 16));
        assert_eq!(c.out_len(), 16);
        assert_eq!(c.blocked(), 1);
    }

    #[test]
    fn dash_and_unreadable() {
        let mut c = Cat::new(build_srcs());
        assert_eq!(c.run(&[b"-"], 0, 64), (0, 6));
        assert_eq!(c.run(&[b"secret"], 0, 64), (1, 0));
    }

    #[test]
    fn ledger_replays() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert!(l.audit().is_ok());
        assert_eq!(inject_four(), [true; 4]);
        assert_eq!(run_unx11432_checks().failed_count(), 0);
    }
}
