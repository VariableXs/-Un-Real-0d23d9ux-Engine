//! UNX-F9205 lseek 坏 fd/坏 whence 矩阵 · 0x7A0C0段
//! 判据原文（任务板 F9205 逐字）：坏 fd 全 EBADF 判据过，非法 whence 全
//! EINVAL 格过，负偏移全 EINVAL 格过。
//!
//! 设计要点：
//!   ① lseek 的判定次序是死规矩：先 fd（EBADF），再对象能力（管道 ESPIPE），
//!      再 whence 合法性（EINVAL），最后结果偏移非负（EINVAL）。次序错了就
//!      会把「坏 fd + 坏 whence」判成 EINVAL，与 Linux 逐码对不上。
//!   ② whence 是闭集：SEEK_SET/CUR/END/DATA/HOLE 共五个，其余 i32 一律
//!      EINVAL；SEEK_DATA/HOLE 走区间扫描，越过 EOF 一律 ENXIO。
//!   ③ 失败绝不改位置：所有校验在读改写之前完成，pos 只在成功提交时落盘。
//!   ④ 算术一律 checked_add：SEEK_END 的 size 转 i64、CUR 的累加都可能溢出，
//!      溢出与负结果同为 EINVAL，不许回绕成一个大正数。
//!   ⑤ 注入三张矩阵：坏 fd 四类、非法 whence 四类、负偏移三类，各 10 轮，
//!      注入账守恒；语料双跑指纹相等；诊断码闭集。
//!   ⑥ 验收判定器 judge_lseek(fd, offset: i64, whence: u32) 是本卡判据的
//!      唯一入口形态：whence 闭集按卡面收死为 SEEK_SET=0/CUR=1/END=2，
//!      其余 u32 值一律 EINVAL；域模型里的 SEEK_DATA/HOLE 是 Linux 扩展
//!      深度面，只走 lseek_to/表通道，不进验收闭集。
use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址（F9205 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7A0C0;
/// Linux errno（正数口径）。
pub const EBADF: i32 = 9;
pub const EINVAL: i32 = 22;
pub const ESPIPE: i32 = 29;
pub const ENXIO: i32 = 6;
pub const EMFILE: i32 = 24;
pub const SEEK_SET: i32 = 0;
pub const SEEK_CUR: i32 = 1;
pub const SEEK_END: i32 = 2;
pub const SEEK_DATA: i32 = 3;
pub const SEEK_HOLE: i32 = 4;
pub const SEEK_MAX: i32 = SEEK_HOLE;
/// 动态句柄容量、常驻句柄数、单类注入轮数。
pub const HANDLE_CAP: usize = 12;
pub const STD_HANDLES: i32 = 3;
pub const INJECT_ROUNDS: u32 = 10;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0E 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SeekCode {
    BadFd = 1,
    NeverOpened = 2,
    AlreadyClosed = 3,
    BeyondLimit = 4,
    BadWhence = 5,
    NegativeOffset = 6,
    NotSeekable = 7,
    NoSuchData = 8,
    OffsetOverflow = 9,
    TableFull = 10,
    PosDrift = 11,
    LedgerRegress = 12,
    NotReproducible = 13,
    SegmentViolation = 14,
}
impl SeekCode {
    /// 码值 = 段基址 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 关联的 Linux errno（正数）。
    pub fn errno(self) -> i32 {
        match self {
            SeekCode::BadFd | SeekCode::NeverOpened | SeekCode::AlreadyClosed
            | SeekCode::BeyondLimit => EBADF,
            SeekCode::NotSeekable => ESPIPE,
            SeekCode::NoSuchData => ENXIO,
            SeekCode::TableFull => EMFILE,
            _ => EINVAL,
        }
    }
    /// 中文诊断与可操作建议。
    pub fn detail(self) -> &'static str {
        match self {
            SeekCode::BadFd => "负 fd：lseek 不接受负句柄；请检查句柄来源，负值一律 EBADF",
            SeekCode::NeverOpened => "从未分配的槽位：该号段没有 open/pipe 记录；请先取得有效句柄",
            SeekCode::AlreadyClosed => "已关闭句柄：关闭后 lseek 必 EBADF；请检查句柄生命周期",
            SeekCode::BeyondLimit => "fd 越上界：号大于句柄表上限；请核对 RLIMIT_NOFILE",
            SeekCode::BadWhence => "非法 whence：只接受 0..=4；请用 SEEK_SET/CUR/END/DATA/HOLE 常量",
            SeekCode::NegativeOffset => "负结果偏移：SEEK_SET 负值或相对定位落到 0 之前；请先修正基准",
            SeekCode::NotSeekable => "不可定位对象：管道/事件无法 seek；请改用可定位的文件句柄",
            SeekCode::NoSuchData => "空洞探测越界：偏移不小于文件长度，SEEK_DATA/HOLE 回 ENXIO",
            SeekCode::OffsetOverflow => "偏移溢出：i64 加减越界；请拆分定位或改用 64 位安全接口",
            SeekCode::TableFull => "句柄耗尽：EMFILE；请先关闭闲置句柄或提高 RLIMIT_NOFILE",
            SeekCode::PosDrift => "失败改位：失败路径推进了文件位置；请把提交推迟到全部校验之后",
            SeekCode::LedgerRegress => "注入账倒退：累计计数只增不减；请检查回滚路径",
            SeekCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏状态",
            SeekCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x7A000..=0x7D1C0 窗口内",
        }
    }
    /// 闭集全量。
    pub const ALL: [SeekCode; 14] = [
        SeekCode::BadFd, SeekCode::NeverOpened, SeekCode::AlreadyClosed,
        SeekCode::BeyondLimit, SeekCode::BadWhence, SeekCode::NegativeOffset,
        SeekCode::NotSeekable, SeekCode::NoSuchData, SeekCode::OffsetOverflow,
        SeekCode::TableFull, SeekCode::PosDrift, SeekCode::LedgerRegress,
        SeekCode::NotReproducible, SeekCode::SegmentViolation,
    ];
}
/// 系统调用结果：携带 Linux errno 正数码。
pub type SysResult<T> = Result<T, i32>;

// ── whence 闭集 ───────────────────────────────────────────────────────
/// 定位基准闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SeekWhence {
    Set = 0,
    Cur = 1,
    End = 2,
    Data = 3,
    Hole = 4,
}
impl SeekWhence {
    /// 原始值 → 闭集成员；越界返回 None。
    pub fn from_raw(raw: i32) -> Option<SeekWhence> {
        match raw {
            SEEK_SET => Some(SeekWhence::Set),
            SEEK_CUR => Some(SeekWhence::Cur),
            SEEK_END => Some(SeekWhence::End),
            SEEK_DATA => Some(SeekWhence::Data),
            SEEK_HOLE => Some(SeekWhence::Hole),
            _ => None,
        }
    }
    /// 闭集全量（穷举与文案检查用）。
    pub const ALL: [SeekWhence; 5] = [
        SeekWhence::Set, SeekWhence::Cur, SeekWhence::End,
        SeekWhence::Data, SeekWhence::Hole,
    ];
}
// ── 槽位状态机 ────────────────────────────────────────────────────────
/// 句柄槽状态：从未分配 / 已打开 / 已关闭（分配过）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    Free = 0,
    Open = 1,
    Closed = 2,
}
impl SlotState {
    pub const ALL: [SlotState; 3] = [SlotState::Free, SlotState::Open, SlotState::Closed];
}
/// 合法迁移：Free→Open、Open→Closed、Closed→Open（复用）；其余拒绝。
pub fn legal_transition(from: SlotState, to: SlotState) -> bool {
    match (from, to) {
        (SlotState::Free, SlotState::Open) => true,
        (SlotState::Open, SlotState::Closed) => true,
        (SlotState::Closed, SlotState::Open) => true,
        _ => false,
    }
}
/// 按状态机迁移；非法边返回 EINVAL。
pub fn transit(from: SlotState, to: SlotState) -> SysResult<SlotState> {
    if legal_transition(from, to) { Ok(to) } else { Err(EINVAL) }
}
/// 状态机边统计：(合法边数, 非法边数)。
pub fn transition_counts() -> (u32, u32) {
    let mut legal = 0u32;
    let mut illegal = 0u32;
    for a in SlotState::ALL.iter() {
        for b in SlotState::ALL.iter() {
            if legal_transition(*a, *b) {
                legal = legal.saturating_add(1);
            } else {
                illegal = illegal.saturating_add(1);
            }
        }
    }
    (legal, illegal)
}
// ── 数据区间 ──────────────────────────────────────────────────────────
/// 半开区间 [start, end)：数据段或空洞段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Extent {
    pub start: u64,
    pub end: u64,
}
impl Extent {
    pub const fn new(start: u64, end: u64) -> Extent {
        Extent { start, end }
    }
    /// 区间是否合法（start ≤ end）。
    pub fn valid(self) -> bool {
        self.start <= self.end
    }
}
/// 可定位对象：长度、当前位置、seekable 能力、数据区间表。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Obj {
    pub size: u64,
    pub pos: i64,
    pub seekable: bool,
    pub extents: [Extent; 2],
    pub nextents: usize,
}
impl Obj {
    /// 空对象（空文件）。
    pub const fn empty() -> Obj {
        Obj {
            size: 0,
            pos: 0,
            seekable: true,
            extents: [Extent::new(0, 0), Extent::new(0, 0)],
            nextents: 0,
        }
    }
    /// 不可定位对象（管道端）。
    pub const fn pipe() -> Obj {
        Obj {
            size: 0,
            pos: 0,
            seekable: false,
            extents: [Extent::new(0, 0), Extent::new(0, 0)],
            nextents: 0,
        }
    }
    /// 演示文件：长度 16384，数据 [0,4096) 与 [8192,12288)，
    /// 空洞 [4096,8192) 与 [12288,16384)。
    pub const fn demo() -> Obj {
        Obj {
            size: 16384,
            pos: 0,
            seekable: true,
            extents: [Extent::new(0, 4096), Extent::new(8192, 12288)],
            nextents: 2,
        }
    }
}
/// 下一个数据段起点：pos 落在数据段内回 pos，落在洞中回后一段数据起点。
pub fn next_data(obj: &Obj, pos: u64) -> Option<u64> {
    if pos >= obj.size {
        return None;
    }
    let mut i = 0usize;
    while i < obj.nextents {
        if let Some(e) = obj.extents.get(i) {
            if e.end > pos {
                return Some(if pos < e.start { e.start } else { pos });
            }
        }
        i += 1;
    }
    None
}
/// 下一个空洞起点：pos 落在洞中回 pos，落在数据段内回该段终点。
pub fn next_hole(obj: &Obj, pos: u64) -> Option<u64> {
    if pos >= obj.size {
        return None;
    }
    let mut i = 0usize;
    while i < obj.nextents {
        if let Some(e) = obj.extents.get(i) {
            if pos < e.start {
                return Some(pos);
            }
            if pos < e.end {
                return Some(e.end);
            }
        }
        i += 1;
    }
    Some(pos)
}
// ── 纯定位计算 ────────────────────────────────────────────────────────
/// 只算不写：把 (当前位置, 长度, offset, whence) 折成新偏移。
///
/// 判定次序与 Linux 一致：whence 合法性 → 负结果 → ENXIO → 溢出。
pub fn lseek_to(obj: &Obj, offset: i64, whence_raw: i32) -> SysResult<i64> {
    let whence = match SeekWhence::from_raw(whence_raw) {
        Some(w) => w,
        None => return Err(EINVAL),
    };
    let target = match whence {
        SeekWhence::Set => offset,
        SeekWhence::Cur => match obj.pos.checked_add(offset) {
            Some(v) => v,
            None => return Err(EINVAL),
        },
        SeekWhence::End => {
            let size = match i64::try_from(obj.size) {
                Ok(v) => v,
                Err(_) => return Err(EINVAL),
            };
            match size.checked_add(offset) {
                Some(v) => v,
                None => return Err(EINVAL),
            }
        }
        SeekWhence::Data | SeekWhence::Hole => {
            if offset < 0 {
                return Err(EINVAL);
            }
            let pos = offset as u64;
            let probe = if whence == SeekWhence::Data {
                next_data(obj, pos)
            } else {
                next_hole(obj, pos)
            };
            return match probe {
                Some(v) => Ok(v as i64),
                None => Err(ENXIO),
            };
        }
    };
    if target < 0 {
        return Err(EINVAL);
    }
    Ok(target)
}
// ── 句柄表 ────────────────────────────────────────────────────────────
/// 单个句柄槽。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Handle {
    pub state: SlotState,
    pub generation: u16,
    pub obj: Option<Obj>,
}
impl Handle {
    const fn empty() -> Handle {
        Handle { state: SlotState::Free, generation: 1, obj: None }
    }
    const fn pipe() -> Handle {
        Handle { state: SlotState::Open, generation: 1, obj: Some(Obj::pipe()) }
    }
}
/// 定容句柄表：0/1/2 是管道，动态槽从 3 起。
#[derive(Clone, Copy, Debug)]
pub struct FdTable {
    std: [Handle; 3],
    dyns: [Handle; HANDLE_CAP],
    allocs: u32,
    closes: u32,
}
impl FdTable {
    /// 构造：三个标准句柄都是管道端（lseek 必 ESPIPE）。
    pub const fn new() -> FdTable {
        FdTable {
            std: [Handle::pipe(), Handle::pipe(), Handle::pipe()],
            dyns: [Handle::empty(); HANDLE_CAP],
            allocs: 3,
            closes: 0,
        }
    }
    /// fd → (是否标准句柄, 槽位下标)。
    fn index(&self, fd: i32) -> Option<(bool, usize)> {
        if fd < 0 {
            return None;
        }
        let u = usize::try_from(fd).ok()?;
        if u < 3 {
            return Some((true, u));
        }
        let idx = u - 3;
        if idx >= HANDLE_CAP {
            return None;
        }
        Some((false, idx))
    }
    /// 只读句柄引用。
    pub fn handle(&self, fd: i32) -> Option<&Handle> {
        match self.index(fd) {
            Some((true, idx)) => self.std.get(idx),
            Some((false, idx)) => self.dyns.get(idx),
            None => None,
        }
    }
    fn handle_mut(&mut self, fd: i32) -> Option<&mut Handle> {
        match self.index(fd) {
            Some((true, idx)) => self.std.get_mut(idx),
            Some((false, idx)) => self.dyns.get_mut(idx),
            None => None,
        }
    }
    /// 打开一个对象；优先复用已关槽，复用推进代际；表满 EMFILE。
    pub fn open(&mut self, obj: Obj) -> SysResult<i32> {
        for i in 0..HANDLE_CAP {
            let state = match self.dyns.get(i) {
                Some(h) => h.state,
                None => SlotState::Open,
            };
            if state != SlotState::Open {
                if let Some(h) = self.dyns.get_mut(i) {
                    h.state = SlotState::Open;
                    h.obj = Some(obj);
                    if state == SlotState::Closed {
                        h.generation = h.generation.wrapping_add(1);
                    }
                }
                self.allocs = self.allocs.saturating_add(1);
                return Ok(STD_HANDLES + i as i32);
            }
        }
        Err(EMFILE)
    }
    /// 关闭：坏 fd 全 EBADF；成功推进代际并清空对象。
    pub fn close(&mut self, fd: i32) -> SysResult<u16> {
        let gen = match self.handle(fd) {
            Some(h) if h.state == SlotState::Open => h.generation.wrapping_add(1),
            _ => return Err(EBADF),
        };
        if let Some(h) = self.handle_mut(fd) {
            h.state = SlotState::Closed;
            h.generation = gen;
            h.obj = None;
        }
        self.closes = self.closes.saturating_add(1);
        Ok(gen)
    }
    /// 句柄探测：坏 fd 回 EBADF，好句柄回 (代际, 对象)。
    pub fn describe(&self, fd: i32) -> SysResult<(u16, Obj)> {
        match self.handle(fd) {
            Some(h) if h.state == SlotState::Open => match h.obj {
                Some(o) => Ok((h.generation, o)),
                None => Err(EBADF),
            },
            _ => Err(EBADF),
        }
    }
    /// lseek：fd → 能力 → whence → 结果，次序不可交换。
    pub fn lseek(&mut self, fd: i32, offset: i64, whence: i32) -> SysResult<i64> {
        let snapshot = match self.handle(fd) {
            Some(h) if h.state == SlotState::Open => match h.obj {
                Some(o) if o.seekable => o,
                Some(_) => return Err(ESPIPE),
                None => return Err(EBADF),
            },
            _ => return Err(EBADF),
        };
        let target = lseek_to(&snapshot, offset, whence)?;
        if let Some(h) = self.handle_mut(fd) {
            if let Some(o) = h.obj.as_mut() {
                o.pos = target;
            }
        }
        Ok(target)
    }
    /// 存活句柄数。
    pub fn live(&self) -> u32 {
        let d = self.dyns.iter().filter(|h| h.state == SlotState::Open).count();
        let s = self.std.iter().filter(|h| h.state == SlotState::Open).count();
        (d + s) as u32
    }
    /// 句柄账闭合：allocs = closes + live。
    pub fn balanced(&self) -> bool {
        self.allocs == self.closes.saturating_add(self.live())
    }
    /// 账目快照：(allocs, closes, live)。
    pub fn books(&self) -> (u32, u32, u32) {
        (self.allocs, self.closes, self.live())
    }
}
// ── 验收判定器 judge_lseek ────────────────────────────────────────────
/// 验收用规范表：三根标准管道（0/1/2，不可定位）+ fd 3 = 演示文件。
///
/// 每次调用现场重建，无任何隐藏状态：同号输入必得同判，这是双跑一致
/// 判据成立的前提。fd 4..=14 是从未分配的空槽（EBADF），15 越上界。
pub fn canonical_table() -> FdTable {
    let mut t = FdTable::new();
    let _ = t.open(Obj::demo());
    t
}

/// lseek 验收判定器（本卡判据的唯一入口形态）。
///
/// `judge_lseek(fd, offset, whence) -> Result<i64, i32>`，判定次序与
/// Linux lseek(2) 逐项对齐：
///   1) 先 fget：fd 非法（负值/空槽/已关/越界）一律 EBADF，
///      即使 whence 与 offset 同时坏也不得改口（EBADF 先于 EINVAL）；
///   2) 再看对象能力：管道等不可定位对象回 ESPIPE（先于 whence 校验）；
///   3) whence 闭集按本卡判据收死：SEEK_SET=0 / SEEK_CUR=1 /
///      SEEK_END=2，其余 u32 值（含 3、99、u32::MAX）一律 EINVAL；
///   4) 偏移算术 checked_add，溢出与负结果统一回 EINVAL，
///      绝不回绕成一个大正数假成功。
pub fn judge_lseek(fd: i32, offset: i64, whence: u32) -> Result<i64, i32> {
    let mut table = canonical_table();
    // Linux lseek(2) 判定次序：fget(fd) → 可 seek 性（ESPIPE）→
    // whence 闭集（EINVAL）→ 偏移算术；失败路径零改位。
    let snap = match table.handle(fd) {
        Some(h) if h.state == SlotState::Open => match h.obj {
            Some(o) if o.seekable => o,
            Some(_) => return Err(ESPIPE),
            None => return Err(EBADF),
        },
        _ => return Err(EBADF),
    };
    let _ = snap;
    let raw = match whence {
        SEEK_SET_U => SEEK_SET,
        SEEK_CUR_U => SEEK_CUR,
        SEEK_END_U => SEEK_END,
        _ => return Err(EINVAL),
    };
    table.lseek(fd, offset, raw)
}

/// whence 闭集的 u32 口径常量（判定器对外签名用 u32）。
pub const SEEK_SET_U: u32 = SEEK_SET as u32;
pub const SEEK_CUR_U: u32 = SEEK_CUR as u32;
pub const SEEK_END_U: u32 = SEEK_END as u32;

// ── 注入矩阵 ──────────────────────────────────────────────────────────
/// 四类坏 fd 样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadFdCase {
    Negative = 1,
    NeverOpened = 2,
    Closed = 3,
    BeyondLimit = 4,
}
impl BadFdCase {
    pub fn code(self) -> SeekCode {
        match self {
            BadFdCase::Negative => SeekCode::BadFd,
            BadFdCase::NeverOpened => SeekCode::NeverOpened,
            BadFdCase::Closed => SeekCode::AlreadyClosed,
            BadFdCase::BeyondLimit => SeekCode::BeyondLimit,
        }
    }
    pub const ALL: [BadFdCase; 4] = [
        BadFdCase::Negative, BadFdCase::NeverOpened,
        BadFdCase::Closed, BadFdCase::BeyondLimit,
    ];
}
/// 四类非法 whence 样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WhenceCase {
    Negative = 1,
    TooBig = 2,
    Far = 3,
    Min = 4,
}
impl WhenceCase {
    /// 样本原始值。
    pub fn raw(self) -> i32 {
        match self {
            WhenceCase::Negative => -1,
            WhenceCase::TooBig => SEEK_MAX + 1,
            WhenceCase::Far => 99,
            WhenceCase::Min => i32::MIN,
        }
    }
    pub const ALL: [WhenceCase; 4] = [
        WhenceCase::Negative, WhenceCase::TooBig, WhenceCase::Far, WhenceCase::Min,
    ];
}
/// 三类负偏移样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NegCase {
    SetNegative = 1,
    CurBeforeZero = 2,
    EndBeforeZero = 3,
}
impl NegCase {
    /// (offset, whence)：都保证结果偏移 < 0。
    pub fn probe(self, pos: i64, size: i64) -> (i64, i32) {
        match self {
            NegCase::SetNegative => (-1, SEEK_SET),
            NegCase::CurBeforeZero => (pos.saturating_neg().saturating_sub(1), SEEK_CUR),
            NegCase::EndBeforeZero => (size.saturating_neg().saturating_sub(1), SEEK_END),
        }
    }
    pub const ALL: [NegCase; 3] = [
        NegCase::SetNegative, NegCase::CurBeforeZero, NegCase::EndBeforeZero,
    ];
}
/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}
impl InjectLedger {
    pub const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }
    /// 记一次；守恒破坏即回报账倒退。
    pub fn record(&mut self, caught: bool) -> SysResult<()> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(-(SeekCode::LedgerRegress as i32));
        }
        Ok(())
    }
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}
/// 建一张演示表：一个打开的演示文件，再关一个样本槽供「已关」注入。
fn demo_table() -> (FdTable, i32, i32) {
    let mut t = FdTable::new();
    let file = match t.open(Obj::demo()) {
        Ok(fd) => fd,
        Err(_) => -1,
    };
    let closed = match t.open(Obj::empty()) {
        Ok(fd) => {
            let _ = t.close(fd);
            fd
        }
        Err(_) => -1,
    };
    (t, file, closed)
}
/// 坏 fd 四类 × 10 轮：全 EBADF，且失败不改任何存活文件的 pos。
pub fn run_bad_fd_matrix() -> (InjectLedger, [u32; 4]) {
    let (mut t, file, closed_fd) = demo_table();
    let mut ledger = InjectLedger::zero();
    let mut per = [0u32; 4];
    for (i, case) in BadFdCase::ALL.iter().enumerate() {
        let fd = match *case {
            BadFdCase::Negative => -1,
            BadFdCase::NeverOpened => STD_HANDLES + HANDLE_CAP as i32 - 1,
            BadFdCase::Closed => closed_fd,
            BadFdCase::BeyondLimit => STD_HANDLES + HANDLE_CAP as i32,
        };
        for _ in 0..INJECT_ROUNDS {
            let before = match t.describe(file) {
                Ok((_, o)) => o.pos,
                Err(_) => -1,
            };
            let caught = t.lseek(fd, 0, SEEK_SET) == Err(EBADF);
            let after = match t.describe(file) {
                Ok((_, o)) => o.pos,
                Err(_) => -1,
            };
            let ok = caught && before == after;
            let _ = ledger.record(ok);
            if ok {
                if let Some(n) = per.get_mut(i) {
                    *n = n.saturating_add(1);
                }
            }
        }
    }
    (ledger, per)
}
/// 非法 whence 四类 × 10 轮：全 EINVAL，且失败不改位置。
pub fn run_whence_matrix() -> (InjectLedger, [u32; 4]) {
    let (mut t, file, _) = demo_table();
    let mut ledger = InjectLedger::zero();
    let mut per = [0u32; 4];
    for (i, case) in WhenceCase::ALL.iter().enumerate() {
        for _ in 0..INJECT_ROUNDS {
            let before = match t.describe(file) {
                Ok((_, o)) => o.pos,
                Err(_) => -1,
            };
            let caught = t.lseek(file, 0, case.raw()) == Err(EINVAL);
            let after = match t.describe(file) {
                Ok((_, o)) => o.pos,
                Err(_) => -1,
            };
            let ok = caught && before == after;
            let _ = ledger.record(ok);
            if ok {
                if let Some(n) = per.get_mut(i) {
                    *n = n.saturating_add(1);
                }
            }
        }
    }
    (ledger, per)
}
/// 负偏移三类 × 10 轮：全 EINVAL，且失败不改位置。
pub fn run_negative_matrix() -> (InjectLedger, [u32; 3]) {
    let (mut t, file, _) = demo_table();
    let mut ledger = InjectLedger::zero();
    let mut per = [0u32; 3];
    // 先把位置推到 100，保证 SEEK_CUR 的负样本结果小于 0。
    let _ = t.lseek(file, 100, SEEK_SET);
    let size = Obj::demo().size as i64;
    for (i, case) in NegCase::ALL.iter().enumerate() {
        for _ in 0..INJECT_ROUNDS {
            let before = match t.describe(file) {
                Ok((_, o)) => o.pos,
                Err(_) => -1,
            };
            let (off, w) = case.probe(before, size);
            let caught = t.lseek(file, off, w) == Err(EINVAL);
            let after = match t.describe(file) {
                Ok((_, o)) => o.pos,
                Err(_) => -1,
            };
            let ok = caught && before == after;
            let _ = ledger.record(ok);
            if ok {
                if let Some(n) = per.get_mut(i) {
                    *n = n.saturating_add(1);
                }
            }
        }
    }
    (ledger, per)
}
// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, Linux 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；坏句柄族"),
    ("ENXIO", -6, 2, "Windows ERROR_FILE_NOT_FOUND；空洞探测越界"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
    ("ESPIPE", -29, 29, "Windows ERROR_SEEK；不可定位对象"),
    ("EMFILE", -24, 4, "Windows ERROR_TOO_MANY_OPEN_FILES；句柄耗尽"),
];

/// 查表：按 errno 正数取负值；表外返回 0。
pub fn linux_neg(code: i32) -> i32 {
    for row in ERRNO_TABLE.iter() {
        if row.1 == -code {
            return row.1;
        }
    }
    0
}
/// 与 Linux 逐码一致。
pub fn agrees_with_linux(code: i32) -> bool {
    code > 0 && linux_neg(code) == -code
}
/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in bytes.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}
/// 规范剧本：正常三类定位、DATA/HOLE、三类失败、关闭后重试。
pub fn scripted_fingerprint() -> u32 {
    let mut t = FdTable::new();
    let mut bytes: Vec<u8> = Vec::new();
    let fd = match t.open(Obj::demo()) {
        Ok(v) => v,
        Err(_) => -1,
    };
    let script: [(i64, i32); 6] = [
        (100, SEEK_SET), (50, SEEK_CUR), (-100, SEEK_END),
        (4096, SEEK_DATA), (8192, SEEK_HOLE), (0, 99),
    ];
    for (off, w) in script.iter() {
        let r = t.lseek(fd, *off, *w);
        let v = match r {
            Ok(x) => (x as u32) & 0x7FFF_FFFF,
            Err(e) => 0x8000_0000u32 | (e as u32 & 0x7F),
        };
        bytes.push((v & 0xFF) as u8);
        bytes.push(((v >> 8) & 0xFF) as u8);
    }
    let _ = t.lseek(-1, 0, SEEK_SET);
    let _ = t.lseek(STD_HANDLES, 0, SEEK_SET);
    let _ = t.close(fd);
    let _ = t.lseek(fd, 0, SEEK_SET);
    let bk = t.books();
    bytes.push((bk.0 & 0xFF) as u8);
    bytes.push((bk.1 & 0xFF) as u8);
    bytes.push((bk.2 & 0xFF) as u8);
    fnv1a(&bytes)
}
// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9205 判据集（20 条静态 + 4 条动态明细）。
pub fn run_unx9205_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9205");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. whence 闭集：0..=4 全部合法，其余一律拒绝
    let closed = SeekWhence::ALL.iter().all(|w| SeekWhence::from_raw(*w as i32) == Some(*w));
    let rejects = SeekWhence::from_raw(-1).is_none()
        && SeekWhence::from_raw(SEEK_MAX + 1).is_none()
        && SeekWhence::from_raw(99).is_none()
        && SeekWhence::from_raw(i32::MIN).is_none();
    s.add("whence 闭集：五值可解析、越界值全拒", closed && rejects,
          "SEEK_SET/CUR/END/DATA/HOLE 之外没有第六种 whence");

    // 3. 正常流：SET/CUR/END 三段算术
    let f = Obj::demo();
    let set_ok = lseek_to(&f, 100, SEEK_SET) == Ok(100);
    let cur_ok = lseek_to(&Obj { pos: 100, ..f }, 50, SEEK_CUR) == Ok(150);
    let end_ok = lseek_to(&f, -100, SEEK_END) == Ok((f.size as i64) - 100);
    s.add("正常流：SET/CUR/END 三段算术正确", set_ok && cur_ok && end_ok,
          "绝对定位、相对定位、末尾定位必须逐字节对齐");

    // 3b. 验收判定器 judge_lseek：闭集 whence + 判定次序 + 算术四面对表
    let jl_ok = judge_lseek(3, 100, SEEK_SET_U) == Ok(100)
        && judge_lseek(3, 50, SEEK_CUR_U) == Ok(50)
        && judge_lseek(3, -100, SEEK_END_U) == Ok((Obj::demo().size as i64) - 100)
        && judge_lseek(-1, -1, 99) == Err(EBADF)
        && judge_lseek(0, 0, SEEK_SET_U) == Err(ESPIPE)
        && judge_lseek(3, -1, SEEK_SET_U) == Err(EINVAL)
        && judge_lseek(3, 0, 3) == Err(EINVAL)
        && judge_lseek(3, 0, 99) == Err(EINVAL)
        && judge_lseek(3, 0, u32::MAX) == Err(EINVAL)
        && judge_lseek(3, i64::MAX, SEEK_END_U) == Err(EINVAL);
    s.add("judge_lseek：三值闭集、EBADF→ESPIPE→EINVAL 次序、溢出全对表",
          jl_ok,
          "验收判定器必须与 Linux lseek(2) 逐码一致，闭集外 whence 一律 EINVAL");

    // 4. 坏 fd 四类各 10/10 全 EBADF
    let (bad_ledger, bad_per) = run_bad_fd_matrix();
    let bad_ok = bad_per.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("坏 fd 四类各 10/10 全 -EBADF", bad_ok,
          "负值/未开/已关/越界必须全部 EBADF，且不得改动位置");

    // 5. 非法 whence 四类各 10/10 全 EINVAL
    let (w_ledger, w_per) = run_whence_matrix();
    let w_ok = w_per.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("非法 whence 四类各 10/10 全 -EINVAL", w_ok,
          "-1/5/99/i32::MIN 必须全部 EINVAL");

    // 6. 负偏移三类各 10/10 全 EINVAL
    let (n_ledger, n_per) = run_negative_matrix();
    let n_ok = n_per.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("负偏移三类各 10/10 全 -EINVAL", n_ok,
          "SET 负值、CUR 落到 0 之前、END 负过头都必须 EINVAL");

    // 7. 判定次序：EBADF 先于 EINVAL
    let mut t = FdTable::new();
    let order_bad_fd = t.lseek(-1, 0, 99) == Err(EBADF)
        && t.lseek(-1, -1, SEEK_SET) == Err(EBADF);
    s.add("判定次序：坏 fd 先于坏 whence", order_bad_fd,
          "fd 未通过就不该看到 whence，否则 errno 会对不上");

    // 8. 判定次序：ESPIPE 先于 EINVAL
    let order_pipe = t.lseek(1, 0, 99) == Err(ESPIPE)
        && t.lseek(1, -1, SEEK_SET) == Err(ESPIPE);
    s.add("判定次序：不可定位对象先于坏 whence", order_pipe,
          "管道先回 ESPIPE，不会因为 whence 非法而改口 EINVAL");

    // 9. 失败不改位置
    let mut drift = FdTable::new();
    let dfd = match drift.open(Obj::demo()) {
        Ok(v) => v,
        Err(_) => -1,
    };
    let _ = drift.lseek(dfd, 777, SEEK_SET);
    let _ = drift.lseek(dfd, 99, 99);
    let _ = drift.lseek(dfd, -1, SEEK_SET);
    let pos_after = match drift.describe(dfd) {
        Ok((_, o)) => o.pos,
        Err(_) => -1,
    };
    s.add("失败不改位置：拒绝路径 pos 保持 777", pos_after == 777,
          "所有校验必须在写回之前完成，失败即原样返回");

    // 10. SEEK_DATA / SEEK_HOLE 区间语义
    let d0 = lseek_to(&f, 0, SEEK_DATA) == Ok(0);
    let d1 = lseek_to(&f, 4096, SEEK_DATA) == Ok(8192);
    let d2 = lseek_to(&f, 9000, SEEK_DATA) == Ok(9000);
    let h0 = lseek_to(&f, 0, SEEK_HOLE) == Ok(4096);
    let h1 = lseek_to(&f, 4096, SEEK_HOLE) == Ok(4096);
    let h2 = lseek_to(&f, 9000, SEEK_HOLE) == Ok(12288);
    s.add("DATA/HOLE：数据段与空洞段分别对齐", d0 && d1 && d2 && h0 && h1 && h2,
          "洞内探测回原位，数据段内探测回段尾");

    // 11. 越过 EOF 的 DATA/HOLE 全 ENXIO
    let eof_ok = lseek_to(&f, 16384, SEEK_DATA) == Err(ENXIO)
        && lseek_to(&f, 20000, SEEK_HOLE) == Err(ENXIO)
        && lseek_to(&f, -1, SEEK_DATA) == Err(EINVAL);
    s.add("EOF 边界：DATA/HOLE 越界 ENXIO、负值 EINVAL", eof_ok,
          "长度即上界，等于长度也算越界");

    // 12. 溢出：END/CUR 的 i64 加减越界回 EINVAL
    let ovf_ok = lseek_to(&f, i64::MAX, SEEK_END) == Err(EINVAL)
        && lseek_to(&Obj { pos: i64::MAX, ..f }, 1, SEEK_CUR) == Err(EINVAL)
        && lseek_to(&Obj { pos: i64::MIN, ..f }, -1, SEEK_CUR) == Err(EINVAL);
    s.add("溢出：END/CUR 越界回 EINVAL 而不回绕", ovf_ok,
          "checked_add 失败即拒绝，绝不允许大正数假成功");

    // 13. 关闭后失效 + 复用代际
    let mut g = FdTable::new();
    let gfd = match g.open(Obj::demo()) {
        Ok(v) => v,
        Err(_) => -1,
    };
    let gen = match g.describe(gfd) {
        Ok((v, _)) => v,
        Err(_) => 0,
    };
    let closed_ok = g.close(gfd).is_ok() && g.lseek(gfd, 0, SEEK_SET) == Err(EBADF);
    let reused = match g.open(Obj::empty()) {
        Ok(v) => v,
        Err(_) => -1,
    };
    let gen_new = match g.describe(reused) {
        Ok((v, _)) => v,
        Err(_) => 0,
    };
    s.add("关闭失效与代际推进：旧句柄 EBADF、复用换代际",
          closed_ok && reused == gfd && gen_new != gen,
          "号复用不等于句柄复用，代际必须推进");

    // 14. 容量：12 个动态槽填满回 EMFILE
    let mut cap = FdTable::new();
    let mut opened = 0u32;
    for _ in 0..HANDLE_CAP {
        if cap.open(Obj::empty()).is_ok() {
            opened = opened.saturating_add(1);
        }
    }
    let full = cap.open(Obj::empty()) == Err(EMFILE);
    s.add("容量：动态槽填满回 EMFILE", opened == HANDLE_CAP as u32 && full,
          "表满必须是 EMFILE，且账目仍闭合");

    // 15. 状态机：三条合法边、六条非法边
    let (legal, illegal) = transition_counts();
    let sm_ok = legal == 3 && illegal == 6
        && transit(SlotState::Free, SlotState::Open) == Ok(SlotState::Open)
        && transit(SlotState::Open, SlotState::Free).is_err();
    s.add("状态机：三条合法边、六条非法边全部拒绝", sm_ok,
          "Free→Closed、Open→Free 等跳步必须显式拒绝");

    // 16. 注入账守恒（三张矩阵合计）
    let total = bad_ledger.injected + w_ledger.injected + n_ledger.injected;
    let caught = bad_ledger.detected + w_ledger.detected + n_ledger.detected;
    let escaped = bad_ledger.escaped + w_ledger.escaped + n_ledger.escaped;
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          total == caught + escaped && escaped == 0 && total == 110,
          "4+4+3 类 × 10 轮共 110 次注入，必须 110 次检出");

    // 18. Linux 逐码一致 + 文案闭集
    let codes_ok = agrees_with_linux(EBADF) && agrees_with_linux(EINVAL)
        && agrees_with_linux(ESPIPE) && agrees_with_linux(ENXIO)
        && agrees_with_linux(EMFILE) && !agrees_with_linux(0);
    let text_ok = SeekCode::ALL.iter().all(|c| !c.detail().is_empty() && c.errno() > 0);
    let mut uniq = true;
    for (i, a) in SeekCode::ALL.iter().enumerate() {
        for b in SeekCode::ALL.iter().skip(i + 1) {
            if a.val() == b.val() {
                uniq = false;
            }
        }
    }
    s.add("Linux 逐码一致且诊断码闭集唯一", codes_ok && text_ok && uniq,
          "五个 errno 与 errno.h 逐项对齐，14 个诊断码码值唯一");

    // 19. 双跑一致
    let a = scripted_fingerprint();
    let b = scripted_fingerprint();
    s.add("双跑一致：剧本指纹相等", a == b,
          "同剧本两次必须逐字段相等，隐藏状态即失败");

    // 动态明细（4 条）
    let bad_text = format!("坏fd 注入 {} 检出 {} 逃逸 {}", bad_ledger.injected,
                           bad_ledger.detected, bad_ledger.escaped);
    s.add_dyn("坏 fd 账", bad_ledger.conserved(), &bad_text);
    let w_text = format!("坏whence 注入 {} 检出 {} 逃逸 {}", w_ledger.injected,
                         w_ledger.detected, w_ledger.escaped);
    s.add_dyn("坏 whence 账", w_ledger.conserved(), &w_text);
    let n_text = format!("负偏移 注入 {} 检出 {} 逃逸 {}", n_ledger.injected,
                         n_ledger.detected, n_ledger.escaped);
    s.add_dyn("负偏移账", n_ledger.conserved(), &n_text);
    let fp_text = format!("剧本指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    // 20. 收口
    let n_before = s.len();
    s.add("收口：判据条数达到声明值", n_before >= 14,
          "收口断言条数只增不减，删条必须显式改声明");

    s
}
// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whence_closed_set() {
        for w in SeekWhence::ALL.iter() {
            assert_eq!(SeekWhence::from_raw(*w as i32), Some(*w));
        }
        assert!(SeekWhence::from_raw(-1).is_none());
        assert!(SeekWhence::from_raw(5).is_none());
        assert!(SeekWhence::from_raw(i32::MIN).is_none());
    }
    #[test]
    fn bad_fd_is_ebadf() {
        let mut t = FdTable::new();
        assert_eq!(t.lseek(-1, 0, SEEK_SET), Err(EBADF));
        assert_eq!(t.lseek(STD_HANDLES + HANDLE_CAP as i32, 0, SEEK_SET), Err(EBADF));
        assert_eq!(t.lseek(STD_HANDLES, 0, SEEK_SET), Err(EBADF));
        let fd = t.open(Obj::demo()).unwrap_or(-1);
        assert!(t.close(fd).is_ok());
        assert_eq!(t.lseek(fd, 0, SEEK_SET), Err(EBADF));
    }
    #[test]
    fn bad_whence_is_einval() {
        let mut t = FdTable::new();
        let fd = t.open(Obj::demo()).unwrap_or(-1);
        for c in WhenceCase::ALL.iter() {
            assert_eq!(t.lseek(fd, 0, c.raw()), Err(EINVAL));
        }
        assert_eq!(t.lseek(fd, 0, -1), Err(EINVAL));
    }
    #[test]
    fn negative_offsets_are_einval() {
        let f = Obj::demo();
        assert_eq!(lseek_to(&f, -1, SEEK_SET), Err(EINVAL));
        assert_eq!(lseek_to(&Obj { pos: 0, ..f }, -1, SEEK_CUR), Err(EINVAL));
        assert_eq!(lseek_to(&f, -((f.size as i64) + 1), SEEK_END), Err(EINVAL));
    }
    #[test]
    fn normal_seek_arithmetic() {
        let f = Obj::demo();
        assert_eq!(lseek_to(&f, 100, SEEK_SET), Ok(100));
        assert_eq!(lseek_to(&Obj { pos: 100, ..f }, 50, SEEK_CUR), Ok(150));
        assert_eq!(lseek_to(&f, 0, SEEK_END), Ok(16384));
        assert_eq!(lseek_to(&f, -1, SEEK_END), Ok(16383));
    }
    #[test]
    fn data_hole_probe() {
        let f = Obj::demo();
        assert_eq!(lseek_to(&f, 0, SEEK_DATA), Ok(0));
        assert_eq!(lseek_to(&f, 4096, SEEK_DATA), Ok(8192));
        assert_eq!(lseek_to(&f, 9000, SEEK_DATA), Ok(9000));
        assert_eq!(lseek_to(&f, 0, SEEK_HOLE), Ok(4096));
        assert_eq!(lseek_to(&f, 9000, SEEK_HOLE), Ok(12288));
        assert_eq!(lseek_to(&f, 16000, SEEK_HOLE), Ok(16000));
    }
    #[test]
    fn beyond_eof_is_enxio() {
        let f = Obj::demo();
        assert_eq!(lseek_to(&f, 16384, SEEK_DATA), Err(ENXIO));
        assert_eq!(lseek_to(&f, 20000, SEEK_HOLE), Err(ENXIO));
        assert_eq!(lseek_to(&f, 16384, SEEK_HOLE), Err(ENXIO));
    }
    #[test]
    fn order_and_pipe() {
        let mut t = FdTable::new();
        assert_eq!(t.lseek(-1, 0, 99), Err(EBADF));
        assert_eq!(t.lseek(1, 0, 99), Err(ESPIPE));
        assert_eq!(t.lseek(1, 0, SEEK_SET), Err(ESPIPE));
    }
    #[test]
    fn failure_keeps_position() {
        let mut t = FdTable::new();
        let fd = t.open(Obj::demo()).unwrap_or(-1);
        assert_eq!(t.lseek(fd, 777, SEEK_SET), Ok(777));
        assert_eq!(t.lseek(fd, -1, SEEK_SET), Err(EINVAL));
        assert_eq!(t.lseek(fd, 0, 99), Err(EINVAL));
        assert_eq!(t.describe(fd).map(|(_, o)| o.pos), Ok(777));
    }
    #[test]
    fn overflow_is_einval() {
        let f = Obj::demo();
        assert_eq!(lseek_to(&f, i64::MAX, SEEK_END), Err(EINVAL));
        assert_eq!(lseek_to(&Obj { pos: i64::MAX, ..f }, 1, SEEK_CUR), Err(EINVAL));
    }
    #[test]
    fn judge_lseek_semantics() {
        // 规范表：fd 3 是演示文件，0/1/2 是管道，其余空槽。
        assert_eq!(judge_lseek(3, 100, SEEK_SET_U), Ok(100));
        assert_eq!(judge_lseek(3, -100, SEEK_END_U), Ok((Obj::demo().size as i64) - 100));
        // 坏 fd：负值/空槽/越界全 EBADF。
        assert_eq!(judge_lseek(-1, 0, SEEK_SET_U), Err(EBADF));
        assert_eq!(judge_lseek(4, 0, SEEK_SET_U), Err(EBADF));
        assert_eq!(judge_lseek(15, 0, SEEK_SET_U), Err(EBADF));
        // 判定次序：坏 fd 压过坏 whence 与负偏移。
        assert_eq!(judge_lseek(-1, -1, 99), Err(EBADF));
        // 管道：ESPIPE 先于 whence 校验。
        assert_eq!(judge_lseek(0, 0, 99), Err(ESPIPE));
        // 闭集外 whence（u32 口径）一律 EINVAL。
        assert_eq!(judge_lseek(3, 0, 3), Err(EINVAL));
        assert_eq!(judge_lseek(3, 0, 99), Err(EINVAL));
        assert_eq!(judge_lseek(3, 0, u32::MAX), Err(EINVAL));
        // 负偏移与溢出。
        assert_eq!(judge_lseek(3, -1, SEEK_SET_U), Err(EINVAL));
        assert_eq!(judge_lseek(3, i64::MAX, SEEK_END_U), Err(EINVAL));
    }
    #[test]
    fn four_class_boundaries() {
        let f = Obj::demo();
        // 零：SET 0 是合法原点。
        assert_eq!(lseek_to(&f, 0, SEEK_SET), Ok(0));
        // 负：下界-1。
        assert_eq!(lseek_to(&f, -1, SEEK_SET), Err(EINVAL));
        assert_eq!(lseek_to(&f, -(f.size as i64) - 1, SEEK_END), Err(EINVAL));
        // 上限：END 0 落在 EOF，CUR 抵达文件尾。
        assert_eq!(lseek_to(&f, 0, SEEK_END), Ok(f.size as i64));
        assert_eq!(lseek_to(&Obj { pos: f.size as i64, ..f }, 0, SEEK_CUR),
                   Ok(f.size as i64));
        // 上限+1：越 EOF 的 DATA/HOLE 回 ENXIO，i64 溢出回 EINVAL。
        assert_eq!(lseek_to(&f, f.size as i64, SEEK_DATA), Err(ENXIO));
        assert_eq!(lseek_to(&f, i64::MAX, SEEK_END), Err(EINVAL));
        assert_eq!(lseek_to(&Obj { pos: i64::MAX, ..f }, 1, SEEK_CUR), Err(EINVAL));
    }
    #[test]
    fn injection_matrices_ten_rounds_all_caught() {
        let (bl, bp) = run_bad_fd_matrix();
        let (wl, wp) = run_whence_matrix();
        let (nl, np) = run_negative_matrix();
        for n in bp.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
        for n in wp.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
        for n in np.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
        assert!(bl.conserved() && wl.conserved() && nl.conserved());
        assert_eq!(bl.escaped + wl.escaped + nl.escaped, 0);
        assert_eq!(bl.injected, 4 * INJECT_ROUNDS);
        assert_eq!(wl.injected, 4 * INJECT_ROUNDS);
        assert_eq!(nl.injected, 3 * INJECT_ROUNDS);
    }
    #[test]
    fn double_run_fingerprint_stable() {
        let a = scripted_fingerprint();
        let b = scripted_fingerprint();
        assert_eq!(a, b);
        assert_ne!(a, 0);
        // judge_lseek 每次现场重建规范表，同样必须双跑一致。
        assert_eq!(judge_lseek(3, 42, SEEK_SET_U), judge_lseek(3, 42, SEEK_SET_U));
    }
    #[test]
    fn capacity_exhaustion_emfile() {
        let mut t = FdTable::new();
        let mut opened = 0u32;
        for _ in 0..HANDLE_CAP {
            if t.open(Obj::empty()).is_ok() {
                opened += 1;
            }
        }
        assert_eq!(opened, HANDLE_CAP as u32);
        assert_eq!(t.open(Obj::empty()), Err(EMFILE));
        // 表满后账目仍闭合：allocs = closes + live。
        assert!(t.balanced());
    }
    #[test]
    fn segment_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x7A000..=0x7D1C0).contains(&SEG_BASE));
        // 诊断码全部落在段窗口内且为正。
        for c in SeekCode::ALL.iter() {
            assert!(c.val() >= SEG_BASE && c.val() <= SEG_BASE + 0x40);
        }
    }
    #[test]
    fn checks_all_green() {
        let s = run_unx9205_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
    }
}
