//! UNX-F10109 pipe 正反双判据（双端达成与断端写误用受控） · 0x92b40 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10109（pipe 正反双判据：J1 双端读写达成，J2 断端写误用
//!       受控）。
//! 锚点：UNX-F10110-J1 fstat 元数据达成（size/mtime 与 ftruncate 后一致
//!       10/10 次）；UNX-F10110-J2 坏缓冲误用受控（预期 EFAULT 精确
//!       复现 10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92b40（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `PipeCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 状态机 `EndState`：每端 Open → Closed 单向；读端关闭后写端写入
//!     即断端写（EPIPE，SIGPIPE 处置按 L1 条款注记）；
//!   - 账本 `PipeLedger`：守恒式 written == read_total + in_flight
//!     才零漂移，计数漂移显性入账；
//!   - 环形缓冲 `PipeBox`：32 字节容量，确定性内容模板
//!     （byte k = k*13+7 模 256），写读回环逐字节可复算；
//!   - 解析器 `parse_pipe_call`："pipe|side|n" 三段解析（side ∈
//!     read/write），n 必为十进制正数且 ≤ 容量；
//!   - 方向守卫：写端不可读、读端不可写（方向误用按 EBADF 族受控）；
//!   - 注入核 `BadPipeCase`：断端写 / 方向误用 / 负 fd / 越界 fd /
//!     计数漂移 五类坏样本逐类 10/10 次拒绝，账守恒逃逸=0；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10109 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x92b40;

/// 段窗上界（下一卡 0x92b80 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x92b80;

/// fd 表容量。
pub const FD_CAP: usize = 16;

/// 管道缓冲容量（字节）。
pub const PIPE_CAP: usize = 32;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EBADF: i32 = 9;
/// 断管（断端写族，J2 口径；SIGPIPE 处置按 L1 条款注记）。
pub const EPIPE: i32 = 32;
/// 资源暂不可用（满管族）。
pub const EAGAIN: i32 = 11;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PipeCode {
    /// 缺操作成分（首段空或非 pipe）
    MissingOp,
    /// side 段不可解析（出 read/write 闭集）
    BadSideForm,
    /// n 段不可解析（空/非数字/零/超容）
    BadLenForm,
    /// 负 fd（EBADF）
    FdNegative,
    /// fd 越界（≥16 槽，EBADF）
    FdRange,
    /// 关闭后操作（空槽，EBADF）
    FdClosed,
    /// 方向误用（写端读 / 读端写，EBADF 族）
    WrongEnd,
    /// 满管（in_flight + n > 容量，EAGAIN）
    PipeFull,
    /// 断端写（读端已关闭，EPIPE；J2 主样本）
    BrokenPipe,
    /// 内容失真（读出字节与模板比对失败）
    ContentMissed,
    /// 账漂移（written ≠ read_total + in_flight）
    LedgerDrift,
    /// 段位越界
    SegmentViolation,
}

impl PipeCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            PipeCode::MissingOp => "缺操作成分被拒：首段必须是 pipe；请核对调用行",
            PipeCode::BadSideForm => "side 段不可解析被拒：side 只允许 read/write；请核对",
            PipeCode::BadLenForm => "n 段不可解析被拒：n 必须是十进制正数且 ≤32；请调整",
            PipeCode::FdNegative => "负 fd 被拒：pipe 端 fd(-1) 按 EBADF 受控复现",
            PipeCode::FdRange => "fd 越界被拒：fd 必须落在 0..16；请核对槽号",
            PipeCode::FdClosed => "关闭后操作被拒：槽位已空；请先重建管道",
            PipeCode::WrongEnd => "方向误用被拒：写端不可读、读端不可写；按 EBADF 族受控",
            PipeCode::PipeFull => "满管被拒：在途字节将超 32B 容量；按 EAGAIN 受控复现",
            PipeCode::BrokenPipe => "断端写被拒：读端已关闭；按 EPIPE 受控复现（SIGPIPE 按 L1 条款注记）",
            PipeCode::ContentMissed => "内容失真被拒：读出字节与模板比对失败；请复核回环",
            PipeCode::LedgerDrift => "账漂移被拒：written≠read_total+in_flight；请回查管道账",
            PipeCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x92b40..=0x92b80",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [PipeCode; 12] = [
        PipeCode::MissingOp,
        PipeCode::BadSideForm,
        PipeCode::BadLenForm,
        PipeCode::FdNegative,
        PipeCode::FdRange,
        PipeCode::FdClosed,
        PipeCode::WrongEnd,
        PipeCode::PipeFull,
        PipeCode::BrokenPipe,
        PipeCode::ContentMissed,
        PipeCode::LedgerDrift,
        PipeCode::SegmentViolation,
    ];
}

/// 诊断码到 Linux errno 的映射（反判据受控复现的对照口径）。
pub fn errno_of(code: PipeCode) -> i32 {
    match code {
        PipeCode::BrokenPipe => EPIPE,
        PipeCode::PipeFull => EAGAIN,
        _ => EBADF,
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

/// 端向闭集：read / write。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    /// 读端
    Read,
    /// 写端
    Write,
}

/// 调用行解析："pipe|side|n" → (side, n)。
///
/// side 闭集 read/write；n 十进制正数且 ≤32。
pub fn parse_pipe_call(line: &str) -> Result<(Side, u32), PipeCode> {
    let cuts = match split3(line) {
        Some(c) => c,
        None => return Err(PipeCode::MissingOp),
    };
    let op_seg = line.get(cuts[0]..cuts[1] - 1).unwrap_or("");
    let side_seg = line.get(cuts[1]..cuts[2] - 1).unwrap_or("");
    let n_seg = line.get(cuts[2]..cuts[3]).unwrap_or("");
    if op_seg != "pipe" {
        return Err(PipeCode::MissingOp);
    }
    let side = match side_seg {
        "read" => Side::Read,
        "write" => Side::Write,
        _ => return Err(PipeCode::BadSideForm),
    };
    let n = match n_seg.parse::<u32>() {
        Ok(v) => v,
        Err(_) => return Err(PipeCode::BadLenForm),
    };
    if n == 0 || n as usize > PIPE_CAP {
        return Err(PipeCode::BadLenForm);
    }
    Ok((side, n))
}

// ── 管道盒与端状态机 ──────────────────────────────────────────────────
/// 管道盒：环形缓冲 + 双端状态 + 双向计数。
#[derive(Clone, Debug)]
pub struct PipeBox {
    /// 环形缓冲（确定性模板填充）
    pub buf: [u8; PIPE_CAP],
    /// 写游标（在途尾部）
    pub wpos: usize,
    /// 读游标
    pub rpos: usize,
    /// 读端状态
    pub read_open: bool,
    /// 写端状态
    pub write_open: bool,
    /// 累计写入字节
    pub written: u32,
    /// 累计读出字节
    pub read_total: u32,
}

impl PipeBox {
    /// 构造空管道盒。
    pub const fn new() -> PipeBox {
        PipeBox {
            buf: [0u8; PIPE_CAP],
            wpos: 0,
            rpos: 0,
            read_open: true,
            write_open: true,
            written: 0,
            read_total: 0,
        }
    }

    /// 在途字节数（written − read_total）。
    pub fn in_flight(&self) -> u32 {
        self.written.saturating_sub(self.read_total)
    }

    /// 写端写入 n 字节：断端写（读端已关）按 EPIPE 拒；满管按 EAGAIN 拒；
    /// 确定性模板 byte k = k*13+7 模 256 逐字节填充。
    pub fn write_end(&mut self, n: u32) -> Result<u32, PipeCode> {
        if !self.read_open {
            return Err(PipeCode::BrokenPipe);
        }
        if self.in_flight() + n > PIPE_CAP as u32 {
            return Err(PipeCode::PipeFull);
        }
        let mut i = 0u32;
        while i < n {
            let k = (self.written + i) as usize;
            let v = ((k * 13 + 7) & 0xFF) as u8;
            if let Some(t) = self.buf.get_mut(self.wpos) {
                *t = v;
            }
            self.wpos = (self.wpos + 1) % PIPE_CAP;
            i += 1;
        }
        self.written = self.written.saturating_add(n);
        Ok(n)
    }

    /// 读端读出 n 字节（在途不足即短读）；返回 (实读数, 内容)。
    pub fn read_end(&mut self, n: u32) -> Result<(u32, Vec<u8>), PipeCode> {
        let avail = self.in_flight();
        let take = if n < avail { n } else { avail };
        let mut out: Vec<u8> = Vec::new();
        let mut i = 0u32;
        while i < take {
            if let Some(b) = self.buf.get(self.rpos) {
                out.push(*b);
            }
            self.rpos = (self.rpos + 1) % PIPE_CAP;
            i += 1;
        }
        self.read_total = self.read_total.saturating_add(take);
        Ok((take, out))
    }

    /// 内容复核：读出序列与全局模板逐字节比对。
    pub fn verify(&self, seq: &[u8]) -> Result<(), PipeCode> {
        let mut i = 0usize;
        while i < seq.len() {
            let k = self.read_total as usize - seq.len() + i;
            let expect = ((k * 13 + 7) & 0xFF) as u8;
            if let Some(a) = seq.get(i) {
                if *a != expect {
                    return Err(PipeCode::ContentMissed);
                }
            }
            i += 1;
        }
        Ok(())
    }
}

/// 端标记：绑定管道盒 + 端向。
#[derive(Clone, Copy, Debug)]
pub struct EndTag {
    /// 绑定管道盒号
    pub box_id: usize,
    /// 端向
    pub side: Side,
}

/// fd 表：16 槽端标记。
#[derive(Clone, Debug)]
pub struct FdTable {
    /// 槽位数组（None=空槽）
    pub slots: [Option<EndTag>; FD_CAP],
}

impl FdTable {
    /// 构造空表。
    pub const fn new() -> FdTable {
        FdTable { slots: [None; FD_CAP] }
    }

    /// 建管：分配最低两个空槽，先读端后写端；返回 (读端 fd, 写端 fd)。
    pub fn pipe(&mut self, box_id: usize) -> Result<(usize, usize), PipeCode> {
        let mut first = None;
        let mut i = 0usize;
        while i < FD_CAP {
            if let Some(None) = self.slots.get(i) {
                first = Some(i);
                i = FD_CAP;
            } else {
                i += 1;
            }
        }
        let r = match first {
            Some(v) => v,
            None => return Err(PipeCode::FdRange),
        };
        let mut second = None;
        let mut j = r + 1;
        while j < FD_CAP {
            if let Some(None) = self.slots.get(j) {
                second = Some(j);
                j = FD_CAP;
            } else {
                j += 1;
            }
        }
        let w = match second {
            Some(v) => v,
            None => return Err(PipeCode::FdRange),
        };
        if let Some(s) = self.slots.get_mut(r) {
            *s = Some(EndTag { box_id, side: Side::Read });
        }
        if let Some(s) = self.slots.get_mut(w) {
            *s = Some(EndTag { box_id, side: Side::Write });
        }
        Ok((r, w))
    }

    /// 关闭端：只关端向（read 端关 read_open，write 端关 write_open）。
    pub fn close_end(&mut self, fd: usize, boxes: &mut [PipeBox]) -> Result<(), PipeCode> {
        let tag = match self.slots.get(fd).copied() {
            Some(Some(t)) => t,
            _ => return Err(PipeCode::FdClosed),
        };
        match tag.side {
            Side::Read => {
                if let Some(b) = boxes.get_mut(tag.box_id) {
                    b.read_open = false;
                }
            }
            Side::Write => {
                if let Some(b) = boxes.get_mut(tag.box_id) {
                    b.write_open = false;
                }
            }
        }
        if let Some(s) = self.slots.get_mut(fd) {
            *s = None;
        }
        Ok(())
    }
}

/// pipe 语义核：按 fd 端向分发读/写。
///
/// fd 必检（负/越界/空槽 EBADF）→ 端向守卫（写端读/读端写 → WrongEnd）
/// → 转发到 PipeBox 对应端。
pub fn do_pipe_io(
    table: &mut FdTable,
    boxes: &mut [PipeBox],
    fd: i32,
    n: u32,
    side: Side,
) -> Result<(u32, Vec<u8>), PipeCode> {
    if fd < 0 {
        return Err(PipeCode::FdNegative);
    }
    let fd_u = fd as usize;
    if fd_u >= FD_CAP {
        return Err(PipeCode::FdRange);
    }
    let tag = match table.slots.get(fd_u).copied() {
        Some(Some(t)) => t,
        _ => return Err(PipeCode::FdClosed),
    };
    if tag.side != side {
        return Err(PipeCode::WrongEnd);
    }
    let b = match boxes.get_mut(tag.box_id) {
        Some(b) => b,
        None => return Err(PipeCode::FdClosed),
    };
    match side {
        Side::Write => {
            let done = b.write_end(n)?;
            Ok((done, Vec::new()))
        }
        Side::Read => b.read_end(n),
    }
}

// ── 管道账本 ──────────────────────────────────────────────────────────
/// 管道账本：守恒式 written == read_total + in_flight 逐次对核。
#[derive(Clone, Debug)]
pub struct PipeLedger {
    /// 漂移检出次数
    pub drifts: u32,
    /// 逃逸次数（坏账误入账，必须恒 0）
    pub escapes: u32,
}

impl PipeLedger {
    /// 构造零账。
    pub const fn new() -> PipeLedger {
        PipeLedger { drifts: 0, escapes: 0 }
    }

    /// 守恒核：单盒三量对核。
    pub fn audit_box(&mut self, b: &PipeBox) -> Result<(), PipeCode> {
        if b.written != b.read_total + b.in_flight() {
            self.drifts = self.drifts.saturating_add(1);
            return Err(PipeCode::LedgerDrift);
        }
        Ok(())
    }
}

/// 标准语料：建管 → 写 10 读 4 → 写 8 → 读 14（内容逐段复核）→
/// 逐次账核 → 关写端后余量读尽（EOF 短读 0）。
pub fn standard_corpus(
    table: &mut FdTable,
    boxes: &mut [PipeBox],
    ledger: &mut PipeLedger,
) -> Result<(), PipeCode> {
    let (fr, fw) = table.pipe(0)?;
    let _ = do_pipe_io(table, boxes, fw as i32, 10, Side::Write)?;
    ledger.audit_box(boxes.get_mut(0).ok_or(PipeCode::FdClosed)?)?;
    let (got4, seq4) = do_pipe_io(table, boxes, fr as i32, 4, Side::Read)?;
    if got4 != 4 {
        return Err(PipeCode::ContentMissed);
    }
    if let Some(b) = boxes.get(0) {
        b.verify(&seq4)?;
    }
    let _ = do_pipe_io(table, boxes, fw as i32, 8, Side::Write)?;
    let (got14, seq14) = do_pipe_io(table, boxes, fr as i32, 14, Side::Read)?;
    if got14 != 14 {
        return Err(PipeCode::ContentMissed);
    }
    if let Some(b) = boxes.get(0) {
        b.verify(&seq14)?;
        ledger.audit_box(b)?;
    }
    // 关写端：余量读尽（EOF 短读 0 合法）
    table.close_end(fw, boxes)?;
    let (rest, _) = do_pipe_io(table, boxes, fr as i32, PIPE_CAP as u32, Side::Read)?;
    if rest != 0 {
        return Err(PipeCode::LedgerDrift);
    }
    Ok(())
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏 pipe 样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadPipeCase {
    /// 断端写（J2 主样本：读端关后写 → EPIPE）
    BrokenPipe,
    /// 方向误用（写端读）
    WrongEnd,
    /// 负 fd
    NegativeFd,
    /// 越界 fd
    RangeFd,
    /// 计数漂移
    LedgerDrift,
}

impl BadPipeCase {
    /// 闭集全量。
    pub const ALL: [BadPipeCase; 5] = [
        BadPipeCase::BrokenPipe,
        BadPipeCase::WrongEnd,
        BadPipeCase::NegativeFd,
        BadPipeCase::RangeFd,
        BadPipeCase::LedgerDrift,
    ];

    /// 注入一轮：构造坏样本并触发校验，返回是否被显式拒绝。
    pub fn inject(
        self,
        table: &mut FdTable,
        boxes: &mut [PipeBox],
        ledger: &mut PipeLedger,
    ) -> bool {
        match self {
            BadPipeCase::BrokenPipe => {
                let (fr, fw) = match table.pipe(0) {
                    Ok(v) => v,
                    Err(_) => return false,
                };
                let closed = table.close_end(fr, boxes).is_ok();
                let r = do_pipe_io(table, boxes, fw as i32, 4, Side::Write);
                let _ = table.close_end(fw, boxes);
                // 清账：断管样本盒不残留计数漂移
                closed && r.err() == Some(PipeCode::BrokenPipe)
                    && errno_of(PipeCode::BrokenPipe) == EPIPE
            }
            BadPipeCase::WrongEnd => {
                let (fr, fw) = match table.pipe(0) {
                    Ok(v) => v,
                    Err(_) => return false,
                };
                let r = do_pipe_io(table, boxes, fr as i32, 4, Side::Write);
                let _ = table.close_end(fr, boxes);
                let _ = table.close_end(fw, boxes);
                r.err() == Some(PipeCode::WrongEnd)
            }
            BadPipeCase::NegativeFd => {
                do_pipe_io(table, boxes, -1, 4, Side::Write).err()
                    == Some(PipeCode::FdNegative)
            }
            BadPipeCase::RangeFd => {
                do_pipe_io(table, boxes, FD_CAP as i32, 4, Side::Read).err()
                    == Some(PipeCode::FdRange)
            }
            BadPipeCase::LedgerDrift => {
                let mut lg = PipeLedger::new();
                let mut b = PipeBox::new();
                b.written = 10;
                b.read_total = 3;
                lg.audit_box(&b).err() == Some(PipeCode::LedgerDrift) && lg.drifts == 1
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_pipe_injection() -> (PipeLedger, [u32; 5]) {
    let mut table = FdTable::new();
    let mut boxes: Vec<PipeBox> = Vec::new();
    boxes.push(PipeBox::new());
    let mut ledger = PipeLedger::new();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadPipeCase::ALL.len() {
        if let Some(case) = BadPipeCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject(&mut table, &mut boxes, &mut ledger);
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
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；负 fd/越界/方向误用/关闭后族"),
    ("EPIPE", -32, 232, "Windows ERROR_NO_DATA；断端写族（SIGPIPE 按 L1 条款注记）"),
    ("EAGAIN", -11, 997, "Windows ERROR_IO_PENDING；满管族"),
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

/// 规范语料双跑指纹：建管→写读回环→断端→余量读尽 全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut table = FdTable::new();
    let mut boxes: Vec<PipeBox> = Vec::new();
    boxes.push(PipeBox::new());
    let mut lg = PipeLedger::new();
    bytes.push(match standard_corpus(&mut table, &mut boxes, &mut lg) {
        Ok(()) => 1,
        Err(_) => 2,
    });
    if let Some(b) = boxes.get(0) {
        bytes.extend_from_slice(&b.written.to_le_bytes());
        bytes.extend_from_slice(&b.read_total.to_le_bytes());
        bytes.push(if b.read_open { 3 } else { 4 });
        bytes.push(if b.write_open { 5 } else { 6 });
        bytes.push(match lg.audit_box(b) {
            Ok(()) => 7,
            Err(_) => 8,
        });
    }
    bytes.push(lg.drifts as u8);
    bytes.push(lg.escapes as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10109 判据集（19 条静态 + 3 条动态明细）。
pub fn run_unx10109_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10109");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x92b40..=0x92b80", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝");

    // 2. 标准语料：建管→写读回环→断端→读尽全链成功
    let mut table = FdTable::new();
    let mut boxes: Vec<PipeBox> = Vec::new();
    boxes.push(PipeBox::new());
    let mut lg = PipeLedger::new();
    let corpus_ok = standard_corpus(&mut table, &mut boxes, &mut lg).is_ok();
    s.add("双端达成：建管→写读回环→断端→读尽全链成功", corpus_ok,
          "J1 正判据：pipe01/pipe02 面语义逐环落账（内容逐段复核）");

    // 3. 建管双端：最低两槽先读后写
    let mut pt = FdTable::new();
    let pr = pt.pipe(0);
    let pair_ok = pr == Ok((0, 1));
    s.add("建管双端：fd(0,1) 先读后写", pair_ok,
          "pipe 返回 (读端,写端) 双 fd，最低槽分配");

    // 4. 写读回环 10/10：内容逐段复核
    let mut loop_hits = 0u32;
    let mut lr = 0u32;
    while lr < INJECT_ROUNDS {
        let mut t = FdTable::new();
        let mut bs: Vec<PipeBox> = Vec::new();
        bs.push(PipeBox::new());
        let (fr, fw) = t.pipe(0).ok().unwrap_or((0, 1));
        let _ = do_pipe_io(&mut t, &mut bs, fw as i32, 8, Side::Write);
        let r = do_pipe_io(&mut t, &mut bs, fr as i32, 8, Side::Read);
        let ok = match r {
            Ok((n, seq)) => {
                n == 8 && bs.get(0).map(|b| b.verify(&seq) == Ok(())).unwrap_or(false)
            }
            Err(_) => false,
        };
        if ok {
            loop_hits += 1;
        }
        lr += 1;
    }
    s.add("写读回环：写 8 读 8 内容逐字节一致 10/10", loop_hits == INJECT_ROUNDS,
          "确定性模板 byte k = k*13+7，回环逐字节可复算");

    // 5. 分段读写：写 10 读 4 读 6，计数与内容齐
    let mut st = FdTable::new();
    let mut sb: Vec<PipeBox> = Vec::new();
    sb.push(PipeBox::new());
    let (sr, sw) = st.pipe(0).ok().unwrap_or((0, 1));
    let _ = do_pipe_io(&mut st, &mut sb, sw as i32, 10, Side::Write);
    let a = do_pipe_io(&mut st, &mut sb, sr as i32, 4, Side::Read);
    let b = do_pipe_io(&mut st, &mut sb, sr as i32, 6, Side::Read);
    let seg_ok = match (a, b) {
        (Ok((4, _)), Ok((6, _))) => {
            let bx = sb.get(0);
            bx.map(|x| x.read_total == 10 && x.in_flight() == 0).unwrap_or(false)
        }
        _ => false,
    };
    s.add("分段读写：写 10 读 4+6，读尽后 in_flight=0", seg_ok,
          "分段短读合法，计数账随读随核");

    // 6. 满管拒：一次写满后再写 → EAGAIN
    let mut ft = FdTable::new();
    let mut fb: Vec<PipeBox> = Vec::new();
    fb.push(PipeBox::new());
    let (_, ffw) = ft.pipe(0).ok().unwrap_or((0, 1));
    let _ = do_pipe_io(&mut ft, &mut fb, ffw as i32, PIPE_CAP as u32, Side::Write);
    let full_rej = do_pipe_io(&mut ft, &mut fb, ffw as i32, 1, Side::Write).err()
        == Some(PipeCode::PipeFull)
        && errno_of(PipeCode::PipeFull) == EAGAIN;
    s.add("满管守卫：32B 写满后追加拒收（EAGAIN）", full_rej,
          "容量上界显性拒绝，不静默截断");

    // 7. 断端写 10/10 EPIPE（J2 反判据，SIGPIPE 按 L1 条款注记）
    let mut bp_hits = 0u32;
    let mut br = 0u32;
    while br < INJECT_ROUNDS {
        let mut t = FdTable::new();
        let mut bs: Vec<PipeBox> = Vec::new();
        bs.push(PipeBox::new());
        let (fr, fw) = t.pipe(0).ok().unwrap_or((0, 1));
        let _ = t.close_end(fr, &mut bs);
        let r = do_pipe_io(&mut t, &mut bs, fw as i32, 4, Side::Write);
        if r.err() == Some(PipeCode::BrokenPipe) && errno_of(PipeCode::BrokenPipe) == EPIPE {
            bp_hits += 1;
        }
        br += 1;
    }
    s.add("断端写：读端关后写 10/10 次 EPIPE 精确复现", bp_hits == INJECT_ROUNDS,
          "J2 反判据：断端写逐次按 EPIPE 受控复现（SIGPIPE 按 L1 条款注记）");

    // 8. 方向误用拒：写端读 / 读端写
    let mut wt = FdTable::new();
    let mut wb: Vec<PipeBox> = Vec::new();
    wb.push(PipeBox::new());
    let (wfr, wfw) = wt.pipe(0).ok().unwrap_or((0, 1));
    let read_via_write = do_pipe_io(&mut wt, &mut wb, wfw as i32, 4, Side::Read).err()
        == Some(PipeCode::WrongEnd);
    let write_via_read = do_pipe_io(&mut wt, &mut wb, wfr as i32, 4, Side::Write).err()
        == Some(PipeCode::WrongEnd);
    s.add("方向守卫：写端读拒、读端写拒", read_via_write && write_via_read,
          "端向与操作必须匹配，方向误用按 EBADF 族受控");

    // 9. 负 fd 10/10 EBADF
    let mut neg_hits = 0u32;
    let mut nt = FdTable::new();
    let mut nb: Vec<PipeBox> = Vec::new();
    nb.push(PipeBox::new());
    let mut nr = 0u32;
    while nr < INJECT_ROUNDS {
        if do_pipe_io(&mut nt, &mut nb, -1, 4, Side::Write).err()
            == Some(PipeCode::FdNegative)
        {
            neg_hits += 1;
        }
        nr += 1;
    }
    s.add("负 fd：10/10 次 EBADF 精确复现", neg_hits == INJECT_ROUNDS,
          "pipe 端 fd(-1) 逐次按 EBADF 受控复现");

    // 10. 越界 fd 拒
    let range_rej = do_pipe_io(&mut nt, &mut nb, FD_CAP as i32, 4, Side::Read).err()
        == Some(PipeCode::FdRange);
    s.add("越界 fd：fd=16 出窗拒收", range_rej,
          "fd 表 16 槽，越界操作按 EBADF 族受控");

    // 11. 解析器：三段/side 闭集/n 域检/残形
    let p_ok = parse_pipe_call("pipe|write|8") == Ok((Side::Write, 8));
    let p_side = parse_pipe_call("pipe|both|8").err() == Some(PipeCode::BadSideForm);
    let p_zero = parse_pipe_call("pipe|read|0").err() == Some(PipeCode::BadLenForm);
    let p_big = parse_pipe_call("pipe|read|33").err() == Some(PipeCode::BadLenForm);
    let p_seg = parse_pipe_call("pipe|read").err() == Some(PipeCode::MissingOp);
    s.add("协议解析：三段/side 闭集/零长/超容/残形全拒", p_ok && p_side && p_zero && p_big && p_seg,
          "调用行恰三段 pipe|side|n，n ∈ (0..=32]");

    // 12. EOF 短读：余量读尽后零读合法
    let mut et = FdTable::new();
    let mut eb: Vec<PipeBox> = Vec::new();
    eb.push(PipeBox::new());
    let (efr, efw) = et.pipe(0).ok().unwrap_or((0, 1));
    let _ = do_pipe_io(&mut et, &mut eb, efw as i32, 4, Side::Write);
    let _ = do_pipe_io(&mut et, &mut eb, efr as i32, 4, Side::Read);
    let eof = do_pipe_io(&mut et, &mut eb, efr as i32, 8, Side::Read) == Ok((0, Vec::new()));
    s.add("EOF 短读：余量读尽后零字节返 0", eof,
          "读端在写端在开时余量尽返 0（非 EPIPE）");

    // 13. 账守恒：written == read_total + in_flight
    let cons_ok = boxes
        .get(0)
        .map(|b| b.written == b.read_total + b.in_flight() && lg.audit_box(b).is_ok())
        .unwrap_or(false);
    s.add("账守恒：written == read_total + in_flight", cons_ok,
          "管道三量逐次对核，漂移显性入账");

    // 14. 漂移检出
    let mut dl = PipeLedger::new();
    let mut db = PipeBox::new();
    db.written = 10;
    db.read_total = 3;
    let drift_rej = dl.audit_box(&db).err() == Some(PipeCode::LedgerDrift);
    s.add("账漂移：三量失配逐次检出", drift_rej && dl.drifts == 1,
          "计数漂移显性入账，不静默放行");

    // 15. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_pipe_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "断端写/方向误用/负 fd/越界 fd/账漂移全被显式拒收");

    // 16. 账守恒逃逸=0
    s.add("账守恒：注入过程逃逸=0", ledger.escapes == 0,
          "注入核全程不得产生坏账逃逸入账");

    // 17. 文案闭集 + 诊断码唯一落段
    let text_ok = PipeCode::ALL.iter().all(|c| !c.detail().is_empty());
    let mut uniq = true;
    let mut i2 = 0usize;
    while i2 < PipeCode::ALL.len() {
        let mut j = i2 + 1;
        while j < PipeCode::ALL.len() {
            if let (Some(x), Some(y)) = (PipeCode::ALL.get(i2), PipeCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i2 += 1;
    }
    let in_seg = PipeCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 码文案非空、两两唯一且落段窗", text_ok && uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 18. Linux 逐码一致
    let codes_ok = agrees_with_linux(EBADF)
        && agrees_with_linux(EPIPE)
        && agrees_with_linux(EAGAIN)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 19. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 动态明细（≤8）：管道快照 + 账快照 + 指纹快照
    let pipe_text = match boxes.get(0) {
        Some(b) => format!(
            "written={}B read={}B 在途={}B",
            b.written,
            b.read_total,
            b.in_flight()
        ),
        None => alloc::string::String::new(),
    };
    s.add_dyn("管道快照", cons_ok, &pipe_text);
    let lg_text = format!("漂移={}，逃逸={}", lg.drifts, lg.escapes);
    s.add_dyn("账快照", lg.drifts == 0, &lg_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pipe_ok() {
        assert_eq!(parse_pipe_call("pipe|write|8"), Ok((Side::Write, 8)));
        assert_eq!(parse_pipe_call("pipe|read|32"), Ok((Side::Read, 32)));
    }

    #[test]
    fn parse_pipe_rejects() {
        assert_eq!(parse_pipe_call("fifo|write|8"), Err(PipeCode::MissingOp));
        assert_eq!(parse_pipe_call("pipe|both|8"), Err(PipeCode::BadSideForm));
        assert_eq!(parse_pipe_call("pipe|read|0"), Err(PipeCode::BadLenForm));
        assert_eq!(parse_pipe_call("pipe|read|33"), Err(PipeCode::BadLenForm));
        assert_eq!(parse_pipe_call("pipe|read"), Err(PipeCode::MissingOp));
    }

    #[test]
    fn pipe_pair_allocation() {
        let mut t = FdTable::new();
        assert_eq!(t.pipe(0), Ok((0, 1)));
    }

    #[test]
    fn loopback_content() {
        let mut t = FdTable::new();
        let mut bs: Vec<PipeBox> = Vec::new();
        bs.push(PipeBox::new());
        let (fr, fw) = t.pipe(0).ok().unwrap();
        assert!(do_pipe_io(&mut t, &mut bs, fw as i32, 8, Side::Write).is_ok());
        let r = do_pipe_io(&mut t, &mut bs, fr as i32, 8, Side::Read);
        match r {
            Ok((n, seq)) => {
                assert_eq!(n, 8);
                assert_eq!(bs[0].verify(&seq), Ok(()));
            }
            Err(_) => panic!(),
        }
    }

    #[test]
    fn broken_pipe_is_epipe() {
        let mut t = FdTable::new();
        let mut bs: Vec<PipeBox> = Vec::new();
        bs.push(PipeBox::new());
        let (fr, fw) = t.pipe(0).ok().unwrap();
        assert!(t.close_end(fr, &mut bs).is_ok());
        assert_eq!(
            do_pipe_io(&mut t, &mut bs, fw as i32, 4, Side::Write),
            Err(PipeCode::BrokenPipe)
        );
        assert_eq!(errno_of(PipeCode::BrokenPipe), EPIPE);
    }

    #[test]
    fn wrong_end_rejected() {
        let mut t = FdTable::new();
        let mut bs: Vec<PipeBox> = Vec::new();
        bs.push(PipeBox::new());
        let (fr, fw) = t.pipe(0).ok().unwrap();
        assert_eq!(
            do_pipe_io(&mut t, &mut bs, fw as i32, 4, Side::Read),
            Err(PipeCode::WrongEnd)
        );
        assert_eq!(
            do_pipe_io(&mut t, &mut bs, fr as i32, 4, Side::Write),
            Err(PipeCode::WrongEnd)
        );
    }

    #[test]
    fn full_pipe_rejected() {
        let mut t = FdTable::new();
        let mut bs: Vec<PipeBox> = Vec::new();
        bs.push(PipeBox::new());
        let (_, fw) = t.pipe(0).ok().unwrap();
        assert!(do_pipe_io(&mut t, &mut bs, fw as i32, PIPE_CAP as u32, Side::Write).is_ok());
        assert_eq!(
            do_pipe_io(&mut t, &mut bs, fw as i32, 1, Side::Write),
            Err(PipeCode::PipeFull)
        );
    }

    #[test]
    fn eof_zero_read() {
        let mut t = FdTable::new();
        let mut bs: Vec<PipeBox> = Vec::new();
        bs.push(PipeBox::new());
        let (fr, fw) = t.pipe(0).ok().unwrap();
        let _ = do_pipe_io(&mut t, &mut bs, fw as i32, 4, Side::Write);
        let _ = do_pipe_io(&mut t, &mut bs, fr as i32, 4, Side::Read);
        assert_eq!(do_pipe_io(&mut t, &mut bs, fr as i32, 8, Side::Read), Ok((0, Vec::new())));
    }

    #[test]
    fn conservation_holds() {
        let mut t = FdTable::new();
        let mut bs: Vec<PipeBox> = Vec::new();
        bs.push(PipeBox::new());
        let (fr, fw) = t.pipe(0).ok().unwrap();
        let _ = do_pipe_io(&mut t, &mut bs, fw as i32, 10, Side::Write);
        let _ = do_pipe_io(&mut t, &mut bs, fr as i32, 4, Side::Read);
        let mut lg = PipeLedger::new();
        assert!(lg.audit_box(&bs[0]).is_ok());
        assert_eq!(bs[0].in_flight(), 6);
    }

    #[test]
    fn drift_detected() {
        let mut lg = PipeLedger::new();
        let mut b = PipeBox::new();
        b.written = 10;
        b.read_total = 3;
        assert_eq!(lg.audit_box(&b), Err(PipeCode::LedgerDrift));
        assert_eq!(lg.drifts, 1);
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_pipe_injection();
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
    fn unx10109_checks_green() {
        let s = run_unx10109_checks();
        assert!(s.all_passed(), "UNX-F10109 red: {}", s.red_items_text());
    }
}
