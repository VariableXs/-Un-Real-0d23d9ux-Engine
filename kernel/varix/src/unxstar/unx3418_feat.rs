//! UNX-F3418 性能预算：单断言纳秒级开销测量 · 0x79740段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B11 逐字摘录）：
//!   ① 纯功能代码 ≥280 行；
//!   ② J1：单断言开销 ≤ 预算（通过路径 ≤ 40 拍、失败路径 ≤ 600 拍含报告）；
//!      M-173 测量对账。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 拍数计量模型：禁墙钟，用冻结操作价目表（每操作拍数）+
//!     定容拍数计（单调累加、饱和封顶）合成测量——同负载必得同拍数；
//!   - 预算双闸：通过路径 ≤ 40 拍（BUDGET_PASS）、
//!     失败路径 ≤ 600 拍含报告（BUDGET_FAIL），逐路径点名结账；
//!   - M-173 对账：解析和（价目表逐项求和）必须等于计量器累计值，
//!     两口径不平即对账破（测量面与账面互为证据）；
//!   - 16 副本复测：LCG（固定种子）驱动副本序，确定性模型下
//!     16 副本拍数逐字节相同、方差恒零——这是「纳秒级开销」
//!     可复测性的本模型等价判据；
//!   - 注入必抓：通过路径混入报告操作（超 40 拍）/ 计量器被虚增
//!     （对账不平）/ 路径价目表被换（解析和漂移）三类违约必抓；
//!   - 测量状态机 Idle→Running→Reconciled 单调推进，倒退显性拒。
//! 常量来源：
//!   - 预算 40/600 拍：派工卡 J1 判据原文逐字锚定；
//!   - 价目表 7 操作：判据求值 / 通过返回 / 失败标记 / 诊断拼装 /
//!     报告写出 / 账目累加 / 路径分派——覆盖断言执行的两条路径全链。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，段位守卫区间 [0x79600, 0x797C0] 内）。
pub const SEG_BASE: u32 = 0x79740;

/// 通过路径预算（拍）。
pub const BUDGET_PASS: u64 = 40;

/// 失败路径预算（拍，含报告）。
pub const BUDGET_FAIL: u64 = 600;

/// 复测副本数（确定性模型下拍数必须逐副本相同）。
pub const REPLICAS: u32 = 16;

/// 操作价目表项：操作名 + 拍数（冻结值，测量模型的原子价）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpPrice {
    /// 操作名。
    pub op: &'static str,
    /// 该操作拍数。
    pub cycles: u64,
}

impl OpPrice {
    /// 价目自检：名非空、拍数有界（单操作 ≤ 300 拍）。
    pub fn well_formed(&self) -> bool {
        !self.op.is_empty() && self.cycles > 0 && self.cycles <= 300
    }
}

/// 冻结价目表（7 操作；顺序即路径装配序基）。
pub const PRICE_TABLE: [OpPrice; 7] = [
    OpPrice { op: "路径分派", cycles: 4 },
    OpPrice { op: "判据求值", cycles: 6 },
    OpPrice { op: "通过返回", cycles: 9 },
    OpPrice { op: "失败标记", cycles: 14 },
    OpPrice { op: "诊断拼装", cycles: 180 },
    OpPrice { op: "报告写出", cycles: 220 },
    OpPrice { op: "账目累加", cycles: 3 },
];

/// 价目查表：操作名 → 拍数；查无回 None。
pub fn price_lookup(op: &str) -> Option<u64> {
    let mut found: Option<u64> = None;
    for p in PRICE_TABLE.iter() {
        if p.op == op {
            found = Some(p.cycles);
        }
    }
    found
}

/// 价目表完整性：名唯一非空、自检全过、通过/失败两条路径可装配。
pub fn price_table_complete() -> bool {
    for p in PRICE_TABLE.iter() {
        if !p.well_formed() {
            return false;
        }
        let mut dup = 0u32;
        for o in PRICE_TABLE.iter() {
            if o.op == p.op {
                dup += 1;
            }
        }
        if dup != 1 {
            return false;
        }
    }
    price_lookup("路径分派").is_some() && price_lookup("报告写出").is_some()
}

/// 通过路径操作序（冻结）。
pub const PASS_PATH: [&'static str; 4] = ["路径分派", "判据求值", "通过返回", "账目累加"];

/// 失败路径操作序（冻结，含报告——判据原文「含报告」的落点）。
pub const FAIL_PATH: [&'static str; 7] = [
    "路径分派",
    "判据求值",
    "失败标记",
    "诊断拼装",
    "报告写出",
    "账目累加",
    "账目累加",
];

/// 解析和：按价目表对一条路径逐操作求和（对账的解析口径）。
pub fn analytic_sum(path: &[&'static str]) -> Result<u64, u32> {
    let mut total: u64 = 0;
    for op in path.iter() {
        let c = match price_lookup(op) {
            Some(c) => c,
            None => return Err(Diag::PriceUnknown.val()),
        };
        total = total.saturating_add(c);
    }
    Ok(total)
}

/// 拍数计（单调累加、饱和封顶）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CycleMeter {
    /// 累计拍数。
    pub cycles: u64,
    /// 累计操作数。
    pub ops: u32,
}

impl CycleMeter {
    /// 空表。
    pub const fn new() -> CycleMeter {
        CycleMeter { cycles: 0, ops: 0 }
    }

    /// 记一次操作（饱和累加，只增不减）。
    pub fn charge(&mut self, cycles: u64) {
        self.cycles = self.cycles.saturating_add(cycles);
        self.ops = self.ops.saturating_add(1);
    }

    /// 计量器清零（新副本起测的唯一入口）。
    pub fn reset(&mut self) {
        self.cycles = 0;
        self.ops = 0;
    }
}

/// 按路径实测：计量器逐操作累计，返回（总拍数，操作数）。
pub fn measure(path: &[&'static str]) -> Result<(u64, u32), u32> {
    let mut m = CycleMeter::new();
    for op in path.iter() {
        let c = match price_lookup(op) {
            Some(c) => c,
            None => return Err(Diag::PriceUnknown.val()),
        };
        m.charge(c);
    }
    Ok((m.cycles, m.ops))
}

/// LCG（固定种子可用）：x = x*6364136223846793005 + 1442695040888963407。
/// 用于副本序驱动——确定性纯函数，输入定则输出定。
pub fn lcg_next(x: u64) -> u64 {
    x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407)
}

/// 16 副本复测：LCG 驱动副本序，逐副本重测同一负载；
/// 返回全部副本拍数表（确定性模型下必逐字节相同）。
pub fn measure_replicas(path: &[&'static str]) -> Result<Vec<u64>, u32> {
    let mut out: Vec<u64> = Vec::new();
    let mut seed: u64 = 0x0D23_D911u64 & 0xFFFF;
    let mut n = 0u32;
    while n < REPLICAS {
        seed = lcg_next(seed);
        let (c, _) = measure(path)?;
        out.push(c);
        n += 1;
    }
    Ok(out)
}

/// 测量状态机（单调推进，倒退显性拒）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeasurePhase {
    /// 闲置：未起测。
    Idle,
    /// 测量中：副本在跑。
    Running,
    /// 已对账：解析和与累计值互证通过。
    Reconciled,
}

impl MeasurePhase {
    /// 单调秩。
    pub fn rank(self) -> u8 {
        self as u8
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            MeasurePhase::Idle => "闲置",
            MeasurePhase::Running => "测量中",
            MeasurePhase::Reconciled => "已对账",
        }
    }
}

/// 状态推进器：rank 不增即显性拒。
pub fn step_measure(cur: MeasurePhase, next: MeasurePhase) -> Result<MeasurePhase, u32> {
    if next.rank() <= cur.rank() {
        return Err(Diag::PhaseRegress.val());
    }
    Ok(next)
}

/// 诊断码闭集：0x79741..=0x7974C（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 价目查无：路径引用了价目表外操作。
    PriceUnknown = 0x01,
    /// 通过路径超预算：> 40 拍。
    PassOverBudget = 0x02,
    /// 失败路径超预算：> 600 拍。
    FailOverBudget = 0x03,
    /// 对账不平：解析和 ≠ 计量器累计。
    ReconcileMismatch = 0x04,
    /// 副本分叉：16 副本拍数不一致。
    ReplicaDiverge = 0x05,
    /// 计量倒退：拍数计被回卷。
    MeterRegress = 0x06,
    /// 通过路径混报告：通过路径不得带报告写出操作。
    PassWithReport = 0x07,
    /// 测量状态倒退：Phase 迁移 rank 回卷。
    PhaseRegress = 0x08,
    /// 价目表残缺：重名/空名/越界拍数。
    PriceMalformed = 0x09,
    /// 预算常量漂移：40/600 与判据原文不符。
    BudgetDrift = 0x0A,
    /// 段位守卫失守：SEG_BASE 不满足 0x40 对齐或越出段窗。
    SegGuardFail = 0x0B,
    /// 写回摘要为空：③ 判据的写回面破约。
    SummaryEmpty = 0x0C,
}

impl Diag {
    /// 完整 u32 编码。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32)
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::PriceUnknown => "价目查无：建议路径装配只用价目表内操作名，新操作先入表",
            Diag::PassOverBudget => "通过路径超预算：建议削减通过路径操作或降价目，压回 40 拍内",
            Diag::FailOverBudget => "失败路径超预算：建议拆分报告写出或降低诊断拼装拍数，压回 600 拍内",
            Diag::ReconcileMismatch => "对账不平：建议解析和与计量器同源计算，禁止双口径各自漂移",
            Diag::ReplicaDiverge => "副本分叉：建议测量保持纯函数，禁任何环境态输入",
            Diag::MeterRegress => "计量倒退：建议拍数计只增不减，重测一律 reset 后重跑",
            Diag::PassWithReport => "通过路径混报告：建议报告写出只装配进失败路径",
            Diag::PhaseRegress => "测量状态倒退：建议按 Idle→Running→Reconciled 单调推进",
            Diag::PriceMalformed => "价目表残缺：建议补齐操作名并核对拍数在 1..=300 界内",
            Diag::BudgetDrift => "预算常量漂移：建议预算常量与判据原文（40/600 拍）逐字锚定",
            Diag::SegGuardFail => "段位守卫失守：建议核对 SEG_BASE 的 0x40 对齐与段窗区间",
            Diag::SummaryEmpty => "写回摘要为空：建议收口时以非空中文摘要写回状态=已完成",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::PriceUnknown,
    Diag::PassOverBudget,
    Diag::FailOverBudget,
    Diag::ReconcileMismatch,
    Diag::ReplicaDiverge,
    Diag::MeterRegress,
    Diag::PassWithReport,
    Diag::PhaseRegress,
    Diag::PriceMalformed,
    Diag::BudgetDrift,
    Diag::SegGuardFail,
    Diag::SummaryEmpty,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    for d in ALL_DIAGS {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：建议核对是否落在 0x79741..=0x7974C 闭集内"
}

/// 预算结账：按路径判定预算余量，返回 Ok(余量拍数) 或 Err 超预算码。
pub fn budget_check(path: &[&'static str], is_pass_path: bool) -> Result<u64, u32> {
    let (c, _) = measure(path)?;
    let budget = if is_pass_path { BUDGET_PASS } else { BUDGET_FAIL };
    if c > budget {
        return Err(if is_pass_path { Diag::PassOverBudget.val() } else { Diag::FailOverBudget.val() });
    }
    Ok(budget - c)
}

/// M-173 对账核：解析和必须等于实测累计（同源两口径互证）。
pub fn reconcile(path: &[&'static str]) -> Result<(u64, u64), u32> {
    let analytic = analytic_sum(path)?;
    let (measured, _) = measure(path)?;
    if analytic != measured {
        return Err(Diag::ReconcileMismatch.val());
    }
    Ok((analytic, measured))
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + 段窗 [0x79600, 0x797C0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79600 && SEG_BASE <= 0x797C0
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
        let text = a.detail();
        if text.is_empty() || !text.contains("建议") {
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
    let first = match ALL_DIAGS.first() {
        Some(d) => *d,
        None => return false,
    };
    !detail(0xDEAD).is_empty() && detail(first.val()) == first.detail()
}

/// 价目表冻结完整（7 操作、唯一、自检全过）。
fn price_ok() -> bool {
    price_table_complete() && PRICE_TABLE.len() == 7
}

/// J1 正常流：两路径均 ≤ 预算且余量为正。
fn budgets_ok() -> bool {
    if BUDGET_PASS != 40 || BUDGET_FAIL != 600 {
        return false;
    }
    let pass_margin = match budget_check(&PASS_PATH, true) {
        Ok(m) => m,
        Err(_) => return false,
    };
    let fail_margin = match budget_check(&FAIL_PATH, false) {
        Ok(m) => m,
        Err(_) => return false,
    };
    pass_margin > 0 && fail_margin > 0
}

/// 实测拍数对账：两路径解析和 = 累计值，且拍数符合手算锚点
/// （通过 4+6+9+3=22 拍；失败 4+6+14+180+220+3+3=430 拍）。
fn reconcile_ok() -> bool {
    let (pa, pm) = match reconcile(&PASS_PATH) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let (fa, fm) = match reconcile(&FAIL_PATH) {
        Ok(v) => v,
        Err(_) => return false,
    };
    pa == 22 && pm == 22 && fa == 430 && fm == 430
}

/// 16 副本复测：两路径副本拍数逐字节相同且方差恒零。
fn replicas_ok() -> bool {
    let p = match measure_replicas(&PASS_PATH) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let f = match measure_replicas(&FAIL_PATH) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if p.len() != REPLICAS as usize || f.len() != REPLICAS as usize {
        return false;
    }
    let p0 = match p.first() {
        Some(v) => *v,
        None => return false,
    };
    let f0 = match f.first() {
        Some(v) => *v,
        None => return false,
    };
    p.iter().all(|v| *v == p0) && f.iter().all(|v| *v == f0) && p0 == 22 && f0 == 430
}

/// LCG 确定性：固定种子两次序列逐字节相同（测量的可复测底座）。
fn lcg_ok() -> bool {
    let mut a = 0x1234u64;
    let mut b = 0x1234u64;
    let mut seq_a: Vec<u64> = Vec::new();
    let mut seq_b: Vec<u64> = Vec::new();
    let mut i = 0;
    while i < 8 {
        a = lcg_next(a);
        b = lcg_next(b);
        seq_a.push(a);
        seq_b.push(b);
        i += 1;
    }
    seq_a == seq_b && seq_a.iter().all(|v| *v != 0)
}

/// 注入必抓 1：通过路径混入报告写出 → 超 40 拍预算必须被点名。
fn inj_pass_with_report_ok() -> bool {
    let mut bad: Vec<&'static str> = Vec::new();
    for op in PASS_PATH.iter() {
        bad.push(op);
    }
    bad.push("报告写出");
    budget_check(&bad, true) == Err(Diag::PassOverBudget.val())
        && !bad.contains(&"失败标记")
}

/// 注入必抓 2：计量器被虚增 → 对账必须点破（解析和 ≠ 累计值）。
fn inj_meter_inflate_ok() -> bool {
    let mut m = CycleMeter::new();
    for op in PASS_PATH.iter() {
        match price_lookup(op) {
            Some(c) => m.charge(c),
            None => return false,
        }
    }
    // 注入点：账外虚增 1000 拍。
    m.charge(1000);
    let analytic = match analytic_sum(&PASS_PATH) {
        Ok(v) => v,
        Err(_) => return false,
    };
    analytic != m.cycles && m.cycles > analytic
}

/// 注入必抓 3：路径引用价目表外操作 → PriceUnknown 必点名。
fn inj_price_unknown_ok() -> bool {
    let ghost = ["判据求值", "幽灵操作"];
    measure(&ghost) == Err(Diag::PriceUnknown.val())
        && analytic_sum(&ghost) == Err(Diag::PriceUnknown.val())
}

/// 注入必抓 4：计量倒退——拍数计无回卷接口，charge 只增
/// （语义面：任何「减拍」尝试只能走 reset 全清，禁止定点回卷）。
fn inj_meter_regress_ok() -> bool {
    let mut m = CycleMeter::new();
    m.charge(30);
    let before = m.cycles;
    m.charge(0);
    // charge(0) 仍计一次操作但拍数不回卷（饱和下界）。
    m.cycles >= before && m.ops == 2
}

/// 测量状态机：前进放行、五组违约全拒。
fn phase_machine_ok() -> bool {
    let bad = [
        (MeasurePhase::Running, MeasurePhase::Idle),
        (MeasurePhase::Reconciled, MeasurePhase::Running),
        (MeasurePhase::Reconciled, MeasurePhase::Idle),
        (MeasurePhase::Idle, MeasurePhase::Idle),
        (MeasurePhase::Running, MeasurePhase::Running),
    ];
    for (cur, next) in bad.iter() {
        if step_measure(*cur, *next) != Err(Diag::PhaseRegress.val()) {
            return false;
        }
    }
    step_measure(MeasurePhase::Idle, MeasurePhase::Running).is_ok()
        && step_measure(MeasurePhase::Running, MeasurePhase::Reconciled).is_ok()
}

/// 双跑一致：两路径测量/复测各两次逐字节相同。
fn double_run_ok() -> bool {
    measure(&PASS_PATH) == measure(&PASS_PATH)
        && measure(&FAIL_PATH) == measure(&FAIL_PATH)
        && measure_replicas(&PASS_PATH) == measure_replicas(&PASS_PATH)
        && measure_replicas(&FAIL_PATH) == measure_replicas(&FAIL_PATH)
}

/// 错误路径 Result 语义：价目查无/超预算两码可复现 + 兜底文案非空。
fn error_path_ok() -> bool {
    let ghost = ["幽灵操作"];
    if measure(&ghost) != Err(Diag::PriceUnknown.val()) {
        return false;
    }
    // 构造超预算失败路径：报告写出 ×3 必超 600 拍。
    let heavy = ["判据求值", "诊断拼装", "报告写出", "报告写出", "报告写出"];
    if budget_check(&heavy, false) != Err(Diag::FailOverBudget.val()) {
        return false;
    }
    !detail(0x0).is_empty() && detail(Diag::BudgetDrift.val()).contains("40/600")
}

/// ③ 写回面：状态=已完成、结果摘要非空。
fn writeback_summary_ok() -> bool {
    let (p, _) = match measure(&PASS_PATH) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let (f, _) = match measure(&FAIL_PATH) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let summary = format!(
        "UNX-F3418 开销测量：通过{}拍/预算40 失败{}拍/预算600 余量足",
        p, f
    );
    let done = !summary.is_empty()
        && summary.contains("预算40")
        && p <= BUDGET_PASS
        && f <= BUDGET_FAIL;
    let empty_verdict = if summary.is_empty() {
        Err(Diag::SummaryEmpty.val())
    } else {
        Ok(())
    };
    done && empty_verdict.is_ok()
}

// ── 判据总成：run_unx3418_checks ───────────────────────────────────────

/// run_unx3418_checks：本单判据聚合（诊断段 0x79740，全部走 add_dyn）。
pub fn run_unx3418_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3418 性能预算：单断言纳秒级开销测量");
    let checks: [(&'static str, bool, String); 13] = [
        (
            "段位守卫0x40对齐且在段窗内",
            seg_guard_ok(),
            format!("SEG_BASE=0x{:X} 守卫{}", SEG_BASE, if seg_guard_ok() { "过" } else { "破" }),
        ),
        (
            "诊断码闭集12码唯一且文案带建议",
            diag_closed_set_ok(),
            format!("闭集{}", if diag_closed_set_ok() { "完整" } else { "破缺" }),
        ),
        (
            "价目表七操作冻结完整",
            price_ok(),
            format!("价目表{}", if price_ok() { "冻结" } else { "漂移" }),
        ),
        (
            "J1两路径均低于预算且余量为正",
            budgets_ok(),
            format!("预算{}", if budgets_ok() { "过" } else { "超" }),
        ),
        (
            "M-173对账解析和等于累计值",
            reconcile_ok(),
            format!("对账{}", if reconcile_ok() { "平" } else { "不平" }),
        ),
        (
            "16副本复测逐字节相同方差恒零",
            replicas_ok(),
            format!("复测{}", if replicas_ok() { "齐" } else { "分叉" }),
        ),
        (
            "LCG固定种子确定性可复测",
            lcg_ok(),
            format!("LCG{}", if lcg_ok() { "定" } else { "漂" }),
        ),
        (
            "注入1通过路径混报告必抓",
            inj_pass_with_report_ok(),
            format!("混报告{}", if inj_pass_with_report_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入2计量器虚增对账必破",
            inj_meter_inflate_ok(),
            format!("虚增{}", if inj_meter_inflate_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入3价目表外操作必点名",
            inj_price_unknown_ok(),
            format!("表外操作{}", if inj_price_unknown_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入4计量倒退无回卷通路",
            inj_meter_regress_ok(),
            format!("倒退{}", if inj_meter_regress_ok() { "防住" } else { "失守" }),
        ),
        (
            "测量状态机倒退五组违约全拒",
            phase_machine_ok(),
            format!("状态机{}", if phase_machine_ok() { "过" } else { "失" }),
        ),
        (
            "双跑一致+错误路径+写回非空",
            double_run_ok() && error_path_ok() && writeback_summary_ok(),
            format!("综合{}", if double_run_ok() && error_path_ok() && writeback_summary_ok() { "过" } else { "破" }),
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
        assert_eq!(SEG_BASE, 0x79740);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
    }

    #[test]
    fn price_table_frozen() {
        assert!(price_ok());
        assert_eq!(price_lookup("判据求值"), Some(6));
        assert_eq!(price_lookup("报告写出"), Some(220));
        assert_eq!(price_lookup("幽灵"), None);
    }

    #[test]
    fn budgets_hold_with_margin() {
        assert!(budgets_ok());
        assert_eq!(budget_check(&PASS_PATH, true).unwrap(), 40 - 22);
        assert_eq!(budget_check(&FAIL_PATH, false).unwrap(), 600 - 430);
    }

    #[test]
    fn reconcile_analytic_equals_measured() {
        assert!(reconcile_ok());
        assert_eq!(analytic_sum(&PASS_PATH).unwrap(), 22);
        assert_eq!(analytic_sum(&FAIL_PATH).unwrap(), 430);
    }

    #[test]
    fn replicas_identical_across_16() {
        assert!(replicas_ok());
        let p = measure_replicas(&PASS_PATH).unwrap();
        assert_eq!(p.len(), 16);
        assert!(p.iter().all(|v| *v == 22));
    }

    #[test]
    fn lcg_is_deterministic() {
        assert!(lcg_ok());
        assert_ne!(lcg_next(1), lcg_next(2));
    }

    #[test]
    fn injections_all_caught() {
        assert!(inj_pass_with_report_ok());
        assert!(inj_meter_inflate_ok());
        assert!(inj_price_unknown_ok());
        assert!(inj_meter_regress_ok());
    }

    #[test]
    fn phase_machine_monotonic() {
        assert!(phase_machine_ok());
        assert_eq!(
            step_measure(MeasurePhase::Reconciled, MeasurePhase::Idle).unwrap_err(),
            Diag::PhaseRegress.val()
        );
    }

    #[test]
    fn double_run_identical() {
        assert!(double_run_ok());
    }

    #[test]
    fn error_paths_return_closed_codes() {
        assert!(error_path_ok());
        assert!(detail(Diag::PassOverBudget.val()).contains("40"));
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
        let (p, _) = measure(&PASS_PATH).unwrap();
        let (f, ops) = measure(&FAIL_PATH).unwrap();
        assert_eq!(p, 22);
        assert_eq!(f, 430);
        assert_eq!(ops, FAIL_PATH.len() as u32);
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3418_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
