//! UNX-F9278 权限族 E 型升级接管注记 · 0x7B300段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9278-J1 逐字）：
//!   接管边界声明判据过，存量错误路径零重写承诺格过，增量清单落账格过。
//!
//! 设计要点：
//!   ① 接管边界是「错误类型 × 系统调用」的二维声明：权限族 8 类错误各自
//!      只允许在登记的系统调用面上被接管（EACCES 不能在 CHOWN 面接管，
//!      EPERM 不能在 OPEN 面接管），越面即 CrossSyscall。
//!   ② A 账（存量错误路径持有）与 B 账（统一接管后持有）分立，划转必须
//!      原子整体搬移：部分划转会让同一错误双计，二次划转必须显式拒绝。
//!      守恒式 A+B = 8×12 = 96，任何凭空增删都要报 LedgerNotConserved。
//!   ③ 零重写承诺用「重放」证明而非口头承诺：8 条存量样本在接管前后各跑
//!      一遍，返回码必须与冻结重放账逐项相等；接管只许改归属，不许改
//!      判定结果与错误码。
//!   ④ 增量清单逐次登记（序号 + 类型 + 注记），序号从 1 连续递增、类型
//!      唯一、条数恰为 8；重复登记或序号断裂即 IncrementListViolation。
//!   ⑤ 阶段机 Declared→Registered→Transferred→Verified 单向；未登记就
//!      划转、未划转就核销都必须被拒绝。
//!   ⑥ 注入五类（未登记/跨面/部分划转/双计/承诺破坏）各 10/10 必抓，
//!      注入账守恒 50 = 50 + 0。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9278 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7B300;

/// 接管面登记的错误类型数。
pub const TYPES: usize = 8;

/// 每类错误的存量路径持有量（A 账初始值）。
pub const LEGACY_UNITS: u32 = 12;

/// 接管后总额守恒值：8 × 12 = 96。
pub const TOTAL_UNITS: u32 = (TYPES as u32) * LEGACY_UNITS;

/// 增量清单容量上限。
pub const INCR_MAX: usize = 16;

/// 单类注入的复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 注入类别数。
pub const INJECT_CLASSES: u32 = 5;

// ── 系统调用面（位掩码闭集） ──────────────────────────────────────────
/// open 面。
pub const SYS_OPEN: u32 = 1 << 0;
/// mkdir 面。
pub const SYS_MKDIR: u32 = 1 << 1;
/// chmod 面。
pub const SYS_CHMOD: u32 = 1 << 2;
/// chown 面。
pub const SYS_CHOWN: u32 = 1 << 3;
/// access 面。
pub const SYS_ACCESS: u32 = 1 << 4;
/// exec 面。
pub const SYS_EXEC: u32 = 1 << 5;

/// 全部系统调用面。
pub const SYS_ALL: u32 = SYS_OPEN | SYS_MKDIR | SYS_CHMOD | SYS_CHOWN | SYS_ACCESS | SYS_EXEC;

// ── 错误类型闭集 ──────────────────────────────────────────────────────
/// 权限族错误类型（8 类，全部在接管边界内）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PermErr {
    /// EACCES = 13
    Eacces = 0,
    /// EPERM = 1
    Eperm = 1,
    /// EBUSY = 16
    Ebusy = 2,
    /// EROFS = 30
    Erofs = 3,
    /// ETXTBSY = 26
    Etxtbsy = 4,
    /// EISDIR = 21
    Eisdir = 5,
    /// ENOTDIR = 20
    Enotdir = 6,
    /// ENOMEM = 12
    Enomem = 7,
}

impl PermErr {
    /// 表内序号。
    pub fn index(self) -> usize {
        self as usize
    }

    /// Linux errno 正码。
    pub fn errno(self) -> i32 {
        match self { PermErr::Eacces => 13, PermErr::Eperm => 1, PermErr::Ebusy => 16, PermErr::Erofs => 30, PermErr::Etxtbsy => 26, PermErr::Eisdir => 21, PermErr::Enotdir => 20, PermErr::Enomem => 12, }
    }

    /// 符号名。
    pub fn name(self) -> &'static str {
        match self { PermErr::Eacces => "EACCES", PermErr::Eperm => "EPERM", PermErr::Ebusy => "EBUSY", PermErr::Erofs => "EROFS", PermErr::Etxtbsy => "ETXTBSY", PermErr::Eisdir => "EISDIR", PermErr::Enotdir => "ENOTDIR", PermErr::Enomem => "ENOMEM", }
    }

    /// 闭集全量（顺序即序号）。
    pub const ALL: [PermErr; TYPES] = [PermErr::Eacces, PermErr::Eperm, PermErr::Ebusy, PermErr::Erofs, PermErr::Etxtbsy, PermErr::Eisdir, PermErr::Enotdir, PermErr::Enomem,];
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 接管注记诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TakeCode {
    /// 未登记类型就发起接管
    UnregisteredErrno,
    /// 类型序号越界
    OutOfBoundary,
    /// 跨系统调用面接管
    CrossSyscall,
    /// 部分划转
    PartialTransfer,
    /// 双计（重复登记）
    DoubleCount,
    /// 阶段机回退/跳步
    StateRegression,
    /// 增量清单违约
    IncrementListViolation,
    /// 零重写承诺破坏
    RewritePromiseBroken,
    /// 划转账不守恒
    LedgerNotConserved,
    /// 注入账倒退
    InjectLedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl TakeCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self { TakeCode::UnregisteredErrno => "未登记类型被拒：接管必须先登记再划转；\
                请补 REGISTER 阶段，禁止跳过登记直接改归属",
            TakeCode::OutOfBoundary => "类型序号越界：只允许闭集内 8 类；请按 \
                PermErr::ALL 的顺序取号，越界号不得进入接管账",
            TakeCode::CrossSyscall => "跨面接管被拒：该错误类型未在目标系统调用面\
                登记；请核对边界声明的掩码，越面接管会破坏错误归属",
            TakeCode::PartialTransfer => "部分划转被拒：A 账该类已空或只搬了一半；\
                接管必须一次性整体划转，否则同一错误双计",
            TakeCode::DoubleCount => "双计被拒：该类型已登记/已在 B 账；一条错误\
                任意时刻只允许有一处归属",
            TakeCode::StateRegression => "阶段回退或跳步：Declared→Registered→\
                Transferred→Verified 只许单向推进",
            TakeCode::IncrementListViolation => "增量清单违约：序号必须从 1 连续、\
                类型唯一、注记非空；请按登记顺序补账",
            TakeCode::RewritePromiseBroken => "零重写承诺破坏：接管前后同一样本的\
                返回码漂移；接管只改归属，不得改判定结果",
            TakeCode::LedgerNotConserved => "划转账不守恒：A+B 总额必须恒为 96；\
                请检查是否有凭空增删或重复搬运",
            TakeCode::InjectLedgerRegress => "注入账倒退：注入=检出+逃逸且只增不减；\
                请检查回滚路径误改累计值",
            TakeCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查\
                隐藏状态或非确定哈希",
            TakeCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量。
    pub const ALL: [TakeCode; 12] = [TakeCode::UnregisteredErrno, TakeCode::OutOfBoundary, TakeCode::CrossSyscall, TakeCode::PartialTransfer, TakeCode::DoubleCount, TakeCode::StateRegression, TakeCode::IncrementListViolation, TakeCode::RewritePromiseBroken, TakeCode::LedgerNotConserved, TakeCode::InjectLedgerRegress, TakeCode::NotReproducible, TakeCode::SegmentViolation,];
}

// ── 接管边界声明 ──────────────────────────────────────────────────────
/// 单条边界声明：错误类型 + 允许接管的系统调用面掩码 + 注记。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BoundaryRow {
    /// 错误类型
    pub err: PermErr,
    /// 允许面（位掩码，单比特声明）
    pub syscalls: u32,
    /// 面语义注记
    pub note: &'static str,
}

/// 权限族接管边界（8 行，覆盖六面）。
pub const BOUNDARY: [BoundaryRow; TYPES] = [BoundaryRow { err: PermErr::Eacces, syscalls: SYS_OPEN | SYS_MKDIR | SYS_ACCESS | SYS_EXEC, note: "对象权限不足：读/建/探/执行四面" },
    BoundaryRow { err: PermErr::Eperm, syscalls: SYS_CHMOD | SYS_CHOWN | SYS_EXEC,
        note: "操作特权缺失：改权限/改属主/特权执行三面" },
    BoundaryRow { err: PermErr::Ebusy, syscalls: SYS_OPEN | SYS_MKDIR,
        note: "忙态：打开与建目录两面" },
    BoundaryRow { err: PermErr::Erofs, syscalls: SYS_OPEN | SYS_MKDIR | SYS_CHMOD | SYS_CHOWN,
        note: "只读文件系统：写类四面" },
    BoundaryRow { err: PermErr::Etxtbsy, syscalls: SYS_EXEC | SYS_OPEN,
        note: "可执行体占用：执行与打开两面" },
    BoundaryRow { err: PermErr::Eisdir, syscalls: SYS_OPEN,
        note: "目录当文件打开：仅 open 面" },
    BoundaryRow { err: PermErr::Enotdir, syscalls: SYS_OPEN | SYS_ACCESS,
        note: "路径分量非目录：解析两面" },
    BoundaryRow { err: PermErr::Enomem, syscalls: SYS_OPEN | SYS_MKDIR | SYS_EXEC,
        note: "资源不足：需分配的三面" },
];

/// 每类错误的主接管面（单比特，跑批默认面）。
pub const PRIMARY_SYSCALL: [u32; TYPES] = [SYS_OPEN, SYS_CHOWN, SYS_OPEN, SYS_OPEN, SYS_EXEC, SYS_OPEN, SYS_OPEN, SYS_OPEN,];

/// 边界是否允许：单比特面 + 掩码命中。
pub fn boundary_allows(err: PermErr, syscall: u32) -> bool {
if syscall.count_ones() != 1 || syscall & !SYS_ALL != 0 { return false; }
    match BOUNDARY.get(err.index()) { Some(row) => row.syscalls & syscall != 0, None => false, }
}

/// 边界声明完整性：8 行、序号对齐、掩码非零且在全集内、注记非空。
pub fn boundary_complete() -> bool {
if BOUNDARY.len() != PermErr::ALL.len() { return false; }
    let mut i = 0usize;
while i < BOUNDARY.len() { if let Some(row) = BOUNDARY.get(i) { if row.err.index() != i || row.syscalls == 0 { return false; } if row.syscalls & !SYS_ALL != 0 || row.note.is_empty() { return false; } } i += 1; }
    true
}

/// 六面覆盖：所有系统调用位都被至少一条声明覆盖。
pub fn syscalls_covered() -> bool {
    let mut union = 0u32;
    let mut i = 0usize;
while i < BOUNDARY.len() { if let Some(row) = BOUNDARY.get(i) { union |= row.syscalls; } i += 1; }
    union & SYS_ALL == SYS_ALL
}

// ── 划转账 ────────────────────────────────────────────────────────────
/// A/B 双账。A=存量错误路径，B=统一接管后路径。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SplitLedger {
    /// 存量账
    pub a: [u32; TYPES],
    /// 接管账
    pub b: [u32; TYPES],
}

impl SplitLedger {
    /// 初始账：A 每类 12，B 全零。
    pub const fn new() -> SplitLedger {
        SplitLedger { a: [LEGACY_UNITS; TYPES], b: [0; TYPES] }
    }

    /// 总额（守恒式左边）。
    pub fn total(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
while i < self.a.len() { if let Some(v) = self.a.get(i) { sum = sum.saturating_add(*v); } i += 1; }
        let mut k = 0usize;
while k < self.b.len() { if let Some(v) = self.b.get(k) { sum = sum.saturating_add(*v); } k += 1; }
        sum
    }

    /// 守恒：总额恒为 96。
    pub fn conserved(&self) -> bool {
        self.total() == TOTAL_UNITS
    }

    /// 原子划转：整类从 A 搬到 B；A 已空即部分划转（二次划转）拒绝。
    pub fn transfer(&mut self, idx: usize) -> Result<u32, TakeCode> {
        let cur = match self.a.get(idx) { Some(v) => *v, None => return Err(TakeCode::OutOfBoundary), };
if cur == 0 { return Err(TakeCode::PartialTransfer); }
if let Some(dst) = self.b.get_mut(idx) { *dst = dst.saturating_add(cur); }
if let Some(src) = self.a.get_mut(idx) { *src = 0; }
        Ok(cur)
    }
}

// ── 零重写承诺：重放账 ────────────────────────────────────────────────
/// 存量错误路径样本（重放用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReplaySample {
    /// 样本归属错误类型
    pub err: PermErr,
    /// 对象 mode
    pub mode: u32,
    /// 是否属主类
    pub owner: bool,
    /// 是否 root
    pub root: bool,
}

/// 8 条存量样本（每类错误一条）。
pub const REPLAY: [ReplaySample; TYPES] = [ReplaySample { err: PermErr::Eacces, mode: 0o640, owner: true, root: false },
    ReplaySample { err: PermErr::Eperm, mode: 0o600, owner: false, root: false },
    ReplaySample { err: PermErr::Ebusy, mode: 0o644, owner: true, root: false },
    ReplaySample { err: PermErr::Erofs, mode: 0o600, owner: false, root: false },
    ReplaySample { err: PermErr::Etxtbsy, mode: 0o755, owner: true, root: false },
    ReplaySample { err: PermErr::Eisdir, mode: 0o600, owner: false, root: false },
    ReplaySample { err: PermErr::Enotdir, mode: 0o700, owner: true, root: false },
    ReplaySample { err: PermErr::Enomem, mode: 0o600, owner: false, root: false },
];

/// 冻结重放账：接管前后都必须逐项等于该数组。
pub const FROZEN_REPLAY: [i32; TYPES] = [0, 1, 0, 30, 0, 21, 0, 12];

/// 存量路径判定（只读样本，不读接管状态）。
pub fn legacy_verdict(s: &ReplaySample) -> i32 {
if s.root { return 0; }
    let bits = if s.owner {
        (s.mode >> 6) & 0o7
    } else {
        s.mode & 0o7
    };
if bits & 4 == 4 { 0 } else { s.err.errno() }
}

/// 重放全部样本；rewrite 为真时模拟「接管顺手改了返回码」的破坏实现。
pub fn replay_all(rewrite: bool, transferred: &[bool; TYPES]) -> [i32; TYPES] {
    let mut out = [0i32; TYPES];
    let mut i = 0usize;
while i < REPLAY.len() { if let Some(s) = REPLAY.get(i) { let mut v = legacy_verdict(s); if rewrite { if let Some(flag) = transferred.get(i) { if *flag { v = v.wrapping_add(1); } } } if let Some(slot) = out.get_mut(i) { *slot = v; } } i += 1; }
    out
}

/// 零重写承诺：重放账与冻结账逐项相等。
pub fn promise_holds(rewrite: bool, transferred: &[bool; TYPES]) -> bool {
    let now = replay_all(rewrite, transferred);
    let mut ok = true;
    let mut i = 0usize;
while i < now.len() { if let (Some(a), Some(b)) = (now.get(i), FROZEN_REPLAY.get(i)) { if a != b { ok = false; } } i += 1; }
    ok
}

// ── 增量清单 ──────────────────────────────────────────────────────────
/// 单条增量记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IncrementEntry {
    /// 序号（从 1 连续）
    pub seq: u32,
    /// 错误类型
    pub err: PermErr,
    /// 注记
    pub note: &'static str,
}

/// 增量清单：只增、序号连续、类型唯一。
pub struct IncrementList {
    entries: Vec<IncrementEntry>,
    /// 被拒笔数（显性记账）
    pub rejected: u32,
}

impl IncrementList {
    /// 空清单。
    pub fn new() -> IncrementList {
        IncrementList { entries: Vec::new(), rejected: 0 }
    }

    /// 追加一条；重号/空注记/满容都拒绝。
    pub fn push(&mut self, err: PermErr, note: &'static str) -> Result<u32, TakeCode> {
if note.is_empty() || self.entries.len() >= INCR_MAX { self.rejected = self.rejected.saturating_add(1); return Err(TakeCode::IncrementListViolation); }
        let mut i = 0usize;
while i < self.entries.len() { if let Some(e) = self.entries.get(i) { if e.err == err { self.rejected = self.rejected.saturating_add(1); return Err(TakeCode::IncrementListViolation); } } i += 1; }
        let seq = (self.entries.len() as u32).saturating_add(1);
        self.entries.push(IncrementEntry { seq, err, note });
        Ok(seq)
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 判空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 序号连续且逐项 +1。
    pub fn monotonic(&self) -> bool {
        let mut i = 0usize;
while i < self.entries.len() { if let Some(e) = self.entries.get(i) { if e.seq != (i as u32).saturating_add(1) { return false; } } i += 1; }
        true
    }

    /// 落账完整：8 条、序号连续、类型唯一、注记非空。
    pub fn complete(&self) -> bool {
if self.entries.len() != PermErr::ALL.len() || !self.monotonic() { return false; }
        let mut i = 0usize;
while i < self.entries.len() { if let Some(e) = self.entries.get(i) { if e.note.is_empty() { return false; } let mut j = i + 1; while j < self.entries.len() { if let Some(o) = self.entries.get(j) { if o.err == e.err { return false; } } j += 1; } } i += 1; }
        true
    }

    /// 取第 i 条（越界 None）。
    pub fn get(&self, i: usize) -> Option<IncrementEntry> {
        match self.entries.get(i) { Some(e) => Some(*e), None => None, }
    }
}

// ── 阶段机 ────────────────────────────────────────────────────────────
/// 接管阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TakeStage {
    /// 边界声明完成
    Declared,
    /// 类型登记完成
    Registered,
    /// 划转完成
    Transferred,
    /// 核销完成
    Verified,
}

impl TakeStage {
    /// 下一阶段。
    pub fn next(self) -> Option<TakeStage> {
        match self { TakeStage::Declared => Some(TakeStage::Registered), TakeStage::Registered => Some(TakeStage::Transferred), TakeStage::Transferred => Some(TakeStage::Verified), TakeStage::Verified => None, }
    }

    /// 单向推进。
    pub fn advance(self) -> Result<TakeStage, TakeCode> {
        match self.next() { Some(n) => Ok(n), None => Err(TakeCode::StateRegression), }
    }

    /// 阶段码（指纹用）。
    pub fn code(self) -> u32 {
        match self { TakeStage::Declared => 0, TakeStage::Registered => 1, TakeStage::Transferred => 2, TakeStage::Verified => 3, }
    }
}

// ── 接管主体 ──────────────────────────────────────────────────────────
/// 接管主体：登记表 + 双账 + 增量清单 + 阶段。
pub struct TakeOver {
    /// 各类型是否已登记
    pub registered: [bool; TYPES],
    /// 双账
    pub ledger: SplitLedger,
    /// 增量清单
    pub list: IncrementList,
    /// 阶段
    pub stage: TakeStage,
}

impl TakeOver {
    /// 初始状态：边界已声明，其余未动。
    pub fn new() -> TakeOver {
        TakeOver { registered: [false; TYPES], ledger: SplitLedger::new(), list: IncrementList::new(), stage: TakeStage::Declared, }
    }

    /// 登记一类错误（先登记后划转）。
    pub fn register(&mut self, err: PermErr) -> Result<u32, TakeCode> {
if self.stage != TakeStage::Declared && self.stage != TakeStage::Registered { return Err(TakeCode::StateRegression); }
        let idx = err.index();
        let already = match self.registered.get(idx) { Some(v) => *v, None => return Err(TakeCode::OutOfBoundary), };
if already { return Err(TakeCode::DoubleCount); }
if let Some(slot) = self.registered.get_mut(idx) { *slot = true; }
        self.stage = TakeStage::Registered;
        self.list.push(err, "接管登记")
    }

    /// 划转一类错误到 B 账（边界面必须命中）。
    pub fn take(&mut self, err: PermErr, syscall: u32) -> Result<u32, TakeCode> {
if !boundary_allows(err, syscall) { return Err(TakeCode::CrossSyscall); }
        let idx = err.index();
        let registered = match self.registered.get(idx) { Some(v) => *v, None => return Err(TakeCode::UnregisteredErrno), };
if !registered { return Err(TakeCode::UnregisteredErrno); }
        let before = self.ledger.total();
        let units = self.ledger.transfer(idx)?;
if self.ledger.total() != before || !self.ledger.conserved() { return Err(TakeCode::LedgerNotConserved); }
        self.stage = TakeStage::Transferred;
        Ok(units)
    }

    /// 各类型是否已在 B 账（划转标记）。
    pub fn transferred_flags(&self) -> [bool; TYPES] {
        let mut flags = [false; TYPES];
        let mut i = 0usize;
while i < TYPES { if let Some(v) = self.ledger.b.get(i) { if *v > 0 { if let Some(slot) = flags.get_mut(i) { *slot = true; } } } i += 1; }
        flags
    }

    /// 核销：零重写承诺必须仍然成立。
    pub fn verify(&mut self, rewrite: bool) -> Result<(), TakeCode> {
if self.stage != TakeStage::Transferred { return Err(TakeCode::StateRegression); }
        let flags = self.transferred_flags();
if !promise_holds(rewrite, &flags) { return Err(TakeCode::RewritePromiseBroken); }
        self.stage = TakeStage::Verified;
        Ok(())
    }
}

/// 跑完整接管但不核销（供承诺破坏注入与分阶段断言使用）。
pub fn takeover_pending() -> Result<TakeOver, TakeCode> {
    let mut t = TakeOver::new();
    let mut i = 0usize;
while i < PermErr::ALL.len() { if let Some(err) = PermErr::ALL.get(i).copied() { t.register(err)?; } i += 1; }
    let mut k = 0usize;
while k < PermErr::ALL.len() { if let Some(err) = PermErr::ALL.get(k).copied() { let syscall = match PRIMARY_SYSCALL.get(k) { Some(s) => *s, None => SYS_OPEN, }; t.take(err, syscall)?; } k += 1; }
    Ok(t)
}

/// 完整接管并核销（零重写承诺版）。
pub fn run_takeover() -> Result<TakeOver, TakeCode> {
    let mut t = takeover_pending()?;
    t.verify(false)?;
    Ok(t)
}

// ── 注入账与坏样本 ────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数
    pub escaped: u32,
}

impl InjectLedger {
    /// 空账。
    pub const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入。
    pub fn record(&mut self, caught: bool) -> Result<(), TakeCode> {
        self.injected = self.injected.saturating_add(1);
if caught { self.detected = self.detected.saturating_add(1); } else { self.escaped = self.escaped.saturating_add(1); }
if self.detected.saturating_add(self.escaped) != self.injected { return Err(TakeCode::InjectLedgerRegress); }
        Ok(())
    }

    /// 守恒判定。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 五类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadTakeCase {
    /// 未登记就划转
    Unregistered,
    /// 跨系统调用面接管
    CrossSyscall,
    /// 部分（二次）划转
    PartialTransfer,
    /// 重复登记（双计）
    DoubleCount,
    /// 零重写承诺破坏
    PromiseBreak,
}

impl BadTakeCase {
    /// 探针：返回是否被接管核抓到。
    pub fn probe(self) -> bool {
        match self { BadTakeCase::Unregistered => { let mut t = TakeOver::new(); t.take(PermErr::Eacces, SYS_OPEN) == Err(TakeCode::UnregisteredErrno) } BadTakeCase::CrossSyscall => { let mut t = TakeOver::new(); let reg = t.register(PermErr::Eacces); let cross = t.take(PermErr::Eacces, SYS_CHOWN); reg.is_ok() && cross == Err(TakeCode::CrossSyscall) } BadTakeCase::PartialTransfer => { let mut ledger = SplitLedger::new(); let first = ledger.transfer(0); let second = ledger.transfer(0); first.is_ok() && second == Err(TakeCode::PartialTransfer) } BadTakeCase::DoubleCount => { let mut t = TakeOver::new(); let first = t.register(PermErr::Eperm); let second = t.register(PermErr::Eperm); first.is_ok() && second == Err(TakeCode::DoubleCount) } BadTakeCase::PromiseBreak => match takeover_pending() { Ok(mut t) => t.verify(true) == Err(TakeCode::RewritePromiseBroken), Err(_) => false, }, }
    }

    /// 期望诊断码。
    pub fn code(self) -> TakeCode {
        match self { BadTakeCase::Unregistered => TakeCode::UnregisteredErrno, BadTakeCase::CrossSyscall => TakeCode::CrossSyscall, BadTakeCase::PartialTransfer => TakeCode::PartialTransfer, BadTakeCase::DoubleCount => TakeCode::DoubleCount, BadTakeCase::PromiseBreak => TakeCode::RewritePromiseBroken, }
    }

    /// 闭集全量。
    pub const ALL: [BadTakeCase; 5] = [BadTakeCase::Unregistered, BadTakeCase::CrossSyscall, BadTakeCase::PartialTransfer, BadTakeCase::DoubleCount, BadTakeCase::PromiseBreak,];
}

/// 五类注入各 INJECT_ROUNDS 轮。
pub fn run_take_matrix() -> (InjectLedger, [u32; 5]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
while ci < BadTakeCase::ALL.len() { if let Some(case) = BadTakeCase::ALL.get(ci).copied() { let mut round = 0u32; while round < INJECT_ROUNDS { let caught = case.probe(); let _ = ledger.record(caught); if caught { if let Some(slot) = per_class.get_mut(ci) { *slot = slot.saturating_add(1); } } round = round.saturating_add(1); } } ci += 1; }
    (ledger, per_class)
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
fn mix(h: u32, v: u32) -> u32 {
    (h ^ v).wrapping_mul(0x0100_0193)
}

/// 语料指纹：边界声明 + 接管后双账 + 重放账 + 阶段。
pub fn corpus_fingerprint() -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
while i < BOUNDARY.len() { if let Some(row) = BOUNDARY.get(i) { h = mix(h, row.err.index() as u32); h = mix(h, row.syscalls); } i += 1; }
    match run_takeover() { Ok(t) => { h = mix(h, t.ledger.total()); h = mix(h, t.list.len() as u32); h = mix(h, t.stage.code()); } Err(_) => h = mix(h, 0), }
    let replay = replay_all(false, &[true; TYPES]);
    let mut k = 0usize;
while k < replay.len() { if let Some(v) = replay.get(k) { h = mix(h, *v as u32); } k += 1; }
    h
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9278 判据集（静态 19 条 + 动态 3 条）。
pub fn run_unx9278_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9278");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x7B300 0x40 对齐且在批窗口内", seg_ok, "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：完整接管成功
    let full = run_takeover();
    let full_ok = match &full { Ok(t) => t.stage == TakeStage::Verified, Err(_) => false, };
    s.add("正常流：8 类登记→划转→核销一气跑通", full_ok, "Declared→Registered→Transferred→Verified 必须走完");

    // 3. 划转结果：A 全零、B 全 12
    let split_ok = match &full { Ok(t) => { let mut ok = true; let mut i = 0usize; while i < TYPES { if let (Some(a), Some(b)) = (t.ledger.a.get(i), t.ledger.b.get(i)) { if *a != 0 || *b != LEGACY_UNITS { ok = false; } } i += 1; } ok } Err(_) => false, };
    s.add("划转：A 账全零、B 账每类 12 单位", split_ok, "一次接管必须整类搬移，禁止半搬或凭空加账");

    // 4. 守恒：总额 96
    let conserved_ok = match &full { Ok(t) => t.ledger.conserved() && t.ledger.total() == TOTAL_UNITS, Err(_) => false, };
    s.add("守恒：A+B 总额恒为 96（8×12）", conserved_ok, "划转只改分布，不改总量");

    // 5. 零重写承诺：8 样本重放与冻结账一致
    let flags = match &full { Ok(t) => t.transferred_flags(), Err(_) => [false; TYPES], };
    let replay = replay_all(false, &flags);
    let replay_ok = promise_holds(false, &flags);
    let mut frozen_ok = true;
    let mut ri = 0usize;
while ri < replay.len() { if let (Some(v), Some(f)) = (replay.get(ri), FROZEN_REPLAY.get(ri)) { if v != f { frozen_ok = false; } } ri += 1; }
    s.add("零重写承诺：8 条存量样本重放与冻结账逐项相等", replay_ok && frozen_ok && REPLAY.len() == TYPES, "接管只改归属，不得改判定结果与错误码");

    // 6. 承诺破坏可检出
    s.add("零重写承诺：改写返回码的破坏实现在核销时必被抓", !promise_holds(true, &flags), "承诺格必须能反向证伪，否则只是一句声明");

    // 7. 边界声明完整
    s.add("边界声明：8 行序号对齐、掩码非零合法、注记非空", boundary_complete() && BOUNDARY.len() == TYPES, "边界是接管的前置条件，缺行或缺注记都不能收口");

    // 8. 六面覆盖
    s.add("边界声明：六个系统调用面全部被覆盖", syscalls_covered(), "未被覆盖的面等于没有边界声明");

    // 9. 越面拒绝
    let cross_ok = !boundary_allows(PermErr::Eacces, SYS_CHOWN)
        && boundary_allows(PermErr::Eacces, SYS_OPEN)
        && !boundary_allows(PermErr::Eperm, SYS_OPEN)
        && boundary_allows(PermErr::Eperm, SYS_CHOWN)
        && !boundary_allows(PermErr::Eacces, SYS_OPEN | SYS_MKDIR);
    s.add("越面：EACCES×CHOWN 与 EPERM×OPEN 均拒，多比特声明也拒", cross_ok, "二维边界必须逐格判定，不接受模糊面");

    // 10. 未登记先划转拒绝
    let mut t10 = TakeOver::new();
    let unreg = t10.take(PermErr::Eacces, SYS_OPEN);
    s.add("判序：未登记先划转 → UnregisteredErrno", unreg == Err(TakeCode::UnregisteredErrno), "边界命中先于登记检查，登记缺失必须先报未登记");

    // 11. 增量清单落账
    let list_ok = match &full { Ok(t) => t.list.len() == TYPES && t.list.complete() && t.list.rejected == 0, Err(_) => false, };
    s.add("增量清单：8 条、序号 1..8 连续、类型唯一、零拒绝", list_ok, "清单只增不减，序号断裂即失去对账能力");

    // 12. 清单单调
    let mono_list = match &full { Ok(t) => t.list.monotonic(), Err(_) => false, };
    s.add("增量清单：序号严格 +1 单调", mono_list, "序号单调是增量可重放的唯一依据");

    // 13. 双计拒绝
    let mut t13 = TakeOver::new();
    let a13 = t13.register(PermErr::Ebusy);
    let b13 = t13.register(PermErr::Ebusy);
    s.add("双计：同类型二次登记 → DoubleCount", a13.is_ok() && b13 == Err(TakeCode::DoubleCount), "一条错误任意时刻只允许一处归属");

    // 14. 部分划转拒绝
    let mut ledger14 = SplitLedger::new();
    let a14 = ledger14.transfer(2);
    let b14 = ledger14.transfer(2);
    s.add("部分划转：二次划转同类 → PartialTransfer", a14.is_ok() && b14 == Err(TakeCode::PartialTransfer), "原子划转必须一次搬完，半搬会造成双计");

    // 15. 阶段机单向
    let mut st = TakeStage::Declared;
    let mut steps = 0u32;
    let mut forward = true;
while let Ok(next) = st.advance() { st = next; steps = steps.saturating_add(1); if steps > 8 { forward = false; break; } }
    s.add("状态机：Declared→Registered→Transferred→Verified 单向三步", forward && steps == 3 && st == TakeStage::Verified && st.advance() == Err(TakeCode::StateRegression), "回退会让核销读到未划转的账");

    // 16. 核销前置：未划转就核销拒绝
    let mut t16 = TakeOver::new();
    let early = t16.verify(false);
    s.add("判序：未划转就核销 → StateRegression", early == Err(TakeCode::StateRegression), "核销必须发生在划转之后");

    // 17. 注入五类各 10/10
    let (ledger, per_class) = run_take_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入必抓：未登记/越面/部分划转/双计/承诺破坏各 10/10", all_caught, "五类违约样本必须全部被接管核抓到");

    // 18. 注入账守恒
    s.add("守恒：注入 50 = 检出 50 + 逃逸 0", ledger.conserved() && ledger.escaped == 0 && ledger.injected == BadTakeCase::ALL.len() as u32 * INJECT_ROUNDS, "50 次注入必须 50 次检出、0 逃逸");

    // 19. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：语料指纹两次相等", a == b, "同输入两次必须逐字段相等，隐藏状态即失败");

    // 20. 文案闭集
    let mut text_ok = true;
    let mut i20 = 0usize;
while i20 < TakeCode::ALL.len() { if let Some(c) = TakeCode::ALL.get(i20) { if c.val() != SEG_BASE + i20 as u32 || c.detail().is_empty() { text_ok = false; } let mut j20 = i20 + 1; while j20 < TakeCode::ALL.len() { if let Some(o) = TakeCode::ALL.get(j20) { if c.val() == o.val() { text_ok = false; } } j20 += 1; } } i20 += 1; }
    s.add("文案闭集：12 个诊断码连续、唯一、detail 非空", text_ok, "每个码必须能取到非空且可操作的说明");

    // 21. 账目单调
    let mut mono = ledger;
    let before = mono.injected;
    let mut k = 0u32;
while k < 4 { let _ = mono.record(true); k = k.saturating_add(1); }
    s.add("账目单调：再注入 4 次只增不减", mono.injected == before + 4, "累计计数禁止回退或饱和到旧值");

    // 动态明细（≤8）
    let split_text = match &full { Ok(t) => format!("a0={} b0={} total={}", match t.ledger.a.get(0) { Some(v) => *v, None => 0 }, match t.ledger.b.get(0) { Some(v) => *v, None => 0 }, t.ledger.total()), Err(_) => String::from("takeover=err"), };
    s.add_dyn("划转快照", conserved_ok, &split_text);
    let list_text = match &full { Ok(t) => format!("incr={} rejected={}", t.list.len(), t.list.rejected), Err(_) => String::from("incr=0"), };
    s.add_dyn("清单快照", list_ok, &list_text);
    let ledger_text = format!("inj={} det={} esc={}", ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    s.add_dyn("指纹快照", a == b, &format!("fp=0x{:08X}", a));

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundary_rows_are_complete() {
        assert_eq!(BOUNDARY.len(), TYPES);
        assert!(boundary_complete());
        assert!(syscalls_covered());
        assert_eq!(BOUNDARY.len(), PermErr::ALL.len());
    }

    #[test]
    fn boundary_rejects_cross_syscall() {
        assert!(boundary_allows(PermErr::Eacces, SYS_OPEN));
        assert!(!boundary_allows(PermErr::Eacces, SYS_CHOWN));
        assert!(boundary_allows(PermErr::Eperm, SYS_CHOWN));
        assert!(!boundary_allows(PermErr::Eperm, SYS_OPEN));
    }

    #[test]
    fn transfer_is_atomic_and_conserved() {
        let mut ledger = SplitLedger::new();
        assert!(ledger.conserved());
        assert_eq!(ledger.transfer(0), Ok(LEGACY_UNITS));
        assert_eq!(ledger.transfer(0), Err(TakeCode::PartialTransfer));
        assert_eq!(ledger.total(), TOTAL_UNITS);
        assert!(ledger.conserved());
    }

    #[test]
    fn full_takeover_lands_in_b() {
        let full = run_takeover();
        match full { Ok(t) => { assert_eq!(t.stage, TakeStage::Verified); assert!(t.ledger.conserved()); assert_eq!(t.list.len(), TYPES); assert!(t.list.complete()); let mut i = 0usize; while i < TYPES { assert_eq!(t.ledger.a.get(i).copied(), Some(0)); assert_eq!(t.ledger.b.get(i).copied(), Some(LEGACY_UNITS)); i += 1; } } Err(e) => panic!("takeover failed: {:?}", e), }
    }

    #[test]
    fn promise_holds_and_breaks() {
        let flags = [true; TYPES];
        assert!(promise_holds(false, &flags));
        assert!(!promise_holds(true, &flags));
        assert_eq!(replay_all(false, &flags), FROZEN_REPLAY);
    }

    #[test]
    fn unregistered_take_is_rejected() {
        let mut t = TakeOver::new();
        assert_eq!(t.take(PermErr::Eacces, SYS_OPEN), Err(TakeCode::UnregisteredErrno));
    }

    #[test]
    fn double_register_rejected() {
        let mut t = TakeOver::new();
        assert!(t.register(PermErr::Ebusy).is_ok());
        assert_eq!(t.register(PermErr::Ebusy), Err(TakeCode::DoubleCount));
    }

    #[test]
    fn verify_requires_transfer() {
        let mut t = TakeOver::new();
        assert_eq!(t.verify(false), Err(TakeCode::StateRegression));
        match takeover_pending() { Ok(mut p) => { assert_eq!(p.stage, TakeStage::Transferred); assert_eq!(p.verify(true), Err(TakeCode::RewritePromiseBroken)); } Err(_) => assert!(false, "pending takeover failed"), }
    }

    #[test]
    fn increment_list_is_monotonic() {
        let mut list = IncrementList::new();
        let mut i = 0usize;
while i < PermErr::ALL.len() { if let Some(err) = PermErr::ALL.get(i).copied() { assert_eq!(list.push(err, "登记"), Ok((i as u32) + 1)); } i += 1; }
        assert!(list.complete());
        assert!(list.monotonic());
        assert_eq!(list.len(), TYPES);
        assert_eq!(list.push(PermErr::Eacces, "重号"), Err(TakeCode::IncrementListViolation));
    }

    #[test]
    fn injection_matrix_all_caught() {
        let (l, per) = run_take_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 50);
for n in per.iter() { assert_eq!(*n, INJECT_ROUNDS); }
    }

    #[test]
    fn stage_machine_is_forward_only() {
        let mut st = TakeStage::Declared;
        let mut n = 0;
while let Ok(next) = st.advance() { st = next; n += 1; }
        assert_eq!(n, 3);
        assert_eq!(st, TakeStage::Verified);
        assert_eq!(st.advance(), Err(TakeCode::StateRegression));
    }

    #[test]
    fn fingerprint_and_checks_green() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
        let set = run_unx9278_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
        assert!(!set.truncated());
    }
}
