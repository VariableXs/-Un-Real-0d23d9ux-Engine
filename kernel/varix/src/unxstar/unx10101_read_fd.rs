//! UNX-F10101 read 正反双判据（常规读达成与负 fd 误用受控） · 0x92940 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10101（read 系统调用正反双判据：J1 常规读达成，J2 负 fd
//!       误用受控）。
//! 锚点：UNX-F10102-J1 write 常规写达成（LTP write01/write03 面落盘计数
//!       一致 10/10 次）；UNX-F10102-J2 只读 fd 写误用受控（预期 EBADF
//!       精确复现 10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92940（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `ReadCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 解析器 `parse_read_call`：调用行 "read|fd|want" 三成分逐段解析，
//!     fd 允许带负号（负值是 J2 的注入对象，解析层不吞），want 必为
//!     十进制且落在 (0..=4096]；
//!   - 注册表 `FaceRegistry`：LTP 面 read01/read03 闭集，每面确定性
//!     内容模板（byte i = face*13 + i*7 模 256），供读后内容一致复核；
//!   - 状态机 `ReadCycle`：Idle → Open → Reading → Eof 单向推进，
//!     读满面长转 EOF，EOF 后零字节读（合法返回 0）；
//!   - 账本 `ReadLedger`：逐面 reads/bytes 计数，守恒式
//!     Σbytes == 面长×轮数 才零偏离，计数漂移显性入账；
//!   - fd 表 `FdTable`：16 槽，负 fd / 越界 fd / 关闭后读逐类拒绝；
//!   - 注入核 `BadReadCase`：负 fd / 越界 fd / 关闭后读 / 零长 /
//!     计数漂移 五类坏样本逐类 10/10 次拒绝，账守恒逃逸=0；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10101 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x92940;

/// 段窗上界（下一卡 0x92980 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x92980;

/// fd 表容量（本卡模拟 fd 上限）。
pub const FD_CAP: usize = 16;

/// LTP read01 面标识。
pub const READ01: u32 = 1;

/// LTP read03 面标识。
pub const READ03: u32 = 3;

/// 每面内容模板字节数（读预算 = 2 面 × 64B = 128B）。
pub const FACE_BYTES: usize = 64;

/// 单次读请求上限（超限即 LenTooBig）。
pub const WANT_CAP: u32 = 4096;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EBADF: i32 = 9;
/// 非法参数（对照零长读族）。
pub const EINVAL: i32 = 22;
/// I/O 错误（对照计数漂移族）。
pub const EIO: i32 = 5;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReadCode {
    /// 缺操作成分（首段空或非 read）
    MissingOp,
    /// fd 段不可解析（空或非整数）
    BadFdForm,
    /// fd 越界（负值或 ≥ 16 槽）
    FdRange,
    /// fd 已关闭（槽位空）
    FdClosed,
    /// 零长读（want == 0）
    ZeroLen,
    /// 超长读（want > 4096）
    LenTooBig,
    /// 面标识出闭集（只认 read01/read03）
    FaceUnknown,
    /// 计数漂移（登记字节数 ≠ 实读字节数）
    CountDrift,
    /// 重复打开同面且未复用槽（协议层禁止双占）
    Reopen,
    /// 状态倒退（EOF 后重回 Reading）
    StateRegress,
    /// 内容不一致（读出字节与面模板比对失败）
    ContentMismatch,
    /// 段位越界
    SegmentViolation,
}

impl ReadCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ReadCode::MissingOp => "缺操作成分被拒：首段必须是 read；请核对调用行",
            ReadCode::BadFdForm => "fd 段不可解析被拒：fd 必须是十进制整数（可带负号）",
            ReadCode::FdRange => "fd 越界被拒：负 fd 或 ≥16 槽均按 EBADF 受控复现",
            ReadCode::FdClosed => "关闭后读被拒：槽位已空；请先重新打开再读",
            ReadCode::ZeroLen => "零长读被拒：本协议 want 必须为正；请调整读长",
            ReadCode::LenTooBig => "超长读被拒：want 不得超过 4096；请分段读",
            ReadCode::FaceUnknown => "面标识出闭集被拒：只允许 read01/read03；请核对面号",
            ReadCode::CountDrift => "计数漂移被拒：登记字节数≠实读字节数；请回查账面",
            ReadCode::Reopen => "重复打开被拒：同面未释放不得二次占槽；请先关闭",
            ReadCode::StateRegress => "状态倒退被拒：EOF 后不得重回 Reading；请重新开面",
            ReadCode::ContentMismatch => "内容不一致被拒：读出字节与面模板比对失败；请复核",
            ReadCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x92940..=0x92980",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [ReadCode; 12] = [
        ReadCode::MissingOp,
        ReadCode::BadFdForm,
        ReadCode::FdRange,
        ReadCode::FdClosed,
        ReadCode::ZeroLen,
        ReadCode::LenTooBig,
        ReadCode::FaceUnknown,
        ReadCode::CountDrift,
        ReadCode::Reopen,
        ReadCode::StateRegress,
        ReadCode::ContentMismatch,
        ReadCode::SegmentViolation,
    ];
}

/// 诊断码到 Linux errno 的映射（反判据受控复现的对照口径）。
pub fn errno_of(code: ReadCode) -> i32 {
    match code {
        ReadCode::FdRange | ReadCode::FdClosed | ReadCode::BadFdForm => EBADF,
        ReadCode::ZeroLen | ReadCode::LenTooBig => EINVAL,
        ReadCode::CountDrift | ReadCode::ContentMismatch => EIO,
        _ => EINVAL,
    }
}

// ── 调用行解析器 ──────────────────────────────────────────────────────
/// 分段器：按 '|' 切三段，段数≠3 返回 None。
fn split3(line: &str) -> Option<[usize; 4]> {
    let b = line.as_bytes();
    let mut cuts = [0usize; 4];
    let mut n = 0usize;
    let mut i = 0usize;
    while i < b.len() {
        if b.get(i) == Some(&b'|') {
            if n >= 2 {
                return None;
            }
            cuts[n + 1] = i + 1;
            n += 1;
        }
        i += 1;
    }
    if n != 2 {
        return None;
    }
    cuts[3] = b.len();
    Some(cuts)
}

/// 调用行解析："read|fd|want" → (fd, want)。
///
/// fd 允许负号（负值留给 fd 表层按 EBADF 受控）；want 十进制、
/// 非零且 ≤4096。任一成分缺失/非法显性返回。
pub fn parse_read_call(line: &str) -> Result<(i32, u32), ReadCode> {
    let cuts = match split3(line) {
        Some(c) => c,
        None => return Err(ReadCode::MissingOp),
    };
    let op_seg = line.get(cuts[0]..cuts[1] - 1).unwrap_or("");
    let fd_seg = line.get(cuts[1]..cuts[2] - 1).unwrap_or("");
    let want_seg = line.get(cuts[2]..cuts[3]).unwrap_or("");
    if op_seg != "read" {
        return Err(ReadCode::MissingOp);
    }
    let fd = match fd_seg.parse::<i32>() {
        Ok(v) => v,
        Err(_) => return Err(ReadCode::BadFdForm),
    };
    let want = match want_seg.parse::<u32>() {
        Ok(v) => v,
        Err(_) => return Err(ReadCode::BadFdForm),
    };
    if want == 0 {
        return Err(ReadCode::ZeroLen);
    }
    if want > WANT_CAP {
        return Err(ReadCode::LenTooBig);
    }
    Ok((fd, want))
}

// ── LTP 面注册表（确定性内容模板） ────────────────────────────────────
/// 一张 LTP 面：标识 + 确定性内容模板。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReadFace {
    /// 面标识（READ01/READ03）
    pub id: u32,
    /// 内容模板（byte i = id*13 + i*7 模 256，逐字节可复算）
    pub pattern: [u8; FACE_BYTES],
}

/// 面注册表：read01/read03 两面闭集。
#[derive(Clone, Copy, Debug)]
pub struct FaceRegistry {
    /// 两面（下标 0=read01，1=read03）
    pub faces: [ReadFace; 2],
}

impl FaceRegistry {
    /// 构造标准注册表（模板逐字节确定性生成）。
    pub const fn new() -> FaceRegistry {
        FaceRegistry { faces: [ReadFace { id: READ01, pattern: [0u8; FACE_BYTES] }, ReadFace { id: READ03, pattern: [0u8; FACE_BYTES] }] }
    }

    /// 初始化模板：逐字节按 id*13+i*7 模 256 写入，幂等可复算。
    pub fn init(&mut self) {
        let mut f = 0usize;
        while f < self.faces.len() {
            let mut i = 0usize;
            if let Some(face) = self.faces.get_mut(f) {
                while i < FACE_BYTES {
                    face.pattern[i] = ((face.id * 13 + (i as u32) * 7) & 0xFF) as u8;
                    i += 1;
                }
            }
            f += 1;
        }
    }

    /// 按面标识取模板（闭集外返回 None）。
    pub fn face_of(&self, id: u32) -> Option<&ReadFace> {
        let mut i = 0usize;
        while i < self.faces.len() {
            if let Some(f) = self.faces.get(i) {
                if f.id == id {
                    return Some(f);
                }
            }
            i += 1;
        }
        None
    }

    /// 内容一致复核：读出分段与模板 [start, start+len) 逐字节比对。
    /// 起点越界或超出面长即 ContentMismatch。
    pub fn verify(&self, id: u32, start: usize, buf: &[u8]) -> Result<(), ReadCode> {
        match self.face_of(id) {
            None => Err(ReadCode::FaceUnknown),
            Some(f) => {
                if start + buf.len() > f.pattern.len() {
                    return Err(ReadCode::ContentMismatch);
                }
                let mut i = 0usize;
                while i < buf.len() {
                    if let (Some(a), Some(b)) = (buf.get(i), f.pattern.get(start + i)) {
                        if a != b {
                            return Err(ReadCode::ContentMismatch);
                        }
                    }
                    i += 1;
                }
                Ok(())
            }
        }
    }
}

// ── fd 表与读周期状态机 ───────────────────────────────────────────────
/// 读周期状态机：Idle → Open → Reading → Eof 单向推进。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReadCycle {
    /// 空闲（槽位未用）
    Idle,
    /// 已打开（面已绑定）
    Open,
    /// 读进行中（偏移在 0..面长）
    Reading,
    /// 已读尽（EOF，之后读返回 0 字节）
    Eof,
}

/// 一个 fd 槽位：绑定面 + 偏移 + 周期态。
#[derive(Clone, Copy, Debug)]
pub struct FdEntry {
    /// 绑定面标识
    pub face: u32,
    /// 当前偏移（0..=FACE_BYTES）
    pub offset: usize,
    /// 周期态
    pub cycle: ReadCycle,
}

/// fd 表：16 槽，负 fd/越界/关闭后读逐类拒绝。
#[derive(Clone, Debug)]
pub struct FdTable {
    /// 槽位数组（None=空槽）
    pub slots: [Option<FdEntry>; FD_CAP],
    /// 累计读次数
    pub reads: u32,
    /// 累计读字节数
    pub bytes: u32,
}

impl FdTable {
    /// 构造空表。
    pub const fn new() -> FdTable {
        FdTable { slots: [None; FD_CAP], reads: 0, bytes: 0 }
    }

    /// 打开一张面：分配最低空槽；同面已开且未释放拒绝（防双占）。
    pub fn open(&mut self, face: u32) -> Result<usize, ReadCode> {
        if face != READ01 && face != READ03 {
            return Err(ReadCode::FaceUnknown);
        }
        let mut i = 0usize;
        while i < FD_CAP {
            if let Some(Some(e)) = self.slots.get(i) {
                if e.face == face {
                    return Err(ReadCode::Reopen);
                }
            }
            i += 1;
        }
        let mut slot = None;
        let mut j = 0usize;
        while j < FD_CAP {
            if let Some(None) = self.slots.get(j) {
                slot = Some(j);
                j = FD_CAP;
            } else {
                j += 1;
            }
        }
        match slot {
            None => Err(ReadCode::FdRange),
            Some(s) => {
                if let Some(t) = self.slots.get_mut(s) {
                    *t = Some(FdEntry { face, offset: 0, cycle: ReadCycle::Open });
                }
                Ok(s)
            }
        }
    }

    /// 关闭 fd：越界/已关闭显性拒绝；空槽回收可复用。
    pub fn close(&mut self, fd: usize) -> Result<(), ReadCode> {
        match self.slots.get(fd) {
            Some(Some(_)) => {
                if let Some(t) = self.slots.get_mut(fd) {
                    *t = None;
                }
                Ok(())
            }
            _ => Err(ReadCode::FdClosed),
        }
    }

    /// 常规读：从面模板偏移处取 min(want, 剩余) 字节；EOF 后返回 0。
    /// 负 fd / 越界 fd / 关闭后读逐类按 EBADF 受控复现。
    pub fn read(&mut self, fd: i32, want: u32, reg: &FaceRegistry) -> Result<(usize, Vec<u8>), ReadCode> {
        if fd < 0 {
            return Err(ReadCode::FdRange);
        }
        let fd_u = fd as usize;
        if fd_u >= FD_CAP {
            return Err(ReadCode::FdRange);
        }
        let entry = match self.slots.get(fd_u).copied() {
            Some(Some(e)) => e,
            _ => return Err(ReadCode::FdClosed),
        };
        if entry.cycle == ReadCycle::Eof {
            return Ok((0, Vec::new()));
        }
        let face = match reg.face_of(entry.face) {
            Some(f) => f,
            None => return Err(ReadCode::FaceUnknown),
        };
        let remain = FACE_BYTES - entry.offset;
        let n = if (want as usize) < remain { want as usize } else { remain };
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < n {
            if let Some(b) = face.pattern.get(entry.offset + i) {
                buf.push(*b);
            }
            i += 1;
        }
        let new_off = entry.offset + n;
        let new_cycle = if new_off >= FACE_BYTES { ReadCycle::Eof } else { ReadCycle::Reading };
        if let Some(Some(t)) = self.slots.get_mut(fd_u) {
            t.offset = new_off;
            t.cycle = new_cycle;
        }
        self.reads = self.reads.saturating_add(1);
        self.bytes = self.bytes.saturating_add(n as u32);
        Ok((n, buf))
    }
}

// ── 读账本 ────────────────────────────────────────────────────────────
/// 读账本：逐面 reads/bytes 计数，计数漂移显性入账不静默。
#[derive(Clone, Debug)]
pub struct ReadLedger {
    /// 逐面读次数（下标 0=read01，1=read03）
    pub per_face_reads: [u32; 2],
    /// 逐面累计字节
    pub per_face_bytes: [u32; 2],
    /// 计数漂移检出次数
    pub drifts: u32,
    /// 逃逸次数（坏账误入账，必须恒 0）
    pub escapes: u32,
}

impl ReadLedger {
    /// 构造零账。
    pub const fn new() -> ReadLedger {
        ReadLedger { per_face_reads: [0, 0], per_face_bytes: [0, 0], drifts: 0, escapes: 0 }
    }

    /// 登记一次读：面标识必检，claimed 与 actual 不符即计数漂移。
    pub fn record(&mut self, face: u32, claimed: u32, actual: u32) -> Result<(), ReadCode> {
        let idx = match face {
            READ01 => 0usize,
            READ03 => 1usize,
            _ => return Err(ReadCode::FaceUnknown),
        };
        if claimed != actual {
            self.drifts = self.drifts.saturating_add(1);
            return Err(ReadCode::CountDrift);
        }
        if let Some(c) = self.per_face_reads.get_mut(idx) {
            *c = c.saturating_add(1);
        }
        if let Some(b) = self.per_face_bytes.get_mut(idx) {
            *b = b.saturating_add(actual);
        }
        Ok(())
    }

    /// 守恒式：逐面 bytes == FACE_BYTES×expected 轮数才零偏离。
    pub fn deficit(&self, rounds: u32) -> Option<u32> {
        let expect = FACE_BYTES as u32 * rounds;
        let mut gap = 0u32;
        let mut i = 0usize;
        while i < self.per_face_bytes.len() {
            if let Some(b) = self.per_face_bytes.get(i) {
                gap = gap.saturating_add(expect.saturating_sub(*b));
            }
            i += 1;
        }
        if gap == 0 {
            None
        } else {
            Some(gap)
        }
    }
}

/// 标准语料：两面各开一槽、各读 FACE_BYTES 分 8 段、内容逐段复核入账。
pub fn standard_corpus(table: &mut FdTable, reg: &FaceRegistry, ledger: &mut ReadLedger) -> Result<(), ReadCode> {
    let f0 = table.open(READ01)?;
    let f1 = table.open(READ03)?;
    let mut round = 0u32;
    while round < 8 {
        let mut fi = 0usize;
        while fi < 2 {
            let fd = if fi == 0 { f0 } else { f1 };
            let face = if fi == 0 { READ01 } else { READ03 };
            let (n, buf) = table.read(fd as i32, 8, reg)?;
            reg.verify(face, round as usize * 8, &buf)?;
            ledger.record(face, n as u32, n as u32)?;
            fi += 1;
        }
        round += 1;
    }
    table.close(f0)?;
    table.close(f1)?;
    Ok(())
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏读样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadReadCase {
    /// 负 fd（J2 反判据主样本）
    NegativeFd,
    /// 越界 fd（≥16）
    RangeFd,
    /// 关闭后读
    ClosedFd,
    /// 零长读
    ZeroLen,
    /// 计数漂移（登记数≠实读数）
    CountDrift,
}

impl BadReadCase {
    /// 闭集全量。
    pub const ALL: [BadReadCase; 5] = [
        BadReadCase::NegativeFd,
        BadReadCase::RangeFd,
        BadReadCase::ClosedFd,
        BadReadCase::ZeroLen,
        BadReadCase::CountDrift,
    ];

    /// 注入一轮：构造坏样本并触发校验，返回是否被显式拒绝。
    pub fn inject(self, table: &mut FdTable, reg: &FaceRegistry, ledger: &mut ReadLedger) -> bool {
        match self {
            BadReadCase::NegativeFd => {
                table.read(-1, 8, reg).err().map(|e| errno_of(e) == EBADF) == Some(true)
            }
            BadReadCase::RangeFd => {
                table.read(FD_CAP as i32, 8, reg).err() == Some(ReadCode::FdRange)
            }
            BadReadCase::ClosedFd => {
                let f = match table.open(READ01) {
                    Ok(f) => f,
                    Err(_) => return false,
                };
                let closed = table.close(f).is_ok();
                closed && table.read(f as i32, 8, reg).err() == Some(ReadCode::FdClosed)
            }
            BadReadCase::ZeroLen => {
                parse_read_call("read|0|0").err() == Some(ReadCode::ZeroLen)
            }
            BadReadCase::CountDrift => {
                ledger.record(READ01, 8, 7).err() == Some(ReadCode::CountDrift)
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_read_injection() -> (ReadLedger, [u32; 5]) {
    let mut reg = FaceRegistry::new();
    reg.init();
    let mut table = FdTable::new();
    let mut ledger = ReadLedger::new();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadReadCase::ALL.len() {
        if let Some(case) = BadReadCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject(&mut table, &reg, &mut ledger);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                } else {
                    ledger.escapes = ledger.escapes.saturating_add(1);
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；负 fd/越界 fd/关闭后读族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；零长/超长读族"),
    ("EIO", -5, 1117, "Windows ERROR_GEN_FAILURE；计数漂移/内容不一致族"),
    ("ENODEV", -19, 55, "Windows ERROR_DEV_NOT_EXIST；面标识出闭集族"),
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

/// 规范语料双跑指纹：开面→分段读→复核→入账→关闭全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut reg = FaceRegistry::new();
    reg.init();
    let mut table = FdTable::new();
    let mut lg = ReadLedger::new();
    bytes.push(match standard_corpus(&mut table, &reg, &mut lg) {
        Ok(()) => 1,
        Err(_) => 2,
    });
    bytes.extend_from_slice(&table.reads.to_le_bytes());
    bytes.extend_from_slice(&table.bytes.to_le_bytes());
    if let Some(v) = lg.per_face_bytes.get(0) {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    if let Some(v) = lg.per_face_bytes.get(1) {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    bytes.push(match lg.deficit(1) {
        None => 3,
        Some(_) => 4,
    });
    bytes.push(lg.drifts as u8);
    bytes.push(lg.escapes as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10101 判据集（18 条静态 + 3 条动态明细）。
pub fn run_unx10101_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10101");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x92940..=0x92980", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝");

    // 2. 标准语料：两面各 8 段读全、内容一致、账零偏离
    let mut reg = FaceRegistry::new();
    reg.init();
    let mut table = FdTable::new();
    let mut lg = ReadLedger::new();
    let corpus_ok = standard_corpus(&mut table, &reg, &mut lg).is_ok();
    s.add("常规读达成：read01/read03 各 8 段读全且内容一致", corpus_ok,
          "J1 正判据：分段读出字节与面模板逐字节复核全过");

    // 3. 账守恒：Σbytes == 2 面 × 64B、Σreads == 16
    let conservation = table.bytes == FACE_BYTES as u32 * 2
        && table.reads == 16
        && lg.deficit(1).is_none();
    s.add("账守恒：Σbytes=128 且逐面零缺口", conservation,
          "计数账与实读逐字节守恒，漂移即显性");

    // 4. 负 fd 10/10 EBADF 精确复现（J2 反判据）
    let mut neg_hits = 0u32;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let r = table.read(-1, 8, &reg);
        if r.err().map(|e| errno_of(e) == EBADF) == Some(true) {
            neg_hits += 1;
        }
        round += 1;
    }
    s.add("负 fd：10/10 次 EBADF 精确复现", neg_hits == INJECT_ROUNDS,
          "J2 反判据：fd=-1 逐次按 EBADF 受控复现，不静默");

    // 5. 越界 fd 拒
    let range_rej = table.read(FD_CAP as i32, 8, &reg).err() == Some(ReadCode::FdRange);
    s.add("越界 fd：fd=16 出窗拒收", range_rej,
          "fd 表 16 槽，越界读按 EBADF 族受控");

    // 6. 关闭后读拒
    let f = table.open(READ01).ok().unwrap_or(FD_CAP);
    let _ = table.close(f);
    let closed_rej = table.read(f as i32, 8, &reg).err() == Some(ReadCode::FdClosed);
    s.add("关闭后读：空槽读拒收", closed_rej,
          "槽位已空的 fd 读必须显性拒绝");

    // 7. 零长/超长读拒
    let zero_rej = parse_read_call("read|0|0").err() == Some(ReadCode::ZeroLen);
    let big_rej = parse_read_call("read|0|4097").err() == Some(ReadCode::LenTooBig);
    s.add("读长边界：零长拒、4097 超限拒", zero_rej && big_rej,
          "want 必须落在 (0..=4096] 闭开区间");

    // 8. 协议解析缺成分拒
    let op_rej = parse_read_call("seek|0|8").err() == Some(ReadCode::MissingOp);
    let fd_rej = parse_read_call("read|x|8").err() == Some(ReadCode::BadFdForm);
    let seg_rej = parse_read_call("read|0").err() == Some(ReadCode::MissingOp);
    s.add("协议解析：非法 op/非数字 fd/段残全拒", op_rej && fd_rej && seg_rej,
          "调用行恰三段 read|fd|want，逐段显性核");

    // 9. 面闭集拒
    let face_rej = table.open(5).err() == Some(ReadCode::FaceUnknown);
    let face_ok = reg.face_of(READ01).is_some() && reg.face_of(READ03).is_some();
    s.add("面闭集：read01/read03 可查、面 5 拒", face_rej && face_ok,
          "LTP 面标识闭集，闭集外一律 FaceUnknown");

    // 10. EOF 推进：读满面长转 EOF，EOF 后零字节读合法返 0
    let mut eof_t = FdTable::new();
    let _ = eof_t.open(READ01);
    let mut total = 0usize;
    let mut eof_ok = true;
    let mut guard = 0u32;
    while eof_ok && guard < 64 {
        match eof_t.read(0, 16, &reg) {
            Ok((n, _)) => total += n,
            Err(_) => eof_ok = false,
        }
        guard += 1;
    }
    let zero_after = eof_t.read(0, 16, &reg) == Ok((0, Vec::new()));
    s.add("EOF 推进：64B 读尽转 EOF 后零字节返 0", eof_ok && total == FACE_BYTES && zero_after,
          "Idle→Open→Reading→Eof 单向，EOF 后零读合法");

    // 11. 重复打开拒
    let mut ro_t = FdTable::new();
    let _ = ro_t.open(READ01);
    let reopen_rej = ro_t.open(READ01).err() == Some(ReadCode::Reopen);
    s.add("防双占：同面未释放二次打开拒收", reopen_rej,
          "一面一槽，双占显性拒绝");

    // 12. 计数漂移检出
    let mut dl = ReadLedger::new();
    let drift_rej = dl.record(READ01, 8, 7).err() == Some(ReadCode::CountDrift);
    s.add("计数漂移：登记≠实读逐次检出", drift_rej && dl.drifts == 1,
          "账面计数与实读逐字节比对，漂移显性入账");

    // 13. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_read_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "负 fd/越界 fd/关闭后读/零长/计数漂移全被显式拒收");

    // 14. 账守恒逃逸=0
    s.add("账守恒：注入过程逃逸=0",
          ledger.escapes == 0 && ledger.per_face_bytes == [0, 0],
          "注入核全程不得产生坏账逃逸入账");

    // 15. 文案闭集：非空
    let text_ok = ReadCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i2 = 0usize;
    while i2 < ReadCode::ALL.len() {
        let mut j = i2 + 1;
        while j < ReadCode::ALL.len() {
            if let (Some(x), Some(y)) = (ReadCode::ALL.get(i2), ReadCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i2 += 1;
    }
    let in_seg = ReadCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 17. Linux 逐码一致
    let codes_ok = agrees_with_linux(EBADF)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(EIO)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 18. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 动态明细（≤8）：fd 快照 + 账快照 + 指纹快照
    let used = table.slots.iter().filter(|e| e.is_some()).count();
    let fd_text = format!("fd 占用 {}/16，累计读 {} 次 / {} 字节", used, table.reads, table.bytes);
    s.add_dyn("fd 快照", used <= FD_CAP, &fd_text);
    let lg_text = format!("read01={}B read03={}B", lg.per_face_bytes[0], lg.per_face_bytes[1]);
    s.add_dyn("账快照", lg.deficit(1).is_none(), &lg_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ok_read_call() {
        assert_eq!(parse_read_call("read|3|8"), Ok((3, 8)));
        assert_eq!(parse_read_call("read|-1|16"), Ok((-1, 16)));
    }

    #[test]
    fn parse_rejects_bad_shapes() {
        assert_eq!(parse_read_call("seek|0|8"), Err(ReadCode::MissingOp));
        assert_eq!(parse_read_call("read|x|8"), Err(ReadCode::BadFdForm));
        assert_eq!(parse_read_call("read|0|0"), Err(ReadCode::ZeroLen));
        assert_eq!(parse_read_call("read|0|4097"), Err(ReadCode::LenTooBig));
        assert_eq!(parse_read_call("read|0"), Err(ReadCode::MissingOp));
    }

    #[test]
    fn pattern_deterministic() {
        let mut a = FaceRegistry::new();
        a.init();
        let mut b = FaceRegistry::new();
        b.init();
        assert_eq!(a.faces[0].pattern, b.faces[0].pattern);
        assert_eq!(a.faces[1].pattern, b.faces[1].pattern);
    }

    #[test]
    fn standard_corpus_conserves() {
        let mut reg = FaceRegistry::new();
        reg.init();
        let mut t = FdTable::new();
        let mut lg = ReadLedger::new();
        assert!(standard_corpus(&mut t, &reg, &mut lg).is_ok());
        assert_eq!(t.bytes, FACE_BYTES as u32 * 2);
        assert!(lg.deficit(1).is_none());
    }

    #[test]
    fn negative_fd_is_ebadf() {
        let mut reg = FaceRegistry::new();
        reg.init();
        let mut t = FdTable::new();
        let e = t.read(-1, 8, &reg).err().unwrap();
        assert_eq!(errno_of(e), EBADF);
    }

    #[test]
    fn closed_fd_rejected() {
        let mut reg = FaceRegistry::new();
        reg.init();
        let mut t = FdTable::new();
        let f = t.open(READ01).ok().unwrap();
        assert!(t.close(f).is_ok());
        assert_eq!(t.read(f as i32, 8, &reg), Err(ReadCode::FdClosed));
    }

    #[test]
    fn eof_then_zero_read() {
        let mut reg = FaceRegistry::new();
        reg.init();
        let mut t = FdTable::new();
        let _ = t.open(READ01);
        let mut guard = 0u32;
        let mut total = 0usize;
        while guard < 8 {
            if let Ok((n, _)) = t.read(0, 8, &reg) {
                total += n;
            }
            guard += 1;
        }
        assert_eq!(total, FACE_BYTES);
        assert_eq!(t.read(0, 8, &reg), Ok((0, Vec::new())));
    }

    #[test]
    fn face_closed_set() {
        let reg = FaceRegistry::new();
        assert!(reg.face_of(READ01).is_some());
        assert!(reg.face_of(READ03).is_some());
        assert!(reg.face_of(5).is_none());
    }

    #[test]
    fn drift_detected() {
        let mut lg = ReadLedger::new();
        assert_eq!(lg.record(READ01, 8, 7), Err(ReadCode::CountDrift));
        assert_eq!(lg.drifts, 1);
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_read_injection();
        assert_eq!(l.escapes, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10101_checks_green() {
        let s = run_unx10101_checks();
        assert!(s.all_passed(), "UNX-F10101 red: {}", s.red_items_text());
    }
}
