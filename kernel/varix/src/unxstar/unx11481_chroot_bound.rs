//! UNX-F11481 chroot 边界与目录逃逸（0x20de0 段）
//!
//! `chroot` 只改变进程的根目录指针，并不改变当前工作目录。若调用前 cwd
//! 位于新根之外，调用后进程仍然合法地停在旧位置上，这就是经典的 chroot
//! 逃逸起点：从外部 cwd 出发的相对路径 `..` 可以继续向上，甚至重新进入
//! 被隔离的子树；绝对路径则一律从新根开始，无法看到根外的兄弟目录。
//!
//! 路径解析从 cwd（相对）或 root（绝对）出发，逐段处理 `.` 与 `..`。当
//! 当前节点恰好等于进程根时，`..` 必须钳制在根上；当当前节点在根外时，
//! `..` 按真实父链接上升，到真实文件系统根后自然停住。判定“是否在根内”
//! 不看字符串前缀，而是沿父链上溯，只有真正经过根节点才算包含；因此
//! `/srv/jail` 之外的 `/srv` 与 `/secret` 都被判为逃逸。
//!
//! 不变量：`cwd` 与 `root` 永远是有效目录；`contained` 为真等价于从 cwd
//! 沿父链可达 root；cwd 等于 root 时一定被包含；深度按真实父链计算，
//! 不受根截断影响；绝对路径的每一段都在根内查找。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20de0;
pub const CR_EID: u32 = SEG_BASE + 0x01;
pub const CR_EORDER: u32 = SEG_BASE + 0x02;
pub const CR_EDUP: u32 = SEG_BASE + 0x03;
pub const CR_EFROZEN: u32 = SEG_BASE + 0x04;
pub const CR_ENOBS: u32 = SEG_BASE + 0x05;
pub const CR_EMISMATCH: u32 = SEG_BASE + 0x06;
pub const CR_EBAL: u32 = SEG_BASE + 0x07;
pub const CR_ESEAL: u32 = SEG_BASE + 0x08;
pub const CR_ENOENT: u32 = SEG_BASE + 0x09;
pub const CR_ENOTDIR: u32 = SEG_BASE + 0x0a;
pub const CR_EPERM: u32 = SEG_BASE + 0x0b;
pub const CR_ELOOP: u32 = SEG_BASE + 0x0c;

pub const SAMPLES: usize = 14;
pub const NAME_CAP: usize = 8;
pub const MAX_DEPTH: u32 = 32;
pub const PID: u32 = 200;

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
pub struct Node {
    pub id: u32,
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
    pub parent: i32,
}

/// 进程根与 cwd。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PState {
    pub pid: u32,
    pub root: u32,
    pub cwd: u32,
}

/// 世界。
pub struct World {
    pub nodes: Vec<Node>,
    pub proc: PState,
}

/// 状态快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Probe {
    pub pid: u32,
    pub root: i32,
    pub cwd: i32,
    pub contained: bool,
    pub depth: u32,
    pub path_len: u32,
}

fn mknode(id: u32, name: &[u8], parent: i32) -> Node {
    let mut buf = [0u8; NAME_CAP];
    let n = if name.len() > NAME_CAP { NAME_CAP } else { name.len() };
    let mut i = 0usize;
    while i < n {
        if let Some(&b) = name.get(i) {
            buf[i] = b;
        }
        i += 1;
    }
    Node { id, name: buf, nlen: n as u8, parent }
}

impl World {
    /// 根、/srv、/srv/jail、/srv/jail/etc、/secret。
    pub fn base() -> World {
        let mut nodes = Vec::new();
        nodes.push(mknode(0, b"", -1));
        nodes.push(mknode(1, b"srv", 0));
        nodes.push(mknode(2, b"jail", 1));
        nodes.push(mknode(3, b"etc", 2));
        nodes.push(mknode(4, b"secret", 0));
        World { nodes, proc: PState { pid: PID, root: 0, cwd: 0 } }
    }

    pub fn node(&self, id: u32) -> Option<&Node> {
        self.nodes.get(id as usize)
    }

    pub fn depth(&self, id: u32) -> u32 {
        let mut cur = id;
        let mut n = 0u32;
        loop {
            let d = match self.nodes.get(cur as usize) {
                Some(v) => v,
                None => return n,
            };
            if d.parent < 0 {
                return n;
            }
            cur = d.parent as u32;
            n += 1;
            if n > MAX_DEPTH {
                return n;
            }
        }
    }

    /// 沿父链判断节点是否真正位于进程根之下。
    pub fn contained(&self, id: u32) -> bool {
        let mut cur = id;
        let mut guard = 0u32;
        loop {
            if cur == self.proc.root {
                return true;
            }
            let d = match self.nodes.get(cur as usize) {
                Some(v) => v,
                None => return false,
            };
            if d.parent < 0 {
                return false;
            }
            cur = d.parent as u32;
            guard += 1;
            if guard > MAX_DEPTH {
                return false;
            }
        }
    }

    fn child_of(&self, id: u32, comp: &[u8]) -> Option<u32> {
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                if n.parent == (id as i32)
                    && (n.nlen as usize) == comp.len()
                    && n.name.get(0..comp.len()).map(|s| s == comp).unwrap_or(false)
                {
                    return Some(n.id);
                }
            }
            i += 1;
        }
        None
    }

    /// 按进程根与 cwd 解析路径。
    pub fn resolve(&self, path: &[u8]) -> Result<u32, u32> {
        let absolute = path.first() == Some(&b'/');
        let mut cur = if absolute { self.proc.root } else { self.proc.cwd };
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
                None => return Err(CR_ENOENT),
            };
            steps += 1;
            if steps > MAX_DEPTH {
                return Err(CR_ELOOP);
            }
            if comp == b"." {
                continue;
            }
            if comp == b".." {
                if cur == self.proc.root {
                    continue;
                }
                let d = match self.nodes.get(cur as usize) {
                    Some(v) => v,
                    None => return Err(CR_ENOENT),
                };
                if d.parent >= 0 {
                    cur = d.parent as u32;
                }
                continue;
            }
            match self.child_of(cur, comp) {
                Some(v) => cur = v,
                None => return Err(CR_ENOENT),
            }
        }
        Ok(cur)
    }

    /// 真实绝对路径。
    pub fn getcwd(&self) -> Result<Vec<u8>, u32> {
        let mut rev: Vec<&[u8]> = Vec::new();
        let mut cur = self.proc.cwd;
        let mut guard = 0u32;
        loop {
            let d = match self.nodes.get(cur as usize) {
                Some(v) => v,
                None => return Err(CR_ENOENT),
            };
            if d.parent < 0 {
                break;
            }
            let n = d.nlen as usize;
            match d.name.get(0..n) {
                Some(s) => rev.push(s),
                None => return Err(CR_ENOENT),
            }
            cur = d.parent as u32;
            guard += 1;
            if guard > MAX_DEPTH {
                return Err(CR_ELOOP);
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

    /// 切换 cwd。
    pub fn chdir(&mut self, path: &[u8]) -> Result<(), u32> {
        let target = self.resolve(path)?;
        if self.nodes.get(target as usize).is_none() {
            return Err(CR_ENOENT);
        }
        self.proc.cwd = target;
        Ok(())
    }

    /// 改变根目录，cwd 保持不动。
    pub fn chroot(&mut self, path: &[u8]) -> Result<(), u32> {
        let target = self.resolve(path)?;
        if self.nodes.get(target as usize).is_none() {
            return Err(CR_ENOENT);
        }
        if target == 0 {
            return Err(CR_EPERM);
        }
        self.proc.root = target;
        Ok(())
    }

    /// 状态快照。
    pub fn state(&self) -> Result<Probe, u32> {
        let cwd = self.proc.cwd;
        if self.nodes.get(cwd as usize).is_none() {
            return Err(CR_ENOENT);
        }
        let path = self.getcwd()?;
        Ok(Probe {
            pid: self.proc.pid,
            root: self.proc.root as i32,
            cwd: cwd as i32,
            contained: self.contained(cwd),
            depth: self.depth(cwd),
            path_len: path.len() as u32,
        })
    }
}

/// 施加场景。
pub fn apply(w: &mut World, id: u8) -> Result<(), u32> {
    match id {
        0 => Ok(()),
        1 => w.chdir(b"srv/jail"),
        2 => w.chdir(b"/srv/jail/etc"),
        3 => w.chroot(b"/srv/jail"),
        4 => {
            w.chdir(b"/srv")?;
            w.chroot(b"/srv/jail")?;
            w.chdir(b"..")?;
            w.chdir(b"secret")
        }
        5 => {
            w.chroot(b"/srv/jail")?;
            w.chdir(b"/")
        }
        6 => {
            w.chroot(b"/srv/jail")?;
            w.chdir(b"/etc")
        }
        7 => {
            w.chroot(b"/srv/jail")?;
            w.chdir(b"/../..")
        }
        8 => w.chroot(b"/nope"),
        9 => {
            w.chroot(b"/srv/jail")?;
            w.chdir(b"srv")
        }
        10 => {
            w.chroot(b"/srv/jail")?;
            w.chdir(b"srv/jail/etc")
        }
        11 => {
            w.chroot(b"/srv/jail")?;
            w.chdir(b"/etc")?;
            w.chdir(b"../..")
        }
        12 => {
            w.chdir(b"/srv/jail")?;
            w.chroot(b".")?;
            w.chdir(b"..")
        }
        13 => {
            w.chroot(b"/srv/jail")?;
            w.chdir(b"/")?;
            w.chdir(b"../../secret")
        }
        _ => Err(CR_EID),
    }
}

/// 重建场景世界。
pub fn make_world(id: u8) -> Result<World, u32> {
    let mut w = World::base();
    apply(&mut w, id)?;
    Ok(w)
}

/// 运行场景。
pub fn run_scen(id: u8) -> Result<Probe, u32> {
    match make_world(id) {
        Ok(w) => w.state(),
        Err(e) => Err(e),
    }
}

/// 样本行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub scen: u8,
    pub expect_ok: bool,
    pub pid: u32,
    pub root: i32,
    pub cwd: i32,
    pub contained: bool,
    pub depth: u32,
    pub path_len: u32,
    pub code: u16,
}

/// 实际结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    pub pid: u32,
    pub root: i32,
    pub cwd: i32,
    pub contained: bool,
    pub depth: u32,
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
        root: i32,
        cwd: i32,
        contained: bool,
        depth: u32,
        path_len: u32,
        code: u16,
    ) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(CR_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(CR_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(CR_EDUP);
        }
        if (slot as usize) != self.samples.len() {
            self.rejects += 1;
            return Err(CR_EORDER);
        }
        let s = Sample { scen, expect_ok: ok, pid, root, cwd, contained, depth, path_len, code };
        let out = match run_scen(scen) {
            Ok(v) => {
                if !ok
                    || v.pid != pid
                    || v.root != root
                    || v.cwd != cwd
                    || v.contained != contained
                    || v.depth != depth
                    || v.path_len != path_len
                {
                    return Err(CR_EMISMATCH);
                }
                Outcome {
                    ok: true,
                    pid: v.pid,
                    root: v.root,
                    cwd: v.cwd,
                    contained: v.contained,
                    depth: v.depth,
                    path_len: v.path_len,
                    code: 0,
                }
            }
            Err(e) => {
                let low = (e & 0xffff) as u16;
                if ok || low != code {
                    return Err(CR_EMISMATCH);
                }
                Outcome {
                    ok: false,
                    pid: 0,
                    root: -1,
                    cwd: -1,
                    contained: false,
                    depth: 0,
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
            return Err(CR_EBAL);
        }
        let mut i = 0;
        while i < self.samples.len() {
            let s = match self.samples.get(i) {
                Some(v) => *v,
                None => return Err(CR_ENOBS),
            };
            let o = match self.outcomes.get(i) {
                Some(v) => *v,
                None => return Err(CR_ENOBS),
            };
            if s.expect_ok != o.ok {
                return Err(CR_EMISMATCH);
            }
            match run_scen(s.scen) {
                Ok(v) => {
                    if !o.ok
                        || v.pid != o.pid
                        || v.root != o.root
                        || v.cwd != o.cwd
                        || v.contained != o.contained
                        || v.depth != o.depth
                        || v.path_len != o.path_len
                    {
                        return Err(CR_EMISMATCH);
                    }
                    if v.root < 0 || v.cwd < 0 || v.depth > MAX_DEPTH {
                        return Err(CR_EBAL);
                    }
                    if v.cwd == v.root && !v.contained {
                        return Err(CR_EBAL);
                    }
                }
                Err(e) => {
                    let low = (e & 0xffff) as u16;
                    if o.ok || low != o.code {
                        return Err(CR_EMISMATCH);
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
            return Err(CR_EFROZEN);
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
                h = fold(h, o.root as u64);
                h = fold(h, o.cwd as u64);
                h = fold(h, u64::from(o.contained));
                h = fold(h, o.depth as u64);
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
            Err(CR_ESEAL)
        }
    }
}

/// 十四行账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let enoent = (CR_ENOENT & 0xffff) as u16;
    let _ = l.register(0, 0, true, PID, 0, 0, true, 0, 1, 0);
    let _ = l.register(1, 1, true, PID, 0, 2, true, 2, 9, 0);
    let _ = l.register(2, 2, true, PID, 0, 3, true, 3, 13, 0);
    let _ = l.register(3, 3, true, PID, 2, 0, false, 0, 1, 0);
    let _ = l.register(4, 4, true, PID, 2, 4, false, 1, 7, 0);
    let _ = l.register(5, 5, true, PID, 2, 2, true, 2, 9, 0);
    let _ = l.register(6, 6, true, PID, 2, 3, true, 3, 13, 0);
    let _ = l.register(7, 7, true, PID, 2, 2, true, 2, 9, 0);
    let _ = l.register(8, 8, false, 0, -1, -1, false, 0, 0, enoent);
    let _ = l.register(9, 9, true, PID, 2, 1, false, 1, 4, 0);
    let _ = l.register(10, 10, true, PID, 2, 3, true, 3, 13, 0);
    let _ = l.register(11, 11, true, PID, 2, 2, true, 2, 9, 0);
    let _ = l.register(12, 12, true, PID, 2, 2, true, 2, 9, 0);
    let _ = l.register(13, 13, false, 0, -1, -1, false, 0, 0, enoent);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.ok_count() == 12 && l.err_count() == 2 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let twice = l.freeze() == Err(CR_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && twice;

    let kept = match make_world(3) {
        Ok(w) => (w.proc.root, w.proc.cwd, w.contained(w.proc.cwd)) == (2, 0, false),
        Err(_) => false,
    };
    let reentered = match make_world(3) {
        Ok(mut w) => w.chdir(b"/").is_ok() && w.contained(w.proc.cwd),
        Err(_) => false,
    };
    out[1] = kept && reentered;

    let escaped = match make_world(4) {
        Ok(w) => (w.proc.root, w.proc.cwd, w.contained(w.proc.cwd)) == (2, 4, false),
        Err(_) => false,
    };
    let recaptured = match make_world(4) {
        Ok(mut w) => w.chdir(b"/").is_ok() && w.proc.cwd == 2 && w.contained(2),
        Err(_) => false,
    };
    out[2] = escaped && recaptured;

    let inside = match make_world(11) {
        Ok(w) => w.proc.cwd == 2 && w.contained(2),
        Err(_) => false,
    };
    let outside = match make_world(10) {
        Ok(w) => w.proc.cwd == 3 && w.contained(3),
        Err(_) => false,
    };
    out[3] = run_scen(8) == Err(CR_ENOENT)
        && run_scen(13) == Err(CR_ENOENT)
        && inside
        && outside;
    out
}

/// 十三项判据。
pub fn run_unx11481_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11481_chroot_bound");
    let l = build_ledger();
    s.add(
        "CR1-十四行齐备",
        l.len() == SAMPLES && l.sample(0).map(|x| x.scen) == Some(0),
        "static",
    );
    s.add_dyn(
        "CR2-十二应两拒",
        l.ok_count() == 12 && l.err_count() == 2 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );
    s.add(
        "CR3-基准根内",
        run_scen(0).map(|p| (p.root, p.cwd, p.contained)) == Ok((0, 0, true)),
        "static",
    );
    s.add(
        "CR4-相对进入隔离树",
        run_scen(1).map(|p| (p.cwd, p.depth, p.contained)) == Ok((2, 2, true)),
        "static",
    );
    s.add(
        "CR5-chroot 不改变 cwd",
        run_scen(3).map(|p| (p.root, p.cwd, p.contained, p.path_len)) == Ok((2, 0, false, 1)),
        "static",
    );
    s.add(
        "CR6-外部 cwd 可逃逸",
        run_scen(4).map(|p| (p.root, p.cwd, p.contained, p.path_len)) == Ok((2, 4, false, 7)),
        "static",
    );
    s.add(
        "CR7-绝对路径重新进入",
        run_scen(5).map(|p| (p.cwd, p.contained)) == Ok((2, true)),
        "static",
    );
    s.add(
        "CR8-根内绝对解析",
        run_scen(6).map(|p| (p.cwd, p.depth, p.contained)) == Ok((3, 3, true)),
        "static",
    );
    s.add(
        "CR9-绝对上溯在根处钳制",
        run_scen(7).map(|p| (p.cwd, p.contained)) == Ok((2, true)),
        "static",
    );
    s.add(
        "CR10-无效 chroot 路径",
        run_scen(8) == Err(CR_ENOENT),
        "static",
    );
    s.add(
        "CR11-外部相对路径可见兄弟",
        run_scen(9).map(|p| (p.cwd, p.contained, p.path_len)) == Ok((1, false, 4)),
        "static",
    );
    s.add(
        "CR12-根内上溯无法逃逸",
        run_scen(11).map(|p| (p.cwd, p.contained)) == Ok((2, true))
            && run_scen(12).map(|p| (p.root, p.cwd, p.contained)) == Ok((2, 2, true)),
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(CR_EFROZEN);
    s.add_dyn(
        "CR13-冻结重放与四路",
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
    fn chroot_keeps_cwd_outside() {
        let w = make_world(3);
        assert!(w.is_ok());
        match w {
            Ok(v) => {
                assert_eq!((v.proc.root, v.proc.cwd), (2, 0));
                assert!(!v.contained(v.proc.cwd));
            }
            Err(_) => {}
        }
    }

    #[test]
    fn absolute_path_enters_new_root() {
        let mut w = World::base();
        assert!(w.chroot(b"/srv/jail").is_ok());
        assert!(w.chdir(b"/").is_ok());
        assert_eq!(w.proc.cwd, 2);
        assert!(w.contained(2));
    }

    #[test]
    fn outside_cwd_can_walk_away() {
        let w = make_world(4);
        match w {
            Ok(v) => {
                assert_eq!((v.proc.root, v.proc.cwd), (2, 4));
                assert!(!v.contained(4));
            }
            Err(_) => {}
        }
    }

    #[test]
    fn dotdot_clamps_at_root() {
        let mut w = World::base();
        assert!(w.chroot(b"/srv/jail").is_ok());
        assert!(w.chdir(b"/").is_ok());
        assert!(w.chdir(b"../..").is_ok());
        assert_eq!(w.proc.cwd, 2);
    }

    #[test]
    fn invalid_chroot_is_enoent() {
        let mut w = World::base();
        assert_eq!(w.chroot(b"/nope"), Err(CR_ENOENT));
        assert_eq!(w.proc.root, 0);
    }

    #[test]
    fn cannot_escape_from_inside() {
        let w = make_world(11);
        match w {
            Ok(v) => {
                assert_eq!(v.proc.cwd, 2);
                assert!(v.contained(2));
            }
            Err(_) => {}
        }
    }

    #[test]
    fn ledger_is_consistent() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 12);
        assert_eq!(l.err_count(), 2);
        assert!(l.audit().is_ok());
        let s = run_unx11481_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
    }
}
