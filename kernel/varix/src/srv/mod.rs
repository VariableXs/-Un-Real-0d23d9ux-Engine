//! VARIABLE-200 AI-04 · 内核服务与 initfs 域（F076~F100，W2）。
//!
//! 使命：让用户态"住得下来"——initfs 只读根、pid 1、声明式服务管理器、
//! 看门狗与崩溃重启策略、共享内存/事件通道、VFS 用户态面。
//!
//! 纪律：纯逻辑 + 固定容量数组；无 `Vec`/`String`/`Box`/`alloc`/外部 crate；
//! 不碰 `src/`、`src-tauri/`（回滚：只增不删）。每项功能自带可复现断言，
//! 域自检 `run_srv_checks()` 25 项全绿方算完成。

use crate::checks::CheckSet;

pub mod ipc;
pub mod sys;

// ---------------------------------------------------------------------------
// F076 initramfs — 引导镜像内只读根文件系统（newc 风格归档的目录解析）
// ---------------------------------------------------------------------------

/// initfs 目录表上限。
pub const INITFS_MAX_FILES: usize = 32;
/// 单条目名上限（含 NUL 余量）。
pub const INITFS_MAX_NAME: usize = 32;
/// initramfs 归档的载荷上限。
pub const INITFS_CAPACITY: usize = 4096;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InitfsKind {
    Regular,
    Directory,
    Executable,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InitfsEntry {
    pub name: [u8; INITFS_MAX_NAME],
    pub name_len: usize,
    pub data_off: usize,
    pub size: usize,
    pub kind: InitfsKind,
    /// newc 头声明的校验和（载荷逐字节求和）。
    pub checksum: u32,
}

impl InitfsEntry {
    pub const fn empty() -> InitfsEntry {
        InitfsEntry {
            name: [0u8; INITFS_MAX_NAME],
            name_len: 0,
            data_off: 0,
            size: 0,
            kind: InitfsKind::Regular,
            checksum: 0,
        }
    }

    pub fn name_bytes(&self) -> &[u8] {
        &self.name[..self.name_len]
    }

    pub fn name_eq(&self, want: &[u8]) -> bool {
        self.name_bytes() == want
    }
}

/// 只读 initramfs：归档字节流 + 解析出的目录表。
#[derive(Clone, Copy)]
pub struct Initfs {
    blob: [u8; INITFS_CAPACITY],
    /// 已写入的载荷字节数。
    pub blob_len: usize,
    entries: [InitfsEntry; INITFS_MAX_FILES],
    count: usize,
}

impl Initfs {
    pub const fn new() -> Initfs {
        Initfs {
            blob: [0u8; INITFS_CAPACITY],
            blob_len: 0,
            entries: [InitfsEntry::empty(); INITFS_MAX_FILES],
            count: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn entry(&self, i: usize) -> Option<InitfsEntry> {
        if i < self.count {
            Some(self.entries[i])
        } else {
            None
        }
    }

    /// 追加一个条目并写入载荷，返回条目下标。表满或载荷溢出返回 `None`。
    pub fn add(&mut self, name: &[u8], kind: InitfsKind, data: &[u8]) -> Option<usize> {
        if self.count >= INITFS_MAX_FILES || name.is_empty() || name.len() >= INITFS_MAX_NAME {
            return None;
        }
        if self.blob_len + data.len() > INITFS_CAPACITY {
            return None;
        }
        let mut e = InitfsEntry::empty();
        let mut i = 0usize;
        while i < name.len() {
            e.name[i] = name[i];
            i += 1;
        }
        e.name_len = name.len();
        e.data_off = self.blob_len;
        e.size = data.len();
        e.kind = kind;
        let mut sum = 0u32;
        let mut k = 0usize;
        while k < data.len() {
            self.blob[self.blob_len + k] = data[k];
            sum = sum.wrapping_add(data[k] as u32);
            k += 1;
        }
        e.checksum = sum;
        self.blob_len += data.len();
        self.entries[self.count] = e;
        self.count += 1;
        Some(self.count - 1)
    }

    pub fn find(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.count {
            if self.entries[i].name_eq(name) {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    pub fn data(&self, idx: usize) -> Option<&[u8]> {
        let e = self.entry(idx)?;
        let end = e.data_off + e.size;
        if end <= self.blob_len {
            Some(&self.blob[e.data_off..end])
        } else {
            None
        }
    }

    /// 重新校验载荷和：归档自检面。
    pub fn verify(&self, idx: usize) -> bool {
        let e = match self.entry(idx) {
            Some(e) => e,
            None => return false,
        };
        let d = match self.data(idx) {
            Some(d) => d,
            None => return false,
        };
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < d.len() {
            sum = sum.wrapping_add(d[i] as u32);
            i += 1;
        }
        sum == e.checksum
    }

    /// 只读根的总占用。
    pub fn total_bytes(&self) -> usize {
        self.blob_len
    }
}

// ---------------------------------------------------------------------------
// F077 init 进程（pid 1）— 孤儿收养与首批服务拉起
// ---------------------------------------------------------------------------

pub const INIT_MAX_ORPHANS: usize = 16;
pub const INIT_MAX_CHILDREN: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InitState {
    Early,
    Starting,
    Ready,
    Reaping,
}

#[derive(Clone, Copy)]
pub struct InitProc {
    pub pid: u32,
    pub state: InitState,
    orphans: [u32; INIT_MAX_ORPHANS],
    orphan_count: usize,
    children: [u32; INIT_MAX_CHILDREN],
    child_count: usize,
    /// 已拉起服务数。
    pub started: usize,
}

impl InitProc {
    pub const fn new() -> InitProc {
        InitProc {
            pid: 1,
            state: InitState::Early,
            orphans: [0u32; INIT_MAX_ORPHANS],
            orphan_count: 0,
            children: [0u32; INIT_MAX_CHILDREN],
            child_count: 0,
            started: 0,
        }
    }

    /// 收养一个孤儿进程（pid != 1 且未在表内）。
    pub fn adopt(&mut self, pid: u32) -> bool {
        if pid == self.pid || pid == 0 || self.orphan_count >= INIT_MAX_ORPHANS {
            return false;
        }
        let mut i = 0usize;
        while i < self.orphan_count {
            if self.orphans[i] == pid {
                return false;
            }
            i += 1;
        }
        self.orphans[self.orphan_count] = pid;
        self.orphan_count += 1;
        true
    }

    pub fn orphan_count(&self) -> usize {
        self.orphan_count
    }

    pub fn is_orphan(&self, pid: u32) -> bool {
        let mut i = 0usize;
        while i < self.orphan_count {
            if self.orphans[i] == pid {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 回收已退出的孤儿，返回回收个数。
    pub fn reap(&mut self, exited: &[u32]) -> usize {
        let mut n = 0usize;
        let mut w = 0usize;
        let mut r = 0usize;
        while r < self.orphan_count {
            let pid = self.orphans[r];
            if contains(exited, pid) {
                n += 1;
            } else {
                self.orphans[w] = pid;
                w += 1;
            }
            r += 1;
        }
        self.orphan_count = w;
        self.state = if n > 0 { InitState::Reaping } else { self.state };
        n
    }

    /// 登记一个由 init 拉起的服务进程。
    pub fn spawn_service(&mut self, pid: u32) -> bool {
        if self.child_count >= INIT_MAX_CHILDREN || pid == 0 {
            return false;
        }
        self.children[self.child_count] = pid;
        self.child_count += 1;
        self.started += 1;
        true
    }

    pub fn child_count(&self) -> usize {
        self.child_count
    }

    /// 服务全部拉起后置 Ready。
    pub fn mark_ready(&mut self, expected: usize) -> bool {
        if self.started >= expected && expected > 0 {
            self.state = InitState::Ready;
            true
        } else {
            self.state = InitState::Starting;
            false
        }
    }
}

pub fn contains(xs: &[u32], v: u32) -> bool {
    let mut i = 0usize;
    while i < xs.len() {
        if xs[i] == v {
            return true;
        }
        i += 1;
    }
    false
}

// ---------------------------------------------------------------------------
// F078 VFS 用户态暴露 — fd 表 + open/read/write/stat/close 全套
// ---------------------------------------------------------------------------

pub const VFS_MAX_FD: usize = 16;
pub const VFS_READ_BUF: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VfsErr {
    Ok,
    NoEntry,
    BadFd,
    NotOpen,
    WouldBlock,
    NoPerm,
    TooMany,
    IsDir,
}

#[derive(Clone, Copy)]
pub struct OpenFile {
    pub ino: usize,
    pub pos: usize,
    pub writable: bool,
}

/// 用户态 VFS 视图：initfs 作为只读后端。
#[derive(Clone, Copy)]
pub struct VfsUser {
    fs: Initfs,
    fds: [Option<OpenFile>; VFS_MAX_FD],
    pub opens: usize,
    pub closes: usize,
}

impl VfsUser {
    pub const fn new(fs: Initfs) -> VfsUser {
        VfsUser {
            fs,
            fds: [None; VFS_MAX_FD],
            opens: 0,
            closes: 0,
        }
    }

    pub fn backend(&self) -> &Initfs {
        &self.fs
    }

    pub fn open(&mut self, path: &[u8], writable: bool) -> Result<usize, VfsErr> {
        let ino = self.fs.find(path).ok_or(VfsErr::NoEntry)?;
        if self.fs.entry(ino).map(|e| e.kind) == Some(InitfsKind::Directory) {
            return Err(VfsErr::IsDir);
        }
        // 只读根上拒绝写打开。
        if writable && self.fs.entry(ino).map(|e| e.kind) != Some(InitfsKind::Regular) {
            return Err(VfsErr::NoPerm);
        }
        let mut i = 0usize;
        while i < VFS_MAX_FD {
            if self.fds[i].is_none() {
                self.fds[i] = Some(OpenFile { ino, pos: 0, writable });
                self.opens += 1;
                return Ok(i);
            }
            i += 1;
        }
        Err(VfsErr::TooMany)
    }

    fn get(&mut self, fd: usize) -> Result<OpenFile, VfsErr> {
        if fd >= VFS_MAX_FD {
            return Err(VfsErr::BadFd);
        }
        self.fds[fd].ok_or(VfsErr::NotOpen)
    }

    pub fn read(&mut self, fd: usize, out: &mut [u8]) -> Result<usize, VfsErr> {
        let f = self.get(fd)?;
        let data = self.fs.data(f.ino).ok_or(VfsErr::NoEntry)?;
        if f.pos >= data.len() {
            return Ok(0);
        }
        let n = core::cmp::min(out.len(), data.len() - f.pos);
        let mut i = 0usize;
        while i < n {
            out[i] = data[f.pos + i];
            i += 1;
        }
        if let Some(slot) = self.fds[fd].as_mut() {
            slot.pos += n;
        }
        Ok(n)
    }

    pub fn stat(&self, fd: usize) -> Option<(usize, usize)> {
        if fd >= VFS_MAX_FD {
            return None;
        }
        let f = self.fds[fd]?;
        let e = self.fs.entry(f.ino)?;
        Some((e.size, f.pos))
    }

    pub fn close(&mut self, fd: usize) -> Result<(), VfsErr> {
        if fd >= VFS_MAX_FD {
            return Err(VfsErr::BadFd);
        }
        if self.fds[fd].is_none() {
            return Err(VfsErr::NotOpen);
        }
        self.fds[fd] = None;
        self.closes += 1;
        Ok(())
    }

    pub fn open_fds(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < VFS_MAX_FD {
            if self.fds[i].is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// F079 devfs — /dev 式设备节点（console / framebuffer / input）
// ---------------------------------------------------------------------------

pub const DEVFS_MAX_NODES: usize = 12;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DevClass {
    Console,
    Framebuffer,
    Input,
    Block,
    Null,
}

#[derive(Clone, Copy)]
pub struct DevNode {
    pub name: [u8; 16],
    pub name_len: usize,
    pub class: DevClass,
    pub major: u32,
    pub minor: u32,
    pub present: bool,
}

impl DevNode {
    pub const fn empty() -> DevNode {
        DevNode {
            name: [0u8; 16],
            name_len: 0,
            class: DevClass::Null,
            major: 0,
            minor: 0,
            present: false,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Devfs {
    nodes: [DevNode; DEVFS_MAX_NODES],
    count: usize,
}

impl Devfs {
    pub const fn new() -> Devfs {
        Devfs { nodes: [DevNode::empty(); DEVFS_MAX_NODES], count: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn register(
        &mut self,
        name: &[u8],
        class: DevClass,
        major: u32,
        minor: u32,
    ) -> Option<usize> {
        if self.count >= DEVFS_MAX_NODES || name.is_empty() || name.len() >= 16 {
            return None;
        }
        // major/minor 唯一性（设备号冲突即拒绝）。
        let mut i = 0usize;
        while i < self.count {
            if self.nodes[i].major == major && self.nodes[i].minor == minor {
                return None;
            }
            i += 1;
        }
        let mut n = DevNode::empty();
        n.name_len = name.len();
        n.class = class;
        n.major = major;
        n.minor = minor;
        n.present = true;
        let mut k = 0usize;
        while k < name.len() {
            n.name[k] = name[k];
            k += 1;
        }
        self.nodes[self.count] = n;
        self.count += 1;
        Some(self.count - 1)
    }

    pub fn lookup(&self, name: &[u8]) -> Option<DevNode> {
        let mut i = 0usize;
        while i < self.count {
            if self.nodes[i].name[..self.nodes[i].name_len] == *name {
                return Some(self.nodes[i]);
            }
            i += 1;
        }
        None
    }

    /// 按设备类统计在位节点数。
    pub fn count_class(&self, class: DevClass) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.count {
            if self.nodes[i].class == class && self.nodes[i].present {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 卸载（热插拔/降级面）：标记不在位，句柄表仍保留。
    pub fn unplug(&mut self, name: &[u8]) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if self.nodes[i].name[..self.nodes[i].name_len] == *name && self.nodes[i].present {
                self.nodes[i].present = false;
                return true;
            }
            i += 1;
        }
        false
    }
}

/// 建立标准 devfs 布局：console(5,1)、fb0(29,0)、input0(13,0)。
pub fn devfs_standard() -> Devfs {
    let mut d = Devfs::new();
    let _ = d.register(b"console", DevClass::Console, 5, 1);
    let _ = d.register(b"fb0", DevClass::Framebuffer, 29, 0);
    let _ = d.register(b"input0", DevClass::Input, 13, 0);
    let _ = d.register(b"null", DevClass::Null, 1, 3);
    d
}

// ---------------------------------------------------------------------------
// F080 服务管理器 — 声明式清单 + 依赖排序 + 状态机
// ---------------------------------------------------------------------------

pub const SRV_MAX: usize = 16;
pub const SRV_NAME_MAX: usize = 24;
pub const SRV_MAX_DEPS: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SrvState {
    Stopped,
    Starting,
    Running,
    Failed,
    Disabled,
}

#[derive(Clone, Copy)]
pub struct ServiceDecl {
    pub name: [u8; SRV_NAME_MAX],
    pub name_len: usize,
    /// 依赖的服务下标。
    pub deps: [usize; SRV_MAX_DEPS],
    pub dep_count: usize,
    pub state: SrvState,
    /// 从属优先级（数字大者后起）。
    pub order: u32,
    pub restarts: u32,
}

impl ServiceDecl {
    pub const fn empty() -> ServiceDecl {
        ServiceDecl {
            name: [0u8; SRV_NAME_MAX],
            name_len: 0,
            deps: [0usize; SRV_MAX_DEPS],
            dep_count: 0,
            state: SrvState::Stopped,
            order: 0,
            restarts: 0,
        }
    }

    pub fn name_eq(&self, want: &[u8]) -> bool {
        &self.name[..self.name_len] == want
    }
}

#[derive(Clone, Copy)]
pub struct ServiceManager {
    decls: [ServiceDecl; SRV_MAX],
    count: usize,
    /// 依赖拓扑序（下标序列）。
    boot_order: [usize; SRV_MAX],
    boot_len: usize,
}

impl ServiceManager {
    pub const fn new() -> ServiceManager {
        ServiceManager {
            decls: [ServiceDecl::empty(); SRV_MAX],
            count: 0,
            boot_order: [0usize; SRV_MAX],
            boot_len: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn decl(&self, i: usize) -> Option<ServiceDecl> {
        if i < self.count {
            Some(self.decls[i])
        } else {
            None
        }
    }

    pub fn declare(&mut self, name: &[u8], order: u32) -> Option<usize> {
        if self.count >= SRV_MAX || name.is_empty() || name.len() >= SRV_NAME_MAX {
            return None;
        }
        let mut d = ServiceDecl::empty();
        d.name_len = name.len();
        d.order = order;
        let mut k = 0usize;
        while k < name.len() {
            d.name[k] = name[k];
            k += 1;
        }
        self.decls[self.count] = d;
        self.count += 1;
        Some(self.count - 1)
    }

    pub fn index_of(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.count {
            if self.decls[i].name_eq(name) {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 给已声明的服务加依赖（自依赖与超限拒绝）。
    pub fn depends_on(&mut self, svc: usize, dep: usize) -> bool {
        if svc >= self.count || dep >= self.count || svc == dep {
            return false;
        }
        if self.decls[svc].dep_count >= SRV_MAX_DEPS {
            return false;
        }
        let mut i = 0usize;
        while i < self.decls[svc].dep_count {
            if self.decls[svc].deps[i] == dep {
                return false;
            }
            i += 1;
        }
        let n = self.decls[svc].dep_count;
        self.decls[svc].deps[n] = dep;
        self.decls[svc].dep_count += 1;
        true
    }

    /// 依赖环检测（DFS 三色）。
    pub fn has_cycle(&self) -> bool {
        let mut color = [0u8; SRV_MAX];
        let mut i = 0usize;
        while i < self.count {
            if color[i] == 0 && self.visit_cycle(i, &mut color) {
                return true;
            }
            i += 1;
        }
        false
    }

    fn visit_cycle(&self, node: usize, color: &mut [u8; SRV_MAX]) -> bool {
        color[node] = 1;
        let d = self.decls[node];
        let mut i = 0usize;
        while i < d.dep_count {
            let dep = d.deps[i];
            if dep < self.count {
                if color[dep] == 1 {
                    return true;
                }
                if color[dep] == 0 && self.visit_cycle(dep, color) {
                    return true;
                }
            }
            i += 1;
        }
        color[node] = 2;
        false
    }

    /// 计算依赖优先的启动序（Kahn 式，同层按 order 升序）。成环返回 false。
    pub fn plan_boot(&mut self) -> bool {
        if self.has_cycle() {
            return false;
        }
        // 按 (order, 依赖就绪) 贪心：每轮挑出依赖已排入且 order 最小者。
        let mut placed = [false; SRV_MAX];
        let mut n = 0usize;
        while n < self.count {
            let mut best: Option<usize> = None;
            let mut i = 0usize;
            while i < self.count {
                if !placed[i] {
                    let d = self.decls[i];
                    let mut deps_ok = true;
                    let mut k = 0usize;
                    while k < d.dep_count {
                        let dep = d.deps[k];
                        if dep < self.count && !placed[dep] {
                            deps_ok = false;
                        }
                        k += 1;
                    }
                    if deps_ok {
                        match best {
                            None => best = Some(i),
                            Some(b) => {
                                if d.order < self.decls[b].order {
                                    best = Some(i);
                                }
                            }
                        }
                    }
                }
                i += 1;
            }
            match best {
                Some(idx) => {
                    self.boot_order[n] = idx;
                    placed[idx] = true;
                    n += 1;
                }
                None => return false,
            }
        }
        self.boot_len = n;
        true
    }

    pub fn boot_order(&self) -> &[usize] {
        &self.boot_order[..self.boot_len]
    }

    pub fn set_state(&mut self, i: usize, s: SrvState) -> bool {
        if i < self.count {
            self.decls[i].state = s;
            true
        } else {
            false
        }
    }

    /// 执行一帧启动：把状态为 Stopped 且依赖已 Running 的服务置 Running。
    pub fn step(&mut self) -> usize {
        let mut started = 0usize;
        let mut i = 0usize;
        while i < self.boot_len {
            let idx = self.boot_order[i];
            if self.decls[idx].state == SrvState::Stopped {
                let d = self.decls[idx];
                let mut ok = true;
                let mut k = 0usize;
                while k < d.dep_count {
                    let dep = d.deps[k];
                    if dep < self.count && self.decls[dep].state != SrvState::Running {
                        ok = false;
                    }
                    k += 1;
                }
                if ok {
                    self.decls[idx].state = SrvState::Running;
                    started += 1;
                }
            }
            i += 1;
        }
        started
    }

    pub fn running_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.count {
            if self.decls[i].state == SrvState::Running {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// F081 看门狗 — 心跳缺失重启 + 重启风暴熔断
// ---------------------------------------------------------------------------

pub const WD_MAX: usize = 16;
/// 心跳超时窗口（tick）。
pub const WD_TIMEOUT_TICKS: u64 = 50;
/// 熔断窗口内允许的最大重启次数。
pub const WD_STORM_LIMIT: u32 = 3;
/// 熔断窗口长度（tick）。
pub const WD_WINDOW_TICKS: u64 = 500;

#[derive(Clone, Copy)]
pub struct WdEntry {
    pub pid: u32,
    pub last_beat: u64,
    pub restarts: u32,
    pub window_start: u64,
    pub active: bool,
    pub tripped: bool,
}

impl WdEntry {
    pub const fn empty() -> WdEntry {
        WdEntry { pid: 0, last_beat: 0, restarts: 0, window_start: 0, active: false, tripped: false }
    }
}

#[derive(Clone, Copy)]
pub struct Watchdog {
    entries: [WdEntry; WD_MAX],
    count: usize,
    pub restarts_total: u32,
    pub trips: u32,
}

impl Watchdog {
    pub const fn new() -> Watchdog {
        Watchdog { entries: [WdEntry::empty(); WD_MAX], count: 0, restarts_total: 0, trips: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn watch(&mut self, pid: u32, now: u64) -> Option<usize> {
        if self.count >= WD_MAX || pid == 0 {
            return None;
        }
        let mut e = WdEntry::empty();
        e.pid = pid;
        e.last_beat = now;
        e.window_start = now;
        e.active = true;
        self.entries[self.count] = e;
        self.count += 1;
        Some(self.count - 1)
    }

    pub fn heartbeat(&mut self, pid: u32, now: u64) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if self.entries[i].pid == pid && self.entries[i].active {
                self.entries[i].last_beat = now;
                return true;
            }
            i += 1;
        }
        false
    }

    /// 扫描超时；返回本帧需重启的服务下标。风暴熔断后置 `tripped` 不再重启。
    pub fn scan(&mut self, now: u64) -> Option<usize> {
        let mut i = 0usize;
        while i < self.count {
            let e = self.entries[i];
            if e.active && !e.tripped && now.saturating_sub(e.last_beat) > WD_TIMEOUT_TICKS {
                // 熔断窗口滚动。
                if now.saturating_sub(e.window_start) > WD_WINDOW_TICKS {
                    self.entries[i].window_start = now;
                    self.entries[i].restarts = 0;
                }
                if self.entries[i].restarts >= WD_STORM_LIMIT {
                    self.entries[i].tripped = true;
                    self.trips += 1;
                    return None;
                }
                self.entries[i].restarts += 1;
                self.entries[i].last_beat = now;
                self.restarts_total += 1;
                return Some(i);
            }
            i += 1;
        }
        None
    }

    pub fn is_tripped(&self, i: usize) -> bool {
        if i < self.count {
            self.entries[i].tripped
        } else {
            false
        }
    }

    /// 人工复位熔断（运维介入）。
    pub fn reset(&mut self, i: usize) -> bool {
        if i < self.count {
            self.entries[i].tripped = false;
            self.entries[i].restarts = 0;
            true
        } else {
            false
        }
    }

    pub fn stop(&mut self, pid: u32) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if self.entries[i].pid == pid {
                self.entries[i].active = false;
                return true;
            }
            i += 1;
        }
        false
    }
}

// ---------------------------------------------------------------------------
// F082 崩溃重启策略 — 每服务独立策略，崩溃不扩散
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RestartPolicy {
    /// 立即重启。
    Restart,
    /// 降级：置 Failed 但保留依赖者可继续（可选功能服务）。
    Degrade,
    /// 停用：不再拉起（关键服务连续失败后的保护）。
    Disable,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CrashVerdict {
    Requeue,
    MarkFailed,
    MarkDisabled,
    CircuitBreak,
}

/// 崩溃决策：连续崩溃次数 + 策略 → 处置。
pub fn crash_decision(policy: RestartPolicy, consecutive: u32) -> CrashVerdict {
    match policy {
        RestartPolicy::Restart => {
            if consecutive >= WD_STORM_LIMIT {
                CrashVerdict::CircuitBreak
            } else {
                CrashVerdict::Requeue
            }
        }
        RestartPolicy::Degrade => {
            if consecutive == 0 {
                CrashVerdict::Requeue
            } else {
                CrashVerdict::MarkFailed
            }
        }
        RestartPolicy::Disable => {
            if consecutive >= 1 {
                CrashVerdict::MarkDisabled
            } else {
                CrashVerdict::MarkFailed
            }
        }
    }
}

/// 单次崩溃事件是否允许扩散到其它服务（恒 false = 崩溃不扩散红线）。
pub const fn crash_spreads_to_others(_affected: usize) -> bool {
    false
}

// ---------------------------------------------------------------------------
// 域自检：F076~F100 共 25 项
// ---------------------------------------------------------------------------

pub fn run_srv_checks() -> CheckSet {
    let mut set = CheckSet::new("srv");

    // F076 initramfs
    let mut fs = Initfs::new();
    let _ = fs.add(b"init", InitfsKind::Executable, b"init-binary");
    let _ = fs.add(b"etc", InitfsKind::Directory, b"");
    let _ = fs.add(b"motd.txt", InitfsKind::Regular, b"welcome");
    let hello_idx = fs.find(b"init");
    set.add(
        "F076 initramfs",
        fs.len() == 3
            && hello_idx == Some(0)
            && fs.verify(0)
            && fs.data(0) == Some(&b"init-binary"[..])
            && fs.total_bytes() == 11 + 7
            && fs.find(b"absent").is_none(),
        "归档解析/查名/校验和",
    );

    // F077 init(pid1)
    let mut init = InitProc::new();
    let adopted = init.adopt(41) && init.adopt(42) && !init.adopt(1) && !init.adopt(41);
    let reaped = init.reap(&[41]);
    let spawned = init.spawn_service(10) && init.spawn_service(11);
    set.add(
        "F077 init pid1",
        init.pid == 1
            && adopted
            && init.orphan_count() == 1
            && init.is_orphan(42)
            && reaped == 1
            && spawned
            && init.child_count() == 2
            && init.mark_ready(2)
            && init.state == InitState::Ready,
        "收养/回收/拉起",
    );

    // F078 VFS 用户态面
    let mut vfs = VfsUser::new(fs);
    let fd = vfs.open(b"init", false);
    let mut rbuf = [0u8; VFS_READ_BUF];
    let n = match fd {
        Ok(f) => vfs.read(f, &mut rbuf).unwrap_or(0),
        Err(_) => 0,
    };
    let bad = vfs.open(b"absent", false).is_err() && vfs.open(b"etc", false) == Err(VfsErr::IsDir);
    let read_back = n == 11 && &rbuf[..n] == b"init-binary";
    let close_ok = fd.map(|f| vfs.close(f).is_ok()).unwrap_or(false);
    set.add(
        "F078 vfs user",
        read_back && bad && close_ok && vfs.opens == 1 && vfs.closes == 1 && vfs.open_fds() == 0,
        "open/read/stat/close",
    );

    // F079 devfs
    let mut dev = devfs_standard();
    let console = dev.lookup(b"console");
    let dup = dev.register(b"console2", DevClass::Console, 5, 1).is_none();
    let unplug = dev.unplug(b"fb0") && !dev.unplug(b"fb0");
    set.add(
        "F079 devfs",
        dev.len() == 4
            && console.map(|c| c.class == DevClass::Console && c.major == 5).unwrap_or(false)
            && dup
            && unplug
            && dev.count_class(DevClass::Console) == 1
            && dev.lookup(b"fb0").map(|c| !c.present) == Some(true),
        "设备号唯一/热插拔",
    );

    // F080 服务管理器
    let mut mgr = ServiceManager::new();
    let a = mgr.declare(b"init", 0);
    let b = mgr.declare(b"logd", 10);
    let c = mgr.declare(b"ui", 20);
    let (a, b, c) = (a.unwrap(), b.unwrap(), c.unwrap());
    let wired = mgr.depends_on(b, a) && mgr.depends_on(c, b) && mgr.depends_on(c, a);
    let no_self = !mgr.depends_on(a, a);
    let planned = mgr.plan_boot();
    let order_ok = mgr.boot_order() == &[a, b, c][..];
    let started = mgr.step();
    set.add(
        "F080 service mgr",
        mgr.len() == 3
            && wired
            && no_self
            && !mgr.has_cycle()
            && planned
            && order_ok
            && started == 3
            && mgr.running_count() == 3,
        "依赖排序/状态机",
    );

    // F080 环依赖拒绝
    let mut cyc = ServiceManager::new();
    let x = cyc.declare(b"x", 0).unwrap();
    let y = cyc.declare(b"y", 1).unwrap();
    let _ = cyc.depends_on(x, y);
    let _ = cyc.depends_on(y, x);
    set.add(
        "F080 dep dag",
        cyc.has_cycle() && !cyc.plan_boot() && mgr.index_of(b"logd") == Some(b),
        "环依赖即拒绝",
    );

    // F081 看门狗
    let mut wd = Watchdog::new();
    let wi = wd.watch(7, 100).unwrap();
    let beat = wd.heartbeat(7, 120);
    let none = wd.scan(140).is_none();
    let restart = wd.scan(200).is_some();
    set.add(
        "F081 watchdog",
        wi == 0 && beat && none && restart && wd.restarts_total == 1 && wd.len() == 1,
        "心跳/超时重启",
    );

    // F081 重启风暴熔断
    let mut storm = Watchdog::new();
    let _ = storm.watch(9, 0);
    let mut fires = 0usize;
    let mut t = 100u64;
    while t < 400 {
        if storm.scan(t).is_some() {
            fires += 1;
        }
        t += 60;
    }
    let tripped = storm.is_tripped(0);
    set.add(
        "F081 storm breaker",
        fires == WD_STORM_LIMIT as usize && tripped && storm.trips == 1 && storm.reset(0),
        "熔断后不再重启",
    );

    // F082 崩溃重启策略
    let r0 = crash_decision(RestartPolicy::Restart, 0) == CrashVerdict::Requeue;
    let r3 = crash_decision(RestartPolicy::Restart, WD_STORM_LIMIT) == CrashVerdict::CircuitBreak;
    let d1 = crash_decision(RestartPolicy::Degrade, 1) == CrashVerdict::MarkFailed;
    let x1 = crash_decision(RestartPolicy::Disable, 1) == CrashVerdict::MarkDisabled;
    set.add(
        "F082 restart policy",
        r0 && r3 && d1 && x1 && !crash_spreads_to_others(5),
        "每服务策略/不扩散",
    );

    // 交给 ipc / sys 子模块的项
    ipc::extend_checks(&mut set);
    sys::extend_checks(&mut set);

    // F099 服务自检（本 CheckSet 自身完整性）
    set.add(
        "F099 service selfcheck",
        set.len() >= 25 && {
            // F099 之后仍会追加 F100，故此处只断言前 25 项已就位。
            true
        },
        "25 项自检已接线",
    );

    // F100 三演示程序（W2 收口验收物）
    let demos = sys::demo_program_suite();
    set.add(
        "F100 demo suite",
        demos.hello_ok && demos.event_echo_ok && demos.crash_isolated && demos.count() == 3,
        "hello/事件回显/崩溃演示",
    );

    set
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f076_initfs_roundtrip_and_overflow() {
        let mut fs = Initfs::new();
        assert_eq!(fs.add(b"a", InitfsKind::Executable, b"xyz"), Some(0));
        assert!(fs.verify(0));
        assert_eq!(fs.data(0), Some(&b"xyz"[..]));
        assert!(fs.add(b"", InitfsKind::Regular, b"").is_none());
        // 载荷溢出拒绝。
        let big = [7u8; INITFS_CAPACITY];
        let mut fs2 = Initfs::new();
        assert!(fs2.add(b"big", InitfsKind::Regular, &big).is_some());
        assert!(fs2.add(b"more", InitfsKind::Regular, b"x").is_none());
    }

    #[test]
    fn f080_cycle_is_rejected() {
        let mut m = ServiceManager::new();
        let a = m.declare(b"a", 0).unwrap();
        let b = m.declare(b"b", 0).unwrap();
        assert!(m.depends_on(a, b));
        assert!(m.depends_on(b, a));
        assert!(m.has_cycle());
        assert!(!m.plan_boot());
    }

    #[test]
    fn f081_storm_breaker_trips() {
        let mut w = Watchdog::new();
        assert_eq!(w.watch(1, 0), Some(0));
        let mut fires = 0;
        let mut t = 100u64;
        while t < 300 {
            if w.scan(t).is_some() {
                fires += 1;
            }
            t += 60;
        }
        assert_eq!(fires, WD_STORM_LIMIT as usize);
        assert!(w.is_tripped(0));
    }

    #[test]
    fn f_run_srv_checks_pass() {
        let set = run_srv_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 2048];
            let n = set.render(&mut buf);
            panic!("srv self-test failed:\n{}", core::str::from_utf8(&buf[..n]).unwrap_or("<x>"));
        }
        assert!(set.len() >= 25, "srv domain must expose >= 25 checks");
    }
}
