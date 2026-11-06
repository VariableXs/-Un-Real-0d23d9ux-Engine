//! UNX-F9367 参数探针联测（uaccess 拷贝 × C3 值域库） · 0x7C940段
//! ─────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9367 逐字）：
//!   UNX-F9367-J1 拷贝探针联测判据过，copy_from_user/copy_to_user 与 C3 值域
//!   库对接可观测，坏指针 EFAULT 对账格过。
//!
//! 设计要点：
//!   ① 三层判定，三层各有自己的 errno：值域层（C3 类型化实参的合法区间）
//!      回 EINVAL/E2BIG/EBADF，窗口层（用户地址窗口 + checked_add）回
//!      EFAULT，页层（逐页权限 Absent/Ro/Rw）也回 EFAULT 但带 fault_at。
//!      不允许用一个 EFAULT 盖住参数错误——那会让调用方把「我传错了」误读
//!      成「地址坏了」。
//!   ② 判序是第一性判据：read(fd, buf, count) 的次序固定为 fd（EBADF）→
//!      buf（EFAULT）→ count（EINVAL/E2BIG）→ 逐页搬运。值域层一旦拒绝，
//!      拷贝引擎一步都不许走（moved 恒 0）；否则「先搬一半再报参数错」就是
//!      真实内核缺陷——调用方已经看到部分副作用，却拿到参数错误。
//!   ③ 页层只读非对称：copy_from_user 从 Ro 页读是合法的（读权限足够），
//!      copy_to_user 写 Ro 页必须 EFAULT。同一个地址、两个方向、两个结论，
//!      这条只有把方向作为独立维度建模才能观测到。
//!   ④ 部分拷贝显性化：撞洞时 moved 与 fault_at 同时上报，残量 len-moved
//!      入账（partial_bytes）。不允许「静默截断后返回成功」。
//!   ⑤ 坏指针 12 格 × 10 轮对账：域级 5 格 / 页级 4 格 / 溢出 1 格 / 清洁
//!      2 格。每格逐轮复算 errno + moved + fault_at 三元组，账本满足
//!      injected == detected + escaped 且 escaped == 0。
//!   ⑥ 内容可观测不靠真搬字节，靠页标签求和（tag_sum）：每页固定一个标签
//!      字节，搬运路径上逐字节累加。跨页读的 tag_sum 必须等于两页标签按
//!      字节数的加权和——搬错段、搬少、搬重都会让这个和偏离。
//!   ⑦ 指纹 FNV-1a 32 位，双跑逐字节相等；无浮点、无 IO、无墙钟。
//! ─────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9367 独占 0x7C940..=0x7C97F）。
pub const SEG_BASE: u32 = 0x7C940;

/// 用户页大小（与内核 4K 页一致）。
pub const PAGE: u64 = 4096;
/// 页表槽位容量（三页实装 + 一槽余量，便于容量面注入）。
pub const PAGES: usize = 4;
/// 单格注入轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 坏指针对账格格数。
pub const FAULT_CELLS: usize = 12;
/// 拷贝预算（C3 Len 域上界，超出即 E2BIG）。
pub const COPY_BUDGET: i64 = 8192;
/// 用户窗口下界：null guard page 之上一字节。
pub const USER_LO: u64 = 0x1000;
/// 用户窗口上界：非规范低半区之下一字节。
pub const USER_HI: u64 = 0x0000_7FFF_FFFF_FFFF;
/// 探针使用的合法 fd（值域层 Fd 域上界内）。
pub const PROBE_FD: i64 = 3;

/// Linux errno（正数口径，逐码与内核一致）。
pub const E2BIG: i32 = 7;
pub const EBADF: i32 = 9;
pub const ENOMEM: i32 = 12;
pub const EFAULT: i32 = 14;
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// F9367 段内诊断码（12 个，段内序号 +0x01..=+0x0C）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeCode {
    /// 空指针进了窗口层
    NullPtr,
    /// 内核半区地址当用户指针
    KernelPtr,
    /// 非规范地址当用户指针
    NonCanonical,
    /// 未映射洞被踩
    UnmappedHole,
    /// 只读页被写
    ReadOnlyWrite,
    /// 半页跨界搬运
    HalfPageStraddle,
    /// 指针加长度回绕
    WrapAround,
    /// 值域拒绝却仍搬了字节
    DomainBypassed,
    /// 部分拷贝残量未入账
    PartialResidue,
    /// 判序倒置（参数错报成地址错）
    OrderInverted,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
}

impl ProbeCode {
    /// 段内码值（0x7C940 全宽收不进 u16，故取段基址低 16 位 + 序号）。
    pub fn code(self) -> u16 {
        (SEG_BASE as u16).wrapping_add(1 + self as u16)
    }

    /// 绝对诊断码（u32 全宽：段基址 + 序号，两两唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与下一步动作（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ProbeCode::NullPtr => "空指针被拒：buf=0 落在 null guard page 下界之外，回 EFAULT；调用方应先判空再进内核",
            ProbeCode::KernelPtr => "内核半区地址被拒：0xFFFF_8000_0000_0000 以上不是用户窗口，回 EFAULT；不要把内核对像地址当用户指针透传",
            ProbeCode::NonCanonical => "非规范地址被拒：0x0000_8000_0000_0000 以上超出用户窗口上界，回 EFAULT；请按 canonical 用户区重新取址",
            ProbeCode::UnmappedHole => "未映射洞被踩：页表缺页，回 EFAULT 并给出 fault_at；调用方需先 mmap 该段再拷",
            ProbeCode::ReadOnlyWrite => "只读页被写：copy_to_user 撞 Ro 页必须 EFAULT（读方向不受影响）；请改映射为可写或改走只读接口",
            ProbeCode::HalfPageStraddle => "跨页搬运停在页界：前段已搬、后段撞洞，moved 只计前段；请按页对齐拆成两次拷贝",
            ProbeCode::WrapAround => "指针加长度回绕：uptr+len 溢出 u64，回 EFAULT 且 moved=0；请先用 checked_add 校验区间",
            ProbeCode::DomainBypassed => "值域被绕过：值域层已拒绝但拷贝引擎仍搬了字节；判序必须为值域先于搬运",
            ProbeCode::PartialResidue => "部分拷贝残量未入账：len-moved 必须计入 partial_bytes，供调用方知悉副作用范围",
            ProbeCode::OrderInverted => "判序倒置：fd 应最先判（EBADF），buf 次之（EFAULT），count 再次（EINVAL/E2BIG）；倒置会把参数错报成地址错",
            ProbeCode::LedgerRegress => "注入账倒退：计数只增不减，回滚路径不得改写累计值",
            ProbeCode::NotReproducible => "双跑不一致：同输入两次结果不同，存在隐藏状态或非确定累加序",
        }
    }

    /// 闭集全量（文案与唯一性判据使用）。
    pub const ALL: &'static [ProbeCode] = &[
        ProbeCode::NullPtr,
        ProbeCode::KernelPtr,
        ProbeCode::NonCanonical,
        ProbeCode::UnmappedHole,
        ProbeCode::ReadOnlyWrite,
        ProbeCode::HalfPageStraddle,
        ProbeCode::WrapAround,
        ProbeCode::DomainBypassed,
        ProbeCode::PartialResidue,
        ProbeCode::OrderInverted,
        ProbeCode::LedgerRegress,
        ProbeCode::NotReproducible,
    ];
}

// ── C3 值域库 ─────────────────────────────────────────────────────────
/// 类型化实参类（C3 值域库的六个域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VClass {
    /// 有符号 32 位实参
    S32,
    /// 无符号 32 位实参
    U32,
    /// 长度实参（带拷贝预算）
    Len,
    /// 16 位标志字
    Flags16,
    /// 用户指针
    Ptr,
    /// 文件描述符
    Fd,
}

/// 一个值域：闭区间 [lo, hi]。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VDomain {
    pub class: VClass,
    pub lo: i64,
    pub hi: i64,
}

/// 六个域的冻结区间（顺序与 `VClass` 声明序一致）。
pub const DOMAINS: [VDomain; 6] = [
    VDomain { class: VClass::S32, lo: -2_147_483_648, hi: 2_147_483_647 },
    VDomain { class: VClass::U32, lo: 0, hi: 4_294_967_295 },
    VDomain { class: VClass::Len, lo: 0, hi: COPY_BUDGET },
    VDomain { class: VClass::Flags16, lo: 0, hi: 65_535 },
    VDomain { class: VClass::Ptr, lo: USER_LO as i64, hi: USER_HI as i64 },
    VDomain { class: VClass::Fd, lo: 0, hi: 1023 },
];

/// 按类取域（闭集，表内必命中；表外回 Len 域作保守下界）。
pub fn domain_of(class: VClass) -> VDomain {
    let mut i = 0usize;
    while i < DOMAINS.len() {
        if let Some(d) = DOMAINS.get(i) {
            if d.class == class {
                return *d;
            }
        }
        i += 1;
    }
    VDomain { class: VClass::Len, lo: 0, hi: 0 }
}

/// 值域拒绝时按类回不同的 errno：指针错不是参数错，fd 错也不是参数错。
fn reject_errno(class: VClass, below: bool) -> i32 {
    match class {
        VClass::Ptr => EFAULT,
        VClass::Fd => EBADF,
        VClass::Len => {
            if below {
                EINVAL
            } else {
                E2BIG
            }
        }
        _ => EINVAL,
    }
}

/// 值域判定：下界与上界分别给码（Len 下越界是 EINVAL，上越界是 E2BIG）。
pub fn admit(class: VClass, v: i64) -> Result<(), i32> {
    let d = domain_of(class);
    if v < d.lo {
        return Err(reject_errno(class, true));
    }
    if v > d.hi {
        return Err(reject_errno(class, false));
    }
    Ok(())
}

/// 值域判定编码为 i32（0 = 通过），供表驱动判据直接比对。
pub fn admit_code(class: VClass, v: i64) -> i32 {
    match admit(class, v) {
        Ok(()) => 0,
        Err(e) => e,
    }
}

/// 一行「值域 ↔ 拷贝」对接样例。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LinkRow {
    pub class: VClass,
    pub value: i64,
    pub want: i32,
}

/// 八行对接表：四类实参各自的界内/界外各取一点，外加指针窗口两端。
pub const LINK_ROWS: [LinkRow; 8] = [
    LinkRow { class: VClass::S32, value: 2_147_483_647, want: 0 },
    LinkRow { class: VClass::S32, value: 2_147_483_648, want: EINVAL },
    LinkRow { class: VClass::U32, value: -1, want: EINVAL },
    LinkRow { class: VClass::Len, value: COPY_BUDGET, want: 0 },
    LinkRow { class: VClass::Len, value: COPY_BUDGET + 1, want: E2BIG },
    LinkRow { class: VClass::Flags16, value: 65_536, want: EINVAL },
    LinkRow { class: VClass::Ptr, value: USER_LO as i64, want: 0 },
    LinkRow { class: VClass::Ptr, value: 0, want: EFAULT },
];

/// 逐行复算对接表，返回命中行数。
pub fn run_link_rows() -> u32 {
    let mut hit = 0u32;
    let mut i = 0usize;
    while i < LINK_ROWS.len() {
        if let Some(r) = LINK_ROWS.get(i) {
            if admit_code(r.class, r.value) == r.want {
                hit = hit.saturating_add(1);
            }
        }
        i += 1;
    }
    hit
}

// ── 地址空间（页级权限） ──────────────────────────────────────────────
/// 页权限三态。地址不在任何已装页里即 Absent。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PagePerm {
    Absent,
    Ro,
    Rw,
}

/// 一个已装页：基址（4K 对齐）+ 权限 + 内容标签。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UserPage {
    pub base: u64,
    pub perm: PagePerm,
    pub tag: u8,
}

/// 空页（数组重复初始化用；不能写 `[None; N]`，非 Copy 类型不成）。
pub const PAGE_INIT: UserPage = UserPage { base: 0, perm: PagePerm::Absent, tag: 0 };

impl UserPage {
    pub const fn empty() -> UserPage {
        PAGE_INIT
    }

    /// 本页是否覆盖 addr。
    pub fn covers(&self, addr: u64) -> bool {
        if self.perm == PagePerm::Absent {
            return false;
        }
        let end = self.base.saturating_add(PAGE);
        addr >= self.base && addr < end
    }
}

/// 定容地址空间：四槽，只装 4K 对齐、非 Absent 的页。
#[derive(Clone, Copy, Debug)]
pub struct AddrSpace {
    pages: [UserPage; PAGES],
    n: u32,
}

impl AddrSpace {
    pub const fn new() -> AddrSpace {
        AddrSpace { pages: [PAGE_INIT; PAGES], n: 0 }
    }

    pub fn len(&self) -> u32 {
        self.n
    }

    /// 装页：基址必须 4K 对齐且权限非 Absent，槽满回 ENOMEM。
    pub fn install(&mut self, base: u64, perm: PagePerm, tag: u8) -> Result<usize, i32> {
        if perm == PagePerm::Absent || base & (PAGE - 1) != 0 {
            return Err(EINVAL);
        }
        let idx = self.n as usize;
        if idx >= PAGES {
            return Err(ENOMEM);
        }
        if let Some(slot) = self.pages.get_mut(idx) {
            slot.base = base;
            slot.perm = perm;
            slot.tag = tag;
        }
        self.n = self.n.saturating_add(1);
        Ok(idx)
    }

    /// addr 所在页的权限；不在任何已装页里即 Absent。
    pub fn perm_at(&self, addr: u64) -> PagePerm {
        let mut i = 0usize;
        while i < PAGES {
            if let Some(p) = self.pages.get(i) {
                if p.covers(addr) {
                    return p.perm;
                }
            }
            i += 1;
        }
        PagePerm::Absent
    }

    /// addr 所在页的内容标签；缺页返回 None。
    pub fn tag_at(&self, addr: u64) -> Option<u8> {
        let mut i = 0usize;
        while i < PAGES {
            if let Some(p) = self.pages.get(i) {
                if p.covers(addr) {
                    return Some(p.tag);
                }
            }
            i += 1;
        }
        None
    }

    /// 以 base 起始的连续已装页数（容量面探针用）。
    pub fn run_of_mapped(&self, base: u64) -> u32 {
        let mut k = 0u32;
        while k < PAGES as u32 {
            let addr = base.saturating_add(PAGE * k as u64);
            if self.perm_at(addr) == PagePerm::Absent {
                break;
            }
            k = k.saturating_add(1);
        }
        k
    }

    /// 已装页总字节数。
    pub fn mapped_bytes(&self) -> u64 {
        PAGE * (self.n as u64)
    }
}

/// 裁决夹具用的标准地址空间：Rw(0x1000) / Ro(0x2000) / Rw(0x3000)，0x4000 留洞。
///
/// 装配必须成功；任一页装不上就回空表——后续判据会显性红，不会静默造绿。
pub fn standard_space() -> AddrSpace {
    let mut sp = AddrSpace::new();
    let a = sp.install(0x1000, PagePerm::Rw, 0x11);
    let b = sp.install(0x2000, PagePerm::Ro, 0x22);
    let c = sp.install(0x3000, PagePerm::Rw, 0x33);
    if a.is_err() || b.is_err() || c.is_err() {
        return AddrSpace::new();
    }
    sp
}

// ── 拷贝引擎 ──────────────────────────────────────────────────────────
/// 搬运方向。方向是独立维度：同一地址两个方向可以给出不同结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    /// 用户 → 内核（copy_from_user）：只要求可读
    FromUser,
    /// 内核 → 用户（copy_to_user）：要求可写
    ToUser,
}

/// 一次搬运的结果。`errno == 0` 即成功；失败时 `moved` 是已经搬走的字节数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CopyReport {
    pub moved: u32,
    pub fault_at: u64,
    pub errno: i32,
    pub tag_sum: u32,
}

impl CopyReport {
    pub const fn ok_report(moved: u32, tag_sum: u32) -> CopyReport {
        CopyReport { moved, fault_at: 0, errno: 0, tag_sum }
    }

    /// 值域层/窗口层的失败（尚未定位到具体字节，fault_at 记 0）。
    pub const fn param_fault(errno: i32) -> CopyReport {
        CopyReport { moved: 0, fault_at: 0, errno, tag_sum: 0 }
    }

    pub fn is_ok(&self) -> bool {
        self.errno == 0
    }

    /// 未搬完的残量（len - moved）；成功时恒 0。
    pub fn residue(&self, len: u32) -> u32 {
        len.saturating_sub(self.moved)
    }
}

/// 逐字节搬运模型：不碰真实内存，按页权限裁决，按页标签累加内容指纹。
///
/// 不变量：
///   - 成功时 moved == len 且 fault_at == 0；
///   - 失败时 fault_at 是第一个不可访问字节，moved 恰为其前已搬字节数；
///   - uptr+len 溢出时 moved == 0（连第一个字节都没搬）。
pub fn xfer(space: &AddrSpace, uptr: u64, len: u32, dir: Dir) -> CopyReport {
    if len == 0 {
        return CopyReport::ok_report(0, 0);
    }
    let end = match uptr.checked_add(len as u64) {
        Some(v) => v,
        None => return CopyReport { moved: 0, fault_at: uptr, errno: EFAULT, tag_sum: 0 },
    };
    let mut a = uptr;
    let mut moved = 0u32;
    let mut sum = 0u32;
    while a < end {
        let perm = space.perm_at(a);
        let allowed = match dir {
            Dir::FromUser => perm != PagePerm::Absent,
            Dir::ToUser => perm == PagePerm::Rw,
        };
        if !allowed {
            return CopyReport { moved, fault_at: a, errno: EFAULT, tag_sum: sum };
        }
        if let Some(t) = space.tag_at(a) {
            sum = sum.wrapping_add(t as u32);
        }
        moved = moved.saturating_add(1);
        a = a.saturating_add(1);
    }
    CopyReport::ok_report(moved, sum)
}

/// 系统调用实参三元组：read(fd, buf, count) / write(fd, buf, count)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Args {
    pub fd: i64,
    pub buf: i64,
    pub count: i64,
}

impl Args {
    pub const fn new(fd: i64, buf: i64, count: i64) -> Args {
        Args { fd, buf, count }
    }
}

/// 参数探针：值域层三段判序（fd → buf → count）之后再进搬运引擎。
///
/// 判序不可换：Linux 的 read/write 先查 fd（EBADF），再查地址（EFAULT），
/// 再查长度（EINVAL）。倒过来就会把「坏 fd」报成「坏地址」，调用方的错误
/// 恢复路径会走错分支。
pub fn syscall_copy_probe(space: &AddrSpace, a: Args, dir: Dir) -> CopyReport {
    if let Err(e) = admit(VClass::Fd, a.fd) {
        return CopyReport::param_fault(e);
    }
    if let Err(e) = admit(VClass::Ptr, a.buf) {
        return CopyReport::param_fault(e);
    }
    if let Err(e) = admit(VClass::Len, a.count) {
        return CopyReport::param_fault(e);
    }
    let n = match u32::try_from(a.count) {
        Ok(v) => v,
        Err(_) => return CopyReport::param_fault(EINVAL),
    };
    xfer(space, a.buf as u64, n, dir)
}

// ── 坏指针对账格 ──────────────────────────────────────────────────────
/// 一格的失败分类（对账时分栏计数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaultKind {
    /// 值域层拒绝（未进搬运）
    Domain,
    /// 页层缺页/只读（搬运中途或首字节）
    Page,
    /// 指针加长度回绕
    Overflow,
    /// 清洁格（必须成功）
    Clean,
}

/// 一个坏指针注入格。
///
/// `raw == true` 绕过值域层直接进搬运引擎——只有这样才够得着 `xfer` 里的
/// 回绕分支（值域层会把 u64 高位地址先拒掉，回绕路径就永远测不到）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FaultCell {
    pub uptr: i64,
    pub len: i32,
    pub dir: Dir,
    pub expect: i32,
    pub moved: u32,
    pub fault_at: u64,
    pub kind: FaultKind,
    pub raw: bool,
}

/// 十二格冻结表：五域级 / 四页级 / 一溢出 / 两清洁。
///
/// 逐格手算依据（标准空间：0x1000 Rw tag0x11、0x2000 Ro tag0x22、
/// 0x3000 Rw tag0x33、0x4000 洞）：
///   ①0 与 ②1 低于 USER_LO        → 域级 EFAULT, moved 0
///   ③-1（(void*)-1）             → 域级 EFAULT, moved 0
///   ④0x0000_8000_0000_0000 超上界 → 域级 EFAULT, moved 0
///   ⑤-0x8000_0000_0000 内核半区   → 域级 EFAULT, moved 0
///   ⑥0x4000 洞                    → 页级 EFAULT@0x4000, moved 0
///   ⑦0x2000 只读写                → 页级 EFAULT@0x2000, moved 0
///   ⑧0x2FF0 长 32 读跨 Ro→Rw      → 成功 moved 32
///   ⑨0x2FF0 长 32 写撞首字节 Ro   → 页级 EFAULT@0x2FF0, moved 0
///   ⑩0x3FF0 长 32 读撞洞          → 页级 EFAULT@0x4000, moved 16
///   ⑪0x1FF0 长 32 读跨 Rw→Ro      → 成功 moved 32
///   ⑫raw u64::MAX-8 长 16         → 回绕 EFAULT@uptr, moved 0
pub const FAULT_GRID: [FaultCell; FAULT_CELLS] = [
    FaultCell { uptr: 0, len: 16, dir: Dir::FromUser, expect: EFAULT, moved: 0, fault_at: 0, kind: FaultKind::Domain, raw: false },
    FaultCell { uptr: 1, len: 16, dir: Dir::FromUser, expect: EFAULT, moved: 0, fault_at: 0, kind: FaultKind::Domain, raw: false },
    FaultCell { uptr: -1, len: 16, dir: Dir::FromUser, expect: EFAULT, moved: 0, fault_at: 0, kind: FaultKind::Domain, raw: false },
    FaultCell { uptr: 0x0000_8000_0000_0000, len: 8, dir: Dir::FromUser, expect: EFAULT, moved: 0, fault_at: 0, kind: FaultKind::Domain, raw: false },
    FaultCell { uptr: -0x8000_0000_0000, len: 8, dir: Dir::FromUser, expect: EFAULT, moved: 0, fault_at: 0, kind: FaultKind::Domain, raw: false },
    FaultCell { uptr: 0x4000, len: 8, dir: Dir::FromUser, expect: EFAULT, moved: 0, fault_at: 0x4000, kind: FaultKind::Page, raw: false },
    FaultCell { uptr: 0x2000, len: 8, dir: Dir::ToUser, expect: EFAULT, moved: 0, fault_at: 0x2000, kind: FaultKind::Page, raw: false },
    FaultCell { uptr: 0x2FF0, len: 32, dir: Dir::FromUser, expect: 0, moved: 32, fault_at: 0, kind: FaultKind::Clean, raw: false },
    FaultCell { uptr: 0x2FF0, len: 32, dir: Dir::ToUser, expect: EFAULT, moved: 0, fault_at: 0x2FF0, kind: FaultKind::Page, raw: false },
    FaultCell { uptr: 0x3FF0, len: 32, dir: Dir::FromUser, expect: EFAULT, moved: 16, fault_at: 0x4000, kind: FaultKind::Page, raw: false },
    FaultCell { uptr: 0x1FF0, len: 32, dir: Dir::FromUser, expect: 0, moved: 32, fault_at: 0, kind: FaultKind::Clean, raw: false },
    FaultCell { uptr: -8, len: 16, dir: Dir::FromUser, expect: EFAULT, moved: 0, fault_at: u64::MAX - 7, kind: FaultKind::Overflow, raw: true },
];

/// 单调性探针格（与表首格同值，显式书写以免运行期下标）。
pub const FIRST_CELL: FaultCell = FaultCell {
    uptr: 0, len: 16, dir: Dir::FromUser, expect: EFAULT, moved: 0,
    fault_at: 0, kind: FaultKind::Domain, raw: false,
};

/// 跑一格（raw 格绕过值域层，直接进搬运引擎）。
pub fn run_cell(space: &AddrSpace, cell: FaultCell) -> CopyReport {
    let n = if cell.len < 0 { 0u32 } else { cell.len as u32 };
    if cell.raw {
        xfer(space, cell.uptr as u64, n, cell.dir)
    } else {
        syscall_copy_probe(space, Args::new(PROBE_FD, cell.uptr, cell.len as i64), cell.dir)
    }
}

/// 坏指针注入账本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FaultLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
    pub domain_faults: u32,
    pub page_faults: u32,
    pub overflow_faults: u32,
    pub clean: u32,
    pub moved_total: u64,
    pub partial_bytes: u64,
}

impl FaultLedger {
    pub const fn zero() -> FaultLedger {
        FaultLedger { injected: 0, detected: 0, escaped: 0, domain_faults: 0,
            page_faults: 0, overflow_faults: 0, clean: 0, moved_total: 0, partial_bytes: 0 }
    }

    /// 记一轮：`detected` 由三元组（errno/moved/fault_at）完全匹配决定。
    pub fn record(&mut self, cell: FaultCell, rep: CopyReport, detected: bool) -> Result<(), i32> {
        self.injected = self.injected.saturating_add(1);
        if detected {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        match cell.kind {
            FaultKind::Domain => self.domain_faults = self.domain_faults.saturating_add(1),
            FaultKind::Page => self.page_faults = self.page_faults.saturating_add(1),
            FaultKind::Overflow => self.overflow_faults = self.overflow_faults.saturating_add(1),
            FaultKind::Clean => self.clean = self.clean.saturating_add(1),
        }
        self.moved_total = self.moved_total.saturating_add(rep.moved as u64);
        // 设计要点④：残量只在「部分拷贝」入账——即搬运已经启动
        // （moved > 0）却因页层拒绝而中止；首字节即故障（moved == 0）
        // 没有副作用，不算残量。
        if cell.kind == FaultKind::Page && rep.errno != 0 && rep.moved > 0 {
            let len = if cell.len < 0 { 0u32 } else { cell.len as u32 };
            self.partial_bytes = self
                .partial_bytes
                .saturating_add(rep.residue(len) as u64);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(EINVAL);
        }
        Ok(())
    }

    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 逐格 × 逐轮复算，返回 (账本, 每格命中轮数)。
pub fn run_fault_grid() -> (FaultLedger, [u32; FAULT_CELLS]) {
    let space = standard_space();
    let mut ledger = FaultLedger::zero();
    let mut per_cell = [0u32; FAULT_CELLS];
    let mut ci = 0usize;
    while ci < FAULT_GRID.len() {
        if let Some(cell) = FAULT_GRID.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let rep = run_cell(&space, cell);
                let ok = rep.errno == cell.expect
                    && rep.moved == cell.moved
                    && rep.fault_at == cell.fault_at;
                if ledger.record(cell, rep, ok).is_err() {
                    // 账目自洽性破裂：本轮不造绿，直接跳出（后续判据会红）。
                    return (ledger, per_cell);
                }
                if ok {
                    if let Some(slot) = per_cell.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round = round.saturating_add(1);
            }
        }
        ci += 1;
    }
    (ledger, per_cell)
}

// ── 内容可观测：页标签加权和 ──────────────────────────────────────────
/// 按页标签计算 [addr, addr+len) 的期望标签和（独立于搬运引擎的第二算路）。
pub fn expected_tag_sum(space: &AddrSpace, addr: u64, len: u32) -> u32 {
    let mut sum = 0u32;
    let mut k = 0u32;
    while k < len {
        if let Some(t) = space.tag_at(addr.saturating_add(k as u64)) {
            sum = sum.wrapping_add(t as u32);
        }
        k = k.saturating_add(1);
    }
    sum
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（整数同余，无浮点/随机/墙钟）。
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

/// 把一个搬运结果编码进字节流（定宽，便于逐字节对账）。
fn encode_report(out: &mut Vec<u8>, r: CopyReport) {
    out.push(if r.errno == 0 { 1 } else { 2 });
    out.push((r.errno & 0x7F) as u8);
    out.push((r.moved & 0xFF) as u8);
    out.push(((r.moved >> 8) & 0xFF) as u8);
    out.push((r.fault_at & 0xFF) as u8);
    out.push(((r.fault_at >> 8) & 0xFF) as u8);
    out.push((r.tag_sum & 0xFF) as u8);
    out.push(((r.tag_sum >> 8) & 0xFF) as u8);
}

/// 规范语料指纹：十二格全跑一遍 + 三条判序样例 + 一条跨页读。
pub fn corpus_fingerprint() -> u32 {
    let space = standard_space();
    let mut bytes: Vec<u8> = Vec::new();
    let mut ci = 0usize;
    while ci < FAULT_GRID.len() {
        if let Some(cell) = FAULT_GRID.get(ci).copied() {
            encode_report(&mut bytes, run_cell(&space, cell));
        }
        ci += 1;
    }
    // 判序样例：坏 fd 先于坏地址；坏地址先于坏长度；域拒后 moved 恒 0。
    encode_report(&mut bytes, syscall_copy_probe(&space, Args::new(-1, 0, 16), Dir::FromUser));
    encode_report(&mut bytes, syscall_copy_probe(&space, Args::new(PROBE_FD, 0, 16), Dir::FromUser));
    encode_report(&mut bytes, syscall_copy_probe(&space, Args::new(PROBE_FD, 0x1000, COPY_BUDGET + 1), Dir::FromUser));
    // 全长跨页读（0x1000..0x3000，两页全命）。
    encode_report(&mut bytes, xfer(&space, 0x1000, 8192, Dir::FromUser));
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9367 判据集（add ≥12；add_dyn ≤8）。
pub fn run_unx9367_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9367");

    s.add(
        "段位守卫：0x7C940 落在本批窗口且 0x40 对齐",
        SEG_BASE == 0x7C940 && SEG_BASE % 0x40 == 0 && (0x7C000..=0x7D1C0).contains(&SEG_BASE),
        "段基址唯一，段间不得互相覆盖",
    );

    let s32_ok = admit_code(VClass::S32, -2_147_483_648) == 0
        && admit_code(VClass::S32, 2_147_483_647) == 0
        && admit_code(VClass::S32, 2_147_483_648) == EINVAL
        && admit_code(VClass::S32, -2_147_483_649) == EINVAL;
    s.add(
        "C3 值域：S32 上下界恰在端点放行、越界一即拒",
        s32_ok,
        "有符号域必须两端都测，不能只测正越界",
    );

    let wide_ok = admit_code(VClass::U32, 0) == 0
        && admit_code(VClass::U32, 4_294_967_295) == 0
        && admit_code(VClass::U32, 4_294_967_296) == EINVAL
        && admit_code(VClass::U32, -1) == EINVAL
        && admit_code(VClass::Flags16, 65_535) == 0
        && admit_code(VClass::Flags16, 65_536) == EINVAL;
    s.add(
        "C3 值域：U32 与 16 位标志字上界逐格",
        wide_ok,
        "无符号域必须拒负值：负实参转无符号后是巨大正值，静默放行即越界",
    );

    let len_ok = admit_code(VClass::Len, 0) == 0
        && admit_code(VClass::Len, COPY_BUDGET) == 0
        && admit_code(VClass::Len, COPY_BUDGET + 1) == E2BIG
        && admit_code(VClass::Len, -1) == EINVAL;
    s.add(
        "C3 值域：长度预算上越界 E2BIG、下越界 EINVAL",
        len_ok,
        "长度上越界与负长度是两类错误：前者超声明容量，后者是参数符号错",
    );

    let ptr_fd_ok = admit_code(VClass::Ptr, USER_LO as i64) == 0
        && admit_code(VClass::Ptr, (USER_HI - 1) as i64) == 0
        && admit_code(VClass::Ptr, 0) == EFAULT
        && admit_code(VClass::Ptr, USER_HI as i64 + 1) == EFAULT
        && admit_code(VClass::Fd, 1023) == 0
        && admit_code(VClass::Fd, 1024) == EBADF
        && admit_code(VClass::Fd, -1) == EBADF;
    s.add(
        "C3 值域：指针窗口两端与 fd 表上界逐格",
        ptr_fd_ok,
        "指针错回 EFAULT、fd 错回 EBADF：两类错不能合并成一个码",
    );

    let link_hit = run_link_rows();
    s.add(
        "C3 值域库对接：八行表逐行命中 8/8",
        link_hit == LINK_ROWS.len() as u32,
        "值域库是本模块的唯一判据源，表外取样不得绕过",
    );

    let space = standard_space();
    let order_fd = syscall_copy_probe(&space, Args::new(-1, 0, 16), Dir::FromUser);
    let order_buf = syscall_copy_probe(&space, Args::new(PROBE_FD, 0, 16), Dir::FromUser);
    let order_cnt = syscall_copy_probe(&space, Args::new(PROBE_FD, 0x1000, -1), Dir::FromUser);
    s.add(
        "判序：坏 fd 报 EBADF 而非 EFAULT，坏地址才报 EFAULT",
        order_fd.errno == EBADF && order_buf.errno == EFAULT && order_cnt.errno == EINVAL,
        "fd → buf → count 的判别次序是 Linux read/write 的既有语义",
    );

    let budget = syscall_copy_probe(
        &space,
        Args::new(PROBE_FD, 0x1000, COPY_BUDGET + 1),
        Dir::FromUser,
    );
    s.add(
        "判序：长度超预算 E2BIG 且 moved 恒 0（域层先于搬运）",
        budget.errno == E2BIG && budget.moved == 0 && budget.tag_sum == 0,
        "域层拒绝后一个字节都不许搬，否则调用方会看到半截副作用",
    );

    let plain = syscall_copy_probe(&space, Args::new(PROBE_FD, 0x1000, 16), Dir::FromUser);
    let plain_want = 0x11u32 * 16;
    s.add(
        "正常流：单页读搬满 16 字节且标签和 = 0x11×16",
        plain.is_ok()
            && plain.moved == 16
            && plain.tag_sum == plain_want
            && plain.fault_at == 0,
        "清洁路径必须 moved==len 且内容可复算",
    );

    let mut cap = standard_space();
    let fourth = cap.install(0x5000, PagePerm::Rw, 0x44);
    let fifth = cap.install(0x6000, PagePerm::Rw, 0x55);
    s.add(
        "容量面：四槽装到第 4 页成功，第 5 页回 ENOMEM",
        fourth == Ok(3)
            && fifth == Err(ENOMEM)
            && cap.len() == PAGES as u32
            && cap.mapped_bytes() == PAGE * PAGES as u64
            && space.run_of_mapped(0x1000) == 3,
        "前 0x1000/0x2000/0x3000 三页连续，0x4000 起断；槽满必须显性拒",
    );

    let straddle = xfer(&space, 0x1FF0, 32, Dir::FromUser);
    s.add(
        "跨页读：0x1FF0 起 32 字节跨 Rw→Ro 两页全通",
        straddle.errno == 0
            && straddle.moved == 32
            && straddle.tag_sum == expected_tag_sum(&space, 0x1FF0, 32)
            && straddle.tag_sum == 0x110 + 0x220,
        "跨页读的标签和必须是两页标签按字节数加权，搬错段即偏离",
    );

    let ro_read = xfer(&space, 0x2000, 8, Dir::FromUser);
    let ro_write = xfer(&space, 0x2000, 8, Dir::ToUser);
    s.add(
        "只读非对称：Ro 页可读、不可写（同址两个方向两个结论）",
        ro_read.errno == 0
            && ro_read.moved == 8
            && ro_write.errno == EFAULT
            && ro_write.moved == 0
            && ro_write.fault_at == 0x2000,
        "只按地址分类建不出这条：方向必须独立成维度",
    );

    let half = xfer(&space, 0x2FF0, 32, Dir::ToUser);
    s.add(
        "半页跨界写：首字节即在 Ro 页，moved 0 且 fault_at 精确",
        half.errno == EFAULT && half.moved == 0 && half.fault_at == 0x2FF0,
        "写方向撞 Ro 必须停在第一个字节，不许把前段写进去",
    );

    let hole = xfer(&space, 0x3FF0, 32, Dir::FromUser);
    s.add(
        "撞洞残量：前 16 字节已搬、fault_at=0x4000、残量 16",
        hole.errno == EFAULT
            && hole.moved == 16
            && hole.fault_at == 0x4000
            && hole.residue(32) == 16
            && hole.tag_sum == 0x33 * 16,
        "部分拷贝必须显性上报 moved 与残量，不许静默截断",
    );

    let wrap = xfer(&space, u64::MAX - 7, 16, Dir::FromUser);
    s.add(
        "回绕：uptr+len 溢出回 EFAULT 且 moved=0、fault_at=uptr",
        wrap.errno == EFAULT && wrap.moved == 0 && wrap.fault_at == u64::MAX - 7,
        "地址算术一律 checked：回绕的区间不是长区间，是非法区间",
    );

    let (ledger, per_cell) = run_fault_grid();
    let all_cells = per_cell.iter().all(|n| *n == INJECT_ROUNDS);
    s.add(
        "坏指针 12 格 × 10 轮：逐格 10/10 检出",
        all_cells && ledger.detected == (FAULT_CELLS as u32) * INJECT_ROUNDS,
        "每格必须轮轮命中，任何一格抖动都说明裁决依赖隐藏状态",
    );

    s.add(
        "对账账本：注入 == 检出 + 逃逸，逃逸恒 0",
        ledger.conserved() && ledger.escaped == 0 && ledger.injected == 120,
        "120 次注入必须 120 次检出：漏检即判据失效",
    );

    let split_ok = ledger.domain_faults == 5 * INJECT_ROUNDS
        && ledger.page_faults == 4 * INJECT_ROUNDS
        && ledger.overflow_faults == INJECT_ROUNDS
        && ledger.clean == 2 * INJECT_ROUNDS;
    s.add(
        "对账分栏：域级 50 / 页级 40 / 溢出 10 / 清洁 20",
        split_ok,
        "四栏之和必须等于注入总数，错一栏就说明分类面漏了格",
    );

    let residue_ok = ledger.partial_bytes == 16 * INJECT_ROUNDS as u64
        && ledger.moved_total == 80 * INJECT_ROUNDS as u64;
    s.add(
        "残量与搬运量入账：每轮残量 16、搬走 80 字节",
        residue_ok,
        "残量是调用方恢复的唯一依据，必须逐轮精确",
    );

    let mut mono = ledger;
    let before = mono.injected;
    let rep0 = run_cell(&space, FIRST_CELL);
    let mono_probe = mono.record(FIRST_CELL, rep0, rep0.errno == FIRST_CELL.expect);
    s.add(
        "账目单调：追加注入只增不减",
        mono.injected == before + 1 && mono.conserved() && mono_probe.is_ok(),
        "累计计数禁止回退；回滚路径改写累计值即账目失真",
    );

    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add(
        "双跑一致：规范语料指纹逐字节相等",
        fp_a == fp_b,
        "同输入两次必须相等，隐藏状态会立刻暴露",
    );

    let mut uniq = true;
    let mut i = 0usize;
    while i < ProbeCode::ALL.len() {
        let mut j = i + 1;
        while j < ProbeCode::ALL.len() {
            if let (Some(a), Some(b)) = (ProbeCode::ALL.get(i), ProbeCode::ALL.get(j)) {
                if a.val() == b.val() || a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add(
        "诊断码：12 个码值（段内与绝对）两两唯一",
        uniq,
        "段内序号与绝对码值都必须唯一，否则红项无法定位",
    );

    s.add(
        "文案闭集：12 个诊断码均有中文建议",
        ProbeCode::ALL.iter().all(|c| !c.detail().is_empty()),
        "每个码都要给得出下一步动作，不许只给结论",
    );

    let cell_text = format!(
        "域级 {} / 页级 {} / 溢出 {} / 清洁 {}，共 {} 次注入",
        ledger.domain_faults, ledger.page_faults, ledger.overflow_faults, ledger.clean, ledger.injected
    );
    s.add_dyn(
        "坏指针账目",
        ledger.conserved() && ledger.escaped == 0,
        &cell_text,
    );

    let tag_text = format!(
        "单页 {} / 跨页 {} / 撞洞 {}",
        plain.tag_sum, straddle.tag_sum, hole.tag_sum
    );
    s.add_dyn(
        "标签和快照",
        plain.tag_sum == 0x110 && straddle.tag_sum == 0x330 && hole.tag_sum == 0x330,
        &tag_text,
    );

    let fp_text = format!("语料指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_domain_bounds() {
        assert_eq!(admit_code(VClass::S32, 2_147_483_647), 0);
        assert_eq!(admit_code(VClass::S32, 2_147_483_648), EINVAL);
        assert_eq!(admit_code(VClass::U32, -1), EINVAL);
        assert_eq!(admit_code(VClass::Len, COPY_BUDGET + 1), E2BIG);
        assert_eq!(admit_code(VClass::Fd, 1024), EBADF);
        assert_eq!(admit_code(VClass::Ptr, 0), EFAULT);
    }

    #[test]
    fn fd_is_checked_before_pointer() {
        let space = standard_space();
        let r = syscall_copy_probe(&space, Args::new(-1, 0, 16), Dir::FromUser);
        assert_eq!(r.errno, EBADF);
        assert_eq!(r.moved, 0);
        let r2 = syscall_copy_probe(&space, Args::new(PROBE_FD, 0, 16), Dir::FromUser);
        assert_eq!(r2.errno, EFAULT);
    }

    #[test]
    fn len_budget_rejects_before_copy() {
        let space = standard_space();
        let r = syscall_copy_probe(
            &space,
            Args::new(PROBE_FD, 0x1000, COPY_BUDGET + 1),
            Dir::FromUser,
        );
        assert_eq!(r.errno, E2BIG);
        assert_eq!(r.moved, 0);
        assert_eq!(r.tag_sum, 0);
    }

    #[test]
    fn page_read_only_is_asymmetric() {
        let space = standard_space();
        let rd = xfer(&space, 0x2000, 8, Dir::FromUser);
        let wr = xfer(&space, 0x2000, 8, Dir::ToUser);
        assert_eq!(rd.errno, 0);
        assert_eq!(wr.errno, EFAULT);
        assert_eq!(wr.fault_at, 0x2000);
    }

    #[test]
    fn hole_stops_partial_copy() {
        let space = standard_space();
        let r = xfer(&space, 0x3FF0, 32, Dir::FromUser);
        assert_eq!(r.moved, 16);
        assert_eq!(r.fault_at, 0x4000);
        assert_eq!(r.residue(32), 16);
        assert_eq!(r.tag_sum, 0x33 * 16);
    }

    #[test]
    fn wrap_is_detected_without_moving() {
        let space = standard_space();
        let r = xfer(&space, u64::MAX - 7, 16, Dir::FromUser);
        assert_eq!(r.errno, EFAULT);
        assert_eq!(r.moved, 0);
        assert_eq!(r.fault_at, u64::MAX - 7);
    }

    #[test]
    fn zero_length_is_always_ok() {
        let space = standard_space();
        let r = xfer(&space, 0x4000, 0, Dir::FromUser);
        assert_eq!(r.errno, 0);
        assert_eq!(r.moved, 0);
        assert_eq!(r.tag_sum, 0);
    }

    #[test]
    fn grid_is_conserved_and_split() {
        let (l, per) = run_fault_grid();
        assert_eq!(l.injected, 120);
        assert_eq!(l.detected, 120);
        assert_eq!(l.escaped, 0);
        assert_eq!(l.domain_faults, 50);
        assert_eq!(l.page_faults, 40);
        assert_eq!(l.overflow_faults, 10);
        assert_eq!(l.clean, 20);
        assert_eq!(l.partial_bytes, 160);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn tag_sums_are_weighted_by_page() {
        let space = standard_space();
        assert_eq!(xfer(&space, 0x1FF0, 32, Dir::FromUser).tag_sum, 0x110 + 0x220);
        assert_eq!(xfer(&space, 0x2FF0, 32, Dir::FromUser).tag_sum, 0x220 + 0x330);
    }

    #[test]
    fn fingerprint_is_stable() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn codes_are_unique_and_documented() {
        let mut i = 0usize;
        while i < ProbeCode::ALL.len() {
            let mut j = i + 1;
            while j < ProbeCode::ALL.len() {
                if let (Some(a), Some(b)) = (ProbeCode::ALL.get(i), ProbeCode::ALL.get(j)) {
                    assert_ne!(a.val(), b.val());
                    assert_ne!(a.code(), b.code());
                }
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9367_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
    }
}
