//! UNX-F11491 xargs 基本批执行（0x20e80 段）
//!
//! xargs 先把标准输入切成词，再按“每次最多几个参数、最多多少字节”拼成若干次
//! 调用。切词不是简单按空白分割：单引号内的一切（包括空白）原样成为一个词，
//! 双引号同理，引号本身不进入参数；未闭合的引号是 EQUOTE，这是输入被截断的
//! 典型信号，必须报而不是猜。
//!
//! 分批有两条上界：参数个数（`-n`）与命令字节预算（`-s`）。模型按词顺序贪心
//! 装填，装不下就开下一批，单个数就超过预算则 E2BIG——xargs 在这种情况同样
//! 拒绝执行而不是拆词。空输入在缺省语义下仍会执行一次命令（无参数），这一格
//! 专门用来防止实现把“零词”误当成“零次调用”。词数超过 16 或命令为空分别
//! 是 EWORDS 与 ENOCMD。
//!
//! 账本十四格：八格成功覆盖空白切词、单双引号、三批、单批、空输入一次、首批
//! 参数数与字节预算分批；六格拒绝依次是未闭合引号、输入超长、参数上限为零、
//! 单数超预算、空命令与词数超限。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0x20e80;

/// 编号非法。
pub const XB_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const XB_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const XB_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const XB_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const XB_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const XB_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const XB_EBAL: u32 = SEG_BASE + 0x07;
/// 容量满。
pub const XB_EFULL: u32 = SEG_BASE + 0x08;
/// 票号不一致。
pub const XB_ESEAL: u32 = SEG_BASE + 0x09;
/// 引号未闭合。
pub const XB_EQUOTE: u32 = SEG_BASE + 0x0a;
/// 输入超长。
pub const XB_EINLEN: u32 = SEG_BASE + 0x0b;
/// 参数上限为零。
pub const XB_EMAXARG: u32 = SEG_BASE + 0x0c;
/// 单个词超预算。
pub const XB_E2BIG: u32 = SEG_BASE + 0x0d;
/// 命令为空。
pub const XB_ENOCMD: u32 = SEG_BASE + 0x0e;
/// 词数超限。
pub const XB_EWORDS: u32 = SEG_BASE + 0x0f;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 操作：切词。
pub const OP_SPLIT: u8 = 1;
/// 操作：分批数。
pub const OP_BATCHES: u8 = 2;
/// 操作：首批参数数。
pub const OP_ARGS: u8 = 3;
/// 操作：命令行长度。
pub const OP_ARGV: u8 = 4;

/// 输入容量。
pub const INPUT_CAP: usize = 64;
/// 词数上限。
pub const MAX_WORDS: usize = 16;
/// 批数上限。
pub const MAX_BATCHES: usize = 8;
/// 样本格数。
pub const SAMPLES: usize = 14;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入。
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

/// 一批调用。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Batch {
    pub first: usize,
    pub n: usize,
}

/// 空白或换行。
fn sep(c: u8) -> bool {
    c == b' ' || c == b'\t' || c == b'\n'
}

/// 切词：单双引号成组，未闭合报错。
pub fn split_words<'a>(input: &'a [u8], out: &mut [&'a [u8]]) -> Result<usize, u32> {
    if input.len() > INPUT_CAP {
        return Err(XB_EINLEN);
    }
    let mut n = 0usize;
    let mut i = 0usize;
    while i < input.len() {
        while let Some(&c) = input.get(i) {
            if sep(c) {
                i += 1;
            } else {
                break;
            }
        }
        if i >= input.len() {
            break;
        }
        let c = match input.get(i) {
            Some(&v) => v,
            None => break,
        };
        let start;
        let end;
        if c == b'\'' {
            i += 1;
            start = i;
            loop {
                match input.get(i) {
                    Some(&b'\'') => {
                        end = i;
                        i += 1;
                        break;
                    }
                    Some(_) => i += 1,
                    None => return Err(XB_EQUOTE),
                }
            }
        } else if c == b'"' {
            i += 1;
            start = i;
            loop {
                match input.get(i) {
                    Some(&b'"') => {
                        end = i;
                        i += 1;
                        break;
                    }
                    Some(_) => i += 1,
                    None => return Err(XB_EQUOTE),
                }
            }
        } else {
            start = i;
            while let Some(&d) = input.get(i) {
                if sep(d) {
                    break;
                }
                i += 1;
            }
            end = i;
        }
        if n >= MAX_WORDS || n >= out.len() {
            return Err(XB_EWORDS);
        }
        match out.get_mut(n) {
            Some(slot) => *slot = input.get(start..end).unwrap_or(&[]),
            None => return Err(XB_EWORDS),
        }
        n += 1;
    }
    Ok(n)
}

/// 分批：个数与字节双上界，空输入仍有一批。
pub fn plan(
    words: &[&[u8]],
    max_args: usize,
    max_bytes: u32,
    out: &mut [Batch],
) -> Result<usize, u32> {
    if max_args == 0 {
        return Err(XB_EMAXARG);
    }
    if words.len() > MAX_WORDS {
        return Err(XB_EWORDS);
    }
    if words.is_empty() {
        return match out.get_mut(0) {
            Some(s) => {
                *s = Batch { first: 0, n: 0 };
                Ok(1)
            }
            None => Err(XB_EWORDS),
        };
    }
    let mut nb = 0usize;
    let mut i = 0usize;
    while i < words.len() {
        let first = i;
        let mut bytes = 0u32;
        let mut n = 0usize;
        while i < words.len() && n < max_args {
            let w = match words.get(i) {
                Some(v) => *v,
                None => break,
            };
            let cost = match u32::try_from(w.len()) {
                Ok(v) => v.saturating_add(1),
                Err(_) => return Err(XB_E2BIG),
            };
            if cost > max_bytes {
                return Err(XB_E2BIG);
            }
            if n > 0 && bytes.saturating_add(cost) > max_bytes {
                break;
            }
            bytes = bytes.saturating_add(cost);
            n += 1;
            i += 1;
        }
        if n == 0 {
            return Err(XB_E2BIG);
        }
        if nb >= MAX_BATCHES || nb >= out.len() {
            return Err(XB_EWORDS);
        }
        match out.get_mut(nb) {
            Some(s) => *s = Batch { first, n },
            None => return Err(XB_EWORDS),
        }
        nb += 1;
    }
    Ok(nb)
}

/// 命令行长度：命令 + 参数个数。
pub fn argv_len(cmd: &[u8], words: &[&[u8]]) -> Result<u32, u32> {
    if cmd.is_empty() {
        return Err(XB_ENOCMD);
    }
    let n = match words.len().checked_add(1) {
        Some(v) => v,
        None => return Err(XB_EWORDS),
    };
    if n > MAX_WORDS {
        return Err(XB_EWORDS);
    }
    Ok(n as u32)
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub slot: u8,
    pub op: u8,
    pub ok: bool,
    pub code: u32,
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample {
            slot: 0,
            op: 0,
            ok: false,
            code: 0,
            value: 0,
        }
    }
}

/// 样本账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    cap: usize,
    seal: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger {
            rows: [Sample::blank(); SAMPLES],
            len: 0,
            cap: SAMPLES,
            seal: 0,
            frozen: false,
            rejects: 0,
        }
    }

    /// 指定容量。
    pub const fn with_cap(cap: usize) -> Ledger {
        Ledger {
            rows: [Sample::blank(); SAMPLES],
            len: 0,
            cap,
            seal: 0,
            frozen: false,
            rejects: 0,
        }
    }

    /// 登记一格：冻结 → 编号 → 重复 → 顺序 → 容量。
    pub fn register(&mut self, row: Sample) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(XB_EFROZEN);
        }
        if (row.slot as usize) >= SAMPLES {
            return Err(XB_EID);
        }
        if (row.slot as usize) < self.len {
            return Err(XB_EDUP);
        }
        if (row.slot as usize) != self.len {
            return Err(XB_EORDER);
        }
        if self.len >= self.cap {
            return Err(XB_EFULL);
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = row,
            None => return Err(XB_EID),
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取样本。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功计数。
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

    /// 拒绝计数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(XB_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(XB_EBAL);
        }
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.slot as usize != i {
                        return Err(XB_EORDER);
                    }
                    if s.ok {
                        if s.code != V_OK {
                            return Err(XB_EMISMATCH);
                        }
                    } else if s.code == V_OK || s.value != 0 {
                        return Err(XB_EMISMATCH);
                    }
                }
                None => return Err(XB_ENOBS),
            }
            i += 1;
        }
        let vals = [4u32, 2, 2, 3, 1, 1, 3, 3];
        let mut k = 0usize;
        while k < vals.len() {
            match (self.sample(k), vals.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.value != *w {
                        return Err(XB_EMISMATCH);
                    }
                }
                _ => return Err(XB_EBAL),
            }
            k += 1;
        }
        let codes = [XB_EQUOTE, XB_EINLEN, XB_EMAXARG, XB_E2BIG, XB_ENOCMD, XB_EWORDS];
        let mut c = 0usize;
        let mut j = 8usize;
        while j < self.len {
            match self.rows.get(j) {
                Some(s) => match codes.get(c) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(XB_EMISMATCH);
                        }
                    }
                    None => return Err(XB_EBAL),
                },
                None => return Err(XB_ENOBS),
            }
            c += 1;
            j += 1;
        }
        if c != codes.len() {
            return Err(XB_EBAL);
        }
        Ok(())
    }

    /// 熔断票号。
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
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(XB_EFROZEN);
        }
        let s = self.seal();
        self.seal = s;
        self.frozen = true;
        Ok(s)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.len != self.len {
            return Err(XB_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.seal {
            return Err(XB_ESEAL);
        }
        Ok(h)
    }

    /// 封存票号。
    pub fn root(&self) -> u64 {
        self.seal
    }

    /// 二次冻结次数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

fn cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample {
            slot,
            op,
            ok: true,
            code: V_OK,
            value: v,
        },
        Err(c) => Sample {
            slot,
            op,
            ok: false,
            code: c,
            value: 0,
        },
    }
}

/// 十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut words: [&[u8]; MAX_WORDS] = [b""; MAX_WORDS];
    let mut batches = [Batch { first: 0, n: 0 }; MAX_BATCHES];

    let five: [&[u8]; 5] = [b"a", b"b", b"c", b"d", b"e"];
    let aa: [&[u8]; 5] = [b"aa", b"aa", b"aa", b"aa", b"aa"];

    let _ = l.register(cell(
        0,
        OP_SPLIT,
        split_words(b"a b\tc\nd", &mut words).map(|n| n as u32),
    ));
    let _ = l.register(cell(
        1,
        OP_SPLIT,
        split_words(b"'a b' c", &mut words).map(|n| n as u32),
    ));
    let _ = l.register(cell(
        2,
        OP_SPLIT,
        split_words(b"\"x y\" z", &mut words).map(|n| n as u32),
    ));
    let _ = l.register(cell(
        3,
        OP_BATCHES,
        plan(&five, 2, 64, &mut batches).map(|n| n as u32),
    ));
    let _ = l.register(cell(
        4,
        OP_BATCHES,
        plan(&five, 5, 64, &mut batches).map(|n| n as u32),
    ));
    let _ = l.register(cell(
        5,
        OP_BATCHES,
        plan(&[], 4, 64, &mut batches).map(|n| n as u32),
    ));
    let first_batch = plan(&five, 3, 64, &mut batches);
    let first_n = match batches.get(0) {
        Some(b) => b.n as u32,
        None => 0,
    };
    let _ = l.register(cell(6, OP_ARGS, first_batch.map(|_| first_n)));
    let _ = l.register(cell(
        7,
        OP_BATCHES,
        plan(&aa, 4, 7, &mut batches).map(|n| n as u32),
    ));

    let _ = l.register(cell(
        8,
        OP_SPLIT,
        split_words(b"'abc", &mut words).map(|n| n as u32),
    ));
    let long = [b'a'; 65];
    let _ = l.register(cell(
        9,
        OP_SPLIT,
        split_words(&long, &mut words).map(|n| n as u32),
    ));
    let _ = l.register(cell(
        10,
        OP_BATCHES,
        plan(&five, 0, 64, &mut batches).map(|n| n as u32),
    ));
    let ten: [&[u8]; 1] = [b"0123456789"];
    let _ = l.register(cell(
        11,
        OP_BATCHES,
        plan(&ten, 1, 5, &mut batches).map(|n| n as u32),
    ));
    let _ = l.register(cell(12, OP_ARGV, argv_len(b"", &five)));
    let _ = l.register(cell(
        13,
        OP_SPLIT,
        split_words(b"a a a a a a a a a a a a a a a a a", &mut words).map(|n| n as u32),
    ));

    l
}

/// 四路注入：账本自洽、引号切词、分批、拒绝后恢复。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = l.freeze() == Err(XB_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && again && l.rejects() == 1;

    let mut w: [&[u8]; MAX_WORDS] = [b""; MAX_WORDS];
    let split = split_words(b"a 'b c' \"d e\"", &mut w);
    out[1] = split == Ok(3)
        && w.get(0).copied() == Some(&b"a"[..])
        && w.get(1).copied() == Some(&b"b c"[..])
        && w.get(2).copied() == Some(&b"d e"[..]);

    let five: [&[u8]; 5] = [b"a", b"b", b"c", b"d", b"e"];
    let mut b = [Batch { first: 0, n: 0 }; MAX_BATCHES];
    out[2] = plan(&five, 2, 64, &mut b) == Ok(3)
        && b.get(0) == Some(&Batch { first: 0, n: 2 })
        && b.get(2) == Some(&Batch { first: 4, n: 1 })
        && plan(&five, 5, 64, &mut b) == Ok(1);

    let blocked = argv_len(b"", &five) == Err(XB_ENOCMD);
    let resumed = argv_len(b"echo", &five) == Ok(6);
    let zero = plan(&five, 0, 64, &mut b) == Err(XB_EMAXARG);
    let retry = plan(&five, 3, 64, &mut b) == Ok(2);
    out[3] = blocked && resumed && zero && retry;
    out
}

/// 十三判据。
pub fn run_unx11491_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11491.xargsbatch");
    let l = build_ledger();

    s.add(
        "XB1-十四格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.op) == Some(OP_SPLIT),
        "static",
    );
    s.add_dyn(
        "XB2-八允六拒分布",
        l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("ok={} err={} audit={:?}", l.ok_count(), l.err_count(), l.audit()),
    );
    s.add(
        "XB3-空白切词",
        {
            let mut w: [&[u8]; MAX_WORDS] = [b""; MAX_WORDS];
            split_words(b"a b\tc\nd", &mut w) == Ok(4)
                && w.get(1).copied() == Some(&b"b"[..])
                && split_words(b"  ", &mut w) == Ok(0)
                && split_words(b"", &mut w) == Ok(0)
        },
        "static",
    );
    s.add(
        "XB4-引号成组",
        {
            let mut w: [&[u8]; MAX_WORDS] = [b""; MAX_WORDS];
            split_words(b"'a b' c", &mut w) == Ok(2)
                && w.get(0).copied() == Some(&b"a b"[..])
                && split_words(b"\"x y\" z", &mut w) == Ok(2)
                && w.get(0).copied() == Some(&b"x y"[..])
                && split_words(b"'' x", &mut w) == Ok(2)
                && w.get(0).copied() == Some(&b""[..])
        },
        "static",
    );
    s.add(
        "XB5-个数分批",
        {
            let five: [&[u8]; 5] = [b"a", b"b", b"c", b"d", b"e"];
            let mut b = [Batch { first: 0, n: 0 }; MAX_BATCHES];
            plan(&five, 2, 64, &mut b) == Ok(3)
                && b.get(0) == Some(&Batch { first: 0, n: 2 })
                && b.get(2) == Some(&Batch { first: 4, n: 1 })
                && l.sample(3).map(|x| x.value) == Some(3)
        },
        "static",
    );
    s.add(
        "XB6-单批覆盖",
        {
            let five: [&[u8]; 5] = [b"a", b"b", b"c", b"d", b"e"];
            let mut b = [Batch { first: 0, n: 0 }; MAX_BATCHES];
            plan(&five, 5, 64, &mut b) == Ok(1)
                && b.get(0) == Some(&Batch { first: 0, n: 5 })
                && l.sample(4).map(|x| x.value) == Some(1)
        },
        "static",
    );
    s.add(
        "XB7-字节预算分批",
        {
            let aa: [&[u8]; 5] = [b"aa", b"aa", b"aa", b"aa", b"aa"];
            let mut b = [Batch { first: 0, n: 0 }; MAX_BATCHES];
            plan(&aa, 4, 7, &mut b) == Ok(3)
                && b.get(0) == Some(&Batch { first: 0, n: 2 })
                && l.sample(7).map(|x| x.value) == Some(3)
        },
        "static",
    );
    s.add(
        "XB8-空输入仍一次",
        {
            let mut b = [Batch { first: 0, n: 0 }; MAX_BATCHES];
            plan(&[], 4, 64, &mut b) == Ok(1)
                && b.get(0) == Some(&Batch { first: 0, n: 0 })
                && l.sample(5).map(|x| x.value) == Some(1)
        },
        "static",
    );
    s.add(
        "XB9-未闭合引号",
        {
            let mut w: [&[u8]; MAX_WORDS] = [b""; MAX_WORDS];
            split_words(b"'abc", &mut w) == Err(XB_EQUOTE)
                && split_words(b"\"abc", &mut w) == Err(XB_EQUOTE)
                && l.sample(8).map(|x| x.code) == Some(XB_EQUOTE)
        },
        "static",
    );
    s.add(
        "XB10-输入与参数上限",
        {
            let long = [b'a'; 65];
            let mut w: [&[u8]; MAX_WORDS] = [b""; MAX_WORDS];
            let five: [&[u8]; 5] = [b"a", b"b", b"c", b"d", b"e"];
            let mut b = [Batch { first: 0, n: 0 }; MAX_BATCHES];
            split_words(&long, &mut w) == Err(XB_EINLEN)
                && plan(&five, 0, 64, &mut b) == Err(XB_EMAXARG)
                && l.sample(9).map(|x| x.code) == Some(XB_EINLEN)
                && l.sample(10).map(|x| x.code) == Some(XB_EMAXARG)
        },
        "static",
    );
    s.add(
        "XB11-单数超预算与空命令",
        {
            let ten: [&[u8]; 1] = [b"0123456789"];
            let mut b = [Batch { first: 0, n: 0 }; MAX_BATCHES];
            plan(&ten, 1, 5, &mut b) == Err(XB_E2BIG)
                && argv_len(b"", &ten) == Err(XB_ENOCMD)
                && l.sample(11).map(|x| x.code) == Some(XB_E2BIG)
                && l.sample(12).map(|x| x.code) == Some(XB_ENOCMD)
        },
        "static",
    );
    s.add(
        "XB12-词数上限",
        {
            let mut w: [&[u8]; MAX_WORDS] = [b""; MAX_WORDS];
            split_words(b"a a a a a a a a a a a a a a a a a", &mut w) == Err(XB_EWORDS)
                && l.sample(13).map(|x| x.code) == Some(XB_EWORDS)
                && l.sample(13).map(|x| x.value) == Some(0)
        },
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(XB_EFROZEN);
    let inj = inject_four();
    s.add_dyn(
        "XB13-冻结重放与四路",
        root != 0 && rep == Ok(root) && again && f.root() == root && inj == [true; 4],
        &format!("root={:016x} inj={:?}", root, inj),
    );
    s
}

/// 摘要。
pub fn summary(l: &Ledger) -> alloc::string::String {
    format!("rows={} ok={} err={}", l.len(), l.ok_count(), l.err_count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitespace_split() {
        let mut w: [&[u8]; MAX_WORDS] = [b""; MAX_WORDS];
        assert_eq!(split_words(b"a b\tc\nd", &mut w), Ok(4));
        assert_eq!(w.get(3).copied(), Some(&b"d"[..]));
    }

    #[test]
    fn quotes_group() {
        let mut w: [&[u8]; MAX_WORDS] = [b""; MAX_WORDS];
        assert_eq!(split_words(b"'a b' c", &mut w), Ok(2));
        assert_eq!(w.get(0).copied(), Some(&b"a b"[..]));
        assert_eq!(split_words(b"\"x y\" z", &mut w), Ok(2));
        assert_eq!(w.get(1).copied(), Some(&b"z"[..]));
    }

    #[test]
    fn unclosed_quote_rejected() {
        let mut w: [&[u8]; MAX_WORDS] = [b""; MAX_WORDS];
        assert_eq!(split_words(b"'abc", &mut w), Err(XB_EQUOTE));
        assert_eq!(split_words(b"\"abc", &mut w), Err(XB_EQUOTE));
    }

    #[test]
    fn batches_respect_limits() {
        let five: [&[u8]; 5] = [b"a", b"b", b"c", b"d", b"e"];
        let mut b = [Batch { first: 0, n: 0 }; MAX_BATCHES];
        assert_eq!(plan(&five, 2, 64, &mut b), Ok(3));
        assert_eq!(b.get(1), Some(&Batch { first: 2, n: 2 }));
        assert_eq!(plan(&five, 0, 64, &mut b), Err(XB_EMAXARG));
    }

    #[test]
    fn byte_budget_splits() {
        let aa: [&[u8]; 5] = [b"aa", b"aa", b"aa", b"aa", b"aa"];
        let mut b = [Batch { first: 0, n: 0 }; MAX_BATCHES];
        assert_eq!(plan(&aa, 4, 7, &mut b), Ok(3));
        assert_eq!(b.get(2), Some(&Batch { first: 4, n: 1 }));
        let ten: [&[u8]; 1] = [b"0123456789"];
        assert_eq!(plan(&ten, 1, 5, &mut b), Err(XB_E2BIG));
    }

    #[test]
    fn empty_runs_once() {
        let mut b = [Batch { first: 0, n: 0 }; MAX_BATCHES];
        assert_eq!(plan(&[], 4, 64, &mut b), Ok(1));
        assert_eq!(b.get(0), Some(&Batch { first: 0, n: 0 }));
    }

    #[test]
    fn ledger_and_checks() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert!(l.audit().is_ok());
        let s = run_unx11491_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert_eq!(inject_four(), [true; 4]);
    }
}
