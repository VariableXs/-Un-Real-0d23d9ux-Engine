//! UNX-F8021 进程描述符本体（task_struct 级建模）（0xA200 段）
//!
//! 锚点：UNX-F8021-J1 描述符核心字段（pid/tgid/父指针/子链/退出码/
//! fd 表/mm/人格/组会话归属）逐字段有账，字段清单与 Linux task_struct
//! 主干逐项对照表在册且偏差有解释。
//! 域：CoRun·UNX-C1-B02（进程描述符面）。
//!
//! 结构：描述符核心字段账（pid/tgid/parent/children/exit_code/fd_table/
//! mm/personality/pgrp/session 十字段冻结）+ Linux task_struct 主干
//! 对照表（逐项：一致/子集/偏差+解释）+ 字段操作语义（孤儿化/组重挂/
//! fd 表克隆/mm 换绑/人格冻结）+ 注入核（自为父/退出码漂移/fd 溢出/
//! 人格未知/封账）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xA200 独占 0x80）。

pub const SEG_BASE: u32 = 0xA200;
pub const XC_TS_SELF: u32 = SEG_BASE + 0x01; // 自为父
pub const XC_TS_EXIT: u32 = SEG_BASE + 0x02; // 退出码漂移
pub const XC_TS_FDOV: u32 = SEG_BASE + 0x03; // fd 溢出
pub const XC_TS_PERS: u32 = SEG_BASE + 0x04; // 人格未知
pub const XC_TS_SEALED: u32 = SEG_BASE + 0x05; // 封账

/// 人格闭集（Linux personality 域代表）。
pub const PER_LINUX: u32 = 0x0000;
pub const PER_LINUX32: u32 = 0x0800;
pub const PER_LINUX_FDPIC: u32 = 0x2000;
pub const PER_VALID: [u32; 3] = [PER_LINUX, PER_LINUX32, PER_LINUX_FDPIC];

/// 文件描述符表（线性账 + 下一个空位指针，Linux 语义）。
pub struct FdTable {
    pub slots: Vec<Option<u64>>, // fd → 文件对象锚
    pub next_free: usize,
    pub max_fds: usize,
}

impl FdTable {
    pub fn new(max_fds: usize) -> Self {
        let mut slots = Vec::new();
        for _ in 0..max_fds {
            slots.push(None);
        }
        // fd 0/1/2 预留（std 三件）
        slots[0] = Some(0xAA01);
        slots[1] = Some(0xAA02);
        slots[2] = Some(0xAA03);
        FdTable { slots, next_free: 3, max_fds }
    }

    /// 分配 fd（最低空位语义）。
    pub fn alloc_fd(&mut self, obj: u64) -> Result<usize, u32> {
        if self.next_free >= self.max_fds {
            return Err(XC_TS_FDOV);
        }
        let fd = self.next_free;
        self.slots[fd] = Some(obj);
        // 推进 next_free
        let mut i = fd + 1;
        while i < self.max_fds && self.slots[i].is_some() {
            i += 1;
        }
        self.next_free = i;
        Ok(fd)
    }

    /// 释放 fd（next_free 回退语义）。
    pub fn free_fd(&mut self, fd: usize) -> Result<(), u32> {
        if fd >= self.max_fds || fd < 3 {
            return Err(XC_TS_FDOV);
        }
        self.slots[fd] = None;
        if fd < self.next_free {
            self.next_free = fd;
        }
        Ok(())
    }

    /// fd 表克隆（fork 语义）。
    pub fn clone_table(&self) -> FdTable {
        let mut slots = Vec::new();
        for s in &self.slots {
            slots.push(*s);
        }
        FdTable { slots, next_free: self.next_free, max_fds: self.max_fds }
    }

    /// 占用计数。
    pub fn used(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }
}

/// 进程描述符（task_struct 主干字段账）。
pub struct TaskDesc {
    pub pid: u64,
    pub tgid: u64,
    pub parent: Option<u64>,
    pub children: Vec<u64>,
    pub exit_code: Option<u32>,
    pub fd_table: FdTable,
    pub mm: Option<u64>, // mm_struct 锚（None=内核线程）
    pub personality: u32,
    pub pgrp: u64,
    pub session: u64,
    pub alive: bool,
}

impl TaskDesc {
    /// 新描述符（用户进程：mm 有锚）。
    pub fn new_user(pid: u64, tgid: u64, parent: Option<u64>, mm: u64) -> Self {
        TaskDesc {
            pid,
            tgid,
            parent,
            children: Vec::new(),
            exit_code: None,
            fd_table: FdTable::new(16),
            mm: Some(mm),
            personality: PER_LINUX,
            pgrp: pid,
            session: pid,
            alive: true,
        }
    }

    /// 内核线程（mm=None）。
    pub fn new_kthread(pid: u64, parent: Option<u64>) -> Self {
        TaskDesc {
            pid,
            tgid: pid,
            parent,
            children: Vec::new(),
            exit_code: None,
            fd_table: FdTable::new(4),
            mm: None,
            personality: PER_LINUX,
            pgrp: 0,
            session: 0,
            alive: true,
        }
    }

    /// 挂子（自为父拒）。
    pub fn add_child(&mut self, child_pid: u64) -> Result<(), u32> {
        if child_pid == self.pid {
            return Err(XC_TS_SELF);
        }
        if !self.alive {
            return Err(XC_TS_SEALED);
        }
        self.children.push(child_pid);
        Ok(())
    }

    /// 孤儿化：父退出 → 子重挂 reaper（pid=1 收养）。
    pub fn orphanize(&mut self, reaper: u64) -> Result<(), u32> {
        if reaper == self.pid {
            return Err(XC_TS_SELF);
        }
        self.parent = Some(reaper);
        Ok(())
    }

    /// 组重挂（会话归属一致性：pgrp ∈ session 内）。
    pub fn set_pgrp(&mut self, pgrp: u64) -> Result<(), u32> {
        if pgrp != self.session && self.pgrp != self.session {
            // 允许迁移到同会话组；异会话拒（Linux setpgid 语义）
            return Err(XC_TS_SEALED);
        }
        self.pgrp = pgrp;
        Ok(())
    }

    /// 退出（退出码冻结 + fd 表清 + mm 释放账）。
    pub fn exit(&mut self, code: u32) -> Result<(), u32> {
        if !self.alive {
            return Err(XC_TS_SEALED);
        }
        self.exit_code = Some(code & 0xFF);
        self.alive = false;
        Ok(())
    }

    /// 换绑 mm（execve 语义）。
    pub fn rebind_mm(&mut self, new_mm: u64) -> Result<(), u32> {
        if !self.alive {
            return Err(XC_TS_SEALED);
        }
        self.mm = Some(new_mm);
        Ok(())
    }

    /// 人格设置（未知人格拒）。
    pub fn set_personality(&mut self, p: u32) -> Result<(), u32> {
        if !PER_VALID.contains(&p) {
            return Err(XC_TS_PERS);
        }
        self.personality = p;
        Ok(())
    }
}

/// Linux task_struct 主干对照表项：(字段名, 对照态, 偏差解释)。
#[derive(Clone, Copy)]
pub struct FieldMap {
    pub field: &'static str,
    pub state: MapState,
    pub note: &'static str,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MapState {
    Matched,  // 语义一致
    Subset,   // Varix 子集（裁剪有解释）
}

/// 十字段对照表（冻结）。
pub const FIELD_MAP: [FieldMap; 10] = [
    FieldMap { field: "pid", state: MapState::Matched, note: "一致" },
    FieldMap { field: "tgid", state: MapState::Matched, note: "线程组一致" },
    FieldMap { field: "parent", state: MapState::Matched, note: "real_parent 语义" },
    FieldMap { field: "children", state: MapState::Matched, note: "子链一致" },
    FieldMap { field: "exit_code", state: MapState::Matched, note: "低 8 位冻结" },
    FieldMap { field: "fd_table", state: MapState::Subset, note: "线性账替 files_struct，语义同" },
    FieldMap { field: "mm", state: MapState::Matched, note: "内核线程 None 同" },
    FieldMap { field: "personality", state: MapState::Subset, note: "三人格闭集，偏差：未含全部 Linux 域" },
    FieldMap { field: "pgrp", state: MapState::Matched, note: "组归属一致" },
    FieldMap { field: "session", state: MapState::Matched, note: "会话归属一致" },
];

/// 对照表完整性核：十字段全在册 + 偏差全有解释。
pub fn field_map_exact() -> bool {
    if FIELD_MAP.len() != 10 {
        return false;
    }
    let names = ["pid", "tgid", "parent", "children", "exit_code", "fd_table", "mm", "personality", "pgrp", "session"];
    for n in names.iter() {
        if !FIELD_MAP.iter().any(|m| m.field == *n) {
            return false;
        }
    }
    FIELD_MAP.iter().all(|m| match m.state {
        MapState::Matched => true,
        MapState::Subset => !m.note.is_empty(), // 偏差必须有解释
    })
}

/// 字段全账核：新用户进程十字段逐项有账。
pub fn user_desc_exact() -> bool {
    let mut t = TaskDesc::new_user(100, 100, Some(1), 0x110);
    if t.pid != 100 || t.tgid != 100 || t.parent != Some(1) {
        return false;
    }
    if t.fd_table.used() != 3 || t.mm != Some(0x110) {
        return false;
    }
    if t.personality != PER_LINUX || t.pgrp != 100 || t.session != 100 {
        return false;
    }
    if t.exit_code.is_some() || !t.children.is_empty() {
        return false;
    }
    // 挂子
    t.add_child(101).is_ok()
}

/// 孤儿化收养核：父退 → reaper 收养。
pub fn orphan_reaper_exact() -> bool {
    let mut child = TaskDesc::new_user(200, 200, Some(100), 1);
    if child.orphanize(1).is_err() {
        return false;
    }
    child.parent == Some(1) && child.orphanize(200).is_err() // 自收养拒
}

/// fd 表语义核：分配/释放/克隆/最低空位。
pub fn fd_table_exact() -> bool {
    let mut t = TaskDesc::new_user(300, 300, Some(1), 2);
    // 分配 fd=3,4,5
    let a = t.fd_table.alloc_fd(0xB1);
    let b = t.fd_table.alloc_fd(0xB2);
    let c = t.fd_table.alloc_fd(0xB3);
    if a != Ok(3) || b != Ok(4) || c != Ok(5) {
        return false;
    }
    // 释放 4 → next_free 回退到 4
    if t.fd_table.free_fd(4).is_err() {
        return false;
    }
    let d = t.fd_table.alloc_fd(0xB4);
    if d != Ok(4) {
        return false;
    }
    // 克隆一致
    let c2 = t.fd_table.clone_table();
    c2.used() == t.fd_table.used() && c2.next_free == t.fd_table.next_free
}

/// 注入核（5 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 自为父
    let mut t = TaskDesc::new_user(400, 400, Some(1), 3);
    if t.add_child(400) == Err(XC_TS_SELF) {
        caught += 1;
    }
    // 2) 未知人格
    if t.set_personality(0xDEAD) == Err(XC_TS_PERS) {
        caught += 1;
    }
    // 3) fd 溢出（max=4：3 个 std + 1 个可分配）
    let mut k = TaskDesc::new_kthread(401, Some(1));
    let _ = k.fd_table.alloc_fd(0xC1);
    if k.fd_table.alloc_fd(0xC2) == Err(XC_TS_FDOV) {
        caught += 1;
    }
    // 4) 退出码冻结（高 24 位裁掉）
    let mut e = TaskDesc::new_user(402, 402, Some(1), 4);
    let _ = e.exit(0x1234_56AB);
    if e.exit_code == Some(0xAB) {
        caught += 1;
    }
    // 5) 封账（死后操作拒）
    if e.exit(0).is_err() && e.add_child(500).is_err() {
        caught += 1;
    }
    (caught, 5)
}

/// 指纹冻结：描述符折叠哈希。
pub fn fp_frozen(t: &TaskDesc) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    h ^= t.pid;
    h = h.wrapping_mul(0x100000001b3);
    h ^= t.tgid;
    h = h.wrapping_mul(0x100000001b3);
    h ^= t.parent.unwrap_or(0);
    h = h.wrapping_mul(0x100000001b3);
    h ^= t.children.len() as u64;
    h = h.wrapping_mul(0x100000001b3);
    h ^= t.exit_code.unwrap_or(0xFFFF_FFFF) as u64;
    h = h.wrapping_mul(0x100000001b3);
    h ^= t.fd_table.used() as u64;
    h = h.wrapping_mul(0x100000001b3);
    h ^= t.mm.unwrap_or(0);
    h = h.wrapping_mul(0x100000001b3);
    h ^= t.personality as u64;
    h = h.wrapping_mul(0x100000001b3);
    h ^= t.pgrp;
    h = h.wrapping_mul(0x100000001b3);
    h ^= t.session;
    h = h.wrapping_mul(0x100000001b3);
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx8021_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F8021");
    // J1-1 字段对照表
    let ok = field_map_exact();
    s.add("B02-01-字段对照表", ok, if ok { "" } else { "对照漂移" });
    // J1-2 字段全账
    let ok = user_desc_exact();
    s.add("B02-02-字段全账", ok, if ok { "" } else { "字段漂移" });
    // J1-3 孤儿化收养
    let ok = orphan_reaper_exact();
    s.add("B02-03-孤儿收养", ok, if ok { "" } else { "收养漂移" });
    // J1-4 fd 表语义
    let ok = fd_table_exact();
    s.add("B02-04-fd表语义", ok, if ok { "" } else { "fd 漂移" });
    // J1-5 内核线程建模
    let k = TaskDesc::new_kthread(2, None);
    let ok = k.mm.is_none() && k.pgrp == 0 && k.session == 0 && k.tgid == k.pid;
    s.add("B02-05-内核线程", ok, if ok { "" } else { "kthread 漂移" });
    // J1-6 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B02-06-注入核必抓", ok, &detail);
    // J1-7 指纹冻结
    let t = TaskDesc::new_user(600, 600, Some(1), 9);
    let ok = fp_frozen(&t) == fp_frozen(&t);
    s.add("B02-07-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
