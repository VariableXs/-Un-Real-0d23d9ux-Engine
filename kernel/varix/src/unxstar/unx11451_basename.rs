//! UNX-F11451 路径拆分（0x20bf0 段）
//!
//! basename 与 dirname 看起来只是找最后一个斜杠，但 POSIX 对边界的规定相当细：
//! 空串的名字是 "."，目录是 "."；全是斜杠的名字是 "/"，目录仍是 "/"，唯独恰好
//! 两个斜杠的目录部分保留为 "//"；尾随斜杠先被剥掉再找分隔符，因此 "a/b/" 的
//! 名字是 "b" 而不是空串；名字里没有斜杠时目录是 "."。
//!
//! 本模型把两个函数实现为对同一输入切片的两次独立扫描，返回的切片要么指向输入，
//! 要么指向静态的 "." 与 "/"。三个以上连续斜杠在目录部分被折叠成一个；前缀里的
//! 双斜杠只在“它本身就是全部前缀”时保留。输入超过容量返回 ENAMETOOLONG，这是
//! 唯一会失败的路径，空串与纯斜杠都合法。
//!
//! 账本每一行同时保存名字与目录两个期望值，审计重新拆分并逐字节比较。封印把输入
//! 与两个结果一起折叠，冻结后拒绝登记，重放重建账本比较签名。四路注入覆盖空串、
//! 根、尾随斜杠与双斜杠特例，确保实现没有把“找最后一个斜杠”当成全部规则。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20bf0;
/// 编号非法。
pub const BN_EID: u32 = SEG_BASE + 0x01;
/// 顺序错误。
pub const BN_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const BN_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const BN_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测缺失。
pub const BN_ENOBS: u32 = SEG_BASE + 0x05;
/// 结果不符。
pub const BN_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const BN_EBAL: u32 = SEG_BASE + 0x07;
/// 路径过长。
pub const BN_ENAMETOOLONG: u32 = SEG_BASE + 0x08;
/// 票据不一致。
pub const BN_ESEAL: u32 = SEG_BASE + 0x09;

/// 路径容量。
pub const PATH_CAP: usize = 64;
/// 账本格数。
pub const ROWS: usize = 15;

fn put(dst: &mut [u8], src: &[u8]) -> u8 {
    let mut i = 0usize;
    while i < src.len() && i < dst.len() {
        dst[i] = src[i];
        i += 1;
    }
    i as u8
}

/// 名字部分。
pub fn basename(p: &[u8]) -> &[u8] {
    if p.is_empty() {
        return b".";
    }
    let mut e = p.len();
    while e > 0 && p.get(e - 1) == Some(&b'/') {
        e -= 1;
    }
    if e == 0 {
        return b"/";
    }
    let mut i = e;
    while i > 0 {
        if p.get(i - 1) == Some(&b'/') {
            return match p.get(i..e) {
                Some(v) => v,
                None => b"",
            };
        }
        i -= 1;
    }
    match p.get(..e) {
        Some(v) => v,
        None => b"",
    }
}

fn leading(p: &[u8]) -> usize {
    let mut n = 0usize;
    while p.get(n) == Some(&b'/') {
        n += 1;
    }
    n
}

/// 目录部分。
pub fn dirname(p: &[u8]) -> &[u8] {
    if p.is_empty() {
        return b".";
    }
    let lead = leading(p);
    let mut e = p.len();
    while e > 0 && p.get(e - 1) == Some(&b'/') {
        e -= 1;
    }
    if e == 0 {
        if lead == 2 {
            return b"//";
        }
        return b"/";
    }
    let mut cut = 0usize;
    let mut i = e;
    while i > 0 {
        if p.get(i - 1) == Some(&b'/') {
            cut = i - 1;
            break;
        }
        i -= 1;
    }
    if i == 0 {
        return b".";
    }
    let mut q = cut;
    while q > 0 && p.get(q - 1) == Some(&b'/') {
        q -= 1;
    }
    if q == 0 {
        if lead == 2 {
            return b"//";
        }
        return b"/";
    }
    match p.get(..q) {
        Some(v) => v,
        None => b"",
    }
}

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub path: [u8; PATH_CAP],
    pub plen: u8,
    pub base: [u8; PATH_CAP],
    pub blen: u8,
    pub dir: [u8; PATH_CAP],
    pub dlen: u8,
    pub code: u16,
    pub ok: bool,
}

impl Row {
    pub fn path_bytes(&self) -> &[u8] {
        let n = self.plen as usize;
        if n > PATH_CAP {
            return &[];
        }
        match self.path.get(..n) {
            Some(v) => v,
            None => &[],
        }
    }

    pub fn base_bytes(&self) -> &[u8] {
        let n = self.blen as usize;
        if n > PATH_CAP {
            return &[];
        }
        match self.base.get(..n) {
            Some(v) => v,
            None => &[],
        }
    }

    pub fn dir_bytes(&self) -> &[u8] {
        let n = self.dlen as usize;
        if n > PATH_CAP {
            return &[];
        }
        match self.dir.get(..n) {
            Some(v) => v,
            None => &[],
        }
    }
}

/// 账本。
pub struct Ledger {
    rows: Vec<Row>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger { rows: Vec::new(), frozen: false, root: 0, rejects: 0 }
    }

    pub fn register(&mut self, slot: u8, path: &[u8], wb: &[u8], wd: &[u8], ok: bool) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(BN_EFROZEN);
        }
        if (slot as usize) >= ROWS {
            self.rejects += 1;
            return Err(BN_EID);
        }
        if (slot as usize) < self.rows.len() {
            self.rejects += 1;
            return Err(BN_EDUP);
        }
        if (slot as usize) != self.rows.len() {
            self.rejects += 1;
            return Err(BN_EORDER);
        }
        if wb.len() > PATH_CAP || wd.len() > PATH_CAP {
            self.rejects += 1;
            return Err(BN_EBAL);
        }
        let mut r = Row { path: [0; PATH_CAP], plen: 0, base: [0; PATH_CAP], blen: 0, dir: [0; PATH_CAP], dlen: 0, code: 0, ok };
        r.plen = put(&mut r.path, path);
        if path.len() >= PATH_CAP {
            // 超容量路径只有失败一种结局。
            if ok {
                self.rejects += 1;
                return Err(BN_EMISMATCH);
            }
            r.code = (BN_ENAMETOOLONG & 0xffff) as u16;
            self.rows.push(r);
            return Ok(());
        }
        let b = basename(path);
        let d = dirname(path);
        if !ok || b != wb || d != wd {
            self.rejects += 1;
            return Err(BN_EMISMATCH);
        }
        r.blen = put(&mut r.base, b);
        r.dlen = put(&mut r.dir, d);
        self.rows.push(r);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn row(&self, i: usize) -> Option<&Row> {
        self.rows.get(i)
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
            return Err(BN_EBAL);
        }
        let mut i = 0usize;
        let mut ok = 0usize;
        let mut bad = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(BN_ENOBS),
            };
            if r.plen as usize > PATH_CAP {
                return Err(BN_EBAL);
            }
            if r.plen as usize >= PATH_CAP {
                if r.ok || r.code != (BN_ENAMETOOLONG & 0xffff) as u16 {
                    return Err(BN_EBAL);
                }
                bad += 1;
            } else {
                let b = basename(r.path_bytes());
                let d = dirname(r.path_bytes());
                if !r.ok || b != r.base_bytes() || d != r.dir_bytes() || r.code != 0 {
                    return Err(BN_EBAL);
                }
                ok += 1;
            }
            i += 1;
        }
        if ok != self.ok_count() || bad != self.err_count() {
            return Err(BN_EBAL);
        }
        Ok(())
    }

    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                h = fold(h, r.plen as u64);
                let mut k = 0usize;
                while k < r.plen as usize {
                    if let Some(b) = r.path.get(k) {
                        h ^= *b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                    k += 1;
                }
                h = fold(h, r.blen as u64);
                let mut j = 0usize;
                while j < r.blen as usize {
                    if let Some(b) = r.base.get(j) {
                        h ^= *b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                    j += 1;
                }
                h = fold(h, r.dlen as u64);
                let mut q = 0usize;
                while q < r.dlen as usize {
                    if let Some(b) = r.dir.get(q) {
                        h ^= *b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                    q += 1;
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
            return Err(BN_EFROZEN);
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
            Err(BN_ESEAL)
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

/// 十五行账本。
pub fn build_ledger() -> Ledger {
    let long = b"0123456789012345678901234567890123456789012345678901234567890123";
    let mut l = Ledger::new();
    let _ = l.register(0, b"", b".", b".", true);
    let _ = l.register(1, b"/", b"/", b"/", true);
    let _ = l.register(2, b"//", b"/", b"//", true);
    let _ = l.register(3, b"///", b"/", b"/", true);
    let _ = l.register(4, b"a", b"a", b".", true);
    let _ = l.register(5, b"a/b", b"b", b"a", true);
    let _ = l.register(6, b"a/b/", b"b", b"a", true);
    let _ = l.register(7, b"/a", b"a", b"/", true);
    let _ = l.register(8, b"a//b", b"b", b"a", true);
    let _ = l.register(9, b"///a", b"a", b"/", true);
    let _ = l.register(10, b"/a/b//", b"b", b"/a", true);
    let _ = l.register(11, b"a/..", b"..", b"a", true);
    let _ = l.register(12, b".", b".", b".", true);
    let _ = l.register(13, b"..", b"..", b".", true);
    let _ = l.register(14, long, b"", b"", false);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.len() == ROWS && l.ok_count() == 14 && l.err_count() == 1 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    out[0] = base && root != 0 && rep == Ok(root);

    out[1] = basename(b"a/b/") == b"b" && dirname(b"a/b/") == b"a";

    out[2] = basename(b"///") == b"/" && dirname(b"///") == b"/"
        && basename(b"//") == b"/" && dirname(b"//") == b"//";

    out[3] = basename(b"") == b"." && dirname(b"") == b"."
        && basename(b"..") == b".." && dirname(b"..") == b"."
        && basename(b"/a") == b"a" && dirname(b"/a") == b"/";
    out
}

/// 十三判据。
pub fn run_unx11451_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11451.basename");
    let l = build_ledger();
    s.add_dyn("BN1-十四成一拒", l.len() == ROWS && l.ok_count() == 14 && l.err_count() == 1 && l.audit().is_ok(), &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()));
    s.add("BN2-空串", l.row(0).map(|r| (r.blen, r.dlen)) == Some((1, 1)), "static");
    s.add("BN3-单斜杠", l.row(1).map(|r| (r.blen, r.dlen)) == Some((1, 1)), "static");
    s.add("BN4-双斜杠目录", l.row(2).map(|r| r.dir_bytes()) == Some(&b"//"[..]) && l.row(2).map(|r| r.base_bytes()) == Some(&b"/"[..]), "static");
    s.add("BN5-三斜杠折叠", l.row(3).map(|r| r.dir_bytes()) == Some(&b"/"[..]), "static");
    s.add("BN6-单名", l.row(4).map(|r| r.dir_bytes()) == Some(&b"."[..]), "static");
    s.add("BN7-两级", l.row(5).map(|r| r.base_bytes()) == Some(&b"b"[..]) && l.row(5).map(|r| r.dir_bytes()) == Some(&b"a"[..]), "static");
    s.add("BN8-尾随斜杠", l.row(6).map(|r| r.base_bytes()) == Some(&b"b"[..]) && l.row(10).map(|r| r.dir_bytes()) == Some(&b"/a"[..]), "static");
    s.add("BN9-绝对路径", l.row(7).map(|r| r.dir_bytes()) == Some(&b"/"[..]), "static");
    s.add("BN10-内部重复", l.row(8).map(|r| r.dir_bytes()) == Some(&b"a"[..]) && l.row(9).map(|r| r.dir_bytes()) == Some(&b"/"[..]), "static");
    s.add("BN11-点与双点", l.row(11).map(|r| (r.base_bytes(), r.dir_bytes())) == Some((&b".."[..], &b"a"[..])) && l.row(12).map(|r| (r.dlen, r.blen)) == Some((1, 1)) && l.row(13).map(|r| r.dir_bytes()) == Some(&b"."[..]), "static");
    s.add("BN12-超长拒绝", l.row(14).map(|r| r.code) == Some((BN_ENAMETOOLONG & 0xffff) as u16) && l.rejects() == 0, "static");
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(BN_EFROZEN);
    let inj = inject_four();
    s.add_dyn("BN13-冻结重放四注入", root != 0 && rep == Ok(root) && again && inj == [true; 4], &format!("root={:016x} {:?}", root, inj));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_root() {
        assert_eq!(basename(b""), b".");
        assert_eq!(dirname(b""), b".");
        assert_eq!(basename(b"/"), b"/");
        assert_eq!(dirname(b"/"), b"/");
    }

    #[test]
    fn double_slash_kept() {
        assert_eq!(dirname(b"//"), b"//");
        assert_eq!(dirname(b"//a"), b"//");
        assert_eq!(dirname(b"///a"), b"/");
    }

    #[test]
    fn trailing_slashes_stripped() {
        assert_eq!(basename(b"a/b/"), b"b");
        assert_eq!(dirname(b"a/b/"), b"a");
        assert_eq!(basename(b"///"), b"/");
    }

    #[test]
    fn no_separator() {
        assert_eq!(basename(b"abc"), b"abc");
        assert_eq!(dirname(b"abc"), b".");
    }

    #[test]
    fn nested() {
        assert_eq!(basename(b"/a/b//"), b"b");
        assert_eq!(dirname(b"/a/b//"), b"/a");
        assert_eq!(dirname(b"a//b"), b"a");
    }

    #[test]
    fn dots() {
        assert_eq!(basename(b"."), b".");
        assert_eq!(dirname(b".."), b".");
        assert_eq!(dirname(b"a/.."), b"a");
    }

    #[test]
    fn ledger_and_checks() {
        let l = build_ledger();
        assert_eq!(l.len(), ROWS);
        assert_eq!(l.ok_count(), 14);
        assert_eq!(inject_four(), [true; 4]);
        assert_eq!(run_unx11451_checks().failed_count(), 0);
    }
}
