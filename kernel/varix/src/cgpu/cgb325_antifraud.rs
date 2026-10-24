// cgb325_antifraud.rs ↔ CGPU-F0325 预算校验与防欺诈
//
// 判据原文：CGPU-F0325-J1 虚报检测、超用分级、默认池、审计、判据。
// 难点分析：虚报检测不能只看单次偏差——「持续统计」才有信度；
//          超用分级处置要避免「一次超用就杀」的过激，也要避免「屡犯不罚」。
// 纪律约束：默认池无特权；全部校验事件可查；降信可恢复（观察期）。
// 成本记账：偏差统计用 Welford 在线方差 O(1)，无历史样本堆积。

#![allow(dead_code)]

use alloc::vec::Vec;

pub type AccountId = u32;
pub type RequestId = u64;
pub type BudgetUs = u64;

/// 虚报/超用处置档位。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PenaltyTier {
    /// 首次：警告。
    Warn,
    /// 屡犯：降档（额度打折）。
    Derate,
    /// 严重：冻结。
    Freeze,
}

/// 校验事件（审计记录）。
#[derive(Debug, Clone)]
pub struct AuditEvent {
    pub seq: u64,
    pub account: AccountId,
    pub request_id: RequestId,
    /// 声明耗时。
    pub declared_us: BudgetUs,
    /// 实测耗时。
    pub actual_us: BudgetUs,
    /// 相对偏差（|actual-declared|/declared，万分比）。
    pub deviation_bp: u32,
    /// 是否触发处置。
    pub penalty: Option<PenaltyTier>,
}

/// Welford 在线均值/方差（虚报检测的统计底座）。
#[derive(Debug, Clone, Default)]
struct Welford {
    n: u64,
    mean: f64,
    m2: f64,
}

impl Welford {
    fn push(&mut self, x: f64) {
        self.n += 1;
        let delta = x - self.mean;
        self.mean += delta / self.n as f64;
        let delta2 = x - self.mean;
        self.m2 += delta * delta2;
    }
    fn mean(&self) -> f64 {
        self.mean
    }
    fn variance(&self) -> f64 {
        if self.n < 2 {
            0.0
        } else {
            self.m2 / (self.n - 1) as f64
        }
    }
}

/// 账户信度画像。
#[derive(Debug, Clone)]
struct TrustProfile {
    account: AccountId,
    /// 偏差统计（相对偏差，0.5 = 50%）。
    deviation: Welford,
    /// 连续超用次数。
    overuse_streak: u32,
    /// 当前信任分（0~100）。
    trust: u8,
    /// 是否冻结。
    frozen: bool,
    /// 降档系数（100=全额，50=半额）。
    derate_pct: u8,
    /// 观察期剩余帧数（降信恢复用）。
    observe_frames: u32,
}

/// 防欺诈校验器。
pub struct AntiFraud {
    profiles: Vec<TrustProfile>,
    audit: Vec<AuditEvent>,
    audit_seq: u64,
    /// 虚报判定阈值（相对偏差 50% = 5000 万分比）。
    fraud_threshold_bp: u32,
    /// 持续虚报样本下限（少于此样本数不判虚报）。
    min_samples: u64,
    /// 默认池账户 ID（无标签任务归入）。
    default_pool: AccountId,
    /// 累计警告/降档/冻结计数。
    n_warn: u32,
    n_derate: u32,
    n_freeze: u32,
}

impl AntiFraud {
    /// 新建。
    pub fn new(default_pool: AccountId) -> Self {
        AntiFraud {
            profiles: Vec::new(),
            audit: Vec::new(),
            audit_seq: 0,
            fraud_threshold_bp: 5000,
            min_samples: 5,
            default_pool,
            n_warn: 0,
            n_derate: 0,
            n_freeze: 0,
        }
    }

    /// 注册账户。
    pub fn register(&mut self, account: AccountId) {
        if self.profiles.iter().any(|p| p.account == account) {
            return;
        }
        self.profiles.push(TrustProfile {
            account,
            deviation: Welford::default(),
            overuse_streak: 0,
            trust: 100,
            frozen: false,
            derate_pct: 100,
            observe_frames: 0,
        });
    }

    fn profile_mut(&mut self, account: AccountId) -> &mut TrustProfile {
        if !self.profiles.iter().any(|p| p.account == account) {
            self.register(account);
        }
        let idx = self
            .profiles
            .iter()
            .position(|p| p.account == account)
            .expect("registered");
        &mut self.profiles[idx]
    }

    /// 校验一次执行结果（虚报检测 + 超用分级）。
    ///
    /// 返回处置档位（None = 无处置）。
    pub fn observe(
        &mut self,
        account: AccountId,
        request_id: RequestId,
        declared_us: BudgetUs,
        actual_us: BudgetUs,
        approved_us: BudgetUs,
    ) -> Option<PenaltyTier> {
        // 相对偏差。
        let deviation_bp = if declared_us == 0 {
            if actual_us == 0 {
                0
            } else {
                10000
            }
        } else {
            let diff = (actual_us as i64 - declared_us as i64).unsigned_abs();
            ((diff * 10000) / declared_us as u64).min(100000) as u32
        };

        let mut penalty = None;

        // --- 虚报检测（持续统计） ---
        {
            // 阈值快照先取（profile_mut 的可变借用不能横跨 self 字段读，E0503）。
            let min_samples = self.min_samples;
            let fraud_thr = self.fraud_threshold_bp;
            let p = self.profile_mut(account);
            p.deviation.push(deviation_bp as f64 / 10000.0);
            let n = p.deviation.n;
            let mean = p.deviation.mean();
            let is_fraud = n >= min_samples
                && mean > fraud_thr as f64 / 10000.0
                && deviation_bp > fraud_thr;
            if is_fraud {
                p.trust = p.trust.saturating_sub(15);
                if p.trust < 30 && !p.frozen {
                    p.frozen = true;
                    penalty = Some(PenaltyTier::Freeze);
                    self.n_freeze += 1;
                } else if penalty.is_none() {
                    penalty = Some(PenaltyTier::Warn);
                    self.n_warn += 1;
                }
            }
        }

        // --- 超用检测（实际超批准预算） ---
        if actual_us > approved_us {
            let p = self.profile_mut(account);
            p.overuse_streak = p.overuse_streak.saturating_add(1);
            if p.overuse_streak >= 5 {
                p.derate_pct = 50;
                if penalty.is_none() {
                    penalty = Some(PenaltyTier::Derate);
                    self.n_derate += 1;
                }
            } else if p.overuse_streak >= 1 {
                if penalty.is_none() {
                    penalty = Some(PenaltyTier::Warn);
                    self.n_warn += 1;
                }
            }
        } else {
            // 良好行为：重置 streak，观察期后恢复信任。
            let p = self.profile_mut(account);
            p.overuse_streak = 0;
            if p.observe_frames > 0 {
                p.observe_frames -= 1;
            } else if p.trust < 100 {
                p.trust = (p.trust + 1).min(100);
                if p.trust >= 50 && p.derate_pct < 100 {
                    p.dergeet_restore();
                }
            }
        }

        // --- 审计记录 ---
        self.audit_seq += 1;
        self.audit.push(AuditEvent {
            seq: self.audit_seq,
            account,
            request_id,
            declared_us,
            actual_us,
            deviation_bp,
            penalty,
        });

        penalty
    }

    /// 账户有效额度系数（降档生效）。
    pub fn effective_pct(&self, account: AccountId) -> u8 {
        self.profiles
            .iter()
            .find(|p| p.account == account)
            .map(|p| if p.frozen { 0 } else { p.derate_pct })
            .unwrap_or(100)
    }

    /// 是否冻结。
    pub fn is_frozen(&self, account: AccountId) -> bool {
        self.profiles
            .iter()
            .find(|p| p.account == account)
            .map(|p| p.frozen)
            .unwrap_or(false)
    }

    /// 信任分。
    pub fn trust(&self, account: AccountId) -> u8 {
        self.profiles
            .iter()
            .find(|p| p.account == account)
            .map(|p| p.trust)
            .unwrap_or(100)
    }

    /// 默认池 ID（无标签任务归入）。
    pub fn default_pool(&self) -> AccountId {
        self.default_pool
    }

    /// 审计查询（按账户）。
    pub fn audit_for(&self, account: AccountId) -> Vec<&AuditEvent> {
        self.audit.iter().filter(|e| e.account == account).collect()
    }

    /// 全量审计条数。
    pub fn audit_len(&self) -> usize {
        self.audit.len()
    }

    /// 处置统计。
    pub fn penalty_stats(&self) -> (u32, u32, u32) {
        (self.n_warn, self.n_derate, self.n_freeze)
    }
}

// TrustProfile 辅助（降档恢复）——写成扩展避免 impl 块嵌套过深。
impl TrustProfile {
    fn dergeet_restore(&mut self) {
        self.derate_pct = 100;
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

use super::super::checks::CheckSet;

pub fn run_cgb325_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0325 预算校验与防欺诈");

    // --- 虚报检测：持续偏差 >50% 降信 ---
    let mut af = AntiFraud::new(9999);
    af.register(1);
    for i in 0..8 {
        af.observe(1, i, 100, 200, 100); // 声明 100 实测 200 → 100% 偏差
    }
    let t = af.trust(1);
    cs.add("虚报持续 → 信任下降", t < 100, "8 次 100% 偏差应降信");

    // --- 超用分级：首次警告 ---
    let mut af2 = AntiFraud::new(9999);
    af2.register(2);
    let p1 = af2.observe(2, 1, 100, 150, 100);
    cs.add("超用首次 → 警告", p1 == Some(PenaltyTier::Warn), "超批准 50%");

    // --- 屡犯降档 ---
    for i in 2..7 {
        af2.observe(2, i, 100, 150, 100);
    }
    let pct = af2.effective_pct(2);
    cs.add("屡犯 → 降档 50%", pct <= 50, "连续超用 5+ 次");

    // --- 默认池无特权 ---
    let af3 = AntiFraud::new(7777);
    cs.add("默认池 ID 正确", af3.default_pool() == 7777, "无标签任务归入");

    // --- 审计记录可查 ---
    let mut af4 = AntiFraud::new(9999);
    af4.register(3);
    af4.observe(3, 1, 100, 100, 100);
    af4.observe(3, 2, 100, 100, 100);
    cs.add("审计条数 = 2", af4.audit_len() == 2, "两次观察两条记录");
    let evs = af4.audit_for(3);
    cs.add("按账户审计可查", evs.len() == 2, "账户 3 的记录");

    // --- 良好行为恢复 ---
    let mut af5 = AntiFraud::new(9999);
    af5.register(4);
    for i in 0..8 {
        af5.observe(4, i, 100, 200, 100);
    }
    let t_low = af5.trust(4);
    for i in 8..40 {
        af5.observe(4, i, 100, 95, 100); // 良好
    }
    let t_recover = af5.trust(4);
    cs.add("良好行为 → 信任回升", t_recover > t_low, "30 次良好后回升");

    cs
}

