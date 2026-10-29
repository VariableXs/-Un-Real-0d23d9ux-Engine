//! unx4138_bigfile — UNX-F4138 大文件标志与 EOVERFLOW 预锚（诊断段 0x9320）。
//!
//! 判据原文：UNX-F4138-J1 超过 32 位偏移边界的 stat/open 注入按 O_LARGEFILE 语义
//!           返回 EOVERFLOW 或按 64 位语义通过——双模式行为账齐备。
//!   J2 32 位边界值恰好合法，上界加一才越界。
//!   J3 64 位模式零拒收零截断。
//!   J4 双模式对照表逐行与授权表一致。
//! 设计纪律：
//!   1. 边界值只认 0xFFFF_FFFF（即 2^32-1）：偏移「恰好等于上界」在 32 位模式下合法，
//!      「上界 +1」才越界。边界写成严格比较而非近似，避免 off-by-one 型静默截断。
//!   2. 双模式互斥：Small（无 O_LARGEFILE）与 Large（有 O_LARGEFILE）两模式下同一
//!      输入必须给出不同结论，且结论可复现；两模式的通过/拒收各记一账，互不抵扣。
//!   3. EOVERFLOW(75) 只在「值装不下」时返回，不得用于「文件太大不支持」——后者是
//!      EFBIG。本域只裁值域，故只有 EOVERFLOW。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9320 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9320;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_OVERFLOW: u16 = SEG_BASE + 1; // 对应 EOVERFLOW(75)：值装不进 32 位
const E_INVAL: u16 = SEG_BASE + 2; // 标志非法（如 O_LARGEFILE 与 O_PATH 冲突）
const E_MODE: u16 = SEG_BASE + 3; // 模式字不在闭集
const E_ACCT: u16 = SEG_BASE + 4; // 双模式账不平
const E_TRUNC: u16 = SEG_BASE + 5; // 发生静默截断（判据失守）
const E_SIZE: u16 = SEG_BASE + 6; // 文件尺寸为 0 却要走大文件通路
const E_NEG: u16 = SEG_BASE + 7; // 偏移负值语义（本域不收）
const E_DUP: u16 = SEG_BASE + 8; // 同一探测重复记账

/// EOVERFLOW 的 errno 原值（POSIX errno.h）。
pub const EOVERFLOW_ERRNO: u16 = 75;
/// 32 位偏移上界（2^32 - 1）。
pub const OFF32_MAX: u64 = 0xFFFF_FFFF;
/// O_LARGEFILE 的 Linux 原值（asm-generic/fcntl.h, 0o100000）。
pub const O_LARGEFILE: u32 = 0o100000;
/// O_PATH（与 O_LARGEFILE 联用时大文件语义由 fcntl 侧另行裁定，本域拒收）。
pub const O_PATH: u32 = 0o10000000;

/// 尺寸分类状态机：Empty → Small → Large（不可回退到 Empty）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SizeClass {
    Empty,
    Small,
    Large,
}

impl SizeClass {
    /// 分类：>0 且 <= OFF32_MAX 为 Small，> OFF32_MAX 为 Large，0 为 Empty。
    pub fn classify(size: u64) -> SizeClass {
        if size == 0 {
            SizeClass::Empty
        } else if size <= OFF32_MAX {
            SizeClass::Small
        } else {
            SizeClass::Large
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            SizeClass::Empty => "Empty",
            SizeClass::Small => "Small",
            SizeClass::Large => "Large",
        }
    }
}

/// 偏移模式：Small（32 位 ABI）与 Large（64 位 ABI）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OffsetMode {
    Small,
    Large,
}

impl OffsetMode {
    /// 由 open 标志推模式：O_LARGEFILE 置位即 Large。
    pub fn from_flags(flags: u32) -> Result<OffsetMode, u16> {
        if (flags & O_LARGEFILE) != 0 && (flags & O_PATH) != 0 {
            return Err(E_INVAL); // 两者联用的语义不在本域裁断范围
        }
        if (flags & O_LARGEFILE) != 0 {
            Ok(OffsetMode::Large)
        } else {
            Ok(OffsetMode::Small)
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            OffsetMode::Small => "32bit",
            OffsetMode::Large => "64bit",
        }
    }
}

/// stat 结果的可裁断视图：只保留裁断需要的三个量。
pub struct StatView {
    /// 文件尺寸（字节）
    pub size: u64,
    /// 尺寸分类
    pub class: SizeClass,
    /// 是否为稀疏文件（不计入块数，仅用于对照）
    pub sparse: bool,
}

impl StatView {
    pub fn new(size: u64, sparse: bool) -> StatView {
        StatView {
            size,
            class: SizeClass::classify(size),
            sparse,
        }
    }

    /// 32 位视图下的可表达性：size <= OFF32_MAX。
    pub fn fits32(&self) -> bool {
        self.size <= OFF32_MAX
    }
}

/// 裁断结果：模式 + 结论 + 实际落地的偏移位宽。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct VerdictOut {
    /// 采用模式
    pub mode: OffsetMode,
    /// 是否通过
    pub passed: bool,
    /// 32 位视图落地的截断值（仅在 Small 模式有意义；通过时等于原值低 32 位）
    pub low32: u32,
    /// 64 位视图落地的完整值
    pub full64: u64,
}

/// 大文件裁断门：双模式账 + 截断自检账。
pub struct BigFileGate {
    /// Small 模式通过次数
    pub small_pass: u32,
    /// Small 模式 EOVERFLOW 次数
    pub small_overflow: u32,
    /// Large 模式通过次数
    pub large_pass: u32,
    /// Large 模式拒收次数（本应恒 0）
    pub large_reject: u32,
    /// 静默截断侦测次数（本应恒 0）
    pub truncations: u32,
    /// 探测总次数
    pub probes: u32,
}

impl BigFileGate {
    pub fn new() -> BigFileGate {
        BigFileGate {
            small_pass: 0,
            small_overflow: 0,
            large_pass: 0,
            large_reject: 0,
            truncations: 0,
            probes: 0,
        }
    }

    /// stat 裁断：Small 模式超界 → EOVERFLOW；Large 模式一律通过且值不变。
    pub fn stat(&mut self, st: &StatView, mode: OffsetMode) -> Result<VerdictOut, u16> {
        self.probes += 1;
        match mode {
            OffsetMode::Small => {
                if !st.fits32() {
                    self.small_overflow += 1;
                    return Err(E_OVERFLOW);
                }
                self.small_pass += 1;
                Ok(VerdictOut {
                    mode,
                    passed: true,
                    low32: (st.size & 0xFFFF_FFFF) as u32,
                    full64: st.size,
                })
            }
            OffsetMode::Large => {
                self.large_pass += 1;
                Ok(VerdictOut {
                    mode,
                    passed: true,
                    low32: (st.size & 0xFFFF_FFFF) as u32,
                    full64: st.size,
                })
            }
        }
    }

    /// open 裁断：在 stat 基础上再校验偏移参数（off 必须是值域内非负量）。
    pub fn open(&mut self, st: &StatView, mode: OffsetMode, off: u64) -> Result<u64, u16> {
        if off > st.size && st.size != 0 {
            // 越过 EOF 的 open 偏移在 O_APPEND 之外无意义，按越界拒
            return Err(E_SIZE);
        }
        let v = self.stat(st, mode)?;
        // 截断自检：Small 模式下若原值与 low32 不等，说明发生了静默截断
        if v.mode == OffsetMode::Small && v.full64 > OFF32_MAX {
            self.truncations += 1;
            return Err(E_TRUNC);
        }
        Ok(v.full64)
    }

    /// 双模式账守恒：probes == small_pass + small_overflow + large_pass + large_reject。
    pub fn conserved(&self) -> bool {
        self.probes == self.small_pass + self.small_overflow + self.large_pass + self.large_reject
    }

    /// 不变量：Large 模式零拒收、零截断。
    pub fn invariants(&self) -> bool {
        self.large_reject == 0 && self.truncations == 0
    }

    /// 双模式对照表导出：(尺寸, Small 结论码, Large 结论码)。
    /// 结论码 0 表通过，非 0 为诊断码。
    pub fn contrast_table(&mut self, sizes: &[u64]) -> Vec<(u64, u16, u16)> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < sizes.len() {
            let st = StatView::new(sizes[i], false);
            let s = match self.stat(&st, OffsetMode::Small) {
                Ok(_) => 0u16,
                Err(e) => e,
            };
            let l = match self.stat(&st, OffsetMode::Large) {
                Ok(_) => 0u16,
                Err(e) => e,
            };
            out.push((sizes[i], s, l));
            i += 1;
        }
        out
    }
}

/// 诊断码解释。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_OVERFLOW => "值装不进 32 位（EOVERFLOW 75）",
        E_INVAL => "标志非法（O_LARGEFILE 与 O_PATH 冲突）",
        E_MODE => "模式字不在闭集",
        E_ACCT => "双模式账不平",
        E_TRUNC => "发生静默截断（判据失守）",
        E_SIZE => "文件尺寸为 0 或 open 偏移越界",
        E_NEG => "偏移负值语义（本域不收）",
        E_DUP => "同一探测重复记账",
        _ => "未知诊断码",
    }
}

/// 确定性演练：边界 → 32 位溢出 10/10 → 64 位通过 → 对照表 → 对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：32 位边界——size == OFF32_MAX 必须通过（恰好装得下）。
    let mut g = BigFileGate::new();
    let edge = StatView::new(OFF32_MAX, false);
    if edge.class != SizeClass::Small || !edge.fits32() {
        return Err(E_OVERFLOW);
    }
    let v = g.stat(&edge, OffsetMode::Small)?;
    if v.low32 != 0xFFFF_FFFF || v.full64 != OFF32_MAX {
        return Err(E_ACCT);
    }
    hits += 1;

    // R2：32 位超界（OFF32_MAX+1）注入 10/10 次全部 EOVERFLOW。
    let over = StatView::new(OFF32_MAX + 1, false);
    if over.class != SizeClass::Large {
        return Err(E_OVERFLOW);
    }
    let mut ovf = 0u32;
    let mut i = 0;
    while i < 10 {
        if g.stat(&over, OffsetMode::Small) != Err(E_OVERFLOW) {
            return Err(E_OVERFLOW);
        }
        ovf += 1;
        i += 1;
    }
    if ovf != 10 || g.small_overflow != 10 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R3：64 位模式同一输入通过，且值不截断（full64 精确保留）。
    let big = StatView::new(0x1_0000_0000 + 12345, false);
    let v = g.stat(&big, OffsetMode::Large)?;
    if v.full64 != 0x1_0000_0000 + 12345 {
        return Err(E_ACCT);
    }
    if v.low32 != 12345 {
        return Err(E_ACCT); // 低 32 位 = 12345，可验
    }
    if !g.invariants() {
        return Err(E_TRUNC); // Large 侧零拒收零截断
    }
    hits += 1;

    // R4：O_LARGEFILE 标志解码——置位为 Large，未置为 Small，与 O_PATH 联用拒收。
    if OffsetMode::from_flags(O_LARGEFILE)? != OffsetMode::Large {
        return Err(E_MODE);
    }
    if OffsetMode::from_flags(0)? != OffsetMode::Small {
        return Err(E_MODE);
    }
    if OffsetMode::from_flags(O_LARGEFILE | O_PATH) != Err(E_INVAL) {
        return Err(E_INVAL);
    }
    hits += 1;

    // R5：open 通路——Large 模式下 5 GiB 文件可通过，Small 模式拒收。
    let huge = StatView::new(5 * 0x1_0000_0000, false);
    let mut g2 = BigFileGate::new();
    if g2.open(&huge, OffsetMode::Large, 0)? != 5 * 0x1_0000_0000 {
        return Err(E_ACCT);
    }
    if g2.open(&huge, OffsetMode::Small, 0) != Err(E_OVERFLOW) {
        return Err(E_OVERFLOW);
    }
    hits += 1;

    // R6：双模式对照表——四个尺寸两两成行，越界行 Small 必 Overflow、Large 必通过。
    let sizes = [0u64, 1, OFF32_MAX, OFF32_MAX + 1, 0xFFFF_FFFF_FFFF];
    let rows = g.contrast_table(&sizes);
    if rows.len() != 5 {
        return Err(E_ACCT);
    }
    let mut k = 0usize;
    while k < rows.len() {
        let (sz, s, l) = rows[k];
        if l != 0 {
            return Err(E_ACCT); // Large 列恒通过
        }
        if sz > OFF32_MAX && s != E_OVERFLOW {
            return Err(E_OVERFLOW);
        }
        if sz <= OFF32_MAX && s != 0 {
            return Err(E_ACCT); // 值域内 Small 也必须通过
        }
        k += 1;
    }
    hits += 1;

    // I1：Small 模式绝不产生截断值（full64 > 上界时在 stat 层就被拦）。
    if g.truncations != 0 {
        return Err(E_TRUNC);
    }
    if g.large_reject != 0 {
        return Err(E_MODE);
    }
    hits += 1;

    // I2：open 偏移越界（超过 EOF 且文件非空）→ E_SIZE。
    let small = StatView::new(1024, false);
    if g2.open(&small, OffsetMode::Large, 2048) != Err(E_SIZE) {
        return Err(E_SIZE);
    }
    // 偏移恰好等于 EOF：合法
    if g2.open(&small, OffsetMode::Large, 1024)? != 1024 {
        return Err(E_ACCT);
    }
    hits += 1;

    // I3：尺寸分类闭集——0/Small/Large 三态互斥。
    if SizeClass::classify(0) != SizeClass::Empty {
        return Err(E_SIZE);
    }
    if SizeClass::classify(1) != SizeClass::Small {
        return Err(E_SIZE);
    }
    if SizeClass::classify(OFF32_MAX + 2) != SizeClass::Large {
        return Err(E_SIZE);
    }
    hits += 1;

    // C1：账目终审——g 的 probes 与四类落点相等；Large 侧全通。
    if !g.conserved() {
        return Err(E_ACCT);
    }
    // g：R1 1 + R2 10 + R3 1 + R6 10 = 22
    if g.probes != 22 {
        return Err(E_ACCT);
    }
    // Small 通过 4（边界 1 + 对照表 3），Small 溢出 12（10 + 对照表 2），Large 通过 6
    if g.small_pass != 4 || g.small_overflow != 12 || g.large_pass != 6 {
        return Err(E_ACCT);
    }
    if !g2.conserved() || !g2.invariants() {
        return Err(E_ACCT);
    }
    hits += 1;

    Ok(hits)
}
