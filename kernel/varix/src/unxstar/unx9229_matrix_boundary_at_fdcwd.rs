//! UNX-F9229 相对路径与 AT_FDCWD 边界矩阵 · 0x7A6C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9229 逐字）：
//!   ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9229-J1 AT_FDCWD 正判据判据过，
//!   坏 dirfd 全 EBADF 格过，绝对路径忽略 dirfd 语义格过；③ 完成时 API/软件
//!   写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① dirfd 有三层语义：`AT_FDCWD`（-100，取 cwd，不是 fd 号）、合法目录 fd、
//!      以及一切非法号段（负值/越界/已关）。三层必须分格，不许把 AT_FDCWD
//!      当成「-100 号 fd」去查表。
//!   ② 绝对路径是**短路**语义：`path_init` 见到首字符 `/` 就把 nd->path 钉到
//!      root，dirfd 全不参与。因此哪怕 dirfd 是 -101、越界或已关，绝对路径
//!      也照样命中——这是本模块最容易写反的一格（很多人先把 dirfd 验一遍）。
//!   ③ 相对路径才走 fd 校验：负数/越界/空槽/已关 → EBADF；合法但是普通文件
//!      → ENOTDIR；合法目录 → 以该目录为基。
//!   ④ 判定次序：dirfd 类型（ENOTDIR）先于目标查找（ENOENT）——`openat(filefd,
//!      "ghost")` 必须回 ENOTDIR，不许因为目标不存在而回 ENOENT。
//!   ⑤ AT_FDCWD 边界值：-101 是普通坏 fd（EBADF）；i32::MIN 同理；fd 号上界
//!      是 FIRST_FD + MAX_DIRFDS，上界+1 亦 EBADF。AT_FDCWD 永不占槽。
//!   ⑥ 状态机：fd 槽 Free → Open →（close 推进代际）→ 复用；close 两次 EBADF，
//!      旧代际在复用后不可用。容量满回 EMFILE。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x7A6C0;

/// 动态 dirfd 槽数（fd 号 3..=10）。
pub const MAX_DIRFDS: usize = 8;
/// 首个动态 fd 号（0/1/2 为标准流，本域不建模）。
pub const FIRST_FD: i32 = 3;
/// AT_FDCWD 冻结值。
pub const AT_FDCWD: i32 = -100;
/// cwd 的目录号（本域用抽象目录号，不建 inode 表）。
pub const CWD_DIR: u16 = 9;

pub const ENOENT: i32 = 2;
pub const EBADF: i32 = 9;
pub const ENOTDIR: i32 = 20;
pub const EINVAL: i32 = 22;
pub const EMFILE: i32 = 24;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AtCode {
    AtFdcwdTakenAsFd,
    BadDirfd,
    NonDirDirfd,
    AboveFdLimit,
    ClosedDirfd,
    FdTableFull,
    MissingTarget,
    AbsoluteIgnoredDirfd,
    StaleGeneration,
    LedgerRegress,
    NotReproducible,
    SegmentViolation,
}

impl AtCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            AtCode::AtFdcwdTakenAsFd => "AT_FDCWD 被当成 fd 查表：-100 是常量不是号段，\
                必须先识别再取 cwd；否则会把 cwd 语义误判成 EBADF",
            AtCode::BadDirfd => "坏 dirfd：负值（-100 之外）/空槽一律 EBADF；请先核对\
                fd 来源，不要与 ENOENT 混码",
            AtCode::NonDirDirfd => "dirfd 非目录：合法 fd 但是普通文件，回 ENOTDIR；\
                相对路径必须有目录基",
            AtCode::AboveFdLimit => "dirfd 越上界：号大于 FIRST_FD+MAX_DIRFDS 回 EBADF；\
                请按 3..=10 分配",
            AtCode::ClosedDirfd => "已关闭 dirfd：槽位已回收，回 EBADF；复用会得到新\
                代际，旧号不可继续用",
            AtCode::FdTableFull => "fd 表满：open 无法再分配槽位回 EMFILE；请先 close\
                回收，旧槽账目不得丢",
            AtCode::MissingTarget => "目标缺失：dirfd 合法、目录基确定，但目标名不存在，\
                回 ENOENT",
            AtCode::AbsoluteIgnoredDirfd => "绝对路径忽略 dirfd：首字符 / 直接把基钉到\
                root，dirfd 不参与校验；这是 Linux 语义，不许提前验 fd",
            AtCode::StaleGeneration => "陈旧代际：槽位已复用，旧代际句柄不可用；\
                请用新代际重建",
            AtCode::LedgerRegress => "注入账倒退：计数只增不减；请检查回滚路径",
            AtCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏\
                状态或非确定遍历序",
            AtCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    pub const ALL: [AtCode; 12] = [
        AtCode::AtFdcwdTakenAsFd,
        AtCode::BadDirfd,
        AtCode::NonDirDirfd,
        AtCode::AboveFdLimit,
        AtCode::ClosedDirfd,
        AtCode::FdTableFull,
        AtCode::MissingTarget,
        AtCode::AbsoluteIgnoredDirfd,
        AtCode::StaleGeneration,
        AtCode::LedgerRegress,
        AtCode::NotReproducible,
        AtCode::SegmentViolation,
    ];
}

// ── dirfd 表 ──────────────────────────────────────────────────────────
/// 槽位类别。Free 同时表示「从未分配」与「已关闭」——两者对外都是 EBADF，
/// 区别只在内部代际：已关闭的槽在 close 时已推进代际。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotKind {
    Free,
    Dir,
    File,
}

#[derive(Clone, Copy, Debug)]
pub struct Slot {
    kind: SlotKind,
    generation: u16,
    dir: u16,
}

impl Slot {
    const fn blank() -> Slot {
        Slot { kind: SlotKind::Free, generation: 1, dir: 0 }
    }
}

/// dirfd 表（号段 3..3+MAX_DIRFDS）。
#[derive(Clone, Copy, Debug)]
pub struct FdTable {
    slots: [Slot; MAX_DIRFDS],
    opens: u32,
    closes: u32,
}

impl FdTable {
    pub const fn new() -> FdTable {
        FdTable { slots: [Slot::blank(); MAX_DIRFDS], opens: 0, closes: 0 }
    }

    fn idx(fd: i32) -> Option<usize> {
        if fd < FIRST_FD {
            return None;
        }
        let i = (fd - FIRST_FD) as usize;
        if i >= MAX_DIRFDS {
            return None;
        }
        Some(i)
    }

    /// 分配：优先复用 Free 槽（含已关槽，代际已由 close 推进）。
    pub fn open(&mut self, kind: SlotKind, dir: u16) -> Result<(i32, u16), i32> {
        if kind == SlotKind::Free {
            return Err(EINVAL);
        }
        let mut i = 0usize;
        while i < MAX_DIRFDS {
            let take = match self.slots.get(i) {
                Some(s) => s.kind == SlotKind::Free,
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
                        s.dir = dir;
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

    /// 关闭：只接受 Open 槽，成功时代际 +1。
    pub fn close(&mut self, fd: i32) -> Result<u16, i32> {
        let i = match Self::idx(fd) {
            Some(i) => i,
            None => return Err(EBADF),
        };
        let open = match self.slots.get(i) {
            Some(s) => s.kind != SlotKind::Free,
            None => false,
        };
        if !open {
            return Err(EBADF);
        }
        let gen = match self.slots.get_mut(i) {
            Some(s) => {
                s.kind = SlotKind::Free;
                s.dir = 0;
                s.generation = s.generation.wrapping_add(1);
                s.generation
            }
            None => return Err(EBADF),
        };
        self.closes = self.closes.saturating_add(1);
        Ok(gen)
    }

    /// 判 fd：返回 (类别, 代际, 目录号)。
    pub fn describe(&self, fd: i32) -> Result<(SlotKind, u16, u16), i32> {
        let i = match Self::idx(fd) {
            Some(i) => i,
            None => return Err(EBADF),
        };
        match self.slots.get(i) {
            Some(s) => {
                if s.kind == SlotKind::Free {
                    Err(EBADF)
                } else {
                    Ok((s.kind, s.generation, s.dir))
                }
            }
            None => Err(EBADF),
        }
    }

    /// 代际校验：号与代际都要对。
    pub fn describe_gen(&self, fd: i32, gen: u16) -> Result<SlotKind, i32> {
        let (kind, g, _dir) = self.describe(fd)?;
        if g != gen {
            return Err(EBADF);
        }
        Ok(kind)
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
        while i < MAX_DIRFDS {
            if let Some(s) = self.slots.get(i) {
                if s.kind != SlotKind::Free {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

// ── 路径形态与判定 ────────────────────────────────────────────────────
/// 路径形态。空路径的 AT_EMPTY_PATH 面归 F9230，本域只判非空路径。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    Absolute,
    Relative,
}

/// 解析结果：基目录号 + 目标号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Resolution {
    pub dir: u16,
    pub ino: u16,
}

/// at 族判定：绝对路径短路 root；相对路径按 dirfd 三层语义取基。
pub fn judge_at(
    t: &FdTable,
    cwd: u16,
    dirfd: i32,
    form: Form,
    target_exists: bool,
    target_ino: u16,
) -> Result<Resolution, i32> {
    if form == Form::Absolute {
        // 绝对路径：dirfd 完全不参与（path_init 钉 root）。
        return if target_exists {
            Ok(Resolution { dir: 0, ino: target_ino })
        } else {
            Err(ENOENT)
        };
    }
    let base_dir = if dirfd == AT_FDCWD {
        cwd
    } else {
        match t.describe(dirfd) {
            Ok((SlotKind::Dir, _gen, dir)) => {
                if dir == 0 {
                    return Err(EBADF);
                }
                dir
            }
            Ok((SlotKind::File, _gen, _dir)) => return Err(ENOTDIR),
            Ok((SlotKind::Free, _gen, _dir)) => return Err(EBADF),
            Err(e) => return Err(e),
        }
    };
    if target_exists {
        Ok(Resolution { dir: base_dir, ino: target_ino })
    } else {
        Err(ENOENT)
    }
}

/// 只取码（0 = 成功）的薄包装，矩阵与对账共用。
pub fn judge_code(
    t: &FdTable,
    cwd: u16,
    dirfd: i32,
    form: Form,
    target_exists: bool,
    target_ino: u16,
) -> i32 {
    match judge_at(t, cwd, dirfd, form, target_exists, target_ino) {
        Ok(_) => 0,
        Err(e) => e,
    }
}

// ── 矩阵 ──────────────────────────────────────────────────────────────
/// dirfd 六类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FdClass {
    AtFdcwd,
    ValidDir,
    ValidFile,
    BelowFdcwd,
    BeyondLimit,
    Closed,
}

impl FdClass {
    pub const ALL: [FdClass; 6] = [
        FdClass::AtFdcwd,
        FdClass::ValidDir,
        FdClass::ValidFile,
        FdClass::BelowFdcwd,
        FdClass::BeyondLimit,
        FdClass::Closed,
    ];

    /// 该类在本域样本表里的 dirfd 值。
    pub fn dirfd(self) -> i32 {
        match self {
            FdClass::AtFdcwd => AT_FDCWD,
            FdClass::ValidDir => FIRST_FD,
            FdClass::ValidFile => FIRST_FD + 1,
            FdClass::BelowFdcwd => AT_FDCWD - 1,
            FdClass::BeyondLimit => FIRST_FD + MAX_DIRFDS as i32,
            FdClass::Closed => FIRST_FD + 2,
        }
    }
}

/// 样本表：fd3 目录（dir=CWD_DIR）、fd4 文件、fd5 已关闭。
pub fn sample_table() -> FdTable {
    let mut t = FdTable::new();
    let _ = t.open(SlotKind::Dir, CWD_DIR);
    let _ = t.open(SlotKind::File, 0);
    let closed = match t.open(SlotKind::Dir, 8) {
        Ok((fd, _)) => fd,
        Err(_) => -1,
    };
    let _ = t.close(closed);
    t
}

/// 12 行 × 2 列（存在/缺失）：行 = 2 形态 × 6 类。
pub const EXPECTED: [[i32; 2]; 12] = [
    // 绝对路径：六类 dirfd 全部忽略
    [0, ENOENT],
    [0, ENOENT],
    [0, ENOENT],
    [0, ENOENT],
    [0, ENOENT],
    [0, ENOENT],
    // 相对路径：cwd/目录 fd 正常，文件 fd ENOTDIR，三类坏 fd EBADF
    [0, ENOENT],
    [0, ENOENT],
    [ENOTDIR, ENOTDIR],
    [EBADF, EBADF],
    [EBADF, EBADF],
    [EBADF, EBADF],
];

#[derive(Clone, Copy, Debug)]
pub struct Grid {
    pub codes: [[i32; 2]; 12],
    pub ok_cells: u32,
    pub enoent_cells: u32,
    pub notdir_cells: u32,
    pub ebadf_cells: u32,
}

pub fn run_grid() -> Grid {
    let mut codes = [[0i32; 2]; 12];
    let mut ok_cells = 0u32;
    let mut enoent_cells = 0u32;
    let mut notdir_cells = 0u32;
    let mut ebadf_cells = 0u32;
    let t = sample_table();
    let mut ci = 0usize;
    while ci < FdClass::ALL.len() {
        if let Some(class) = FdClass::ALL.get(ci).copied() {
            let mut exists_i = 0usize;
            while exists_i < 2 {
                let exists = exists_i == 0;
                // 绝对路径行
                let abs_code = judge_code(&t, CWD_DIR, class.dirfd(), Form::Absolute, exists, 7);
                // 相对路径行
                let rel_code = judge_code(&t, CWD_DIR, class.dirfd(), Form::Relative, exists, 7);
                match abs_code {
                    0 => ok_cells = ok_cells.saturating_add(1),
                    ENOENT => enoent_cells = enoent_cells.saturating_add(1),
                    ENOTDIR => notdir_cells = notdir_cells.saturating_add(1),
                    EBADF => ebadf_cells = ebadf_cells.saturating_add(1),
                    _ => {}
                }
                match rel_code {
                    0 => ok_cells = ok_cells.saturating_add(1),
                    ENOENT => enoent_cells = enoent_cells.saturating_add(1),
                    ENOTDIR => notdir_cells = notdir_cells.saturating_add(1),
                    EBADF => ebadf_cells = ebadf_cells.saturating_add(1),
                    _ => {}
                }
                if let Some(cell) = codes.get_mut(ci) {
                    if let Some(slot) = cell.get_mut(exists_i) {
                        *slot = abs_code;
                    }
                }
                if let Some(cell) = codes.get_mut(ci + FdClass::ALL.len()) {
                    if let Some(slot) = cell.get_mut(exists_i) {
                        *slot = rel_code;
                    }
                }
                exists_i += 1;
            }
        }
        ci += 1;
    }
    Grid { codes, ok_cells, enoent_cells, notdir_cells, ebadf_cells }
}

// ── 注入账 ────────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AtLedger {
    pub shots: u32,
    pub detected: u32,
    pub escaped: u32,
    pub bad: u32,
    pub beyond: u32,
    pub closed: u32,
    pub nondir: u32,
}

impl AtLedger {
    pub const fn zero() -> AtLedger {
        AtLedger { shots: 0, detected: 0, escaped: 0, bad: 0, beyond: 0, closed: 0, nondir: 0 }
    }

    fn record(&mut self, want: i32, got: i32, slot: usize) {
        self.shots = self.shots.saturating_add(1);
        if got == want {
            self.detected = self.detected.saturating_add(1);
            match slot {
                0 => self.bad = self.bad.saturating_add(1),
                1 => self.beyond = self.beyond.saturating_add(1),
                2 => self.closed = self.closed.saturating_add(1),
                _ => self.nondir = self.nondir.saturating_add(1),
            }
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.shots
            && self.bad
                .saturating_add(self.beyond)
                .saturating_add(self.closed)
                .saturating_add(self.nondir)
                == self.detected
    }
}

/// 四类坏 dirfd × 10 轮（相对路径、目标缺失，确保错在 fd 侧）。
pub fn run_injection() -> AtLedger {
    let t = sample_table();
    let mut ledger = AtLedger::zero();
    let mut round = 0u32;
    while round < 10 {
        ledger.record(EBADF, judge_code(&t, CWD_DIR, AT_FDCWD - 1, Form::Relative, false, 0), 0);
        ledger.record(EBADF, judge_code(&t, CWD_DIR, i32::MIN, Form::Relative, true, 0), 0);
        ledger.record(EBADF, judge_code(&t, CWD_DIR, FIRST_FD + MAX_DIRFDS as i32, Form::Relative, false, 0), 1);
        ledger.record(EBADF, judge_code(&t, CWD_DIR, FIRST_FD + 2, Form::Relative, true, 0), 2);
        ledger.record(ENOTDIR, judge_code(&t, CWD_DIR, FIRST_FD + 1, Form::Relative, false, 0), 3);
        round = round.saturating_add(1);
    }
    ledger
}

// ── 对账表 ────────────────────────────────────────────────────────────
/// (dirfd 描述, 形态, 目标, 期望码) —— 与 Linux openat 行为逐行对齐。
pub const RECON: &[(&str, &str, &str, i32)] = &[
    ("AT_FDCWD", "relative", "存在", 0),
    ("AT_FDCWD", "relative", "缺失", ENOENT),
    ("AT_FDCWD", "absolute", "存在", 0),
    ("-101", "relative", "缺失", EBADF),
    ("-101", "absolute", "缺失", ENOENT),
    ("11", "relative", "存在", EBADF),
    ("filefd", "relative", "缺失", ENOTDIR),
    ("filefd", "absolute", "存在", 0),
    ("closed", "relative", "存在", EBADF),
    ("INT_MIN", "absolute", "存在", 0),
];

/// 边界值表：dirfd 的三个边缘号。
pub const BOUNDARIES: &[(i32, &str, i32)] = &[
    (AT_FDCWD, "AT_FDCWD 本体", 0),
    (AT_FDCWD - 1, "cwd 下邻", EBADF),
    (i32::MIN, "i32 下界", EBADF),
    (FIRST_FD + MAX_DIRFDS as i32 - 1, "号段上界号（本样本未开槽）", EBADF),
    (FIRST_FD + MAX_DIRFDS as i32, "号段上界+1", EBADF),
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

/// 指纹：24 格 + 基目录号（cwd/fd/root 三态）。
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
    let t = sample_table();
    let bases = [
        judge_at(&t, CWD_DIR, AT_FDCWD, Form::Relative, true, 7),
        judge_at(&t, CWD_DIR, FIRST_FD, Form::Relative, true, 7),
        judge_at(&t, CWD_DIR, AT_FDCWD, Form::Absolute, true, 7),
    ];
    let mut bi = 0usize;
    while bi < bases.len() {
        if let Some(r) = bases.get(bi) {
            match r {
                Ok(res) => {
                    bytes.push((res.dir & 0xFF) as u8);
                    bytes.push((res.ino & 0xFF) as u8);
                }
                Err(_) => bytes.push(0xFF),
            }
        }
        bi += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
pub fn run_unx9229_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9229");

    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 正常流：AT_FDCWD 正判据
    let t = sample_table();
    let cwd_ok = judge_at(&t, CWD_DIR, AT_FDCWD, Form::Relative, true, 7)
        == Ok(Resolution { dir: CWD_DIR, ino: 7 })
        && judge_code(&t, CWD_DIR, AT_FDCWD, Form::Relative, false, 7) == ENOENT;
    s.add("AT_FDCWD 正判据：相对路径以 cwd 为基", cwd_ok,
          "命中返回 cwd 目录号 9；缺失回 ENOENT，不得回 EBADF");

    // 正常流：合法目录 fd 为基
    let dirfd_ok = judge_at(&t, CWD_DIR, FIRST_FD, Form::Relative, true, 7)
        == Ok(Resolution { dir: CWD_DIR, ino: 7 })
        && judge_code(&t, CWD_DIR, FIRST_FD, Form::Relative, false, 7) == ENOENT;
    s.add("正常流：合法目录 fd 作基、缺失回 ENOENT", dirfd_ok,
          "目录 fd 携带自己的目录号 9，与 cwd 等价但不能混同来源");

    // 绝对路径忽略 dirfd：六类全过
    let mut abs_ok = true;
    let mut ai = 0usize;
    while ai < FdClass::ALL.len() {
        if let Some(c) = FdClass::ALL.get(ai).copied() {
            if judge_code(&t, CWD_DIR, c.dirfd(), Form::Absolute, true, 7) != 0 {
                abs_ok = false;
            }
            if judge_code(&t, CWD_DIR, c.dirfd(), Form::Absolute, false, 7) != ENOENT {
                abs_ok = false;
            }
        }
        ai += 1;
    }
    s.add("绝对路径忽略 dirfd：六类 dirfd（含坏值）全不参与", abs_ok,
          "首字符 / 直接钉 root；提前验 fd 是本域最典型的语义写反");

    // 24 格矩阵
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
    s.add("矩阵：2 形态 × 6 类 dirfd × 2 存在性 = 24 格逐格复算", grid_ok,
          "整表复算；绝对路径段 12 格与相对路径段 12 格必须同时成立");

    // 坏 dirfd 全 EBADF
    let bad_ok = judge_code(&t, CWD_DIR, AT_FDCWD - 1, Form::Relative, true, 7) == EBADF
        && judge_code(&t, CWD_DIR, AT_FDCWD - 1, Form::Relative, false, 7) == EBADF
        && judge_code(&t, CWD_DIR, i32::MIN, Form::Relative, true, 7) == EBADF
        && judge_code(&t, CWD_DIR, FIRST_FD + MAX_DIRFDS as i32, Form::Relative, false, 7) == EBADF
        && judge_code(&t, CWD_DIR, FIRST_FD + 2, Form::Relative, true, 7) == EBADF
        && g.ebadf_cells == 6;
    s.add("坏 dirfd：负数/越界/已关 × 存在性全 EBADF（6 格）", bad_ok,
          "三类坏 fd 各两格共 6 格；AT_FDCWD 的 -100 不在坏值里");

    // 非目录 fd 全 ENOTDIR
    let nondir_ok = judge_code(&t, CWD_DIR, FIRST_FD + 1, Form::Relative, true, 7) == ENOTDIR
        && judge_code(&t, CWD_DIR, FIRST_FD + 1, Form::Relative, false, 7) == ENOTDIR
        && g.notdir_cells == 2;
    s.add("非目录 dirfd：合法文件 fd × 存在性全 ENOTDIR（2 格）", nondir_ok,
          "目标存在的两格也必须是 ENOTDIR，不能用命中与否掩盖 fd 类型错误");

    // 判定次序：ENOTDIR 先于 ENOENT
    let order_ok = judge_code(&t, CWD_DIR, FIRST_FD + 1, Form::Relative, false, 7) == ENOTDIR
        && judge_code(&t, CWD_DIR, AT_FDCWD - 1, Form::Relative, false, 7) == EBADF
        && judge_code(&t, CWD_DIR, AT_FDCWD - 1, Form::Absolute, false, 7) == ENOENT;
    s.add("判定次序：fd 类型先于查找，绝对路径跳过 fd 面", order_ok,
          "同一格「fd 坏 + 目标缺失」只能按形态取码：相对 EBADF、绝对 ENOENT");

    // AT_FDCWD 边界值
    let mut bounds_ok = true;
    let mut bi = 0usize;
    while bi < BOUNDARIES.len() {
        if let Some(row) = BOUNDARIES.get(bi) {
            let got = judge_code(&t, CWD_DIR, row.0, Form::Relative, true, 7);
            if got != row.2 {
                bounds_ok = false;
            }
        }
        bi += 1;
    }
    s.add("边界：AT_FDCWD/-101/i32::MIN/上界/上界+1 逐值正确", bounds_ok,
          "五值缺一不可；上界号本身合法但本样本未开槽（EBADF），开槽合法性在容量格另验");

    // AT_FDCWD 不占槽
    let mut tc = FdTable::new();
    let mut opened = 0usize;
    let mut k = 0usize;
    while k < MAX_DIRFDS {
        if tc.open(SlotKind::Dir, CWD_DIR).is_ok() {
            opened += 1;
        }
        k += 1;
    }
    let atfd_slot_ok = opened == MAX_DIRFDS
        && tc.open_count() == MAX_DIRFDS
        && tc.describe(AT_FDCWD) == Err(EBADF)
        && tc.open(SlotKind::Dir, CWD_DIR) == Err(EMFILE);
    s.add("AT_FDCWD 不占槽：表满 EMFILE 且 -100 永不进号段", atfd_slot_ok,
          "AT_FDCWD 是常量不是 fd；把它塞进槽表会让号段语义整段失真");

    // 状态机：close 两次 + 复用代际
    let mut ts = FdTable::new();
    let first = ts.open(SlotKind::Dir, CWD_DIR);
    let state_ok = match first {
        Ok((fd, gen1)) => {
            let closed_gen = ts.close(fd).unwrap_or(0);
            let again = ts.close(fd);
            let reopened = ts.open(SlotKind::Dir, CWD_DIR);
            match reopened {
                Ok((fd2, gen2)) => {
                    fd2 == fd
                        && gen2 == closed_gen
                        && gen2 == gen1.wrapping_add(1)
                        && again == Err(EBADF)
                        && ts.describe_gen(fd, gen1) == Err(EBADF)
                        && ts.describe_gen(fd, gen2) == Ok(SlotKind::Dir)
                }
                Err(_) => false,
            }
        }
        Err(_) => false,
    };
    s.add("状态机：close 两次 EBADF、复用槽带新代际、旧代际失效", state_ok,
          "代际只在 close 时推进；复用后旧句柄必须彻底失效");

    // 容量账
    let mut tcap = FdTable::new();
    let mut c = 0u32;
    while c < MAX_DIRFDS as u32 {
        let _ = tcap.open(SlotKind::Dir, CWD_DIR);
        c += 1;
    }
    let cap_ok = tcap.open_count() == MAX_DIRFDS
        && tcap.opens() == MAX_DIRFDS as u32
        && tcap.open(SlotKind::Dir, CWD_DIR) == Err(EMFILE)
        && judge_code(&tcap, CWD_DIR, FIRST_FD + MAX_DIRFDS as i32 - 1, Form::Relative, true, 7) == 0;
    s.add("容量：8 槽满回 EMFILE 且分配账只记成功", cap_ok,
          "失败分配不得记账，open_count 必须停在 MAX_DIRFDS");

    // 注入必抓
    let ledger = run_injection();
    let inject_ok = ledger.shots == 50
        && ledger.detected == 50
        && ledger.escaped == 0
        && ledger.bad == 20
        && ledger.beyond == 10
        && ledger.closed == 10
        && ledger.nondir == 10;
    s.add("注入必抓：四类坏 dirfd × 10 轮全检出、逃逸 0", inject_ok,
          "50 次注入（坏负值 20 / 越界 10 / 已关 10 / 文件 fd 10）必须全抓");
    s.add("账目守恒：注入=检出+逃逸且四类之和=检出", ledger.conserved(),
          "分格计数与总数不等说明有注入未归类");

    // 对账表
    let mut recon_ok = true;
    let mut i2 = 0usize;
    while i2 < RECON.len() {
        if let Some(row) = RECON.get(i2) {
            let dirfd = match row.0 {
                "AT_FDCWD" => AT_FDCWD,
                "-101" => AT_FDCWD - 1,
                "11" => FIRST_FD + MAX_DIRFDS as i32,
                "filefd" => FIRST_FD + 1,
                "closed" => FIRST_FD + 2,
                "INT_MIN" => i32::MIN,
                _ => -1,
            };
            let form = if row.1 == "absolute" { Form::Absolute } else { Form::Relative };
            let exists = row.2 == "存在";
            let got = judge_code(&t, CWD_DIR, dirfd, form, exists, 7);
            if got != row.3 {
                recon_ok = false;
            }
        }
        i2 += 1;
    }
    s.add("对账：10 行 Linux openat 对照表逐行复算相等", recon_ok,
          "表与判定器必须同源；dirfd 语义的每一行都可复算");

    // 双跑一致
    let fp1 = corpus_fingerprint();
    let fp2 = corpus_fingerprint();
    s.add("双跑一致：语料指纹相等", fp1 == fp2,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 文案闭集
    let text_ok = AtCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带非空中文建议", text_ok,
          "12 个码都必须能取到可操作说明");

    // 码唯一
    let mut uniq = true;
    let mut a = 0usize;
    while a < AtCode::ALL.len() {
        let mut b = a + 1;
        while b < AtCode::ALL.len() {
            if let (Some(x), Some(y)) = (AtCode::ALL.get(a), AtCode::ALL.get(b)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            b += 1;
        }
        a += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码重复会让判据无法定位");

    // 账目单调
    let mut mono = ledger;
    let before = mono.shots;
    mono.record(EBADF, EBADF, 0);
    s.add("账目单调：再注入只增不减且守恒", mono.shots == before.saturating_add(1) && mono.conserved(),
          "累计计数禁止回退");

    // 形态计数对账：24 格四类分布
    let dist_ok = g.ok_cells == 8 && g.enoent_cells == 8 && g.notdir_cells == 2 && g.ebadf_cells == 6;
    s.add("分布对账：24 格 = 8 成功 + 8 ENOENT + 2 ENOTDIR + 6 EBADF", dist_ok,
          "四类计数之和必须等于 24，缺一类说明矩阵有格未执行");

    let grid_text = format!(
        "24 格 ok={} enoent={} enotdir={} ebadf={}",
        g.ok_cells, g.enoent_cells, g.notdir_cells, g.ebadf_cells
    );
    s.add_dyn("矩阵快照", grid_ok, &grid_text);
    let ledger_text = format!(
        "注入 {} 检出 {} 逃逸 {} | 坏 {} 越界 {} 关 {} 文件 {}",
        ledger.shots, ledger.detected, ledger.escaped, ledger.bad, ledger.beyond,
        ledger.closed, ledger.nondir
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
    fn at_fdcwd_relative_uses_cwd() {
        let t = sample_table();
        assert_eq!(
            judge_at(&t, CWD_DIR, AT_FDCWD, Form::Relative, true, 7),
            Ok(Resolution { dir: CWD_DIR, ino: 7 })
        );
        assert_eq!(judge_code(&t, CWD_DIR, AT_FDCWD, Form::Relative, false, 7), ENOENT);
    }

    #[test]
    fn absolute_ignores_every_dirfd_class() {
        let t = sample_table();
        for c in FdClass::ALL.iter() {
            assert_eq!(judge_code(&t, CWD_DIR, c.dirfd(), Form::Absolute, true, 7), 0, "{:?}", c);
            assert_eq!(
                judge_code(&t, CWD_DIR, c.dirfd(), Form::Absolute, false, 7),
                ENOENT,
                "{:?}",
                c
            );
        }
    }

    #[test]
    fn relative_bad_dirfds_ebadf() {
        let t = sample_table();
        assert_eq!(judge_code(&t, CWD_DIR, AT_FDCWD - 1, Form::Relative, true, 7), EBADF);
        assert_eq!(judge_code(&t, CWD_DIR, i32::MIN, Form::Relative, true, 7), EBADF);
        assert_eq!(
            judge_code(&t, CWD_DIR, FIRST_FD + MAX_DIRFDS as i32, Form::Relative, false, 7),
            EBADF
        );
        assert_eq!(judge_code(&t, CWD_DIR, FIRST_FD + 2, Form::Relative, true, 7), EBADF);
    }

    #[test]
    fn relative_file_fd_enotdir() {
        let t = sample_table();
        assert_eq!(judge_code(&t, CWD_DIR, FIRST_FD + 1, Form::Relative, true, 7), ENOTDIR);
        assert_eq!(judge_code(&t, CWD_DIR, FIRST_FD + 1, Form::Relative, false, 7), ENOTDIR);
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
        assert_eq!(g.ok_cells, 8);
        assert_eq!(g.enoent_cells, 8);
        assert_eq!(g.notdir_cells, 2);
        assert_eq!(g.ebadf_cells, 6);
    }

    #[test]
    fn boundary_values() {
        let t = sample_table();
        for row in BOUNDARIES.iter() {
            assert_eq!(
                judge_code(&t, CWD_DIR, row.0, Form::Relative, true, 7),
                row.2,
                "{}",
                row.1
            );
        }
    }

    #[test]
    fn fd_slot_state_machine() {
        let mut t = FdTable::new();
        let (fd, gen1) = t.open(SlotKind::Dir, CWD_DIR).unwrap_or((-1, 0));
        assert_eq!(fd, FIRST_FD);
        let closed_gen = t.close(fd).unwrap_or(0);
        assert_eq!(closed_gen, gen1 + 1);
        assert_eq!(t.close(fd), Err(EBADF));
        let (fd2, gen2) = t.open(SlotKind::Dir, CWD_DIR).unwrap_or((-1, 0));
        assert_eq!(fd2, fd);
        assert_eq!(gen2, closed_gen);
        assert_eq!(t.describe_gen(fd, gen1), Err(EBADF));
        assert_eq!(t.describe_gen(fd, gen2), Ok(SlotKind::Dir));
        assert_eq!(t.describe(AT_FDCWD), Err(EBADF));
    }

    #[test]
    fn fd_table_capacity() {
        let mut t = FdTable::new();
        for _ in 0..MAX_DIRFDS {
            assert!(t.open(SlotKind::Dir, CWD_DIR).is_ok());
        }
        assert_eq!(t.open(SlotKind::Dir, CWD_DIR), Err(EMFILE));
        assert_eq!(t.open_count(), MAX_DIRFDS);
        assert_eq!(t.opens(), MAX_DIRFDS as u32);
    }

    #[test]
    fn injection_and_recon() {
        let l = run_injection();
        assert_eq!(l.shots, 50);
        assert_eq!(l.detected, 50);
        assert_eq!(l.escaped, 0);
        assert!(l.conserved());
        let t = sample_table();
        assert_eq!(judge_code(&t, CWD_DIR, -101, Form::Absolute, false, 7), ENOENT);
        assert_eq!(judge_code(&t, CWD_DIR, FIRST_FD + 1, Form::Relative, false, 7), ENOTDIR);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9229_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(s.len() >= 12);
        assert!(!s.truncated());
    }
}
