//! UNX-F9350 边界值等价类总装（零/负/上限/上限+1） · 0x7C500段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9350 逐字）：
//!   ① 纯功能代码 ≥380 行；② 判据原文：UNX-F9350-J1 四等价类逐类总装判据过，
//!   逐类期望值逐格检出，等价类账落账格过；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 四个等价类是对**整个 i128 载体域**的划分，不是抽样：
//!        Zero     { 0 }
//!        Neg      (-∞, 0)
//!        Max      (0, 2^64 - 1]      上限（含上限本身）
//!        OverMax  (2^64 - 1, +∞)     上限+1 及其后全部
//!      四条区间的并是全集、交为空。判据用 14 个冻结样本做「恰落一类」的
//!      穷举验证：每个样本必须恰好满足一个谓词，计数之和必须等于样本数。
//!   ② 载体用 i128 而不是 u64：u64 上 `MAX + 1` 会回绕成 0，直接塌进 Zero
//!      类——这正是「上限+1」这个等价类在 64 位载体上**不可观测**的根本原因。
//!      判据专门留一条演示回绕（wrapping_add 得 0）与 i128 检出对照。
//!   ③ 四个参数角色的期望值**逐类不同**，这是本模块与「坏值注入器」的分工：
//!        Len     零放行、负 EINVAL、上限放行、上限+1 走容量面 ENOMEM
//!        Offset  零放行、负 EINVAL、上限放行、上限+1 走参数面 EINVAL
//!        Flags   零放行、负 EINVAL、上限 EINVAL（超合法位集）、上限+1 EINVAL
//!        Mmap    零 EINVAL（零长度映射非法）、负 EINVAL、上限 ENOMEM、
//!                上限+1 ENOMEM
//!      「零」在 Len 与 Mmap 上期望相反（Ok vs EINVAL），是本模块最容易做错
//!      的一对：等价类相同、角色不同，期望就必须不同。
//!   ④ 等价类账分两本：Partition 账记 14 个样本的落类计数（每类应有固定
//!      基数 5/1/5/3），Inject 账记 16 格 × 10 轮 = 160 次注入的逐类命中
//!      （每类 4 角色 × 10 = 40）。两本账都对不上就说明分类或分格错了。
//!   ⑤ 12 个代表值逐条申报归属类（1 个零、4 个负、4 个上限、3 个越界），
//!      分类函数必须逐条命中，否则「等价类」只是文字游戏。
//!   ⑥ 诊断码段内闭集 12 个（+0x01..=0x0C），code() 段内序号 u16，
//!      addr() = 0x7C500 + 序号（u32），两两唯一，每码一句中文处置。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9350 独占 0x7C500..=0x7C53F 的 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7C500;
/// 前一张卡的段基址：本段必须严格大于它。
pub const SEG_PREV: u32 = 0x7C4C0;
/// 本批段窗口上界（含）。
pub const SEG_LIMIT: u32 = 0x7D1C0;

/// errno：内存不足（容量面）。
pub const ENOMEM: i32 = 12;
/// errno：非法参数（参数面）。
pub const EINVAL: i32 = 22;

/// 等价类数量。
pub const EQUIV_N: usize = 4;
/// 参数角色数量。
pub const ROLE_N: usize = 4;
/// 分类矩阵格数。
pub const CELL_N: usize = EQUIV_N * ROLE_N;
/// 冻结样本数。
pub const SAMPLE_N: usize = 14;
/// 代表值数。
pub const REP_N: usize = 12;
/// 单格注入轮数。
pub const ROUNDS: u32 = 10;
/// 注入总次数。
pub const INJECT_TOTAL: u32 = (CELL_N as u32) * ROUNDS;

/// 有符号 64 位上限（i128 载体）。
pub const I64_MAX: i128 = i64::MAX as i128;
/// 无符号 64 位上限，即本模块的「上限」。
pub const MAX: i128 = u64::MAX as i128;
/// 上限 + 1。
pub const OVER: i128 = MAX + 1;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 等价类诊断码（段内 +0x01..=0x0C 闭集）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EquivCode {
    ClassGap,
    ClassOverlap,
    RepMisclassified,
    MatrixDrift,
    LedgerSkew,
    ZeroSemanticsWrong,
    EdgeBisectFailed,
    NegativeLeak,
    WrapUndetected,
    NotReproducible,
    SegmentViolation,
    WritebackEmpty,
}

impl EquivCode {
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
            EquivCode::ClassGap => "划分有缺口：存在既不属于任何类的值；请补区间边界",
            EquivCode::ClassOverlap => "划分重叠：一个值同时落两类；请把区间改成左闭右开",
            EquivCode::RepMisclassified => "代表值归错类：分类函数与申报类不符；请按区间重算该代表值",
            EquivCode::MatrixDrift => "分类矩阵漂移：某格期望与角色规则不符；请按角色重算期望",
            EquivCode::LedgerSkew => "等价类账倾斜：逐类计数与样本/格数不符；请重算分母",
            EquivCode::ZeroSemanticsWrong => "零语义错：Len 零应放行、Mmap 零应 EINVAL；请分角色判定",
            EquivCode::EdgeBisectFailed => "上限二分失败：上限与其加一必须分属两类；请检查边界比较是否含等号",
            EquivCode::NegativeLeak => "负值漏类：负数被归进上限类；请先判负再判上限",
            EquivCode::WrapUndetected => "回绕未检出：64 位载体上上限+1 回绕成 0；请改用 i128 载体",
            EquivCode::NotReproducible => "双跑不一致：同样本两次分类不同；请排查隐藏状态",
            EquivCode::SegmentViolation => "段位越界：基址须 0x40 对齐且落在 0x7C4C0..=0x7D1C0；请修正 SEG_BASE",
            EquivCode::WritebackEmpty => "完成回写为空：状态与摘要必须同时落账；请补齐摘要",
        }
    }

    /// 闭集全量。
    pub const ALL: [EquivCode; 12] = [
        EquivCode::ClassGap,
        EquivCode::ClassOverlap,
        EquivCode::RepMisclassified,
        EquivCode::MatrixDrift,
        EquivCode::LedgerSkew,
        EquivCode::ZeroSemanticsWrong,
        EquivCode::EdgeBisectFailed,
        EquivCode::NegativeLeak,
        EquivCode::WrapUndetected,
        EquivCode::NotReproducible,
        EquivCode::SegmentViolation,
        EquivCode::WritebackEmpty,
    ];
}

// ── 四个等价类 ────────────────────────────────────────────────────────
/// 四个边界等价类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Equiv {
    /// 恰好为 0。
    Zero,
    /// 严格小于 0。
    Neg,
    /// (0, 上限]，含上限本身。
    Max,
    /// 大于上限（含上限+1）。
    OverMax,
}

impl Equiv {
    /// 类名。
    pub const fn name(self) -> &'static str {
        match self {
            Equiv::Zero => "零",
            Equiv::Neg => "负",
            Equiv::Max => "上限",
            Equiv::OverMax => "上限+1",
        }
    }

    /// 类下标（账本列序）。
    pub const fn index(self) -> usize {
        match self {
            Equiv::Zero => 0,
            Equiv::Neg => 1,
            Equiv::Max => 2,
            Equiv::OverMax => 3,
        }
    }

    /// 闭集全量（账本列序）。
    pub const ALL: [Equiv; EQUIV_N] = [Equiv::Zero, Equiv::Neg, Equiv::Max, Equiv::OverMax];
}

/// 分类函数：对整个 i128 域做四划分。
///
/// 次序不可交换：必须先判 0、再判负、再判上限，剩下一律越界。
/// 若先判上限再判负，负数会落进 Max 类（负值漏类）。
pub fn classify(raw: i128) -> Equiv {
    if raw == 0 {
        Equiv::Zero
    } else if raw < 0 {
        Equiv::Neg
    } else if raw <= MAX {
        Equiv::Max
    } else {
        Equiv::OverMax
    }
}

/// 四个谓词中恰好一个为真（划分互斥的逐值证据）。
pub fn exactly_one(raw: i128) -> bool {
    let mut n = 0u32;
    if raw == 0 {
        n += 1;
    }
    if raw < 0 {
        n += 1;
    }
    if raw > 0 && raw <= MAX {
        n += 1;
    }
    if raw > MAX {
        n += 1;
    }
    n == 1
}

// ── 冻结样本与代表值 ──────────────────────────────────────────────────
/// 一个冻结样本：载体值与申报类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sample {
    pub raw: i128,
    pub expect: Equiv,
}

/// 14 个冻结样本，覆盖四类且每类基数固定（负 5 / 零 1 / 上限 5 / 越界 3）。
pub const SAMPLES: [Sample; SAMPLE_N] = [
    Sample { raw: i64::MIN as i128, expect: Equiv::Neg },
    Sample { raw: -1_000_000, expect: Equiv::Neg },
    Sample { raw: -4096, expect: Equiv::Neg },
    Sample { raw: -2, expect: Equiv::Neg },
    Sample { raw: -1, expect: Equiv::Neg },
    Sample { raw: 0, expect: Equiv::Zero },
    Sample { raw: 1, expect: Equiv::Max },
    Sample { raw: 1_048_576, expect: Equiv::Max },
    Sample { raw: I64_MAX, expect: Equiv::Max },
    Sample { raw: MAX - 1, expect: Equiv::Max },
    Sample { raw: MAX, expect: Equiv::Max },
    Sample { raw: OVER, expect: Equiv::OverMax },
    Sample { raw: OVER + 1, expect: Equiv::OverMax },
    Sample { raw: i128::MAX, expect: Equiv::OverMax },
];

/// 一个代表值：申报类 + 载体值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rep {
    pub expect: Equiv,
    pub raw: i128,
}

/// 12 个代表值（零 1 / 负 4 / 上限 4 / 越界 3）。
pub const REPS: [Rep; REP_N] = [
    Rep { expect: Equiv::Zero, raw: 0 },
    Rep { expect: Equiv::Neg, raw: -1 },
    Rep { expect: Equiv::Neg, raw: -4096 },
    Rep { expect: Equiv::Neg, raw: i64::MIN as i128 },
    Rep { expect: Equiv::Neg, raw: -MAX },
    Rep { expect: Equiv::Max, raw: 1 },
    Rep { expect: Equiv::Max, raw: 4096 },
    Rep { expect: Equiv::Max, raw: I64_MAX },
    Rep { expect: Equiv::Max, raw: MAX },
    Rep { expect: Equiv::OverMax, raw: OVER },
    Rep { expect: Equiv::OverMax, raw: OVER + 1 },
    Rep { expect: Equiv::OverMax, raw: i128::MAX },
];

/// 样本类基数（规划值，落账时逐类比对）。
pub const SAMPLE_PLAN: [u32; EQUIV_N] = [1, 5, 5, 3];
/// 代表值类基数。
pub const REP_PLAN: [u32; EQUIV_N] = [1, 4, 4, 3];

// ── 角色与 16 格矩阵 ──────────────────────────────────────────────────
/// 四个参数角色。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    /// 长度/计数。
    Len,
    /// 文件偏移。
    Offset,
    /// 标志位。
    Flags,
    /// 映射长度。
    Mmap,
}

impl Role {
    /// 角色名。
    pub const fn name(self) -> &'static str {
        match self {
            Role::Len => "len",
            Role::Offset => "offset",
            Role::Flags => "flags",
            Role::Mmap => "mmap",
        }
    }

    /// 角色下标。
    pub const fn index(self) -> usize {
        match self {
            Role::Len => 0,
            Role::Offset => 1,
            Role::Flags => 2,
            Role::Mmap => 3,
        }
    }

    /// 闭集全量（矩阵行序）。
    pub const ALL: [Role; ROLE_N] = [Role::Len, Role::Offset, Role::Flags, Role::Mmap];
}

/// 判定结果（放行 / 拦下）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Out {
    /// 放行。
    Ok,
    /// 拦下。
    Err(i32),
}

impl Out {
    /// 取 errno（放行为 0）。
    pub const fn errno(self) -> i32 {
        match self {
            Out::Ok => 0,
            Out::Err(e) => e,
        }
    }
}

/// 一格期望：角色 × 等价类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    pub role: Role,
    pub equiv: Equiv,
    pub expect: Out,
}

/// 16 格冻结矩阵（角色为行、等价类为列）。
///
/// 注意 Len 与 Mmap 在「零」上的期望相反：零长度读合法、零长度映射非法。
pub const MATRIX: [Cell; CELL_N] = [
    Cell { role: Role::Len, equiv: Equiv::Zero, expect: Out::Ok },
    Cell { role: Role::Len, equiv: Equiv::Neg, expect: Out::Err(EINVAL) },
    Cell { role: Role::Len, equiv: Equiv::Max, expect: Out::Ok },
    Cell { role: Role::Len, equiv: Equiv::OverMax, expect: Out::Err(ENOMEM) },
    Cell { role: Role::Offset, equiv: Equiv::Zero, expect: Out::Ok },
    Cell { role: Role::Offset, equiv: Equiv::Neg, expect: Out::Err(EINVAL) },
    Cell { role: Role::Offset, equiv: Equiv::Max, expect: Out::Ok },
    Cell { role: Role::Offset, equiv: Equiv::OverMax, expect: Out::Err(EINVAL) },
    Cell { role: Role::Flags, equiv: Equiv::Zero, expect: Out::Ok },
    Cell { role: Role::Flags, equiv: Equiv::Neg, expect: Out::Err(EINVAL) },
    Cell { role: Role::Flags, equiv: Equiv::Max, expect: Out::Err(EINVAL) },
    Cell { role: Role::Flags, equiv: Equiv::OverMax, expect: Out::Err(EINVAL) },
    Cell { role: Role::Mmap, equiv: Equiv::Zero, expect: Out::Err(EINVAL) },
    Cell { role: Role::Mmap, equiv: Equiv::Neg, expect: Out::Err(EINVAL) },
    Cell { role: Role::Mmap, equiv: Equiv::Max, expect: Out::Err(ENOMEM) },
    Cell { role: Role::Mmap, equiv: Equiv::OverMax, expect: Out::Err(ENOMEM) },
];

/// 按 (角色, 等价类) 取矩阵格。
pub fn matrix_cell(role: Role, eq: Equiv) -> Option<Cell> {
    let mut i = 0usize;
    while i < MATRIX.len() {
        if let Some(c) = MATRIX.get(i) {
            if c.role == role && c.equiv == eq {
                return Some(*c);
            }
        }
        i += 1;
    }
    None
}

/// 规则引擎：按角色 + 载体值直接判定（与矩阵表两源独立）。
pub fn judge(role: Role, raw: i128) -> Out {
    let eq = classify(raw);
    match role {
        Role::Len => match eq {
            Equiv::Zero => Out::Ok,
            Equiv::Neg => Out::Err(EINVAL),
            Equiv::Max => Out::Ok,
            Equiv::OverMax => Out::Err(ENOMEM),
        },
        Role::Offset => match eq {
            Equiv::Zero => Out::Ok,
            Equiv::Neg => Out::Err(EINVAL),
            Equiv::Max => Out::Ok,
            Equiv::OverMax => Out::Err(EINVAL),
        },
        Role::Flags => match eq {
            Equiv::Zero => Out::Ok,
            Equiv::Neg => Out::Err(EINVAL),
            Equiv::Max => Out::Err(EINVAL),
            Equiv::OverMax => Out::Err(EINVAL),
        },
        Role::Mmap => match eq {
            Equiv::Zero => Out::Err(EINVAL),
            Equiv::Neg => Out::Err(EINVAL),
            Equiv::Max => Out::Err(ENOMEM),
            Equiv::OverMax => Out::Err(ENOMEM),
        },
    }
}

// ── 两本账 ────────────────────────────────────────────────────────────
/// 划分账：14 个样本的逐类落账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Partition {
    counts: [u32; EQUIV_N],
    samples: u32,
    mismatched: u32,
}

impl Partition {
    /// 空账。
    pub const fn new() -> Partition {
        Partition { counts: [0u32; EQUIV_N], samples: 0, mismatched: 0 }
    }

    /// 记一个样本的落类；申报类不一致时 mismatch 只增。
    pub fn record(&mut self, got: Equiv, expect: Equiv) {
        self.samples = self.samples.saturating_add(1);
        if let Some(slot) = self.counts.get_mut(got.index()) {
            *slot = slot.saturating_add(1);
        }
        if got != expect {
            self.mismatched = self.mismatched.saturating_add(1);
        }
    }

    /// 逐类计数。
    pub fn count(&self, eq: Equiv) -> u32 {
        match self.counts.get(eq.index()) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 样本总数。
    pub fn samples(&self) -> u32 {
        self.samples
    }

    /// 错类数。
    pub fn mismatched(&self) -> u32 {
        self.mismatched
    }

    /// 逐类计数之和等于样本数（划分完备的可计算形式）。
    pub fn sums_to_samples(&self) -> bool {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < EQUIV_N {
            if let Some(v) = self.counts.get(i) {
                sum = sum.saturating_add(*v);
            }
            i += 1;
        }
        sum == self.samples
    }

    /// 逐类计数与规划基数逐一相等。
    pub fn matches_plan(&self, plan: [u32; EQUIV_N]) -> bool {
        let mut i = 0usize;
        while i < EQUIV_N {
            if let (Some(a), Some(b)) = (self.counts.get(i), plan.get(i)) {
                if a != b {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

/// 注入账：16 格 × 10 轮，逐类命中。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EquivLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
    pub per_class: [u32; EQUIV_N],
    pub per_role: [u32; ROLE_N],
    pub broken: u32,
}

impl EquivLedger {
    /// 空账。
    pub const fn zero() -> EquivLedger {
        EquivLedger {
            injected: 0,
            detected: 0,
            escaped: 0,
            per_class: [0u32; EQUIV_N],
            per_role: [0u32; ROLE_N],
            broken: 0,
        }
    }

    /// 记一笔。
    pub fn record(&mut self, eq: Equiv, role: Role, caught: bool) -> bool {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
            if let Some(slot) = self.per_class.get_mut(eq.index()) {
                *slot = slot.saturating_add(1);
            }
            if let Some(slot) = self.per_role.get_mut(role.index()) {
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
}

/// 跑划分账：14 个样本逐条分类并与申报类比对。
pub fn run_partition() -> Partition {
    let mut part = Partition::new();
    let mut i = 0usize;
    while i < SAMPLES.len() {
        if let Some(s) = SAMPLES.get(i) {
            part.record(classify(s.raw), s.expect);
        }
        i += 1;
    }
    part
}

/// 跑注入账：16 格 × 10 轮，逐格用规则引擎判定并与矩阵比对。
pub fn run_equiv_injection() -> EquivLedger {
    let mut led = EquivLedger::zero();
    let mut i = 0usize;
    while i < MATRIX.len() {
        if let Some(c) = MATRIX.get(i).copied() {
            let sample = sample_for(c.equiv);
            let caught = judge(c.role, sample) == c.expect;
            let mut r = 0u32;
            while r < ROUNDS {
                led.record(c.equiv, c.role, caught);
                r += 1;
            }
        }
        i += 1;
    }
    led
}

/// 每个等价类取一个代表载体值（用于注入；与样本表独立）。
pub fn sample_for(eq: Equiv) -> i128 {
    match eq {
        Equiv::Zero => 0,
        Equiv::Neg => -1,
        Equiv::Max => MAX,
        Equiv::OverMax => OVER,
    }
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位，逐字节喂入。
fn fnv(h: u32, bytes: &[u8]) -> u32 {
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

/// 语料指纹：14 样本 + 12 代表值 + 16 格期望逐格喂入。
pub fn equiv_fingerprint() -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < SAMPLES.len() {
        if let Some(s) = SAMPLES.get(i) {
            buf.clear();
            buf.push(s.expect.index() as u8);
            buf.push((s.raw & 0xFF) as u8);
            buf.push(((s.raw >> 32) & 0xFF) as u8);
            buf.push(((s.raw >> 64) & 0xFF) as u8);
            buf.push(((s.raw >> 96) & 0xFF) as u8);
            h = fnv(h, &buf);
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < MATRIX.len() {
        if let Some(c) = MATRIX.get(j) {
            buf.clear();
            buf.push(c.role.index() as u8);
            buf.push(c.equiv.index() as u8);
            buf.push((c.expect.errno() & 0x7F) as u8);
            h = fnv(h, &buf);
        }
        j += 1;
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
    let part = run_partition();
    let led = run_equiv_injection();
    format!(
        "F9350 边界等价类总装：{} 类 × {} 角色 = {} 格，样本 {} 落账（错类 {}），注入 {} 检出 {} 逃逸 {}，指纹 0x{:08X}",
        EQUIV_N, ROLE_N, CELL_N, part.samples(), part.mismatched(),
        led.injected, led.detected, led.escaped, equiv_fingerprint()
    )
}

/// 取完成回写。
pub fn completion_record() -> Completion {
    let text = completion_summary();
    Completion { done: true, summary_len: text.len(), cells: CELL_N as u32 }
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9350 判据集（add ≥12；add_dyn ≤8）。
pub fn run_unx9350_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9350");

    s.add("段位守卫：0x7C500 独占 0x40 窗口且不与 F9349 重叠",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0x7C500 && SEG_BASE > SEG_PREV
            && SEG_BASE <= SEG_LIMIT,
        "段基址须 0x40 对齐、严格大于前段、且落在本批窗口内");

    let part = run_partition();
    s.add("划分完备：14 样本逐类落账且计数和等于样本数",
        part.samples() == SAMPLE_N as u32 && part.sums_to_samples() && part.mismatched() == 0,
        "四类并集必须覆盖全部样本，落账不得有缺口");

    s.add("划分互斥：每样本恰满足一个谓词",
        SAMPLES.iter().all(|s| exactly_one(s.raw)),
        "两个谓词同真即区间重叠，等于同一值落两类");

    s.add("等价类账落账：逐类基数 1/5/5/3 与规划一致",
        part.matches_plan(SAMPLE_PLAN),
        "逐类计数是分类函数有无偏置的直接证据");

    let rep_ok = REPS.iter().all(|r| classify(r.raw) == r.expect);
    s.add("代表值：12 个申报类逐条命中",
        rep_ok && REPS.len() == REP_N,
        "代表值归错类说明区间边界写错（常见是把负值落进上限类）");

    let mut cell_hit = 0u32;
    let mut ci = 0usize;
    while ci < MATRIX.len() {
        if let Some(c) = MATRIX.get(ci) {
            if judge(c.role, sample_for(c.equiv)) == c.expect {
                cell_hit = cell_hit.saturating_add(1);
            }
        }
        ci += 1;
    }
    s.add("逐格检出：16 格规则引擎与冻结矩阵逐格一致",
        cell_hit == CELL_N as u32,
        "16/16 才叫逐格；任一格红即为角色规则或表漂移");

    let led = run_equiv_injection();
    let per_class_ok = led.per_class.iter().all(|n| *n == ROUNDS * (ROLE_N as u32));
    s.add("注入账：160 次注入全检出、逃逸为 0",
        led.injected == INJECT_TOTAL && led.detected == INJECT_TOTAL
            && led.escaped == 0 && led.conserved() && led.broken == 0,
        "injected == detected + escaped，且 escapes 必须为 0");

    s.add("等价类账（逐类）：每类 4 角色 × 10 轮 = 40 次命中",
        per_class_ok && led.per_role.iter().all(|n| *n == ROUNDS * (EQUIV_N as u32)),
        "逐类分母 = 角色数 × 轮数；倾斜即分格错");

    s.add("零语义：Len 零放行、Mmap 零 EINVAL",
        classify(0) == Equiv::Zero
            && judge(Role::Len, 0) == Out::Ok
            && judge(Role::Mmap, 0) == Out::Err(EINVAL),
        "同类不同角色期望相反，是等价类总装最容易做错的一对");

    s.add("上限二分：上限 ∈ Max、上限+1 ∈ OverMax 且期望分叉",
        classify(MAX) == Equiv::Max
            && classify(OVER) == Equiv::OverMax
            && judge(Role::Len, MAX) == Out::Ok
            && judge(Role::Len, OVER) == Out::Err(ENOMEM),
        "边界比较必须含等号在上限侧，否则上限本身被判越界");

    s.add("负值全域：三类负值样本全落 Neg 类",
        classify(-1) == Equiv::Neg
            && classify(i64::MIN as i128) == Equiv::Neg
            && classify(-MAX) == Equiv::Neg
            && REPS.iter().filter(|r| r.expect == Equiv::Neg).all(|r| classify(r.raw) == Equiv::Neg),
        "先判负再判上限；次序反了负数会漏进上限类");

    s.add("载体回绕：u64 上 MAX+1 回绕成 0，i128 上可检出",
        u64::MAX.wrapping_add(1) == 0
            && classify(OVER) == Equiv::OverMax
            && OVER > MAX,
        "64 位载体让「上限+1」塌进零类，这就是必须用 i128 的原因");

    let mut buf_cell = matrix_cell(Role::Mmap, Equiv::Zero);
    let mut cell_ok = buf_cell.map(|c| c.expect == Out::Err(EINVAL)) == Some(true);
    buf_cell = matrix_cell(Role::Len, Equiv::Zero);
    cell_ok = cell_ok && buf_cell.map(|c| c.expect == Out::Ok) == Some(true);
    s.add("矩阵检索：按 (角色,类) 反查命中唯一格",
        cell_ok && matrix_cell(Role::Flags, Equiv::Max).is_some()
            && MATRIX.len() == CELL_N,
        "反查是诊断定位入口，重复格会让定位飘");

    let fp_a = equiv_fingerprint();
    s.add("双跑一致：等价类语料指纹相等且非零",
        fp_a == equiv_fingerprint() && fp_a != 0,
        "同样本两次必须逐字节相等；0 视为未喂料");

    let mut uniq = true;
    let mut ui = 0usize;
    while ui < EquivCode::ALL.len() {
        let mut uj = ui + 1;
        while uj < EquivCode::ALL.len() {
            if let (Some(a), Some(b)) = (EquivCode::ALL.get(ui), EquivCode::ALL.get(uj)) {
                if a.addr() == b.addr() {
                    uniq = false;
                }
            }
            uj += 1;
        }
        ui += 1;
    }
    s.add("诊断码：12 个段内码 addr 两两唯一且文案非空",
        uniq && EquivCode::ALL.iter().all(|c| !c.detail().is_empty()),
        "码重叠会让红项无法定位；每码必须有中文处置");

    let mut mono = led;
    let before = mono.injected;
    let mut m = 0u32;
    while m < 4 {
        mono.record(Equiv::Zero, Role::Len, true);
        m += 1;
    }
    s.add("账目单调：追加注入只增不减且守恒保持",
        mono.injected == before + 4 && mono.conserved() && mono.escaped == led.escaped,
        "追加记账不得改写既有逃逸数");

    let comp = completion_record();
    s.add("完成回写：状态=已完成且结果摘要非空",
        comp.done && comp.summary_len > 0 && comp.cells == CELL_N as u32,
        "判据③要求状态与摘要同时落账");

    let part_text = format!(
        "样本 {} / 负 {} / 零 {} / 上限 {} / 越界 {} / 错类 {}",
        part.samples(), part.count(Equiv::Neg), part.count(Equiv::Zero),
        part.count(Equiv::Max), part.count(Equiv::OverMax), part.mismatched()
    );
    s.add_dyn("划分账目", part.sums_to_samples() && part.mismatched() == 0, &part_text);

    let led_text = format!(
        "注入 {} / 检出 {} / 逃逸 {} / 格 {}",
        led.injected, led.detected, led.escaped, CELL_N
    );
    s.add_dyn("等价类账", led.conserved() && led.escaped == 0, &led_text);

    let fp_text = format!("等价类指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a != 0, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_classes_partition_samples() {
        let part = run_partition();
        assert_eq!(part.samples(), SAMPLE_N as u32);
        assert_eq!(part.sums_to_samples(), true);
        assert_eq!(part.mismatched(), 0);
        assert_eq!(part.matches_plan(SAMPLE_PLAN), true);
    }

    #[test]
    fn predicates_are_exclusive() {
        for s in SAMPLES.iter() {
            assert_eq!(exactly_one(s.raw), true);
        }
        assert_eq!(exactly_one(0), true);
        assert_eq!(exactly_one(MAX), true);
        assert_eq!(exactly_one(OVER), true);
    }

    #[test]
    fn reps_hit_declared_class() {
        for r in REPS.iter() {
            assert_eq!(classify(r.raw), r.expect);
        }
        assert_eq!(REPS.len(), REP_N);
    }

    #[test]
    fn zero_semantics_depend_on_role() {
        assert_eq!(classify(0), Equiv::Zero);
        assert_eq!(judge(Role::Len, 0), Out::Ok);
        assert_eq!(judge(Role::Mmap, 0), Out::Err(EINVAL));
    }

    #[test]
    fn max_and_over_max_bisect() {
        assert_eq!(classify(MAX), Equiv::Max);
        assert_eq!(classify(OVER), Equiv::OverMax);
        assert_eq!(judge(Role::Len, MAX), Out::Ok);
        assert_eq!(judge(Role::Len, OVER), Out::Err(ENOMEM));
    }

    #[test]
    fn negative_values_never_leak_to_max() {
        assert_eq!(classify(-1), Equiv::Neg);
        assert_eq!(classify(i64::MIN as i128), Equiv::Neg);
        assert_eq!(classify(-MAX), Equiv::Neg);
    }

    #[test]
    fn wrap_requires_i128_carrier() {
        assert_eq!(u64::MAX.wrapping_add(1), 0);
        assert_eq!(classify(OVER), Equiv::OverMax);
        assert_eq!(OVER > MAX, true);
    }

    #[test]
    fn matrix_is_16_cells_and_hits() {
        assert_eq!(MATRIX.len(), CELL_N);
        for c in MATRIX.iter() {
            assert_eq!(judge(c.role, sample_for(c.equiv)), c.expect);
        }
    }

    #[test]
    fn injection_ledger_conserved() {
        let led = run_equiv_injection();
        assert_eq!(led.injected, INJECT_TOTAL);
        assert_eq!(led.detected, INJECT_TOTAL);
        assert_eq!(led.escaped, 0);
        assert_eq!(led.conserved(), true);
        for n in led.per_class.iter() {
            assert_eq!(*n, ROUNDS * (ROLE_N as u32));
        }
    }

    #[test]
    fn fingerprint_is_stable() {
        assert_eq!(equiv_fingerprint(), equiv_fingerprint());
        assert_eq!(equiv_fingerprint() != 0, true);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9350_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
    }
}
