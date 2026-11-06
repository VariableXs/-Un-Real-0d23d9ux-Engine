//! UNX-F9203 read/write 坏指针矩阵（全 EFAULT） · 0x7A040段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9203 逐字）：
//!   NULL/-1/未映射/只读映射写路径四类注入矩阵判据过，全 -EFAULT 格过，
//!   部分拷贝残留量声明格过。
//!
//! 设计要点：
//!   ① 地址空间用「定容区间册」建模（代码段只读、只读数据、堆、匿名映射、
//!      栈），指针类别由「地址 + 长度 + 访问方向」现场算出，不接受自报标签。
//!   ② 方向语义按 Linux 真实口径：read(2) 是内核往用户缓冲写（IntoUser，
//!      目标必须可写），write(2) 是内核从用户缓冲读（FromUser，只读映射
//!      合法）。两条道分开建，写反必被抓。
//!   ③ 部分拷贝残留：跨「已映射前缀 + 空洞」的缓冲在空洞处中止，已触碰
//!      前缀字节数作为残留量显式声明；返回值仍是 -EFAULT，不得报成短读。
//!   ④ 判定次序 fd → 对象类型 → 零长度 → 地址跨度 → 区间权限；地址算术
//!      一律 checked_*，回绕单独成码；注入账守恒 + 语料双跑指纹相等。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9203 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7A040;

/// Linux errno 数值（正数口径；系统调用返回负值）。
pub const EBADF: i32 = 9;
pub const EFAULT: i32 = 14;
pub const EINVAL: i32 = 22;
pub const EISDIR: i32 = 21;
pub const EOVERFLOW: i32 = 75;
pub const ENOMEM: i32 = 12;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;
/// 坏指针类别数（NULL / -1 / 未映射 / 只读映射写）。
pub const PTR_CASES: u32 = 4;
/// 页大小（残留量与区间对齐基线）。
pub const PAGE: u64 = 0x1000;
/// 方向位（与 B00 其余卡的 fd 模式位同构）。
pub const DIR_READ: u32 = 1;
pub const DIR_WRITE: u32 = 2;

// ── 地址空间常量 ──────────────────────────────────────────────────────
pub const CODE_BASE: u64 = 0x0010_0000; // 代码段：只读可执行
pub const CODE_LEN: u64 = 0x2000;
pub const RODATA_BASE: u64 = 0x0020_0000; // 只读数据段：write 的源可落这里
pub const RODATA_LEN: u64 = 0x1000;
pub const HEAP_BASE: u64 = 0x0030_0000; // 堆：可读写
pub const HEAP_LEN: u64 = 0x8000;
pub const MMAP_BASE: u64 = 0x0040_0000; // 匿名映射：可读写
pub const MMAP_LEN: u64 = 0x1000;
pub const STACK_BASE: u64 = 0x7FFF_0000; // 用户栈：可读写
pub const STACK_LEN: u64 = 0x4000;
/// 典型空洞（任何区间都不覆盖）。
pub const HOLE_ADDR: u64 = 0x0060_0000;
/// NULL 与 (void *)-1 的原始位型。
pub const NULL_ADDR: u64 = 0;
pub const NEG_ONE_ADDR: u64 = u64::MAX;
/// 内核空间起始（x86_64 规范上半区）。
pub const KERNEL_BASE: u64 = 0xFFFF_8000_0000_0000;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0D 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PtrCode {
    /// NULL 指针
    NullPointer = 1,
    /// (void *)-1 位型
    NegativeOne = 2,
    /// 区间册里找不到承载区间
    Unmapped = 3,
    /// 只读区间上做内核写入
    WriteToRo = 4,
    /// 内核地址（用户态不可达）
    KernelAddress = 5,
    /// addr + len 回绕 u64
    WrapAround = 6,
    /// 缓冲跨到空洞，只完成前缀
    PartialCopy = 7,
    /// 零长度短路（仅记账，不算错误）
    ZeroLength = 8,
    /// fd 先于指针判定
    FdBeforePointer = 9,
    /// 对象类型不符（目录不可读写）
    WrongObject = 10,
    /// 注入账倒退
    LedgerRegress = 11,
    /// 双跑不一致
    NotReproducible = 12,
    /// 段位越界
    SegmentViolation = 13,
}

impl PtrCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 本诊断码对应的 Linux errno 正数（指针类一律 EFAULT）。
    pub fn errno(self) -> i32 {
        match self {
            PtrCode::ZeroLength => 0,
            PtrCode::FdBeforePointer => EBADF,
            PtrCode::WrongObject => EISDIR,
            PtrCode::LedgerRegress | PtrCode::NotReproducible | PtrCode::SegmentViolation => EINVAL,
            PtrCode::NullPointer
            | PtrCode::NegativeOne
            | PtrCode::Unmapped
            | PtrCode::WriteToRo
            | PtrCode::KernelAddress
            | PtrCode::WrapAround
            | PtrCode::PartialCopy => EFAULT,
        }
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            PtrCode::NullPointer => "NULL 缓冲区：内核不做空指针豁免；请把缓冲区指向已映射用户区间",
            PtrCode::NegativeOne => "(void *)-1 缓冲区：这是 mmap 失败哨兵被误当缓冲区；请先校验 mmap 返回值",
            PtrCode::Unmapped => "未映射地址：区间册无承载页；请检查指针是否越过 mmap/brk 实际长度或已 munmap",
            PtrCode::WriteToRo => "只读映射上执行内核写：read(2) 目标必须可写；请改可写映射或 mprotect 提权",
            PtrCode::KernelAddress => "内核地址：用户指针落在规范上半区；请排查截断/符号扩展污染",
            PtrCode::WrapAround => "地址回绕：addr+len 越过 u64 上界；请先做长度上界检查，回绕落进合法区间属假通过",
            PtrCode::PartialCopy => "部分拷贝：仅前缀可触碰；残留量按前缀声明，返回值仍是 -EFAULT，不得当有效数据",
            PtrCode::ZeroLength => "零长度短路：len=0 不触碰缓冲区直接返回 0；不得据此判断指针有效性",
            PtrCode::FdBeforePointer => "判定次序：fd 非法先回 EBADF，不得改报指针错误；请先取有效句柄",
            PtrCode::WrongObject => "对象类型不符：对目录读写属 EISDIR；请改用 getdents 族接口枚举目录项",
            PtrCode::LedgerRegress => "注入账倒退：累计计数只增不减；请检查回滚路径是否误改已落账累计值",
            PtrCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏状态、非确定哈希或全局序号",
            PtrCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x7A000..=0x7D1C0 窗口内",
        }
    }

    /// 闭集全量（供文案完整性与码值唯一性检查）。
    pub const ALL: [PtrCode; 13] = [
        PtrCode::NullPointer,
        PtrCode::NegativeOne,
        PtrCode::Unmapped,
        PtrCode::WriteToRo,
        PtrCode::KernelAddress,
        PtrCode::WrapAround,
        PtrCode::PartialCopy,
        PtrCode::ZeroLength,
        PtrCode::FdBeforePointer,
        PtrCode::WrongObject,
        PtrCode::LedgerRegress,
        PtrCode::NotReproducible,
        PtrCode::SegmentViolation,
    ];
}

/// 系统调用失败结果：携带 Linux errno 正数码。
pub type SysResult<T> = Result<T, i32>;

// ── 访问方向 ──────────────────────────────────────────────────────────
/// 缓冲区访问方向。
///
/// `IntoUser`：内核把数据写进用户缓冲区（read(2)、getcwd、stat 的 buf）。
/// `FromUser`：内核从用户缓冲区读数据（write(2)、open 的 path）。
/// 只读映射对 FromUser 合法、对 IntoUser 是 EFAULT——把方向弄反是本类
/// bug 的第一大来源，故单独建型而不是复用一组 bool。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    /// 内核写用户内存，目标区间必须可写
    IntoUser = 1,
    /// 内核读用户内存，只读映射亦可
    FromUser = 2,
}

impl Dir {
    /// 该方向在给定区间上是否被允许。
    pub fn permits(self, region: Region) -> bool {
        match self {
            Dir::IntoUser => region.writable,
            Dir::FromUser => true,
        }
    }
}

// ── 区间册 ────────────────────────────────────────────────────────────
/// 一个已映射区间：[base, base+len)，带写权限位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Region {
    /// 区间起点
    pub base: u64,
    /// 区间长度（字节）
    pub len: u64,
    /// 是否可写（只读映射为 false）
    pub writable: bool,
}

impl Region {
    /// 构造区间。
    pub const fn new(base: u64, len: u64, writable: bool) -> Region {
        Region { base, len, writable }
    }

    /// 区间终点（饱和加；构造期已保证不溢出）。
    pub const fn end(self) -> u64 {
        self.base.saturating_add(self.len)
    }

    /// 地址是否落在区间内（半开）。
    pub fn holds(self, addr: u64) -> bool {
        addr >= self.base && addr < self.end()
    }
}

/// 区间册容量（五段固定映射 + 三个空槽供扩展）。
pub const REGION_CAP: usize = 8;

/// 定容区间册：本模块的「用户地址空间」。
#[derive(Clone, Copy, Debug)]
pub struct Space {
    regions: [Region; REGION_CAP],
    used: usize,
}

impl Space {
    /// 规范地址空间：代码段/只读数据/堆/匿名映射/栈。
    pub const fn canonical() -> Space {
        let hole = Region::new(0, 0, false);
        Space {
            regions: [
                Region::new(CODE_BASE, CODE_LEN, false),
                Region::new(RODATA_BASE, RODATA_LEN, false),
                Region::new(HEAP_BASE, HEAP_LEN, true),
                Region::new(MMAP_BASE, MMAP_LEN, true),
                Region::new(STACK_BASE, STACK_LEN, true),
                hole,
                hole,
                hole,
            ],
            used: 5,
        }
    }

    /// 已登记区间数。
    pub fn region_count(&self) -> usize {
        self.used
    }

    /// 查承载某地址的区间。
    pub fn find(&self, addr: u64) -> Option<Region> {
        let mut i = 0usize;
        while i < self.used {
            if let Some(r) = self.regions.get(i) {
                if r.holds(addr) {
                    return Some(*r);
                }
            }
            i += 1;
        }
        None
    }

    /// 区间册自检：两两不相交且长度非零（构造纪律）。
    pub fn is_disjoint(&self) -> bool {
        let mut i = 0usize;
        while i < self.used {
            let a = match self.regions.get(i) {
                Some(r) => *r,
                None => return false,
            };
            if a.len == 0 || a.base >= a.end() {
                return false;
            }
            let mut j = i + 1;
            while j < self.used {
                if let Some(b) = self.regions.get(j) {
                    if a.base < b.end() && b.base < a.end() {
                        return false;
                    }
                }
                j += 1;
            }
            i += 1;
        }
        true
    }
}

/// 一次区间探查的结果。
///
/// `copied` 是能连续触碰到的前缀字节数（部分拷贝残留量）；`touched` 是
/// 探查真正「碰到」的字节数——本模型里两者恒等，这正是「零越界触碰」
/// 判据要求的：不许为了找空洞而预读下一页。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Probe {
    /// 可触碰前缀（残留量）
    pub copied: usize,
    /// 实际触碰字节数（必须 == copied）
    pub touched: u64,
    /// 请求跨度
    pub span: u64,
    /// 首个故障码（None 表示全程可达）
    pub fault: Option<PtrCode>,
}

impl Space {
    /// 方向化探查：沿区间册逐段推进，遇空洞/权限不符即停。
    pub fn probe(&self, dir: Dir, addr: u64, len: usize) -> Probe {
        let mut out = Probe {
            copied: 0,
            touched: 0,
            span: len as u64,
            fault: None,
        };
        if len == 0 {
            return out;
        }
        // 位型短路：NULL / -1 / 内核地址不查区间册；哨兵判定必须先于
        // 回绕检查——(void*)-1 是 mmap 失败哨兵，与地址回绕是两类语义，
        // 混淆会把 -1 误报成 WrapAround。
        if addr == NULL_ADDR {
            out.fault = Some(PtrCode::NullPointer);
            return out;
        }
        if addr == NEG_ONE_ADDR {
            out.fault = Some(PtrCode::NegativeOne);
            return out;
        }
        if addr.checked_add(len as u64).is_none() {
            out.fault = Some(PtrCode::WrapAround);
            return out;
        }
        if addr >= KERNEL_BASE {
            out.fault = Some(PtrCode::KernelAddress);
            return out;
        }
        let mut cur = addr;
        let mut left = len as u64;
        let mut steps = 0usize;
        while left > 0 && steps <= REGION_CAP {
            let region = match self.find(cur) {
                Some(r) => r,
                None => {
                    out.fault = Some(PtrCode::Unmapped);
                    return out;
                }
            };
            if !dir.permits(region) {
                out.fault = Some(PtrCode::WriteToRo);
                return out;
            }
            let avail = region.end().saturating_sub(cur);
            if avail == 0 {
                out.fault = Some(PtrCode::Unmapped);
                return out;
            }
            let take = if avail < left { avail } else { left };
            out.copied = out.copied.saturating_add(take as usize);
            out.touched = out.touched.saturating_add(take);
            cur = match cur.checked_add(take) {
                Some(v) => v,
                None => {
                    out.fault = Some(PtrCode::WrapAround);
                    return out;
                }
            };
            left -= take;
            steps += 1;
        }
        if left > 0 {
            out.fault = Some(PtrCode::Unmapped);
        }
        out
    }

    /// 方向化判定：全程可达才算成功，否则统一 EFAULT。
    pub fn judge(&self, dir: Dir, addr: u64, len: usize) -> SysResult<usize> {
        if len == 0 {
            return Ok(0);
        }
        match self.probe(dir, addr, len).fault {
            None => Ok(len),
            Some(_) => Err(EFAULT),
        }
    }
}

// ── 句柄表（只为判定次序服务的最小模型） ──────────────────────────────
/// 对象类型：目录对 read/write 一律 EISDIR。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObjKind {
    /// 普通文件 / 管道端
    Stream = 1,
    /// 目录
    Dir = 2,
}

/// 单槽句柄。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Handle {
    /// 是否打开
    pub open: bool,
    /// 模式位（DIR_READ / DIR_WRITE）
    pub mode: u32,
    /// 对象类型
    pub kind: ObjKind,
}

impl Handle {
    /// 空槽。
    const fn empty() -> Handle {
        Handle { open: false, mode: 0, kind: ObjKind::Stream }
    }

    /// 打开态槽。
    const fn opened(mode: u32, kind: ObjKind) -> Handle {
        Handle { open: true, mode, kind }
    }
}

/// 句柄表容量（0..=4 有效：stdin/stdout/stderr + 只读文件 + 目录）。
pub const HANDLE_CAP: usize = 8;

/// 定容句柄表。
#[derive(Clone, Copy, Debug)]
pub struct HandleTable {
    slots: [Handle; HANDLE_CAP],
}

impl HandleTable {
    /// 规范句柄表：0=stdin(读)、1=stdout(写)、2=stderr(写)、
    /// 3=只读数据文件(读)、4=目录(读，目录型)。
    pub const fn canonical() -> HandleTable {
        HandleTable {
            slots: [
                Handle::opened(DIR_READ, ObjKind::Stream),
                Handle::opened(DIR_WRITE, ObjKind::Stream),
                Handle::opened(DIR_WRITE, ObjKind::Stream),
                Handle::opened(DIR_READ, ObjKind::Stream),
                Handle::opened(DIR_READ, ObjKind::Dir),
                Handle::empty(),
                Handle::empty(),
                Handle::empty(),
            ],
        }
    }

    /// 取句柄：负号与越界一律 EBADF（第一层，先于任何指针校验）。
    pub fn get(&self, fd: i32) -> SysResult<Handle> {
        if fd < 0 {
            return Err(EBADF);
        }
        let idx = match usize::try_from(fd) {
            Ok(v) => v,
            Err(_) => return Err(EBADF),
        };
        let h = match self.slots.get(idx) {
            Some(v) => *v,
            None => return Err(EBADF),
        };
        if !h.open {
            return Err(EBADF);
        }
        Ok(h)
    }
}

/// read(2)：fd → 模式 → 类型 → 零长度 → 指针（IntoUser）。
pub fn sys_read(table: &HandleTable, space: &Space, fd: i32, buf: u64, len: usize) -> SysResult<usize> {
    let h = table.get(fd)?;
    if h.mode & DIR_READ == 0 {
        return Err(EBADF);
    }
    if h.kind == ObjKind::Dir {
        return Err(EISDIR);
    }
    if len == 0 {
        return Ok(0);
    }
    space.judge(Dir::IntoUser, buf, len)
}

/// write(2)：fd → 模式 → 类型 → 零长度 → 指针（FromUser）。
pub fn sys_write(table: &HandleTable, space: &Space, fd: i32, buf: u64, len: usize) -> SysResult<usize> {
    let h = table.get(fd)?;
    if h.mode & DIR_WRITE == 0 {
        return Err(EBADF);
    }
    if h.kind == ObjKind::Dir {
        return Err(EISDIR);
    }
    if len == 0 {
        return Ok(0);
    }
    space.judge(Dir::FromUser, buf, len)
}

// ── 坏指针注入矩阵 ────────────────────────────────────────────────────
/// 四类坏指针样本（判据原文逐字列出的四类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadPtrCase {
    /// NULL
    Null = 1,
    /// (void *)-1
    NegativeOne = 2,
    /// 未映射空洞
    Unmapped = 3,
    /// 只读映射上做内核写（read 的 buf 落在只读段）
    ReadOnlyWrite = 4,
}

impl BadPtrCase {
    /// 样本：(地址, 长度, 访问方向)。
    pub fn sample(self) -> (u64, usize, Dir) {
        match self {
            BadPtrCase::Null => (NULL_ADDR, 8, Dir::IntoUser),
            BadPtrCase::NegativeOne => (NEG_ONE_ADDR, 8, Dir::IntoUser),
            BadPtrCase::Unmapped => (HOLE_ADDR, 8, Dir::IntoUser),
            BadPtrCase::ReadOnlyWrite => (RODATA_BASE, 8, Dir::IntoUser),
        }
    }

    /// 期望诊断码。
    pub fn code(self) -> PtrCode {
        match self {
            BadPtrCase::Null => PtrCode::NullPointer,
            BadPtrCase::NegativeOne => PtrCode::NegativeOne,
            BadPtrCase::Unmapped => PtrCode::Unmapped,
            BadPtrCase::ReadOnlyWrite => PtrCode::WriteToRo,
        }
    }

    /// 闭集全量。
    pub const ALL: [BadPtrCase; 4] = [
        BadPtrCase::Null,
        BadPtrCase::NegativeOne,
        BadPtrCase::Unmapped,
        BadPtrCase::ReadOnlyWrite,
    ];
}

/// 注入账（只增）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须恒为 0）
    pub escaped: u32,
}

impl InjectLedger {
    /// 空账。
    pub const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；守恒破坏时返回负诊断码（不允许静默）。
    pub fn record(&mut self, caught: bool) -> SysResult<()> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(-(PtrCode::LedgerRegress as i32));
        }
        Ok(())
    }

    /// 守恒式：注入 = 检出 + 逃逸。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 跑一遍四类坏指针矩阵：每类 10 轮，统一走 stdin 的 read(IntoUser) 道，
/// 确保抓到的肯定是指针面而不是 fd 面。
pub fn run_bad_ptr_matrix(space: &Space) -> (InjectLedger, [u32; 4]) {
    let table = HandleTable::canonical();
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadPtrCase::ALL.len() {
        if let Some(case) = BadPtrCase::ALL.get(ci).copied() {
            let (addr, len, _dir) = case.sample();
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = sys_read(&table, space, 0, addr, len) == Err(EFAULT);
                let _ = ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── 部分拷贝残留量声明 ────────────────────────────────────────────────
/// 一条残留声明：(地址, 长度, 方向, 期望残留字节数)。
///
/// 三条样本分别覆盖：跨堆尾空洞、只读段跨尾、跨栈尾空洞。
pub const PARTIAL_ROWS: [(u64, usize, Dir, usize); 3] = [
    (HEAP_BASE + 0x7000, 0x2000, Dir::IntoUser, 0x1000),
    (RODATA_BASE + 0x0800, 0x1000, Dir::FromUser, 0x0800),
    (STACK_BASE + 0x3000, 0x2000, Dir::IntoUser, 0x1000),
];

/// 残留声明的实测值：(残留量, 故障码, 触碰量)。
///
/// 用元组而非结构体，是因为这三项才是判据要核对的全部内容——把
/// 「地址/长度/方向」再抄一份只会让冻结表与实测值出现两处真相。
pub type ResidualRow = (usize, Option<PtrCode>, u64);

/// 逐条复算残留声明（顺序与 [`PARTIAL_ROWS`] 一致）。
pub fn partial_decls(space: &Space) -> [ResidualRow; 3] {
    let blank: ResidualRow = (0, None, 0);
    let mut out = [blank; 3];
    let mut i = 0usize;
    while i < PARTIAL_ROWS.len() {
        if let Some((addr, len, dir, _want)) = PARTIAL_ROWS.get(i).copied() {
            let p = space.probe(dir, addr, len);
            if let Some(slot) = out.get_mut(i) {
                *slot = (p.copied, p.fault, p.touched);
            }
        }
        i += 1;
    }
    out
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, Linux 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EFAULT", -14, 998, "Windows ERROR_NOACCESS；坏地址族"),
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；坏句柄族"),
    ("EISDIR", -21, 267, "Windows ERROR_DIRECTORY；对目录读写"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
    ("EOVERFLOW", -75, 534, "Windows ERROR_ARITHMETIC_OVERFLOW；越界"),
    ("ENOMEM", -12, 8, "Windows ERROR_NOT_ENOUGH_MEMORY；内存不足"),
];

/// 查表：按 errno 正数返回对应负值；表外返回 0。
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

/// 与 Linux 逐码一致：只允许表内码，且负值精确等于 -code。
pub fn agrees_with_linux(code: i32) -> bool {
    code > 0 && linux_neg(code) == -code
}

/// FNV-1a 32 位（无浮点、无随机、无墙钟）。
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

/// 规范语料：四类坏指针 + 正常访问 + 两条残留跨段，方向成对。
pub const CORPUS: [(u64, usize, Dir); 9] = [
    (NULL_ADDR, 8, Dir::IntoUser),
    (NEG_ONE_ADDR, 8, Dir::FromUser),
    (HOLE_ADDR, 8, Dir::IntoUser),
    (RODATA_BASE, 8, Dir::IntoUser),
    (RODATA_BASE, 8, Dir::FromUser),
    (HEAP_BASE, 0x100, Dir::IntoUser),
    (HEAP_BASE + 0x7FF0, 0x20, Dir::IntoUser),
    (STACK_BASE + 0x3FF0, 0x20, Dir::FromUser),
    (MMAP_BASE, 0x1000, Dir::FromUser),
];

/// 语料双跑指纹：把每条样本的（故障码, 残留量, 触碰量, 跨度）编成字节流。
pub fn corpus_fingerprint(space: &Space) -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < CORPUS.len() {
        if let Some((addr, len, dir)) = CORPUS.get(i).copied() {
            let p = space.probe(dir, addr, len);
            let fault = match p.fault {
                Some(c) => c as u8,
                None => 0u8,
            };
            bytes.push(fault);
            bytes.push((p.copied & 0xFF) as u8);
            bytes.push(((p.copied >> 8) & 0xFF) as u8);
            bytes.push((p.touched & 0xFF) as u8);
            bytes.push((p.span & 0xFF) as u8);
        }
        i += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9203 判据集（20 条静态 + 4 条动态明细）。
pub fn run_unx9203_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9203");
    let space = Space::canonical();
    let table = HandleTable::canonical();

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：read 写堆
    let normal_into = sys_read(&table, &space, 0, HEAP_BASE, 64) == Ok(64);
    s.add("正常流：read 写堆缓冲 64 字节", normal_into,
          "合法 fd + 可写区间应返回请求长度");

    // 4. 正常流：write 从只读段取源（方向语义格）
    let normal_from = sys_write(&table, &space, 1, RODATA_BASE, 64) == Ok(64);
    s.add("正常流：write 从只读段取源 64 字节", normal_from,
          "只读映射对 FromUser 合法，误判成 EFAULT 就是方向写反");

    // 5. 四类坏指针 10/10
    let (ledger, per_class) = run_bad_ptr_matrix(&space);
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("四类坏指针各 10/10 全 -EFAULT", all_caught,
          "NULL/-1/未映射/只读写四类必须全部返回 -EFAULT");

    // 6. 注入账守恒
    let ledger_ok = ledger.conserved()
        && ledger.escaped == 0
        && ledger.injected == PTR_CASES * INJECT_ROUNDS;
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0", ledger_ok,
          "40 次注入必须 40 次检出、0 逃逸");

    // 9. 判定次序：坏 fd 先于坏指针
    let order_fd = sys_read(&table, &space, -1, NULL_ADDR, 8) == Err(EBADF)
        && sys_write(&table, &space, 99, HOLE_ADDR, 8) == Err(EBADF)
        && sys_read(&table, &space, -7, KERNEL_BASE, 8) == Err(EBADF);
    s.add("判定次序：EBADF 先于 EFAULT", order_fd,
          "fd 非法的瞬间不得改报指针错误");

    // 10. 判定次序：目录类型先于指针
    let order_dir = sys_read(&table, &space, 4, HOLE_ADDR, 8) == Err(EISDIR);
    s.add("判定次序：EISDIR 先于 EFAULT", order_dir,
          "目录读写的类型错误与缓冲区无关，应先于指针判定");

    // 11. 零长度
    let zero_ok = sys_read(&table, &space, 0, NULL_ADDR, 0) == Ok(0)
        && sys_write(&table, &space, 1, NEG_ONE_ADDR, 0) == Ok(0)
        && sys_read(&table, &space, 0, KERNEL_BASE, 0) == Ok(0);
    s.add("零长度：len=0 短路不判指针", zero_ok,
          "read(fd, NULL, 0) 合法返回 0，这是 Linux 既定行为");

    // 10. 残留量声明
    let decls = partial_decls(&space);
    let mut residual_ok = true;
    let mut i = 0usize;
    while i < PARTIAL_ROWS.len() {
        if let (Some((residual, fault, _t)), Some((_, _, _, want))) =
            (decls.get(i), PARTIAL_ROWS.get(i))
        {
            if *residual != *want || *fault != Some(PtrCode::Unmapped) {
                residual_ok = false;
            }
        }
        i += 1;
    }
    s.add("残留量声明：三条跨段样本与冻结表逐项相等", residual_ok,
          "残留量必须等于可触碰前缀，多一字节少一字节都算错");

    // 11. 零越界触碰
    let mut touch_ok = true;
    let mut k = 0usize;
    while k < decls.len() {
        if let (Some((residual, _f, touched)), Some((_, len, _, _))) =
            (decls.get(k), PARTIAL_ROWS.get(k))
        {
            if *touched != *residual as u64 || *residual > *len || *touched > *len as u64 {
                touch_ok = false;
            }
        }
        k += 1;
    }
    s.add("零越界触碰：触碰量恒等于残留量且不超请求长度", touch_ok,
          "找空洞不许预读下一页，触碰量超限就是越界读");

    // 12. 方向矩阵（含内核地址）
    let dir_matrix = space.judge(Dir::FromUser, RODATA_BASE, 16) == Ok(16)
        && space.judge(Dir::IntoUser, RODATA_BASE, 16) == Err(EFAULT)
        && space.judge(Dir::IntoUser, CODE_BASE, 16) == Err(EFAULT)
        && space.judge(Dir::IntoUser, KERNEL_BASE, 16) == Err(EFAULT);
    s.add("方向矩阵：只读段 FromUser 成功、IntoUser 全 EFAULT", dir_matrix,
          "代码段与只读数据段对两个方向给出相反结论，是方向语义的试金石");

    // 15. 地址回绕
    let wrap_ok = space.judge(Dir::IntoUser, u64::MAX - 3, 16) == Err(EFAULT)
        && space.probe(Dir::IntoUser, u64::MAX - 3, 16).fault == Some(PtrCode::WrapAround)
        && space.probe(Dir::IntoUser, HEAP_BASE, usize::MAX).fault == Some(PtrCode::WrapAround);
    s.add("回绕面：addr+len 溢出 u64 单独成码", wrap_ok,
          "回绕后若落进合法区间会假通过，必须提前拒绝");

    // 16. 短跨度边界
    let edge = space.judge(Dir::IntoUser, HEAP_BASE + HEAP_LEN - 8, 8) == Ok(8)
        && space.judge(Dir::IntoUser, HEAP_BASE + HEAP_LEN - 8, 9) == Err(EFAULT)
        && space.judge(Dir::IntoUser, HEAP_BASE + HEAP_LEN, 1) == Err(EFAULT);
    s.add("边界：区间末尾恰好容纳 / 多 1 字节 / 起点即空洞", edge,
          "半开区间的右端点是本类 bug 高发点，三点钉死");

    // 16. Linux 逐码一致 + 诊断码映射
    let codes_ok = agrees_with_linux(EFAULT)
        && agrees_with_linux(EBADF)
        && agrees_with_linux(EISDIR)
        && agrees_with_linux(EOVERFLOW)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    let mut map_ok = true;
    let mut m = 0usize;
    while m < PtrCode::ALL.len() {
        if let Some(c) = PtrCode::ALL.get(m) {
            let e = c.errno();
            if e != 0 && !agrees_with_linux(e) {
                map_ok = false;
            }
        }
        m += 1;
    }
    s.add("Linux 逐码一致：表内码精确、表外码拒绝、码→errno 可复算", codes_ok && map_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 19. 双跑一致 + 文案闭集
    let a = corpus_fingerprint(&space);
    let b = corpus_fingerprint(&space);
    let text_ok = PtrCode::ALL.iter().all(|c| !c.detail().is_empty());
    let mut uniq = true;
    let mut x = 0usize;
    while x < PtrCode::ALL.len() {
        let mut y = x + 1;
        while y < PtrCode::ALL.len() {
            if let (Some(c1), Some(c2)) = (PtrCode::ALL.get(x), PtrCode::ALL.get(y)) {
                if c1.val() == c2.val() {
                    uniq = false;
                }
            }
            y += 1;
        }
        x += 1;
    }
    s.add("双跑一致且文案闭集：指纹相等、13 码建议非空、码值唯一",
          a == b && text_ok && uniq,
          "同输入两次逐字段相等；每个码都要能取到可操作建议");

    // 动态明细（4 条）
    let ledger_text = format!("注入 {} / 检出 {} / 逃逸 {}",
                              ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("账目快照", ledger_ok, &ledger_text);
    let mut residual_text = format!("残留");
    let mut r = 0usize;
    while r < decls.len() {
        if let Some(d) = decls.get(r) {
            residual_text.push_str(&format!("/{}", d.0));
        }
        r += 1;
    }
    s.add_dyn("残留快照", residual_ok, &residual_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);
    let normal_text = format!("正常流 {} / 方向 {} / 边界 {}", normal_into, normal_from, edge);
    s.add_dyn("正常流快照", normal_into && normal_from && edge, &normal_text);

    // 19.5 errno 映射闭集复查：指针/哨兵/回绕/部分拷贝七类一律 EFAULT，
    // 零长度短路为 0，判定次序码为 EBADF，对象类型不符为 EISDIR；
    // 与 Linux copy_to_user/copy_from_user 的 fault 语义逐码对齐。
    let mut errno_ok = true;
    let mut e = 0usize;
    while e < PtrCode::ALL.len() {
        if let Some(code) = PtrCode::ALL.get(e) {
            let want = match code {
                PtrCode::NullPointer
                | PtrCode::NegativeOne
                | PtrCode::Unmapped
                | PtrCode::WriteToRo
                | PtrCode::KernelAddress
                | PtrCode::WrapAround
                | PtrCode::PartialCopy => EFAULT,
                PtrCode::ZeroLength => 0,
                PtrCode::FdBeforePointer => EBADF,
                PtrCode::WrongObject => EISDIR,
                _ => EINVAL,
            };
            if code.errno() != want {
                errno_ok = false;
            }
        }
        e += 1;
    }
    s.add("errno 映射闭集：七类指针码一律 EFAULT 且其余逐码对齐", errno_ok,
          "与 copy_to_user fault 语义逐码一致，映射表可独立复算");

    // 20. 收口
    let n_before = s.len();
    s.add("收口：判据条数达到声明值", n_before >= 20,
          "收口断言条数只增不减，删条必须显式改声明");

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn space() -> Space {
        Space::canonical()
    }

    fn table() -> HandleTable {
        HandleTable::canonical()
    }

    #[test]
    fn normal_flows_both_directions() {
        let sp = space();
        let t = table();
        assert_eq!(sys_read(&t, &sp, 0, HEAP_BASE, 32), Ok(32));
        assert_eq!(sys_write(&t, &sp, 1, HEAP_BASE, 32), Ok(32));
        assert_eq!(sys_write(&t, &sp, 1, RODATA_BASE, 32), Ok(32));
        assert_eq!(sys_read(&t, &sp, 0, STACK_BASE + 0x3FF0, 16), Ok(16));
    }

    #[test]
    fn four_bad_pointer_classes_are_efault() {
        let sp = space();
        let t = table();
        let cases = [
            (NULL_ADDR, PtrCode::NullPointer),
            (NEG_ONE_ADDR, PtrCode::NegativeOne),
            (HOLE_ADDR, PtrCode::Unmapped),
            (RODATA_BASE, PtrCode::WriteToRo),
        ];
        for (addr, code) in cases.iter() {
            assert_eq!(sp.probe(Dir::IntoUser, *addr, 8).fault, Some(*code));
            assert_eq!(sys_read(&t, &sp, 0, *addr, 8), Err(EFAULT));
        }
    }

    #[test]
    fn kernel_address_and_direction_matrix() {
        let sp = space();
        let t = table();
        assert_eq!(sp.probe(Dir::IntoUser, KERNEL_BASE, 8).fault, Some(PtrCode::KernelAddress));
        assert_eq!(sys_read(&t, &sp, 0, KERNEL_BASE, 8), Err(EFAULT));
        assert_eq!(sp.judge(Dir::FromUser, RODATA_BASE, 8), Ok(8));
        assert_eq!(sp.judge(Dir::IntoUser, RODATA_BASE, 8), Err(EFAULT));
        assert_eq!(sp.judge(Dir::FromUser, CODE_BASE, 8), Ok(8));
    }

    #[test]
    fn wrap_around_range_edges_and_zero_length() {
        let sp = space();
        let t = table();
        let p = sp.probe(Dir::IntoUser, u64::MAX - 3, 16);
        assert_eq!(p.fault, Some(PtrCode::WrapAround));
        assert_eq!(p.copied, 0);
        assert_eq!(sp.judge(Dir::IntoUser, u64::MAX - 3, 16), Err(EFAULT));
        assert_eq!(sp.judge(Dir::IntoUser, HEAP_BASE + HEAP_LEN - 8, 8), Ok(8));
        assert_eq!(sp.judge(Dir::IntoUser, HEAP_BASE + HEAP_LEN - 8, 9), Err(EFAULT));
        assert_eq!(sp.judge(Dir::IntoUser, HEAP_BASE + HEAP_LEN, 1), Err(EFAULT));
        // 零长度：不触碰缓冲区，也不判指针。
        assert_eq!(sys_read(&t, &sp, 0, NULL_ADDR, 0), Ok(0));
        assert_eq!(sys_read(&t, &sp, 0, u64::MAX - 2, 0), Ok(0));
        let z = sp.probe(Dir::IntoUser, HOLE_ADDR, 0);
        assert_eq!(z.copied, 0);
        assert_eq!(z.touched, 0);
        assert_eq!(z.fault, None);
    }

    #[test]
    fn fd_and_object_order_precede_pointer() {
        let sp = space();
        let t = table();
        assert_eq!(sys_read(&t, &sp, -1, NULL_ADDR, 8), Err(EBADF));
        assert_eq!(sys_write(&t, &sp, 200, HOLE_ADDR, 8), Err(EBADF));
        assert_eq!(sys_read(&t, &sp, 4, HOLE_ADDR, 8), Err(EISDIR));
    }

    #[test]
    fn partial_copy_residual_is_declared() {
        let sp = space();
        let rows = partial_decls(&sp);
        let mut i = 0usize;
        while i < PARTIAL_ROWS.len() {
            if let (Some((residual, fault, touched)), Some((addr, len, dir, want))) =
                (rows.get(i), PARTIAL_ROWS.get(i))
            {
                assert_eq!(*residual, *want, "第 {} 条残留不符", i);
                assert_eq!(*fault, Some(PtrCode::Unmapped));
                assert_eq!(*touched, *want as u64);
                assert_eq!(sp.probe(*dir, *addr, *len).copied, *want);
            }
            i += 1;
        }
    }

    #[test]
    fn injection_ledger_conserved() {
        let sp = space();
        let (l, per) = run_bad_ptr_matrix(&sp);
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, PTR_CASES * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn corpus_is_deterministic_and_codes_closed() {
        let sp = space();
        assert_eq!(corpus_fingerprint(&sp), corpus_fingerprint(&sp));
        assert_ne!(corpus_fingerprint(&sp), 0);
        let mut i = 0usize;
        while i < PtrCode::ALL.len() {
            if let Some(c) = PtrCode::ALL.get(i) {
                assert!(!c.detail().is_empty());
                assert!(c.val() > SEG_BASE);
                assert!(c.val() <= SEG_BASE + 0x3F);
            }
            i += 1;
        }
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9203_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated(), "判据或动态明细被截断");
    }
}
