//! UNX-F11470 目录粘滞位删除规则（0x20d30 段）
//!
//! 带粘滞位（`S_ISVTX`）的目录（典型如 `/tmp`）改变的是“谁能删除”：
//! 即使目录本身对所有人可写，删除或改名一个条目仍要求调用者是目录属主、
//! 条目属主，或持有特权。非粘滞目录只检查目录写权限，条目的属主是谁
//! 无关紧要。本模块把这条规则独立成谓词，供 unlink 与 rename 共用。
//!
//! rename 有两端：源条目与（可能存在的）目标条目。粘滞位分别检查两端，
//! 因此“换掉别人的文件”与“删掉别人的文件”同样被拒绝；带 `NOREPLACE`
//! 时目标存在直接 `EEXIST`，不再进入权限判定。目录条目不能被 unlink，
//! 必须走 rmdir，否则返回 `EISDIR`。
//!
//! 不变量：任何被拒绝的操作都不改变条目数；成功替换会恰好减少一个条目；
//! 成功改名到新名字保持条目数；空名字、含斜杠、超长名字在查找前拒绝。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20d30;
pub const ST_EID: u32 = SEG_BASE + 0x01;
pub const ST_EORDER: u32 = SEG_BASE + 0x02;
pub const ST_EDUP: u32 = SEG_BASE + 0x03;
pub const ST_EFROZEN: u32 = SEG_BASE + 0x04;
pub const ST_ENOBS: u32 = SEG_BASE + 0x05;
pub const ST_EMISMATCH: u32 = SEG_BASE + 0x06;
pub const ST_EBAL: u32 = SEG_BASE + 0x07;
pub const ST_ESEAL: u32 = SEG_BASE + 0x08;
pub const ST_EPERM: u32 = SEG_BASE + 0x09;
pub const ST_ENOENT: u32 = SEG_BASE + 0x0a;
pub const ST_EINVAL: u32 = SEG_BASE + 0x0b;
pub const ST_EISDIR: u32 = SEG_BASE + 0x0c;
pub const ST_EEXIST: u32 = SEG_BASE + 0x0d;
pub const ST_EBADF: u32 = SEG_BASE + 0x0e;

pub const S_ISVTX: u16 = 0o1000;
pub const NAME_MAX: usize = 10;
pub const NAME_CAP: usize = 12;
pub const SAMPLES: usize = 14;

pub const K_FILE: u8 = 1;
pub const K_DIR: u8 = 2;

pub const OP_UNLINK: u8 = 1;
pub const OP_RENAME: u8 = 2;

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

/// 主体。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Subj {
    pub uid: u16,
    pub cap: bool,
}

/// 目录条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Item {
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
    pub owner: u16,
    pub kind: u8,
}

/// 目录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dir {
    pub uid: u16,
    pub mode: u16,
    pub items: Vec<Item>,
}

/// 操作结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Out {
    pub removed: bool,
    pub renamed: bool,
    pub entries: u8,
}

fn name_bytes(s: &[u8]) -> (u8, [u8; NAME_CAP]) {
    let mut b = [0u8; NAME_CAP];
    let mut i = 0usize;
    while i < s.len() && i < NAME_CAP {
        if let (Some(d), Some(&c)) = (b.get_mut(i), s.get(i)) {
            *d = c;
        }
        i += 1;
    }
    (i as u8, b)
}

fn same_name(e: &Item, s: &[u8]) -> bool {
    if (e.nlen as usize) != s.len() {
        return false;
    }
    let mut i = 0usize;
    while i < s.len() {
        if e.name.get(i) != s.get(i) {
            return false;
        }
        i += 1;
    }
    true
}

fn valid_name(s: &[u8]) -> bool {
    if s.is_empty() || s.len() > NAME_MAX {
        return false;
    }
    let mut i = 0usize;
    while i < s.len() {
        match s.get(i) {
            Some(&c) => {
                if c == 0 || c == b'/' {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

impl Dir {
    pub fn entry(&self, name: &[u8]) -> Option<Item> {
        let mut i = 0usize;
        while i < self.items.len() {
            if let Some(e) = self.items.get(i) {
                if same_name(e, name) {
                    return Some(*e);
                }
            }
            i += 1;
        }
        None
    }
    fn index(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.items.len() {
            if let Some(e) = self.items.get(i) {
                if same_name(e, name) {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }
    pub fn sticky(&self) -> bool {
        self.mode & S_ISVTX != 0
    }
    pub fn may_remove(&self, subj: &Subj, it: &Item) -> bool {
        if subj.cap {
            return true;
        }
        if !self.sticky() {
            return true;
        }
        subj.uid == self.uid || subj.uid == it.owner
    }
    fn rename_raw(&mut self, from: usize, to: &[u8]) {
        if let Some(e) = self.items.get_mut(from) {
            let (n, b) = name_bytes(to);
            e.name = b;
            e.nlen = n;
        }
    }
}

/// unlink 语义。
pub fn unlink(d: &mut Dir, name: &[u8], subj: &Subj) -> Result<Out, u32> {
    if !valid_name(name) {
        return Err(ST_EINVAL);
    }
    let idx = match d.index(name) {
        Some(v) => v,
        None => return Err(ST_ENOENT),
    };
    let it = match d.items.get(idx) {
        Some(v) => *v,
        None => return Err(ST_ENOENT),
    };
    if it.kind == K_DIR {
        return Err(ST_EISDIR);
    }
    if !d.may_remove(subj, &it) {
        return Err(ST_EPERM);
    }
    let _ = d.items.remove(idx);
    Ok(Out { removed: true, renamed: false, entries: d.items.len() as u8 })
}

/// rename 语义（同目录内）。
pub fn rename(d: &mut Dir, from: &[u8], to: &[u8], subj: &Subj, noreplace: bool) -> Result<Out, u32> {
    if !valid_name(from) || !valid_name(to) {
        return Err(ST_EINVAL);
    }
    let fi = match d.index(from) {
        Some(v) => v,
        None => return Err(ST_ENOENT),
    };
    let src = match d.items.get(fi) {
        Some(v) => *v,
        None => return Err(ST_ENOENT),
    };
    if !d.may_remove(subj, &src) {
        return Err(ST_EPERM);
    }
    if let Some(ti) = d.index(to) {
        if noreplace {
            return Err(ST_EEXIST);
        }
        let tgt = match d.items.get(ti) {
            Some(v) => *v,
            None => return Err(ST_ENOENT),
        };
        if !d.may_remove(subj, &tgt) {
            return Err(ST_EPERM);
        }
        if tgt.kind == K_DIR && src.kind != K_DIR {
            return Err(ST_EISDIR);
        }
        let _ = d.items.remove(ti);
        let src_idx = if ti < fi { fi - 1 } else { fi };
        d.rename_raw(src_idx, to);
    } else {
        d.rename_raw(fi, to);
    }
    Ok(Out { removed: false, renamed: true, entries: d.items.len() as u8 })
}

/// 基础目录。
pub fn base_dir(sticky: bool) -> Dir {
    let mut items = Vec::new();
    let mk = |n: &[u8], owner: u16, kind: u8| {
        let (nlen, name) = name_bytes(n);
        Item { name, nlen, owner, kind }
    };
    items.push(mk(b"alpha", 1000, K_FILE));
    items.push(mk(b"beta", 2000, K_FILE));
    items.push(mk(b"gamma", 3000, K_DIR));
    items.push(mk(b"delta", 1000, K_FILE));
    Dir {
        uid: 1000,
        mode: if sticky { 0o1755 } else { 0o755 },
        items,
    }
}

fn subj(uid: u16, cap: bool) -> Subj {
    Subj { uid, cap }
}

/// 场景执行。
pub fn run_scen(id: u8) -> Result<Out, u32> {
    match id {
        0 => {
            let mut d = base_dir(false);
            unlink(&mut d, b"beta", &subj(3000, false))
        }
        1 => {
            let mut d = base_dir(true);
            unlink(&mut d, b"beta", &subj(1000, false))
        }
        2 => {
            let mut d = base_dir(true);
            unlink(&mut d, b"beta", &subj(2000, false))
        }
        3 => {
            let mut d = base_dir(true);
            unlink(&mut d, b"beta", &subj(3000, false))
        }
        4 => {
            let mut d = base_dir(true);
            unlink(&mut d, b"beta", &subj(3000, true))
        }
        5 => {
            let mut d = base_dir(true);
            rename(&mut d, b"beta", b"epsilon", &subj(3000, false), false)
        }
        6 => {
            let mut d = base_dir(true);
            rename(&mut d, b"beta", b"epsilon", &subj(1000, false), false)
        }
        7 => {
            let mut d = base_dir(true);
            rename(&mut d, b"beta", b"epsilon", &subj(2000, false), false)
        }
        8 => {
            let mut d = base_dir(false);
            rename(&mut d, b"beta", b"epsilon", &subj(3000, false), false)
        }
        9 => {
            let mut d = base_dir(true);
            unlink(&mut d, b"alpha", &subj(1000, false))
        }
        10 => {
            let mut d = base_dir(true);
            rename(&mut d, b"beta", b"alpha", &subj(1000, false), false)
        }
        11 => {
            let mut d = base_dir(true);
            unlink(&mut d, b"missing", &subj(1000, false))
        }
        12 => {
            let mut d = base_dir(true);
            rename(&mut d, b"beta", b"", &subj(1000, false), false)
        }
        13 => {
            let mut d = base_dir(true);
            unlink(&mut d, b"gamma", &subj(1000, false))
        }
        _ => Err(ST_EBADF),
    }
}

/// 样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub scen: u8,
    pub expect_ok: bool,
    pub removed: bool,
    pub renamed: bool,
    pub entries: u8,
    pub code: u16,
}

/// 结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    pub removed: bool,
    pub renamed: bool,
    pub entries: u8,
    pub code: u16,
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
    pub fn register(&mut self, slot: u8, scen: u8, ok: bool, removed: bool, renamed: bool, entries: u8, code: u16) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(ST_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(ST_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(ST_EDUP);
        }
        if (slot as usize) != self.samples.len() {
            self.rejects += 1;
            return Err(ST_EORDER);
        }
        let s = Sample { scen, expect_ok: ok, removed, renamed, entries, code };
        let out = match run_scen(scen) {
            Ok(o) => {
                if !ok || o.removed != removed || o.renamed != renamed || o.entries != entries {
                    return Err(ST_EMISMATCH);
                }
                Outcome { ok: true, removed: o.removed, renamed: o.renamed, entries: o.entries, code: 0 }
            }
            Err(e) => {
                let low = (e & 0xffff) as u16;
                if ok || low != code {
                    return Err(ST_EMISMATCH);
                }
                Outcome { ok: false, removed: false, renamed: false, entries: 4, code: low }
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
    pub fn audit(&self) -> Result<(), u32> {
        if self.samples.len() != self.outcomes.len() || self.samples.is_empty() {
            return Err(ST_EBAL);
        }
        let mut i = 0;
        while i < self.samples.len() {
            let s = match self.samples.get(i) {
                Some(v) => *v,
                None => return Err(ST_ENOBS),
            };
            let o = match self.outcomes.get(i) {
                Some(v) => *v,
                None => return Err(ST_ENOBS),
            };
            if s.expect_ok != o.ok {
                return Err(ST_EMISMATCH);
            }
            match run_scen(s.scen) {
                Ok(v) => {
                    if !o.ok || v.removed != o.removed || v.renamed != o.renamed || v.entries != o.entries {
                        return Err(ST_EMISMATCH);
                    }
                    if v.removed && v.renamed {
                        return Err(ST_EBAL);
                    }
                }
                Err(e) => {
                    let low = (e & 0xffff) as u16;
                    if o.ok || low != o.code {
                        return Err(ST_EMISMATCH);
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
            return Err(ST_EFROZEN);
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
                h = fold(h, u64::from(o.removed));
                h = fold(h, u64::from(o.renamed));
                h = fold(h, o.entries as u64);
                h = fold(h, o.code as u64);
            }
            if let Some(s) = self.samples.get(i) {
                h = fold(h, s.scen as u64);
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
            Err(ST_ESEAL)
        }
    }
}

/// 十四行账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let eperm = (ST_EPERM & 0xffff) as u16;
    let enoent = (ST_ENOENT & 0xffff) as u16;
    let einval = (ST_EINVAL & 0xffff) as u16;
    let eisdir = (ST_EISDIR & 0xffff) as u16;
    let _ = l.register(0, 0, true, true, false, 3, 0);
    let _ = l.register(1, 1, true, true, false, 3, 0);
    let _ = l.register(2, 2, true, true, false, 3, 0);
    let _ = l.register(3, 3, false, false, false, 4, eperm);
    let _ = l.register(4, 4, true, true, false, 3, 0);
    let _ = l.register(5, 5, false, false, false, 4, eperm);
    let _ = l.register(6, 6, true, false, true, 4, 0);
    let _ = l.register(7, 7, true, false, true, 4, 0);
    let _ = l.register(8, 8, true, false, true, 4, 0);
    let _ = l.register(9, 9, true, true, false, 3, 0);
    let _ = l.register(10, 10, true, false, true, 3, 0);
    let _ = l.register(11, 11, false, false, false, 4, enoent);
    let _ = l.register(12, 12, false, false, false, 4, einval);
    let _ = l.register(13, 13, false, false, false, 4, eisdir);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.ok_count() == 9 && l.err_count() == 5 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let twice = l.freeze() == Err(ST_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && twice;

    let d = base_dir(true);
    out[1] = match d.entry(b"beta") {
        Some(it) => {
            d.may_remove(&subj(1000, false), &it)
                && d.may_remove(&subj(2000, false), &it)
                && !d.may_remove(&subj(3000, false), &it)
                && d.may_remove(&subj(3000, true), &it)
        }
        None => false,
    };

    let mut a = base_dir(true);
    let denied = unlink(&mut a, b"beta", &subj(3000, false)) == Err(ST_EPERM);
    let allowed = unlink(&mut a, b"beta", &subj(1000, false));
    out[2] = denied
        && allowed.map(|o| (o.removed, o.entries)) == Ok((true, 3))
        && a.entry(b"beta").is_none()
        && unlink(&mut a, b"missing", &subj(1000, false)) == Err(ST_ENOENT);

    let mut b = base_dir(true);
    let busy = rename(&mut b, b"beta", b"alpha", &subj(1000, false), true) == Err(ST_EEXIST);
    let rep = rename(&mut b, b"beta", b"alpha", &subj(1000, false), false);
    out[3] = busy
        && rep.map(|o| (o.renamed, o.entries)) == Ok((true, 3))
        && b.entry(b"alpha").map(|e| e.owner) == Some(2000)
        && b.entry(b"beta").is_none();
    out
}

/// 十三项判据。
pub fn run_unx11470_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11470_sticky_dir");
    let l = build_ledger();
    s.add("SB1-十四行齐备", l.len() == SAMPLES && l.sample(0).map(|x| x.scen) == Some(0), "static");
    s.add_dyn(
        "SB2-九应五拒",
        l.ok_count() == 9 && l.err_count() == 5 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );
    s.add(
        "SB3-非粘滞目录放行",
        {
            let mut d = base_dir(false);
            unlink(&mut d, b"beta", &subj(3000, false)).map(|o| (o.removed, o.entries)) == Ok((true, 3))
        },
        "static",
    );
    s.add(
        "SB4-目录属主放行",
        {
            let mut d = base_dir(true);
            unlink(&mut d, b"beta", &subj(1000, false)).map(|o| o.removed) == Ok(true)
        },
        "static",
    );
    s.add(
        "SB5-条目属主放行",
        {
            let mut d = base_dir(true);
            unlink(&mut d, b"beta", &subj(2000, false)).map(|o| o.removed) == Ok(true)
        },
        "static",
    );
    s.add(
        "SB6-陌生人拒绝",
        {
            let mut d = base_dir(true);
            unlink(&mut d, b"beta", &subj(3000, false)) == Err(ST_EPERM)
                && d.entry(b"beta").is_some()
        },
        "static",
    );
    s.add(
        "SB7-能力位覆盖",
        {
            let mut d = base_dir(true);
            unlink(&mut d, b"beta", &subj(3000, true)).map(|o| o.removed) == Ok(true)
        },
        "static",
    );
    s.add(
        "SB8-改名检查源条目",
        {
            let mut d = base_dir(true);
            rename(&mut d, b"beta", b"epsilon", &subj(3000, false), false) == Err(ST_EPERM)
                && d.entry(b"beta").is_some()
        },
        "static",
    );
    s.add(
        "SB9-改名保持条目数",
        {
            let mut d = base_dir(true);
            rename(&mut d, b"beta", b"epsilon", &subj(2000, false), false)
                .map(|o| (o.renamed, o.entries)) == Ok((true, 4))
                && d.entry(b"epsilon").is_some()
        },
        "static",
    );
    s.add(
        "SB10-替换减少一条",
        {
            let mut d = base_dir(true);
            rename(&mut d, b"beta", b"alpha", &subj(1000, false), false)
                .map(|o| (o.renamed, o.entries)) == Ok((true, 3))
                && d.entry(b"alpha").map(|e| e.owner) == Some(2000)
        },
        "static",
    );
    s.add(
        "SB11-缺失与空名",
        {
            let mut d = base_dir(true);
            unlink(&mut d, b"missing", &subj(1000, false)) == Err(ST_ENOENT)
                && rename(&mut d, b"beta", b"", &subj(1000, false), false) == Err(ST_EINVAL)
        },
        "static",
    );
    s.add(
        "SB12-目录不可 unlink",
        {
            let mut d = base_dir(true);
            unlink(&mut d, b"gamma", &subj(1000, false)) == Err(ST_EISDIR)
                && d.entry(b"gamma").is_some()
        },
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(ST_EFROZEN);
    s.add_dyn(
        "SB13-冻结重放与四路",
        root != 0 && rep == Ok(root) && again && inject_four() == [true; 4],
        &format!("root={:016x}", root),
    );
    s
}

/// 摘要。
pub fn summary(l: &Ledger) -> alloc::string::String {
    format!("ok={} err={} rows={}", l.ok_count(), l.err_count(), l.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_sticky_allows_stranger() {
        let mut d = base_dir(false);
        assert_eq!(unlink(&mut d, b"beta", &subj(3000, false)).map(|o| o.removed), Ok(true));
    }

    #[test]
    fn sticky_blocks_stranger() {
        let mut d = base_dir(true);
        assert_eq!(unlink(&mut d, b"beta", &subj(3000, false)), Err(ST_EPERM));
        assert!(d.entry(b"beta").is_some());
    }

    #[test]
    fn sticky_allows_owners() {
        let mut d = base_dir(true);
        assert_eq!(unlink(&mut d, b"beta", &subj(1000, false)).map(|o| o.removed), Ok(true));
        let mut e = base_dir(true);
        assert_eq!(unlink(&mut e, b"beta", &subj(2000, false)).map(|o| o.removed), Ok(true));
    }

    #[test]
    fn sticky_checks_rename_target() {
        let mut d = base_dir(true);
        d.uid = 2000;
        d.items.push(Item { name: [0u8; NAME_CAP], nlen: 0, owner: 4000, kind: K_FILE });
        if let Some(it) = d.items.last_mut() {
            let (n, b) = name_bytes(b"omega");
            it.name = b;
            it.nlen = n;
        }
        assert_eq!(rename(&mut d, b"alpha", b"omega", &subj(1000, false), false), Err(ST_EPERM));
    }

    #[test]
    fn noreplace_reports_eexist() {
        let mut d = base_dir(true);
        assert_eq!(rename(&mut d, b"beta", b"alpha", &subj(1000, false), true), Err(ST_EEXIST));
    }

    #[test]
    fn directories_need_rmdir() {
        let mut d = base_dir(true);
        assert_eq!(unlink(&mut d, b"gamma", &subj(1000, false)), Err(ST_EISDIR));
    }

    #[test]
    fn ledger_is_consistent() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 9);
        assert_eq!(l.err_count(), 5);
        assert!(l.audit().is_ok());
        let s = run_unx11470_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
    }
}
