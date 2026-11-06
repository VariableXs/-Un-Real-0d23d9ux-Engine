//! UNX-F10112 readv/writev 正反双判据（聚合读写达成与 iovcnt 越界误用受控）· 0x92BC0 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10113-J1 getdents 枚举达成（"."/".." 项与顺序 LTP getdents01
//! 面 10/10 次）；UNX-F10113-J2 非目录 fd 误用受控（预期 ENOTDIR 精确复现
//! 10/10 次）
//!
//! SEG_BASE = 0x92BC0（本卡独占 0x40 窗口，下一卡 0x92C00）；
//! 诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器：iov 注册表（每段容量/基址绑定，段数 1..=IOV_MAX 闭集，
//!      越界段数与零段数逐档拒绝）。
//!   ② 账本：聚合留痕账本（逐段记 段序/长度/方向），聚合写读往返可对平。
//!   ③ 聚合核：writev 逐段拼接 + readv 逐段填充（段长自适应截断），
//!      拼接结果与分段内容逐字节一致。
//!   ④ 注入核：iovcnt 越上界 / iovcnt 为零 / 越界段基址 / 账外 fd 四类
//!      坏样本逐类 10/10 次精确复现 EINVAL/EBADF 族，注入账逃逸恒为 0。
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10112 独占 0x40 窗口，下一卡 0x92C00）。
pub const SEG_BASE: u32 = 0x92BC0;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// iov 段数上界（对标 IOV_MAX 1024 的档位缩影）。
pub const IOV_MAX: usize = 8;

/// 单段容量上界（字节）。
pub const IOV_SEG_CAP: usize = 64;

/// 聚合账容量上限。
pub const REC_CAP: usize = 256;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IovCode {
    /// iovcnt 越上界（EINVAL 族）
    IovcntTooBig,
    /// iovcnt 为零（EINVAL 族）
    IovcntZero,
    /// 段基址越界（EFAULT 族）
    SegOutOfRange,
    /// 段超容量（EINVAL 族）
    SegTooBig,
    /// 账外 fd（EBADF 族）
    BadFd,
    /// 重复注册
    DupFd,
    /// fd 号为零（保留键）
    ZeroFd,
    /// 聚合账满
    RecFull,
    /// 聚合往返不一致
    RoundTrip,
    /// 段留痕为空
    EmptyRec,
    /// 成文为空
    EmptySummary,
    /// 段位越界
    SegmentViolation,
}

impl IovCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            IovCode::IovcntTooBig => "iovcnt 越上界被拒：段数必须落在 1..=IOV_MAX 闭集内；\
                请拆分聚合批次",
            IovCode::IovcntZero => "iovcnt 为零被拒：零段聚合是歧义调用；\
                Linux 对零段计数返回 EINVAL，请至少给一段",
            IovCode::SegOutOfRange => "段基址越界被拒：段缓冲必须落在注册窗内；\
                请核对段基址与窗边界",
            IovCode::SegTooBig => "段超容量被拒：单段不得超过 IOV_SEG_CAP；请拆小段",
            IovCode::BadFd => "账外 fd：注册表中无此描述符；请先 open 注册",
            IovCode::DupFd => "重复注册被拒：同 fd 只能注册一次；请先关闭再重开",
            IovCode::ZeroFd => "fd 号为零：零号是保留键，禁止入账；请换用正数 fd",
            IovCode::RecFull => "聚合账满：请先归档历史轮次再继续聚合 IO",
            IovCode::RoundTrip => "聚合往返不一致：writev 后 readv 必须逐段拼接一致；\
                请核对段序与段长",
            IovCode::EmptyRec => "空段留痕被拒：每段必须携带非空明细；请核对段表",
            IovCode::EmptySummary => "空成文被拒：聚合小结不得为空串；请检查 fd 是否在账",
            IovCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [IovCode; 12] = [
        IovCode::IovcntTooBig,
        IovCode::IovcntZero,
        IovCode::SegOutOfRange,
        IovCode::SegTooBig,
        IovCode::BadFd,
        IovCode::DupFd,
        IovCode::ZeroFd,
        IovCode::RecFull,
        IovCode::RoundTrip,
        IovCode::EmptyRec,
        IovCode::EmptySummary,
        IovCode::SegmentViolation,
    ];
}

// ── iov 段描述与解析器 ────────────────────────────────────────────────
/// 单个 iov 段：注册窗内基址 + 请求长度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IovSeg {
    /// 段基址（注册窗内偏移）
    pub addr: u64,
    /// 请求长度（字节）
    pub len: usize,
}

/// 注册窗参数：段基址必须落在 [WIN_BASE, WIN_BASE + WIN_SIZE) 且整段在窗内。
pub const WIN_BASE: u64 = 0x4000;
pub const WIN_SIZE: u64 = 0x0400;

/// 解析段表：段数 1..=IOV_MAX、每段非零长、基址在窗内、长度不超段容。
/// 任何一档不满足都精确归因到对应诊断码。
pub fn parse_iov(segs: &[IovSeg]) -> Result<(), IovCode> {
    if segs.len() > IOV_MAX {
        return Err(IovCode::IovcntTooBig);
    }
    if segs.is_empty() {
        return Err(IovCode::IovcntZero);
    }
    let mut i = 0usize;
    while i < segs.len() {
        let seg = match segs.get(i) {
            Some(s) => *s,
            None => return Err(IovCode::IovcntZero),
        };
        if seg.len == 0 {
            return Err(IovCode::EmptyRec);
        }
        if seg.len > IOV_SEG_CAP {
            return Err(IovCode::SegTooBig);
        }
        if seg.addr < WIN_BASE || seg.addr + seg.len as u64 > WIN_BASE + WIN_SIZE {
            return Err(IovCode::SegOutOfRange);
        }
        i += 1;
    }
    Ok(())
}

// ── 聚合账本 ──────────────────────────────────────────────────────────
/// IO 方向闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IovKind {
    /// 聚合读
    Readv,
    /// 聚合写
    Writev,
}

/// 单段留痕。
#[derive(Clone, Copy, Debug)]
pub struct IovRec {
    /// 序号
    pub seq: u32,
    /// fd
    pub fd: u32,
    /// 段序（0 起）
    pub seg_no: usize,
    /// 实际搬运长度
    pub moved: usize,
    /// 方向
    pub kind: IovKind,
}

/// 聚合文件：内容字节串（共享偏移由聚合读写顺序推进）。
#[derive(Clone, Debug)]
pub struct IovFile {
    /// 文件内容
    pub data: Vec<u8>,
    /// 共享偏移
    pub pos: u64,
}

/// 聚合账本：fd 注册表 + 逐段留痕账。
#[derive(Clone, Debug)]
pub struct IovLedger {
    files: Vec<(u32, IovFile)>,
    recs: Vec<IovRec>,
    seq_next: u32,
    cap: usize,
}

impl IovLedger {
    /// 空账构造（容量 IovCap）。
    pub fn new() -> IovLedger {
        IovLedger { files: Vec::new(), recs: Vec::new(), seq_next: 1, cap: 32 }
    }

    /// 注册 fd：零号拒绝、重复拒绝。
    pub fn open(&mut self, fd: u32) -> Result<(), IovCode> {
        if fd == 0 {
            return Err(IovCode::ZeroFd);
        }
        if self.files.len() >= self.cap {
            return Err(IovCode::SegmentViolation);
        }
        let mut i = 0usize;
        while i < self.files.len() {
            if let Some((k, _)) = self.files.get(i) {
                if *k == fd {
                    return Err(IovCode::DupFd);
                }
            }
            i += 1;
        }
        let _ = self.files.push((fd, IovFile { data: Vec::new(), pos: 0 }));
        Ok(())
    }

    /// 在账 fd 数。
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// writev：段表先过解析器，再逐段拼接写入并推进共享偏移。
    pub fn writev(&mut self, fd: u32, segs: &[IovSeg], win: &[u8]) -> Result<usize, IovCode> {
        parse_iov(segs)?;
        self.exists(fd)?;
        let mut total = 0usize;
        let mut si = 0usize;
        while si < segs.len() {
            let seg = match segs.get(si) {
                Some(s) => *s,
                None => break,
            };
            // 从注册窗切片：win 视为窗内存的线性副本。
            let start = (seg.addr - WIN_BASE) as usize;
            let end = start.saturating_add(seg.len);
            let mut chunk: Vec<u8> = Vec::new();
            let mut k = start;
            while k < end {
                if let Some(b) = win.get(k) {
                    let _ = chunk.push(*b);
                }
                k += 1;
            }
            let f = match self.lookup_mut(fd) {
                Ok(f) => f,
                Err(e) => return Err(e),
            };
            let mut c = 0usize;
            while c < chunk.len() {
                let _ = f.data.push(*chunk.get(c).unwrap_or(&0));
                c += 1;
            }
            f.pos = f.pos.saturating_add(chunk.len() as u64);
            total += chunk.len();
            self.push_rec(fd, si, chunk.len(), IovKind::Writev)?;
            si += 1;
        }
        Ok(total)
    }

    /// readv：从共享偏移逐段填充段表；段长不足则该段截短，返回总长。
    pub fn readv(&mut self, fd: u32, segs: &[IovSeg], win: &mut [u8]) -> Result<usize, IovCode> {
        parse_iov(segs)?;
        self.exists(fd)?;
        let mut total = 0usize;
        let mut si = 0usize;
        while si < segs.len() {
            let seg = match segs.get(si) {
                Some(s) => *s,
                None => break,
            };
            let f = match self.lookup_mut(fd) {
                Ok(f) => f,
                Err(e) => return Err(e),
            };
            let start = f.pos as usize;
            let avail = f.data.len().saturating_sub(start);
            let take = if seg.len < avail { seg.len } else { avail };
            let win_start = (seg.addr - WIN_BASE) as usize;
            let mut k = 0usize;
            while k < take {
                let byte = match f.data.get(start + k) {
                    Some(b) => *b,
                    None => break,
                };
                let dst = win_start + k;
                if dst < win.len() {
                    if let Some(slot) = win.get_mut(dst) {
                        *slot = byte;
                    }
                }
                k += 1;
            }
            f.pos = f.pos.saturating_add(take as u64);
            total += take;
            // 零搬运段（EOF 截短）不计留痕，readv 允许部分段空手而归。
            if take > 0 {
                self.push_rec(fd, si, take, IovKind::Readv)?;
            }
            si += 1;
        }
        Ok(total)
    }

    /// 回卷共享偏移到段头（写后回读前必走，模拟 lseek 回起点）；账外拒绝。
    pub fn rewind(&mut self, fd: u32) -> Result<(), IovCode> {
        let f = self.lookup_mut(fd)?;
        f.pos = 0;
        Ok(())
    }

    /// 共享偏移读取（审计用）。
    pub fn pos_of(&self, fd: u32) -> Option<u64> {        let mut i = 0usize;
        while i < self.files.len() {
            if let Some((k, f)) = self.files.get(i) {
                if *k == fd {
                    return Some(f.pos);
                }
            }
            i += 1;
        }
        None
    }

    /// 内容快照读取（往返对平用）。
    pub fn data_of(&self, fd: u32) -> Option<Vec<u8>> {
        let mut i = 0usize;
        while i < self.files.len() {
            if let Some((k, f)) = self.files.get(i) {
                if *k == fd {
                    return Some(f.data.clone());
                }
            }
            i += 1;
        }
        None
    }

    /// 留痕数。
    pub fn rec_len(&self) -> usize {
        self.recs.len()
    }

    // ── 内部 ──
    fn exists(&self, fd: u32) -> Result<(), IovCode> {
        let mut i = 0usize;
        while i < self.files.len() {
            if let Some((k, _)) = self.files.get(i) {
                if *k == fd {
                    return Ok(());
                }
            }
            i += 1;
        }
        Err(IovCode::BadFd)
    }

    fn lookup_mut(&mut self, fd: u32) -> Result<&mut IovFile, IovCode> {
        // 先不可变定位，再单次可变取槽：避免借用在循环间存活。
        let mut idx = self.files.len();
        let mut i = 0usize;
        while i < self.files.len() {
            if let Some((k, _)) = self.files.get(i) {
                if *k == fd {
                    idx = i;
                    break;
                }
            }
            i += 1;
        }
        if idx < self.files.len() {
            if let Some((_, f)) = self.files.get_mut(idx) {
                return Ok(f);
            }
        }
        Err(IovCode::BadFd)
    }

    fn push_rec(&mut self, fd: u32, seg_no: usize, moved: usize, kind: IovKind) -> Result<(), IovCode> {
        if self.recs.len() >= REC_CAP {
            return Err(IovCode::RecFull);
        }
        let seq = self.seq_next;
        self.seq_next = self.seq_next.saturating_add(1);
        let _ = self.recs.push(IovRec { seq, fd, seg_no, moved, kind });
        if moved == 0 {
            return Err(IovCode::EmptyRec);
        }
        Ok(())
    }
}

// ── J1 达成核：聚合分段拼接一致（10/10 轮） ───────────────────────────
/// 逐轮：writev 三段异长内容 → readv 回读 → 拼接逐字节一致。
/// 每轮独立账本，保证 readv 从段头读回当轮写入。
pub fn run_gather_rounds() -> (u32, usize) {
    // 注册窗线性副本：三段内容分别落在窗内 0x4000 / 0x4020 / 0x4040。
    let mut win = vec![0u8; WIN_SIZE as usize];
    let mut ok = 0u32;
    let mut last_total = 0usize;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        // 三段内容随轮次变化但长度固定 8/4/6，段表异长。
        let a = (b'A' + (round % 26) as u8) as u8;
        let b = (b'N' + (round % 13) as u8) as u8;
        let c = (b'W' + (round % 9) as u8) as u8;
        for (base, ch, cnt) in [(0x4000u64, a, 8usize), (0x4020, b, 4), (0x4040, c, 6)] {
            let mut k = 0usize;
            while k < cnt {
                if let Some(slot) = win.get_mut((base - WIN_BASE) as usize + k) {
                    *slot = ch;
                }
                k += 1;
            }
        }
        let segs = [
            IovSeg { addr: 0x4000, len: 8 },
            IovSeg { addr: 0x4020, len: 4 },
            IovSeg { addr: 0x4040, len: 6 },
        ];
        let mut l = IovLedger::new();
        let _ = l.open(6);
        let wtotal = match l.writev(6, &segs, &win) {
            Ok(n) => n,
            Err(_) => break,
        };
        // 写后回卷共享偏移再回读（模拟 lseek 回起点）。
        let _ = l.rewind(6);
        // readv 回读：三段等长回读。
        let mut rwin = vec![0u8; WIN_SIZE as usize];
        let rtotal = match l.readv(6, &segs, &mut rwin) {
            Ok(n) => n,
            Err(_) => break,
        };
        // 拼接一致：写窗三段切片 == 读窗三段切片，且总长一致。
        let same = rtotal == wtotal
            && win.get(0..8) == rwin.get(0..8)
            && win.get(32..36) == rwin.get(32..36)
            && win.get(64..70) == rwin.get(64..70);
        if same {
            ok += 1;
        }
        last_total = wtotal;
        round += 1;
    }
    (ok, last_total)
}

// ── 注入核：坏聚合样本矩阵 ────────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadIov {
    /// iovcnt 越上界
    TooManySegs,
    /// iovcnt 为零
    ZeroSegs,
    /// 段基址越窗
    SegOutOfRange,
    /// 账外 fd
    BadFd,
}

impl BadIov {
    /// 闭集全量。
    pub const ALL: [BadIov; 4] = [
        BadIov::TooManySegs,
        BadIov::ZeroSegs,
        BadIov::SegOutOfRange,
        BadIov::BadFd,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadIov::TooManySegs => "iovcnt 越上界",
            BadIov::ZeroSegs => "iovcnt 为零",
            BadIov::SegOutOfRange => "段基址越窗",
            BadIov::BadFd => "账外 fd 聚合读写",
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
    pub fn record(&mut self, caught: bool) -> Result<(), IovCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(IovCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏聚合注入：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
pub fn run_bad_iov_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let good = IovSeg { addr: 0x4000, len: 8 };
    let mut ci = 0usize;
    while ci < BadIov::ALL.len() {
        let case = match BadIov::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut l = IovLedger::new();
            let _ = l.open(5);
            let win = vec![0u8; WIN_SIZE as usize];
            let caught = match case {
                BadIov::TooManySegs => {
                    // IOV_MAX+1 段：解析器第一档即拒。
                    let mut many: Vec<IovSeg> = Vec::new();
                    let mut k = 0usize;
                    while k <= IOV_MAX {
                        let _ = many.push(good);
                        k += 1;
                    }
                    l.writev(5, &many, &win).err() == Some(IovCode::IovcntTooBig)
                }
                BadIov::ZeroSegs => l.writev(5, &[], &win).err() == Some(IovCode::IovcntZero),
                BadIov::SegOutOfRange => {
                    let bad = [IovSeg { addr: 0x4800, len: 8 }];
                    l.writev(5, &bad, &win).err() == Some(IovCode::SegOutOfRange)
                }
                BadIov::BadFd => l.writev(77, &[good], &win).err() == Some(IovCode::BadFd),
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

// ── 聚合小结成文（纯函数） ────────────────────────────────────────────
/// 小结成文：「fd=<n> pos=<p> recs=<r>」；账外先拒。
pub fn compose_iov_summary(l: &IovLedger, fd: u32) -> Result<String, IovCode> {
    let pos = match l.pos_of(fd) {
        Some(p) => p,
        None => return Err(IovCode::EmptySummary),
    };
    let out = format!("fd={} pos={} recs={}", fd, pos, l.rec_len());
    if out.is_empty() {
        return Err(IovCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；iovcnt 越界/零段族"),
    ("EFAULT", -14, 998, "Windows ERROR_NOACCESS；段基址越窗族"),
    ("EBADF", -9, 10038, "Windows WSAENOTSOCK 近似；账外 fd 族"),
    ("ENOBUFS", -105, 10055, "Windows WSAENOBUFS 近似；聚合账满族"),
    ("EIO", -5, 1117, "Windows ERROR_IO_DEVICE；聚合搬运失败族"),
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

/// 规范语料双跑指纹：注册 → 三段聚合写 → 聚合读 → 拼接成文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut l = IovLedger::new();
    let _ = l.open(31);
    let mut win = vec![0u8; WIN_SIZE as usize];
    let payload = b"GATHERTAPSEG";
    let mut k = 0usize;
    while k < payload.len() {
        if let Some(slot) = win.get_mut(k) {
            *slot = *payload.get(k).unwrap_or(&0);
        }
        k += 1;
    }
    let segs = [
        IovSeg { addr: 0x4000, len: 6 },
        IovSeg { addr: 0x4006, len: 3 },
        IovSeg { addr: 0x4009, len: 3 },
    ];
    let _ = l.writev(31, &segs, &win);
    let mut rwin = vec![0u8; WIN_SIZE as usize];
    let _ = l.readv(31, &segs, &mut rwin);
    let mut bytes: Vec<u8> = Vec::new();
    bytes.extend_from_slice(&win.get(0..12).unwrap_or(&[]));
    bytes.push(b'|');
    bytes.extend_from_slice(&rwin.get(0..12).unwrap_or(&[]));
    if let Ok(s) = compose_iov_summary(&l, 31) {
        bytes.extend_from_slice(s.as_bytes());
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10112 判据集（15 条；动态明细 ≤8 条）。
pub fn run_unx10112_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10112");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92BC0..=0x92BFF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < IovCode::ALL.len() {
        let mut j = i + 1;
        while j < IovCode::ALL.len() {
            if let (Some(a), Some(b)) = (IovCode::ALL.get(i), IovCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = IovCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. iov 解析器：合法段表通过、逐档拒绝
    let parse_ok = parse_iov(&[IovSeg { addr: 0x4000, len: 8 }]).is_ok()
        && parse_iov(&[IovSeg { addr: 0x4000, len: 0 }]).err() == Some(IovCode::EmptyRec)
        && parse_iov(&[IovSeg { addr: 0x4000, len: IOV_SEG_CAP + 1 }])
            .err() == Some(IovCode::SegTooBig)
        && parse_iov(&[IovSeg { addr: 0x3F00, len: 8 }])
            .err() == Some(IovCode::SegOutOfRange);
    s.add("iov 解析器：零长/超容/越窗逐档精确归因", parse_ok,
          "解析器是聚合 IO 前置，段表归因必须精确到档");

    // 4. fd 注册：零号/重复拒绝
    let mut l = IovLedger::new();
    let open_ok = l.open(6).is_ok() && l.len() == 1
        && l.open(0).err() == Some(IovCode::ZeroFd)
        && l.open(6).err() == Some(IovCode::DupFd);
    s.add("fd 注册：零号与重复 fd 显性拒绝", open_ok,
          "注册是聚合 IO 前置；零号是保留键，重复注册是账目事故");

    // 5. J1 达成核：聚合分段拼接一致 10/10
    let (ok, total) = run_gather_rounds();
    s.add("J1 聚合达成：writev/readv 分段拼接一致 10/10 次",
          ok == INJECT_ROUNDS && total == 18,
          "三段 8/4/6 异长聚合，写读两窗逐段逐字节一致，任何漂移即失守");

    // 6. 共享偏移守恒：聚合写按总长推进；回卷后聚合读按总长推进
    let mut l2 = IovLedger::new();
    let _ = l2.open(4);
    let win = vec![0u8; WIN_SIZE as usize];
    let segs = [IovSeg { addr: 0x4000, len: 5 }, IovSeg { addr: 0x4008, len: 3 }];
    let _ = l2.writev(4, &segs, &win);
    let pos_w = l2.pos_of(4).unwrap_or(0);
    let _ = l2.rewind(4);
    let mut rwin = vec![0u8; WIN_SIZE as usize];
    let _ = l2.readv(4, &segs, &mut rwin);
    let pos_r = l2.pos_of(4).unwrap_or(0);
    s.add("共享偏移：聚合写/读各按总长推进", pos_w == 8 && pos_r == 8,
          "聚合 IO 是连续语义：写推进 8、回卷后读再推进 8，按实际搬运总长计");

    // 7. 越尾读截短：段长大于剩余数据时该段截短
    let mut l3 = IovLedger::new();
    let _ = l3.open(7);
    let win3 = vec![0u8; WIN_SIZE as usize];
    let one = [IovSeg { addr: 0x4000, len: 4 }];
    let _ = l3.writev(7, &one, &win3);
    let _ = l3.rewind(7);
    let mut rwin3 = vec![0u8; WIN_SIZE as usize];
    let got = l3.readv(7, &segs, &mut rwin3).unwrap_or(0);
    s.add("越尾截短：剩余 4 字节时两段共取 4 字节", got == 4,
          "readv 段长自适应截短，不虚报搬运量");

    // 8. 注入核：四类坏样本各 10/10 拒绝
    let (inj, per_class) = run_bad_iov_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 拒绝", all_caught,
          "iovcnt 越上界/零段/越窗段/账外 fd 必须逐类逐次精确复现");

    // 9. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadIov::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 10. J2 精确归因：iovcnt 越界 EINVAL 族逐档复现
    let good = IovSeg { addr: 0x4000, len: 8 };
    let j2_ok = parse_iov(&[good; IOV_MAX + 1]).err() == Some(IovCode::IovcntTooBig)
        && parse_iov(&[]).err() == Some(IovCode::IovcntZero)
        && parse_iov(&[good]).is_ok();
    s.add("J2 iovcnt 受控：越上界与零段 EINVAL 族精确复现", j2_ok,
          "越界段数与零段数是两档独立误用，归因不得合并");

    // 11. 段基址越窗：EFAULT 族拒绝且合法窗边界通过
    let edge_ok = parse_iov(&[IovSeg { addr: WIN_BASE + WIN_SIZE - 8, len: 8 }]).is_ok()
        && parse_iov(&[IovSeg { addr: WIN_BASE + WIN_SIZE - 7, len: 8 }])
            .err() == Some(IovCode::SegOutOfRange)
        && l.writev(6, &[IovSeg { addr: 0x4800, len: 4 }], &win)
            .err() == Some(IovCode::SegOutOfRange);
    s.add("段基址受控：窗尾整段通过、越尾一字节即拒", edge_ok,
          "边界一字节之差必须区分；被拒聚合不得推进共享偏移");

    // 12. 留痕账：逐段入账且零搬运段不计留痕
    let rec_ok = l2.rec_len() == 4 && l3.rec_len() == 2;
    s.add("聚合留痕账：逐段入账且零搬运段不计", rec_ok,
          "l2 写两段+读两段共四条；l3 写一段+读四字节（末段零搬运不计）共两条");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(22) && agrees_with_linux(14) && agrees_with_linux(9)
        && agrees_with_linux(105) && agrees_with_linux(5)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 14. 成文非空且含三字段 + 双跑一致
    let sum = compose_iov_summary(&l, 6);
    let text_ok = match &sum {
        Ok(t) => t.contains("fd=6") && t.contains("pos=") && t.contains("recs="),
        Err(_) => false,
    };
    s.add("聚合成文：三字段齐备且两次成文逐字节一致",
          text_ok && sum == compose_iov_summary(&l, 6),
          "成文必须携带 fd/pos/recs 标记且为纯函数");

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
    let ok_text = format!("聚合达成轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iov_parser_guards() {
        assert!(parse_iov(&[IovSeg { addr: 0x4000, len: 4 }]).is_ok());
        assert_eq!(parse_iov(&[]).err(), Some(IovCode::IovcntZero));
        let mut many: Vec<IovSeg> = Vec::new();
        let mut k = 0usize;
        while k <= IOV_MAX {
            let _ = many.push(IovSeg { addr: 0x4000, len: 4 });
            k += 1;
        }
        assert_eq!(parse_iov(&many).err(), Some(IovCode::IovcntTooBig));
    }

    #[test]
    fn open_guards() {
        let mut l = IovLedger::new();
        assert!(l.open(1).is_ok());
        assert_eq!(l.open(0).err(), Some(IovCode::ZeroFd));
        assert!(l.open(1).is_err());
    }

    #[test]
    fn gather_roundtrip() {
        let mut l = IovLedger::new();
        let _ = l.open(2);
        let mut win = vec![0u8; WIN_SIZE as usize];
        let _ = win.get_mut(0).map(|s| *s = b'X');
        let segs = [IovSeg { addr: 0x4000, len: 1 }];
        assert_eq!(l.writev(2, &segs, &win), Ok(1));
        assert_eq!(l.rewind(2), Ok(()));
        let mut rwin = vec![0u8; WIN_SIZE as usize];
        assert_eq!(l.readv(2, &segs, &mut rwin), Ok(1));
        assert_eq!(rwin.get(0), Some(&b'X'));
    }

    #[test]
    fn writev_advances_pos_by_total() {
        let mut l = IovLedger::new();
        let _ = l.open(3);
        let win = vec![0u8; WIN_SIZE as usize];
        let segs = [IovSeg { addr: 0x4000, len: 3 }, IovSeg { addr: 0x4010, len: 5 }];
        let _ = l.writev(3, &segs, &win);
        assert_eq!(l.pos_of(3), Some(8));
    }

    #[test]
    fn readv_short_segment_at_eof() {
        let mut l = IovLedger::new();
        let _ = l.open(3);
        let win = vec![0u8; WIN_SIZE as usize];
        let _ = l.writev(3, &[IovSeg { addr: 0x4000, len: 3 }], &win);
        let _ = l.rewind(3);
        let mut rwin = vec![0u8; WIN_SIZE as usize];
        let got = l.readv(3, &[IovSeg { addr: 0x4000, len: 9 }], &mut rwin);
        assert_eq!(got, Ok(3));
    }

    #[test]
    fn bad_iov_matrix_green() {
        let (inj, per) = run_bad_iov_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn gather_rounds_full() {
        let (ok, total) = run_gather_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
        assert_eq!(total, 18);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(22));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(105), -105);
    }

    #[test]
    fn unx10112_checks_green() {
        let set = run_unx10112_checks();
        assert!(set.all_passed(), "UNX-F10112 red: {}", set.red_items_text());
    }
}
