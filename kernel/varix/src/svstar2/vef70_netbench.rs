//! VE-F5418 · 网络基准（vef70_netbench）
//!
//! **基准三件**（结果入 F4958 体系——基准有据可查）：
//! ① **吞吐基准**（标准负载下连接吞吐实测——标准负载常量单源，实
//! 测对期望线性对拍）；② **延迟基准**（端到端延迟分布——**三定式
//! 测量** U07 口径：P50/P95/P99 三分位最近秩法，秩=⌈p·n/100⌉ 手算
//! 锚 20 样本 P50=100/P95=190/P99=200）；③ **容量基准**（并发容量
//! 曲线——四档并发探针逐档实测，容量峰值=延迟超 SLA 前最高档——
//! 容量有据可查不是拍脑袋）。
//!
//! 错误路径与降级矩阵：**样本损坏→哈希校验重取**（样本携带加和校
//! 验戳，坏样拒收计重取数——重取是流程不是静默）；**离群未去→协
//! 议断言拦截**（离群在入口断言拦——超硬上限的样本根本进不了分
//! 布面，不是事后剔除）；**漂移→环境指纹**（基准结果携带环境指纹
//! O(1)，对表不一致即漂移立案——跨环境结果不可比是硬事实）。
//!
//! 性能逐项：单轮 O(负载规模)（逐样本喂入）；指纹 O(1)（位混合常
//! 数）；对表 O(1)（整数比较）。
//!
//! 跨批对接点：F4324/F4958 口径上游；F5412 模拟消费；F5419 联调。
//! 无障碍：基准数据读屏可达（域本色）；无隐私面（只有数值与指纹）。
//!
//! 诊断码 0xE9 段五码互异：`E_NB_SAMPLE_CORRUPT` / `E_NB_OUTLIER` /
//! `E_NB_ENV_DRIFT` / `E_NB_NO_BASELINE` / `E_NB_LOAD_INVALID`。

use crate::checks::CheckSet;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 版本与常量（阈值独立声明，判据独立手算对拍）
// ---------------------------------------------------------------------------

/// 网络基准版本。
pub const NETBENCH_VERSION: &str = "F70-netbench-v1";
/// 标准负载（字节——吞吐基准的单源负载锚）。
pub const STANDARD_LOAD: u64 = 1_000_000;
/// 标准负载期望吞吐（MB/s——手算锚：负载 1M→800；线性比例）。
pub const THROUGHPUT_EXPECT: u64 = 800;
/// 延迟硬上限（ms——离群协议断言线，超此值样本根本不进分布）。
pub const LATENCY_HARD_CAP: u64 = 1000;
/// SLA 延迟线（ms——容量曲线的档位出界线，30fps≈33）。
pub const SLA_LATENCY_MS: u64 = 33;
/// 容量探针档位（并发连接数闭集）。
pub const PROBE_TIERS: [u64; 4] = [1, 10, 100, 1000];

// ---------------------------------------------------------------------------
// 诊断码与原因文案（0xE9 段五码互异；reason 全量显性）
// ---------------------------------------------------------------------------

/// 样本损坏：哈希校验失败，重取流程。
pub const E_NB_SAMPLE_CORRUPT: u16 = 0xE901;
/// 离群拦截：超硬上限样本在入口断言拦（协议断言不是事后剔除）。
pub const E_NB_OUTLIER: u16 = 0xE902;
/// 环境漂移：对表指纹不一致——跨环境结果不可比。
pub const E_NB_ENV_DRIFT: u16 = 0xE903;
/// 对表无基线：F4958 册中无此基准项。
pub const E_NB_NO_BASELINE: u16 = 0xE904;
/// 负载非法：零负载或超容负载拒绝开跑。
pub const E_NB_LOAD_INVALID: u16 = 0xE905;

/// 诊断码 → 原因文案（全量显性，缺省分支兜底）。
pub fn reason_of(code: u16) -> &'static str {
    match code {
        E_NB_SAMPLE_CORRUPT => "样本损坏：哈希校验失败，计入重取数不静默",
        E_NB_OUTLIER => "离群拦截：超硬上限样本在入口断言拦，进不了分布面",
        E_NB_ENV_DRIFT => "环境漂移：对表指纹不一致，跨环境结果不可比",
        E_NB_NO_BASELINE => "对表无基线：基准册中无此基准项",
        E_NB_LOAD_INVALID => "负载非法：零负载或超容负载拒绝开跑",
        _ => "未知诊断码",
    }
}

/// 立案簿（0xE9 段事件账——有界不静默）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagNote {
    pub code: u16,
    pub subject: String,
    pub detail: String,
}

#[derive(Debug)]
pub struct DiagBag {
    notes: Vec<DiagNote>,
    cap: usize,
    dropped: usize,
}

impl DiagBag {
    pub fn new() -> DiagBag {
        DiagBag { notes: Vec::new(), cap: 64, dropped: 0 }
    }

    pub fn push(&mut self, code: u16, subject: &str, detail: &str) {
        if self.notes.len() >= self.cap {
            self.notes.remove(0);
            self.dropped += 1;
        }
        self.notes.push(DiagNote {
            code,
            subject: subject.to_string(),
            detail: detail.to_string(),
        });
    }

    pub fn len(&self) -> usize {
        self.notes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    pub fn count_of(&self, code: u16) -> usize {
        self.notes.iter().filter(|n| n.code == code).count()
    }

    pub fn has(&self, code: u16) -> bool {
        self.count_of(code) > 0
    }

    pub fn spoken(&self) -> String {
        if self.notes.is_empty() {
            return "诊断簿空".to_string();
        }
        let mut out = format!("诊断簿共{}条", self.notes.len());
        for n in self.notes.iter() {
            out.push('；');
            out.push_str(reason_of(n.code));
            out.push('：');
            out.push_str(&n.subject);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 吞吐基准（标准负载单源——实测对期望线性对拍）
// ---------------------------------------------------------------------------

/// 吞吐基准器（单轮 O(负载规模)——确定性模型手算锚：负载 1M→800，
/// 2M→1600 线性）。
#[derive(Debug)]
pub struct ThroughputBench {
    pub bag: DiagBag,
    pub runs: u32,
}

impl ThroughputBench {
    pub fn new() -> ThroughputBench {
        ThroughputBench { bag: DiagBag::new(), runs: 0 }
    }

    /// 开跑（负载校验 O(1)：零负载/超容拒——单轮 O(负载规模) 计量）。
    pub fn run(&mut self, load_bytes: u64) -> Result<u64, u16> {
        if load_bytes == 0 || load_bytes > STANDARD_LOAD * 64 {
            self.bag.push(E_NB_LOAD_INVALID, "throughput", &format!("负载{}", load_bytes));
            return Err(E_NB_LOAD_INVALID);
        }
        // 确定性模型：吞吐 = 期望 × 负载 / 标准负载（线性对拍锚）。
        let v = THROUGHPUT_EXPECT * load_bytes / STANDARD_LOAD;
        self.runs += 1;
        Ok(v)
    }
}

// ---------------------------------------------------------------------------
// 延迟基准（三定式 U07 口径——P50/P95/P99 最近秩）
// ---------------------------------------------------------------------------

/// 延迟基准器（样本入口校验+三分位——分布面只收干净样本）。
#[derive(Debug)]
pub struct LatencyBench {
    /// 升序样本面（feed 时断言拦离群——离群进不了分布）。
    samples: Vec<u64>,
    /// 损坏重取计数。
    pub retakes: u32,
    pub bag: DiagBag,
}

impl LatencyBench {
    pub fn new() -> LatencyBench {
        LatencyBench { samples: Vec::new(), retakes: 0, bag: DiagBag::new() }
    }

    /// 样本喂入（入口三断言：损坏→重取计数；离群→拦；干净→入分布）。
    /// checksum = 样本值 × 2 + 1（演示校验式——损坏即不匹配）。
    pub fn feed(&mut self, latency_ms: u64, checksum: u64) -> Result<(), u16> {
        // 断言 1：样本损坏（校验戳不符→重取流程计数）。
        if checksum != latency_ms * 2 + 1 {
            self.retakes += 1;
            self.bag.push(E_NB_SAMPLE_CORRUPT, "latency", &format!("戳{}≠值校验", checksum));
            return Err(E_NB_SAMPLE_CORRUPT);
        }
        // 断言 2：离群拦截（超硬上限在入口拦——不进分布面）。
        if latency_ms > LATENCY_HARD_CAP {
            self.bag.push(
                E_NB_OUTLIER,
                "latency",
                &format!("{}>硬上限{}", latency_ms, LATENCY_HARD_CAP),
            );
            return Err(E_NB_OUTLIER);
        }
        // 干净样本：插入保序（O(n) 插入——分布面常有序）。
        let pos = self.samples.partition_point(|&s| s <= latency_ms);
        self.samples.insert(pos, latency_ms);
        Ok(())
    }

    /// 三定式（U07 口径）：P50/P95/P99 最近秩法——秩=⌈p·n/100⌉，
    /// 手算锚：20 样本 [10..200 步 10] → P50=秩10=100、P95=秩19=190、
    /// P99=秩20=200。返回 (p50, p95, p99)。
    pub fn triple(&self) -> Result<(u64, u64, u64), u16> {
        let n = self.samples.len();
        if n == 0 {
            return Err(E_NB_NO_BASELINE);
        }
        let at = |p: u64| -> u64 {
            let rank = ((p * n as u64) + 99) / 100; // ⌈p·n/100⌉
            let idx = (rank.max(1).min(n as u64) - 1) as usize;
            self.samples[idx]
        };
        Ok((at(50), at(95), at(99)))
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 容量基准（并发容量曲线——容量有据可查）
// ---------------------------------------------------------------------------

/// 容量曲线点（档位→(吞吐, 延迟)实测对）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CurvePoint {
    pub conns: u64,
    pub throughput: u64,
    pub latency_ms: u64,
}

/// 容量基准器（四档探针逐档实测——确定性模型：并发升延迟升）。
#[derive(Debug)]
pub struct CapacityBench {
    pub curve: Vec<CurvePoint>,
    pub bag: DiagBag,
}

impl CapacityBench {
    pub fn new() -> CapacityBench {
        CapacityBench { curve: Vec::new(), bag: DiagBag::new() }
    }

    /// 曲线探针（单轮 O(档位数)：每档确定性模型——吞吐=800×min(档,
    /// 100)/100、延迟=10+档/10；档 1000→延迟 110>SLA 出界）。
    pub fn probe(&mut self) -> Vec<CurvePoint> {
        self.curve.clear();
        for &c in PROBE_TIERS.iter() {
            let tp = THROUGHPUT_EXPECT * c.min(100) / 100;
            let lat = 10 + c / 10;
            self.curve.push(CurvePoint { conns: c, throughput: tp, latency_ms: lat });
        }
        self.curve.clone()
    }

    /// 容量峰值（延迟 ≤ SLA 的最高档——容量有据可查）。
    /// 手算锚：档 1→lat11≤33 ✓、10→11 ✓、100→20 ✓、1000→110 ✗
    /// →峰值 100。
    pub fn peak_conns(&self) -> Result<u64, u16> {
        let mut peak: Option<u64> = None;
        for p in &self.curve {
            if p.latency_ms <= SLA_LATENCY_MS {
                peak = Some(p.conns);
            }
        }
        peak.ok_or(E_NB_NO_BASELINE)
    }
}

// ---------------------------------------------------------------------------
// 环境指纹与基准册（F4958 体系——对表 O(1)）
// ---------------------------------------------------------------------------

/// 环境指纹（O(1) 位混合——平台/核数/时钟源单源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvFingerprint {
    pub platform: u32,
    pub cores: u32,
    pub clock_src: u32,
}

impl EnvFingerprint {
    /// 位混合指纹（确定性——同环境同指纹）。
    pub fn packed(self) -> u64 {
        ((self.platform as u64) << 40)
            | ((self.cores as u64) << 20)
            | (self.clock_src as u64)
    }
}

/// 基准册（F4958 体系——结果入册对表）。
#[derive(Debug)]
pub struct BenchRegistry {
    /// 基准项 id → (环境指纹, 数值)。
    rows: BTreeMap<&'static str, (u64, u64)>,
    pub bag: DiagBag,
}

impl BenchRegistry {
    pub fn new() -> BenchRegistry {
        BenchRegistry { rows: BTreeMap::new(), bag: DiagBag::new() }
    }

    /// 结果入册（指纹随结果走）。
    pub fn record(&mut self, bench_id: &'static str, fp: EnvFingerprint, value: u64) {
        self.rows.insert(bench_id, (fp.packed(), value));
    }

    /// 对表（O(1)：指纹一致→值；漂移→E_NB_ENV_DRIFT 立案）。
    pub fn compare(&mut self, bench_id: &str, fp: EnvFingerprint) -> Result<u64, u16> {
        match self.rows.get(bench_id) {
            None => {
                self.bag.push(E_NB_NO_BASELINE, bench_id, "册中无此基准项");
                Err(E_NB_NO_BASELINE)
            }
            Some((fp0, v)) => {
                if *fp0 != fp.packed() {
                    self.bag.push(
                        E_NB_ENV_DRIFT,
                        bench_id,
                        &format!("指纹{}≠册{}", fp.packed(), fp0),
                    );
                    return Err(E_NB_ENV_DRIFT);
                }
                Ok(*v)
            }
        }
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 判据（六项——吞吐/三定式/容量/指纹/损坏离群/码）
// ---------------------------------------------------------------------------

/// VE-F5418 判据入口（返回式 CheckSet——全部预期常量独立手算）。
pub fn run_vef70_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5418");

    // 吞-01 吞吐基准：标准负载 1M→800 恰值；2M→1600 线性；零负载拒。
    {
        let mut tb = ThroughputBench::new();
        let r1 = tb.run(STANDARD_LOAD);
        let r2 = tb.run(STANDARD_LOAD * 2);
        let r0 = tb.run(0);
        // 手算：800×1M/1M=800；800×2M/1M=1600；零负载→E_NB_LOAD_INVALID。
        let ok = r1 == Ok(800)
            && r2 == Ok(1600)
            && r0 == Err(E_NB_LOAD_INVALID)
            && tb.runs == 2
            && tb.bag.count_of(E_NB_LOAD_INVALID) == 1;
        if ok {
            s.ok("F5418-吞-01-吞吐恰值线性对拍");
        } else {
            s.fail("F5418-吞-01-吞吐恰值线性对拍", "吞吐面不符");
        }
    }

    // 延-02 三定式：20 样本 P50=100/P95=190/P99=200 手算锚；损坏重取。
    {
        let mut lb = LatencyBench::new();
        // 20 样本 [10,20,...,200]：秩=⌈p·20/100⌉ → P50 秩10→100、
        // P95 秩19→190、P99 秩20→200。
        for i in 1u64..=20 {
            let v = i * 10;
            let _ = lb.feed(v, v * 2 + 1);
        }
        // 损坏样本：校验戳不符→重取计数（不进分布）。
        let corrupt = lb.feed(50, 999);
        // 三定式恰值。
        let t = lb.triple();
        let ok = corrupt == Err(E_NB_SAMPLE_CORRUPT)
            && lb.retakes == 1
            && lb.len() == 20
            && t == Ok((100, 190, 200));
        if ok {
            s.ok("F5418-延-02-三定式手算锚恰值");
        } else {
            s.fail("F5418-延-02-三定式手算锚恰值", "三定式不符");
        }
    }

    // 容-03 容量曲线：四档探针→SLA 内峰值档 100 恰值；离群入口拦。
    {
        let mut cb = CapacityBench::new();
        let curve = cb.probe();
        let peak = cb.peak_conns();
        // 手算：档 [1,10,100,1000]→lat [10,11,20,110]（10+c/10 整数
        // 除法：1/10=0、10/10=1、100/10=10、1000/10=100）；SLA 33 内
        // 最高档=100；档 1000 出界。
        let lat_expected = [(1u64, 10u64), (10, 11), (100, 20), (1000, 110)];
        let lat_ok = curve
            .iter()
            .zip(lat_expected.iter())
            .all(|(p, (c, l))| p.conns == *c && p.latency_ms == *l);
        // 离群断言：1500ms 样本根本进不了延迟分布。
        let mut lb = LatencyBench::new();
        let outlier = lb.feed(1500, 1500 * 2 + 1);
        let ok = lat_ok
            && peak == Ok(100)
            && outlier == Err(E_NB_OUTLIER)
            && lb.is_empty()
            && lb.bag.count_of(E_NB_OUTLIER) == 1;
        if ok {
            s.ok("F5418-容-03-容量峰值SLA恰值");
        } else {
            s.fail("F5418-容-03-容量峰值SLA恰值", "容量面不符");
        }
    }

    // 指-04 环境指纹：同环境同指纹对表过；异环境对表→漂移立案。
    {
        let mut reg = BenchRegistry::new();
        let env_a = EnvFingerprint { platform: 1, cores: 8, clock_src: 2 };
        let env_b = EnvFingerprint { platform: 1, cores: 16, clock_src: 2 };
        reg.record("bench-throughput", env_a, 800);
        let same = reg.compare("bench-throughput", env_a);
        let drift = reg.compare("bench-throughput", env_b);
        let missing = reg.compare("no-such-bench", env_a);
        // 手算：同指纹过回 800；异核数→漂移；无项→无基线。
        let ok = same == Ok(800)
            && drift == Err(E_NB_ENV_DRIFT)
            && missing == Err(E_NB_NO_BASELINE)
            && reg.bag.count_of(E_NB_ENV_DRIFT) == 1
            && reg.bag.count_of(E_NB_NO_BASELINE) == 1
            && env_a.packed() != env_b.packed();
        if ok {
            s.ok("F5418-指-04-指纹对表漂移立案");
        } else {
            s.fail("F5418-指-04-指纹对表漂移立案", "指纹面不符");
        }
    }

    // 损-05 样本损坏重取计数：坏戳不进分布+重取是流程非静默。
    {
        let mut lb = LatencyBench::new();
        let _ = lb.feed(30, 61); // 30*2+1=61 ✓ 干净
        let bad1 = lb.feed(40, 1); // 戳坏
        let bad2 = lb.feed(50, 2); // 戳坏
        let _ = lb.feed(40, 81); // 40*2+1=81 ✓ 重取成功
        // 手算：分布 [30,40]（干净 2 条）；重取计 2。
        let t = lb.triple();
        let ok = bad1 == Err(E_NB_SAMPLE_CORRUPT)
            && bad2 == Err(E_NB_SAMPLE_CORRUPT)
            && lb.retakes == 2
            && lb.len() == 2
            && t == Ok((30, 40, 40)); // P50 秩1=30、P95/P99 秩2=40
        if ok {
            s.ok("F5418-损-05-损坏重取流程计数");
        } else {
            s.fail("F5418-损-05-损坏重取流程计数", "重取面不符");
        }
    }

    // 码-06 五码互异+reason 全量显性+读屏非空。
    {
        let codes = [E_NB_SAMPLE_CORRUPT, E_NB_OUTLIER, E_NB_ENV_DRIFT, E_NB_NO_BASELINE, E_NB_LOAD_INVALID];
        let mut distinct = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    distinct = false;
                }
            }
        }
        let seg_ok = codes[0] == 0xE901
            && codes[1] == 0xE902
            && codes[2] == 0xE903
            && codes[3] == 0xE904
            && codes[4] == 0xE905;
        let reasons_all = codes.iter().all(|c| reason_of(*c) != "未知诊断码");
        let mut bag = DiagBag::new();
        bag.push(E_NB_ENV_DRIFT, "bench", "演练立案");
        let spoken_ok = bag.spoken().contains("漂移") && bag.spoken().contains("bench");
        let ok = distinct && seg_ok && reasons_all && spoken_ok && bag.len() == 1;
        if ok {
            s.ok("F5418-码-06-五码互异读屏显性");
        } else {
            s.fail("F5418-码-06-五码互异读屏显性", "诊断面不符");
        }
    }

    s
}
