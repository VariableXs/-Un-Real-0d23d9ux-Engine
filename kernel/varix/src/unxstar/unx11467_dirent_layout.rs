//! UNX-F11467 目录项二进制布局（0x20d00 段）
//!
//! `getdents(2)` 返回的不是结构体数组，而是一条条首尾相接的变长记录：
//! 每条记录以 `(d_ino, d_off, d_reclen, d_type)` 开头，随后是 NUL 结尾的
//! 文件名，再补齐到 8 字节边界。用户态只能按 `d_reclen` 步进而不能按
//! `sizeof(struct dirent)` 步进，否则名字较长的记录会把后续记录读歪。
//!
//! 本模块把“编码一份目录项列表”建模为纯函数：给定条目与缓冲区大小，
//! 逐条计算偏移、记录长度和指向下一条的 `d_off`，任何一条放不下就整批
//! 拒绝并返回 `ENOSPC`，绝不产生半条记录。名字为空、含 `/` 或 NUL、
//! 超过 `NAME_MAX`、类型码越界、重名，都在入队前拒绝。尾部的 slack
//! 属于调用方缓冲区，不写零也不算错误。
//!
//! 不变量：第一条记录偏移为 0；每条记录的 `d_off` 等于自身偏移加
//! `d_reclen`；最后一条的 `d_off` 等于总字节数；`total + slack == bsize`；
//! `d_reclen` 是 8 的倍数且不小于 `8 + namelen + 1`。审计对手写布局
//! 同样有效：篡改记录长度或偏移会立即被 `EBAL` 抓住。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20d00;
pub const DD_EID: u32 = SEG_BASE + 0x01;
pub const DD_EORDER: u32 = SEG_BASE + 0x02;
pub const DD_EDUP: u32 = SEG_BASE + 0x03;
pub const DD_EFROZEN: u32 = SEG_BASE + 0x04;
pub const DD_ENOBS: u32 = SEG_BASE + 0x05;
pub const DD_EMISMATCH: u32 = SEG_BASE + 0x06;
pub const DD_EBAL: u32 = SEG_BASE + 0x07;
pub const DD_ESEAL: u32 = SEG_BASE + 0x08;
pub const DD_ENAMETOOLONG: u32 = SEG_BASE + 0x09;
pub const DD_EINVAL: u32 = SEG_BASE + 0x0a;
pub const DD_ENOSPC: u32 = SEG_BASE + 0x0b;
pub const DD_EEXIST: u32 = SEG_BASE + 0x0c;
pub const DD_EOVERFLOW: u32 = SEG_BASE + 0x0d;
pub const DD_EBADF: u32 = SEG_BASE + 0x0e;

pub const T_UNKNOWN: u8 = 0;
pub const T_REG: u8 = 1;
pub const T_DIR: u8 = 2;
pub const T_LNK: u8 = 3;
pub const T_CHR: u8 = 4;
pub const T_FIFO: u8 = 5;
pub const T_SOCK: u8 = 6;
pub const T_BLK: u8 = 7;

pub const NAME_MAX: usize = 15;
pub const NAME_CAP: usize = 16;
pub const SAMPLES: usize = 14;

pub const OP_PLAIN: u8 = 1;
pub const OP_DUP: u8 = 2;
pub const OP_LONG: u8 = 3;
pub const OP_ZERO: u8 = 4;
pub const OP_SLASH: u8 = 5;
pub const OP_BADTYPE: u8 = 6;

pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
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

fn align8(x: u32) -> u32 {
    x.saturating_add(7) & !7u32
}

/// 一个目录条目（名字定长存储，避免静态字符串）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dirent {
    pub ino: u32,
    pub dtype: u8,
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
}

/// 编码后的记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    pub off: u32,
    pub next: u32,
    pub ino: u32,
    pub reclen: u16,
    pub dtype: u8,
    pub nlen: u8,
}

/// 一次编码的结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub rows: Vec<Record>,
    pub total: u32,
    pub slack: u32,
    pub bsize: u32,
}

/// 样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub op: u8,
    pub n: u8,
    pub bsize: u32,
    pub expect_ok: bool,
    pub val: u32,
    pub extra: u32,
}

/// 结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    pub total: u32,
    pub slack: u32,
    pub code: u16,
}

fn dirent(ino: u32, dtype: u8, name: &[u8]) -> Dirent {
    let mut b = [0u8; NAME_CAP];
    let mut i = 0usize;
    while i < name.len() && i < NAME_CAP {
        if let (Some(d), Some(s)) = (b.get_mut(i), name.get(i)) {
            *d = *s;
        }
        i += 1;
    }
    Dirent { ino, dtype, name: b, nlen: i as u8 }
}

fn same_name(a: &Dirent, b: &Dirent) -> bool {
    if a.nlen != b.nlen {
        return false;
    }
    let mut i = 0usize;
    while i < a.nlen as usize {
        match (a.name.get(i), b.name.get(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

fn check_name(e: &Dirent) -> Result<(), u32> {
    if e.nlen == 0 {
        return Err(DD_EINVAL);
    }
    if (e.nlen as usize) > NAME_MAX {
        return Err(DD_ENAMETOOLONG);
    }
    if e.dtype > T_BLK {
        return Err(DD_EINVAL);
    }
    let mut i = 0usize;
    while i < e.nlen as usize {
        match e.name.get(i) {
            Some(&c) => {
                if c == 0 || c == b'/' {
                    return Err(DD_EINVAL);
                }
            }
            None => return Err(DD_EINVAL),
        }
        i += 1;
    }
    Ok(())
}

/// 编码整批目录项。
pub fn encode(ents: &[Dirent], bsize: u32) -> Result<Layout, u32> {
    if bsize == 0 {
        return Err(DD_EINVAL);
    }
    let mut rows: Vec<Record> = Vec::new();
    let mut off = 0u32;
    let mut i = 0usize;
    while i < ents.len() {
        let e = match ents.get(i) {
            Some(v) => *v,
            None => return Err(DD_ENOBS),
        };
        check_name(&e)?;
        let mut j = 0usize;
        while j < i {
            if let Some(p) = ents.get(j) {
                if same_name(p, &e) {
                    return Err(DD_EEXIST);
                }
            }
            j += 1;
        }
        let need = 8u32.saturating_add(e.nlen as u32).saturating_add(1);
        let reclen = align8(need);
        let end = match off.checked_add(reclen) {
            Some(v) => v,
            None => return Err(DD_EOVERFLOW),
        };
        if end > bsize {
            return Err(DD_ENOSPC);
        }
        rows.push(Record {
            off,
            next: end,
            ino: e.ino,
            reclen: reclen as u16,
            dtype: e.dtype,
            nlen: e.nlen,
        });
        off = end;
        i += 1;
    }
    Ok(Layout { rows, total: off, slack: bsize - off, bsize })
}

/// 只依赖布局本身的结构审计。
pub fn audit_layout(l: &Layout) -> Result<(), u32> {
    if l.total > l.bsize {
        return Err(DD_EBAL);
    }
    if l.total.saturating_add(l.slack) != l.bsize {
        return Err(DD_EBAL);
    }
    let mut expect = 0u32;
    let mut i = 0usize;
    while i < l.rows.len() {
        let r = match l.rows.get(i) {
            Some(v) => *v,
            None => return Err(DD_ENOBS),
        };
        if r.off != expect {
            return Err(DD_EBAL);
        }
        let need = 8u32.saturating_add(r.nlen as u32).saturating_add(1);
        if (r.reclen as u32) < need || (r.reclen as u32) % 8 != 0 {
            return Err(DD_EBAL);
        }
        let end = match r.off.checked_add(r.reclen as u32) {
            Some(v) => v,
            None => return Err(DD_EOVERFLOW),
        };
        if r.next != end {
            return Err(DD_EBAL);
        }
        if r.ino == 0 {
            return Err(DD_EBAL);
        }
        expect = end;
        i += 1;
    }
    if expect != l.total {
        return Err(DD_EBAL);
    }
    Ok(())
}

fn make_entries(op: u8, n: u8) -> Result<Vec<Dirent>, u32> {
    let base = [
        dirent(2, T_DIR, b"."),
        dirent(2, T_DIR, b".."),
        dirent(3, T_REG, b"alpha"),
        dirent(4, T_REG, b"report.txt"),
        dirent(5, T_REG, b"notes.md"),
        dirent(6, T_DIR, b"sub"),
        dirent(7, T_LNK, b"lnk"),
        dirent(8, T_REG, b"zeta"),
    ];
    let mut v: Vec<Dirent> = Vec::new();
    let take = if (n as usize) > base.len() { base.len() } else { n as usize };
    let mut i = 0usize;
    while i < take {
        match base.get(i) {
            Some(e) => v.push(*e),
            None => return Err(DD_ENOBS),
        }
        i += 1;
    }
    match op {
        OP_PLAIN => Ok(v),
        OP_DUP => {
            if let Some(e) = base.get(2) {
                v.push(*e);
                Ok(v)
            } else {
                Err(DD_ENOBS)
            }
        }
        OP_LONG => {
            v.push(dirent(9, T_REG, b"xxxxxxxxxxxxxxxx"));
            Ok(v)
        }
        OP_ZERO => {
            v.push(Dirent { ino: 9, dtype: T_REG, name: [0u8; NAME_CAP], nlen: 0 });
            Ok(v)
        }
        OP_SLASH => {
            v.push(dirent(9, T_REG, b"a/b"));
            Ok(v)
        }
        OP_BADTYPE => {
            v.push(dirent(9, 9, b"bad"));
            Ok(v)
        }
        _ => Err(DD_EBADF),
    }
}

/// 账本。
pub struct Ledger {
    samples: Vec<Sample>,
    outcomes: Vec<Outcome>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger {
            samples: Vec::new(),
            outcomes: Vec::new(),
            frozen: false,
            root: 0,
            rejects: 0,
        }
    }
    pub fn register(&mut self, slot: u8, op: u8, n: u8, bsize: u32, ok: bool, val: u32, extra: u32) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(DD_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(DD_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(DD_EDUP);
        }
        if (slot as usize) != self.samples.len() {
            self.rejects += 1;
            return Err(DD_EORDER);
        }
        let s = Sample { op, n, bsize, expect_ok: ok, val, extra };
        let ents = make_entries(op, n)?;
        let r = encode(&ents, bsize);
        let out = match r {
            Ok(l) => {
                if !ok || l.total != val || l.slack != extra {
                    return Err(DD_EMISMATCH);
                }
                audit_layout(&l)?;
                Outcome { ok: true, total: l.total, slack: l.slack, code: 0 }
            }
            Err(e) => {
                let low = (e & 0xffff) as u32;
                if ok || low != val {
                    return Err(DD_EMISMATCH);
                }
                Outcome { ok: false, total: 0, slack: 0, code: low as u16 }
            }
        };
        self.samples.push(s);
        self.outcomes.push(out);
        Ok(())
    }
    pub fn len(&self) -> usize {
        self.samples.len()
    }
    pub fn sample(&self, i: usize) -> Option<Sample> {
        self.samples.get(i).copied()
    }
    pub fn outcome(&self, i: usize) -> Option<Outcome> {
        self.outcomes.get(i).copied()
    }
    pub fn ok_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.outcomes.len() {
            if let Some(o) = self.outcomes.get(i) {
                if o.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
    pub fn err_count(&self) -> usize {
        self.outcomes.len().saturating_sub(self.ok_count())
    }
    /// 重算每条样本并与记录比对。
    pub fn audit(&self) -> Result<(), u32> {
        if self.samples.len() != self.outcomes.len() || self.samples.is_empty() {
            return Err(DD_EBAL);
        }
        let mut i = 0;
        while i < self.samples.len() {
            let s = match self.samples.get(i) {
                Some(v) => *v,
                None => return Err(DD_ENOBS),
            };
            let o = match self.outcomes.get(i) {
                Some(v) => *v,
                None => return Err(DD_ENOBS),
            };
            if s.expect_ok != o.ok {
                return Err(DD_EMISMATCH);
            }
            let ents = make_entries(s.op, s.n)?;
            match encode(&ents, s.bsize) {
                Ok(l) => {
                    audit_layout(&l)?;
                    if !o.ok || l.total != o.total || l.slack != o.slack || o.code != 0 {
                        return Err(DD_EMISMATCH);
                    }
                }
                Err(e) => {
                    let low = (e & 0xffff) as u16;
                    if o.ok || low != o.code || o.total != 0 || o.slack != 0 {
                        return Err(DD_EMISMATCH);
                    }
                }
            }
            i += 1;
        }
        Ok(())
    }
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(DD_EFROZEN);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0;
        while i < self.outcomes.len() {
            if let Some(o) = self.outcomes.get(i) {
                h = fold(h, u64::from(o.ok));
                h = fold(h, o.total as u64);
                h = fold(h, o.slack as u64);
                h = fold(h, o.code as u64);
            }
            if let Some(s) = self.samples.get(i) {
                h = fold(h, s.op as u64);
                h = fold(h, s.n as u64);
                h = fold(h, s.bsize as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }
    pub fn root(&self) -> u64 {
        self.root
    }
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() && fresh.ok_count() == self.ok_count() && fresh.err_count() == self.err_count() {
            Ok(fresh.seal())
        } else {
            Err(DD_ESEAL)
        }
    }
}

/// 基线目录项。
pub fn build_entries() -> Vec<Dirent> {
    let mut v = Vec::new();
    v.push(dirent(2, T_DIR, b"."));
    v.push(dirent(2, T_DIR, b".."));
    v.push(dirent(3, T_REG, b"alpha"));
    v.push(dirent(4, T_REG, b"report.txt"));
    v.push(dirent(5, T_REG, b"notes.md"));
    v.push(dirent(6, T_DIR, b"sub"));
    v.push(dirent(7, T_LNK, b"lnk"));
    v.push(dirent(8, T_REG, b"zeta"));
    v
}

/// 十四行账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let enospc = DD_ENOSPC & 0xffff;
    let eexist = DD_EEXIST & 0xffff;
    let elong = DD_ENAMETOOLONG & 0xffff;
    let einval = DD_EINVAL & 0xffff;
    let _ = l.register(0, OP_PLAIN, 8, 512, true, 144, 368);
    let _ = l.register(1, OP_PLAIN, 3, 64, true, 48, 16);
    let _ = l.register(2, OP_PLAIN, 0, 32, true, 0, 32);
    let _ = l.register(3, OP_PLAIN, 8, 144, true, 144, 0);
    let _ = l.register(4, OP_PLAIN, 5, 4096, true, 96, 4000);
    let _ = l.register(5, OP_PLAIN, 8, 160, true, 144, 16);
    let _ = l.register(6, OP_PLAIN, 4, 96, true, 72, 24);
    let _ = l.register(7, OP_PLAIN, 8, 140, false, enospc, 0);
    let _ = l.register(8, OP_DUP, 4, 512, false, eexist, 0);
    let _ = l.register(9, OP_LONG, 1, 512, false, elong, 0);
    let _ = l.register(10, OP_ZERO, 1, 512, false, einval, 0);
    let _ = l.register(11, OP_SLASH, 1, 512, false, einval, 0);
    let _ = l.register(12, OP_BADTYPE, 1, 512, false, einval, 0);
    let _ = l.register(13, OP_PLAIN, 1, 0, false, einval, 0);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.ok_count() == 7 && l.err_count() == 7 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let twice = l.freeze() == Err(DD_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && twice;

    let e = build_entries();
    let exact = encode(&e, 144);
    out[1] = match exact {
        Ok(v) => {
            v.total == 144
                && v.slack == 0
                && v.rows.len() == 8
                && v.rows.get(7).map(|r| r.next) == Some(144)
                && audit_layout(&v).is_ok()
        }
        Err(_) => false,
    };

    let three = encode(&e[0..3], 64);
    out[2] = match three {
        Ok(v) => {
            v.rows.get(1).map(|r| (r.off, r.reclen)) == Some((16, 16))
                && v.rows.get(2).map(|r| (r.off, r.next)) == Some((32, 48))
                && v.total == 48
                && v.slack == 16
        }
        Err(_) => false,
    };

    let mut d = build_entries();
    d.push(dirent(9, T_REG, b"alpha"));
    let mut z = build_entries();
    z.push(Dirent { ino: 9, dtype: T_REG, name: [0u8; NAME_CAP], nlen: 0 });
    out[3] = encode(&d, 512) == Err(DD_EEXIST)
        && encode(&z, 512) == Err(DD_EINVAL)
        && encode(&e, 140) == Err(DD_ENOSPC)
        && encode(&e[0..1], 0) == Err(DD_EINVAL);
    out
}

/// 十三项判据。
pub fn run_unx11467_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11467.dirent_layout");
    let l = build_ledger();
    s.add("DD1-十四行齐备", l.len() == SAMPLES && l.sample(0).map(|x| x.op) == Some(OP_PLAIN), "static");
    s.add_dyn(
        "DD2-七应七拒",
        l.ok_count() == 7 && l.err_count() == 7 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );
    let e = build_entries();
    let full = encode(&e, 512);
    let offsets_ok = match &full {
        Ok(v) => {
            let want = [0u32, 16, 32, 48, 72, 96, 112, 128];
            let mut ok = v.rows.len() == want.len();
            let mut i = 0usize;
            while i < want.len() {
                if v.rows.get(i).map(|r| r.off) != want.get(i).copied() {
                    ok = false;
                }
                i += 1;
            }
            ok && v.total == 144
        }
        Err(_) => false,
    };
    s.add("DD3-偏移连续", offsets_ok, "static");
    s.add(
        "DD4-恰好填满",
        encode(&e, 144).map(|v| (v.total, v.slack)) == Ok((144, 0)),
        "static",
    );
    s.add(
        "DD5-容量边界",
        encode(&e, 140) == Err(DD_ENOSPC)
            && encode(&e[0..7], 128).map(|v| (v.total, v.slack)) == Ok((128, 0))
            && encode(&e[0..2], 24) == Err(DD_ENOSPC),
        "static",
    );
    s.add(
        "DD6-重名拒绝",
        {
            let mut d = build_entries();
            d.push(dirent(9, T_REG, b"alpha"));
            encode(&d, 512) == Err(DD_EEXIST)
        },
        "static",
    );
    s.add(
        "DD7-名字长度上限",
        {
            let mut d = build_entries();
            d.push(dirent(9, T_REG, b"0123456789abcde"));
            let fifteen = encode(&d, 512).is_ok();
            d.push(dirent(9, T_REG, b"0123456789abcdef"));
            fifteen && encode(&d, 512) == Err(DD_ENAMETOOLONG)
        },
        "static",
    );
    s.add(
        "DD8-非法字符",
        {
            let mut a = build_entries();
            a.push(dirent(9, T_REG, b"a/b"));
            let mut b = build_entries();
            b.push(Dirent { ino: 9, dtype: T_REG, name: [0u8; NAME_CAP], nlen: 0 });
            encode(&a, 512) == Err(DD_EINVAL) && encode(&b, 512) == Err(DD_EINVAL)
        },
        "static",
    );
    s.add(
        "DD9-类型与块大小",
        {
            let mut a = build_entries();
            a.push(dirent(9, 9, b"bad"));
            encode(&a, 512) == Err(DD_EINVAL)
                && encode(&e, 0) == Err(DD_EINVAL)
        },
        "static",
    );
    s.add(
        "DD10-空目录合法",
        encode(&[], 32).map(|v| (v.rows.len(), v.total, v.slack)) == Ok((0, 0, 32)),
        "static",
    );
    let tampered = match encode(&e, 512) {
        Ok(mut v) => {
            if let Some(r) = v.rows.get_mut(2) {
                r.reclen = 15;
            }
            audit_layout(&v) == Err(DD_EBAL)
        }
        Err(_) => false,
    };
    s.add("DD11-布局篡改检出", tampered, "static");
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(DD_EFROZEN);
    s.add_dyn(
        "DD12-封存与重放",
        root != 0 && rep == Ok(root) && again && build_ledger().seal() == build_ledger().seal(),
        &format!("root={:016x}", root),
    );
    s.add_dyn(
        "DD13-四路注入",
        inject_four() == [true; 4],
        &format!("inject={:?}", inject_four()),
    );
    s
}

fn summary(l: &Ledger) -> alloc::string::String {
    format!("ok={} err={} rows={}", l.ok_count(), l.err_count(), l.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alignment_is_eight() {
        assert_eq!(align8(9), 16);
        assert_eq!(align8(16), 16);
        assert_eq!(align8(17), 24);
    }

    #[test]
    fn full_directory_fits_exactly() {
        let l = encode(&build_entries(), 144);
        assert_eq!(l.as_ref().map(|v| v.total), Ok(144));
        assert_eq!(l.as_ref().map(|v| v.slack), Ok(0));
        assert_eq!(l.as_ref().map(|v| v.rows.len()), Ok(8));
        assert!(l.map(|v| audit_layout(&v).is_ok()).unwrap_or(false));
    }

    #[test]
    fn short_buffer_is_refused_whole() {
        assert_eq!(encode(&build_entries(), 140), Err(DD_ENOSPC));
        assert_eq!(encode(&build_entries()[0..2], 24), Err(DD_ENOSPC));
    }

    #[test]
    fn duplicate_names_rejected() {
        let mut d = build_entries();
        d.push(dirent(9, T_REG, b"zeta"));
        assert_eq!(encode(&d, 512), Err(DD_EEXIST));
    }

    #[test]
    fn long_names_rejected() {
        let mut d = build_entries();
        d.push(dirent(9, T_REG, b"0123456789abcdef"));
        assert_eq!(encode(&d, 512), Err(DD_ENAMETOOLONG));
    }

    #[test]
    fn empty_directory_is_valid() {
        let l = encode(&[], 32);
        assert_eq!(l.as_ref().map(|v| v.total), Ok(0));
        assert!(l.map(|v| audit_layout(&v).is_ok()).unwrap_or(false));
    }

    #[test]
    fn ledger_is_consistent() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 7);
        assert_eq!(l.err_count(), 7);
        assert!(l.audit().is_ok());
        let s = run_unx11467_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
    }
}
