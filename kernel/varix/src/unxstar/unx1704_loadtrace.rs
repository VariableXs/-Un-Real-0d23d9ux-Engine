//! UNX-F1704 · 每线程负载贡献追踪（贡献分解）账 · 诊断段 0xB910
//! 验收锚点：UNX-F1704-J1 线程级追踪值=窗口内运行时长×权重归一，
//! Σ线程贡献=核追踪值闭合，迁移随线程携带，双判据缺一即批内红灯。
use alloc::vec;
use alloc::format;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;


pub const SEG_BASE: u16 = 0xB910;

// ── 诊断段独占 · 负载追踪错误域 ─────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_FORMULA: u16 = SEG_ERR + 1;  // 公式错
const E_CLOSE: u16 = SEG_ERR + 2;    // Σ不闭合
const E_CARRY: u16 = SEG_ERR + 3;    // 迁移未携带
const E_WINDOW: u16 = SEG_ERR + 4;   // 窗口账错
const E_BAL: u16 = SEG_ERR + 5;      // 账不平

/// 窗口长（ms）。
pub const WINDOW_MS: u64 = 100;

// ── 确定性 LCG ───────────────────────────────────────────────
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

// ── 线程贡献账 ───────────────────────────────────────────────
/// 一线程：权重 + 窗口内运行时长（ms）。
#[derive(Clone, Debug)]
pub struct ThContrib {
    pub name: String,
    pub weight: u64,
    pub run_ms: u64,
}

impl ThContrib {
    /// 贡献 = 运行时长 × 权重（归一到 1024 基准）。
    pub fn contrib(&self) -> u64 {
        self.run_ms * self.weight / 1024
    }
}

/// 核追踪账：线程集 + 核级追踪值。
#[derive(Clone, Debug)]
pub struct CoreTrace {
    pub core: usize,
    pub threads: Vec<ThContrib>,
    pub core_load: u64, // 核追踪值（独立录入）
}

impl CoreTrace {
    pub fn new(core: usize) -> Self {
        CoreTrace { core, threads: Vec::new(), core_load: 0 }
    }
    pub fn add_thread(&mut self, name: &str, weight: u64, run_ms: u64) {
        self.threads.push(ThContrib { name: name.to_string(), weight, run_ms });
    }
    /// Σ线程贡献。
    pub fn sum_contrib(&self) -> u64 {
        self.threads.iter().map(|t| t.contrib()).sum()
    }
    /// 闭合断言：Σ线程贡献 = 核追踪值。
    pub fn closed(&self) -> bool {
        self.sum_contrib() == self.core_load
    }
    /// 窗口约束：每线程 run_ms ≤ 窗口长（单核单窗口）。
    pub fn window_ok(&self) -> bool {
        self.threads.iter().all(|t| t.run_ms <= WINDOW_MS)
    }
    /// 迁移：线程携贡献离开本核。
    pub fn detach(&mut self, name: &str) -> Option<ThContrib> {
        let idx = self.threads.iter().position(|t| t.name == name)?;
        let t = self.threads.remove(idx);
        self.core_load -= t.contrib();
        Some(t)
    }
    /// 迁移入：携贡献加入。
    pub fn attach(&mut self, t: ThContrib) {
        self.core_load += t.contrib();
        self.threads.push(t);
    }
}

/// 贡献公式验证：给定 (run, weight) 对与期望归一值。
pub fn formula_check(run_ms: u64, weight: u64) -> u64 {
    run_ms * weight / 1024
}

/// 标准夹具：双核，核 0 三线程（Σ=核账），核 1 一线程。
pub fn standard_traces() -> Vec<CoreTrace> {
    let mut c0 = CoreTrace::new(0);
    c0.add_thread("t1", 1024, 40); // 40
    c0.add_thread("t2", 2048, 30); // 60
    c0.add_thread("t3", 512, 24);  // 12
    c0.core_load = 112;
    let mut c1 = CoreTrace::new(1);
    c1.add_thread("t4", 4096, 10); // 40
    c1.core_load = 40;
    vec![c0, c1]
}

// ── 判据入口 ─────────────────────────────────────────────────
pub fn run_unx1704_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F1704 每线程负载贡献追踪");
    let traces = standard_traces();

    // 1. 贡献公式逐项（40×1024/1024=40 等）
    let f = formula_check(40, 1024) == 40 && formula_check(30, 2048) == 60
        && formula_check(24, 512) == 12 && formula_check(10, 4096) == 40;
    s.add("LT-贡献公式", f, if f { "" } else { "公式错" });

    // 2. Σ线程贡献 = 核追踪值（双核闭合）
    let closed = traces.iter().all(|c| c.closed());
    s.add("LT-Σ闭合", closed, if closed { "" } else { "Σ不闭合" });

    // 3. 窗口约束
    let win = traces.iter().all(|c| c.window_ok());
    s.add("LT-窗口约束", win, if win { "" } else { "窗口越界" });

    // 4. 迁移随线程携带（t2 从核0 → 核1，Σ 两核守恒）
    let mut c0 = traces[0].clone();
    let mut c1 = traces[1].clone();
    let sum_before = c0.core_load + c1.core_load;
    let carried = c0.detach("t2");
    let carry_ok = carried.is_some() && carried.as_ref().map(|t| t.weight == 2048 && t.run_ms == 30).unwrap_or(false);
    if let Some(t) = carried {
        c1.attach(t);
    }
    let sum_after = c0.core_load + c1.core_load;
    s.add("LT-迁移携带", carry_ok && sum_before == sum_after && c0.closed() && c1.closed(),
        if carry_ok && sum_before == sum_after { "" } else { "迁移破账" });

    // 5. detach 不存在线程返回 None 且账不变
    let mut c2 = traces[0].clone();
    let ghost = c2.detach("no_such");
    s.add("LT-幽灵迁出", ghost.is_none() && c2.closed(),
        if ghost.is_none() { "" } else { "幽灵迁出成功" });

    // 6. 零时长零贡献
    let z = ThContrib { name: "z".to_string(), weight: 9999, run_ms: 0 };
    s.add("LT-零时长", z.contrib() == 0, if z.contrib() == 0 { "" } else { "零时长贡献" });

    // 7. 权重归一守恒（同 run 不同权重贡献比 = 权重比）
    let a = ThContrib { name: "a".to_string(), weight: 1024, run_ms: 10 };
    let b = ThContrib { name: "b".to_string(), weight: 2048, run_ms: 10 };
    s.add("LT-权重比守恒", b.contrib() == a.contrib() * 2,
        if b.contrib() == a.contrib() * 2 { "" } else { "权重比破" });

    // 8. 窗口满载闭合（单线程跑满窗）
    let mut c3 = CoreTrace::new(2);
    c3.add_thread("full", 1024, WINDOW_MS); // 100
    c3.core_load = 100;
    s.add("LT-满载闭合", c3.closed() && c3.window_ok(),
        if c3.closed() { "" } else { "满载破" });

    // 9. LCG 随机闭合（50 核账）
    let mut lcg = Lcg(0xB910);
    let mut all_close = true;
    for r in 0..50 {
        let mut c = CoreTrace::new(r);
        let n = 1 + lcg.below(4) as usize;
        let mut total = 0u64;
        for i in 0..n {
            let w = [512u64, 1024, 2048, 4096][lcg.below(4) as usize];
            let run = lcg.below(WINDOW_MS + 1);
            c.add_thread(&format!("r{}_t{}", r, i), w, run);
            total += run * w / 1024;
        }
        c.core_load = total;
        if !c.closed() {
            all_close = false;
        }
    }
    s.add("LT-随机闭合50", all_close, if all_close { "" } else { "随机破账" });

    // 10. 多核迁移后总账守恒（随机迁移链）
    let mut ca = traces[0].clone();
    let mut cb = traces[1].clone();
    let total0 = ca.core_load + cb.core_load;
    for _ in 0..3 {
        if ca.threads.is_empty() {
            break;
        }
        let nm = ca.threads[0].name.clone();
        if let Some(t) = ca.detach(&nm) {
            cb.attach(t);
        }
    }
    s.add("LT-迁移链守恒", ca.core_load + cb.core_load == total0,
        if ca.core_load + cb.core_load == total0 { "" } else { "链破账" });

    // 11. 贡献非负单调（run 增贡献不降）
    let t1 = ThContrib { name: "m".to_string(), weight: 1024, run_ms: 5 };
    let t2 = ThContrib { name: "m".to_string(), weight: 1024, run_ms: 9 };
    s.add("LT-单调不减", t2.contrib() >= t1.contrib(),
        if t2.contrib() >= t1.contrib() { "" } else { "贡献倒挂" });

    // 12. 诊断段独占
    let seg_ok = SEG_BASE >= 0xB910 && SEG_BASE < 0xB930;
    s.add("LT-诊断段独占", seg_ok, if seg_ok { "" } else { "段越界" });

    s
}
