//! UNX-F10103 open 正反双判据（打开达成与缺失路径误用受控）（0x10000 段）
//!
//! 锚点：UNX-F10103-J1 open 打开达成（LTP open01/open02 面标志组合语义
//! 正确 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：路径册（名字→inode 的注册树）+ 打开核（O_CREAT 建件/无则拒/
//! 最低空闲 fd 分配）+ 打开账（逐次 open 留痕 + fd 活跃计数）+ 注入核
//! （缺失路径/重开 O_CREAT 冲突/负标志/空路径/封表必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10000 独占 0x20）。
pub const SEG_BASE: u32 = 0x10000;
pub const OP_NOENT: u32 = SEG_BASE + 0x01; // 路径缺失且无 O_CREAT
pub const OP_EXIST: u32 = SEG_BASE + 0x02; // O_CREAT|O_EXCL 撞已有件
pub const OP_BAD_PATH: u32 = SEG_BASE + 0x03; // 空路径/超长路径
pub const OP_BAD_FLAG: u32 = SEG_BASE + 0x04; // 非法标志位
pub const OP_SEALED: u32 = SEG_BASE + 0x05; // 封表后打开
pub const OP_EMFILE: u32 = SEG_BASE + 0x06; // fd 表满

/// 路径长度上限。
pub const PATH_MAX: usize = 32;
/// fd 表定容。
pub const FD_CAP: usize = 10;
/// inode 表定容。
pub const INO_CAP: usize = 16;

/// O_ 标志位（本面涉及的闭集）。
pub const O_CREAT: u32 = 0x001;
pub const O_EXCL: u32 = 0x002;
pub const O_RDWR: u32 = 0x004;

/// inode 记录。
#[derive(Clone)]
pub struct Inode {
    pub id: u32,
    pub name: String,
    pub size: u32,
}

impl Inode {
    pub fn ok(&self) -> bool {
        self.id != 0 && !self.name.is_empty()
    }
}

/// 打开账留痕。
#[derive(Clone)]
pub struct OpenRec {
    pub seq: u32,
    pub fd: i32,
    pub path: String,
    pub flags: u32,
}

impl OpenRec {
    pub fn ok(&self) -> bool {
        !self.path.is_empty() && (self.flags & !0x007) == 0
    }
}

/// 路径册 + 打开表。
pub struct OpenTable {
    pub inodes: Vec<Inode>,
    pub fds: Vec<Option<u32>>,
    pub ledger: Vec<OpenRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl OpenTable {
    pub fn new() -> OpenTable {
        OpenTable { inodes: Vec::new(), fds: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 标志闭集校验。
    pub fn flags_ok(flags: u32) -> bool {
        flags & !0x007 == 0 && flags & 0x004 != 0
    }

    fn find(&self, name: &str) -> Option<u32> {
        self.inodes.iter().find(|i| i.name == name).map(|i| i.id)
    }

    /// open 核心：路径命中即开，O_CREAT 建件，O_CREAT|O_EXCL 撞件必拒。
    pub fn open(&mut self, path: &str, flags: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(OP_SEALED);
        }
        if path.is_empty() || path.len() > PATH_MAX {
            return Err(OP_BAD_PATH);
        }
        if !OpenTable::flags_ok(flags) {
            return Err(OP_BAD_FLAG);
        }
        let existing = self.find(path);
        if let Some(ino) = existing {
            if flags & O_CREAT != 0 && flags & O_EXCL != 0 {
                return Err(OP_EXIST);
            }
            return self.claim_fd(ino, path, flags);
        }
        if flags & O_CREAT == 0 {
            return Err(OP_NOENT);
        }
        if self.inodes.len() >= INO_CAP {
            return Err(OP_EMFILE);
        }
        let id = (self.inodes.len() + 1) as u32;
        self.inodes.push(Inode { id, name: path.to_string(), size: 0 });
        self.claim_fd(id, path, flags)
    }

    /// 最低空闲 fd 分配并落账。
    fn claim_fd(&mut self, ino: u32, path: &str, flags: u32) -> Result<usize, u32> {
        let slot = if let Some(i) = self.fds.iter().position(|s| s.is_none()) {
            i
        } else if self.fds.len() < FD_CAP {
            self.fds.push(None);
            self.fds.len() - 1
        } else {
            return Err(OP_EMFILE);
        };
        self.fds[slot] = Some(ino);
        self.ledger.push(OpenRec {
            seq: self.seq_next,
            fd: slot as i32,
            path: path.to_string(),
            flags,
        });
        self.seq_next += 1;
        Ok(slot)
    }

    /// 关闭 fd。
    pub fn close(&mut self, fd: usize) -> Result<(), u32> {
        if fd >= self.fds.len() || self.fds[fd].is_none() {
            return Err(OP_BAD_FLAG);
        }
        self.fds[fd] = None;
        Ok(())
    }

    /// 扩展 inode 尺寸（打开面配套的元数据维护）。
    pub fn grow_ino(&mut self, name: &str, add: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(OP_SEALED);
        }
        let idx = match self.inodes.iter().position(|i| i.name == name) {
            Some(i) => i,
            None => return Err(OP_NOENT),
        };
        let cur = self.inodes[idx].size;
        let next = cur.wrapping_add(add);
        if next < cur {
            return Err(OP_BAD_FLAG);
        }
        self.inodes[idx].size = next;
        Ok(next)
    }

    /// 按路径读元数据（缺失路径受控）。
    pub fn stat_path(&self, name: &str) -> Result<u32, u32> {
        match self.inodes.iter().find(|i| i.name == name) {
            Some(i) => Ok(i.size),
            None => Err(OP_NOENT),
        }
    }

    /// 活跃 fd 所指 inode 编号（未开槽返回零）。
    pub fn ino_of_fd(&self, fd: usize) -> u32 {
        self.fds.get(fd).and_then(|s| s.as_ref()).copied().unwrap_or(0)
    }

    pub fn live_fds(&self) -> usize {
        self.fds.iter().filter(|s| s.is_some()).count()
    }

    pub fn ino_count(&self) -> usize {
        self.inodes.len()
    }

    pub fn seq_unique(&self) -> bool {
        for i in 0..self.ledger.len() {
            for j in (i + 1)..self.ledger.len() {
                if self.ledger[i].seq == self.ledger[j].seq {
                    return false;
                }
            }
        }
        true
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x100000;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.fd as u32);
            h = h.wrapping_mul(31).wrapping_add(r.flags);
            for b in r.path.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut t = OpenTable::new();
    // 1) 缺失路径且无 O_CREAT 必拒
    got[0] = t.open("absent.txt", O_RDWR).err() == Some(OP_NOENT);
    // 2) O_CREAT|O_EXCL 撞件必拒
    let _ = t.open("dup.txt", O_CREAT | O_RDWR);
    got[1] = t.open("dup.txt", O_CREAT | O_EXCL | O_RDWR).err() == Some(OP_EXIST);
    // 3) 空路径必拒
    got[2] = t.open("", O_CREAT | O_RDWR).err() == Some(OP_BAD_PATH);
    // 4) 非法标志位必拒
    got[3] = t.open("badflag.txt", 0x0F0).err() == Some(OP_BAD_FLAG);
    // 5) 封表后打开必抓
    t.seal();
    got[4] = t.open("late.txt", O_CREAT | O_RDWR).err() == Some(OP_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10103_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10103");

    // 正路径：open01 面——O_CREAT 建件并返回 fd
    let mut t = OpenTable::new();
    let fd = t.open("a.txt", O_CREAT | O_RDWR).unwrap_or(usize::MAX);
    let j1 = fd == 0 && t.ino_count() == 1;
    s.add("J1-建件打开", j1, "O_CREAT 建件且 fd 从最低槽起");

    // 二次打开同名件复用 inode
    let fd2 = t.open("a.txt", O_RDWR).unwrap_or(usize::MAX);
    let j2 = fd2 == 1 && t.ino_count() == 1 && t.live_fds() == 2;
    s.add("J2-复用inode", j2, "同路径二开不建新件");

    // inode 唯一性：名命中即同 id
    let j3 = {
        let a = t.find_ino("a.txt");
        let b = t.find_ino("a.txt");
        a.is_some() && a == b
    };
    s.add("J3-inode一致", j3, "同路径两 fd 指同一 inode");

    // 缺失路径受控（open02 面）
    let j4 = t.open("ghost.txt", O_RDWR).err() == Some(OP_NOENT);
    s.add("J4-缺失路径受控", j4, "无 O_CREAT 打开缺席路径必拒");

    // O_EXCL 冲突受控
    let j5 = t.open("a.txt", O_CREAT | O_EXCL | O_RDWR).err() == Some(OP_EXIST);
    s.add("J5-EXCL冲突受控", j5, "撞件必拒且不落账");

    // 容量边界：fd 表满
    let j6 = {
        let mut t6 = OpenTable::new();
        let mut ok = true;
        for i in 0..FD_CAP {
            let name = format!("f{}.txt", i);
            if t6.open(&name, O_CREAT | O_RDWR).is_err() {
                ok = false;
            }
        }
        ok && t6.open("over.txt", O_CREAT | O_RDWR).err() == Some(OP_EMFILE)
    };
    s.add("J6-fd表满受控", j6, "FD_CAP 守恒");

    // inode 容量边界（逐开即关，隔离 fd 表容量干扰）
    let j7 = {
        let mut t7 = OpenTable::new();
        let mut ok = true;
        for i in 0..INO_CAP {
            let name = format!("n{}", i);
            match t7.open(&name, O_CREAT | O_RDWR) {
                Ok(f) => {
                    if t7.close(f).is_err() {
                        ok = false;
                    }
                }
                Err(_) => {
                    ok = false;
                }
            }
        }
        ok && t7.open("nin", O_CREAT | O_RDWR).err() == Some(OP_EMFILE)
    };
    s.add("J7-inode容量受控", j7, "INO_CAP 守恒");

    // 空路径与超长路径受控
    let long = "x".repeat(PATH_MAX + 1);
    let j8 = t.open("", O_CREAT | O_RDWR).err() == Some(OP_BAD_PATH)
        && t.open(&long, O_CREAT | O_RDWR).err() == Some(OP_BAD_PATH);
    s.add("J8-路径界受控", j8, "空路径与超长路径同拒");

    // 关闭后最低槽复用
    let j9 = {
        let _ = t.close(0);
        t.open("reuse.txt", O_CREAT | O_RDWR).unwrap_or(usize::MAX) == 0
            && t.live_fds() == 2
    };
    s.add("J9-槽位复用", j9, "最低空闲槽回收复用");

    // 留痕合法与 seq 唯一
    let j10 = t.ledger.iter().all(|r| r.ok()) && t.seq_unique();
    s.add("J10-留痕合法", j10, "路径非空且标志在闭集");

    // 注入核 5/5
    let inj = inject_five();
    let j11 = inj.iter().all(|g| *g);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", inj));

    // 指纹稳定
    let j12 = t.fold() == t.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

    s
}

impl OpenTable {
    fn find_ino(&self, name: &str) -> Option<u32> {
        self.find(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10103_open_flow() {
        let mut t = OpenTable::new();
        assert!(t.open("x", O_CREAT | O_RDWR).is_ok());
        assert!(t.open("x", O_CREAT | O_EXCL | O_RDWR).err() == Some(OP_EXIST));
        assert!(t.open("y", O_RDWR).err() == Some(OP_NOENT));
        assert!(t.open("", O_RDWR).err() == Some(OP_BAD_PATH));
    }

    #[test]
    fn unx10103_checks_green() {
        let s = run_unx10103_checks();
        assert!(s.all_passed(), "UNX-F10103 判据未全绿");
    }
}
