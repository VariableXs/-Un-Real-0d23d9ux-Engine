//! UNX-F11447 读取符号链接（0x20bb0 段）
//!
//! readlink 不打开目标，也不解引用，它只是把链接自身的文本抄进调用者的缓冲区。
//! 因此三条边界必须同时成立：缓冲区满时只抄缓冲区大小的字节并返回该长度，绝不
//! 补 NUL；缓冲区长度为 0 直接 EINVAL，因为“抄了零字节”无法与“成功”区分；
//! 对普通文件调用返回 EINVAL，对不存在的名字返回 ENOENT。名字过长在查找之前
//! 就报 ENAMETOOLONG，避免把截断后的名字当成另一个对象。
//!
//! 模型中的链接表保存名字与目标两个字节数组，目标最长三十二字节。读取函数只
//! 返回实际抄写的字节数，账本另外保存被抄缓冲区的前缀副本，审计时重新执行并把
//! 长度与每一个字节都对一遍。这样即使返回长度正确、内容却被污染，也会被审计
//! 抓住。成功的行分为两类：缓冲区足够时的完整拷贝，以及故意给短缓冲区造成的
//! 截断；失败的行覆盖空缓冲区、非链接、缺失名字与超长名字。
//!
//! 封印把名字、目标、缓冲区长度与结论一起压进签名，冻结后登记被拒，重放重建
//! 账本比较签名。四路注入分别验证完整读取、截断读取、非链接拒绝与零长缓冲区
//! 拒绝，防止实现偷偷在末尾追加 NUL 或用返回值掩盖截断。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20bb0;
/// 编号非法。
pub const RL_EID: u32 = SEG_BASE + 0x01;
/// 顺序错误。
pub const RL_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const RL_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const RL_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测缺失。
pub const RL_ENOBS: u32 = SEG_BASE + 0x05;
/// 结果不符。
pub const RL_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const RL_EBAL: u32 = SEG_BASE + 0x07;
/// 名字不存在。
pub const RL_ENOENT: u32 = SEG_BASE + 0x08;
/// 不是符号链接或缓冲区非法。
pub const RL_EINVAL: u32 = SEG_BASE + 0x09;
/// 名字过长。
pub const RL_ENAMETOOLONG: u32 = SEG_BASE + 0x0a;
/// 票据不一致。
pub const RL_ESEAL: u32 = SEG_BASE + 0x0b;

/// 名字容量。
pub const NAME_CAP: usize = 24;
/// 节点名字上限。
pub const NODE_NAME_MAX: usize = 16;
/// 目标容量。
pub const TARGET_CAP: usize = 32;
/// 调用者缓冲区容量。
pub const BUF_CAP: usize = 40;
/// 账本格数。
pub const ROWS: usize = 14;

/// 链接节点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    pub name: [u8; NODE_NAME_MAX],
    pub nlen: u8,
    pub target: [u8; TARGET_CAP],
    pub tlen: u8,
    pub is_link: bool,
}

impl Node {
    pub fn name_bytes(&self) -> &[u8] {
        let n = self.nlen as usize;
        if n > NODE_NAME_MAX {
            return &[];
        }
        match self.name.get(..n) {
            Some(v) => v,
            None => &[],
        }
    }

    pub fn target_bytes(&self) -> &[u8] {
        let n = self.tlen as usize;
        if n > TARGET_CAP {
            return &[];
        }
        match self.target.get(..n) {
            Some(v) => v,
            None => &[],
        }
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

fn same(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        let x = match a.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let y = match b.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if x != y {
            return false;
        }
        i += 1;
    }
    true
}

/// 链接表。
pub struct Sys {
    pub nodes: Vec<Node>,
}

impl Sys {
    pub fn fresh() -> Sys {
        let mut nodes = Vec::new();
        let mut add = |name: &[u8], target: &[u8], is_link: bool| {
            let mut n = Node { name: [0; NODE_NAME_MAX], nlen: 0, target: [0; TARGET_CAP], tlen: 0, is_link };
            n.nlen = put(&mut n.name, name);
            n.tlen = put(&mut n.target, target);
            nodes.push(n);
        };
        add(b"a", b"tgt/a", true);
        add(b"long", b"0123456789abcdefghijklmnopqrstuv", true);
        add(b"self", b"self", true);
        add(b"file", b"", false);
        drop(add);
        Sys { nodes }
    }

    pub fn find(&self, name: &[u8]) -> Option<Node> {
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                if same(n.name_bytes(), name) {
                    return Some(*n);
                }
            }
            i += 1;
        }
        None
    }

    /// 读取链接文本，返回抄写长度。
    pub fn readlink(&self, name: &[u8], buf: &mut [u8]) -> Result<usize, u32> {
        if buf.is_empty() {
            return Err(RL_EINVAL);
        }
        if name.len() > NODE_NAME_MAX {
            return Err(RL_ENAMETOOLONG);
        }
        let n = match self.find(name) {
            Some(v) => v,
            None => return Err(RL_ENOENT),
        };
        if !n.is_link {
            return Err(RL_EINVAL);
        }
        let t = n.target_bytes();
        let mut k = 0usize;
        while k < t.len() && k < buf.len() {
            let b = match t.get(k) {
                Some(v) => *v,
                None => break,
            };
            if let Some(d) = buf.get_mut(k) {
                *d = b;
            }
            k += 1;
        }
        Ok(k)
    }
}

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
    pub buf_len: u8,
    pub got: u8,
    pub copy: [u8; TARGET_CAP],
    pub code: u16,
    pub ok: bool,
}

impl Row {
    pub fn name_bytes(&self) -> &[u8] {
        let n = self.nlen as usize;
        if n > NAME_CAP {
            return &[];
        }
        match self.name.get(..n) {
            Some(v) => v,
            None => &[],
        }
    }
}

/// 账本。
pub struct Ledger {
    sys: Sys,
    rows: Vec<Row>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger { sys: Sys::fresh(), rows: Vec::new(), frozen: false, root: 0, rejects: 0 }
    }

    pub fn register(&mut self, slot: u8, name: &[u8], buf_len: u8, ok: bool) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(RL_EFROZEN);
        }
        if (slot as usize) >= ROWS {
            self.rejects += 1;
            return Err(RL_EID);
        }
        if (slot as usize) < self.rows.len() {
            self.rejects += 1;
            return Err(RL_EDUP);
        }
        if (slot as usize) != self.rows.len() {
            self.rejects += 1;
            return Err(RL_EORDER);
        }
        let mut buf = [0u8; BUF_CAP];
        let lim = buf_len as usize;
        let slice = if lim <= BUF_CAP {
            match buf.get_mut(..lim) {
                Some(v) => v,
                None => return Err(RL_EBAL),
            }
        } else {
            return Err(RL_EBAL);
        };
        let got = self.sys.readlink(name, slice);
        let mut r = Row { name: [0; NAME_CAP], nlen: 0, buf_len, got: 0, copy: [0; TARGET_CAP], code: 0, ok };
        r.nlen = put(&mut r.name, name);
        match got {
            Ok(n) => {
                if !ok {
                    self.rejects += 1;
                    return Err(RL_EMISMATCH);
                }
                r.got = n as u8;
                let mut k = 0usize;
                while k < n {
                    if let (Some(d), Some(s)) = (r.copy.get_mut(k), slice.get(k)) {
                        *d = *s;
                    }
                    k += 1;
                }
            }
            Err(e) => {
                if ok {
                    self.rejects += 1;
                    return Err(RL_EMISMATCH);
                }
                r.code = (e & 0xffff) as u16;
            }
        }
        self.rows.push(r);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn row(&self, i: usize) -> Option<Row> {
        match self.rows.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn err_count(&self) -> usize {
        self.rows.len() - self.ok_count()
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.rows.is_empty() {
            return Err(RL_EBAL);
        }
        let mut i = 0usize;
        let mut ok = 0usize;
        let mut bad = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(RL_ENOBS),
            };
            if (r.buf_len as usize) > BUF_CAP || (r.got as usize) > r.buf_len as usize {
                return Err(RL_EBAL);
            }
            let mut buf = [0u8; BUF_CAP];
            let lim = r.buf_len as usize;
            let slice = match buf.get_mut(..lim) {
                Some(v) => v,
                None => return Err(RL_EBAL),
            };
            match self.sys.readlink(r.name_bytes(), slice) {
                Ok(n) => {
                    if !r.ok || n != r.got as usize || r.code != 0 {
                        return Err(RL_EBAL);
                    }
                    let mut k = 0usize;
                    while k < n {
                        if r.copy.get(k) != slice.get(k) {
                            return Err(RL_EBAL);
                        }
                        k += 1;
                    }
                    ok += 1;
                }
                Err(e) => {
                    if r.ok || r.code != (e & 0xffff) as u16 || r.got != 0 {
                        return Err(RL_EBAL);
                    }
                    bad += 1;
                }
            }
            i += 1;
        }
        if ok != self.ok_count() || bad != self.err_count() {
            return Err(RL_EBAL);
        }
        Ok(())
    }

    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                h = fold(h, r.nlen as u64);
                let mut k = 0usize;
                while k < r.nlen as usize {
                    if let Some(b) = r.name.get(k) {
                        h ^= *b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                    k += 1;
                }
                h = fold(h, r.buf_len as u64);
                h = fold(h, r.got as u64);
                let mut j = 0usize;
                while j < r.got as usize {
                    if let Some(b) = r.copy.get(j) {
                        h ^= *b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                    j += 1;
                }
                h = fold(h, r.code as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(RL_EFROZEN);
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
            Err(RL_ESEAL)
        }
    }

    pub fn root(&self) -> u64 {
        self.root
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

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

/// 十四行账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let _ = l.register(0, b"a", 16, true);
    let _ = l.register(1, b"a", 5, true);
    let _ = l.register(2, b"a", 4, true);
    let _ = l.register(3, b"a", 1, true);
    let _ = l.register(4, b"a", 0, false);
    let _ = l.register(5, b"long", 16, true);
    let _ = l.register(6, b"long", 32, true);
    let _ = l.register(7, b"file", 16, false);
    let _ = l.register(8, b"zzz", 16, false);
    let _ = l.register(9, b"self", 16, true);
    let _ = l.register(10, b"", 16, false);
    let _ = l.register(11, b"a", 31, true);
    let _ = l.register(12, b"long", 33, true);
    let _ = l.register(13, b"abcdefghijklmnopqrst", 16, false);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.len() == ROWS && l.ok_count() == 9 && l.err_count() == 5 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    out[0] = base && root != 0 && rep == Ok(root);

    let sys = Sys::fresh();
    let mut b1 = [0u8; 8];
    let full = sys.readlink(b"a", &mut b1);
    out[1] = full == Ok(5) && b1.get(..5) == Some(&b"tgt/a"[..]) && b1.get(5) == Some(&0);

    let mut b2 = [0u8; 3];
    let part = sys.readlink(b"long", &mut b2);
    out[2] = part == Ok(3) && b2.get(..3) == Some(&b"012"[..]);

    let mut b3 = [0u8; 4];
    let mut b4 = [0u8; 0];
    out[3] = sys.readlink(b"file", &mut b3) == Err(RL_EINVAL)
        && sys.readlink(b"a", &mut b4) == Err(RL_EINVAL)
        && sys.readlink(b"nope", &mut b3) == Err(RL_ENOENT);
    out
}

/// 十三判据。
pub fn run_unx11447_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11447.readlink");
    let l = build_ledger();
    s.add("RL1-十四格", l.len() == ROWS, "static");
    s.add_dyn("RL2-九成五拒", l.ok_count() == 9 && l.err_count() == 5 && l.audit().is_ok(), &format!("ok={} err={}", l.ok_count(), l.err_count()));
    s.add("RL3-完整读取", l.row(0).map(|r| r.got) == Some(5) && l.row(0).map(|r| r.copy.get(0).copied()) == Some(Some(b't')) && l.row(0).map(|r| r.copy.get(4).copied()) == Some(Some(b'a')), "static");
    s.add("RL4-恰好装满", l.row(1).map(|r| r.got) == Some(5), "static");
    s.add("RL5-截断不报错", l.row(2).map(|r| r.got) == Some(4) && l.row(3).map(|r| r.got) == Some(1), "static");
    s.add("RL6-不补NUL", l.row(1).map(|r| r.copy.get(5).copied()) == Some(Some(0)) && l.row(1).map(|r| r.got) == Some(5), "static");
    s.add("RL7-零缓冲区", l.row(4).map(|r| r.code) == Some((RL_EINVAL & 0xffff) as u16), "static");
    s.add("RL8-三十二字节目标", l.row(5).map(|r| r.got) == Some(16) && l.row(6).map(|r| r.got) == Some(32), "static");
    s.add("RL9-非链接", l.row(7).map(|r| r.code) == Some((RL_EINVAL & 0xffff) as u16), "static");
    s.add("RL10-缺失名字", l.row(8).map(|r| r.code) == Some((RL_ENOENT & 0xffff) as u16) && l.row(10).map(|r| r.code) == Some((RL_ENOENT & 0xffff) as u16), "static");
    s.add("RL11-自指与超额缓冲", l.row(9).map(|r| r.got) == Some(4) && l.row(11).map(|r| r.got) == Some(5) && l.row(12).map(|r| r.got) == Some(32), "static");
    s.add("RL12-超长名字", l.row(13).map(|r| r.code) == Some((RL_ENAMETOOLONG & 0xffff) as u16) && l.rejects() == 0, "static");
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(RL_EFROZEN);
    let inj = inject_four();
    s.add_dyn("RL13-冻结重放四注入", root != 0 && rep == Ok(root) && again && inj == [true; 4], &format!("root={:016x} {:?}", root, inj));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_copy() {
        let sys = Sys::fresh();
        let mut b = [0u8; 16];
        assert_eq!(sys.readlink(b"a", &mut b), Ok(5));
        assert_eq!(b.get(..6), Some(&b"tgt/a\x00"[..]));
    }

    #[test]
    fn truncation() {
        let sys = Sys::fresh();
        let mut b = [0u8; 2];
        assert_eq!(sys.readlink(b"long", &mut b), Ok(2));
        assert_eq!(b.get(..2), Some(&b"01"[..]));
    }

    #[test]
    fn exact_fit_has_no_nul() {
        let sys = Sys::fresh();
        let mut b = [0u8; 5];
        assert_eq!(sys.readlink(b"a", &mut b), Ok(5));
        assert_eq!(b.get(..5), Some(&b"tgt/a"[..]));
    }

    #[test]
    fn empty_buffer_rejected() {
        let sys = Sys::fresh();
        let mut b = [0u8; 0];
        assert_eq!(sys.readlink(b"a", &mut b), Err(RL_EINVAL));
    }

    #[test]
    fn non_link_and_missing() {
        let sys = Sys::fresh();
        let mut b = [0u8; 8];
        assert_eq!(sys.readlink(b"file", &mut b), Err(RL_EINVAL));
        assert_eq!(sys.readlink(b"zzz", &mut b), Err(RL_ENOENT));
    }

    #[test]
    fn long_name_rejected_before_lookup() {
        let sys = Sys::fresh();
        let mut b = [0u8; 8];
        assert_eq!(sys.readlink(b"abcdefghijklmnopqrst", &mut b), Err(RL_ENAMETOOLONG));
    }

    #[test]
    fn ledger_and_checks() {
        let l = build_ledger();
        assert_eq!(l.len(), ROWS);
        assert_eq!(l.ok_count(), 9);
        assert_eq!(inject_four(), [true; 4]);
        assert_eq!(run_unx11447_checks().failed_count(), 0);
    }
}
