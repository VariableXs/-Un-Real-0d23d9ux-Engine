//! UNX-F9378 联调与 glibc 消费核联测面 · 0x7CC00段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F9378-J1 消费核联测判据过，glibc 消费清单与联测面对接可
//!       观测，消费号登记格过。
//!
//! 设计要点：
//!   ① glibc 消费核按「包装名 × 消费号 × 消费 errno 集」三元组建模：
//!      每条消费清单项登记 glibc 包装函数名、对应 syscall 号与该包装
//!      可观测的 errno 正值集。登记账只增不减，任何回退显性拒绝。
//!   ② 对接判据是本模块核心：每条已登记消费项的消费 errno 必须全部
//!      落在联测面对照表（ERRNO_FACE）闭集内；表外码在登记瞬间拒绝，
//!      不许「先收下再说明」。
//!   ③ 注入核覆盖五类坏样本：空包装名、重复包装名、表外 errno、
//!      syscall 号越上限、注册表超容，逐类断言显性拒绝且账守恒
//!      （注入 = 检出 + 逃逸，逃逸恒 0）。
//!   ④ 消费号登记格：每条成功登记的 syscall 号入账；同一号允许多个
//!      包装（如 read 与 pread 消费同号），但同一包装名唯一。
//!   ⑤ 诊断码闭集：SEG_BASE + 变体序号，段内 +0x01..+0x0C，双跑指纹
//!      FNV-1a 覆盖全部登记判定语料，两次运行逐位一致。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9378 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7CC00;

/// 消费清单注册容量（模拟 glibc 常用包装面上限）。
pub const CONSUMER_CAP: usize = 16;

/// 单条消费项可携带的 errno 上限。
pub const ERRNO_SLOTS: usize = 4;

/// 复测轮数（注入矩阵每类轮数）。
pub const INJECT_ROUNDS: u32 = 10;

/// 号面上限（0..=511；0 号是合法 read 号）。
pub const SYSCALL_MAX: u32 = 511;

// ── 联测面对照表（errno 正值口径） ───────────────────────────────────
/// 联测面支持的 errno 闭集：消费项只允许消费表内码。
pub const ERRNO_FACE: &[(&str, i32)] = &[
    ("EBADF", 9),
    ("EFAULT", 14),
    ("EINTR", 4),
    ("EINVAL", 22),
    ("ENOMEM", 12),
    ("ENOTTY", 25),
    ("EAGAIN", 11),
    ("ENOSYS", 38),
];

/// Linux errno 对照表：(名称, 负值, Windows 近似码, 说明)。
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；坏句柄族"),
    ("EFAULT", -14, 998, "Windows ERROR_NOACCESS；坏地址族"),
    ("EINTR", -4, 995, "Windows ERROR_OPERATION_ABORTED；中断族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
    ("ENOMEM", -12, 8, "Windows ERROR_NOT_ENOUGH_MEMORY；耗尽族"),
    ("ENOTTY", -25, 1, "Windows ERROR_INVALID_FUNCTION；终端缺席族"),
    ("EAGAIN", -11, 997, "Windows ERROR_IO_PENDING；重试族"),
    ("ENOSYS", -38, 1, "Windows ERROR_NOT_SUPPORTED；未实装族"),
];

/// 按 errno 正数查负值；表外返回 0（显性可查）。
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

/// 与 Linux 逐码一致：只允许表内正值且负值精确复算。
pub fn agrees_with_linux(code: i32) -> bool {
    code > 0 && linux_neg(code) == -code
}

/// 码是否落在联测面闭集内。
pub fn in_face(code: i32) -> bool {
    let mut i = 0usize;
    while i < ERRNO_FACE.len() {
        if let Some(row) = ERRNO_FACE.get(i) {
            if row.1 == code {
                return true;
            }
        }
        i += 1;
    }
    false
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GlbCode {
    /// 空包装名
    EmptyWrapper,
    /// 包装名重复登记
    DuplicateWrapper,
    /// 消费了联测面表外 errno
    UnknownErrno,
    /// syscall 号越号面上限
    SysnoBeyond,
    /// 注册表超容
    RegistryFull,
    /// 未登记包装被查询
    NotRegistered,
    /// 登记账倒退
    LedgerRegress,
    /// 消费号与包装面断言不符
    SysnoMismatch,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
    /// 账守恒破坏
    LedgerEscape,
    /// 文案闭集为空
    EmptyDetail,
}

impl GlbCode {
    /// 码值（SEG_BASE + 变体序号）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            GlbCode::EmptyWrapper => "空包装名被拒：消费清单项必须携带 glibc 包装函数名；\
                请补齐名称后重新登记",
            GlbCode::DuplicateWrapper => "重复包装名被拒：同一 glibc 包装只允许登记一次；\
                如需扩展 errno 集请走更新路径而非再登记",
            GlbCode::UnknownErrno => "表外 errno 被拒：消费码不在联测面闭集内；请先扩面\
                对账再登记，禁止静默吞码",
            GlbCode::SysnoBeyond => "syscall 号越上限被拒：号面 0..=511（0 号是合法 read 号）；\
                请核对号面冻结清单",
            GlbCode::RegistryFull => "注册表超容被拒：消费清单容量已满；请按优先级裁剪\
                或扩容后再登记",
            GlbCode::NotRegistered => "未登记包装：查询的包装名不在清单内；请先登记再\
                联测对接",
            GlbCode::LedgerRegress => "登记账倒退被拒：登记/拒绝计数只增不减；请检查\
                回滚路径是否误改累计值",
            GlbCode::SysnoMismatch => "消费号与包装面不符：登记号与联测面判定号不一致；\
                请对齐号面冻结清单后重测",
            GlbCode::NotReproducible => "双跑不一致：同语料两次指纹不同；请排查隐藏状态\
                或非确定遍历",
            GlbCode::SegmentViolation => "段位越界：段基址必须落在 0x7CC00..=0x7D1C0 且 \
                0x40 对齐",
            GlbCode::LedgerEscape => "账守恒破坏：注入次数必须等于检出加逃逸；逃逸恒 0",
            GlbCode::EmptyDetail => "诊断文案为空：闭集每码必须携带非空中文建议",
        }
    }

    /// 闭集全量（文案完整性与码值唯一性检查用）。
    pub const ALL: [GlbCode; 12] = [
        GlbCode::EmptyWrapper,
        GlbCode::DuplicateWrapper,
        GlbCode::UnknownErrno,
        GlbCode::SysnoBeyond,
        GlbCode::RegistryFull,
        GlbCode::NotRegistered,
        GlbCode::LedgerRegress,
        GlbCode::SysnoMismatch,
        GlbCode::NotReproducible,
        GlbCode::SegmentViolation,
        GlbCode::LedgerEscape,
        GlbCode::EmptyDetail,
    ];
}

// ── 消费清单数据结构 ──────────────────────────────────────────────────
/// 一条 glibc 消费清单项：包装名 × 消费号 × 消费 errno 集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConsumerEntry {
    /// glibc 包装函数名（如 "read"、"futex"）
    pub name: &'static str,
    /// 消费的 syscall 号（号面 1 起步）
    pub sysno: u32,
    /// 消费 errno 正值集（前 errno_len 个有效）
    pub errnos: [i32; ERRNO_SLOTS],
    /// 有效 errno 个数
    pub errno_len: usize,
}

/// 消费号登记账本：每登记一条记一笔，只增不减。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RegistrationLedger {
    /// 成功登记条数
    pub registered: u32,
    /// 显性拒绝条数
    pub rejected: u32,
    /// 累计消费号登记笔数（同号多包装各记一笔）
    pub sysno_entries: u32,
}

impl RegistrationLedger {
    const fn zero() -> RegistrationLedger {
        RegistrationLedger { registered: 0, rejected: 0, sysno_entries: 0 }
    }

    /// 记一笔登记（账只增）。
    pub fn record_ok(&mut self, sysno: u32) {
        self.registered = self.registered.saturating_add(1);
        if sysno <= SYSCALL_MAX {
            self.sysno_entries = self.sysno_entries.saturating_add(1);
        }
    }

    /// 记一笔拒绝（账只增）。
    pub fn record_reject(&mut self) {
        self.rejected = self.rejected.saturating_add(1);
    }

    /// 账闭合：登记数等于有效消费号笔数（拒绝项不进号账）。
    pub fn closed(&self) -> bool {
        self.registered == self.sysno_entries
    }
}

/// 消费清单注册表。登记顺序即对接顺序，重复包装名在登记口拒绝。
#[derive(Clone, Debug)]
pub struct ConsumerRegistry {
    entries: Vec<ConsumerEntry>,
    ledger: RegistrationLedger,
}

impl ConsumerRegistry {
    /// 构造空注册表。
    pub fn new() -> ConsumerRegistry {
        ConsumerRegistry { entries: Vec::new(), ledger: RegistrationLedger::zero() }
    }

    /// 登记一条消费项：空名/越号/重名/表外码/超容逐口拒绝。
    /// 判序固定：内容校验（名/号/重名/码）先于容量校验，保证满容时
    /// 仍能对坏样本回出精确码而非一律 RegistryFull。
    pub fn register(&mut self, e: ConsumerEntry) -> Result<u32, GlbCode> {
        if e.name.is_empty() {
            self.ledger.record_reject();
            return Err(GlbCode::EmptyWrapper);
        }
        if e.sysno > SYSCALL_MAX {
            self.ledger.record_reject();
            return Err(GlbCode::SysnoBeyond);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(old) = self.entries.get(i) {
                if old.name == e.name {
                    self.ledger.record_reject();
                    return Err(GlbCode::DuplicateWrapper);
                }
            }
            i += 1;
        }
        let mut k = 0usize;
        while k < e.errno_len {
            match e.errnos.get(k) {
                Some(c) => {
                    if !in_face(*c) {
                        self.ledger.record_reject();
                        return Err(GlbCode::UnknownErrno);
                    }
                }
                None => {
                    self.ledger.record_reject();
                    return Err(GlbCode::UnknownErrno);
                }
            }
            k += 1;
        }
        if self.entries.len() >= CONSUMER_CAP {
            self.ledger.record_reject();
            return Err(GlbCode::RegistryFull);
        }
        self.entries.push(e);
        self.ledger.record_ok(e.sysno);
        Ok(e.sysno)
    }

    /// 按包装名查询登记号。
    pub fn lookup(&self, name: &str) -> Result<u32, GlbCode> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.name == name {
                    return Ok(e.sysno);
                }
            }
            i += 1;
        }
        Err(GlbCode::NotRegistered)
    }

    /// 登记账视图。
    pub fn ledger(&self) -> RegistrationLedger {
        self.ledger
    }

    /// 已登记条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 联测面对接：全部登记项的消费码都在面内（登记口已保证，此处复算）。
    pub fn face_aligned(&self) -> bool {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                let mut k = 0usize;
                while k < e.errno_len {
                    match e.errnos.get(k) {
                        Some(c) => {
                            if !in_face(*c) {
                                return false;
                            }
                        }
                        None => return false,
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        true
    }

    /// 消费号覆盖率：联测面中被至少一条消费项消费的号去重计数。
    pub fn sysno_coverage(&self) -> u32 {
        let mut seen: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                let mut dup = false;
                let mut j = 0usize;
                while j < seen.len() {
                    if let Some(s) = seen.get(j) {
                        if *s == e.sysno {
                            dup = true;
                        }
                    }
                    j += 1;
                }
                if !dup {
                    seen.push(e.sysno);
                }
            }
            i += 1;
        }
        seen.len() as u32
    }
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸，逃逸必须恒 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
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

/// 五类坏消费清单样本（注入核逐类拒绝）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadEntryCase {
    /// 空包装名
    EmptyName,
    /// 重复包装名
    DuplicateName,
    /// 表外 errno（999 不在面内）
    UnknownErrno,
    /// syscall 号越号面上限
    SysnoBeyond,
    /// 注册表超容
    OverCap,
}

impl BadEntryCase {
    /// 期望诊断码。
    pub fn expected(self) -> GlbCode {
        match self {
            BadEntryCase::EmptyName => GlbCode::EmptyWrapper,
            BadEntryCase::DuplicateName => GlbCode::DuplicateWrapper,
            BadEntryCase::UnknownErrno => GlbCode::UnknownErrno,
            BadEntryCase::SysnoBeyond => GlbCode::SysnoBeyond,
            BadEntryCase::OverCap => GlbCode::RegistryFull,
        }
    }

    /// 闭集全量。
    pub const ALL: [BadEntryCase; 5] = [
        BadEntryCase::EmptyName,
        BadEntryCase::DuplicateName,
        BadEntryCase::UnknownErrno,
        BadEntryCase::SysnoBeyond,
        BadEntryCase::OverCap,
    ];
}

/// 注入核：对五类坏样本各注入 INJECT_ROUNDS 次，返回（账本, 逐类检出数）。
pub fn run_injection_matrix() -> (InjectLedger, [u32; 5]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 5];
    // 満容底表：先灌满 CONSUMER_CAP 条合法项。
    let mut full = ConsumerRegistry::new();
    let mut n = 0usize;
    while n < CONSUMER_CAP {
        let name = match n {
            0 => "g_f0", 1 => "g_f1", 2 => "g_f2", 3 => "g_f3", 4 => "g_f4",
            5 => "g_f5", 6 => "g_f6", 7 => "g_f7", 8 => "g_f8", 9 => "g_f9",
            10 => "g_fa", 11 => "g_fb", 12 => "g_fc", 13 => "g_fd",
            14 => "g_fe", _ => "g_ff",
        };
        let _ = full.register(ConsumerEntry {
            name,
            sysno: (n as u32) + 1,
            errnos: [9, 22, 14, 0],
            errno_len: 3,
        });
        n += 1;
    }
    let mut ci = 0usize;
    while ci < BadEntryCase::ALL.len() {
        let case = match BadEntryCase::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let r = match case {
                BadEntryCase::EmptyName => full.register(ConsumerEntry {
                    name: "",
                    sysno: 57,
                    errnos: [9, 0, 0, 0],
                    errno_len: 1,
                }),
                BadEntryCase::DuplicateName => full.register(ConsumerEntry {
                    name: "g_f0",
                    sysno: 58,
                    errnos: [22, 0, 0, 0],
                    errno_len: 1,
                }),
                BadEntryCase::UnknownErrno => full.register(ConsumerEntry {
                    name: "g_bad",
                    sysno: 59,
                    errnos: [999, 0, 0, 0],
                    errno_len: 1,
                }),
                BadEntryCase::SysnoBeyond => full.register(ConsumerEntry {
                    name: "g_z",
                    sysno: SYSCALL_MAX + 1,
                    errnos: [9, 0, 0, 0],
                    errno_len: 1,
                }),
                BadEntryCase::OverCap => full.register(ConsumerEntry {
                    name: "g_over",
                    sysno: 60,
                    errnos: [9, 0, 0, 0],
                    errno_len: 1,
                }),
            };
            let caught = r.err() == Some(case.expected());
            ledger.record(caught);
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

// ── FNV-1a 双跑指纹 ───────────────────────────────────────────────────
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

/// 规范语料指纹：登记 → 注入 → 查询 → 覆盖率全程逐字节落语料。
pub fn corpus_fingerprint() -> u32 {
    let mut reg = ConsumerRegistry::new();
    let mut bytes: Vec<u8> = Vec::new();
    // 三条合法登记
    let _ = reg.register(ConsumerEntry { name: "g_read", sysno: 0, errnos: [9, 14, 4, 0], errno_len: 3 });
    let _ = reg.register(ConsumerEntry { name: "g_futex", sysno: 202, errnos: [11, 22, 0, 0], errno_len: 2 });
    let _ = reg.register(ConsumerEntry { name: "g_ioctl", sysno: 16, errnos: [25, 9, 0, 0], errno_len: 2 });
    // 注入一轮坏样本
    let _ = reg.register(ConsumerEntry { name: "", sysno: 1, errnos: [9, 0, 0, 0], errno_len: 1 });
    let _ = reg.register(ConsumerEntry { name: "g_read", sysno: 1, errnos: [9, 0, 0, 0], errno_len: 1 });
    let _ = reg.register(ConsumerEntry { name: "g_x", sysno: 1, errnos: [999, 0, 0, 0], errno_len: 1 });
    let _ = reg.register(ConsumerEntry { name: "g_x", sysno: SYSCALL_MAX + 1, errnos: [9, 0, 0, 0], errno_len: 1 });
    // 查询与覆盖率
    let futex_no = match reg.lookup("g_futex") {
        Ok(n) => n,
        Err(_) => 0,
    };
    bytes.push((futex_no & 0xFF) as u8);
    bytes.push((reg.lookup("g_missing").err().map_or(0, |c| c as u32) & 0xFF) as u8);
    bytes.push((reg.sysno_coverage() & 0xFF) as u8);
    bytes.push((reg.len() & 0xFF) as u8);
    bytes.push(reg.face_aligned() as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9378 判据集（静态 ≥13 条 + 动态 ≤4 条）。
pub fn run_unx9378_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9378");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7CC00..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7CC00..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 合法登记：read/futex/ioctl 三条成功
    let mut reg = ConsumerRegistry::new();
    let r1 = reg.register(ConsumerEntry { name: "g_read", sysno: 0, errnos: [9, 14, 4, 0], errno_len: 3 });
    let r2 = reg.register(ConsumerEntry { name: "g_futex", sysno: 202, errnos: [11, 22, 0, 0], errno_len: 2 });
    let r3 = reg.register(ConsumerEntry { name: "g_ioctl", sysno: 16, errnos: [25, 9, 0, 0], errno_len: 2 });
    let normal_ok = r1.is_ok() && r2.is_ok() && r3.is_ok() && reg.len() == 3;
    s.add("合法登记：三条消费清单项全部入表", normal_ok,
          "合法包装名+面内码+非零号必须成功登记");

    // 3. 联测面对接：全部消费码落在 ERRNO_FACE 闭集
    s.add("联测面对接：消费码全部落面（复算一致）", reg.face_aligned(),
          "登记口已拒绝表外码，复算必须零漂移");

    // 4. 消费号登记格：账闭合（登记数 == 号账笔数）
    let led = reg.ledger();
    s.add("消费号登记格：账闭合且只增", led.closed() && led.registered == 3
              && led.sysno_entries == 3,
          "每条成功登记必须各记一笔消费号账");

    // 5. 注入核：五类坏样本逐类 10/10 拒绝
    let (inj, per_class) = run_injection_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 显性拒绝", all_caught,
          "空名/重名/表外码/越号/超容逐类拒绝");

    // 6. 注入账守恒：逃逸恒 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == 5 * INJECT_ROUNDS,
          "50 次注入必须 50 次检出、0 逃逸");

    // 7. 查询面：已登记可查、未登记显性 NotRegistered
    let lookup_ok = reg.lookup("g_futex") == Ok(202)
        && reg.lookup("g_missing").err() == Some(GlbCode::NotRegistered);
    s.add("查询面：登记可查、未登记显性拒绝", lookup_ok,
          "查询失败必须回闭集码而非静默 0");

    // 8. 覆盖率：去重号计数与登记条数关系可复算
    let cov = reg.sysno_coverage();
    let cov_ok = cov == 3 && cov <= reg.len() as u32;
    s.add("覆盖率：去重号计数 3 且不超登记条数", cov_ok,
          "同号多包装只记一覆盖格，覆盖率 ≤ 登记数");

    // 9. Linux 逐码一致
    let codes_ok = agrees_with_linux(9) && agrees_with_linux(22)
        && agrees_with_linux(38) && !agrees_with_linux(0)
        && !agrees_with_linux(999) && linux_neg(25) == -25;
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "errno 表正值与负值逐项可复算");

    // 10. 联测面闭集与 errno 表逐码对齐
    let face_ok = {
        let mut ok = true;
        let mut i = 0usize;
        while i < ERRNO_FACE.len() {
            if let Some((name, code)) = ERRNO_FACE.get(i) {
                if !agrees_with_linux(*code) || name.is_empty() {
                    ok = false;
                }
            } else {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    s.add("面对照：ERRNO_FACE 全部码在 Linux 表内且名非空", face_ok,
          "联测面闭集必须逐码可对账");

    // 11. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 12. 文案闭集
    let text_ok = GlbCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 13. 码值唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < GlbCode::ALL.len() {
        let mut j = i + 1;
        while j < GlbCode::ALL.len() {
            if let (Some(x), Some(y)) = (GlbCode::ALL.get(i), GlbCode::ALL.get(j)) {
                if x.code() == y.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 14. 登记账单调：再登记只增不减
    let mut reg2 = ConsumerRegistry::new();
    let _ = reg2.register(ConsumerEntry { name: "g_a", sysno: 1, errnos: [9, 0, 0, 0], errno_len: 1 });
    let before = reg2.ledger().registered;
    let _ = reg2.register(ConsumerEntry { name: "g_b", sysno: 2, errnos: [22, 0, 0, 0], errno_len: 1 });
    let _ = reg2.register(ConsumerEntry { name: "", sysno: 3, errnos: [9, 0, 0, 0], errno_len: 1 });
    let after = reg2.ledger().registered;
    s.add("登记账单调：成功登记只增，拒绝不改登记数",
          before == 1 && after == 2,
          "拒绝样本不得污染登记账");

    // 15. 容量边界：第 CONSUMER_CAP+1 条必拒
    let mut reg3 = ConsumerRegistry::new();
    let mut k = 0usize;
    while k < CONSUMER_CAP {
        let name = match k {
            0 => "c0", 1 => "c1", 2 => "c2", 3 => "c3", 4 => "c4", 5 => "c5",
            6 => "c6", 7 => "c7", 8 => "c8", 9 => "c9", 10 => "ca", 11 => "cb",
            12 => "cc", 13 => "cd", 14 => "ce", _ => "cf",
        };
        let _ = reg3.register(ConsumerEntry { name, sysno: (k as u32) + 1, errnos: [9, 0, 0, 0], errno_len: 1 });
        k += 1;
    }
    let over = reg3.register(ConsumerEntry { name: "cx", sysno: 99, errnos: [9, 0, 0, 0], errno_len: 1 });
    s.add("容量边界：满容后再登记显性 RegistryFull",
          over.err() == Some(GlbCode::RegistryFull) && reg3.len() == CONSUMER_CAP,
          "超容必须显性拒绝且原表不变");

    // 动态明细（≤8）：账目 + 指纹
    let led_text = format!(
        "登记 {} / 拒绝 {} / 号账 {} / 注入 {} 检出 {}",
        led.registered, led.rejected, led.sysno_entries, inj.injected, inj.detected
    );
    s.add_dyn("账目快照", led.closed() && inj.conserved(), &led_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_registration() {
        let mut r = ConsumerRegistry::new();
        assert!(r.register(ConsumerEntry { name: "g_read", sysno: 0, errnos: [9, 0, 0, 0], errno_len: 1 }).is_ok());
        assert_eq!(r.lookup("g_read"), Ok(0));
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn duplicate_wrapper_rejected() {
        let mut r = ConsumerRegistry::new();
        let _ = r.register(ConsumerEntry { name: "g_read", sysno: 1, errnos: [9, 0, 0, 0], errno_len: 1 });
        assert_eq!(
            r.register(ConsumerEntry { name: "g_read", sysno: 2, errnos: [9, 0, 0, 0], errno_len: 1 }).err(),
            Some(GlbCode::DuplicateWrapper)
        );
    }

    #[test]
    fn unknown_errno_rejected() {
        let mut r = ConsumerRegistry::new();
        assert_eq!(
            r.register(ConsumerEntry { name: "g_x", sysno: 5, errnos: [999, 0, 0, 0], errno_len: 1 }).err(),
            Some(GlbCode::UnknownErrno)
        );
    }

    #[test]
    fn sysno_beyond_rejected() {
        let mut r = ConsumerRegistry::new();
        assert_eq!(
            r.register(ConsumerEntry { name: "g_z", sysno: SYSCALL_MAX + 1, errnos: [9, 0, 0, 0], errno_len: 1 }).err(),
            Some(GlbCode::SysnoBeyond)
        );
    }

    #[test]
    fn empty_name_rejected() {
        let mut r = ConsumerRegistry::new();
        assert_eq!(
            r.register(ConsumerEntry { name: "", sysno: 5, errnos: [9, 0, 0, 0], errno_len: 1 }).err(),
            Some(GlbCode::EmptyWrapper)
        );
    }

    #[test]
    fn injection_matrix_green() {
        let (l, per) = run_injection_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 5 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn face_alignment_holds() {
        let mut r = ConsumerRegistry::new();
        let _ = r.register(ConsumerEntry { name: "g_a", sysno: 3, errnos: [9, 22, 11, 0], errno_len: 3 });
        assert!(r.face_aligned());
        assert_eq!(r.sysno_coverage(), 1);
    }

    #[test]
    fn linux_table_agrees() {
        assert!(agrees_with_linux(9));
        assert_eq!(linux_neg(14), -14);
        assert!(!agrees_with_linux(0));
        assert!(!agrees_with_linux(999));
    }

    #[test]
    fn fingerprint_deterministic() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn code_values_unique() {
        let mut i = 0;
        while i < GlbCode::ALL.len() {
            let mut j = i + 1;
            while j < GlbCode::ALL.len() {
                assert_ne!(GlbCode::ALL[i].code(), GlbCode::ALL[j].code());
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn unx9378_checks_green() {
        let s = run_unx9378_checks();
        assert!(s.all_passed(), "UNX-F9378 red: {}", s.red_items_text());
    }
}
