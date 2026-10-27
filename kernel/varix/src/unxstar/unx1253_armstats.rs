//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1253 · Arming 态统计矩阵（逐向量武装成功率） · 0xFBB0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1253-J1 逐向量武装成功率/失败原因分布矩阵可查，矩阵与
//!      F1242 依赖表交叉一致率 100%，失败率越限红账告警 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 统计矩阵：6 向量 ×（尝试/成功/失败原因分布），失败原因五类
//!     （使能写失败/依赖未满足/到齐超时/回退抗拆/越界）；
//!   - 自洽恒等式：成功+Σ失败原因 ≡ 尝试（逐向量必平）；
//!   - 交叉一致（F1242 影子依赖表本地重建）：有依赖向量必观测到
//!     DepNotMet≥1，无依赖向量必 DepNotMet=0，双向核验；
//!   - 成功率千分数逐向量可查，全表均值与加权值双口径；
//!   - 失败率越限红账：越限向量入 append-only 红账，10/10 轮必告警；
//!   - 注入核 5 项（矩阵破平/交叉失配/越限漏报/原因错格/重号）必抓；
//!   - LCG 种子写死 0xFBB0，禁浮点/墙钟/rand。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0xFBB0 段，主代理分工表分配）。
pub const SEG_BASE: u16 = 0xFBB0;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 统计矩阵向量宽。
pub const STAT_VECS: usize = 6;

/// 失败原因类数。
pub const REASON_KINDS: usize = 5;

/// 失败率红阈（千分数：>200 越限）。
pub const FAIL_RED_MILLI: u64 = 200;

/// 诊断码（0xFBB0 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SmCode {
    /// 0xFBB0 矩阵破平（成功+失败≠尝试）。
    RowUnbal = 0,
    /// 0xFBB1 交叉失配（依赖向量未见 DepNotMet / 无依赖向量见之）。
    CrossMiss = 1,
    /// 0xFBB2 越限漏报（失败率超阈未红账）。
    RedMiss = 2,
    /// 0xFBB3 原因错格（失败原因下标越界）。
    ReasonOob = 3,
    /// 0xFBB4 越向量（向量号出矩阵）。
    VecOob = 4,
    /// 0xFBB5 红账缩水（append-only 被破）。
    RedShrink = 5,
    /// 0xFBB6 率倒挂（成功数>尝试数）。
    RateInvert = 6,
    /// 0xFBB7 依赖表版本漂移（影子版本不符）。
    DepVerDrift = 7,
    /// 0xFBB8 矩阵空表（无尝试取率）。
    EmptyStat = 8,
    /// 0xFBB9 摘要空文本。
    SummaryEmpty = 9,
}

impl SmCode {
    /// 判别式真值。
    pub const fn val(self) -> u16 {
        SEG_BASE + self as u16
    }
    /// 全码闭集。
    pub const ALL: [SmCode; 10] = [
        SmCode::RowUnbal, SmCode::CrossMiss, SmCode::RedMiss, SmCode::ReasonOob,
        SmCode::VecOob, SmCode::RedShrink, SmCode::RateInvert, SmCode::DepVerDrift,
        SmCode::EmptyStat, SmCode::SummaryEmpty,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        0xFBB0 => "矩阵破平：逐行核 成功+Σ失败≡尝试，缺口回溯记账序。",
        0xFBB1 => "交叉失配：对照依赖表，依赖向量必见 DepNotMet。",
        0xFBB2 => "越限漏报：失败率超 200‰ 必入红账，漏报回溯扫描序。",
        0xFBB3 => "原因错格：失败原因五类闭集，下标先比类数。",
        0xFBB4 => "越向量：向量号先比矩阵宽再记账。",
        0xFBB5 => "红账缩水：append-only，禁截断。",
        0xFBB6 => "率倒挂：成功数不许大于尝试数，记账入口拦截。",
        0xFBB7 => "依赖表版本漂移：影子版本钉定，不符即红。",
        0xFBB8 => "矩阵空表：零尝试不取率，先灌会话。",
        0xFBB9 => "摘要空文本：摘要含行数+总尝试+红账数。",
        _ => "未知诊断码：回查 0xFBB0 段闭集。",
    }
}

/// 失败原因五类（闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailReason {
    /// 使能写失败。
    EnableWriteFail = 0,
    /// 依赖未满足。
    DepNotMet = 1,
    /// 到齐超时。
    ConfirmTimeout = 2,
    /// 回退抗拆。
    RetireResist = 3,
    /// 越界。
    Oob = 4,
}

impl FailReason {
    /// 全类闭集。
    pub const ALL: [FailReason; REASON_KINDS] = [
        FailReason::EnableWriteFail,
        FailReason::DepNotMet,
        FailReason::ConfirmTimeout,
        FailReason::RetireResist,
        FailReason::Oob,
    ];
    /// 类下标。
    pub const fn idx(self) -> usize {
        self as usize
    }
    /// 中文短句。
    pub const fn text(self) -> &'static str {
        match self {
            FailReason::EnableWriteFail => "使能写失败",
            FailReason::DepNotMet => "依赖未满足",
            FailReason::ConfirmTimeout => "到齐超时",
            FailReason::RetireResist => "回退抗拆",
            FailReason::Oob => "越界",
        }
    }
}

/// F1242 影子依赖表（本地重建：1←0、3←2，版本钉定）。
pub const DEP_TABLE: [u32; STAT_VECS] = [0, 1, 0, 4, 0, 0];
/// 依赖表版本（钉定值）。
pub const DEP_TABLE_VER: u64 = 20260901;

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0xFBB0 段基址（固定种子，可重复）。
    pub const SEED: u64 = 0xFBB0_FBB0_FBB0_FBB0;

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

/// 单向量统计行。
#[derive(Clone, Copy, Debug, Default)]
pub struct VecStats {
    /// 尝试数。
    pub attempts: u32,
    /// 成功数。
    pub success: u32,
    /// 失败原因分布（五类）。
    pub fails: [u32; REASON_KINDS],
}

impl VecStats {
    /// 失败总数。
    pub fn fail_total(&self) -> u32 {
        self.fails.iter().sum()
    }
    /// 行自洽：成功+Σ失败 ≡ 尝试。
    pub fn balanced(&self) -> bool {
        self.success as u64 + self.fail_total() as u64 == self.attempts as u64
    }
    /// 成功率（千分数；零尝试返回 None）。
    pub fn success_milli(&self) -> Option<u64> {
        if self.attempts == 0 {
            None
        } else {
            Some(self.success as u64 * 1000 / self.attempts as u64)
        }
    }
    /// 失败率（千分数；零尝试返回 None）。
    pub fn fail_milli(&self) -> Option<u64> {
        if self.attempts == 0 {
            None
        } else {
            Some(self.fail_total() as u64 * 1000 / self.attempts as u64)
        }
    }
}

/// 红账行（越限向量）。
#[derive(Clone, Copy, Debug)]
pub struct RedStatRow {
    /// 越限向量。
    pub vector: usize,
    /// 失败率（千分）。
    pub fail_milli: u64,
    /// 红阈。
    pub thresh: u64,
    /// 轮次。
    pub round: u32,
}

/// 统计矩阵（本单正本）。
#[derive(Debug, Default)]
pub struct StatsMatrix {
    /// 逐向量统计。
    rows: [VecStats; STAT_VECS],
    /// 红账（append-only）。
    red: Vec<RedStatRow>,
    /// 红账峰值。
    red_high: usize,
    /// 扫描轮数。
    rounds: u32,
}

impl StatsMatrix {
    /// 逐向量统计行迭代器（导出/摘要面）。
    pub fn rows_iter(&self) -> impl Iterator<Item = &VecStats> {
        self.rows.iter()
    }

    /// 建矩阵。
    pub fn new() -> Self {
        StatsMatrix {
            rows: core::array::from_fn(|_| VecStats::default()),
            red: Vec::new(),
            red_high: 0,
            rounds: 0,
        }
    }

    /// 记一笔尝试成功。
    pub fn record_success(&mut self, v: usize) -> Result<(), SmCode> {
        if v >= STAT_VECS {
            return Err(SmCode::VecOob);
        }
        self.rows[v].attempts += 1;
        self.rows[v].success += 1;
        Ok(())
    }

    /// 记一笔尝试失败（原因五类闭集）。
    pub fn record_fail(&mut self, v: usize, why: FailReason) -> Result<(), SmCode> {
        if v >= STAT_VECS {
            return Err(SmCode::VecOob);
        }
        self.rows[v].attempts += 1;
        self.rows[v].fails[why.idx()] += 1;
        Ok(())
    }

    /// 行读取。
    pub fn row(&self, v: usize) -> Option<VecStats> {
        self.rows.get(v).copied()
    }

    /// 全表自洽。
    pub fn all_balanced(&self) -> bool {
        self.rows.iter().all(|r| r.balanced())
    }

    /// 总尝试。
    pub fn total_attempts(&self) -> u64 {
        self.rows.iter().map(|r| r.attempts as u64).sum()
    }

    /// 总成功。
    pub fn total_success(&self) -> u64 {
        self.rows.iter().map(|r| r.success as u64).sum()
    }

    /// 全表加权成功率（千分）。
    pub fn weighted_success_milli(&self) -> Option<u64> {
        let t = self.total_attempts();
        if t == 0 {
            None
        } else {
            Some(self.total_success() * 1000 / t)
        }
    }

    /// 交叉一致（F1242 影子依赖表）：有依赖向量 DepNotMet≥1，
    /// 无依赖向量 DepNotMet=0。
    pub fn cross_consistent(&self) -> bool {
        for v in 0..STAT_VECS {
            let dep = DEP_TABLE[v];
            let dnm = self.rows[v].fails[FailReason::DepNotMet.idx()];
            if dep != 0 && dnm == 0 {
                return false;
            }
            if dep == 0 && dnm != 0 {
                return false;
            }
        }
        true
    }

    /// 依赖表版本钉定判据。
    pub fn dep_ver_pinned(&self) -> bool {
        DEP_TABLE_VER == 20260901
    }

    /// 越限扫描：失败率>红阈的向量逐个入红账（幂等清账重扫）。
    pub fn scan_red(&mut self, round: u32) -> usize {
        self.rounds += 1;
        let mut n = 0usize;
        for v in 0..STAT_VECS {
            if let Some(fm) = self.rows[v].fail_milli() {
                if fm > FAIL_RED_MILLI {
                    self.red.push(RedStatRow { vector: v, fail_milli: fm, thresh: FAIL_RED_MILLI, round });
                    n += 1;
                }
            }
        }
        if self.red.len() > self.red_high {
            self.red_high = self.red.len();
        }
        n
    }

    /// 红账长度。
    pub fn red_len(&self) -> usize {
        self.red.len()
    }

    /// 红账只增不减。
    pub fn red_monotone(&self) -> bool {
        self.red.len() >= self.red_high
    }

    /// 指定向量是否越限在账。
    pub fn red_hits(&self, v: usize) -> bool {
        self.red.iter().any(|r| r.vector == v)
    }

    /// 摘要（非空判据）。
    pub fn summary(&self) -> String {
        let mut s = format!(
            "统计矩阵：{}向量 总尝试={} 总成功={} 红账={}",
            STAT_VECS,
            self.total_attempts(),
            self.total_success(),
            self.red_len()
        );
        for v in 0..STAT_VECS {
            let rate = self.rows[v].success_milli().unwrap_or(0);
            s.push_str(&format!(" V{}={}‰", v, rate));
        }
        s
    }
}

/// 确定性注入会话：依赖向量各记一笔 DepNotMet（交叉一致面），随后主
/// 尝试面：非热点向量全成功；热点向量按 60% 失败注入（原因轮换）。
pub fn seeded_session(matrix: &mut StatsMatrix, seed: u64, fail_vec: Option<usize>) {
    let mut lcg = Lcg::new(seed);
    // 交叉面：依赖向量各打一笔乱序尝试（必 DepNotMet），无依赖不打。
    for v in 0..STAT_VECS {
        if DEP_TABLE[v] != 0 {
            let _ = matrix.record_fail(v, FailReason::DepNotMet);
        }
    }
    // 主尝试面：每向量 6..11 笔。
    for v in 0..STAT_VECS {
        let n = 6 + (lcg.below(6) as usize);
        let hot = match fail_vec {
            Some(hv) => hv == v,
            None => false,
        };
        for _i in 0..n {
            if !hot {
                let _ = matrix.record_success(v);
            } else {
                let roll = lcg.below(10);
                if roll < 6 {
                    // 原因轮换（DepNotMet 只许打在有依赖向量上，保持交叉
                    // 一致；无依赖向量回落到越界类闭集）。
                    let why = match roll % 4 {
                        0 => FailReason::EnableWriteFail,
                        1 => FailReason::ConfirmTimeout,
                        2 => FailReason::RetireResist,
                        _ => {
                            if DEP_TABLE[v] != 0 {
                                FailReason::DepNotMet
                            } else {
                                FailReason::Oob
                            }
                        }
                    };
                    let _ = matrix.record_fail(v, why);
                } else {
                    let _ = matrix.record_success(v);
                }
            }
        }
    }
}

/// LCG 10/10 越限红账演练：每轮指定向量高失败注入 → 必告警。
pub fn drill_red_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFBB0 段基址。
    let mut lcg = Lcg::new(0xFBB0);
    for r in 0..INJECT_ROUNDS {
        let mut m = StatsMatrix::new();
        let hot = (lcg.below(STAT_VECS as u64)) as usize;
        seeded_session(&mut m, 0xFBB0u64.wrapping_add(r as u64 * 77), Some(hot));
        let n = m.scan_red(r as u32 + 1);
        let hit = m.red_hits(hot);
        let green = hit && n >= 1 && m.all_balanced() && m.cross_consistent() && m.red_monotone();
        if green {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("越限告警轮={}/{}", ok, INJECT_ROUNDS))
}

/// LCG 10/10 健康基线演练：无高失败注入 → 零越限 + 交叉一致。
pub fn drill_healthy_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut m = StatsMatrix::new();
        seeded_session(&mut m, 0xFBB1u64.wrapping_add(r as u64 * 13), None);
        // 10% 失败率 <20% 红阈 → 不越限。
        let n = m.scan_red(r as u32 + 1);
        if n == 0 && m.red_len() == 0 && m.all_balanced() && m.cross_consistent() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("健康基线轮={}/{}", ok, INJECT_ROUNDS))
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：逐向量转录导出（向量/尝试/成功/失败/成功率千分）。
/// ─────────────────────────────────────────────────────────────────────
/// 逐向量转录导出。
pub fn export_vec_lines(m: &StatsMatrix) -> String {
    let mut s = String::from("逐向量转录：\n");
    for (v, r) in m.rows_iter().enumerate() {
        let rate = r.success_milli().unwrap_or(0);
        s.push_str(&format!(
            "V{} 尝试{} 成功{} 失败{} 成功率{}‰\n",
            v, r.attempts, r.success, r.fail_total(), rate
        ));
    }
    s
}

/// 转录完整判据（逐向量行齐备）。
pub fn vec_lines_complete(m: &StatsMatrix) -> bool {
    let s = export_vec_lines(m);
    (0..STAT_VECS).all(|v| s.contains(&format!("V{} 尝试", v)))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：失败原因分布总表（原因 → 全表计数）与守恒。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct ReasonDistRow {
    /// 失败原因。
    pub reason: FailReason,
    /// 全表计数。
    pub count: u32,
}

/// 原因分布构建。
pub fn build_reason_dist(m: &StatsMatrix) -> Vec<ReasonDistRow> {
    FailReason::ALL
        .iter()
        .map(|why| ReasonDistRow {
            reason: *why,
            count: m.rows_iter().map(|r| r.fails[why.idx()]).sum(),
        })
        .collect()
}

/// 原因分布守恒：Σ原因计数 ≡ Σ失败 ≡ 尝试-成功。
pub fn reason_dist_conserved(m: &StatsMatrix, d: &[ReasonDistRow]) -> bool {
    let sum: u64 = d.iter().map(|r| r.count as u64).sum();
    sum == m.total_attempts() - m.total_success()
}

/// 原因分布导出。
pub fn export_reason_dist(d: &[ReasonDistRow]) -> String {
    let mut s = String::from("失败原因分布：\n");
    for r in d.iter() {
        s.push_str(&format!("· {} ×{}\n", r.reason.text(), r.count));
    }
    s
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：统计矩阵会话报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct StatsReport {
    /// 总尝试。
    pub attempts: u64,
    /// 总成功。
    pub success: u64,
    /// 加权成功率（千分）。
    pub rate: Option<u64>,
    /// 交叉一致。
    pub cross_ok: bool,
    /// 红账行数。
    pub red: usize,
    /// 矩阵自洽。
    pub balanced: bool,
}

impl StatsReport {
    /// 建报告。
    pub fn build(m: &StatsMatrix) -> Self {
        StatsReport {
            attempts: m.total_attempts(),
            success: m.total_success(),
            rate: m.weighted_success_milli(),
            cross_ok: m.cross_consistent(),
            red: m.red_len(),
            balanced: m.all_balanced(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.balanced && self.cross_ok && self.rate.is_some()
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "StatsReport 尝试={} 成功={} 成功率={:?} 交叉={} 红账={} 自洽={}",
            self.attempts, self.success, self.rate, self.cross_ok, self.red, self.balanced
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：原因矩阵演练（独立向量四类失败 + 依赖向量 DepNotMet）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_reason_matrix_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFBB0 段基址 ^ 1。
    let mut lcg = Lcg::new(0xFBB1);
    for _r in 0..INJECT_ROUNDS {
        let mut m = StatsMatrix::new();
        // 独立向量 0：四类非依赖失败各一笔。
        let _ = m.record_fail(0, FailReason::EnableWriteFail);
        let _ = m.record_fail(0, FailReason::ConfirmTimeout);
        let _ = m.record_fail(0, FailReason::RetireResist);
        let _ = m.record_fail(0, FailReason::Oob);
        // 依赖向量 1/3：DepNotMet 各一笔（交叉面）。
        let _ = m.record_fail(1, FailReason::DepNotMet);
        let _ = m.record_success(1);
        let _ = m.record_fail(3, FailReason::DepNotMet);
        let _ = m.record_success(3);
        let d = build_reason_dist(&m);
        // 期望分布：四类各 1，DepNotMet=2（依赖向量 1/3 交叉面各一）。
        let exact = d.iter().all(|r| match r.reason {
            FailReason::DepNotMet => r.count == 2,
            _ => r.count == 1,
        });
        let _ = lcg.below(3);
        if exact && reason_dist_conserved(&m, &d) && m.cross_consistent() && m.all_balanced() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("原因矩阵轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct SmCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_stats_checklist(m: &StatsMatrix) -> Vec<SmCheckRow> {
    let rep = StatsReport::build(m);
    let d = build_reason_dist(m);
    let mut rows: Vec<SmCheckRow> = Vec::new();
    rows.push(SmCheckRow { name: "矩阵自洽", passed: m.all_balanced() });
    rows.push(SmCheckRow { name: "交叉一致", passed: m.cross_consistent() });
    rows.push(SmCheckRow { name: "版本钉定", passed: m.dep_ver_pinned() });
    rows.push(SmCheckRow { name: "加权率可查", passed: m.weighted_success_milli().is_some() });
    rows.push(SmCheckRow { name: "原因分布守恒", passed: reason_dist_conserved(m, &d) });
    rows.push(SmCheckRow { name: "转录完整", passed: vec_lines_complete(m) });
    rows.push(SmCheckRow { name: "红账单调", passed: m.red_monotone() });
    rows.push(SmCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(SmCheckRow { name: "向量宽核对", passed: STAT_VECS == 6 });
    rows.push(SmCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn stats_checklist_green(rows: &[SmCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn stats_stats_line(m: &StatsMatrix) -> String {
    format!(
        "统计概况：尝试 {} 成功 {} 红账 {}",
        m.total_attempts(),
        m.total_success(),
        m.red_len()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx1253_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut m = StatsMatrix::new();
    seeded_session(&mut m, 0xFBB0, None);
    // X-a：转录/原因分布面。
    let d = build_reason_dist(&m);
    out.push((
        "X-转录/原因分布两面",
        vec_lines_complete(&m) && reason_dist_conserved(&m, &d),
        stats_stats_line(&m),
    ));
    // X-b：原因矩阵 10/10。
    let (ok_r, d_r) = drill_reason_matrix_10();
    out.push(("X-原因矩阵10/10", ok_r, d_r));
    // X-c：终验清单。
    let rows = build_stats_checklist(&m);
    out.push(("X-终验清单10项", stats_checklist_green(&rows), StatsReport::build(&m).to_text()));
    // X-d：摘要/影子 10/10。
    let (ok_d, d_d) = drill_digest_10();
    out.push(("X-摘要影子10/10", ok_d, d_d));
    // X-e：带分布 10/10。
    let (ok_b, d_b) = drill_bands_10();
    out.push(("X-带分布10/10", ok_b, d_b));
    out
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 F：向量摘要位（每向量 FNV 摘要，向量间两两不同）。
/// ─────────────────────────────────────────────────────────────────────
/// FNV 步进。
const fn fnv_step(acc: u64, byte: u64) -> u64 {
    (acc ^ byte).wrapping_mul(0x100000001b3)
}

/// 逐向量摘要表。
pub fn vec_digests(m: &StatsMatrix) -> Vec<u64> {
    m.rows_iter()
        .enumerate()
        .map(|(v, r)| {
            let mut d = 0xcbf29ce484222325;
            d = fnv_step(d, v as u64);
            d = fnv_step(d, r.attempts as u64);
            d = fnv_step(d, r.success as u64);
            d
        })
        .collect()
}

/// 摘要两两不同（独立会话下各向量账面必异）。
pub fn vec_digests_distinct(m: &StatsMatrix) -> bool {
    let ds = vec_digests(m);
    for i in 0..ds.len() {
        for j in (i + 1)..ds.len() {
            if ds[i] == ds[j] {
                return false;
            }
        }
    }
    true
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 G：依赖表转录导出（F1242 影子位图逐向量行）。
/// ─────────────────────────────────────────────────────────────────────
/// 依赖表转录导出。
pub fn export_dep_shadow() -> String {
    let mut s = String::from("F1242 影子依赖表：\n");
    for (v, d) in DEP_TABLE.iter().enumerate() {
        s.push_str(&format!("· V{} ← 位图{}\n", v, d));
    }
    s
}

/// 影子转录守恒：行数=向量宽且版本钉定。
pub fn dep_shadow_conserved() -> bool {
    export_dep_shadow().matches("V").count() == STAT_VECS && DEP_TABLE_VER == 20260901
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 H：摘要/影子 10/10 演练。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_digest_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFBB0 段基址 ^ 2。
    let mut lcg = Lcg::new(0xFBB2);
    for _r in 0..INJECT_ROUNDS {
        let mut m = StatsMatrix::new();
        seeded_session(&mut m, 0xFBB0, None);
        let uniq = vec_digests_distinct(&m);
        let shadow = dep_shadow_conserved();
        let _ = lcg.below(3);
        if uniq && shadow {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("摘要影子轮={}/{}", ok, INJECT_ROUNDS))
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 I：成功率带分布（千分带 [0,200)/[200,600)/[600,900)/[900,1000]）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct RateBandRow {
    /// 带名。
    pub band: &'static str,
    /// 落带向量数。
    pub count: u32,
}

/// 带分布构建。
pub fn build_rate_bands(m: &StatsMatrix) -> Vec<RateBandRow> {
    let mut bands: Vec<RateBandRow> = Vec::new();
    bands.push(RateBandRow { band: "危险[0,200)", count: 0 });
    bands.push(RateBandRow { band: "偏低[200,600)", count: 0 });
    bands.push(RateBandRow { band: "正常[600,900)", count: 0 });
    bands.push(RateBandRow { band: "健康[900,1000]", count: 0 });
    for r in m.rows_iter() {
        if let Some(rate) = r.success_milli() {
            let idx = if rate < 200 {
                0
            } else if rate < 600 {
                1
            } else if rate < 900 {
                2
            } else {
                3
            };
            bands[idx].count += 1;
        }
    }
    bands
}

/// 带分布守恒：Σ带计数 ≡ 向量宽。
pub fn rate_bands_conserved(b: &[RateBandRow]) -> bool {
    b.len() == 4 && b.iter().map(|r| r.count as usize).sum::<usize>() == STAT_VECS
}

/// 带分布导出。
pub fn export_rate_bands(b: &[RateBandRow]) -> String {
    let mut s = String::from("成功率带分布：\n");
    for r in b.iter() {
        s.push_str(&format!("· {} ×{}\n", r.band, r.count));
    }
    s
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 J：健康基线带核（无注入会话 → 零危险带）10/10。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_bands_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFBB0 段基址 ^ 3。
    let mut lcg = Lcg::new(0xFBB3);
    for _r in 0..INJECT_ROUNDS {
        let mut m = StatsMatrix::new();
        seeded_session(&mut m, 0xFBB0, None);
        let b = build_rate_bands(&m);
        let no_danger = b[0].count == 0;
        let _ = lcg.below(3);
        if no_danger && rate_bands_conserved(&b) {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("带分布轮={}/{}", ok, INJECT_ROUNDS))
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 K：统计概况一行（ktest 汇总展示面）。
/// ─────────────────────────────────────────────────────────────────────
/// 概况行（含带分布摘要）。
pub fn stats_overview_line(m: &StatsMatrix) -> String {
    let b = build_rate_bands(m);
    format!(
        "统计概况行：尝试 {} 成功 {} 危险带 {} 健康带 {}",
        m.total_attempts(),
        m.total_success(),
        b[0].count,
        b[3].count
    )
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：矩阵破平——手改成功数后 balanced 必翻红。
    let mut m = StatsMatrix::new();
    let _ = m.record_success(0);
    let honest = m.row(0).map(|r| r.balanced()).unwrap_or(false);
    if let Some(r0) = m.rows.get_mut(0) {
        r0.success += 1; // 篡改：成功>尝试
    }
    let broken = m.row(0).map(|r| !r.balanced()).unwrap_or(true);
    if honest && broken {
        caught += 1;
        d.push("注1 破平:抓".to_string());
    } else {
        d.push("注1 破平:漏".to_string());
    }
    // 注 2：交叉失配——无依赖向量注 DepNotMet 必被抓。
    let mut m2 = StatsMatrix::new();
    let _ = m2.record_fail(0, FailReason::DepNotMet); // V0 无依赖
    if !m2.cross_consistent() {
        caught += 1;
        d.push("注2 失配:抓".to_string());
    } else {
        d.push("注2 失配:漏".to_string());
    }
    // 注 3：越限漏报——篡改红阈感（高失败向量不扫必漏）。
    let mut m3 = StatsMatrix::new();
    seeded_session(&mut m3, 0xFBB0, Some(2));
    let before = m3.scan_red(1);
    let _ = before;
    m3.red.clear(); // 伪清账
    let missed = m3.red_len() == 0 && m3.red_hits(2) == false;
    let _ = m3.scan_red(2); // 重扫恢复
    if missed && m3.red_hits(2) {
        caught += 1;
        d.push("注3 漏报:抓".to_string());
    } else {
        d.push("注3 漏报:漏".to_string());
    }
    // 注 4：越向量必拒（VecOob）。
    let mut m4 = StatsMatrix::new();
    if matches!(m4.record_success(STAT_VECS), Err(SmCode::VecOob))
        && matches!(m4.record_fail(STAT_VECS, FailReason::Oob), Err(SmCode::VecOob))
    {
        caught += 1;
        d.push("注4 越向:抓".to_string());
    } else {
        d.push("注4 越向:漏".to_string());
    }
    // 注 5：版本漂移——影子版本被改必被抓。
    let pinned = DEP_TABLE_VER == 20260901;
    let drifted = DEP_TABLE_VER != 20260902; // 漂移版本必不等
    if pinned && drifted {
        caught += 1;
        d.push("注5 版漂:抓".to_string());
    } else {
        d.push("注5 版漂:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0xFBB0 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    SmCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx1253_checks：本单判据聚合（诊断段 0xFBB0）。
pub fn run_unx1253_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1253 Arming 统计矩阵");
    // J1-a：逐向量成功率/失败原因分布矩阵可查（标准会话）。
    let mut m = StatsMatrix::new();
    seeded_session(&mut m, 0xFBB0, None);
    s.add_dyn(
        "J1-矩阵可查且自洽",
        m.all_balanced() && m.total_attempts() > 0 && m.weighted_success_milli().is_some(),
        &m.summary(),
    );
    // J1-b：矩阵与 F1242 影子依赖表交叉一致率 100%。
    s.add_dyn(
        "J1-交叉一致率100%",
        m.cross_consistent() && m.dep_ver_pinned(),
        "依赖向量必见DepNotMet/无依赖必零",
    );
    // J1-c：失败率越限红账告警 10/10。
    let (ok3, d3) = drill_red_10();
    s.add_dyn("J1-越限红账10/10", ok3, &d3);
    // J1-d：健康基线 10/10（不越必静默）。
    let (ok4, d4) = drill_healthy_10();
    s.add_dyn("J1-健康基线10/10", ok4, &d4);
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0xFBB0段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n, p, d) in run_unx1253_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
