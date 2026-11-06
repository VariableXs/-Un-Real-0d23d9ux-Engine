//! UNX-F11469 umask 与 chmod 权限语义（0x20d20 段）
//!
//! 新建文件的权限是 `mode & ~umask`，而 `chmod` 的权限是绝对赋值、完全
//! 不经过 umask；两者混用是用户态最常见的权限错误来源。本模块把进程
//! 主体（uid、gid、附加组、umask、能力位）和 inode（uid、gid、十二位
//! 模式）分开建模，每次操作都重新选择属主/属组/其他三段权限位。
//!
//! 特权规则单独成层：非属主不能 `chmod`，除非持有能力位；属主可以把
//! 文件组改成自己所在的组，但只有特权主体能改属主；无论谁执行 `chown`
//! 成功，都会清掉 setuid 与 setgid。非特权属主把 setgid 位写回时，若
//! 自己并不属于文件属组，内核会静默清除该位，而不是返回错误。
//!
//! 不变量：模式字段永远只有低十二位；`access` 只返回允许或 `EACCES`，
//! 不会修改 inode；`chown` 成功后 suid/sgid 必为零；能力位主体绕过
//! 属主检查但不改变 umask 的计算方式。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20d20;
pub const UM_EID: u32 = SEG_BASE + 0x01;
pub const UM_EORDER: u32 = SEG_BASE + 0x02;
pub const UM_EDUP: u32 = SEG_BASE + 0x03;
pub const UM_EFROZEN: u32 = SEG_BASE + 0x04;
pub const UM_ENOBS: u32 = SEG_BASE + 0x05;
pub const UM_EMISMATCH: u32 = SEG_BASE + 0x06;
pub const UM_EBAL: u32 = SEG_BASE + 0x07;
pub const UM_ESEAL: u32 = SEG_BASE + 0x08;
pub const UM_EPERM: u32 = SEG_BASE + 0x09;
pub const UM_EACCES: u32 = SEG_BASE + 0x0a;
pub const UM_EINVAL: u32 = SEG_BASE + 0x0b;
pub const UM_ENOENT: u32 = SEG_BASE + 0x0c;
pub const UM_EBADF: u32 = SEG_BASE + 0x0d;

pub const S_ISUID: u16 = 0o4000;
pub const S_ISGID: u16 = 0o2000;
pub const S_ISVTX: u16 = 0o1000;
pub const PERM_MASK: u16 = 0o7777;

pub const A_R: u8 = 4;
pub const A_W: u8 = 2;
pub const A_X: u8 = 1;

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

/// 主体。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Subject {
    pub uid: u16,
    pub gid: u16,
    pub groups: [u16; 4],
    pub ngroups: u8,
    pub umask: u16,
    pub cap: bool,
}

impl Subject {
    pub fn in_group(&self, gid: u16) -> bool {
        if self.gid == gid {
            return true;
        }
        let mut i = 0usize;
        while i < self.ngroups as usize {
            if self.groups.get(i) == Some(&gid) {
                return true;
            }
            i += 1;
        }
        false
    }
}

/// inode 权限状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inode {
    pub uid: u16,
    pub gid: u16,
    pub mode: u16,
}

/// 世界。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct World {
    pub subj: Subject,
    pub ino: Inode,
}

/// 一次操作的结果数据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Out {
    pub mode: u16,
    pub granted: bool,
}

/// 新建文件：mode 与 umask 的按位清除。
pub fn apply_umask(mode: u16, umask: u16) -> u16 {
    mode & !umask & PERM_MASK
}

/// chmod：绝对赋值，但受属主与能力约束。
pub fn chmod(ino: &mut Inode, mode: u16, subj: &Subject) -> Result<u16, u32> {
    if mode & !PERM_MASK != 0 {
        return Err(UM_EINVAL);
    }
    if subj.uid != ino.uid && !subj.cap {
        return Err(UM_EPERM);
    }
    let mut m = mode & PERM_MASK;
    if !subj.cap && !subj.in_group(ino.gid) {
        m &= !S_ISGID;
    }
    ino.mode = m;
    Ok(m)
}

/// chown：属主不能改属主；成功后清除 suid/sgid。
pub fn chown(ino: &mut Inode, uid: u16, gid: u16, subj: &Subject) -> Result<(), u32> {
    if !subj.cap {
        if subj.uid != ino.uid {
            return Err(UM_EPERM);
        }
        if uid != ino.uid {
            return Err(UM_EPERM);
        }
        if gid != ino.gid && !subj.in_group(gid) {
            return Err(UM_EPERM);
        }
    }
    ino.uid = uid;
    ino.gid = gid;
    ino.mode &= !(S_ISUID | S_ISGID);
    Ok(())
}

/// 选择权限三元组。
pub fn class_bits(ino: &Inode, subj: &Subject) -> u16 {
    if subj.uid == ino.uid {
        (ino.mode >> 6) & 7
    } else if subj.in_group(ino.gid) {
        (ino.mode >> 3) & 7
    } else {
        ino.mode & 7
    }
}

/// access(2) 语义。
pub fn access(ino: &Inode, want: u8, subj: &Subject) -> Result<bool, u32> {
    if want == 0 || want & !7 != 0 {
        return Err(UM_EINVAL);
    }
    if subj.cap {
        return Ok(true);
    }
    let bits = class_bits(ino, subj);
    if (bits & (want as u16)) == (want as u16) {
        Ok(true)
    } else {
        Err(UM_EACCES)
    }
}

/// 基础世界。
pub fn base_world() -> World {
    World {
        subj: Subject {
            uid: 1000,
            gid: 100,
            groups: [100, 200, 0, 0],
            ngroups: 2,
            umask: 0o022,
            cap: false,
        },
        ino: Inode { uid: 1000, gid: 100, mode: 0o6755 },
    }
}

/// 场景执行。
pub fn run_scen(id: u8) -> Result<Out, u32> {
    let mut w = base_world();
    match id {
        0 => Ok(Out { mode: apply_umask(0o777, w.subj.umask), granted: false }),
        1 => {
            w.subj.umask = 0o077;
            Ok(Out { mode: apply_umask(0o666, w.subj.umask), granted: false })
        }
        2 => Ok(Out { mode: apply_umask(0o4755, w.subj.umask), granted: false }),
        3 => {
            let m = chmod(&mut w.ino, 0o640, &w.subj)?;
            Ok(Out { mode: m, granted: false })
        }
        4 => {
            w.subj.uid = 2000;
            let m = chmod(&mut w.ino, 0o640, &w.subj)?;
            Ok(Out { mode: m, granted: false })
        }
        5 => {
            w.subj.uid = 2000;
            w.subj.cap = true;
            let m = chmod(&mut w.ino, 0o640, &w.subj)?;
            Ok(Out { mode: m, granted: false })
        }
        6 => {
            w.subj.gid = 300;
            w.subj.groups = [300, 0, 0, 0];
            w.subj.ngroups = 1;
            let m = chmod(&mut w.ino, 0o2750, &w.subj)?;
            Ok(Out { mode: m, granted: false })
        }
        7 => {
            let g = access(&w.ino, A_R, &w.subj)?;
            Ok(Out { mode: w.ino.mode, granted: g })
        }
        8 => {
            w.subj.uid = 2000;
            w.subj.gid = 100;
            let g = access(&w.ino, A_W, &w.subj)?;
            Ok(Out { mode: w.ino.mode, granted: g })
        }
        9 => {
            w.subj.uid = 3000;
            w.subj.gid = 300;
            w.subj.groups = [300, 0, 0, 0];
            w.subj.ngroups = 1;
            let g = access(&w.ino, A_W, &w.subj)?;
            Ok(Out { mode: w.ino.mode, granted: g })
        }
        10 => {
            w.subj.uid = 3000;
            w.subj.gid = 300;
            w.subj.groups = [300, 0, 0, 0];
            w.subj.ngroups = 1;
            w.subj.cap = true;
            let g = access(&w.ino, A_W, &w.subj)?;
            Ok(Out { mode: w.ino.mode, granted: g })
        }
        11 => {
            chown(&mut w.ino, 1000, 200, &w.subj)?;
            Ok(Out { mode: w.ino.mode, granted: false })
        }
        12 => {
            chown(&mut w.ino, 4000, 100, &w.subj)?;
            Ok(Out { mode: w.ino.mode, granted: false })
        }
        13 => {
            let m = chmod(&mut w.ino, 0o10000, &w.subj)?;
            Ok(Out { mode: m, granted: false })
        }
        _ => Err(UM_EBADF),
    }
}

/// 样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub scen: u8,
    pub expect_ok: bool,
    pub mode: u16,
    pub granted: bool,
    pub code: u16,
}

/// 结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    pub mode: u16,
    pub granted: bool,
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
    pub fn register(&mut self, slot: u8, scen: u8, ok: bool, mode: u16, granted: bool, code: u16) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(UM_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(UM_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(UM_EDUP);
        }
        if (slot as usize) != self.samples.len() {
            self.rejects += 1;
            return Err(UM_EORDER);
        }
        let s = Sample { scen, expect_ok: ok, mode, granted, code };
        let out = match run_scen(scen) {
            Ok(o) => {
                if !ok || o.mode != mode || o.granted != granted {
                    return Err(UM_EMISMATCH);
                }
                Outcome { ok: true, mode: o.mode, granted: o.granted, code: 0 }
            }
            Err(e) => {
                let low = (e & 0xffff) as u16;
                if ok || low != code {
                    return Err(UM_EMISMATCH);
                }
                Outcome { ok: false, mode: 0, granted: false, code: low }
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
            return Err(UM_EBAL);
        }
        let mut i = 0;
        while i < self.samples.len() {
            let s = match self.samples.get(i) {
                Some(v) => *v,
                None => return Err(UM_ENOBS),
            };
            let o = match self.outcomes.get(i) {
                Some(v) => *v,
                None => return Err(UM_ENOBS),
            };
            if s.expect_ok != o.ok {
                return Err(UM_EMISMATCH);
            }
            match run_scen(s.scen) {
                Ok(v) => {
                    if !o.ok || v.mode != o.mode || v.granted != o.granted || o.code != 0 {
                        return Err(UM_EMISMATCH);
                    }
                    if v.mode & !PERM_MASK != 0 {
                        return Err(UM_EBAL);
                    }
                }
                Err(e) => {
                    let low = (e & 0xffff) as u16;
                    if o.ok || low != o.code {
                        return Err(UM_EMISMATCH);
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
            return Err(UM_EFROZEN);
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
                h = fold(h, o.mode as u64);
                h = fold(h, u64::from(o.granted));
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
            Err(UM_ESEAL)
        }
    }
}

/// 十四行账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let eperm = (UM_EPERM & 0xffff) as u16;
    let eacces = (UM_EACCES & 0xffff) as u16;
    let einval = (UM_EINVAL & 0xffff) as u16;
    let _ = l.register(0, 0, true, 0o755, false, 0);
    let _ = l.register(1, 1, true, 0o600, false, 0);
    let _ = l.register(2, 2, true, 0o4755, false, 0);
    let _ = l.register(3, 3, true, 0o640, false, 0);
    let _ = l.register(4, 4, false, 0, false, eperm);
    let _ = l.register(5, 5, true, 0o640, false, 0);
    let _ = l.register(6, 6, true, 0o750, false, 0);
    let _ = l.register(7, 7, true, 0o6755, true, 0);
    let _ = l.register(8, 8, false, 0, false, eacces);
    let _ = l.register(9, 9, false, 0, false, eacces);
    let _ = l.register(10, 10, true, 0o6755, true, 0);
    let _ = l.register(11, 11, true, 0o755, false, 0);
    let _ = l.register(12, 12, false, 0, false, eperm);
    let _ = l.register(13, 13, false, 0, false, einval);
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
    let twice = l.freeze() == Err(UM_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && twice;

    out[1] = apply_umask(0o777, 0o027) == 0o750
        && apply_umask(0o666, 0) == 0o666
        && apply_umask(0o777, 0o777) == 0
        && apply_umask(0o4755, 0o022) == 0o4755;

    let mut w = base_world();
    w.subj.gid = 300;
    w.subj.groups = [300, 0, 0, 0];
    w.subj.ngroups = 1;
    let sgid = chmod(&mut w.ino, 0o2750, &w.subj);
    let mut c = base_world();
    let owned = chown(&mut c.ino, 1000, 200, &c.subj);
    out[2] = sgid == Ok(0o750)
        && owned.is_ok()
        && c.ino.uid == 1000
        && c.ino.gid == 200
        && c.ino.mode == 0o755
        && c.ino.mode & (S_ISUID | S_ISGID) == 0;

    let mut s = base_world();
    s.subj.uid = 3000;
    s.subj.gid = 300;
    s.subj.groups = [300, 0, 0, 0];
    s.subj.ngroups = 1;
    s.ino.mode = 0o601;
    let deny_r = access(&s.ino, A_R, &s.subj);
    let allow_x = access(&s.ino, A_X, &s.subj);
    s.subj.cap = true;
    let cap_w = access(&s.ino, A_W, &s.subj);
    out[3] = deny_r == Err(UM_EACCES) && allow_x == Ok(true) && cap_w == Ok(true)
        && access(&s.ino, 0, &s.subj) == Err(UM_EINVAL);
    out
}

/// 十三项判据。
pub fn run_unx11469_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11469_umask_chmod");
    let l = build_ledger();
    s.add("UM1-十四行齐备", l.len() == SAMPLES && l.sample(0).map(|x| x.scen) == Some(0), "static");
    s.add_dyn(
        "UM2-九应五拒",
        l.ok_count() == 9 && l.err_count() == 5 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );
    s.add(
        "UM3-umask 按位清除",
        apply_umask(0o777, 0o022) == 0o755
            && apply_umask(0o666, 0o077) == 0o600
            && apply_umask(0o4755, 0o022) == 0o4755,
        "static",
    );
    s.add(
        "UM4-特定位不受 umask 影响",
        apply_umask(0o7777, 0o022) == 0o7755
            && apply_umask(0o2000, 0o002) == 0o2000,
        "static",
    );
    s.add(
        "UM5-chmod 绝对赋值",
        {
            let mut w = base_world();
            chmod(&mut w.ino, 0o640, &w.subj) == Ok(0o640) && w.ino.mode == 0o640
        },
        "static",
    );
    s.add(
        "UM6-非属主 chmod 拒绝",
        {
            let mut a = base_world();
            a.subj.uid = 2000;
            let denied = chmod(&mut a.ino, 0o640, &a.subj) == Err(UM_EPERM);
            a.subj.cap = true;
            denied && chmod(&mut a.ino, 0o640, &a.subj) == Ok(0o640)
        },
        "static",
    );
    s.add(
        "UM7-非成员清 setgid",
        {
            let mut w = base_world();
            w.subj.gid = 300;
            w.subj.groups = [300, 0, 0, 0];
            w.subj.ngroups = 1;
            chmod(&mut w.ino, 0o2750, &w.subj) == Ok(0o750)
                && w.ino.mode & S_ISGID == 0
        },
        "static",
    );
    s.add(
        "UM8-chown 清 suid 与 sgid",
        {
            let mut w = base_world();
            let r = chown(&mut w.ino, 1000, 200, &w.subj);
            r.is_ok() && w.ino.mode == 0o755 && w.ino.gid == 200
        },
        "static",
    );
    s.add(
        "UM9-属主不可改属主",
        {
            let mut w = base_world();
            let denied = chown(&mut w.ino, 4000, 100, &w.subj) == Err(UM_EPERM);
            w.subj.cap = true;
            denied && chown(&mut w.ino, 4000, 100, &w.subj).is_ok() && w.ino.uid == 4000
        },
        "static",
    );
    s.add(
        "UM10-access 三段选择",
        {
            let w = base_world();
            class_bits(&w.ino, &w.subj) == 7
                && {
                    let mut g = w;
                    g.subj.uid = 2000;
                    class_bits(&g.ino, &g.subj) == 5
                }
                && {
                    let mut o = w;
                    o.subj.uid = 3000;
                    o.subj.gid = 300;
                    o.subj.groups = [300, 0, 0, 0];
                    o.subj.ngroups = 1;
                    class_bits(&o.ino, &o.subj) == 5
                }
        },
        "static",
    );
    s.add(
        "UM11-access 拒绝与能力",
        {
            let mut w = base_world();
            w.subj.uid = 2000;
            w.subj.gid = 100;
            let denied = access(&w.ino, A_W, &w.subj) == Err(UM_EACCES);
            w.subj.cap = true;
            denied && access(&w.ino, A_W, &w.subj) == Ok(true)
        },
        "static",
    );
    s.add(
        "UM12-模式高位拒绝",
        {
            let mut w = base_world();
            chmod(&mut w.ino, 0o10000, &w.subj) == Err(UM_EINVAL)
                && w.ino.mode == 0o6755
                && apply_umask(0o17777, 0) == 0o7777
        },
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(UM_EFROZEN);
    s.add_dyn(
        "UM13-冻结重放与四路",
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
    fn umask_only_clears() {
        assert_eq!(apply_umask(0o777, 0o022), 0o755);
        assert_eq!(apply_umask(0o666, 0o077), 0o600);
        assert_eq!(apply_umask(0o4755, 0o022), 0o4755);
    }

    #[test]
    fn chmod_requires_ownership() {
        let mut w = base_world();
        w.subj.uid = 2000;
        assert_eq!(chmod(&mut w.ino, 0o640, &w.subj), Err(UM_EPERM));
        w.subj.cap = true;
        assert_eq!(chmod(&mut w.ino, 0o640, &w.subj), Ok(0o640));
    }

    #[test]
    fn non_member_loses_setgid() {
        let mut w = base_world();
        w.subj.gid = 300;
        w.subj.groups = [300, 0, 0, 0];
        w.subj.ngroups = 1;
        assert_eq!(chmod(&mut w.ino, 0o2750, &w.subj), Ok(0o750));
    }

    #[test]
    fn chown_clears_special_bits() {
        let mut w = base_world();
        assert_eq!(chown(&mut w.ino, 1000, 200, &w.subj), Ok(()));
        assert_eq!(w.ino.mode, 0o755);
        assert_eq!(w.ino.gid, 200);
    }

    #[test]
    fn access_uses_owner_class() {
        let w = base_world();
        assert_eq!(access(&w.ino, A_R | A_W, &w.subj), Ok(true));
    }

    #[test]
    fn access_denies_other_write() {
        let mut w = base_world();
        w.subj.uid = 3000;
        w.subj.gid = 300;
        w.subj.groups = [300, 0, 0, 0];
        w.subj.ngroups = 1;
        assert_eq!(access(&w.ino, A_W, &w.subj), Err(UM_EACCES));
    }

    #[test]
    fn ledger_is_consistent() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 9);
        assert_eq!(l.err_count(), 5);
        assert!(l.audit().is_ok());
        let s = run_unx11469_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
    }
}
