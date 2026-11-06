//! UNX-F11449 磁盘用量统计（0x20bd0 段）
//!
//! du 报告的是“实际占用”而不是表观大小：目录项也占块，稀疏文件按块数折算，
//! 符号链接只算自身文本。更关键的是硬链接去重——同一个 (设备号, inode 号) 在
//! 一次统计里只能出现一次，否则同一份数据会被链接名重复计费。模型先按目录树
//! 递归收集条目，用一个已见集合挡住重复 inode，再把块数乘 512 得到磁盘字节。
//!
//! df 用另一个视角回答同一组块：总量、空闲、已用与百分比。总量为零是 EINVAL，
//! 空闲大于总量也是 EINVAL。百分比用整数除法向下取整，空闲为零时正好是一百，
//! 但总量与空闲相等时是零而不是一。du 的根可以给多个，计数跨根共享去重集合，
//! 因此“先统计 /a 再统计 /b”与“一次统计两个根”对硬链接的结论一致。
//!
//! 审计从固定目录树重建统计，比较表观字节、磁盘字节与条目数；df 行比较已用、
//! 可用与百分比。封印把根、容量与结果一起折叠，冻结后拒绝登记，重放重建账本
//! 比较签名。四路注入分别验证整树统计、硬链接去重、稀疏文件与 df 空盘，防止
//! 把磁盘占用误写成表观大小或忘记去重。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20bd0;
/// 编号非法。
pub const DF_EID: u32 = SEG_BASE + 0x01;
/// 顺序错误。
pub const DF_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const DF_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const DF_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测缺失。
pub const DF_ENOBS: u32 = SEG_BASE + 0x05;
/// 结果不符。
pub const DF_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const DF_EBAL: u32 = SEG_BASE + 0x07;
/// 根不存在。
pub const DF_ENOENT: u32 = SEG_BASE + 0x08;
/// 容量参数非法。
pub const DF_EINVAL: u32 = SEG_BASE + 0x09;
/// 票据不一致。
pub const DF_ESEAL: u32 = SEG_BASE + 0x0a;

/// 统计整树。
pub const Q_DU: u8 = 1;
/// 统计文件系统。
pub const Q_DF: u8 = 2;
/// 无第二根。
pub const NO_ROOT: u16 = 0xffff;
/// 账本格数。
pub const ROWS: usize = 14;

/// 条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub dev: u8,
    pub ino: u16,
    pub nlink: u8,
    pub parent: i16,
    pub size: u32,
    pub blocks: u32,
    pub dir: bool,
    pub name: [u8; 8],
    pub nlen: u8,
}

impl Entry {
    pub fn name_bytes(&self) -> &[u8] {
        let n = self.nlen as usize;
        if n > 8 {
            return &[];
        }
        match self.name.get(..n) {
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

fn seen_has(seen: &Vec<(u8, u16)>, k: (u8, u16)) -> bool {
    let mut i = 0usize;
    while i < seen.len() {
        if seen.get(i) == Some(&k) {
            return true;
        }
        i += 1;
    }
    false
}

/// 固定目录树。
pub fn entries() -> Vec<Entry> {
    let mut v: Vec<Entry> = Vec::new();
    let rows: [(u8, u16, u8, i16, u32, u32, bool, &[u8]); 11] = [
        (1, 1, 3, -1, 4096, 8, true, b"/"),
        (1, 2, 3, 0, 4096, 8, true, b"a"),
        (1, 3, 1, 1, 10000, 20, false, b"f1"),
        (1, 4, 2, 1, 5000, 0, false, b"f2"),
        (1, 5, 3, 0, 4096, 8, true, b"b"),
        (1, 4, 2, 4, 5000, 0, false, b"f1"),
        (1, 6, 1, 4, 300, 1, false, b"f3"),
        (1, 7, 1, 0, 7, 0, false, b"ln"),
        (1, 8, 3, 0, 4096, 8, true, b"c"),
        (1, 3, 1, 8, 10000, 20, false, b"f4"),
        (1, 9, 1, 8, 0, 0, false, b"f5"),
    ];
    let mut i = 0usize;
    while i < rows.len() {
        if let Some(r) = rows.get(i) {
            let mut name = [0u8; 8];
            let nlen = put(&mut name, r.7);
            v.push(Entry { dev: r.0, ino: r.1, nlink: r.2, parent: r.3, size: r.4, blocks: r.5, dir: r.6, name, nlen });
        }
        i += 1;
    }
    v
}

/// 统计器。
pub struct Meter {
    pub es: Vec<Entry>,
    pub seen: Vec<(u8, u16)>,
    pub apparent: u64,
    pub disk: u64,
    pub counted: u32,
}

impl Meter {
    pub fn new() -> Meter {
        Meter { es: entries(), seen: Vec::new(), apparent: 0, disk: 0, counted: 0 }
    }

    fn visit(&mut self, i: usize) -> Result<(), u32> {
        let e = match self.es.get(i) {
            Some(v) => *v,
            None => return Err(DF_ENOENT),
        };
        let key = (e.dev, e.ino);
        if !seen_has(&self.seen, key) {
            self.seen.push(key);
            self.apparent += e.size as u64;
            self.disk += (e.blocks as u64) * 512;
            self.counted += 1;
        }
        let mut k = 0usize;
        while k < self.es.len() {
            let is_child = match self.es.get(k) {
                Some(c) => c.parent >= 0 && c.parent as usize == i,
                None => false,
            };
            if is_child {
                self.visit(k)?;
            }
            k += 1;
        }
        Ok(())
    }

    pub fn du(&mut self, roots: &[u16]) -> Result<(u32, u64, u32), u32> {
        if roots.is_empty() {
            return Err(DF_EINVAL);
        }
        let mut i = 0usize;
        while i < roots.len() {
            let r = match roots.get(i) {
                Some(v) => *v,
                None => return Err(DF_EINVAL),
            };
            if (r as usize) >= self.es.len() {
                return Err(DF_ENOENT);
            }
            self.visit(r as usize)?;
            i += 1;
        }
        Ok((self.apparent as u32, self.disk, self.counted))
    }

    pub fn df(&self, total: u32, free: u32) -> Result<(u32, u32, u32), u32> {
        if total == 0 || free > total {
            return Err(DF_EINVAL);
        }
        let used = total - free;
        let pct = (used as u64 * 100 / total as u64) as u32;
        Ok((used, free, pct))
    }
}

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub kind: u8,
    pub root0: u16,
    pub root1: u16,
    pub total: u32,
    pub free: u32,
    pub a: u32,
    pub b: u32,
    pub c: u32,
    pub code: u16,
    pub ok: bool,
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

    pub fn register(&mut self, slot: u8, kind: u8, r0: u16, r1: u16, total: u32, free: u32, a: u32, b: u32, c: u32, ok: bool) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(DF_EFROZEN);
        }
        if (slot as usize) >= ROWS {
            self.rejects += 1;
            return Err(DF_EID);
        }
        if (slot as usize) < self.rows.len() {
            self.rejects += 1;
            return Err(DF_EDUP);
        }
        if (slot as usize) != self.rows.len() {
            self.rejects += 1;
            return Err(DF_EORDER);
        }
        let mut roots: Vec<u16> = Vec::new();
        if kind == Q_DU {
            roots.push(r0);
            if r1 != NO_ROOT {
                roots.push(r1);
            }
        }
        let got: Result<(u32, u32, u32), u32> = if kind == Q_DU {
            let mut m = Meter::new();
            match m.du(&roots) {
                Ok((ap, disk, cnt)) => Ok((ap, disk as u32, cnt)),
                Err(e) => Err(e),
            }
        } else if kind == Q_DF {
            let m = Meter::new();
            m.df(total, free)
        } else {
            Err(DF_EBAL)
        };
        let mut r = Row { kind, root0: r0, root1: r1, total, free, a: 0, b: 0, c: 0, code: 0, ok };
        match got {
            Ok((x, y, z)) => {
                if !ok || x != a || y != b || z != c {
                    self.rejects += 1;
                    return Err(DF_EMISMATCH);
                }
                r.a = x;
                r.b = y;
                r.c = z;
            }
            Err(e) => {
                if ok {
                    self.rejects += 1;
                    return Err(DF_EMISMATCH);
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

    fn eval(r: Row) -> Result<(u32, u32, u32), u32> {
        if r.kind == Q_DU {
            let mut roots: Vec<u16> = Vec::new();
            roots.push(r.root0);
            if r.root1 != NO_ROOT {
                roots.push(r.root1);
            }
            let mut m = Meter::new();
            match m.du(&roots) {
                Ok((ap, disk, cnt)) => Ok((ap, disk as u32, cnt)),
                Err(e) => Err(e),
            }
        } else if r.kind == Q_DF {
            Meter::new().df(r.total, r.free)
        } else {
            Err(DF_EBAL)
        }
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.rows.is_empty() {
            return Err(DF_EBAL);
        }
        let mut i = 0usize;
        let mut ok = 0usize;
        let mut bad = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(DF_ENOBS),
            };
            match Ledger::eval(r) {
                Ok((x, y, z)) => {
                    if !r.ok || x != r.a || y != r.b || z != r.c || r.code != 0 {
                        return Err(DF_EBAL);
                    }
                    ok += 1;
                }
                Err(e) => {
                    if r.ok || r.code != (e & 0xffff) as u16 {
                        return Err(DF_EBAL);
                    }
                    bad += 1;
                }
            }
            i += 1;
        }
        if ok != self.ok_count() || bad != self.err_count() {
            return Err(DF_EBAL);
        }
        Ok(())
    }

    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                h = fold(h, r.kind as u64);
                h = fold(h, r.root0 as u64);
                h = fold(h, r.root1 as u64);
                h = fold(h, r.total as u64);
                h = fold(h, r.free as u64);
                h = fold(h, r.a as u64);
                h = fold(h, r.b as u64);
                h = fold(h, r.c as u64);
                h = fold(h, r.code as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(DF_EFROZEN);
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
            Err(DF_ESEAL)
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
    let _ = l.register(0, Q_DU, 0, NO_ROOT, 0, 0, 31691, 27136, 9, true);
    let _ = l.register(1, Q_DU, 1, NO_ROOT, 0, 0, 19096, 14336, 3, true);
    let _ = l.register(2, Q_DU, 2, NO_ROOT, 0, 0, 10000, 10240, 1, true);
    let _ = l.register(3, Q_DU, 8, NO_ROOT, 0, 0, 14096, 14336, 3, true);
    let _ = l.register(4, Q_DU, 4, NO_ROOT, 0, 0, 9396, 4608, 3, true);
    let _ = l.register(5, Q_DU, 1, 4, 0, 0, 23492, 18944, 5, true);
    let _ = l.register(6, Q_DF, 0, NO_ROOT, 10000, 2500, 7500, 2500, 75, true);
    let _ = l.register(7, Q_DF, 0, NO_ROOT, 10000, 0, 10000, 0, 100, true);
    let _ = l.register(8, Q_DF, 0, NO_ROOT, 0, 0, 0, 0, 0, false);
    let _ = l.register(9, Q_DU, 99, NO_ROOT, 0, 0, 0, 0, 0, false);
    let _ = l.register(10, Q_DU, 10, NO_ROOT, 0, 0, 0, 0, 1, true);
    let _ = l.register(11, Q_DF, 0, NO_ROOT, 3, 3, 0, 3, 0, true);
    let _ = l.register(12, Q_DF, 0, NO_ROOT, 1, 1, 0, 1, 0, true);
    let _ = l.register(13, Q_DU, 7, NO_ROOT, 0, 0, 7, 0, 1, true);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.len() == ROWS && l.ok_count() == 12 && l.err_count() == 2 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    out[0] = base && root != 0 && rep == Ok(root);

    let mut m = Meter::new();
    out[1] = m.du(&[0]) == Ok((31691, 27136, 9)) && m.seen.len() == 9;

    let mut h = Meter::new();
    let both = h.du(&[1, 4]);
    let mut s = Meter::new();
    let single = s.du(&[1]);
    out[2] = both == Ok((23492, 18944, 5)) && single == Ok((19096, 14336, 3));

    let mut z = Meter::new();
    let sparse = z.du(&[2]);
    let mut d = Meter::new();
    out[3] = sparse == Ok((10000, 10240, 1))
        && d.du(&[7]) == Ok((7, 0, 1))
        && d.df(10000, 2500) == Ok((7500, 2500, 75))
        && d.df(0, 0) == Err(DF_EINVAL);
    out
}

/// 十三判据。
pub fn run_unx11449_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11449.df_du");
    let l = build_ledger();
    s.add("DF1-十四格", l.len() == ROWS, "static");
    s.add_dyn("DF2-十二成二拒", l.ok_count() == 12 && l.err_count() == 2 && l.audit().is_ok(), &format!("ok={} err={}", l.ok_count(), l.err_count()));
    s.add("DF3-整树表观", l.row(0).map(|r| r.a) == Some(31691), "static");
    s.add("DF4-整树磁盘", l.row(0).map(|r| r.b) == Some(27136) && l.row(0).map(|r| r.c) == Some(9), "static");
    s.add("DF5-子树", l.row(1).map(|r| (r.a, r.b, r.c)) == Some((19096, 14336, 3)), "static");
    s.add("DF6-单文件", l.row(2).map(|r| (r.a, r.b)) == Some((10000, 10240)), "static");
    s.add("DF7-硬链接去重", l.row(3).map(|r| (r.a, r.c)) == Some((14096, 3)) && l.row(5).map(|r| r.a) == Some(23492), "static");
    s.add("DF8-稀疏目录", l.row(4).map(|r| (r.a, r.b)) == Some((9396, 4608)), "static");
    s.add("DF9-磁盘容量", l.row(6).map(|r| (r.a, r.b, r.c)) == Some((7500, 2500, 75)) && l.row(7).map(|r| r.c) == Some(100), "static");
    s.add("DF10-零容量拒绝", l.row(8).map(|r| r.code) == Some((DF_EINVAL & 0xffff) as u16) && l.row(9).map(|r| r.code) == Some((DF_ENOENT & 0xffff) as u16), "static");
    s.add("DF11-空文件与链接", l.row(10).map(|r| (r.a, r.b, r.c)) == Some((0, 0, 1)) && l.row(13).map(|r| (r.a, r.b)) == Some((7, 0)), "static");
    s.add("DF12-满盘与全空", l.row(11).map(|r| r.c) == Some(0) && l.row(12).map(|r| (r.a, r.b)) == Some((0, 1)) && l.rejects() == 0, "static");
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(DF_EFROZEN);
    let inj = inject_four();
    s.add_dyn("DF13-冻结重放四注入", root != 0 && rep == Ok(root) && again && inj == [true; 4], &format!("root={:016x} {:?}", root, inj));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_tree() {
        let mut m = Meter::new();
        assert_eq!(m.du(&[0]), Ok((31691, 27136, 9)));
    }

    #[test]
    fn hardlink_once() {
        let mut m = Meter::new();
        assert_eq!(m.du(&[0]).map(|x| x.0), Ok(31691));
        assert_eq!(m.seen.len(), 9);
    }

    #[test]
    fn sparse_file() {
        let mut m = Meter::new();
        assert_eq!(m.du(&[2]), Ok((10000, 10240, 1)));
    }

    #[test]
    fn multi_root_shares_seen() {
        let mut m = Meter::new();
        assert_eq!(m.du(&[1, 4]), Ok((23492, 18944, 5)));
    }

    #[test]
    fn df_percent() {
        let m = Meter::new();
        assert_eq!(m.df(10000, 2500), Ok((7500, 2500, 75)));
        assert_eq!(m.df(10, 0), Ok((10, 0, 100)));
        assert_eq!(m.df(0, 0), Err(DF_EINVAL));
        assert_eq!(m.df(10, 11), Err(DF_EINVAL));
    }

    #[test]
    fn missing_root() {
        let mut m = Meter::new();
        assert_eq!(m.du(&[99]), Err(DF_ENOENT));
        assert_eq!(m.du(&[]), Err(DF_EINVAL));
    }

    #[test]
    fn ledger_and_checks() {
        let l = build_ledger();
        assert_eq!(l.len(), ROWS);
        assert_eq!(l.ok_count(), 12);
        assert_eq!(inject_four(), [true; 4]);
        assert_eq!(run_unx11449_checks().failed_count(), 0);
    }
}
