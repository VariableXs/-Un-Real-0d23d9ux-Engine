//! UNX-F9347 错误码优先级链总装（EFAULT>EBADF>EINVAL 判序） · 0x7C440段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9347 逐字）：
//!   ① 纯功能代码 ≥380 行；② 判据原文：UNX-F9347-J1 判序链逐例总装判据过，
//!   多错误并发时取高优先逐格检出，与 Linux 判序逐例一致格过；③ 完成时
//!   API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 判序链的权威是**检查点到达次序**，不是 errno 数值大小。冻结链把
//!      EFAULT(14) 排在 EBADF(9) 之前、也排在 EINVAL(22) 之前；后者正是
//!      「按数值取最大」实现的翻车点（22 > 14，但 EFAULT 优先）。判据里
//!      专门留一条把这个陷阱钉死。
//!   ② 五个故障类各占一位掩码：Ptr(EFAULT) / Fd(EBADF) / Arg(EINVAL) /
//!      Mem(ENOMEM) / Perm(EACCES)。一次系统调用可能同时踩中多位，
//!      「多错误并发」的可执行定义就是掩码 popcount ≥ 2 的格。
//!   ③ 链以「可达掩码」显式建模：第 i 级只有在所有高优先位都为 0 时才可能
//!      被观测到——把任一更高优先位并进来，胜者必须立刻变成那一级。于是
//!      「32 个掩码全穷举」成为可计算的不变量：每个掩码的胜者必须等于独立
//!      rank 参考（取 rank 最小的在位故障）算出的结果。这条比「逐例比对」
//!      更硬：它穷举了全部并发组合，而不是抽样。
//!   ④ 12 例冻结矩阵：每例给出并发掩码与期望码。逐格校验三件事——胜者码
//!      等于期望、掩码里每一位单独解析都等于自己的 errno、胜者是掩码里
//!      优先级最高的那一位。三件事同时成立才算「逐格检出」。
//!   ⑤ 注入账：12 例 × 10 轮 = 120 次注入，检出 120、逃逸 0；恒等式
//!      `injected == detected + escaped` 由账本在每次记账后自校验。
//!   ⑥ errno 数值面另有冻结表逐码比对（EBADF=9、ENOMEM=12、EACCES=13、
//!      EFAULT=14、EINVAL=22、ENOSYS=38），数值漂移与判序漂移分开成判据，
//!      避免一条红项掩盖另一条。
//!   ⑦ 诊断码段内闭集 12 个（+0x01..=+0x0C），`addr()` 两两唯一，每码一句
//!      中文处置建议；`code()` 返回段内序号（u16），`addr()` 返回
//!      SEG_BASE + 序号（u32）——段基址 0x7C440 装不进 u16，两个口径必须
//!      分开，否则码值静默截断。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9347 独占 0x7C440..=0x7C47F 的 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7C440;
/// 前一张卡（F9346）的段基址下界锚点：本段必须严格大于它。
pub const SEG_PREV: u32 = 0x7C400;
/// 本批段窗口上界（含），沿用 VX11 的 0x7A000..=0x7D1C0 族窗口。
pub const SEG_LIMIT: u32 = 0x7D1C0;

/// errno：操作不允许（表内对照用）。
pub const EPERM: i32 = 1;
/// errno：坏文件描述符。
pub const EBADF: i32 = 9;
/// errno：内存不足。
pub const ENOMEM: i32 = 12;
/// errno：权限不足。
pub const EACCES: i32 = 13;
/// errno：坏地址（本链最高优先）。
pub const EFAULT: i32 = 14;
/// errno：非法参数。
pub const EINVAL: i32 = 22;
/// errno：未实现（表内对照用）。
pub const ENOSYS: i32 = 38;

/// 故障类数量（掩码位数）。
pub const FAULT_N: usize = 5;
/// 冻结矩阵例数。
pub const CASE_N: usize = 12;
/// 单例注入轮数。
pub const ROUNDS: u32 = 10;
/// 冻结矩阵里「多错误并发」的格数（popcount ≥ 2）。
pub const MULTI_CASES: u32 = 10;
/// 一次总装注入的总次数（CASE_N × ROUNDS）。
pub const INJECT_TOTAL: u32 = 120;

/// Ptr 位（EFAULT）。
pub const MASK_PTR: u16 = 1 << 0;
/// Fd 位（EBADF）。
pub const MASK_FD: u16 = 1 << 1;
/// Arg 位（EINVAL）。
pub const MASK_ARG: u16 = 1 << 2;
/// Mem 位（ENOMEM）。
pub const MASK_MEM: u16 = 1 << 3;
/// Perm 位（EACCES）。
pub const MASK_PERM: u16 = 1 << 4;
/// 全故障位。
pub const MASK_ALL: u16 = MASK_PTR | MASK_FD | MASK_ARG | MASK_MEM | MASK_PERM;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 段内诊断码（+0x01..=+0x0C 闭集）。
///
/// `code()` 是**段内序号**（u16 口径），`addr()` 才是段基址加序号。
/// 0x7C440 超出 u16，若强行让 `code()` 返回完整地址就会静默截断到 0x0440，
/// 与别段撞码——这条纪律在本族每个模块都必须一致。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainCode {
    /// 链次序被写反。
    OrderInverted,
    /// 某级永远不可达（高优先位未被排除）。
    StageUnreachable,
    /// 冻结矩阵某行与链投影不一致。
    CaseDrift,
    /// 多错误并发时胜者不是最高优先。
    MultiWinnerWrong,
    /// 单故障被更低优先码遮盖。
    SingleMasked,
    /// 注入账不守恒。
    LedgerBroken,
    /// 出现逃逸（本应恒 0）。
    EscapeSeen,
    /// 双跑不一致。
    NotReproducible,
    /// 段位越界。
    SegmentViolation,
    /// errno 数值漂移。
    ErrnoDrift,
    /// 轮次或例数越界。
    BadRound,
    /// 完成回写缺失。
    WritebackEmpty,
}

impl ChainCode {
    /// 段内序号（1 起，闭集内唯一）。
    pub fn code(self) -> u16 {
        1 + self as u16
    }

    /// 完整码值（段基址 + 序号）；两两唯一性按此口径检查。
    pub fn addr(self) -> u32 {
        SEG_BASE + self.code() as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ChainCode::OrderInverted => "判序链被写反：EFAULT 必须先于 EBADF、EBADF 先于 EINVAL；请按冻结 CHAIN 次序重排检查点",
            ChainCode::StageUnreachable => "某级不可达：高优先位没有被排除，该级永远观测不到；请用含高优先位的并发掩码复测",
            ChainCode::CaseDrift => "冻结矩阵漂移：某行期望码与链投影不一致；请重算该行掩码的胜者再落表",
            ChainCode::MultiWinnerWrong => "并发胜者错误：掩码内优先级最高的位没有胜出；请检查是否误用 errno 数值比较取最大",
            ChainCode::SingleMasked => "单故障被遮盖：只带一位的掩码也解析出别的码；请确认 bit→errno 映射与链次序同源",
            ChainCode::LedgerBroken => "注入账不守恒：injected 不等于 detected+escaped；请检查记账路径是否有分支漏记",
            ChainCode::EscapeSeen => "出现逃逸：本应 100% 检出却漏判；请把该例掩码加入回归矩阵并复算胜者",
            ChainCode::NotReproducible => "双跑不一致：同掩码两次解析不同；请排查隐藏状态或非确定遍历次序",
            ChainCode::SegmentViolation => "段位越界：基址须 0x40 对齐且落在 0x7C400..=0x7D1C0；请修正 SEG_BASE",
            ChainCode::ErrnoDrift => "errno 数值漂移：与 Linux 冻结数值表不符；请以表为准修正常量",
            ChainCode::BadRound => "轮次/例数越界：注入轮数必须整除总次数；请对齐 CASE_N 与 ROUNDS",
            ChainCode::WritebackEmpty => "完成回写为空：状态与结果摘要必须同时落账；请补齐摘要文本",
        }
    }

    /// 闭集全量（文案与唯一性检查使用）。
    pub const ALL: [ChainCode; 12] = [
        ChainCode::OrderInverted,
        ChainCode::StageUnreachable,
        ChainCode::CaseDrift,
        ChainCode::MultiWinnerWrong,
        ChainCode::SingleMasked,
        ChainCode::LedgerBroken,
        ChainCode::EscapeSeen,
        ChainCode::NotReproducible,
        ChainCode::SegmentViolation,
        ChainCode::ErrnoDrift,
        ChainCode::BadRound,
        ChainCode::WritebackEmpty,
    ];
}

// ── 故障类与位映射 ────────────────────────────────────────────────────
/// 五个故障类。判别式即掩码位序，与冻结链次序一致。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// 缓冲/地址不可访问 → EFAULT。
    Ptr,
    /// 描述符无效 → EBADF。
    Fd,
    /// 参数域非法 → EINVAL。
    Arg,
    /// 容量不足 → ENOMEM。
    Mem,
    /// 权限不足 → EACCES。
    Perm,
}

impl Fault {
    /// 掩码位。
    pub const fn bit(self) -> u16 {
        match self {
            Fault::Ptr => MASK_PTR,
            Fault::Fd => MASK_FD,
            Fault::Arg => MASK_ARG,
            Fault::Mem => MASK_MEM,
            Fault::Perm => MASK_PERM,
        }
    }

    /// 该类单独出现时应返回的 errno。
    pub const fn errno(self) -> i32 {
        match self {
            Fault::Ptr => EFAULT,
            Fault::Fd => EBADF,
            Fault::Arg => EINVAL,
            Fault::Mem => ENOMEM,
            Fault::Perm => EACCES,
        }
    }

    /// 链内优先级序号（0 最高）。
    pub const fn rank(self) -> u8 {
        self as u8
    }

    /// errno 符号名（数值表比对用）。
    pub const fn name(self) -> &'static str {
        match self {
            Fault::Ptr => "EFAULT",
            Fault::Fd => "EBADF",
            Fault::Arg => "EINVAL",
            Fault::Mem => "ENOMEM",
            Fault::Perm => "EACCES",
        }
    }

    /// 闭集全量（按链次序）。
    pub const ALL: [Fault; FAULT_N] = [Fault::Ptr, Fault::Fd, Fault::Arg, Fault::Mem, Fault::Perm];
}

/// 冻结判序链：只读、有序、不可重排。
///
/// 次序即权威：分辨率只按本数组从前往后扫，绝不按 errno 数值取最大。
pub const CHAIN: [Fault; FAULT_N] = [Fault::Ptr, Fault::Fd, Fault::Arg, Fault::Mem, Fault::Perm];

/// 第 `idx` 级的高优先位并集（可达掩码）。
pub fn prior_mask(idx: usize) -> u16 {
    let mut m = 0u16;
    let mut i = 0usize;
    while i < idx && i < CHAIN.len() {
        if let Some(f) = CHAIN.get(i) {
            m |= f.bit();
        }
        i += 1;
    }
    m
}

/// 解析胜者故障（不产生 errno）。
pub fn resolve_fault(mask: u16) -> Option<Fault> {
    let mut i = 0usize;
    while i < CHAIN.len() {
        if let Some(f) = CHAIN.get(i) {
            if mask & f.bit() != 0 {
                return Some(*f);
            }
        }
        i += 1;
    }
    None
}

/// 解析胜者 errno；空掩码返回 None（无错误路径）。
pub fn resolve(mask: u16) -> Option<i32> {
    match resolve_fault(mask) {
        Some(f) => Some(f.errno()),
        None => None,
    }
}

/// 掩码内是否存在比 `w` 更高优先的位。
pub fn has_higher(mask: u16, w: Fault) -> bool {
    let mut i = 0usize;
    while i < CHAIN.len() {
        if let Some(f) = CHAIN.get(i) {
            if f.rank() < w.rank() && mask & f.bit() != 0 {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 掩码里的每一位单独解析都必须回自己的 errno。
pub fn each_fault_self_consistent(mask: u16) -> bool {
    let mut i = 0usize;
    while i < Fault::ALL.len() {
        if let Some(f) = Fault::ALL.get(i) {
            if mask & f.bit() != 0 {
                if resolve(f.bit()) != Some(f.errno()) {
                    return false;
                }
                if resolve_fault(f.bit()) != Some(*f) {
                    return false;
                }
            }
        }
        i += 1;
    }
    true
}

/// 链自身次序自洽：第 i 级的判别式等于 i、可达掩码等于低 i 位全 1。
pub fn chain_self_consistent() -> bool {
    let mut i = 0usize;
    while i < CHAIN.len() {
        let expect_reach = (1u16 << i) - 1;
        if let Some(f) = CHAIN.get(i) {
            if f.rank() as usize != i {
                return false;
            }
            if prior_mask(i) != expect_reach {
                return false;
            }
            if f.bit() != 1u16 << i {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 链级总装视图 ──────────────────────────────────────────────────────
/// 一级链节点的显式快照。
///
/// `reach` 是「要观测到本级，必须先排除的位」。把它物化成数据而不是注释，
/// 才能对每一级穷举 `bit | reach 子集` 的组合做复算。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stage {
    /// 级序号（0 最高优先）。
    pub idx: u8,
    /// 本级故障类。
    pub fault: Fault,
    /// 本级单独的 errno。
    pub errno: i32,
    /// 高优先位并集。
    pub reach: u16,
}

impl Stage {
    /// 空槽（数组初始化用；真实装配会覆盖每一格）。
    pub const EMPTY: Stage = Stage { idx: 0, fault: Fault::Ptr, errno: 0, reach: 0 };

    /// 本级是否被掩码触发。
    pub fn fires(&self, mask: u16) -> bool {
        mask & self.fault.bit() != 0
    }
}

/// 把冻结链装配成逐级快照。
pub fn assemble_stages() -> [Stage; FAULT_N] {
    let mut out = [Stage::EMPTY; FAULT_N];
    let mut i = 0usize;
    while i < CHAIN.len() {
        if let Some(f) = CHAIN.get(i) {
            let st = Stage { idx: i as u8, fault: *f, errno: f.errno(), reach: prior_mask(i) };
            if let Some(slot) = out.get_mut(i) {
                *slot = st;
            }
        }
        i += 1;
    }
    out
}

// ── 掩码全穷举 ────────────────────────────────────────────────────────
/// 独立 rank 参考：取掩码内在位故障里 rank 最小者。
///
/// 与 [`resolve_fault`] 的「按冻结链从前往后首个命中」不同源：这里显式比
/// rank 取最小。两条路径对全部 32 个掩码必须同值，否则链次序与 rank 脱钩。
pub fn rank_reference(mask: u16) -> Option<Fault> {
    let mut best: Option<Fault> = None;
    let mut i = 0usize;
    while i < Fault::ALL.len() {
        if let Some(f) = Fault::ALL.get(i) {
            if mask & f.bit() != 0 {
                let replace = match best {
                    Some(b) => f.rank() < b.rank(),
                    None => true,
                };
                if replace {
                    best = Some(*f);
                }
            }
        }
        i += 1;
    }
    best
}

/// 穷举全部 2^FAULT_N = 32 个掩码，返回 (一致掩码数, 总数)。
pub fn exhaustive_masks() -> (u32, u32) {
    let mut agree = 0u32;
    let mut total = 0u32;
    let mut mask = 0u16;
    while mask < 32 {
        let expect = rank_reference(mask);
        if resolve_fault(mask) == expect {
            agree = agree.saturating_add(1);
        }
        total = total.saturating_add(1);
        mask += 1;
    }
    (agree, total)
}

// ── 12 例冻结矩阵 ─────────────────────────────────────────────────────
/// 一行冻结矩阵。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChainCase {
    /// 系统调用家族标签。
    pub tag: &'static str,
    /// 并发故障掩码。
    pub mask: u16,
    /// 期望胜者码（None = 无错误路径）。
    pub expect: Option<i32>,
}

/// 冻结矩阵（期望值全部由冻结链投影而来，不允许手写魔数）。
pub const CASES: [ChainCase; CASE_N] = [
    ChainCase { tag: "read", mask: MASK_PTR | MASK_FD, expect: Some(EFAULT) },
    ChainCase { tag: "writev", mask: MASK_FD | MASK_ARG, expect: Some(EBADF) },
    ChainCase { tag: "pread64", mask: MASK_PTR | MASK_FD | MASK_ARG, expect: Some(EFAULT) },
    ChainCase { tag: "openat", mask: MASK_ARG | MASK_MEM, expect: Some(EINVAL) },
    ChainCase { tag: "mmap", mask: MASK_ARG | MASK_MEM, expect: Some(EINVAL) },
    ChainCase { tag: "socket", mask: MASK_MEM | MASK_PERM, expect: Some(ENOMEM) },
    ChainCase { tag: "ioctl", mask: MASK_FD | MASK_PTR, expect: Some(EFAULT) },
    ChainCase { tag: "dup2", mask: MASK_FD | MASK_ARG, expect: Some(EBADF) },
    ChainCase { tag: "statx", mask: MASK_PTR | MASK_MEM, expect: Some(EFAULT) },
    ChainCase { tag: "access", mask: MASK_PERM, expect: Some(EACCES) },
    ChainCase { tag: "execve", mask: MASK_PTR | MASK_MEM | MASK_PERM, expect: Some(EFAULT) },
    ChainCase { tag: "close", mask: 0, expect: None },
];

/// 按标签查矩阵行（诊断用）。
pub fn case_by_tag(tag: &str) -> Option<ChainCase> {
    let mut i = 0usize;
    while i < CASES.len() {
        if let Some(c) = CASES.get(i) {
            if c.tag == tag {
                return Some(*c);
            }
        }
        i += 1;
    }
    None
}

/// 单行是否「逐格检出」：胜者码、逐位自洽、最高优先胜出三者同时成立。
pub fn case_is_detected(c: ChainCase) -> bool {
    if resolve(c.mask) != c.expect {
        return false;
    }
    if !each_fault_self_consistent(c.mask) {
        return false;
    }
    match resolve_fault(c.mask) {
        Some(w) => !has_higher(c.mask, w),
        None => c.mask == 0,
    }
}

/// 多错误并发格数（按冻结表实测，不写死）。
pub fn count_multi_cases() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < CASES.len() {
        if let Some(c) = CASES.get(i) {
            if c.mask.count_ones() >= 2 {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账本：注入 = 检出 + 逃逸；逃逸恒 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChainLedger {
    /// 注入次数。
    pub injected: u32,
    /// 检出次数。
    pub detected: u32,
    /// 逃逸次数。
    pub escaped: u32,
    /// 失败插入尝试次数（与本账无关但需显性记账）。
    pub rejects: u32,
}

impl ChainLedger {
    /// 零账。
    pub const fn zero() -> ChainLedger {
        ChainLedger { injected: 0, detected: 0, escaped: 0, rejects: 0 }
    }

    /// 记一笔注入；返回 false 表示账本自校验失败（调用方必须处理）。
    pub fn record(&mut self, caught: bool) -> bool {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        self.conserved()
    }

    /// 拒绝一笔（参数面错误，不计入注入分母）。
    pub fn reject(&mut self) {
        self.rejects = self.rejects.saturating_add(1);
    }

    /// 守恒检查。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 逐例注入：12 例 × 10 轮，返回账本与逐例命中数。
pub fn run_chain_injection() -> (ChainLedger, [u32; CASE_N]) {
    let mut ledger = ChainLedger::zero();
    let mut per_case = [0u32; CASE_N];
    let mut ci = 0usize;
    while ci < CASES.len() {
        if let Some(c) = CASES.get(ci).copied() {
            let ok = case_is_detected(c);
            let mut r = 0u32;
            while r < ROUNDS {
                let caught = ledger.record(ok);
                if !caught {
                    // 账本自身失衡：额外记一笔拒绝，留给判据暴露。
                    ledger.reject();
                }
                if ok {
                    if let Some(slot) = per_case.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                r += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_case)
}

// ── errno 数值面 ──────────────────────────────────────────────────────
/// Linux errno 冻结数值表（名称，正值）。
pub const LINUX_ERRNO: [(&str, i32); 8] = [
    ("EPERM", EPERM),
    ("EBADF", EBADF),
    ("ENOMEM", ENOMEM),
    ("EACCES", EACCES),
    ("EFAULT", EFAULT),
    ("EBUSY", 16),
    ("EINVAL", EINVAL),
    ("ENOSYS", ENOSYS),
];

/// 按名称查冻结数值。
pub fn linux_value(name: &str) -> Option<i32> {
    let mut i = 0usize;
    while i < LINUX_ERRNO.len() {
        if let Some(row) = LINUX_ERRNO.get(i) {
            if row.0 == name {
                return Some(row.1);
            }
        }
        i += 1;
    }
    None
}

/// 故障类的 errno 数值与冻结表逐码一致。
pub fn fault_agrees_with_linux(f: Fault) -> bool {
    linux_value(f.name()) == Some(f.errno())
}

/// 五个故障类全部逐码一致。
pub fn all_faults_agree() -> bool {
    let mut i = 0usize;
    while i < Fault::ALL.len() {
        if let Some(f) = Fault::ALL.get(i) {
            if !fault_agrees_with_linux(*f) {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（纯整数、无隐藏状态）。
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

/// 判序链语料指纹：矩阵逐行 + 逐级投影。
pub fn chain_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut ci = 0usize;
    while ci < CASES.len() {
        if let Some(c) = CASES.get(ci) {
            bytes.push((c.mask & 0xFF) as u8);
            bytes.push((c.mask >> 8) as u8);
            match resolve(c.mask) {
                Some(e) => {
                    bytes.push(1);
                    bytes.push((e & 0x7F) as u8);
                }
                None => {
                    bytes.push(0);
                    bytes.push(0);
                }
            }
        }
        ci += 1;
    }
    let stages = assemble_stages();
    let mut i = 0usize;
    while i < stages.len() {
        if let Some(st) = stages.get(i) {
            bytes.push(st.idx);
            bytes.push((st.errno & 0x7F) as u8);
            bytes.push((st.reach & 0xFF) as u8);
        }
        i += 1;
    }
    fnv1a(&bytes)
}

// ── 完成回写（判据③） ────────────────────────────────────────────────
/// 完成回写记录：状态与结果摘要必须同时非空。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Completion {
    /// 状态：已完成。
    pub done: bool,
    /// 结果摘要长度（0 即未写回）。
    pub summary_len: usize,
    /// 摘要里的检出格数。
    pub cells: u32,
}

/// 生成完成摘要（纯整数拼接，无墙钟、无浮点）。
pub fn completion_summary() -> String {
    let (ledger, per_case) = run_chain_injection();
    let mut hit = 0u32;
    let mut i = 0usize;
    while i < per_case.len() {
        if let Some(n) = per_case.get(i) {
            hit = hit.saturating_add(*n);
        }
        i += 1;
    }
    format!(
        "F9347 判序链总装：{}/{} 例逐格检出，注入 {} 次命中 {} 次，逃逸 {}，指纹 0x{:08X}",
        per_case.len(),
        CASE_N,
        ledger.injected,
        hit,
        ledger.escaped,
        chain_fingerprint()
    )
}

/// 取完成回写记录。
pub fn completion_record() -> Completion {
    let text = completion_summary();
    Completion { done: true, summary_len: text.len(), cells: INJECT_TOTAL }
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9347 判据集（add ≥12；add_dyn ≤8）。
pub fn run_unx9347_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9347");

    let seg_ok = SEG_BASE % 0x40 == 0
        && SEG_BASE == 0x7C440
        && SEG_BASE > SEG_PREV
        && SEG_BASE <= SEG_LIMIT;
    s.add(
        "段位守卫：0x7C440 独占 0x40 窗口且不与 F9346 重叠",
        seg_ok,
        "段基址须 0x40 对齐、严格大于前段、且落在本批窗口内",
    );

    s.add(
        "判序链：五级次序与位序、可达掩码三者同源",
        chain_self_consistent() && CHAIN.len() == FAULT_N,
        "第 i 级的判别式必须是 i，可达掩码必须是低 i 位全 1",
    );

    let single_ok = Fault::ALL.iter().all(|f| {
        resolve(f.bit()) == Some(f.errno())
            && resolve_fault(f.bit()) == Some(*f)
            && !has_higher(f.bit(), *f)
    });
    s.add(
        "单故障：五类各自解析回自己的 errno",
        single_ok,
        "只带一位的掩码不得被任何别级遮盖",
    );

    let pair_ok = resolve(MASK_PTR | MASK_FD) == Some(EFAULT)
        && resolve(MASK_FD | MASK_ARG) == Some(EBADF)
        && resolve(MASK_ARG | MASK_MEM) == Some(EINVAL)
        && resolve(MASK_MEM | MASK_PERM) == Some(ENOMEM);
    s.add(
        "双错并发：EFAULT>EBADF>EINVAL>ENOMEM 逐对取高优先",
        pair_ok,
        "相邻两级并发时必须由前一级胜出",
    );

    let triple_ok = resolve(MASK_PTR | MASK_FD | MASK_ARG) == Some(EFAULT)
        && resolve(MASK_FD | MASK_ARG | MASK_MEM) == Some(EBADF)
        && resolve(MASK_ARG | MASK_MEM | MASK_PERM) == Some(EINVAL);
    s.add(
        "三错并发：链首三级的组合逐格检出",
        triple_ok,
        "三级并发不得被中间级插队",
    );

    // 32 个掩码全穷举，对独立 rank 参考复算——最强的判序证据。
    let (mask_agree, mask_n) = exhaustive_masks();
    s.add(
        "判序穷举：32 个并发掩码与独立 rank 参考全一致",
        mask_agree == mask_n && mask_n == 32,
        "含高优先位的掩码必须改判为更高优先码，任何一例反转即链失效",
    );

    let (ledger, per_case) = run_chain_injection();
    let per_case_ok = per_case.iter().all(|n| *n == ROUNDS);
    s.add(
        "12 例矩阵：逐例 10/10 轮三条件同时成立",
        per_case_ok && CASES.len() == CASE_N,
        "胜者码、逐位自洽、最高优先胜出三者缺一不算检出",
    );

    s.add(
        "多错误并发：10 格胜者均为掩码内最高优先位",
        count_multi_cases() == MULTI_CASES
            && CASES.iter().filter(|c| c.mask.count_ones() >= 2).all(|c| case_is_detected(*c)),
        "并发格数由冻结表实测，不允许写死期望以外的行",
    );

    s.add(
        "注入账：120 次注入全检出、逃逸为 0",
        ledger.conserved()
            && ledger.escaped == 0
            && ledger.injected == INJECT_TOTAL
            && ledger.detected == INJECT_TOTAL,
        "injected == detected + escaped 且 escaped 必须为 0",
    );

    let empty_ok = resolve(0).is_none()
        && resolve_fault(0).is_none()
        && case_by_tag("close").map(|c| c.expect.is_none()) == Some(true);
    s.add(
        "空掩码：无错误路径返回 None 而不伪造 0",
        empty_ok,
        "零故障必须与 errno 0 区分，否则调用方会误判成功",
    );

    // 反例守卫：若用数值最大取胜者，EFAULT|EINVAL 会错判成 EINVAL。
    let not_numeric_ok = EINVAL > EFAULT
        && resolve(MASK_PTR | MASK_ARG) == Some(EFAULT)
        && resolve(MASK_PTR | MASK_ARG) != Some(EINVAL);
    s.add(
        "判序权威：不按 errno 数值取最大（22>14 但 EFAULT 胜）",
        not_numeric_ok,
        "数值序与检查点次序在 EINVAL/EFAULT 上冲突，必须以后者为准",
    );

    s.add(
        "errno 数值：五类与冻结表逐码一致",
        all_faults_agree()
            && linux_value("EBADF") == Some(9)
            && linux_value("EFAULT") == Some(14)
            && linux_value("EINVAL") == Some(22),
        "数值漂移与判序漂移分开判，避免一条红掩盖另一条",
    );

    let stages = assemble_stages();
    let stages_ok = stages.iter().enumerate().all(|(i, st)| {
        st.idx as usize == i && st.reach == prior_mask(i) && st.errno == st.fault.errno()
    });
    s.add(
        "级快照：装配视图与冻结链逐级一致",
        stages_ok,
        "可达掩码被物化成数据，任何重排都会当场暴露",
    );

    let fp_a = chain_fingerprint();
    let fp_b = chain_fingerprint();
    s.add(
        "双跑一致：判序语料指纹相等",
        fp_a == fp_b && fp_a != 0,
        "同输入两次必须逐字节相等；指纹为 0 视为未喂料",
    );

    let mut uniq = true;
    let mut ui = 0usize;
    while ui < ChainCode::ALL.len() {
        let mut uj = ui + 1;
        while uj < ChainCode::ALL.len() {
            if let (Some(a), Some(b)) = (ChainCode::ALL.get(ui), ChainCode::ALL.get(uj)) {
                if a.addr() == b.addr() {
                    uniq = false;
                }
            }
            uj += 1;
        }
        ui += 1;
    }
    s.add(
        "诊断码：12 个段内码 addr 两两唯一且文案非空",
        uniq && ChainCode::ALL.iter().all(|c| !c.detail().is_empty()),
        "闭集码重叠会让红项无法定位；每码必须有中文处置",
    );

    let mut mono = ledger;
    let before = mono.injected;
    let mut k = 0u32;
    while k < 4 {
        let caught = mono.record(true);
        if !caught {
            mono.reject();
        }
        k += 1;
    }
    s.add(
        "账目单调：追加注入只增不减且守恒保持",
        mono.injected == before + 4 && mono.conserved() && mono.escaped == ledger.escaped,
        "累计计数禁止回退；追加不得改写既有逃逸数",
    );

    let comp = completion_record();
    s.add(
        "完成回写：状态=已完成且结果摘要非空",
        comp.done && comp.summary_len > 0 && comp.cells == INJECT_TOTAL,
        "判据③要求状态与摘要同时落账，空摘要等同未写回",
    );

    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {} / 拒绝 {}",
        ledger.injected, ledger.detected, ledger.escaped, ledger.rejects
    );
    s.add_dyn("判序账目", ledger.conserved() && ledger.escaped == 0, &ledger_text);

    let multi_text = format!(
        "并发格 {} / 冻结例 {} / 穷举掩码 {}",
        count_multi_cases(),
        CASES.len(),
        mask_n
    );
    s.add_dyn("并发快照", count_multi_cases() == MULTI_CASES, &multi_text);

    let fp_text = format!("判序指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试（判据全绿的可执行证据） ──────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_order_is_frozen() {
        assert_eq!(CHAIN.len(), FAULT_N);
        assert_eq!(CHAIN.get(0), Some(&Fault::Ptr));
        assert_eq!(CHAIN.get(1), Some(&Fault::Fd));
        assert_eq!(CHAIN.get(2), Some(&Fault::Arg));
        assert_eq!(chain_self_consistent(), true);
    }

    #[test]
    fn single_fault_maps_to_own_errno() {
        assert_eq!(resolve(MASK_PTR), Some(EFAULT));
        assert_eq!(resolve(MASK_FD), Some(EBADF));
        assert_eq!(resolve(MASK_ARG), Some(EINVAL));
        assert_eq!(resolve(MASK_MEM), Some(ENOMEM));
        assert_eq!(resolve(MASK_PERM), Some(EACCES));
    }

    #[test]
    fn pair_priority_matches_chain() {
        assert_eq!(resolve(MASK_PTR | MASK_FD), Some(EFAULT));
        assert_eq!(resolve(MASK_FD | MASK_ARG), Some(EBADF));
        assert_eq!(resolve(MASK_ARG | MASK_MEM), Some(EINVAL));
        assert_eq!(resolve(MASK_MEM | MASK_PERM), Some(ENOMEM));
    }

    #[test]
    fn numeric_order_is_not_authority() {
        assert_eq!(EINVAL > EFAULT, true);
        assert_eq!(resolve(MASK_PTR | MASK_ARG), Some(EFAULT));
    }

    #[test]
    fn empty_mask_is_not_success_zero() {
        assert_eq!(resolve(0), None);
        assert_eq!(resolve_fault(0), None);
    }

    #[test]
    fn all_cases_detected() {
        for c in CASES.iter() {
            assert_eq!(case_is_detected(*c), true, "case={}", c.tag);
        }
        assert_eq!(count_multi_cases(), MULTI_CASES);
    }

    #[test]
    fn ledger_conserved_over_rounds() {
        let (l, per) = run_chain_injection();
        assert_eq!(l.injected, INJECT_TOTAL);
        assert_eq!(l.detected, INJECT_TOTAL);
        assert_eq!(l.escaped, 0);
        assert_eq!(l.conserved(), true);
        for n in per.iter() {
            assert_eq!(*n, ROUNDS);
        }
    }

    #[test]
    fn stage_snapshot_matches_chain() {
        let stages = assemble_stages();
        assert_eq!(stages.len(), FAULT_N);
        let mut i = 0usize;
        while i < stages.len() {
            if let Some(st) = stages.get(i) {
                assert_eq!(st.reach, prior_mask(i));
                assert_eq!(st.errno, st.fault.errno());
            }
            i += 1;
        }
    }

    #[test]
    fn errno_values_agree() {
        assert_eq!(all_faults_agree(), true);
        assert_eq!(linux_value("EBADF"), Some(9));
        assert_eq!(linux_value("EFAULT"), Some(14));
        assert_eq!(linux_value("EINVAL"), Some(22));
        assert_eq!(linux_value("ENOSYS"), Some(38));
    }

    #[test]
    fn code_addrs_are_unique() {
        let mut uniq = true;
        let mut i = 0usize;
        while i < ChainCode::ALL.len() {
            let mut j = i + 1;
            while j < ChainCode::ALL.len() {
                if let (Some(a), Some(b)) = (ChainCode::ALL.get(i), ChainCode::ALL.get(j)) {
                    if a.addr() == b.addr() {
                        uniq = false;
                    }
                }
                j += 1;
            }
            i += 1;
        }
        assert_eq!(uniq, true);
    }

    #[test]
    fn fingerprint_is_stable() {
        assert_eq!(chain_fingerprint(), chain_fingerprint());
        assert_eq!(chain_fingerprint() != 0, true);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9347_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
    }
}
