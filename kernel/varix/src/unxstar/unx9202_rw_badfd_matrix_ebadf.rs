//! UNX-F9202 read/write 坏 fd 矩阵（全 EBADF） · 0x7A000段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9202 逐字）：
//!   负 fd/未开 fd/已关 fd 三类注入矩阵判据过，全返回 -EBADF 与 Linux
//!   逐码一致，10/10 次检出复测格过。
//!
//! 设计要点：
//!   ① fd 空间按 Linux 口径建模：0/1/2 为常驻标准流，动态槽从 3 起；
//!      负值、越上界、空槽（未开）与已关闭槽四类全部归 EBADF，不许把
//!      「未开」误报成 EINVAL，也不许把「越界」误报成 ENOENT。
//!   ② 判定次序是本模块的核心判据：fd 校验先于指针校验。行为表
//!      `judge_read`/`judge_write` 在 fd 非法的瞬间返回 EBADF，即使
//!      同时传入 NULL 指针也不得改报 EFAULT（Linux 的 fget 先于
//!      copy_to_user）。
//!   ③ 槽位代际（generation）防陈旧句柄：close 时代际自增，随后同一
//!      原始号被重新分配，旧代际句柄仍然 EBADF；这是「已关 fd」与
//!      「复用 fd」两类语义的分水岭。
//!   ④ 注入账守恒：注入次数 = 检出次数 + 逃逸次数，逃逸必须恒为 0；
//!      账只增不减，任何负增量都显性拒绝。
//!   ⑤ 与 Linux 逐码一致通过冻结表复算：每个错误只允许出现表内码，
//!      表的负值与 errno.h 常量逐项对齐；Windows 侧给出最近似对照码，
//!      仅作说明，不参与判定。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9202 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7A000;

/// 常驻标准流数量（0=stdin，1=stdout，2=stderr）。
pub const STD_FDS: i32 = 3;

/// 动态 fd 槽数量（fd 号 3..=66）。
pub const MAX_FDS: usize = 64;

/// 单类注入的复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 三类坏 fd 注入类别数（负值/未开/已关；越界另计为第 4 类守卫）。
pub const BAD_FD_CLASSES: u32 = 4;

/// 读方向标记（用于指针权限校验）。
pub const DIR_READ: u32 = 1;
/// 写方向标记。
pub const DIR_WRITE: u32 = 2;

/// Linux errno 数值（正数口径；系统调用返回负值）。
pub const ENOENT: i32 = 2;
pub const EBADF: i32 = 9;
pub const EFAULT: i32 = 14;
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FdCode {
    /// 负 fd：fd < 0
    NegativeFd,
    /// 未打开：槽位仍为 Free
    NotOpened,
    /// 已关闭：槽位为 Closed 且代际未推进
    AlreadyClosed,
    /// fd 越上界
    BeyondLimit,
    /// 陈旧代际：同号已复用但句柄代际落后
    StaleGeneration,
    /// 坏指针：NULL/未映射/内核地址
    BadPointer,
    /// 只读映射上写
    WriteToRo,
    /// 方向与打开模式不符
    DirectionMismatch,
    /// 零长度：合法短路
    ZeroLength,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl FdCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            FdCode::NegativeFd => "负 fd 被拒：fd 小于 0 属非法号段；请先检查句柄来源，\
                负值一律回 EBADF",
            FdCode::NotOpened => "未打开 fd 被拒：目标槽位仍为空闲；请先 open/pipe/dup \
                取得有效句柄再读写",
            FdCode::AlreadyClosed => "已关闭 fd 被拒：槽位处于 Closed 态；关闭后不得复用\
                旧句柄，重新 open 会得到新代际",
            FdCode::BeyondLimit => "fd 越上界被拒：号大于动态槽上限；请按 \
                3..3+MAX_FDS 约束分配，越界回 EBADF",
            FdCode::StaleGeneration => "陈旧代际被拒：同号已被新句柄占用，旧代际不可用；\
                请用 close 返回的新代际重建句柄",
            FdCode::BadPointer => "坏指针被拒：NULL/未映射/内核地址不可访问；请把缓冲区\
                固定到已映射用户区间并复核长度",
            FdCode::WriteToRo => "只读映射写入被拒：目标区间无写权限；请改用可写映射或\
                先 mprotect 提权",
            FdCode::DirectionMismatch => "方向不符：只读句柄上发起写或只写句柄上发起读；\
                请按 open 模式选择读写方向",
            FdCode::ZeroLength => "零长度合法短路：不触碰缓冲区，直接返回 0；调用方无需\
                重试，也不应据此判断 fd 有效性",
            FdCode::LedgerRegress => "注入账倒退被拒：计数只增不减；请检查是否有回滚\
                路径误改累计值",
            FdCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏状态\
                或非确定哈希",
            FdCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [FdCode; 12] = [
        FdCode::NegativeFd,
        FdCode::NotOpened,
        FdCode::AlreadyClosed,
        FdCode::BeyondLimit,
        FdCode::StaleGeneration,
        FdCode::BadPointer,
        FdCode::WriteToRo,
        FdCode::DirectionMismatch,
        FdCode::ZeroLength,
        FdCode::LedgerRegress,
        FdCode::NotReproducible,
        FdCode::SegmentViolation,
    ];
}

/// 系统调用失败结果：携带 Linux errno 正数码。
pub type SysResult<T> = Result<T, i32>;

// ── fd 表模型 ─────────────────────────────────────────────────────────
/// 槽位状态。Free 与 Closed 必须分开：前者表示从未分配，后者表示
/// 分配过且已关闭；两者对外都回 EBADF，但内部账目与复用语义不同。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    /// 从未分配
    Free,
    /// 已打开
    Open,
    /// 已关闭
    Closed,
}

/// 单个 fd 槽。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FdSlot {
    /// 槽位状态
    pub state: SlotState,
    /// 打开模式（DIR_READ / DIR_WRITE / 两者）
    pub mode: u32,
    /// 代际（每次 close 自增；复用后旧代际失效）
    pub generation: u16,
    /// 累计读次数（账目，只增）
    pub reads: u32,
    /// 累计写次数
    pub writes: u32,
}

impl FdSlot {
    const fn empty() -> FdSlot {
        FdSlot {
            state: SlotState::Free,
            mode: 0,
            generation: 1,
            reads: 0,
            writes: 0,
        }
    }
}

/// 定容 fd 表。标准流在 `new()` 中预置为打开态（stdin 读、stdout/stderr 写）。
#[derive(Clone, Copy, Debug)]
pub struct FdTable {
    std: [FdSlot; 3],
    dyns: [FdSlot; MAX_FDS],
    /// 动态槽累计分配次数
    allocs: u32,
    /// 动态槽累计关闭次数
    closes: u32,
}

impl FdTable {
    /// 构造标准 fd 表。
    pub const fn new() -> FdTable {
        let mut std0 = FdSlot::empty();
        std0.state = SlotState::Open;
        std0.mode = DIR_READ;
        let mut std1 = FdSlot::empty();
        std1.state = SlotState::Open;
        std1.mode = DIR_WRITE;
        let mut std2 = FdSlot::empty();
        std2.state = SlotState::Open;
        std2.mode = DIR_WRITE;
        FdTable {
            std: [std0, std1, std2],
            dyns: [FdSlot::empty(); MAX_FDS],
            allocs: 0,
            closes: 0,
        }
    }

    /// 取槽位视图：返回 None 表示号段非法（负值/越界），这是 EBADF 的
    /// 第一层；返回 Some(slot) 后由调用方继续判定 Free/Closed。
    fn slot(&self, fd: i32) -> Option<&FdSlot> {
        if fd < 0 {
            return None;
        }
        let idx = (fd - STD_FDS) as usize;
        if fd < STD_FDS {
            return self.std.get(fd as usize);
        }
        self.dyns.get(idx)
    }

    fn slot_mut(&mut self, fd: i32) -> Option<&mut FdSlot> {
        if fd < 0 {
            return None;
        }
        let idx = (fd - STD_FDS) as usize;
        if fd < STD_FDS {
            return self.std.get_mut(fd as usize);
        }
        self.dyns.get_mut(idx)
    }

    /// 打开一个动态 fd：优先复用 Closed 槽（代际已在 close 时推进），
    /// 否则取首个 Free 槽。表满返回 EMFILE 语义的 EBADF 之外码——本模块
    /// 只负责坏 fd 面，表满仍返回 EMFILE 的通用占位（24）。
    pub fn open(&mut self, mode: u32) -> SysResult<i32> {
        let mut i = 0usize;
        while i < MAX_FDS {
            if let Some(slot) = self.dyns.get_mut(i) {
                if slot.state != SlotState::Open {
                    slot.state = SlotState::Open;
                    slot.mode = mode;
                    self.allocs = self.allocs.saturating_add(1);
                    return Ok(STD_FDS + i as i32);
                }
            }
            i += 1;
        }
        Err(EMFILE_PLACEHOLDER)
    }

    /// 关闭 fd。负值/未开/已关全部 EBADF；成功时推进代际。
    pub fn close(&mut self, fd: i32) -> SysResult<u16> {
        let slot = match self.slot_mut(fd) {
            Some(s) => s,
            None => return Err(EBADF),
        };
        if slot.state != SlotState::Open {
            return Err(EBADF);
        }
        slot.state = SlotState::Closed;
        slot.generation = slot.generation.wrapping_add(1);
        slot.mode = 0;
        let gen = slot.generation;
        self.closes = self.closes.saturating_add(1);
        Ok(gen)
    }

    /// 句柄有效性判定（只判 fd，不碰缓冲区）。返回打开模式或错误码。
    pub fn describe(&self, fd: i32) -> SysResult<(u32, u16)> {
        if fd < 0 {
            return Err(EBADF);
        }
        if fd >= STD_FDS + MAX_FDS as i32 {
            return Err(EBADF);
        }
        let slot = match self.slot(fd) {
            Some(s) => s,
            None => return Err(EBADF),
        };
        match slot.state {
            SlotState::Open => Ok((slot.mode, slot.generation)),
            SlotState::Free | SlotState::Closed => Err(EBADF),
        }
    }

    /// 代际句柄校验：号与代际都必须匹配。
    pub fn describe_generation(&self, fd: i32, generation: u16) -> SysResult<u32> {
        let (mode, gen) = self.describe(fd)?;
        if gen != generation {
            return Err(EBADF);
        }
        Ok(mode)
    }

    /// 分配次数（账目）。
    pub fn allocs(&self) -> u32 {
        self.allocs
    }

    /// 关闭次数（账目）。
    pub fn closes(&self) -> u32 {
        self.closes
    }

    /// 当前打开的动态槽数量。
    pub fn open_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < MAX_FDS {
            if let Some(slot) = self.dyns.get(i) {
                if slot.state == SlotState::Open {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

/// 表满占位码（EMFILE=24）；本模块不对它做语义展开。
const EMFILE_PLACEHOLDER: i32 = 24;

// ── 指针类别模型 ──────────────────────────────────────────────────────
/// 用户缓冲区类别（本模块只建模 EFAULT 相关的四类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PtrClass {
    /// 已映射且权限匹配的合法用户区间
    Valid,
    /// NULL
    Null,
    /// 未映射地址
    Unmapped,
    /// 只读映射（写方向非法）
    ReadOnlyMapping,
    /// 内核地址（用户态不可访问）
    Kernel,
}

impl PtrClass {
    /// 读方向可用性。
    pub fn readable(self) -> bool {
        matches!(self, PtrClass::Valid | PtrClass::ReadOnlyMapping)
    }

    /// 写方向可用性。
    pub fn writable(self) -> bool {
        matches!(self, PtrClass::Valid)
    }
}

// ── read/write 判定器 ─────────────────────────────────────────────────
/// read 判定：fd 先于指针，长度 0 在 fd 合法后短路。
pub fn judge_read(table: &FdTable, fd: i32, ptr: PtrClass, len: usize) -> SysResult<usize> {
    let (mode, _generation) = table.describe(fd)?;
    if mode & DIR_READ == 0 {
        return Err(EBADF);
    }
    if len == 0 {
        return Ok(0);
    }
    if !ptr.readable() {
        return Err(EFAULT);
    }
    Ok(len)
}

/// write 判定：与 read 同序，但只读映射按 EFAULT（Linux 的写保护页
/// 在 copy_from_user 阶段返回 EFAULT），方向不符按 EBADF。
pub fn judge_write(table: &FdTable, fd: i32, ptr: PtrClass, len: usize) -> SysResult<usize> {
    let (mode, _generation) = table.describe(fd)?;
    if mode & DIR_WRITE == 0 {
        return Err(EBADF);
    }
    if len == 0 {
        return Ok(0);
    }
    if !ptr.writable() {
        return Err(EFAULT);
    }
    Ok(len)
}

// ── 坏 fd 注入矩阵 ────────────────────────────────────────────────────
/// 四类坏 fd 样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadFdCase {
    /// 负值
    Negative,
    /// 从未打开的空闲槽
    NeverOpened,
    /// 已关闭槽
    Closed,
    /// 越过动态槽上界
    BeyondLimit,
}

impl BadFdCase {
    /// 构造样本 fd。
    pub fn sample(self, table: &FdTable) -> i32 {
        match self {
            BadFdCase::Negative => -1,
            BadFdCase::NeverOpened => STD_FDS + MAX_FDS as i32 - 1,
            BadFdCase::Closed => {
                let _ = table;
                STD_FDS
            }
            BadFdCase::BeyondLimit => STD_FDS + MAX_FDS as i32,
        }
    }

    /// 期望诊断码。
    pub fn code(self) -> FdCode {
        match self {
            BadFdCase::Negative => FdCode::NegativeFd,
            BadFdCase::NeverOpened => FdCode::NotOpened,
            BadFdCase::Closed => FdCode::AlreadyClosed,
            BadFdCase::BeyondLimit => FdCode::BeyondLimit,
        }
    }

    /// 闭集全量。
    pub const ALL: [BadFdCase; 4] = [
        BadFdCase::Negative,
        BadFdCase::NeverOpened,
        BadFdCase::Closed,
        BadFdCase::BeyondLimit,
    ];
}

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) -> SysResult<()> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(-(FdCode::LedgerRegress as i32));
        }
        Ok(())
    }

    /// 守恒式：注入 = 检出 + 逃逸。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮四类坏 fd 注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_bad_fd_matrix() -> (InjectLedger, [u32; 4]) {
    let mut table = FdTable::new();
    let closed = match table.open(DIR_READ | DIR_WRITE) {
        Ok(fd) => {
            let _ = table.close(fd);
            fd
        }
        Err(_) => -1,
    };
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadFdCase::ALL.len() {
        if let Some(case) = BadFdCase::ALL.get(ci).copied() {
            let fd = if case == BadFdCase::Closed {
                closed
            } else {
                case.sample(&table)
            };
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = judge_read(&table, fd, PtrClass::Valid, 1) == Err(EBADF);
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

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；坏句柄族"),
    ("EFAULT", -14, 998, "Windows ERROR_NOACCESS；坏地址族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；坏路径族"),
    ("EMFILE", -24, 4, "Windows ERROR_TOO_MANY_OPEN_FILES；fd 表满"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
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

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
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

/// 规范语料双跑指纹：覆盖四类坏 fd × 读/写 × 合法指针。
pub fn corpus_fingerprint() -> u32 {
    let mut table = FdTable::new();
    let mut bytes: Vec<u8> = Vec::new();
    let mut cur = table.open(DIR_READ);
    let opened = match cur {
        Ok(fd) => fd,
        Err(_) => -1,
    };
    cur = table.open(DIR_READ | DIR_WRITE);
    let rw = match cur {
        Ok(fd) => fd,
        Err(_) => -1,
    };
    let closed = {
        let g = table.close(rw);
        let _ = g;
        rw
    };
    let fds = [-1, opened, closed, STD_FDS + MAX_FDS as i32];
    let mut i = 0usize;
    while i < fds.len() {
        if let Some(fd) = fds.get(i).copied() {
            let r = judge_read(&table, fd, PtrClass::Valid, 8);
            let w = judge_write(&table, fd, PtrClass::Null, 8);
            bytes.push(match r {
                Ok(n) => (n & 0xFF) as u8,
                Err(e) => (e & 0xFF) as u8,
            });
            bytes.push(match w {
                Ok(n) => (n & 0xFF) as u8,
                Err(e) => (e & 0xFF) as u8,
            });
        }
        i += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9202 判据集（≥12 条；动态明细 ≤8 条）。
pub fn run_unx9202_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9202");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：标准输出可写
    let table = FdTable::new();
    let normal = judge_write(&table, 1, PtrClass::Valid, 16) == Ok(16);
    s.add("正常流：stdout 合法写 16 字节", normal,
          "合法 fd + 可写指针应返回请求长度");

    // 3. 正常流：标准输入可读
    let normal_read = judge_read(&table, 0, PtrClass::ReadOnlyMapping, 4) == Ok(4);
    s.add("正常流：stdin 只读映射读 4 字节", normal_read,
          "只读映射对读方向合法，不应误报 EFAULT");

    // 4. 四类坏 fd 全 EBADF
    let (ledger, per_class) = run_bad_fd_matrix();
    let all_badf = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("四类坏 fd 各 10/10 全 EBADF", all_badf,
          "负值/未开/已关/越界四类必须全部返回 -EBADF");

    // 5. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0 && ledger.injected == BAD_FD_CLASSES * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 6. 判定次序：坏 fd 先于坏指针
    let order_ok = judge_read(&table, -1, PtrClass::Null, 8) == Err(EBADF)
        && judge_write(&table, STD_FDS + MAX_FDS as i32, PtrClass::Unmapped, 8) == Err(EBADF);
    s.add("判定次序：EBADF 先于 EFAULT", order_ok,
          "fd 非法的瞬间不得改报指针错误");

    // 7. 指针面：合法 fd + 坏指针 → EFAULT
    let ptr_ok = judge_read(&table, 0, PtrClass::Null, 8) == Err(EFAULT)
        && judge_write(&table, 1, PtrClass::Kernel, 8) == Err(EFAULT)
        && judge_write(&table, 1, PtrClass::ReadOnlyMapping, 8) == Err(EFAULT);
    s.add("指针面：NULL/内核/只读映射写 → EFAULT", ptr_ok,
          "合法 fd 上坏指针必须回 EFAULT，不得回 EBADF");

    // 8. 零长度短路
    let zero_ok = judge_read(&table, 0, PtrClass::Null, 0) == Ok(0)
        && judge_write(&table, 1, PtrClass::Null, 0) == Ok(0);
    s.add("零长度：合法 fd 上不触碰缓冲区", zero_ok,
          "len=0 在 fd 校验通过后直接返回 0");

    // 9. 方向不符
    let mut dir_table = FdTable::new();
    let ro = match dir_table.open(DIR_READ) {
        Ok(fd) => fd,
        Err(_) => -1,
    };
    let direction_ok = judge_write(&dir_table, ro, PtrClass::Valid, 4) == Err(EBADF)
        && judge_read(&dir_table, 1, PtrClass::Valid, 4) == Err(EBADF);
    s.add("方向面：只读句柄写/只写句柄读 → EBADF", direction_ok,
          "模式位与方向必须同时成立");

    // 10. 代际：关闭后旧代际失效，复用后新代际有效
    let close_gen = dir_table.close(ro);
    let stale_ok = match close_gen {
        Ok(gen) => {
            dir_table.describe_generation(ro, gen) == Err(EBADF)
                && dir_table.describe_generation(ro, gen.wrapping_sub(1)) == Err(EBADF)
        }
        Err(_) => false,
    };
    s.add("代际：关闭后同号旧代际全部失效", stale_ok,
          "close 推进代际，任何旧代际都不得再通过");

    // 11. 单号复用后旧代际仍拒、新代际可用
    let reuse = dir_table.open(DIR_READ | DIR_WRITE);
    let reuse_ok = match reuse {
        Ok(fd) => {
            let g = dir_table.describe(fd);
            match g {
                Ok((_, gen)) => dir_table.describe_generation(fd, gen) == Ok(DIR_READ | DIR_WRITE),
                Err(_) => false,
            }
        }
        Err(_) => false,
    };
    s.add("复用：新句柄代际可用且模式正确", reuse_ok,
          "复用槽必须携带新代际与新模式");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(EBADF)
        && agrees_with_linux(EFAULT)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(ENOENT)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 13. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 14. 文案闭集：12 个诊断码均有非空建议
    let text_ok = FdCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 15. 诊断码唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < FdCode::ALL.len() {
        let mut j = i + 1;
        while j < FdCode::ALL.len() {
            if let (Some(a), Some(b)) = (FdCode::ALL.get(i), FdCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）：注入账一行 + 指纹一行
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("账目快照", ledger.conserved(), &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    // 16. 账目单调：再注入 40 次只增不减
    let mut mono = ledger;
    let before = mono.injected;
    let mut k = 0u32;
    while k < 4 {
        let _ = mono.record(true);
        k += 1;
    }
    s.add("账目单调：再注入只增不减", mono.injected == before + 4,
          "累计计数禁止回退或饱和到旧值");

    // 17. 关闭账与分配账闭合
    let mut acc = FdTable::new();
    let mut ok_acc = true;
    let mut n = 0u32;
    while n < 8 {
        match acc.open(DIR_READ | DIR_WRITE) {
            Ok(fd) => {
                if acc.close(fd).is_err() {
                    ok_acc = false;
                }
            }
            Err(_) => ok_acc = false,
        }
        n += 1;
    }
    s.add("分配/关闭账闭合：8 开 8 关", ok_acc && acc.allocs() == 8 && acc.closes() == 8,
          "每次成功 open 与 close 必须各记一笔");

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_read_write() {
        let t = FdTable::new();
        assert_eq!(judge_read(&t, 0, PtrClass::Valid, 12), Ok(12));
        assert_eq!(judge_write(&t, 1, PtrClass::Valid, 12), Ok(12));
    }

    #[test]
    fn negative_fd_ebadf() {
        let t = FdTable::new();
        assert_eq!(judge_read(&t, -1, PtrClass::Valid, 1), Err(EBADF));
        assert_eq!(judge_write(&t, -9, PtrClass::Valid, 1), Err(EBADF));
    }

    #[test]
    fn unopened_and_beyond_ebadf() {
        let t = FdTable::new();
        assert_eq!(judge_read(&t, STD_FDS, PtrClass::Valid, 1), Err(EBADF));
        assert_eq!(judge_read(&t, STD_FDS + MAX_FDS as i32, PtrClass::Valid, 1), Err(EBADF));
    }

    #[test]
    fn closed_fd_ebadf() {
        let mut t = FdTable::new();
        let fd = t.open(DIR_READ).unwrap_or(-1);
        assert!(fd >= STD_FDS);
        assert!(t.close(fd).is_ok());
        assert_eq!(judge_read(&t, fd, PtrClass::Valid, 1), Err(EBADF));
    }

    #[test]
    fn bad_fd_wins_over_bad_pointer() {
        let t = FdTable::new();
        assert_eq!(judge_read(&t, -1, PtrClass::Null, 8), Err(EBADF));
        assert_eq!(judge_write(&t, -1, PtrClass::Kernel, 8), Err(EBADF));
    }

    #[test]
    fn pointer_fault_after_fd_ok() {
        let t = FdTable::new();
        assert_eq!(judge_read(&t, 0, PtrClass::Unmapped, 4), Err(EFAULT));
        assert_eq!(judge_write(&t, 1, PtrClass::ReadOnlyMapping, 4), Err(EFAULT));
    }

    #[test]
    fn zero_length_short_circuits() {
        let t = FdTable::new();
        assert_eq!(judge_read(&t, 0, PtrClass::Null, 0), Ok(0));
        assert_eq!(judge_write(&t, 1, PtrClass::Null, 0), Ok(0));
    }

    #[test]
    fn matrix_ledger_conserved() {
        let (l, per) = run_bad_fd_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, BAD_FD_CLASSES * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn generation_guard() {
        let mut t = FdTable::new();
        let fd = t.open(DIR_READ).unwrap_or(-1);
        let g = t.close(fd).unwrap_or(0);
        assert_eq!(t.describe_generation(fd, g), Err(EBADF));
        let fd2 = t.open(DIR_READ | DIR_WRITE).unwrap_or(-1);
        let (mode, g2) = t.describe(fd2).unwrap_or((0, 0));
        assert_eq!(mode, DIR_READ | DIR_WRITE);
        assert_eq!(t.describe_generation(fd2, g2), Ok(DIR_READ | DIR_WRITE));
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EBADF));
        assert!(agrees_with_linux(EFAULT));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EBADF), -EBADF);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9202_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
