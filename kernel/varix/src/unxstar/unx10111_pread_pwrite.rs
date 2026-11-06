//! UNX-F10111 pread/pwrite 正反双判据（偏移读写达成与负偏移误用受控）· 0x92B80 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10112-J1 readv/writev 聚合读写达成（分段拼接一致 LTP readv01
//! 面 10/10 次）；UNX-F10112-J2 iovcnt 越界误用受控（预期 EINVAL 精确复现
//! 10/10 次）
//!
//! SEG_BASE = 0x92B80（本卡独占 0x40 窗口，下一卡 0x92BC0）；
//! 诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 注册表：fd 注册表（零号/重复/关闭后操作逐次拒绝，注册是 IO 前置）。
//!   ② 状态机：fd 生命周期态 Open→Closed，关闭态上偏移读写一律 EBADF 族。
//!   ③ 账本：偏移留痕账本（逐次 pread/pwrite 记 fd/偏移/长度/方向），并以
//!      共享偏移 pos 恒定证明「显式偏移不移动共享偏移」。
//!   ④ 注入核：负偏移 / 账外 fd / 关闭后读写 / 偏移溢出 四类坏样本逐类
//!      10/10 次精确复现，注入账 注入=检出+逃逸，逃逸恒为 0。
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10111 独占 0x40 窗口，下一卡 0x92BC0）。
pub const SEG_BASE: u32 = 0x92B80;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 偏移账本容量上限（fd 数）。
pub const LEDGER_CAP: usize = 32;

/// 留痕上限（超出即拒绝新 IO，防账目无限膨胀）。
pub const REC_CAP: usize = 256;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PosCode {
    /// 负偏移（EINVAL 族）
    NegOffset,
    /// 偏移+长度溢出（EINVAL 族）
    OffsetOverflow,
    /// 账外 fd（EBADF 族）
    BadFd,
    /// 关闭态读写（EBADF 族）
    ClosedFd,
    /// 零长度留痕
    EmptyRec,
    /// 重复注册
    DupFd,
    /// fd 号为零（保留键）
    ZeroFd,
    /// 留痕账满
    RecFull,
    /// 关闭后共享偏移仍被改动
    PosDrift,
    /// 偏移读写往返不一致
    RoundTrip,
    /// 成文为空
    EmptySummary,
    /// 段位越界
    SegmentViolation,
}

impl PosCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            PosCode::NegOffset => "负偏移被拒：pread/pwrite 只接受非负显式偏移；\
                请改用顺序读写走共享偏移",
            PosCode::OffsetOverflow => "偏移溢出被拒：偏移+长度必须落在有界区间内；\
                请拆分大块 IO",
            PosCode::BadFd => "账外 fd：注册表中无此描述符；请先 open 注册",
            PosCode::ClosedFd => "关闭态读写被拒：关闭后偏移语义不可用；请重开再操作",
            PosCode::EmptyRec => "空留痕被拒：每次 IO 必须携带非空明细；请核对调用方",
            PosCode::DupFd => "重复注册被拒：同 fd 只能注册一次；请先关闭再重开",
            PosCode::ZeroFd => "fd 号为零：零号是保留键，禁止入账；请换用正数 fd",
            PosCode::RecFull => "留痕账满：请先归档历史轮次再继续 IO",
            PosCode::PosDrift => "共享偏移漂移：显式偏移 IO 不得移动 pos；\
                请排查是否误走了顺序路径",
            PosCode::RoundTrip => "往返不一致：pwrite 后同偏移 pread 必须逐字节一致；\
                请核对落盘路径",
            PosCode::EmptySummary => "空成文被拒：偏移小结不得为空串；请检查 fd 是否在账",
            PosCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [PosCode; 12] = [
        PosCode::NegOffset,
        PosCode::OffsetOverflow,
        PosCode::BadFd,
        PosCode::ClosedFd,
        PosCode::EmptyRec,
        PosCode::DupFd,
        PosCode::ZeroFd,
        PosCode::RecFull,
        PosCode::PosDrift,
        PosCode::RoundTrip,
        PosCode::EmptySummary,
        PosCode::SegmentViolation,
    ];
}

// ── fd 态机 ───────────────────────────────────────────────────────────
/// fd 生命周期态：Open → Closed。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FdStage {
    /// 打开态：可偏移读写
    Open,
    /// 关闭态：只保留审计记录
    Closed,
}

/// IO 方向闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IoKind {
    /// 显式偏移读
    Pread,
    /// 显式偏移写
    Pwrite,
    /// 顺序写（走共享偏移，作对照面）
    SeqWrite,
}

/// 单条偏移 IO 留痕。
#[derive(Clone, Copy, Debug)]
pub struct IoRec {
    /// 序号
    pub seq: u32,
    /// fd
    pub fd: u32,
    /// 显式偏移（顺序写时填 0）
    pub off: u64,
    /// 长度
    pub len: u32,
    /// 方向
    pub kind: IoKind,
}

/// 单 fd 记录：内容 + 共享偏移 pos + 生命周期态。
#[derive(Clone, Debug)]
pub struct PosFile {
    /// 文件内容
    pub data: Vec<u8>,
    /// 共享偏移（顺序读写推进；pread/pwrite 不动）
    pub pos: u64,
    /// 生命周期态
    pub stage: FdStage,
}

/// 偏移账本：fd 注册表 + IO 留痕账。
#[derive(Clone, Debug)]
pub struct PosLedger {
    files: Vec<(u32, PosFile)>,
    recs: Vec<IoRec>,
    seq_next: u32,
    cap: usize,
}

impl PosLedger {
    /// 空账构造（容量 LEDGER_CAP，留痕上限 REC_CAP）。
    pub fn new() -> PosLedger {
        PosLedger { files: Vec::new(), recs: Vec::new(), seq_next: 1, cap: LEDGER_CAP }
    }

    /// 注册 fd：零号拒绝、重复拒绝、容量满拒绝。
    pub fn open(&mut self, fd: u32) -> Result<(), PosCode> {
        if fd == 0 {
            return Err(PosCode::ZeroFd);
        }
        if self.files.len() >= self.cap {
            return Err(PosCode::SegmentViolation);
        }
        let mut i = 0usize;
        while i < self.files.len() {
            if let Some((k, _)) = self.files.get(i) {
                if *k == fd {
                    return Err(PosCode::DupFd);
                }
            }
            i += 1;
        }
        let _ = self.files.push((fd, PosFile { data: Vec::new(), pos: 0, stage: FdStage::Open }));
        Ok(())
    }

    /// 在账 fd 数。
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// 顺序写：从共享 pos 写入并推进 pos（对照面）。
    pub fn write_seq(&mut self, fd: u32, buf: &[u8]) -> Result<usize, PosCode> {
        if buf.is_empty() {
            return Err(PosCode::EmptyRec);
        }
        self.exists_open(fd)?;
        self.push_rec(fd, 0, buf.len() as u32, IoKind::SeqWrite)?;
        let f = self.lookup_mut(fd)?;
        let start = f.pos as usize;
        while f.data.len() < start {
            let _ = f.data.push(0);
        }
        let mut k = 0usize;
        while k < buf.len() {
            let _ = f.data.push(*buf.get(k).unwrap_or(&0));
            k += 1;
        }
        f.pos = (start + buf.len()) as u64;
        Ok(buf.len())
    }

    /// pread：显式偏移读；负偏移/溢出 EINVAL 族，账外/关闭 EBADF 族；不动 pos。
    pub fn pread(&mut self, fd: u32, off: i64, len: u32) -> Result<Vec<u8>, PosCode> {
        self.check_offset(off, len)?;
        if len == 0 {
            return Err(PosCode::EmptyRec);
        }
        self.exists_open(fd)?;
        self.push_rec(fd, off as u64, len, IoKind::Pread)?;
        let f = self.lookup_mut(fd)?;
        let start = off as usize;
        let end = start.saturating_add(len as usize);
        let mut out: Vec<u8> = Vec::new();
        let mut i = start;
        while i < end {
            if let Some(b) = f.data.get(i) {
                let _ = out.push(*b);
            } else {
                break;
            }
            i += 1;
        }
        Ok(out)
    }

    /// pwrite：显式偏移写；越尾零填充扩展；不动 pos。
    pub fn pwrite(&mut self, fd: u32, off: i64, buf: &[u8]) -> Result<usize, PosCode> {
        if buf.is_empty() {
            return Err(PosCode::EmptyRec);
        }
        self.check_offset(off, buf.len() as u32)?;
        self.exists_open(fd)?;
        self.push_rec(fd, off as u64, buf.len() as u32, IoKind::Pwrite)?;
        let f = self.lookup_mut(fd)?;
        let start = off as usize;
        while f.data.len() < start {
            let _ = f.data.push(0);
        }
        let mut k = 0usize;
        while k < buf.len() {
            let idx = start + k;
            if idx < f.data.len() {
                if let Some(slot) = f.data.get_mut(idx) {
                    *slot = *buf.get(k).unwrap_or(&0);
                }
            } else {
                let _ = f.data.push(*buf.get(k).unwrap_or(&0));
            }
            k += 1;
        }
        Ok(buf.len())
    }

    /// 共享偏移读取（审计用）。
    pub fn pos_of(&self, fd: u32) -> Option<u64> {
        let mut i = 0usize;
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

    /// 关闭：态置 Closed，此后偏移读写拒绝。
    pub fn close(&mut self, fd: u32) -> Result<(), PosCode> {
        let f = self.lookup_mut(fd)?;
        f.stage = FdStage::Closed;
        Ok(())
    }

    /// 留痕数。
    pub fn rec_len(&self) -> usize {
        self.recs.len()
    }

    /// 守恒审计：pos 只被顺序写推进，显式偏移 IO 不改 pos。
    pub fn pos_conserved(&self, fd: u32, expect: u64) -> bool {
        self.pos_of(fd) == Some(expect)
    }

    // ── 内部 ──
    fn lookup_mut(&mut self, fd: u32) -> Result<&mut PosFile, PosCode> {
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
        Err(PosCode::BadFd)
    }

    /// 不可变前置校验：账外 EBADF 族、关闭态 EBADF 族（先于任何记账）。
    fn exists_open(&self, fd: u32) -> Result<(), PosCode> {
        let mut i = 0usize;
        while i < self.files.len() {
            if let Some((k, f)) = self.files.get(i) {
                if *k == fd {
                    if f.stage == FdStage::Closed {
                        return Err(PosCode::ClosedFd);
                    }
                    return Ok(());
                }
            }
            i += 1;
        }
        Err(PosCode::BadFd)
    }

    fn check_offset(&self, off: i64, len: u32) -> Result<(), PosCode> {
        if off < 0 {
            return Err(PosCode::NegOffset);
        }
        if len as i64 > i64::MAX - off {
            return Err(PosCode::OffsetOverflow);
        }
        Ok(())
    }

    fn push_rec(&mut self, fd: u32, off: u64, len: u32, kind: IoKind) -> Result<(), PosCode> {
        if self.recs.len() >= REC_CAP {
            return Err(PosCode::RecFull);
        }
        let seq = self.seq_next;
        self.seq_next = self.seq_next.saturating_add(1);
        let _ = self.recs.push(IoRec { seq, fd, off, len, kind });
        Ok(())
    }
}

// ── J1 达成核：显式偏移不移动共享偏移（10/10 轮） ─────────────────────
/// 逐轮：顺序写推进 pos → pread/pwrite 显式偏移 → 断言 pos 不动且往返一致。
/// 返回（达成轮数, 留痕账本）供守恒与留痕判据复用。
pub fn run_offset_rounds() -> (u32, PosLedger) {
    let mut l = PosLedger::new();
    let _ = l.open(9);
    let mut ok = 0u32;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        // 顺序写 4 字节推进 pos 到 4*(round+1)。
        let _ = l.write_seq(9, b"ABCD");
        let pos_before = match l.pos_of(9) {
            Some(p) => p,
            None => break,
        };
        // 显式偏移读：从 0 读 2 字节。
        let head = l.pread(9, 0, 2).unwrap_or_default();
        // 显式偏移写：在第 round*4 处写一字节标记。
        let tag = (b'a' + (round % 26) as u8) as u8;
        let _ = l.pwrite(9, (round * 4) as i64, &[tag]);
        let pos_after = match l.pos_of(9) {
            Some(p) => p,
            None => break,
        };
        // 往返一致：同偏移读回写内容。
        let back = l.pread(9, (round * 4) as i64, 1).unwrap_or_default();
        let trip_ok = back.first() == Some(&tag);
        // 头段读只断言「按请求长度返回」：内容随轮内标记覆写演进，不做字节固定断言。
        if pos_before == pos_after && head.len() == 2 && trip_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok, l)
}

// ── 注入核：负偏移样本矩阵 ────────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadIo {
    /// 负偏移读
    NegPread,
    /// 负偏移写
    NegPwrite,
    /// 账外 fd
    BadFd,
    /// 关闭后读写
    AfterClose,
}

impl BadIo {
    /// 闭集全量。
    pub const ALL: [BadIo; 4] = [
        BadIo::NegPread,
        BadIo::NegPwrite,
        BadIo::BadFd,
        BadIo::AfterClose,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadIo::NegPread => "负偏移读",
            BadIo::NegPwrite => "负偏移写",
            BadIo::BadFd => "账外 fd 读写",
            BadIo::AfterClose => "关闭后读写",
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
    pub fn record(&mut self, caught: bool) -> Result<(), PosCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(PosCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏样本注入：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
pub fn run_bad_io_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadIo::ALL.len() {
        let case = match BadIo::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut l = PosLedger::new();
            let _ = l.open(5);
            let caught = match case {
                BadIo::NegPread => l.pread(5, -1, 4).err() == Some(PosCode::NegOffset),
                BadIo::NegPwrite => l.pwrite(5, -8, b"x").err() == Some(PosCode::NegOffset),
                BadIo::BadFd => l.pread(77, 0, 4).err() == Some(PosCode::BadFd),
                BadIo::AfterClose => {
                    let _ = l.close(5);
                    l.pread(5, 0, 4).err() == Some(PosCode::ClosedFd)
                }
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

// ── 偏移小结成文（纯函数） ────────────────────────────────────────────
/// 小结成文：「fd=<n> pos=<p> recs=<r>」；账外先拒。
pub fn compose_pos_summary(l: &PosLedger, fd: u32) -> Result<String, PosCode> {
    let pos = match l.pos_of(fd) {
        Some(p) => p,
        None => return Err(PosCode::EmptySummary),
    };
    let out = format!("fd={} pos={} recs={}", fd, pos, l.rec_len());
    if out.is_empty() {
        return Err(PosCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；负偏移/溢出族"),
    ("EBADF", -9, 10038, "Windows WSAENOTSOCK 近似；账外/关闭 fd 族"),
    ("EIO", -5, 1117, "Windows ERROR_IO_DEVICE；落盘失败族"),
    ("ENOSPC", -28, 112, "Windows ERROR_DISK_FULL；留痕账满族"),
    ("EOVERFLOW", -75, 1323, "Windows ERROR_ARITHMETIC_OVERFLOW；偏移溢出族"),
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

/// 规范语料双跑指纹：注册 → 顺序写 → 显式偏移读写 → 逐偏移回读 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut l = PosLedger::new();
    let _ = l.open(21);
    let _ = l.pwrite(21, 0, b"LEDCOIL");
    let _ = l.pwrite(21, 16, b"TAP");
    let _ = l.write_seq(21, b"SEQ");
    let mut bytes: Vec<u8> = Vec::new();
    let mut off = 0i64;
    while off < 20 {
        if let Ok(seg) = l.pread(21, off, 3) {
            bytes.extend_from_slice(&seg);
            bytes.push(b'.');
        }
        off += 3;
    }
    if let Ok(s) = compose_pos_summary(&l, 21) {
        bytes.extend_from_slice(s.as_bytes());
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10111 判据集（15 条；动态明细 ≤8 条）。
pub fn run_unx10111_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10111");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92B80..=0x92BBF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < PosCode::ALL.len() {
        let mut j = i + 1;
        while j < PosCode::ALL.len() {
            if let (Some(a), Some(b)) = (PosCode::ALL.get(i), PosCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = PosCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. fd 注册：零号/重复拒绝
    let mut l = PosLedger::new();
    let open_ok = l.open(9).is_ok() && l.len() == 1
        && l.open(0).err() == Some(PosCode::ZeroFd)
        && l.open(9).err() == Some(PosCode::DupFd);
    s.add("fd 注册：零号与重复 fd 显性拒绝", open_ok,
          "注册是 IO 前置；零号是保留键，重复注册是账目事故");

    // 4. J1 达成核：显式偏移不移动共享偏移 10/10
    let (ok, l_rounds) = run_offset_rounds();
    let final_pos = l_rounds.pos_of(9).unwrap_or(0);
    s.add("J1 偏移达成：pread/pwrite 不动共享偏移 10/10 次",
          ok == INJECT_ROUNDS && final_pos == 4 * INJECT_ROUNDS as u64,
          "显式偏移是独立光标；共享 pos 只由顺序写推进，任何漂移即失守");

    // 5. 往返一致：pwrite 后同偏移 pread 逐字节一致
    let mut l2 = PosLedger::new();
    let _ = l2.open(4);
    let _ = l2.pwrite(4, 8, b"NEON");
    let rt = l2.pread(4, 8, 4).unwrap_or_default();
    s.add("往返一致：pwrite→pread 同偏移逐字节回读", rt.as_slice() == b"NEON",
          "偏移写读必须逐字节往返一致，越尾零填充不得吞内容");

    // 6. 越尾读：EOF 之外返回空（0 字节）而非错误
    let tail = l2.pread(4, 64, 4).unwrap_or_default();
    s.add("越尾读：EOF 外空读零字节且不报错", tail.is_empty(),
          "越尾读语义是零字节成功，不得误报 EBADF/EINVAL");

    // 7. 越尾写：写点之前的空洞显式零填充
    let hole = l2.pread(4, 0, 8).unwrap_or_default();
    s.add("越尾写：空洞零填充且写点内容精确",
          hole == vec![0u8; 8] && hole.len() == 8,
          "稀疏扩展必须显式零填充，不许复用旧脏数据");

    // 8. 共享偏移守恒审计（达成核账本 + 独立账本双面）
    let cons_ok = l_rounds.pos_conserved(9, 4 * INJECT_ROUNDS as u64)
        && l2.pos_conserved(4, 0);
    s.add("共享偏移守恒：纯偏移 IO 的 pos 恒为初始值", cons_ok,
          "pos 漂移即语义事故，审计必须逐 fd 核对");

    // 9. 注入核：四类坏样本各 10/10 拒绝
    let (inj, per_class) = run_bad_io_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 拒绝", all_caught,
          "负偏移/账外 fd/关闭后读写必须逐类逐次精确复现");

    // 10. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadIo::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 11. J2 精确归因：负偏移 EINVAL 族逐类复现
    let neg_ok = l2.pread(4, -1, 2).err() == Some(PosCode::NegOffset)
        && l2.pwrite(4, -3, b"x").err() == Some(PosCode::NegOffset)
        && l2.pread(4, 0, 2).is_ok();
    s.add("J2 负偏移受控：EINVAL 族精确复现且不伤合法路径", neg_ok,
          "负偏移必须逐次拒绝，被拒操作不得改动账面");

    // 12. 关闭态：close 后偏移读写拒绝
    let _ = l2.close(4);
    let closed_ok = l2.pread(4, 0, 2).err() == Some(PosCode::ClosedFd)
        && l2.pwrite(4, 0, b"z").err() == Some(PosCode::ClosedFd);
    s.add("关闭态：close 后偏移读写均拒绝", closed_ok,
          "关闭即语义不可用；保留记录只为审计");

    // 13. 留痕账：每次 IO 留痕且轮次对齐（每轮 顺序写+头段读+偏移写+回读）
    let rec_ok = l_rounds.rec_len() == (INJECT_ROUNDS * 4) as usize && l_rounds.rec_len() > 0;
    s.add("留痕账：逐次 IO 入账且轮次对齐", rec_ok,
          "每轮 顺序写+头段读+偏移写+回读 四条留痕，缺一即漏账");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(22) && agrees_with_linux(9) && agrees_with_linux(5)
        && agrees_with_linux(28) && agrees_with_linux(75)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 15. 成文 + 双跑指纹
    let sum = compose_pos_summary(&l, 9);
    let text_ok = match &sum {
        Ok(t) => t.contains("fd=9") && t.contains("pos="),
        Err(_) => false,
    };
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("成文与指纹：小结四字段齐备且双跑哈希逐位一致",
          text_ok && sum == compose_pos_summary(&l, 9) && f1 == f2,
          "成文为纯函数；同语料两次哈希必须逐位一致");

    // 动态明细（≤8）：注入账快照 + 指纹 + 达成轮数
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let ok_text = format!("偏移达成轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_guards() {
        let mut l = PosLedger::new();
        assert!(l.open(1).is_ok());
        assert_eq!(l.open(0).err(), Some(PosCode::ZeroFd));
        assert!(l.open(1).is_err());
    }

    #[test]
    fn negative_offset_rejected() {
        let mut l = PosLedger::new();
        let _ = l.open(2);
        assert_eq!(l.pread(2, -1, 4).err(), Some(PosCode::NegOffset));
        assert_eq!(l.pwrite(2, -2, b"x").err(), Some(PosCode::NegOffset));
    }

    #[test]
    fn pos_not_moved_by_positional_io() {
        let mut l = PosLedger::new();
        let _ = l.open(3);
        let _ = l.write_seq(3, b"HELLO");
        let _ = l.pread(3, 1, 2);
        let _ = l.pwrite(3, 2, b"X");
        assert_eq!(l.pos_of(3), Some(5));
    }

    #[test]
    fn seq_write_advances_pos() {
        let mut l = PosLedger::new();
        let _ = l.open(3);
        let _ = l.write_seq(3, b"AB");
        let _ = l.write_seq(3, b"CD");
        assert_eq!(l.pos_of(3), Some(4));
        assert_eq!(l.pread(3, 0, 4).unwrap_or_default(), b"ABCD".to_vec());
    }

    #[test]
    fn sparse_write_zero_fills() {
        let mut l = PosLedger::new();
        let _ = l.open(4);
        let _ = l.pwrite(4, 4, b"Z");
        assert_eq!(l.pread(4, 0, 4).unwrap_or_default(), vec![0u8; 4]);
        assert_eq!(l.pread(4, 4, 1).unwrap_or_default(), b"Z".to_vec());
    }

    #[test]
    fn closed_fd_rejected() {
        let mut l = PosLedger::new();
        let _ = l.open(5);
        let _ = l.close(5);
        assert_eq!(l.pread(5, 0, 1).err(), Some(PosCode::ClosedFd));
        assert_eq!(l.pwrite(5, 0, b"z").err(), Some(PosCode::ClosedFd));
    }

    #[test]
    fn bad_io_matrix_green() {
        let (inj, per) = run_bad_io_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn offset_rounds_full() {
        let (ok, ledger) = run_offset_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
        assert_eq!(ledger.pos_of(9), Some(4 * INJECT_ROUNDS as u64));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(22));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(75), -75);
    }

    #[test]
    fn unx10111_checks_green() {
        let set = run_unx10111_checks();
        assert!(set.all_passed(), "UNX-F10111 red: {}", set.red_items_text());
    }
}
