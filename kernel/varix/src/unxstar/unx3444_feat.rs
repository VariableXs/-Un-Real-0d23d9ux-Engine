//! UNX-F3444 随机化执行与种子账本（种子注入 + 顺序记录复现性账本） · 0x79C40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B14 逐字摘录）：
//!   ① 纯功能代码 ≥280 行；
//!   ② J1：种子注入接口（`--seed=N`）+ 执行顺序账（种子→序的确定性
//!     映射）；M-203 同种子双跑顺序逐位一致。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 种子注入接口：`--seed=N` 逐字节解析（格式坏/零种子/超上限三类
//!     注入全部显性拒），N 为十进制无符号、上限 9 位（≤ 999_999_999），
//!     零种子保留为「未注入」哨兵值不予执行；
//!   - 确定性映射：LCG（a=6364136223846793005, c=1442695040888963407，
//!     模 2^64 环绕乘加，纯整数）由 (种子, 用例号) 派生排序键，同种子
//!     同输入恒出同键——种子→序映射纯函数化；
//!   - 执行顺序账：SeedLedger 记录 种子→序 条目，查账按种子精确匹配，
//!     未知种子查无显性回报（LedgerMiss），不许静默回退；
//!   - M-203：同种子双跑执行顺序逐位一致（置换性 + 双跑一致性双闸），
//!     序必须是 1..=n 的完整置换（缺号/重号都点名）；
//!   - LCG 单射性：a 为奇数 → 乘法模 2^64 是双射 → 不同输入恒不同
//!     输出（可证性质，逐点验证）；
//!   - 注入必抓：坏旗标、零种子、超上限种子、用例数越界（0 与 65）
//!     四类人为违约全部被闭集诊断码点名。
//! 常量来源：
//!   - LCG 常量：Knuth MMIX 乘加线性同余推荐值（整数伪随机规范引用）；
//!   - 种子上限 9 位十进制：解析面冻结（避免 u64 解析溢出歧义）；
//!   - 诊断码闭集：SEG_BASE+0x01..0x0C（派工卡约定）；
//!   - 段窗：B14 批段窗 [0x79C00, 0x79DC0]。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B14 批段窗 [0x79C00, 0x79DC0] 内第 2 单）。
pub const SEG_BASE: u32 = 0x79C40;

/// 种子旗标前缀（判据原文冻结：`--seed=N`）。
pub const SEED_FLAG_PREFIX: &str = "--seed=";

/// 种子上限（9 位十进制：999_999_999）。
pub const SEED_MAX: u64 = 999_999_999;

/// 最小用例数（顺序账至少覆盖 1 个用例）。
pub const MIN_CASES: usize = 1;

/// 最大用例数（定容置换核验表容量上限）。
pub const MAX_CASES: usize = 64;

/// LCG 乘数（Knuth MMIX 推荐值）。
pub const LCG_A: u64 = 6364136223846793005;

/// LCG 增量（Knuth MMIX 推荐值）。
pub const LCG_C: u64 = 1442695040888963407;

/// 排序键混淆乘数（黄金比例常数，整数折叠用）。
pub const KEY_MIX: u64 = 0x9E37_79B9_7F4A_7C15;

/// LCG 单步：模 2^64 环绕乘加（纯整数，无浮点无随机熵源）。
pub fn lcg_next(state: u64) -> u64 {
    LCG_A.wrapping_mul(state).wrapping_add(LCG_C)
}

/// 排序键派生：(种子, 用例号) → 64 位键（三轮 LCG 折叠，纯函数）。
pub fn order_key(seed: u64, case_id: u32) -> u64 {
    let mut k = seed ^ ((case_id as u64).wrapping_mul(KEY_MIX));
    let mut round = 0u32;
    while round < 3 {
        k = lcg_next(k);
        round += 1;
    }
    k
}

/// 种子旗标解析：`--seed=N` → N。
/// 格式坏（SeedFlagBad）、零种子（SeedZero）、超上限（SeedTooLarge）
/// 三类违约显性拒；前导零按数值解析（`--seed=0000042` → 42）。
pub fn parse_seed_flag(arg: &str) -> Result<u64, u32> {
    let rest = match arg.strip_prefix(SEED_FLAG_PREFIX) {
        Some(r) => r,
        None => return Err(Diag::SeedFlagBad.val()),
    };
    if rest.is_empty() {
        return Err(Diag::SeedFlagBad.val());
    }
    if rest.len() > 10 {
        return Err(Diag::SeedTooLarge.val());
    }
    let bytes = rest.as_bytes();
    let mut acc: u64 = 0;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(b) => *b,
            None => return Err(Diag::SeedFlagBad.val()),
        };
        if b < b'0' || b > b'9' {
            return Err(Diag::SeedFlagBad.val());
        }
        acc = match acc.checked_mul(10).and_then(|x| x.checked_add((b - b'0') as u64)) {
            Some(v) => v,
            None => return Err(Diag::SeedTooLarge.val()),
        };
        i += 1;
    }
    if acc > SEED_MAX {
        return Err(Diag::SeedTooLarge.val());
    }
    if acc == 0 {
        return Err(Diag::SeedZero.val());
    }
    Ok(acc)
}

/// 执行顺序账条目：种子 → 该种子下的执行序（1..=n 的置换）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerEntry {
    /// 种子（非零）。
    pub seed: u64,
    /// 执行序：用例号 1..=n 的确定性排列。
    pub order: Vec<u32>,
    /// 用例总数 n。
    pub case_count: usize,
}

/// 种子→序确定性映射：给定种子与用例数，产出 1..=n 的确定性排列。
/// 排序键 = order_key(seed, id)，键相同按用例号升序（稳定全序）。
pub fn order_for_seed(seed: u64, case_count: usize) -> Result<Vec<u32>, u32> {
    if seed == 0 {
        return Err(Diag::SeedZero.val());
    }
    if case_count < MIN_CASES || case_count > MAX_CASES {
        return Err(Diag::CaseCountBad.val());
    }
    // 定容键表：(键, 用例号)。
    let mut keys: Vec<(u64, u32)> = Vec::new();
    let mut id = 1u32;
    while id <= case_count as u32 {
        keys.push((order_key(seed, id), id));
        id += 1;
    }
    // 插入排序（全序：键升序，键同则号升序——确定性无双值）。
    let mut i = 1usize;
    while i < keys.len() {
        let mut j = i;
        while j > 0 {
            let prev = match keys.get(j - 1) {
                Some(p) => *p,
                None => return Err(Diag::OrderLenBad.val()),
            };
            let cur = match keys.get(j) {
                Some(c) => *c,
                None => return Err(Diag::OrderLenBad.val()),
            };
            if cur < prev {
                keys[j - 1] = cur;
                keys[j] = prev;
                j -= 1;
            } else {
                break;
            }
        }
        i += 1;
    }
    let mut order: Vec<u32> = Vec::new();
    let mut k = 0usize;
    while k < keys.len() {
        match keys.get(k) {
            Some((_, cid)) => order.push(*cid),
            None => return Err(Diag::OrderLenBad.val()),
        }
        k += 1;
    }
    if order.len() != case_count {
        return Err(Diag::OrderLenBad.val());
    }
    Ok(order)
}

/// 置换核验：序恰为 1..=n 的完整置换（缺号/重号/越界都算破）。
pub fn is_permutation(order: &[u32], case_count: usize) -> bool {
    if order.len() != case_count || case_count == 0 || case_count > MAX_CASES {
        return false;
    }
    let mut seen = [false; MAX_CASES];
    let mut i = 0usize;
    while i < order.len() {
        let id = match order.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if id == 0 || id as usize > case_count {
            return false;
        }
        let slot = id as usize - 1;
        match seen.get(slot) {
            Some(true) => return false,
            Some(false) => seen[slot] = true,
            None => return false,
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < case_count {
        match seen.get(k) {
            Some(true) => {}
            _ => return false,
        }
        k += 1;
    }
    true
}

/// 执行顺序账（复现性账本）：记录 种子→序，查账按种子精确匹配。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeedLedger {
    /// 条目表（按记录序追加）。
    pub entries: Vec<LedgerEntry>,
}

impl SeedLedger {
    /// 空账本。
    pub fn new() -> SeedLedger {
        SeedLedger { entries: Vec::new() }
    }

    /// 记一笔：算序、核置换、入账。
    pub fn record(&mut self, seed: u64, case_count: usize) -> Result<(), u32> {
        let order = order_for_seed(seed, case_count)?;
        if !is_permutation(&order, case_count) {
            return Err(Diag::OrderNotPerm.val());
        }
        self.entries.push(LedgerEntry { seed, order, case_count });
        Ok(())
    }

    /// 查账：按种子精确匹配（未知种子返回 None——查无显性，不静默回退）。
    pub fn lookup(&self, seed: u64) -> Option<&LedgerEntry> {
        let mut i = 0usize;
        while i < self.entries.len() {
            match self.entries.get(i) {
                Some(e) => {
                    if e.seed == seed {
                        return Some(e);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }
}

/// 诊断码闭集：0x79C41..=0x79C4C（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 旗标格式坏：`--seed=N` 前缀或数字位违约。
    SeedFlagBad = 0x01,
    /// 零种子：0 保留为未注入哨兵，不予执行。
    SeedZero = 0x02,
    /// 种子超上限：超 9 位十进制上限。
    SeedTooLarge = 0x03,
    /// 序长失配：执行序长度与用例数不一致。
    OrderLenBad = 0x04,
    /// 序非置换：执行序缺号/重号/越界。
    OrderNotPerm = 0x05,
    /// 账本查无：未知种子查账无果（显性回报不回退）。
    LedgerMiss = 0x06,
    /// 账本分叉：双跑两次建账内容不一致。
    LedgerDiverge = 0x07,
    /// 用例数越界：n 不落在 [1, 64]。
    CaseCountBad = 0x08,
    /// LCG 非单射：不同输入派生同键（可证性质被破坏）。
    LcgNotInjective = 0x09,
    /// M-203 分叉：同种子双跑执行顺序不一致。
    M203Diverge = 0x0A,
    /// 段位守卫失守：SEG_BASE 不满足 0x40 对齐或越出批段窗。
    SegGuardFail = 0x0B,
    /// 写回失守：完成时摘要空或账本为空。
    WritebackFail = 0x0C,
}

impl Diag {
    /// 完整 u32 编码。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32)
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::SeedFlagBad => "旗标格式坏：建议使用 --seed=N 十进制无符号格式",
            Diag::SeedZero => "零种子：建议换非零种子，0 保留为未注入哨兵",
            Diag::SeedTooLarge => "种子超上限：建议种子不超过 9 位十进制上限",
            Diag::OrderLenBad => "序长失配：建议核对执行序长度与用例总数一致",
            Diag::OrderNotPerm => "序非置换：建议执行序必须覆盖 1..=n 且不重不漏",
            Diag::LedgerMiss => "账本查无：建议先记录该种子再查账，查无显性回报",
            Diag::LedgerDiverge => "账本分叉：建议账本构建纯函数化，同种子同账",
            Diag::CaseCountBad => "用例数越界：建议用例数落在 1..=64 定容区",
            Diag::LcgNotInjective => "LCG 非单射：建议核对乘数为奇数与模 2^64 环绕",
            Diag::M203Diverge => "M-203分叉：建议同种子双跑顺序逐位一致后复测",
            Diag::SegGuardFail => "段位守卫失守：建议核对 SEG_BASE 的 0x40 对齐与批段窗",
            Diag::WritebackFail => "写回失守：建议完成时状态=已完成且摘要非空",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::SeedFlagBad,
    Diag::SeedZero,
    Diag::SeedTooLarge,
    Diag::OrderLenBad,
    Diag::OrderNotPerm,
    Diag::LedgerMiss,
    Diag::LedgerDiverge,
    Diag::CaseCountBad,
    Diag::LcgNotInjective,
    Diag::M203Diverge,
    Diag::SegGuardFail,
    Diag::WritebackFail,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    for d in ALL_DIAGS.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：建议核对是否落在 0x79C41..=0x79C4C 闭集内"
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + B14 批段窗 [0x79C00, 0x79DC0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79C00 && SEG_BASE <= 0x79DC0
}

/// 诊断闭集完整：12 码、编码连续、唯一、文案非空且带「建议」。
fn diag_closed_set_ok() -> bool {
    if ALL_DIAGS.len() != 12 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_DIAGS.len() {
        let a = match ALL_DIAGS.get(i) {
            Some(d) => *d,
            None => return false,
        };
        if a.val() != SEG_BASE + (i as u32) + 1 {
            return false;
        }
        if a.detail().is_empty() || !a.detail().contains("建议") {
            return false;
        }
        let mut j = i + 1;
        while j < ALL_DIAGS.len() {
            match ALL_DIAGS.get(j) {
                Some(b) => {
                    if a.val() == b.val() {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    detail(0xDEAD) == detail(0x5A5A) && !detail(0xDEAD).is_empty()
}

/// 种子注入接口正常流：`--seed=42` → 42；前导零按数值解析。
fn seed_flag_ok() -> bool {
    (match parse_seed_flag("--seed=42") {
        Ok(v) => v == 42,
        Err(_) => false,
    }) && parse_seed_flag("--seed=0000042") == Ok(42)
        && parse_seed_flag("--seed=999999999") == Ok(SEED_MAX)
}

/// 注入 1：坏旗标——缺前缀/空值/夹字母三类全部被 SeedFlagBad 点名。
fn inj_seed_flag_ok() -> bool {
    parse_seed_flag("seed=42") == Err(Diag::SeedFlagBad.val())
        && parse_seed_flag("--seed=") == Err(Diag::SeedFlagBad.val())
        && parse_seed_flag("--seed=12x4") == Err(Diag::SeedFlagBad.val())
        && parse_seed_flag("--seed=四十二") == Err(Diag::SeedFlagBad.val())
        && detail(Diag::SeedFlagBad.val()).contains("建议")
}

/// 注入 2：零种子与超上限——SeedZero / SeedTooLarge 双闸必抓。
fn inj_seed_bounds_ok() -> bool {
    parse_seed_flag("--seed=0") == Err(Diag::SeedZero.val())
        && parse_seed_flag("--seed=1000000000") == Err(Diag::SeedTooLarge.val())
        && parse_seed_flag("--seed=999999999999") == Err(Diag::SeedTooLarge.val())
        && order_for_seed(0, 8) == Err(Diag::SeedZero.val())
}

/// LCG 单射性：连续 8 个不同输入派生 8 个互异输出（可证性质逐点验证）。
fn lcg_injective_ok() -> bool {
    let mut out = [0u64; 8];
    let mut i = 0u64;
    while i < 8 {
        match out.get_mut(i as usize) {
            Some(slot) => *slot = lcg_next(i),
            None => return false,
        }
        i += 1;
    }
    let mut a = 0usize;
    while a < out.len() {
        let va = match out.get(a) {
            Some(v) => *v,
            None => return false,
        };
        let mut b = a + 1;
        while b < out.len() {
            let vb = match out.get(b) {
                Some(v) => *v,
                None => return false,
            };
            if va == vb {
                return false;
            }
            b += 1;
        }
        a += 1;
    }
    detail(Diag::LcgNotInjective.val()).contains("单射")
}

/// 执行序置换性：order_for_seed 产出恰为 1..=n 的完整置换。
fn order_permutation_ok() -> bool {
    let mut n = MIN_CASES;
    while n <= MAX_CASES {
        let order = match order_for_seed(42, n) {
            Ok(o) => o,
            Err(_) => return false,
        };
        if !is_permutation(&order, n) {
            return false;
        }
        n += 7;
    }
    true
}

/// 执行序边界：序内用例号全部落在 1..=n 且长度恰为 n。
fn order_bounds_ok() -> bool {
    let n = 16usize;
    let order = match order_for_seed(7, n) {
        Ok(o) => o,
        Err(_) => return false,
    };
    if order.len() != n {
        return false;
    }
    let mut i = 0usize;
    while i < order.len() {
        let id = match order.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if id == 0 || id as usize > n {
            return false;
        }
        i += 1;
    }
    detail(Diag::OrderLenBad.val()).contains("长度")
}

/// M-203 同种子双跑顺序逐位一致（三个种子点逐一复测）。
fn m203_double_run_ok() -> bool {
    let seeds = [1u64, 42, 999_999_999];
    let mut k = 0usize;
    while k < seeds.len() {
        let s = match seeds.get(k) {
            Some(v) => *v,
            None => return false,
        };
        let a = order_for_seed(s, 32);
        let b = order_for_seed(s, 32);
        if a != b || a.is_err() {
            return false;
        }
        k += 1;
    }
    detail(Diag::M203Diverge.val()).contains("逐位")
}

/// 注入 3：用例数越界——n=0 与 n=65 必被 CaseCountBad 点名。
fn inj_case_count_ok() -> bool {
    order_for_seed(42, 0) == Err(Diag::CaseCountBad.val())
        && order_for_seed(42, MAX_CASES + 1) == Err(Diag::CaseCountBad.val())
        && detail(Diag::CaseCountBad.val()).contains("定容")
}

/// 账本记录与查账：记账后按种子精确取回同序。
fn ledger_record_lookup_ok() -> bool {
    let mut ledger = SeedLedger::new();
    if ledger.record(42, 16).is_err() {
        return false;
    }
    if ledger.record(7, 8).is_err() {
        return false;
    }
    let e = match ledger.lookup(42) {
        Some(e) => e,
        None => return false,
    };
    e.seed == 42
        && e.case_count == 16
        && e.order == match order_for_seed(42, 16) {
            Ok(o) => o,
            Err(_) => return false,
        }
}

/// 注入 4：查无显性——未知种子查账必返回 None（LedgerMiss 语义）。
fn ledger_miss_ok() -> bool {
    let mut ledger = SeedLedger::new();
    if ledger.record(42, 8).is_err() {
        return false;
    }
    ledger.lookup(999).is_none() && detail(Diag::LedgerMiss.val()).contains("查无")
}

/// 账本双跑一致：两次独立建账逐条相同（LedgerDiverge 反向守卫）。
fn ledger_double_run_ok() -> bool {
    let mut a = SeedLedger::new();
    let mut b = SeedLedger::new();
    let mut i = 0u64;
    while i < 5 {
        let s = (i + 1) * 111;
        if a.record(s, 12).is_err() || b.record(s, 12).is_err() {
            return false;
        }
        i += 1;
    }
    a == b && !a.entries.is_empty()
}

/// 用例数定容表冻结：MIN_CASES=1、MAX_CASES=64 与置换核验表容量一致。
fn case_count_constants_ok() -> bool {
    MIN_CASES == 1
        && MAX_CASES == 64
        && SEED_MAX == 999_999_999
        && detail(Diag::SeedTooLarge.val()).contains("9 位")
}

/// 写回面：状态=已完成、结果摘要非空（含账本条数与 M-203 结论）。
fn writeback_summary_ok() -> bool {
    let mut ledger = SeedLedger::new();
    if ledger.record(2024, 16).is_err() {
        return false;
    }
    let order = match ledger.lookup(2024) {
        Some(e) => e.order.clone(),
        None => return false,
    };
    let summary = format!(
        "UNX-F3444 完成：种子2024账{}条 M-203双跑{} 首序用例{}",
        ledger.entries.len(),
        "逐位一致",
        match order.first() {
            Some(v) => *v,
            None => return false,
        },
    );
    !summary.is_empty() && summary.contains("UNX-F3444") && summary.contains("M-203")
        && detail(Diag::WritebackFail.val()).contains("写回")
}

// ── 判据总成：run_unx3444_checks ───────────────────────────────────────

/// run_unx3444_checks：本单判据聚合（诊断段 0x79C40，全部走 add_dyn）。
pub fn run_unx3444_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3444 随机化执行与种子账本");
    let checks: [(&'static str, bool, String); 15] = [
        (
            "段位守卫0x40对齐且在批段窗内",
            seg_guard_ok(),
            format!("SEG_BASE=0x{:X} 守卫{}", SEG_BASE, if seg_guard_ok() { "过" } else { "破" }),
        ),
        (
            "诊断码闭集12码唯一且文案带建议",
            diag_closed_set_ok(),
            format!("闭集{}", if diag_closed_set_ok() { "完整" } else { "破缺" }),
        ),
        (
            "种子注入接口--seed=N正常解析",
            seed_flag_ok(),
            format!("旗标{}", if seed_flag_ok() { "通" } else { "断" }),
        ),
        (
            "注入1坏旗标三类必抓",
            inj_seed_flag_ok(),
            format!("坏旗标注入{}", if inj_seed_flag_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入2零种子与超上限必抓",
            inj_seed_bounds_ok(),
            format!("种子越界注入{}", if inj_seed_bounds_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "LCG单射性连续8点互异",
            lcg_injective_ok(),
            format!("LCG{}", if lcg_injective_ok() { "单射" } else { "碰撞" }),
        ),
        (
            "执行序为1..=n完整置换",
            order_permutation_ok(),
            format!("置换{}", if order_permutation_ok() { "完整" } else { "破缺" }),
        ),
        (
            "执行序边界全部落在1..=n",
            order_bounds_ok(),
            format!("序界{}", if order_bounds_ok() { "守住" } else { "越界" }),
        ),
        (
            "M-203同种子双跑顺序逐位一致",
            m203_double_run_ok(),
            format!("M-203 {}", if m203_double_run_ok() { "一致" } else { "分叉" }),
        ),
        (
            "注入3用例数越界必抓",
            inj_case_count_ok(),
            format!("用例数注入{}", if inj_case_count_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "账本记录后按种子精确取回",
            ledger_record_lookup_ok(),
            format!("账本{}", if ledger_record_lookup_ok() { "可查" } else { "失记" }),
        ),
        (
            "注入4未知种子查无显性",
            ledger_miss_ok(),
            format!("查无注入{}", if ledger_miss_ok() { "显性" } else { "静默" }),
        ),
        (
            "账本双跑两次建账逐条同",
            ledger_double_run_ok(),
            format!("账本双跑{}", if ledger_double_run_ok() { "一致" } else { "分叉" }),
        ),
        (
            "定容常量表冻结",
            case_count_constants_ok(),
            format!("常量表{}", if case_count_constants_ok() { "冻结" } else { "漂移" }),
        ),
        (
            "写回摘要非空状态已完成",
            writeback_summary_ok(),
            format!("写回{}", if writeback_summary_ok() { "非空" } else { "空" }),
        ),
    ];
    for (name, passed, det) in checks.iter() {
        s.add_dyn(name, *passed, det);
    }
    s
}

// ── 测试区（#[cfg(test)]：允许 assert!/assert_eq!/unwrap）──────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_and_diag_closed_set() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x79C40);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
    }

    #[test]
    fn seed_flag_parse_paths() {
        assert!(seed_flag_ok());
        assert_eq!(parse_seed_flag("--seed=1"), Ok(1));
        assert_eq!(parse_seed_flag("--seed=0000042"), Ok(42));
        assert!(inj_seed_flag_ok());
        assert!(inj_seed_bounds_ok());
    }

    #[test]
    fn lcg_injective_and_constants() {
        assert!(lcg_injective_ok());
        assert_ne!(lcg_next(0), lcg_next(1));
        assert_ne!(lcg_next(u64::MAX), lcg_next(1));
    }

    #[test]
    fn order_is_permutation_for_all_sizes() {
        assert!(order_permutation_ok());
        assert!(order_bounds_ok());
        let o = order_for_seed(42, 5).unwrap();
        assert_eq!(o.len(), 5);
        assert!(is_permutation(&o, 5));
    }

    #[test]
    fn m203_same_seed_double_run_identical() {
        assert!(m203_double_run_ok());
        let a = order_for_seed(42, 16).unwrap();
        let b = order_for_seed(42, 16).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn injection_case_count_bounds() {
        assert!(inj_case_count_ok());
        assert_eq!(order_for_seed(42, 0), Err(Diag::CaseCountBad.val()));
        assert_eq!(order_for_seed(42, 65), Err(Diag::CaseCountBad.val()));
    }

    #[test]
    fn ledger_record_lookup_and_miss() {
        assert!(ledger_record_lookup_ok());
        assert!(ledger_miss_ok());
        let mut l = SeedLedger::new();
        l.record(9, 4).unwrap();
        assert!(l.lookup(9).is_some());
        assert!(l.lookup(10).is_none());
    }

    #[test]
    fn ledger_double_run_identical() {
        assert!(ledger_double_run_ok());
    }

    #[test]
    fn constants_frozen() {
        assert!(case_count_constants_ok());
        assert_eq!(SEED_MAX, 999_999_999);
        assert_eq!(MAX_CASES, 64);
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn order_key_deterministic_and_varied() {
        // 同输入键恒同；不同用例号键互异（同种子下）。
        assert_eq!(order_key(42, 1), order_key(42, 1));
        assert_ne!(order_key(42, 1), order_key(42, 2));
        assert_ne!(order_key(41, 1), order_key(42, 1));
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3444_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
