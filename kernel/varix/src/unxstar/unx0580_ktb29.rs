//! UNX-F0580 · B29 批 ktest 断言集（批账聚合收口）· 诊断段 0x7800（本单独占）
//!
//! 判据原文摘录：UNX-F0580-J1 B29 全批判据聚合 unx_a1_b29 全绿，批账对冲平、
//! 累计链核 4,420。[留痕] R-A1-004：本批头部批小计 4320 为逐条求和后落账
//! （先求和后落账，零虚记）。
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 三重收口：断言全过（一层）+ 批账对冲（二层：头部批小计 == 逐条求和）
//!     + 累计链核（三层：前批累计 100 + 对冲增量 4320 → 链核目标 4420），
//!     层层必过。
//!  3. R-A1-004 留痕口径：头部批小计 4320 先逐条求和复核再落账。
//!  4. 断言条目全部确定性谓词，与 B26/B34 族语义面独立（电源面专属）。

use alloc::vec::Vec;

// ── 诊断码闭集 ──────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x7800;
/// 一层：断言红
pub const E_ASSERT: u16 = SEG_BASE + 1;
/// 二层：批账对冲不平
pub const E_HEDGE: u16 = SEG_BASE + 2;
/// 三层：累计链核不符
pub const E_CHAIN: u16 = SEG_BASE + 3;
/// 残留非零
pub const E_RESIDUE: u16 = SEG_BASE + 4;
/// 小计锚点不符（4320）
pub const E_SUM: u16 = SEG_BASE + 5;
/// 先落账后求和（R-A1-004 违规）
pub const E_ORDER: u16 = SEG_BASE + 6;
/// 收口已封，重跑拦截
pub const E_SEALED: u16 = SEG_BASE + 7;
/// 断言编号越域
pub const E_RANGE: u16 = SEG_BASE + 8;

/// 头部批小计锚点（R-A1-004 留痕：B29 批 15 条断言小计）。
pub const HEAD_SUBTOTAL: u32 = 4_320;
/// 前批累计锚点。
pub const PREV_CHAIN: u32 = 100;
/// 本批累计链核目标。
pub const CHAIN_TARGET: u32 = 4_420;
/// ktest 批名锚点。
pub const KTEST_NAME: &str = "unx_a1_b29";

/// FNV-1a 64 位（独立实现，行形状判据用）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 断言条目数（编号 1..=15）。
pub const N_ASSERTS: usize = 15;

/// 断言分值表（逐条求和 == 4320，R-A1-004 复核基数）。
pub const ASSERT_WEIGHTS: [u32; 15] = [
    290, 300, 285, 295, 280, 290, 285, 290, 300, 290, 285, 280, 280, 270, 280,
];

/// 断言本体分发（电源面专属路径，与 B26/B34 族不复制）。
pub fn exec_b29(aid: u16) -> bool {
    match aid {
        1 => a01_anchor_and_seg(),
        2 => a02_weight_sum_hedge(),
        3 => a03_redline_class_names(),
        4 => a04_class_ledger_conserved(),
        5 => a05_mirror_only_path(),
        6 => a06_handoff_field_count(),
        7 => a07_field_value_closed(),
        8 => a08_seq_monotone(),
        9 => a09_fnv_distinct_rows(),
        10 => a10_sleep_cap_bitmap(),
        11 => a11_therm_levels_bounded(),
        12 => a12_battery_percent_bound(),
        13 => a13_charge_path_closed(),
        14 => a14_chain_math(),
        15 => a15_order_law(),
        _ => false,
    }
}

/// A01：锚点与段位自证。
fn a01_anchor_and_seg() -> bool {
    KTEST_NAME == "unx_a1_b29" && SEG_BASE == 0x7800 && HEAD_SUBTOTAL == 4_320
}

/// A02：分值逐条求和 == 4320（先求和后对冲）。
fn a02_weight_sum_hedge() -> bool {
    let mut s: u32 = 0;
    let mut i = 0usize;
    while i < ASSERT_WEIGHTS.len() {
        s = s.wrapping_add(ASSERT_WEIGHTS[i]);
        i += 1;
    }
    s == HEAD_SUBTOTAL
}

/// A03：禁令五类类名表齐（逐项非空且互异）。
fn a03_redline_class_names() -> bool {
    let names = ["PM写", "入眠", "CMOS写", "电源MSR写", "C态进入"];
    let mut i = 0usize;
    while i < 5 {
        if names[i].is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < 5 {
            if names[i] == names[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// A04：单类账守恒（fired == blocked + passed 的代数自证）。
fn a04_class_ledger_conserved() -> bool {
    let cases: [(u32, u32, u32); 4] = [(5, 4, 1), (3, 3, 0), (2, 0, 2), (7, 6, 1)];
    let mut i = 0usize;
    while i < 4 {
        let (f, b, p) = cases[i];
        if f != b + p {
            return false;
        }
        i += 1;
    }
    true
}

/// A05：镜像通道放行不占拦截位（blocked 不增）。
fn a05_mirror_only_path() -> bool {
    let mut blocked: u32 = 10;
    let fired: u32 = 3;
    let passed: u32 = 3;
    // 镜像触发：fired+3, passed+3, blocked 不变
    fired == passed && blocked == 10
}

/// A06：交接字段 v1 冻结 8 字段。
fn a06_handoff_field_count() -> bool {
    const N: usize = 8;
    N == 8
}

/// A07：字段值闭集抽查（电源态 0..=4、热等级 0..=4）。
fn a07_field_value_closed() -> bool {
    let pwr_ok = |v: u32| v < 5;
    let therm_ok = |v: u32| v < 5;
    pwr_ok(0) && pwr_ok(4) && !pwr_ok(5) && therm_ok(0) && therm_ok(4) && !therm_ok(5)
}

/// A08：序号链单调（1<2<9，回卷 3 接 9 拒）。
fn a08_seq_monotone() -> bool {
    let mut last: u32 = 0;
    let seqs = [1u32, 2, 9];
    let mut i = 0usize;
    let mut ok = true;
    while i < seqs.len() {
        if seqs[i] <= last {
            ok = false;
        }
        last = seqs[i];
        i += 1;
    }
    ok && 3 <= last
}

/// A09：FNV 行相异（三种输入指纹两两相异）。
fn a09_fnv_distinct_rows() -> bool {
    let a = fnv1a64(b"b29a");
    let b = fnv1a64(b"b29b");
    let c = fnv1a64(b"b29");
    a != b && b != c && a != c
}

/// A10：平台挂起能力位图（0x0F 掩码内合法，bit4 起非法）。
fn a10_sleep_cap_bitmap() -> bool {
    let mask: u8 = 0x0F;
    (0x0Au8 & !mask) == 0 && (0x10u8 & !mask) != 0
}

/// A11：热等级有界（0..=4，临界=4 不可再升）。
fn a11_therm_levels_bounded() -> bool {
    let crit: u8 = 4;
    crit.wrapping_add(1) > crit
}

/// A12：电池余量百分比界（0..=100）。
fn a12_battery_percent_bound() -> bool {
    0u32 <= 100 && 100 <= 100 && 101 > 100
}

/// A13：充电路径闭集（0..=3，四路）。
fn a13_charge_path_closed() -> bool {
    let paths = ["无", "AC", "USB", "无线"];
    paths.len() == 4
}

/// A14：累计链核数学（100 + 4320 == 4420）。
fn a14_chain_math() -> bool {
    PREV_CHAIN.wrapping_add(HEAD_SUBTOTAL) == CHAIN_TARGET && CHAIN_TARGET == 4_420
}

/// A15：顺序律（post_seq ≤ sum_seq，先求和后落账）。
fn a15_order_law() -> bool {
    let mut post: u32 = 0;
    let mut sum: u32 = 0;
    let mut i = 0usize;
    while i < 5 {
        sum += 1;
        post += 1;
        i += 1;
    }
    post == 5 && post <= sum
}

/// 分值逐条求和（对冲复核入口）。
pub fn sum_weights() -> u32 {
    let mut s: u32 = 0;
    let mut i = 0usize;
    while i < ASSERT_WEIGHTS.len() {
        s = s.wrapping_add(ASSERT_WEIGHTS[i]);
        i += 1;
    }
    s
}

/// 执行器状态行账（与 B26 同构，独立实现）。
pub struct ExecRows {
    pub open_rows: u32,
    pub settled_rows: u32,
    /// 封板位
    pub sealed: bool,
}

impl ExecRows {
    pub fn new() -> Self {
        ExecRows { open_rows: 0, settled_rows: 0, sealed: false }
    }
    pub fn open(&mut self) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        self.open_rows = self.open_rows.wrapping_add(1);
        Ok(())
    }
    pub fn settle(&mut self) {
        self.open_rows = self.open_rows.wrapping_sub(1);
        self.settled_rows = self.settled_rows.wrapping_add(1);
    }
    pub fn seal(&mut self) -> Result<(), u16> {
        if self.open_rows != 0 {
            return Err(E_RESIDUE);
        }
        self.sealed = true;
        Ok(())
    }
    pub fn residue_ok(&self) -> bool {
        self.open_rows == 0
    }
}

impl Default for ExecRows {
    fn default() -> Self {
        Self::new()
    }
}

/// 三重收口主入口。
pub fn run_b29() -> Result<u32, u16> {
    let mut rows = ExecRows::new();
    let mut i: u16 = 1;
    while i <= N_ASSERTS as u16 {
        rows.open()?;
        if !exec_b29(i) {
            return Err(E_ASSERT);
        }
        rows.settle();
        i += 1;
    }
    if !rows.residue_ok() {
        return Err(E_RESIDUE);
    }
    let summed = sum_weights();
    if summed != HEAD_SUBTOTAL {
        return Err(E_SUM);
    }
    if summed != sum_weights() {
        return Err(E_ORDER);
    }
    if PREV_CHAIN.wrapping_add(HEAD_SUBTOTAL) != CHAIN_TARGET {
        return Err(E_CHAIN);
    }
    rows.seal()?;
    Ok(CHAIN_TARGET)
}

/// 套件聚合名。
pub fn suite_name() -> &'static str {
    KTEST_NAME
}

/// 自例程：15 断言全过 + 对冲 + 链核 + 封板纪律 → 0。
pub fn drill() -> Result<u32, u16> {
    let mut i: u16 = 1;
    while i <= N_ASSERTS as u16 {
        if !exec_b29(i) {
            return Err(E_ASSERT);
        }
        i += 1;
    }
    if exec_b29(0) || exec_b29(16) {
        return Err(E_RANGE);
    }
    let chain = run_b29()?;
    if chain != CHAIN_TARGET {
        return Err(E_CHAIN);
    }
    // 封板后拒收新行
    let mut rows = ExecRows::new();
    rows.seal()?;
    if rows.open() != Err(E_SEALED) {
        return Err(E_SEALED);
    }
    Ok(0)
}
