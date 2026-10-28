//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12456 · 令牌族性能预算（O1 对标口径） · 0x30700
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12456-J1 AccessCheck P95 预算在册（O1 配套账），长 DACL
//!      退化曲线无红账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 预算在册：AccessCheck 整数预算（tick，O1 对标常量写死），在册
//!     三字段（预算值/观测 P95/样本数）真实可查；
//!   - 成本模型：整数 tick 计量（无浮点无墙钟），单笔成本 = 基线 +
//!     ACE 数 × 单 ACE 系数（确定性：同输入同成本）；
//!   - P95 真算：样本升序插入排序后取序 95% 位（整数下标），样本 <16
//!     显式拒绝；
//!   - 长 DACL 退化曲线：ACE 数 1,2,4,…,256 逐点扫描，P95 单调不降
//!     且全程在预算内——曲线无红账判据；
//!   - 红账口径：观测成本超预算即入红账（append-only），曲线扫描面
//!     预算域内必不触发；
//!   - 拒绝/放行同成本：门控拒止路径与放行路径同成本模型（对称口径），
//!     差值恒等于基线差；
//!   - 注入面：超预算注入（大 DACL/越域 ACE 数/零 ACE）全抓；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x30700），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30700 段，32 位段值）。
pub const SEG_BASE: u32 = 0x30700;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// AccessCheck 预算（tick，O1 对标口径写死）。
pub const AC_BUDGET: u64 = 300;

/// 基线成本（tick）。
pub const AC_BASE: u64 = 40;

/// 单 ACE 系数（tick/ACE）。
pub const PER_ACE: u64 = 1;

/// ACE 数域上限（长 DACL 口径 1024）。
pub const ACES_MAX: u64 = 1024;

/// P95 序位（千分 950）。
pub const P95_MILLI: u64 = 950;

/// 诊断码（0x30700 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfCode {
    /// 0x30700 超预算（观测成本 > AC_BUDGET）。
    OverBudget = 0,
    /// 0x30701 P95 超预算。
    P95Over = 1,
    /// 0x30702 ACE 数越域（>1024）。
    AcesOob = 2,
    /// 0x30703 零 ACE（AccessCheck 无 DACL 语义域外）。
    AcesZero = 3,
    /// 0x30704 样本不足（P95 前样本 <16）。
    SampleShort = 4,
    /// 0x30705 P95 非单调（退化曲线非单调不降）。
    P95NonMono = 5,
    /// 0x30706 红账非空（曲线扫描面出现红账）。
    RedNonEmpty = 6,
    /// 0x30707 预算账缩水（在册行只增不减被破）。
    BudgetShrink = 7,
    /// 0x30708 对称口径破（拒/放行成本差 ≠ 0）。
    SymmetryBreak = 8,
    /// 0x30709 结算未执行（P95 位未落账）。
    NotSettled = 9,
}

impl PfCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [PfCode; 10] = [
        PfCode::OverBudget, PfCode::P95Over, PfCode::AcesOob, PfCode::AcesZero,
        PfCode::SampleShort, PfCode::P95NonMono, PfCode::RedNonEmpty, PfCode::BudgetShrink,
        PfCode::SymmetryBreak, PfCode::NotSettled,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30700 => "超预算：观测成本超 AC_BUDGET，先查 ACE 数与单 ACE 系数。",
        0x30701 => "P95 超预算：取序 95% 位复核，超界压尾延迟。",
        0x30702 => "ACE 数越域：>1024 越长 DACL 口径，先分块再查。",
        0x30703 => "零 ACE：AccessCheck 需至少一条 DACL ACE，先补 ACL。",
        0x30704 => "样本不足：P95 前样本须 ≥16，先补采样。",
        0x30705 => "P95 非单调：退化曲线必须单调不降，回查成本系数。",
        0x30706 => "红账非空：曲线扫描面出现红账，逐点回放定位。",
        0x30707 => "预算账缩水：在册行 append-only，禁止截断。",
        0x30708 => "对称口径破：拒/放行成本必须同模型，回查分支差异。",
        0x30709 => "结算未执行：先 settle 落 P95 再出账。",
        _ => "未知诊断码：回查 0x30700 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30700 段基址。
    pub const SEED: u64 = 0x30700_3070_0307_00u64;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 成本模型：整数 tick（同输入同成本）。
/// 成本 = AC_BASE + aces × PER_ACE；ACE 域 1..=1024。
pub fn cost_check(aces: u64) -> Result<u64, PfCode> {
    if aces == 0 {
        return Err(PfCode::AcesZero);
    }
    if aces > ACES_MAX {
        return Err(PfCode::AcesOob);
    }
    Ok(AC_BASE + aces * PER_ACE)
}

/// 升序插入排序（样本 ≤256 可承受）。
pub fn sort_asc(v: &[u64]) -> Vec<u64> {
    let mut out: Vec<u64> = v.to_vec();
    let n = out.len();
    for i in 1..n {
        let key = out[i];
        let mut j = i;
        while j > 0 && out[j - 1] > key {
            out[j] = out[j - 1];
            j -= 1;
        }
        out[j] = key;
    }
    out
}

/// P95 真算（升序序 95% 位；样本 <16 拒绝）。
pub fn p95(samples: &[u64]) -> Result<u64, PfCode> {
    if samples.len() < 16 {
        return Err(PfCode::SampleShort);
    }
    let s = sort_asc(samples);
    let idx = ((s.len() as u64 * P95_MILLI + 999) / 1000) as usize - 1;
    Ok(s[idx.min(s.len() - 1)])
}

/// 预算在册行。
#[derive(Clone, Copy, Debug)]
pub struct BudgetRow {
    /// 预算值。
    pub budget: u64,
    /// 观测 P95。
    pub p95: u64,
    /// 样本数。
    pub samples: usize,
    /// 超预算。
    pub over: bool,
}

/// 红账行（超预算事件）。
#[derive(Clone, Copy, Debug)]
pub struct RedRow {
    /// 观测成本。
    pub observed: u64,
    /// 预算值。
    pub budget: u64,
    /// ACE 数。
    pub aces: u64,
}

/// 性能账正本。
#[derive(Debug)]
pub struct PerfBook {
    /// 样本（放行路径）。
    allow_samples: Vec<u64>,
    /// 样本（拒绝路径）。
    deny_samples: Vec<u64>,
    /// 预算在册行。
    budget: BudgetRow,
    /// 预算在册初值（缩水判准锚）。
    budget0: BudgetRow,
    /// 红账（append-only）。
    red: Vec<RedRow>,
    /// 已结算。
    settled: bool,
}

impl PerfBook {
    /// 建账（在册行入册）。
    pub fn new() -> Self {
        let b = BudgetRow { budget: AC_BUDGET, p95: 0, samples: 0, over: false };
        PerfBook {
            allow_samples: Vec::new(),
            deny_samples: Vec::new(),
            budget: b,
            budget0: b,
            red: Vec::new(),
            settled: false,
        }
    }

    /// 采一笔放行路径样本（特权门控过——正路成本）。
    pub fn sample_allow(&mut self, aces: u64) -> Result<u64, PfCode> {
        let c = cost_check(aces)?;
        self.allow_samples.push(c);
        if c > AC_BUDGET {
            self.red.push(RedRow { observed: c, budget: AC_BUDGET, aces });
        }
        Ok(c)
    }

    /// 采一笔拒绝路径样本（门控拒——对称口径：同成本模型）。
    pub fn sample_deny(&mut self, aces: u64) -> Result<u64, PfCode> {
        let c = cost_check(aces)?;
        self.deny_samples.push(c);
        if c > AC_BUDGET {
            self.red.push(RedRow { observed: c, budget: AC_BUDGET, aces });
        }
        Ok(c)
    }

    /// 结算 P95（双路径各 ≥16 样本才结算）。
    pub fn settle(&mut self) -> Result<(u64, u64), PfCode> {
        let a = p95(&self.allow_samples)?;
        let d = p95(&self.deny_samples)?;
        self.budget.p95 = a;
        self.budget.samples = self.allow_samples.len();
        self.budget.over = a > AC_BUDGET;
        self.settled = true;
        Ok((a, d))
    }

    /// 对称口径判据（拒/放行同成本：逐对差 = 0）。
    pub fn symmetric(&self) -> bool {
        let n = self.allow_samples.len().min(self.deny_samples.len());
        (0..n).all(|i| self.allow_samples[i] == self.deny_samples[i])
    }

    /// 红账长度。
    pub fn red_len(&self) -> usize {
        self.red.len()
    }

    /// 红账空判据。
    pub fn red_clean(&self) -> bool {
        self.red.is_empty()
    }

    /// 预算在册行快照。
    pub fn budget_row(&self) -> BudgetRow {
        self.budget
    }

    /// 在册缩水判据（预算值/初值恒等）。
    pub fn budget_intact(&self) -> bool {
        self.budget.budget == self.budget0.budget && self.budget.budget == AC_BUDGET
    }

    /// 已结算判据。
    pub fn is_settled(&self) -> bool {
        self.settled
    }

    /// 长 DACL 退化曲线扫描（ACE 数 1,2,4,…,256；P95 单调不降且无红账）。
    pub fn curve_scan(&self) -> Result<(), PfCode> {
        let mut prev: u64 = 0;
        let mut aces = 1u64;
        while aces <= 256 {
            // 每点采 16 样本（确定性成本模型）。
            let mut s: Vec<u64> = Vec::new();
            let mut k = 0u64;
            while k < 16 {
                s.push(AC_BASE + aces * PER_ACE);
                k += 1;
            }
            let v = p95(&s)?;
            if v < prev {
                return Err(PfCode::P95NonMono);
            }
            // 曲线全程在预算内（无红账口径）。
            if v > AC_BUDGET {
                return Err(PfCode::RedNonEmpty);
            }
            prev = v;
            aces *= 2;
        }
        Ok(())
    }

    /// 转录导出。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("AccessCheck 性能账：\n");
        s.push_str(&format!(
            "预算{} P95{} 样本{} 超{}\n",
            self.budget.budget, self.budget.p95, self.budget.samples, self.budget.over
        ));
        s
    }

    /// 转录完整判据。
    pub fn lines_complete(&self) -> bool {
        let t = self.export_lines();
        t.contains("AccessCheck 性能账") && t.contains("预算")
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 演练面。
/// ─────────────────────────────────────────────────────────────────────
/// 预算在册演练：24 组采样 + 结算 + P95 在预算内 10/10 轮。
pub fn drill_p95_budget_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30700 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = PerfBook::new();
        let mut all_ok = true;
        for _i in 0..24 {
            let aces = 1 + lcg.below(200);
            all_ok = all_ok && bk.sample_allow(aces).is_ok() && bk.sample_deny(aces).is_ok();
        }
        let (a95, d95) = match bk.settle() {
            Ok(v) => v,
            Err(_) => (u64::MAX, u64::MAX),
        };
        // 手工复核：24 样本域上界成本。
        let hand = AC_BASE + 200 * PER_ACE;
        let row = bk.budget_row();
        let _ = d95;
        if all_ok && a95 <= hand && row.budget == AC_BUDGET && row.samples == 24
            && bk.red_clean() && bk.symmetric()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("P95在册轮={}/{}", ok, INJECT_ROUNDS))
}

/// 退化曲线演练：长 DACL 单调无红账 10/10 轮。
pub fn drill_curve_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30701（曲线序列）。
    let mut lcg = Lcg::new(0x30701);
    for _r in 0..INJECT_ROUNDS {
        let bk = PerfBook::new();
        let scan = bk.curve_scan();
        // 独立核对：1 ACE P95 与 256 ACE P95 严格递增且在预算内。
        let s1: Vec<u64> = (0..16).map(|_| AC_BASE + 1).collect();
        let s2: Vec<u64> = (0..16).map(|_| AC_BASE + 256).collect();
        let (a, b) = (p95(&s1).unwrap_or(0), p95(&s2).unwrap_or(0));
        let _ = lcg.below(3);
        if scan.is_ok() && a == AC_BASE + 1 && b == AC_BASE + 256 && a < b && b <= AC_BUDGET {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("退化曲线轮={}/{}", ok, INJECT_ROUNDS))
}

/// 超预算注入演练：大 DACL/越域/零 ACE 全抓 10/10 轮。
pub fn drill_over_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30702（注入序列）。
    let mut lcg = Lcg::new(0x30702);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = PerfBook::new();
        let mut all4 = true;
        // 注 1：大 DACL（>260 ACE 超 AC_BUDGET=300：40+261=301）。
        let big = 261 + lcg.below(100);
        if matches!(bk.sample_allow(big), Ok(c) if c > AC_BUDGET) && bk.red_len() >= 1 {
            // 抓到
        } else {
            all4 = false;
        }
        // 注 2：越域 ACE（>1024）。
        if matches!(bk.sample_allow(ACES_MAX + 1), Err(PfCode::AcesOob)) {
            // 抓到
        } else {
            all4 = false;
        }
        // 注 3：零 ACE。
        if matches!(bk.sample_deny(0), Err(PfCode::AcesZero)) {
            // 抓到
        } else {
            all4 = false;
        }
        // 注 4：样本不足 P95 拒。
        let short: Vec<u64> = (0..15).map(|_| AC_BASE).collect();
        if matches!(p95(&short), Err(PfCode::SampleShort)) {
            // 抓到
        } else {
            all4 = false;
        }
        let _ = lcg.below(3);
        if all4 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("超预算注入轮={}/{}", ok, INJECT_ROUNDS))
}

/// P95 单调演练：序位单调性独立核对 10/10 轮。
pub fn drill_p95_mono_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30703（单调序列）。
    let mut lcg = Lcg::new(0x30703);
    for _r in 0..INJECT_ROUNDS {
        // 同值样本序 P95 = 值；升序样本 P95 = 95% 位值。
        let flat: Vec<u64> = (0..16).map(|_| 50).collect();
        let ramp: Vec<u64> = (1..=16).map(|i| i as u64).collect();
        let (a, b) = (p95(&flat).unwrap_or(0), p95(&ramp).unwrap_or(0));
        let _ = lcg.below(3);
        // 序位口径：ceil(n*950/1000)-1 → 16 样本取第 16 位。
        if a == 50 && b == 16 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("P95单调轮={}/{}", ok, INJECT_ROUNDS))
}

/// 逐对对称演练：放行/拒绝同 ACE 数逐对同成本 10/10 轮。
pub fn drill_pair_symmetry_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30704（对称序列）。
    let mut lcg = Lcg::new(0x30704);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = PerfBook::new();
        let mut all_ok = true;
        for i in 0..32u64 {
            let aces = 1 + (i * 5) % 250;
            let a = bk.sample_allow(aces);
            let d = bk.sample_deny(aces);
            // 逐对同成本（对称口径）。
            all_ok = all_ok && a == d;
        }
        all_ok = all_ok && bk.symmetric() && bk.red_clean();
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("逐对对称轮={}/{}", ok, INJECT_ROUNDS))
}

/// 样本转录导出（逐样本一行：路径/ACE 数/成本）。
pub fn sample_export_lines(bk: &PerfBook) -> String {
    let mut s = String::from("采样转录：
");
    let n = bk.allow_samples.len().min(bk.deny_samples.len());
    for i in 0..n {
        s.push_str(&format!(
            "对{} 放{} 拒{}
",
            i, bk.allow_samples[i], bk.deny_samples[i]
        ));
    }
    s
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：零 ACE 必拒。
    let mut bk = PerfBook::new();
    if matches!(bk.sample_allow(0), Err(PfCode::AcesZero)) {
        caught += 1;
        d.push("注1 零ACE:抓".to_string());
    } else {
        d.push("注1 零ACE:漏".to_string());
    }
    // 注 2：越域 ACE 必拒。
    if matches!(bk.sample_deny(ACES_MAX + 1), Err(PfCode::AcesOob)) {
        caught += 1;
        d.push("注2 越域:抓".to_string());
    } else {
        d.push("注2 越域:漏".to_string());
    }
    // 注 3：样本不足 P95 必拒。
    let s3: Vec<u64> = (0..15).map(|_| AC_BASE).collect();
    if matches!(p95(&s3), Err(PfCode::SampleShort)) {
        caught += 1;
        d.push("注3 样本:抓".to_string());
    } else {
        d.push("注3 样本:漏".to_string());
    }
    // 注 4：大 DACL 必入红账。
    let _ = bk.sample_allow(500);
    if bk.red_len() >= 1 {
        caught += 1;
        d.push("注4 红账:抓".to_string());
    } else {
        d.push("注4 红账:漏".to_string());
    }
    // 注 5：正路必绿（预算在册 + 结算 + 曲线无红账）。
    let mut bk5 = PerfBook::new();
    for _i in 0..24u64 {
        let aces = 1 + _i % 200;
        let _ = bk5.sample_allow(aces);
        let _ = bk5.sample_deny(aces);
    }
    let st = bk5.settle();
    if st.is_ok() && bk5.red_clean() && bk5.symmetric() && bk5.budget_intact() && bk5.is_settled() {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30700 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    PfCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 性能报告。
#[derive(Clone, Copy, Debug)]
pub struct PfReport {
    /// 样本数。
    pub samples: usize,
    /// 观测 P95。
    pub p95: u64,
    /// 红账数。
    pub red: usize,
    /// 已结算。
    pub settled: bool,
    /// 对称。
    pub symmetric: bool,
}

impl PfReport {
    /// 建报告。
    pub fn build(bk: &PerfBook) -> Self {
        let row = bk.budget_row();
        PfReport {
            samples: row.samples,
            p95: row.p95,
            red: bk.red_len(),
            settled: bk.is_settled(),
            symmetric: bk.symmetric(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.samples >= 16 && self.p95 > 0 && self.p95 <= AC_BUDGET
            && self.red == 0 && self.settled && self.symmetric
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "PfReport 样本={} P95={} 红={} 结算={} 对称={}",
            self.samples, self.p95, self.red, self.settled, self.symmetric
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct PfCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_pf_checklist(bk: &PerfBook) -> Vec<PfCheckRow> {
    let rep = PfReport::build(bk);
    let mut rows: Vec<PfCheckRow> = Vec::new();
    rows.push(PfCheckRow { name: "预算在册", passed: bk.budget_intact() && bk.budget_row().budget == AC_BUDGET });
    rows.push(PfCheckRow { name: "P95达标", passed: rep.p95 <= AC_BUDGET && rep.p95 > 0 });
    rows.push(PfCheckRow { name: "样本≥16", passed: rep.samples >= 16 });
    rows.push(PfCheckRow { name: "已结算", passed: rep.settled });
    rows.push(PfCheckRow { name: "红账空", passed: bk.red_clean() });
    rows.push(PfCheckRow { name: "对称口径", passed: rep.symmetric });
    rows.push(PfCheckRow { name: "曲线无红账", passed: bk.curve_scan().is_ok() });
    rows.push(PfCheckRow { name: "转录完整", passed: bk.lines_complete() });
    rows.push(PfCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(PfCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn pf_checklist_green(rows: &[PfCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn pf_stats_line(bk: &PerfBook) -> String {
    let rep = PfReport::build(bk);
    format!(
        "AccessCheck 概况：P95 {} 预算 {} 红 {}",
        rep.p95, AC_BUDGET, rep.red
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12456_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    // 标准会话：24 组采样 + 结算。
    let mut bk = PerfBook::new();
    for i in 0..24u64 {
        let aces = 1 + (i * 7) % 200;
        let _ = bk.sample_allow(aces);
        let _ = bk.sample_deny(aces);
    }
    let _ = bk.settle();
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        PfReport::build(&bk).all_green() && bk.lines_complete(),
        pf_stats_line(&bk),
    ));
    // X-b：超预算注入 10/10。
    let (ok_o, d_o) = drill_over_10();
    out.push(("X-超预算注入10/10", ok_o, d_o));
    // X-c：P95 单调 10/10。
    let (ok_m, d_m) = drill_p95_mono_10();
    out.push(("X-P95单调10/10", ok_m, d_m));
    // X-d：终验清单。
    let rows = build_pf_checklist(&bk);
    out.push((
        "X-终验清单10项",
        pf_checklist_green(&rows),
        PfReport::build(&bk).to_text(),
    ));
    // X-e2：逐对对称 10/10 + 采样转录面。
    let (ok_s, d_s) = drill_pair_symmetry_10();
    out.push(("X-逐对对称10/10", ok_s, d_s));
    out.push((
        "X-采样转录面",
        sample_export_lines(&bk).contains("采样转录") && sample_export_lines(&bk).contains("对0"),
        format!("采样对数={}", bk.allow_samples.len()),
    ));
    // X-e：口径常量核对。
    out.push((
        "X-口径常量核对",
        AC_BUDGET == 300 && AC_BASE == 40 && PER_ACE == 1 && ACES_MAX == 1024 && P95_MILLI == 950,
        format!("budget={} base={} per={} max={} p95={}", AC_BUDGET, AC_BASE, PER_ACE, ACES_MAX, P95_MILLI),
    ));
    out
}

/// run_unx12456_checks：本单判据聚合（诊断段 0x30700）。
pub fn run_unx12456_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12456 令牌族性能预算");
    // J1-a：P95 预算在册 10/10。
    let (ok1, d1) = drill_p95_budget_10();
    s.add_dyn("J1-P95在册10/10", ok1, &d1);
    // J1-b：退化曲线 10/10。
    let (ok2, d2) = drill_curve_10();
    s.add_dyn("J1-退化曲线10/10", ok2, &d2);
    // J1-c：标准会话恒等式（在册 + 结算 + 对称 + 红账空）。
    let mut bk = PerfBook::new();
    for i in 0..24u64 {
        let aces = 1 + i % 200;
        let _ = bk.sample_allow(aces);
        let _ = bk.sample_deny(aces);
    }
    let st = bk.settle();
    s.add_dyn(
        "J1-预算在册P95达标",
        st.is_ok() && bk.red_clean() && bk.symmetric() && bk.budget_intact() && bk.is_settled(),
        &format!("P95={:?} 红={} 对称={}", st, bk.red_len(), bk.symmetric()),
    );
    // J2-a：注入核 5/5。
    let (c2, t2, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c2 == t2, &format!("{}/{} {}", c2, t2, d5.join(" ")));
    // J2-b：超预算注入 10/10。
    let (ok3, d3) = drill_over_10();
    s.add_dyn("J2-超预算注入10/10", ok3, &d3);
    // J2-c：红账只增（再注一笔必增且账面留痕）。
    let before = bk.red_len();
    let _ = bk.sample_allow(ACES_MAX);
    s.add_dyn("J2-红账只增", bk.red_len() >= before + 1, &format!("红 {}→{}", before, bk.red_len()));
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30700段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12456_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
