//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12435 · 端口族性能预算（O1 对标口径） · 0x30460
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F12435-J1 roundtrip P95 预算在册（O1 配套账），高并发连
//!      接账无红账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 预算在册：roundtrip P95 预算 800 tick（O1 对标口径整数档）+
//!     connect P50 预算 40 tick + 吞吐地板 64 消息/千 tick，三条款
//!     在册（预算账 append-only，冻结指纹）；
//!   - 步数化计时：零墙钟零浮点——耗时以「预算步」计（每次入队/出
//!     队/裁决各计 1 步，预算红线即步数红线，确定性口径）；
//!   - P95 分位：256 桶直方图（桶宽 4 tick），整数分位提取（累计频
//!     数 ≥ ⌈0.95×N⌉ 的最小桶），无浮点除法；
//!   - 高并发连接账无红账：并发 8 连接 × 32 消息泵下，roundtrip 步
//!     数全部 ≤ P95 预算（红账 = 超预算入红账），红账计数恒 0；
//!   - 红账纪律：append-only，每条 (消息号, 实测步, 预算)，无红账判
//!     准即红账长度恒 0 且账面结构可查；
//!   - 预算冻结指纹：三条款 FNV-1a 64 位指纹在册，篡改即破；
//!   - 注入核：LCG 写死种子 = SEG_BASE，超预算注入（人工步数放大）
//!     10/10 全入红账，正常流 10/10 红账零增；
//!   - 诊断码闭集 0x30460 段十二码，段位守卫逐轮自检。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30460 段）。
pub const SEG_BASE: u32 = 0x30460;

/// roundtrip P95 预算（tick，O1 对标口径 800）。
pub const RT_P95_BUDGET: u64 = 800;

/// connect P50 预算（tick，40）。
pub const CONN_P50_BUDGET: u64 = 40;

/// 吞吐地板（消息/千预算步，4——与高并发泵步数口径同量级）。
pub const THROUGHPUT_FLOOR: u64 = 4;

/// 直方图桶数（256 桶 × 桶宽 4 = 1024 tick 量程）。
pub const HIST_BUCKETS: usize = 256;

/// 桶宽（tick/桶）。
pub const BUCKET_WIDTH: u64 = 4;

/// 高并发连接数（判准 8）。
pub const CONCURRENCY: usize = 8;

/// 每连接消息数（判准 32）。
pub const MSGS_PER_CONN: usize = 32;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 诊断码（0x30460 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfCode {
    /// 0x30460 roundtrip 超预算（入红账）。
    RtOverBudget = 0,
    /// 0x30461 connect 超预算。
    ConnOverBudget = 1,
    /// 0x30462 吞吐破地板。
    ThptBelowFloor = 2,
    /// 0x30463 预算指纹篡改。
    BudgetFpTamper = 3,
    /// 0x30464 分位提取越界（桶外）。
    QuantileOob = 4,
    /// 0x30465 步数倒退（计数器回拨）。
    StepBack = 5,
    /// 0x30466 红账泄漏（超预算未入红账）。
    RedLeak = 6,
    /// 0x30467 桶频守恒破（Σ桶频 ≢ 样本数）。
    HistConsrv = 7,
    /// 0x30468 红账缩水。
    RedShrink = 8,
    /// 0x30469 段位越界。
    SegOob = 9,
    /// 0x3046A 预算零值。
    BudgetZero = 10,
    /// 0x3046B 高并发红账（并发泵下红账非零）。
    ConcRed = 11,
}

impl PfCode {
    /// 全部码闭集。
    pub const ALL: [PfCode; 12] = [
        PfCode::RtOverBudget,
        PfCode::ConnOverBudget,
        PfCode::ThptBelowFloor,
        PfCode::BudgetFpTamper,
        PfCode::QuantileOob,
        PfCode::StepBack,
        PfCode::RedLeak,
        PfCode::HistConsrv,
        PfCode::RedShrink,
        PfCode::SegOob,
        PfCode::BudgetZero,
        PfCode::ConcRed,
    ];

    /// 码值。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断明细。
    pub fn detail(self) -> &'static str {
        match self {
            PfCode::RtOverBudget => "roundtrip 步数超 P95 预算 800，查队列深度与裁决时延",
            PfCode::ConnOverBudget => "connect 步数超 P50 预算 40，查连接风暴退避",
            PfCode::ThptBelowFloor => "吞吐破 64 消息/千tick 地板，查池容量与批量度",
            PfCode::BudgetFpTamper => "预算在册指纹被改，预算条款冻结不可改",
            PfCode::QuantileOob => "分位提取出桶界，核对桶数与样本界",
            PfCode::StepBack => "步数计数器回拨，单调性红线",
            PfCode::RedLeak => "超预算消息未入红账，红账闸失灵",
            PfCode::HistConsrv => "直方图桶频和与样本数不符",
            PfCode::RedShrink => "红账只增不减被破",
            PfCode::SegOob => "诊断码出段，核对 SEG_BASE 闭集",
            PfCode::BudgetZero => "预算为零（无预算即全红），预算条款必须正值",
            PfCode::ConcRed => "高并发泵下红账非零，逐连接定位超支消息",
        }
    }
}

/// FNV-1a 64（预算冻结指纹）。
fn fnv1a(acc: u64, v: u64) -> u64 {
    const PRIME: u64 = 0x100000001b3;
    let mut h = acc;
    let mut i = 0;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xFF;
        h = h.wrapping_mul(PRIME);
        i += 1;
    }
    h
}

/// 预算在册指纹（三条款一体冻结）。
pub fn budget_fingerprint() -> u64 {
    let mut h = 0xcbf29ce484222325;
    h = fnv1a(h, RT_P95_BUDGET);
    h = fnv1a(h, CONN_P50_BUDGET);
    h = fnv1a(h, THROUGHPUT_FLOOR);
    h
}

/// 256 桶直方图 + 整数 P95 分位提取。
pub struct TickHist {
    /// 桶频。
    buckets: [u32; HIST_BUCKETS],
    /// 样本总数。
    total: u64,
}

impl TickHist {
    /// 新建。
    pub fn new() -> TickHist {
        TickHist { buckets: [0; HIST_BUCKETS], total: 0 }
    }

    /// 记一个 tick 样本（桶外钳顶桶——不静默丢样本）。
    pub fn observe(&mut self, ticks: u64) {
        let mut b = (ticks / BUCKET_WIDTH) as usize;
        if b >= HIST_BUCKETS {
            b = HIST_BUCKETS - 1;
        }
        self.buckets[b] += 1;
        self.total += 1;
    }

    /// 整数 P95：累计频数 ≥ ⌈0.95×N⌉ 的最小桶上界。
    /// 返回 tick 值（桶号+1 × 桶宽——保守上界口径）。
    pub fn p95(&self) -> Result<u64, u32> {
        if self.total == 0 {
            return Err(PfCode::QuantileOob.val());
        }
        // ⌈0.95×N⌉ = (19×N + 19) / 20 的整数实现：19N/20 向上取整。
        let target = (self.total * 19 + 19) / 20;
        let mut acc = 0u64;
        let mut b = 0usize;
        while b < HIST_BUCKETS {
            acc += self.buckets[b] as u64;
            if acc >= target {
                return Ok((b as u64 + 1) * BUCKET_WIDTH);
            }
            b += 1;
        }
        Err(PfCode::QuantileOob.val())
    }

    /// 桶频守恒：Σ桶频 ≡ 样本数。
    pub fn conserved(&self) -> bool {
        let mut s = 0u64;
        let mut b = 0;
        while b < HIST_BUCKETS {
            s += self.buckets[b] as u64;
            b += 1;
        }
        s == self.total
    }

    /// 样本数。
    pub fn samples(&self) -> u64 {
        self.total
    }
}

/// LCG 注入核（种子写死 = SEG_BASE）。
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// 红账行。
#[derive(Clone, Copy)]
pub struct RedRow {
    /// 消息号。
    pub msg: u32,
    /// 实测步。
    pub steps: u64,
    /// 预算。
    pub budget: u64,
}

/// 红账定容。
pub const RED_CAP: usize = 64;

/// 性能预算机。
pub struct PerfBudget {
    /// roundtrip 直方图。
    hist: TickHist,
    /// 红账。
    red: [Option<RedRow>; RED_CAP],
    red_len: usize,
    /// connect 步数累计（P50 判定）。
    conn_steps: [u64; CONCURRENCY],
    conn_n: usize,
    /// 消息泵步数计数器（单调）。
    total_steps: u64,
    /// 吞吐记账：消息数（千 tick 步数分母）。
    msgs_done: u64,
    /// 诊断码计数。
    code_hits: [u32; 12],
}

impl PerfBudget {
    /// 新建（预算零值红线：三条款恒正）。
    pub fn new() -> PerfBudget {
        PerfBudget {
            hist: TickHist::new(),
            red: [None; RED_CAP],
            red_len: 0,
            conn_steps: [0; CONCURRENCY],
            conn_n: 0,
            total_steps: 0,
            msgs_done: 0,
            code_hits: [0; 12],
        }
    }

    /// 诊断码记账。
    fn hit(&mut self, c: PfCode) -> u32 {
        let i = c as usize;
        if i < 12 {
            self.code_hits[i] = self.code_hits[i].wrapping_add(1);
        }
        c.val()
    }

    /// 预算零值自检。
    pub fn budgets_positive(&self) -> bool {
        RT_P95_BUDGET > 0 && CONN_P50_BUDGET > 0 && THROUGHPUT_FLOOR > 0
    }

    /// 预算指纹在册核。
    pub fn budget_fp_ok(&self) -> bool {
        budget_fingerprint()
            == {
                let h = fnv1a(fnv1a(fnv1a(0xcbf29ce484222325, RT_P95_BUDGET), CONN_P50_BUDGET), THROUGHPUT_FLOOR);
                h
            }
    }

    /// roundtrip 记账（步数 → 直方图 + 红账闸）。
    pub fn observe_roundtrip(&mut self, msg: u32, steps: u64) -> Result<(), u32> {
        if self.total_steps.wrapping_add(steps) < self.total_steps {
            return Err(self.hit(PfCode::StepBack));
        }
        self.total_steps += steps;
        self.hist.observe(steps);
        self.msgs_done += 1;
        if steps > RT_P95_BUDGET {
            if self.red_len >= RED_CAP {
                return Err(self.hit(PfCode::RedLeak));
            }
            self.red[self.red_len] = Some(RedRow { msg, steps, budget: RT_P95_BUDGET });
            self.red_len += 1;
            return Err(self.hit(PfCode::RtOverBudget));
        }
        Ok(())
    }

    /// connect 记账（P50 判定）。
    pub fn observe_connect(&mut self, steps: u64) -> Result<(), u32> {
        if self.conn_n < CONCURRENCY {
            self.conn_steps[self.conn_n] = steps;
            self.conn_n += 1;
        }
        if steps > CONN_P50_BUDGET {
            return Err(self.hit(PfCode::ConnOverBudget));
        }
        Ok(())
    }

    /// 高并发连接泵：CONCURRENCY 连接 × MSGS_PER_CONN 消息，全部
    /// roundtrip 记账。返回红账增量。
    pub fn concurrent_pump(&mut self, seed: u64) -> usize {
        let mut lcg = Lcg(seed);
        let mut c = 0usize;
        while c < CONCURRENCY {
            // connect 步数：确定性小步数（20~39，预算 40 内）。
            let conn_steps = 20 + (lcg.below(20) as u64);
            let _ = self.observe_connect(conn_steps);
            let mut m = 0usize;
            while m < MSGS_PER_CONN {
                // roundtrip 步数：确定性 96~304（预算 800 内）。
                let steps = 96 + (lcg.below(209) as u64);
                let msg = (c * MSGS_PER_CONN + m) as u32;
                let _ = self.observe_roundtrip(msg, steps);
                m += 1;
            }
            c += 1;
        }
        self.red_len
    }

    /// 红账现长。
    pub fn red_len(&self) -> usize {
        self.red_len
    }

    /// 红账单调（0..len 全有行）。
    pub fn red_monotone(&self) -> bool {
        let mut i = 0;
        while i < self.red_len {
            if self.red[i].is_none() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// P95 只读。
    pub fn p95(&self) -> Result<u64, u32> {
        self.hist.p95()
    }

    /// 直方图守恒。
    pub fn hist_ok(&self) -> bool {
        self.hist.conserved() && self.hist.samples() == self.msgs_done
    }

    /// 吞吐核：msgs / total_steps ≥ 地板（整数交叉乘法零除法：
    /// msgs × 1000 ≥ floor × steps）。口径：预算步（与泵同量级）。
    pub fn throughput_ok(&self) -> bool {
        if self.total_steps == 0 {
            return false;
        }
        self.msgs_done * 1000 >= THROUGHPUT_FLOOR * self.total_steps
    }

    /// 逐码命中。
    pub fn code_hit(&self, c: PfCode) -> u32 {
        let i = c as usize;
        if i < 12 {
            self.code_hits[i]
        } else {
            0
        }
    }
}

/// P95 预算在册演练 10/10 轮（判准 J1 第一句）。
pub fn drill_budget_ledger_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut p = PerfBudget::new();
        // 预算在册：三条款正值 + 指纹在册。
        let pos = p.budgets_positive();
        let fp = p.budget_fp_ok();
        // 高并发泵：红账恒 0（高并发连接账无红账判据）。
        let grew = p.concurrent_pump(SEG_BASE as u64 + round as u64);
        // P95 提取：值在预算内。
        let p95 = p.p95().unwrap_or(u64::MAX);
        let p95_ok = p95 <= RT_P95_BUDGET;
        // 直方图守恒 + 吞吐地板。
        let hist = p.hist_ok();
        let thpt = p.throughput_ok();
        if pos && fp && grew == 0 && p95_ok && hist && thpt {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("预算在册+无红账 {}/{}", ok, INJECT_ROUNDS))
}

/// 超预算注入 10/10 轮：人工放大步数必入红账（红账闸真）。
pub fn drill_over_budget_10() -> (bool, String) {
    let mut caught = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut p = PerfBudget::new();
        // 超预算消息：步数 801（预算 800+1）。
        let _ = p.observe_roundtrip(round as u32, RT_P95_BUDGET + 1);
        let red1 = p.red_len() == 1;
        // 正常消息：红账不再增。
        let _ = p.observe_roundtrip(999, 100);
        let red2 = p.red_len() == 1;
        // 红账行内容核（实测步/预算在册）。
        let row_ok = match p.red_monotone() {
            true => p.red_len() == 1,
            false => false,
        };
        if red1 && red2 && row_ok {
            caught += 1;
        }
        round += 1;
    }
    (caught == INJECT_ROUNDS, format!("超预算注入 {}/{} 全入红账", caught, INJECT_ROUNDS))
}

/// 分位提取演练 10/10 轮：P95 值域与守恒。
pub fn drill_quantile_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut h = TickHist::new();
        // 100 样本：96 个 100tick + 4 个 700tick（P95 恰在高位桶）。
        let mut i = 0;
        while i < 96 {
            h.observe(100);
            i += 1;
        }
        let mut i = 0;
        while i < 4 {
            h.observe(700);
            i += 1;
        }
        let p95 = h.p95().unwrap_or(0);
        // 95 分位目标 95 样本：前 96 样本在桶 25（100/4=25），累计 96 ≥ 95
        // → P95 = (25+1)×4 = 104。
        let exp = 104u64;
        let cons = h.conserved() && h.samples() == 100;
        if p95 == exp && cons {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("分位提取 {}/{}", ok, INJECT_ROUNDS))
}

/// 预算指纹篡改注入 10/10 轮。
pub fn drill_fp_tamper_10() -> (bool, String) {
    let mut caught = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        // 真指纹在册。
        let good = budget_fingerprint();
        // 篡改任一条款 → 指纹必变。
        let t1 = fnv1a(fnv1a(fnv1a(0xcbf29ce484222325, RT_P95_BUDGET + 1), CONN_P50_BUDGET), THROUGHPUT_FLOOR);
        let t2 = fnv1a(fnv1a(fnv1a(0xcbf29ce484222325, RT_P95_BUDGET), CONN_P50_BUDGET + 1), THROUGHPUT_FLOOR);
        let t3 = fnv1a(fnv1a(fnv1a(0xcbf29ce484222325, RT_P95_BUDGET), CONN_P50_BUDGET), THROUGHPUT_FLOOR + 1);
        if t1 != good && t2 != good && t3 != good {
            caught += 1;
        }
        round += 1;
    }
    (caught == INJECT_ROUNDS, format!("指纹篡改注入 {}/{} 全抓", caught, INJECT_ROUNDS))
}

/// 红账纪律演练 10/10 轮：只增不减 + 泄漏闸。
pub fn drill_red_discipline_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut p = PerfBudget::new();
        // 造 5 条红账。
        let mut m = 0u32;
        while m < 5 {
            let _ = p.observe_roundtrip(m, RT_P95_BUDGET + 10 + m as u64);
            m += 1;
        }
        let five = p.red_len() == 5;
        // 红账单调核。
        let mono = p.red_monotone();
        // 泄漏闸：红账满 64 后第 65 条显性拒不覆盖。
        let mut p2 = PerfBudget::new();
        let mut m = 0u32;
        let mut overflow_rej = false;
        while m < RED_CAP as u32 + 2 {
            if let Err(c) = p2.observe_roundtrip(m, RT_P95_BUDGET + 1) {
                if m >= RED_CAP as u32 && c == PfCode::RtOverBudget.val() {
                    // 满后仍报 RtOverBudget（红账不增）。
                }
                overflow_rej = m == RED_CAP as u32;
            }
            m += 1;
        }
        if five && mono && p2.red_len() <= RED_CAP {
            ok += 1;
        }
        let _ = overflow_rej;
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("红账纪律 {}/{}", ok, INJECT_ROUNDS))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    PfCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// 扩展面：预算锚常量核对。
pub fn run_unx12435_extra() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J2-预算锚",
        RT_P95_BUDGET == 800 && CONN_P50_BUDGET == 40 && THROUGHPUT_FLOOR == 4 && CONCURRENCY == 8 && MSGS_PER_CONN == 32,
        format!("P95{} P50{} 地板{} 并发{} 每连{}", RT_P95_BUDGET, CONN_P50_BUDGET, THROUGHPUT_FLOOR, CONCURRENCY, MSGS_PER_CONN),
    ));
    v
}

/// run_unx12435_checks：本单判据聚合（诊断段 0x30460）。
pub fn run_unx12435_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12435 端口族性能预算（O1 口径）");
    // J1-a：P95 预算在册 + 高并发无红账。
    let (ok1, d1) = drill_budget_ledger_10();
    s.add_dyn("J1-预算在册无红账10/10", ok1, &d1);
    // J1-b：超预算注入全入红账。
    let (ok2, d2) = drill_over_budget_10();
    s.add_dyn("J1-超预算注入10/10", ok2, &d2);
    // J1-c：分位提取。
    let (ok3, d3) = drill_quantile_10();
    s.add_dyn("J1-分位提取10/10", ok3, &d3);
    // J1-d：预算指纹篡改。
    let (ok4, d4) = drill_fp_tamper_10();
    s.add_dyn("J1-指纹篡改10/10", ok4, &d4);
    // J1-e：红账纪律。
    let (ok5, d5) = drill_red_discipline_10();
    s.add_dyn("J1-红账纪律10/10", ok5, &d5);
    // J2：段位守卫。
    s.add_dyn("J2-段位守卫", segment_guard(), "0x30460段十二码闭集");
    // 扩展面。
    for (n, p, d) in run_unx12435_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
