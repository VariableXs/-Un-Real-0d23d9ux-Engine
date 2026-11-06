//! UNX-F9352 fd 等价类总装（负值/未开/已关/越限） · 0x7C580段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9352 逐字）：
//!   ① 纯功能代码 ≥360 行；② 判据原文：UNX-F9352-J1 四 fd 等价类总装判据过，
//!   逐类期望 EBADF 逐格检出，等价类账落账格过；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 「等价类」是四个可判决的谓词，不是四个标签：`class_of(fd)` 对任一整数
//!      fd 返回四类之一或 None（None = 合法在册 fd）。判定与 `validate` 共用
//!      同一张槽表，不存在「分类说负值、校验说越限」的双写漂移。
//!   ② 四类探针是冻结常量：负值 {-1,-2,-9,-1024,i32::MIN}、未开 {3,4,7,16,31}、
//!      已关 {3..7 先开后关}、越限 {32,33,63,1024,i32::MAX}。每类 5 × 10 轮 =
//!      50 格，四类 200 格；每格必须同时满足「分类命中」「validate 回 EBADF」
//!      「槽位账未动」，三者缺一即逃逸。
//!   ③ 边界刻意落在 31/32：31 是最后一个在册槽（未开类），32 是 RLIMIT_NOFILE
//!      上界（越限类）。相邻两号不同类，把 `>=` 写成 `>` 的实现当场翻车。
//!   ④ 状态机 Free → Live(gen) → Closed(gen+1) → Free(gen+2)：开槽与关槽都推进
//!      代际，故「关掉再开回同号」的代际必大于旧代际。`validate_gen` 让陈旧
//!      句柄回 EBADF，裸 fd 仍可用（与 Linux fd 复用语义一致；代际是内部分账）。
//!   ⑤ 账本逐类五列（injected/detected/escaped/ebadf/other），合计必须
//!      200 = 200 + 0 且 other 恒 0（任何一格回出 EBADF 之外的码立刻判红）。
//!   ⑥ 判定次序用「40 个坏 fd 连打后槽位指纹不变」证明：范围闸先于槽位闸，
//!      否则坏 fd 会被当槽下标解引用，槽位必被读脏。
//!   ⑦ 容量面单列：动态槽 29 个（fd 3..=31），满表回 EMFILE(24)，恒等式
//!      opens == closes + live 仍须成立；关一槽立即复开验证可回收。
//!   ⑧ 诊断码段内闭集 12 个（+1..=+12）：`code()` 回序号（u16），`addr()` 回
//!      SEG_BASE+序号（u32）——0x7C580 装不进 u16，两个口径必须分开。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9352 独占 0x7C580..=0x7C5BF 的 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7C580;

/// 坏文件描述符。
pub const EBADF: i32 = 9;
/// 参数值非法。
pub const EINVAL: i32 = 22;
/// 每进程打开文件数超限。
pub const EMFILE: i32 = 24;
/// 常驻标准流数量（0/1/2 恒在册，除非显式关闭）。
pub const STD_FDS: usize = 3;
/// 动态槽数量：fd 3..=31 共 29 个。
pub const DYN_SLOTS: usize = 29;
/// fd 号上界（fd >= 32 即越限，与 RLIMIT_NOFILE 对齐）。
pub const MAX_FDS: i32 = 32;
/// 每类探针个数。
pub const PROBES_PER_CLASS: usize = 5;
/// 单格复测轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 等价类个数（闭集）。
pub const CLASS_COUNT: usize = 4;
/// 写回状态：已完成 / 未完成。
pub const WB_DONE: u8 = 2;
pub const WB_TODO: u8 = 1;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +1..=+12，码值即段内序号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum FdCode {
    SegmentOutOfWindow = 1,
    ClassCountMismatch = 2,
    NegativeLeaked = 3,
    UnopenedLeaked = 4,
    ClosedLeaked = 5,
    RangeLeaked = 6,
    LedgerNotConserved = 7,
    EscapedNonZero = 8,
    GateOrderInverted = 9,
    GenerationStuck = 10,
    CapacityBroken = 11,
    NotReproducible = 12,
}

impl FdCode {
    /// 段内序号（段基址装不进 u16，与 `addr()` 分开）。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 段内绝对地址。
    pub fn addr(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            FdCode::SegmentOutOfWindow => "段位越界：须落 0x7C580..=0x7C5BF 且 0x40 对齐，回查分工表",
            FdCode::ClassCountMismatch => "等价类个数漂移：四类是冻结闭集，增删类须改判据",
            FdCode::NegativeLeaked => "负值 fd 逃逸：fd<0 须在范围闸第一步回 EBADF，禁入槽位下标",
            FdCode::UnopenedLeaked => "未开 fd 逃逸：Free 槽须回 EBADF，禁止把空槽当合法句柄",
            FdCode::ClosedLeaked => "已关 fd 逃逸：Closed 槽须回 EBADF，关闭不是幂等操作",
            FdCode::RangeLeaked => "越限 fd 逃逸：fd>=32 须在范围闸回 EBADF，边界 31/32 不可互换",
            FdCode::LedgerNotConserved => "账不守恒：injected 须等于 detected+escaped，逐类核对",
            FdCode::EscapedNonZero => "存在逃逸格：某探针未按期望分类或未回 EBADF，按类定位探针",
            FdCode::GateOrderInverted => "判序颠倒：范围闸须先于槽位闸，否则坏 fd 会被当槽下标",
            FdCode::GenerationStuck => "代际未推进：开关槽都须推进代际，否则陈旧句柄仍可命中",
            FdCode::CapacityBroken => "容量面破损：29 槽满表须回 EMFILE 且关一槽可立即复开",
            FdCode::NotReproducible => "双跑不一致：同输入两次指纹不同，排查隐藏状态",
        }
    }

    /// 闭集全量（文案与唯一性检查使用）。
    pub const ALL: &'static [FdCode] = &[
        FdCode::SegmentOutOfWindow,
        FdCode::ClassCountMismatch,
        FdCode::NegativeLeaked,
        FdCode::UnopenedLeaked,
        FdCode::ClosedLeaked,
        FdCode::RangeLeaked,
        FdCode::LedgerNotConserved,
        FdCode::EscapedNonZero,
        FdCode::GateOrderInverted,
        FdCode::GenerationStuck,
        FdCode::CapacityBroken,
        FdCode::NotReproducible,
    ];
}

// ── fd 等价类 ─────────────────────────────────────────────────────────
/// 四类 fd 错误等价类（闭集，顺序即判据与账格顺序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum FdClass {
    /// fd < 0。
    Negative = 0,
    /// 槽位存在但从未打开。
    Unopened = 1,
    /// 槽位曾打开、已被关闭。
    Closed = 2,
    /// fd >= RLIMIT_NOFILE。
    OutOfRange = 3,
}

/// 负值类探针（含 i32::MIN，验证符号极值不参与取反）。
const NEGATIVE_PROBES: [i64; PROBES_PER_CLASS] = [-1, -2, -9, -1024, i32::MIN as i64];
/// 未开类探针（全部在册槽号）。
const UNOPENED_PROBES: [i64; PROBES_PER_CLASS] = [3, 4, 7, 16, 31];
/// 已关类探针（先开到该号再关掉）。
const CLOSED_PROBES: [i64; PROBES_PER_CLASS] = [3, 4, 5, 6, 7];
/// 越限类探针（32 是上界本身；31 不属于本类）。
const RANGE_PROBES: [i64; PROBES_PER_CLASS] = [32, 33, 63, 1024, i32::MAX as i64];

impl FdClass {
    /// 段内编码（0..=3），等价类账的下标。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 闭集全量。
    pub const ALL: &'static [FdClass] = &[
        FdClass::Negative,
        FdClass::Unopened,
        FdClass::Closed,
        FdClass::OutOfRange,
    ];

    /// 类名（禁止空串）。
    pub fn name(self) -> &'static str {
        match self {
            FdClass::Negative => "负值",
            FdClass::Unopened => "未开",
            FdClass::Closed => "已关",
            FdClass::OutOfRange => "越限",
        }
    }

    /// 该类第 i 个探针；越界回 0（0 是合法标准流号，必被后续分类判红，
    /// 不会静默通过）。
    pub fn probe(self, i: usize) -> i64 {
        let table: &'static [i64; PROBES_PER_CLASS] = match self {
            FdClass::Negative => &NEGATIVE_PROBES,
            FdClass::Unopened => &UNOPENED_PROBES,
            FdClass::Closed => &CLOSED_PROBES,
            FdClass::OutOfRange => &RANGE_PROBES,
        };
        match table.get(i).copied() {
            Some(v) => v,
            None => 0,
        }
    }
}

// ── 槽位状态机 ────────────────────────────────────────────────────────
/// 单槽状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    Free,
    Live,
    Closed,
}

/// 槽位：状态 + 代际（开关都推进，单调不回退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Slot {
    pub state: SlotState,
    pub gen: u16,
}

/// 空槽常量（自定义结构不能 [None; N]，改用 const 初始化）。
const FREE_SLOT: Slot = Slot { state: SlotState::Free, gen: 0 };

/// 句柄解析结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FdRef {
    Std(u8),
    Dyn { slot: usize, gen: u16 },
}

/// fd 表：标准流位图 + 29 个动态槽 + 四个单调计数。
#[derive(Clone, Copy, Debug)]
pub struct FdBoard {
    slots: [Slot; DYN_SLOTS],
    std_closed: u8,
    live: u32,
    opens: u32,
    closes: u32,
    rejects: u32,
}

impl FdBoard {
    /// 初始态：三条标准流在册，动态槽全空。
    pub const fn new() -> FdBoard {
        FdBoard {
            slots: [FREE_SLOT; DYN_SLOTS],
            std_closed: 0,
            live: STD_FDS as u32,
            opens: STD_FDS as u32,
            closes: 0,
            rejects: 0,
        }
    }

    pub fn live(&self) -> u32 {
        self.live
    }
    pub fn opens(&self) -> u32 {
        self.opens
    }
    pub fn closes(&self) -> u32 {
        self.closes
    }
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 零泄漏恒等式：成功打开 = 成功关闭 + 在册。
    pub fn balanced(&self) -> bool {
        self.opens == self.closes.saturating_add(self.live)
    }

    /// 标准流是否已被本表关闭。
    pub fn std_is_closed(&self, fd: u8) -> bool {
        if fd >= STD_FDS as u8 {
            return false;
        }
        self.std_closed & (1u8 << fd) != 0
    }

    /// 打开新槽：取首个可回收槽（Free 或 Closed——关闭不是死态，状态机
    /// Free → Live(gen) → Closed(gen+1) → Free(gen+2)），代际 +1；
    /// 无可回收槽回 EMFILE 且不动任何槽位。
    pub fn open(&mut self) -> Result<i64, i32> {
        let mut i = 0usize;
        while i < DYN_SLOTS {
            let free = match self.slots.get(i) {
                Some(s) => !matches!(s.state, SlotState::Live),
                None => false,
            };
            if free {
                let next_gen = match self.slots.get(i) {
                    Some(s) => s.gen.wrapping_add(1),
                    None => 1,
                };
                if let Some(s) = self.slots.get_mut(i) {
                    s.state = SlotState::Live;
                    s.gen = next_gen;
                }
                self.live = self.live.saturating_add(1);
                self.opens = self.opens.saturating_add(1);
                return Ok((STD_FDS + i) as i64);
            }
            i += 1;
        }
        self.rejects = self.rejects.saturating_add(1);
        Err(EMFILE)
    }

    /// 关闭 fd：标准流置位图，动态槽转 Closed 并推进代际。
    /// 先过 `validate`——坏 fd 不产生任何状态变化，只推 rejects。
    pub fn close(&mut self, fd: i64) -> Result<u16, i32> {
        match self.validate(fd) {
            Ok(FdRef::Std(n)) => {
                let bit = 1u8 << n;
                if self.std_closed & bit != 0 {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(EBADF);
                }
                self.std_closed |= bit;
                self.live = self.live.saturating_sub(1);
                self.closes = self.closes.saturating_add(1);
                Ok(0)
            }
            Ok(FdRef::Dyn { slot, gen }) => {
                let next_gen = gen.wrapping_add(1);
                if let Some(s) = self.slots.get_mut(slot) {
                    s.state = SlotState::Closed;
                    s.gen = next_gen;
                }
                self.live = self.live.saturating_sub(1);
                self.closes = self.closes.saturating_add(1);
                Ok(next_gen)
            }
            Err(e) => {
                self.rejects = self.rejects.saturating_add(1);
                Err(e)
            }
        }
    }

    /// 无副作用校验：范围闸 → 标准流闸 → 槽位闸。
    pub fn validate(&self, fd: i64) -> Result<FdRef, i32> {
        if fd < 0 {
            return Err(EBADF);
        }
        if fd >= MAX_FDS as i64 {
            return Err(EBADF);
        }
        if fd < STD_FDS as i64 {
            let n = fd as u8;
            if self.std_is_closed(n) {
                return Err(EBADF);
            }
            return Ok(FdRef::Std(n));
        }
        let slot = (fd - STD_FDS as i64) as usize;
        match self.slots.get(slot) {
            Some(s) => {
                if s.state == SlotState::Live {
                    Ok(FdRef::Dyn { slot, gen: s.gen })
                } else {
                    Err(EBADF)
                }
            }
            None => Err(EBADF),
        }
    }

    /// 带调用的校验：失败推 rejects。
    pub fn query(&mut self, fd: i64) -> Result<FdRef, i32> {
        match self.validate(fd) {
            Ok(r) => Ok(r),
            Err(e) => {
                self.rejects = self.rejects.saturating_add(1);
                Err(e)
            }
        }
    }

    /// 代际校验：裸 fd 合法但代际不符 → EBADF（陈旧句柄）。
    pub fn validate_gen(&self, fd: i64, gen: u16) -> Result<usize, i32> {
        match self.validate(fd) {
            Ok(FdRef::Dyn { slot, gen: cur }) => {
                if cur == gen {
                    Ok(slot)
                } else {
                    Err(EBADF)
                }
            }
            Ok(FdRef::Std(_)) => Err(EBADF),
            Err(e) => Err(e),
        }
    }

    /// 等价类判定：None 表示合法在册（不属于任何错误类）。
    pub fn class_of(&self, fd: i64) -> Option<FdClass> {
        if fd < 0 {
            return Some(FdClass::Negative);
        }
        if fd >= MAX_FDS as i64 {
            return Some(FdClass::OutOfRange);
        }
        if fd < STD_FDS as i64 {
            if self.std_is_closed(fd as u8) {
                return Some(FdClass::Closed);
            }
            return None;
        }
        let slot = (fd - STD_FDS as i64) as usize;
        match self.slots.get(slot) {
            Some(s) => match s.state {
                SlotState::Free => Some(FdClass::Unopened),
                SlotState::Closed => Some(FdClass::Closed),
                SlotState::Live => None,
            },
            None => Some(FdClass::OutOfRange),
        }
    }

    /// 槽位指纹：状态 + 代际逐字节喂 FNV-1a，证明坏 fd 连打不动槽。
    pub fn slot_fingerprint(&self) -> u32 {
        let mut bytes: Vec<u8> = Vec::new();
        bytes.push(self.std_closed);
        let mut i = 0usize;
        while i < DYN_SLOTS {
            if let Some(s) = self.slots.get(i) {
                bytes.push(match s.state {
                    SlotState::Free => 0,
                    SlotState::Live => 1,
                    SlotState::Closed => 2,
                });
                bytes.push((s.gen & 0xFF) as u8);
                bytes.push(((s.gen >> 8) & 0xFF) as u8);
            }
            i += 1;
        }
        fnv1a(&bytes)
    }
}

// ── 探针预备与单格复算 ────────────────────────────────────────────────
/// 为某类探针预备一张表，返回关闭代际（非 Closed 类为 0）。
/// Closed 探针必须「先开到该号再关掉」，否则分类只会得到 Unopened。
pub fn prepared(class: FdClass, fd: i64) -> Result<(FdBoard, u16), i32> {
    let mut board = FdBoard::new();
    if class != FdClass::Closed {
        return Ok((board, 0));
    }
    if fd < STD_FDS as i64 {
        return Err(EINVAL);
    }
    let target_live = fd as u32 + 1;
    let mut guard = 0u32;
    while board.live() < target_live && guard < MAX_FDS as u32 {
        if board.open().is_err() {
            return Err(EMFILE);
        }
        guard = guard.saturating_add(1);
    }
    let gen = board.close(fd)?;
    Ok((board, gen))
}

/// 单格复算：(是否检出, 是否回 EBADF)。判据与测试共用同一条路径。
pub fn eval_cell(class: FdClass, fd: i64) -> (bool, bool) {
    match prepared(class, fd) {
        Ok((mut board, gen)) => {
            let live_before = board.live();
            let got = board.query(fd) == Err(EBADF);
            let cls_ok = board.class_of(fd) == Some(class);
            let gen_ok = class != FdClass::Closed || gen > 0;
            let live_ok = board.live() == live_before && board.balanced();
            (got && cls_ok && gen_ok && live_ok, got)
        }
        Err(_) => (false, false),
    }
}

// ── 等价类账 ──────────────────────────────────────────────────────────
/// 单类账格：五列单调。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClassTally {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
    pub ebadf: u32,
    pub other: u32,
}

impl ClassTally {
    /// 零账。
    pub const ZERO: ClassTally = ClassTally {
        injected: 0,
        detected: 0,
        escaped: 0,
        ebadf: 0,
        other: 0,
    };

    /// 注入 = 检出 + 逃逸。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected.saturating_add(self.escaped)
    }
}

/// 四类等价类账。
#[derive(Clone, Copy, Debug)]
pub struct EquivLedger {
    cells: [ClassTally; CLASS_COUNT],
}

impl EquivLedger {
    /// 零账。
    pub const fn zero() -> EquivLedger {
        EquivLedger {
            cells: [ClassTally::ZERO; CLASS_COUNT],
        }
    }

    /// 单类账格视图。
    pub fn cell(&self, class: FdClass) -> Option<&ClassTally> {
        self.cells.get(class.code() as usize)
    }

    /// 记一笔（按类落格；失败笔也入账，不静默丢）。
    pub fn record(&mut self, class: FdClass, caught: bool, ebadf: bool) {
        let idx = class.code() as usize;
        if let Some(c) = self.cells.get_mut(idx) {
            c.injected = c.injected.saturating_add(1);
            if caught {
                c.detected = c.detected.saturating_add(1);
            } else {
                c.escaped = c.escaped.saturating_add(1);
            }
            if ebadf {
                c.ebadf = c.ebadf.saturating_add(1);
            } else {
                c.other = c.other.saturating_add(1);
            }
        }
    }

    /// 五列合计（注入, 检出, 逃逸, EBADF 格, 非 EBADF 格）。
    pub fn totals(&self) -> (u32, u32, u32, u32, u32) {
        let mut inj = 0u32;
        let mut det = 0u32;
        let mut esc = 0u32;
        let mut ebadf = 0u32;
        let mut other = 0u32;
        let mut i = 0usize;
        while i < CLASS_COUNT {
            if let Some(c) = self.cells.get(i) {
                inj = inj.saturating_add(c.injected);
                det = det.saturating_add(c.detected);
                esc = esc.saturating_add(c.escaped);
                ebadf = ebadf.saturating_add(c.ebadf);
                other = other.saturating_add(c.other);
            }
            i += 1;
        }
        (inj, det, esc, ebadf, other)
    }

    /// 四类格子都守恒。
    pub fn all_conserved(&self) -> bool {
        let mut i = 0usize;
        while i < CLASS_COUNT {
            let ok = match self.cells.get(i) {
                Some(c) => c.conserved(),
                None => false,
            };
            if !ok {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 一类账格是否「50 格全检出、全 EBADF、零逃逸、零非 EBADF」。
pub fn class_cell_ok(ledger: &EquivLedger, class: FdClass) -> bool {
    let want = (PROBES_PER_CLASS as u32) * INJECT_ROUNDS;
    match ledger.cell(class) {
        Some(c) => {
            c.injected == want
                && c.detected == want
                && c.escaped == 0
                && c.ebadf == want
                && c.other == 0
                && c.conserved()
        }
        None => false,
    }
}

/// 四类总装：逐类逐探针逐轮落账（单格判定走 `eval_cell` 单一来源）。
pub fn run_equiv_assembly() -> EquivLedger {
    let mut ledger = EquivLedger::zero();
    let mut ci = 0usize;
    while ci < FdClass::ALL.len() {
        if let Some(class) = FdClass::ALL.get(ci).copied() {
            let mut pi = 0usize;
            while pi < PROBES_PER_CLASS {
                let fd = class.probe(pi);
                let mut round = 0u32;
                while round < INJECT_ROUNDS {
                    let (caught, got) = eval_cell(class, fd);
                    ledger.record(class, caught, got);
                    round += 1;
                }
                pi += 1;
            }
        }
        ci += 1;
    }
    ledger
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 正值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", 1, 5, "Windows ERROR_ACCESS_DENIED；权限族"),
    ("EBADF", 9, 6, "Windows ERROR_INVALID_HANDLE；坏句柄族"),
    ("EINVAL", 22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
    ("EMFILE", 24, 4, "Windows ERROR_TOO_MANY_OPEN_FILES；句柄配额族"),
];

/// 按名查正值；表外回 0（0 不是合法 errno，调用方必须显式判）。
pub fn linux_code(name: &'static str) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.0 == name {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
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

/// 小端四字节编码（避免 to_ne_bytes 的平台歧义）。
fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.push((v & 0xFF) as u8);
    out.push(((v >> 8) & 0xFF) as u8);
    out.push(((v >> 16) & 0xFF) as u8);
    out.push(((v >> 24) & 0xFF) as u8);
}

/// 等价类语料指纹：类序 × 五列逐字节编码后哈希，任一列漂移都变指纹。
pub fn equiv_fingerprint() -> u32 {
    let ledger = run_equiv_assembly();
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push(CLASS_COUNT as u8);
    bytes.push(PROBES_PER_CLASS as u8);
    bytes.push(INJECT_ROUNDS as u8);
    let mut i = 0usize;
    while i < CLASS_COUNT {
        if let Some(c) = ledger.cells.get(i) {
            push_u32(&mut bytes, c.injected);
            push_u32(&mut bytes, c.detected);
            push_u32(&mut bytes, c.escaped);
            push_u32(&mut bytes, c.ebadf);
            push_u32(&mut bytes, c.other);
        }
        i += 1;
    }
    fnv1a(&bytes)
}

// ── 写回格（判据③） ───────────────────────────────────────────────────
/// 完成写回：状态 + 非空摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Writeback {
    pub status: u8,
    pub summary: &'static str,
}

/// 只有 200 格全检出且逃逸 0 才允许「已完成」。
pub fn writeback(ledger: &EquivLedger) -> Writeback {
    let expected = (CLASS_COUNT * PROBES_PER_CLASS) as u32 * INJECT_ROUNDS;
    let (inj, det, esc, _ebadf, _other) = ledger.totals();
    let done = inj == expected && det == expected && esc == 0 && ledger.all_conserved();
    if done {
        Writeback {
            status: WB_DONE,
            summary: "四类 fd 等价类总装：20 探针 × 10 轮 = 200 格全 EBADF，逃逸 0",
        }
    } else {
        Writeback {
            status: WB_TODO,
            summary: "等价类总装未达：存在逃逸或账目不守恒格，按类定位",
        }
    }
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9352 判据集（add ≥12；add_dyn ≤8）。
pub fn run_unx9352_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9352");

    let seg_ok = SEG_BASE % 0x40 == 0
        && SEG_BASE >= 0x7C000
        && SEG_BASE <= 0x7C7C0
        && SEG_BASE > 0x7C440
        && SEG_BASE < 0x7C5C0;
    s.add(
        "段位守卫：0x7C580 独占 0x40 窗口，夹在 F9347 与 F9353 之间",
        seg_ok,
        "段基址须 0x40 对齐且不与他卡重叠，否则诊断码互相覆盖",
    );

    let classes_ok = FdClass::ALL.len() == CLASS_COUNT
        && FdClass::ALL
            .iter()
            .enumerate()
            .all(|(i, c)| c.code() as usize == i && !c.name().is_empty());
    s.add(
        "等价类闭集：四类齐备且编码 0..=3 无空洞",
        classes_ok,
        "负值/未开/已关/越限是冻结闭集，编码即账格下标",
    );

    let ledger = run_equiv_assembly();
    let expected_total = (CLASS_COUNT * PROBES_PER_CLASS) as u32 * INJECT_ROUNDS;
    let (inj, det, esc, ebadf, other) = ledger.totals();

    s.add(
        "负值类：5 探针 × 10 轮全 EBADF 且分类命中",
        class_cell_ok(&ledger, FdClass::Negative),
        "fd<0 须在范围闸第一步拒绝，含 i32::MIN 极值",
    );
    s.add(
        "未开类：5 探针 × 10 轮全 EBADF 且分类命中",
        class_cell_ok(&ledger, FdClass::Unopened),
        "空槽不得被当作合法句柄，槽位闸须显性拒绝",
    );
    s.add(
        "已关类：先开后关的 5 探针全 EBADF 且代际推进",
        class_cell_ok(&ledger, FdClass::Closed),
        "关闭须推进代际并转 Closed，否则关闭形同虚设",
    );
    s.add(
        "越限类：5 探针全 EBADF 且分类命中",
        class_cell_ok(&ledger, FdClass::OutOfRange),
        "fd>=32 属越限而非未开，边界 31/32 必须分开判",
    );

    s.add(
        "等价类账：四类各 50 笔，注入=检出+逃逸 且合计 200，other 恒 0",
        ledger.all_conserved()
            && inj == expected_total
            && det == expected_total
            && esc == 0
            && other == 0
            && ebadf == expected_total
            && ledger.cell(FdClass::Negative).map(|c| c.injected) == Some(50)
            && ledger.cell(FdClass::Closed).map(|c| c.injected) == Some(50),
        "逐类账格须守恒；任一类少记多记都说明装配漏了探针",
    );

    let mut order_board = FdBoard::new();
    let fp_before = order_board.slot_fingerprint();
    let mut order_ok = true;
    let mut k = 0u32;
    while k < 20 {
        if order_board.query(-1i64 - k as i64) != Err(EBADF) {
            order_ok = false;
        }
        if order_board.query(MAX_FDS as i64 + k as i64) != Err(EBADF) {
            order_ok = false;
        }
        k += 1;
    }
    let fp_after = order_board.slot_fingerprint();
    s.add(
        "判定次序：40 个坏 fd 连打后槽位指纹不变",
        order_ok && fp_before == fp_after && order_board.live() == STD_FDS as u32,
        "范围闸先于槽位闸：坏 fd 不得进入下标运算、不得改槽",
    );

    let mut gen_board = FdBoard::new();
    let g_fd = match gen_board.open() {
        Ok(v) => v,
        Err(_) => -1,
    };
    let g1 = match gen_board.close(g_fd) {
        Ok(v) => v,
        Err(_) => 0,
    };
    let g_fd2 = match gen_board.open() {
        Ok(v) => v,
        Err(_) => -2,
    };
    let g2 = match gen_board.validate(g_fd2) {
        Ok(FdRef::Dyn { gen, .. }) => gen,
        _ => 0,
    };
    s.add(
        "槽位复用：同号复开代际必增，旧代际拒、新代际过",
        g_fd == g_fd2
            && g2 > g1
            && gen_board.validate_gen(g_fd2, g1) == Err(EBADF)
            && gen_board.validate_gen(g_fd2, g2).is_ok(),
        "代际只增不减；复用号上的陈旧句柄必须失效",
    );

    let mut cap_board = FdBoard::new();
    let mut opened = 0u32;
    while opened < MAX_FDS as u32 {
        if cap_board.open().is_ok() {
            opened += 1;
        } else {
            break;
        }
    }
    let overflow = cap_board.open();
    let reclaimed = match cap_board.close(3) {
        Ok(_) => match cap_board.open() {
            Ok(v) => v == 3,
            Err(_) => false,
        },
        Err(_) => false,
    };
    s.add(
        "容量：29 动态槽满表回 EMFILE，关一槽立即可复用且账目守恒",
        opened == (MAX_FDS as usize - STD_FDS) as u32
            && overflow == Err(EMFILE)
            && reclaimed
            && cap_board.live() == MAX_FDS as u32
            && cap_board.balanced(),
        "满表必须显性回 EMFILE；满表即死表是设计缺陷",
    );

    let mut dbl = FdBoard::new();
    let d_fd = match dbl.open() {
        Ok(v) => v,
        Err(_) => -1,
    };
    let first_close = dbl.close(d_fd);
    let live_after_first = dbl.live();
    s.add(
        "双关：第二次 close 回 EBADF 且在册数不再下降",
        first_close.is_ok()
            && dbl.close(d_fd) == Err(EBADF)
            && dbl.live() == live_after_first
            && dbl.closes() == 1,
        "关闭不是幂等操作；第二次必须拒绝且不重复记账",
    );

    let mut std_board = FdBoard::new();
    let std_close = std_board.close(0);
    s.add(
        "标准流：关闭后归入已关类且查询回 EBADF",
        std_close == Ok(0)
            && std_board.class_of(0) == Some(FdClass::Closed)
            && std_board.validate(0) == Err(EBADF)
            && std_board.live() == (STD_FDS - 1) as u32,
        "标准流无槽位，但关闭语义须与动态槽同构（归 Closed、查询即坏）",
    );

    s.add(
        "Linux 逐码一致：EBADF=9 / EINVAL=22 / EMFILE=24 可复算",
        linux_code("EBADF") == EBADF
            && linux_code("EINVAL") == EINVAL
            && linux_code("EMFILE") == EMFILE
            && linux_code("ENOENT") == 0
            && EBADF == 9
            && EINVAL == 22
            && EMFILE == 24,
        "错误码只允许 errno 表内正值；表外名回 0 由调用方判",
    );

    let wb = writeback(&ledger);
    s.add(
        "写回格：状态=已完成且结果摘要非空",
        wb.status == WB_DONE && !wb.summary.is_empty(),
        "判据③要求状态与摘要同时成立，缺一不得写回已完成",
    );

    let fp_a = equiv_fingerprint();
    let fp_b = equiv_fingerprint();
    s.add(
        "双跑一致：等价类语料指纹相等",
        fp_a == fp_b,
        "同输入两次必须逐字节相等，隐藏状态即失败",
    );

    let mut uniq = true;
    let mut i = 0usize;
    while i < FdCode::ALL.len() {
        let mut j = i + 1;
        while j < FdCode::ALL.len() {
            if let (Some(a), Some(b)) = (FdCode::ALL.get(i), FdCode::ALL.get(j)) {
                if a.code() == b.code() || a.addr() == b.addr() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add(
        "诊断码：12 个码序号与段内地址两两唯一，文案全非空",
        uniq && FdCode::ALL.len() == 12 && FdCode::ALL.iter().all(|c| !c.detail().is_empty()),
        "码值重复会让诊断无法定位到唯一处置建议",
    );

    let neg_inj = ledger.cell(FdClass::Negative).map(|c| c.injected).unwrap_or(0);
    let neg_det = ledger.cell(FdClass::Negative).map(|c| c.detected).unwrap_or(0);
    let sum_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}（负值 {}/{}）",
        inj, det, esc, neg_det, neg_inj
    );
    s.add_dyn("等价类账目快照", ledger.all_conserved(), &sum_text);

    let class_text = format!(
        "负值 {} 未开 {} 已关 {} 越限 {}，格数上限 {}",
        ledger.cell(FdClass::Negative).map(|c| c.ebadf).unwrap_or(0),
        ledger.cell(FdClass::Unopened).map(|c| c.ebadf).unwrap_or(0),
        ledger.cell(FdClass::Closed).map(|c| c.ebadf).unwrap_or(0),
        ledger.cell(FdClass::OutOfRange).map(|c| c.ebadf).unwrap_or(0),
        expected_total
    );
    s.add_dyn("逐类落账格", other == 0, &class_text);

    let fp_text = format!("语料指纹 0x{:08X}（双跑）", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_and_simple_probes() {
        assert_eq!(FdClass::ALL.len(), 4);
        assert_eq!(FdClass::OutOfRange.code(), 3);
        let mut i = 0usize;
        while i < PROBES_PER_CLASS {
            let neg = FdClass::Negative.probe(i);
            let mut a = FdBoard::new();
            assert_eq!(a.query(neg), Err(EBADF));
            assert_eq!(a.class_of(neg), Some(FdClass::Negative));
            let unop = FdClass::Unopened.probe(i);
            let mut b = FdBoard::new();
            assert_eq!(b.query(unop), Err(EBADF));
            assert_eq!(b.class_of(unop), Some(FdClass::Unopened));
            i += 1;
        }
    }

    #[test]
    fn closed_probes_advance_generation() {
        let mut i = 0usize;
        while i < PROBES_PER_CLASS {
            let fd = FdClass::Closed.probe(i);
            let prep = prepared(FdClass::Closed, fd);
            assert!(prep.is_ok());
            let (mut b, gen) = match prep {
                Ok(v) => v,
                Err(_) => (FdBoard::new(), 0),
            };
            assert!(gen > 0);
            assert_eq!(b.class_of(fd), Some(FdClass::Closed));
            assert_eq!(b.query(fd), Err(EBADF));
            i += 1;
        }
    }

    #[test]
    fn range_boundary_is_31_vs_32() {
        let b = FdBoard::new();
        assert_eq!(b.class_of(31), Some(FdClass::Unopened));
        assert_eq!(b.class_of(32), Some(FdClass::OutOfRange));
        assert_eq!(b.class_of(0), None);
        assert_eq!(b.class_of(-1), Some(FdClass::Negative));
    }

    #[test]
    fn ledger_counts_are_conserved() {
        let led = run_equiv_assembly();
        let (inj, det, esc, ebadf, other) = led.totals();
        assert_eq!(inj, 200);
        assert_eq!(det, 200);
        assert_eq!(esc, 0);
        assert_eq!(other, 0);
        assert_eq!(ebadf, 200);
        assert!(led.all_conserved());
        assert_eq!(led.cell(FdClass::Unopened).map(|c| c.injected), Some(50));
        assert_eq!(led.cell(FdClass::OutOfRange).map(|c| c.injected), Some(50));
    }

    #[test]
    fn generation_and_double_close() {
        let mut b = FdBoard::new();
        let fd = match b.open() {
            Ok(v) => v,
            Err(_) => -1,
        };
        let gen1 = match b.close(fd) {
            Ok(v) => v,
            Err(_) => 0,
        };
        let fd2 = match b.open() {
            Ok(v) => v,
            Err(_) => -2,
        };
        let gen2 = match b.validate(fd2) {
            Ok(FdRef::Dyn { gen, .. }) => gen,
            _ => 0,
        };
        assert_eq!(fd, fd2);
        assert!(gen2 > gen1);
        assert_eq!(b.validate_gen(fd2, gen1), Err(EBADF));
        assert!(b.validate_gen(fd2, gen2).is_ok());
        let live = b.live();
        assert_eq!(b.close(fd2), Ok(gen2.wrapping_add(1)));
        assert_eq!(b.close(fd2), Err(EBADF));
        assert_eq!(b.live(), live - 1);
    }

    #[test]
    fn emfile_then_reclaim() {
        let mut b = FdBoard::new();
        let mut n = 0u32;
        while b.open().is_ok() {
            n += 1;
            if n > 64 {
                break;
            }
        }
        assert_eq!(n, (MAX_FDS as usize - STD_FDS) as u32);
        assert_eq!(b.open(), Err(EMFILE));
        assert!(b.close(3).is_ok());
        assert_eq!(b.open(), Ok(3));
        assert!(b.balanced());
    }

    #[test]
    fn std_close_moves_to_closed_class() {
        let mut b = FdBoard::new();
        assert_eq!(b.close(0), Ok(0));
        assert_eq!(b.class_of(0), Some(FdClass::Closed));
        assert_eq!(b.validate(0), Err(EBADF));
        assert_eq!(b.live(), (STD_FDS - 1) as u32);
        assert_eq!(b.close(0), Err(EBADF));
    }

    #[test]
    fn fingerprint_and_writeback() {
        assert_eq!(equiv_fingerprint(), equiv_fingerprint());
        let a = FdBoard::new();
        let b = FdBoard::new();
        assert_eq!(a.slot_fingerprint(), b.slot_fingerprint());
        let wb = writeback(&run_equiv_assembly());
        assert_eq!(wb.status, WB_DONE);
        assert!(!wb.summary.is_empty());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9352_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
