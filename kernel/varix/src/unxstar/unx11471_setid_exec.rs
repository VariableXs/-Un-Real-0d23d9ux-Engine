//! UNX-F11471 setuid 与 setgid 执行语义（0x20d40 段）
//!
//! 执行一个带 setuid 位的文件时，进程的 effective uid 变成文件属主，
//! 同时把该值写入 saved-set-uid，以便程序随后自愿放弃特权；real uid 与
//! 其余身份保持不变。setgid 位对 effective gid 与 saved-set-gid 做同样的
//! 事。两个位可以同时生效，例如 `-rwsr-sr-x root root` 的守护进程。
//!
//! 提权有三个闸门：文件系统以 `nosuid` 挂载、进程设置了 `no_new_privs`、
//! 以及文件本身没有任何执行位。前两者让特殊位静默失效（不是错误），
//! 第三者返回 `EACCES`。执行权限按 effective uid 选择属主/属组/其他
//! 三段中的一段，属组判断包含附加组。
//!
//! 不变量：exec 从不修改 real uid/gid；被抑制时凭据逐字段不变；成功后
//! `euid == suid` 且 `egid == sgid`（未提权时保持原值）；没有执行位时
//! 任何凭据都不改变。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20d40;
pub const SU_EID: u32 = SEG_BASE + 0x01;
pub const SU_EORDER: u32 = SEG_BASE + 0x02;
pub const SU_EDUP: u32 = SEG_BASE + 0x03;
pub const SU_EFROZEN: u32 = SEG_BASE + 0x04;
pub const SU_ENOBS: u32 = SEG_BASE + 0x05;
pub const SU_EMISMATCH: u32 = SEG_BASE + 0x06;
pub const SU_EBAL: u32 = SEG_BASE + 0x07;
pub const SU_ESEAL: u32 = SEG_BASE + 0x08;
pub const SU_EACCES: u32 = SEG_BASE + 0x09;
pub const SU_EPERM: u32 = SEG_BASE + 0x0a;
pub const SU_EINVAL: u32 = SEG_BASE + 0x0b;
pub const SU_ENOEXEC: u32 = SEG_BASE + 0x0c;
pub const SU_ENOTDIR: u32 = SEG_BASE + 0x0d;
pub const SU_EBADF: u32 = SEG_BASE + 0x0e;

pub const S_ISUID: u16 = 0o4000;
pub const S_ISGID: u16 = 0o2000;
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

/// 凭据三元组。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cred {
    pub uid: u16,
    pub euid: u16,
    pub suid: u16,
    pub gid: u16,
    pub egid: u16,
    pub sgid: u16,
}

/// 进程。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Proc {
    pub cred: Cred,
    pub groups: [u16; 4],
    pub ngroups: u8,
    pub nnps: bool,
}

impl Proc {
    pub fn in_group(&self, gid: u16) -> bool {
        if self.cred.gid == gid || self.cred.egid == gid {
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

/// 可执行文件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct File {
    pub uid: u16,
    pub gid: u16,
    pub mode: u16,
}

/// 结果快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Out {
    pub uid: u16,
    pub euid: u16,
    pub egid: u16,
    pub sgid: u16,
}

fn snapshot(p: &Proc) -> Out {
    Out { uid: p.cred.uid, euid: p.cred.euid, egid: p.cred.egid, sgid: p.cred.sgid }
}

/// 执行权限按 effective uid 选择类。
pub fn may_exec(file: &File, p: &Proc) -> bool {
    let bit = if p.cred.euid == file.uid {
        (file.mode >> 6) & 1
    } else if p.in_group(file.gid) {
        (file.mode >> 3) & 1
    } else {
        file.mode & 1
    };
    bit == 1
}

/// exec 的凭据变换。
pub fn exec(p: &mut Proc, file: &File, nosuid: bool, want_x: bool) -> Result<Out, u32> {
    if want_x && !may_exec(file, p) {
        return Err(SU_EACCES);
    }
    let elev = !p.nnps && !nosuid;
    let suid = elev && (file.mode & S_ISUID) != 0;
    let sgid = elev && (file.mode & S_ISGID) != 0;
    if suid {
        p.cred.euid = file.uid;
        p.cred.suid = file.uid;
    }
    if sgid {
        p.cred.egid = file.gid;
        p.cred.sgid = file.gid;
    }
    Ok(snapshot(p))
}

/// setuid(2) 语义。
pub fn setuid(p: &mut Proc, uid: u16) -> Result<Out, u32> {
    if p.cred.euid == 0 {
        p.cred.uid = uid;
        p.cred.euid = uid;
        p.cred.suid = uid;
        return Ok(snapshot(p));
    }
    if uid == p.cred.uid || uid == p.cred.suid {
        p.cred.euid = uid;
        p.cred.suid = uid;
        Ok(snapshot(p))
    } else {
        Err(SU_EPERM)
    }
}

/// setgid(2) 语义。
pub fn setgid(p: &mut Proc, gid: u16) -> Result<Out, u32> {
    if p.cred.euid == 0 {
        p.cred.gid = gid;
        p.cred.egid = gid;
        p.cred.sgid = gid;
        return Ok(snapshot(p));
    }
    if gid == p.cred.gid || gid == p.cred.sgid || p.in_group(gid) {
        p.cred.egid = gid;
        p.cred.sgid = gid;
        Ok(snapshot(p))
    } else {
        Err(SU_EPERM)
    }
}

/// 基础进程。
pub fn base_proc() -> Proc {
    Proc {
        cred: Cred { uid: 1000, euid: 1000, suid: 1000, gid: 100, egid: 100, sgid: 100 },
        groups: [100, 200, 0, 0],
        ngroups: 2,
        nnps: false,
    }
}

/// 基础文件。
pub fn base_file() -> File {
    File { uid: 0, gid: 0, mode: 0o4755 }
}

/// 场景执行。
pub fn run_scen(id: u8) -> Result<Out, u32> {
    let mut p = base_proc();
    let mut f = base_file();
    match id {
        0 => exec(&mut p, &f, false, true),
        1 => {
            f.gid = 50;
            f.mode = 0o2755;
            exec(&mut p, &f, false, true)
        }
        2 => {
            f.mode = 0o6755;
            exec(&mut p, &f, false, true)
        }
        3 => exec(&mut p, &f, true, true),
        4 => {
            p.nnps = true;
            exec(&mut p, &f, false, true)
        }
        5 => {
            f.mode = 0o644;
            exec(&mut p, &f, false, true)
        }
        6 => {
            f.uid = 1000;
            f.mode = 0o4700;
            p.cred.uid = 2000;
            p.cred.euid = 2000;
            p.cred.suid = 2000;
            exec(&mut p, &f, false, true)
        }
        7 => {
            f.mode = 0o755;
            exec(&mut p, &f, false, true)
        }
        8 => setuid(&mut p, 1000),
        9 => setuid(&mut p, 0),
        10 => {
            p.cred.uid = 0;
            p.cred.euid = 0;
            p.cred.suid = 0;
            setuid(&mut p, 2000)
        }
        11 => setgid(&mut p, 200),
        12 => setgid(&mut p, 300),
        13 => {
            f.uid = 1000;
            f.mode = 0o4700;
            exec(&mut p, &f, false, true)
        }
        _ => Err(SU_EBADF),
    }
}

/// 样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub scen: u8,
    pub expect_ok: bool,
    pub uid: u16,
    pub euid: u16,
    pub egid: u16,
    pub code: u16,
}

/// 结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    pub uid: u16,
    pub euid: u16,
    pub egid: u16,
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
    pub fn register(&mut self, slot: u8, scen: u8, ok: bool, uid: u16, euid: u16, egid: u16, code: u16) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(SU_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(SU_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(SU_EDUP);
        }
        if (slot as usize) != self.samples.len() {
            self.rejects += 1;
            return Err(SU_EORDER);
        }
        let s = Sample { scen, expect_ok: ok, uid, euid, egid, code };
        let out = match run_scen(scen) {
            Ok(o) => {
                if !ok || o.uid != uid || o.euid != euid || o.egid != egid {
                    return Err(SU_EMISMATCH);
                }
                Outcome { ok: true, uid: o.uid, euid: o.euid, egid: o.egid, code: 0 }
            }
            Err(e) => {
                let low = (e & 0xffff) as u16;
                if ok || low != code {
                    return Err(SU_EMISMATCH);
                }
                Outcome { ok: false, uid: 0, euid: 0, egid: 0, code: low }
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
            return Err(SU_EBAL);
        }
        let mut i = 0;
        while i < self.samples.len() {
            let s = match self.samples.get(i) {
                Some(v) => *v,
                None => return Err(SU_ENOBS),
            };
            let o = match self.outcomes.get(i) {
                Some(v) => *v,
                None => return Err(SU_ENOBS),
            };
            if s.expect_ok != o.ok {
                return Err(SU_EMISMATCH);
            }
            match run_scen(s.scen) {
                Ok(v) => {
                    if !o.ok || v.uid != o.uid || v.euid != o.euid || v.egid != o.egid {
                        return Err(SU_EMISMATCH);
                    }
                }
                Err(e) => {
                    let low = (e & 0xffff) as u16;
                    if o.ok || low != o.code {
                        return Err(SU_EMISMATCH);
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
            return Err(SU_EFROZEN);
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
                h = fold(h, o.uid as u64);
                h = fold(h, o.euid as u64);
                h = fold(h, o.egid as u64);
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
            Err(SU_ESEAL)
        }
    }
}

/// 十四行账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let eacces = (SU_EACCES & 0xffff) as u16;
    let eperm = (SU_EPERM & 0xffff) as u16;
    let _ = l.register(0, 0, true, 1000, 0, 100, 0);
    let _ = l.register(1, 1, true, 1000, 1000, 50, 0);
    let _ = l.register(2, 2, true, 1000, 0, 0, 0);
    let _ = l.register(3, 3, true, 1000, 1000, 100, 0);
    let _ = l.register(4, 4, true, 1000, 1000, 100, 0);
    let _ = l.register(5, 5, false, 0, 0, 0, eacces);
    let _ = l.register(6, 6, false, 0, 0, 0, eacces);
    let _ = l.register(7, 7, true, 1000, 1000, 100, 0);
    let _ = l.register(8, 8, true, 1000, 1000, 100, 0);
    let _ = l.register(9, 9, false, 0, 0, 0, eperm);
    let _ = l.register(10, 10, true, 2000, 2000, 100, 0);
    let _ = l.register(11, 11, true, 1000, 1000, 200, 0);
    let _ = l.register(12, 12, false, 0, 0, 0, eperm);
    let _ = l.register(13, 13, true, 1000, 1000, 100, 0);
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
    let twice = l.freeze() == Err(SU_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && twice;

    let mut p = base_proc();
    let f = File { uid: 0, gid: 0, mode: 0o6755 };
    let r = exec(&mut p, &f, false, true);
    out[1] = r == Ok(Out { uid: 1000, euid: 0, egid: 0, sgid: 0 })
        && p.cred.uid == 1000
        && p.cred.gid == 100
        && p.cred.euid == 0
        && p.cred.suid == 0;

    let mut a = base_proc();
    let normal = exec(&mut a, &base_file(), true, true);
    let mut b = base_proc();
    b.nnps = true;
    let locked = exec(&mut b, &base_file(), false, true);
    out[2] = normal.map(|o| (o.euid, o.egid)) == Ok((1000, 100))
        && locked.map(|o| (o.euid, o.egid)) == Ok((1000, 100))
        && a.cred.suid == 1000
        && b.cred.suid == 1000;

    let mut c = base_proc();
    c.cred.uid = 0;
    c.cred.euid = 0;
    c.cred.suid = 0;
    let root_set = setuid(&mut c, 2000);
    let mut d = base_proc();
    let deny = setuid(&mut d, 0);
    let own = setuid(&mut d, 1000);
    let mut g = base_proc();
    let supp = setgid(&mut g, 200);
    let other = setgid(&mut g, 300);
    out[3] = root_set.map(|o| (o.uid, o.euid)) == Ok((2000, 2000))
        && c.cred.suid == 2000
        && deny == Err(SU_EPERM)
        && own.map(|o| o.euid) == Ok(1000)
        && supp.map(|o| o.egid) == Ok(200)
        && other == Err(SU_EPERM);
    out
}

/// 十三项判据。
pub fn run_unx11471_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11471_setid_exec");
    let l = build_ledger();
    s.add("SU1-十四行齐备", l.len() == SAMPLES && l.sample(0).map(|x| x.scen) == Some(0), "static");
    s.add_dyn(
        "SU2-十应四拒",
        l.ok_count() == 10 && l.err_count() == 4 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );
    s.add(
        "SU3-setuid 提权",
        {
            let mut p = base_proc();
            let f = base_file();
            exec(&mut p, &f, false, true).is_ok()
                && p.cred.uid == 1000
                && p.cred.euid == 0
                && p.cred.suid == 0
        },
        "static",
    );
    s.add(
        "SU4-setgid 提权",
        {
            let mut p = base_proc();
            let f = File { uid: 0, gid: 50, mode: 0o2755 };
            exec(&mut p, &f, false, true).is_ok()
                && p.cred.egid == 50
                && p.cred.sgid == 50
                && p.cred.gid == 100
        },
        "static",
    );
    s.add(
        "SU5-双位同时生效",
        {
            let mut p = base_proc();
            let f = File { uid: 0, gid: 0, mode: 0o6755 };
            exec(&mut p, &f, false, true).map(|o| (o.euid, o.egid)) == Ok((0, 0))
                && p.cred.suid == 0
                && p.cred.sgid == 0
        },
        "static",
    );
    s.add(
        "SU6-nosuid 静默失效",
        {
            let mut p = base_proc();
            exec(&mut p, &base_file(), true, true).map(|o| (o.euid, o.egid)) == Ok((1000, 100))
                && p.cred.suid == 1000
                && p.cred.sgid == 100
        },
        "static",
    );
    s.add(
        "SU7-no_new_privs 抑制",
        {
            let mut p = base_proc();
            p.nnps = true;
            exec(&mut p, &base_file(), false, true).map(|o| o.euid) == Ok(1000)
                && p.cred.suid == 1000
        },
        "static",
    );
    s.add(
        "SU8-执行权限分类",
        {
            let mut p = base_proc();
            p.cred.uid = 2000;
            p.cred.euid = 2000;
            p.cred.suid = 2000;
            let f = File { uid: 1000, gid: 100, mode: 0o4700 };
            let denied = exec(&mut p, &f, false, true) == Err(SU_EACCES);
            let mut q = base_proc();
            let owner = File { uid: 1000, gid: 100, mode: 0o4700 };
            denied && exec(&mut q, &owner, false, true).map(|o| o.euid) == Ok(1000)
        },
        "static",
    );
    s.add(
        "SU9-普通执行不变",
        {
            let mut p = base_proc();
            let f = File { uid: 0, gid: 0, mode: 0o755 };
            exec(&mut p, &f, false, true).map(|o| (o.uid, o.euid, o.egid)) == Ok((1000, 1000, 100))
        },
        "static",
    );
    s.add(
        "SU10-setuid 三种路径",
        {
            let mut a = base_proc();
            let own = setuid(&mut a, 1000).is_ok() && a.cred.euid == 1000 && a.cred.suid == 1000;
            let mut b = base_proc();
            let deny = setuid(&mut b, 0) == Err(SU_EPERM) && b.cred.euid == 1000;
            let mut c = base_proc();
            c.cred.uid = 0;
            c.cred.euid = 0;
            let root = setuid(&mut c, 2000).is_ok() && c.cred.uid == 2000 && c.cred.euid == 2000;
            own && deny && root
        },
        "static",
    );
    s.add(
        "SU11-setgid 三种路径",
        {
            let mut a = base_proc();
            let own = setgid(&mut a, 100).is_ok() && a.cred.egid == 100;
            let mut b = base_proc();
            let supp = setgid(&mut b, 200).is_ok() && b.cred.egid == 200 && b.cred.sgid == 200;
            let mut c = base_proc();
            let deny = setgid(&mut c, 300) == Err(SU_EPERM) && c.cred.egid == 100;
            own && supp && deny
        },
        "static",
    );
    s.add(
        "SU12-审计与重放",
        l.audit().is_ok()
            && build_ledger().seal() == build_ledger().seal()
            && build_ledger().seal() != 0
            && {
                let mut f = build_ledger();
                let a = f.freeze();
                let b = f.freeze();
                a.is_ok() && b == Err(SU_EFROZEN)
            },
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    s.add_dyn(
        "SU13-冻结与四路注入",
        root != 0 && rep == Ok(root) && inject_four() == [true; 4],
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
    fn setuid_raises_effective_only() {
        let mut p = base_proc();
        assert!(exec(&mut p, &base_file(), false, true).is_ok());
        assert_eq!(p.cred.uid, 1000);
        assert_eq!(p.cred.euid, 0);
        assert_eq!(p.cred.suid, 0);
    }

    #[test]
    fn nosuid_is_silent() {
        let mut p = base_proc();
        assert_eq!(exec(&mut p, &base_file(), true, true).map(|o| o.euid), Ok(1000));
        assert_eq!(p.cred.suid, 1000);
    }

    #[test]
    fn no_new_privs_blocks() {
        let mut p = base_proc();
        p.nnps = true;
        assert_eq!(exec(&mut p, &base_file(), false, true).map(|o| o.euid), Ok(1000));
    }

    #[test]
    fn exec_needs_x_bit() {
        let mut p = base_proc();
        let f = File { uid: 0, gid: 0, mode: 0o644 };
        assert_eq!(exec(&mut p, &f, false, true), Err(SU_EACCES));
    }

    #[test]
    fn setuid_denies_unrelated() {
        let mut p = base_proc();
        assert_eq!(setuid(&mut p, 0), Err(SU_EPERM));
    }

    #[test]
    fn setgid_allows_supplementary() {
        let mut p = base_proc();
        assert_eq!(setgid(&mut p, 200).map(|o| o.egid), Ok(200));
        assert_eq!(setgid(&mut p, 300), Err(SU_EPERM));
    }

    #[test]
    fn ledger_is_consistent() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 10);
        assert_eq!(l.err_count(), 4);
        assert!(l.audit().is_ok());
        let s = run_unx11471_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
    }
}
