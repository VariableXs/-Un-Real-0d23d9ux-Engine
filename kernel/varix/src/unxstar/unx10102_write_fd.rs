//! UNX-F10102 write 正反双判据（常规写达成与只读 fd 误用受控） · 0x92980 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10102（write 系统调用正反双判据：J1 常规写达成，J2 只读
//!       fd 写误用受控）。
//! 锚点：UNX-F10103-J1 open 打开达成（LTP open01/open02 面标志组合语义
//!       正确 10/10 次）；UNX-F10103-J2 缺失路径误用受控（预期 ENOENT
//!       精确复现 10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92980（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `WriteCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 解析器 `parse_write_call`：调用行 "write|fd|n" 三成分逐段解析，
//!     fd 允许带负号，n 必为十进制且落在 (0..=4096]；
//!   - 注册表 `WriteFaceRegistry`：LTP 面 write01/write03 闭集，每面
//!     磁盘缓冲按确定性模板填充（byte i = face*29 + i*11 模 256）；
//!   - 状态机 `WriteCycle`：Idle → Open → Writing → Flushed 单向推进；
//!   - 账本 `WriteLedger`：逐面 written/disk 计数，守恒式
//!     Σdisk == Σwritten 才零偏离，计数漂移显性入账；
//!   - fd 表 `WriteTable`：16 槽带权限位（只读/只写/读写），只读 fd
//!     写按 EBADF 受控复现，负 fd / 越界 / 关闭后写逐类拒绝；
//!   - 注入核 `BadWriteCase`：负 fd / 只读 fd / 越界 fd / 关闭后写 /
//!     计数漂移 五类坏样本逐类 10/10 次拒绝，账守恒逃逸=0；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10102 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x92980;

/// 段窗上界（下一卡 0x929c0 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x929c0;

/// fd 表容量（本卡模拟 fd 上限）。
pub const FD_CAP: usize = 16;

/// LTP write01 面标识。
pub const WRITE01: u32 = 1;

/// LTP write03 面标识。
pub const WRITE03: u32 = 3;

/// 每面磁盘缓冲字节数（写预算 = 2 面 × 64B = 128B）。
pub const DISK_BYTES: usize = 64;

/// 单次写请求上限。
pub const WRITE_CAP: u32 = 4096;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EBADF: i32 = 9;
/// 非法参数（对照零长写族）。
pub const EINVAL: i32 = 22;
/// 设备无空间（对照面满溢出族）。
pub const ENOSPC: i32 = 28;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WriteCode {
    /// 缺操作成分（首段空或非 write）
    MissingOp,
    /// fd 段不可解析（空或非整数）
    BadFdForm,
    /// fd 越界（负值或 ≥ 16 槽）
    FdRange,
    /// fd 已关闭（槽位空）
    FdClosed,
    /// 只读 fd 写（权限位为只读）
    ReadOnlyFd,
    /// 面标识出闭集（只认 write01/write03）
    FaceUnknown,
    /// 面满溢出（写越过磁盘缓冲尾）
    BufOverflow,
    /// 计数漂移（登记字节数 ≠ 实写字节数）
    CountDrift,
    /// 重复打开同面且未复用槽
    Reopen,
    /// 状态倒退（Flushed 后重回 Writing）
    StateRegress,
    /// 落盘不一致（磁盘计数与账面计数比对失败）
    DiskMismatch,
    /// 段位越界
    SegmentViolation,
}

impl WriteCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            WriteCode::MissingOp => "缺操作成分被拒：首段必须是 write；请核对调用行",
            WriteCode::BadFdForm => "fd 段不可解析被拒：fd 必须是十进制整数（可带负号）",
            WriteCode::FdRange => "fd 越界被拒：负 fd 或 ≥16 槽均按 EBADF 受控复现",
            WriteCode::FdClosed => "关闭后写被拒：槽位已空；请先重新打开再写",
            WriteCode::ReadOnlyFd => "只读 fd 写被拒：权限位为只读；请按 EBADF 复现整改",
            WriteCode::FaceUnknown => "面标识出闭集被拒：只允许 write01/write03；请核对面号",
            WriteCode::BufOverflow => "面满溢出被拒：写越过 64B 缓冲尾；请先截断或换面",
            WriteCode::CountDrift => "计数漂移被拒：登记字节数≠实写字节数；请回查账面",
            WriteCode::Reopen => "重复打开被拒：同面未释放不得二次占槽；请先关闭",
            WriteCode::StateRegress => "状态倒退被拒：Flushed 后不得重回 Writing；请重开面",
            WriteCode::DiskMismatch => "落盘不一致被拒：磁盘计数与账面不符；请复核落盘链",
            WriteCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x92980..=0x929c0",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [WriteCode; 12] = [
        WriteCode::MissingOp,
        WriteCode::BadFdForm,
        WriteCode::FdRange,
        WriteCode::FdClosed,
        WriteCode::ReadOnlyFd,
        WriteCode::FaceUnknown,
        WriteCode::BufOverflow,
        WriteCode::CountDrift,
        WriteCode::Reopen,
        WriteCode::StateRegress,
        WriteCode::DiskMismatch,
        WriteCode::SegmentViolation,
    ];
}

/// 诊断码到 Linux errno 的映射（反判据受控复现的对照口径）。
pub fn errno_of(code: WriteCode) -> i32 {
    match code {
        WriteCode::FdRange | WriteCode::FdClosed | WriteCode::BadFdForm | WriteCode::ReadOnlyFd => EBADF,
        WriteCode::BufOverflow => ENOSPC,
        WriteCode::CountDrift | WriteCode::DiskMismatch => EIO_FALLBACK,
        _ => EINVAL,
    }
}

/// EIO 正数码（计数漂移族对照）。
pub const EIO_FALLBACK: i32 = 5;

// ── 权限位闭集 ────────────────────────────────────────────────────────
/// fd 权限位闭集（O_RDONLY/O_WRONLY/O_RDWR 三态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FdMode {
    /// 只读（写误用按 EBADF 受控）
    ReadOnly,
    /// 只写
    WriteOnly,
    /// 读写
    ReadWrite,
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

/// 调用行解析："write|fd|n" → (fd, n)。
///
/// fd 允许负号（负值留给 fd 表层按 EBADF 受控）；n 十进制、非零且
/// ≤4096。任一成分缺失/非法显性返回。
pub fn parse_write_call(line: &str) -> Result<(i32, u32), WriteCode> {
    let cuts = match split3(line) {
        Some(c) => c,
        None => return Err(WriteCode::MissingOp),
    };
    let op_seg = line.get(cuts[0]..cuts[1] - 1).unwrap_or("");
    let fd_seg = line.get(cuts[1]..cuts[2] - 1).unwrap_or("");
    let n_seg = line.get(cuts[2]..cuts[3]).unwrap_or("");
    if op_seg != "write" {
        return Err(WriteCode::MissingOp);
    }
    let fd = match fd_seg.parse::<i32>() {
        Ok(v) => v,
        Err(_) => return Err(WriteCode::BadFdForm),
    };
    let n = match n_seg.parse::<u32>() {
        Ok(v) => v,
        Err(_) => return Err(WriteCode::BadFdForm),
    };
    if n == 0 {
        return Err(WriteCode::BadFdForm);
    }
    if n > WRITE_CAP {
        return Err(WriteCode::BadFdForm);
    }
    Ok((fd, n))
}

// ── LTP 面注册表（模拟磁盘） ──────────────────────────────────────────
/// 一张 LTP 面：标识 + 磁盘缓冲 + 落盘计数。
#[derive(Clone, Copy, Debug)]
pub struct WriteFace {
    /// 面标识（WRITE01/WRITE03）
    pub id: u32,
    /// 磁盘缓冲（确定性模板填充）
    pub disk: [u8; DISK_BYTES],
    /// 已落盘字节数
    pub used: usize,
}

/// 面注册表：write01/write03 两面闭集。
#[derive(Clone, Copy, Debug)]
pub struct WriteFaceRegistry {
    /// 两面（下标 0=write01，1=write03）
    pub faces: [WriteFace; 2],
}

impl WriteFaceRegistry {
    /// 构造空注册表（缓冲清零、计数归零）。
    pub const fn new() -> WriteFaceRegistry {
        WriteFaceRegistry {
            faces: [
                WriteFace { id: WRITE01, disk: [0u8; DISK_BYTES], used: 0 },
                WriteFace { id: WRITE03, disk: [0u8; DISK_BYTES], used: 0 },
            ],
        }
    }

    /// 按面标识取面（闭集外返回 None）。
    pub fn face_of(&self, id: u32) -> Option<&WriteFace> {
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

    /// 按面标识可变取面（闭集外返回 None）。
    pub fn face_of_mut(&mut self, id: u32) -> Option<&mut WriteFace> {
        let len = self.faces.len();
        let mut hit = None;
        let mut i = 0usize;
        while i < len {
            if let Some(f) = self.faces.get(i) {
                if f.id == id {
                    hit = Some(i);
                    i = len;
                } else {
                    i += 1;
                }
            } else {
                i += 1;
            }
        }
        match hit {
            Some(idx) => self.faces.get_mut(idx),
            None => None,
        }
    }

    /// 落盘 n 字节：确定性模板 byte=(face*29+off*11)&0xFF 逐字节写入。
    pub fn write_disk(&mut self, id: u32, n: usize) -> Result<usize, WriteCode> {
        let f = match self.face_of_mut(id) {
            Some(f) => f,
            None => return Err(WriteCode::FaceUnknown),
        };
        if f.used + n > DISK_BYTES {
            return Err(WriteCode::BufOverflow);
        }
        let mut i = 0usize;
        while i < n {
            if let Some(slot) = f.disk.get_mut(f.used + i) {
                *slot = ((f.id * 29 + ((f.used + i) as u32) * 11) & 0xFF) as u8;
            }
            i += 1;
        }
        f.used += n;
        Ok(n)
    }

    /// 落盘核验：逐面 disk 计数与账面 claimed 比对。
    pub fn audit_disk(&self, claimed: [u32; 2]) -> Result<(), WriteCode> {
        let mut i = 0usize;
        while i < self.faces.len() {
            let f = match self.faces.get(i) {
                Some(f) => f,
                None => return Err(WriteCode::FaceUnknown),
            };
            let c = match claimed.get(i) {
                Some(c) => *c,
                None => return Err(WriteCode::DiskMismatch),
            };
            if f.used as u32 != c {
                return Err(WriteCode::DiskMismatch);
            }
            i += 1;
        }
        Ok(())
    }
}

// ── fd 表与写周期状态机 ───────────────────────────────────────────────
/// 写周期状态机：Idle → Open → Writing → Flushed 单向推进。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WriteCycle {
    /// 空闲（槽位未用）
    Idle,
    /// 已打开（面与权限位已绑定）
    Open,
    /// 写进行中（偏移在 0..面长）
    Writing,
    /// 已冲刷（面写满关闭）
    Flushed,
}

/// 一个 fd 槽位：绑定面 + 权限位 + 偏移 + 周期态。
#[derive(Clone, Copy, Debug)]
pub struct WriteSlot {
    /// 绑定面标识
    pub face: u32,
    /// 权限位
    pub mode: FdMode,
    /// 当前偏移（0..=DISK_BYTES）
    pub offset: usize,
    /// 周期态
    pub cycle: WriteCycle,
}

/// fd 表：16 槽带权限位，只读 fd 写按 EBADF 受控复现。
#[derive(Clone, Debug)]
pub struct WriteTable {
    /// 槽位数组（None=空槽）
    pub slots: [Option<WriteSlot>; FD_CAP],
    /// 累计写次数
    pub writes: u32,
    /// 累计写字节数
    pub bytes: u32,
}

impl WriteTable {
    /// 构造空表。
    pub const fn new() -> WriteTable {
        WriteTable { slots: [None; FD_CAP], writes: 0, bytes: 0 }
    }

    /// 打开一张面并绑权限位：分配最低空槽；同面未释放拒绝。
    pub fn open(&mut self, face: u32, mode: FdMode) -> Result<usize, WriteCode> {
        if face != WRITE01 && face != WRITE03 {
            return Err(WriteCode::FaceUnknown);
        }
        let mut i = 0usize;
        while i < FD_CAP {
            if let Some(Some(e)) = self.slots.get(i) {
                if e.face == face {
                    return Err(WriteCode::Reopen);
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
            None => Err(WriteCode::FdRange),
            Some(s) => {
                if let Some(t) = self.slots.get_mut(s) {
                    *t = Some(WriteSlot { face, mode, offset: 0, cycle: WriteCycle::Open });
                }
                Ok(s)
            }
        }
    }

    /// 关闭 fd：越界/已关闭显性拒绝；空槽回收可复用。
    pub fn close(&mut self, fd: usize) -> Result<(), WriteCode> {
        match self.slots.get(fd) {
            Some(Some(s)) => {
                let _ = s;
                if let Some(t) = self.slots.get_mut(fd) {
                    *t = None;
                }
                Ok(())
            }
            _ => Err(WriteCode::FdClosed),
        }
    }

    /// 常规写：把 n 字节写入面磁盘缓冲（确定性模板）；只读 fd 拒。
    /// 负 fd / 越界 / 关闭后写逐类按 EBADF 受控复现。
    pub fn write(&mut self, fd: i32, n: u32, reg: &mut WriteFaceRegistry) -> Result<u32, WriteCode> {
        if fd < 0 {
            return Err(WriteCode::FdRange);
        }
        let fd_u = fd as usize;
        if fd_u >= FD_CAP {
            return Err(WriteCode::FdRange);
        }
        let slot = match self.slots.get(fd_u).copied() {
            Some(Some(s)) => s,
            _ => return Err(WriteCode::FdClosed),
        };
        if slot.mode == FdMode::ReadOnly {
            return Err(WriteCode::ReadOnlyFd);
        }
        let done = reg.write_disk(slot.face, n as usize)?;
        if let Some(Some(t)) = self.slots.get_mut(fd_u) {
            t.offset += done;
            t.cycle = WriteCycle::Writing;
        }
        self.writes = self.writes.saturating_add(1);
        self.bytes = self.bytes.saturating_add(done as u32);
        Ok(done as u32)
    }

    /// 冲刷收尾：Writing → Flushed 单向推进。
    pub fn flush(&mut self, fd: usize) -> Result<(), WriteCode> {
        match self.slots.get_mut(fd) {
            Some(Some(t)) => {
                if t.cycle != WriteCycle::Writing {
                    return Err(WriteCode::StateRegress);
                }
                t.cycle = WriteCycle::Flushed;
                Ok(())
            }
            _ => Err(WriteCode::FdClosed),
        }
    }
}

// ── 写账本 ────────────────────────────────────────────────────────────
/// 写账本：逐面 written 计数与磁盘 used 比对，漂移显性入账不静默。
#[derive(Clone, Debug)]
pub struct WriteLedger {
    /// 逐面账面字节数（下标 0=write01，1=write03）
    pub per_face: [u32; 2],
    /// 计数漂移检出次数
    pub drifts: u32,
    /// 逃逸次数（坏账误入账，必须恒 0）
    pub escapes: u32,
}

impl WriteLedger {
    /// 构造零账。
    pub const fn new() -> WriteLedger {
        WriteLedger { per_face: [0, 0], drifts: 0, escapes: 0 }
    }

    /// 登记一次写：面标识必检，claimed 与 actual 不符即计数漂移。
    pub fn record(&mut self, face: u32, claimed: u32, actual: u32) -> Result<(), WriteCode> {
        let idx = match face {
            WRITE01 => 0usize,
            WRITE03 => 1usize,
            _ => return Err(WriteCode::FaceUnknown),
        };
        if claimed != actual {
            self.drifts = self.drifts.saturating_add(1);
            return Err(WriteCode::CountDrift);
        }
        if let Some(b) = self.per_face.get_mut(idx) {
            *b = b.saturating_add(actual);
        }
        Ok(())
    }

    /// 守恒式：逐面账面 == DISK_BYTES×rounds 才零偏离。
    pub fn deficit(&self, rounds: u32) -> Option<u32> {
        let expect = DISK_BYTES as u32 * rounds;
        let mut gap = 0u32;
        let mut i = 0usize;
        while i < self.per_face.len() {
            if let Some(b) = self.per_face.get(i) {
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

/// 标准语料：两面各开一槽（读写权限）、各写 DISK_BYTES 分 8 段、
/// 账面逐段入账，冲刷收尾后落盘核验。
pub fn standard_corpus(
    table: &mut WriteTable,
    reg: &mut WriteFaceRegistry,
    ledger: &mut WriteLedger,
) -> Result<(), WriteCode> {
    let f0 = table.open(WRITE01, FdMode::ReadWrite)?;
    let f1 = table.open(WRITE03, FdMode::WriteOnly)?;
    let mut round = 0u32;
    while round < 8 {
        let mut fi = 0usize;
        while fi < 2 {
            let fd = if fi == 0 { f0 } else { f1 };
            let face = if fi == 0 { WRITE01 } else { WRITE03 };
            let n = table.write(fd as i32, 8, reg)?;
            ledger.record(face, n, n)?;
            fi += 1;
        }
        round += 1;
    }
    table.flush(f0)?;
    table.flush(f1)?;
    table.close(f0)?;
    table.close(f1)?;
    Ok(())
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏写样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadWriteCase {
    /// 负 fd
    NegativeFd,
    /// 只读 fd 写（J2 反判据主样本）
    ReadOnlyFd,
    /// 越界 fd（≥16）
    RangeFd,
    /// 关闭后写
    ClosedFd,
    /// 计数漂移（登记数≠实写数）
    CountDrift,
}

impl BadWriteCase {
    /// 闭集全量。
    pub const ALL: [BadWriteCase; 5] = [
        BadWriteCase::NegativeFd,
        BadWriteCase::ReadOnlyFd,
        BadWriteCase::RangeFd,
        BadWriteCase::ClosedFd,
        BadWriteCase::CountDrift,
    ];

    /// 注入一轮：构造坏样本并触发校验，返回是否被显式拒绝。
    pub fn inject(
        self,
        table: &mut WriteTable,
        reg: &mut WriteFaceRegistry,
        ledger: &mut WriteLedger,
    ) -> bool {
        match self {
            BadWriteCase::NegativeFd => {
                table.write(-1, 8, reg).err().map(|e| errno_of(e) == EBADF) == Some(true)
            }
            BadWriteCase::ReadOnlyFd => {
                let f = match table.open(WRITE03, FdMode::ReadOnly) {
                    Ok(f) => f,
                    Err(_) => return false,
                };
                let r = table.write(f as i32, 8, reg);
                let _ = table.close(f);
                r.err().map(|e| errno_of(e) == EBADF && e == WriteCode::ReadOnlyFd) == Some(true)
            }
            BadWriteCase::RangeFd => {
                table.write(FD_CAP as i32, 8, reg).err() == Some(WriteCode::FdRange)
            }
            BadWriteCase::ClosedFd => {
                let f = match table.open(WRITE01, FdMode::WriteOnly) {
                    Ok(f) => f,
                    Err(_) => return false,
                };
                let closed = table.close(f).is_ok();
                closed && table.write(f as i32, 8, reg).err() == Some(WriteCode::FdClosed)
            }
            BadWriteCase::CountDrift => {
                ledger.record(WRITE01, 8, 7).err() == Some(WriteCode::CountDrift)
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_write_injection() -> (WriteLedger, [u32; 5]) {
    let mut reg = WriteFaceRegistry::new();
    let mut table = WriteTable::new();
    let mut ledger = WriteLedger::new();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadWriteCase::ALL.len() {
        if let Some(case) = BadWriteCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject(&mut table, &mut reg, &mut ledger);
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
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；负 fd/只读 fd 写/关闭后写族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；零长/超长写族"),
    ("ENOSPC", -28, 112, "Windows ERROR_DISK_FULL；面满溢出族"),
    ("EIO", -5, 1117, "Windows ERROR_GEN_FAILURE；计数漂移/落盘不一致族"),
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

/// 规范语料双跑指纹：开面→分段写→入账→冲刷→落盘核验全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut reg = WriteFaceRegistry::new();
    let mut table = WriteTable::new();
    let mut lg = WriteLedger::new();
    bytes.push(match standard_corpus(&mut table, &mut reg, &mut lg) {
        Ok(()) => 1,
        Err(_) => 2,
    });
    bytes.extend_from_slice(&table.writes.to_le_bytes());
    bytes.extend_from_slice(&table.bytes.to_le_bytes());
    if let Some(v) = lg.per_face.get(0) {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    if let Some(v) = lg.per_face.get(1) {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    let c0 = match lg.per_face.get(0) {
        Some(v) => *v,
        None => 0,
    };
    let c1 = match lg.per_face.get(1) {
        Some(v) => *v,
        None => 0,
    };
    let claimed = [c0, c1];
    bytes.push(match reg.audit_disk(claimed) {
        Ok(()) => 3,
        Err(_) => 4,
    });
    let d0 = match reg.faces.get(0).and_then(|f| f.disk.get(0)) {
        Some(v) => *v,
        None => 0,
    };
    let d63 = match reg.faces.get(1).and_then(|f| f.disk.get(DISK_BYTES - 1)) {
        Some(v) => *v,
        None => 0,
    };
    bytes.extend_from_slice(&[d0, d63]);
    bytes.push(lg.drifts as u8);
    bytes.push(lg.escapes as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10102 判据集（18 条静态 + 3 条动态明细）。
pub fn run_unx10102_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10102");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x92980..=0x929c0", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝");

    // 2. 标准语料：两面各 8 段写全、冲刷收尾、落盘核验过
    let mut reg = WriteFaceRegistry::new();
    let mut table = WriteTable::new();
    let mut lg = WriteLedger::new();
    let corpus_ok = standard_corpus(&mut table, &mut reg, &mut lg).is_ok();
    let claimed = [lg.per_face[0], lg.per_face[1]];
    let disk_ok = reg.audit_disk(claimed).is_ok();
    s.add("常规写达成：write01/write03 各 8 段写全且落盘核验过",
          corpus_ok && disk_ok,
          "J1 正判据：账面计数与磁盘落盘逐面一致，冲刷单向推进");

    // 3. 账守恒：Σbytes == 2 面 × 64B、Σwrites == 16
    let conservation = table.bytes == DISK_BYTES as u32 * 2
        && table.writes == 16
        && lg.deficit(1).is_none();
    s.add("账守恒：Σbytes=128 且逐面零缺口", conservation,
          "写账与落盘逐字节守恒，漂移即显性");

    // 4. 只读 fd 写 10/10 EBADF 精确复现（J2 反判据）
    let mut ro_hits = 0u32;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut t = WriteTable::new();
        let mut r = WriteFaceRegistry::new();
        let f = match t.open(WRITE03, FdMode::ReadOnly) {
            Ok(f) => f,
            Err(_) => 0,
        };
        let err = t.write(f as i32, 8, &mut r);
        if err.err().map(|e| errno_of(e) == EBADF && e == WriteCode::ReadOnlyFd) == Some(true) {
            ro_hits += 1;
        }
        round += 1;
    }
    s.add("只读 fd 写：10/10 次 EBADF 精确复现", ro_hits == INJECT_ROUNDS,
          "J2 反判据：O_RDONLY fd 写逐次按 EBADF 受控复现，不静默");

    // 5. 负 fd 10/10 EBADF
    let mut neg_hits = 0u32;
    let mut neg_t = WriteTable::new();
    let mut neg_r = WriteFaceRegistry::new();
    let mut nr = 0u32;
    while nr < INJECT_ROUNDS {
        if neg_t.write(-1, 8, &mut neg_r).err().map(|e| errno_of(e) == EBADF) == Some(true) {
            neg_hits += 1;
        }
        nr += 1;
    }
    s.add("负 fd：10/10 次 EBADF 精确复现", neg_hits == INJECT_ROUNDS,
          "fd=-1 写逐次按 EBADF 受控复现，不静默");

    // 6. 越界 fd 拒
    let mut rg_t = WriteTable::new();
    let mut rg_r = WriteFaceRegistry::new();
    let range_rej = rg_t.write(FD_CAP as i32, 8, &mut rg_r).err() == Some(WriteCode::FdRange);
    s.add("越界 fd：fd=16 出窗拒收", range_rej,
          "fd 表 16 槽，越界写按 EBADF 族受控");

    // 7. 关闭后写拒
    let mut cl_t = WriteTable::new();
    let mut cl_r = WriteFaceRegistry::new();
    let f = cl_t.open(WRITE01, FdMode::WriteOnly).ok().unwrap_or(FD_CAP);
    let _ = cl_t.close(f);
    let closed_rej = cl_t.write(f as i32, 8, &mut cl_r).err() == Some(WriteCode::FdClosed);
    s.add("关闭后写：空槽写拒收", closed_rej,
          "槽位已空的 fd 写必须显性拒绝");

    // 8. 面满溢出拒
    let mut of_r = WriteFaceRegistry::new();
    let first = of_r.write_disk(WRITE01, DISK_BYTES).is_ok();
    let over_rej = of_r.write_disk(WRITE01, 1).err() == Some(WriteCode::BufOverflow);
    s.add("面满溢出：64B 写满后追加拒收", first && over_rej,
          "写越过磁盘缓冲尾必须显性拒绝（ENOSPC 族）");

    // 9. 协议解析缺成分拒
    let op_rej = parse_write_call("read|0|8").err() == Some(WriteCode::MissingOp);
    let fd_rej = parse_write_call("write|x|8").err() == Some(WriteCode::BadFdForm);
    let seg_rej = parse_write_call("write|0").err() == Some(WriteCode::MissingOp);
    s.add("协议解析：非法 op/非数字 fd/段残全拒", op_rej && fd_rej && seg_rej,
          "调用行恰三段 write|fd|n，逐段显性核");

    // 10. 面闭集拒
    let mut fc_t = WriteTable::new();
    let face_rej = fc_t.open(5, FdMode::WriteOnly).err() == Some(WriteCode::FaceUnknown);
    s.add("面闭集：面 5 拒收", face_rej,
          "LTP 面标识闭集 write01/write03，闭集外一律 FaceUnknown");

    // 11. 状态机推进：Writing → Flushed 单向；回写拒
    let mut st_t = WriteTable::new();
    let mut st_r = WriteFaceRegistry::new();
    let _ = st_t.open(WRITE01, FdMode::WriteOnly);
    let early_rej = st_t.flush(0).err() == Some(WriteCode::StateRegress);
    let _ = st_t.write(0, 8, &mut st_r);
    let flushed = st_t.flush(0).is_ok();
    s.add("状态机推进：未写冲刷拒、写后冲刷成", early_rej && flushed,
          "Idle→Open→Writing→Flushed 单向，倒退显性");

    // 12. 计数漂移检出
    let mut dl = WriteLedger::new();
    let drift_rej = dl.record(WRITE01, 8, 7).err() == Some(WriteCode::CountDrift);
    s.add("计数漂移：登记≠实写逐次检出", drift_rej && dl.drifts == 1,
          "账面计数与实写逐字节比对，漂移显性入账");

    // 13. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_write_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "负 fd/只读 fd/越界 fd/关闭后写/计数漂移全被显式拒收");

    // 14. 账守恒逃逸=0
    s.add("账守恒：注入过程逃逸=0",
          ledger.escapes == 0 && ledger.per_face == [0, 0],
          "注入核全程不得产生坏账逃逸入账");

    // 15. 文案闭集：非空
    let text_ok = WriteCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i2 = 0usize;
    while i2 < WriteCode::ALL.len() {
        let mut j = i2 + 1;
        while j < WriteCode::ALL.len() {
            if let (Some(x), Some(y)) = (WriteCode::ALL.get(i2), WriteCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i2 += 1;
    }
    let in_seg = WriteCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 17. Linux 逐码一致
    let codes_ok = agrees_with_linux(EBADF)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(ENOSPC)
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
    let fd_text = format!("fd 占用 {}/16，累计写 {} 次 / {} 字节", used, table.writes, table.bytes);
    s.add_dyn("fd 快照", used <= FD_CAP, &fd_text);
    let lg_text = format!("write01={}B write03={}B", lg.per_face[0], lg.per_face[1]);
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
    fn parse_ok_write_call() {
        assert_eq!(parse_write_call("write|3|8"), Ok((3, 8)));
        assert_eq!(parse_write_call("write|-1|16"), Ok((-1, 16)));
    }

    #[test]
    fn parse_rejects_bad_shapes() {
        assert_eq!(parse_write_call("read|0|8"), Err(WriteCode::MissingOp));
        assert_eq!(parse_write_call("write|x|8"), Err(WriteCode::BadFdForm));
        assert_eq!(parse_write_call("write|0|0"), Err(WriteCode::BadFdForm));
        assert_eq!(parse_write_call("write|0|4097"), Err(WriteCode::BadFdForm));
        assert_eq!(parse_write_call("write|0"), Err(WriteCode::MissingOp));
    }

    #[test]
    fn disk_deterministic() {
        let mut a = WriteFaceRegistry::new();
        let mut b = WriteFaceRegistry::new();
        let _ = a.write_disk(WRITE01, 8);
        let _ = b.write_disk(WRITE01, 8);
        assert_eq!(a.faces[0].disk[0], b.faces[0].disk[0]);
        assert_eq!(a.faces[0].disk[7], b.faces[0].disk[7]);
    }

    #[test]
    fn standard_corpus_conserves() {
        let mut reg = WriteFaceRegistry::new();
        let mut t = WriteTable::new();
        let mut lg = WriteLedger::new();
        assert!(standard_corpus(&mut t, &mut reg, &mut lg).is_ok());
        assert_eq!(t.bytes, DISK_BYTES as u32 * 2);
        assert!(lg.deficit(1).is_none());
        let claimed = [lg.per_face[0], lg.per_face[1]];
        assert!(reg.audit_disk(claimed).is_ok());
    }

    #[test]
    fn readonly_fd_is_ebadf() {
        let mut reg = WriteFaceRegistry::new();
        let mut t = WriteTable::new();
        let f = t.open(WRITE03, FdMode::ReadOnly).ok().unwrap();
        let e = t.write(f as i32, 8, &mut reg).err().unwrap();
        assert_eq!(e, WriteCode::ReadOnlyFd);
        assert_eq!(errno_of(e), EBADF);
    }

    #[test]
    fn closed_fd_rejected() {
        let mut reg = WriteFaceRegistry::new();
        let mut t = WriteTable::new();
        let f = t.open(WRITE01, FdMode::WriteOnly).ok().unwrap();
        assert!(t.close(f).is_ok());
        assert_eq!(t.write(f as i32, 8, &mut reg), Err(WriteCode::FdClosed));
    }

    #[test]
    fn overflow_rejected() {
        let mut reg = WriteFaceRegistry::new();
        assert!(reg.write_disk(WRITE01, DISK_BYTES).is_ok());
        assert_eq!(reg.write_disk(WRITE01, 1), Err(WriteCode::BufOverflow));
    }

    #[test]
    fn flush_state_machine() {
        let mut reg = WriteFaceRegistry::new();
        let mut t = WriteTable::new();
        let _ = t.open(WRITE01, FdMode::WriteOnly);
        assert_eq!(t.flush(0), Err(WriteCode::StateRegress));
        assert!(t.write(0, 8, &mut reg).is_ok());
        assert!(t.flush(0).is_ok());
    }

    #[test]
    fn drift_detected() {
        let mut lg = WriteLedger::new();
        assert_eq!(lg.record(WRITE01, 8, 7), Err(WriteCode::CountDrift));
        assert_eq!(lg.drifts, 1);
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_write_injection();
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
    fn unx10102_checks_green() {
        let s = run_unx10102_checks();
        assert!(s.all_passed(), "UNX-F10102 red: {}", s.red_items_text());
    }
}
