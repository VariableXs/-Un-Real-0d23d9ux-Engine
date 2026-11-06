//! UNX-F9230 dirfd 坏值矩阵（AT_EMPTY_PATH/AT_SYMLINK_FOLLOW） · 0x7A700段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9230 逐字）：
//!   ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9230-J1 两标志判据过，坏 dirfd
//!   全 EBADF 格过，非目录 fd 全 ENOTDIR 格过；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 两个标志不是同一族都能用：`fstatat` 合法集是
//!      NOFOLLOW|EMPTY_PATH，`linkat` 合法集是 FOLLOW|EMPTY_PATH。把
//!      AT_SYMLINK_FOLLOW 传给 fstatat、或把 NOFOLLOW 传给 linkat，一律
//!      EINVAL——这是本卡最容易照抄错的一格。
//!   ② AT_EMPTY_PATH 的正判据：空路径 + 该标志 + **任意**有效 fd（目录或
//!      普通文件都行）→ 直接对 fd 自身对象操作。所以「非目录 fd 全 ENOTDIR」
//!      有一条显性例外：AT_EMPTY_PATH 下文件 fd 合法，这一格单列，不许被
//!      ENOTDIR 判据吞掉。
//!   ③ 空路径的判序按 `getname_flags`：未置 AT_EMPTY_PATH 时先回 ENOENT，
//!      早于 fd 校验；置了 AT_EMPTY_PATH 才走到 fd，坏 fd 才回 EBADF。
//!   ④ 非空相对路径：坏 dirfd（负数/越界/已关）→ EBADF；合法但非目录 →
//!      ENOTDIR；且 fd 面先于查找面（目标不存在也得先报 ENOTDIR/EBADF）。
//!   ⑤ 绝对路径忽略 dirfd（首字符 / 钉 root），与 F9229 同源；本卡只复验
//!      它在两标志语境下不被破坏。
//!   ⑥ 跟随语义：statat 默认跟随、NOFOLLOW 关；linkat 默认不跟随、FOLLOW 开。
//!      悬挂链接在跟随面 ENOENT、不跟随面成功——两面对照才能锁住语义。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x7A700;

/// 动态 fd 槽数（号段 3..=8）。
pub const MAX_FDS: usize = 6;
pub const FIRST_FD: i32 = 3;
pub const AT_FDCWD: i32 = -100;

/// 标志位（Linux 原值）。
pub const AT_SYMLINK_NOFOLLOW: u32 = 0x100;
pub const AT_SYMLINK_FOLLOW: u32 = 0x400;
pub const AT_EMPTY_PATH: u32 = 0x1000;

pub const ENOENT: i32 = 2;
pub const EBADF: i32 = 9;
pub const ENOTDIR: i32 = 20;
pub const EINVAL: i32 = 22;
pub const EMFILE: i32 = 24;

/// 样本对象号。
pub const OBJ_ROOT: u16 = 10;
pub const OBJ_DIR: u16 = 10;
pub const OBJ_FILE: u16 = 20;
pub const OBJ_LINK_DANGLING: u16 = 30;
pub const OBJ_LINK_TO_FILE: u16 = 31;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlagCode {
    FlagNotAllowed,
    EmptyPathNoFlag,
    EmptyPathBadFd,
    EmptyPathFileFd,
    BadDirfd,
    NonDirDirfd,
    DanglingFollowed,
    DanglingKept,
    AboveFdLimit,
    FdTableFull,
    StaleGeneration,
    LedgerRegress,
    NotReproducible,
    SegmentViolation,
}

impl FlagCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            FlagCode::FlagNotAllowed => "标志族不匹配：fstatat 只收 NOFOLLOW|EMPTY_PATH、\
                linkat 只收 FOLLOW|EMPTY_PATH；越集即 EINVAL，先于路径与 fd",
            FlagCode::EmptyPathNoFlag => "空路径未置 AT_EMPTY_PATH：按 getname_flags 回\
                ENOENT，且早于 fd 校验；请补标志或给出真实路径",
            FlagCode::EmptyPathBadFd => "空路径 + AT_EMPTY_PATH 但 fd 无效：回 EBADF；\
                该路径下 fd 是唯一对象来源，必须有效",
            FlagCode::EmptyPathFileFd => "空路径 + AT_EMPTY_PATH 且 fd 是普通文件：合法，\
                直接对 fd 自身对象操作；这是 ENOTDIR 面的显性例外",
            FlagCode::BadDirfd => "坏 dirfd：负值（AT_FDCWD 之外）/越界/已关一律 EBADF；\
                请先核对 fd 来源",
            FlagCode::NonDirDirfd => "非目录 dirfd：非空相对路径必须挂在目录 fd 上，回\
                ENOTDIR；AT_EMPTY_PATH 面不受此限",
            FlagCode::DanglingFollowed => "悬挂链接被跟随：目标不存在，回 ENOENT；\
                若只需要链接本身请去掉 FOLLOW / 加 NOFOLLOW",
            FlagCode::DanglingKept => "悬挂链接不跟随：返回链接自身对象，成功；\
                这是 linkat 默认语义，不要误加 FOLLOW",
            FlagCode::AboveFdLimit => "dirfd 越上界：号大于 FIRST_FD+MAX_FDS 回 EBADF；\
                请按 3..=8 分配",
            FlagCode::FdTableFull => "fd 表满：分配回 EMFILE；请先 close 回收",
            FlagCode::StaleGeneration => "陈旧代际：槽位已复用，旧代际句柄不可用；\
                请用新代际重建",
            FlagCode::LedgerRegress => "注入账倒退：计数只增不减；请检查回滚路径",
            FlagCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏\
                状态或非确定遍历序",
            FlagCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    pub const ALL: [FlagCode; 14] = [
        FlagCode::FlagNotAllowed,
        FlagCode::EmptyPathNoFlag,
        FlagCode::EmptyPathBadFd,
        FlagCode::EmptyPathFileFd,
        FlagCode::BadDirfd,
        FlagCode::NonDirDirfd,
        FlagCode::DanglingFollowed,
        FlagCode::DanglingKept,
        FlagCode::AboveFdLimit,
        FlagCode::FdTableFull,
        FlagCode::StaleGeneration,
        FlagCode::LedgerRegress,
        FlagCode::NotReproducible,
        FlagCode::SegmentViolation,
    ];
}

// ── fd 表 ─────────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FdKind {
    Free,
    Dir,
    File,
}

#[derive(Clone, Copy, Debug)]
pub struct FdSlot {
    kind: FdKind,
    obj: u16,
    generation: u16,
}

impl FdSlot {
    const fn blank() -> FdSlot {
        FdSlot { kind: FdKind::Free, obj: 0, generation: 1 }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FdTable {
    slots: [FdSlot; MAX_FDS],
    opens: u32,
    closes: u32,
}

impl FdTable {
    pub const fn new() -> FdTable {
        FdTable { slots: [FdSlot::blank(); MAX_FDS], opens: 0, closes: 0 }
    }

    fn idx(fd: i32) -> Option<usize> {
        if fd < FIRST_FD {
            return None;
        }
        let i = (fd - FIRST_FD) as usize;
        if i >= MAX_FDS {
            return None;
        }
        Some(i)
    }

    pub fn open(&mut self, kind: FdKind, obj: u16) -> Result<(i32, u16), i32> {
        if kind == FdKind::Free {
            return Err(EINVAL);
        }
        let mut i = 0usize;
        while i < MAX_FDS {
            let take = match self.slots.get(i) {
                Some(s) => s.kind == FdKind::Free,
                None => false,
            };
            if take {
                let gen = match self.slots.get(i) {
                    Some(s) => s.generation,
                    None => 1,
                };
                match self.slots.get_mut(i) {
                    Some(s) => {
                        s.kind = kind;
                        s.obj = obj;
                    }
                    None => return Err(EMFILE),
                }
                self.opens = self.opens.saturating_add(1);
                return Ok((FIRST_FD + i as i32, gen));
            }
            i += 1;
        }
        Err(EMFILE)
    }

    pub fn close(&mut self, fd: i32) -> Result<u16, i32> {
        let i = match Self::idx(fd) {
            Some(i) => i,
            None => return Err(EBADF),
        };
        let live = match self.slots.get(i) {
            Some(s) => s.kind != FdKind::Free,
            None => false,
        };
        if !live {
            return Err(EBADF);
        }
        let gen = match self.slots.get_mut(i) {
            Some(s) => {
                s.kind = FdKind::Free;
                s.obj = 0;
                s.generation = s.generation.wrapping_add(1);
                s.generation
            }
            None => return Err(EBADF),
        };
        self.closes = self.closes.saturating_add(1);
        Ok(gen)
    }

    pub fn describe(&self, fd: i32) -> Result<(FdKind, u16), i32> {
        let i = match Self::idx(fd) {
            Some(i) => i,
            None => return Err(EBADF),
        };
        match self.slots.get(i) {
            Some(s) => {
                if s.kind == FdKind::Free {
                    Err(EBADF)
                } else {
                    Ok((s.kind, s.obj))
                }
            }
            None => Err(EBADF),
        }
    }

    pub fn describe_gen(&self, fd: i32, gen: u16) -> Result<FdKind, i32> {
        let i = match Self::idx(fd) {
            Some(i) => i,
            None => return Err(EBADF),
        };
        match self.slots.get(i) {
            Some(s) => {
                if s.kind == FdKind::Free {
                    Err(EBADF)
                } else if s.generation != gen {
                    Err(EBADF)
                } else {
                    Ok(s.kind)
                }
            }
            None => Err(EBADF),
        }
    }

    pub fn opens(&self) -> u32 {
        self.opens
    }

    pub fn closes(&self) -> u32 {
        self.closes
    }

    pub fn open_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < MAX_FDS {
            if let Some(s) = self.slots.get(i) {
                if s.kind != FdKind::Free {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

// ── 名字空间 ──────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObjKind {
    Dir,
    File,
    Symlink,
}

/// 一级名字表（本域是标志语义域，名字空间只做最小可判模型）。
fn lookup_one(name: &str) -> Option<(ObjKind, u16, &'static str)> {
    match name {
        "dir" => Some((ObjKind::Dir, OBJ_DIR, "")),
        "file" => Some((ObjKind::File, OBJ_FILE, "")),
        "dangling" => Some((ObjKind::Symlink, OBJ_LINK_DANGLING, "ghost")),
        "link2file" => Some((ObjKind::Symlink, OBJ_LINK_TO_FILE, "file")),
        _ => None,
    }
}

/// 路径解析：`/` 开头从 root；一级名 + 可选二级（只有目录能下钻）。
pub fn lookup_path(path: &str, follow: bool) -> Result<u16, i32> {
    let body = match path.strip_prefix('/') {
        Some(rest) => rest,
        None => path,
    };
    if body.is_empty() {
        // "/" 即根目录
        return if path.is_empty() { Err(ENOENT) } else { Ok(OBJ_ROOT) };
    }
    let mut parts = body.split('/');
    let head = match parts.next() {
        Some(h) => h,
        None => return Err(ENOENT),
    };
    if head.is_empty() {
        return Ok(OBJ_ROOT);
    }
    let base = match lookup_one(head) {
        Some(v) => v,
        None => return Err(ENOENT),
    };
    if parts.next().is_some() {
        // 二级组件：只有目录能下钻
        return if base.0 == ObjKind::Dir { Err(ENOENT) } else { Err(ENOTDIR) };
    }
    match base.0 {
        ObjKind::Symlink => {
            if !follow {
                return Ok(base.1);
            }
            if base.2.is_empty() {
                return Err(ENOENT);
            }
            match lookup_one(base.2) {
                Some((_k, id, _t)) => Ok(id),
                None => Err(ENOENT),
            }
        }
        _ => Ok(base.1),
    }
}

// ── at 族 ─────────────────────────────────────────────────────────────
/// 两个 syscall 族：fstatat 与 linkat。标志合法集不同是本卡的骨架。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    StatAt,
    LinkAt,
}

impl Family {
    pub const ALL: [Family; 2] = [Family::StatAt, Family::LinkAt];

    pub fn allowed(self) -> u32 {
        match self {
            Family::StatAt => AT_SYMLINK_NOFOLLOW | AT_EMPTY_PATH,
            Family::LinkAt => AT_SYMLINK_FOLLOW | AT_EMPTY_PATH,
        }
    }

    /// 该族「跟随后缀」语义：statat 默认跟随，linkat 默认不跟随。
    pub fn follow(self, flags: u32) -> bool {
        match self {
            Family::StatAt => flags & AT_SYMLINK_NOFOLLOW == 0,
            Family::LinkAt => flags & AT_SYMLINK_FOLLOW != 0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Family::StatAt => "fstatat",
            Family::LinkAt => "linkat",
        }
    }
}

/// 判定：标志闭集 → 空路径三分 → dirfd 三层 → 查找/跟随。
pub fn judge(fam: Family, t: &FdTable, dirfd: i32, path: &str, flags: u32) -> Result<u16, i32> {
    if flags & !fam.allowed() != 0 {
        return Err(EINVAL);
    }
    if path.is_empty() {
        if flags & AT_EMPTY_PATH == 0 {
            // getname_flags：空串直接 -ENOENT，早于 fd
            return Err(ENOENT);
        }
        return match t.describe(dirfd) {
            Ok((_kind, obj)) => Ok(obj),
            Err(e) => Err(e),
        };
    }
    let absolute = path.starts_with('/');
    if !absolute && dirfd != AT_FDCWD {
        match t.describe(dirfd) {
            Ok((FdKind::Dir, _obj)) => {}
            Ok((FdKind::File, _obj)) => return Err(ENOTDIR),
            Ok((FdKind::Free, _obj)) => return Err(EBADF),
            Err(e) => return Err(e),
        }
    }
    lookup_path(path, fam.follow(flags))
}

/// 样本表：fd3 目录（对象 10）、fd4 文件（对象 20）、fd5 已关闭。
pub fn sample_table() -> FdTable {
    let mut t = FdTable::new();
    let _ = t.open(FdKind::Dir, OBJ_DIR);
    let _ = t.open(FdKind::File, OBJ_FILE);
    let closed = match t.open(FdKind::Dir, 8) {
        Ok((fd, _)) => fd,
        Err(_) => -1,
    };
    let _ = t.close(closed);
    t
}

// ── 样本格（手写期望，逐格可复算） ────────────────────────────────────
/// 一格里 (族, 标志, 路径, dirfd) 四要素齐全。
#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub fam: Family,
    pub flags: u32,
    pub path: &'static str,
    pub fd: i32,
    pub want: i32,
}

pub const FD_DIR: i32 = FIRST_FD;
pub const FD_FILE: i32 = FIRST_FD + 1;
pub const FD_CLOSED: i32 = FIRST_FD + 2;
pub const FD_BEYOND: i32 = FIRST_FD + MAX_FDS as i32;
pub const FD_BELOW: i32 = AT_FDCWD - 1;

/// 36 格：22 格 fstatat + 14 格 linkat。
pub const CELLS: [Cell; 36] = [
    // ── fstatat ──
    Cell { fam: Family::StatAt, flags: 0, path: "dir", fd: FD_DIR, want: 0 },
    Cell { fam: Family::StatAt, flags: 0, path: "file", fd: FD_DIR, want: 0 },
    Cell { fam: Family::StatAt, flags: 0, path: "dangling", fd: FD_DIR, want: ENOENT },
    Cell { fam: Family::StatAt, flags: AT_SYMLINK_NOFOLLOW, path: "dangling", fd: FD_DIR, want: 0 },
    Cell { fam: Family::StatAt, flags: 0, path: "ghost", fd: FD_DIR, want: ENOENT },
    Cell { fam: Family::StatAt, flags: 0, path: "", fd: FD_DIR, want: ENOENT },
    Cell { fam: Family::StatAt, flags: AT_EMPTY_PATH, path: "", fd: FD_FILE, want: 0 },
    Cell { fam: Family::StatAt, flags: AT_EMPTY_PATH, path: "", fd: FD_DIR, want: 0 },
    Cell { fam: Family::StatAt, flags: AT_EMPTY_PATH, path: "", fd: FD_CLOSED, want: EBADF },
    Cell { fam: Family::StatAt, flags: AT_EMPTY_PATH, path: "", fd: AT_FDCWD, want: EBADF },
    Cell { fam: Family::StatAt, flags: 0, path: "file", fd: FD_FILE, want: ENOTDIR },
    Cell { fam: Family::StatAt, flags: 0, path: "ghost", fd: FD_FILE, want: ENOTDIR },
    Cell { fam: Family::StatAt, flags: 0, path: "file", fd: FD_CLOSED, want: EBADF },
    Cell { fam: Family::StatAt, flags: 0, path: "ghost", fd: FD_BELOW, want: EBADF },
    Cell { fam: Family::StatAt, flags: 0, path: "ghost", fd: FD_BEYOND, want: EBADF },
    Cell { fam: Family::StatAt, flags: AT_SYMLINK_FOLLOW, path: "file", fd: FD_DIR, want: EINVAL },
    Cell { fam: Family::StatAt, flags: AT_SYMLINK_NOFOLLOW, path: "", fd: FD_DIR, want: ENOENT },
    Cell { fam: Family::StatAt, flags: 0, path: "/ghost", fd: FD_BELOW, want: ENOENT },
    Cell { fam: Family::StatAt, flags: 0, path: "/file", fd: FD_FILE, want: 0 },
    Cell { fam: Family::StatAt, flags: 0, path: "/file/child", fd: FD_DIR, want: ENOTDIR },
    Cell { fam: Family::StatAt, flags: AT_EMPTY_PATH, path: "dir", fd: FD_FILE, want: ENOTDIR },
    Cell {
        fam: Family::StatAt,
        flags: AT_EMPTY_PATH | AT_SYMLINK_NOFOLLOW,
        path: "dangling",
        fd: FD_DIR,
        want: 0,
    },
    // ── linkat ──
    Cell { fam: Family::LinkAt, flags: 0, path: "file", fd: FD_DIR, want: 0 },
    Cell { fam: Family::LinkAt, flags: 0, path: "dangling", fd: FD_DIR, want: 0 },
    Cell { fam: Family::LinkAt, flags: AT_SYMLINK_FOLLOW, path: "dangling", fd: FD_DIR, want: ENOENT },
    Cell { fam: Family::LinkAt, flags: AT_SYMLINK_FOLLOW, path: "link2file", fd: FD_DIR, want: 0 },
    Cell { fam: Family::LinkAt, flags: AT_SYMLINK_NOFOLLOW, path: "file", fd: FD_DIR, want: EINVAL },
    Cell { fam: Family::LinkAt, flags: AT_EMPTY_PATH, path: "", fd: FD_FILE, want: 0 },
    Cell { fam: Family::LinkAt, flags: AT_EMPTY_PATH, path: "", fd: FD_CLOSED, want: EBADF },
    Cell { fam: Family::LinkAt, flags: 0, path: "", fd: FD_FILE, want: ENOENT },
    Cell { fam: Family::LinkAt, flags: 0, path: "file", fd: FD_FILE, want: ENOTDIR },
    Cell { fam: Family::LinkAt, flags: 0, path: "ghost", fd: FD_CLOSED, want: EBADF },
    Cell { fam: Family::LinkAt, flags: AT_EMPTY_PATH, path: "", fd: AT_FDCWD, want: EBADF },
    Cell { fam: Family::LinkAt, flags: 0, path: "/ghost", fd: FD_BELOW, want: ENOENT },
    Cell {
        fam: Family::LinkAt,
        flags: AT_SYMLINK_FOLLOW | AT_EMPTY_PATH,
        path: "dangling",
        fd: FD_DIR,
        want: ENOENT,
    },
    Cell { fam: Family::LinkAt, flags: AT_EMPTY_PATH, path: "dir", fd: FD_FILE, want: ENOTDIR },
];

#[derive(Clone, Copy, Debug)]
pub struct Grid {
    pub codes: [i32; 36],
    pub ok_cells: u32,
    pub enoent_cells: u32,
    pub ebadf_cells: u32,
    pub enotdir_cells: u32,
    pub einval_cells: u32,
}

pub fn run_grid() -> Grid {
    let t = sample_table();
    let mut codes = [0i32; 36];
    let mut ok_cells = 0u32;
    let mut enoent_cells = 0u32;
    let mut ebadf_cells = 0u32;
    let mut enotdir_cells = 0u32;
    let mut einval_cells = 0u32;
    let mut i = 0usize;
    while i < CELLS.len() {
        if let Some(c) = CELLS.get(i) {
            let code = match judge(c.fam, &t, c.fd, c.path, c.flags) {
                Ok(_) => 0,
                Err(e) => e,
            };
            match code {
                0 => ok_cells = ok_cells.saturating_add(1),
                ENOENT => enoent_cells = enoent_cells.saturating_add(1),
                EBADF => ebadf_cells = ebadf_cells.saturating_add(1),
                ENOTDIR => enotdir_cells = enotdir_cells.saturating_add(1),
                EINVAL => einval_cells = einval_cells.saturating_add(1),
                _ => {}
            }
            if let Some(slot) = codes.get_mut(i) {
                *slot = code;
            }
        }
        i += 1;
    }
    Grid { codes, ok_cells, enoent_cells, ebadf_cells, enotdir_cells, einval_cells }
}

// ── 注入账 ────────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FlagLedger {
    pub shots: u32,
    pub detected: u32,
    pub escaped: u32,
    pub badfd: u32,
    pub beyond: u32,
    pub closed: u32,
    pub nondir: u32,
    pub emptybad: u32,
}

impl FlagLedger {
    pub const fn zero() -> FlagLedger {
        FlagLedger {
            shots: 0,
            detected: 0,
            escaped: 0,
            badfd: 0,
            beyond: 0,
            closed: 0,
            nondir: 0,
            emptybad: 0,
        }
    }

    fn record(&mut self, want: i32, got: i32, slot: usize) {
        self.shots = self.shots.saturating_add(1);
        if got == want {
            self.detected = self.detected.saturating_add(1);
            match slot {
                0 => self.badfd = self.badfd.saturating_add(1),
                1 => self.beyond = self.beyond.saturating_add(1),
                2 => self.closed = self.closed.saturating_add(1),
                3 => self.nondir = self.nondir.saturating_add(1),
                _ => self.emptybad = self.emptybad.saturating_add(1),
            }
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.shots
            && self
                .badfd
                .saturating_add(self.beyond)
                .saturating_add(self.closed)
                .saturating_add(self.nondir)
                .saturating_add(self.emptybad)
                == self.detected
    }
}

/// 五类 × 10 轮：坏负值 / 越界 / 已关 / 文件 fd（相对非空）/ 空路径坏 fd。
pub fn run_injection() -> FlagLedger {
    let t = sample_table();
    let mut ledger = FlagLedger::zero();
    let mut round = 0u32;
    while round < 10 {
        ledger.record(EBADF, code_of(judge(Family::StatAt, &t, FD_BELOW, "file", 0)), 0);
        ledger.record(EBADF, code_of(judge(Family::StatAt, &t, FD_BEYOND, "file", 0)), 1);
        ledger.record(EBADF, code_of(judge(Family::LinkAt, &t, FD_CLOSED, "file", 0)), 2);
        ledger.record(ENOTDIR, code_of(judge(Family::StatAt, &t, FD_FILE, "ghost", 0)), 3);
        ledger.record(
            EBADF,
            code_of(judge(Family::StatAt, &t, FD_BELOW, "", AT_EMPTY_PATH)),
            4,
        );
        round = round.saturating_add(1);
    }
    ledger
}

fn code_of(r: Result<u16, i32>) -> i32 {
    match r {
        Ok(_) => 0,
        Err(e) => e,
    }
}

// ── 对账表 ────────────────────────────────────────────────────────────
/// (族, 标志, 路径, dirfd, 期望码)
pub const RECON: &[(&str, u32, &str, i32, i32)] = &[
    ("fstatat", AT_EMPTY_PATH, "", FD_FILE, 0),
    ("fstatat", AT_EMPTY_PATH, "", FD_CLOSED, EBADF),
    ("fstatat", 0, "", FD_DIR, ENOENT),
    ("fstatat", AT_SYMLINK_FOLLOW, "file", FD_DIR, EINVAL),
    ("fstatat", AT_SYMLINK_NOFOLLOW, "dangling", FD_DIR, 0),
    ("fstatat", 0, "dangling", FD_DIR, ENOENT),
    ("fstatat", 0, "file", FD_FILE, ENOTDIR),
    ("linkat", AT_SYMLINK_NOFOLLOW, "file", FD_DIR, EINVAL),
    ("linkat", 0, "dangling", FD_DIR, 0),
    ("linkat", AT_SYMLINK_FOLLOW, "dangling", FD_DIR, ENOENT),
    ("linkat", AT_EMPTY_PATH, "", FD_FILE, 0),
    ("linkat", 0, "/ghost", FD_BELOW, ENOENT),
];

/// 标志合法集对照（两族唯一差异的显性登记）。
pub const FLAG_MATRIX: &[(&str, &str, u32, i32)] = &[
    ("fstatat", "AT_SYMLINK_NOFOLLOW", AT_SYMLINK_NOFOLLOW, 0),
    ("fstatat", "AT_SYMLINK_FOLLOW", AT_SYMLINK_FOLLOW, EINVAL),
    ("fstatat", "AT_EMPTY_PATH", AT_EMPTY_PATH, 0),
    ("linkat", "AT_SYMLINK_FOLLOW", AT_SYMLINK_FOLLOW, 0),
    ("linkat", "AT_SYMLINK_NOFOLLOW", AT_SYMLINK_NOFOLLOW, EINVAL),
    ("linkat", "AT_EMPTY_PATH", AT_EMPTY_PATH, 0),
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

/// 指纹：36 格 + 跟随/不跟随对象号对照。
pub fn corpus_fingerprint() -> u32 {
    let g = run_grid();
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < g.codes.len() {
        if let Some(v) = g.codes.get(i) {
            bytes.push((*v & 0xFF) as u8);
        }
        i += 1;
    }
    let pairs = [
        ("dangling", false),
        ("dangling", true),
        ("link2file", false),
        ("link2file", true),
    ];
    let mut pi = 0usize;
    while pi < pairs.len() {
        if let Some((name, follow)) = pairs.get(pi) {
            match lookup_path(*name, *follow) {
                Ok(id) => bytes.push((id & 0xFF) as u8),
                Err(e) => bytes.push((e & 0xFF) as u8),
            }
        }
        pi += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
pub fn run_unx9230_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9230");

    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    let t = sample_table();

    // 正常流：普通路径命中
    let normal_ok = judge(Family::StatAt, &t, FD_DIR, "file", 0) == Ok(OBJ_FILE)
        && judge(Family::LinkAt, &t, FD_DIR, "file", 0) == Ok(OBJ_FILE)
        && judge(Family::StatAt, &t, AT_FDCWD, "dir", 0) == Ok(OBJ_DIR);
    s.add("正常流：两族对普通文件/目录均命中", normal_ok,
          "fstatat 与 linkat 对非链接对象语义一致，dirfd=AT_FDCWD 走 cwd");

    // AT_EMPTY_PATH 正判据：任意有效 fd
    let empty_ok = judge(Family::StatAt, &t, FD_DIR, "", AT_EMPTY_PATH) == Ok(OBJ_DIR)
        && judge(Family::StatAt, &t, FD_FILE, "", AT_EMPTY_PATH) == Ok(OBJ_FILE)
        && judge(Family::LinkAt, &t, FD_FILE, "", AT_EMPTY_PATH) == Ok(OBJ_FILE);
    s.add("AT_EMPTY_PATH 正判据：空路径 + 任意有效 fd 直接命中对象", empty_ok,
          "目录 fd 返回 10、文件 fd 返回 20；两族都支持该标志");

    // AT_EMPTY_PATH 缺标志 / 坏 fd
    let empty_neg_ok = judge(Family::StatAt, &t, FD_DIR, "", 0) == Err(ENOENT)
        && judge(Family::StatAt, &t, FD_CLOSED, "", AT_EMPTY_PATH) == Err(EBADF)
        && judge(Family::StatAt, &t, AT_FDCWD, "", AT_EMPTY_PATH) == Err(EBADF)
        && judge(Family::LinkAt, &t, FD_BELOW, "", AT_EMPTY_PATH) == Err(EBADF);
    s.add("AT_EMPTY_PATH 负判据：缺标志 ENOENT、坏 fd EBADF", empty_neg_ok,
          "空串先按 getname 回 ENOENT；置标志后 fd 才是唯一对象来源");

    // 跟随语义：statat 默认跟、linkat 默认不跟
    let follow_ok = judge(Family::StatAt, &t, FD_DIR, "dangling", 0) == Err(ENOENT)
        && judge(Family::StatAt, &t, FD_DIR, "dangling", AT_SYMLINK_NOFOLLOW)
            == Ok(OBJ_LINK_DANGLING)
        && judge(Family::LinkAt, &t, FD_DIR, "dangling", 0) == Ok(OBJ_LINK_DANGLING)
        && judge(Family::LinkAt, &t, FD_DIR, "dangling", AT_SYMLINK_FOLLOW) == Err(ENOENT)
        && judge(Family::LinkAt, &t, FD_DIR, "link2file", AT_SYMLINK_FOLLOW) == Ok(OBJ_FILE);
    s.add("跟随语义：statat 默认跟/NOFOLLOW 关、linkat 默认不跟/FOLLOW 开", follow_ok,
          "悬挂链接在跟随面 ENOENT、不跟随面成功；两面对照才锁得住");

    // 标志合法集：跨族错用 EINVAL
    let flag_ok = judge(Family::StatAt, &t, FD_DIR, "file", AT_SYMLINK_FOLLOW) == Err(EINVAL)
        && judge(Family::LinkAt, &t, FD_DIR, "file", AT_SYMLINK_NOFOLLOW) == Err(EINVAL)
        && judge(Family::StatAt, &t, FD_CLOSED, "file", AT_SYMLINK_FOLLOW) == Err(EINVAL);
    s.add("标志族：fstatat 拒 FOLLOW、linkat 拒 NOFOLLOW，均 EINVAL", flag_ok,
          "EINVAL 先于 fd 与路径：坏 fd + 坏标志也只能报 EINVAL");

    // 36 格矩阵
    let g = run_grid();
    let mut grid_ok = true;
    let mut i = 0usize;
    while i < CELLS.len() {
        if let (Some(c), Some(got)) = (CELLS.get(i), g.codes.get(i)) {
            if *got != c.want {
                grid_ok = false;
            }
        }
        i += 1;
    }
    s.add("矩阵：36 格（fstatat 22 + linkat 14）逐格复算", grid_ok,
          "每格 (族, 标志, 路径, dirfd) 四要素齐全，缺一格即判红");

    // 坏 dirfd 全 EBADF
    let bad_ok = g.ebadf_cells == 8
        && judge(Family::StatAt, &t, FD_CLOSED, "file", 0) == Err(EBADF)
        && judge(Family::StatAt, &t, FD_BELOW, "file", 0) == Err(EBADF)
        && judge(Family::StatAt, &t, FD_BEYOND, "file", 0) == Err(EBADF)
        && judge(Family::LinkAt, &t, FD_CLOSED, "file", 0) == Err(EBADF);
    s.add("坏 dirfd：已关/负值/越界 × 两族全 EBADF（8 格）", bad_ok,
          "坏 fd 面不允许被路径缺失或标志位遮住");

    // 非目录 fd 全 ENOTDIR（显性排除 AT_EMPTY_PATH）
    let nondir_ok = g.enotdir_cells == 6
        && judge(Family::StatAt, &t, FD_FILE, "file", 0) == Err(ENOTDIR)
        && judge(Family::StatAt, &t, FD_FILE, "ghost", 0) == Err(ENOTDIR)
        && judge(Family::LinkAt, &t, FD_FILE, "dir", 0) == Err(ENOTDIR)
        && judge(Family::StatAt, &t, FD_FILE, "", AT_EMPTY_PATH) == Ok(OBJ_FILE);
    s.add("非目录 fd：非空相对路径全 ENOTDIR（6 格），空路径面例外", nondir_ok,
          "AT_EMPTY_PATH 下文件 fd 合法，这一格必须显性排除，不许被 ENOTDIR 吞掉");

    // 判序：fd 面先于查找面
    let order_ok = judge(Family::StatAt, &t, FD_FILE, "ghost", 0) == Err(ENOTDIR)
        && judge(Family::StatAt, &t, FD_CLOSED, "ghost", 0) == Err(EBADF)
        && judge(Family::StatAt, &t, FD_DIR, "ghost", 0) == Err(ENOENT)
        && judge(Family::StatAt, &t, FD_BELOW, "/ghost", 0) == Err(ENOENT);
    s.add("判定次序：fd 面先于查找，绝对路径跳过 fd 面", order_ok,
          "同一缺失目标按 dirfd 三层给三种码；绝对路径只留 ENOENT");

    // 注入必抓
    let ledger = run_injection();
    let inject_ok = ledger.shots == 50
        && ledger.detected == 50
        && ledger.escaped == 0
        && ledger.badfd == 10
        && ledger.beyond == 10
        && ledger.closed == 10
        && ledger.nondir == 10
        && ledger.emptybad == 10;
    s.add("注入必抓：五类 × 10 轮全检出、逃逸 0", inject_ok,
          "50 次注入（坏负值/越界/已关/文件 fd/空路径坏 fd 各 10）必须全抓");
    s.add("账目守恒：注入=检出+逃逸且五类之和=检出", ledger.conserved(),
          "分格计数与总数不等说明有注入未归类");

    // 状态机：代际
    let mut ts = FdTable::new();
    let opened = ts.open(FdKind::Dir, OBJ_DIR);
    let state_ok = match opened {
        Ok((fd, gen1)) => {
            let g2 = ts.close(fd).unwrap_or(0);
            let again = ts.close(fd);
            match ts.open(FdKind::File, OBJ_FILE) {
                Ok((fd2, gen3)) => {
                    fd2 == fd
                        && g2 == gen1.wrapping_add(1)
                        && gen3 == g2
                        && again == Err(EBADF)
                        && ts.describe_gen(fd, gen1) == Err(EBADF)
                        && ts.describe_gen(fd, gen3) == Ok(FdKind::File)
                }
                Err(_) => false,
            }
        }
        Err(_) => false,
    };
    s.add("状态机：close 两次 EBADF、复用槽换代际、旧代际失效", state_ok,
          "代际是「已关 fd」与「复用 fd」的分水岭，必须双向验");

    // 容量
    let mut tc = FdTable::new();
    let mut k = 0usize;
    while k < MAX_FDS {
        let _ = tc.open(FdKind::Dir, OBJ_DIR);
        k += 1;
    }
    let cap_ok = tc.open_count() == MAX_FDS
        && tc.opens() == MAX_FDS as u32
        && tc.open(FdKind::Dir, OBJ_DIR) == Err(EMFILE)
        && tc.describe(FIRST_FD + MAX_FDS as i32) == Err(EBADF);
    s.add("容量：6 槽满回 EMFILE、上界+1 仍 EBADF", cap_ok,
          "分配账只记成功；号段外一律 EBADF");

    // 对账表
    let mut recon_ok = true;
    let mut ri = 0usize;
    while ri < RECON.len() {
        if let Some(row) = RECON.get(ri) {
            let fam = if row.0 == "fstatat" { Family::StatAt } else { Family::LinkAt };
            let got = code_of(judge(fam, &t, row.3, row.2, row.1));
            if got != row.4 {
                recon_ok = false;
            }
        }
        ri += 1;
    }
    s.add("对账：12 行 at 族对照表逐行复算相等", recon_ok,
          "表与判定器同源；两族差异行逐条可复算");

    // 标志矩阵
    let mut fm_ok = true;
    let mut fi = 0usize;
    while fi < FLAG_MATRIX.len() {
        if let Some(row) = FLAG_MATRIX.get(fi) {
            let fam = if row.0 == "fstatat" { Family::StatAt } else { Family::LinkAt };
            let got = code_of(judge(fam, &t, FD_DIR, "file", row.2));
            if got != row.3 {
                fm_ok = false;
            }
        }
        fi += 1;
    }
    s.add("对账：6 行标志合法集（两族唯一差异）逐行复算", fm_ok,
          "NOFOLLOW/FOLLOW 的族归属是硬语义，表错一格语义就反了");

    // 双跑一致
    let fp1 = corpus_fingerprint();
    let fp2 = corpus_fingerprint();
    s.add("双跑一致：语料指纹相等", fp1 == fp2,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 文案闭集
    let text_ok = FlagCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带非空中文建议", text_ok,
          "14 个码都必须能取到可操作说明");

    // 码唯一
    let mut uniq = true;
    let mut a = 0usize;
    while a < FlagCode::ALL.len() {
        let mut b = a + 1;
        while b < FlagCode::ALL.len() {
            if let (Some(x), Some(y)) = (FlagCode::ALL.get(a), FlagCode::ALL.get(b)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            b += 1;
        }
        a += 1;
    }
    s.add("诊断码：14 个码值两两唯一", uniq,
          "段内闭集码重复会让判据无法定位");

    // 账目单调
    let mut mono = ledger;
    let before = mono.shots;
    mono.record(EBADF, EBADF, 0);
    s.add("账目单调：再注入只增不减且守恒", mono.shots == before.saturating_add(1) && mono.conserved(),
          "累计计数禁止回退");

    // 分布对账
    let dist_ok = g.ok_cells == 11
        && g.enoent_cells == 9
        && g.ebadf_cells == 8
        && g.enotdir_cells == 6
        && g.einval_cells == 2;
    s.add("分布对账：36 格 = 11 成功 + 9 ENOENT + 8 EBADF + 6 ENOTDIR + 2 EINVAL", dist_ok,
          "五类计数之和必须等于 36，缺一类说明有格未执行");

    let grid_text = format!(
        "36 格 ok={} enoent={} ebadf={} enotdir={} einval={}",
        g.ok_cells, g.enoent_cells, g.ebadf_cells, g.enotdir_cells, g.einval_cells
    );
    s.add_dyn("矩阵快照", grid_ok, &grid_text);
    let ledger_text = format!(
        "注入 {} 检出 {} 逃逸 {} | 坏 {} 越界 {} 关 {} 文件 {} 空坏 {}",
        ledger.shots, ledger.detected, ledger.escaped, ledger.badfd, ledger.beyond,
        ledger.closed, ledger.nondir, ledger.emptybad
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

    #[test]
    fn normal_paths_hit() {
        let t = sample_table();
        assert_eq!(judge(Family::StatAt, &t, FD_DIR, "file", 0), Ok(OBJ_FILE));
        assert_eq!(judge(Family::StatAt, &t, FD_DIR, "dir", 0), Ok(OBJ_DIR));
        assert_eq!(judge(Family::LinkAt, &t, AT_FDCWD, "file", 0), Ok(OBJ_FILE));
    }

    #[test]
    fn empty_path_with_flag_any_fd() {
        let t = sample_table();
        assert_eq!(judge(Family::StatAt, &t, FD_FILE, "", AT_EMPTY_PATH), Ok(OBJ_FILE));
        assert_eq!(judge(Family::LinkAt, &t, FD_FILE, "", AT_EMPTY_PATH), Ok(OBJ_FILE));
        assert_eq!(judge(Family::StatAt, &t, FD_DIR, "", AT_EMPTY_PATH), Ok(OBJ_DIR));
    }

    #[test]
    fn empty_path_without_flag_is_enoent_first() {
        let t = sample_table();
        assert_eq!(judge(Family::StatAt, &t, FD_CLOSED, "", 0), Err(ENOENT));
        assert_eq!(judge(Family::StatAt, &t, FD_CLOSED, "", AT_EMPTY_PATH), Err(EBADF));
    }

    #[test]
    fn follow_semantics_per_family() {
        let t = sample_table();
        assert_eq!(judge(Family::StatAt, &t, FD_DIR, "dangling", 0), Err(ENOENT));
        assert_eq!(
            judge(Family::StatAt, &t, FD_DIR, "dangling", AT_SYMLINK_NOFOLLOW),
            Ok(OBJ_LINK_DANGLING)
        );
        assert_eq!(judge(Family::LinkAt, &t, FD_DIR, "dangling", 0), Ok(OBJ_LINK_DANGLING));
        assert_eq!(
            judge(Family::LinkAt, &t, FD_DIR, "dangling", AT_SYMLINK_FOLLOW),
            Err(ENOENT)
        );
    }

    #[test]
    fn flag_families_reject_cross_use() {
        let t = sample_table();
        assert_eq!(
            judge(Family::StatAt, &t, FD_DIR, "file", AT_SYMLINK_FOLLOW),
            Err(EINVAL)
        );
        assert_eq!(
            judge(Family::LinkAt, &t, FD_DIR, "file", AT_SYMLINK_NOFOLLOW),
            Err(EINVAL)
        );
    }

    #[test]
    fn non_dir_fd_enotdir_unless_empty_path() {
        let t = sample_table();
        assert_eq!(judge(Family::StatAt, &t, FD_FILE, "file", 0), Err(ENOTDIR));
        assert_eq!(judge(Family::StatAt, &t, FD_FILE, "ghost", 0), Err(ENOTDIR));
        assert_eq!(judge(Family::StatAt, &t, FD_FILE, "", AT_EMPTY_PATH), Ok(OBJ_FILE));
    }

    #[test]
    fn grid_all_cells() {
        let g = run_grid();
        for (i, c) in CELLS.iter().enumerate() {
            let got = g.codes.get(i).copied().unwrap_or(-1);
            assert_eq!(got, c.want, "cell={} fam={:?}", i, c.fam);
        }
        assert_eq!(g.ok_cells, 11);
        assert_eq!(g.enoent_cells, 9);
        assert_eq!(g.ebadf_cells, 8);
        assert_eq!(g.enotdir_cells, 6);
        assert_eq!(g.einval_cells, 2);
    }

    #[test]
    fn fd_generation_and_capacity() {
        let mut t = FdTable::new();
        let (fd, gen1) = t.open(FdKind::Dir, OBJ_DIR).unwrap_or((-1, 0));
        let g2 = t.close(fd).unwrap_or(0);
        assert_eq!(g2, gen1 + 1);
        assert_eq!(t.close(fd), Err(EBADF));
        let (fd2, gen3) = t.open(FdKind::File, OBJ_FILE).unwrap_or((-1, 0));
        assert_eq!(fd2, fd);
        assert_eq!(gen3, g2);
        assert_eq!(t.describe_gen(fd, gen1), Err(EBADF));
        assert_eq!(t.describe_gen(fd, gen3), Ok(FdKind::File));

        let mut cap = FdTable::new();
        for _ in 0..MAX_FDS {
            assert!(cap.open(FdKind::Dir, OBJ_DIR).is_ok());
        }
        assert_eq!(cap.open(FdKind::Dir, OBJ_DIR), Err(EMFILE));
        assert_eq!(cap.open_count(), MAX_FDS);
    }

    #[test]
    fn injection_ledger() {
        let l = run_injection();
        assert_eq!(l.shots, 50);
        assert_eq!(l.detected, 50);
        assert_eq!(l.escaped, 0);
        assert!(l.conserved());
    }

    #[test]
    fn recon_rows() {
        let t = sample_table();
        for row in RECON.iter() {
            let fam = if row.0 == "fstatat" { Family::StatAt } else { Family::LinkAt };
            assert_eq!(code_of(judge(fam, &t, row.3, row.2, row.1)), row.4, "{:?}", row);
        }
        for row in FLAG_MATRIX.iter() {
            let fam = if row.0 == "fstatat" { Family::StatAt } else { Family::LinkAt };
            assert_eq!(code_of(judge(fam, &t, FD_DIR, "file", row.2)), row.3, "{:?}", row);
        }
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9230_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(s.len() >= 12);
        assert!(!s.truncated());
    }
}
