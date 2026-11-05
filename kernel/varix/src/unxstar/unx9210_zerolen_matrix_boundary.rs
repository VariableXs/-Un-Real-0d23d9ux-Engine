//! UNX-F9210 文件族零长度读写边界矩阵 · 0x7A200段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9210 逐字）：
//!   ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9210-J1 零长度 read/write
//!   正判据（零副作用返回 0）判据过，反向判序（坏 fd 优先于零长度）格过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 「零长度一律返回 0」是错的。目录的 read 落到 generic_read_dir()，
//!      即便 count==0 也直接 -EISDIR；eventfd / timerfd 的「count < 8 →
//!      -EINVAL」尺寸闸同样排在零长度短路之前。两族必须与普通文件分列
//!      成格，否则 J1 的「零副作用返回 0」会被误扩成全族公理。
//!   ② 反向判序是本卡第二条主判据：fd 未开、方向不符必须在 count==0 之前
//!      判掉。open(O_RDONLY) 的句柄上 write(fd,p,0) 在 Linux 是 -EBADF 而非
//!      0——方向闸先于零长度。
//!   ③ 零副作用做成可观测断言：零长度调用后偏移 pos、长度 size、累计搬运
//!      字节 moved 三相冻结，且探针指针取 NULL 也不得被解引用。
//!   ④ 位置状态机只有 Idle→Frozen / Idle→Advanced / Frozen→Advanced 三条
//!      合法边；已 Advanced 再回 Frozen 属越级倒退，直接拒绝。
//!   ⑤ 上限口径用 MAX_RW_COUNT（0x7FFFF000）：count 从 0x7FFFF001 起钳到
//!      0x7FFFF000，钳位在零长度之后、真实搬运之前，0 与 1 的分界仍清晰。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9210 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7A200;
/// 标准流数量：本模块只建模动态槽，0/1/2 一律按 EBADF 看待。
pub const STD_FDS: i32 = 3;
/// 动态槽数量（fd 号 3..=10）。
pub const DYN_SLOTS: usize = 8;
/// Linux 单次读写搬运上限（MAX_RW_COUNT）。
pub const MAX_RW_COUNT: u32 = 0x7FFF_F000;
/// eventfd / timerfd 的最小读写尺寸。
pub const MIN_EVENT_BYTES: u32 = 8;
/// 管道剩余容量。
pub const PIPE_CAP: u32 = 64;
/// 单类注入的复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 坏文件描述符。
pub const EBADF: u32 = 9;
/// 非阻塞且无空间。
pub const EAGAIN: u32 = 11;
/// 用户缓冲区不可访问。
pub const EFAULT: u32 = 14;
/// 目录上发起 read。
pub const EISDIR: u32 = 21;
/// 参数非法（尺寸闸 / 状态机越级）。
pub const EINVAL: u32 = 22;
/// fd 表耗尽。
pub const EMFILE: u32 = 24;
/// 偏移或长度溢出。
pub const EOVERFLOW: u32 = 75;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x00..+0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZlCode {
    BadFdBeforeZero,
    DirZeroRead,
    SpecialMinSize,
    DirectionGate,
    FrozenSideEffect,
    CountClamp,
    AppendFollowsTail,
    OffsetOverflow,
    PosRegress,
    MatrixMismatch,
    LedgerRegress,
    NotReproducible,
}

impl ZlCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ZlCode::BadFdBeforeZero => "坏 fd 先于零长度：未开/已关/越界 fd 上 count==0 也必须回 EBADF；请先确认句柄有效",
            ZlCode::DirZeroRead => "目录零长读回 EISDIR：read 走 generic_read_dir，与 count 无关；列目录请改用 getdents64",
            ZlCode::SpecialMinSize => "eventfd/timerfd 尺寸闸：count 小于 8 字节即 EINVAL，零长度同样拒；请按 8 字节对齐读写",
            ZlCode::DirectionGate => "方向闸：只读句柄写或只写句柄读一律 EBADF；请按 open 的模式位选择读写方向",
            ZlCode::FrozenSideEffect => "零副作用被破：零长度后偏移/长度/搬运账必须三相冻结；请排查无条件推进 offset 的路径",
            ZlCode::CountClamp => "上限钳位：count 超 0x7FFFF000 按 MAX_RW_COUNT 截断；勿按请求长度回填返回值",
            ZlCode::AppendFollowsTail => "追加写偏移：O_APPEND 下写偏移从文件尾起算，与当前 pos 无关；请以文件尾复算 pos_after",
            ZlCode::OffsetOverflow => "偏移溢出：pos+len 或 size+len 越过 u64 上界回 EOVERFLOW；请用 checked_add 复算",
            ZlCode::PosRegress => "位置状态机越级：已 Advanced 的一轮内不得回到 Frozen；零长度不得回拉已推进偏移",
            ZlCode::MatrixMismatch => "矩阵逐格不符：文件族 × 方向 × 零长度的期望与实测不一致；请核对偏离格",
            ZlCode::LedgerRegress => "注入账倒退：注入=检出+逃逸，累计值只增不减；请检查回滚路径误改累计值",
            ZlCode::NotReproducible => "双跑不一致：同输入两次指纹不同；请排查隐藏状态或非确定遍历序",
        }
    }

    /// 闭集全量（文案完整性检查用）。
    pub const ALL: [ZlCode; 12] = [
        ZlCode::BadFdBeforeZero, ZlCode::DirZeroRead, ZlCode::SpecialMinSize,
        ZlCode::DirectionGate, ZlCode::FrozenSideEffect, ZlCode::CountClamp,
        ZlCode::AppendFollowsTail, ZlCode::OffsetOverflow, ZlCode::PosRegress,
        ZlCode::MatrixMismatch, ZlCode::LedgerRegress, ZlCode::NotReproducible,
    ];
}

// ── 文件族与模式位 ────────────────────────────────────────────────────
/// 文件族（只取零长度语义有分叉的族）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileKind {
    Regular,
    Directory,
    Pipe,
    EventFd,
    TimerFd,
    Tty,
    Socket,
    OPath,
}

/// 读方向位。
pub const MODE_READ: u32 = 1;
/// 写方向位。
pub const MODE_WRITE: u32 = 2;
/// 追加写位。
pub const MODE_APPEND: u32 = 4;
/// 非阻塞位。
pub const MODE_NONBLOCK: u32 = 8;

/// 读写方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Read,
    Write,
}

impl Dir {
    /// 该方向需要的模式位。
    pub fn need(self) -> u32 {
        match self {
            Dir::Read => MODE_READ,
            Dir::Write => MODE_WRITE,
        }
    }
}

// ── 探针指针类别 ──────────────────────────────────────────────────────
/// 用户缓冲区类别（零长度时一律不得被触碰）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PtrClass {
    Valid,
    Null,
    Unmapped,
    ReadOnly,
    Kernel,
}

impl PtrClass {
    /// 该类别在某方向上是否可用。
    pub fn allows(self, dir: Dir) -> bool {
        match (self, dir) {
            (PtrClass::Valid, _) => true,
            (PtrClass::ReadOnly, Dir::Read) => true,
            _ => false,
        }
    }
}

// ── 槽位与文件表 ──────────────────────────────────────────────────────
/// 单个 fd 槽。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Slot {
    pub alive: bool,
    pub kind: FileKind,
    pub mode: u32,
    pub pos: u64,
    pub size: u64,
    pub gen: u16,
    pub pipe_free: u32,
    pub moved: u64,
}

impl Slot {
    /// 空槽（const）。
    pub const fn empty() -> Slot {
        Slot { alive: false, kind: FileKind::Regular, mode: 0, pos: 0, size: 0,
               gen: 1, pipe_free: PIPE_CAP, moved: 0 }
    }

    /// 构造已打开槽。
    pub fn opened(kind: FileKind, mode: u32, size: u64) -> Slot {
        let mut s = Slot::empty();
        s.alive = true;
        s.kind = kind;
        s.mode = mode;
        s.size = size;
        s
    }

    /// 是否追加写。
    pub fn append(&self) -> bool {
        self.mode & MODE_APPEND != 0
    }
}
/// 定容文件表：0/1/2 不建模，动态槽 3..=10。
#[derive(Clone, Debug)]
pub struct FxTable {
    slots: Vec<Slot>,
    allocs: u32,
    closes: u32,
    emfile_hits: u32,
}

impl FxTable {
    /// 空表。
    pub fn new() -> FxTable {
        let mut slots: Vec<Slot> = Vec::new();
        let mut i = 0usize;
        while i < DYN_SLOTS {
            slots.push(Slot::empty());
            i += 1;
        }
        FxTable { slots, allocs: 0, closes: 0, emfile_hits: 0 }
    }

    /// fd → 槽下标：负数、标准流、越界统一 EBADF（第一层判定）。
    fn index(&self, fd: i32) -> Result<usize, u32> {
        if fd < STD_FDS {
            return Err(EBADF);
        }
        let idx = (fd - STD_FDS) as usize;
        if idx >= DYN_SLOTS {
            return Err(EBADF);
        }
        Ok(idx)
    }

    /// 取槽（只读）。alive=false 回 EBADF。
    pub fn get(&self, fd: i32) -> Result<&Slot, u32> {
        let idx = self.index(fd)?;
        match self.slots.get(idx) {
            Some(slot) if slot.alive => Ok(slot),
            _ => Err(EBADF),
        }
    }

    /// 取槽（可变）。
    pub fn get_mut(&mut self, fd: i32) -> Result<&mut Slot, u32> {
        let idx = self.index(fd)?;
        match self.slots.get_mut(idx) {
            Some(slot) if slot.alive => Ok(slot),
            _ => Err(EBADF),
        }
    }

    /// 安装文件，优先复用最小空闲槽（Linux 取最低可用 fd）。
    pub fn install(&mut self, kind: FileKind, mode: u32) -> Result<i32, u32> {
        let mut i = 0usize;
        while i < DYN_SLOTS {
            let free = match self.slots.get(i) {
                Some(slot) => !slot.alive,
                None => false,
            };
            if free {
                if let Some(slot) = self.slots.get_mut(i) {
                    slot.alive = true;
                    slot.kind = kind;
                    slot.mode = mode;
                    slot.pos = 0;
                    slot.size = 0;
                    slot.pipe_free = PIPE_CAP;
                    slot.moved = 0;
                    slot.gen = slot.gen.wrapping_add(1);
                }
                self.allocs = self.allocs.saturating_add(1);
                return Ok(STD_FDS + i as i32);
            }            i += 1;
        }
        self.emfile_hits = self.emfile_hits.saturating_add(1);
        Err(EMFILE)
    }

    /// 关闭：推进代际、清模式位。
    pub fn close(&mut self, fd: i32) -> Result<u16, u32> {
        let idx = self.index(fd)?;
        let gen = match self.slots.get(idx) {
            Some(slot) if slot.alive => slot.gen,
            _ => return Err(EBADF),
        };
        let next = gen.wrapping_add(1);
        if let Some(slot) = self.slots.get_mut(idx) {
            slot.alive = false;
            slot.mode = 0;
            slot.kind = FileKind::Regular;
            slot.gen = next;
        }
        self.closes = self.closes.saturating_add(1);
        Ok(next)
    }

    /// 当前打开槽数 / 安装账 / 关闭账 / 表满拒绝账。
    pub fn open_count(&self) -> usize {
        self.slots.iter().filter(|s| s.alive).count()
    }
    pub fn allocs(&self) -> u32 {
        self.allocs
    }
    pub fn closes(&self) -> u32 {
        self.closes
    }
    pub fn emfile_hits(&self) -> u32 {
        self.emfile_hits
    }
}

// ── 效果与位置状态机 ──────────────────────────────────────────────────
/// 一次读写的效果（零长度时全零）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Effect {
    pub moved: u32,
    pub pos_after: u64,
    pub size_after: u64,
    pub touched: bool,
}

impl Effect {
    /// 零长度效果：三相冻结、不触碰。
    pub const fn frozen(pos: u64, size: u64) -> Effect {
        Effect { moved: 0, pos_after: pos, size_after: size, touched: false }
    }
}

/// 位置状态机：一轮调用内只允许一次推进。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PosState {
    /// 尚未调用
    Idle,
    /// 零长度：位置冻结
    Frozen,
    /// 已推进
    Advanced,
}

/// 状态迁移：越级倒退（Advanced→其它）拒绝。
pub fn transition(cur: PosState, zero_len: bool) -> Result<PosState, u32> {
    match (cur, zero_len) {
        (PosState::Idle, true) => Ok(PosState::Frozen),
        (PosState::Idle, false) => Ok(PosState::Advanced),
        (PosState::Frozen, true) => Ok(PosState::Frozen),
        (PosState::Frozen, false) => Ok(PosState::Advanced),
        (PosState::Advanced, _) => Err(EINVAL),
    }
}

// ── 判定器（单源） ────────────────────────────────────────────────────
/// 核心判定：顺序为 方向闸 → 目录 read → 特殊族尺寸闸 → 零长度短路 →
/// 指针 → 上限钳位 → 偏移/长度计算，与 Linux 的检查次序一致。
pub fn judge_slot(slot: &Slot, dir: Dir, count: u32, ptr: PtrClass) -> Result<Effect, u32> {
    // ① 方向闸：f_mode 检查在 vfs_read/vfs_write 首行，先于任何长度逻辑。
    if slot.mode & dir.need() == 0 {
        return Err(EBADF);
    }
    // ② 目录读：generic_read_dir 直接 -EISDIR，与 count 无关。
    if dir == Dir::Read && slot.kind == FileKind::Directory {
        return Err(EISDIR);
    }
    // ③ 特殊族尺寸闸：eventfd 双向、timerfd 读向都要求 count >= 8。
    let min_gate = match (slot.kind, dir) {
        (FileKind::EventFd, _) => true,
        (FileKind::TimerFd, Dir::Read) => true,
        _ => false,
    };
    if min_gate && count < MIN_EVENT_BYTES {
        return Err(EINVAL);
    }
    // ④ 零长度短路：fd/方向/族闸均通过后才允许 0 返回。
    if count == 0 {
        return Ok(Effect::frozen(slot.pos, slot.size));
    }
    // ⑤ 指针校验：零长度路径永不走到这里。
    if !ptr.allows(dir) {
        return Err(EFAULT);
    }
    // ⑥ 上限钳位。
    let mut want = if count > MAX_RW_COUNT { MAX_RW_COUNT } else { count };

    // ⑦ 管道：非阻塞且无空间 → EAGAIN；否则按剩余空间截短。
    if slot.kind == FileKind::Pipe && dir == Dir::Write {
        if slot.pipe_free == 0 && slot.mode & MODE_NONBLOCK != 0 {
            return Err(EAGAIN);
        }
        if want > slot.pipe_free {
            want = slot.pipe_free;
        }
    }

    // ⑧ 偏移/长度计算。
    match (slot.kind, dir) {
        (FileKind::Regular, Dir::Read)
        | (FileKind::Socket, Dir::Read)
        | (FileKind::Tty, Dir::Read) => {
            let avail = slot.size.saturating_sub(slot.pos);
            let n = (want as u64).min(avail);
            let pos_after = match slot.pos.checked_add(n) {
                Some(v) => v,
                None => return Err(EOVERFLOW),
            };
            Ok(Effect { moved: n as u32, pos_after, size_after: slot.size, touched: true })
        }
        (FileKind::Regular, Dir::Write) => {
            let base = if slot.append() { slot.size } else { slot.pos };
            let n = want as u64;
            let pos_after = match base.checked_add(n) {
                Some(v) => v,
                None => return Err(EOVERFLOW),
            };
            let tail = if base > slot.size { base } else { slot.size };
            let size_after = match tail.checked_add(n) {
                Some(v) => v,
                None => return Err(EOVERFLOW),
            };
            Ok(Effect { moved: want, pos_after, size_after, touched: true })
        }
        _ => Ok(Effect { moved: want, pos_after: slot.pos, size_after: slot.size, touched: true }),
    }
}

/// 带 fd 校验的判定（对外主入口）。
pub fn judge(table: &FxTable, fd: i32, dir: Dir, count: u32, ptr: PtrClass) -> Result<Effect, u32> {
    let slot = table.get(fd)?;
    judge_slot(slot, dir, count, ptr)
}

/// 把效果提交回槽位：位置倒退且非追加写 → 拒绝（EINVAL）。
pub fn commit(slot: &mut Slot, eff: Effect) -> Result<(), u32> {
    if eff.pos_after < slot.pos && !slot.append() {
        return Err(EINVAL);
    }
    slot.pos = eff.pos_after;
    if eff.size_after > slot.size {
        slot.size = eff.size_after;
    }
    slot.moved = slot.moved.saturating_add(eff.moved as u64);
    Ok(())
}

// ── 矩阵 ──────────────────────────────────────────────────────────────
/// 一格的期望。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Expect {
    /// 返回 0 字节且三相冻结
    Bytes0,
    /// 返回指定 errno
    Err(u32),
}

/// 一格：文件族 × 模式位 × 方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    pub kind: FileKind,
    pub mode: u32,
    pub dir: Dir,
    pub expect: Expect,
}

/// 零长度边界矩阵全量（11 格）。
pub const MATRIX: [Cell; 11] = [
    Cell { kind: FileKind::Regular, mode: MODE_READ | MODE_WRITE, dir: Dir::Read, expect: Expect::Bytes0 },
    Cell { kind: FileKind::Regular, mode: MODE_READ | MODE_WRITE, dir: Dir::Write, expect: Expect::Bytes0 },
    Cell { kind: FileKind::Regular, mode: MODE_WRITE, dir: Dir::Read, expect: Expect::Err(EBADF) },
    Cell { kind: FileKind::Regular, mode: MODE_READ, dir: Dir::Write, expect: Expect::Err(EBADF) },
    Cell { kind: FileKind::Directory, mode: MODE_READ, dir: Dir::Read, expect: Expect::Err(EISDIR) },
    Cell { kind: FileKind::EventFd, mode: MODE_READ | MODE_WRITE, dir: Dir::Read, expect: Expect::Err(EINVAL) },
    Cell { kind: FileKind::EventFd, mode: MODE_READ | MODE_WRITE, dir: Dir::Write, expect: Expect::Err(EINVAL) },
    Cell { kind: FileKind::TimerFd, mode: MODE_READ | MODE_WRITE, dir: Dir::Read, expect: Expect::Err(EINVAL) },
    Cell { kind: FileKind::Pipe, mode: MODE_READ | MODE_WRITE, dir: Dir::Write, expect: Expect::Bytes0 },
    Cell { kind: FileKind::OPath, mode: 0, dir: Dir::Read, expect: Expect::Err(EBADF) },
    Cell { kind: FileKind::Tty, mode: MODE_READ | MODE_WRITE, dir: Dir::Read, expect: Expect::Bytes0 },
];

/// 按格造槽：size=32 便于观察追加写与位置冻结。
pub fn build_slot(cell: &Cell) -> Slot {
    Slot::opened(cell.kind, cell.mode, 32)
}

/// 便捷安装：失败返回 -1（仅供判据构造，不改变 install 语义）。
fn install_or_neg(t: &mut FxTable, kind: FileKind, mode: u32) -> i32 {
    match t.install(kind, mode) {
        Ok(fd) => fd,
        Err(_) => -1,
    }
}

/// 便捷种子：设定偏移与长度。
fn seed(t: &mut FxTable, fd: i32, pos: u64, size: u64) {
    if let Ok(slot) = t.get_mut(fd) {
        slot.pos = pos;
        slot.size = size;
    }
}

/// 矩阵扫描：返回 (格数, 不符数)。
pub fn matrix_sweep() -> (u32, u32) {
    let mut cells = 0u32;
    let mut bad = 0u32;
    let mut i = 0usize;
    while i < MATRIX.len() {
        if let Some(cell) = MATRIX.get(i) {
            cells += 1;
            let slot = build_slot(cell);
            let got = judge_slot(&slot, cell.dir, 0, PtrClass::Null);
            let ok = match (cell.expect, got) {
                (Expect::Bytes0, Ok(e)) => {
                    e.moved == 0 && !e.touched && e.pos_after == slot.pos && e.size_after == slot.size
                }
                (Expect::Err(want), Err(actual)) => want == actual,
                _ => false,
            };
            if !ok {
                bad += 1;
            }
        }
        i += 1;
    }
    (cells, bad)
}

// ── 注入器 ────────────────────────────────────────────────────────────
/// 四类注入：负 fd / 已关 fd / 目录零读 / eventfd 零写。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZlInject {
    NegativeFd,
    ClosedFd,
    DirZeroRead,
    EventZero,
}

impl ZlInject {
    /// 期望错误码。
    pub fn expected(self) -> u32 {
        match self {
            ZlInject::NegativeFd | ZlInject::ClosedFd => EBADF,
            ZlInject::DirZeroRead => EISDIR,
            ZlInject::EventZero => EINVAL,
        }
    }

    /// 对应诊断码。
    pub fn code(self) -> ZlCode {
        match self {
            ZlInject::NegativeFd | ZlInject::ClosedFd => ZlCode::BadFdBeforeZero,
            ZlInject::DirZeroRead => ZlCode::DirZeroRead,
            ZlInject::EventZero => ZlCode::SpecialMinSize,
        }
    }

    /// 闭集全量。
    pub const ALL: [ZlInject; 4] = [
        ZlInject::NegativeFd, ZlInject::ClosedFd, ZlInject::DirZeroRead, ZlInject::EventZero,
    ];
}

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ledger {
    /// 注入次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl Ledger {
    /// 零账。
    pub const fn zero() -> Ledger {
        Ledger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一笔；返回本次是否仍守恒。
    pub fn record(&mut self, caught: bool) -> bool {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        self.conserved()
    }

    /// 守恒式：注入 = 检出 + 逃逸。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮四类注入，每类 INJECT_ROUNDS 次。
pub fn run_injections() -> (Ledger, [u32; 4]) {
    let mut table = FxTable::new();
    let dir_fd = install_or_neg(&mut table, FileKind::Directory, MODE_READ);
    let ev_fd = install_or_neg(&mut table, FileKind::EventFd, MODE_READ | MODE_WRITE);
    let closed_fd = install_or_neg(&mut table, FileKind::Regular, MODE_READ | MODE_WRITE);
    let _ = table.close(closed_fd);
    let mut ledger = Ledger::zero();
    let mut per = [0u32; 4];
    let mut ci = 0usize;
    while ci < ZlInject::ALL.len() {
        if let Some(case) = ZlInject::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = match case {
                    ZlInject::NegativeFd => judge(&table, -1, Dir::Read, 0, PtrClass::Null) == Err(EBADF),
                    ZlInject::ClosedFd => judge(&table, closed_fd, Dir::Write, 0, PtrClass::Null) == Err(EBADF),
                    ZlInject::DirZeroRead => judge(&table, dir_fd, Dir::Read, 0, PtrClass::Null) == Err(EISDIR),
                    ZlInject::EventZero => judge(&table, ev_fd, Dir::Write, 0, PtrClass::Null) == Err(EINVAL),
                };
                let _ = ledger.record(caught);
                if caught {
                    if let Some(slot) = per.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per)
}

// ── 对账表（Linux 零长度语义） ────────────────────────────────────────
/// (场景, 期望码：0=成功, 依据)
pub const LINUX_ZERO_TABLE: &[(&str, u32, &str)] = &[
    ("普通文件 read(fd,buf,0)", 0, "generic_file_read_iter 直接返 0，不碰 buf"),
    ("普通文件 write(fd,buf,0)", 0, "零长度不落盘，O_APPEND 也不动偏移"),
    ("目录 read(fd,buf,0)", EISDIR, "generic_read_dir 恒回 -EISDIR"),
    ("O_RDONLY 上 write(fd,buf,0)", EBADF, "vfs_write 首行判 f_mode & FMODE_WRITE"),
    ("eventfd_write(fd,0)", EINVAL, "count < sizeof(uint64_t) 先拒"),
    ("timerfd_read(fd,buf,0)", EINVAL, "read 要求 8 字节结构"),
    ("管道满时 write(fd,buf,0)", 0, "pipe_write 对 total_len==0 直接返 0"),
];

/// 查表：场景 → 期望码（表外返回 u32::MAX 哨兵）。
pub fn linux_zero_expected(scenario: &str) -> u32 {
    let mut i = 0usize;
    while i < LINUX_ZERO_TABLE.len() {
        if let Some(row) = LINUX_ZERO_TABLE.get(i) {
            if row.0 == scenario {
                return row.1;
            }
        }
        i += 1;
    }
    u32::MAX
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

/// 规范语料：11 格 × (零长 / 一字节) 两档，结果低字节入指纹。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < MATRIX.len() {
        if let Some(cell) = MATRIX.get(i) {
            let slot = build_slot(cell);
            let counts = [0u32, 1u32];
            let mut k = 0usize;
            while k < counts.len() {
                if let Some(count) = counts.get(k).copied() {
                    let code = match judge_slot(&slot, cell.dir, count, PtrClass::Null) {
                        Ok(e) => (e.moved & 0xFF) as u8,
                        Err(err) => (err & 0xFF) as u8,
                    };
                    bytes.push(code);
                }
                k += 1;
            }
        }
        i += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9210 判据集（21 条静态判据 + 3 条动态明细）。
pub fn run_unx9210_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9210");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须独占且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：普通文件零长读
    let mut t2 = FxTable::new();
    let reg = install_or_neg(&mut t2, FileKind::Regular, MODE_READ | MODE_WRITE);
    let zero_read = judge(&t2, reg, Dir::Read, 0, PtrClass::Null) == Ok(Effect::frozen(0, 0));
    s.add("正常流：普通文件零长读返 0 且不碰 NULL", zero_read,
          "fd/方向合法时 count==0 直接短路，探针指针不得被解引用");

    // 3. 正常流：零长写
    let mut t3 = FxTable::new();
    let wfd = install_or_neg(&mut t3, FileKind::Regular, MODE_READ | MODE_WRITE);
    seed(&mut t3, wfd, 5, 9);
    let zero_write = judge(&t3, wfd, Dir::Write, 0, PtrClass::Null) == Ok(Effect::frozen(5, 9));
    s.add("正常流：零长写返 0 且 pos/size 冻结", zero_write,
          "零长度写不落盘，偏移与长度都不得移动");

    // 4. 零副作用：搬运账不变
    let moved_before = match t3.get(wfd) {
        Ok(slot) => slot.moved,
        Err(_) => u64::MAX,
    };
    let committed = match t3.get_mut(wfd) {
        Ok(slot) => {
            let e = Effect::frozen(slot.pos, slot.size);
            commit(slot, e).is_ok()
        }
        Err(_) => false,
    };
    let moved_after = match t3.get(wfd) {
        Ok(slot) => slot.moved,
        Err(_) => u64::MAX - 1,
    };
    s.add("零副作用：零长度提交后搬运账不动", committed && moved_before == moved_after,
          "moved 是只增账目，零长度提交不得加 1");

    // 5. 反向判序：坏 fd 先于零长度
    let empty = FxTable::new();
    let order_ok = judge(&empty, -1, Dir::Read, 0, PtrClass::Valid) == Err(EBADF)
        && judge(&empty, STD_FDS, Dir::Write, 0, PtrClass::Valid) == Err(EBADF);
    s.add("反向判序：坏 fd 优先于零长度（EBADF）", order_ok,
          "fd 校验必须在 count==0 短路之前，否则空句柄会被判成功");

    // 6. 方向闸先于零长度
    let mut t6 = FxTable::new();
    let ro = install_or_neg(&mut t6, FileKind::Regular, MODE_READ);
    let wo = install_or_neg(&mut t6, FileKind::Regular, MODE_WRITE);
    let dir_ok = judge(&t6, ro, Dir::Write, 0, PtrClass::Valid) == Err(EBADF)
        && judge(&t6, wo, Dir::Read, 0, PtrClass::Valid) == Err(EBADF);
    s.add("方向闸：只读句柄写 0 字节仍 EBADF", dir_ok,
          "vfs_write 首行判 f_mode，零长度不得绕过方向闸");

    // 7. 目录零长读
    let mut t7 = FxTable::new();
    let dfd = install_or_neg(&mut t7, FileKind::Directory, MODE_READ);
    let dir_zero = judge(&t7, dfd, Dir::Read, 0, PtrClass::Null) == Err(EISDIR);
    s.add("目录：零长读仍回 EISDIR", dir_zero,
          "generic_read_dir 与 count 无关；列目录须走 getdents64");

    // 8. 特殊族尺寸闸
    let mut t8 = FxTable::new();
    let efd = install_or_neg(&mut t8, FileKind::EventFd, MODE_READ | MODE_WRITE);
    let tfd = install_or_neg(&mut t8, FileKind::TimerFd, MODE_READ);
    let special_ok = judge(&t8, efd, Dir::Read, 0, PtrClass::Valid) == Err(EINVAL)
        && judge(&t8, efd, Dir::Write, 0, PtrClass::Valid) == Err(EINVAL)
        && judge(&t8, tfd, Dir::Read, 0, PtrClass::Valid) == Err(EINVAL)
        && judge(&t8, tfd, Dir::Read, 8, PtrClass::Valid).is_ok();
    s.add("特殊族：eventfd/timerfd 零长 EINVAL、8 字节放行", special_ok,
          "尺寸闸排在零长度之前，8 字节才进入正常路径");

    // 9. 矩阵逐格
    let (cells, mism) = matrix_sweep();
    s.add("矩阵逐格：11 格零长度期望全部命中", cells == MATRIX.len() as u32 && mism == 0,
          "文件族 × 方向 × 零长度必须与 Linux 语义逐格一致");

    // 10. 上限钳位
    let mut t10 = FxTable::new();
    let big = install_or_neg(&mut t10, FileKind::Regular, MODE_READ | MODE_WRITE);
    seed(&mut t10, big, 0, 0x1_0000_0000);
    let clamp_ok = judge(&t10, big, Dir::Read, MAX_RW_COUNT + 1, PtrClass::Valid) == Ok(Effect {
        moved: MAX_RW_COUNT, pos_after: MAX_RW_COUNT as u64,
        size_after: 0x1_0000_0000, touched: true,
    }) && judge(&t10, big, Dir::Read, MAX_RW_COUNT - 1, PtrClass::Valid) == Ok(Effect {
        moved: MAX_RW_COUNT - 1, pos_after: (MAX_RW_COUNT - 1) as u64,
        size_after: 0x1_0000_0000, touched: true,
    });
    s.add("上限：count 超过 MAX_RW_COUNT 被钳位", clamp_ok,
          "0x7FFFF001 → 0x7FFFF000；0x7FFF_EFFF 原样通过");

    // 11. 追加写跟随文件尾
    let mut t11 = FxTable::new();
    let ap = install_or_neg(&mut t11, FileKind::Regular, MODE_WRITE | MODE_APPEND);
    seed(&mut t11, ap, 0, 40);
    let append_ok = judge(&t11, ap, Dir::Write, 6, PtrClass::Valid)
        == Ok(Effect { moved: 6, pos_after: 46, size_after: 46, touched: true });
    s.add("追加写：偏移从文件尾起算", append_ok,
          "O_APPEND 忽略当前 pos，pos_after 与 size_after 同为 size+len");

    // 12. 偏移溢出
    let mut t12 = FxTable::new();
    let ofd = install_or_neg(&mut t12, FileKind::Regular, MODE_WRITE);
    seed(&mut t12, ofd, u64::MAX - 1, u64::MAX - 1);
    let of_ok = judge(&t12, ofd, Dir::Write, 8, PtrClass::Valid) == Err(EOVERFLOW);
    s.add("偏移溢出：pos+len 越过 u64 上界回 EOVERFLOW", of_ok,
          "用 checked_add 复算，溢出不得回绕成小偏移");

    // 13. 状态机：越级倒退被拒
    let sm_ok = transition(PosState::Idle, true) == Ok(PosState::Frozen)
        && transition(PosState::Frozen, false) == Ok(PosState::Advanced)
        && transition(PosState::Advanced, true) == Err(EINVAL)
        && transition(PosState::Advanced, false) == Err(EINVAL);
    s.add("状态机：Advanced→Frozen 越级倒退被拒", sm_ok,
          "一轮调用只允许一次推进；零长度不得回拉已推进偏移");

    // 14. 注入 4 类 10/10
    let (ledger, per) = run_injections();
    let all_caught = per.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入：四类违约各 10/10 全检出", all_caught,
          "负 fd/已关 fd/目录零读/eventfd 零写必须逐类全抓");

    // 15. 守恒
    s.add("守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == ZlInject::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出，逃逸恒为 0");

    // 16. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等；隐藏状态即失败");

    // 17. 对账表
    let recon_ok = linux_zero_expected("普通文件 read(fd,buf,0)") == 0
        && linux_zero_expected("目录 read(fd,buf,0)") == EISDIR
        && linux_zero_expected("eventfd_write(fd,0)") == EINVAL
        && linux_zero_expected("管道满时 write(fd,buf,0)") == 0
        && linux_zero_expected("表外场景") == u32::MAX;
    s.add("对账：Linux 零长度语义表逐行可复算", recon_ok,
          "7 行冻结表按场景名查询，表外返回显性哨兵");

    // 18. 文案闭集
    let text_ok = ZlCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：12 个诊断码均带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 19. 容量：表满 EMFILE 且账不丢
    let mut t20 = FxTable::new();
    let mut filled = 0u32;
    let mut k = 0u32;
    while k < DYN_SLOTS as u32 {
        if t20.install(FileKind::Regular, MODE_READ).is_ok() {
            filled += 1;
        }
        k += 1;
    }
    let overflow = t20.install(FileKind::Regular, MODE_READ) == Err(EMFILE);
    s.add("容量：8 槽填满后第 9 次安装回 EMFILE",
          filled == DYN_SLOTS as u32 && overflow && t20.emfile_hits() == 1
              && t20.open_count() == DYN_SLOTS,
          "表满拒绝必须记账，已安装的 8 个槽不得被覆盖");

    // 动态明细（≤8）：矩阵、注入、指纹三行
    let matrix_text = format!("矩阵 {} 格 / 不符 {}", cells, mism);
    s.add_dyn("矩阵快照", mism == 0, &matrix_text);
    let ledger_text = format!("注入 {} / 检出 {} / 逃逸 {}",
                              ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("注入账快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    // 20. 收口：判据条数只增不减
    let before = s.len();
    let trunc = s.truncated();
    s.add("收口：判据条数不少于 20 且未截断", before >= 20 && !trunc,
          "本模块断言只增不减；CheckSet 容量 112，动态槽仅用 3 个");

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn table_with(kind: FileKind, mode: u32, size: u64) -> (FxTable, i32) {
        let mut t = FxTable::new();
        let fd = t.install(kind, mode).unwrap_or(-1);
        if let Ok(slot) = t.get_mut(fd) {
            slot.size = size;
        }
        (t, fd)
    }

    #[test]
    fn zero_length_regular_is_noop() {
        let (t, fd) = table_with(FileKind::Regular, MODE_READ | MODE_WRITE, 40);
        assert_eq!(judge(&t, fd, Dir::Read, 0, PtrClass::Null), Ok(Effect::frozen(0, 40)));
        assert_eq!(judge(&t, fd, Dir::Write, 0, PtrClass::Kernel), Ok(Effect::frozen(0, 40)));
    }

    #[test]
    fn bad_fd_wins_over_zero_length() {
        let t = FxTable::new();
        assert_eq!(judge(&t, -1, Dir::Read, 0, PtrClass::Valid), Err(EBADF));
        assert_eq!(judge(&t, 3, Dir::Write, 0, PtrClass::Valid), Err(EBADF));
        assert_eq!(judge(&t, STD_FDS + DYN_SLOTS as i32, Dir::Write, 0, PtrClass::Valid), Err(EBADF));
    }

    #[test]
    fn direction_gate_beats_zero_length() {
        let (t, ro) = table_with(FileKind::Regular, MODE_READ, 8);
        assert_eq!(judge(&t, ro, Dir::Write, 0, PtrClass::Valid), Err(EBADF));
        let (t2, wo) = table_with(FileKind::Regular, MODE_WRITE, 8);
        assert_eq!(judge(&t2, wo, Dir::Read, 0, PtrClass::Valid), Err(EBADF));
    }

    #[test]
    fn directory_zero_read_is_eisdir() {
        let (t, fd) = table_with(FileKind::Directory, MODE_READ, 0);
        assert_eq!(judge(&t, fd, Dir::Read, 0, PtrClass::Null), Err(EISDIR));
        assert_eq!(judge(&t, fd, Dir::Read, 4096, PtrClass::Valid), Err(EISDIR));
    }

    #[test]
    fn matrix_is_all_green() {
        let (cells, bad) = matrix_sweep();
        assert_eq!(cells, MATRIX.len() as u32);
        assert_eq!(bad, 0);
    }

    #[test]
    fn count_is_clamped_to_max_rw_count() {
        let (t, fd) = table_with(FileKind::Regular, MODE_READ, 0x2_0000_0000);
        let e = judge(&t, fd, Dir::Read, MAX_RW_COUNT + 1, PtrClass::Valid).unwrap_or(Effect::frozen(0, 0));
        assert_eq!(e.moved, MAX_RW_COUNT);
    }

    #[test]
    fn overflow_and_regress_are_rejected() {
        let mut t = FxTable::new();
        let fd = t.install(FileKind::Regular, MODE_READ | MODE_WRITE).unwrap_or(-1);
        seed(&mut t, fd, u64::MAX, u64::MAX);
        assert_eq!(judge(&t, fd, Dir::Write, 1, PtrClass::Valid), Err(EOVERFLOW));
        seed(&mut t, fd, 64, 64);
        if let Ok(slot) = t.get_mut(fd) {
            let bad = Effect { moved: 4, pos_after: 16, size_after: 64, touched: true };
            assert_eq!(commit(slot, bad), Err(EINVAL));
        }
    }

    #[test]
    fn pos_state_machine_rejects_regress() {
        assert_eq!(transition(PosState::Idle, true), Ok(PosState::Frozen));
        assert_eq!(transition(PosState::Frozen, true), Ok(PosState::Frozen));
        assert_eq!(transition(PosState::Frozen, false), Ok(PosState::Advanced));
        assert_eq!(transition(PosState::Advanced, true), Err(EINVAL));
    }

    #[test]
    fn injections_are_all_caught() {
        let (l, per) = run_injections();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 40);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn fingerprint_is_stable() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn table_capacity_reports_emfile() {
        let mut t = FxTable::new();
        for _ in 0..DYN_SLOTS {
            assert!(t.install(FileKind::Regular, MODE_READ).is_ok());
        }
        assert_eq!(t.install(FileKind::Regular, MODE_READ), Err(EMFILE));
        assert_eq!(t.emfile_hits(), 1);
        assert_eq!(t.open_count(), DYN_SLOTS);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9210_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(s.len() >= 20);
    }
}
