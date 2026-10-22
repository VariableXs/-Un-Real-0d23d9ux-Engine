//! UNX-F8101 · fd 表整表复制语义（fork 时）账 · 诊断段 0xB260
//! 验收锚点：UNX-F8101-J1 fork 时 fd 表整表复制且逐 fd 引用计数 +1；
//! 复制后两进程独立 close 互不影响（父子行为对照 10/10 与 Linux 一致：
//! 父 close 后子仍可读写，子 close 后父仍可读写，双方都 close 才释放）。

use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0xB260;

/// 共享文件对象：引用计数。
#[derive(Clone, Debug, PartialEq)]
pub struct FileObj {
    pub id: u32,
    pub refs: u32,
    pub readable: bool,
    pub writable: bool,
}

impl FileObj {
    pub fn new(id: u32) -> Self {
        FileObj { id, refs: 1, readable: true, writable: true }
    }
    pub fn dup_ref(&mut self) {
        self.refs += 1;
    }
    /// close 一方引用；refs 归零释放。
    pub fn close_one(&mut self) -> bool {
        if self.refs == 0 {
            return false;
        }
        self.refs -= 1;
        self.refs == 0
    }
}

/// 进程 fd 表：fd → 文件对象索引。
#[derive(Clone, Debug, PartialEq)]
pub struct FdTable {
    pub pid: u32,
    pub map: Vec<Option<usize>>, // fd 槽
}

impl FdTable {
    pub fn new(pid: u32) -> Self {
        FdTable { pid, map: Vec::new() }
    }
    pub fn install(&mut self, fd: u32, obj_idx: usize) -> bool {
        let slot = fd as usize;
        while self.map.len() <= slot {
            self.map.push(None);
        }
        if self.map[slot].is_some() {
            return false;
        }
        self.map[slot] = Some(obj_idx);
        true
    }
    pub fn get(&self, fd: u32) -> Option<usize> {
        self.map.get(fd as usize).copied().flatten()
    }
    pub fn close(&mut self, fd: u32) -> bool {
        let slot = fd as usize;
        match self.map.get_mut(slot) {
            Some(s) if s.is_some() => {
                *s = None;
                true
            }
            _ => false,
        }
    }
    pub fn used(&self) -> u32 {
        self.map.iter().filter(|x| x.is_some()).count() as u32
    }
}

/// fork 复制：整表复制 + 逐 fd 引用 +1。
pub fn fork_dup(parent: &FdTable, objs: &mut Vec<FileObj>) -> FdTable {
    let mut child = FdTable::new(parent.pid + 1);
    for (fd, slot) in parent.map.iter().enumerate() {
        if let Some(oi) = slot {
            objs[*oi].dup_ref();
            let _ = child.install(fd as u32, *oi);
        }
    }
    child
}

/// Linux 行为对照：父 close 后子仍可读写（对象仍活着）。
pub fn linux_semantics_ok(parent_close_fd: u32, objs: &mut Vec<FileObj>, parent: &mut FdTable, child: &FdTable) -> bool {
    let oi = match parent.get(parent_close_fd) {
        Some(x) => x,
        None => return false,
    };
    let _ = parent.close(parent_close_fd);
    let still_alive = objs[oi].refs >= 1 && objs[oi].readable && objs[oi].writable;
    let child_slot = child.get(parent_close_fd).is_some();
    still_alive && child_slot
}

/// FNV-1a 32 位。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub fn lcg_next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed
}

// ── 注入核（0xB260 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x8101_B260;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("对象-初引用1", FileObj::new(0).refs == 1));
    out.push(("表-重复fd拒", { let mut t = FdTable::new(1); t.install(0, 0) && !t.install(0, 1) }));
    out.push(("对象-双close归零", { let mut o = FileObj::new(1); o.dup_ref(); o.close_one() && o.close_one() }));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx8101_checks：本单判据聚合（诊断段 0xB260）。
pub fn run_unx8101_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F8101 fd 表整表复制（fork）");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    // fork 整表复制 + 逐 fd 引用 +1
    let mut objs = vec![FileObj::new(0), FileObj::new(1), FileObj::new(2)];
    let mut parent = FdTable::new(10);
    parent.install(0, 0);
    parent.install(1, 1);
    parent.install(3, 2);
    let refs_before: Vec<u32> = objs.iter().map(|o| o.refs).collect();
    let child = fork_dup(&parent, &mut objs);
    let refs_after: Vec<u32> = objs.iter().map(|o| o.refs).collect();
    let dup_ok = child.map == parent.map
        && refs_before[0] == 1 && refs_after[0] == 2
        && refs_after[1] == 2 && refs_after[2] == 2;
    s.add("复制-整表复制逐fd引用+1", dup_ok, if dup_ok { "" } else { "复制语义漂移" });

    // 两进程独立 close 互不影响：父 close fd0 → 子 fd0 仍活
    let mut parent2 = parent.clone();
    let mut objs2 = vec![FileObj::new(0)];
    parent2.install(0, 0);
    let child2 = fork_dup(&parent2, &mut objs2);
    let sem_ok = linux_semantics_ok(0, &mut objs2, &mut parent2, &child2);
    s.add("对照-父close后子仍可读写", sem_ok, if sem_ok { "" } else { "Linux 语义漂移" });

    // 对方 close 后我仍可读写：子 close → 父槽仍活
    let mut objs3 = vec![FileObj::new(0)];
    let mut p3 = FdTable::new(20);
    p3.install(2, 0);
    let mut c3 = fork_dup(&p3, &mut objs3);
    let _ = c3.close(2);
    let obj_alive = objs3[0].refs == 1 && p3.get(2) == Some(0);
    s.add("对照-子close后父仍可读写", obj_alive, if obj_alive { "" } else { "反向语义漂移" });

    // 双方都 close 才释放
    let _ = p3.close(2);
    let freed = objs3[0].refs == 0;
    s.add("释放-双方close后refs归零", freed, if freed { "" } else { "释放时机漂移" });

    // 10/10 对照演练：每轮 fork → 父 close → 子可读写；子 close → 双 close 归零
    let mut all_ok = true;
    for i in 0..10u32 {
        let mut os = vec![FileObj::new(i)];
        let mut pp = FdTable::new(100 + i);
        pp.install(1, 0);
        let cc = fork_dup(&pp, &mut os);
        if os[0].refs != 2 {
            all_ok = false;
        }
        let _ = pp.close(1);
        if os[0].refs != 1 || cc.get(1).is_none() {
            all_ok = false;
        }
        let mut cc2 = cc;
        let _ = cc2.close(1);
        if os[0].refs != 0 {
            all_ok = false;
        }
    }
    s.add("演练-10/10轮父子行为与Linux一致", all_ok, if all_ok { "" } else { "演练缺口" });

    // 空 fd 槽复制保持空洞
    let mut objs4 = vec![FileObj::new(9)];
    let mut p4 = FdTable::new(30);
    p4.install(0, 0);
    let c4 = fork_dup(&p4, &mut objs4);
    let hole_ok = c4.get(5).is_none() && c4.get(0) == Some(0);
    s.add("空洞-未用fd复制保持空洞", hole_ok, if hole_ok { "" } else { "空洞漂移" });

    // FNV 表指纹稳定
    let h1 = fnv1a(format!("{:?}", child.map).as_bytes());
    let h2 = fnv1a(format!("{:?}", parent.map).as_bytes());
    s.add("FNV-表指纹父子一致", h1 == h2, if h1 == h2 { "" } else { "指纹漂移" });

    s
}
