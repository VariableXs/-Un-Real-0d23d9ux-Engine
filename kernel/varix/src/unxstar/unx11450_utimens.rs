//! UNX-F11450 更新时间戳（0x20be0 段）
//!
//! touch 与 utimensat 共用一套时间语义：每个时间参数有三种形态——给定具体值、
//! 取当前时钟（UTIME_NOW）、保持原值（UTIME_OMIT）。两个参数都省略时什么也不
//! 做，因此不需要任何权限；只要有一项要变，就必须满足权限规则。具体值只有属主
//! 或具备管理能力的主体才能写；取当前时间则宽松一些，属主或对文件有写权限即可。
//!
//! 文件不存在时，带创建标志的调用会先造一个普通文件：权限取 0666 再按 umask
//! 去掉写位，属主是调用者，两个时间都从当前时钟开始，然后再走一次时间设置。
//! 不带创建标志则返回 ENOENT。只读挂载在任何实际改动之前返回 EROFS，哪怕调用者
//! 是 root；这条判断与权限判断的先后顺序被写进账本，防止错误码漂移。
//!
//! 每一行固定初始 inode、调用者、两个时间参数、创建与只读标志，以及期望的
//! atime、mtime 和是否新建。审计从行内字段重建 inode 重放判定。封印把输入与
//! 结果一起折叠，冻结后拒绝登记，重放重建账本比较签名。四路注入覆盖显式值、
//! 省略保持、非属主被拒与创建路径。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20be0;
/// 编号非法。
pub const TM_EID: u32 = SEG_BASE + 0x01;
/// 顺序错误。
pub const TM_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const TM_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const TM_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测缺失。
pub const TM_ENOBS: u32 = SEG_BASE + 0x05;
/// 结果不符。
pub const TM_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const TM_EBAL: u32 = SEG_BASE + 0x07;
/// 权限不足。
pub const TM_EACCES: u32 = SEG_BASE + 0x08;
/// 只读挂载。
pub const TM_EROFS: u32 = SEG_BASE + 0x09;
/// 文件不存在。
pub const TM_ENOENT: u32 = SEG_BASE + 0x0a;
/// 票据不一致。
pub const TM_ESEAL: u32 = SEG_BASE + 0x0b;

/// 取当前时间。
pub const T_NOW: u8 = 1;
/// 保持原值。
pub const T_OMIT: u8 = 2;
/// 当前时钟读数。
pub const CLOCK: u32 = 1000;
/// 新建文件基线权限。
pub const BASE_MODE: u16 = 0o666;
/// 进程 umask。
pub const UMASK: u16 = 0o022;
/// 账本格数。
pub const ROWS: usize = 14;

/// inode。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inode {
    pub uid: u16,
    pub mode: u16,
    pub atime: u32,
    pub mtime: u32,
}

/// 调用者。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cred {
    pub uid: u16,
    pub cap: bool,
}

/// 时间参数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spec {
    pub kind: u8,
    pub val: u32,
}

/// 结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Done {
    pub atime: u32,
    pub mtime: u32,
    pub created: bool,
}

fn resolve(s: Spec, old: u32) -> u32 {
    if s.kind == T_NOW {
        CLOCK
    } else if s.kind == T_OMIT {
        old
    } else {
        s.val
    }
}

/// 设置时间。
pub fn utimens(ino: Option<Inode>, cred: Cred, at: Spec, mt: Spec, create: bool, ro: bool) -> Result<(Inode, Done), u32> {
    let mut node = match ino {
        Some(v) => v,
        None => {
            if !create {
                return Err(TM_ENOENT);
            }
            Inode { uid: cred.uid, mode: BASE_MODE & !UMASK, atime: CLOCK, mtime: CLOCK }
        }
    };
    let created = ino.is_none();
    let changes = at.kind != T_OMIT || mt.kind != T_OMIT;
    if !changes {
        return Ok((node, Done { atime: node.atime, mtime: node.mtime, created }));
    }
    if ro {
        return Err(TM_EROFS);
    }
    let owner = cred.uid == node.uid;
    if !owner && !cred.cap {
        let explicit = at.kind == 0 || mt.kind == 0;
        let writable = if owner { node.mode & 0o200 != 0 } else { node.mode & 0o002 != 0 };
        if explicit || !writable {
            return Err(TM_EACCES);
        }
    }
    node.atime = resolve(at, node.atime);
    node.mtime = resolve(mt, node.mtime);
    Ok((node, Done { atime: node.atime, mtime: node.mtime, created }))
}

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub have: bool,
    pub ino: Inode,
    pub cred: Cred,
    pub at: Spec,
    pub mt: Spec,
    pub create: bool,
    pub ro: bool,
    pub w_at: u32,
    pub w_mt: u32,
    pub created: bool,
    pub code: u16,
    pub ok: bool,
}

fn eval(r: Row) -> Result<(u32, u32, bool), u32> {
    let ino = if r.have { Some(r.ino) } else { None };
    match utimens(ino, r.cred, r.at, r.mt, r.create, r.ro) {
        Ok((n, d)) => Ok((n.atime, n.mtime, d.created)),
        Err(e) => Err(e),
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

    pub fn register(&mut self, slot: u8, have: bool, ino: Inode, cred: Cred, at: Spec, mt: Spec, create: bool, ro: bool, w_at: u32, w_mt: u32, created: bool, ok: bool) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(TM_EFROZEN);
        }
        if (slot as usize) >= ROWS {
            self.rejects += 1;
            return Err(TM_EID);
        }
        if (slot as usize) < self.rows.len() {
            self.rejects += 1;
            return Err(TM_EDUP);
        }
        if (slot as usize) != self.rows.len() {
            self.rejects += 1;
            return Err(TM_EORDER);
        }
        let mut r = Row { have, ino, cred, at, mt, create, ro, w_at, w_mt, created, code: 0, ok };
        match eval(r) {
            Ok((a, m, c)) => {
                if !ok || a != w_at || m != w_mt || c != created {
                    self.rejects += 1;
                    return Err(TM_EMISMATCH);
                }
            }
            Err(e) => {
                if ok {
                    self.rejects += 1;
                    return Err(TM_EMISMATCH);
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
            return Err(TM_EBAL);
        }
        let mut i = 0usize;
        let mut ok = 0usize;
        let mut bad = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(TM_ENOBS),
            };
            match eval(r) {
                Ok((a, m, c)) => {
                    if !r.ok || a != r.w_at || m != r.w_mt || c != r.created || r.code != 0 {
                        return Err(TM_EBAL);
                    }
                    if r.at.kind == T_OMIT && a != r.ino.atime && r.have {
                        return Err(TM_EBAL);
                    }
                    ok += 1;
                }
                Err(e) => {
                    if r.ok || r.code != (e & 0xffff) as u16 {
                        return Err(TM_EBAL);
                    }
                    bad += 1;
                }
            }
            i += 1;
        }
        if ok != self.ok_count() || bad != self.err_count() {
            return Err(TM_EBAL);
        }
        Ok(())
    }

    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                h = fold(h, if r.have { 1 } else { 0 });
                h = fold(h, r.ino.uid as u64);
                h = fold(h, r.ino.mode as u64);
                h = fold(h, r.ino.atime as u64);
                h = fold(h, r.ino.mtime as u64);
                h = fold(h, r.cred.uid as u64);
                h = fold(h, if r.cred.cap { 1 } else { 0 });
                h = fold(h, r.at.kind as u64);
                h = fold(h, r.at.val as u64);
                h = fold(h, r.mt.kind as u64);
                h = fold(h, r.mt.val as u64);
                h = fold(h, r.w_at as u64);
                h = fold(h, r.w_mt as u64);
                h = fold(h, r.code as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(TM_EFROZEN);
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
            Err(TM_ESEAL)
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

fn node() -> Inode {
    Inode { uid: 100, mode: 0o644, atime: 10, mtime: 20 }
}

fn own() -> Cred {
    Cred { uid: 100, cap: false }
}

fn other() -> Cred {
    Cred { uid: 200, cap: false }
}

fn rootc() -> Cred {
    Cred { uid: 0, cap: true }
}

fn val(v: u32) -> Spec {
    Spec { kind: 0, val: v }
}

/// 十四行账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let _ = l.register(0, true, node(), own(), val(100), val(200), false, false, 100, 200, false, true);
    let _ = l.register(1, true, node(), own(), Spec { kind: T_NOW, val: 0 }, Spec { kind: T_NOW, val: 0 }, false, false, CLOCK, CLOCK, false, true);
    let _ = l.register(2, true, node(), own(), Spec { kind: T_OMIT, val: 0 }, val(300), false, false, 10, 300, false, true);
    let _ = l.register(3, true, node(), other(), Spec { kind: T_OMIT, val: 0 }, Spec { kind: T_OMIT, val: 0 }, false, false, 10, 20, false, true);
    let _ = l.register(4, true, node(), other(), val(5), Spec { kind: T_OMIT, val: 0 }, false, false, 0, 0, false, false);
    let _ = l.register(5, true, node(), other(), Spec { kind: T_NOW, val: 0 }, Spec { kind: T_OMIT, val: 0 }, false, false, 0, 0, false, false);
    let _ = l.register(6, true, Inode { uid: 100, mode: 0o666, atime: 10, mtime: 20 }, other(), Spec { kind: T_NOW, val: 0 }, Spec { kind: T_OMIT, val: 0 }, false, false, CLOCK, 20, false, true);
    let _ = l.register(7, true, node(), rootc(), val(7), val(8), false, false, 7, 8, false, true);
    let _ = l.register(8, true, node(), rootc(), Spec { kind: T_NOW, val: 0 }, Spec { kind: T_OMIT, val: 0 }, false, true, 0, 0, false, false);
    let _ = l.register(9, false, node(), own(), Spec { kind: T_NOW, val: 0 }, Spec { kind: T_NOW, val: 0 }, true, false, CLOCK, CLOCK, true, true);
    let _ = l.register(10, false, node(), own(), val(50), val(60), false, false, 0, 0, false, false);
    let _ = l.register(11, true, node(), own(), val(4_000_000_000), val(4_000_000_001), false, false, 4_000_000_000, 4_000_000_001, false, true);
    let _ = l.register(12, true, node(), own(), val(77), Spec { kind: T_OMIT, val: 0 }, false, false, 77, 20, false, true);
    let _ = l.register(13, true, node(), other(), val(88), Spec { kind: T_OMIT, val: 0 }, false, false, 0, 0, false, false);
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

    let a = utimens(Some(node()), own(), val(100), Spec { kind: T_OMIT, val: 0 }, false, false);
    out[1] = a.map(|(n, d)| (n.atime, n.mtime, d.created)) == Ok((100, 20, false));

    let b = utimens(Some(node()), own(), Spec { kind: T_NOW, val: 0 }, Spec { kind: T_OMIT, val: 0 }, false, false);
    out[2] = b.map(|(n, _)| (n.atime, n.mtime)) == Ok((CLOCK, 20));

    let c = utimens(Some(node()), other(), val(5), Spec { kind: T_OMIT, val: 0 }, false, false);
    let d = utimens(None, own(), Spec { kind: T_NOW, val: 0 }, Spec { kind: T_NOW, val: 0 }, true, false);
    out[3] = c == Err(TM_EACCES)
        && d.map(|(n, k)| (n.mode, n.atime, k.created)) == Ok((0o644, CLOCK, true))
        && utimens(Some(node()), rootc(), val(1), val(2), false, true) == Err(TM_EROFS);
    out
}

/// 十三判据。
pub fn run_unx11450_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11450.utimens");
    let l = build_ledger();
    s.add_dyn("TM1-九成五拒", l.ok_count() == 9 && l.err_count() == 5 && l.audit().is_ok(), &format!("ok={} err={}", l.ok_count(), l.err_count()));
    s.add("TM2-显式双值", l.row(0).map(|r| (r.w_at, r.w_mt)) == Some((100, 200)), "static");
    s.add("TM3-当前时钟", l.row(1).map(|r| (r.w_at, r.w_mt)) == Some((CLOCK, CLOCK)), "static");
    s.add("TM4-省略保持", l.row(2).map(|r| (r.w_at, r.w_mt)) == Some((10, 300)), "static");
    s.add("TM5-双省略免权限", l.row(3).map(|r| (r.w_at, r.w_mt)) == Some((10, 20)), "static");
    s.add("TM6-非属主显式被拒", l.row(4).map(|r| r.code) == Some((TM_EACCES & 0xffff) as u16) && l.row(13).map(|r| r.code) == Some((TM_EACCES & 0xffff) as u16), "static");
    s.add("TM7-非属主无权now", l.row(5).map(|r| r.code) == Some((TM_EACCES & 0xffff) as u16), "static");
    s.add("TM8-可写文件now", l.row(6).map(|r| (r.w_at, r.w_mt)) == Some((CLOCK, 20)), "static");
    s.add("TM9-能力显式", l.row(7).map(|r| (r.w_at, r.w_mt)) == Some((7, 8)), "static");
    s.add("TM10-只读优先", l.row(8).map(|r| r.code) == Some((TM_EROFS & 0xffff) as u16), "static");
    s.add("TM11-创建与缺失", l.row(9).map(|r| (r.created, r.w_at)) == Some((true, CLOCK)) && l.row(10).map(|r| r.code) == Some((TM_ENOENT & 0xffff) as u16), "static");
    s.add("TM12-大值与单参数", l.row(11).map(|r| r.w_at) == Some(4_000_000_000) && l.row(12).map(|r| (r.w_at, r.w_mt)) == Some((77, 20)) && l.rejects() == 0, "static");
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(TM_EFROZEN);
    let inj = inject_four();
    s.add_dyn("TM13-冻结重放四注入", root != 0 && rep == Ok(root) && again && inj == [true; 4], &format!("root={:016x} {:?}", root, inj));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_values() {
        let r = utimens(Some(node()), own(), val(1), val(2), false, false);
        assert_eq!(r.map(|(n, _)| (n.atime, n.mtime)), Ok((1, 2)));
    }

    #[test]
    fn omit_keeps() {
        let r = utimens(Some(node()), own(), Spec { kind: T_OMIT, val: 0 }, val(9), false, false);
        assert_eq!(r.map(|(n, _)| (n.atime, n.mtime)), Ok((10, 9)));
    }

    #[test]
    fn now_sets_clock() {
        let r = utimens(Some(node()), own(), Spec { kind: T_NOW, val: 0 }, Spec { kind: T_NOW, val: 0 }, false, false);
        assert_eq!(r.map(|(n, _)| (n.atime, n.mtime)), Ok((CLOCK, CLOCK)));
    }

    #[test]
    fn permission_rules() {
        assert_eq!(utimens(Some(node()), other(), val(1), val(2), false, false), Err(TM_EACCES));
        assert_eq!(utimens(Some(node()), other(), Spec { kind: T_NOW, val: 0 }, Spec { kind: T_OMIT, val: 0 }, false, false), Err(TM_EACCES));
        let w = Inode { uid: 100, mode: 0o666, atime: 10, mtime: 20 };
        assert!(utimens(Some(w), other(), Spec { kind: T_NOW, val: 0 }, Spec { kind: T_OMIT, val: 0 }, false, false).is_ok());
    }

    #[test]
    fn read_only_wins() {
        assert_eq!(utimens(Some(node()), rootc(), val(1), val(2), false, true), Err(TM_EROFS));
    }

    #[test]
    fn create_mode() {
        let r = utimens(None, own(), Spec { kind: T_NOW, val: 0 }, Spec { kind: T_NOW, val: 0 }, true, false);
        assert_eq!(r.map(|(n, d)| (n.mode, d.created)), Ok((0o644, true)));
        assert_eq!(utimens(None, own(), val(1), val(2), false, false), Err(TM_ENOENT));
    }

    #[test]
    fn ledger_and_checks() {
        let l = build_ledger();
        assert_eq!(l.len(), ROWS);
        assert_eq!(l.ok_count(), 9);
        assert_eq!(inject_four(), [true; 4]);
        assert_eq!(run_unx11450_checks().failed_count(), 0);
    }
}
