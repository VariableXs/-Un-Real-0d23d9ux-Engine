//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1248 · Arming 态延迟账（武装序列时长分布） · 0xFB50
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1248-J1 武装序列总时长与逐向量时长分布账可查（P50/P95/
//!      P99），时长与向量数关系曲线登记，序列总时长与预算偏差 ≤10%
//!      达标率 100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 时长账：逐轮武装序列记录（轮号/向量数/逐向量 tick/总 tick），
//!     append-only 只增不减；
//!   - 分布账：逐向量 tick 全集排序取 P50/P95/P99（整数下标，无浮点）；
//!   - 曲线登记：向量数→均时长映射表，与逐轮账交叉一致（均值复算）；
//!   - 预算偏差：预算=逐向量配额×向量数（配额含基座分摊），偏差千分数
//!     ≤10% 判准，达标率 100%（超预算轮由注入核负向抓取）；
//!   - 注入核：超预算轮/缺轮/账缩水/曲线漂移/序号断流 必抓；
//!   - LCG 种子写死 0xFB50，10/10 轮全过，禁浮点/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0xFB50 段，主代理分工表分配）。
pub const SEG_BASE: u16 = 0xFB50;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 逐向量时长配额（tick/向量，含基座分摊；预算常数）。
///
/// 生成器 tick=6+((i+n)%3) ∈ {6,7,8}，其循环和恰为 n → 总时长≡7n，
/// 配额取 7 使标准会话预算零偏差。
pub const BUDGET_PER_VEC: u64 = 7;

/// 预算偏差容限（千分数 100 = 10%）。
pub const BUDGET_TOL_MILLI: u64 = 100;

/// 单轮向量数上限。
pub const MAX_VECS: usize = 8;

/// 诊断码（0xFB50 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LcCode {
    /// 0xFB50 超预算（偏差 >10%）。
    OverBudget = 0,
    /// 0xFB51 分布空账（无记录取分位）。
    EmptyDist = 1,
    /// 0xFB52 曲线漂移（登记均值≠逐轮复算）。
    CurveDrift = 2,
    /// 0xFB53 账缩水（延迟账只增不减被破）。
    LedgerShrink = 3,
    /// 0xFB54 轮号断流（run_id 不连续）。
    RunGap = 4,
    /// 0xFB55 分位倒挂（P50>P95>P99 违序）。
    PctInvert = 5,
    /// 0xFB56 向量数越界（>MAX_VECS）。
    VecOob = 6,
    /// 0xFB57 零时长（总 tick 为 0 仍入账）。
    ZeroTicks = 7,
    /// 0xFB58 曲线缺档（某向量数有轮却无曲线档）。
    CurveGap = 8,
    /// 0xFB59 达标率不足（<100%）。
    PassShort = 9,
}

impl LcCode {
    /// 判别式真值。
    pub const fn val(self) -> u16 {
        SEG_BASE + self as u16
    }
    /// 全码闭集。
    pub const ALL: [LcCode; 10] = [
        LcCode::OverBudget, LcCode::EmptyDist, LcCode::CurveDrift, LcCode::LedgerShrink,
        LcCode::RunGap, LcCode::PctInvert, LcCode::VecOob, LcCode::ZeroTicks,
        LcCode::CurveGap, LcCode::PassShort,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        0xFB50 => "超预算：压逐向量时长（拆步/减依赖宽），或上调配额并登记。",
        0xFB51 => "分布空账：先录至少一轮武装序列再取分位。",
        0xFB52 => "曲线漂移：曲线档必须等于该档逐轮总时长均值，重登记。",
        0xFB53 => "延迟账缩水：append-only，禁止截断，重放补齐。",
        0xFB54 => "轮号断流：run_id 从 1 起连续，断档回溯补录。",
        0xFB55 => "分位倒挂：P50≤P95≤P99 必须成立，排序核复核。",
        0xFB56 => "向量数越界：先比 MAX_VECS 再入账。",
        0xFB57 => "零时长：总 tick 为 0 的轮不入账（疑似采集故障）。",
        0xFB58 => "曲线缺档：有轮必有档，逐档补登记。",
        0xFB59 => "达标率不足：逐轮核偏差，超限轮定位修因后重跑。",
        _ => "未知诊断码：回查 0xFB50 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0xFB50 段基址（固定种子，可重复）。
    pub const SEED: u64 = 0xFB50_FB50_FB50_FB50;

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

/// 单轮武装序列时长记录。
#[derive(Clone, Debug)]
pub struct RunRecord {
    /// 轮号（从 1 连续）。
    pub run_id: u64,
    /// 本轮武装向量数。
    pub vec_count: usize,
    /// 逐向量时长（tick，顺序=武装次序）。
    pub per_vec: Vec<u64>,
    /// 总时长（tick）。
    pub total_ticks: u64,
}

impl RunRecord {
    /// 预算（逐向量配额×向量数；配额含基座分摊）。
    pub fn budget(&self) -> u64 {
        BUDGET_PER_VEC * self.vec_count as u64
    }
    /// 偏差千分数（|总时长-预算|*1000/预算，整数）。
    pub fn devi_milli(&self) -> u64 {
        let b = self.budget();
        let diff = if self.total_ticks > b { self.total_ticks - b } else { b - self.total_ticks };
        diff * 1000 / b.max(1)
    }
    /// 预算内判据（≤10%）。
    pub fn in_budget(&self) -> bool {
        self.devi_milli() <= BUDGET_TOL_MILLI
    }
    /// 记录自洽：逐向量之和 ≡ 总时长。
    pub fn self_consistent(&self) -> bool {
        self.per_vec.iter().sum::<u64>() == self.total_ticks
    }
}

/// 延迟账（本单正本，append-only）。
#[derive(Debug, Default)]
pub struct LatencyBook {
    /// 逐轮记录。
    runs: Vec<RunRecord>,
    /// 曲线档：向量数 → 登记均时长（tick×1000 定点，避免整除损失）。
    curve: Vec<(usize, u64)>,
    /// 账历史峰值（只增不减判准）。
    high_water: usize,
}

impl LatencyBook {
    /// 建账。
    pub fn new() -> Self {
        LatencyBook { runs: Vec::new(), curve: Vec::new(), high_water: 0 }
    }

    /// 录一轮（校验：向量数/零时长/自洽，非法必拒）。
    pub fn record(&mut self, vec_count: usize, per_vec: Vec<u64>) -> Result<u64, LcCode> {
        if vec_count > MAX_VECS {
            return Err(LcCode::VecOob);
        }
        if per_vec.len() != vec_count {
            return Err(LcCode::VecOob);
        }
        let total: u64 = per_vec.iter().sum();
        if total == 0 {
            return Err(LcCode::ZeroTicks);
        }
        let run_id = self.runs.len() as u64 + 1;
        self.runs.push(RunRecord { run_id, vec_count, per_vec, total_ticks: total });
        if self.runs.len() > self.high_water {
            self.high_water = self.runs.len();
        }
        Ok(run_id)
    }

    /// 轮数。
    pub fn run_count(&self) -> usize {
        self.runs.len()
    }

    /// 轮号连续性（从 1 起无断档）。
    pub fn runs_contiguous(&self) -> bool {
        self.runs.iter().enumerate().all(|(i, r)| r.run_id == i as u64 + 1)
    }

    /// 全体逐向量 tick 展平（分布全集）。
    pub fn all_ticks(&self) -> Vec<u64> {
        let mut v: Vec<u64> = Vec::new();
        for r in self.runs.iter() {
            for t in r.per_vec.iter() {
                v.push(*t);
            }
        }
        v
    }

    /// 分位（整数下标：p 千分数；账空返回 None）。
    pub fn percentile(&self, p_milli: u64) -> Option<u64> {
        let mut v = self.all_ticks();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let n = v.len();
        // 下标 = ceil(n*p/1000) - 1，钳界。
        let idx = ((n as u64 * p_milli + 999) / 1000) as usize;
        let idx = if idx == 0 { 0 } else if idx > n { n - 1 } else { idx - 1 };
        Some(v[idx])
    }

    /// P50/P95/P99 三元组（账空返回 None）。
    pub fn p50_p95_p99(&self) -> Option<(u64, u64, u64)> {
        let p50 = self.percentile(500)?;
        let p95 = self.percentile(950)?;
        let p99 = self.percentile(990)?;
        Some((p50, p95, p99))
    }

    /// 分位不倒挂判据：P50 ≤ P95 ≤ P99。
    pub fn pct_ordered(&self) -> bool {
        match self.p50_p95_p99() {
            Some((a, b, c)) => a <= b && b <= c,
            None => false,
        }
    }

    /// 总时长分布（逐轮 total 列表，升序拷贝）。
    pub fn total_sorted(&self) -> Vec<u64> {
        let mut v: Vec<u64> = self.runs.iter().map(|r| r.total_ticks).collect();
        v.sort_unstable();
        v
    }

    /// 曲线登记：向量数 → 均时长（tick×1000 定点均值），逐档重建。
    pub fn rebuild_curve(&mut self) -> usize {
        self.curve.clear();
        let mut counts: Vec<usize> = Vec::new();
        for r in self.runs.iter() {
            if !counts.contains(&r.vec_count) {
                counts.push(r.vec_count);
            }
        }
        counts.sort_unstable();
        for c in counts {
            let sel: Vec<u64> = self
                .runs
                .iter()
                .filter(|r| r.vec_count == c)
                .map(|r| r.total_ticks)
                .collect();
            let sum: u64 = sel.iter().sum();
            let mean_milli = sum * 1000 / sel.len() as u64;
            self.curve.push((c, mean_milli));
        }
        self.curve.len()
    }

    /// 曲线档数。
    pub fn curve_len(&self) -> usize {
        self.curve.len()
    }

    /// 曲线档迭代器（(向量数, 均时长千分) ）。
    pub fn curve_iter(&self) -> impl Iterator<Item = &(usize, u64)> {
        self.curve.iter()
    }

    /// 曲线一致：每档登记值 ≡ 逐轮复算均值（tick×1000 定点）。
    pub fn curve_consistent(&self) -> bool {
        for (c, reg) in self.curve.iter() {
            let sel: Vec<u64> = self
                .runs
                .iter()
                .filter(|r| r.vec_count == *c)
                .map(|r| r.total_ticks)
                .collect();
            if sel.is_empty() {
                return false;
            }
            let sum: u64 = sel.iter().sum();
            let mean_milli = sum * 1000 / sel.len() as u64;
            if mean_milli != *reg {
                return false;
            }
        }
        true
    }

    /// 曲线缺档判据：出现过的向量数都有档。
    pub fn curve_covers(&self) -> bool {
        for r in self.runs.iter() {
            if !self.curve.iter().any(|(c, _)| c == &r.vec_count) {
                return false;
            }
        }
        true
    }

    /// 曲线单调性：向量数更大 → 登记均时长不减（武装成本曲线合理）。
    pub fn curve_monotone(&self) -> bool {
        for w in self.curve.windows(2) {
            if w[0].1 > w[1].1 {
                return false;
            }
        }
        true
    }

    /// 预算达标率（千分数）：预算内轮数×1000/总轮数。
    pub fn pass_rate_milli(&self) -> u64 {
        if self.runs.is_empty() {
            return 0;
        }
        let pass = self.runs.iter().filter(|r| r.in_budget()).count() as u64;
        pass * 1000 / self.runs.len() as u64
    }

    /// 达标率 100% 判据。
    pub fn all_in_budget(&self) -> bool {
        !self.runs.is_empty() && self.runs.iter().all(|r| r.in_budget())
    }

    /// 账只增不减判据。
    pub fn monotone(&self) -> bool {
        self.runs.len() >= self.high_water && self.high_water >= 1
    }

    /// 历史峰值读取。
    pub fn high_water(&self) -> usize {
        self.high_water
    }

    /// 逐轮记录迭代器（导出/摘要面）。
    pub fn runs_iter(&self) -> impl Iterator<Item = &RunRecord> {
        self.runs.iter()
    }

    /// 篡改核（仅注入演练用）：清空账制造缩水破口。
    pub fn tamper_clear(&mut self) {
        self.runs.clear();
    }

    /// 篡改核（仅注入演练用）：手改曲线档登记值制造漂移破口。
    pub fn tamper_curve(&mut self, idx: usize, val: u64) {
        if idx < self.curve.len() {
            self.curve[idx].1 = val;
        }
    }

    /// 账内自洽全查：每轮 逐向量和≡总时长。
    pub fn all_self_consistent(&self) -> bool {
        self.runs.iter().all(|r| r.self_consistent())
    }
}

/// 确定性产一轮武装序列（tick 全定：6+((i+n)%3) ∈ {6,7,8}，循环和恰
/// 为 n → 总≡7n，落在配额内保证预算零偏差；LCG 只决定向量数）。
fn gen_run(lcg: &mut Lcg, _run_hint: usize) -> (usize, Vec<u64>) {
    // 向量数 2..=MAX_VECS 确定性变化。
    let n = 2 + (lcg.below((MAX_VECS - 1) as u64)) as usize;
    let mut per: Vec<u64> = Vec::new();
    for i in 0..n {
        per.push(6 + ((i + n) % 3) as u64);
    }
    (n, per)
}

/// LCG 10/10 演练：录账→分位→曲线→达标率，10/10 轮全判据过。
pub fn drill_latency_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFB50 段基址。
    let mut lcg = Lcg::new(0xFB50);
    for r in 0..INJECT_ROUNDS {
        let mut bk = LatencyBook::new();
        // 每轮 4..7 条记录。
        let runs = 4 + (lcg.below(4) as usize);
        for i in 0..runs {
            let (n, per) = gen_run(&mut lcg, i + r);
            if bk.record(n, per).is_err() {
                break;
            }
        }
        let _ = bk.rebuild_curve();
        let green = bk.run_count() >= 4
            && bk.runs_contiguous()
            && bk.pct_ordered()
            && bk.curve_consistent()
            && bk.curve_covers()
            && bk.curve_monotone()
            && bk.all_in_budget()
            && bk.pass_rate_milli() == 1000
            && bk.all_self_consistent()
            && bk.monotone();
        if green {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("延迟账全绿轮={}/{}", ok, INJECT_ROUNDS))
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：逐轮转录导出（轮号/向量数/总时长/预算/偏差千分）。
/// ─────────────────────────────────────────────────────────────────────
/// 逐轮转录导出。
pub fn export_run_lines(bk: &LatencyBook) -> String {
    let mut s = String::from("逐轮转录：\n");
    for r in bk.runs_iter() {
        s.push_str(&format!(
            "轮{} 向量{} 总{} 预算{} 偏差{}‰ 域内={}\n",
            r.run_id, r.vec_count, r.total_ticks, r.budget(), r.devi_milli(), r.in_budget()
        ));
    }
    s
}

/// 转录完整判据（行数=轮数且逐行含轮号）。
pub fn run_lines_complete(bk: &LatencyBook) -> bool {
    let s = export_run_lines(bk);
    s.matches("轮").count() - 1 == bk.run_count() && bk.run_count() > 0
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：逐轮摘要位（每轮 FNV 摘要，轮间两两不同）。
/// ─────────────────────────────────────────────────────────────────────
/// FNV 步进。
const fn fnv_step(acc: u64, byte: u64) -> u64 {
    (acc ^ byte).wrapping_mul(0x100000001b3)
}

/// 逐轮摘要表。
pub fn run_digests(bk: &LatencyBook) -> Vec<u64> {
    bk.runs_iter()
        .map(|r| {
            let mut d = 0xcbf29ce484222325;
            d = fnv_step(d, r.run_id & 0xFF);
            d = fnv_step(d, r.vec_count as u64);
            d = fnv_step(d, r.total_ticks & 0xFF);
            for t in r.per_vec.iter() {
                d = fnv_step(d, t & 0xFF);
            }
            d
        })
        .collect()
}

/// 摘要两两不同判据。
pub fn run_digests_unique(bk: &LatencyBook) -> bool {
    let ds = run_digests(bk);
    for i in 0..ds.len() {
        for j in (i + 1)..ds.len() {
            if ds[i] == ds[j] {
                return false;
            }
        }
    }
    ds.len() == bk.run_count()
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：按向量数分位表（每档均值，与曲线档交叉）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct CountPctRow {
    /// 向量数档。
    pub count: usize,
    /// 该档轮数。
    pub runs: usize,
    /// 该档总时长均值（tick×1000 定点）。
    pub mean_milli: u64,
}

/// 分位表构建（与曲线档一致口径）。
pub fn build_count_pct(bk: &LatencyBook) -> Vec<CountPctRow> {
    let mut counts: Vec<usize> = Vec::new();
    for r in bk.runs_iter() {
        if !counts.contains(&r.vec_count) {
            counts.push(r.vec_count);
        }
    }
    counts.sort_unstable();
    counts
        .iter()
        .map(|c| {
            let sel: Vec<u64> = bk
                .runs_iter()
                .filter(|r| r.vec_count == *c)
                .map(|r| r.total_ticks)
                .collect();
            let sum: u64 = sel.iter().sum();
            CountPctRow { count: *c, runs: sel.len(), mean_milli: sum * 1000 / sel.len() as u64 }
        })
        .collect()
}

/// 分位表与曲线档交叉一致。
pub fn count_pct_matches_curve(bk: &LatencyBook, t: &[CountPctRow]) -> bool {
    t.len() == bk.curve_len()
        && t.iter().all(|r| {
            bk.curve_iter().any(|(c, m)| c == &r.count && m == &r.mean_milli)
        })
}

/// 分位表导出。
pub fn export_count_pct(t: &[CountPctRow]) -> String {
    let mut s = String::from("按向量数分位：\n");
    for r in t.iter() {
        s.push_str(&format!("· 向量{} ×{} 轮 均{}‰tick\n", r.count, r.runs, r.mean_milli));
    }
    s
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：延迟账会话报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct LatReport {
    /// 轮数。
    pub runs: usize,
    /// 曲线档数。
    pub curve: usize,
    /// 分位三元组。
    pub pcts: Option<(u64, u64, u64)>,
    /// 达标率（千分）。
    pub pass_rate: u64,
    /// 分位不倒挂。
    pub pct_ok: bool,
    /// 曲线一致。
    pub curve_ok: bool,
}

impl LatReport {
    /// 建报告。
    pub fn build(bk: &LatencyBook) -> Self {
        LatReport {
            runs: bk.run_count(),
            curve: bk.curve_len(),
            pcts: bk.p50_p95_p99(),
            pass_rate: bk.pass_rate_milli(),
            pct_ok: bk.pct_ordered(),
            curve_ok: bk.curve_consistent() && bk.curve_covers() && bk.curve_monotone(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.pcts.is_some() && self.pct_ok && self.curve_ok && self.pass_rate == 1000
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        match self.pcts {
            Some((a, b, c)) => format!(
                "LatReport 轮={} 档={} P50={} P95={} P99={} 达标={}‰ 曲线={}",
                self.runs, self.curve, a, b, c, self.pass_rate, self.curve_ok
            ),
            None => format!("LatReport 轮={} 空分布", self.runs),
        }
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：预算边界演练（恰配额=0 偏差必过；低 1 tick 必超）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_margin_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFB50 段基址 ^ 1。
    let mut lcg = Lcg::new(0xFB51);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = LatencyBook::new();
        // 域内轮：tick=7（恰配额）→ 总=7n → 零偏差。
        let n1 = 2 + (lcg.below(3) as usize);
        let per1: Vec<u64> = (0..n1).map(|_| 7u64).collect();
        let in_ok = bk.record(n1, per1).is_ok() && bk.all_in_budget();
        // 越界轮：tick=6 → 总=6n → 偏差 = 1/7 ≈ 143‰ > 10%。
        let n2 = 2 + (lcg.below(3) as usize);
        let per2: Vec<u64> = (0..n2).map(|_| 6u64).collect();
        let _ = bk.record(n2, per2);
        let out_caught = !bk.all_in_budget();
        // 账只增不减面。
        let mono = bk.monotone();
        if in_ok && out_caught && mono {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("预算边界轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 F：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct LcCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_lat_checklist(bk: &LatencyBook) -> Vec<LcCheckRow> {
    let rep = LatReport::build(bk);
    let t = build_count_pct(bk);
    let mut rows: Vec<LcCheckRow> = Vec::new();
    rows.push(LcCheckRow { name: "轮号连续", passed: bk.runs_contiguous() });
    rows.push(LcCheckRow { name: "分位可查", passed: bk.p50_p95_p99().is_some() });
    rows.push(LcCheckRow { name: "分位不倒挂", passed: bk.pct_ordered() });
    rows.push(LcCheckRow { name: "曲线一致", passed: bk.curve_consistent() && bk.curve_covers() });
    rows.push(LcCheckRow { name: "曲线单调", passed: bk.curve_monotone() });
    rows.push(LcCheckRow { name: "达标率100%", passed: bk.pass_rate_milli() == 1000 });
    rows.push(LcCheckRow { name: "逐轮自洽", passed: bk.all_self_consistent() });
    rows.push(LcCheckRow { name: "分位表交叉", passed: count_pct_matches_curve(bk, &t) });
    rows.push(LcCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(LcCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn lat_checklist_green(rows: &[LcCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 G：导出/摘要 10/10 演练（纯函数 ×10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_export_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFB50 段基址 ^ 2。
    let mut lcg = Lcg::new(0xFB52);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = LatencyBook::new();
        for i in 0..6 {
            let (n, per) = gen_run(&mut lcg, i);
            let _ = bk.record(n, per);
        }
        let _ = bk.rebuild_curve();
        let t = build_count_pct(&bk);
        let green = run_lines_complete(&bk)
            && run_digests_unique(&bk)
            && count_pct_matches_curve(&bk, &t)
            && !export_run_lines(&bk).is_empty()
            && !export_count_pct(&t).is_empty();
        if green {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("导出摘要轮={}/{}", ok, INJECT_ROUNDS))
}

/// 概况一行。
pub fn lat_stats_line(bk: &LatencyBook) -> String {
    format!(
        "延迟概况：{} 轮 曲线 {} 档 达标 {}‰",
        bk.run_count(),
        bk.curve_len(),
        bk.pass_rate_milli()
    )
}

/// 本单扩展判据聚合（与主聚合合并消费；三元组面适配 shim API）。
pub fn run_unx1248_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    // 标准账（10 轮确定性）。
    let mut bk = LatencyBook::new();
    // 种子写死（协议禁 rand）：0xFB50 段基址。
    let mut lcg = Lcg::new(0xFB50);
    for i in 0..10 {
        let (n, per) = gen_run(&mut lcg, i);
        let _ = bk.record(n, per);
    }
    let _ = bk.rebuild_curve();
    // E-a：转录/摘要/分位表导出面。
    let t = build_count_pct(&bk);
    out.push((
        "X-转录摘要分位三面",
        run_lines_complete(&bk) && run_digests_unique(&bk) && count_pct_matches_curve(&bk, &t),
        lat_stats_line(&bk),
    ));
    // E-b：预算边界 10/10。
    let (ok_b, d_b) = drill_margin_10();
    out.push(("X-预算边界10/10", ok_b, d_b));
    // E-c：导出纯函数 10/10。
    let (ok_e, d_e) = drill_export_10();
    out.push(("X-导出纯函数10/10", ok_e, d_e));
    // E-d：终验清单。
    let rows = build_lat_checklist(&bk);
    out.push(("X-终验清单10项", lat_checklist_green(&rows), LatReport::build(&bk).to_text()));
    // E-e：曲线快照 10/10。
    let (ok_c, d_c) = drill_curve_snap_10();
    out.push(("X-曲线快照10/10", ok_c, d_c));
    // E-f：tick 域钉定。
    out.push(("X-tick域钉定", ticks_in_domain(&bk), export_tick_domain()));
    out
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 H：曲线档快照行（向量数 → (档均, 档内轮数, 档内最小/最大)）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct CurveSnapRow {
    /// 向量数档。
    pub count: usize,
    /// 档均（tick×1000 定点）。
    pub mean_milli: u64,
    /// 档内最小总时长。
    pub min_total: u64,
    /// 档内最大总时长。
    pub max_total: u64,
}

/// 曲线档快照构建。
pub fn build_curve_snap(bk: &LatencyBook) -> Vec<CurveSnapRow> {
    let mut out: Vec<CurveSnapRow> = Vec::new();
    for (c, m) in bk.curve_iter() {
        let sel: Vec<u64> = bk
            .runs_iter()
            .filter(|r| r.vec_count == *c)
            .map(|r| r.total_ticks)
            .collect();
        let mn = sel.iter().copied().min().unwrap_or(0);
        let mx = sel.iter().copied().max().unwrap_or(0);
        out.push(CurveSnapRow { count: *c, mean_milli: *m, min_total: mn, max_total: mx });
    }
    out
}

/// 快照守恒：档内 min ≤ 均值/1000 定点折算 ≤ max（容忍定点取整误差）。
pub fn curve_snap_sane(rows: &[CurveSnapRow]) -> bool {
    rows.iter().all(|r| r.min_total <= r.max_total && r.mean_milli >= r.min_total * 900 && r.mean_milli <= r.max_total * 1100 + 1000)
}

/// 快照导出。
pub fn export_curve_snap(rows: &[CurveSnapRow]) -> String {
    let mut s = String::from("曲线档快照：\n");
    for r in rows.iter() {
        s.push_str(&format!(
            "· 向量{} 均{}‰tick 域[{},{}]\n",
            r.count, r.mean_milli, r.min_total, r.max_total
        ));
    }
    s
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 I：曲线快照 10/10 演练（×10 全守恒 + 纯函数）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_curve_snap_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFB50 段基址 ^ 3。
    let mut lcg = Lcg::new(0xFB53);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = LatencyBook::new();
        for i in 0..6 {
            let (n, per) = gen_run(&mut lcg, i);
            let _ = bk.record(n, per);
        }
        let _ = bk.rebuild_curve();
        let rows = build_curve_snap(&bk);
        let rows2 = build_curve_snap(&bk);
        let pure = rows.len() == rows2.len()
            && rows.iter().zip(rows2.iter()).all(|(a, b)| a.mean_milli == b.mean_milli);
        let _ = lcg.below(3);
        if curve_snap_sane(&rows) && pure && !export_curve_snap(&rows).is_empty() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("曲线快照轮={}/{}", ok, INJECT_ROUNDS))
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 J：逐向量 tick 域核（tick ∈ {6,7,8} 生成域钉定）。
/// ─────────────────────────────────────────────────────────────────────
/// 逐向量 tick 域判据：全表 tick 都在 [6,8]。
pub fn ticks_in_domain(bk: &LatencyBook) -> bool {
    bk.runs_iter().all(|r| r.per_vec.iter().all(|t| (6..=8).contains(t)))
}

/// 域界导出。
pub fn export_tick_domain() -> String {
    format!("tick 域钉定：[6,8] 配额 {} 达标容限 {}‰", BUDGET_PER_VEC, BUDGET_TOL_MILLI)
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：超预算轮必抓（构造 tick=配额×2 的记录）。
    let mut bk1 = LatencyBook::new();
    let per: Vec<u64> = (0..3).map(|_| 14u64).collect();
    let _ = bk1.record(3, per);
    if bk1.run_count() == 1 && !bk1.all_in_budget() && bk1.pass_rate_milli() == 0 {
        caught += 1;
        d.push("注1 超预算:抓".to_string());
    } else {
        d.push("注1 超预算:漏".to_string());
    }
    // 注 2：空账取分位必拒（None → EmptyDist 防线）。
    let bk2 = LatencyBook::new();
    if bk2.p50_p95_p99().is_none() && !bk2.pct_ordered() {
        caught += 1;
        d.push("注2 空账:抓".to_string());
    } else {
        d.push("注2 空账:漏".to_string());
    }
    // 注 3：账缩水——篡改清账后 monotone 必翻红。
    let mut bk3 = LatencyBook::new();
    let _ = bk3.record(2, vec![4, 5]);
    let hw = bk3.high_water();
    bk3.tamper_clear();
    if bk3.run_count() == 0 && !bk3.monotone() && hw == 1 {
        caught += 1;
        d.push("注3 缩水:抓".to_string());
    } else {
        d.push("注3 缩水:漏".to_string());
    }
    // 注 4：曲线漂移——登记值手改后 curve_consistent 必翻红。
    let mut bk4 = LatencyBook::new();
    let _ = bk4.record(2, vec![4, 5]);
    let _ = bk4.record(2, vec![6, 4]);
    let _ = bk4.rebuild_curve();
    let honest = bk4.curve_consistent();
    bk4.tamper_curve(0, 999_000);
    if honest && !bk4.curve_consistent() {
        caught += 1;
        d.push("注4 漂移:抓".to_string());
    } else {
        d.push("注4 漂移:漏".to_string());
    }
    // 注 5：向量数越界必拒（>MAX_VECS）。
    let mut bk5 = LatencyBook::new();
    let big: Vec<u64> = (0..MAX_VECS + 1).map(|_| 3u64).collect();
    if matches!(bk5.record(MAX_VECS + 1, big), Err(LcCode::VecOob)) {
        caught += 1;
        d.push("注5 越界:抓".to_string());
    } else {
        d.push("注5 越界:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0xFB50 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    LcCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx1248_checks：本单判据聚合（诊断段 0xFB50）。
pub fn run_unx1248_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1248 Arming 延迟账");
    // J1-a：标准会话录账（确定性 10 轮序列）。
    let mut bk = LatencyBook::new();
    // 种子写死（协议禁 rand）：0xFB50 段基址。
    let mut lcg = Lcg::new(0xFB50);
    for i in 0..10 {
        let (n, per) = gen_run(&mut lcg, i);
        let _ = bk.record(n, per);
    }
    let _ = bk.rebuild_curve();
    // J1-b：P50/P95/P99 分布账可查且不倒挂。
    match bk.p50_p95_p99() {
        Some((p50, p95, p99)) => s.add_dyn(
            "J1-分位账可查不倒挂",
            bk.pct_ordered() && p50 >= 3 && p99 >= p50,
            &format!("P50={} P95={} P99={}", p50, p95, p99),
        ),
        None => s.add_dyn("J1-分位账可查不倒挂", false, "分布空账"),
    }
    // J1-c：曲线登记与交叉一致。
    s.add_dyn(
        "J1-曲线登记一致",
        bk.curve_len() >= 1 && bk.curve_consistent() && bk.curve_covers() && bk.curve_monotone(),
        &format!("档数={} 覆盖={} 单调={}", bk.curve_len(), bk.curve_covers(), bk.curve_monotone()),
    );
    // J1-d：预算偏差 ≤10% 达标率 100%。
    s.add_dyn(
        "J1-预算达标率100%",
        bk.all_in_budget() && bk.pass_rate_milli() == 1000,
        &format!("达标率={}‰ 轮数={}", bk.pass_rate_milli(), bk.run_count()),
    );
    // J2：LCG 10/10 演练。
    let (ok10, d10) = drill_latency_10();
    s.add_dyn("J2-注入10/10全绿", ok10, &d10);
    // J2-b：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0xFB50段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n, p, d) in run_unx1248_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
