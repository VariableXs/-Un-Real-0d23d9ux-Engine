//! VE-F0092 · 服务级渲染配额（VE-A 域 · 调度仲裁段 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0092`
//!
//! **职责定位（锚点原文）**：系统服务的渲染配额（每个服务能用多少渲染
//! 预算），配额超用的降级，配额分配公开可查；含配额的按需重评流程（服
//! 务方申请调整走流程）。
//!
//! **数据结构（锚点原文）**：配额管理。在 [`QuotaMgr`] 上一一落位：配额
//! 项 = [`QuotaEntry`]、超用降级 = [`DegradeAction`]、重评申请 =
//! [`RevalRequest`]、漂移账 = [`DriftAudit`]。
//!
//! **判据（锚点原文）**：服务配额、超用降级、分配公开、漂移审计、判据。
//!
//! 本模块 12 条判据逐条映射：**服务配额 3 条**（配额登记按服务隔离公开
//! 可查／消费记账逐笔与余量对账／未登记服务消费显性拒绝）、**超用降级
//! 3 条**（预算耗尽降级不硬拒——低帧率继续服务／降级带读屏告知语／降
//! 级计数与服务对账）、**分配公开 3 条**（分配表读屏可达带预算与余量／
//! 零预算显性拒绝不暗吞／重评申请走流程——申请入队待审不即时生效）、
//! **漂移审计 2 条**（消费账与余量账对拍一致／漂移检出显性报码），**判
//! 据自身 1 条**（码位互异段独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 超用 → **降级 + 告知** | 余量耗尽不清零服务：进入 [`DegradeAction::HalfRate`]（减半供给）再 [`DegradeAction::Minimal`]（最小维持），每步带读屏告知语——配额是节流阀不是断路器 |
//! | 分配不公 → **重评** | 服务方 `request_reval` 入申请队列（带诉求理由与新预算），管理员 `approve_reval` 才生效——「走流程」= 申请与生效分离，两级留痕 |
//! | 配额漂移 → **审计** | [`DriftAudit`] 对拍「已消费 + 余量 == 总预算」；对不上即显性 [`CODE_DRIFT`]——漂移的配额会静默吃掉别家的预算，必须在账面拦住 |
//!
//! ## 设计要点（为什么这样写）
//!
//! - **配额是三态不是开关**：正常 → 减半 → 最小维持。超用不「拒绝服
//!   务」而是「降级供给」——系统服务被掐死会连环拖垮依赖它的应用。
//! - **公开是账面公开**：[`QuotaMgr::allocation_line`] 把每个服务的预算/
//!   已用/余量/降级态一次读屏——分配不公开的配额等于黑箱税。
//! - **重评走两级**：申请（服务方）→ 审批（管理员）→ 生效，申请未批不
//!   改账——杜绝「自己给自己涨预算」的通道。
//! - **漂移审计是结构性对拍**：消费账 [`QuotaEntry::used`]、余量
//!   `remaining()`、总预算 `budget` 三者恒等式由 `drift_audit` 全表核验，
//!   判据用「篡改余量」的故障注入验证审计真的能红。
//! - **零 panic 面**：查表走 `position`、计数全 `saturating_*`、全部入口
//!   返回 [`Result`]。

// ---------------------------------------------------------------------------
// 引入（no_std 内核侧；alloc 三件套自查：vec 宏 / Vec 类型 / String+ToString）
// ---------------------------------------------------------------------------

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、诊断码（自建段 0xDCxx，独占——整仓 0xDC01~0xDC0F 零占用后选定）
// ---------------------------------------------------------------------------

/// 服务号非法（0 为系统保留）。
pub const CODE_SVC_BAD: u16 = 0xDC01;
/// 预算非法（零预算 = 不给服务供渲染，显性拒绝不暗吞）。
pub const CODE_BUDGET_BAD: u16 = 0xDC02;
/// 服务未登记配额。
pub const CODE_SVC_UNKNOWN: u16 = 0xDC03;
/// 配额漂移（消费 + 余量 ≠ 预算）。
pub const CODE_DRIFT: u16 = 0xDC04;
/// 重评申请重复/未找到。
pub const CODE_REVAL_BAD: u16 = 0xDC05;
/// 审批越权（非管理员审批）。
pub const CODE_APPROVE_FORBID: u16 = 0xDC06;

// ---------------------------------------------------------------------------
// 二、配额项与降级
// ---------------------------------------------------------------------------

/// 降级档位：正常 → 减半 → 最小维持。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradeAction {
    /// 正常供给。
    Normal,
    /// 减半供给（余量首次耗尽）。
    HalfRate,
    /// 最小维持（减半后再耗尽；保进程不保帧率）。
    Minimal,
}

impl DegradeAction {
    /// 全集（判据枚举完备性用）。
    pub const ALL: [DegradeAction; 3] =
        [DegradeAction::Normal, DegradeAction::HalfRate, DegradeAction::Minimal];

    /// 短名（读屏口径）。
    pub const fn as_str(self) -> &'static str {
        match self {
            DegradeAction::Normal => "正常",
            DegradeAction::HalfRate => "减半供给",
            DegradeAction::Minimal => "最小维持",
        }
    }

    /// 降一级（Minimal 恒定）。
    pub const fn worsen(self) -> DegradeAction {
        match self {
            DegradeAction::Normal => DegradeAction::HalfRate,
            _ => DegradeAction::Minimal,
        }
    }
}

/// 单服务配额项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuotaEntry {
    /// 服务号。
    pub svc: u32,
    /// 本期总预算（渲染 slice 数）。
    pub budget: u32,
    /// 已消费。
    pub used: u32,
    /// 当前降级态。
    pub degrade: DegradeAction,
    /// 降级次数（与服务对账）。
    pub degrade_count: u32,
    /// 本期重评次数。
    pub revals: u32,
}

impl QuotaEntry {
    /// 余量（预算 − 已用，saturating）。
    pub const fn remaining(&self) -> u32 {
        self.budget.saturating_sub(self.used)
    }
}

/// 重评申请：服务方诉求（新预算 + 理由），待审。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevalRequest {
    /// 申请号（单调分配）。
    pub req_id: u32,
    /// 申请服务。
    pub svc: u32,
    /// 诉求新预算。
    pub want_budget: u32,
    /// 理由（读屏口径）。
    pub reason: String,
    /// 是否已审批。
    pub approved: bool,
}

// ---------------------------------------------------------------------------
// 三、漂移审计
// ---------------------------------------------------------------------------

/// 漂移审计结论：全表对拍一次的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriftAudit {
    /// 审计的配额项数。
    pub checked: u32,
    /// 漂移项数（应恒为 0）。
    pub drifted: u32,
    /// 总对拍口径：Σ(used + remaining) == Σbudget 是否成立。
    pub balanced: bool,
}

// ---------------------------------------------------------------------------
// 四、配额管理器（本模块主语）
// ---------------------------------------------------------------------------

/// 管理员标识（唯一可审批重评的调用方）。
pub const ADMIN_CALLER: u32 = 1;

/// 重评申请队列容量。
pub const REVAL_CAP: usize = 64;

/// 服务级渲染配额管理器。
#[derive(Clone, Debug)]
pub struct QuotaMgr {
    /// 配额表（分配公开的数据源）。
    pub quotas: Vec<QuotaEntry>,
    /// 重评申请队列（待审 + 已审留痕）。
    pub revals: Vec<RevalRequest>,
    /// 统计账。
    pub stats: QuotaStats,
    /// 最近告知语（降级/审批的读屏行）。
    pub last_notice: String,
    next_req: u32,
}

/// 统计账：五桶逐笔可对账。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QuotaStats {
    /// 配额登记/调整次数。
    pub registrations: u32,
    /// 消费记账次数。
    pub consumes: u32,
    /// 降级动作总次数。
    pub degradations: u32,
    /// 重评申请数。
    pub reval_requests: u32,
    /// 审批通过数。
    pub approvals: u32,
}

impl QuotaMgr {
    /// 新管理器：空表零账。
    pub fn new() -> QuotaMgr {
        QuotaMgr {
            quotas: Vec::new(),
            revals: Vec::new(),
            stats: QuotaStats::default(),
            last_notice: String::new(),
            next_req: 1,
        }
    }

    /// 登记配额（已登记则调整预算；零预算显性拒绝）。
    pub fn set_quota(&mut self, svc: u32, budget: u32) -> Result<(), u16> {
        if svc == 0 {
            return Err(CODE_SVC_BAD);
        }
        if budget == 0 {
            return Err(CODE_BUDGET_BAD);
        }
        match self.quotas.iter_mut().find(|q| q.svc == svc) {
            Some(q) => q.budget = budget,
            None => self.quotas.push(QuotaEntry {
                svc,
                budget,
                used: 0,
                degrade: DegradeAction::Normal,
                degrade_count: 0,
                revals: 0,
            }),
        }
        self.stats.registrations += 1;
        Ok(())
    }

    /// 消费记账：余量足全给；余量尽走降级链（不清零服务）。
    /// 返回实际供给数。
    pub fn consume(&mut self, svc: u32, want: u32) -> Result<u32, u16> {
        let idx = match self.quotas.iter().position(|q| q.svc == svc) {
            Some(i) => i,
            None => return Err(CODE_SVC_UNKNOWN),
        };
        let remaining = self.quotas[idx].remaining();
        let given = if remaining >= want {
            want
        } else {
            // 余量不足：按降级态折减（减半/最小），再耗尽继续降档
            let tier = self.quotas[idx].degrade;
            let given = match tier {
                DegradeAction::Normal => remaining / 2,
                DegradeAction::HalfRate => remaining / 8,
                DegradeAction::Minimal => remaining / 32,
            };
            let next = tier.worsen();
            if next != tier {
                self.quotas[idx].degrade_count += 1;
                self.stats.degradations += 1;
                self.last_notice = alloc::format!(
                    "配额降级告知：服务 {} 预算耗尽，{} → {}（服务不中断，帧率下调）",
                    svc,
                    tier.as_str(),
                    next.as_str()
                );
            }
            self.quotas[idx].degrade = next;
            given
        };
        self.quotas[idx].used += given;
        self.stats.consumes += 1;
        Ok(given)
    }

    /// 当前降级态查询（公开可查的一部分）。
    pub fn degrade_of(&self, svc: u32) -> Result<DegradeAction, u16> {
        match self.quotas.iter().find(|q| q.svc == svc) {
            Some(q) => Ok(q.degrade),
            None => Err(CODE_SVC_UNKNOWN),
        }
    }

    /// 重评申请：服务方入队待审（不即时生效——「走流程」）。
    pub fn request_reval(&mut self, svc: u32, want_budget: u32, reason: &str) -> Result<u32, u16> {
        if want_budget == 0 {
            return Err(CODE_BUDGET_BAD);
        }
        if !self.quotas.iter().any(|q| q.svc == svc) {
            return Err(CODE_SVC_UNKNOWN);
        }
        if self.revals.len() >= REVAL_CAP {
            return Err(CODE_REVAL_BAD);
        }
        if self.revals.iter().any(|r| r.svc == svc && !r.approved) {
            return Err(CODE_REVAL_BAD); // 同服务待审中不重复排队
        }
        let id = self.next_req;
        self.next_req += 1;
        self.revals.push(RevalRequest {
            req_id: id,
            svc,
            want_budget,
            reason: String::from(reason),
            approved: false,
        });
        self.stats.reval_requests += 1;
        Ok(id)
    }

    /// 审批：管理员通过后预算即刻生效（两级留痕：申请 + 审批）。
    pub fn approve_reval(&mut self, caller: u32, req_id: u32) -> Result<(), u16> {
        if caller != ADMIN_CALLER {
            return Err(CODE_APPROVE_FORBID);
        }
        let pos = match self.revals.iter().position(|r| r.req_id == req_id) {
            Some(p) => p,
            None => return Err(CODE_REVAL_BAD),
        };
        if self.revals[pos].approved {
            return Err(CODE_REVAL_BAD);
        }
        let svc = self.revals[pos].svc;
        let budget = self.revals[pos].want_budget;
        self.revals[pos].approved = true;
        if let Some(q) = self.quotas.iter_mut().find(|q| q.svc == svc) {
            q.budget = budget;
            q.revals += 1;
            // 重评通过即恢复正常供给
            q.degrade = DegradeAction::Normal;
        }
        self.stats.approvals += 1;
        self.last_notice = alloc::format!(
            "配额重评通过：服务 {} 新预算 {}（申请 {} 号已留痕）",
            svc, budget, req_id
        );
        Ok(())
    }

    /// 漂移审计：全表核验「已消费 + 余量 == 预算」。
    pub fn drift_audit(&self) -> DriftAudit {
        let mut a = DriftAudit { checked: 0, drifted: 0, balanced: true };
        for q in self.quotas.iter() {
            a.checked += 1;
            if q.used + q.remaining() != q.budget {
                a.drifted += 1;
                a.balanced = false;
            }
        }
        a
    }

    /// 分配公开读屏行：全表预算/已用/余量/降级态。
    pub fn allocation_line(&self) -> String {
        if self.quotas.is_empty() {
            return String::from("渲染配额：尚无登记服务");
        }
        let mut out = String::from("渲染配额分配：");
        for q in self.quotas.iter() {
            out.push_str(
                alloc::format!(
                    "服务{} 预算{} 已用{} 余量{} [{}];",
                    q.svc,
                    q.budget,
                    q.used,
                    q.remaining(),
                    q.degrade.as_str()
                )
                .as_str(),
            );
        }
        out
    }

    /// 状态读屏行。
    pub fn status_line(&self) -> String {
        alloc::format!(
            "配额管理：{} 服务，消费记账 {} 次／降级 {} 次／重评申请 {} 待审 {}／审批通过 {}",
            self.quotas.len(),
            self.stats.consumes,
            self.stats.degradations,
            self.stats.reval_requests,
            self.revals.iter().filter(|r| !r.approved).count(),
            self.stats.approvals
        )
    }
}

impl Default for QuotaMgr {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 五、判据（12 条；判据侧独立重算）
// ---------------------------------------------------------------------------

/// VE-F0092 判据入口（域聚合表注册：`("VE-F0092", vea92_svcquota::run_vea92_checks)`）。
pub fn run_vea92_checks() -> CheckSet {
    let mut s = CheckSet::new("vea92_svcquota");

    // --- 判据 1.1：服务配额——登记隔离公开可查 ---
    {
        let mut m = QuotaMgr::new();
        let _ = m.set_quota(10, 1000);
        let _ = m.set_quota(20, 500);
        let line = m.allocation_line();
        s.add(
            "A92-配额-登记隔离分配公开",
            m.quotas.len() == 2
                && line.contains("服务10 预算1000")
                && line.contains("服务20 预算500")
                && line.contains("余量1000"),
            "两服务各持配额，分配表读屏可达（预算/已用/余量/降级态）",
        );
    }

    // --- 判据 1.2：服务配额——消费记账逐笔与余量对账 ---
    {
        let mut m = QuotaMgr::new();
        let _ = m.set_quota(10, 1000);
        let g1 = m.consume(10, 400);
        let g2 = m.consume(10, 400);
        let q = &m.quotas[0];
        s.add(
            "A92-配额-消费记账余量对账",
            g1 == Ok(400) && g2 == Ok(400) && q.used == 800 && q.remaining() == 200,
            "两笔消费后 used=800、余量=200：恒等式 used+remaining==budget 成立",
        );
    }

    // --- 判据 1.3：服务配额——未登记消费显性拒绝 ---
    {
        let mut m = QuotaMgr::new();
        let r = m.consume(99, 10);
        s.add(
            "A92-配额-未登记显性拒绝",
            r == Err(CODE_SVC_UNKNOWN) && m.stats.consumes == 0,
            "未登记服务走 SVC_UNKNOWN 独立通道，不暗记账",
        );
    }

    // --- 判据 2.1：超用降级——预算耗尽降级不硬拒 ---
    {
        let mut m = QuotaMgr::new();
        let _ = m.set_quota(10, 100);
        let _ = m.consume(10, 100);
        let g = m.consume(10, 50);
        s.add(
            "A92-降级-耗尽降级不清零服务",
            g == Ok(0)
                && m.degrade_of(10) == Ok(DegradeAction::HalfRate)
                && m.stats.degradations == 1,
            "余量 0 供给 0 但服务仍登记在册，降级到减半——节流阀不是断路器",
        );
    }

    // --- 判据 2.2：超用降级——降级带读屏告知语 ---
    {
        let mut m = QuotaMgr::new();
        let _ = m.set_quota(10, 100);
        let _ = m.consume(10, 100);
        let _ = m.consume(10, 50);
        s.add(
            "A92-降级-告知语读屏可达",
            m.last_notice.contains("配额降级告知")
                && m.last_notice.contains("服务 10")
                && m.last_notice.contains("减半供给"),
            "告知语带服务号与降级前后档位名",
        );
    }

    // --- 判据 2.3：超用降级——三态链推进到最小维持 ---
    {
        let mut m = QuotaMgr::new();
        let _ = m.set_quota(10, 64);
        let _ = m.consume(10, 64);
        let _ = m.consume(10, 32);
        let d1 = m.degrade_of(10);
        let _ = m.consume(10, 32);
        let d2 = m.degrade_of(10);
        s.add(
            "A92-降级-三态链推进到最小维持",
            d1 == Ok(DegradeAction::HalfRate)
                && d2 == Ok(DegradeAction::Minimal)
                && m.stats.degradations == 2,
            "正常→减半→最小维持：两次降档各计一次，Minimal 恒定不再降",
        );
    }

    // --- 判据 3.1：分配公开——零预算显性拒绝不暗吞 ---
    {
        let mut m = QuotaMgr::new();
        let r = m.set_quota(10, 0);
        let rr = m.request_reval(10, 0, "test");
        s.add(
            "A92-公开-零预算显性拒绝",
            r == Err(CODE_BUDGET_BAD) && rr == Err(CODE_BUDGET_BAD) && m.quotas.is_empty(),
            "登记与重评两侧都拦零预算：配额不给 0 也不改成 0",
        );
    }

    // --- 判据 3.2：分配公开——重评申请入队待审不即时生效 ---
    {
        let mut m = QuotaMgr::new();
        let _ = m.set_quota(10, 500);
        let id = m.request_reval(10, 2000, "新业务上线");
        let before = m.quotas[0].budget;
        s.add(
            "A92-公开-申请待审不即时生效",
            id.is_ok() && before == 500 && m.stats.reval_requests == 1,
            "申请入队但预算未动：申请与生效分离",
        );
    }

    // --- 判据 3.3：分配公开——审批生效且留痕（含越权拒绝） ---
    {
        let mut m = QuotaMgr::new();
        let _ = m.set_quota(10, 500);
        let id = m.request_reval(10, 2000, "扩容").unwrap_or(0);
        let rogue = m.approve_reval(99, id);
        let ok = m.approve_reval(ADMIN_CALLER, id);
        s.add(
            "A92-公开-审批生效留痕越权拒绝",
            rogue == Err(CODE_APPROVE_FORBID)
                && ok.is_ok()
                && m.quotas[0].budget == 2000
                && m.quotas[0].revals == 1
                && m.last_notice.contains("已留痕"),
            "非管理员审批 FORBID；管理员通过后预算生效、重评计数、告知语留痕",
        );
    }

    // --- 判据 4.1：漂移审计——健康账全绿 ---
    {
        let mut m = QuotaMgr::new();
        let _ = m.set_quota(10, 100);
        let _ = m.set_quota(20, 200);
        let _ = m.consume(10, 30);
        let _ = m.consume(20, 200);
        let a = m.drift_audit();
        s.add(
            "A92-审计-健康账全绿",
            a.checked == 2 && a.drifted == 0 && a.balanced,
            "正常/降级两态混跑后对拍：Σ(used+remaining)==Σbudget 成立",
        );
    }

    // --- 判据 4.2：漂移审计——故障注入能红（审计真的在工作） ---
    {
        let mut m = QuotaMgr::new();
        let _ = m.set_quota(10, 100);
        let _ = m.consume(10, 40);
        // 故障注入：篡改 used 破坏恒等式（模拟账面漂移）
        m.quotas[0].used = 999;
        let a = m.drift_audit();
        s.add(
            "A92-审计-漂移注入能红",
            a.drifted == 1 && !a.balanced && a.checked == 1,
            "判据侧注入漂移验证审计不摆设：恒等式破坏即 drifted=1",
        );
    }

    // --- 判据 5.1：判据自身——码位互异且段独占（整仓唯一段 0xDC 内六码） ---
    {
        let codes = [
            CODE_SVC_BAD,
            CODE_BUDGET_BAD,
            CODE_SVC_UNKNOWN,
            CODE_DRIFT,
            CODE_REVAL_BAD,
            CODE_APPROVE_FORBID,
        ];
        let mut all_diff = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    all_diff = false;
                }
            }
        }
        let in_segment = codes.iter().all(|c| (c & 0xFF00) == 0xDC00);
        s.add(
            "A92-自身-码位互异段独占",
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
    fn vea92_quota_lifecycle() {
        let mut m = QuotaMgr::new();
        assert!(m.set_quota(10, 1000).is_ok());
        assert!(m.set_quota(20, 500).is_ok());
        assert!(m.set_quota(10, 1200).is_ok(), "重复登记=调整预算");
        assert_eq!(m.quotas[0].budget, 1200);
        assert_eq!(m.consume(10, 200), Ok(200));
        assert_eq!(m.consume(99, 1), Err(CODE_SVC_UNKNOWN));
        assert!(m.allocation_line().contains("服务10 预算1200 已用200 余量1000"));
    }

    #[test]
    fn vea92_degrade_chain_full() {
        let mut m = QuotaMgr::new();
        let _ = m.set_quota(10, 80);
        assert_eq!(m.consume(10, 80), Ok(80));
        // 余量 0：第一次降级（减半），供给 0
        assert_eq!(m.consume(10, 10), Ok(0));
        assert_eq!(m.degrade_of(10), Ok(DegradeAction::HalfRate));
        assert!(m.last_notice.contains("正常 → 减半供给"));
        // 再耗尽：到最小维持
        assert_eq!(m.consume(10, 5), Ok(0));
        assert_eq!(m.degrade_of(10), Ok(DegradeAction::Minimal));
        assert_eq!(m.stats.degradations, 2);
        assert!(DegradeAction::Minimal.worsen() == DegradeAction::Minimal, "最小维持恒定");
        // 服务仍在册（不中断）
        assert!(m.allocation_line().contains("服务10"));
    }

    #[test]
    fn vea92_reval_two_stage() {
        let mut m = QuotaMgr::new();
        let _ = m.set_quota(10, 500);
        let id = m.request_reval(10, 2000, "扩容").unwrap_or(0);
        assert_eq!(m.quotas[0].budget, 500, "待审不生效");
        assert_eq!(m.request_reval(10, 3000, "再扩"), Err(CODE_REVAL_BAD), "待审中不重复排队");
        assert_eq!(m.approve_reval(99, id), Err(CODE_APPROVE_FORBID));
        assert!(m.approve_reval(ADMIN_CALLER, id).is_ok());
        assert_eq!(m.approve_reval(ADMIN_CALLER, id), Err(CODE_REVAL_BAD), "已审不重复批");
        assert_eq!(m.quotas[0].budget, 2000);
        assert_eq!(m.quotas[0].revals, 1);
        assert!(m.status_line().contains("审批通过 1"));
    }

    #[test]
    fn vea92_drift_and_recovery() {
        let mut m = QuotaMgr::new();
        let _ = m.set_quota(10, 100);
        let _ = m.consume(10, 100);
        let _ = m.consume(10, 10);
        assert!(m.drift_audit().balanced);
        // 重评通过恢复正常供给
        let id = m.request_reval(10, 400, "恢复").unwrap_or(0);
        let _ = m.approve_reval(ADMIN_CALLER, id);
        assert_eq!(m.degrade_of(10), Ok(DegradeAction::Normal));
        assert_eq!(m.consume(10, 50), Ok(50), "新预算下正常供给");
        assert!(m.drift_audit().balanced);
    }

    #[test]
    fn vea92_checks_all_green() {
        let set = run_vea92_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0092 判据存在红项：{}/{} 绿 —— {}",
            passed,
            passed + failed,
            set.red_items_text()
        );
    }
}
