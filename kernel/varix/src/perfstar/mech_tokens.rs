//! mech_tokens — 分层令牌桶（HTB）+ PID 调频器（AI-K1 深化批次四 · F047/F048）。
//!
//! 主册依据：
//! - F047【设计细节】「四类子预算 + 2000μs 总预算」——预算的**执行器**
//!   （超了怎么办：排队/借用/降级）此前只有直方图观测面；HTB（Hierarchical
//!   Token Bucket，Linux tc 同族）是预算执行的标准答案：类内保证（CIR）+
//!   类间借用（父桶余量），预算不再是"记账"而是"闸门"。
//! - F048【设计细节】「负载感知 P-state 升降档」——升降档决策的**控制环**
//!   此前是阈值判定；PID（比例-积分-微分）是连续负载下的成熟控制律，
//!   积分限幅（anti-windup）对应主册「切换守卫」的数学形态。
//! - 零堆、零浮点（PID 全整数定点：误差 ×1024）。

// ---------------------------------------------------------------------------
// 1. 令牌桶（HTB 叶节点）
// ---------------------------------------------------------------------------

/// 令牌桶：rate = 令牌/ms（毫单位），burst = 容量上沿。
#[derive(Clone, Copy, Debug)]
pub struct TokenBucket {
    /// 桶内令牌（毫单位——1 令牌 = 1000，避免长期取整漂移）。
    tokens: i64,
    /// 容量（毫单位）。
    burst: i64,
    /// 补充速率（毫单位/ms）。
    rate: i64,
    pub last_ms: u64,
    /// 拒绝计数（预算执行证据）。
    pub rejects: u64,
}

impl TokenBucket {
    pub const fn new(burst_tokens: u32, rate_per_ms_milli: u32) -> Self {
        TokenBucket {
            tokens: burst_tokens as i64 * 1000,
            burst: burst_tokens as i64 * 1000,
            rate: rate_per_ms_milli as i64,
            last_ms: 0,
            rejects: 0,
        }
    }

    /// 推进时间并补充令牌（线性补充，容量封顶）。
    pub fn refill(&mut self, now_ms: u64) {
        if now_ms > self.last_ms {
            let elapsed = (now_ms - self.last_ms) as i64;
            self.tokens = (self.tokens + elapsed * self.rate).min(self.burst);
            self.last_ms = now_ms;
        }
    }

    /// 取令牌（毫单位）。不足 → false + 计数。
    pub fn take(&mut self, cost_milli: u64) -> bool {
        if self.tokens >= cost_milli as i64 {
            self.tokens -= cost_milli as i64;
            true
        } else {
            self.rejects += 1;
            false
        }
    }

    /// 归还预扣（借用失败的事务性回滚——不留半空状态）。
    pub fn restore(&mut self, milli: u64) {
        self.tokens = (self.tokens + milli as i64).min(self.burst);
    }

    pub fn avail_milli(&self) -> i64 {
        self.tokens
    }
}

/// 预算类（F047 四类；叶桶 + 父桶两级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetClass {
    Input,
    Compose,
    Audio,
    Normal,
}

/// 每类预算参数（μs 预算 → 令牌 1:1，ms 时间轴；主册 2000μs 总预算）。
/// 叶桶 burst = 类预算，rate = 预算/16ms 的补充线（一个调度切片回血）。
pub const CLASS_BUDGET_US: [u32; 4] = [500, 6000, 3000, 12000]; // Input/Compose/Audio/Normal 的批内份额（合计 21.5ms > 2ms 总线——突发靠 burst，持续靠 rate）
/// 总预算（μs）：主册 F047「2000μs 总预算内」。
pub const TOTAL_BUDGET_US: u32 = 2000;

/// HTB：四叶 + 一父。叶桶保类内保证；父桶余量供类间借用。
pub struct Htb {
    leaves: [TokenBucket; 4],
    parent: TokenBucket,
}

impl Htb {
    /// 各类 burst = 类预算（μs→毫单位令牌），补充率 = burst/16ms。
    pub const fn new() -> Self {
        Htb {
            leaves: [
                TokenBucket::new(CLASS_BUDGET_US[0], CLASS_BUDGET_US[0] * 1000 / 16),
                TokenBucket::new(CLASS_BUDGET_US[1], CLASS_BUDGET_US[1] * 1000 / 16),
                TokenBucket::new(CLASS_BUDGET_US[2], CLASS_BUDGET_US[2] * 1000 / 16),
                TokenBucket::new(CLASS_BUDGET_US[3], CLASS_BUDGET_US[3] * 1000 / 16),
            ],
            parent: TokenBucket::new(TOTAL_BUDGET_US, TOTAL_BUDGET_US * 1000 / 16),
        }
    }

    /// 申请预算（μs）。叶桶自给 → 直接过；不足 → 父桶借用（HTB ceil
    /// 语义——总预算不可超支）；借用失败 → 叶桶预扣事务性回滚。
    pub fn request(&mut self, cls: BudgetClass, us: u32, now_ms: u64) -> bool {
        let i = cls as usize;
        self.leaves[i].refill(now_ms);
        self.parent.refill(now_ms);
        let need = us as i64 * 1000;
        let leaf_avail = self.leaves[i].avail_milli().max(0);
        let from_leaf = need.min(leaf_avail) as u64;
        let from_parent = (need - leaf_avail).max(0) as u64;
        if from_leaf > 0 {
            // 叶桶充足时 avail ≥ need，take 必成（契约内断言，不失手）。
            let _ = self.leaves[i].take(from_leaf);
        }
        if from_parent == 0 {
            return true;
        }
        if self.parent.take(from_parent) {
            true
        } else {
            self.leaves[i].restore(from_leaf);
            self.leaves[i].rejects += 1;
            false
        }
    }

    pub fn leaf_rejects(&self, cls: BudgetClass) -> u64 {
        self.leaves[cls as usize].rejects
    }

    pub fn parent_rejects(&self) -> u64 {
        self.parent.rejects
    }
}

// ---------------------------------------------------------------------------
// 2. PID 调频器（整数定点 ×1024）
// ---------------------------------------------------------------------------

/// PID 增益（定点 ×1024）：主册无数字——这是控制参数登记处（旋钮清单
/// 第 9 查口径），随闸门实机标定后回写。
pub const PID_KP: i64 = 32; // 0.03125
pub const PID_KI: i64 = 4; // 0.00390625
pub const PID_KD: i64 = 8; // 0.0078125
/// 积分限幅（anti-windup，定点单位）。
pub const PID_I_CLAMP: i64 = 64 * 1024;

/// PID 调频控制器：目标 = 期望忙占比（permille），反馈 = 实测忙占比。
/// 输出 = P-state 档位偏移（0 = 最高频，向上 = 降频——ACPI _PSS 降序口径
/// 与 cpufreq 域一致）。
#[derive(Clone, Copy, Debug)]
pub struct PidGov {
    target_permille: u32,
    integral: i64,
    prev_err: i64,
    primed: bool,
    /// 输出档位钳制 [max_shift, 0]——0 表示维持最高频。
    pub shifts_issued: u64,
}

impl PidGov {
    pub const fn new(target_permille: u32) -> Self {
        PidGov { target_permille, integral: 0, prev_err: 0, primed: false, shifts_issued: 0 }
    }

    /// 一个控制周期：输入实测忙占比（permille），输出降档偏移（0=不降，
    /// 正值 = 向下移几档）。误差 = 实测 - 目标：忙过头 → 升频（负偏移/
    /// 0），闲 → 降频（正偏移）。
    pub fn step(&mut self, busy_permille: u32, max_shift: i32) -> i32 {
        let err = (busy_permille as i64 - self.target_permille as i64) * 1024;
        if !self.primed {
            self.primed = true;
            self.prev_err = err;
        }
        // 漏积分（leaky anti-windup）：先衰减再累积——饱和期误差不再无限
        // 堆积，回到目标后积分自然泄放（切换守卫的数学形态）。
        self.integral = self.integral * 15 / 16 + err;
        if self.integral > PID_I_CLAMP {
            self.integral = PID_I_CLAMP;
        }
        if self.integral < -PID_I_CLAMP {
            self.integral = -PID_I_CLAMP;
        }
        let deriv = err - self.prev_err;
        self.prev_err = err;
        let u = PID_KP * err + PID_KI * self.integral + PID_KD * deriv;
        // u > 0：忙过头 → 提频 → 偏移取负（向 0 收）；u < 0：闲 → 降频。
        let shift = (-u) / (1024 * 1024);
        let clamped = shift.clamp(-(max_shift as i64), max_shift as i64) as i32;
        if clamped != 0 {
            self.shifts_issued += 1;
        }
        clamped
    }

    pub fn reset_integral(&mut self) {
        self.integral = 0;
    }
}

// ---------------------------------------------------------------------------
// 宿主单测
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// CheckSet（挂 F047）
// ---------------------------------------------------------------------------

/// 运行检查项（判据锚点见对账表批次四段）。
use crate::checks::CheckSet;

pub fn run_checks() -> CheckSet {
    let mut cs = CheckSet::new("F047-mech-tokens");
    // 1) 令牌桶补充与封顶。
    let mut b = TokenBucket::new(10, 1000);
    let _ = b.take(10_000);
    b.refill(5);
    let r5 = b.avail_milli() == 5000;
    b.refill(100);
    cs.add("bucket_refill_cap", r5 && b.avail_milli() == 10_000, "");
    // 2) HTB：叶保证 → 父借用 → 总预算上沿真拒。
    let mut h = Htb::new();
    let ok1 = h.request(BudgetClass::Input, 400, 0);
    let ok2 = h.request(BudgetClass::Input, 600, 0);
    let mut denied = 0;
    for _ in 0..100 {
        if !h.request(BudgetClass::Audio, 3000, 0) {
            denied += 1;
        }
    }
    cs.add("htb_borrow_and_gate", ok1 && ok2 && denied > 0, "");
    // 3) PID 双边界收敛 + 无 windup 残留。
    let mut g = PidGov::new(300);
    let mut last = 0;
    for _ in 0..200 {
        last = g.step(1000, 3);
    }
    let mut g2 = PidGov::new(500);
    for _ in 0..500 {
        g2.step(1000, 4);
    }
    let mut nonzero = 0;
    for _ in 0..64 {
        if g2.step(500, 4) != 0 {
            nonzero += 1;
        }
    }
    cs.add("pid_boundaries_and_no_windup", last == -3 && nonzero < 16, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_refills_and_caps() {
        let mut b = TokenBucket::new(10, 1000); // 10 令牌容量，1000 毫令牌/ms
        let _ = b.take(10_000); // 清空（10 令牌 = 10000 毫单位）
        assert_eq!(b.avail_milli(), 0);
        b.refill(5);
        assert_eq!(b.avail_milli(), 5000, "5ms × 1000 毫令牌/ms");
        b.refill(100);
        assert_eq!(b.avail_milli(), 10_000, "容量封顶");
    }

    #[test]
    fn bucket_rejects_are_counted() {
        let mut b = TokenBucket::new(2, 0); // 无补充
        assert!(b.take(2000));
        assert!(!b.take(1));
        assert_eq!(b.rejects, 1);
    }

    #[test]
    fn htb_leaf_guarantee_then_borrow() {
        let mut h = Htb::new();
        // Input 预算 500μs：直接自给。
        assert!(h.request(BudgetClass::Input, 400, 0));
        // 超出叶桶 → 借父桶（父桶 2000μs 余额充足）。
        assert!(h.request(BudgetClass::Input, 600, 0), "叶桶不足但父桶有余 → 借用成功");
        // 父桶也被掏空（2000 - 600 = 1400 已借 + 叶余 100；再借大额 → 拒）。
        let leaf_rejects_before = h.leaf_rejects(BudgetClass::Normal);
        // Normal 桶 12000μs 自给充足——不会拒；制造一个叶桶耗尽 + 父桶耗尽：
        assert!(h.request(BudgetClass::Normal, 12000, 0)); // Normal 全自给
        let _ = leaf_rejects_before;
        // 连续大额请求最终必须被拒（总预算 2000μs 的 HTB 上沿）。
        let mut denied = 0;
        for _ in 0..100 {
            if !h.request(BudgetClass::Audio, 3000, 0) {
                denied += 1;
            }
        }
        assert!(denied > 0, "总预算上沿必须真实拒绝（F047 闸门语义）");
    }

    #[test]
    fn htb_time_heals_budget() {
        let mut h = Htb::new();
        // 耗尽 Input 叶桶 + 父桶借用余额：连续大额申请直到拒绝。
        let mut exhausted = false;
        for _ in 0..200 {
            if !h.request(BudgetClass::Input, 500, 0) {
                exhausted = true;
                break;
            }
        }
        assert!(exhausted, "持续超预算必须走到拒绝");
        // 64ms 后回血（rate = burst/16ms → 4 个整周期补满）。
        assert!(h.request(BudgetClass::Input, 100, 64), "时间回血后预算恢复");
    }

    #[test]
    fn pid_converges_to_target() {
        // 恒定 100% 忙、目标 30%：输出应钳在升频边界；恒 0% 忙 → 降频边界。
        let mut g = PidGov::new(300);
        let mut last = 0;
        for _ in 0..200 {
            last = g.step(1000, 3);
        }
        assert_eq!(last, -3, "忙过头必须顶到升频边界");
        let mut g2 = PidGov::new(300);
        let mut last2 = 0;
        for _ in 0..200 {
            last2 = g2.step(0, 3);
        }
        assert_eq!(last2, 3, "闲置必须顶到降频边界");
    }

    #[test]
    fn pid_no_prolonged_windup() {
        // anti-windup：长期饱和回到目标后，输出必须在有限步内归零
        // （漏积分泄放）——饱和残留不得变成永久偏置。
        let mut g = PidGov::new(500);
        for _ in 0..500 {
            g.step(1000, 4); // 长期饱和
        }
        let mut nonzero = 0;
        for _ in 0..64 {
            if g.step(500, 4) != 0 {
                nonzero += 1;
            }
        }
        assert!(nonzero < 16, "回目标后 64 步内输出必须归零（非零 {} 步）", nonzero);
        // 更长程：输出恒 0。
        for _ in 0..200 {
            assert_eq!(g.step(500, 4), 0, "积分泄放完成后不得残留偏置");
        }
    }

    #[test]
    fn pid_idle_target_midline() {
        // 恰好在目标上：误差 0，输出应为 0（微分不激发、积分为 0）。
        let mut g = PidGov::new(500);
        assert_eq!(g.step(500, 3), 0);
        assert_eq!(g.step(500, 3), 0);
    }
}
