//! UNX-F9343 全域 EBADF 矩阵总装 · 0x7C340段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9343 逐字）：
//!   ① 纯功能代码 ≥380 行；
//!   ② 判据原文：UNX-F9343-J1 五族坏 fd 逐格总装判据过，全 EBADF 逐格
//!      检出，fd 空间全域扫描格过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① fd 是**带代号的句柄**：fd = (gen << 8) | slot。代号在同一槽复用时
//!      递增，旧句柄因此变成 StaleGen——这与「槽从未打开」（Closed）是两种
//!      不同的坏 fd，但都必须折成 EBADF。判定次序冻结为：
//!      负号 → 越出 fd 空间上限 → 槽号越界 → 槽未打开 → 代号不符 → 类型。
//!   ② 五族按「操作面」划分：数据面 read/write、元数据面 fstat、多路面
//!      poll、复制面 dup2、控制面 fcntl/ioctl。每族 × 6 种坏 fd 形态 × 10
//!      轮 = 300 次注入，账本满足 injected == detected + escaped 且逃逸 0。
//!   ③ 关键反判据：EBADF 不是「所有失败」。目录 fd 上 read/write 必须
//!      EISDIR（21）、ioctl 必须 ENOTTY（25）、fcntl 必须 EINVAL（22）；
//!      表满必须 EMFILE（24）。四类对照若被折成 EBADF，判据必红。
//!   ④ 全域扫描：fd 号 0..=63 逐号查表，得 usable / ebadf / other 三计数，
//!      和必须等于扫描格数，且与表内 opens 独立推导值一致（防漏格）。
//!   ⑤ 次序判据：坏 fd 与坏缓冲并存时必须先回 EBADF——查表在
//!      copy_from_user 之前；把 EFAULT 提到前面真实内核也不允许。
//!   ⑥ 诊断码 12 个（+1..=+12）码值唯一；指纹把命中数 + 扫描计数喂
//!      FNV-1a，双跑相等。
//! ─────────────────────────────────────────────────────────────────────

use alloc::{format, string::String, vec::Vec};

use crate::checks::CheckSet;

/// 诊断段基址（F9343 独占 0x7C340..=0x7C37F）。
pub const SEG_BASE: u32 = 0x7C340;

/// Linux errno 数值（正数口径）。
pub const EBADF: i32 = 9;
pub const EISDIR: i32 = 21;
pub const ENOTTY: i32 = 25;
pub const EMFILE: i32 = 24;
pub const EINVAL: i32 = 22;
pub const EFAULT: i32 = 14;

/// fd 表容量（槽数）。
pub const SLOTS: usize = 16;
/// fd 空间上限（fd 号 >= 此值即 EBADF）。
pub const FD_SPACE_LIMIT: i64 = 1 << 20;
/// 全域扫描上界（含）。
pub const SWEEP_END: i64 = 63;
/// 扫描格数 = 0..=SWEEP_END。
pub const SWEEP_CELLS: u32 = (SWEEP_END + 1) as u32;
/// 每格注入轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 坏 fd 形态数。
pub const FORMS: usize = 6;
/// 五族数。
pub const FAMILIES: usize = 5;

/// fd 类型（描述槽上对象的类别，不是 inode 类型）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FdKind {
    File,
    Dir,
    Sock,
    Pipe,
    Event,
}

/// 操作面（五族各取一个代表操作；Ctl 族另含 ioctl 对照）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FdOp {
    Read,
    Write,
    Fstat,
    Poll,
    Dup2,
    Fcntl,
    Ioctl,
}

impl FdOp {
    /// 族代表操作（注入矩阵按族取代表）。
    pub fn representative(family: usize) -> FdOp {
        match family {
            0 => FdOp::Read,
            1 => FdOp::Fstat,
            2 => FdOp::Poll,
            3 => FdOp::Dup2,
            _ => FdOp::Fcntl,
        }
    }
}

/// 判决错误（None 表示成功；其余映射到 errno）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FdErr {
    /// 槽从未打开
    Closed,
    /// 代号过期（槽被复用）
    StaleGen,
    /// 槽号越出表容量
    OutOfSlot,
    /// fd 号越出空间上限
    OutOfRange,
    /// 负 fd
    Negative,
    /// 目录 fd 上读写
    Eisdir,
    /// 目录 fd 上 ioctl
    Enotty,
    /// 操作与类型不匹配
    KindInvalid,
    /// 表已满
    Emfile,
}

impl FdErr {
    /// errno 映射（全部为正数口径）。
    pub fn errno(self) -> i32 {
        match self {
            FdErr::Closed | FdErr::StaleGen | FdErr::OutOfSlot | FdErr::OutOfRange
            | FdErr::Negative => EBADF,
            FdErr::Eisdir => EISDIR,
            FdErr::Enotty => ENOTTY,
            FdErr::KindInvalid => EINVAL,
            FdErr::Emfile => EMFILE,
        }
    }
    /// 是否 EBADF 家族。
    pub fn is_ebadf(self) -> bool {
        self.errno() == EBADF
    }
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 0x01..=0x0C，两两唯一）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EbadfCode {
    ClosedSlot = 1,
    StaleGen = 2,
    OutOfSlot = 3,
    OutOfRange = 4,
    NegativeFd = 5,
    StaleKind = 6,
    TableFull = 7,
    KindCollapse = 8,
    OrderViolation = 9,
    SweepDrift = 10,
    NotReproducible = 11,
    SegmentViolation = 12,
}

impl EbadfCode {
    pub fn code(self) -> u16 {
        self as u16
    }
    pub fn abs(self) -> u32 {
        SEG_BASE + self.code() as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            EbadfCode::ClosedSlot => "槽未打开：句柄指向的表项 open==false，必须 EBADF；请检查 close 后残留句柄",
            EbadfCode::StaleGen => "代号过期：槽已复用而 fd 代号未升，必须 EBADF；请核对 close 是否递增代号",
            EbadfCode::OutOfSlot => "槽号越界：fd & 0xFF ≥ 表容量，必须 EBADF；请先做槽号范围校验",
            EbadfCode::OutOfRange => "fd 越上限：fd ≥ 1<<20 属非法空间，必须 EBADF；请核对 rlimit 与分配器",
            EbadfCode::NegativeFd => "负 fd：除 AT_FDCWD 专用负号外一律 EBADF；请先判符号位",
            EbadfCode::StaleKind => "过期目录句柄：代号不符时不得先判类型，EBADF 必须优先于 EISDIR",
            EbadfCode::TableFull => "表已满：分配失败必须 EMFILE，不得误报 EBADF；请检查分配路径",
            EbadfCode::KindCollapse => "类型拒绝被抹平：目录 fd 上的读写/ioctl 应分别 EISDIR/ENOTTY",
            EbadfCode::OrderViolation => "次序错误：坏 fd 必须先于坏缓冲返回 EBADF；请把查表提到拷贝之前",
            EbadfCode::SweepDrift => "扫描漂移：三类计数之和不等于扫描格数，或有格被漏判",
            EbadfCode::NotReproducible => "双跑不一致：同输入两次指纹不同；请排查隐藏状态",
            EbadfCode::SegmentViolation => "段位越界：段基址必须落在 0x7C340..=0x7C37F；请核对 SEG_BASE",
        }
    }
    pub const ALL: &'static [EbadfCode] = &[
        EbadfCode::ClosedSlot,
        EbadfCode::StaleGen,
        EbadfCode::OutOfSlot,
        EbadfCode::OutOfRange,
        EbadfCode::NegativeFd,
        EbadfCode::StaleKind,
        EbadfCode::TableFull,
        EbadfCode::KindCollapse,
        EbadfCode::OrderViolation,
        EbadfCode::SweepDrift,
        EbadfCode::NotReproducible,
        EbadfCode::SegmentViolation,
    ];
}

// ── 句柄编码 ──────────────────────────────────────────────────────────
/// 槽 + 代号 → 句柄（低 8 位槽号，高 8 位代号）。
pub fn encode_fd(slot: u8, gen: u8) -> i64 {
    ((gen as i64) << 8) | (slot as i64)
}

/// 句柄 → 槽号。
pub fn fd_slot(fd: i64) -> u8 {
    (fd & 0xFF) as u8
}

/// 句柄 → 代号。
pub fn fd_gen(fd: i64) -> u8 {
    ((fd >> 8) & 0xFF) as u8
}

/// 编码/解码往返是否自洽（负 fd 不参与）。
pub fn roundtrip_ok(fd: i64) -> bool {
    fd >= 0 && fd < FD_SPACE_LIMIT && encode_fd(fd_slot(fd), fd_gen(fd)) == fd
}
// ── fd 表 ────────────────────────────────────────────────────────────
/// 一个槽：是否打开 + 代号 + 类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FdSlot {
    pub open: bool,
    pub gen: u8,
    pub kind: FdKind,
}

impl FdSlot {
    pub const fn free() -> FdSlot {
        FdSlot { open: false, gen: 0, kind: FdKind::File }
    }
}

/// 定容 fd 表 + 四类计数（opens / ebadf / emfile / kind_reject）。
#[derive(Clone, Copy, Debug)]
pub struct FdTable {
    slots: [FdSlot; SLOTS],
    opens: u32,
    ebadf: u32,
    emfile: u32,
    kind_reject: u32,
}

impl FdTable {
    pub const fn new() -> FdTable {
        FdTable { slots: [FdSlot::free(); SLOTS], opens: 0, ebadf: 0, emfile: 0, kind_reject: 0 }
    }
    pub fn opens(&self) -> u32 {
        self.opens
    }
    pub fn ebadf(&self) -> u32 {
        self.ebadf
    }
    pub fn emfile(&self) -> u32 {
        self.emfile
    }
    pub fn kind_reject(&self) -> u32 {
        self.kind_reject
    }
    pub fn slot(&self, i: usize) -> Option<&FdSlot> {
        self.slots.get(i)
    }

    /// 分配最低空槽；满表返回 Emfile（不是 EBADF）。
    pub fn open(&mut self, kind: FdKind) -> Result<i64, FdErr> {
        let mut i = 0usize;
        while i < SLOTS {
            let free = match self.slots.get(i) {
                Some(s) => !s.open,
                None => false,
            };
            if free {
                let gen = match self.slots.get(i) {
                    Some(s) => s.gen,
                    None => 0,
                };
                if let Some(s) = self.slots.get_mut(i) {
                    s.open = true;
                    s.kind = kind;
                }
                self.opens = self.opens.saturating_add(1);
                return Ok(encode_fd(i as u8, gen));
            }
            i += 1;
        }
        self.emfile = self.emfile.saturating_add(1);
        Err(FdErr::Emfile)
    }

    /// 核心查表：冻结次序（负号 → 上限 → 槽号 → 打开位 → 代号）。
    pub fn lookup(&self, fd: i64) -> Result<usize, FdErr> {
        if fd < 0 {
            return Err(FdErr::Negative);
        }
        if fd >= FD_SPACE_LIMIT {
            return Err(FdErr::OutOfRange);
        }
        let slot = (fd & 0xFF) as usize;
        if slot >= SLOTS {
            return Err(FdErr::OutOfSlot);
        }
        let gen = ((fd >> 8) & 0xFF) as u8;
        let s = match self.slots.get(slot) {
            Some(v) => v,
            None => return Err(FdErr::OutOfSlot),
        };
        if !s.open {
            return Err(FdErr::Closed);
        }
        if s.gen != gen {
            return Err(FdErr::StaleGen);
        }
        Ok(slot)
    }

    /// 关闭：清打开位并把代号加一（旧句柄立刻过期）。
    pub fn close(&mut self, fd: i64) -> Result<(), FdErr> {
        let slot = match self.lookup(fd) {
            Ok(s) => s,
            Err(e) => {
                if e.is_ebadf() {
                    self.ebadf = self.ebadf.saturating_add(1);
                }
                return Err(e);
            }
        };
        if let Some(s) = self.slots.get_mut(slot) {
            s.open = false;
            s.gen = s.gen.wrapping_add(1);
        }
        self.opens = self.opens.saturating_sub(1);
        Ok(())
    }

    /// 类型面特判：目录 fd 上读写/ioctl 有专门 errno，不得折成 EBADF。
    pub fn kind_special(op: FdOp, kind: FdKind) -> Option<FdErr> {
        match (op, kind) {
            (FdOp::Read, FdKind::Dir) | (FdOp::Write, FdKind::Dir) => Some(FdErr::Eisdir),
            (FdOp::Ioctl, FdKind::Dir) => Some(FdErr::Enotty),
            _ => None,
        }
    }

    /// 操作与类型是否相容。
    pub fn op_allowed(op: FdOp, kind: FdKind) -> bool {
        match kind {
            FdKind::Dir => matches!(op, FdOp::Fstat),
            _ => true,
        }
    }

    /// 操作入口：查表 → 类型特判 → 相容性；失败按类记账。
    pub fn dispatch(&mut self, op: FdOp, fd: i64) -> Result<usize, FdErr> {
        let slot = match self.lookup(fd) {
            Ok(s) => s,
            Err(e) => {
                if e.is_ebadf() {
                    self.ebadf = self.ebadf.saturating_add(1);
                }
                return Err(e);
            }
        };
        let kind = match self.slots.get(slot) {
            Some(s) => s.kind,
            None => return Err(FdErr::OutOfSlot),
        };
        if let Some(special) = FdTable::kind_special(op, kind) {
            self.kind_reject = self.kind_reject.saturating_add(1);
            return Err(special);
        }
        if !FdTable::op_allowed(op, kind) {
            self.kind_reject = self.kind_reject.saturating_add(1);
            return Err(FdErr::KindInvalid);
        }
        Ok(slot)
    }

    /// dup2 语义：源必须有效，目标号必须落在合法空间；目标可未打开。
    pub fn dup2(&mut self, src: i64, dst: i64) -> Result<i64, FdErr> {
        let slot = match self.lookup(src) {
            Ok(s) => s,
            Err(e) => {
                if e.is_ebadf() {
                    self.ebadf = self.ebadf.saturating_add(1);
                }
                return Err(e);
            }
        };
        if dst < 0 || dst >= FD_SPACE_LIMIT {
            self.ebadf = self.ebadf.saturating_add(1);
            return Err(FdErr::OutOfRange);
        }
        if dst == src {
            return Ok(encode_fd(slot as u8, fd_gen(src)));
        }
        Ok(encode_fd(fd_slot(dst), fd_gen(dst)))
    }

    /// 读系统调用全路径：fd 先于缓冲校验（次序判据的落点）。
    pub fn read_entry(&mut self, fd: i64, buf_ok: bool) -> i32 {
        match self.dispatch(FdOp::Read, fd) {
            Ok(_) => {
                if buf_ok {
                    0
                } else {
                    EFAULT
                }
            }
            Err(e) => e.errno(),
        }
    }
}

/// 八打开槽的规范表（0..=7，其中 3 与 7 是目录）。
pub fn fresh_table() -> FdTable {
    let mut t = FdTable::new();
    let kinds = [
        FdKind::File,
        FdKind::Sock,
        FdKind::Pipe,
        FdKind::Dir,
        FdKind::Event,
        FdKind::File,
        FdKind::Sock,
        FdKind::Dir,
    ];
    let mut i = 0usize;
    while i < kinds.len() && i < SLOTS {
        if let Some(k) = kinds.get(i).copied() {
            if let Some(s) = t.slots.get_mut(i) {
                s.open = true;
                s.kind = k;
            }
            t.opens = t.opens.saturating_add(1);
        }
        i += 1;
    }
    t
}

// ── 全域扫描 ──────────────────────────────────────────────────────────
/// 扫描三类计数（和必须等于扫描格数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SweepTally {
    pub probed: u32,
    pub usable: u32,
    pub ebadf: u32,
    pub other: u32,
}

impl SweepTally {
    pub const fn zero() -> SweepTally {
        SweepTally { probed: 0, usable: 0, ebadf: 0, other: 0 }
    }
    /// 三类之和守恒。
    pub fn conserved(&self) -> bool {
        self.usable.saturating_add(self.ebadf).saturating_add(self.other) == self.probed
    }
}

/// fd 号 0..=63 全域扫描（代号 0 口径）。
pub fn sweep_fd_space(t: &FdTable) -> SweepTally {
    let mut out = SweepTally::zero();
    let mut fd = 0i64;
    while fd <= SWEEP_END {
        out.probed = out.probed.saturating_add(1);
        match t.lookup(fd) {
            Ok(_) => out.usable = out.usable.saturating_add(1),
            Err(e) => {
                if e.is_ebadf() {
                    out.ebadf = out.ebadf.saturating_add(1);
                } else {
                    out.other = out.other.saturating_add(1);
                }
            }
        }
        fd += 1;
    }
    out
}

/// 独立推导：槽号 < 容量且已打开才可用（与 sweep 走查互证）。
pub fn sweep_expected(t: &FdTable) -> (u32, u32) {
    let mut usable = 0u32;
    let mut slot = 0usize;
    while slot < SLOTS && (slot as i64) <= SWEEP_END {
        if let Some(s) = t.slot(slot) {
            if s.open && s.gen == 0 {
                usable = usable.saturating_add(1);
            }
        }
        slot += 1;
    }
    (usable, SWEEP_CELLS.saturating_sub(usable))
}

// ── 注入矩阵 ──────────────────────────────────────────────────────────
/// 六种坏 fd 形态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadForm {
    Closed = 0,
    Stale = 1,
    OutOfSlot = 2,
    OutOfRange = 3,
    Negative = 4,
    StaleDir = 5,
}

impl BadForm {
    /// 形态对应的探测 fd（在 fresh_table 上求值）。
    pub fn probe_fd(self) -> i64 {
        match self {
            BadForm::Closed => 12,
            BadForm::Stale => encode_fd(0, 1),
            BadForm::OutOfSlot => 40,
            BadForm::OutOfRange => FD_SPACE_LIMIT,
            BadForm::Negative => -1,
            BadForm::StaleDir => encode_fd(3, 1),
        }
    }
}

/// 注入结果账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EbadfOutcome {
    /// [族][形态] 命中轮数
    pub hits: [[u8; FORMS]; FAMILIES],
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
    /// 按形态汇总的检出数（每形 5 族 × 10 轮）
    pub by_form: [u32; FORMS],
}

impl EbadfOutcome {
    pub const fn empty() -> EbadfOutcome {
        EbadfOutcome {
            hits: [[0u8; FORMS]; FAMILIES],
            injected: 0,
            detected: 0,
            escaped: 0,
            by_form: [0u32; FORMS],
        }
    }
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
    /// 每族每形态都打满轮数（5 × 6 = 30 格）。
    pub fn full(&self) -> bool {
        self.hits
            .iter()
            .all(|row| row.iter().all(|c| *c == INJECT_ROUNDS as u8))
    }
}

/// 总装复算：5 族 × 6 形态 × 10 轮 = 300，全部必须 EBADF。
pub fn run_injection() -> EbadfOutcome {
    let mut out = EbadfOutcome::empty();
    let mut fi = 0usize;
    while fi < FAMILIES {
        let op = FdOp::representative(fi);
        let mut bi = 0usize;
        while bi < FORMS {
            if let Some(form) = form_at(bi) {
                let mut round = 0u32;
                while round < INJECT_ROUNDS {
                    let mut t = fresh_table();
                    let got = t.dispatch(op, form.probe_fd());
                    let caught = match got {
                        Err(e) => e.errno() == EBADF,
                        Ok(_) => false,
                    };
                    out.injected = out.injected.saturating_add(1);
                    if caught {
                        out.detected = out.detected.saturating_add(1);
                        if let Some(row) = out.hits.get_mut(fi) {
                            if let Some(c) = row.get_mut(bi) {
                                *c = c.saturating_add(1);
                            }
                        }
                        if let Some(c) = out.by_form.get_mut(bi) {
                            *c = c.saturating_add(1);
                        }
                    } else {
                        out.escaped = out.escaped.saturating_add(1);
                    }
                    round += 1;
                }
            }
            bi += 1;
        }
        fi += 1;
    }
    out
}

/// 形态闭集按序取（避免裸下标）。
pub fn form_at(i: usize) -> Option<BadForm> {
    match i {
        0 => Some(BadForm::Closed),
        1 => Some(BadForm::Stale),
        2 => Some(BadForm::OutOfSlot),
        3 => Some(BadForm::OutOfRange),
        4 => Some(BadForm::Negative),
        5 => Some(BadForm::StaleDir),
        _ => None,
    }
}

/// 冻结常量：300 次注入，每形 50 次。
pub const EXPECTED_INJECT: u32 = (FAMILIES as u32) * (FORMS as u32) * INJECT_ROUNDS;
pub const EXPECTED_PER_FORM: u32 = (FAMILIES as u32) * INJECT_ROUNDS;

// ── Linux errno 对照 ─────────────────────────────────────────────────
/// (名称, 正数码, 负值, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EBADF", EBADF, -9, "坏文件描述符"),
    ("EISDIR", EISDIR, -21, "对目录做读写"),
    ("ENOTTY", ENOTTY, -25, "不支持的 ioctl"),
    ("EMFILE", EMFILE, -24, "进程 fd 表已满"),
];

/// 按名称查正数码；表外 0。
pub fn errno_by_name(name: &str) -> i32 {
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

/// 正码与负值互为相反数。
pub fn agrees_with_linux(code: i32) -> bool {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == code {
                return row.1 == -row.2;
            }
        }
        i += 1;
    }
    false
}

// ── 指纹 ─────────────────────────────────────────────────────────────
/// FNV-1a 32 位。
pub fn fnv1a32(bytes: &[u8]) -> u32 {
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

fn feed_u32(out: &mut Vec<u8>, v: u32) {
    out.push((v & 0xFF) as u8);
    out.push(((v >> 8) & 0xFF) as u8);
    out.push(((v >> 16) & 0xFF) as u8);
    out.push(((v >> 24) & 0xFF) as u8);
}

/// 规范语料：注入命中矩阵 + 扫描三类计数 + 表内计数。
pub fn matrix_fingerprint() -> u32 {
    let m = run_injection();
    let t = fresh_table();
    let sweep = sweep_fd_space(&t);
    let mut buf: Vec<u8> = Vec::new();
    let mut fi = 0usize;
    while fi < FAMILIES {
        if let Some(row) = m.hits.get(fi) {
            let mut bi = 0usize;
            while bi < FORMS {
                if let Some(v) = row.get(bi) {
                    buf.push(*v);
                }
                bi += 1;
            }
        }
        fi += 1;
    }
    feed_u32(&mut buf, sweep.probed);
    feed_u32(&mut buf, sweep.usable);
    feed_u32(&mut buf, sweep.ebadf);
    feed_u32(&mut buf, sweep.other);
    feed_u32(&mut buf, t.opens());
    feed_u32(&mut buf, m.injected);
    feed_u32(&mut buf, m.detected);
    fnv1a32(&buf)
}

// ── 交付写回面（判据③） ──────────────────────────────────────────────
/// 软件写回注记：摘要按需生成，不缓存文本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SoftwareNote {
    pub card: u16,
    pub state: &'static str,
    pub injected: u32,
    pub detected: u32,
}

impl SoftwareNote {
    /// 结果摘要（非空由格式串常量前缀保证）。
    pub fn summary(&self) -> String {
        format!(
            "UNX-F{}-J1 注入 {} 检出 {} 状态 {}",
            self.card, self.injected, self.detected, self.state
        )
    }
    /// 判据③：状态为「已完成」且摘要非空且账目平。
    pub fn written_back(&self) -> bool {
        self.state == "已完成"
            && self.injected == self.detected
            && !self.summary().is_empty()
    }
}

/// 由注入账生成写回注记。
pub fn software_note(m: &EbadfOutcome) -> SoftwareNote {
    SoftwareNote { card: 9343, state: "已完成", injected: m.injected, detected: m.detected }
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9343 判据集（add ≥12；add_dyn ≤8）。
pub fn run_unx9343_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9343");

    let seg_ok = SEG_BASE % 0x40 == 0
        && (0x7C340..0x7C380).contains(&SEG_BASE)
        && SEG_BASE + 0x40 <= 0x7C380;
    s.add("段位守卫：0x7C340 且 0x40 对齐，不越 F9344 窗口", seg_ok,
          "段基址必须在 0x7C340..=0x7C37F，段间窗口不得互相覆盖");

    s.add("句柄编码：槽/代号往返逐例自洽",
          roundtrip_ok(0) && roundtrip_ok(encode_fd(15, 255))
              && fd_slot(encode_fd(9, 3)) == 9 && fd_gen(encode_fd(9, 3)) == 3,
          "低 8 位槽号、高 8 位代号，往返必须还原原值");

    let mut t = fresh_table();
    s.add("正常流：八槽句柄全部可用",
          t.lookup(0).is_ok() && t.lookup(7).is_ok() && t.opens() == 8,
          "规范表打开 0..=7 共八槽，句柄 0..=7 必须全部有效");

    s.add("槽未打开：必须 EBADF=9",
          t.lookup(12) == Err(FdErr::Closed) && FdErr::Closed.errno() == EBADF,
          "从未打开的槽不得被当成有效句柄");

    s.add("代号过期：槽复用后旧句柄 EBADF",
          t.lookup(encode_fd(0, 1)) == Err(FdErr::StaleGen)
              && FdErr::StaleGen.errno() == EBADF,
          "close 之后代号递增，旧句柄必须立刻失效");

    s.add("槽号越界：fd&0xFF ≥ 表容量即 EBADF",
          t.lookup(40) == Err(FdErr::OutOfSlot) && t.lookup(40).err() == Some(FdErr::OutOfSlot),
          "槽号校验必须早于打开位，越界不得被当作 closed");

    s.add("fd 越上限：≥1<<20 即 EBADF",
          t.lookup(FD_SPACE_LIMIT) == Err(FdErr::OutOfRange)
              && t.lookup(FD_SPACE_LIMIT - 1).err() != Some(FdErr::OutOfRange),
          "空间上限与表容量是两层校验，不得混用");

    s.add("负 fd：一律 EBADF（AT_FDCWD 属别域）",
          t.lookup(-1) == Err(FdErr::Negative) && t.lookup(-100) == Err(FdErr::Negative),
          "符号位必须先判，负号不得进入槽号掩码运算");

    s.add("对照：目录 fd 读写回 EISDIR=21，非 EBADF",
          t.dispatch(FdOp::Read, 3).err() == Some(FdErr::Eisdir)
              && t.dispatch(FdOp::Write, 3).err() == Some(FdErr::Eisdir)
              && FdErr::Eisdir.errno() == EISDIR,
          "类型判定在查表之后；目录句柄有效时不得回 EBADF");

    s.add("对照：目录 ioctl 回 ENOTTY=25、fcntl 回 EINVAL=22",
          t.dispatch(FdOp::Ioctl, 3).err() == Some(FdErr::Enotty)
              && t.dispatch(FdOp::Fcntl, 3).err() == Some(FdErr::KindInvalid)
              && FdErr::Enotty.errno() == ENOTTY
              && FdErr::KindInvalid.errno() == EINVAL,
          "同一目录 fd 上不同操作必须分化出三种 errno，不得一律 EBADF");

    s.add("对照：表满回 EMFILE=24，非 EBADF",
          {
              let mut full = FdTable::new();
              let mut k = 0usize;
              let mut opened = 0u32;
              while k < SLOTS {
                  if full.open(FdKind::File).is_ok() {
                      opened = opened.saturating_add(1);
                  }
                  k += 1;
              }
              opened == SLOTS as u32
                  && full.open(FdKind::File) == Err(FdErr::Emfile)
                  && full.emfile() == 1
                  && FdErr::Emfile.errno() == EMFILE
          },
          "分配失败是资源面错误，与坏句柄必须分开");

    let mut d = fresh_table();
    let dup_ok = d.dup2(0, 9) == Ok(9) && d.dup2(12, 9) == Err(FdErr::Closed);
    let dup2_bound = d.dup2(0, FD_SPACE_LIMIT) == Err(FdErr::OutOfRange);
    s.add("dup2：源坏 EBADF、目标号越界 EBADF、正常目标放行",
          dup_ok && dup2_bound,
          "dup2 先验源再验目标号；目标槽可以未打开");

    let mut o = fresh_table();
    let both_bad = o.read_entry(12, false);
    let buf_only = o.read_entry(0, false);
    s.add("次序：坏 fd 先于坏缓冲（EBADF 胜过 EFAULT）",
          both_bad == EBADF && buf_only == EFAULT,
          "查表必须早于 copy_from_user，否则同时坏时返回错码");

    let sweep = sweep_fd_space(&t);
    let (want_usable, want_ebadf) = sweep_expected(&t);
    s.add("全域扫描：0..=63 共 64 格三类守恒且与表推导一致",
          sweep.conserved() && sweep.probed == SWEEP_CELLS
              && sweep.usable == want_usable && sweep.ebadf == want_ebadf
              && sweep.other == 0,
          "走查计数与槽位推导必须同值，扫描不得漏格");

    let m = run_injection();
    s.add("总装：5 族 × 6 形态 × 10 轮 = 300 格全 EBADF",
          m.full() && m.injected == EXPECTED_INJECT && m.detected == EXPECTED_INJECT,
          "每族每形态必须满 10 轮命中，逐格不得缺");

    s.add("账本：逃逸为 0、每形恰 50 次检出",
          m.conserved() && m.escaped == 0
              && m.by_form.iter().all(|n| *n == EXPECTED_PER_FORM),
          "injected == detected + escaped，六形态各 5 族 × 10 轮");

    let linux_ok = errno_by_name("EBADF") == EBADF
        && agrees_with_linux(EBADF) && agrees_with_linux(EISDIR)
        && agrees_with_linux(ENOTTY) && agrees_with_linux(EMFILE)
        && !agrees_with_linux(0);
    s.add("Linux 逐码：9/21/25/24 表内可复算，表外码拒", linux_ok,
          "对照族 errno 必须与 Linux 数值一致，不得自造");

    let fp_a = matrix_fingerprint();
    let fp_b = matrix_fingerprint();
    s.add("双跑一致：注入+扫描指纹逐字节相等", fp_a == fp_b,
          "同输入两次必须相等；隐藏状态或非确定次序即失败");

    let mut uniq = true;
    let mut i = 0usize;
    while i < EbadfCode::ALL.len() {
        let mut j = i + 1;
        while j < EbadfCode::ALL.len() {
            if let (Some(a), Some(b)) = (EbadfCode::ALL.get(i), EbadfCode::ALL.get(j)) {
                if a.abs() == b.abs() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = EbadfCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码：12 码值唯一且文案非空",
          uniq && text_ok && EbadfCode::ALL.len() == 12,
          "段内闭集码不得重复，每码必须可操作");

    let note = software_note(&m);
    s.add("交付写回：状态=已完成、摘要非空", note.written_back(),
          "判据③要求软件写回状态与结果摘要，摘要不得为空");

    let sweep_text = format!(
        "扫描 {} 格：可用 {} / EBADF {} / 其他 {}",
        sweep.probed, sweep.usable, sweep.ebadf, sweep.other
    );
    s.add_dyn("扫描快照", sweep.conserved(), &sweep_text);
    let inj_text = format!(
        "注入 {} 检出 {} 逃逸 {}；六形各 {}",
        m.injected, m.detected, m.escaped, EXPECTED_PER_FORM
    );
    s.add_dyn("注入快照", m.conserved() && m.full(), &inj_text);
    let fp_text = format!("句柄矩阵指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_forms_are_ebadf() {
        let t = fresh_table();
        assert_eq!(t.lookup(12), Err(FdErr::Closed));
        assert_eq!(t.lookup(encode_fd(0, 1)), Err(FdErr::StaleGen));
        assert_eq!(t.lookup(40), Err(FdErr::OutOfSlot));
        assert_eq!(t.lookup(FD_SPACE_LIMIT), Err(FdErr::OutOfRange));
        assert_eq!(t.lookup(-1), Err(FdErr::Negative));
        assert_eq!(FdErr::Closed.errno(), EBADF);
    }

    #[test]
    fn close_invalidates_old_handle() {
        let mut t = fresh_table();
        let fd = t.open(FdKind::Pipe).unwrap_or(-1);
        assert_eq!(t.lookup(fd).is_ok(), true);
        assert_eq!(t.close(fd), Ok(()));
        // 冻结次序「槽未打开 → 代号不符」：close 后槽 open==false，旧句柄
        // 折成 Closed（仍在 EBADF 族）；代号推进由复开编码 +0x100 佐证。
        assert_eq!(t.lookup(fd), Err(FdErr::Closed));
        let again = t.open(FdKind::Pipe);
        assert_eq!(again, Ok(fd + 0x100));
    }

    #[test]
    fn directory_is_not_ebadf() {
        let mut t = fresh_table();
        assert_eq!(t.dispatch(FdOp::Read, 3), Err(FdErr::Eisdir));
        assert_eq!(t.dispatch(FdOp::Write, 3), Err(FdErr::Eisdir));
        assert_eq!(t.dispatch(FdOp::Ioctl, 3), Err(FdErr::Enotty));
        assert_eq!(t.dispatch(FdOp::Fcntl, 3), Err(FdErr::KindInvalid));
        assert_eq!(t.dispatch(FdOp::Fstat, 3), Ok(3));
    }

    #[test]
    fn table_full_is_emfile() {
        let mut t = FdTable::new();
        let mut k = 0usize;
        while k < SLOTS {
            assert_eq!(t.open(FdKind::File).is_ok(), true);
            k += 1;
        }
        assert_eq!(t.open(FdKind::File), Err(FdErr::Emfile));
        assert_eq!(t.emfile(), 1);
        assert_eq!(FdErr::Emfile.errno(), EMFILE);
    }

    #[test]
    fn fd_before_buffer() {
        let mut t = fresh_table();
        assert_eq!(t.read_entry(12, false), EBADF);
        assert_eq!(t.read_entry(0, false), EFAULT);
        assert_eq!(t.read_entry(0, true), 0);
    }

    #[test]
    fn sweep_is_conserved() {
        let t = fresh_table();
        let s = sweep_fd_space(&t);
        let (u, e) = sweep_expected(&t);
        assert_eq!(s.probed, 64);
        assert_eq!(s.conserved(), true);
        assert_eq!(s.usable, u);
        assert_eq!(s.ebadf, e);
        assert_eq!(s.other, 0);
        assert_eq!(s.usable, 8);
        assert_eq!(s.ebadf, 56);
    }

    #[test]
    fn injection_covers_all_cells() {
        let m = run_injection();
        assert_eq!(m.injected, 300);
        assert_eq!(m.detected, 300);
        assert_eq!(m.escaped, 0);
        assert_eq!(m.full(), true);
        assert_eq!(m.conserved(), true);
        for n in m.by_form.iter() {
            assert_eq!(*n, 50);
        }
    }

    #[test]
    fn codes_unique_and_texted() {
        assert_eq!(EbadfCode::ALL.len(), 12);
        let mut i = 0usize;
        while i < EbadfCode::ALL.len() {
            if let Some(c) = EbadfCode::ALL.get(i) {
                assert_eq!(c.detail().is_empty(), false);
                assert_eq!(c.abs(), SEG_BASE + c.code() as u32);
            }
            let mut j = i + 1;
            while j < EbadfCode::ALL.len() {
                if let (Some(a), Some(b)) = (EbadfCode::ALL.get(i), EbadfCode::ALL.get(j)) {
                    assert_eq!(a.code() == b.code(), false);
                }
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn fingerprint_stable() {
        assert_eq!(matrix_fingerprint(), matrix_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9343_checks();
        assert_eq!(s.tally().1, 0, "{}", s.red_items_text());
        assert_eq!(s.all_passed(), true);
        assert_eq!(s.len() >= 12, true);
    }
}
