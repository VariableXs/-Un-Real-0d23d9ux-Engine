//! UNX-F11446 访问权限判定（0x20ba0 段）
//!
//! access 系列系统调用回答的不是“我是谁”，而是“以某个身份请求这三位权限
//! 是否可行”。模型把文件属性（属主、属组、模式、是否目录）与调用者身份（真实
//! 与有效 uid/gid）分开保存，由 effective 开关选择使用哪一组身份。判定先看
//! 挂载点是否只读，再处理 root 的特例，最后按属主、属组、其他三级取位。
//!
//! root 的读写总是通过，执行则要看对象：目录对 root 永远可进入，普通文件必须
//! 至少有一位执行位置一，否则仍然 EACCES。只读挂载对写请求返回 EROFS，这个
//! 判断发生在身份之前，因此 root 也不能绕过。请求掩码只允许低三位，出现第四位
//! 即 EINVAL，避免把“清空请求”误当成成功。
//!
//! 账本每一行固定文件、身份、请求与有效位，审计时从行内字段重建判定并核对
//! 结果或错误码。封印把属性字节与结论一起折叠，冻结后拒绝登记，重放则重建账本
//! 比对签名。四路注入分别检查属主读写、其他用户被拒、root 文件执行被拒和只读
//! 挂载，确保实现没有把“root 万能”或“只读只写在打开时检查”当成捷径。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20ba0;
/// 编号非法。
pub const AC_EID: u32 = SEG_BASE + 0x01;
/// 顺序错误。
pub const AC_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const AC_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const AC_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测缺失。
pub const AC_ENOBS: u32 = SEG_BASE + 0x05;
/// 结果不符。
pub const AC_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const AC_EBAL: u32 = SEG_BASE + 0x07;
/// 权限不足。
pub const AC_EACCES: u32 = SEG_BASE + 0x08;
/// 只读挂载。
pub const AC_EROFS: u32 = SEG_BASE + 0x09;
/// 请求非法。
pub const AC_EINVAL: u32 = SEG_BASE + 0x0a;
/// 票据不一致。
pub const AC_ESEAL: u32 = SEG_BASE + 0x0b;

/// 读。
pub const A_R: u8 = 4;
/// 写。
pub const A_W: u8 = 2;
/// 执行。
pub const A_X: u8 = 1;
/// 合法请求掩码。
pub const A_ALL: u8 = 7;

/// 文件属性。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct File {
    pub uid: u16,
    pub gid: u16,
    pub mode: u16,
    pub is_dir: bool,
    pub ro: bool,
}

/// 调用者身份。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cred {
    pub ruid: u16,
    pub euid: u16,
    pub rgid: u16,
    pub egid: u16,
}

/// 判定。
pub fn access(f: File, c: Cred, amode: u8, effective: bool) -> Result<(), u32> {
    if amode & !A_ALL != 0 {
        return Err(AC_EINVAL);
    }
    if amode == 0 {
        return Ok(());
    }
    if f.ro && amode & A_W != 0 {
        return Err(AC_EROFS);
    }
    let uid = if effective { c.euid } else { c.ruid };
    let gid = if effective { c.egid } else { c.rgid };
    if uid == 0 {
        if amode & A_X != 0 {
            if f.is_dir {
                return Ok(());
            }
            if f.mode & 0o111 == 0 {
                return Err(AC_EACCES);
            }
        }
        return Ok(());
    }
    let shift = if uid == f.uid {
        6u16
    } else if gid == f.gid {
        3u16
    } else {
        0u16
    };
    let bits = (f.mode >> shift) & 7;
    let mut need = 0u16;
    if amode & A_R != 0 {
        need |= 4;
    }
    if amode & A_W != 0 {
        need |= 2;
    }
    if amode & A_X != 0 {
        need |= 1;
    }
    if bits & need == need {
        Ok(())
    } else {
        Err(AC_EACCES)
    }
}

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub f: File,
    pub c: Cred,
    pub amode: u8,
    pub effective: bool,
    pub code: u16,
    pub ok: bool,
}

impl Row {
    pub fn pack(&self) -> [u8; 12] {
        let mode = self.f.mode;
        let mut b = [0u8; 12];
        b[0] = (mode & 0xff) as u8;
        b[1] = (mode >> 8) as u8;
        b[2] = if self.f.is_dir { 1 } else { 0 };
        b[3] = if self.f.ro { 1 } else { 0 };
        b[4] = (self.f.uid & 0xff) as u8;
        b[5] = (self.f.gid & 0xff) as u8;
        b[6] = (self.c.ruid & 0xff) as u8;
        b[7] = (self.c.euid & 0xff) as u8;
        b[8] = (self.c.rgid & 0xff) as u8;
        b[9] = (self.c.egid & 0xff) as u8;
        b[10] = self.amode;
        b[11] = if self.effective { 1 } else { 0 };
        b
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

    pub fn register(&mut self, slot: u8, f: File, c: Cred, amode: u8, effective: bool, ok: bool) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(AC_EFROZEN);
        }
        if (slot as usize) >= 16 {
            self.rejects += 1;
            return Err(AC_EID);
        }
        if (slot as usize) < self.rows.len() {
            self.rejects += 1;
            return Err(AC_EDUP);
        }
        if (slot as usize) != self.rows.len() {
            self.rejects += 1;
            return Err(AC_EORDER);
        }
        let got = access(f, c, amode, effective);
        let mut r = Row { f, c, amode, effective, code: 0, ok };
        match got {
            Ok(()) => {
                if !ok {
                    self.rejects += 1;
                    return Err(AC_EMISMATCH);
                }
            }
            Err(e) => {
                if ok {
                    self.rejects += 1;
                    return Err(AC_EMISMATCH);
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
            return Err(AC_EBAL);
        }
        let mut i = 0usize;
        let mut ok = 0usize;
        let mut bad = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(AC_ENOBS),
            };
            match access(r.f, r.c, r.amode, r.effective) {
                Ok(()) => {
                    if !r.ok || r.code != 0 {
                        return Err(AC_EBAL);
                    }
                    ok += 1;
                }
                Err(e) => {
                    if r.ok || r.code != (e & 0xffff) as u16 {
                        return Err(AC_EBAL);
                    }
                    bad += 1;
                }
            }
            let p = r.pack();
            if p[10] & !A_ALL != 0 && r.ok {
                return Err(AC_EBAL);
            }
            i += 1;
        }
        if ok != self.ok_count() || bad != self.err_count() {
            return Err(AC_EBAL);
        }
        Ok(())
    }

    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                let p = r.pack();
                let mut k = 0usize;
                while k < p.len() {
                    if let Some(b) = p.get(k) {
                        h ^= *b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                    k += 1;
                }
                h = fold(h, r.code as u64);
                h = fold(h, if r.ok { 1 } else { 0 });
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(AC_EFROZEN);
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
            Err(AC_ESEAL)
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

fn owner() -> Cred {
    Cred { ruid: 100, euid: 100, rgid: 100, egid: 100 }
}

fn other() -> Cred {
    Cred { ruid: 200, euid: 200, rgid: 200, egid: 200 }
}

fn rootc() -> Cred {
    Cred { ruid: 0, euid: 0, rgid: 0, egid: 0 }
}

fn f644() -> File {
    File { uid: 100, gid: 100, mode: 0o644, is_dir: false, ro: false }
}

fn f664() -> File {
    File { uid: 100, gid: 100, mode: 0o664, is_dir: false, ro: false }
}

fn f755d() -> File {
    File { uid: 100, gid: 100, mode: 0o755, is_dir: true, ro: false }
}

/// 十五行账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let _ = l.register(0, f644(), owner(), A_R, true, true);
    let _ = l.register(1, f644(), owner(), A_W, true, true);
    let _ = l.register(2, f644(), owner(), A_X, true, false);
    let _ = l.register(3, f644(), other(), A_R, true, true);
    let _ = l.register(4, f644(), other(), A_W, true, false);
    let _ = l.register(5, f644(), Cred { ruid: 150, euid: 150, rgid: 100, egid: 100 }, A_R, false, true);
    let _ = l.register(6, f664(), Cred { ruid: 150, euid: 150, rgid: 100, egid: 100 }, A_W, false, true);
    let _ = l.register(7, f644(), Cred { ruid: 150, euid: 150, rgid: 100, egid: 100 }, A_X, false, false);
    let _ = l.register(8, f644(), rootc(), A_R, true, true);
    let _ = l.register(9, f644(), rootc(), A_X, true, false);
    let _ = l.register(10, f755d(), rootc(), A_X, true, true);
    let _ = l.register(11, File { uid: 100, gid: 100, mode: 0o644, is_dir: false, ro: true }, owner(), A_W, true, false);
    let _ = l.register(12, f644(), owner(), 8, true, false);
    let _ = l.register(13, File { uid: 100, gid: 100, mode: 0o600, is_dir: false, ro: false }, Cred { ruid: 100, euid: 200, rgid: 100, egid: 200 }, A_R, false, true);
    let _ = l.register(14, File { uid: 100, gid: 100, mode: 0o600, is_dir: false, ro: false }, Cred { ruid: 100, euid: 200, rgid: 100, egid: 200 }, A_R, true, false);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.len() == 15 && l.ok_count() == 8 && l.err_count() == 7 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    out[0] = base && root != 0 && rep == Ok(root);

    out[1] = access(f644(), owner(), A_R | A_W, true).is_ok()
        && access(f644(), owner(), A_X, true) == Err(AC_EACCES);

    out[2] = access(f644(), other(), A_R, true).is_ok()
        && access(f644(), other(), A_W, true) == Err(AC_EACCES)
        && access(f664(), Cred { ruid: 150, euid: 150, rgid: 100, egid: 100 }, A_W, false).is_ok();

    out[3] = access(f644(), rootc(), A_X, true) == Err(AC_EACCES)
        && access(f755d(), rootc(), A_X, true).is_ok()
        && access(File { uid: 0, gid: 0, mode: 0o644, is_dir: false, ro: true }, rootc(), A_W, true) == Err(AC_EROFS);
    out
}

/// 十三判据。
pub fn run_unx11446_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11446.access");
    let l = build_ledger();
    s.add("AC1-十五行", l.len() == 15, "static");
    s.add_dyn("AC2-八成七拒", l.ok_count() == 8 && l.err_count() == 7 && l.audit().is_ok(), &format!("ok={} err={}", l.ok_count(), l.err_count()));
    s.add("AC3-属主读写", l.row(0).map(|r| r.ok) == Some(true) && l.row(1).map(|r| r.ok) == Some(true), "static");
    s.add("AC4-属主执行被拒", l.row(2).map(|r| r.code) == Some((AC_EACCES & 0xffff) as u16), "static");
    s.add("AC5-其他用户", l.row(3).map(|r| r.ok) == Some(true) && l.row(4).map(|r| r.code) == Some((AC_EACCES & 0xffff) as u16), "static");
    s.add("AC6-属组两级", l.row(5).map(|r| r.ok) == Some(true) && l.row(6).map(|r| r.ok) == Some(true) && l.row(7).map(|r| r.code) == Some((AC_EACCES & 0xffff) as u16), "static");
    s.add("AC7-root读写", l.row(8).map(|r| r.ok) == Some(true), "static");
    s.add("AC8-root执行特例", l.row(9).map(|r| r.code) == Some((AC_EACCES & 0xffff) as u16) && l.row(10).map(|r| r.ok) == Some(true), "static");
    s.add("AC9-只读挂载", l.row(11).map(|r| r.code) == Some((AC_EROFS & 0xffff) as u16), "static");
    s.add("AC10-非法掩码", l.row(12).map(|r| r.code) == Some((AC_EINVAL & 0xffff) as u16) && access(f644(), owner(), 0, true).is_ok(), "static");
    s.add("AC11-有效身份切换", l.row(13).map(|r| r.ok) == Some(true) && l.row(14).map(|r| r.code) == Some((AC_EACCES & 0xffff) as u16), "static");
    s.add("AC12-计数守恒", l.rejects() == 0 && l.ok_count() + l.err_count() == l.len(), "static");
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(AC_EFROZEN);
    let inj = inject_four();
    s.add_dyn("AC13-冻结重放四注入", root != 0 && rep == Ok(root) && again && inj == [true; 4], &format!("root={:016x} {:?}", root, inj));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_bits() {
        assert!(access(f644(), owner(), A_R | A_W, true).is_ok());
        assert_eq!(access(f644(), owner(), A_X, true), Err(AC_EACCES));
    }

    #[test]
    fn other_bits() {
        assert!(access(f644(), other(), A_R, true).is_ok());
        assert_eq!(access(f644(), other(), A_R | A_W, true), Err(AC_EACCES));
    }

    #[test]
    fn group_bits() {
        let c = Cred { ruid: 150, euid: 150, rgid: 100, egid: 100 };
        assert!(access(f664(), c, A_R | A_W, false).is_ok());
        assert_eq!(access(f644(), c, A_W, false), Err(AC_EACCES));
    }

    #[test]
    fn root_exec_rule() {
        assert!(access(f644(), rootc(), A_R | A_W, true).is_ok());
        assert_eq!(access(f644(), rootc(), A_X, true), Err(AC_EACCES));
        assert!(access(f755d(), rootc(), A_X, true).is_ok());
    }

    #[test]
    fn read_only_blocks_write() {
        let f = File { uid: 0, gid: 0, mode: 0o777, is_dir: false, ro: true };
        assert_eq!(access(f, rootc(), A_W, true), Err(AC_EROFS));
        assert!(access(f, rootc(), A_R | A_X, true).is_ok());
    }

    #[test]
    fn invalid_mask() {
        assert_eq!(access(f644(), owner(), 8, true), Err(AC_EINVAL));
        assert!(access(f644(), owner(), 0, true).is_ok());
    }

    #[test]
    fn ledger_and_checks() {
        let l = build_ledger();
        assert_eq!(l.len(), 15);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 7);
        assert_eq!(inject_four(), [true; 4]);
        assert_eq!(run_unx11446_checks().failed_count(), 0);
    }
}
