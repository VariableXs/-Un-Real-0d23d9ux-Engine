//! UNX-F11480 工作目录继承与引用计数（0x20dd0 段）
//!
//! 每个进程的当前工作目录只是目录节点上的一枚引用。`fork` 让子进程继承
//! 父进程的 cwd，引用计数加一；`exit` 释放引用，计数减一。`chdir` 先对
//! 目标目录加引用再对旧目录减引用，因此 `chdir` 到自身是合法的空操作，
//! 而切换到别处可能让旧目录的计数归零。
//!
//! 目录节点是否“存活”由引用计数和链接状态共同决定。`rmdir` 只能删除空
//! 目录，不能删除根目录，也不能删除调用者自己的 cwd；但它允许删除别的
//! 进程正在使用的目录：节点从父目录摘除并标记为 `removed`，引用它的进程
//! 仍持有 cwd，此时 `getcwd` 返回 `ENOENT`，相对路径中的 `..` 也失败，只有
//! `"."` 与绝对路径还能解析。当最后一个引用通过 `exit` 或 `chdir` 释放时
//! 节点被真正回收（`gone`）。
//!
//! 路径解析把连续的 `/` 压成一个，`.` 直接跳过，`..` 在根目录处钳制为
//! 根。不变量：`rc` 等于持有该 cwd 的存活进程数；`removed` 节点的父链接
//! 为 -1 且不在任何目录项中；`gone` 节点不可再被解析；存活节点数只统计
//! 未回收的节点。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20dd0;
pub const CT_EID: u32 = SEG_BASE + 0x01;
pub const CT_EORDER: u32 = SEG_BASE + 0x02;
pub const CT_EDUP: u32 = SEG_BASE + 0x03;
pub const CT_EFROZEN: u32 = SEG_BASE + 0x04;
pub const CT_ENOBS: u32 = SEG_BASE + 0x05;
pub const CT_EMISMATCH: u32 = SEG_BASE + 0x06;
pub const CT_EBAL: u32 = SEG_BASE + 0x07;
pub const CT_ESEAL: u32 = SEG_BASE + 0x08;
pub const CT_ENOENT: u32 = SEG_BASE + 0x09;
pub const CT_EINVAL: u32 = SEG_BASE + 0x0a;
pub const CT_ENOTEMPTY: u32 = SEG_BASE + 0x0b;
pub const CT_EBUSY: u32 = SEG_BASE + 0x0c;
pub const CT_EEXIST: u32 = SEG_BASE + 0x0d;
pub const CT_ESRCH: u32 = SEG_BASE + 0x0e;
pub const CT_ELOOP: u32 = SEG_BASE + 0x0f;

pub const SAMPLES: usize = 14;
pub const NAME_CAP: usize = 12;
pub const MAX_DEPTH: u32 = 40;

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

/// 目录节点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dir {
    pub id: u32,
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
    pub parent: i32,
    pub rc: u32,
    pub kids: u32,
    pub removed: bool,
    pub gone: bool,
}

/// 进程。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Proc {
    pub pid: u32,
    pub cwd: u32,
    pub alive: bool,
}

/// 目录世界。
pub struct World {
    pub dirs: Vec<Dir>,
    pub procs: Vec<Proc>,
    pub next_id: u32,
}

/// 查询结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Probe {
    pub pid: u32,
    pub cwd: i32,
    pub rc: u32,
    pub removed: bool,
    pub live: u32,
    pub path_len: u32,
}

fn blank(name: &[u8]) -> ([u8; NAME_CAP], u8) {
    let mut buf = [0u8; NAME_CAP];
    let n = if name.len() > NAME_CAP { NAME_CAP } else { name.len() };
    let mut i = 0usize;
    while i < n {
        if let Some(&b) = name.get(i) {
            buf[i] = b;
        }
        i += 1;
    }
    (buf, n as u8)
}

fn named(d: &Dir, comp: &[u8]) -> bool {
    if (d.nlen as usize) != comp.len() {
        return false;
    }
    match d.name.get(0..comp.len()) {
        Some(s) => s == comp,
        None => false,
    }
}

impl World {
    /// 根、/a、/a/b，两个进程分别停在 /a 与 /a/b。
    pub fn base() -> World {
        let (rname, rlen) = blank(b"");
        let (aname, alen) = blank(b"a");
        let (bname, blen) = blank(b"b");
        let mut dirs = Vec::new();
        dirs.push(Dir { id: 0, name: rname, nlen: rlen, parent: -1, rc: 0, kids: 1, removed: false, gone: false });
        dirs.push(Dir { id: 1, name: aname, nlen: alen, parent: 0, rc: 1, kids: 1, removed: false, gone: false });
        dirs.push(Dir { id: 2, name: bname, nlen: blen, parent: 1, rc: 1, kids: 0, removed: false, gone: false });
        let mut procs = Vec::new();
        procs.push(Proc { pid: 100, cwd: 1, alive: true });
        procs.push(Proc { pid: 101, cwd: 2, alive: true });
        World { dirs, procs, next_id: 3 }
    }

    pub fn dir(&self, id: u32) -> Option<&Dir> {
        self.dirs.get(id as usize)
    }

    fn dir_mut(&mut self, id: u32) -> Option<&mut Dir> {
        self.dirs.get_mut(id as usize)
    }

    pub fn live(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.dirs.len() {
            if let Some(d) = self.dirs.get(i) {
                if !d.gone {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    fn find_proc(&self, pid: u32) -> Result<&Proc, u32> {
        let mut i = 0usize;
        while i < self.procs.len() {
            if let Some(p) = self.procs.get(i) {
                if p.pid == pid {
                    if p.alive {
                        return Ok(p);
                    }
                    return Err(CT_ESRCH);
                }
            }
            i += 1;
        }
        Err(CT_ESRCH)
    }

    fn find_proc_mut(&mut self, pid: u32) -> Result<&mut Proc, u32> {
        let mut i = 0usize;
        while i < self.procs.len() {
            let hit = match self.procs.get(i) {
                Some(p) => p.pid == pid,
                None => false,
            };
            if hit {
                match self.procs.get_mut(i) {
                    Some(p) => {
                        if p.alive {
                            return Ok(p);
                        }
                        return Err(CT_ESRCH);
                    }
                    None => return Err(CT_ESRCH),
                }
            }
            i += 1;
        }
        Err(CT_ESRCH)
    }

    fn reap(&mut self, id: u32) {
        if let Some(d) = self.dir_mut(id) {
            if d.removed && d.rc == 0 {
                d.gone = true;
            }
        }
    }

    /// 逐段解析路径，返回目录 id。
    pub fn resolve(&self, pid: u32, path: &[u8]) -> Result<u32, u32> {
        let p = self.find_proc(pid)?;
        let absolute = path.first() == Some(&b'/');
        let mut cur = if absolute { 0u32 } else { p.cwd };
        let mut idx = 0usize;
        let mut steps = 0u32;
        while idx < path.len() {
            while idx < path.len() && path.get(idx) == Some(&b'/') {
                idx += 1;
            }
            if idx >= path.len() {
                break;
            }
            let start = idx;
            while idx < path.len() && path.get(idx) != Some(&b'/') {
                idx += 1;
            }
            let comp = match path.get(start..idx) {
                Some(c) => c,
                None => return Err(CT_ENOENT),
            };
            steps += 1;
            if steps > MAX_DEPTH {
                return Err(CT_ELOOP);
            }
            if comp == b"." {
                continue;
            }
            if comp == b".." {
                let d = match self.dirs.get(cur as usize) {
                    Some(v) => v,
                    None => return Err(CT_ENOENT),
                };
                if d.removed || d.gone {
                    return Err(CT_ENOENT);
                }
                if cur != 0 {
                    if d.parent < 0 {
                        return Err(CT_ENOENT);
                    }
                    cur = d.parent as u32;
                }
                continue;
            }
            let here = match self.dirs.get(cur as usize) {
                Some(v) => v,
                None => return Err(CT_ENOENT),
            };
            if here.removed || here.gone {
                return Err(CT_ENOENT);
            }
            let mut next = None;
            let mut i = 0usize;
            while i < self.dirs.len() {
                if let Some(k) = self.dirs.get(i) {
                    if k.parent == (cur as i32) && !k.removed && !k.gone && named(k, comp) {
                        next = Some(k.id);
                        break;
                    }
                }
                i += 1;
            }
            match next {
                Some(v) => cur = v,
                None => return Err(CT_ENOENT),
            }
        }
        Ok(cur)
    }

    /// 构造绝对路径。
    pub fn getcwd(&self, pid: u32) -> Result<Vec<u8>, u32> {
        let p = self.find_proc(pid)?;
        let mut rev: Vec<&[u8]> = Vec::new();
        let mut cur = p.cwd;
        let mut guard = 0u32;
        loop {
            let d = match self.dirs.get(cur as usize) {
                Some(v) => v,
                None => return Err(CT_ENOENT),
            };
            if d.removed || d.gone {
                return Err(CT_ENOENT);
            }
            if d.parent < 0 {
                break;
            }
            let n = d.nlen as usize;
            match d.name.get(0..n) {
                Some(s) => rev.push(s),
                None => return Err(CT_ENOENT),
            }
            cur = d.parent as u32;
            guard += 1;
            if guard > MAX_DEPTH {
                return Err(CT_ELOOP);
            }
        }
        let mut out = Vec::new();
        out.push(b'/');
        let mut i = rev.len();
        while i > 0 {
            i -= 1;
            if let Some(s) = rev.get(i) {
                let mut k = 0usize;
                while k < s.len() {
                    if let Some(&b) = s.get(k) {
                        out.push(b);
                    }
                    k += 1;
                }
                if i > 0 {
                    out.push(b'/');
                }
            }
        }
        Ok(out)
    }

    /// 在当前目录下创建子目录。
    pub fn mkdir(&mut self, pid: u32, name: &[u8]) -> Result<u32, u32> {
        if name.is_empty() || name.len() > NAME_CAP || name == b"." || name == b".." {
            return Err(CT_EINVAL);
        }
        if self.resolve(pid, name).is_ok() {
            return Err(CT_EEXIST);
        }
        let cwd = self.find_proc(pid)?.cwd;
        let here = match self.dirs.get(cwd as usize) {
            Some(v) => v,
            None => return Err(CT_ENOENT),
        };
        if here.removed || here.gone {
            return Err(CT_ENOENT);
        }
        let id = self.next_id;
        self.next_id += 1;
        let (nbuf, nlen) = blank(name);
        self.dirs.push(Dir {
            id,
            name: nbuf,
            nlen,
            parent: cwd as i32,
            rc: 0,
            kids: 0,
            removed: false,
            gone: false,
        });
        if let Some(d) = self.dir_mut(cwd) {
            d.kids += 1;
        }
        Ok(id)
    }

    /// 切换工作目录。
    pub fn chdir(&mut self, pid: u32, path: &[u8]) -> Result<(), u32> {
        let target = self.resolve(pid, path)?;
        let old = self.find_proc(pid)?.cwd;
        if let Some(d) = self.dir_mut(target) {
            if d.removed || d.gone {
                return Err(CT_ENOENT);
            }
            d.rc += 1;
        } else {
            return Err(CT_ENOENT);
        }
        if let Ok(p) = self.find_proc_mut(pid) {
            p.cwd = target;
        }
        if let Some(d) = self.dir_mut(old) {
            d.rc = d.rc.saturating_sub(1);
        }
        self.reap(old);
        Ok(())
    }

    /// 子进程继承父进程 cwd。
    pub fn fork(&mut self, parent: u32, child: u32) -> Result<(), u32> {
        let pcwd = self.find_proc(parent)?.cwd;
        if self.find_proc(child).is_ok() {
            return Err(CT_EEXIST);
        }
        if let Some(d) = self.dir_mut(pcwd) {
            d.rc += 1;
        } else {
            return Err(CT_ENOENT);
        }
        self.procs.push(Proc { pid: child, cwd: pcwd, alive: true });
        Ok(())
    }

    /// 进程退出，释放 cwd 引用。
    pub fn exit(&mut self, pid: u32) -> Result<(), u32> {
        let cwd = self.find_proc(pid)?.cwd;
        if let Ok(p) = self.find_proc_mut(pid) {
            p.alive = false;
        }
        if let Some(d) = self.dir_mut(cwd) {
            d.rc = d.rc.saturating_sub(1);
        }
        self.reap(cwd);
        Ok(())
    }

    /// 删除空目录；允许删除其他进程的 cwd。
    pub fn rmdir(&mut self, pid: u32, path: &[u8]) -> Result<u32, u32> {
        let target = self.resolve(pid, path)?;
        if target == 0 {
            return Err(CT_EBUSY);
        }
        let actor = self.find_proc(pid)?.cwd;
        let (parent, kids, rc) = match self.dirs.get(target as usize) {
            Some(d) => (d.parent, d.kids, d.rc),
            None => return Err(CT_ENOENT),
        };
        if target == actor {
            return Err(CT_EINVAL);
        }
        if kids > 0 {
            return Err(CT_ENOTEMPTY);
        }
        if let Some(d) = self.dir_mut(target) {
            d.removed = true;
            d.parent = -1;
            if rc == 0 {
                d.gone = true;
            }
        }
        if parent >= 0 {
            if let Some(p) = self.dir_mut(parent as u32) {
                p.kids = p.kids.saturating_sub(1);
            }
        }
        Ok(target)
    }

    /// 抓取某个进程的最终状态。
    pub fn state(&self, pid: u32) -> Result<Probe, u32> {
        let p = self.find_proc(pid)?;
        let d = match self.dirs.get(p.cwd as usize) {
            Some(v) => v,
            None => return Err(CT_ENOENT),
        };
        let dead = d.removed || d.gone;
        let mut path_len = 0u32;
        match self.getcwd(pid) {
            Ok(v) => path_len = v.len() as u32,
            Err(e) => {
                if !dead {
                    return Err(e);
                }
            }
        }
        Ok(Probe {
            pid,
            cwd: p.cwd as i32,
            rc: d.rc,
            removed: dead,
            live: self.live(),
            path_len,
        })
    }
}

/// 施加场景操作，返回要查询的 pid。
pub fn apply(w: &mut World, id: u8) -> Result<u32, u32> {
    match id {
        0 => Ok(100),
        1 => {
            w.fork(100, 102)?;
            Ok(100)
        }
        2 => {
            w.fork(100, 102)?;
            w.exit(102)?;
            Ok(100)
        }
        3 => {
            w.chdir(100, b"/a/b")?;
            Ok(100)
        }
        4 => {
            w.chdir(100, b"..")?;
            Ok(100)
        }
        5 => {
            w.chdir(100, b"../nope")?;
            Ok(100)
        }
        6 => {
            w.mkdir(100, b"new")?;
            w.chdir(100, b"new")?;
            Ok(100)
        }
        7 => {
            w.rmdir(100, b"b")?;
            Ok(101)
        }
        8 => {
            w.chdir(100, b"/a/b")?;
            w.rmdir(100, b".")?;
            Ok(100)
        }
        9 => {
            w.chdir(100, b"/")?;
            w.rmdir(100, b"a")?;
            Ok(100)
        }
        10 => {
            w.chdir(100, b"/")?;
            w.rmdir(100, b"/")?;
            Ok(100)
        }
        11 => {
            w.rmdir(100, b"b")?;
            w.exit(101)?;
            Ok(100)
        }
        12 => {
            w.chdir(100, b"/")?;
            w.rmdir(100, b"a/b")?;
            Ok(101)
        }
        13 => {
            w.chdir(100, b"/")?;
            w.rmdir(100, b"a/b")?;
            w.chdir(101, b"..")?;
            Ok(101)
        }
        _ => Err(CT_EID),
    }
}

/// 重建场景世界。
pub fn make_world(id: u8) -> Result<(World, u32), u32> {
    let mut w = World::base();
    let pid = apply(&mut w, id)?;
    Ok((w, pid))
}

/// 运行场景。
pub fn run_scen(id: u8) -> Result<Probe, u32> {
    let (w, pid) = make_world(id)?;
    w.state(pid)
}

/// 样本行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub scen: u8,
    pub expect_ok: bool,
    pub pid: u32,
    pub cwd: i32,
    pub rc: u32,
    pub removed: bool,
    pub live: u32,
    pub path_len: u32,
    pub code: u16,
}

/// 实际结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    pub pid: u32,
    pub cwd: i32,
    pub rc: u32,
    pub removed: bool,
    pub live: u32,
    pub path_len: u32,
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

    pub fn register(
        &mut self,
        slot: u8,
        scen: u8,
        ok: bool,
        pid: u32,
        cwd: i32,
        rc: u32,
        removed: bool,
        live: u32,
        path_len: u32,
        code: u16,
    ) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(CT_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(CT_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(CT_EDUP);
        }
        if (slot as usize) != self.samples.len() {
            self.rejects += 1;
            return Err(CT_EORDER);
        }
        let s = Sample { scen, expect_ok: ok, pid, cwd, rc, removed, live, path_len, code };
        let out = match run_scen(scen) {
            Ok(v) => {
                if !ok
                    || v.pid != pid
                    || v.cwd != cwd
                    || v.rc != rc
                    || v.removed != removed
                    || v.live != live
                    || v.path_len != path_len
                {
                    return Err(CT_EMISMATCH);
                }
                Outcome {
                    ok: true,
                    pid: v.pid,
                    cwd: v.cwd,
                    rc: v.rc,
                    removed: v.removed,
                    live: v.live,
                    path_len: v.path_len,
                    code: 0,
                }
            }
            Err(e) => {
                let low = (e & 0xffff) as u16;
                if ok || low != code {
                    return Err(CT_EMISMATCH);
                }
                Outcome {
                    ok: false,
                    pid: 0,
                    cwd: -1,
                    rc: 0,
                    removed: false,
                    live: 0,
                    path_len: 0,
                    code: low,
                }
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
            return Err(CT_EBAL);
        }
        let mut i = 0;
        while i < self.samples.len() {
            let s = match self.samples.get(i) {
                Some(v) => *v,
                None => return Err(CT_ENOBS),
            };
            let o = match self.outcomes.get(i) {
                Some(v) => *v,
                None => return Err(CT_ENOBS),
            };
            if s.expect_ok != o.ok {
                return Err(CT_EMISMATCH);
            }
            match run_scen(s.scen) {
                Ok(v) => {
                    if !o.ok
                        || v.pid != o.pid
                        || v.cwd != o.cwd
                        || v.rc != o.rc
                        || v.removed != o.removed
                        || v.live != o.live
                        || v.path_len != o.path_len
                    {
                        return Err(CT_EMISMATCH);
                    }
                    if v.cwd < 0 || v.live > 8 {
                        return Err(CT_EBAL);
                    }
                    if v.removed && v.path_len != 0 {
                        return Err(CT_EBAL);
                    }
                }
                Err(e) => {
                    let low = (e & 0xffff) as u16;
                    if o.ok || low != o.code {
                        return Err(CT_EMISMATCH);
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
            return Err(CT_EFROZEN);
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
                h = fold(h, o.pid as u64);
                h = fold(h, o.cwd as u64);
                h = fold(h, o.rc as u64);
                h = fold(h, u64::from(o.removed));
                h = fold(h, o.live as u64);
                h = fold(h, o.path_len as u64);
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
        if fresh.seal() == self.seal()
            && fresh.ok_count() == self.ok_count()
            && fresh.err_count() == self.err_count()
        {
            Ok(fresh.seal())
        } else {
            Err(CT_ESEAL)
        }
    }
}

/// 十四行账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let enoent = (CT_ENOENT & 0xffff) as u16;
    let einval = (CT_EINVAL & 0xffff) as u16;
    let enotempty = (CT_ENOTEMPTY & 0xffff) as u16;
    let ebusy = (CT_EBUSY & 0xffff) as u16;
    let _ = l.register(0, 0, true, 100, 1, 1, false, 3, 2, 0);
    let _ = l.register(1, 1, true, 100, 1, 2, false, 3, 2, 0);
    let _ = l.register(2, 2, true, 100, 1, 1, false, 3, 2, 0);
    let _ = l.register(3, 3, true, 100, 2, 2, false, 3, 4, 0);
    let _ = l.register(4, 4, true, 100, 0, 1, false, 3, 1, 0);
    let _ = l.register(5, 5, false, 0, -1, 0, false, 0, 0, enoent);
    let _ = l.register(6, 6, true, 100, 3, 1, false, 4, 6, 0);
    let _ = l.register(7, 7, true, 101, 2, 1, true, 3, 0, 0);
    let _ = l.register(8, 8, false, 0, -1, 0, false, 0, 0, einval);
    let _ = l.register(9, 9, false, 0, -1, 0, false, 0, 0, enotempty);
    let _ = l.register(10, 10, false, 0, -1, 0, false, 0, 0, ebusy);
    let _ = l.register(11, 11, true, 100, 1, 1, false, 2, 2, 0);
    let _ = l.register(12, 12, true, 101, 2, 1, true, 3, 0, 0);
    let _ = l.register(13, 13, false, 0, -1, 0, false, 0, 0, enoent);
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
    let twice = l.freeze() == Err(CT_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && twice;

    let fork_rc = match make_world(1) {
        Ok((w, _)) => w.dir(1).map(|d| d.rc) == Some(2),
        Err(_) => false,
    };
    let exit_rc = match make_world(2) {
        Ok((w, _)) => w.dir(1).map(|d| d.rc) == Some(1),
        Err(_) => false,
    };
    let moved = match make_world(3) {
        Ok((w, _)) => {
            w.dir(1).map(|d| d.rc) == Some(0)
                && w.dir(2).map(|d| d.rc) == Some(2)
        }
        Err(_) => false,
    };
    out[1] = fork_rc && exit_rc && moved;

    let mut held = false;
    let mut reaped = false;
    match make_world(7) {
        Ok((mut w, pid)) => {
            let gone_cwd = w.getcwd(101) == Err(CT_ENOENT);
            let marked = w.dir(2).map(|d| (d.rc, d.removed)) == Some((1, true));
            let r = w.exit(101);
            held = pid == 101 && gone_cwd && marked;
            reaped = r.is_ok() && w.live() == 2 && w.dir(2).map(|d| d.gone) == Some(true);
        }
        Err(_) => {}
    }
    out[2] = held && reaped;

    out[3] = run_scen(5) == Err(CT_ENOENT)
        && run_scen(8) == Err(CT_EINVAL)
        && run_scen(9) == Err(CT_ENOTEMPTY)
        && run_scen(10) == Err(CT_EBUSY);
    out
}

/// 十三项判据。
pub fn run_unx11480_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11480_cwd_inherit");
    let l = build_ledger();
    s.add(
        "CT1-十四行齐备",
        l.len() == SAMPLES && l.sample(0).map(|x| x.scen) == Some(0),
        "static",
    );
    s.add_dyn(
        "CT2-九应五拒",
        l.ok_count() == 9 && l.err_count() == 5 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );
    s.add(
        "CT3-fork 继承并加引用",
        match make_world(1) {
            Ok((w, _)) => w.dir(1).map(|d| d.rc) == Some(2) && w.procs.len() == 3,
            Err(_) => false,
        },
        "static",
    );
    s.add(
        "CT4-exit 释放引用",
        match make_world(2) {
            Ok((w, _)) => w.dir(1).map(|d| d.rc) == Some(1),
            Err(_) => false,
        },
        "static",
    );
    s.add(
        "CT5-chdir 迁移引用",
        match make_world(3) {
            Ok((w, _)) => {
                w.dir(1).map(|d| d.rc) == Some(0)
                    && w.dir(2).map(|d| d.rc) == Some(2)
                    && w.state(100).map(|p| p.cwd) == Ok(2)
            }
            Err(_) => false,
        },
        "static",
    );
    s.add(
        "CT6-根目录上溯钳制",
        run_scen(4).map(|p| (p.cwd, p.path_len)) == Ok((0, 1)),
        "static",
    );
    s.add(
        "CT7-新建并进入子目录",
        run_scen(6).map(|p| (p.cwd, p.live, p.path_len)) == Ok((3, 4, 6)),
        "static",
    );
    s.add(
        "CT8-删除他人 cwd 合法",
        match make_world(7) {
            Ok((w, _)) => {
                w.dir(2).map(|d| (d.rc, d.removed, d.parent)) == Some((1, true, -1))
                    && w.getcwd(101) == Err(CT_ENOENT)
                    && w.state(101).map(|p| (p.removed, p.path_len)) == Ok((true, 0))
            }
            Err(_) => false,
        },
        "static",
    );
    s.add(
        "CT9-删除自身 cwd 拒绝",
        run_scen(8) == Err(CT_EINVAL),
        "static",
    );
    s.add(
        "CT10-非空目录拒绝",
        run_scen(9) == Err(CT_ENOTEMPTY),
        "static",
    );
    s.add(
        "CT11-根目录不可删除",
        run_scen(10) == Err(CT_EBUSY),
        "static",
    );
    s.add(
        "CT12-最后引用释放后回收",
        match make_world(11) {
            Ok((w, _)) => {
                w.live() == 2
                    && w.dir(2).map(|d| d.gone) == Some(true)
                    && w.dir(1).map(|d| d.rc) == Some(1)
            }
            Err(_) => false,
        },
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(CT_EFROZEN);
    s.add_dyn(
        "CT13-冻结重放与四路",
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
    fn fork_shares_cwd_reference() {
        let mut w = World::base();
        assert!(w.fork(100, 102).is_ok());
        assert_eq!(w.dir(1).map(|d| d.rc), Some(2));
        assert_eq!(w.procs.len(), 3);
    }

    #[test]
    fn exit_drops_reference() {
        let mut w = World::base();
        assert!(w.fork(100, 102).is_ok());
        assert!(w.exit(102).is_ok());
        assert_eq!(w.dir(1).map(|d| d.rc), Some(1));
    }

    #[test]
    fn chdir_moves_reference() {
        let mut w = World::base();
        assert!(w.chdir(100, b"/a/b").is_ok());
        assert_eq!(w.dir(2).map(|d| d.rc), Some(2));
        assert_eq!(w.dir(1).map(|d| d.rc), Some(0));
    }

    #[test]
    fn parent_of_root_stays_root() {
        let mut w = World::base();
        assert!(w.chdir(100, b"..").is_ok());
        assert_eq!(w.state(100).map(|p| (p.cwd, p.path_len)), Ok((0, 1)));
    }

    #[test]
    fn removing_foreign_cwd_keeps_reference() {
        let mut w = World::base();
        assert_eq!(w.rmdir(100, b"b"), Ok(2));
        assert_eq!(w.getcwd(101), Err(CT_ENOENT));
        assert_eq!(w.dir(2).map(|d| (d.rc, d.removed)), Some((1, true)));
        assert!(w.exit(101).is_ok());
        assert_eq!(w.dir(2).map(|d| d.gone), Some(true));
    }

    #[test]
    fn cannot_remove_own_cwd_or_root() {
        let mut w = World::base();
        assert_eq!(w.rmdir(100, b"."), Err(CT_EINVAL));
        assert!(w.chdir(100, b"/").is_ok());
        assert_eq!(w.rmdir(100, b"/"), Err(CT_EBUSY));
        assert_eq!(w.rmdir(100, b"a"), Err(CT_ENOTEMPTY));
    }

    #[test]
    fn ledger_is_consistent() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 9);
        assert_eq!(l.err_count(), 5);
        assert!(l.audit().is_ok());
        let s = run_unx11480_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
    }
}
