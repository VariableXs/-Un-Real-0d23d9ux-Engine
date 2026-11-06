//! UNX-F11445 打开文件被删除（0x20b90 段）
//!
//! Unix 允许删除一个仍被进程打开的文件。删除只摘掉目录项并把链接数减一，inode
//! 与数据块要等最后一个句柄关闭才真正释放。本模型把这条规则拆成三个计数：目录项
//! 是否存在、链接数、打开句柄数。判定释放的条件只有一个，链接数为零且打开数为零；
//! 链接数为零但句柄尚存的状态被记作“僵尸”，它仍然占用槽位，仍能读写。
//!
//! 操作序列由单字节脚本驱动：打开 a/b/c/d、按名删除、按句柄号关闭。句柄号在关闭
//! 之后不会被复用，因此重复关闭同一个号必然报 EBADF；删除已经删除的名字报 ENOENT；
//! 打开已经摘除目录项的名字同样报 ENOENT，哪怕 inode 还因僵尸状态活着。返回的
//! 三元组是（活 inode 数、打开句柄数、僵尸数），它是脚本执行后的唯一事实。
//!
//! 审计从空白文件系统重放每一行脚本，比较三元组或错误码；封印把脚本、长度与
//! 三元组一起哈希。冻结后登记被拒绝并增加 rejects，重放重建账本比较签名。四路
//! 注入分别覆盖正常删除、删除后仍打开、非法句柄与重复删除，避免模型退化成
//! “删除即释放”的简单实现。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20b90;
/// 编号非法。
pub const UO_EID: u32 = SEG_BASE + 0x01;
/// 顺序错误。
pub const UO_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const UO_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const UO_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测缺失。
pub const UO_ENOBS: u32 = SEG_BASE + 0x05;
/// 结果不符。
pub const UO_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const UO_EBAL: u32 = SEG_BASE + 0x07;
/// 名字不存在。
pub const UO_ENOENT: u32 = SEG_BASE + 0x08;
/// 句柄非法。
pub const UO_EBADF: u32 = SEG_BASE + 0x09;
/// 票据不一致。
pub const UO_ESEAL: u32 = SEG_BASE + 0x0a;

/// 目录项数。
pub const NAMES: usize = 4;
/// 脚本容量。
pub const SCRIPT_CAP: usize = 24;
/// 账本格数。
pub const ROWS: usize = 14;

const OP_OPEN_A: u8 = 1;
const OP_OPEN_D: u8 = 4;
const OP_UNLINK_A: u8 = 0x81;
const OP_UNLINK_D: u8 = 0x84;
const OP_CLOSE0: u8 = 0x40;

/// inode 槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inode {
    pub name: [u8; 8],
    pub nlen: u8,
    pub nlink: u32,
    pub open: u32,
    pub linked: bool,
    pub alive: bool,
}

/// 打开句柄。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handle {
    pub ino: u8,
    pub open: bool,
}

/// 四个名字的文件系统。
pub struct Fs {
    pub inodes: Vec<Inode>,
    pub handles: Vec<Handle>,
}

fn eq(a: &[u8], b: &[u8]) -> bool {
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

impl Fs {
    pub fn fresh() -> Fs {
        let mut inodes = Vec::new();
        let names: [&[u8]; NAMES] = [b"a", b"b", b"c", b"d"];
        let mut i = 0usize;
        while i < NAMES {
            let mut nm = [0u8; 8];
            let nlen = match names.get(i) {
                Some(v) => put(&mut nm, v),
                None => 0,
            };
            inodes.push(Inode { name: nm, nlen, nlink: 1, open: 0, linked: true, alive: true });
            i += 1;
        }
        Fs { inodes, handles: Vec::new() }
    }

    fn find(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.inodes.len() {
            if let Some(n) = self.inodes.get(i) {
                if n.alive && n.linked && eq(n.bytes(), name) {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    fn free_if_done(&mut self, i: usize) {
        let mut done = false;
        if let Some(n) = self.inodes.get(i) {
            done = n.nlink == 0 && n.open == 0;
        }
        if done {
            if let Some(n) = self.inodes.get_mut(i) {
                n.alive = false;
                n.linked = false;
            }
        }
    }

    pub fn open_name(&mut self, name: &[u8]) -> Result<u32, u32> {
        let i = match self.find(name) {
            Some(v) => v,
            None => return Err(UO_ENOENT),
        };
        if let Some(n) = self.inodes.get_mut(i) {
            n.open += 1;
        } else {
            return Err(UO_EBAL);
        }
        self.handles.push(Handle { ino: i as u8, open: true });
        Ok((self.handles.len() - 1) as u32)
    }

    pub fn unlink(&mut self, name: &[u8]) -> Result<(), u32> {
        let i = match self.find(name) {
            Some(v) => v,
            None => return Err(UO_ENOENT),
        };
        if let Some(n) = self.inodes.get_mut(i) {
            n.linked = false;
            n.nlink -= 1;
        }
        self.free_if_done(i);
        Ok(())
    }

    pub fn close(&mut self, fd: usize) -> Result<(), u32> {
        let h = match self.handles.get(fd) {
            Some(v) => *v,
            None => return Err(UO_EBADF),
        };
        if !h.open {
            return Err(UO_EBADF);
        }
        if let Some(x) = self.handles.get_mut(fd) {
            x.open = false;
        }
        let ino = h.ino as usize;
        if let Some(n) = self.inodes.get_mut(ino) {
            if n.open > 0 {
                n.open -= 1;
            }
        }
        self.free_if_done(ino);
        Ok(())
    }

    pub fn alive(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.inodes.len() {
            if let Some(x) = self.inodes.get(i) {
                if x.alive {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn open_handles(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.handles.len() {
            if let Some(h) = self.handles.get(i) {
                if h.open {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn zombies(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.inodes.len() {
            if let Some(x) = self.inodes.get(i) {
                if x.alive && x.nlink == 0 && x.open > 0 {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 执行一段脚本，返回三元组或首个错误。
    pub fn run(&mut self, script: &[u8]) -> Result<(u32, u32, u32), u32> {
        let mut i = 0usize;
        while i < script.len() {
            let op = match script.get(i) {
                Some(v) => *v,
                None => return Err(UO_EBAL),
            };
            match op {
                OP_OPEN_A..=OP_OPEN_D => {
                    let idx = (op - OP_OPEN_A) as usize;
                    let mut nm = [0u8; 8];
                    let mut k = 0usize;
                    let nlen = match self.inodes.get(idx) {
                        Some(v) => v.nlen,
                        None => return Err(UO_EBAL),
                    };
                    while k < nlen as usize {
                        if let Some(v) = self.inodes.get(idx).and_then(|x| x.name.get(k)) {
                            if let Some(d) = nm.get_mut(k) {
                                *d = *v;
                            }
                        }
                        k += 1;
                    }
                    let slice = match nm.get(..nlen as usize) {
                        Some(v) => v,
                        None => return Err(UO_EBAL),
                    };
                    self.open_name(slice)?;
                }
                OP_UNLINK_A..=OP_UNLINK_D => {
                    let idx = (op - OP_UNLINK_A) as usize;
                    let mut nm = [0u8; 8];
                    let nlen = match self.inodes.get(idx) {
                        Some(v) => v.nlen,
                        None => return Err(UO_EBAL),
                    };
                    let mut k = 0usize;
                    while k < nlen as usize {
                        if let Some(v) = self.inodes.get(idx).and_then(|x| x.name.get(k)) {
                            if let Some(d) = nm.get_mut(k) {
                                *d = *v;
                            }
                        }
                        k += 1;
                    }
                    let slice = match nm.get(..nlen as usize) {
                        Some(v) => v,
                        None => return Err(UO_EBAL),
                    };
                    self.unlink(slice)?;
                }
                OP_CLOSE0..=0x4f => {
                    let fd = (op - OP_CLOSE0) as usize;
                    self.close(fd)?;
                }
                _ => return Err(UO_EBAL),
            }
            i += 1;
        }
        Ok((self.alive(), self.open_handles(), self.zombies()))
    }
}

impl Inode {
    pub fn bytes(&self) -> &[u8] {
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

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub script: [u8; SCRIPT_CAP],
    pub len: u8,
    pub alive: u32,
    pub open: u32,
    pub zomb: u32,
    pub code: u16,
    pub ok: bool,
}

impl Row {
    pub fn bytes(&self) -> &[u8] {
        let n = self.len as usize;
        if n > SCRIPT_CAP {
            return &[];
        }
        match self.script.get(..n) {
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

    pub fn register(&mut self, slot: u8, script: &[u8], alive: u32, open: u32, zomb: u32, ok: bool) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(UO_EFROZEN);
        }
        if (slot as usize) >= ROWS {
            self.rejects += 1;
            return Err(UO_EID);
        }
        if (slot as usize) < self.rows.len() {
            self.rejects += 1;
            return Err(UO_EDUP);
        }
        if (slot as usize) != self.rows.len() {
            self.rejects += 1;
            return Err(UO_EORDER);
        }
        let mut fs = Fs::fresh();
        let got = fs.run(script);
        let mut r = Row { script: [0; SCRIPT_CAP], len: 0, alive: 0, open: 0, zomb: 0, code: 0, ok };
        r.len = put(&mut r.script, script);
        match got {
            Ok((a, o, z)) => {
                if !ok || a != alive || o != open || z != zomb {
                    self.rejects += 1;
                    return Err(UO_EMISMATCH);
                }
                r.alive = a;
                r.open = o;
                r.zomb = z;
            }
            Err(e) => {
                if ok {
                    self.rejects += 1;
                    return Err(UO_EMISMATCH);
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
            return Err(UO_EBAL);
        }
        let mut i = 0usize;
        let mut ok = 0usize;
        let mut bad = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(UO_ENOBS),
            };
            if (r.len as usize) > SCRIPT_CAP || r.len as usize != r.bytes().len() {
                return Err(UO_EBAL);
            }
            let mut fs = Fs::fresh();
            match fs.run(r.bytes()) {
                Ok((a, o, z)) => {
                    if !r.ok || a != r.alive || o != r.open || z != r.zomb || r.code != 0 {
                        return Err(UO_EBAL);
                    }
                    ok += 1;
                }
                Err(e) => {
                    if r.ok || r.code != (e & 0xffff) as u16 {
                        return Err(UO_EBAL);
                    }
                    bad += 1;
                }
            }
            i += 1;
        }
        if ok != self.ok_count() || bad != self.err_count() {
            return Err(UO_EBAL);
        }
        Ok(())
    }

    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                h = fold(h, r.len as u64);
                let mut k = 0usize;
                while k < r.len as usize {
                    if let Some(b) = r.script.get(k) {
                        h ^= *b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                    k += 1;
                }
                h = fold(h, r.alive as u64);
                h = fold(h, r.open as u64);
                h = fold(h, r.zomb as u64);
                h = fold(h, r.code as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(UO_EFROZEN);
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
            Err(UO_ESEAL)
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

fn put(dst: &mut [u8], src: &[u8]) -> u8 {
    let mut i = 0usize;
    while i < src.len() && i < dst.len() {
        dst[i] = src[i];
        i += 1;
    }
    i as u8
}

/// 十四行脚本账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let _ = l.register(0, &[1], 4, 1, 0, true);
    let _ = l.register(1, &[0x81], 3, 0, 0, true);
    let _ = l.register(2, &[1, 0x81], 4, 1, 1, true);
    let _ = l.register(3, &[1, 0x81, 0x40], 3, 0, 0, true);
    let _ = l.register(4, &[1, 3, 0x81, 0x41], 4, 1, 1, true);
    let _ = l.register(5, &[1, 0x40, 0x81], 3, 0, 0, true);
    let _ = l.register(6, &[0x81, 1], 0, 0, 0, false);
    let _ = l.register(7, &[0x40], 0, 0, 0, false);
    let _ = l.register(8, &[0x81, 0x81], 0, 0, 0, false);
    let _ = l.register(9, &[2, 2, 0x82, 0x40, 0x41], 3, 0, 0, true);
    let _ = l.register(10, &[1, 0x81, 0x40, 0x40], 0, 0, 0, false);
    let _ = l.register(11, &[3, 0x83, 0x40, 1], 3, 1, 0, true);
    let _ = l.register(12, &[1, 2, 0x81, 0x82], 4, 2, 2, true);
    let _ = l.register(13, &[1, 0x81, 0x40, 0x81], 0, 0, 0, false);
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

    let mut a = Fs::fresh();
    let seq = a.run(&[1, 0x81]);
    out[1] = seq == Ok((4, 1, 1)) && a.zombies() == 1 && a.inodes.get(0).map(|i| i.nlink) == Some(0);

    let mut b = Fs::fresh();
    let opened = b.open_name(b"a");
    let removed = b.unlink(b"a");
    let closed = b.close(0);
    out[2] = opened == Ok(0) && removed.is_ok() && closed.is_ok() && b.run(&[]).is_ok();

    let mut d = Fs::fresh();
    let x = d.run(&[0x81, 0x81]);
    let y = d.run(&[0x40]);
    out[3] = x == Err(UO_ENOENT) && y == Err(UO_EBADF) && d.alive() == 3;
    out
}

/// 十三判据。
pub fn run_unx11445_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11445.unlink_open");
    let l = build_ledger();
    s.add("UO1-十四脚本", l.len() == ROWS, "static");
    s.add_dyn("UO2-九成五拒", l.ok_count() == 9 && l.err_count() == 5 && l.audit().is_ok(), &format!("ok={} err={}", l.ok_count(), l.err_count()));
    s.add("UO3-纯删除", l.row(1).map(|r| (r.alive, r.open)) == Some((3, 0)), "static");
    s.add("UO4-僵尸保留", l.row(2).map(|r| (r.alive, r.zomb)) == Some((4, 1)), "static");
    s.add("UO5-末句柄释放", l.row(3).map(|r| (r.alive, r.zomb)) == Some((3, 0)), "static");
    s.add("UO6-多句柄交错", l.row(4).map(|r| (r.open, r.zomb)) == Some((1, 1)), "static");
    s.add("UO7-先后顺序无关", l.row(5).map(|r| r.alive) == Some(3), "static");
    s.add("UO8-摘除后打开失败", l.row(6).map(|r| r.code) == Some((UO_ENOENT & 0xffff) as u16), "static");
    s.add("UO9-空句柄表", l.row(7).map(|r| r.code) == Some((UO_EBADF & 0xffff) as u16), "static");
    s.add("UO10-重复删除", l.row(8).map(|r| r.code) == Some((UO_ENOENT & 0xffff) as u16) && l.row(13).map(|r| r.code) == Some((UO_ENOENT & 0xffff) as u16), "static");
    s.add("UO11-双开未删", l.row(9).map(|r| (r.alive, r.open)) == Some((3, 0)), "static");
    s.add("UO12-重复关闭", l.row(10).map(|r| r.code) == Some((UO_EBADF & 0xffff) as u16) && l.row(12).map(|r| (r.open, r.zomb)) == Some((2, 2)), "static");
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(UO_EFROZEN);
    let inj = inject_four();
    s.add_dyn("UO13-冻结重放四注入", root != 0 && rep == Ok(root) && again && inj == [true; 4], &format!("root={:016x} {:?}", root, inj));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_without_open_frees() {
        let mut fs = Fs::fresh();
        assert_eq!(fs.run(&[0x81]), Ok((3, 0, 0)));
    }

    #[test]
    fn delete_while_open_keeps_inode() {
        let mut fs = Fs::fresh();
        assert_eq!(fs.run(&[1, 0x81]), Ok((4, 1, 1)));
    }

    #[test]
    fn close_releases_zombie() {
        let mut fs = Fs::fresh();
        assert_eq!(fs.run(&[1, 0x81, 0x40]), Ok((3, 0, 0)));
    }

    #[test]
    fn double_unlink_fails() {
        let mut fs = Fs::fresh();
        assert_eq!(fs.run(&[0x81, 0x81]), Err(UO_ENOENT));
        assert_eq!(fs.alive(), 3);
    }

    #[test]
    fn bad_close_fails() {
        let mut fs = Fs::fresh();
        assert_eq!(fs.run(&[0x40]), Err(UO_EBADF));
        assert_eq!(fs.run(&[1, 0x40, 0x40]), Err(UO_EBADF));
    }

    #[test]
    fn two_handles() {
        let mut fs = Fs::fresh();
        assert_eq!(fs.run(&[2, 2, 0x82, 0x40, 0x41]), Ok((3, 0, 0)));
    }

    #[test]
    fn ledger_and_injection() {
        let l = build_ledger();
        assert_eq!(l.ok_count(), 9);
        assert_eq!(l.err_count(), 5);
        assert!(l.audit().is_ok());
        assert_eq!(inject_four(), [true; 4]);
        assert_eq!(run_unx11445_checks().failed_count(), 0);
    }
}
