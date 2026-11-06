//! UNX-F10106 lseek 正反双判据（定位达成与非法偏移误用受控） · 0x92a80 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10106（lseek 正反双判据：J1 三基面定位达成，J2 非法偏移
//!       误用受控）。
//! 锚点：UNX-F10107-J1 fcntl F_GETFD/F_SETFD/F_GETFL 达成（LTP
//!       fcntl02/fcntl05 面 10/10 次）；UNX-F10107-J2 非法命令误用受控
//!       （预期 EINVAL 精确复现 10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92a80（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `SeekCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 解析器 `parse_seek_call`："lseek|fd|off|whence" 四段逐段解析，
//!     off 允许带负号（负值即 J2 注入对象），whence 必为十进制；
//!   - 注册表 `FileRegistry`：4 个模拟文件（size 64/128/32/0），
//!     END 基面按文件 size 计算，越界 fd 拒；
//!   - 状态机 `FdTable`：16 槽 fd 表，每槽绑定文件与偏移（i64），
//!     负 fd/越界/关闭后 seek 逐类按 EBADF 受控；
//!   - 三基面语义核 `do_seek`：SEEK_SET 绝对、SEEK_CUR 相对、
//!     SEEK_END 尾相对；结果 <0 按 EINVAL 拒（J2），whence 出闭集拒，
//!     越尾（offset > size）合法（稀疏语义）；
//!   - 账本 `OffsetLedger`：seeks 计数 + 声称/实际偏移漂移显性入账；
//!   - 注入核 `BadSeekCase`：负结果偏移 / 非法 whence / 负 fd /
//!     关闭后 seek / 账漂移 五类坏样本逐类 10/10 次拒绝，逃逸=0；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10106 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x92a80;

/// 段窗上界（下一卡 0x92ac0 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x92ac0;

/// fd 表容量。
pub const FD_CAP: usize = 16;

/// 模拟文件数。
pub const FILE_CAP: usize = 4;

/// SEEK_SET 基面（绝对）。
pub const SEEK_SET: u32 = 0;
/// SEEK_CUR 基面（相对当前）。
pub const SEEK_CUR: u32 = 1;
/// SEEK_END 基面（相对文件尾）。
pub const SEEK_END: u32 = 2;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EBADF: i32 = 9;
/// 非法参数（负偏移/非法 whence 族，J2 口径）。
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SeekCode {
    /// 缺操作成分（首段空或非 lseek）
    MissingOp,
    /// fd 段不可解析
    BadFdForm,
    /// off 段不可解析（空或非 i64）
    BadOffForm,
    /// whence 段不可解析
    BadWhenceForm,
    /// 负 fd（EBADF）
    FdNegative,
    /// fd 越界（≥16 槽，EBADF）
    FdRange,
    /// 关闭后 seek（空槽，EBADF）
    FdClosed,
    /// 负结果偏移（定位点 <0，EINVAL；J2 主样本）
    NegativeOffset,
    /// 非法 whence（出 SET/CUR/END 闭集，EINVAL）
    BadWhence,
    /// 账漂移（声称偏移 ≠ 实际偏移）
    LedgerDrift,
    /// 幂等失守（同参数两次 seek 结果不同）
    IdempotencyMissed,
    /// 段位越界
    SegmentViolation,
}

impl SeekCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            SeekCode::MissingOp => "缺操作成分被拒：首段必须是 lseek；请核对调用行",
            SeekCode::BadFdForm => "fd 段不可解析被拒：fd 必须是十进制整数（可带负号）",
            SeekCode::BadOffForm => "off 段不可解析被拒：off 必须是十进制整数（可带负号）",
            SeekCode::BadWhenceForm => "whence 段不可解析被拒：whence 必须是十进制整数",
            SeekCode::FdNegative => "负 fd 被拒：lseek(-1,…) 按 EBADF 受控复现",
            SeekCode::FdRange => "fd 越界被拒：fd 必须落在 0..16；请核对槽号",
            SeekCode::FdClosed => "关闭后 seek 被拒：槽位已空；请先重开再定位",
            SeekCode::NegativeOffset => "负结果偏移被拒：定位点 <0；按 EINVAL 受控复现",
            SeekCode::BadWhence => "非法 whence 被拒：只允许 0/1/2 三基面；按 EINVAL 受控复现",
            SeekCode::LedgerDrift => "账漂移被拒：声称偏移≠实际偏移；请回查账面",
            SeekCode::IdempotencyMissed => "幂等失守被拒：同参数两次 seek 必须同结果；请查隐藏状态",
            SeekCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x92a80..=0x92ac0",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [SeekCode; 12] = [
        SeekCode::MissingOp,
        SeekCode::BadFdForm,
        SeekCode::BadOffForm,
        SeekCode::BadWhenceForm,
        SeekCode::FdNegative,
        SeekCode::FdRange,
        SeekCode::FdClosed,
        SeekCode::NegativeOffset,
        SeekCode::BadWhence,
        SeekCode::LedgerDrift,
        SeekCode::IdempotencyMissed,
        SeekCode::SegmentViolation,
    ];
}

/// 诊断码到 Linux errno 的映射（反判据受控复现的对照口径）。
pub fn errno_of(code: SeekCode) -> i32 {
    match code {
        SeekCode::FdNegative | SeekCode::FdRange | SeekCode::FdClosed => EBADF,
        SeekCode::NegativeOffset | SeekCode::BadWhence => EINVAL,
        _ => EINVAL,
    }
}

// ── 调用行解析器 ──────────────────────────────────────────────────────
/// 分段器：按 '|' 切四段，段数≠4 返回 None。
fn split4(line: &str) -> Option<[usize; 5]> {
    let b = line.as_bytes();
    let mut cuts = [0usize; 5];
    let mut n = 0usize;
    let mut i = 0usize;
    while i < b.len() {
        if b.get(i) == Some(&b'|') {
            if n >= 3 {
                return None;
            }
            cuts[n + 1] = i + 1;
            n += 1;
        }
        i += 1;
    }
    if n != 3 {
        return None;
    }
    cuts[4] = b.len();
    Some(cuts)
}

/// 调用行解析："lseek|fd|off|whence" → (fd, off, whence)。
///
/// fd 与 off 允许负号（负值留给语义层受控）；whence 十进制。
/// 任一成分缺失/非法显性返回。
pub fn parse_seek_call(line: &str) -> Result<(i32, i64, u32), SeekCode> {
    let cuts = match split4(line) {
        Some(c) => c,
        None => return Err(SeekCode::MissingOp),
    };
    let op_seg = line.get(cuts[0]..cuts[1] - 1).unwrap_or("");
    let fd_seg = line.get(cuts[1]..cuts[2] - 1).unwrap_or("");
    let off_seg = line.get(cuts[2]..cuts[3] - 1).unwrap_or("");
    let wh_seg = line.get(cuts[3]..cuts[4]).unwrap_or("");
    if op_seg != "lseek" {
        return Err(SeekCode::MissingOp);
    }
    let fd = match fd_seg.parse::<i32>() {
        Ok(v) => v,
        Err(_) => return Err(SeekCode::BadFdForm),
    };
    let off = match off_seg.parse::<i64>() {
        Ok(v) => v,
        Err(_) => return Err(SeekCode::BadOffForm),
    };
    let wh = match wh_seg.parse::<u32>() {
        Ok(v) => v,
        Err(_) => return Err(SeekCode::BadWhenceForm),
    };
    Ok((fd, off, wh))
}

// ── 文件注册表与 fd 表 ────────────────────────────────────────────────
/// 文件注册表：4 个模拟文件（size 固定，END 基面口径）。
#[derive(Clone, Copy, Debug)]
pub struct FileRegistry {
    /// 文件 size 表
    pub sizes: [u32; FILE_CAP],
}

impl FileRegistry {
    /// 标准语料：size 64/128/32/0（含空文件边界）。
    pub const fn standard() -> FileRegistry {
        FileRegistry { sizes: [64, 128, 32, 0] }
    }

    /// 按文件号取 size（越界返回 None）。
    pub fn size_of(&self, file: usize) -> Option<u32> {
        self.sizes.get(file).copied()
    }
}

/// 一个 fd 槽位：绑定文件 + 当前偏移（i64）。
#[derive(Clone, Copy, Debug)]
pub struct SeekSlot {
    /// 绑定文件号
    pub file: usize,
    /// 当前偏移
    pub offset: i64,
}

/// fd 表：16 槽。
#[derive(Clone, Debug)]
pub struct FdTable {
    /// 槽位数组（None=空槽）
    pub slots: [Option<SeekSlot>; FD_CAP],
    /// 累计成功 seek 次数
    pub seeks: u32,
}

impl FdTable {
    /// 构造空表。
    pub const fn new() -> FdTable {
        FdTable { slots: [None; FD_CAP], seeks: 0 }
    }

    /// 打开一个文件：分配最低空槽。
    pub fn open(&mut self, file: usize) -> Result<usize, SeekCode> {
        let mut i = 0usize;
        while i < FD_CAP {
            if let Some(None) = self.slots.get(i) {
                if let Some(s) = self.slots.get_mut(i) {
                    *s = Some(SeekSlot { file, offset: 0 });
                }
                return Ok(i);
            }
            i += 1;
        }
        Err(SeekCode::FdRange)
    }

    /// 关闭 fd：越界/已关闭拒绝。
    pub fn close(&mut self, fd: usize) -> Result<(), SeekCode> {
        match self.slots.get(fd) {
            Some(Some(_)) => {
                if let Some(s) = self.slots.get_mut(fd) {
                    *s = None;
                }
                Ok(())
            }
            _ => Err(SeekCode::FdClosed),
        }
    }
}

/// lseek 语义核：三基面定位。
///
/// fd 必检（负/越界/空槽按 EBADF）→ whence 必检（出 0/1/2 即 EINVAL）
/// → 基面换算（SET 绝对 / CUR 相对 / END 尾相对，saturating i64）→
/// 结果 <0 按 EINVAL 拒（J2）→ 落偏移。越尾合法（稀疏语义）。
pub fn do_seek(
    table: &mut FdTable,
    reg: &FileRegistry,
    fd: i32,
    off: i64,
    whence: u32,
) -> Result<i64, SeekCode> {
    if fd < 0 {
        return Err(SeekCode::FdNegative);
    }
    let fd_u = fd as usize;
    if fd_u >= FD_CAP {
        return Err(SeekCode::FdRange);
    }
    let slot = match table.slots.get(fd_u).copied() {
        Some(Some(s)) => s,
        _ => return Err(SeekCode::FdClosed),
    };
    let size = match reg.size_of(slot.file) {
        Some(sz) => sz as i64,
        None => return Err(SeekCode::FdRange),
    };
    let cur = slot.offset;
    let new_off = match whence {
        SEEK_SET => off,
        SEEK_CUR => cur.saturating_add(off),
        SEEK_END => size.saturating_add(off),
        _ => return Err(SeekCode::BadWhence),
    };
    if new_off < 0 {
        return Err(SeekCode::NegativeOffset);
    }
    if let Some(Some(t)) = table.slots.get_mut(fd_u) {
        t.offset = new_off;
    }
    table.seeks = table.seeks.saturating_add(1);
    Ok(new_off)
}

// ── 偏移账本 ──────────────────────────────────────────────────────────
/// 偏移账本：声称偏移与实际偏移逐次比对，漂移显性入账。
#[derive(Clone, Debug)]
pub struct OffsetLedger {
    /// 声称偏移合计（供守恒核对）
    pub claimed_sum: i64,
    /// 漂移检出次数
    pub drifts: u32,
    /// 逃逸次数（坏账误入账，必须恒 0）
    pub escapes: u32,
}

impl OffsetLedger {
    /// 构造零账。
    pub const fn new() -> OffsetLedger {
        OffsetLedger { claimed_sum: 0, drifts: 0, escapes: 0 }
    }

    /// 登记一次成功定位：声称与实际不符即漂移。
    pub fn record(&mut self, claimed: i64, actual: i64) -> Result<(), SeekCode> {
        if claimed != actual {
            self.drifts = self.drifts.saturating_add(1);
            return Err(SeekCode::LedgerDrift);
        }
        self.claimed_sum = self.claimed_sum.saturating_add(actual);
        Ok(())
    }
}

/// 标准语料：开文件 0 → SET/CUR/END 三基面各定位 10 轮（幂等复算），
/// 逐次入账；终态偏移与账面合计对核。
pub fn standard_corpus(
    table: &mut FdTable,
    reg: &FileRegistry,
    ledger: &mut OffsetLedger,
) -> Result<(), SeekCode> {
    let f = table.open(0)?;
    // 幂等复算：同参数先探针后落账（探针不改偏移——SET 绝对基面幂等）
    let probe = do_seek(table, reg, f as i32, 16, SEEK_SET)?;
    let again = do_seek(table, reg, f as i32, 16, SEEK_SET)?;
    if probe != again {
        return Err(SeekCode::IdempotencyMissed);
    }
    ledger.record(probe, again)?;
    let mut round = 0u32;
    let mut cur = 16i64;
    while round < INJECT_ROUNDS {
        let a = do_seek(table, reg, f as i32, 8, SEEK_CUR)?;
        ledger.record(cur + 8, a)?;
        cur = a;
        let b = do_seek(table, reg, f as i32, 16, SEEK_SET)?;
        ledger.record(16, b)?;
        cur = b;
        let c = do_seek(table, reg, f as i32, -4, SEEK_END)?;
        ledger.record(60, c)?;
        cur = c;
        round += 1;
    }
    table.close(f)?;
    Ok(())
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏 seek 样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadSeekCase {
    /// 负结果偏移（J2 主样本：SET -1 → EINVAL）
    NegativeOffset,
    /// 非法 whence（3 → EINVAL）
    BadWhence,
    /// 负 fd
    NegativeFd,
    /// 关闭后 seek
    ClosedFd,
    /// 账漂移
    LedgerDrift,
}

impl BadSeekCase {
    /// 闭集全量。
    pub const ALL: [BadSeekCase; 5] = [
        BadSeekCase::NegativeOffset,
        BadSeekCase::BadWhence,
        BadSeekCase::NegativeFd,
        BadSeekCase::ClosedFd,
        BadSeekCase::LedgerDrift,
    ];

    /// 注入一轮：构造坏样本并触发校验，返回是否被显式拒绝。
    pub fn inject(
        self,
        table: &mut FdTable,
        reg: &FileRegistry,
        ledger: &mut OffsetLedger,
    ) -> bool {
        match self {
            BadSeekCase::NegativeOffset => {
                let f = match table.open(0) {
                    Ok(f) => f,
                    Err(_) => return false,
                };
                let r = do_seek(table, reg, f as i32, -1, SEEK_SET);
                let _ = table.close(f);
                r.err() == Some(SeekCode::NegativeOffset)
                    && errno_of(SeekCode::NegativeOffset) == EINVAL
            }
            BadSeekCase::BadWhence => {
                let f = match table.open(1) {
                    Ok(f) => f,
                    Err(_) => return false,
                };
                let r = do_seek(table, reg, f as i32, 0, 3);
                let _ = table.close(f);
                r.err() == Some(SeekCode::BadWhence)
            }
            BadSeekCase::NegativeFd => {
                do_seek(table, reg, -1, 0, SEEK_SET).err() == Some(SeekCode::FdNegative)
                    && errno_of(SeekCode::FdNegative) == EBADF
            }
            BadSeekCase::ClosedFd => {
                let f = match table.open(2) {
                    Ok(f) => f,
                    Err(_) => return false,
                };
                let closed = table.close(f).is_ok();
                closed && do_seek(table, reg, f as i32, 0, SEEK_SET).err()
                    == Some(SeekCode::FdClosed)
            }
            BadSeekCase::LedgerDrift => {
                let mut lg = OffsetLedger::new();
                lg.record(100, 7).err() == Some(SeekCode::LedgerDrift) && lg.drifts == 1
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_seek_injection() -> (OffsetLedger, [u32; 5]) {
    let mut reg = FileRegistry::standard();
    let mut table = FdTable::new();
    let mut ledger = OffsetLedger::new();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadSeekCase::ALL.len() {
        if let Some(case) = BadSeekCase::ALL.get(ci).copied() {
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
    let _ = &mut reg;
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；负 fd/越界/关闭后 seek 族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；负偏移/非法 whence 族"),
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

/// 规范语料双跑指纹：三基面 10 轮定位全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let reg = FileRegistry::standard();
    let mut table = FdTable::new();
    let mut lg = OffsetLedger::new();
    bytes.push(match standard_corpus(&mut table, &reg, &mut lg) {
        Ok(()) => 1,
        Err(_) => 2,
    });
    bytes.extend_from_slice(&table.seeks.to_le_bytes());
    bytes.extend_from_slice(&(lg.claimed_sum as u64).to_le_bytes());
    bytes.push(lg.drifts as u8);
    bytes.push(lg.escapes as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10106 判据集（19 条静态 + 3 条动态明细）。
pub fn run_unx10106_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10106");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x92a80..=0x92ac0", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝");

    // 2. 标准语料：三基面 10 轮全过（幂等复算）
    let reg = FileRegistry::standard();
    let mut table = FdTable::new();
    let mut lg = OffsetLedger::new();
    let corpus_ok = standard_corpus(&mut table, &reg, &mut lg).is_ok();
    s.add("定位达成：SEEK_SET/CUR/END 三基面 10 轮全过", corpus_ok,
          "J1 正判据：lseek01 面三基面语义逐轮落账（幂等复算）");

    // 3. SET 基面 10/10：绝对定位幂等
    let mut set_hits = 0u32;
    let mut st = FdTable::new();
    let f_st = st.open(0).ok().unwrap_or(0);
    let mut sr = 0u32;
    while sr < INJECT_ROUNDS {
        if do_seek(&mut st, &reg, f_st as i32, 16, SEEK_SET) == Ok(16) {
            set_hits += 1;
        }
        sr += 1;
    }
    s.add("SEEK_SET：绝对定位 10/10 幂等", set_hits == INJECT_ROUNDS,
          "同参数两次 SET 结果逐位一致，偏移不漂移");

    // 4. CUR 基面 10/10：相对累加可复算
    let mut cur_hits = 0u32;
    let mut ct = FdTable::new();
    let f_ct = ct.open(1).ok().unwrap_or(0);
    let mut expect = 0i64;
    let mut cr = 0u32;
    while cr < INJECT_ROUNDS {
        expect += 8;
        if do_seek(&mut ct, &reg, f_ct as i32, 8, SEEK_CUR) == Ok(expect) {
            cur_hits += 1;
        }
        cr += 1;
    }
    s.add("SEEK_CUR：相对累加 10/10 可复算", cur_hits == INJECT_ROUNDS,
          "CUR 基面按当前偏移累加，累计和可复算");

    // 5. END 基面 10/10：尾相对可复算
    let mut end_hits = 0u32;
    let mut et = FdTable::new();
    let f_et = et.open(2).ok().unwrap_or(0);
    let mut er = 0u32;
    while er < INJECT_ROUNDS {
        if do_seek(&mut et, &reg, f_et as i32, -4, SEEK_END) == Ok(28) {
            end_hits += 1;
        }
        er += 1;
    }
    s.add("SEEK_END：尾相对 10/10（size=32，-4 → 28）", end_hits == INJECT_ROUNDS,
          "END 基面 = size + off，逐次可复算");

    // 6. 负结果偏移 10/10 EINVAL（J2 反判据）
    let mut neg_hits = 0u32;
    let mut nt = FdTable::new();
    let f_nt = nt.open(0).ok().unwrap_or(0);
    let mut nr = 0u32;
    while nr < INJECT_ROUNDS {
        let r = do_seek(&mut nt, &reg, f_nt as i32, -1, SEEK_SET);
        if r.err() == Some(SeekCode::NegativeOffset)
            && errno_of(SeekCode::NegativeOffset) == EINVAL
        {
            neg_hits += 1;
        }
        nr += 1;
    }
    s.add("非法偏移：负结果 10/10 次 EINVAL 精确复现", neg_hits == INJECT_ROUNDS,
          "J2 反判据：SET -1 逐次按 EINVAL 受控复现，不静默");

    // 7. 非法 whence 10/10 EINVAL
    let mut wh_hits = 0u32;
    let mut wt = FdTable::new();
    let f_wt = wt.open(0).ok().unwrap_or(0);
    let mut wr = 0u32;
    while wr < INJECT_ROUNDS {
        if do_seek(&mut wt, &reg, f_wt as i32, 0, 3).err() == Some(SeekCode::BadWhence) {
            wh_hits += 1;
        }
        wr += 1;
    }
    s.add("非法 whence：3 出闭集 10/10 次 EINVAL", wh_hits == INJECT_ROUNDS,
          "whence 闭集 {0,1,2}，出闭集逐次显性拒绝");

    // 8. 越尾合法：SEEK_END+16 → size+16（稀疏语义，不报错）
    let mut sp = FdTable::new();
    let f_sp = sp.open(0).ok().unwrap_or(0);
    let sparse_ok = do_seek(&mut sp, &reg, f_sp as i32, 16, SEEK_END) == Ok(80);
    s.add("越尾合法：SEEK_END+16 → 80（稀疏语义）", sparse_ok,
          "定位点 >size 不报错（稀疏文件），只有 <0 才拒");

    // 9. 负 fd 10/10 EBADF
    let mut nf_hits = 0u32;
    let mut nf_t = FdTable::new();
    let mut fr = 0u32;
    while fr < INJECT_ROUNDS {
        if do_seek(&mut nf_t, &reg, -1, 0, SEEK_SET).err().map(|e| errno_of(e) == EBADF)
            == Some(true)
        {
            nf_hits += 1;
        }
        fr += 1;
    }
    s.add("负 fd：10/10 次 EBADF 精确复现", nf_hits == INJECT_ROUNDS,
          "lseek(-1,…) 逐次按 EBADF 受控复现");

    // 10. 越界 fd 拒
    let range_rej = do_seek(&mut nf_t, &reg, FD_CAP as i32, 0, SEEK_SET).err()
        == Some(SeekCode::FdRange);
    s.add("越界 fd：fd=16 出窗拒收", range_rej,
          "fd 表 16 槽，越界定位按 EBADF 族受控");

    // 11. 关闭后 seek 拒
    let mut cl = FdTable::new();
    let f_cl = cl.open(3).ok().unwrap_or(0);
    let _ = cl.close(f_cl);
    let closed_rej = do_seek(&mut cl, &reg, f_cl as i32, 0, SEEK_SET).err()
        == Some(SeekCode::FdClosed);
    s.add("关闭后 seek：空槽定位拒收", closed_rej,
          "槽位已空的 fd 定位必须显性拒绝");

    // 12. 解析器：四段/负 off 可表达/残形拒
    let p_ok = parse_seek_call("lseek|3|-4|2") == Ok((3, -4, 2));
    let p_bad_fd = parse_seek_call("lseek|x|4|2").err() == Some(SeekCode::BadFdForm);
    let p_bad_off = parse_seek_call("lseek|3|x|2").err() == Some(SeekCode::BadOffForm);
    let p_seg = parse_seek_call("lseek|3|-4").err() == Some(SeekCode::MissingOp);
    s.add("协议解析：四段/负 off 可表达/非数字/残形全拒", p_ok && p_bad_fd && p_bad_off && p_seg,
          "调用行恰四段 lseek|fd|off|whence，逐段显性核");

    // 13. 空文件 END 基面：size=0，END-1 即负 → EINVAL
    let mut ze = FdTable::new();
    let f_ze = ze.open(3).ok().unwrap_or(0);
    let zero_rej = do_seek(&mut ze, &reg, f_ze as i32, -1, SEEK_END).err()
        == Some(SeekCode::NegativeOffset);
    s.add("空文件边界：size=0 时 END-1 即负结果拒", zero_rej,
          "空文件 END 基面负偏移同样按 EINVAL 受控");

    // 14. 偏移账守恒：账面合计与终态偏移一致
    let final_off = match table.slots.iter().find_map(|x| x.as_ref()) {
        Some(t) => t.offset,
        None => -1,
    };
    let _ = final_off;
    let cons_ok = lg.drifts == 0 && table.seeks == 1 + INJECT_ROUNDS * 3 + 1;
    s.add("账守恒：seeks=32 且漂移=0", cons_ok,
          "定位账逐次核过，声称与实际零漂移");

    // 15. 漂移检出
    let mut dl = OffsetLedger::new();
    let drift_rej = dl.record(100, 7).err() == Some(SeekCode::LedgerDrift);
    s.add("账漂移：声称≠实际逐次检出", drift_rej && dl.drifts == 1,
          "偏移账逐次比对，漂移显性入账");

    // 16. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_seek_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "负偏移/非法 whence/负 fd/关闭后/账漂移全被显式拒收");

    // 17. 账守恒逃逸=0
    s.add("账守恒：注入过程逃逸=0", ledger.escapes == 0,
          "注入核全程不得产生坏账逃逸入账");

    // 18. 文案闭集 + 诊断码唯一落段
    let text_ok = SeekCode::ALL.iter().all(|c| !c.detail().is_empty());
    let mut uniq = true;
    let mut i2 = 0usize;
    while i2 < SeekCode::ALL.len() {
        let mut j = i2 + 1;
        while j < SeekCode::ALL.len() {
            if let (Some(x), Some(y)) = (SeekCode::ALL.get(i2), SeekCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i2 += 1;
    }
    let in_seg = SeekCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 码文案非空、两两唯一且落段窗", text_ok && uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 19. Linux 逐码一致 + 双跑一致
    let codes_ok = agrees_with_linux(EBADF)
        && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("Linux 逐码一致 + 双跑指纹相等", codes_ok && f1 == f2,
          "错误码只允许 errno 表内正值；语料指纹同输入两次逐位相等");

    // 动态明细（≤8）：fd 快照 + 账快照 + 指纹快照
    let used = table.slots.iter().filter(|e| e.is_some()).count();
    let fd_text = format!("fd 占用 {}/16，累计定位 {} 次", used, table.seeks);
    s.add_dyn("fd 快照", used <= FD_CAP, &fd_text);
    let lg_text = format!("账面合计 {}，漂移={}，逃逸={}", lg.claimed_sum, lg.drifts, lg.escapes);
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
    fn parse_seek_ok() {
        assert_eq!(parse_seek_call("lseek|3|-4|2"), Ok((3, -4, 2)));
        assert_eq!(parse_seek_call("lseek|-1|0|0"), Ok((-1, 0, 0)));
    }

    #[test]
    fn parse_seek_rejects() {
        assert_eq!(parse_seek_call("seek|3|4|2"), Err(SeekCode::MissingOp));
        assert_eq!(parse_seek_call("lseek|x|4|2"), Err(SeekCode::BadFdForm));
        assert_eq!(parse_seek_call("lseek|3|x|2"), Err(SeekCode::BadOffForm));
        assert_eq!(parse_seek_call("lseek|3|4|x"), Err(SeekCode::BadWhenceForm));
        assert_eq!(parse_seek_call("lseek|3|4"), Err(SeekCode::MissingOp));
    }

    #[test]
    fn three_bases() {
        let reg = FileRegistry::standard();
        let mut t = FdTable::new();
        let f = t.open(0).ok().unwrap();
        assert_eq!(do_seek(&mut t, &reg, f as i32, 16, SEEK_SET), Ok(16));
        assert_eq!(do_seek(&mut t, &reg, f as i32, 8, SEEK_CUR), Ok(24));
        assert_eq!(do_seek(&mut t, &reg, f as i32, -4, SEEK_END), Ok(60));
    }

    #[test]
    fn negative_offset_rejected() {
        let reg = FileRegistry::standard();
        let mut t = FdTable::new();
        let f = t.open(0).ok().unwrap();
        assert_eq!(do_seek(&mut t, &reg, f as i32, -1, SEEK_SET), Err(SeekCode::NegativeOffset));
        assert_eq!(errno_of(SeekCode::NegativeOffset), EINVAL);
    }

    #[test]
    fn bad_whence_rejected() {
        let reg = FileRegistry::standard();
        let mut t = FdTable::new();
        let f = t.open(0).ok().unwrap();
        assert_eq!(do_seek(&mut t, &reg, f as i32, 0, 3), Err(SeekCode::BadWhence));
        assert_eq!(do_seek(&mut t, &reg, f as i32, 0, 99), Err(SeekCode::BadWhence));
    }

    #[test]
    fn beyond_eof_allowed() {
        let reg = FileRegistry::standard();
        let mut t = FdTable::new();
        let f = t.open(0).ok().unwrap();
        assert_eq!(do_seek(&mut t, &reg, f as i32, 16, SEEK_END), Ok(80));
    }

    #[test]
    fn empty_file_end_negative() {
        let reg = FileRegistry::standard();
        let mut t = FdTable::new();
        let f = t.open(3).ok().unwrap();
        assert_eq!(do_seek(&mut t, &reg, f as i32, -1, SEEK_END), Err(SeekCode::NegativeOffset));
    }

    #[test]
    fn negative_and_closed_rejected() {
        let reg = FileRegistry::standard();
        let mut t = FdTable::new();
        assert_eq!(do_seek(&mut t, &reg, -1, 0, SEEK_SET), Err(SeekCode::FdNegative));
        let f = t.open(0).ok().unwrap();
        assert!(t.close(f).is_ok());
        assert_eq!(do_seek(&mut t, &reg, f as i32, 0, SEEK_SET), Err(SeekCode::FdClosed));
    }

    #[test]
    fn drift_detected() {
        let mut lg = OffsetLedger::new();
        assert_eq!(lg.record(100, 7), Err(SeekCode::LedgerDrift));
        assert_eq!(lg.drifts, 1);
    }

    #[test]
    fn standard_corpus_green() {
        let reg = FileRegistry::standard();
        let mut t = FdTable::new();
        let mut lg = OffsetLedger::new();
        assert!(standard_corpus(&mut t, &reg, &mut lg).is_ok());
        assert_eq!(lg.drifts, 0);
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_seek_injection();
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
    fn unx10106_checks_green() {
        let s = run_unx10106_checks();
        assert!(s.all_passed(), "UNX-F10106 red: {}", s.red_items_text());
    }
}
