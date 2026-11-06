//! UNX-F10103 open 正反双判据（打开达成与缺失路径误用受控） · 0x929c0 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10103（open 系统调用正反双判据：J1 打开达成，J2 缺失路径
//!       误用受控）。
//! 锚点：UNX-F10104-J1 close 关闭释放达成（fd 表槽位回收守恒 10/10 次）；
//!       UNX-F10104-J2 重关闭误用受控（预期 EBADF 精确复现 10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x929c0（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `OpenCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 解析器 `parse_flag_string`：标志串 "O_RDONLY|O_CREAT|O_TRUNC"
//!     逐段解析为（accmode, flags）位图，未知位/accmode 双位显性拒绝，
//!     同串两次解析逐位一致（可复算）；
//!   - 注册表 `PathTable`：8 条路径闭集（existing/missing 两类），
//!     越界路径号拒绝、重复登记拒绝、exists 翻转留痕；
//!   - 状态机 `FdSlot`：Free → Open 单向 + 关闭回收（回收守恒由账本核）；
//!   - 账本 `OpenLedger`：opens/closes/in_use 三账，守恒式
//!     in_use == opens − closes 才零漂移，漂移显性入账；
//!   - open 语义核 `open_path`：accmode 三态（O_RDONLY/O_WRONLY/O_RDWR）、
//!     O_CREAT 缺路径创建、O_EXCL 冲突 EEXIST、O_TRUNC 截断、
//!     O_APPEND 偏移置尾；
//!   - 注入核 `BadOpenCase`：缺路径无 CREATE / O_EXCL 冲突 / accmode
//!     双位 / 路径越界 / 账漂移 五类坏样本逐类 10/10 次拒绝，
//!     账守恒逃逸=0；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10103 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x929c0;

/// 段窗上界（下一卡 0x92a00 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x92a00;

/// 路径表容量（本卡模拟路径闭集上限）。
pub const PATH_CAP: usize = 8;

/// fd 表容量（本卡模拟 fd 上限）。
pub const FD_CAP: usize = 16;

/// O_RDONLY 读态（accmode 位图 0）。
pub const O_RDONLY: u32 = 0;
/// O_WRONLY 写态（accmode 位图 1）。
pub const O_WRONLY: u32 = 1;
/// O_RDWR 读写态（accmode 位图 2）。
pub const O_RDWR: u32 = 2;
/// O_ACCMODE 掩码（低 2 位）。
pub const O_ACCMODE: u32 = 3;
/// O_CREAT 不存在则创建（0o100）。
pub const O_CREAT: u32 = 0o100;
/// O_EXCL 必须不存在（0o200）。
pub const O_EXCL: u32 = 0o200;
/// O_TRUNC 打开即截断（0o1000）。
pub const O_TRUNC: u32 = 0o1000;
/// O_APPEND 追加态（0o2000）。
pub const O_APPEND: u32 = 0o2000;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const ENOENT: i32 = 2;
/// 已存在（O_EXCL 冲突族）。
pub const EEXIST: i32 = 17;
/// 非法参数（accmode/越界族）。
pub const EINVAL: i32 = 22;
/// fd 耗尽（对照 EMFILE）。
pub const EMFILE: i32 = 24;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpenCode {
    /// accmode 双位同置（读/写态互斥）
    BadAccmode,
    /// 未知标志位（出闭集位图）
    IllegalFlag,
    /// 缺失路径且无 O_CREAT
    MissingPath,
    /// O_EXCL 冲突（路径已存在）
    ExclConflict,
    /// 路径号越界（出 0..8 窗）
    PathRange,
    /// fd 越界
    FdRange,
    /// fd 耗尽（16 槽占满）
    FdExhausted,
    /// 账漂移（opens/closes/in_use 守恒式破坏）
    LedgerDrift,
    /// 重复登记同路径号
    Reopen,
    /// 状态倒退（Closed 后复用未回收槽）
    StateRegress,
    /// 截断未生效（O_TRUNC 后 size 未归零）
    TruncMissed,
    /// 段位越界
    SegmentViolation,
}

impl OpenCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            OpenCode::BadAccmode => "accmode 双位被拒：读/写态必须恰取其一；请改用 O_RDONLY/O_WRONLY/O_RDWR",
            OpenCode::IllegalFlag => "未知标志位被拒：标志串只允许 O_ 闭集位；请核对位图",
            OpenCode::MissingPath => "缺失路径被拒：路径不存在且无 O_CREAT；按 ENOENT 受控复现",
            OpenCode::ExclConflict => "O_EXCL 冲突被拒：路径已存在；按 EEXIST 受控复现",
            OpenCode::PathRange => "路径号越界被拒：只允许 0..8；请核对路径槽号",
            OpenCode::FdRange => "fd 越界被拒：fd 必须落在 0..16；请核对槽号",
            OpenCode::FdExhausted => "fd 耗尽被拒：16 槽占满；请先关闭再开（EMFILE 族）",
            OpenCode::LedgerDrift => "账漂移被拒：in_use≠opens−closes；请回查开关账面",
            OpenCode::Reopen => "重复登记被拒：同路径号已登记；请先核查再改",
            OpenCode::StateRegress => "状态倒退被拒：Free→Open 单向；请排查回收链",
            OpenCode::TruncMissed => "截断未生效被拒：O_TRUNC 后 size 未归零；请复核截断链",
            OpenCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x929c0..=0x92a00",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [OpenCode; 12] = [
        OpenCode::BadAccmode,
        OpenCode::IllegalFlag,
        OpenCode::MissingPath,
        OpenCode::ExclConflict,
        OpenCode::PathRange,
        OpenCode::FdRange,
        OpenCode::FdExhausted,
        OpenCode::LedgerDrift,
        OpenCode::Reopen,
        OpenCode::StateRegress,
        OpenCode::TruncMissed,
        OpenCode::SegmentViolation,
    ];
}

/// 诊断码到 Linux errno 的映射（反判据受控复现的对照口径）。
pub fn errno_of(code: OpenCode) -> i32 {
    match code {
        OpenCode::MissingPath => ENOENT,
        OpenCode::ExclConflict => EEXIST,
        OpenCode::BadAccmode | OpenCode::IllegalFlag | OpenCode::PathRange => EINVAL,
        OpenCode::FdExhausted => EMFILE,
        _ => EINVAL,
    }
}

// ── 标志串解析器 ──────────────────────────────────────────────────────
/// 标志位闭集表：(名字, 位值)。accmode 三态位单独处理。
pub const FLAG_TABLE: &[(&str, u32)] = &[
    ("O_RDONLY", O_RDONLY),
    ("O_WRONLY", O_WRONLY),
    ("O_RDWR", O_RDWR),
    ("O_CREAT", O_CREAT),
    ("O_EXCL", O_EXCL),
    ("O_TRUNC", O_TRUNC),
    ("O_APPEND", O_APPEND),
];

/// 标志串解析："O_RDONLY|O_CREAT|O_TRUNC" → (accmode, flags)。
///
/// 逐段查闭集表；accmode 位（低 2 位）至多一个；出现未知名即拒；
/// 空串即拒。同串两次调用逐位一致（纯函数可复算）。
pub fn parse_flag_string(line: &str) -> Result<(u32, u32), OpenCode> {
    let mut accmode: Option<u32> = None;
    let mut flags: u32 = 0;
    let b = line.as_bytes();
    if b.is_empty() {
        return Err(OpenCode::IllegalFlag);
    }
    let mut seg_start = 0usize;
    let mut i = 0usize;
    loop {
        let is_end = i >= b.len();
        if is_end || b.get(i) == Some(&b'|') {
            let seg = line.get(seg_start..i).unwrap_or("");
            let mut hit = false;
            let mut ti = 0usize;
            while ti < FLAG_TABLE.len() {
                if let Some((name, bit)) = FLAG_TABLE.get(ti) {
                    if *name == seg {
                        hit = true;
                        if *bit <= O_ACCMODE {
                            if let Some(prev) = accmode {
                                if prev != *bit {
                                    return Err(OpenCode::BadAccmode);
                                }
                            }
                            accmode = Some(*bit);
                        } else {
                            flags |= *bit;
                        }
                        ti = FLAG_TABLE.len();
                    }
                }
                ti += 1;
            }
            if !hit {
                return Err(OpenCode::IllegalFlag);
            }
            seg_start = i + 1;
        }
        if is_end {
            break;
        }
        i += 1;
    }
    let acc = match accmode {
        Some(a) => a,
        None => O_RDONLY,
    };
    Ok((acc, flags))
}

// ── 路径注册表 ────────────────────────────────────────────────────────
/// 一条路径槽：存在性 + 模拟 size + 打开计数。
#[derive(Clone, Copy, Debug)]
pub struct PathEntry {
    /// 是否存在（O_CREAT 可翻转）
    pub exists: bool,
    /// 模拟文件 size（O_TRUNC 归零）
    pub size: u32,
    /// 累计打开次数
    pub opens: u32,
}

/// 路径注册表：8 槽闭集（标准语料 5 存在 + 3 缺失）。
#[derive(Clone, Debug)]
pub struct PathTable {
    /// 路径槽（None=未登记）
    pub entries: [Option<PathEntry>; PATH_CAP],
}

impl PathTable {
    /// 构造空表。
    pub const fn new() -> PathTable {
        PathTable { entries: [None; PATH_CAP] }
    }

    /// 登记一条路径：越界拒、重复拒。
    pub fn register(&mut self, slot: usize, exists: bool, size: u32) -> Result<(), OpenCode> {
        if slot >= PATH_CAP {
            return Err(OpenCode::PathRange);
        }
        if let Some(Some(_)) = self.entries.get(slot) {
            return Err(OpenCode::Reopen);
        }
        if let Some(e) = self.entries.get_mut(slot) {
            *e = Some(PathEntry { exists, size, opens: 0 });
        }
        Ok(())
    }

    /// 标准语料：槽 0..5 存在（size 64），槽 5..8 缺失（size 0）。
    pub fn fill_standard(&mut self) -> Result<(), OpenCode> {
        let mut i = 0usize;
        while i < PATH_CAP {
            self.register(i, i < 5, if i < 5 { 64 } else { 0 })?;
            i += 1;
        }
        Ok(())
    }
}

// ── fd 表与开关状态机 ────────────────────────────────────────────────
/// fd 槽状态：Free → Open（关闭即回 Free，可复用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FdSlot {
    /// 空槽（可分配）
    Free,
    /// 已打开（绑定路径槽与 accmode）
    Open(u32, u32),
}

/// fd 表：16 槽，最低空槽优先分配。
#[derive(Clone, Debug)]
pub struct FdTable {
    /// 槽位数组
    pub slots: [FdSlot; FD_CAP],
}

impl FdTable {
    /// 构造空表（全 Free）。
    pub const fn new() -> FdTable {
        FdTable { slots: [FdSlot::Free; FD_CAP] }
    }

    /// 分配最低空槽，绑定 (路径槽, accmode)。
    pub fn alloc(&mut self, path: usize, accmode: u32) -> Result<usize, OpenCode> {
        let mut i = 0usize;
        while i < FD_CAP {
            if let Some(FdSlot::Free) = self.slots.get(i) {
                if let Some(s) = self.slots.get_mut(i) {
                    *s = FdSlot::Open(path as u32, accmode);
                }
                return Ok(i);
            }
            i += 1;
        }
        Err(OpenCode::FdExhausted)
    }

    /// 关闭回收：越界/未打开拒绝；回收后槽变 Free 可复用。
    pub fn release(&mut self, fd: usize) -> Result<(), OpenCode> {
        match self.slots.get(fd) {
            Some(FdSlot::Open(_, _)) => {
                if let Some(s) = self.slots.get_mut(fd) {
                    *s = FdSlot::Free;
                }
                Ok(())
            }
            _ => Err(OpenCode::FdRange),
        }
    }

    /// 在用计数（供守恒核）。
    pub fn in_use(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < FD_CAP {
            if let Some(FdSlot::Open(_, _)) = self.slots.get(i) {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// open 语义核：按 (路径槽, (accmode, flags)) 打开并落语义效应。
///
/// accmode 必检 → 路径必检（缺且无 O_CREAT 按 ENOENT 拒）→ O_EXCL
/// 冲突按 EEXIST 拒 → 分配 fd → O_TRUNC 归零 size → O_APPEND 偏移
/// 置 size。返回 (fd, 打开后偏移)。
pub fn open_path(
    table: &mut FdTable,
    paths: &mut PathTable,
    path_slot: usize,
    flags: (u32, u32),
) -> Result<(usize, u32), OpenCode> {
    let (accmode, flag_bits) = flags;
    // accmode 合法域恰为 {O_RDONLY, O_WRONLY, O_RDWR}；双位同置（3）即拒
    if accmode > O_RDWR {
        return Err(OpenCode::BadAccmode);
    }
    if path_slot >= PATH_CAP {
        return Err(OpenCode::PathRange);
    }
    let entry = match paths.entries.get(path_slot).copied() {
        Some(Some(e)) => e,
        _ => return Err(OpenCode::MissingPath),
    };
    if !entry.exists {
        if flag_bits & O_CREAT == 0 {
            return Err(OpenCode::MissingPath);
        }
        if let Some(Some(e)) = paths.entries.get_mut(path_slot) {
            e.exists = true;
            e.size = 0;
        }
    } else if flag_bits & O_EXCL != 0 {
        return Err(OpenCode::ExclConflict);
    }
    let fd = table.alloc(path_slot, accmode)?;
    if flag_bits & O_TRUNC != 0 {
        if let Some(Some(e)) = paths.entries.get_mut(path_slot) {
            e.size = 0;
        }
    }
    let offset = if flag_bits & O_APPEND != 0 {
        match paths.entries.get(path_slot).and_then(|e| e.as_ref()) {
            Some(e) => e.size,
            None => 0,
        }
    } else {
        0
    };
    if let Some(Some(e)) = paths.entries.get_mut(path_slot) {
        e.opens = e.opens.saturating_add(1);
    }
    Ok((fd, offset))
}

// ── 开关账本 ──────────────────────────────────────────────────────────
/// 开关账本：opens/closes/in_use 三账守恒（in_use == opens − closes）。
#[derive(Clone, Debug)]
pub struct OpenLedger {
    /// 累计成功打开次数
    pub opens: u32,
    /// 累计成功关闭次数
    pub closes: u32,
    /// 账面在用数（与表实数比对）
    pub claimed_in_use: u32,
    /// 漂移检出次数
    pub drifts: u32,
    /// 逃逸次数（坏账误入账，必须恒 0）
    pub escapes: u32,
}

impl OpenLedger {
    /// 构造零账。
    pub const fn new() -> OpenLedger {
        OpenLedger { opens: 0, closes: 0, claimed_in_use: 0, drifts: 0, escapes: 0 }
    }

    /// 记一次成功打开。
    pub fn record_open(&mut self) {
        self.opens = self.opens.saturating_add(1);
        self.claimed_in_use = self.claimed_in_use.saturating_add(1);
    }

    /// 记一次成功关闭。
    pub fn record_close(&mut self) {
        self.closes = self.closes.saturating_add(1);
        if self.claimed_in_use > 0 {
            self.claimed_in_use -= 1;
        }
    }

    /// 守恒核：账面三账与表实数逐项比对，不符即漂移入账。
    pub fn audit(&mut self, table: &FdTable) -> Result<(), OpenCode> {
        let actual = table.in_use();
        let derived = self.opens.saturating_sub(self.closes);
        if self.claimed_in_use != actual || self.claimed_in_use != derived {
            self.drifts = self.drifts.saturating_add(1);
            return Err(OpenCode::LedgerDrift);
        }
        Ok(())
    }
}

/// 标准语料：accmode 三态各开一次（existing 路径）+ O_CREAT 创建缺失
/// 路径 + O_TRUNC 截断 + O_APPEND 偏移置尾，逐次入账并审计。
pub fn standard_corpus(
    table: &mut FdTable,
    paths: &mut PathTable,
    ledger: &mut OpenLedger,
) -> Result<(), OpenCode> {
    let r0 = open_path(table, paths, 0, (O_RDONLY, 0))?;
    ledger.record_open();
    let r1 = open_path(table, paths, 1, (O_WRONLY, 0))?;
    ledger.record_open();
    let r2 = open_path(table, paths, 2, (O_RDWR, 0))?;
    ledger.record_open();
    let r3 = open_path(table, paths, 5, (O_WRONLY, O_CREAT))?;
    ledger.record_open();
    let r4 = open_path(table, paths, 3, (O_WRONLY, O_TRUNC))?;
    ledger.record_open();
    let r5 = open_path(table, paths, 4, (O_WRONLY, O_APPEND))?;
    ledger.record_open();
    // O_TRUNC 后 size 归零核
    let t_size = match paths.entries.get(3).and_then(|e| e.as_ref()) {
        Some(e) => e.size,
        None => return Err(OpenCode::TruncMissed),
    };
    if t_size != 0 {
        return Err(OpenCode::TruncMissed);
    }
    // O_APPEND 偏移 = size 核（size 64 → 偏移 64）
    if r5.1 != 64 {
        return Err(OpenCode::StateRegress);
    }
    // 三态位图回读核
    let expect = [O_RDONLY, O_WRONLY, O_RDWR];
    let mut i = 0usize;
    while i < expect.len() {
        if let Some(FdSlot::Open(_, acc)) = table.slots.get(i) {
            if let Some(e) = expect.get(i) {
                if acc != e {
                    return Err(OpenCode::BadAccmode);
                }
            }
        } else {
            return Err(OpenCode::StateRegress);
        }
        i += 1;
    }
    let _ = (r0.1, r1.1, r2.1, r3.1, r4.1);
    Ok(())
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏 open 样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadOpenCase {
    /// 缺失路径且无 O_CREAT（J2 主样本，ENOENT）
    MissingPath,
    /// O_EXCL 冲突（已存在路径，EEXIST）
    ExclConflict,
    /// accmode 双位同置（EINVAL）
    BadAccmode,
    /// 路径号越界（EINVAL）
    PathRange,
    /// 账漂移（守恒式破坏被检出）
    LedgerDrift,
}

impl BadOpenCase {
    /// 闭集全量。
    pub const ALL: [BadOpenCase; 5] = [
        BadOpenCase::MissingPath,
        BadOpenCase::ExclConflict,
        BadOpenCase::BadAccmode,
        BadOpenCase::PathRange,
        BadOpenCase::LedgerDrift,
    ];

    /// 注入一轮：构造坏样本并触发校验，返回是否被显式拒绝。
    pub fn inject(
        self,
        table: &mut FdTable,
        paths: &mut PathTable,
        ledger: &mut OpenLedger,
    ) -> bool {
        match self {
            BadOpenCase::MissingPath => {
                open_path(table, paths, 6, (O_RDONLY, 0)).err() == Some(OpenCode::MissingPath)
                    && errno_of(OpenCode::MissingPath) == ENOENT
            }
            BadOpenCase::ExclConflict => {
                open_path(table, paths, 0, (O_RDONLY, O_EXCL)).err()
                    == Some(OpenCode::ExclConflict)
            }
            BadOpenCase::BadAccmode => {
                open_path(table, paths, 0, (O_ACCMODE, 0)).err() == Some(OpenCode::BadAccmode)
            }
            BadOpenCase::PathRange => {
                open_path(table, paths, PATH_CAP, (O_RDONLY, 0)).err()
                    == Some(OpenCode::PathRange)
            }
            BadOpenCase::LedgerDrift => {
                let mut lg = OpenLedger::new();
                lg.claimed_in_use = 7;
                lg.opens = 1;
                lg.closes = 0;
                lg.audit(table).err() == Some(OpenCode::LedgerDrift) && lg.drifts == 1
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_open_injection() -> (OpenLedger, [u32; 5]) {
    let mut paths = PathTable::new();
    let _ = paths.fill_standard();
    let mut table = FdTable::new();
    let mut ledger = OpenLedger::new();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadOpenCase::ALL.len() {
        if let Some(case) = BadOpenCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject(&mut table, &mut paths, &mut ledger);
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
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺失路径且无 O_CREAT 族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；O_EXCL 冲突族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；accmode 双位/越界族"),
    ("EMFILE", -24, 4, "Windows ERROR_TOO_MANY_OPEN_FILES；fd 耗尽族"),
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

/// 规范语料双跑指纹：登记→三态开→创建→截断→追加→账核 全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut paths = PathTable::new();
    bytes.push(match paths.fill_standard() {
        Ok(()) => 1,
        Err(_) => 2,
    });
    let mut table = FdTable::new();
    let mut lg = OpenLedger::new();
    bytes.push(match standard_corpus(&mut table, &mut paths, &mut lg) {
        Ok(()) => 3,
        Err(_) => 4,
    });
    bytes.extend_from_slice(&lg.opens.to_le_bytes());
    bytes.push(match lg.audit(&table) {
        Ok(()) => 5,
        Err(_) => 6,
    });
    let p0 = match paths.entries.get(5).and_then(|e| e.as_ref()) {
        Some(e) => e.exists as u8,
        None => 7,
    };
    bytes.push(p0);
    bytes.push(lg.drifts as u8);
    bytes.push(lg.escapes as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10103 判据集（19 条静态 + 3 条动态明细）。
pub fn run_unx10103_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10103");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x929c0..=0x92a00", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝");

    // 2. 标准语料：三态开 + 创建 + 截断 + 追加全链成功
    let mut paths = PathTable::new();
    let _ = paths.fill_standard();
    let mut table = FdTable::new();
    let mut lg = OpenLedger::new();
    let corpus_ok = standard_corpus(&mut table, &mut paths, &mut lg).is_ok();
    s.add("打开达成：三态开+O_CREAT+O_TRUNC+O_APPEND 全链成功", corpus_ok,
          "J1 正判据：标志组合语义逐面落账，open01/open02 口径");

    // 3. 标志串解析可复算：同串两次逐位一致
    let p1 = parse_flag_string("O_RDONLY|O_CREAT|O_TRUNC");
    let p2 = parse_flag_string("O_RDONLY|O_CREAT|O_TRUNC");
    s.add("标志解析可复算：同串两次逐位一致", p1.is_ok() && p1 == p2,
          "纯函数无隐藏状态，解析逐位可复算");

    // 4. accmode 三态语义 10/10：三态各开成功且位图回读一致
    let mut acc_hits = 0u32;
    let accs = [O_RDONLY, O_WRONLY, O_RDWR];
    let mut ai = 0usize;
    while ai < accs.len() {
        if let Some(a) = accs.get(ai) {
            let mut t = FdTable::new();
            let mut ps = PathTable::new();
            let _ = ps.fill_standard();
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                if let Ok((fd, _)) = open_path(&mut t, &mut ps, 0, (*a, 0)) {
                    if let Some(FdSlot::Open(_, pm)) = t.slots.get(fd) {
                        if pm == a {
                            acc_hits += 1;
                        }
                    }
                }
                round += 1;
            }
        }
        ai += 1;
    }
    s.add("accmode 三态：读/写/读写各 10/10 位图回读一致", acc_hits == INJECT_ROUNDS * 3,
          "打开后权限位回读与请求逐位一致，三态语义正确");

    // 5. 缺失路径 10/10 ENOENT（J2 反判据）
    let mut miss_hits = 0u32;
    let mut mp_t = FdTable::new();
    let mut mp_p = PathTable::new();
    let _ = mp_p.fill_standard();
    let mut mr = 0u32;
    while mr < INJECT_ROUNDS {
        if open_path(&mut mp_t, &mut mp_p, 6, (O_RDONLY, 0)).err() == Some(OpenCode::MissingPath) {
            miss_hits += 1;
        }
        mr += 1;
    }
    s.add("缺失路径：10/10 次 ENOENT 精确复现", miss_hits == INJECT_ROUNDS,
          "J2 反判据：无 O_CREAT 开缺失路径逐次按 ENOENT 受控复现");

    // 6. O_CREAT 创建：缺失路径带 O_CREAT 成功且 exists 翻转
    let mut cr_t = FdTable::new();
    let mut cr_p = PathTable::new();
    let _ = cr_p.fill_standard();
    let cr_ok = open_path(&mut cr_t, &mut cr_p, 6, (O_WRONLY, O_CREAT)).is_ok()
        && cr_p.entries.get(6).and_then(|e| e.as_ref()).map(|e| e.exists) == Some(true);
    s.add("O_CREAT 创建：缺失路径建出且 exists 翻转", cr_ok,
          "O_CREAT|缺路径 → 创建成功，存在性翻转留痕");

    // 7. O_EXCL 冲突 10/10 EEXIST
    let mut ex_hits = 0u32;
    let mut ex_t = FdTable::new();
    let mut ex_p = PathTable::new();
    let _ = ex_p.fill_standard();
    let mut er = 0u32;
    while er < INJECT_ROUNDS {
        if open_path(&mut ex_t, &mut ex_p, 0, (O_RDONLY, O_EXCL)).err()
            == Some(OpenCode::ExclConflict)
        {
            ex_hits += 1;
        }
        er += 1;
    }
    s.add("O_EXCL 冲突：10/10 次 EEXIST 精确复现", ex_hits == INJECT_ROUNDS,
          "已存在路径带 O_EXCL 逐次按 EEXIST 受控复现");

    // 8. accmode 双位拒 + 未知标志位拒
    let dual_rej = parse_flag_string("O_RDONLY|O_RDWR").err() == Some(OpenCode::BadAccmode);
    let unknown_rej = parse_flag_string("O_RDONLY|O_MAGIC").err() == Some(OpenCode::IllegalFlag);
    s.add("标志校验：accmode 双位拒、未知位拒", dual_rej && unknown_rej,
          "读/写态互斥恰取其一；标志位出闭集即拒");

    // 9. O_TRUNC 截断语义：size 归零
    let mut tr_t = FdTable::new();
    let mut tr_p = PathTable::new();
    let _ = tr_p.fill_standard();
    let tr_ok = open_path(&mut tr_t, &mut tr_p, 3, (O_WRONLY, O_TRUNC)).is_ok()
        && tr_p.entries.get(3).and_then(|e| e.as_ref()).map(|e| e.size) == Some(0);
    s.add("O_TRUNC 截断：打开即 size 归零", tr_ok,
          "截断语义落账，TruncMissed 兜底检出");

    // 10. O_APPEND 偏移语义：offset = size
    let mut ap_t = FdTable::new();
    let mut ap_p = PathTable::new();
    let _ = ap_p.fill_standard();
    let ap = open_path(&mut ap_t, &mut ap_p, 4, (O_WRONLY, O_APPEND));
    let ap_ok = match ap {
        Ok((_, off)) => off == 64,
        Err(_) => false,
    };
    s.add("O_APPEND 偏移：offset 置为文件尾 size=64", ap_ok,
          "追加态打开偏移即文件尾，非零起点语义正确");

    // 11. 路径越界拒 + 重复登记拒
    let range_rej = open_path(&mut ap_t, &mut ap_p, PATH_CAP, (O_RDONLY, 0)).err()
        == Some(OpenCode::PathRange);
    let dup_rej = ap_p.register(0, true, 64).err() == Some(OpenCode::Reopen);
    s.add("路径守卫：越界拒、重复登记拒", range_rej && dup_rej,
          "路径槽 0..8 闭集；一槽一路径");

    // 12. fd 分配守恒：开 6 关 2 → in_use=4，最低空槽复用
    let mut cn_t = FdTable::new();
    let mut cn_p = PathTable::new();
    let _ = cn_p.fill_standard();
    let mut i0 = 0usize;
    while i0 < 6 {
        let _ = open_path(&mut cn_t, &mut cn_p, i0 % 5, (O_RDONLY, 0));
        i0 += 1;
    }
    let _ = cn_t.release(0);
    let _ = cn_t.release(1);
    // 复用探针会新开一槽，先记账面 in_use 再分配
    let before = cn_t.in_use();
    let reuse = match cn_t.alloc(0, 0) {
        Ok(fd) => fd == 0,
        Err(_) => false,
    };
    s.add("fd 守恒：开 6 关 2 后 in_use=4 且最低空槽复用",
          before == 4 && reuse,
          "分配回收守恒，回收槽按最低号复用（close 语义）");

    // 13. fd 耗尽拒（EMFILE 族）
    let mut ex_t2 = FdTable::new();
    let mut filled = 0u32;
    let mut i1 = 0usize;
    while i1 < FD_CAP {
        if ex_t2.alloc(0, 0).is_ok() {
            filled += 1;
        }
        i1 += 1;
    }
    let exhaust_rej = ex_t2.alloc(0, 0).err() == Some(OpenCode::FdExhausted);
    s.add("fd 耗尽：16 槽占满后拒绝（EMFILE 族）", filled == FD_CAP as u32 && exhaust_rej,
          "容量上界显性拒绝，不静默覆盖");

    // 14. 账守恒：三账审计零漂移
    let audit_ok = lg.audit(&table).is_ok()
        && lg.claimed_in_use == lg.opens - lg.closes
        && lg.claimed_in_use == table.in_use();
    s.add("账守恒：in_use == opens−closes == 表实数", audit_ok,
          "开关三账逐项比对，漂移即显性入账");

    // 15. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_open_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "缺路径/O_EXCL 冲突/accmode 双位/路径越界/账漂移全被显式拒收");

    // 16. 账守恒逃逸=0
    s.add("账守恒：注入过程逃逸=0", ledger.escapes == 0,
          "注入核全程不得产生坏账逃逸入账");

    // 17. 文案闭集 + 诊断码唯一落段
    let text_ok = OpenCode::ALL.iter().all(|c| !c.detail().is_empty());
    let mut uniq = true;
    let mut i2 = 0usize;
    while i2 < OpenCode::ALL.len() {
        let mut j = i2 + 1;
        while j < OpenCode::ALL.len() {
            if let (Some(x), Some(y)) = (OpenCode::ALL.get(i2), OpenCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i2 += 1;
    }
    let in_seg = OpenCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 码文案非空、两两唯一且落段窗", text_ok && uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 18. Linux 逐码一致
    let codes_ok = agrees_with_linux(ENOENT)
        && agrees_with_linux(EEXIST)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(EMFILE)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 19. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 动态明细（≤8）：fd 快照 + 路径快照 + 指纹快照
    let fd_text = format!("fd 在用 {}/16，累计开 {} 次", table.in_use(), lg.opens);
    s.add_dyn("fd 快照", table.in_use() <= FD_CAP as u32, &fd_text);
    let exist_n = paths.entries.iter().filter(|e| e.map(|x| x.exists) == Some(true)).count();
    let path_text = format!("路径存在 {}/8（O_CREAT 创建留痕）", exist_n);
    s.add_dyn("路径快照", exist_n >= 6, &path_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_flag_ok() {
        assert_eq!(parse_flag_string("O_RDONLY"), Ok((O_RDONLY, 0)));
        assert_eq!(parse_flag_string("O_WRONLY|O_CREAT"), Ok((O_WRONLY, O_CREAT)));
        assert_eq!(
            parse_flag_string("O_RDWR|O_CREAT|O_TRUNC|O_APPEND"),
            Ok((O_RDWR, O_CREAT | O_TRUNC | O_APPEND))
        );
    }

    #[test]
    fn parse_flag_rejects() {
        assert_eq!(parse_flag_string("O_RDONLY|O_RDWR"), Err(OpenCode::BadAccmode));
        assert_eq!(parse_flag_string("O_RDONLY|O_MAGIC"), Err(OpenCode::IllegalFlag));
        assert_eq!(parse_flag_string(""), Err(OpenCode::IllegalFlag));
    }

    #[test]
    fn missing_path_is_enoent() {
        let mut t = FdTable::new();
        let mut p = PathTable::new();
        let _ = p.fill_standard();
        assert_eq!(open_path(&mut t, &mut p, 6, (O_RDONLY, 0)), Err(OpenCode::MissingPath));
        assert!(open_path(&mut t, &mut p, 6, (O_WRONLY, O_CREAT)).is_ok());
    }

    #[test]
    fn excl_conflict_is_eexist() {
        let mut t = FdTable::new();
        let mut p = PathTable::new();
        let _ = p.fill_standard();
        assert_eq!(
            open_path(&mut t, &mut p, 0, (O_RDONLY, O_EXCL)),
            Err(OpenCode::ExclConflict)
        );
    }

    #[test]
    fn trunc_and_append_semantics() {
        let mut t = FdTable::new();
        let mut p = PathTable::new();
        let _ = p.fill_standard();
        let _ = open_path(&mut t, &mut p, 3, (O_WRONLY, O_TRUNC));
        assert_eq!(p.entries.get(3).and_then(|e| e.as_ref()).map(|e| e.size), Some(0));
        let r = open_path(&mut t, &mut p, 4, (O_WRONLY, O_APPEND));
        assert_eq!(r.ok().map(|(_, off)| off), Some(64));
    }

    #[test]
    fn fd_conservation_and_reuse() {
        let mut t = FdTable::new();
        let mut p = PathTable::new();
        let _ = p.fill_standard();
        let mut opened = 0u32;
        let mut i = 0usize;
        while i < 5 {
            if open_path(&mut t, &mut p, 0, (O_RDONLY, 0)).is_ok() {
                opened += 1;
            }
            i += 1;
        }
        assert_eq!(opened, 5);
        assert!(t.release(2).is_ok());
        assert_eq!(t.alloc(1, 0), Ok(2));
    }

    #[test]
    fn fd_exhaustion_rejected() {
        let mut t = FdTable::new();
        let mut i = 0usize;
        while i < FD_CAP {
            assert!(t.alloc(0, 0).is_ok());
            i += 1;
        }
        assert_eq!(t.alloc(0, 0), Err(OpenCode::FdExhausted));
    }

    #[test]
    fn ledger_audit_drift() {
        let mut t = FdTable::new();
        let mut lg = OpenLedger::new();
        assert!(lg.audit(&t).is_ok());
        lg.claimed_in_use = 5;
        assert_eq!(lg.audit(&t), Err(OpenCode::LedgerDrift));
        assert_eq!(lg.drifts, 1);
    }

    #[test]
    fn standard_corpus_green() {
        let mut t = FdTable::new();
        let mut p = PathTable::new();
        let _ = p.fill_standard();
        let mut lg = OpenLedger::new();
        assert!(standard_corpus(&mut t, &mut p, &mut lg).is_ok());
        assert!(lg.audit(&t).is_ok());
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_open_injection();
        assert_eq!(l.escapes, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(ENOENT));
        assert!(agrees_with_linux(EEXIST));
        assert!(agrees_with_linux(EMFILE));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EINVAL), -EINVAL);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10103_checks_green() {
        let s = run_unx10103_checks();
        assert!(s.all_passed(), "UNX-F10103 red: {}", s.red_items_text());
    }
}
