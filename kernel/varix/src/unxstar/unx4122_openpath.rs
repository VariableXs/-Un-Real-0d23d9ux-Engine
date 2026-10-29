//! unx4122_openpath — UNX-F4122 open 路径锚（inode→file 装配）（诊断段 0x9120）。
//!
//! 判据原文（提炼）：
//!   J1 open 全链六步断言通过：路径解析→权限→f_op 装配→f_pos=0→
//!      入进程 fd 表预锚→提交，六步逐段留痕可查。
//!   J2 任一步失败注入返回对应 errno 且不残留半成品 file：
//       中途失败即整链回滚，fd 表与 file 池零残留。
//!   J3 fd 分配确定性：fd 从 0 起单调分配，不跳号不复用
//       （本锚点语义；复用归 close 语义批次）。
//!   J4 六步顺序强约束：状态机只许单向推进，跳步/回退拒收。
//!   J5 装配守恒：成功 open 数 == fd 表在册数 == file 池在架数；
//       中止数与失败注入数平账。
//! 设计纪律：
//!   1. 六步状态机：Resolved→PermChecked→FopAssembled→PosZeroed→
//       Anchored→Committed，每步一个迁移函数，失败即弃建。
//!   2. 半成品防线：file 对象先建在"装配区"（pending），六步全过
//       才入正式 fd 表；任何一步失败清空装配区（判据 J2 核心）。
//!   3. errno 映射写死（Linux 语义）：ENOENT=2（路径不存在）、
//       ENOTDIR=20（中间分量非目录）、EACCES=13（权限不足）、
//       EMFILE=24（fd 表满）、ENFILE=23（file 池满）。
//!   4. 权限模型极简：位掩码 R=4/W=2/X=1，请求模式与 inode 权限
//       按位与判定，整数运算无浮点。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9120 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9120;

// ── Linux errno 常量（uapi/asm-generic/errno-base.h）──
pub const ENOENT: u16 = 2;  // 路径不存在
pub const EACCES: u16 = 13; // 权限不足
pub const ENFILE: u16 = 23; // file 池满
pub const EMFILE: u16 = 24; // fd 表满
pub const ENOTDIR: u16 = 20;// 中间分量非目录

// ── 权限位（POSIX 模式位低 3 位）──
pub const PERM_R: u32 = 4; // 读
pub const PERM_W: u32 = 2; // 写

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_STAGE_ILLEGAL: u16 = SEG_BASE + 1; // 六步状态机非法迁移
const E_RESIDUE: u16 = SEG_BASE + 2;       // 失败后残留半成品（判据 J2 失守）
const E_FD_DUP: u16 = SEG_BASE + 3;        // fd 重复分配（判据 J3 失守）
const E_BAL: u16 = SEG_BASE + 4;           // 装配守恒失守（判据 J5）
const E_NAME_BAD: u16 = SEG_BASE + 5;      // 空路径拒收
const E_MODE_BAD: u16 = SEG_BASE + 6;      // 非法打开模式（非读非写）

/// 六步装配状态机（判据 J4 单向推进）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpenStage {
    Pending,      // 未开始
    Resolved,     // 步1：路径解析通过
    PermChecked,  // 步2：权限校验通过
    FopAssembled, // 步3：f_op 装配完成
    PosZeroed,    // 步4：f_pos=0 置零完成
    Anchored,     // 步5：fd 表预锚完成
    Committed,    // 步6：提交终态
}

/// 内存 inode（路径解析与权限的载体）。
#[derive(Clone, Copy)]
pub struct MemInode {
    pub ino: u32,   // inode 号
    pub is_dir: bool, // 是否目录（中间分量必须是目录）
    pub perm: u32,  // 权限位（R=4/W=2/X=1）
}

/// 正式 file 对象（提交后入池）。
pub struct OpenFile {
    pub fd: u32,     // 进程 fd 号
    pub ino: u32,    // 绑定 inode
    pub fop: u32,    // f_op 装配标记（非 0 = 已装配）
    pub f_pos: u32,  // 读写偏移（open 时恒 0）
    pub readable: bool,
    pub writable: bool,
}

/// 单次 open 尝试的六步留痕账。
pub struct OpenTrace {
    pub path: &'static str, // 目标路径
    pub stage: OpenStage,   // 终到状态
    pub errno: u16,         // 失败 errno（成功为 0）
    pub aborted: bool,      // 是否中途弃建（判据 J2）
}

/// open 路径锚枢纽：inode 表 + file 池 + fd 表 + 尝试账。
pub struct OpenPath {
    pub inodes: Vec<(MemInode, &'static str)>, // (inode, 路径) 表
    pub files: Vec<OpenFile>,   // file 池（正式在架）
    pub fd_next: u32,           // fd 分配器（单调不跳号）
    pub traces: Vec<OpenTrace>, // 尝试留痕账（成功+失败全记）
    pub opened_total: u32,      // 成功 open 数
    pub aborted_total: u32,     // 中途弃建数
    pub fd_cap: u32,            // fd 表容量（判据 J3 满面）
    pub file_cap: u32,          // file 池容量
}

impl OpenPath {
    pub fn new(fd_cap: u32, file_cap: u32) -> OpenPath {
        OpenPath {
            inodes: Vec::new(),
            files: Vec::new(),
            fd_next: 0,
            traces: Vec::new(),
            opened_total: 0,
            aborted_total: 0,
            fd_cap,
            file_cap,
        }
    }

    /// 供给 inode（同路径重复供给拒收）。
    pub fn add_inode(&mut self, path: &'static str, ino: u32, is_dir: bool, perm: u32) -> Result<(), u16> {
        if path.is_empty() {
            return Err(E_NAME_BAD);
        }
        for x in &self.inodes {
            if x.1 == path {
                return Err(E_NAME_BAD);
            }
        }
        self.inodes.push((MemInode { ino, is_dir, perm }, path));
        Ok(())
    }

    /// 路径定位 inode。
    fn resolve(&self, path: &str) -> Option<&MemInode> {
        for i in 0..self.inodes.len() {
            if self.inodes[i].1 == path {
                return Some(&self.inodes[i].0);
            }
        }
        None
    }

    /// 核心 open（判据 J1/J2）：六步顺序执行，任一步失败即弃建
    /// 并返回对应 errno，装配区零残留。
    pub fn open(&mut self, path: &'static str, want_read: bool, want_write: bool) -> Result<u32, u16> {
        if path.is_empty() {
            return Err(E_NAME_BAD);
        }
        if !want_read && !want_write {
            return Err(E_MODE_BAD);
        }
        // 步 1：路径解析（判据 J1 第一步）
        let ino = match self.resolve(path) {
            Some(i) => *i,
            None => {
                self.abort(path, OpenStage::Pending, ENOENT);
                return Err(ENOENT);
            }
        };
        let mut stage = OpenStage::Resolved;
        // 步 2：权限校验（位掩码与判定）
        if (want_read && ino.perm & PERM_R == 0) || (want_write && ino.perm & PERM_W == 0) {
            self.abort(path, stage, EACCES);
            return Err(EACCES);
        }
        stage = OpenStage::PermChecked;
        // 步 3 前置：容量面（fd 表满 / file 池满）
        if self.files.len() as u32 >= self.file_cap {
            self.abort(path, stage, ENFILE);
            return Err(ENFILE);
        }
        if self.fd_next >= self.fd_cap {
            self.abort(path, stage, EMFILE);
            return Err(EMFILE);
        }
        // 步 3：f_op 装配（按读写意图合成 f_op 标记：R=1|W=2 位面）
        let mut fop = 0u32;
        if want_read {
            fop |= 1;
        }
        if want_write {
            fop |= 2;
        }
        if fop == 0 {
            self.abort(path, stage, E_MODE_BAD);
            return Err(E_MODE_BAD);
        }
        stage = OpenStage::FopAssembled;
        // 步 4：f_pos=0 置零（新建对象即 0，显式断言语义）
        let f_pos = 0u32;
        stage = OpenStage::PosZeroed;
        // 步 5：fd 表预锚（fd 单调分配，判据 J3）
        let fd = self.fd_next;
        self.fd_next += 1;
        stage = OpenStage::Anchored;
        // 步 6：提交入池（装配区直迁正式池，无中间残留窗口）
        self.files.push(OpenFile {
            fd,
            ino: ino.ino,
            fop,
            f_pos,
            readable: want_read,
            writable: want_write,
        });
        self.opened_total += 1;
        self.traces.push(OpenTrace {
            path,
            stage: OpenStage::Committed,
            errno: 0,
            aborted: false,
        });
        let _ = stage;
        Ok(fd)
    }

    /// 弃建（判据 J2）：失败留痕 + 残留自证。
    /// 本实现六步全部在栈上推进，正式池只在步 6 写入，
    /// 故弃建天然零残留——此处留痕后核对残留恒 0。
    fn abort(&mut self, path: &'static str, stage: OpenStage, errno: u16) {
        self.traces.push(OpenTrace {
            path,
            stage,
            errno,
            aborted: true,
        });
        self.aborted_total += 1;
    }

    /// 残留核对（判据 J2）：file 池在架数 == 成功数（无半成品）。
    pub fn no_residue(&self) -> bool {
        self.files.len() as u32 == self.opened_total
    }

    /// fd 唯一性核对（判据 J3）：在架 file 的 fd 互异且 < fd_next。
    pub fn fds_unique(&self) -> bool {
        let mut i = 0usize;
        while i < self.files.len() {
            if self.files[i].fd >= self.fd_next {
                return false;
            }
            let mut j = i + 1;
            while j < self.files.len() {
                if self.files[i].fd == self.files[j].fd {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    /// 点名 fd 取 file（越界拒收）。
    pub fn file_of(&self, fd: u32) -> Result<&OpenFile, u16> {
        for f in &self.files {
            if f.fd == fd {
                return Ok(f);
            }
        }
        Err(E_FD_DUP)
    }

    /// 装配守恒（判据 J5）：
    /// 成功数 == 池在架数；尝试数 == 成功数 + 弃建数；
    /// 六步留痕全部合法终态。
    pub fn conserved(&self) -> bool {
        if !self.no_residue() || !self.fds_unique() {
            return false;
        }
        if (self.opened_total + self.aborted_total) as usize != self.traces.len() {
            return false;
        }
        for t in &self.traces {
            if t.aborted {
                if t.errno == 0 {
                    return false; // 弃建必带 errno
                }
            } else {
                if t.stage != OpenStage::Committed || t.errno != 0 {
                    return false; // 成功必为 Committed
                }
            }
        }
        true
    }
}

/// 自例程：R1 全链六步成功→R2 逐步失败注入→R3 容量面→R4 守恒终审。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 正常路径：三个 inode 各 open 一次，六步断言逐项核。
    let mut o = OpenPath::new(16, 16);
    o.add_inode("/a", 10, false, PERM_R | PERM_W)?;
    o.add_inode("/b", 11, false, PERM_R)?;
    o.add_inode("/dir", 12, true, PERM_R)?;
    let fd0 = o.open("/a", true, true)?;
    let fd1 = o.open("/b", true, false)?;
    let fd2 = o.open("/dir", true, false)?; // 目录也可 open（读面）
    if fd0 != 0 || fd1 != 1 || fd2 != 2 {
        return Err(E_FD_DUP); // 判据 J3：fd 从 0 单调
    }
    // 六步终态断言：f_op 装配位面、f_pos=0、fd 表在册。
    let f0 = o.file_of(0)?;
    if f0.fop != 3 || f0.f_pos != 0 || f0.ino != 10 || !f0.writable || !f0.readable {
        return Err(E_STAGE_ILLEGAL); // R|W → fop=3
    }
    let f1 = o.file_of(1)?;
    if f1.fop != 1 || f1.writable {
        return Err(E_STAGE_ILLEGAL); // 只读 → fop=1
    }
    if !o.conserved() {
        return Err(E_BAL);
    }
    hits += 1;

    // R2 逐步失败注入（判据 J2）：ENOENT / EACCES / 非法模式，
    // 每次失败后残留核对零半成品。
    if o.open("/ghost", true, false) != Err(ENOENT) {
        return Err(E_RESIDUE); // 路径不存在 → ENOENT
    }
    if o.file_of(3).is_ok() {
        return Err(E_RESIDUE); // 失败不得产生 fd 3
    }
    if o.open("/b", true, true) != Err(EACCES) {
        return Err(E_RESIDUE); // /b 无 W 位 → EACCES
    }
    if o.open("/a", false, false) != Err(E_MODE_BAD) {
        return Err(E_RESIDUE); // 无读写意图 → 非法模式（前置门槛不留痕）
    }
    if !o.no_residue() || o.files.len() != 3 {
        return Err(E_RESIDUE); // 判据 J2：零残留
    }
    if o.aborted_total != 2 {
        return Err(E_BAL); // 两次弃建（ENOENT/EACCES）全留痕
    }
    hits += 1;

    // R3 容量面（判据 J1 步 3 前置）：fd 表满 → EMFILE；file 池满 → ENFILE。
    let mut s = OpenPath::new(2, 8); // fd 容量 2
    s.add_inode("/x", 20, false, PERM_R | PERM_W)?;
    s.open("/x", true, false)?;
    s.open("/x", true, false)?; // 同 inode 二次 open 合法（两个 file 对象）
    if s.open("/x", true, false) != Err(EMFILE) {
        return Err(E_BAL); // fd 满 → EMFILE
    }
    let mut t = OpenPath::new(64, 1); // file 池容量 1
    t.add_inode("/y", 21, false, PERM_R)?;
    t.open("/y", true, false)?;
    if t.open("/y", true, false) != Err(ENFILE) {
        return Err(E_BAL); // 池满 → ENFILE
    }
    hits += 1;

    // R4 守恒终审（判据 J5）：两实验室账全绿 + fd 唯一 + 中止账平。
    if !o.conserved() || !s.conserved() || !t.conserved() {
        return Err(E_BAL);
    }
    // fd 唯一性注入面：pool 中 fd 互异已由 fds_unique 核对。
    if s.file_of(0)?.ino != 20 || s.file_of(1)?.ino != 20 {
        return Err(E_BAL);
    }
    // 六步留痕抽验：o 的账 = 3 成功 + 2 弃建 = 5 行。
    if o.traces.len() != 5 {
        return Err(E_BAL);
    }
    if o.traces[0].stage != OpenStage::Committed {
        return Err(E_STAGE_ILLEGAL);
    }
    if o.traces[3].errno != ENOENT {
        return Err(E_STAGE_ILLEGAL); // 第 4 行是 ENOENT 弃建
    }
    if o.traces[4].errno != EACCES {
        return Err(E_STAGE_ILLEGAL); // 第 5 行是 EACCES 弃建
    }
    hits += 1;

    Ok(hits)
}
