//! UNX-F11468 路径词法规范化（0x20d10 段）
//!
//! 路径规范化不是简单的字符串替换，而是一次带状态的扫描：绝对路径的
//! `..` 在根处被吞掉，相对路径的 `..` 在开头必须保留，`...` 只是一个
//! 普通名字而不是“更上一层”。空组件（连续斜杠）和 `.` 组件被丢弃，
//! 但“输入是否有尾斜杠”仍要作为信息保留，因为 `rename("a/", "b")`
//! 与 `rename("a", "b")` 的语义不同。
//!
//! 本模块只做词法层：不解析符号链接，不访问磁盘，因此同样的字节串
//! 永远得到同样的规范形。组件长度上限与整条路径上限分别检查，先于
//! 任何写出动作；NUL 字节在扫描前就被拒绝。相对的“上跳”用计数器
//! 保存，而不是真的退到根外，从而让 `../b` 与 `a/../../b` 共用一条
//! 代码路径。
//!
//! 不变量：规范形中不出现空组件、`.` 或 `//`；绝对形的首字节必为 `/`；
//! 相对形若没有任何名字则输出 `.`；`depth == 名字数 - 未决上跳数`；
//! 再次规范化必须得到同一字节串（幂等）。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20d10;
pub const PN_EID: u32 = SEG_BASE + 0x01;
pub const PN_EORDER: u32 = SEG_BASE + 0x02;
pub const PN_EDUP: u32 = SEG_BASE + 0x03;
pub const PN_EFROZEN: u32 = SEG_BASE + 0x04;
pub const PN_ENOBS: u32 = SEG_BASE + 0x05;
pub const PN_EMISMATCH: u32 = SEG_BASE + 0x06;
pub const PN_EBAL: u32 = SEG_BASE + 0x07;
pub const PN_ESEAL: u32 = SEG_BASE + 0x08;
pub const PN_ENAMETOOLONG: u32 = SEG_BASE + 0x09;
pub const PN_EINVAL: u32 = SEG_BASE + 0x0a;
pub const PN_ENOENT: u32 = SEG_BASE + 0x0b;
pub const PN_EOVERFLOW: u32 = SEG_BASE + 0x0c;
pub const PN_EBADF: u32 = SEG_BASE + 0x0d;

pub const NAME_MAX: usize = 10;
pub const PATH_MAX: usize = 48;
pub const SAMPLES: usize = 14;

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

/// 单个名字组件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name {
    pub b: [u8; NAME_MAX],
    pub n: u8,
}

/// 规范化结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Norm {
    pub text: [u8; PATH_MAX],
    pub len: u8,
    pub depth: i16,
    pub dots: u8,
    pub ups: u8,
    pub absolute: bool,
    pub trailing: bool,
}

impl Norm {
    pub fn bytes(&self) -> &[u8] {
        let n = if (self.len as usize) > PATH_MAX { PATH_MAX } else { self.len as usize };
        match self.text.get(0..n) {
            Some(s) => s,
            None => &[],
        }
    }
}

fn seg_eq(seg: &[u8], lit: &[u8]) -> bool {
    if seg.len() != lit.len() {
        return false;
    }
    let mut i = 0usize;
    while i < seg.len() {
        match (seg.get(i), lit.get(i)) {
            (Some(a), Some(b)) => {
                if a != b {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

fn put(out: &mut [u8; PATH_MAX], idx: &mut usize, byte: u8) -> Result<(), u32> {
    if *idx >= PATH_MAX {
        return Err(PN_ENAMETOOLONG);
    }
    if let Some(p) = out.get_mut(*idx) {
        *p = byte;
    }
    *idx += 1;
    Ok(())
}

/// 词法规范化。
pub fn normalize(path: &[u8], keep_trailing: bool) -> Result<Norm, u32> {
    if path.is_empty() {
        return Err(PN_ENOENT);
    }
    if path.len() > PATH_MAX {
        return Err(PN_ENAMETOOLONG);
    }
    let mut k = 0usize;
    while k < path.len() {
        match path.get(k) {
            Some(&0) => return Err(PN_EINVAL),
            Some(_) => {}
            None => return Err(PN_ENOENT),
        }
        k += 1;
    }
    let absolute = path.first() == Some(&b'/');
    let mut live: Vec<Name> = Vec::new();
    let mut leading: u16 = 0;
    let mut dots: u8 = 0;
    let mut ups: u8 = 0;
    let mut i = 0usize;
    let mut start = 0usize;
    while i <= path.len() {
        let at_end = i == path.len();
        let is_sep = path.get(i) == Some(&b'/');
        if at_end || is_sep {
            let seg = match path.get(start..i) {
                Some(s) => s,
                None => &[],
            };
            if !seg.is_empty() {
                if seg_eq(seg, b".") {
                    dots = dots.saturating_add(1);
                } else if seg_eq(seg, b"..") {
                    ups = ups.saturating_add(1);
                    if !live.is_empty() {
                        let _ = live.pop();
                    } else if !absolute {
                        leading = leading.saturating_add(1);
                    }
                } else {
                    if seg.len() > NAME_MAX {
                        return Err(PN_ENAMETOOLONG);
                    }
                    let mut nm = Name { b: [0u8; NAME_MAX], n: seg.len() as u8 };
                    let mut j = 0usize;
                    while j < seg.len() {
                        if let (Some(d), Some(s)) = (nm.b.get_mut(j), seg.get(j)) {
                            *d = *s;
                        }
                        j += 1;
                    }
                    live.push(nm);
                }
            }
            start = i.saturating_add(1);
        }
        i += 1;
    }
    let trailing = path.last() == Some(&b'/') && !(absolute && live.is_empty() && leading == 0);
    let mut text = [0u8; PATH_MAX];
    let mut idx = 0usize;
    if absolute {
        put(&mut text, &mut idx, b'/')?;
    } else if live.is_empty() && leading == 0 {
        put(&mut text, &mut idx, b'.')?;
    }
    let mut u = 0u16;
    while u < leading {
        if idx > 0 {
            put(&mut text, &mut idx, b'/')?;
        }
        put(&mut text, &mut idx, b'.')?;
        put(&mut text, &mut idx, b'.')?;
        u += 1;
    }
    let mut j = 0usize;
    while j < live.len() {
        if let Some(nm) = live.get(j) {
            if idx > 0 && !(absolute && idx == 1) {
                put(&mut text, &mut idx, b'/')?;
            }
            let mut q = 0usize;
            while q < nm.n as usize {
                if let Some(&c) = nm.b.get(q) {
                    put(&mut text, &mut idx, c)?;
                }
                q += 1;
            }
        }
        j += 1;
    }
    if keep_trailing && trailing && idx > 1 {
        put(&mut text, &mut idx, b'/')?;
    }
    let depth = if absolute {
        live.len() as i16
    } else {
        (live.len() as i16).saturating_sub(leading as i16)
    };
    Ok(Norm {
        text,
        len: idx as u8,
        depth,
        dots,
        ups,
        absolute,
        trailing,
    })
}

/// 幂等检查（保留尾斜杠时与去掉尾斜杠的规范形比较）。
pub fn idempotent(n: &Norm) -> bool {
    match normalize(n.bytes(), false) {
        Ok(m) => {
            if m.bytes() == n.bytes() {
                return true;
            }
            if n.trailing && n.len > 1 {
                let cut = n.len as usize - 1;
                return n.text.get(0..cut) == Some(m.bytes());
            }
            false
        }
        Err(_) => false,
    }
}

/// 样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub id: u8,
    pub keep: bool,
    pub expect_ok: bool,
    pub len: u8,
    pub depth: i16,
    pub dots: u8,
    pub ups: u8,
    pub code: u16,
}

/// 结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    pub len: u8,
    pub depth: i16,
    pub dots: u8,
    pub ups: u8,
    pub code: u16,
}

fn push_bytes(v: &mut Vec<u8>, s: &[u8]) {
    let mut i = 0usize;
    while i < s.len() {
        if let Some(&b) = s.get(i) {
            v.push(b);
        }
        i += 1;
    }
}

/// 样本路径。
pub fn path_of(id: u8) -> Vec<u8> {
    let mut v = Vec::new();
    match id {
        0 => push_bytes(&mut v, b"/a/./b"),
        1 => push_bytes(&mut v, b"/a/../b"),
        2 => push_bytes(&mut v, b"/../.."),
        3 => push_bytes(&mut v, b"a//b/"),
        4 => push_bytes(&mut v, b"a/../../b"),
        5 => push_bytes(&mut v, b"."),
        6 => push_bytes(&mut v, b"/"),
        7 => push_bytes(&mut v, b"a/.../b"),
        8 => push_bytes(&mut v, b"/a/b/../.."),
        9 => push_bytes(&mut v, b"a/b/c"),
        10 => {}
        11 => push_bytes(&mut v, b"abcdefghijk"),
        12 => {
            let mut k = 0u8;
            while k < 5 {
                v.push(b'/');
                let mut q = 0u8;
                while q < 10 {
                    v.push(b'a' + k);
                    q += 1;
                }
                k += 1;
            }
        }
        13 => push_bytes(&mut v, b"/a\0b"),
        _ => {}
    }
    v
}

/// 账本。
pub struct Ledger {
    samples: Vec<Sample>,
    outcomes: Vec<Outcome>,
    texts: Vec<[u8; PATH_MAX]>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger {
            samples: Vec::new(),
            outcomes: Vec::new(),
            texts: Vec::new(),
            frozen: false,
            root: 0,
            rejects: 0,
        }
    }
    pub fn register(&mut self, slot: u8, id: u8, keep: bool, ok: bool, len: u8, depth: i16, dots: u8, ups: u8, code: u16) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(PN_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(PN_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(PN_EDUP);
        }
        if (slot as usize) != self.samples.len() {
            self.rejects += 1;
            return Err(PN_EORDER);
        }
        let s = Sample { id, keep, expect_ok: ok, len, depth, dots, ups, code };
        let p = path_of(id);
        let out = match normalize(&p, keep) {
            Ok(n) => {
                if !ok || n.len != len || n.depth != depth || n.dots != dots || n.ups != ups {
                    return Err(PN_EMISMATCH);
                }
                if !idempotent(&n) {
                    return Err(PN_EBAL);
                }
                Outcome { ok: true, len: n.len, depth: n.depth, dots: n.dots, ups: n.ups, code: 0 }
            }
            Err(e) => {
                let low = (e & 0xffff) as u16;
                if ok || low != code {
                    return Err(PN_EMISMATCH);
                }
                Outcome { ok: false, len: 0, depth: 0, dots: 0, ups: 0, code: low }
            }
        };
        let mut t = [0u8; PATH_MAX];
        if let Ok(n) = normalize(&p, keep) {
            let mut i = 0usize;
            while i < n.len as usize {
                if let (Some(d), Some(sb)) = (t.get_mut(i), n.text.get(i)) {
                    *d = *sb;
                }
                i += 1;
            }
        }
        self.samples.push(s);
        self.outcomes.push(out);
        self.texts.push(t);
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
    pub fn text(&self, i: usize) -> Option<[u8; PATH_MAX]> {
        self.texts.get(i).copied()
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
    /// 重算审计。
    pub fn audit(&self) -> Result<(), u32> {
        if self.samples.len() != self.outcomes.len() || self.samples.len() != self.texts.len() || self.samples.is_empty() {
            return Err(PN_EBAL);
        }
        let mut i = 0;
        while i < self.samples.len() {
            let s = match self.samples.get(i) {
                Some(v) => *v,
                None => return Err(PN_ENOBS),
            };
            let o = match self.outcomes.get(i) {
                Some(v) => *v,
                None => return Err(PN_ENOBS),
            };
            if s.expect_ok != o.ok {
                return Err(PN_EMISMATCH);
            }
            let p = path_of(s.id);
            match normalize(&p, s.keep) {
                Ok(n) => {
                    if !o.ok || n.len != o.len || n.depth != o.depth || n.dots != o.dots || n.ups != o.ups {
                        return Err(PN_EMISMATCH);
                    }
                    let t = match self.texts.get(i) {
                        Some(v) => v,
                        None => return Err(PN_ENOBS),
                    };
                    let mut j = 0usize;
                    while j < n.len as usize {
                        if t.get(j) != n.text.get(j) {
                            return Err(PN_EBAL);
                        }
                        j += 1;
                    }
                    if !idempotent(&n) {
                        return Err(PN_EBAL);
                    }
                }
                Err(e) => {
                    let low = (e & 0xffff) as u16;
                    if o.ok || low != o.code {
                        return Err(PN_EMISMATCH);
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
            return Err(PN_EFROZEN);
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
                h = fold(h, o.len as u64);
                h = fold(h, o.depth as u16 as u64);
                h = fold(h, o.dots as u64);
                h = fold(h, o.ups as u64);
                h = fold(h, o.code as u64);
            }
            if let Some(t) = self.texts.get(i) {
                let mut j = 0usize;
                while j < PATH_MAX {
                    if let Some(&b) = t.get(j) {
                        h = fold(h, b as u64);
                    }
                    j += 1;
                }
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
            Err(PN_ESEAL)
        }
    }
}

/// 十四行账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let elong = (PN_ENAMETOOLONG & 0xffff) as u16;
    let enoent = (PN_ENOENT & 0xffff) as u16;
    let einval = (PN_EINVAL & 0xffff) as u16;
    let _ = l.register(0, 0, false, true, 4, 2, 1, 0, 0);
    let _ = l.register(1, 1, false, true, 2, 1, 0, 1, 0);
    let _ = l.register(2, 2, false, true, 1, 0, 0, 2, 0);
    let _ = l.register(3, 3, true, true, 4, 2, 0, 0, 0);
    let _ = l.register(4, 4, false, true, 4, 0, 0, 2, 0);
    let _ = l.register(5, 5, false, true, 1, 0, 1, 0, 0);
    let _ = l.register(6, 6, false, true, 1, 0, 0, 0, 0);
    let _ = l.register(7, 7, false, true, 7, 3, 0, 0, 0);
    let _ = l.register(8, 8, false, true, 1, 0, 0, 2, 0);
    let _ = l.register(9, 9, false, true, 5, 3, 0, 0, 0);
    let _ = l.register(10, 10, false, false, 0, 0, 0, 0, enoent);
    let _ = l.register(11, 11, false, false, 0, 0, 0, 0, elong);
    let _ = l.register(12, 12, false, false, 0, 0, 0, 0, elong);
    let _ = l.register(13, 13, false, false, 0, 0, 0, 0, einval);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.ok_count() == 10 && l.err_count() == 4 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let twice = l.freeze() == Err(PN_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && twice;

    let n = match normalize(b"/a//./b/../c/", false) {
        Ok(v) => v,
        Err(_) => return out,
    };
    out[1] = n.bytes() == b"/a/c"
        && n.len == 4
        && n.depth == 2
        && n.dots == 1
        && n.ups == 1
        && n.trailing
        && n.absolute;

    let mut up: Vec<u8> = Vec::new();
    push_bytes(&mut up, b"../../x");
    let r = match normalize(&up, false) {
        Ok(v) => v,
        Err(_) => return out,
    };
    out[2] = r.bytes() == b"../../x" && r.depth == -1 && r.ups == 2 && !r.absolute;

    let done = normalize(n.bytes(), false);
    let root_only = normalize(b"/", false);
    out[3] = match (done, root_only) {
        (Ok(a), Ok(b)) => a.bytes() == n.bytes() && b.bytes() == b"/" && b.depth == 0
            && normalize(b"", false) == Err(PN_ENOENT)
            && normalize(b"a\0b", false) == Err(PN_EINVAL)
            && normalize(b"abcdefghijk", false) == Err(PN_ENAMETOOLONG),
        _ => false,
    };
    out
}

/// 十三项判据。
pub fn run_unx11468_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11468.path_normalize");
    let l = build_ledger();
    s.add("PN1-十四行齐备", l.len() == SAMPLES && l.sample(0).map(|x| x.id) == Some(0), "static");
    s.add_dyn(
        "PN2-十应四拒",
        l.ok_count() == 10 && l.err_count() == 4 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );
    s.add(
        "PN3-点组件剔除",
        normalize(&path_of(0), false).map(|n| (n.len, n.dots, n.depth)) == Ok((4, 1, 2))
            && l.text(0).map(|t| t.get(0..4) == Some(b"/a/b")) == Some(true),
        "static",
    );
    s.add(
        "PN4-父组件回退",
        normalize(&path_of(1), false).map(|n| (n.len, n.ups, n.depth)) == Ok((2, 1, 1)),
        "static",
    );
    s.add(
        "PN5-根部钳制",
        normalize(&path_of(2), false).map(|n| (n.len, n.ups, n.depth)) == Ok((1, 2, 0)),
        "static",
    );
    s.add(
        "PN6-保留尾斜杠",
        normalize(&path_of(3), true).map(|n| (n.len, n.trailing)) == Ok((4, true))
            && normalize(&path_of(3), false).map(|n| n.len) == Ok(3),
        "static",
    );
    s.add(
        "PN7-相对未决上跳",
        normalize(&path_of(4), false).map(|n| (n.len, n.depth, n.ups)) == Ok((4, 0, 2))
            && normalize(&path_of(5), false).map(|n| n.len) == Ok(1),
        "static",
    );
    s.add(
        "PN8-根与三连点",
        normalize(&path_of(6), false).map(|n| (n.len, n.absolute)) == Ok((1, true))
            && normalize(&path_of(7), false).map(|n| (n.len, n.depth)) == Ok((7, 3)),
        "static",
    );
    s.add(
        "PN9-多级回退到根",
        normalize(&path_of(8), false).map(|n| (n.len, n.ups, n.depth)) == Ok((1, 2, 0)),
        "static",
    );
    s.add(
        "PN10-普通相对路径",
        normalize(&path_of(9), false).map(|n| (n.len, n.depth)) == Ok((5, 3))
            && normalize(b"a/b/", false).map(|n| n.len) == Ok(3),
        "static",
    );
    s.add(
        "PN11-空路径与空字节",
        normalize(b"", false) == Err(PN_ENOENT)
            && normalize(b"a\0b", false) == Err(PN_EINVAL),
        "static",
    );
    s.add(
        "PN12-长度双限",
        normalize(b"abcdefghijk", false) == Err(PN_ENAMETOOLONG)
            && normalize(&path_of(12), false) == Err(PN_ENAMETOOLONG)
            && normalize(b"abcdefghij", false).map(|n| n.len) == Ok(10),
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(PN_EFROZEN);
    s.add_dyn(
        "PN13-冻结重放与幂等",
        root != 0
            && rep == Ok(root)
            && again
            && normalize(b"/a/./b/../c", false).map(|n| idempotent(&n)) == Ok(true)
            && inject_four() == [true; 4],
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
    fn dots_are_removed() {
        let n = normalize(b"/a/./b", false);
        assert_eq!(n.map(|v| v.len), Ok(4));
        assert_eq!(n.map(|v| v.dots), Ok(1));
    }

    #[test]
    fn parents_pop_names() {
        let n = normalize(b"/a/../b", false);
        assert_eq!(n.map(|v| (v.len, v.ups, v.depth)), Ok((2, 1, 1)));
    }

    #[test]
    fn root_absorbs_parents() {
        let n = normalize(b"/../..", false);
        assert_eq!(n.map(|v| (v.len, v.ups, v.depth)), Ok((1, 2, 0)));
    }

    #[test]
    fn relative_parents_are_kept() {
        let n = normalize(b"a/../../b", false);
        assert_eq!(n.map(|v| v.len), Ok(4));
        assert_eq!(n.map(|v| v.depth), Ok(0));
    }

    #[test]
    fn trailing_slash_is_reportable() {
        let n = normalize(b"a//b/", true);
        assert_eq!(n.map(|v| (v.len, v.trailing)), Ok((4, true)));
    }

    #[test]
    fn length_limits_apply() {
        assert_eq!(normalize(b"abcdefghijk", false), Err(PN_ENAMETOOLONG));
        assert_eq!(normalize(b"", false), Err(PN_ENOENT));
    }

    #[test]
    fn normalization_is_idempotent() {
        let n = match normalize(b"/a/b/../c/./d", false) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(idempotent(&n));
    }

    #[test]
    fn ledger_is_consistent() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 10);
        assert_eq!(l.err_count(), 4);
        assert!(l.audit().is_ok());
        let s = run_unx11468_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
    }
}
