//! UNX-F9227 open/stat/unlink ENOENT 矩阵 · 0x7A640段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9227 逐字）：
//!   ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9227-J1 三 syscall 缺失路径
//!   注入矩阵判据过，全 ENOENT 逐格检出，O_CREAT 正判据（存在即打开）格过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 矩阵是**三 syscall × 五路径形态**的 15 格整表：open/stat/unlink 各自
//!      对「文件存在 / 目录存在 / 末段缺失 / 中间段缺失 / 悬挂符号链接」给码。
//!      五列里的后三列才是 ENOENT 面，但整表都要复算——只验三列会把
//!      「目录作 unlink → EISDIR」这类格漏掉。
//!   ② O_CREAT 正判据是**双向**的：缺失路径 + O_CREAT = 建后打开（creates+1），
//!      已存在 + O_CREAT = 直接打开（creates 不动）。只验前者会把「O_CREAT
//!      误当截断/新建」的缺陷放过去。
//!   ③ 判定次序：非法标志位（EINVAL）在路径解析之前；缺父目录（ENOENT）在
//!      O_CREAT 创建之前——O_CREAT 只能补末段，补不了中间段。
//!   ④ 状态机：槽位 Free → Live → Tombstone，unlink 只允许 Live→Tombstone；
//!      再次 unlink 同一 inode 属非法（ENOENT），tombstone 不得被查找命中。
//!   ⑤ 容量：dentry 表满时 O_CREAT 回 ENOSPC，且已存在的 dentry 一个不丢。
//!   ⑥ 注入账：缺失末段 / 缺失中间段 / O_EXCL 命中 三类各 10 轮，全抓、逃逸 0。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u32 = 0x7A640;

/// dentry 表容量（含根）。
pub const MAX_DENTRIES: usize = 16;

/// 名称字节上限。
pub const NAME_CAP: usize = 10;

/// 单路径组件上限。
pub const MAX_COMPS: usize = 4;

/// 单类注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;

// Linux errno 正数口径
pub const ENOENT: i32 = 2;
pub const EEXIST: i32 = 17;
pub const ENOTDIR: i32 = 20;
pub const EISDIR: i32 = 21;
pub const EINVAL: i32 = 22;
pub const ENOSPC: i32 = 28;
pub const ENAMETOOLONG: i32 = 36;

// open 标志位（精简字节，够本域复算）
pub const O_WRONLY: u32 = 0x0_0001;
pub const O_RDWR: u32 = 0x0_0002;
pub const O_CREAT: u32 = 0x0_0040;
pub const O_EXCL: u32 = 0x0_0080;
pub const O_DIRECTORY: u32 = 0x1_0000;
/// 合法标志位闭集。
pub const OPEN_FLAG_MASK: u32 = O_WRONLY | O_RDWR | O_CREAT | O_EXCL | O_DIRECTORY;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SysCode {
    BadFlags,
    MissingLast,
    MissingParent,
    NonDirParent,
    ChildExists,
    DirAsFile,
    DirWrite,
    TableFull,
    TombstoneHit,
    LedgerRegress,
    NotReproducible,
    SegmentViolation,
    NameOverflow,
}

impl SysCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            SysCode::BadFlags => "非法标志位：open 标志落在闭集之外，按 openat 实序在\
                路径解析之前回 EINVAL；请对照 OPEN_FLAG_MASK 逐位清理",
            SysCode::MissingLast => "末段缺失：父目录在、叶子不在；无 O_CREAT 时回\
                ENOENT，有 O_CREAT 时只补这一层",
            SysCode::MissingParent => "中间段缺失：父目录本身不存在；O_CREAT 补不了\
                中间段，必须先 mkdir 再重试",
            SysCode::NonDirParent => "中间段非目录：父路径上某段是普通文件，无法下钻；\
                请回 ENOTDIR 面处理",
            SysCode::ChildExists => "O_CREAT|O_EXCL 撞上已存在对象：回 EEXIST；请去掉\
                O_EXCL 或改用其它名称",
            SysCode::DirAsFile => "目录当文件打开：O_DIRECTORY 挂在普通文件上回\
                ENOTDIR，unlink 目录回 EISDIR；请先分辨对象类别",
            SysCode::DirWrite => "目录写打开：O_WRONLY/O_RDWR 打开目录回 EISDIR；\
                目录只能只读打开或改用 opendir",
            SysCode::TableFull => "dentry 表满：O_CREAT 无法再建节点回 ENOSPC；请先\
                unlink 回收或扩容，旧节点不得因此丢失",
            SysCode::TombstoneHit => "墓碑命中：已 unlink 的 inode 再次被访问；墓碑只\
                记账不可查找，重复 unlink 必须回 ENOENT",
            SysCode::LedgerRegress => "注入账倒退：计数只增不减；请检查回滚路径",
            SysCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏\
                状态或非确定遍历序",
            SysCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
            SysCode::NameOverflow => "名称超容：越过 NAME_CAP 或为空名；请截短后重试",
        }
    }

    pub const ALL: [SysCode; 13] = [
        SysCode::BadFlags,
        SysCode::MissingLast,
        SysCode::MissingParent,
        SysCode::NonDirParent,
        SysCode::ChildExists,
        SysCode::DirAsFile,
        SysCode::DirWrite,
        SysCode::TableFull,
        SysCode::TombstoneHit,
        SysCode::LedgerRegress,
        SysCode::NotReproducible,
        SysCode::SegmentViolation,
        SysCode::NameOverflow,
    ];
}

// ── dentry 名与表 ─────────────────────────────────────────────────────
/// 定容名。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Name {
    bytes: [u8; NAME_CAP],
    len: u8,
}

impl Name {
    pub const fn blank() -> Name {
        Name { bytes: [0u8; NAME_CAP], len: 0 }
    }

    pub fn parse(s: &str) -> Option<Name> {
        let raw = s.as_bytes();
        if raw.is_empty() || raw.len() > NAME_CAP {
            return None;
        }
        let mut out = [0u8; NAME_CAP];
        let mut i = 0usize;
        while i < raw.len() {
            match raw.get(i) {
                Some(b) => match out.get_mut(i) {
                    Some(slot) => *slot = *b,
                    None => return None,
                },
                None => return None,
            }
            i += 1;
        }
        Some(Name { bytes: out, len: raw.len() as u8 })
    }

    pub fn as_str(&self) -> &str {
        let n = self.len as usize;
        match self.bytes.get(..n) {
            Some(s) => core::str::from_utf8(s).unwrap_or(""),
            None => "",
        }
    }

    pub fn eq_str(&self, s: &str) -> bool {
        self.as_str() == s
    }
}

/// 对象类别。Free 即墓碑（已 unlink）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Free,
    Dir,
    File,
    Symlink,
}

/// 单个 dentry。
#[derive(Clone, Copy, Debug)]
pub struct Dentry {
    kind: Kind,
    name: Name,
    link_to: Name,
    parent: u16,
    ino: u16,
}

impl Dentry {
    const fn blank() -> Dentry {
        Dentry { kind: Kind::Free, name: Name::blank(), link_to: Name::blank(), parent: 1, ino: 0 }
    }
}

/// dentry 表（根 ino=1）。
#[derive(Clone, Copy, Debug)]
pub struct DentryTable {
    slots: [Dentry; MAX_DENTRIES],
    used: usize,
    root_ino: u16,
    creates: u32,
    unlinks: u32,
    opens: u32,
    tombs: u32,
}

impl DentryTable {
    pub fn new() -> DentryTable {
        let mut slots = [Dentry::blank(); MAX_DENTRIES];
        slots[0] = Dentry { kind: Kind::Dir, name: Name::blank(), link_to: Name::blank(), parent: 1, ino: 1 };
        DentryTable { slots, used: 1, root_ino: 1, creates: 0, unlinks: 0, opens: 0, tombs: 0 }
    }

    pub fn root(&self) -> u16 {
        self.root_ino
    }

    pub fn used(&self) -> usize {
        self.used
    }

    pub fn creates(&self) -> u32 {
        self.creates
    }

    pub fn unlinks(&self) -> u32 {
        self.unlinks
    }

    pub fn opens(&self) -> u32 {
        self.opens
    }

    pub fn tombs(&self) -> u32 {
        self.tombs
    }

    fn idx(&self, ino: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.used {
            if let Some(d) = self.slots.get(i) {
                if d.ino == ino && d.kind != Kind::Free {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    pub fn kind(&self, ino: u16) -> Kind {
        match self.idx(ino) {
            Some(i) => match self.slots.get(i) {
                Some(d) => d.kind,
                None => Kind::Free,
            },
            None => Kind::Free,
        }
    }

    pub fn link_target(&self, ino: u16) -> Name {
        match self.idx(ino) {
            Some(i) => match self.slots.get(i) {
                Some(d) => d.link_to,
                None => Name::blank(),
            },
            None => Name::blank(),
        }
    }

    /// 父 inode（含墓碑；未命中收敛到根）。
    pub fn parent_of(&self, ino: u16) -> u16 {
        let mut i = 0usize;
        while i < self.used {
            if let Some(d) = self.slots.get(i) {
                if d.ino == ino {
                    return d.parent;
                }
            }
            i += 1;
        }
        self.root_ino
    }

    /// 在 parent 下查名。
    pub fn lookup(&self, parent: u16, name: &str) -> Option<u16> {
        let mut i = 0usize;
        while i < self.used {
            if let Some(d) = self.slots.get(i) {
                if d.kind != Kind::Free && d.parent == parent && d.name.eq_str(name) {
                    return Some(d.ino);
                }
            }
            i += 1;
        }
        None
    }

    /// 建节点：父非目录 ENOTDIR、重名 EEXIST、超容 ENOSPC、名非法 ENAMETOOLONG。
    pub fn add(&mut self, parent: u16, name: &str, kind: Kind, link_to: &str) -> Result<u16, i32> {
        if kind == Kind::Free {
            return Err(EINVAL);
        }
        if self.kind(parent) != Kind::Dir {
            return Err(ENOTDIR);
        }
        if self.lookup(parent, name).is_some() {
            return Err(EEXIST);
        }
        let nm = match Name::parse(name) {
            Some(n) => n,
            None => return Err(ENAMETOOLONG),
        };
        let lt = if kind == Kind::Symlink {
            match Name::parse(link_to) {
                Some(n) => n,
                None => return Err(ENAMETOOLONG),
            }
        } else {
            Name::blank()
        };
        if self.used >= MAX_DENTRIES {
            return Err(ENOSPC);
        }
        let idx = self.used;
        let ino = 1u16.saturating_add(idx as u16);
        match self.slots.get_mut(idx) {
            Some(slot) => *slot = Dentry { kind, name: nm, link_to: lt, parent, ino },
            None => return Err(ENOSPC),
        }
        self.used = self.used.saturating_add(1);
        self.creates = self.creates.saturating_add(1);
        Ok(ino)
    }

    /// 删节点：只允许 Live → Tombstone。
    pub fn remove(&mut self, ino: u16) -> Result<(), i32> {
        let idx = match self.idx(ino) {
            Some(i) => i,
            None => return Err(ENOENT),
        };
        match self.slots.get_mut(idx) {
            Some(slot) => {
                slot.kind = Kind::Free;
                slot.link_to = Name::blank();
            }
            None => return Err(ENOENT),
        }
        self.unlinks = self.unlinks.saturating_add(1);
        self.tombs = self.tombs.saturating_add(1);
        Ok(())
    }

    fn note_open(&mut self) {
        self.opens = self.opens.saturating_add(1);
    }

    /// 活跃节点数（不含墓碑）。
    pub fn live_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.used {
            if let Some(d) = self.slots.get(i) {
                if d.kind != Kind::Free {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

// ── 路径切分（本域口径：折叠斜杠，路径形态归 F9226） ──────────────────
/// 切分：跳过连续斜杠（本域只关心「哪一段」，路径形态错误面在 F9226）。
pub fn split(path: &str) -> Result<([&str; MAX_COMPS], usize), i32> {
    let mut out = [""; MAX_COMPS];
    let mut n = 0usize;
    let bytes = path.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes.get(i) {
            Some(b) if *b == b'/' => i += 1,
            _ => {
                let start = i;
                while i < bytes.len() {
                    match bytes.get(i) {
                        Some(b) if *b != b'/' => i += 1,
                        _ => break,
                    }
                }
                let seg = match path.get(start..i) {
                    Some(s) => s,
                    None => return Err(EINVAL),
                };
                match out.get_mut(n) {
                    Some(slot) => *slot = seg,
                    None => return Err(ENAMETOOLONG),
                }
                n = n.saturating_add(1);
            }
        }
    }
    Ok((out, n))
}

// ── 路径形态分类 ──────────────────────────────────────────────────────
/// 五列路径形态（矩阵列）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathClass {
    ExistFile,
    ExistDir,
    DanglingLink,
    MissingLast,
    MissingParent,
    NonDirParent,
}

impl PathClass {
    pub fn errno(self) -> i32 {
        match self {
            PathClass::ExistFile | PathClass::ExistDir => 0,
            PathClass::DanglingLink | PathClass::MissingLast | PathClass::MissingParent => ENOENT,
            PathClass::NonDirParent => ENOTDIR,
        }
    }

    pub fn is_enoent(self) -> bool {
        matches!(
            self,
            PathClass::DanglingLink | PathClass::MissingLast | PathClass::MissingParent
        )
    }
}

/// 解析父目录 + 叶子名。
fn resolve_parent<'a>(t: &DentryTable, path: &'a str) -> Result<(&'a str, u16), i32> {
    let (comps, n) = split(path)?;
    if n == 0 {
        return Err(ENOENT);
    }
    let mut cur = t.root();
    let mut i = 0usize;
    while i + 1 < n {
        let c = match comps.get(i).copied() {
            Some(x) => x,
            None => return Err(ENOENT),
        };
        match t.lookup(cur, c) {
            Some(ino) => {
                if t.kind(ino) != Kind::Dir {
                    return Err(ENOTDIR);
                }
                cur = ino;
            }
            None => return Err(ENOENT),
        }
        i += 1;
    }
    let leaf = match comps.get(n.saturating_sub(1)).copied() {
        Some(x) => x,
        None => return Err(ENOENT),
    };
    Ok((leaf, cur))
}

/// 叶子 inode（不跟随符号链接）。
fn leaf_ino(t: &DentryTable, path: &str) -> Option<u16> {
    match resolve_parent(t, path) {
        Ok((leaf, parent)) => t.lookup(parent, leaf),
        Err(_) => None,
    }
}

/// 分类（跟随符号链接一层；悬挂链接单列一态）。
pub fn classify(t: &DentryTable, path: &str) -> PathClass {
    let (comps, n) = match split(path) {
        Ok(v) => v,
        Err(_) => return PathClass::MissingLast,
    };
    if n == 0 {
        return if path.starts_with('/') {
            PathClass::ExistDir
        } else {
            PathClass::MissingLast
        };
    }
    let mut cur = t.root();
    let mut i = 0usize;
    while i + 1 < n {
        let c = match comps.get(i).copied() {
            Some(x) => x,
            None => return PathClass::MissingLast,
        };
        match t.lookup(cur, c) {
            Some(ino) => {
                if t.kind(ino) != Kind::Dir {
                    return PathClass::NonDirParent;
                }
                cur = ino;
            }
            None => return PathClass::MissingParent,
        }
        i += 1;
    }
    let leaf = match comps.get(n.saturating_sub(1)).copied() {
        Some(x) => x,
        None => return PathClass::MissingLast,
    };
    match t.lookup(cur, leaf) {
        Some(ino) => match t.kind(ino) {
            Kind::File => PathClass::ExistFile,
            Kind::Dir => PathClass::ExistDir,
            Kind::Symlink => {
                let target = t.link_target(ino);
                if target.eq_str("") {
                    PathClass::DanglingLink
                } else {
                    match t.lookup(cur, target.as_str()) {
                        Some(_) => PathClass::ExistFile,
                        None => PathClass::DanglingLink,
                    }
                }
            }
            Kind::Free => PathClass::MissingLast,
        },
        None => PathClass::MissingLast,
    }
}

// ── 三 syscall 判定器 ─────────────────────────────────────────────────
/// open：标志 → 路径形态 → 创建/打开。
pub fn sys_open(t: &mut DentryTable, path: &str, flags: u32) -> Result<u16, i32> {
    if flags & !OPEN_FLAG_MASK != 0 {
        return Err(EINVAL);
    }
    let class = classify(t, path);
    match class {
        PathClass::NonDirParent => Err(ENOTDIR),
        PathClass::MissingParent => Err(ENOENT),
        PathClass::MissingLast | PathClass::DanglingLink => {
            if flags & O_CREAT == 0 {
                return Err(ENOENT);
            }
            if flags & O_DIRECTORY != 0 {
                return Err(ENOENT);
            }
            let (leaf, parent) = match resolve_parent(t, path) {
                Ok(v) => v,
                Err(e) => return Err(e),
            };
            // 悬挂链接 + O_CREAT：目标名挂在链接节点上，落地在链接所在目录。
            // 目标名先进本块内的定容缓冲，&str 的生命期不出块（避免悬垂借用）。
            let mut name_buf = [0u8; NAME_CAP];
            let mut name_len = 0usize;
            let mut use_leaf = true;
            if class == PathClass::DanglingLink {
                if let Some(link) = leaf_ino(t, path) {
                    let tn = t.link_target(link);
                    let raw = tn.as_str().as_bytes();
                    if !raw.is_empty() && raw.len() <= NAME_CAP {
                        let mut k = 0usize;
                        while k < raw.len() {
                            match raw.get(k) {
                                Some(b) => match name_buf.get_mut(k) {
                                    Some(slot) => *slot = *b,
                                    None => break,
                                },
                                None => break,
                            }
                            k += 1;
                        }
                        name_len = k;
                        use_leaf = false;
                    }
                }
            }
            let target = if use_leaf {
                leaf
            } else {
                match name_buf.get(..name_len) {
                    Some(raw) => core::str::from_utf8(raw).unwrap_or(leaf),
                    None => leaf,
                }
            };
            let ino = t.add(parent, target, Kind::File, "")?;
            t.note_open();
            Ok(ino)
        }
        PathClass::ExistFile => {
            if flags & O_CREAT != 0 && flags & O_EXCL != 0 {
                return Err(EEXIST);
            }
            if flags & O_DIRECTORY != 0 {
                return Err(ENOTDIR);
            }
            match leaf_ino(t, path) {
                Some(ino) => {
                    t.note_open();
                    Ok(ino)
                }
                None => Err(ENOENT),
            }
        }
        PathClass::ExistDir => {
            if flags & (O_WRONLY | O_RDWR) != 0 {
                return Err(EISDIR);
            }
            match leaf_ino(t, path) {
                Some(ino) => {
                    t.note_open();
                    Ok(ino)
                }
                None => Err(ENOENT),
            }
        }
    }
}

/// stat：follow=false 时悬挂链接本身可被 stat。
pub fn sys_stat(t: &DentryTable, path: &str, follow: bool) -> Result<u16, i32> {
    let class = classify(t, path);
    match class {
        PathClass::NonDirParent => Err(ENOTDIR),
        PathClass::MissingParent | PathClass::MissingLast => Err(ENOENT),
        PathClass::DanglingLink => {
            if follow {
                Err(ENOENT)
            } else {
                match leaf_ino(t, path) {
                    Some(ino) => Ok(ino),
                    None => Err(ENOENT),
                }
            }
        }
        _ => match leaf_ino(t, path) {
            Some(ino) => Ok(ino),
            None => Err(ENOENT),
        },
    }
}

/// unlink：不跟随符号链接；目录回 EISDIR。
pub fn sys_unlink(t: &mut DentryTable, path: &str) -> Result<u16, i32> {
    let class = classify(t, path);
    match class {
        PathClass::NonDirParent => return Err(ENOTDIR),
        PathClass::MissingParent | PathClass::MissingLast => return Err(ENOENT),
        PathClass::ExistDir => return Err(EISDIR),
        _ => {}
    }
    let ino = match leaf_ino(t, path) {
        Some(i) => i,
        None => return Err(ENOENT),
    };
    t.remove(ino)?;
    Ok(ino)
}

// ── 样本树与矩阵表 ────────────────────────────────────────────────────
/// 样本树：/etc/passwd、/etc/link → ghost（悬挂）。
pub fn fixture() -> DentryTable {
    let mut t = DentryTable::new();
    let root = t.root();
    let etc = match t.add(root, "etc", Kind::Dir, "") {
        Ok(i) => i,
        Err(_) => return t,
    };
    let _ = t.add(etc, "passwd", Kind::File, "");
    let _ = t.add(etc, "link", Kind::Symlink, "ghost");
    t
}

/// 矩阵行：三 syscall。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Call {
    Open,
    Stat,
    Unlink,
}

impl Call {
    pub const ALL: [Call; 3] = [Call::Open, Call::Stat, Call::Unlink];

    /// 行内只用最朴素的参数：open 不带 O_CREAT，stat 跟随，unlink 无标志。
    pub fn invoke(self, t: &mut DentryTable, path: &str) -> Result<u16, i32> {
        match self {
            Call::Open => sys_open(t, path, 0),
            Call::Stat => sys_stat(t, path, true),
            Call::Unlink => sys_unlink(t, path),
        }
    }
}

/// 五列路径。
pub const COLUMNS: [&str; 5] = ["/etc/passwd", "/etc", "/etc/ghost", "/ghost/passwd", "/etc/link"];

/// 15 格期望码（0 = 成功）。
pub const EXPECTED: [[i32; 5]; 3] = [
    // open(0)：文件可开、目录只读可开、三缺失 ENOENT
    [0, 0, ENOENT, ENOENT, ENOENT],
    // stat(follow)：同上
    [0, 0, ENOENT, ENOENT, ENOENT],
    // unlink：目录 EISDIR，悬挂链接本身可删
    [0, EISDIR, ENOENT, ENOENT, 0],
];

/// 矩阵执行结果。
#[derive(Clone, Copy, Debug)]
pub struct Grid {
    pub codes: [[i32; 5]; 3],
    pub ok_cells: u32,
    pub enoent_cells: u32,
    pub created: i32,
}

/// 逐格执行：每格一棵新树，杜绝跨格污染。
pub fn run_grid() -> Grid {
    let mut codes = [[0i32; 5]; 3];
    let mut ok_cells = 0u32;
    let mut enoent_cells = 0u32;
    let mut created = 0i32;
    let mut ri = 0usize;
    while ri < Call::ALL.len() {
        if let Some(call) = Call::ALL.get(ri).copied() {
            let mut ci = 0usize;
            while ci < COLUMNS.len() {
                if let Some(path) = COLUMNS.get(ci).copied() {
                    let mut t = fixture();
                    let before = t.creates();
                    let r = call.invoke(&mut t, path);
                    let code = match r {
                        Ok(_) => 0,
                        Err(e) => e,
                    };
                    if code == 0 {
                        ok_cells = ok_cells.saturating_add(1);
                    }
                    if code == ENOENT {
                        enoent_cells = enoent_cells.saturating_add(1);
                    }
                    created = created.saturating_add(t.creates() as i32 - before as i32);
                    if let Some(row) = codes.get_mut(ri) {
                        if let Some(cell) = row.get_mut(ci) {
                            *cell = code;
                        }
                    }
                }
                ci += 1;
            }
        }
        ri += 1;
    }
    Grid { codes, ok_cells, enoent_cells, created }
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账：缺失末段 / 缺失中间段 / O_EXCL 命中。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MatrixLedger {
    pub shots: u32,
    pub detected: u32,
    pub escaped: u32,
    pub last: u32,
    pub parent: u32,
    pub excl: u32,
}

impl MatrixLedger {
    pub const fn zero() -> MatrixLedger {
        MatrixLedger { shots: 0, detected: 0, escaped: 0, last: 0, parent: 0, excl: 0 }
    }

    fn record(&mut self, want: i32, got: i32, slot: usize) {
        self.shots = self.shots.saturating_add(1);
        if got == want {
            self.detected = self.detected.saturating_add(1);
            match slot {
                0 => self.last = self.last.saturating_add(1),
                1 => self.parent = self.parent.saturating_add(1),
                _ => self.excl = self.excl.saturating_add(1),
            }
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.shots
            && self.last.saturating_add(self.parent).saturating_add(self.excl) == self.detected
    }
}

/// 三类注入 × 10 轮：缺失末段（3 syscall）、缺失中间段（3 syscall）、O_EXCL 命中。
pub fn run_injection() -> MatrixLedger {
    let mut ledger = MatrixLedger::zero();
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut t = fixture();
        // 缺失末段
        let got_open = match sys_open(&mut t, "/etc/ghost", 0) {
            Ok(_) => 0,
            Err(e) => e,
        };
        ledger.record(ENOENT, got_open, 0);
        let got_stat = match sys_stat(&t, "/etc/ghost", true) {
            Ok(_) => 0,
            Err(e) => e,
        };
        ledger.record(ENOENT, got_stat, 0);
        let got_unlink = match sys_unlink(&mut t, "/etc/ghost") {
            Ok(_) => 0,
            Err(e) => e,
        };
        ledger.record(ENOENT, got_unlink, 0);
        // 缺失中间段
        let got_open2 = match sys_open(&mut t, "/ghost/passwd", O_CREAT) {
            Ok(_) => 0,
            Err(e) => e,
        };
        ledger.record(ENOENT, got_open2, 1);
        let got_stat2 = match sys_stat(&t, "/ghost/passwd", true) {
            Ok(_) => 0,
            Err(e) => e,
        };
        ledger.record(ENOENT, got_stat2, 1);
        let got_unlink2 = match sys_unlink(&mut t, "/ghost/passwd") {
            Ok(_) => 0,
            Err(e) => e,
        };
        ledger.record(ENOENT, got_unlink2, 1);
        // O_EXCL 命中
        let got_excl = match sys_open(&mut t, "/etc/passwd", O_CREAT | O_EXCL) {
            Ok(_) => 0,
            Err(e) => e,
        };
        ledger.record(EEXIST, got_excl, 2);
        round = round.saturating_add(1);
    }
    ledger
}

// ── 对账表 ────────────────────────────────────────────────────────────
/// (syscall, 路径形态, 标志/跟随, 期望码) —— 与 Linux 逐行对齐。
pub const RECON: &[(&str, &str, &str, i32)] = &[
    ("open", "文件存在", "flags=0", 0),
    ("open", "目录存在", "flags=0", 0),
    ("open", "末段缺失", "无 O_CREAT", ENOENT),
    ("open", "中间段缺失", "有 O_CREAT", ENOENT),
    ("open", "文件存在", "O_CREAT", 0),
    ("open", "文件存在", "O_CREAT|O_EXCL", EEXIST),
    ("open", "目录存在", "O_WRONLY", EISDIR),
    ("open", "文件存在", "O_DIRECTORY", ENOTDIR),
    ("open", "末段缺失", "非法标志位", EINVAL),
    ("stat", "末段缺失", "follow=1", ENOENT),
    ("stat", "悬挂链接", "follow=0", 0),
    ("stat", "悬挂链接", "follow=1", ENOENT),
    ("unlink", "目录存在", "无标志", EISDIR),
    ("unlink", "悬挂链接", "无标志", 0),
    ("unlink", "末段缺失", "无标志", ENOENT),
];

/// 本域与路径形态域（F9226）的口径差异登记。
pub const DIFFS: &[(&str, &str)] = &[
    ("连续斜杠", "F9226 从严判 EmptyComponent，本域折叠后继续"),
    ("空路径", "F9226 判空组件，本域按 MissingLast 回 ENOENT"),
    ("尾斜杠", "本域不区分，形态判据归 F9226"),
];

// ── 确定性指纹 ────────────────────────────────────────────────────────
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 指纹：15 格 + 6 条模式格（O_CREAT/O_EXCL/O_DIRECTORY/写目录/坏标志/缺父）。
pub fn corpus_fingerprint() -> u32 {
    let g = run_grid();
    let mut bytes: Vec<u8> = Vec::new();
    let mut ri = 0usize;
    while ri < g.codes.len() {
        if let Some(row) = g.codes.get(ri) {
            let mut ci = 0usize;
            while ci < row.len() {
                if let Some(v) = row.get(ci) {
                    bytes.push((*v & 0xFF) as u8);
                }
                ci += 1;
            }
        }
        ri += 1;
    }
    let modes: [(u32, &str); 6] = [
        (O_CREAT, "/etc/ghost"),
        (O_CREAT, "/etc/passwd"),
        (O_CREAT | O_EXCL, "/etc/passwd"),
        (O_DIRECTORY, "/etc/passwd"),
        (O_WRONLY, "/etc"),
        (0x8000_0000, "/etc/passwd"),
    ];
    let mut mi = 0usize;
    while mi < modes.len() {
        if let Some(row) = modes.get(mi) {
            let mut t = fixture();
            let code = match sys_open(&mut t, row.1, row.0) {
                Ok(_) => 0,
                Err(e) => e,
            };
            bytes.push((code & 0xFF) as u8);
            bytes.push((t.creates() & 0xFF) as u8);
        }
        mi += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
pub fn run_unx9227_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9227");

    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 正常流：open/stat 命中
    let mut t = fixture();
    let open_ok = sys_open(&mut t, "/etc/passwd", 0) == Ok(3) && t.opens() == 1;
    let stat_ok = sys_stat(&t, "/etc/passwd", true) == Ok(3) && sys_stat(&t, "/etc", true) == Ok(2);
    s.add("正常流：open/stat 命中文件与目录", open_ok && stat_ok,
          "两 syscall 必须对同一 inode 达成一致（/etc/passwd=3、/etc=2），open 账 +1");

    // 正常流：unlink 生效且墓碑计数
    let unlink_ok = sys_unlink(&mut t, "/etc/passwd") == Ok(3)
        && t.kind(3) == Kind::Free
        && t.unlinks() == 1
        && t.lookup(2, "passwd").is_none();
    s.add("正常流：unlink 置墓碑且查找不再命中", unlink_ok,
          "删除后 inode 转 Free、unlink 账 +1、按名查找必须为空");

    // 15 格矩阵逐格
    let g = run_grid();
    let mut grid_ok = true;
    let mut ri = 0usize;
    while ri < EXPECTED.len() {
        if let (Some(exp), Some(got)) = (EXPECTED.get(ri), g.codes.get(ri)) {
            let mut ci = 0usize;
            while ci < exp.len() {
                if let (Some(a), Some(b)) = (exp.get(ci), got.get(ci)) {
                    if a != b {
                        grid_ok = false;
                    }
                }
                ci += 1;
            }
        }
        ri += 1;
    }
    s.add("矩阵：3 syscall × 5 形态 = 15 格逐格复算", grid_ok,
          "整表复算，缺一格即判红；三列 ENOENT 之外的两列同样在表内");

    // 缺失 6 格（三 syscall × 两缺失形态）全 ENOENT；
    // 另外 open/stat 跟随悬挂链接的 2 格同样落 ENOENT，合计 8 格。
    let enoent_ok = g.enoent_cells == 8
        && g.codes.get(0).and_then(|r| r.get(2)).copied() == Some(ENOENT)
        && g.codes.get(1).and_then(|r| r.get(3)).copied() == Some(ENOENT)
        && g.codes.get(2).and_then(|r| r.get(2)).copied() == Some(ENOENT);
    s.add("缺失路径：三 syscall × 两缺失形态全 ENOENT", enoent_ok,
          "6 个缺失格 + 2 格悬挂链接必须全回 ENOENT，任何一格 EISDIR/ENOTDIR 都算漏判");

    // O_CREAT 正判据：存在即打开
    let mut t2 = fixture();
    let before = t2.creates();
    let r_exist = sys_open(&mut t2, "/etc/passwd", O_CREAT);
    let exist_ok = r_exist == Ok(3) && t2.creates() == before && t2.kind(3) == Kind::File;
    s.add("O_CREAT 正判据：已存在直接打开且不新建", exist_ok,
          "creates 计数不动是关键——O_CREAT 不是截断/重建");

    // O_CREAT 缺失：建后打开，二次打开同一 inode
    let mut t3 = fixture();
    let first = sys_open(&mut t3, "/etc/ghost", O_CREAT);
    let second = sys_open(&mut t3, "/etc/ghost", O_CREAT);
    let create_ok = match (first, second) {
        (Ok(a), Ok(b)) => a == b && t3.creates() == 4 && t3.kind(a) == Kind::File,
        _ => false,
    };
    s.add("O_CREAT 缺失：建后打开，二次打开同 inode", create_ok,
          "首次创建 creates+1（样本树 3 + 1 = 4），二次必须命中同一 inode");

    // O_EXCL 命中
    let mut t4 = fixture();
    let excl_ok = sys_open(&mut t4, "/etc/passwd", O_CREAT | O_EXCL) == Err(EEXIST)
        && sys_open(&mut t4, "/etc/ghost", O_CREAT | O_EXCL).is_ok();
    s.add("O_CREAT|O_EXCL：存在回 EEXIST、缺失可建", excl_ok,
          "O_EXCL 只在同时给 O_CREAT 时生效，命中已存在对象必须 EEXIST");

    // 判序：缺父 O_CREAT 也救不了
    let mut t5 = fixture();
    let parent_missing_ok = sys_open(&mut t5, "/ghost/passwd", O_CREAT) == Err(ENOENT)
        && t5.creates() == 3;
    s.add("判定次序：缺父目录先于创建，O_CREAT 回 ENOENT", parent_missing_ok,
          "O_CREAT 只补末段；中间段缺失必须在创建之前被拦下，creates 不得增加");

    // 判序：坏标志先于路径
    let mut t6 = fixture();
    let flags_ok = sys_open(&mut t6, "/etc/ghost", 0x8000_0000) == Err(EINVAL)
        && sys_open(&mut t6, "/etc/ghost", 0x8000_0000 | O_CREAT) == Err(EINVAL);
    s.add("判定次序：非法标志 EINVAL 先于路径 ENOENT", flags_ok,
          "标志闭集校验在路径解析之前，按 openat 的 build_open_flags 实序");

    // 目录语义：O_DIRECTORY on file / 写打开目录
    let mut t7 = fixture();
    let dir_sem_ok = sys_open(&mut t7, "/etc/passwd", O_DIRECTORY) == Err(ENOTDIR)
        && sys_open(&mut t7, "/etc", O_WRONLY) == Err(EISDIR)
        && sys_open(&mut t7, "/etc", O_RDWR) == Err(EISDIR);
    s.add("目录语义：O_DIRECTORY 撞文件 ENOTDIR、写开目录 EISDIR", dir_sem_ok,
          "两格分别属 ENOTDIR 与 EISDIR 面，不得混为 ENOENT");

    // stat 跟随/不跟随
    let follow_ok = sys_stat(&fixture(), "/etc/link", true) == Err(ENOENT)
        && sys_stat(&fixture(), "/etc/link", false).is_ok();
    s.add("符号链接：跟随悬挂回 ENOENT、不跟随命中链接自身", follow_ok,
          "follow 语义只影响 stat，不影响 unlink 删链接本身");

    // 注入必抓 10/10
    let ledger = run_injection();
    let inject_ok = ledger.shots == INJECT_ROUNDS * 7
        && ledger.detected == ledger.shots
        && ledger.escaped == 0
        && ledger.last == INJECT_ROUNDS * 3
        && ledger.parent == INJECT_ROUNDS * 3
        && ledger.excl == INJECT_ROUNDS;
    s.add("注入必抓：三类 × 10 轮全检出、逃逸 0", inject_ok,
          "70 次注入（末段 30 / 中间段 30 / EXCL 10）必须全抓");

    s.add("账目守恒：注入=检出+逃逸且三类之和=检出", ledger.conserved() && ledger.escaped == 0,
          "守恒式任一侧不等即说明有注入未被归类");

    // 容量：表满 ENOSPC 且旧账不丢
    let mut full = fixture();
    let root = full.root();
    let mut k = 0usize;
    while k < MAX_DENTRIES {
        let mut buf = [0u8; 2];
        buf[0] = b'f';
        buf[1] = b'a' + (k as u8 % 26);
        let nm = match core::str::from_utf8(&buf) {
            Ok(v) => v,
            Err(_) => "",
        };
        let _ = full.add(root, nm, Kind::File, "");
        k += 1;
    }
    let live_before = full.live_count();
    let cap_ok = full.used() == MAX_DENTRIES
        && sys_open(&mut full, "/zzz", O_CREAT) == Err(ENOSPC)
        && full.live_count() == live_before;
    s.add("容量：表满 O_CREAT 回 ENOSPC 且活跃节点不丢", cap_ok,
          "表满只拒绝新增，live_count 必须与拒绝前相等");

    // 状态机：重复 unlink 回 ENOENT，墓碑不复活
    let mut t8 = fixture();
    let once = sys_unlink(&mut t8, "/etc/passwd");
    let twice = sys_unlink(&mut t8, "/etc/passwd");
    let sm_ok = once == Ok(3)
        && twice == Err(ENOENT)
        && t8.tombs() == 1
        && t8.kind(3) == Kind::Free
        && t8.parent_of(3) == 2;
    s.add("状态机：Live→Tombstone 单向，重复 unlink 回 ENOENT", sm_ok,
          "墓碑不可查找、不可二次删除，unlink 账只记一次");

    // 对账表复算
    let mut recon_ok = true;
    let mut i2 = 0usize;
    while i2 < RECON.len() {
        if let Some(row) = RECON.get(i2) {
            let mut tt = fixture();
            let code = match row.0 {
                "open" => match row.2 {
                    "flags=0" => code_of(sys_open(&mut tt, path_of(row.1), 0)),
                    "无 O_CREAT" => code_of(sys_open(&mut tt, path_of(row.1), 0)),
                    "有 O_CREAT" => code_of(sys_open(&mut tt, path_of(row.1), O_CREAT)),
                    "O_CREAT" => code_of(sys_open(&mut tt, path_of(row.1), O_CREAT)),
                    "O_CREAT|O_EXCL" => code_of(sys_open(&mut tt, path_of(row.1), O_CREAT | O_EXCL)),
                    "O_WRONLY" => code_of(sys_open(&mut tt, path_of(row.1), O_WRONLY)),
                    "O_DIRECTORY" => code_of(sys_open(&mut tt, path_of(row.1), O_DIRECTORY)),
                    "非法标志位" => code_of(sys_open(&mut tt, path_of(row.1), 0x8000_0000)),
                    _ => -1,
                },
                "stat" => {
                    let follow = row.2 == "follow=1";
                    code_of(sys_stat(&tt, path_of(row.1), follow))
                }
                "unlink" => code_of(sys_unlink(&mut tt, path_of(row.1))),
                _ => -1,
            };
            if code != row.3 {
                recon_ok = false;
            }
        }
        i2 += 1;
    }
    s.add("对账：15 行 Linux 对照表逐行复算相等", recon_ok,
          "对照表任一行与判定器不一致即判红，不许「表是表、码是码」");

    // 双跑一致
    let fp1 = corpus_fingerprint();
    let fp2 = corpus_fingerprint();
    s.add("双跑一致：语料指纹相等", fp1 == fp2,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 文案闭集
    let text_ok = SysCode::ALL.iter().all(|c| !c.detail().is_empty()) && !SysCode::ALL.is_empty();
    s.add("文案闭集：全部诊断码带非空中文建议", text_ok,
          "13 个码都必须能取到可操作说明");

    // 码值唯一
    let mut uniq = true;
    let mut a = 0usize;
    while a < SysCode::ALL.len() {
        let mut b = a + 1;
        while b < SysCode::ALL.len() {
            if let (Some(x), Some(y)) = (SysCode::ALL.get(a), SysCode::ALL.get(b)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            b += 1;
        }
        a += 1;
    }
    s.add("诊断码：13 个码值两两唯一", uniq,
          "段内闭集码重复会让判据无法定位");

    // 账目单调
    let mut mono = ledger;
    let before_shots = mono.shots;
    mono.record(ENOENT, ENOENT, 0);
    s.add("账目单调：再注入只增不减且守恒", mono.shots == before_shots.saturating_add(1) && mono.conserved(),
          "累计计数禁止回退");

    // 差异登记完整
    let diff_ok = DIFFS.len() == 3 && DIFFS.iter().all(|r| !r.0.is_empty() && !r.1.is_empty());
    s.add("对账：与路径形态域的口径差异逐条登记", diff_ok,
          "折叠斜杠/空路径/尾斜杠三处差异必须显性，不许暗改语义");

    let grid_text = format!(
        "15 格 ok={} enoent={} created={}",
        g.ok_cells, g.enoent_cells, g.created
    );
    s.add_dyn("矩阵快照", grid_ok, &grid_text);
    let ledger_text = format!(
        "注入 {} 检出 {} 逃逸 {} | 末 {} 中 {} EXCL {}",
        ledger.shots, ledger.detected, ledger.escaped, ledger.last, ledger.parent, ledger.excl
    );
    s.add_dyn("注入账", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", fp1);
    s.add_dyn("指纹快照", fp1 == fp2, &fp_text);

    s
}

/// 结果折成码（0 = 成功）。
fn code_of(r: Result<u16, i32>) -> i32 {
    match r {
        Ok(_) => 0,
        Err(e) => e,
    }
}

/// 对账表里的列名折成路径。
fn path_of(column: &str) -> &'static str {
    match column {
        "文件存在" => "/etc/passwd",
        "目录存在" => "/etc",
        "末段缺失" => "/etc/ghost",
        "中间段缺失" => "/ghost/passwd",
        "悬挂链接" => "/etc/link",
        _ => "/etc/passwd",
    }
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_open_stat() {
        let mut t = fixture();
        assert_eq!(sys_open(&mut t, "/etc/passwd", 0), Ok(3));
        assert_eq!(sys_open(&mut t, "/etc", 0), Ok(2));
        assert_eq!(sys_stat(&t, "/etc/passwd", true), Ok(3));
    }

    #[test]
    fn grid_matches_expected() {
        let g = run_grid();
        for (ri, row) in EXPECTED.iter().enumerate() {
            for (ci, want) in row.iter().enumerate() {
                let got = g.codes.get(ri).and_then(|r| r.get(ci)).copied().unwrap_or(-1);
                assert_eq!(got, *want, "row={} col={}", ri, ci);
            }
        }
        assert_eq!(g.enoent_cells, 8);
        assert_eq!(g.created, 0);
    }

    #[test]
    fn o_creat_positive_paths() {
        let mut t = fixture();
        let before = t.creates();
        assert_eq!(sys_open(&mut t, "/etc/passwd", O_CREAT), Ok(3));
        assert_eq!(t.creates(), before);
        let a = sys_open(&mut t, "/etc/ghost", O_CREAT).unwrap_or(0);
        let b = sys_open(&mut t, "/etc/ghost", O_CREAT).unwrap_or(0);
        assert!(a >= 4);
        assert_eq!(a, b);
        assert_eq!(t.creates(), before + 1);
    }

    #[test]
    fn excl_and_directory_codes() {
        let mut t = fixture();
        assert_eq!(sys_open(&mut t, "/etc/passwd", O_CREAT | O_EXCL), Err(EEXIST));
        assert_eq!(sys_open(&mut t, "/etc/passwd", O_DIRECTORY), Err(ENOTDIR));
        assert_eq!(sys_open(&mut t, "/etc", O_WRONLY), Err(EISDIR));
        assert_eq!(sys_open(&mut t, "/etc/ghost", 0x8000_0000), Err(EINVAL));
    }

    #[test]
    fn missing_parent_not_created() {
        let mut t = fixture();
        let before = t.creates();
        assert_eq!(sys_open(&mut t, "/ghost/passwd", O_CREAT), Err(ENOENT));
        assert_eq!(t.creates(), before);
        assert_eq!(sys_stat(&t, "/ghost/passwd", true), Err(ENOENT));
        assert_eq!(sys_unlink(&mut t, "/ghost/passwd"), Err(ENOENT));
    }

    #[test]
    fn unlink_state_machine() {
        let mut t = fixture();
        assert_eq!(sys_unlink(&mut t, "/etc"), Err(EISDIR));
        assert_eq!(sys_unlink(&mut t, "/etc/link"), Ok(4));
        assert_eq!(t.kind(4), Kind::Free);
        assert_eq!(sys_unlink(&mut t, "/etc/passwd"), Ok(3));
        assert_eq!(sys_unlink(&mut t, "/etc/passwd"), Err(ENOENT));
        assert_eq!(t.tombs(), 2);
        assert_eq!(t.unlinks(), 2);
    }

    #[test]
    fn symlink_follow_semantics() {
        let t = fixture();
        assert_eq!(sys_stat(&t, "/etc/link", true), Err(ENOENT));
        assert!(sys_stat(&t, "/etc/link", false).is_ok());
    }

    #[test]
    fn capacity_enospc_keeps_live() {
        let mut t = fixture();
        let root = t.root();
        let mut k = 0usize;
        while k < MAX_DENTRIES {
            let mut buf = [0u8; 2];
            buf[0] = b'f';
            buf[1] = b'a' + (k as u8 % 26);
            let nm = core::str::from_utf8(&buf).unwrap_or("");
            let _ = t.add(root, nm, Kind::File, "");
            k += 1;
        }
        assert_eq!(t.used(), MAX_DENTRIES);
        let live = t.live_count();
        assert_eq!(sys_open(&mut t, "/zzz", O_CREAT), Err(ENOSPC));
        assert_eq!(t.live_count(), live);
    }

    #[test]
    fn injection_ledger() {
        let l = run_injection();
        assert_eq!(l.shots, 70);
        assert_eq!(l.detected, 70);
        assert_eq!(l.escaped, 0);
        assert!(l.conserved());
    }

    #[test]
    fn recon_table_recomputes() {
        for row in RECON.iter() {
            let mut t = fixture();
            let code = match row.0 {
                "open" => match row.2 {
                    "flags=0" => code_of(sys_open(&mut t, path_of(row.1), 0)),
                    "无 O_CREAT" => code_of(sys_open(&mut t, path_of(row.1), 0)),
                    "有 O_CREAT" => code_of(sys_open(&mut t, path_of(row.1), O_CREAT)),
                    "O_CREAT" => code_of(sys_open(&mut t, path_of(row.1), O_CREAT)),
                    "O_CREAT|O_EXCL" => code_of(sys_open(&mut t, path_of(row.1), O_CREAT | O_EXCL)),
                    "O_WRONLY" => code_of(sys_open(&mut t, path_of(row.1), O_WRONLY)),
                    "O_DIRECTORY" => code_of(sys_open(&mut t, path_of(row.1), O_DIRECTORY)),
                    "非法标志位" => code_of(sys_open(&mut t, path_of(row.1), 0x8000_0000)),
                    _ => -1,
                },
                "stat" => {
                    let follow = row.2 == "follow=1";
                    code_of(sys_stat(&t, path_of(row.1), follow))
                }
                "unlink" => code_of(sys_unlink(&mut t, path_of(row.1))),
                _ => -1,
            };
            assert_eq!(code, row.3, "row={:?}", row);
        }
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9227_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(s.len() >= 12);
        assert!(!s.truncated());
    }
}
