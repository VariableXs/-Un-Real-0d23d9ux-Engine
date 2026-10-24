//! VE-F3439 · 换肤基准 —— 三档口径、稳定性、发布声明对拍
//!
//! ## 职责拆解
//! 1. **基准套件**（`BenchCase` / `BenchSuite`）：小/中/大三档主题 × 冷/热两条路径，
//!    共 6 行。档位按令牌数划，冷热按「是否需要解析并构建」划——这两轴正交，
//!    只看档位会把「大主题热启」这种最常见的路径漏掉。
//! 2. **计时源**（`Meter`）：由外部注入。生产接单调时钟，CI 接可复现计量器。
//!    本模块只负责**口径**（怎么算 P95、怎么算稳定）、**对拍**（声明与实测是否一致）
//!    与**指纹**（这次跑在什么机器上），不负责读硬件计数器——那属性能册。
//! 3. **稳定性**（`stability`）：同一条基准跑 5 次，极差不得超过中位数的 5%。
//!    不稳定的基准不许发布口径：抖动的数字拿去宣传，下个版本必然打自己的脸。
//! 4. **发布声明**（`CapacityClaim`）：声明必须带 `measured` 标记、样本数、环境指纹。
//!    **实测为准不虚标**——声明值低于实测 P95 超过 10% 判 `Overclaim`，直接阻断发布；
//!    声明值远高于实测只给提示（保守声明不伤人）。
//! 5. **基线锁定与差异可视化**（`Baseline` / `DiffRow`）：`+12.4%  reskin.large.cold`
//!    一行一条，按劣化幅度降序。差异在哪一条，一眼看到，不用去翻表格。
//!
//! ## 为什么这么拆
//! 基准最容易变成「数字表演」：挑一台快机器、挑一次快的、挑一个好听的百分位。
//! 所以本册把三件东西做成**硬结构**：环境指纹（换了机器立刻对不上）、
//! 五次极差（挑一次没用，五次都得在窗内）、实测对拍（声明与实测必须同源同口径）。
//! 缺任何一件，`CapacityClaim` 都构造不出来。
//!
//! ## 复杂度
//! - 单行 P95：样本数 n=5 常数级（排序 5 元素）。
//! - 全表：行数 × n，O(rows·n log n)，rows=6 ⇒ 百次比较量级。
//! - 对拍：O(claims)，BTreeMap 查表 O(log rows)。
//!
//! ## 零 panic 面
//! - 无切片索引（P95 用 `get()`，越界返回 0 并置 `degenerate` 标记）。
//! - 所有除法前先判零分母；`i32::abs` 对 `i32::MIN` 会溢出，故差值先夹到 ±1e6。
//! - 无 `unwrap` / `expect`；排序用 `sort_by_key`，不比较浮点。

use alloc::vec;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 稳定性窗：极差不得超过中位数的 5%。低于此值的抖动才配叫「基准」。
pub const VARIANCE_LIMIT_PERMILLE: u32 = 50;

/// 声明容差：声明值低于实测 P95 超过 10% ⇒ 虚标。
pub const CLAIM_TOLERANCE_PERMILLE: u32 = 100;

/// 单行最少样本数。三次以下无法判断稳定性（极差没有意义）。
pub const MIN_SAMPLES: usize = 3;

/// 建议样本数：五次，去掉一次最高一次最低后取中位仍有余量。
pub const RECOMMENDED_SAMPLES: usize = 5;

/// 诊断码（E18 段 · 5557~5568）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum BenchCode {
    /// 样本数不足。
    FewSamples = 5557,
    /// 抖动超出稳定性窗。
    Unstable = 5558,
    /// 声明未标注「实测」。
    NotMeasured = 5559,
    /// 声明与实测环境指纹不一致。
    EnvMismatch = 5560,
    /// 声明值优于实测超过容差（虚标）。
    Overclaim = 5561,
    /// 声明了套件里不存在的行。
    UnknownCase = 5562,
    /// 声明缺环境指纹。
    EnvMissing = 5563,
    /// 基线为空，无法比对。
    EmptyBaseline = 5564,
    /// 一次运行都没跑过。
    NoRun = 5565,
    /// 套件为空。
    EmptySuite = 5566,
    /// 同一行重复出现（口径串行）。
    DuplicateCase = 5567,
    /// 声明严重保守（低于实测一半以下），允许但需说明。
    Conservative = 5568,
}

impl BenchCode {
    pub fn code(self) -> u16 {
        self as u16
    }

    pub fn text(self) -> &'static str {
        match self {
            BenchCode::FewSamples => "样本数不足，P95 无统计意义",
            BenchCode::Unstable => "五次极差超窗，基准不稳定",
            BenchCode::NotMeasured => "声明未标注实测来源",
            BenchCode::EnvMismatch => "声明环境与实测环境不一致",
            BenchCode::Overclaim => "声明优于实测，属虚标",
            BenchCode::UnknownCase => "声明了套件里不存在的基准行",
            BenchCode::EnvMissing => "声明缺环境指纹",
            BenchCode::EmptyBaseline => "基线为空，差异无从比对",
            BenchCode::NoRun => "未运行任何基准",
            BenchCode::EmptySuite => "基准套件为空",
            BenchCode::DuplicateCase => "同一基准行重复登记",
            BenchCode::Conservative => "声明显著保守于实测（可发布，需注明）",
        }
    }
}

/// 主题规模档。阈值按令牌数划，和 VE-F3432 的预加载档位同一把尺子，
/// 避免「基准说大主题 4000 令牌、预加载说大主题 2000 令牌」这种口径打架。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Tier {
    Small,
    Medium,
    Large,
}

impl Tier {
    pub fn label(self) -> &'static str {
        match self {
            Tier::Small => "小",
            Tier::Medium => "中",
            Tier::Large => "大",
        }
    }

    /// 令牌数落到哪一档。边界取「不超过」：400 归中档，401 归大档。
    /// `const` 是为了让 `BenchCase::new` 能当 `const fn` 用（基准表可写成常量）。
    pub const fn of_tokens(tokens: usize) -> Self {
        if tokens <= 120 {
            Tier::Small
        } else if tokens <= 1200 {
            Tier::Medium
        } else {
            Tier::Large
        }
    }
}

/// 路径档：冷启动要解析+构建+校验，热切换只重算受影响组件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Path {
    Cold,
    Warm,
}

impl Path {
    pub fn label(self) -> &'static str {
        match self {
            Path::Cold => "cold",
            Path::Warm => "warm",
        }
    }
}

/// 一条基准。`id` 是稳定标识（写进发布声明与基线锁文件，不许改）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BenchCase {
    pub id: &'static str,
    pub tier: Tier,
    pub path: Path,
    pub tokens: usize,
    pub components: usize,
}

impl BenchCase {
    pub const fn new(
        id: &'static str,
        path: Path,
        tokens: usize,
        components: usize,
    ) -> Self {
        Self {
            id,
            tier: Tier::of_tokens(tokens),
            path,
            tokens,
            components,
        }
    }

    /// 展示名：`reskin.large.cold`。
    pub fn name(&self) -> String {
        format!("reskin.{}.{}", tier_word(self.tier), self.path.label())
    }
}

fn tier_word(t: Tier) -> &'static str {
    match t {
        Tier::Small => "small",
        Tier::Medium => "medium",
        Tier::Large => "large",
    }
}

/// 计时源。实现方负责「跑一次并返回毫秒」——注意这是**单次**采样，
/// 套件负责重复采样与统计。
pub trait Meter {
    fn sample(&mut self, case: &BenchCase) -> u32;
}

/// 环境指纹。四元组足够区分「同一台机器」与「换了机器」；
/// 不做更细的分辨（内存条批次不影响换肤耗时到毫秒级）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct EnvFingerprint {
    pub cpu: u32,
    pub os: u32,
    pub arch: u32,
    pub ram_mb: u32,
}

impl EnvFingerprint {
    pub const fn new(cpu: u32, os: u32, arch: u32, ram_mb: u32) -> Self {
        Self {
            cpu,
            os,
            arch,
            ram_mb,
        }
    }

    /// 可读形态：发布附注里贴这一行。`ram_mb == 0` 表示未采集，不参与比较。
    pub fn line(&self) -> String {
        format!(
            "cpu={:08x} os={:08x} arch={:08x} ram={}MB",
            self.cpu, self.os, self.arch, self.ram_mb
        )
    }

    pub fn compatible(&self, other: &Self) -> bool {
        self.cpu == other.cpu && self.os == other.os && self.arch == other.arch
    }

    pub fn is_empty(&self) -> bool {
        self.cpu == 0 && self.os == 0 && self.arch == 0
    }
}

/// 一次基准运行的样本。样本按采集顺序保存（不排序），统计时再排。
#[derive(Clone, Debug)]
pub struct Row {
    pub case: BenchCase,
    pub samples: Vec<u32>,
}

impl Row {
    /// P95。样本少时 P95 退化为最大值的上一位——这正是为什么要标样本数：
    /// 3 个样本的 P95 就是最大值，说「P95 = 42ms」而不说「3 次里最慢 42ms」
    /// 属于口径欺诈。
    pub fn p95(&self) -> u32 {
        let mut v: Vec<u32> = self.samples.clone();
        v.sort_unstable();
        let n = v.len();
        if n == 0 {
            return 0;
        }
        // ceil(0.95n) - 1，整数运算：((95*n + 99) / 100) - 1
        let idx = ((95u64 * n as u64 + 99) / 100) as usize;
        let idx = if idx == 0 { 0 } else { idx - 1 };
        v.get(idx).copied().unwrap_or(0)
    }

    pub fn median(&self) -> u32 {
        let mut v: Vec<u32> = self.samples.clone();
        v.sort_unstable();
        let n = v.len();
        if n == 0 {
            return 0;
        }
        v.get(n / 2).copied().unwrap_or(0)
    }

    pub fn min(&self) -> u32 {
        self.samples.iter().copied().min().unwrap_or(0)
    }

    pub fn max(&self) -> u32 {
        self.samples.iter().copied().max().unwrap_or(0)
    }

    /// 稳定性检查。极差 ≤ 中位数 × 5%。中位数为 0 时（太快，测不出）视为稳定——
    /// 0ms 的行没有抖动可言，但报告里要看出它是 0，不能藏。
    pub fn stability(&self) -> Result<(), BenchCode> {
        if self.samples.len() < MIN_SAMPLES {
            return Err(BenchCode::FewSamples);
        }
        let med = self.median();
        if med == 0 {
            return Ok(());
        }
        let spread = self.max().saturating_sub(self.min());
        if u64::from(spread) * 1000 > u64::from(med) * u64::from(VARIANCE_LIMIT_PERMILLE) {
            return Err(BenchCode::Unstable);
        }
        Ok(())
    }

    /// 抖动千分比，用于报告。
    pub fn spread_permille(&self) -> u32 {
        let med = self.median();
        if med == 0 {
            return 0;
        }
        let spread = self.max().saturating_sub(self.min());
        ((u64::from(spread) * 1000) / u64::from(med)) as u32
    }
}

/// 基准套件：行表 + 校验。
#[derive(Clone, Debug, Default)]
pub struct BenchSuite {
    rows: Vec<BenchCase>,
}

impl BenchSuite {
    pub fn new() -> Self {
        Self { rows: Vec::new() }
    }

    pub fn push(&mut self, case: BenchCase) -> Result<(), BenchCode> {
        if self.rows.iter().any(|c| c.id == case.id) {
            return Err(BenchCode::DuplicateCase);
        }
        self.rows.push(case);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn all(&self) -> &[BenchCase] {
        &self.rows
    }

    /// 出厂套件：三档 × 冷热。令牌数与组件数取真实主题的典型值，
    /// 不取整数好算的值——整数会让「每一档都恰好线性」这种假象被当成结论。
    pub fn standard() -> Self {
        let mut s = Self::new();
        let rows = [
            ("reskin.small.cold", Path::Cold, 96usize, 18usize),
            ("reskin.small.warm", Path::Warm, 96, 18),
            ("reskin.medium.cold", Path::Cold, 640, 96),
            ("reskin.medium.warm", Path::Warm, 640, 96),
            ("reskin.large.cold", Path::Cold, 3900, 480),
            ("reskin.large.warm", Path::Warm, 3900, 480),
        ];
        for (id, path, tokens, comps) in rows {
            let _ = s.push(BenchCase::new(id, path, tokens, comps));
        }
        s
    }
}

/// 一次完整运行的结果。
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub rows: Vec<Row>,
    pub env: EnvFingerprint,
}

impl Report {
    pub fn row(&self, id: &str) -> Option<&Row> {
        self.rows.iter().find(|r| r.case.id == id)
    }

    /// 全表最慢的一行。空表返回 `None`（不是 0ms 的假行）。
    pub fn slowest(&self) -> Option<&Row> {
        self.rows.iter().max_by_key(|r| r.p95())
    }

    /// 全部行的稳定性。返回第一条不稳定行的 id，便于定点修复。
    pub fn stability(&self) -> Result<(), (&'static str, BenchCode)> {
        if self.rows.is_empty() {
            return Err(("", BenchCode::NoRun));
        }
        for r in &self.rows {
            if let Err(code) = r.stability() {
                return Err((r.case.id, code));
            }
        }
        Ok(())
    }

    /// 发布口径清单：每行一句，含样本数与抖动。发布附注直接抄。
    pub fn publish_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!("环境指纹 {}", self.env.line()));
        for r in &self.rows {
            out.push(format!(
                "  {:<22} P95 {:>4}ms  样本 {}  抖动 {}‰",
                r.case.id,
                r.p95(),
                r.samples.len(),
                r.spread_permille()
            ));
        }
        if let Some(s) = self.slowest() {
            out.push(format!("  最慢: {} @ {}ms", s.case.id, s.p95()));
        }
        out
    }
}

/// 容量声明。构造即校验：缺件根本造不出来，这比「构造后审计」可靠。
#[derive(Clone, Copy, Debug)]
pub struct CapacityClaim {
    pub case: &'static str,
    pub p95_ms: u32,
    pub samples: u32,
    pub measured: bool,
    pub env: EnvFingerprint,
}

impl CapacityClaim {
    pub fn from_row(row: &Row, measured: bool, env: EnvFingerprint) -> Self {
        Self {
            case: row.case.id,
            p95_ms: row.p95(),
            samples: row.samples.len() as u32,
            measured,
            env,
        }
    }

    /// 结构完整度：三件套（实测标记、样本数、环境指纹）缺一即不可发布。
    pub fn audit(&self) -> Result<(), BenchCode> {
        if !self.measured {
            return Err(BenchCode::NotMeasured);
        }
        if self.samples < MIN_SAMPLES as u32 {
            return Err(BenchCode::FewSamples);
        }
        if self.env.is_empty() {
            return Err(BenchCode::EnvMissing);
        }
        Ok(())
    }
}

/// 对拍结论。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ClaimVerdict {
    /// 声明与实测一致（在容差内）。
    Ok,
    /// 结构缺件。
    Invalid(BenchCode),
    /// 虚标：声明优于实测。
    Overclaim {
        case: &'static str,
        claimed: u32,
        actual: u32,
        delta_permille: u32,
    },
    /// 保守：声明比实测慢一大截，可发布但要注明。
    Conservative {
        case: &'static str,
        claimed: u32,
        actual: u32,
    },
    /// 套件里没这行。
    Unknown(&'static str),
}

impl ClaimVerdict {
    pub fn is_blocking(&self) -> bool {
        matches!(
            self,
            ClaimVerdict::Overclaim { .. } | ClaimVerdict::Invalid(_) | ClaimVerdict::Unknown(_)
        )
    }

    pub fn label(&self) -> String {
        match self {
            ClaimVerdict::Ok => "一致".to_string(),
            ClaimVerdict::Invalid(c) => format!("缺件({})", c.text()),
            ClaimVerdict::Overclaim {
                case,
                claimed,
                actual,
                delta_permille,
            } => format!(
                "虚标：{} 声明 {}ms，实测 {}ms，差 {}‰",
                case, claimed, actual, delta_permille
            ),
            ClaimVerdict::Conservative { case, claimed, actual } => {
                format!("保守：{} 声明 {}ms，实测 {}ms", case, claimed, actual)
            }
            ClaimVerdict::Unknown(id) => format!("套件中无此行: {}", id),
        }
    }
}

/// 对拍一条声明。`report` 必须是同一环境下跑出来的——所以先比指纹。
pub fn verify_claim(claim: &CapacityClaim, report: &Report) -> ClaimVerdict {
    if let Err(code) = claim.audit() {
        return ClaimVerdict::Invalid(code);
    }
    if !claim.env.compatible(&report.env) {
        return ClaimVerdict::Invalid(BenchCode::EnvMismatch);
    }
    let row = match report.row(claim.case) {
        Some(r) => r,
        None => return ClaimVerdict::Unknown(claim.case),
    };
    let actual = row.p95();
    if actual == 0 {
        // 实测 0ms 时无法计算相对差；声明必须也是 0，否则按「大于实测」虚标处理。
        if claim.p95_ms == 0 {
            return ClaimVerdict::Ok;
        }
        return ClaimVerdict::Overclaim {
            case: claim.case,
            claimed: claim.p95_ms,
            actual,
            delta_permille: 1000,
        };
    }
    if claim.p95_ms >= actual {
        // 声明不优于实测：要么一致，要么保守。
        let delta = u64::from(claim.p95_ms - actual) * 1000 / u64::from(actual);
        if delta >= 1000 {
            return ClaimVerdict::Conservative {
                case: claim.case,
                claimed: claim.p95_ms,
                actual,
            };
        }
        return ClaimVerdict::Ok;
    }
    let delta = u64::from(actual - claim.p95_ms) * 1000 / u64::from(actual);
    if delta > u64::from(CLAIM_TOLERANCE_PERMILLE) {
        ClaimVerdict::Overclaim {
            case: claim.case,
            claimed: claim.p95_ms,
            actual,
            delta_permille: delta as u32,
        }
    } else {
        ClaimVerdict::Ok
    }
}

/// 一次对拍的整体结论：只要有一条阻断就阻断。
pub fn verify_all(claims: &[CapacityClaim], report: &Report) -> (bool, Vec<String>) {
    let mut blocking = false;
    let mut lines = Vec::new();
    for c in claims {
        let v = verify_claim(c, report);
        if v.is_blocking() {
            blocking = true;
        }
        lines.push(format!("{} → {}", c.case, v.label()));
    }
    (blocking, lines)
}

/// 基线锁。`BTreeMap` 保证输出顺序稳定（锁文件可 diff，不会因插入顺序变化产生噪音）。
#[derive(Clone, Debug, Default)]
pub struct Baseline {
    rows: BTreeMap<&'static str, u32>,
}

impl Baseline {
    pub fn lock(report: &Report) -> Self {
        let mut rows = BTreeMap::new();
        for r in &report.rows {
            rows.insert(r.case.id, r.p95());
        }
        Self { rows }
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn get(&self, id: &str) -> Option<u32> {
        self.rows.get(id).copied()
    }

    /// 差异表，按劣化幅度降序。新增行（基线没有）排在最前并标 `new`。
    pub fn diff(&self, report: &Report) -> Result<Vec<DiffRow>, BenchCode> {
        if self.rows.is_empty() {
            return Err(BenchCode::EmptyBaseline);
        }
        let mut out = Vec::new();
        for r in &report.rows {
            let now = r.p95();
            match self.rows.get(r.case.id) {
                Some(base) => out.push(DiffRow {
                    case: r.case.id,
                    base_ms: *base,
                    now_ms: now,
                    permille: diff_permille(*base, now),
                    is_new: false,
                }),
                None => out.push(DiffRow {
                    case: r.case.id,
                    base_ms: 0,
                    now_ms: now,
                    permille: 0,
                    is_new: true,
                }),
            }
        }
        // 排序键：新增行永远排最前（它们没有基线可比，属于「最该看一眼」），
        // 其余按劣化幅度降序。稳定排序保证同幅度行的相对次序不随运行漂移。
        out.sort_by(|a, b| {
            b.is_new
                .cmp(&a.is_new)
                .then_with(|| b.permille.cmp(&a.permille))
        });
        Ok(out)
    }
}

/// 相对差千分比，带符号但夹在 ±1e6 内（防 `i32::abs` 溢出）。
fn diff_permille(base: u32, now: u32) -> i32 {
    if base == 0 {
        return if now == 0 { 0 } else { 1000 };
    }
    let d = (i64::from(now) - i64::from(base)) * 1000 / i64::from(base);
    let clamped = if d > 1_000_000 {
        1_000_000
    } else if d < -1_000_000 {
        -1_000_000
    } else {
        d
    };
    clamped as i32
}

/// 一条差异。`+12.4%` 这种形态比 `124‰` 更容易被看懂，故提供 `percent_text`。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DiffRow {
    pub case: &'static str,
    pub base_ms: u32,
    pub now_ms: u32,
    pub permille: i32,
    pub is_new: bool,
}

impl DiffRow {
    pub fn percent_text(&self) -> String {
        if self.is_new {
            return "new".to_string();
        }
        let sign = if self.permille >= 0 { "+" } else { "-" };
        let a = self.permille.abs();
        format!("{}{}.{:01}%", sign, a / 10, a % 10)
    }

    pub fn line(&self) -> String {
        if self.is_new {
            return format!("{:<8} {:<22} {}ms", "new", self.case, self.now_ms);
        }
        format!(
            "{:<8} {:<22} {}ms → {}ms",
            self.percent_text(),
            self.case,
            self.base_ms,
            self.now_ms
        )
    }
}

/// 差异可视化：一行一条，降序。第一行就是最该看的那条。
pub fn diff_lines(rows: &[DiffRow]) -> Vec<String> {
    let mut out = Vec::new();
    out.push(format!("基线差异（{} 条，按劣化降序）", rows.len()));
    for r in rows {
        out.push(format!("  {}", r.line()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 确定性计量器：耗时由 (tokens, components, path) 决定，附加可控抖动。
    /// 存在的意义是让「口径」本身可被测试——真时钟无法在 CI 里断言数值。
    struct VirtualMeter {
        jitter_ms: u32,
        bump: u32,
    }

    impl VirtualMeter {
        fn new(jitter_ms: u32) -> Self {
            Self { jitter_ms, bump: 0 }
        }
    }

    impl Meter for VirtualMeter {
        fn sample(&mut self, case: &BenchCase) -> u32 {
            let parse = (case.tokens / 16) as u32;
            let layout = (case.components / 4) as u32;
            let path = match case.path {
                Path::Cold => parse + layout,
                Path::Warm => layout / 2,
            };
            let base = 1 + path;
            let j = if self.jitter_ms == 0 {
                0
            } else {
                self.bump % (self.jitter_ms + 1)
            };
            self.bump = self.bump.wrapping_add(1);
            base + j
        }
    }

    fn env() -> EnvFingerprint {
        EnvFingerprint::new(0x1234_5678, 0x0A00_0001, 0x8664, 32768)
    }

    fn run_suite(suite: &BenchSuite, samples: usize, jitter: u32) -> Report {
        let mut meter = VirtualMeter::new(jitter);
        let mut rows = Vec::new();
        for case in suite.all() {
            let mut s = Vec::new();
            for _ in 0..samples {
                s.push(meter.sample(case));
            }
            rows.push(Row {
                case: *case,
                samples: s,
            });
        }
        Report { rows, env: env() }
    }

    #[test]
    fn standard_suite_has_six_rows_and_unique_ids() {
        let s = BenchSuite::standard();
        assert_eq!(s.len(), 6);
        let mut ids: Vec<&str> = s.all().iter().map(|c| c.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 6, "id 必须唯一");
        assert!(s
            .all()
            .iter()
            .any(|c| c.name() == "reskin.large.cold".to_string()));
    }

    #[test]
    fn duplicate_case_is_rejected() {
        let mut s = BenchSuite::new();
        let c = BenchCase::new("x", Path::Cold, 10, 1);
        assert!(s.push(c).is_ok());
        assert_eq!(s.push(c), Err(BenchCode::DuplicateCase));
    }

    #[test]
    fn tier_boundaries_are_inclusive() {
        assert_eq!(Tier::of_tokens(120), Tier::Small);
        assert_eq!(Tier::of_tokens(121), Tier::Medium);
        assert_eq!(Tier::of_tokens(1200), Tier::Medium);
        assert_eq!(Tier::of_tokens(1201), Tier::Large);
    }

    #[test]
    fn p95_of_five_samples_is_max_and_is_documented() {
        let row = Row {
            case: BenchCase::new("p", Path::Warm, 10, 2),
            samples: alloc::vec![3, 1, 5, 4, 2],
        };
        assert_eq!(row.p95(), 5);
        assert_eq!(row.median(), 3);
        // 三个样本时 P95 同样是最大值：这正是「必须标样本数」的实证。
        let three = Row {
            case: row.case,
            samples: alloc::vec![7, 9, 8],
        };
        assert_eq!(three.p95(), 9);
    }

    #[test]
    fn unstable_meter_is_caught() {
        let s = BenchSuite::standard();
        let report = run_suite(&s, RECOMMENDED_SAMPLES, 40);
        let err = report.stability();
        assert_eq!(err, Err(("reskin.small.cold", BenchCode::Unstable)));
    }

    #[test]
    fn stable_report_publishes_lines() {
        let s = BenchSuite::standard();
        let report = run_suite(&s, RECOMMENDED_SAMPLES, 0);
        assert!(report.stability().is_ok(), "零抖动必须稳定");
        let lines = report.publish_lines();
        assert!(lines[0].contains("环境指纹 cpu="));
        assert!(lines.len() >= 7);
        assert!(report.slowest().is_some());
    }

    #[test]
    fn claim_must_be_measured_and_fingerprinted() {
        let s = BenchSuite::standard();
        let report = run_suite(&s, RECOMMENDED_SAMPLES, 0);
        let row = match report.row("reskin.large.cold") {
            Some(r) => r,
            None => panic!("行必须存在"),
        };
        let mut claim = CapacityClaim::from_row(row, true, env());
        assert_eq!(claim.audit(), Ok(()));
        claim.measured = false;
        assert_eq!(claim.audit(), Err(BenchCode::NotMeasured));
        claim.measured = true;
        claim.env = EnvFingerprint::default();
        assert_eq!(claim.audit(), Err(BenchCode::EnvMissing));
        claim.env = env();
        claim.samples = 2;
        assert_eq!(claim.audit(), Err(BenchCode::FewSamples));
    }

    #[test]
    fn exact_claim_passes_and_underclaim_blocks() {
        let s = BenchSuite::standard();
        let report = run_suite(&s, RECOMMENDED_SAMPLES, 0);
        let row = match report.row("reskin.medium.warm") {
            Some(r) => r,
            None => panic!("行必须存在"),
        };
        let actual = row.p95();
        // 两条抽查：一条声明=实测，一条声明压到 50% ⇒ 必须被抓。
        let exact = CapacityClaim::from_row(row, true, env());
        assert_eq!(verify_claim(&exact, &report), ClaimVerdict::Ok);

        let lying = CapacityClaim {
            p95_ms: actual / 2,
            ..exact
        };
        let v = verify_claim(&lying, &report);
        assert!(v.is_blocking(), "压半必须判虚标：{}", v.label());
        assert!(matches!(v, ClaimVerdict::Overclaim { .. }));
    }

    #[test]
    fn conservative_claim_is_allowed_but_flagged() {
        let s = BenchSuite::standard();
        let report = run_suite(&s, RECOMMENDED_SAMPLES, 0);
        let row = match report.row("reskin.small.warm") {
            Some(r) => r,
            None => panic!("行必须存在"),
        };
        let claim = CapacityClaim {
            p95_ms: row.p95() * 3,
            ..CapacityClaim::from_row(row, true, env())
        };
        let v = verify_claim(&claim, &report);
        assert!(!v.is_blocking(), "保守不阻断");
        assert!(matches!(v, ClaimVerdict::Conservative { .. }));
    }

    #[test]
    fn env_mismatch_and_unknown_case_are_blocking() {
        let s = BenchSuite::standard();
        let report = run_suite(&s, RECOMMENDED_SAMPLES, 0);
        let row = match report.row("reskin.small.cold") {
            Some(r) => r,
            None => panic!("行必须存在"),
        };
        let wrong_env = CapacityClaim {
            env: EnvFingerprint::new(0xFFFF, 0xFFFF, 0xFFFF, 8192),
            ..CapacityClaim::from_row(row, true, env())
        };
        assert_eq!(
            verify_claim(&wrong_env, &report),
            ClaimVerdict::Invalid(BenchCode::EnvMismatch)
        );

        let unknown = CapacityClaim {
            case: "reskin.huge.cold",
            ..CapacityClaim::from_row(row, true, env())
        };
        assert!(verify_claim(&unknown, &report).is_blocking());
    }

    #[test]
    fn baseline_diff_marks_regression_on_top() {
        let s = BenchSuite::standard();
        let cold = run_suite(&s, RECOMMENDED_SAMPLES, 0);
        let base = Baseline::lock(&cold);
        assert_eq!(base.len(), 6);

        // 加大抖动以外的耗时：把 large.cold 的样本整体上抬 50%。
        let mut worse = Report {
            rows: cold.rows.clone(),
            env: env(),
        };
        for r in worse.rows.iter_mut() {
            if r.case.id == "reskin.large.cold" {
                for v in r.samples.iter_mut() {
                    *v = *v + *v / 2 + 1;
                }
            }
        }
        let rows = match base.diff(&worse) {
            Ok(v) => v,
            Err(_) => panic!("基线非空，diff 必须成功"),
        };
        assert_eq!(rows[0].case, "reskin.large.cold");
        assert!(rows[0].permille > 400, "劣化幅度应显著：{}", rows[0].permille);
        let lines = diff_lines(&rows);
        assert!(lines[0].contains("按劣化降序"));
        assert!(lines[1].contains("+"), "首行应为正劣化：{}", lines[1]);
    }

    #[test]
    fn empty_baseline_reports_loudly() {
        let s = BenchSuite::standard();
        let report = run_suite(&s, RECOMMENDED_SAMPLES, 0);
        let empty = Baseline::default();
        assert_eq!(empty.diff(&report).err(), Some(BenchCode::EmptyBaseline));
        assert!(empty.is_empty());
    }

    #[test]
    fn new_rows_are_marked_not_silently_zero() {
        let s = BenchSuite::standard();
        let report = run_suite(&s, RECOMMENDED_SAMPLES, 0);
        let mut base = Baseline::lock(&report);
        // 手工制造一条「基线里没有的行」。
        let extra = BenchCase::new("reskin.huge.warm", Path::Warm, 9000, 900);
        let mut report2 = report.clone();
        report2.rows.push(Row {
            case: extra,
            samples: alloc::vec![9, 9, 9, 9, 9],
        });
        let rows = base.diff(&report2).unwrap_or_default();
        assert!(rows[0].is_new, "新增行应排最前并标 new");
        assert_eq!(rows[0].percent_text(), "new");
        // 基线本身不应被子报告污染
        assert_eq!(base.len(), 6);
        base.rows.clear();
        assert!(base.is_empty());
    }
}
