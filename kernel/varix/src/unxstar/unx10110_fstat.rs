//! UNX-F10110 fstat 正反双判据（元数据达成与坏缓冲误用受控）· 0x92B40 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10111-J1 pread/pwrite 偏移读写达成（不移动共享偏移 LTP pread01
//! 面 10/10 次）；UNX-F10111-J2 负偏移误用受控（预期 EINVAL 精确复现 10/10 次）
//!
//! SEG_BASE = 0x92B40（本卡独占 0x40 窗口，下一卡 0x92B80）；
//! 诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 状态机：文件态机 Closed→Open→Truncated→Frozen，跨态操作（关后 fstat /
//!      冻结后截断）逐次显性拒绝。
//!   ② 账本：元数据账本 MetaLedger（fd→size/mtime/nlink/态），截断即改账，
//!      fstat 只读快照不改账。
//!   ③ 解析器：缓冲区地址解析器（0x1000 起的 8 字节对齐窗，越窗/未对齐/
//!      零地址一律 EFAULT 族），fstat 落账前必须先过解析器。
//!   ④ 注入核：零地址 / 未对齐 / 越窗 / 坏 fd 四类坏缓冲样本逐类 10/10 次
//!      精确复现，注入账 注入=检出+逃逸，逃逸恒为 0。
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10110 独占 0x40 窗口，下一卡 0x92B80）。
pub const SEG_BASE: u32 = 0x92B40;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 元数据账本容量上限（fd 数）。
pub const LEDGER_CAP: usize = 32;

/// 缓冲区合法窗基址（模拟用户态映射区起点）。
pub const BUF_BASE: u64 = 0x1000;

/// 缓冲区合法窗终点（窗内地址解析通过，窗外 EFAULT 族）。
pub const BUF_LIMIT: u64 = 0x2000;

/// stat 缓冲区宽度（单槽字节数，地址须 8 对齐且整槽落在窗内）。
pub const STAT_BUF_WIDTH: u64 = 16;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StatCode {
    /// 零地址缓冲（EFAULT 族）
    NullBuffer,
    /// 未对齐缓冲（EFAULT 族）
    UnalignedBuffer,
    /// 缓冲越窗（EFAULT 族）
    OutOfRangeBuffer,
    /// 坏 fd（EBADF 族）
    BadFd,
    /// 重复打开同 fd
    DupFd,
    /// 关闭态上取元数据
    ClosedFd,
    /// 冻结后仍截断
    FrozenTruncate,
    /// 截断长度回退（size 只增不减）
    ShrinkTruncate,
    /// 元数据快照为空
    EmptySnapshot,
    /// 账本容量满
    LedgerFull,
    /// fd 号为零（保留键）
    ZeroFd,
    /// 段位越界
    SegmentViolation,
}

impl StatCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            StatCode::NullBuffer => "零地址缓冲被拒：fstat 落点不得为空指针；\
                请先在合法窗内分配缓冲",
            StatCode::UnalignedBuffer => "未对齐缓冲被拒：落点必须 8 字节对齐；\
                请检查分配器的对齐承诺",
            StatCode::OutOfRangeBuffer => "越窗缓冲被拒：落点整槽必须落在合法窗内；\
                请核对 BUF_BASE..BUF_LIMIT 边界",
            StatCode::BadFd => "坏 fd：账本中无此描述符；请先 open 注册再取元数据",
            StatCode::DupFd => "重复打开被拒：同 fd 只能注册一次；请先关闭再重开",
            StatCode::ClosedFd => "关闭态取元数据被拒：关闭后快照不可用；请重开后 fstat",
            StatCode::FrozenTruncate => "冻结后截断被拒：冻结态账面只读；\
                截断前请先解冻或换新 fd",
            StatCode::ShrinkTruncate => "收缩截断被拒：本档 size 只增不减；\
                收缩语义走独立档位核验",
            StatCode::EmptySnapshot => "空快照被拒：fstat 结果不得为空；请检查 fd 是否在账",
            StatCode::LedgerFull => "账本容量满：fd 槽位耗尽；请先关闭闲置 fd",
            StatCode::ZeroFd => "fd 号为零：零号是保留键，禁止入账；请换用正数 fd",
            StatCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [StatCode; 12] = [
        StatCode::NullBuffer,
        StatCode::UnalignedBuffer,
        StatCode::OutOfRangeBuffer,
        StatCode::BadFd,
        StatCode::DupFd,
        StatCode::ClosedFd,
        StatCode::FrozenTruncate,
        StatCode::ShrinkTruncate,
        StatCode::EmptySnapshot,
        StatCode::LedgerFull,
        StatCode::ZeroFd,
        StatCode::SegmentViolation,
    ];
}

// ── 文件态机 ──────────────────────────────────────────────────────────
/// 文件生命周期态：Closed → Open → Truncated → Frozen。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileStage {
    /// 关闭态：快照不可用
    Closed,
    /// 打开态：可 fstat 可截断
    Open,
    /// 截断态：size/mtime 已被 ftruncate 改写
    Truncated,
    /// 冻结态：账面只读
    Frozen,
}

impl FileStage {
    /// 态序（0..=3），用于单调推进校验。
    pub fn rank(self) -> u8 {
        match self {
            FileStage::Closed => 0,
            FileStage::Open => 1,
            FileStage::Truncated => 2,
            FileStage::Frozen => 3,
        }
    }
}

// ── 缓冲区地址解析器 ──────────────────────────────────────────────────
/// 解析缓冲区落点：地址非零、8 字节对齐且整槽落在 [BUF_BASE, BUF_LIMIT)。
/// 任何一档不满足都精确归因到对应诊断码，不静默归并为笼统 EFAULT。
pub fn parse_buffer(addr: u64) -> Result<(), StatCode> {
    if addr == 0 {
        return Err(StatCode::NullBuffer);
    }
    if addr % 8 != 0 {
        return Err(StatCode::UnalignedBuffer);
    }
    if addr < BUF_BASE || addr + STAT_BUF_WIDTH > BUF_LIMIT {
        return Err(StatCode::OutOfRangeBuffer);
    }
    Ok(())
}

// ── 元数据账本 ────────────────────────────────────────────────────────
/// 单 fd 元数据记录。
#[derive(Clone, Copy, Debug)]
pub struct FileMeta {
    /// 描述符号
    pub fd: u32,
    /// 文件大小（字节，ftruncate 后刷新）
    pub size: u64,
    /// 修改时刻（每次截断单调 +1，保证两次截断可区分）
    pub mtime: u64,
    /// 硬链计数
    pub nlink: u32,
    /// 生命周期态
    pub stage: FileStage,
}

/// fstat 快照（只读投影）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StatBuf {
    /// 快照大小
    pub size: u64,
    /// 快照时刻
    pub mtime: u64,
    /// 快照链数
    pub nlink: u32,
}

/// 元数据账本：fd→元数据；截断即改账，fstat 只读不改账。
#[derive(Clone, Debug)]
pub struct MetaLedger {
    files: Vec<FileMeta>,
    cap: usize,
}

impl MetaLedger {
    /// 空账构造（容量 LEDGER_CAP）。
    pub fn new() -> MetaLedger {
        MetaLedger { files: Vec::new(), cap: LEDGER_CAP }
    }

    /// 打开注册：零号拒绝、重复拒绝、容量满拒绝；注册即 Open 态。
    pub fn open(&mut self, fd: u32, init_size: u64) -> Result<(), StatCode> {
        if fd == 0 {
            return Err(StatCode::ZeroFd);
        }
        if self.files.len() >= self.cap {
            return Err(StatCode::LedgerFull);
        }
        let mut i = 0usize;
        while i < self.files.len() {
            if let Some(f) = self.files.get(i) {
                if f.fd == fd {
                    return Err(StatCode::DupFd);
                }
            }
            i += 1;
        }
        self.files.push(FileMeta {
            fd,
            size: init_size,
            mtime: 1,
            nlink: 1,
            stage: FileStage::Open,
        });
        Ok(())
    }

    /// 在账 fd 数。
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// ftruncate：Open/Truncated 态可用；size 只增不减；截断刷新 mtime 并入 Truncated 态。
    pub fn ftruncate(&mut self, fd: u32, new_size: u64) -> Result<(), StatCode> {
        let mut i = 0usize;
        while i < self.files.len() {
            if let Some(f) = self.files.get_mut(i) {
                if f.fd == fd {
                    if f.stage == FileStage::Closed {
                        return Err(StatCode::ClosedFd);
                    }
                    if f.stage == FileStage::Frozen {
                        return Err(StatCode::FrozenTruncate);
                    }
                    if new_size < f.size {
                        return Err(StatCode::ShrinkTruncate);
                    }
                    if new_size > f.size {
                        f.size = new_size;
                    }
                    f.mtime = f.mtime.saturating_add(1);
                    f.stage = FileStage::Truncated;
                    return Ok(());
                }
            }
            i += 1;
        }
        Err(StatCode::BadFd)
    }

    /// fstat：fd 必须在账且非 Closed，缓冲落点必须过解析器；返回只读快照。
    pub fn fstat(&self, fd: u32, buf_addr: u64) -> Result<StatBuf, StatCode> {
        parse_buffer(buf_addr)?;
        let mut i = 0usize;
        while i < self.files.len() {
            if let Some(f) = self.files.get(i) {
                if f.fd == fd {
                    if f.stage == FileStage::Closed {
                        return Err(StatCode::ClosedFd);
                    }
                    return Ok(StatBuf { size: f.size, mtime: f.mtime, nlink: f.nlink });
                }
            }
            i += 1;
        }
        Err(StatCode::BadFd)
    }

    /// 冻结：Truncated → Frozen，此后 fstat 仍可读、ftruncate 拒绝。
    pub fn freeze(&mut self, fd: u32) -> Result<(), StatCode> {
        let mut i = 0usize;
        while i < self.files.len() {
            if let Some(f) = self.files.get_mut(i) {
                if f.fd == fd {
                    if f.stage == FileStage::Closed {
                        return Err(StatCode::ClosedFd);
                    }
                    f.stage = FileStage::Frozen;
                    return Ok(());
                }
            }
            i += 1;
        }
        Err(StatCode::BadFd)
    }

    /// 关闭：态置 Closed（保留记录以便审计），fstat/截断均拒绝。
    pub fn close(&mut self, fd: u32) -> Result<(), StatCode> {
        let mut i = 0usize;
        while i < self.files.len() {
            if let Some(f) = self.files.get_mut(i) {
                if f.fd == fd {
                    f.stage = FileStage::Closed;
                    return Ok(());
                }
            }
            i += 1;
        }
        Err(StatCode::BadFd)
    }

    /// 单条元数据读取（审计用）。
    pub fn meta(&self, fd: u32) -> Option<FileMeta> {
        let mut i = 0usize;
        while i < self.files.len() {
            if let Some(f) = self.files.get(i) {
                if f.fd == fd {
                    return Some(*f);
                }
            }
            i += 1;
        }
        None
    }
}

// ── J1 达成核：ftruncate 后 fstat 元数据一致（10/10 轮） ───────────────
/// 逐轮以递增长度截断并回读快照；size/mtime 必须逐轮一致，返回达成轮数。
pub fn run_metadata_rounds() -> (u32, Vec<(u64, u64)>) {
    let mut l = MetaLedger::new();
    let _ = l.open(7, 64);
    let mut ok = 0u32;
    let mut snaps: Vec<(u64, u64)> = Vec::new();
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        // 长度推导：64 + 16*round，单调递增，不触收缩档。
        let target = 64u64 + 16u64 * round as u64;
        let trunc_ok = l.ftruncate(7, target).is_ok();
        let snap_ok = match l.fstat(7, 0x1010) {
            Ok(s) => {
                // mtime 推导：open 置 1，每轮截断再 +1。
                let expect_mtime = (1 + round + 1) as u64;
                s.size == target && s.mtime == expect_mtime && s.nlink == 1
            }
            Err(_) => false,
        };
        if trunc_ok && snap_ok {
            ok += 1;
        }
        if let Ok(s) = l.fstat(7, 0x1010) {
            let _ = snaps.push((s.size, s.mtime));
        }
        round += 1;
    }
    (ok, snaps)
}

// ── 注入核：坏缓冲样本矩阵 ────────────────────────────────────────────
/// 四类坏缓冲样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadBuffer {
    /// 零地址
    NullAddr,
    /// 未对齐
    Unaligned,
    /// 越窗（低于窗基址）
    BelowWindow,
    /// 坏 fd（缓冲合法但 fd 不在账）
    BadFd,
}

impl BadBuffer {
    /// 闭集全量。
    pub const ALL: [BadBuffer; 4] = [
        BadBuffer::NullAddr,
        BadBuffer::Unaligned,
        BadBuffer::BelowWindow,
        BadBuffer::BadFd,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadBuffer::NullAddr => "零地址缓冲",
            BadBuffer::Unaligned => "未对齐缓冲",
            BadBuffer::BelowWindow => "越窗缓冲",
            BadBuffer::BadFd => "账外 fd 取元数据",
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0。
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

    /// 记一次注入结果；账失守恒返回 SegmentViolation。
    pub fn record(&mut self, caught: bool) -> Result<(), StatCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(StatCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮坏缓冲注入：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
pub fn run_bad_buffer_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadBuffer::ALL.len() {
        let case = match BadBuffer::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut l = MetaLedger::new();
            let _ = l.open(3, 32);
            let caught = match case {
                BadBuffer::NullAddr => l.fstat(3, 0).err() == Some(StatCode::NullBuffer),
                BadBuffer::Unaligned => {
                    l.fstat(3, 0x1013).err() == Some(StatCode::UnalignedBuffer)
                }
                BadBuffer::BelowWindow => {
                    l.fstat(3, 0x800).err() == Some(StatCode::OutOfRangeBuffer)
                }
                BadBuffer::BadFd => l.fstat(99, 0x1010).err() == Some(StatCode::BadFd),
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── 快照成文（纯函数） ────────────────────────────────────────────────
/// 快照成文：「fd=<n> size=<s> mtime=<t> nlink=<k>」；账外/空快照先拒。
pub fn compose_stat_summary(l: &MetaLedger, fd: u32) -> Result<String, StatCode> {
    let m = match l.meta(fd) {
        Some(m) if m.stage != FileStage::Closed => m,
        _ => return Err(StatCode::EmptySnapshot),
    };
    let out = format!("fd={} size={} mtime={} nlink={}", m.fd, m.size, m.mtime, m.nlink);
    if out.is_empty() {
        return Err(StatCode::EmptySnapshot);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EFAULT", -14, 998, "Windows ERROR_NOACCESS；坏缓冲落点族"),
    ("EBADF", -9, 10038, "Windows WSAENOTSOCK 近似；账外 fd 族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；非法长度族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；账外路径族"),
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；冻结态写操作族"),
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

/// 规范语料双跑指纹：双 fd 注册 → 截断 → 快照 → 冻结 → 逐条成文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut l = MetaLedger::new();
    let _ = l.open(11, 128);
    let _ = l.open(12, 256);
    let _ = l.ftruncate(11, 512);
    let _ = l.ftruncate(11, 640);
    let _ = l.ftruncate(12, 384);
    let _ = l.freeze(12);
    let mut bytes: Vec<u8> = Vec::new();
    let mut e = 0u32;
    while e < 2 {
        if let Ok(s) = compose_stat_summary(&l, 11 + e) {
            bytes.extend_from_slice(s.as_bytes());
            bytes.push(b';');
        }
        e += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10110 判据集（15 条；动态明细 ≤8 条）。
pub fn run_unx10110_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10110");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92B40..=0x92B7F).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < StatCode::ALL.len() {
        let mut j = i + 1;
        while j < StatCode::ALL.len() {
            if let (Some(a), Some(b)) = (StatCode::ALL.get(i), StatCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = StatCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. 缓冲解析器：窗内合法点通过，边界逐档归因
    //    0x1FF8 对齐但 0x1FF8+0x10 越尾 → OutOfRange；0x1FFF 未对齐 → Unaligned。
    let parse_ok = parse_buffer(0x1000).is_ok()
        && parse_buffer(0x1010).is_ok()
        && parse_buffer(BUF_LIMIT - STAT_BUF_WIDTH).is_ok()
        && parse_buffer(BUF_LIMIT - 8).err() == Some(StatCode::OutOfRangeBuffer)
        && parse_buffer(BUF_LIMIT - 1).err() == Some(StatCode::UnalignedBuffer);
    s.add("缓冲解析器：窗内整槽通过且窗外逐点拒绝", parse_ok,
          "解析器是 fstat 前置，落点归因必须精确到档");

    // 4. 打开注册：零号/重复拒绝
    let mut l = MetaLedger::new();
    let open_ok = l.open(7, 64).is_ok() && l.len() == 1
        && l.open(0, 8).err() == Some(StatCode::ZeroFd)
        && l.open(7, 16).err() == Some(StatCode::DupFd);
    s.add("打开注册：零号与重复 fd 显性拒绝", open_ok,
          "零号是保留键；重复注册是账目事故，必须拒绝");

    // 5. J1 达成核：ftruncate 后 fstat 元数据一致 10/10
    let (ok, snaps) = run_metadata_rounds();
    s.add("J1 元数据达成：ftruncate→fstat size/mtime 一致 10/10 次",
          ok == INJECT_ROUNDS && snaps.len() == INJECT_ROUNDS as usize,
          "截断后快照必须与账面元数据逐字段一致，任何一轮漂移即失守");

    // 6. 幂等：同一时刻连续 fstat 两次逐字段一致
    let s1 = l.fstat(7, 0x1010);
    let s2 = l.fstat(7, 0x1010);
    s.add("fstat 幂等：同态两次快照逐字段一致", s1 == s2 && s1.is_ok(),
          "fstat 是只读投影，不得有隐藏状态推进");

    // 7. 截断单调：size 只增不减，回退被拒且账不变
    let before = l.meta(7).map(|m| m.size).unwrap_or(0);
    let shrink = l.ftruncate(7, 8).err() == Some(StatCode::ShrinkTruncate);
    let after = l.meta(7).map(|m| m.size).unwrap_or(0);
    s.add("截断单调：收缩被拒且账面 size 不变", shrink && before == after && before >= 64,
          "本档截断只增不减；被拒操作不得改动任何已留痕账目");

    // 8. 冻结态：fstat 仍可读、ftruncate 拒绝
    let mut l2 = MetaLedger::new();
    let _ = l2.open(9, 32);
    let _ = l2.ftruncate(9, 48);
    let _ = l2.freeze(9);
    let frozen_ok = l2.fstat(9, 0x1010).is_ok()
        && l2.ftruncate(9, 64).err() == Some(StatCode::FrozenTruncate)
        && l2.meta(9).map(|m| m.stage) == Some(FileStage::Frozen);
    s.add("冻结态：读放行写拒绝且态位准确", frozen_ok,
          "冻结是账面只读开关；冻结后截断必须逐次拦截");

    // 9. 注入核：四类坏缓冲样本各 10/10 拒绝
    let (inj, per_class) = run_bad_buffer_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏缓冲样本各 10/10 拒绝", all_caught,
          "零地址/未对齐/越窗/坏 fd 必须逐类逐次精确复现");

    // 10. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadBuffer::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 11. J2 精确归因：EFAULT 族逐档、EBADF 逐次
    let fault_ok = l2.fstat(9, 0).err() == Some(StatCode::NullBuffer)
        && l2.fstat(9, 0x1013).err() == Some(StatCode::UnalignedBuffer)
        && l2.fstat(9, 0x1FF8).err() == Some(StatCode::OutOfRangeBuffer)
        && l2.fstat(123, 0x1010).err() == Some(StatCode::BadFd);
    s.add("J2 坏缓冲受控：EFAULT/EBADF 族逐类精确复现", fault_ok,
          "误用路径必须可复现且归因到具体档，不许静默吞并");

    // 12. 关闭态：close 后 fstat/截断均拒绝
    let mut l3 = MetaLedger::new();
    let _ = l3.open(5, 16);
    let _ = l3.close(5);
    let closed_ok = l3.fstat(5, 0x1010).err() == Some(StatCode::ClosedFd)
        && l3.ftruncate(5, 32).err() == Some(StatCode::ClosedFd)
        && l3.fstat(5, 0).err() == Some(StatCode::NullBuffer);
    s.add("关闭态：关后读写拒绝且解析器仍前置", closed_ok,
          "关闭即快照不可用；前置解析器不得因态跳过");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(14) && agrees_with_linux(9) && agrees_with_linux(22)
        && agrees_with_linux(2) && agrees_with_linux(1)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 14. 成文非空且含四字段 + 双跑一致
    let sum = compose_stat_summary(&l, 7);
    let text_ok = match &sum {
        Ok(t) => t.contains("fd=7") && t.contains("size=") && t.contains("mtime=")
            && t.contains("nlink=1"),
        Err(_) => false,
    };
    s.add("快照成文：四字段齐备且两次成文逐字节一致",
          text_ok && sum == compose_stat_summary(&l, 7),
          "成文必须携带 fd/size/mtime/nlink 标记且为纯函数");

    // 15. 双跑指纹
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑指纹：规范语料两次哈希逐位一致", f1 == f2,
          "同语料两次哈希必须逐位一致，隐藏状态即失败");

    // 动态明细（≤8）：注入账快照 + 指纹 + 达成轮数
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let ok_text = format!("元数据达成轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_parser_guards() {
        assert!(parse_buffer(0x1000).is_ok());
        assert_eq!(parse_buffer(0).err(), Some(StatCode::NullBuffer));
        assert_eq!(parse_buffer(0x1013).err(), Some(StatCode::UnalignedBuffer));
        assert_eq!(parse_buffer(0x1FF8).err(), Some(StatCode::OutOfRangeBuffer));
    }

    #[test]
    fn open_guards() {
        let mut l = MetaLedger::new();
        assert!(l.open(1, 8).is_ok());
        assert_eq!(l.open(0, 8).err(), Some(StatCode::ZeroFd));
        assert!(l.open(1, 16).is_err());
    }

    #[test]
    fn truncate_updates_snapshot() {
        let mut l = MetaLedger::new();
        let _ = l.open(2, 64);
        assert!(l.ftruncate(2, 128).is_ok());
        let s = l.fstat(2, 0x1010).unwrap_or(StatBuf { size: 0, mtime: 0, nlink: 0 });
        assert_eq!(s.size, 128);
        assert_eq!(s.mtime, 2);
    }

    #[test]
    fn shrink_rejected() {
        let mut l = MetaLedger::new();
        let _ = l.open(2, 64);
        let _ = l.ftruncate(2, 128);
        assert_eq!(l.ftruncate(2, 64).err(), Some(StatCode::ShrinkTruncate));
    }

    #[test]
    fn freeze_blocks_write() {
        let mut l = MetaLedger::new();
        let _ = l.open(3, 8);
        let _ = l.freeze(3);
        assert!(l.fstat(3, 0x1010).is_ok());
        assert_eq!(l.ftruncate(3, 16).err(), Some(StatCode::FrozenTruncate));
    }

    #[test]
    fn closed_fd_rejected() {
        let mut l = MetaLedger::new();
        let _ = l.open(4, 8);
        let _ = l.close(4);
        assert_eq!(l.fstat(4, 0x1010).err(), Some(StatCode::ClosedFd));
    }

    #[test]
    fn bad_buffer_matrix_green() {
        let (inj, per) = run_bad_buffer_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn metadata_rounds_full() {
        let (ok, _) = run_metadata_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(14));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(9), -9);
    }

    #[test]
    fn unx10110_checks_green() {
        let set = run_unx10110_checks();
        assert!(set.all_passed(), "UNX-F10110 red: {}", set.red_items_text());
    }
}
