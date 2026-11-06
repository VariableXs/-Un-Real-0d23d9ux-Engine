//! UNX-F11356 procfs 挂载与可见性语义（0xec40 段）
//!
//! procfs 能被看见的前提有两层：**挂载**决定命名空间里有没有这棵树，**hidepid**
//! 决定同一棵树里谁能看见谁。诊断工具经常把这两层混为一谈，于是把「没挂上」报成
//! 「进程不存在」，或者把「被隐藏」当成「真的没有了」。
//!
//! 本模块把两层分开：
//!
//! - 没有挂载：`ENOMOUNT`——这棵 procfs 在目标命名空间里根本不存在；
//! - 有挂载但进程号不存在：`ENOENT`——树在，条目不在；
//! - `hidepid=1`：他人条目**目录可见、stat 拒绝**（`EHIDDEN`）——`ps` 能数到，
//!   但读不到细节；
//! - `hidepid=2`：他人条目**完全不可见**（`EINVIS`）——连目录都不出现。
//!
//! 自己的条目在任何 hidepid 下都完全可见；枚举只统计可见条目，因此 `hidepid=2`
//! 会让同一棵树对他人「少几条」。
//!
//! 卸载规则：命名空间里还有进程条目时卸载是忙的（`EBUSY`），必须先 `reap` 掉
//! 所有条目。这条约束保证「卸载成功」在语义上等价于「这棵树确实空到可以消失」。
//!
//! 闸门次序：
//! - `mount`：命名空间为零或 hidepid 越界 → 重复 → 表满；
//! - `proc`：零号 → 重复 → 表满；
//! - `lookup`：未知号 → 未挂载 → 隐藏；
//! - `stat`：未知号 → 未挂载 → 不可见 → 细节被藏；
//! - `umount`：未挂载 → 仍有条目。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xec40;

/// 参数非法。
pub const PV_EINVAL: u32 = SEG_BASE + 0x01;
/// 重复挂载或重复进程号。
pub const PV_EDUP: u32 = SEG_BASE + 0x02;
/// 表满。
pub const PV_EFULL: u32 = SEG_BASE + 0x03;
/// 进程号不存在。
pub const PV_ENOENT: u32 = SEG_BASE + 0x04;
/// 命名空间没有挂载。
pub const PV_ENOMOUNT: u32 = SEG_BASE + 0x05;
/// 目录可见但细节被藏。
pub const PV_EHIDDEN: u32 = SEG_BASE + 0x06;
/// 条目完全不可见。
pub const PV_EINVIS: u32 = SEG_BASE + 0x07;
/// 卸载时仍有条目。
pub const PV_EBUSY: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const PV_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const PV_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const PV_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const PV_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const PV_ENOBS: u32 = SEG_BASE + 0x0d;

/// 挂载表容量。
pub const MCAP: usize = 6;
/// 进程表容量。
pub const PCAP: usize = 8;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// hidepid：全部可见。
pub const HIDE_NONE: u8 = 0;
/// hidepid：他人目录可见、细节被藏。
pub const HIDE_STAT: u8 = 1;
/// hidepid：他人条目完全不可见。
pub const HIDE_ALL: u8 = 2;

/// 操作码：挂载。
pub const OP_MOUNT: u8 = 1;
/// 操作码：登记进程。
pub const OP_PROC: u8 = 2;
/// 操作码：解析路径。
pub const OP_LOOKUP: u8 = 3;
/// 操作码：读细节。
pub const OP_STAT: u8 = 4;
/// 操作码：枚举。
pub const OP_ENUM: u8 = 5;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个 64 位字。
pub fn fold(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut n = v;
    let mut i = 0u32;
    while i < 8 {
        x ^= n & 0xff;
        x = x.wrapping_mul(FNV_PRIME);
        n >>= 8;
        i += 1;
    }
    x
}

/// 一次挂载。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mount {
    /// 命名空间。
    pub ns: u32,
    /// hidepid 级别。
    pub hidepid: u8,
}

/// 一个进程条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Proc {
    /// 进程号。
    pub pid: u32,
    /// 属主用户。
    pub uid: u32,
    /// 所在命名空间。
    pub ns: u32,
}

/// procfs 可见性模型。
#[derive(Clone, Copy, Debug)]
pub struct Procs {
    m: [Mount; MCAP],
    mn: usize,
    p: [Proc; PCAP],
    pn: usize,
    looks: usize,
    hides: usize,
    reaps: usize,
}

impl Procs {
    /// 空模型。
    pub const fn new() -> Procs {
        Procs {
            m: [Mount { ns: 0, hidepid: 0 }; MCAP],
            mn: 0,
            p: [Proc { pid: 0, uid: 0, ns: 0 }; PCAP],
            pn: 0,
            looks: 0,
            hides: 0,
            reaps: 0,
        }
    }

    /// 挂载条数。
    pub fn mount_count(&self) -> usize {
        self.mn
    }

    /// 进程条数。
    pub fn proc_count(&self) -> usize {
        self.pn
    }

    /// 解析次数。
    pub fn looks(&self) -> usize {
        self.looks
    }

    /// 因隐藏被拒次数。
    pub fn hides(&self) -> usize {
        self.hides
    }

    /// 回收次数。
    pub fn reaps(&self) -> usize {
        self.reaps
    }

    /// 第 i 个挂载。
    pub fn mount_at(&self, i: usize) -> Option<Mount> {
        if i >= self.mn {
            return None;
        }
        self.m.get(i).copied()
    }

    /// 第 i 个进程。
    pub fn proc_at(&self, i: usize) -> Option<Proc> {
        if i >= self.pn {
            return None;
        }
        self.p.get(i).copied()
    }

    /// 按命名空间找挂载。
    fn find_mount(&self, ns: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.mn {
            match self.m.get(i) {
                Some(x) => {
                    if x.ns == ns {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 按号找进程。
    fn find_proc(&self, pid: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.pn {
            match self.p.get(i) {
                Some(x) => {
                    if x.pid == pid {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 挂载一个命名空间。
    pub fn mount(&mut self, ns: u32, hidepid: u8) -> Result<usize, u32> {
        if ns == 0 || hidepid > HIDE_ALL {
            return Err(PV_EINVAL);
        }
        if self.find_mount(ns).is_some() {
            return Err(PV_EDUP);
        }
        if self.mn >= MCAP {
            return Err(PV_EFULL);
        }
        if let Some(slot) = self.m.get_mut(self.mn) {
            *slot = Mount { ns, hidepid };
        }
        self.mn += 1;
        Ok(self.mn)
    }

    /// 卸载一个命名空间。
    pub fn umount(&mut self, ns: u32) -> Result<usize, u32> {
        let at = match self.find_mount(ns) {
            Some(v) => v,
            None => return Err(PV_ENOENT),
        };
        let mut i = 0usize;
        while i < self.pn {
            match self.p.get(i) {
                Some(x) => {
                    if x.ns == ns {
                        return Err(PV_EBUSY);
                    }
                }
                None => break,
            }
            i += 1;
        }
        let mut k = at;
        while k + 1 < self.mn {
            match self.m.get(k + 1).copied() {
                Some(next) => {
                    if let Some(dst) = self.m.get_mut(k) {
                        *dst = next;
                    }
                }
                None => break,
            }
            k += 1;
        }
        self.mn -= 1;
        Ok(self.mn)
    }

    /// 登记一个进程条目。
    pub fn proc(&mut self, pid: u32, uid: u32, ns: u32) -> Result<usize, u32> {
        if pid == 0 || uid == 0 || ns == 0 {
            return Err(PV_EINVAL);
        }
        if self.find_proc(pid).is_some() {
            return Err(PV_EDUP);
        }
        if self.pn >= PCAP {
            return Err(PV_EFULL);
        }
        if let Some(slot) = self.p.get_mut(self.pn) {
            *slot = Proc { pid, uid, ns };
        }
        self.pn += 1;
        Ok(self.pn)
    }

    /// 回收一个进程条目。
    pub fn reap(&mut self, pid: u32) -> Result<usize, u32> {
        let at = match self.find_proc(pid) {
            Some(v) => v,
            None => return Err(PV_ENOENT),
        };
        let mut k = at;
        while k + 1 < self.pn {
            match self.p.get(k + 1).copied() {
                Some(next) => {
                    if let Some(dst) = self.p.get_mut(k) {
                        *dst = next;
                    }
                }
                None => break,
            }
            k += 1;
        }
        self.pn -= 1;
        self.reaps += 1;
        Ok(self.pn)
    }

    /// 解析「某用户能否看见某个号」。
    pub fn lookup(&mut self, viewer: u32, pid: u32) -> Result<usize, u32> {
        let at = match self.find_proc(pid) {
            Some(v) => v,
            None => return Err(PV_ENOENT),
        };
        let pr = match self.p.get(at) {
            Some(v) => *v,
            None => return Err(PV_ENOENT),
        };
        let mi = match self.find_mount(pr.ns) {
            Some(v) => v,
            None => return Err(PV_ENOMOUNT),
        };
        let m = match self.m.get(mi) {
            Some(v) => *v,
            None => return Err(PV_ENOMOUNT),
        };
        self.looks += 1;
        if pr.uid == viewer {
            return Ok(1);
        }
        if m.hidepid == HIDE_ALL {
            self.hides += 1;
            return Err(PV_EINVIS);
        }
        Ok(1)
    }

    /// 读取条目细节。
    pub fn stat(&mut self, viewer: u32, pid: u32) -> Result<usize, u32> {
        let at = match self.find_proc(pid) {
            Some(v) => v,
            None => return Err(PV_ENOENT),
        };
        let pr = match self.p.get(at) {
            Some(v) => *v,
            None => return Err(PV_ENOENT),
        };
        let mi = match self.find_mount(pr.ns) {
            Some(v) => v,
            None => return Err(PV_ENOMOUNT),
        };
        let m = match self.m.get(mi) {
            Some(v) => *v,
            None => return Err(PV_ENOMOUNT),
        };
        if pr.uid == viewer {
            return Ok(1);
        }
        match m.hidepid {
            HIDE_NONE => Ok(1),
            HIDE_STAT => {
                self.hides += 1;
                Err(PV_EHIDDEN)
            }
            HIDE_ALL => {
                self.hides += 1;
                Err(PV_EINVIS)
            }
            _ => Err(PV_EINVAL),
        }
    }

    /// 统计某用户能看到的条目数。
    pub fn enumerate(&mut self, viewer: u32) -> Result<usize, u32> {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.pn {
            let pr = match self.p.get(i) {
                Some(v) => *v,
                None => return Err(PV_EBAL),
            };
            i += 1;
            if pr.uid == viewer {
                n += 1;
                continue;
            }
            let mi = match self.find_mount(pr.ns) {
                Some(v) => v,
                None => continue,
            };
            match self.m.get(mi) {
                Some(m) => {
                    if m.hidepid != HIDE_ALL {
                        n += 1;
                    }
                }
                None => return Err(PV_EBAL),
            }
        }
        Ok(n)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.mn > MCAP || self.pn > PCAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.mn {
            let m = match self.m.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if m.ns == 0 || m.hidepid > HIDE_ALL {
                return false;
            }
            let mut j = i + 1;
            while j < self.mn {
                match self.m.get(j) {
                    Some(o) => {
                        if o.ns == m.ns {
                            return false;
                        }
                    }
                    None => return false,
                }
                j += 1;
            }
            i += 1;
        }
        let mut k = 0usize;
        while k < self.pn {
            let p = match self.p.get(k) {
                Some(v) => *v,
                None => return false,
            };
            if p.pid == 0 || p.uid == 0 || p.ns == 0 {
                return false;
            }
            let mut l = k + 1;
            while l < self.pn {
                match self.p.get(l) {
                    Some(o) => {
                        if o.pid == p.pid {
                            return false;
                        }
                    }
                    None => return false,
                }
                l += 1;
            }
            k += 1;
        }
        self.looks >= self.hides
    }
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽位。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 是否成功。
    pub ok: bool,
    /// 错误码。
    pub code: u32,
    /// 观测值。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
    }
}

/// `Result<usize, u32>` → 样本。
fn cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 样本账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    root: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记一格。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(PV_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                if s.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(PV_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 1, 2, 3, 1, 1, 3];
        const CODES: [u32; 6] =
            [PV_EINVAL, PV_EDUP, PV_ENOENT, PV_EHIDDEN, PV_ENOMOUNT, PV_EFULL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(PV_EBAL),
            };
            if s.slot as usize != i {
                return Err(PV_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(PV_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(PV_EVALUE);
                        }
                    }
                    None => return Err(PV_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(PV_EVALUE);
                        }
                    }
                    None => return Err(PV_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(PV_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold(h, s.slot as u64);
                h = fold(h, s.op as u64);
                h = fold(h, s.ok as u64);
                h = fold(h, s.code as u64);
                h = fold(h, s.value as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(PV_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(PV_ESEAL)
        }
    }

    /// 冻结指纹。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 十四格账本：两挂载三进程 → 可见性 → 枚举 → 各类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut p = Procs::new();

    let _ = l.register(cell(0, OP_MOUNT, p.mount(1, HIDE_NONE)));
    let _ = l.register(cell(1, OP_MOUNT, p.mount(2, HIDE_ALL)));
    let _ = l.register(cell(2, OP_PROC, p.proc(10, 100, 1)));
    let _ = l.register(cell(3, OP_PROC, p.proc(11, 200, 1)));
    let _ = l.register(cell(4, OP_PROC, p.proc(12, 200, 2)));
    let _ = l.register(cell(5, OP_LOOKUP, p.lookup(200, 11)));
    let _ = l.register(cell(6, OP_STAT, p.stat(200, 11)));
    let _ = l.register(cell(7, OP_ENUM, p.enumerate(200)));

    let mut z = Procs::new();
    let _ = l.register(cell(8, OP_MOUNT, z.mount(0, 0)));

    let mut d = Procs::new();
    let _ = d.mount(1, 0);
    let _ = l.register(cell(9, OP_MOUNT, d.mount(1, 1)));

    let mut n = Procs::new();
    let _ = l.register(cell(10, OP_LOOKUP, n.lookup(1, 99)));

    let mut h = Procs::new();
    let _ = h.mount(3, HIDE_STAT);
    let _ = h.proc(20, 100, 3);
    let _ = l.register(cell(11, OP_STAT, h.stat(200, 20)));

    let mut u = Procs::new();
    let _ = u.proc(30, 100, 9);
    let _ = l.register(cell(12, OP_LOOKUP, u.lookup(100, 30)));

    let mut f = Procs::new();
    let mut i = 1u32;
    while (i as usize) <= MCAP {
        let _ = f.mount(i, 0);
        i += 1;
    }
    let _ = l.register(cell(13, OP_MOUNT, f.mount(99, 0)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：三档 hidepid 的可见性矩阵。
    let mut a = Procs::new();
    let _ = a.mount(1, HIDE_NONE);
    let _ = a.mount(2, HIDE_STAT);
    let _ = a.mount(3, HIDE_ALL);
    let _ = a.proc(100, 7, 1);
    let _ = a.proc(101, 8, 1);
    let _ = a.proc(102, 7, 2);
    let _ = a.proc(103, 8, 2);
    let _ = a.proc(104, 7, 3);
    let _ = a.proc(105, 8, 3);
    let own = a.lookup(7, 100) == Ok(1)
        && a.lookup(7, 102) == Ok(1)
        && a.lookup(7, 104) == Ok(1)
        && a.stat(7, 100) == Ok(1)
        && a.stat(7, 104) == Ok(1);
    let other = a.lookup(8, 100) == Ok(1)
        && a.stat(8, 100) == Ok(1)
        && a.lookup(8, 102) == Ok(1)
        && a.stat(8, 102) == Err(PV_EHIDDEN)
        && a.lookup(8, 104) == Err(PV_EINVIS)
        && a.stat(8, 104) == Err(PV_EINVIS);
    let counts = a.enumerate(8) == Ok(5) && a.enumerate(7) == Ok(5);
    out[1] = own && other && counts && a.consistent();

    // 二路：四类拒绝各自可辨。
    let mut b = Procs::new();
    let unknown = b.lookup(1, 1) == Err(PV_ENOENT);
    let _ = b.proc(7, 50, 4);
    let nomount = b.lookup(50, 7) == Err(PV_ENOMOUNT);
    let _ = b.mount(4, 0);
    let busy = b.umount(4) == Err(PV_EBUSY);
    let _ = b.reap(7);
    let gone = b.umount(4) == Ok(0);
    let twice = b.umount(4) == Err(PV_ENOENT);
    let reap_twice = b.reap(7) == Err(PV_ENOENT);
    out[2] = unknown && nomount && busy && gone && twice && reap_twice && b.consistent();

    // 三路：闸门与容量。
    let mut q = Procs::new();
    let q1 = q.mount(0, 0) == Err(PV_EINVAL);
    let q2 = q.mount(1, 3) == Err(PV_EINVAL);
    let q3 = q.mount(1, 0) == Ok(1);
    let q4 = q.mount(1, 0) == Err(PV_EDUP);
    let q5 = q.proc(0, 1, 1) == Err(PV_EINVAL);
    let q6 = q.proc(9, 0, 1) == Err(PV_EINVAL);
    let q7 = q.proc(9, 5, 1) == Ok(1);
    let q8 = q.proc(9, 5, 1) == Err(PV_EDUP);
    let mut cap = Procs::new();
    let mut i = 1u32;
    while (i as usize) <= PCAP {
        let _ = cap.proc(i, 1, 1);
        i += 1;
    }
    let q9 = cap.proc(99, 1, 1) == Err(PV_EFULL);
    let mut mc = Procs::new();
    let mut k = 1u32;
    while (k as usize) <= MCAP {
        let _ = mc.mount(k, 0);
        k += 1;
    }
    let q10 = mc.mount(99, 0) == Err(PV_EFULL);
    out[3] = q1 && q2 && q3 && q4 && q5 && q6 && q7 && q8 && q9 && q10 && q.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11356_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11356.procfs_visibility");
    let l = build_ledger();

    s.add_dyn(
        "PV1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "PV2-两档挂载",
        l.sample(0).map(|x| x.value) == Some(1) && l.sample(1).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "PV3-三条进程",
        l.sample(2).map(|x| x.value) == Some(1)
            && l.sample(3).map(|x| x.value) == Some(2)
            && l.sample(4).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "PV4-hidepid零他用户可见",
        l.sample(5).map(|x| x.value) == Some(1) && l.sample(6).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "PV5-枚举可见三条",
        l.sample(7).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "PV6-零号挂载被拒",
        l.sample(8).map(|x| x.code) == Some(PV_EINVAL),
        "static",
    );

    s.add(
        "PV7-重复挂载被拒",
        l.sample(9).map(|x| x.code) == Some(PV_EDUP),
        "static",
    );

    s.add(
        "PV8-未知号为ENOENT",
        l.sample(10).map(|x| x.code) == Some(PV_ENOENT),
        "static",
    );

    s.add(
        "PV9-hidepid一藏细节",
        l.sample(11).map(|x| x.code) == Some(PV_EHIDDEN),
        "static",
    );

    s.add(
        "PV10-未挂载命名空间",
        l.sample(12).map(|x| x.code) == Some(PV_ENOMOUNT),
        "static",
    );

    s.add(
        "PV11-挂载表满被拒",
        l.sample(13).map(|x| x.code) == Some(PV_EFULL),
        "static",
    );

    s.add(
        "PV12-hidepid二不可见且卸载忙",
        {
            let mut a = Procs::new();
            let _ = a.mount(1, HIDE_ALL);
            let _ = a.proc(5, 100, 1);
            let _ = a.proc(6, 100, 1);
            let hidden = a.lookup(200, 5) == Err(PV_EINVIS);
            let own = a.lookup(100, 5) == Ok(1) && a.stat(100, 6) == Ok(1);
            let seen = a.enumerate(200) == Ok(0) && a.enumerate(100) == Ok(2);
            let busy = a.umount(1) == Err(PV_EBUSY);
            let _ = a.reap(5);
            let _ = a.reap(6);
            let empty = a.umount(1) == Ok(0) && a.proc_count() == 0;
            hidden && own && seen && busy && empty && a.consistent()
        },
        "static",
    );

    s.add(
        "PV13-冻结重放四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two.is_err()
                && f.is_frozen()
                && f.root() != 0
                && f.rejects() == 1
                && inj == [true; 4]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11356_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn hidepid_levels() {
        let mut p = Procs::new();
        assert_eq!(p.mount(1, HIDE_NONE), Ok(1));
        assert_eq!(p.mount(2, HIDE_STAT), Ok(2));
        assert_eq!(p.mount(3, HIDE_ALL), Ok(3));
        assert_eq!(p.proc(10, 1, 1), Ok(1));
        assert_eq!(p.proc(20, 1, 2), Ok(2));
        assert_eq!(p.proc(30, 1, 3), Ok(3));
        assert_eq!(p.lookup(2, 10), Ok(1));
        assert_eq!(p.stat(2, 10), Ok(1));
        assert_eq!(p.lookup(2, 20), Ok(1));
        assert_eq!(p.stat(2, 20), Err(PV_EHIDDEN));
        assert_eq!(p.lookup(2, 30), Err(PV_EINVIS));
        assert_eq!(p.stat(2, 30), Err(PV_EINVIS));
    }

    #[test]
    fn own_proc_always_visible() {
        let mut p = Procs::new();
        assert_eq!(p.mount(1, HIDE_ALL), Ok(1));
        assert_eq!(p.proc(10, 5, 1), Ok(1));
        assert_eq!(p.lookup(5, 10), Ok(1));
        assert_eq!(p.stat(5, 10), Ok(1));
        assert_eq!(p.enumerate(5), Ok(1));
        assert_eq!(p.enumerate(6), Ok(0));
    }

    #[test]
    fn unmounted_namespace_differs_from_missing() {
        let mut p = Procs::new();
        assert_eq!(p.lookup(1, 77), Err(PV_ENOENT));
        assert_eq!(p.proc(77, 1, 3), Ok(1));
        assert_eq!(p.lookup(1, 77), Err(PV_ENOMOUNT));
        assert_eq!(p.stat(1, 77), Err(PV_ENOMOUNT));
    }

    #[test]
    fn umount_busy_until_empty() {
        let mut p = Procs::new();
        assert_eq!(p.mount(1, 0), Ok(1));
        assert_eq!(p.proc(1, 9, 1), Ok(1));
        assert_eq!(p.umount(1), Err(PV_EBUSY));
        assert_eq!(p.reap(1), Ok(0));
        assert_eq!(p.umount(1), Ok(0));
        assert_eq!(p.mount_count(), 0);
        assert_eq!(p.umount(1), Err(PV_ENOENT));
    }

    #[test]
    fn gates_and_capacity() {
        let mut p = Procs::new();
        assert_eq!(p.mount(0, 0), Err(PV_EINVAL));
        assert_eq!(p.mount(1, 9), Err(PV_EINVAL));
        assert_eq!(p.proc(0, 1, 1), Err(PV_EINVAL));
        assert_eq!(p.proc(1, 0, 1), Err(PV_EINVAL));
        assert_eq!(p.proc(1, 1, 0), Err(PV_EINVAL));
        let mut i = 1u32;
        while (i as usize) <= PCAP {
            assert_eq!(p.proc(i, 1, 1), Ok(i as usize));
            i += 1;
        }
        assert_eq!(p.proc(99, 1, 1), Err(PV_EFULL));
        assert_eq!(p.reap(1), Ok(PCAP - 1));
        assert!(p.consistent());
    }

    #[test]
    fn enumerate_skips_invisible() {
        let mut p = Procs::new();
        assert_eq!(p.mount(1, HIDE_NONE), Ok(1));
        assert_eq!(p.mount(2, HIDE_ALL), Ok(2));
        assert_eq!(p.proc(1, 7, 1), Ok(1));
        assert_eq!(p.proc(2, 8, 1), Ok(2));
        assert_eq!(p.proc(3, 8, 2), Ok(3));
        assert_eq!(p.proc(4, 7, 2), Ok(4));
        assert_eq!(p.enumerate(8), Ok(3));
        assert_eq!(p.enumerate(7), Ok(3));
    }
}
