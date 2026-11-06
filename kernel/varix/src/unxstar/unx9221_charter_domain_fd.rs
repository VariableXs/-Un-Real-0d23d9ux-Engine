//! UNX-F9221 fd 语义域总纲（EBADF 判定域与 fd 空间模型） · 0x7A4C0段
//!
//! 判据原文（逐字摘录）：
//! ① 纯功能代码 ≥400 行；② 判据原文：UNX-F9221-J1 fd 空间模型（0/1/2 基座+分配序）判据过，
//! EBADF 判定域三条件（越界/未开/已关）逐格落账，判定先于操作语义格过；
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. fd 空间双模式：charter（基座保留，下界 3）与 linux_like（下界 0）共用同一分配器，
//!    模式差异只落在 floor 一个字段上，可逐格对拍——保留是总纲立场，复用是 Linux 实况。
//! 2. 三条件判定域：`FdClass` 四值（合法/越界/未开/已关），`classify` 是唯一入口，
//!    `perform` 的第一关必须先调它，操作语义永远排在后面。
//! 3. 判定先于操作语义：坏 fd 叠加坏 whence/空指针/坏 flags 时一律先回 EBADF，
//!    EINVAL/EFAULT 只对通过 fd 判定的调用生效（这是本总纲最容易被写反的一格）。
//! 4. 墓碑与世代：close 落 Closed 并 gen+1，重开同槽换世代；旧持有者手里的号
//!    仍然被判为「已关」，不许被重新解释成「未开」。
//! 5. 账实守恒：opens/closes/槽表 Open 计数三账对齐，注入 30 轮漏检为 0；
//!    对账 8 行 errno/Windows 码/模型重算三方对齐，逐行可复算。
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址（派工表给定，0x7A4C0 独占 0x40 格）。
pub const SEG_BASE: u32 = 0x7A4C0;
/// fd 槽容量，与软限对齐。
pub const SLOT_CAP: usize = 64;
/// RLIMIT_NOFILE 软限（本域固定 64，与槽数一致）。
pub const SOFT_LIMIT: i32 = SLOT_CAP as i32;
/// 基座 fd 个数：0/1/2 为标准流。
pub const BASE_FDS: i32 = 3;
/// 每类违约注入轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 注入类别数。
pub const INJECT_CLASSES: usize = 3;

/// 坏 fd 的统一 errno。
pub const EBADF: i32 = 9;
/// 坏指针的统一 errno。
pub const EFAULT: i32 = 14;
/// 参数非法的统一 errno（坏 whence / 坏 flags）。
pub const EINVAL: i32 = 22;
/// fd 表耗尽的统一 errno。
pub const EMFILE: i32 = 24;

/// 诊断码闭集：每条都是「总纲被破坏」的一种形态，不是 errno 的复写。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FdCode {
    BaseLost,
    AllocOrderBroken,
    FloorNotReleased,
    RangeMisjudged,
    TombstoneLost,
    OrderViolation,
    CapacityOverrun,
    CountMismatch,
    LedgerRegress,
    NotReproducible,
    CharterGap,
    SummaryEmpty,
    SegmentViolation,
}

impl FdCode {
    /// 诊断码值：本段基址 + 变体序号，彼此不重叠。
    pub fn val(self) -> u32 { SEG_BASE + self as u32 }
    /// 中文处置建议：只报码不给路等于没报。
    pub fn detail(self) -> &'static str {
        match self {
            FdCode::BaseLost => "基座 0/1/2 未按标准流就位：先装基座再开放分配，禁止让文件槽顶替 0 号",
            FdCode::AllocOrderBroken => "分配序被破坏：必须自下界起取最低空槽，不许跳跃或回卷",
            FdCode::FloorNotReleased => "基座回收后下界未降：0/1/2 全关时应把下界放回 0，否则槽位虚耗",
            FdCode::RangeMisjudged => "越界判定错：负数或 ≥ 软限 的 fd 必须先判越界，不许落到未开分支",
            FdCode::TombstoneLost => "墓碑丢失：关闭的 fd 号必须记 Closed 并保留世代，不得退回未开",
            FdCode::OrderViolation => "判定次序倒置：EBADF 三条件先于 EINVAL/EFAULT 等操作语义码",
            FdCode::CapacityOverrun => "满槽仍分配：64 槽全开必须回 EMFILE，不许挤掉已有槽",
            FdCode::CountMismatch => "账实不符：opens−closes 必须等于槽表 Open 计数，逐轮复核",
            FdCode::LedgerRegress => "账簿回退：检出/漏检/开/关计数只增不减，发现回退即判红",
            FdCode::NotReproducible => "双跑不一致：同脚本两次指纹必须相等，先查时间/随机源",
            FdCode::CharterGap => "总纲缺条：6 条条款须编号连续、文本非空，缺项即返工",
            FdCode::SummaryEmpty => "摘要为空：写回必须包含对账/注入/条款三组数，便于复核",
            FdCode::SegmentViolation => "段位越界：SEG_BASE 必须 0x40 对齐且落在 0x7A000..=0x7D1C0",
        }
    }
    /// 全码表：文案闭集与段位守卫都按它遍历。
    pub const ALL: [FdCode; 13] = [
        FdCode::BaseLost, FdCode::AllocOrderBroken, FdCode::FloorNotReleased,
        FdCode::RangeMisjudged, FdCode::TombstoneLost, FdCode::OrderViolation,
        FdCode::CapacityOverrun, FdCode::CountMismatch, FdCode::LedgerRegress,
        FdCode::NotReproducible, FdCode::CharterGap, FdCode::SummaryEmpty,
        FdCode::SegmentViolation,
    ];
}

/// 打开该 fd 的角色：基座三槽之外的槽一律按业务角色登记。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    None,
    Stdin,
    Stdout,
    Stderr,
    File,
    Pipe,
    Socket,
}

/// 槽状态：Never 从未开过 / Open 打开 / Closed 关过（有墓碑）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    Never,
    Open,
    Closed,
}

/// 单个 fd 槽：状态 + 世代 + 角色。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FdSlot {
    pub state: SlotState,
    pub gen: u16,
    pub role: Role,
}

impl FdSlot {
    /// 空槽常量，供数组铺底。
    pub const fn empty() -> FdSlot {
        FdSlot { state: SlotState::Never, gen: 0, role: Role::None }
    }
}

/// fd 归类结果：EBADF 判定域的四值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FdClass {
    OpenEntry,
    OutOfRange,
    NeverOpened,
    AlreadyClosed,
}

impl FdClass {
    /// 归类名（账本与探针明细用）。
    pub const fn label(self) -> &'static str {
        match self {
            FdClass::OpenEntry => "open",
            FdClass::OutOfRange => "out-of-range",
            FdClass::NeverOpened => "never-opened",
            FdClass::AlreadyClosed => "already-closed",
        }
    }
    /// 是否落在 EBADF 域内（三条件之一）。
    pub const fn is_bad(self) -> bool {
        !matches!(self, FdClass::OpenEntry)
    }
}

/// 操作种类：fd 判定对七种操作一视同仁，语义码各异。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpKind {
    Close,
    Read,
    Write,
    Lseek,
    Fcntl,
    Fstat,
    Dup,
}

impl OpKind {
    /// 操作名（对账表用）。
    pub const fn label(self) -> &'static str {
        match self {
            OpKind::Close => "close", OpKind::Read => "read", OpKind::Write => "write",
            OpKind::Lseek => "lseek", OpKind::Fcntl => "fcntl", OpKind::Fstat => "fstat",
            OpKind::Dup => "dup",
        }
    }
    /// 七种操作全表。
    pub const ALL: [OpKind; 7] = [
        OpKind::Close, OpKind::Read, OpKind::Write, OpKind::Lseek,
        OpKind::Fcntl, OpKind::Fstat, OpKind::Dup,
    ];
}

/// 操作侧附加参数：只在 fd 过关之后才参与判定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpArg {
    None,
    BadWhence,
    NullBuf,
    BadFlags,
}

/// fd 空间：槽表 + 软限 + 分配下界 + 三本账。
#[derive(Clone, Copy, Debug)]
pub struct FdSpace {
    slots: [FdSlot; SLOT_CAP],
    soft: i32,
    floor: i32,
    opens: u32,
    closes: u32,
}

impl FdSpace {
    /// 裸空间：无基座、下界 0，供内部铺底。
    pub const fn bare() -> FdSpace {
        FdSpace { slots: [FdSlot::empty(); SLOT_CAP], soft: SOFT_LIMIT, floor: 0, opens: 0, closes: 0 }
    }
    /// 开机（linux_like）：装 0/1/2 基座，下界 0，关掉基座后 0 号可被复用。
    pub fn boot() -> FdSpace {
        let mut space = FdSpace::bare();
        space.install_base();
        space
    }
    /// 开机（charter）：装 0/1/2 基座并把下界钉在 3，基座槽不许被新开顶替。
    pub fn boot_pinned() -> FdSpace {
        let mut space = FdSpace::boot();
        space.floor = BASE_FDS;
        space
    }
    /// 装基座三槽，世代从 1 起算。
    fn install_base(&mut self) {
        let mut fd = 0i32;
        while fd < BASE_FDS {
            let role = match fd { 0 => Role::Stdin, 1 => Role::Stdout, _ => Role::Stderr };
            if let Some(slot) = self.slots.get_mut(fd as usize) {
                *slot = FdSlot { state: SlotState::Open, gen: 1, role };
            }
            fd += 1;
        }
        self.opens = self.opens.saturating_add(BASE_FDS as u32);
    }
    /// 软限。
    pub const fn soft(&self) -> i32 { self.soft }
    /// 分配下界。
    pub const fn floor(&self) -> i32 { self.floor }
    /// 累计打开次数（含基座）。
    pub const fn opens(&self) -> u32 { self.opens }
    /// 累计关闭次数。
    pub const fn closes(&self) -> u32 { self.closes }
    /// 取槽：越界返回 None，调用方据此再归类。
    pub fn slot(&self, fd: i32) -> Option<FdSlot> {
        if fd < 0 { return None; }
        self.slots.get(fd as usize).copied()
    }
    /// 唯一归类入口：越界 → 未开 → 已关 → 合法。
    pub fn classify(&self, fd: i32) -> FdClass {
        if fd < 0 || fd >= self.soft { return FdClass::OutOfRange; }
        match self.slot(fd) {
            Some(slot) => match slot.state {
                SlotState::Open => FdClass::OpenEntry,
                SlotState::Never => FdClass::NeverOpened,
                SlotState::Closed => FdClass::AlreadyClosed,
            },
            None => FdClass::OutOfRange,
        }
    }
    /// 分配：自下界起取最低空槽；槽满回 EMFILE（不挤占既有槽）。
    pub fn open(&mut self, role: Role) -> Result<i32, i32> {
        let start = if self.floor > 0 { self.floor } else { 0 };
        let mut fd = start;
        while fd < self.soft {
            let idx = fd as usize;
            let (state, gen) = match self.slots.get(idx) {
                Some(slot) => (slot.state, slot.gen),
                None => (SlotState::Open, 0),
            };
            if state != SlotState::Open {
                if let Some(slot) = self.slots.get_mut(idx) {
                    *slot = FdSlot { state: SlotState::Open, gen: gen.saturating_add(1), role };
                    self.opens = self.opens.saturating_add(1);
                    return Ok(fd);
                }
            }
            fd += 1;
        }
        Err(EMFILE)
    }
    /// 关闭：坏 fd 一律 EBADF；合法槽落墓碑并递增世代。
    pub fn close(&mut self, fd: i32) -> Result<i32, i32> {
        if self.classify(fd).is_bad() { return Err(EBADF); }
        let gen = match self.slots.get(fd as usize) {
            Some(slot) => slot.gen,
            None => 0,
        };
        if let Some(slot) = self.slots.get_mut(fd as usize) {
            *slot = FdSlot { state: SlotState::Closed, gen: gen.saturating_add(1), role: Role::None };
        }
        self.closes = self.closes.saturating_add(1);
        Ok(0)
    }
    /// 基座回收：0/1/2 全关时把下界放回 0（charter 模式的唯一降界入口）。
    pub fn reclaim_base(&mut self) -> bool {
        let mut all_closed = true;
        let mut fd = 0i32;
        while fd < BASE_FDS {
            match self.slots.get(fd as usize) {
                Some(slot) => {
                    if slot.state == SlotState::Open { all_closed = false; }
                }
                None => all_closed = false,
            }
            fd += 1;
        }
        if all_closed { self.floor = 0; }
        all_closed
    }
    /// 当前打开的槽数。
    pub fn open_count(&self) -> u32 {
        (0..SLOT_CAP).filter_map(|i| self.slots.get(i))
            .filter(|slot| slot.state == SlotState::Open).count() as u32
    }
    /// 墓碑槽数。
    pub fn tombstones(&self) -> u32 {
        (0..SLOT_CAP).filter_map(|i| self.slots.get(i))
            .filter(|slot| slot.state == SlotState::Closed).count() as u32
    }
    /// 某槽世代；越界回 0。
    pub fn gen_of(&self, fd: i32) -> u16 {
        match self.slot(fd) {
            Some(slot) => slot.gen,
            None => 0,
        }
    }
    /// 空间指纹：槽状态/世代/角色 + 三本账末字节。
    pub fn fingerprint(&self) -> u32 {
        let mut bytes: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < SLOT_CAP {
            if let Some(slot) = self.slots.get(i) {
                bytes.push(slot.state as u8);
                bytes.push((slot.gen & 0xFF) as u8);
                bytes.push(slot.role as u8);
            }
            i += 1;
        }
        bytes.push((self.floor & 0xFF) as u8);
        bytes.push((self.opens & 0xFF) as u8);
        bytes.push((self.closes & 0xFF) as u8);
        fnv1a(&bytes)
    }
}

/// 执行一次操作：第一关 fd 判定，第二关操作语义。
///
/// 这是「判定先于操作语义」的唯一实现点——任何调用都绕不过 classify。
pub fn perform(space: &mut FdSpace, op: OpKind, fd: i32, arg: OpArg) -> Result<i32, i32> {
    if space.classify(fd).is_bad() {
        return Err(EBADF);
    }
    match (op, arg) {
        (OpKind::Lseek, OpArg::BadWhence) => Err(EINVAL),
        (OpKind::Fcntl, OpArg::BadFlags) => Err(EINVAL),
        (OpKind::Dup, OpArg::BadFlags) => Err(EINVAL),
        (OpKind::Read, OpArg::NullBuf) => Err(EFAULT),
        (OpKind::Write, OpArg::NullBuf) => Err(EFAULT),
        (OpKind::Close, _) => space.close(fd),
        _ => Ok(0),
    }
}

/// 注入类别：三条件各一类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FdInject {
    OutOfRange,
    NeverOpened,
    ReusedAfterClose,
}

impl FdInject {
    /// 该类别应落到的归类。
    pub const fn expected(self) -> FdClass {
        match self {
            FdInject::OutOfRange => FdClass::OutOfRange,
            FdInject::NeverOpened => FdClass::NeverOpened,
            FdInject::ReusedAfterClose => FdClass::AlreadyClosed,
        }
    }
    /// 三类全表。
    pub const ALL: [FdInject; INJECT_CLASSES] = [
        FdInject::OutOfRange, FdInject::NeverOpened, FdInject::ReusedAfterClose,
    ];
}

/// 注入账簿：三条件分账 + 检出/漏检总账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FdLedger {
    pub out_of_range: u32,
    pub never_opened: u32,
    pub already_closed: u32,
    pub detects: u32,
    pub misses: u32,
}

impl FdLedger {
    /// 空账。
    pub const fn empty() -> FdLedger {
        FdLedger { out_of_range: 0, never_opened: 0, already_closed: 0, detects: 0, misses: 0 }
    }
    /// 记一笔：归类对且探测回 EBADF 才算检出。
    pub fn account(&mut self, expected: FdClass, actual: FdClass, hit: bool) {
        if hit && actual == expected {
            match actual {
                FdClass::OutOfRange => self.out_of_range = self.out_of_range.saturating_add(1),
                FdClass::NeverOpened => self.never_opened = self.never_opened.saturating_add(1),
                FdClass::AlreadyClosed => self.already_closed = self.already_closed.saturating_add(1),
                FdClass::OpenEntry => {}
            }
            self.detects = self.detects.saturating_add(1);
        } else {
            self.misses = self.misses.saturating_add(1);
        }
    }
    /// 守恒：检出 + 漏检 = 注入总数，且三条件分账之和 = 检出。
    pub fn conserved(&self) -> bool {
        let total = INJECT_ROUNDS * INJECT_CLASSES as u32;
        self.detects.saturating_add(self.misses) == total
            && self.out_of_range.saturating_add(self.never_opened)
                .saturating_add(self.already_closed) == self.detects
    }
    /// 分类计数。
    pub fn per(&self, class: FdClass) -> u32 {
        match class {
            FdClass::OutOfRange => self.out_of_range,
            FdClass::NeverOpened => self.never_opened,
            FdClass::AlreadyClosed => self.already_closed,
            FdClass::OpenEntry => 0,
        }
    }
    /// 账簿摘要。
    pub fn summary(&self) -> String {
        format!("越界 {} / 未开 {} / 已关 {} / 检出 {} / 漏检 {}",
                self.out_of_range, self.never_opened, self.already_closed,
                self.detects, self.misses)
    }
}

/// 单轮注入：构造坏 fd，断言归类 + EBADF +（叠加坏 whence 后仍是 EBADF）。
pub fn inject_once(kind: FdInject, round: u32) -> (FdClass, bool) {
    let mut space = FdSpace::boot();
    let fd = match kind {
        FdInject::OutOfRange => -1 - (round as i32 % 9),
        FdInject::NeverOpened => 9 + (round as i32 % 5),
        FdInject::ReusedAfterClose => {
            let opened = match space.open(Role::File) {
                Ok(v) => v,
                Err(_) => BASE_FDS,
            };
            let _ = space.close(opened);
            opened
        }
    };
    let class = space.classify(fd);
    let probe = perform(&mut space, OpKind::Close, fd, OpArg::None);
    let arg_probe = perform(&mut space, OpKind::Lseek, fd, OpArg::BadWhence);
    (class, probe == Err(EBADF) && arg_probe == Err(EBADF))
}

/// 三类 × 10 轮注入矩阵，返回账簿与分类计数。
pub fn run_fd_matrix() -> (FdLedger, [u32; INJECT_CLASSES]) {
    let mut ledger = FdLedger::empty();
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut i = 0usize;
        while i < INJECT_CLASSES {
            let kind = match FdInject::ALL.get(i) {
                Some(k) => *k,
                None => FdInject::OutOfRange,
            };
            let (actual, hit) = inject_once(kind, round);
            ledger.account(kind.expected(), actual, hit);
            i += 1;
        }
        round += 1;
    }
    let per = [ledger.out_of_range, ledger.never_opened, ledger.already_closed];
    (ledger, per)
}

/// 判定次序探针：越界 → 未开 → 已关，三类互不混判。
pub fn judge_order_probe() -> (FdClass, FdClass, FdClass) {
    let mut space = FdSpace::boot();
    let first = space.classify(-1);
    let second = space.classify(9);
    let opened = match space.open(Role::File) {
        Ok(v) => v,
        Err(_) => BASE_FDS,
    };
    let _ = space.close(opened);
    let third = space.classify(opened);
    (first, second, third)
}

/// 总纲条款。
#[derive(Clone, Copy, Debug)]
pub struct Clause {
    pub no: u8,
    pub text: &'static str,
}

/// 本域六条总纲：后续 E 型矩阵全部由它派生。
pub const CHARTER: [Clause; 6] = [
    Clause { no: 1, text: "fd 自 0 起连续编号，0/1/2 为进程基座，开机即按标准流就位" },
    Clause { no: 2, text: "分配自下界起取最低空槽；基座全关并回收后下界归 0，否则钉在 3" },
    Clause { no: 3, text: "越界/未开/已关三条件一律回 EBADF，不许降级成 EINVAL/EFAULT" },
    Clause { no: 4, text: "判定先于操作语义：坏 fd 的 EBADF 永远排在 whence/指针/flags 之前" },
    Clause { no: 5, text: "关闭留墓碑并递增世代，旧号不得被重新解释成未开" },
    Clause { no: 6, text: "开/关/探测三账只增不减，账实不符即整册返工" },
];

/// 条款闭集：编号连续、正文非空、条数等于 6。
pub fn charter_closed() -> bool {
    if CHARTER.len() != 6 { return false; }
    let mut i = 0usize;
    while i < CHARTER.len() {
        match CHARTER.get(i) {
            Some(clause) => {
                if clause.no as usize != i + 1 || clause.text.is_empty() { return false; }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 对账行：操作 + 探针 fd + 附加参数 + 期望 errno + 符号名 + Windows 码。
#[derive(Clone, Copy, Debug)]
pub struct ReconRow {
    /// 操作
    pub op: OpKind,
    /// 探针 fd
    pub fd: i32,
    /// 附加参数
    pub arg: OpArg,
    /// 期望 errno
    pub errno: i32,
    /// errno 符号名
    pub name: &'static str,
    /// Windows 对照码
    pub win: u16,
}

/// 对账表八行：七种操作的坏 fd 格 + 一格合法 fd 坏 whence。
pub const RECON_TABLE: [ReconRow; 8] = [
    ReconRow { op: OpKind::Close, fd: -1, arg: OpArg::None, errno: EBADF, name: "EBADF", win: 6 },
    ReconRow { op: OpKind::Read, fd: -1, arg: OpArg::NullBuf, errno: EBADF, name: "EBADF", win: 6 },
    ReconRow { op: OpKind::Write, fd: -1, arg: OpArg::NullBuf, errno: EBADF, name: "EBADF", win: 6 },
    ReconRow { op: OpKind::Lseek, fd: -1, arg: OpArg::BadWhence, errno: EBADF, name: "EBADF", win: 6 },
    ReconRow { op: OpKind::Fcntl, fd: -1, arg: OpArg::BadFlags, errno: EBADF, name: "EBADF", win: 6 },
    ReconRow { op: OpKind::Fstat, fd: -1, arg: OpArg::None, errno: EBADF, name: "EBADF", win: 6 },
    ReconRow { op: OpKind::Dup, fd: -1, arg: OpArg::BadFlags, errno: EBADF, name: "EBADF", win: 6 },
    ReconRow { op: OpKind::Lseek, fd: 0, arg: OpArg::BadWhence, errno: EINVAL, name: "EINVAL", win: 87 },
];

/// 逐行重算：返回（一致行数, 总行数）。
pub fn reconcile() -> (u32, u32) {
    let mut ok = 0u32;
    let mut i = 0usize;
    while i < RECON_TABLE.len() {
        if let Some(row) = RECON_TABLE.get(i) {
            let mut space = FdSpace::boot();
            let got = perform(&mut space, row.op, row.fd, row.arg);
            if got == Err(row.errno) { ok = ok.saturating_add(1); }
        }
        i += 1;
    }
    (ok, RECON_TABLE.len() as u32)
}

/// 失配明细：全一致时给一句确定话，便于写进动态槽。
pub fn recon_mismatch_text() -> String {
    let mut text = String::new();
    let mut i = 0usize;
    while i < RECON_TABLE.len() {
        if let Some(row) = RECON_TABLE.get(i) {
            let mut space = FdSpace::boot();
            let got = perform(&mut space, row.op, row.fd, row.arg);
            if got != Err(row.errno) {
                text.push_str(&format!("第{}行 {} 表={} 实={:?}; ", i + 1, row.op.label(), row.name, got));
            }
        }
        i += 1;
    }
    if text.is_empty() { text.push_str("8 行逐项一致"); }
    text
}

/// FNV-1a 32 位：无浮点无时钟，双跑必然同值。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(byte) = bytes.get(i) {
            hash ^= *byte as u32;
            hash = hash.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    hash
}

/// 巡逻脚本：固定动作序列，用于双跑一致判据。
pub fn patrol_fingerprint() -> u32 {
    let mut space = FdSpace::boot_pinned();
    let mut i = 0u32;
    while i < 5 {
        let _ = space.open(Role::File);
        i += 1;
    }
    let _ = space.close(0);
    let _ = space.close(3);
    let _ = space.open(Role::Pipe);
    space.fingerprint()
}

/// 写回摘要：对账 / 注入 / 条款三组数缺一不可。
pub fn charter_summary() -> String {
    let (ok, total) = reconcile();
    let (ledger, _) = run_fd_matrix();
    format!("F9221 fd 总纲：槽 {} / 软限 {} / 对账 {}/{} / 注入 检{} 漏{} / 总纲 {} 条",
            SLOT_CAP, SOFT_LIMIT, ok, total, ledger.detects, ledger.misses, CHARTER.len())
}

/// 本卡判据入口：静态明细为主，动态槽只留四条最关键的探针。
pub fn run_unx9221_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9221");

    // ── 段位守卫 ─────────────────────────────────────────────────────
    s.add("段位：0x7A4C0 0x40 对齐且落在 0x7A000..=0x7D1C0 内",
          SEG_BASE == 0x7A4C0 && SEG_BASE % 0x40 == 0
              && SEG_BASE >= 0x7A000 && SEG_BASE <= 0x7D1C0,
          "段位独占，越界即整册返工");

    // ── 正常流：基座 ─────────────────────────────────────────────────
    let boot = FdSpace::boot();
    s.add("正常流：开机 0/1/2 就位为 Stdin/Stdout/Stderr",
          boot.classify(0) == FdClass::OpenEntry
              && boot.classify(1) == FdClass::OpenEntry
              && boot.classify(2) == FdClass::OpenEntry
              && matches!(boot.slot(0), Some(slot) if slot.role == Role::Stdin)
              && matches!(boot.slot(1), Some(slot) if slot.role == Role::Stdout)
              && matches!(boot.slot(2), Some(slot) if slot.role == Role::Stderr)
              && boot.open_count() == 3,
          "基座三槽必须是标准流，不许被文件槽顶替");

    // ── 分配序：charter 模式 ─────────────────────────────────────────
    let mut order = FdSpace::boot_pinned();
    let first = order.open(Role::File);
    let second = order.open(Role::File);
    s.add("分配序：基座保留模式自 3 起取最低空槽",
          first == Ok(3) && second == Ok(4) && order.floor() == 3
              && order.open_count() == 5,
          "下界 3 保证基座不被新开顶掉，分配自 3 递增");

    // ── 分配序：linux_like 与回收 ───────────────────────────────────
    let mut reuse = FdSpace::boot();
    let _ = reuse.close(0);
    let recycled = reuse.open(Role::Pipe);
    let mut pinned = FdSpace::boot_pinned();
    let _ = pinned.close(0);
    let held = pinned.open(Role::Pipe);
    let released = {
        let _ = pinned.close(1);
        let _ = pinned.close(2);
        let ok = pinned.reclaim_base();
        (ok, pinned.floor(), pinned.open(Role::File))
    };
    s.add("分配序：linux_like 关基座后复用得 0，charter 模式须先回收下界",
          recycled == Ok(0) && held == Ok(3)
              && released.0 && released.1 == 0 && released.2 == Ok(0),
          "复用与保留是模式差异，下界回收后 0 号才可再分配");

    // ── 边界四类 ─────────────────────────────────────────────────────
    let bound = FdSpace::boot_pinned();
    s.add("边界：fd 0 合法、-1 越界、63 未开、64 与 65 越界",
          bound.classify(0) == FdClass::OpenEntry
              && bound.classify(-1) == FdClass::OutOfRange
              && bound.classify(63) == FdClass::NeverOpened
              && bound.classify(64) == FdClass::OutOfRange
              && bound.classify(65) == FdClass::OutOfRange,
          "四类边界：0 合法；负数越界；软限-1 未开；软限及以上越界");

    // ── 三条件 × 七操作逐格 ─────────────────────────────────────────
    let mut tri = FdSpace::boot();
    let closed_slot = match tri.open(Role::File) {
        Ok(v) => v,
        Err(_) => BASE_FDS,
    };
    let _ = tri.close(closed_slot);
    let mut all_ebadf = true;
    let mut k = 0usize;
    while k < OpKind::ALL.len() {
        if let Some(op) = OpKind::ALL.get(k) {
            let mut probe = FdSpace::boot();
            let p = match probe.open(Role::File) {
                Ok(v) => v,
                Err(_) => BASE_FDS,
            };
            let _ = probe.close(p);
            if perform(&mut probe, *op, -1, OpArg::NullBuf) != Err(EBADF) { all_ebadf = false; }
            if perform(&mut probe, *op, 9, OpArg::BadWhence) != Err(EBADF) { all_ebadf = false; }
            if perform(&mut probe, *op, p, OpArg::BadFlags) != Err(EBADF) { all_ebadf = false; }
        }
        k += 1;
    }
    s.add("三条件逐格：越界/未开/已关 × 7 操作共 21 格全回 EBADF",
          all_ebadf && tri.classify(closed_slot) == FdClass::AlreadyClosed,
          "21 格必须无一漏格，任何一格回别的码都算总纲被撕开");

    // ── 判定先于操作语义 ────────────────────────────────────────────
    let mut ord = FdSpace::boot();
    s.add("判定先于语义：坏 fd + 坏 whence/flags → EBADF 而非 EINVAL",
          perform(&mut ord, OpKind::Lseek, -1, OpArg::BadWhence) == Err(EBADF)
              && perform(&mut ord, OpKind::Fcntl, 12, OpArg::BadFlags) == Err(EBADF)
              && perform(&mut ord, OpKind::Dup, 12, OpArg::BadFlags) == Err(EBADF),
          "语义码只对合法 fd 生效，坏 fd 一律先回 EBADF");
    s.add("判定先于语义：坏 fd + 空指针 → EBADF 而非 EFAULT",
          perform(&mut ord, OpKind::Read, -1, OpArg::NullBuf) == Err(EBADF)
              && perform(&mut ord, OpKind::Write, 12, OpArg::NullBuf) == Err(EBADF),
          "read/write 的 EFAULT 位于 EBADF 之后，不得抢先");
    s.add("语义关：合法 fd 上坏 whence → EINVAL、空指针 → EFAULT",
          perform(&mut ord, OpKind::Lseek, 0, OpArg::BadWhence) == Err(EINVAL)
              && perform(&mut ord, OpKind::Read, 0, OpArg::NullBuf) == Err(EFAULT),
          "fd 过关后操作语义才轮到回 EINVAL/EFAULT");

    // ── 状态机与世代 ────────────────────────────────────────────────
    let mut st = FdSpace::boot_pinned();
    let g0 = st.gen_of(3);
    let opened = st.open(Role::File);
    let g1 = st.gen_of(3);
    let closed = match st.close(3) {
        Ok(v) => v,
        Err(_) => -1,
    };
    let g2 = st.gen_of(3);
    let again = st.close(3);
    // 墓碑必须在重开之前取样：重开同一槽后状态已回到 Open，旧墓碑
    // 只剩代际可辨，再用 classify 断言就变成自欺。
    let tombstone = st.classify(3);
    let reopened = st.open(Role::Socket);
    let g3 = st.gen_of(3);
    s.add("状态机：Never→Open→Closed 单向，二次 close → EBADF",
          opened == Ok(3) && closed == 0 && again == Err(EBADF)
              && tombstone == FdClass::AlreadyClosed && reopened == Ok(3),
          "关后墓碑必须保留，二次 close 不许静默成功");
    s.add("世代：同槽每经一次开关 gen 递增，重开即换世代",
          g0 < g1 && g1 < g2 && g2 < g3,
          "世代是识别陈旧 fd 的唯一凭据，只增不减");

    // ── 容量 ─────────────────────────────────────────────────────────
    let mut cap = FdSpace::boot_pinned();
    let mut n = 0u32;
    while n < (SLOT_CAP as u32 - BASE_FDS as u32) {
        let _ = cap.open(Role::File);
        n += 1;
    }
    let full = cap.open(Role::File);
    let reuse_after_full = {
        let _ = cap.close(5);
        (cap.open(Role::Pipe), cap.open_count())
    };
    s.add("容量：第 65 次 open → EMFILE；关一槽后复用最低空槽且账不丢",
          full == Err(EMFILE) && cap.classify(63) == FdClass::OpenEntry
              && reuse_after_full.0 == Ok(5) && reuse_after_full.1 == SLOT_CAP as u32,
          "满槽必须回 EMFILE；腾出的最低空槽必须复用，64 槽一个不丢");

    // ── 注入必抓与守恒 ──────────────────────────────────────────────
    let (ledger, per) = run_fd_matrix();
    s.add("注入必抓：越界/未开/已关三类各 10/10",
          matches!(per.get(0), Some(v) if *v == INJECT_ROUNDS)
              && matches!(per.get(1), Some(v) if *v == INJECT_ROUNDS)
              && matches!(per.get(2), Some(v) if *v == INJECT_ROUNDS),
          "三类违约各 10 轮必须全检出，漏一轮即红");
    s.add("守恒：注入 30 次 = 检出 + 漏检，漏检为 0",
          ledger.detects == INJECT_ROUNDS * INJECT_CLASSES as u32
              && ledger.misses == 0 && ledger.conserved(),
          "账簿自洽：检出与漏检之和恒等于注入轮数");

    // ── 判定次序探针 ────────────────────────────────────────────────
    let (j1, j2, j3) = judge_order_probe();
    s.add("判定次序：越界 > 未开 > 已关，三类不混判",
          j1 == FdClass::OutOfRange && j2 == FdClass::NeverOpened
              && j3 == FdClass::AlreadyClosed,
          "同一 fd 只归一类：负数先判越界，空槽判未开，墓碑判已关");

    // ── 双跑一致 ────────────────────────────────────────────────────
    let f1 = patrol_fingerprint();
    let f2 = patrol_fingerprint();
    s.add("双跑一致：同脚本两次巡逻指纹相等且非零", f1 == f2 && f1 != 0,
          "无时钟无随机，同输入必须逐字段同指纹");

    // ── 对账 ────────────────────────────────────────────────────────
    let (rok, rtotal) = reconcile();
    s.add("对账：8 行 errno 与模型重算逐项一致",
          rok == rtotal && rtotal == 8,
          "EBADF/EINVAL 两码与 Windows 6/87 对表，逐行可复算");

    // ── 账实守恒 ────────────────────────────────────────────────────
    let mut cons = FdSpace::boot_pinned();
    let _ = cons.open(Role::File);
    let _ = cons.open(Role::File);
    let _ = cons.close(0);
    let _ = cons.close(4);
    s.add("守恒：opens − closes == Open 计数，且关过的号判已关",
          cons.opens() == 5 && cons.closes() == 2 && cons.open_count() == 3
              && cons.tombstones() == 2 && cons.classify(4) == FdClass::AlreadyClosed,
          "三账必须相等（开 5 关 2 余 3）；关掉的 fd 号不许退回未开");

    // ── 总纲与文案闭集 ──────────────────────────────────────────────
    s.add("总纲：6 条条款编号连续且正文非空", charter_closed(),
          "条款缺项会让后续模块各自为政，必须逐条落账");
    let mut text_closed = true;
    let mut d = 0usize;
    while d < FdCode::ALL.len() {
        match FdCode::ALL.get(d) {
            Some(code) => {
                if code.detail().is_empty() { text_closed = false; }
                if code.val() < SEG_BASE || code.val() > SEG_BASE + 0x3F { text_closed = false; }
            }
            None => text_closed = false,
        }
        d += 1;
    }
    s.add("文案闭集：13 个诊断码均有非空中文建议且落本段", text_closed,
          "诊断码必须自带处置建议，避免只报码不给路");

    // ── 收口 ────────────────────────────────────────────────────────
    let summary = charter_summary();
    s.add("收口：摘要非空且含对账/注入/总纲三组数",
          !summary.is_empty() && summary.contains("对账")
              && summary.contains("注入") && summary.contains("总纲"),
          "写回摘要必须一眼看出对账与注入结果");

    // ── 动态明细（≤8 槽，此处 4 条）────────────────────────────────
    let mut dyn_space = FdSpace::boot_pinned();
    let dyn_open = match dyn_space.open(Role::File) {
        Ok(v) => v,
        Err(_) => -1,
    };
    let dyn_bad = dyn_space.classify(-1);
    s.add_dyn("越界探针明细", dyn_bad == FdClass::OutOfRange,
              &format!("fd=-1 归类 {}，首开得 {}", dyn_bad.label(), dyn_open));
    let dyn_closed = {
        let _ = dyn_space.close(dyn_open);
        let again = dyn_space.close(dyn_open);
        (dyn_space.classify(dyn_open), again, dyn_space.gen_of(dyn_open))
    };
    s.add_dyn("已关探针明细", dyn_closed.0 == FdClass::AlreadyClosed,
              &format!("归类 {} / 二次 close {:?} / gen {}",
                       dyn_closed.0.label(), dyn_closed.1, dyn_closed.2));
    let recon_text = recon_mismatch_text();
    s.add_dyn("对账失配明细", recon_text.starts_with("8 行"), &recon_text);
    s.add_dyn("注入账簿", ledger.conserved() && ledger.misses == 0, &ledger.summary());

    s
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn boot_base_and_order() {
        let space = FdSpace::boot();
        assert_eq!(space.open_count(), 3);
        assert_eq!(space.classify(0), FdClass::OpenEntry);
        assert_eq!(space.slot(2).map(|s| s.role), Some(Role::Stderr));

        let mut pinned = FdSpace::boot_pinned();
        assert_eq!(pinned.floor(), 3);
        assert_eq!(pinned.open(Role::File), Ok(3));
        assert_eq!(pinned.open(Role::Pipe), Ok(4));
        assert_eq!(pinned.open(Role::Socket), Ok(5));
    }

    #[test]
    fn floor_release_and_reuse() {
        let mut linux = FdSpace::boot();
        assert_eq!(linux.close(0), Ok(0));
        assert_eq!(linux.open(Role::Pipe), Ok(0));

        let mut pinned = FdSpace::boot_pinned();
        assert_eq!(pinned.close(0), Ok(0));
        assert_eq!(pinned.open(Role::File), Ok(3));
        assert!(!pinned.reclaim_base());
        assert_eq!(pinned.close(1), Ok(0));
        assert_eq!(pinned.close(2), Ok(0));
        assert!(pinned.reclaim_base());
        assert_eq!(pinned.floor(), 0);
        assert_eq!(pinned.open(Role::Socket), Ok(0));

        let bounds = FdSpace::boot_pinned();
        assert_eq!(bounds.classify(0), FdClass::OpenEntry);
        assert_eq!(bounds.classify(-1), FdClass::OutOfRange);
        assert_eq!(bounds.classify(-4096), FdClass::OutOfRange);
        assert_eq!(bounds.classify(63), FdClass::NeverOpened);
        assert_eq!(bounds.classify(64), FdClass::OutOfRange);
        assert_eq!(bounds.classify(65), FdClass::OutOfRange);
    }

    #[test]
    fn tri_condition_all_ops() {
        let mut space = FdSpace::boot();
        let fd = space.open(Role::File).unwrap_or(3);
        assert_eq!(space.close(fd), Ok(0));
        let mut i = 0usize;
        while i < OpKind::ALL.len() {
            if let Some(op) = OpKind::ALL.get(i) {
                let mut probe = FdSpace::boot();
                let p = probe.open(Role::File).unwrap_or(3);
                assert_eq!(probe.close(p), Ok(0));
                assert_eq!(perform(&mut probe, *op, -1, OpArg::NullBuf), Err(EBADF));
                assert_eq!(perform(&mut probe, *op, 9, OpArg::BadWhence), Err(EBADF));
                assert_eq!(perform(&mut probe, *op, p, OpArg::BadFlags), Err(EBADF));
            }
            i += 1;
        }
        assert_eq!(space.classify(fd), FdClass::AlreadyClosed);
    }

    #[test]
    fn judge_before_semantics() {
        let mut space = FdSpace::boot();
        assert_eq!(perform(&mut space, OpKind::Lseek, -1, OpArg::BadWhence), Err(EBADF));
        assert_eq!(perform(&mut space, OpKind::Read, -1, OpArg::NullBuf), Err(EBADF));
        assert_eq!(perform(&mut space, OpKind::Lseek, 0, OpArg::BadWhence), Err(EINVAL));
        assert_eq!(perform(&mut space, OpKind::Read, 0, OpArg::NullBuf), Err(EFAULT));
        assert_eq!(perform(&mut space, OpKind::Fcntl, 0, OpArg::BadFlags), Err(EINVAL));
        assert_eq!(perform(&mut space, OpKind::Close, 0, OpArg::None), Ok(0));
        assert_eq!(space.classify(0), FdClass::AlreadyClosed);
    }

    #[test]
    fn state_machine_and_generation() {
        let mut space = FdSpace::boot_pinned();
        assert_eq!(space.gen_of(3), 0);
        assert_eq!(space.open(Role::File), Ok(3));
        let g1 = space.gen_of(3);
        assert_eq!(space.close(3), Ok(0));
        let g2 = space.gen_of(3);
        assert_eq!(space.close(3), Err(EBADF));
        assert_eq!(space.classify(3), FdClass::AlreadyClosed);
        assert_eq!(space.open(Role::File), Ok(3));
        assert!(g1 > 0 && g2 > g1 && space.gen_of(3) > g2);
        assert_eq!(space.tombstones(), 0);
    }

    #[test]
    fn capacity_emfile_and_reuse() {
        let mut space = FdSpace::boot_pinned();
        let mut n = 0u32;
        while n < (SLOT_CAP as u32 - BASE_FDS as u32) {
            assert!(space.open(Role::File).is_ok());
            n += 1;
        }
        assert_eq!(space.open_count(), SLOT_CAP as u32);
        assert_eq!(space.open(Role::File), Err(EMFILE));
        assert_eq!(space.close(5), Ok(0));
        assert_eq!(space.open(Role::Pipe), Ok(5));
        assert_eq!(space.open_count(), SLOT_CAP as u32);
    }

    #[test]
    fn injection_catches_and_conserves() {
        let (ledger, per) = run_fd_matrix();
        assert_eq!(ledger.detects, INJECT_ROUNDS * INJECT_CLASSES as u32);
        assert_eq!(ledger.misses, 0);
        assert!(ledger.conserved());
        assert_eq!(per, [INJECT_ROUNDS, INJECT_ROUNDS, INJECT_ROUNDS]);
        assert_eq!(ledger.per(FdClass::OutOfRange), INJECT_ROUNDS);
        assert_eq!(ledger.per(FdClass::NeverOpened), INJECT_ROUNDS);
        assert_eq!(ledger.per(FdClass::AlreadyClosed), INJECT_ROUNDS);
    }

    #[test]
    fn determinism_and_segment() {
        assert_eq!(patrol_fingerprint(), patrol_fingerprint());
        assert_ne!(patrol_fingerprint(), 0);
        assert_eq!(SEG_BASE, 0x7A4C0);
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x7A000..=0x7D1C0).contains(&SEG_BASE));
    }

    #[test]
    fn recon_charter_and_text() {
        let (ok, total) = reconcile();
        assert_eq!(ok, total);
        assert_eq!(total, 8);
        assert!(charter_closed());
        assert_eq!(CHARTER.len(), 6);
        assert_eq!(FdCode::ALL.len(), 13);
        let mut i = 0usize;
        while let Some(code) = FdCode::ALL.get(i) {
            assert!(!code.detail().is_empty());
            assert!(code.val() >= SEG_BASE && code.val() <= SEG_BASE + 0x3F);
            i += 1;
        }
        assert!(recon_mismatch_text().starts_with("8 行"));
        assert!(charter_summary().contains("对账"));
        assert_eq!(
            judge_order_probe(),
            (FdClass::OutOfRange, FdClass::NeverOpened, FdClass::AlreadyClosed)
        );
    }
}
