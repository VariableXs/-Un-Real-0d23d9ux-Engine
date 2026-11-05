//! UNX-F9250 地址空间越界访问 SIGSEGV 路径矩阵 · 0x7AC00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9250 逐字）：
//!   UNX-F9250-J1 越界读写触发 #PF 判据过，SIGSEGV 投递可观测格过，
//!   si_addr 逐格正确格过。
//!
//! 设计要点：
//!   ① #PF 判定次序：内核地址 → 映射查找 → 权限位。找不到映射一律
//!      SEGV_MAPERR；找到映射但位不匹配才是 SEGV_ACCERR。次序不可对调，
//!      否则保护区页会被误报成权限错误。
//!   ② si_addr 是「触发故障的原始地址」，不是页基址：保护区、映射空洞、
//!      只读页写、NX 取指、内核地址五种故障的 si_addr 必须逐格等于注入
//!      地址，任何取整或偏移都判红。
//!   ③ 投递可观测：故障必须落到定容 4 槽信号队列，槽内容 (signo=11,
//!      si_code, si_addr) 可逐字段核对；成功访问不得产生任何投递。
//!   ④ 队列是状态机：满 4 槽后第 5 次投递回 ENOBUFS 并计入 dropped，
//!      pop 腾出一槽后同一投递必须成功；delivered 与 dropped 都只增。
//!   ⑤ 故障注入 6 类 × 每类 10 轮：未映射低位、保护页、映射空洞、只读写、
//!      NX 取指、内核地址，全部必须检出并携带正确 si_addr。
//!   ⑥ 诊断码段内闭集 12 个（+0x01..=+0x0C），码值唯一，每码带中文建议。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9250 独占 0x7AC00..=0x7AC3F）。
pub const SEG_BASE: u32 = 0x7AC00;
/// 4K 页大小。
pub const PAGE: u64 = 4096;
/// 语料区间基址。
pub const REGION_BASE: u64 = 0x0000_0000_6000_0000;
/// 内核地址起点（用户态不可访问）。
pub const KERNEL_BASE: u64 = 0xFFFF_8000_0000_0000;
/// SIGSEGV 信号号。
pub const SIGSEGV: i32 = 11;
/// 地址未映射。
pub const SEGV_MAPERR: i32 = 1;
/// 权限位不匹配。
pub const SEGV_ACCERR: i32 = 2;
/// 信号队列容量。
pub const QUEUE_CAP: usize = 4;
/// 单类注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 故障注入类别数。
pub const FAULT_CLASSES: u32 = 6;
/// 队列满（Linux ENOBUFS）。
pub const ENOBUFS: i32 = 105;

/// PROT_READ。
pub const PROT_READ: u8 = 1;
/// PROT_WRITE。
pub const PROT_WRITE: u8 = 2;
/// PROT_EXEC。
pub const PROT_EXEC: u8 = 4;

/// 访问类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Access {
    Read,
    Write,
    Exec,
}

impl Access {
    /// 对应的保护位。
    pub fn bit(self) -> u8 {
        match self {
            Access::Read => PROT_READ,
            Access::Write => PROT_WRITE,
            Access::Exec => PROT_EXEC,
        }
    }
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SegvCode {
    UnmappedAccess,
    GuardHit,
    WriteToRo,
    ExecOnNx,
    KernelAddress,
    SiAddrMismatch,
    SignalLost,
    QueueFull,
    BadOrder,
    LedgerRegress,
    NotReproducible,
    SegmentViolation,
}

impl SegvCode {
    /// 码值（段基址 + 1 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            SegvCode::UnmappedAccess => "访问未映射地址：si_code=SEGV_MAPERR；请先 mmap 建立映射再访问，si_addr 保留原始故障地址",
            SegvCode::GuardHit => "命中保护区页：保护页故意不映射；请检查越界长度，切勿用保护页做数据缓冲",
            SegvCode::WriteToRo => "只读页写入：si_code=SEGV_ACCERR；请先 mprotect 加 WRITE 位或改写映射策略",
            SegvCode::ExecOnNx => "NX 页取指：si_code=SEGV_ACCERR；请补 PROT_EXEC 后再执行，不要依赖隐式可执行",
            SegvCode::KernelAddress => "内核地址访问：用户态一律 SEGV_MAPERR；请核对指针来源，严禁回绕到高半区",
            SegvCode::SiAddrMismatch => "si_addr 不匹配：信号携带地址与注入地址不同；请排查是否误做页对齐或偏移",
            SegvCode::SignalLost => "信号丢失：故障已产生但队列未收到；请检查投递路径是否被短路",
            SegvCode::QueueFull => "信号队列满：4 槽已耗尽，新故障回 ENOBUFS；请及时消费队列或扩大容量",
            SegvCode::BadOrder => "判定次序错误：MAPERR 与 ACCERR 被对调；请先查映射再查权限位",
            SegvCode::LedgerRegress => "投递账倒退被拒：计数只增不减；请检查回滚路径是否误改累计值",
            SegvCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏状态或非确定哈希",
            SegvCode::SegmentViolation => "段位越界：本模块段基址必须落在 0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量（文案与唯一性检查使用）。
    pub const ALL: [SegvCode; 12] = [
        SegvCode::UnmappedAccess,
        SegvCode::GuardHit,
        SegvCode::WriteToRo,
        SegvCode::ExecOnNx,
        SegvCode::KernelAddress,
        SegvCode::SiAddrMismatch,
        SegvCode::SignalLost,
        SegvCode::QueueFull,
        SegvCode::BadOrder,
        SegvCode::LedgerRegress,
        SegvCode::NotReproducible,
        SegvCode::SegmentViolation,
    ];
}

// ── 映射模型 ──────────────────────────────────────────────────────────
/// 一个映射区间。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Mapping {
    pub used: bool,
    pub base: u64,
    pub pages: u32,
    pub prot: u8,
}

impl Mapping {
    pub const fn empty() -> Mapping {
        Mapping { used: false, base: 0, pages: 0, prot: 0 }
    }
    /// 区间末端（不含）。
    pub fn end(&self) -> u64 {
        self.base.saturating_add(PAGE * self.pages as u64)
    }
    /// 是否包含地址。
    pub fn contains(&self, addr: u64) -> bool {
        self.used && addr >= self.base && addr < self.end()
    }
}

/// 定容地址空间：3 段已映射 + 保护区与空洞。
#[derive(Clone, Copy, Debug)]
pub struct AddressSpace {
    maps: [Mapping; 6],
    count: u32,
}

impl AddressSpace {
    /// 语料布局：
    ///   A: base+0p  4 页 RW（0..4）
    ///   B: base+8p  2 页 R （8..10）
    ///   C: base+16p 1 页 RX（16..17）
    /// 4..8 为低保护区，10..16 为空洞，17.. 为高保护区。
    pub const fn new() -> AddressSpace {
        AddressSpace {
            maps: [
                Mapping { used: true, base: REGION_BASE, pages: 4, prot: PROT_READ | PROT_WRITE },
                Mapping {
                    used: true,
                    base: REGION_BASE + PAGE * 8,
                    pages: 2,
                    prot: PROT_READ,
                },
                Mapping {
                    used: true,
                    base: REGION_BASE + PAGE * 16,
                    pages: 1,
                    prot: PROT_READ | PROT_EXEC,
                },
                Mapping::empty(),
                Mapping::empty(),
                Mapping::empty(),
            ],
            count: 3,
        }
    }

    /// 已注册映射段数。
    pub fn count(&self) -> u32 {
        self.count
    }

    /// 查找包含 addr 的映射。
    fn find(&self, addr: u64) -> Option<&Mapping> {
        let mut i = 0usize;
        while i < self.maps.len() {
            if let Some(m) = self.maps.get(i) {
                if m.contains(addr) {
                    return Some(m);
                }
            }
            i += 1;
        }
        None
    }

    /// #PF 判定：内核地址 → 未映射 → 权限位。
    pub fn probe(&self, addr: u64, kind: Access) -> Result<u8, FaultInfo> {
        if addr >= KERNEL_BASE {
            return Err(FaultInfo { addr, si_code: SEGV_MAPERR });
        }
        let m = match self.find(addr) {
            Some(v) => v,
            None => return Err(FaultInfo { addr, si_code: SEGV_MAPERR }),
        };
        if m.prot & kind.bit() == 0 {
            return Err(FaultInfo { addr, si_code: SEGV_ACCERR });
        }
        Ok(m.prot)
    }

    /// 地址是否落在已映射区间（诊断辅助，不参与判定）。
    pub fn is_mapped(&self, addr: u64) -> bool {
        self.find(addr).is_some()
    }
}

// ── 信号队列 ──────────────────────────────────────────────────────────
/// 一次故障的 siginfo 摘要（只保留判据所需字段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FaultInfo {
    pub addr: u64,
    pub si_code: i32,
}

/// 定容信号队列：4 槽 + 投递/丢弃计数。
#[derive(Clone, Copy, Debug)]
pub struct SigQueue {
    slots: [Option<FaultInfo>; QUEUE_CAP],
    len: usize,
    delivered: u32,
    dropped: u32,
}

impl SigQueue {
    pub const fn new() -> SigQueue {
        SigQueue { slots: [None; QUEUE_CAP], len: 0, delivered: 0, dropped: 0 }
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn delivered(&self) -> u32 {
        self.delivered
    }
    pub fn dropped(&self) -> u32 {
        self.dropped
    }
    /// 按槽下标取信号；越界或空槽返回 None。
    pub fn slot(&self, i: usize) -> Option<FaultInfo> {
        match self.slots.get(i) {
            Some(v) => *v,
            None => None,
        }
    }
    /// 投递一条信号；队列满回 ENOBUFS 并计入 dropped。
    pub fn push(&mut self, info: FaultInfo) -> Result<usize, i32> {
        if self.len >= QUEUE_CAP {
            self.dropped = self.dropped.saturating_add(1);
            return Err(ENOBUFS);
        }
        let idx = self.len;
        if let Some(slot) = self.slots.get_mut(idx) {
            *slot = Some(info);
        }
        self.len = self.len.saturating_add(1);
        self.delivered = self.delivered.saturating_add(1);
        Ok(idx)
    }
    /// 消费最早一条（FIFO），腾出队尾槽。
    pub fn pop(&mut self) -> Option<FaultInfo> {
        if self.len == 0 {
            return None;
        }
        let first = match self.slots.get(0) {
            Some(v) => *v,
            None => None,
        };
        let mut i = 1usize;
        while i < self.len {
            let v = match self.slots.get(i) {
                Some(x) => *x,
                None => None,
            };
            if let Some(dst) = self.slots.get_mut(i - 1) {
                *dst = v;
            }
            i += 1;
        }
        if self.len > 0 {
            if let Some(last) = self.slots.get_mut(self.len - 1) {
                *last = None;
            }
        }
        self.len -= 1;
        first
    }
}

/// 一次陷阱的结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TrapOutcome {
    pub fault: Option<FaultInfo>,
    pub delivered: bool,
    pub queue_len: usize,
}

/// 访问 + 故障投递一体化：成功不投递，失败必投递（除非队列满）。
pub fn trap_and_deliver(space: &AddressSpace, q: &mut SigQueue, addr: u64, kind: Access) -> TrapOutcome {
    match space.probe(addr, kind) {
        Ok(_) => TrapOutcome { fault: None, delivered: false, queue_len: q.len() },
        Err(info) => {
            let delivered = q.push(info).is_ok();
            TrapOutcome { fault: Some(info), delivered, queue_len: q.len() }
        }
    }
}

// ── 故障注入矩阵 ──────────────────────────────────────────────────────
/// 一类故障样本：(地址, 访问类型, 期望 si_code)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FaultCase {
    pub addr: u64,
    pub kind: Access,
    pub si_code: i32,
}

/// 六类故障样本（地址全部避开映射内的合法字节）。
pub const FAULT_CASES: [FaultCase; 6] = [
    FaultCase { addr: 0x1000, kind: Access::Read, si_code: SEGV_MAPERR },
    FaultCase {
        addr: REGION_BASE + PAGE * 5,
        kind: Access::Read,
        si_code: SEGV_MAPERR,
    },
    FaultCase {
        addr: REGION_BASE + PAGE * 12,
        kind: Access::Write,
        si_code: SEGV_MAPERR,
    },
    FaultCase {
        addr: REGION_BASE + PAGE * 8,
        kind: Access::Write,
        si_code: SEGV_ACCERR,
    },
    FaultCase {
        addr: REGION_BASE + PAGE,
        kind: Access::Exec,
        si_code: SEGV_ACCERR,
    },
    FaultCase {
        addr: KERNEL_BASE + 0x100,
        kind: Access::Read,
        si_code: SEGV_MAPERR,
    },
];

/// 注入 6 类 × 10 轮：每轮独立队列，故障必须投递且 si_addr 精确。
/// 返回 (逐类命中数, 注入账)。
pub fn run_fault_matrix() -> ([u32; 6], InjectLedger) {
    let space = AddressSpace::new();
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 6];
    let mut ci = 0usize;
    while ci < FAULT_CASES.len() {
        if let Some(case) = FAULT_CASES.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let mut q = SigQueue::new();
                let out = trap_and_deliver(&space, &mut q, case.addr, case.kind);
                let caught = match out.fault {
                    Some(info) => {
                        out.delivered
                            && info.addr == case.addr
                            && info.si_code == case.si_code
                            && q.len() == 1
                    }
                    None => false,
                };
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
    (per_class, ledger)
}

/// si_addr 十格网格：(地址, 访问类型, 期望 si_code)。0 表示期望成功。
pub const ADDR_GRID: [(u64, Access, i32); 10] = [
    (0x1000, Access::Read, SEGV_MAPERR),
    (REGION_BASE + PAGE * 5, Access::Read, SEGV_MAPERR),
    (REGION_BASE + PAGE * 7, Access::Write, SEGV_MAPERR),
    (REGION_BASE + PAGE * 12, Access::Exec, SEGV_MAPERR),
    (REGION_BASE + PAGE * 8, Access::Write, SEGV_ACCERR),
    (REGION_BASE + PAGE * 16, Access::Write, SEGV_ACCERR),
    (KERNEL_BASE + 0x100, Access::Read, SEGV_MAPERR),
    (REGION_BASE + PAGE * 3, Access::Write, 0),
    (REGION_BASE + PAGE, Access::Read, 0),
    (REGION_BASE + PAGE * 16, Access::Exec, 0),
];

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账本：注入 = 检出 + 逃逸，逃逸恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl InjectLedger {
    pub const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }
    pub fn record(&mut self, caught: bool) -> Result<(), i32> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(ENOBUFS);
        }
        Ok(())
    }
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ENOMEM", -12, 8, "Windows ERROR_NOT_ENOUGH_MEMORY；内存不足族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
    ("ENOBUFS", -105, 8, "Windows ERROR_NOT_ENOUGH_MEMORY；队列满族"),
];

/// 查表：按 errno 正数取负值；表外返回 0。
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

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机、无墙钟）。
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

/// 把一次陷阱编码为字节：无故障 0；有故障 1 + si_code + 地址低字节。
fn encode(out: &mut Vec<u8>, out_come: TrapOutcome) {
    match out_come.fault {
        None => out.push(0),
        Some(info) => {
            out.push(1);
            out.push((info.si_code & 0x7F) as u8);
            out.push((info.addr & 0xFF) as u8);
        }
    }
}

/// 规范语料双跑指纹：十格网格 + 队列两轮压满。
pub fn corpus_fingerprint() -> u32 {
    let space = AddressSpace::new();
    let mut bytes: Vec<u8> = Vec::new();
    let mut q = SigQueue::new();
    let mut i = 0usize;
    while i < ADDR_GRID.len() {
        if let Some((addr, kind, _code)) = ADDR_GRID.get(i).copied() {
            let out = trap_and_deliver(&space, &mut q, addr, kind);
            encode(&mut bytes, out);
        }
        i += 1;
    }
    bytes.push(9);
    bytes.push(q.len() as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9250 判据集（add ≥12；add_dyn ≤8）。
pub fn run_unx9250_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9250");

    let seg_ok = SEG_BASE % 0x40 == 0
        && (0x7A000..=0x7D1C0).contains(&SEG_BASE)
        && SEG_BASE > 0x7ABC0;
    s.add("段位守卫：0x40 对齐且不与 F9249 重叠", seg_ok,
          "段基址唯一且落在本批窗口，段间不得互相覆盖");

    let space = AddressSpace::new();
    let mut quiet = SigQueue::new();
    let ok_read = trap_and_deliver(&space, &mut quiet, REGION_BASE + PAGE, Access::Read);
    let ok_write = trap_and_deliver(&space, &mut quiet, REGION_BASE + PAGE * 2, Access::Write);
    let normal_ok = ok_read.fault.is_none()
        && ok_write.fault.is_none()
        && !ok_read.delivered
        && quiet.len() == 0
        && space.count() == 3
        && space.is_mapped(REGION_BASE)
        && !space.is_mapped(REGION_BASE + PAGE * 5);
    s.add("正常流：合法读写不产生故障与投递", normal_ok,
          "成功访问必须静默，队列长度保持 0");

    let (per_class, ledger) = run_fault_matrix();
    s.add("注入：6 类故障各 10/10 检出且 si_addr 精确",
          per_class.iter().all(|n| *n == INJECT_ROUNDS),
          "未映射/保护页/空洞/只读写/NX 取指/内核地址必须全部命中");
    s.add("注入账：60 次故障全投递且逃逸为 0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == FAULT_CLASSES * INJECT_ROUNDS,
          "60 次注入必须 60 次检出，故障不得丢失");

    let mut grid_ok = true;
    let mut code_ok = true;
    let mut i = 0usize;
    while i < ADDR_GRID.len() {
        if let Some((addr, kind, expect)) = ADDR_GRID.get(i).copied() {
            let mut q = SigQueue::new();
            let out = trap_and_deliver(&space, &mut q, addr, kind);
            match out.fault {
                Some(info) => {
                    if expect == 0 || info.addr != addr {
                        grid_ok = false;
                    }
                    if info.si_code != expect {
                        code_ok = false;
                    }
                }
                None => {
                    if expect != 0 {
                        grid_ok = false;
                    }
                }
            }
        }
        i += 1;
    }
    s.add("si_addr：十格网格逐格等于注入地址", grid_ok,
          "信号地址必须是原始故障地址，不得做页对齐或偏移");
    s.add("si_code：MAPERR/ACCERR 十格逐格正确", code_ok,
          "未映射必须 MAPERR，权限位不符必须 ACCERR");

    let mut order = SigQueue::new();
    let out_kernel = trap_and_deliver(&space, &mut order, KERNEL_BASE, Access::Write);
    let order_ok = match out_kernel.fault {
        Some(info) => info.si_code == SEGV_MAPERR,
        None => false,
    };
    s.add("判定次序：内核地址先回 MAPERR 而非 ACCERR", order_ok,
          "映射查找必须先于权限位判定");

    let mut full = SigQueue::new();
    let mut pushed = 0u32;
    let mut k = 0u32;
    while k < QUEUE_CAP as u32 {
        let info = FaultInfo { addr: REGION_BASE + PAGE * (k as u64 + 20), si_code: SEGV_MAPERR };
        if full.push(info).is_ok() {
            pushed = pushed.saturating_add(1);
        }
        k += 1;
    }
    let fifth = full.push(FaultInfo { addr: KERNEL_BASE, si_code: SEGV_MAPERR });
    s.add("容量：4 槽压满后第 5 次投递回 ENOBUFS",
          pushed == 4 && fifth == Err(ENOBUFS) && full.len() == 4
              && full.dropped() == 1 && full.delivered() == 4,
          "队列满必须显性回 ENOBUFS，投递/丢弃计数各记一笔");

    let mut fifo = SigQueue::new();
    let a = FaultInfo { addr: REGION_BASE + PAGE * 20, si_code: SEGV_MAPERR };
    let b = FaultInfo { addr: REGION_BASE + PAGE * 21, si_code: SEGV_ACCERR };
    let _ = fifo.push(a);
    let _ = fifo.push(b);
    let first_out = fifo.pop();
    let mut after = SigQueue::new();
    let _ = after.push(b);
    s.add("状态机：FIFO 出队后槽位可复用",
          first_out == Some(a) && fifo.len() == 1
              && fifo.slot(0) == Some(b)
              && fifo.push(a).is_ok()
              && after.delivered() == 1,
          "pop 必须按投递序出队并清空尾槽，容量随之恢复");

    let mut obs = SigQueue::new();
    let observed = trap_and_deliver(&space, &mut obs, REGION_BASE + PAGE * 8, Access::Write);
    let obs_ok = match observed.fault {
        Some(info) => {
            observed.delivered
                && obs.len() == 1
                && obs.slot(0) == Some(info)
                && info.si_code == SEGV_ACCERR
                && info.addr == REGION_BASE + PAGE * 8
        }
        None => false,
    };
    s.add("可观测：投递槽内容与故障逐字段一致", obs_ok,
          "槽内必须保留 (signo 语义, si_code, si_addr) 三元组");

    let codes_ok = agrees_with_linux(ENOBUFS)
        && agrees_with_linux(12)
        && linux_neg(ENOBUFS) == -ENOBUFS
        && !agrees_with_linux(0)
        && SIGSEGV == 11;
    s.add("Linux 逐码一致：ENOBUFS 表内可复算", codes_ok,
          "队列满错误码必须在 errno 表内，信号号固定为 11");

    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", fp_a == fp_b,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    s.add("文案闭集：12 个诊断码均有中文建议",
          SegvCode::ALL.iter().all(|c| !c.detail().is_empty()),
          "每个码必须能取到非空且可操作的说明");

    let mut uniq = true;
    let mut x = 0usize;
    while x < SegvCode::ALL.len() {
        let mut y = x + 1;
        while y < SegvCode::ALL.len() {
            if let (Some(p), Some(q)) = (SegvCode::ALL.get(x), SegvCode::ALL.get(y)) {
                if p.val() == q.val() {
                    uniq = false;
                }
            }
            y += 1;
        }
        x += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    let mut mono = ledger;
    let before = mono.injected;
    let mut n = 0u32;
    while n < 4 {
        let _ = mono.record(true);
        n += 1;
    }
    s.add("账目单调：再注入只增不减", mono.injected == before + 4,
          "累计计数禁止回退或饱和到旧值");

    let fault_text = format!(
        "故障注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("故障账目", ledger.conserved(), &fault_text);
    let queue_text = format!(
        "投递 {} / 丢弃 {} / 剩余 {}",
        full.delivered(), full.dropped(), full.len()
    );
    s.add_dyn("队列快照", full.len() == QUEUE_CAP, &queue_text);
    let fp_text = format!("语料指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legal_access_is_silent() {
        let space = AddressSpace::new();
        let mut q = SigQueue::new();
        let out = trap_and_deliver(&space, &mut q, REGION_BASE, Access::Write);
        assert!(out.fault.is_none());
        assert!(!out.delivered);
        assert_eq!(q.len(), 0);
    }

    #[test]
    fn guard_and_gap_maperr() {
        let space = AddressSpace::new();
        let mut q = SigQueue::new();
        let guard = trap_and_deliver(&space, &mut q, REGION_BASE + PAGE * 5, Access::Read);
        let gap = trap_and_deliver(&space, &mut q, REGION_BASE + PAGE * 12, Access::Read);
        assert_eq!(guard.fault, Some(FaultInfo { addr: REGION_BASE + PAGE * 5, si_code: SEGV_MAPERR }));
        assert_eq!(gap.fault, Some(FaultInfo { addr: REGION_BASE + PAGE * 12, si_code: SEGV_MAPERR }));
    }

    #[test]
    fn ro_write_accoerr() {
        let space = AddressSpace::new();
        let mut q = SigQueue::new();
        let out = trap_and_deliver(&space, &mut q, REGION_BASE + PAGE * 8, Access::Write);
        assert_eq!(out.fault, Some(FaultInfo { addr: REGION_BASE + PAGE * 8, si_code: SEGV_ACCERR }));
        assert!(out.delivered);
    }

    #[test]
    fn nx_exec_accoerr() {
        let space = AddressSpace::new();
        let mut q = SigQueue::new();
        let out = trap_and_deliver(&space, &mut q, REGION_BASE + PAGE * 8, Access::Exec);
        assert_eq!(out.fault, Some(FaultInfo { addr: REGION_BASE + PAGE * 8, si_code: SEGV_ACCERR }));
    }

    #[test]
    fn kernel_is_maperr() {
        let space = AddressSpace::new();
        let mut q = SigQueue::new();
        let out = trap_and_deliver(&space, &mut q, KERNEL_BASE + 0x100, Access::Read);
        assert_eq!(
            out.fault,
            Some(FaultInfo { addr: KERNEL_BASE + 0x100, si_code: SEGV_MAPERR })
        );
    }

    #[test]
    fn si_addr_not_page_aligned() {
        let space = AddressSpace::new();
        let mut q = SigQueue::new();
        let addr = REGION_BASE + PAGE * 5 + 0x123;
        let out = trap_and_deliver(&space, &mut q, addr, Access::Read);
        assert_eq!(out.fault.map(|f| f.addr), Some(addr));
    }

    #[test]
    fn queue_capacity_and_reuse() {
        let mut q = SigQueue::new();
        let info = FaultInfo { addr: REGION_BASE, si_code: SEGV_MAPERR };
        let mut i = 0u32;
        while i < 4 {
            assert!(q.push(info).is_ok());
            i += 1;
        }
        assert_eq!(q.push(info), Err(ENOBUFS));
        assert_eq!(q.dropped(), 1);
        let _ = q.pop();
        assert!(q.push(info).is_ok());
        assert_eq!(q.len(), 4);
    }

    #[test]
    fn matrix_conserved() {
        let (per, l) = run_fault_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 60);
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
        let s = run_unx9250_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
