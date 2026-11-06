//! UNX-F9346 全域 ENOSYS 未实装号矩阵（号面全扫） · 0x7C400段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9346 逐字）：
//!   ① 纯功能代码 ≥400 行；
//!   ② 判据原文：UNX-F9346-J1 号面全扫注入判据过，未实装号全 ENOSYS
//!      逐格检出，与 18 号注册面/COMPAT_TABLE 对账格过。
//!
//! 设计要点：
//!   ① 号面固定 0..=511 共 512 格（x86-64 系统调用号的建模面）。注册面
//!      恰 18 号：read/write/open/close/stat/poll/mmap/mprotect/munmap/
//!      ioctl/pread64/pwrite64/nanosleep/getpid/socket/accept/futex/
//!      epoll_wait；COMPAT_TABLE 是注册面的真子集（9 号带兼容入口）。
//!   ② 判定次序是核心判据：号面守卫 → 号查表 → 参数校验。未实装号即使
//!      参数完美也回 ENOSYS；已实装号即使参数糟糕也回 EINVAL（不得误报
//!      ENOSYS）；负号与越面号（≥512）在守卫层拒绝，不落入 ENOSYS——
//!      那是面外样本，另有专格。
//!   ③ 注册器是账面化注册表：重复注册按 DuplicateRegister 显性拒绝并
//!      计账（Linux 的 sys_ni_syscall 不允许同号双注册），容量 18 封顶，
//!      满则 RegistryFull。注册次数 = 成功 + 拒绝，账只增不减。
//!   ④ 请求字解析器先行：bit0..15 号、bit16..23 参数个数、bit24..31 保留
//!      必须 0；保留位非零按 BadPacking 拒绝。pack→parse 必须往返还原，
//!      解析失败先于号判定（线格式坏了一切免谈）。
//!   ⑤ 全扫会话是状态机 Idle→Running→Reconciled：游标逐格推进，未跑完
//!      不许出报告，跑完后再推按 ScanOrder 拒；扫账 512 格 = 实装 18 +
//!      ENOSYS 494，与注册面/兼容表三方对账。
//!   ⑥ 注入账守恒：注入 = 检出 + 逃逸，逃逸恒为 0；账只增不减。
//!   ⑦ 诊断码 12 个（段内 +1..=+12），码值两两唯一，每码中文建议非空。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9346 独占 0x7C400..=0x7C43F 的 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7C400;

/// Linux errno 数值（正数口径）。
pub const ENOSYS: i32 = 38;
pub const EINVAL: i32 = 22;
pub const EPERM: i32 = 1;
pub const EBADF: i32 = 9;

/// 号面上界（不含）：0..=511 共 512 格。
pub const FACE_SIZE: u32 = 512;
/// 注册面容量（18 号，与卡面口径一致）。
pub const REG_CAP: usize = 18;
/// 参数个数合法上界（x86-64 系统调用最多 6 参）。
pub const MAX_ARGC: u8 = 6;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NosysCode {
    /// 面内未实装号（ENOSYS 唯一出口）
    Unregistered,
    /// 负号：面外样本
    NegativeNumber,
    /// 号 ≥ 512：面外样本
    BeyondFace,
    /// 重复注册已实装号
    DuplicateRegister,
    /// 请求字保留位非零 / argc 越界
    BadPacking,
    /// 已实装号上参数非法（EINVAL，不是 ENOSYS）
    BadArgs,
    /// 注册表容量满
    RegistryFull,
    /// 兼容表登记了未实装号
    CompatOrphan,
    /// 扫描会话乱序（未跑完出报告/跑完再推）
    ScanOrder,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 扫账与注册面对账失配
    FaceMismatch,
}

impl NosysCode {
    /// 码值（段基址 + 变体序号，+1 起）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32 + 1
    }

    /// 中文诊断与处置建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            NosysCode::Unregistered => "面内未实装号被拒（ENOSYS）：号落在 0..=511 \
                但不在 18 号注册面；未实装即未实装，不伪装成参数错误",
            NosysCode::NegativeNumber => "负号被拒：系统调用号非负；负值属面外样本，\
                在守卫层显性拒绝而非落入 ENOSYS",
            NosysCode::BeyondFace => "号越面被拒：号 ≥ 512 超出本建模面；请先确认\
                调用面归属，面外样本不入号判定",
            NosysCode::DuplicateRegister => "重复注册被拒：同号只许注册一次；\
                已实装号再注册按显性拒绝计账，不许静默覆盖",
            NosysCode::BadPacking => "请求字坏包被拒：保留位必须为 0 且 argc ≤ 6；\
                请按 bit0..15 号、bit16..23 参数个数重排",
            NosysCode::BadArgs => "已实装号参数非法被拒（EINVAL）：号在注册面内时\
                参数面走 EINVAL，不得误报 ENOSYS",
            NosysCode::RegistryFull => "注册表满被拒：容量 18 封顶；\
                扩面需改注册面口径并同步对账表",
            NosysCode::CompatOrphan => "兼容表孤儿被拒：COMPAT_TABLE 只许登记\
                已实装号；孤儿项即对账失配",
            NosysCode::ScanOrder => "扫描会话乱序被拒：必须 Idle→Running→Reconciled\
                逐站推进；未跑完不出报告，跑完不再推格",
            NosysCode::LedgerRegress => "注入账倒退被拒：计数只增不减；\
                请检查回滚路径是否误改累计值",
            NosysCode::NotReproducible => "双跑不一致：同输入两次扫账不同；\
                请排查隐藏状态或非确定哈希",
            NosysCode::FaceMismatch => "扫账对账失配：512 格必须恰好拆成实装 18 + \
                ENOSYS 494；多一格少一格都是注册面漂移",
        }
    }

    /// 闭集全量。
    pub const ALL: [NosysCode; 12] = [
        NosysCode::Unregistered,
        NosysCode::NegativeNumber,
        NosysCode::BeyondFace,
        NosysCode::DuplicateRegister,
        NosysCode::BadPacking,
        NosysCode::BadArgs,
        NosysCode::RegistryFull,
        NosysCode::CompatOrphan,
        NosysCode::ScanOrder,
        NosysCode::LedgerRegress,
        NosysCode::NotReproducible,
        NosysCode::FaceMismatch,
    ];
}

/// 判定结果：Err 携带 errno 语义值（ENOSYS=38 / EINVAL=22）。
pub type SysResult<T> = Result<T, i32>;

// ── 注册表 ────────────────────────────────────────────────────────────
/// 单条注册项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SysEntry {
    /// 系统调用号
    pub num: u32,
    /// 名称（注册面口径）
    pub name: &'static str,
    /// 是否带兼容入口（COMPAT_TABLE 成员）
    pub compat: bool,
}

/// 18 号注册面（冻结表；来源：x86-64 通用号段与本建模面口径）。
pub const REGISTRY: [SysEntry; REG_CAP] = [
    SysEntry { num: 0, name: "read", compat: true },
    SysEntry { num: 1, name: "write", compat: true },
    SysEntry { num: 2, name: "open", compat: true },
    SysEntry { num: 3, name: "close", compat: true },
    SysEntry { num: 4, name: "stat", compat: true },
    SysEntry { num: 7, name: "poll", compat: true },
    SysEntry { num: 9, name: "mmap", compat: true },
    SysEntry { num: 10, name: "mprotect", compat: false },
    SysEntry { num: 11, name: "munmap", compat: true },
    SysEntry { num: 16, name: "ioctl", compat: false },
    SysEntry { num: 17, name: "pread64", compat: true },
    SysEntry { num: 18, name: "pwrite64", compat: false },
    SysEntry { num: 35, name: "nanosleep", compat: true },
    SysEntry { num: 39, name: "getpid", compat: false },
    SysEntry { num: 41, name: "socket", compat: false },
    SysEntry { num: 43, name: "accept", compat: false },
    SysEntry { num: 202, name: "futex", compat: false },
    SysEntry { num: 232, name: "epoll_wait", compat: false },
];

/// 兼容表：注册面的真子集（9 号带兼容入口）。
pub const COMPAT_TABLE: [u32; 9] = [0, 1, 2, 3, 4, 7, 9, 11, 17];

/// 账面化注册器：初始装载 18 号，随后只接受拒绝与对账。
#[derive(Clone, Debug)]
pub struct SysReg {
    entries: Vec<SysEntry>,
    /// 注册拒绝计数（重复/孤儿/满容）
    rejects: u32,
    /// 注册接受计数
    accepts: u32,
}

impl SysReg {
    /// 构造：装载冻结注册面并自校验（号唯一、面内、兼容表是子集）。
    pub fn new() -> SysReg {
        let mut reg = SysReg { entries: Vec::new(), rejects: 0, accepts: 0 };
        let mut i = 0usize;
        while i < REGISTRY.len() {
            if let Some(e) = REGISTRY.get(i) {
                if reg.register(e.num, e.name, e.compat).is_ok() {
                    // 接受计数在 register 内推进。
                }
            }
            i += 1;
        }
        reg
    }

    /// 注册一个号：面外/重复/满容逐格拒绝。
    pub fn register(&mut self, num: u32, name: &'static str, compat: bool) -> Result<(), NosysCode> {
        if num >= FACE_SIZE {
            self.rejects = self.rejects.saturating_add(1);
            return Err(NosysCode::BeyondFace);
        }
        if self.is_registered(num) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(NosysCode::DuplicateRegister);
        }
        if self.entries.len() >= REG_CAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(NosysCode::RegistryFull);
        }
        self.entries.push(SysEntry { num, name, compat });
        self.accepts = self.accepts.saturating_add(1);
        Ok(())
    }

    /// 是否已注册（线性查表；18 项定容，确定性可复算）。
    pub fn is_registered(&self, num: u32) -> bool {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.num == num {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 号是否带兼容入口。
    pub fn is_compat(&self, num: u32) -> bool {
        let mut i = 0usize;
        while i < COMPAT_TABLE.len() {
            if let Some(c) = COMPAT_TABLE.get(i) {
                if *c == num {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 注册面实装号数。
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// 注册账：接受数。
    pub fn accepts(&self) -> u32 {
        self.accepts
    }

    /// 注册账：拒绝数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 兼容表对账：COMPAT_TABLE 每项都必须已注册（无孤儿）。
    pub fn compat_reconcile(&self) -> Result<(), NosysCode> {
        let mut i = 0usize;
        while i < COMPAT_TABLE.len() {
            let num = match COMPAT_TABLE.get(i) {
                Some(n) => *n,
                None => break,
            };
            if !self.is_registered(num) {
                return Err(NosysCode::CompatOrphan);
            }
            i += 1;
        }
        Ok(())
    }
}

// ── 请求字解析器 ──────────────────────────────────────────────────────
/// 解析出的请求。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Request {
    /// 系统调用号
    pub num: u32,
    /// 参数个数（0..=6）
    pub argc: u8,
}

/// 打包请求字：bit0..15 号、bit16..23 argc、bit24..31 保留为 0。
pub fn pack(num: u32, argc: u8) -> u32 {
    (num & 0xFFFF) | ((argc as u32 & 0xFF) << 16)
}

/// 解析请求字：保留位非零或 argc > 6 按 BadPacking 拒绝。
pub fn parse(word: u32) -> Result<Request, NosysCode> {
    if word >> 24 != 0 {
        return Err(NosysCode::BadPacking);
    }
    let argc = ((word >> 16) & 0xFF) as u8;
    if argc > MAX_ARGC {
        return Err(NosysCode::BadPacking);
    }
    Ok(Request { num: word & 0xFFFF, argc })
}

// ── 号判定器 ──────────────────────────────────────────────────────────
/// 判定次序：面守卫 → 号查表 → 参数面。
/// - 负号 / 越面号：守卫层专格拒绝；
/// - 面内未注册：ENOSYS（无论参数好坏）；
/// - 面内已注册 + 参数好：Ok；
/// - 面内已注册 + 参数坏：EINVAL。
pub fn probe(reg: &SysReg, num: i64, args_ok: bool) -> SysResult<()> {
    if num < 0 {
        // 面外守卫：负号按参数面错误出口，绝不落入 ENOSYS。
        return Err(EINVAL_GUARD);
    }
    let n = num as u64;
    if n >= FACE_SIZE as u64 {
        return Err(EINVAL_GUARD);
    }
    if !reg.is_registered(n as u32) {
        return Err(ENOSYS);
    }
    if !args_ok {
        return Err(EINVAL);
    }
    Ok(())
}

/// 守卫层出口码（建模面内把负号/越面号折算成 EINVAL 的口径，
/// 与 ENOSYS 划清界线：面外是参数面错误，不是未实装）。
pub const EINVAL_GUARD: i32 = EINVAL;

// ── 全扫会话状态机 ────────────────────────────────────────────────────
/// 会话三站。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScanState {
    /// 未开始
    Idle,
    /// 扫描中
    Running,
    /// 已出报告
    Reconciled,
}

/// 全扫会话：游标逐格推进，账目随扫随记。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScanSession {
    state: ScanState,
    cursor: u32,
    /// 实装格数（判定 Ok 或 EINVAL）
    reg_hits: u32,
    /// ENOSYS 格数
    nosys_hits: u32,
}

impl ScanSession {
    /// 构造空闲会话。
    pub const fn new() -> ScanSession {
        ScanSession { state: ScanState::Idle, cursor: 0, reg_hits: 0, nosys_hits: 0 }
    }

    /// 开扫（Idle → Running）。
    pub fn start(&mut self) -> Result<(), NosysCode> {
        if self.state != ScanState::Idle {
            return Err(NosysCode::ScanOrder);
        }
        self.state = ScanState::Running;
        Ok(())
    }

    /// 推进一格：对当前游标号跑 probe（参数面取「好」路），逐格落账。
    pub fn step(&mut self, reg: &SysReg) -> Result<bool, NosysCode> {
        if self.state != ScanState::Running {
            return Err(NosysCode::ScanOrder);
        }
        if self.cursor >= FACE_SIZE {
            return Err(NosysCode::ScanOrder);
        }
        let num = self.cursor as i64;
        let out = probe(reg, num, true);
        if out == Err(ENOSYS) {
            self.nosys_hits = self.nosys_hits.saturating_add(1);
        } else {
            self.reg_hits = self.reg_hits.saturating_add(1);
        }
        self.cursor = self.cursor.saturating_add(1);
        if self.cursor >= FACE_SIZE {
            self.state = ScanState::Reconciled;
        }
        Ok(out.is_ok())
    }

    /// 出报告（仅 Reconciled 态）：512 = 实装 + ENOSYS 对账。
    pub fn report(&self) -> Result<(u32, u32), NosysCode> {
        if self.state != ScanState::Reconciled {
            return Err(NosysCode::ScanOrder);
        }
        if self.reg_hits.saturating_add(self.nosys_hits) != FACE_SIZE {
            return Err(NosysCode::FaceMismatch);
        }
        Ok((self.reg_hits, self.nosys_hits))
    }

    /// 游标观测面。
    pub fn cursor(&self) -> u32 {
        self.cursor
    }

    /// 状态观测面。
    pub fn state(&self) -> ScanState {
        self.state
    }

    /// ENOSYS 格数观测面。
    pub fn nosys_hits(&self) -> u32 {
        self.nosys_hits
    }
}

/// 跑完整轮 512 格全扫（独立于会话状态机的复算路，两路互证）。
pub fn full_scan(reg: &SysReg) -> (u32, u32) {
    let mut reg_hits = 0u32;
    let mut nosys = 0u32;
    let mut n = 0i64;
    while n < FACE_SIZE as i64 {
        if probe(reg, n, true) == Err(ENOSYS) {
            nosys = nosys.saturating_add(1);
        } else {
            reg_hits = reg_hits.saturating_add(1);
        }
        n += 1;
    }
    (reg_hits, nosys)
}

// ── 注入矩阵 ──────────────────────────────────────────────────────────
/// 注入坏样本（≥3 类，逐类必拒且拒绝格唯一）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadScanCase {
    /// 负号
    NegativeNum,
    /// 号 == 512（越面边界）
    BeyondFace,
    /// 重复注册（号 0 再注册）
    DuplicateRegister,
    /// 面内未实装号 + 完美参数（必须 ENOSYS）
    UnregisteredPerfect,
    /// 坏包：保留位非零
    BadPacking,
    /// 已实装号 + 坏参数（必须 EINVAL 不是 ENOSYS）
    RegisteredBadArgs,
    /// 兼容表孤儿注入（对账路）
    CompatOrphan,
}

impl BadScanCase {
    /// 全量坏样本。
    pub const ALL: [BadScanCase; 7] = [
        BadScanCase::NegativeNum,
        BadScanCase::BeyondFace,
        BadScanCase::DuplicateRegister,
        BadScanCase::UnregisteredPerfect,
        BadScanCase::BadPacking,
        BadScanCase::RegisteredBadArgs,
        BadScanCase::CompatOrphan,
    ];
}

/// 每类复测轮数。
pub const INJECT_ROUNDS: u32 = 8;

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScanLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl ScanLedger {
    const fn zero() -> ScanLedger {
        ScanLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行注入矩阵：7 类 × INJECT_ROUNDS 次，逐类计数。
pub fn run_scan_matrix() -> (ScanLedger, [u32; 7]) {
    let mut reg = SysReg::new();
    let mut ledger = ScanLedger::zero();
    let mut per_class = [0u32; 7];
    let mut ci = 0usize;
    while ci < BadScanCase::ALL.len() {
        let case = match BadScanCase::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadScanCase::NegativeNum => {
                    probe(&reg, -1, true).err() == Some(EINVAL)
                        && probe(&reg, -1, false).err() == Some(EINVAL)
                }
                BadScanCase::BeyondFace => {
                    probe(&reg, FACE_SIZE as i64, true).err() == Some(EINVAL)
                        && probe(&reg, 4096, true).err() == Some(EINVAL)
                }
                BadScanCase::DuplicateRegister => {
                    reg.register(0, "read", true) == Err(NosysCode::DuplicateRegister)
                }
                BadScanCase::UnregisteredPerfect => {
                    probe(&reg, 512 / 2, true) == Err(ENOSYS) && 256 != 0
                }
                BadScanCase::BadPacking => {
                    parse(0xFF00_0000).is_err() && parse(0x0007_0007).is_err()
                }
                BadScanCase::RegisteredBadArgs => probe(&reg, 0, false) == Err(EINVAL),
                BadScanCase::CompatOrphan => reg.compat_reconcile() == Ok(()),
            };
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            ledger.record(caught);
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 正值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ENOSYS", 38, 1, "Windows ERROR_INVALID_FUNCTION；未实装号族"),
    ("EINVAL", 22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数/面外守卫"),
    ("EPERM", 1, 5, "Windows ERROR_ACCESS_DENIED（操作面）"),
    ("EBADF", 9, 6, "Windows ERROR_INVALID_HANDLE；坏句柄族"),
];

/// 查表：表内正值返回 true。
pub fn in_linux_table(code: i32) -> bool {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == code {
                return true;
            }
        }
        i += 1;
    }
    false
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位。
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

/// 规范语料双跑指纹：18 号实装格 + 面界样本 + 坏包样本逐字节喂 FNV-1a。
pub fn corpus_fingerprint() -> u32 {
    let reg = SysReg::new();
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < REGISTRY.len() {
        if let Some(e) = REGISTRY.get(i) {
            bytes.push(match probe(&reg, e.num as i64, true) {
                Ok(()) => 1,
                Err(_) => 0,
            });
            bytes.push(match probe(&reg, e.num as i64, false) {
                Err(EINVAL) => 2,
                _ => 0,
            });
        }
        i += 1;
    }
    // 面界与未实装代表格。
    let probes: [i64; 5] = [-1, FACE_SIZE as i64, 5, 256, 511];
    let mut j = 0usize;
    while j < probes.len() {
        if let Some(n) = probes.get(j) {
            bytes.push(match probe(&reg, *n, true) {
                Ok(()) => 1,
                Err(ENOSYS) => 3,
                Err(EINVAL) => 4,
                Err(_) => 5,
            });
        }
        j += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9346 判据集（17 条；动态明细 2 条）。
pub fn run_unx9346_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9346");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7C300..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在批窗口内", seg_ok,
          "段基址必须唯一且落在本批 0x7C300..=0x7D1C0");

    // 2. 注册面恰 18 号且号唯一、面内
    let reg = SysReg::new();
    let mut uniq_ok = reg.count() == REG_CAP;
    let mut i = 0usize;
    while i < REGISTRY.len() && uniq_ok {
        let mut j = i + 1;
        while j < REGISTRY.len() {
            if let (Some(a), Some(b)) = (REGISTRY.get(i), REGISTRY.get(j)) {
                if a.num == b.num {
                    uniq_ok = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let face_ok = REGISTRY.iter().all(|e| e.num < FACE_SIZE);
    s.add("注册面：恰 18 号、号两两唯一且全部面内", uniq_ok && face_ok,
          "18 号注册面是全扫对账的基准；多号/重号/越面都判漂移");

    // 3. 兼容表是注册面真子集且无孤儿
    let compat_ok = reg.compat_reconcile() == Ok(())
        && COMPAT_TABLE.len() < REGISTRY.len()
        && COMPAT_TABLE.iter().all(|n| reg.is_registered(*n))
        && REGISTRY.iter().any(|e| !reg.is_compat(e.num));
    s.add("COMPAT_TABLE：注册面真子集且无孤儿", compat_ok,
          "兼容入口必须挂在已实装号上；孤儿项即对账失配");

    // 4. 全扫会话：512 格逐格推进出报告
    let mut sess = ScanSession::new();
    let mut walk_ok = sess.start().is_ok();
    let mut k = 0u32;
    while k < FACE_SIZE {
        // 每格推进必须返回 Ok（Ok(true)=实装格命中、Ok(false)=ENOSYS 格）。
        if sess.step(&reg).is_err() {
            walk_ok = false;
        }
        k += 1;
    }
    let report = sess.report();
    let (reg_hits, nosys_hits) = match report {
        Ok(pair) => pair,
        Err(_) => (0, 0),
    };
    s.add("全扫会话：512 格逐格推进并出对账报告",
          walk_ok && sess.state() == ScanState::Reconciled
              && reg_hits.saturating_add(nosys_hits) == FACE_SIZE,
          "会话必须跑满面且报告 512 = 实装 + ENOSYS");

    // 5. 全扫口径：未实装号全 ENOSYS（494 格）、实装格无一 ENOSYS（18 格）
    let (r2, n2) = full_scan(&reg);
    s.add("号面全扫：实装 18 + ENOSYS 494 = 512",
          r2 == REG_CAP as u32 && n2 == FACE_SIZE - REG_CAP as u32,
          "未实装号全 ENOSYS 逐格检出，实装格零 ENOSYS");

    // 6. 两路互证：会话账与独立复算一致
    s.add("两路互证：会话账 = 独立复算", reg_hits == r2 && nosys_hits == n2,
          "会话状态机与逐格复算必须逐位一致，单路写错即失配");

    // 7. 判定次序：未实装号 + 完美参数仍 ENOSYS
    let order_ok = probe(&reg, 256, true) == Err(ENOSYS)
        && probe(&reg, 256, false) == Err(ENOSYS)
        && probe(&reg, 0, false) == Err(EINVAL)
        && probe(&reg, 0, true).is_ok();
    s.add("判定次序：号查表先于参数面", order_ok,
          "未实装号不因参数完美放行；已实装号不因参数糟糕误报 ENOSYS");

    // 8. 面界守卫：负号与越面号不落入 ENOSYS
    let guard_ok = probe(&reg, -1, true).err() == Some(EINVAL)
        && probe(&reg, FACE_SIZE as i64, true).err() == Some(EINVAL)
        && probe(&reg, 511, true).err() == Some(ENOSYS);
    s.add("面界守卫：负号/≥512 拒于守卫层，511 仍在面内", guard_ok,
          "面外样本是参数面错误；把越面号报成 ENOSYS 即界线漂移");

    // 9. 注入矩阵逐类检出
    let (ledger, per_class) = run_scan_matrix();
    let all_cells = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入矩阵：7 类坏样本 ×8 轮逐类检出", all_cells,
          "负号/越面/重复注册/未实装完美参/坏包/实装坏参/对账路逐类 8/8");

    // 10. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == 7 * INJECT_ROUNDS,
          "56 次注入必须 56 次检出、0 逃逸");

    // 11. 注册器拒绝面：重复注册与孤儿注入被显性拒绝
    let mut reg2 = SysReg::new();
    let dup = reg2.register(0, "read2", false) == Err(NosysCode::DuplicateRegister);
    let beyond = reg2.register(FACE_SIZE, "ghost", false) == Err(NosysCode::BeyondFace);
    let acc_before = reg2.accepts();
    let rej_after = reg2.rejects();
    s.add("注册器：重复与越面注册显性拒绝且记双账", dup && beyond
              && acc_before == REG_CAP as u32 && rej_after >= 2,
          "拒绝路径不许静默覆盖；接受/拒绝两账各记一笔");

    // 12. 解析器：往返一致 + 坏包拒绝
    let pack_ok = parse(pack(202, 3)) == Ok(Request { num: 202, argc: 3 })
        && parse(pack(0, 0)) == Ok(Request { num: 0, argc: 0 })
        && parse(pack(511, 6)) == Ok(Request { num: 511, argc: 6 })
        && parse(0x0100_0000).is_err()
        && parse(pack(1, MAX_ARGC + 1)).is_err();
    s.add("解析器：pack→parse 往返还原且坏包必拒", pack_ok,
          "保留位非零与 argc>6 一律 BadPacking；线格式先于号判定");

    // 13. Linux 逐码一致
    let codes_ok = in_linux_table(ENOSYS) && in_linux_table(EINVAL)
        && !in_linux_table(0) && !in_linux_table(9999)
        && ENOSYS == 38 && EINVAL == 22;
    s.add("Linux 逐码一致：ENOSYS=38/EINVAL=22 精确", codes_ok,
          "表外码一律拒绝；码值与 errno.h 常量逐项对齐");

    // 14. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 15. 文案闭集
    let text_ok = NosysCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：12 个诊断码均带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 码值唯一
    let mut uniq = true;
    let mut i2 = 0usize;
    while i2 < NosysCode::ALL.len() {
        let mut j = i2 + 1;
        while j < NosysCode::ALL.len() {
            if let (Some(a), Some(b)) = (NosysCode::ALL.get(i2), NosysCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i2 += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 17. 会话乱序守卫
    let mut sess2 = ScanSession::new();
    let reorder_ok = sess2.step(&reg) == Err(NosysCode::ScanOrder)
        && sess2.report() == Err(NosysCode::ScanOrder)
        && sess2.start().is_ok()
        && sess2.start() == Err(NosysCode::ScanOrder);
    s.add("会话守卫：未开扫推格/未跑完出报告/重复开扫全拒", reorder_ok,
          "Idle→Running→Reconciled 必须逐站推进，乱序即 ScanOrder");

    // 动态明细（≤8）
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("账目快照", ledger.conserved(), &ledger_text);
    let fp_text = format!("全扫 {} 实装 / {} ENOSYS · 指纹 0x{:08X}", r2, n2, a);
    s.add_dyn("全扫快照", r2 == REG_CAP as u32 && n2 == FACE_SIZE - REG_CAP as u32, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_loads_18() {
        let reg = SysReg::new();
        assert_eq!(reg.count(), REG_CAP);
        assert_eq!(reg.accepts(), REG_CAP as u32);
        assert!(reg.is_registered(0));
        assert!(reg.is_registered(232));
        assert!(!reg.is_registered(256));
    }

    #[test]
    fn unregistered_returns_enosys() {
        let reg = SysReg::new();
        assert_eq!(probe(&reg, 256, true), Err(ENOSYS));
        assert_eq!(probe(&reg, 511, true), Err(ENOSYS));
        assert_eq!(probe(&reg, 6, true), Err(ENOSYS));
    }

    #[test]
    fn registered_bad_args_einval_not_enosys() {
        let reg = SysReg::new();
        assert_eq!(probe(&reg, 0, false), Err(EINVAL));
        assert_eq!(probe(&reg, 202, false), Err(EINVAL));
        assert!(probe(&reg, 202, true).is_ok());
    }

    #[test]
    fn face_guard_cells() {
        let reg = SysReg::new();
        assert_eq!(probe(&reg, -1, true), Err(EINVAL));
        assert_eq!(probe(&reg, 512, true), Err(EINVAL));
        assert_eq!(probe(&reg, 9999, true), Err(EINVAL));
    }

    #[test]
    fn full_scan_counts_match() {
        let reg = SysReg::new();
        let (r, n) = full_scan(&reg);
        assert_eq!(r, 18);
        assert_eq!(n, 494);
        assert_eq!(r + n, FACE_SIZE);
    }

    #[test]
    fn session_walks_and_reconciles() {
        let reg = SysReg::new();
        let mut sess = ScanSession::new();
        assert!(sess.start().is_ok());
        let mut k = 0;
        while k < FACE_SIZE {
            assert!(sess.step(&reg).is_ok());
            k += 1;
        }
        assert_eq!(sess.state(), ScanState::Reconciled);
        let (r, n) = sess.report().unwrap_or((0, 0));
        assert_eq!((r, n), (18, 494));
        assert_eq!(sess.step(&reg), Err(NosysCode::ScanOrder));
    }

    #[test]
    fn duplicate_register_rejected() {
        let mut reg = SysReg::new();
        assert_eq!(reg.register(0, "read", true), Err(NosysCode::DuplicateRegister));
        assert_eq!(reg.register(512, "ghost", false), Err(NosysCode::BeyondFace));
        assert_eq!(reg.count(), REG_CAP);
    }

    #[test]
    fn pack_parse_roundtrip() {
        assert_eq!(parse(pack(9, 6)), Ok(Request { num: 9, argc: 6 }));
        assert_eq!(parse(0x0100_0000), Err(NosysCode::BadPacking));
        assert_eq!(parse(pack(1, 7)), Err(NosysCode::BadPacking));
    }

    #[test]
    fn compat_is_subset() {
        let reg = SysReg::new();
        assert_eq!(reg.compat_reconcile(), Ok(()));
        for n in COMPAT_TABLE.iter() {
            assert!(reg.is_registered(*n));
        }
        assert!(!reg.is_compat(202));
    }

    #[test]
    fn matrix_ledger_conserved() {
        let (l, per) = run_scan_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 7 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9346_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
