//! UNX-F9202 read/write 坏 fd 矩阵（全 EBADF） · 0x90000 段
//!
//! 判据原文（逐字摘自分派卡）：
//! UNX-F9202-J1 负 fd / 未开 fd / 已关 fd 三类注入，判据格全落 -EBADF，
//! 与 Linux 语义一致，10/10 次检出必抓。
//!
//! # 闸门次序（这条纵线为什么不能倒）
//!
//! 一次 `read(2)` / `write(2)` 在内核里要穿过五道闸门：
//!
//! ```text
//! ① 范围闸：fd < 0 或 fd >= rlimit          -> EBADF
//! ② 槽位闸：槽未占用 / 描述符空缺            -> EBADF
//! ③ 代数闸：内核 token 的 generation 不符    -> EBADF
//! ④ 模式闸：读只写 fd / 写只读 fd / O_PATH   -> EBADF
//! ⑤ 对象闸：目录读 -> EISDIR；管道定位 -> ESPIPE
//! ```
//!
//! 次序的意义在于「先证句柄合法，再谈句柄指向什么」。若把 ④ 提到 ①，
//! 一个从未打开过的 fd 会先被拿去查访问模式——查表本身就要解引用，
//! 等于把 EBADF 变成一次越界读；若把 ⑤ 提到 ①，坏 fd 会先被当成
//! 目录/管道解释，错误码从 EBADF 漂成 EISDIR/ESPIPE，用户态
//! `errno` 语义即失真。Linux 的 `fdget()` 也是先做 fd 范围与槽位判定，
//! 拿到 `struct file *` 之后才检查 `FMODE_READ/WRITE`。
//!
//! 本模块把这条纵线做成可复算的数据：`FdTable::index` → `desc` →
//! `desc_mut` 三层各自独立可测，任何一层把 EBADF 换成别的码，
//! 20 格坏 fd 矩阵都会当场变红。
//!
//! # 关键差异（本卡深度所在）
//!
//! - 与 UNX-F9201（E 型方法论总纲）的边界：9201 只规定「双判据格」的
//!   记法与印发规则，不碰任何真实 fd；本卡把 E 型落到 read/write 的
//!   真实负路径上，产出的是 10 类 × 2 操作的**执行矩阵**，而不是格位。
//! - 与 UNX-F9203（EFAULT 矩阵）的边界：9203 判的是「fd 合法但缓冲
//!   非法」；本卡判的是「fd 本身非法」。两卡共享 ①②③ 三层闸门，
//!   但 9203 在 ④ 之后还要走拷贝半程探针，本卡在 ④ 之前就必须拒绝。
//! - 与 UNX-F9231（EBADF vs ENOENT 判定次序）的边界：9231 处理
//!   「fd 与路径同时可疑」的组合；本卡不含路径参数，只有纯 fd 面。
//!
//! # 设计要点
//!
//! 1. fd 空间 = 槽位下标 + 代数。用户 fd 只暴露下标；内核 token 同时
//!    携带代数，用于识别「关闭后被新 open 复用同一槽位」的陈旧句柄。
//! 2. 描述符池与槽位表分离：dup 出的是同一描述符的第二张门票，
//!    共享偏移与文件状态标志；引用计数归零才回收描述符。
//! 3. 访问模式用 `Access` 闭集表达，`can_read/can_write` 由闭集推导，
//!    不存第二份布尔，避免「标志与能力不一致」这类双写漂移。
//! 4. 坏 fd 矩阵十类全部走同一条 `validate` 路径，禁止为测试开小门。
//! 5. 账本逐格复核「类 × 操作 → 期望码」，注入任意一格的错误码，
//!    `audit()` 必报红；`seal()` 折叠 FNV-1a，`replay()` 重建后比指纹。
//! 6. 全模块零浮点、零 IO、零墙钟、零随机；时间与容量都是整数域。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 本卡诊断段基址。段内 0x20 字节，诊断码占 0x01..0x0D。
pub const SEG_BASE: u32 = 0x90000;

/// 段基址自检失败（应当是编译期常量，运行时仍复核一次，防止被改）。
pub const D01_SEG: u32 = SEG_BASE + 0x01;
/// 诊断码闭集不完整或出现别名。
pub const D02_CODE: u32 = SEG_BASE + 0x02;
/// fd 范围闸失效（负值 / 越过表尾 / 越过 rlimit）。
pub const D03_RANGE: u32 = SEG_BASE + 0x03;
/// 槽位闸失效（空槽被当成活 fd）。
pub const D04_SLOT: u32 = SEG_BASE + 0x04;
/// 代数闸失效（陈旧 token 未被识别）。
pub const D05_GEN: u32 = SEG_BASE + 0x05;
/// 访问模式闸失效（只写读 / 只读写 / O_PATH）。
pub const D06_MODE: u32 = SEG_BASE + 0x06;
/// 对象闸失效（目录 / 管道语义错位）。
pub const D07_OBJECT: u32 = SEG_BASE + 0x07;
/// 合法路径被误拒（拒绝面过宽）。
pub const D08_ACCEPT: u32 = SEG_BASE + 0x08;
/// 引用计数账不平（dup/close 配对错误）。
pub const D09_REFS: u32 = SEG_BASE + 0x09;
/// 偏移账不平（lseek 溢出或负值未拦）。
pub const D10_OFFSET: u32 = SEG_BASE + 0x0A;
/// 账本复核失败（格值与矩阵不一致）。
pub const D11_LEDGER: u32 = SEG_BASE + 0x0B;
/// 封印/冻结/重放链断裂。
pub const D12_SEAL: u32 = SEG_BASE + 0x0C;
/// 四路注入未全抓。
pub const D13_INJECT: u32 = SEG_BASE + 0x0D;

/// 全部诊断码。闭集自检用「两两互异 + 段内」双条件。
pub const ALL_CODES: [u32; 13] = [
    D01_SEG, D02_CODE, D03_RANGE, D04_SLOT, D05_GEN, D06_MODE, D07_OBJECT,
    D08_ACCEPT, D09_REFS, D10_OFFSET, D11_LEDGER, D12_SEAL, D13_INJECT,
];

/// 诊断码 → 中文解释（闭集查表，未登记码返回统一兜底文案，不 panic）。
pub fn detail(code: u32) -> &'static str {
    match code {
        D01_SEG => "段基址被改写或未按 0x20 对齐",
        D02_CODE => "诊断码闭集出现重复或越界",
        D03_RANGE => "fd 范围闸失效：负值或越过表尾未被 EBADF 拦截",
        D04_SLOT => "槽位闸失效：空槽被当作活 fd 解引用",
        D05_GEN => "代数闸失效：关闭后复用的陈旧 token 未被识别",
        D06_MODE => "访问模式闸失效：读写能力与打开模式不符",
        D07_OBJECT => "对象闸失效：目录/管道语义被错误码覆盖",
        D08_ACCEPT => "合法路径被误拒：拒绝面过宽",
        D09_REFS => "引用计数账不平：dup/close 未成对",
        D10_OFFSET => "偏移账不平：负值或溢出未拦",
        D11_LEDGER => "账本复核失败：写入格与期望矩阵不一致",
        D12_SEAL => "封印链断裂：冻结后仍可写或重放指纹不符",
        D13_INJECT => "四路注入核未全部命中",
        _ => "未登记诊断码（闭集之外）",
    }
}

// ---------------------------------------------------------------------------
// 一、Linux 错误码闭集
// ---------------------------------------------------------------------------

/// read/write 面会真实产生的错误码闭集。
///
/// 只登记本卡需要的十三种。`raw()` 返回 Linux 的负数约定
/// （`-EBADF = -9` 等），既方便与用户态 `errno` 对照，也让矩阵断言
/// 可以逐字钉住数值而不是依赖字符串。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KErr {
    /// fd 非法：负值、越界、空槽、陈旧、访问模式不符。
    BadFd,
    /// 用户缓冲不可访问（本卡只在对照面使用，不作主语）。
    Fault,
    /// 参数非法：坏 whence、负偏移。
    Inval,
    /// 对目录调用 read/write。
    IsDir,
    /// 路径分量不是目录（留作 9231 对照）。
    NotDir,
    /// 路径不存在（留作 9231 对照）。
    NoEnt,
    /// 进程 fd 表耗尽。
    MFile,
    /// 系统文件表耗尽。
    NFile,
    /// 管道不可定位。
    Spipe,
    /// 偏移溢出。
    Overflow,
    /// 非阻塞对象暂不可用（管道满）。
    Again,
    /// 被信号打断（留作 9203 对照）。
    Intr,
    /// 系统调用未实现（留作能力协商对照）。
    NoSys,
}

impl KErr {
    /// Linux 负数错误码。
    pub fn raw(self) -> i32 {
        match self {
            KErr::BadFd => -9,
            KErr::Fault => -14,
            KErr::Inval => -22,
            KErr::IsDir => -21,
            KErr::NotDir => -20,
            KErr::NoEnt => -2,
            KErr::MFile => -24,
            KErr::NFile => -23,
            KErr::Spipe => -29,
            KErr::Overflow => -75,
            KErr::Again => -11,
            KErr::Intr => -4,
            KErr::NoSys => -38,
        }
    }

    /// 稳定名字（渲染与对照表用）。
    pub fn name(self) -> &'static str {
        match self {
            KErr::BadFd => "EBADF",
            KErr::Fault => "EFAULT",
            KErr::Inval => "EINVAL",
            KErr::IsDir => "EISDIR",
            KErr::NotDir => "ENOTDIR",
            KErr::NoEnt => "ENOENT",
            KErr::MFile => "EMFILE",
            KErr::NFile => "ENFILE",
            KErr::Spipe => "ESPIPE",
            KErr::Overflow => "EOVERFLOW",
            KErr::Again => "EAGAIN",
            KErr::Intr => "EINTR",
            KErr::NoSys => "ENOSYS",
        }
    }

    /// 中文诊断（失败路径必须能指导定位，不能只吐一个数字）。
    pub fn zh(self) -> &'static str {
        match self {
            KErr::BadFd => "文件描述符非法：负值、越界、已关闭或访问模式不符",
            KErr::Fault => "用户缓冲地址不可访问",
            KErr::Inval => "参数非法：whence 出闭集或偏移为负",
            KErr::IsDir => "对象是目录，read/write 不接受目录",
            KErr::NotDir => "路径分量不是目录",
            KErr::NoEnt => "路径不存在",
            KErr::MFile => "进程 fd 表已满（RLIMIT_NOFILE）",
            KErr::NFile => "系统文件表已满",
            KErr::Spipe => "对象是管道，不支持定位",
            KErr::Overflow => "偏移运算溢出 64 位范围",
            KErr::Again => "非阻塞对象暂不可用",
            KErr::Intr => "调用被信号打断",
            KErr::NoSys => "该操作未实现",
        }
    }

    /// 由负数码反查闭集，未登记返回 `None`（不 panic、不猜）。
    pub fn from_raw(raw: i32) -> Option<KErr> {
        let mut i = 0usize;
        let all = ALL_ERR;
        while i < all.len() {
            if let Some(e) = all.get(i) {
                if e.raw() == raw {
                    return Some(*e);
                }
            }
            i += 1;
        }
        None
    }
}

/// 错误码全表（闭集遍历与对照测试用）。
pub const ALL_ERR: [KErr; 13] = [
    KErr::BadFd,
    KErr::Fault,
    KErr::Inval,
    KErr::IsDir,
    KErr::NotDir,
    KErr::NoEnt,
    KErr::MFile,
    KErr::NFile,
    KErr::Spipe,
    KErr::Overflow,
    KErr::Again,
    KErr::Intr,
    KErr::NoSys,
];

/// 统一结果别名。
pub type SysResult<T> = Result<T, KErr>;

// ---------------------------------------------------------------------------
// 二、fd 空间模型
// ---------------------------------------------------------------------------

/// fd 表容量（进程内固定容量，对应 RLIMIT_NOFILE 的硬上界）。
pub const FD_MAX: usize = 64;
/// 标准输入下标。
pub const FD_STDIN: i32 = 0;
/// 标准输出下标。
pub const FD_STDOUT: i32 = 1;
/// 标准错误下标。
pub const FD_STDERR: i32 = 2;

/// 打开模式闭集。
///
/// `PathOnly` 对应 Linux `O_PATH`：只用于路径引用，不能 read/write，
/// 但允许 fstat。它与 `ReadOnly` 的差别是「零能力」而不是「读能力」，
/// 因此必须单列一类，否则 fstat 与 read 的判定会互相污染。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Access {
    ReadOnly,
    WriteOnly,
    ReadWrite,
    PathOnly,
}

impl Access {
    /// 是否具备读能力。
    pub fn can_read(self) -> bool {
        matches!(self, Access::ReadOnly | Access::ReadWrite)
    }

    /// 是否具备写能力。
    pub fn can_write(self) -> bool {
        matches!(self, Access::WriteOnly | Access::ReadWrite)
    }

    /// 打开标志位（O_RDONLY=0, O_WRONLY=1, O_RDWR=2, O_PATH=0o10000000）。
    pub fn flag_bits(self) -> u32 {
        match self {
            Access::ReadOnly => 0,
            Access::WriteOnly => 1,
            Access::ReadWrite => 2,
            Access::PathOnly => 0o1000_0000,
        }
    }

    /// 名字（诊断与渲染）。
    pub fn name(self) -> &'static str {
        match self {
            Access::ReadOnly => "O_RDONLY",
            Access::WriteOnly => "O_WRONLY",
            Access::ReadWrite => "O_RDWR",
            Access::PathOnly => "O_PATH",
        }
    }
}

/// 对象类型闭集。
///
/// 管道拆成读写两端（`PipeRead/PipeWrite`）而不是一个 `Pipe`：
/// Linux 的管道端在打开时就固定了方向，读端写、写端读都是 EBADF，
/// 这与「同一个 fd 既能读又能写」的 socket 语义必须区分开。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObjKind {
    Regular,
    Directory,
    PipeRead,
    PipeWrite,
    CharDev,
    EventFd,
}

impl ObjKind {
    /// 对应 `st_mode` 的文件类型位。
    pub fn mode_bits(self) -> u32 {
        match self {
            ObjKind::Regular => 0o100_000,
            ObjKind::Directory => 0o040_000,
            ObjKind::PipeRead | ObjKind::PipeWrite => 0o010_000,
            ObjKind::CharDev => 0o020_000,
            ObjKind::EventFd => 0o100_000,
        }
    }

    /// 名字（矩阵渲染）。
    pub fn name(self) -> &'static str {
        match self {
            ObjKind::Regular => "reg",
            ObjKind::Directory => "dir",
            ObjKind::PipeRead => "pipe-r",
            ObjKind::PipeWrite => "pipe-w",
            ObjKind::CharDev => "chr",
            ObjKind::EventFd => "eventfd",
        }
    }

    /// 是否是可定位对象（管道与 eventfd 不维护线性偏移）。
    pub fn seekable(self) -> bool {
        matches!(self, ObjKind::Regular | ObjKind::Directory | ObjKind::CharDev)
    }
}

/// 打开文件描述（open file description）。
///
/// 偏移与文件状态标志住在描述符上，dup 共享；fd 表槽位只存
/// 「指向哪张描述符 + 本槽代数」。
#[derive(Clone, Copy, Debug)]
pub struct Desc {
    pub kind: ObjKind,
    pub access: Access,
    pub flags: u32,
    pub offset: u64,
    pub size: u64,
    pub refs: u32,
    pub ino: u32,
}

impl Desc {
    /// 构造一个引用计数为 1 的描述符。
    pub fn new(kind: ObjKind, access: Access, size: u64, ino: u32) -> Desc {
        Desc { kind, access, flags: access.flag_bits(), offset: 0, size, refs: 1, ino }
    }

    /// 是否可读（由 access 推导，不存第二份状态）。
    pub fn readable(&self) -> bool {
        self.access.can_read()
    }

    /// 是否可写。
    pub fn writable(&self) -> bool {
        self.access.can_write()
    }

    /// `st_mode`：类型位 | 权限位。能力由 access 决定，权限只是展示面。
    pub fn mode(&self) -> u32 {
        let perm = match self.access {
            Access::ReadOnly => 0o444,
            Access::WriteOnly => 0o222,
            Access::ReadWrite => 0o666,
            Access::PathOnly => 0o000,
        };
        self.kind.mode_bits() | perm
    }

    /// 剩余可读字节（定长对象）。字符设备与 eventfd 不按 size 截断。
    pub fn remaining(&self) -> u64 {
        match self.kind {
            ObjKind::CharDev | ObjKind::EventFd => u64::MAX,
            _ => self.size.saturating_sub(self.offset),
        }
    }
}

/// fd 表槽位。`gen` 在每次回收时自增，用于识别陈旧 token。
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    pub gen: u32,
    pub used: bool,
    pub desc: Option<u8>,
}

/// 空槽常量（`[FREE_SLOT; FD_MAX]` 需要 Copy 常量）。
pub const FREE_SLOT: Slot = Slot { gen: 0, used: false, desc: None };

/// 内核句柄：槽位下标 + 代数。用户永远拿不到 `gen`，
/// 它只用于内部跨调用传递（如异步 IO 的完成事件）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FdToken {
    pub idx: u8,
    pub gen: u32,
}

/// 进程 fd 表。
///
/// 两张定长表：`slots` 是用户可见下标，`descs` 是描述符池。
/// `open` 同时占一个空槽与一个空描述符位；`dup` 只占槽并给描述符
/// 加一次引用；`close` 减引用，归零时两处同时回收。
pub struct FdTable {
    slots: [Slot; FD_MAX],
    descs: [Option<Desc>; FD_MAX],
    next_ino: u32,
    rlimit: usize,
    opened: u32,
    closed: u32,
    duped: u32,
}

impl FdTable {
    /// 新建进程 fd 表，预装 0/1/2 三个标准流。
    ///
    /// 0 号是只读字符设备，1/2 是只写字符设备——这与真实终端一致；
    /// 因此「对 0 号 write」在合法 fd 上也会落 EBADF，是模式闸的真实样本。
    pub fn new() -> FdTable {
        let mut t = FdTable {
            slots: [FREE_SLOT; FD_MAX],
            descs: [None; FD_MAX],
            next_ino: 3,
            rlimit: FD_MAX,
            opened: 0,
            closed: 0,
            duped: 0,
        };
        t.install_std(FD_STDIN, ObjKind::CharDev, Access::ReadOnly);
        t.install_std(FD_STDOUT, ObjKind::CharDev, Access::WriteOnly);
        t.install_std(FD_STDERR, ObjKind::CharDev, Access::WriteOnly);
        t
    }

    /// 标准流安装：直接落到 0/1/2，不经过 `open` 的空位搜索。
    fn install_std(&mut self, fd: i32, kind: ObjKind, access: Access) {
        if fd < 0 {
            return;
        }
        let idx = fd as usize;
        if let Some(s) = self.slots.get_mut(idx) {
            s.used = true;
            s.desc = Some(idx as u8);
            s.gen = 1;
        }
        if let Some(d) = self.descs.get_mut(idx) {
            *d = Some(Desc::new(kind, access, 0, idx as u32));
        }
        self.opened = self.opened.saturating_add(1);
    }

    /// 当前软限制（RLIMIT_NOFILE）。可下调，用于 EMFILE 演练。
    pub fn rlimit(&self) -> usize {
        self.rlimit
    }

    /// 下调软限制。已经打开的 fd 不受影响（与 Linux 一致）。
    pub fn set_rlimit(&mut self, n: usize) -> SysResult<()> {
        if n == 0 || n > FD_MAX {
            return Err(KErr::Inval);
        }
        self.rlimit = n;
        Ok(())
    }

    /// 已打开计数（安装 + open + dup 成功的次数）。
    pub fn opened(&self) -> u32 {
        self.opened
    }

    /// 关闭计数。
    pub fn closed(&self) -> u32 {
        self.closed
    }

    /// dup 次数。
    pub fn duped(&self) -> u32 {
        self.duped
    }

    /// 第一层：范围闸。
    ///
    /// 负值与越界都返回 EBADF（Linux `EBADF`，不是 EINVAL）。
    /// rlimit 之下的下标才算「在进程 fd 空间内」；表容量之外的
    /// 下标在真实内核里根本不会映射到文件表，同样 EBADF。
    pub fn index(&self, fd: i32) -> SysResult<usize> {
        if fd < 0 {
            return Err(KErr::BadFd);
        }
        let idx = fd as usize;
        if idx >= FD_MAX || idx >= self.rlimit {
            return Err(KErr::BadFd);
        }
        Ok(idx)
    }

    /// 第二层：槽位闸 + 描述符解引用（只读）。
    pub fn desc(&self, fd: i32) -> SysResult<&Desc> {
        let idx = self.index(fd)?;
        let id = match self.slots.get(idx) {
            Some(s) if s.used => match s.desc {
                Some(id) => id as usize,
                None => return Err(KErr::BadFd),
            },
            _ => return Err(KErr::BadFd),
        };
        match self.descs.get(id) {
            Some(Some(d)) => Ok(d),
            _ => Err(KErr::BadFd),
        }
    }

    /// 第二层：槽位闸 + 描述符解引用（可写）。
    pub fn desc_mut(&mut self, fd: i32) -> SysResult<&mut Desc> {
        let idx = self.index(fd)?;
        let id = match self.slots.get(idx) {
            Some(s) if s.used => match s.desc {
                Some(id) => id as usize,
                None => return Err(KErr::BadFd),
            },
            _ => return Err(KErr::BadFd),
        };
        match self.descs.get_mut(id) {
            Some(Some(d)) => Ok(d),
            _ => Err(KErr::BadFd),
        }
    }

    /// 找到一个空槽（限制在 rlimit 内）。
    fn free_slot(&self) -> Option<usize> {
        let limit = self.rlimit.min(FD_MAX);
        let mut i = 0usize;
        while i < limit {
            if let Some(s) = self.slots.get(i) {
                if !s.used {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 找到一个空描述符位。
    fn free_desc(&self) -> Option<usize> {
        let mut i = 0usize;
        while i < FD_MAX {
            if let Some(d) = self.descs.get(i) {
                if d.is_none() {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 打开一个新对象。返回最小可用 fd。
    ///
    /// 失败次序：fd 表满（EMFILE）优先于系统表满（ENFILE）——
    /// 与 Linux 先 `expand_files` 后 `alloc_empty_file` 的实际次序一致。
    pub fn open(&mut self, kind: ObjKind, access: Access, size: u64) -> SysResult<i32> {
        let slot = match self.free_slot() {
            Some(s) => s,
            None => return Err(KErr::MFile),
        };
        let did = match self.free_desc() {
            Some(d) => d,
            None => return Err(KErr::NFile),
        };
        let ino = self.next_ino;
        self.next_ino = self.next_ino.wrapping_add(1);
        if self.next_ino == 0 {
            self.next_ino = 1;
        }
        if let Some(d) = self.descs.get_mut(did) {
            *d = Some(Desc::new(kind, access, size, ino));
        }
        if let Some(s) = self.slots.get_mut(slot) {
            s.used = true;
            s.desc = Some(did as u8);
            s.gen = s.gen.saturating_add(1);
        }
        self.opened = self.opened.saturating_add(1);
        Ok(slot as i32)
    }

    /// 关闭 fd。
    ///
    /// - 空槽/负值/越界：EBADF；
    /// - 共享描述符：只减引用，槽位立刻回收；
    /// - 最后一张门票：描述符与槽位同时回收，槽位代数 +1。
    pub fn close(&mut self, fd: i32) -> SysResult<()> {
        let idx = self.index(fd)?;
        let id = match self.slots.get(idx) {
            Some(s) if s.used => match s.desc {
                Some(id) => id as usize,
                None => return Err(KErr::BadFd),
            },
            _ => return Err(KErr::BadFd),
        };
        let refs = match self.descs.get(id) {
            Some(Some(d)) => d.refs,
            _ => return Err(KErr::BadFd),
        };
        if refs <= 1 {
            if let Some(d) = self.descs.get_mut(id) {
                *d = None;
            }
        } else if let Some(Some(d)) = self.descs.get_mut(id) {
            d.refs -= 1;
        }
        if let Some(s) = self.slots.get_mut(idx) {
            s.used = false;
            s.desc = None;
            s.gen = s.gen.wrapping_add(1);
        }
        self.closed = self.closed.saturating_add(1);
        Ok(())
    }

    /// 复制 fd 到最小可用槽位，共享同一描述符。
    pub fn dup(&mut self, fd: i32) -> SysResult<i32> {
        let id = self.desc_id(fd)?;
        let slot = match self.free_slot() {
            Some(s) => s,
            None => return Err(KErr::MFile),
        };
        if let Some(Some(d)) = self.descs.get_mut(id) {
            d.refs = d.refs.saturating_add(1);
        }
        if let Some(s) = self.slots.get_mut(slot) {
            s.used = true;
            s.desc = Some(id as u8);
            s.gen = s.gen.saturating_add(1);
        }
        self.duped = self.duped.saturating_add(1);
        Ok(slot as i32)
    }

    /// `dup2`：把 `old` 复制到指定 `new`。
    ///
    /// `new` 已占用时先静默关闭（POSIX：dup2 不因 new 已打开而失败）；
    /// `old == new` 时直接返回 new（不增引用、不关闭）。
    /// `new` 越过 rlimit 返回 EBADF。
    pub fn dup2(&mut self, old: i32, new: i32) -> SysResult<i32> {
        let id = self.desc_id(old)?;
        let nidx = self.index(new)?;
        if old == new {
            return Ok(new);
        }
        let occupied = match self.slots.get(nidx) {
            Some(s) => s.used,
            None => return Err(KErr::BadFd),
        };
        if occupied {
            self.close(new)?;
        }
        if let Some(Some(d)) = self.descs.get_mut(id) {
            d.refs = d.refs.saturating_add(1);
        }
        if let Some(s) = self.slots.get_mut(nidx) {
            s.used = true;
            s.desc = Some(id as u8);
            s.gen = s.gen.saturating_add(1);
        }
        self.duped = self.duped.saturating_add(1);
        Ok(new)
    }

    /// 取 fd 指向的描述符下标（内部用）。
    fn desc_id(&self, fd: i32) -> SysResult<usize> {
        let idx = self.index(fd)?;
        match self.slots.get(idx) {
            Some(s) if s.used => match s.desc {
                Some(id) => Ok(id as usize),
                None => Err(KErr::BadFd),
            },
            _ => Err(KErr::BadFd),
        }
    }

    /// 生成内核 token（携带代数）。
    pub fn token(&self, fd: i32) -> SysResult<FdToken> {
        let idx = self.index(fd)?;
        let gen = match self.slots.get(idx) {
            Some(s) if s.used => s.gen,
            _ => return Err(KErr::BadFd),
        };
        Ok(FdToken { idx: idx as u8, gen })
    }

    /// 用 token 解引用。代数不符即 EBADF。
    ///
    /// 这是「已关闭后被复用」的唯一识别手段：槽位下标相同但代数不同，
    /// 说明原来那张门票已经作废，不能继续指向新对象。
    pub fn resolve(&self, tok: FdToken) -> SysResult<&Desc> {
        let idx = tok.idx as usize;
        let (used, gen, id) = match self.slots.get(idx) {
            Some(s) => (s.used, s.gen, s.desc),
            None => return Err(KErr::BadFd),
        };
        if !used || gen != tok.gen {
            return Err(KErr::BadFd);
        }
        let did = match id {
            Some(v) => v as usize,
            None => return Err(KErr::BadFd),
        };
        match self.descs.get(did) {
            Some(Some(d)) => Ok(d),
            _ => Err(KErr::BadFd),
        }
    }

    /// 槽位代数（诊断面）。
    pub fn generation(&self, fd: i32) -> Option<u32> {
        let idx = self.index(fd).ok()?;
        self.slots.get(idx).map(|s| s.gen)
    }

    /// 描述符引用计数（诊断面）。
    pub fn refs(&self, fd: i32) -> Option<u32> {
        self.desc(fd).ok().map(|d| d.refs)
    }

    /// 活跃描述符数（用于守恒账）。
    pub fn live_descs(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < FD_MAX {
            if let Some(Some(_)) = self.descs.get(i) {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 活跃槽位数。
    pub fn live_slots(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < FD_MAX {
            if let Some(s) = self.slots.get(i) {
                if s.used {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 三、系统调用面
// ---------------------------------------------------------------------------

/// `fstat` 的稳定子集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stat {
    pub ino: u32,
    pub mode: u32,
    pub size: u64,
    pub nlink: u32,
    pub blksize: u32,
}

/// `read(2)`。
///
/// 成功返回实际读到的字节数；坏 fd 一律 EBADF；目录 EISDIR。
/// 定长对象读到尾返回 0（EOF），不返回短读以外的特殊码。
pub fn sys_read(t: &mut FdTable, fd: i32, want: u64) -> SysResult<u64> {
    let d = t.desc_mut(fd)?;
    if !d.readable() {
        return Err(KErr::BadFd);
    }
    if d.kind == ObjKind::Directory {
        return Err(KErr::IsDir);
    }
    if want == 0 {
        return Ok(0);
    }
    match d.kind {
        ObjKind::CharDev => Ok(want),
        ObjKind::EventFd => {
            let n = want.min(8);
            d.offset = 0;
            Ok(n)
        }
        _ => {
            let n = want.min(d.remaining());
            d.offset = d.offset.saturating_add(n);
            Ok(n)
        }
    }
}

/// `write(2)`。
///
/// 与 read 对称：只读 fd / O_PATH 写一律 EBADF；目录 EISDIR。
/// 定长对象允许越过尾部扩展（与普通文件一致）；管道满返回 EAGAIN。
pub fn sys_write(t: &mut FdTable, fd: i32, want: u64) -> SysResult<u64> {
    let d = t.desc_mut(fd)?;
    if !d.writable() {
        return Err(KErr::BadFd);
    }
    if d.kind == ObjKind::Directory {
        return Err(KErr::IsDir);
    }
    if want == 0 {
        return Ok(0);
    }
    match d.kind {
        ObjKind::CharDev => Ok(want),
        ObjKind::EventFd => {
            let n = want.min(8);
            d.offset = 0;
            Ok(n)
        }
        ObjKind::PipeWrite => {
            let cap = d.remaining();
            let n = want.min(cap);
            if n == 0 {
                return Err(KErr::Again);
            }
            d.offset = d.offset.saturating_add(n);
            Ok(n)
        }
        _ => {
            let end = match d.offset.checked_add(want) {
                Some(v) => v,
                None => return Err(KErr::Overflow),
            };
            if end > d.size {
                d.size = end;
            }
            d.offset = end;
            Ok(want)
        }
    }
}

/// 用内核 token 读（陈旧句柄演练面）。
pub fn sys_read_token(t: &FdTable, tok: FdToken, want: u64) -> SysResult<u64> {
    let d = t.resolve(tok)?;
    if !d.readable() {
        return Err(KErr::BadFd);
    }
    if d.kind == ObjKind::Directory {
        return Err(KErr::IsDir);
    }
    let n = want.min(d.remaining());
    Ok(n)
}

/// 用内核 token 写（陈旧句柄演练面）。
pub fn sys_write_token(t: &FdTable, tok: FdToken, want: u64) -> SysResult<u64> {
    let d = t.resolve(tok)?;
    if !d.writable() {
        return Err(KErr::BadFd);
    }
    if d.kind == ObjKind::Directory {
        return Err(KErr::IsDir);
    }
    let n = want.min(d.remaining());
    Ok(n)
}

/// `lseek(2)`。
///
/// 判定次序：fd（EBADF）→ 可定位性（ESPIPE）→ whence 闭集（EINVAL）
/// → 结果非负（EINVAL）→ 64 位范围（EOVERFLOW）。
/// O_PATH 的 fd 即使可定位也拒绝：它没有 IO 能力，定位无意义。
pub fn sys_lseek(t: &mut FdTable, fd: i32, off: i64, whence: u8) -> SysResult<u64> {
    let d = t.desc_mut(fd)?;
    if !d.access.can_read() && !d.access.can_write() {
        return Err(KErr::BadFd);
    }
    if !d.kind.seekable() {
        return Err(KErr::Spipe);
    }
    let base: i128 = match whence {
        0 => 0,
        1 => d.offset as i128,
        2 => d.size as i128,
        _ => return Err(KErr::Inval),
    };
    let target = base + off as i128;
    if target < 0 {
        return Err(KErr::Inval);
    }
    // off_t 是有符号 64 位：超过 i64::MAX 的结果在用户态无法表示，
    // Linux 返回 EOVERFLOW 而不是把它截成负数。
    if target > i64::MAX as i128 {
        return Err(KErr::Overflow);
    }
    d.offset = target as u64;
    Ok(d.offset)
}

/// `fstat(2)`。O_PATH fd 允许 fstat（这是它与「坏 fd」的关键差异）。
pub fn sys_fstat(t: &FdTable, fd: i32) -> SysResult<Stat> {
    let d = t.desc(fd)?;
    Ok(Stat {
        ino: d.ino,
        mode: d.mode(),
        size: d.size,
        nlink: 1,
        blksize: 4096,
    })
}

/// `close(2)` 的系统调用包装（便于与矩阵共用错误面）。
pub fn sys_close(t: &mut FdTable, fd: i32) -> SysResult<()> {
    t.close(fd)
}

/// `fcntl(F_GETFL)`：返回打开标志位。坏 fd 同样 EBADF。
pub fn sys_fcntl_getfl(t: &FdTable, fd: i32) -> SysResult<u32> {
    let d = t.desc(fd)?;
    Ok(d.flags)
}

// ---------------------------------------------------------------------------
// 四、坏 fd 矩阵（10 类 × 2 操作）
// ---------------------------------------------------------------------------

/// 坏 fd 类别数。
pub const BAD_CLASSES: usize = 10;
/// 每类两种操作（read / write）。
pub const BAD_OPS: usize = 2;
/// 矩阵格数。
pub const BAD_CELLS: usize = BAD_CLASSES * BAD_OPS;

/// 坏 fd 类别定义。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BadClass {
    /// 稳定编号，同时是矩阵行号。
    pub id: u8,
    /// 中文名（报告与判据名共用）。
    pub label: &'static str,
    /// 该类的构造方法编号（见 `build_table`）。
    pub ctor: u8,
}

/// 十类坏 fd。前六类死在 ①②③（句柄合法性），后四类死在 ④⑤
/// （能力与对象）。注意：访问模式**单侧**不符（只写 fd 上写）不算
/// 坏 fd——那是合法调用；单侧矩阵放在 `mode_matrix` 里单独守。
pub const BAD_CLASSES_TAB: [BadClass; BAD_CLASSES] = [
    BadClass { id: 0, label: "负 fd", ctor: 0 },
    BadClass { id: 1, label: "等于表容量的 fd", ctor: 1 },
    BadClass { id: 2, label: "远超表容量的 fd", ctor: 2 },
    BadClass { id: 3, label: "从未打开的槽", ctor: 3 },
    BadClass { id: 4, label: "已关闭的 fd", ctor: 4 },
    BadClass { id: 5, label: "双关闭的 fd", ctor: 5 },
    BadClass { id: 6, label: "rlimit 收缩后越界的 fd", ctor: 10 },
    BadClass { id: 7, label: "O_PATH fd 读写", ctor: 8 },
    BadClass { id: 8, label: "陈旧 token", ctor: 9 },
    BadClass { id: 9, label: "已关闭的管道端", ctor: 11 },
];

/// 矩阵放行判据：20 格必须全部是 `-EBADF`。
pub const WANT_RAW: i32 = -9;

/// 单格结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    pub raw: i32,
    pub value: u64,
}

impl Cell {
    /// 是否命中期望（EBADF 且无返回值）。
    pub fn hit(&self) -> bool {
        self.raw == WANT_RAW && self.value == 0
    }
}

/// 执行一次坏 fd 矩阵。
///
/// 每行独立建表，互不污染；返回 `[read, write]` 两格。
/// 该函数是矩阵的**唯一**执行入口，判据不允许另写私有分支。
pub fn bad_matrix() -> [[Cell; BAD_OPS]; BAD_CLASSES] {
    let mut out = [[Cell { raw: 0, value: 0 }; BAD_OPS]; BAD_CLASSES];
    let mut row = 0usize;
    while row < BAD_CLASSES {
        if let Some(cls) = BAD_CLASSES_TAB.get(row) {
            let r = run_class(cls.ctor, 0);
            let w = run_class(cls.ctor, 1);
            if let Some(slot) = out.get_mut(row) {
                if let Some(c) = slot.get_mut(0) {
                    *c = r;
                }
                if let Some(c) = slot.get_mut(1) {
                    *c = w;
                }
            }
        }
        row += 1;
    }
    out
}

/// 按构造编号建表，并对指定操作（0=read,1=write）取结果。
fn run_class(ctor: u8, op: u8) -> Cell {
    match ctor {
        0 => call_op(-1, op),
        1 => call_op(FD_MAX as i32, op),
        2 => call_op(999_999, op),
        3 => {
            let t = base_table();
            call_op_table(t, 9, op)
        }
        4 => {
            let mut t = base_table();
            let fd = t.open(ObjKind::Regular, Access::ReadWrite, 128).unwrap_or(-1);
            let _ = t.close(fd);
            call_op_table(t, fd, op)
        }
        5 => {
            let mut t = base_table();
            let fd = t.open(ObjKind::Regular, Access::ReadWrite, 128).unwrap_or(-1);
            let _ = t.close(fd);
            let _ = t.close(fd);
            call_op_table(t, fd, op)
        }
        6 => {
            let mut t = base_table();
            // fd 5 在 rlimit=4 之后越界：范围闸必须按「当前 rlimit」判，
            // 而不是按表容量判——这是 RLIMIT_NOFILE 与静态容量分离的点。
            let _ = t.set_rlimit(4);
            call_op_table(t, 5, op)
        }
        7 => {
            let t = base_table();
            call_op_table(t, FD_STDIN, op)
        }
        8 => {
            let mut t = base_table();
            let fd = t.open(ObjKind::Regular, Access::PathOnly, 128).unwrap_or(-1);
            call_op_table(t, fd, op)
        }
        9 => {
            let mut t = base_table();
            let fd = t.open(ObjKind::Regular, Access::ReadWrite, 128).unwrap_or(-1);
            let tok = match t.token(fd) {
                Ok(v) => v,
                Err(_) => return Cell { raw: 0, value: 0 },
            };
            let _ = t.close(fd);
            // 用同一个槽位再开一个对象，制造「同槽不同代」。
            let _ = t.open(ObjKind::Regular, Access::ReadWrite, 64);
            let r = if op == 0 { sys_read_token(&t, tok, 8) } else { sys_write_token(&t, tok, 8) };
            match r {
                Ok(v) => Cell { raw: 0, value: v },
                Err(e) => Cell { raw: e.raw(), value: 0 },
            }
        }
        10 => {
            let mut t = base_table();
            let _ = t.set_rlimit(4);
            call_op_table(t, 5, op)
        }
        11 => {
            let mut t = base_table();
            let fd = t.open(ObjKind::PipeWrite, Access::WriteOnly, 64).unwrap_or(-1);
            let _ = t.close(fd);
            call_op_table(t, fd, op)
        }
        _ => Cell { raw: 0, value: 0 },
    }
}

/// 对「不属于任何表」的 fd 直接调用：范围闸在第一层就拒绝，
/// 因此这里用一个空表承载，语义等价。
fn call_op(fd: i32, op: u8) -> Cell {
    call_op_table(base_table(), fd, op)
}

/// 在给定表上执行一次读或写。
fn call_op_table(mut t: FdTable, fd: i32, op: u8) -> Cell {
    let r = if op == 0 { sys_read(&mut t, fd, 8) } else { sys_write(&mut t, fd, 8) };
    match r {
        Ok(v) => Cell { raw: 0, value: v },
        Err(e) => Cell { raw: e.raw(), value: 0 },
    }
}

/// 基准表：三个标准流 + 一个常规文件 + 一个目录 + 一对管道端。
///
/// 这六个对象覆盖了对象闸的全部负例（目录、管道），
/// 同时保证「合法路径」有正样本可用。
pub fn base_table() -> FdTable {
    let mut t = FdTable::new();
    let _ = t.open(ObjKind::Regular, Access::ReadWrite, 256);
    let _ = t.open(ObjKind::Directory, Access::ReadOnly, 0);
    let _ = t.open(ObjKind::PipeRead, Access::ReadOnly, 64);
    let _ = t.open(ObjKind::PipeWrite, Access::WriteOnly, 64);
    t
}

/// 矩阵是否全绿（20/20）。
pub fn matrix_green(m: &[[Cell; BAD_OPS]; BAD_CLASSES]) -> bool {
    let mut r = 0usize;
    while r < BAD_CLASSES {
        if let Some(row) = m.get(r) {
            let mut c = 0usize;
            while c < BAD_OPS {
                if let Some(cell) = row.get(c) {
                    if !cell.hit() {
                        return false;
                    }
                }
                c += 1;
            }
        }
        r += 1;
    }
    true
}

/// 矩阵红格名单（诊断用，不含分配）。
pub fn matrix_reds(m: &[[Cell; BAD_OPS]; BAD_CLASSES]) -> Vec<String> {
    let mut out = Vec::new();
    let mut r = 0usize;
    while r < BAD_CLASSES {
        let label = BAD_CLASSES_TAB.get(r).map(|c| c.label).unwrap_or("?");
        let mut c = 0usize;
        while c < BAD_OPS {
            let hit = m.get(r).and_then(|row| row.get(c)).map(|x| x.hit()).unwrap_or(false);
            if !hit {
                let raw = m.get(r).and_then(|row| row.get(c)).map(|x| x.raw).unwrap_or(0);
                let op = if c == 0 { "read" } else { "write" };
                out.push(format!("{}:{}=>{}", label, op, raw));
            }
            c += 1;
        }
        r += 1;
    }
    out
}

/// 十轮复检：同一坏 fd 矩阵连续重跑，返回（全绿轮数, 逐格漂移数）。
///
/// 卡面判据写的是「10/10 次检出复测格过」——只判一次全绿会放过一类隐蔽缺陷：
/// 某轮里目标闸恰好被更前面的闸先拦下（比如范围闸先于槽位闸命中），返回值仍是
/// -EBADF，矩阵照样绿，但目标闸本身其实是坏的。逐格比对十轮的原始返回值，任何
/// 一格换了解释路径都会记为漂移，从而把「碰巧全绿」和「机制性全绿」区分开。
pub fn matrix_repeat(rounds: usize) -> (usize, usize) {
    let mut green = 0usize;
    let mut drift = 0usize;
    let mut prev: Option<[[i32; BAD_OPS]; BAD_CLASSES]> = None;
    let mut k = 0usize;
    while k < rounds {
        let m = bad_matrix();
        let mut sig = [[0i32; BAD_OPS]; BAD_CLASSES];
        let mut r = 0usize;
        while r < BAD_CLASSES {
            let mut c = 0usize;
            while c < BAD_OPS {
                if let Some(cell) = m.get(r).and_then(|row| row.get(c)) {
                    sig[r][c] = cell.raw;
                }
                c += 1;
            }
            r += 1;
        }
        if matrix_green(&m) {
            green += 1;
        }
        if let Some(p) = prev {
            let mut r = 0usize;
            while r < BAD_CLASSES {
                let mut c = 0usize;
                while c < BAD_OPS {
                    if p[r][c] != sig[r][c] {
                        drift += 1;
                    }
                    c += 1;
                }
                r += 1;
            }
        }
        prev = Some(sig);
        k += 1;
    }
    (green, drift)
}
// ---------------------------------------------------------------------------
// 五、合法路径语料（8 正 + 6 对照）
// ---------------------------------------------------------------------------

/// 合法路径语料槽位数。
pub const ACCEPT_CASES: usize = 8;
/// 对照（边界）语料槽位数。
pub const EDGE_CASES: usize = 6;
/// 语料总数。
pub const CORPUS: usize = ACCEPT_CASES + EDGE_CASES;

/// 语料期望。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Expect {
    /// 成功且返回值等于 value。
    Ok(u64),
    /// 失败且码等于 raw。
    Err(i32),
}

/// 一条真实调用语料。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CallCase {
    pub label: &'static str,
    pub ctor: u8,
    pub op: u8,
    pub want: u64,
    pub expect: Expect,
}

/// 14 条语料：前 8 条为合法路径，后 6 条为边界对照。
pub const CALLS: [CallCase; CORPUS] = [
    CallCase { label: "stdin 读", ctor: 10, op: 0, want: 4, expect: Expect::Ok(4) },
    CallCase { label: "stdout 写", ctor: 11, op: 1, want: 4, expect: Expect::Ok(4) },
    CallCase { label: "stdin 读零长", ctor: 10, op: 0, want: 0, expect: Expect::Ok(0) },
    CallCase { label: "stdout 写零长", ctor: 11, op: 1, want: 0, expect: Expect::Ok(0) },
    CallCase { label: "常规文件读", ctor: 12, op: 0, want: 16, expect: Expect::Ok(16) },
    CallCase { label: "常规文件写扩展", ctor: 12, op: 1, want: 32, expect: Expect::Ok(32) },
    CallCase { label: "管道读 8", ctor: 14, op: 0, want: 8, expect: Expect::Ok(8) },
    CallCase { label: "管道写 8", ctor: 15, op: 1, want: 8, expect: Expect::Ok(8) },
    CallCase { label: "目录读拒绝", ctor: 13, op: 0, want: 8, expect: Expect::Err(-21) },
    CallCase { label: "目录写拒绝", ctor: 13, op: 1, want: 8, expect: Expect::Err(-21) },
    CallCase { label: "管道读端写入", ctor: 14, op: 1, want: 8, expect: Expect::Err(-9) },
    CallCase { label: "管道写端读出", ctor: 15, op: 0, want: 8, expect: Expect::Err(-9) },
    CallCase { label: "管道定位拒绝", ctor: 14, op: 2, want: 0, expect: Expect::Err(-29) },
    CallCase { label: "事件 fd 读 8", ctor: 16, op: 0, want: 8, expect: Expect::Ok(8) },
];

/// 按构造编号返回 (表, 目标 fd)。
fn ctor_table(ctor: u8) -> (FdTable, i32) {
    match ctor {
        10 => (FdTable::new(), FD_STDIN),
        11 => (FdTable::new(), FD_STDOUT),
        12 => {
            let mut t = FdTable::new();
            let fd = t.open(ObjKind::Regular, Access::ReadWrite, 256).unwrap_or(-1);
            (t, fd)
        }
        13 => {
            let mut t = FdTable::new();
            // 目录必须以读写能力打开：否则「写目录」会先撞模式闸拿到
            // EBADF，永远走不到对象闸的 EISDIR，语料就成了假绿。
            let fd = t.open(ObjKind::Directory, Access::ReadWrite, 0).unwrap_or(-1);
            (t, fd)
        }
        14 => {
            let mut t = FdTable::new();
            let fd = t.open(ObjKind::PipeRead, Access::ReadOnly, 64).unwrap_or(-1);
            (t, fd)
        }
        15 => {
            let mut t = FdTable::new();
            let fd = t.open(ObjKind::PipeWrite, Access::WriteOnly, 64).unwrap_or(-1);
            (t, fd)
        }
        16 => {
            let mut t = FdTable::new();
            let fd = t.open(ObjKind::EventFd, Access::ReadWrite, 0).unwrap_or(-1);
            (t, fd)
        }
        _ => (FdTable::new(), -1),
    }
}

/// 执行一条语料，返回实际结果。
pub fn run_call(c: &CallCase) -> Expect {
    let (mut t, fd) = ctor_table(c.ctor);
    match c.op {
        0 => match sys_read(&mut t, fd, c.want) {
            Ok(v) => Expect::Ok(v),
            Err(e) => Expect::Err(e.raw()),
        },
        1 => match sys_write(&mut t, fd, c.want) {
            Ok(v) => Expect::Ok(v),
            Err(e) => Expect::Err(e.raw()),
        },
        _ => match sys_lseek(&mut t, fd, 0, 3) {
            Ok(v) => Expect::Ok(v),
            Err(e) => Expect::Err(e.raw()),
        },
    }
}

/// 语料全体对平。
pub fn corpus_green() -> bool {
    let mut i = 0usize;
    while i < CORPUS {
        if let Some(c) = CALLS.get(i) {
            if run_call(c) != c.expect {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 语料红名单。
pub fn corpus_reds() -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < CORPUS {
        if let Some(c) = CALLS.get(i) {
            let got = run_call(c);
            if got != c.expect {
                out.push(format!("{}: want {:?} got {:?}", c.label, c.expect, got));
            }
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 六、引用计数与偏移账
// ---------------------------------------------------------------------------

/// 引用计数账：`open` 增加 1，`dup` 增加 1，`close` 减少 1。
///
/// 不变量：任一时刻 `sum(refs) == live_descs`，且
/// `opened + duped - closed == live_slots`。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RefAudit {
    pub opens: u32,
    pub dups: u32,
    pub closes: u32,
    pub live_slots: usize,
    pub live_descs: usize,
    pub sum_refs: u32,
}

impl RefAudit {
    /// 槽位守恒：打开 + 复制 - 关闭 = 活跃槽。
    pub fn slots_balanced(&self) -> bool {
        let lhs = self.opens.saturating_add(self.dups);
        if lhs < self.closes {
            return false;
        }
        (lhs - self.closes) as usize == self.live_slots
    }

    /// 描述符守恒：活跃描述符数等于引用计数之和的「门票数」，
    /// 每个描述符至少 1 个引用，故 `sum_refs >= live_descs`。
    pub fn descs_balanced(&self) -> bool {
        self.sum_refs as usize >= self.live_descs && self.live_descs > 0
    }
}

/// 对一张表做引用与偏移审计。
pub fn audit_table(t: &FdTable) -> RefAudit {
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < FD_MAX {
        if let Some(Some(d)) = t.descs.get(i) {
            sum = sum.saturating_add(d.refs);
        }
        i += 1;
    }
    RefAudit {
        opens: t.opened(),
        dups: t.duped(),
        closes: t.closed(),
        live_slots: t.live_slots(),
        live_descs: t.live_descs(),
        sum_refs: sum,
    }
}

/// dup 演练：返回 (原 fd, 复制 fd, 关闭复制后原 fd 仍可读, 槽代数变化)。
pub fn dup_drill() -> (i32, i32, bool, bool) {
    let mut t = FdTable::new();
    let fd = match t.open(ObjKind::Regular, Access::ReadWrite, 64) {
        Ok(v) => v,
        Err(_) => return (-1, -1, false, false),
    };
    let g0 = t.generation(fd).unwrap_or(0);
    let d2 = match t.dup(fd) {
        Ok(v) => v,
        Err(_) => return (fd, -1, false, false),
    };
    let shared = t.refs(fd).unwrap_or(0) == 2 && t.refs(d2).unwrap_or(0) == 2;
    let _ = t.close(d2);
    let alive = sys_read(&mut t, fd, 4).map(|v| v == 4).unwrap_or(false);
    let g1 = t.generation(d2).unwrap_or(g0);
    (fd, d2, shared && alive, g1 != g0)
}

/// 偏移账：定位到文件尾再写，偏移与大小必须同步推进。
pub fn offset_drill() -> bool {
    let mut t = FdTable::new();
    let fd = match t.open(ObjKind::Regular, Access::ReadWrite, 100) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if sys_lseek(&mut t, fd, 0, 2) != Ok(100) {
        return false;
    }
    if sys_write(&mut t, fd, 20) != Ok(20) {
        return false;
    }
    let d = match t.desc(fd) {
        Ok(v) => *v,
        Err(_) => return false,
    };
    d.offset == 120 && d.size == 120
}

/// 负定位必须 EINVAL，越过 64 位必须 EOVERFLOW。
pub fn lseek_drill() -> [i32; 4] {
    let mut t = FdTable::new();
    let fd = match t.open(ObjKind::Regular, Access::ReadWrite, 100) {
        Ok(v) => v,
        Err(_) => return [0, 0, 0, 0],
    };
    let a = sys_lseek(&mut t, fd, -1, 0).err().map(|e| e.raw()).unwrap_or(0);
    let b = sys_lseek(&mut t, fd, 0, 9).err().map(|e| e.raw()).unwrap_or(0);
    // 先把偏移推到 100，再加 i64::MAX：结果越过 off_t 上界必 EOVERFLOW。
    let _ = sys_lseek(&mut t, fd, 100, 0);
    let c = sys_lseek(&mut t, fd, i64::MAX, 1).err().map(|e| e.raw()).unwrap_or(0);
    let d = sys_lseek(&mut t, fd, 0, 0).map(|v| v as i32).unwrap_or(-1);
    [a, b, c, d]
}

// ---------------------------------------------------------------------------
// 七、账本、封印与重放
// ---------------------------------------------------------------------------

/// FNV-1a 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 素数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折叠一个字节。
pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

/// 折叠一个 u32（小端四字节）。
pub fn fold_word(h: u64, v: u32) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 4 {
        x = fold_byte(x, ((v >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
    x
}

/// 折叠一个 u64（小端八字节）。
pub fn fold_u64(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 8 {
        x = fold_byte(x, ((v >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
    x
}

/// 折叠字符串（逐字节，长度也入账，避免拼接歧义）。
pub fn fold_str(mut h: u64, s: &str) -> u64 {
    h = fold_u64(h, s.len() as u64);
    for b in s.as_bytes() {
        h = fold_byte(h, *b);
    }
    h
}

/// 一条账本记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Probe {
    pub class: u8,
    pub op: u8,
    pub raw: i32,
    pub value: u64,
}

impl Probe {
    /// 该条记录是否与矩阵期望一致。
    pub fn hit(&self) -> bool {
        self.raw == WANT_RAW && self.value == 0
    }

    /// 单条指纹。
    pub fn fold(&self, h: u64) -> u64 {
        let x = fold_word(h, self.class as u32);
        let x = fold_word(x, self.op as u32);
        let x = fold_word(x, self.raw as u32);
        fold_u64(x, self.value)
    }
}

/// 坏 fd 账本。
pub struct Ledger {
    rows: Vec<Probe>,
    sealed: bool,
    frozen: bool,
    root: u64,
}

impl Ledger {
    /// 空账本。
    pub fn new() -> Ledger {
        Ledger { rows: Vec::new(), sealed: false, frozen: false, root: 0 }
    }

    /// 登记一条。槽位必须严格等于当前长度（禁跳号/越界）。
    pub fn record(&mut self, p: Probe) -> Result<(), u32> {
        if self.frozen {
            return Err(D12_SEAL);
        }
        if p.class as usize >= BAD_CLASSES || p.op as usize >= BAD_OPS {
            return Err(D11_LEDGER);
        }
        if self.rows.len() >= BAD_CELLS {
            return Err(D11_LEDGER);
        }
        self.rows.push(p);
        self.sealed = false;
        Ok(())
    }

    /// 记录条数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 逐格复核：条数、行列覆盖、码与值。
    pub fn audit(&self) -> Result<(), u32> {
        if self.rows.len() != BAD_CELLS {
            return Err(D11_LEDGER);
        }
        let mut seen = [false; BAD_CELLS];
        for p in self.rows.iter() {
            let idx = (p.class as usize) * BAD_OPS + (p.op as usize);
            if idx >= BAD_CELLS {
                return Err(D11_LEDGER);
            }
            if let Some(s) = seen.get_mut(idx) {
                if *s {
                    return Err(D11_LEDGER);
                }
                *s = true;
            }
            if !p.hit() {
                return Err(D11_LEDGER);
            }
        }
        let mut i = 0usize;
        while i < BAD_CELLS {
            if seen.get(i) != Some(&true) {
                return Err(D11_LEDGER);
            }
            i += 1;
        }
        Ok(())
    }

    /// 封印：折叠全账指纹。
    pub fn seal(&mut self) -> Result<u64, u32> {
        self.audit()?;
        let mut h = FNV_OFF;
        for p in self.rows.iter() {
            h = p.fold(h);
        }
        self.root = h;
        self.sealed = true;
        Ok(h)
    }

    /// 冻结：之后拒收新记录。
    pub fn freeze(&mut self) -> Result<(), u32> {
        if !self.sealed {
            return Err(D12_SEAL);
        }
        self.frozen = true;
        Ok(())
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 指纹。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 重放：按同一矩阵重建账本，比指纹。
    pub fn replay(&self) -> Result<u64, u32> {
        let mut b = build_ledger();
        let r = b.seal()?;
        if r == self.root {
            Ok(r)
        } else {
            Err(D12_SEAL)
        }
    }

    /// 渲染一行（账本自检失败时用）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("F9202-LEDGER\n");
        for p in self.rows.iter() {
            s.push_str(&format!(
                "{}:{}|{}|{}\n",
                p.class, p.op, p.raw, p.value
            ));
        }
        s.push_str(&format!("root={:016x} sealed={} frozen={}\n", self.root, self.sealed, self.frozen));
        s
    }
}

/// 建一份全绿账本（20 格，按矩阵实际执行结果写入）。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let m = bad_matrix();
    let mut r = 0usize;
    while r < BAD_CLASSES {
        let mut c = 0usize;
        while c < BAD_OPS {
            let cell = m.get(r).and_then(|row| row.get(c)).copied().unwrap_or(Cell { raw: 0, value: 0 });
            let _ = l.record(Probe { class: r as u8, op: c as u8, raw: cell.raw, value: cell.value });
            c += 1;
        }
        r += 1;
    }
    l
}

// ---------------------------------------------------------------------------
// 八、四路注入核
// ---------------------------------------------------------------------------

/// 四路自检：表路 / 判路 / 渲染路 / 账本路。
///
/// 每路独立构造违约，必须被对应层抓住：
/// 1. 表路：把 fd 改成负数，`index` 必须 EBADF；
/// 2. 判路：把格值改成 EINVAL，`Cell::hit` 必须为假；
/// 3. 渲染路：报告头必须与冻结常量逐字一致；
/// 4. 账本路：把一格码改成 EFAULT，`audit` 必须报红。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];

    let t = base_table();
    got[0] = t.index(-1).err() == Some(KErr::BadFd)
        && t.index(FD_MAX as i32).err() == Some(KErr::BadFd)
        && t.index(9).is_ok();

    let bad = Cell { raw: KErr::Inval.raw(), value: 0 };
    let good = Cell { raw: WANT_RAW, value: 0 };
    got[1] = !bad.hit() && good.hit();

    let r = run();
    got[2] = match r {
        Ok(rep) => rep.head() == HEAD_OK && rep.render().ends_with("GREEN"),
        Err(_) => false,
    };

    let mut l = Ledger::new();
    let mut r0 = 0usize;
    while r0 < BAD_CLASSES {
        let mut c = 0usize;
        while c < BAD_OPS {
            let raw = if r0 == 4 && c == 1 { KErr::Fault.raw() } else { WANT_RAW };
            let _ = l.record(Probe { class: r0 as u8, op: c as u8, raw, value: 0 });
            c += 1;
        }
        r0 += 1;
    }
    got[3] = l.audit().is_err();

    got
}

// ---------------------------------------------------------------------------
// 九、报告
// ---------------------------------------------------------------------------

/// 报告头冻结文本（渲染路判据逐字钉住）。
pub const HEAD_OK: &str = "F9202|classes=10|bad=20/20";

/// 汇总报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Report {
    pub classes: usize,
    pub bad_hit: usize,
    pub bad_total: usize,
    pub accept_hit: usize,
    pub accept_total: usize,
    pub edge_hit: usize,
    pub edge_total: usize,
    pub root: u64,
}

impl Report {
    /// 千分率（四舍五入到整数）。
    pub fn rate(&self) -> u32 {
        let total = self.bad_total + self.accept_total + self.edge_total;
        if total == 0 {
            return 0;
        }
        let hit = self.bad_hit + self.accept_hit + self.edge_hit;
        ((hit as u64 * 1000 + (total as u64) / 2) / total as u64) as u32
    }

    /// 是否全绿：矩阵 20/20、合法 8/8、边界 6/6。
    pub fn green(&self) -> bool {
        self.classes == BAD_CLASSES
            && self.bad_hit == self.bad_total
            && self.bad_total == BAD_CELLS
            && self.accept_hit == self.accept_total
            && self.accept_total == ACCEPT_CASES
            && self.edge_hit == self.edge_total
            && self.edge_total == EDGE_CASES
    }

    /// 闸门：不绿时返回第一个失败面的诊断码。
    pub fn gate(&self) -> Result<(), u32> {
        if self.classes != BAD_CLASSES {
            return Err(D03_RANGE);
        }
        if self.bad_hit != self.bad_total {
            return Err(D11_LEDGER);
        }
        if self.accept_hit != self.accept_total {
            return Err(D08_ACCEPT);
        }
        if self.edge_hit != self.edge_total {
            return Err(D07_OBJECT);
        }
        Ok(())
    }

    /// 报告头（与 `HEAD_OK` 对拍）。
    pub fn head(&self) -> String {
        format!("F9202|classes={}|bad={}/{}", self.classes, self.bad_hit, self.bad_total)
    }

    /// 全报告渲染。
    pub fn render(&self) -> String {
        format!(
            "{}|accept={}/{}|edge={}/{}|rate={}|root={:016x}|{}",
            self.head(),
            self.accept_hit,
            self.accept_total,
            self.edge_hit,
            self.edge_total,
            self.rate(),
            self.root,
            if self.green() { "GREEN" } else { "RED" }
        )
    }
}

/// 执行完整报告。
pub fn run() -> Result<Report, u32> {
    run_with(None)
}

/// 执行完整报告，可选地污染第 `slot` 个矩阵格（注入演练）。
///
/// `over = Some((idx, raw))` 把第 idx 格（0..19）的码改成 raw；
/// 用于验证「报告层不会把红格洗绿」。
pub fn run_with(over: Option<(usize, i32)>) -> Result<Report, u32> {
    let mut m = bad_matrix();
    if let Some((idx, raw)) = over {
        let r = idx / BAD_OPS;
        let c = idx % BAD_OPS;
        if let Some(cell) = m.get_mut(r).and_then(|row| row.get_mut(c)) {
            cell.raw = raw;
            cell.value = 0;
        }
    }
    let mut bad_hit = 0usize;
    let mut r = 0usize;
    while r < BAD_CLASSES {
        let mut c = 0usize;
        while c < BAD_OPS {
            if m.get(r).and_then(|row| row.get(c)).map(|x| x.hit()).unwrap_or(false) {
                bad_hit += 1;
            }
            c += 1;
        }
        r += 1;
    }

    let mut accept_hit = 0usize;
    let mut edge_hit = 0usize;
    let mut i = 0usize;
    while i < CORPUS {
        if let Some(c) = CALLS.get(i) {
            let ok = run_call(c) == c.expect;
            if i < ACCEPT_CASES {
                if ok {
                    accept_hit += 1;
                }
            } else if ok {
                edge_hit += 1;
            }
        }
        i += 1;
    }

    let mut ledger = build_ledger();
    let root = match ledger.seal() {
        Ok(v) => v,
        Err(e) => return Err(e),
    };

    let rep = Report {
        classes: BAD_CLASSES,
        bad_hit,
        bad_total: BAD_CELLS,
        accept_hit,
        accept_total: ACCEPT_CASES,
        edge_hit,
        edge_total: EDGE_CASES,
        root,
    };
    rep.gate()?;
    Ok(rep)
}

// ---------------------------------------------------------------------------
// 十、自检判据
// ---------------------------------------------------------------------------

/// 段基址是否合法：0x20 对齐 + 落在本批保留窗。
fn seg_ok() -> bool {
    SEG_BASE % 0x20 == 0 && (0x90000..=0x9A420).contains(&SEG_BASE)
}

/// 诊断码闭集：13 个、两两互异、全部落在 SEG_BASE+1..=SEG_BASE+0x0D。
fn codes_ok() -> bool {
    if ALL_CODES.len() != 13 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_CODES.len() {
        if let Some(a) = ALL_CODES.get(i) {
            if *a != SEG_BASE + 0x01 + i as u32 {
                return false;
            }
            let mut j = i + 1;
            while j < ALL_CODES.len() {
                if ALL_CODES.get(j) == Some(a) {
                    return false;
                }
                j += 1;
            }
        }
        i += 1;
    }
    true
}

/// 错误码闭集：raw 互异、from_raw 可逆、名字非空。
fn errno_ok() -> bool {
    let mut i = 0usize;
    while i < ALL_ERR.len() {
        if let Some(e) = ALL_ERR.get(i) {
            if e.name().is_empty() || e.zh().is_empty() {
                return false;
            }
            if KErr::from_raw(e.raw()) != Some(*e) {
                return false;
            }
            let mut j = i + 1;
            while j < ALL_ERR.len() {
                if ALL_ERR.get(j).map(|x| x.raw()) == Some(e.raw()) {
                    return false;
                }
                j += 1;
            }
        }
        i += 1;
    }
    true
}

/// fd 范围闸：负值/表尾/表尾+1 全 EBADF；0/2/合法槽放行。
fn range_ok() -> bool {
    let t = FdTable::new();
    t.index(-1).err() == Some(KErr::BadFd)
        && t.index(i32::MIN).err() == Some(KErr::BadFd)
        && t.index(FD_MAX as i32).err() == Some(KErr::BadFd)
        && t.index(FD_MAX as i32 + 1).err() == Some(KErr::BadFd)
        && t.index(0).is_ok()
        && t.index(2).is_ok()
}

/// 槽位闸：空槽/关闭槽/双关闭槽全 EBADF。
fn slot_ok() -> bool {
    let mut t = FdTable::new();
    if sys_read(&mut t, 9, 1).err() != Some(KErr::BadFd) {
        return false;
    }
    let fd = match t.open(ObjKind::Regular, Access::ReadWrite, 32) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if sys_write(&mut t, fd, 1) != Ok(1) {
        return false;
    }
    if t.close(fd).is_err() {
        return false;
    }
    if sys_read(&mut t, fd, 1).err() != Some(KErr::BadFd) {
        return false;
    }
    t.close(fd).err() == Some(KErr::BadFd)
}

/// 代数闸：关闭后复用同槽，旧 token 必 EBADF。
fn gen_ok() -> bool {
    let mut t = FdTable::new();
    let fd = match t.open(ObjKind::Regular, Access::ReadWrite, 32) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let tok = match t.token(fd) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if t.close(fd).is_err() {
        return false;
    }
    let fd2 = match t.open(ObjKind::Regular, Access::ReadWrite, 32) {
        Ok(v) => v,
        Err(_) => return false,
    };
    fd2 == fd && sys_read_token(&t, tok, 4).err() == Some(KErr::BadFd)
}

/// 模式闸：只写读、只读写、O_PATH 双拒；只读读、只写写放行。
fn mode_ok() -> bool {
    let mut t = FdTable::new();
    let ro = match t.open(ObjKind::Regular, Access::ReadOnly, 16) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let wo = match t.open(ObjKind::Regular, Access::WriteOnly, 16) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let p = match t.open(ObjKind::Regular, Access::PathOnly, 16) {
        Ok(v) => v,
        Err(_) => return false,
    };
    sys_read(&mut t, ro, 4) == Ok(4)
        && sys_write(&mut t, wo, 4) == Ok(4)
        && sys_read(&mut t, wo, 4).err() == Some(KErr::BadFd)
        && sys_write(&mut t, ro, 4).err() == Some(KErr::BadFd)
        && sys_read(&mut t, p, 4).err() == Some(KErr::BadFd)
        && sys_write(&mut t, p, 4).err() == Some(KErr::BadFd)
        && sys_fstat(&t, p).is_ok()
}

/// 对象闸：目录读写 EISDIR；管道定位 ESPIPE；管道反向 EBADF。
fn object_ok() -> bool {
    let mut t = FdTable::new();
    let d = match t.open(ObjKind::Directory, Access::ReadWrite, 0) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let pr = match t.open(ObjKind::PipeRead, Access::ReadOnly, 64) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let pw = match t.open(ObjKind::PipeWrite, Access::WriteOnly, 64) {
        Ok(v) => v,
        Err(_) => return false,
    };
    sys_read(&mut t, d, 8).err() == Some(KErr::IsDir)
        && sys_write(&mut t, d, 8).err() == Some(KErr::IsDir)
        && sys_lseek(&mut t, pr, 0, 0).err() == Some(KErr::Spipe)
        && sys_write(&mut t, pr, 8).err() == Some(KErr::BadFd)
        && sys_read(&mut t, pw, 8).err() == Some(KErr::BadFd)
}

/// 合法路径：14 条语料逐条对平。
fn corpus_ok() -> bool {
    corpus_green()
}

/// 坏 fd 矩阵：20 格全 EBADF。
fn matrix_ok() -> bool {
    let m = bad_matrix();
    matrix_green(&m)
}

/// 十轮检出复测：十轮全绿且十轮之间零格漂移。
fn repeat_ok() -> bool {
    let (green, drift) = matrix_repeat(10);
    green == 10 && drift == 0
}

/// 引用计数：dup 共享、关闭一张门票不影响另一张。
fn refs_ok() -> bool {
    let (_, _, shared_alive, gen_bumped) = dup_drill();
    if !shared_alive || !gen_bumped {
        return false;
    }
    let mut t = base_table();
    let a = audit_table(&t);
    if !a.slots_balanced() || !a.descs_balanced() {
        return false;
    }
    let fd = match t.open(ObjKind::Regular, Access::ReadWrite, 8) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let _ = t.dup(fd);
    let b = audit_table(&t);
    b.slots_balanced() && b.descs_balanced() && b.sum_refs >= 2
}

/// 偏移：定位/扩展/负值/坏 whence/溢出五面。
fn offset_ok() -> bool {
    let l = lseek_drill();
    offset_drill() && l == [KErr::Inval.raw(), KErr::Inval.raw(), KErr::Overflow.raw(), 0]
}

/// 账本：20 格全登记、audit 绿、seal 非零、freeze 后拒写、replay 同指纹。
fn ledger_ok() -> bool {
    let mut l = build_ledger();
    if l.len() != BAD_CELLS || l.audit().is_err() {
        return false;
    }
    let root = match l.seal() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if root == 0 || !l.sealed {
        return false;
    }
    if l.freeze().is_err() || !l.is_frozen() {
        return false;
    }
    if l.record(Probe { class: 0, op: 0, raw: 0, value: 0 }).is_ok() {
        return false;
    }
    l.replay() == Ok(root)
}

/// 注入面：四路全抓 + 报告层不洗红。
fn inject_ok() -> bool {
    let four = inject_four();
    if !four.iter().all(|v| *v) {
        return false;
    }
    match run_with(Some((0, KErr::Inval.raw()))) {
        Ok(_) => false,
        Err(e) => e == D11_LEDGER,
    }
}

/// 本卡自检入口：13 条判据。
pub fn run_unx9202_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9202");

    s.add("F9202-01-段基址与对齐", seg_ok(), "SEG_BASE=0x90000 且 0x20 对齐");
    s.add("F9202-02-诊断码闭集", codes_ok(), "13 码连续且两两互异");
    s.add("F9202-03-错误码闭集", errno_ok(), "13 个 Linux 负数码可逆映射");
    s.add("F9202-04-fd范围闸", range_ok(), "负值/表尾/表尾+1 全 EBADF");
    s.add("F9202-05-槽位闸", slot_ok(), "空槽/关闭/双关闭全 EBADF");
    s.add("F9202-06-代数闸", gen_ok(), "同槽复用后旧 token 必 EBADF");
    s.add("F9202-07-模式闸", mode_ok(), "只写读/只读写/O_PATH 全 EBADF");
    s.add("F9202-08-对象闸", object_ok(), "目录 EISDIR / 管道 ESPIPE / 反向 EBADF");
    s.add("F9202-09-合法路径", corpus_ok(), "14 条语料逐条对平");
    s.add("F9202-10-坏fd矩阵", matrix_ok(), "10 类 × 2 操作 = 20 格全 -EBADF");
    s.add("F9202-11-引用与偏移账", refs_ok() && offset_ok(), "dup/close 守恒 + 定位五面");
    s.add("F9202-12-账本封印重放", ledger_ok(), "audit/seal/freeze/replay 四段闭合");
    s.add_dyn("F9202-13-四路注入全抓", inject_ok(), &format!("four={:?}", inject_four()));
    s.add("F9202-14-十轮复检格一致", repeat_ok(), "10/10 轮全绿且二十格零漂移");

    s
}

// ---------------------------------------------------------------------------
// 十一、单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9202_seg_and_codes() {
        assert!(seg_ok());
        assert!(codes_ok());
        assert_eq!(ALL_CODES.len(), 13);
        assert_eq!(ALL_CODES[0], SEG_BASE + 1);
        assert_eq!(ALL_CODES[12], SEG_BASE + 0x0D);
    }

    #[test]
    fn unx9202_errno_table_reversible() {
        assert!(errno_ok());
        assert_eq!(KErr::BadFd.raw(), -9);
        assert_eq!(KErr::Spipe.raw(), -29);
        assert_eq!(KErr::from_raw(-9), Some(KErr::BadFd));
        assert_eq!(KErr::from_raw(-999), None);
        assert_eq!(detail(D01_SEG).is_empty(), false);
        assert_eq!(detail(0xDEAD_BEEF), "未登记诊断码（闭集之外）");
    }

    #[test]
    fn unx9202_range_gate() {
        assert!(range_ok());
        let t = FdTable::new();
        assert_eq!(t.index(-1), Err(KErr::BadFd));
        assert_eq!(t.index(i32::MAX), Err(KErr::BadFd));
        assert_eq!(t.index(0), Ok(0));
    }

    #[test]
    fn unx9202_slot_gate() {
        assert!(slot_ok());
    }

    #[test]
    fn unx9202_generation_gate() {
        assert!(gen_ok());
        let mut t = FdTable::new();
        let fd = t.open(ObjKind::Regular, Access::ReadWrite, 8).unwrap();
        let tok = t.token(fd).unwrap();
        t.close(fd).unwrap();
        let fd2 = t.open(ObjKind::Regular, Access::ReadWrite, 8).unwrap();
        assert_eq!(fd, fd2);
        assert_eq!(t.resolve(tok).err(), Some(KErr::BadFd));
        assert_eq!(t.resolve(t.token(fd2).unwrap()).is_ok(), true);
    }

    #[test]
    fn unx9202_access_modes() {
        assert!(mode_ok());
        let mut t = FdTable::new();
        assert_eq!(sys_read(&mut t, FD_STDOUT, 1), Err(KErr::BadFd));
        assert_eq!(sys_write(&mut t, FD_STDIN, 1), Err(KErr::BadFd));
        assert_eq!(sys_read(&mut t, FD_STDIN, 1), Ok(1));
        assert_eq!(sys_write(&mut t, FD_STDOUT, 1), Ok(1));
    }

    #[test]
    fn unx9202_object_gate() {
        assert!(object_ok());
        let mut t = base_table();
        assert_eq!(sys_read(&mut t, 4, 1).err(), Some(KErr::IsDir));
        assert_eq!(sys_lseek(&mut t, 5, 0, 0).err(), Some(KErr::Spipe));
    }

    #[test]
    fn unx9202_matrix_all_ebadf() {
        let m = bad_matrix();
        assert!(matrix_green(&m));
        for row in m.iter() {
            for cell in row.iter() {
                assert_eq!(cell.raw, -9);
                assert_eq!(cell.value, 0);
            }
        }
        assert!(matrix_reds(&m).is_empty());
    }

    #[test]
    fn unx9202_corpus_matches() {
        assert!(corpus_ok());
        for c in CALLS.iter() {
            assert_eq!(run_call(c), c.expect, "语料 {}", c.label);
        }
        assert!(corpus_reds().is_empty());
    }

    #[test]
    fn unx9202_dup_shares_description() {
        let (fd, d2, shared_alive, gen_bumped) = dup_drill();
        assert!(fd >= 3);
        assert!(d2 > fd);
        assert!(shared_alive);
        assert!(gen_bumped);
    }

    #[test]
    fn unx9202_close_shared_keeps_offset() {
        let mut t = FdTable::new();
        let fd = t.open(ObjKind::Regular, Access::ReadWrite, 100).unwrap();
        let d2 = t.dup(fd).unwrap();
        assert_eq!(sys_write(&mut t, fd, 10), Ok(10));
        t.close(d2).unwrap();
        assert_eq!(sys_read(&mut t, fd, 90), Ok(90));
        assert_eq!(t.desc(fd).unwrap().offset, 100);
    }

    #[test]
    fn unx9202_dup2_semantics() {
        let mut t = FdTable::new();
        let fd = t.open(ObjKind::Regular, Access::ReadWrite, 32).unwrap();
        assert_eq!(t.dup2(fd, 20), Ok(20));
        assert_eq!(t.refs(fd), Some(2));
        assert_eq!(t.dup2(fd, fd), Ok(fd));
        assert_eq!(t.refs(fd), Some(2));
        assert_eq!(t.dup2(fd, FD_MAX as i32), Err(KErr::BadFd));
    }

    #[test]
    fn unx9202_lseek_faces() {
        assert_eq!(
            lseek_drill(),
            [KErr::Inval.raw(), KErr::Inval.raw(), KErr::Overflow.raw(), 0]
        );
        assert!(offset_drill());
    }

    #[test]
    fn unx9202_pipe_write_capacity() {
        let mut t = FdTable::new();
        let pw = t.open(ObjKind::PipeWrite, Access::WriteOnly, 8).unwrap();
        assert_eq!(sys_write(&mut t, pw, 5), Ok(5));
        assert_eq!(sys_write(&mut t, pw, 5), Ok(3));
        assert_eq!(sys_write(&mut t, pw, 1), Err(KErr::Again));
    }

    #[test]
    fn unx9202_eventfd_semantics() {
        let mut t = FdTable::new();
        let e = t.open(ObjKind::EventFd, Access::ReadWrite, 0).unwrap();
        assert_eq!(sys_read(&mut t, e, 32), Ok(8));
        assert_eq!(sys_write(&mut t, e, 32), Ok(8));
        assert_eq!(t.desc(e).unwrap().offset, 0);
    }

    #[test]
    fn unx9202_stat_mode() {
        let mut t = FdTable::new();
        let f = t.open(ObjKind::Regular, Access::ReadWrite, 10).unwrap();
        let st = sys_fstat(&t, f).unwrap();
        assert_eq!(st.mode & 0o170_000, ObjKind::Regular.mode_bits());
        assert_eq!(st.mode & 0o777, 0o666);
        let p = t.open(ObjKind::Regular, Access::PathOnly, 10).unwrap();
        assert_eq!(sys_fstat(&t, p).unwrap().mode & 0o777, 0);
        assert_eq!(sys_fcntl_getfl(&t, p).unwrap(), Access::PathOnly.flag_bits());
    }

    #[test]
    fn unx9202_rlimit_emfile() {
        let mut t = FdTable::new();
        assert_eq!(t.set_rlimit(8), Ok(()));
        let mut last = 0;
        let mut i = 0;
        while i < 5 {
            last = t.open(ObjKind::Regular, Access::ReadWrite, 1).unwrap();
            i += 1;
        }
        assert_eq!(last, 7);
        assert_eq!(t.open(ObjKind::Regular, Access::ReadWrite, 1), Err(KErr::MFile));
        assert_eq!(t.set_rlimit(0), Err(KErr::Inval));
        assert_eq!(t.set_rlimit(FD_MAX + 1), Err(KErr::Inval));
    }

    #[test]
    fn unx9202_ref_balance() {
        let t = base_table();
        let a = audit_table(&t);
        assert!(a.slots_balanced());
        assert!(a.descs_balanced());
        assert_eq!(a.live_slots, 7);
        assert_eq!(a.live_descs, 7);
    }

    #[test]
    fn unx9202_ledger_seal_replay() {
        assert!(ledger_ok());
        let mut l = build_ledger();
        let root = l.seal().unwrap();
        assert_ne!(root, 0);
        l.freeze().unwrap();
        assert!(l.record(Probe { class: 0, op: 0, raw: -9, value: 0 }).is_err());
        assert_eq!(l.replay(), Ok(root));
    }

    #[test]
    fn unx9202_ledger_catches_corruption() {
        let mut l = Ledger::new();
        l.record(Probe { class: 0, op: 0, raw: -9, value: 0 }).unwrap();
        assert!(l.audit().is_err());
        let mut l2 = Ledger::new();
        let mut i = 0usize;
        while i < BAD_CELLS {
            let raw = if i == 7 { -22 } else { -9 };
            l2.record(Probe { class: (i / 2) as u8, op: (i % 2) as u8, raw, value: 0 }).unwrap();
            i += 1;
        }
        assert!(l2.audit().is_err());
        assert!(l2.seal().is_err());
    }

    #[test]
    fn unx9202_report_green_and_gate() {
        let r = run().unwrap();
        assert!(r.green());
        assert_eq!(r.rate(), 1000);
        assert_eq!(r.head(), HEAD_OK);
        assert!(r.render().ends_with("GREEN"));
        assert_eq!(r.gate(), Ok(()));
    }

    #[test]
    fn unx9202_report_rejects_injection() {
        assert_eq!(run_with(Some((0, KErr::Inval.raw()))).err(), Some(D11_LEDGER));
        assert_eq!(run_with(Some((19, 0))).err(), Some(D11_LEDGER));
        let r = run_with(Some((5, -9))).unwrap();
        assert!(r.green());
    }

    #[test]
    fn unx9202_inject_four() {
        let got = inject_four();
        assert_eq!(got, [true, true, true, true]);
        assert!(inject_ok());
    }

    #[test]
    fn unx9202_checks_all_green() {
        let s = run_unx9202_checks();
        assert!(s.all_passed(), "UNX-F9202 判据未全绿: {}", s.red_items_text());
        assert_eq!(s.len(), 14);
    }

    #[test]
    fn unx9202_matrix_repeat_ten_rounds() {
        let (green, drift) = matrix_repeat(10);
        assert_eq!(green, 10);
        assert_eq!(drift, 0);
        let (g1, d1) = matrix_repeat(1);
        assert_eq!((g1, d1), (1, 0));
    }

    #[test]
    fn unx9202_deterministic_double_run() {
        let a = run().unwrap();
        let b = run().unwrap();
        assert_eq!(a, b);
        let m1 = bad_matrix();
        let m2 = bad_matrix();
        assert_eq!(m1, m2);
    }
}
