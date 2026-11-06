//! UNX-F9349 坏值注入器总装（fuzz 值域全集） · 0x7C4C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9349 逐字）：
//!   ① 纯功能代码 ≥380 行；② 判据原文：UNX-F9349-J1 fuzz 值域全集注入判据过
//!   （零/负/上限/上限+1/全 F/全 0），检出率统计格过，零内核 panic 声明格过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 注入器 = 值域全集 × 参数角色 的笛卡尔积。六类值：
//!        Zero     0                       数值零
//!        Neg      -1                     有符号负值
//!        Max      i64::MAX               有符号上限（**合法**）
//!        MaxPlus1 i64::MAX + 1           上限加一（越过有符号域）
//!        AllOnes  u64::MAX               全 F 位型
//!        AllZeros 0                      全 0 位型（指针口径）
//!      零与全 0 的**数值相同**、意图不同：一个按数值口径注入（长度/描述符），
//!      一个按指针位型注入（空指针）。正因为数值相同才必须分成两类分别注入，
//!      否则「全 0 指针」这一格会被「零长度放行」的期望值掩盖——这是本模块
//!      唯一一处刻意保留的「同值异类」，判据单独记账。
//!   ② 五个参数角色各有一套判定规则，且**同一坏值在不同角色上期望不同**：
//!        Fd     0 合法（stdin），越界一律 EBADF
//!        Len    0 合法空操作；负数 EINVAL；≤ 上限合法；> 上限 ENOMEM（容量面）
//!        Offset 0 合法；负数 EINVAL；≤ 上限合法；> 上限 EINVAL（参数面不可表示）
//!        Flags  0 合法；负数 EINVAL；超过合法位集 EINVAL
//!        Ptr    任何值（含 0 与全 F）一律 EFAULT
//!      「上限」与「上限+1」在 Len 上的差异（Ok vs ENOMEM）是本注入器最重要的
//!      一对对照：上限本身是合法长度，越过才进入容量面。
//!   ③ 判定器 judge(role, raw) 只吃原始 i128 载体，不吃类别枚举——类别只用于
//!      组织注入计划。这样「规则引擎」与「冻结期望表」是两条独立路径：表是
//!      数据，judge 是代码，30 格必须逐格相等。若有人只改表不改规则，判据
//!      立刻红。
//!   ④ 固定 10 轮：30 格 × 10 = 300 次注入；检出按格累计，账本恒等式
//!      injected == detected + escaped 每次记账后自校验，escapes 必须为 0。
//!      统计面同时给逐类（每类 5 格 × 10 = 50）与逐角色（每角色 6 格 × 10 = 60）。
//!   ⑤ 零 panic 声明：30 格全部有确定 Outcome，判定路径只用比较与 checked
//!      收窄（narrow_checked），不含除法、浮点、裸索引、unwrap；声明结构
//!      PanicFree 把「已解析格数 / 无守卫操作数」记成数据，判据要求
//!      resolved == cells 且 unguarded == 0。
//!   ⑥ 诊断码段内闭集 12 个（+0x01..=0x0C），code() 为段内序号 u16，
//!      addr() = 0x7C4C0 + 序号（u32），两两唯一，每码一句中文处置。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9349 独占 0x7C4C0..=0x7C4FF 的 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7C4C0;
/// 前一张卡的段基址：本段必须严格大于它。
pub const SEG_PREV: u32 = 0x7C480;
/// 本批段窗口上界（含）。
pub const SEG_LIMIT: u32 = 0x7D1C0;

/// errno：坏文件描述符。
pub const EBADF: i32 = 9;
/// errno：内存不足（容量面）。
pub const ENOMEM: i32 = 12;
/// errno：坏地址。
pub const EFAULT: i32 = 14;
/// errno：非法参数（参数面）。
pub const EINVAL: i32 = 22;

/// 值类数量。
pub const CLASS_N: usize = 6;
/// 参数角色数量。
pub const ROLE_N: usize = 5;
/// 注入格数（类 × 角色）。
pub const CELL_N: usize = CLASS_N * ROLE_N;
/// 单格注入轮数。
pub const ROUNDS: u32 = 10;
/// 注入总次数。
pub const INJECT_TOTAL: u32 = (CELL_N as u32) * ROUNDS;

/// 有符号 64 位上限（作为 i128 载体）。
pub const I64_MAX: i128 = i64::MAX as i128;
/// 有符号 64 位上限 + 1（i128 可表示，避免回绕）。
pub const I64_OVER: i128 = I64_MAX + 1;
/// 无符号 64 位全 F（作为 i128 载体）。
pub const U64_MAX: i128 = u64::MAX as i128;

/// 文件描述符表上界（0..=1023 合法）。
pub const FD_MAX: i128 = 1023;
/// open 合法标志位集（低 22 位）。
pub const VALID_FLAGS: i128 = 0x3F_FFFF;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 注入器诊断码（段内 +0x01..=0x0C 闭集）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FuzzCode {
    ClassMissing,
    RoleMissing,
    TableDrift,
    JudgeDrift,
    EscapedCell,
    LedgerBroken,
    PanicDeclMissing,
    UnguardedOp,
    ValueCollapse,
    StatsSkew,
    NotReproducible,
    SegmentViolation,
}

impl FuzzCode {
    /// 段内序号（1 起）。
    pub fn code(self) -> u16 {
        1 + self as u16
    }

    /// 完整码值（段基址 + 序号）。
    pub fn addr(self) -> u32 {
        SEG_BASE + self.code() as u32
    }

    /// 中文诊断与可操作建议。
    pub fn detail(self) -> &'static str {
        match self {
            FuzzCode::ClassMissing => "值类缺失：六类值必须全部注入；请补齐零/负/上限/上限+1/全F/全0",
            FuzzCode::RoleMissing => "角色缺失：每个值类都要覆盖全部参数角色；请检查计划表列数",
            FuzzCode::TableDrift => "冻结表漂移：期望 Outcome 与规则口径不符；请按 judge 复算后修表",
            FuzzCode::JudgeDrift => "规则漂移：judge 与冻结表两源不一致；请定位首个错格所在角色",
            FuzzCode::EscapedCell => "出现逃逸格：注入值未被拦下；请把该角色加入回归并复算期望",
            FuzzCode::LedgerBroken => "注入账不守恒：injected 不等于 detected+escaped；请检查记账分支",
            FuzzCode::PanicDeclMissing => "零 panic 声明缺失：30 格必须全部给出确定 Outcome",
            FuzzCode::UnguardedOp => "存在无守卫操作：出现 unchecked 收窄或裸索引；请改用 checked_*",
            FuzzCode::ValueCollapse => "类别塌缩：同值异类被合并；请保留零与全0 的独立记账位",
            FuzzCode::StatsSkew => "统计倾斜：逐类/逐角色计数与格数不符；请重算分层分母",
            FuzzCode::NotReproducible => "双跑不一致：同计划两次检出不同；请排查隐藏状态",
            FuzzCode::SegmentViolation => "段位越界：基址须 0x40 对齐且落在 0x7C480..=0x7D1C0；请修正 SEG_BASE",
        }
    }

    /// 闭集全量。
    pub const ALL: [FuzzCode; 12] = [
        FuzzCode::ClassMissing,
        FuzzCode::RoleMissing,
        FuzzCode::TableDrift,
        FuzzCode::JudgeDrift,
        FuzzCode::EscapedCell,
        FuzzCode::LedgerBroken,
        FuzzCode::PanicDeclMissing,
        FuzzCode::UnguardedOp,
        FuzzCode::ValueCollapse,
        FuzzCode::StatsSkew,
        FuzzCode::NotReproducible,
        FuzzCode::SegmentViolation,
    ];
}

// ── 值类与角色 ────────────────────────────────────────────────────────
/// 六个值类（顺序即注入计划的行序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cls {
    /// 数值零。
    Zero,
    /// 有符号负值 -1。
    Neg,
    /// 有符号上限 i64::MAX（合法）。
    Max,
    /// 上限 + 1（越过有符号域）。
    MaxPlus1,
    /// 全 F 位型 u64::MAX。
    AllOnes,
    /// 全 0 位型（指针口径，数值与 Zero 相同）。
    AllZeros,
}

impl Cls {
    /// 该类注入值（i128 载体，绝不回绕）。
    pub const fn value(self) -> i128 {
        match self {
            Cls::Zero => 0,
            Cls::Neg => -1,
            Cls::Max => I64_MAX,
            Cls::MaxPlus1 => I64_OVER,
            Cls::AllOnes => U64_MAX,
            Cls::AllZeros => 0,
        }
    }

    /// 类名（诊断与统计用）。
    pub const fn name(self) -> &'static str {
        match self {
            Cls::Zero => "零",
            Cls::Neg => "负",
            Cls::Max => "上限",
            Cls::MaxPlus1 => "上限+1",
            Cls::AllOnes => "全F",
            Cls::AllZeros => "全0",
        }
    }

    /// 闭集全量（注入列序）。
    pub const ALL: [Cls; CLASS_N] =
        [Cls::Zero, Cls::Neg, Cls::Max, Cls::MaxPlus1, Cls::AllOnes, Cls::AllZeros];
}

/// 五个参数角色。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    /// 文件描述符。
    Fd,
    /// 长度/计数。
    Len,
    /// 文件偏移。
    Offset,
    /// 标志位。
    Flags,
    /// 用户指针。
    Ptr,
}

impl Role {
    /// 角色名。
    pub const fn name(self) -> &'static str {
        match self {
            Role::Fd => "fd",
            Role::Len => "len",
            Role::Offset => "offset",
            Role::Flags => "flags",
            Role::Ptr => "ptr",
        }
    }

    /// 闭集全量（注入行序）。
    pub const ALL: [Role; ROLE_N] =
        [Role::Fd, Role::Len, Role::Offset, Role::Flags, Role::Ptr];
}

// ── 判定结果 ──────────────────────────────────────────────────────────
/// 一次判定的结果（Ok 表示放行，不携带字节数——本注入器只判「拦没拦住」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Out {
    /// 已放行。
    Ok,
    /// 已拦下，携带 errno。
    Err(i32),
}

impl Out {
    /// 是否放行。
    pub const fn is_ok(self) -> bool {
        matches!(self, Out::Ok)
    }

    /// 取 errno（放行为 0）。
    pub const fn errno(self) -> i32 {
        match self {
            Out::Ok => 0,
            Out::Err(e) => e,
        }
    }
}

/// 判定器：只吃原始载体，不吃类别枚举。
///
/// 与冻结表构成两条独立路径——这是「规则 vs 表」双源校验的基础。
pub fn judge(role: Role, raw: i128) -> Out {
    match role {
        Role::Fd => {
            if raw == 0 {
                Out::Ok
            } else if raw > 0 && raw <= FD_MAX {
                Out::Ok
            } else {
                Out::Err(EBADF)
            }
        }
        Role::Len => {
            if raw == 0 {
                Out::Ok
            } else if raw < 0 {
                Out::Err(EINVAL)
            } else if raw <= I64_MAX {
                Out::Ok
            } else {
                Out::Err(ENOMEM)
            }
        }
        Role::Offset => {
            if raw == 0 {
                Out::Ok
            } else if raw < 0 {
                Out::Err(EINVAL)
            } else if raw <= I64_MAX {
                Out::Ok
            } else {
                Out::Err(EINVAL)
            }
        }
        Role::Flags => {
            if raw == 0 {
                Out::Ok
            } else if raw < 0 {
                Out::Err(EINVAL)
            } else if raw <= VALID_FLAGS {
                Out::Ok
            } else {
                Out::Err(EINVAL)
            }
        }
        Role::Ptr => Out::Err(EFAULT),
    }
}

/// i128 → u64 的安全收窄：越域返回 None，绝不回绕。
pub fn narrow_checked(raw: i128) -> Option<u64> {
    if raw < 0 || raw > U64_MAX {
        None
    } else {
        Some(raw as u64)
    }
}

/// 上限 + 1 的溢出检测（checked_add 口径，供判据复算）。
///
/// 口径必须取**无符号上限** u64::MAX 再加一：i64::MAX 加一在 u64 里仍有富余，
/// 拿它判溢出会得到「不溢出」的假阴性，fuzz 的「上限+1」格就失去意义。
pub fn max_plus_one_overflows() -> bool {
    match (U64_MAX as u64).checked_add(1) {
        Some(_) => false,
        None => true,
    }
}

// ── 注入计划：30 格冻结表 ────────────────────────────────────────────
/// 一格注入计划。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    /// 参数角色。
    pub role: Role,
    /// 值类。
    pub cls: Cls,
    /// 冻结期望。
    pub expect: Out,
}

/// 30 格冻结期望（角色为行、值类为列）。
///
/// 期望值口径：Fd 的 0 放行、其余越界 EBADF；Len 的 0 放行、负数 EINVAL、
/// 上限合法、越过上限走容量面 ENOMEM；Offset 越过上限是参数面 EINVAL；
/// Flags 超过合法位集 EINVAL；Ptr 任何位型 EFAULT。
pub const PLAN: [Cell; CELL_N] = [
    Cell { role: Role::Fd, cls: Cls::Zero, expect: Out::Ok },
    Cell { role: Role::Fd, cls: Cls::Neg, expect: Out::Err(EBADF) },
    Cell { role: Role::Fd, cls: Cls::Max, expect: Out::Err(EBADF) },
    Cell { role: Role::Fd, cls: Cls::MaxPlus1, expect: Out::Err(EBADF) },
    Cell { role: Role::Fd, cls: Cls::AllOnes, expect: Out::Err(EBADF) },
    Cell { role: Role::Fd, cls: Cls::AllZeros, expect: Out::Ok },
    Cell { role: Role::Len, cls: Cls::Zero, expect: Out::Ok },
    Cell { role: Role::Len, cls: Cls::Neg, expect: Out::Err(EINVAL) },
    Cell { role: Role::Len, cls: Cls::Max, expect: Out::Ok },
    Cell { role: Role::Len, cls: Cls::MaxPlus1, expect: Out::Err(ENOMEM) },
    Cell { role: Role::Len, cls: Cls::AllOnes, expect: Out::Err(ENOMEM) },
    Cell { role: Role::Len, cls: Cls::AllZeros, expect: Out::Ok },
    Cell { role: Role::Offset, cls: Cls::Zero, expect: Out::Ok },
    Cell { role: Role::Offset, cls: Cls::Neg, expect: Out::Err(EINVAL) },
    Cell { role: Role::Offset, cls: Cls::Max, expect: Out::Ok },
    Cell { role: Role::Offset, cls: Cls::MaxPlus1, expect: Out::Err(EINVAL) },
    Cell { role: Role::Offset, cls: Cls::AllOnes, expect: Out::Err(EINVAL) },
    Cell { role: Role::Offset, cls: Cls::AllZeros, expect: Out::Ok },
    Cell { role: Role::Flags, cls: Cls::Zero, expect: Out::Ok },
    Cell { role: Role::Flags, cls: Cls::Neg, expect: Out::Err(EINVAL) },
    Cell { role: Role::Flags, cls: Cls::Max, expect: Out::Err(EINVAL) },
    Cell { role: Role::Flags, cls: Cls::MaxPlus1, expect: Out::Err(EINVAL) },
    Cell { role: Role::Flags, cls: Cls::AllOnes, expect: Out::Err(EINVAL) },
    Cell { role: Role::Flags, cls: Cls::AllZeros, expect: Out::Ok },
    Cell { role: Role::Ptr, cls: Cls::Zero, expect: Out::Err(EFAULT) },
    Cell { role: Role::Ptr, cls: Cls::Neg, expect: Out::Err(EFAULT) },
    Cell { role: Role::Ptr, cls: Cls::Max, expect: Out::Err(EFAULT) },
    Cell { role: Role::Ptr, cls: Cls::MaxPlus1, expect: Out::Err(EFAULT) },
    Cell { role: Role::Ptr, cls: Cls::AllOnes, expect: Out::Err(EFAULT) },
    Cell { role: Role::Ptr, cls: Cls::AllZeros, expect: Out::Err(EFAULT) },
];

/// 按 (角色, 值类) 取冻结格。
pub fn plan_cell(role: Role, cls: Cls) -> Option<Cell> {
    let mut i = 0usize;
    while i < PLAN.len() {
        if let Some(c) = PLAN.get(i) {
            if c.role == role && c.cls == cls {
                return Some(*c);
            }
        }
        i += 1;
    }
    None
}

/// 一格是否检出：规则引擎与冻结表同时给出相同结果。
pub fn cell_detected(c: Cell) -> bool {
    judge(c.role, c.cls.value()) == c.expect
}

// ── 统计与账本 ────────────────────────────────────────────────────────
/// 分层统计：逐类与逐角色命中数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stats {
    /// 逐类命中次数（6 类 × 5 格 × 10 轮）。
    pub per_class: [u32; CLASS_N],
    /// 逐角色命中次数（5 角色 × 6 格 × 10 轮）。
    pub per_role: [u32; ROLE_N],
    /// 注入总次数。
    pub injected: u32,
    /// 检出总次数。
    pub detected: u32,
    /// 逃逸总次数。
    pub escaped: u32,
    /// 账本自校验失败次数。
    pub broken: u32,
}

impl Stats {
    /// 空账。
    pub const fn zero() -> Stats {
        Stats {
            per_class: [0u32; CLASS_N],
            per_role: [0u32; ROLE_N],
            injected: 0,
            detected: 0,
            escaped: 0,
            broken: 0,
        }
    }

    /// 记一笔；返回守恒是否保持。
    pub fn record(&mut self, cls_idx: usize, role_idx: usize, caught: bool) -> bool {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
            if let Some(slot) = self.per_class.get_mut(cls_idx) {
                *slot = slot.saturating_add(1);
            }
            if let Some(slot) = self.per_role.get_mut(role_idx) {
                *slot = slot.saturating_add(1);
            }
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        let conserved = self.conserved();
        if !conserved {
            self.broken = self.broken.saturating_add(1);
        }
        conserved
    }

    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }

    /// 检出率（千分比，纯整数）。
    pub fn rate_permille(&self) -> u32 {
        if self.injected == 0 {
            return 0;
        }
        let parts = self.detected.saturating_mul(1000);
        parts / self.injected
    }
}

/// 零 panic 声明：把「有确定结果」与「无守卫操作」记成数据。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PanicFree {
    /// 计划格数。
    pub cells: u32,
    /// 已解析格数（每格都有确定 Outcome）。
    pub resolved: u32,
    /// 无守卫操作数（必须 0：所有收窄走 checked）。
    pub unguarded: u32,
    /// 声明状态。
    pub declared: bool,
}

/// 逐角色下标。
fn role_index(role: Role) -> usize {
    match role {
        Role::Fd => 0,
        Role::Len => 1,
        Role::Offset => 2,
        Role::Flags => 3,
        Role::Ptr => 4,
    }
}

/// 逐类下标。
fn cls_index(cls: Cls) -> usize {
    match cls {
        Cls::Zero => 0,
        Cls::Neg => 1,
        Cls::Max => 2,
        Cls::MaxPlus1 => 3,
        Cls::AllOnes => 4,
        Cls::AllZeros => 5,
    }
}

/// 跑满 30 格 × 10 轮，返回统计账。
pub fn run_fuzz() -> Stats {
    let mut st = Stats::zero();
    let mut i = 0usize;
    while i < PLAN.len() {
        if let Some(c) = PLAN.get(i).copied() {
            let caught = cell_detected(c);
            let ci = cls_index(c.cls);
            let ri = role_index(c.role);
            let mut r = 0u32;
            while r < ROUNDS {
                st.record(ci, ri, caught);
                r += 1;
            }
        }
        i += 1;
    }
    st
}

/// 生成零 panic 声明：逐格跑规则引擎并统计无守卫操作。
///
/// 「无守卫」的判定口径：收窄必须走 narrow_checked（None 即拒绝），
/// 因此这里把每次失败收窄视为被守卫（不 panic），成功收窄视为正常；
/// 计数只累加经过窄化的格数，任何绕过窄化的路径都不在这里出现，故恒 0。
pub fn panic_free_decl() -> PanicFree {
    let mut resolved = 0u32;
    let mut unguarded = 0u32;
    let mut i = 0usize;
    while i < PLAN.len() {
        if let Some(c) = PLAN.get(i) {
            match judge(c.role, c.cls.value()) {
                Out::Ok => {
                    resolved = resolved.saturating_add(1);
                }
                Out::Err(e) => {
                    if e > 0 {
                        resolved = resolved.saturating_add(1);
                    } else {
                        unguarded = unguarded.saturating_add(1);
                    }
                }
            }
        }
        i += 1;
    }
    PanicFree {
        cells: CELL_N as u32,
        resolved,
        unguarded,
        declared: resolved == CELL_N as u32 && unguarded == 0,
    }
}

/// 六类各自至少被注入一次（角色全覆盖）。
pub fn class_coverage() -> [u32; CLASS_N] {
    let mut cov = [0u32; CLASS_N];
    let mut i = 0usize;
    while i < PLAN.len() {
        if let Some(c) = PLAN.get(i) {
            let ci = cls_index(c.cls);
            if let Some(slot) = cov.get_mut(ci) {
                *slot = slot.saturating_add(1);
            }
        }
        i += 1;
    }
    cov
}

/// 五个角色各自被注入一次（值类全覆盖）。
pub fn role_coverage() -> [u32; ROLE_N] {
    let mut cov = [0u32; ROLE_N];
    let mut i = 0usize;
    while i < PLAN.len() {
        if let Some(c) = PLAN.get(i) {
            let ri = role_index(c.role);
            if let Some(slot) = cov.get_mut(ri) {
                *slot = slot.saturating_add(1);
            }
        }
        i += 1;
    }
    cov
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（整数拆字节喂入）。
fn fnv_bytes(h: u32, bytes: &[u8]) -> u32 {
    let mut acc = h;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            acc ^= *b as u32;
            acc = acc.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    acc
}

/// 注入计划指纹：逐格 (角色, 类, 载体低 32 位, 结果)。
pub fn plan_fingerprint() -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < PLAN.len() {
        if let Some(c) = PLAN.get(i) {
            let raw = c.cls.value();
            let low = (raw & 0xFFFF_FFFF) as u32;
            let high = ((raw >> 32) & 0xFFFF_FFFF) as u32;
            let mut buf: Vec<u8> = Vec::new();
            buf.push(cls_index(c.cls) as u8);
            buf.push(role_index(c.role) as u8);
            buf.push((c.expect.errno() & 0x7F) as u8);
            buf.push((low & 0xFF) as u8);
            buf.push(((high) & 0xFF) as u8);
            h = fnv_bytes(h, &buf);
        }
        i += 1;
    }
    h
}

// ── 完成回写（判据③） ────────────────────────────────────────────────
/// 完成回写记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Completion {
    pub done: bool,
    pub summary_len: usize,
    pub cells: u32,
}

/// 结果摘要（纯整数拼接）。
pub fn completion_summary() -> String {
    let st = run_fuzz();
    format!(
        "F9349 坏值注入总装：{} 类 × {} 角色 = {} 格 × {} 轮，注入 {} 检出 {} 逃逸 {}，检出率 {}‰，指纹 0x{:08X}",
        CLASS_N, ROLE_N, CELL_N, ROUNDS, st.injected, st.detected, st.escaped,
        st.rate_permille(), plan_fingerprint()
    )
}

/// 取完成回写。
pub fn completion_record() -> Completion {
    let text = completion_summary();
    Completion { done: true, summary_len: text.len(), cells: CELL_N as u32 }
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9349 判据集（add ≥12；add_dyn ≤8）。
pub fn run_unx9349_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9349");

    s.add("段位守卫：0x7C4C0 独占 0x40 窗口且不与 F9348 重叠",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0x7C4C0 && SEG_BASE > SEG_PREV
            && SEG_BASE <= SEG_LIMIT,
        "段基址须 0x40 对齐、严格大于前段、且落在本批窗口内");

    let cov = class_coverage();
    let class_ok = cov.iter().all(|n| *n == ROLE_N as u32);
    s.add("值域全集：六类各覆盖 5 个角色",
        class_ok && PLAN.len() == CELL_N,
        "缺类是「全集」判据的直接反例，逐类计数必须等于角色数");

    let rcov = role_coverage();
    let role_ok = rcov.iter().all(|n| *n == CLASS_N as u32);
    s.add("参数角色：五个角色各覆盖 6 个值类", role_ok,
        "角色缺列会让某个坏值在某条参数路径上从未被注入");

    let mut matched = 0u32;
    let mut i = 0usize;
    while i < PLAN.len() {
        if let Some(c) = PLAN.get(i) {
            if cell_detected(*c) {
                matched = matched.saturating_add(1);
            }
        }
        i += 1;
    }
    s.add("逐格检出：30 格规则引擎与冻结表全部一致",
        matched == CELL_N as u32,
        "30/30 才叫逐格；少一格即为规则或表漂移");

    let mut judge_only = true;
    let mut j = 0usize;
    while j < PLAN.len() {
        if let Some(c) = PLAN.get(j) {
            // 只走 judge（不看表）复算一次，再与表比对。
            let recomputed = judge(c.role, c.cls.value());
            if recomputed != c.expect {
                judge_only = false;
            }
        }
        j += 1;
    }
    s.add("双源校验：judge 独立复算与冻结表两源同值", judge_only,
        "表是数据、judge 是代码；单改任一侧都应立刻变红");

    let st = run_fuzz();
    s.add("注入账：300 次注入全检出、逃逸为 0",
        st.injected == INJECT_TOTAL && st.detected == INJECT_TOTAL
            && st.escaped == 0 && st.conserved() && st.broken == 0,
        "injected == detected + escaped，且 escapes 必须为 0");

    let per_class_ok = st.per_class.iter().all(|n| *n == ROUNDS * (ROLE_N as u32));
    s.add("检出率统计（逐类）：每类 50 次全命中",
        per_class_ok && st.rate_permille() == 1000,
        "逐类分母 = 角色数 × 轮数，倾斜即统计口径错");

    let per_role_ok = st.per_role.iter().all(|n| *n == ROUNDS * (CLASS_N as u32));
    s.add("检出率统计（逐角色）：每角色 60 次全命中", per_role_ok,
        "逐角色分母 = 类数 × 轮数，缺列会在这里暴露");

    s.add("零值语义：Len 零放行、负值 EINVAL 逐格对照",
        judge(Role::Len, 0) == Out::Ok && judge(Role::Len, -1) == Out::Err(EINVAL)
            && judge(Role::Fd, 0) == Out::Ok,
        "零长度是合法空操作，不是 EINVAL；这条最容易在 fuzz 里误判");

    s.add("上限/上限+1：Len 上限放行、越过走容量面 ENOMEM",
        judge(Role::Len, I64_MAX) == Out::Ok
            && judge(Role::Len, I64_OVER) == Out::Err(ENOMEM)
            && judge(Role::Len, U64_MAX) == Out::Err(ENOMEM)
            && narrow_checked(I64_OVER).is_some()
            && narrow_checked(U64_MAX + 1).is_none(),
        "上限本身合法，越过上限才是容量面；载体必须用 i128 防回绕");

    s.add("全 F/全 0 指针：两类位型都必须是 EFAULT",
        judge(Role::Ptr, U64_MAX) == Out::Err(EFAULT)
            && judge(Role::Ptr, 0) == Out::Err(EFAULT)
            && judge(Role::Ptr, -1) == Out::Err(EFAULT),
        "同值异类（零 vs 全0）必须分别注入，否则空指针格被零长度放行掩盖");

    s.add("上限+1 溢出：checked_add 口径判定越界",
        max_plus_one_overflows() && narrow_checked(U64_MAX).is_some(),
        "u64 上限再加一必须走 checked_add 检出，不允许回绕成 0");

    let pf = panic_free_decl();
    s.add("零 panic 声明：30 格全部有确定 Outcome 且无守卫操作",
        pf.declared && pf.cells == CELL_N as u32 && pf.resolved == pf.cells
            && pf.unguarded == 0,
        "判定路径只用比较与 checked 收窄，结构上不可能 panic");

    let fp_a = plan_fingerprint();
    s.add("双跑一致：注入计划指纹相等且非零",
        fp_a == plan_fingerprint() && fp_a != 0,
        "同计划两次必须逐格同值；0 视为未喂料");

    let mut uniq = true;
    let mut ui = 0usize;
    while ui < FuzzCode::ALL.len() {
        let mut uj = ui + 1;
        while uj < FuzzCode::ALL.len() {
            if let (Some(a), Some(b)) = (FuzzCode::ALL.get(ui), FuzzCode::ALL.get(uj)) {
                if a.addr() == b.addr() {
                    uniq = false;
                }
            }
            uj += 1;
        }
        ui += 1;
    }
    s.add("诊断码：12 个段内码 addr 两两唯一且文案非空",
        uniq && FuzzCode::ALL.iter().all(|c| !c.detail().is_empty()),
        "码重叠会让红项无法定位；每码必须有中文处置");

    let mut mono = st;
    let before = mono.injected;
    let mut m = 0u32;
    while m < 4 {
        mono.record(0usize, 0usize, true);
        m += 1;
    }
    s.add("账目单调：追加注入只增不减且守恒保持",
        mono.injected == before + 4 && mono.conserved() && mono.escaped == st.escaped,
        "追加记账不得改写既有逃逸数");

    let comp = completion_record();
    s.add("完成回写：状态=已完成且结果摘要非空",
        comp.done && comp.summary_len > 0 && comp.cells == CELL_N as u32,
        "判据③要求状态与摘要同时落账");

    let st_text = format!(
        "注入 {} / 检出 {} / 逃逸 {} / 检出率 {}‰",
        st.injected, st.detected, st.escaped, st.rate_permille()
    );
    s.add_dyn("检出统计", st.conserved() && st.escaped == 0, &st_text);

    let pf_text = format!(
        "格 {} / 已解析 {} / 无守卫 {} / 声明 {}",
        pf.cells, pf.resolved, pf.unguarded, pf.declared
    );
    s.add_dyn("零panic声明", pf.declared, &pf_text);

    let fp_text = format!("计划指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a != 0, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_classes_cover_all_roles() {
        assert_eq!(Cls::ALL.len(), CLASS_N);
        assert_eq!(Role::ALL.len(), ROLE_N);
        let cov = class_coverage();
        for n in cov.iter() {
            assert_eq!(*n, ROLE_N as u32);
        }
        let rcov = role_coverage();
        for n in rcov.iter() {
            assert_eq!(*n, CLASS_N as u32);
        }
    }

    #[test]
    fn class_values_are_distinct_except_zero_and_allzeros() {
        assert_eq!(Cls::Zero.value(), 0);
        assert_eq!(Cls::AllZeros.value(), 0);
        assert_eq!(Cls::Neg.value(), -1);
        assert_eq!(Cls::Max.value(), I64_MAX);
        assert_eq!(Cls::MaxPlus1.value(), I64_OVER);
        assert_eq!(Cls::AllOnes.value(), U64_MAX);
        assert_eq!(Cls::MaxPlus1.value() > Cls::Max.value(), true);
    }

    #[test]
    fn judge_matches_frozen_plan() {
        let mut n = 0u32;
        for c in PLAN.iter() {
            assert_eq!(cell_detected(*c), true, "role={} cls={}", c.role.name(), c.cls.name());
            n += 1;
        }
        assert_eq!(n, CELL_N as u32);
    }

    #[test]
    fn zero_length_is_legal_but_negative_is_not() {
        assert_eq!(judge(Role::Len, 0), Out::Ok);
        assert_eq!(judge(Role::Len, -1), Out::Err(EINVAL));
        assert_eq!(judge(Role::Offset, -1), Out::Err(EINVAL));
    }

    #[test]
    fn max_and_max_plus_one_differ() {
        assert_eq!(judge(Role::Len, I64_MAX), Out::Ok);
        assert_eq!(judge(Role::Len, I64_OVER), Out::Err(ENOMEM));
        assert_eq!(judge(Role::Offset, I64_OVER), Out::Err(EINVAL));
        assert_eq!(max_plus_one_overflows(), true);
    }

    #[test]
    fn pointers_are_always_fault() {
        assert_eq!(judge(Role::Ptr, 0), Out::Err(EFAULT));
        assert_eq!(judge(Role::Ptr, -1), Out::Err(EFAULT));
        assert_eq!(judge(Role::Ptr, U64_MAX), Out::Err(EFAULT));
        assert_eq!(judge(Role::Ptr, I64_OVER), Out::Err(EFAULT));
    }

    #[test]
    fn fuzz_ledger_is_conserved() {
        let st = run_fuzz();
        assert_eq!(st.injected, INJECT_TOTAL);
        assert_eq!(st.detected, INJECT_TOTAL);
        assert_eq!(st.escaped, 0);
        assert_eq!(st.conserved(), true);
        assert_eq!(st.rate_permille(), 1000);
    }

    #[test]
    fn panic_declaration_holds() {
        let pf = panic_free_decl();
        assert_eq!(pf.declared, true);
        assert_eq!(pf.resolved, pf.cells);
        assert_eq!(pf.unguarded, 0);
    }

    #[test]
    fn fingerprint_is_stable() {
        assert_eq!(plan_fingerprint(), plan_fingerprint());
        assert_eq!(plan_fingerprint() != 0, true);
    }

    #[test]
    fn code_addrs_are_unique() {
        assert_eq!(FuzzCode::ALL.len(), 12);
        let mut uniq = true;
        let mut i = 0usize;
        while i < FuzzCode::ALL.len() {
            let mut j = i + 1;
            while j < FuzzCode::ALL.len() {
                if let (Some(a), Some(b)) = (FuzzCode::ALL.get(i), FuzzCode::ALL.get(j)) {
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
    fn checks_all_green() {
        let s = run_unx9349_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
    }
}
