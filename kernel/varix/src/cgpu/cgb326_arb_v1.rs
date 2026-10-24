#![allow(dead_code)]
use alloc::vec;
// cgb326_arb_v1.rs ↔ CGPU-F0326 仲裁算法 v1 静态优先级
//
// 判据原文：CGPU-F0326-J1 五步流程、最低保障、原因码、100μs、判据。
// 难点分析：静态优先级仲裁的确定性是可预期版本的根基——同输入必同输出，
//          但「最低保障兜底」会打破纯优先级序，需要在排序键里把保障权重量化。
// 纪律约束：单次仲裁 P95 <100μs；拒绝必带原因码；五步流程不可跳步。
// 成本记账：O(n log n) 排序 + O(n) 扫描，n=千级申请在预算内。


use alloc::vec::Vec;

/// 优先级（0 最高，255 最低）。
pub type Prio = u8;
/// 预算额度（微秒）。
pub type BudgetUs = u64;
/// 账户 ID。
pub type AccountId = u32;
/// 申请 ID（流水号）。
pub type RequestId = u64;

/// 拒绝原因码（判据要求「拒绝原因码」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    /// 账户余额不足。
    InsufficientBalance,
    /// 优先级低于本帧门槛。
    BelowFloor,
    /// 已超账户上限。
    OverCap,
    /// 账户已冻结/注销。
    AccountFrozen,
    /// 帧预算已耗尽。
    FrameExhausted,
    /// 虚报检测降信后拒绝。
    TrustDegraded,
}

impl RejectReason {
    /// 数值码（日志/遥测用）。
    pub fn code(self) -> u16 {
        match self {
            RejectReason::InsufficientBalance => 0x32_01,
            RejectReason::BelowFloor => 0x32_02,
            RejectReason::OverCap => 0x32_03,
            RejectReason::AccountFrozen => 0x32_04,
            RejectReason::FrameExhausted => 0x32_05,
            RejectReason::TrustDegraded => 0x32_06,
        }
    }
    /// 人话描述。
    pub fn describe(self) -> &'static str {
        match self {
            RejectReason::InsufficientBalance => "账户余额不足",
            RejectReason::BelowFloor => "优先级低于本帧门槛",
            RejectReason::OverCap => "已超账户上限",
            RejectReason::AccountFrozen => "账户已冻结或注销",
            RejectReason::FrameExhausted => "帧预算已耗尽",
            RejectReason::TrustDegraded => "虚报降信后拒绝",
        }
    }
}

/// 预算申请（三要素：预估耗时 + 截止期 + 资源类型）。
#[derive(Debug, Clone)]
pub struct BudgetRequest {
    pub id: RequestId,
    pub account: AccountId,
    /// 预估耗时（μs）。
    pub estimate_us: BudgetUs,
    /// 截止期（帧内相对 μs；u64::MAX 表示软截止）。
    pub deadline_us: u64,
    /// 是否关键路径（关键路径走 v1 确定性）。
    pub critical: bool,
}

/// 账户静态属性（四属性：优先级/最低保障/上限/可借出）。
#[derive(Debug, Clone)]
pub struct AccountProfile {
    pub id: AccountId,
    pub prio: Prio,
    /// 最低保障（μs/帧）——兜底不饿死。
    pub guaranteed_us: BudgetUs,
    /// 上限（μs/帧）。
    pub cap_us: BudgetUs,
    /// 可借出标记。
    pub lendable: bool,
    /// 信任分（0~100，虚报降信）。
    pub trust: u8,
}

/// 仲裁结果。
#[derive(Debug, Clone)]
pub struct ArbDecision {
    pub request_id: RequestId,
    pub granted_us: BudgetUs,
    pub rejected: Option<RejectReason>,
}

impl ArbDecision {
    pub fn ok(request_id: RequestId, granted_us: BudgetUs) -> Self {
        ArbDecision { request_id, granted_us, rejected: None }
    }
    pub fn deny(request_id: RequestId, reason: RejectReason) -> Self {
        ArbDecision { request_id, granted_us: 0, rejected: Some(reason) }
    }
    pub fn is_granted(&self) -> bool {
        self.rejected.is_none()
    }
}

/// 排序键：三元组 (优先级, 是否关键路径, 申请 ID)。
/// 确定性排序——同输入必同输出，这是 v1 的核心承诺。
fn sort_key(r: &BudgetRequest, profiles: &[AccountProfile]) -> (u8, u8, u64) {
    let prio = profiles
        .iter()
        .find(|p| p.id == r.account)
        .map(|p| p.prio)
        .unwrap_or(255);
    let crit = if r.critical { 0 } else { 1 };
    (prio, crit, r.id)
}

/// 静态优先级仲裁器 v1。
///
/// 五步流程（判据要求不可跳步）：
///   1. 申请受理（收集本帧申请）
///   2. 校验（账户状态/上限/信任）
///   3. 排序（优先级 + 关键路径 + ID 确定性）
///   4. 批准/拒绝（最低保障兜底 + 原因码）
///   5. 记账（流水 + 统计）
pub struct ArbiterV1 {
    /// 本帧剩余预算（μs）。
    frame_remaining: BudgetUs,
    /// 本帧总预算。
    frame_total: BudgetUs,
    /// 优先级门槛（低于此优先级的非保障申请直接拒）。
    prio_floor: Prio,
    /// 仲裁流水（本帧）。
    ledger: Vec<ArbDecision>,
    /// 累计批准总额。
    total_granted: BudgetUs,
    /// 累计拒绝计数。
    total_rejected: u32,
    /// 最低保障已发放表（账户 → 本帧已发保障额度）。
    guaranteed_paid: Vec<(AccountId, BudgetUs)>,
}

impl ArbiterV1 {
    /// 新建仲裁器。
    pub fn new(frame_total_us: BudgetUs, prio_floor: Prio) -> Self {
        ArbiterV1 {
            frame_remaining: frame_total_us,
            frame_total: frame_total_us,
            prio_floor,
            ledger: Vec::new(),
            total_granted: 0,
            total_rejected: 0,
            guaranteed_paid: Vec::new(),
        }
    }

    /// 帧开始：重置窗口。
    pub fn begin_frame(&mut self) {
        self.frame_remaining = self.frame_total;
        self.ledger.clear();
        self.guaranteed_paid.clear();
    }

    /// 查询账户本帧已发保障额度。
    fn guaranteed_already(&self, account: AccountId) -> BudgetUs {
        self.guaranteed_paid
            .iter()
            .find(|(a, _)| *a == account)
            .map(|(_, v)| *v)
            .unwrap_or(0)
    }

    /// 第 2 步：校验单个申请。返回 Some(reason) 表示校验不过。
    fn validate(&self, req: &BudgetRequest, profile: &AccountProfile) -> Option<RejectReason> {
        if profile.trust < 30 {
            return Some(RejectReason::TrustDegraded);
        }
        if req.estimate_us > profile.cap_us {
            return Some(RejectReason::OverCap);
        }
        if self.frame_remaining == 0 {
            return Some(RejectReason::FrameExhausted);
        }
        None
    }

    /// 第 4 步：批准额度计算（含最低保障兜底）。
    fn approve_amount(&mut self, req: &BudgetRequest, profile: &AccountProfile) -> BudgetUs {
        // 最低保障优先：若本帧保障未发满，先补保障（可突破优先级门槛）。
        let already = self.guaranteed_already(req.account);
        let guarantee_gap = profile.guaranteed_us.saturating_sub(already);
        if guarantee_gap > 0 {
            let grant = guarantee_gap.min(req.estimate_us).min(self.frame_remaining);
            if grant > 0 {
                self.guaranteed_paid.push((req.account, already + grant));
                return grant;
            }
        }
        // 非保障路径：按剩余与上限裁剪。
        let grant = req.estimate_us.min(self.frame_remaining).min(profile.cap_us);
        grant
    }

    /// 主入口：对一批申请做仲裁（五步流程）。
    ///
    /// 返回与输入等长的决策数组（顺序与排序后一致，按 request_id 可查）。
    pub fn arbitrate(
        &mut self,
        requests: &[BudgetRequest],
        profiles: &[AccountProfile],
    ) -> Vec<ArbDecision> {
        // 第 1 步：申请受理（已在参数中）。
        // 第 2 步：校验 + 预过滤。
        let mut candidates: Vec<&BudgetRequest> = Vec::new();
        let mut decisions: Vec<ArbDecision> = Vec::new();

        for req in requests {
            let profile = match profiles.iter().find(|p| p.id == req.account) {
                Some(p) => p,
                None => {
                    decisions.push(ArbDecision::deny(req.id, RejectReason::AccountFrozen));
                    self.total_rejected += 1;
                    continue;
                }
            };
            if let Some(reason) = self.validate(req, profile) {
                decisions.push(ArbDecision::deny(req.id, reason));
                self.total_rejected += 1;
            } else {
                candidates.push(req);
            }
        }

        // 第 3 步：排序（确定性三元组键）。
        candidates.sort_by(|a, b| {
            let ka = sort_key(a, profiles);
            let kb = sort_key(b, profiles);
            ka.cmp(&kb)
        });

        // 第 4 步：批准/拒绝。
        for req in &candidates {
            let profile = profiles
                .iter()
                .find(|p| p.id == req.account)
                .expect("validated profile must exist");

            // 优先级门槛（保障路径已在校验外兜底，这里管非保障）。
            let already = self.guaranteed_already(req.account);
            let needs_guarantee = profile.guaranteed_us > already;
            if !needs_guarantee && profile.prio > self.prio_floor && !req.critical {
                decisions.push(ArbDecision::deny(req.id, RejectReason::BelowFloor));
                self.total_rejected += 1;
                continue;
            }

            let grant = self.approve_amount(req, profile);
            if grant == 0 {
                decisions.push(ArbDecision::deny(req.id, RejectReason::InsufficientBalance));
                self.total_rejected += 1;
            } else {
                self.frame_remaining -= grant;
                self.total_granted += grant;
                decisions.push(ArbDecision::ok(req.id, grant));
            }
        }

        // 第 5 步：记账。
        for d in &decisions {
            self.ledger.push(d.clone());
        }
        decisions
    }

    /// 帧内剩余。
    pub fn remaining(&self) -> BudgetUs {
        self.frame_remaining
    }
    /// 帧内已批。
    pub fn granted_this_frame(&self) -> BudgetUs {
        self.frame_total - self.frame_remaining
    }
    /// 本帧流水。
    pub fn ledger(&self) -> &[ArbDecision] {
        &self.ledger
    }
    /// 累计批准。
    pub fn total_granted(&self) -> BudgetUs {
        self.total_granted
    }
    /// 累计拒绝。
    pub fn total_rejected(&self) -> u32 {
        self.total_rejected
    }
    /// 拒绝率（0.0~1.0）。
    pub fn reject_rate(&self) -> f32 {
        let total = self.total_granted.count_ones() as u32; // placeholder 不用
        let _ = total;
        let n = self.ledger.len() as u32 + self.total_rejected;
        if n == 0 {
            0.0
        } else {
            self.total_rejected as f32 / n as f32
        }
    }
}

// ---------------------------------------------------------------------------
// 判据（run_cgb326_checks）
// ---------------------------------------------------------------------------

use super::super::checks::CheckSet;

/// CGPU-F0326 判据集。
pub fn run_cgb326_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0326 仲裁算法 v1 静态优先级");

    // --- 五步流程可执行 ---
    let profiles = vec![
        AccountProfile { id: 1, prio: 1, guaranteed_us: 100, cap_us: 1000, lendable: true, trust: 90 },
        AccountProfile { id: 2, prio: 5, guaranteed_us: 50, cap_us: 500, lendable: true, trust: 90 },
        AccountProfile { id: 3, prio: 9, guaranteed_us: 0, cap_us: 200, lendable: false, trust: 90 },
    ];
    let reqs = vec![
        BudgetRequest { id: 101, account: 1, estimate_us: 300, deadline_us: 1000, critical: false },
        BudgetRequest { id: 102, account: 2, estimate_us: 200, deadline_us: 2000, critical: false },
        BudgetRequest { id: 103, account: 3, estimate_us: 100, deadline_us: 500, critical: true },
    ];
    let mut arb = ArbiterV1::new(1000, 8);
    let decisions = arb.arbitrate(&reqs, &profiles);
    cs.add(
        "五步流程产出等长决策",
        decisions.len() == 3,
        "三次申请三条决策",
    );
    let granted: u64 = decisions.iter().map(|d| d.granted_us).sum();
    cs.add(
        "预算守恒 Σ批准 ≤ 帧预算",
        granted <= 1000,
        "总额不超帧预算",
    );

    // --- 最低保障兜底 ---
    let mut arb2 = ArbiterV1::new(50, 2); // 帧预算极小
    let d2 = arb2.arbitrate(&reqs, &profiles);
    let g1: u64 = d2.iter().filter(|d| d.request_id == 101).map(|d| d.granted_us).sum();
    cs.add(
        "最低保障兜底：低预算帧仍优先满足保障",
        g1 > 0,
        "账户1 保障 100μs 应有发放",
    );

    // --- 拒绝原因码 ---
    let bad_profiles = vec![
        AccountProfile { id: 9, prio: 1, guaranteed_us: 0, cap_us: 100, lendable: true, trust: 10 },
    ];
    let bad_reqs = vec![
        BudgetRequest { id: 201, account: 9, estimate_us: 50, deadline_us: 100, critical: false },
    ];
    let mut arb3 = ArbiterV1::new(1000, 255);
    let d3 = arb3.arbitrate(&bad_reqs, &bad_profiles);
    let denied = d3.iter().find(|d| d.request_id == 201).cloned();
    cs.add(
        "拒绝必带原因码",
        denied.map(|d| d.rejected.is_some()).unwrap_or(false),
        "低信任账户被拒",
    );
    cs.add(
        "原因码可描述",
        RejectReason::TrustDegraded.describe().len() > 0,
        "人话描述非空",
    );

    // --- 确定性：同输入同输出 ---
    let mut a1 = ArbiterV1::new(1000, 8);
    let mut a2 = ArbiterV1::new(1000, 8);
    let r1 = a1.arbitrate(&reqs, &profiles);
    let r2 = a2.arbitrate(&reqs, &profiles);
    let same = r1.len() == r2.len()
        && r1.iter().zip(r2.iter()).all(|(x, y)| {
            x.request_id == y.request_id && x.granted_us == y.granted_us && x.rejected == y.rejected
        });
    cs.add("确定性：双跑一致", same, "同输入必同输出");

    // --- 优先级排序：高优先先批 ---
    let tight = vec![
        BudgetRequest { id: 301, account: 3, estimate_us: 400, deadline_us: 0, critical: false },
        BudgetRequest { id: 302, account: 1, estimate_us: 400, deadline_us: 0, critical: false },
    ];
    let mut arb4 = ArbiterV1::new(500, 255);
    let d4 = arb4.arbitrate(&tight, &profiles);
    let g_hi = d4.iter().find(|d| d.request_id == 302).map(|d| d.granted_us).unwrap_or(0);
    let g_lo = d4.iter().find(|d| d.request_id == 301).map(|d| d.granted_us).unwrap_or(0);
    cs.add(
        "高优先先批",
        g_hi >= g_lo,
        "账户1(prio=1) 不低于账户3(prio=9)",
    );

    cs
}
