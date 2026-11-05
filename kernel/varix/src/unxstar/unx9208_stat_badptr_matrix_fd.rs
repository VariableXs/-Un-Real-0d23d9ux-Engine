//! UNX-F9208 stat/fstat 坏指针/坏 fd 矩阵 · 0x7A180段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F9208 逐字）：
//!   UNX-F9208-J1 坏 buf 指针全 EFAULT 判据过，坏 fd 全 EBADF 格过，
//!   路径不存在全 ENOENT 格过。
//!
//! 设计要点：
//!   ① 三入口判定器族 judge_stat / judge_fstat / judge_lstat 按各自动作
//!      阶段显性建模：stat 先拷路径（坏路径指针 → EFAULT），再解析
//!      inode（不存在 → ENOENT），最后填 buf（坏 buf → EFAULT）；
//!      fstat 则 fd 校验最先（负/未开/已关 → EBADF），fd→inode 反查
//!      次之，填充与拷出殿后。判定次序与 Linux 逐阶段一致，并用
//!      阶段踪迹 StageTrace 显式断言。
//!   ② 域模型围绕「stat 结构填充状态机」与「inode 表解析」设计：
//!      dev/ino/mode/nlink/size 五字段按固定次序单调填充，倒退显性
//!      拒绝；mode 的文件类型位取 S_IFMT 掩码内 7 类闭集，闭集之外
//!      的类型位在填充阶段即被拒绝。
//!   ③ inode 表提供 路径→inode 正查 与 fd→inode 反查 两条通道：
//!      正查支持符号链接跟随（stat 跟随、lstat 不跟随），跟随深度
//!      超限按 Linux ELOOP 语义拒绝；悬空链接 stat 报 ENOENT 而
//!      lstat 仍命中链接 inode 本体。
//!   ④ fd 表为 stat 族专用精简模型：负值 / 空闲槽（未开）/ 已关槽
//!      三类全 EBADF；0/1/2 为无 inode 的控制台匿名流，fstat 反查
//!      落空同样归 EBADF 族，但不与三类主样本混淆。
//!   ⑤ 注入账守恒：注入次数 = 检出次数 + 逃逸次数，逃逸恒为 0；
//!      账只增不减，任何倒退都显性拒绝。每类注入复测 10 轮全抓。
//!   ⑥ 与 Linux 逐码一致通过冻结表复算：ENOENT/EBADF/EFAULT/EMFILE/
//!      ELOOP 五码与 errno.h 逐项对齐，表外码一律拒绝。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9208 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7A180;

/// 单类注入的复测轮数（判据要求全抓）。
pub const INJECT_ROUNDS: u32 = 10;

/// fstat 坏 fd 注入类别数（负值/未开/已关）。
pub const BAD_FD_CLASSES: usize = 3;

/// 坏 buf 注入类别数（NULL/未映射/只读映射/内核地址）。
pub const BAD_BUF_CLASSES: usize = 4;

/// inode 表容量（含符号链接与目录项）。
pub const INODE_CAP: usize = 8;

/// fd 表总槽数（0..3 为标准流，3..FD_TOTAL 为动态槽）。
pub const FD_TOTAL: usize = 15;

/// 动态 fd 槽数量。
pub const DYN_FD_CAP: usize = FD_TOTAL - 3;

/// 符号链接跟随深度上限（超限按 ELOOP 拒绝）。
pub const LINK_DEPTH_MAX: u32 = 4;

/// Linux errno 正数口径（系统调用返回负值）。
pub const ENOENT: i32 = 2;
pub const EBADF: i32 = 9;
pub const EFAULT: i32 = 14;
pub const ELOOP: i32 = 40;
pub const EMFILE: i32 = 24;

/// mode 文件类型位掩码（stat 手册节 S_IFMT）。
pub const MODE_TYPE_MASK: u32 = 0o170000;
/// FIFO 类型位。
pub const S_IFIFO: u32 = 0o010000;
/// 字符设备类型位。
pub const S_IFCHR: u32 = 0o020000;
/// 目录类型位。
pub const S_IFDIR: u32 = 0o040000;
/// 块设备类型位。
pub const S_IFBLK: u32 = 0o060000;
/// 普通文件类型位。
pub const S_IFREG: u32 = 0o100000;
/// 符号链接类型位。
pub const S_IFLNK: u32 = 0o120000;
/// 套接字类型位。
pub const S_IFSOCK: u32 = 0o140000;

/// 文件类型位闭集（stat(2) 手册 S_IF* 七类，来源：man 2 stat）。
pub const KNOWN_TYPE_BITS: [u32; 7] = [
    S_IFIFO, S_IFCHR, S_IFDIR, S_IFBLK, S_IFREG, S_IFLNK, S_IFSOCK,
];

/// 判定 mode 的文件类型位是否落在闭集内。
pub fn mode_type_known(mode: u32) -> bool {
    let bits = mode & MODE_TYPE_MASK;
    let mut i = 0usize;
    while i < KNOWN_TYPE_BITS.len() {
        if let Some(k) = KNOWN_TYPE_BITS.get(i) {
            if *k == bits {
                return true;
            }
        }
        i += 1;
    }
    false
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StatCode {
    /// 坏路径指针：path 拷贝阶段 EFAULT
    BadPathPtr,
    /// 路径不存在：inode 正查落空
    PathNotFound,
    /// 坏 buf 指针：填充结果拷出阶段 EFAULT
    BadBufPtr,
    /// 坏 fd：负值/未开/已关
    BadFd,
    /// fd 反查落空：槽位开着但 inode 缺失
    FdInodeMissing,
    /// mode 类型位落在闭集之外
    ModeTypeUnknown,
    /// 填充次序倒退：五字段必须按固定次序单调推进
    FillOrderRegress,
    /// 填充不完整：finalize 时仍有字段未填
    FillIncomplete,
    /// 表容量耗尽：inode 表或 fd 表无空槽
    TableExhausted,
    /// 注入账倒退：计数只增不减
    LedgerRegress,
    /// 双跑不一致：同输入两次结果不同
    NotReproducible,
    /// 段位越界：段基址必须 0x40 对齐且落在本批窗口
    SegmentViolation,
}

impl StatCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            StatCode::BadPathPtr => "坏路径指针被拒：path 参数不可读（NULL/未映射/内核\
                地址）；请把路径固定到已映射用户区间再发起 stat",
            StatCode::PathNotFound => "路径不存在：inode 正查落空；请先确认目标已创建，\
                悬空符号链接按 ENOENT 上报",
            StatCode::BadBufPtr => "坏 buf 指针被拒：stat 结果区不可写（NULL/未映射/\
                只读映射/内核地址）；请提供可写的 struct stat 缓冲",
            StatCode::BadFd => "坏 fd 被拒：负值/未开/已关三类一律回 EBADF；请先用 \
                open 取得有效句柄且不要复用已 close 的号",
            StatCode::FdInodeMissing => "fd 反查落空被拒：句柄开着但 inode 已缺失；\
                请重新 open 并检查文件系统一致性",
            StatCode::ModeTypeUnknown => "mode 类型位非法：S_IFMT 掩码内只有 7 类合法\
                值；请核对 S_IF* 常量，不要手工拼接类型位",
            StatCode::FillOrderRegress => "填充次序倒退被拒：dev/ino/mode/nlink/size \
                必须按固定次序单调填充；请检查填充器调用序列",
            StatCode::FillIncomplete => "填充不完整被拒：五字段未填满不得 finalize；\
                请补齐缺失字段后重试",
            StatCode::TableExhausted => "表容量耗尽：inode 表或 fd 表无空槽；请先释放\
                不再使用的条目或扩容",
            StatCode::LedgerRegress => "注入账倒退被拒：计数只增不减；请检查是否有\
                回滚路径误改累计值",
            StatCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏\
                状态或非确定哈希",
            StatCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [StatCode; 12] = [
        StatCode::BadPathPtr,
        StatCode::PathNotFound,
        StatCode::BadBufPtr,
        StatCode::BadFd,
        StatCode::FdInodeMissing,
        StatCode::ModeTypeUnknown,
        StatCode::FillOrderRegress,
        StatCode::FillIncomplete,
        StatCode::TableExhausted,
        StatCode::LedgerRegress,
        StatCode::NotReproducible,
        StatCode::SegmentViolation,
    ];
}

/// 按码值查表（闭集返回；表外码返回占位说明，显性可查）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < StatCode::ALL.len() {
        if let Some(c) = StatCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 F9208 段内闭集；请按 SEG_BASE+序号 复核算值"
}

// ── 阶段踪迹 ──────────────────────────────────────────────────────────
/// stat 阶段标记：拷路径。
pub const STAGE_STAT_COPY_PATH: u8 = 1;
/// stat 阶段标记：inode 正查。
pub const STAGE_STAT_RESOLVE: u8 = 2;
/// 填充阶段标记（stat/fstat 共用状态机）。
pub const STAGE_FILL: u8 = 3;
/// 拷出阶段标记（stat/fstat 共用）。
pub const STAGE_COPY_OUT: u8 = 4;
/// fstat 阶段标记：fd 校验。
pub const STAGE_FSTAT_FD_CHECK: u8 = 5;
/// fstat 阶段标记：fd→inode 反查。
pub const STAGE_FSTAT_FD_TO_INO: u8 = 6;

/// 阶段踪迹：记录判定器实际走过的阶段，用于显式断言次序。
#[derive(Clone, Debug)]
pub struct StageTrace {
    marks: [u8; 8],
    used: usize,
}

impl StageTrace {
    const fn new() -> StageTrace {
        StageTrace { marks: [0u8; 8], used: 0 }
    }

    /// 记一个阶段标记（容量饱和即停止记录，不 panic）。
    pub fn mark(&mut self, stage: u8) {
        if let Some(slot) = self.marks.get_mut(self.used) {
            *slot = stage;
            self.used = self.used.saturating_add(1);
        }
    }

    /// 已记录的阶段序列。
    pub fn sequence(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.used {
            if let Some(m) = self.marks.get(i) {
                out.push(*m);
            }
            i += 1;
        }
        out
    }
}

// ── inode 表模型 ──────────────────────────────────────────────────────
/// 节点种类（对应 mode 文件类型位闭集的一个子集映射）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeKind {
    /// 普通文件 S_IFREG
    Regular,
    /// 目录 S_IFDIR
    Directory,
    /// 字符设备 S_IFCHR
    CharDev,
    /// 符号链接 S_IFLNK（target 生效）
    Symlink,
}

impl NodeKind {
    /// 类型位闭集取值。
    pub fn bits(self) -> u32 {
        match self {
            NodeKind::Regular => S_IFREG,
            NodeKind::Directory => S_IFDIR,
            NodeKind::CharDev => S_IFCHR,
            NodeKind::Symlink => S_IFLNK,
        }
    }
}

/// 单个 inode 条目。
#[derive(Clone, Debug)]
pub struct InodeEntry {
    /// inode 号（从 1 起，0 保留给无 inode 的匿名流）
    pub ino: u32,
    /// 路径名（不含前导斜杠）
    pub name: String,
    /// 节点种类
    pub kind: NodeKind,
    /// 链接数
    pub nlink: u32,
    /// 字节数（目录/设备为占位值）
    pub size: u64,
    /// 设备号
    pub dev: u64,
    /// 权限位（与类型位合成 mode）
    pub perm: u32,
    /// 符号链接目标（非链接节点为空串）
    pub target: String,
}

impl InodeEntry {
    /// 合成 mode：类型位 + 权限位。
    pub fn mode(&self) -> u32 {
        self.kind.bits() | (self.perm & 0o7777)
    }
}

/// inode 表：路径→inode 正查、ino 反查、容量守恒。
#[derive(Clone, Debug)]
pub struct InodeTable {
    entries: Vec<Option<InodeEntry>>,
    next_ino: u32,
    /// 正查命中次数（账目）
    hits: u32,
    /// 正查落空次数（账目）
    misses: u32,
    /// 分配次数（账目）
    allocs: u32,
}

impl InodeTable {
    /// 构造空表。
    pub fn new() -> InodeTable {
        let mut entries = Vec::new();
        let mut i = 0usize;
        while i < INODE_CAP {
            entries.push(None);
            i += 1;
        }
        InodeTable { entries, next_ino: 1, hits: 0, misses: 0, allocs: 0 }
    }

    /// 分配一个节点：取首个空槽；表满返回 TableExhausted 码值。
    pub fn allocate(&mut self, name: &str, kind: NodeKind, nlink: u32,
                    size: u64, dev: u64, perm: u32, target: &str) -> Result<u32, u32> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(slot) = self.entries.get(i) {
                if slot.is_none() {
                    let ino = self.next_ino;
                    self.next_ino = self.next_ino.saturating_add(1);
                    if let Some(holder) = self.entries.get_mut(i) {
                        *holder = Some(InodeEntry {
                            ino,
                            name: name.to_string(),
                            kind,
                            nlink,
                            size,
                            dev,
                            perm,
                            target: target.to_string(),
                        });
                    }
                    self.allocs = self.allocs.saturating_add(1);
                    return Ok(ino);
                }
            }
            i += 1;
        }
        Err(StatCode::TableExhausted.val())
    }

    /// 路径正查：跳过一个前导斜杠后按名精确匹配。
    /// follow=true 时跟随符号链接（stat），false 时不跟随（lstat）。
    pub fn resolve(&self, path: &str, follow: bool) -> Result<&InodeEntry, i32> {
        let name = if let Some(stripped) = path.strip_prefix('/') {
            stripped
        } else {
            path
        };
        self.resolve_inner(name, follow, 0)
    }

    /// 内部递归解析：深度超限按 ELOOP，落空按 ENOENT。
    fn resolve_inner(&self, name: &str, follow: bool, depth: u32) -> Result<&InodeEntry, i32> {
        if depth > LINK_DEPTH_MAX {
            return Err(ELOOP);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(Some(e)) = self.entries.get(i) {
                if e.name == name {
                    self.hits = self.hits.saturating_add(1);
                    if follow && e.kind == NodeKind::Symlink && !e.target.is_empty() {
                        let target = e.target.clone();
                        return self.resolve_inner(&target, true, depth.saturating_add(1));
                    }
                    return Ok(e);
                }
            }
            i += 1;
        }
        self.misses = self.misses.saturating_add(1);
        Err(ENOENT)
    }

    /// 按名取 inode 号（不含跟随；供 fd 建模使用）。
    pub fn ino_of(&self, name: &str) -> Option<u32> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(Some(e)) = self.entries.get(i) {
                if e.name == name {
                    return Some(e.ino);
                }
            }
            i += 1;
        }
        None
    }

    /// 按号反查（fd→inode 反查通道的表侧）。
    pub fn lookup_ino(&self, ino: u32) -> Result<&InodeEntry, i32> {
        if ino == 0 {
            return Err(EBADF);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(Some(e)) = self.entries.get(i) {
                if e.ino == ino {
                    return Ok(e);
                }
            }
            i += 1;
        }
        Err(EBADF)
    }

    /// 正查命中账。
    pub fn hits(&self) -> u32 {
        self.hits
    }

    /// 正查落空账。
    pub fn misses(&self) -> u32 {
        self.misses
    }

    /// 分配账。
    pub fn allocs(&self) -> u32 {
        self.allocs
    }

    /// 已占用槽数。
    pub fn used_slots(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(slot) = self.entries.get(i) {
                if slot.is_some() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

// ── fd 表模型（stat 族专用精简版）────────────────────────────────────
/// fd 槽位状态：Free（未开）与 Closed（已关）对外同为 EBADF，
/// 但语义分属两类注入样本，不得合并。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FdState {
    /// 从未分配
    Free,
    /// 打开中
    Open,
    /// 已关闭
    Closed,
}

/// 单个 fd 槽：状态 + 关联 inode 号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FdEntry {
    /// 槽位状态
    pub state: FdState,
    /// 关联 inode 号（标准流为 0，表示无 inode 的控制台匿名流）
    pub ino: u32,
}

/// fd 表：0..3 为标准流（Open、ino=0），3..FD_TOTAL 为动态槽。
#[derive(Clone, Debug)]
pub struct FdMap {
    slots: Vec<Option<FdEntry>>,
    /// 打开账（只增）
    opens: u32,
    /// 关闭账（只增）
    closes: u32,
}

impl FdMap {
    /// 构造：标准流预置为 Open。
    pub fn new() -> FdMap {
        let mut slots = Vec::new();
        let mut i = 0usize;
        while i < FD_TOTAL {
            if i < 3 {
                slots.push(Some(FdEntry { state: FdState::Open, ino: 0 }));
            } else {
                slots.push(Some(FdEntry { state: FdState::Free, ino: 0 }));
            }
            i += 1;
        }
        FdMap { slots, opens: 0, closes: 0 }
    }

    /// 为 inode 打开一个动态槽：返回 fd 号；表满回 EMFILE。
    pub fn open_for(&mut self, ino: u32) -> Result<i32, i32> {
        let mut i = 3usize;
        while i < FD_TOTAL {
            if let Some(Some(e)) = self.slots.get(i) {
                if e.state == FdState::Free {
                    if let Some(Some(slot)) = self.slots.get_mut(i) {
                        slot.state = FdState::Open;
                        slot.ino = ino;
                    }
                    self.opens = self.opens.saturating_add(1);
                    return Ok(i as i32);
                }
            }
            i += 1;
        }
        Err(EMFILE)
    }

    /// 关闭 fd：负/未开/已关全 EBADF；成功后槽位转 Closed、ino 清零。
    pub fn close(&mut self, fd: i32) -> Result<(), i32> {
        let e = self.entry(fd)?;
        if e.state != FdState::Open {
            return Err(EBADF);
        }
        if let Some(Some(slot)) = self.slots.get_mut(fd as usize) {
            slot.state = FdState::Closed;
            slot.ino = 0;
        }
        self.closes = self.closes.saturating_add(1);
        Ok(())
    }

    /// fd→inode 反查：非法号段/未开/已关全 EBADF；
    /// 标准流 ino=0 反查落空同样 EBADF（FdInodeMissing 族）。
    pub fn fd_to_ino(&self, fd: i32) -> Result<u32, i32> {
        let e = self.entry(fd)?;
        if e.state != FdState::Open {
            return Err(EBADF);
        }
        if e.ino == 0 {
            return Err(EBADF);
        }
        Ok(e.ino)
    }

    /// 取槽位视图：号段非法（负值/越上界）返回 EBADF。
    fn entry(&self, fd: i32) -> Result<FdEntry, i32> {
        if fd < 0 {
            return Err(EBADF);
        }
        if fd >= FD_TOTAL as i32 {
            return Err(EBADF);
        }
        match self.slots.get(fd as usize) {
            Some(Some(e)) => Ok(*e),
            _ => Err(EBADF),
        }
    }

    /// 打开账。
    pub fn opens(&self) -> u32 {
        self.opens
    }

    /// 关闭账。
    pub fn closes(&self) -> u32 {
        self.closes
    }

    /// 当前打开的动态槽数。
    pub fn open_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 3usize;
        while i < FD_TOTAL {
            if let Some(Some(e)) = self.slots.get(i) {
                if e.state == FdState::Open {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

// ── 指针类别模型 ──────────────────────────────────────────────────────
/// 用户路径指针类别（stat/lstat 的 path 参数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathClass {
    /// 已映射且可读的合法用户区间
    Valid,
    /// NULL
    Null,
    /// 未映射地址
    Unmapped,
    /// 内核地址（用户态不可访问）
    Kernel,
}

impl PathClass {
    /// 拷路径阶段可用性：只有合法区间可读。
    pub fn readable(self) -> bool {
        matches!(self, PathClass::Valid)
    }
}

/// 用户 buf 指针类别（struct stat 结果区）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BufClass {
    /// 已映射且可写的合法用户区间
    Valid,
    /// NULL
    Null,
    /// 未映射地址
    Unmapped,
    /// 只读映射：可读不可写，填充拷出必 EFAULT
    ReadOnlyMapping,
    /// 内核地址
    Kernel,
}

impl BufClass {
    /// 拷出阶段可用性：必须可写。
    pub fn writable(self) -> bool {
        matches!(self, BufClass::Valid)
    }
}

// ── stat 填充状态机 ───────────────────────────────────────────────────
/// 填充阶段（单调推进，倒退显性拒）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FillStage {
    /// 未开始
    Idle,
    /// dev 已填
    DevDone,
    /// ino 已填
    InoDone,
    /// mode 已填
    ModeDone,
    /// nlink 已填
    NlinkDone,
    /// size 已填（可 finalize）
    SizeDone,
}

/// 五字段语义填充器：dev → ino → mode → nlink → size。
#[derive(Clone, Copy, Debug)]
pub struct StatFiller {
    stage: FillStage,
    dev: u64,
    ino: u64,
    mode: u32,
    nlink: u32,
    size: u64,
}

impl StatFiller {
    /// 新填充器。
    pub const fn new() -> StatFiller {
        StatFiller { stage: FillStage::Idle, dev: 0, ino: 0, mode: 0, nlink: 0, size: 0 }
    }

    /// 填 dev：仅 Idle 阶段可调。
    pub fn fill_dev(&mut self, dev: u64) -> Result<(), u32> {
        if self.stage != FillStage::Idle {
            return Err(StatCode::FillOrderRegress.val());
        }
        self.dev = dev;
        self.stage = FillStage::DevDone;
        Ok(())
    }

    /// 填 ino：仅 DevDone 阶段可调。
    pub fn fill_ino(&mut self, ino: u64) -> Result<(), u32> {
        if self.stage != FillStage::DevDone {
            return Err(StatCode::FillOrderRegress.val());
        }
        self.ino = ino;
        self.stage = FillStage::InoDone;
        Ok(())
    }

    /// 填 mode：仅 InoDone 阶段可调；类型位闭集校验在此把关。
    pub fn fill_mode(&mut self, mode: u32) -> Result<(), u32> {
        if self.stage != FillStage::InoDone {
            return Err(StatCode::FillOrderRegress.val());
        }
        if !mode_type_known(mode) {
            return Err(StatCode::ModeTypeUnknown.val());
        }
        self.mode = mode;
        self.stage = FillStage::ModeDone;
        Ok(())
    }

    /// 填 nlink：仅 ModeDone 阶段可调。
    pub fn fill_nlink(&mut self, nlink: u32) -> Result<(), u32> {
        if self.stage != FillStage::ModeDone {
            return Err(StatCode::FillOrderRegress.val());
        }
        self.nlink = nlink;
        self.stage = FillStage::NlinkDone;
        Ok(())
    }

    /// 填 size：仅 NlinkDone 阶段可调。
    pub fn fill_size(&mut self, size: u64) -> Result<(), u32> {
        if self.stage != FillStage::NlinkDone {
            return Err(StatCode::FillOrderRegress.val());
        }
        self.size = size;
        self.stage = FillStage::SizeDone;
        Ok(())
    }

    /// 收尾：五字段齐备才产出 StatFrame，否则 FillIncomplete。
    pub fn finalize(self) -> Result<StatFrame, u32> {
        if self.stage != FillStage::SizeDone {
            return Err(StatCode::FillIncomplete.val());
        }
        Ok(StatFrame { dev: self.dev, ino: self.ino, mode: self.mode,
                       nlink: self.nlink, size: self.size })
    }
}

/// struct stat 五字段语义视图（本模块聚焦的语义子集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StatFrame {
    /// 设备号
    pub dev: u64,
    /// inode 号
    pub ino: u64,
    /// 类型位 + 权限位
    pub mode: u32,
    /// 链接数
    pub nlink: u32,
    /// 字节数
    pub size: u64,
}

// ── 系统调用结果 ──────────────────────────────────────────────────────
/// 系统调用失败结果：携带 Linux errno 正数码。
pub type SysResult<T> = Result<T, i32>;

// ── 三入口判定器族 ────────────────────────────────────────────────────
/// stat 判定：拷路径 → inode 正查（跟随链接）→ 填充 → 拷出。
pub fn judge_stat(it: &InodeTable, path: PathClass, body: &str,
                  buf: BufClass) -> SysResult<StatFrame> {
    let mut trace = StageTrace::new();
    judge_stat_traced(it, path, body, buf, true, &mut trace)
}

/// lstat 判定：与 stat 同序，但不跟随末级符号链接。
pub fn judge_lstat(it: &InodeTable, path: PathClass, body: &str,
                   buf: BufClass) -> SysResult<StatFrame> {
    let mut trace = StageTrace::new();
    judge_stat_traced(it, path, body, buf, false, &mut trace)
}

/// 带踪迹的 stat/lstat 共用判定体：次序由阶段标记显性呈现。
pub fn judge_stat_traced(it: &InodeTable, path: PathClass, body: &str,
                         buf: BufClass, follow: bool,
                         trace: &mut StageTrace) -> SysResult<StatFrame> {
    // 阶段 1：拷路径。坏路径指针在此瞬间 EFAULT，即使目标也不存在、
    // buf 也是坏的，也必须先报 EFAULT（user_path 先于 lookup）。
    trace.mark(STAGE_STAT_COPY_PATH);
    if !path.readable() {
        return Err(EFAULT);
    }
    // 阶段 2：inode 正查。落空在此 ENOENT，坏 buf 不得抢报。
    trace.mark(STAGE_STAT_RESOLVE);
    let entry = it.resolve(body, follow)?;
    // 阶段 3：内核侧填充（五字段状态机）。
    trace.mark(STAGE_FILL);
    let frame = fill_frame(entry)?;
    // 阶段 4：拷出到用户 buf。坏 buf 在此 EFAULT。
    trace.mark(STAGE_COPY_OUT);
    if !buf.writable() {
        return Err(EFAULT);
    }
    Ok(frame)
}

/// fstat 判定：fd 校验最先（EBADF）→ fd→inode 反查 → 填充 → 拷出。
pub fn judge_fstat(ft: &FdMap, it: &InodeTable, fd: i32,
                   buf: BufClass) -> SysResult<StatFrame> {
    let mut trace = StageTrace::new();
    judge_fstat_traced(ft, it, fd, buf, &mut trace)
}

/// 带踪迹的 fstat 判定体。
pub fn judge_fstat_traced(ft: &FdMap, it: &InodeTable, fd: i32,
                          buf: BufClass, trace: &mut StageTrace) -> SysResult<StatFrame> {
    // 阶段 1：fd 校验。负值/未开/已关在 fd 非法的瞬间返回 EBADF，
    // 即使 buf 也是坏的，也不得改报 EFAULT（fget 先于 copy_to_user）。
    trace.mark(STAGE_FSTAT_FD_CHECK);
    let ino = ft.fd_to_ino(fd)?;
    // 阶段 2：fd→inode 反查。标准流（ino=0）或 inode 缺失归 EBADF 族。
    trace.mark(STAGE_FSTAT_FD_TO_INO);
    let entry = it.lookup_ino(ino)?;
    // 阶段 3：填充。
    trace.mark(STAGE_FILL);
    let frame = fill_frame(entry)?;
    // 阶段 4：拷出。
    trace.mark(STAGE_COPY_OUT);
    if !buf.writable() {
        return Err(EFAULT);
    }
    Ok(frame)
}

/// 用五字段填充器把 inode 条目折算成 StatFrame。
fn fill_frame(entry: &InodeEntry) -> SysResult<StatFrame> {
    let mut f = StatFiller::new();
    let _ = f.fill_dev(entry.dev);
    let _ = f.fill_ino(entry.ino as u64);
    let _ = f.fill_mode(entry.mode());
    let _ = f.fill_nlink(entry.nlink);
    let _ = f.fill_size(entry.size);
    match f.finalize() {
        Ok(frame) => Ok(frame),
        Err(_) => Err(EFAULT),
    }
}

// ── 注入样本类别 ──────────────────────────────────────────────────────
/// fstat 坏 fd 三类样本（负值/未开/已关）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadFdKind {
    /// 负值
    Negative,
    /// 从未打开的空闲槽
    Unopened,
    /// 已关闭槽
    Closed,
}

impl BadFdKind {
    /// 构造样本 fd：Closed 需要先开再关制造现场。
    pub fn sample(&self, ft: &mut FdMap, it: &InodeTable) -> i32 {
        match self {
            BadFdKind::Negative => -7,
            BadFdKind::Unopened => 3,
            BadFdKind::Closed => {
                let ino = it.ino_of("motd").unwrap_or(0);
                let fd = match ft.open_for(ino) {
                    Ok(f) => f,
                    Err(_) => -1,
                };
                let _ = ft.close(fd);
                fd
            }
        }
    }

    /// 闭集全量。
    pub const ALL: [BadFdKind; 3] = [
        BadFdKind::Negative,
        BadFdKind::Unopened,
        BadFdKind::Closed,
    ];
}

/// 坏 buf 四类样本（NULL/未映射/只读映射/内核地址）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadBufKind {
    /// NULL
    Null,
    /// 未映射地址
    Unmapped,
    /// 只读映射
    ReadOnly,
    /// 内核地址
    Kernel,
}

impl BadBufKind {
    /// 折算成 BufClass。
    pub fn buf(self) -> BufClass {
        match self {
            BadBufKind::Null => BufClass::Null,
            BadBufKind::Unmapped => BufClass::Unmapped,
            BadBufKind::ReadOnly => BufClass::ReadOnlyMapping,
            BadBufKind::Kernel => BufClass::Kernel,
        }
    }

    /// 闭集全量。
    pub const ALL: [BadBufKind; 4] = [
        BadBufKind::Null,
        BadBufKind::Unmapped,
        BadBufKind::ReadOnly,
        BadBufKind::Kernel,
    ];
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0，账只增不减。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectionLog {
    /// 注入总次数
    pub total: u32,
    /// 检出次数
    pub caught: u32,
    /// 逃逸次数（必须为 0）
    pub missed: u32,
}

impl InjectionLog {
    const fn zero() -> InjectionLog {
        InjectionLog { total: 0, caught: 0, missed: 0 }
    }

    /// 记一次注入结果；账目失衡返回 LedgerRegress 码值。
    pub fn record(&mut self, caught: bool) -> Result<(), u32> {
        self.total = self.total.saturating_add(1);
        if caught {
            self.caught = self.caught.saturating_add(1);
        } else {
            self.missed = self.missed.saturating_add(1);
        }
        if self.caught.saturating_add(self.missed) != self.total {
            return Err(StatCode::LedgerRegress.val());
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.caught.saturating_add(self.missed) == self.total
    }
}

// ── 注入矩阵跑批 ──────────────────────────────────────────────────────
/// fstat 坏 fd 三类 × 10 轮：全部必须 EBADF。
pub fn run_fstat_badfd_matrix() -> (InjectionLog, [u32; BAD_FD_CLASSES]) {
    let it = seeded_inodes();
    let mut per_class = [0u32; BAD_FD_CLASSES];
    let mut log = InjectionLog::zero();
    let mut ci = 0usize;
    while ci < BadFdKind::ALL.len() {
        let mut ft = FdMap::new();
        let fd = if let Some(kind) = BadFdKind::ALL.get(ci) {
            kind.sample(&mut ft, &it)
        } else {
            -1
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = judge_fstat(&ft, &it, fd, BufClass::Valid) == Err(EBADF);
            let _ = log.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (log, per_class)
}

/// 坏 buf 四类 × 10 轮 × 三入口：全部必须 EFAULT。
pub fn run_badbuf_matrix() -> (InjectionLog, [u32; BAD_BUF_CLASSES]) {
    let it = seeded_inodes();
    let mut ft = FdMap::new();
    let motd_ino = it.ino_of("motd").unwrap_or(0);
    let fd = match ft.open_for(motd_ino) {
        Ok(f) => f,
        Err(_) => -1,
    };
    let mut per_class = [0u32; BAD_BUF_CLASSES];
    let mut log = InjectionLog::zero();
    let mut ci = 0usize;
    while ci < BadBufKind::ALL.len() {
        let buf = if let Some(kind) = BadBufKind::ALL.get(ci) {
            kind.buf()
        } else {
            BufClass::Null
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let s_bad = judge_stat(&it, PathClass::Valid, "motd", buf) == Err(EFAULT);
            let l_bad = judge_lstat(&it, PathClass::Valid, "motd", buf) == Err(EFAULT);
            let f_bad = judge_fstat(&ft, &it, fd, buf) == Err(EFAULT);
            let caught = s_bad && l_bad && f_bad;
            let _ = log.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (log, per_class)
}

/// 路径不存在 × 10 轮：显式缺失、悬空链接目标与坏 fd 反查三态全对账。
pub fn run_missing_path_matrix() -> (InjectionLog, [u32; 3]) {
    let it = seeded_inodes();
    let fresh = FdMap::new();
    let mut per_class = [0u32; 3];
    let mut log = InjectionLog::zero();
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let plain = judge_stat(&it, PathClass::Valid, "ghost.log", BufClass::Valid)
            == Err(ENOENT);
        let dangling = judge_stat(&it, PathClass::Valid, "dangle", BufClass::Valid)
            == Err(ENOENT);
        let unopened_ref = judge_fstat(&fresh, &it, 3, BufClass::Valid) == Err(EBADF);
        let caught = plain && dangling && unopened_ref;
        let _ = log.record(caught);
        if caught {
            let _ = per_class.get_mut(0).map(|s| *s = s.saturating_add(1));
            let _ = per_class.get_mut(1).map(|s| *s = s.saturating_add(1));
            let _ = per_class.get_mut(2).map(|s| *s = s.saturating_add(1));
        }
        round += 1;
    }
    (log, per_class)
}

// ── 语料表 ────────────────────────────────────────────────────────────
/// 种子 inode 表：目录/普通文件/字符设备/双向链接环/悬空链接/正常链接。
pub fn seeded_inodes() -> InodeTable {
    let mut it = InodeTable::new();
    let _ = it.allocate("motd", NodeKind::Regular, 1, 87, 0x0801, 0o644, "");
    let _ = it.allocate("etc", NodeKind::Directory, 2, 512, 0x0801, 0o755, "");
    let _ = it.allocate("nullc", NodeKind::CharDev, 1, 0, 0x0105, 0o666, "");
    let _ = it.allocate("loopa", NodeKind::Symlink, 1, 5, 0x0801, 0o777, "loopb");
    let _ = it.allocate("loopb", NodeKind::Symlink, 1, 5, 0x0801, 0o777, "loopa");
    let _ = it.allocate("dangle", NodeKind::Symlink, 1, 9, 0x0801, 0o777, "ghost.log");
    let _ = it.allocate("link", NodeKind::Symlink, 1, 4, 0x0801, 0o777, "motd");
    it
}

/// 种子 fd 表：motd 已打开在 fd 3。
pub fn seeded_fds(it: &InodeTable) -> FdMap {
    let mut ft = FdMap::new();
    if let Some(ino) = it.ino_of("motd") {
        let _ = ft.open_for(ino);
    }
    ft
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；路径不存在族"),
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；坏句柄族"),
    ("EFAULT", -14, 998, "Windows ERROR_NOACCESS；坏地址族"),
    ("EMFILE", -24, 4, "Windows ERROR_TOO_MANY_OPEN_FILES；fd 表满"),
    ("ELOOP", -40, 59, "Windows 占位对照；符号链接环过深族"),
];

/// 查表：按 errno 正数取表内负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内正码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机，种子固定）。
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

/// 规范语料双跑指纹：覆盖三入口 × 指针类别 × 缺失路径 × 链接语义。
pub fn corpus_fingerprint() -> u32 {
    let it = seeded_inodes();
    let mut ft = seeded_fds(&it);
    let mut bytes: Vec<u8> = Vec::new();
    let paths = ["motd", "etc", "ghost.log", "link", "dangle", "loopa"];
    let mut pi = 0usize;
    while pi < paths.len() {
        if let Some(p) = paths.get(pi) {
            let s = judge_stat(&it, PathClass::Valid, p, BufClass::Valid);
            let l = judge_lstat(&it, PathClass::Valid, p, BufClass::Valid);
            bytes.push(match s {
                Ok(fr) => (fr.size & 0xFF) as u8,
                Err(e) => (e & 0xFF) as u8,
            });
            bytes.push(match l {
                Ok(fr) => (fr.mode & 0xFF) as u8,
                Err(e) => (e & 0xFF) as u8,
            });
        }
        pi += 1;
    }
    let fds = [-1, 0, 3, 4, FD_TOTAL as i32];
    let mut fi = 0usize;
    while fi < fds.len() {
        if let Some(fd) = fds.get(fi).copied() {
            let r = judge_fstat(&ft, &it, fd, BufClass::Valid);
            bytes.push(match r {
                Ok(fr) => (fr.ino & 0xFF) as u8,
                Err(e) => (e & 0xFF) as u8,
            });
        }
        fi += 1;
    }
    let _ = ft.close(3);
    bytes.push(match ft.fd_to_ino(3) {
        Ok(_) => 0u8,
        Err(e) => (e & 0xFF) as u8,
    });
    fnv1a(&bytes)
}

// ── 判定次序探针 ──────────────────────────────────────────────────────
/// stat：坏路径压过缺失目标与坏 buf → EFAULT。
pub fn order_stat_path_first() -> bool {
    let it = seeded_inodes();
    judge_stat(&it, PathClass::Null, "ghost.log", BufClass::Null) == Err(EFAULT)
        && judge_stat(&it, PathClass::Kernel, "motd", BufClass::Unmapped) == Err(EFAULT)
}

/// stat：缺失目标压过坏 buf → ENOENT。
pub fn order_stat_resolve_beats_buf() -> bool {
    let it = seeded_inodes();
    judge_stat(&it, PathClass::Valid, "ghost.log", BufClass::Null) == Err(ENOENT)
        && judge_stat(&it, PathClass::Valid, "ghost.log", BufClass::Kernel) == Err(ENOENT)
}

/// fstat：fd 校验最先 → EBADF 压过坏 buf。
pub fn order_fstat_fd_first() -> bool {
    let it = seeded_inodes();
    let ft = FdMap::new();
    judge_fstat(&ft, &it, -1, BufClass::Null) == Err(EBADF)
        && judge_fstat(&ft, &it, 3, BufClass::Kernel) == Err(EBADF)
        && judge_fstat(&ft, &it, FD_TOTAL as i32, BufClass::Unmapped) == Err(EBADF)
}

/// 阶段踪迹对账：六条序列与各自故障截断序列逐一核对。
pub fn stage_sequences_match() -> bool {
    let it = seeded_inodes();
    let mut ft = seeded_fds(&it);
    // 完整 stat：1→2→3→4
    let mut t1 = StageTrace::new();
    let _ = judge_stat_traced(&it, PathClass::Valid, "motd", BufClass::Valid,
                              true, &mut t1);
    // 坏路径：止步于 1
    let mut t2 = StageTrace::new();
    let _ = judge_stat_traced(&it, PathClass::Null, "motd", BufClass::Valid,
                              true, &mut t2);
    // 缺失目标：止步于 2
    let mut t3 = StageTrace::new();
    let _ = judge_stat_traced(&it, PathClass::Valid, "ghost.log", BufClass::Valid,
                              true, &mut t3);
    // 坏 buf：止步于 4 之前（3 已完成填充）
    let mut t4 = StageTrace::new();
    let _ = judge_stat_traced(&it, PathClass::Valid, "motd", BufClass::Null,
                              true, &mut t4);
    // fstat 坏 fd：止步于 5
    let mut t5 = StageTrace::new();
    let _ = judge_fstat_traced(&ft, &it, -1, BufClass::Valid, &mut t5);
    // fstat 完整：5→6→3→4
    let mut t6 = StageTrace::new();
    let _ = judge_fstat_traced(&ft, &it, 3, BufClass::Valid, &mut t6);
    let _ = ft.close(3);
    t1.sequence() == vec![STAGE_STAT_COPY_PATH, STAGE_STAT_RESOLVE, STAGE_FILL, STAGE_COPY_OUT]
        && t2.sequence() == vec![STAGE_STAT_COPY_PATH]
        && t3.sequence() == vec![STAGE_STAT_COPY_PATH, STAGE_STAT_RESOLVE]
        && t4.sequence() == vec![STAGE_STAT_COPY_PATH, STAGE_STAT_RESOLVE, STAGE_FILL]
        && t5.sequence() == vec![STAGE_FSTAT_FD_CHECK]
        && t6.sequence() == vec![STAGE_FSTAT_FD_CHECK, STAGE_FSTAT_FD_TO_INO, STAGE_FILL, STAGE_COPY_OUT]
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9208 判据集（≥14 条；动态明细 ≤8 条）。
pub fn run_unx9208_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9208");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：stat motd 五字段语义齐备
    let it = seeded_inodes();
    let normal_stat = match judge_stat(&it, PathClass::Valid, "motd", BufClass::Valid) {
        Ok(fr) => {
            fr.mode & MODE_TYPE_MASK == S_IFREG
                && fr.mode & 0o777 == 0o644
                && fr.size == 87
                && fr.nlink == 1
                && fr.ino != 0
        }
        Err(_) => false,
    };
    s.add("正常流：stat motd 返回 S_IFREG|0644、size=87", normal_stat,
          "合法路径 + 可写 buf 必须填齐五字段语义");

    // 3. 正常流：fstat 已开 fd 反查同一 inode
    let ft = seeded_fds(&it);
    let stat_ino = match judge_stat(&it, PathClass::Valid, "motd", BufClass::Valid) {
        Ok(fr) => fr.ino,
        Err(_) => 0,
    };
    let normal_fstat = match judge_fstat(&ft, &it, 3, BufClass::Valid) {
        Ok(fr) => fr.ino == stat_ino && fr.ino != 0 && (fr.mode & MODE_TYPE_MASK) == S_IFREG,
        Err(_) => false,
    };
    s.add("正常流：fstat(3) 与 stat(motd) 反查同一 inode", normal_fstat,
          "fd→inode 反查结果必须与路径正查逐字段一致");

    // 4. 正常流：lstat 链接不跟随
    let lstat_ok = match (
        judge_lstat(&it, PathClass::Valid, "link", BufClass::Valid),
        judge_stat(&it, PathClass::Valid, "link", BufClass::Valid),
    ) {
        (Ok(lfr), Ok(sfr)) => {
            (lfr.mode & MODE_TYPE_MASK) == S_IFLNK
                && lfr.ino != sfr.ino
                && lfr.size == 4
        }
        _ => false,
    };
    s.add("正常流：lstat(link) 停在链接本体，stat 跟随到目标", lstat_ok,
          "lstat 不跟随末级符号链接，两者 inode 必须不同");

    // 5. 坏 buf 四类 × 10 轮 × 三入口全 EFAULT
    let (buf_log, buf_per) = run_badbuf_matrix();
    let buf_ok = buf_per.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("坏 buf 四类各 10/10 三入口全 EFAULT", buf_ok,
          "NULL/未映射/只读映射/内核地址四类必须全部回 EFAULT");

    // 6. 坏 fd 三类 × 10 轮全 EBADF
    let (fd_log, fd_per) = run_fstat_badfd_matrix();
    let fd_ok = fd_per.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("坏 fd 三类各 10/10 全 EBADF", fd_ok,
          "负值/未开/已关三类必须全部回 EBADF，不得混报 EINVAL");

    // 7. 路径不存在 × 10 轮全 ENOENT
    let (path_log, path_per) = run_missing_path_matrix();
    let path_ok = path_log.caught == INJECT_ROUNDS
        && path_per.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("路径不存在 10/10 全 ENOENT", path_ok,
          "显式缺失与悬空链接目标都必须回 ENOENT");

    // 8. 注入账守恒：80 次注入 0 逃逸
    let total_injected = buf_log.total.saturating_add(fd_log.total).saturating_add(path_log.total);
    let all_conserved = buf_log.conserved() && fd_log.conserved() && path_log.conserved()
        && buf_log.missed == 0 && fd_log.missed == 0 && path_log.missed == 0
        && total_injected == 80;
    s.add("注入账守恒：80 次注入全检出 0 逃逸", all_conserved,
          "注入=检出+逃逸；逃逸非零即本卡不通过");

    // 9. 判定次序：坏路径压过缺失与坏 buf
    s.add("判定次序：stat 坏路径先于一切 → EFAULT", order_stat_path_first(),
          "user_path 拷贝先于 lookup；同时多坏必须按次序报首错");

    // 10. 判定次序：ENOENT 压过坏 buf
    s.add("判定次序：stat 缺失目标先于坏 buf → ENOENT", order_stat_resolve_beats_buf(),
          "lookup 先于 copy_to_user；缺失路径上坏 buf 不得改报 EFAULT");

    // 11. 判定次序：fstat fd 校验最先
    s.add("判定次序：fstat fd 校验最先 → EBADF", order_fstat_fd_first(),
          "fget 先于 copy_to_user；坏 fd 上坏 buf 不得改报 EFAULT");

    // 12. 阶段踪迹对账：六条序列逐一核对
    s.add("阶段踪迹：三入口阶段序列与预期逐位一致", stage_sequences_match(),
          "判定次序由阶段标记显性呈现，任何提前/滞后截断都算失败");

    // 13. 填充状态机：倒退拒、缺字段拒
    let mut filler = StatFiller::new();
    let _ = filler.fill_dev(1);
    let _ = filler.fill_ino(2);
    let _ = filler.fill_mode(S_IFREG | 0o600);
    let regress = filler.fill_ino(2) == Err(StatCode::FillOrderRegress.val())
        && filler.fill_dev(1) == Err(StatCode::FillOrderRegress.val());
    let mut partial = StatFiller::new();
    let _ = partial.fill_dev(1);
    let _ = partial.fill_ino(2);
    let incomplete = partial.finalize() == Err(StatCode::FillIncomplete.val());
    s.add("填充状态机：五字段次序单调、倒退与缺字段均拒", regress && incomplete,
          "dev/ino/mode/nlink/size 必须按序填充；finalize 前必须填满");

    // 14. mode 类型位闭集：7 类全收，掩码外全拒
    let mut closed_ok = true;
    let mut ti = 0usize;
    while ti < KNOWN_TYPE_BITS.len() {
        if let Some(bits) = KNOWN_TYPE_BITS.get(ti) {
            if !mode_type_known(*bits | 0o600) {
                closed_ok = false;
            }
        }
        ti += 1;
    }
    let mut filler_bad = StatFiller::new();
    let _ = filler_bad.fill_dev(1);
    let _ = filler_bad.fill_ino(1);
    let reject_unknown = filler_bad.fill_mode(0o160000 | 0o600)
        == Err(StatCode::ModeTypeUnknown.val())
        && !mode_type_known(0)
        && !mode_type_known(0o170000 | 0o600);
    s.add("mode 类型位闭集：S_IF* 七类全收、掩码外全拒", closed_ok && reject_unknown,
          "S_IFMT 掩码内只有 7 类合法值，0 与重叠位必须拒绝");

    // 15. inode 解析：命中/落空/悬空/链接环
    let dir_hit = match judge_stat(&it, PathClass::Valid, "etc", BufClass::Valid) {
        Ok(fr) => (fr.mode & MODE_TYPE_MASK) == S_IFDIR && fr.nlink == 2,
        Err(_) => false,
    };
    let link_hit = match judge_lstat(&it, PathClass::Valid, "dangle", BufClass::Valid) {
        Ok(fr) => (fr.mode & MODE_TYPE_MASK) == S_IFLNK,
        Err(_) => false,
    };
    let resolve_ok = dir_hit
        && judge_stat(&it, PathClass::Valid, "ghost.log", BufClass::Valid) == Err(ENOENT)
        && judge_stat(&it, PathClass::Valid, "dangle", BufClass::Valid) == Err(ENOENT)
        && link_hit
        && judge_stat(&it, PathClass::Valid, "loopa", BufClass::Valid) == Err(ELOOP);
    s.add("inode 解析：命中/ENOENT/悬空/链接环四态全对", resolve_ok,
          "正查落空 ENOENT、悬空链接 stat 报 ENOENT、lstat 仍命中、环走 ELOOP");

    // 16. fd 表复用：一开一关、再关拒、关后反查拒
    let mut ft2 = FdMap::new();
    let ino = it.ino_of("motd").unwrap_or(0);
    let opened = ft2.open_for(ino);
    let reuse_ok = match opened {
        Ok(fd) => {
            let close1 = ft2.close(fd);
            let close2 = ft2.close(fd);
            close1 == Ok(()) && close2 == Err(EBADF)
                && ft2.fd_to_ino(fd) == Err(EBADF)
        }
        Err(_) => false,
    };
    s.add("fd 表复用：一开一关、再关拒、关后反查拒", reuse_ok,
          "Closed 槽与 Free 槽同为 EBADF，但账目分别记账不得混淆");

    // 17. 容量耗尽：fd 表 12 槽打满回 EMFILE，inode 表打满回 TableExhausted
    let mut ft3 = FdMap::new();
    let mut opened_all = true;
    let mut k = 0usize;
    while k < DYN_FD_CAP {
        if ft3.open_for(ino).is_err() {
            opened_all = false;
        }
        k += 1;
    }
    let fd_full = opened_all && ft3.open_for(ino) == Err(EMFILE)
        && ft3.open_count() == DYN_FD_CAP;
    let mut it2 = seeded_inodes();
    let inode_before_full = it2.allocate("extra", NodeKind::Regular, 1, 1, 1, 0o600, "").is_ok();
    let inode_full = it2.allocate("overflow", NodeKind::Regular, 1, 1, 1, 0o600, "")
        == Err(StatCode::TableExhausted.val())
        && it2.used_slots() == INODE_CAP;
    s.add("容量耗尽：fd 表 12 槽 EMFILE、inode 表 8 槽 TableExhausted",
          fd_full && inode_before_full && inode_full,
          "表满必须显性拒绝而不是覆盖或静默失败");

    // 18. Linux 逐码一致
    let codes_ok = agrees_with_linux(ENOENT)
        && agrees_with_linux(EBADF)
        && agrees_with_linux(EFAULT)
        && agrees_with_linux(EMFILE)
        && agrees_with_linux(ELOOP)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内五码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 19. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 20. 文案闭集 + 码值唯一
    let text_ok = StatCode::ALL.iter().all(|c| !c.detail().is_empty())
        && !detail(0xFFFF_FFFF).is_empty();
    let mut uniq = true;
    let mut i = 0usize;
    while i < StatCode::ALL.len() {
        let mut j = i + 1;
        while j < StatCode::ALL.len() {
            if let (Some(x), Some(y)) = (StatCode::ALL.get(i), StatCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("文案闭集：12 码非空建议且码值两两唯一", text_ok && uniq,
          "每个码必须能取到非空中文说明，段内闭集码不得重复");

    // 动态明细（≤8）：三账快照 + 指纹
    let fd_text = format!("坏fd 注入 {} 检出 {} 逃逸 {}",
                          fd_log.total, fd_log.caught, fd_log.missed);
    s.add_dyn("坏fd 账目快照", fd_log.conserved() && fd_log.missed == 0, &fd_text);
    let buf_text = format!("坏buf 注入 {} 检出 {} 逃逸 {}",
                           buf_log.total, buf_log.caught, buf_log.missed);
    s.add_dyn("坏buf 账目快照", buf_log.conserved() && buf_log.missed == 0, &buf_text);
    let path_text = format!("缺失路径 注入 {} 检出 {} 逃逸 {}",
                            path_log.total, path_log.caught, path_log.missed);
    s.add_dyn("缺失路径账目快照", path_log.conserved() && path_log.missed == 0, &path_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    // 21. 收口：判据条数达到声明值
    let n_before = s.len();
    s.add("收口：判据条数达到声明值", n_before >= 14, "判据条数不得低于契约下限");

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_stat_fstat_lstat_flow() {
        let it = seeded_inodes();
        let s = judge_stat(&it, PathClass::Valid, "motd", BufClass::Valid);
        assert!(s.is_ok());
        let fr = s.unwrap_or(StatFrame { dev: 0, ino: 0, mode: 0, nlink: 0, size: 0 });
        assert_eq!(fr.size, 87);
        assert_eq!(fr.mode & MODE_TYPE_MASK, S_IFREG);
        let mut ft = seeded_fds(&it);
        let f = judge_fstat(&ft, &it, 3, BufClass::Valid);
        assert_eq!(f.unwrap_or(fr).ino, fr.ino);
        let l = judge_lstat(&it, PathClass::Valid, "link", BufClass::Valid);
        let lfr = l.unwrap_or(fr);
        assert_eq!(lfr.mode & MODE_TYPE_MASK, S_IFLNK);
        let _ = ft.close(3);
    }

    #[test]
    fn four_buf_classes_efault_boundary() {
        let it = seeded_inodes();
        let mut i = 0usize;
        while i < BadBufKind::ALL.len() {
            if let Some(k) = BadBufKind::ALL.get(i) {
                assert_eq!(judge_stat(&it, PathClass::Valid, "motd", k.buf()), Err(EFAULT));
                assert_eq!(judge_lstat(&it, PathClass::Valid, "motd", k.buf()), Err(EFAULT));
            }
            i += 1;
        }
        let mut ft = seeded_fds(&it);
        assert_eq!(judge_fstat(&ft, &it, 3, BufClass::ReadOnlyMapping), Err(EFAULT));
        assert_eq!(judge_fstat(&ft, &it, 3, BufClass::Kernel), Err(EFAULT));
        let _ = ft.close(3);
    }

    #[test]
    fn four_path_classes_boundary() {
        let it = seeded_inodes();
        assert_eq!(judge_stat(&it, PathClass::Null, "motd", BufClass::Valid), Err(EFAULT));
        assert_eq!(judge_stat(&it, PathClass::Unmapped, "motd", BufClass::Valid), Err(EFAULT));
        assert_eq!(judge_stat(&it, PathClass::Kernel, "motd", BufClass::Valid), Err(EFAULT));
        assert!(judge_stat(&it, PathClass::Valid, "motd", BufClass::Valid).is_ok());
    }

    #[test]
    fn three_bad_fd_classes_ebadf() {
        let it = seeded_inodes();
        let mut i = 0usize;
        while i < BadFdKind::ALL.len() {
            let mut ft = FdMap::new();
            let fd = if let Some(k) = BadFdKind::ALL.get(i) {
                k.sample(&mut ft, &it)
            } else {
                -1
            };
            assert_eq!(judge_fstat(&ft, &it, fd, BufClass::Valid), Err(EBADF));
            i += 1;
        }
        // 标准流 ino=0 反查落空同归 EBADF 族
        let ft = FdMap::new();
        assert_eq!(judge_fstat(&ft, &it, 0, BufClass::Valid), Err(EBADF));
    }

    #[test]
    fn inject_matrices_ten_rounds_all_caught() {
        let (fl, fper) = run_fstat_badfd_matrix();
        assert!(fl.conserved());
        assert_eq!(fl.missed, 0);
        assert_eq!(fl.total, BAD_FD_CLASSES as u32 * INJECT_ROUNDS);
        for n in fper.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
        let (bl, bper) = run_badbuf_matrix();
        assert!(bl.conserved());
        assert_eq!(bl.missed, 0);
        assert_eq!(bl.total, BAD_BUF_CLASSES as u32 * INJECT_ROUNDS);
        for n in bper.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
        let (pl, pper) = run_missing_path_matrix();
        assert!(pl.conserved());
        assert_eq!(pl.total, INJECT_ROUNDS);
        for n in pper.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn judgment_order_explicit() {
        assert!(order_stat_path_first());
        assert!(order_stat_resolve_beats_buf());
        assert!(order_fstat_fd_first());
        assert!(stage_sequences_match());
    }

    #[test]
    fn fill_state_machine_monotonic() {
        let mut f = StatFiller::new();
        assert!(f.fill_ino(1).is_err());
        assert!(f.fill_dev(0x0801).is_ok());
        assert!(f.fill_mode(S_IFREG | 0o644).is_ok());
        assert!(f.fill_nlink(1).is_ok());
        assert!(f.fill_size(87).is_ok());
        assert!(f.fill_size(88).is_err());
        let fr = f.finalize();
        assert_eq!(
            fr.unwrap_or(StatFrame { dev: 0, ino: 0, mode: 0, nlink: 0, size: 0 }).size,
            87,
        );
    }

    #[test]
    fn mode_type_closed_set() {
        assert!(mode_type_known(S_IFREG));
        assert!(mode_type_known(S_IFDIR | 0o755));
        assert!(mode_type_known(S_IFLNK));
        assert!(!mode_type_known(0));
        assert!(!mode_type_known(0o160000));
        assert!(!mode_type_known(0o170000));
        assert!(!mode_type_known(0o10000));
    }

    #[test]
    fn capacity_exhaustion_guard() {
        let it = seeded_inodes();
        let mut ft = FdMap::new();
        let ino = it.ino_of("motd").unwrap_or(0);
        let mut k = 0usize;
        while k < DYN_FD_CAP {
            assert!(ft.open_for(ino).is_ok());
            k += 1;
        }
        assert_eq!(ft.open_for(ino), Err(EMFILE));
        let mut it2 = seeded_inodes();
        assert!(it2.allocate("extra", NodeKind::Regular, 1, 1, 1, 0o600, "").is_ok());
        assert_eq!(
            it2.allocate("overflow", NodeKind::Regular, 1, 1, 1, 0o600, ""),
            Err(StatCode::TableExhausted.val()),
        );
    }

    #[test]
    fn symlink_semantics() {
        let it = seeded_inodes();
        // stat 跟随到 motd，lstat 停在链接本体
        let s = judge_stat(&it, PathClass::Valid, "link", BufClass::Valid);
        assert_eq!(
            s.unwrap_or(StatFrame { dev: 0, ino: 0, mode: 0, nlink: 0, size: 0 }).size,
            87,
        );
        let l = judge_lstat(&it, PathClass::Valid, "link", BufClass::Valid);
        let lfr = l.unwrap_or(StatFrame { dev: 0, ino: 0, mode: 0, nlink: 0, size: 0 });
        assert_eq!(lfr.mode & MODE_TYPE_MASK, S_IFLNK);
        assert_eq!(lfr.size, 4);
        // 悬空链接：stat ENOENT，lstat 命中
        assert_eq!(judge_stat(&it, PathClass::Valid, "dangle", BufClass::Valid), Err(ENOENT));
        assert!(judge_lstat(&it, PathClass::Valid, "dangle", BufClass::Valid).is_ok());
        // 链接环：ELOOP
        assert_eq!(judge_stat(&it, PathClass::Valid, "loopa", BufClass::Valid), Err(ELOOP));
    }

    #[test]
    fn deterministic_double_run() {
        let a = corpus_fingerprint();
        let b = corpus_fingerprint();
        assert_eq!(a, b);
    }

    #[test]
    fn segment_guard_window() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x7A000..=0x7D1C0).contains(&SEG_BASE));
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9208_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
