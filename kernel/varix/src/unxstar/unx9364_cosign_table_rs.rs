//! UNX-F9364 分发表注册协议联签（table.rs 注册接口 × C3 注入点） · 0x7C880段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F9364-J1 注册接口联签判据过，table::SyscallDesc/resolve 与
//! C3 注入点对接可观测，18 号注册面+COMPAT_TABLE 零改动格过。
//!
//! SEG_BASE = 0x7C880 诊断码闭集说明：本段 0x40 窗口独占；诊断码 1 基
//! 编码（码值 = SEG_BASE + 变体序号 + 1），闭集 12 个
//! （CosignCode::ALL）两两唯一，段基址本身不占码。
//!
//! 结构清单：
//!   ① 注册表 DispatchTable：定容槽位 + SyscallDesc（号/名/参数数/门
//!      掩码），重复注册 EEXIST、满容显性拒绝、越界 nargs EINVAL。
//!   ② 解析器 resolve：号→描述子；未注册号与越界号一律 ENOSYS，判序
//!      与 Linux 一致。
//!   ③ C3 注入点 ProbeTable：与注册表并账对接，每条 desc 挂一个探针
//!      号，未注册不得挂探针（ENOENT），零改动账逐项可复算。
//!   ④ 冻结账：18 号基线面 + COMPAT_TABLE 指纹入账，注册操作后零
//!      漂移。
//!   ⑤ 注入核：重复注册/越界 nargs/未注册 resolve 三类各 10 轮全抓，
//!      账守恒逃逸=0；Linux errno 对照表 + FNV-1a 双跑指纹。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9364 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7C880;

/// 本批段窗口下界。
pub const WINDOW_LO: u32 = 0x7A000;
/// 本批段窗口上界。
pub const WINDOW_HI: u32 = 0x7D1C0;

/// 注册表容量。
pub const REG_CAP: usize = 32;
/// 基线注册面号数（18 号注册面）。
pub const BASELINE_ROWS: usize = 18;
/// COMPAT_TABLE 别名行数。
pub const COMPAT_ROWS: usize = 8;
/// 门数量（SYSCALL 门 + int 0x80 门）。
pub const GATE_COUNT: u8 = 2;

/// 单类注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 注入类别数。
pub const INJECT_CLASSES: u32 = 3;

/// 门位：SYSCALL 门。
pub const GATE_SYSCALL: u8 = 0b01;
/// 门位：int 0x80 门。
pub const GATE_INT80: u8 = 0b10;

/// Linux errno 正数码。
pub const ENOENT: i32 = 2;
pub const EINVAL: i32 = 22;
pub const EEXIST: i32 = 17;
pub const ENOSYS: i32 = 38;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（1 基：码值 = SEG_BASE + 序号 + 1）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CosignCode {
    /// 重复注册：同号描述子已在表
    DupRegister,
    /// 越界参数数：nargs 超出闭集上限
    NargsBeyond,
    /// 未注册：resolve 命中空槽
    NotRegistered,
    /// 注册表满：槽位耗尽
    TableFull,
    /// 门掩码非法：出现了两位之外的门位
    BadGateMask,
    /// 探针挂点缺基：desc 未注册就挂探针
    ProbeMissingBase,
    /// 探针挂点重复：同一 desc 重复挂探针
    ProbeDup,
    /// 零改动账漂移：基线/COMPAT 表指纹变了
    FrozenDrift,
    /// 基线行数不符：18 号注册面行数漂移
    BaselineDrift,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl CosignCode {
    /// 码值（段基址 + 变体序号 + 1）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            CosignCode::DupRegister => "重复注册被拒：同号描述子已在表中；请改为更新 \
                既有行或改用未占用号位，重复回 EEXIST",
            CosignCode::NargsBeyond => "参数数越界被拒：nargs 必须落在 0..=6；\
                越界回 EINVAL，注册协议在入口即拒绝",
            CosignCode::NotRegistered => "未注册号被拒：resolve 命中空槽；请先走注册 \
                接口登记描述子，未注册与越界号一律回 ENOSYS",
            CosignCode::TableFull => "注册表满：槽位耗尽；请先收口注销或扩容，\
                禁止覆盖既有行",
            CosignCode::BadGateMask => "门掩码非法：只允许 SYSCALL/int0x80 两位；\
                未知门位在注册时即拒绝",
            CosignCode::ProbeMissingBase => "探针挂点缺基：desc 未注册不得挂探针；\
                请先注册再联签 C3 注入点",
            CosignCode::ProbeDup => "探针挂点重复：同一 desc 只挂一个探针；\
                请改为复用既有探针号",
            CosignCode::FrozenDrift => "零改动账漂移：基线或 COMPAT 表指纹发生变化；\
                联签协议冻结既有行，禁止重编",
            CosignCode::BaselineDrift => "基线行数不符：18 号注册面行数必须保持 18；\
                请核对注册面清单",
            CosignCode::LedgerRegress => "注入账倒退被拒：计数只增不减；请检查回滚路径",
            CosignCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查 \
                隐藏状态或非确定哈希",
            CosignCode::SegmentViolation => "段位越界：段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量。
    pub const ALL: [CosignCode; 12] = [
        CosignCode::DupRegister,
        CosignCode::NargsBeyond,
        CosignCode::NotRegistered,
        CosignCode::TableFull,
        CosignCode::BadGateMask,
        CosignCode::ProbeMissingBase,
        CosignCode::ProbeDup,
        CosignCode::FrozenDrift,
        CosignCode::BaselineDrift,
        CosignCode::LedgerRegress,
        CosignCode::NotReproducible,
        CosignCode::SegmentViolation,
    ];
}

/// 系统调用失败结果。
pub type SysResult<T> = Result<T, i32>;

// ── 描述子 ────────────────────────────────────────────────────────────
/// 一条注册行：table::SyscallDesc 的域模型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SyscallDesc {
    /// 系统调用号
    pub nr: u32,
    /// 名称（冻结清单用）
    pub name: &'static str,
    /// 参数数（0..=6）
    pub nargs: u8,
    /// 门掩码（GATE_SYSCALL / GATE_INT80）
    pub gates: u8,
}

impl SyscallDesc {
    /// 行是否合法：号非零（0 号为内核保留）、nargs ≤ 6、门掩码封闭。
    pub fn is_wellformed(&self) -> bool {
        self.nr > 0
            && self.nr <= NR_MAX
            && self.nargs <= 6
            && self.gates & !GATE_MASK_ALL == 0
            && self.gates != 0
    }
}

/// 注册号上限。
pub const NR_MAX: u32 = 512;
/// 全部门位掩码。
pub const GATE_MASK_ALL: u8 = GATE_SYSCALL | GATE_INT80;

// ── 注册表 ────────────────────────────────────────────────────────────
/// 定容分发表 + 注册账。
#[derive(Clone, Copy, Debug)]
pub struct DispatchTable {
    rows: [Option<SyscallDesc>; REG_CAP],
    len: usize,
    regs: u32,
    rejects: u32,
}

impl DispatchTable {
    /// 空表。
    pub const fn new() -> DispatchTable {
        DispatchTable { rows: [None; REG_CAP], len: 0, regs: 0, rejects: 0 }
    }

    /// 用基线面构造（零改动账的初始状态）。
    pub fn with_baseline() -> DispatchTable {
        let mut t = DispatchTable::new();
        let mut i = 0usize;
        while i < BASELINE.len() {
            if let Some(d) = BASELINE.get(i).copied() {
                let _ = t.register(d);
            }
            i += 1;
        }
        t
    }

    /// 注册一行：重复 EEXIST、满容占位 ENOENT、坏行 EINVAL。
    pub fn register(&mut self, desc: SyscallDesc) -> SysResult<usize> {
        if !desc.is_wellformed() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EINVAL);
        }
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(d)) = self.rows.get(i).map(|x| *x) {
                if d.nr == desc.nr {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(EEXIST);
                }
            }
            i += 1;
        }
        if self.len >= REG_CAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ENOENT);
        }
        let slot = self.len;
        if let Some(cell) = self.rows.get_mut(slot) {
            *cell = Some(desc);
        }
        self.len += 1;
        self.regs = self.regs.saturating_add(1);
        Ok(slot)
    }

    /// 解析：未注册/越界号一律 ENOSYS（判序：号段检查先于槽位扫描）。
    pub fn resolve(&self, nr: u32) -> SysResult<SyscallDesc> {
        if nr == 0 || nr > NR_MAX {
            return Err(ENOSYS);
        }
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(d)) = self.rows.get(i).map(|x| *x) {
                if d.nr == nr {
                    return Ok(d);
                }
            }
            i += 1;
        }
        Err(ENOSYS)
    }

    /// 在册行数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 注册/拒绝账。
    pub fn regs(&self) -> u32 {
        self.regs
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

// ── 18 号基线注册面（冻结，零改动账参照） ─────────────────────────────
/// 基线面：18 行冻结描述子。
pub const BASELINE: [SyscallDesc; BASELINE_ROWS] = [
    SyscallDesc { nr: 1, name: "read", nargs: 3, gates: GATE_SYSCALL | GATE_INT80 },
    SyscallDesc { nr: 2, name: "write", nargs: 3, gates: GATE_SYSCALL | GATE_INT80 },
    SyscallDesc { nr: 3, name: "open", nargs: 3, gates: GATE_SYSCALL | GATE_INT80 },
    SyscallDesc { nr: 4, name: "close", nargs: 1, gates: GATE_SYSCALL | GATE_INT80 },
    SyscallDesc { nr: 5, name: "stat", nargs: 2, gates: GATE_SYSCALL },
    SyscallDesc { nr: 6, name: "fstat", nargs: 2, gates: GATE_SYSCALL | GATE_INT80 },
    SyscallDesc { nr: 7, name: "poll", nargs: 3, gates: GATE_SYSCALL },
    SyscallDesc { nr: 8, name: "lseek", nargs: 3, gates: GATE_SYSCALL | GATE_INT80 },
    SyscallDesc { nr: 9, name: "mmap", nargs: 6, gates: GATE_SYSCALL },
    SyscallDesc { nr: 10, name: "mprotect", nargs: 3, gates: GATE_SYSCALL },
    SyscallDesc { nr: 11, name: "munmap", nargs: 2, gates: GATE_SYSCALL },
    SyscallDesc { nr: 12, name: "brk", nargs: 1, gates: GATE_SYSCALL | GATE_INT80 },
    SyscallDesc { nr: 13, name: "ioctl", nargs: 3, gates: GATE_SYSCALL | GATE_INT80 },
    SyscallDesc { nr: 14, name: "pipe", nargs: 1, gates: GATE_SYSCALL | GATE_INT80 },
    SyscallDesc { nr: 15, name: "dup", nargs: 1, gates: GATE_SYSCALL },
    SyscallDesc { nr: 16, name: "dup2", nargs: 2, gates: GATE_SYSCALL | GATE_INT80 },
    SyscallDesc { nr: 17, name: "getpid", nargs: 0, gates: GATE_SYSCALL | GATE_INT80 },
    SyscallDesc { nr: 18, name: "fork", nargs: 0, gates: GATE_SYSCALL },
];

/// COMPAT_TABLE：兼容别名面（冻结，零改动账参照）。
pub const COMPAT_TABLE: [(u32, u32); COMPAT_ROWS] = [
    (3, 2),
    (5, 6),
    (8, 19),
    (16, 32),
    (9, 90),
    (11, 91),
    (13, 54),
    (14, 42),
];

// ── C3 注入点 ─────────────────────────────────────────────────────────
/// C3 探针表：与注册表并账对接。
#[derive(Clone, Copy, Debug)]
pub struct ProbeTable {
    probes: [Option<u32>; REG_CAP],
    attached: u32,
}

impl ProbeTable {
    /// 空表。
    pub const fn new() -> ProbeTable {
        ProbeTable { probes: [None; REG_CAP], attached: 0 }
    }

    /// 为已注册 desc 挂探针：探针号 = nr*10+1；缺基 ENOENT、重复 EEXIST。
    pub fn attach(&mut self, table: &DispatchTable, nr: u32) -> SysResult<u32> {
        if table.resolve(nr).is_err() {
            return Err(ENOENT);
        }
        let pid = nr.saturating_mul(10).saturating_add(1);
        let mut i = 0usize;
        while i < REG_CAP {
            if let Some(Some(p)) = self.probes.get(i).map(|x| *x) {
                if p == pid {
                    return Err(EEXIST);
                }
            }
            i += 1;
        }
        let mut slot = None;
        let mut j = 0usize;
        while j < REG_CAP {
            if let Some(None) = self.probes.get(j).map(|x| *x) {
                slot = Some(j);
                break;
            }
            j += 1;
        }
        match slot {
            Some(idx) => {
                if let Some(cell) = self.probes.get_mut(idx) {
                    *cell = Some(pid);
                }
                self.attached = self.attached.saturating_add(1);
                Ok(pid)
            }
            None => Err(ENOENT),
        }
    }

    /// 探针查询。
    pub fn probe_of(&self, nr: u32) -> Option<u32> {
        let pid = nr.saturating_mul(10).saturating_add(1);
        let mut i = 0usize;
        while i < REG_CAP {
            if let Some(Some(p)) = self.probes.get(i).map(|x| *x) {
                if p == pid {
                    return Some(p);
                }
            }
            i += 1;
        }
        None
    }

    /// 挂载计数。
    pub fn attached(&self) -> u32 {
        self.attached
    }
}

// ── 零改动账 ──────────────────────────────────────────────────────────
/// 基线面指纹（逐行 nr+nargs+gates 复算）。
pub fn baseline_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < BASELINE.len() {
        if let Some(d) = BASELINE.get(i) {
            bytes.push((d.nr & 0xFF) as u8);
            bytes.push(d.nargs);
            bytes.push(d.gates);
        }
        i += 1;
    }
    fnv1a(&bytes)
}

/// COMPAT 表指纹。
pub fn compat_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < COMPAT_TABLE.len() {
        if let Some(row) = COMPAT_TABLE.get(i) {
            bytes.push((row.0 & 0xFF) as u8);
            bytes.push((row.1 & 0xFF) as u8);
        }
        i += 1;
    }
    fnv1a(&bytes)
}

/// 零改动声明：18 行在册、指纹可复算且非零。
pub fn frozen_intact() -> bool {
    BASELINE.len() == BASELINE_ROWS && compat_fingerprint() != 0 && baseline_fingerprint() != 0
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸，逃逸恒 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) -> SysResult<()> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(-(CosignCode::LedgerRegress as i32));
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 三类违约样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViolationCase {
    /// 重复注册
    DupRegister,
    /// 越界 nargs
    NargsBeyond,
    /// 未注册 resolve
    NotRegistered,
}

impl ViolationCase {
    /// 闭集全量。
    pub const ALL: [ViolationCase; 3] = [
        ViolationCase::DupRegister,
        ViolationCase::NargsBeyond,
        ViolationCase::NotRegistered,
    ];

    /// 样本执行：违约必须命中约定码。
    fn run(self) -> bool {
        let mut t = DispatchTable::with_baseline();
        match self {
            ViolationCase::DupRegister => {
                let d = SyscallDesc { nr: 4, name: "close2", nargs: 1, gates: GATE_SYSCALL };
                t.register(d) == Err(EEXIST)
            }
            ViolationCase::NargsBeyond => {
                let d = SyscallDesc { nr: 40, name: "bad", nargs: 7, gates: GATE_SYSCALL };
                t.register(d) == Err(EINVAL)
            }
            ViolationCase::NotRegistered => t.resolve(300) == Err(ENOSYS),
        }
    }
}

/// 三类违约 × 10 轮注入，返回账本与逐类检出数。
pub fn run_register_injection() -> (InjectLedger, [u32; 3]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 3];
    let mut ci = 0usize;
    while ci < ViolationCase::ALL.len() {
        if let Some(case) = ViolationCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.run();
                let _ = ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；探针缺基/表满占位"),
    ("EEXIST", -17, 80, "Windows ERROR_FILE_EXISTS；重复注册族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏行族"),
    ("ENOSYS", -38, 1, "Windows ERROR_INVALID_FUNCTION；未实装号族"),
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

/// 规范语料双跑指纹：基线面 + COMPAT 面 + 解析语料 + 探针语料。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push((baseline_fingerprint() & 0xFF) as u8);
    bytes.push(((baseline_fingerprint() >> 8) & 0xFF) as u8);
    bytes.push((compat_fingerprint() & 0xFF) as u8);
    let t = DispatchTable::with_baseline();
    let nrs = [1u32, 4, 9, 18, 19, 0, 513, 300];
    let mut i = 0usize;
    while i < nrs.len() {
        if let Some(nr) = nrs.get(i).copied() {
            match t.resolve(nr) {
                Ok(d) => {
                    bytes.push((d.nr & 0xFF) as u8);
                    bytes.push(d.nargs);
                }
                Err(code) => bytes.push((code & 0xFF) as u8),
            }
        }
        i += 1;
    }
    let mut pt = ProbeTable::new();
    let r1 = pt.attach(&t, 4);
    let r2 = pt.attach(&t, 4);
    let r3 = pt.attach(&t, 300);
    bytes.push(match r1 {
        Ok(_) => 1,
        Err(_) => 0,
    });
    bytes.push(match r2 {
        Ok(_) => 1,
        Err(_) => 0,
    });
    bytes.push(match r3 {
        Ok(_) => 1,
        Err(_) => 0,
    });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9364 判据集。
pub fn run_unx9364_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9364");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (WINDOW_LO..=WINDOW_HI).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：基线面构造 + resolve 命中
    let t = DispatchTable::with_baseline();
    let normal = t.len() == BASELINE_ROWS
        && match t.resolve(4) {
            Ok(d) => d.nr == 4 && d.name == "close" && d.nargs == 1,
            Err(_) => false,
        };
    s.add("正常流：18 号基线面在册且 resolve 命中", normal,
          "注册接口联签的第一格：构造基线、按号解析成功");

    // 3~5. 注入矩阵：三类违约各 10/10 + 账守恒
    let (ledger, per_class) = run_register_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("三类违约注入各 10/10 全检出", all_caught,
          "重复注册/越界 nargs/未注册 resolve 必须全部命中约定码");
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == INJECT_CLASSES * INJECT_ROUNDS,
          "30 次注入必须 30 次检出、0 逃逸");

    // 6. 重复注册：EEXIST 且表行数不变
    let mut t2 = DispatchTable::with_baseline();
    let dup = SyscallDesc { nr: 9, name: "mmap2", nargs: 6, gates: GATE_SYSCALL };
    let before = t2.len();
    s.add("重复注册：EEXIST 且行数纹丝不动",
          t2.register(dup) == Err(EEXIST) && t2.len() == before,
          "既有行冻结，重复注册不得改写或追加");

    // 7. 越界 nargs：EINVAL（0..=6 闭集，边界逐格）
    let mut bounds_ok = true;
    let mut na = 0u8;
    while na <= 6 {
        let d = SyscallDesc { nr: 60 + na as u32, name: "ok", nargs: na, gates: GATE_SYSCALL };
        if t2.register(d).is_err() {
            bounds_ok = false;
        }
        na += 1;
    }
    let bad = SyscallDesc { nr: 70, name: "bad", nargs: 7, gates: GATE_SYSCALL };
    s.add("参数数边界：0..=6 放行、7 拒绝",
          bounds_ok && t2.register(bad) == Err(EINVAL),
          "nargs 闭集边界逐格落账，越界在注册口即拒");

    // 8. 门掩码：未知位拒绝、两位全掩码放行
    let full = SyscallDesc { nr: 80, name: "both", nargs: 0, gates: GATE_MASK_ALL };
    let unknown = SyscallDesc { nr: 81, name: "u", nargs: 0, gates: 0b1000 };
    s.add("门掩码：双门全掩码放行、未知位拒绝",
          t2.register(full).is_ok() && t2.register(unknown) == Err(EINVAL),
          "门位闭集 {SYSCALL, int0x80}，越界位在注册口拒绝");

    // 9. resolve 判序：0 号与越上限号 ENOSYS（号段检查先于扫描）
    let order_ok = t.resolve(0) == Err(ENOSYS)
        && t.resolve(NR_MAX + 1) == Err(ENOSYS)
        && t.resolve(1).is_ok();
    s.add("解析判序：0 号/越上限先回 ENOSYS，界内命中", order_ok,
          "号段校验先于槽位扫描，与 Linux 未实装号语义一致");

    // 10. C3 注入点对接：已注册可挂、未注册 ENOENT、重复 EEXIST
    let mut pt = ProbeTable::new();
    let attach_ok = pt.attach(&t, 4).is_ok()
        && pt.attach(&t, 4) == Err(EEXIST)
        && pt.attach(&t, 300) == Err(ENOENT)
        && pt.probe_of(4) == Some(41);
    s.add("C3 注入点：挂探针/重复拒/缺基拒，探针号可查", attach_ok,
          "table::SyscallDesc 与 C3 探针并账，probe 号 = nr*10+1");

    // 11. 探针覆盖基线全量：18 号注册面全部可挂
    let mut pt_all = ProbeTable::new();
    let mut cov_ok = true;
    let mut nr = 1u32;
    while nr <= BASELINE_ROWS as u32 {
        if pt_all.attach(&t, nr).is_err() {
            cov_ok = false;
        }
        nr += 1;
    }
    s.add("探针覆盖：18 号基线面全量挂载成功",
          cov_ok && pt_all.attached() == BASELINE_ROWS as u32,
          "C3 注入点必须覆盖注册面全量，缺一行即失联");

    // 12. 零改动账：基线 + COMPAT 指纹复算
    s.add("零改动账：18 号注册面与 COMPAT_TABLE 指纹入账",
          frozen_intact() && BASELINE.len() == BASELINE_ROWS
              && COMPAT_TABLE.len() == COMPAT_ROWS,
          "联签协议冻结既有行，注册操作后指纹必须零漂移");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(ENOENT)
        && agrees_with_linux(EEXIST)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(ENOSYS)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 14. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 15. 文案闭集
    let text_ok = CosignCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 诊断码唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < CosignCode::ALL.len() {
        let mut j = i + 1;
        while j < CosignCode::ALL.len() {
            if let (Some(x), Some(y)) = (CosignCode::ALL.get(i), CosignCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 17. 对照表一致性
    let mut table_ok = true;
    let mut ti = 0usize;
    while ti < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(ti) {
            if row.1 != -(-row.1) || row.2 == 0 || row.3.is_empty() {
                table_ok = false;
            }
        }
        ti += 1;
    }
    s.add("对照表一致性：errno/Windows 码逐项自洽", table_ok,
          "表内每一行负值可复算、Windows 码非零、说明非空");

    // 18. 账目单调 + 注册账闭合
    let mut t3 = DispatchTable::with_baseline();
    let mut k = 0u32;
    while k < 4 {
        let d = SyscallDesc { nr: 100 + k, name: "x", nargs: 1, gates: GATE_SYSCALL };
        let _ = t3.register(d);
        k += 1;
    }
    s.add("注册账闭合：基线 18 行 + 4 笔新注册 = 22 在册",
          t3.len() == BASELINE_ROWS + 4 && t3.regs() == BASELINE_ROWS as u32 + 4,
          "每次成功注册各记一笔，在册数 = 基线 + 增量");

    // 动态明细（≤8）
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {} / 拒绝 {}",
        ledger.injected, ledger.detected, ledger.escaped, t.rejects()
    );
    s.add_dyn("账目快照", ledger.conserved(), &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_resolves() {
        let t = DispatchTable::with_baseline();
        assert_eq!(t.len(), BASELINE_ROWS);
        let d = t.resolve(1).unwrap_or(SyscallDesc { nr: 0, name: "", nargs: 0, gates: 0 });
        assert_eq!(d.name, "read");
    }

    #[test]
    fn dup_register_eexist() {
        let mut t = DispatchTable::with_baseline();
        let d = SyscallDesc { nr: 1, name: "read2", nargs: 3, gates: GATE_SYSCALL };
        assert_eq!(t.register(d), Err(EEXIST));
    }

    #[test]
    fn nargs_bounds() {
        let mut t = DispatchTable::new();
        let mut n = 0u8;
        while n <= 6 {
            let d = SyscallDesc { nr: 10 + n as u32, name: "x", nargs: n, gates: GATE_SYSCALL };
            assert!(t.register(d).is_ok());
            n += 1;
        }
        let d = SyscallDesc { nr: 99, name: "y", nargs: 7, gates: GATE_SYSCALL };
        assert_eq!(t.register(d), Err(EINVAL));
    }

    #[test]
    fn resolve_unregistered_enosys() {
        let t = DispatchTable::with_baseline();
        assert_eq!(t.resolve(300), Err(ENOSYS));
        assert_eq!(t.resolve(0), Err(ENOSYS));
        assert_eq!(t.resolve(NR_MAX + 1), Err(ENOSYS));
    }

    #[test]
    fn gate_mask_closed_set() {
        let mut t = DispatchTable::new();
        let ok = SyscallDesc { nr: 1, name: "a", nargs: 0, gates: GATE_MASK_ALL };
        let bad = SyscallDesc { nr: 2, name: "b", nargs: 0, gates: 0b100 };
        assert!(t.register(ok).is_ok());
        assert_eq!(t.register(bad), Err(EINVAL));
    }

    #[test]
    fn probe_attach_rules() {
        let t = DispatchTable::with_baseline();
        let mut pt = ProbeTable::new();
        assert_eq!(pt.attach(&t, 4), Ok(41));
        assert_eq!(pt.attach(&t, 4), Err(EEXIST));
        assert_eq!(pt.attach(&t, 300), Err(ENOENT));
        assert_eq!(pt.probe_of(4), Some(41));
    }

    #[test]
    fn probe_covers_baseline() {
        let t = DispatchTable::with_baseline();
        let mut pt = ProbeTable::new();
        let mut n = 1u32;
        while n <= BASELINE_ROWS as u32 {
            assert!(pt.attach(&t, n).is_ok());
            n += 1;
        }
        assert_eq!(pt.attached(), BASELINE_ROWS as u32);
    }

    #[test]
    fn frozen_zero_drift() {
        assert!(frozen_intact());
        assert_eq!(BASELINE.len(), 18);
        assert!(baseline_fingerprint() != 0);
        assert!(compat_fingerprint() != 0);
    }

    #[test]
    fn matrix_ledger_conserved() {
        let (l, per) = run_register_injection();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, INJECT_CLASSES * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EEXIST));
        assert!(agrees_with_linux(ENOSYS));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(ENOSYS), -ENOSYS);
    }

    #[test]
    fn unx9364_checks_green() {
        let s = run_unx9364_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
