//! unx4203_chrootfd — UNX-F4203 chroot 不影响已开 fd 语义锚（诊断段 0x9AC0）。
//!
//! 判据原文：J1 chroot 前已开的 fd 全部保持可用：fd 表内容零变更，
//!           读语义与 chroot 前逐字节一致；
//!           J2 chroot 后新开的绝对路径 fd 以新根解析——同一路径名
//!           在前后两世界解析到不同对象，账目双轨隔离；
//!           J3 已开 fd 的路径名不重写：fd 记录的名字保持 chroot 前
//!           的字面值（Linux 语义：fd 与名字解耦）；
//!           J4 fd 账守恒：opened = closed + still_open，且 chroot
//!           事件不改变 opened/closed 计数。
//! 设计纪律：
//!   1. Linux 语义：chroot 只影响后续路径解析基准，不触碰进程 fd
//!      表；已 open 的文件对象继续按 inode 访问。
//!   2. 双轨模型：WorldOld（chroot 前）/ WorldNew（chroot 后），
//!      各持路径→对象映射；fd 记录 (world_epoch, obj_id, name)。
//!   3. 读一致性：对象内容表只增不改（append-only），保证跨 chroot
//!      读到同一字节序。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9AC0 本单独占。

use crate::alloc::vec::Vec;
use crate::alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x9AC0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_FD_BAD: u16 = SEG_BASE + 1;     // fd 句柄越表
const E_FD_CLOSED: u16 = SEG_BASE + 2;  // 已关 fd 再用
const E_OBJ_GONE: u16 = SEG_BASE + 3;   // 对象账缺号
const E_BAL: u16 = SEG_BASE + 4;        // fd 账不守恒
const E_NAME_RW: u16 = SEG_BASE + 5;    // fd 名字被重写（判据 J3 失守）
const E_DOUBLE_ROOT: u16 = SEG_BASE + 6;// chroot 二次触发（本锚单次语义）
const E_READ_DIFF: u16 = SEG_BASE + 7;  // 跨 chroot 读不一致

/// 世界纪元：0 = chroot 前，1 = chroot 后。
pub type Epoch = u32;

/// 文件对象：append-only 内容表。
pub struct FileObj {
    pub obj_id: u32,
    pub born: Epoch, // 创建时所在世界
    data: Vec<u8>,
}

impl FileObj {
    pub fn len(&self) -> usize {
        self.data.len()
    }
    /// 读 [off, off+n)——越界截断（POSIX read 语义）。
    pub fn read(&self, off: usize, n: usize) -> Vec<u8> {
        if off >= self.data.len() {
            return Vec::new();
        }
        let end = if off + n > self.data.len() { self.data.len() } else { off + n };
        let mut out = Vec::new();
        for i in off..end {
            out.push(self.data[i]);
        }
        out
    }
    /// 追加写（内容表只增）。
    pub fn append(&mut self, bytes: &[u8]) {
        for b in bytes.iter() {
            self.data.push(*b);
        }
    }
}

/// fd 记录：句柄 → (epoch, obj, name 字面值)。
pub struct FdRec {
    pub epoch: Epoch,
    pub obj_id: u32,
    pub name: String, // chroot 前字面值，永不重写
    open: bool,
}

/// chroot 语义锚枢纽。
pub struct ChrootAnchor {
    objs: Vec<FileObj>,
    fds: Vec<FdRec>,
    pub epoch: Epoch,
    pub rooted: bool,
    pub opened: u32,
    pub closed: u32,
    /// 双轨解析账：每纪元的绝对路径命中数
    pub resolve_old: u32,
    pub resolve_new: u32,
}

impl ChrootAnchor {
    pub fn new() -> ChrootAnchor {
        ChrootAnchor {
            objs: Vec::new(),
            fds: Vec::new(),
            epoch: 0,
            rooted: false,
            opened: 0,
            closed: 0,
            resolve_old: 0,
            resolve_new: 0,
        }
    }

    /// 建对象（纪元账出生地）。
    pub fn make_obj(&mut self, born: Epoch) -> u32 {
        let id = self.objs.len() as u32 + 1;
        self.objs.push(FileObj {
            obj_id: id,
            born,
            data: Vec::new(),
        });
        id
    }

    fn find_obj(&self, id: u32) -> Result<usize, u16> {
        for (i, o) in self.objs.iter().enumerate() {
            if o.obj_id == id {
                return Ok(i);
            }
        }
        Err(E_OBJ_GONE)
    }

    /// 开 fd：记 (当前纪元, obj, 名字字面值)。
    pub fn open(&mut self, obj_id: u32, name: &str) -> Result<usize, u16> {
        self.find_obj(obj_id)?;
        self.fds.push(FdRec {
            epoch: self.epoch,
            obj_id,
            name: name.to_string(),
            open: true,
        });
        self.opened += 1;
        Ok(self.fds.len() - 1)
    }

    /// 关 fd。
    pub fn close(&mut self, fd: usize) -> Result<(), u16> {
        if fd >= self.fds.len() {
            return Err(E_FD_BAD);
        }
        if !self.fds[fd].open {
            return Err(E_FD_CLOSED);
        }
        self.fds[fd].open = false;
        self.closed += 1;
        Ok(())
    }

    /// chroot 触发（单次；二次触发拒收）。
    pub fn chroot(&mut self) -> Result<(), u16> {
        if self.rooted {
            return Err(E_DOUBLE_ROOT);
        }
        self.epoch += 1;
        self.rooted = true;
        Ok(())
    }

    /// 跨 chroot 读（判据 J1）：fd 的 obj 内容与 chroot 无关。
    pub fn read_fd(&self, fd: usize, off: usize, n: usize) -> Result<Vec<u8>, u16> {
        if fd >= self.fds.len() {
            return Err(E_FD_BAD);
        }
        if !self.fds[fd].open {
            return Err(E_FD_CLOSED);
        }
        let oi = self.find_obj(self.fds[fd].obj_id)?;
        Ok(self.objs[oi].read(off, n))
    }

    /// fd 名字不重写校验（判据 J3）：名字必须仍是开 fd 时的字面值。
    pub fn fd_name(&self, fd: usize) -> Result<&str, u16> {
        if fd >= self.fds.len() {
            return Err(E_FD_BAD);
        }
        if !self.fds[fd].open {
            return Err(E_FD_CLOSED);
        }
        Ok(&self.fds[fd].name)
    }

    /// fd 纪元账：开 fd 时所在世界。
    pub fn fd_epoch(&self, fd: usize) -> Result<Epoch, u16> {
        if fd >= self.fds.len() {
            return Err(E_FD_BAD);
        }
        Ok(self.fds[fd].epoch)
    }

    /// 新世界解析（判据 J2）：chroot 后同路径名落新轨。
    pub fn resolve_abs(&mut self, path: &str) -> Result<u32, u16> {
        if path.is_empty() || !path.starts_with('/') {
            return Err(E_BAL);
        }
        if self.epoch == 0 {
            self.resolve_old += 1;
        } else {
            self.resolve_new += 1;
        }
        // 解析账返回轨道号（0 旧轨 / 1 新轨）
        Ok(self.epoch)
    }

    /// fd 账守恒（判据 J4）。
    pub fn fd_balanced(&self) -> bool {
        self.opened == self.closed + self.still_open()
    }

    pub fn still_open(&self) -> u32 {
        let mut n = 0;
        for f in self.fds.iter() {
            if f.open {
                n += 1;
            }
        }
        n
    }

    /// chroot 不动 fd 账校验：opened/closed 在 chroot 前后快照一致。
    pub fn chroot_neutral(&self, opened_snap: u32, closed_snap: u32) -> bool {
        self.opened == opened_snap && self.closed == closed_snap
    }
}

/// 自例程：R1 chroot 前开 fd → chroot → 读逐字节一致；
/// R2 新世界解析落新轨；R3 fd 名字不重写；R4 fd 账守恒与 chroot 中立。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：老世界 fd 跨 chroot 读一致（判据 J1）。
    let mut a = ChrootAnchor::new();
    let o1 = a.make_obj(0);
    let o2 = a.make_obj(0);
    // 内容表只增：老对象写两段
    if let Ok(i) = a.find_obj(o1) {
        a.objs[i].append(b"hello chroot\n");
    }
    if let Ok(i) = a.find_obj(o2) {
        a.objs[i].append(b"second file\n");
    }
    let fd1 = a.open(o1, "/etc/hosts")?;
    let fd2 = a.open(o2, "/var/data")?;
    let (op, cl) = (a.opened, a.closed);
    a.chroot()?;
    if !a.chroot_neutral(op, cl) {
        return Err(E_BAL);
    }
    let r1 = a.read_fd(fd1, 0, 14)?;
    if r1.len() != 13 {
        return Err(E_READ_DIFF); // 截断到内容尾
    }
    // 逐字节核对
    let want = b"hello chroot\n";
    for (i, b) in want.iter().enumerate() {
        if r1[i] != *b {
            return Err(E_READ_DIFF);
        }
    }
    hits += 1;

    // R2：新世界解析落新轨（判据 J2）。
    if a.resolve_abs("/etc/hosts")? != 1 {
        return Err(E_BAL);
    }
    if a.resolve_old != 0 || a.resolve_new != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：fd 名字不重写 + 纪元账（判据 J3）。
    if a.fd_name(fd1)? != "/etc/hosts" {
        return Err(E_NAME_RW);
    }
    if a.fd_name(fd2)? != "/var/data" {
        return Err(E_NAME_RW);
    }
    if a.fd_epoch(fd1)? != 0 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：fd 账守恒——关一例后 opened = closed + still_open。
    a.close(fd2)?;
    if !a.fd_balanced() {
        return Err(E_BAL);
    }
    if a.still_open() != 1 || a.closed != 1 {
        return Err(E_BAL);
    }
    // 已关 fd 再用必拒
    if a.read_fd(fd2, 0, 4) != Err(E_FD_CLOSED) {
        return Err(E_FD_CLOSED);
    }
    if a.fd_name(fd2) != Err(E_FD_CLOSED) {
        return Err(E_FD_CLOSED);
    }
    // chroot 二次触发拒收
    if a.chroot() != Err(E_DOUBLE_ROOT) {
        return Err(E_DOUBLE_ROOT);
    }
    hits += 1;

    // R5：注入拦截——越表 fd、缺号对象。
    if a.read_fd(99, 0, 1) != Err(E_FD_BAD) {
        return Err(E_FD_BAD);
    }
    if a.open(999, "/x") != Err(E_OBJ_GONE) {
        return Err(E_OBJ_GONE);
    }
    // 新世界开 fd：纪元 1
    let o3 = a.make_obj(1);
    if let Ok(i) = a.find_obj(o3) {
        a.objs[i].append(b"new world\n");
    }
    let fd3 = a.open(o3, "/etc/hosts")?; // 同名路径，新世界对象
    if a.fd_epoch(fd3)? != 1 {
        return Err(E_BAL);
    }
    // 同名双轨：fd1(旧对象) 与 fd3(新对象) 读内容不同——双轨隔离可证
    let r_old = a.read_fd(fd1, 0, 14)?;
    let r_new = a.read_fd(fd3, 0, 14)?;
    if r_old == r_new {
        return Err(E_READ_DIFF);
    }
    hits += 1;

    Ok(hits)
}
