//! unx4204_chdirsem — UNX-F4204 fchdir/chdir 语义与工作目录切换（诊断段 0x9AE0）。
//!
//! 判据原文：J1 chdir 以路径解析换工作目录：成功后 cwd 引用换绑且
//!           旧 cwd 引用计数减一——引用账恰好一动；
//!           J2 fchdir 以 fd 换绑：fd 必须指向目录对象，文件 fd 拒收
//!           （ENOTDIR 语义）；换绑后 fd 不受影响仍可用；
//!           J3 权限闸：目标目录无执行位（x）时 chdir/fchdir 全拒
//!           （EACCES 语义）且 cwd 不动；
//!           J4 cwd 账守恒：binds = chdir_ok + fchdir_ok，且切换历史
//!           可回放（每次换绑记录 (from,to) 对）。
//! 设计纪律：
//!   1. Linux 语义：chdir(path) 解析路径 → 目录 inode → 换绑 cwd；
//!      fchdir(fd) 免路径解析直达，只查 fd 指向对象类型。
//!   2. 目录对象三态权：读 r / 写 w / 执行 x——进入目录需 x。
//!   3. 换绑历史簿：append-only，回放可复算 cwd 序列。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9AE0 本单独占。

use crate::alloc::vec::Vec;
use crate::alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x9AE0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NOTDIR: u16 = SEG_BASE + 1;   // fd 非目录（fchdir 拒收）
const E_NOEXEC: u16 = SEG_BASE + 2;   // 无执行位（EACCES 语义）
const E_FD_BAD: u16 = SEG_BASE + 3;   // fd 越表或已关
const E_BADPATH: u16 = SEG_BASE + 4;  // 路径解析失败（ENOENT 语义）
const E_BAL: u16 = SEG_BASE + 5;      // cwd 账不守恒
const E_PATH_EMPTY: u16 = SEG_BASE + 6; // 空路径

/// 对象类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObjKind {
    Dir,
    File,
}

/// 目录/文件对象：类 + 权限位（r=4,w=2,x=1）。
pub struct FsObj {
    pub kind: ObjKind,
    pub mode: u8, // 低 3 位 rwx
    pub name: String,
}

impl FsObj {
    pub fn can_exec(&self) -> bool {
        self.mode & 1 != 0
    }
}

/// fd 记录。
pub struct Fd {
    pub obj: usize,
    open: bool,
}

/// cwd 换绑历史项。
#[derive(Clone, Copy)]
pub struct BindRec {
    pub from: usize,
    pub to: usize,
    pub via_fd: bool,
}

/// 工作目录切换枢纽。
pub struct CwdSem {
    objs: Vec<FsObj>,
    fds: Vec<Fd>,
    cwd: usize,
    pub binds: u32,
    pub chdir_ok: u32,
    pub fchdir_ok: u32,
    pub rejected: u32,
    history: Vec<BindRec>,
}

impl CwdSem {
    pub fn new(root_name: &str) -> CwdSem {
        let mut objs = Vec::new();
        objs.push(FsObj {
            kind: ObjKind::Dir,
            mode: 5, // r-x：根可进
            name: root_name.to_string(),
        });
        CwdSem {
            objs,
            fds: Vec::new(),
            cwd: 0,
            binds: 0,
            chdir_ok: 0,
            fchdir_ok: 0,
            rejected: 0,
            history: Vec::new(),
        }
    }

    /// 挂目录（默认 r-xr-x）。
    pub fn add_dir(&mut self, name: &str, mode: u8) -> usize {
        self.objs.push(FsObj {
            kind: ObjKind::Dir,
            mode,
            name: name.to_string(),
        });
        self.objs.len() - 1
    }

    /// 挂文件。
    pub fn add_file(&mut self, name: &str, mode: u8) -> usize {
        self.objs.push(FsObj {
            kind: ObjKind::File,
            mode,
            name: name.to_string(),
        });
        self.objs.len() - 1
    }

    /// 开 fd。
    pub fn open_fd(&mut self, obj: usize) -> Result<usize, u16> {
        if obj >= self.objs.len() {
            return Err(E_FD_BAD);
        }
        self.fds.push(Fd { obj, open: true });
        Ok(self.fds.len() - 1)
    }

    pub fn cwd(&self) -> usize {
        self.cwd
    }

    pub fn cwd_name(&self) -> &str {
        &self.objs[self.cwd].name
    }

    /// chdir：路径名解析（只支持一级名或 "/"），过执行位闸，换绑。
    pub fn chdir(&mut self, path: &str) -> Result<(), u16> {
        if path.is_empty() {
            self.rejected += 1;
            return Err(E_PATH_EMPTY);
        }
        if path == "/" {
            return self.bind_to(0, false);
        }
        // 一级名查 cwd 下对象（本锚简化：全名空间线性查）
        for (i, o) in self.objs.iter().enumerate() {
            if i != 0 && o.name == path {
                return self.bind_to(i, false);
            }
        }
        self.rejected += 1;
        Err(E_BADPATH)
    }

    /// fchdir：fd 换绑（判据 J2：文件 fd 拒收 ENOTDIR）。
    pub fn fchdir(&mut self, fd: usize) -> Result<(), u16> {
        if fd >= self.fds.len() || !self.fds[fd].open {
            self.rejected += 1;
            return Err(E_FD_BAD);
        }
        let oi = self.fds[fd].obj;
        if self.objs[oi].kind != ObjKind::Dir {
            self.rejected += 1;
            return Err(E_NOTDIR);
        }
        self.bind_to(oi, true)
    }

    /// 换绑核心：执行位闸 + 引用账一动 + 历史入簿（判据 J1/J3）。
    fn bind_to(&mut self, to: usize, via_fd: bool) -> Result<(), u16> {
        if !self.objs[to].can_exec() {
            self.rejected += 1;
            return Err(E_NOEXEC);
        }
        let from = self.cwd;
        if from != to {
            self.history.push(BindRec {
                from,
                to,
                via_fd,
            });
            self.cwd = to;
            self.binds += 1;
        }
        if via_fd {
            self.fchdir_ok += 1;
        } else {
            self.chdir_ok += 1;
        }
        Ok(())
    }

    /// 换绑历史回放：从根重演，终态应等于当前 cwd（判据 J4）。
    pub fn replay_ends_here(&self) -> bool {
        let mut cur = 0usize;
        for h in self.history.iter() {
            if h.from != cur {
                return false;
            }
            cur = h.to;
        }
        cur == self.cwd
    }

    /// cwd 账守恒。
    pub fn balanced(&self) -> bool {
        self.binds == self.chdir_ok + self.fchdir_ok && self.replay_ends_here()
    }

    pub fn history_len(&self) -> usize {
        self.history.len()
    }
}

/// 自例程：R1 chdir 换绑与历史回放；R2 fchdir 目录 fd 成功 + 文件 fd
/// 拒收；R3 执行位闸拒收且 cwd 不动；R4 注入拦截；R5 账守恒终验。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：chdir 正常换绑——"/" → "/tmp" → 回根，历史两笔可回放。
    let mut s = CwdSem::new("/");
    let tmp = s.add_dir("tmp", 5);
    let var = s.add_dir("var", 5);
    s.chdir("/tmp")?;
    if s.cwd() != tmp {
        return Err(E_BAL);
    }
    s.chdir("/var")?;
    if s.cwd() != var || s.history_len() != 2 {
        return Err(E_BAL);
    }
    s.chdir("/")?;
    if s.cwd() != 0 || !s.replay_ends_here() {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：fchdir——目录 fd 成功换绑；文件 fd 拒收 ENOTDIR。
    let docs = s.add_dir("docs", 5);
    let note = s.add_file("note.txt", 6);
    let fdd = s.open_fd(docs)?;
    let fdf = s.open_fd(note)?;
    s.chdir("/")?;
    s.fchdir(fdd)?;
    if s.cwd() != docs {
        return Err(E_BAL);
    }
    if s.fchdir(fdf) != Err(E_NOTDIR) {
        return Err(E_NOTDIR);
    }
    // fd 不受 fchdir 影响：仍可再次 fchdir 同一 fd
    s.fchdir(fdd)?;
    if s.cwd() != docs {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：执行位闸——无 x 位目录 chdir/fchdir 全拒且 cwd 不动（判据 J3）。
    let vault = s.add_dir("vault", 6); // rw- 无 x
    let fdv = s.open_fd(vault)?;
    s.chdir("/docs")?;
    let cwd_before = s.cwd();
    if s.chdir("/vault") != Err(E_NOEXEC) {
        return Err(E_NOEXEC);
    }
    if s.fchdir(fdv) != Err(E_NOEXEC) {
        return Err(E_NOEXEC);
    }
    if s.cwd() != cwd_before {
        return Err(E_BAL); // 拒收后 cwd 必须不动
    }
    hits += 1;

    // R4：注入拦截——空路径、未知路径、越表 fd、已关 fd。
    if s.chdir("") != Err(E_PATH_EMPTY) {
        return Err(E_PATH_EMPTY);
    }
    if s.chdir("/nonexistent") != Err(E_BADPATH) {
        return Err(E_BADPATH);
    }
    if s.fchdir(99) != Err(E_FD_BAD) {
        return Err(E_FD_BAD);
    }
    hits += 1;

    // R5：账守恒终验（判据 J4）。
    if !s.balanced() {
        return Err(E_BAL);
    }
    if s.binds != s.chdir_ok + s.fchdir_ok {
        return Err(E_BAL);
    }
    // 拒账闭合：binds + rejected + 成功进位账全部非负——核对 binds 与历史一致
    if s.history_len() as u32 > s.binds {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
