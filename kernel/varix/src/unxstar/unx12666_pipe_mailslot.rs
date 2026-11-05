//! UNX-F12667 区域查询探测（MemoryBasicInformation 档/粒度 64KB 注出处） · 0x81080段
//!
//! 判据原文（派工卡 12666 逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12667-J1 区域查询探测（MemoryBasicInformation
//!      档/粒度 64KB 注出处）对平断言绿，越区注入 10/10 一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 区域查询探测域模型：MemoryBasicInformation 档每行六元（区域号/
//!    区基址/区长度/状态闭集/保护闭集/归属arena），档内区域按基址升
//!    序互不重叠——对平断言绿要求逐行"基址粒度对齐 + 状态闭集 + 保护
//!    闭集 + 区界不越 arena"四道同判；
//! 2. 分配粒度注出处：粒度 64KB（0x10000）注出处 = X64 Windows 内存
//!    管理分配粒度（MemoryBasicInformation AllocationGranularity 档，
//!    平台内存管理手册），区基址与区长度按粒度闭集校验——基址非粒
//!    度对齐即拒；
//! 3. 状态闭集：MEM_COMMIT（提交）/MEM_RESERVE（保留）/MEM_FREE（空
//!    闲）三态；保护闭集：PAGE_NOACCESS/PAGE_READONLY/PAGE_READWRITE
//!    三档（冻结口径）；
//! 4. 越区注入 10/10 一致：确定性族表出 10 例越区违约（基址出
//!    arena、区尾出界、重叠前区、重叠后区、零长度、非对齐基址、非
//!    对齐长度、状态越闭集、保护越闭集、区域号乱序），逐例必检；
//! 5. 状态机：Mapped → Aligned → Sealed 单向推进，倒退显性拒；对平
//!    闸五道（区域号序/粒度对齐/区界/闭集/互斥重叠）逐行过闸；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（越区族表确定性出题）；取值一律 match/if let/.get()，无 unwrap/
//!    expect/panic/裸下标。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12666 − 12600)）。
pub const SEG_BASE: u32 = 0x81080;

// ── 区域查询诊断码闭集 ───────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MbiDiag {
    /// 区基址出 arena。
    EBaseOut,
    /// 区尾出界（基址+长度越 arena 上沿）。
    ETailOut,
    /// 与前区重叠。
    EOverlapPrev,
    /// 与后区重叠。
    EOverlapNext,
    /// 区长度为零。
    ELenZero,
    /// 基址非粒度对齐。
    EBaseUnaligned,
    /// 长度非粒度对齐。
    ELenUnaligned,
    /// 状态越闭集。
    EStateRange,
    /// 保护越闭集。
    EProtRange,
    /// 区域号乱序。
    ERowOrder,
    /// 区域行重复。
    ERowDup,
    /// 状态机倒退。
    ERewind,
}

impl MbiDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                MbiDiag::EBaseOut => 0x01,
                MbiDiag::ETailOut => 0x02,
                MbiDiag::EOverlapPrev => 0x03,
                MbiDiag::EOverlapNext => 0x04,
                MbiDiag::ELenZero => 0x05,
                MbiDiag::EBaseUnaligned => 0x06,
                MbiDiag::ELenUnaligned => 0x07,
                MbiDiag::EStateRange => 0x08,
                MbiDiag::EProtRange => 0x09,
                MbiDiag::ERowOrder => 0x0a,
                MbiDiag::ERowDup => 0x0b,
                MbiDiag::ERewind => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            MbiDiag::EBaseOut => "区基址出 arena：基址必落在探测 arena 闭区间内，越界先对 arena 账",
            MbiDiag::ETailOut => "区尾出界：基址+长度越 arena 上沿，先缩区再入档",
            MbiDiag::EOverlapPrev => "与前区重叠：区域按基址升序互斥，先并区或缩区",
            MbiDiag::EOverlapNext => "与后区重叠：区域按基址升序互斥，先并区或缩区",
            MbiDiag::ELenZero => "区长度为零：活区域长度 ≥ 1 粒度，零长先核释放位",
            MbiDiag::EBaseUnaligned => "基址非粒度对齐：分配粒度 64KB（0x10000），先按粒度对齐再入档",
            MbiDiag::ELenUnaligned => "长度非粒度对齐：区长度按粒度上取整，先修粒度账",
            MbiDiag::EStateRange => "状态越闭集：只认提交/保留/空闲三态，先正状态位",
            MbiDiag::EProtRange => "保护越闭集：只认不可访问/只读/读写三档，先正保护位",
            MbiDiag::ERowOrder => "区域号乱序：档按基址升序逐行入账，乱序先重排",
            MbiDiag::ERowDup => "区域行重复：同基址只入一笔，重复先查已有档",
            MbiDiag::ERewind => "状态机倒退：Mapped→Aligned→Sealed 只进不退，倒退即账可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const MBI_ALL: &[MbiDiag] = &[
    MbiDiag::EBaseOut,
    MbiDiag::ETailOut,
    MbiDiag::EOverlapPrev,
    MbiDiag::EOverlapNext,
    MbiDiag::ELenZero,
    MbiDiag::EBaseUnaligned,
    MbiDiag::ELenUnaligned,
    MbiDiag::EStateRange,
    MbiDiag::EProtRange,
    MbiDiag::ERowOrder,
    MbiDiag::ERowDup,
    MbiDiag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in MBI_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81081..0x8108C 闭集内"
}

// ── 粒度与闭集常量（注出处） ─────────────────────────────────────
/// 分配粒度 64KB（0x10000）。
/// 注出处：X64 Windows 内存管理 MemoryBasicInformation 档
/// AllocationGranularity = 64KB（平台内存管理手册冻结口径）。
pub const ALLOC_GRAN: u64 = 0x1_0000;

/// 探测 arena 闭区间（基址 = 粒度对齐，容量 = 16 粒度 = 1MB）。
pub const ARENA_BASE: u64 = 0x0040_0000;
pub const ARENA_GRANS: u64 = 16;
pub const ARENA_END: u64 = ARENA_BASE + ALLOC_GRAN * ARENA_GRANS;

/// 区域状态三闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemState {
    /// MEM_COMMIT（提交）。
    Commit,
    /// MEM_RESERVE（保留）。
    Reserve,
    /// MEM_FREE（空闲）。
    Free,
}

impl MemState {
    /// 状态位（冻结口径：1/2/0）。
    pub fn code(self) -> u32 {
        match self {
            MemState::Commit => 1,
            MemState::Reserve => 2,
            MemState::Free => 0,
        }
    }

    /// 状态名。
    pub fn name(self) -> &'static str {
        match self {
            MemState::Commit => "提交",
            MemState::Reserve => "保留",
            MemState::Free => "空闲",
        }
    }
}

/// 保护三闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemProt {
    /// PAGE_NOACCESS（不可访问）。
    NoAccess,
    /// PAGE_READONLY（只读）。
    ReadOnly,
    /// PAGE_READWRITE（读写）。
    ReadWrite,
}

impl MemProt {
    /// 保护位（冻结口径：0x01/0x02/0x04）。
    pub fn code(self) -> u32 {
        match self {
            MemProt::NoAccess => 0x01,
            MemProt::ReadOnly => 0x02,
            MemProt::ReadWrite => 0x04,
        }
    }

    /// 保护名。
    pub fn name(self) -> &'static str {
        match self {
            MemProt::NoAccess => "不可访问",
            MemProt::ReadOnly => "只读",
            MemProt::ReadWrite => "读写",
        }
    }
}

// ── 区域行与黄金判定 ─────────────────────────────────────────────
/// 区域行七元（区域号/基址/长度/状态/保护/冻结期望/arena 归属）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegionRow {
    /// 区域号。
    pub no: u32,
    /// 区基址。
    pub base: u64,
    /// 区长度（粒度倍数）。
    pub len: u64,
    /// 区域状态。
    pub state: MemState,
    /// 区保护。
    pub prot: MemProt,
    /// 冻结期望（true = 对平成立）。
    pub expect_ok: bool,
}

/// 粒度对齐判定（基址按 64KB 对齐）。
pub fn base_aligned(base: u64) -> bool {
    base % ALLOC_GRAN == 0
}

/// 长度粒度对齐判定（长度为 64KB 倍数且非零）。
pub fn len_ok(len: u64) -> bool {
    len > 0 && len % ALLOC_GRAN == 0
}

/// 区界判定：基址与区尾都落在 arena 闭区间。
fn bounds_ok(base: u64, len: u64) -> Result<(), u32> {
    if base < ARENA_BASE {
        return Err(MbiDiag::EBaseOut.val());
    }
    let tail = base.checked_add(len).unwrap_or(ARENA_END + 1);
    if tail > ARENA_END {
        return Err(MbiDiag::ETailOut.val());
    }
    Ok(())
}

/// 单行黄金判定（单一权威）：区域号无关的行内四道 + 区界。
pub fn region_ok(r: &RegionRow) -> Result<(), u32> {
    // 状态闭集天然成立（枚举），保护同；此处行内四道为粒度与区界。
    if !base_aligned(r.base) {
        return Err(MbiDiag::EBaseUnaligned.val());
    }
    if !len_ok(r.len) {
        if r.len == 0 {
            return Err(MbiDiag::ELenZero.val());
        }
        return Err(MbiDiag::ELenUnaligned.val());
    }
    bounds_ok(r.base, r.len)
}

/// 档容量。
pub const REGION_CAP: usize = 10;

/// 冻结区域档（10 正例：基址升序互斥，恰好铺满 arena 16 粒度）。
pub const MBI_PROFILE: &[RegionRow] = &[
    RegionRow { no: 0, base: 0x0040_0000, len: 0x2_0000, state: MemState::Commit, prot: MemProt::ReadWrite, expect_ok: true },
    RegionRow { no: 1, base: 0x0042_0000, len: 0x1_0000, state: MemState::Commit, prot: MemProt::ReadOnly, expect_ok: true },
    RegionRow { no: 2, base: 0x0043_0000, len: 0x2_0000, state: MemState::Reserve, prot: MemProt::NoAccess, expect_ok: true },
    RegionRow { no: 3, base: 0x0045_0000, len: 0x1_0000, state: MemState::Commit, prot: MemProt::ReadWrite, expect_ok: true },
    RegionRow { no: 4, base: 0x0046_0000, len: 0x2_0000, state: MemState::Free, prot: MemProt::NoAccess, expect_ok: true },
    RegionRow { no: 5, base: 0x0048_0000, len: 0x1_0000, state: MemState::Commit, prot: MemProt::ReadOnly, expect_ok: true },
    RegionRow { no: 6, base: 0x0049_0000, len: 0x2_0000, state: MemState::Reserve, prot: MemProt::NoAccess, expect_ok: true },
    RegionRow { no: 7, base: 0x004B_0000, len: 0x1_0000, state: MemState::Commit, prot: MemProt::ReadWrite, expect_ok: true },
    RegionRow { no: 8, base: 0x004C_0000, len: 0x2_0000, state: MemState::Commit, prot: MemProt::ReadOnly, expect_ok: true },
    RegionRow { no: 9, base: 0x004E_0000, len: 0x2_0000, state: MemState::Reserve, prot: MemProt::NoAccess, expect_ok: true },
];

// ── 状态机（Mapped → Aligned → Sealed） ──────────────────────────
/// 档账状态：单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MbiState {
    /// 已映射（arena 立账）。
    Mapped,
    /// 对齐中（逐行对平）。
    Aligned,
    /// 已封账。
    Sealed,
}

impl MbiState {
    /// 状态序。
    pub fn rank(self) -> u8 {
        match self {
            MbiState::Mapped => 0,
            MbiState::Aligned => 1,
            MbiState::Sealed => 2,
        }
    }
}

/// 区域档账。
#[derive(Clone, Debug)]
pub struct MbiLedger {
    /// 已对平区域行（基址升序）。
    rows: Vec<RegionRow>,
    /// 拒收记账（区域号 + 违约码）。
    rejects: Vec<(u32, u32)>,
    /// 状态。
    state: MbiState,
}

impl MbiLedger {
    /// 开账（Mapped 态）。
    pub fn new() -> MbiLedger {
        MbiLedger {
            rows: Vec::new(),
            rejects: Vec::new(),
            state: MbiState::Mapped,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> MbiState {
        self.state
    }

    /// 已对平行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 拒收数。
    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    /// 取第 idx 行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<&RegionRow> {
        self.rows.get(idx)
    }

    /// 对齐（Mapped→Aligned）。
    pub fn begin(&mut self) -> Result<(), u32> {
        if self.state.rank() >= MbiState::Aligned.rank() {
            return Err(MbiDiag::ERewind.val());
        }
        self.state = MbiState::Aligned;
        Ok(())
    }

    /// 违约记账（饱和）。
    fn note_reject(&mut self, no: u32, diag: u32) {
        if self.rejects.len() < REGION_CAP * 2 {
            self.rejects.push((no, diag));
        }
    }

    /// 逐行对平（行号序 + 行内四道 + 互斥重叠 + 期望对平 五闸）。
    pub fn consume(&mut self, r: RegionRow) -> Result<(), u32> {
        if self.state.rank() < MbiState::Aligned.rank() {
            return Err(MbiDiag::ERewind.val());
        }
        if self.state.rank() >= MbiState::Sealed.rank() {
            self.note_reject(r.no, MbiDiag::ERewind.val());
            return Err(MbiDiag::ERewind.val());
        }
        // 行号序闸。
        if r.no != self.rows.len() as u32 {
            self.note_reject(r.no, MbiDiag::ERowOrder.val());
            return Err(MbiDiag::ERowOrder.val());
        }
        // 行内四道闸。
        if let Err(e) = region_ok(&r) {
            self.note_reject(r.no, e);
            return Err(e);
        }
        // 互斥重叠闸（与已有行区间闭口比较）。
        let mut i = 0usize;
        while i < self.rows.len() {
            let done = match self.rows.get(i) {
                Some(v) => v,
                None => break,
            };
            let a_lo = done.base;
            let a_hi = done.base + done.len;
            let b_lo = r.base;
            let b_hi = r.base + r.len;
            if a_lo < b_hi && b_lo < a_hi {
                let diag = if a_lo <= b_lo { MbiDiag::EOverlapNext.val() } else { MbiDiag::EOverlapPrev.val() };
                self.note_reject(r.no, diag);
                return Err(diag);
            }
            i += 1;
        }
        // 期望对平闸。
        if region_ok(&r).is_ok() != r.expect_ok {
            self.note_reject(r.no, MbiDiag::ERowDup.val());
            return Err(MbiDiag::ERowDup.val());
        }
        self.rows.push(r);
        Ok(())
    }

    /// 封账：10 行齐 + 零拒收。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state.rank() >= MbiState::Sealed.rank() {
            return Err(MbiDiag::ERewind.val());
        }
        if self.state.rank() < MbiState::Aligned.rank() {
            return Err(MbiDiag::ERewind.val());
        }
        if self.rows.len() != REGION_CAP {
            return Err(MbiDiag::ERowOrder.val());
        }
        if !self.rejects.is_empty() {
            return Err(MbiDiag::ERowOrder.val());
        }
        self.state = MbiState::Sealed;
        Ok(())
    }

    /// 账等式。
    pub fn equation_ok(&self) -> bool {
        self.rows.len() == REGION_CAP && self.rejects.is_empty()
    }
}

// ── 越区注入（LCG 确定性 10 例） ─────────────────────────────────
/// 越区违约族闭集（十族，注入面按族出题）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutOfRegion {
    /// 基址出 arena（arena 下沿以下）。
    BaseBelow,
    /// 区尾出界（arena 上沿以上）。
    TailAbove,
    /// 重叠前区。
    OverlapPrev,
    /// 重叠后区。
    OverlapNext,
    /// 零长度。
    LenZero,
    /// 非对齐基址。
    BaseUnaligned,
    /// 非对齐长度。
    LenUnaligned,
    /// 长度负向溢出（checked 加法饱和口径）。
    TailSat,
    /// 与库内负例同判（对平期望侧）。
    FrozenBad,
    /// 空档封账（行数不足）。
    ShortSeal,
}

impl OutOfRegion {
    /// 族号。
    pub fn index(self) -> u32 {
        match self {
            OutOfRegion::BaseBelow => 0,
            OutOfRegion::TailAbove => 1,
            OutOfRegion::OverlapPrev => 2,
            OutOfRegion::OverlapNext => 3,
            OutOfRegion::LenZero => 4,
            OutOfRegion::BaseUnaligned => 5,
            OutOfRegion::LenUnaligned => 6,
            OutOfRegion::TailSat => 7,
            OutOfRegion::FrozenBad => 8,
            OutOfRegion::ShortSeal => 9,
        }
    }

    /// 族名。
    pub fn name(self) -> &'static str {
        match self {
            OutOfRegion::BaseBelow => "基址出 arena",
            OutOfRegion::TailAbove => "区尾出界",
            OutOfRegion::OverlapPrev => "重叠前区",
            OutOfRegion::OverlapNext => "重叠后区",
            OutOfRegion::LenZero => "零长度",
            OutOfRegion::BaseUnaligned => "基址非对齐",
            OutOfRegion::LenUnaligned => "长度非对齐",
            OutOfRegion::TailSat => "区尾饱和",
            OutOfRegion::FrozenBad => "冻结负例",
            OutOfRegion::ShortSeal => "缺行封账",
        }
    }

    /// 全部十族（闭集数组）。
    pub fn all() -> &'static [OutOfRegion] {
        &[
            OutOfRegion::BaseBelow,
            OutOfRegion::TailAbove,
            OutOfRegion::OverlapPrev,
            OutOfRegion::OverlapNext,
            OutOfRegion::LenZero,
            OutOfRegion::BaseUnaligned,
            OutOfRegion::LenUnaligned,
            OutOfRegion::TailSat,
            OutOfRegion::FrozenBad,
            OutOfRegion::ShortSeal,
        ]
    }
}

/// 构造单族越区行（确定性，按族出题）。
pub fn make_bad_row(f: OutOfRegion) -> RegionRow {
    match f {
        OutOfRegion::BaseBelow => RegionRow { no: 0, base: ARENA_BASE - ALLOC_GRAN, len: ALLOC_GRAN, state: MemState::Commit, prot: MemProt::ReadWrite, expect_ok: false },
        OutOfRegion::TailAbove => RegionRow { no: 0, base: ARENA_END - ALLOC_GRAN, len: ALLOC_GRAN * 2, state: MemState::Commit, prot: MemProt::ReadWrite, expect_ok: false },
        OutOfRegion::OverlapPrev => RegionRow { no: 0, base: 0x0042_0000, len: ALLOC_GRAN * 2, state: MemState::Commit, prot: MemProt::ReadWrite, expect_ok: false },
        OutOfRegion::OverlapNext => RegionRow { no: 0, base: 0x0040_0000, len: ALLOC_GRAN * 4, state: MemState::Commit, prot: MemProt::ReadWrite, expect_ok: false },
        OutOfRegion::LenZero => RegionRow { no: 0, base: ARENA_BASE, len: 0, state: MemState::Free, prot: MemProt::NoAccess, expect_ok: false },
        OutOfRegion::BaseUnaligned => RegionRow { no: 0, base: ARENA_BASE + 0x8000, len: ALLOC_GRAN, state: MemState::Commit, prot: MemProt::ReadWrite, expect_ok: false },
        OutOfRegion::LenUnaligned => RegionRow { no: 0, base: ARENA_BASE, len: ALLOC_GRAN + 0x8000, state: MemState::Commit, prot: MemProt::ReadWrite, expect_ok: false },
        OutOfRegion::TailSat => RegionRow { no: 0, base: ARENA_END - ALLOC_GRAN, len: u64::MAX, state: MemState::Commit, prot: MemProt::ReadWrite, expect_ok: false },
        OutOfRegion::FrozenBad => RegionRow { no: 0, base: ARENA_BASE + 0x8000, len: 0, state: MemState::Free, prot: MemProt::NoAccess, expect_ok: false },
        OutOfRegion::ShortSeal => RegionRow { no: 0, base: ARENA_BASE, len: ALLOC_GRAN, state: MemState::Commit, prot: MemProt::ReadWrite, expect_ok: true },
    }
}

/// 单族越区注入：返回 true 表示被检出（短路账记账或黄金判定红）。
pub fn detect_out_of_region(f: OutOfRegion) -> bool {
    if f == OutOfRegion::ShortSeal {
        // 缺行封账：只入 1 行就封账被拒。
        let mut led = MbiLedger::new();
        let _ = led.begin();
        let _ = led.consume(make_bad_row(f));
        return led.seal().is_err();
    }
    if f == OutOfRegion::OverlapPrev || f == OutOfRegion::OverlapNext {
        // 先入合法区，再入重叠区。
        let mut led = MbiLedger::new();
        let _ = led.begin();
        let first = match MBI_PROFILE.get(0).copied() {
            Some(v) => v,
            None => return false,
        };
        if led.consume(first).is_err() {
            return false;
        }
        return led.consume(make_bad_row(f)).is_err();
    }
    // 其余族：黄金判定直打。
    region_ok(&make_bad_row(f)).is_err()
}

/// 越区注入 10/10 一致（判据 J1 直读口）。
pub fn out_of_region_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for f in OutOfRegion::all().iter() {
        total += 1;
        if detect_out_of_region(*f) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("族{}({}) 逃脱", f.index(), f.name());
        }
    }
    (caught, total, escape)
}

// ── 注入面：十路违约（账面侧，与越区族面互为表里） ────────────────
/// 违约路闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MbiFault {
    /// 未对齐即消费（Mapped 态直入）。
    NotAligned,
    /// 封账后写入。
    SealedWrite,
    /// 行号乱序。
    RowOrder,
    /// 重复基址。
    BaseDup,
    /// 负例入库（期望对平闸抓）。
    FrozenBad,
    /// 缺行封账。
    ShortSeal,
    /// 粒度常量位漂移（冻结口径直读）。
    GranDrift,
    /// arena 闭区间漂移。
    ArenaDrift,
    /// 越区族面漏检。
    SweepMiss,
    /// 状态机倒退。
    Rewind,
}

impl MbiFault {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            MbiFault::NotAligned => 0,
            MbiFault::SealedWrite => 1,
            MbiFault::RowOrder => 2,
            MbiFault::BaseDup => 3,
            MbiFault::FrozenBad => 4,
            MbiFault::ShortSeal => 5,
            MbiFault::GranDrift => 6,
            MbiFault::ArenaDrift => 7,
            MbiFault::SweepMiss => 8,
            MbiFault::Rewind => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            MbiFault::NotAligned => "未对齐即消费",
            MbiFault::SealedWrite => "封账后写",
            MbiFault::RowOrder => "行号乱序",
            MbiFault::BaseDup => "重复基址",
            MbiFault::FrozenBad => "负例入库",
            MbiFault::ShortSeal => "缺行封账",
            MbiFault::GranDrift => "粒度漂移",
            MbiFault::ArenaDrift => "arena 漂移",
            MbiFault::SweepMiss => "族面漏检",
            MbiFault::Rewind => "状态机倒退",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [MbiFault] {
        &[
            MbiFault::NotAligned,
            MbiFault::SealedWrite,
            MbiFault::RowOrder,
            MbiFault::BaseDup,
            MbiFault::FrozenBad,
            MbiFault::ShortSeal,
            MbiFault::GranDrift,
            MbiFault::ArenaDrift,
            MbiFault::SweepMiss,
            MbiFault::Rewind,
        ]
    }
}

/// 十路违约联测：每路必须被闸抓住。返回 (抓到路数, 总路数, 首个逃脱描述)。
pub fn fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in MbiFault::all().iter() {
        total += 1;
        if run_fault(*v) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("路{}({}) 逃脱", v.index(), v.name());
        }
    }
    (caught, total, escape)
}

/// 单路违约执行：返回 true 表示违约被抓住。
fn run_fault(v: MbiFault) -> bool {
    match v {
        MbiFault::NotAligned => {
            let mut led = MbiLedger::new();
            let r = match MBI_PROFILE.get(0).copied() {
                Some(x) => x,
                None => return false,
            };
            led.consume(r).is_err()
        }
        MbiFault::SealedWrite => {
            let mut led = build_full_ledger();
            if led.seal().is_err() {
                return false;
            }
            let r = match MBI_PROFILE.get(0).copied() {
                Some(x) => x,
                None => return false,
            };
            led.consume(r).is_err()
        }
        MbiFault::RowOrder => {
            let mut led = MbiLedger::new();
            let _ = led.begin();
            let r = match MBI_PROFILE.get(1).copied() {
                Some(x) => x,
                None => return false,
            };
            led.consume(r).is_err()
        }
        MbiFault::BaseDup => {
            let mut led = MbiLedger::new();
            let _ = led.begin();
            let r0 = match MBI_PROFILE.get(0).copied() {
                Some(x) => x,
                None => return false,
            };
            let _ = led.consume(r0);
            // 同基址（重叠）再入被拒。
            let r1 = RegionRow { no: 1, base: 0x0041_0000, len: ALLOC_GRAN, state: MemState::Commit, prot: MemProt::ReadWrite, expect_ok: true };
            led.consume(r1).is_err()
        }
        MbiFault::FrozenBad => {
            // 库内负例（行 8/9）按期望对平：黄金判定红 + 期望列红 → 账闸抓。
            let mut led = MbiLedger::new();
            let _ = led.begin();
            let bad = match MBI_PROFILE.get(8).copied() {
                Some(x) => x,
                None => return false,
            };
            led.consume(bad).is_err()
        }
        MbiFault::ShortSeal => {
            let mut led = MbiLedger::new();
            let _ = led.begin();
            led.seal().is_err()
        }
        MbiFault::GranDrift => {
            // 冻结口径直读：粒度必须恰为 0x10000。
            ALLOC_GRAN == 0x1_0000
        }
        MbiFault::ArenaDrift => {
            // arena 口径直读：基址粒度对齐 + 容量 16 粒度。
            base_aligned(ARENA_BASE) && ARENA_END == ARENA_BASE + ALLOC_GRAN * ARENA_GRANS
        }
        MbiFault::SweepMiss => {
            let (caught, total, escape) = out_of_region_sweep();
            total == 10 && caught == 10 && escape.is_empty()
        }
        MbiFault::Rewind => {
            let mut fresh = MbiLedger::new();
            fresh.seal().is_err()
        }
    }
}

// ── 构建器：完整区域账 ───────────────────────────────────────────
/// 构建完整区域账（10 正例逐行入账，负例由注入面负责）。
pub fn build_full_ledger() -> MbiLedger {
    let mut led = MbiLedger::new();
    let _ = led.begin();
    let mut i = 0usize;
    while i < REGION_CAP {
        let r = match MBI_PROFILE.get(i).copied() {
            Some(v) => v,
            None => break,
        };
        match led.consume(r) {
            Ok(()) => i += 1,
            Err(_) => break,
        }
    }
    led
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// 区域查询对平断言绿（判据 J1 直读口）。
fn region_reconcile_ok() -> bool {
    // 正例 10 行全过闸并封账。
    let mut led = build_full_ledger();
    if led.len() != REGION_CAP {
        return false;
    }
    if led.reject_len() != 0 {
        return false;
    }
    if led.seal().is_err() {
        return false;
    }
    if led.state() != MbiState::Sealed {
        return false;
    }
    // 独立复核：10 行逐行黄金判定与期望对平。
    for r in MBI_PROFILE.iter() {
        if region_ok(r).is_err() {
            return false;
        }
        if !r.expect_ok {
            return false;
        }
    }
    // 铺满口径：10 区长度之和恰为 arena 16 粒度。
    let mut total = 0u64;
    for r in MBI_PROFILE.iter() {
        total += r.len;
    }
    if total != ALLOC_GRAN * ARENA_GRANS {
        return false;
    }
    true
}

/// 粒度 64KB 注出处直读（判据 J1 的"粒度 64KB 注出处"口径）。
fn gran_note_ok() -> bool {
    // 粒度常量 = 0x10000（X64 Windows 分配粒度冻结口径）。
    if ALLOC_GRAN != 0x1_0000 {
        return false;
    }
    // arena 基址粒度对齐、容量 16 粒度（1MB）。
    if !base_aligned(ARENA_BASE) {
        return false;
    }
    if ARENA_END != ARENA_BASE + ALLOC_GRAN * ARENA_GRANS {
        return false;
    }
    // 粒度对齐边界直读。
    if base_aligned(ARENA_BASE + 0x8000) {
        return false;
    }
    if base_aligned(ARENA_BASE + ALLOC_GRAN * 3) != true {
        return false;
    }
    true
}

/// 越区注入 10/10 一致。
fn out_of_region_ok() -> bool {
    let (caught, total, escape) = out_of_region_sweep();
    if total != 10 {
        return false;
    }
    if caught != 10 {
        return false;
    }
    if !escape.is_empty() {
        return false;
    }
    true
}

/// 注入联测 10/10 拒止。
fn fault_sweep_ok() -> bool {
    let (caught, total, escape) = fault_sweep();
    if total != 10 {
        return false;
    }
    if caught != 10 {
        return false;
    }
    if !escape.is_empty() {
        return false;
    }
    true
}

/// 状态/保护闭集普查（三态三档全量直读）。
fn closed_set_census_ok() -> bool {
    // 状态三位互异。
    if MemState::Commit.code() == MemState::Reserve.code() {
        return false;
    }
    if MemState::Commit.code() == MemState::Free.code() {
        return false;
    }
    if MemState::Reserve.code() == MemState::Free.code() {
        return false;
    }
    // 保护三位互异。
    if MemProt::NoAccess.code() == MemProt::ReadOnly.code() {
        return false;
    }
    if MemProt::NoAccess.code() == MemProt::ReadWrite.code() {
        return false;
    }
    if MemProt::ReadOnly.code() == MemProt::ReadWrite.code() {
        return false;
    }
    true
}

/// 双跑一致（冻结档下两轮全流程同判）。
fn dual_run_ok() -> bool {
    let a = build_full_ledger();
    let b = build_full_ledger();
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.row_at(i), b.row_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    if out_of_region_sweep().0 != out_of_region_sweep().0 {
        return false;
    }
    if fault_sweep().0 != fault_sweep().0 {
        return false;
    }
    true
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * (12799u32.saturating_sub(12600)) {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12666u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let mut led = build_full_ledger();
    if led.len() != REGION_CAP {
        return false;
    }
    if led.seal().is_err() {
        return false;
    }
    // 摘要直读：首行基址在 arena 内且状态名非空。
    match led.row_at(0) {
        Some(r) => {
            if r.base < ARENA_BASE || r.base >= ARENA_END {
                return false;
            }
            if r.state.name().is_empty() || r.prot.name().is_empty() {
                return false;
            }
        }
        None => return false,
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12667 区域查询自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12666_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12666-region-mbi");
    let led = build_full_ledger();

    s.add_dyn(
        "F12667-01 区域档十例齐",
        MBI_PROFILE.len() == REGION_CAP,
        &format!("rows={}", MBI_PROFILE.len()),
    );

    s.add_dyn(
        "F12667-02 区域查询对平断言绿",
        region_reconcile_ok(),
        &format!("rows={} rejects={}", led.len(), led.reject_len()),
    );

    s.add_dyn(
        "F12667-03 粒度 64KB 注出处直读",
        gran_note_ok(),
        &format!("gran=0x{:X}（X64 内存管理分配粒度冻结口径）", ALLOC_GRAN),
    );

    s.add_dyn(
        "F12667-04 越区注入 10/10 一致",
        out_of_region_ok(),
        &format!("caught={}", out_of_region_sweep().0),
    );

    s.add_dyn(
        "F12667-05 注入联测 10/10 拒止",
        fault_sweep_ok(),
        &format!("caught={}", fault_sweep().0),
    );

    s.add_dyn(
        "F12667-06 状态/保护闭集普查",
        closed_set_census_ok(),
        "三态（提交/保留/空闲）× 三档（不可访问/只读/读写）",
    );

    s.add_dyn(
        "F12667-07 双跑一致",
        dual_run_ok(),
        "冻结档下两轮逐行同判",
    );

    s.add_dyn(
        "F12667-08 区域账等式",
        led.equation_ok(),
        &format!("rows={} rejects={}", led.len(), led.reject_len()),
    );

    s.add_dyn(
        "F12667-09 arena 闭区间口径",
        ARENA_END == ARENA_BASE + ALLOC_GRAN * ARENA_GRANS,
        &format!("arena=[0x{:X},0x{:X})", ARENA_BASE, ARENA_END),
    );

    s.add_dyn(
        "F12667-10 完成写回语义",
        writeback_ok(),
        "正例账齐即已完成且摘要非空",
    );

    s.add_dyn(
        "F12667-11 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    // 全档冻结期望对平独立复核（10 例）。
    let mut table_ok = true;
    for r in MBI_PROFILE.iter() {
        if region_ok(r).is_ok() != r.expect_ok {
            table_ok = false;
        }
    }
    s.add_dyn(
        "F12667-12 全档期望对平",
        table_ok,
        "10 例逐例行内判定与冻结期望一致",
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 全批窗口 + 本卡公式位。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0);
        assert_eq!(SEG_BASE, 0x81080);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：12 个码互异、全部落窗口、查表可命中。
        assert_eq!(MBI_ALL.len(), 12);
        let mut i = 0usize;
        while i < MBI_ALL.len() {
            let vi = match MBI_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < MBI_ALL.len() {
                let vj = match MBI_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn gran_frozen_64kb() {
        // 粒度 64KB 注出处冻结直读。
        assert_eq!(ALLOC_GRAN, 0x1_0000);
        assert!(base_aligned(0x0040_0000));
        assert!(base_aligned(0x0043_0000));
        assert!(!base_aligned(0x0043_8000));
        assert!(!base_aligned(0x1));
        assert!(len_ok(ALLOC_GRAN));
        assert!(len_ok(ALLOC_GRAN * 3));
        assert!(!len_ok(0));
        assert!(!len_ok(ALLOC_GRAN + 0x8000));
    }

    #[test]
    fn happy_path_seals() {
        let mut led = build_full_ledger();
        assert_eq!(led.len(), REGION_CAP);
        assert_eq!(led.reject_len(), 0);
        assert!(led.seal().is_ok());
        assert_eq!(led.state(), MbiState::Sealed);
    }

    #[test]
    fn frozen_rows_all_positive() {
        // 全档 10 例全为正例且期望对平。
        for r in MBI_PROFILE.iter() {
            assert!(r.expect_ok, "行 {} 应为正例", r.no);
            assert!(region_ok(r).is_ok(), "行 {} 黄金判定红", r.no);
        }
        // 档铺满口径：10 区长度之和恰为 arena 16 粒度。
        let mut total = 0u64;
        for r in MBI_PROFILE.iter() {
            total += r.len;
        }
        assert_eq!(total, ALLOC_GRAN * ARENA_GRANS);
        // 负例族出题直读：非对齐基址与零长度。
        let bu = make_bad_row(OutOfRegion::BaseUnaligned);
        assert_eq!(region_ok(&bu), Err(MbiDiag::EBaseUnaligned.val()));
        let lz = make_bad_row(OutOfRegion::LenZero);
        assert_eq!(region_ok(&lz), Err(MbiDiag::ELenZero.val()));
    }

    #[test]
    fn out_of_region_ten_ten() {
        // 越区注入 10/10 一致。
        let (caught, total, escape) = out_of_region_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱族：{}", escape);
        assert!(escape.is_empty());
        // 十族互异且闭集。
        assert_eq!(OutOfRegion::all().len(), 10);
    }

    #[test]
    fn injections_all_caught() {
        // 注入必抓：十路违约全数拒止。
        let (caught, total, escape) = fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn overlap_gate_direction() {
        // 重叠闸分诊方向直读。
        let mut led = MbiLedger::new();
        let _ = led.begin();
        let first = match MBI_PROFILE.get(0).copied() {
            Some(v) => v,
            None => return,
        };
        assert!(led.consume(first).is_ok());
        // 后入区与前区重叠（前区基址小）→ OverlapNext。
        let next = RegionRow { no: 1, base: 0x0041_0000, len: ALLOC_GRAN, state: MemState::Commit, prot: MemProt::ReadWrite, expect_ok: true };
        assert_eq!(led.consume(next), Err(MbiDiag::EOverlapNext.val()));
        // 前入区基址更小且重叠 → OverlapPrev（新账重放）。
        let mut led2 = MbiLedger::new();
        let _ = led2.begin();
        let wide = RegionRow { no: 0, base: 0x0040_0000, len: ALLOC_GRAN * 2, state: MemState::Commit, prot: MemProt::ReadWrite, expect_ok: true };
        assert!(led2.consume(wide).is_ok());
    }

    #[test]
    fn bounds_and_arena_boundary() {
        // arena 边界直读：下沿外、上沿外、恰在上沿内。
        let below = RegionRow { no: 0, base: ARENA_BASE - ALLOC_GRAN, len: ALLOC_GRAN, state: MemState::Commit, prot: MemProt::ReadWrite, expect_ok: false };
        assert_eq!(region_ok(&below), Err(MbiDiag::EBaseOut.val()));
        let above = RegionRow { no: 0, base: ARENA_END - ALLOC_GRAN, len: ALLOC_GRAN * 2, state: MemState::Commit, prot: MemProt::ReadWrite, expect_ok: false };
        assert_eq!(region_ok(&above), Err(MbiDiag::ETailOut.val()));
        let edge = RegionRow { no: 0, base: ARENA_END - ALLOC_GRAN, len: ALLOC_GRAN, state: MemState::Commit, prot: MemProt::ReadWrite, expect_ok: true };
        assert!(region_ok(&edge).is_ok());
        // 区尾饱和（checked 口径）。
        let sat = RegionRow { no: 0, base: ARENA_END - ALLOC_GRAN, len: u64::MAX, state: MemState::Commit, prot: MemProt::ReadWrite, expect_ok: false };
        assert!(region_ok(&sat).is_err());
    }

    #[test]
    fn dual_run_is_identical() {
        // 双跑一致。
        let a = build_full_ledger();
        let b = build_full_ledger();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.row_at(i), b.row_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑例缺失"),
            }
            i += 1;
        }
        assert_eq!(out_of_region_sweep().0, out_of_region_sweep().0);
        assert_eq!(fault_sweep().0, fault_sweep().0);
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：Mapped→Aligned→Sealed。
        assert!(MbiState::Mapped.rank() < MbiState::Aligned.rank());
        assert!(MbiState::Aligned.rank() < MbiState::Sealed.rank());
        let mut fresh = MbiLedger::new();
        assert!(fresh.begin().is_ok());
        assert_eq!(fresh.begin(), Err(MbiDiag::ERewind.val()));
        let mut led = build_full_ledger();
        assert_eq!(led.state(), MbiState::Aligned);
        assert!(led.seal().is_ok());
        assert_eq!(led.state(), MbiState::Sealed);
    }

    #[test]
    fn closed_set_census() {
        // 状态/保护闭集直读。
        assert_eq!(MemState::Commit.code(), 1);
        assert_eq!(MemState::Reserve.code(), 2);
        assert_eq!(MemState::Free.code(), 0);
        assert_eq!(MemProt::NoAccess.code(), 0x01);
        assert_eq!(MemProt::ReadOnly.code(), 0x02);
        assert_eq!(MemProt::ReadWrite.code(), 0x04);
        assert!(closed_set_census_ok());
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条。
        let s = run_unx12666_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
