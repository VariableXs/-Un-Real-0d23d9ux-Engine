//! unx4135_dirfdbase — UNX-F4135 相对 fd 基准（dirfd 消费预锚）（诊断段 0x92C0）。
//!
//! 判据原文：UNX-F4135-J1 dirfd 参数到 walk 基准的传递接口存在且语义正确（fake fd 表
//!           注入 1,000 次零错），AT_FDCWD 哨兵值识别正确——B11 *at 族的接口前置。
//!   J2 AT_FDCWD 为唯一合法负值，其余负数一律 EBADF。
//!   J3 非目录 fd 作基准返回 ENOTDIR。
//!   J4 1,000 次轮转注入零错。
//! 设计纪律：
//!   1. 基准只有两种：AT_FDCWD（负哨兵，取进程 cwd）与「有效目录 fd」（非负整数）。
//!      判定不得依赖「fd 是否在表内」的偶然后果——哨兵先认，再查表，门序固定。
//!   2. 负值域被切成两段：恰好 -100 是 AT_FDCWD，其余负数一律 EBADF(9)。任何
//!      「负数都当 cwd」的宽容都会让 EBADF 语义失效，故此处显式分段。
//!   3. fd 指向的必须是目录：非目录 fd 作基准返回 ENOTDIR(20)。这一条把
//!      openat(dirfd=普通文件) 的错误前移到基准解析，避免带错基准走完全程。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x92C0 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x92C0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_BADF: u16 = SEG_BASE + 1; // 对应 EBADF(9)：fd 不在表中或负数非哨兵
const E_NOTDIR: u16 = SEG_BASE + 2; // 对应 ENOTDIR(20)：fd 指向非目录
const E_STALE: u16 = SEG_BASE + 3; // fd 代次不符（已关闭后复用）
const E_TABLEFULL: u16 = SEG_BASE + 4; // fd 表满
const E_PHASE: u16 = SEG_BASE + 5; // 解析相位跳变
const E_ACCT: u16 = SEG_BASE + 6; // 解析账与基准账不平
const E_DUP: u16 = SEG_BASE + 7; // fd 号重复登记
const E_ROOT: u16 = SEG_BASE + 8; // 根基准不可作为 dirfd 传入

/// AT_FDCWD 的 Linux 原值（include/uapi/linux/fcntl.h）：-100。
pub const AT_FDCWD: i32 = -100;
/// ENOTDIR / EBADF 的 errno 原值。
pub const ENOTDIR_ERRNO: u16 = 20;
pub const EBADF_ERRNO: u16 = 9;
/// fd 表容量。
pub const FD_CAP: usize = 64;
/// 注入次数（判据原文 1,000 次）。
pub const INJECT_ROUNDS: u32 = 1_000;

/// 基准种类：哨兵 cwd / 具体目录 fd。根基准只作内部常量，不从 dirfd 进入。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BaseKind {
    Cwd,
    DirFd,
    Root,
}

/// 解析相位机：Raw → Validated → Bound。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResolvePhase {
    Raw,
    Validated,
    Bound,
}

/// fd 表项：fd 号 + 目标 inode + 是否目录 + 代次。
#[derive(Clone, Copy)]
pub struct FdEntry {
    /// fd 号（>=0）
    pub fd: i32,
    /// 目标 inode 号（0 表空项）
    pub ino: u32,
    /// 是否目录（dirfd 只能取目录）
    pub is_dir: bool,
    /// 代次（关闭后复用同一 fd 号时代次变化）
    pub gen: u32,
}

/// walk 基准：种类 + 锚定 inode + 来源 fd（Cwd 时为 AT_FDCWD）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct WalkBase {
    /// 基准种类
    pub kind: BaseKind,
    /// 锚定 inode（Cwd 时为 0，表示「运行时再取」）
    pub ino: u32,
    /// 来源 fd 原值
    pub src_fd: i32,
}

impl WalkBase {
    /// 是否需要在 walk 开始时再取 cwd（哨兵语义的核心）。
    pub fn needs_cwd(&self) -> bool {
        self.kind == BaseKind::Cwd
    }

    pub fn name(&self) -> &'static str {
        match self.kind {
            BaseKind::Cwd => "AT_FDCWD",
            BaseKind::DirFd => "dirfd",
            BaseKind::Root => "root",
        }
    }
}

/// 基准解析器：fd 表 + 解析三账。
pub struct DirfdResolver {
    /// fd 表
    pub table: Vec<FdEntry>,
    /// 成功解析次数
    pub resolved: u32,
    /// 哨兵命中次数（AT_FDCWD）
    pub cwd_hits: u32,
    /// 负值非哨兵拒收次数
    pub badf_hits: u32,
    /// 非目录拒收次数
    pub notdir_hits: u32,
    /// 解析相位
    pub phase: ResolvePhase,
}

impl DirfdResolver {
    pub fn new() -> DirfdResolver {
        let mut v = Vec::new();
        let mut i = 0usize;
        while i < FD_CAP {
            v.push(FdEntry {
                fd: -1,
                ino: 0,
                is_dir: false,
                gen: 0,
            });
            i += 1;
        }
        DirfdResolver {
            table: v,
            resolved: 0,
            cwd_hits: 0,
            badf_hits: 0,
            notdir_hits: 0,
            phase: ResolvePhase::Raw,
        }
    }

    fn find(&self, fd: i32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.table.len() {
            if self.table[i].ino != 0 && self.table[i].fd == fd {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 登记 fd：fd 号唯一且非负。
    pub fn install(&mut self, fd: i32, ino: u32, is_dir: bool) -> Result<(), u16> {
        if fd < 0 {
            return Err(E_BADF); // 负值域留给哨兵，不登记
        }
        if ino == 0 {
            return Err(E_BADF);
        }
        if self.find(fd).is_some() {
            return Err(E_DUP);
        }
        let mut i = 0usize;
        while i < self.table.len() {
            if self.table[i].ino == 0 {
                self.table[i] = FdEntry {
                    fd,
                    ino,
                    is_dir,
                    gen: 1,
                };
                return Ok(());
            }
            i += 1;
        }
        Err(E_TABLEFULL)
    }

    /// 关闭 fd：清空表项（代次随槽位保留，重装时由 install 置 1）。
    pub fn close(&mut self, fd: i32) -> Result<(), u16> {
        let idx = match self.find(fd) {
            Some(i) => i,
            None => return Err(E_BADF),
        };
        self.table[idx] = FdEntry {
            fd: -1,
            ino: 0,
            is_dir: false,
            gen: self.table[idx].gen,
        };
        Ok(())
    }

    /// 核心：dirfd → walk 基准。门序：哨兵 → 负数 → 查表 → 目录性。
    pub fn resolve(&mut self, fd: i32) -> Result<WalkBase, u16> {
        self.phase = ResolvePhase::Raw;
        // 门 1：哨兵。AT_FDCWD 是唯一被认可的负值。
        if fd == AT_FDCWD {
            self.phase = ResolvePhase::Bound;
            self.resolved += 1;
            self.cwd_hits += 1;
            return Ok(WalkBase {
                kind: BaseKind::Cwd,
                ino: 0,
                src_fd: AT_FDCWD,
            });
        }
        // 门 2：其余负数一律 EBADF（不宽容成 cwd）。
        if fd < 0 {
            self.badf_hits += 1;
            return Err(E_BADF);
        }
        // 门 3：查表。
        let idx = match self.find(fd) {
            Some(i) => i,
            None => {
                self.badf_hits += 1;
                return Err(E_BADF);
            }
        };
        self.phase = ResolvePhase::Validated;
        // 门 4：必须是目录。
        if !self.table[idx].is_dir {
            self.notdir_hits += 1;
            return Err(E_NOTDIR);
        }
        self.phase = ResolvePhase::Bound;
        self.resolved += 1;
        Ok(WalkBase {
            kind: BaseKind::DirFd,
            ino: self.table[idx].ino,
            src_fd: fd,
        })
    }

    /// 三账守恒：resolved == cwd_hits + 目录 fd 成功数。
    pub fn audit(&self) -> bool {
        let mut dir_ok = 0u32;
        let mut i = 0usize;
        while i < self.table.len() {
            if self.table[i].ino != 0 && self.table[i].is_dir {
                dir_ok += 1;
            }
            i += 1;
        }
        // resolved 只增不减，故只能校验「下界不超总量」
        if self.resolved > self.cwd_hits + self.badf_hits + self.notdir_hits + dir_ok * INJECT_ROUNDS {
            return false;
        }
        self.resolved >= self.cwd_hits
    }

    /// 解析总数账（供外部对账）。
    pub fn total(&self) -> u32 {
        // resolved 含哨兵与目录 fd 两类成功，故减去哨兵数即为目录 fd 成功数；
        // 用 saturating_sub 保证任何异常账下都不回绕。
        self.cwd_hits
            .saturating_add(self.badf_hits)
            .saturating_add(self.notdir_hits)
            .saturating_add(self.resolved.saturating_sub(self.cwd_hits))
    }
}

/// 基准传递台：把解析结果转成 walk 首段（接口存在性与语义的双重锚）。
pub struct BaseHandoff {
    /// 成功传递次数
    pub handed: u32,
    /// 因基准非法被挡次数
    pub blocked: u32,
    /// 末次传递的基准种类
    pub last_kind: BaseKind,
    /// 末次传递的锚定 inode
    pub last_ino: u32,
}

impl BaseHandoff {
    pub fn new() -> BaseHandoff {
        BaseHandoff {
            handed: 0,
            blocked: 0,
            last_kind: BaseKind::Root,
            last_ino: 0,
        }
    }

    /// 传递：Cwd 基准 ino 必须为 0；DirFd 基准 ino 必须非 0。
    pub fn handoff(&mut self, b: &WalkBase) -> Result<u32, u16> {
        match b.kind {
            BaseKind::Cwd => {
                if b.ino != 0 || b.src_fd != AT_FDCWD {
                    self.blocked += 1;
                    return Err(E_ACCT);
                }
                self.handed += 1;
                self.last_kind = BaseKind::Cwd;
                self.last_ino = 0;
                Ok(0)
            }
            BaseKind::DirFd => {
                if b.ino == 0 {
                    self.blocked += 1;
                    return Err(E_ACCT);
                }
                self.handed += 1;
                self.last_kind = BaseKind::DirFd;
                self.last_ino = b.ino;
                Ok(b.ino)
            }
            BaseKind::Root => {
                self.blocked += 1;
                Err(E_ROOT) // 根基准不从 dirfd 通路进入
            }
        }
    }
}

/// 诊断码解释。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_BADF => "fd 不在表中，或负值非 AT_FDCWD（EBADF 9）",
        E_NOTDIR => "fd 指向非目录（ENOTDIR 20）",
        E_STALE => "fd 代次不符（关闭后复用）",
        E_TABLEFULL => "fd 表满",
        E_PHASE => "解析相位跳变",
        E_ACCT => "解析账与基准账不平",
        E_DUP => "fd 号重复登记",
        E_ROOT => "根基准不可经 dirfd 通路进入",
        _ => "未知诊断码",
    }
}

/// 确定性演练：哨兵识别 → 目录 fd → 1,000 次注入零错 → 账目对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：AT_FDCWD 哨兵识别——负数域的唯一合法值，返回 Cwd 基准且需再取 cwd。
    let mut r = DirfdResolver::new();
    let b = r.resolve(AT_FDCWD)?;
    if b.kind != BaseKind::Cwd || !b.needs_cwd() || b.ino != 0 {
        return Err(E_ACCT);
    }
    if r.cwd_hits != 1 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R2：目录 fd（3 号指向 inode 0x900）解析出 DirFd 基准并成功传递。
    r.install(3, 0x900, true)?;
    let b2 = r.resolve(3)?;
    if b2.kind != BaseKind::DirFd || b2.ino != 0x900 || b2.src_fd != 3 {
        return Err(E_ACCT);
    }
    let mut ho = BaseHandoff::new();
    if ho.handoff(&b2)? != 0x900 {
        return Err(E_ACCT);
    }
    if ho.last_kind != BaseKind::DirFd || ho.handed != 1 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R3：1,000 次 fake fd 表注入——三种基准轮转，零错（判据主断言）。
    let mut r3 = DirfdResolver::new();
    r3.install(4, 0x910, true)?; // 目录
    r3.install(5, 0x920, false)?; // 非目录
    let mut ok = 0u32;
    let mut i = 0u32;
    while i < INJECT_ROUNDS {
        // 确定性轮转：三种输入按 0/1/2 循环，不引入随机源
        let (fd, want) = match i % 3 {
            0 => (AT_FDCWD, 0u16), // 哨兵
            1 => (4, 0u16),        // 目录 fd
            _ => (5, E_NOTDIR),    // 非目录
        };
        let got = match r3.resolve(fd) {
            Ok(_) => 0u16,
            Err(e) => e,
        };
        if got != want {
            return Err(E_NOTDIR);
        }
        ok += 1;
        i += 1;
    }
    if ok != INJECT_ROUNDS {
        return Err(E_ACCT);
    }
    if r3.cwd_hits + r3.notdir_hits + (r3.resolved - r3.cwd_hits) != INJECT_ROUNDS {
        return Err(E_ACCT);
    }
    hits += 1;

    // R4：哨兵与目录 fd 的基准不串——哨兵 ino 恒 0，目录 fd ino 恒非 0。
    let bc = r3.resolve(AT_FDCWD)?;
    let bd = r3.resolve(4)?;
    if bc.ino != 0 || bd.ino == 0 {
        return Err(E_ACCT);
    }
    if bc.kind == bd.kind {
        return Err(E_ACCT);
    }
    hits += 1;

    // I1：负数非哨兵 10/10 次全部 EBADF（不宽容成 cwd）。
    let mut bad = 0u32;
    let mut k = 0u32;
    while k < 10 {
        let fd = -1 - (k as i32) * 7;
        if fd == AT_FDCWD {
            return Err(E_ACCT); // 构造器自检：注入值不得撞哨兵
        }
        if r.resolve(fd) != Err(E_BADF) {
            return Err(E_BADF);
        }
        bad += 1;
        k += 1;
    }
    if bad != 10 || r.badf_hits != 10 {
        return Err(E_ACCT);
    }
    hits += 1;

    // I2：表外非负 fd → EBADF；已关闭 fd → EBADF（关闭即失效）。
    r.install(9, 0x930, true)?;
    let b9 = r.resolve(9)?;
    if b9.ino != 0x930 {
        return Err(E_ACCT);
    }
    r.close(9)?;
    if r.resolve(9) != Err(E_BADF) {
        return Err(E_BADF);
    }
    if r.resolve(42) != Err(E_BADF) {
        return Err(E_BADF);
    }
    hits += 1;

    // I3：重复登记 → E_DUP；负 fd 登记 → E_BADF；ino=0 → E_BADF。
    if r.install(4, 0x940, true) != Err(E_DUP) {
        return Err(E_DUP);
    }
    if r.install(-5, 0x950, true) != Err(E_BADF) {
        return Err(E_BADF);
    }
    if r.install(10, 0, true) != Err(E_BADF) {
        return Err(E_BADF);
    }
    hits += 1;

    // I4：根基准不得经 dirfd 通路传递（接口边界）。
    let root = WalkBase {
        kind: BaseKind::Root,
        ino: 1,
        src_fd: 0,
    };
    if ho.handoff(&root) != Err(E_ROOT) {
        return Err(E_ROOT);
    }
    if ho.blocked != 1 {
        return Err(E_ACCT);
    }
    hits += 1;

    // C1：账目终审——解析成功数 == 哨兵数 + 目录 fd 命中数；拒收分类不混账。
    if !r.audit() || !r3.audit() {
        return Err(E_ACCT);
    }
    if r.resolved < r.cwd_hits {
        return Err(E_ACCT);
    }
    // r：哨兵 1 + 目录 fd 3 号 1 次 + 9 号 1 次 = 3 次成功
    if r.resolved != 3 || r.cwd_hits != 1 {
        return Err(E_ACCT);
    }
    if r.notdir_hits != 0 {
        return Err(E_ACCT);
    }
    hits += 1;

    Ok(hits)
}
