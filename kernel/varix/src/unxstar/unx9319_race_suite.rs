//! UNX-F9319 E 型竞态参数探针总成 · 0x7BD40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9319 逐字）：
//!   ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9319-J1 nargs 逐位提取判据过，坏
//!   指针全 EFAULT 格过，与参数拷贝探针单源格过；③ 完成时 API/软件写回：状态=
//!   已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 系统调用描述符是 16 位压缩字：低 3 位是 nargs，其后每个实参占 2 位类型
//!      （0 空位 / 1 整数 / 2 指针 / 3 长度）。nargs **逐位提取**：三位各自移位
//!      或回，不做整字段掩码抄写；类型也逐位取，越界（idx >= nargs）一律 None。
//!      这样表里的 nargs 与类型编码不可能「一处改了两处没改」。
//!   ② 指针探针只有一个入口 MemWindow::probe。参数校验阶段与拷贝阶段都调它，
//!      这就是「单源」：两条路径若给出不同结论，只可能是窗口状态变了（竞态），
//!      而不是两套判据漂移。探针自带 probe_calls 计数，调用方据此核对两条路径
//!      确实各走了一次同一函数。
//!   ③ E 型竞态的模型是「校验后被撤页」：先探一次得可访问，随后把该页标为
//!      未映射，再探一次必须 EFAULT。内核在拷贝期重新校验，因此用户态看到的
//!      是 EFAULT 而不是脏数据或半拷贝；这条链跑 10 轮，另一变体（整条
//!      syscall 探针）同样 10 轮，合计 20 次注入。
//!   ④ 坏指针面按格展开：空指针、全窗口外、尾页缺失、映射缺页四类各成一格，
//!      加上「零长度配空指针放行」与「非对齐但映射内放行」两条反向格——坏指针
//!      判据不是「一律拒绝」，而是「按页判可访问性」。
//!   ⑤ 顺序：实参从左到右探，首个坏参数即返回，后续不再探（probes 计数即证据）。
//!      这样坏 argv 不会掩盖坏 path，也不会在已定案后再触发一次缺页。
//!   ⑥ 诊断码 12 个、码值唯一；指纹覆盖描述符、十格结论与竞态轮次。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9319 独占 0x7BD40..=0x7BD7F）。
pub const SEG_BASE: u32 = 0x7BD40;
/// 段窗口（每卡 0x40）。
pub const SEG_SPAN: u32 = 0x40;
pub const SEG_WINDOW_LO: u32 = 0x7B000;
pub const SEG_WINDOW_HI: u32 = 0x7BFFF;

// ── Linux errno（正数口径） ───────────────────────────────────────────
pub const E2BIG: i32 = 7;
pub const EBADF: i32 = 9;
pub const EFAULT: i32 = 14;
pub const EINVAL: i32 = 22;
pub const ENOTTY: i32 = 25;
pub const ENOSYS: i32 = 38;

/// 页移位与页大小（掩码/移位代替除法）。
pub const PAGE_SHIFT: u32 = 12;
/// 页大小。
pub const PAGE: u64 = 1 << PAGE_SHIFT;
/// 探针窗口页数。
pub const WINDOW_PAGES: usize = 8;
/// 窗口首址（64 位用户态语料区，2M 对齐）。
pub const WINDOW_BASE: u64 = 0x0000_7F00_0000_0000;
/// 表内系统调用数。
pub const SYSCALL_COUNT: usize = 12;
/// 冻结探针格数。
pub const PROBE_CELLS: usize = 10;
/// 竞态注入轮数（每轮两变体）。
pub const INJECT_ROUNDS: u32 = 10;
/// 表内指针实参总数（冻结值）。
pub const EXPECTED_PTR_ARGS: u32 = 12;

// ── 描述符位布局 ──────────────────────────────────────────────────────
/// 类型编码：空位。
pub const K_NONE: u32 = 0;
/// 类型编码：整数。
pub const K_INT: u32 = 1;
/// 类型编码：用户指针。
pub const K_PTR: u32 = 2;
/// 类型编码：长度/计数。
pub const K_SIZE: u32 = 3;

/// 实参类型。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArgKind {
    None = 0,
    Int = 1,
    Ptr = 2,
    Size = 3,
}

impl ArgKind {
    /// 类型码。
    pub fn code(self) -> u32 {
        self as u32
    }

    /// 是否需要按用户指针探测。
    pub fn is_ptr(self) -> bool {
        matches!(self, ArgKind::Ptr)
    }
}

/// 压缩描述符：低 3 位 nargs，其后 6 个 2 位类型槽。
pub const fn make_desc(nargs: u32, k0: u32, k1: u32, k2: u32, k3: u32, k4: u32, k5: u32) -> u32 {
    (nargs & 0x7)
        | ((k0 & 0x3) << 4)
        | ((k1 & 0x3) << 6)
        | ((k2 & 0x3) << 8)
        | ((k3 & 0x3) << 10)
        | ((k4 & 0x3) << 12)
        | ((k5 & 0x3) << 14)
}

/// 逐位提取 nargs（三位各自移位或回，不做整字段抄写）。
pub fn nargs_of(desc: u32) -> u32 {
    let mut n = 0u32;
    let mut bit = 0u32;
    while bit < 3 {
        let v = (desc >> bit) & 0x1;
        n |= v << bit;
        bit = bit.saturating_add(1);
    }
    n
}

/// 逐位提取第 idx 个实参类型；越界（idx >= nargs 或 idx >= 6）返回 None。
pub fn kind_of(desc: u32, idx: u32) -> Option<ArgKind> {
    if idx >= nargs_of(desc) || idx >= 6 {
        return None;
    }
    let off = 4u32.saturating_add(idx.saturating_mul(2));
    let bits = (desc >> off) & 0x3;
    match bits {
        0 => Some(ArgKind::None),
        1 => Some(ArgKind::Int),
        2 => Some(ArgKind::Ptr),
        3 => Some(ArgKind::Size),
        _ => None,
    }
}

/// 描述符内指针实参个数。
pub fn ptr_args_of(desc: u32) -> u32 {
    let n = nargs_of(desc);
    let mut i = 0u32;
    let mut cnt = 0u32;
    while i < n {
        if kind_of(desc, i) == Some(ArgKind::Ptr) {
            cnt = cnt.saturating_add(1);
        }
        i = i.saturating_add(1);
    }
    cnt
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// F9319 诊断码（段内 +1..=+12）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeCode {
    DescriptorGap,
    NargsDrift,
    KindDrift,
    BadArgNotPtr,
    NotApplicableMisjudged,
    BadPtrMissed,
    LegalPtrRejected,
    CrossPageMissed,
    ZeroLenRejected,
    RaceEscaped,
    ProbeSplit,
    NotReproducible,
}

impl ProbeCode {
    /// 段内序号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 段内码值。
    pub fn val(self) -> u32 {
        SEG_BASE.saturating_add(1).saturating_add(self.code() as u32)
    }

    /// 中文建议。
    pub fn detail(self) -> &'static str {
        match self {
            ProbeCode::DescriptorGap => "描述符有缺口：nargs 与类型槽必须自洽，空槽不得声明为有参数",
            ProbeCode::NargsDrift => "nargs 提取漂移：逐位回读必须等于冻结值，否则参数面整体错位",
            ProbeCode::KindDrift => "实参类型漂移：指针位被改成整数位就会漏探，反之会误判",
            ProbeCode::BadArgNotPtr => "坏参数位置不是指针型：冻结的坏参数下标必须指向指针槽",
            ProbeCode::NotApplicableMisjudged => "不适用被误判成失败：无指针系统调用不得产生 EFAULT",
            ProbeCode::BadPtrMissed => "坏指针漏检：空指针/窗口外/缺页都必须回 EFAULT",
            ProbeCode::LegalPtrRejected => "合法指针被拒：映射内即使非对齐也应按页放行",
            ProbeCode::CrossPageMissed => "跨界漏检：区间跨两页时尾页缺失同样要 EFAULT",
            ProbeCode::ZeroLenRejected => "零长度被拒：count 为 0 时内核不触碰用户缓冲，应放行",
            ProbeCode::RaceEscaped => "竞态逃逸：校验后撤页必须在拷贝期重新判定为 EFAULT",
            ProbeCode::ProbeSplit => "探针分叉：校验与拷贝必须共用同一探针函数与同一计数",
            ProbeCode::NotReproducible => "双跑不一致：同一窗口两次结论不同，探针里藏了可变状态",
        }
    }

    /// 闭集全量。
    pub const ALL: &'static [ProbeCode] = &[
        ProbeCode::DescriptorGap,
        ProbeCode::NargsDrift,
        ProbeCode::KindDrift,
        ProbeCode::BadArgNotPtr,
        ProbeCode::NotApplicableMisjudged,
        ProbeCode::BadPtrMissed,
        ProbeCode::LegalPtrRejected,
        ProbeCode::CrossPageMissed,
        ProbeCode::ZeroLenRejected,
        ProbeCode::RaceEscaped,
        ProbeCode::ProbeSplit,
        ProbeCode::NotReproducible,
    ];
}

// ── 系统调用描述表 ────────────────────────────────────────────────────
/// 一条系统调用条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SysEntry {
    pub nr: u32,
    pub name: &'static str,
    pub desc: u32,
    /// 冻结的「坏参数」位置（必须是指针槽；无指针调用为 None）。
    pub bad_arg: Option<u32>,
}

/// 十二行冻结表（x86-64 编号）。
pub const SYSCALLS: [SysEntry; SYSCALL_COUNT] = [
    SysEntry {
        nr: 0,
        name: "read",
        desc: make_desc(3, K_INT, K_PTR, K_SIZE, K_NONE, K_NONE, K_NONE),
        bad_arg: Some(1),
    },
    SysEntry {
        nr: 1,
        name: "write",
        desc: make_desc(3, K_INT, K_PTR, K_SIZE, K_NONE, K_NONE, K_NONE),
        bad_arg: Some(1),
    },
    SysEntry {
        nr: 2,
        name: "open",
        desc: make_desc(3, K_PTR, K_INT, K_INT, K_NONE, K_NONE, K_NONE),
        bad_arg: Some(0),
    },
    SysEntry {
        nr: 5,
        name: "fstat",
        desc: make_desc(2, K_INT, K_PTR, K_NONE, K_NONE, K_NONE, K_NONE),
        bad_arg: Some(1),
    },
    SysEntry {
        nr: 7,
        name: "poll",
        desc: make_desc(3, K_PTR, K_SIZE, K_INT, K_NONE, K_NONE, K_NONE),
        bad_arg: Some(0),
    },
    SysEntry {
        nr: 9,
        name: "mmap",
        desc: make_desc(6, K_PTR, K_SIZE, K_INT, K_INT, K_INT, K_SIZE),
        bad_arg: Some(0),
    },
    SysEntry {
        nr: 16,
        name: "ioctl",
        desc: make_desc(3, K_INT, K_INT, K_PTR, K_NONE, K_NONE, K_NONE),
        bad_arg: Some(2),
    },
    SysEntry {
        nr: 19,
        name: "readv",
        desc: make_desc(3, K_INT, K_PTR, K_SIZE, K_NONE, K_NONE, K_NONE),
        bad_arg: Some(1),
    },
    SysEntry {
        nr: 39,
        name: "getpid",
        desc: make_desc(0, K_NONE, K_NONE, K_NONE, K_NONE, K_NONE, K_NONE),
        bad_arg: None,
    },
    SysEntry {
        nr: 59,
        name: "execve",
        desc: make_desc(3, K_PTR, K_PTR, K_PTR, K_NONE, K_NONE, K_NONE),
        bad_arg: Some(0),
    },
    SysEntry {
        nr: 62,
        name: "kill",
        desc: make_desc(2, K_INT, K_INT, K_NONE, K_NONE, K_NONE, K_NONE),
        bad_arg: None,
    },
    SysEntry {
        nr: 257,
        name: "openat",
        desc: make_desc(4, K_INT, K_PTR, K_INT, K_INT, K_NONE, K_NONE),
        bad_arg: Some(1),
    },
];

/// 按编号查表。
pub fn lookup(nr: u32) -> Option<&'static SysEntry> {
    let mut i = 0usize;
    while i < SYSCALLS.len() {
        if let Some(e) = SYSCALLS.get(i) {
            if e.nr == nr {
                return Some(e);
            }
        }
        i = i.saturating_add(1);
    }
    None
}

/// 表内 nargs 逐位回读是否全部与冻结值一致（用描述符自校验：nargs 位 == 类型槽数）。
pub fn nargs_matches_frozen() -> bool {
    let frozen: [u32; SYSCALL_COUNT] = [3, 3, 3, 2, 3, 6, 3, 3, 0, 3, 2, 4];
    let mut i = 0usize;
    while i < SYSCALLS.len() {
        match (SYSCALLS.get(i), frozen.get(i)) {
            (Some(e), Some(expect)) => {
                if nargs_of(e.desc) != *expect || nargs_of(e.desc) > 6 {
                    return false;
                }
            }
            _ => return false,
        }
        i = i.saturating_add(1);
    }
    true
}

/// 类型槽与冻结序列一致（逐位比对两位一槽）。
pub fn kinds_match_frozen() -> bool {
    // 每行六个类型码，冻结为压缩序列。
    let frozen: [[u32; 6]; SYSCALL_COUNT] = [
        [K_INT, K_PTR, K_SIZE, K_NONE, K_NONE, K_NONE],
        [K_INT, K_PTR, K_SIZE, K_NONE, K_NONE, K_NONE],
        [K_PTR, K_INT, K_INT, K_NONE, K_NONE, K_NONE],
        [K_INT, K_PTR, K_NONE, K_NONE, K_NONE, K_NONE],
        [K_PTR, K_SIZE, K_INT, K_NONE, K_NONE, K_NONE],
        [K_PTR, K_SIZE, K_INT, K_INT, K_INT, K_SIZE],
        [K_INT, K_INT, K_PTR, K_NONE, K_NONE, K_NONE],
        [K_INT, K_PTR, K_SIZE, K_NONE, K_NONE, K_NONE],
        [K_NONE, K_NONE, K_NONE, K_NONE, K_NONE, K_NONE],
        [K_PTR, K_PTR, K_PTR, K_NONE, K_NONE, K_NONE],
        [K_INT, K_INT, K_NONE, K_NONE, K_NONE, K_NONE],
        [K_INT, K_PTR, K_INT, K_INT, K_NONE, K_NONE],
    ];
    let mut i = 0usize;
    while i < SYSCALLS.len() {
        let e = match SYSCALLS.get(i) {
            Some(v) => v,
            None => return false,
        };
        let row = match frozen.get(i) {
            Some(v) => v,
            None => return false,
        };
        let n = nargs_of(e.desc);
        let mut j = 0u32;
        while j < 6 {
            let got = kind_of(e.desc, j);
            let want = match row.get(j as usize) {
                Some(v) => *v,
                None => return false,
            };
            if j < n {
                if got != Some(kind_from_code(want)) {
                    return false;
                }
            } else if got.is_some() {
                return false;
            }
            j = j.saturating_add(1);
        }
        i = i.saturating_add(1);
    }
    true
}

/// 类型码 → 类型。
pub fn kind_from_code(code: u32) -> ArgKind {
    match code {
        1 => ArgKind::Int,
        2 => ArgKind::Ptr,
        3 => ArgKind::Size,
        _ => ArgKind::None,
    }
}

/// 坏参数位置必须是指针槽且下标合法。
pub fn bad_args_are_ptr() -> bool {
    let mut i = 0usize;
    while i < SYSCALLS.len() {
        if let Some(e) = SYSCALLS.get(i) {
            match e.bad_arg {
                Some(idx) => {
                    if idx >= nargs_of(e.desc) {
                        return false;
                    }
                    if kind_of(e.desc, idx) != Some(ArgKind::Ptr) {
                        return false;
                    }
                }
                None => {
                    if ptr_args_of(e.desc) != 0 {
                        return false;
                    }
                }
            }
        }
        i = i.saturating_add(1);
    }
    true
}

/// 表内指针实参总数。
pub fn total_ptr_args() -> u32 {
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < SYSCALLS.len() {
        if let Some(e) = SYSCALLS.get(i) {
            sum = sum.saturating_add(ptr_args_of(e.desc));
        }
        i = i.saturating_add(1);
    }
    sum
}

// ── 页窗口与单源探针 ──────────────────────────────────────────────────
/// 页状态。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PageState {
    Unmapped = 0,
    Mapped = 1,
}

/// 探针结论。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeVerdict {
    Accessible = 0,
    Fault = 1,
}

impl ProbeVerdict {
    /// 结论码。
    pub fn code(self) -> u32 {
        self as u32
    }

    /// 结论 → errno（可访问为 0）。
    pub fn errno(self) -> i32 {
        match self {
            ProbeVerdict::Accessible => 0,
            ProbeVerdict::Fault => EFAULT,
        }
    }
}

/// 用户地址窗口：八页 + 单源探针调用计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MemWindow {
    pages: [PageState; WINDOW_PAGES],
    probe_calls: u32,
}

impl MemWindow {
    /// 全未映射。
    pub fn empty() -> MemWindow {
        MemWindow {
            pages: [PageState::Unmapped; WINDOW_PAGES],
            probe_calls: 0,
        }
    }

    /// 规范窗口：0..=6 页映射，第 7 页缺页。
    pub fn canonical() -> MemWindow {
        let mut w = MemWindow::empty();
        let mut i = 0usize;
        while i < WINDOW_PAGES.saturating_sub(1) {
            if let Some(p) = w.pages.get_mut(i) {
                *p = PageState::Mapped;
            }
            i = i.saturating_add(1);
        }
        w
    }

    /// 单页改状态；下标越界返回 false。
    pub fn set_page(&mut self, idx: usize, state: PageState) -> bool {
        match self.pages.get_mut(idx) {
            Some(slot) => {
                *slot = state;
                true
            }
            None => false,
        }
    }

    /// 取页状态。
    pub fn page_state(&self, idx: usize) -> Option<PageState> {
        self.pages.get(idx).copied()
    }

    /// 单源探针调用计数。
    pub fn probe_calls(&self) -> u32 {
        self.probe_calls
    }

    /// 地址所属页下标（不在窗口内返回 None）。
    pub fn page_index(&self, addr: u64) -> Option<usize> {
        if addr < WINDOW_BASE {
            return None;
        }
        let off = addr.saturating_sub(WINDOW_BASE);
        let idx = off >> PAGE_SHIFT;
        if idx >= WINDOW_PAGES as u64 {
            return None;
        }
        Some(idx as usize)
    }

    /// 单源探针：校验阶段与拷贝阶段共用此函数。
    /// 语义：len==0 视为不触碰用户缓冲；否则区间覆盖的每一页都必须映射。
    pub fn probe(&mut self, addr: u64, len: u64) -> ProbeVerdict {
        self.probe_calls = self.probe_calls.saturating_add(1);
        if len == 0 {
            return ProbeVerdict::Accessible;
        }
        let end = match addr.checked_add(len) {
            Some(v) => v,
            None => return ProbeVerdict::Fault,
        };
        let first = match self.page_index(addr) {
            Some(v) => v,
            None => return ProbeVerdict::Fault,
        };
        let last = match self.page_index(end.saturating_sub(1)) {
            Some(v) => v,
            None => return ProbeVerdict::Fault,
        };
        if last < first {
            return ProbeVerdict::Fault;
        }
        let mut i = first;
        while i <= last {
            match self.pages.get(i) {
                Some(PageState::Mapped) => {}
                _ => return ProbeVerdict::Fault,
            }
            i = i.saturating_add(1);
        }
        ProbeVerdict::Accessible
    }

    /// 只读探针（不计数）：用于「窗口是否保持不变」的旁证。
    pub fn peek(&self, addr: u64, len: u64) -> ProbeVerdict {
        if len == 0 {
            return ProbeVerdict::Accessible;
        }
        let mut copy = *self;
        copy.probe_calls = 0;
        copy.probe(addr, len)
    }
}

/// 一条 syscall 的探针结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProbeOutcome {
    pub verdict: ProbeVerdict,
    pub errno: i32,
    pub first_bad: Option<u32>,
    pub probes: u32,
    pub applicable: bool,
}

impl ProbeOutcome {
    /// 空结论（无指针实参的系统调用）。
    pub const fn not_applicable() -> ProbeOutcome {
        ProbeOutcome {
            verdict: ProbeVerdict::Accessible,
            errno: 0,
            first_bad: None,
            probes: 0,
            applicable: false,
        }
    }
}

/// 指针实参的伴随长度：下一个槽是 Size 时取该值，否则按一页。
pub fn ptr_len(desc: u32, args: &[u64; 6], i: u32) -> u64 {
    let next = i.saturating_add(1);
    if kind_of(desc, next) == Some(ArgKind::Size) {
        match args.get(next as usize) {
            Some(v) => *v,
            None => PAGE,
        }
    } else {
        PAGE
    }
}

/// 整条 syscall 探针：左到右，首个坏参数即返回。
pub fn probe_syscall(w: &mut MemWindow, e: &SysEntry, args: &[u64; 6]) -> ProbeOutcome {
    let nargs = nargs_of(e.desc);
    let mut i = 0u32;
    let mut probes = 0u32;
    while i < nargs {
        if kind_of(e.desc, i) == Some(ArgKind::Ptr) {
            let addr = match args.get(i as usize) {
                Some(v) => *v,
                None => 0,
            };
            let len = ptr_len(e.desc, args, i);
            let v = w.probe(addr, len);
            probes = probes.saturating_add(1);
            if v == ProbeVerdict::Fault {
                return ProbeOutcome {
                    verdict: ProbeVerdict::Fault,
                    errno: EFAULT,
                    first_bad: Some(i),
                    probes,
                    applicable: true,
                };
            }
        }
        i = i.saturating_add(1);
    }
    if probes == 0 {
        ProbeOutcome::not_applicable()
    } else {
        ProbeOutcome {
            verdict: ProbeVerdict::Accessible,
            errno: 0,
            first_bad: None,
            probes,
            applicable: true,
        }
    }
}

// ── 冻结探针格 ────────────────────────────────────────────────────────
/// 一格格：编号 + 实参 + 期望 errno。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProbeCell {
    pub nr: u32,
    pub args: [u64; 6],
    pub expect_errno: i32,
    pub note: &'static str,
}

/// 十格冻结探针（五好五坏）。
pub const PROBES: [ProbeCell; PROBE_CELLS] = [
    ProbeCell {
        nr: 0,
        args: [3, WINDOW_BASE, 64, 0, 0, 0],
        expect_errno: 0,
        note: "正常缓冲：首页映射且长度在页内",
    },
    ProbeCell {
        nr: 0,
        args: [3, 0, 64, 0, 0, 0],
        expect_errno: EFAULT,
        note: "空指针：地址 0 不在任何窗口内",
    },
    ProbeCell {
        nr: 0,
        args: [3, WINDOW_BASE + 1, 8, 0, 0, 0],
        expect_errno: 0,
        note: "非对齐但映射内：按页判定，不要求对齐",
    },
    ProbeCell {
        nr: 0,
        args: [3, WINDOW_BASE + 8 * PAGE, 8, 0, 0, 0],
        expect_errno: EFAULT,
        note: "整窗口之外：页下标越界",
    },
    ProbeCell {
        nr: 0,
        args: [3, WINDOW_BASE + 6 * PAGE, 2 * PAGE, 0, 0, 0],
        expect_errno: EFAULT,
        note: "跨两页且尾页缺页：只看首页会漏检",
    },
    ProbeCell {
        nr: 1,
        args: [3, WINDOW_BASE + 7 * PAGE, 8, 0, 0, 0],
        expect_errno: EFAULT,
        note: "缺页：第 7 页显式未映射",
    },
    ProbeCell {
        nr: 16,
        args: [3, 0x5401, WINDOW_BASE + 2 * PAGE, 0, 0, 0],
        expect_errno: 0,
        note: "ioctl 第三参数为指针：映射内放行",
    },
    ProbeCell {
        nr: 257,
        args: [3, 4096, 0, 0, 0, 0],
        expect_errno: EFAULT,
        note: "openat 路径指针在窗口外：低地址不可访问",
    },
    ProbeCell {
        nr: 62,
        args: [4660, 15, 0, 0, 0, 0],
        expect_errno: 0,
        note: "kill 无指针实参：探针不适用而非失败",
    },
    ProbeCell {
        nr: 0,
        args: [3, 0, 0, 0, 0, 0],
        expect_errno: 0,
        note: "零长度配空指针：内核不触碰缓冲，放行",
    },
];

/// 跑全部探针格，返回（命中格数, 结论快照）。
pub fn run_probe_grid() -> (u32, Vec<ProbeOutcome>) {
    let mut matched = 0u32;
    let mut outs: Vec<ProbeOutcome> = Vec::new();
    let mut i = 0usize;
    while i < PROBES.len() {
        if let Some(cell) = PROBES.get(i).copied() {
            let mut w = MemWindow::canonical();
            let out = match lookup(cell.nr) {
                Some(e) => probe_syscall(&mut w, e, &cell.args),
                None => ProbeOutcome::not_applicable(),
            };
            if out.errno == cell.expect_errno {
                matched = matched.saturating_add(1);
            }
            outs.push(out);
        }
        i = i.saturating_add(1);
    }
    (matched, outs)
}

// ── 竞态注入 ──────────────────────────────────────────────────────────
/// 竞态账本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RaceLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl RaceLedger {
    /// 零值。
    pub const fn zero() -> RaceLedger {
        RaceLedger {
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 记一笔。
    pub fn record(&mut self, caught: bool) -> u32 {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        self.injected
    }

    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 竞态注入：两种变体各 10 轮。
/// 变体 A：裸探针「校验 → 撤页 → 拷贝探针」。
/// 变体 B：整条 syscall 探针在同一窗口上跑两遍，中间撤页。
pub fn run_race_injection() -> (RaceLedger, u32, u32) {
    let mut ledger = RaceLedger::zero();
    let mut variant_a = 0u32;
    let mut variant_b = 0u32;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        // 变体 A
        let mut wa = MemWindow::canonical();
        let first = wa.probe(WINDOW_BASE + PAGE, 8);
        let flipped = wa.set_page(1, PageState::Unmapped);
        let second = wa.probe(WINDOW_BASE + PAGE, 8);
        let caught_a = flipped
            && first == ProbeVerdict::Accessible
            && second == ProbeVerdict::Fault
            && wa.probe_calls() == 2;
        ledger.record(caught_a);
        if caught_a {
            variant_a = variant_a.saturating_add(1);
        }

        // 变体 B
        let mut wb = MemWindow::canonical();
        let entry = match lookup(0) {
            Some(e) => e,
            None => return (ledger, variant_a, variant_b),
        };
        let args = [3u64, WINDOW_BASE + 2 * PAGE, 16, 0, 0, 0];
        let pre = probe_syscall(&mut wb, entry, &args);
        let flipped_b = wb.set_page(2, PageState::Unmapped);
        let post = probe_syscall(&mut wb, entry, &args);
        let caught_b = flipped_b
            && pre.verdict == ProbeVerdict::Accessible
            && pre.errno == 0
            && post.verdict == ProbeVerdict::Fault
            && post.errno == EFAULT
            && post.first_bad == Some(1)
            && wb.probe_calls() == 2;
        ledger.record(caught_b);
        if caught_b {
            variant_b = variant_b.saturating_add(1);
        }

        round = round.saturating_add(1);
    }
    (ledger, variant_a, variant_b)
}

/// 单源判据：两条路径（校验/拷贝）在窗口不变时结论一致，且各记一次探针。
pub fn single_source_agrees() -> bool {
    let mut i = 0usize;
    while i < PROBES.len() {
        let cell = match PROBES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let entry = match lookup(cell.nr) {
            Some(v) => v,
            None => return false,
        };
        let mut w1 = MemWindow::canonical();
        let mut w2 = MemWindow::canonical();
        let o1 = probe_syscall(&mut w1, entry, &cell.args);
        let o2 = probe_syscall(&mut w2, entry, &cell.args);
        if o1 != o2 {
            return false;
        }
        if w1.probe_calls() != o1.probes || w2.probe_calls() != o2.probes {
            return false;
        }
        // 同窗口重复探：窗口未变则结论必须逐次相同（校验路径 == 拷贝路径）。
        let addr = match cell.args.get(1) {
            Some(v) => *v,
            None => 0,
        };
        let before = w1.peek(addr, PAGE);
        let after = w1.peek(addr, PAGE);
        if before != after {
            return false;
        }
        i = i.saturating_add(1);
    }
    true
}

/// 顺序判据：execve 的 path 好、argv 坏，必须停在 argv 且只探两次。
pub fn order_stops_at_first_bad() -> bool {
    let entry = match lookup(59) {
        Some(e) => e,
        None => return false,
    };
    let mut w = MemWindow::canonical();
    let args = [WINDOW_BASE, 1u64, WINDOW_BASE, 0, 0, 0];
    let out = probe_syscall(&mut w, entry, &args);
    out.verdict == ProbeVerdict::Fault
        && out.errno == EFAULT
        && out.first_bad == Some(1)
        && out.probes == 2
        && w.probe_calls() == 2
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i = i.saturating_add(1);
    }
    h
}

fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.push((v >> 24) as u8);
    out.push((v >> 16) as u8);
    out.push((v >> 8) as u8);
    out.push(v as u8);
}

fn push_u64(out: &mut Vec<u8>, v: u64) {
    push_u32(out, (v >> 32) as u32);
    push_u32(out, v as u32);
}

/// 规范语料指纹：描述符、十格结论、竞态轮次。
pub fn corpus_fingerprint() -> u32 {
    let mut out: Vec<u8> = Vec::new();
    push_u32(&mut out, SEG_BASE);
    let mut i = 0usize;
    while i < SYSCALLS.len() {
        if let Some(e) = SYSCALLS.get(i) {
            push_u32(&mut out, e.nr);
            push_u32(&mut out, e.desc);
            push_u32(&mut out, nargs_of(e.desc));
        }
        i = i.saturating_add(1);
    }
    let (_matched, outs) = run_probe_grid();
    let mut k = 0usize;
    while k < outs.len() {
        if let Some(o) = outs.get(k) {
            push_u32(&mut out, o.verdict.code());
            push_u32(&mut out, o.errno as u32);
            push_u32(&mut out, o.probes);
            push_u32(&mut out, if o.applicable { 1 } else { 0 });
        }
        k = k.saturating_add(1);
    }
    let (ledger, va, vb) = run_race_injection();
    push_u32(&mut out, ledger.injected);
    push_u32(&mut out, ledger.detected);
    push_u32(&mut out, ledger.escaped);
    push_u32(&mut out, va);
    push_u32(&mut out, vb);
    // 窗口布局一并入指纹，防止「结论对但窗口被改」的假绿。
    let w = MemWindow::canonical();
    let mut p = 0usize;
    while p < WINDOW_PAGES {
        if let Some(st) = w.page_state(p) {
            push_u32(&mut out, st as u32);
        }
        p = p.saturating_add(1);
    }
    push_u64(&mut out, WINDOW_BASE);
    fnv1a(&out)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9319 判据集。
pub fn run_unx9319_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9319");

    let seg_ok = SEG_BASE % SEG_SPAN == 0
        && (SEG_WINDOW_LO..=SEG_WINDOW_HI).contains(&SEG_BASE)
        && SEG_BASE == 0x7BD40;
    s.add(
        "段位守卫：0x40 对齐且落在本批窗口",
        seg_ok,
        "0x7BD40..=0x7BD7F 归本卡，不得与前后卡交叉",
    );

    s.add(
        "nargs 逐位提取：十二条与冻结值一致",
        nargs_matches_frozen() && SYSCALLS.len() == SYSCALL_COUNT,
        "三位逐位回读必须等于冻结 nargs，且不超过六",
    );

    s.add(
        "参数类型：逐位两码一槽与冻结序列一致",
        kinds_match_frozen(),
        "空槽不得声明类型，nargs 内每槽必须与冻结码相同",
    );

    s.add(
        "坏参数位置：必须是指针槽且下标合法",
        bad_args_are_ptr(),
        "冻结坏参数若落在整数槽，坏指针判据就形同虚设",
    );

    s.add(
        "指针实参总数：十二格与表内计数一致",
        total_ptr_args() == EXPECTED_PTR_ARGS && EXPECTED_PTR_ARGS == 12,
        "逐行指针数之和必须等于冻结总数",
    );

    let (matched, outs) = run_probe_grid();
    s.add(
        "坏指针全 EFAULT：五格无一漏检",
        matched == PROBE_CELLS as u32
            && outs.iter().filter(|o| o.errno == EFAULT).count() == 5,
        "空指针/窗口外/缺页/跨页五格必须全回 EFAULT",
    );

    s.add(
        "正常指针：五格全放行（含非对齐与零长度）",
        outs.iter().filter(|o| o.errno == 0).count() == 5,
        "按页判定可访问性，不要求对齐；零长度不触碰缓冲",
    );

    let cross = outs.get(4).copied();
    let cross_ok = match cross {
        Some(o) => {
            o.verdict == ProbeVerdict::Fault
                && o.errno == EFAULT
                && o.first_bad == Some(1)
                && o.probes == 1
        }
        None => false,
    };
    s.add(
        "跨界：跨两页且尾页缺页必须 EFAULT",
        cross_ok,
        "只检查首页会把缺掉的尾页放过去，等于脏拷贝",
    );

    let zero_len = outs.get(9).copied();
    let zero_ok = match zero_len {
        Some(o) => o.errno == 0 && o.verdict == ProbeVerdict::Accessible && o.applicable,
        None => false,
    };
    s.add(
        "零长度：空指针配 count=0 放行",
        zero_ok,
        "内核在 count 为 0 时不触碰用户缓冲，拒绝反而破坏兼容",
    );

    let not_app = outs.get(8).copied();
    let not_app_ok = match not_app {
        Some(o) => !o.applicable && o.errno == 0 && o.probes == 0 && o.first_bad.is_none(),
        None => false,
    };
    s.add(
        "不适用：kill 无指针实参不产生 EFAULT",
        not_app_ok,
        "无指针系统调用必须判为不适用，不能记成坏指针",
    );

    let (ledger, va, vb) = run_race_injection();
    let expect_inject = INJECT_ROUNDS.saturating_mul(2);
    s.add(
        "竞态：校验后撤页两变体各 10 轮全检出",
        ledger.conserved()
            && ledger.escaped == 0
            && ledger.injected == expect_inject
            && ledger.detected == expect_inject
            && va == INJECT_ROUNDS
            && vb == INJECT_ROUNDS,
        "20 次注入必须 20 次在拷贝期判成 EFAULT",
    );

    s.add(
        "单源：校验与拷贝探针同函数同计数",
        single_source_agrees(),
        "两窗口逐格结论一致，且探针计数等于指针实参个数",
    );

    s.add(
        "顺序：首个坏参数即返回，后续不再探",
        order_stops_at_first_bad(),
        "execve 的 path 好 argv 坏时必须停在 argv，只探两次",
    );

    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add(
        "双跑一致：描述符与探针结论指纹相等",
        fp_a == fp_b && fp_a != 0,
        "同窗口两次必须逐字节相等，探针不得有隐藏状态",
    );

    s.add(
        "诊断码：12 个码文案非空",
        ProbeCode::ALL.len() == 12 && ProbeCode::ALL.iter().all(|c| !c.detail().is_empty()),
        "每个码都要给出可操作的中文建议",
    );

    let mut uniq = true;
    let mut a = 0usize;
    while a < ProbeCode::ALL.len() {
        let mut b = a.saturating_add(1);
        while b < ProbeCode::ALL.len() {
            if let (Some(x), Some(y)) = (ProbeCode::ALL.get(a), ProbeCode::ALL.get(b)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            b = b.saturating_add(1);
        }
        a = a.saturating_add(1);
    }
    s.add(
        "诊断码：12 个码值两两唯一",
        uniq,
        "段内闭集码不得重复",
    );

    let grid_text = format!(
        "探针格 {}/{} 指针参数 {}",
        matched,
        PROBE_CELLS,
        total_ptr_args()
    );
    s.add_dyn(
        "探针格快照",
        matched == PROBE_CELLS as u32,
        &grid_text,
    );
    let race_text = format!(
        "竞态 {}/{} 变体 {}/{}",
        ledger.detected, ledger.injected, va, vb
    );
    s.add_dyn(
        "竞态账快照",
        ledger.conserved() && ledger.escaped == 0,
        &race_text,
    );
    let fp_text = format!("指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desc_bit_extraction() {
        let d = make_desc(3, K_INT, K_PTR, K_SIZE, K_NONE, K_NONE, K_NONE);
        assert_eq!(nargs_of(d), 3);
        assert_eq!(kind_of(d, 0), Some(ArgKind::Int));
        assert_eq!(kind_of(d, 1), Some(ArgKind::Ptr));
        assert_eq!(kind_of(d, 2), Some(ArgKind::Size));
        assert_eq!(kind_of(d, 3), None);
        assert_eq!(ptr_args_of(d), 1);
    }

    #[test]
    fn desc_rounds() {
        let d = make_desc(6, K_PTR, K_SIZE, K_INT, K_INT, K_INT, K_SIZE);
        assert_eq!(nargs_of(d), 6);
        assert_eq!(kind_of(d, 5), Some(ArgKind::Size));
        assert_eq!(kind_of(d, 6), None);
        let zero = make_desc(0, K_NONE, K_NONE, K_NONE, K_NONE, K_NONE, K_NONE);
        assert_eq!(nargs_of(zero), 0);
        assert_eq!(kind_of(zero, 0), None);
    }

    #[test]
    fn syscall_table_is_consistent() {
        assert_eq!(SYSCALLS.len(), SYSCALL_COUNT);
        assert_eq!(nargs_matches_frozen(), true);
        assert_eq!(kinds_match_frozen(), true);
        assert_eq!(bad_args_are_ptr(), true);
        assert_eq!(total_ptr_args(), EXPECTED_PTR_ARGS);
    }

    #[test]
    fn probe_grid_all_match() {
        let (matched, outs) = run_probe_grid();
        assert_eq!(matched, PROBE_CELLS as u32);
        assert_eq!(outs.len(), PROBE_CELLS);
        assert_eq!(outs.iter().filter(|o| o.errno == EFAULT).count(), 5);
        assert_eq!(outs.iter().filter(|o| o.errno == 0).count(), 5);
    }

    #[test]
    fn page_probe_semantics() {
        let mut w = MemWindow::canonical();
        assert_eq!(w.probe(WINDOW_BASE, 64), ProbeVerdict::Accessible);
        assert_eq!(w.probe(0, 64), ProbeVerdict::Fault);
        assert_eq!(w.probe(0, 0), ProbeVerdict::Accessible);
        assert_eq!(w.probe(WINDOW_BASE + 7 * PAGE, 1), ProbeVerdict::Fault);
        assert_eq!(w.probe(WINDOW_BASE + 6 * PAGE, 2 * PAGE), ProbeVerdict::Fault);
        assert_eq!(w.probe(WINDOW_BASE + 6 * PAGE, PAGE), ProbeVerdict::Accessible);
        assert_eq!(w.probe_calls(), 6);
    }

    #[test]
    fn race_detected_both_variants() {
        let (ledger, va, vb) = run_race_injection();
        assert_eq!(ledger.injected, 20);
        assert_eq!(ledger.detected, 20);
        assert_eq!(ledger.escaped, 0);
        assert_eq!(va, INJECT_ROUNDS);
        assert_eq!(vb, INJECT_ROUNDS);
    }

    #[test]
    fn single_source_is_stable() {
        assert_eq!(single_source_agrees(), true);
        let mut w = MemWindow::canonical();
        let e = lookup(0);
        assert_eq!(e.is_some(), true);
        let args = [3u64, WINDOW_BASE, 16, 0, 0, 0];
        let first = match e {
            Some(entry) => probe_syscall(&mut w, entry, &args),
            None => ProbeOutcome::not_applicable(),
        };
        let second = match e {
            Some(entry) => probe_syscall(&mut w, entry, &args),
            None => ProbeOutcome::not_applicable(),
        };
        assert_eq!(first, second);
        assert_eq!(w.probe_calls(), 2);
    }

    #[test]
    fn order_stops_at_first_bad_arg() {
        assert_eq!(order_stops_at_first_bad(), true);
        let e = lookup(59);
        assert_eq!(e.is_some(), true);
        let mut w = MemWindow::canonical();
        let args = [WINDOW_BASE, 1u64, WINDOW_BASE, 0, 0, 0];
        let out = match e {
            Some(entry) => probe_syscall(&mut w, entry, &args),
            None => ProbeOutcome::not_applicable(),
        };
        assert_eq!(out.first_bad, Some(1));
        assert_eq!(out.probes, 2);
    }

    #[test]
    fn no_ptr_syscalls_are_not_applicable() {
        let mut w = MemWindow::canonical();
        let kill = lookup(62);
        let getpid = lookup(39);
        assert_eq!(kill.is_some(), true);
        assert_eq!(getpid.is_some(), true);
        let a = match kill {
            Some(e) => probe_syscall(&mut w, e, &[1, 9, 0, 0, 0, 0]),
            None => ProbeOutcome::not_applicable(),
        };
        let b = match getpid {
            Some(e) => probe_syscall(&mut w, e, &[0, 0, 0, 0, 0, 0]),
            None => ProbeOutcome::not_applicable(),
        };
        assert_eq!(a.applicable, false);
        assert_eq!(b.applicable, false);
        assert_eq!(a.errno, 0);
        assert_eq!(b.errno, 0);
        assert_eq!(a.probes, 0);
    }

    #[test]
    fn lookup_bounds() {
        assert_eq!(lookup(0).map(|e| e.name), Some("read"));
        assert_eq!(lookup(257).map(|e| e.name), Some("openat"));
        assert_eq!(lookup(9999).is_none(), true);
    }

    #[test]
    fn fingerprint_stable() {
        let a = corpus_fingerprint();
        let b = corpus_fingerprint();
        assert_eq!(a, b);
        assert_eq!(a != 0, true);
    }

    #[test]
    fn errno_values_match_linux() {
        assert_eq!(EFAULT, 14);
        assert_eq!(E2BIG, 7);
        assert_eq!(EBADF, 9);
        assert_eq!(EINVAL, 22);
        assert_eq!(ENOSYS, 38);
        assert_eq!(ENOTTY, 25);
        assert_eq!(ProbeVerdict::Fault.errno(), EFAULT);
        assert_eq!(ProbeVerdict::Accessible.errno(), 0);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9319_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
        assert_eq!(s.len() >= 12, true);
    }
}
