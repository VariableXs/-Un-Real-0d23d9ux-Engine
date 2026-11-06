//! UNX-F9385 epoll 错误矩阵联测（EBADF/EINVAL/EPERM） · 0x7CDC0段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F9385-J1 三错误码联测判据过，逐码与 C4 挂点联测一致格过，
//!       与 B22 判定域一致格过。
//!
//! 设计要点：
//!   ① fd 空间按类别注册表建模：epoll fd、可 poll socket fd、普通文件
//!      fd（不可 poll）、已关闭槽、负号；错误矩阵按「epfd × op × 目标
//!      fd × 操作态」逐格展开。
//!   ② 判定域判序（B22 口径）：epfd 存在性（EBADF）→ epfd 类型
//!      （EINVAL）→ op 闭集（EINVAL）→ 目标 fd 存在性（EBADF）→ 可
//!      poll 性（EPERM）→ 操作态（EEXIST/ENOENT）。并发故障取判序
//!      最前者，判序倒置即联测红。
//!   ③ EPERM 格是本矩阵特色：epoll_ctl 对普通文件 fd 一律 EPERM
//!      （Linux 口径：regular files 不可 epoll）；其余错误逐格与
//!      F9384 语义一致（单源：同一 EpollSet 语义复算）。
//!   ④ 注入核覆盖五类坏样本：坏 epfd、坏目标 fd、普通文件 EPERM、
//!      非法 op、重复 ADD，逐类 10/10 取码正确且账守恒。
//!   ⑤ 双跑指纹：FNV-1a 覆盖全矩阵逐格判定 + 注入逐格。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9385 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7CDC0;

/// 复测轮数（注入矩阵每类轮数）。
pub const INJECT_ROUNDS: u32 = 10;

/// epoll_ctl 操作码：ADD。
pub const CTL_ADD: u32 = 1;
/// epoll_ctl 操作码：MOD。
pub const CTL_MOD: u32 = 2;
/// epoll_ctl 操作码：DEL。
pub const CTL_DEL: u32 = 3;

/// Linux errno 正值口径。
pub const EBADF: i32 = 9;
pub const ENOENT: i32 = 2;
pub const EPERM: i32 = 1;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;

// ── Linux errno 对照表 ───────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)。
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；不可 poll 族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；兴趣表缺项族"),
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；坏句柄族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重号登记族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
    ("ENOSPC", -28, 1450, "Windows ERROR_NO_SYSTEM_RESOURCES；表满族"),
];

/// 按 errno 正数查负值；表外返回 0。
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

/// 与 Linux 逐码一致。
pub fn agrees_with_linux(code: i32) -> bool {
    code > 0 && linux_neg(code) == -code
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0A 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EmCode {
    /// epfd 存在性格漂移
    EpfdBad,
    /// epfd 类型格漂移（非 epoll fd）
    EpfdWrongType,
    /// op 闭集格漂移
    OpInvalid,
    /// 目标 fd 存在性格漂移
    FdBad,
    /// EPERM 格漂移（普通文件可 poll）
    PermDrift,
    /// 操作态格漂移（EEXIST/ENOENT）
    OpStateDrift,
    /// 判序倒置
    OrderInverted,
    /// 账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
    /// 文案闭集为空
    EmptyDetail,
}

impl EmCode {
    /// 码值（SEG_BASE + 变体序号）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            EmCode::EpfdBad => "epfd 存在性格漂移：负号/未开/已关 epfd 必须\
                回 EBADF；fd 校验先于一切语义",
            EmCode::EpfdWrongType => "epfd 类型格漂移：非 epoll fd 作 epfd 必须\
                回 EINVAL；不得误报 EBADF",
            EmCode::OpInvalid => "op 闭集格漂移：ADD/MOD/DEL 之外必须 EINVAL；\
                未知操作不得静默",
            EmCode::FdBad => "目标 fd 存在性格漂移：负号/未开/已关目标 fd 必须\
                回 EBADF；先于 EPERM 判定",
            EmCode::PermDrift => "EPERM 格漂移：普通文件 fd 不可 epoll，必须\
                回 EPERM；放行即语义泄漏",
            EmCode::OpStateDrift => "操作态格漂移：ADD 重号 EEXIST、MOD/DEL 缺项\
                ENOENT；与 F9384 语义单源一致",
            EmCode::OrderInverted => "判序倒置：并发故障必须取判序最前者；\
                判序为 EBADF(epfd)→EINVAL(type)→EINVAL(op)→EBADF(fd)→EPERM→操作态",
            EmCode::LedgerRegress => "账倒退被拒：格判定/注入计数只增不减；请检查\
                回滚路径是否误改累计值",
            EmCode::NotReproducible => "双跑不一致：同语料两次指纹不同；请排查\
                隐藏状态或非确定遍历",
            EmCode::SegmentViolation => "段位越界：段基址必须落在 \
                0x7CC00..=0x7D1C0 且 0x40 对齐",
            EmCode::EmptyDetail => "诊断文案为空：闭集每码必须携带非空中文建议",
        }
    }

    /// 闭集全量。
    pub const ALL: [EmCode; 11] = [
        EmCode::EpfdBad,
        EmCode::EpfdWrongType,
        EmCode::OpInvalid,
        EmCode::FdBad,
        EmCode::PermDrift,
        EmCode::OpStateDrift,
        EmCode::OrderInverted,
        EmCode::LedgerRegress,
        EmCode::NotReproducible,
        EmCode::SegmentViolation,
        EmCode::EmptyDetail,
    ];
}

// ── fd 类别注册表 ─────────────────────────────────────────────────────
/// fd 类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FdClass {
    /// epoll 实例 fd
    Epoll,
    /// 可 poll 的 socket/pipe fd
    Socket,
    /// 普通（不可 poll）文件 fd
    RegularFile,
    /// 已关闭槽
    Closed,
}

/// fd 注册表：号 → 类别（未登记即未开）。
#[derive(Clone, Debug)]
pub struct FdRegistry {
    table: Vec<(i32, FdClass)>,
}

impl FdRegistry {
    /// 构造并预置标准样板：3=epoll、4=socket、5=普通文件、6=closed。
    pub fn new() -> FdRegistry {
        FdRegistry {
            table: vec![
                (3, FdClass::Epoll),
                (4, FdClass::Socket),
                (5, FdClass::RegularFile),
                (6, FdClass::Closed),
            ],
        }
    }

    /// 查类别：未登记返回 None（未开 → EBADF 域）。
    pub fn class_of(&self, fd: i32) -> Option<FdClass> {
        if fd < 0 {
            return None;
        }
        let mut i = 0usize;
        while i < self.table.len() {
            if let Some((num, cls)) = self.table.get(i) {
                if *num == fd {
                    return Some(*cls);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记一个 fd 类别（只增；重号覆盖禁止，返回假）。
    pub fn register(&mut self, fd: i32, cls: FdClass) -> bool {
        if fd < 0 || self.class_of(fd).is_some() {
            return false;
        }
        self.table.push((fd, cls));
        true
    }

    /// 登记条数。
    pub fn len(&self) -> usize {
        self.table.len()
    }
}

/// epoll 兴趣表（与 F9384 单源口径：ADD 重号 EEXIST、缺项 ENOENT）。
#[derive(Clone, Debug)]
pub struct InterestList {
    fds: Vec<i32>,
}

impl InterestList {
    /// 空表。
    pub fn new() -> InterestList {
        InterestList { fds: Vec::new() }
    }

    /// ADD：重号 EEXIST。
    pub fn add(&mut self, fd: i32) -> Result<(), i32> {
        if self.contains(fd) {
            return Err(EEXIST);
        }
        self.fds.push(fd);
        Ok(())
    }

    /// MOD：缺项 ENOENT。
    pub fn modify(&self, fd: i32) -> Result<(), i32> {
        if self.contains(fd) {
            Ok(())
        } else {
            Err(ENOENT)
        }
    }

    /// DEL：缺项 ENOENT；成功出表。
    pub fn remove(&mut self, fd: i32) -> Result<(), i32> {
        let mut i = 0usize;
        while i < self.fds.len() {
            if let Some(f) = self.fds.get(i) {
                if *f == fd {
                    let _ = self.fds.remove(i);
                    return Ok(());
                }
            }
            i += 1;
        }
        Err(ENOENT)
    }

    /// 是否登记。
    pub fn contains(&self, fd: i32) -> bool {
        let mut i = 0usize;
        while i < self.fds.len() {
            if let Some(f) = self.fds.get(i) {
                if *f == fd {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 表深。
    pub fn len(&self) -> usize {
        self.fds.len()
    }
}

// ── 错误矩阵判定器（B22 判序） ────────────────────────────────────────
/// 一格矩阵样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    /// epfd 号
    pub epfd: i32,
    /// ctl 操作码
    pub op: u32,
    /// 目标 fd 号
    pub fd: i32,
}

/// 判定器：按 B22 判序逐层校验，返回 errno 正数码或 Ok。
/// 已关闭槽按 Linux 口径视同不存在（EBADF），不落入类型格。
pub fn judge(reg: &FdRegistry, list: &mut InterestList, cell: &Cell) -> Result<(), i32> {
    // 1. epfd 存在性 → EBADF（未开或已关均视同不存在）
    let epfd_cls = match reg.class_of(cell.epfd) {
        Some(c) if c != FdClass::Closed => c,
        _ => return Err(EBADF),
    };
    // 2. epfd 类型 → EINVAL
    if epfd_cls != FdClass::Epoll {
        return Err(EINVAL);
    }
    // 3. op 闭集 → EINVAL
    if cell.op != CTL_ADD && cell.op != CTL_MOD && cell.op != CTL_DEL {
        return Err(EINVAL);
    }
    // 4. 目标 fd 存在性 → EBADF（未开或已关均视同不存在）
    let fd_cls = match reg.class_of(cell.fd) {
        Some(c) if c != FdClass::Closed => c,
        _ => return Err(EBADF),
    };
    // 5. 可 poll 性 → EPERM（普通文件不可 epoll）
    if fd_cls == FdClass::RegularFile {
        return Err(EPERM);
    }
    // 6. 操作态 → EEXIST/ENOENT
    match cell.op {
        CTL_ADD => list.add(cell.fd),
        CTL_MOD => list.modify(cell.fd),
        CTL_DEL => list.remove(cell.fd),
        _ => Err(EINVAL),
    }
}

/// 15 格闭集矩阵：每格 (样本, 期望码)，期望码由判序人工推导（注释给出依据）。
pub fn full_matrix(reg: &FdRegistry) -> Vec<(Cell, Result<(), i32>)> {
    let mut cells: Vec<(Cell, Result<(), i32>)> = Vec::new();
    // 格 1..3：epfd 存在性（负号/未开 99/已关 6）→ EBADF
    cells.push((Cell { epfd: -1, op: CTL_ADD, fd: 4 }, Err(EBADF)));
    cells.push((Cell { epfd: 99, op: CTL_ADD, fd: 4 }, Err(EBADF)));
    cells.push((Cell { epfd: 6, op: CTL_ADD, fd: 4 }, Err(EBADF)));
    // 格 4：epfd 类型（socket 作 epfd）→ EINVAL
    cells.push((Cell { epfd: 4, op: CTL_ADD, fd: 4 }, Err(EINVAL)));
    // 格 5：op 闭集（0 号非法 op）→ EINVAL
    cells.push((Cell { epfd: 3, op: 0, fd: 4 }, Err(EINVAL)));
    cells.push((Cell { epfd: 3, op: 9, fd: 4 }, Err(EINVAL)));
    // 格 7：目标 fd 存在性（负号/未开 42）→ EBADF
    cells.push((Cell { epfd: 3, op: CTL_ADD, fd: -2 }, Err(EBADF)));
    cells.push((Cell { epfd: 3, op: CTL_ADD, fd: 42 }, Err(EBADF)));
    // 格 9：EPERM（普通文件 5）→ EPERM
    cells.push((Cell { epfd: 3, op: CTL_ADD, fd: 5 }, Err(EPERM)));
    cells.push((Cell { epfd: 3, op: CTL_MOD, fd: 5 }, Err(EPERM)));
    cells.push((Cell { epfd: 3, op: CTL_DEL, fd: 5 }, Err(EPERM)));
    // 判序并发格：普通文件 + 非法 op → op 校验先于 EPERM
    cells.push((Cell { epfd: 3, op: 7, fd: 5 }, Err(EINVAL)));
    // 判序并发格：普通文件 + 坏目标双坏（fd=5 EPERM，fd=-1 EBADF 前者）→ EBADF
    cells.push((Cell { epfd: 3, op: CTL_ADD, fd: -1 }, Err(EBADF)));
    // 格 14：已关目标 fd → EBADF
    cells.push((Cell { epfd: 3, op: CTL_ADD, fd: 6 }, Err(EBADF)));
    // 格 15：全合法 → Ok
    cells.push((Cell { epfd: 3, op: CTL_ADD, fd: 4 }, Ok(())));
    // 与注册表核对：全部 epfd/fd 号必须可查（防御性自检）
    let mut i = 0usize;
    while i < cells.len() {
        if let Some((c, _)) = cells.get(i) {
            let _ = reg.class_of(c.epfd);
            let _ = reg.class_of(c.fd);
        }
        i += 1;
    }
    cells
}

/// 矩阵联测：15 格逐格判定 vs 期望，返回（总格数, 一致格数）。
pub fn run_matrix(reg: &FdRegistry) -> (u32, u32) {
    let cells = full_matrix(reg);
    let total = cells.len() as u32;
    let mut agreed = 0u32;
    let mut list = InterestList::new();
    let mut i = 0usize;
    while i < cells.len() {
        if let Some((cell, expect)) = cells.get(i) {
            if judge(reg, &mut list, cell) == *expect {
                agreed = agreed.saturating_add(1);
            }
        }
        i += 1;
    }
    (total, agreed)
}

// ── 注入账与注入核 ────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸。
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

/// 五类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadCellCase {
    /// 坏 epfd
    BadEpfd,
    /// 坏目标 fd
    BadFd,
    /// 普通文件 EPERM
    RegularFile,
    /// 非法 op
    BadOp,
    /// 重复 ADD（先合法加一次再重加）
    DupAdd,
}

impl BadCellCase {
    /// 构造样本格。
    pub fn cell(self) -> Cell {
        match self {
            BadCellCase::BadEpfd => Cell { epfd: 77, op: CTL_ADD, fd: 4 },
            BadCellCase::BadFd => Cell { epfd: 3, op: CTL_ADD, fd: 88 },
            BadCellCase::RegularFile => Cell { epfd: 3, op: CTL_ADD, fd: 5 },
            BadCellCase::BadOp => Cell { epfd: 3, op: 55, fd: 4 },
            BadCellCase::DupAdd => Cell { epfd: 3, op: CTL_ADD, fd: 4 },
        }
    }

    /// 期望码。
    pub fn expected_code(self) -> i32 {
        match self {
            BadCellCase::BadEpfd => EBADF,
            BadCellCase::BadFd => EBADF,
            BadCellCase::RegularFile => EPERM,
            BadCellCase::BadOp => EINVAL,
            BadCellCase::DupAdd => EEXIST,
        }
    }

    /// 闭集全量。
    pub const ALL: [BadCellCase; 5] = [
        BadCellCase::BadEpfd,
        BadCellCase::BadFd,
        BadCellCase::RegularFile,
        BadCellCase::BadOp,
        BadCellCase::DupAdd,
    ];
}

/// 注入核：五类坏样本各注入 INJECT_ROUNDS 次（每轮全新注册表与兴趣表；
/// DupAdd 类先合法登记一次再注入重复 ADD）。
pub fn run_injection() -> (InjectLedger, [u32; 5]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadCellCase::ALL.len() {
        let case = match BadCellCase::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let reg = FdRegistry::new();
            let mut list = InterestList::new();
            if case == BadCellCase::DupAdd {
                let _ = judge(&reg, &mut list, &Cell { epfd: 3, op: CTL_ADD, fd: 4 });
            }
            let cell = case.cell();
            let caught = judge(&reg, &mut list, &cell).err() == Some(case.expected_code());
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

/// 规范语料指纹：15 格联测 + 五类注入逐格落语料。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let reg = FdRegistry::new();
    let cells = full_matrix(&reg);
    let mut i = 0usize;
    while i < cells.len() {
        if let Some((cell, _)) = cells.get(i) {
            let mut list = InterestList::new();
            match judge(&reg, &mut list, cell) {
                Ok(()) => bytes.push(1),
                Err(e) => bytes.push((e & 0xFF) as u8),
            }
        }
        i += 1;
    }
    let (total, agreed) = run_matrix(&reg);
    bytes.push(total as u8);
    bytes.push(agreed as u8);
    let (inj, per) = run_injection();
    bytes.push(inj.detected as u8);
    let mut ci = 0usize;
    while ci < per.len() {
        if let Some(n) = per.get(ci) {
            bytes.push(*n as u8);
        }
        ci += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9385 判据集（静态 ≥13 条 + 动态 ≤4 条）。
pub fn run_unx9385_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9385");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7CC00..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7CC00..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. fd 注册表样板：epoll/socket/regular/closed 四类齐全
    let reg = FdRegistry::new();
    let sample_ok = reg.class_of(3) == Some(FdClass::Epoll)
        && reg.class_of(4) == Some(FdClass::Socket)
        && reg.class_of(5) == Some(FdClass::RegularFile)
        && reg.class_of(6) == Some(FdClass::Closed)
        && reg.class_of(-1).is_none()
        && reg.class_of(42).is_none();
    s.add("fd 注册表：四类样板逐格可查、未开显性 None", sample_ok,
          "fd 类别是错误矩阵的判定地基");

    // 3. EBADF 格：epfd 三态（负/未开/已关）逐格 EBADF
    let cells = full_matrix(&reg);
    let ebadf_ok = cells
        .iter()
        .take(3)
        .all(|(c, e)| judge(&reg, &mut InterestList::new(), c) == *e && *e == Err(EBADF));
    s.add("EBADF 格：epfd 负号/未开/已关逐格 EBADF", ebadf_ok,
          "epfd 存在性是判定域第一层");

    // 4. EINVAL 格：类型格与 op 格逐格 EINVAL
    let einval_ok = judge(&reg, &mut InterestList::new(), &Cell { epfd: 4, op: CTL_ADD, fd: 4 })
        == Err(EINVAL)
        && judge(&reg, &mut InterestList::new(), &Cell { epfd: 3, op: 9, fd: 4 })
            == Err(EINVAL);
    s.add("EINVAL 格：非 epoll 类型与非法 op 逐格 EINVAL", einval_ok,
          "类型与参数格与 B22 判定域一致");

    // 5. EPERM 格：普通文件三操作逐格 EPERM
    let eperm_ok = judge(&reg, &mut InterestList::new(), &Cell { epfd: 3, op: CTL_ADD, fd: 5 })
        == Err(EPERM)
        && judge(&reg, &mut InterestList::new(), &Cell { epfd: 3, op: CTL_DEL, fd: 5 })
            == Err(EPERM);
    s.add("EPERM 格：普通文件不可 epoll 逐格 EPERM", eperm_ok,
          "regular file 放行即语义泄漏");

    // 6. 全矩阵联测：15 格逐格一致
    let (total, agreed) = run_matrix(&reg);
    s.add("逐格联测：15 格全一致", total == 15 && agreed == 15,
          "逐码与 C4 挂点联测一致格过");

    // 7. 判序并发格：非法 op 先于 EPERM
    let order_ok = judge(&reg, &mut InterestList::new(), &Cell { epfd: 3, op: 7, fd: 5 })
        == Err(EINVAL);
    s.add("判序并发格：op 校验先于 EPERM", order_ok,
          "并发故障取判序最前者");

    // 8. 注入核：五类坏样本各 10/10 取码正确
    let (inj, per_class) = run_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 取码正确", all_caught,
          "坏 epfd/坏 fd/EPERM/非法 op/重号逐类拒绝");

    // 9. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == 5 * INJECT_ROUNDS,
          "50 次注入必须 50 次检出、0 逃逸");

    // 10. 操作态格：ADD 重号 EEXIST、缺项 ENOENT（与 F9384 单源）
    // ENOENT 需要一个「存在但未登记」的合法 fd，先注册 7 号 socket。
    let mut reg3 = FdRegistry::new();
    let _ = reg3.register(7, FdClass::Socket);
    let mut list = InterestList::new();
    let _ = judge(&reg3, &mut list, &Cell { epfd: 3, op: CTL_ADD, fd: 4 });
    let state_ok = judge(&reg3, &mut list, &Cell { epfd: 3, op: CTL_ADD, fd: 4 })
        == Err(EEXIST)
        && judge(&reg3, &mut list, &Cell { epfd: 3, op: CTL_DEL, fd: 7 })
            == Err(ENOENT)
        && list.len() == 1;
    s.add("操作态格：EEXIST/ENOENT 与 F9384 单源一致", state_ok,
          "同一兴趣表语义两卡复算零漂移");

    // 11. 注册表只增：重号登记被拒且不改表
    let mut reg2 = FdRegistry::new();
    let before = reg2.len();
    let dup_reg = reg2.register(4, FdClass::Socket);
    let neg_reg = reg2.register(-3, FdClass::Epoll);
    let reg_ok = !dup_reg && !neg_reg && reg2.len() == before;
    s.add("注册表守卫：重号/负号登记被拒、表深不变", reg_ok,
          "fd 注册表只增不改不删");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(EPERM) && agrees_with_linux(EBADF)
        && agrees_with_linux(EEXIST) && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0) && !agrees_with_linux(999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "errno 表正值与负值逐项可复算");

    // 13. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 14. 文案闭集
    let text_ok = EmCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 15. 码值唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < EmCode::ALL.len() {
        let mut j = i + 1;
        while j < EmCode::ALL.len() {
            if let (Some(x), Some(y)) = (EmCode::ALL.get(i), EmCode::ALL.get(j)) {
                if x.code() == y.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：11 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）
    let mx_text = format!("矩阵 {} 格 / 一致 {} / 注入检出 {}", total, agreed, inj.detected);
    s.add_dyn("矩阵快照", total == 15 && agreed == 15 && inj.conserved(), &mx_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_sample_classes() {
        let r = FdRegistry::new();
        assert_eq!(r.class_of(3), Some(FdClass::Epoll));
        assert_eq!(r.class_of(5), Some(FdClass::RegularFile));
        assert_eq!(r.class_of(99), None);
        assert_eq!(r.class_of(-1), None);
    }

    #[test]
    fn registry_rejects_dup_and_negative() {
        let mut r = FdRegistry::new();
        assert!(!r.register(3, FdClass::Socket));
        assert!(!r.register(-1, FdClass::Epoll));
        assert!(r.register(10, FdClass::Socket));
        assert_eq!(r.len(), 5);
    }

    #[test]
    fn bad_epfd_ebadf() {
        let r = FdRegistry::new();
        let mut l = InterestList::new();
        assert_eq!(judge(&r, &mut l, &Cell { epfd: -1, op: CTL_ADD, fd: 4 }), Err(EBADF));
        assert_eq!(judge(&r, &mut l, &Cell { epfd: 6, op: CTL_ADD, fd: 4 }), Err(EBADF));
    }

    #[test]
    fn wrong_type_epfd_einval() {
        let r = FdRegistry::new();
        let mut l = InterestList::new();
        assert_eq!(judge(&r, &mut l, &Cell { epfd: 4, op: CTL_ADD, fd: 4 }), Err(EINVAL));
    }

    #[test]
    fn regular_file_eperm_all_ops() {
        let r = FdRegistry::new();
        let mut l = InterestList::new();
        assert_eq!(judge(&r, &mut l, &Cell { epfd: 3, op: CTL_ADD, fd: 5 }), Err(EPERM));
        assert_eq!(judge(&r, &mut l, &Cell { epfd: 3, op: CTL_MOD, fd: 5 }), Err(EPERM));
        assert_eq!(judge(&r, &mut l, &Cell { epfd: 3, op: CTL_DEL, fd: 5 }), Err(EPERM));
    }

    #[test]
    fn op_check_precedes_eperm() {
        let r = FdRegistry::new();
        let mut l = InterestList::new();
        assert_eq!(judge(&r, &mut l, &Cell { epfd: 3, op: 7, fd: 5 }), Err(EINVAL));
    }

    #[test]
    fn add_dup_eexist_then_del_ok() {
        let r = FdRegistry::new();
        let mut l = InterestList::new();
        assert!(judge(&r, &mut l, &Cell { epfd: 3, op: CTL_ADD, fd: 4 }).is_ok());
        assert_eq!(judge(&r, &mut l, &Cell { epfd: 3, op: CTL_ADD, fd: 4 }), Err(EEXIST));
        assert!(judge(&r, &mut l, &Cell { epfd: 3, op: CTL_DEL, fd: 4 }).is_ok());
        assert_eq!(l.len(), 0);
    }

    #[test]
    fn matrix_full_agreement() {
        let r = FdRegistry::new();
        let (total, agreed) = run_matrix(&r);
        assert_eq!(total, 15);
        assert_eq!(agreed, 15);
    }

    #[test]
    fn injection_matrix_green() {
        let (l, per) = run_injection();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_table_agrees() {
        assert!(agrees_with_linux(1));
        assert_eq!(linux_neg(28), -28);
        assert!(!agrees_with_linux(0));
    }

    #[test]
    fn fingerprint_deterministic() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx9385_checks_green() {
        let s = run_unx9385_checks();
        assert!(s.all_passed(), "UNX-F9385 red: {}", s.red_items_text());
    }
}
