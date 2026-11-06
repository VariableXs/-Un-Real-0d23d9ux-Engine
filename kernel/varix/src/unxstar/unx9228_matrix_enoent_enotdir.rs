//! UNX-F9228 目录操作 ENOENT/ENOTDIR/EISDIR 矩阵 · 0x7A680段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9228 逐字）：
//!   ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9228-J1 三错误码注入矩阵判据过，
//!   目录尾段缺失全 ENOENT 格过，文件作目录全 ENOTDIR 格过；③ 完成时 API/
//!   软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 矩阵是 **6 操作 × 5 目标形态 = 30 格**：mkdir/rmdir/unlink/opendir/
//!      chdir/getdents 对「目录在 / 文件在 / 尾段缺失 / 中间段缺失 / 父是文件」
//!      逐格给码。三错误码不是三张表，而是同一张表的三个区带——只验三个
//!      代表格会漏掉「mkdir 撞目录回 EEXIST」这种真实码。
//!   ② 「文件作目录」是 ENOTDIR 面：rmdir/opendir/chdir/getdents 打到普通
//!      文件上全是 ENOTDIR；而 unlink 打到目录上是 EISDIR。两条面不许互换。
//!   ③ 判定次序：父链上某段是文件（NonDir）必须先于末段缺失返回——路径
//!      `/etc/passwd/sub` 既「末段可能缺失」又「父是文件」，答案只能是
//!      ENOTDIR。
//!   ④ cwd 是状态机：成功 chdir 才推进 epoch；失败必须**原地回滚**。删除
//!      当前工作目录后 cwd 标记 stale：相对路径一律 ENOENT，绝对路径不受影响。
//!   ⑤ 容量有两条：单目录扇出上限（DIR_FANOUT）与全局节点上限（MAX_NODES），
//!      两者都回 ENOSPC，且拒绝新增时旧节点一个不丢。
//!   ⑥ 注入三码各 10 轮，全抓、逃逸 0；账目分格守恒。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x7A680;

/// 全局节点上限（含根）。
pub const MAX_NODES: usize = 20;
/// 单目录子项上限。
pub const DIR_FANOUT: usize = 6;
/// 名称上限。
pub const NAME_CAP: usize = 8;
/// 组件上限。
pub const MAX_COMPS: usize = 4;
/// 单类注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;

pub const ENOENT: i32 = 2;
pub const EEXIST: i32 = 17;
pub const ENOTDIR: i32 = 20;
pub const EISDIR: i32 = 21;
pub const EINVAL: i32 = 22;
pub const ENOSPC: i32 = 28;
pub const ENAMETOOLONG: i32 = 36;
pub const ENOTEMPTY: i32 = 39;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DirCode {
    MissingTail,
    MissingMiddle,
    FileAsDir,
    DirAsFile,
    NotEmpty,
    AlreadyExists,
    DotRemove,
    FanoutFull,
    NodeTableFull,
    CwdStale,
    LedgerRegress,
    NotReproducible,
    SegmentViolation,
}

impl DirCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            DirCode::MissingTail => "目录尾段缺失：父目录在、末段不在，回 ENOENT；\
                mkdir 可补这一层，rmdir/chdir 不行",
            DirCode::MissingMiddle => "中间段缺失：父链本身断了，回 ENOENT；\
                请从根开始逐段核对",
            DirCode::FileAsDir => "文件作目录：中间段或末段是普通文件，回 ENOTDIR；\
                请改用该文件所在目录作父路径",
            DirCode::DirAsFile => "目录作文件：unlink 目录回 EISDIR；请改用 rmdir，\
                且目标必须为空",
            DirCode::NotEmpty => "目录非空：rmdir 仅删空目录，回 ENOTEMPTY；请先\
                清空子项或改用递归删除",
            DirCode::AlreadyExists => "对象已存在：mkdir 撞同名回 EEXIST；请换名或\
                先确认是否已建",
            DirCode::DotRemove => "删除点段：rmdir(\".\") 按 POSIX 回 EINVAL；\
                点段只用于定位，不是可删对象",
            DirCode::FanoutFull => "单目录扇出满：子项数达 DIR_FANOUT 回 ENOSPC；\
                请分桶或提升上限后重测",
            DirCode::NodeTableFull => "节点表满：全局节点达 MAX_NODES 回 ENOSPC；\
                请回收节点或扩容，旧节点不得丢",
            DirCode::CwdStale => "工作目录已失效：cwd 被删后相对路径回 ENOENT；\
                请先 chdir 到绝对路径重建 cwd",
            DirCode::LedgerRegress => "注入账倒退：计数只增不减；请检查回滚路径",
            DirCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查\
                隐藏状态或非确定遍历序",
            DirCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    pub const ALL: [DirCode; 13] = [
        DirCode::MissingTail,
        DirCode::MissingMiddle,
        DirCode::FileAsDir,
        DirCode::DirAsFile,
        DirCode::NotEmpty,
        DirCode::AlreadyExists,
        DirCode::DotRemove,
        DirCode::FanoutFull,
        DirCode::NodeTableFull,
        DirCode::CwdStale,
        DirCode::LedgerRegress,
        DirCode::NotReproducible,
        DirCode::SegmentViolation,
    ];
}

// ── 节点与目录树 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Free,
    Dir,
    File,
}

#[derive(Clone, Copy, Debug)]
pub struct Entry {
    kind: Kind,
    name: [u8; NAME_CAP],
    len: u8,
    parent: u16,
    ino: u16,
}

impl Entry {
    const fn blank() -> Entry {
        Entry { kind: Kind::Free, name: [0u8; NAME_CAP], len: 0, parent: 1, ino: 0 }
    }

    fn name_is(&self, s: &str) -> bool {
        let n = self.len as usize;
        match self.name.get(..n) {
            Some(b) => b == s.as_bytes(),
            None => false,
        }
    }
}

/// 目录树。写操作只走 `add` / `remove`，两者都先验父链与容量。
#[derive(Clone, Copy, Debug)]
pub struct DirTree {
    slots: [Entry; MAX_NODES],
    used: usize,
    root_ino: u16,
    mkdirs: u32,
    rmdirs: u32,
    unlinks: u32,
}

impl DirTree {
    pub fn new() -> DirTree {
        let mut slots = [Entry::blank(); MAX_NODES];
        slots[0] = Entry { kind: Kind::Dir, name: [0u8; NAME_CAP], len: 0, parent: 1, ino: 1 };
        DirTree { slots, used: 1, root_ino: 1, mkdirs: 0, rmdirs: 0, unlinks: 0 }
    }

    pub fn root(&self) -> u16 {
        self.root_ino
    }

    pub fn used(&self) -> usize {
        self.used
    }

    pub fn mkdirs(&self) -> u32 {
        self.mkdirs
    }

    pub fn rmdirs(&self) -> u32 {
        self.rmdirs
    }

    pub fn unlinks(&self) -> u32 {
        self.unlinks
    }

    fn idx(&self, ino: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.used {
            if let Some(e) = self.slots.get(i) {
                if e.ino == ino && e.kind != Kind::Free {
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
                Some(e) => e.kind,
                None => Kind::Free,
            },
            None => Kind::Free,
        }
    }

    /// 父 inode（含墓碑记录；未命中收敛到根）。
    pub fn parent_of(&self, ino: u16) -> u16 {
        let mut i = 0usize;
        while i < self.used {
            if let Some(e) = self.slots.get(i) {
                if e.ino == ino {
                    return e.parent;
                }
            }
            i += 1;
        }
        self.root_ino
    }

    pub fn lookup(&self, parent: u16, name: &str) -> Option<u16> {
        let mut i = 0usize;
        while i < self.used {
            if let Some(e) = self.slots.get(i) {
                if e.kind != Kind::Free && e.parent == parent && e.name_is(name) {
                    return Some(e.ino);
                }
            }
            i += 1;
        }
        None
    }

    pub fn child_count(&self, parent: u16) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.used {
            if let Some(e) = self.slots.get(i) {
                if e.kind != Kind::Free && e.parent == parent {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 挂节点：父非目录 ENOTDIR、重名 EEXIST、扇出满/表满 ENOSPC、名非法 ENAMETOOLONG。
    pub fn add(&mut self, parent: u16, name: &str, kind: Kind) -> Result<u16, i32> {
        if kind == Kind::Free {
            return Err(EINVAL);
        }
        if self.kind(parent) != Kind::Dir {
            return Err(ENOTDIR);
        }
        if self.lookup(parent, name).is_some() {
            return Err(EEXIST);
        }
        let raw = name.as_bytes();
        if raw.is_empty() || raw.len() > NAME_CAP {
            return Err(ENAMETOOLONG);
        }
        if self.child_count(parent) >= DIR_FANOUT {
            return Err(ENOSPC);
        }
        if self.used >= MAX_NODES {
            return Err(ENOSPC);
        }
        let idx = self.used;
        let ino = 1u16.saturating_add(idx as u16);
        let mut nb = [0u8; NAME_CAP];
        let mut k = 0usize;
        while k < raw.len() {
            match raw.get(k) {
                Some(b) => match nb.get_mut(k) {
                    Some(slot) => *slot = *b,
                    None => return Err(ENAMETOOLONG),
                },
                None => return Err(ENAMETOOLONG),
            }
            k += 1;
        }
        match self.slots.get_mut(idx) {
            Some(slot) => *slot = Entry { kind, name: nb, len: k as u8, parent, ino },
            None => return Err(ENOSPC),
        }
        self.used = self.used.saturating_add(1);
        if kind == Kind::Dir {
            self.mkdirs = self.mkdirs.saturating_add(1);
        }
        Ok(ino)
    }

    /// 删节点（置墓碑）；不在此处校验空目录，那是 rmdir 的语义。
    pub fn remove(&mut self, ino: u16) -> Result<(), i32> {
        let idx = match self.idx(ino) {
            Some(i) => i,
            None => return Err(ENOENT),
        };
        match self.slots.get_mut(idx) {
            Some(slot) => slot.kind = Kind::Free,
            None => return Err(ENOENT),
        }
        Ok(())
    }
}

// ── 工作目录状态机 ────────────────────────────────────────────────────
/// cwd：ino + epoch + stale。epoch 只在成功 chdir 时推进。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cwd {
    pub ino: u16,
    pub epoch: u16,
    pub stale: bool,
}

impl Cwd {
    pub const fn new(root: u16) -> Cwd {
        Cwd { ino: root, epoch: 1, stale: false }
    }

    pub fn is_stale(self) -> bool {
        self.stale
    }
}

// ── 路径切分与定位 ────────────────────────────────────────────────────
/// 切分（折叠斜杠；`..`/`.` 保留为组件交给定位器）。
pub fn split(path: &str) -> Result<([&str; MAX_COMPS], usize, bool), i32> {
    let mut out = [""; MAX_COMPS];
    let mut n = 0usize;
    let absolute = path.starts_with('/');
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
    Ok((out, n, absolute))
}

/// 定位结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Locate {
    Found { parent: u16, ino: u16, kind: Kind },
    MissingLeaf { parent: u16 },
    MissingParent,
    NonDir,
}

/// 从 base 开始定位（绝对路径忽略 base）。
pub fn locate_from(t: &DirTree, base: u16, path: &str) -> Locate {
    let (comps, n, absolute) = match split(path) {
        Ok(v) => v,
        Err(_) => return Locate::MissingParent,
    };
    if n == 0 {
        return if absolute {
            Locate::Found { parent: t.root(), ino: t.root(), kind: Kind::Dir }
        } else {
            Locate::MissingParent
        };
    }
    let mut cur = if absolute { t.root() } else { base };
    let mut i = 0usize;
    while i + 1 < n {
        let c = match comps.get(i).copied() {
            Some(x) => x,
            None => return Locate::MissingParent,
        };
        if c == "." {
            i += 1;
            continue;
        }
        if c == ".." {
            cur = t.parent_of(cur);
            i += 1;
            continue;
        }
        match t.lookup(cur, c) {
            Some(ino) => {
                if t.kind(ino) != Kind::Dir {
                    return Locate::NonDir;
                }
                cur = ino;
            }
            None => return Locate::MissingParent,
        }
        i += 1;
    }
    let leaf = match comps.get(n.saturating_sub(1)).copied() {
        Some(x) => x,
        None => return Locate::MissingParent,
    };
    let parent = cur;
    if leaf == "." {
        return Locate::Found { parent, ino: cur, kind: Kind::Dir };
    }
    if leaf == ".." {
        let up = t.parent_of(cur);
        return Locate::Found { parent: up, ino: up, kind: Kind::Dir };
    }
    match t.lookup(cur, leaf) {
        Some(ino) => Locate::Found { parent, ino, kind: t.kind(ino) },
        None => Locate::MissingLeaf { parent },
    }
}

// ── 效果 ──────────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Effect {
    Created(u16),
    Removed(u16),
    Opened(u16, usize),
    Changed(u16),
    Listed(usize),
}

// ── 六操作判定器 ──────────────────────────────────────────────────────
pub fn do_mkdir(t: &mut DirTree, path: &str) -> Result<Effect, i32> {
    match locate_from(t, t.root(), path) {
        Locate::Found { .. } => Err(EEXIST),
        Locate::NonDir => Err(ENOTDIR),
        Locate::MissingParent => Err(ENOENT),
        Locate::MissingLeaf { parent } => {
            let leaf = match split(path) {
                Ok((comps, n, _)) => match comps.get(n.saturating_sub(1)).copied() {
                    Some(s) => s,
                    None => return Err(ENOENT),
                },
                Err(e) => return Err(e),
            };
            let ino = t.add(parent, leaf, Kind::Dir)?;
            Ok(Effect::Created(ino))
        }
    }
}

pub fn do_rmdir(t: &mut DirTree, cwd: &mut Cwd, path: &str) -> Result<Effect, i32> {
    match locate_from(t, t.root(), path) {
        Locate::Found { ino, kind, .. } => {
            if kind != Kind::Dir {
                return Err(ENOTDIR);
            }
            // POSIX：rmdir(".") 回 EINVAL（点段不是可删对象）
            if path == "." || path == "./" || path.ends_with("/.") {
                return Err(EINVAL);
            }
            if t.child_count(ino) > 0 {
                return Err(ENOTEMPTY);
            }
            t.remove(ino)?;
            t.rmdirs = t.rmdirs.saturating_add(1);
            if cwd.ino == ino {
                cwd.stale = true;
            }
            Ok(Effect::Removed(ino))
        }
        Locate::MissingLeaf { .. } | Locate::MissingParent => Err(ENOENT),
        Locate::NonDir => Err(ENOTDIR),
    }
}

pub fn do_unlink(t: &mut DirTree, path: &str) -> Result<Effect, i32> {
    match locate_from(t, t.root(), path) {
        Locate::Found { ino, kind, .. } => {
            if kind == Kind::Dir {
                return Err(EISDIR);
            }
            t.remove(ino)?;
            t.unlinks = t.unlinks.saturating_add(1);
            Ok(Effect::Removed(ino))
        }
        Locate::MissingLeaf { .. } | Locate::MissingParent => Err(ENOENT),
        Locate::NonDir => Err(ENOTDIR),
    }
}

pub fn do_opendir(t: &DirTree, path: &str) -> Result<Effect, i32> {
    match locate_from(t, t.root(), path) {
        Locate::Found { ino, kind, .. } => {
            if kind != Kind::Dir {
                return Err(ENOTDIR);
            }
            Ok(Effect::Opened(ino, t.child_count(ino)))
        }
        Locate::MissingLeaf { .. } | Locate::MissingParent => Err(ENOENT),
        Locate::NonDir => Err(ENOTDIR),
    }
}

pub fn do_getdents(t: &DirTree, path: &str) -> Result<Effect, i32> {
    match locate_from(t, t.root(), path) {
        Locate::Found { ino, kind, .. } => {
            if kind != Kind::Dir {
                return Err(ENOTDIR);
            }
            Ok(Effect::Listed(t.child_count(ino)))
        }
        Locate::MissingLeaf { .. } | Locate::MissingParent => Err(ENOENT),
        Locate::NonDir => Err(ENOTDIR),
    }
}

/// chdir：成功才推进 epoch；任何失败必须原地回滚（cwd 一字不动）。
pub fn do_chdir(t: &DirTree, cwd: &mut Cwd, path: &str) -> Result<Effect, i32> {
    let absolute = path.starts_with('/');
    if cwd.stale && !absolute {
        return Err(ENOENT);
    }
    let base = if cwd.stale { t.root() } else { cwd.ino };
    match locate_from(t, base, path) {
        Locate::Found { ino, kind, .. } => {
            if kind != Kind::Dir {
                return Err(ENOTDIR);
            }
            cwd.ino = ino;
            cwd.epoch = cwd.epoch.wrapping_add(1);
            cwd.stale = false;
            Ok(Effect::Changed(ino))
        }
        Locate::MissingLeaf { .. } | Locate::MissingParent => Err(ENOENT),
        Locate::NonDir => Err(ENOTDIR),
    }
}

// ── 操作矩阵 ──────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    Mkdir,
    Rmdir,
    Unlink,
    Opendir,
    Chdir,
    Getdents,
}

impl Op {
    pub const ALL: [Op; 6] = [
        Op::Mkdir,
        Op::Rmdir,
        Op::Unlink,
        Op::Opendir,
        Op::Chdir,
        Op::Getdents,
    ];

    pub fn run(self, t: &mut DirTree, cwd: &mut Cwd, path: &str) -> Result<Effect, i32> {
        match self {
            Op::Mkdir => do_mkdir(t, path),
            Op::Rmdir => do_rmdir(t, cwd, path),
            Op::Unlink => do_unlink(t, path),
            Op::Opendir => do_opendir(t, path),
            Op::Chdir => do_chdir(t, cwd, path),
            Op::Getdents => do_getdents(t, path),
        }
    }
}

/// 样本树：/etc{passwd,sub}、/usr。
pub fn fixture() -> DirTree {
    let mut t = DirTree::new();
    let root = t.root();
    let etc = match t.add(root, "etc", Kind::Dir) {
        Ok(i) => i,
        Err(_) => return t,
    };
    let _ = t.add(etc, "passwd", Kind::File);
    let _ = t.add(etc, "sub", Kind::Dir);
    let _ = t.add(root, "usr", Kind::Dir);
    t
}

/// 五列目标形态。
pub const COLUMNS: [&str; 5] = ["/etc", "/etc/passwd", "/etc/ghost", "/ghost/x", "/etc/passwd/sub"];

/// 30 格期望码（0 = 成功）。
pub const EXPECTED: [[i32; 5]; 6] = [
    // mkdir：目录在/文件在均 EEXIST；缺失尾段可建；缺父 ENOENT；父是文件 ENOTDIR
    [EEXIST, EEXIST, 0, ENOENT, ENOTDIR],
    // rmdir：非空 ENOTEMPTY；文件 ENOTDIR；两缺失 ENOENT；父是文件 ENOTDIR
    [ENOTEMPTY, ENOTDIR, ENOENT, ENOENT, ENOTDIR],
    // unlink：目录 EISDIR；文件可删；两缺失 ENOENT；父是文件 ENOTDIR
    [EISDIR, 0, ENOENT, ENOENT, ENOTDIR],
    // opendir
    [0, ENOTDIR, ENOENT, ENOENT, ENOTDIR],
    // chdir
    [0, ENOTDIR, ENOENT, ENOENT, ENOTDIR],
    // getdents
    [0, ENOTDIR, ENOENT, ENOENT, ENOTDIR],
];

/// 逐格执行结果。
#[derive(Clone, Copy, Debug)]
pub struct Grid {
    pub codes: [[i32; 5]; 6],
    pub ok_cells: u32,
    pub enoent_cells: u32,
    pub notdir_cells: u32,
    pub isdir_cells: u32,
}

pub fn run_grid() -> Grid {
    let mut codes = [[0i32; 5]; 6];
    let mut ok_cells = 0u32;
    let mut enoent_cells = 0u32;
    let mut notdir_cells = 0u32;
    let mut isdir_cells = 0u32;
    let mut ri = 0usize;
    while ri < Op::ALL.len() {
        if let Some(op) = Op::ALL.get(ri).copied() {
            let mut ci = 0usize;
            while ci < COLUMNS.len() {
                if let Some(path) = COLUMNS.get(ci).copied() {
                    let mut t = fixture();
                    let mut cwd = Cwd::new(t.root());
                    let code = match op.run(&mut t, &mut cwd, path) {
                        Ok(_) => 0,
                        Err(e) => e,
                    };
                    match code {
                        0 => ok_cells = ok_cells.saturating_add(1),
                        ENOENT => enoent_cells = enoent_cells.saturating_add(1),
                        ENOTDIR => notdir_cells = notdir_cells.saturating_add(1),
                        EISDIR => isdir_cells = isdir_cells.saturating_add(1),
                        _ => {}
                    }
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
    Grid { codes, ok_cells, enoent_cells, notdir_cells, isdir_cells }
}

// ── 注入账 ────────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DirLedger {
    pub shots: u32,
    pub detected: u32,
    pub escaped: u32,
    pub enoent: u32,
    pub enotdir: u32,
    pub eisdir: u32,
}

impl DirLedger {
    pub const fn zero() -> DirLedger {
        DirLedger { shots: 0, detected: 0, escaped: 0, enoent: 0, enotdir: 0, eisdir: 0 }
    }

    fn record(&mut self, want: i32, got: i32, slot: usize) {
        self.shots = self.shots.saturating_add(1);
        if got == want {
            self.detected = self.detected.saturating_add(1);
            match slot {
                0 => self.enoent = self.enoent.saturating_add(1),
                1 => self.enotdir = self.enotdir.saturating_add(1),
                _ => self.eisdir = self.eisdir.saturating_add(1),
            }
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.shots
            && self.enoent.saturating_add(self.enotdir).saturating_add(self.eisdir) == self.detected
    }
}

/// 三码注入 × 10 轮：ENOENT 三格、ENOTDIR 三格、EISDIR 一格。
pub fn run_injection() -> DirLedger {
    let mut ledger = DirLedger::zero();
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut t = fixture();
        let mut cwd = Cwd::new(t.root());
        // ENOENT 面
        ledger.record(ENOENT, match do_rmdir(&mut t, &mut cwd, "/etc/ghost") {
            Ok(_) => 0,
            Err(e) => e,
        }, 0);
        ledger.record(ENOENT, match do_chdir(&t, &mut cwd, "/etc/ghost") {
            Ok(_) => 0,
            Err(e) => e,
        }, 0);
        ledger.record(ENOENT, match do_opendir(&t, "/ghost/x") {
            Ok(_) => 0,
            Err(e) => e,
        }, 0);
        // ENOTDIR 面
        ledger.record(ENOTDIR, match do_opendir(&t, "/etc/passwd") {
            Ok(_) => 0,
            Err(e) => e,
        }, 1);
        ledger.record(ENOTDIR, match do_chdir(&t, &mut cwd, "/etc/passwd") {
            Ok(_) => 0,
            Err(e) => e,
        }, 1);
        ledger.record(ENOTDIR, match do_getdents(&t, "/etc/passwd") {
            Ok(_) => 0,
            Err(e) => e,
        }, 1);
        // EISDIR 面
        ledger.record(EISDIR, match do_unlink(&mut t, "/etc") {
            Ok(_) => 0,
            Err(e) => e,
        }, 2);
        round = round.saturating_add(1);
    }
    ledger
}

// ── 对账表 ────────────────────────────────────────────────────────────
/// (op, 目标, 期望码)
pub const RECON: &[(&str, &str, i32)] = &[
    ("mkdir", "/etc", EEXIST),
    ("mkdir", "/etc/ghost", 0),
    ("mkdir", "/ghost/x", ENOENT),
    ("mkdir", "/etc/passwd/sub", ENOTDIR),
    ("rmdir", "/etc", ENOTEMPTY),
    ("rmdir", "/etc/passwd", ENOTDIR),
    ("rmdir", "/etc/sub", 0),
    ("unlink", "/etc", EISDIR),
    ("unlink", "/etc/passwd", 0),
    ("opendir", "/etc/passwd", ENOTDIR),
    ("chdir", "/etc/sub", 0),
    ("getdents", "/etc", 0),
];

/// 与相邻域的口径分工（显性，不重叠）。
pub const SCOPE: &[(&str, &str)] = &[
    ("open/stat/unlink 三码", "F9227 负责；本域只做目录操作面"),
    ("dirfd 与 AT_FDCWD", "F9229 负责；本域路径一律从根/ cwd 起算"),
    ("fd 复用竞态", "F9232 负责；本域不建模并发"),
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

/// 指纹：30 格 + chdir 状态机三步（chdir / 失败回滚 / stale）。
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
    let mut t = fixture();
    let mut cwd = Cwd::new(t.root());
    let step1 = match do_chdir(&t, &mut cwd, "/etc/sub") {
        Ok(_) => cwd.epoch,
        Err(_) => 0,
    };
    bytes.push((step1 & 0xFF) as u8);
    let step2 = match do_chdir(&t, &mut cwd, "/etc/passwd") {
        Ok(_) => 1u8,
        Err(_) => 0u8,
    };
    bytes.push(step2);
    let _ = do_rmdir(&mut t, &mut cwd, "/etc/sub");
    bytes.push(if cwd.is_stale() { 1u8 } else { 0u8 });
    let step3 = match do_chdir(&t, &mut cwd, "x") {
        Ok(_) => 0u8,
        Err(e) => (e & 0xFF) as u8,
    };
    bytes.push(step3);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
pub fn run_unx9228_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9228");

    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 正常流：mkdir 建目录
    let mut t = fixture();
    let mk = do_mkdir(&mut t, "/etc/newdir");
    let mk_ok = mk == Ok(Effect::Created(6))
        && t.kind(6) == Kind::Dir
        && t.child_count(2) == 3
        && t.mkdirs() == 4;
    s.add("正常流：mkdir 建目录、父子项 +1、mkdir 账 +1", mk_ok,
          "新目录 ino=6 挂在 /etc 下；样本树已有 3 个目录，mkdir 账应为 4");

    // 正常流：rmdir 空目录
    let mut cwd = Cwd::new(t.root());
    let rm = do_rmdir(&mut t, &mut cwd, "/etc/sub");
    let rm_ok = rm == Ok(Effect::Removed(4)) && t.kind(4) == Kind::Free && t.rmdirs() == 1;
    s.add("正常流：rmdir 删空目录并置墓碑", rm_ok,
          "空目录可删，rmdir 账 +1，inode 转 Free");

    // 正常流：opendir/getdents 计数
    let t2 = fixture();
    let od = do_opendir(&t2, "/etc");
    let gd = do_getdents(&t2, "/etc");
    let list_ok = od == Ok(Effect::Opened(2, 2)) && gd == Ok(Effect::Listed(2));
    s.add("正常流：opendir/getdents 返回子项数 2", list_ok,
          "/etc 下有 passwd 与 sub 两个子项，两条读目录路径必须一致");

    // 30 格矩阵逐格
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
    s.add("矩阵：6 操作 × 5 形态 = 30 格逐格复算", grid_ok,
          "整表复算：三错误码只是同一张表的三个区带，缺格即判红");

    // 缺失面：mkdir 的尾段缺失是「可建」正常流，其余 5 个操作的尾段缺失必须是
    // ENOENT；中间段缺失对 6 个操作一律 ENOENT。
    let mut tail_ok = true;
    let mut ri2 = 1usize;
    while ri2 < EXPECTED.len() {
        if let Some(row) = EXPECTED.get(ri2) {
            if row.get(2).copied() != Some(ENOENT) {
                tail_ok = false;
            }
        }
        ri2 += 1;
    }
    let mut mid_ok = true;
    let mut ri3 = 0usize;
    while ri3 < EXPECTED.len() {
        if let Some(row) = EXPECTED.get(ri3) {
            if row.get(3).copied() != Some(ENOENT) {
                mid_ok = false;
            }
        }
        ri3 += 1;
    }
    s.add("缺失面：5 操作尾段缺失 + 6 操作中间段缺失全 ENOENT",
          tail_ok && mid_ok && g.enoent_cells == 11,
          "11 个缺失格必须全部 ENOENT；mkdir 尾段缺失属正常流，不计入缺失面");

    // 文件作目录全 ENOTDIR
    let file_as_dir_ok = g.notdir_cells == 10
        && g.codes.get(1).and_then(|r| r.get(1)).copied() == Some(ENOTDIR)
        && g.codes.get(3).and_then(|r| r.get(1)).copied() == Some(ENOTDIR)
        && g.codes.get(4).and_then(|r| r.get(1)).copied() == Some(ENOTDIR)
        && g.codes.get(5).and_then(|r| r.get(1)).copied() == Some(ENOTDIR);
    s.add("文件作目录：rmdir/opendir/chdir/getdents 全 ENOTDIR", file_as_dir_ok,
          "10 个 ENOTDIR 格（含 5 个父是文件格）一个都不许改码");

    // EISDIR 面
    let isdir_ok = g.isdir_cells == 1
        && g.codes.get(2).and_then(|r| r.get(0)).copied() == Some(EISDIR)
        && do_unlink(&mut fixture(), "/etc/sub") == Err(EISDIR);
    s.add("目录作文件：unlink 目录回 EISDIR（且仅此一格）", isdir_ok,
          "EISDIR 只属于 unlink 目录；rmdir 打文件必须是 ENOTDIR 而非 EISDIR");

    // 判序：父是文件先于末段缺失
    let t3 = fixture();
    let order_ok = do_mkdir(&mut fixture(), "/etc/passwd/sub") == Err(ENOTDIR)
        && do_opendir(&t3, "/etc/passwd/sub") == Err(ENOTDIR)
        && do_mkdir(&mut fixture(), "/ghost/passwd") == Err(ENOENT);
    s.add("判定次序：父是文件 ENOTDIR 先于末段 ENOENT", order_ok,
          "/etc/passwd/sub 同时满足「末段缺失」与「父非目录」，答案只能是 ENOTDIR");

    // 注入必抓
    let ledger = run_injection();
    let inject_ok = ledger.shots == INJECT_ROUNDS * 7
        && ledger.detected == ledger.shots
        && ledger.escaped == 0
        && ledger.enoent == INJECT_ROUNDS * 3
        && ledger.enotdir == INJECT_ROUNDS * 3
        && ledger.eisdir == INJECT_ROUNDS;
    s.add("注入必抓：三码 × 10 轮全检出、逃逸 0", inject_ok,
          "70 次注入（ENOENT 30 / ENOTDIR 30 / EISDIR 10）必须全抓");
    s.add("账目守恒：注入=检出+逃逸且三码分格之和=检出", ledger.conserved(),
          "分格计数与总数不等说明有注入未归类");

    // cwd 状态机：失败回滚
    let t4 = fixture();
    let mut c4 = Cwd::new(t4.root());
    let ok_chdir = do_chdir(&t4, &mut c4, "/etc/sub");
    let epoch_after = c4.epoch;
    let bad_chdir = do_chdir(&t4, &mut c4, "/etc/passwd");
    let rollback_ok = ok_chdir == Ok(Effect::Changed(4))
        && epoch_after == 2
        && bad_chdir == Err(ENOTDIR)
        && c4.epoch == epoch_after
        && c4.ino == 4
        && !c4.is_stale();
    s.add("cwd 状态机：失败 chdir 原地回滚（ino/epoch/stale 不动）", rollback_ok,
          "epoch 只在成功时推进；失败路径改 cwd 就是状态污染");

    // cwd 状态机：删掉 cwd 后 stale
    let mut t5 = fixture();
    let mut c5 = Cwd::new(t5.root());
    let _ = do_chdir(&mut t5, &mut c5, "/etc/sub");
    let removed = do_rmdir(&mut t5, &mut c5, "/etc/sub");
    // stale 必须在绝对 chdir 洗掉之前取样，否则测的是恢复后的状态。
    let stale_after_rmdir = c5.is_stale();
    let rel_after = do_chdir(&t5, &mut c5, "x");
    let abs_after = do_chdir(&t5, &mut c5, "/etc");
    let stale_ok = removed == Ok(Effect::Removed(4))
        && stale_after_rmdir
        && rel_after == Err(ENOENT)
        && abs_after == Ok(Effect::Changed(2))
        && !c5.is_stale();
    s.add("cwd 状态机：cwd 被删后 stale，相对 ENOENT、绝对可恢复", stale_ok,
          "stale 只能用绝对路径 chdir 洗掉；相对路径必须 ENOENT");

    // 扇出容量
    let mut t6 = fixture();
    let usr = match t6.lookup(t6.root(), "usr") {
        Some(i) => i,
        None => 0,
    };
    let mut made = 0usize;
    let mut k = 0usize;
    while k < DIR_FANOUT {
        let mut buf = [0u8; 2];
        buf[0] = b'a' + (k as u8);
        buf[1] = b'x';
        let nm = match core::str::from_utf8(&buf) {
            Ok(v) => v,
            Err(_) => "",
        };
        if t6.add(usr, nm, Kind::Dir).is_ok() {
            made += 1;
        }
        k += 1;
    }
    let fanout_reject = t6.add(usr, "zz", Kind::Dir);
    let fanout_ok = made == DIR_FANOUT
        && fanout_reject == Err(ENOSPC)
        && t6.child_count(usr) == DIR_FANOUT;
    s.add("容量：单目录扇出 6 个后第 7 个回 ENOSPC", fanout_ok,
          "扇出上限是冻结值，拒绝新增时既有子项一个不丢");

    // 全局节点容量
    let mut t7 = DirTree::new();
    let mut parent = t7.root();
    let mut n = 0usize;
    while n < MAX_NODES {
        let mut buf = [0u8; 2];
        buf[0] = b'a' + ((n % 26) as u8);
        buf[1] = b'0' + ((n / 26) as u8);
        let nm = match core::str::from_utf8(&buf) {
            Ok(v) => v,
            Err(_) => "",
        };
        match t7.add(parent, nm, Kind::Dir) {
            Ok(ino) => parent = ino,
            Err(_) => {}
        }
        n += 1;
    }
    let node_reject = t7.add(parent, "zz", Kind::Dir);
    let node_ok = t7.used() == MAX_NODES && node_reject == Err(ENOSPC);
    s.add("容量：全局节点满回 ENOSPC 且 used 不越界", node_ok,
          "表满只拒绝新增，used 必须恰好停在 MAX_NODES");

    // 非空目录
    let nonempty_ok = do_rmdir(&mut fixture(), &mut Cwd::new(1), "/etc") == Err(ENOTEMPTY)
        && do_rmdir(&mut fixture(), &mut Cwd::new(1), "/usr") == Ok(Effect::Removed(5));
    s.add("语义：非空目录 ENOTEMPTY、空目录可删", nonempty_ok,
          "/etc 有两个子项、/usr 为空，两格必须是不同结果");

    // 点段删除
    let dot_ok = do_rmdir(&mut fixture(), &mut Cwd::new(1), ".") == Err(EINVAL);
    s.add("语义：rmdir(\".\") 按 POSIX 回 EINVAL", dot_ok,
          "点段不是可删对象，误删 cwd 属于破坏性缺陷");

    // 对账表
    let mut recon_ok = true;
    let mut i3 = 0usize;
    while i3 < RECON.len() {
        if let Some(row) = RECON.get(i3) {
            let mut tt = fixture();
            let mut cc = Cwd::new(tt.root());
            let op = match row.0 {
                "mkdir" => Some(Op::Mkdir),
                "rmdir" => Some(Op::Rmdir),
                "unlink" => Some(Op::Unlink),
                "opendir" => Some(Op::Opendir),
                "chdir" => Some(Op::Chdir),
                "getdents" => Some(Op::Getdents),
                _ => None,
            };
            let code = match op {
                Some(o) => match o.run(&mut tt, &mut cc, row.1) {
                    Ok(_) => 0,
                    Err(e) => e,
                },
                None => -1,
            };
            if code != row.2 {
                recon_ok = false;
            }
        }
        i3 += 1;
    }
    s.add("对账：12 行目录操作对照表逐行复算相等", recon_ok,
          "对照表与判定器必须同源；行内码对不上即判红");

    // 双跑一致
    let fp1 = corpus_fingerprint();
    let fp2 = corpus_fingerprint();
    s.add("双跑一致：语料指纹相等", fp1 == fp2,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 文案闭集
    let text_ok = DirCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带非空中文建议", text_ok,
          "13 个码都必须能取到可操作说明");

    // 码唯一
    let mut uniq = true;
    let mut a = 0usize;
    while a < DirCode::ALL.len() {
        let mut b = a + 1;
        while b < DirCode::ALL.len() {
            if let (Some(x), Some(y)) = (DirCode::ALL.get(a), DirCode::ALL.get(b)) {
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
    let before = mono.shots;
    mono.record(ENOENT, ENOENT, 0);
    s.add("账目单调：再注入只增不减且守恒", mono.shots == before.saturating_add(1) && mono.conserved(),
          "累计计数禁止回退");

    // 域界分工
    let scope_ok = SCOPE.len() == 3 && SCOPE.iter().all(|r| !r.0.is_empty() && !r.1.is_empty());
    s.add("对账：与相邻域的分工逐条登记", scope_ok,
          "三码面、dirfd 面、并发面必须显性分工，不许抢判据");

    let grid_text = format!(
        "30 格 ok={} enoent={} enotdir={} eisdir={}",
        g.ok_cells, g.enoent_cells, g.notdir_cells, g.isdir_cells
    );
    s.add_dyn("矩阵快照", grid_ok, &grid_text);
    let ledger_text = format!(
        "注入 {} 检出 {} 逃逸 {} | ENOENT {} ENOTDIR {} EISDIR {}",
        ledger.shots, ledger.detected, ledger.escaped, ledger.enoent, ledger.enotdir, ledger.eisdir
    );
    s.add_dyn("注入账", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", fp1);
    s.add_dyn("指纹快照", fp1 == fp2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn code(r: Result<Effect, i32>) -> i32 {
        match r {
            Ok(_) => 0,
            Err(e) => e,
        }
    }

    #[test]
    fn mkdir_and_rmdir_round_trip() {
        let mut t = fixture();
        let mut cwd = Cwd::new(t.root());
        assert_eq!(do_mkdir(&mut t, "/usr/lib"), Ok(Effect::Created(6)));
        assert_eq!(t.child_count(5), 1);
        assert_eq!(do_rmdir(&mut t, &mut cwd, "/usr/lib"), Ok(Effect::Removed(6)));
        assert_eq!(t.kind(6), Kind::Free);
        assert_eq!(t.rmdirs(), 1);
    }

    #[test]
    fn grid_all_cells() {
        let g = run_grid();
        for (ri, row) in EXPECTED.iter().enumerate() {
            for (ci, want) in row.iter().enumerate() {
                let got = g.codes.get(ri).and_then(|r| r.get(ci)).copied().unwrap_or(-1);
                assert_eq!(got, *want, "row={} col={}", ri, ci);
            }
        }
        assert_eq!(g.ok_cells, 5);
        assert_eq!(g.enoent_cells, 11);
        assert_eq!(g.notdir_cells, 10);
        assert_eq!(g.isdir_cells, 1);
    }

    #[test]
    fn file_as_dir_is_enotdir() {
        let t = fixture();
        assert_eq!(do_opendir(&t, "/etc/passwd"), Err(ENOTDIR));
        assert_eq!(do_getdents(&t, "/etc/passwd"), Err(ENOTDIR));
        assert_eq!(code(do_chdir(&t, &mut Cwd::new(t.root()), "/etc/passwd")), ENOTDIR);
        assert_eq!(do_rmdir(&mut fixture(), &mut Cwd::new(1), "/etc/passwd"), Err(ENOTDIR));
        assert_eq!(do_mkdir(&mut fixture(), "/etc/passwd/sub"), Err(ENOTDIR));
    }

    #[test]
    fn dir_as_file_is_eisdir() {
        assert_eq!(do_unlink(&mut fixture(), "/etc"), Err(EISDIR));
        assert_eq!(do_unlink(&mut fixture(), "/etc/sub"), Err(EISDIR));
        let mut t = fixture();
        assert_eq!(do_unlink(&mut t, "/etc/passwd"), Ok(Effect::Removed(3)));
        assert_eq!(t.unlinks(), 1);
        assert_eq!(t.kind(3), Kind::Free);
    }

    #[test]
    fn missing_paths_are_enoent() {
        let t = fixture();
        assert_eq!(do_opendir(&t, "/etc/ghost"), Err(ENOENT));
        assert_eq!(do_opendir(&t, "/ghost/x"), Err(ENOENT));
        assert_eq!(do_mkdir(&mut fixture(), "/ghost/x"), Err(ENOENT));
        assert_eq!(do_unlink(&mut fixture(), "/etc/ghost"), Err(ENOENT));
    }

    #[test]
    fn cwd_rollback_on_failure() {
        let t = fixture();
        let mut cwd = Cwd::new(t.root());
        assert_eq!(do_chdir(&t, &mut cwd, "/etc/sub"), Ok(Effect::Changed(4)));
        let epoch = cwd.epoch;
        assert_eq!(do_chdir(&t, &mut cwd, "/etc/ghost"), Err(ENOENT));
        assert_eq!(do_chdir(&t, &mut cwd, "/etc/passwd"), Err(ENOTDIR));
        assert_eq!(cwd.epoch, epoch);
        assert_eq!(cwd.ino, 4);
        assert!(!cwd.is_stale());
    }

    #[test]
    fn cwd_stale_after_rmdir() {
        let mut t = fixture();
        let mut cwd = Cwd::new(t.root());
        assert_eq!(do_chdir(&t, &mut cwd, "/etc/sub"), Ok(Effect::Changed(4)));
        assert_eq!(do_rmdir(&mut t, &mut cwd, "/etc/sub"), Ok(Effect::Removed(4)));
        assert!(cwd.is_stale());
        assert_eq!(do_chdir(&t, &mut cwd, "rel"), Err(ENOENT));
        assert_eq!(do_chdir(&t, &mut cwd, "/etc"), Ok(Effect::Changed(2)));
        assert!(!cwd.is_stale());
    }

    #[test]
    fn capacity_fanout_and_nodes() {
        let mut t = fixture();
        let usr = t.lookup(t.root(), "usr").unwrap_or(0);
        for k in 0..DIR_FANOUT {
            let mut buf = [0u8; 2];
            buf[0] = b'a' + (k as u8);
            buf[1] = b'x';
            let nm = core::str::from_utf8(&buf).unwrap_or("");
            assert!(t.add(usr, nm, Kind::Dir).is_ok());
        }
        assert_eq!(t.add(usr, "zz", Kind::Dir), Err(ENOSPC));
        assert_eq!(t.child_count(usr), DIR_FANOUT);

        let mut t2 = DirTree::new();
        let mut parent = t2.root();
        for n in 0..MAX_NODES {
            let mut buf = [0u8; 2];
            buf[0] = b'a' + ((n % 26) as u8);
            buf[1] = b'0' + ((n / 26) as u8);
            let nm = core::str::from_utf8(&buf).unwrap_or("");
            if let Ok(ino) = t2.add(parent, nm, Kind::Dir) {
                parent = ino;
            }
        }
        assert_eq!(t2.used(), MAX_NODES);
        assert_eq!(t2.add(parent, "zz", Kind::Dir), Err(ENOSPC));
    }

    #[test]
    fn nonempty_and_dot_rules() {
        assert_eq!(do_rmdir(&mut fixture(), &mut Cwd::new(1), "/etc"), Err(ENOTEMPTY));
        assert_eq!(do_rmdir(&mut fixture(), &mut Cwd::new(1), "."), Err(EINVAL));
        assert_eq!(do_rmdir(&mut fixture(), &mut Cwd::new(1), "/usr"), Ok(Effect::Removed(5)));
    }

    #[test]
    fn injection_ledger_and_determinism() {
        let l = run_injection();
        assert_eq!(l.shots, 70);
        assert_eq!(l.detected, 70);
        assert_eq!(l.escaped, 0);
        assert!(l.conserved());
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9228_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(s.len() >= 12);
        assert!(!s.truncated());
    }
}
