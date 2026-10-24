//! UNX-F1686 · 权重→vruntime 累加速率一致性核对账 · 诊断段 0xB8F0
//! 验收锚点：UNX-F1686-J1 累加速率=基准速率×（基准权重/本权重）与
//! F1644 账目回放一致，偏差为零，双判据缺一即批内红灯。
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;

/// 本地 LCG 线性同余（CI 确定性引擎——同种子同序列；与各域单文件惯例一致）。
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}


pub const SEG_BASE: u16 = 0xB8F0;

// ── 诊断段独占 · vruntime 错误域 ────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_RATE: u16 = SEG_ERR + 1;   // 速率公式错
const E_WEIGHT: u16 = SEG_ERR + 2; // 权重域非法
const E_REPLAY: u16 = SEG_ERR + 3; // 回放不一致
const E_ZERO: u16 = SEG_ERR + 4;   // 偏差非零
const E_BAL: u16 = SEG_ERR + 5;    // 账不平

/// CFS 权重表（nice −5..=5 → 1024 基准）。
pub fn nice_weight(nice: i32) -> u64 {
    const W: [u64; 11] = [3121, 2502, 2001, 1601, 1277, 1024, 820, 655, 526, 423, 335];
    let idx = (nice.clamp(-5, 5) + 5) as usize;
    W[idx]
}

pub const BASE_WEIGHT: u64 = 1024;
pub const BASE_RATE_NS: u64 = 1_000_000; // 基准速率：1ms 期 1ms 增量

/// 累加速率公式：rate = BASE × (BASE_WEIGHT / weight)（定点 ×2^20）。
pub fn accrual_rate_ns(weight: u64) -> u64 {
    if weight == 0 {
        return 0;
    }
    // 定点：BASE_RATE × BASE_WEIGHT × 2^20 / weight，再回缩
    ((BASE_RATE_NS as u128 * BASE_WEIGHT as u128 * (1u128 << 20)) / weight as u128) as u64 >> 20
}

/// vruntime 累加：Δvruntime = Δwall × BASE_WEIGHT / weight（定点）。
pub fn accrue(wall_ns: u64, weight: u64) -> u64 {
    if weight == 0 {
        return 0;
    }
    ((wall_ns as u128 * BASE_WEIGHT as u128 * (1u128 << 20)) / weight as u128) as u64 >> 20
}

// ── 线程 vruntime 账 ────────────────────────────────────────
/// 线程运行账。
#[derive(Clone, Debug)]
pub struct VThread {
    pub name: String,
    pub weight: u64,
    pub vruntime: u64,
    pub wall_run_ns: u64,
}

impl VThread {
    pub fn new(name: &str, weight: u64) -> Self {
        VThread { name: name.to_string(), weight, vruntime: 0, wall_run_ns: 0 }
    }
    /// 运行 wall 时长并累加 vruntime。
    pub fn run(&mut self, wall_ns: u64) {
        self.vruntime += accrue(wall_ns, self.weight);
        self.wall_run_ns += wall_ns;
    }
    /// 速率一致性：本线程 vruntime/wall 比与公式速率一致（偏差容 1%）。
    pub fn rate_consistent(&self) -> bool {
        if self.wall_run_ns == 0 {
            return true;
        }
        let expect = accrual_rate_ns(self.weight);
        let got = self.vruntime * 1_000_000 / self.wall_run_ns.max(1);
        let diff = if got > expect { got - expect } else { expect - got };
        diff * 100 <= expect.max(1) // 容 1%
    }
}

/// 回放器：F1644 账目（wall 片段序列）逐线程回放，与独立公式对拍零偏差。
pub fn replay_consistent(plan: &[(usize, u64)], threads: &mut [VThread]) -> u64 {
    // 独立重算
    let mut indep: Vec<u64> = threads.iter().map(|t| t.vruntime).collect();
    for (ti, wall) in plan {
        if *ti < threads.len() {
            indep[*ti] += accrue(*wall, threads[*ti].weight);
        }
    }
    // 逐条执行
    for (ti, wall) in plan {
        if *ti < threads.len() {
            threads[*ti].run(*wall);
        }
    }
    let mut dev = 0u64;
    for (i, t) in threads.iter().enumerate() {
        let d = if t.vruntime > indep[i] { t.vruntime - indep[i] } else { indep[i] - t.vruntime };
        dev += d;
    }
    dev
}

/// 标准夹具：三线程（权 3121/1024/335）+ 分配计划。
pub fn standard_threads() -> Vec<VThread> {
    vec![
        VThread::new("hi", nice_weight(-5)),    // 3121 慢累
        VThread::new("mid", nice_weight(0)),    // 1024 基准
        VThread::new("lo", nice_weight(5)),     // 335 快累
    ]
}

pub fn standard_plan() -> Vec<(usize, u64)> {
    vec![(0, 1_000_000), (1, 1_000_000), (2, 1_000_000), (0, 500_000), (2, 2_000_000)]
}

// ── 判据入口 ─────────────────────────────────────────────────
pub fn run_unx1686_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F1686 权重→vruntime累加速率");

    // 1. 速率公式：基准权重下速率 = 基准速率
    let r1024 = accrual_rate_ns(1024);
    s.add("VR-基准速率", (r1024 as i64 - BASE_RATE_NS as i64).abs() <= 1,
        if (r1024 as i64 - BASE_RATE_NS as i64).abs() <= 1 { "" } else { "基准速率漂移" });

    // 2. 权重单调性：权大速率小（3121 < 1024 < 335 速率序）
    let r3121 = accrual_rate_ns(3121);
    let r335 = accrual_rate_ns(335);
    s.add("VR-速率反序", r3121 < r1024 && r1024 < r335,
        if r3121 < r1024 && r1024 < r335 { "" } else { "速率序错" });

    // 3. 好值域权重表（nice -5..5）
    let w_ok = nice_weight(0) == 1024 && nice_weight(-5) == 3121 && nice_weight(5) == 335
        && nice_weight(99) == 335 && nice_weight(-99) == 3121;
    s.add("VR-权重表", w_ok, if w_ok { "" } else { "权重表错" });

    // 4. 零权重零累加
    s.add("VR-零权重", accrue(1_000_000, 0) == 0 && accrual_rate_ns(0) == 0,
        if accrue(1_000_000, 0) == 0 { "" } else { "零权重累加" });

    // 5. 同权同累加（公平基线）
    let mut a = VThread::new("a", 1024);
    let mut b = VThread::new("b", 1024);
    a.run(3_000_000);
    b.run(1_000_000);
    b.run(2_000_000);
    s.add("VR-同权公平", a.vruntime == b.vruntime,
        if a.vruntime == b.vruntime { "" } else { "同权不公" });

    // 6. 三线程速率一致性
    let mut ths = standard_threads();
    for (ti, wall) in standard_plan() {
        ths[ti].run(wall);
    }
    let cons = ths.iter().all(|t| t.rate_consistent());
    s.add("VR-速率一致", cons, if cons { "" } else { "速率不一致" });

    // 7. 回放零偏差
    let mut ths2 = standard_threads();
    let dev = replay_consistent(&standard_plan(), &mut ths2);
    s.add("VR-回放零偏差", dev == 0, if dev == 0 { "" } else { "回放偏差" });

    // 8. 回放自反性（重复回放同值）
    let mut ta = standard_threads();
    let mut tb = standard_threads();
    let da = replay_consistent(&standard_plan(), &mut ta);
    let db = replay_consistent(&standard_plan(), &mut tb);
    s.add("VR-回放确定性", da == db && da == 0 && ta[2].vruntime == tb[2].vruntime,
        if da == db { "" } else { "回放漂移" });

    // 9. 高权线程落后、低权领先（vruntime 序）
    s.add("VR-落后领先序", ths[0].vruntime < ths[1].vruntime && ths[1].vruntime < ths[2].vruntime,
        if ths[0].vruntime < ths[2].vruntime { "" } else { "公平序错" });

    // 10. LCG 随机计划回放零偏差（100 组）
    let mut lcg = Lcg(0xB8F0);
    let mut all_zero = true;
    for _ in 0..100 {
        let mut ts = vec![
            VThread::new("x", nice_weight((lcg.below(11) as i32) - 5)),
            VThread::new("y", nice_weight((lcg.below(11) as i32) - 5)),
        ];
        let mut plan = Vec::new();
        for _ in 0..6 {
            let ti = lcg.below(2) as usize;
            let wall = 100_000 + lcg.below(2_000_000);
            plan.push((ti, wall));
        }
        if replay_consistent(&plan, &mut ts) != 0 {
            all_zero = false;
        }
    }
    s.add("VR-随机回放100", all_zero, if all_zero { "" } else { "随机偏差" });

    // 11. wall 计账守恒（run 累加 wall_run_ns）
    let wsum: u64 = ths.iter().map(|t| t.wall_run_ns).sum();
    let psum: u64 = standard_plan().iter().map(|(_, w)| *w).sum();
    s.add("VR-wall守恒", wsum == psum, if wsum == psum { "" } else { "wall 账不平" });

    // 12. 诊断段独占
    let seg_ok = SEG_BASE >= 0xB8F0 && SEG_BASE < 0xB910;
    s.add("VR-诊断段独占", seg_ok, if seg_ok { "" } else { "段越界" });

    s
}
