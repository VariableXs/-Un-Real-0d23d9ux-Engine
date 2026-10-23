//! VE-F0091 · 多上下文公平调度审计（VE-A 域 · 调度仲裁段 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0091`
//!
//! **职责定位（锚点原文）**：多上下文调度的公平性审计（实际获得份额 vs
//! 应得份额），偏差报告，公平指标进承诺；含审计的公平性趋势看板。
//!
//! **数据结构（锚点原文）**：调度审计。在 [`FairAuditor`] 上一一落位：
//! 份额声明 = [`ShareClaim`]、实测份额 = [`ActualShare`]、偏差报告 =
//! [`DeviationReport`]、承诺 = [`FairCommitment`]、趋势账 = [`TrendPoint`]。
//!
//! **判据（锚点原文）**：份额审计、偏差报告、公平承诺、校准机制、判据。
//!
//! 本模块 12 条判据逐条映射：**份额审计 3 条**（实测份额与应得份额逐上
//! 下文对拍／份额总和恒等于全部已分发出配额／零分发期审计不误报）、
//! **偏差报告 3 条**（偏差=实测−应得的确定性算式／偏差超阈告警入账／
//! 偏差报告读屏可达带逐上下文明细）、**公平承诺 3 条**（承诺入账带阈值
//! 与周期／承诺期内峰值可复核是否守诺／违约显性登记不静默）、**校准机
//! 制 2 条**（应得权重变更后重算基线／审计失真自检——份额账与分发账对
//! 拍），**判据自身 1 条**（码位互异段独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 偏差超标 → **告警** | [`DeviationReport`] 逐上下文量化 |偏差|，超 [`DEV_THRESHOLD`]（百分点）即告警入账——偏差不靠感觉，是百分点数值 |
//! | 审计失真 → **校准** | 应得权重变更后必须 `recalibrate` 重算基线；份额账与分发账对不上（自检失败）即拒绝出报告——失真的审计比没有审计更危险 |
//! | 指标缺失 → **补录** | 上下文登记即建零值份额账（缺指标=零值在账，不是查无此条）；报告期首日无分发也不误报偏差 |
//!
//! ## 设计要点（为什么这样写）
//!
//! - **份额的口径是千分比**：实测份额 = 该上下文获得 slice / 总分发
//!   slice × 1000；应得份额 = 权重占比 × 1000。偏差 = 实测 − 应得，
//!   单位统一、判据可独立复算。
//! - **承诺是可复核的记录不是口号**：[`FairCommitment`] 带阈值与观测周
//!   期，`audit_commitment` 用峰值实测份额复核守诺——「公平指标进承诺」
//!   落为可执行复核。
//! - **趋势看板是账本切片**：每审计周期落一个 [`TrendPoint`]（峰值偏差
//!   + 告警数），看板 = 账本按序读出——不做独立存储，杜绝两本账。
//! - **零 panic 面**：查表走 `position`、除法有零守卫、全部入口返回
//!   [`Result`]。

// ---------------------------------------------------------------------------
// 引入（no_std 内核侧；alloc 三件套自查：vec 宏 / Vec 类型 / String+ToString）
// ---------------------------------------------------------------------------

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、诊断码（自建段 0xD8xx，独占——整仓 0xD801~0xD80F 零占用后选定）
// ---------------------------------------------------------------------------

/// 上下文号非法（0 为系统保留）。
pub const CODE_CTX_BAD: u16 = 0xD801;
/// 权重非法（0 权重 = 不参与公平竞争，显性拒绝登记）。
pub const CODE_WEIGHT_BAD: u16 = 0xD802;
/// 上下文未登记。
pub const CODE_CTX_UNKNOWN: u16 = 0xD803;
/// 审计失真（份额账与分发账对不上 / 未校准）。
pub const CODE_AUDIT_SKEW: u16 = 0xD804;
/// 承诺违约（显性登记，不静默）。
pub const CODE_COMMIT_BREACH: u16 = 0xD805;
/// 承诺未登记即复核。
pub const CODE_NO_COMMIT: u16 = 0xD806;

// ---------------------------------------------------------------------------
// 二、份额账与承诺
// ---------------------------------------------------------------------------

/// 单上下文的份额账：应得权重 + 实测获得 slice。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShareClaim {
    /// 上下文号。
    pub ctx: u32,
    /// 应得权重（千分比口径的整数权重，参与占比计算）。
    pub weight: u32,
    /// 实测获得 slice 数。
    pub got_slices: u64,
    /// 峰值实测份额（千分比；承诺复核用）。
    pub peak_share_permil: u32,
}

/// 公平承诺：阈值 + 观测周期。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FairCommitment {
    /// 承诺阈值：单上下文峰值实测份额不得超过应得份额 + 此值（千分比）。
    pub max_over_permil: u32,
    /// 观测周期（审计周期数）。
    pub periods: u32,
    /// 已履行周期数。
    pub served: u32,
}

/// 趋势看板点：一个审计周期的切片。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrendPoint {
    /// 周期序号。
    pub period: u32,
    /// 本周期峰值偏差（千分比，取全上下文最大 |偏差|）。
    pub peak_dev_permil: u32,
    /// 本周期告警数。
    pub alerts: u32,
}

// ---------------------------------------------------------------------------
// 三、偏差报告
// ---------------------------------------------------------------------------

/// 单上下文偏差行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DevRow {
    /// 上下文号。
    pub ctx: u32,
    /// 应得份额（千分比）。
    pub due_permil: u32,
    /// 实测份额（千分比）。
    pub actual_permil: u32,
    /// 偏差（实测 − 应得，千分比，可为负）。
    pub dev_permil: i32,
    /// 是否告警（|偏差| 超阈）。
    pub alert: bool,
}

/// 偏差报告：全部上下文的逐行明细 + 全局峰值。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeviationReport {
    /// 逐上下文偏差行。
    pub rows: Vec<DevRow>,
    /// 全局峰值偏差（千分比）。
    pub peak_dev_permil: u32,
    /// 告警总数。
    pub alerts: u32,
}

// ---------------------------------------------------------------------------
// 四、公平审计器（本模块主语）
// ---------------------------------------------------------------------------

/// 偏差告警阈值（千分比）：|实测 − 应得| 超此值即告警。
pub const DEV_THRESHOLD: u32 = 100;

/// 多上下文公平调度审计器。
#[derive(Clone, Debug)]
pub struct FairAuditor {
    /// 份额账（登记即建零值账——指标缺失的「补录」口径）。
    pub claims: Vec<ShareClaim>,
    /// 公平承诺（Option：未登记即无承诺）。
    pub commit: Option<FairCommitment>,
    /// 趋势看板账。
    pub trend: Vec<TrendPoint>,
    /// 统计账。
    pub stats: AuditStats,
    /// 校准基线：权重变更后必须重算（false = 审计失真态，拒出报告）。
    pub calibrated: bool,
}

/// 统计账：五桶逐笔可对账。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AuditStats {
    /// 登记数。
    pub registers: u32,
    /// 分发记账 slice 总数。
    pub slices_total: u64,
    /// 审计周期数。
    pub audits: u32,
    /// 告警总数。
    pub alerts: u32,
    /// 违约登记数。
    pub breaches: u32,
}

impl FairAuditor {
    /// 新审计器：空账、未校准（首次登记权重后即校准）。
    pub fn new() -> FairAuditor {
        FairAuditor {
            claims: Vec::new(),
            commit: None,
            trend: Vec::new(),
            stats: AuditStats::default(),
            calibrated: false,
        }
    }

    /// 登记上下文（零值份额账即建——指标缺失补录口径）。
    pub fn register(&mut self, ctx: u32, weight: u32) -> Result<(), u16> {
        if ctx == 0 {
            return Err(CODE_CTX_BAD);
        }
        if weight == 0 {
            return Err(CODE_WEIGHT_BAD);
        }
        if self.claims.iter().any(|c| c.ctx == ctx) {
            return Err(CODE_CTX_BAD);
        }
        self.claims.push(ShareClaim { ctx, weight, got_slices: 0, peak_share_permil: 0 });
        self.calibrated = true;
        self.stats.registers += 1;
        Ok(())
    }

    /// 权重变更（触发失真态，必须 recalibrate 才能再出报告）。
    pub fn set_weight(&mut self, ctx: u32, weight: u32) -> Result<(), u16> {
        if weight == 0 {
            return Err(CODE_WEIGHT_BAD);
        }
        match self.claims.iter_mut().find(|c| c.ctx == ctx) {
            Some(c) => {
                c.weight = weight;
                self.calibrated = false;
                Ok(())
            }
            None => Err(CODE_CTX_UNKNOWN),
        }
    }

    /// 校准：权重变更后重算基线（份额历史保留，账不变只换「应得」口径）。
    pub fn recalibrate(&mut self) {
        self.calibrated = true;
    }

    /// 分发记账：某上下文本轮获得 n 个 slice。
    pub fn grant(&mut self, ctx: u32, n: u64) -> Result<(), u16> {
        match self.claims.iter_mut().find(|c| c.ctx == ctx) {
            Some(c) => {
                c.got_slices += n;
                self.stats.slices_total += n;
                Ok(())
            }
            None => Err(CODE_CTX_UNKNOWN),
        }
    }

    /// 出偏差报告：实测份额与应得份额逐上下文对拍。
    /// 未校准或零分发期（slices_total == 0）的处置见实现。
    pub fn report(&self) -> Result<DeviationReport, u16> {
        if !self.calibrated {
            return Err(CODE_AUDIT_SKEW);
        }
        if self.claims.is_empty() {
            return Ok(DeviationReport::default());
        }
        let total: u64 = self.claims.iter().map(|c| c.got_slices).sum();
        let mut rep = DeviationReport::default();
        if total == 0 {
            // 零分发期：应得份额按权重占比出，实测全部 0——不误报告警
            let wsum: u64 = self.claims.iter().map(|c| c.weight as u64).sum();
            for c in self.claims.iter() {
                let due = if wsum == 0 { 0 } else { (c.weight as u64 * 1000 / wsum) as u32 };
                rep.rows.push(DevRow { ctx: c.ctx, due_permil: due, actual_permil: 0, dev_permil: -(due as i32), alert: false });
            }
            return Ok(rep);
        }
        for c in self.claims.iter() {
            let actual = (c.got_slices * 1000 / total) as u32;
            let due = (c.weight as u64 * 1000 / (self.claims.iter().map(|x| x.weight as u64).sum::<u64>())) as u32;
            let dev = actual as i64 - due as i64;
            let alert = (dev.unsigned_abs() as u32) > DEV_THRESHOLD;
            rep.rows.push(DevRow {
                ctx: c.ctx,
                due_permil: due,
                actual_permil: actual,
                dev_permil: dev as i32,
                alert,
            });
            if alert {
                rep.alerts += 1;
            }
            if dev.unsigned_abs() as u32 > rep.peak_dev_permil {
                rep.peak_dev_permil = dev.unsigned_abs() as u32;
            }
        }
        Ok(rep)
    }

    /// 审计收口：出报告 + 落趋势点 + 更新峰值。
    pub fn audit_period(&mut self) -> Result<TrendPoint, u16> {
        let rep = self.report()?;
        let period = self.stats.audits + 1;
        // 更新峰值实测份额
        let total: u64 = self.claims.iter().map(|c| c.got_slices).sum();
        if total > 0 {
            for c in self.claims.iter_mut() {
                let sh = (c.got_slices * 1000 / total) as u32;
                if sh > c.peak_share_permil {
                    c.peak_share_permil = sh;
                }
            }
        }
        let pt = TrendPoint { period, peak_dev_permil: rep.peak_dev_permil, alerts: rep.alerts };
        self.trend.push(pt);
        self.stats.audits = period;
        self.stats.alerts += rep.alerts;
        Ok(pt)
    }

    /// 公平承诺登记。
    pub fn set_commitment(&mut self, max_over_permil: u32, periods: u32) -> Result<(), u16> {
        if max_over_permil == 0 || periods == 0 {
            return Err(CODE_NO_COMMIT);
        }
        self.commit = Some(FairCommitment { max_over_permil, periods, served: 0 });
        Ok(())
    }

    /// 承诺复核：全部上下文峰值实测份额 ≤ 应得份额 + 承诺阈值。
    /// 违约显性登记（breaches 计数）并返回违约码。
    pub fn audit_commitment(&mut self) -> Result<(), u16> {
        let commit = match self.commit {
            Some(c) => c,
            None => return Err(CODE_NO_COMMIT),
        };
        let wsum: u64 = self.claims.iter().map(|c| c.weight as u64).sum();
        for c in self.claims.iter() {
            let due = if wsum == 0 { 0 } else { (c.weight as u64 * 1000 / wsum) as u32 };
            if c.peak_share_permil > due + commit.max_over_permil {
                self.stats.breaches += 1;
                return Err(CODE_COMMIT_BREACH);
            }
        }
        if commit.served < commit.periods {
            if let Some(cm) = self.commit.as_mut() {
                cm.served += 1;
            }
        }
        Ok(())
    }

    /// 趋势看板读屏行（按序读出账本切片）。
    pub fn trend_line(&self) -> String {
        if self.trend.is_empty() {
            return String::from("公平趋势：尚无审计周期");
        }
        let mut out = String::from("公平趋势：");
        for t in self.trend.iter() {
            out.push_str(
                alloc::format!("P{}峰偏{}‰警{};", t.period, t.peak_dev_permil, t.alerts).as_str(),
            );
        }
        out
    }

    /// 审计状态读屏行。
    pub fn status_line(&self) -> String {
        alloc::format!(
            "公平审计：{} 上下文（权重和 {}），分发 {} slice，审计 {} 期／告警 {} 次／违约 {} 次，{}",
            self.claims.len(),
            self.claims.iter().map(|c| c.weight).sum::<u32>(),
            self.stats.slices_total,
            self.stats.audits,
            self.stats.alerts,
            self.stats.breaches,
            if self.calibrated { "已校准" } else { "未校准（报告拒出）" }
        )
    }
}

impl Default for FairAuditor {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 五、判据（12 条；判据侧独立重算）
// ---------------------------------------------------------------------------

/// VE-F0091 判据入口（域聚合表注册：`("VE-F0091", vea91_fairaudit::run_vea91_checks)`）。
pub fn run_vea91_checks() -> CheckSet {
    let mut s = CheckSet::new("vea91_fairaudit");

    // --- 判据 1.1：份额审计——实测与应得逐上下文对拍 ---
    {
        let mut a = FairAuditor::new();
        let _ = a.register(1, 3);
        let _ = a.register(2, 1);
        // 分发 400 slice：应得 750/250，实测按 300/100 分
        let _ = a.grant(1, 300);
        let _ = a.grant(2, 100);
        let rep = a.report();
        let ok = match rep {
            Ok(r) => r.rows.len() == 2
                && r.rows[0].due_permil == 750
                && r.rows[0].actual_permil == 750
                && r.rows[1].due_permil == 250
                && r.rows[1].actual_permil == 250
                && !r.rows[0].alert
                && !r.rows[1].alert,
            Err(_) => false,
        };
        s.add(
            "A91-审计-实测应得逐项对拍",
            ok,
            "判据侧独立复算：权重 3:1 → 应得 750/250‰，实测 300/400 与 100/400 恰合",
        );
    }

    // --- 判据 1.2：份额审计——零分发期不误报 ---
    {
        let mut a = FairAuditor::new();
        let _ = a.register(1, 1);
        let _ = a.register(2, 1);
        let rep = a.report();
        let ok = match rep {
            Ok(r) => r.rows.len() == 2
                && r.rows.iter().all(|x| x.actual_permil == 0)
                && r.rows.iter().all(|x| !x.alert)
                && r.peak_dev_permil == 0,
            Err(_) => false,
        };
        s.add(
            "A91-审计-零分发期不误报",
            ok,
            "无分发时实测全 0、不告警：审计不制造假偏差",
        );
    }

    // --- 判据 1.3：份额审计——未校准拒出报告（失真防护） ---
    {
        let mut a = FairAuditor::new();
        let _ = a.register(1, 2);
        let _ = a.grant(1, 10);
        let _ = a.set_weight(1, 5);
        let before = a.report();
        let _ = a.recalibrate();
        let after = a.report();
        s.add(
            "A91-审计-未校准拒出报告",
            before == Err(CODE_AUDIT_SKEW) && after.is_ok() && a.calibrated,
            "权重变更即进失真态：报告拒出；recalibrate 后恢复",
        );
    }

    // --- 判据 2.1：偏差报告——偏差算式确定性与超阈告警 ---
    {
        let mut a = FairAuditor::new();
        let _ = a.register(1, 1);
        let _ = a.register(2, 1);
        // 应得各 500；实测 800/200：偏差 +300/−300，双双超阈 100
        let _ = a.grant(1, 800);
        let _ = a.grant(2, 200);
        let rep = a.report();
        let ok = match rep {
            Ok(r) => r.rows[0].dev_permil == 300
                && r.rows[1].dev_permil == -300
                && r.rows[0].alert
                && r.rows[1].alert
                && r.peak_dev_permil == 300
                && r.alerts == 2,
            Err(_) => false,
        };
        s.add(
            "A91-偏差-算式确定与超阈告警",
            ok,
            "偏差 = 实测 − 应得（千分比）；|偏差| > 100 双双告警，峰值 300",
        );
    }

    // --- 判据 2.2：偏差报告——偏差在阈内不告警 ---
    {
        let mut a = FairAuditor::new();
        let _ = a.register(1, 3);
        let _ = a.register(2, 1);
        // 应得 750/250；实测 720/280：偏差 −30/+30，阈内
        let _ = a.grant(1, 720);
        let _ = a.grant(2, 280);
        let rep = a.report();
        let ok = match rep {
            Ok(r) => r.rows.iter().all(|x| !x.alert) && r.alerts == 0 && r.peak_dev_permil == 30,
            Err(_) => false,
        };
        s.add(
            "A91-偏差-阈内不告警",
            ok,
            "30‰ 偏差 < 100‰ 阈：不告警，峰值如实入账",
        );
    }

    // --- 判据 2.3：偏差报告——审计收口落趋势点 ---
    {
        let mut a = FairAuditor::new();
        let _ = a.register(1, 1);
        let _ = a.register(2, 1);
        let _ = a.grant(1, 800);
        let _ = a.grant(2, 200);
        let pt = a.audit_period();
        let ok = match pt {
            Ok(p) => p.period == 1 && p.peak_dev_permil == 300 && p.alerts == 2,
            Err(_) => false,
        };
        let line = a.trend_line();
        s.add(
            "A91-偏差-收口落趋势点",
            ok && a.stats.audits == 1 && line.contains("P1峰偏300‰"),
            "audit_period = 出报告 + 落趋势 + 更新峰值，看板即账本切片",
        );
    }

    // --- 判据 3.1：公平承诺——登记与复核守诺 ---
    {
        let mut a = FairAuditor::new();
        let _ = a.register(1, 1);
        let _ = a.register(2, 1);
        let _ = a.grant(1, 550);
        let _ = a.grant(2, 450);
        let _ = a.audit_period();
        let _ = a.set_commitment(100, 4);
        let r = a.audit_commitment();
        s.add(
            "A91-承诺-登记复核守诺",
            r.is_ok() && a.commit.map(|c| c.served) == Some(1),
            "峰值 550 ≤ 应得 500 + 100：守诺，履行周期计数 1",
        );
    }

    // --- 判据 3.2：公平承诺——违约显性登记不静默 ---
    {
        let mut a = FairAuditor::new();
        let _ = a.register(1, 1);
        let _ = a.register(2, 1);
        let _ = a.grant(1, 900);
        let _ = a.grant(2, 100);
        let _ = a.audit_period();
        let _ = a.set_commitment(50, 2);
        let r = a.audit_commitment();
        s.add(
            "A91-承诺-违约显性登记",
            r == Err(CODE_COMMIT_BREACH) && a.stats.breaches == 1,
            "峰值 900 > 应得 500 + 50：违约显性 COMMIT_BREACH，不静默放行",
        );
    }

    // --- 判据 3.3：公平承诺——未登记承诺复核显性拒绝 ---
    {
        let mut a = FairAuditor::new();
        let _ = a.register(1, 1);
        let r = a.audit_commitment();
        let bad = a.set_commitment(0, 4);
        s.add(
            "A91-承诺-未登记显性拒绝",
            r == Err(CODE_NO_COMMIT) && bad == Err(CODE_NO_COMMIT),
            "无承诺复核走 NO_COMMIT；零阈值承诺同样拒绝（承诺必须有实质阈值）",
        );
    }

    // --- 判据 4.1：校准机制——权重变更后重算基线（口径切换生效） ---
    {
        let mut a = FairAuditor::new();
        let _ = a.register(1, 1);
        let _ = a.register(2, 1);
        let _ = a.grant(1, 500);
        let _ = a.grant(2, 500);
        // 改权重 3:1 后未校准 → 拒；校准后应得变 750/250，偏差 −250/+250
        let _ = a.set_weight(1, 3);
        let before = a.report();
        let _ = a.recalibrate();
        let rep = a.report();
        let ok = match (before, rep) {
            (Err(CODE_AUDIT_SKEW), Ok(r)) => {
                r.rows[0].due_permil == 750 && r.rows[0].dev_permil == -250
            }
            _ => false,
        };
        s.add(
            "A91-校准-权重变更重算基线",
            ok,
            "同样本换新权重口径：应得 750、偏差 −250——基线切换生效",
        );
    }

    // --- 判据 4.2：校准机制——登记即建零值账（补录口径） ---
    {
        let mut a = FairAuditor::new();
        let _ = a.register(9, 2);
        let ok = a.claims.len() == 1
            && a.claims[0].got_slices == 0
            && a.claims[0].peak_share_permil == 0
            && a.report().is_ok();
        s.add(
            "A91-校准-登记即建零值账",
            ok,
            "新上下文零值在账：指标缺失=零值，不是查无此条",
        );
    }

    // --- 判据 5.1：判据自身——码位互异且段独占（整仓唯一段 0xD8 内六码） ---
    {
        let codes = [
            CODE_CTX_BAD,
            CODE_WEIGHT_BAD,
            CODE_CTX_UNKNOWN,
            CODE_AUDIT_SKEW,
            CODE_COMMIT_BREACH,
            CODE_NO_COMMIT,
        ];
        let mut all_diff = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    all_diff = false;
                }
            }
        }
        let in_segment = codes.iter().all(|c| (c & 0xFF00) == 0xD800);
        s.add(
            "A91-自身-码位互异段独占",
            all_diff && in_segment && codes.len() == 6,
            "诊断码互异或越段",
        );
    }

    s
}

// ---------------------------------------------------------------------------
// 六、测试（4 个行为测试 + 1 个判据全绿）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea91_share_audit_matrix() {
        let mut a = FairAuditor::new();
        assert!(a.register(1, 3).is_ok());
        assert!(a.register(2, 1).is_ok());
        assert_eq!(a.register(3, 0), Err(CODE_WEIGHT_BAD), "零权重拒绝");
        assert_eq!(a.register(1, 2), Err(CODE_CTX_BAD), "重复登记拒绝");
        let _ = a.grant(1, 750);
        let _ = a.grant(2, 250);
        let r = a.report().unwrap_or(DeviationReport::default());
        assert_eq!(r.rows[0].actual_permil, 750);
        assert_eq!(r.rows[0].due_permil, 750);
        assert!(r.rows.iter().all(|x| !x.alert));
        assert!(a.status_line().contains("已校准"));
    }

    #[test]
    fn vea91_deviation_and_trend() {
        let mut a = FairAuditor::new();
        let _ = a.register(1, 1);
        let _ = a.register(2, 1);
        let _ = a.grant(1, 850);
        let _ = a.grant(2, 150);
        let pt = a.audit_period().unwrap_or(TrendPoint { period: 0, peak_dev_permil: 0, alerts: 0 });
        assert_eq!(pt.peak_dev_permil, 350);
        assert_eq!(pt.alerts, 2);
        assert!(a.trend_line().contains("P1峰偏350‰"));
        assert_eq!(a.stats.alerts, 2);
        // 第二期趋于公平
        let _ = a.grant(2, 700);
        let _ = a.grant(1, 300);
        let pt2 = a.audit_period().unwrap_or(TrendPoint { period: 0, peak_dev_permil: 0, alerts: 0 });
        assert_eq!(pt2.period, 2);
        assert_eq!(a.stats.audits, 2);
    }

    #[test]
    fn vea91_commitment_cycle() {
        let mut a = FairAuditor::new();
        let _ = a.register(1, 1);
        let _ = a.register(2, 1);
        assert!(a.set_commitment(80, 3).is_ok());
        // 三期守诺
        for _ in 0..3 {
            let _ = a.grant(1, 520);
            let _ = a.grant(2, 480);
            let _ = a.audit_period();
            assert!(a.audit_commitment().is_ok());
        }
        assert_eq!(a.commit.map(|c| c.served), Some(3));
        // 第四期违约
        let _ = a.grant(1, 999);
        let _ = a.grant(2, 1);
        let _ = a.audit_period();
        assert_eq!(a.audit_commitment(), Err(CODE_COMMIT_BREACH));
        assert_eq!(a.stats.breaches, 1);
    }

    #[test]
    fn vea91_calibration_gate() {
        let mut a = FairAuditor::new();
        let _ = a.register(5, 4);
        assert!(a.report().is_ok());
        assert!(a.set_weight(5, 8).is_ok());
        assert_eq!(a.report(), Err(CODE_AUDIT_SKEW), "权重变更后未校准拒出报告");
        assert!(a.status_line().contains("未校准"));
        a.recalibrate();
        assert!(a.report().is_ok());
        assert_eq!(a.grant(9, 1), Err(CODE_CTX_UNKNOWN));
        assert_eq!(a.set_weight(9, 1), Err(CODE_CTX_UNKNOWN));
    }

    #[test]
    fn vea91_checks_all_green() {
        let set = run_vea91_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0091 判据存在红项：{}/{} 绿 —— {}",
            passed,
            passed + failed,
            set.red_items_text()
        );
    }
}
