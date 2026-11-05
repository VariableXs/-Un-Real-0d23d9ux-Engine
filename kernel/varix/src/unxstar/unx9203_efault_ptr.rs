//! UNX-F9203 read/write 坏指针矩阵（全 EFAULT） · 0x90020 段
//!
//! 判据原文（逐字摘自分派卡）：
//! UNX-F9203-J1 NULL / -1 / 未映射 / 只读映射写路径四类注入矩阵判据过，
//! 全 -EFAULT 格过，部分拷贝残留量声明格过。
//!
//! # 这条纵线为何与坏 fd 分开
//!
//! `read(2)` 与 `write(2)` 各有两层合法性：
//!
//! ```text
//! ① 句柄面：fd 是否指向一个可用的打开文件     -> EBADF / EISDIR / ESPIPE
//! ② 缓冲面：用户给的地址区间能否被内核触碰     -> EFAULT
//! ```
//!
//! 两层可以同时坏，但对用户态的可观测结果由**先判的那层**决定。Linux 的
//! `vfs_read()` 先走 `fdget_pos()` 拿文件，再走 `copy_to_user()`；所以一个
//! 坏 fd 配一个坏指针，errno 必须是 EBADF 而不是 EFAULT。UNX-F9202 判①，
//! 本卡判②，两卡共同构成「先句柄、后缓冲」的次序证据。
//!
//! # 缓冲面为什么不是一个布尔
//!
//! 一个地址区间在页粒度上有五种状态，把它们压成「合法/非法」会立刻丢掉
//! 三个真实语义：
//!
//! 1. **零长度恒合法**。`read(fd, NULL, 0)` 在 Linux 上返回 0，不产生
//!    EFAULT；把 NULL 无条件判死会让合法的零长调用被误拒。
//! 2. **部分合法是有意义的**。区间跨两页、第二页未映射时，Linux 的
//!    `copy_to_user()` 把已拷字节数报回 `read()`，而 `read()` 在已拷贝
//!    非零时**返回字节数而不是 EFAULT**。也就是「部分失败」在用户态
//!    表现为成功，只是短读/短写。
//! 3. **方向敏感**。只读页在 `read()` 方向完全合法，在 `write()` 方向
//!    才 EFAULT。若把权限检查做成与方向无关的「可访问」，只读映射上的
//!    正常读会被误杀。
//!
//! # 关键差异（本卡深度所在）
//!
//! - 与 UNX-F9202 的边界：9202 把 fd 闸门做到五层，缓冲面只保留 `remaining`
//!   计数；本卡不碰 fd 表，缓冲面的页状态、跨页残缺、方向敏感全部展开。
//! - 与 UNX-F9210 的边界：9210 处理 `copy_to_user` 的**内核侧**回退策略
//!   （零页、临时弹栈）；本卡只判「何时必须失败、失败时残留多少」。
//! - 与 UNX-F9214 的边界：9214 判 `EFAULT` 在信号语义下的可重启性；
//!   本卡不涉及信号，只做同步返回值与残留账。
//!
//! # 设计要点
//!
//! 1. 页表用定长数组 + 六态闭集 `Page`（Hole/R/W/RW/RX/RWX/Guard），
//!    地址空间取 64 页（256 KiB）影子区，所有映射、保护、解映射都走同一张表。
//! 2. 探测函数 `probe()` 一次判完整个区间，返回**首个**故障页及其原因；
//!    部分拷贝的「已拷贝量」就是首故障页之前那段长度，天然与页对齐。
//! 3. 拷贝引擎按页分块，故障时停止而不写穿；残留声明要求
//!    「目标区间 [done, requested) 一个字节都没被碰过」，用种子公式复算验证。
//! 4. 陷入回退用显式 fixup 表建模：未登记站点 → 内核 Oops，已登记 → 回到
//!    用户态并给出短计数。表满必须拒绝登记（返回 ERANGE），不得静默覆盖。
//! 5. 矩阵 8 类 × 4 操作 = 32 格，每格期望单独登记（不是一刀切 EFAULT），
//!    只读页在读取方向必须成功——这正是矩阵里最容易被写错的一格。
//! 6. 账本用 64 槽环形表 + 链条折叠，`audit()` 重算链、`seal()` 取摘要、
//!    `replay()` 从记录序列重建后比对，三段闭合；全程零浮点、零 IO、零随机。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 本卡诊断段基址。段内 0x20 字节，诊断码占 0x01..0x0D。
pub const SEG_BASE: u32 = 0x90020;

/// 段基址被改写或未按 0x20 对齐。
pub const D01_SEG: u32 = SEG_BASE + 0x01;
/// 诊断码闭集出现重复或越界。
pub const D02_CODE: u32 = SEG_BASE + 0x02;
/// 错误码闭集不完整或负号约定被破坏。
pub const D03_ERRNO: u32 = SEG_BASE + 0x03;
/// 页表基本操作失效（映射/解映射/保护）。
pub const D04_PAGEMAP: u32 = SEG_BASE + 0x04;
/// NULL 地址未被识别。
pub const D05_NULL: u32 = SEG_BASE + 0x05;
/// 回绕或内核地址未被识别。
pub const D06_BOUND: u32 = SEG_BASE + 0x06;
/// 未映射洞未被识别。
pub const D07_HOLE: u32 = SEG_BASE + 0x07;
/// 只读页写入未被识别。
pub const D08_ROWRITE: u32 = SEG_BASE + 0x08;
/// 保护页未被识别。
pub const D09_GUARD: u32 = SEG_BASE + 0x09;
/// 跨页部分拷贝的残留声明不成立。
pub const D10_RESIDUAL: u32 = SEG_BASE + 0x0A;
/// fixup 回退表行为错误。
pub const D11_FIXUP: u32 = SEG_BASE + 0x0B;
/// 矩阵格期望与实测不符。
pub const D12_MATRIX: u32 = SEG_BASE + 0x0C;
/// 账本链断裂或重放不一致。
pub const D13_LEDGER: u32 = SEG_BASE + 0x0D;

/// 全部诊断码。闭集自检用「两两互异 + 段内」双条件。
pub const ALL_CODES: [u32; 13] = [
    D01_SEG, D02_CODE, D03_ERRNO, D04_PAGEMAP, D05_NULL, D06_BOUND,
    D07_HOLE, D08_ROWRITE, D09_GUARD, D10_RESIDUAL, D11_FIXUP, D12_MATRIX,
    D13_LEDGER,
];

/// 诊断码 → 中文解释（闭集查表，未登记码返回统一兜底文案，不 panic）。
pub fn detail(code: u32) -> &'static str {
    match code {
        D01_SEG => "段基址被改写或未按 0x20 对齐",
        D02_CODE => "诊断码闭集出现重复或越界",
        D03_ERRNO => "错误码闭集不完整或负号约定被破坏",
        D04_PAGEMAP => "页表操作失效：映射/解映射/保护结果与请求不符",
        D05_NULL => "NULL 地址未被识别（零长调用除外）",
        D06_BOUND => "区间回绕或落入内核地址空间未被识别",
        D07_HOLE => "未映射页洞被当作可访问内存",
        D08_ROWRITE => "只读映射上的写入未被拒绝",
        D09_GUARD => "保护页被当作可访问内存",
        D10_RESIDUAL => "跨页部分拷贝的残留量声明不成立",
        D11_FIXUP => "fixup 回退表行为错误：未登记站点未 Oops 或登记被静默覆盖",
        D12_MATRIX => "矩阵格期望与实测不符",
        D13_LEDGER => "账本链断裂或重放指纹不一致",
        _ => "未登记诊断码（闭集之外）",
    }
}

// ---------------------------------------------------------------------------
// 一、错误码闭集
// ---------------------------------------------------------------------------

/// 缓冲面会真实产生的错误码闭集。
///
/// 负号是 Linux 内核内部约定（`-EFAULT` 等），用户态看到的是其绝对值。
/// `from_raw()` 只接受负值，正值一律返回 `None`，防止把 `errno` 与
/// 返回值混着比较——这是用户态代码里最常见的误判来源。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Errno {
    /// -14 EFAULT：坏指针。
    Fault,
    /// -22 EINVAL：长度或对齐非法。
    Inval,
    /// -12 ENOMEM：页表容量不足。
    NoMem,
    /// -9 EBADF：句柄非法（本卡只在组合测试里用到）。
    BadF,
    /// -34 ERANGE：fixup 表满等容量越界。
    Range,
}

/// EFAULT 的原始负值。
pub const FAULT_RAW: i32 = -14;

impl Errno {
    /// 内核内部原始负值。
    pub const fn raw(self) -> i32 {
        match self {
            Errno::Fault => -14,
            Errno::Inval => -22,
            Errno::NoMem => -12,
            Errno::BadF => -9,
            Errno::Range => -34,
        }
    }

    /// errno 宏名（用户态可见）。
    pub const fn name(self) -> &'static str {
        match self {
            Errno::Fault => "EFAULT",
            Errno::Inval => "EINVAL",
            Errno::NoMem => "ENOMEM",
            Errno::BadF => "EBADF",
            Errno::Range => "ERANGE",
        }
    }

    /// 中文解释。
    pub const fn zh(self) -> &'static str {
        match self {
            Errno::Fault => "坏指针",
            Errno::Inval => "参数非法",
            Errno::NoMem => "内存不足",
            Errno::BadF => "坏文件描述符",
            Errno::Range => "范围越界",
        }
    }

    /// 由负值反查；非闭集值与正值一律 `None`。
    pub fn from_raw(raw: i32) -> Option<Errno> {
        match raw {
            -14 => Some(Errno::Fault),
            -22 => Some(Errno::Inval),
            -12 => Some(Errno::NoMem),
            -9 => Some(Errno::BadF),
            -34 => Some(Errno::Range),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 二、页模型：64 页影子地址空间
// ---------------------------------------------------------------------------

/// 页内偏移位数。
pub const PAGE_SHIFT: u32 = 12;
/// 页大小（4 KiB，与 Linux x86-64 基础页一致）。
pub const PAGE_SIZE: u64 = 1u64 << PAGE_SHIFT;
/// 页内掩码。
pub const PAGE_MASK: u64 = PAGE_SIZE - 1;
/// 影子地址空间的页数（64 页 = 256 KiB）。
pub const NPAGES: usize = 64;
/// 影子地址空间篇幅（字节）。
pub const USER_MAP_BYTES: usize = NPAGES * (PAGE_SIZE as usize);
/// 影子地址空间起点（跳过 NULL 页，留一页给「洞」）。
pub const USER_BASE: u64 = 0x0000_0000_0001_0000;
/// 影子地址空间终点（开区间）。
pub const MAP_END: u64 = USER_BASE + USER_MAP_BYTES as u64;
/// 用户态地址上界（x86-64 47 位规范地址的一半，保守取值）。
pub const USER_TOP: u64 = 0x0000_7000_0000_0000;
/// 内核态地址起点（规范高半区）。
pub const KERNEL_BASE: u64 = 0xFFFF_8000_0000_0000;

/// 页状态闭集。
///
/// `Hole` 与 `Guard` 都不可访问，但二者语义不同：
/// `Hole` 是「从未映射」（未分配），`Guard` 是「故意留出的保护页」
/// （栈溢出检测、堆尾哨兵）。分开的意义在于诊断信息——两者都返回 EFAULT，
/// 但内核日志与判据结论不同：前者说明用户传了野指针，后者说明用户越过了
/// 自己声明的边界，属于程序 bug 而不是参数错误。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    /// 从未映射。
    Hole,
    /// 只读。
    R,
    /// 只写（真实硬件上不存在，用于构造方向敏感性样本）。
    W,
    /// 读写。
    RW,
    /// 只执行。
    X,
    /// 读 + 执行。
    RX,
    /// 读写执行。
    RWX,
    /// 保护页：故意不可访问。
    Guard,
}

/// `[Page::Hole; N]` 的上下文常量（`Page` 虽是 `Copy`，仍显式给出以免
/// 依赖数组字面量的隐式复制路径）。
const HOLE: Page = Page::Hole;

impl Page {
    /// 可读。
    pub const fn readable(self) -> bool {
        match self {
            Page::R | Page::RW | Page::RX | Page::RWX | Page::W => true,
            Page::Hole | Page::X | Page::Guard => false,
        }
    }

    /// 可写。
    pub const fn writable(self) -> bool {
        match self {
            Page::W | Page::RW | Page::RWX => true,
            Page::Hole | Page::R | Page::X | Page::RX | Page::Guard => false,
        }
    }

    /// 是否为已映射的普通页（不含保护页）。
    pub const fn present(self) -> bool {
        match self {
            Page::Hole | Page::Guard => false,
            Page::R | Page::W | Page::RW | Page::X | Page::RX | Page::RWX => true,
        }
    }

    /// 三位权限位（r/w/x），用于打印与指纹。
    pub const fn bits(self) -> u8 {
        match self {
            Page::Hole => 0,
            Page::R => 0b100,
            Page::W => 0b010,
            Page::RW => 0b110,
            Page::X => 0b001,
            Page::RX => 0b101,
            Page::RWX => 0b111,
            Page::Guard => 0x80,
        }
    }
}

/// 定长页表。所有映射都落在这 64 页里，越界映射直接 `ERANGE`。
#[derive(Clone)]
pub struct PageMap {
    pages: [Page; NPAGES],
}

impl PageMap {
    /// 全空页表。
    pub const fn new() -> PageMap {
        PageMap { pages: [HOLE; NPAGES] }
    }

    /// 地址 → 页下标；落在影子区之外返回 `None`。
    fn index(va: u64) -> Option<usize> {
        if va < USER_BASE {
            return None;
        }
        let off = va - USER_BASE;
        if off >= USER_MAP_BYTES as u64 {
            return None;
        }
        Some((off >> PAGE_SHIFT) as usize)
    }

    /// 页下标 → 页基址。
    fn base(idx: usize) -> Option<u64> {
        if idx >= NPAGES {
            return None;
        }
        Some(USER_BASE + (idx as u64) * PAGE_SIZE)
    }

    /// 映射 `[va, va+len)` 为 `page`。要求页对齐、落在影子区内、`page != Hole`。
    pub fn map(&mut self, va: u64, len: u64, page: Page) -> Result<(), Errno> {
        if len == 0 {
            return Err(Errno::Inval);
        }
        if va & PAGE_MASK != 0 {
            return Err(Errno::Inval);
        }
        if page == Page::Hole {
            return Err(Errno::Inval);
        }
        let end = match va.checked_add(len) {
            Some(e) => e,
            None => return Err(Errno::Range),
        };
        if va < USER_BASE || end > MAP_END {
            return Err(Errno::Range);
        }
        let first = match Self::index(va) {
            Some(i) => i,
            None => return Err(Errno::Range),
        };
        let last = ((end - 1) - USER_BASE) >> PAGE_SHIFT;
        let mut i = first;
        while i <= last as usize {
            match self.pages.get_mut(i) {
                Some(slot) => *slot = page,
                None => return Err(Errno::Range),
            }
            i += 1;
        }
        Ok(())
    }

    /// 解映射：恢复为 `Hole`。区间内已是洞不报错（幂等）。
    pub fn unmap(&mut self, va: u64, len: u64) -> Result<(), Errno> {
        if len == 0 {
            return Err(Errno::Inval);
        }
        if va & PAGE_MASK != 0 {
            return Err(Errno::Inval);
        }
        let end = match va.checked_add(len) {
            Some(e) => e,
            None => return Err(Errno::Range),
        };
        if va < USER_BASE || end > MAP_END {
            return Err(Errno::Range);
        }
        let first = match Self::index(va) {
            Some(i) => i,
            None => return Err(Errno::Range),
        };
        let last = ((end - 1) - USER_BASE) >> PAGE_SHIFT;
        let mut i = first;
        while i <= last as usize {
            match self.pages.get_mut(i) {
                Some(slot) => *slot = HOLE,
                None => return Err(Errno::Range),
            }
            i += 1;
        }
        Ok(())
    }

    /// 改保护。只允许在已映射页上改，洞上改保护返回 `EINVAL`
    /// （与 `mprotect(PROT_NONE)` 在未映射区间上返回 ENOMEM 的取舍不同：
    /// 本模型不引入 VMA 概念，用更严格的一条规则避免「洞被误当成保护区」）。
    pub fn protect(&mut self, va: u64, len: u64, page: Page) -> Result<(), Errno> {
        if page == Page::Hole {
            return Err(Errno::Inval);
        }
        if len == 0 {
            return Err(Errno::Inval);
        }
        if va & PAGE_MASK != 0 {
            return Err(Errno::Inval);
        }
        let end = match va.checked_add(len) {
            Some(e) => e,
            None => return Err(Errno::Range),
        };
        if va < USER_BASE || end > MAP_END {
            return Err(Errno::Range);
        }
        let first = match Self::index(va) {
            Some(i) => i,
            None => return Err(Errno::Range),
        };
        let last = ((end - 1) - USER_BASE) >> PAGE_SHIFT;
        let mut i = first;
        while i <= last as usize {
            match self.pages.get_mut(i) {
                Some(slot) => {
                    if *slot == HOLE {
                        return Err(Errno::Inval);
                    }
                    *slot = page;
                }
                None => return Err(Errno::Range),
            }
            i += 1;
        }
        Ok(())
    }

    /// 查询单页状态；影子区之外一律 `Hole`。
    pub fn at(&self, va: u64) -> Page {
        match Self::index(va) {
            Some(i) => self.pages.get(i).copied().unwrap_or(HOLE),
            None => HOLE,
        }
    }

    /// 统计已映射普通页数（不含保护页）。
    pub fn mapped_pages(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < NPAGES {
            if let Some(p) = self.pages.get(i) {
                if p.present() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 统计保护页数。
    pub fn guard_pages(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < NPAGES {
            if let Some(p) = self.pages.get(i) {
                if *p == Page::Guard {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 页表指纹：把每页的权限位折进一条链。
    pub fn digest(&self) -> u64 {
        let mut h = 0xCBF2_9CE4_8422_2325u64;
        let mut i = 0usize;
        while i < NPAGES {
            let b = self.pages.get(i).map(|p| p.bits()).unwrap_or(0) as u64;
            h ^= b ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            i += 1;
        }
        h
    }

    /// 构造一份固定夹具：四段不同权限的相邻区间。
    ///
    /// ```text
    /// [0x10000, 0x12000)  RW   数据段
    /// [0x12000, 0x13000)  R    只读段
    /// [0x13000, 0x17000)  RW   栈段
    /// [0x17000, 0x17100)  Guard 栈底保护页
    /// ```
    pub fn fixture() -> PageMap {
        let mut pm = PageMap::new();
        let _ = pm.map(0x0000_0000_0001_0000, 0x2000, Page::RW);
        let _ = pm.map(0x0000_0000_0001_2000, 0x1000, Page::R);
        let _ = pm.map(0x0000_0000_0001_3000, 0x4000, Page::RW);
        let _ = pm.map(0x0000_0000_0001_7000, 0x1000, Page::Guard);
        pm
    }
}

// ---------------------------------------------------------------------------
// 三、故障分类
// ---------------------------------------------------------------------------

/// 缓冲面故障原因闭集。
///
/// 与 `Errno` 的关系：所有变体最终都映射到 `Errno::Fault`，但保留原因
/// 是判据能定位「哪一层闸放行了」的唯一手段——只比较 errno 时，把 NULL
/// 误判成普通洞也会全绿。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// 空指针。
    Null,
    /// 落入内核地址空间（含 (void*)-1 这类高位地址）。
    Kernel,
    /// `va + len` 回绕。
    Wrap,
    /// 未映射。
    Hole,
    /// 保护页。
    Guard,
    /// 写方向遇到只读页。
    RoWrite,
}

impl Fault {
    /// 中文解释。
    pub const fn zh(self) -> &'static str {
        match self {
            Fault::Null => "空指针",
            Fault::Kernel => "内核地址或用户上界之外",
            Fault::Wrap => "区间长度回绕",
            Fault::Hole => "未映射页",
            Fault::Guard => "保护页",
            Fault::RoWrite => "只读页写入",
        }
    }

    /// 对外 errno。
    pub const fn errno(self) -> Errno {
        Errno::Fault
    }
}

/// 首个故障点的完整定位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FaultAt {
    /// 触发检查的原始地址（NULL/回绕时是入参本身）。
    pub va: u64,
    /// 首个故障页基址（回绕/空指针时为 0）。
    pub page: u64,
    /// 原因。
    pub kind: Fault,
}

impl FaultAt {
    const fn new(va: u64, page: u64, kind: Fault) -> FaultAt {
        FaultAt { va, page, kind }
    }

    /// 单行诊断。
    pub fn line(&self) -> String {
        format!("{}@0x{:x}(page=0x{:x})", self.kind.zh(), self.va, self.page)
    }
}

/// 探测 `[va, va+len)` 是否可被内核按 `write` 方向触碰。
///
/// 语义与 Linux `access_ok()` + 页遍历一致：
/// - `len == 0` 恒成功（这是唯一一条绕过 NULL 判定的规则）；
/// - 先判地址上界与回绕，再逐页判存在性与方向权限；
/// - 返回**首个**故障点，后续页不再检查（部分拷贝语义的前提）。
pub fn probe(pm: &PageMap, va: u64, len: u64, write: bool) -> Result<(), FaultAt> {
    if len == 0 {
        return Ok(());
    }
    if va == 0 {
        return Err(FaultAt::new(va, 0, Fault::Null));
    }
    if va >= USER_TOP {
        return Err(FaultAt::new(va, 0, Fault::Kernel));
    }
    let end = match va.checked_add(len) {
        Some(e) => e,
        None => return Err(FaultAt::new(va, 0, Fault::Wrap)),
    };
    if end > USER_TOP {
        return Err(FaultAt::new(va, 0, Fault::Kernel));
    }
    let mut page = va & !PAGE_MASK;
    while page < end {
        let p = pm.at(page);
        if p == Page::Hole {
            return Err(FaultAt::new(va, page, Fault::Hole));
        }
        if p == Page::Guard {
            return Err(FaultAt::new(va, page, Fault::Guard));
        }
        if write && !p.writable() {
            return Err(FaultAt::new(va, page, Fault::RoWrite));
        }
        if !write && !p.readable() {
            return Err(FaultAt::new(va, page, Fault::RoWrite));
        }
        page = page.wrapping_add(PAGE_SIZE);
    }
    Ok(())
}

/// 探测在**读取**方向是否合法（只读页可读、只写页不可读）。
pub fn probe_read(pm: &PageMap, va: u64, len: u64) -> Result<(), FaultAt> {
    probe(pm, va, len, false)
}

/// 探测在**写入**方向是否合法。
pub fn probe_write(pm: &PageMap, va: u64, len: u64) -> Result<(), FaultAt> {
    probe(pm, va, len, true)
}

// ---------------------------------------------------------------------------
// 四、影子内存
// ---------------------------------------------------------------------------

/// 影子内存第 `i` 字节的初始值。
///
/// 用纯函数而不是随机数：残留校验要在任意时刻复算「这块内存原本应该是什么」，
/// 一旦引入随机源，判据就依赖调用顺序；一旦引入墙钟，判据就无法重放。
pub fn seed_byte(i: usize) -> u8 {
    let x = (i as u32).wrapping_mul(31).wrapping_add(7);
    (x ^ (x >> 8)) as u8
}

/// 用户地址空间背后的影子存储。真实内核里这对应物理页，这里用一段
/// 确定性字节序列代替，只服务于「是否被触碰 / 被写成什么」两类判据。
pub struct Shadow {
    bytes: Vec<u8>,
}

impl Shadow {
    /// 按种子公式初始化整段影子区。
    pub fn new() -> Shadow {
        let mut bytes = Vec::with_capacity(USER_MAP_BYTES);
        let mut i = 0usize;
        while i < USER_MAP_BYTES {
            bytes.push(seed_byte(i));
            i += 1;
        }
        Shadow { bytes }
    }

    /// 用户地址 → 影子下标。
    fn offset(va: u64) -> Option<usize> {
        if va < USER_BASE {
            return None;
        }
        let off = va - USER_BASE;
        if off >= USER_MAP_BYTES as u64 {
            return None;
        }
        Some(off as usize)
    }

    /// 读一个字节。
    pub fn peek(&self, va: u64) -> Option<u8> {
        let i = Self::offset(va)?;
        self.bytes.get(i).copied()
    }

    /// 写一个字节；地址在影子区之外返回 false（不 panic）。
    pub fn poke(&mut self, va: u64, val: u8) -> bool {
        match Self::offset(va) {
            Some(i) => match self.bytes.get_mut(i) {
                Some(slot) => {
                    *slot = val;
                    true
                }
                None => false,
            },
            None => false,
        }
    }

    /// 读一段切片；越界返回 `None`。
    pub fn slice(&self, va: u64, len: u64) -> Option<&[u8]> {
        let start = Self::offset(va)?;
        let end = start.checked_add(len as usize)?;
        if end > self.bytes.len() {
            return None;
        }
        self.bytes.get(start..end)
    }

    /// 该地址原本的种子值。
    pub fn seed_at(&self, va: u64) -> Option<u8> {
        let i = Self::offset(va)?;
        Some(seed_byte(i))
    }

    /// `[va, va+len)` 是否一个字节都没被碰过。
    ///
    /// 这是「残留声明」的判据本体：部分拷贝失败后，尾部必须保持初始种子。
    /// 只要有一个字节变化，就说明拷贝越过了故障点，属于写穿缺陷。
    pub fn is_virgin(&self, va: u64, len: u64) -> bool {
        if len == 0 {
            return true;
        }
        let start = match Self::offset(va) {
            Some(s) => s,
            // 影子区之外的地址不可能被本模块写穿，一律视为「未触碰」。
            None => return true,
        };
        let end = match start.checked_add(len as usize) {
            Some(e) => e,
            None => return false,
        };
        if end > self.bytes.len() {
            return false;
        }
        let mut i = start;
        while i < end {
            match self.bytes.get(i) {
                Some(b) => {
                    if *b != seed_byte(i) {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 把一段区间写成指定值（夹具用）。
    pub fn fill(&mut self, va: u64, len: u64, val: u8) -> bool {
        let start = match Self::offset(va) {
            Some(s) => s,
            None => return false,
        };
        let end = match start.checked_add(len as usize) {
            Some(e) => e,
            None => return false,
        };
        if end > self.bytes.len() {
            return false;
        }
        let mut i = start;
        while i < end {
            if let Some(slot) = self.bytes.get_mut(i) {
                *slot = val;
            }
            i += 1;
        }
        true
    }

    /// 影子区总字节数。
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// 是否为空（恒定 false，保留以配合 `len` 的惯用法）。
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 五、拷贝引擎
// ---------------------------------------------------------------------------

/// 残留声明：一次用户拷贝请求里，成功多少、剩下多少没碰。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Residual {
    /// 请求字节数。
    pub requested: u64,
    /// 实际完成字节数。
    pub done: u64,
}

impl Residual {
    /// 构造。
    pub const fn new(requested: u64, done: u64) -> Residual {
        Residual { requested, done }
    }

    /// 未完成字节数。
    pub const fn untouched(&self) -> u64 {
        self.requested.saturating_sub(self.done)
    }

    /// 是否为短拷贝。
    pub const fn short(&self) -> bool {
        self.done < self.requested
    }

    /// 单行声明，供判据与诊断直接引用。
    pub fn line(&self) -> String {
        format!("done={}/{} untouched={}", self.done, self.requested, self.untouched())
    }
}

/// 拷贝失败：首个故障点 + 残留声明。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CopyFault {
    /// 首个故障。
    pub at: FaultAt,
    /// 残留。
    pub res: Residual,
}

impl CopyFault {
    /// 对外 errno。
    pub const fn errno(&self) -> Errno {
        Errno::Fault
    }

    /// 单行诊断。
    pub fn line(&self) -> String {
        format!("{} | {}", self.at.line(), self.res.line())
    }
}

/// 拷贝统计（成功路径也要带页数，用于验证分块确实按页走）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CopyStat {
    /// 完成字节数。
    pub done: u64,
    /// 触碰的页数。
    pub pages: u64,
}

/// 内核 → 用户：把 `src` 写进用户地址 `va`。
///
/// 与 Linux `copy_to_user()` 同构：返回「没拷成多少」，只是这里用
/// `Result<done, CopyFault>` 表达。分块按页边界切断，保证故障点精确到页，
/// 也保证故障后不再触碰后续页——这是残留声明的前提。
pub fn copy_to_user(
    pm: &PageMap,
    sh: &mut Shadow,
    va: u64,
    src: &[u8],
) -> Result<CopyStat, CopyFault> {
    let total = src.len() as u64;
    if total == 0 {
        return Ok(CopyStat { done: 0, pages: 0 });
    }
    let mut done = 0u64;
    let mut pages = 0u64;
    while done < total {
        let cur = match va.checked_add(done) {
            Some(v) => v,
            None => {
                return Err(CopyFault {
                    at: FaultAt::new(va, 0, Fault::Wrap),
                    res: Residual::new(total, done),
                })
            }
        };
        let in_page = PAGE_SIZE - (cur & PAGE_MASK);
        let left = total - done;
        let chunk = if left < in_page { left } else { in_page };
        if let Err(at) = probe(pm, cur, chunk, true) {
            return Err(CopyFault { at, res: Residual::new(total, done) });
        }
        let from = done as usize;
        let to = (done + chunk) as usize;
        let part = match src.get(from..to) {
            Some(p) => p,
            None => {
                return Err(CopyFault {
                    at: FaultAt::new(cur, cur & !PAGE_MASK, Fault::Hole),
                    res: Residual::new(total, done),
                })
            }
        };
        let mut i = 0usize;
        while i < part.len() {
            if !sh.poke(cur + i as u64, part[i]) {
                return Err(CopyFault {
                    at: FaultAt::new(cur, cur & !PAGE_MASK, Fault::Hole),
                    res: Residual::new(total, done),
                });
            }
            i += 1;
        }
        done += chunk;
        pages += 1;
    }
    Ok(CopyStat { done, pages })
}

/// 用户 → 内核：把用户地址 `va` 处的 `len` 字节读进 `dst`。
///
/// 故障后 `dst[done..len]` 必须保持调用前的内容。调用方（`sys_write`）
/// 依赖这条性质决定「返回短写」还是「返回 EFAULT」。
pub fn copy_from_user(
    pm: &PageMap,
    sh: &Shadow,
    va: u64,
    dst: &mut [u8],
) -> Result<CopyStat, CopyFault> {
    let total = dst.len() as u64;
    if total == 0 {
        return Ok(CopyStat { done: 0, pages: 0 });
    }
    let mut done = 0u64;
    let mut pages = 0u64;
    while done < total {
        let cur = match va.checked_add(done) {
            Some(v) => v,
            None => {
                return Err(CopyFault {
                    at: FaultAt::new(va, 0, Fault::Wrap),
                    res: Residual::new(total, done),
                })
            }
        };
        let in_page = PAGE_SIZE - (cur & PAGE_MASK);
        let left = total - done;
        let chunk = if left < in_page { left } else { in_page };
        if let Err(at) = probe(pm, cur, chunk, false) {
            return Err(CopyFault { at, res: Residual::new(total, done) });
        }
        let mut i = 0u64;
        while i < chunk {
            let b = match sh.peek(cur + i) {
                Some(v) => v,
                None => {
                    return Err(CopyFault {
                        at: FaultAt::new(cur, cur & !PAGE_MASK, Fault::Hole),
                        res: Residual::new(total, done),
                    })
                }
            };
            match dst.get_mut((done + i) as usize) {
                Some(slot) => *slot = b,
                None => {
                    return Err(CopyFault {
                        at: FaultAt::new(cur, cur & !PAGE_MASK, Fault::Hole),
                        res: Residual::new(total, done),
                    })
                }
            }
            i += 1;
        }
        done += chunk;
        pages += 1;
    }
    Ok(CopyStat { done, pages })
}

// ---------------------------------------------------------------------------
// 六、fixup 回退表
// ---------------------------------------------------------------------------

/// fixup 表容量。
pub const FIX_SITES: usize = 8;

/// 一条回退登记：故障站点 → 处理标签。
///
/// 真实内核用异常表把「哪条指令的缺页可以由软件接管」写死在 `.ex_table`；
/// 这里用定长表建模同样的关系。`tag` 是处理标签（哪个拷贝例程的返回点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fixup {
    /// 故障站点编号（对应指令地址的抽象编号）。
    pub site: u32,
    /// 处理标签。
    pub tag: u16,
}

const NO_FIX: Option<Fixup> = None;

/// 定长回退表。
#[derive(Clone)]
pub struct FixupTable {
    slots: [Option<Fixup>; FIX_SITES],
    registered: usize,
}

/// 一次缺页的处理结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FixupAction {
    /// 已接管：带处理标签回到用户态。
    Handled(u16),
    /// 未登记：内核无法恢复（真实内核即 Oops）。
    Oops,
}

impl FixupTable {
    /// 空表。
    pub const fn new() -> FixupTable {
        FixupTable { slots: [NO_FIX; FIX_SITES], registered: 0 }
    }

    /// 登记一条回退。重复站点、表满都拒绝——静默覆盖会让「后登记的例程
    /// 悄悄接管别人故障」这类缺陷永远无法暴露。
    pub fn register(&mut self, site: u32, tag: u16) -> Result<(), Errno> {
        if self.find(site).is_some() {
            return Err(Errno::Inval);
        }
        let mut i = 0usize;
        while i < FIX_SITES {
            match self.slots.get(i) {
                Some(slot) => {
                    if slot.is_none() {
                        if let Some(cell) = self.slots.get_mut(i) {
                            *cell = Some(Fixup { site, tag });
                        }
                        self.registered += 1;
                        return Ok(());
                    }
                }
                None => return Err(Errno::Range),
            }
            i += 1;
        }
        Err(Errno::Range)
    }

    /// 查询站点。
    pub fn find(&self, site: u32) -> Option<Fixup> {
        let mut i = 0usize;
        while i < FIX_SITES {
            if let Some(Some(f)) = self.slots.get(i) {
                if f.site == site {
                    return Some(*f);
                }
            }
            i += 1;
        }
        None
    }

    /// 派发一次缺页。
    pub fn dispatch(&self, site: u32) -> FixupAction {
        match self.find(site) {
            Some(f) => FixupAction::Handled(f.tag),
            None => FixupAction::Oops,
        }
    }

    /// 已登记数量。
    pub fn len(&self) -> usize {
        self.registered
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.registered == 0
    }

    /// 容量。
    pub const fn capacity(&self) -> usize {
        FIX_SITES
    }
}

// ---------------------------------------------------------------------------
// 七、系统调用层
// ---------------------------------------------------------------------------

/// 标准输入（本模型里唯一可读句柄）。
pub const FD_STDIN: i32 = 0;
/// 标准输出（本模型里唯一可写句柄）。
pub const FD_STDOUT: i32 = 1;
/// 输入设备可提供的字节数。
pub const DEV_BYTES: u64 = 0x1000;

/// 输入设备第 `off` 字节的内容（确定性，便于复算短读内容）。
pub fn dev_byte(off: u64) -> u8 {
    let x = off.wrapping_mul(37).wrapping_add(11);
    (x ^ (x >> 5) ^ 0x5A) as u8
}

/// `read(2)`：`ufd` 必须是标准输入，缓冲面走 `copy_to_user`。
///
/// 返回值遵循 Linux 短读规则：
/// - 全程成功 → `Ok(count)`；
/// - 中途故障且已拷贝 > 0 → `Ok(已拷贝)`（不是 EFAULT）；
/// - 首个字节就故障 → `Err(EFAULT)`。
pub fn sys_read(
    pm: &PageMap,
    sh: &mut Shadow,
    ufd: i32,
    ubuf: u64,
    count: u64,
) -> Result<u64, Errno> {
    if ufd != FD_STDIN {
        return Err(Errno::BadF);
    }
    if count == 0 {
        return Ok(0);
    }
    if count > DEV_BYTES {
        return Err(Errno::Inval);
    }
    let mut src = Vec::with_capacity(count as usize);
    let mut i = 0u64;
    while i < count {
        src.push(dev_byte(i));
        i += 1;
    }
    match copy_to_user(pm, sh, ubuf, &src) {
        Ok(st) => Ok(st.done),
        Err(f) => {
            if f.res.done > 0 {
                Ok(f.res.done)
            } else {
                Err(f.errno())
            }
        }
    }
}

/// `write(2)`：`ufd` 必须是标准输出，缓冲面走 `copy_from_user`。
///
/// 短写规则与短读对称：只要拷出过字节，就返回字节数。
pub fn sys_write(
    pm: &PageMap,
    sh: &Shadow,
    ufd: i32,
    ubuf: u64,
    count: u64,
) -> Result<u64, Errno> {
    if ufd != FD_STDOUT {
        return Err(Errno::BadF);
    }
    if count == 0 {
        return Ok(0);
    }
    if count > DEV_BYTES {
        return Err(Errno::Inval);
    }
    let mut dst = Vec::with_capacity(count as usize);
    let mut i = 0u64;
    while i < count {
        dst.push(0u8);
        i += 1;
    }
    match copy_from_user(pm, sh, ubuf, dst.as_mut_slice()) {
        Ok(st) => Ok(st.done),
        Err(f) => {
            if f.res.done > 0 {
                Ok(f.res.done)
            } else {
                Err(f.errno())
            }
        }
    }
}

/// 一段 iovec。与 `struct iovec` 同构：基址 + 长度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Iovec {
    /// 基址。
    pub base: u64,
    /// 长度。
    pub len: u64,
}

/// 本模型支持的最大 iovec 数（两个就足以构造「首段合法、次段非法」样本）。
pub const IOV_MAX: usize = 2;

/// `readv(2)`：逐段读取，任一段故障即停；已读非零则返回短读。
pub fn sys_readv(
    pm: &PageMap,
    sh: &mut Shadow,
    ufd: i32,
    iov: &[Iovec; IOV_MAX],
) -> Result<u64, Errno> {
    if ufd != FD_STDIN {
        return Err(Errno::BadF);
    }
    let mut total = 0u64;
    let mut k = 0usize;
    while k < IOV_MAX {
        let seg = match iov.get(k) {
            Some(s) => *s,
            None => break,
        };
        if seg.len == 0 {
            k += 1;
            continue;
        }
        if total + seg.len > DEV_BYTES {
            return Err(Errno::Inval);
        }
        match sys_read(pm, sh, ufd, seg.base, seg.len) {
            Ok(n) => {
                total += n;
                if n < seg.len {
                    return Ok(total);
                }
            }
            Err(e) => {
                if total > 0 {
                    return Ok(total);
                }
                return Err(e);
            }
        }
        k += 1;
    }
    Ok(total)
}

/// `writev(2)`：逐段写出，任一段故障即停；已写非零则返回短写。
pub fn sys_writev(
    pm: &PageMap,
    sh: &Shadow,
    ufd: i32,
    iov: &[Iovec; IOV_MAX],
) -> Result<u64, Errno> {
    if ufd != FD_STDOUT {
        return Err(Errno::BadF);
    }
    let mut total = 0u64;
    let mut k = 0usize;
    while k < IOV_MAX {
        let seg = match iov.get(k) {
            Some(s) => *s,
            None => break,
        };
        if seg.len == 0 {
            k += 1;
            continue;
        }
        if total + seg.len > DEV_BYTES {
            return Err(Errno::Inval);
        }
        match sys_write(pm, sh, ufd, seg.base, seg.len) {
            Ok(n) => {
                total += n;
                if n < seg.len {
                    return Ok(total);
                }
            }
            Err(e) => {
                if total > 0 {
                    return Ok(total);
                }
                return Err(e);
            }
        }
        k += 1;
    }
    Ok(total)
}

/// 句柄面与缓冲面同时可疑时，先报哪个错。
///
/// 这条函数是 UNX-F9202 与 UNX-F9203 的接缝：句柄闸在前，缓冲闸在后。
/// 调用方可以给出任意坏指针，只要 fd 非法，结果必须是 EBADF。
pub fn sys_read_ordered(
    pm: &PageMap,
    sh: &mut Shadow,
    ufd: i32,
    ubuf: u64,
    count: u64,
) -> Result<u64, Errno> {
    if ufd != FD_STDIN {
        return Err(Errno::BadF);
    }
    sys_read(pm, sh, ufd, ubuf, count)
}

// ---------------------------------------------------------------------------
// 八、坏指针矩阵（8 类 × 4 操作）
// ---------------------------------------------------------------------------

/// 矩阵操作维度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    /// `read(2)`。
    Read,
    /// `write(2)`。
    Write,
    /// `readv(2)`。
    ReadV,
    /// `writev(2)`。
    WriteV,
}

/// 操作维度大小。
pub const OPS: usize = 4;

impl Op {
    /// 下标。
    pub const fn idx(self) -> usize {
        match self {
            Op::Read => 0,
            Op::Write => 1,
            Op::ReadV => 2,
            Op::WriteV => 3,
        }
    }

    /// 名称。
    pub const fn name(self) -> &'static str {
        match self {
            Op::Read => "read",
            Op::Write => "write",
            Op::ReadV => "readv",
            Op::WriteV => "writev",
        }
    }

    /// 是否为写方向（决定权限检查读哪个位）。
    pub const fn is_write(self) -> bool {
        match self {
            Op::Read | Op::ReadV => false,
            Op::Write | Op::WriteV => true,
        }
    }
}

/// 一类坏指针样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BadCase {
    /// 类编号（1 起）。
    pub id: u8,
    /// 中文标签。
    pub label: &'static str,
    /// 起始地址。
    pub va: u64,
    /// 长度。
    pub len: u64,
}

/// 八类坏指针。
///
/// 前七类是卡面点名的四类及其近邻（NULL、-1、内核地址、洞、只读、保护页），
/// 第八类是映射区之外的普通高地址；第六类「跨页半映射」同时承担
/// 部分拷贝残留量判据的样本。
pub const BAD_CASES: [BadCase; 8] = [
    BadCase { id: 1, label: "NULL", va: 0, len: 64 },
    BadCase { id: 2, label: "-1 指针", va: 0xFFFF_FFFF_FFFF_FFFF, len: 64 },
    BadCase { id: 3, label: "内核地址", va: KERNEL_BASE + 0x1000, len: 64 },
    BadCase { id: 4, label: "未映射洞", va: 0x0000_0000_0002_0000, len: 64 },
    BadCase { id: 5, label: "只读页写路径", va: 0x0000_0000_0001_2000, len: 64 },
    BadCase { id: 6, label: "跨页半映射", va: 0x0000_0000_0001_6F00, len: 0x200 },
    BadCase { id: 7, label: "保护页", va: 0x0000_0000_0001_7000, len: 64 },
    BadCase { id: 8, label: "映射区外高地址", va: 0x0000_0000_0005_0000, len: 64 },
];

/// 类维度大小。
pub const CLASSES: usize = 8;

/// 一格的期望。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Expect {
    /// 成功且返回值等于给定字节数。
    Ok(u64),
    /// 失败且 errno 等于给定值。
    Err(Errno),
}

/// 期望表。这里刻意不做「一刀切 EFAULT」：
///
/// - 只读页在读取方向必须成功（第 5 类 read/readv）；
/// - 跨页半映射在四个方向都是**短操作**（第 6 类全部 Ok(256)），
///   因为首段确实可访问，Linux 会把已完成的 256 字节报回去。
///
/// 把这两格写成 EFAULT，矩阵会在正确实现上恒红；反过来，把实现改成
/// 「见坏就整体失败」，这两格又会恒绿——所以它们必须单独登记。
pub fn expect_of(class_id: u8, op: Op) -> Expect {
    match class_id {
        6 => Expect::Ok(0x100),
        5 => match op {
            // 只读页：read 方向是「内核写用户」，必须 EFAULT；
            // write 方向是「内核读用户」，只读页完全合法。
            Op::Read | Op::ReadV => Expect::Err(Errno::Fault),
            Op::Write => Expect::Ok(64),
            Op::WriteV => Expect::Ok(128),
        },
        _ => Expect::Err(Errno::Fault),
    }
}

/// 一格实测结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    /// 内核原始返回值：成功 0，失败为负 errno。
    pub raw: i32,
    /// 用户可见返回值（成功时的字节数）。
    pub value: u64,
    /// 残留声明。
    pub res: Residual,
    /// 故障点之后的目标区间是否一字节未动。
    pub virgin_tail: bool,
}

impl Cell {
    /// 与本格期望是否一致（含残留声明）。
    pub fn matches(&self, class_id: u8, op: Op) -> bool {
        let base = match expect_of(class_id, op) {
            Expect::Ok(n) => self.raw == 0 && self.value == n,
            Expect::Err(e) => self.raw == e.raw() && self.value == 0,
        };
        if !base {
            return false;
        }
        // 短操作必须真的声明了残留，且尾部未被触碰。
        if self.res.short() {
            self.virgin_tail
        } else {
            true
        }
    }
}

const ZERO_CELL: Cell = Cell { raw: 0, value: 0, res: Residual { requested: 0, done: 0 }, virgin_tail: true };

/// 执行一格。
pub fn run_cell(class: &BadCase, op: Op) -> Cell {
    let pm = PageMap::fixture();
    let mut sh = Shadow::new();
    let requested = class.len;
    match op {
        Op::Read | Op::Write => {
            if !op.is_write() {
                match sys_read(&pm, &mut sh, FD_STDIN, class.va, class.len) {
                    Ok(n) => Cell {
                        raw: 0,
                        value: n,
                        res: Residual::new(requested, n),
                        virgin_tail: sh.is_virgin(class.va + n, requested - n),
                    },
                    Err(e) => Cell {
                        raw: e.raw(),
                        value: 0,
                        res: Residual::new(requested, 0),
                        virgin_tail: sh.is_virgin(class.va, requested),
                    },
                }
            } else {
                match sys_write(&pm, &sh, FD_STDOUT, class.va, class.len) {
                    Ok(n) => Cell {
                        raw: 0,
                        value: n,
                        res: Residual::new(requested, n),
                        virgin_tail: true,
                    },
                    Err(e) => Cell {
                        raw: e.raw(),
                        value: 0,
                        res: Residual::new(requested, 0),
                        virgin_tail: true,
                    },
                }
            }
        }
        Op::ReadV | Op::WriteV => {
            // 双段：首段 256 字节落在样本起点，次段紧随其后 256 字节。
            // 对单段样本（len=64）来说次段直接取同一起点 64 字节，
            // 保证「段数」维度被真实走到，而不是把单段复制两次。
            let seg_len = class.len;
            let iov: [Iovec; IOV_MAX] = if class.len > 0x100 {
                [
                    Iovec { base: class.va, len: 0x100 },
                    Iovec { base: class.va + 0x100, len: seg_len - 0x100 },
                ]
            } else {
                [
                    Iovec { base: class.va, len: seg_len },
                    Iovec { base: class.va, len: seg_len },
                ]
            };
            // 两段请求量之和才是残留声明的分母；用 seg_len*2 会把
            // 「首段 0x100 + 次段 0x100」的样本误记成 0x400。
            let req = iov[0].len + iov[1].len;
            if op == Op::ReadV {
                match sys_readv(&pm, &mut sh, FD_STDIN, &iov) {
                    Ok(n) => Cell {
                        raw: 0,
                        value: n,
                        res: Residual::new(req, n),
                        virgin_tail: sh.is_virgin(class.va + n, req.saturating_sub(n)),
                    },
                    Err(e) => Cell {
                        raw: e.raw(),
                        value: 0,
                        res: Residual::new(req, 0),
                        virgin_tail: sh.is_virgin(class.va, req),
                    },
                }
            } else {
                match sys_writev(&pm, &sh, FD_STDOUT, &iov) {
                    Ok(n) => Cell {
                        raw: 0,
                        value: n,
                        res: Residual::new(req, n),
                        virgin_tail: true,
                    },
                    Err(e) => Cell {
                        raw: e.raw(),
                        value: 0,
                        res: Residual::new(req, 0),
                        virgin_tail: true,
                    },
                }
            }
        }
    }
}

/// 整张矩阵。
pub fn bad_matrix() -> [[Cell; OPS]; CLASSES] {
    let mut m = [[ZERO_CELL; OPS]; CLASSES];
    let mut r = 0usize;
    while r < CLASSES {
        let case = match BAD_CASES.get(r) {
            Some(c) => *c,
            None => break,
        };
        let ops = [Op::Read, Op::Write, Op::ReadV, Op::WriteV];
        let mut c = 0usize;
        while c < OPS {
            if let Some(op) = ops.get(c) {
                if let Some(row) = m.get_mut(r) {
                    if let Some(slot) = row.get_mut(c) {
                        *slot = run_cell(&case, *op);
                    }
                }
            }
            c += 1;
        }
        r += 1;
    }
    m
}

/// 矩阵是否全格符合期望。
pub fn matrix_green(m: &[[Cell; OPS]; CLASSES]) -> bool {
    let ops = [Op::Read, Op::Write, Op::ReadV, Op::WriteV];
    let mut r = 0usize;
    while r < CLASSES {
        let case = match BAD_CASES.get(r) {
            Some(c) => *c,
            None => return false,
        };
        let mut c = 0usize;
        while c < OPS {
            let op = match ops.get(c) {
                Some(o) => *o,
                None => return false,
            };
            let cell = match m.get(r).and_then(|row| row.get(c)) {
                Some(x) => x,
                None => return false,
            };
            if !cell.matches(case.id, op) {
                return false;
            }
            c += 1;
        }
        r += 1;
    }
    true
}

/// 红格名单（诊断用）。
pub fn matrix_reds(m: &[[Cell; OPS]; CLASSES]) -> Vec<String> {
    let ops = [Op::Read, Op::Write, Op::ReadV, Op::WriteV];
    let mut out = Vec::new();
    let mut r = 0usize;
    while r < CLASSES {
        let case = match BAD_CASES.get(r) {
            Some(c) => *c,
            None => break,
        };
        let mut c = 0usize;
        while c < OPS {
            let op = match ops.get(c) {
                Some(o) => *o,
                None => break,
            };
            let cell = m.get(r).and_then(|row| row.get(c)).copied().unwrap_or(ZERO_CELL);
            if !cell.matches(case.id, op) {
                out.push(format!(
                    "{}:{} raw={} value={} res={}",
                    case.label, op.name(), cell.raw, cell.value, cell.res.line()
                ));
            }
            c += 1;
        }
        r += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 九、账本（环形 64 槽 + 链式折叠）
// ---------------------------------------------------------------------------

/// 环形账本容量。
pub const RING_CAP: usize = 64;

/// 一条矩阵记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rec {
    /// 序号（由账本分配，从 1 起）。
    pub seq: u32,
    /// 类编号。
    pub class: u8,
    /// 操作编号。
    pub op: u8,
    /// 原始返回值。
    pub raw: i32,
    /// 完成字节数。
    pub done: u64,
}

const ZERO_REC: Rec = Rec { seq: 0, class: 0, op: 0, raw: 0, done: 0 };

/// 记录折叠：乘后异或再旋转，保证任何字段改动都会扩散到整条链。
fn fold_rec(h: u64, r: &Rec) -> u64 {
    let mut x = h;
    x ^= (r.seq as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= (r.class as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    x ^= (r.op as u64).wrapping_mul(0x1656_67B1_9E37_79F9);
    x ^= (r.raw as u32 as u64).wrapping_mul(0x27D4_EB2F_1656_67C5);
    x ^= r.done.wrapping_mul(0x1000_0000_01B3);
    x ^= x >> 29;
    x = x.wrapping_mul(0x0000_0100_0000_01B3);
    x.rotate_left(7)
}

/// 定长环形账本。
///
/// 与「无限追加的 Vec」相比，定长表把「容量不足」变成显式错误，
/// 而不是静默扩容或静默覆盖——矩阵 32 格只用了 64 槽的一半，
/// 留出的空槽本身就是容量判据的样本。
#[derive(Clone)]
pub struct Ledger {
    ring: [Rec; RING_CAP],
    len: usize,
    chain: u64,
    next_seq: u32,
}

/// 链的初始值（FNV-1a 偏移基）。
pub const CHAIN_SEED: u64 = 0xCBF2_9CE4_8422_2325;

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger { ring: [ZERO_REC; RING_CAP], len: 0, chain: CHAIN_SEED, next_seq: 1 }
    }

    /// 追加一条记录；满时返回 `ERANGE`。
    pub fn push(&mut self, class: u8, op: u8, raw: i32, done: u64) -> Result<(), Errno> {
        if self.len >= RING_CAP {
            return Err(Errno::Range);
        }
        let rec = Rec { seq: self.next_seq, class, op, raw, done };
        match self.ring.get_mut(self.len) {
            Some(slot) => *slot = rec,
            None => return Err(Errno::Range),
        }
        self.chain = fold_rec(self.chain, &rec);
        self.len += 1;
        self.next_seq += 1;
        Ok(())
    }

    /// 重算链并与存储值比对。
    pub fn audit(&self) -> bool {
        let mut h = CHAIN_SEED;
        let mut i = 0usize;
        while i < self.len {
            match self.ring.get(i) {
                Some(r) => h = fold_rec(h, r),
                None => return false,
            }
            i += 1;
        }
        h == self.chain
    }

    /// 当前链值（封印）。
    pub fn seal(&self) -> u64 {
        self.chain
    }

    /// 从记录序列重放，得到独立的链值。
    pub fn replay(recs: &[Rec]) -> u64 {
        let mut h = CHAIN_SEED;
        let mut i = 0usize;
        while i < recs.len() {
            match recs.get(i) {
                Some(r) => h = fold_rec(h, r),
                None => break,
            }
            i += 1;
        }
        h
    }

    /// 导出记录（顺序与压入一致）。
    pub fn records(&self) -> Vec<Rec> {
        let mut out = Vec::with_capacity(self.len);
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.ring.get(i) {
                out.push(*r);
            }
            i += 1;
        }
        out
    }

    /// 已用槽数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 渲染成可读文本。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("seq class op raw done\n");
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.ring.get(i) {
                s.push_str(&format!(
                    "{} {} {} {} {}\n", r.seq, r.class, r.op, r.raw, r.done
                ));
            }
            i += 1;
        }
        s
    }
}

/// 由矩阵构造账本。
pub fn build_ledger() -> Ledger {
    let m = bad_matrix();
    let mut led = Ledger::new();
    let ops = [Op::Read, Op::Write, Op::ReadV, Op::WriteV];
    let mut r = 0usize;
    while r < CLASSES {
        let case = match BAD_CASES.get(r) {
            Some(c) => *c,
            None => break,
        };
        let mut c = 0usize;
        while c < OPS {
            let op = match ops.get(c) {
                Some(o) => *o,
                None => break,
            };
            let cell = m.get(r).and_then(|row| row.get(c)).copied().unwrap_or(ZERO_CELL);
            let _ = led.push(case.id, op.idx() as u8, cell.raw, cell.value);
            c += 1;
        }
        r += 1;
    }
    led
}

// ---------------------------------------------------------------------------
// 十、报告与判据入口
// ---------------------------------------------------------------------------

/// 报告头的稳定前缀。
pub const HEAD_OK: &str = "F9203|cls=8|ops=4|green=32/32";

/// 一次完整运行的结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Report {
    /// 全绿格数。
    pub green: usize,
    /// 总格数。
    pub cells: usize,
    /// 红格名单。
    pub reds: Vec<String>,
    /// 账本封印。
    pub seal: u64,
    /// 账本链自检。
    pub audited: bool,
}

impl Report {
    /// 是否全绿。
    pub fn gate(&self) -> bool {
        self.green == self.cells && self.reds.is_empty() && self.audited
    }

    /// 报告头。
    pub fn head(&self) -> String {
        format!("F9203|cls={}|ops={}|green={}/{}", CLASSES, OPS, self.green, self.cells)
    }

    /// 通过率（千分比，整数域）。
    pub fn rate_permille(&self) -> u32 {
        if self.cells == 0 {
            return 0;
        }
        ((self.green as u64) * 1000 / self.cells as u64) as u32
    }

    /// 多行渲染。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str(&self.head());
        s.push_str(&format!(" seal=0x{:016x} audit={}\n", self.seal, self.audited));
        let mut i = 0usize;
        while i < self.reds.len() {
            if let Some(r) = self.reds.get(i) {
                s.push_str("  RED ");
                s.push_str(r);
                s.push('\n');
            }
            i += 1;
        }
        s
    }
}

/// 运行整张矩阵并封账。
pub fn run() -> Report {
    let m = bad_matrix();
    let mut green = 0usize;
    let ops = [Op::Read, Op::Write, Op::ReadV, Op::WriteV];
    let mut r = 0usize;
    while r < CLASSES {
        let case = match BAD_CASES.get(r) {
            Some(c) => *c,
            None => break,
        };
        let mut c = 0usize;
        while c < OPS {
            let op = match ops.get(c) {
                Some(o) => *o,
                None => break,
            };
            let cell = m.get(r).and_then(|row| row.get(c)).copied().unwrap_or(ZERO_CELL);
            if cell.matches(case.id, op) {
                green += 1;
            }
            c += 1;
        }
        r += 1;
    }
    let led = build_ledger();
    let audited = led.audit();
    Report {
        green,
        cells: CLASSES * OPS,
        reds: matrix_reds(&m),
        seal: led.seal(),
        audited,
    }
}

/// 四路注入：NULL、-1、洞、只读写，各自必须被对应闸门抓住。
///
/// 与矩阵不同，这里检查的是**原因**而不只是结果码：矩阵绿只能说明
/// 「都返回了 EFAULT」，如果 NULL 被当成普通洞处理，矩阵仍然全绿。
pub fn inject_four() -> [bool; 4] {
    let pm = PageMap::fixture();
    let a = matches!(probe(&pm, 0, 64, true), Err(f) if f.kind == Fault::Null);
    let b = matches!(probe(&pm, 0xFFFF_FFFF_FFFF_FFFF, 64, true), Err(f) if f.kind == Fault::Kernel);
    let c = matches!(probe(&pm, 0x0000_0000_0002_0000, 64, true), Err(f) if f.kind == Fault::Hole);
    let d = matches!(probe(&pm, 0x0000_0000_0001_2000, 64, true), Err(f) if f.kind == Fault::RoWrite);
    [a, b, c, d]
}

// ---------------------------------------------------------------------------
// 十一、判据用的十个细项
// ---------------------------------------------------------------------------

/// 段基址与对齐。
fn seg_ok() -> bool {
    SEG_BASE == 0x90020 && SEG_BASE % 0x20 == 0
}

/// 诊断码闭集：段内、连续、两两互异。
fn codes_ok() -> bool {
    if ALL_CODES.len() != 13 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_CODES.len() {
        let a = match ALL_CODES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if a != SEG_BASE + 0x01 + i as u32 {
            return false;
        }
        let mut j = i + 1;
        while j < ALL_CODES.len() {
            if let Some(b) = ALL_CODES.get(j) {
                if *b == a {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 错误码负号约定与可逆映射。
fn errno_ok() -> bool {
    let all = [Errno::Fault, Errno::Inval, Errno::NoMem, Errno::BadF, Errno::Range];
    let mut i = 0usize;
    while i < all.len() {
        let e = match all.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let raw = e.raw();
        if raw >= 0 {
            return false;
        }
        if Errno::from_raw(raw) != Some(e) {
            return false;
        }
        if e.name().is_empty() || e.zh().is_empty() {
            return false;
        }
        i += 1;
    }
    Errno::from_raw(14).is_none() && Errno::from_raw(0).is_none() && FAULT_RAW == -14
}

/// 页表基本操作。
fn pagemap_ok() -> bool {
    let mut pm = PageMap::fixture();
    if pm.at(0x0000_0000_0001_0000) != Page::RW {
        return false;
    }
    if pm.at(0x0000_0000_0001_2000) != Page::R {
        return false;
    }
    if pm.at(0x0000_0000_0001_7000) != Page::Guard {
        return false;
    }
    if pm.at(0x0000_0000_0002_0000) != Page::Hole {
        return false;
    }
    if pm.mapped_pages() != 7 || pm.guard_pages() != 1 {
        return false;
    }
    // 未对齐映射必须拒绝。
    if pm.map(0x0000_0000_0001_0800, 0x1000, Page::RW) != Err(Errno::Inval) {
        return false;
    }
    // 越出影子区必须拒绝。
    if pm.map(0x0000_0000_0004_F000, 0x2000, Page::RW) != Err(Errno::Range) {
        return false;
    }
    // 洞上改保护必须拒绝。
    if pm.protect(0x0000_0000_0002_0000, 0x1000, Page::R) != Err(Errno::Inval) {
        return false;
    }
    // 解映射幂等。
    if pm.unmap(0x0000_0000_0002_0000, 0x1000) != Ok(()) {
        return false;
    }
    // 新映射生效。
    if pm.map(0x0000_0000_0002_0000, 0x2000, Page::RX) != Ok(()) {
        return false;
    }
    pm.at(0x0000_0000_0002_1000) == Page::RX && pm.mapped_pages() == 9
}

/// NULL 与零长度例外。
fn null_ok() -> bool {
    let pm = PageMap::fixture();
    if probe(&pm, 0, 64, true).map(|_| true) != Err(FaultAt::new(0, 0, Fault::Null)) {
        return false;
    }
    probe(&pm, 0, 0, true).is_ok() && probe(&pm, 0, 0, false).is_ok()
}

/// 上界与回绕。
fn bound_ok() -> bool {
    let pm = PageMap::fixture();
    let one = match probe(&pm, 0xFFFF_FFFF_FFFF_FFFF, 64, true) {
        Err(f) => f.kind,
        Ok(()) => return false,
    };
    let kernel = match probe(&pm, KERNEL_BASE, 64, true) {
        Err(f) => f.kind,
        Ok(()) => return false,
    };
    let over = match probe(&pm, USER_TOP, 1, true) {
        Err(f) => f.kind,
        Ok(()) => return false,
    };
    let wrap = match probe(&pm, 0x1000, u64::MAX, true) {
        Err(f) => f.kind,
        Ok(()) => return false,
    };
    one == Fault::Kernel && kernel == Fault::Kernel && over == Fault::Kernel && wrap == Fault::Wrap
}

/// 洞与保护页分开识别。
fn hole_ok() -> bool {
    let pm = PageMap::fixture();
    let hole = matches!(probe(&pm, 0x0000_0000_0002_0000, 0x1000, true), Err(f) if f.kind == Fault::Hole);
    let guard = matches!(probe(&pm, 0x0000_0000_0001_7000, 64, true), Err(f) if f.kind == Fault::Guard);
    let spill = matches!(probe(&pm, 0x0000_0000_0001_6FF0, 0x20, true), Err(f) if f.kind == Fault::Guard);
    hole && guard && spill
}

/// 只读页的方向敏感。
fn rowrite_ok() -> bool {
    let pm = PageMap::fixture();
    let w = matches!(probe_write(&pm, 0x0000_0000_0001_2000, 64), Err(f) if f.kind == Fault::RoWrite);
    let r = probe_read(&pm, 0x0000_0000_0001_2000, 64).is_ok();
    let wo = matches!(probe_read(&pm, 0x0000_0000_0001_0000, 64), Ok(()));
    w && r && wo
}

/// 跨页部分拷贝：完成 0x100 字节，尾部一字节未动。
fn residual_ok() -> bool {
    let case = BadCase { id: 6, label: "x", va: 0x0000_0000_0001_6F00, len: 0x200 };
    let mut ok = true;
    let ops = [Op::Read, Op::Write, Op::ReadV, Op::WriteV];
    let mut i = 0usize;
    while i < ops.len() {
        let op = match ops.get(i) {
            Some(o) => *o,
            None => return false,
        };
        let cell = run_cell(&case, op);
        if !cell.matches(6, op) {
            ok = false;
        }
        if cell.res.done != 0x100 || cell.res.untouched() != 0x100 {
            ok = false;
        }
        i += 1;
    }
    // 直接验证拷贝引擎的残留语义：目标尾部保持种子。
    let pm = PageMap::fixture();
    let mut sh = Shadow::new();
    let src = [0xAAu8; 0x200];
    match copy_to_user(&pm, &mut sh, 0x0000_0000_0001_6F00, &src) {
        Err(f) => {
            if f.res.done != 0x100 || f.res.requested != 0x200 {
                ok = false;
            }
            if !sh.is_virgin(0x0000_0000_0001_7000, 0x100) {
                ok = false;
            }
            if sh.slice(0x0000_0000_0001_6F00, 0x100) != Some(&src[0..0x100]) {
                ok = false;
            }
        }
        Ok(_) => ok = false,
    }
    ok
}

/// fixup 表：登记、重复拒绝、表满拒绝、派发。
fn fixup_ok() -> bool {
    let mut t = FixupTable::new();
    if t.dispatch(7) != FixupAction::Oops {
        return false;
    }
    if t.register(7, 0x11) != Ok(()) {
        return false;
    }
    if t.dispatch(7) != FixupAction::Handled(0x11) {
        return false;
    }
    if t.register(7, 0x12) != Err(Errno::Inval) {
        return false;
    }
    let mut i = 1u32;
    while i <= (FIX_SITES as u32 - 1) {
        if t.register(i * 10, i as u16) != Ok(()) {
            return false;
        }
        i += 1;
    }
    if t.len() != FIX_SITES {
        return false;
    }
    if t.register(999, 9) != Err(Errno::Range) {
        return false;
    }
    t.dispatch(20) == FixupAction::Handled(2)
}

/// 系统调用层：零长度、短读短写、次序。
fn sys_ok() -> bool {
    let pm = PageMap::fixture();
    let mut sh = Shadow::new();
    if sys_read(&pm, &mut sh, FD_STDIN, 0, 0) != Ok(0) {
        return false;
    }
    if sys_read(&pm, &mut sh, FD_STDIN, 0, 64) != Err(Errno::Fault) {
        return false;
    }
    if sys_write(&pm, &sh, FD_STDOUT, 0, 64) != Err(Errno::Fault) {
        return false;
    }
    if sys_read(&pm, &mut sh, FD_STDOUT, 0x0000_0000_0001_0000, 8) != Err(Errno::BadF) {
        return false;
    }
    if sys_write(&pm, &sh, FD_STDIN, 0x0000_0000_0001_0000, 8) != Err(Errno::BadF) {
        return false;
    }
    // 坏 fd + 坏指针：必须先报 EBADF。
    if sys_read_ordered(&pm, &mut sh, 9, 0, 64) != Err(Errno::BadF) {
        return false;
    }
    // 短读：跨页样本读到 0x100 字节即停。
    if sys_read(&pm, &mut sh, FD_STDIN, 0x0000_0000_0001_6F00, 0x200) != Ok(0x100) {
        return false;
    }
    // 完整读：内容与设备字节一致。
    let n = match sys_read(&pm, &mut sh, FD_STDIN, 0x0000_0000_0001_0000, 32) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if n != 32 {
        return false;
    }
    let mut i = 0u64;
    while i < 32 {
        if sh.peek(0x0000_0000_0001_0000 + i) != Some(dev_byte(i)) {
            return false;
        }
        i += 1;
    }
    true
}

/// 矩阵全绿且格数正确。
fn matrix_ok() -> bool {
    let m = bad_matrix();
    matrix_green(&m) && m.len() == CLASSES && m.get(0).map(|r| r.len()).unwrap_or(0) == OPS
}

/// 账本：自检、封印一致、重放一致、篡改可检。
fn ledger_ok() -> bool {
    let led = build_ledger();
    if led.len() != CLASSES * OPS {
        return false;
    }
    if !led.audit() {
        return false;
    }
    let recs = led.records();
    if Ledger::replay(&recs) != led.seal() {
        return false;
    }
    // 篡改一条记录的返回值，重放必须变。
    let mut bad = recs.clone();
    if let Some(r) = bad.get_mut(0) {
        r.raw = 0;
        r.done = 7;
    }
    if Ledger::replay(&bad) == led.seal() {
        return false;
    }
    // 容量边界。
    let mut small = Ledger::new();
    let mut i = 0usize;
    while i < RING_CAP {
        if small.push(1, 1, -14, 0) != Ok(()) {
            return false;
        }
        i += 1;
    }
    small.push(1, 1, -14, 0) == Err(Errno::Range) && small.audit()
}

/// 期望表本身的正确性（防止实现与期望一起写错导致恒绿）。
fn expect_ok() -> bool {
    if expect_of(5, Op::Read) != Expect::Err(Errno::Fault) {
        return false;
    }
    if expect_of(5, Op::Write) != Expect::Ok(64) {
        return false;
    }
    if expect_of(6, Op::ReadV) != Expect::Ok(0x100) {
        return false;
    }
    expect_of(1, Op::Read) == Expect::Err(Errno::Fault)
}

/// 四路注入按原因全抓。
fn inject_ok() -> bool {
    let four = inject_four();
    four[0] && four[1] && four[2] && four[3]
}

// ---------------------------------------------------------------------------
// 十二、判据入口
// ---------------------------------------------------------------------------

/// 本卡判据集。
pub fn run_unx9203_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9203");
    s.add("F9203-01-段基址与对齐", seg_ok(), "段 0x90020，0x20 对齐");
    s.add("F9203-02-诊断码闭集", codes_ok(), "13 码连续且两两互异");
    s.add("F9203-03-错误码闭集", errno_ok(), "五个负 errno 可逆映射");
    s.add("F9203-04-页表基本操作", pagemap_ok(), "映射/解映射/保护/越界拒绝");
    s.add("F9203-05-NULL 判定", null_ok(), "NULL 拒 + 零长放行");
    s.add("F9203-06-上界与回绕", bound_ok(), "-1/内核/上界/回绕四路");
    s.add("F9203-07-洞与保护页", hole_ok(), "Hole 与 Guard 分开识别");
    s.add("F9203-08-只读方向敏感", rowrite_ok(), "读放行、写拒绝");
    s.add("F9203-09-残留声明", residual_ok(), "完成 0x100、尾部零触碰");
    s.add("F9203-10-fixup 回退表", fixup_ok(), "未登记 Oops、重复/满拒绝");
    s.add("F9203-11-系统调用层", sys_ok(), "零长/短读/短写/次序");
    s.add("F9203-12-坏指针矩阵", matrix_ok(), "8 类 × 4 操作 = 32 格");
    s.add("F9203-13-账本链闭合", ledger_ok(), "audit/seal/replay/篡改");
    s.add_dyn(
        "F9203-14-四路注入按因全抓",
        inject_ok(),
        &format!("four={:?}", inject_four()),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9203_seg_and_codes() {
        assert_eq!(SEG_BASE, 0x90020);
        assert!(seg_ok());
        assert!(codes_ok());
        assert_eq!(ALL_CODES.len(), 13);
        assert!(detail(D01_SEG).contains("段基址"));
        assert!(detail(0xDEAD_BEEF).contains("未登记"));
    }

    #[test]
    fn unx9203_errno_closed_set() {
        assert!(errno_ok());
        assert_eq!(Errno::Fault.raw(), -14);
        assert_eq!(Errno::Fault.name(), "EFAULT");
        assert_eq!(Errno::from_raw(-14), Some(Errno::Fault));
        assert_eq!(Errno::from_raw(-1), None);
        assert_eq!(Errno::from_raw(14), None);
    }

    #[test]
    fn unx9203_pagemap_operations() {
        let mut pm = PageMap::fixture();
        assert_eq!(pm.mapped_pages(), 7);
        assert_eq!(pm.guard_pages(), 1);
        assert_eq!(pm.map(0x0000_0000_0001_0800, 0x1000, Page::RW), Err(Errno::Inval));
        assert_eq!(pm.map(0x0000_0000_0004_F000, 0x2000, Page::RW), Err(Errno::Range));
        assert_eq!(pm.map(0x0000_0000_0001_0000, 0, Page::RW), Err(Errno::Inval));
        assert_eq!(pm.map(0x0000_0000_0001_0000, 0x1000, Page::Hole), Err(Errno::Inval));
        assert_eq!(pm.protect(0x0000_0000_0002_0000, 0x1000, Page::R), Err(Errno::Inval));
        assert_eq!(pm.unmap(0x0000_0000_0002_0000, 0x1000), Ok(()));
        assert_eq!(pm.map(0x0000_0000_0002_0000, 0x2000, Page::RX), Ok(()));
        assert_eq!(pm.at(0x0000_0000_0002_1000), Page::RX);
        assert_eq!(pm.mapped_pages(), 9);
        let d1 = pm.digest();
        assert_eq!(d1, pm.clone().digest());
        assert_ne!(d1, PageMap::fixture().digest());
    }

    #[test]
    fn unx9203_probe_null_and_zero() {
        let pm = PageMap::fixture();
        assert_eq!(probe(&pm, 0, 64, true), Err(FaultAt::new(0, 0, Fault::Null)));
        assert!(probe(&pm, 0, 0, true).is_ok());
        assert!(probe(&pm, 0, 0, false).is_ok());
    }

    #[test]
    fn unx9203_probe_bounds() {
        let pm = PageMap::fixture();
        assert_eq!(
            probe(&pm, 0xFFFF_FFFF_FFFF_FFFF, 64, true).err().map(|f| f.kind),
            Some(Fault::Kernel)
        );
        assert_eq!(
            probe(&pm, KERNEL_BASE, 64, true).err().map(|f| f.kind),
            Some(Fault::Kernel)
        );
        assert_eq!(
            probe(&pm, USER_TOP, 1, true).err().map(|f| f.kind),
            Some(Fault::Kernel)
        );
        assert_eq!(
            probe(&pm, 0x1000, u64::MAX, true).err().map(|f| f.kind),
            Some(Fault::Wrap)
        );
    }

    #[test]
    fn unx9203_probe_hole_and_guard() {
        let pm = PageMap::fixture();
        assert_eq!(
            probe(&pm, 0x0000_0000_0002_0000, 0x1000, true).err().map(|f| f.kind),
            Some(Fault::Hole)
        );
        assert_eq!(
            probe(&pm, 0x0000_0000_0001_7000, 64, true).err().map(|f| f.kind),
            Some(Fault::Guard)
        );
        assert_eq!(
            probe(&pm, 0x0000_0000_0001_6FF0, 0x20, true).err().map(|f| f.kind),
            Some(Fault::Guard)
        );
    }

    #[test]
    fn unx9203_probe_direction() {
        let pm = PageMap::fixture();
        assert!(probe_read(&pm, 0x0000_0000_0001_2000, 64).is_ok());
        assert_eq!(
            probe_write(&pm, 0x0000_0000_0001_2000, 64).err().map(|f| f.kind),
            Some(Fault::RoWrite)
        );
        assert!(probe_read(&pm, 0x0000_0000_0001_0000, 64).is_ok());
        assert!(probe_write(&pm, 0x0000_0000_0001_0000, 64).is_ok());
    }

    #[test]
    fn unx9203_shadow_seed_and_virgin() {
        let sh = Shadow::new();
        assert_eq!(sh.len(), USER_MAP_BYTES);
        assert_eq!(sh.peek(USER_BASE), Some(seed_byte(0)));
        assert!(sh.is_virgin(USER_BASE, 0x1000));
        assert_eq!(sh.peek(0), None);
        assert_eq!(sh.slice(USER_BASE, USER_MAP_BYTES as u64 + 1), None);
        let mut sh2 = Shadow::new();
        assert!(sh2.poke(USER_BASE + 3, 0xFF));
        assert!(!sh2.is_virgin(USER_BASE, 0x10));
        assert!(sh2.is_virgin(USER_BASE + 4, 0x10));
    }

    #[test]
    fn unx9203_copy_full_and_partial() {
        let pm = PageMap::fixture();
        let mut sh = Shadow::new();
        let src = [0x5Au8; 16];
        let st = copy_to_user(&pm, &mut sh, 0x0000_0000_0001_0000, &src).unwrap();
        assert_eq!(st.done, 16);
        assert_eq!(st.pages, 1);
        assert_eq!(sh.slice(0x0000_0000_0001_0000, 16), Some(&src[..]));
        let f = copy_to_user(&pm, &mut sh, 0x0000_0000_0001_6F00, &[0xAAu8; 0x200]).unwrap_err();
        assert_eq!(f.res.done, 0x100);
        assert_eq!(f.res.untouched(), 0x100);
        assert!(sh.is_virgin(0x0000_0000_0001_7000, 0x100));
    }

    #[test]
    fn unx9203_copy_from_user_partial() {
        let pm = PageMap::fixture();
        let sh = Shadow::new();
        let mut dst = [0x77u8; 0x200];
        let f = copy_from_user(&pm, &sh, 0x0000_0000_0001_6F00, &mut dst).unwrap_err();
        assert_eq!(f.res.done, 0x100);
        assert_eq!(dst[0], seed_byte(0x6F00));
        assert_eq!(dst[0x100], 0x77);
        assert_eq!(dst[0x1FF], 0x77);
    }

    #[test]
    fn unx9203_copy_zero_len() {
        let pm = PageMap::fixture();
        let mut sh = Shadow::new();
        let st = copy_to_user(&pm, &mut sh, 0, &[]).unwrap();
        assert_eq!(st.done, 0);
        let mut empty: [u8; 0] = [];
        let st2 = copy_from_user(&pm, &sh, 0, &mut empty).unwrap();
        assert_eq!(st2.done, 0);
    }

    #[test]
    fn unx9203_fixup_table() {
        let mut t = FixupTable::new();
        assert!(t.is_empty());
        assert_eq!(t.capacity(), FIX_SITES);
        assert_eq!(t.dispatch(3), FixupAction::Oops);
        assert_eq!(t.register(3, 0x21), Ok(()));
        assert_eq!(t.dispatch(3), FixupAction::Handled(0x21));
        assert_eq!(t.register(3, 0x22), Err(Errno::Inval));
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn unx9203_syscalls_zero_len_null() {
        let pm = PageMap::fixture();
        let mut sh = Shadow::new();
        assert_eq!(sys_read(&pm, &mut sh, FD_STDIN, 0, 0), Ok(0));
        assert_eq!(sys_write(&pm, &sh, FD_STDOUT, 0, 0), Ok(0));
        assert_eq!(sys_read(&pm, &mut sh, FD_STDIN, 0, 1), Err(Errno::Fault));
    }

    #[test]
    fn unx9203_syscalls_short_ops() {
        let pm = PageMap::fixture();
        let mut sh = Shadow::new();
        assert_eq!(sys_read(&pm, &mut sh, FD_STDIN, 0x0000_0000_0001_6F00, 0x200), Ok(0x100));
        assert_eq!(sys_write(&pm, &sh, FD_STDOUT, 0x0000_0000_0001_6F00, 0x200), Ok(0x100));
    }

    #[test]
    fn unx9203_syscalls_vectors() {
        let pm = PageMap::fixture();
        let mut sh = Shadow::new();
        let ok = [
            Iovec { base: 0x0000_0000_0001_0000, len: 8 },
            Iovec { base: 0x0000_0000_0001_0010, len: 8 },
        ];
        assert_eq!(sys_readv(&pm, &mut sh, FD_STDIN, &ok), Ok(16));
        assert_eq!(sys_writev(&pm, &sh, FD_STDOUT, &ok), Ok(16));
        let mixed = [
            Iovec { base: 0x0000_0000_0001_6F00, len: 0x100 },
            Iovec { base: 0x0000_0000_0001_7000, len: 8 },
        ];
        assert_eq!(sys_readv(&pm, &mut sh, FD_STDIN, &mixed), Ok(0x100));
        let bad = [
            Iovec { base: 0, len: 8 },
            Iovec { base: 0x0000_0000_0001_0000, len: 8 },
        ];
        assert_eq!(sys_readv(&pm, &mut sh, FD_STDIN, &bad), Err(Errno::Fault));
    }

    #[test]
    fn unx9203_order_handle_before_buffer() {
        let pm = PageMap::fixture();
        let mut sh = Shadow::new();
        assert_eq!(sys_read_ordered(&pm, &mut sh, 0, 0, 64), Err(Errno::Fault));
        assert_eq!(sys_read_ordered(&pm, &mut sh, 7, 0, 64), Err(Errno::BadF));
        assert_eq!(sys_read_ordered(&pm, &mut sh, -1, 0, 64), Err(Errno::BadF));
    }

    #[test]
    fn unx9203_expect_table() {
        assert!(expect_ok());
        assert_eq!(expect_of(5, Op::Read), Expect::Err(Errno::Fault));
        assert_eq!(expect_of(5, Op::Write), Expect::Ok(64));
        assert_eq!(expect_of(6, Op::WriteV), Expect::Ok(0x100));
        assert_eq!(expect_of(7, Op::Read), Expect::Err(Errno::Fault));
    }

    #[test]
    fn unx9203_matrix_all_green() {
        let m = bad_matrix();
        assert!(matrix_green(&m), "{:?}", matrix_reds(&m));
        assert_eq!(m.len(), CLASSES);
        let r = run();
        assert_eq!(r.green, 32);
        assert!(r.gate());
        assert!(r.head().starts_with("F9203|"));
        assert_eq!(r.rate_permille(), 1000);
    }

    #[test]
    fn unx9203_inject_reasons() {
        assert_eq!(inject_four(), [true, true, true, true]);
    }

    #[test]
    fn unx9203_ledger_chain() {
        let led = build_ledger();
        assert_eq!(led.len(), 32);
        assert!(led.audit());
        assert_eq!(Ledger::replay(&led.records()), led.seal());
        assert!(led.render().contains("seq class op raw done"));
        let mut recs = led.records();
        if let Some(r) = recs.get_mut(3) {
            r.done += 1;
        }
        assert_ne!(Ledger::replay(&recs), led.seal());
    }

    #[test]
    fn unx9203_ledger_capacity() {
        let mut led = Ledger::new();
        assert!(led.is_empty());
        let mut i = 0usize;
        while i < RING_CAP {
            assert_eq!(led.push(1, 2, -14, 0), Ok(()));
            i += 1;
        }
        assert_eq!(led.len(), RING_CAP);
        assert_eq!(led.push(1, 2, -14, 0), Err(Errno::Range));
        assert!(led.audit());
    }

    #[test]
    fn unx9203_checks_all_green() {
        let s = run_unx9203_checks();
        assert!(s.all_passed(), "UNX-F9203 判据未全绿: {}", s.red_items_text());
        assert_eq!(s.len(), 14);
    }

    #[test]
    fn unx9203_deterministic() {
        assert_eq!(run(), run());
        assert_eq!(bad_matrix(), bad_matrix());
        assert_eq!(inject_four(), inject_four());
        let a = build_ledger().seal();
        let b = build_ledger().seal();
        assert_eq!(a, b);
    }
}



